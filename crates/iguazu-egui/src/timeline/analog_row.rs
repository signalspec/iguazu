use egui::{Pos2, Stroke, Vec2};
use iguazu::{schema::{attribute::AccentColor, EntityStream}, IdxRange};

use crate::color::named_color;

use super::{fixed_height_header, TimelineResponse};

pub(crate) fn render(vcx: &mut crate::ViewerContext, ui: &mut egui::Ui, scale: &super::scale::Scale, label: Option<&str>, entity: &EntityStream) -> TimelineResponse {
    let rect = fixed_height_header(ui, scale, label, 64.0);
    let padded_rect = rect.shrink2(Vec2::new(0.0, 4.0));
    if !ui.is_rect_visible(padded_rect) {
        return TimelineResponse::default();
    }

    let Some(sample_rate) = entity.sample_rate() else {
        return TimelineResponse::default()
    };

    let Some(y_range) = entity.number_range() else {
        return TimelineResponse::default()
    };

    let Some(view) = vcx.view_manager.number_view(&entity) else {
        return TimelineResponse::default();
    };

    let idx_scale = scale.idx_scale(sample_rate);

    let color = named_color(
        entity
            .accent_color()
            .unwrap_or(AccentColor::Green),
    );

    let painter = ui.painter_at(rect);
    let stroke_width = 1.0;
    let stroke = Stroke::new(stroke_width, color);

    let state = view.state();
    let x_range = IdxRange {
        min: idx_scale.visible.min,
        max: (idx_scale.visible.max + 1).min(state.end),
    };

    let v_margin = stroke_width * 2.0;
    let v_scale = -1.0 * (rect.height() - v_margin * 2.0) as f64 / (y_range.max - y_range.min);
    let v_offset = rect.bottom() - v_margin - (y_range.min * v_scale) as f32;

    let mut last: Option<Pos2> = None;

    let dot_opacity = ((idx_scale.points_per_index() - 4.0 * stroke_width) / 8.0).clamp(0.0, 1.0);
    let dot_color = color.gamma_multiply(dot_opacity);

    let min_dist_sq = 4.0 / ui.ctx().pixels_per_point() / ui.ctx().pixels_per_point();

    view.for_each_elem(x_range, |idx, val| {
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
            if lpos.distance_sq(pos) < min_dist_sq {
                // Lines that are too short are not rendered by egui, so we skip them
                // to ensure that the line is continuous and to reduce overdraw.
                return;
            }
            painter.line_segment([lpos, pos], stroke);
        }

        last = pos;
    });

    TimelineResponse {
        snap_to_time: None,
    }
}