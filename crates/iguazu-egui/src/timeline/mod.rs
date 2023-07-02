mod paint_ticks;
mod scale;

use std::ops::RangeInclusive;

use egui::{pos2, CursorIcon, NumExt, PointerButton, Rect, Vec2, Color32, Rounding, Layout};
use iguazu::{TimeType, stream::cache::IntView, Idx};
use crate::{ ViewerContext, ui::draw_shadow_line, IdxRange, IdxRangeF };

use scale::Scale;

pub(crate) struct TimePanel {
    /// Width of the entity name columns previous frame.
    pub col_width: f32,

    pub time_range: IdxRange,
    pub time_type: TimeType,
    pub items: Vec<DisplayItem>,

    pub visible_range: Option<IdxRangeF>,
}

pub struct DisplayItem {
    pub name: String,
    pub kind: DisplayItemKind,
}

pub enum DisplayItemKind {
    Event(DisplayEvent),
}

pub struct DisplayEvent {
    pub data: IntView,
    pub variants: Vec<EnumVariant>,
}

#[derive(Clone)]
pub struct EnumVariant {
    pub name: String,
    pub color: Color32,
}

impl TimePanel {
    fn set_visible_range(&mut self, range: IdxRangeF) {
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
        let scrollbar_width = ui.spacing_mut().scroll_bar_outer_margin + ui.spacing_mut().scroll_bar_width;

        let time_x_range = time_x_left..=rect.right();
        let time_x_range_without_scrollbar = {
            let right = rect.right() - scrollbar_width;
            debug_assert!(time_x_left < right);
            time_x_left..=right
        };

        let scale = Scale::new(
            time_x_range.clone(),
            self.visible_range.unwrap_or(self.time_range.into()),
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
                self.time_type,
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
                    self.show_children(ctx, ui, &scale);

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


    fn show_children(
        &mut self,
        ctx: &mut ViewerContext<'_>,
        ui: &mut egui::Ui,
        scale: &Scale,
    ) {
        let indent = ui.spacing().indent;

        for item in &mut self.items {
            let response = ui
                .horizontal(|ui| {
                    // Add some spacing to match CollapsingHeader:
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let response =
                        ui.allocate_response(egui::vec2(indent, 0.0), egui::Sense::hover());
                    ui.painter().circle_filled(
                        response.rect.center(),
                        2.0,
                        ui.visuals().text_color(),
                    );
                    ui.label(&item.name);
                })
                .response;

            let response_rect = response.rect;

            let row_rect =
                Rect::from_x_y_ranges(scale.x_range.clone(), response_rect.y_range());

            let is_visible = ui.is_rect_visible(row_rect);

            if is_visible {
                render_item(ctx, &ui.painter_at(row_rect), ui, scale, row_rect, item);
            }
        }
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
                delta_x += input.scroll_delta.x;
                zoom_factor *= input.zoom_delta_2d().x;
                zoom_factor *= (input.scroll_delta.y * -0.01).exp();
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

fn render_item(
    ctx: &mut ViewerContext<'_>,
    time_area_painter: &egui::Painter,
    ui: &mut egui::Ui,
    scale: &Scale,
    row_rect: Rect,
    item: &mut DisplayItem,
) {
    match &mut item.kind {
        DisplayItemKind::Event(e) => {
            let bounds = scale.clamped_visible();
            e.data.set_range(bounds);
            e.data.for_each_elem(|idx, value| {
                if let Some(variant) = value.and_then(|v| e.variants.get(v as usize)) {
                    render_event_rect(ctx, time_area_painter, ui, scale, row_rect, idx, variant)
                }
            });
        },
    }
}

fn render_event_rect(
    _ctx: &mut ViewerContext<'_>,
    time_area_painter: &egui::Painter,
    ui: &mut egui::Ui,
    scale: &Scale,
    row_rect: Rect,
    idx: Idx,
    variant: &EnumVariant,
) {
    let x1 = scale.x_from_idx(idx.into());
    let x2 = scale.x_from_idx((idx+1).into());

    let rect = Rect::from_x_y_ranges(x1..=x2, row_rect.y_range());

    let pointer_pos = ui.input(|i| i.pointer.hover_pos());
    let hovered = if let Some(pointer_pos) = pointer_pos { rect.contains(pointer_pos) } else { false };

    let color = variant.color;

    let color = if hovered { color } else { color.gamma_multiply(0.8) };

    time_area_painter.rect_filled(rect, Rounding::same(5.0), color);
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
        let x= time_ranges_ui.x_from_idx(time);
        if timeline_rect.x_range().contains(&x) {
            let line_rect =
                Rect::from_x_y_ranges(x..=x, timeline_rect.top()..=ui.max_rect().bottom())
                    .expand(interact_radius);

            let response = ui
                .interact(line_rect, time_drag_id, egui::Sense::drag())
                .on_hover_and_drag_cursor(timeline_cursor_icon);

            is_hovering = !is_anything_being_dragged && response.hovered();

            if response.dragged() {
                if let Some(pointer_pos) = pointer_pos {
                    let time = time_ranges_ui.idx_from_x(pointer_pos.x);
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
                timeline_rect.top()..=ui.max_rect().bottom(),
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
            let time = time_ranges_ui.idx_from_x(pointer_pos.x);
            let time = time_ranges_ui.clamp_time(time);
            ctx.set_time(time);
            ui.memory_mut(|mem| mem.set_dragged_id(time_drag_id));
        }
    }
}

pub fn paint_time_cursor(
    painter: &egui::Painter,
    x: f32,
    y: RangeInclusive<f32>,
    stroke: egui::Stroke,
) {
    let y_min = *y.start();
    let y_max = *y.end();

    let stroke = egui::Stroke {
        width: 1.5 * stroke.width,
        color: stroke.color,
    };

    let w = 10.0;
    let triangle = vec![
        pos2(x - 0.5 * w, y_min), // left top
        pos2(x + 0.5 * w, y_min), // right top
        pos2(x, y_min + w),       // bottom
    ];
    painter.add(egui::Shape::convex_polygon(
        triangle,
        stroke.color,
        egui::Stroke::NONE,
    ));
    painter.vline(x, (y_min + w)..=y_max, stroke);
}
