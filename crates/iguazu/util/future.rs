use std::{pin::Pin, task::{Context, Poll}};

pub enum FutureOutput<T: Future> {
    Future(T),
    Output(T::Output),
}

impl<T: Future + Unpin> FutureOutput<T> {
    pub fn poll(&mut self, cx: &mut Context<'_>) {
        if let FutureOutput::Future(f) = self {
            match Pin::new(f).poll(cx) {
               Poll::Pending => (),
               Poll::Ready(res) => {
                   *self = FutureOutput::Output(res);
               }
            }
        }
    }

    pub fn get(&self) -> Poll<&T::Output> {
        match self {
            FutureOutput::Future(_) => Poll::Pending,
            FutureOutput::Output(o) => Poll::Ready(o),
        }
    }

    pub fn get_mut(&mut self) -> Poll<&mut T::Output> {
        match self {
            FutureOutput::Future(_) => Poll::Pending,
            FutureOutput::Output(o) => Poll::Ready(o),
        }
    }
}
