use iguazu::schema::EntityStream;

use super::TimelineResponse;

pub(crate) fn render(_ctx: &mut crate::ViewerContext, _ui: &mut egui::Ui, _scale: &super::scale::Scale, _label: Option<&str>, _entity: &EntityStream) -> TimelineResponse {
    TimelineResponse::default()
}


