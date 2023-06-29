use std::ops::RangeInclusive;

use crate::{IdxF, IdxRange, IdxRangeF};

/// Scale between screen space and time or index
#[derive(Debug)]
pub struct Scale {
    /// The visible range of indexes
    pub visible: IdxRangeF,

    /// The endpoints of the overall index space
    pub bounds: IdxRange,

    /// The on-screen x position in points
    pub x_range: RangeInclusive<f32>,

    /// Points per index
    pub x_scale: f32,
}

impl Scale {
    pub fn new(
        x_range: RangeInclusive<f32>,
        visible: IdxRangeF,
        bounds: IdxRange,
    ) -> Self {
        let x_width = *x_range.end() - *x_range.start();
        let x_scale = x_width / visible.length().as_f32();
        let x_scale = if x_scale > 0.0 && x_scale.is_finite() {
            x_scale
        } else {
            1.0
        };

        Self {
            x_range,
            visible,
            bounds,
            x_scale,
        }
    }

    /// Clamp the index to the valid range
    pub fn clamp_time(&self, time: IdxF) -> IdxF {
        time.clamp(
            IdxF::from(self.bounds.min),
            IdxF::from(self.bounds.max),
        )
    }

    /// Visible range clamped to bounds
    pub fn clamped_visible(&self) -> IdxRange {
        let min = self.visible.min.floor().clamp(self.bounds.min, self.bounds.max);
        let max = self.visible.max.ceil().clamp(self.bounds.min, self.bounds.max);
        IdxRange { min, max }
    }

    pub fn x_from_idx(&self, idx: IdxF) -> f32 {
        self.x_range.start() + self.x_scale * (idx - self.visible.min).as_f32()
    }

    pub fn idx_from_x(&self, x: f32) -> IdxF {
        self.visible.min + IdxF::from((x - self.x_range.start()) / self.x_scale)
    }

    /// Pan the view, returning the new visible range.
    pub fn pan(&self, delta_x: f32) -> IdxRangeF {
        let delta_t = IdxF::from(delta_x / self.x_scale);
        IdxRangeF::new(self.visible.min + delta_t, self.visible.max + delta_t)
    }

    /// Zoom the view around the given x, returning the new visble range.
    pub fn zoom_at(&self, x: f32, zoom_factor: f32) -> IdxRangeF {
        let zoom_factor = zoom_factor as f64;
        let t = self.idx_from_x(x);
        let min = (self.visible.min - t) * zoom_factor + t;
        let max = (self.visible.max - t) * zoom_factor + t;
        IdxRangeF { min, max }
    }
}

#[test]
fn test_scale() {
    let scale = Scale::new(
        100.0..=500.0,
        IdxRangeF {
            min: IdxF::from(0),
            max: IdxF::from(50),
        },
        IdxRange {
            min: 10,
            max: 20,
        }
    );

    let pixel_precision = 0.5;

    for x_in in 0..=500 {
        let x_in = x_in as f32;
        let idx = scale.idx_from_x(x_in);
        let x_out = scale.x_from_idx(idx);

        assert!(
            (x_in - x_out).abs() < pixel_precision,
            "x_in: {x_in}, x_out: {x_out}, idx: {idx:?}, scale: {scale:#?}"
        );
    }

    for idx_in in 0..=50 {
        let idx_in = IdxF::from(idx_in as f64);
        let x = scale.x_from_idx(idx_in);
        let idx_out = scale.idx_from_x(x);

        assert!(
            (idx_in - idx_out).abs().as_f64() < 0.1,
            "idx_in: {idx_in:?}, idx_out: {idx_out:?}, x: {x}, scale: {scale:#?}"
        );
    }
}
