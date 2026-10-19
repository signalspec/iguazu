#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use std::sync::Arc;
use std::task::Poll;

use async_executor::Task;
use eframe::{egui, CreationContext};
use egui::{
    Align2, Area, Button, Color32, Context, Direction, Frame, Id, Image, ImageSource, Layout, Modal, ModalResponse, Order, Rect, RichText, Sense, TextStyle, Ui, UiBuilder, UiKind, Vec2, Widget, menu::{MenuButton, menu_style}, util::History
};

use iguazu::import::{IMPORTERS, ImportError, Importer};
use iguazu::io::{IoError, ReadableFile};
use iguazu::schema::{EntitySchema, EntityStream};
use iguazu::storage::{Pool, Storage, MemoryStorage};
use iguazu_egui::ViewerContext;
use iguazu_egui::import::{ImportResponse, ImportUi};
use iguazu_egui::show_entity;
use rfd::AsyncFileDialog;

mod welcome;

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

    let executor = Arc::new(iguazu::send::Executor::new());
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

    let executor = Arc::new(iguazu::send::Executor::new());
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
    Viewer {
        source: Source,
        entity: EntityStream,
    },
}

enum Source {
    File {
        importer: Box<dyn Importer>,
    },
    #[cfg(feature = "capture")]
    Capture {
        session: iguazu_egui::config::ConfigEdit<Box<dyn iguazu_capture::DeviceSession>>,
        config_modal_open: bool,
        running: Option<Task<Result<(), iguazu_capture::CaptureError>>>,
        error: Option<String>,
    },
    Demo,
}

impl Source {
    fn id(&self) -> Id {
        match self {
            Source::Demo => Id::unique("demo"),
            Source::File { importer } => {
                if let Some(url) = importer.file().url() {
                    Id::unique(url.as_str())
                } else {
                    Id::unique(importer.file().filename())
                }
            }
            #[cfg(feature = "capture")]
            Source::Capture { session, .. } => Id::unique(session.inner().device().identifier()),
        }
    }
}

enum PendingSource {
    Import(ImportUi),
    Loading {
        importer: Box<dyn Importer>,
        task: Task<Result<EntityStream, ImportError>>,
    },
    #[cfg(feature = "capture")]
    CaptureDevice(String, Task<Result<Box<dyn iguazu_capture::DeviceSession>, iguazu_capture::CaptureError>>),
    Error { name: String, title: String, message: String },
}

struct App {
    vctx: ViewerContext,

    #[cfg(not(target_arch = "wasm32"))]
    window: Option<Arc<winit::window::Window>>,

    file_picker_task: Option<Task<Option<Result<Arc<dyn ReadableFile>, IoError>>>>,

    #[cfg(feature = "capture")]
    device_watch: iguazu_capture::DeviceWatch,

    pending_source: Option<PendingSource>,
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

            #[cfg(feature = "capture")]
            device_watch: iguazu_capture::DeviceWatch::new(&pool.executor, iguazu_capture::DRIVERS),

            pending_source: None,
            vctx: ViewerContext::new(pool, storage, &cc.egui_ctx),
            frame_time_history: History::new(0..max_len, max_age),
            enable_debug_ui: std::env::var("IGUAZU_DEBUG_UI").is_ok(),

            #[cfg(not(target_arch = "wasm32"))]
            window: cc.winit_window().cloned(),
        }
    }

    fn set_viewer(&mut self, source: Source, entity: EntityStream) {
        self.state = AppState::Viewer {
            source,
            entity,
        };
    }

    fn set_file(&mut self, file: Arc<dyn ReadableFile>) {
        match IMPORTERS.importer_for_file(file.clone()) {
            Some(importer) => self.prompt_import_options(importer),
            None => {
                self.pending_source = Some(PendingSource::Error {
                    name: file.filename().to_string(),
                    title: "Unsupported file".to_string(),
                    message: "No import format matched the file".to_string(),
                });
            },
        }
    }

    fn prompt_import_options(&mut self, importer: Box<dyn Importer>) {
        if importer.should_show_options() {
            self.pending_source = Some(PendingSource::Import(ImportUi::new(importer)));
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
        self.pending_source = Some(PendingSource::Loading { importer, task });
    }

    fn source_header_ui(&mut self, ui: &mut Ui) {
        menu_style(ui.style_mut());

        let label = if let Some(pending_source) = &self.pending_source {
            match pending_source {
                PendingSource::Import(import_ui) => import_ui.inner().file().filename(),
                PendingSource::Loading { importer, .. } => importer.file().filename(),
                PendingSource::Error { name: filename, .. } => filename.as_str(),
                #[cfg(feature = "capture")]
                PendingSource::CaptureDevice(name, _) => name.as_str(),
            }
        } else {
            match &self.state {
                AppState::Welcome(_) => "Choose file or device",
                AppState::Viewer { source, .. } => match source {
                    Source::File { importer } => importer.file().filename(),
                    Source::Demo => "Demo",
                    #[cfg(feature = "capture")]
                    Source::Capture { session, .. } => session.inner().device().description(),
                },
            }
        };

        MenuButton::from_button(Button::new((label, icon_chevron_down())).image_tint_follows_text_color(true)).ui(ui, |ui| {
            if ui.button("Open file…").clicked() {
                self.pick_file();
            }

            #[cfg(feature = "capture")]
            {
                ui.separator();
                ui.label("Capture from device:");

                let selected_device = capture_device_list_ui(ui, &mut self.device_watch);

                if let Some(device) = selected_device {
                    self.select_device(device);
                }
            }
        });

        if self.pending_source.is_none() {
            match &mut self.state {
                #[cfg(feature = "capture")]
                AppState::Viewer { source: Source::Capture { session, config_modal_open, running, error }, entity } => {
                    capture_config_button(ui, session, config_modal_open);
                    capture_start_stop_button(ui, session, running, error, &self.vctx, entity);
                    if let Some(e) = error.as_ref() {
                        if error_modal(ui.ctx(), "Capture error", e).should_close() {
                            *error = None;
                        }
                    }
                }
                _ => {}
            }
        }
    }

    #[cfg(feature = "capture")]
    fn select_device(&mut self, device: Arc<dyn iguazu_capture::Device>) {
        let name = device.description().to_owned();
        let task = self.vctx.spawn(device.open());
        self.pending_source = Some(PendingSource::CaptureDevice(name, task));
    }

    #[cfg(feature = "capture")]
    fn set_capture_session(&mut self, session: Box<dyn iguazu_capture::DeviceSession>) {
        let empty = EntityStream::group()
            .with_attribute(iguazu::schema::attribute::core::ROLE, iguazu::schema::attribute::core::Role::Capture);
        let session = iguazu_egui::config::ConfigEdit::new(session);
        self.set_viewer(Source::Capture {
            session,
            config_modal_open: false,
            running: None,
            error: None
        }, empty);
    }

    fn pick_file(&mut self) {
        #[allow(unused_mut)]
        let mut dialog = AsyncFileDialog::new();

        #[cfg(not(target_arch = "wasm32"))]
        if let Some(window) = &self.window {
            dialog = dialog.set_parent(&window);
        }

        self.file_picker_task = Some(self.vctx.spawn(async move {
            let res = dialog.pick_file().await?;

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

    fn logic(&mut self, _ctx: &Context, _frame: &mut eframe::Frame) {
        #[cfg(feature = "capture")] {
            let res = self.vctx.poll_fn(|c| self.device_watch.poll(c));
            if let Some(dev) = res.requested_device {
                self.select_device(dev);
            }
        }

        if let Some(res) = self.vctx.poll_unpin_take(&mut self.file_picker_task) {
            match res {
                None => { /* file picker cancelled */ }
                Some(Ok(file)) => {
                    self.set_file(file);
                }
                Some(Err(e)) => {
                    self.pending_source = Some(PendingSource::Error {
                        name: e.file.as_ref().and_then(|n| n.split('/').next_back()).unwrap_or("Unknown file").to_string(),
                        title: "Failed to open file".to_string(),
                        message: e.to_string(),
                    });
                }
            }
        }

        self.pending_source = match self.pending_source.take() {
            Some(PendingSource::Loading { importer, mut task }) => {
                if let Poll::Ready(result) = self.vctx.poll_unpin(&mut task) {
                    match result {
                        Ok(entity) => {
                            self.set_viewer(Source::File { importer }, entity);
                            None
                        }
                        Err(e) => {
                            let name = importer.file().filename().to_owned();
                            Some(PendingSource::Error {
                                name,
                                title: "Import failed".to_string(),
                                message: e.to_string(),
                            })
                        }
                    }
                } else {
                    Some(PendingSource::Loading { importer, task })
                }
            }
            #[cfg(feature = "capture")]
            Some(PendingSource::CaptureDevice(name, mut task)) => {
                if let Poll::Ready(res) = self.vctx.poll_unpin(&mut task) {
                    match res {
                        Ok(session) => {
                            self.set_capture_session(session);
                            None
                        }
                        Err(e) => {
                            Some(PendingSource::Error {
                                name,
                                title: "Failed to open device".to_string(),
                                message: e.to_string(),
                            })
                        }
                    }
                } else {
                    Some(PendingSource::CaptureDevice(name, task))
                }
            }
            other => other
        };

        match &mut self.state {
            #[cfg(feature = "capture")]
            AppState::Viewer { source: Source::Capture { running, error, .. }, .. } => {
                match self.vctx.poll_unpin_take(running) {
                    Some(Err(e)) => {
                        *error = Some(e.to_string());
                    }
                    Some(Ok(())) | None => {}
                }
            }
            _ => {}
        }
    }

    fn ui(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        self.vctx.begin();

        if self.file_picker_task.is_some() {
            ui.disable();
        }

        cfg_select! {
            target_arch = "wasm32" => {
                egui::Panel::top("top_bar")
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
                                    self.pending_source = None;
                                }
                            }
                        }
                    }
                    PendingSource::Loading { .. } => {
                        centered_box(ui, Vec2::new(300.0, 100.0), Layout::top_down(egui::Align::Center), |ui| {
                            ui.spinner();
                            ui.add_space(10.0);
                            ui.label(RichText::new(
                                format!("Importing...")
                            ).size(20.0));
                        });
                    }
                    #[cfg(feature = "capture")]
                    PendingSource::CaptureDevice(_, _) => {
                        centered_box(ui, Vec2::new(300.0, 100.0), Layout::top_down(egui::Align::Center), |ui| {
                            ui.spinner();
                            ui.add_space(10.0);
                            ui.label(RichText::new(
                                format!("Opening capture device...")
                            ).size(20.0));
                        });
                    }
                    PendingSource::Error { title, message, .. } => {
                        let close = centered_box(ui, Vec2::new(300.0, 100.0), Layout::top_down(egui::Align::Min), |ui| {
                            ui.heading(title);
                            ui.label(message.as_str());
                            ui.button("OK").clicked()
                        });
                        if close {
                            self.pending_source = None;
                        }
                    }
                }
            } else {
                match &mut self.state {
                    AppState::Welcome(welcome) => {
                        match welcome.show(&mut self.vctx, ui) {
                            Some(WelcomeResponse::PickFile) => self.pick_file(),
                            Some(WelcomeResponse::Entity(entity)) => {
                                self.set_viewer(Source::Demo, entity)
                            }
                            None => {}
                        }
                    }
                    AppState::Viewer { entity, source } => {
                        ui.push_id(source.id(), |ui| {
                            show_entity(&mut self.vctx, ui, entity);
                        });
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

    fn update_frame_time(&mut self, ctx: &Context, frame: &mut eframe::Frame) {
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

#[cfg(feature = "capture")]
fn capture_device_list_ui(ui: &mut Ui, device_watch: &mut iguazu_capture::DeviceWatch) -> Option<Arc<dyn iguazu_capture::Device>> {
    let mut any_device = false;
    let mut selected_device = None;

    for device in device_watch.devices() {
        any_device = true;
        if ui.add(Button::new(device.description())).clicked() {
            selected_device = Some(device.clone());
        }
    }

    if let Err(s) = device_watch.status() {
        ui.add_enabled(false, egui::Button::new(s));
    } else if device_watch.needs_request_device() {
        if ui.add(Button::new("Connect device")).clicked() {
            device_watch.request_device();
        }
    } else if !any_device {
        ui.add_enabled(false, egui::Button::new("No devices found"));
    }

    selected_device
}

#[cfg(feature = "capture")]
fn capture_config_button(
    ui: &mut Ui,
    session: &mut iguazu_egui::config::ConfigEdit<Box<dyn iguazu_capture::DeviceSession + 'static>>,
    config_modal_open: &mut bool
) {
    let label = session.inner().config_summary().unwrap_or("Settings".into());
    let button_response = Button::new((icon_settings(), label, icon_chevron_down()))
        .image_tint_follows_text_color(true)
        .ui(ui);

    if button_response.clicked() {
        *config_modal_open = true;
    }

    if *config_modal_open {
        let modal = popup_modal(button_response, egui::Id::unique("capture-session-config"));

        let modal_response = modal.show(ui.ctx(), |ui| {
            ui.allocate_space(Vec2::new(380.0, 12.0));
            session.show(ui)
        });

        if modal_response.should_close() {
            *config_modal_open = false;
        }
    }
}

#[cfg(feature = "capture")]
fn capture_start_stop_button(
    ui: &mut Ui,
    session: &mut iguazu_egui::config::ConfigEdit<Box<dyn iguazu_capture::DeviceSession + 'static>>,
    running: &mut Option<Task<Result<(), iguazu_capture::CaptureError>>>,
    error: &mut Option<String>,
    vctx: &ViewerContext,
    entity: &mut EntityStream
) {
    if !running.is_some() {
        if ui.add(Button::new((icon_play().tint(Color32::GREEN), "Start"))).clicked() {
            match session.inner().start_capture(vctx.executor(), vctx.default_storage()) {
                Ok((mut new_entity, task)) => {
                    new_entity.build_summaries(&vctx.executor(), vctx.default_storage()).detach();
                    *entity = new_entity;
                    *running = Some(task);
                    *error = None;
                }
                Err(e) => {
                    *error = Some(e.to_string());
                }
            }
        }
    } else {
        if ui.add(Button::new((icon_stop().tint(Color32::RED), "Stop"))).clicked() {
            drop(running.take());
        }
    }
}

fn error_modal(ctx: &Context, heading: &str, message: &str) -> ModalResponse<()> {
    Modal::new(egui::Id::unique("capture-error")).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.add(icon_alert_triangle().tint(Color32::GOLD).fit_to_exact_size(Vec2::splat(32.0)));
            ui.heading(heading);
        });
        ui.add_space(8.0);
        ui.label(message);
        ui.add_space(8.0);
        egui::Sides::new().show(ui, |_| {}, |ui| {
            if ui.button("Ok").clicked() {
                ui.close();
            }
        });
    })
}

/// Like `MenuButton` but actually a `Modal` positioned like a `Popup`, because
/// egui doesn't support nesting popups for the dropdown suggestions.
fn popup_modal(button_response: egui::Response, id: egui::Id) -> Modal {
    let area = Area::new(id)
        .kind(UiKind::Modal)
        .sense(Sense::hover())
        .pivot(Align2::LEFT_TOP)
        .fixed_pos(button_response.rect.left_bottom())
        .order(Order::Middle) // Popup is normally `Foreground` but need to make sure that dropdowns inside it are on top
        .interactable(true);

    let modal = Modal {
        area,
        backdrop_color: Color32::TRANSPARENT,
        frame: None,
    };
    modal
}

fn icon(i: ImageSource<'static>) -> Image<'static> {
    Image::new(i).fit_to_exact_size(Vec2::splat(16.0))
}

fn icon_chevron_down() -> Image<'static> {
    icon(egui::include_image!("../icons/chevron-down.svg"))
}

fn icon_settings() -> Image<'static> {
    icon(egui::include_image!("../icons/settings.svg"))
}

fn icon_play() -> Image<'static> {
    icon(egui::include_image!("../icons/play.svg"))
}

fn icon_stop() -> Image<'static> {
    icon(egui::include_image!("../icons/stop.svg"))
}

fn icon_alert_triangle() -> Image<'static> {
    icon(egui::include_image!("../icons/alert-triangle.svg"))
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
