use egui::{Stroke, Ui, Vec2};
use iguazu::{schema::attribute::AccentColor, stream::{Cache, FieldVal}, IdxRange};

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
        entity
            .attribute::<AccentColor>()
            .unwrap_or(AccentColor::Green),
    );

    let painter = ui.painter_at(rect);
    let stroke_width = 1.0;
    let stroke = Stroke::new(stroke_width, color);

    let state = data.state();
    let mut view = Cache::new(data.clone());
    view.set_range(IdxRange {
        min: idx_scale.visible.min,
        max: idx_scale.visible.max.min(state.end),
    });

    let mut prev_v = view.get(idx_scale.visible.min)
        .unwrap_or(FieldVal::empty())
        .field(bit_offset, bit_width);
    let mut x1 = idx_scale.x_offset;

    view.for_each_elem(|idx, value| {
        if let Some(value) = value {
            let v = value.field(bit_offset, bit_width);
    
            if !prev_v.eq(&v) {
                let x2: f32 = idx_scale.x_from_idx(idx);
    
                let y = if prev_v.as_u64() == 0 {
                    padded_rect.bottom()
                } else {
                    padded_rect.top()
                };
    
                painter.hline(x1..=x2, y, stroke);
                painter.vline(x2, padded_rect.y_range(), stroke);
    
                prev_v = v;
                x1 = x2;
            }
        }
    });

    let x2 = idx_scale.x_from_idx(view.range().max);
    let y = if prev_v.as_u64() == 0 {
        padded_rect.bottom()
    } else {
        padded_rect.top()
    };
    painter.hline(x1..=x2, y, stroke);
}
