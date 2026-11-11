# Fork changelog (HighestFidelity/OpenMouse-Bridge)

Upstream's CHANGELOG.md is release notes; fork-only work is logged here.

## 2026-11-11
- Project files: CLAUDE.md, STATE.md, FORK-CHANGELOG.md.
- Security audit (P9-OMB2): reports/security-audit.md.
- t3/ci-hardening: signing secrets step-scoped, release token write only on publish, persist-credentials false,
  actions pinned to SHAs, Node runtime/appimagetool hash-checked, dmgbuild pinned, ref_name via env.
- t3/app-hardening: udev no-logind rule owner-only (no 0666), FIDO keys blocked on the HID WebSocket,
  dev origins debug-only, PUT /v1/battery bounded, two IPv6 SSRF gaps closed, release builds ignore the
  CI checkout path for native-hid scripts, bridge-server.mjs Host/Origin gate.
- t3/peer-uid (P9-OMB3, N3 -> F17): on Linux the API and HID WebSocket refuse connections whose client
  socket belongs to another uid (/proc/net/tcp{,6}), fail closed; servers use api::make_service. 6 new tests.
- t3/protocol-review (P9-OMB3, N2 -> F16): protocol update PRs are no longer auto-merged; ci.yml runs
  fmt/test/clippy and the script tests on every PR, protocol-only ones included.
- t3/release-safety (P9-OMB4, N9 -> F18): release.yml refuses to release without signing secrets, never
  overwrites published assets, publishes pre-release tags as pre-releases (not latest).
- t3/update-signing (P9-OMB5, N1): ed25519 signature checks on update metadata (latest.json.sig,
  <archive>.sha256.sig) and CI signing with UPDATE_SIGNING_KEY. Off until a public key is built in.
- t3/signing-bind-tag (P9-OMB5 review): .sha256.sig covers the release tag and the checksum must name the archive (no replay/downgrade).
- t3/n12-hardening (P9-OMB6, N12 partial): HID collection nesting capped at 64; Windows .pfx / macOS .p12
  no longer left in RUNNER_TEMP for later steps.
- t3/signed-feed-test (P9-OMB7): end-to-end test of the signed latest.json check (fetch_feed).
- t3/remove-bridge-server (#25): unused native-hid/bridge-server.mjs and its gate removed, stale allowScripts pins trimmed.
- t3/ideas-api-hid (P9-OMB13, ideas #7 #8 #9 #21): peer uid checked once per connection (shared verdict), HID
  session open/close in spawn_blocking, input drained before new commands, button presses logged at debug.
- t3/ideas-ci (P9-OMB12, ideas #12 #13 #14 #23 #24): CI runs on main pushes; one Node.js pin source
  (.github/scripts/native-hid-node.mjs) for test and build; Rust pinned to 1.99.1 (rust-toolchain.toml,
  rust-version, workflows, checked by a test); epaint_default_fonts dropped.
- t3/ideas-runtime (P9-OMB10, ideas #2 #5 #11 #17 #6a): API port bound before config/catalog, catalog fetched in
  the background; update checks/installs run in their own serial queue off the tray loop; quit waits at most 2 s
  for blocking work; headless drops panel/snapshot channels; a failed catalog save only warns.
- t3/bad-config (#6b, P9-OMB14): an unparsable config.json is renamed to config.json.bad-<ts>, Bridge starts on defaults and the tray says where the old file is. Unreadable files still fail (nothing replaced unless kept).
- t3/ideas-perf (P9-OMB11, ideas #3 #4 #16 #18 #19 #22): one process scan per tick in spawn_blocking (icons too),
  blocked-HID list cached by the device scanner, config saves serialized outside the state lock, panel repaints
  only on visible change, X11 connection kept between polls, brands with no native driver skipped for 1 h.
- t3/hybrid-mode (P9-OMB14, #1 #20): run_server starts the mode monitor, so Hybrid mode works; the onboard restore
  after the web panel leaves goes through reconcile_native (forced reapply) instead of a bare thread; the
  background catalog refresh joins the config save queue. Not run on hardware.
- t3/run-server-wiring (P9-OMB15, idea #15a): run_server's startup wiring moved into start_background behind a
  Host seam; tests prove the mode monitor starts and tray settings land while an update job runs.
- t3/native-hid-tests (P9-OMB16, idea #15b): 19 node tests for apply.mjs, button-listener.mjs and the HID
  adapter against a fake node-hid (npm test 19 pass; the 2 baseline failures need node-hid installed).
