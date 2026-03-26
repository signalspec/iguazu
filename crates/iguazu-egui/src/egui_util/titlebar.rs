// Based on egui-desktop under MIT license
// https://github.com/PxlSyl/egui-desktop/tree/42f93944ba91ce871524e3d0e3a818320564023c

use egui::{Align, Color32, Context, Frame, Id, Layout, Margin, Painter, Panel, PointerButton, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2, ViewportCommand, WidgetType};

pub trait ViewportBuilderExt {
    fn with_custom_title_bar(self) -> Self;
}

impl ViewportBuilderExt for egui::ViewportBuilder {
    #[cfg(target_os ="macos")]
    fn with_custom_title_bar(self) -> Self {
        self
            .with_decorations(true)
            .with_title_shown(false)
            .with_titlebar_shown(false)
            .with_titlebar_buttons_shown(true)
            .with_fullsize_content_view(true)
            .with_transparent(true)
    }

    #[cfg(not(target_os ="macos"))]
    fn with_custom_title_bar(self) -> Self {
        self
            .with_decorations(false)
            .with_title_shown(false)
            .with_titlebar_shown(false)
            .with_titlebar_buttons_shown(true)
    }
}

pub struct TitleBar {
    /// Unique egui id for interactions.
    pub id: Id,

    pub height: f32,
}

impl TitleBar {
    pub fn new() -> Self {
        TitleBar {
            id: Id::new("title_bar"),
            height: if cfg!(target_os = "macos") { 28.0 } else { 32.0 },
        }
    }

    pub fn show(&mut self, parent_ui: &mut Ui, inner: impl FnOnce(&mut Ui)) {
        #[cfg(not(target_os ="macos"))]
        self.check_resize(parent_ui.ctx());

        Panel::top(self.id)
            .exact_size(self.height)
            .frame(
                Frame::new()
                    .fill(parent_ui.style().visuals.window_fill())
                    .inner_margin(Margin::same(0))
                    .outer_margin(Margin::same(0)),
            )
            .show_inside(parent_ui, |ui| {
                #[cfg(not(target_os ="macos"))]
                self.interact(ui);

                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    // Reserve the space where the traffic light buttons are overlayed
                    #[cfg(target_os ="macos")]
                    ui.allocate_space(Vec2::new(60.0 / ui.pixels_per_point(), 0.0));

                    inner(ui);

                    #[cfg(not(target_os ="macos"))]
                    self.render_buttons(ui);
                });
            });
    }

    #[cfg(not(target_os ="macos"))]
    fn interact(&mut self, ui: &mut Ui) {
        let title_bar_rect = ui.available_rect_before_wrap();

        let response = ui.interact(title_bar_rect, self.id, Sense::click_and_drag());

        if response.is_pointer_button_down_on() && ui.input(|input| input.pointer.primary_pressed()) {
            log::debug!("Pressed");
        }

        if response.drag_started_by(PointerButton::Primary) {
            log::debug!("Starting drag from title bar");
            ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
        }

        if response.double_clicked() {
            let is_maximized = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
            ui.ctx().send_viewport_cmd(ViewportCommand::Maximized(!is_maximized));
        }

        if response.clicked_by(PointerButton::Secondary) {
            // TODO: context menu
        }
    }

    #[cfg(not(target_os ="macos"))]
    fn check_resize(&mut self, ctx: &Context) {
        let is_maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        let resize_margin = 4.0;

        if !is_maximized && let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) {
            let viewport_rect = ctx.viewport_rect();

            let left = (pos.x - viewport_rect.left()).abs() <= resize_margin;
            let right = (pos.x - viewport_rect.right()).abs() <= resize_margin;
            let top = (pos.y - viewport_rect.top()).abs() <= resize_margin;
            let bottom = (pos.y - viewport_rect.bottom()).abs() <= resize_margin;

            let mode =
                if left && top {
                    Some((egui::CursorIcon::ResizeNorthWest, egui::ResizeDirection::NorthWest))
                } else if right && top {
                    Some((egui::CursorIcon::ResizeNorthEast, egui::ResizeDirection::NorthEast))
                } else if left && bottom {
                    Some((egui::CursorIcon::ResizeSouthWest, egui::ResizeDirection::SouthWest))
                } else if right && bottom {
                    Some((egui::CursorIcon::ResizeSouthEast, egui::ResizeDirection::SouthEast))
                } else if left {
                    Some((egui::CursorIcon::ResizeWest, egui::ResizeDirection::West))
                } else if right {
                    Some((egui::CursorIcon::ResizeEast, egui::ResizeDirection::East))
                } else if top {
                    Some((egui::CursorIcon::ResizeNorth, egui::ResizeDirection::North))
                } else if bottom {
                    Some((egui::CursorIcon::ResizeSouth, egui::ResizeDirection::South))
                } else {
                    None
                };

            if let Some((cursor, direction)) = mode {
                ctx.set_cursor_icon(cursor);

                if ctx.input(|i| i.pointer.primary_pressed()) {
                    ctx.send_viewport_cmd(ViewportCommand::BeginResize(direction));
                }
            }
        }
    }

    #[cfg(not(target_os ="macos"))]
    pub fn render_buttons(&mut self, ui: &mut Ui) {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;

            let color = ui.style().visuals.text_color();

            let close_response = self.render_button("Close", ui, |painter, rect| {
                self.draw_close_icon(painter, rect, color);
            });

             if close_response.clicked() {
                 log::debug!("Close clicked");
                 ui.ctx().send_viewport_cmd(ViewportCommand::Close);
             }

             let is_maximized = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
             if is_maximized {
                 self.render_button("Restore", ui, |painter, rect| {
                     self.draw_restore_icon(painter, rect, color);
                 }).clicked().then(|| {
                     ui.ctx().send_viewport_cmd(ViewportCommand::Maximized(false));
                 });
             } else {
                self.render_button("Maximize", ui, |painter, rect| {
                    self.draw_maximize_icon(painter, rect, color);
                }).clicked().then(|| {
                    ui.ctx().send_viewport_cmd(ViewportCommand::Maximized(true));
                });
             }

            self.render_button("Minimize", ui, |painter, rect| {
                self.draw_minimize_icon(painter, rect, color);
            }).clicked().then(|| {
                ui.ctx().send_viewport_cmd(ViewportCommand::Minimized(true));
            });
        });
    }

    #[cfg(not(target_os ="macos"))]
    fn render_button(&self, label: &str, ui: &mut Ui, draw_icon: impl FnOnce(&Painter, Rect)) -> Response {
        let desired_size = Vec2::new(46.0, 32.0);
        let (rect, response) = ui.allocate_exact_size(desired_size, Sense::click());

        response.widget_info(|| egui::WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), label));

        if response.hovered() {
            ui.painter().rect_filled(rect, 2.0, ui.style().visuals.faint_bg_color);
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }

        draw_icon(ui.painter(), rect);

        response
    }

    #[cfg(not(target_os ="macos"))]
    fn draw_close_icon(&self, painter: &Painter, rect: Rect, color: Color32) {
        let center = rect.center();
        let size = rect.width().min(rect.height()) * 0.4;
        let half_size = size / 2.0;

        let stroke = Stroke::new(2.5, color);
        painter.line_segment(
            [
                center + Vec2::new(-half_size, -half_size),
                center + Vec2::new(half_size, half_size),
            ],
            stroke,
        );
        painter.line_segment(
            [
                center + Vec2::new(half_size, -half_size),
                center + Vec2::new(-half_size, half_size),
            ],
            stroke,
        );
    }

    #[cfg(not(target_os ="macos"))]
    fn draw_maximize_icon(&self, painter: &Painter, rect: Rect, color: Color32) {
        let center = rect.center();
        let size = rect.width().min(rect.height()) * 0.4;
        let stroke = Stroke::new(2.5, color);
        let square_rect = Rect::from_center_size(center, Vec2::splat(size));
        painter.rect_stroke(square_rect, 0.0, stroke, StrokeKind::Inside);
    }

    #[cfg(not(target_os ="macos"))]
    fn draw_restore_icon(&self, painter: &Painter, rect: Rect, color: Color32) {
        let button_size = rect.width().min(rect.height());
        let square_size = button_size * 0.4;
        let icon_rect = Rect::from_center_size(rect.center(), Vec2::splat(square_size));

        let center = icon_rect.center();
        let half_size = square_size / 2.0;

        let stroke = Stroke::new(2.5, color);

        let main_square_size = square_size * 0.75;
        let main_square_center = center + Vec2::new(-half_size * 0.2, 0.0);
        let main_square = Rect::from_center_size(
            main_square_center,
            Vec2::new(main_square_size, main_square_size),
        );
        painter.rect_stroke(main_square, 0.0, stroke, StrokeKind::Inside);

        let spacing = half_size * 0.12;

        let horizontal_start = center + Vec2::new(-half_size * 0.3, -half_size + spacing);
        let horizontal_end = center + Vec2::new(half_size - spacing, -half_size + spacing);

        let vertical_start = center + Vec2::new(half_size - spacing, -half_size + spacing);
        let vertical_end = center + Vec2::new(half_size - spacing, half_size * 0.2);

        painter.line_segment([horizontal_start, horizontal_end], stroke);
        painter.line_segment([vertical_start, vertical_end], stroke);
    }

    #[cfg(not(target_os ="macos"))]
    fn draw_minimize_icon(&self, painter: &Painter, rect: Rect, color: Color32) {
        let center = rect.center();
        let size = rect.width().min(rect.height()) * 0.5;

        let stroke = Stroke::new(2.5, color);
        painter.line_segment(
            [
                center + Vec2::new(-size * 0.5, size * 0.25),
                center + Vec2::new(size * 0.5, size * 0.25),
            ],
            stroke,
        );
    }
}
