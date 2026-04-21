use std::sync::Arc;

use async_executor::Task;
use egui::{Button, Color32, Layout, Rect, RichText, Ui, UiBuilder, Vec2};
use iguazu::{import::IMPORTERS, io::ReadableFile, schema::{Entity, EntityStream, Field, FieldKind}, storage::{MemoryStream, Pool, Storage}, stream::ArcStream};
use iguazu_egui::ViewerContext;
use rfd::AsyncFileDialog;

pub struct Welcome {
    picker_task: Option<Task<Option<Result<EntityStream, String>>>>,
    error: Option<String>,
}

pub struct WelcomeResponse {
    pub loaded_entity: Option<EntityStream>,
}

impl Welcome {
    pub fn new() -> Self {
        Self {
            picker_task: None,
            error: None,
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub fn with_file(file: Arc<dyn ReadableFile>, storage: Arc<dyn Storage>, pool: Arc<Pool>) -> Self {
        let picker_task = Some(pool.executor.spawn(import_file(file, storage, pool.clone())));
        Self {
            picker_task,
            error: None,
        }
    }

    pub fn show(&mut self, vctx: &mut ViewerContext, ui: &mut Ui) -> WelcomeResponse {
        centered_box(ui, Vec2::new(300.0, 100.0), Layout::top_down(egui::Align::Center), |ui| {
            let mut loaded_entity = None;

            let button_font = egui::FontId::proportional(28.0);
            ui.style_mut().spacing.item_spacing.y = 16.0;

            if self.picker_task.is_some() {
                ui.disable();
            }

            if ui.add_sized((ui.available_width(), 0.0), Button::new(RichText::new("Open file…").font(button_font.clone()))).clicked() {
                self.error = None;
                self.picker_task = Some(vctx.spawn(pick_and_import_file(vctx.pool().clone(), vctx.default_storage().clone())));
            }

            if let Some(res) = vctx.poll_unpin_take(&mut self.picker_task) {
                match res {
                    None => {} // cancelled
                    Some(Ok(entity)) => loaded_entity = Some(entity),
                    Some(Err(e)) => self.error = Some(e),
                }
            }

            if ui.add_sized((ui.available_width(), 0.0), Button::new(RichText::new("Load demo data").font(button_font.clone()))).clicked() {
                loaded_entity = Some(generate_demo_entity());
            }

            if let Some(error) = &self.error {
                ui.heading(RichText::from("Error").color(Color32::RED));
                ui.colored_label(Color32::RED, error);
            }

            WelcomeResponse { loaded_entity }
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

async fn pick_file() -> Option<Arc<dyn ReadableFile>> {
    #[cfg(not(target_arch = "wasm32"))]
    let res = AsyncFileDialog::new().pick_file().await;

    #[cfg(target_arch = "wasm32")]
    let res = send_wrapper::SendWrapper::new(AsyncFileDialog::new().pick_file()).await.map(|f| send_wrapper::SendWrapper::new(f));

    if let Some(r) = res {
        #[cfg(not(target_arch = "wasm32"))]
        let file = iguazu::io::FsFile::open(r.inner().to_owned()).await.ok()?;

        #[cfg(target_arch = "wasm32")]
        let file = iguazu::io::WebFile::new(r.inner().clone());
        Some(Arc::new(file) as Arc<dyn ReadableFile>)
    } else {
        None
    }
}

async fn import_file(file: Arc<dyn ReadableFile>, storage: Arc<dyn Storage>, pool: Arc<Pool>) -> Option<Result<EntityStream, String>> {
    let filename = file.filename().unwrap_or("").to_owned();

    let Some(format) = IMPORTERS.first_for_filename(&filename) else {
        return Some(Err(format!("No import format matched filename `{}`", filename)))
    };

    let importer = format.importer();
    let (mut entity, completion) = match importer.import(file, None, pool.clone()).await {
        Ok(v) => v,
        Err(e) => { return Some(Err(format!("Failed to import {}: {}", filename, e))); }
    };

    pool.executor.spawn(completion).detach();
    entity.build_summaries(&pool.executor, &storage).detach();

    Some(Ok(entity))
}

async fn pick_and_import_file(pool: Arc<Pool>, storage: Arc<dyn Storage>) -> Option<Result<EntityStream, String>> {
    let file = pick_file().await?;
    import_file(file, storage, pool).await
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

    Entity::record()
        .with_child("analog".into(), analog)
        .with_child("digital".into(), digital)
        .with_attribute(DISPLAY, Display::Timeline)
}
