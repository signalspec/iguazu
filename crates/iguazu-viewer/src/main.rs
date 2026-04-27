#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use std::sync::Arc;
use std::task::Poll;

use async_executor::Task;
use eframe::{egui, CreationContext};
use egui::{RichText, TextStyle};
use egui::{util::History, Direction, Frame, Layout, Rect, Ui, UiBuilder};

use iguazu::import::{ImportError, Importer};
use iguazu::schema::{EntitySchema, EntityStream};
use iguazu::storage::{Pool, Storage, MemoryStorage};
use iguazu_egui::ViewerContext;
use iguazu_egui::import::{ImportResponse, ImportUi};

mod welcome;
mod viewer;

use crate::viewer::Viewer;
use crate::welcome::{Welcome, WelcomeResponse};

#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), eframe::Error> {
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
        let importer = block_on(import_opts.importer(IMPORTERS)).expect("failed to choose importer");
        let schema = block_on(import_opts.specified_schema()).expect("failed to load schema");
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
    )
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
    ImportOptions(Option<ImportUi>),
    Loading(Task<Result<EntityStream, ImportError>>),
    Viewer(viewer::Viewer),
}

struct App {
    vctx: ViewerContext,
    state: AppState,
    filename: Option<String>,
    frame_time_history: History<f32>,
    enable_debug_ui: bool,
}

impl App {
    fn new(cc: &CreationContext, pool: Arc<Pool>, storage: Arc<dyn Storage>) -> Self {
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
            filename: None,
            vctx: ViewerContext::new(pool, storage, &cc.egui_ctx),
            frame_time_history: History::new(0..max_len, max_age),
            enable_debug_ui: std::env::var("IGUAZU_DEBUG_UI").is_ok(),
        }
    }

    fn set_state(&mut self, state: AppState) {
        self.state = state;
        self.vctx.waker().wake_by_ref();
    }

    fn prompt_import_options(&mut self, importer: Box<dyn Importer>) {
        if importer.should_show_options() {
            self.set_state(AppState::ImportOptions(Some(ImportUi::new(importer))));
        } else {
            self.import(importer, None);
        }
    }

    fn import(&mut self, importer: Box<dyn Importer>, schema: Option<EntitySchema>) {
        let pool = self.vctx.pool().clone();
        let storage = self.vctx.default_storage().clone();
        let task = self.vctx.spawn(async move {
            let (mut entity, completion) = importer.import(schema, pool.clone()).await?;
            pool.executor.spawn(completion).detach();
            entity.build_summaries(&pool.executor, &storage).detach();
            Ok(entity)
        });
        self.set_state(AppState::Loading(task));
    }

    fn return_to_welcome(&mut self) {
        self.filename = None;
        self.set_state(AppState::Welcome(Welcome::new()));
    }

    fn set_import_error(&mut self, message: String) {
        self.filename = None;
        self.set_state(AppState::Welcome(Welcome::with_error(message)));
    }

    fn set_entity(&mut self, entity: EntityStream) {
        self.set_state(AppState::Viewer(Viewer::new(entity)));
    }
}

impl eframe::App for App {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }

    fn ui(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        self.vctx.begin();

        #[cfg(not(target_arch = "wasm32"))]
        iguazu_egui::egui_util::titlebar::TitleBar::new().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                ui.add(egui::Label::new("Iguazu Viewer").selectable(false));
                if let Some(filename) = &self.filename {
                    ui.add_space(4.0);
                    ui.add(egui::Label::new(RichText::new(filename).weak()).selectable(false));
                }
            });
        });

        let central_panel = Frame::central_panel(ui.style()).inner_margin(0.0);
        egui::CentralPanel::default().frame(central_panel).show_inside(ui, |ui| {
            match &mut self.state {
                AppState::Welcome(welcome) => {
                    if let Some(response) = welcome.show(&mut self.vctx, ui) {
                        match response {
                            WelcomeResponse::Import { importer } => {
                                self.prompt_import_options(importer);
                            }
                            WelcomeResponse::Entity(entity) => {
                                self.set_entity(entity);
                            }
                        }
                    }
                }
                AppState::ImportOptions(import_ui) => {
                    if let Some(response) = import_ui.as_mut().unwrap().show(ui) {
                        match response {
                            ImportResponse::Accepted => {
                                let importer = import_ui.take().unwrap().into_inner();
                                self.import(importer, None);
                            }
                            ImportResponse::Cancelled => {
                                self.return_to_welcome();
                            }
                        }
                    }
                }
                AppState::Loading(task) => {
                    ui.vertical_centered(|ui| {
                        ui.add_space(100.0);
                        ui.spinner();
                        ui.add_space(10.0);
                        ui.label(RichText::new(
                            format!("Importing {}...", self.filename.as_deref().unwrap_or(""))
                        ).size(20.0));
                    });

                    if let Poll::Ready(result) = self.vctx.poll_unpin(task) {
                        match result {
                            Ok(entity) => self.set_entity(entity),
                            Err(e) => self.set_import_error(format!("Failed to import {}: {}", self.filename.as_deref().unwrap_or(""), e)),
                        }
                    }
                }
                AppState::Viewer(viewer) => {
                    viewer.show(&mut self.vctx, ui);
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
