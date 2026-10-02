///! Utilities for making things Send and Sync on non-Wasm platforms only
use std::pin::Pin;

cfg_select! {
    target_arch = "wasm32" => {
        pub trait NonWasmSend {}
        impl<T> NonWasmSend for T {}

        pub trait NonWasmSync {}
        impl<T> NonWasmSync for T {}

        pub use async_executor::LocalExecutor as Executor;
    }
    _ => {
        pub use std::marker::Send as NonWasmSend;
        pub use std::marker::Sync as NonWasmSync;
        pub use async_executor::Executor;
    }
}

pub trait SendFuture: Future + NonWasmSend {}
impl<T: Future + NonWasmSend> SendFuture for T {}

pub type BoxFuture<'a, T> = Pin<Box<dyn SendFuture<Output = T> + 'a>>;

pub trait SendAsyncRead: futures_lite::AsyncRead + NonWasmSend + NonWasmSync {}
impl<T: futures_lite::AsyncRead + NonWasmSend + NonWasmSync> SendAsyncRead for T {}

pub trait SendAsyncBufRead: futures_lite::AsyncBufRead + SendAsyncRead {}
impl<T: futures_lite::AsyncBufRead + NonWasmSend + NonWasmSync> SendAsyncBufRead for T {}

pub trait SendAsyncWrite: futures_lite::AsyncWrite + NonWasmSend {}
impl<T: futures_lite::AsyncWrite + NonWasmSend> SendAsyncWrite for T {}
