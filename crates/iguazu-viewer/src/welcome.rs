use egui::{Button, Layout, RichText, Ui, Vec2};
use iguazu::{schema::{Entity, EntityStream, Field, FieldKind}, storage::MemoryStream, stream::ArcStream};
use iguazu_egui::ViewerContext;

use crate::centered_box;

pub struct Welcome {}


pub enum WelcomeResponse {
    PickFile,
    Entity(EntityStream),
}

impl Welcome {
    pub fn new() -> Self {
        Self {}
    }

    pub fn show(&mut self, _vctx: &mut ViewerContext, ui: &mut Ui) -> Option<WelcomeResponse> {
        centered_box(ui, Vec2::new(300.0, 100.0), Layout::top_down(egui::Align::Center), |ui| {
            let mut response = None;

            let button_font = egui::FontId::proportional(28.0);
            ui.style_mut().spacing.item_spacing.y = 16.0;

            if ui.add_sized((ui.available_width(), 0.0), Button::new(RichText::new("Open file…").font(button_font.clone()))).clicked() {
                response = Some(WelcomeResponse::PickFile);
            }

            if ui.add_sized((ui.available_width(), 0.0), Button::new(RichText::new("Load demo data").font(button_font.clone()))).clicked() {
                response = Some(WelcomeResponse::Entity(generate_demo_entity()));
            }

            response
        })
    }
}

fn generate_demo_entity() -> EntityStream {
    use iguazu::schema::attribute::{core::{TIME_RATE, NUMBER_MIN, NUMBER_MAX}, display::{ LAYOUT, COLOR, AccentColor, Layout }};

    let analog = Entity::Data {
        field: Field::new(FieldKind::Float32 { pos: 0 }),
        data: MemoryStream::new(&(0..1000).map(|i| (i as f32 * 0.01).sin()).collect::<Vec<f32>>()) as ArcStream,
        summaries: Default::default(),
    }.with_attribute(TIME_RATE, 100.0)
    .with_attribute(NUMBER_MIN, -1.0)
    .with_attribute(NUMBER_MAX, 1.0)
    .with_attribute(COLOR, AccentColor::Blue);

    let digital = Entity::Data{
        field: Field::new(FieldKind::BitStruct {
            children: FromIterator::from_iter([
                ("bit0".into(), Field::new(FieldKind::Bits { pos: 0, bits: 1 }).with_attribute(COLOR, AccentColor::Red)),
                ("bit1".into(), Field::new(FieldKind::Bits { pos: 1, bits: 1 }).with_attribute(COLOR, AccentColor::Orange)),
                ("bit2".into(), Field::new(FieldKind::Bits { pos: 2, bits: 1 }).with_attribute(COLOR, AccentColor::Yellow)),
                ("bit3".into(), Field::new(FieldKind::Bits { pos: 3, bits: 1 }).with_attribute(COLOR, AccentColor::Green)),
            ])
        }),
        data: MemoryStream::new(&(0..255u8).collect::<Vec<u8>>()) as ArcStream,
        summaries: Default::default(),
    }.with_attribute(TIME_RATE, 25.0);

    Entity::record([
        ("analog".into(), analog),
        ("digital".into(), digital),
    ]).with_attribute(LAYOUT, Layout::Timeline)
}
