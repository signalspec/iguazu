use ecow::EcoString;
use egui::{emath::GuiRounding, Pos2, Rangef, Rect, Stroke, Vec2};
use iguazu::{schema::{attribute::{AccentColor, NumberRange}, Field, Summary}, stream::ArcStream, view::{RangeElement, RangeView}, IdxRange};
use indexmap::IndexMap;

use crate::{color::named_color, Time, TimeRange, ViewerContext};

use super::{label_frame, stream_rect, TimelineResponse};

struct YScale {
    scale: f64,
    offset: f32,
}

impl YScale {
    fn new(v_range: NumberRange, y_range: Rangef) -> Self {
        let scale = -1.0 * y_range.span() as f64 / (v_range.max - v_range.min);
        let offset = y_range.max - (v_range.min * scale) as f32;

        YScale {
            scale,
            offset,
        }
    }

    fn y_from_value(&self, value: f64) -> f32 {
        (value * self.scale) as f32 + self.offset
    }
}

pub(crate) struct YAxisRow<'a> {
    sample_rate: f64,
    y_range: NumberRange,
    view: RangeView<'a>,
    color: AccentColor,
    label: EcoString,
}

impl<'a> YAxisRow<'a> {
    pub fn field(vcx: &'a ViewerContext, stream: &ArcStream, sample_rate: f64, offset: u8, color: Option<AccentColor>, label: EcoString, field: &Field, summaries: &IndexMap<EcoString, Summary<ArcStream>>) -> Option<YAxisRow<'a>> {
        if offset != 0 {
            return None;
        }
        let summary = summaries.get("range").unwrap_or(const { &Summary::empty() });
        let view = RangeView::new(&vcx.view_manager, stream, field, summary)?;
        let color = color.unwrap_or(AccentColor::Green);
        let y_range = field.number_range()?;

        Some(YAxisRow {
            sample_rate,
            y_range,
            view,
            color,
            label,
        })
    }
    
    pub fn time_range(&self) -> TimeRange {
        TimeRange {
            min: Time::ZERO,
            max: (self.view.state().end as i128) * Time::period_float(self.sample_rate),
        }
    }

    pub fn render(&self, ui: &mut egui::Ui, scale: &super::scale::Scale) -> TimelineResponse{
        label_frame(ui, |ui| {
            ui.label(self.label.as_str());
        });
        let rect = stream_rect(ui, scale);

        let stroke_width = 1.0;

        let padded_rect = rect.shrink2(Vec2::new(0.0, 8.0 + stroke_width * 2.0));

        if !ui.is_rect_visible(padded_rect) {
            return TimelineResponse::default();
        }

        let idx_scale = scale.idx_scale(self.sample_rate);

        let color = named_color(self.color);

        let painter = ui.painter_at(rect);
        let stroke = Stroke::new(stroke_width, color);

        let state = self.view.state();
        let x_range = IdxRange {
            min: idx_scale.visible.min,
            max: (idx_scale.visible.max + 1).min(state.end),
        };

        let y_scale = YScale::new(self.y_range, padded_rect.y_range());
        draw_y_ticks(ui, self.y_range, &y_scale, rect);

        let mut last: Option<Pos2> = None;

        let dot_opacity = ((idx_scale.points_per_index() - 4.0 * stroke_width) / 8.0).clamp(0.0, 1.0);
        let dot_color = color.gamma_multiply(dot_opacity);

        let min_dist_sq = 4.0 / ui.ctx().pixels_per_point() / ui.ctx().pixels_per_point();

        let min_width = idx_scale.min_visible_width(ui.pixels_per_point());

        self.view.for_each_elem(x_range, min_width, |e| {
            match e {
                RangeElement::Loading(_) => {
                    last = None;
                },
                RangeElement::Single(idx, val) => {
                    let pos = Pos2 {
                        x: idx_scale.x_from_idx(idx),
                        y: y_scale.y_from_value(val),
                    };

                    if dot_opacity > 0.0 {
                        painter.circle_filled(pos, stroke_width * 2.0, dot_color);
                    }

                    if let Some(lpos) = last {
                        if lpos.distance_sq(pos) < min_dist_sq {
                            // Lines that are too short are not rendered by egui, so we skip them
                            // to ensure that the line is continuous and to reduce overdraw.
                            return;
                        }
                        painter.line_segment([lpos, pos], stroke);
                    }

                    last = Some(pos);
                }
                RangeElement::Range(idx, min, max) => {
                    let pos1 = Pos2 {
                        x: idx_scale.x_from_idx(idx.min),
                        y: y_scale.y_from_value(min),
                    };
                    let pos2 = Pos2 {
                        x: idx_scale.x_from_idx(idx.max),
                        y: y_scale.y_from_value(max),
                    };

                    if let Some(lpos) = last {
                        if lpos.distance_sq(pos1) > min_dist_sq {
                            painter.line_segment([lpos, pos1], stroke);
                        }
                    }

                    painter.line_segment([pos1, pos2], stroke);

                    last = Some(pos2);
                },
            }
        });

        TimelineResponse {
            snap_to_time: None,
        }

        }
}

fn generate_ticks(v_range: NumberRange, desired_tick_count: f64) -> impl Iterator<Item = f64> {
    let v_span = (v_range.max - v_range.min).abs();
    let mut step = 10.0f64.powf((v_span / desired_tick_count).log10().floor());

    let err = desired_tick_count / v_span * step;
    if err < 0.15 {
        step *= 10.0;
    } else if err < 0.35 {
        step *= 5.0;
    } else if err < 0.75 {
        step *= 2.0;
    }

    let tick_min_i = (v_range.min / step).ceil() as i64;
    let tick_max_i = (v_range.max / step).floor() as i64;

    (tick_min_i..=tick_max_i).map(move |i| i as f64 * step)
}

#[test]
fn test_generate_ticks() {
    assert_eq!(
        generate_ticks(NumberRange { min: 0.0, max: 100.0 }, 10.0).collect::<Vec<_>>(),
        vec![0.0, 10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0, 90.0, 100.0]
    );

    assert_eq!(
        generate_ticks(NumberRange { min: -1.0, max: 1.0 }, 4.0).collect::<Vec<_>>(),
        vec![-1.0, -0.5, 0.0, 0.5, 1.0]
    );

    assert_eq!(
        generate_ticks(NumberRange { min: -10.0, max: 10.0 }, 8.0).collect::<Vec<_>>(),
        vec![-10.0, -8.0, -6.0, -4.0, -2.0, 0.0, 2.0, 4.0, 6.0, 8.0, 10.0]
    );
}

fn draw_y_ticks(ui: &mut egui::Ui, v_range: NumberRange, y_scale: &YScale, rect: Rect) {
    let desired_tick_spacing = 60.0;
    let desired_tick_count = (rect.height() / desired_tick_spacing) as f64;

    let painter = ui.painter_at(rect);
    let stroke = Stroke::new(1.0, ui.ctx().style().visuals.widgets.noninteractive.bg_stroke.color);
    let zero_stroke = Stroke::new(1.0, ui.ctx().style().visuals.widgets.noninteractive.fg_stroke.color);

    for v in generate_ticks(v_range, desired_tick_count) {
        let y = y_scale.y_from_value(v).round_to_pixel_center(ui.pixels_per_point());
        painter.hline(rect.x_range(), y, if v == 0.0 { zero_stroke } else { stroke });
    }
}
