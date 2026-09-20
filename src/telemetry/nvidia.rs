//! NVIDIA capability discovery and UMA-aware metric parsing.

use std::collections::BTreeSet;

use serde::Serialize;

use super::{Metric, ProviderState};

const QUERY_FIELDS: [&str; 8] = [
    "index",
    "uuid",
    "name",
    "utilization.gpu",
    "temperature.gpu",
    "power.draw",
    "memory.total",
    "memory.used",
];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlatformIdentity {
    pub product_name: Option<String>,
    pub device_tree_model: Option<String>,
}

impl PlatformIdentity {
    pub fn is_unified_memory(&self, gpu_name: Option<&str>) -> bool {
        self.product_name
            .as_deref()
            .into_iter()
            .chain(self.device_tree_model.as_deref())
            .chain(gpu_name)
            .any(|value| {
                let normalized = value.to_ascii_lowercase();
                normalized.contains("dgx spark") || normalized.contains("gb10")
            })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemorySemantics {
    Dedicated,
    Unified,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GpuMemory {
    pub semantics: MemorySemantics,
    pub total_bytes: Metric<u64>,
    pub used_bytes: Metric<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NvidiaGpuMetrics {
    pub index: Option<u32>,
    pub uuid: Option<String>,
    pub name: Option<String>,
    pub utilization_percent: Metric<f64>,
    pub temperature_celsius: Metric<f64>,
    pub power_watts: Metric<f64>,
    pub memory: GpuMemory,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NvidiaMetrics {
    pub state: ProviderState,
    pub gpus: Vec<NvidiaGpuMetrics>,
}

pub fn parse_nvidia_help(output: &str) -> BTreeSet<String> {
    output
        .lines()
        .filter_map(|line| line.trim().strip_prefix('"')?.strip_suffix('"'))
        .map(str::to_owned)
        .collect()
}

pub fn query_fields(supported: &BTreeSet<String>) -> Vec<&'static str> {
    QUERY_FIELDS
        .iter()
        .copied()
        .filter(|field| supported.contains(*field))
        .collect()
}

pub fn parse_nvidia_query(
    supported: &BTreeSet<String>,
    output: &str,
    identity: &PlatformIdentity,
) -> Result<NvidiaMetrics, NvidiaParseError> {
    let fields = query_fields(supported);
    if !fields.contains(&"index") && !fields.contains(&"name") && !fields.contains(&"uuid") {
        return Ok(NvidiaMetrics {
            state: ProviderState::Unsupported,
            gpus: Vec::new(),
        });
    }

    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .trim(csv::Trim::All)
        .from_reader(output.as_bytes());
    let mut gpus = Vec::new();
    for row in reader.records() {
        let row = row.map_err(NvidiaParseError::Csv)?;
        if row.len() != fields.len() {
            return Err(NvidiaParseError::ColumnCount {
                expected: fields.len(),
                actual: row.len(),
            });
        }
        let value = |field: &str| {
            fields
                .iter()
                .position(|candidate| *candidate == field)
                .and_then(|index| row.get(index))
        };
        let name = optional_text(value("name"));
        let unified = identity.is_unified_memory(name.as_deref());
        let semantics = if unified {
            MemorySemantics::Unified
        } else if supported.contains("memory.total") || supported.contains("memory.used") {
            MemorySemantics::Dedicated
        } else {
            MemorySemantics::Unknown
        };
        let memory_metric = |field: &str| {
            if unified || !supported.contains(field) {
                Metric::Unsupported
            } else {
                parse_u64(value(field))
                    .and_then(|mib| mib.checked_mul(1024 * 1024))
                    .map(Metric::available)
                    .unwrap_or(Metric::Unavailable)
            }
        };
        gpus.push(NvidiaGpuMetrics {
            index: parse_u32(value("index")),
            uuid: optional_text(value("uuid")),
            name,
            utilization_percent: numeric_metric(
                supported,
                "utilization.gpu",
                value("utilization.gpu"),
            ),
            temperature_celsius: numeric_metric(
                supported,
                "temperature.gpu",
                value("temperature.gpu"),
            ),
            power_watts: numeric_metric(supported, "power.draw", value("power.draw")),
            memory: GpuMemory {
                semantics,
                total_bytes: memory_metric("memory.total"),
                used_bytes: memory_metric("memory.used"),
            },
        });
    }
    Ok(NvidiaMetrics {
        state: if gpus.is_empty() {
            ProviderState::Unavailable
        } else {
            ProviderState::Available
        },
        gpus,
    })
}

fn numeric_metric(supported: &BTreeSet<String>, field: &str, value: Option<&str>) -> Metric<f64> {
    if !supported.contains(field) {
        return Metric::Unsupported;
    }
    value
        .filter(|value| !is_unavailable(value))
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite())
        .map(Metric::available)
        .unwrap_or(Metric::Unavailable)
}

fn parse_u32(value: Option<&str>) -> Option<u32> {
    value.filter(|value| !is_unavailable(value))?.parse().ok()
}

fn parse_u64(value: Option<&str>) -> Option<u64> {
    value.filter(|value| !is_unavailable(value))?.parse().ok()
}

fn optional_text(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty() && !is_unavailable(value))
        .map(str::to_owned)
}

fn is_unavailable(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "n/a" | "[not supported]" | "not supported" | "unknown"
    )
}

#[derive(Debug, thiserror::Error)]
pub enum NvidiaParseError {
    #[error("invalid NVIDIA CSV: {0}")]
    Csv(#[source] csv::Error),
    #[error("NVIDIA row has {actual} columns; expected {expected}")]
    ColumnCount { expected: usize, actual: usize },
}
