use std::{future, path::PathBuf, process::exit, sync::Arc, time::Duration};

use clap::Parser;
use futures_lite::future::block_on;
use iguazu::{cli::ImportOpts, import::IMPORTERS, storage::{MemoryStorage, Storage}};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[clap(flatten)]
    import: ImportOpts,

    /// Output image path
    #[clap(short = 'o', long, value_name = "FILE")]
    output: PathBuf,

    /// Image size
    #[clap(long, value_name = "WIDTHxHEIGHT", default_value = "1024x768", value_parser = parse_size)]
    size: (u32, u32),

    /// Timeout for file to load
    #[clap(long, value_name = "SECONDS", default_value = "10")]
    timeout: u64,
}

fn parse_size(s: &str) -> Result<(u32, u32), String> {
    s.split_once('x').and_then(|(w, h)| {
        Some((w.parse().ok()?, h.parse().ok()?))
    }).ok_or_else(|| format!("invalid size `{s}`, expected WIDTHxHEIGHT"))
}

fn main() {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    let executor = Arc::new(iguazu::send::Executor::new());
    std::thread::spawn({
        let executor = executor.clone();
        move || {
            block_on(executor.run(future::pending::<()>()));
        }
    });

    let pool = Arc::new(iguazu::storage::Pool::new(executor.clone(), 16 * 1024 * 1024));
    let storage = Arc::new(MemoryStorage) as Arc<dyn Storage>;

    let (mut entity, task) = match block_on(cli.import.import(IMPORTERS, pool.clone())) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("Failed to import: {e}");
            exit(1);
        }
    };

    match block_on(task) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("Failed to complete import: {e}");
            exit(1);
        }
    };

    match block_on(entity.build_summaries(&pool.executor, &storage)) {
        Ok(_) => {}
        Err(e) => {
            eprintln!("Failed to build summaries: {e}");
            exit(1);
        }
    };

    let Some(view) = entity.display_default() else {
        eprintln!("Unknown view");
        exit(2);
    };

    let mut renderer = iguazu_screenshot::Renderer::new(cli.size.0, cli.size.1);
    let out = iguazu_screenshot::render(&mut renderer, pool, storage, &mut entity, &view, Duration::from_secs(cli.timeout));
    out.image.save(&cli.output).unwrap();
}
