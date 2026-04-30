use std::sync::Arc;

use async_executor::Task;
use egui::{Button, Color32, Layout, Rect, RichText, Ui, UiBuilder, Vec2};
use iguazu::{import::{IMPORTERS, Importer}, io::ReadableFile, schema::{Entity, EntityStream, Field, FieldKind}, storage::MemoryStream, stream::ArcStream};
use iguazu_egui::ViewerContext;
use rfd::AsyncFileDialog;

pub struct Welcome {
    picker_task: Option<Task<Option<Result<WelcomeResponse, String>>>>,
    error: Option<String>,
}

pub enum WelcomeResponse {
    Import {
        importer: Box<dyn Importer>,
    },
    Entity(EntityStream),
}

impl Welcome {
    pub fn new() -> Self {
        Self {
            picker_task: None,
            error: None,
        }
    }

    pub(crate) fn with_error(message: String) -> Welcome {
        Self {
            picker_task: None,
            error: Some(message),
        }
    }

    pub fn show(&mut self, vctx: &mut ViewerContext, ui: &mut Ui) -> Option<WelcomeResponse> {
        centered_box(ui, Vec2::new(300.0, 100.0), Layout::top_down(egui::Align::Center), |ui| {
            let mut response = None;

            let button_font = egui::FontId::proportional(28.0);
            ui.style_mut().spacing.item_spacing.y = 16.0;

            if self.picker_task.is_some() {
                ui.disable();
            }

            if ui.add_sized((ui.available_width(), 0.0), Button::new(RichText::new("Open file…").font(button_font.clone()))).clicked() {
                self.error = None;
                self.picker_task = Some(vctx.spawn(pick_file()));
            }

            if let Some(res) = vctx.poll_unpin_take(&mut self.picker_task) {
                match res {
                    None => {} // cancelled
                    Some(Ok(entity)) => response = Some(entity),
                    Some(Err(e)) => self.error = Some(e),
                }
            }

            if ui.add_sized((ui.available_width(), 0.0), Button::new(RichText::new("Load demo data").font(button_font.clone()))).clicked() {
                response = Some(WelcomeResponse::Entity(generate_demo_entity()));
            }

            if let Some(error) = &self.error {
                ui.heading(RichText::from("Error").color(Color32::RED));
                ui.colored_label(Color32::RED, error);
            }

            response
        })
    }
}


fn centered_box<R>(ui: &mut Ui, size: Vec2, layout: Layout, child_ui: impl FnOnce(&mut Ui) -> R) -> R {
    let available_rect = ui.available_rect_before_wrap();
    let center = available_rect.center();
    let top_left = center - size * 0.5;
    let box_rect = Rect::from_min_size(top_left, size);

    ui.scope_builder(
        UiBuilder::new().max_rect(box_rect).layout(layout),
        child_ui
    ).inner
}

async fn pick_file() -> Option<Result<WelcomeResponse, String>> {
    #[cfg(not(target_arch = "wasm32"))]
    let res = AsyncFileDialog::new().pick_file().await?;

    #[cfg(target_arch = "wasm32")]
    let res = send_wrapper::SendWrapper::new(AsyncFileDialog::new().pick_file()).await?;

    #[cfg(not(target_arch = "wasm32"))]
    let file = iguazu::io::FsFile::open(res.inner().to_owned()).await.ok()?;

    #[cfg(target_arch = "wasm32")]
    let file = iguazu::io::WebFile::new(res.inner().clone());
    let filename = file.filename().unwrap_or("").to_owned();

    let Some(importer) = IMPORTERS.importer_for_file(Arc::new(file)) else {
        return Some(Err(format!(
            "No import format matched filename `{filename}`"
        )));
    };

    Some(Ok(WelcomeResponse::Import {
        importer,
    }))
}

fn generate_demo_entity() -> EntityStream {
    use iguazu::schema::attribute::{core::{SAMPLE_RATE, NUMBER_RANGE, NumberRange}, display::{ DISPLAY, ACCENT_COLOR, AccentColor, Display }};

    let analog = Entity::Data {
        field: Field::new(FieldKind::Float32 { pos: 0 }),
        data: MemoryStream::new(&(0..1000).map(|i| (i as f32 * 0.01).sin()).collect::<Vec<f32>>()) as ArcStream,
        summaries: Default::default(),
    }.with_attribute(SAMPLE_RATE, 100.0)
    .with_attribute(NUMBER_RANGE, NumberRange { min: -1.0, max: 1.0 })
    .with_attribute(ACCENT_COLOR, AccentColor::Blue);

    let digital = Entity::Data{
        field: Field::new(FieldKind::BitStruct {
            children: FromIterator::from_iter([
                ("bit0".into(), Field::new(FieldKind::Bits { pos: 0, bits: 1 }).with_attribute(ACCENT_COLOR, AccentColor::Red)),
                ("bit1".into(), Field::new(FieldKind::Bits { pos: 1, bits: 1 }).with_attribute(ACCENT_COLOR, AccentColor::Orange)),
                ("bit2".into(), Field::new(FieldKind::Bits { pos: 2, bits: 1 }).with_attribute(ACCENT_COLOR, AccentColor::Yellow)),
                ("bit3".into(), Field::new(FieldKind::Bits { pos: 3, bits: 1 }).with_attribute(ACCENT_COLOR, AccentColor::Green)),
            ])
        }),
        data: MemoryStream::new(&(0..255u8).collect::<Vec<u8>>()) as ArcStream,
        summaries: Default::default(),
    }.with_attribute(SAMPLE_RATE, 25.0);

    Entity::record([
        ("analog".into(), analog),
        ("digital".into(), digital),
    ]).with_attribute(DISPLAY, Display::Timeline)
}
