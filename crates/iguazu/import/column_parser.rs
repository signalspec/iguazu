use std::{sync::Arc, time::Duration};

use jiff::{Timestamp, civil::DateTime, fmt::temporal::Pieces, tz::TimeZone};

use crate::{ElementSize, import::ImportError, schema::{Entity, EntitySchema, EntityStream, FieldKind, Ignored}, storage::MemoryStreamWriter, stream::Stream};

#[derive(Copy, Clone)]
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

pub (crate) enum ColumnParser {
    Skip,
    Float32(MemoryStreamWriter),
    String { ends: MemoryStreamWriter, chars: MemoryStreamWriter },
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
                let (data, parser) = match &field.kind {
                    FieldKind::Float32 => {
                        let writer = MemoryStreamWriter::new(ElementSize::U32);
                        let data = writer.stream().clone() as Arc<dyn Stream>;
                        (data, ColumnParser::Float32(writer))
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
                                ColumnParser::TimestampIso { epoch, unit, writer, default_zone: TimeZone::UTC }
                            }
                            None => ColumnParser::TimestampRelative { scale: rate, writer }
                        };
                        (data, parser)
                    }
                    k => {
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
                            FieldKind::Character => {
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
            ColumnParser::String { ends, chars } => {
                chars.extend_from_slice(value);
                ends.extend_from_slice(&chars.pos().to_ne_bytes());
                Ok(())
            }
            ColumnParser::TimestampIso { epoch, unit, default_zone, writer } => {
                let pieces = Pieces::parse(value).map_err(|_| "timestamp")?;
                let dt = DateTime::from_parts(pieces.date(), pieces.time().unwrap_or_default());
                let t = match pieces.offset() {
                    Some(offset) => offset.to_numeric_offset().to_timestamp(dt),
                    None => default_zone.to_timestamp(dt),
                }.map_err(|_| "timestamp (invalid)")?;
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
