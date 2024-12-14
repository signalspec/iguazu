mod paint_ticks;
mod scale;
mod analog_row;
mod trace_row;
mod events_row;

use egui::{CursorIcon, Frame, Layout, Margin, NumExt, PointerButton, Rangef, Rect, Vec2};
use iguazu::schema::{attribute::TimelineRow, EntityStream};
use crate::{ egui_util:: shadow_line::draw_shadow_line, time::TimeRange, Time, ViewerContext };

use scale::Scale;

pub struct TimelineView {}

#[derive(Clone, Debug)]
pub struct TimelineState {
    /// Width of the entity name columns previous frame.
    pub col_width: f32,
    pub time_range: TimeRange,
    pub visible_range: Option<TimeRange>,
}

impl Default for TimelineState {
    fn default() -> Self {
        Self {
            col_width: 0.0,
            time_range: TimeRange { min: Time::ZERO, max: Time::MINUTE },
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
        
        //               |timeline            |
        // ------------------------------------
        // tree          |streams             |
        //               |  . .   .    ...    |
        //               |             ...  . |

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

        let scale = Scale::new(
            time_x_range,
            state.visible_range.unwrap_or(state.time_range),
            state.time_range,
            x_margin,
            x_margin + scrollbar_width,
        );

        ui.with_layout(Layout::top_down_justified(egui::Align::Min), |ui| {
            let (_, top_rect) = ui.allocate_space(Vec2::new(0.0, 36.0));

            let timeline_rect = Rect::from_x_y_ranges(time_x_range.clone(), top_rect.y_range());

            ui.painter().hline(
                rect.x_range(),
                timeline_rect.bottom(),
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

            let streams_response = self.interact(
                &mut state,
                &scale,
                ui,
                &streams_rect,
            );

            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .drag_to_scroll(false)
                .enable_scrolling(!streams_response.hovered())
                .show(ui, |ui| {
                    render_entity(vcx, ui, &scale, None, &entity);

                    // measure sidebar width for next frame
                    state.col_width = ui.min_rect().width();
                });
            {
                // Paint a shadow between the stream names on the left
                // and the data on the right:
                let shadow_width = 30.0;
                let rect = egui::Rect::from_x_y_ranges(
                    time_x_left..=(time_x_left + shadow_width),
                    rect.y_range().clone(),
                );

                draw_shadow_line(ui, rect, egui::Direction::LeftToRight);
            }

            cursor_ui(
                ui,
                &ui.painter().with_clip_rect(Rect::from_x_y_ranges(time_x_range_without_scrollbar.clone(), rect.y_range())),
            );
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

fn render_entity(
    vcx: &mut ViewerContext,
    ui: &mut egui::Ui,
    scale: &Scale,
    label: Option<&str>,
    entity: &EntityStream,
) {
    match entity.attribute::<TimelineRow>() {
        None | Some(TimelineRow::Group) => {
            for (name, child) in &entity.children {
                render_entity(vcx, ui, scale, Some(name), child);
            }
        }
        Some(TimelineRow::YAxis) => {
            analog_row::render(vcx, ui, scale, label, entity)
        }
        Some(TimelineRow::Trace) => {
            trace_row::render(vcx, ui, scale, label, entity)
        }
        Some(TimelineRow::Logic) => {
            trace_row::render_logic(vcx, ui, scale, label, entity)
        }
        Some(TimelineRow::Events) => {
            events_row::render(vcx, ui, scale, label, entity)
        }
    }
}

fn fixed_height_header(ui: &mut egui::Ui, scale: &Scale, label: Option<&str>, height: f32) -> Rect {
    let header_y_range = ui.horizontal(|ui| {
        ui.set_height(height);
        Frame::none()
            .inner_margin(Margin::symmetric(6.0, 6.0))
            .show(ui, |ui| {
            ui.label(label.unwrap_or(""));
        });
    }).response.rect.y_range();

    Rect::from_x_y_ranges(scale.x_range.clone(), header_y_range)
}

/// A vertical line that shows the current time.
fn cursor_ui(
    ui: &mut egui::Ui,
    painter: &egui::Painter,
) {
    let pointer_pos = ui.input(|i| i.pointer.hover_pos());
    let is_anything_being_dragged = ui.ctx().dragged_id().is_some();

    if let Some(pointer_pos) = pointer_pos {
        let is_pointer_in_timeline_rect = painter.clip_rect().contains(pointer_pos);

        if is_pointer_in_timeline_rect && !is_anything_being_dragged {
            let mut stroke = ui.visuals().widgets.noninteractive.bg_stroke;
            stroke.color = stroke.color.gamma_multiply(0.5);

            painter.vline(
                pointer_pos.x,
                painter.clip_rect().y_range(),
                stroke,
            );
        }
    }
}
