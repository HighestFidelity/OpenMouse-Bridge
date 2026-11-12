fix(api): trust the Vite dev origins only in debug builds

Release builds trust `http://localhost:5173` and `http://127.0.0.1:5173` by default. Nothing guarantees that port is your Vite server on a user's machine, so whatever page is served there, by any program or any account, gets the whole local API and raw HID.

What changed:
- The two dev origins are only in the defaults for debug builds (`cfg!(debug_assertions)`).
- Release builds also drop them from configs that saved them earlier as defaults. Every other saved origin stays.

Heads up on one side effect: if someone tests a release build against a local Vite server, they'll need a debug build for that now.

Tested:
- New unit test `only_debug_builds_trust_the_dev_server`, checking both the defaults and a normalized saved config, and that a user-added origin survives. Ran it in debug and in `--release`.
- `cargo fmt --check`, `cargo test --locked`, and clippy with `-D warnings` for Linux and x86_64-pc-windows-gnu all pass.
- Not tested against the real control panel.

One of four small security fixes I'm opening first. I have a handful more (CI hardening, a couple of bugs) and will send them after these.
