use std::{mem, sync::Mutex, task::Waker};

pub struct WakerSet {
    wakers: Mutex<Vec<Waker>>,
}

impl WakerSet {
    pub fn new() -> Self {
        Self {
            wakers: Mutex::new(Vec::new()),
        }
    }

    pub fn insert(&self, waker: &Waker) {
        let mut wakers = self.wakers.lock().unwrap();
        if !wakers.iter().any(|w| waker.will_wake(w)) {
            wakers.push(waker.clone());
        }
    }

    pub fn wake_all(&self) {
        let wakers = mem::take(&mut *self.wakers.lock().unwrap());
        for waker in wakers {
            waker.wake();
        }
    }
}
