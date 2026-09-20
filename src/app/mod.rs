//! Application use cases shared by CLI, TUI, and scheduled invocations.

use std::io::{self, Write};

use crate::{
    action::{
        ActionError, ActionExecutor, ActionRequest, list_actions, run_action,
        write_human as write_action_human, write_json as write_action_json, write_list_human,
    },
    alias::{AliasError, expand_alias},
    cli::{ActionCommand, AliasCommand, Cli, Command, ConfigCommand, ListCommand},
    config::{Config, ConfigError, config_path},
    domain::Operation,
    ssh::{InteractiveSsh, OpenSsh, SshError},
    status::{
        ConfiguredStatusSource, ExitStatus, StatusError, StatusRequest, StatusSource,
        collect_status, write_human, write_json,
    },
    tui,
};

/// Runs an Opilio invocation.
pub fn run(cli: Cli) -> Result<ExitStatus, AppError> {
    execute(cli, &mut io::stdout().lock())
}

/// Executes a parsed CLI invocation against shared application APIs.
pub fn execute(cli: Cli, output: &mut dyn Write) -> Result<ExitStatus, AppError> {
    execute_with_status_source(cli, output, &ConfiguredStatusSource)
}

/// Executes a parsed invocation with an injectable status boundary.
pub fn execute_with_status_source(
    cli: Cli,
    output: &mut dyn Write,
    status_source: &dyn StatusSource,
) -> Result<ExitStatus, AppError> {
    execute_with_adapters(cli, output, status_source, None, None)
}

/// Executes a parsed invocation with an injectable interactive OpenSSH boundary.
pub fn execute_with_ssh(
    cli: Cli,
    output: &mut dyn Write,
    ssh: &dyn InteractiveSsh,
) -> Result<ExitStatus, AppError> {
    execute_with_adapters(cli, output, &ConfiguredStatusSource, Some(ssh), None)
}

/// Executes a parsed invocation with an injectable named-action boundary.
pub fn execute_with_action_executor(
    cli: Cli,
    output: &mut dyn Write,
    executor: &dyn ActionExecutor,
) -> Result<ExitStatus, AppError> {
    execute_with_adapters(cli, output, &ConfiguredStatusSource, None, Some(executor))
}

fn execute_with_adapters(
    cli: Cli,
    output: &mut dyn Write,
    status_source: &dyn StatusSource,
    ssh: Option<&dyn InteractiveSsh>,
    action_executor: Option<&dyn ActionExecutor>,
) -> Result<ExitStatus, AppError> {
    let Some(command) = cli.command else {
        tui::run().map_err(AppError::Io)?;
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
            let exit_code = if let Some(ssh) = ssh {
                ssh.interactive(&configured_device.ssh)?
            } else {
                OpenSsh::system()?.interactive(&configured_device.ssh)?
            };
            Ok(if exit_code == 0 {
                ExitStatus::Success
            } else {
                ExitStatus::Failed
            })
        }
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
                ),
                operation => Err(AppError::Alias(AliasError::Unsupported(operation))),
            }
        }
    }
}

fn execute_status(
    config: &Config,
    target: String,
    parallelism: std::num::NonZeroUsize,
    json: bool,
    quiet: bool,
    output: &mut dyn Write,
    status_source: &dyn StatusSource,
) -> Result<ExitStatus, AppError> {
    let report = collect_status(
        config,
        StatusRequest {
            target,
            parallelism,
        },
        status_source,
    )?;
    if !quiet {
        if json {
            write_json(output, &report)?;
        } else {
            write_human(output, &report)?;
        }
    }
    Ok(report.exit_status())
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
            | Self::SshTarget(_) => ExitStatus::ConfigOrUsage.code(),
            Self::Ssh(_) | Self::Io(_) => 1,
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
