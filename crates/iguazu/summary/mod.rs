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

pub type LiveSummary = Summary<Arc<OnceArray<ArcStream>>>;
pub type StoredSummary<D> = Summary<Box<[D]>>;
pub type BorrowedSummary<'a, D> = Summary<&'a [D]>;

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

impl<D> Summary<&[D]> {
    pub const fn empty() -> Self {
        Summary { base_level: 255, levels: &[] }
    }
}
