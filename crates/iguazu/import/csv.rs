use std::iter;
use std::{pin::Pin, sync::Arc};
use std::future;

use csv_core::ReadRecordResult;
use futures_lite::{AsyncBufRead, AsyncBufReadExt};
use indexmap::IndexMap;

use crate::schema::{Entity, EntityKind, EntityStream, Ignored};
use crate::storage::MemoryStreamWriter;
use crate::stream::Stream;
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

    fn import(self: Box<Self>, schema: Option<EntitySchema>) -> Pin<Box<dyn Future<Output = Result<(EntityStream, Pin<Box<dyn Future<Output = Result<(), ImportError>> + Send>>), ImportError>> + Send>> {
        Box::pin(async {
            let schema = schema.ok_or_else(|| ImportError::SchemaMismatch("Schema must be specified for CSV import".into()))?;
            let file_stream = self.file.stream();
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

enum ColumnParser {
    Skip,
    Float32(MemoryStreamWriter),
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
        }
    }
}

fn column_parsers(schema: &EntitySchema, headers: &[String]) -> Result<(Vec<ColumnParser>, EntityStream), ImportError> {
    let mut parsers = (0..headers.len()).map(|_| ColumnParser::Skip).collect::<Vec<_>>();
    
    let entity = match schema.kind {
        EntityKind::Record { ref children } => {
            let children = children.iter().map(|(name, child)| {
                let column = headers.iter().position(|h| h == name)
                    .ok_or_else(|| ImportError::SchemaMismatch(format!("No column found for field `{}`", name)))?;
                let (entity, parser) = column_parser(&child)?;
                parsers[column] = parser;
                Ok((name.clone(), entity))
            }).collect::<Result<IndexMap<_, _>, ImportError>>()?;

            Entity { 
                kind: EntityKind::Record { children },
                attributes: schema.attributes.clone(),
            }
        }
        _ => return Err(ImportError::SchemaMismatch(format!("CSV import expects top-level entity to be a record, not {:?}", schema.kind))),
    };

    Ok((parsers, entity))
}

fn column_parser(schema: &EntitySchema) -> Result<(EntityStream, ColumnParser), ImportError>{
    Ok(match schema.kind {
        EntityKind::Number { data: Ignored, encoding } => {
            let writer = MemoryStreamWriter::new(crate::stream::ElementSize::U32);
            let data = writer.stream().clone() as Arc<dyn Stream>;

            let entity = Entity { 
                kind: EntityKind::Number { data, encoding },
                attributes: schema.attributes.clone(),
            };

            (entity, ColumnParser::Float32(writer))
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

    async fn read_row(&mut self) -> Result<Option<Row>, ImportError> {
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
        }
        log::info!("import completed, {} lines", rows);
        Ok(())
    }))
}

