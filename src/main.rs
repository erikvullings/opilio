use std::process::ExitCode;

use clap::Parser;
use opilio::cli::Cli;

fn main() -> ExitCode {
    match opilio::run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("opilio: {error}");
            ExitCode::from(error.exit_code())
        }
    }
}
