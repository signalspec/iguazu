use egui::{Align, Align2, Color32, FontId, Painter, Pos2, Rangef, Rect, Stroke, Ui, Vec2};
use iguazu::{schema::{attribute::{AccentColor, SampleRate}, EntityKind, EntityStream}, view::IntView, Idx, IdxRange};

use crate::{color::named_color, ViewerContext};

use super::{fixed_height_header, scale::{IdxScale, Scale}, TimelineResponse};

pub(crate) fn render(
    vcx: &mut ViewerContext,
    ui: &mut Ui,
    scale: &Scale,
    label: Option<&str>,
    entity: &EntityStream,
) -> TimelineResponse {
    let rect = fixed_height_header(ui, scale, label, 32.0);
    let padded_rect = rect.shrink2(Vec2::new(0.0, 4.0));
    if !ui.is_rect_visible(padded_rect) {
        return TimelineResponse::default();
    }

    let Some(sample_rate) = entity.attribute::<SampleRate>() else {
        return TimelineResponse::default();
    };

    let idx_scale = scale.idx_scale(sample_rate.0);

    let color = named_color(
        entity
            .attribute::<AccentColor>()
            .unwrap_or(AccentColor::Green),
    );

    let font_id = egui::TextStyle::Body.resolve(ui.style());
    let font_color = ui.style().visuals.text_color();

    let painter = ui.painter_at(rect);
    let stroke_width = 1.0;
    let stroke = Stroke::new(stroke_width, color);

    let state = entity.data.state();
    let range = IdxRange {
        min: idx_scale.visible.min,
        max: idx_scale.visible.max.min(state.end),
    };
    let view = vcx.view_manager.int_view(&entity);

    scan(&idx_scale, &view, range, |a, b| a==b, |x1, x2, _idx1, _idx2, val | {
        let h_pad = 5.0;
        let text_min_width = 8.0;
        let tx = x1.max(padded_rect.left()) + h_pad;
        if x2 - tx > text_min_width {
            let opacity = ((x2 - tx - text_min_width) / 4.0).clamp(0.0, 1.0);
            painter
                .with_clip_rect(
                    Rect::from_x_y_ranges(
                        Rangef::new(tx, x2 - h_pad),
                        padded_rect.y_range()
                    )
                )
                .text(
                    Pos2::new(tx, padded_rect.y_range().center()),
                    Align2::LEFT_CENTER,
                    entity.kind.format(val).to_string(),
                    font_id.clone(), 
                    font_color.gamma_multiply(opacity)
                );
        }
        painter.hline(x1..=x2, padded_rect.bottom(), stroke);
        painter.hline(x1..=x2, padded_rect.top(), stroke);
        painter.vline(x2, padded_rect.y_range(), stroke);
    });

    TimelineResponse {
        snap_to_time: None
    }
}

pub(crate) fn render_logic(
    vcx: &mut ViewerContext,
    ui: &mut Ui,
    scale: &Scale,
    _label: Option<&str>,
    entity: &EntityStream,
) -> TimelineResponse {
    let EntityKind::Logic { ref bits } = entity.kind else {
        return TimelineResponse::default();
    };

    let Some(sample_rate) = entity.attribute::<SampleRate>() else {
        return TimelineResponse::default();
    };
    let idx_scale = scale.idx_scale(sample_rate.0);

    let state = entity.data.state();

    let range = IdxRange {
        min: idx_scale.visible.min,
        max: idx_scale.visible.max.min(state.end),
    };
    let view = vcx.view_manager.int_view(&entity);

    let font_id = egui::TextStyle::Body.resolve(ui.style());
    let font_color = ui.style().visuals.text_color();

    let interact_radius = ui.style().interaction.resize_grab_radius_side;
    let mut snap_to_idx = None;

    for (bit, field) in bits.iter().enumerate() {
        let rect = fixed_height_header(ui, scale, Some(&field.name), 32.0);
        let padded_rect = rect.shrink2(Vec2::new(0.0, 4.0));
        if !ui.is_rect_visible(padded_rect) {
            continue;
        }

        let color = field.attribute::<AccentColor>()
            .unwrap_or(AccentColor::Green);
        let color = named_color(color);

        let painter = ui.painter_at(rect);
        let stroke_width = 1.0;
        let stroke = Stroke::new(stroke_width, color);

        let mask = 1 << bit;
        let eq = |v1: u64, v2: u64| {
            v1 & mask == v2 & mask
        };

        let hover_x = ui.input(|i| i.pointer.interact_pos())
            .filter(|pos| rect.contains(*pos) && ui.ctx().dragged_id().is_none())
            .map(|pos| pos.x);

        let mut prev_idx = None;

        scan(&idx_scale, &view, range, eq, |x1, x2, idx1, idx2, val | {
            if val & mask != 0 {
                painter.hline(x1..=x2, padded_rect.top(), stroke);
            } else {
                painter.hline(x1..=x2, padded_rect.bottom(), stroke);
            }

            painter.vline(x2, padded_rect.y_range(), stroke);

            if let Some(hover_x) = hover_x {
                if (hover_x - x1).abs() < interact_radius {
                    // Hovering the left edge of this span
                    snap_to_idx = idx1;

                    if let (Some(idx0), Some(idx2)) = (prev_idx, idx2) {
                        let x0 = idx_scale.x_from_idx(idx0);

                        let anchor = if x2 - x1 > 80.0 {
                            Some(Align::LEFT)
                        } else if x1 - x0 > 80.0 {
                            Some(Align::RIGHT)
                        } else { None };

                        if let Some(anchor) = anchor {
                            let span_rect = Rect::from_x_y_ranges(x0..=x2, padded_rect.y_range());
                            let width = idx_scale.t_from_idx(idx2) - idx_scale.t_from_idx(idx0);
                            let text = width.format_period_as_freq().to_string();
                            span_width_label(&painter, span_rect, x1 - anchor.to_sign() * 8.0, anchor, &font_id, font_color, text);
                        }
                    }
                } else if (x1..x2 - interact_radius).contains(&hover_x) {
                    // hovering within the span
                    if let (Some(idx1), Some(idx2)) = (idx1, idx2) {
                        if x2 - x1 > 80.0 {
                            let span_rect = Rect::from_x_y_ranges(x1..=x2, padded_rect.y_range());
                            let width = idx_scale.t_from_idx(idx2) - idx_scale.t_from_idx(idx1);
                            let text = width.format_relative(idx_scale.sample_period()).to_string();
                            span_width_label(&painter, span_rect, span_rect.x_range().center(), Align::Center, &font_id, font_color, text);
                        }
                    }
                }
            }

            prev_idx = idx1;
        });
    }

    let snap_to_time = snap_to_idx.map(|idx| {
        idx_scale.t_from_idx(idx)
    });

    TimelineResponse {
        snap_to_time,
    }
}

fn scan(
    idx_scale: &IdxScale,
    view: &IntView,
    range: IdxRange,
    eq: impl Fn(u64, u64) -> bool,
    mut render: impl FnMut(f32, f32, Option<Idx>, Option<Idx>, u64)
) {
    let mut prev_v_opt: Option<u64> = None;
    let mut idx1 = None;
    let mut x1 = idx_scale.x_from_idx(idx_scale.visible.min);

    view.for_each_elem(range, |idx, value| {
        if let Some(value) = value {
            let next_v = value;

            if let Some(prev_v) = prev_v_opt {
                if !eq(prev_v, next_v) {
                    let x2: f32 = idx_scale.x_from_idx(idx);
                    render(x1, x2, idx1, Some(idx), prev_v);
                    x1 = x2;
                    idx1 = Some(idx);
                }
            }        
        }
        prev_v_opt = value;
    });

    if let Some(prev_v) = prev_v_opt {
        let x2 = idx_scale.x_from_idx(range.max);
        render(x1, x2, idx1, None, prev_v);
    }
}

fn span_width_label(
    painter: &Painter,
    rect: Rect,
    text_x: f32,
    anchor: Align,
    font_id: &FontId,
    color: Color32,
    text: String
) {
    let text_rect = painter.text(
        Pos2::new(text_x, rect.y_range().center()),
        Align2([anchor, Align::Center]),
        text,
        font_id.clone(), 
        color,
    );

    let stroke = Stroke::new(1.0, color.gamma_multiply(0.5));
    h_arrow(painter, text_rect.left(), rect.left(), rect.y_range().center(), stroke);
    h_arrow(painter, text_rect.right(), rect.right(), rect.y_range().center(), stroke);
}

fn h_arrow(painter: &Painter, x1: f32, x2: f32, y: f32, stroke: Stroke) {
    let pad = 4.0;
    let arrow_size = 4.0;
    let range = Rangef::new(x1, x2).as_positive().shrink(pad);
    painter.hline(range, y, stroke);
    
    let sig = (x1 - x2).signum(); // x vector towards middle
    let tip = Pos2::new(x2 + pad * sig, y);
    painter.line_segment([tip, tip + Vec2::new(sig * arrow_size, arrow_size)], stroke);
    painter.line_segment([tip, tip + Vec2::new(sig * arrow_size, -arrow_size)], stroke);
}
