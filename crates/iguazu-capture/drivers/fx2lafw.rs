use std::{borrow::Cow, fmt::{Display, Write}, hash::{DefaultHasher, Hash, Hasher}, sync::Arc};
use async_executor::Task;
use iguazu::send::Executor;
use ecow::{EcoString, eco_format};
#[allow(unused_imports)] use itertools::Itertools;
use nusb::DeviceInfo;
use fx2la::SampleRate;
use iguazu::{
    send::BoxFuture,
    cow_strs,
    ElementSize,
    config::{Configurable, OptionDescription, format_rate, format_time, parse_optional, parse_rate, parse_time},
    schema::{Entity, EntityStream, Field, FieldKind, attribute},
    storage::Storage,
    stream::ArcStream
};

use crate::{CaptureError, Device, DeviceSession, Driver, ProbeFunc};

pub const DRIVER: Driver = Driver {
    name: "fx2lafw",
    description: "Cypress FX2 based logic analyzers with the fx2lafw firmware",
    probe: ProbeFunc::Usb(fx2la::SELECTORS, |d| Fx2lafwDevice::new(d.clone()).map(|d| Arc::new(d) as Arc<dyn Device>)),
};

impl From<fx2la::InitError> for CaptureError {
    fn from(e: fx2la::InitError) -> Self {
        CaptureError { msg: e.to_string() }
    }
}

impl From<fx2la::CaptureError> for CaptureError {
    fn from(e: fx2la::CaptureError) -> Self {
        CaptureError { msg: e.to_string() }
    }
}

pub struct Fx2lafwDevice {
    usb: DeviceInfo,
    id: String,
    model: &'static fx2la::Model,
}

#[allow(unused)]
fn hash_str(t: impl Hash) -> impl Display {
    let mut hasher = DefaultHasher::new();
    t.hash(&mut hasher);

    struct Hex(u64);

    impl Display for Hex {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{:08x}", self.0)
        }
    }

    Hex(hasher.finish())
}

impl Fx2lafwDevice {
    pub fn new(usb: DeviceInfo) -> Option<Self> {
        let model = fx2la::MODELS.iter().find(|m| m.matches(&usb))?;

        // FX2 devices don't usually have a serial number, so port position is the best we have.
        let id = cfg_select! {
            all(target_arch = "wasm32", target_os = "unknown", target_env = "") => format!("{}", hash_str(usb.id())),
            windows => format!("{}.{}", hash_str(usb.bus_id()), usb.port_chain().iter().format(".")),
            _ => format!("{}.{}", usb.bus_id(), usb.port_chain().iter().format(".")),
        };

        Some(Self { usb, id, model })
    }
}

impl Device for Fx2lafwDevice {
    fn driver(&self) -> &'static Driver {
        &DRIVER
    }

    fn identifier(&self) -> &str {
        &self.id
    }

    fn description(&self) -> &str {
        self.model.description
    }

    fn usb_device(&self) -> Option<&DeviceInfo> {
        Some(&self.usb)
    }

    fn open(self: Arc<Self>) -> BoxFuture<'static, Result<Box<dyn DeviceSession>, CaptureError>> {
        Box::pin(async move {
            let firmware_provider = cfg_select! {
                any(windows, unix) => fx2la::DefaultFirmwareProvider,
                all(target_arch = "wasm32", target_os = "unknown", target_env = "") =>
                    fx2la::FetchFirmwareProvider::new("https://media.signalspec.org/fx2lafw/".into()),
            };

            let fx2la = Arc::new(fx2la::Device::from_nusb(self.usb.clone(), firmware_provider).await?);
            Ok(Box::new(Fx2lafwSession {
                device: self,
                fx2la,
                channel_names: (0..8).map(|i| eco_format!("D{}", i)).collect(),
                rate: SampleRate::from_hz(1e6),
                time: None,
            }) as Box<dyn DeviceSession>)
        })
    }
}

struct Fx2lafwSession {
    device: Arc<dyn Device>,
    fx2la: Arc<fx2la::Device>,
    channel_names: Vec<EcoString>,
    rate: SampleRate,
    time: Option<f64>,
}

impl DeviceSession for Fx2lafwSession {
    fn device(&self) -> &Arc<dyn Device> {
        &self.device
    }

    fn config_summary(&self) -> Option<String> {
        let mut summary = format_rate(self.rate.as_hz() as f64);
        if let Some(time) = self.time {
            write!(&mut summary, " for {time}s").unwrap();
        }
        Some(summary)
    }

    fn start_capture(&self, executor: &Arc<Executor<'static>>, storage: &Arc<dyn Storage>) -> Result<(EntityStream, Task<Result<(), CaptureError>>), CaptureError> {
        let mut writer = storage.create_stream(ElementSize::U8);
        let data = writer.stream().clone() as ArcStream;
        let fields = self.channel_names.iter().enumerate().filter(|(_, name)| !name.is_empty()).map(|(i, name)| {
            let field = Field::new(FieldKind::Bits { pos: i as u8, bits: 1 })
                .with_attribute(attribute::display::COLOR, attribute::display::AccentColor::from_bit_position(i as u8));
            (name.clone(), field)
        }).collect();
        let field = Field::new(FieldKind::BitStruct { children: fields })
            .with_attribute(attribute::core::TIME_RATE, self.rate.as_hz() as f64)
            .with_attribute_opt(attribute::core::CAPTURE_LENGTH, self.time);
        let entity = Entity::Data { data, field, summaries: Default::default() };

        let rate = self.rate;
        let sample_limit = self.time.map(|t| (t * self.rate.as_hz() as f64).ceil() as u64);
        let fx2la = self.fx2la.clone();

        let task = executor.spawn(async move {
            let mut capture = fx2la.start_capture(rate).await?;

            if let Some(samples) = sample_limit {
                capture.limit_remaining_samples(samples);
            }

            loop {
                let data = capture.read().await?;

                if data.is_empty() {
                    break;
                }

                log::trace!("writing {} samples", data.len());
                writer.extend_from_slice(&data).await.map_err(CaptureError::new)?;
                writer.commit();
            }

            Ok(())
        });

        Ok((entity, task))
    }
}

impl Configurable for Fx2lafwSession {
    fn options(&self) -> &'static [ iguazu::config::OptionDescription ] {
        &[
            OptionDescription {
                name: "rate",
                description: "Sample rate",
            },
            OptionDescription {
                name: "time",
                description: "Limit sampling time",
            },
            OptionDescription {
                name: "channels",
                description: "List of channel names",
            },
        ]
    }

    fn set(&mut self, option: &str, value: &str) -> Result<(), String> {
        match option {
            "rate" => self.rate = SampleRate::from_hz(parse_rate(value)? as f32),
            "time" => self.time = parse_optional(parse_time, value)?,
            "channels" => self.channel_names = value.split(',').map(|s| EcoString::from(s.trim())).collect(),
            _ => return Err(format!("Unknown option")),
        }
        Ok(())
    }

    fn get(&self, option: &str) -> Option<String> {
        match option {
            "rate" => Some(format_rate(self.rate.as_hz() as f64)),
            "time" => Some(self.time.map_or("".to_string(), format_time)),
            "channels" => Some(self.channel_names.join(",")),
            _ => None,
        }
    }

    fn get_suggestions(&self, option: &str) -> Cow<'_, [Cow<'_, str>]> {
        match option {
            "rate" => cow_strs!["100 kHz", "500 kHz", "1 MHz", "2 MHz", "4 MHz", "8 MHz", "12 MHz", "16 MHz", "24 MHz", "30 MHz"],
            _ => (&[]).into(),
        }
    }
}
