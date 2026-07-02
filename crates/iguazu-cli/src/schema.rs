use std::io::Write;

use clap::Args;
use iguazu::{cli::ImportOpts, import::IMPORTERS};
use futures_lite::future::block_on;

/// Dump the schema as JSON
#[derive(Args)]
pub struct Cli {
    #[clap(flatten)]
    import: ImportOpts,
}

pub fn main(args: &Cli) -> Result<(), String> {
    block_on(async {
        let schema = args.import.schema_or_inferred(IMPORTERS).await?;
        let mut stdout = std::io::stdout().lock();
        serde_json::to_writer_pretty(&mut stdout, &schema).map_err(|e| e.to_string())?;
        stdout.write_all(b"\n").map_err(|e| e.to_string())?;
        Ok(())
    })
}
