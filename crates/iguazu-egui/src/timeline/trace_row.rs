use egui::{Align2, Pos2, Rangef, Rect, Stroke, Ui, Vec2};
use iguazu::{schema::{attribute::{AccentColor, SampleRate}, EntityKind, EntityStream}, view::{View, ViewManager}, IdxRange};

use crate::{cache::ViewCache, color::named_color, ViewerContext};

use super::{fixed_height_header, scale::{IdxScale, Scale}};

pub(crate) fn render(
    _ctx: &mut ViewerContext,
    ui: &mut Ui,
    scale: &Scale,
    label: Option<&str>,
    entity: &EntityStream,
) {
    let rect = fixed_height_header(ui, scale, label);
    let padded_rect = rect.shrink2(Vec2::new(0.0, 2.0));
    if !ui.is_rect_visible(padded_rect) {
        return;
    }

    let Some(sample_rate) = entity.attribute::<SampleRate>() else { return };

    let idx_scale = scale.idx_scale(sample_rate.0);

    let color = named_color(
        entity
            .attribute::<AccentColor>()
            .unwrap_or(AccentColor::Green),
    );

    let font_id = egui::TextStyle::Small.resolve(ui.style());
    let font_color = ui.style().visuals.text_color();

    let painter = ui.painter_at(rect);
    let stroke_width = 1.0;
    let stroke = Stroke::new(stroke_width, color);

    let state = entity.data.state();
    let range = IdxRange {
        min: idx_scale.visible.min,
        max: idx_scale.visible.max.min(state.end),
    };
    let view = ViewCache::with(ui).view(&entity.data, range);

    scan(&idx_scale, &view, <[u8]>::eq, |x1, x2, val | {
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
}

pub(crate) fn render_logic(
    _ctx: &mut ViewerContext,
    ui: &mut Ui,
    scale: &Scale,
    _label: Option<&str>,
    entity: &EntityStream,
) {
    let EntityKind::Logic { ref bits } = entity.kind else { return };

    let Some(sample_rate) = entity.attribute::<SampleRate>() else { return };
    let idx_scale = scale.idx_scale(sample_rate.0);

    let state = entity.data.state();

    let range = IdxRange {
        min: idx_scale.visible.min,
        max: idx_scale.visible.max.min(state.end),
    };
    let view = ViewCache::with(ui).view(&entity.data, range);

    for (bit, field) in bits.iter().enumerate() {
        let rect = fixed_height_header(ui, scale, Some(&field.name));
        let padded_rect = rect.shrink2(Vec2::new(0.0, 2.0));
        if !ui.is_rect_visible(padded_rect) {
            continue;
        }

        let color = field.attribute::<AccentColor>()
            .unwrap_or(AccentColor::Green);
        let color = named_color(color);

        let painter = ui.painter_at(rect);
        let stroke_width = 1.0;
        let stroke = Stroke::new(stroke_width, color);

        let offset = (bit / 8) as usize;
        let mask = 1 << (bit % 8);
        let eq = |v1: &[u8], v2: &[u8]| {
            v1[offset] & mask == v2[offset] & mask
        };

        scan(&idx_scale, &view, eq, |x1, x2, val | {
            if val[offset] & mask != 0 {
                painter.hline(x1..=x2, padded_rect.top(), stroke);
            } else {
                painter.hline(x1..=x2, padded_rect.bottom(), stroke);
            }

            painter.vline(x2, padded_rect.y_range(), stroke);
        });
    }
}

fn scan(
    idx_scale: &IdxScale,
    view: &View,
    eq: impl Fn(&'_ [u8], &'_ [u8]) -> bool,
    mut render: impl FnMut(f32, f32, &'_ [u8])
) {
    let mut prev_v_opt: Option<&[u8]> = None;
    let mut x1 = idx_scale.x_from_idx(idx_scale.visible.min);

    view.for_each_elem(|idx, value| {
        if let Some(value) = value {
            let next_v = value;

            if let Some(prev_v) = prev_v_opt {
                if !eq(prev_v, next_v) {
                    let x2: f32 = idx_scale.x_from_idx(idx);
                    render(x1, x2, prev_v);
                    x1 = x2;
                }
            }
        
            prev_v_opt = Some(next_v);
        }
    });

    if let Some(prev_v) = prev_v_opt {
        let x2 = idx_scale.x_from_idx(view.range().max);
        render(x1, x2, prev_v);
    }
}
