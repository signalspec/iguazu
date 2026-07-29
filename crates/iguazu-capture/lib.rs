use std::{pin::Pin, sync::Arc};
use async_executor::Task;
use iguazu::send::Executor;
use iguazu::{send::{BoxFuture, NonWasmSync, NonWasmSend}, config::Configurable, schema::EntityStream, storage::Storage};
use nusb::{DeviceInfo, DeviceSelector};

pub mod drivers;
pub use drivers::DRIVERS;

pub struct Driver {
    /// Driver name
    pub name: &'static str,

    pub description: &'static str,

    pub probe: ProbeFunc,
}

pub enum ProbeFunc {
    /// Called for every matching USB device.
    Usb(&'static [DeviceSelector], fn(&nusb::DeviceInfo) -> Option<Arc<dyn Device>>),

    /// Called once to list available devices.
    Static(fn() -> BoxFuture<'static, Vec<Arc<dyn Device>>>),

    /// Create a device for an explicitly specified identifier.
    ///
    /// e.g. for a remote device, where the identifier is the address
    Explicit(fn(&str) -> Pin<Box<dyn Future<Output = Result<Arc<dyn Device>, String>>>>),
}

pub trait Device: NonWasmSend + NonWasmSync {
    fn driver(&self) -> &'static Driver;

    /// Unique identifier for this device, such as a serial number or port path.
    ///
    /// It should try to be as stable as possible for the same physical hardware.
    fn identifier(&self) -> &str;

    /// Label for the device, such as the model.
    ///
    /// It should not include the identifier.
    fn description(&self) -> &str;

    /// Device info if this device is USB.
    fn usb_device(&self) -> Option<&DeviceInfo>;

    /// Open the device
    fn open(self: Arc<Self>) -> BoxFuture<'static, Result<Box<dyn DeviceSession>, CaptureError>>;
}

#[derive(Debug, Clone)]
pub struct CaptureError {
    msg: String,
}

impl CaptureError {
    pub fn new(msg: String) -> Self {
        Self { msg }
    }
}

impl std::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.msg)
    }
}

impl std::error::Error for CaptureError {}

impl From<std::io::Error> for CaptureError {
    fn from(e: std::io::Error) -> Self {
        CaptureError { msg: e.to_string() }
    }
}

impl From<nusb::Error> for CaptureError {
    fn from(e: nusb::Error) -> Self {
        CaptureError { msg: e.to_string() }
    }
}

pub trait DeviceSession: Configurable + NonWasmSend {
    fn device(&self) -> &Arc<dyn Device>;

    fn config_summary(&self) -> Option<String>;

    fn start_capture(&self, executor: &Arc<Executor<'static>>, storage: &Arc<dyn Storage>) -> Result<(EntityStream, Task<Result<(), CaptureError>>), CaptureError>;
}

impl Driver {
    pub async fn device_by_id(&self, id: &str) -> Result<Arc<dyn Device>, String> {
        match self.probe {
            ProbeFunc::Usb(selectors, probe) => {
                nusb::list_devices().await.map_err(|e| e.to_string())?
                    .filter(|d| selectors.iter().any(|s| d.matches(s)))
                    .filter_map(|d| probe(&d))
                    .find(|d| d.identifier() == id)
                    .ok_or_else(|| "device not found".to_string())
            }
            ProbeFunc::Static(probe) => probe().await.into_iter().find(|d| d.identifier() == id).ok_or_else(|| "device not found".to_string()),
            ProbeFunc::Explicit(probe) => probe(id).await,
        }
    }
}

/// A wrapper around `&[Driver]` that provides methods for finding and using drivers
#[derive(Clone, Copy)]
pub struct Drivers<'a>(pub &'a [Driver]);

impl<'a> Drivers<'a> {
    pub fn iter(&self) -> std::slice::Iter<'a, Driver> {
        self.0.iter()
    }

    pub fn by_name(&self, name: &str) -> Option<&'a Driver> {
        self.iter().find(|d| d.name.eq_ignore_ascii_case(name))
    }

    pub async fn list_devices(&self) -> Vec<Arc<dyn Device>> {
        let usb_list = nusb::list_devices().await.map_or(Vec::new(), |l| l.collect());
        let mut devices = Vec::new();

        for driver in self.iter() {
            match driver.probe {
                ProbeFunc::Usb(selectors, probe) => {
                    devices.extend(
                        usb_list.iter()
                            .filter(|d| selectors.iter().any(|s| d.matches(s)))
                            .filter_map(|d| probe(d))
                    )
                }
                ProbeFunc::Static(probe) => {
                    devices.extend(probe().await);
                }
                ProbeFunc::Explicit(_) => {}
            }
        }

        devices
    }

    pub async fn device_by_driver_and_id(&self, id: &str) -> Result<Arc<dyn Device>, String> {
        let (driver, id) = id.split_once(":").ok_or_else(|| "device ID must include a driver and a device identifier".to_string())?;
        let driver = self.by_name(driver).ok_or_else(|| format!("unknown driver `{driver}`"))?;
        driver.device_by_id(id).await
    }
}
