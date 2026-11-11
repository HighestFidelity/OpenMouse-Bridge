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

NEXT: P9-OMB3 in a fresh session: (1) N3 Linux peer-uid check (src/api.rs request_gate + hid_upgrade, /proc/net/tcp{,6} owner uid of the peer socket must equal getuid(), Linux only, tests with fake /proc text); (2) N2 update-mouse-protocol.yml drop --auto, ci.yml never skip fmt/test/clippy on protocol-only PRs. Each on a t3/* worktree branch, merge --no-ff, move N2/N3 to Fixed in the report, push main. Skills: delegation-board.
