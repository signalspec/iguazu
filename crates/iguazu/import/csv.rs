use std::iter;
use std::{pin::Pin, sync::Arc};
use std::future;

use csv_core::ReadRecordResult;
use futures_lite::{AsyncBufRead, AsyncBufReadExt};
use indexmap::IndexMap;

use crate::schema::{Entity, EntityKind, EntityStream};
use crate::storage::MemoryStreamWriter;
use crate::{io::ReadableFile, schema::EntitySchema, storage::MemoryStream};

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
            let (entity, completion) = load(file_stream, self.opts, schema)?;
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
    pub header_row: u64,
    pub start_row: u64,
}

impl Default for CsvOptions {
    fn default() -> Self {
        CsvOptions {
            delimiter: b',',
            header_row: 1,
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
    Float32(MemoryStreamWriter),
}

impl ColumnParser {
    fn parse(&mut self, value: &[u8]) -> Result<(), &'static str> {
        match self {
            ColumnParser::Float32(writer) => {
                let f = lexical_core::parse::<f32>(value)
                    .map_err(|_| "float")?;
                writer.extend_from_slice(&f.to_ne_bytes());
                Ok(())
            }
        }
    }
}

fn column_parsers(schema: &EntitySchema, parsers: &mut Vec<ColumnParser>) -> Result<EntityStream, ImportError>{
    Ok(match schema.kind {
        EntityKind::Group => {
            return Err(ImportError::SchemaMismatch("Groups are not supported in CSV".into()));
        }

        EntityKind::Record => {
            let children: IndexMap<_, _> = schema.children.iter()
                .map(|(name, child)| {
                    column_parsers(child, parsers).map(|child_stream| (name.clone(), child_stream))
                })
                .collect::<Result<_, ImportError>>()?;

            Entity { 
                data: MemoryStream::null(),
                kind: EntityKind::Record,
                attributes: schema.attributes.clone(),
                children,
            }
        }

        EntityKind::Float { bits: 32 } => {
            let writer = MemoryStreamWriter::new(crate::stream::ElementSize::U32);
            let data = writer.stream().clone();

            parsers.push(ColumnParser::Float32(writer));

            Entity { 
                data,
                kind: EntityKind::Float { bits: 32 },
                attributes: schema.attributes.clone(),
                children: IndexMap::new(),
            }
        }

        _ => {
            return Err(ImportError::SchemaMismatch("Field type not supported in CSV".into()));
        }
    })
}


pub fn load(mut stream: Pin<Box<dyn AsyncBufRead + Send>>, opts: CsvOptions, schema: EntitySchema) -> Result<(EntityStream, impl Future<Output = Result<(), ImportError>> + ), ImportError> {
    let mut parsers = Vec::new();
    let entity = column_parsers(&schema, &mut parsers)?;

    Ok((entity, async move {
        let mut reader = opts.reader();
        let mut fields = vec![0; 1024];
        let mut ends: Vec<usize> = vec![0; parsers.len()];

        let mut input = &[][..];
        let mut consumed = 0;
        let mut outlen = 0;
        let mut endlen = 0;

        loop {
            if input.is_empty() {
                stream.as_mut().consume(consumed);
                consumed = 0;
                input = stream.fill_buf().await.map_err(ImportError::Io)?;
            }

            let (res, nin, nout, nend) = reader.read_record(
                &input,
                &mut fields[outlen..],
                &mut ends[endlen..],
            );

            input = &input[nin..];
            consumed += nin;
            outlen += nout;
            endlen += nend;

            match res {
                ReadRecordResult::InputEmpty => continue,
                ReadRecordResult::OutputFull => {
                    let len = fields.len();
                    fields.resize(len * 2, 0);
                }
                ReadRecordResult::OutputEndsFull => {
                    let len = ends.len();
                    ends.resize(len * 2, 0);
                }
                ReadRecordResult::Record => {
                    if reader.line() >= opts.start_row {
                        if endlen != parsers.len() {
                            return Err(ImportError::InvalidFile(format!("Expected {} fields, but got {} on line {}", parsers.len(), endlen, reader.line())));
                        }

                        let starts = iter::once(0).chain(ends[..endlen].iter().copied());
                        let ends = ends[..endlen].iter().copied();
                        let fields = starts.zip(ends).map(|(start, end)| &fields[start..end]);

                        for (value, parser) in fields.zip(parsers.iter_mut()) {
                            parser.parse(value)
                                .map_err(|e| ImportError::InvalidFile(format!("Failed to parse value {:?} as {} on line {}", String::from_utf8_lossy(value), e, reader.line())))?;
                        }
                    }

                    outlen = 0;
                    endlen = 0;
                }
                ReadRecordResult::End => {
                    return Ok(());
                }
            }
        }
    }))
}

