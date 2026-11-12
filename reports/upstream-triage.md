# Upstream triage (P9-OMB18)

Date: 2026-11-12. Upstream: OpenMouse-Project/OpenMouse-Bridge. Fork main: ce0c5ca (contains all of upstream/main @ bd7035a, 72 fork commits ahead).
API counts (public REST, unauthenticated): **6 open PRs, 5 open issues** (same as the 2026-11-11 count).
Raw data (untrusted, never run): /home/claude/workspace/temp/omb18-upstream/ (pulls.json, issues.json, pr<N>.diff, mouse-protocol v0.58.0 package.json/registry/vendors).
Merge tests: throwaway shared clone at /home/claude/workspace/temp/omb18-apply/fork, `git merge --no-commit` of each PR head onto fork main. Nothing compiled or run [Unverified build].

Key context: our native-hid pin is `@openmouse/protocol` **v0.58.0** (lock commit b62a1cce, checked with ls-remote). That release already ships Scyrox (incl. V8 0xf5f7), all three G-Wolves classes, Thermaltake and three Redragon clients, so the "waiting on a protocol release" notes in #58/#62 are resolved for us.

## Summary

| # | Type | Title | Verdict | Effort | Risk | Feature? | Fixed in fork? |
|---|---|---|---|---|---|---|---|
| PR 60 | device support (bug) | Tt eSPORTS Iris config reports | ADOPT (Victor picks) | S | low | yes: Iris settings via Bridge HID | no |
| PR 62 | device support + bug | Scyrox to its own client, only 0xff02 opened | ADOPT (Victor picks) | S | low-med | yes: Scyrox native settings | no |
| PR 58 | device support | G-Wolves discovery + native settings | ADOPT (Victor picks) | S | low-med | yes: G-Wolves native settings | no |
| PR 61 | bug (Linux) | group hidraw nodes by USB device | NEEDS-VICTOR, lean SKIP | L | med-high | no (WS message gains a field) | no |
| PR 98 | CI | appimagetool 1.9.1 via token, checksummed | SKIP (superseded) | S | low | no | yes, F11 |
| PR 100 | packaging | NixOS flake | NEEDS-VICTOR, lean SKIP | S | low | yes: new install method | no |
| #79 | bug | tray empty after login until panel opens | REWRITE (scan part); Redragon part NEEDS-VICTOR | S | low | scan fix no; Redragon yes | no |
| #99 | bug (Linux) | overlay banner straddles monitors on KDE Wayland | NEEDS-VICTOR | S-M | low | yes if a setting is added | no |
| #74 | bug | X11 native settings 429 while panel owns HID WS | SKIP (needs web panel change) | M | med | no | no |
| #31 | device request | Eastern Times T7 258a:1007 | SKIP (no protocol driver) | n/a | n/a | yes | no |
| #21 | bug | relaunches never reach ready, no UI | SKIP (fixed) | n/a | n/a | no | yes |

## PRs

### PR 60 fix(hid): read the Tt eSPORTS Iris config reports (kagaminehaku, 1 commit)
- What: adds `THERMALTAKE_VENDOR_ID 0x264A` with a 2053-byte fallback buffer in `receive_feature_report`, next to the Razer/Microsoft arms. The Iris config collection declares no feature reports on Windows, so every read failed "feature report 12 is not declared".
- Fork: same code still there, src/hid.rs:1177-1182. Merge onto fork main: **clean** (auto-merge).
- Security: nothing new. Fixed size buffer, vendor-gated, no deps. Runs after our F2 FIDO path block.
- Tested by the author on real hardware (Windows 10), not with cargo test/clippy. Only useful with the web panel's Thermaltake driver (mouse-protocol v0.58.0 has src/drivers/thermaltake/hid.ts).
- Verdict: ADOPT if Victor wants Iris support. Cherry-pick 0c519c3d, run our checks.

### PR 62 feat(native-hid): Scyrox own client, open only its config collection (klNuno, 2 commits)
- What: Scyrox products on CompX 0x3554 (0xf515, 0xf517, 0xf5f7) were mapped to Teevolution and probed with `TeevolutionHidClient`, which cannot drive them [Inference: so the wrong protocol's commands get written to a Scyrox dongle]. Adds a `scyrox` brands.mjs entry (productIds + usagePage 0xff02), `candidateDevices(vendorId, hints)` filtering, `compx_brand()` in src/drivers/mod.rs. Bisect data in the PR: listening on all 7 collections makes the V8 dongle go silent.
- Fork: src/drivers/mod.rs:114-121 still routes every non-VGN 0x3554 to Teevolution; native-hid/src/brands.mjs has no scyrox key. Merge: one conflict, `native-hid/bridge-server.mjs` (we deleted it in t3/remove-bridge-server; resolution = keep it deleted). Rust and JS otherwise merge clean.
- Security: no new deps; new import is `@openmouse/protocol/scyrox` (pure data per PR, export exists in v0.58.0). Opening fewer collections reduces exposure. Fine.
- Not tested by the author end to end (apply.mjs on the mouse, writes, Linux). The `usagePage` filter is a no-op on Linux hidraw (one entry per interface).
- Verdict: ADOPT if Victor wants Scyrox. Our v0.58.0 pin already has the V8 id, so the V8 works here instead of being skipped [Inference].

### PR 58 feat(devices): G-Wolves discovery + native settings (klNuno, 3 commits)
- What: `g-wolves` brands.mjs entry with 3 classes, each limited to its protocol's product ids via `classesFor()`; G-Wolves 0x33e4 in KNOWN_BRANDS; adapter retries a failed feature read at the length that id was last sent at (wired HTS Ultra stalls at 91 bytes, answers at 65). That retry applies to **every brand** but only after the 91-byte read fails on all handles.
- Fork: native-hid/src/brands.mjs:12-15 still says "G-Wolves not exported". Merge onto fork main: conflicts in `install-linux.sh` (stale VENDOR_IDS hunk; our installer's udev rule is already brand-agnostic, drop the hunk) and `native-hid/src/hid-device-adapter.test.mjs` (add/add: our P9-OMB16 file vs theirs, merge both test sets). Stacked after #62: two one-line conflicts in apply.mjs and button-listener.mjs, resolution spelled out in #62's body (`candidateDevices(vendorId, entry)` + `classesFor(entry, device.productId)`).
- Security: no new deps, imports only `@openmouse/protocol/drivers/gwolves/*` (present in v0.58.0). Fine.
- Tested by the author: wireless HTS Ultra 8K on Windows 11, set/readback DPI and rate. Not tested: wired, other two protocols, Linux/macOS.
- Verdict: ADOPT if Victor wants G-Wolves. Land after #62.

### PR 61 fix(hid): group hidraw nodes by USB device (Renari, 3 commits)
- What: Linux only. Resolves each hidraw node to its USB device via /sys/class/hidraw so a serial-less mouse's interfaces become one candidate (like Chromium WebHID); orders paths by bInterfaceNumber; adds an `interface` index to `CollectionInfo` and to every `inputreport` WebSocket message; parses descriptors for Razer on Linux too; feature reads try declaring paths first.
- Fork: our src/hid.rs is heavily rewritten (P9-OMB17 #10 blocking readers, reader identity check, input during commands). Merge onto fork main: **5 conflict hunks in src/hid.rs**, one ~160 lines (fork 1300-1460, the reader code). The PR does not even apply cleanly to current upstream main (stale since 2026-10-31).
- Security: reads sysfs only, no deps, no unsafe. But grouping all interfaces of a USB device into one candidate means a page that opens a combo receiver's mouse now also gets its keyboard hidraw node on the HID WebSocket, so keystroke input reports could reach the page [Inference]. That widens our open item N5. FIDO stays blocked (our F2 block is per path, src/hid.rs:1444-1457) [Inference].
- We cannot hardware-test Linux (no Linux box with a mouse, per STATE). The web panel has to understand the new `interface` field to benefit [Unverified].
- Verdict: NEEDS-VICTOR, lean SKIP. Big port into our riskiest file, Linux-only value, untestable here, and it trades against N5. If wanted: REWRITE on top of our reader code, opt-in per vendor or with keyboard collections filtered.

### PR 98 ci: fetch appimagetool 1.9.1 with the workflow token, checksummed (upstream maintainers' branch, 2 commits)
- What: pins appimagetool 1.9.1 + SHA-256 and downloads it with `gh release download` + GH_TOKEN and 6 retries (anonymous downloads were 504ing). Second commit drops the 16 MiB size cap on the bridge stub archives.
- Fork: already pinned to the **same version and same hash** (ed4ce84f...) in F11: release.yml:445-446, gpui-build.yml:265-266, anonymous curl with --retry 5. Merge: conflicts in both workflows (our change vs theirs on the same lines). The cap removal (packaging/bridge/build-bridge-package.sh:24,55, release.yml:153-154) only matters if the fork publishes releases and the stub grows past 16 MiB.
- Security: fine. GH_TOKEN in that step is read-only on our fork's build jobs (F9).
- Odd but not dangerous: upstream's v1.0.0 tag points at 23d7749, which is this PR's branch, **not upstream main**. Upstream shipped 1.0.0 from an unmerged branch.
- Verdict: SKIP (superseded). If our fork CI ever hits the 504s, REWRITE just the token download.

### PR 100 Add NixOS flake support (Reasel, 2 commits)
- What: flake.nix/flake.lock (nixpkgs-unstable pinned to a rev), README note, `OPENMOUSE_TEST_SLEEP` env hook in a src/games.rs test.
- Fork: merges clean. But the flake would likely not build on our fork as is: hardcoded `npmDepsHash` is tied to upstream's native-hid lockfile, and our Cargo.toml pins rust-version 1.99.1 (rust-toolchain.toml) which nixpkgs' rustc may not meet [Unverified].
- Security: `npmInstallFlags --ignore-scripts=false` + `forceGitDeps` run git deps' install scripts inside the Nix sandbox (same exposure as our N7, sandboxed). nixpkgs pinned by rev. No new runtime deps. Fine.
- Verdict: NEEDS-VICTOR, lean SKIP. New install method nobody here uses [Inference: Victor is on Windows].

## Issues

### #79 Tray shows no mouse after login until the control panel opens
- Two causes. (a) No Redragon reader: no `redragon` in native-hid/src/brands.mjs and no Redragon in KNOWN_BRANDS (src/drivers/mod.rs). mouse-protocol v0.58.0 has RedragonHidClient (0xfc7a, 0xfc61), M690 Pro and M719 Pro clients, but the reporter's M916 Pro 4K PID is still unknown. (b) Generic: the scanner never re-polls on its **first** scan because `previous` starts `None` (fork src/service.rs:1669 and 1685-1687), and the battery poll runs every 5 min (src/service.rs:107). A dongle that enumerates after the first poll leaves the tray blank for up to 5 min for any brand.
- Fork: not fixed.
- Verdict: REWRITE (b): treat the first scan as "appeared" and retry once after a failed read. S, low, no feature change. (a) is device support: NEEDS-VICTOR, and blocked on the PID anyway.

### #99 Linux overlay banner lands on the monitor seam (KDE Wayland, 2 outputs)
- Confirmed by the reporter: Wayland session, XWayland gives a DISPLAY, overlay centres on the 3840 px root, banner straddles the seam. Fork has the same code: src/overlay/linux.rs:28 (TOP_MARGIN), :129-134 (root width centring); overlay created unconditionally at src/gpui_shell/panel.rs:373.
- Fix options: (1) on Wayland sessions use the existing native notification fallback (S, changes how alerts look for Wayland users); (2) a `notification_style` setting (feature add); (3) place on the focused output via src/kwin_focus.rs (M).
- Verdict: NEEDS-VICTOR (behaviour/feature choice). Low value unless Victor runs Linux desktop.

### #74 Attack Shark X11 native settings 429 while the panel holds the HID WebSocket
- Fork has the same gate: src/api.rs:462-465 (`client_session_active()` -> 429). Fix needs a session token from Bridge **and** the openmouse web panel sending it; Bridge-only work does nothing.
- Security: a token that lets requests bypass the HID ownership gate is new API surface; would need to be per-session random and checked with our F17 peer-uid gate [Inference].
- Verdict: SKIP for the 2nd build (needs an upstream web change, M effort, med risk).

### #31 Eastern Times Tech T7 (258a:1007)
- No client for 258a:1007 in mouse-protocol v0.58.0 (Sino Wealth 0x258a only covers Glorious 0x0036/0x0033 and Redragon ids). Bridge cannot drive it without a protocol driver.
- Verdict: SKIP (mouse-protocol work, not Bridge).

### #21 Relaunches never reach ready + no UI (beta.14)
- Fixed: upstream d894090 (2026-10-13) opens the running Bridge's tray panel on relaunch and added the first-run tray hint; our fork's 46f837c (P9-OMB10) binds port 17846 before config/catalog work, so a second launch finds the first at once and logs "already running; opening its tray panel" (src/runtime.rs:380-395). Issue just never got closed upstream.
- Verdict: SKIP (fixed in fork and upstream). Razer DeathAdder Essential path still unverified, nothing to do here.

## 2nd build candidates (ranked value/risk)

1. **#79b first-scan re-poll** (REWRITE, S, low, no feature change): every brand benefits after login. Safe to do without a feature pick.
2. **PR 60 Thermaltake Iris** (ADOPT, S, low, clean merge, hardware-tested by author). Needs Victor's yes: device support.
3. **PR 62 Scyrox** (ADOPT, S, low-med): also stops the Teevolution client probing Scyrox dongles. Resolve the bridge-server.mjs conflict by keeping it deleted. Needs Victor's yes.
4. **PR 58 G-Wolves** (ADOPT, S, low-med, after #62): drop the install-linux.sh hunk, merge the two adapter test files. Generic feature-read retry touches all brands. Needs Victor's yes.
5. **PR 98 token download** (REWRITE only if our CI 504s): otherwise nothing to take.

Not recommended for the 2nd build: PR 61 (big conflict in our HID reader, Linux-only, widens N5, untestable), PR 100, #74, #99, #31, #21.

## Alarming (outside the PR list)
- The fork's updater still follows **upstream's** release feed (src/updater/feed.rs:27,32; src/updater.rs:22) and `automatic_updates` defaults on (src/config.rs:203). Our builds report version 1.0.0 (Cargo.toml:3), so upstream's next release (1.0.1+) would auto-replace Victor's fork exe and silently drop every fork fix [Inference, not tested]. Worth deciding before the 2nd exe: turn auto-updates off in the test build, point the feed at the fork, or bump the fork version scheme.
