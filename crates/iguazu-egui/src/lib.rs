mod util;
mod color;
pub mod egui_util;

pub mod timeline;
pub mod table;

use std::{future::Future, pin::Pin, sync::Arc, task::{Poll, Waker}};

use async_executor::Executor;
use iguazu::view::ViewManager;
pub use timeline::TimelineView;

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
    executor: Arc<Executor<'static>>,

    view_manager: ViewManager,

    /// Waker that requests egui repaint on wake.
    waker: Waker,
}
impl ViewerContext {
    pub fn new(executor: Arc<Executor<'static>>, egui_ctx: &egui::Context) -> Self {
        let waker: Waker = Arc::new(RepaintWaker { context: egui_ctx.clone() }).into();
        Self {
            executor,
            view_manager: ViewManager::new(waker.clone()),
            waker,
        }
    }

    pub fn begin(&mut self) {
        self.view_manager.begin();
    }

    pub fn end(&mut self) {
        self.view_manager.end();
    }

    pub fn executor(&self) -> &Arc<Executor<'static>> {
        &self.executor
    }

    pub fn spawn<F>(&self, fut: F) -> async_executor::Task<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.executor.spawn(fut)
    }

    pub fn waker(&self) -> &Waker {
        &self.waker
    }

    pub fn poll<T>(&self, f: Pin<&mut impl Future<Output = T>>) -> Poll<T> {
        let mut cx = std::task::Context::from_waker(&self.waker);
        f.poll(&mut cx)
    }

    pub fn poll_unpin<T>(&self, f: &mut (impl Future<Output = T> + Unpin)) -> Poll<T> {
        self.poll(Pin::new(f))
    }
}
