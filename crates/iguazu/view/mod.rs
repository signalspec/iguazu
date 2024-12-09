use std::sync::Arc;

use crate::{schema::EntityStream, stream::ArcStream, IdxRange};

mod view;
pub use view::View;

mod number_view;
pub use number_view::NumberView;

pub trait ViewManager {
    fn view(&mut self, stream: &ArcStream, range: IdxRange) -> Arc<View>;

    fn number_view(&mut self, entity: &EntityStream, range: IdxRange) -> NumberView {
        let view = self.view(&entity.data, range);
        NumberView::new(entity, view)
    }
}

