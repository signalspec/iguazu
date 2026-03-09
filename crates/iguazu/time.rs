//! High precision time types
use std::{fmt::Display, time::Duration};
use num_traits::Float;

/// A duration represented in 128-bit attoseconds
#[derive(Copy, Clone, PartialOrd, Ord, PartialEq, Eq, Hash, Debug)]
pub struct Time(i128);

impl Time {
    pub const ZERO: Time = Time(0);
    pub const UNIT: Time = Time(1);

    pub const ATTOSECOND:  Time =                     Time(1);
    pub const FEMTOSECOND: Time =                 Time(1_000);
    pub const PICOSECOND:  Time =             Time(1_000_000);
    pub const NANOSECOND:  Time =         Time(1_000_000_000);
    pub const MICROSECOND: Time =     Time(1_000_000_000_000);
    pub const MILLISECOND: Time = Time(1_000_000_000_000_000);
    pub const SECOND: Time =  Time(1_000_000_000_000_000_000);
    pub const MINUTE: Time = Time(60_000_000_000_000_000_000);
    pub const HOUR: Time = Time(3_600_000_000_000_000_000_000);
    pub const DAY: Time = Time(86_400_000_000_000_000_000_000);

    /// Get the period for a frequency expressed as a ratio
    pub fn period_ratio(num: u64, denom: u64) -> Time {
        (denom as i128) * Self::SECOND / (num as i128)
    }

    /// Get the period for a frequency expressed as a float
    pub fn period_float(freq: f64) -> Time {
        Time((Self::SECOND.0 as f64 / freq).round() as i128)
    }

    pub fn abs(self) -> Time {
        Time(self.0.abs())
    }

    pub fn div_rem(&self, t: Time) -> (i128, Time) {
        (*self / t, Time(self.0 % t.0))
    }

    pub fn format_relative(&self, precision: Time) -> FormatRelative {
        FormatRelative { time: *self, precision }
    }

    pub fn format_period_as_freq(&self) -> FormatFreq {
        FormatFreq { period: *self }
    }

    pub fn format_fixed(&self, precision: Time) -> FormatFixed {
        FormatFixed { time: *self, precision }
    }

    pub fn scale(&self, factor: f32) -> Time {
        let (mantissa, exponent, sign) = factor.integer_decode();

        if exponent <= -128 {
            Time(0)
        } else if exponent < 0 {
            Time(((self.0 * (mantissa as i128)) >> -exponent) * sign as i128)
        } else {
            Time(((self.0 * (mantissa as i128)) << exponent) * sign as i128)
        }
    }

    pub fn div_as_f32(l: Time, r: Time) -> f32 {
        let sign = (l.0.signum() as f32) / (r.0.signum() as f32);
        if !sign.is_normal() {
            return sign;
        }

        let mut l = l.0.abs();
        let mut r = r.0.abs();

        let l_bits = (i128::BITS - l.leading_zeros()) as i32;
        let r_bits = (i128::BITS - r.leading_zeros()) as i32;

        let shift = l_bits - r_bits - f32::MANTISSA_DIGITS as i32 + 1;
        if shift < 0 {
            l <<= -shift as usize;
        } else {
            r <<= shift as usize;
        }

        ((l / r) as u32 as f32) * (shift as f32).exp2() * sign
    }
}

#[test]
fn test_scale() {
    assert_eq!(Time::SECOND.scale(5.0), 5 * Time::SECOND);
    assert_eq!(Time::SECOND.scale(0.125), 125 * Time::MILLISECOND);
    assert_eq!(Time::NANOSECOND.scale(1e9), Time::SECOND);
    assert_eq!(Time::DAY.scale(-10.0), -10 * Time::DAY);
    assert_eq!(Time::DAY.scale(0.0), Time::ZERO);
    assert_eq!(Time::DAY.scale(1.157407407e-23f32), Time::UNIT);
    assert_eq!(Time::DAY.scale(2.465190329e-32f32), Time::ZERO); // exponent -128
    assert_eq!(Time::DAY.scale(f32::MIN_POSITIVE), Time::ZERO);
}

#[test]
fn test_div_f32() {
    assert_eq!(Time::div_as_f32(Time::SECOND, Time::SECOND), 1.0);
    assert_eq!(Time::div_as_f32(Time::DAY, Time::SECOND), 86400.0);
    assert_eq!(Time::div_as_f32(Time::SECOND, Time::DAY), 1.0 / 86400.0);

    assert_eq!(Time::div_as_f32(-1 * Time::MICROSECOND, Time::NANOSECOND), -1000.0);
    assert_eq!(Time::div_as_f32(Time::MICROSECOND, -1 * Time::NANOSECOND), -1000.0);
}

impl std::ops::Sub for Time {
    type Output = Time;

    #[inline]
    fn sub(self, rhs: Time) -> Time {
        Time(self.0.saturating_sub(rhs.0))
    }
}

impl std::ops::Add for Time {
    type Output = Time;

    #[inline]
    fn add(self, rhs: Time) -> Time {
        Time(self.0.saturating_add(rhs.0))
    }
}

impl std::ops::AddAssign for Time {
    #[inline]
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0;
    }
}

impl std::ops::Mul<Time> for i128 {
    type Output = Time;

    fn mul(self, rhs: Time) -> Self::Output {
        Time(self.saturating_mul(rhs.0))
    }
}

impl std::ops::Div<i128> for Time {
    type Output = Time;

    fn div(self, rhs: i128) -> Self::Output {
        Time(self.0.saturating_div(rhs))
    }
}

impl std::ops::Div for Time {
    type Output = i128;

    fn div(self, rhs: Time) -> Self::Output {
        self.0.saturating_div(rhs.0)
    }
}

impl std::ops::Rem for Time {
    type Output = Time;

    #[inline]
    fn rem(self, rhs: Time) -> Self::Output {
        Time(self.0 % rhs.0)
    }
}

impl From<Duration> for Time {
    fn from(duration: Duration) -> Time {
        duration.as_nanos() as i128 * Self::NANOSECOND
    }
}

impl TryFrom<Time> for Duration {
    type Error = std::num::TryFromIntError;

    fn try_from(time: Time) -> Result<Duration, Self::Error> {
        let (seconds, rem) = time.div_rem(Time::SECOND);
        let seconds = u64::try_from(seconds)?;
        let nanos = u32::try_from(rem / Time::NANOSECOND)?;
        Ok(Duration::new(seconds, nanos))
    }
}

/// Range of times
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct TimeRange {
    pub min: Time,
    pub max: Time,
}

impl TimeRange {
    pub const ZERO: TimeRange = TimeRange {
        min: Time::ZERO,
        max: Time::ZERO,
    };

    pub fn length(&self) -> Time {
        self.max - self.min
    }

    pub fn union(&self, other: &TimeRange) -> TimeRange {
        TimeRange {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
        }
    }
}

/// Formatter from [`Time::format_relative`].
pub struct FormatRelative {
    time: Time,
    precision: Time,
}

impl Display for FormatRelative {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.precision >= Time::HOUR {
            write!(f, "{} h", self.time / Time::HOUR)
        } else if self.precision >= Time::MINUTE {
            write!(f, "{} min", self.time / Time::MINUTE)
        } else if self.precision >= Time::SECOND {
            write!(f, "{} s", self.time / Time::SECOND)
        } else if self.precision >= Time::MILLISECOND {
            write!(f, "{} ms", self.time / Time::MILLISECOND)
        } else if self.precision >= Time::MICROSECOND {
            write!(f, "{} μs", self.time / Time::MICROSECOND)
        } else if self.precision >= Time::NANOSECOND {
            write!(f, "{} ns", self.time / Time::NANOSECOND)
        } else if self.precision >= Time::PICOSECOND {
            write!(f, "{} ps", self.time / Time::PICOSECOND)
        } else if self.precision >= Time::FEMTOSECOND {
            write!(f, "{} fs", self.time / Time::FEMTOSECOND)
        } else {
            write!(f, "{} as", self.time / Time::ATTOSECOND)
        }
    }
}

#[test]
fn test_format_relative() {
    assert_eq!(&format!("{}", (42 * Time::SECOND).format_relative(Time::SECOND)), "42 s");
    assert_eq!(&format!("{}", (42 * Time::MINUTE).format_relative(Time::MINUTE)), "42 min");
    assert_eq!(&format!("{}", (42 * Time::HOUR).format_relative(Time::HOUR)), "42 h");

    assert_eq!(&format!("{}", (42_123_456_789 * Time::NANOSECOND).format_relative(Time::SECOND)), "42 s");
    assert_eq!(&format!("{}", (42_123_456_789 * Time::NANOSECOND).format_relative(Time::MILLISECOND)), "42123 ms");
    assert_eq!(&format!("{}", (42_123_456_789 * Time::NANOSECOND).format_relative(Time::MICROSECOND)), "42123456 μs");
    assert_eq!(&format!("{}", (42_123_456_789 * Time::NANOSECOND).format_relative(Time::NANOSECOND)), "42123456789 ns");
    assert_eq!(&format!("{}", (42_123_456_789 * Time::NANOSECOND).format_relative(Time::PICOSECOND)), "42123456789000 ps");

    assert_eq!(&format!("{}", (-70 * Time::NANOSECOND).format_relative(Time::NANOSECOND)), "-70 ns");
}

/// Formatter from [`Time::format_fixed`].
pub struct FormatFixed {
    time: Time,
    precision: Time,
}

impl Display for FormatFixed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (day, time) = self.time.div_rem(Time::DAY);
        let (hour, time) = time.div_rem(Time::HOUR);
        let hour = hour as i64;
        let (minute, time) = time.div_rem(Time::MINUTE);
        let minute = minute as u32;
        let (second, time) = time.div_rem(Time::SECOND);
        let second = second as u32;
        let subsec_atto = time.0 as u64;

        if self.precision >= Time::DAY {
            write!(f, "{day}d")
        } else if self.precision >= Time::HOUR {
            if day > 0 {
                write!(f, "{day}d {hour}h")
            } else {
                write!(f, "{hour}h")
            }
        } else if self.precision >= Time::MINUTE {
            if day > 0 {
                write!(f, "{day}d {hour}:{minute:02}min")
            } else if hour > 0 {
                write!(f, "{hour}:{minute:02}min")
            } else {
                write!(f, "{minute:02}min")
            }
        } else {
            if day > 0 {
                write!(f, "{day}d {hour}:{minute:02}:{second:02}")?;
            } else if hour > 0 {
                write!(f, "{hour}:{minute:02}:{second:02}")?;
            } else if minute > 0 {
                write!(f, "{minute}:{second:02}")?;
            } else {
                write!(f, "{second}")?;
            }

            if self.precision >= Time::SECOND {
                write!(f, "s")
            } else {
                let digits = 18u32.saturating_sub((self.precision.0 as u64).ilog10());
                let subsec = subsec_atto / (10u64.pow(18 - digits));
                write!(f, ".{subsec:0digits$}s", digits = digits as usize)
            }
        }
    }
}

#[test]
fn test_format_fixed() {
    assert_eq!(&format!("{}", (2 * Time::DAY + 1 * Time::SECOND).format_fixed(Time::DAY)), "2d");
    assert_eq!(&format!("{}", (2 * Time::DAY + 1 * Time::SECOND).format_fixed(Time::HOUR)), "2d 0h");
    assert_eq!(&format!("{}", (2 * Time::DAY + 1 * Time::SECOND).format_fixed(Time::MINUTE)), "2d 0:00min");
    assert_eq!(&format!("{}", (2 * Time::DAY + 1 * Time::SECOND).format_fixed(Time::SECOND)), "2d 0:00:01s");

    assert_eq!(&format!("{}", (45296780 * Time::MILLISECOND).format_fixed(Time::DAY)), "0d");
    assert_eq!(&format!("{}", (45296780 * Time::MILLISECOND).format_fixed(Time::HOUR)), "12h");
    assert_eq!(&format!("{}", (45296780 * Time::MILLISECOND).format_fixed(Time::MINUTE)), "12:34min");
    assert_eq!(&format!("{}", (45296780 * Time::MILLISECOND).format_fixed(Time::SECOND)), "12:34:56s");
    assert_eq!(&format!("{}", (45296780 * Time::MILLISECOND).format_fixed(10 * Time::MILLISECOND)), "12:34:56.78s");

    assert_eq!(&format!("{}", (62_123_456_789 * Time::NANOSECOND).format_fixed(Time::MILLISECOND)), "1:02.123s");

    assert_eq!(&format!("{}", (42_123_456_789 * Time::NANOSECOND).format_fixed(Time::SECOND)), "42s");
    assert_eq!(&format!("{}", (42_123_456_789 * Time::NANOSECOND).format_fixed(Time::MILLISECOND)), "42.123s");
    assert_eq!(&format!("{}", (42_123_456_789 * Time::NANOSECOND).format_fixed(Time::MICROSECOND)), "42.123456s");
    assert_eq!(&format!("{}", (42_123_456_789 * Time::NANOSECOND).format_fixed(Time::NANOSECOND)), "42.123456789s");
    assert_eq!(&format!("{}", (42_123_456_789 * Time::NANOSECOND).format_fixed(Time::PICOSECOND)), "42.123456789000s");
}

/// Formatter from [`Time::format_period_as_freq`].
pub struct FormatFreq {
    period: Time,
}

impl Display for FormatFreq {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        fn fp(f: &mut std::fmt::Formatter<'_>, time_unit: Time, period: Time, unit: &str) -> std::fmt::Result {
            let val = u32::try_from(1000 * time_unit / period).unwrap();
            if val % 1000 == 0 {
                write!(f, "{} {}", val / 1000, unit)
            } else {
                write!(f, "{}.{} {}", val / 1000, val % 1000, unit)
            }
        }

        if self.period >= Time::HOUR {
            write!(f, "1 / {} h", self.period / Time::HOUR)
        } else if self.period >= Time::MINUTE {
            write!(f, "1 / {} min", self.period / Time::MINUTE)
        } else if self.period > Time::SECOND {
            write!(f, "1 / {} s", self.period / Time::SECOND)
        } else if self.period > Time::MILLISECOND {
            fp(f, Time::SECOND, self.period, "Hz")
        } else if self.period > Time::MICROSECOND {
            fp(f, Time::MILLISECOND, self.period, "kHz")
        } else if self.period > Time::NANOSECOND {
            fp(f, Time::MICROSECOND, self.period, "MHz")
        } else if self.period > Time::PICOSECOND {
            fp(f, Time::NANOSECOND, self.period, "GHz")
        } else if self.period > Time::FEMTOSECOND {
            fp(f, Time::PICOSECOND, self.period, "THz")
        } else {
            fp(f, Time::FEMTOSECOND, self.period, "PHz")
        }
    }
}

#[test]
fn test_format_freq() {
    assert_eq!(&format!("{}", (10 * Time::SECOND).format_period_as_freq()), "1 / 10 s");
    assert_eq!(&format!("{}", (Time::SECOND).format_period_as_freq()), "1 Hz");
    assert_eq!(&format!("{}", (500 * Time::MILLISECOND).format_period_as_freq()), "2 Hz");
    assert_eq!(&format!("{}", (Time::MILLISECOND).format_period_as_freq()), "1 kHz");
    assert_eq!(&format!("{}", (125 * Time::MICROSECOND).format_period_as_freq()), "8 kHz");
    assert_eq!(&format!("{}", (Time::MICROSECOND).format_period_as_freq()), "1 MHz");
    assert_eq!(&format!("{}", (Time::NANOSECOND).format_period_as_freq()), "1 GHz");
    assert_eq!(&format!("{}", (408 * Time::PICOSECOND).format_period_as_freq()), "2.450 GHz");
}

// Based on rerun.io: © 2023 Rerun Technologies AB under MIT OR Apache-2.0
/// Generate grid tick spacing for a time axis.
///
/// Given some spacing (e.g. 10s), return the next spacing (60s).
pub fn next_time_step(spacing: Time) -> Time {
    if spacing <= Time::SECOND {
        10 * spacing // up to 10 second ticks
    } else if spacing == 10 * Time::SECOND {
        6 * spacing // to the whole minute
    } else if spacing == Time::MINUTE {
        10 * spacing // to ten minutes
    } else if spacing == 10 * Time::MINUTE {
        6 * spacing // to an hour
    } else if spacing == Time::HOUR {
        12 * spacing // to 12 h
    } else if spacing == 12 * Time::HOUR {
        2 * spacing // to a day
    } else {
        10 * spacing
    }
}
