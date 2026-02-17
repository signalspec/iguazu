use std::iter;
use std::{pin::Pin, sync::Arc};
use std::future;

use csv_core::ReadRecordResult;
use futures_lite::{AsyncBufRead, AsyncBufReadExt};
use indexmap::IndexMap;

use crate::schema::{Entity, EntityStream};
use crate::storage::Pool;
use crate::{io::ReadableFile, schema::EntitySchema};
use super::{ImportError, Importer, column_parser::ColumnParser};

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

    fn import(self: Box<Self>, schema: Option<EntitySchema>, _executor: Arc<Pool>) -> Pin<Box<dyn Future<Output = Result<(EntityStream, Pin<Box<dyn Future<Output = Result<(), ImportError>> + Send>>), ImportError>> + Send>> {
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

fn column_parsers(schema: &EntitySchema, headers: &[String]) -> Result<(Vec<ColumnParser>, EntityStream), ImportError> {
    let mut parsers = (0..headers.len()).map(|_| ColumnParser::Skip).collect::<Vec<_>>();

    let entity = match schema {
        Entity::Record { children, attributes } => {
            let children = children.iter().map(|(name, child)| {
                let column = headers.iter().position(|h| h == name)
                    .ok_or_else(|| ImportError::SchemaMismatch(format!("No column found for field `{}`", name)))?;
                let (entity, parser) = ColumnParser::new(&child)?;
                parsers[column] = parser;
                Ok((name.clone(), entity))
            }).collect::<Result<IndexMap<_, _>, ImportError>>()?;

            Entity::Record { children, attributes: attributes.clone() }
        }
        _ => return Err(ImportError::SchemaMismatch(format!("CSV import expects top-level entity to be a record, not {:?}", schema))),
    };

    Ok((parsers, entity))
}

trait CsvHandler {
    fn should_continue(&self, _line: u64) -> bool {
        true
    }
    fn row(&mut self, row: Row<'_>) -> Result<(), ImportError>;
    fn flush(&mut self) {}
}

struct ColumnsHandler<'a>(&'a mut [ColumnParser]);

impl CsvHandler for ColumnsHandler<'_> {
    fn row(&mut self, row: Row<'_>) -> Result<(), ImportError> {
        if row.col_ends.len() != self.0.len() {
            return Err(ImportError::InvalidFile(format!("Line {} has {} columns, expected {}", row.line, row.col_ends.len(), self.0.len())));
        }
        for (value, parser) in row.column_values().zip(self.0.iter_mut()) {
            parser.parse(value)
                .map_err(|e| ImportError::InvalidFile(format!("Failed to parse value {:?} as {} on line {}", String::from_utf8_lossy(value), e, row.line)))?;
        }
        Ok(())
    }

    fn flush(&mut self) {
        for parser in self.0.iter_mut() {
            parser.commit();
        }
    }
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
            ends: vec![0; 128],
        }
    }

    async fn skip_to_line(&mut self, line: u64) -> Result<(), ImportError> {
        struct SkipToLineHandler { line: u64 }
        impl CsvHandler for SkipToLineHandler {
            fn should_continue(&self, line: u64) -> bool {
                line < self.line
            }
            fn row(&mut self, _: Row<'_>) -> Result<(), ImportError> { Ok(()) }
        }
        self.read_rows(SkipToLineHandler { line }).await
    }

    async fn read_row(&mut self) -> Result<Vec<String>, ImportError> {
        let mut result: Option<Vec<String>> = None;
        struct CsvRowHandler<'a> { result: &'a mut Option<Vec<String>> }
        impl CsvHandler for CsvRowHandler<'_> {
            fn should_continue(&self, _: u64) -> bool {
                self.result.is_none()
            }

            fn row(&mut self, row: Row<'_>) -> Result<(), ImportError> {
                let values = row.column_values()
                    .map(|f| String::from_utf8(f.to_owned())).collect::<Result<Vec<_>, _>>()
                    .map_err(|_| ImportError::InvalidFile(format!("Failed to parse header row as UTF-8")))?;
                *self.result = Some(values);
                Ok(())
            }
        }

        self.read_rows(CsvRowHandler { result: &mut result }).await?;
        result.ok_or_else(|| ImportError::InvalidFile("CSV file ended before header row".into()))
    }

    async fn read_rows(&mut self, mut handler: impl CsvHandler) -> Result<(), ImportError> {
        let mut input = &[][..];
        let mut consumed = 0;
        let mut outlen = 0;
        let mut endlen = 0;

        while handler.should_continue(self.reader.line()) {
            if input.is_empty() {
                if consumed > 0 {
                    handler.flush();
                    self.source.as_mut().consume(consumed);
                    consumed = 0;
                }
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
                    let len = self.out.len().checked_mul(2).ok_or_else(|| ImportError::InvalidFile("line too long".into()))?;
                    self.out.resize(len, 0);
                }
                ReadRecordResult::OutputEndsFull => {
                    let len = self.ends.len().checked_mul(2).ok_or_else(|| ImportError::InvalidFile("line too long".into()))?;
                    self.ends.resize(len, 0);
                }
                ReadRecordResult::Record => {
                    let line = self.reader.line() - 1;
                    handler.row(Row { line, buf: &self.out[..outlen], col_ends: &self.ends[..endlen] })?;
                    outlen = 0;
                    endlen = 0;
                }
                ReadRecordResult::End => break,
            }
        }

        if consumed > 0 {
            handler.flush();
            self.source.as_mut().consume(consumed);
        }

        return Ok(());
    }
}

struct Row<'a> {
    line: u64,
    buf: &'a [u8],
    col_ends: &'a [usize],
}

impl Row<'_> {
    fn column_values(&self) -> impl Iterator<Item = &[u8]> {
        let starts = iter::once(0).chain(self.col_ends.iter().copied());
        let ends = self.col_ends.iter().copied();
        starts.zip(ends).map(|(start, end)| &self.buf[start..end])
    }
}

async fn read_header(csv: &mut CsvParser, header_opt: &CsvHeaders, start_row: u64) -> Result<Vec<String>, ImportError> {
    let header = match *header_opt {
        CsvHeaders::Specified(ref headers) => Ok(headers.clone()),
        CsvHeaders::Row(header_row) => {
            csv.skip_to_line(header_row).await?;
            csv.read_row().await
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
        csv.read_rows(ColumnsHandler(&mut parsers)).await?;
        log::info!("import completed, {} lines", csv.reader.line());
        Ok(())
    }))
}

#[test]
fn test_csv() {
    use std::str::FromStr;
    use crate::schema::{Field, FieldKind, Ignored, attribute::core::{TIME_EPOCH, TIME_RATE}};
    use jiff::Timestamp;
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
