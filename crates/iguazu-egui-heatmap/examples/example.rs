use std::sync::Arc;

use egui::{Color32, Rangef, Ui};

use iguazu_egui_heatmap::{Colormap, DisplayOptions, Heatmap, HeatmapRenderer, SampleOrder, TextureOptions};

pub struct App {
    h1: State,
    h2: State,
}

pub struct State {
    heatmap: Heatmap::<u8>,
    x: Rangef,
    y: Rangef,
    alpha: f32,
    colormap: Colormap,
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

        let h1 = heatmap_renderer.heatmap(32, 32, TextureOptions::default());
        h1.write_all(&buf);

        //let h2 = heatmap_renderer.heatmap(32, 32, TextureOptions::default());
        //h2.write_all(&buf);

        Self {
            h1: State {
                heatmap: h1.clone(),
                alpha: 1.0,
                colormap: Colormap::Inferno,
                x: Rangef::new(0., 1.),
                y: Rangef::new(0., 1.),
                range: Rangef::new(0., 1.),
                order: SampleOrder::RightDown,
            },
            h2: State {
                heatmap: h1,
                alpha: 1.0,
                colormap: Colormap::Viridis,
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

            ui.painter().add(state.heatmap.shape_at(rect, DisplayOptions {
                alpha: state.alpha,
                colormap: state.colormap,
                transformation: (state.x, state.y, state.order).into(),
                value_range: state.range
            }));
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

        ui.add(egui::Slider::new(&mut state.alpha, 0.0..=1.0).text("alpha"));

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
            });

        egui::ComboBox::from_label("Colormap")
            .selected_text(format!("{:?}", state.colormap))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut state.colormap, Colormap::Viridis, "Viridis");
                ui.selectable_value(&mut state.colormap, Colormap::Inferno, "Inferno");
                ui.selectable_value(&mut state.colormap, Colormap::Magma, "Magma");
                ui.selectable_value(&mut state.colormap, Colormap::Plasma, "Plasma");
                ui.selectable_value(&mut state.colormap, Colormap::Linear(Color32::BLACK, Color32::WHITE), "Grayscale");
                ui.selectable_value(&mut state.colormap, Colormap::Linear(Color32::RED, Color32::BLUE), "Red-Blue");
            });
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
