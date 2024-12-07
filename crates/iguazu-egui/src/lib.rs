mod time;
mod util;
mod cache;
mod color;
mod egui_util;
pub mod timeline;

pub use time::{ Time, TimeRange };
pub use timeline::TimePanel;

pub struct ViewerContext<'a> {
    pub time: &'a mut Option<Time>,
}

impl<'a> ViewerContext<'a> {
    fn time(&self) -> Option<Time> {
        *self.time
    }

    fn set_time(&mut self, time: Time) {
        *self.time = Some(time);
    }
}

