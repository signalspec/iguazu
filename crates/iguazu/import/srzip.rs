use std::collections::BTreeMap;
use std::{pin::Pin, sync::Arc};
use ecow::{EcoString, eco_format};

use crate::ElementSize;
use crate::schema::{Entity, EntityStream, Field, FieldKind, Ignored, attribute};
use crate::storage::srzip::SrZipStream;
use crate::stream::Stream;
use crate::{io::{ReadableFile, zip::{load_zip_file, ZipEntry}}, schema::EntitySchema, storage::Pool};

use super::{ImportError, Importer};

/// Importer for Sigrok files
pub struct SrZipImporter {}

impl SrZipImporter {
    pub fn new() -> Self { Self {} }
}

impl Importer for SrZipImporter {
    fn load_schema(&self, file: Arc<dyn ReadableFile>) -> Pin<Box<dyn Future<Output = Result<EntitySchema, ImportError>> + Send>> {
        Box::pin(async move {
            let (_, metadata) = load_metadata(file).await?;
            Ok(make_entity(&metadata, &mut |_, _| Ignored))
        })
    }

    fn import(&self, file: Arc<dyn ReadableFile>, _schema: Option<EntitySchema>, pool: Arc<Pool>) -> Pin<Box<dyn Future<Output = Result<(EntityStream, Pin<Box<dyn Future<Output = Result<(), ImportError>> + Send>>), ImportError>> + Send + '_>> {
        Box::pin(async move {
            let (mut zip_entries, metadata) = load_metadata(file).await?;
            let entity = make_entity(&metadata, &mut |element_size, entry_prefix| {
                let entries = find_chunks(&mut zip_entries, entry_prefix);
                Arc::new(SrZipStream::new(pool.clone(), entries, element_size)) as Arc<dyn Stream>
            });
            Ok((entity, Box::pin(async move {Ok(())}) as Pin<Box<_>>))
        })
    }
}

async fn load_metadata(file: Arc<dyn ReadableFile>) -> Result<(BTreeMap<Box<[u8]>, ZipEntry>, Vec<Metadata>), ImportError> {
    let entries = load_zip_file(file.clone()).await
        .map_err(|e| ImportError::InvalidFile(e.to_string()))?;

    if log::log_enabled!(log::Level::Debug) {
        log::debug!("{} contents:", file.filename().unwrap_or("<unknown>"));
        for (name, entry) in &entries {
            log::debug!("  {:?} ({} bytes)", String::from_utf8_lossy(name), entry.uncompressed_size());
        }
    }

    let version = entries.get(&b"version"[..])
        .ok_or_else(|| ImportError::InvalidFile("missing `version` file in srzip".into()))?
        .read_all(256).await?;

    if version.trim_ascii() != b"2" {
        return Err(ImportError::InvalidFile(format!("unsupported srzip version `{}`", String::from_utf8_lossy(&version))));
    }

    let metadata = entries.get(&b"metadata"[..])
        .ok_or_else(|| ImportError::InvalidFile("missing `metadata` file in srzip".into()))?
        .read_all(64 * 1024).await?;

    let metadata = parse_metadata(&metadata)?;

    Ok((entries, metadata))
}

fn make_entity<D, S: Default>(
    devices: &[Metadata],
    make_stream: &mut impl FnMut(ElementSize, &[u8]) -> D
) -> Entity<D, S> {
    if devices.len() == 1 {
        make_device_entity(&devices[0], make_stream)
    } else {
        devices.iter().fold(Entity::group(), |group, device| {
            let entity = make_device_entity(device, make_stream);
            group.with_child(eco_format!("device{}", device.device_id), entity)
        })
    }
}

fn make_device_entity<D, S: Default>(
    device: &Metadata,
    make_stream: &mut impl FnMut(ElementSize, &[u8]) -> D
) -> Entity<D, S> {
    if device.analog_channels.is_empty() {
        make_digital_entity(device, make_stream)
    } else {
        let mut group = Entity::group();

        if !device.digital_channels.is_empty() {
            let digital = make_digital_entity(device, make_stream);
            group = group.with_child("digital".into(), digital);
        }

        for (&id, name) in &device.analog_channels {
            let channel = make_analog_entity(device, id, make_stream);
            group = group.with_child(name.into(), channel);
        }

        group
    }
}

fn make_digital_entity<D, S: Default>(
    device: &Metadata,
    make_stream: &mut impl FnMut(ElementSize, &[u8]) -> D
) -> Entity<D, S> {
    let children = device.digital_channels.iter().map(|(id, name)| {
        let pos = id.saturating_sub(1);
        let field = Field::new(FieldKind::Bits { pos, bits: 1 })
            .with_attribute(attribute::display::ACCENT_COLOR, attribute::display::AccentColor::from_bit_position(pos));
        (EcoString::from(name), field)
    }).collect();

    let field = Field::new(FieldKind::BitStruct { children })
        .with_attribute_opt(attribute::core::SAMPLE_RATE, device.sample_rate);

    let element_size = ElementSize::from_bytes(device.unitsize).unwrap_or(ElementSize::U8);
    let data = make_stream(element_size, &device.capturefile);
    Entity::Data { field, data, summaries: Default::default() }
}

fn make_analog_entity<D, S: Default>(
    device: &Metadata,
    id: u8,
    make_stream: &mut impl FnMut(ElementSize, &[u8]) -> D
) -> Entity<D, S> {
    let field = Field::new(FieldKind::Float32 { pos: 0 })
        .with_attribute_opt(attribute::core::SAMPLE_RATE, device.sample_rate);

    let data = make_stream(ElementSize::U32, format!("analog-{id}").as_bytes());
    Entity::Data { field, data, summaries: Default::default() }
}

struct Metadata {
    device_id: u8,
    sample_rate: Option<f64>,
    capturefile: Box<[u8]>,
    digital_channels: BTreeMap<u8, String>,
    analog_channels: BTreeMap<u8, String>,
    unitsize: u8,
}

fn parse_metadata(data: &[u8]) -> Result<Vec<Metadata>, ImportError> {
    use lexical_core::parse;

    let mut devices = Vec::new();
    let mut device = None;
    for line in data.split(|&b| b == b'\n') {
        let line = line.trim_ascii();

        if let Some(h) = line.strip_prefix(b"[") && let Some(header) = h.strip_suffix(b"]") {
            devices.extend(device.take());
            if let Some(id) = header.strip_prefix(b"device ") && let Ok(device_id) = parse(id) {
                device = Some(Metadata {
                    device_id,
                    sample_rate: None,
                    capturefile: Box::new([]),
                    digital_channels: BTreeMap::new(),
                    analog_channels: BTreeMap::new(),
                    unitsize: 0,
                });
            }
        } else if let Some(sep) = line.iter().position(|&b| b == b'=') {
            let key = line[..sep].trim_ascii_end();
            let value = line[sep+1..].trim_ascii_start();

            if let Some(device) = device.as_mut() {
                if key == b"samplerate" {
                    device.sample_rate = Some(parse_samplerate(value)
                        .ok_or_else(|| ImportError::InvalidFile(format!("invalid `samplerate` value `{}`", String::from_utf8_lossy(value))))?
                    );
                } else if key == b"unitsize" {
                    device.unitsize = parse(value)
                        .map_err(|_| ImportError::InvalidFile(format!("invalid `unitsize` value `{}`", String::from_utf8_lossy(value))))?;
                } else if key == b"capturefile" {
                    device.capturefile = value.into();
                } else if let Some(rest) = key.strip_prefix(b"probe") && let Ok(channel) = parse(rest) {
                    device.digital_channels.insert(channel, String::from_utf8_lossy(value).into_owned());
                } else if let Some(rest) = key.strip_prefix(b"analog") && let Ok(channel) = parse(rest) {
                    device.analog_channels.insert(channel, String::from_utf8_lossy(value).into_owned());
                }
            }
        }
    }
    devices.extend(device.take());
    Ok(devices)
}

fn parse_samplerate(value: &[u8]) -> Option<f64> {
    let (num, pos) = lexical_core::parse_partial::<f64>(value).ok()?;
    match value[pos..].trim_ascii_start() {
        b"GHz" => Some(num * 1e9),
        b"MHz" => Some(num * 1e6),
        b"kHz" => Some(num * 1e3),
        b"Hz" => Some(num),
        _ => None,
    }
}

#[test]
fn test_parse_metadata() {
    let data = b"
    [global]
    sigrok version=0.6.0

    [device 1]
    capturefile=logic-1
    total probes=8
    samplerate=12 MHz
    total analog=1
    probe1=D0
    probe2=D1
    probe8=D7
    analog9=A0
    unitsize=1
    ";

    let devices = parse_metadata(data).unwrap();

    assert_eq!(devices.len(), 1);
    let device = &devices[0];
    assert_eq!(device.device_id, 1);
    assert_eq!(device.sample_rate, Some(12e6));
    assert_eq!(device.unitsize, 1);
    assert_eq!(device.digital_channels, BTreeMap::from_iter([(1, "D0".into()), (2, "D1".into()), (8, "D7".into())]));
    assert_eq!(device.analog_channels, BTreeMap::from_iter([(9, "A0".into())]));
}

fn find_chunks(zip_entries: &mut BTreeMap<Box<[u8]>, ZipEntry>, prefix: &[u8]) -> Vec<ZipEntry> {
    if let Some(single) = zip_entries.remove(prefix) {
        vec![single]
    } else {
        let mut chunks: Vec<ZipEntry> = zip_entries
            .extract_if(.., |name, _| name.strip_prefix(prefix)
                .and_then(|r| r.strip_prefix(b"-"))
                .is_some_and(|r| r.len() < 10 && r.iter().all(|b| b.is_ascii_digit())))
            .map(|(_, entry)| entry)
            .collect();

        // Sort by chunk number as int, because BTreeMap keys are lexicographically ordered
        chunks.sort_by_key(|entry| {
            lexical_core::parse::<u32>(&entry.name()[prefix.len() + 1..]).unwrap()
        });

        chunks
    }
}

#[cfg(all(test, feature = "fs"))]
mod tests {
    use super::*;
    use std::sync::Arc;
    use futures_lite::future::block_on;
    use crate::io::FsFile;
    use crate::import::Importer;
    use crate::storage::Pool;

    #[test]
    fn test_srzip_example() {
        use crate::schema::{Entity, FieldKind};
        use crate::schema::attribute::core::SAMPLE_RATE;

        let importer = SrZipImporter::new();

        let file: Arc<dyn crate::io::ReadableFile> = Arc::new(
            block_on(FsFile::open("../../test-data/i2c.sr".into())).expect("failed to open test file")
        );

        // Test `load_schema`
        let schema = block_on(importer.load_schema(file.clone())).expect("failed to load schema");
        check_schema(schema);

        fn check_schema<D, S>(schema: Entity<D, S>) -> D {
            let Entity::Data { field: ref schema_field, data, .. } = schema else {
                panic!("expected `Data` entity in schema");
            };
            let FieldKind::BitStruct { ref children } = schema_field.kind else {
                panic!("expected `BitStruct` field kind");
            };
            assert_eq!(children.len(), 2);
            assert!(matches!(children.get("sda").unwrap().kind, FieldKind::Bits { pos: 0, bits: 1 }));
            assert!(matches!(children.get("scl").unwrap().kind, FieldKind::Bits { pos: 1, bits: 1 }));
            assert_eq!(schema_field.attribute(SAMPLE_RATE), Some(8_000_000.0));
            data
        }

        // Test `import`
        let executor = Arc::new(async_executor::Executor::new());
        let pool = Arc::new(Pool::new(executor.clone(), 8 * 1024 * 1024));
        let (entity, completion) = block_on(importer.import(file, None, pool)).expect("failed to import");
        block_on(completion).expect("import completion failed");

        let data = check_schema(entity);

        // Stream metadata
        let state = data.state();
        assert_eq!(state.end, 13348017);
        assert!(!state.streaming);
        assert_eq!(data.desc().element_size, crate::ElementSize::U8);
        assert_eq!(data.desc().count, 4096*1024);

        // Read samples via iter
        let mut iter = block_on(data.clone().iter()).unwrap();
        let iter_data = block_on(iter.read_to_vec(5000*1024)).unwrap();

        assert_eq!(iter_data[0], 0xFF);
        assert_eq!(iter_data[925444], 0xFF);
        assert_eq!(iter_data[925445], 0xFE);
        assert_eq!(iter_data[4834824], 0xFF);
        assert_eq!(iter_data[4834825], 0xFE);
    }
}
