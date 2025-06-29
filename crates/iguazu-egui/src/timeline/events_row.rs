use egui::{Align2, Color32, Rect, Stroke, Vec2};
use iguazu::{schema::{attribute::AccentColor, EntityStream}, view::{EventView, TextView}, IdxRange};

use crate::{color::named_color, Time, TimeRange, ViewerContext};

use super::{fixed_height_header, TimelineResponse};

pub struct EventsRow<'a> {
    event_view: EventView<'a>,
    text_view: TextView<'a>,
    color: AccentColor,
    label: Option<String>,
}

impl<'a> EventsRow<'a> {
    pub fn new(vcx: &'a ViewerContext, entity: &EntityStream, label: Option<&str>) -> Option<Self> {
        let event_view = vcx.view_manager.event_view(entity)?;
        let text_view = vcx.view_manager.text_view(entity);
        let color = entity.accent_color().unwrap_or(AccentColor::Green);

        Some(EventsRow {
            event_view,
            text_view,
            color,
            label: label.map(|s| s.to_string()),
        })
    }

    pub fn time_range(&self) -> TimeRange {
        TimeRange {
            min: Time::ZERO,
            max: (self.event_view.latest_idx() as i128) * Time::period_float(self.event_view.sample_rate()),
        }
    }

    pub fn render(&self, ui: &mut egui::Ui, scale: &super::scale::Scale) -> TimelineResponse {
        let rect = fixed_height_header(ui, scale, self.label.as_deref(), 32.0);
        let padded_rect = rect.shrink2(Vec2::new(0.0, 4.0));
        if !ui.is_rect_visible(padded_rect) {
            return TimelineResponse::default();
        }

        let idx_scale = scale.idx_scale(self.event_view.sample_rate());

        let color = named_color(self.color);

        let painter = ui.painter_at(rect);

        let font_id = egui::TextStyle::Body.resolve(ui.style());
        let font_color = ui.style().visuals.text_color();

        let evt_rect = |t_range: IdxRange| -> Rect {
            let x1 = idx_scale.x_from_idx(t_range.min);
            let x2 = idx_scale.x_from_idx(t_range.max);
            Rect::from_x_y_ranges(x1..=x2, padded_rect.y_range())
        };

        let interact_radius = ui.style().interaction.resize_grab_radius_side;
        let mut snap_to_idx = None;
        let hover_x = ui.input(|i| i.pointer.interact_pos())
            .filter(|pos| rect.contains(*pos) && ui.ctx().dragged_id().is_none())
            .map(|pos| pos.x);

        for evt in self.event_view.range(idx_scale.visible, idx_scale.min_visible_width()) {
            match evt {
                iguazu::view::Event::Event(t_range, idx) => {
                    let r = evt_rect(t_range);
                    painter.rect_filled(r, 4.0, color.gamma_multiply(0.3));
                    painter.rect_stroke(r, 4.0, Stroke::new(2.0, color), egui::StrokeKind::Inside);

                    let text_rect = r.shrink2(Vec2::new(5.0, 0.0));
                    let text_min_width = 8.0;
                    if text_rect.width() > text_min_width {
                        let opacity = ((text_rect.width() - text_min_width) / 4.0).clamp(0.0, 1.0);
                        painter
                            .with_clip_rect(text_rect)
                            .text(
                                text_rect.left_center(),
                                Align2::LEFT_CENTER,
                                self.text_view.format(idx).to_string(),
                                font_id.clone(), 
                                font_color.gamma_multiply(opacity)
                            );
                    }

                    if let Some(hover_x) = hover_x {
                        if (hover_x - r.min.x).abs() < interact_radius {
                            snap_to_idx = Some(t_range.min);
                        } else if (hover_x - r.max.x).abs() < interact_radius {
                            snap_to_idx = Some(t_range.max);
                        }
                    }
                }
                iguazu::view::Event::Dense(t_range) => {
                    painter.rect_filled(evt_rect(t_range), 0.0, Color32::WHITE);
                }
                iguazu::view::Event::Loading(t_range) => {
                    painter.rect_filled(evt_rect(t_range), 0.0, Color32::GRAY);
                }
            }
        }

        let snap_to_time = snap_to_idx.map(|idx| {
            idx_scale.t_from_idx(idx)
        });

        TimelineResponse {
            snap_to_time,
        }
    }
}

