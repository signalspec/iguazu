use egui::{NumExt, Rangef};
use iguazu::{ Idx, IdxRange };
use crate::time::{TimeRange, Time};

#[cfg(test)]
use crate::util::assert_approx_eq;

/// Scale between screen space and time
#[derive(Debug)]
pub(crate) struct Scale {
    /// The range of time for view
    ///
    /// The visible range is slightly wider due to margins, see [`Self::visible`]
    pub view_range: TimeRange,

    /// The maximum scroll range
    pub bounds: TimeRange,

    /// The on-screen x position in points (outside of margin)
    pub x_range: Rangef,

    /// Margin in points used to center the view on-screen
    pub x_margin_left: f32,

    /// Margin in points used to center the view on-screen
    pub x_margin_right: f32,
}

const MIN_TIME: Time = Time::NANOSECOND;

impl Scale {
    pub fn new(
        x_range: Rangef,
        view_range: TimeRange,
        bounds: TimeRange,
        x_margin_left: f32,
        x_margin_right: f32,
    ) -> Self {
        Self {
            x_range,
            view_range,
            bounds,
            x_margin_left,
            x_margin_right,
        }
    }

    /// Width in points corresponding to the view_range
    pub fn x_width(&self) -> f32 {
        self.x_range.span() - self.x_margin_left - self.x_margin_right
    }

    /// The time offset represented by an on-screen displacement in points
    pub fn points_to_time(&self, points: f32) -> Time {
        self.view_range.length().scale(points / self.x_width())
    }

    /// The pixel displacement in points corresponding to a time offset
    pub fn points_from_time(&self, time: Time) -> f32 {
        Time::div_as_f32(time, self.view_range.length()) * self.x_width()
    }

    /// Clamp the index to the valid range
    pub fn clamp_time(&self, time: Time) -> Time {
        time.clamp(self.bounds.min, self.bounds.max)
    }

    /// Viewport including margins
    pub fn visible(&self) -> TimeRange {
        let min = self.view_range.min - self.points_to_time(self.x_margin_left);
        let max = self.view_range.max + self.points_to_time(self.x_margin_right);
        TimeRange { min, max }
    }

    /// Visible range clamped to bounds
    pub fn clamped_visible(&self) -> TimeRange {
        let min = self.visible().min.clamp(self.bounds.min, self.bounds.max);
        let max = self.visible().max.clamp(self.bounds.min, self.bounds.max);
        TimeRange { min, max }
    }

    /// Get an `IdxScale` mapping screen positions to ticks
    /// sampled at `frequency`.
    pub fn idx_scale(&self, frequency: f64) -> IdxScale {
        let period = Time::period_float(frequency);
        let t_range = self.clamped_visible();
        let visible = IdxRange {
            min: (t_range.min / period) as u64,
            max: ((t_range.max + period - Time::UNIT) / period) as u64,
        };

        let ref_idx = ((t_range.min + period / 2) / period) as u64;
        let x_offset = self.x_from_t(ref_idx as i128 * period);
        let x_scale = self.points_from_time(period);

        IdxScale { visible, ref_idx, x_offset, x_scale, period }
    }

    /// Map a timestamp to a screen position
    pub fn x_from_t(&self, idx: Time) -> f32 {
        self.x_range.min + self.x_margin_left + self.points_from_time(idx - self.view_range.min)
    }

    /// Map a screen position to a timestamp
    pub fn t_from_x(&self, x: f32) -> Time {
        self.view_range.min + self.points_to_time(x - self.x_range.min - self.x_margin_left)
    }

    /// Pan the view, returning the new visible range.
    pub fn pan(&self, delta_x: f32) -> TimeRange {
        let delta_t = self.points_to_time(delta_x)
            .max(self.bounds.min - self.view_range.min)
            .min(self.bounds.max - self.view_range.max);
    
        TimeRange {
            min: self.view_range.min + delta_t,
            max: self.view_range.max + delta_t
        }
    }

    /// Zoom the view around the given x, returning the new view range.
    pub fn zoom_at(&self, x: f32, zoom_factor: f32) -> TimeRange {
        let zoom_factor = zoom_factor
            .at_least(1.0/((self.view_range.length() / MIN_TIME) as f32));
        let t = self.t_from_x(x);
        let min = ((self.view_range.min - t).scale(zoom_factor) + t)
            .max(self.bounds.min);
        let max = ((self.view_range.max - t).scale(zoom_factor) + t)
            .min(self.bounds.max);
        TimeRange { min, max }
    }
}

pub struct IdxScale {
    /// Sample rate
    period: Time,

    /// The range of indexes that is at least partially visible
    pub visible: IdxRange,

    /// An index near `visible.min` used as a reference point.
    /// It may differ from `visible.min` to keep `x_offset` small for
    /// floating point stability at extreme zoom.
    pub ref_idx: Idx,

    /// The screen position in points of `ref_idx`
    pub x_offset: f32,

    /// Points per index
    pub x_scale: f32,
}

impl IdxScale {
    /// Map an index to a screen position
    pub fn x_from_idx(&self, idx: Idx) -> f32 {
        self.x_offset + ((idx.wrapping_sub(self.ref_idx) as i64 as f32) * self.x_scale)
    }

    pub fn points_per_index(&self) -> f32 {
        self.x_scale
    }

    pub fn t_from_idx(&self, idx: Idx) -> Time {
        (idx as i128) * self.period
    }
    
    pub(crate) fn sample_period(&self) -> Time {
        self.period
    }
    
    pub(crate) fn min_visible_width(&self) -> u64 {
        (2.0 / self.x_scale).ceil() as u64
    }
}

#[test]
fn test_scale() {
    let x1 = 80.0;
    let x2 = 530.0;
    let m1 = 20.0;
    let m2 = 30.0;
    let scale = Scale::new(
        Rangef::new(x1, x2),
        TimeRange {
            min: 10 * Time::SECOND,
            max: 30 * Time::SECOND,
        },
        TimeRange {
            min: 0 * Time::SECOND,
            max: 60 * Time::SECOND,
        },
        m1,
        m2,
    );

    assert_eq!(scale.t_from_x(x1 + m1), 10 * Time::SECOND);
    assert_eq!(scale.t_from_x(x2 - m2), 30 * Time::SECOND);
    assert_eq!(scale.t_from_x((x1 + m1 + x2 - m2) / 2.0), 20 * Time::SECOND);
    assert_eq!(scale.t_from_x(x1 + m1 - (x2-x1-m1-m2) / 2.0), 0 * Time::SECOND);

    assert_eq!(scale.x_from_t(10 * Time::SECOND), x1 + m1);
    assert_eq!(scale.x_from_t(30 * Time::SECOND), x2 - m2 );
    assert_eq!(scale.x_from_t(20 * Time::SECOND), (x1 + m1 + x2 - m2) / 2.0);
    assert_eq!(scale.x_from_t(Time::ZERO), x1 + m1 - (x2-x1-m1-m2) / 2.0);
    
    for x_in in 0..=600 {
        let x_in = x_in as f32;
        let t = scale.t_from_x(x_in);
        let x_out = scale.x_from_t(t);
        assert_approx_eq!(x_in, x_out, 1/1000.0);
    }

    for t_in in 0i128..=60 {
        let t_in = t_in * Time::SECOND;
        let x = scale.x_from_t(t_in);
        let t_out = scale.t_from_x(x);

        assert_approx_eq!(t_in, t_out, 1/1000);
    }
}

#[test]
fn test_idx_scale() {
    let x1 = 80.0;
    let x2 = 1130.0;
    let m1 = 20.0;
    let m2 = 30.0;
    let scale = Scale::new(
        Rangef::new(x1, x2),
        TimeRange {
            min: 10 * Time::SECOND,
            max: 20 * Time::SECOND,
        },
        TimeRange {
            min: 0 * Time::SECOND,
            max: 60 * Time::SECOND,
        },
        m1,
        m2,
    );

    let t_visible = scale.visible();
    assert_approx_eq!(t_visible.min, 9800 * Time::MILLISECOND, 1/10_000_000);
    assert_approx_eq!(t_visible.max, 20300 * Time::MILLISECOND, 1/10_000_000);

    let idx_scale = scale.idx_scale(1000.0);
    assert_eq!(idx_scale.visible, IdxRange { min: 9800, max: 20300 });

    assert_approx_eq!(idx_scale.x_from_idx(9800), x1);
    assert_approx_eq!(idx_scale.x_from_idx(10000), 100.0);
    assert_approx_eq!(idx_scale.x_from_idx(20000), 1100.0);
    assert_approx_eq!(idx_scale.x_from_idx(20300), x2);
}

#[test]
fn test_pan_zoom() {
    let scale = Scale::new(
        Rangef::new(1000.0, 2000.0),
        TimeRange {
            min: 2 * Time::SECOND,
            max: 102 * Time::SECOND,
        },
        TimeRange {
            min: 0 * Time::SECOND,
            max: 200 * Time::SECOND,
        },
        0.0,
        0.0,
    );

    let pan_left = scale.pan(-10.0);
    assert_approx_eq!(pan_left.min, 1 * Time::SECOND, 1/1_000_000);
    assert_approx_eq!(pan_left.max, 101 * Time::SECOND, 1/1_000_000);

    let pan_right = scale.pan(20.0);
    assert_approx_eq!(pan_right.min, 4 * Time::SECOND, 1/1_000_000);
    assert_approx_eq!(pan_right.max, 104 * Time::SECOND, 1/1_000_000);

    let pan_left_limit = scale.pan(-30.0);
    assert_approx_eq!(pan_left_limit.min, 0 * Time::SECOND, 1/1_000_000);
    assert_approx_eq!(pan_left_limit.max, 100 * Time::SECOND, 1/1_000_000);

    let zoom_in_center = scale.zoom_at(1500., 0.9);
    assert_approx_eq!(zoom_in_center.min, 7 * Time::SECOND, 1/1_000_000);
    assert_approx_eq!(zoom_in_center.max, 97 * Time::SECOND, 1/1_000_000);

    let zoom_in_left = scale.zoom_at(1250., 0.99);
    assert_approx_eq!(zoom_in_left.min, 2250 * Time::MILLISECOND, 1/1_000_000);
    assert_approx_eq!(zoom_in_left.max, 101250 * Time::MILLISECOND, 1/1_000_000);

    let zoom_out_limit_left = scale.zoom_at(1500., 1.1);
    assert_approx_eq!(zoom_out_limit_left.min, 0 * Time::SECOND, 1/1_000_000);
    assert_approx_eq!(zoom_out_limit_left.max, 107 * Time::SECOND, 1/1_000_000);
}
