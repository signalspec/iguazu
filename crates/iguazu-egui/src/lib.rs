mod time;
mod util;
mod color;
mod egui_util;

pub mod timeline;
pub mod table;

use std::sync::Arc;

use iguazu::view::ViewManager;
pub use time::{ Time, TimeRange };
pub use timeline::TimelineView;

struct RepaintWaker {
    context: egui::Context,
}

impl std::task::Wake for RepaintWaker {
    fn wake(self: Arc<Self>) {
        self.context.request_repaint();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.context.request_repaint();
    }
}

pub struct ViewerContext {
    view_manager: ViewManager,
}
impl ViewerContext {
    pub fn new(egui_ctx: &egui::Context) -> Self {
        let waker = Arc::new(RepaintWaker { context: egui_ctx.clone() }).into();
        Self {
            view_manager: ViewManager::new(waker),
        }
    }

    pub fn begin(&mut self) {
        self.view_manager.begin();
    }

    pub fn end(&mut self) {
        self.view_manager.end();
    }
}
