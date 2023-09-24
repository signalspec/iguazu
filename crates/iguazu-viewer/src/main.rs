#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use std::fs::File;

use eframe::egui;
use egui::Frame;

use iguazu_egui::{timeline::TimePanel, TimeRange, Time, ViewerContext};

fn main() -> Result<(), eframe::Error> {
    // Log to stdout (if you run with `RUST_LOG=debug`).
    tracing_subscriber::fmt::init();

    let options = eframe::NativeOptions {
        initial_window_size: Some(egui::vec2(640.0, 480.0)),
        ..Default::default()
    };

    let fname = std::env::args().nth(1).expect("filename passed as command line arg");
    let importer = iguazu::import::IMPORTERS.first_for_filename(&fname).expect("No importer for extension");
    let mut file = File::open(fname).unwrap();
    let entity = importer.import(&mut file).unwrap();

    let app = App {
        time: None,
        time_panel: TimePanel {
            col_width: 0.0,
            time_range: TimeRange { min: Time::ZERO, max: Time::MINUTE },
            visible_range: None,
            entity
        },
    };

    eframe::run_native(
        "Iguazu Viewer",
        options,
        Box::new(|_cc| Box::new(app)),
    )
}

struct App {
    time_panel: TimePanel,

    /// Selected time
    time: Option<Time>,
}

impl eframe::App for App {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.set_pixels_per_point(2.0);
        let frame = Frame::central_panel(&*ctx.style()).inner_margin(0.0);
        egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
            let vctx = &mut ViewerContext {
                time: &mut self.time,
            };
            self.time_panel.show(vctx, ui)
        });
    }
}
