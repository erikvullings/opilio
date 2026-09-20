//! Application use cases shared by CLI, TUI, and scheduled invocations.

use std::io::{self, Write};

use crate::{
    cli::{Cli, Command, ConfigCommand, ListCommand},
    config::{Config, ConfigError, config_path},
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
    let Some(command) = cli.command else {
        tui::run().map_err(AppError::Io)?;
        return Ok(ExitStatus::Success);
    };
    let path = config_path(cli.config)?;

    match command {
        Command::Status {
            target,
            json,
            quiet,
            parallel,
        } => {
            let report = collect_status(
                &Config::load(&path)?,
                StatusRequest {
                    target: target.unwrap_or_else(|| "all".to_owned()),
                    parallelism: parallel,
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
            command: ListCommand::List,
        } => {
            write_names(output, Config::load(&path)?.actions().keys())?;
            Ok(ExitStatus::Success)
        }
    }
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
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
}

impl AppError {
    pub const fn exit_code(&self) -> u8 {
        match self {
            Self::Config(_) | Self::Status(_) => ExitStatus::ConfigOrUsage.code(),
            Self::Io(_) => 1,
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
