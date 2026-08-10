use ecow::EcoString;
use egui::{emath::GuiRounding, Pos2, Rangef, Rect, Stroke, Vec2};
use iguazu::{IdxRange, schema::{FieldRef, attribute::display::AccentColor}, time::{Time, TimeRange}, view::{RangeView, Span, Timebase}};

use crate::{color::named_color, ViewerContext};

use super::{TimelineResponse, label_frame, scale::Scale, stream_rect};

struct YScale {
    scale: f64,
    offset: f32,
}

impl YScale {
    fn new(v_range: (f64, f64), y_range: Rangef) -> Self {
        let scale = -1.0 * y_range.span() as f64 / (v_range.1 - v_range.0);
        let offset = y_range.max - (v_range.0 * scale) as f32;

        YScale {
            scale,
            offset,
        }
    }

    fn y_from_value(&self, value: f64) -> f32 {
        (value * self.scale) as f32 + self.offset
    }
}

const STROKE_WIDTH: f32 = 1.0;

pub(crate) struct YAxisRow<'a> {
    timebase: Timebase<'a>,
    y_range: (f64, f64),
    view: RangeView<'a>,
    color: AccentColor,
    label: EcoString,
}

impl<'a> YAxisRow<'a> {
    pub fn field(vcx: &'a ViewerContext, field: FieldRef<'_>, timebase: &Timebase<'a>, color: Option<AccentColor>, label: EcoString) -> Option<YAxisRow<'a>> {
        let view = RangeView::new(&vcx.view_manager, field)?;
        let color = color.unwrap_or(AccentColor::Green);
        let y_range = field.field.number_range().or(view.bounds())?;

        if !y_range.0.is_finite() || !y_range.1.is_finite() || y_range.0 >= y_range.1 {
            return None;
        }

        Some(YAxisRow {
            timebase: timebase.clone(),
            y_range,
            view,
            color,
            label,
        })
    }

    pub fn time_range(&self) -> Option<TimeRange> {
        match &self.timebase {
            Timebase::Fixed(sample_rate) => Some(TimeRange {
                min: Time::ZERO,
                max: (self.view.state().end as i128) * Time::period_float(*sample_rate),
            }),
            Timebase::Nonuniform(v) => {
                v.time_range()
            }
        }
    }

    pub fn render(&self, ui: &mut egui::Ui, scale: &Scale) -> TimelineResponse{
        label_frame(ui, |ui| {
            ui.label(self.label.as_str());
        });
        let rect = stream_rect(ui, scale);

        let padded_rect = rect.shrink2(Vec2::new(0.0, 8.0 + STROKE_WIDTH * 2.0));

        if !ui.is_rect_visible(padded_rect) {
            return TimelineResponse::default();
        }

        let color = named_color(self.color);

        let painter = ui.painter_at(rect);

        let y_scale = YScale::new(self.y_range, padded_rect.y_range());
        draw_y_ticks(ui, self.y_range, &y_scale, rect);

        match self.timebase {
            Timebase::Fixed(sample_rate) => {
                self.render_fixed_rate(ui, painter, scale, sample_rate, y_scale, color);
            }
            Timebase::Nonuniform(ref time_view) => {
                self.render_nonuniform(ui, painter, scale, time_view, y_scale, color);
            }
        }



        TimelineResponse {
            snap_to_time: None,
        }
    }

    fn render_fixed_rate(&self,
        ui: &mut egui::Ui,
        painter: egui::Painter,
        scale: &Scale,
        sample_rate: f64,
        y_scale: YScale,
        color: egui::Color32,
    ) {
        let idx_scale = scale.idx_scale(sample_rate);
        let state = self.view.state();
        let x_range = IdxRange {
            min: idx_scale.visible.min,
            max: (idx_scale.visible.max + 1).min(state.end),
        };

        let level = idx_scale.min_visible_width_log2(ui.pixels_per_point());
        let stroke = Stroke::new(STROKE_WIDTH, color);

        let mut last: Option<Pos2> = None;

        if level == 0 {
            let (dot_opacity, dot_color) = get_dot_opacity(color, idx_scale.points_per_index());

            for (idx, val) in x_range.into_iter().zip(self.view.iter_base(x_range)) {
                let Some(val) = val else {
                    last = None;
                    continue;
                };

                let pos = Pos2 {
                    x: idx_scale.x_from_idx(idx),
                    y: y_scale.y_from_value(val),
                };

                if dot_opacity > 0.0 {
                    painter.circle_filled(pos, STROKE_WIDTH * 2.0, dot_color);
                }

                if let Some(lpos) = last {
                    painter.line_segment([lpos, pos], stroke);
                }

                last = Some(pos);
            }
        } else {
            for idx in x_range.into_iter().step_by(1<<level) {
                let Some((min, max)) = self.view.get_at_level(level, idx) else {
                    last = None;
                    continue;
                };

                let pos1 = Pos2 {
                    x: idx_scale.x_from_idx(idx),
                    y: y_scale.y_from_value(min),
                };
                let pos2 = Pos2 {
                    x: idx_scale.x_from_idx(idx + (1<<level)),
                    y: y_scale.y_from_value(max),
                };

                if let Some(lpos) = last {
                    painter.line_segment([lpos, pos1], stroke);
                }

                last = Some(pos2);

                painter.line_segment([pos1, pos2], stroke);
            }
        }
    }

    fn render_nonuniform(
        &self,
        ui: &mut egui::Ui,
        painter: egui::Painter,
        scale: &Scale,
        time_view: &iguazu::view::TimestampView<'_>,
        y_scale: YScale,
        color: egui::Color32
    ) {
        let idx_scale = scale.idx_scale(time_view.time_rate());
        let visible_t_range = IdxRange {
            min: idx_scale.visible.min,
            max: idx_scale.visible.max + 1,
        };

        let stroke = Stroke::new(STROKE_WIDTH, color);
        let min_visible = idx_scale.min_visible_width(ui.pixels_per_point());

        let mut last: Option<Pos2> = None;
        let mut last_dx = f32::INFINITY;

        for span in time_view.iter(visible_t_range.into(), min_visible.get()) {
            match span {
                Span::Loading => {
                    last = None;
                    last_dx = f32::INFINITY;
                }
                Span::Sparse(idx_range, t_range) | Span::Dense(0, idx_range, t_range) => {
                    let (Some(val1), Some(val2)) = (self.view.get_base(idx_range.min), self.view.get_base(idx_range.max)) else {
                        last = None;
                        last_dx = f32::INFINITY;
                        continue;
                    };

                    let pos1 = Pos2 {
                        x: idx_scale.x_from_idx(t_range.start),
                        y: y_scale.y_from_value(val1),
                    };
                    let pos2 = Pos2 {
                        x: idx_scale.x_from_idx(t_range.end),
                        y: y_scale.y_from_value(val2),
                    };

                    painter.line_segment([pos1, pos2], stroke);

                    let dx = pos2.x - pos1.x;
                    let (dot_opacity, dot_color) = get_dot_opacity(color, dx.min(last_dx));

                    if dot_opacity > 0.0 {
                        painter.circle_filled(pos1, STROKE_WIDTH * 2.0, dot_color);
                    }

                    last_dx = dx;
                    last = Some(pos2);
                }
                Span::Dense(level, idx_range, t_range) => {
                    let Some((min, max)) = self.view.get_at_level(level, idx_range.min) else {
                        last = None;
                        last_dx = f32::INFINITY;
                        continue;
                    };

                    let pos1 = Pos2 {
                        x: idx_scale.x_from_idx(t_range.start),
                        y: y_scale.y_from_value(min),
                    };
                    let pos2 = Pos2 {
                        x: idx_scale.x_from_idx(t_range.end),
                        y: y_scale.y_from_value(max),
                    };

                    if let Some(lpos) = last {
                        painter.line_segment([lpos, pos1], stroke);
                    }

                    last = Some(pos2);
                    last_dx = 0.0;

                    painter.line_segment([pos1, pos2], stroke);
                }
            }
        }

        if let Some(lpos) = last {
            let (dot_opacity, dot_color) = get_dot_opacity(color, last_dx);

            if dot_opacity > 0.0 {
                painter.circle_filled(lpos, STROKE_WIDTH * 2.0, dot_color);
            }
        }
    }
}

fn get_dot_opacity(color: egui::Color32, dx: f32) -> (f32, egui::Color32) {
    let dot_opacity = ((dx - 4.0 * STROKE_WIDTH) / 8.0).clamp(0.0, 1.0);
    let dot_color = color.gamma_multiply(dot_opacity);
    (dot_opacity, dot_color)
}

fn generate_ticks(v_range: (f64, f64), desired_tick_count: f64) -> impl Iterator<Item = f64> {
    let v_span = (v_range.1 - v_range.0).abs();
    let mut step = 10.0f64.powf((v_span / desired_tick_count).log10().floor());

    let err = desired_tick_count / v_span * step;
    if err < 0.15 {
        step *= 10.0;
    } else if err < 0.35 {
        step *= 5.0;
    } else if err < 0.75 {
        step *= 2.0;
    }

    let tick_min_i = (v_range.0 / step).ceil() as i64;
    let tick_max_i = (v_range.1 / step).floor() as i64;

    (tick_min_i..=tick_max_i).map(move |i| i as f64 * step)
}

#[test]
fn test_generate_ticks() {
    assert_eq!(
        generate_ticks((0.0, 100.0), 10.0).collect::<Vec<_>>(),
        vec![0.0, 10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0, 90.0, 100.0]
    );

    assert_eq!(
        generate_ticks((-1.0, 1.0), 4.0).collect::<Vec<_>>(),
        vec![-1.0, -0.5, 0.0, 0.5, 1.0]
    );

    assert_eq!(
        generate_ticks((-10.0, 10.0), 8.0).collect::<Vec<_>>(),
        vec![-10.0, -8.0, -6.0, -4.0, -2.0, 0.0, 2.0, 4.0, 6.0, 8.0, 10.0]
    );
}

fn draw_y_ticks(ui: &mut egui::Ui, v_range: (f64, f64), y_scale: &YScale, rect: Rect) {
    let desired_tick_spacing = 60.0;
    let desired_tick_count = (rect.height() / desired_tick_spacing) as f64;

    let painter = ui.painter_at(rect);
    let stroke = Stroke::new(1.0, ui.style().visuals.widgets.noninteractive.bg_stroke.color);
    let zero_stroke = Stroke::new(1.0, ui.style().visuals.widgets.noninteractive.fg_stroke.color);

    for v in generate_ticks(v_range, desired_tick_count) {
        let y = y_scale.y_from_value(v).round_to_pixel_center(ui.pixels_per_point());
        painter.hline(rect.x_range(), y, if v == 0.0 { zero_stroke } else { stroke });
    }
}
