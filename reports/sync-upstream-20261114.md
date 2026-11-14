# Fork sync: upstream/main 66ba469 (v1.0.1) into fork main 076a8c0

Branch t3/sync-upstream, merge commit 022f28e (parents 076a8c0, 66ba469). Not pushed.

## Conflict files (15)
- ci.yml: rebuilt on upstream's split layout (test / clippy / helper + "Check (<os>)" gate, cache save-if main, CARGO_INCREMENTAL/DEBUG env). Kept fork SHA pins, toolchain 1.99.1, Node pin script, persist-credentials false. Dropped upstream's `changes` job and protocol-only skip (fork F16: every PR runs full CI) and its NATIVE_HID_NODE_VERSION env (fork test forbids copies). Gate job gets `permissions: {}`.
- gpui-build.yml, release.yml: took upstream's appimagetool fetch (gh release download with workflow token, 6 retries, same sha256). Upstream's 16 MiB cap removal in release.yml merged clean.
- native-hid/README.md: kept fork text (v0.58.0 already lists the Scyrox V8).
- native-hid/bridge-server.mjs (modify/delete): kept deleted. `git grep bridge-server upstream/main` finds nothing that loads it.
- native-hid/package.json: upstream deps (keyboard v1.6.1, mouse v0.59.0); trimmed upstream's 5 mouse-protocol commit-hash allowScripts pins (stale, lockfile uses neither).
- native-hid/src/apply.mjs, button-listener.mjs: kept fork `classesFor()` (needed by G-Wolves/Redragon per-product classes).
- native-hid/src/brands.mjs: kept fork (plain-data catalog, no `@openmouse/protocol/scyrox` import, so test fakes load without the package).
- native-hid/src/candidate-devices.test.mjs (add/add): kept fork (fake node-hid register, tests 0xf5f7 too).
- src/config.rs: kept fork test block (bad-config recovery tests); upstream #119 was ours, rest merged identical.
- src/drivers/mod.rs: kept fork (Redragon `product_brand`, KNOWN_PRODUCTS, its test). Scyrox ids identical.
- src/hid.rs: took upstream's inline Iris match arm; kept upstream's N12 comment wording.
- src/hid_access.rs, src/service.rs: took upstream's wording (same code, #107 / #118 squashes).

## Port duplicates dropped / kept
- Iris (#60): dropped fork `undeclared_feature_buffer_len()` helper and its test `undeclared_feature_reads_fall_back_only_for_known_vendors`; upstream 49fa3f8 inline arms are equivalent. 1 unit test fewer.
- Scyrox (#62): kept the fork port. Upstream 02a2594 imports SCYROX_PRODUCT_IDS from the package and tests only 0xf515/0xf517 waiting for a release with 0xf5f7; v0.58.0 and v0.59.0 both already ship 0xf5f7 (checked via gh api), so ours is ahead.

## Ours that upstream lacks (all kept)
Signed update feed, fork update feed, Linux peer-uid gate, Linux blocking HID readers + identity check, workflow hardening, G-Wolves/Redragon ports + classesFor, ideas-runtime/perf/hybrid, bad-config recovery, bridge-server.mjs removal.

## Judgment calls on new upstream workflows (merged clean)
- update-keyboard-protocol.yml: mirrored the fork's update-mouse-protocol.yml hardening (contents: read, SHA pins, persist-credentials false, Node pin script, --ignore-scripts, no auto-merge step, manual-review PR body).
- weekly-release.yml: checkout and setup-node pinned to the fork's SHAs only (it pushes with the checkout token, so no persist-credentials change). Left enabled; disabling is the coordinator's step.
- Tests adjusted: rust-toolchain.test.mjs expects 8 toolchain steps (was 7); native-hid-node.test.mjs expects 9 pin readers (was 8).
- CHANGELOG.md = upstream's exactly; Cargo.toml version 1.0.1, fork rust-version 1.99.1 kept; Cargo.lock unchanged by checks.

## Checks (Windows, Rust 1.99.1, Node v24.21.0)
- cargo fmt --check: ok.
- cargo test --locked: 287 + 39 + 11 + 0 pass, 0 fail (3 + 1 ignored).
- cargo clippy --all-targets --locked -D warnings: clean.
- node --test .github/scripts/: the directory form fails to load on this Node; glob form: 20 tests, 19 pass, 1 fail. The fail is
  native-hid-node "every job reads the pins" (2 !== 9): this checkout has CRLF workflows and the test splits on `jobs:\n`.
  On an LF copy of .github it passes (9 readers). [Inference] same CRLF fail on main in this checkout; CI sees LF.
- native-hid npm test: 35 tests, 32 pass, 2 fail (baseline attack-shark-x11, automation: need node-hid), 1 skipped.
- Not run: anything on hardware, workflows on a runner, Linux/macOS builds.
