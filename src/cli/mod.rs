//! Command-line parsing.

use std::{num::NonZeroUsize, path::PathBuf};

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
    /// Open an interactive system OpenSSH session to exactly one device.
    Ssh {
        /// Configured Opilio device name.
        device: String,
    },
    /// Show configured status for a device, group, site, or all devices.
    Status {
        /// Device, group, site, or `all`; defaults to all devices.
        target: Option<String>,
        /// Emit the stable JSON representation.
        #[arg(long, conflicts_with = "quiet")]
        json: bool,
        /// Suppress status output; use the exit code only.
        #[arg(long, conflicts_with = "json")]
        quiet: bool,
        /// Maximum number of devices processed concurrently.
        #[arg(long, default_value = "4", value_name = "N")]
        parallel: NonZeroUsize,
    },
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
