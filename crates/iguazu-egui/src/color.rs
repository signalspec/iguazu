use egui::Color32;
use iguazu::schema::attribute::AccentColor;

pub fn named_color(color: AccentColor) -> Color32 {
    use AccentColor::*;
    // Based on https://iamkate.com/data/12-bit-rainbow/
    match color {
        Red => Color32::from_rgb(200, 16, 16),
        Brown => Color32::from_rgb(184, 116, 92),
        Orange => Color32::from_rgb(238, 153, 68),
        Yellow => Color32::from_rgb(242, 226, 0),
        Green => Color32::from_rgb(153, 221, 85),
        Blue => Color32::from_rgb(51, 102, 187),
        Purple => Color32::from_rgb(102, 51, 153),
    }
}
