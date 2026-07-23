use std::{collections::BTreeSet, sync::Arc, time::Duration};

use ecow::EcoString;
use jiff::{Timestamp, civil::DateTime, fmt::temporal::Pieces, tz::TimeZone};

use crate::{ElementSize, import::ImportError, schema::{Entity, EntitySchema, EntityStream, Field, FieldKind, Ignored, attribute}, storage::MemoryStreamWriter, stream::Stream};

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum TimeUnit {
    Second,
    Millisecond,
    Microsecond,
    Nanosecond,
}

impl TimeUnit {
    pub fn from_rate(rate: f64) -> Option<Self> {
        match rate {
            1.0 => Some(TimeUnit::Second),
            1_000.0 => Some(TimeUnit::Millisecond),
            1_000_000.0 => Some(TimeUnit::Microsecond),
            1_000_000_000.0 => Some(TimeUnit::Nanosecond),
            _ => None,
        }
    }

    pub fn scale(self, d: Duration) -> Option<u64> {
        match self {
            TimeUnit::Second => Some(d.as_secs()),
            TimeUnit::Millisecond => d.as_millis().try_into().ok(),
            TimeUnit::Microsecond => d.as_micros().try_into().ok(),
            TimeUnit::Nanosecond => d.as_nanos().try_into().ok(),
        }
    }
}

fn pieces_time(pieces: &Pieces, default_zone: &TimeZone) -> Result<Timestamp, ()> {
    let dt = DateTime::from_parts(pieces.date(), pieces.time().unwrap_or_default());
    match pieces.to_numeric_offset() {
        Some(offset) => offset.to_timestamp(dt),
        None => default_zone.to_timestamp(dt),
    }.map_err(|_| ())
}

pub(crate) enum ColumnParser {
    Skip,
    Float32(MemoryStreamWriter),
    String { ends: MemoryStreamWriter, chars: MemoryStreamWriter },
    Enum8 { values: Vec<EcoString>, writer: MemoryStreamWriter },
    TimestampIso {
        epoch: Timestamp,
        unit: TimeUnit,
        default_zone: TimeZone,
        writer: MemoryStreamWriter
    },
    TimestampRelative {
        scale: f64,
        writer: MemoryStreamWriter
    },
}

impl ColumnParser {
    pub fn new(schema: &EntitySchema) -> Result<(EntityStream, ColumnParser), ImportError>{
        Ok(match schema {
            Entity::Data { data: Ignored, field, .. } => {
                let (data, parser) = match field.kind {
                    FieldKind::Float32 { pos: 0 } => {
                        let writer = MemoryStreamWriter::new(ElementSize::U32);
                        let data = writer.stream().clone() as Arc<dyn Stream>;
                        (data, ColumnParser::Float32(writer))
                    }
                    FieldKind::Enum { pos: 0, bits, ref values, ref variants } if bits <= 8 && variants.is_empty() => {
                        let writer = MemoryStreamWriter::new(ElementSize::U8);
                        let data = writer.stream().clone() as Arc<dyn Stream>;
                        (data, ColumnParser::Enum8 { values: values.clone(), writer })
                    }
                    FieldKind::Timestamp => {
                        let writer = MemoryStreamWriter::new(ElementSize::U64);
                        let data = writer.stream().clone() as Arc<dyn Stream>;

                        let Some(rate) = field.time_rate() else {
                          return Err(ImportError::SchemaMismatch("Timestamp rate must be defined".into()));
                        };

                        let parser = match field.time_epoch() {
                            Some(epoch) => {
                                let Some(unit) = TimeUnit::from_rate(rate) else {
                                    return Err(ImportError::SchemaMismatch("Timestamp unit must be s, ms, us, or ns".into()));
                                };
                                ColumnParser::TimestampIso { epoch: epoch.timestamp(), unit, writer, default_zone: epoch.time_zone().clone() }
                            }
                            None => ColumnParser::TimestampRelative { scale: rate, writer }
                        };
                        (data, parser)
                    }
                    ref k => {
                        return Err(ImportError::SchemaMismatch(format!("Field type {:?} not supported in CSV column", k)));
                    }
                };

                let entity = Entity::Data { data, field: field.clone(), summaries: Default::default() };
                (entity, parser)
            }

            Entity::VariableArray { data: Ignored, child, attributes } => {
                match **child {
                    Entity::Data { data: Ignored, ref field, ..} => {
                        let ends = MemoryStreamWriter::new(ElementSize::U64);
                        let ends_stream = ends.stream().clone() as Arc<dyn Stream>;

                        let (data, parser) = match &field.kind {
                            FieldKind::Character { pos: 0 } => {
                                let chars = MemoryStreamWriter::new(ElementSize::U8);
                                let data = chars.stream().clone() as Arc<dyn Stream>;
                                (data, ColumnParser::String { ends, chars })
                            }
                            k => {
                                return Err(ImportError::SchemaMismatch(format!("Field type {:?} not supported in CSV column", k)));
                            }
                        };

                        let inner = Entity::Data { data, field: field.clone(), summaries: Default::default() };

                        let entity = Entity::VariableArray {
                            data: ends_stream,
                            child: Box::new(inner),
                            attributes: attributes.clone(),
                        };

                        (entity, parser)
                    }

                    ref k => {
                        return Err(ImportError::SchemaMismatch(format!("VariableArray child type {:?} not supported in CSV column", k)));
                    }
                }
            }

            ref k => {
                return Err(ImportError::SchemaMismatch(format!("Field type {:?} not supported in CSV column", k)));
            }
        })
    }

    pub fn parse(&mut self, value: &[u8]) -> Result<(), &'static str> {
        match self {
            ColumnParser::Skip => {
                Ok(())
            }
            ColumnParser::Float32(writer) => {
                let f = lexical_core::parse::<f32>(value)
                    .map_err(|_| "float")?;
                writer.extend_from_slice(&f.to_ne_bytes());
                Ok(())
            }
            ColumnParser::Enum8 { values, writer } => {
                let val = values.iter().position(|v| v.as_bytes() == value)
                    .ok_or("enum value")?;
                writer.extend_from_slice(&[val as u8]);
                Ok(())
            }
            ColumnParser::String { ends, chars } => {
                chars.extend_from_slice(value);
                ends.extend_from_slice(&chars.pos().to_ne_bytes());
                Ok(())
            }
            ColumnParser::TimestampIso { epoch, unit, default_zone, writer } => {
                let pieces = Pieces::parse(value).map_err(|_| "timestamp")?;
                let t = pieces_time(&pieces, default_zone).map_err(|_| "timestamp (invalid)")?;
                let d = Duration::try_from(t.duration_since(*epoch)).map_err(|_| "timestamp (prior to epoch)")?;
                let val: u64 = unit.scale(d).ok_or("timestamp (out of range)")?;
                writer.extend_from_slice(&val.to_ne_bytes());
                Ok(())
            }
            ColumnParser::TimestampRelative { writer, scale } => {
                let f = lexical_core::parse::<f64>(value)
                    .map_err(|_| "decimal timestamp")?;

                let fv = (*scale * f).round();
                if fv < 0.0 || fv > (u64::MAX as f64) {
                    return Err("decimal timestamp (out of range)");
                }
                let v = fv as u64;

                writer.extend_from_slice(&v.to_ne_bytes());
                Ok(())
            }
        }
    }

    pub fn commit(&mut self) {
        match self {
            ColumnParser::Float32(writer) => writer.commit(),
            ColumnParser::Enum8 { writer, .. } => writer.commit(),
            ColumnParser::String { ends, chars } => {
                ends.commit();
                chars.commit();
            }
            ColumnParser::TimestampIso { writer, .. } => writer.commit(),
            ColumnParser::TimestampRelative { writer, .. } => writer.commit(),
            ColumnParser::Skip => {}
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum InferredType {
    AbsoluteTimestamp(TimeZone),
    RelativeTimestamp,
    Float(f64, f64),
    Enum(BTreeSet<EcoString>),
    String,
}

impl InferredType {
    fn schema(&self) -> EntitySchema {
        match self {
            InferredType::AbsoluteTimestamp(time_zone) => EntitySchema::field(
                Field::timestamp(1e9, Some(Timestamp::UNIX_EPOCH.to_zoned(time_zone.clone())))
            ),
            InferredType::RelativeTimestamp => EntitySchema::field(Field::timestamp(1e9, None)),
            InferredType::Float(min, max) => EntitySchema::field(
                Field::float32()
                    .with_attribute(attribute::core::NUMBER_MIN, *min)
                    .with_attribute(attribute::core::NUMBER_MAX, *max)
            ),
            InferredType::Enum(value_set) => EntitySchema::field(Field::r#enum(value_set.iter().cloned())),
            InferredType::String => EntitySchema::string(),
        }
    }
}

pub struct TypeInfer {
    prioritize_relative_timestamp: bool,
    time_zone: Option<TimeZone>,
    absolute_timestamp: Option<jiff::Timestamp>,
    float: Option<(f64, f64)>,
    monotonic_float: bool,
    enum_set: Option<BTreeSet<EcoString>>,
}

impl TypeInfer {
    pub fn new(prioritize_relative_timestamp: bool) -> Self {
        // Widest set, will be reduced as we see values
        Self {
            prioritize_relative_timestamp,
            time_zone: None,
            absolute_timestamp: Some(jiff::Timestamp::UNIX_EPOCH),
            float: Some((f64::INFINITY, f64::NEG_INFINITY)),
            monotonic_float: true,
            enum_set: Some(BTreeSet::new()),
        }
    }

    pub fn update(&mut self, val: &[u8]) {
        if let Some(max) = self.absolute_timestamp {
            if let Ok(pieces) = Pieces::parse(val) && let Ok(t) = pieces_time(&pieces, &TimeZone::UTC) && t >= max {
                if self.time_zone.is_none() {
                    self.time_zone = pieces.to_time_zone().ok().flatten()
                        .or_else(|| pieces.to_numeric_offset().map(|o| o.to_time_zone()));
                }
                self.absolute_timestamp = Some(t);
            } else {
                self.absolute_timestamp = None;
            }
        }

        if let Some((min, max)) = self.float {
            if let Ok(f) = lexical_core::parse::<f64>(val) {
                self.monotonic_float &= f >= max;
                self.float = Some((min.min(f), max.max(f)));
            } else {
                self.monotonic_float = false;
                self.float = None;
            }
        }

        if let Some(enum_set) = &mut self.enum_set {
            if val.len() <= 15 && let Ok(s) = std::str::from_utf8(val) {
                enum_set.insert(s.into());
                if enum_set.len() > 32 {
                    self.enum_set = None;
                }
            } else {
                self.enum_set = None;
            }
        }
    }

    pub fn as_absolute_timestamp(&self) -> Option<TimeZone> {
        self.absolute_timestamp.map(|_| self.time_zone.clone().unwrap_or(TimeZone::UTC))
    }

    pub fn is_relative_timestamp(&self) -> bool {
        self.float.is_some() && self.monotonic_float
    }

    pub fn as_float(&self) -> Option<(f64, f64)> {
        self.float
            .filter(|(min, max)| min.is_finite() && max.is_finite())
    }

    pub fn as_enum(&self) -> Option<&BTreeSet<EcoString>> {
        self.enum_set.as_ref()
    }

    pub fn possible_types(&self) -> impl Iterator<Item = InferredType> {
        [
            self.as_absolute_timestamp().map(InferredType::AbsoluteTimestamp),
            (self.prioritize_relative_timestamp && self.is_relative_timestamp()).then_some(InferredType::RelativeTimestamp),
            self.as_float().map(|(min, max)| InferredType::Float(min, max)),
            (!self.prioritize_relative_timestamp && self.is_relative_timestamp()).then_some(InferredType::RelativeTimestamp),
            self.as_enum().map(|set| InferredType::Enum(set.clone())),
            Some(InferredType::String)
        ].into_iter().flatten()
    }

    pub fn schema(&self) -> EntitySchema {
        // Always includes at least String
        self.possible_types().next().unwrap().schema()
    }
}

#[test]
fn test_infer_type() {
    let mut infer = TypeInfer::new(true);

    infer.update(b"2020-01-01T00:00:00-07:00");
    infer.update(b"2020-01-02T00:00:00-07:00");
    assert_eq!(Vec::from_iter(infer.possible_types()), &[
        InferredType::AbsoluteTimestamp(TimeZone::fixed(jiff::tz::Offset::from_hours(-7).unwrap())),
        InferredType::String,
    ]);

    infer.update(b"not a timestamp");
    assert_eq!(Vec::from_iter(infer.possible_types()), &[
        InferredType::String,
    ]);

    let mut infer = TypeInfer::new(true);
    infer.update(b"1.5");
    infer.update(b"2.5");
    infer.update(b"3.5");
    assert_eq!(Vec::from_iter(infer.possible_types()), &[
        InferredType::RelativeTimestamp,
        InferredType::Float(1.5, 3.5),
        InferredType::Enum(BTreeSet::from_iter(["1.5".into(), "2.5".into(), "3.5".into()])),
        InferredType::String,
    ]);

    infer.update(b"2.5");
    assert_eq!(Vec::from_iter(infer.possible_types()), &[
        InferredType::Float(1.5, 3.5),
        InferredType::Enum(BTreeSet::from_iter(["1.5".into(), "2.5".into(), "3.5".into()])),
        InferredType::String,
    ]);


    infer.update(b"x");
    assert_eq!(Vec::from_iter(infer.possible_types()), &[
        InferredType::Enum(BTreeSet::from_iter(["1.5".into(), "2.5".into(), "3.5".into(), "x".into()])),
        InferredType::String,
    ]);

    let mut infer = TypeInfer::new(false);
    infer.update(b"1.5");
    infer.update(b"2.5");
    assert_eq!(Vec::from_iter(infer.possible_types()), &[
        InferredType::Float(1.5, 2.5),
        InferredType::RelativeTimestamp,
        InferredType::Enum(BTreeSet::from_iter(["1.5".into(), "2.5".into()])),
        InferredType::String,
    ]);

    let mut infer = TypeInfer::new(true);
    infer.update(b"red");
    infer.update(b"red");
    infer.update(b"green");
    infer.update(b"blue");
    assert_eq!(Vec::from_iter(infer.possible_types()), &[
        InferredType::Enum(BTreeSet::from_iter(["red".into(), "green".into(), "blue".into()])),
        InferredType::String,
    ]);

    for i in 0..29 {
        infer.update(format!("value{}", i).as_bytes());
    }
    assert!(infer.enum_set.is_some());

    infer.update(b"too many");
    assert_eq!(Vec::from_iter(infer.possible_types()), &[
        InferredType::String,
    ]);
}
