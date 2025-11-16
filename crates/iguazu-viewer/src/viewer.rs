use egui::Ui;
use iguazu::schema::{attribute::DefaultView, EntityStream};
use iguazu_egui::{table::TableView, TimelineView, ViewerContext};

pub struct Viewer {
    entity: EntityStream,
    view: Option<DefaultView>,
}

impl Viewer {
    pub fn new(entity: EntityStream) -> Self {
        let view = entity.display_default();
        Self { entity, view }
    }

    pub fn show(&mut self, vctx: &mut ViewerContext, ui: &mut Ui) {
        match self.view {
            Some(DefaultView::Table) => TableView::new().show(vctx, ui, &mut self.entity),
            Some(DefaultView::Timeline) => TimelineView::new().show(vctx, ui, &mut self.entity),
            None => {
                ui.label("unknown view");
            }
        }
    }
}
