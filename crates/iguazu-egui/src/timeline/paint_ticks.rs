use num_traits::cast::ToPrimitive;
use std::ops::RangeInclusive;

use egui::{lerp, pos2, remap_clamp, Align2, Color32, Rect, Rgba, Shape, Stroke};

use crate::{Time, IdxF, TimeType};

use crate::format_time::next_grid_tick_magnitude_ns;

use super::Scale;

pub fn paint_time_ranges_and_ticks(
    time_ranges_ui: &Scale,
    ui: &mut egui::Ui,
    time_area_painter: &egui::Painter,
    line_y_range: RangeInclusive<f32>,
    time_type: TimeType,
) {
    time_area_painter
        .extend(paint_time_range_ticks(ui, time_ranges_ui, line_y_range, time_type));

}

fn min_tick_ns_for_period(period: f64) -> u64 {
    10u64.pow((period.log10() + 9.0).floor().clamp(0.0, 19.0) as u32)
}

fn paint_time_range_ticks(
    ui: &mut egui::Ui,
    scale: &Scale,
    line_y_range: RangeInclusive<f32>,
    time_type: TimeType,
) -> Vec<Shape> {
    let font_id = egui::TextStyle::Small.resolve(ui.style());

    let clip_rect = ui.clip_rect();

    let min_idx = scale.clamp_time(scale.idx_from_x(clip_rect.min.x)).floor();
    let max_idx = scale.clamp_time(scale.idx_from_x(clip_rect.max.x)).ceil();

    match time_type {
        TimeType::Absolute { period, ..} | TimeType::Relative { period } => {
            let period = period.to_f64().unwrap();
            let min_tick_size_ns = min_tick_ns_for_period(period);

            let min_tick = (min_idx as f64 * period * 1e9).floor() as u64;
            let max_tick = (max_idx as f64 * period * 1e9).ceil() as u64;
            let tick_range = min_tick..=max_tick;

            paint_ticks(
                ui.ctx(),
                (scale.x_scale as f64 / (period * 1e9)) as f32,
                tick_range,
                min_tick_size_ns,
                ui.visuals().dark_mode,
                &font_id,
                line_y_range,
                &clip_rect,
                &|t| scale.x_from_idx(IdxF::from(t) * (1.0 / (period * 1e9))),
                next_grid_tick_magnitude_ns,
                |ns| crate::format_time::format_time_compact(Time::from_ns_since_epoch(ns as i64)),
            )
        }
        TimeType::Sequence => {
            fn next_power_of_10(i: u64) -> u64 {
                i * 10
            }
            paint_ticks(
                ui.ctx(),
                scale.x_scale as f32,
                min_idx..=max_idx,
                1,
                ui.visuals().dark_mode,
                &font_id,
                line_y_range,
                &clip_rect,
                &|i| scale.x_from_idx(i.into()),
                next_power_of_10,
                |seq| format!("#{seq}"),
            )
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_ticks(
    egui_ctx: &egui::Context,
    points_per_unit: f32,
    tick_range: RangeInclusive<u64>,
    min_tick_size: u64,
    dark_mode: bool,
    font_id: &egui::FontId,
    line_y_range: RangeInclusive<f32>,
    clip_rect: &Rect,
    x_from_time: &dyn Fn(u64) -> f32,
    next_time_step: fn(u64) -> u64,
    format_tick: fn(u64) -> String,
) -> Vec<egui::Shape> {
    let color_from_alpha = |alpha: f32| -> Color32 {
        if dark_mode {
            Rgba::from_white_alpha(alpha * alpha).into()
        } else {
            Rgba::from_black_alpha(alpha).into()
        }
    };

    let mut shapes = vec![];

    let minimum_small_line_spacing = 4.0;
    let expected_text_width = 60.0;

    let line_strength_from_spacing = |spacing_time: u64| -> f32 {
        let next_tick_magnitude = next_time_step(spacing_time) / spacing_time;
        remap_clamp(
            spacing_time as f32 * points_per_unit as f32,
            minimum_small_line_spacing..=(next_tick_magnitude as f32 * minimum_small_line_spacing),
            0.0..=1.0,
        )
    };

    let text_color_from_spacing = |spacing_time: u64| -> Color32 {
        let alpha = remap_clamp(
            spacing_time as f32 * points_per_unit as f32,
            expected_text_width..=(3.0 * expected_text_width),
            0.0..=0.5,
        );
        color_from_alpha(alpha)
    };

    let mut small_spacing_time = min_tick_size;
    while (small_spacing_time as f32 * points_per_unit) < minimum_small_line_spacing {
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

    let mut current_time = tick_range.start() / small_spacing_time * small_spacing_time;
    let max_time = *tick_range.end();

    while current_time <= max_time {
        let line_x = x_from_time(current_time);

        if clip_rect.min.x <= line_x && line_x <= clip_rect.max.x {
            let medium_line = current_time % medium_spacing_time == 0;
            let big_line = current_time % big_spacing_time == 0;

            let (height_factor, line_color, text_color) = if big_line {
                (medium_line_strength, big_line_color, big_text_color)
            } else if medium_line {
                (small_line_strength, medium_line_color, medium_text_color)
            } else {
                (0.0, small_line_color, small_text_color)
            };

            // Make line higher if it is stronger:
            let line_top = lerp(line_y_range.clone(), lerp(0.75..=0.5, height_factor));

            shapes.push(egui::Shape::line_segment(
                [pos2(line_x, line_top), pos2(line_x, *line_y_range.end())],
                Stroke::new(1.0, line_color),
            ));

            if text_color != Color32::TRANSPARENT {
                let text = format_tick(current_time);
                let text_x = line_x + 4.0;

                egui_ctx.fonts(|fonts| {
                    shapes.push(egui::Shape::text(
                        fonts,
                        pos2(text_x, lerp(line_y_range.clone(), 0.5)),
                        Align2::LEFT_CENTER,
                        &text,
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