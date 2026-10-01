use std::{
    sync::Arc,
    time::Duration,
};

use egui::{TextStyle, Ui};
use iguazu::{io::IoError, schema::{EntityStream, attribute::display::Layout}, storage::{Pool, Storage}, view::LoadingState};
use iguazu_egui::{TimelineView, ViewerContext, table::TableView};

mod screenshot;
pub use screenshot::{Renderer, WgpuImage};

pub struct ScreenshotOutput {
    pub image: WgpuImage,
    pub errors: Vec<Arc<IoError>>,
    pub timed_out: bool,
}

pub fn render(renderer: &mut Renderer, pool: Arc<Pool>, storage: Arc<dyn Storage>, entity: &mut EntityStream, view: &Layout, timeout: Duration) -> ScreenshotOutput {
    let deadline = std::time::Instant::now() + timeout;
    let mut timed_out = false;

    renderer.egui_ctx().set_theme(egui::Theme::Dark);
    renderer.egui_ctx().global_style_mut(|style| {
        style.text_styles.insert(TextStyle::Body, egui::FontId::proportional(16.0));
        style.text_styles.insert(TextStyle::Heading, egui::FontId::proportional(24.0));
    });
    renderer.egui_ctx().tessellation_options_mut(|o| {
        // Rounding causes jitter and gaps in timeline logic traces
        o.round_line_segments_to_pixels = false;
        o.round_rects_to_pixels = false;
    });

    let mut vctx = ViewerContext::new(pool, storage, &renderer.egui_ctx(), renderer.render_state());

    loop {
        log::info!("Running frame {}", renderer.egui_ctx().cumulative_frame_nr());
        vctx.begin();

        renderer.run_ui(|ui| {
            show(&mut vctx, ui, entity, view)
        });

        vctx.end();

        match vctx.loading_state() {
            LoadingState::Loading => {}
            _ => break,
        }

        if !renderer.wait_next_frame_until(deadline) {
            log::warn!("Loading timed out ({} ms)", timeout.as_millis());
            timed_out = true;
            break;
        }
    }

    log::info!("Rendering output");
    let image = renderer.render();
    log::info!("Rendering complete");

    let errors = vctx.view_errors();

    ScreenshotOutput { image, errors, timed_out }
}

pub fn show(vctx: &mut ViewerContext, ui: &mut Ui, entity: &mut EntityStream, view: &Layout) {
    match view {
        Layout::Table => TableView::new().show(vctx, ui, entity),
        Layout::Timeline => TimelineView::new().show(vctx, ui, entity),
    }
}
