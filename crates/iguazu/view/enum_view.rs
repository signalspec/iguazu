use crate::{schema::EntityStream, stream::StreamState, Idx};

use super::{IntView, ViewManager};

pub struct EnumView<'a> {
    view: IntView<'a>,
}

impl<'a> EnumView<'a> {
    pub fn new(vm: &'a ViewManager, entity: &EntityStream) -> Option<Self> {
        let view = vm.int_view(entity)?;
        Some(EnumView { view })
    }

    pub fn get(&self, idx: Idx) -> Option<(usize, Idx)> {
        let val = self.view.get_u64(idx)? as usize;

        // TODO: get child index
        Some((val, 0))
    }

    pub fn state(&self) -> StreamState {
        self.view.state()
    }
}
