# OpenMouse-Bridge security audit (fork HighestFidelity/OpenMouse-Bridge)

Date: 2026-11-11. Base: upstream main @ bd7035a. Task P9-OMB2.
Method: read-only review of all of src/, native-hid/, install-linux.sh, packaging/, .github/; fixes on
t3/app-hardening and t3/ci-hardening, merged --no-ff to main. Verified with `cargo test --locked`
(258 passed vs 254 baseline, plus 4 new tests), `cargo fmt --check`, `cargo clippy --all-targets -D warnings`,
`cargo test --release` for the release-only paths, `node --test` for the new gate (3/3; the 2 other node
suites fail on main too because node-hid is not installed here), YAML parse of the workflows.
Workflow changes are NOT run on a real runner yet [Unverified in CI].

## HQ's list, verified

| # | HQ claim | Verdict |
|---|---|---|
| 1 | udev: seat user gets uinput; hidraw MODE=0666 without logind | True. 0666 fixed (F1). uinput open (N4) |
| 2 | Updater checks only a SHA256 from the same release, no signature | True (N1), needs signing keys |
| 3 | curl\|bash install runs with sudo | Partly. Script runs as the user and calls sudo only for packages, /etc/udev and modules-load. Trust is TLS + same-release SHA256 (N6) |
| 4 | bridge-server.mjs: ACAO *, no WS Origin check, unused | True. Rust never spawns it, `ws` is not even a dependency so it cannot start, but it ships in every package. Gated anyway (F7) |
| 5 | Node deps from GitHub tags, not commits | Partly. package-lock pins commits (mouse-protocol b62a1cce, keyboard-protocol cae3d7a7) and CI uses `npm ci`; no integrity hash for git deps; `allowScripts` is not enforced by npm (N7) |
| - | API gate (api.rs) | OK: Host must be loopback (blocks DNS rebinding), Origin allowlist, `null` refused, WS needs an allowed Origin, browsers always send Origin on PUT/DELETE. Gaps: dev origins (F3), other local accounts (F17, Linux only) |
| - | remote_image.rs SSRF | OK: resolves once, pins addresses, no proxy, every redirect re-checked, 12 MB cap. Two IPv6 gaps fixed (F5) |
| - | pkexec / sh args | OK: script and args are separate argv, `"$1"`/`"$2"` quoted, manual sudo command shell-quoted |

## Fixed

| ID | Sev | Where (pre-fix line) | Issue | Fix |
|---|---|---|---|---|
| F1 | Med | src/hid_access.rs:42-45, install-linux.sh:199 | Without logind every hidraw node of a mouse's USB device (combo receiver keyboard included) was 0666: any local account could read keystrokes / write reports | `OWNER="<uid>", MODE="0600"` for the installing account (1d21c91) |
| F2 | Med | src/hid.rs:580-588 (list), :795-800 (recover) | HID WebSocket offered any interface of a requested vendor, FIDO security keys included (CTAP from a web page) | Paths with a 0xF1D0 collection blocked whole, like WebHID's blocklist (0f2d9fa) |
| F3 | Low | src/config.rs:462-469 | Release builds trusted http://localhost:5173 and 127.0.0.1:5173: any page on that port got the full API and raw HID | Debug builds only; release builds drop them from saved configs too (582e685) |
| F4 | Low | src/service.rs:1387-1440 | `PUT /v1/battery` with made-up ids grew memory without bound (2 MB bodies) | 256-byte id/name/mode cap (400), max 64 readings, oldest evicted (7220ae7) |
| F5 | Low | src/remote_image.rs:61-66 | fec0::/10 and 64:ff9b:1::/48 counted as public | Refused (28d2cf1) |
| F6 | Low | src/drivers/native_hid.rs:285-292 | Release binaries fell back to the CI checkout path (`D:\a\...` on Windows), plantable by another account | Release builds use it only when running from inside that checkout (0d642f5) |
| F7 | Info (High if run) | native-hid/bridge-server.mjs:57,99-107 | ACAO `*`, WS upgrade with no Origin check: any web page could drive raw HID | Loopback Host + Origin allowlist, WS requires Origin; gate in src/request-gate.mjs with tests (3a6083b) |
| F8 | High | release.yml:40-42,139-145; gpui-build.yml:28-34,257-259 | Signing certs, passwords, Apple ID were job-level env, visible to npm ci / build.rs of every dependency | Step-level env on the signing/notarize steps only, `HAS_*` flags for the `if:` checks |
| F9 | Med | release.yml:7-8 + checkouts | Whole workflow `contents: write`, checkout persisted the token | `contents: read` top level, write only on the publish job, `persist-credentials: false` everywhere; protocol workflow GITHUB_TOKEN read-only |
| F10 | Med | release.yml:79,177-178,294; gpui-build.yml:66-67,187,298 | Bundled Node runtime downloaded with no hash check | SHA-256 per archive pinned next to the version, checked before extract |
| F11 | Med | release.yml:348; gpui-build.yml:227 | appimagetool from the moving `continuous` tag, unchecked | Pinned release 1.9.1 + SHA-256. Note: 1.9.1 is older than the continuous build it replaces |
| F12 | Med | release.yml:229; gpui-build.yml:112 | `pip install dmgbuild` unpinned on the job holding Apple secrets | `dmgbuild==1.6.7` (no hash pinning of its deps) |
| F13 | Med | all workflows | Third-party and actions/* pinned by moving tag/branch | Full commit SHAs (verified with git ls-remote); label-pr's old pin was a tag object, fixed to the commit |
| F14 | Low | release.yml:28,35,397,406-415 | `${{ github.ref_name }}` pasted into run: scripts | `$GITHUB_REF_NAME` / `$GITHUB_REPOSITORY` |
| F15 | Low | update-mouse-protocol.yml:59 | `npm install --package-lock-only` could run git deps' prepare scripts | `--ignore-scripts` |
| F16 | High (was N2) | update-mouse-protocol.yml:74-80; ci.yml:36-69 | New mouse-protocol releases auto-merged with no human review, and protocol-only PRs skipped fmt/test/clippy | Merge step removed (the PR stays open for a maintainer); the protocol-only shortcut dropped, every PR runs the full CI (762c2ea, P9-OMB3) [Unverified in CI] |
| F17 | Med (was N3) | src/api.rs:145-191; runtime.rs:528; main.rs:75 | Another local account could use the API and HID WebSocket (Origin is forgeable outside a browser) | Linux: the request gate (every route, HID WebSocket included) finds the client socket in /proc/net/tcp{,6} and refuses it unless its uid is the bridge's; fails closed on no entry, conflicting entries, unreadable table or missing connection info (src/peer.rs, 6c8c082, P9-OMB3). Left open: another account can still squat port 17846 before the bridge starts; macOS/Windows unchanged (needs the per-user token, upstream web change) |
| F18 | Med (was N9) | release.yml:63,113,199,218,406-414 | A missing signing secret shipped unsigned builds silently; a re-pushed tag `--clobber`ed published assets; `--latest` was set even for pre-release tags | preflight fails without the Windows/macOS signing secrets; re-runs add missing assets but never overwrite; semver pre-release tags get `--prerelease --latest=false` (isPrerelease + test) (3ee7f5a, P9-OMB4) [Unverified in CI]. Tag protection is still a repo setting |

## Not fixed (and why)

| ID | Sev | Where | Issue | Why not / what it needs |
|---|---|---|---|---|
| N1 | Med | src/updater.rs:320-345,523-545; src/updater/feed.rs; make-release-files.mjs:72 | Updates are trusted on a SHA-256 published in the same GitHub release; no signature. Anyone with release write access (or a stolen token) ships to every auto-updating install | Ready but off (Victor 2026-11-10 23:05 PT): src/updater/signature.rs verifies ed25519 signatures on latest.json and the stub checksum (bound to its tag and archive name against replay), release.yml signs with UPDATE_SIGNING_KEY (b688f2c, 4f54ee0, P9-OMB5). Off until a public key is put in UPDATE_PUBLIC_KEY_HEX, because upstream's feed is unsigned. To turn on: make an ed25519 key, store the PEM as the UPDATE_SIGNING_KEY secret, put the public key hex in the const |
| N4 | Med | src/hid_access.rs:34; install-linux.sh:208 | uinput `TAG+="uaccess"`: any process running as the seat user can create a virtual keyboard and inject input, bypassing Wayland's input isolation | Needed by the button-remap feature; removing it removes a feature. Victor's call |
| N5 | Low | src/hid.rs list/open | After F2 the WebSocket still reaches keyboard collections and any vendor id the page asks for | Limiting to mouse vendors / vendor collections may break mice the web panel supports. Victor's call |
| N6 | Low | install-linux.sh, README.md:341 | `curl \| bash` trusts openmouse.app over TLS; release zip checked only against a same-release SHA-256 | Same fix as N1 |
| N7 | Low | native-hid/package.json:14-28 | Git deps by tag (lockfile pins commits), no integrity hash; `allowScripts` not enforced by npm, so install scripts run | Pinning commits in package.json fights the protocol update workflow (F16); `ignore-scripts` could break node-hid's prebuild install, untestable here without npm install |
| N8 | Med | packaging/macos/build-app.sh:53-64,110,121 | disable-library-validation / allow-unsigned-executable-memory / allow-jit signed onto the main binary, not just Node: dylib injection into an app that may hold Input Monitoring | Needs a macOS signing + notarization run to verify; cannot test on the box |
| N10 | Low | src/local_targets.rs:335; src/applications.rs:143 | A saved UNC app path makes the Windows icon lookup contact the share (NTLM hash leak); trigger needs a trusted origin or local process | Fix would drop icons for share-hosted apps |
| N11 | Info | native_hid.rs:230; updater.rs:81,126 | `node` from PATH when the bundled one is missing; update feed URL env override in release builds | Same-user control only; env override is the documented test hook (docs/update-testing.md) |
| N12 | Info | hid.rs:1580; gpui-build.yml:49-56; release.yml:208 | Descriptor recursion depth (max ~2k on a 4 KB descriptor); gpui-build skips `npm ci` on cache hit; `security import -A`; .pfx left in RUNNER_TEMP | Low impact on hosted runners / bounded input |
