use std::sync::Arc;

use egui::{Rangef, Ui};

use iguazu_egui_heatmap::{Heatmap, HeatmapRenderer, SampleOrder};

pub struct App {
    h1: State,
    h2: State,
}

pub struct State {
    heatmap: Heatmap,
    x: Rangef,
    y: Rangef,
    range: Rangef,
    order: SampleOrder,
}

impl App {
    pub fn new<'a>(cc: &'a eframe::CreationContext<'a>) -> Self {
        let wgpu_render_state = cc.wgpu_render_state.as_ref().unwrap();

        let heatmap_renderer = Arc::new(HeatmapRenderer::new(wgpu_render_state));

        let mut buf: Vec<u8> = (0..(32*32)).map(|i| (i * 256 / 32 / 32) as u8).collect();
        buf[0] = 254;
        buf[31] = 64;

        let mut h1 = heatmap_renderer.heatmap(32, 32);
        h1.write(&buf);

        let mut h2 = heatmap_renderer.heatmap(32, 32);
        h2.write(&buf);
        
        Self {
            h1: State {
                heatmap: h1,
                x: Rangef::new(0., 1.),
                y: Rangef::new(0., 1.),
                range: Rangef::new(0., 1.),
                order: SampleOrder::RightDown,
            },
            h2: State {
                heatmap: h2,
                x: Rangef::new(0., 1.),
                y: Rangef::new(0., 1.),
                range: Rangef::new(0., 1.),
                order: SampleOrder::RightDown,
            }
        }
    }
}

fn heatmap_demo(ui: &mut Ui, state: &mut State) {
    ui.vertical(|ui| {
        egui::Frame::canvas(ui.style()).show(ui, |ui| {
            let (rect, _) = ui.allocate_exact_size(
                egui::Vec2::splat(400.0),
                egui::Sense::hover()
            );

            ui.painter().add(state.heatmap.shape_at(rect, (state.x, state.y, state.order), state.range));
        });
        
        ui.horizontal(|ui| {
            ui.add(egui::Slider::new(&mut state.x.min, -0.1..=1.1).text("xleft"));
            ui.add(egui::Slider::new(&mut state.x.max, -0.1..=1.1).text("xright"));
        });
    
        ui.horizontal(|ui| {
            ui.add(egui::Slider::new(&mut state.y.min, -0.1..=1.1).text("ytop"));
            ui.add(egui::Slider::new(&mut state.y.max, -0.1..=1.1).text("ybottom"));
        });

        ui.horizontal(|ui| {
            ui.add(egui::Slider::new(&mut state.range.min, -0.1..=1.1).text("vmin"));
            ui.add(egui::Slider::new(&mut state.range.max, -0.1..=1.1).text("vmax"));
        });

        egui::ComboBox::from_label("Sample order")
            .selected_text(format!("{:?}", state.order))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut state.order, SampleOrder::RightDown, "RightDown");
                ui.selectable_value(&mut state.order, SampleOrder::LeftDown, "LeftDown");
                ui.selectable_value(&mut state.order, SampleOrder::RightUp, "RightUp");
                ui.selectable_value(&mut state.order, SampleOrder::LeftUp, "LeftUp");
                ui.selectable_value(&mut state.order, SampleOrder::DownRight, "DownRight");
                ui.selectable_value(&mut state.order, SampleOrder::UpRight, "UpRight");
                ui.selectable_value(&mut state.order, SampleOrder::DownLeft, "DownLeft");
                ui.selectable_value(&mut state.order, SampleOrder::UpLeft, "UpLeft");
            })
            

    });
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.horizontal(|ui| {
            ui.push_id(1, |ui| {
                heatmap_demo(ui, &mut self.h1);
            });
            ui.push_id(2, |ui| {
                heatmap_demo(ui, &mut self.h2);
            })
        });
    }
}


fn main() {
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };

    eframe::run_native(
        "egui demo app",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    ).unwrap()
}