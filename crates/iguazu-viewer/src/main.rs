#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use eframe::egui;
use egui::Frame;

use iguazu::{io::FsFile, schema::EntityStream};
use iguazu_egui::{table::TableView, timeline::TimelineView, ViewerContext};

fn main() -> Result<(), eframe::Error> {
    // Log to stdout (if you run with `RUST_LOG=debug`).
    tracing_subscriber::fmt::init();

    let options = eframe::NativeOptions {
        ..Default::default()
    };

    let fname = std::env::args().nth(1).expect("filename passed as command line arg");
    let importer = iguazu::import::IMPORTERS.first_for_filename(&fname).expect("No importer for extension");
    let file = FsFile::new(fname.into()).unwrap();
    let entity = importer.import(file).unwrap();

    let app = App {
        entity,
    };

    eframe::run_native(
        "Iguazu Viewer",
        options,
        Box::new(|_cc| Ok(Box::new(app))),
    )
}

struct App {
    entity: EntityStream,
}

impl eframe::App for App {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let frame = Frame::central_panel(&*ctx.style()).inner_margin(0.0);
        egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
            let vctx = &mut ViewerContext {};
            TimelineView::new().show(vctx, ui, &mut self.entity);
        });
    }
}
