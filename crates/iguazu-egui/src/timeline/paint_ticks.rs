// Based on rerun.io: © 2023 Rerun Technologies AB under MIT OR Apache-2.0
use egui::{lerp, pos2, remap_clamp, Align2, Color32, Rect, Rgba, Stroke, Rangef};

use iguazu::time::{DateStep, Time, TimeRange, next_time_step};
use jiff::{SignedDuration, Zoned};

use super::Scale;

const MIN_TICK_SIZE: Time = Time::PICOSECOND;

struct Ticks {
    small_spacing_time: Time,
    medium_spacing_time: Time,
    big_spacing_time: Time,
    ticks: Vec<Tick>,
}

struct Tick {
    kind: TickKind,
    time: Time,
    text: String,
}

enum TickKind {
    Small,
    Medium,
    Big,
}

fn relative_ticks(minimum_small_line_time: Time, range: TimeRange) -> Ticks {
    let mut small_spacing_time = MIN_TICK_SIZE;
    while small_spacing_time < minimum_small_line_time {
        small_spacing_time = next_time_step(small_spacing_time);
    }
    let medium_spacing_time = next_time_step(small_spacing_time);
    let big_spacing_time = next_time_step(medium_spacing_time);

    let mut ticks = Vec::new();
    let mut time = range.min / big_spacing_time * big_spacing_time;

    while time <= range.max {
        let kind = if time % big_spacing_time == Time::ZERO {
            TickKind::Big
        } else if time % medium_spacing_time == Time::ZERO {
            TickKind::Medium
        } else {
            TickKind::Small
        };

        let text = match kind {
            TickKind::Small => String::new(),
            TickKind::Medium => time.format_relative(medium_spacing_time).to_string(),
            TickKind::Big => time.format_relative(big_spacing_time).to_string(),
        };

        ticks.push(Tick { kind, time, text });

        time += small_spacing_time;
    }

    Ticks { small_spacing_time, medium_spacing_time, big_spacing_time, ticks }
}

fn date_ticks(minimum_small_line_time: Time, range: TimeRange, epoch: Zoned) -> Option<Ticks> {
    let mut small_step = DateStep::smallest();
    while small_step.approx_time() < minimum_small_line_time {
        small_step = small_step.next();
    }
    let medium_step = small_step.next();
    let big_step = medium_step.next();

    let mut ticks = Vec::new();
    let start = epoch.checked_add(SignedDuration::try_from(range.min).ok()?).ok()?;
    let mut t = big_step.floor_date(&start).inspect_err(|e| { dbg!(e); }).ok()?;

    let small_span = small_step.as_span();
    let medium_span = medium_step.as_span();
    let big_span = big_step.as_span();

    let mut next_medium = t.clone();
    let mut next_big = t.clone();

    loop {
        t = t.checked_add(small_span).ok()?;
        let kind = if t >= next_big {
            t = next_big;
            next_medium = t.checked_add(medium_span).ok()?;
            next_big = t.checked_add(big_span).ok()?;
            TickKind::Big
        } else if t >= next_medium {
            t = next_medium;
            next_medium = t.checked_add(medium_span).ok()?;
            TickKind::Medium
        } else {
            TickKind::Small
        };

        let time = Time::from(t.duration_since(&epoch));

        if time > range.max {
            break;
        }

        let text = match kind {
            TickKind::Small => String::new(),
            TickKind::Medium => medium_step.format(&t),
            TickKind::Big => big_step.format(&t),
        };

        ticks.push(Tick { kind, time, text })
    }

    Some(Ticks {
        small_spacing_time: small_step.approx_time(),
        medium_spacing_time: medium_step.approx_time(),
        big_spacing_time: big_step.approx_time(),
        ticks
    })
}

pub fn paint_time_ticks(
    ui: &mut egui::Ui,
    scale: &Scale,
    line_y_range: Rangef,
    epoch: Option<Zoned>,
) {
    let clip_rect = Rect::from_x_y_ranges(scale.x_range, line_y_range);
    let font_id = egui::TextStyle::Body.resolve(ui.style());

    let egui_ctx = ui.ctx();
    let dark_mode = ui.visuals().dark_mode;

    if scale.view_range.length() < MIN_TICK_SIZE || scale.x_width() <= 0.0 {
        return; // or we'd infinite loop
    }
    let visible = scale.clamped_visible();
    let color_from_alpha = |alpha: f32| -> Color32 {
        if dark_mode {
            Rgba::from_white_alpha(alpha).into()
        } else {
            Rgba::from_black_alpha(alpha).into()
        }
    };

    let minimum_small_line_spacing = 4.0;
    let minimum_small_line_time = scale.points_to_time(minimum_small_line_spacing);
    let expected_text_width = 60.0;

    let line_strength_from_spacing = |spacing_time: Time, next_spacing_time: Time| -> f32 {
        let next_tick_magnitude = next_spacing_time / spacing_time;
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

    let ticks = if let Some(epoch) = epoch {
        date_ticks(minimum_small_line_time, visible, epoch)
    } else {
        Some(relative_ticks(minimum_small_line_time, visible))
    };

    let Some(ticks) = ticks else { return };

    // We fade in lines as we zoom in:
    let medium_line_strength = line_strength_from_spacing(ticks.medium_spacing_time, ticks.big_spacing_time);
    let small_line_strength = line_strength_from_spacing(ticks.small_spacing_time, ticks.medium_spacing_time);

    let big_line_color = color_from_alpha(0.4);
    let medium_line_color = color_from_alpha(0.4 * medium_line_strength);
    let small_line_color = color_from_alpha(0.4 * small_line_strength);

    let big_text_color = text_color_from_spacing(ticks.big_spacing_time);
    let medium_text_color = text_color_from_spacing(ticks.medium_spacing_time);

    let clip_x = clip_rect.x_range().expand(expected_text_width);

    let mut shapes = vec![];

    for tick in ticks.ticks {
        let line_x = scale.x_from_t(tick.time);

        if clip_x.contains(line_x) {
            let (height_factor, line_color, text_color) = match tick.kind {
                TickKind::Small => (0.0, small_line_color, Color32::TRANSPARENT),
                TickKind::Medium => (small_line_strength, medium_line_color, medium_text_color),
                TickKind::Big => (medium_line_strength, big_line_color, big_text_color),
            };

            // Make line higher if it is stronger:
            let line_top = lerp(line_y_range, lerp(0.75..=0.5, height_factor));

            shapes.push(egui::Shape::line_segment(
                [pos2(line_x, line_top), pos2(line_x, line_y_range.max)],
                Stroke::new(1.0, line_color),
            ));

            if text_color != Color32::TRANSPARENT {
                let text_x = line_x + 4.0;

                egui_ctx.fonts_mut(|fonts| {
                    shapes.push(egui::Shape::text(
                        fonts,
                        pos2(text_x, lerp(line_y_range, 0.5)),
                        Align2::LEFT_CENTER,
                        tick.text,
                        font_id.clone(),
                        text_color,
                    ));
                });
            }
        }
    }

    ui.painter_at(clip_rect).extend(shapes);
}
