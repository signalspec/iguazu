use jiff::Zoned;
use super::Time;

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct TickStep(Time);

impl TickStep {
    pub const SMALLEST: TickStep = TickStep(Time::PICOSECOND);

    pub fn time(self) -> Time {
        self.0
    }

    pub fn larger(self) -> TickStep {
        TickStep(match self.0 {
            spacing if spacing <= Time::SECOND      => 10 * spacing,
            spacing if spacing <= 10 * Time::SECOND => Time::MINUTE,
            spacing if spacing <= Time::MINUTE      => 10 * Time::MINUTE,
            spacing if spacing <= 10 * Time::MINUTE => Time::HOUR,
            spacing if spacing <= Time::HOUR        => Time::DAY,
            spacing => 10 * spacing,
        })
    }

    pub fn floor(self, t: Time) -> Time {
        t / self.0 * self.0
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum DateTickStep {
    Nanosecond(u64), // 1ns - 10s by factors of 10
    Day,
    Month,
    Year(i16),
}

impl DateTickStep {
    pub const SMALLEST: DateTickStep = DateTickStep::Nanosecond(1);

    pub fn larger(self) -> DateTickStep {
        match self {
            DateTickStep::Nanosecond(ns) if ns <= 1_000_000_000 => DateTickStep::Nanosecond(ns * 10),
            DateTickStep::Nanosecond(ns) if ns <= 10_000_000_000 => DateTickStep::Nanosecond(ns * 6),
            DateTickStep::Nanosecond(ns) if ns <= 60_000_000_000 => DateTickStep::Nanosecond(ns * 10),
            DateTickStep::Nanosecond(ns) if ns <= 600_000_000_000 => DateTickStep::Nanosecond(ns * 6),
            DateTickStep::Nanosecond(_) => DateTickStep::Day,
            DateTickStep::Day => DateTickStep::Month,
            DateTickStep::Month => DateTickStep::Year(1),
            DateTickStep::Year(years) => DateTickStep::Year(years*10),
        }
    }

    pub fn approx_time(self) -> Time {
        match self {
            DateTickStep::Nanosecond(ns) => (ns as i128) * Time::NANOSECOND,
            DateTickStep::Day => Time::DAY,
            DateTickStep::Month => 30 * Time::DAY,
            DateTickStep::Year(years) => years as i128 * (365 * Time::DAY + 6 * Time::HOUR),
        }
    }

    pub fn floor_date(self, date: &Zoned) -> Result<Zoned, jiff::Error> {
        use jiff::{ZonedRound, RoundMode, Unit};

        pub(crate) fn trunc(date: &Zoned, unit: Unit, increment: i64) -> Result<Zoned, jiff::Error> {
            date.round(ZonedRound::new().smallest(unit).increment(increment).mode(RoundMode::Trunc))
        }

        match self {
            DateTickStep::Nanosecond(ns) if ns < 1_000 => trunc(date, Unit::Nanosecond, ns as i64),
            DateTickStep::Nanosecond(ns) if ns < 1_000_000 => trunc(date, Unit::Microsecond, (ns / 1_000) as i64),
            DateTickStep::Nanosecond(ns) if ns < 1_000_000_000 => trunc(date, Unit::Millisecond, (ns / 1_000_000) as i64),
            DateTickStep::Nanosecond(ns) if ns < 60_000_000_000 => trunc(date, Unit::Second, (ns / 1_000_000_000) as i64),
            DateTickStep::Nanosecond(ns) if ns < 3_600_000_000_000 => trunc(date, Unit::Minute, (ns / 60_000_000_000) as i64),
            DateTickStep::Nanosecond(ns) => trunc(date, Unit::Hour, (ns / 3_600_000_000_000) as i64),
            DateTickStep::Day => date.start_of_day(),
            DateTickStep::Month => date.start_of_day()?.first_of_month(),
            DateTickStep::Year(n) => jiff::civil::Date::new((date.year() / n) * n, 1, 1)?.to_zoned(date.time_zone().clone()),
        }
    }

    pub fn as_span(self) -> jiff::Span {
        use jiff::Span;
        match self {
            DateTickStep::Nanosecond(ns) => Span::new().nanoseconds(ns as i64),
            DateTickStep::Day => Span::new().days(1),
            DateTickStep::Month => Span::new().months(1),
            DateTickStep::Year(n) => Span::new().years(n),
        }
    }

    pub fn format(self, time: &Zoned) -> String {
        match self {
            DateTickStep::Nanosecond(_) => time.time().to_string(),
            DateTickStep::Day | DateTickStep::Month | DateTickStep::Year(_) => time.date().to_string(),
        }
    }
}

#[test]
pub(crate) fn test_floor_date() {
    let date: Zoned = "2026-11-15T12:34:56.123456789[UTC]".parse().unwrap();
    let expected = [
        "2026-11-15T12:34:56.123456789[UTC]", // Nanosecond(1)
        "2026-11-15T12:34:56.12345678[UTC]", // Nanosecond(10)
        "2026-11-15T12:34:56.1234567[UTC]", // Nanosecond(100)
        "2026-11-15T12:34:56.123456[UTC]", // Nanosecond(1_000)
        "2026-11-15T12:34:56.12345[UTC]", // Nanosecond(10_000)
        "2026-11-15T12:34:56.1234[UTC]", // Nanosecond(100_000)
        "2026-11-15T12:34:56.123[UTC]", // Nanosecond(1_000_000)
        "2026-11-15T12:34:56.12[UTC]", // Nanosecond(10_000_000)
        "2026-11-15T12:34:56.1[UTC]", // Nanosecond(100_000_000)
        "2026-11-15T12:34:56[UTC]", // Nanosecond(1_000_000_000)
        "2026-11-15T12:34:50[UTC]", // Nanosecond(10_000_000_000)
        "2026-11-15T12:34:00[UTC]", // Minute
        "2026-11-15T12:30:00[UTC]", // TenMinute
        "2026-11-15T12:00:00[UTC]", // Hour
        "2026-11-15T00:00:00[UTC]", // Day
        "2026-11-01T00:00:00[UTC]", // Month
        "2026-01-01T00:00:00[UTC]", // Year(1)
        "2020-01-01T00:00:00[UTC]", // Year(10)
        "2000-01-01T00:00:00[UTC]", // Year(100)
    ];
    let mut step = DateTickStep::SMALLEST;
    for expected in expected {
        match step.floor_date(&date) {
            Ok(floored) => assert_eq!(floored, expected.parse().unwrap(), "step {step:?}"),
            Err(e) =>  panic!("{step:?}.floor_date() = {e:?}"),
        }
        step = step.larger();
    }
}
