//! Shell completion generation.

use std::process::ExitCode;

use anyhow::Result;
use clap::CommandFactory;
use clap_complete::generate;

use crate::cli::Cli;

pub fn run(shell: clap_complete::Shell) -> Result<ExitCode> {
    let mut cmd = Cli::command();
    let bin = "tastile";
    generate(shell, &mut cmd, bin, &mut std::io::stdout());
    Ok(ExitCode::SUCCESS)
}
