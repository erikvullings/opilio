//! Terminal user interface entry point.

use std::io::{self, Write};

/// Starts the terminal user interface.
pub fn run() -> io::Result<()> {
    writeln!(
        io::stdout().lock(),
        "Opilio TUI is not implemented yet. Run `opilio --help` for available commands."
    )
}
