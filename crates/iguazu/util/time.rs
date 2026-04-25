use jiff::{Error, Zoned, civil::DateTime, fmt::temporal::Pieces, tz::{OffsetConflict, TimeZone}};

/// Preserve the time zone if specified, but don't require it.
pub fn parse_zoned_with_zone_or_offset(value: &str) -> Result<Zoned, Error> {
    pieces_to_zoned(Pieces::parse(value)?)
}

pub fn pieces_to_zoned(pieces: Pieces) -> Result<Zoned, Error> {
    // https://docs.rs/jiff/latest/jiff/fmt/temporal/struct.Pieces.html#case-study-how-to-parse-2025-01-03t1728-05-into-zoned
    let dt = DateTime::from_parts(pieces.date(), pieces.time().unwrap_or_default());

    let conflict_resolution = OffsetConflict::Reject;

    let ambiguous_zdt = match pieces.to_time_zone()? {
        Some(tz) => match pieces.to_numeric_offset() {
            None => tz.into_ambiguous_zoned(dt),
            Some(offset) => conflict_resolution.resolve(dt, offset, tz)?,
        },
        None => {
            let Some(offset) = pieces.to_numeric_offset() else {
                return Err(Error::from_args(format_args!("timestamp has no time zone or offset")));
            };
            // Won't even be ambiguous, but gets us the same
            // type as the branch above.
            TimeZone::fixed(offset).into_ambiguous_zoned(dt)
        }
    };

    ambiguous_zdt.compatible()
}
