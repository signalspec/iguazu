use egui::{Align2, Color32, Painter, Pos2, Rangef, Rect, Stroke, Ui, Vec2};
use iguazu::{schema::{attribute::{AccentColor, LogicLevel}, Field, TextFormat}, stream::{Cache, FieldVal}, IdxRange};

use crate::{color::named_color, ViewerContext};

use super::{fixed_height_header, scale::Scale, Ref};

pub(crate) fn render(
    ctx: &mut ViewerContext,
    ui: &mut Ui,
    scale: &Scale,
    label: Option<&str>,
    entity: Ref,
) {
    let rect = fixed_height_header(ui, scale, label);
    let padded_rect = rect.shrink2(Vec2::new(0.0, 2.0));
    if !ui.is_rect_visible(padded_rect) {
        return;
    }

    let Ref::Field {
        data,
        field,
        bit_offset,
        sample_rate: Some(sample_rate),
    } = entity else {
        return;
    };

    let bit_width = field.kind.bit_width();

    let idx_scale = scale.idx_scale(sample_rate.0);

    let color = named_color(
        field
            .attribute::<AccentColor>()
            .unwrap_or(AccentColor::Green),
    );

    let logic_levels = match field.kind {
        Field::Tagged { tag_bits, ref values } if bit_width <= 64 && bit_width == tag_bits => {
            values.values().map(|variant| variant.attribute::<LogicLevel>()).collect()
        }
        _ => Vec::new()
    };

    let text_format = TextFormat::new(0, field);
    let font_id = egui::TextStyle::Small.resolve(ui.style());
    let font_color = ui.style().visuals.text_color();

    let painter = ui.painter_at(rect);
    let stroke_width = 1.0;
    let stroke = Stroke::new(stroke_width, color);

    let state = data.state();
    let mut view = Cache::new(data.clone());
    view.set_range(IdxRange {
        min: idx_scale.visible.min,
        max: idx_scale.visible.max.min(state.end),
    });

    let mut prev_v_opt: Option<FieldVal> = None;
    let mut x1 = idx_scale.x_offset;

    view.for_each_elem(|idx, value| {
        if let Some(value) = value {
            let next_v = value.field(bit_offset, bit_width);

            if let Some(prev_v) = prev_v_opt {
                if !prev_v.eq(&next_v) {
                    let x2: f32 = idx_scale.x_from_idx(idx);
                    render_span(&painter, prev_v, padded_rect, x1, x2, stroke, &logic_levels, &text_format, &font_id, font_color);
                    x1 = x2;
                }
            }
            
            prev_v_opt = Some(next_v);
        }
    });

    if let Some(prev_v) = prev_v_opt {
        let x2 = idx_scale.x_from_idx(view.range().max);
        render_span(&painter, prev_v, padded_rect, x1, x2, stroke, &logic_levels, &text_format, &font_id, font_color);
    }
}

fn render_span(painter: &Painter, value: FieldVal, padded_rect: Rect, x1: f32, x2: f32, stroke: Stroke, logic_levels: &[Option<LogicLevel>], text_format: &TextFormat, font_id: &egui::FontId, text_color: Color32) {
    let logic_level = if !logic_levels.is_empty() {
        logic_levels.get(value.as_u64() as usize).copied().unwrap_or(None)
    } else { None };

    
    match logic_level {
        Some(LogicLevel::Low) => {
            painter.hline(x1..=x2, padded_rect.bottom(), stroke);

        }
        Some(LogicLevel::High) => {
            painter.hline(x1..=x2, padded_rect.top(), stroke);
        }
        None => {
            let tx = x1.max(padded_rect.left()) + 5.0;
            if x2 - tx > 10.0 {
                let text = text_format.format(value).to_string();
                painter.with_clip_rect(Rect::from_x_y_ranges(Rangef::new(tx, x2 - 5.0), padded_rect.y_range()))
                    .text(Pos2::new(tx, padded_rect.y_range().center()), Align2::LEFT_CENTER, text, font_id.clone(), text_color);
            }
            painter.hline(x1..=x2, padded_rect.bottom(), stroke);
            painter.hline(x1..=x2, padded_rect.top(), stroke);
        }
    }

    painter.vline(x2, padded_rect.y_range(), stroke);

}
