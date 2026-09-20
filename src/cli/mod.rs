//! Command-line parsing.

use std::{num::NonZeroUsize, path::PathBuf};

use clap::{Args, Parser, Subcommand};

use crate::history::OperationSource;

/// Agentless management for a small flock of machines.
#[derive(Debug, Parser)]
#[command(name = "opilio", version, about)]
pub struct Cli {
    /// Use this configuration file instead of OPILIO_CONFIG or the platform default.
    #[arg(long, global = true, value_name = "PATH")]
    pub config: Option<PathBuf>,

    /// Identify the caller in operation history.
    #[arg(long, global = true, value_enum, default_value = "cli", hide = true)]
    pub source: OperationSource,

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
    /// Request power-on and optionally wait for SSH readiness.
    On {
        #[command(flatten)]
        common: LifecycleCommon,
        /// Wait until SSH is available; does not wait for services or models.
        #[arg(long)]
        wait: bool,
    },
    /// Gracefully shut down, wait, then cut configured Shelly power.
    Off {
        #[command(flatten)]
        common: LifecycleCommon,
        /// Bypass graceful shutdown and authorize an immediate physical cut.
        #[arg(long)]
        force: bool,
    },
    /// Request graceful OS shutdown over SSH.
    Shutdown {
        #[command(flatten)]
        common: LifecycleCommon,
    },
    /// Request an OS reboot over SSH.
    Reboot {
        #[command(flatten)]
        common: LifecycleCommon,
    },
    /// Cut physical power; requires --force.
    PowerOff {
        #[command(flatten)]
        common: LifecycleCommon,
        /// Authorize an immediate physical power cut.
        #[arg(long)]
        force: bool,
    },
    /// Cut and restore physical power; requires --force.
    PowerCycle {
        #[command(flatten)]
        common: LifecycleCommon,
        /// Authorize a physical power cycle.
        #[arg(long)]
        force: bool,
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
        command: ActionCommand,
    },
    /// Run a configured one-operation alias.
    Alias {
        #[command(subcommand)]
        command: AliasCommand,
    },
    /// Inspect persistent operation history.
    History {
        /// Device, group, site, or `all`.
        target: Option<String>,
        #[command(subcommand)]
        command: Option<HistoryCommand>,
        /// Emit the stable JSON representation.
        #[arg(long, global = true)]
        json: bool,
    },
}

#[derive(Debug, Args)]
pub struct LifecycleCommon {
    /// Device, group, site, or `all`.
    pub target: String,
    /// Emit the stable JSON representation.
    #[arg(long, conflicts_with = "quiet")]
    pub json: bool,
    /// Suppress output; use the exit code only.
    #[arg(long, conflicts_with = "json")]
    pub quiet: bool,
    /// Confirm a multi-device operation without an interactive prompt.
    #[arg(long)]
    pub yes: bool,
    /// Maximum number of devices processed concurrently.
    #[arg(long, default_value = "1", value_name = "N")]
    pub parallel: NonZeroUsize,
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

#[derive(Debug, Subcommand)]
pub enum ActionCommand {
    /// List configured action names.
    #[command(name = "ls")]
    List {
        /// Emit the stable JSON representation.
        #[arg(long, conflicts_with = "quiet")]
        json: bool,
        /// Suppress output; use the exit code only.
        #[arg(long, conflicts_with = "json")]
        quiet: bool,
    },
    /// Run a named action on a device, group, site, or all devices.
    Run {
        /// Configured action name.
        name: String,
        /// Device, group, site, or `all`.
        target: String,
        /// Emit the stable JSON representation.
        #[arg(long, conflicts_with = "quiet")]
        json: bool,
        /// Suppress output; use the exit code only.
        #[arg(long, conflicts_with = "json")]
        quiet: bool,
        /// Maximum number of devices processed concurrently.
        #[arg(long, default_value = "1", value_name = "N")]
        parallel: NonZeroUsize,
    },
}

#[derive(Debug, Subcommand)]
pub enum AliasCommand {
    /// Expand and execute an alias exactly once.
    Run {
        /// Configured alias name.
        name: String,
        /// Emit the operation's stable JSON representation.
        #[arg(long, conflicts_with = "quiet")]
        json: bool,
        /// Suppress output; use the exit code only.
        #[arg(long, conflicts_with = "json")]
        quiet: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum HistoryCommand {
    /// Show one history record by its stable ID.
    Show {
        /// History record ID.
        id: String,
    },
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
