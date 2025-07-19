use egui::{emath::GuiRounding, Align, Align2, Color32, FontId, Painter, Pos2, Rangef, Rect, Stroke, Ui, Vec2};
use iguazu::{schema::{attribute::AccentColor, fmt::TextFormat, Field}, stream::ArcStream, view::{TraceElement, TraceView}, IdxRange};
use ecow::EcoString;

use crate::{color::named_color, Time, TimeRange, ViewerContext};

use super::{label_frame, stream_rect, TimelineResponse};

pub struct TraceRow<'a> {
    view: TraceView<'a>,
    sample_rate: f64,
    formatter: TextFormat,
    label: EcoString,
    color: AccentColor,
    offset: u8,
    width: u8,
}

impl<'a> TraceRow<'a> {
    pub fn field(vcx: &'a ViewerContext, stream: &ArcStream, sample_rate: f64, offset: u8, color: Option<AccentColor>, label: EcoString, field: &Field) -> TraceRow<'a> {
        let view = TraceView::new(&vcx.view_manager, stream.clone(), &[]);
        let width = field.kind.width();
        let color = color.unwrap_or(AccentColor::Green);
        let formatter = TextFormat::new(offset, field);
        TraceRow { view, sample_rate, label, color, formatter, offset, width }
    }

    pub fn time_range(&self) -> TimeRange {
        TimeRange {
            min: Time::ZERO,
            max: (self.view.state().end as i128) * Time::period_float(self.sample_rate),
        }
    }

    pub fn render(&self, ui: &mut Ui, scale: &super::scale::Scale) -> TimelineResponse {
        label_frame(ui, |ui| {
            ui.label(self.label.as_str());
        });
        let rect = stream_rect(ui, scale);
        let padded_rect = rect.shrink2(Vec2::new(0.0, 8.0))
            .round_to_pixel_center(ui.pixels_per_point());
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

        let mask = ((1 << self.width) - 1) << self.offset;
        let mut last = TraceElement::Loading;

        self.view.scan(range, mask, idx_scale.min_visible_width(), |range, elem| {
            let x1 = idx_scale.x_from_idx(range.min);
            let x2 = idx_scale.x_from_idx(range.max);

            match elem {
                TraceElement::Loading => {
                }
                TraceElement::Dense => {
                    let xr = Rangef::new(x1, x2);
                    if xr.span() <= stroke_width {
                        painter.vline(xr.center(), padded_rect.y_range(), stroke);
                    } else {
                        painter.rect_filled(
                            Rect::from_x_y_ranges(xr, padded_rect.y_range().expand(stroke_width / 2.0)),
                            0.0,
                            color,
                        );
                    }
                }
                TraceElement::Value(val) => {
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

                    if matches!(last, TraceElement::Value(_)) {
                        painter.vline(x1, padded_rect.y_range(), stroke);
                    }
                }
            }
            last = elem;
        });
    
        TimelineResponse {
            snap_to_time: None
        }
    }
}

pub struct LogicRow<'a> {
    view: TraceView<'a>,
    offset: u8,
    sample_rate: f64,
    label: EcoString,
    color: AccentColor,
}

impl<'a> LogicRow<'a> {
    pub fn field(vcx: &'a ViewerContext, stream: &ArcStream, sample_rate: f64, offset: u8, color: Option<AccentColor>, label: EcoString, _field: &Field) -> LogicRow<'a> {
        let view = TraceView::new(&vcx.view_manager, stream.clone(), &[]);
        let color = color.unwrap_or(AccentColor::Green);
        LogicRow { view, sample_rate, label, color, offset }
    }

    pub fn time_range(&self) -> TimeRange {
        TimeRange {
            min: Time::ZERO,
            max: (self.view.state().end as i128) * Time::period_float(self.sample_rate),
        }
    }

    pub fn render(&self, ui: &mut egui::Ui, scale: &super::scale::Scale) -> TimelineResponse {
        label_frame(ui, |ui| {
            ui.label(self.label.as_str());
        });
        let rect = stream_rect(ui, scale);
        let idx_scale = scale.idx_scale(self.sample_rate);
    
        let state = self.view.state();
    
        let range = IdxRange {
            min: idx_scale.visible.min,
            max: idx_scale.visible.max.min(state.end),
        };
        let min_width = idx_scale.min_visible_width();
    
        let font_id = egui::TextStyle::Body.resolve(ui.style());
        let font_color = ui.style().visuals.text_color();
    
        let interact_radius = ui.style().interaction.resize_grab_radius_side;
        let mut snap_to_idx = None;

        let padded_rect = rect.shrink2(Vec2::new(0.0, 8.0))
            .round_to_pixel_center(ui.pixels_per_point());
        if !ui.is_rect_visible(padded_rect) {
            return TimelineResponse::default();
        }

        let color = named_color(self.color);

        let painter = ui.painter_at(rect);
        let stroke_width = 1.0;
        let stroke = Stroke::new(stroke_width, color);

        let mask = 1 << self.offset;

        let hover_x = ui.input(|i| i.pointer.interact_pos())
            .filter(|pos| rect.contains(*pos) && ui.ctx().dragged_id().is_none())
            .map(|pos| pos.x);

        let mut idx0 = 0;
        let mut prev_val = TraceElement::Loading;

        self.view.scan(range, mask, min_width, |IdxRange { min: idx1, max: idx2 }, elem|{
            let x1 = idx_scale.x_from_idx(idx1);
            let x2 = idx_scale.x_from_idx(idx2);

            match elem {
                TraceElement::Loading => {

                }
                TraceElement::Dense => {
                    let xr = Rangef::new(x1, x2);
                    if xr.span() <= stroke_width {
                        painter.vline(xr.center(), padded_rect.y_range(), stroke);
                    } else {
                        painter.rect_filled(
                            Rect::from_x_y_ranges(xr, padded_rect.y_range().expand(stroke_width / 2.0)),
                            0.0,
                            color,
                        );
                    }
                }
                TraceElement::Value(val) => {
                    if matches!(prev_val, TraceElement::Value(_)) {
                        painter.vline(x1, padded_rect.y_range(), stroke);
                    }

                    if val != 0 {
                        painter.hline(x1..=x2, padded_rect.top(), stroke);
                    } else {
                        painter.hline(x1..=x2, padded_rect.bottom(), stroke);
                    }
                }
            }

            if let Some(hover_x) = hover_x {
                if (hover_x - x1).abs() < interact_radius {
                    // Hovering the left edge of this span
                    snap_to_idx = Some(idx1);

                    if matches!(prev_val, TraceElement::Value(_)) && idx0 > range.min && idx2 < range.max {
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
                    if idx1 > range.min && idx2 < range.max {
                        if x2 - x1 > 80.0 {
                            let span_rect = Rect::from_x_y_ranges(x1..=x2, padded_rect.y_range());
                            let width = idx_scale.t_from_idx(idx2) - idx_scale.t_from_idx(idx1);
                            let text = width.format_relative(idx_scale.sample_period()).to_string();
                            span_width_label(&painter, span_rect, span_rect.x_range().center(), Align::Center, &font_id, font_color, text);
                        }
                    }
                }
            }

            idx0 = idx1;
            prev_val = elem;
        });
        
        let snap_to_time = snap_to_idx.map(|idx| {
            idx_scale.t_from_idx(idx)
        });
    
        TimelineResponse {
            snap_to_time,
        }
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
