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

NEXT: triage reports/improvement-ideas.md (P9-OMB9); Victor pushes main (ahead of dd49d0f). Open: N4-N8, N10-N12 (his calls); N1 activation (key) when he wants.
