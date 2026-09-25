#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use std::sync::Arc;
use std::task::Poll;

use async_executor::Task;
use eframe::{egui, CreationContext};
use egui::menu::{MenuButton, menu_style};
use egui::{Atom, AtomExt, Button, RichText, TextStyle, Vec2};
use egui::{util::History, Direction, Frame, Layout, Rect, Ui, UiBuilder};

use iguazu::import::{IMPORTERS, ImportError, Importer};
use iguazu::io::{IoError, ReadableFile};
use iguazu::schema::{EntitySchema, EntityStream};
use iguazu::storage::{Pool, Storage, MemoryStorage};
use iguazu_egui::ViewerContext;
use iguazu_egui::import::{ImportResponse, ImportUi};
use rfd::AsyncFileDialog;

mod welcome;
mod viewer;

use crate::viewer::Viewer;
use crate::welcome::{Welcome, WelcomeResponse};

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    match run() {
        Ok(()) => {}
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn run() -> Result<(), Box<dyn std::error::Error>> {
    use std::sync::Arc;
    use std::future;

    use clap::Parser;
    use futures_lite::future::block_on;

    use iguazu::cli::ImportOpts;
    use iguazu::import::IMPORTERS;
    use iguazu_egui::egui_util::titlebar::ViewportBuilderExt;

    #[derive(Parser)]
    #[command(author, version, about, long_about = None)]
    struct Cli {
        #[clap(flatten)]
        import: Option<ImportOpts>,
    }

    // Log to stdout (if you run with `RUST_LOG=debug`).
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    let executor = Arc::new(async_executor::Executor::new());
    std::thread::spawn({
        let executor = executor.clone();
        move || {
            block_on(executor.run(future::pending::<()>()));
        }
    });

    let pool = Arc::new(iguazu::storage::Pool::new(executor.clone(), 16 * 1024 * 1024));
    let storage = Arc::new(MemoryStorage) as Arc<dyn Storage>;

    let to_import = if let Some(import_opts) = cli.import {
        let importer = block_on(import_opts.importer(IMPORTERS))?;
        let schema = block_on(import_opts.specified_schema())?;
        let skip_options_ui = import_opts.format_specified() || schema.is_some();
        Some((importer, skip_options_ui, schema))
    } else {
        None
    };

    let viewport = egui::ViewportBuilder::default()
        .with_custom_title_bar()
        .with_app_id("org.signalspec.iguazu");

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "Iguazu Viewer",
        options,
        Box::new(|cc| {
            let mut app = Box::new(App::new(cc, pool, storage));

            if let Some((importer, skip_options_ui, schema)) = to_import {
                if skip_options_ui {
                    app.import(importer, schema);
                } else {
                    app.prompt_import_options(importer);
                }
            }

            Ok(app)
        })
    )?;
    Ok(())
}

// When compiling to web using trunk:
#[cfg(target_arch = "wasm32")]
fn main() {
    use eframe::wasm_bindgen::JsCast as _;

    // Redirect `log` message to `console.log` and friends:
    eframe::WebLogger::init(log::LevelFilter::Debug).ok();

    let web_options = eframe::WebOptions::default();

    let query_string = web_sys::window()
        .expect("No window")
        .location()
        .search()
        .expect("Failed to get query string");

    let url_params: Vec<_> = url::form_urlencoded::parse(query_string.trim_start_matches("?").as_bytes()).collect();
    let input_url = url_params.iter().find(|(key, _)| key == "url").and_then(|(_, value)| value.to_string().parse().ok());

    let executor = Arc::new(async_executor::Executor::new());
    wasm_bindgen_futures::spawn_local({
        let executor = executor.clone();
        async move {
            executor.run(futures_lite::future::pending::<()>()).await;
        }
    });
    let pool = Arc::new(iguazu::storage::Pool::new(executor, 64 * 1024 * 1024));
    let storage = Arc::new(MemoryStorage) as Arc<dyn Storage>;

    wasm_bindgen_futures::spawn_local(async {
        let document = web_sys::window()
            .expect("No window")
            .document()
            .expect("No document");

        let canvas = document
            .get_element_by_id("the_canvas_id")
            .expect("Failed to find the_canvas_id")
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("the_canvas_id was not a HtmlCanvasElement");

        let file = input_url.map(|url| {
            Arc::new(iguazu::io::WebFetchFile::new(url)) as Arc<dyn iguazu::io::ReadableFile>
        });

        let start_result = eframe::WebRunner::new()
            .start(
                canvas,
                web_options,
                Box::new(move |cc| {
                    let mut app = Box::new(App::new(cc, pool.clone(), storage.clone()));
                    if let Some(file) = file {
                        app.import(Box::new(iguazu::import::IzsImporter::new(file)), None);
                    }
                    Ok(app)
                })
            )
            .await;

        // Remove the loading text and spinner:
        if let Some(loading_text) = document.get_element_by_id("loading_text") {
            match start_result {
                Ok(_) => {
                    loading_text.remove();
                }
                Err(e) => {
                    loading_text.set_inner_html(
                        "<p> The app has crashed. See the developer console for details. </p>",
                    );
                    panic!("Failed to start eframe: {e:?}");
                }
            }
        }
    });
}

enum AppState {
    Welcome(welcome::Welcome),
    Loading(Task<Result<EntityStream, ImportError>>),
    Error(String, String),
    Viewer(viewer::Viewer),
}

enum Source {
    File {
        importer: Box<dyn Importer>,
    },
    Demo,
}

enum PendingSource {
    Import(ImportUi),
    Error { filename: String, title: String, message: String },
}

struct App {
    vctx: ViewerContext,

    file_picker_task: Option<Task<Option<Result<Arc<dyn ReadableFile>, IoError>>>>,

    pending_source: Option<PendingSource>,
    source: Option<Source>,
    state: AppState,

    frame_time_history: History<f32>,
    enable_debug_ui: bool,
}

impl App {
    fn new(cc: &CreationContext, pool: Arc<Pool>, storage: Arc<dyn Storage>) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);

        cc.egui_ctx.set_theme(egui::Theme::Dark);
        cc.egui_ctx.global_style_mut(|style| {
            style.text_styles.insert(TextStyle::Body, egui::FontId::proportional(16.0));
            style.text_styles.insert(TextStyle::Heading, egui::FontId::proportional(24.0));
        });
        cc.egui_ctx.tessellation_options_mut(|o| {
            // Rounding causes jitter and gaps in timeline logic traces
            o.round_line_segments_to_pixels = false;
            o.round_rects_to_pixels = false;
        });

        let max_age: f32 = 1.0;
        let max_len = (max_age * 300.0).round() as usize;

        App {
            state: AppState::Welcome(Welcome::new()),
            file_picker_task: None,
            pending_source: None,
            source: None,
            vctx: ViewerContext::new(pool, storage, &cc.egui_ctx),
            frame_time_history: History::new(0..max_len, max_age),
            enable_debug_ui: std::env::var("IGUAZU_DEBUG_UI").is_ok(),
        }
    }

    fn set_state(&mut self, source: Source, state: AppState) {
        self.state = state;
        self.source = Some(source);
        self.vctx.waker().wake_by_ref();
    }

    fn set_pending_source(&mut self, source: Option<PendingSource>) {
        self.pending_source = source;
        self.vctx.waker().wake_by_ref();
    }

    fn set_file(&mut self, file: Arc<dyn ReadableFile>) {
        match IMPORTERS.importer_for_file(file.clone()) {
            Some(importer) => self.prompt_import_options(importer),
            None => self.set_pending_source(Some(PendingSource::Error {
                filename: file.filename().to_string(),
                title: "Unsupported file".to_string(),
                message: "No import format matched the file".to_string(),
            })),
        }
    }

    fn prompt_import_options(&mut self, importer: Box<dyn Importer>) {
        if importer.should_show_options() {
            self.set_pending_source(Some(PendingSource::Import(ImportUi::new(importer))));
        } else {
            self.import(importer, None);
        }
    }

    fn import(&mut self, importer: Box<dyn Importer>, schema: Option<EntitySchema>) {
        let pool = self.vctx.pool().clone();
        let storage = self.vctx.default_storage().clone();
        let import_fut = importer.import(schema, pool.clone());
        let task = self.vctx.spawn(async move {
            let (mut entity, completion) = import_fut.await?;
            completion.detach();
            entity.build_summaries(&pool.executor, &storage).detach();
            Ok(entity)
        });
        self.set_state(Source::File { importer }, AppState::Loading(task));
    }

    fn set_import_error(&mut self, title: String, message: String) {
        self.state = AppState::Error(title, message);
        self.vctx.waker().wake_by_ref();
    }

    fn set_entity(&mut self, entity: EntityStream) {
        self.state = AppState::Viewer(Viewer::new(entity));
        self.vctx.waker().wake_by_ref();
    }

    fn source_header_ui(&mut self, ui: &mut Ui) {
        menu_style(ui.style_mut());

        let label = if let Some(pending_source) = &self.pending_source {
            match pending_source {
                PendingSource::Import(import_ui) => import_ui.inner().file().filename(),
                PendingSource::Error { filename, .. } => filename.as_str(),
            }
        } else {
            match &self.source {
                Some(Source::File { importer }) => importer.file().filename(),
                Some(Source::Demo) => "Demo",
                None => "Choose file or device",
            }
        };

        MenuButton::from_button(Button::new((label, icon_chevron_down())).image_tint_follows_text_color(true)).ui(ui, |ui| {
            if ui.button("Open file…").clicked() {
                self.pick_file();
            }
        });
    }

    fn pick_file(&mut self) {
        self.file_picker_task = Some(self.vctx.spawn(async move {
            #[cfg(not(target_arch = "wasm32"))]
            let res = AsyncFileDialog::new().pick_file().await?;

            #[cfg(target_arch = "wasm32")]
            let res = send_wrapper::SendWrapper::new(AsyncFileDialog::new().pick_file()).await?;

            #[cfg(not(target_arch = "wasm32"))]
            let file = iguazu::io::FsFile::open(res.inner().to_owned()).await;

            #[cfg(target_arch = "wasm32")]
            let file = Ok(iguazu::io::WebFile::new(res.inner().clone()));

            Some(file.map(|f| Arc::new(f) as Arc<dyn ReadableFile>))
        }));
    }
}

impl eframe::App for App {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }

    fn logic(&mut self, _ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(res) = self.vctx.poll_unpin_take(&mut self.file_picker_task) {
            match res {
                None => { /* file picker cancelled */ }
                Some(Ok(file)) => {
                    self.set_file(file);
                }
                Some(Err(e)) => {
                    self.set_pending_source(Some(PendingSource::Error {
                        filename: e.file.as_ref().and_then(|n| n.split('/').next_back()).unwrap_or("Unknown file").to_string(),
                        title: "Failed to open file".to_string(),
                        message: e.to_string(),
                    }));
                }
            }
        }

        match &mut self.state {
            AppState::Welcome(..) | AppState::Viewer(..) | AppState::Error(..) => {},
            AppState::Loading(task) => {
                if let Poll::Ready(result) = self.vctx.poll_unpin(task) {
                    match result {
                        Ok(entity) => self.set_entity(entity),
                        Err(e) => self.set_import_error("Import failed".into(), e.to_string()),
                    }
                }
            }
        }
    }

    fn ui(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        self.vctx.begin();

        if self.file_picker_task.is_some() {
            ui.disable();
        }

        cfg_select! {
            target_arch = "wasm32" => {
                egui::Panel::top(egui::Id::new("top_bar"))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            self.source_header_ui(ui);
                        });
                    });
            }
            _ => {
                iguazu_egui::egui_util::titlebar::TitleBar::new().show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.add_space(8.0);
                        self.source_header_ui(ui);
                    });
                });

            }
        }

        let central_panel = Frame::central_panel(ui.style()).inner_margin(0.0);
        egui::CentralPanel::default().frame(central_panel).show(ui, |ui| {
            if let Some(pending_source) = &mut self.pending_source {
                match pending_source {
                    PendingSource::Import(import_ui) => {
                        if let Some(response) = import_ui.show(ui) {
                            match response {
                                ImportResponse::Accepted => {
                                    let Some(PendingSource::Import(import_opts)) = self.pending_source.take() else { unreachable!() };
                                    self.import(import_opts.into_inner(), None);
                                }
                                ImportResponse::Cancelled => {
                                    self.set_pending_source(None);
                                }
                            }
                        }
                    }
                    PendingSource::Error { title, message, .. } => {
                        let close = centered_box(ui, Vec2::new(300.0, 100.0), Layout::top_down(egui::Align::Min), |ui| {
                            ui.heading(title);
                            ui.label(message.as_str());
                            ui.button("OK").clicked()
                        });
                        if close {
                            self.set_pending_source(None);
                        }
                    }
                }
            } else {
                match &mut self.state {
                    AppState::Welcome(welcome) => {
                        match welcome.show(&mut self.vctx, ui) {
                            Some(WelcomeResponse::PickFile) => self.pick_file(),
                            Some(WelcomeResponse::Entity(entity)) => {
                                self.source = Some(Source::Demo);
                                self.set_entity(entity)
                            }
                            None => {}
                        }
                    }
                    AppState::Loading(_) => {
                        centered_box(ui, Vec2::new(300.0, 100.0), Layout::top_down(egui::Align::Center), |ui| {
                            ui.spinner();
                            ui.add_space(10.0);
                            ui.label(RichText::new(
                                format!("Importing...")
                            ).size(20.0));
                        });
                    }
                    AppState::Error(title, message) => {
                        centered_box(ui, Vec2::new(300.0, 100.0), Layout::top_down(egui::Align::Min), |ui| {
                            ui.heading(title);
                            ui.label(message.as_str());
                        });
                    }
                    AppState::Viewer(viewer) => {
                        viewer.show(&mut self.vctx, ui);
                    }
                }
            }
        });

        if self.enable_debug_ui {
            self.debug_ui(ui);
        }

        self.vctx.end();
        self.update_frame_time(ui.ctx(), frame);
    }
}

impl App {
    fn debug_ui(&mut self, ui: &mut Ui) {
        let debug = Rect::from_x_y_ranges(ui.max_rect().x_range(), (ui.max_rect().bottom() - 20.0)..=ui.max_rect().bottom());
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(debug)
                .layout(Layout::centered_and_justified(Direction::TopDown)),
            |ui| {
                ui.horizontal(|ui| {
                    ui.label("Debug:");
                    self.frame_time_ui(ui);
                });
            }
        );
    }

    fn update_frame_time(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let now = ctx.input(|i| i.time);
        let previous_frame_time = frame.info().cpu_usage;

        let previous_frame_time = previous_frame_time.unwrap_or_default();
        if let Some(latest) = self.frame_time_history.latest_mut() {
            *latest = previous_frame_time; // rewrite history now that we know
        }
        self.frame_time_history.add(now, previous_frame_time);
    }

    fn frame_time_ui(&self, ui: &mut egui::Ui) {
        let is_anything_being_dragged = ui.ctx().dragged_id().is_some();
        let down = ui.input(|input| input.pointer.primary_down());
        let focused = ui.input(|i| i.focused);

        let pool_stats = self.vctx.pool().stats();
        let pool_usage = pool_stats.cache_usage as f32 / 1024.0 / 1024.0;
        let pool_limit = pool_stats.cache_limit as f32 / 1024.0 / 1024.0;

        if let Some(frame_time) = self.frame_time_history.average() {
            ui.label(format!(
                "Mean CPU usage: {:.2} ms / frame. Dragging: {is_anything_being_dragged}, down: {down}, focus: {focused}, cache: {pool_usage:.2} MiB / {pool_limit:.2} MiB",
                1e3 * frame_time
            ));
        }
    }
}

fn icon_chevron_down() -> Atom<'static> {
    egui::include_image!("../icons/chevron-down.svg").atom_max_height(16.0)
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
