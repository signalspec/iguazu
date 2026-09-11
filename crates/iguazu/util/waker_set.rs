use std::{mem, sync::Mutex, task::Waker};

pub struct WakerSet(Mutex<WakerSetUnlocked>);

impl WakerSet {
    pub fn new() -> Self {
        Self(Mutex::new(WakerSetUnlocked::new()))
    }

    pub fn insert(&self, waker: &Waker) {
        self.0.lock().unwrap().insert(waker);
    }

    pub fn wake_all(&self) {
        let wakers = mem::take(&mut *self.0.lock().unwrap());
        wakers.wake_all();
    }

}

#[derive(Default)]
pub struct WakerSetUnlocked {
    wakers: Vec<Waker>,
}

impl WakerSetUnlocked {
    pub fn new() -> Self {
        Self {
            wakers: Vec::new(),
        }
    }

    pub fn insert(&mut self, waker: &Waker) {
        if !self.wakers.iter().any(|w| waker.will_wake(w)) {
            self.wakers.push(waker.clone());
        }
    }

    pub fn wake_all(self) {
        for waker in self.wakers {
            waker.wake();
        }
    }
}
