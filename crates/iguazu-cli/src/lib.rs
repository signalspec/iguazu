use clap::{Parser, Subcommand};

pub mod info;
pub mod convert;
pub mod schema;
#[cfg(feature = "capture")]
pub mod capture;

#[derive(Parser)]
#[command(name = "iguazu", author, version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    Info(info::Cli),
    Schema(schema::Cli),
    Convert(convert::Cli),
    #[cfg(feature = "capture")]
    Capture(capture::Cli),
}

impl Cli {
    pub fn main(&self) -> Result<(), String> {
        match &self.command {
            Commands::Info(args) => info::main(args),
            Commands::Convert(args) => convert::main(args),
            Commands::Schema(args) => schema::main(args),
            #[cfg(feature = "capture")]
            Commands::Capture(args) => capture::main(args),
        }
    }
}
