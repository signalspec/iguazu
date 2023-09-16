#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use std::{sync::Arc, hash::{Hash, Hasher}, collections::hash_map::DefaultHasher};

use eframe::egui;
use egui::{Color32, Frame};
use iguazu::{ Stream, in_memory::MemoryStream, stream::cache::{IntView, index::IndexView} };
use num_rational::Ratio;
use time::TimeRange;
use timeline::{TimePanel, EnumVariant, DisplayItem, DisplayEvent, DisplayItemKind, DisplayLogic};

mod time;
mod ui;
mod util;
use self::time::Time;

mod timeline;

struct ViewerContext<'a> {
    time: &'a mut Option<Time>,
}
impl<'a> ViewerContext<'a> {
    fn time(&self) -> Option<Time> {
        *self.time
    }

    fn set_time(&mut self, time: Time) {
        *self.time = Some(time);
    }
}

fn main() -> Result<(), eframe::Error> {
    // Log to stdout (if you run with `RUST_LOG=debug`).
    tracing_subscriber::fmt::init();

    let options = eframe::NativeOptions {
        initial_window_size: Some(egui::vec2(640.0, 480.0)),
        ..Default::default()
    };

    let variants = vec![
        EnumVariant { name: "A".to_string(), color: Color32::RED },
        EnumVariant { name: "B".to_string(), color: Color32::GREEN },
        EnumVariant { name: "C".to_string(), color: Color32::BLUE },
    ];

    let items = [
        DisplayItem {
            name: format!("Logic"),
            sample_rate: Ratio::new(1000, 1),
            kind: DisplayItemKind::Logic(DisplayLogic{
                data: IndexView::new(MemoryStream::new(&[
                    10,
                    11, 12, 13, 14, 15, 16, 17, 18, 19, 20,
                    25, 30, 35, 40, 45, 55, 60, 65, 70,
                    80, 90, 100, 110, 120, 130, 140, 150, 160, 170,
                ])),
                color: Color32::GREEN,
            }),
        }
    ];

    let app = App {
        time: None,
        time_panel: TimePanel {
            col_width: 0.0,
            time_range: TimeRange { min: Time::ZERO, max: Time::SECOND },
            visible_range: None,
            items: items.into_iter().chain((1..=40).map(|i| {
                let items: Vec<_> = (0..200).map(|x| {
                    let mut hasher = DefaultHasher::new();
                    (i, x).hash(&mut hasher);
                    (hasher.finish() % 3) as u8
                }).collect();
                let data = MemoryStream::new(&items) as Arc<dyn Stream<u8>>;

                DisplayItem {
                    name: format!("Channel {i}"),
                    sample_rate: Ratio::new(200, 1),
                    kind: DisplayItemKind::Event(DisplayEvent { data: IntView::new(data.into()), variants: variants.clone() }),
                }
            })).collect(),
        },
    };

    eframe::run_native(
        "Iguazu Viewer",
        options,
        Box::new(|_cc| Box::new(app)),
    )
}

struct App {
    time_panel: timeline::TimePanel,

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
