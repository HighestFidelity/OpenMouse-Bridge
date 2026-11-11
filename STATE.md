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
- Pushing main is blocked for agents by git-guardrails: Victor pushes.

NEXT: Victor pushes main (git -C /home/claude/data/AppData/claude-code-custom/projects/openmouse-bridge push origin main). Open items stay N1 (signing), N4/N5 (features), N6-N12. No task queued.
