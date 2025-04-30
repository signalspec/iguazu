mod time;
mod util;
mod color;
mod egui_util;

pub mod timeline;
pub mod table;

use iguazu::view::ViewManager;
pub use time::{ Time, TimeRange };
pub use timeline::TimelineView;

pub struct ViewerContext {
    view_manager: ViewManager,
}

impl ViewerContext {
    pub fn new() -> Self {
        Self {
            view_manager: ViewManager::new(),
        }
    }

    pub fn update(&mut self) {
        self.view_manager.update();
    }
}

