use std::iter;
use std::{pin::Pin, sync::Arc};
use std::future;

use csv_core::{ReadRecordResult, ReaderBuilder};
use futures_lite::{AsyncBufRead, AsyncBufReadExt};
use indexmap::IndexMap;

use crate::schema::{Entity, EntityStream};
use crate::storage::Pool;
use crate::{io::ReadableFile, schema::EntitySchema};
use super::OptionDescription;
use super::{ImportError, Importer, column_parser::ColumnParser};

/// Importer for CSV / TSV and similar formats.
pub struct CsvImporter {
    delimiter: u8,
    terminator: Option<u8>,
    quote: Option<u8>,
    escape: Option<u8>,
    double_quote: bool,
    comment: Option<u8>,
    skip: u64,
    columns: Option<Vec<String>>,
}

impl CsvImporter {
    pub fn csv() -> Self {
        Self {
            delimiter: b',',
            terminator: None,
            quote: Some(b'"'),
            escape: None,
            double_quote: true,
            comment: None,
            skip: 0,
            columns: None,
        }
    }

    pub fn tsv() -> Self {
        Self::csv().with_delimiter(b'\t').with_quote(None)
    }

    pub fn delimiter(&self) -> u8 {
        self.delimiter
    }

    pub fn set_delimiter(&mut self, byte: u8) {
        self.delimiter = byte;
    }

    pub fn with_delimiter(mut self, byte: u8) -> Self {
        self.set_delimiter(byte);
        self
    }

    pub fn terminator(&self) -> Option<u8> {
        self.terminator
    }

    pub fn set_terminator(&mut self, byte: Option<u8>) {
        self.terminator = byte;
    }

    pub fn with_terminator(mut self, byte: Option<u8>) -> Self {
        self.set_terminator(byte);
        self
    }

    pub fn quote(&self) -> Option<u8> {
        self.quote
    }

    pub fn set_quote(&mut self, byte: Option<u8>) {
        self.quote = byte;
    }

    pub fn with_quote(mut self, byte: Option<u8>) -> Self {
        self.set_quote(byte);
        self
    }

    pub fn escape(&self) -> Option<u8> {
        self.escape
    }

    pub fn set_escape(&mut self, byte: Option<u8>) {
        self.escape = byte;
    }

    pub fn with_escape(mut self, byte: Option<u8>) -> Self {
        self.set_escape(byte);
        self
    }

    pub fn double_quote(&self) -> bool {
        self.double_quote
    }

    pub fn set_double_quote(&mut self, double_quote: bool) {
        self.double_quote = double_quote;
    }

    pub fn with_double_quote(mut self, double_quote: bool) -> Self {
        self.set_double_quote(double_quote);
        self
    }

    pub fn comment(&self) -> Option<u8> {
        self.comment
    }

    pub fn set_comment(&mut self, byte: Option<u8>) {
        self.comment = byte;
    }

    pub fn with_comment(mut self, byte: Option<u8>) -> Self {
        self.set_comment(byte);
        self
    }

    pub fn columns(&self) -> Option<&[String]> {
        self.columns.as_deref()
    }

    pub fn set_columns(&mut self, columns: Option<Vec<String>>) {
        self.columns = columns;
    }

    pub fn with_columns(mut self, columns: Option<Vec<String>>) -> Self {
        self.set_columns(columns);
        self
    }

    /// Load input from `AsyncBufRead`.
    pub async fn load(&self, stream: Pin<Box<dyn AsyncBufRead + Send>>, schema: EntitySchema) -> Result<(EntityStream, impl Future<Output = Result<(), ImportError>> + 'static), ImportError> {
        let mut csv = CsvParser::new(stream, ReaderBuilder::new()
            .delimiter(self.delimiter)
            .terminator(match self.terminator {
                Some(b) => csv_core::Terminator::Any(b),
                None => csv_core::Terminator::CRLF,
            })
            .quote(self.quote.unwrap_or(0))
            .quoting(self.quote.is_some())
            .escape(self.escape)
            .double_quote(self.double_quote)
            .comment(self.comment)
            .build());

        csv.skip_records(self.skip).await?;
        let headers = match &self.columns {
            Some(columns) => columns.clone(),
            None => csv.read_row().await?
        };

        log::info!("found headers {:?}", headers);

        let (mut parsers, entity) = column_parsers(&schema, &headers)?;

        Ok((entity, async move {
            csv.read_rows(ColumnsHandler(&mut parsers)).await?;
            log::info!("import completed, {} lines", csv.reader.line());
            Ok(())
        }))
    }
}

impl Importer for CsvImporter {
    fn options(&self) -> &'static [ OptionDescription ] {
        &[
            OptionDescription {
                name: "delimiter",
                description: "Delimiter byte",
            },
            OptionDescription {
                name: "terminator",
                description: "Record terminator byte. If empty, either `\\n` or `\\r\\n` is accepted.",
            },
            OptionDescription {
                name: "quote",
                description: "Quote byte. Set to `none` to disable quoting.",
            },
            OptionDescription {
                name: "escape",
                description: "Escape byte before quotes. Set to `none` to disable escaping.",
            },
            OptionDescription {
                name: "double_quote",
                description: "Whether to interpret doubled quote characters as an escaped quote.",
            },
            OptionDescription {
                name: "comment",
                description: "Comment byte. If specified, lines beginning with this byte will be skipped.",
            },
            OptionDescription {
                name: "skip",
                description: "Number of lines to skip before reading headers.",
            },
            OptionDescription {
                name: "columns",
                description: "Comma-separated list of column names. If empty, the first line of the file (after skip) is used as headers.",
            },
        ]
    }

    fn set(&mut self, option: &str, value: &str) -> Result<(), String> {
        fn parse_byte(value: &str) -> Result<u8, String> {
            match value {
                "\\t" => Ok(b'\t'),
                "\\n" => Ok(b'\n'),
                "\\r" => Ok(b'\r'),
                s if s.starts_with("\\x") && s.len() == 4 => {
                    u8::from_str_radix(&s[2..], 16).map_err(|_| format!("Invalid hex byte value"))
                },
                s if s.len() == 1 => Ok(s.as_bytes()[0]),
                _ => Err("Invalid single-byte value".into()),
            }
        }

        fn parse_optional_byte(value: &str) -> Result<Option<u8>, String> {
            match value {
                "" | "no" | "off" | "none" => Ok(None),
                _ => parse_byte(value).map(Some),
            }
        }

        fn parse_bool(value: &str) -> Result<bool, String> {
            match value {
                "true" | "yes" | "y" | "on" | "1" => Ok(true),
                "false" | "no" | "n" | "off" | "0" => Ok(false),
                _ => Err("Invalid boolean value".into()),
            }
        }

        Ok(match option {
            "delimiter" => {
                self.delimiter = parse_byte(value)?;
            }
            "terminator" => {
                self.terminator = parse_optional_byte(value)?;
            }
            "quote" => {
                self.quote = parse_optional_byte(value)?;
            }
            "escape" => {
                self.escape = parse_optional_byte(value)?;
            }
            "double_quote" => {
                self.double_quote = parse_bool(value)?;
            }
            "comment" => {
                self.comment = parse_optional_byte(value)?;
            }
            "skip" => {
                self.skip = value.parse().map_err(|_| "Invalid integer value")?;
            }
            "columns" => {
                self.columns = if value.trim().is_empty() {
                    None
                } else {
                    Some(value.split(',').map(|s| s.trim().to_string()).collect())
                };
            }
            _ => return Err("Unknown option".to_string()),
        })
    }

    fn get(&self, option: &str) -> Option<String> {
        fn char_repr(byte: u8) -> String {
            match byte {
                b'\t' => "\\t".into(),
                b'\n' => "\\n".into(),
                b'\r' => "\\r".into(),
                b if byte.is_ascii_graphic() => (byte as char).to_string(),
                _ => format!("\\x{:02x}", byte),
            }
        }

        match option {
            "delimiter" => Some(format!("{:?}", self.delimiter as char)),
            "terminator" => Some(self.terminator.map(|b| char_repr(b)).unwrap_or("".into())),
            "quote" => Some(self.quote.map(|b| char_repr(b)).unwrap_or("".into())),
            "escape" => Some(self.escape.map(|b| char_repr(b)).unwrap_or("".into())),
            "double_quote" => Some(self.double_quote.to_string()),
            "comment" => Some(self.comment.map(|b| char_repr(b)).unwrap_or("".into())),
            "skip" => Some(self.skip.to_string()),
            "columns" => Some(self.columns.as_ref().map(|cols| cols.join(",")).unwrap_or_else(|| "".into())),
            _ => None,
        }
    }

    fn load_schema(&self, _file: Arc<dyn ReadableFile>) -> Pin<Box<dyn Future<Output = Result<EntitySchema, super::ImportError>> + Send + '_>> {
        Box::pin(future::ready(Err(ImportError::SchemaMismatch("Schema must currently be specified for CSV".into()))))
    }

    fn import(&self, file: Arc<dyn ReadableFile>, schema: Option<EntitySchema>, _pool: Arc<Pool>) -> Pin<Box<dyn Future<Output = Result<(EntityStream, Pin<Box<dyn Future<Output = Result<(), ImportError>> + Send>>), ImportError>> + Send + '_>> {
        Box::pin(async {
            let schema = schema.ok_or_else(|| ImportError::SchemaMismatch("Schema must be specified for CSV import".into()))?;
            let file_stream = file.stream(0, None).await?;
            let (entity, completion) = self.load(file_stream, schema).await?;
            Ok((entity, Box::pin(completion) as Pin<Box<_>>))
        })
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
    fn new(source: Pin<Box<dyn AsyncBufRead + Send>>, reader: csv_core::Reader) -> Self {
        CsvParser {
            source,
            reader,
            out: vec![0; 1024],
            ends: vec![0; 128],
        }
    }

    async fn skip_records(&mut self, skip: u64) -> Result<(), ImportError> {
        struct SkipHandler { skip: u64 }
        impl CsvHandler for SkipHandler {
            fn should_continue(&self, _line: u64) -> bool {
                self.skip > 0
            }
            fn row(&mut self, _: Row<'_>) -> Result<(), ImportError> {
                self.skip -= 1;
                Ok(())
            }
        }
        self.read_rows(SkipHandler { skip }).await
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

#[test]
fn test_csv() {
    use std::str::FromStr;
    use crate::schema::{Field, FieldKind, Ignored, attribute::core::{TIME_EPOCH, TIME_RATE}};
    use jiff::Timestamp;
    use futures_lite::{future::block_on, io::Cursor};

    let file = Box::pin(Cursor::new(b"timestamp,value,str\n2025-01-01T00:00:01Z,1.0,abc\n2025-01-01T00:00:01.100Z,2.0,defg\n"));

    let importer = CsvImporter::csv();

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

    let (entity, completion) = block_on(importer.load(file, schema)).unwrap();
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
