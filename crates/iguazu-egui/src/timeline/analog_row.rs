use egui::{Pos2, Stroke, Vec2};
use iguazu::{schema::{attribute::{AccentColor, NumberRange, SampleRate}, EntityStream}, IdxRange};

use crate::color::named_color;

use super::{fixed_height_header, TimelineResponse};

pub(crate) fn render(vcx: &mut crate::ViewerContext, ui: &mut egui::Ui, scale: &super::scale::Scale, label: Option<&str>, entity: &EntityStream) -> TimelineResponse {
    let rect = fixed_height_header(ui, scale, label, 64.0);
    let padded_rect = rect.shrink2(Vec2::new(0.0, 4.0));
    if !ui.is_rect_visible(padded_rect) {
        return TimelineResponse::default();
    }

    let Some(sample_rate) = entity.attribute::<SampleRate>() else {
        return TimelineResponse::default()
    };

    let Some(number_range) = entity.attribute::<NumberRange>() else {
        return TimelineResponse::default()
    };

    let idx_scale = scale.idx_scale(sample_rate.0);

    let color = named_color(
        entity
            .attribute::<AccentColor>()
            .unwrap_or(AccentColor::Green),
    );

    let painter = ui.painter_at(rect);
    let stroke_width = 1.0;
    let stroke = Stroke::new(stroke_width, color);

    let state = entity.data.state();
    let range = IdxRange {
        min: idx_scale.visible.min,
        max: (idx_scale.visible.max + 1).min(state.end),
    };
    let view = vcx.view_manager.number_view(&entity);

    let v_margin = stroke_width * 2.0;
    let v_scale = -1.0 * (rect.height() - v_margin * 2.0) as f64 / (number_range.max - number_range.min);
    let v_offset = v_margin + rect.top() + (number_range.min * v_scale) as f32;

    let mut last = None;
    let mut last_idx = 0;

    let dot_opacity = ((idx_scale.points_per_index() - 4.0 * stroke_width) / 8.0).clamp(0.0, 1.0);
    let dot_color = color.gamma_multiply(dot_opacity);

    view.for_each_elem(range, |idx, val| {
        let pos = val.map(|val| Pos2 {
            x: idx_scale.x_from_idx(idx),
            y: (val * v_scale) as f32 + v_offset,
        });

        if dot_opacity > 0.0 {
            if let Some(pos) = pos {
                painter.circle_filled(pos, stroke_width * 2.0, dot_color);
            }
        }

        if let (Some(lpos), Some(pos)) = (last, pos) {
            painter.line_segment([lpos, pos], stroke);
        }

        last = pos;
        last_idx = idx;
    });

    TimelineResponse {
        snap_to_time: None,
    }
}