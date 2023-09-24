use egui::{Color32, Rect};

pub fn draw_shadow_line(ui: &mut egui::Ui, rect: Rect, direction: egui::Direction) {
    let color_dark = ui.visuals().extreme_bg_color.gamma_multiply(0.3);
    let color_bright = Color32::TRANSPARENT;

    let (left_top, right_top, left_bottom, right_bottom) = match direction {
        egui::Direction::RightToLeft => (color_bright, color_dark, color_bright, color_dark),
        egui::Direction::LeftToRight => (color_dark, color_bright, color_dark, color_bright),
        egui::Direction::BottomUp => (color_bright, color_bright, color_dark, color_dark),
        egui::Direction::TopDown => (color_dark, color_dark, color_bright, color_bright),
    };

    use egui::epaint::Vertex;
    let shadow = egui::Mesh {
        indices: vec![0, 1, 2, 2, 1, 3],
        vertices: vec![
            Vertex {
                pos: rect.left_top(),
                uv: egui::epaint::WHITE_UV,
                color: left_top,
            },
            Vertex {
                pos: rect.right_top(),
                uv: egui::epaint::WHITE_UV,
                color: right_top,
            },
            Vertex {
                pos: rect.left_bottom(),
                uv: egui::epaint::WHITE_UV,
                color: left_bottom,
            },
            Vertex {
                pos: rect.right_bottom(),
                uv: egui::epaint::WHITE_UV,
                color: right_bottom,
            },
        ],
        texture_id: Default::default(),
    };
    ui.painter().add(shadow);
}
