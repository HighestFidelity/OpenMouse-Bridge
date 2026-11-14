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

## 2026-11-12
- t3/hid-reads (P9-OMB17, #10) merged ce0c5ca: Linux input readers block on their own hidraw handle, the reopened
  reader must match vendor/product/serial/interface/descriptor (FIDO refused), input reports keep flowing during
  slow commands. Windows real-mouse test passed (Victor 2026-11-11 16:28 PT). Linux part not hardware-tested.
- Cleanup (Victor's yes, APPROVALS 2026-11-12 16:29 PT): 5 merged worktrees removed, branches kept; temp/omb17-* deleted.
- 2nd build (P9-OMB18, Victor's picks 2026-11-12):
  - t3/fork-update-feed: in-app updates come from HighestFidelity/OpenMouse-Bridge releases (fork-only); a missing
    latest.json (404) means up to date. install-linux.sh still points at upstream.
  - t3/first-scan (upstream #79): the first device scan reads batteries when the startup poll got nothing.
  - t3/device-support: ports of upstream PRs 60 (Tt eSPORTS Iris), 62 (Scyrox), 58 (G-Wolves) and a Redragon reader
    (M724, M612, M690 PRO by exact vendor/product ids). None tested on hardware; M690 PRO settings may not load.
  - Checks on main: fmt ok, cargo test 299+43+11, clippy clean linux + windows-gnu, scripts 16/16, native-hid 32 pass
    (2 baseline fails need node-hid).
- Upstream triage and PR plan: reports/upstream-triage.md, reports/upstream-pr-plan.md (Opus; Fable 5.2 review later).

## 2026-11-14
- t3/sync-upstream (P9-OMB26): real merge of upstream/main 66ba469 (v1.0.1, our #106-#109 #116-#119 back as squashes).
  Took upstream's Iris (#60), kept our Scyrox port (loads without the protocol package), bridge-server.mjs stays deleted
  (nothing loads it). Fork hardening kept; upstream's update-keyboard-protocol.yml and weekly-release.yml SHA-pinned.
  Checks: Linux fmt ok, cargo test 298+46+11, clippy clean linux + windows-gnu, scripts 20/20; Windows tests 287+39+11.
  Report: reports/sync-upstream-20261114.md.
