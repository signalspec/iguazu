use egui::Color32;
use iguazu::schema::attribute::display::AccentColor;

pub fn named_color(color: AccentColor) -> Color32 {
    // Based on https://iamkate.com/data/12-bit-rainbow/
    match color {
        AccentColor::Neutral => Color32::from_rgb(192, 192, 192),
        AccentColor::Brown => Color32::from_rgb(175, 142, 113),
        AccentColor::Red => Color32::from_rgb(200, 16, 16),
        AccentColor::Orange => Color32::from_rgb(238, 153, 68),
        AccentColor::Yellow => Color32::from_rgb(242, 226, 0),
        AccentColor::Green => Color32::from_rgb(153, 221, 85),
        AccentColor::Blue => Color32::from_rgb(51, 102, 187),
        AccentColor::Purple => Color32::from_rgb(102, 51, 153),
    }
}
