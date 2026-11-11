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
