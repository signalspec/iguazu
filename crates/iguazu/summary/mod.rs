//! Downsampled overviews of data for zoomed-out views
use std::{fmt::Debug, mem, sync::{Arc, Mutex}, task::{Poll, Waker}};

use ecow::EcoString;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use crate::{stream::ArcStream, util::waker_set::WakerSetUnlocked};

mod build;

pub type LiveSummaryMap = IndexMap<EcoString, LiveSummary>;
pub type StoredSummaryMap<S> = IndexMap<EcoString, StoredSummary<S>>;

#[derive(Clone, Serialize, Deserialize)]
pub struct Summary<L> {
    #[serde(alias = "base_level")] // pre-0.1
    pub first_level: u8,
    pub levels: L,
}

pub type StoredSummary<D> = Summary<Box<[D]>>;
pub type BorrowedSummary<'a, D> = Summary<&'a [D]>;

impl<L> Debug for Summary<L> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Summary").finish()
    }
}

impl<L> Summary<L> {
    pub fn empty() -> Self where L: Default {
        Summary { first_level: 255, levels: Default::default() }
    }
}

impl<'a, D> Summary<&'a [D]> {
    pub fn max_level(&self) -> u8 {
        if self.levels.is_empty() {
            0
        } else {
            (self.first_level as usize + self.levels.len() - 1).try_into().unwrap_or(u8::MAX)
        }
    }

    pub fn get(&self, level: u8) -> SummaryLevel<&'a D> {
        if self.levels.len() > 0 && let Some(i) = level.checked_sub(self.first_level) {
            let len = self.levels.len() as u8;
            if let Some(above) = i.checked_sub(len) {
                SummaryLevel::Above {
                    last_level: self.first_level + len - 1,
                    last_view: self.levels.last().unwrap(),
                    above: above + 1,
                }
            } else {
                SummaryLevel::Level(&self.levels[i as usize])
            }
        } else {
            SummaryLevel::Base
        }
    }

    pub fn at_least_level(&self, level: u8) -> Summary<&'a [D]> {
        if let Some(min_i) = level.checked_sub(self.first_level) {
            if let Some(levels) = self.levels.get(min_i as usize ..) {
                Summary { first_level: level, levels }
            } else {
                Summary::empty()
            }
        } else {
            self.clone()
        }
    }

    pub fn limit_to_level(&self, level: u8) -> Summary<&'a [D]> {
        if let Some(max_i) = level.checked_sub(self.first_level) {
            Summary { first_level: self.first_level, levels: &self.levels[..(max_i as usize).min(self.levels.len())] }
        } else {
            Summary::empty()
        }
    }

    pub fn iter_levels(&self) -> impl DoubleEndedIterator<Item = (u8, &D)> {
        self.levels.iter().enumerate().map(move |(i, level)| (self.first_level + i as u8, level))
    }

    pub fn last(&self) -> Option<(u8, &D)> {
        self.levels.last().map(|level| (self.first_level + (self.levels.len() - 1) as u8, level))
    }

    pub fn map<O>(&self, f: impl Fn(&D) -> O) -> Summary<Box<[O]>> {
        Summary {
            first_level: self.first_level,
            levels: self.levels.iter().map(f).collect(),
        }
    }
}

impl<D> StoredSummary<D> {
    pub fn borrow(&self) -> BorrowedSummary<'_, D> {
        Summary { first_level: self.first_level, levels: &self.levels }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SummaryLevel<D> {
    Base,
    Level(D),
    Above { last_level: u8, last_view: D, above: u8 }
}

#[derive(Clone)]
pub struct LiveSummary {
    inner: Arc<Mutex<LiveSummaryInner>>,
}

pub struct LiveSummaryWriter {
    inner: Arc<Mutex<LiveSummaryInner>>,
}

pub struct LiveSummaryInner {
    levels: Vec<ArcStream>,
    first_level: u8,
    done: bool,
    wakers: WakerSetUnlocked,
}

impl LiveSummaryInner {
    fn new(first_level: u8, levels: Vec<ArcStream>, done: bool) -> Self {
        let wakers = Default::default();
        LiveSummaryInner { levels, first_level, done, wakers }
    }
}

impl LiveSummary {
    pub fn new(first_level: u8, levels: impl IntoIterator<Item = ArcStream>) -> Self {
        LiveSummary {
            inner: Arc::new(Mutex::new(LiveSummaryInner::new(first_level, levels.into_iter().collect::<Vec<_>>(), true))),
        }
    }

    pub fn map<R>(&self, waker: &Waker, f: impl FnMut(&ArcStream) -> R) -> StoredSummary<R> {
        let mut inner = self.inner.lock().unwrap();
        if !inner.done {
            inner.wakers.insert(waker);
        }
        StoredSummary {
            first_level: inner.first_level,
            levels: inner.levels.iter().map(f).collect(),
        }
    }

    pub fn async_iter(&self) -> impl futures_lite::Stream<Item = ArcStream> {
        let mut i = 0;
        futures_lite::stream::poll_fn(move |cx| {
            let mut inner = self.inner.lock().unwrap();
            if let Some(level) = inner.levels.get(i) {
                i += 1;
                Poll::Ready(Some(level.clone()))
            } else if inner.done {
                Poll::Ready(None)
            } else {
                inner.wakers.insert(cx.waker());
                Poll::Pending
            }

        })
    }

    pub fn levels(&self) -> usize {
        self.inner.lock().unwrap().levels.len()
    }

    pub fn first_level(&self) -> u8 {
        self.inner.lock().unwrap().first_level
    }
}

impl LiveSummaryWriter {
    pub fn new(first_level: u8) -> Self {
        LiveSummaryWriter {
            inner: Arc::new(Mutex::new(LiveSummaryInner::new(first_level, Vec::new(), false))),
        }
    }

    pub fn reader(&self) -> LiveSummary {
        LiveSummary { inner: self.inner.clone() }
    }

    fn modify(&self, f: impl FnOnce(&mut LiveSummaryInner)) {
        let mut inner = self.inner.lock().unwrap();
        f(&mut inner);
        let wakers = mem::take(&mut inner.wakers);
        drop(inner);
        wakers.wake_all();
    }

    pub fn insert(&self, stream: ArcStream) {
        self.modify(|inner| inner.levels.push(stream));
    }
}

impl Drop for LiveSummaryWriter {
    fn drop(&mut self) {
        self.modify(|inner| inner.done = true);
    }
}

#[test]
fn test_summary_ops() {
    let s = BorrowedSummary { first_level: 2, levels: &[20, 30, 40, 50] };
    assert_eq!(s.get(0), SummaryLevel::Base);
    assert_eq!(s.get(1), SummaryLevel::Base);
    assert_eq!(s.get(2), SummaryLevel::Level(&20));
    assert_eq!(s.get(5), SummaryLevel::Level(&50));
    assert_eq!(s.get(6), SummaryLevel::Above { last_level: 5, last_view: &50, above: 1 });

    assert_eq!(s.iter_levels().collect::<Vec<_>>(), vec![(2, &20), (3, &30), (4, &40), (5, &50)]);

    assert_eq!(s.at_least_level(1).iter_levels().collect::<Vec<_>>(), vec![(2, &20), (3, &30), (4, &40), (5, &50)]);
    assert_eq!(s.at_least_level(4).iter_levels().collect::<Vec<_>>(), vec![(4, &40), (5, &50)]);
    assert_eq!(s.at_least_level(6).iter_levels().collect::<Vec<_>>(), vec![]);

    assert_eq!(s.limit_to_level(2).iter_levels().collect::<Vec<_>>(), vec![]);
    assert_eq!(s.limit_to_level(3).iter_levels().collect::<Vec<_>>(), vec![(2, &20)]);
    assert_eq!(s.limit_to_level(6).iter_levels().collect::<Vec<_>>(), vec![(2, &20), (3, &30), (4, &40), (5, &50)]);
}
