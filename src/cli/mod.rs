//! Command-line parsing.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// Agentless management for a small flock of machines.
#[derive(Debug, Parser)]
#[command(name = "opilio", version, about)]
pub struct Cli {
    /// Use this configuration file instead of OPILIO_CONFIG or the platform default.
    #[arg(long, global = true, value_name = "PATH")]
    pub config: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Inspect and validate configuration.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Inspect configured devices.
    Device {
        #[command(subcommand)]
        command: ListCommand,
    },
    /// Inspect configured groups.
    Group {
        #[command(subcommand)]
        command: ListCommand,
    },
    /// Inspect configured sites.
    Site {
        #[command(subcommand)]
        command: ListCommand,
    },
    /// Inspect configured actions.
    Action {
        #[command(subcommand)]
        command: ListCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Print the selected configuration path.
    Path,
    /// Parse and statically validate configuration without making network calls.
    Check,
}

#[derive(Debug, Subcommand)]
pub enum ListCommand {
    /// List configured names.
    #[command(name = "ls")]
    List,
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    #[test]
    fn command_definition_is_valid() {
        Cli::command().debug_assert();
    }
}
