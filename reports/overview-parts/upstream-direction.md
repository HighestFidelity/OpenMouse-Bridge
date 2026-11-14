# Upstream direction and Bridge API contract

Snapshot 2026-11-14. Web app = openmouse shallow clone at 28f08dd (2026-11-12, protocol 0.59.0). Bridge = fork main f52b9d6. GitHub API, unauthenticated.

## A. Where upstream is heading

Facts
- Bridge releases: v1.0.0 2026-11-09, v1.0.1 2026-11-13. No milestones on Bridge.
- openmouse: no GitHub releases. One milestone "1.0 Beta" with 3 open, 0 closed issues (#140, #141, #142), all filed 2026-08-30 by snekxs.
- #142: no roadmap exists in any repo; placeholder to define surfaces, the beta promise, versioning off 0.x. Its only comment (2026-08-30): Desktop and Bridge are both out of scope for the 1.0 beta push, "web app + protocol only"; Desktop's own 1.0 Beta milestone closed.
- #141: CHANGELOG.md stale since 2026-07-31; backfill or replace with GitHub Releases notes before a beta announcement.
- #140: main JS chunk at 96-98% of a 700 kB budget; split per-brand driver UI via dynamic import. Budget raised again since (PR #1339, #1404), partial splits landed (#1227, #1230), so still live.
- Web app merges (Oct 22 to Nov 12, mostly snekxs, some jazzstack/onyxhq-dev): keyboards are the big new surface (Razer/Logitech/SteelSeries #1281, Arbiter Studio #1288, Keychron HE behind a flag #1287, VIA #1306, Fn functions #1307); games/profiles (#1177, #1245, #1382 macro card); P0/P1/P2 audit batches incl. "Bridge retry" (#1335-#1337); PostHog analytics with opt-out (#1191); Bridge download fixes (#1250 manual-update prompt below beta.20, #1271/#1272 Windows zip URL).
- Bridge maintainer merges (snekxs, Nov 7 to 13): release pipeline from CHANGELOG.md plus latest.json and AppImage (#81, #82, #115, #121); weekly stable release every Friday plus daily keyboard-protocol bump (#102, #104); macOS CI on macos-15 (#94), docs stop claiming signed/notarized (#95); Linux polish (#84 hidraw for every USB mouse, #86 Proton/Wine games, #87 KWin focus, #90 AppImage autostart, #101 Wayland notifications); release notes in panel (#88); stable device keys (#89); receiver linking (#85).
- Bridge community merges: Scyrox client (#62), Tt eSPORTS Iris (#60), tray fixes (#110-#112). Open PRs: G-Wolves (#58), hidraw grouping (#61), NixOS flake (#100), panel click-away (#113).
- Fork's own upstream PRs (HighestFidelity, not maintainer direction): #106-#109 merged 2026-11-12, #116-#119 merged 2026-11-13.
- Open Bridge issues worth watching: #120 Windows tray RefCell crash on left click, #79 tray empty until panel opens, #74 Attack Shark X11 429 while panel holds HID socket, #21 relaunch never reaches ready.
- Desktop: last push 2026-10-22 (macOS support #25/#26, brand controls #24). README describes a standalone GPUI app running @openmouse/protocol drivers in QuickJS over native HID; never mentions Bridge. Fork docs/gpui-migration.md:16-19: Bridge and Desktop "stay separate apps", Desktop is a pattern reference, Bridge does not depend on it.
- gpui-migration.md (identical on fork main and upstream/main 66ba469) is stale: line 37 says installers "not started", line 36 says latest.json not published, lines 214-219 say Windows never built or run, Next at 221-229. Reality: v1.0.1 assets already ship latest.json, OpenMouse-Bridge-windows-x64-setup.exe, the Linux AppImage and a macOS DMG, alongside the legacy -full and plain zips; Windows fixes landed (#97) and a Windows crash is open (#120).

Inference
- [Inference] Bridge ran its own 1.0.0-beta.N line (manual-update.ts:4, #1250) and matured to 1.0.0 outside the web milestone; #142's scope comment and the milestone have not moved since 2026-08-30. Weekly Friday cadence (#102) suggests v1.0.2 around Fri 2026-11-20, notes from CHANGELOG.md.
- [Inference] Bridge next weeks: hardening what just shipped rather than building it: Windows tray stability (#120, #79), validating installer/AppImage/latest.json upgrades from older installs, then dropping the legacy zips (gpui-migration.md:221-229). Native device coverage grows via community PRs (#58, #61, #62).
- [Inference] Desktop is parked, not folded into Bridge: activity moved to web app plus Bridge (commit dates), and gpui-migration.md:16 states the opposite of a merge.
- [Inference] Web app focus: keyboards (keyboard-protocol now auto-bumped daily into Bridge too, #102/#104), bundle size (#140), and a 1.0 beta definition nobody has written yet (#142).

## B. Web app to Bridge API contract

Transport: web app uses http://127.0.0.1:17846 (bridge.ts:3) and ws://127.0.0.1:17846/v1/hid (bridge-hid.ts:25); Bridge binds 127.0.0.1:17846 (src/lib.rs:20, src/main.rs:70). Web CSP allows both (functions/_lib/security-headers.js:30,32). Bridge origins: control/dev/www/apex openmouse.app (src/config.rs:14-17), Vite dev only in debug builds (config.rs:539, PR #108).

Bodies were compared only for /v1/status and the HID socket, not for profiles, battery or local-applications. Called by the web app and present in Bridge (src/api.rs:71-101); line numbers are bridge.ts unless noted:
- GET /v1/status (111), GET /v1/running-applications (128), GET+PUT /v1/profiles (132, 140), GET /v1/games (136), GET+PUT /v1/local-applications (99, 103), GET /v1/applications/{id}/icon (41).
- GET+PUT /v1/default-profile (149, 153; caller ButtonMap.tsx:149,192), PUT /v1/battery (177; useBridgeBatteryReporter.ts:52), PUT /v1/native/settings (197; controller.ts:6052, Attack Shark only), WS /v1/hid.
- Nothing the web app calls is missing from Bridge.
- HID WS commands match 1:1 (bridge-hid.ts:84-92 vs src/hid.rs:143-183). Reply shape matches {id, ok, error?, devices?, data?} (hid.rs:202-211 vs bridge-hid.ts:75-81); DeviceSummary key/vendorId/productId/productName/collections, camelCase (hid.rs:275-282).
- Status: BridgeSnapshot is camelCase (service.rs:724-726); web reads a subset (bridge.ts:10-24). Bridge-only fields deviceMode, batteries, knownDevices, blockedHidDevices, whatsNew, hybridMode, games go unread by the web app.

Handled differently / conflicts
- Native settings vs HID socket: main.tsx:54 runs installBridgeHid({force:true}), so whenever Bridge is up the web app holds /v1/hid, even in Chrome. Bridge answers PUT /v1/native/settings with 429 while any HID WS session is active (api.rs:461-466). controller.ts:6052 has no 429 handling. The session guard is taken on socket connect, not on device open (hid.rs:1830-1834), so this is issue #74 and the force-install makes it the normal case.
- clientConnected = HID WS session active or handshake under 20 s old (service.rs:548-553). The web app never calls bridgeHandshake (0 callers), so it hinges on the socket alone.
- Stale comment: useBridgeProfileApplier.ts:15 says nothing writes /v1/default-profile; ButtonMap.tsx:192 does.

Bridge endpoints with no caller in the web app at 28f08dd (Desktop and Bridge's GPUI panel not checked for HTTP use [Unverified]):
- PUT /v1/handshake (wrapper bridge.ts:114, never called); GET /v1/applications (bridgeApplications, 0 callers).
- GET/PUT /v1/custom-games, GET/PUT /v1/custom-games/image, GET /v1/custom-games/image-from-url: zero references in openmouse/src; documented in fork README.md:224-228. [Inference] superseded by /v1/local-applications (#1245), so dead surface (image-from-url is the one fork PR #116 hardened against SSRF).
- DELETE /v1/battery/{id} (README.md:250) and PUT /v1/autostart: no web caller. PUT /v1/panel: only Bridge itself, to raise its panel on a second launch (src/runtime.rs:350).

Version and compat handshake
- No API or protocol version on REST or WS. The web app reads GET /v1/status .version and (a) flags "outdated" when status.version !== GitHub releases/latest tag (BridgeStatus.tsx:24-25,77). Plain string inequality: a fork or ahead-of-release build always shows outdated; compareVersions (updates.ts:21) has no non-test caller. (b) Shows a manual reinstall banner below 1.0.0-beta.20 (manual-update.ts:4,51; App.tsx:191).
- Feature detection is implicit via optional fields, e.g. profile.enabled absent on old Bridges (bridge.ts:53-56).

Gaps
- Firefox: no WebHID, so the web app swaps in a navigator.hid shim over Bridge's socket (bridge-hid.ts:1-3, 676-690). Works only while Bridge runs; no hot-plug push, client re-polls every 2 s (bridge-hid.ts:29-30).
- Safari: blocks ws/http loopback from an https page as mixed content; needs Bridge to serve the app over loopback, called "a separate change" (bridge-hid.ts:16-19). Bridge has no static/app route (api.rs:71-101), so Safari users have no path today.
- Manual-update banner (BridgeManualUpdate.tsx:30, only for Bridge below beta.20) offers Linux curl install-linux.sh (manual-update.ts:8,64) and the Windows plain zip (manual-update.ts:62), though v1.0.1 ships an AppImage (#82, #90) and setup.exe.
- Not Bridge: ws://localhost:9010 in src/device/firmware-updates.ts:9 refers to a vendor Electron updater.
