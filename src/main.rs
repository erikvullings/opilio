use std::process::ExitCode;

use clap::Parser;
use opilio::cli::Cli;

fn main() -> ExitCode {
    match opilio::run(Cli::parse()) {
        Ok(status) => ExitCode::from(status.code()),
        Err(error) => {
            eprintln!("opilio: {error}");
            ExitCode::from(error.exit_code())
        }
    }
}
