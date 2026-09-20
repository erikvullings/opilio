//! Application use cases shared by CLI, TUI, and scheduled invocations.

use std::io::{self, Write};

use crate::{
    cli::{Cli, Command, ConfigCommand, ListCommand},
    config::{Config, ConfigError, config_path},
    tui,
};

/// Runs an Opilio invocation.
pub fn run(cli: Cli) -> Result<(), AppError> {
    execute(cli, &mut io::stdout().lock())
}

/// Executes a parsed CLI invocation against shared application APIs.
pub fn execute(cli: Cli, output: &mut dyn Write) -> Result<(), AppError> {
    let Some(command) = cli.command else {
        return tui::run().map_err(AppError::Io);
    };
    let path = config_path(cli.config)?;

    match command {
        Command::Config {
            command: ConfigCommand::Path,
        } => writeln!(output, "{}", path.display()).map_err(AppError::Io),
        Command::Config {
            command: ConfigCommand::Check,
        } => {
            Config::load(&path)?;
            writeln!(output, "configuration is valid: {}", path.display()).map_err(AppError::Io)
        }
        Command::Device {
            command: ListCommand::List,
        } => write_names(output, Config::load(&path)?.devices().keys()),
        Command::Group {
            command: ListCommand::List,
        } => write_names(output, Config::load(&path)?.groups().keys()),
        Command::Site {
            command: ListCommand::List,
        } => write_names(output, Config::load(&path)?.sites().keys()),
        Command::Action {
            command: ListCommand::List,
        } => write_names(output, Config::load(&path)?.actions().keys()),
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
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
}

impl AppError {
    pub const fn exit_code(&self) -> u8 {
        match self {
            Self::Config(_) => 2,
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
