use std::ops::RangeInclusive;

use egui::NumExt;

use crate::{IdxF, IdxRange, IdxRangeF};

/// Scale between screen space and time or index
#[derive(Debug)]
pub(crate) struct Scale {
    /// The range of indexes for view
    ///
    /// The visible range is slightly wider due to margins, see [`Self::visible`]
    pub view_range: IdxRangeF,

    /// The endpoints of the overall index space
    pub bounds: IdxRange,

    /// The on-screen x position in points
    pub x_range: RangeInclusive<f32>,

    /// Points per index
    pub x_scale: f32,

    /// Margin in points used to center the view on-screen
    pub x_margin_left: f32,

    /// Margin in points used to center the view on-screen
    pub x_margin_right: f32,
}

impl Scale {
    pub fn new(
        x_range: RangeInclusive<f32>,
        visible: IdxRangeF,
        bounds: IdxRange,
        x_margin_left: f32,
        x_margin_right: f32,
    ) -> Self {
        let x_width = *x_range.end() - *x_range.start();

        let x_scale = (x_width - x_margin_left - x_margin_right) / visible.length().as_f32();
        let x_scale = if x_scale > 0.0 && x_scale.is_finite() {
            x_scale
        } else {
            1.0
        };

        Self {
            x_range,
            view_range: visible,
            bounds,
            x_scale,
            x_margin_left,
            x_margin_right,
        }
    }

    /// Clamp the index to the valid range
    pub fn clamp_time(&self, time: IdxF) -> IdxF {
        time.clamp(
            IdxF::from(self.bounds.min),
            IdxF::from(self.bounds.max),
        )
    }

    pub fn visible(&self) -> IdxRangeF {
        let min = self.view_range.min - IdxF::from(self.x_margin_left / self.x_scale);
        let max = self.view_range.max + IdxF::from(self.x_margin_right / self.x_scale);
        IdxRangeF { min, max }
    }

    /// Visible range clamped to bounds
    pub fn clamped_visible(&self) -> IdxRange {
        let min = self.visible().min.floor().clamp(self.bounds.min, self.bounds.max);
        let max = self.visible().max.ceil().clamp(self.bounds.min, self.bounds.max);
        IdxRange { min, max }
    }

    pub fn x_from_idx(&self, idx: IdxF) -> f32 {
        self.x_range.start() + self.x_margin_left + self.x_scale * (idx - self.view_range.min).as_f32()
    }

    pub fn idx_from_x(&self, x: f32) -> IdxF {
        self.view_range.min + IdxF::from((x - self.x_range.start() - self.x_margin_left) / self.x_scale)
    }

    /// Pan the view, returning the new visible range.
    pub fn pan(&self, delta_x: f32) -> IdxRangeF {
        let delta_t = IdxF::from(delta_x / self.x_scale)
            .max(IdxF::from(self.bounds.min) - self.view_range.min)
            .min(IdxF::from(self.bounds.max) - self.view_range.max);
        IdxRangeF::new(self.view_range.min + delta_t, self.view_range.max + delta_t)
    }

    /// Zoom the view around the given x, returning the new visble range.
    pub fn zoom_at(&self, x: f32, zoom_factor: f32) -> IdxRangeF {
        let range_width = self.view_range.max - self.view_range.min;
        let zoom_factor = (zoom_factor as f64)
            .at_least(1.0/(range_width.as_f64()));
        let t = self.idx_from_x(x);
        let min = ((self.view_range.min - t) * zoom_factor + t)
            .max(IdxF::from(self.bounds.min));
        let max = ((self.view_range.max - t) * zoom_factor + t)
            .min(IdxF::from(self.bounds.max));
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
        },
        20.0,
        28.0,
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
