use std::{
    collections::{HashMap, HashSet},
    ffi::{CStr, CString},
    ops::ControlFlow,
    sync::{
        Arc, Mutex, Once, OnceLock, PoisonError,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime},
};

use axum::extract::ws::{Message, WebSocket};
use hidapi::{HidApi, HidDevice, MAX_REPORT_DESCRIPTOR_SIZE};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, watch};

const MAX_REPORT_BYTES: usize = MAX_REPORT_DESCRIPTOR_SIZE;
const INPUT_QUEUE_CAPACITY: usize = 64;
const SOCKET_SEND_TIMEOUT: Duration = Duration::from_secs(2);
/// How long an owned-handle reader blocks waiting for input before it checks
/// its stop flag again. It bounds how long closing, unlistening or ending a
/// session waits for the reader threads to join.
#[cfg(target_os = "windows")]
const READ_TIMEOUT_MS: i32 = 100;
#[cfg(target_os = "linux")]
const READ_TIMEOUT_MS: i32 = 50;
/// How long a shared-handle reader waits between empty polls. The wait happens
/// with the device lock released, so a write never queues behind a read.
#[cfg(not(any(target_os = "windows", target_os = "linux")))]
const SHARED_READ_IDLE: std::time::Duration = std::time::Duration::from_millis(2);
const MICROSOFT_VENDOR_ID: u16 = 0x045E;
const RAZER_VENDOR_ID: u16 = 0x1532;
/// FIDO Alliance usage page: security keys (U2F/CTAP). Never offered over the
/// WebSocket, like the browser's own WebHID blocklist, so a page cannot talk
/// to a security key through Bridge.
const FIDO_USAGE_PAGE: u16 = 0xF1D0;
const RAZER_FEATURE_BUFFER_LEN: usize = 91;
const THERMALTAKE_VENDOR_ID: u16 = 0x264A;
/// Vendor-defined usage pages start here.
#[cfg_attr(not(any(target_os = "windows", target_os = "linux")), allow(dead_code))]
const VENDOR_USAGE_PAGE: u16 = 0xff00;
/// The Iris's config collection holds only constant items, so Windows gives it
/// no caps and hidapi's rebuilt descriptor declares none of its 2052-byte
/// feature reports. Reads use this buffer (report id plus 2052 bytes) instead.
const THERMALTAKE_FEATURE_BUFFER_LEN: usize = 2053;
const RESUME_WATCH_STEP: Duration = Duration::from_secs(2);
/// A step that overshoots by this much means the machine was suspended.
const RESUME_GAP_THRESHOLD: Duration = Duration::from_secs(15);
const RESUME_REOPEN_ATTEMPTS: u32 = 8;
const RESUME_REOPEN_DELAY: Duration = Duration::from_secs(1);
static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);
static ACTIVE_SESSIONS: AtomicUsize = AtomicUsize::new(0);
/// Monotonic touch order for open HID devices, so the most recently opened
/// device sorts first without relying on clock resolution.
static ACTIVE_DEVICE_SEQ: AtomicU64 = AtomicU64::new(1);

/// A HID device the control panel currently holds open, as the status view
/// sees it. Device keys are allocated per websocket session (`hid-N`), so the
/// registry is keyed by session: one panel tab re-listing must not clear
/// another tab's open device.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveDeviceInfo {
    pub key: String,
    pub name: String,
    pub vendor_id: u16,
    pub product_id: u16,
}

struct ActiveDeviceEntry {
    info: ActiveDeviceInfo,
    seq: u64,
}

static ACTIVE_DEVICES: OnceLock<Mutex<HashMap<u64, HashMap<String, ActiveDeviceEntry>>>> =
    OnceLock::new();

fn active_device_registry() -> &'static Mutex<HashMap<u64, HashMap<String, ActiveDeviceEntry>>> {
    ACTIVE_DEVICES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn note_device_opened(session_id: u64, summary: &DeviceSummary) {
    let mut registry = active_device_registry()
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    registry.entry(session_id).or_default().insert(
        summary.key.clone(),
        ActiveDeviceEntry {
            info: ActiveDeviceInfo {
                key: summary.key.clone(),
                name: summary.product_name.clone(),
                vendor_id: summary.vendor_id,
                product_id: summary.product_id,
            },
            seq: ACTIVE_DEVICE_SEQ.fetch_add(1, Ordering::Relaxed),
        },
    );
}

fn note_device_closed(session_id: u64, key: &str) {
    let mut registry = active_device_registry()
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if let Some(devices) = registry.get_mut(&session_id) {
        devices.remove(key);
        if devices.is_empty() {
            registry.remove(&session_id);
        }
    }
}

fn note_session_ended(session_id: u64) {
    active_device_registry()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(&session_id);
}

/// The devices the control panel currently holds open over native HID, most
/// recently opened first. Empty when no HID session has a device open — e.g.
/// a wired mouse that was never opened still leaves no trace here, which is
/// why callers fall back to the battery list and the active profile.
pub fn active_devices() -> Vec<ActiveDeviceInfo> {
    let registry = active_device_registry()
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let mut devices = registry
        .values()
        .flat_map(|devices| devices.values())
        .collect::<Vec<_>>();
    devices.sort_by_key(|entry| std::cmp::Reverse(entry.seq));
    devices
        .into_iter()
        .map(|entry| entry.info.clone())
        .collect()
}

#[derive(Debug, Deserialize)]
struct ClientMessage {
    id: u64,
    #[serde(flatten)]
    command: Command,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum Command {
    List {
        #[serde(rename = "vendorIds")]
        vendor_ids: Vec<u16>,
    },
    Open {
        device: String,
    },
    Close {
        device: String,
    },
    Listen {
        device: String,
    },
    Unlisten {
        device: String,
    },
    SendReport {
        device: String,
        #[serde(rename = "reportId")]
        report_id: u8,
        data: Vec<u8>,
    },
    SendFeatureReport {
        device: String,
        #[serde(rename = "reportId")]
        report_id: u8,
        data: Vec<u8>,
    },
    ReceiveFeatureReport {
        device: String,
        #[serde(rename = "reportId")]
        report_id: u8,
    },
    ReceiveInputReport {
        device: String,
        #[serde(rename = "reportId")]
        report_id: u8,
    },
}

impl Command {
    fn kind(&self) -> &'static str {
        match self {
            Self::List { .. } => "list",
            Self::Open { .. } => "open",
            Self::Close { .. } => "close",
            Self::Listen { .. } => "listen",
            Self::Unlisten { .. } => "unlisten",
            Self::SendReport { .. } => "sendReport",
            Self::SendFeatureReport { .. } => "sendFeatureReport",
            Self::ReceiveFeatureReport { .. } => "receiveFeatureReport",
            Self::ReceiveInputReport { .. } => "receiveInputReport",
        }
    }
}

#[derive(Debug, Serialize)]
struct Reply {
    id: u64,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    devices: Option<Vec<DeviceSummary>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Vec<u8>>,
}

impl Reply {
    fn ok(id: u64) -> Self {
        Self {
            id,
            ok: true,
            error: None,
            devices: None,
            data: None,
        }
    }

    fn error(id: u64, error: impl Into<String>) -> Self {
        Self {
            id,
            ok: false,
            error: Some(error.into()),
            devices: None,
            data: None,
        }
    }
}

#[derive(Debug, Serialize)]
struct InputReport {
    #[serde(rename = "type")]
    kind: &'static str,
    device: String,
    #[serde(rename = "reportId")]
    report_id: u8,
    data: Vec<u8>,
}

#[derive(Clone)]
struct InputSender {
    reports: mpsc::Sender<InputReport>,
    failed: watch::Sender<bool>,
}

impl InputSender {
    fn send(&self, report: InputReport) -> Result<(), ()> {
        if *self.failed.borrow() {
            return Err(());
        }
        if self.reports.try_send(report).is_err() {
            self.failed.send_replace(true);
            return Err(());
        }
        Ok(())
    }
}

fn input_channel() -> (
    InputSender,
    mpsc::Receiver<InputReport>,
    watch::Receiver<bool>,
) {
    let (reports, rx) = mpsc::channel(INPUT_QUEUE_CAPACITY);
    let (failed, failure_rx) = watch::channel(false);
    (InputSender { reports, failed }, rx, failure_rx)
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DeviceSummary {
    key: String,
    vendor_id: u16,
    product_id: u16,
    product_name: String,
    collections: Vec<CollectionInfo>,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct CollectionInfo {
    usage_page: u16,
    usage: u16,
    input_reports: Vec<ReportInfo>,
    output_reports: Vec<ReportInfo>,
    feature_reports: Vec<ReportInfo>,
    children: Vec<CollectionInfo>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReportInfo {
    report_id: u8,
    items: Vec<ReportItem>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReportItem {
    report_size: u32,
    report_count: u32,
}

#[derive(Clone, Debug, Default)]
struct ReportLayout {
    feature_bits: HashMap<u8, usize>,
    input_bits: HashMap<u8, usize>,
    output_bits: HashMap<u8, usize>,
}

impl ReportLayout {
    fn feature_buffer_len(&self, report_id: u8) -> Result<usize, String> {
        let bits = self.feature_bits.get(&report_id).copied().ok_or_else(|| {
            format!("feature report {report_id} is not declared by this interface")
        })?;
        Ok(bits
            .div_ceil(8)
            .saturating_add(1)
            .clamp(2, MAX_REPORT_BYTES))
    }

    fn input_buffer_len_for(&self, report_id: u8) -> Result<usize, String> {
        let bits =
            self.input_bits.get(&report_id).copied().ok_or_else(|| {
                format!("input report {report_id} is not declared by this interface")
            })?;
        Ok(bits
            .div_ceil(8)
            .saturating_add(1)
            .clamp(2, MAX_REPORT_BYTES))
    }

    fn input_buffer_len(&self) -> usize {
        self.input_bits
            .values()
            .copied()
            .max()
            .unwrap_or(8 * 64)
            .div_ceil(8)
            .saturating_add(1)
            .clamp(2, MAX_REPORT_BYTES)
    }

    fn input_uses_report_ids(&self) -> bool {
        self.input_bits.keys().any(|report_id| *report_id != 0)
    }
}

#[derive(Clone)]
struct CandidatePath {
    path: CString,
    layout: ReportLayout,
    /// Whether `layout` came from the path's report descriptor. Razer
    /// descriptors are not read, so their layout is empty and says nothing.
    descriptor_known: bool,
}

#[derive(Clone)]
struct Candidate {
    paths: Vec<CandidatePath>,
    summary: DeviceSummary,
}

struct OpenPath {
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    path: CString,
    device: Arc<Mutex<HidDevice>>,
    layout: ReportLayout,
    descriptor_known: bool,
}

/// Another interface of the same physical device (same vendor, product,
/// serial and USB port or Windows container) that declares feature reports.
/// Feature reports only: no reader, no output or input reports.
#[cfg_attr(not(any(target_os = "windows", target_os = "linux")), allow(dead_code))]
struct SiblingPath {
    device: Mutex<HidDevice>,
    layout: ReportLayout,
    vendor_page: bool,
}

/// The handle an input reader thread reads from.
///
/// Windows and Linux give the reader its own handle: each Windows handle and
/// each open hidraw fd on Linux gets its own copy of every input report, so
/// the reader can block in `read_timeout` without holding up writes on the
/// command handle, and without spinning. macOS (and the libusb backends)
/// keep polling the shared command handle: whether a second IOHIDDevice open
/// of the same interface also receives every input report is unverified.
enum ReaderHandle {
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    Owned(HidDevice),
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    Shared(Arc<Mutex<HidDevice>>),
}

impl ReaderHandle {
    fn read(&self, buffer: &mut [u8]) -> Result<usize, String> {
        match self {
            #[cfg(any(target_os = "windows", target_os = "linux"))]
            Self::Owned(device) => device
                .read_timeout(buffer, READ_TIMEOUT_MS)
                .map_err(|error| error.to_string()),
            // The handle is shared with send_report, and std's Mutex is not
            // fair: a reader that blocks in read_timeout while holding the
            // lock, then immediately re-locks, can starve a writer for many
            // seconds — and the writer holds the session lock meanwhile, so
            // every other command (even `list`) stalls behind it. Read without
            // blocking under the lock and idle with it released instead.
            #[cfg(not(any(target_os = "windows", target_os = "linux")))]
            Self::Shared(device) => {
                let size = device
                    .lock()
                    .map_err(|_| "native HID lock was poisoned".to_owned())?
                    .read_timeout(buffer, 0)
                    .map_err(|error| error.to_string())?;
                if size == 0 {
                    thread::sleep(SHARED_READ_IDLE);
                }
                Ok(size)
            }
        }
    }
}

/// What a reader handle must share with the command handle of the same path:
/// both handles have to reach the same physical interface.
#[cfg(any(target_os = "windows", target_os = "linux"))]
#[derive(Debug, PartialEq)]
struct HandleIdentity {
    vendor_id: u16,
    product_id: u16,
    serial: Option<String>,
    interface: i32,
    descriptor: Vec<u8>,
}

/// Reads the identity from the open handle itself (on Linux, from the hidraw
/// node the fd refers to), never from the path it was opened by.
#[cfg(any(target_os = "windows", target_os = "linux"))]
fn handle_identity(device: &HidDevice) -> Result<HandleIdentity, String> {
    let info = device
        .get_device_info()
        .map_err(|error| error.to_string())?;
    let mut descriptor = vec![0; MAX_REPORT_DESCRIPTOR_SIZE];
    let size = device
        .get_report_descriptor(&mut descriptor)
        .map_err(|error| error.to_string())?;
    descriptor.truncate(size);
    Ok(HandleIdentity {
        vendor_id: info.vendor_id(),
        product_id: info.product_id(),
        serial: info.serial_number().map(str::to_owned),
        interface: info.interface_number(),
        descriptor,
    })
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn has_security_key_collection(collections: &[CollectionInfo]) -> bool {
    collections.iter().any(|collection| {
        collection.usage_page == FIDO_USAGE_PAGE
            || has_security_key_collection(&collection.children)
    })
}

/// The reader opens its path again after `open` checked it. If the node was
/// replaced in between (unplug and replug, or a security key that took the
/// same hidraw number), the new handle reaches another device: refuse it
/// unless it matches the command handle, and never accept a FIDO collection.
#[cfg(any(target_os = "windows", target_os = "linux"))]
fn check_reader_identity(expected: &HandleIdentity, actual: &HandleIdentity) -> Result<(), String> {
    let security_key = parse_report_descriptor(&actual.descriptor)
        .is_ok_and(|(collections, _)| has_security_key_collection(&collections));
    if security_key {
        return Err("the HID input reader reached a security key; refused".into());
    }
    if expected != actual {
        return Err(
            "the HID interface changed before its input reader opened; reopen the device".into(),
        );
    }
    Ok(())
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn open_verified_reader(api: &HidApi, path: &OpenPath) -> Result<HidDevice, String> {
    let reader = api
        .open_path(&path.path)
        .map_err(|error| format!("could not open HID input reader: {error}"))?;
    let command = path
        .device
        .lock()
        .map_err(|_| "native HID lock was poisoned".to_owned())?;
    let expected = handle_identity(&command)
        .map_err(|error| format!("could not identify the HID interface: {error}"))?;
    drop(command);
    let actual = handle_identity(&reader)
        .map_err(|error| format!("could not identify the HID input reader: {error}"))?;
    check_reader_identity(&expected, &actual)?;
    Ok(reader)
}

/// One input reader thread's loop: forwards each report `read` returns until
/// the stop flag is set, the session's input queue rejects a report, or the
/// read fails. `read` returns 0 when nothing arrived within its timeout, so
/// the stop flag is seen within one read timeout.
fn run_reader(
    mut read: impl FnMut(&mut [u8]) -> Result<usize, String>,
    stop: &AtomicBool,
    tx: &InputSender,
    device_key: &str,
    interface: usize,
    buffer_len: usize,
    uses_report_ids: bool,
) {
    let mut buffer = vec![0; buffer_len];
    while !stop.load(Ordering::Acquire) {
        match read(&mut buffer) {
            Ok(0) => {}
            Ok(size) => {
                let (report_id, data) = if uses_report_ids {
                    (buffer[0], buffer[1..size].to_vec())
                } else {
                    (0, buffer[..size].to_vec())
                };
                if tx
                    .send(InputReport {
                        kind: "inputreport",
                        device: device_key.to_owned(),
                        report_id,
                        data,
                    })
                    .is_err()
                {
                    stop.store(true, Ordering::Release);
                    break;
                }
            }
            Err(error) => {
                tracing::warn!(
                    device = %device_key,
                    interface,
                    %error,
                    "Native HID input reader stopped"
                );
                break;
            }
        }
    }
}

struct OpenDevice {
    paths: Vec<OpenPath>,
    vendor_id: u16,
    listening: bool,
    stop: Option<Arc<AtomicBool>>,
    readers: Vec<thread::JoinHandle<()>>,
    /// Found the first time a feature report needs them, kept until close.
    feature_siblings: OnceLock<Vec<SiblingPath>>,
}

impl OpenDevice {
    /// True when a listener was started and every reader thread has exited
    /// without being asked to stop.
    fn readers_died(&self) -> bool {
        self.listening
            && !self.readers.is_empty()
            && self.readers.iter().all(thread::JoinHandle::is_finished)
    }

    fn stop_reader(&mut self) {
        if let Some(stop) = self.stop.take() {
            stop.store(true, Ordering::Release);
        }
        for reader in self.readers.drain(..) {
            let _ = reader.join();
        }
        self.listening = false;
    }
}

impl Drop for OpenDevice {
    fn drop(&mut self) {
        self.stop_reader();
    }
}

struct HidSession {
    session_id: u64,
    api: HidApi,
    device_keys: HashMap<Vec<u8>, String>,
    last_device_keys: Option<Vec<String>>,
    candidates: HashMap<String, Candidate>,
    open_devices: HashMap<String, OpenDevice>,
    listeners: HashSet<String>,
    input_tx: InputSender,
}

impl Drop for HidSession {
    /// Asks every reader to stop before any is joined, so ending a session
    /// waits about one read timeout in total rather than one per device.
    fn drop(&mut self) {
        for open in self.open_devices.values() {
            if let Some(stop) = &open.stop {
                stop.store(true, Ordering::Release);
            }
        }
    }
}

impl HidSession {
    fn new(session_id: u64, input_tx: InputSender) -> Result<Self, String> {
        Ok(Self {
            session_id,
            api: new_api().map_err(|error| format!("could not initialize native HID: {error}"))?,
            device_keys: HashMap::new(),
            last_device_keys: None,
            candidates: HashMap::new(),
            open_devices: HashMap::new(),
            listeners: HashSet::new(),
            input_tx,
        })
    }

    fn execute(&mut self, message: ClientMessage) -> Reply {
        let id = message.id;
        let command = message.command.kind();
        let vendor_count = match &message.command {
            Command::List { vendor_ids } => Some(vendor_ids.len()),
            _ => None,
        };
        let started = Instant::now();
        let outcome = match message.command {
            Command::List { vendor_ids } => self.list(vendor_ids).map(|devices| Reply {
                devices: Some(devices),
                ..Reply::ok(id)
            }),
            Command::Open { device } => self.open(&device).map(|()| Reply::ok(id)),
            Command::Close { device } => self.close(&device).map(|()| Reply::ok(id)),
            Command::Listen { device } => self.listen(&device).map(|()| Reply::ok(id)),
            Command::Unlisten { device } => self.unlisten(&device).map(|()| Reply::ok(id)),
            Command::SendReport {
                device,
                report_id,
                data,
            } => self
                .with_recovery(&device, |session| {
                    session.send_report(&device, report_id, data.clone(), false)
                })
                .map(|()| Reply::ok(id)),
            Command::SendFeatureReport {
                device,
                report_id,
                data,
            } => self
                .with_recovery(&device, |session| {
                    session.send_report(&device, report_id, data.clone(), true)
                })
                .map(|()| Reply::ok(id)),
            Command::ReceiveFeatureReport { device, report_id } => self
                .with_recovery(&device, |session| {
                    session.receive_feature_report(&device, report_id)
                })
                .map(|data| Reply {
                    data: Some(data),
                    ..Reply::ok(id)
                }),
            Command::ReceiveInputReport { device, report_id } => self
                .with_recovery(&device, |session| {
                    session.receive_input_report(&device, report_id)
                })
                .map(|data| Reply {
                    data: Some(data),
                    ..Reply::ok(id)
                }),
        };
        let reply = outcome.unwrap_or_else(|error| Reply::error(id, error));
        let elapsed_ms = started.elapsed().as_millis();
        let device_count = reply.devices.as_ref().map(Vec::len);
        let listed_keys = reply.devices.as_ref().map(|devices| {
            devices
                .iter()
                .map(|device| device.key.clone())
                .collect::<Vec<_>>()
        });
        let inventory_changed = listed_keys
            .as_ref()
            .is_some_and(|keys| self.last_device_keys.as_ref() != Some(keys));
        if let Some(keys) = listed_keys {
            self.last_device_keys = Some(keys);
        }
        if !reply.ok {
            tracing::warn!(
                session_id = self.session_id,
                request_id = id,
                command,
                elapsed_ms,
                error = reply.error.as_deref().unwrap_or("unknown error"),
                "HID command failed"
            );
        } else if command == "list" && elapsed_ms >= 1_000 {
            tracing::warn!(
                session_id = self.session_id,
                request_id = id,
                command,
                elapsed_ms,
                vendor_count,
                device_count,
                "HID enumeration was slow"
            );
        } else if matches!(command, "open" | "close") || (command == "list" && inventory_changed) {
            tracing::info!(
                session_id = self.session_id,
                request_id = id,
                command,
                elapsed_ms,
                vendor_count,
                device_count,
                "HID command completed"
            );
        } else {
            tracing::debug!(
                session_id = self.session_id,
                request_id = id,
                command,
                elapsed_ms,
                "HID command completed"
            );
        }
        reply
    }

    fn list(&mut self, vendor_ids: Vec<u16>) -> Result<Vec<DeviceSummary>, String> {
        if vendor_ids.len() > 256 {
            return Err("too many HID vendor ids were requested".into());
        }
        let vendors: HashSet<u16> = vendor_ids.into_iter().collect();
        refresh_api(&mut self.api)
            .map_err(|error| format!("could not refresh HID devices: {error}"))?;

        // Reconcile against the full inventory, not the requested vendor filter:
        // a picker scanning one brand must not close another brand's device.
        let present = self.api.device_list().map(device_group_key).collect();
        self.reconcile_inventory(&present)?;

        if vendors.is_empty() {
            return Ok(Vec::new());
        }

        // Ordered, so devices are keyed and listed the same way every time.
        let mut by_device = std::collections::BTreeMap::<Vec<u8>, Vec<hidapi::DeviceInfo>>::new();
        let blocked = security_key_paths(&self.api);
        for info in self.api.device_list() {
            if vendors.contains(&info.vendor_id())
                && !blocked.contains(info.path())
                && !is_receiver_child(info.path())
            {
                by_device
                    .entry(device_group_key(info))
                    .or_default()
                    .push(info.clone());
            }
        }

        let mut summaries = Vec::with_capacity(by_device.len());
        for (device_id, infos) in by_device {
            let key = if let Some(key) = self.device_keys.get(&device_id) {
                key.clone()
            } else {
                let key = self.unused_key(stable_device_key(&device_id));
                self.device_keys.insert(device_id, key.clone());
                key
            };
            let paths = infos.iter().map(|info| info.path()).collect::<HashSet<_>>();
            if self.candidates.get(&key).is_none_or(|candidate| {
                candidate.paths.len() != paths.len()
                    || candidate
                        .paths
                        .iter()
                        .any(|path| !paths.contains(path.path.as_c_str()))
            }) {
                if !self.open_devices.contains_key(&key) {
                    let candidate = inspect_candidate(&self.api, key.clone(), &infos)?;
                    self.candidates.insert(key.clone(), candidate);
                } else {
                    self.recover(&key)?;
                }
            }
            if let Some(candidate) = self.candidates.get(&key) {
                summaries.push(candidate.summary.clone());
                tracing::debug!(
                    session_id = self.session_id,
                    device_key = %candidate.summary.key,
                    vendor_id = format_args!("{:#06x}", candidate.summary.vendor_id),
                    product_id = format_args!("{:#06x}", candidate.summary.product_id),
                    product_name = %candidate.summary.product_name,
                    path_count = candidate.paths.len(),
                    "HID device enumerated"
                );
            }
        }
        summaries.sort_by(|left, right| left.key.cmp(&right.key));
        Ok(summaries)
    }

    /// `key`, or with a counter appended when another device in this session
    /// already has it (two identical devices with no serial).
    fn unused_key(&self, key: String) -> String {
        let taken = |candidate: &str| self.device_keys.values().any(|used| used == candidate);
        if !taken(&key) {
            return key;
        }
        (2..)
            .map(|count| format!("{key}-{count}"))
            .find(|candidate| !taken(candidate))
            .unwrap_or(key)
    }

    fn reconcile_inventory(&mut self, present: &HashSet<Vec<u8>>) -> Result<(), String> {
        let removed = self
            .device_keys
            .iter()
            .filter(|(identity, _)| !present.contains(*identity))
            .map(|(identity, key)| (identity.clone(), key.clone()))
            .collect::<Vec<_>>();
        for (identity, key) in removed {
            self.close(&key)?;
            self.listeners.remove(&key);
            self.candidates.remove(&key);
            self.device_keys.remove(&identity);
        }
        Ok(())
    }

    fn open(&mut self, key: &str) -> Result<(), String> {
        if self
            .open_devices
            .get(key)
            .is_some_and(OpenDevice::readers_died)
        {
            // Every reader exited on an I/O error: the handles are dead
            // (typically after hibernate re-enumerated USB).
            return self.recover(key);
        }
        if self.open_devices.contains_key(key) {
            // Re-selecting an already-open device still makes it the current
            // one for status purposes.
            if let Some(candidate) = self.candidates.get(key) {
                note_device_opened(self.session_id, &candidate.summary);
            }
            return Ok(());
        }
        let candidate = self
            .candidates
            .get(key)
            .ok_or_else(|| "the selected HID interface is no longer available".to_owned())?;
        let mut paths = Vec::with_capacity(candidate.paths.len());
        let mut errors = Vec::new();
        for candidate_path in &candidate.paths {
            match self.api.open_path(&candidate_path.path) {
                Ok(device) => paths.push(OpenPath {
                    #[cfg(any(target_os = "windows", target_os = "linux"))]
                    path: candidate_path.path.clone(),
                    device: Arc::new(Mutex::new(device)),
                    layout: candidate_path.layout.clone(),
                    descriptor_known: candidate_path.descriptor_known,
                }),
                Err(error) => errors.push(error.to_string()),
            }
        }
        if paths.is_empty() {
            return Err(format!(
                "could not open the HID interface: {}",
                errors.join("; ")
            ));
        }
        tracing::info!(
            session_id = self.session_id,
            device_key = key,
            vendor_id = format_args!("{:#06x}", candidate.summary.vendor_id),
            product_id = format_args!("{:#06x}", candidate.summary.product_id),
            opened_paths = paths.len(),
            failed_paths = errors.len(),
            "HID device opened"
        );
        self.open_devices.insert(
            key.to_owned(),
            OpenDevice {
                paths,
                vendor_id: candidate.summary.vendor_id,
                listening: false,
                stop: None,
                readers: Vec::new(),
                feature_siblings: OnceLock::new(),
            },
        );
        if self.listeners.contains(key) {
            self.start_reader(key)?;
        }
        if let Some(candidate) = self.candidates.get(key) {
            note_device_opened(self.session_id, &candidate.summary);
        }
        Ok(())
    }

    /// Runs a device I/O operation, and when the handle turns out to be dead
    /// (USB re-enumerated after hibernate/resume) re-resolves the device,
    /// reopens it, and retries once.
    fn with_recovery<T>(
        &mut self,
        key: &str,
        mut operation: impl FnMut(&Self) -> Result<T, String>,
    ) -> Result<T, String> {
        let error = match operation(self) {
            Ok(value) => return Ok(value),
            Err(error) => error,
        };
        if !self.open_devices.contains_key(key) || !error.starts_with("could not ") {
            return Err(error);
        }
        tracing::warn!(
            session_id = self.session_id,
            device_key = key,
            %error,
            "HID I/O failed; re-enumerating and reopening the device"
        );
        if let Err(recover_error) = self.recover(key) {
            return Err(format!("{error} (reopen failed: {recover_error})"));
        }
        operation(self)
    }

    /// After a resume every pre-suspend handle is dead and USB may still be
    /// re-enumerating, so reopen each open device, waiting briefly for it to
    /// reappear. Listeners are kept so readers restart on the new handles.
    fn handle_resume(&mut self) {
        let keys = self.open_devices.keys().cloned().collect::<Vec<_>>();
        if keys.is_empty() {
            let _ = refresh_api(&mut self.api);
            return;
        }
        for key in keys {
            for attempt in 1..=RESUME_REOPEN_ATTEMPTS {
                match self.recover(&key) {
                    Ok(()) => break,
                    Err(error) if attempt == RESUME_REOPEN_ATTEMPTS => {
                        tracing::warn!(
                            session_id = self.session_id,
                            device_key = %key,
                            %error,
                            "HID device did not return after resume"
                        );
                    }
                    Err(_) => thread::sleep(RESUME_REOPEN_DELAY),
                }
            }
        }
    }

    /// Drops the device's handles, re-enumerates, rebuilds its paths from the
    /// fresh inventory, and reopens it.
    fn recover(&mut self, key: &str) -> Result<(), String> {
        self.close(key)?;
        refresh_api(&mut self.api)
            .map_err(|error| format!("could not refresh HID devices: {error}"))?;
        let identity = self
            .device_keys
            .iter()
            .find(|(_, device_key)| device_key.as_str() == key)
            .map(|(identity, _)| identity.clone())
            .ok_or_else(|| "the selected HID interface is no longer available".to_owned())?;
        let blocked = security_key_paths(&self.api);
        let infos = self
            .api
            .device_list()
            .filter(|info| device_group_key(info) == identity && !blocked.contains(info.path()))
            .cloned()
            .collect::<Vec<_>>();
        if infos.is_empty() {
            self.listeners.remove(key);
            self.candidates.remove(key);
            self.device_keys.remove(&identity);
            return Err("the HID device is no longer connected".into());
        }
        let candidate = inspect_candidate(&self.api, key.to_owned(), &infos)?;
        self.candidates.insert(key.to_owned(), candidate);
        self.open(key)
    }

    fn close(&mut self, key: &str) -> Result<(), String> {
        let Some(mut open) = self.open_devices.remove(key) else {
            return Ok(());
        };
        open.stop_reader();
        note_device_closed(self.session_id, key);
        Ok(())
    }

    fn listen(&mut self, key: &str) -> Result<(), String> {
        if !self.candidates.contains_key(key) {
            return Err("the selected HID interface is no longer available".into());
        }
        self.listeners.insert(key.to_owned());
        if self.open_devices.contains_key(key) {
            self.start_reader(key)?;
        }
        Ok(())
    }

    fn start_reader(&mut self, key: &str) -> Result<(), String> {
        let reader_specs = {
            let open = self
                .open_devices
                .get(key)
                .ok_or_else(|| "the HID interface must be open before listening".to_owned())?;
            if open.listening {
                return Ok(());
            }
            open.paths
                .iter()
                .map(|path| {
                    #[cfg(any(target_os = "windows", target_os = "linux"))]
                    let device = ReaderHandle::Owned(open_verified_reader(&self.api, path)?);
                    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
                    let device = ReaderHandle::Shared(Arc::clone(&path.device));
                    Ok((
                        device,
                        path.layout.input_buffer_len(),
                        path.layout.input_uses_report_ids(),
                    ))
                })
                .collect::<Result<Vec<_>, String>>()?
        };

        let open = self
            .open_devices
            .get_mut(key)
            .ok_or_else(|| "the HID interface must be open before listening".to_owned())?;
        let stop = Arc::new(AtomicBool::new(false));
        // Stored before any reader starts, so a later spawn failure can stop and
        // join the readers already running instead of leaving them to block cleanup.
        open.stop = Some(Arc::clone(&stop));
        for (index, (device, buffer_len, uses_report_ids)) in reader_specs.into_iter().enumerate() {
            let thread_stop = Arc::clone(&stop);
            let tx = self.input_tx.clone();
            let device_key = key.to_owned();
            let reader = thread::Builder::new()
                .name(format!("openmouse-hid-{key}-{index}"))
                .spawn(move || {
                    run_reader(
                        |buffer| device.read(buffer),
                        &thread_stop,
                        &tx,
                        &device_key,
                        index,
                        buffer_len,
                        uses_report_ids,
                    );
                });
            match reader {
                Ok(reader) => open.readers.push(reader),
                Err(error) => {
                    open.stop_reader();
                    return Err(format!("could not start the HID input reader: {error}"));
                }
            }
        }
        open.listening = true;
        Ok(())
    }

    fn unlisten(&mut self, key: &str) -> Result<(), String> {
        self.listeners.remove(key);
        if let Some(open) = self.open_devices.get_mut(key) {
            open.stop_reader();
        }
        Ok(())
    }

    fn send_report(
        &self,
        key: &str,
        report_id: u8,
        data: Vec<u8>,
        feature: bool,
    ) -> Result<(), String> {
        if data.len() >= MAX_REPORT_BYTES {
            return Err("HID report exceeds the native descriptor limit".into());
        }
        let open = self
            .open_devices
            .get(key)
            .ok_or_else(|| "the HID interface is not open".to_owned())?;
        // An open device has several paths only when `grouped_device_key` matched
        // them by serial or Windows HID interface. A report still goes to exactly one.
        if open.paths.is_empty() {
            return Err("the HID interface has no open path".into());
        }
        let _access = crate::drivers::manual_report_access(open.vendor_id)
            .map_err(|error| error.to_string())?;
        let mut frame = Vec::with_capacity(data.len() + 1);
        frame.push(report_id);
        frame.extend_from_slice(&data);
        let mut last_error = None;
        // The first path that takes the report is the only one written, so a
        // command never reaches the device twice.
        let mut write = |device: &Mutex<HidDevice>| {
            let result = device
                .lock()
                .map_err(|_| "native HID lock was poisoned".to_owned())
                .and_then(|device| {
                    if feature {
                        device.send_feature_report(&frame).map(|_| ())
                    } else {
                        device.write(&frame).map(|_| ())
                    }
                    .map_err(|error| error.to_string())
                });
            match result {
                Ok(()) => ControlFlow::Break(()),
                Err(error) => {
                    last_error = Some(error);
                    ControlFlow::Continue(())
                }
            }
        };
        if feature {
            if self
                .route_feature(key, open, report_id, &mut |device, _| write(device))
                .is_break()
            {
                return Ok(());
            }
        } else {
            let layouts: Vec<&ReportLayout> = open.paths.iter().map(|path| &path.layout).collect();
            for index in report_paths(&layouts, report_id, false) {
                if write(&open.paths[index].device).is_break() {
                    return Ok(());
                }
            }
        }
        let operation = if feature { "feature" } else { "output" };
        Err(format!(
            "could not send {operation} report {report_id}: {}",
            last_error.unwrap_or_else(|| "no HID path was open".to_owned())
        ))
    }

    fn receive_input_report(&self, key: &str, report_id: u8) -> Result<Vec<u8>, String> {
        let open = self
            .open_devices
            .get(key)
            .ok_or_else(|| "the HID interface is not open".to_owned())?;
        let mut available = false;
        let mut last_error = None;
        for path in &open.paths {
            let buffer_len = match path.layout.input_buffer_len_for(report_id) {
                Ok(length) => length,
                Err(_) if open.vendor_id == MICROSOFT_VENDOR_ID => 73,
                Err(_) => continue,
            };
            available = true;
            let mut data = vec![0; buffer_len];
            data[0] = report_id;
            let result = path
                .device
                .lock()
                .map_err(|_| "native HID lock was poisoned".to_owned())
                .and_then(|device| {
                    device
                        .get_input_report(&mut data)
                        .map_err(|error| error.to_string())
                });
            match result {
                Ok(size) => {
                    tracing::info!(
                        session_id = self.session_id,
                        report_id,
                        size,
                        first_byte = data[0],
                        second_byte = data.get(1).unwrap_or(&0),
                        "DEBUG get_input_report result"
                    );
                    return Ok(report_payload(&data, size));
                }
                Err(error) => last_error = Some(error),
            }
        }
        if !available {
            return Err(format!(
                "input report {report_id} is not declared by this interface"
            ));
        }
        Err(format!(
            "could not receive input report {report_id}: {}",
            last_error.unwrap_or_else(|| "no HID path was open".to_owned())
        ))
    }

    fn receive_feature_report(&self, key: &str, report_id: u8) -> Result<Vec<u8>, String> {
        let open = self
            .open_devices
            .get(key)
            .ok_or_else(|| "the HID interface is not open".to_owned())?;
        let mut available = false;
        let mut response = None;
        let mut fallback_response = None;
        let mut last_error = None;
        let vendor_id = open.vendor_id;
        let _ = self.route_feature(key, open, report_id, &mut |device, layout| {
            let buffer_len = match layout.feature_buffer_len(report_id) {
                Ok(length) => length,
                Err(_) if vendor_id == RAZER_VENDOR_ID => RAZER_FEATURE_BUFFER_LEN,
                Err(_) if vendor_id == MICROSOFT_VENDOR_ID => 64,
                Err(_) if vendor_id == THERMALTAKE_VENDOR_ID => THERMALTAKE_FEATURE_BUFFER_LEN,
                Err(_) => return ControlFlow::Continue(()),
            };
            available = true;
            let mut data = vec![0; buffer_len];
            data[0] = report_id;
            let result = device
                .lock()
                .map_err(|_| "native HID lock was poisoned".to_owned())
                .and_then(|device| {
                    device
                        .get_feature_report(&mut data)
                        .map_err(|error| error.to_string())
                });
            match result {
                Ok(size) => {
                    let payload = report_payload(&data, size);
                    if vendor_id != RAZER_VENDOR_ID || payload.iter().any(|byte| *byte != 0) {
                        response = Some(payload);
                        return ControlFlow::Break(());
                    }
                    fallback_response.get_or_insert(payload);
                }
                Err(error) => last_error = Some(error),
            }
            ControlFlow::Continue(())
        });
        if let Some(response) = response.or(fallback_response) {
            return Ok(response);
        }
        if !available {
            return Err(format!(
                "feature report {report_id} is not declared by this interface"
            ));
        }
        Err(format!(
            "could not receive feature report {report_id}: {}",
            last_error.unwrap_or_else(|| "no HID path was open".to_owned())
        ))
    }

    /// Offers a feature report to the paths that may take it, in order, until
    /// `attempt` breaks. The opened paths that declare the report go first and
    /// alone, as before. When none declares it, sibling interfaces of the same
    /// physical device that do are tried too: before the opened paths when
    /// their descriptors show no feature report at all, after them otherwise
    /// (Razer, whose descriptors are not read, and the vendors whose undeclared
    /// reports are known to work), so a path that works today still goes first.
    fn route_feature(
        &self,
        key: &str,
        open: &OpenDevice,
        report_id: u8,
        attempt: &mut dyn FnMut(&Mutex<HidDevice>, &ReportLayout) -> ControlFlow<()>,
    ) -> ControlFlow<()> {
        let opened: Vec<(&ReportLayout, bool)> = open
            .paths
            .iter()
            .map(|path| (&path.layout, path.descriptor_known))
            .collect();
        let quirks = matches!(
            open.vendor_id,
            RAZER_VENDOR_ID | MICROSOFT_VENDOR_ID | THERMALTAKE_VENDOR_ID
        );
        let all_opened = (0..open.paths.len()).collect::<Vec<_>>();
        match feature_route(&opened, report_id, quirks) {
            FeatureRoute::Declared(indexes) => try_opened_paths(&open.paths, indexes, attempt),
            FeatureRoute::SiblingsFirst => {
                self.try_feature_siblings(key, open, report_id, attempt)?;
                try_opened_paths(&open.paths, all_opened, attempt)
            }
            FeatureRoute::OpenedFirst => {
                try_opened_paths(&open.paths, all_opened, attempt)?;
                self.try_feature_siblings(key, open, report_id, attempt)
            }
        }
    }

    fn try_feature_siblings(
        &self,
        key: &str,
        open: &OpenDevice,
        report_id: u8,
        attempt: &mut dyn FnMut(&Mutex<HidDevice>, &ReportLayout) -> ControlFlow<()>,
    ) -> ControlFlow<()> {
        let siblings = open.feature_siblings.get_or_init(|| {
            let siblings = scan_feature_siblings(&self.api, open);
            tracing::info!(
                session_id = self.session_id,
                device_key = key,
                siblings = siblings.len(),
                "HID feature siblings resolved"
            );
            siblings
        });
        let routes: Vec<(&ReportLayout, bool)> = siblings
            .iter()
            .map(|sibling| (&sibling.layout, sibling.vendor_page))
            .collect();
        for index in sibling_order(&routes, report_id) {
            let sibling = &siblings[index];
            attempt(&sibling.device, &sibling.layout)?;
        }
        ControlFlow::Continue(())
    }
}

/// The bytes after the report-id byte of a read into `data`. hidapi on Windows
/// adds one to the count when the first byte is 0 (an unnumbered report), so a
/// read that fills the buffer reports one byte more than the buffer holds.
pub(crate) fn report_payload(data: &[u8], size: usize) -> Vec<u8> {
    data.get(1..size.min(data.len()))
        .unwrap_or_default()
        .to_vec()
}

/// hidapi enumerates devices outside its own initialisation lock, and on macOS
/// every enumeration shares one IOHIDManager and run loop, so two threads
/// enumerating at once can crash the process with SIGTRAP. Bridge enumerates
/// from the control-panel session, the battery poll, the 5-second device scan
/// and the tests, so every enumeration goes through this lock.
static ENUMERATION: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// hidapi's macOS backend schedules its IOHIDManager on the run loop of the
/// thread that first initialises it. When that thread exits (a test thread, or
/// a pool thread that went idle) the run loop goes with it, and the next
/// enumeration from any other thread traps. So the one-time initialisation runs
/// on a thread that lives as long as the process.
#[cfg(target_os = "macos")]
fn initialise_on_a_permanent_thread() {
    static INITIALISED: std::sync::Once = std::sync::Once::new();
    INITIALISED.call_once(|| {
        let (done, wait) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("openmouse-hid-init".into())
            .spawn(move || {
                let _ = HidApi::new();
                let _ = done.send(());
                loop {
                    std::thread::park();
                }
            });
        if spawned.is_ok() {
            let _ = wait.recv();
        }
    });
}

#[cfg(not(target_os = "macos"))]
fn initialise_on_a_permanent_thread() {}

/// A new HID context (which enumerates), serialised with every other one.
pub(crate) fn new_api() -> hidapi::HidResult<HidApi> {
    let _guard = ENUMERATION
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    initialise_on_a_permanent_thread();
    HidApi::new()
}

/// Refreshes a context's device list, serialised with every other enumeration.
pub(crate) fn refresh_api(api: &mut HidApi) -> hidapi::HidResult<()> {
    let _guard = ENUMERATION
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    api.refresh_devices()
}

/// The interfaces of one device that declare a feature report, opened for a
/// native driver that talks to the mouse itself rather than through a control
/// panel session.
pub(crate) struct FeatureInterfaces {
    // Keeps the HID library initialised while the devices are open.
    _api: HidApi,
    pub interfaces: Vec<FeatureInterface>,
}

pub(crate) struct FeatureInterface {
    pub device: HidDevice,
    pub product_id: u16,
    /// Buffer size to read the feature report into, report id byte included.
    pub buffer_len: usize,
}

/// Opens every vendor-defined interface (usage page `0xff00` and up) of the
/// given products whose report descriptor declares feature report `report_id`.
/// Standard mouse, keyboard and consumer interfaces are never opened.
pub(crate) fn feature_interfaces(
    vendor_id: u16,
    product_ids: &[u16],
    report_id: u8,
) -> Result<FeatureInterfaces, String> {
    let api = new_api().map_err(|error| error.to_string())?;
    let mut seen = HashSet::new();
    let mut interfaces = Vec::new();
    for info in api.device_list() {
        if info.vendor_id() != vendor_id
            || !product_ids.contains(&info.product_id())
            || info.usage_page() < 0xff00
            || !seen.insert(info.path().to_bytes().to_vec())
        {
            continue;
        }
        let Ok(device) = api.open_path(info.path()) else {
            continue;
        };
        let mut descriptor = vec![0; MAX_REPORT_DESCRIPTOR_SIZE];
        let Ok(size) = device.get_report_descriptor(&mut descriptor) else {
            continue;
        };
        let Ok((_, layout)) = parse_report_descriptor(&descriptor[..size]) else {
            continue;
        };
        if let Ok(buffer_len) = layout.feature_buffer_len(report_id) {
            interfaces.push(FeatureInterface {
                device,
                product_id: info.product_id(),
                buffer_len,
            });
        }
    }
    Ok(FeatureInterfaces {
        _api: api,
        interfaces,
    })
}

/// Whether `path` is a hidraw node the Linux kernel made for a device paired
/// through a Logitech receiver (`hid-logitech-dj`), rather than the receiver
/// itself. Windows and macOS have no such node, so the control panel never
/// expects one: it reported the receiver's name ("USB Receiver"), was tried
/// first, and its HID++ probes went unanswered for 10-20 s before the panel
/// fell back to the real receiver. The panel reaches paired mice through the
/// receiver, as on the other platforms, so these nodes are left out. Bridge's
/// own bus scan leaves them out too: the receiver stands for its mice.
#[cfg(target_os = "linux")]
pub(crate) fn is_receiver_child(path: &std::ffi::CStr) -> bool {
    let Some(node) = path
        .to_str()
        .ok()
        .and_then(|path| path.strip_prefix("/dev/"))
    else {
        return false;
    };
    std::fs::read_to_string(format!("/sys/class/hidraw/{node}/device/uevent"))
        .is_ok_and(|uevent| is_receiver_child_uevent(&uevent))
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn is_receiver_child(_path: &std::ffi::CStr) -> bool {
    false
}

/// The `uevent` side of [`is_receiver_child`]: the per-device HID++ driver,
/// on a physical path with the receiver's device index appended
/// (`usb-0000:00:14.0-9/input2:1`). Wired and Bluetooth mice on the same
/// driver have no such suffix.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn is_receiver_child_uevent(uevent: &str) -> bool {
    let field = |name: &str| {
        uevent
            .lines()
            .find_map(|line| line.strip_prefix(name)?.strip_prefix('='))
    };
    field("DRIVER") == Some("logitech-hidpp-device")
        && field("HID_PHYS")
            .and_then(|phys| phys.rsplit_once('/'))
            .and_then(|(_, interface)| interface.split_once(':'))
            .is_some_and(|(input, index)| {
                input.starts_with("input")
                    && !index.is_empty()
                    && index.bytes().all(|byte| byte.is_ascii_digit())
            })
}

fn grouped_device_key(
    vendor_id: u16,
    product_id: u16,
    path: &[u8],
    serial: Option<&str>,
) -> Vec<u8> {
    // A serial is a verified physical identity, so every collection of that
    // device is one candidate. Without one, Windows collections group by their
    // interface's OS-assigned parent instance; anything else stays apart by path.
    let serial = serial.filter(|serial| !serial.is_empty());
    let interface = collection_interface(path);
    let (kind, identity) = match (serial, &interface) {
        (Some(serial), _) => (1, serial.as_bytes()),
        (None, Some(interface)) => (2, interface.as_bytes()),
        (None, None) => (0, path),
    };
    let mut key = Vec::with_capacity(13 + identity.len());
    key.extend_from_slice(&vendor_id.to_le_bytes());
    key.extend_from_slice(&product_id.to_le_bytes());
    key.push(kind);
    key.extend_from_slice(&(identity.len() as u64).to_le_bytes());
    key.extend_from_slice(identity);
    key
}

/// The key a device is listed under: the same for the same device across
/// sessions, page reloads and Bridge restarts, so the control panel can
/// remember what it learned about it (that a receiver carries a mouse, and
/// that mouse's name). Before, keys were numbered per session in no fixed
/// order, so a reload forgot all of it.
///
/// `hid-<vendor><product>-<hash>`, where the hash covers the device's
/// grouping identity (vendor, product and serial, or the interface or path
/// without one). The serial itself is never sent: WebHID does not give pages
/// serial numbers, and Bridge should not either.
fn stable_device_key(identity: &[u8]) -> String {
    // FNV-1a: fixed, unlike `HashMap`'s randomly seeded hasher.
    let hash = identity
        .iter()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
        });
    let id = |at: usize| {
        u16::from_le_bytes([
            identity.get(at).copied().unwrap_or(0),
            identity.get(at + 1).copied().unwrap_or(0),
        ])
    };
    let (vendor, product) = (id(0), id(2));
    format!(
        "hid-{vendor:04x}{product:04x}-{:08x}",
        hash as u32 ^ (hash >> 32) as u32
    )
}

/// The paths that carry a FIDO collection. A path is blocked whole, since one
/// hidraw node can hold a security key's collection beside others.
fn security_key_paths(api: &HidApi) -> HashSet<std::ffi::CString> {
    security_key_paths_in(
        api.device_list()
            .map(|info| (info.path(), info.usage_page())),
    )
}

fn security_key_paths_in<'a>(
    interfaces: impl Iterator<Item = (&'a std::ffi::CStr, u16)>,
) -> HashSet<std::ffi::CString> {
    interfaces
        .filter(|(_, usage_page)| *usage_page == FIDO_USAGE_PAGE)
        .map(|(path, _)| path.to_owned())
        .collect()
}

fn device_group_key(info: &hidapi::DeviceInfo) -> Vec<u8> {
    grouped_device_key(
        info.vendor_id(),
        info.product_id(),
        info.path().to_bytes(),
        info.serial_number(),
    )
}

/// The paths to try for a report, in order: those whose descriptor declares it,
/// or every path when none does (Razer descriptors are not parsed). The sender
/// stops at the first path that takes the report.
fn report_paths(layouts: &[&ReportLayout], report_id: u8, feature: bool) -> Vec<usize> {
    let declares = |layout: &ReportLayout| {
        if feature {
            layout.feature_bits.contains_key(&report_id)
        } else {
            layout.output_bits.contains_key(&report_id)
        }
    };
    let declared: Vec<usize> = (0..layouts.len())
        .filter(|&index| declares(layouts[index]))
        .collect();
    if declared.is_empty() {
        (0..layouts.len()).collect()
    } else {
        declared
    }
}

/// Where a feature report goes when it is sent to or read from an open device.
#[derive(Debug, PartialEq)]
enum FeatureRoute {
    /// These opened paths declare it; nothing else is tried.
    Declared(Vec<usize>),
    /// The opened descriptors were read and declare no feature report at all:
    /// sibling interfaces that declare it first, then every opened path.
    SiblingsFirst,
    /// Every opened path first (as before), then sibling interfaces.
    OpenedFirst,
}

/// `opened` is each opened path's layout and whether it came from a read
/// descriptor. `quirks` marks the vendors whose undeclared feature reports
/// are known to work on the opened path (Razer, Microsoft, Thermaltake).
fn feature_route(opened: &[(&ReportLayout, bool)], report_id: u8, quirks: bool) -> FeatureRoute {
    let declared: Vec<usize> = opened
        .iter()
        .enumerate()
        .filter(|(_, (layout, _))| layout.feature_bits.contains_key(&report_id))
        .map(|(index, _)| index)
        .collect();
    if !declared.is_empty() {
        return FeatureRoute::Declared(declared);
    }
    let known_featureless = !opened.is_empty()
        && opened
            .iter()
            .all(|(layout, known)| *known && layout.feature_bits.is_empty());
    if known_featureless && !quirks {
        FeatureRoute::SiblingsFirst
    } else {
        FeatureRoute::OpenedFirst
    }
}

/// The siblings to try for a feature report: only those that declare it,
/// vendor-defined interfaces first, each group in enumeration order.
/// `siblings` is each sibling's layout and whether it has a vendor-page collection.
fn sibling_order(siblings: &[(&ReportLayout, bool)], report_id: u8) -> Vec<usize> {
    let declaring = |vendor: bool| {
        siblings
            .iter()
            .enumerate()
            .filter(move |(_, (layout, is_vendor))| {
                *is_vendor == vendor && layout.feature_bits.contains_key(&report_id)
            })
            .map(|(index, _)| index)
    };
    declaring(true).chain(declaring(false)).collect()
}

/// What two interfaces must share to count as one physical device.
#[derive(Debug, PartialEq)]
#[cfg_attr(not(any(target_os = "windows", target_os = "linux")), allow(dead_code))]
struct PhysicalIdentity {
    vendor_id: u16,
    product_id: u16,
    serial: Option<String>,
    /// The USB port (Linux) or Windows container the interface belongs to.
    physical: Vec<u8>,
}

#[cfg_attr(not(any(target_os = "windows", target_os = "linux")), allow(dead_code))]
impl PhysicalIdentity {
    fn new(vendor_id: u16, product_id: u16, serial: Option<&str>, physical: Vec<u8>) -> Self {
        Self {
            vendor_id,
            product_id,
            serial: serial
                .filter(|serial| !serial.is_empty())
                .map(str::to_owned),
            physical,
        }
    }
}

/// The USB port of a hidraw node, from its `uevent`: `HID_PHYS` without the
/// trailing `/inputN` (`usb-0000:00:14.0-2/input2` -> `usb-0000:00:14.0-2`).
/// Anything else (Bluetooth, whose `HID_PHYS` is the adapter's address and
/// so the same for every device on it, receiver children, virtual devices)
/// has no port, so it never gets siblings.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn usb_port_from_uevent(uevent: &str) -> Option<String> {
    let phys = uevent
        .lines()
        .find_map(|line| line.strip_prefix("HID_PHYS="))?;
    let (port, interface) = phys.rsplit_once('/')?;
    let number = interface.strip_prefix("input")?;
    (port.starts_with("usb-")
        && !number.is_empty()
        && number.bytes().all(|byte| byte.is_ascii_digit()))
    .then(|| port.to_owned())
}

/// A Windows container id as a physical key, unless it is the null id or the
/// computer's own container ({00000000-0000-0000-FFFF-FFFFFFFFFFFF}), which
/// every non-removable device shares, so it proves nothing.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn usable_container_id(data1: u32, data2: u16, data3: u16, data4: [u8; 8]) -> Option<Vec<u8>> {
    let empty = data1 == 0 && data2 == 0 && data3 == 0;
    if empty && (data4 == [0; 8] || data4 == [0xff; 8]) {
        return None;
    }
    let mut key = Vec::with_capacity(16);
    key.extend_from_slice(&data1.to_le_bytes());
    key.extend_from_slice(&data2.to_le_bytes());
    key.extend_from_slice(&data3.to_le_bytes());
    key.extend_from_slice(&data4);
    Some(key)
}

#[cfg(target_os = "windows")]
fn physical_device_key(device: &HidDevice, _path: &CStr) -> Option<Vec<u8>> {
    let id = device.get_container_id().ok()?;
    usable_container_id(id.data1, id.data2, id.data3, id.data4)
}

#[cfg(target_os = "linux")]
fn physical_device_key(_device: &HidDevice, path: &CStr) -> Option<Vec<u8>> {
    let node = path.to_str().ok()?.strip_prefix("/dev/")?;
    let uevent = std::fs::read_to_string(format!("/sys/class/hidraw/{node}/device/uevent")).ok()?;
    usb_port_from_uevent(&uevent).map(String::into_bytes)
}

/// The identity of an open handle: vendor, product and serial from the handle
/// itself, the physical key from the handle (Windows) or its node (Linux).
#[cfg(any(target_os = "windows", target_os = "linux"))]
fn physical_identity(device: &HidDevice, path: &CStr) -> Option<PhysicalIdentity> {
    let info = device.get_device_info().ok()?;
    Some(PhysicalIdentity::new(
        info.vendor_id(),
        info.product_id(),
        info.serial_number(),
        physical_device_key(device, path)?,
    ))
}

/// Whether an enumerated interface may be a feature sibling of an open
/// device: the same vendor and product, not already open, and through the
/// same filters `list` applies (no security keys, no receiver children).
#[cfg_attr(not(any(target_os = "windows", target_os = "linux")), allow(dead_code))]
fn sibling_candidate(
    expected: &PhysicalIdentity,
    vendor_id: u16,
    product_id: u16,
    opened: bool,
    blocked: bool,
    receiver_child: bool,
) -> bool {
    vendor_id == expected.vendor_id
        && product_id == expected.product_id
        && !opened
        && !blocked
        && !receiver_child
}

/// Opens the other interfaces of an open device's physical device that
/// declare feature reports. Any failure skips that interface: this only ever
/// adds paths to try, so the device keeps working without it.
#[cfg(any(target_os = "windows", target_os = "linux"))]
fn scan_feature_siblings(api: &HidApi, open: &OpenDevice) -> Vec<SiblingPath> {
    let Some(expected) = open.paths.iter().find_map(|path| {
        let device = path.device.lock().ok()?;
        physical_identity(&device, &path.path)
    }) else {
        return Vec::new();
    };
    let opened: HashSet<&CStr> = open.paths.iter().map(|path| path.path.as_c_str()).collect();
    let blocked = security_key_paths(api);
    let mut seen = HashSet::new();
    let mut siblings = Vec::new();
    for info in api.device_list() {
        if !sibling_candidate(
            &expected,
            info.vendor_id(),
            info.product_id(),
            opened.contains(info.path()),
            blocked.contains(info.path()),
            is_receiver_child(info.path()),
        ) || !seen.insert(info.path().to_owned())
        {
            continue;
        }
        let Ok(device) = api.open_path(info.path()) else {
            continue;
        };
        if physical_identity(&device, info.path()).as_ref() != Some(&expected) {
            continue;
        }
        let mut descriptor = vec![0; MAX_REPORT_DESCRIPTOR_SIZE];
        let Ok(size) = device.get_report_descriptor(&mut descriptor) else {
            continue;
        };
        let Some((layout, vendor_page)) = sibling_layout(&descriptor[..size]) else {
            continue;
        };
        siblings.push(SiblingPath {
            device: Mutex::new(device),
            layout,
            vendor_page,
        });
    }
    siblings
}

/// A sibling's layout and whether it has a vendor-page collection, when its
/// descriptor parses, declares feature reports and holds no FIDO collection.
#[cfg(any(target_os = "windows", target_os = "linux"))]
fn sibling_layout(descriptor: &[u8]) -> Option<(ReportLayout, bool)> {
    let (collections, layout) = parse_report_descriptor(descriptor).ok()?;
    if has_security_key_collection(&collections) || layout.feature_bits.is_empty() {
        return None;
    }
    let vendor_page = collections
        .iter()
        .any(|collection| collection.usage_page >= VENDOR_USAGE_PAGE);
    Some((layout, vendor_page))
}

/// macOS and the libusb backends keep the opened paths only.
#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn scan_feature_siblings(_api: &HidApi, _open: &OpenDevice) -> Vec<SiblingPath> {
    Vec::new()
}

fn try_opened_paths(
    paths: &[OpenPath],
    indexes: Vec<usize>,
    attempt: &mut dyn FnMut(&Mutex<HidDevice>, &ReportLayout) -> ControlFlow<()>,
) -> ControlFlow<()> {
    for index in indexes {
        let path = &paths[index];
        attempt(&path.device, &path.layout)?;
    }
    ControlFlow::Continue(())
}

fn collection_interface(path: &[u8]) -> Option<String> {
    let path = std::str::from_utf8(path).ok()?.to_ascii_lowercase();
    let (hardware, rest) = path.strip_prefix(r"\\?\hid#")?.split_once('#')?;
    let (instance, _) = rest.split_once('#')?;
    let (hardware, _) = hardware.rsplit_once("&col")?;
    let (parent, _) = instance.rsplit_once('&')?;
    Some(format!("{hardware}#{parent}"))
}

fn inspect_candidate(
    api: &HidApi,
    key: String,
    infos: &[hidapi::DeviceInfo],
) -> Result<Candidate, String> {
    let primary = infos
        .first()
        .ok_or_else(|| "HID enumeration returned an empty interface".to_owned())?;
    let mut unique_paths = Vec::new();
    let mut seen_paths = HashSet::new();
    for info in infos {
        if seen_paths.insert(info.path().to_bytes().to_vec()) {
            unique_paths.push(info.path().to_owned());
        }
    }

    let inspect_descriptors = primary.vendor_id() != RAZER_VENDOR_ID;
    let mut collections = Vec::new();
    let mut paths = Vec::with_capacity(unique_paths.len());
    for path in unique_paths {
        let parsed = inspect_descriptors
            .then(|| {
                api.open_path(&path).ok().and_then(|device| {
                    let mut descriptor = vec![0; MAX_REPORT_DESCRIPTOR_SIZE];
                    let size = device.get_report_descriptor(&mut descriptor).ok()?;
                    parse_report_descriptor(&descriptor[..size]).ok()
                })
            })
            .flatten();
        let descriptor_known = parsed.is_some();
        let layout = if let Some((mut path_collections, path_layout)) = parsed {
            collections.append(&mut path_collections);
            path_layout
        } else {
            collections.extend(
                infos
                    .iter()
                    .filter(|info| info.path() == path.as_c_str())
                    .map(|info| CollectionInfo {
                        usage_page: info.usage_page(),
                        usage: info.usage(),
                        ..CollectionInfo::default()
                    }),
            );
            ReportLayout::default()
        };
        paths.push(CandidatePath {
            path,
            layout,
            descriptor_known,
        });
    }

    Ok(Candidate {
        paths,
        summary: DeviceSummary {
            key,
            vendor_id: primary.vendor_id(),
            product_id: primary.product_id(),
            product_name: infos
                .iter()
                .find_map(|info| info.product_string().filter(|name| !name.is_empty()))
                .unwrap_or("HID device")
                .to_owned(),
            collections,
        },
    })
}

#[derive(Clone, Default)]
struct GlobalState {
    usage_page: u16,
    report_id: u8,
    report_size: u32,
    report_count: u32,
}

#[derive(Default)]
struct CollectionNode {
    info: CollectionInfo,
    parent: Option<usize>,
}

/// Deepest collection nesting accepted. Real devices nest a handful deep; a
/// descriptor from a hostile device could otherwise nest ~1,400 deep and
/// drive the recursive tree building and serialization.
const MAX_COLLECTION_DEPTH: usize = 64;

fn parse_report_descriptor(bytes: &[u8]) -> Result<(Vec<CollectionInfo>, ReportLayout), String> {
    let mut globals = GlobalState::default();
    let mut global_stack = Vec::new();
    let mut usages = Vec::<u32>::new();
    let mut nodes = Vec::<CollectionNode>::new();
    let mut collections = Vec::<usize>::new();
    let mut roots = Vec::<usize>::new();
    let mut layout = ReportLayout::default();
    let mut offset = 0;

    while offset < bytes.len() {
        let prefix = bytes[offset];
        offset += 1;
        if prefix == 0xfe {
            if offset + 2 > bytes.len() {
                return Err("truncated long HID item".into());
            }
            let size = bytes[offset] as usize;
            offset += 2;
            if offset + size > bytes.len() {
                return Err("truncated long HID item payload".into());
            }
            offset += size;
            continue;
        }

        let size = match prefix & 0x03 {
            0 => 0,
            1 => 1,
            2 => 2,
            _ => 4,
        };
        if offset + size > bytes.len() {
            return Err("truncated HID item payload".into());
        }
        let data = &bytes[offset..offset + size];
        offset += size;
        let value = item_value(data);
        let item_type = (prefix >> 2) & 0x03;
        let tag = prefix >> 4;

        match (item_type, tag) {
            (1, 0) => globals.usage_page = value as u16,
            (1, 7) => globals.report_size = value,
            (1, 8) => globals.report_id = value as u8,
            (1, 9) => globals.report_count = value,
            (1, 10) => global_stack.push(globals.clone()),
            (1, 11) => {
                globals = global_stack
                    .pop()
                    .ok_or_else(|| "HID descriptor popped an empty global stack".to_owned())?;
            }
            (2, 0) => usages.push(if size == 4 && value > u16::MAX as u32 {
                value
            } else {
                (u32::from(globals.usage_page) << 16) | (value & 0xffff)
            }),
            (0, 10) => {
                if collections.len() >= MAX_COLLECTION_DEPTH {
                    return Err("HID descriptor nests collections too deep".into());
                }
                let usage = usages.first().copied().unwrap_or(0);
                let index = nodes.len();
                let parent = collections.last().copied();
                nodes.push(CollectionNode {
                    info: CollectionInfo {
                        usage_page: (usage >> 16) as u16,
                        usage: usage as u16,
                        ..CollectionInfo::default()
                    },
                    parent,
                });
                if parent.is_none() {
                    roots.push(index);
                }
                collections.push(index);
                usages.clear();
            }
            (0, 12) => {
                collections
                    .pop()
                    .ok_or_else(|| "HID descriptor closed an unopened collection".to_owned())?;
                usages.clear();
            }
            (0, 8 | 9 | 11) => {
                if let Some(collection) = collections.last().copied() {
                    let item = ReportItem {
                        report_size: globals.report_size,
                        report_count: globals.report_count,
                    };
                    let reports = match tag {
                        8 => &mut nodes[collection].info.input_reports,
                        9 => &mut nodes[collection].info.output_reports,
                        _ => &mut nodes[collection].info.feature_reports,
                    };
                    append_report(reports, globals.report_id, item);
                    let bits = usize::try_from(globals.report_size)
                        .unwrap_or(usize::MAX)
                        .saturating_mul(
                            usize::try_from(globals.report_count).unwrap_or(usize::MAX),
                        );
                    let totals = match tag {
                        8 => &mut layout.input_bits,
                        9 => &mut layout.output_bits,
                        11 => &mut layout.feature_bits,
                        _ => {
                            unreachable!("only input, output, and feature items reach this branch")
                        }
                    };
                    *totals.entry(globals.report_id).or_default() = totals
                        .get(&globals.report_id)
                        .copied()
                        .unwrap_or_default()
                        .saturating_add(bits);
                }
                usages.clear();
            }
            (0, _) => usages.clear(),
            _ => {}
        }
    }

    if !collections.is_empty() {
        return Err("HID descriptor ended inside a collection".into());
    }
    Ok((
        roots
            .into_iter()
            .map(|index| build_collection(index, &nodes))
            .collect(),
        layout,
    ))
}

fn item_value(bytes: &[u8]) -> u32 {
    bytes.iter().enumerate().fold(0, |value, (shift, byte)| {
        value | (u32::from(*byte) << (shift * 8))
    })
}

fn append_report(reports: &mut Vec<ReportInfo>, report_id: u8, item: ReportItem) {
    if let Some(report) = reports
        .iter_mut()
        .find(|report| report.report_id == report_id)
    {
        report.items.push(item);
    } else {
        reports.push(ReportInfo {
            report_id,
            items: vec![item],
        });
    }
}

fn build_collection(index: usize, nodes: &[CollectionNode]) -> CollectionInfo {
    let mut info = nodes[index].info.clone();
    info.children = nodes
        .iter()
        .enumerate()
        .filter_map(|(child, node)| (node.parent == Some(index)).then_some(child))
        .map(|child| build_collection(child, nodes))
        .collect();
    info
}

/// Bumped each time the machine resumes from sleep or hibernate.
static RESUME_GENERATION: AtomicU64 = AtomicU64::new(0);
static RESUME_WATCH: Once = Once::new();

/// A thread that sleeps in short steps can only wake late if the whole machine
/// was suspended, so a wall-clock jump well past the step means a resume. This
/// works the same on every platform without a hidden window or suspend hook.
fn start_resume_watch() {
    RESUME_WATCH.call_once(|| {
        let spawned = thread::Builder::new()
            .name("openmouse-resume-watch".into())
            .spawn(|| {
                let mut last = SystemTime::now();
                loop {
                    thread::sleep(RESUME_WATCH_STEP);
                    let now = SystemTime::now();
                    if now
                        .duration_since(last)
                        .is_ok_and(|gap| gap >= RESUME_GAP_THRESHOLD)
                    {
                        tracing::info!("System resume detected; HID handles will be reopened");
                        RESUME_GENERATION.fetch_add(1, Ordering::AcqRel);
                    }
                    last = now;
                }
            });
        if let Err(error) = spawned {
            tracing::warn!(%error, "could not start the resume watcher");
        }
    });
}

/// Whether the control panel currently has a HID websocket open. Bridge treats
/// that as a connected client, and background device reads stay off the
/// hardware meanwhile so they never interleave with the panel's traffic.
pub fn client_session_active() -> bool {
    ACTIVE_SESSIONS.load(Ordering::Acquire) > 0
}

struct ActiveSessionGuard;

impl ActiveSessionGuard {
    fn new() -> Self {
        let _access = crate::drivers::device_access();
        ACTIVE_SESSIONS.fetch_add(1, Ordering::AcqRel);
        Self
    }
}

impl Drop for ActiveSessionGuard {
    fn drop(&mut self) {
        ACTIVE_SESSIONS.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Counts as an open control-panel HID session until dropped. The counter is
/// process-wide, so callers hold `drivers::TEST_NATIVE` alongside it to keep
/// other native tests from observing the session.
#[cfg(test)]
pub(crate) fn hold_client_session() -> impl Sized {
    ActiveSessionGuard::new()
}

pub async fn serve(mut socket: WebSocket) {
    let session_id = NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed);
    let Ok(_active) = tokio::task::spawn_blocking(ActiveSessionGuard::new).await else {
        return;
    };
    tracing::info!(session_id, "HID websocket connected");
    start_resume_watch();
    let resume_seen = RESUME_GENERATION.load(Ordering::Acquire);
    let (input_tx, input_rx, failure_rx) = input_channel();
    // Opening the session enumerates HID devices under a process-wide lock:
    // keep that off the async workers.
    let opened = tokio::task::spawn_blocking(move || HidSession::new(session_id, input_tx))
        .await
        .unwrap_or_else(|error| Err(format!("native HID worker stopped: {error}")));
    let session = match opened {
        Ok(session) => Arc::new(Mutex::new(session)),
        Err(error) => {
            tracing::warn!(session_id, %error, "HID session initialization failed");
            let _ = send_json(&mut socket, &Reply::error(0, error)).await;
            return;
        }
    };

    run_session(
        &mut socket,
        session_id,
        &session,
        input_rx,
        failure_rx,
        resume_seen,
    )
    .await;

    // Dropping the session joins its reader threads. Every reader is told to
    // stop first, so this waits out about one read timeout in total.
    let _ = tokio::task::spawn_blocking(move || drop(session)).await;
    note_session_ended(session_id);
    tracing::info!(session_id, "HID websocket disconnected");
}

/// What the websocket loop runs commands on: the HID session, or a fake one
/// in tests.
trait SessionWorker: Send + 'static {
    fn execute(&mut self, message: ClientMessage) -> Reply;
    fn handle_resume(&mut self);
}

impl SessionWorker for HidSession {
    fn execute(&mut self, message: ClientMessage) -> Reply {
        HidSession::execute(self, message)
    }

    fn handle_resume(&mut self) {
        HidSession::handle_resume(self);
    }
}

/// The one command running on a blocking worker.
struct PendingCommand {
    request_id: u64,
    command: &'static str,
    task: tokio::task::JoinHandle<Reply>,
}

/// Resolves when the pending command finishes; never, when none is running.
async fn pending_reply(
    pending: &mut Option<PendingCommand>,
) -> Result<Reply, tokio::task::JoinError> {
    match pending {
        Some(pending) => (&mut pending.task).await,
        None => std::future::pending().await,
    }
}

/// The websocket loop. Commands run one at a time on a blocking worker, and
/// the next command is not read until the previous reply is sent, so replies
/// keep the order of their commands. While a command runs, input reports keep
/// going to the socket: a slow feature report exchange must not let a fast
/// reporting device fill the input queue. Every frame is sent from this one
/// task, so the socket never has two writers.
async fn run_session<W: SessionWorker>(
    socket: &mut impl HidSocket,
    session_id: u64,
    session: &Arc<Mutex<W>>,
    mut input_rx: mpsc::Receiver<InputReport>,
    mut failure_rx: watch::Receiver<bool>,
    mut resume_seen: u64,
) {
    let mut resume_tick = tokio::time::interval(Duration::from_secs(1));
    let mut pending: Option<PendingCommand> = None;
    loop {
        // Overflow ends the session first. Input reports come before
        // replies and new commands, so neither a slow command nor a stream
        // of commands can keep the input queue from draining until it
        // overflows.
        tokio::select! {
            biased;
            _ = failure_rx.changed() => {
                tracing::warn!(session_id, "HID input overflow; disconnecting session");
                break;
            }
            report = input_rx.recv() => {
                let Some(report) = report else { break };
                if send_json(socket, &report).await.is_err() { break; }
            }
            outcome = pending_reply(&mut pending), if pending.is_some() => {
                let Some(PendingCommand { request_id, command, .. }) = pending.take() else {
                    unreachable!("the branch only runs with a command pending");
                };
                let reply = match outcome {
                    Ok(reply) => reply,
                    Err(error) => {
                        tracing::error!(session_id, request_id, command, %error, "HID command panicked");
                        Reply::error(request_id, format!("native HID worker stopped: {error}"))
                    }
                };
                if send_json(socket, &reply).await.is_err() { break; }
            }
            message = socket.recv(), if pending.is_none() => {
                let Some(message) = message else { break };
                let Message::Text(text) = message else {
                    if matches!(message, Message::Close(_)) { break; }
                    continue;
                };
                let request = match serde_json::from_str::<ClientMessage>(&text) {
                    Ok(request) => request,
                    Err(error) => {
                        tracing::warn!(session_id, %error, "Invalid HID websocket command");
                        if send_json(socket, &Reply::error(0, format!("invalid HID command: {error}"))).await.is_err() {
                            break;
                        }
                        continue;
                    }
                };
                let request_id = request.id;
                let command = request.command.kind();
                let worker = Arc::clone(session);
                let task = tokio::task::spawn_blocking(move || {
                    // A panicked command is answered with its own error. Left
                    // poisoned, the lock would fail every later command, `list` too.
                    worker
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .execute(request)
                });
                pending = Some(PendingCommand { request_id, command, task });
            }
            _ = resume_tick.tick(), if pending.is_none() => {
                let generation = RESUME_GENERATION.load(Ordering::Acquire);
                if generation != resume_seen {
                    resume_seen = generation;
                    let worker = Arc::clone(session);
                    let _ = tokio::task::spawn_blocking(move || {
                        worker
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .handle_resume();
                    }).await;
                }
            }
        }
    }
    // A command still running keeps the hardware busy: let it finish before
    // the session counts as closed, as when commands were awaited inline.
    if let Some(pending) = pending {
        let _ = pending.task.await;
    }
}

trait HidSocket {
    fn send(
        &mut self,
        message: Message,
    ) -> impl std::future::Future<Output = Result<(), String>> + Send;

    /// The next frame from the client; `None` once the socket is closed or
    /// failed.
    fn recv(&mut self) -> impl std::future::Future<Output = Option<Message>> + Send;
}

impl HidSocket for WebSocket {
    async fn send(&mut self, message: Message) -> Result<(), String> {
        WebSocket::send(self, message)
            .await
            .map_err(|error| error.to_string())
    }

    async fn recv(&mut self) -> Option<Message> {
        WebSocket::recv(self).await?.ok()
    }
}

async fn send_json(socket: &mut impl HidSocket, value: &impl Serialize) -> Result<(), String> {
    let json = serde_json::to_string(value).map_err(|error| error.to_string())?;
    tokio::time::timeout(SOCKET_SEND_TIMEOUT, socket.send(Message::Text(json.into())))
        .await
        .map_err(|_| "HID socket send deadline exceeded; session disconnected".to_owned())?
}

#[cfg(test)]
mod tests {
    #[test]
    fn device_keys_are_stable_and_never_carry_the_serial() {
        let receiver = grouped_device_key(0x046d, 0xc54d, b"/dev/hidraw2", Some("375D35803234"));
        let key = stable_device_key(&receiver);
        // Same device, another session or interface path: same key.
        assert_eq!(
            stable_device_key(&grouped_device_key(
                0x046d,
                0xc54d,
                b"/dev/hidraw4",
                Some("375D35803234")
            )),
            key
        );
        assert!(key.starts_with("hid-046dc54d-"), "{key}");
        assert!(!key.contains("375D35803234"));
        // Another receiver of the same model.
        assert_ne!(
            stable_device_key(&grouped_device_key(
                0x046d,
                0xc54d,
                b"/dev/hidraw2",
                Some("0000AAAA")
            )),
            key
        );
    }

    #[test]
    fn a_taken_key_gets_a_counter() {
        let (input_tx, _, _) = input_channel();
        let mut session = HidSession::new(u64::MAX - 41_140, input_tx).unwrap();
        session
            .device_keys
            .insert(b"a".to_vec(), "hid-12345678-00000001".into());
        assert_eq!(
            session.unused_key("hid-12345678-00000001".into()),
            "hid-12345678-00000001-2"
        );
        assert_eq!(session.unused_key("hid-other".into()), "hid-other");
    }

    #[test]
    fn only_the_kernels_receiver_children_are_left_out() {
        use super::is_receiver_child_uevent as child;
        // A mouse paired through a Logitech receiver, split out by hid-logitech-dj.
        assert!(child(
            "DRIVER=logitech-hidpp-device\nHID_ID=0003:0000046D:000040B7\n\
             HID_NAME=Logitech PRO X 2c\nHID_PHYS=usb-0000:00:14.0-9/input2:1\n"
        ));
        // The receiver itself.
        assert!(!child(
            "DRIVER=logitech-djreceiver\nHID_ID=0003:0000046D:0000C54D\n\
             HID_PHYS=usb-0000:00:14.0-9/input2\n"
        ));
        // A wired Logitech mouse on the same driver.
        assert!(!child(
            "DRIVER=logitech-hidpp-device\nHID_PHYS=usb-0000:00:14.0-3/input1\n"
        ));
        // A Bluetooth one: the physical path is an address.
        assert!(!child(
            "DRIVER=logitech-hidpp-device\nHID_ID=0005:0000046D:0000B023\n\
             HID_PHYS=aa:bb:cc:dd:ee:ff\n"
        ));
    }

    /// macOS seizes devices opened in exclusive mode, which stops a mouse's
    /// pointer collection from moving the cursor while Bridge holds it.
    #[cfg(target_os = "macos")]
    #[test]
    fn macos_opens_devices_without_seizing_them() {
        // Initialising IOHID traps on the headless macos-14 release runner;
        // Bridge CI still covers this on macOS 26.
        if std::env::var_os("OPENMOUSE_SKIP_IOHID_TEST").is_some() {
            return;
        }
        let api = new_api().expect("HID subsystem should initialize");
        assert!(!api.get_open_exclusive());
    }

    use super::*;

    /// Logitech receivers also split their report paths across several HID
    /// entries; used only to pin the grouping rule for them here.
    const LOGITECH_VENDOR_ID: u16 = 0x046D;

    struct FakeSocket {
        stalled: bool,
        frames: Vec<Message>,
    }

    impl HidSocket for FakeSocket {
        async fn send(&mut self, message: Message) -> Result<(), String> {
            if self.stalled {
                std::future::pending::<()>().await;
            }
            self.frames.push(message);
            Ok(())
        }

        async fn recv(&mut self) -> Option<Message> {
            std::future::pending().await
        }
    }

    /// A client: frames to Bridge come from `incoming`, every frame Bridge
    /// sends is parsed onto `outgoing`.
    struct ChannelSocket {
        incoming: mpsc::UnboundedReceiver<Message>,
        outgoing: mpsc::UnboundedSender<serde_json::Value>,
    }

    impl HidSocket for ChannelSocket {
        async fn send(&mut self, message: Message) -> Result<(), String> {
            let Message::Text(text) = message else {
                return Err("expected a text frame".into());
            };
            let value = serde_json::from_str(&text).map_err(|error| error.to_string())?;
            self.outgoing.send(value).map_err(|error| error.to_string())
        }

        async fn recv(&mut self) -> Option<Message> {
            self.incoming.recv().await
        }
    }

    /// Command 1 blocks until released, standing in for a slow feature report
    /// exchange; the others answer at once.
    struct SlowWorker {
        started: mpsc::UnboundedSender<u64>,
        release: std::sync::mpsc::Receiver<()>,
    }

    impl SessionWorker for SlowWorker {
        fn execute(&mut self, message: ClientMessage) -> Reply {
            self.started.send(message.id).unwrap();
            if message.id == 1 {
                self.release.recv().unwrap();
            }
            Reply::ok(message.id)
        }

        fn handle_resume(&mut self) {}
    }

    struct SessionHarness {
        input_tx: InputSender,
        client_tx: mpsc::UnboundedSender<Message>,
        frames: mpsc::UnboundedReceiver<serde_json::Value>,
        started: mpsc::UnboundedReceiver<u64>,
        release: std::sync::mpsc::Sender<()>,
        session: tokio::task::JoinHandle<()>,
    }

    fn start_session() -> SessionHarness {
        let (input_tx, input_rx, failure_rx) = input_channel();
        let (client_tx, incoming) = mpsc::unbounded_channel();
        let (outgoing, frames) = mpsc::unbounded_channel();
        let (started_tx, started) = mpsc::unbounded_channel();
        let (release, release_rx) = std::sync::mpsc::channel();
        let worker = Arc::new(Mutex::new(SlowWorker {
            started: started_tx,
            release: release_rx,
        }));
        let resume_seen = RESUME_GENERATION.load(Ordering::Acquire);
        let session = tokio::spawn(async move {
            let mut socket = ChannelSocket { incoming, outgoing };
            run_session(&mut socket, 0, &worker, input_rx, failure_rx, resume_seen).await;
        });
        SessionHarness {
            input_tx,
            client_tx,
            frames,
            started,
            release,
            session,
        }
    }

    fn list_command(id: u64) -> Message {
        Message::Text(format!(r#"{{"id":{id},"type":"list","vendorIds":[]}}"#).into())
    }

    fn input_report(byte: u8) -> InputReport {
        InputReport {
            kind: "inputreport",
            device: "hid-test".into(),
            report_id: 2,
            data: vec![byte],
        }
    }

    async fn next_frame(
        frames: &mut mpsc::UnboundedReceiver<serde_json::Value>,
    ) -> serde_json::Value {
        tokio::time::timeout(Duration::from_secs(2), frames.recv())
            .await
            .expect("frame within 2 s")
            .expect("session still sending")
    }

    #[tokio::test]
    async fn input_reports_flow_while_a_slow_command_runs_and_replies_stay_ordered() {
        let mut harness = start_session();
        for id in 1..=3 {
            harness.client_tx.send(list_command(id)).unwrap();
        }
        assert_eq!(harness.started.recv().await, Some(1));
        for byte in 0..3 {
            harness.input_tx.send(input_report(byte)).unwrap();
        }
        // Command 1 is still running: its input reports reach the client anyway.
        for byte in 0..3 {
            let frame = next_frame(&mut harness.frames).await;
            assert_eq!(frame["type"], "inputreport", "{frame}");
            assert_eq!(frame["data"], serde_json::json!([byte]));
        }
        // Commands stay one at a time: 2 is not read before 1 is answered.
        assert!(harness.started.try_recv().is_err());
        harness.release.send(()).unwrap();
        for id in 1..=3 {
            let frame = next_frame(&mut harness.frames).await;
            assert_eq!(frame["id"], id, "{frame}");
            assert_eq!(frame["ok"], true);
        }
        assert_eq!(harness.started.recv().await, Some(2));
        assert_eq!(harness.started.recv().await, Some(3));
        drop(harness.client_tx);
        tokio::time::timeout(Duration::from_secs(2), harness.session)
            .await
            .expect("a closed client ends the session")
            .unwrap();
    }

    #[tokio::test]
    async fn overflow_during_a_slow_command_still_ends_the_session() {
        let mut harness = start_session();
        harness.client_tx.send(list_command(1)).unwrap();
        assert_eq!(harness.started.recv().await, Some(1));
        harness.input_tx.failed.send_replace(true);
        // The session waits for the running command before it counts as closed.
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(!harness.session.is_finished());
        harness.release.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(2), harness.session)
            .await
            .expect("overflow ends the session")
            .unwrap();
        // Its reply is dropped with the session; nothing else was sent.
        assert!(harness.frames.try_recv().is_err());
    }

    #[test]
    fn input_overflow_is_terminal_and_never_silently_accepts_reports() {
        let (tx, rx, failure) = input_channel();
        let report = || InputReport {
            kind: "inputreport",
            device: "fake".into(),
            report_id: 1,
            data: vec![7],
        };
        for _ in 0..INPUT_QUEUE_CAPACITY {
            tx.send(report()).unwrap();
        }
        assert!(tx.send(report()).is_err());
        assert!(*failure.borrow());
        assert_eq!(rx.len(), INPUT_QUEUE_CAPACITY);
        assert!(tx.send(report()).is_err());
    }

    #[tokio::test]
    async fn protocol_replies_are_delivered_or_fail_at_the_send_deadline() {
        let mut fast = FakeSocket {
            stalled: false,
            frames: Vec::new(),
        };
        send_json(&mut fast, &Reply::ok(77)).await.unwrap();
        let Message::Text(frame) = &fast.frames[0] else {
            panic!("expected text reply");
        };
        let reply: serde_json::Value = serde_json::from_str(frame).unwrap();
        assert_eq!(reply["id"], 77);
        assert_eq!(reply["ok"], true);
        let mut slow = FakeSocket {
            stalled: true,
            frames: Vec::new(),
        };
        let outcome = tokio::time::timeout(
            SOCKET_SEND_TIMEOUT + Duration::from_secs(1),
            send_json(&mut slow, &Reply::ok(78)),
        )
        .await
        .expect("send must return its explicit deadline error");
        assert!(outcome.unwrap_err().contains("deadline"));
        assert!(slow.frames.is_empty());
    }

    #[test]
    fn descriptor_becomes_webhid_collections_and_report_lengths() {
        let descriptor = [
            0x05, 0x01, // Usage Page (Generic Desktop)
            0x09, 0x02, // Usage (Mouse)
            0xa1, 0x01, // Collection (Application)
            0x85, 0x08, // Report ID 8
            0x75, 0x08, // Report Size 8
            0x95, 0x10, // Report Count 16
            0x81, 0x02, // Input
            0x91, 0x02, // Output
            0x85, 0x05, // Report ID 5
            0x95, 0x40, // Report Count 64
            0xb1, 0x02, // Feature
            0xc0, // End Collection
        ];
        let (collections, layout) = parse_report_descriptor(&descriptor).unwrap();
        assert_eq!(collections.len(), 1);
        assert_eq!(collections[0].usage_page, 1);
        assert_eq!(collections[0].usage, 2);
        assert_eq!(collections[0].input_reports[0].report_id, 8);
        assert_eq!(collections[0].input_reports[0].items[0].report_count, 16);
        assert_eq!(collections[0].output_reports[0].report_id, 8);
        assert_eq!(layout.output_bits.get(&8), Some(&128));
        assert_eq!(collections[0].feature_reports[0].report_id, 5);
        assert_eq!(layout.feature_buffer_len(5).unwrap(), 65);
        assert!(layout.input_uses_report_ids());
    }

    #[test]
    fn opaque_interfaces_do_not_merge_without_verified_physical_identity() {
        assert_ne!(
            grouped_device_key(0x046d, 0xc54d, b"opaque-A", None),
            grouped_device_key(0x046d, 0xc54d, b"opaque-B", None)
        );
        assert_eq!(
            grouped_device_key(0x046d, 0xc54d, b"opaque-A", None),
            grouped_device_key(0x046d, 0xc54d, b"opaque-A", None)
        );
        assert_ne!(
            grouped_device_key(0x046d, 0xc54d, b"opaque-A", None),
            grouped_device_key(0x1532, 0xc54d, b"opaque-A", None)
        );
        assert_ne!(
            grouped_device_key(0x046d, 0xc54d, b"opaque-A", Some("")),
            grouped_device_key(0x046d, 0xc54d, b"opaque-B", Some("")),
            "an empty serial is no identity"
        );
    }

    #[test]
    fn collections_of_one_serial_bearing_device_group_together() {
        assert_eq!(
            grouped_device_key(0x1532, 0x00b0, b"col01", Some("receiver-123")),
            grouped_device_key(0x1532, 0x00b0, b"col02", Some("receiver-123"))
        );
        assert_ne!(
            grouped_device_key(0x1532, 0x00b0, b"col01", Some("receiver-123")),
            grouped_device_key(0x1532, 0x00b0, b"col01", Some("receiver-456"))
        );
        // A serial that happens to equal another device's path is still a different identity.
        assert_ne!(
            grouped_device_key(0x1532, 0x00b0, b"same", Some("same")),
            grouped_device_key(0x1532, 0x00b0, b"same", None)
        );
    }

    /// A blocking reader: each read either returns the next scripted report
    /// or waits out the timeout and returns 0, like `read_timeout` on an
    /// owned handle with no input pending.
    fn scripted_read(
        mut reports: Vec<Vec<u8>>,
        timeout: Duration,
        reads: Arc<AtomicUsize>,
    ) -> impl FnMut(&mut [u8]) -> Result<usize, String> {
        reports.reverse();
        move |buffer| {
            reads.fetch_add(1, Ordering::Relaxed);
            match reports.pop() {
                Some(report) => {
                    buffer[..report.len()].copy_from_slice(&report);
                    Ok(report.len())
                }
                None => {
                    thread::sleep(timeout);
                    Ok(0)
                }
            }
        }
    }

    #[test]
    fn reader_forwards_reports_and_stops_within_one_read_timeout() {
        let (tx, mut rx, failure) = input_channel();
        let stop = Arc::new(AtomicBool::new(false));
        let reads = Arc::new(AtomicUsize::new(0));
        let timeout = Duration::from_millis(50);
        let read = scripted_read(
            vec![vec![3, 10, 11], vec![4, 12]],
            timeout,
            Arc::clone(&reads),
        );
        let thread_stop = Arc::clone(&stop);
        let reader = thread::spawn(move || {
            run_reader(read, &thread_stop, &tx, "hid-test", 0, 8, true);
        });
        let first = rx.blocking_recv().unwrap();
        assert_eq!((first.report_id, first.data), (3, vec![10, 11]));
        let second = rx.blocking_recv().unwrap();
        assert_eq!((second.device.as_str(), second.report_id), ("hid-test", 4));
        assert_eq!(second.data, vec![12]);
        // Idle: a blocking read waits out its timeout instead of spinning.
        thread::sleep(timeout * 4);
        assert!(
            reads.load(Ordering::Relaxed) <= 2 + 5,
            "reader spun while idle"
        );
        let asked = Instant::now();
        stop.store(true, Ordering::Release);
        reader.join().unwrap();
        assert!(asked.elapsed() < timeout * 4, "{:?}", asked.elapsed());
        assert!(!*failure.borrow());
    }

    #[test]
    fn reader_without_report_ids_keeps_the_whole_buffer_and_stops_on_error() {
        let (tx, mut rx, _failure) = input_channel();
        let stop = AtomicBool::new(false);
        let mut calls = 0;
        run_reader(
            |buffer: &mut [u8]| {
                calls += 1;
                if calls == 1 {
                    buffer[..2].copy_from_slice(&[9, 8]);
                    Ok(2)
                } else {
                    Err("device unplugged".into())
                }
            },
            &stop,
            &tx,
            "hid-test",
            1,
            8,
            false,
        );
        let report = rx.try_recv().unwrap();
        assert_eq!((report.report_id, report.data), (0, vec![9, 8]));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn reader_stops_itself_when_the_input_queue_overflows() {
        let (tx, _rx, failure) = input_channel();
        let stop = AtomicBool::new(false);
        run_reader(
            |buffer: &mut [u8]| {
                buffer[0] = 1;
                Ok(2)
            },
            &stop,
            &tx,
            "hid-test",
            0,
            8,
            true,
        );
        assert!(stop.load(Ordering::Acquire));
        assert!(*failure.borrow());
    }

    #[cfg(any(target_os = "windows", target_os = "linux"))]
    #[test]
    fn a_reader_handle_that_reaches_another_device_is_refused() {
        let mouse_descriptor = vec![
            0x05, 0x01, 0x09, 0x02, 0xa1, 0x01, 0x85, 0x08, 0x75, 0x08, 0x95, 0x10, 0x81, 0x02,
            0xc0,
        ];
        let identity = |descriptor: Vec<u8>| HandleIdentity {
            vendor_id: 0x1532,
            product_id: 0x00b6,
            serial: Some("A1".into()),
            interface: 2,
            descriptor,
        };
        let expected = identity(mouse_descriptor.clone());
        assert!(check_reader_identity(&expected, &identity(mouse_descriptor.clone())).is_ok());

        // Same interface number, another device behind the reused node.
        let mut other = identity(mouse_descriptor.clone());
        other.product_id = 0x0083;
        assert!(check_reader_identity(&expected, &other).is_err());
        let mut other = identity(mouse_descriptor.clone());
        other.serial = Some("B2".into());
        assert!(check_reader_identity(&expected, &other).is_err());
        let mut other = identity(mouse_descriptor.clone());
        other.interface = 0;
        assert!(check_reader_identity(&expected, &other).is_err());

        // A security key took the node: refused for what it is, even if the
        // rest were to match.
        let fido_descriptor = vec![
            0x06, 0xd0, 0xf1, 0x09, 0x01, 0xa1, 0x01, 0x09, 0x20, 0x15, 0x00, 0x26, 0xff, 0x00,
            0x75, 0x08, 0x95, 0x40, 0x81, 0x02, 0xc0,
        ];
        let key = identity(fido_descriptor.clone());
        let error = check_reader_identity(&identity(fido_descriptor), &key).unwrap_err();
        assert!(error.contains("security key"), "{error}");
        assert!(check_reader_identity(&expected, &key).is_err());
    }

    #[test]
    fn ending_a_session_stops_every_reader_before_joining_any() {
        let (input_tx, _, _) = input_channel();
        let mut session = HidSession::new(u64::MAX - 41_141, input_tx).unwrap();
        let flags = (0..3)
            .map(|_| Arc::new(AtomicBool::new(false)))
            .collect::<Vec<_>>();
        let all_saw_every_stop = Arc::new(AtomicBool::new(true));
        for (index, flag) in flags.iter().enumerate() {
            let own = Arc::clone(flag);
            let others = flags.clone();
            let verdict = Arc::clone(&all_saw_every_stop);
            // Each reader exits once asked, but only after every other reader
            // was asked too (or 2 s pass): a session that joins one reader
            // before telling the next to stop makes the first one time out.
            let reader = thread::spawn(move || {
                while !own.load(Ordering::Acquire) {
                    thread::sleep(Duration::from_millis(5));
                }
                let deadline = Instant::now() + Duration::from_secs(2);
                while !others.iter().all(|flag| flag.load(Ordering::Acquire)) {
                    if Instant::now() >= deadline {
                        verdict.store(false, Ordering::Release);
                        return;
                    }
                    thread::sleep(Duration::from_millis(5));
                }
            });
            session.open_devices.insert(
                format!("hid-test-{index}"),
                OpenDevice {
                    paths: Vec::new(),
                    vendor_id: 0,
                    listening: true,
                    stop: Some(Arc::clone(flag)),
                    readers: vec![reader],
                    feature_siblings: OnceLock::new(),
                },
            );
        }
        drop(session);
        assert!(all_saw_every_stop.load(Ordering::Acquire));
    }

    #[test]
    fn a_report_goes_to_the_paths_that_declare_it_first() {
        let mut vendor = ReportLayout::default();
        vendor.feature_bits.insert(0x11, 160);
        vendor.output_bits.insert(0x10, 48);
        let mouse = ReportLayout::default();
        let mut second = ReportLayout::default();
        second.feature_bits.insert(0x11, 160);
        let layouts = [&mouse, &vendor, &second];
        assert_eq!(report_paths(&layouts, 0x11, true), vec![1, 2]);
        assert_eq!(report_paths(&layouts, 0x10, false), vec![1]);
        // Undeclared (a Razer path without parsed descriptors): every path is a candidate, tried in order.
        assert_eq!(report_paths(&layouts, 0x00, true), vec![0, 1, 2]);
        assert!(report_paths(&[], 0x11, true).is_empty());
    }

    fn features(ids: &[u8]) -> ReportLayout {
        let mut layout = ReportLayout::default();
        for id in ids {
            layout.feature_bits.insert(*id, 90 * 8);
        }
        layout
    }

    #[test]
    fn a_feature_report_declared_by_an_opened_path_never_reaches_a_sibling() {
        let mouse = ReportLayout::default();
        let vendor = features(&[0]);
        assert_eq!(
            feature_route(&[(&mouse, true), (&vendor, true)], 0, false),
            FeatureRoute::Declared(vec![1])
        );
        assert_eq!(
            feature_route(&[(&vendor, true)], 0, true),
            FeatureRoute::Declared(vec![0])
        );
    }

    #[test]
    fn siblings_go_first_only_when_the_opened_descriptors_have_no_feature_reports() {
        let mouse = ReportLayout::default();
        let other = features(&[5]);
        // Known and featureless (#16's 0x1:2 interface): siblings first.
        assert_eq!(
            feature_route(&[(&mouse, true)], 0, false),
            FeatureRoute::SiblingsFirst
        );
        // Unread descriptors (Razer) and vendor quirks keep today's paths first.
        assert_eq!(
            feature_route(&[(&mouse, false)], 0, false),
            FeatureRoute::OpenedFirst
        );
        assert_eq!(
            feature_route(&[(&mouse, true)], 0, true),
            FeatureRoute::OpenedFirst
        );
        // An opened path with other feature reports is feature capable: it stays first.
        assert_eq!(
            feature_route(&[(&mouse, true), (&other, true)], 0, false),
            FeatureRoute::OpenedFirst
        );
        assert_eq!(feature_route(&[], 0, false), FeatureRoute::OpenedFirst);
    }

    #[test]
    fn siblings_are_tried_vendor_page_first_and_only_when_they_declare_the_report() {
        let consumer = features(&[0]);
        let vendor = features(&[0, 2]);
        let numbered = features(&[2, 3]);
        let featureless = ReportLayout::default();
        let siblings = [
            (&consumer, false),
            (&featureless, true),
            (&vendor, true),
            (&numbered, true),
        ];
        // Unnumbered vendor feature report (one shape #16 may have): routed, vendor first.
        assert_eq!(sibling_order(&siblings, 0), vec![2, 0]);
        // Numbered only: report 0 finds no sibling, so the opened paths stay all there is.
        assert!(sibling_order(&[(&numbered, true)], 0).is_empty());
        assert_eq!(sibling_order(&siblings, 3), vec![3]);
        assert!(sibling_order(&[], 0).is_empty());
    }

    #[test]
    fn a_hidraw_node_is_placed_by_its_usb_port_only() {
        let uevent = |phys: &str| {
            format!("DRIVER=razerkbd\nHID_ID=0003:00001532:0000026B\nHID_PHYS={phys}\nHID_UNIQ=\n")
        };
        let input2 = usb_port_from_uevent(&uevent("usb-0000:00:14.0-2/input2"));
        let input3 = usb_port_from_uevent(&uevent("usb-0000:00:14.0-2/input3"));
        assert_eq!(input2.as_deref(), Some("usb-0000:00:14.0-2"));
        assert_eq!(input2, input3, "interfaces of one device share a port");
        assert_ne!(
            input2,
            usb_port_from_uevent(&uevent("usb-0000:00:14.0-3/input2")),
            "an identical device on another port is another device"
        );
        // Bluetooth phys is the adapter, shared by every device: no siblings.
        assert_eq!(usb_port_from_uevent(&uevent("a4:c3:f0:12:34:56")), None);
        // A receiver child (input2:1) and malformed values: no siblings.
        assert_eq!(
            usb_port_from_uevent(&uevent("usb-0000:00:14.0-9/input2:1")),
            None
        );
        assert_eq!(
            usb_port_from_uevent(&uevent("usb-0000:00:14.0-9/input")),
            None
        );
        assert_eq!(usb_port_from_uevent("DRIVER=hid-generic\n"), None);
    }

    #[test]
    fn the_computer_container_and_the_null_container_prove_nothing() {
        assert_eq!(usable_container_id(0, 0, 0, [0; 8]), None);
        assert_eq!(usable_container_id(0, 0, 0, [0xff; 8]), None);
        let device = usable_container_id(0x1234_5678, 0x9abc, 0xdef0, [1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(device.as_ref().map(Vec::len), Some(16));
        assert_ne!(
            device,
            usable_container_id(0x1234_5678, 0x9abc, 0xdef0, [1, 2, 3, 4, 5, 6, 7, 9])
        );
    }

    #[test]
    fn identical_devices_without_a_serial_never_become_siblings() {
        let port = |name: &str| name.as_bytes().to_vec();
        let opened = PhysicalIdentity::new(0x1532, 0x026b, None, port("usb-1-2"));
        assert_eq!(
            opened,
            PhysicalIdentity::new(0x1532, 0x026b, Some(""), port("usb-1-2")),
            "an empty serial is no serial"
        );
        assert_ne!(
            opened,
            PhysicalIdentity::new(0x1532, 0x026b, None, port("usb-1-3"))
        );
        assert_ne!(
            opened,
            PhysicalIdentity::new(0x1532, 0x026b, Some("A1"), port("usb-1-2"))
        );
        assert_ne!(
            opened,
            PhysicalIdentity::new(0x1532, 0x026c, None, port("usb-1-2"))
        );
    }

    #[test]
    fn siblings_pass_the_same_filters_as_the_device_list() {
        let expected = PhysicalIdentity::new(0x1532, 0x026b, None, Vec::new());
        assert!(sibling_candidate(
            &expected, 0x1532, 0x026b, false, false, false
        ));
        assert!(
            !sibling_candidate(&expected, 0x1532, 0x026b, true, false, false),
            "already open"
        );
        assert!(
            !sibling_candidate(&expected, 0x1532, 0x026b, false, true, false),
            "security key"
        );
        assert!(
            !sibling_candidate(&expected, 0x1532, 0x026b, false, false, true),
            "receiver child"
        );
        assert!(!sibling_candidate(
            &expected, 0x1532, 0x0084, false, false, false
        ));
        assert!(!sibling_candidate(
            &expected, 0x046d, 0x026b, false, false, false
        ));
    }

    #[cfg(any(target_os = "windows", target_os = "linux"))]
    #[test]
    fn a_sibling_must_declare_feature_reports_and_hold_no_security_key() {
        let vendor = [
            0x06, 0x00, 0xff, // Usage Page 0xff00
            0x09, 0x01, // Usage 1
            0xa1, 0x01, // Collection (Application)
            0x75, 0x08, // Report Size 8
            0x95, 0x5a, // Report Count 90
            0xb1, 0x02, // Feature (unnumbered)
            0xc0, // End Collection
        ];
        let (layout, vendor_page) = sibling_layout(&vendor).expect("a vendor feature sibling");
        assert!(vendor_page);
        assert_eq!(layout.feature_buffer_len(0).unwrap(), 91);
        let mut fido = vendor;
        fido[1] = 0xd0;
        fido[2] = 0xf1; // Usage Page 0xf1d0, still declaring feature report 0
        assert!(sibling_layout(&fido).is_none());
        let mouse_without_features = [
            0x05, 0x01, 0x09, 0x02, 0xa1, 0x01, 0x75, 0x08, 0x95, 0x04, 0x81, 0x02, 0xc0,
        ];
        assert!(sibling_layout(&mouse_without_features).is_none());
        let consumer = [
            0x05, 0x0c, 0x09, 0x01, 0xa1, 0x01, 0x75, 0x08, 0x95, 0x04, 0xb1, 0x02, 0xc0,
        ];
        assert_eq!(
            sibling_layout(&consumer).map(|(_, vendor)| vendor),
            Some(false)
        );
    }

    #[test]
    fn concurrent_enumeration_does_not_crash() {
        // Unserialised hidapi enumeration crashes macOS with SIGTRAP, only now
        // and then, so this hammers it from several threads.
        let threads: Vec<_> = (0..6)
            .map(|_| {
                std::thread::spawn(|| {
                    for _ in 0..12 {
                        let mut api = new_api().expect("HID subsystem should initialize");
                        refresh_api(&mut api).expect("refresh");
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().expect("enumeration thread");
        }
    }

    #[test]
    fn report_payload_never_reads_past_the_buffer() {
        // A full unnumbered Razer read: hidapi on Windows reports 92 bytes.
        let full = [0; RAZER_FEATURE_BUFFER_LEN];
        assert_eq!(
            report_payload(&full, RAZER_FEATURE_BUFFER_LEN + 1).len(),
            RAZER_FEATURE_BUFFER_LEN - 1
        );
        assert_eq!(report_payload(&[5, 1, 2, 3], 3), vec![1, 2]);
        assert!(report_payload(&full, 0).is_empty());
    }

    #[test]
    fn active_devices_prefer_the_most_recently_opened_device() {
        // A session id no other test uses, so the shared registry is ours.
        let session = u64::MAX - 41_137;
        note_session_ended(session);
        let summary = |key: &str, name: &str| DeviceSummary {
            key: key.into(),
            vendor_id: RAZER_VENDOR_ID,
            product_id: 0x00b0,
            product_name: name.into(),
            collections: Vec::new(),
        };
        note_device_opened(session, &summary("hid-1", "PRO SUPERLIGHT 2C"));
        note_device_opened(session, &summary("hid-2", "DeathAdder V3"));
        let names = active_devices()
            .iter()
            .map(|device| device.name.clone())
            .collect::<Vec<_>>();
        let newer = names.iter().position(|name| name == "DeathAdder V3");
        let older = names.iter().position(|name| name == "PRO SUPERLIGHT 2C");
        assert!(newer.is_some() && older.is_some() && newer < older);

        // Closing the current device falls back to the previous one, and a
        // dead session leaves no stale entry behind.
        note_device_closed(session, "hid-2");
        let names = active_devices()
            .iter()
            .map(|device| device.name.clone())
            .collect::<Vec<_>>();
        assert!(!names.iter().any(|name| name == "DeathAdder V3"));
        assert!(names.iter().any(|name| name == "PRO SUPERLIGHT 2C"));
        note_session_ended(session);
        assert!(
            active_devices()
                .iter()
                .all(|device| device.name != "PRO SUPERLIGHT 2C")
        );
    }

    #[test]
    fn inventory_removal_cleans_handles_and_preserves_other_sessions() {
        let session_id = u64::MAX - 41_138;
        let other_session_id = u64::MAX - 41_139;
        let (input_tx, _, _) = input_channel();
        let mut session = HidSession::new(session_id, input_tx).unwrap();
        for (key, vendor_id, name) in [
            ("hid-1", RAZER_VENDOR_ID, "unplugged-test-mouse"),
            ("hid-2", LOGITECH_VENDOR_ID, "remaining-test-mouse"),
        ] {
            let summary = DeviceSummary {
                key: key.into(),
                vendor_id,
                product_id: 1,
                product_name: name.into(),
                collections: Vec::new(),
            };
            session
                .device_keys
                .insert(key.as_bytes().to_vec(), key.into());
            session.candidates.insert(
                key.into(),
                Candidate {
                    paths: Vec::new(),
                    summary: summary.clone(),
                },
            );
            session.open_devices.insert(
                key.into(),
                OpenDevice {
                    paths: Vec::new(),
                    vendor_id,
                    listening: false,
                    stop: None,
                    readers: Vec::new(),
                    feature_siblings: OnceLock::new(),
                },
            );
            session.listeners.insert(key.into());
            note_device_opened(session_id, &summary);
            if key == "hid-1" {
                note_device_opened(other_session_id, &summary);
            }
        }

        session
            .reconcile_inventory(&HashSet::from([b"hid-2".to_vec()]))
            .unwrap();
        assert!(!session.open_devices.contains_key("hid-1"));
        assert!(!session.candidates.contains_key("hid-1"));
        assert!(!session.listeners.contains("hid-1"));
        assert!(!session.device_keys.contains_key(b"hid-1".as_slice()));
        assert!(session.open("hid-1").is_err());
        assert!(session.open_devices.contains_key("hid-2"));
        {
            let registry = active_device_registry().lock().unwrap();
            assert!(!registry[&session_id].contains_key("hid-1"));
            assert!(registry[&session_id].contains_key("hid-2"));
            assert!(registry[&other_session_id].contains_key("hid-1"));
        }
        // Even an empty vendor filter must refresh the inventory and retire
        // the remaining synthetic device, while returning no devices.
        assert!(session.list(Vec::new()).unwrap().is_empty());
        assert!(
            !active_device_registry()
                .lock()
                .unwrap()
                .contains_key(&session_id)
        );
        note_session_ended(other_session_id);
    }

    #[test]
    fn descriptor_preserves_nested_collections() {
        let descriptor = [
            0x06, 0x00, 0xff, // Usage Page 0xff00
            0x09, 0x01, 0xa1, 0x01, // top-level collection
            0x09, 0x02, 0xa1, 0x02, // child collection
            0x75, 0x01, 0x95, 0x08, 0x81, 0x02, // eight one-bit inputs
            0xc0, 0xc0,
        ];
        let (collections, _) = parse_report_descriptor(&descriptor).unwrap();
        assert_eq!(collections[0].usage_page, 0xff00);
        assert_eq!(collections[0].children.len(), 1);
        assert_eq!(collections[0].children[0].usage, 2);
        assert_eq!(
            collections[0].children[0].input_reports[0].items[0].report_size,
            1
        );
    }

    #[test]
    fn descriptor_nesting_is_capped() {
        let nested = |depth: usize| {
            let mut descriptor = vec![0x06, 0x00, 0xff, 0x09, 0x01];
            descriptor.extend(std::iter::repeat_n([0xa1, 0x02], depth).flatten());
            descriptor.extend(std::iter::repeat_n(0xc0, depth));
            descriptor
        };
        assert!(parse_report_descriptor(&nested(MAX_COLLECTION_DEPTH)).is_ok());
        assert!(parse_report_descriptor(&nested(MAX_COLLECTION_DEPTH + 1)).is_err());
        assert!(parse_report_descriptor(&nested(1_400)).is_err());
    }

    #[test]
    fn a_path_with_a_security_key_collection_is_blocked_whole() {
        let key = c"/dev/hidraw3";
        let mouse = c"/dev/hidraw1";
        let blocked = security_key_paths_in(
            [
                (mouse, 0x0001),
                (mouse, 0xff00),
                (key, 0x0001),
                (key, FIDO_USAGE_PAGE),
            ]
            .into_iter(),
        );
        assert!(blocked.contains(key));
        assert!(!blocked.contains(mouse));
    }
}
