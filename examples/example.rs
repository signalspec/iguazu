use std::sync::Arc;

use egui::Ui;

use iguazu_egui_heatmap::{Heatmap, HeatmapRenderer};

pub struct App {
    h1: Heatmap,
    h2: Heatmap,
}

impl App {
    pub fn new<'a>(cc: &'a eframe::CreationContext<'a>) -> Self {
        let wgpu_render_state = cc.wgpu_render_state.as_ref().unwrap();

        let heatmap_renderer = Arc::new(HeatmapRenderer::new(wgpu_render_state));

        let buf: Vec<u8> = (0..(32*32)).map(|i| (i * 256 / 32 / 32) as u8).collect();

        let mut h1 = heatmap_renderer.heatmap(32, 32);
        h1.write(&buf);

        let mut h2 = heatmap_renderer.heatmap(32, 32);
        h2.write(&buf);
        
        Self { h1, h2 }
    }
}

fn heatmap_demo(ui: &mut Ui, heatmap: &mut Heatmap) {
    ui.vertical(|ui| {
        egui::Frame::canvas(ui.style()).show(ui, |ui| {
            let (rect, _) = ui.allocate_exact_size(
                egui::Vec2::splat(400.0),
                egui::Sense::hover()
            );

            ui.painter().add(heatmap.shape_at(rect));
        });
        
        ui.horizontal(|ui| {
            ui.add(egui::Slider::new(&mut heatmap.uniforms.top_left[0], -0.1..=1.1).text("xleft"));
            ui.add(egui::Slider::new(&mut heatmap.uniforms.bottom_right[0], -0.1..=1.1).text("xright"));
        });
    
        ui.horizontal(|ui| {
            ui.add(egui::Slider::new(&mut heatmap.uniforms.top_left[1], -0.1..=1.1).text("ytop"));
            ui.add(egui::Slider::new(&mut heatmap.uniforms.bottom_right[1], -0.1..=1.1).text("ybottom"));
        });
    });
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                heatmap_demo(ui, &mut self.h1);
                heatmap_demo(ui, &mut self.h2);
            });
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