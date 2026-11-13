fix(service): show the battery right away when the mouse shows up just after Bridge starts

Refs #79 (the "tray stays blank" part only, not the Redragon reader).

The device scanner reads batteries when a mouse appears compared with the previous scan. The first scan has no previous scan, so it never triggers a read. If the dongle enumerated just after the startup battery poll (common right after boot or login), the tray had no battery until the next poll, 5 minutes later.

What changed:
- On the first scan, if a mouse is on the bus and the startup poll left no reading, Bridge reads the batteries once.
- A scan that lands while a poll is still running is judged after that poll ends, instead of spawning a read the running poll's guard would just drop. So the first scan sees what the startup poll found.
- No retry loop, and the poll interval is unchanged.

Tested:
- New unit test `the_first_scan_reads_a_mouse_already_on_the_bus_when_no_reading_landed` for the decision (`scan_wants_read`): first scan with a mouse and no reading reads; with a reading, or with no mouse, it doesn't; later scans read only when a new mouse appears.
- `cargo fmt --check`, `cargo test --locked`, and clippy with `-D warnings` for Linux and x86_64-pc-windows-gnu all pass.
- A fork build with this change passed a quick Windows test with a real mouse, battery included. I didn't reproduce the boot-time race on purpose, so that part isn't hardware verified.
