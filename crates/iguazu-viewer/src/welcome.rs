use std::{sync::Arc, task::Poll};

use async_executor::{Executor, Task};
use egui::{Button, Color32, Layout, Pos2, Rect, RichText, Ui, UiBuilder, Vec2};
use iguazu::{import::IMPORTERS, io::ReadableFile, schema::{AttributeMap, Entity, EntityStream, Field, FieldKind}, storage::{MemoryStorage, MemoryStream}, stream::ArcStream};
use iguazu_egui::ViewerContext;
#[cfg(not(target_arch = "wasm32"))]
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

    pub fn show(&mut self, vctx: &mut ViewerContext, ui: &mut Ui) -> WelcomeResponse {
        centered_box(ui, Vec2::new(300.0, 100.0), Layout::top_down(egui::Align::Center), |ui| {
            let mut loaded_entity = None;

            ui.style_mut().text_styles.insert(
                egui::TextStyle::Button,
                egui::FontId::new(28.0, eframe::epaint::FontFamily::Proportional),
            );
            ui.style_mut().spacing.item_spacing.y = 16.0;

            if self.picker_task.is_some() {
                ui.disable();
            }

            if ui.add_sized((ui.available_width(), 0.0), Button::new("Open file…")).clicked() {
                self.error = None;
                self.picker_task = Some(vctx.spawn(pick_and_import_file(vctx.executor().clone())));
            }

            if let Some(t) = &mut self.picker_task && let Poll::Ready(res) = vctx.poll_unpin(t) {
                self.picker_task = None;
                if let Some(file) = res {
                    match file {
                        Ok(entity) => {
                            loaded_entity = Some(entity);
                        }
                        Err(e) => {
                            self.error = Some(e);
                        }
                    }
                }
            }

            if ui.add_sized((ui.available_width(), 0.0),Button::new("Load demo data")).clicked() {
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
    let box_rect = Rect::from_min_size(Pos2::from(top_left), size);

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

async fn pick_and_import_file(executor: Arc<Executor<'static>>) -> Option<Result<EntityStream, String>> {
    let file = pick_file().await?;
    let filename = file.filename().unwrap_or("").to_owned();

    let Some(format) = IMPORTERS.first_for_filename(&filename) else {
        return Some(Err(format!("No import format matched filename `{}`", filename)))
    };

    let importer = format.import(file);
    let (mut entity, _completion) = match importer.import(None, executor.clone()).await {
        Ok(v) => v,
        Err(e) => { return Some(Err(format!("Failed to import {}: {}", filename, e))); }
    };

    let storage = MemoryStorage;
    entity.build_summaries(&executor, &storage);

    Some(Ok(entity))
}

fn generate_demo_entity() -> EntityStream {
    let analog = Entity::Data {
        field: Field::new(FieldKind::Float32),
        data: MemoryStream::new(&(0..1000).map(|i| (i as f32 * 0.01).sin()).collect::<Vec<f32>>()) as ArcStream,
        summaries: Default::default(),
    }.with_attribute("sample_rate", 100.0)
    .with_attribute("number:range", AttributeMap::from_iter([
        ("min".into(), (-1.0).into()),
        ("max".into(), 1.0.into()),
    ]))
    .with_attribute("display:accent_color", "blue");

    let digital = Entity::Data{
        field: Field::new(FieldKind::BitStruct {
            children: FromIterator::from_iter([
                ("bit0".into(), Field::new(FieldKind::Bits { bits: 1 }).with_attribute("display:accent_color", "red")),
                ("bit1".into(), Field::new(FieldKind::Bits { bits: 1 }).with_attribute("display:accent_color", "orange")),
                ("bit2".into(), Field::new(FieldKind::Bits { bits: 1 }).with_attribute("display:accent_color", "yellow")),
                ("bit3".into(), Field::new(FieldKind::Bits { bits: 1 }).with_attribute("display:accent_color", "green")),
            ])
        }),
        data: MemoryStream::new(&(0..255u8).collect::<Vec<u8>>()) as ArcStream,
        summaries: Default::default(),
    }.with_attribute("sample_rate", 25.0);

    Entity::record()
        .with_child("analog".into(), analog)
        .with_child("digital".into(), digital)
        .with_attribute("display:default", AttributeMap::from_iter([
            ("view".into(), "timeline".into()),
        ]))
}
