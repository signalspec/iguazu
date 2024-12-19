use std::sync::Arc;

use crate::{schema::EntityStream, stream::ArcStream, IdxRange};

mod view;
pub use view::View;

mod number_view;
pub use number_view::NumberView;

mod int_view;
pub use int_view::IntView;

mod enum_view;
pub use enum_view::EnumView;

mod text_view;
pub use text_view::TextView;

pub trait ViewManager: Sized {
    fn view(&mut self, stream: &ArcStream, range: IdxRange) -> Arc<View>;

    fn number_view(&mut self, entity: &EntityStream, range: IdxRange) -> NumberView {
        let view = self.view(&entity.data, range);
        NumberView::new(entity, view)
    }

    fn int_view(&mut self, entity: &EntityStream, range: IdxRange) -> IntView {
        let view = self.view(&entity.data, range);
        IntView::new(entity, view)
    }

    fn enum_view(&mut self, entity: &EntityStream, range: IdxRange) -> EnumView {
        let view = self.view(&entity.data, range);
        EnumView::new(entity, view)
    }

    fn text_view(&mut self, entity: &EntityStream, range: IdxRange) -> TextView {
        TextView::new(self, entity, range)
    }
}

pub struct SimpleViewManager;

impl ViewManager for SimpleViewManager {
    fn view(&mut self, stream: &ArcStream, range: IdxRange) -> Arc<View> {
        let mut v = View::new(stream.clone());
        v.set_range(range);
        Arc::new(v)
    }
}

