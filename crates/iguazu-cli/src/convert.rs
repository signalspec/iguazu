use std::sync::Arc;

use clap::Args;
use iguazu::{cli::{ExportOpts, ImportOpts}, export::EXPORTERS, import::IMPORTERS, io::FsFile};
use futures_lite::future::{self, block_on};

#[derive(Args)]
#[command(about = "Convert between formats")]
pub struct Cli {
    #[clap(flatten)]
    import: ImportOpts,

    #[clap(flatten)]
    export: ExportOpts,
}

pub fn main(args: &Cli) -> Result<(), String> {
    let executor = Arc::new(async_executor::Executor::new());
    block_on(executor.run(async {
        let (entity, import_completion) = args.import.import(IMPORTERS, executor.clone()).await?;
        let export_completion = args.export.export(EXPORTERS, executor.clone(), entity);

        future::try_zip(
            async { import_completion.await.map_err(|s| s.to_string()) },
            async { export_completion.await.map_err(|s| s.to_string()) },
        ).await?;

        Ok(())
    }))
}