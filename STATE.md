# STATE

## 2026-11-11 Update (P9-OMB2 security audit)
- Toolchain on the box (Victor approved 2026-11-10 22:25 PT): rustup stable 1.99.1 in ~/.cargo, apt build deps (CI list).
  Build with `PATH=$HOME/.cargo/bin:$PATH CARGO_TARGET_DIR=/home/claude/workspace/temp/omb-target`.
- Baseline main bd7035a: cargo test 254+39+11 pass. node --test: 2 suites fail on main too (node-hid not installed, no npm install here).
- Merged to main: t3/ci-hardening (workflow secrets/pins/hashes) and t3/app-hardening (7 fixes, F1-F7).
- Report: reports/security-audit.md (F1-F15 fixed, N1-N12 written up).
- Victor's decisions (APPROVALS 2026-11-11): N2 require review + full CI on protocol PRs; N3 Linux peer-uid check;
  N1 signing and N4/N5 feature items stay written up.
- Workflow changes are not run on a real runner yet.


## 2026-11-11 Update (P9-OMB3, N2 + N3)
- Merged to main: t3/peer-uid (722b694, N3 -> F17: Linux peer-uid gate, src/peer.rs) and t3/protocol-review (2a55d15, N2 -> F16).
- Verified: cargo test --locked 264+39+11 pass (6 new), fmt --check and clippy -D warnings clean. Negative check: serving
  without api::make_service makes 8/11 integration tests 403, so the gate is live outside unit tests.
- Unverified: macOS/Windows compile (no targets on the box), workflows on a real runner.
- Pushing main is blocked for agents by git-guardrails: Victor pushes via docker exec into claude-code-app (creds live there, not on the host).


## 2026-11-11 Update (P9-OMB4 N9, P9-OMB5 N1 started)
- Victor picked N9 + N1 (APPROVALS 2026-11-10 22:54 PT). N9 merged (4e17261, F18): node --test scripts 8/8, YAML parses. Not run on a runner.
- Side effect: with N9 the fork cannot cut a release without Windows/macOS signing certs.
- N1 design question to Victor: the updater reads upstream's feed (OpenMouse-Project), which is unsigned; verifying a fork key fail-closed stops fork builds auto-updating from upstream.

- N1 merged as ready-but-off (b688f2c, Victor picked it 23:05 PT): cargo test 268+39+11, fmt, clippy clean; CI sign step simulated
  locally (4 cases ok); with a key built in, the stub-completion test fails on the missing .sig as it should. The latest.json path
  has no end-to-end test (review only).


## 2026-11-11 Update (P9-OMB6..9)
- N12 partial (408fd2e), signed-feed e2e test (30be774). Victor: N4/N5/N10/N11 stay (23:50 PT).
- Windows check on the box (Victor approved): rustup target x86_64-pc-windows-gnu + apt gcc-mingw-w64-x86-64. On 441bfc0
  `cargo check` and `cargo clippy --all-targets --target x86_64-pc-windows-gnu -D warnings` are clean
  (CARGO_TARGET_DIR=/home/claude/workspace/temp/omb-target-win). CI uses MSVC; macOS still unchecked.
- P9-OMB9: brainstorm agent writing reports/improvement-ideas.md; triage into tasks next.

## 2026-11-11 Update (P9-OMB10..14, improvement ideas)
- T3 restart killed the 4 helpers mid-task; resumed from their worktrees, nothing lost.
- Merged to main (1f77224): ideas #2 #3 #4 #5 #6a #6b #7 #8 #9 #11 #12 #13 #14 #16 #17 #18 #19 #21 #22 #23 #24 #25,
  plus #1 + #20 (Hybrid mode on, owner approved). Skipped: #10 (needs hardware), #15 (run_server wiring test, not picked).
- Verified on 1f77224: fmt ok, cargo test 286+41+11 pass, clippy -D warnings clean on Linux and x86_64-pc-windows-gnu,
  node .github/scripts 16/16 (check script: temp/omb-checks.sh). Rust is now pinned to 1.99.1 (rust-toolchain.toml);
  the windows-gnu target was added to that toolchain.
- Unverified: anything on hardware (Hybrid restore, X11 kept connection, HID spawn_blocking), workflows on a runner, MSVC/macOS.
- Helper E risk [Inference]: a slow HID command (>~64 ms) still blocks input reads and can overflow the queue (#10 territory).

## 2026-11-11 Update (P9-OMB15..17, hand-off)
- Main 62c9471 (local, ahead of origin 00a0eea): merged #15a t3/run-server-wiring and #15b t3/native-hid-tests.
  Checks: fmt ok, cargo test 286+43+11, clippy clean Linux + windows-gnu, scripts 16/16, native-hid npm test 19 pass
  (2 baseline fails, node-hid not installed; bare `node --test` also picks up test-delux-hardware.mjs, ignore).
- P9-OMB17 (Victor approved, wants a real-mouse test): branch t3/hid-reads (pushed to origin, NOT merged), commits
  d5e6680 (#10 Linux blocking reader), 80df688 (reopened reader handle must match vendor/product/serial/interface/descriptor,
  FIDO refused; security-review finding), 57cdfce (input reports keep flowing during slow commands). 293+41+11 pass.
  Risk: identity check could wrongly refuse a real device, most likely on Windows (Listen fails "HID interface changed").
- Windows exe cannot be built on the box (GPUI needs fxc.exe). GitHub Actions now enabled on the fork; run 43031950095
  cancelled at Victor's word ("takeover from github actions"). Building on E-Daddy instead: PC child
  <t3-thread://v2/032c749a-9cf5-42d3-a0e2-7beecea76516/mcp:launch:e6ddcd3a46957391f03e25745d2d4a55d30656656cfeee55c9ef5c8bcb159ac1>
  in C:\Users\Victo\OpenMouse-Bridge (detached at 57cdfce, switches back after), exe to Desktop\openmouse-bridge-hid-test.exe,
  report Desktop\omb-build-report.txt. E-Daddy: Rust 1.90 + rustup (Ascendara, 05:44 PT), VS Build Tools just installed by Victor.
- Test plan for Victor (Windows): run the Desktop exe, open control.openmouse.app, Listen: live input (buttons, DPI, battery),
  Unlisten/close fast, a long settings write while moving the mouse fast must not disconnect. Linux part of #10 needs a Linux
  box with a mouse + browser (the ZimaOS container has no HID devices); not planned.
- Disk cleanup done (workspace CHANGELOG): 30G -> 96G free. omb-target and omb-target-win kept.

NEXT: read the E-Daddy build child (link above). If green: give Victor the test steps (DO THIS format). If his test passes,
merge t3/hid-reads --no-ff into main, run temp/omb-checks.sh, FORK-CHANGELOG line, tasks.js done P9-OMB17 with evidence,
Victor pushes main (docker exec line in STATE above). If Listen fails with "interface changed", relax/fix the identity check.
Open (his calls): N4-N8, N10-N12, N1 activation. Skills: none required (delegation-board for helpers).
