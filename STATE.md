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

- E-Daddy build GREEN on 57cdfce (14:09 UTC): fmt ok, tests 282+36+11 (one updater test needs `sh`: Git usr\bin on PATH), clippy ok,
  release 3m35s. Exe Desktop\openmouse-bridge-hid-test.exe 29.5 MB sha256 F8EC3D97...69AA6. Test steps already sent to Victor.

- Victor's Windows mouse test PASSED (2026-11-11 16:28 PT, "Works, all checks passed", ledgered). Origin main = 506c0a3 (pushed).
- New ask P9-OMB18 (deps P9-OMB17): triage upstream OpenMouse-Project/OpenMouse-Bridge open PRs (6) + open issues (5, API count
  2026-11-11) for a 2nd build: diagnose each vs our fork, brainstorm, plan in reports/upstream-triage.md, Victor picks features
  (AskUserQuestion), fix in worktrees t3/<topic>, merge, then 2nd Windows exe via an E-Daddy PC child. Upstream read-only (no gh CLI;
  public API via curl works). Victor asked to retire the old OpenMouse threads and continue in a fresh session.

NEXT: merge t3/hid-reads --no-ff into main, run temp/omb-checks.sh, FORK-CHANGELOG line, tasks.js done P9-OMB17 with evidence,
Victor pushes main (docker exec line above). Then P9-OMB18: subagent triage of upstream PRs/issues -> reports/upstream-triage.md,
AskUserQuestion which to adopt, worktree fixes, merge, E-Daddy 2nd build.
Cleanup (needs Victor's yes, AskUserQuestion): 5 leftover worktrees (agent-a299cd on t3/hid-reads, agent-a5fdabd, agent-aadafcd,
agent-aff6c11, openmouse-bridge-wt/app-hardening; the 3 non-hid are merged, hid-reads after its merge) + workspace/temp/omb17-* scratch.
Open (his calls): N4-N8, N10-N12, N1 activation. Skills: delegation-board for helpers.

## 2026-11-12 Update (P9-OMB17 merged, P9-OMB18 started)
- Main ce0c5ca: merged t3/hid-reads --no-ff. fmt ok, cargo test 293+43+11, clippy clean linux + windows-gnu, scripts 16/16.
  P9-OMB17 done. Origin main still 506c0a3: Victor pushes.
- Cleanup done (Victor's yes): 5 merged worktrees removed (branches kept), temp/omb17-* deleted.
- P9-OMB18: triage subagent writing reports/upstream-triage.md (upstream open PRs + issues vs fork).

NEXT: read reports/upstream-triage.md, brainstorm 2nd-build plan, AskUserQuestion which items to adopt, worktree fixes
t3/<topic>, review, merge, E-Daddy 2nd build (env 032c749a-..., C:\Users\Victo\OpenMouse-Bridge, Git usr\bin + .cargo\bin on PATH).
- 2026-11-12 PAUSED for Victor's Claude update (Fable 5.2 child failed: "Claude Code 2.1.280 ... 2.1.290 or newer is required").
  Picks ledgered (APPROVALS): PR60 Iris, PR62 Scyrox, PR58 G-Wolves, Redragon reader, #79 first-scan fix, updater feed -> fork
  releases, no Linux items. Upstream PRs (P9-OMB19): split by topic, scope decided after a Fable 5.2 consult.
  Triage done: reports/upstream-triage.md. Helpers stopped: t3/device-support worktree .claude/worktrees/agent-ada5c67229b726f70
  (0 commits, PR60 port uncommitted); first-scan and fork-update-feed had no commits (worktrees auto-removed).
  Origin main still 506c0a3 unless Victor pushed.

NEXT: after the update, relaunch the Fable 5.2 consult (delegate_task claudeHub claude-fable-5-2, effort high) -> reports/upstream-pr-plan.md,
relaunch 3 helpers (device-support resumes in its worktree; first-scan; fork-update-feed), review, merge, E-Daddy 2nd build, then P9-OMB19.
- 2026-11-12 00:45 UTC: Fable 5.2 failed again (T3 server from Nov 11 still spawns children as 2.1.280). Victor: "Restart now".
  Helpers stopped mid-work, uncommitted work kept in worktrees (all based on local main):
  .claude/worktrees/agent-ada5c67229b726f70 (device-support: PR60 reapplied on main hid.rs, checks pending),
  .claude/worktrees/agent-a2be35f5c2508a6ab (first-scan: fix + test in progress),
  .claude/worktrees/agent-a9880f05cf9833157 (fork-update-feed: code done, tests pending).
  Security: ttyd :8080 login is visible in `ps` and leaked into this transcript; task P9-SEC-TTYD (Victor).

NEXT: after t3 serve restart, check claude version of T3 children, relaunch Fable 5.2 consult (delegate_task claudeHub
claude-fable-5-2 effort high, same brief) -> reports/upstream-pr-plan.md; relaunch 3 helpers each told to continue in its
existing worktree above (cd there, checkout -B t3/<topic>, finish, commit, checks). Then review, merge, E-Daddy build, P9-OMB19.
- Fable 5.2 also failed on the direct "Claude" provider (enabled, Victor's yes; backup workspace/backups/20261112-0045-t3-settings).
  Cause: my 00:43 restart ran from inside a session, so t3 serve inherited ANTHROPIC_BASE_URL=cliproxyapi + CLAUDE_CODE_* vars;
  direct children still hit the proxy, which reports 2.1.280 [Inference]. Fixed script: workspace/temp/t3-restart.sh strips them.
  Plan: when the 3 helpers finish, `setsid nohup bash /home/claude/workspace/temp/t3-restart.sh 60 &`, then relaunch Fable 5.2
  on providerInstanceId claudeAgent. Proxy update later: P9-CLIPROXY-VER.

## 2026-11-12 Update (2nd build merged)
- Main: merged t3/fork-update-feed, t3/first-scan, t3/device-support. Checks green (see FORK-CHANGELOG).
- reports/upstream-pr-plan.md (Opus): ~14 upstream PRs, security first; policy bits stay fork-only; don't duplicate PRs 60/62/58.
  Scratch /home/claude/workspace/temp/omb19-try (~8.5 GB) awaiting Victor's yes to delete.
- Fable 5.2 blocked by cliproxyapi version rewrite (P9-CLIPROXY-VER). T3 direct "Claude" provider enabled (doesn't help yet).

NEXT: Victor pushes main -> E-Daddy PC child builds the 2nd exe (env 032c749a-..., project c89ec849-..., checkout
C:\Users\Victo\OpenMouse-Bridge, Git usr\bin + .cargo\bin on PATH) -> Victor tests -> P9-OMB19 upstream PRs per the plan.
- E-Daddy build2 GREEN on 8316e4d: fmt ok, tests 288+38+11, clippy ok, release 33.5s. Desktop\openmouse-bridge-build2.exe
  30,962,688 B sha256 8DB79387...6C013E046. PC checkout restored to main 3d0146f (local main behind origin, not pulled).
  Test steps sent to Victor. NEXT: his result -> P9-OMB18 done -> P9-OMB19 upstream PRs per reports/upstream-pr-plan.md.

## 2026-11-12 Update (P9-OMB19 first wave opened, hand-off)
- 2nd build: Victor's Windows test passed ("it works and auto starts to, works"); P9-OMB18 done.
- Upstream first wave (Victor: first 4, low-key note that the rest follow after review; token via T3 secret prompt):
  #106 fido-block e572a7f, #107 hidraw-owner 817b1a2, #108 dev-origins f711a38, #109 battery-bounds 2d26200
  (https://github.com/OpenMouse-Project/OpenMouse-Bridge/pull/106..109). Branches upstream/* on origin, each one commit
  on upstream main ee9b799, checks green. Bodies: reports/upstream-prs/*.md. Opener: workspace/temp/omb19-open-prs.py.
- Token: T3 secret file /home/claude/.t3/userdata/secrets/files/f68ebe0a8bfef15e08e48fc36803ca2b (workspace/.secrets writes
  are denied); push via tools/omb-askpass.sh (OMB_TOKEN_FILE=<that file> GIT_ASKPASS=... git -c credential.helper= push <url> ...).
- Cleanup awaiting Victor's yes: worktrees agent-ada5c67 (device-support), agent-a2be35f (first-scan), agent-a9880f0
  (fork-update-feed), agent-a1382ed (upstream PR prep), all merged/pushed; temp/omb19-try (~8.5 GB, keep until PRs are out);
  temp/omb-target-{dev,scan,upd,up} build caches.
- Open: P9-CLIPROXY-VER (Fable 5.2 blocked by proxy version rewrite), P9-SEC-TTYD (ttyd login in ps), Fable 5.2 review of
  reports/upstream-pr-plan.md, install-linux.sh still points at upstream (Victor's call).

NEXT: watch #106-#109 for maintainer review (answer comments with the leaving-pr-comments skill, fix on the same upstream/*
branch, push via askpass). When they respond, send wave 2 from reports/upstream-pr-plan.md items 5-14. Ask Victor (AskUserQuestion)
about the cleanup list above. Skills: delegation-board, leaving-pr-comments, secrets-hygiene.
