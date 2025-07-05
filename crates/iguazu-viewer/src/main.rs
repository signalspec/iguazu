#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use clap::Parser;
use eframe::{egui, CreationContext};
use egui::Frame;

use futures_lite::future::block_on;
use iguazu::{cli::ImportOpts, import::IMPORTERS, schema::{attribute::DefaultView, Entity, EntityStream}, stream::ArcStream};
use iguazu_egui::{table::TableView, timeline::TimelineView, ViewerContext};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[clap(flatten)]
    import: ImportOpts,
}

fn main() -> Result<(), eframe::Error> {
    // Log to stdout (if you run with `RUST_LOG=debug`).
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    let (entity, completion) = block_on(cli.import.import(IMPORTERS)).expect("Failed to load file");
    block_on(completion).expect("Failed to complete import");

    let view = entity.display_default();

    let options = eframe::NativeOptions {
        ..Default::default()
    };

    eframe::run_native(
        "Iguazu Viewer",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc, entity, view)))),
    )
}

struct App {
    entity: EntityStream,
    view: Option<DefaultView>,
    vctx: ViewerContext,
}
impl App {
    fn new(cc: &CreationContext, entity: Entity<ArcStream>, view: Option<DefaultView>) -> Self {
        cc.egui_ctx.tessellation_options_mut(|o| {
            // Rounding causes jitter and gaps in timeline logic traces
            o.round_line_segments_to_pixels = false;
            o.round_rects_to_pixels = false;
        });

        App {
            view,
            entity,
            vctx: ViewerContext::new(),
        }
    }
}

impl eframe::App for App {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let frame = Frame::central_panel(&*ctx.style()).inner_margin(0.0);
        egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
            match self.view {
                Some(DefaultView::Table) => TableView::new().show(&mut self.vctx, ui, &mut self.entity),
                Some(DefaultView::Timeline) => TimelineView::new().show(&mut self.vctx, ui, &mut self.entity),
                None => {
                    ui.label("unknown view");
                }
            }
        });

        self.vctx.update();
    }
}
