// Based on rerun.io: © 2023 Rerun Technologies AB under MIT OR Apache-2.0
mod paint_ticks;
mod scale;
mod analog_row;
mod trace_row;
mod events_row;

use std::iter::Sum;

use analog_row::YAxisRow;
use ecow::EcoString;
use egui::{emath::GuiRounding, scroll_area::ScrollSource, Align, CursorIcon, Frame, Layout, Margin, NumExt, PointerButton, Rangef, Rect, Stroke, UiBuilder, Vec2};
use events_row::EventsRow;
use iguazu::{schema::{attribute::{AccentColor, TimelineRow}, Entity, EntityStream, Field, FieldKind, Summary}, stream::ArcStream};
use indexmap::IndexMap;
use trace_row::{LogicRow, TraceRow};
use crate::{ egui_util:: shadow_line::draw_shadow_line, time::TimeRange, Time, ViewerContext };

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
        let mut state: TimelineState = ui.data_mut(|d| d.get_temp(ui.id())).unwrap_or_default();

        let rect = ui.max_rect();

        // x position of split between sidebar and data
        let time_x_left =
            (rect.left() + state.col_width + ui.spacing().item_spacing.x)
                .at_least(50.0)
                .at_most(ui.max_rect().right() - 100.0);

        let x_margin = 20.0;
        let scrollbar_width = ui.spacing_mut().scroll.bar_outer_margin + ui.spacing_mut().scroll.bar_width;

        let time_x_range = Rangef::new(time_x_left, rect.right());
        let time_x_range_without_scrollbar = {
            let right = rect.right() - scrollbar_width;
            debug_assert!(time_x_left < right);
            time_x_left..=right
        };

        let rows = timeline_rows(vcx, entity);
        let time_range = rows.iter().fold(TimeRange::ZERO, |acc, row| {
            acc.union(&row.time_range())
        });

        let scale = Scale::new(
            time_x_range,
            state.visible_range.unwrap_or(time_range),
            time_range,
            x_margin,
            x_margin + scrollbar_width,
        );

        ui.with_layout(Layout::top_down_justified(egui::Align::Min), |ui| {
            let (_, top_rect) = ui.allocate_space(Vec2::new(0.0, 36.0));

            let timeline_rect = Rect::from_x_y_ranges(time_x_range.clone(), top_rect.y_range());

            ui.painter().hline(
                rect.x_range(),
                timeline_rect.bottom().round_to_pixel_center(ui.pixels_per_point()),
                ui.visuals().widgets.noninteractive.bg_stroke,
            );

            paint_ticks::paint_time_ranges_and_ticks(
                &scale,
                ui,
                &ui.painter_at(timeline_rect),
                timeline_rect.y_range(),
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

                        res.merge(row.render(&mut child_ui, &scale));
                        ui.advance_cursor_after_rect(child_ui.min_rect());
                    }

                    // measure sidebar width for next frame
                    state.col_width = ui.min_rect().width();
                    res
                }).inner;
            
            {
                // Paint a shadow between the names on the left
                // and the data on the right:
                let shadow_width = 30.0;
                let rect = egui::Rect::from_x_y_ranges(
                    time_x_left..=(time_x_left + shadow_width),
                    rect.y_range().clone(),
                );

                draw_shadow_line(ui, rect, egui::Direction::LeftToRight, 0.4);
            }

            let cursor_x = entity_response.snap_to_time.map(|snap_to_time| {
                scale.x_from_t(snap_to_time)
            }).or_else(|| {
                ui.input(|i| i.pointer.hover_pos()).map(|pos| pos.x)
            });

            if let Some(cursor_x) = cursor_x {
                cursor_ui(
                    ui,
                    &ui.painter().with_clip_rect(Rect::from_x_y_ranges(time_x_range_without_scrollbar.clone(), rect.y_range())),
                    cursor_x,
                );
            }

        });

        ui.data_mut(|d| d.insert_temp(ui.id(), state));
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
            ui.id().with("time_area_interact"),
            egui::Sense::click_and_drag(),
        );

        if response.hovered() {
            ui.input(|input| {
                delta_x += input.smooth_scroll_delta.x;
                zoom_factor *= input.zoom_delta_2d().x;
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

        if zoom_factor != 1.0 {
            if let Some(pointer_pos) = pointer_pos {
                state.set_visible_range(time_ranges_ui.zoom_at(pointer_pos.x, zoom_factor));
            }
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
}

fn timeline_rows<'a>(vcx: &'a ViewerContext, entity: &'a EntityStream) -> Vec<TimelineRowKind<'a>> {
    let mut rows = Vec::new();

    fn add_field<'a>(
        vcx: &'a ViewerContext,
        rows: &mut Vec<TimelineRowKind<'a>>,
        stream: &ArcStream,
        sample_rate: f64,
        offset: u8,
        color: Option<AccentColor>,
        name: EcoString,
        field: &Field,
        summaries: &IndexMap<EcoString, Summary<ArcStream>>,
    ) {
        let color = field.accent_color().or(color);
        match field.timeline_row() {
            TimelineRow::Group => {
                if let FieldKind::BitStruct { children } = &field.kind {
                    let mut offset = offset;
                    for (name, field) in children {
                        add_field(vcx, rows, stream, sample_rate, offset, color, name.clone(), field, summaries);
                        offset += field.kind.width();
                    }
                }
            }
            TimelineRow::Logic => {
                rows.push(TimelineRowKind::Logic(LogicRow::field(vcx, stream, sample_rate, offset, color, name, field, summaries)));
            }
            TimelineRow::Trace => {
                rows.push(TimelineRowKind::Trace(TraceRow::field(vcx, stream, sample_rate, offset, color, name, field, summaries)));
            }
            TimelineRow::YAxis => {
                rows.extend(YAxisRow::field(vcx, stream, sample_rate, offset, color, name, field, summaries).map(TimelineRowKind::YAxis));
            }
            _ => {}
        }
    }

    fn add_entity<'a>(vcx: &'a ViewerContext, rows: &mut Vec<TimelineRowKind<'a>>, name: EcoString, entity: &'a EntityStream) {
        if let Entity::Data { field, data, summaries } = entity {
            let color = entity.accent_color();
            let Some(sample_rate) = entity.sample_rate() else { return };
            add_field(vcx, rows, data, sample_rate, 0, color, name, field, summaries);
        } else {
            match entity.timeline_row() {
                TimelineRow::Group => {
                    match &entity {
                        Entity::Group { children, .. } | Entity::Record { children, .. } => {
                            for (name, child) in children {
                                add_entity(vcx, rows, name.clone(), child)
                            }
                        }
                        
                        _ => {},
                    }
                }
                TimelineRow::Events => {
                    rows.extend(EventsRow::new(vcx, name, entity).map(TimelineRowKind::Events));
                }
                _ => {}
            }

        }
    }

    add_entity(vcx, &mut rows, EcoString::new(), entity);
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
    fn render(self, ui: &mut egui::Ui, scale: &Scale) -> TimelineResponse {
        match self {
            TimelineRowKind::YAxis(row) => row.render(ui, scale),
            TimelineRowKind::Trace(row) => row.render(ui, scale),
            TimelineRowKind::Logic(row) => row.render(ui, scale),
            TimelineRowKind::Events(row) => row.render(ui, scale),
        }
    }

    fn time_range(&self) -> TimeRange {
        match self {
            TimelineRowKind::YAxis(row) => row.time_range(),
            TimelineRowKind::Trace(row) => row.time_range(),
            TimelineRowKind::Logic(row) => row.time_range(),
            TimelineRowKind::Events(row) => row.time_range(),
        }
    }

    fn height(&self) -> HeightRequest {
        match self {
            TimelineRowKind::YAxis(_) => HeightRequest { min_height: 72.0, flex: 1.0 },
            TimelineRowKind::Trace(_) => HeightRequest { min_height: 40.0, flex: 0.0 },
            TimelineRowKind::Logic(_) => HeightRequest { min_height: 40.0, flex: 0.0 },
            TimelineRowKind::Events(_) => HeightRequest { min_height: 40.0, flex: 0.0 },
        }
    }
}

fn label_frame<T>(ui: &mut egui::Ui, inner: impl FnOnce(&mut egui::Ui) -> T) -> T {
    Frame::new()
        .inner_margin(Margin::symmetric(6, 6))
        .show(ui, |ui| {
            inner(ui)
        }).inner
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
