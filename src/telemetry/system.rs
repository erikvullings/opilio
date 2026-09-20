//! Generic Linux system telemetry parsing.

use serde::Serialize;

use super::Metric;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LoadAverage {
    pub one: Metric<f64>,
    pub five: Metric<f64>,
    pub fifteen: Metric<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SystemMemory {
    pub total_bytes: Metric<u64>,
    pub available_bytes: Metric<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SystemMetrics {
    pub cpu_busy_percent: Metric<f64>,
    pub logical_cpus: Metric<u32>,
    pub load: LoadAverage,
    pub memory: SystemMemory,
    pub uptime_seconds: Metric<u64>,
}

pub const LINUX_SYSTEM_COMMAND: &str = "\
printf '%s\\n' '--- loadavg ---'; cat /proc/loadavg; \
printf '%s\\n' '--- meminfo ---'; cat /proc/meminfo; \
printf '%s\\n' '--- uptime ---'; cat /proc/uptime; \
printf '%s\\n' '--- stat-before ---'; cat /proc/stat; sleep 0.1; \
printf '%s\\n' '--- stat-after ---'; cat /proc/stat";

pub fn parse_linux_system(output: &str) -> Result<SystemMetrics, SystemParseError> {
    let sections = Sections::parse(output);
    if sections.present == 0 {
        return Err(SystemParseError::MissingSections);
    }

    let load_values = sections
        .loadavg
        .and_then(|value| value.lines().next())
        .map(|line| line.split_whitespace().collect::<Vec<_>>())
        .unwrap_or_default();
    let load = LoadAverage {
        one: parse_float(load_values.first().copied()),
        five: parse_float(load_values.get(1).copied()),
        fifteen: parse_float(load_values.get(2).copied()),
    };

    let total_kib = meminfo_value(sections.meminfo, "MemTotal");
    let available_kib = meminfo_value(sections.meminfo, "MemAvailable");
    let uptime_seconds = sections
        .uptime
        .and_then(|value| value.split_whitespace().next())
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value >= 0.0)
        .map(|value| Metric::available(value.floor() as u64))
        .unwrap_or(Metric::Unavailable);
    let (cpu_busy_percent, logical_cpus) =
        parse_cpu_stat(sections.stat_before, sections.stat_after);

    Ok(SystemMetrics {
        cpu_busy_percent,
        logical_cpus,
        load,
        memory: SystemMemory {
            total_bytes: kib_to_bytes(total_kib),
            available_bytes: kib_to_bytes(available_kib),
        },
        uptime_seconds,
    })
}

fn parse_float(value: Option<&str>) -> Metric<f64> {
    value
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite())
        .map(Metric::available)
        .unwrap_or(Metric::Unavailable)
}

fn meminfo_value(section: Option<&str>, key: &str) -> Option<u64> {
    section?.lines().find_map(|line| {
        let (candidate, value) = line.split_once(':')?;
        (candidate == key)
            .then(|| value.split_whitespace().next()?.parse().ok())
            .flatten()
    })
}

fn kib_to_bytes(value: Option<u64>) -> Metric<u64> {
    value
        .and_then(|value| value.checked_mul(1024))
        .map(Metric::available)
        .unwrap_or(Metric::Unavailable)
}

fn parse_cpu_stat(before: Option<&str>, after: Option<&str>) -> (Metric<f64>, Metric<u32>) {
    let Some(after) = after else {
        return (Metric::Unavailable, Metric::Unavailable);
    };
    let logical_count = after
        .lines()
        .filter(|line| {
            line.strip_prefix("cpu")
                .and_then(|suffix| suffix.split_whitespace().next())
                .is_some_and(|index| {
                    line.as_bytes().get(3).is_some_and(u8::is_ascii_digit)
                        && index.chars().all(|c| c.is_ascii_digit())
                })
        })
        .count();
    let logical_cpus = u32::try_from(logical_count)
        .ok()
        .filter(|count| *count > 0)
        .map(Metric::available)
        .unwrap_or(Metric::Unavailable);
    let busy = before
        .and_then(cpu_totals)
        .zip(cpu_totals(after))
        .and_then(|((before_total, before_idle), (after_total, after_idle))| {
            let total = after_total.checked_sub(before_total)?;
            let idle = after_idle.checked_sub(before_idle)?;
            (total > 0 && idle <= total).then(|| (total - idle) as f64 * 100.0 / total as f64)
        })
        .map(Metric::available)
        .unwrap_or(Metric::Unavailable);
    (busy, logical_cpus)
}

fn cpu_totals(section: &str) -> Option<(u64, u64)> {
    let values = section
        .lines()
        .find(|line| line.starts_with("cpu "))?
        .split_whitespace()
        .skip(1)
        .map(str::parse::<u64>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    if values.len() < 4 {
        return None;
    }
    let total = values
        .iter()
        .try_fold(0_u64, |sum, value| sum.checked_add(*value))?;
    let idle = values[3].checked_add(values.get(4).copied().unwrap_or(0))?;
    Some((total, idle))
}

#[derive(Default)]
struct Sections<'a> {
    loadavg: Option<&'a str>,
    meminfo: Option<&'a str>,
    uptime: Option<&'a str>,
    stat_before: Option<&'a str>,
    stat_after: Option<&'a str>,
    present: usize,
}

impl<'a> Sections<'a> {
    fn parse(output: &'a str) -> Self {
        let mut result = Self::default();
        let mut current = None;
        let mut start = 0;
        for (offset, line) in line_offsets(output) {
            if let Some(name) = line
                .strip_prefix("--- ")
                .and_then(|line| line.strip_suffix(" ---"))
            {
                if let Some(previous) = current {
                    result.set(previous, &output[start..offset]);
                }
                current = Some(name);
                start = offset + line.len() + 1;
            }
        }
        if let Some(previous) = current {
            result.set(previous, &output[start..]);
        }
        result
    }

    fn set(&mut self, name: &str, value: &'a str) {
        let destination = match name {
            "loadavg" => &mut self.loadavg,
            "meminfo" => &mut self.meminfo,
            "uptime" => &mut self.uptime,
            "stat-before" => &mut self.stat_before,
            "stat-after" => &mut self.stat_after,
            _ => return,
        };
        *destination = Some(value.trim());
        self.present += 1;
    }
}

fn line_offsets(input: &str) -> impl Iterator<Item = (usize, &str)> {
    input.split_inclusive('\n').scan(0, |offset, line| {
        let current = *offset;
        *offset += line.len();
        Some((current, line.trim_end_matches('\n').trim_end_matches('\r')))
    })
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SystemParseError {
    #[error("system telemetry output contains no recognized sections")]
    MissingSections,
}
