//! Safety planning and lifecycle execution shared by every frontend.

use std::{
    fmt,
    io::{self, Write},
    num::NonZeroUsize,
    thread,
    time::Duration,
    time::Instant,
};

use serde::Serialize;

use crate::{
    config::Config,
    domain::{Device, PowerProvider as ConfiguredPowerProvider},
    power::{
        OutletCommand, PowerCapabilities, PowerProvider,
        shelly::{ReqwestHttpClient, ShellyProvider},
        wol::{SystemUdpSender, WolProvider},
    },
    ssh::{ExecutionOptions, OpenSsh, ProcessOutput, RemoteInvocation},
    status::{ConcurrentExecutor, ExitStatus},
    target::{Target, TargetError},
};

const DEFAULT_TRANSITION_TIMEOUT: Duration = Duration::from_secs(120);
const SSH_PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const PROBE_INTERVAL: Duration = Duration::from_secs(1);
const SHELLY_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LifecycleOperation {
    On,
    Off,
    Shutdown,
    Reboot,
    PowerOff,
    PowerCycle,
}

impl LifecycleOperation {
    const fn requires_force(self) -> bool {
        matches!(self, Self::PowerOff | Self::PowerCycle)
    }
}

impl fmt::Display for LifecycleOperation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(operation_name(*self))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifecycleRequest {
    pub operation: LifecycleOperation,
    pub target: String,
    pub confirmed: bool,
    pub force: bool,
    pub wait: bool,
    pub parallelism: NonZeroUsize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PlannedStep {
    RequestPowerOn,
    WaitForSsh,
    GracefulShutdown,
    WaitForShutdown,
    Reboot,
    CutPhysicalPower,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceLifecyclePlan {
    pub device: String,
    pub steps: Vec<PlannedStep>,
    timeout: Duration,
    error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifecyclePlan {
    pub operation: LifecycleOperation,
    pub target: String,
    pub devices: Vec<DeviceLifecyclePlan>,
    pub parallelism: NonZeroUsize,
    confirmed: bool,
    confirmation_required: bool,
}

impl LifecyclePlan {
    pub fn confirmation_device_names(&self) -> Option<Vec<String>> {
        (self.confirmation_required && !self.confirmed).then(|| {
            self.devices
                .iter()
                .map(|plan| plan.device.clone())
                .collect()
        })
    }

    pub fn confirm(&mut self) {
        self.confirmed = true;
    }
}

pub const fn configured_capabilities(device: &Device) -> PowerCapabilities {
    match device.power {
        Some(ConfiguredPowerProvider::Shelly { .. }) => PowerCapabilities {
            can_request_power_on: true,
            can_cut_physical_power: true,
        },
        Some(ConfiguredPowerProvider::Wol { .. }) => PowerCapabilities {
            can_request_power_on: true,
            can_cut_physical_power: false,
        },
        None => PowerCapabilities {
            can_request_power_on: false,
            can_cut_physical_power: false,
        },
    }
}

pub fn plan_lifecycle(
    config: &Config,
    request: LifecycleRequest,
) -> Result<LifecyclePlan, LifecycleError> {
    if request.wait && request.operation != LifecycleOperation::On {
        return Err(LifecycleError::InvalidOptions(
            "`--wait` is supported only by `on`".to_owned(),
        ));
    }
    if request.force
        && !matches!(
            request.operation,
            LifecycleOperation::Off | LifecycleOperation::PowerOff | LifecycleOperation::PowerCycle
        )
    {
        return Err(LifecycleError::InvalidOptions(format!(
            "`--force` is not supported by `{}`",
            operation_name(request.operation)
        )));
    }
    if request.operation.requires_force() && !request.force {
        return Err(LifecycleError::ForceRequired(request.operation));
    }

    let device_names = Target::resolve(&request.target, config)?;
    let confirmation_required = request.force || !config.devices().contains_key(&request.target);
    let multiple_devices = device_names.len() > 1;
    let mut devices = Vec::with_capacity(device_names.len());
    for device_name in device_names {
        let device = &config.devices()[&device_name];
        let capabilities = configured_capabilities(device);
        let (steps, error) = match planned_steps(&device_name, device, capabilities, &request) {
            Ok(steps) => (steps, None),
            Err(error) if multiple_devices => (Vec::new(), Some(error.to_string())),
            Err(error) => return Err(error),
        };
        let timeout = device
            .shutdown
            .as_ref()
            .and_then(|shutdown| shutdown.timeout)
            .map_or(DEFAULT_TRANSITION_TIMEOUT, |duration| duration.0);
        devices.push(DeviceLifecyclePlan {
            device: device_name,
            steps,
            timeout,
            error,
        });
    }
    devices.sort_by(|left, right| left.device.cmp(&right.device));

    Ok(LifecyclePlan {
        operation: request.operation,
        target: request.target,
        devices,
        parallelism: request.parallelism,
        confirmed: request.confirmed,
        confirmation_required,
    })
}

fn planned_steps(
    device_name: &str,
    device: &Device,
    capabilities: PowerCapabilities,
    request: &LifecycleRequest,
) -> Result<Vec<PlannedStep>, LifecycleError> {
    let unsupported = |capability: &str| LifecycleError::Unsupported {
        device: device_name.to_owned(),
        operation: request.operation,
        reason: capability.to_owned(),
    };
    match request.operation {
        LifecycleOperation::On => {
            if !capabilities.can_request_power_on {
                return Err(unsupported("no power-on provider is configured"));
            }
            let mut steps = vec![PlannedStep::RequestPowerOn];
            if request.wait {
                steps.push(PlannedStep::WaitForSsh);
            }
            Ok(steps)
        }
        LifecycleOperation::Shutdown => Ok(vec![PlannedStep::GracefulShutdown]),
        LifecycleOperation::Reboot => Ok(vec![PlannedStep::Reboot]),
        LifecycleOperation::Off if request.force => {
            if !capabilities.can_cut_physical_power {
                return Err(unsupported(
                    "the configured provider cannot cut physical power",
                ));
            }
            Ok(vec![PlannedStep::CutPhysicalPower])
        }
        LifecycleOperation::Off => {
            let mut steps = vec![PlannedStep::GracefulShutdown, PlannedStep::WaitForShutdown];
            if device
                .shutdown
                .as_ref()
                .is_some_and(|shutdown| shutdown.cut_power)
                && capabilities.can_cut_physical_power
            {
                steps.push(PlannedStep::CutPhysicalPower);
            }
            Ok(steps)
        }
        LifecycleOperation::PowerOff => {
            if !capabilities.can_cut_physical_power {
                return Err(unsupported(
                    "the configured provider cannot cut physical power",
                ));
            }
            Ok(vec![PlannedStep::CutPhysicalPower])
        }
        LifecycleOperation::PowerCycle => {
            if !capabilities.can_cut_physical_power || !capabilities.can_request_power_on {
                return Err(unsupported(
                    "the configured provider cannot cut and restore physical power",
                ));
            }
            Ok(vec![
                PlannedStep::CutPhysicalPower,
                PlannedStep::RequestPowerOn,
            ])
        }
    }
}

pub trait LifecycleExecutor: Sync {
    fn execute_step(
        &self,
        device_name: &str,
        device: &Device,
        step: PlannedStep,
        timeout: Duration,
    ) -> Result<(), String>;
}

#[derive(Debug, Clone)]
pub struct SystemLifecycleExecutor {
    ssh: Option<OpenSsh>,
}

impl SystemLifecycleExecutor {
    pub fn new(ssh: OpenSsh) -> Self {
        Self { ssh: Some(ssh) }
    }

    pub const fn system() -> Self {
        Self { ssh: None }
    }

    fn power_provider(device: &Device) -> Result<Box<dyn PowerProvider>, LifecycleSystemError> {
        match &device.power {
            Some(ConfiguredPowerProvider::Shelly { .. }) => {
                ShellyProvider::<ReqwestHttpClient>::from_device(device, SHELLY_TIMEOUT)
                    .map(|provider| Box::new(provider) as Box<dyn PowerProvider>)
                    .map_err(|error| LifecycleSystemError(error.to_string()))
            }
            Some(ConfiguredPowerProvider::Wol { .. }) => {
                WolProvider::<SystemUdpSender>::from_device(device)
                    .map(|provider| Box::new(provider) as Box<dyn PowerProvider>)
                    .map_err(|error| LifecycleSystemError(error.to_string()))
            }
            None => Err(LifecycleSystemError(
                "device has no configured power provider".to_owned(),
            )),
        }
    }

    fn remote(
        &self,
        device: &Device,
        command: &str,
        timeout: Duration,
    ) -> Result<ProcessOutput, LifecycleSystemError> {
        let discovered;
        let ssh = if let Some(ssh) = &self.ssh {
            ssh
        } else {
            discovered =
                OpenSsh::system().map_err(|error| LifecycleSystemError(error.to_string()))?;
            &discovered
        };
        ssh.execute(
            &device.ssh,
            &RemoteInvocation::command(command, &device.shell),
            ExecutionOptions {
                timeout: Some(timeout),
                ..ExecutionOptions::default()
            },
        )
        .map_err(|error| LifecycleSystemError(error.to_string()))
    }

    fn require_remote_success(
        &self,
        device: &Device,
        command: &str,
        timeout: Duration,
    ) -> Result<(), LifecycleSystemError> {
        let output = self.remote(device, command, timeout)?;
        if output.success() {
            Ok(())
        } else if output.timed_out {
            Err(LifecycleSystemError("SSH command timed out".to_owned()))
        } else {
            Err(LifecycleSystemError(format!(
                "SSH command failed{}",
                output
                    .exit_code
                    .map_or_else(String::new, |code| format!(" with exit code {code}"))
            )))
        }
    }

    fn wait_for_ssh(
        &self,
        device: &Device,
        timeout: Duration,
        ready: bool,
    ) -> Result<(), LifecycleSystemError> {
        let started = Instant::now();
        loop {
            let remaining = timeout.saturating_sub(started.elapsed());
            let probe_timeout = remaining.min(SSH_PROBE_TIMEOUT);
            let reachable = !probe_timeout.is_zero()
                && self
                    .remote(device, "true", probe_timeout)
                    .is_ok_and(|output| output.success());
            if reachable == ready {
                return Ok(());
            }
            if started.elapsed() >= timeout {
                return Err(LifecycleSystemError(format!(
                    "timed out after {} waiting for SSH to become {}",
                    humantime::format_duration(timeout),
                    if ready { "ready" } else { "unreachable" }
                )));
            }
            thread::sleep(PROBE_INTERVAL.min(timeout.saturating_sub(started.elapsed())));
        }
    }
}

impl LifecycleExecutor for SystemLifecycleExecutor {
    fn execute_step(
        &self,
        _device_name: &str,
        device: &Device,
        step: PlannedStep,
        timeout: Duration,
    ) -> Result<(), String> {
        let result = match step {
            PlannedStep::RequestPowerOn => Self::power_provider(device).and_then(|provider| {
                provider
                    .request_on()
                    .map_err(|error| LifecycleSystemError(error.to_string()))
            }),
            PlannedStep::WaitForSsh => self.wait_for_ssh(device, timeout, true),
            PlannedStep::GracefulShutdown => self.require_remote_success(
                device,
                device
                    .shutdown
                    .as_ref()
                    .map_or("sudo shutdown -h now", |shutdown| &shutdown.command),
                SSH_PROBE_TIMEOUT,
            ),
            PlannedStep::WaitForShutdown => self.wait_for_ssh(device, timeout, false),
            PlannedStep::Reboot => {
                self.require_remote_success(device, "sudo shutdown -r now", SSH_PROBE_TIMEOUT)
            }
            PlannedStep::CutPhysicalPower => Self::power_provider(device).and_then(|provider| {
                provider
                    .set_outlet(OutletCommand::Off)
                    .map(|_| ())
                    .map_err(|error| LifecycleSystemError(error.to_string()))
            }),
        };
        result.map_err(|error| error.0)
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct LifecycleSystemError(String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleState {
    Booting,
    SshReady,
    ShuttingDown,
    Rebooting,
    PoweredOff,
    Unreachable,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleResultStatus {
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeviceLifecycleResult {
    pub device: String,
    pub site: Option<String>,
    pub ssh: String,
    pub status: LifecycleResultStatus,
    pub state: LifecycleState,
    pub physical_power_cut: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LifecycleSummary {
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LifecycleReport {
    pub schema_version: u8,
    pub operation: LifecycleOperation,
    pub target: String,
    pub summary: LifecycleSummary,
    pub devices: Vec<DeviceLifecycleResult>,
}

impl LifecycleReport {
    pub fn exit_status(&self) -> ExitStatus {
        match (self.summary.succeeded, self.summary.failed) {
            (_, 0) => ExitStatus::Success,
            (0, _) => ExitStatus::Failed,
            _ => ExitStatus::PartialSuccess,
        }
    }
}

pub fn execute_lifecycle(
    config: &Config,
    plan: LifecyclePlan,
    executor: &dyn LifecycleExecutor,
) -> Result<LifecycleReport, LifecycleError> {
    if let Some(devices) = plan.confirmation_device_names() {
        return Err(LifecycleError::ConfirmationRequired(devices));
    }
    let operation = plan.operation;
    let target = plan.target.clone();
    let mut results = ConcurrentExecutor::new(plan.parallelism).run(&plan.devices, |device_plan| {
        let device = &config.devices()[&device_plan.device];
        execute_device(operation, device_plan, device, executor)
    });
    results.sort_by(|left, right| left.device.cmp(&right.device));
    let failed = results
        .iter()
        .filter(|result| result.status == LifecycleResultStatus::Failed)
        .count();
    Ok(LifecycleReport {
        schema_version: 1,
        operation,
        target,
        summary: LifecycleSummary {
            total: results.len(),
            succeeded: results.len() - failed,
            failed,
        },
        devices: results,
    })
}

fn execute_device(
    operation: LifecycleOperation,
    plan: &DeviceLifecyclePlan,
    device: &Device,
    executor: &dyn LifecycleExecutor,
) -> DeviceLifecycleResult {
    if let Some(error) = &plan.error {
        return DeviceLifecycleResult {
            device: plan.device.clone(),
            site: device.site.clone(),
            ssh: device.ssh.clone(),
            status: LifecycleResultStatus::Failed,
            state: LifecycleState::Unknown,
            physical_power_cut: false,
            error: Some(error.clone()),
        };
    }
    let mut state = initial_state(operation);
    let mut physical_power_cut = false;
    for step in &plan.steps {
        if let Err(error) = executor.execute_step(&plan.device, device, *step, plan.timeout) {
            return DeviceLifecycleResult {
                device: plan.device.clone(),
                site: device.site.clone(),
                ssh: device.ssh.clone(),
                status: LifecycleResultStatus::Failed,
                state: LifecycleState::Unknown,
                physical_power_cut,
                error: Some(error),
            };
        }
        match step {
            PlannedStep::RequestPowerOn => state = LifecycleState::Booting,
            PlannedStep::WaitForSsh => state = LifecycleState::SshReady,
            PlannedStep::GracefulShutdown => state = LifecycleState::ShuttingDown,
            PlannedStep::WaitForShutdown => state = LifecycleState::Unreachable,
            PlannedStep::Reboot => state = LifecycleState::Rebooting,
            PlannedStep::CutPhysicalPower => {
                state = LifecycleState::PoweredOff;
                physical_power_cut = true;
            }
        }
    }
    DeviceLifecycleResult {
        device: plan.device.clone(),
        site: device.site.clone(),
        ssh: device.ssh.clone(),
        status: LifecycleResultStatus::Succeeded,
        state,
        physical_power_cut,
        error: None,
    }
}

const fn initial_state(operation: LifecycleOperation) -> LifecycleState {
    match operation {
        LifecycleOperation::On => LifecycleState::Booting,
        LifecycleOperation::Off | LifecycleOperation::Shutdown => LifecycleState::ShuttingDown,
        LifecycleOperation::Reboot => LifecycleState::Rebooting,
        LifecycleOperation::PowerOff => LifecycleState::PoweredOff,
        LifecycleOperation::PowerCycle => LifecycleState::Booting,
    }
}

pub fn write_human(output: &mut dyn Write, report: &LifecycleReport) -> io::Result<()> {
    for result in &report.devices {
        writeln!(
            output,
            "{}: {}{}",
            result.device,
            state_name(result.state),
            result
                .error
                .as_deref()
                .map_or_else(String::new, |error| format!(" ({error})"))
        )?;
    }
    writeln!(
        output,
        "{} succeeded, {} failed",
        report.summary.succeeded, report.summary.failed
    )
}

pub fn write_json(output: &mut dyn Write, report: &LifecycleReport) -> io::Result<()> {
    serde_json::to_writer_pretty(&mut *output, report).map_err(io::Error::other)?;
    writeln!(output)
}

const fn state_name(state: LifecycleState) -> &'static str {
    match state {
        LifecycleState::Booting => "booting",
        LifecycleState::SshReady => "ssh-ready",
        LifecycleState::ShuttingDown => "shutting-down",
        LifecycleState::Rebooting => "rebooting",
        LifecycleState::PoweredOff => "powered-off",
        LifecycleState::Unreachable => "unreachable",
        LifecycleState::Unknown => "unknown",
    }
}

const fn operation_name(operation: LifecycleOperation) -> &'static str {
    match operation {
        LifecycleOperation::On => "on",
        LifecycleOperation::Off => "off",
        LifecycleOperation::Shutdown => "shutdown",
        LifecycleOperation::Reboot => "reboot",
        LifecycleOperation::PowerOff => "power-off",
        LifecycleOperation::PowerCycle => "power-cycle",
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LifecycleError {
    #[error(transparent)]
    Target(#[from] TargetError),
    #[error("`{0}` requires `--force`; `--yes` only confirms the operation")]
    ForceRequired(LifecycleOperation),
    #[error("device `{device}` cannot perform `{operation:?}`: {reason}")]
    Unsupported {
        device: String,
        operation: LifecycleOperation,
        reason: String,
    },
    #[error("{0}")]
    InvalidOptions(String),
    #[error("confirmation required for devices: {}", .0.join(", "))]
    ConfirmationRequired(Vec<String>),
}
