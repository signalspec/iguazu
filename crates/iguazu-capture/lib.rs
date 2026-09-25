use std::{pin::Pin, sync::Arc, task::{Context, Poll}};
use async_executor::Task;
use futures_lite::{FutureExt, StreamExt};
use nusb::{DeviceInfo, DeviceSelector, hotplug::HotplugEvent};
use iguazu::{send::Executor, util::future::FutureOutput};
use iguazu::{send::{BoxFuture, NonWasmSync, NonWasmSend}, config::Configurable, schema::EntityStream, storage::Storage};

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

    fn probe_usb(&self, dev: &DeviceInfo) -> Option<Arc<dyn Device>> {
        if let ProbeFunc::Usb(sel, probe) = &self.probe && sel.iter().any(|s| dev.matches(s)) {
            probe(dev)
        } else {
            None
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
        let usb_list = nusb::list_devices().await
            .inspect_err(|e| log::error!("Failed to list USB devices: {e}"))
            .map_or(Vec::new(), |l| l.collect());

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

/// A list of matching devices kept updated by USB hotplug events.
pub struct DeviceWatch {
    drivers: Drivers<'static>,
    devices: FutureOutput<Task<Vec<Arc<dyn Device>>>>,
    watch: Result<nusb::hotplug::HotplugWatch, String>,
    request_device_fut: Option<BoxFuture<'static, Option<DeviceInfo>>>,
}

impl DeviceWatch {
    pub fn new(executor: &Arc<Executor<'static>>, drivers: Drivers<'static>) -> Self {
        let watch = nusb::watch_devices().map_err(|e| e.to_string());
        let devices = FutureOutput::Future(executor.spawn(async move { drivers.list_devices().await }));
        Self { drivers, devices, watch, request_device_fut: None }
    }

    pub fn poll(&mut self, cx: &mut Context) -> WatchPollResult {
        self.devices.poll(cx);
        if let Poll::Ready(devices) = self.devices.get_mut() {
            if let Ok(watch) = &mut self.watch {
                match watch.poll_next(cx) {
                    Poll::Ready(Some(HotplugEvent::Connected(dev))) => {
                        devices.extend(self.drivers.iter().flat_map(|driver| driver.probe_usb(&dev)));
                    }
                    Poll::Ready(Some(HotplugEvent::Disconnected(dev_id))) => {
                        devices.retain(|dev| dev.usb_device().is_none_or(|d| d.id() != dev_id));
                    }
                    Poll::Ready(None) => {
                        log::error!("Hotplug watch unexpectedly completed!");
                    }
                    Poll::Pending => {}
                }
            }

            let requested_device = if let Some(f) = &mut self.request_device_fut && let Poll::Ready(res) = f.poll(cx) {
                self.request_device_fut = None;
                res.and_then(|dev| {
                    if let Some(existing) = devices.iter().find(|d| d.usb_device().is_some_and(|d| d.id() == dev.id())) {
                        Some(existing.clone())
                    } else {
                        let new = self.drivers.iter().find_map(|driver| driver.probe_usb(&dev));
                        devices.extend(new.clone());
                        new
                    }
                })
            } else { None };

            WatchPollResult { requested_device }
        } else {
            WatchPollResult { requested_device: None }
        }
    }

    pub fn needs_request_device(&self) -> bool {
        cfg!(target_arch = "wasm32")
    }

    pub fn request_device(&mut self) {
        let selectors = self.drivers.iter().flat_map(|d| {
            if let ProbeFunc::Usb(selectors, _) = &d.probe {
                *selectors
            } else {
                &[]
            }
        }).cloned().collect::<Vec<_>>();

        log::debug!("Requesting device with selectors: {:?}", selectors);

        self.request_device_fut = Some(Box::pin(async move {
            nusb::request_device(&selectors[..]).await
                .inspect_err(|e| log::error!("Failed to request device: {e}"))
                .ok().flatten()
        }));
    }

    pub fn devices(&self) -> impl Iterator<Item = &Arc<dyn Device>> + '_ {
        if let Poll::Ready(devices) = self.devices.get() {
            devices.iter()
        } else {
            [].iter()
        }
    }

    pub fn status(&self) -> Result<(), &str> {
        self.watch.as_ref().map(|_| ()).map_err(|e| e.as_str())
    }
}

pub struct WatchPollResult {
    pub requested_device: Option<Arc<dyn Device>>,
}
