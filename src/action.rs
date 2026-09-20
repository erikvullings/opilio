//! Named action execution and stable per-device results.

use std::{
    io::{self, Write},
    num::NonZeroUsize,
};

use serde::Serialize;

use crate::{
    config::{Config, ConfigError},
    ssh::{ExecutionOptions, OpenSsh, ProcessOutput, RemoteInvocation},
    status::{ConcurrentExecutor, ExitStatus},
    target::TargetError,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionRequest {
    pub name: String,
    pub target: String,
    pub parallelism: NonZeroUsize,
}

impl ActionRequest {
    pub fn new(name: impl Into<String>, target: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            target: target.into(),
            parallelism: NonZeroUsize::MIN,
        }
    }
}

pub trait ActionExecutor: Sync {
    fn execute(
        &self,
        ssh_target: &str,
        invocation: &RemoteInvocation,
        options: ExecutionOptions,
    ) -> Result<ProcessOutput, String>;
}

impl ActionExecutor for OpenSsh {
    fn execute(
        &self,
        ssh_target: &str,
        invocation: &RemoteInvocation,
        options: ExecutionOptions,
    ) -> Result<ProcessOutput, String> {
        OpenSsh::execute(self, ssh_target, invocation, options).map_err(|error| error.to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionState {
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeviceActionResult {
    pub device: String,
    pub site: Option<String>,
    pub ssh: String,
    pub status: ActionState,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub cancelled: bool,
    pub stdout: String,
    pub stderr: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ActionSummary {
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ActionReport {
    pub schema_version: u8,
    pub action: String,
    pub target: String,
    pub summary: ActionSummary,
    pub devices: Vec<DeviceActionResult>,
}

impl ActionReport {
    pub fn exit_status(&self) -> ExitStatus {
        match (self.summary.succeeded, self.summary.failed) {
            (_, 0) => ExitStatus::Success,
            (0, _) => ExitStatus::Failed,
            _ => ExitStatus::PartialSuccess,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ActionListReport {
    pub schema_version: u8,
    pub actions: Vec<String>,
}

pub fn list_actions(config: &Config) -> ActionListReport {
    ActionListReport {
        schema_version: 1,
        actions: config.actions().keys().cloned().collect(),
    }
}

pub fn run_action(
    config: &Config,
    request: ActionRequest,
    executor: &dyn ActionExecutor,
) -> Result<ActionReport, ActionError> {
    if !config.actions().contains_key(&request.name) {
        return Err(ActionError::UnknownAction(request.name));
    }
    let device_names = crate::target::Target::resolve(&request.target, config)?;
    let devices = device_names
        .iter()
        .map(|name| {
            let device = &config.devices()[name];
            let implementation = config.resolve_action(&request.name, name)?;
            let invocation = match (implementation.command(), implementation.exec()) {
                (Some(command), None) => RemoteInvocation::command(command, &device.shell),
                (None, Some(exec)) => {
                    RemoteInvocation::exec(&exec.program, exec.args.iter().map(String::as_str))
                }
                _ => unreachable!("validated actions have exactly one implementation"),
            };
            let invocation = if let Some(cwd) = implementation.cwd() {
                invocation.with_cwd(cwd)
            } else {
                invocation
            };
            Ok((name.as_str(), device, invocation, implementation.timeout()))
        })
        .collect::<Result<Vec<_>, ConfigError>>()?;

    let mut results = ConcurrentExecutor::new(request.parallelism).run(
        &devices,
        |(name, device, invocation, timeout)| {
            let output = executor.execute(
                &device.ssh,
                invocation,
                ExecutionOptions {
                    timeout: *timeout,
                    ..ExecutionOptions::default()
                },
            );
            match output {
                Ok(output) => result_from_output(name, device, output),
                Err(error) => DeviceActionResult {
                    device: (*name).to_owned(),
                    site: device.site.clone(),
                    ssh: device.ssh.clone(),
                    status: ActionState::Failed,
                    exit_code: None,
                    timed_out: false,
                    cancelled: false,
                    stdout: String::new(),
                    stderr: String::new(),
                    error: Some(error),
                },
            }
        },
    );
    results.sort_by(|left, right| left.device.cmp(&right.device));
    let failed = results
        .iter()
        .filter(|result| result.status == ActionState::Failed)
        .count();

    Ok(ActionReport {
        schema_version: 1,
        action: request.name,
        target: request.target,
        summary: ActionSummary {
            total: results.len(),
            succeeded: results.len() - failed,
            failed,
        },
        devices: results,
    })
}

fn result_from_output(
    name: &&str,
    device: &&crate::domain::Device,
    output: ProcessOutput,
) -> DeviceActionResult {
    let succeeded = output.success();
    let error = (!succeeded).then(|| {
        if output.timed_out {
            "action timed out".to_owned()
        } else if output.cancelled {
            "action was cancelled".to_owned()
        } else if let Some(exit_code) = output.exit_code {
            format!("remote action exited with code {exit_code}")
        } else {
            "remote action exited without an exit code".to_owned()
        }
    });
    DeviceActionResult {
        device: (*name).to_owned(),
        site: device.site.clone(),
        ssh: device.ssh.clone(),
        status: if succeeded {
            ActionState::Succeeded
        } else {
            ActionState::Failed
        },
        exit_code: output.exit_code,
        timed_out: output.timed_out,
        cancelled: output.cancelled,
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        error,
    }
}

pub fn write_list_human(output: &mut dyn Write, report: &ActionListReport) -> io::Result<()> {
    for action in &report.actions {
        writeln!(output, "{action}")?;
    }
    Ok(())
}

pub fn write_human(output: &mut dyn Write, report: &ActionReport) -> io::Result<()> {
    for result in &report.devices {
        writeln!(
            output,
            "{}: {}{}",
            result.device,
            match result.status {
                ActionState::Succeeded => "succeeded",
                ActionState::Failed => "failed",
            },
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

pub fn write_json<T: Serialize>(output: &mut dyn Write, report: &T) -> io::Result<()> {
    serde_json::to_writer_pretty(&mut *output, report).map_err(io::Error::other)?;
    writeln!(output)
}

#[derive(Debug, thiserror::Error)]
pub enum ActionError {
    #[error("unknown action `{0}`")]
    UnknownAction(String),
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Target(#[from] TargetError),
}
