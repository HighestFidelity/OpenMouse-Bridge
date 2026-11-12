fix(hid): never offer FIDO security keys over the HID WebSocket

The HID WebSocket lists every HID interface for whatever vendor ids the client asks for, and nothing stops it from asking for a security key's vendor. So any trusted origin (or a local program that just sends a matching Origin header) could open a security key through Bridge and talk CTAP to it. Browsers block the FIDO usage page in WebHID for exactly this reason, so Bridge going around that felt worth closing.

What changed:
- Any hidraw path that carries a FIDO collection (usage page 0xF1D0) is left out of the device list and refused on open. The whole path is blocked, since one node can hold a key's collection next to others.
- While I was in the descriptor parser: collection nesting is now capped at 64. Real mice nest a handful deep, but a hostile descriptor could nest around 1,400 deep and push the recursive tree building and serialization pretty hard. Deeper descriptors are refused with an error. Happy to split this into its own PR if you'd rather.

Tested:
- New unit tests: `a_path_with_a_security_key_collection_is_blocked_whole` and `descriptor_nesting_is_capped` (64 parses, 65 and 1,400 are refused).
- `cargo fmt --check`, `cargo test --locked`, and clippy with `-D warnings` for Linux and x86_64-pc-windows-gnu all pass.
- Not hardware tested: I didn't plug in a real security key, so the blocking is checked on the path/usage-page logic only.

This is one of four small security fixes I'm sending first; I've got a few more (CI hardening, a couple of bug fixes) ready and will send them once these get a look.
