#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use eframe::{egui, CreationContext};
use egui::Frame;

use iguazu::{schema::{attribute::DefaultView, Entity, EntityStream}, stream::ArcStream};
use iguazu_egui::{table::TableView, timeline::TimelineView, ViewerContext};

#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), eframe::Error> {
    use std::sync::Arc;
    use std::future;

    use clap::Parser;
    use futures_lite::future::block_on;

    use iguazu::cli::ImportOpts;
    use iguazu::import::IMPORTERS;

    #[derive(Parser)]
    #[command(author, version, about, long_about = None)]
    struct Cli {
        #[clap(flatten)]
        import: ImportOpts,
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

    let (mut entity, completion) = block_on(cli.import.import(IMPORTERS, executor.clone())).expect("Failed to load file");
    block_on(completion).expect("Failed to complete import");

    entity.build_summaries(&executor);

    let view = entity.display_default();

    let options = eframe::NativeOptions {
        ..Default::default()
    };

    eframe::run_native(
        "Iguazu Viewer",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc, entity, view)))),
    )
}

// When compiling to web using trunk:
#[cfg(target_arch = "wasm32")]
fn main() {
    use eframe::wasm_bindgen::JsCast as _;

    // Redirect `log` message to `console.log` and friends:
    eframe::WebLogger::init(log::LevelFilter::Debug).ok();

    let web_options = eframe::WebOptions::default();

    wasm_bindgen_futures::spawn_local(async {
        use iguazu::{schema::{AttributeMap, EntityKind, Field}, storage::MemoryStream};

        let document = web_sys::window()
            .expect("No window")
            .document()
            .expect("No document");

        let canvas = document
            .get_element_by_id("the_canvas_id")
            .expect("Failed to find the_canvas_id")
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("the_canvas_id was not a HtmlCanvasElement");

        let analog = Entity::new(EntityKind::Number {
            data: MemoryStream::new(&(0..1000).map(|i| (i as f32 * 0.01).sin()).collect::<Vec<f32>>()) as ArcStream,
        }).with_attribute("sample_rate", 100.0)
        .with_attribute("number:range", AttributeMap::from_iter([
            ("min".into(), (-1.0).into()),
            ("max".into(), 1.0.into()),
        ]));

        let digital = Entity::new(EntityKind::Logic {
            data: MemoryStream::new(&(0..255u8).collect::<Vec<u8>>()) as ArcStream,
            bits: vec![
                Field { name: "bit0".into(), attributes: AttributeMap::default() },
                Field { name: "bit1".into(), attributes: AttributeMap::default() },
                Field { name: "bit2".into(), attributes: AttributeMap::default() },
                Field { name: "bit3".into(), attributes: AttributeMap::default() },
            ]
        }).with_attribute("sample_rate", 32.0);

        let entity = Entity::record()
            .with_child("analog".into(), analog)
            .with_child("digital".into(), digital);
        let view = Some(DefaultView::Timeline);
        log::debug!("Entity: {entity:?}, view: {view:?}");

        let start_result = eframe::WebRunner::new()
            .start(
                canvas,
                web_options,
                Box::new(move |cc| Ok(Box::new(App::new(cc, entity, view)))),
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

struct App {
    entity: EntityStream,
    view: Option<DefaultView>,
    vctx: ViewerContext,
}
impl App {
    fn new(cc: &CreationContext, entity: Entity<ArcStream>, view: Option<DefaultView>) -> Self {
        cc.egui_ctx.tessellation_options_mut(|o| {
            // Rounding causes jitter and gaps in timeline logic traces
            o.round_line_segments_to_pixels = false;
            o.round_rects_to_pixels = false;
        });

        App {
            view,
            entity,
            vctx: ViewerContext::new(&cc.egui_ctx),
        }
    }
}

impl eframe::App for App {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.vctx.begin();

        let frame = Frame::central_panel(&*ctx.style()).inner_margin(0.0);
        egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
            match self.view {
                Some(DefaultView::Table) => TableView::new().show(&mut self.vctx, ui, &mut self.entity),
                Some(DefaultView::Timeline) => TimelineView::new().show(&mut self.vctx, ui, &mut self.entity),
                None => {
                    ui.label("unknown view");
                }
            }
        });

        self.vctx.end();
    }
}
