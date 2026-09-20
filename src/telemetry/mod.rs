//! Optional telemetry providers and rendering-independent time-series support.

use std::{
    collections::VecDeque,
    time::{Duration, SystemTime},
};

use serde::Serialize;

use crate::{
    domain::Telemetry,
    ssh::{CancellationToken, ControlMaster, ExecutionOptions, OpenSsh, RemoteInvocation},
};

pub mod nvidia;
pub mod system;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Metric<T> {
    Available { value: T },
    Unsupported,
    Unavailable,
}

impl<T> Metric<T> {
    pub fn available(value: T) -> Self {
        Self::Available { value }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderState {
    Available,
    Unsupported,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ProviderSnapshot<T> {
    Available { data: T },
    Unsupported,
    Unavailable { error: String },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TelemetrySnapshot {
    pub collected_at_unix_ms: u64,
    pub system: ProviderSnapshot<system::SystemMetrics>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nvidia: Option<ProviderSnapshot<nvidia::NvidiaMetrics>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteTelemetryOutput {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl RemoteTelemetryOutput {
    pub fn success(stdout: impl Into<String>) -> Self {
        Self {
            exit_code: Some(0),
            stdout: stdout.into(),
            stderr: String::new(),
        }
    }

    pub fn is_success(&self) -> bool {
        self.exit_code == Some(0)
    }
}

pub trait RemoteTelemetryExecutor: Sync {
    fn run(
        &self,
        target: &str,
        shell: &str,
        command: &str,
        cancellation: &CancellationToken,
    ) -> Result<RemoteTelemetryOutput, String>;
}

#[derive(Debug, Clone)]
pub struct SshTelemetryExecutor {
    ssh: OpenSsh,
}

impl SshTelemetryExecutor {
    pub fn new(ssh: OpenSsh) -> Self {
        Self { ssh }
    }

    pub fn system() -> Result<Self, String> {
        OpenSsh::system()
            .map(Self::new)
            .map_err(|error| error.to_string())
    }
}

impl RemoteTelemetryExecutor for SshTelemetryExecutor {
    fn run(
        &self,
        target: &str,
        shell: &str,
        command: &str,
        cancellation: &CancellationToken,
    ) -> Result<RemoteTelemetryOutput, String> {
        let output = self
            .ssh
            .execute(
                target,
                &RemoteInvocation::command(command, shell),
                ExecutionOptions {
                    cancellation: cancellation.clone(),
                    ..ExecutionOptions::default()
                },
            )
            .map_err(|error| error.to_string())?;
        Ok(RemoteTelemetryOutput {
            exit_code: output.exit_code,
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

impl RemoteTelemetryExecutor for ControlMaster {
    fn run(
        &self,
        _target: &str,
        shell: &str,
        command: &str,
        cancellation: &CancellationToken,
    ) -> Result<RemoteTelemetryOutput, String> {
        let output = self
            .execute(
                &RemoteInvocation::command(command, shell),
                ExecutionOptions {
                    cancellation: cancellation.clone(),
                    ..ExecutionOptions::default()
                },
            )
            .map_err(|error| error.to_string())?;
        Ok(RemoteTelemetryOutput {
            exit_code: output.exit_code,
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

pub struct TelemetryCollector<'a> {
    remote: &'a dyn RemoteTelemetryExecutor,
}

impl<'a> TelemetryCollector<'a> {
    pub fn new(remote: &'a dyn RemoteTelemetryExecutor) -> Self {
        Self { remote }
    }

    pub fn collect(
        &self,
        target: &str,
        shell: &str,
        provider: Telemetry,
        cancellation: &CancellationToken,
    ) -> TelemetrySnapshot {
        if cancellation.is_cancelled() {
            return TelemetrySnapshot {
                collected_at_unix_ms: unix_ms_now(),
                system: ProviderSnapshot::Unavailable {
                    error: "telemetry collection cancelled".to_owned(),
                },
                nvidia: matches!(provider, Telemetry::Nvidia).then(|| {
                    ProviderSnapshot::Unavailable {
                        error: "telemetry collection cancelled".to_owned(),
                    }
                }),
            };
        }
        let system = self.collect_system(target, shell, cancellation);
        let nvidia = match provider {
            Telemetry::System => None,
            Telemetry::Nvidia => Some(self.collect_nvidia(target, shell, cancellation)),
        };
        TelemetrySnapshot {
            collected_at_unix_ms: unix_ms_now(),
            system,
            nvidia,
        }
    }

    fn collect_system(
        &self,
        target: &str,
        shell: &str,
        cancellation: &CancellationToken,
    ) -> ProviderSnapshot<system::SystemMetrics> {
        match self
            .remote
            .run(target, shell, system::LINUX_SYSTEM_COMMAND, cancellation)
        {
            Ok(output) if output.is_success() => match system::parse_linux_system(&output.stdout) {
                Ok(data) => ProviderSnapshot::Available { data },
                Err(error) => ProviderSnapshot::Unavailable {
                    error: error.to_string(),
                },
            },
            Ok(output) => ProviderSnapshot::Unavailable {
                error: remote_failure(&output),
            },
            Err(error) => ProviderSnapshot::Unavailable { error },
        }
    }

    fn collect_nvidia(
        &self,
        target: &str,
        shell: &str,
        cancellation: &CancellationToken,
    ) -> ProviderSnapshot<nvidia::NvidiaMetrics> {
        let help = match self
            .remote
            .run(target, shell, "nvidia-smi --help-query-gpu", cancellation)
        {
            Ok(output) if output.is_success() => output.stdout,
            Ok(output) if output.exit_code == Some(127) => return ProviderSnapshot::Unsupported,
            Ok(output) => {
                return ProviderSnapshot::Unavailable {
                    error: remote_failure(&output),
                };
            }
            Err(error) => return ProviderSnapshot::Unavailable { error },
        };
        let supported = nvidia::parse_nvidia_help(&help);
        let fields = nvidia::query_fields(&supported);
        if fields.is_empty() {
            return ProviderSnapshot::Unsupported;
        }
        let identity = self.collect_identity(target, shell, cancellation);
        let query = format!(
            "nvidia-smi --query-gpu={} --format=csv,noheader,nounits",
            fields.join(",")
        );
        match self.remote.run(target, shell, &query, cancellation) {
            Ok(output) if output.is_success() => {
                match nvidia::parse_nvidia_query(&supported, &output.stdout, &identity) {
                    Ok(data) => match data.state {
                        ProviderState::Available => ProviderSnapshot::Available { data },
                        ProviderState::Unsupported => ProviderSnapshot::Unsupported,
                        ProviderState::Unavailable => ProviderSnapshot::Unavailable {
                            error: "nvidia-smi reported no GPU data".to_owned(),
                        },
                    },
                    Err(error) => ProviderSnapshot::Unavailable {
                        error: error.to_string(),
                    },
                }
            }
            Ok(output) => ProviderSnapshot::Unavailable {
                error: remote_failure(&output),
            },
            Err(error) => ProviderSnapshot::Unavailable { error },
        }
    }

    fn collect_identity(
        &self,
        target: &str,
        shell: &str,
        cancellation: &CancellationToken,
    ) -> nvidia::PlatformIdentity {
        const COMMAND: &str = "\
cat /sys/devices/virtual/dmi/id/product_name 2>/dev/null || true; \
cat /proc/device-tree/model 2>/dev/null || true";
        let Ok(output) = self.remote.run(target, shell, COMMAND, cancellation) else {
            return nvidia::PlatformIdentity::default();
        };
        let mut lines = output
            .stdout
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty());
        nvidia::PlatformIdentity {
            product_name: lines.next().map(str::to_owned),
            device_tree_model: lines.next().map(str::to_owned),
        }
    }
}

pub fn unix_ms_now() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn remote_failure(output: &RemoteTelemetryOutput) -> String {
    let detail = output.stderr.trim();
    if detail.is_empty() {
        format!(
            "remote telemetry command exited with {:?}",
            output.exit_code
        )
    } else {
        detail.to_owned()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TelemetrySample<T> {
    pub collected_at: SystemTime,
    pub value: T,
}

#[derive(Debug, Clone)]
pub struct TelemetryHistory<T> {
    capacity: usize,
    samples: VecDeque<TelemetrySample<T>>,
}

impl<T> TelemetryHistory<T> {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            samples: VecDeque::with_capacity(capacity),
        }
    }

    pub fn push(&mut self, collected_at: SystemTime, value: T) {
        if self.capacity == 0 {
            return;
        }
        if self.samples.len() == self.capacity {
            self.samples.pop_front();
        }
        self.samples.push_back(TelemetrySample {
            collected_at,
            value,
        });
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = &TelemetrySample<T>> {
        self.samples.iter()
    }
}

#[derive(Debug, Clone)]
pub struct PollSchedule {
    interval: Duration,
    next_due: SystemTime,
}

impl PollSchedule {
    pub fn new(interval: Duration, first_due: SystemTime) -> Self {
        assert!(!interval.is_zero(), "poll interval must be non-zero");
        Self {
            interval,
            next_due: first_due,
        }
    }

    pub fn run_if_due(
        &mut self,
        now: SystemTime,
        cancellation: &CancellationToken,
        poll: impl FnOnce(),
    ) -> bool {
        if cancellation.is_cancelled() || now < self.next_due {
            return false;
        }
        poll();
        self.next_due = now + self.interval;
        true
    }
}
