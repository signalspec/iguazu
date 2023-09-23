use egui::Color32;
use iguazu::entity::NamedColor;

pub fn named_color(color: NamedColor) -> Color32 {
    // Based on https://iamkate.com/data/12-bit-rainbow/
    match color {
        NamedColor::Red => Color32::from_rgb(200, 16, 16),
        NamedColor::Brown => Color32::from_rgb(184, 116, 92),
        NamedColor::Orange => Color32::from_rgb(238, 153, 68),
        NamedColor::Yellow => Color32::from_rgb(242, 226, 0),
        NamedColor::Green => Color32::from_rgb(153, 221, 85),
        NamedColor::Blue => Color32::from_rgb(51, 102, 187),
        NamedColor::Purple => Color32::from_rgb(102, 51, 153),
        NamedColor::White => Color32::from_rgb(224, 224, 224),
        NamedColor::Black => Color32::from_rgb(32, 32, 32),
    }
}
