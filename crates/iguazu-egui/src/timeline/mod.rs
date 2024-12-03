mod paint_ticks;
mod scale;
mod analog_row;
mod trace_row;
mod events_row;

use egui::{pos2, CursorIcon, NumExt, PointerButton, Rect, Vec2, Layout, Rangef};
use iguazu::schema::{attribute::TimelineRow, EntityStream};
use crate::{ ViewerContext, time::TimeRange, egui_util:: shadow_line::draw_shadow_line };

use scale::Scale;

pub struct TimePanel {
    /// Width of the entity name columns previous frame.
    pub col_width: f32,

    pub time_range: TimeRange,
    pub entity: EntityStream,

    pub visible_range: Option<TimeRange>,
}

impl TimePanel {
    fn set_visible_range(&mut self, range: TimeRange) {
        self.visible_range = Some(range);
    }

    fn reset_visible_range(&mut self) {
        self.visible_range = None;
    }

    pub fn show(
        &mut self,
        ctx: &mut ViewerContext<'_>,
        ui: &mut egui::Ui,
    ) {
        //               |timeline            |
        // ------------------------------------
        // tree          |streams             |
        //               |  . .   .    ...    |
        //               |             ...  . |

        let rect = ui.max_rect();

        // x position of split between sidebar and data
        let time_x_left =
            (rect.left() + self.col_width + ui.spacing().item_spacing.x)
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
            self.visible_range.unwrap_or(self.time_range),
            self.time_range,
            x_margin,
            x_margin + scrollbar_width,
        );

        ui.with_layout(Layout::top_down_justified(egui::Align::Min), |ui| {
            let (_, top_rect) = ui.allocate_space(Vec2::new(0.0, 28.0));

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

            let streams_response = self.interact_with_streams_rect(
                &scale,
                ctx,
                ui,
                &streams_rect,
            );

            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .drag_to_scroll(false)
                .enable_scrolling(!streams_response.hovered())
                .show(ui, |ui| {
                    render_entity(ctx, ui, &scale, None, &self.entity);

                    // measure sidebar width for next frame
                    self.col_width = ui.min_rect().width();
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

            let time_area_painter = ui.painter().with_clip_rect(Rect::from_x_y_ranges(time_x_range_without_scrollbar.clone(), rect.y_range()));

            // Put time-marker on top and last, so that you can always drag it
            time_marker_ui(
                &scale,
                ctx,
                ui,
                &time_area_painter,
                &timeline_rect,
            );
        });
    }

    fn interact_with_streams_rect(
        &mut self,
        time_ranges_ui: &Scale,
        _ctx: &mut ViewerContext<'_>,
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
            self.set_visible_range(time_ranges_ui.pan(-delta_x));
        }

        if zoom_factor != 1.0 {
            if let Some(pointer_pos) = pointer_pos {
                self.set_visible_range(time_ranges_ui.zoom_at(pointer_pos.x, zoom_factor));
            }
        }

        if response.double_clicked() {
            self.reset_visible_range();
        }

        response
    }
}

fn render_entity(
    ctx: &mut ViewerContext<'_>,
    ui: &mut egui::Ui,
    scale: &Scale,
    label: Option<&str>,
    entity: &EntityStream,
) {
    match entity.attribute::<TimelineRow>() {
        None | Some(TimelineRow::Group) => {
            for (name, child) in &entity.children {
                render_entity(ctx, ui, scale, Some(name), child);
            }
        }
        Some(TimelineRow::YAxis) => {
            analog_row::render(ctx, ui, scale, label, entity)
        }
        Some(TimelineRow::Trace) => {
            trace_row::render(ctx, ui, scale, label, entity)
        }
        Some(TimelineRow::Logic) => {
            trace_row::render_logic(ctx, ui, scale, label, entity)
        }
        Some(TimelineRow::Events) => {
            events_row::render(ctx, ui, scale, label, entity)
        }
    }
}

fn fixed_height_header(ui: &mut egui::Ui, scale: &Scale, label: Option<&str>) -> Rect {
    let header_y_range = ui.horizontal(|ui| {
        ui.label(label.unwrap_or(""));
    }).response.rect.y_range();

    Rect::from_x_y_ranges(scale.x_range.clone(), header_y_range)
}

/// A vertical line that shows the current time.
fn time_marker_ui(
    time_ranges_ui: &Scale,
    ctx: &mut ViewerContext<'_>,
    ui: &mut egui::Ui,
    time_area_painter: &egui::Painter,
    timeline_rect: &Rect,
) {
    // timeline_rect: top part with the second ticks and time marker

    let pointer_pos = ui.input(|i| i.pointer.hover_pos());
    let time_drag_id = ui.id().with("time_drag_id");
    let timeline_cursor_icon = CursorIcon::ResizeHorizontal;
    let is_hovering_the_loop_selection = ui.output(|o| o.cursor_icon) != CursorIcon::Default; // A kind of hacky proxy
    let is_anything_being_dragged = ui.memory(|mem| mem.is_anything_being_dragged());
    let interact_radius = ui.style().interaction.resize_grab_radius_side;

    let mut is_hovering = false;

    // show current time as a line:
    if let Some(time) = ctx.time() {
        let x= time_ranges_ui.x_from_t(time);
        if timeline_rect.x_range().contains(x) {
            let line_rect =
                Rect::from_x_y_ranges(x..=x, timeline_rect.top()..=ui.max_rect().bottom())
                    .expand(interact_radius);

            let response = ui
                .interact(line_rect, time_drag_id, egui::Sense::drag())
                .on_hover_and_drag_cursor(timeline_cursor_icon);

            is_hovering = !is_anything_being_dragged && response.hovered();

            if response.dragged() {
                if let Some(pointer_pos) = pointer_pos {
                    let time = time_ranges_ui.t_from_x(pointer_pos.x);
                    let time = time_ranges_ui.clamp_time(time);
                    ctx.set_time(time);
                }
            }

            let stroke = if response.dragged() {
                ui.style().visuals.widgets.active.fg_stroke
            } else if is_hovering {
                ui.style().visuals.widgets.hovered.fg_stroke
            } else {
                ui.visuals().widgets.inactive.fg_stroke
            };
            paint_time_cursor(
                time_area_painter,
                x,
                Rangef::new(timeline_rect.top(), ui.max_rect().bottom()),
                stroke,
            );
        }
    }

    // "click here to view time here"
    if let Some(pointer_pos) = pointer_pos {
        let is_pointer_in_timeline_rect = timeline_rect.contains(pointer_pos);

        // Show preview?
        if !is_hovering
            && is_pointer_in_timeline_rect
            && !is_anything_being_dragged
            && !is_hovering_the_loop_selection
        {
            time_area_painter.vline(
                pointer_pos.x,
                timeline_rect.top()..=ui.max_rect().bottom(),
                ui.visuals().widgets.noninteractive.bg_stroke,
            );
            ui.ctx().set_cursor_icon(timeline_cursor_icon); // preview!
        }

        // Click to move time here:
        if ui.input(|i| i.pointer.primary_down())
            && is_pointer_in_timeline_rect
            && !is_anything_being_dragged
            && !is_hovering_the_loop_selection
        {
            let time = time_ranges_ui.t_from_x(pointer_pos.x);
            let time = time_ranges_ui.clamp_time(time);
            ctx.set_time(time);
            ui.memory_mut(|mem| mem.set_dragged_id(time_drag_id));
        }
    }
}

pub fn paint_time_cursor(
    painter: &egui::Painter,
    x: f32,
    y: Rangef,
    stroke: egui::Stroke,
) {
    let stroke = egui::Stroke {
        width: 1.5 * stroke.width,
        color: stroke.color,
    };

    let w = 10.0;
    let triangle = vec![
        pos2(x - 0.5 * w, y.min), // left top
        pos2(x + 0.5 * w, y.min), // right top
        pos2(x, y.min + w),       // bottom
    ];
    painter.add(egui::Shape::convex_polygon(
        triangle,
        stroke.color,
        egui::Stroke::NONE,
    ));
    painter.vline(x, (y.min + w)..=y.max, stroke);
}
