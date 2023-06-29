use fixed::{traits::{LossyInto as _,}, FixedI128};

use crate::{Idx, IdxRange};

/// Fractional index numbers
///
/// This is like [`Idx`] with added precision to be able to represent
/// time between sequences and negative numbers.
/// This is needed in the time panel to refer to time between sequence numbers,
/// e.g. for panning.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[derive(serde::Deserialize, serde::Serialize)]
pub struct IdxF(FixedI128<typenum::U63>);

impl IdxF {
    #[inline]
    pub fn floor(&self) -> Idx {
        let int: u64 = self.0.saturating_floor().saturating_to_num();
        Idx::from(int)
    }

    #[inline]
    pub fn round(&self) -> Idx {
        let int: u64 = self.0.saturating_round().saturating_to_num();
        Idx::from(int)
    }

    #[inline]
    pub fn ceil(&self) -> Idx {
        let int: u64 = self.0.saturating_ceil().saturating_to_num();
        Idx::from(int)
    }

    #[inline]
    pub fn as_f32(&self) -> f32 {
        self.0.lossy_into()
    }

    #[inline]
    pub fn as_f64(self) -> f64 {
        self.0.lossy_into()
    }

    #[inline]
    pub fn abs(self) -> Self {
        Self(self.0.saturating_abs())
    }
}

impl From<u64> for IdxF {
    #[inline]
    fn from(integer: u64) -> Self {
        Self(integer.into())
    }
}

impl From<i64> for IdxF {
    #[inline]
    fn from(integer: i64) -> Self {
        Self(integer.into())
    }
}

impl From<i32> for IdxF {
    #[inline]
    fn from(integer: i32) -> Self {
        Self(integer.into())
    }
}


impl From<f32> for IdxF {
    #[inline]
    fn from(value: f32) -> Self {
        Self(FixedI128::from_num(value))
    }
}

impl From<f64> for IdxF {
    #[inline]
    fn from(value: f64) -> Self {
        Self(FixedI128::from_num(value))
    }
}

impl std::ops::Neg for IdxF {
    type Output = Self;

    #[inline]
    fn neg(self) -> Self::Output {
        Self(self.0.saturating_neg())
    }
}

impl std::ops::Add for IdxF {
    type Output = Self;

    #[inline]
    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0.saturating_add(rhs.0))
    }
}

impl std::ops::Sub for IdxF {
    type Output = Self;

    #[inline]
    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0.saturating_sub(rhs.0))
    }
}

impl std::ops::Mul<f64> for IdxF {
    type Output = Self;

    #[inline]
    fn mul(self, rhs: f64) -> Self::Output {
        Self(self.0.saturating_mul(FixedI128::from_num(rhs)))
    }
}

impl std::ops::AddAssign for IdxF {
    #[inline]
    fn add_assign(&mut self, rhs: Self) {
        self.0 = self.0.saturating_add(rhs.0);
    }
}

impl std::ops::SubAssign for IdxF {
    #[inline]
    fn sub_assign(&mut self, rhs: Self) {
        self.0 = self.0.saturating_sub(rhs.0);
    }
}

impl std::iter::Sum for IdxF {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        let mut sum = IdxF::from(0i64);
        for item in iter {
            sum += item;
        }
        sum
    }
}

impl std::ops::Add<Idx> for IdxF {
    type Output = IdxF;

    #[inline]
    fn add(self, rhs: Idx) -> Self::Output {
        self + IdxF::from(rhs)
    }
}

impl std::ops::Sub<Idx> for IdxF {
    type Output = IdxF;

    #[inline]
    fn sub(self, rhs: Idx) -> Self::Output {
        self - IdxF::from(rhs)
    }
}

impl std::ops::Add<IdxF> for Idx {
    type Output = IdxF;

    #[inline]
    fn add(self, rhs: IdxF) -> Self::Output {
        IdxF::from(self) + rhs
    }
}

impl std::ops::Sub<IdxF> for Idx {
    type Output = IdxF;

    #[inline]
    fn sub(self, rhs: IdxF) -> Self::Output {
        IdxF::from(self) - rhs
    }
}

impl PartialEq<Idx> for IdxF {
    #[inline]
    fn eq(&self, other: &Idx) -> bool {
        self.0 == *other
    }
}

impl PartialEq<IdxF> for Idx {
    #[inline]
    fn eq(&self, other: &IdxF) -> bool {
        *self == other.0
    }
}

impl PartialOrd<Idx> for IdxF {
    #[inline]
    fn partial_cmp(&self, other: &Idx) -> Option<std::cmp::Ordering> {
        self.0.partial_cmp(other)
    }
}

impl PartialOrd<IdxF> for Idx {
    #[inline]
    fn partial_cmp(&self, other: &IdxF) -> Option<std::cmp::Ordering> {
        self.partial_cmp(&other.0)
    }
}

// ---------------

#[test]
fn test_time_value_f() {
    type T = IdxF;

    let nice_floats = [-1.75, -0.25, 0.0, 0.25, 1.0, 1.75];
    for &f in &nice_floats {
        assert_eq!(T::from(f).as_f64(), f);
        assert_eq!(-T::from(f), T::from(-f));
        assert_eq!(T::from(f).abs(), T::from(f.abs()));

        for &g in &nice_floats {
            assert_eq!(T::from(f) + T::from(g), T::from(f + g));
            assert_eq!(T::from(f) - T::from(g), T::from(f - g));
        }
    }
}


/// Like [`IdxRange`], but using [`IdxF`] for fractional precision
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct IdxRangeF {
    pub min: IdxF,
    pub max: IdxF,
}

impl IdxRangeF {
    #[inline]
    pub fn new(min: impl Into<IdxF>, max: impl Into<IdxF>) -> Self {
        Self {
            min: min.into(),
            max: max.into(),
        }
    }

    #[inline]
    pub fn point(value: impl Into<IdxF>) -> Self {
        let value = value.into();
        Self {
            min: value,
            max: value,
        }
    }

    // pub fn add(&mut self, value: IdxF) {
    //     self.min = self.min.min(value);
    //     self.max = self.max.max(value);
    // }

    /// Inclusive
    pub fn contains(&self, value: IdxF) -> bool {
        self.min <= value && value <= self.max
    }

    /// Where in the range is this value? Returns 0-1 if within the range.
    ///
    /// Returns <0 if before and >1 if after.
    pub fn inverse_lerp(&self, value: IdxF) -> f64 {
        if self.min == self.max {
            0.5
        } else {
            (value - self.min).as_f64() / (self.max - self.min).as_f64()
        }
    }

    pub fn lerp(&self, t: f64) -> IdxF {
        self.min + (self.max - self.min) * t
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.min == self.max
    }

    /// The amount of time or sequences covered by this range.
    #[inline]
    pub fn length(&self) -> IdxF {
        self.max - self.min
    }
}

impl From<IdxRange> for IdxRangeF {
    fn from(range: IdxRange) -> Self {
        Self::new(range.min, range.max)
    }
}
