mod util;
mod color;
pub mod egui_util;

pub mod timeline;
pub mod table;
pub mod import;
pub mod config;

use std::{any::{Any, TypeId}, cell::{Ref, RefCell, RefMut}, collections::{HashMap, hash_map::Entry}, future::Future, pin::Pin, sync::Arc, task::{Poll, Waker}};

use egui::{Id, Ui};
use iguazu::{send::{Executor, NonWasmSend, SendFuture}, schema::{attribute::display::Layout, EntityStream}, storage::{Pool, Storage}, view::ViewManager};
use iguazu_egui_heatmap::HeatmapRenderer;
pub use timeline::TimelineView;

pub fn show_entity(vctx: &mut ViewerContext, ui: &mut Ui, entity: &mut EntityStream) {
    match entity.display_default() {
        Some(Layout::Table) => table::TableView::new().show(vctx, ui, entity),
        Some(Layout::Timeline) => TimelineView::new().show(vctx, ui, entity),
        None => {
            ui.label("unknown view");
        }
    }
}

struct RepaintWaker {
    context: egui::Context,
}

impl std::task::Wake for RepaintWaker {
    fn wake(self: Arc<Self>) {
        self.context.request_repaint();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.context.request_repaint();
    }
}

pub struct ViewerContext {
    pool: Arc<Pool>,
    default_storage: Arc<dyn Storage>,

    heatmap_renderer: Arc<HeatmapRenderer>,
    view_manager: ViewManager,
    frame_textures: RefCell<TextureStorage>,

    /// Waker that requests egui repaint on wake.
    waker: Waker,
}

impl ViewerContext {
    pub fn new(pool: Arc<Pool>, default_storage: Arc<dyn Storage>, egui_ctx: &egui::Context, wgpu: &egui_wgpu::RenderState) -> Self {
        let waker: Waker = Arc::new(RepaintWaker { context: egui_ctx.clone() }).into();
        let heatmap_renderer = Arc::new(HeatmapRenderer::new(wgpu));
        Self {
            pool,
            default_storage,
            heatmap_renderer,
            view_manager: ViewManager::new(),
            frame_textures: RefCell::new(TextureStorage::new()),
            waker,
        }
    }

    pub fn begin(&mut self) {
        self.view_manager.begin(&self.waker);
    }

    pub fn end(&mut self) {
        self.view_manager.end();
        self.frame_textures.get_mut().end();
    }

    pub fn pool(&self) -> &Arc<Pool> {
        &self.pool
    }

    pub fn default_storage(&self) -> &Arc<dyn Storage> {
        &self.default_storage
    }

    pub fn executor(&self) -> &Arc<Executor<'static>> {
        &self.pool.executor
    }

    pub fn spawn<F>(&self, fut: F) -> async_executor::Task<F::Output>
    where
        F: SendFuture + 'static,
        F::Output: NonWasmSend + 'static,
    {
        self.executor().spawn(fut)
    }

    pub fn waker(&self) -> &Waker {
        &self.waker
    }

    pub fn poll_fn<T>(&self, f: impl FnOnce(&mut std::task::Context) -> T) -> T {
        let mut cx = std::task::Context::from_waker(&self.waker);
        f(&mut cx)
    }

    pub fn poll<T>(&self, f: Pin<&mut impl Future<Output = T>>) -> Poll<T> {
        self.poll_fn(|cx| f.poll(cx))
    }

    pub fn poll_unpin<T>(&self, f: &mut (impl Future<Output = T> + Unpin)) -> Poll<T> {
        self.poll(Pin::new(f))
    }

    pub fn poll_unpin_take<T>(&self, f: &mut Option<impl Future<Output = T> + Unpin>) -> Option<T> {
        match self.poll_unpin(f.as_mut()?) {
            Poll::Ready(res) => {
                f.take();
                Some(res)
            }
            Poll::Pending => None,
        }
    }

    pub fn get_texture<T>(&self, id: Id) -> Option<Box<T>>
    where
        T: Any + 'static,
    {
        self.frame_textures.borrow_mut().this_frame.remove(&id.with(TypeId::of::<T>()))
            .and_then(|slot| slot.downcast::<T>().ok())
    }

    pub fn put_texture<T>(&self, id: Id, data: Box<T>)
    where
        T: Any + 'static,
    {
        self.frame_textures.borrow_mut().next_frame.insert(id.with(TypeId::of::<T>()), data);
    }
}

struct TextureStorage {
    this_frame: HashMap<Id, Box<dyn Any>>,
    next_frame: HashMap<Id, Box<dyn Any>>,
}

impl TextureStorage {
    pub fn new() -> Self {
        Self {
            this_frame: HashMap::new(),
            next_frame: HashMap::new(),
        }
    }

    pub fn end(&mut self) {
        std::mem::swap(&mut self.this_frame, &mut self.next_frame);
        self.next_frame.clear();
    }
}
