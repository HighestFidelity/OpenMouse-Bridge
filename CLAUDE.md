# OpenMouse-Bridge fork (HQ project openmouse-bridge)

@/home/claude/workspace/shared-rules.md

Fork of https://github.com/OpenMouse-Project/OpenMouse-Bridge at
https://github.com/HighestFidelity/OpenMouse-Bridge. Rust tray app (GPUI) that
talks to gaming mice over HID, plus a Node helper in native-hid/. Started
2026-11-11 for a security audit and fixes (task P9-OMB2, ledger 2026-11-10 17:15 PT).

## Fork rules
- Remotes: origin = fork, upstream = OpenMouse-Project. Commit and push to origin only.
- No PRs, issues or comments upstream unless Victor says so.
- Never run the app or its installer on the box; never install udev rules or touch /etc.
- Offline builds and tests only (cargo test, node --test). Ask Victor before installing toolchains.
- Fixes in worktree children on t3/<topic> branches; coordinator reviews the diff, merges --no-ff, pushes main.
- CHANGELOG.md is upstream's release-notes file (the release workflow parses it): do not log fork work there. Fork log is FORK-CHANGELOG.md.
- Features: never add or remove one without asking Victor (AskUserQuestion).

## Files
- STATE.md: current state, ends with NEXT:
- FORK-CHANGELOG.md: what this fork changed and when
- reports/security-audit.md: findings (severity, file:line, fixed / not fixed and why)
