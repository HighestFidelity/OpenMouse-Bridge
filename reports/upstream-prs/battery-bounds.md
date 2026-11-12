fix(api): bound what PUT /v1/battery can make Bridge hold

`PUT /v1/battery` lets the control panel report a battery, and the client picks the device id and name. Nothing limited either one, so a trusted origin could keep sending made-up ids (or very long strings) and Bridge would hold every one of them in memory until the 24 hour eviction.

What changed:
- Device id, name and switch mode are capped at 256 bytes, and an empty device id is refused. Both come back as a 400 (the handler now uses `profile_error`, so `InvalidInput` maps to a bad request).
- At most 64 battery readings are held at once. When a new device would go past that, the oldest reading is dropped. Updating a device that's already there never evicts anything.

64 is way above any real desk of mice, but tell me if you'd like a different number.

Tested:
- New unit test `made_up_battery_devices_cannot_grow_memory_without_bound`: an oversized id and an empty id are refused, 74 fake devices leave 64 held, and the newest one is kept.
- `cargo fmt --check`, `cargo test --locked`, and clippy with `-D warnings` for Linux and x86_64-pc-windows-gnu all pass.
- Not tested with the real control panel or a mouse.

This one rounds out the first four small security fixes. I've got a few more lined up (CI hardening, a couple of bug fixes) and will send them once these have had a look.
