#![cfg(feature = "capture")]

use std::sync::Arc;

use futures_util::future;
use futures_lite::{FutureExt, StreamExt, future::block_on};
use iguazu::{cli::ExportOpts, export::EXPORTERS, storage::{MemoryStorage, Storage}};
use owo_colors::OwoColorize;
use clap::{CommandFactory, Parser};
use iguazu_capture::{DRIVERS, DeviceSession, Drivers, ProbeFunc};
use itertools::Itertools;

/// Capture data from devices
#[derive(Parser)]
#[clap(override_usage = "iguazu capture DRIVER:ID [OPTIONS] -o <FILE>")]
pub struct Cli {
    /// Selected device ("driver:identifier")
    #[clap(value_name="ID")]
    device: Option<String>,

    /// List available devices on this system
    #[clap(short='l', long, action, exclusive(true))]
    list_devices: bool,

    /// List supported devices
    #[clap(short='L', long, action, exclusive(true))]
    list_drivers: bool,

    /// List options on the specified device
    #[clap(long, action, conflicts_with("out_filename"))]
    list_options: bool,

    /// Device configuration option (see --list-options)
    #[clap(short='c', value_name="KEY=VALUE", value_parser = parse_key_val)]
    option: Vec<(String, String)>,

    /// Output file
    #[clap(flatten)]
    output: Option<ExportOpts>,
}

fn parse_key_val(s: &str) -> Result<(String, String), String> {
    s.split_once('=')
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .ok_or_else(|| format!("invalid option `{s}`: expected KEY=VALUE"))
}

pub fn main(args: &Cli) -> Result<(), String> {
    if args.list_drivers {
        list_supported(&DRIVERS)
    } else if args.list_devices {
        block_on(list_devices(&DRIVERS))
    } else if let Some(device_id) = &args.device {
        let device = block_on(DRIVERS.device_by_driver_and_id(device_id))?;
        let mut session = block_on(device.open()).map_err(|e| e.to_string())?;

        for (key, value) in &args.option {
            session.set(key, value).map_err(|e| format!("failed to set `{key}={value}`: {}", e))?;
        }

        if args.list_options {
            list_options(session)
        } else if let Some(output) = &args.output {
            block_on(capture(session, output))
        } else {
            Err("no output file specified".to_string())
        }
    } else {
        Cli::command().error(
            clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand,
            "No device specified"
        ).print().map_err(|e| e.to_string())?;

        eprintln!("\n{}", "Available devices:".bold().underline());
        block_on(list_devices(&DRIVERS))?;
        std::process::exit(1);
    }
}

fn list_supported(drivers: &Drivers<'_>) -> Result<(), String> {
    let driver_name_len = drivers.iter().map(|d| d.name.len()).max().unwrap_or(0);

    for driver in drivers.iter() {
        println!("{name:driver_name_len$}  {desc}", name = driver.name.bold(), desc = driver.description);
    }

    Ok(())
}

async fn list_devices(drivers: &Drivers<'_>) -> Result<(), String> {
    let devices = drivers.list_devices().await;
    let device_id_len = devices.iter().map(|d| d.driver().name.len() + 1 + d.identifier().len()).max().unwrap_or(0);

    for device in &devices {
        println!("{id:device_id_len$}  {desc}",
            id = format_args!("{}:{}", device.driver().name, device.identifier()).bold(),
            desc = device.description()
        );
    }

    if devices.is_empty() {
        println!("{}", "No devices found.".bold());
    }

    let drivers_explicit: Vec<_> = drivers.iter().filter(|d| matches!(d.probe, ProbeFunc::Explicit(_))).collect();
    if !drivers_explicit.is_empty() {
        let msg = format_args!("{} devices not included in this list.", drivers_explicit.iter().map(|d| d.name.bold()).format(", "));
        eprintln!();
        eprintln!("{}", msg.dimmed());
    }

    Ok(())
}

fn list_options(session: Box<dyn DeviceSession>) -> Result<(), String> {
    println!("{}", format_args!("{} ({}:{})", session.device().description(), session.device().driver().name, session.device().identifier()).bold().underline());

    let option_len = session.options().iter().map(|o| o.name.len()).max().unwrap_or(0);

    for option in session.options() {
        println!("{name:option_len$}  {description} {value}",
            name = option.name.bold(),
            description = option.description,
            value = format_args!(" =  {:?}", session.get(option.name).unwrap_or("".to_string())).dimmed(),
        );
    }

    Ok(())
}

async fn capture(session: Box<dyn DeviceSession>, output: &ExportOpts) -> Result<(), String> {
    let executor = Arc::new(async_executor::Executor::new());
    let storage = Arc::new(MemoryStorage) as Arc<dyn Storage>;

    executor.run(async {
        eprintln!("Starting capture...");
        let (mut entity, capture_task) = session.start_capture(&executor, &storage).map_err(|e| format!("Failed to start capture: {e}"))?;
        let summary_task = entity.build_summaries(&executor, &storage);
        let export_task = output.export(EXPORTERS, executor.clone(), entity);

        let mut ctrlc = async_signal::Signals::new(&[async_signal::Signal::Int])
            .map_err(|e| format!("Failed to register signal handler: {e}"))?;

        future::try_join3(
            async {
                let result = async { capture_task.await.map(|()| true) }.or(async { ctrlc.next().await; Ok(false) }).await;

                let status = match result {
                    Ok(true) => "completed",
                    Ok(false) => "interrupted",
                    Err(_) => "failed",
                };

                eprintln!("Capture {status}, flushing output file...");

                result.map_err(|e| format!("Failed to complete capture: {}", e))
            },
            async { summary_task.await.map_err(|e| format!("Failed to build summaries: {}", e)) },
            async { export_task.await.map_err(|e| format!("Failed to write output: {}", e)) },
        ).await?;

        Ok(())
    }).await
}
