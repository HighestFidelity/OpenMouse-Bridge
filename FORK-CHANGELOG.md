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
