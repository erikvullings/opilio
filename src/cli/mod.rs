//! Command-line parsing.

use clap::Parser;

/// Agentless management for a small flock of machines.
#[derive(Debug, Parser)]
#[command(name = "opilio", version, about)]
pub struct Cli {}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    #[test]
    fn command_definition_is_valid() {
        Cli::command().debug_assert();
    }
}
