use std::{process::exit};

use clap::Parser;
use owo_colors::OwoColorize;

use iguazu_cli::Cli;

fn main() {
    env_logger::init();
    let cli = Cli::parse();
    if let Err(e) = cli.main() {
        eprintln!("{} {e}", "Error:".red().bold());
        exit(1);
    }
}
