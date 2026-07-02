use std::sync::Arc;

use clap::Args;
use iguazu::{cli::{ExportOpts, ImportOpts}, export::EXPORTERS, import::IMPORTERS, storage::{MemoryStorage, Storage}};
use futures_lite::future::{self, block_on};

/// Convert between formats
#[derive(Args)]
pub struct Cli {
    #[clap(flatten)]
    import: ImportOpts,

    #[clap(long, help = "Build default summary of all entities")]
    build_summary: bool,

    #[clap(flatten)]
    export: ExportOpts,
}

pub fn main(args: &Cli) -> Result<(), String> {
    let executor = Arc::new(async_executor::Executor::new());
    let pool = Arc::new(iguazu::storage::Pool::new(executor.clone(), 256 * 1024 * 1024));
    let storage = Arc::new(MemoryStorage) as Arc<dyn Storage>;

    block_on(executor.run(async {
        let (mut entity, import_completion) = args.import.import(IMPORTERS, pool).await?;
        let import_completion = executor.spawn(import_completion);

        if args.build_summary {
            entity.build_summaries(&executor, &storage).await?;
        }

        let export_completion = args.export.export(EXPORTERS, executor.clone(), entity);

        future::try_zip(
            async { import_completion.await.map_err(|s| s.to_string()) },
            async { export_completion.await.map_err(|s| s.to_string()) },
        ).await?;

        Ok(())
    }))
}
