#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use std::{sync::Arc, hash::{Hash, Hasher}, collections::hash_map::DefaultHasher};

use eframe::egui;
use egui::Frame;
use iguazu::{ Stream, in_memory::MemoryStream, entity::{Entity, NamedColor, Enum, EnumVariant, Timestamp } };
use indexmap::indexmap;
use num_rational::Ratio;
use time::TimeRange;
use timeline::TimePanel;

mod time;
mod ui;
mod util;
mod color;
mod egui_util;
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

    let entity = Entity::Group(indexmap!{
        "Enum".into() => Entity::Enum(Enum {
            sample_rate: Some(Ratio::new(1000, 1)),
            data: {
                let values: Vec<_> = (0..2000).map(|x| {
                    let mut hasher = DefaultHasher::new();
                    x.hash(&mut hasher);
                    (hasher.finish() % 3) as u8
                }).collect();
                (MemoryStream::new(&values) as Arc<dyn Stream<_>>).into()
            },
            variants: indexmap!{
                "A".into() => EnumVariant { color: Some(NamedColor::Red) },
                "B".into() => EnumVariant { color: Some(NamedColor::Green) },
                "C".into() => EnumVariant { color: Some(NamedColor::Blue) },
            }
        }),

        "Logic".into() => Entity::Timestamp(Timestamp {
            color: Some(NamedColor::Green),
            base_clock: Ratio::new(1000, 1),
            data: {
                MemoryStream::new(&[
                    10,
                    11, 12, 13, 14, 15, 16, 17, 18, 19, 20,
                    25, 30, 35, 40, 45, 55, 60, 65, 70,
                    80, 90, 100, 110, 120, 130, 140, 150, 160, 170,
                ])
            }
        }),
    });

    let app = App {
        time: None,
        time_panel: TimePanel {
            col_width: 0.0,
            time_range: TimeRange { min: Time::ZERO, max: Time::SECOND },
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
