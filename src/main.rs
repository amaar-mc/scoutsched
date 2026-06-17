//! scoutsched command line entry point.
//!
//! Parses arguments, runs the requested action, and maps any error onto a
//! non zero exit code with a clear message on stderr. All real work lives in the
//! library so it is unit testable; this file is a thin shell.

use clap::Parser;
use scoutsched::cli::{run, Cli};
use std::process::ExitCode;

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("scoutsched: {e}");
            ExitCode::FAILURE
        }
    }
}
