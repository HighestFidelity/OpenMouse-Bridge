# Cross-repo asks that land on Bridge (2026-11-14)

Source: 21 upstream issues/PRs read via the public API (bodies plus comments on 8). All still open.
Fit = is Bridge the right place (yes/partly/no). Fork = upstream already has it (inherited), fork added it (t3 branch), or not covered.

## openmouse (web app)
- **#1473** MX Master 3S DPI resets to 1000 after sleep/wake; DPI is live-only (0x2201). Fit: partly (true fix = persistent DPI write in mouse-protocol).
  Bridge need: a resident "reapply on wake" pass, i.e. when a mouse leaves the `asleep` set (src/service.rs:1599) call `reapply_native` with the stored profile. Building blocks exist (Hybrid mode push, `reapply_on_next_reconcile`).
  Fork: not covered. Wake only clears `asleep`; the per-tick `reconcile_native` (service.rs:2007) is unforced, so an already-applied profile is skipped [Inference: not hardware-checked]. Forced reapply happens only on panel disconnect (`resume_onboard`, t3/hybrid-mode).
- **#272** Remap a Logitech button to a keyboard key. Project contributor reply: works only where the onboard profile format is confirmed; the MX 3S may be software-only (host side, like Options+). Fit: yes for software-only mice.
  Bridge need: Host mode remap with OS key injection. Exists: src/buttons.rs (SendInput on Windows, uinput on Linux, macOS) driven by `hostButtons`, but only back/forward (bits 8/16) and only while a game profile is active.
  Fork: inherited, not extended. Gap: always-on (not per game) assignments, more buttons, middle click.
- **#361** MX Master 3 gesture button: pick per-direction gestures. Fit: yes (gestures are host-side in Options+ too).
  Bridge need: divert the gesture button and raw XY over HID++ (0x1b04 plus raw XY) [Inference], classify swipes, inject keys. Fork: not covered (no gesture code).
- **#166** Basilisk V3 (Linux): picker opens the boot-mouse interface; the real control channel (hidraw7) has its 0xFF00 page nested inside a Consumer collection. Fit: partly (web app `hasVendorCollection` is the bug).
  Bridge need: little; Bridge already parses nested collections and sends per-collection feature reports (`CollectionInfo`, src/hid.rs:285). Could add a "likely control interface" hint. Fork: inherited.
- **#777** G-Wolves Fenrir Pro 8K failed on Firefox/Edge via Bridge and in Desktop. Root cause: missing protocol driver, fixed upstream (openmouse#783, mouse-protocol#203). Remaining: button slot mapping still off.
  Fit: partly. Bridge role is the Firefox/Edge transport (`/v1/hid` WebSocket), which worked once the driver existed. Fork: transport inherited; G-Wolves port in t3/device-support.
- **#126** Firefox users want an extension that talks to a local HID server. Fit: yes, Bridge already is that server; the web app uses its WebSocket when WebHID is missing.
  Bridge need: nothing new, mostly discoverability of the "install Bridge" path. Fork: inherited.
- **#47** Tauri desktop app plus a HAL abstracting WebHID vs native. Fit: no (web app architecture; the Desktop GPUI app and Bridge took that role). Fork: n/a.
- **#154** Surface calibration and lift-off distance per pad. Fit: no for the commands (mouse-protocol drivers), partly for storing per-surface values in Bridge profiles.
  Bridge need: only profile fields if lift-off becomes a reapplied setting. Fork: not covered (lift-off only appears in Zaunkoenig tests).
- **#142** What "1.0 beta" means. Maintainer: 1.0 beta = web app + protocol only; Desktop and Bridge deferred. Fit: no. Context: Bridge features get little upstream priority before 1.0.

## mouse-protocol
- **#247** Basilisk V3 Pro 35K wired: battery and DPI readback via Bridge match Synapse; the panel reconnects after reload. Fit: yes (a positive Bridge report). Need: none, maybe mark "Bridge verified". Fork: inherited.
- **#161** Attack Shark X11: config is native-only (Bridge or Desktop). DPI and polling now show; a measured DPI write test and a wired 0xfa55 check are pending. Fit: yes.
  Bridge need: keep the native-hid protocol pin current (fork pins v0.58.0, past the v0.30.0 fix). Fork: inherited (pin only).
- **#99** DeathAdder V2 X HyperSpeed 0x009c connects only on the whole-device picker filter (title only, no body). Fit: partly. See Desktop #28.
- **#170** Logitech PRO X2 SUPERSTRIKE battery goes stale when polled open/close; fix = keep the session open and consume HID++ 0x1004 battery events, poll every minute as a fallback, show "last checked". Fit: yes for the tray.
  Bridge need: a persistent HID++ listener for battery events instead of spawning the Node helper per poll (5 min, 60 s while asleep). Fork: not covered.

## keyboard-protocol
- **#16** Razer Huntsman V2 TKL (Fedora): Bridge opens the 0x1:2 interface with `feat[none]`, sendFeatureReport gets EPIPE; the vendor interface with Feature items is hidraw5. Fit: yes (partly the panel filter).
  Bridge need: rank last (or skip) an interface whose descriptor declares no feature reports when the client wants them; feature ids are already in the device list (`feat[...]`). `report_paths` (src/hid.rs:1492) already prefers declaring paths, but only inside one grouped device, and Linux hidraw nodes are not grouped (PR #61 territory).
  Fork: not covered. The razerkbd kernel driver may also hold the device [Unverified].

## Desktop
- **#28** DeathAdder V2 X HyperSpeed (0x009c) missing in the Desktop app: openmouse-hid merges all composite interfaces, so Razer `isSupported` matches nothing; 0x009c missing from SUPPORTED_HID_FILTERS; reader threads log about 4 lines/s. Fit: partly (Bridge shares the native-hid brand table).
  Bridge need [Unverified, Desktop's openmouse-hid merge may not apply to Bridge]: per-interface candidates, a vendor fallback, rate-limited reader errors. The razer entry in native-hid/src/brands.mjs matches by vendor id only. Fork: partly (t3/hid-reads reworked readers).

## OpenMouse-Bridge
- **#74** X11 polling changes return 429 on Firefox/Safari because the panel's own `/v1/hid` session owns HID. Fit: yes, but needs a web panel change too.
  Bridge need: a session token returned on WS open, accepted on `PUT /v1/native/settings`, still serialized under `device_access()`. Fork: not covered (triage: SKIP, needs panel).
- **#79** Tray blank after login until the panel opens (Redragon M916 Pro 4K). Fit: yes. Fork: partly covered: t3/first-scan re-reads batteries on the first scan; t3/device-support adds a Redragon reader for M724/M612/M690 PRO only, M916 PID still unknown.
- **#21** Relaunch stalls after catalog load, no UI ("nothing happens"). Fit: yes. Fork: covered. Upstream d894090 (inherited) opens the running Bridge's panel on relaunch and added a first-run tray hint; t3/ideas-runtime binds the port first.
- **#61** (outside PR) Linux: group hidraw nodes by USB device, tag reports with interface index. Fit: yes, would help #16/#166 on Linux. Fork: not taken; triage flagged a conflict with our hid.rs reader rework, wider HID exposure (a combo receiver's keyboard comes along), untestable on Linux here.
- **#100** (outside PR) NixOS flake. Fit: yes (packaging). Maintainer asked for a rebase. Fork: not taken.
- **#113** (outside PR, stacked on #112, which was not read) Panel hides on focus loss, with a 500 ms guard against the tray click reopening it. Fit: yes (UX). Fork: not covered (no activation observer in gpui_shell).

## Feature themes (ranked by user value vs effort; "fork alone" = shippable without upstream panel or protocol work)
1. **Settings that stick: reapply on wake and replug** (#1473, also Hybrid users). Value high, effort S to M, fork alone: yes.
   Hook the asleep-to-awake and reconnect transitions into `reapply_native` with the active profile; rate-limit it; test with a fake driver.
2. **Interface selection by capability** (#16, #166, Desktop #28, #99). Value high (whole device families dead), effort M, fork alone: partly (#16 yes, #166 no).
   Bridge already reports nested usage pages and feature ids; the work is ranking: open feature-capable vendor interfaces first, Linux grouping like #61 without widening exposure. #166 itself is a web app filter bug.
3. **Reliable battery tray** (#170, #79). Value high for wireless users, effort M, fork alone: yes.
   Persistent HID++ listener with 0x1004 events plus a slow fallback poll, "last checked" in the tray, more brand readers (Redragon M916 once its PID is known).
4. **OS-level button remap and gestures in Host mode** (#272, #361). Value high for MX owners (the reason people keep Options+), effort L, fork alone: mostly (needs a panel UI or a tray editor to set assignments).
   Extend src/buttons.rs from back/forward per game to always-on, more buttons, gesture button with swipe classification.
5. **Firefox/Safari parity through the Bridge WebSocket** (#74, #126, #777). Value medium, effort S on Bridge, fork alone: no (the panel must send the token).
   Session token for native settings, clearer error bodies, documented "install Bridge" path.
6. **Tray UX polish** (#113). Value medium, effort S, fork alone: yes. Close on focus loss (review #112/#113 first); #21 already fixed.
7. **Packaging** (#100). Value low to medium (NixOS niche), effort S, fork alone: yes, but Linux is untestable here.

Not Bridge: #47 (Tauri), #142 (scope), #154 calibration commands (protocol work; Bridge would only store values).
