use std::{rc::Rc, sync::Arc};
use append_array::AppendArray;

use crate::{schema::EntityStream, stream::{ArcStream, Block, Stream}};

mod int_view;
pub use int_view::IntView;

mod number_view;
pub use number_view::NumberView;

mod enum_view;
pub use enum_view::EnumView;

mod text_view;
pub use text_view::TextView;

pub trait StreamAccess {
    fn stream(&self) -> &Arc<dyn Stream>;

    fn get_block(&self, block: u64) -> Option<Block>;
}

pub trait ViewManager: Sized {
    fn stream(&mut self, stream: &ArcStream) -> Rc<dyn StreamAccess>;
    
    fn int_view(&mut self, entity: &EntityStream) -> IntView {
        IntView::new(self, entity)
    }

    fn number_view(&mut self, entity: &EntityStream) -> NumberView {
        NumberView::new(self, entity)
    }

    fn enum_view(&mut self, entity: &EntityStream) -> EnumView {
        EnumView::new(self, entity)
    }

    fn text_view(&mut self, entity: &EntityStream) -> TextView {
        TextView::new(self, entity)
    }
}

pub struct SimpleViewManager;

impl ViewManager for SimpleViewManager {
    fn stream(&mut self, stream: &ArcStream) -> Rc<dyn StreamAccess> {
        Rc::new(SimpleViewManagerAccess(stream.clone()))
    }
}

struct SimpleViewManagerAccess(Arc<dyn Stream>);

impl StreamAccess for SimpleViewManagerAccess {
    fn stream(&self) -> &Arc<dyn Stream> {
        &self.0
    }

    fn get_block(&self, block: u64) -> Option<Arc<AppendArray<u8>>> {
        self.0.get_block(block)
    }
}

