use crate::{schema::EntityStream, Idx};

use super::{IntView, ViewManager};

pub struct EnumView<'a> {
    view: IntView<'a>,
}

impl<'a> EnumView<'a> {
    pub fn new(vm: &'a ViewManager, entity: &EntityStream) -> Self {
        let view = vm.int_view(&entity);
        EnumView { view }
    }

    pub fn get(&self, idx: Idx) -> Option<(usize, Idx)> {
        let val = self.view.get_u64(idx)? as usize;

        // TODO: get child index
        Some((val, 0))
    }
}