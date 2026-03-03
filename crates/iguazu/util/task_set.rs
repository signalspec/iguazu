use std::{pin::Pin, sync::Arc, task::{Context, Poll}};

use async_executor::{Executor, Task};
use futures_util::{FutureExt};

#[must_use]
pub struct TaskSet<E> {
    tasks: Vec<Task<Result<(), E>>>,
}

impl<E> TaskSet<E> {
    pub fn new() -> Self {
        TaskSet { tasks: Vec::new() }
    }

    pub fn len(&self) -> usize {
        self.tasks.len()
    }

    pub fn push(&mut self, task: Task<Result<(), E>>) {
        self.tasks.push(task);
    }

    pub fn detach(self) {
        for task in self.tasks {
            task.detach();
        }
    }
}

impl<E> Future for TaskSet<E> {
    type Output = Result<(), E>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut i = 0;
        while i < self.tasks.len() {
            match self.tasks[i].poll_unpin(cx) {
                Poll::Ready(Ok(())) => {
                    drop(self.tasks.swap_remove(i));
                }
                Poll::Ready(Err(e)) => {
                    self.tasks.clear();
                    return Poll::Ready(Err(e));
                }
                Poll::Pending => {
                    i += 1;
                }
            }
        }

        if self.tasks.is_empty() {
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    }
}

pub struct TaskSetBuilder<E> {
    pub executor: Arc<Executor<'static>>,
    pub task_set: TaskSet<E>,
}

impl<E> TaskSetBuilder<E> {
    pub fn new(executor: Arc<Executor<'static>>) -> Self {
        TaskSetBuilder { executor, task_set: TaskSet::new() }
    }

    pub fn spawn<F>(&mut self, future: F)
    where
        F: std::future::Future<Output = Result<(), E>> + Send + 'static,
        E: Send + 'static,
    {
        let task = self.executor.spawn(future);
        self.task_set.push(task);
    }
}
