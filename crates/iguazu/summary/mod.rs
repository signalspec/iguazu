//! Downsampled overviews of data for zoomed-out views
use std::{fmt::Debug, sync::Arc};

use ecow::EcoString;
use indexmap::IndexMap;
use once_array::{OnceArray, OnceArrayWriter};
use serde::{Deserialize, Serialize};
use crate::stream::ArcStream;

mod build;

pub trait SummaryMap: Default + FromIterator<(EcoString, StoredSummary<Self::Data>)> {
    type Data;
    fn iter(&self) -> impl Iterator<Item = (EcoString, BorrowedSummary<'_, Self::Data>)>;
    fn is_empty(&self) -> bool;
}

#[derive(Clone)]
pub struct LiveSummaryMap(IndexMap<EcoString, LiveSummary>);

#[derive(Clone, Serialize, Deserialize)]
pub struct StoredSummaryMap<D>(pub IndexMap<EcoString, StoredSummary<D>>);

impl LiveSummaryMap {
    pub fn get(&self, key: &str) -> BorrowedSummary<'_, ArcStream> {
        self.0.get(key).map_or(BorrowedSummary::empty(), |summary| summary.borrow())
    }
}

impl SummaryMap for LiveSummaryMap {
    type Data = ArcStream;

    fn iter(&self) -> impl Iterator<Item = (EcoString, BorrowedSummary<'_, Self::Data>)> {
        self.0.iter().map(|(key, summary)| (key.clone(), summary.borrow()))
    }

    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl FromIterator<(EcoString, StoredSummary<ArcStream>)> for LiveSummaryMap {
    fn from_iter<T: IntoIterator<Item = (EcoString, StoredSummary<ArcStream>)>>(iter: T) -> Self {
        LiveSummaryMap(iter.into_iter().map(|(key, summary)| {
            (key, LiveSummary { base_level: summary.base_level, levels: Arc::new(OnceArray::from(Vec::from(summary.levels))) })
        }).collect())
    }
}

impl Default for LiveSummaryMap {
    fn default() -> Self {
        LiveSummaryMap(IndexMap::new())
    }
}

impl<D> SummaryMap for StoredSummaryMap<D> {
    type Data = D;

    fn iter(&self) -> impl Iterator<Item = (EcoString, BorrowedSummary<'_, Self::Data>)> {
        self.0.iter().map(|(key, summary)| (key.clone(), summary.borrow()))
    }

    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<D> FromIterator<(EcoString, StoredSummary<D>)> for StoredSummaryMap<D> {
    fn from_iter<T: IntoIterator<Item = (EcoString, StoredSummary<D>)>>(iter: T) -> Self {
        StoredSummaryMap(iter.into_iter().collect())
    }
}

impl<D> Default for StoredSummaryMap<D> {
    fn default() -> Self {
        StoredSummaryMap(IndexMap::new())
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Summary<L> {
    pub base_level: u8,
    pub levels: L,
}

impl<L> Debug for Summary<L> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Summary").finish()
    }
}

impl<'a, D> Summary<&'a [D]> {
    pub const fn empty() -> Self {
        Summary { base_level: 255, levels: &[] }
    }

    pub fn max_level(&self) -> u8 {
        if self.levels.is_empty() {
            0
        } else {
            (self.base_level as usize + self.levels.len() - 1).try_into().unwrap_or(u8::MAX)
        }
    }

    pub fn get(&self, level: u8) -> Option<&'a D> {
        self.levels.get(level.checked_sub(self.base_level)? as usize)
    }

    pub fn at_least_level(&self, level: u8) -> Summary<&'a [D]> {
        if let Some(min_i) = level.checked_sub(self.base_level) {
            if let Some(levels) = self.levels.get(min_i as usize ..) {
                Summary { base_level: level, levels }
            } else {
                Summary::empty()
            }
        } else {
            self.clone()
        }
    }

    pub fn limit_to_level(&self, level: u8) -> Summary<&'a [D]> {
        if let Some(max_i) = level.checked_sub(self.base_level) {
            Summary { base_level: self.base_level, levels: &self.levels[..(max_i as usize).min(self.levels.len())] }
        } else {
            Summary::empty()
        }
    }

    pub fn iter_levels(&self) -> impl DoubleEndedIterator<Item = (u8, &D)> {
        self.levels.iter().enumerate().map(move |(i, level)| (self.base_level + i as u8, level))
    }

    pub fn last(&self) -> Option<(u8, &D)> {
        self.levels.last().map(|level| (self.base_level + (self.levels.len() - 1) as u8, level))
    }

    pub fn map<O>(&self, f: impl Fn(&D) -> O) -> Summary<Box<[O]>> {
        Summary {
            base_level: self.base_level,
            levels: self.levels.iter().map(f).collect(),
        }
    }
}

impl<D> Summary<Box<[D]>> {
    pub fn borrowed(&self) -> Summary<&[D]> {
        Summary { base_level: self.base_level, levels: &*self.levels }
    }

    pub fn max_level(&self) -> u8 {
        self.borrowed().max_level()
    }

    pub fn level(&self, level: u8) -> Option<&D> {
        self.levels.get(level.checked_sub(self.base_level)? as usize)
    }

    pub fn iter_levels(&self) -> impl DoubleEndedIterator<Item = (u8, &D)> {
        self.levels.iter().enumerate().map(move |(i, level)| (self.base_level + i as u8, level))
    }
}

pub type LiveSummary = Summary<Arc<OnceArray<ArcStream>>>;
pub type StoredSummary<D> = Summary<Box<[D]>>;
pub type BorrowedSummary<'a, D> = Summary<&'a [D]>;

impl LiveSummary {
    pub fn new(base_level: u8, levels: impl IntoIterator<Item = ArcStream>) -> Self {
        Summary { base_level, levels: Arc::new(levels.into_iter().collect::<Vec<_>>().into()) }
    }

    pub fn with_capacity(base_level: u8, capacity: usize) -> (Self, OnceArrayWriter<ArcStream>) {
        let writer = OnceArrayWriter::with_capacity(capacity);
        (Summary { base_level, levels: writer.reader().clone() }, writer)
    }

    pub fn borrow(&self) -> BorrowedSummary<'_, ArcStream> {
        Summary { base_level: self.base_level, levels: &self.levels }
    }
}

impl<D> StoredSummary<D> {
    pub fn borrow(&self) -> BorrowedSummary<'_, D> {
        Summary { base_level: self.base_level, levels: &self.levels }
    }
}


#[test]
fn test_summary_ops() {
    let s = BorrowedSummary { base_level: 2, levels: &[20, 30, 40, 50] };
    assert_eq!(s.get(0), None);
    assert_eq!(s.get(1), None);
    assert_eq!(s.get(2), Some(&20));
    assert_eq!(s.get(5), Some(&50));
    assert_eq!(s.get(6), None);

    assert_eq!(s.iter_levels().collect::<Vec<_>>(), vec![(2, &20), (3, &30), (4, &40), (5, &50)]);

    assert_eq!(s.at_least_level(1).iter_levels().collect::<Vec<_>>(), vec![(2, &20), (3, &30), (4, &40), (5, &50)]);
    assert_eq!(s.at_least_level(4).iter_levels().collect::<Vec<_>>(), vec![(4, &40), (5, &50)]);
    assert_eq!(s.at_least_level(6).iter_levels().collect::<Vec<_>>(), vec![]);

    assert_eq!(s.limit_to_level(2).iter_levels().collect::<Vec<_>>(), vec![]);
    assert_eq!(s.limit_to_level(3).iter_levels().collect::<Vec<_>>(), vec![(2, &20)]);
    assert_eq!(s.limit_to_level(6).iter_levels().collect::<Vec<_>>(), vec![(2, &20), (3, &30), (4, &40), (5, &50)]);
}
