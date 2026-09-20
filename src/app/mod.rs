//! Application use cases shared by CLI, TUI, and scheduled invocations.

use std::io;

use crate::{cli::Cli, tui};

/// Runs an Opilio invocation.
pub fn run(_cli: Cli) -> io::Result<()> {
    tui::run()
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
