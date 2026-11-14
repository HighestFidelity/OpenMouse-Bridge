//! Logitech HID++ 2.0 battery events, read off a receiver Bridge keeps open.
//!
//! A Logitech mouse pushes its charge level unsolicited when it changes:
//! feature 0x1004 UNIFIED_BATTERY sends a battery status event and the older
//! 0x1000 BATTERY_LEVEL_STATUS a battery level broadcast, both as event 0 of
//! the feature. Polled by opening and closing the receiver the level can sit
//! still for a long time (mouse-protocol #170, PRO X2 SUPERSTRIKE); held open,
//! the events arrive as the level moves. Mice without either (the G305 and
//! other AA-cell Lightspeed mice) have 0x1001 BATTERY_VOLTAGE, whose event 0
//! carries a voltage turned into a percent the way mouse-protocol does it.
//! This module parses those events and runs the listener; the timed poll
//! stays as the fallback.
//!
//! Report layouts, report id at byte 0 as hidapi returns it:
//! `[0x10 short | 0x11 long, device index, feature index, function << 4 |
//! software id, params...]`. An event has function 0 and software id 0.
//! 0x1004 params: `[state of charge %, level flags, charging status, external
//! power]`; 0x1000 params: `[discharge level %, next level, battery status]`;
//! 0x1001 params: `[voltage mV high, voltage mV low, flags]`.
//! The two charging enums differ (mouse-protocol `src/logitech/index.ts`
//! `decodeUnifiedBatteryState` / `decodeBatteryLevelState`, and the Linux
//! kernel's `hid-logitech-hidpp.c`), so each has its own table.
//!
//! Ownership: a control panel session owns the mouse. The listener drops its
//! handles as soon as `crate::hid::client_session_active()` says one is open,
//! and its few writes (the feature lookup) each run under a `try_lock` of
//! `drivers::device_access`, so a panel connecting is never held up by more
//! than one request.
//!
//! Off by default (`hidppBatteryEvents` in config.json): written from the
//! documented layouts, not tested on hardware yet. Whether a 0x1001 mouse
//! sends its event 0 unsolicited, and how often, is unverified.

use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

pub(crate) const SHORT_REPORT_ID: u8 = 0x10;
pub(crate) const LONG_REPORT_ID: u8 = 0x11;
const SHORT_LEN: usize = 7;
const LONG_LEN: usize = 20;
/// The largest input report read: a long HID++ report, with room to spare.
const READ_LEN: usize = 64;

pub(crate) const UNIFIED_BATTERY: u16 = 0x1004;
pub(crate) const BATTERY_LEVEL_STATUS: u16 = 0x1000;
pub(crate) const BATTERY_VOLTAGE: u16 = 0x1001;

/// Bridge's HID++ software id. mouse-protocol uses 0x05, so the panel's and
/// the helper's replies are never taken for Bridge's; events carry 0.
const SOFTWARE_ID: u8 = 0x0b;
/// The root feature (feature lookup) is always at index 0.
const ROOT_FEATURE_INDEX: u8 = 0x00;
/// A mouse on its own USB cable or Bluetooth answers as device 0xFF.
const DEVICE_INDEX_DIRECT: u8 = 0xff;
/// A receiver's pairing slots (Lightspeed uses 1, Bolt 1..6).
const RECEIVER_SLOTS: [u8; 6] = [1, 2, 3, 4, 5, 6];

const LOGITECH_VENDOR_ID: u16 = 0x046d;
const HIDPP_USAGE_PAGE: u16 = 0xff00;
const HIDPP_BLUETOOTH_USAGE_PAGE: u16 = 0xff43;

/// How long one request waits for its reply.
const REQUEST_TIMEOUT: Duration = Duration::from_millis(500);
/// How long one read waits before the listener looks at the session again.
const LISTEN_SLICE_MS: i32 = 250;
/// After a receiver had nothing to listen to, or none is plugged in.
const RETRY_AFTER_NOTHING: Duration = Duration::from_secs(60);
/// After the panel took the mouse over or the receiver went away.
const RETRY_AFTER_RELEASE: Duration = Duration::from_secs(5);

/// Which battery feature a mouse reports through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BatteryFeature {
    Unified,
    LevelStatus,
    Voltage,
}

impl BatteryFeature {
    fn id(self) -> u16 {
        match self {
            Self::Unified => UNIFIED_BATTERY,
            Self::LevelStatus => BATTERY_LEVEL_STATUS,
            Self::Voltage => BATTERY_VOLTAGE,
        }
    }

    fn decode_state(self, status: u8) -> ChargeState {
        match (self, status) {
            // First: 0x1001's byte is a flag set, not an enum (0x01 is not
            // charging there).
            (Self::Voltage, flags) => decode_voltage_flags(flags),
            (_, 0x00) => ChargeState::Discharging,
            (_, 0x01) => ChargeState::Charging,
            (Self::Unified, 0x02) => ChargeState::ChargingSlowly,
            (Self::Unified, 0x03) => ChargeState::Full,
            (Self::LevelStatus, 0x02) => ChargeState::AlmostFull,
            (Self::LevelStatus, 0x03) => ChargeState::Full,
            (Self::LevelStatus, 0x04) => ChargeState::ChargingSlowly,
            // 0x1004: 4 is a charging fault; 0x1000: 5..7 are battery errors.
            _ => ChargeState::Unknown,
        }
    }
}

/// 0x1001's flags byte as mouse-protocol's `readBatteryVoltage` reads it
/// (`src/drivers/logitech/hidpp.ts` 3279-3297 at commit 5a4f643), in its
/// order: bit 7 charging, and charging with `flags & 0x03 == 2` is full; else
/// bit 4 is charging slowly (even without bit 7); else bit 7 is charging.
/// The kernel reads `flags & 0x07` under bit 7 differently (1 full, 2 not
/// charging); this follows mouse-protocol so events agree with the poll.
fn decode_voltage_flags(flags: u8) -> ChargeState {
    let charging = flags & 0x80 != 0;
    if charging && flags & 0x03 == 0x02 {
        ChargeState::Full
    } else if flags & 0x10 != 0 {
        ChargeState::ChargingSlowly
    } else if charging {
        ChargeState::Charging
    } else {
        ChargeState::Discharging
    }
}

/// mouse-protocol's `BATTERY_VOLTAGE_CURVE` (`src/drivers/logitech/hidpp.ts`
/// 429-433 at commit 5a4f643): (mV, percent), highest first.
const BATTERY_VOLTAGE_CURVE: [(u16, u8); 13] = [
    (4186, 100),
    (4067, 90),
    (3989, 80),
    (3922, 70),
    (3859, 60),
    (3811, 50),
    (3778, 40),
    (3751, 30),
    (3717, 20),
    (3671, 10),
    (3646, 5),
    (3579, 2),
    (3500, 0),
];

/// mouse-protocol's `estimateBatteryPercent` (hidpp.ts 3313-3324 at commit
/// 5a4f643): 100 at or above the top point, 0 at or below the bottom one,
/// else linear between the two points around `mv`, rounded half up like
/// `Math.round`.
pub(crate) fn voltage_percent(mv: u16) -> u8 {
    let (top_mv, _) = BATTERY_VOLTAGE_CURVE[0];
    let (bottom_mv, _) = BATTERY_VOLTAGE_CURVE[BATTERY_VOLTAGE_CURVE.len() - 1];
    if mv >= top_mv {
        return 100;
    }
    if mv <= bottom_mv {
        return 0;
    }
    for pair in BATTERY_VOLTAGE_CURVE.windows(2) {
        let [(high_mv, high_percent), (low_mv, low_percent)] = [pair[0], pair[1]];
        if mv >= low_mv {
            let num = u32::from(high_percent - low_percent) * u32::from(mv - low_mv);
            let den = u32::from(high_mv - low_mv);
            // round(num / den) for positive values: floor((2num + den) / 2den).
            return low_percent + ((2 * num + den) / (2 * den)) as u8;
        }
    }
    0
}

/// The charge state an event reports, named as mouse-protocol names them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ChargeState {
    Discharging,
    Charging,
    ChargingSlowly,
    AlmostFull,
    Full,
    Unknown,
}

impl ChargeState {
    /// Charging the way the poll reads it: the state starts with "Charging".
    pub(crate) fn charging(self) -> bool {
        matches!(self, Self::Charging | Self::ChargingSlowly)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatteryEvent {
    pub percent: u8,
    pub charging: bool,
}

/// Where a mouse's battery events come from: its device index and the index
/// its battery feature sits at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Binding {
    pub device_index: u8,
    pub feature_index: u8,
    pub feature: BatteryFeature,
}

/// The battery event in `report`, if it is one for `binding`. Replies to
/// requests (a non-zero software id), error reports and out-of-range levels
/// are not.
pub(crate) fn parse_event(report: &[u8], binding: &Binding) -> Option<BatteryEvent> {
    if !matches!(report.first(), Some(&(SHORT_REPORT_ID | LONG_REPORT_ID)))
        || report.len() < SHORT_LEN
        || binding.feature_index == ROOT_FEATURE_INDEX
        || report[1] != binding.device_index
        || report[2] != binding.feature_index
        || report[3] != 0x00
    {
        return None;
    }
    if binding.feature == BatteryFeature::Voltage {
        // hidpp.ts reply[3..=5] is report[4..=6] here (report id at byte 0).
        let mv = u16::from_be_bytes([report[4], report[5]]);
        // mouse-protocol has no percent for 0 mV (hidpp.ts 3293).
        if mv == 0 {
            return None;
        }
        return Some(BatteryEvent {
            percent: voltage_percent(mv),
            charging: binding.feature.decode_state(report[6]).charging(),
        });
    }
    let percent = report[4];
    if percent > 100 {
        return None;
    }
    // A 0x1004 mouse without a percentage gauge sends 0 with its level flags
    // (bit 2 good, bit 3 full): 0% there is no reading, and must not raise
    // the low-battery alert.
    if binding.feature == BatteryFeature::Unified && percent == 0 && report[5] & 0x0c != 0 {
        return None;
    }
    let state = binding.feature.decode_state(report[6]);
    Some(BatteryEvent {
        percent,
        charging: state.charging(),
    })
}

/// A HID++ 2.0 request on the writer's report size.
fn request(
    report_id: u8,
    device_index: u8,
    feature_index: u8,
    function: u8,
    params: &[u8],
) -> Vec<u8> {
    let len = if report_id == SHORT_REPORT_ID {
        SHORT_LEN
    } else {
        LONG_LEN
    };
    let mut report = vec![0; len];
    report[0] = report_id;
    report[1] = device_index;
    report[2] = feature_index;
    report[3] = (function << 4) | SOFTWARE_ID;
    for (slot, byte) in report[4..].iter_mut().zip(params) {
        *slot = *byte;
    }
    report
}

/// What came back for a request: its reply, an error, or something else.
#[derive(Debug, PartialEq, Eq)]
enum Answer {
    Reply,
    Error,
    Other,
}

fn answer(report: &[u8], device_index: u8, feature_index: u8, function: u8) -> Answer {
    let tag = (function << 4) | SOFTWARE_ID;
    if report.len() < SHORT_LEN
        || !matches!(report[0], SHORT_REPORT_ID | LONG_REPORT_ID)
        || report[1] != device_index
    {
        return Answer::Other;
    }
    if report[2] == feature_index && report[3] == tag {
        return Answer::Reply;
    }
    // HID++ 2.0 errors are 0xFF, HID++ 1.0 ones (a receiver's empty slot)
    // 0x8F; both echo the feature index and function.
    if matches!(report[2], 0xff | 0x8f) && report[3] == feature_index && report[4] == tag {
        return Answer::Error;
    }
    Answer::Other
}

/// One HID handle as the listener uses it.
pub(crate) trait HidppIo: Send {
    fn write(&mut self, report: &[u8]) -> Result<(), String>;
    /// One input report into `buf`, report id first; `Ok(0)` on timeout.
    fn read(&mut self, buf: &mut [u8], timeout_ms: i32) -> Result<usize, String>;
}

struct HidHandle(hidapi::HidDevice);

impl HidppIo for HidHandle {
    fn write(&mut self, report: &[u8]) -> Result<(), String> {
        self.0.write(report).map(|_| ()).map_err(|e| e.to_string())
    }

    fn read(&mut self, buf: &mut [u8], timeout_ms: i32) -> Result<usize, String> {
        self.0
            .read_timeout(buf, timeout_ms)
            .map_err(|e| e.to_string())
    }
}

/// The HID++ collections of one receiver or mouse. On Windows the short and
/// long reports are separate collections, so both are held: an event can
/// arrive on either. The first handle writes.
pub(crate) struct Link {
    pub handles: Vec<Box<dyn HidppIo>>,
    /// The report id the writer takes.
    pub write_report_id: u8,
}

impl Link {
    /// The next input report from any handle, `Ok(None)` when none came in
    /// about `timeout_ms`.
    fn read_any(&mut self, buf: &mut [u8], timeout_ms: i32) -> Result<Option<usize>, String> {
        let per = (timeout_ms / self.handles.len().max(1) as i32).max(1);
        for handle in &mut self.handles {
            let read = handle.read(buf, per)?;
            if read > 0 {
                return Ok(Some(read));
            }
        }
        Ok(None)
    }
}

/// Who may touch the mouse right now. The real one asks the HID session
/// count and `drivers::device_access`; tests script it.
pub(crate) trait Gate {
    type Guard;
    fn session_active(&self) -> bool;
    /// Bridge's device lock, if free right now. Never waits for it.
    fn try_access(&self) -> Option<Self::Guard>;
}

struct LiveGate;

impl Gate for LiveGate {
    type Guard = std::sync::MutexGuard<'static, super::automation::ManualIntent>;

    fn session_active(&self) -> bool {
        crate::hid::client_session_active()
    }

    fn try_access(&self) -> Option<Self::Guard> {
        match super::DEVICE_ACCESS.try_lock() {
            Ok(guard) => Some(guard),
            Err(std::sync::TryLockError::Poisoned(poisoned)) => Some(poisoned.into_inner()),
            Err(std::sync::TryLockError::WouldBlock) => None,
        }
    }
}

/// Why no binding was found.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Unbound {
    /// A panel session or another Bridge read holds the mouse; try soon.
    Busy,
    /// Nothing with a battery feature answered.
    NoBattery,
    /// More than one paired device reports a battery (a mouse and a
    /// keyboard on one receiver): which is the mouse cannot be told.
    Ambiguous,
    /// The handle failed.
    Lost(String),
}

/// Sends one request and waits for its reply. `Ok(None)` is an error reply
/// or silence. The device lock is held for this one request only.
fn query<G: Gate>(
    link: &mut Link,
    gate: &G,
    device_index: u8,
    feature_index: u8,
    function: u8,
    params: &[u8],
) -> Result<Option<Vec<u8>>, Unbound> {
    let Some(guard) = gate.try_access() else {
        return Err(Unbound::Busy);
    };
    if gate.session_active() {
        return Err(Unbound::Busy);
    }
    let report = request(
        link.write_report_id,
        device_index,
        feature_index,
        function,
        params,
    );
    link.handles[0].write(&report).map_err(Unbound::Lost)?;
    let deadline = Instant::now() + REQUEST_TIMEOUT;
    let mut buf = [0u8; READ_LEN];
    let outcome = loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break None;
        }
        let Some(read) = link
            .read_any(&mut buf, left.as_millis().max(1) as i32)
            .map_err(Unbound::Lost)?
        else {
            continue;
        };
        match answer(&buf[..read], device_index, feature_index, function) {
            Answer::Reply => break Some(buf[..read].to_vec()),
            Answer::Error => break None,
            Answer::Other => {}
        }
    };
    drop(guard);
    Ok(outcome)
}

/// The battery feature of `device_index`, if it answers HID++ 2.0 at all,
/// in mouse-protocol's order (hidpp.ts 814-824): a percent outright from
/// 0x1004 or 0x1000 before an estimate from 0x1001.
fn battery_feature<G: Gate>(
    link: &mut Link,
    gate: &G,
    device_index: u8,
) -> Result<Option<Binding>, Unbound> {
    for feature in [
        BatteryFeature::Unified,
        BatteryFeature::LevelStatus,
        BatteryFeature::Voltage,
    ] {
        let id = feature.id();
        let Some(reply) = query(
            link,
            gate,
            device_index,
            ROOT_FEATURE_INDEX,
            0,
            &[(id >> 8) as u8, id as u8],
        )?
        else {
            // No answer to the root feature: nothing on this index.
            return Ok(None);
        };
        let feature_index = reply[4];
        if feature_index != 0 {
            return Ok(Some(Binding {
                device_index,
                feature_index,
                feature,
            }));
        }
    }
    Ok(None)
}

/// Finds the one paired mouse with a battery feature: a receiver slot, else
/// the device itself on 0xFF (a cable or Bluetooth).
pub(crate) fn resolve<G: Gate>(link: &mut Link, gate: &G) -> Result<Binding, Unbound> {
    let mut found = Vec::new();
    for slot in RECEIVER_SLOTS {
        if let Some(binding) = battery_feature(link, gate, slot)? {
            found.push(binding);
        }
    }
    match found.as_slice() {
        [one] => Ok(*one),
        [] => battery_feature(link, gate, DEVICE_INDEX_DIRECT)?.ok_or(Unbound::NoBattery),
        _ => Err(Unbound::Ambiguous),
    }
}

/// Why listening stopped.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Ended {
    /// A control panel session opened; the handles are already dropped.
    PanelConnected,
    /// The receiver went away or a read failed.
    Lost,
    Stopped,
}

/// Reads `link` until a panel session opens, the device goes or `stop` is
/// set, handing each battery event to `on_event`. Takes the link by value, so
/// every handle is closed by the time this returns.
pub(crate) fn listen<G: Gate>(
    mut link: Link,
    binding: Binding,
    gate: &G,
    stop: &AtomicBool,
    mut on_event: impl FnMut(BatteryEvent),
) -> Ended {
    let mut buf = [0u8; READ_LEN];
    loop {
        if stop.load(Ordering::Acquire) {
            return Ended::Stopped;
        }
        if gate.session_active() {
            drop(link);
            return Ended::PanelConnected;
        }
        match link.read_any(&mut buf, LISTEN_SLICE_MS) {
            Ok(Some(read)) => {
                if let Some(event) = parse_event(&buf[..read], &binding) {
                    on_event(event);
                }
            }
            Ok(None) => {}
            Err(_) => return Ended::Lost,
        }
    }
}

/// The HID++ collections of every Logitech receiver or mouse on the bus, as
/// `(bus device id, link)`. The ids match `drivers::detect_mice`'s.
fn open_logitech() -> Vec<(String, Link)> {
    let Ok(api) = crate::hid::new_api() else {
        return Vec::new();
    };
    // (identity, paths with "takes long reports") in bus order.
    let mut groups: Vec<(String, Vec<(std::ffi::CString, bool)>)> = Vec::new();
    for info in api.device_list() {
        if info.vendor_id() != LOGITECH_VENDOR_ID || crate::hid::is_receiver_child(info.path()) {
            continue;
        }
        let long = match (info.usage_page(), info.usage()) {
            (HIDPP_USAGE_PAGE, 0x01) => false,
            (HIDPP_USAGE_PAGE, 0x02) | (HIDPP_BLUETOOTH_USAGE_PAGE, _) => true,
            _ => continue,
        };
        let id = format!(
            "Logitech:{}",
            super::identity_tail(&(
                info.vendor_id(),
                info.product_id(),
                info.serial_number().unwrap_or_default(),
            ))
        );
        let index = match groups.iter().position(|(known, _)| *known == id) {
            Some(index) => index,
            None => {
                groups.push((id, Vec::new()));
                groups.len() - 1
            }
        };
        let paths = &mut groups[index].1;
        match paths
            .iter_mut()
            .find(|(path, _)| path.as_c_str() == info.path())
        {
            // One Linux hidraw node lists both collections.
            Some((_, takes_long)) => *takes_long |= long,
            None => paths.push((info.path().to_owned(), long)),
        }
    }
    let mut links = Vec::new();
    for (id, mut paths) in groups {
        // The writer goes first: a long collection when there is one.
        paths.sort_by_key(|(_, long)| !*long);
        let write_report_id = match paths.first() {
            Some((_, true)) => LONG_REPORT_ID,
            Some((_, false)) => SHORT_REPORT_ID,
            None => continue,
        };
        let handles: Vec<Box<dyn HidppIo>> = paths
            .iter()
            .filter_map(|(path, _)| api.open_path(path).ok())
            .map(|device| Box::new(HidHandle(device)) as Box<dyn HidppIo>)
            .collect();
        if handles.len() == paths.len() {
            links.push((
                id,
                Link {
                    handles,
                    write_report_id,
                },
            ));
        }
    }
    links
}

/// Starts the listener thread. `on_event` gets the bus device id of the
/// receiver (or mouse) an event came through, and the event.
pub fn spawn(
    on_event: impl Fn(&str, BatteryEvent) + Send + 'static,
) -> std::io::Result<std::thread::JoinHandle<()>> {
    std::thread::Builder::new()
        .name("openmouse-hidpp-battery".into())
        .spawn(move || {
            let stop = AtomicBool::new(false);
            let gate = LiveGate;
            let mut next_attempt = Instant::now();
            loop {
                if gate.session_active() || Instant::now() < next_attempt {
                    std::thread::sleep(Duration::from_secs(1));
                    continue;
                }
                next_attempt = Instant::now() + RETRY_AFTER_NOTHING;
                let mut chosen = None;
                // Leaving the loop drops every other link, so only the
                // receiver listened to stays open.
                for (id, mut link) in open_logitech() {
                    match resolve(&mut link, &gate) {
                        Ok(binding) => {
                            chosen = Some((id, link, binding));
                            break;
                        }
                        Err(Unbound::Busy) => {
                            next_attempt = Instant::now() + RETRY_AFTER_RELEASE;
                            break;
                        }
                        Err(reason) => {
                            tracing::debug!(device = %id, ?reason, "no HID++ battery events here");
                        }
                    }
                }
                let Some((id, link, binding)) = chosen else {
                    continue;
                };
                tracing::info!(device = %id, ?binding, "listening for HID++ battery events");
                let ended = listen(link, binding, &gate, &stop, |event| on_event(&id, event));
                tracing::info!(device = %id, ?ended, "stopped listening for HID++ battery events");
                next_attempt = Instant::now() + RETRY_AFTER_RELEASE;
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::Cell,
        collections::VecDeque,
        sync::{Arc, Mutex},
    };

    const UNIFIED: Binding = Binding {
        device_index: 0x01,
        feature_index: 0x08,
        feature: BatteryFeature::Unified,
    };
    const LEVEL: Binding = Binding {
        device_index: 0xff,
        feature_index: 0x06,
        feature: BatteryFeature::LevelStatus,
    };
    const VOLTAGE: Binding = Binding {
        device_index: 0x01,
        feature_index: 0x05,
        feature: BatteryFeature::Voltage,
    };

    fn long(bytes: &[u8]) -> Vec<u8> {
        let mut report = vec![0; LONG_LEN];
        report[0] = LONG_REPORT_ID;
        report[1..=bytes.len()].copy_from_slice(bytes);
        report
    }

    fn event(percent: u8, charging: bool) -> Option<BatteryEvent> {
        Some(BatteryEvent { percent, charging })
    }

    #[test]
    fn unified_battery_events_parse() {
        // device 1, feature 8, event 0, 70%, level good, discharging.
        assert_eq!(
            parse_event(&long(&[0x01, 0x08, 0x00, 70, 0x04, 0x00, 0x00]), &UNIFIED),
            event(70, false)
        );
        assert_eq!(
            parse_event(&long(&[0x01, 0x08, 0x00, 71, 0x04, 0x01, 0x01]), &UNIFIED),
            event(71, true)
        );
        assert_eq!(
            parse_event(&long(&[0x01, 0x08, 0x00, 72, 0x04, 0x02, 0x01]), &UNIFIED),
            event(72, true),
            "charging slowly"
        );
        assert_eq!(
            parse_event(&long(&[0x01, 0x08, 0x00, 100, 0x08, 0x03, 0x01]), &UNIFIED),
            event(100, false),
            "full is not charging"
        );
        assert_eq!(
            parse_event(&long(&[0x01, 0x08, 0x00, 50, 0x04, 0x04, 0x01]), &UNIFIED),
            event(50, false),
            "fault"
        );
        // A short report carries the same first three params.
        let short = [SHORT_REPORT_ID, 0x01, 0x08, 0x00, 33, 0x04, 0x00];
        assert_eq!(parse_event(&short, &UNIFIED), event(33, false));
    }

    #[test]
    fn level_status_events_use_their_own_table() {
        assert_eq!(
            parse_event(&long(&[0xff, 0x06, 0x00, 40, 30, 0x00]), &LEVEL),
            event(40, false)
        );
        assert_eq!(
            parse_event(&long(&[0xff, 0x06, 0x00, 90, 100, 0x02]), &LEVEL),
            event(90, false),
            "almost full"
        );
        assert_eq!(
            parse_event(&long(&[0xff, 0x06, 0x00, 45, 50, 0x04]), &LEVEL),
            event(45, true),
            "charging slowly is 4 here"
        );
        assert_eq!(
            parse_event(&long(&[0xff, 0x06, 0x00, 45, 50, 0x06]), &LEVEL),
            event(45, false),
            "thermal error"
        );
        // A coarse 0x1000 mouse reports 0 as a real reading, like the poll.
        assert_eq!(
            parse_event(&long(&[0xff, 0x06, 0x00, 0, 0, 0x00]), &LEVEL),
            event(0, false)
        );
    }

    #[test]
    fn the_voltage_curve_matches_mouse_protocol() {
        // Every point of BATTERY_VOLTAGE_CURVE lands on its own percent.
        for (mv, percent) in BATTERY_VOLTAGE_CURVE {
            assert_eq!(voltage_percent(mv), percent, "{mv} mV");
        }
        // Clamped at both ends.
        assert_eq!(voltage_percent(4300), 100);
        assert_eq!(voltage_percent(u16::MAX), 100);
        assert_eq!(voltage_percent(3499), 0);
        assert_eq!(voltage_percent(1), 0);
        // Linear between points: 10 + 10 * 29 / 46 = 16.3 -> 16.
        assert_eq!(voltage_percent(3700), 16);
        // 80 + 10 * 11 / 78 = 81.4 -> 81.
        assert_eq!(voltage_percent(4000), 81);
        // 50 + 10 * 12 / 48 = 52.5 rounds up, like Math.round.
        assert_eq!(voltage_percent(3823), 53);
        assert_eq!(voltage_percent(3822), 52, "52.29");
        // One mV above the bottom: 2 * 1 / 79 = 0.03 -> 0.
        assert_eq!(voltage_percent(3501), 0);
    }

    #[test]
    fn voltage_events_parse_with_mouse_protocols_flags() {
        let at = |mv: u16, flags: u8| {
            let [high, low] = mv.to_be_bytes();
            parse_event(&long(&[0x01, 0x05, 0x00, high, low, flags]), &VOLTAGE)
        };
        // 0x0F3A = 3898 mV: 60 + 10 * 39 / 63 = 66.2 -> 66, discharging.
        assert_eq!(at(3898, 0x00), event(66, false));
        assert_eq!(at(3898, 0x80), event(66, true), "charging");
        assert_eq!(at(4186, 0x82), event(100, false), "full is not charging");
        assert_eq!(at(4186, 0x92), event(100, false), "full wins over slow");
        assert_eq!(at(3898, 0x10), event(66, true), "slow, even without bit 7");
        assert_eq!(at(3898, 0x90), event(66, true), "charging slowly");
        // The kernel reads 0x81 as full; mouse-protocol as charging.
        assert_eq!(at(3898, 0x81), event(66, true));
        assert_eq!(at(3898, 0x02), event(66, false), "full bits without bit 7");
        // Without bit 7 or bit 4 every other bit is discharging, 0x01 too.
        for flags in (0..=0xffu8).filter(|flags| flags & 0x90 == 0) {
            assert_eq!(at(3898, flags), event(66, false), "{flags:#04x}");
        }
        assert_eq!(at(3400, 0x00), event(0, false), "flat is a real 0%");
        assert_eq!(at(0, 0x00), None, "no voltage, no reading");
        // The high byte (0x0F) is never taken for a percent.
        assert_eq!(at(3500, 0x00), event(0, false));
        // A short report carries all three params.
        let short = [SHORT_REPORT_ID, 0x01, 0x05, 0x00, 0x0f, 0x3a, 0x80];
        assert_eq!(parse_event(&short, &VOLTAGE), event(66, true));
        assert_eq!(parse_event(&short[..6], &VOLTAGE), None, "truncated");
        // Not an event: a reply, another feature index.
        let mut reply = short;
        reply[3] = 0x05;
        assert_eq!(parse_event(&reply, &VOLTAGE), None);
        let mut other = short;
        other[2] = 0x08;
        assert_eq!(parse_event(&other, &VOLTAGE), None);
        // The same bytes on a 0x1004 binding read as a percent, not a voltage.
        let unified = Binding {
            feature: BatteryFeature::Unified,
            ..VOLTAGE
        };
        assert_eq!(parse_event(&short, &unified), event(15, false));
    }

    #[test]
    fn everything_else_is_not_a_battery_event() {
        let good = long(&[0x01, 0x08, 0x00, 70, 0x04, 0x00]);
        assert!(parse_event(&good, &UNIFIED).is_some());
        let mut other = good.clone();
        other[1] = 0x02;
        assert_eq!(parse_event(&other, &UNIFIED), None, "another device");
        let mut other = good.clone();
        other[2] = 0x09;
        assert_eq!(parse_event(&other, &UNIFIED), None, "another feature");
        let mut other = good.clone();
        other[3] = 0x15;
        assert_eq!(
            parse_event(&other, &UNIFIED),
            None,
            "mouse-protocol's getStatus reply"
        );
        let mut other = good.clone();
        other[3] = 0x10;
        assert_eq!(
            parse_event(&other, &UNIFIED),
            None,
            "function 1, not an event"
        );
        let mut other = good.clone();
        other[4] = 101;
        assert_eq!(parse_event(&other, &UNIFIED), None, "out of range");
        let mut other = good.clone();
        other[0] = 0x20;
        assert_eq!(parse_event(&other, &UNIFIED), None, "not HID++");
        assert_eq!(parse_event(&good[..6], &UNIFIED), None, "truncated");
        assert_eq!(parse_event(&[], &UNIFIED), None);
        // An error report for the feature.
        let error = long(&[0x01, 0xff, 0x08, 0x1b, 0x02]);
        assert_eq!(parse_event(&error, &UNIFIED), None);
        // No gauge: 0% with "good" level flags is no reading.
        assert_eq!(
            parse_event(&long(&[0x01, 0x08, 0x00, 0, 0x04, 0x00]), &UNIFIED),
            None
        );
        assert_eq!(
            parse_event(&long(&[0x01, 0x08, 0x00, 0, 0x01, 0x00]), &UNIFIED),
            event(0, false),
            "critical"
        );
        let unbound = Binding {
            feature_index: 0,
            ..UNIFIED
        };
        assert_eq!(
            parse_event(&long(&[0x01, 0x00, 0x00, 70, 0, 0]), &unbound),
            None
        );
    }

    #[test]
    fn requests_and_answers_carry_bridges_software_id() {
        assert_eq!(
            request(LONG_REPORT_ID, 0x01, 0x00, 0, &[0x10, 0x04]),
            long(&[0x01, 0x00, 0x0b, 0x10, 0x04])
        );
        assert_eq!(
            request(SHORT_REPORT_ID, 0xff, 0x00, 0, &[0x10, 0x00]),
            vec![SHORT_REPORT_ID, 0xff, 0x00, 0x0b, 0x10, 0x00, 0x00]
        );
        assert_eq!(
            answer(&long(&[0x01, 0x00, 0x0b, 0x08]), 0x01, 0x00, 0),
            Answer::Reply
        );
        assert_eq!(
            answer(&long(&[0x01, 0x00, 0x05, 0x08]), 0x01, 0x00, 0),
            Answer::Other,
            "the panel's"
        );
        assert_eq!(
            answer(&long(&[0x01, 0xff, 0x00, 0x0b, 0x06]), 0x01, 0x00, 0),
            Answer::Error
        );
        assert_eq!(
            answer(&long(&[0x02, 0x8f, 0x00, 0x0b, 0x09]), 0x02, 0x00, 0),
            Answer::Error,
            "empty slot"
        );
        assert_eq!(
            answer(&long(&[0x02, 0x00, 0x0b, 0x08]), 0x01, 0x00, 0),
            Answer::Other
        );
    }

    type Replies = Arc<dyn Fn(&[u8]) -> Option<Vec<u8>> + Send + Sync>;

    /// A scripted handle: answers requests from `replies` by request, and
    /// otherwise plays `inputs`. Records writes; flags its own drop.
    struct Fake {
        replies: Replies,
        queued: VecDeque<Vec<u8>>,
        inputs: Arc<Mutex<VecDeque<Vec<u8>>>>,
        writes: Arc<Mutex<Vec<Vec<u8>>>>,
        dropped: Arc<AtomicBool>,
    }

    impl Drop for Fake {
        fn drop(&mut self) {
            self.dropped.store(true, Ordering::Release);
        }
    }

    impl HidppIo for Fake {
        fn write(&mut self, report: &[u8]) -> Result<(), String> {
            self.writes.lock().unwrap().push(report.to_vec());
            if let Some(reply) = (self.replies)(report) {
                self.queued.push_back(reply);
            }
            Ok(())
        }

        fn read(&mut self, buf: &mut [u8], _timeout_ms: i32) -> Result<usize, String> {
            let next = self
                .queued
                .pop_front()
                .or_else(|| self.inputs.lock().unwrap().pop_front());
            match next {
                Some(report) if report.is_empty() => Err("gone".into()),
                Some(report) => {
                    buf[..report.len()].copy_from_slice(&report);
                    Ok(report.len())
                }
                None => Ok(0),
            }
        }
    }

    struct Rig {
        link: Link,
        inputs: Arc<Mutex<VecDeque<Vec<u8>>>>,
        writes: Arc<Mutex<Vec<Vec<u8>>>>,
        dropped: Arc<AtomicBool>,
    }

    /// A receiver whose `slots` answer HID++ 2.0 with `feature` at index 8
    /// (or 0 for none); other slots get a HID++ 1.0 error.
    fn receiver(slots: &'static [(u8, Option<u16>)]) -> Rig {
        let replies = Arc::new(move |report: &[u8]| -> Option<Vec<u8>> {
            let device = report[1];
            let asked = u16::from_be_bytes([report[4], report[5]]);
            match slots.iter().find(|(slot, _)| *slot == device) {
                Some((_, feature)) => {
                    let index = if *feature == Some(asked) { 0x08 } else { 0x00 };
                    Some(long(&[device, 0x00, report[3], index]))
                }
                None => Some(long(&[device, 0x8f, 0x00, report[3], 0x09])),
            }
        });
        let inputs = Arc::new(Mutex::new(VecDeque::new()));
        let writes = Arc::new(Mutex::new(Vec::new()));
        let dropped = Arc::new(AtomicBool::new(false));
        Rig {
            link: Link {
                handles: vec![Box::new(Fake {
                    replies,
                    queued: VecDeque::new(),
                    inputs: Arc::clone(&inputs),
                    writes: Arc::clone(&writes),
                    dropped: Arc::clone(&dropped),
                })],
                write_report_id: LONG_REPORT_ID,
            },
            inputs,
            writes,
            dropped,
        }
    }

    /// Scripted ownership: the lock is free unless `locked`; a panel session
    /// opens once `session_after` checks have passed.
    struct FakeGate {
        locked: bool,
        checks: Cell<usize>,
        session_after: usize,
        held: Cell<usize>,
    }

    impl FakeGate {
        fn free() -> Self {
            Self {
                locked: false,
                checks: Cell::new(0),
                session_after: usize::MAX,
                held: Cell::new(0),
            }
        }
    }

    struct FakeGuard<'a>(&'a Cell<usize>);

    impl Drop for FakeGuard<'_> {
        fn drop(&mut self) {
            self.0.set(self.0.get() - 1);
        }
    }

    impl<'a> Gate for &'a FakeGate {
        type Guard = FakeGuard<'a>;

        fn session_active(&self) -> bool {
            let checks = self.checks.get() + 1;
            self.checks.set(checks);
            checks > self.session_after
        }

        fn try_access(&self) -> Option<Self::Guard> {
            if self.locked {
                return None;
            }
            self.held.set(self.held.get() + 1);
            Some(FakeGuard(&self.held))
        }
    }

    #[test]
    fn resolves_the_one_mouse_on_a_receiver() {
        let mut rig = receiver(&[(0x01, Some(UNIFIED_BATTERY))]);
        let gate = FakeGate::free();
        assert_eq!(
            resolve(&mut rig.link, &&gate),
            Ok(Binding {
                device_index: 0x01,
                feature_index: 0x08,
                feature: BatteryFeature::Unified
            })
        );
        assert_eq!(gate.held.get(), 0, "the lock is never kept");
        // A HERO-era mouse has only 0x1000.
        let mut rig = receiver(&[(0x02, Some(BATTERY_LEVEL_STATUS))]);
        assert_eq!(
            resolve(&mut rig.link, &&FakeGate::free()).map(|binding| binding.feature),
            Ok(BatteryFeature::LevelStatus)
        );
        // A G305 has only 0x1001, asked for last.
        let mut rig = receiver(&[(0x01, Some(BATTERY_VOLTAGE))]);
        assert_eq!(
            resolve(&mut rig.link, &&FakeGate::free()),
            Ok(Binding {
                device_index: 0x01,
                feature_index: 0x08,
                feature: BatteryFeature::Voltage
            })
        );
        let asked: Vec<u16> = rig
            .writes
            .lock()
            .unwrap()
            .iter()
            .filter(|write| write[1] == 0x01)
            .map(|write| u16::from_be_bytes([write[4], write[5]]))
            .collect();
        assert_eq!(
            asked,
            [UNIFIED_BATTERY, BATTERY_LEVEL_STATUS, BATTERY_VOLTAGE]
        );
        // A wired mouse answers as itself.
        let mut rig = receiver(&[(0xff, Some(UNIFIED_BATTERY))]);
        assert_eq!(
            resolve(&mut rig.link, &&FakeGate::free()).map(|binding| binding.device_index),
            Ok(0xff)
        );
    }

    #[test]
    fn a_receiver_with_two_batteries_or_none_is_left_alone() {
        let mut rig = receiver(&[(0x01, Some(UNIFIED_BATTERY)), (0x02, Some(UNIFIED_BATTERY))]);
        assert_eq!(
            resolve(&mut rig.link, &&FakeGate::free()),
            Err(Unbound::Ambiguous)
        );
        let mut rig = receiver(&[(0x01, None)]);
        assert_eq!(
            resolve(&mut rig.link, &&FakeGate::free()),
            Err(Unbound::NoBattery)
        );
    }

    #[test]
    fn nothing_is_written_while_the_lock_or_a_session_is_taken() {
        let mut rig = receiver(&[(0x01, Some(UNIFIED_BATTERY))]);
        let locked = FakeGate {
            locked: true,
            ..FakeGate::free()
        };
        assert_eq!(resolve(&mut rig.link, &&locked), Err(Unbound::Busy));
        let session = FakeGate {
            session_after: 0,
            ..FakeGate::free()
        };
        assert_eq!(resolve(&mut rig.link, &&session), Err(Unbound::Busy));
        assert!(rig.writes.lock().unwrap().is_empty());
        assert_eq!(session.held.get(), 0);
    }

    #[test]
    fn events_flow_until_the_panel_connects_then_the_handles_close() {
        let rig = receiver(&[]);
        {
            let mut inputs = rig.inputs.lock().unwrap();
            // A reply to the panel's own request, then two events.
            inputs.push_back(long(&[0x01, 0x08, 0x15, 69, 0x04, 0x00]));
            inputs.push_back(long(&[0x01, 0x08, 0x00, 69, 0x04, 0x00]));
            inputs.push_back(long(&[0x01, 0x08, 0x00, 70, 0x04, 0x01]));
        }
        let gate = FakeGate {
            session_after: 4,
            ..FakeGate::free()
        };
        let mut seen = Vec::new();
        let dropped = Arc::clone(&rig.dropped);
        let ended = listen(
            rig.link,
            UNIFIED,
            &&gate,
            &AtomicBool::new(false),
            |event| seen.push(event),
        );
        assert_eq!(ended, Ended::PanelConnected);
        assert!(dropped.load(Ordering::Acquire), "released for the panel");
        assert_eq!(seen, [event(69, false).unwrap(), event(70, true).unwrap()]);
        assert!(
            rig.writes.lock().unwrap().is_empty(),
            "listening never writes"
        );
    }

    #[test]
    fn a_vanished_receiver_ends_listening() {
        let rig = receiver(&[]);
        rig.inputs.lock().unwrap().push_back(Vec::new());
        let ended = listen(
            rig.link,
            UNIFIED,
            &&FakeGate::free(),
            &AtomicBool::new(false),
            |_| {},
        );
        assert_eq!(ended, Ended::Lost);
        assert!(rig.dropped.load(Ordering::Acquire));
    }
}
