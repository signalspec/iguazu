use std::ops::ControlFlow;

use crate::{Idx, schema::{Entity, EntityStream, Field, FieldKind}, stream::ArcStream, summary::{LiveSummaryMap, StoredSummary}};

use super::{IntView, ViewManager};

fn resolve_time_field(mut entity: &EntityStream) -> Option<&EntityStream> {
    while let Some(time_field) = entity.time_field() {
        entity = entity.child(&time_field)?;
    };
    Some(entity)
}

pub struct TimestampView<'v> {
    view: IntView<'v>,
    summary: StoredSummary<IntView<'v>>,
    time_rate: f64,
}

impl<'v> TimestampView<'v> {
    pub fn new(vm: &'v ViewManager, mut entity: &EntityStream) -> Option<Self> {
        entity = resolve_time_field(entity)?;

        let Entity::Data { field: field @ Field { kind: FieldKind::Timestamp, .. }, data, summaries } = entity else {
            return None;
        };

        let time_rate = field.time_rate()?;

        Self::new_from_stream(vm, time_rate, data, summaries)
    }

    pub fn new_from_stream(vm: &'v ViewManager, time_rate: f64, stream: &ArcStream, summaries: &LiveSummaryMap) -> Option<Self> {
        let view = IntView::new_from_stream(vm, stream);
        let summary = summaries.get("skip").map(|s| IntView::new_from_stream(vm, s));

        Some(TimestampView {
            view,
            summary,
            time_rate,
        })

    }

    pub fn time_rate(&self) -> f64 { self.time_rate }

    pub fn first_timestamp(&self) -> Option<u64> {
        if let Some((_, level_view)) = self.summary.borrow().last() {
            level_view.get_u64(0)
        } else {
            self.view.get_u64(0)
        }
    }

    pub fn latest_timestamp(&self) -> Option<u64> {
        self.view.get_u64(self.view.state().end)
    }

    pub fn get_base(&self, idx: Idx) -> Option<u64> {
        self.view.get_u64(idx)
    }

    /// Finds maximal `n` such that `i mod 2^n == 0` and `stream[i + 2^n] <= max_t`, or returns `Sparse` if no such `n` exists because `stream[i + 1] > max_t`
    pub fn skip(&self, i: Idx, max_t: u64) -> SkipResult {
        use ControlFlow::{Break, Continue};
        let summary = self.summary.borrow();

        if i + 1 >= self.view.len() {
            return SkipResult::End;
        }

        // Levels that include `i`
        let index_trailing_zeros = u8::try_from(i.trailing_zeros()).unwrap();
        // Levels where `i + 2^l` will be in bounds
        let remaining_log2 = u8::try_from((self.view.len() - i).ilog2()).unwrap();
        let max_level = index_trailing_zeros.min(remaining_log2);

        fn probe(i: Idx, max_t: u64, level: u8, level_view: &IntView<'_>, p: u64) -> ControlFlow<SkipResult, ()> {
            if p >= level_view.len() {
                // If this summary is not computed yet, fall through to the next level.
                return Continue(());
            }
            let Some(val) = level_view.get_u64(p) else {
                return Break(SkipResult::Loading(level));
            };
            if val <= max_t {
                return Break(SkipResult::Dense(level, i + (1 << level), val));
            }
            Continue(())
        }

        // If the max level is beyond the computed summaries, try skipping ahead in the coarsest summary.
        if let Some((coarse_level, coarse_view)) = summary.last() && max_level > coarse_level {
            for level in (coarse_level+1 ..= max_level).rev() {
                let p = (i >> coarse_level) + (1 << (level - coarse_level));
                if let Break(value) = probe(i, max_t, level, coarse_view, p) {
                    return value;
                }
            }
        }

        // Descend through each of the stored summaries, checking the next element in each.
        for (level, level_view) in summary.limit_to_level(max_level + 1).iter_levels().rev() {
            let p = (i >> level) + 1;
            if let Break(value) = probe(i, max_t, level, level_view, p) {
                return value;
            }
        }

        // For the levels below the base summary level, try skipping ahead in the level 0 stream.
        for level in (1..=max_level.min(summary.base_level)).rev() {
            let p = i + (1 << level);
            if let Break(value) = probe(i, max_t, level, &self.view, p) {
                return value;
            }
        }

        // Only at level 0 can we return Sparse.
        let p = i + 1;
        let Some(val) = self.view.get_u64(p) else {
            return SkipResult::Loading(0);
        };
        if val <= max_t {
            SkipResult::Dense(0, p, val)
        } else {
            SkipResult::Sparse(val)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipResult {
    /// This query required data that is not loaded.
    Loading(u8),

    Dense(u8, Idx, u64),

    Sparse(u64),

    End,
}

#[test]
fn test_timestamp_view() {
    use crate::storage::{MemoryStream, MemoryStorage};
    use crate::schema::FieldKind;
    use std::sync::Arc;
    use crate::storage::Storage;
    use async_executor::Executor;
    use futures_lite::future::block_on;

    let executor = Arc::new(Executor::new());
    let storage = Arc::new(MemoryStorage) as Arc<dyn Storage>;

    let vm = super::ViewManager::new();

    let mut data = Vec::new();
    // 1_000_000 to 2_000_000 by 1000
    for i in 0..1000 {
        data.push(1_000_000 + i * 1000);
    }
    // 3_000_000 to 4_000_000 by 100
    for i in 0..10_000 {
        data.push(3_000_000 + i * 100);
    }
    let data = MemoryStream::new(&data);

    let mut entity = EntityStream::field_data(
        FieldKind::Timestamp, data
    ).with_attribute(crate::schema::attribute::core::TIME_RATE, 1e6);

    fn check_step(view: &TimestampView<'_>) {
        // Large jump as far as possible based on alignment
        assert_eq!(view.skip(0, 5_000_000), SkipResult::Dense(13, 8192, 3_719_200));
        assert_eq!(view.skip(0, 4_000_000), SkipResult::Dense(13, 8192, 3_719_200));
        assert_eq!(view.skip(1, 4_000_000), SkipResult::Dense(0, 2, 1_000_000 + 1000 * 2));
        assert_eq!(view.skip(2, 4_000_000), SkipResult::Dense(1, 4, 1_000_000 + 1000 * 4));
        assert_eq!(view.skip(3, 4_000_000), SkipResult::Dense(0, 4, 1_000_000 + 1000 * 4));
        assert_eq!(view.skip(4, 4_000_000), SkipResult::Dense(2, 8, 1_000_000 + 1000 * 8));
        assert_eq!(view.skip(256, 4_000_000), SkipResult::Dense(8, 512, 1_000_000 + 1000 * 512));
        assert_eq!(view.skip(4096, 4_000_000), SkipResult::Dense(12, 8192, 3_719_200));

        // Large jumps limited by `max_t`.
        assert_eq!(view.skip(8192, 4_000_000), SkipResult::Dense(11, 10_240, 3_924_000));
        assert_eq!(view.skip(8192, 3_900_000), SkipResult::Dense(10, 9_216, 3_821_600));
        assert_eq!(view.skip(8192, 3_800_000), SkipResult::Dense(9, 8_704, 3_770_400));
        assert_eq!(view.skip(8192, 3_750_000), SkipResult::Dense(8, 8_448, 3_744_800));
        assert_eq!(view.skip(8192, 3_700_000), SkipResult::Sparse(3_719_300));

        let step = 20_000;

        // Iterate through first segment where time steps by 1000.
        // Step of 20000 ticks gets 16 samples (16000 ticks) at level 4
        assert_eq!(view.skip(0, 1_000_000 + step), SkipResult::Dense(4, 16, 1_016_000));
        assert_eq!(view.skip(16, 1_016_000 + step), SkipResult::Dense(4, 32, 1_032_000));
        assert_eq!(view.skip(256, 1_256_000 + step), SkipResult::Dense(4, 272, 1_272_000));
        assert_eq!(view.skip(512, 1_512_000 + step), SkipResult::Dense(4, 528, 1_528_000));

        // Approaching end of first segment, smaller steps
        assert_eq!(view.skip(976, 1_976_000 + step), SkipResult::Dense(4, 992, 1_992_000));
        assert_eq!(view.skip(992, 1_992_000 + step), SkipResult::Dense(2, 996, 1_996_000));
        assert_eq!(view.skip(996, 1_996_000 + step), SkipResult::Dense(1, 998, 1_998_000));
        assert_eq!(view.skip(998, 1_998_000 + step), SkipResult::Dense(0, 999, 1_999_000));

        // Sparse jump to the next segment
        assert_eq!(view.skip(999, 1_999_000 + step), SkipResult::Sparse(3_000_000));

        // Start of the second segment: larger steps as the the index realigns with powers of 2
        // up to level 7 (steps of 128 samples spaced at 100 = 12800 ticks)
        assert_eq!(view.skip(1_000, 3_000_000 + step), SkipResult::Dense(3, 1_008, 3_000_800));
        assert_eq!(view.skip(1_008, 3_000_800 + step), SkipResult::Dense(4, 1_024, 3_002_400));
        assert_eq!(view.skip(1_024, 3_002_400 + step), SkipResult::Dense(7, 1_152, 3_015_200));
        assert_eq!(view.skip(1_152, 3_015_200 + step), SkipResult::Dense(7, 1_280, 3_028_000));

        // Approaching the end and falling through the incomplete higher summary levels
        assert_eq!(view.skip(10_752, 3_975_200 + step), SkipResult::Dense(7, 10_880, 3_988_000));
        assert_eq!(view.skip(10_880, 3_988_000 + step), SkipResult::Dense(6, 10_944, 3_994_400));
        assert_eq!(view.skip(10_944, 3_994_400 + step), SkipResult::Dense(5, 10_976, 3_997_600));
        // Level 4 summary has len 10992 (floor((10999 / 16) * 16)), so with that summary available,
        // we fall through and use level 3 twice, but without summaries, it just probes 10992 directly.
        assert!(matches!(view.skip(10_976, 3_997_600 + step),
            | SkipResult::Dense(3, 10_984, 3_998_400)
            | SkipResult::Dense(4, 10_992, 3_999_200)
        ));
        assert_eq!(view.skip(10_984, 3_998_400 + step), SkipResult::Dense(3, 10_992, 3_999_200));
        assert_eq!(view.skip(10_992, 3_999_200 + step), SkipResult::Dense(2, 10_996, 3_999_600));
        assert_eq!(view.skip(10_996, 3_999_600 + step), SkipResult::Dense(1, 10_998, 3_999_800));
        assert_eq!(view.skip(10_998, 3_999_800 + step), SkipResult::Dense(0, 10_999, 3_999_900));
        assert_eq!(view.skip(10_999, 3_999_900 + step), SkipResult::End);
    }

    // Tests without summaries
    let view = vm.timestamp_view(&entity).unwrap();
    check_step(&view);

    // Build summaries and test again
    block_on(executor.run(entity.build_summaries(&executor, &storage))).unwrap();
    let view = vm.timestamp_view(&entity).unwrap();
    check_step(&view);
}
