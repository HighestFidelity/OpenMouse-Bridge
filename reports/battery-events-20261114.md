# F3 reliable battery tray (t3/battery-events, 2026-11-14)
Evidence: mouse-protocol #170 (PRO X2 SUPERSTRIKE level stale under open/poll/close; held open, an
unsolicited 0x1004 event moved 69% to 70%), Bridge #79 (blank tray after login, fixed by t3/first-scan).

## (1) Reading age: shipped, on
- Tray tooltip ends "checked N min ago" (`tray_battery::checked_text`) from the live entry's `updated_at`
  (new `DeviceBattery.checked_seconds_ago`); Home rows add it (`KnownDeviceStatus.battery_checked_seconds_ago`).
- Not `last_seen`: the 5 s bus scan bumps it ("receiver seen", not "battery read"). After a restart a row
  shows no age until the next read. Minute-bucketed: tray and panel repaint at most once a minute.
## (2) HID++ battery events: behind `hidppBatteryEvents` (config.json, default false, read at start)
- `src/drivers/hidpp_battery.rs` parses event 0 of 0x1004 (status 0 dis, 1 chg, 2 slow, 3 full, 4 fault)
  and 0x1000 (0 dis, 1 chg, 2 almost full, 3 full, 4 slow, 5-7 errors), separate tables per mouse-protocol
  v0.59.0 `src/logitech/index.ts` and kernel hid-logitech-hidpp.c. Short + long reports, report id at byte 0,
  device/feature index must match, sw id 0 only; errors, >100 and 0% with good/full level flags rejected.
- Listener thread (Rust, hidapi): opens a Logitech receiver's HID++ collections (short and long, separate
  on Windows), finds the one slot with 0x1004/0x1000 (two battery slots, e.g. a keyboard too: refused), then
  only reads. Lookups use sw id 0x0B, each under `try_lock` of `device_access`, 500 ms reply wait, never a
  blocking lock. Drops all handles within ~250 ms of `client_session_active()`; retry 5 s after release,
  60 s after nothing found. One receiver held at a time, the others closed.
- Events update only an existing native row read through that receiver (same id/name/via); none yet means
  a rate-limited poll (60 s), never a new row. The 5 min poll stays as the fallback.
- `reconcile_poll`: an event under 15 min old, same charge state and further along beats a lagging poll;
  a state change or a poll further along wins and clears the anchor (heuristic from #170).

## Not done / risks
- HARDWARE-UNTESTED: no Logitech mouse here; layouts from sources (kernel event 0 + flags checked), no captures.
- Two Logitech devices on the bus: polls set no via, so events find no row and are dropped (poll still works).
- Linux/macOS builds not checked (only windows-msvc target installed); macOS shared open vs node-hid unknown.
- Toggle needs a restart, no UI switch.
Checks: fmt ok; cargo test 300 + 42 + 11 pass (3 + 1 ignored); clippy -D warnings clean; npm 32 pass, 2 baseline fail (node-hid not installed).

## (3) 0x1001 BATTERY_VOLTAGE (t3/battery-voltage, 2026-11-14)
Why: Victor's G305 Lightspeed (receiver 046d:c53f) has neither 0x1004 nor 0x1000, so the listener found no binding.
- Lookup now asks 0x1004, 0x1000, then 0x1001 per slot (mouse-protocol's order, hidpp.ts 814-824 at 5a4f643).
- Event 0 params `[mV high, mV low, flags]` (= hidpp.ts reply[3..=5]); 0 mV is no reading.
- Percent: mouse-protocol `BATTERY_VOLTAGE_CURVE` (hidpp.ts 429-433) + `estimateBatteryPercent` (3313-3324):
  clamp >= 4186 -> 100, <= 3500 -> 0, linear between points, Math.round (half up). Integer math, tested on all
  13 points, both clamps, interpolation and a .5 tie.
- Charging: `readBatteryVoltage` order (3279-3297): bit 7 and `flags & 3 == 2` full (not charging); else bit 4
  charging slowly (even without bit 7); else bit 7 charging; else discharging. Same `BatteryEvent` and
  `record_battery_event` / `reconcile_poll` path as 0x1004/0x1000. Default still off.
- Kernel differs [from memory, unverified]: hid-logitech-hidpp.c reads `flags & 0x07` under bit 7 (0 charging,
  1 full, 2 not charging) and maps mV with its own 100-step table; we follow mouse-protocol so events match the poll.
- Risks: whether a G305 sends 0x1001 event 0 unsolicited, and how often, is unknown (untested on hardware);
  voltage jitters, so events may bounce 1-2% (reconcile only lets a lower discharging event win).
Checks: fmt ok; cargo test 316 + 46 + 11 pass (3 + 1 ignored); clippy -D warnings clean. Built with
CARGO_TARGET_DIR = main checkout's target: C: had under 1 GB free.
