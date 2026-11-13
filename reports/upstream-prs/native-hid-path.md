fix(native-hid): release builds never load the HID helper from the build machine's checkout path

When `native-hid/src/apply.mjs` isn't next to the executable, Bridge falls back to the source checkout it was built from (`CARGO_MANIFEST_DIR`, baked in at compile time). For a release binary that's the CI runner's path, `D:\a\...` on Windows. Any account on the user's machine can create `D:\a\...` and drop its own `apply.mjs` there, and Bridge would run it with Node.

What changed:
- This only gates the dev fallback. A release build uses the checkout path only when the running executable is inside that checkout (`cargo build --release`, then `target/release/...`). Debug builds keep the fallback as before.
- The bundled helper paths next to the executable are untouched, so installed releases find the helper exactly as they do now.

Tested:
- New unit test `a_release_build_uses_the_checkout_only_from_inside_it`: the test binary counts as inside its own folder, a made-up checkout does not, and debug builds always allow the fallback.
- `cargo fmt --check`, `cargo test --locked`, and clippy with `-D warnings` for Linux and x86_64-pc-windows-gnu all pass.
- A fork build with this change passed a quick Windows test using the bundled helper. I didn't stage a fake `D:\a\...` checkout to watch it get refused.
