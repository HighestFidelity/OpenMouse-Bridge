# Simplify the Rust (and Node) code: ranked ideas

Read-only review 2026-11-14, line numbers = fork main f52b9d6. "Upstream" = the block is upstream code: refactor it
via an upstream PR or not at all (fork-only refactors make every sync worse). improvement-ideas.md #1-#25 not repeated.
Fork delta (+/-) vs upstream: service.rs +303/-93, hid.rs +658/-80 (HOLD), runtime.rs +521/-140, config.rs +156/-13,
updater.rs +108/-21, panel.rs +99/-6, drivers/mod.rs +89/-6, delux.rs 0, apply.mjs and button-listener.mjs +2/-2.

## Do first (high value, files where fork and upstream are near-identical)

1. Delux copy-paste. delux.rs:49-79 `apply` repeats the set_dpi/sleep/set_polling_rate block per device type;
   `Delux1d57Device::open` (delux.rs:90-125) and `Delux248aDevice::open` (173-203) differ only in vid, report id and
   probe length. One `open_probing(api, vid, report_id, len)` + a 2-method trait cuts ~60 lines.
   Upstream (fork delta 0). S, low (no packet changes; no Delux hardware here).
2. Node helper duplication. `withTimeout` + `probe` copy-pasted in apply.mjs:58-85 and button-listener.mjs:32-52,
   and the vendor x device x class probe loop in apply.mjs:121-132 vs button-listener.mjs:62-85. Move to one
   probe.mjs with the timeout as a parameter. Upstream. S, low (both have node tests).
3. Config defaults written twice: `Default` (config.rs:198-219) repeats what the serde default fns (520-536)
   return. Struct-level `#[serde(default)]` lets 3 const fns + `default_tray_battery_style` go (~20 lines);
   `default_origins` and `enabled_by_default` stay (ApplicationProfile uses the latter, 129-133). Upstream. S, low.
4. Typed tray style: `tray_battery_style: String` (config.rs:37-38) is re-parsed in normalized() (228),
   panel.rs:409, tray_battery.rs:93 and re-stringified at runtime.rs:665, while `platform::TrayBatteryStyle`
   (platform/mod.rs:53-80) exists. Make the field that enum, deserialized through the existing `from_config_str`
   so trim/case handling stays. Upstream. S, low-med (snapshot JSON must keep its string shape).
5. panel.rs:163-189 `UpdateState` is `UpdateEvent` minus Notes plus Idle, mapped arm by arm. Store
   `Option<UpdateEvent>` or give Notes its own channel. Upstream. S, low.
6. Device-id brand parsing: 6 same-shape `split(':').next()` sites (service.rs:2045 `brand_of`, 2314, 2328;
   drivers/mod.rs:291-297, 360; native_hid.rs:111) plus 3 related `split_once` ones that also need the name or an
   error (buttons.rs:84-88, automation.rs:95, 356). One `ProfileDevice::brand()`. All upstream. S, low.

## Worth it, but sequence after the held fork deltas land upstream

7. Settings plumbing in three hand-written layers: panel.rs:608-658 (one setter per setting, same send+notify),
   runtime.rs:630-694 (one match arm each, same "could not save Bridge settings" log), service.rs:1440-1485
   (one-line setters). A `BridgeCommand::Setting(SettingChange)` + one `apply_setting` drops ~80 lines. Keep both
   save paths (`update_config` vs `set_config_then_save`, service.rs:1305-1353; tests 3360-3398). Mixed: panel
   setters and dispatch arms are upstream (upstream/main runtime.rs:434), but the loop's home `start_background`
   and `set_config_then_save` are fork-added. S, low.

8. Tests out of the god-files, like updater/tests.rs already: service.rs:2362-4203 (1840 lines, 44% of the
   file), hid.rs:2029-2800 (770). Move-only, upstream only (fork-side it conflicts with every upstream test
   edit). S, low. Cheap first step before #9/#10.
9. Split service.rs by concern: profile resolution + images (24-400) to service/profiles.rs; battery, known
   devices, polling (610-700, 1487-1648, 2031-2345) to service/battery.rs; game monitor (1852-2030) to
   service/monitor.rs. Upstream. M, low risk but high sync cost while the fork carries +303 here.
10. Split hid.rs: pure descriptor parser (1584-1767) to hid/descriptor.rs, WebSocket session layer (1802-2027)
    to hid/ws.rs, active-device registry (48-134) to hid/registry.rs. Upstream. M, low; collides with the
    HOLD reader delta (+658), so only after that PR settles.
11. Fork-only cfg noise: `cfg(any(windows, linux))` and its `not(...)` appear 15x in hid.rs (367-501, 852,
    1010-1012, 2499), all from fork commits d5e6680/80df688 (HOLD, upstream-pr-plan.md:54). Gather the
    owned-reader code into one cfg'd `mod owned_reader`, or emit one custom cfg from the existing build.rs.
    Fork-only, so do it before offering that PR. S, low, no new crate.
12. Two resume detectors, two thresholds: hid.rs:1774-1797 runs its own thread (2 s step, 15 s gap, 43-45),
    runtime.rs:134 + 280-284 `resume_gap` (10 s) drives gpui_shell/mod.rs:229. One lib fn
    `resume_gap(prev, now, threshold)` for both. Upstream. S, low-med (keep both thresholds).
13. Updater shell: UNIX_TRANSACTION (updater.rs:1437-1500) and BUNDLE_TRANSACTION (1626-1680) share ~25 lines
    (symlink-chain check, special-file find, lock dir, mktemp, rollback skeleton). A shared prelude const.
    Upstream. M, high (installer; tests run both, updater/tests.rs:337, 949, macOS path not runnable here).

## Needs Victor (removes or changes behaviour)

14. Same hardware, two paths: delux.rs:127-155 writes the 0x1d57 polling report natively, and
    attack-shark-x11.mjs:3-30 builds the identical packet for the same vid/pids, via apply.mjs:111-113. They are
    reached under different brand labels ("delux" vs "attack shark"), which is why both exist. Pick one owner.
    Upstream. M, med.
15. Non-desktop fallback: main.rs:50-77 + `config::load_with_catalog` (config.rs:429, only caller main.rs:62)
    is an older second server startup for OSes Bridge does not ship on. Dropping it removes a platform
    fallback. Upstream. S, low.
16. Brand tables: drivers/mod.rs:55-87 (Rust, one label per vid with precedence) and brands.mjs:69-215 (JS,
    driver classes) differ on purpose, do not merge. The hand-rolled JS text parser test (mod.rs:806-873)
    could become a shared brands.json for vendor/product ids. Test is upstream; KNOWN_PRODUCTS and the
    Redragon block are fork-only. M, med (labels become device.id profile keys).

## Checked, nothing worth doing

17. State machines: NativePass (service.rs:483-492), ApplyOutcome (409-413), reconcile chain (1767-1852) are
    small and test-pinned (2428-2594). Nit: `reconcile_native`/`reapply_native` (1814-1824) are 1-line
    wrappers, one caller each (2007, 2349): inline. S.
18. Dead code (zero-caller check only, panel.rs not scanned): no `pub fn` in the other focus files has zero
    callers; clippy -D warnings covers private items. keymap.rs:7 file-wide `allow(dead_code)` could be per-fn. S.
19. updater.rs has 49 cfg attributes, the most of any focus file; the per-OS installer block (1436-1943) could
    move to updater/install_{unix,macos,windows}.rs. Upstream. M, low (move-only). tray.rs `mod platform` is fine.
20. sha2 + ring both ship (updater.rs:17 vs fork signature.rs). ring::digest could replace sha2, but sha2
    is upstream code, so a fork-only swap is churn. Leave it.
