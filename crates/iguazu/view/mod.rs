mod view;
use std::sync::Arc;

pub use view::View;

use crate::{stream::ArcStream, IdxRange};

pub trait ViewManager {
    fn view(&mut self, stream: &ArcStream, range: IdxRange) -> Arc<View>;
}

