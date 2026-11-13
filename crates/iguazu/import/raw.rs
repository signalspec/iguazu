use std::borrow::Cow;
use std::sync::{Arc, LazyLock};
use std::future;
use async_executor::Task;
use regex::Regex;

use crate::{ElementSize, cow_strs};
use crate::config::{Configurable, OptionDescription, format_rate, parse_optional, parse_rate};
use crate::schema::{EntityStream, Field};
use crate::{io::ReadableFile, schema::{EntitySchema, attribute}, storage::{Pool, RawFileOpts, RawFileStream}, send::BoxFuture};

use super::{ImportError, Importer};

/// Importer for arrays of elements that are directly read from disk.
#[non_exhaustive]
#[derive(Clone)]
pub struct RawImporter {
    pub file: Arc<dyn ReadableFile>,
    pub element: ElementSize,
    pub dtype: DataType,
    pub attributes: Attributes,
    pub opts: RawFileOpts,
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

impl RawImporter {
    pub fn new(file: Arc<dyn ReadableFile>, element: ElementSize, dtype: DataType) -> Self {
        Self {
            file,
            element,
            dtype,
            attributes: Attributes::default(),
            opts: RawFileOpts::default(),
        }
    }

    pub fn for_file(file: Arc<dyn ReadableFile>) -> Self {
        let (element, dtype) = match file.filename().rsplit('.').next().unwrap_or("") {
            "logic8" => (ElementSize::U8, DataType::Logic),
            "f32" => (ElementSize::U32, DataType::Real(ScalarType::Float)),
            "cf32" | "cfile" => (ElementSize::U32, DataType::Complex(ScalarType::Float)),
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
        let attributes = Attributes::from_file_name(file.filename(), jiff::tz::TimeZone::system());
        Self { file, element, dtype, attributes, opts: RawFileOpts::default() }
    }

    pub fn schema(&self) -> Result<EntitySchema, ImportError> {
        Ok(
            self.dtype.schema(self.element)
                .ok_or_else(|| ImportError::SchemaMismatch("Unsupported combination of element and dtype".into()))?
                .with_attribute_opt(attribute::core::TIME_RATE, self.attributes.sample_rate)
                .with_attribute_opt(attribute::core::FREQ_CENTER, self.attributes.center_frequency)
                .with_attribute_opt(attribute::core::TIME_EPOCH, self.attributes.epoch.clone())
        )
    }
}

impl Configurable for RawImporter {
    fn options(&self) -> &'static [ OptionDescription ] {
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
                name: "center_freq",
                description: "Center frequency",
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
            "sample_rate" => self.attributes.sample_rate = parse_optional(parse_rate, value)?,
            "center_freq" => self.attributes.center_frequency = parse_optional(parse_rate, value)?,
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
            "sample_rate" => format_rate(self.attributes.sample_rate?),
            "center_freq" => format_rate(self.attributes.center_frequency?),
            _ => return None,
        })
    }

    fn get_suggestions(&self, option: &str) -> Cow<'_, [std::borrow::Cow<'_, str>]> {
        match option {
            "bits" => cow_strs!["8", "16", "32", "64"],
            "dtype" => cow_strs!["binary", "logic", "unsigned", "signed", "float", "complex_unsigned", "complex_signed", "complex_float"],
            _ => (&[]).into()
        }
    }
}

impl Importer for RawImporter {
    fn load_schema(&self) -> BoxFuture<'static, Result<EntitySchema, ImportError>> {
        Box::pin(future::ready(self.schema()))
    }

    fn import(&self, schema: Option<EntitySchema>, pool: Arc<Pool>) -> BoxFuture<'static, Result<(EntityStream, Task<Result<(), ImportError>>), ImportError>> {
        let file = self.file.clone();
        let element = self.element;
        let opts = self.opts.clone();
        let schema = if let Some(s) = schema { Ok(s) } else { self.schema() };

        Box::pin(async move {
            let schema = schema?;
            let task = pool.executor.spawn(async move { Ok(()) });
            let entity = RawFileStream::entity(file, pool, element, schema, &opts).await?;
            Ok((entity, task))
        })
    }

    fn file(&self) -> &Arc<dyn ReadableFile> {
        &self.file
    }
}

#[derive(Debug, PartialEq, Default, Clone)]
pub struct Attributes {
    pub center_frequency: Option<f64>,
    pub sample_rate: Option<f64>,
    pub epoch: Option<jiff::Zoned>,
}

impl Attributes {
    fn from_file_name(name: &str, default_zone: jiff::tz::TimeZone) -> Attributes {
        use jiff::{tz::TimeZone, fmt::strtime, civil::DateTime};

        static REGEX: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(concat!(
                r"(?:-f(?<freq>\d+(?:[.]\d+)?(?:[eE][-+]?\d+)?))?",
                r"(?:-s(?<rate>\d+(?:[.]\d+)?(?:[eE][-+]?\d+)?))?",
                r"(?:-t(?<date>\d{14}(?:[.]\d+)?)(?<offset>[+-]\d{4})?)?",
                r"(?:\.\w+)?$",
            )).unwrap()
        });

        REGEX.captures(name).map(|captures| {
            let center_frequency = captures.name("freq").and_then(|m| m.as_str().parse().ok());
            let sample_rate = captures.name("rate").and_then(|m| m.as_str().parse().ok());
            let epoch = captures.name("date").and_then(|m| {
                let dt = DateTime::strptime("%Y%m%d%H%M%S%.f", m.as_str()).ok()?;
                let tz = captures.name("offset")
                    .and_then(|m| strtime::parse("%z", m.as_str()).ok()?.offset())
                    .map_or(default_zone, |o| TimeZone::fixed(o));
                dt.to_zoned(tz).ok()
            });

            Attributes { center_frequency, sample_rate, epoch }
        }).unwrap_or_default()
    }
}

#[test]
fn test_infer_attributes_from_name() {
    use jiff::{Timestamp, tz::TimeZone};

    // osmocom_fft, old
    assert_eq!(Attributes::from_file_name("idle-f2.700000e+07-s1.024000e+06-t20171231164913.cfile", TimeZone::UTC), Attributes {
        center_frequency: Some(27.0e6),
        sample_rate: Some(1.024e6),
        epoch: Some("2017-12-31T16:49:13Z".parse::<Timestamp>().unwrap().to_zoned(TimeZone::UTC))
    });
    // osmocom_fft with zone offset
    assert_eq!(Attributes::from_file_name("xxx-yyy-f433900000-s100000-t20261107143035-0700.cf32", TimeZone::UTC), Attributes {
        center_frequency: Some(433.9e6),
        sample_rate: Some(100000.0),
        epoch: Some("2026-11-07T14:30:35-07:00".parse::<Timestamp>().unwrap().to_zoned(TimeZone::unknown()))
    });
    // sample rate only
    assert_eq!(Attributes::from_file_name("test-s1000000.0.f32", TimeZone::UTC), Attributes {
        center_frequency: None,
        sample_rate: Some(1000000.0),
        epoch: None
    });
    // sample rate and epoch only
    assert_eq!(Attributes::from_file_name("test-s1000000-t20261107143035.456+0100.f32", TimeZone::UTC), Attributes {
        center_frequency: None,
        sample_rate: Some(1000000.0),
        epoch: Some("2026-11-07T14:30:35.456+01:00".parse::<Timestamp>().unwrap().to_zoned(TimeZone::unknown()))
    });
}
