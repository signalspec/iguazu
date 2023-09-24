use std::process::exit;

use clap::{Parser, Subcommand};
use owo_colors::OwoColorize;

pub mod info;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Info(info::Cli),
}

fn main() {
    let cli = Cli::parse();

    let result = match &cli.command {
        Commands::Info(args) => info::main(args),
    };

    if let Err(e) = result {
        eprintln!("{} {e}", "Error:".red().bold());
        exit(1);
    }
}