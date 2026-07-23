use std::{pin::Pin, sync::Arc};
use std::future;
use crate::ElementSize;
use crate::import::OptionDescription;
use crate::schema::{EntityStream, Field};
use crate::{io::ReadableFile, schema::{EntitySchema, attribute}, storage::{Pool, FlatFileOpts, FlatFileStream}};

use super::{ImportError, Importer};

/// Importer for flat files that are directly read from disk.
#[non_exhaustive]
#[derive(Clone)]
pub struct FlatFileImporter {
    pub file: Arc<dyn ReadableFile>,
    pub element: ElementSize,
    pub dtype: DataType,
    pub sample_rate: Option<f64>,
    pub opts: FlatFileOpts,
}

#[derive(Clone, Copy, Debug)]
pub enum DataType {
    Binary,
    Logic,
    Real(ScalarType),
    Complex(ScalarType),
}

#[derive(Clone, Copy, Debug)]
pub enum ScalarType {
    Unsigned,
    Signed,
    Float,
}

impl DataType {
    fn from_str(s: &str) -> Option<Self> {
        match s.trim() {
            "b" | "binary" => Some(DataType::Binary),
            "l" | "logic" => Some(DataType::Logic),
            "u" | "unsigned" => Some(DataType::Real(ScalarType::Unsigned)),
            "s" | "signed" => Some(DataType::Real(ScalarType::Signed)),
            "f" | "float" => Some(DataType::Real(ScalarType::Float)),
            "cu" | "complex_unsigned" => Some(DataType::Complex(ScalarType::Unsigned)),
            "cs" | "complex_signed" => Some(DataType::Complex(ScalarType::Signed)),
            "cf" | "complex_float" => Some(DataType::Complex(ScalarType::Float)),
            _ => None,

        }
    }

    fn to_str(&self) -> &'static str {
        match self {
            DataType::Binary => "binary",
            DataType::Logic => "logic",
            DataType::Real(ScalarType::Unsigned) => "unsigned",
            DataType::Real(ScalarType::Signed) => "signed",
            DataType::Real(ScalarType::Float) => "float",
            DataType::Complex(ScalarType::Unsigned) => "complex_unsigned",
            DataType::Complex(ScalarType::Signed) => "complex_signed",
            DataType::Complex(ScalarType::Float) => "complex_float",
        }
    }

    fn schema(&self, element: ElementSize) -> Option<EntitySchema> {
        Some(match self {
            DataType::Binary => EntitySchema::field(Field::bits(element.bits() as u8)),
            DataType::Logic => EntitySchema::field(Field::logic(element.bits() as u8)),
            DataType::Real(scalar) => EntitySchema::field(scalar.field(element)?),
            DataType::Complex(scalar) => EntitySchema::complex(scalar.field(element)?),
        })
    }
}

impl ScalarType {
    fn field(&self, element: ElementSize) -> Option<Field> {
        Some(match self {
            ScalarType::Unsigned => Field::unsigned(element.bits() as u8),
            ScalarType::Signed => Field::signed(element.bits() as u8),
            ScalarType::Float => Field::float(element.bits() as u8)?,
        })
    }
}

impl FlatFileImporter {
    pub fn new(file: Arc<dyn ReadableFile>, element: ElementSize, dtype: DataType) -> Self {
        Self {
            file,
            element,
            dtype,
            sample_rate: None,
            opts: FlatFileOpts::default(),
        }
    }

    pub fn for_file(file: Arc<dyn ReadableFile>,) -> Self {
        let (element, dtype) = match file.filename().unwrap_or("").rsplit('.').next().unwrap_or("") {
            "logic8" => (ElementSize::U8, DataType::Logic),
            "f32" => (ElementSize::U32, DataType::Real(ScalarType::Float)),
            "cf32" | "cfile "=> (ElementSize::U32, DataType::Complex(ScalarType::Float)),
            "u8" => (ElementSize::U8, DataType::Real(ScalarType::Unsigned)),
            "u16" => (ElementSize::U16, DataType::Real(ScalarType::Unsigned)),
            "u32" => (ElementSize::U32, DataType::Real(ScalarType::Unsigned)),
            "u64" => (ElementSize::U64, DataType::Real(ScalarType::Unsigned)),
            "s8" => (ElementSize::U8, DataType::Real(ScalarType::Signed)),
            "s16" => (ElementSize::U16, DataType::Real(ScalarType::Signed)),
            "s32" => (ElementSize::U32, DataType::Real(ScalarType::Signed)),
            "s64" => (ElementSize::U64, DataType::Real(ScalarType::Signed)),
            "cu8" => (ElementSize::U8, DataType::Complex(ScalarType::Unsigned)),
            "cu16" => (ElementSize::U16, DataType::Complex(ScalarType::Unsigned)),
            "cu32" => (ElementSize::U32, DataType::Complex(ScalarType::Unsigned)),
            "cu64" => (ElementSize::U64, DataType::Complex(ScalarType::Unsigned)),
            "cs8" => (ElementSize::U8, DataType::Complex(ScalarType::Signed)),
            "cs16" => (ElementSize::U16, DataType::Complex(ScalarType::Signed)),
            "cs32" => (ElementSize::U32, DataType::Complex(ScalarType::Signed)),
            "cs64" => (ElementSize::U64, DataType::Complex(ScalarType::Signed)),
            _ => (ElementSize::U8, DataType::Binary),
        };
        Self::new(file, element, dtype)
    }

    pub fn schema(&self) -> Result<EntitySchema, ImportError> {
        let mut schema = self.dtype.schema(self.element).ok_or_else(|| ImportError::SchemaMismatch("Unsupported combination of element and dtype".into()))?;

        if let Some(sample_rate) = self.sample_rate {
            schema = schema.with_attribute(attribute::core::TIME_RATE, sample_rate);
        }

        Ok(schema)
    }
}

impl Importer for FlatFileImporter {
    fn options(&self) -> &'static [ super::OptionDescription ] {
        &[
            OptionDescription {
                name: "bits",
                description: "Element size in bits (8, 16, 32, 64)",
            },
            OptionDescription {
                name: "dtype",
                description: "Data type (binary, logic, real, complex)",
            },
            OptionDescription {
                name: "sample_rate",
                description: "Sample rate",
            },
            OptionDescription {
                name: "offset",
                description: "Byte offset in the file where the data starts.",
            },
            OptionDescription {
                name: "count",
                description: "Number of elements to read from the file. Empty means to read until the end of the file.",
            },
            OptionDescription {
                name: "block_size",
                description: "Number of elements to read in each block.",
            },
        ]
    }

    fn set(&mut self, option: &str, value: &str) -> Result<(), String> {
        match option {
            "bits" => self.element = ElementSize::from_bits_exact(value.parse().map_err(|_| "Invalid integer")?).ok_or_else(|| "Unsupported element size")?,
            "dtype" => self.dtype = DataType::from_str(value).ok_or_else(|| "Unsupported data type")?,
            "offset" => self.opts.offset = value.parse().map_err(|_| "Invalid integer")?,
            "count" => self.opts.count = if value.is_empty() { None } else { Some(value.parse().map_err(|_| "Invalid integer")?) },
            "block_size" => self.opts.block_size = value.parse().map_err(|_| "Invalid integer")?,
            "sample_rate" => self.sample_rate = if value.is_empty() { None } else { Some(value.parse().map_err(|_| "Invalid float")?) },
            _ => return Err("Unknown option".into()),
        }
        Ok(())
    }

    fn get(&self, option: &str) -> Option<String> {
        Some(match option {
            "bits" => self.element.bits().to_string(),
            "dtype" => self.dtype.to_str().to_owned(),
            "offset" => self.opts.offset.to_string(),
            "count" => self.opts.count.map_or_else(|| "".into(), |c| c.to_string()),
            "block_size" => self.opts.block_size.to_string(),
            "sample_rate" => self.sample_rate?.to_string(),
            _ => return None,
        })
    }

    fn load_schema(&self) -> Pin<Box<dyn Future<Output = Result<EntitySchema, ImportError>> + Send>> {
        Box::pin(future::ready(self.schema()))
    }

    fn import(&self, schema: Option<EntitySchema>, pool: Arc<Pool>) -> Pin<Box<dyn Future<Output = Result<(EntityStream, Pin<Box<dyn Future<Output = Result<(), ImportError>> + Send>>), ImportError>> + Send + '_>> {
        Box::pin(async move {
            let schema = if let Some(s) = schema { s } else { self.schema()? };
            let entity = FlatFileStream::entity(self.file.clone(), pool, self.element, schema, &self.opts).await?;
            Ok((entity, Box::pin(async move {Ok(())}) as Pin<Box<_>>))
        })
    }
}
