use std::iter;
use std::time::Duration;
use std::{pin::Pin, sync::Arc};
use std::future;

use async_executor::Executor;
use csv_core::ReadRecordResult;
use futures_lite::{AsyncBufRead, AsyncBufReadExt};
use indexmap::IndexMap;
use jiff::civil::DateTime;
use jiff::fmt::temporal::Pieces;
use jiff::tz::TimeZone;
use jiff::Timestamp;

use crate::schema::{Entity, FieldKind, EntityStream, Ignored};
use crate::storage::MemoryStreamWriter;
use crate::{ ElementType, stream::Stream };
use crate::{io::ReadableFile, schema::EntitySchema};

use super::{ImportError, Importer};

pub struct CsvImporter {
    file: Arc<dyn ReadableFile>,
    opts: CsvOptions,
}

impl CsvImporter {
    fn new(file: Arc<dyn ReadableFile>) -> Self {
        CsvImporter {
            file,
            opts: CsvOptions::default(),
        }
    }
}

impl Importer for CsvImporter {
    fn load_schema(&mut self) -> Pin<Box<dyn Future<Output = Result<EntitySchema, super::ImportError>> + Send + '_>> {
        Box::pin(future::ready(Err(ImportError::SchemaMismatch("Schema must currently be specified for CSV".into()))))
    }

    fn import(self: Box<Self>, schema: Option<EntitySchema>, _executor: Arc<Executor<'static>>) -> Pin<Box<dyn Future<Output = Result<(EntityStream, Pin<Box<dyn Future<Output = Result<(), ImportError>> + Send>>), ImportError>> + Send>> {
        Box::pin(async {
            let schema = schema.ok_or_else(|| ImportError::SchemaMismatch("Schema must be specified for CSV import".into()))?;
            let file_stream = self.file.stream().await?;
            let (entity, completion) = load(file_stream, self.opts, schema).await?;
            Ok((entity, Box::pin(completion) as Pin<Box<_>>))
        })
    }
}

pub fn csv(file: Arc<dyn ReadableFile>) -> Box<dyn Importer> {
    Box::new(CsvImporter::new(file))
}

pub fn tsv(file: Arc<dyn ReadableFile>) -> Box<dyn Importer> {
    Box::new(CsvImporter::new(file))
}

pub struct CsvOptions {
    pub delimiter: u8,
    pub headers: CsvHeaders,
    pub start_row: u64,
}

pub enum CsvHeaders {
    Row(u64),
    Specified(Vec<String>),
}

impl Default for CsvOptions {
    fn default() -> Self {
        CsvOptions {
            delimiter: b',',
            headers: CsvHeaders::Row(1),
            start_row: 2,
        }
    }
}

impl CsvOptions {
    fn reader(&self) -> csv_core::Reader {
        csv_core::ReaderBuilder::new()
            .delimiter(self.delimiter)
            .build()
    }
}

#[derive(Copy, Clone)]
enum TimeUnit {
    Second,
    Millisecond,
    Microsecond,
    Nanosecond,
}

impl TimeUnit {
    fn from_rate(rate: f64) -> Option<Self> {
        match rate {
            1.0 => Some(TimeUnit::Second),
            1_000.0 => Some(TimeUnit::Millisecond),
            1_000_000.0 => Some(TimeUnit::Microsecond),
            1_000_000_000.0 => Some(TimeUnit::Nanosecond),
            _ => None,
        }
    }

    fn scale(self, d: Duration) -> Option<u64> {
        match self {
            TimeUnit::Second => Some(d.as_secs()),
            TimeUnit::Millisecond => d.as_millis().try_into().ok(),
            TimeUnit::Microsecond => d.as_micros().try_into().ok(),
            TimeUnit::Nanosecond => d.as_nanos().try_into().ok(),
        }
    }
}

enum ColumnParser {
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
    fn parse(&mut self, value: &[u8]) -> Result<(), &'static str> {
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

    fn commit(&mut self) {
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

fn column_parsers(schema: &EntitySchema, headers: &[String]) -> Result<(Vec<ColumnParser>, EntityStream), ImportError> {
    let mut parsers = (0..headers.len()).map(|_| ColumnParser::Skip).collect::<Vec<_>>();

    let entity = match schema {
        Entity::Record { children, attributes } => {
            let children = children.iter().map(|(name, child)| {
                let column = headers.iter().position(|h| h == name)
                    .ok_or_else(|| ImportError::SchemaMismatch(format!("No column found for field `{}`", name)))?;
                let (entity, parser) = column_parser(&child)?;
                parsers[column] = parser;
                Ok((name.clone(), entity))
            }).collect::<Result<IndexMap<_, _>, ImportError>>()?;

            Entity::Record { children, attributes: attributes.clone() }
        }
        _ => return Err(ImportError::SchemaMismatch(format!("CSV import expects top-level entity to be a record, not {:?}", schema))),
    };

    Ok((parsers, entity))
}

fn column_parser(schema: &EntitySchema) -> Result<(EntityStream, ColumnParser), ImportError>{
    Ok(match schema {
        Entity::Data { data: Ignored, field, .. } => {
            let (data, parser) = match &field.kind {
                FieldKind::Float32 => {
                    let writer = MemoryStreamWriter::new(ElementType::U32);
                    let data = writer.stream().clone() as Arc<dyn Stream>;
                    (data, ColumnParser::Float32(writer))
                }
                FieldKind::Timestamp => {
                    let writer = MemoryStreamWriter::new(ElementType::U64);
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
                    let ends = MemoryStreamWriter::new(ElementType::U64);
                    let ends_stream = ends.stream().clone() as Arc<dyn Stream>;

                    let (data, parser) = match &field.kind {
                        FieldKind::Character => {
                            let chars = MemoryStreamWriter::new(ElementType::U8);
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

struct CsvParser {
    source: Pin<Box<dyn AsyncBufRead + Send>>,
    reader: csv_core::Reader,
    out: Vec<u8>,
    ends: Vec<usize>,
}

impl CsvParser {
    fn new(source: Pin<Box<dyn AsyncBufRead + Send>>, opts: &CsvOptions) -> Self {
        let reader = opts.reader();
        CsvParser {
            source,
            reader,
            out: vec![0; 1024],
            ends: vec![0; 1024],
        }
    }

    async fn skip_to_line(&mut self, line: u64) -> Result<(), ImportError> {
        while self.reader.line() + 1 < line {
            log::debug!("Skipping line {}", self.reader.line());
            if self.read_row().await?.is_none() {
                return Err(ImportError::InvalidFile("CSV file ended before start row".into()));
            }
        }
        Ok(())
    }

    async fn read_row(&mut self) -> Result<Option<Row<'_>>, ImportError> {
        let mut input = &[][..];
        let mut consumed = 0;
        let mut outlen = 0;
        let mut endlen = 0;

        loop {
            if input.is_empty() {
                self.source.as_mut().consume(consumed);
                consumed = 0;
                input = self.source.fill_buf().await.map_err(ImportError::Io)?;
            }

            let (res, nin, nout, nend) = self.reader.read_record(
                &input,
                &mut self.out[outlen..],
                &mut self.ends[endlen..],
            );

            input = &input[nin..];
            consumed += nin;
            outlen += nout;
            endlen += nend;

            match res {
                ReadRecordResult::InputEmpty => continue,
                ReadRecordResult::OutputFull => {
                    let len = self.out.len();
                    self.out.resize(len * 2, 0);
                }
                ReadRecordResult::OutputEndsFull => {
                    let len = self.ends.len();
                    self.ends.resize(len * 2, 0);
                }
                ReadRecordResult::Record => {
                    self.source.as_mut().consume(consumed);
                    let line = self.reader.line() - 1;
                    return Ok(Some(Row { parser: self, line, cols: endlen}));
                }
                ReadRecordResult::End => {
                    return Ok(None);
                }
            }
        }
    }
}

struct Row<'a> {
    parser: &'a mut CsvParser,
    line: u64,
    cols: usize,
}

impl Row<'_> {
    fn column_values(&self) -> impl Iterator<Item = &[u8]> {
        let starts = iter::once(0).chain(self.parser.ends[..self.cols].iter().copied());
        let ends = self.parser.ends[..self.cols].iter().copied();
        starts.zip(ends).map(|(start, end)| &self.parser.out[start..end])
    }
}

async fn read_header(csv: &mut CsvParser, header_opt: &CsvHeaders, start_row: u64) -> Result<Vec<String>, ImportError> {
    let header = match *header_opt {
        CsvHeaders::Specified(ref headers) => Ok(headers.clone()),
        CsvHeaders::Row(header_row) => {
            csv.skip_to_line(header_row).await?;
            csv.read_row().await?
                .ok_or_else(|| ImportError::InvalidFile("CSV file ended before header row".into()))?
                .column_values().map(|f| String::from_utf8(f.to_owned())).collect::<Result<Vec<_>, _>>()
                .map_err(|_| ImportError::InvalidFile(format!("Failed to parse header row as UTF-8")))
        }
    };

    csv.skip_to_line(start_row).await?;
    header
}

pub async fn load(stream: Pin<Box<dyn AsyncBufRead + Send>>, opts: CsvOptions, schema: EntitySchema) -> Result<(EntityStream, impl Future<Output = Result<(), ImportError>> + ), ImportError> {
    let mut csv = CsvParser::new(stream, &opts);

    let headers = read_header(&mut csv, &opts.headers, opts.start_row).await?;
    log::info!("found headers {:?}", headers);

    let (mut parsers, entity) = column_parsers(&schema, &headers)?;

    Ok((entity, async move {
        let mut rows: u64 = 0;
        while let Some(row) = csv.read_row().await? {
            if row.cols != parsers.len() {
                return Err(ImportError::InvalidFile(format!("Line {} has {} columns, expected {}", row.line, row.cols, parsers.len())));
            }
            for (value, parser) in row.column_values().zip(parsers.iter_mut()) {
                parser.parse(value)
                    .map_err(|e| ImportError::InvalidFile(format!("Failed to parse value {:?} as {} on line {}", String::from_utf8_lossy(value), e, row.line)))?;
            }
            rows += 1;
            for parser in parsers.iter_mut() {
                parser.commit();
            }
        }
        log::info!("import completed, {} lines", rows);
        Ok(())
    }))
}

#[test]
fn test_csv() {
    use std::str::FromStr;
    use crate::schema::{Field, attribute::core::{TIME_EPOCH, TIME_RATE}};
    use futures_lite::{future::block_on, io::Cursor};

    let file = Box::pin(Cursor::new(b"timestamp,value,str\n2025-01-01T00:00:01Z,1.0,abc\n2025-01-01T00:00:01.100Z,2.0,defg\n"));

    let opts = CsvOptions::default();

    let schema = Entity::Record {
        children: IndexMap::from([
            ("timestamp".into(), Entity::Data {
                field: Field::new(FieldKind::Timestamp)
                    .with_attribute(TIME_RATE, 1000.0)
                    .with_attribute(TIME_EPOCH, Timestamp::from_str("2025-01-01T00:00:00Z").unwrap()),
                data: Ignored,
                summaries: Default::default(),
            }),
            ("value".into(), Entity::Data {
                field: Field::new(FieldKind::Float32),
                data: Ignored,
                summaries: Default::default(),
            }),
            ("str".into(), Entity::VariableArray {
                data: Ignored,
                child: Box::new(Entity::Data {
                    field: Field::new(FieldKind::Character),
                    data: Ignored,
                    summaries: Default::default(),
                }),
                attributes: Default::default(),
            }),
        ]),
        attributes: Default::default(),
    };

    let (entity, completion) = block_on(load(file, opts, schema)).unwrap();
    block_on(completion).unwrap();

    let vm = crate::view::ViewManager::new(std::task::Waker::noop().clone());
    let timestamp = vm.int_view(entity.child("timestamp").unwrap()).unwrap();
    assert_eq!(timestamp.get_u64(0), Some(1000));
    assert_eq!(timestamp.get_u64(1), Some(1100));

    let num = vm.number_view(entity.child("value").unwrap()).unwrap();
    assert_eq!(num.get(0), Some(1.0));
    assert_eq!(num.get(1), Some(2.0));

    let str = vm.text_view(entity.child("str").unwrap());
    assert_eq!(str.format(0).to_string(), "abc");
    assert_eq!(str.format(1).to_string(), "defg");
}
