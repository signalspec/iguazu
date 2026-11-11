// Based on rerun.io: © 2023 Rerun Technologies AB under MIT OR Apache-2.0
use std::iter::Sum;

use ecow::EcoString;
use egui::{Align, CursorIcon, Frame, Id, Label, Layout, Margin, PointerButton, Rangef, Rect, Stroke, UiBuilder, Vec2, emath::GuiRounding, scroll_area::ScrollSource};
use iguazu::{schema::{Entity, EntityStream, FieldRef, attribute::{self, display::{AccentColor, TimelineRow}}}, time::{Time, TimeRange}, view::Timebase};
use crate::{ ViewerContext, egui_util:: shadow_line::draw_shadow_line };

mod analog_row;
use analog_row::YAxisRow;
mod trace_row;
use trace_row::{LogicRow, TraceRow};
mod events_row;
use events_row::EventsRow;
mod spectrogram_row;
use spectrogram_row::SpectrogramRow;

mod paint_ticks;
mod scale;
use scale::Scale;

#[derive(Clone, Debug)]
pub struct TimelineState {
    /// Width of the entity name columns previous frame.
    pub col_width: f32,
    pub visible_range: Option<TimeRange>,
}

impl Default for TimelineState {
    fn default() -> Self {
        Self {
            col_width: 0.0,
            visible_range: None,
        }
    }
}

impl TimelineState {
    fn set_visible_range(&mut self, range: TimeRange) {
        self.visible_range = Some(range);
    }

    fn reset_visible_range(&mut self) {
        self.visible_range = None;
    }
}

pub struct TimelineView {}

impl TimelineView {
    pub fn new() -> Self {
        Self { }
    }

    pub fn show(
        &mut self,
        vcx: &mut ViewerContext,
        ui: &mut egui::Ui,
        entity: &mut EntityStream,
    ) {
        let mut state: TimelineState = ui.data_mut(|d| d.get_temp(ui.scope_id())).unwrap_or_default();

        let rect = ui.max_rect();

        // x position of split between sidebar and data
        let time_x_left = rect.left() + state.col_width;

        let x_margin = 20.0;
        let scrollbar_width = ui.spacing_mut().scroll.bar_outer_margin + ui.spacing_mut().scroll.bar_width;

        let time_x_range = Rangef::new(time_x_left, rect.right());
        let time_x_range_without_scrollbar = {
            let right = rect.right() - scrollbar_width;
            debug_assert!(time_x_left < right);
            time_x_left..=right
        };

        let rows = timeline_rows(vcx, ui.scope_id(), entity);

        let capture_len = entity.attribute(attribute::core::CAPTURE_LENGTH).map_or(Time::ZERO, Time::from_seconds);
        let mut time_range = rows.iter()
            .filter_map(|row| row.time_range())
            .reduce(|a, b| TimeRange::union(a, b))
            .unwrap_or(TimeRange::ZERO);
        time_range.max = time_range.max.max(time_range.min + capture_len);

        let visible_range = state.visible_range
            .and_then(|r| r.clamp(time_range))
            .unwrap_or(time_range);

        let scale = Scale::new(
            time_x_range,
            visible_range,
            time_range,
            x_margin,
            x_margin + scrollbar_width,
        );

        let epoch = entity.time_epoch();

        ui.with_layout(Layout::top_down_justified(egui::Align::Min), |ui| {
            let (_, top_rect) = ui.allocate_space(Vec2::new(0.0, 36.0));

            let timeline_rect = Rect::from_x_y_ranges(time_x_range, top_rect.y_range());

            ui.painter().hline(
                rect.x_range(),
                timeline_rect.bottom().round_to_pixel_center(ui.pixels_per_point()),
                ui.visuals().widgets.noninteractive.bg_stroke,
            );

            paint_ticks::paint_time_ticks(
                ui,
                &scale,
                timeline_rect.y_range(),
                epoch,
            );

            let streams_rect = Rect::from_x_y_ranges(
                time_x_range_without_scrollbar.clone(),
                timeline_rect.bottom()..=rect.bottom(),
            );

            let rows_height_request: HeightRequest = rows.iter().map(|row| row.height()).sum();
            let extra_height = (streams_rect.height() - rows_height_request.min_height - 8.0).max(0.0);
            let flex_height = extra_height / rows_height_request.flex.max(1.0);

            let streams_response = self.interact(
                &mut state,
                &scale,
                ui,
                &streams_rect,
            );

            let entity_response = egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .scroll_source(if streams_response.hovered() { ScrollSource::NONE } else { ScrollSource::MOUSE_WHEEL | ScrollSource::SCROLL_BAR })
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(0.0, 0.0);
                    let mut res = TimelineResponse::default();
                    for row in rows {
                        let height_request = row.height();
                        let height = (height_request.min_height + flex_height * height_request.flex).floor();
                        let mut child_ui = ui.new_child(UiBuilder::default().layout(Layout::top_down(Align::Min)));
                        child_ui.set_height(height);

                        let top = child_ui.min_rect().top();
                        ui.painter().hline(
                            time_x_range,
                            top.round_to_pixel_center(ui.pixels_per_point()),
                            Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color.gamma_multiply(0.2))
                        );
                        let shade_rect = Rect::from_x_y_ranges(time_x_range, top..=(top + 16.0));
                        draw_shadow_line(ui, shade_rect, egui::Direction::TopDown, 0.15);

                        res.merge(row.render(vcx, &mut child_ui, &scale));
                        ui.advance_cursor_after_rect(child_ui.min_rect());
                    }

                    // measure sidebar width for next frame
                    state.col_width = ui.min_rect().width();
                    res
                }).inner;

            if state.col_width > 0.0 {
                // Paint a shadow between the names on the left
                // and the data on the right:
                let shadow_width = 30.0;
                let rect = egui::Rect::from_x_y_ranges(
                    time_x_left..=(time_x_left + shadow_width),
                    rect.y_range(),
                );

                draw_shadow_line(ui, rect, egui::Direction::LeftToRight, 0.4);
            }

            let cursor_x = entity_response.snap_to_time.map(|snap_to_time| {
                scale.x_from_t(snap_to_time)
            }).or_else(|| {
                ui.input(|i| i.pointer.hover_pos()).filter(|&pos| rect.contains(pos)).map(|pos| pos.x)
            });

            if let Some(cursor_x) = cursor_x {
                cursor_ui(
                    ui,
                    &ui.painter().with_clip_rect(Rect::from_x_y_ranges(time_x_range_without_scrollbar.clone(), rect.y_range())),
                    cursor_x,
                );
            }

        });

        ui.data_mut(|d| d.insert_temp(ui.scope_id(), state));
    }

    /// Handle zoom / pan interactions
    fn interact(
        &mut self,
        state: &mut TimelineState,
        time_ranges_ui: &Scale,
        ui: &mut egui::Ui,
        streams_rect: &Rect,
    ) -> egui::Response {
        let pointer_pos = ui.input(|i| i.pointer.hover_pos());

        let mut delta_x = 0.0;
        let mut zoom_factor = 1.0;

        let response = ui.interact(
            *streams_rect,
            ui.scope_id().with("time_area_interact"),
            egui::Sense::click_and_drag(),
        );

        if response.hovered() {
            ui.input(|input| {
                delta_x += input.smooth_scroll_delta.x;
                zoom_factor /= input.zoom_delta_2d().x;
                zoom_factor *= (input.smooth_scroll_delta.y * -0.01).exp();
            });
        }

        if response.dragged_by(PointerButton::Primary) {
            delta_x += response.drag_delta().x;
            ui.ctx().set_cursor_icon(CursorIcon::AllScroll);
        }
        if response.dragged_by(PointerButton::Secondary) {
            zoom_factor *= (response.drag_delta().y * 0.01).exp();
        }

        if delta_x != 0.0 {
            state.set_visible_range(time_ranges_ui.pan(-delta_x));
        }

        if zoom_factor != 1.0 && let Some(pointer_pos) = pointer_pos {
            state.set_visible_range(time_ranges_ui.zoom_at(pointer_pos.x, zoom_factor));
        }

        if response.double_clicked() {
            state.reset_visible_range();
        }

        response
    }
}

#[derive(Debug, Clone, Default)]
struct TimelineResponse {
    snap_to_time: Option<Time>,
}

impl TimelineResponse {
    fn merge(&mut self, r: TimelineResponse) {
        self.snap_to_time = self.snap_to_time.or(r.snap_to_time)
    }
}

enum TimelineRowKind<'a> {
    YAxis(YAxisRow<'a>),
    Trace(TraceRow<'a>),
    Logic(LogicRow<'a>),
    Events(EventsRow<'a>),
    Spectrogram(SpectrogramRow<'a>),
}

fn timeline_rows<'a>(vcx: &'a ViewerContext, id: Id, entity: &'a EntityStream) -> Vec<TimelineRowKind<'a>> {
    let mut rows = Vec::new();

    fn add_field<'a>(
        vcx: &'a ViewerContext,
        rows: &mut Vec<TimelineRowKind<'a>>,
        field: FieldRef<'_>,
        timebase: &Timebase<'a>,
        color: Option<AccentColor>,
        name: EcoString,
    ) {
        let color = field.accent_color().or(color);
        match field.timeline_row() {
            TimelineRow::Stack => {
                if let Some(fields) = field.bit_struct_fields() {
                    for (name, field) in fields {
                        add_field(vcx, rows, field, timebase, color, name.clone());
                    }
                }
            }
            TimelineRow::Logic => {
                rows.extend(LogicRow::field(vcx, field, timebase, color, name).map(TimelineRowKind::Logic));
            }
            TimelineRow::Trace => {
                rows.extend(TraceRow::field(vcx, field, timebase, color, name).map(TimelineRowKind::Trace));
            }
            TimelineRow::YAxis => {
                rows.extend(YAxisRow::field(vcx, field, timebase, color, name).map(TimelineRowKind::YAxis));
            }
            TimelineRow::Spectrogram => {
                // TODO: real-valued FFT
            }
            _ => {}
        }
    }

    fn add_entity<'a>(vcx: &'a ViewerContext, id: Id, rows: &mut Vec<TimelineRowKind<'a>>, name: EcoString, timebase: Option<&Timebase<'a>>, entity: &'a EntityStream) {
        let this_timebase = vcx.view_manager.timebase(entity);
        let timebase = this_timebase.as_ref().or(timebase);
        if let Some(field) = entity.as_field() {
            let color = entity.accent_color();
            let Some(timebase) = timebase else { return };
            add_field(vcx, rows, field, timebase, color, name);
        } else {
            match entity.timeline_row() {
                TimelineRow::Stack => {
                    match &entity {
                        Entity::Group { children, .. } => {
                            for (name, child) in children {
                                let child_id = id.with(&name);
                                add_entity(vcx, child_id, rows, name.clone(), timebase, child)
                            }
                        }

                        _ => {},
                    }
                }
                TimelineRow::Events => {
                    rows.extend(EventsRow::new(vcx, name, entity).map(TimelineRowKind::Events));
                }
                TimelineRow::Spectrogram if let Some(timebase) = timebase => {
                    rows.extend(SpectrogramRow::new(vcx, id, name, entity, timebase).map(TimelineRowKind::Spectrogram))
                }
                _ => {}
            }

        }
    }

    add_entity(vcx, id, &mut rows, EcoString::new(), None, entity);
    rows
}

struct HeightRequest {
    min_height: f32,
    flex: f32,
}

impl Sum for HeightRequest {
    fn sum<I: Iterator<Item = HeightRequest>>(iter: I) -> HeightRequest {
        iter.fold(HeightRequest { min_height: 0.0, flex: 0.0 }, |acc, h| HeightRequest {
            min_height: acc.min_height + h.min_height,
            flex: acc.flex + h.flex,
        })
    }
}

impl<'a> TimelineRowKind<'a> {
    fn render(self, vcx: &'a ViewerContext, ui: &mut egui::Ui, scale: &Scale) -> TimelineResponse {
        match self {
            TimelineRowKind::YAxis(row) => row.render(ui, scale),
            TimelineRowKind::Trace(row) => row.render(ui, scale),
            TimelineRowKind::Logic(row) => row.render(ui, scale),
            TimelineRowKind::Events(row) => row.render(ui, scale),
            TimelineRowKind::Spectrogram(row) => row.render(vcx, ui, scale),
        }
    }

    fn time_range(&self) -> Option<TimeRange> {
        match self {
            TimelineRowKind::YAxis(row) => row.time_range(),
            TimelineRowKind::Trace(row) => row.time_range(),
            TimelineRowKind::Logic(row) => row.time_range(),
            TimelineRowKind::Events(row) => row.time_range(),
            TimelineRowKind::Spectrogram(row) => row.time_range(),
        }
    }

    fn height(&self) -> HeightRequest {
        match self {
            TimelineRowKind::YAxis(_) => HeightRequest { min_height: 72.0, flex: 1.0 },
            TimelineRowKind::Trace(_) => HeightRequest { min_height: 40.0, flex: 0.0 },
            TimelineRowKind::Logic(_) => HeightRequest { min_height: 40.0, flex: 0.0 },
            TimelineRowKind::Events(_) => HeightRequest { min_height: 40.0, flex: 0.0 },
            TimelineRowKind::Spectrogram(_) => HeightRequest { min_height: 120.0, flex: 1.0 },
        }
    }
}

fn row_label(ui: &mut egui::Ui, name: &str) {
    if !name.is_empty() {
        Frame::new()
            .inner_margin(Margin::symmetric(12, 6))
            .show(ui, |ui| {
               ui.horizontal(|ui| {
                    ui.add(Label::new(name).selectable(false))
               });
           });
    }
}

fn stream_rect(ui: &egui::Ui, scale: &Scale) -> Rect {
    let rect = ui.max_rect();
    Rect::from_x_y_ranges(scale.x_range, rect.y_range())
}

/// A vertical line that shows the current time.
fn cursor_ui(
    ui: &mut egui::Ui,
    painter: &egui::Painter,
    pointer_x: f32,
) {
    let is_anything_being_dragged = ui.ctx().dragged_id().is_some();

    if !is_anything_being_dragged {
        let stroke = Stroke::new(1.0, ui.visuals().widgets.noninteractive.fg_stroke.color.gamma_multiply(0.2));

        painter.vline(
            pointer_x,
            painter.clip_rect().y_range(),
            stroke,
        );
    }
}
