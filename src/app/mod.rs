//! Application use cases shared by CLI, TUI, and scheduled invocations.

use std::{
    io::{self, BufRead, Write},
    time::Instant,
};

use crate::{
    action::{
        ActionError, ActionExecutor, ActionRequest, list_actions, run_action,
        write_human as write_action_human, write_json as write_action_json, write_list_human,
    },
    alias::{AliasError, expand_alias},
    cli::{
        ActionCommand, AliasCommand, Cli, Command, ConfigCommand, HistoryCommand, LifecycleCommon,
        ListCommand,
    },
    config::{Config, ConfigError, config_path},
    domain::Operation,
    history::{
        HistoryDetail, HistoryError, HistoryResult, HistoryStore, NewHistoryRecord,
        OperationSource, Redactor, write_detail_human, write_human as write_history_human,
        write_json as write_history_json,
    },
    lifecycle::{
        LifecycleError, LifecycleExecutor, LifecycleOperation, LifecycleRequest,
        SystemLifecycleExecutor, execute_lifecycle, plan_lifecycle,
        write_human as write_lifecycle_human, write_json as write_lifecycle_json,
    },
    ssh::{InteractiveSsh, OpenSsh, SshError},
    status::{
        ConfiguredStatusSource, ExitStatus, RuntimeStatusSource, StatusError, StatusRequest,
        StatusSource, collect_status, write_human, write_json,
    },
    target::TargetError,
    tui,
};

/// Runs an Opilio invocation.
pub fn run(cli: Cli) -> Result<ExitStatus, AppError> {
    let source = cli.source;
    let redactor = config_path(cli.config.clone())
        .ok()
        .and_then(|path| Config::load(&path).ok())
        .map_or_else(Redactor::default, |config| {
            Redactor::new(config.resolved_secret_values())
        });
    let history = HistoryStore::platform_default(redactor)?;
    execute_with_history(cli, &mut io::stdout().lock(), source, &history)
}

/// Executes a parsed CLI invocation against shared application APIs.
pub fn execute(cli: Cli, output: &mut dyn Write) -> Result<ExitStatus, AppError> {
    execute_with_adapters(
        cli,
        output,
        &RuntimeStatusSource,
        None,
        None,
        None,
        None,
        None,
    )
}

/// Executes an invocation and records operations in the supplied history store.
pub fn execute_with_history(
    cli: Cli,
    output: &mut dyn Write,
    source: OperationSource,
    history: &HistoryStore,
) -> Result<ExitStatus, AppError> {
    execute_with_adapters(
        cli,
        output,
        &RuntimeStatusSource,
        None,
        None,
        None,
        None,
        Some(HistoryContext { source, history }),
    )
}

/// Executes a parsed invocation with an injectable status boundary.
pub fn execute_with_status_source(
    cli: Cli,
    output: &mut dyn Write,
    status_source: &dyn StatusSource,
) -> Result<ExitStatus, AppError> {
    execute_with_adapters(cli, output, status_source, None, None, None, None, None)
}

/// Executes a parsed invocation with an injectable interactive OpenSSH boundary.
pub fn execute_with_ssh(
    cli: Cli,
    output: &mut dyn Write,
    ssh: &dyn InteractiveSsh,
) -> Result<ExitStatus, AppError> {
    execute_with_adapters(
        cli,
        output,
        &ConfiguredStatusSource,
        Some(ssh),
        None,
        None,
        None,
        None,
    )
}

/// Executes a parsed invocation with an injectable named-action boundary.
pub fn execute_with_action_executor(
    cli: Cli,
    output: &mut dyn Write,
    executor: &dyn ActionExecutor,
) -> Result<ExitStatus, AppError> {
    execute_with_adapters(
        cli,
        output,
        &ConfiguredStatusSource,
        None,
        Some(executor),
        None,
        None,
        None,
    )
}

/// Executes a named action with a fakeable boundary and persistent history.
pub fn execute_with_action_executor_and_history(
    cli: Cli,
    output: &mut dyn Write,
    executor: &dyn ActionExecutor,
    source: OperationSource,
    history: &HistoryStore,
) -> Result<ExitStatus, AppError> {
    execute_with_adapters(
        cli,
        output,
        &ConfiguredStatusSource,
        None,
        Some(executor),
        None,
        None,
        Some(HistoryContext { source, history }),
    )
}

/// Executes lifecycle commands with fakeable SSH/power/wait and confirmation boundaries.
pub fn execute_with_lifecycle_executor(
    cli: Cli,
    output: &mut dyn Write,
    executor: &dyn LifecycleExecutor,
    confirmation: &dyn Confirmation,
) -> Result<ExitStatus, AppError> {
    execute_with_adapters(
        cli,
        output,
        &ConfiguredStatusSource,
        None,
        None,
        Some(executor),
        Some(confirmation),
        None,
    )
}

#[derive(Clone, Copy)]
struct HistoryContext<'a> {
    source: OperationSource,
    history: &'a HistoryStore,
}

#[allow(clippy::too_many_arguments)]
fn execute_with_adapters(
    cli: Cli,
    output: &mut dyn Write,
    status_source: &dyn StatusSource,
    ssh: Option<&dyn InteractiveSsh>,
    action_executor: Option<&dyn ActionExecutor>,
    lifecycle_executor: Option<&dyn LifecycleExecutor>,
    confirmation: Option<&dyn Confirmation>,
    history: Option<HistoryContext<'_>>,
) -> Result<ExitStatus, AppError> {
    let Some(command) = cli.command else {
        let path = config_path(cli.config)?;
        tui::run(Config::load(&path)?).map_err(AppError::Io)?;
        return Ok(ExitStatus::Success);
    };
    let path = config_path(cli.config)?;

    match command {
        Command::Ssh { device } => {
            let config = Config::load(&path)?;
            let configured_device = config.devices().get(&device).ok_or_else(|| {
                AppError::SshTarget(format!(
                    "`{device}` is not a device; `opilio ssh` rejects groups and sites"
                ))
            })?;
            let started = Instant::now();
            let result = if let Some(ssh) = ssh {
                ssh.interactive(&configured_device.ssh)
            } else {
                OpenSsh::system()?.interactive(&configured_device.ssh)
            };
            let exit_code = match result {
                Ok(exit_code) => exit_code,
                Err(error) => {
                    if let Some(history) = history {
                        history.history.append(NewHistoryRecord {
                            source: history.source,
                            operation: "ssh".to_owned(),
                            action: None,
                            requested_target: device.clone(),
                            resolved_device: device,
                            duration_ms: elapsed_millis(started),
                            result: HistoryResult::Failed,
                            exit_code: None,
                            force: false,
                            stdout: None,
                            stderr: None,
                            error: Some(error.to_string()),
                        })?;
                    }
                    return Err(error.into());
                }
            };
            if let Some(history) = history {
                history.history.append(NewHistoryRecord {
                    source: history.source,
                    operation: "ssh".to_owned(),
                    action: None,
                    requested_target: device.clone(),
                    resolved_device: device,
                    duration_ms: elapsed_millis(started),
                    result: if exit_code == 0 {
                        HistoryResult::Succeeded
                    } else {
                        HistoryResult::Failed
                    },
                    exit_code: Some(exit_code),
                    force: false,
                    stdout: None,
                    stderr: None,
                    error: (exit_code != 0).then(|| format!("interactive SSH exited {exit_code}")),
                })?;
            }
            Ok(if exit_code == 0 {
                ExitStatus::Success
            } else {
                ExitStatus::Failed
            })
        }
        Command::On { common, wait } => execute_lifecycle_command(
            &Config::load(&path)?,
            common,
            LifecycleOperation::On,
            wait,
            false,
            output,
            lifecycle_executor,
            confirmation,
            history,
        ),
        Command::Off { common, force } => execute_lifecycle_command(
            &Config::load(&path)?,
            common,
            LifecycleOperation::Off,
            false,
            force,
            output,
            lifecycle_executor,
            confirmation,
            history,
        ),
        Command::Shutdown { common } => execute_lifecycle_command(
            &Config::load(&path)?,
            common,
            LifecycleOperation::Shutdown,
            false,
            false,
            output,
            lifecycle_executor,
            confirmation,
            history,
        ),
        Command::Reboot { common } => execute_lifecycle_command(
            &Config::load(&path)?,
            common,
            LifecycleOperation::Reboot,
            false,
            false,
            output,
            lifecycle_executor,
            confirmation,
            history,
        ),
        Command::PowerOff { common, force } => execute_lifecycle_command(
            &Config::load(&path)?,
            common,
            LifecycleOperation::PowerOff,
            false,
            force,
            output,
            lifecycle_executor,
            confirmation,
            history,
        ),
        Command::PowerCycle { common, force } => execute_lifecycle_command(
            &Config::load(&path)?,
            common,
            LifecycleOperation::PowerCycle,
            false,
            force,
            output,
            lifecycle_executor,
            confirmation,
            history,
        ),
        Command::Status {
            target,
            json,
            quiet,
            parallel,
        } => execute_status(
            &Config::load(&path)?,
            target.unwrap_or_else(|| "all".to_owned()),
            parallel,
            json,
            quiet,
            output,
            status_source,
            history,
        ),
        Command::Config {
            command: ConfigCommand::Path,
        } => {
            writeln!(output, "{}", path.display())?;
            Ok(ExitStatus::Success)
        }
        Command::Config {
            command: ConfigCommand::Check,
        } => {
            Config::load(&path)?;
            writeln!(output, "configuration is valid: {}", path.display())?;
            Ok(ExitStatus::Success)
        }
        Command::Device {
            command: ListCommand::List,
        } => {
            write_names(output, Config::load(&path)?.devices().keys())?;
            Ok(ExitStatus::Success)
        }
        Command::Group {
            command: ListCommand::List,
        } => {
            write_names(output, Config::load(&path)?.groups().keys())?;
            Ok(ExitStatus::Success)
        }
        Command::Site {
            command: ListCommand::List,
        } => {
            write_names(output, Config::load(&path)?.sites().keys())?;
            Ok(ExitStatus::Success)
        }
        Command::Action {
            command: ActionCommand::List { json, quiet },
        } => {
            let report = list_actions(&Config::load(&path)?);
            if !quiet {
                if json {
                    write_action_json(output, &report)?;
                } else {
                    write_list_human(output, &report)?;
                }
            }
            Ok(ExitStatus::Success)
        }
        Command::Action {
            command:
                ActionCommand::Run {
                    name,
                    target,
                    json,
                    quiet,
                    parallel,
                },
        } => {
            let config = Config::load(&path)?;
            let started = Instant::now();
            let request = ActionRequest {
                name,
                target,
                parallelism: parallel,
            };
            let report = if let Some(executor) = action_executor {
                run_action(&config, request, executor)?
            } else {
                let ssh = OpenSsh::system()?;
                run_action(&config, request, &ssh)?
            };
            if let Some(history) = history {
                record_action_history(history, &report, elapsed_millis(started))?;
            }
            if !quiet {
                if json {
                    write_action_json(output, &report)?;
                } else {
                    write_action_human(output, &report)?;
                }
            }
            Ok(report.exit_status())
        }
        Command::Alias {
            command: AliasCommand::Run { name, json, quiet },
        } => {
            let config = Config::load(&path)?;
            let alias = expand_alias(&config, &name)?;
            match alias.operation {
                Operation::Status => execute_status(
                    &config,
                    alias.target,
                    alias.parallelism,
                    json,
                    quiet,
                    output,
                    status_source,
                    history,
                ),
                operation => {
                    let lifecycle_operation = lifecycle_operation(operation)?;
                    execute_lifecycle_values(
                        &config,
                        lifecycle_operation,
                        alias.target,
                        alias.parallelism,
                        alias.wait,
                        alias.force,
                        false,
                        json,
                        quiet,
                        output,
                        lifecycle_executor,
                        confirmation,
                        history,
                    )
                }
            }
        }
        Command::History {
            target,
            command,
            json,
        } => {
            let owned_history;
            let history = if let Some(history) = history {
                history.history
            } else {
                owned_history = HistoryStore::platform_default(Redactor::default())?;
                &owned_history
            };
            match command {
                Some(HistoryCommand::Show { id }) => {
                    let (record, warnings) = history.show(&id)?;
                    let detail = HistoryDetail {
                        schema_version: 1,
                        record,
                        warnings,
                    };
                    if json {
                        write_history_json(output, &detail)?;
                    } else {
                        write_detail_human(output, &detail)?;
                    }
                }
                None => {
                    let listing = if let Some(target) = target {
                        let config = Config::load(&path)?;
                        let devices = crate::target::Target::resolve(&target, &config)?;
                        history.list_for_target(&target, &devices)?
                    } else {
                        history.list(None)?
                    };
                    if json {
                        write_history_json(output, &listing)?;
                    } else {
                        write_history_human(output, &listing)?;
                    }
                }
            }
            Ok(ExitStatus::Success)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn execute_lifecycle_command(
    config: &Config,
    common: LifecycleCommon,
    operation: LifecycleOperation,
    wait: bool,
    force: bool,
    output: &mut dyn Write,
    executor: Option<&dyn LifecycleExecutor>,
    confirmation: Option<&dyn Confirmation>,
    history: Option<HistoryContext<'_>>,
) -> Result<ExitStatus, AppError> {
    execute_lifecycle_values(
        config,
        operation,
        common.target,
        common.parallel,
        wait,
        force,
        common.yes,
        common.json,
        common.quiet,
        output,
        executor,
        confirmation,
        history,
    )
}

#[allow(clippy::too_many_arguments)]
fn execute_lifecycle_values(
    config: &Config,
    operation: LifecycleOperation,
    target: String,
    parallelism: std::num::NonZeroUsize,
    wait: bool,
    force: bool,
    confirmed: bool,
    json: bool,
    quiet: bool,
    output: &mut dyn Write,
    executor: Option<&dyn LifecycleExecutor>,
    confirmation: Option<&dyn Confirmation>,
    history: Option<HistoryContext<'_>>,
) -> Result<ExitStatus, AppError> {
    let requested_target = target.clone();
    let started = Instant::now();
    let mut plan = plan_lifecycle(
        config,
        LifecycleRequest {
            operation,
            target,
            confirmed,
            force,
            wait,
            parallelism,
        },
    )?;
    if let Some(devices) = plan.confirmation_device_names() {
        let accepted = if let Some(confirmation) = confirmation {
            confirmation
                .confirm(operation, &devices)
                .map_err(AppError::Confirmation)?
        } else {
            StdinConfirmation
                .confirm(operation, &devices)
                .map_err(AppError::Confirmation)?
        };
        if !accepted {
            return Err(AppError::ConfirmationDeclined);
        }
        plan.confirm();
    }

    let system_executor;
    let executor = if let Some(executor) = executor {
        executor
    } else {
        system_executor = SystemLifecycleExecutor::system();
        &system_executor
    };
    let report = execute_lifecycle(config, plan, executor)?;
    if let Some(history) = history {
        record_lifecycle_history(
            history,
            &report,
            &requested_target,
            force,
            elapsed_millis(started),
        )?;
    }
    if !quiet {
        if json {
            write_lifecycle_json(output, &report)?;
        } else {
            write_lifecycle_human(output, &report)?;
        }
    }
    Ok(report.exit_status())
}

fn lifecycle_operation(operation: Operation) -> Result<LifecycleOperation, AppError> {
    match operation {
        Operation::On => Ok(LifecycleOperation::On),
        Operation::Off => Ok(LifecycleOperation::Off),
        Operation::Shutdown => Ok(LifecycleOperation::Shutdown),
        Operation::Reboot => Ok(LifecycleOperation::Reboot),
        Operation::PowerOff => Ok(LifecycleOperation::PowerOff),
        Operation::PowerCycle => Ok(LifecycleOperation::PowerCycle),
        Operation::Status => Err(AppError::Alias(AliasError::Unsupported(Operation::Status))),
    }
}

pub trait Confirmation {
    fn confirm(&self, operation: LifecycleOperation, devices: &[String]) -> Result<bool, String>;
}

#[derive(Debug, Clone, Copy)]
struct StdinConfirmation;

impl Confirmation for StdinConfirmation {
    fn confirm(&self, operation: LifecycleOperation, devices: &[String]) -> Result<bool, String> {
        eprintln!(
            "Confirm {operation} for {} device(s): {}",
            devices.len(),
            devices.join(", ")
        );
        eprint!("Continue? [y/N] ");
        io::stderr().flush().map_err(|error| error.to_string())?;
        let mut answer = String::new();
        io::stdin()
            .lock()
            .read_line(&mut answer)
            .map_err(|error| error.to_string())?;
        Ok(matches!(
            answer.trim().to_ascii_lowercase().as_str(),
            "y" | "yes"
        ))
    }
}

#[allow(clippy::too_many_arguments)]
fn execute_status(
    config: &Config,
    target: String,
    parallelism: std::num::NonZeroUsize,
    json: bool,
    quiet: bool,
    output: &mut dyn Write,
    status_source: &dyn StatusSource,
    history: Option<HistoryContext<'_>>,
) -> Result<ExitStatus, AppError> {
    let started = Instant::now();
    let report = collect_status(
        config,
        StatusRequest {
            target,
            parallelism,
        },
        status_source,
    )?;
    if let Some(history) = history {
        record_status_history(history, &report, elapsed_millis(started))?;
    }
    if !quiet {
        if json {
            write_json(output, &report)?;
        } else {
            write_human(output, &report)?;
        }
    }
    Ok(report.exit_status())
}

fn record_action_history(
    context: HistoryContext<'_>,
    report: &crate::action::ActionReport,
    duration_ms: u64,
) -> Result<(), HistoryError> {
    for result in &report.devices {
        let succeeded = result.status == crate::action::ActionState::Succeeded;
        context.history.append(NewHistoryRecord {
            source: context.source,
            operation: "action".to_owned(),
            action: Some(report.action.clone()),
            requested_target: report.target.clone(),
            resolved_device: result.device.clone(),
            duration_ms,
            result: if succeeded {
                HistoryResult::Succeeded
            } else {
                HistoryResult::Failed
            },
            exit_code: result.exit_code,
            force: false,
            stdout: Some(result.stdout.clone()),
            stderr: Some(result.stderr.clone()),
            error: result.error.clone(),
        })?;
    }
    Ok(())
}

fn record_lifecycle_history(
    context: HistoryContext<'_>,
    report: &crate::lifecycle::LifecycleReport,
    requested_target: &str,
    force: bool,
    duration_ms: u64,
) -> Result<(), HistoryError> {
    for result in &report.devices {
        let succeeded = result.status == crate::lifecycle::LifecycleResultStatus::Succeeded;
        context.history.append(NewHistoryRecord {
            source: context.source,
            operation: report.operation.to_string(),
            action: None,
            requested_target: requested_target.to_owned(),
            resolved_device: result.device.clone(),
            duration_ms,
            result: if succeeded {
                HistoryResult::Succeeded
            } else {
                HistoryResult::Failed
            },
            exit_code: Some(if succeeded { 0 } else { 1 }),
            force,
            stdout: None,
            stderr: None,
            error: result.error.clone(),
        })?;
    }
    Ok(())
}

fn record_status_history(
    context: HistoryContext<'_>,
    report: &crate::status::StatusReport,
    duration_ms: u64,
) -> Result<(), HistoryError> {
    for result in &report.devices {
        let succeeded = result.status != crate::status::StatusState::Failed;
        context.history.append(NewHistoryRecord {
            source: context.source,
            operation: "status".to_owned(),
            action: None,
            requested_target: report.target.clone(),
            resolved_device: result.device.clone(),
            duration_ms,
            result: if succeeded {
                HistoryResult::Succeeded
            } else {
                HistoryResult::Failed
            },
            exit_code: Some(if succeeded { 0 } else { 1 }),
            force: false,
            stdout: None,
            stderr: None,
            error: result.error.clone(),
        })?;
    }
    Ok(())
}

fn elapsed_millis(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn write_names<'a>(
    output: &mut dyn Write,
    names: impl IntoIterator<Item = &'a String>,
) -> Result<(), AppError> {
    for name in names {
        writeln!(output, "{name}")?;
    }
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Status(#[from] StatusError),
    #[error(transparent)]
    Action(#[from] ActionError),
    #[error(transparent)]
    Alias(#[from] AliasError),
    #[error(transparent)]
    Lifecycle(#[from] LifecycleError),
    #[error(transparent)]
    Target(#[from] TargetError),
    #[error(transparent)]
    History(#[from] HistoryError),
    #[error("could not read confirmation: {0}")]
    Confirmation(String),
    #[error("operation cancelled")]
    ConfirmationDeclined,
    #[error(transparent)]
    Ssh(#[from] SshError),
    #[error("invalid SSH target: {0}")]
    SshTarget(String),
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
}

impl AppError {
    pub const fn exit_code(&self) -> u8 {
        match self {
            Self::Config(_)
            | Self::Status(_)
            | Self::Action(_)
            | Self::Alias(_)
            | Self::Lifecycle(_)
            | Self::Target(_)
            | Self::History(HistoryError::NotFound(_) | HistoryError::InvalidConfig(_))
            | Self::SshTarget(_) => ExitStatus::ConfigOrUsage.code(),
            Self::History(_)
            | Self::Ssh(_)
            | Self::Confirmation(_)
            | Self::ConfirmationDeclined
            | Self::Io(_) => 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[test]
    fn accepts_an_invocation_without_arguments() {
        let cli = Cli::try_parse_from(["opilio"]);

        assert!(cli.is_ok());
    }
}
