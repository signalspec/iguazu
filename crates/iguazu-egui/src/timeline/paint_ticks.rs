// Based on rerun.io: © 2023 Rerun Technologies AB under MIT OR Apache-2.0
use egui::{lerp, pos2, remap_clamp, Align2, Color32, Rect, Rgba, Shape, Stroke, Rangef};

use iguazu::time::{Time, next_time_step};

use super::Scale;

pub(crate) fn paint_time_ranges_and_ticks(
    time_ranges_ui: &Scale,
    ui: &mut egui::Ui,
    time_area_painter: &egui::Painter,
    line_y_range: Rangef,
) {
    time_area_painter
        .extend(paint_time_range_ticks(ui, time_ranges_ui, line_y_range));

}

fn paint_time_range_ticks(
    ui: &mut egui::Ui,
    scale: &Scale,
    line_y_range: Rangef,
) -> Vec<Shape> {
    let font_id = egui::TextStyle::Body.resolve(ui.style());

     paint_ticks(
        ui.ctx(),
        scale,
        ui.visuals().dark_mode,
        &font_id,
        line_y_range,
        &ui.clip_rect()
    )
}

#[allow(clippy::too_many_arguments)]
fn paint_ticks(
    egui_ctx: &egui::Context,
    scale: &Scale,
    dark_mode: bool,
    font_id: &egui::FontId,
    line_y_range: Rangef,
    clip_rect: &Rect,
) -> Vec<egui::Shape> {
    let min_tick_size = Time::NANOSECOND; // TODO: set from max sample rate
    let visible = scale.clamped_visible();

    let color_from_alpha = |alpha: f32| -> Color32 {
        if dark_mode {
            Rgba::from_white_alpha(alpha).into()
        } else {
            Rgba::from_black_alpha(alpha).into()
        }
    };

    let mut shapes = vec![];

    let minimum_small_line_spacing = 4.0;
    let expected_text_width = 60.0;

    let line_strength_from_spacing = |spacing_time: Time| -> f32 {
        let next_tick_magnitude = next_time_step(spacing_time) / spacing_time;
        remap_clamp(
            scale.points_from_time(spacing_time),
            minimum_small_line_spacing..=(next_tick_magnitude as f32 * minimum_small_line_spacing),
            0.0..=1.0,
        )
    };

    let text_color_from_spacing = |spacing_time: Time| -> Color32 {
        let alpha = remap_clamp(
            scale.points_from_time(spacing_time),
            expected_text_width..=(3.0 * expected_text_width),
            0.0..=0.5,
        );
        color_from_alpha(alpha)
    };

    let mut small_spacing_time = min_tick_size;
    while scale.points_from_time(small_spacing_time) < minimum_small_line_spacing {
        small_spacing_time = next_time_step(small_spacing_time);
    }
    let medium_spacing_time = next_time_step(small_spacing_time);
    let big_spacing_time = next_time_step(medium_spacing_time);

    // We fade in lines as we zoom in:
    let big_line_strength = line_strength_from_spacing(big_spacing_time);
    let medium_line_strength = line_strength_from_spacing(medium_spacing_time);
    let small_line_strength = line_strength_from_spacing(small_spacing_time);

    let big_line_color = color_from_alpha(0.4 * big_line_strength);
    let medium_line_color = color_from_alpha(0.4 * medium_line_strength);
    let small_line_color = color_from_alpha(0.4 * small_line_strength);

    let big_text_color = text_color_from_spacing(big_spacing_time);
    let medium_text_color = text_color_from_spacing(medium_spacing_time);
    let small_text_color = text_color_from_spacing(small_spacing_time);

    let mut current_time = visible.min / small_spacing_time * small_spacing_time;

    while current_time <= visible.max {
        let line_x = scale.x_from_t(current_time);

        if clip_rect.x_range().contains(line_x) {
            let medium_line = current_time % medium_spacing_time == Time::ZERO;
            let big_line = current_time % big_spacing_time == Time::ZERO;

            let (height_factor, line_color, text_color, precision) = if big_line {
                (medium_line_strength, big_line_color, big_text_color, big_spacing_time)
            } else if medium_line {
                (small_line_strength, medium_line_color, medium_text_color, medium_spacing_time)
            } else {
                (0.0, small_line_color, small_text_color, small_spacing_time)
            };

            // Make line higher if it is stronger:
            let line_top = lerp(line_y_range, lerp(0.75..=0.5, height_factor));

            shapes.push(egui::Shape::line_segment(
                [pos2(line_x, line_top), pos2(line_x, line_y_range.max)],
                Stroke::new(1.0, line_color),
            ));

            if text_color != Color32::TRANSPARENT {
                let text = format!("{}", current_time.format_relative(precision));
                let text_x = line_x + 4.0;

                egui_ctx.fonts_mut(|fonts| {
                    shapes.push(egui::Shape::text(
                        fonts,
                        pos2(text_x, lerp(line_y_range, 0.5)),
                        Align2::LEFT_CENTER,
                        text,
                        font_id.clone(),
                        text_color,
                    ));
                });
            }
        }

        current_time += small_spacing_time;
    }

    shapes
}
