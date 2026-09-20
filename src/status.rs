//! Stable status results and orchestration shared by terminal frontends.

use std::{
    io::{self, Write},
    num::NonZeroUsize,
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
};

use serde::Serialize;

use crate::{
    config::Config,
    domain::Device,
    ssh::CancellationToken,
    target::TargetError,
    telemetry::{SshTelemetryExecutor, TelemetryCollector, TelemetrySnapshot},
};

const DEFAULT_STATUS_PARALLELISM: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusRequest {
    pub target: String,
    pub parallelism: NonZeroUsize,
}

impl Default for StatusRequest {
    fn default() -> Self {
        Self {
            target: "all".to_owned(),
            parallelism: NonZeroUsize::new(DEFAULT_STATUS_PARALLELISM).expect("non-zero constant"),
        }
    }
}

pub trait StatusSource: Sync {
    fn status(&self, device_name: &str, device: &Device) -> Result<StatusState, String>;

    fn telemetry(&self, _device_name: &str, _device: &Device) -> Option<TelemetrySnapshot> {
        None
    }
}

#[derive(Debug, Default)]
pub struct ConfiguredStatusSource;

impl StatusSource for ConfiguredStatusSource {
    fn status(&self, _device_name: &str, _device: &Device) -> Result<StatusState, String> {
        Ok(StatusState::Configured)
    }
}

#[derive(Debug, Default)]
pub struct RuntimeStatusSource;

impl StatusSource for RuntimeStatusSource {
    fn status(&self, _device_name: &str, _device: &Device) -> Result<StatusState, String> {
        Ok(StatusState::Configured)
    }

    fn telemetry(&self, _device_name: &str, device: &Device) -> Option<TelemetrySnapshot> {
        let provider = device.telemetry.clone()?;
        match SshTelemetryExecutor::system() {
            Ok(remote) => Some(TelemetryCollector::new(&remote).collect(
                &device.ssh,
                &device.shell,
                provider,
                &CancellationToken::new(),
            )),
            Err(error) => Some(TelemetrySnapshot {
                collected_at_unix_ms: crate::telemetry::unix_ms_now(),
                system: crate::telemetry::ProviderSnapshot::Unavailable {
                    error: error.clone(),
                },
                nvidia: matches!(provider, crate::domain::Telemetry::Nvidia)
                    .then_some(crate::telemetry::ProviderSnapshot::Unavailable { error }),
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusState {
    Configured,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DeviceStatusResult {
    pub device: String,
    pub site: Option<String>,
    pub ssh: String,
    pub status: StatusState,
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub telemetry: Option<TelemetrySnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StatusSummary {
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StatusReport {
    pub schema_version: u8,
    pub target: String,
    pub summary: StatusSummary,
    pub devices: Vec<DeviceStatusResult>,
}

impl StatusReport {
    pub fn exit_status(&self) -> ExitStatus {
        match (self.summary.succeeded, self.summary.failed) {
            (_, 0) => ExitStatus::Success,
            (0, _) => ExitStatus::Failed,
            _ => ExitStatus::PartialSuccess,
        }
    }
}

pub fn write_human(output: &mut dyn Write, report: &StatusReport) -> io::Result<()> {
    if report.devices.len() == 1 {
        let result = &report.devices[0];
        writeln!(output, "Device: {}", result.device)?;
        writeln!(output, "Site: {}", value_or_dash(result.site.as_deref()))?;
        writeln!(output, "SSH: {}", result.ssh)?;
        writeln!(output, "Status: {}", status_name(result.status))?;
        writeln!(output, "Detail: {}", value_or_dash(result.error.as_deref()))?;
        return Ok(());
    }

    let rows = report
        .devices
        .iter()
        .map(|result| {
            [
                result.device.as_str(),
                value_or_dash(result.site.as_deref()),
                result.ssh.as_str(),
                status_name(result.status),
                value_or_dash(result.error.as_deref()),
            ]
        })
        .collect::<Vec<_>>();
    let headers = ["DEVICE", "SITE", "SSH", "STATUS", "DETAIL"];
    let widths = std::array::from_fn::<_, 5, _>(|column| {
        rows.iter()
            .map(|row| row[column].len())
            .max()
            .unwrap_or(0)
            .max(headers[column].len())
    });
    write_table_row(output, headers, widths)?;
    for row in rows {
        write_table_row(output, row, widths)?;
    }
    writeln!(output)?;
    writeln!(
        output,
        "{} succeeded, {} failed",
        report.summary.succeeded, report.summary.failed
    )
}

pub fn write_json(output: &mut dyn Write, report: &StatusReport) -> io::Result<()> {
    serde_json::to_writer_pretty(&mut *output, report).map_err(io::Error::other)?;
    writeln!(output)
}

fn write_table_row(
    output: &mut dyn Write,
    values: [&str; 5],
    widths: [usize; 5],
) -> io::Result<()> {
    writeln!(
        output,
        "{:<device_width$}  {:<site_width$}  {:<ssh_width$}  {:<status_width$}  {}",
        values[0],
        values[1],
        values[2],
        values[3],
        values[4],
        device_width = widths[0],
        site_width = widths[1],
        ssh_width = widths[2],
        status_width = widths[3],
    )
}

const fn status_name(status: StatusState) -> &'static str {
    match status {
        StatusState::Configured => "configured",
        StatusState::Failed => "failed",
    }
}

fn value_or_dash(value: Option<&str>) -> &str {
    value.unwrap_or("-")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ExitStatus {
    Success = 0,
    Failed = 1,
    ConfigOrUsage = 2,
    PartialSuccess = 3,
}

impl ExitStatus {
    pub const fn code(self) -> u8 {
        self as u8
    }
}

pub fn collect_status(
    config: &Config,
    request: StatusRequest,
    source: &dyn StatusSource,
) -> Result<StatusReport, StatusError> {
    let device_names = crate::target::Target::resolve(&request.target, config)?;
    let devices = device_names
        .iter()
        .map(|name| (name.as_str(), &config.devices()[name]))
        .collect::<Vec<_>>();
    let executor = ConcurrentExecutor::new(request.parallelism);
    let mut results = executor.run(&devices, |(name, device)| {
        let status = source.status(name, device);
        match status {
            Ok(status) => DeviceStatusResult {
                device: (*name).to_owned(),
                site: device.site.clone(),
                ssh: device.ssh.clone(),
                status,
                error: None,
                telemetry: source.telemetry(name, device),
            },
            Err(error) => DeviceStatusResult {
                device: (*name).to_owned(),
                site: device.site.clone(),
                ssh: device.ssh.clone(),
                status: StatusState::Failed,
                error: Some(error),
                telemetry: None,
            },
        }
    });
    results.sort_by(|left, right| left.device.cmp(&right.device));
    let failed = results
        .iter()
        .filter(|result| result.status == StatusState::Failed)
        .count();

    Ok(StatusReport {
        schema_version: 1,
        target: request.target,
        summary: StatusSummary {
            total: results.len(),
            succeeded: results.len() - failed,
            failed,
        },
        devices: results,
    })
}

#[derive(Debug, thiserror::Error)]
pub enum StatusError {
    #[error(transparent)]
    Target(#[from] TargetError),
}

#[derive(Debug, Clone, Copy)]
pub struct ConcurrentExecutor {
    parallelism: NonZeroUsize,
}

impl ConcurrentExecutor {
    pub const fn new(parallelism: NonZeroUsize) -> Self {
        Self { parallelism }
    }

    pub fn run<T: Sync, R: Send, F: Fn(&T) -> R + Sync>(
        &self,
        items: &[T],
        operation: F,
    ) -> Vec<R> {
        let next = AtomicUsize::new(0);
        let results = Mutex::new(Vec::with_capacity(items.len()));
        let worker_count = self.parallelism.get().min(items.len());

        thread::scope(|scope| {
            for _ in 0..worker_count {
                scope.spawn(|| {
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(item) = items.get(index) else {
                            break;
                        };
                        let result = operation(item);
                        results
                            .lock()
                            .expect("status result lock poisoned")
                            .push((index, result));
                    }
                });
            }
        });

        let mut indexed_results = results.into_inner().expect("status result lock poisoned");
        indexed_results.sort_by_key(|(index, _)| *index);
        indexed_results
            .into_iter()
            .map(|(_, result)| result)
            .collect()
    }
}
