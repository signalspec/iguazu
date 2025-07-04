use egui::{Align, Align2, Color32, FontId, Painter, Pos2, Rangef, Rect, Stroke, Ui, Vec2};
use iguazu::{schema::{attribute::AccentColor, fmt::ValueFormatter, EntityKind, EntityStream}, view::IntView, Idx, IdxRange};

use crate::{color::named_color, Time, TimeRange, ViewerContext};

use super::{label_frame, scale::IdxScale, stream_rect, TimelineResponse};

pub struct TraceRow<'a> {
    view: IntView<'a>,
    sample_rate: f64,
    formatter: ValueFormatter<'a>,
    label: Option<String>,
    color: AccentColor,
}

impl<'a> TraceRow<'a> {
    pub fn new(vcx: &'a ViewerContext, entity: &'a EntityStream, label: Option<&str>) -> Option<Self> {
        let sample_rate = entity.sample_rate()?;
        let view = vcx.view_manager.int_view(entity)?;
        let color = entity.accent_color().unwrap_or(AccentColor::Green);
        let label = label.map(|s| s.to_string());
        let formatter = entity.formatter()?;

        Some(TraceRow { view, sample_rate, label, color, formatter })
    }

    pub fn time_range(&self) -> TimeRange {
        TimeRange {
            min: Time::ZERO,
            max: (self.view.state().end as i128) * Time::period_float(self.sample_rate),
        }
    }

    pub fn render(&self, ui: &mut Ui, scale: &super::scale::Scale) -> TimelineResponse {
        label_frame(ui, |ui| {
            ui.label(self.label.as_deref().unwrap_or(""));
        });
        let rect = stream_rect(ui, scale);
        let padded_rect = rect.shrink2(Vec2::new(0.0, 8.0));
        if !ui.is_rect_visible(padded_rect) {
            return TimelineResponse::default();
        }
    
        let idx_scale = scale.idx_scale(self.sample_rate);
    
        let color = named_color(self.color);
    
        let font_id = egui::TextStyle::Body.resolve(ui.style());
        let font_color = ui.style().visuals.text_color();
    
        let painter = ui.painter_at(rect);
        let stroke_width = 1.0;
        let stroke = Stroke::new(stroke_width, color);
    
        let state = self.view.state();
        let range = IdxRange {
            min: idx_scale.visible.min,
            max: idx_scale.visible.max.min(state.end),
        };
    
        scan(&idx_scale, &self.view, range, |a, b| a==b, |x1, x2, _idx1, _idx2, val | {
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
                        self.formatter.format(val).to_string(),
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
}

pub struct LogicRow<'a> {
    view: IntView<'a>,
    bit: u32,
    sample_rate: f64,
    label: Option<String>,
    color: AccentColor,
}

impl<'a> LogicRow<'a> {
    pub fn each_bit(vcx: &'a ViewerContext, entity: &'a EntityStream) -> Option<impl Iterator<Item = Self> + 'a> {
        let EntityKind::Logic { bits, .. } = &entity.kind else {
            return None;
        };

        let sample_rate = entity.sample_rate()?;
        let view = vcx.view_manager.int_view(entity)?;

        Some(bits.iter().enumerate().map(move |(bit, field)| {
            let view = view.clone();
            let bit = bit as u32;
            let name = field.name.clone();
            let color = field.accent_color().unwrap_or(AccentColor::Green);

            LogicRow { view, sample_rate, bit: bit as u32, label: Some(name), color }
        }))
    }

    pub fn time_range(&self) -> TimeRange {
        TimeRange {
            min: Time::ZERO,
            max: (self.view.state().end as i128) * Time::period_float(self.sample_rate),
        }
    }

    pub fn render(&self, ui: &mut egui::Ui, scale: &super::scale::Scale) -> TimelineResponse {
        label_frame(ui, |ui| {
            ui.label(self.label.as_deref().unwrap_or(""));
        });
        let rect = stream_rect(ui, scale);
        let idx_scale = scale.idx_scale(self.sample_rate);
    
        let state = self.view.state();
    
        let range = IdxRange {
            min: idx_scale.visible.min,
            max: idx_scale.visible.max.min(state.end),
        };
    
        let font_id = egui::TextStyle::Body.resolve(ui.style());
        let font_color = ui.style().visuals.text_color();
    
        let interact_radius = ui.style().interaction.resize_grab_radius_side;
        let mut snap_to_idx = None;

        let padded_rect = rect.shrink2(Vec2::new(0.0, 8.0));
        if !ui.is_rect_visible(padded_rect) {
            return TimelineResponse::default();
        }

        let color = named_color(self.color);

        let painter = ui.painter_at(rect);
        let stroke_width = 1.0;
        let stroke = Stroke::new(stroke_width, color);

        let mask = 1 << self.bit;
        let eq = |v1: u64, v2: u64| {
            v1 & mask == v2 & mask
        };

        let hover_x = ui.input(|i| i.pointer.interact_pos())
            .filter(|pos| rect.contains(*pos) && ui.ctx().dragged_id().is_none())
            .map(|pos| pos.x);

        let mut prev_idx = None;

        scan(&idx_scale, &self.view, range, eq, |x1, x2, idx1, idx2, val | {
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
        
        let snap_to_time = snap_to_idx.map(|idx| {
            idx_scale.t_from_idx(idx)
        });
    
        TimelineResponse {
            snap_to_time,
        }
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
