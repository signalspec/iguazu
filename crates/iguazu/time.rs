use num_rational::Rational64;

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum TimeType {
    Absolute {
        /// Duration in seconds of one clock tick
        period: Rational64,

        /// Time of first sample in nanoseconds since Unix epoch
        start: u64,
    },
    Relative {
        /// Duration in seconds of one clock tick
        period: Rational64,
    },
    Sequence
}

impl TimeType {
    pub fn period(&self) -> Option<Rational64> {
        match self {
            TimeType::Absolute { period, .. } => Some(*period),
            TimeType::Relative { period } => Some(*period),
            TimeType::Sequence => None,
        }
    }
}
