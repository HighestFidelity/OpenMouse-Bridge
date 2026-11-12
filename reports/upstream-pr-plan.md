# Upstream PR plan (P9-OMB19)

Date: 2026-11-12. Upstream main = bd7035a (fetched today, unchanged; it is the fork's merge base). Fork main checked at 7ff45e1
(t3/fork-update-feed and t3/first-scan are merged now). Advisory only: nothing pushed, posted or commented.
Throwaway clones: /home/claude/workspace/temp/omb19-try/repo (security stack, built) and repo2 (cherry-pick tests).
Build/test results of the stacks: see "Checks run" at the bottom.

## How upstream works (what this plan is shaped around)

- No CONTRIBUTING file. .github/PR_LABELS.md: Conventional Commit titles (`fix(scope): ...`) drive auto labels. Bodies in
  merged PRs: short "why", bullets of what changed, a "Tested" / "Not verified yet" section. Macroscope and CodeRabbit
  review every PR.
- Every PR merged since #65 was opened by the maintainer (snekxs) from branches in the main repo. Outside PRs: #83
  (Holtek udev) closed as superseded by the maintainer's own #84; #64 asked to rebase, then closed, and the maintainer's
  #97 shipped the same panel-on-screen fix the contributor had found. #58/#60/#61/#62 from outsiders sit open since
  Oct 29-31. [Inference] Expect the maintainer to merge some small ones and rewrite others in their own words.
  So: tiny PRs, problem first, a failing-before test, no fork process baggage.
- Maintainer's own deliberate choices our fork reverses: #67 (protocol PRs auto-merge), #68 (protocol-only PRs skip Rust
  checks, no CI on main pushes), #82 (re-cutting a tag `--clobber`s assets on purpose), #95 (macOS ships ad-hoc signed,
  no Developer ID; README says Windows signing is used only "when the repository has" the secrets).
- Open maintainer PR #98 pins appimagetool 1.9.1 with the same SHA-256 as our F11 and downloads it with the token.

## 1. Upstream or fork-only

| Change (fork commit) | Verdict | Why |
|---|---|---|
| FIDO keys blocked on HID WebSocket (0f2d9fa) | UPSTREAM | Real gain: a trusted origin or a forged-Origin local client can talk CTAP to a security key today. Clean on bd7035a |
| HID collection nesting cap 64 (Rust half of e91f3cb) | UPSTREAM | Hostile descriptor recursion. Tiny |
| udev no-logind fallback OWNER uid 0600, not 0666 (1d21c91) | UPSTREAM | #84 itself says the keyboard interface makes this matter; its fallback still opens it to every account. Clean |
| Vite dev origins only in debug builds (582e685) | UPSTREAM | Any page on :5173 gets the API + raw HID in release builds. Friction: a dev testing a release exe against local Vite loses it, and saved configs lose those origins in release builds |
| PUT /v1/battery bounds (7220ae7) | UPSTREAM | Memory growth from a trusted origin. Clean |
| remote_image fec0::/10, 64:ff9b:1::/48 (28d2cf1) | UPSTREAM | 4 lines, clean |
| native-hid CI checkout path only from inside it (0d642f5) | UPSTREAM | Windows release exe falls back to `D:\a\...`, plantable by another account. Clean |
| Linux peer-uid gate on API + HID WS (6c8c082 + 6c51951) | UPSTREAM, medium odds | Closes "another local account drives the mouse / HID"; ~280 lines, Linux only, fail-closed. Never run on a real Linux desktop [Unverified] |
| Step-scoped signing secrets, `contents: read` default, `persist-credentials: false`, `$GITHUB_REF_NAME` in run:, `--ignore-scripts` on the lockfile update, certs deleted inside signing steps (164c7d3 part + e91f3cb workflow half) | UPSTREAM | Real gain (F8/F9/F14/F15/N12): today every npm ci / build.rs on the signing jobs sees the certs. Builds the same thing, so no release-flow break. Never run on a real runner [Unverified] |
| Action SHA pins, Node.js archive SHA-256, dmgbuild==1.6.7 (rest of 164c7d3) | UPSTREAM, after #98 | Maintainer already SHA-pins label-pr.yml, so the style fits. Cost: hashes to bump with every Node LTS bump (say so in the body). Drop our appimagetool hunk, #98 covers it |
| label-pr pin was a tag object, not a commit (in 164c7d3) | UPSTREAM (inside the SHA-pin PR) | One line |
| Pre-release tags published `--prerelease --latest=false` (part of 3ee7f5a) | UPSTREAM | Today a `v1.1.0-rc.1` tag becomes "latest" and `releases/latest/download/latest.json` offers it to every installed Bridge. Small, with a test |
| Release refuses without signing secrets (part of 3ee7f5a) | FORK-ONLY | Would stop every upstream release right now (no macOS Developer ID per #95, Windows signing optional per README). If ever offered: opt-in via a repo variable, default off |
| Release never `--clobber`s assets (part of 3ee7f5a) | FORK-ONLY | #82 chose re-cutting a tag on purpose and they use it (v1.0.0 was re-cut). Small gain: someone with write access can delete a release anyway |
| Protocol PRs not auto-merged + full CI on protocol-only PRs (762c2ea) | FORK-ONLY | Reverses #67/#68, both deliberate. mouse-protocol is the same org, a human still pushes the release tag, and the Rust tests do not touch the protocol, so the second half buys CI minutes, not signal. Raise as an issue only if Victor wants |
| ed25519 update signing, off until a key (e1dc4d0, 4f54ee0, 0cc9f4d) | UPSTREAM LATER, issue first | Real gain for installed users (stolen token / release write cannot ship an update to everyone). No-op with an empty key, so it does not break their release. But they own the key and the rollout, and dead-until-enabled code is a hard sell: ask in an issue whether they want it before opening the PR |
| Rust 1.99.1 pin, `rust-version` (c618e44), rust-toolchain.test.mjs | FORK-ONLY | Toolchain policy is theirs; they use `@stable` |
| CI on main pushes (a73f057) | FORK-ONLY | #68 removed it on purpose |
| One Node pin source script (4921da8) | FORK-ONLY for now | Refactor of their workflows; offer only if they take the hash pins and complain about bumping |
| Updater feed points at the fork (6215690, merged b9fa1cb) | NEVER | Fork-only by design. Also the 404 = "no update" tweak only matters for a repo without releases |
| first-scan battery read (997d64d) | UPSTREAM | Part (b) of upstream #79, all brands. Clean |
| Hybrid mode monitor never started in the tray app (ec7efa7) | UPSTREAM | Real upstream bug: `start_mode_monitor()` is only in main.rs's headless path, so the Hybrid toggle does nothing in the GPUI app. Rework below |
| Failed catalog save no longer aborts startup (9174b33) | UPSTREAM | Clean, small bug fix |
| Remove unused bridge-server.mjs (b5f1bf6) | UPSTREAM, low priority | Nothing upstream references it, `ws` is not even a dependency, but it ships in every package with ACAO `*`. Note: open PR #62 edits this file |
| Bind first / update queue off tray loop / bounded shutdown (46f837c) | UPSTREAM, Victor decides | Robustness, clean, but changes startup order; lower odds |
| Unparsable config.json set aside, start on defaults (9848c2f) | ASK VICTOR | Behaviour change (renames the user's file). Needs 9174b33 + 46f837c |
| Perf batch: cached blocked-HID list + saves off the lock (4051016), one process scan per tick (90fcc5a) + kept X11 connection (cec280f), repaint only on change (4822aa6), skip no-driver brands 1 h (1267e7f), HID session open/close off workers (5e12a6f), button presses at debug (8ccd795), drop epaint_default_fonts (a1b097e) | UPSTREAM, low priority, Victor decides | All clean on bd7035a as a stack. Nobody upstream asked for them; the maintainer rewrites a lot. 8ccd795 has a small privacy angle (every press logged at info) |
| HID reads: Linux blocking readers, reader identity check, input during commands (5e12a6f d5e6680 80df688 57cdfce) | HOLD | +619 lines in hid.rs, Windows mouse-tested, Linux never hardware-tested, overlaps open PR #61. Offer only after a Linux test |
| node tests with fake node-hid (8445488) | UPSTREAM, low priority | Clean, test-only. Add/add clash with PR #58's hid-device-adapter.test.mjs, so after #58 settles |
| run_server wiring + Host seam (da0e214, 24906a8) | FORK-ONLY | 546-line runtime.rs refactor for testability, needs the runtime stack; low value to them |

## 2. PR list, in order

Each PR = branch off upstream/main, cherry-pick, strip fork bits (section 3), reword commits, run `cargo fmt --check`,
`cargo test --locked`, `cargo clippy --locked --all-targets -- -D warnings`, `node --test .github/scripts/*.test.mjs`.
"Clean" = `git cherry-pick` onto bd7035a with no conflict in the throwaway clone.

Security, Rust (independent of each other):
1. **fix(hid): never offer FIDO security keys over the HID WebSocket** = 0f2d9fa + Rust half of e91f3cb (nesting cap).
   Files: src/hid.rs. Rework: cherry-pick 0f2d9fa (clean), then take only the hid.rs hunks of e91f3cb (the workflow half
   goes to PR 7). Drop "(audit N12)" from the const doc. Could be two PRs if the maintainer prefers.
2. **fix(linux): give hidraw to the installing account, not everyone, without logind** = 1d21c91. Clean.
   Files: src/hid_access.rs, install-linux.sh. Body: quote #84's own "matters more now that a combo receiver's keyboard
   interface is opened too". Trade-off to state: other accounts on a no-logind box lose access.
3. **fix(api): trust the Vite dev origins only in debug builds** = 582e685. Clean. src/config.rs.
4. **fix(api): bound what PUT /v1/battery can make Bridge hold** = 7220ae7. Clean. src/service.rs, src/api.rs.
   (3 and 4 can ride together as "small API hardening" if one PR reads better.)
5. **fix(remote-image): refuse site-local and local-use NAT64 IPv6** = 28d2cf1. Clean. src/remote_image.rs.
6. **fix(native-hid): release builds use the build checkout only from inside it** = 0d642f5. Clean. src/drivers/native_hid.rs.

Security, CI (in this order, after the maintainer's #98 lands or with its lines left alone):
7. **ci: pass signing secrets only to the steps that use them** = hand-split of 164c7d3 (HAS_* flags, step env,
   `permissions: contents: read` + write only on publish, `persist-credentials: false`, `$GITHUB_REF_NAME` /
   `$GITHUB_REPOSITORY`, `--ignore-scripts`, update-mouse-protocol GITHUB_TOKEN read-only) + workflow half of e91f3cb
   (.pfx/.p12 deleted inside the step). Files: release.yml, gpui-build.yml, update-mouse-protocol.yml, ci.yml.
   Rework: 164c7d3 is clean as a whole, but split it by hunk (pins and hashes out to PR 8); then apply e91f3cb's
   workflow hunks (clean once 164c7d3 is in). Strip "(audit N12)" comments. Keep update-mouse-protocol's auto-merge
   step untouched.
8. **ci: pin actions to commit SHAs, check the bundled Node.js archives** = remaining 164c7d3 hunks: SHA pins
   (label-pr tag-object fix included), Node SHA-256 per archive, dmgbuild==1.6.7. Drop the appimagetool hunk (#98).
   Depends on 7 (same lines). Body: say Node bumps now need the 4 hashes from SHASUMS256.txt; offer Dependabot for
   actions as a follow-up only if Victor agrees (that is adding something).
9. **ci(release): publish pre-release tags as pre-releases, never latest** = the isPrerelease half of 3ee7f5a
   (make-release-files.mjs + test + the `channel` array in the publish step). Rework: 3ee7f5a is clean only on top of
   164c7d3; standalone it conflicts in release.yml. Hand-apply onto upstream's publish step, keep `--clobber` and drop the
   "Require signing secrets" step.
10. **fix(api): on Linux, serve only the account Bridge runs as** = 6c8c082 + 6c51951 squashed. Clean as a stack.
   Files: src/peer.rs (new), src/api.rs, src/lib.rs, src/main.rs, src/runtime.rs, tests/backend_local_profiles.rs.
   Strip "(audit N3)" / "idea #7" text. Body must say: fail-closed on an unreadable /proc/net/tcp, not tried on a real
   Linux desktop (Flatpak/Snap browsers worth a check [Unverified]), macOS/Windows unchanged.

Bugs:
11. **fix(service): read batteries on the first device scan when none landed** = 997d64d. Clean. src/service.rs.
   "Refs #79" (part b only; the Redragon half is separate). Reword the two "upstream #79" comments to "#79".
12. **fix(runtime): start the Hybrid mode monitor in the tray app** = ec7efa7 minus its refresh_game_catalog hunk.
   Conflict on bd7035a is only that hunk (needs the fork's config save queue); resolved by keeping upstream's side.
   Files: src/runtime.rs, src/service.rs, src/drivers/automation.rs (+ tests). Not run on hardware: say so. Could be cut
   to the 3-line runtime.rs change if the maintainer wants it minimal [Inference: the resume_onboard rework is what makes
   the now-running monitor write safely, so I would keep it].
13. **fix(config): a failed save of the fetched catalog no longer aborts startup** = 9174b33. Clean. src/config.rs.
14. **chore(native-hid): remove the unused bridge-server.mjs** = b5f1bf6 reworked: on upstream it is a plain
   `git rm native-hid/bridge-server.mjs` (the conflict is only our intermediate gate commit 3a6083b, which upstream never
   had). Leave the allowScripts trim out or keep it, it is cosmetic. Tell #62's author their hunk on that file goes away.

Device support:
15. **PR 60 / 62 / 58: do not duplicate.** They are open upstream by their authors; a second copy splits review and
   looks like poaching. Our t3/device-support ports carry only fork-specific conflict fixes (bridge-server.mjs deleted,
   our test file). Useful upstream input instead, only with Victor's OK: a short review comment on #58 that it no longer
   applies to main (install-linux.sh hunk is stale since #84, src/drivers/mod.rs moved; checked with `git apply --check`)
   and on #62/#58 the stacking resolution. #60 and #62 still apply cleanly to bd7035a.
16. **feat(native-hid): read Redragon batteries** (new reader, Refs #79 part a) = from t3/device-support once written.
   Not on the branch yet (only PR60 and PR62 ports are committed). Dependencies: if it uses #62's `candidateDevices()`
   / #58's `classesFor()`, it cannot go up until those merge, so either build it standalone on upstream brands.mjs or
   wait. The reporter's M916 Pro 4K PID is unknown (v0.58.0 knows 0xfc7a, 0xfc61, M690 Pro, M719 Pro), so it must not
   say "Fixes #79". Hardware: nobody here has a Redragon [Inference], so it ships untested; say so.

Later / only if Victor wants (lower odds, in this order):
17. fix(runtime): bind the API port first, update work off the tray loop, bounded shutdown = 46f837c. Clean.
18. perf batch, as 2 or 3 PRs: (a) 90fcc5a + cec280f process scan / X11; (b) 4051016 + 1267e7f battery scanner and
   config saves; (c) 4822aa6 panel repaint; plus 5e12a6f, 8ccd795, a1b097e as tiny ones. All clean as one stack.
19. test(native-hid): fake node-hid tests = 8445488, after #58 merges (add/add on hid-device-adapter.test.mjs).
20. feat(updater): ed25519-signed update metadata = e1dc4d0 + 4f54ee0 + 0cc9f4d, after an issue gets a yes. Rework:
   clean only on top of 164c7d3 + 3ee7f5a; on upstream, re-apply the two release.yml steps by hand (preflight key check,
   sign step) onto their publish job and keep `--clobber`; take updater.rs from those commits, not from fork main (main
   has the fork feed change); reword signature.rs's "upstream's releases are unsigned" doc; drop "(audit N1)".
21. HID reads (hid-reads stack) only after a Linux hardware run; clean on top of PR 1 + 5e12a6f.

## 3. Must not go upstream

- Files: STATE.md, CLAUDE.md, FORK-CHANGELOG.md, reports/, rust-toolchain.toml, `rust-version = "1.99.1"` in Cargo.toml,
  .github/scripts/rust-toolchain.test.mjs.
- 6215690 / b9fa1cb: `HighestFidelity/OpenMouse-Bridge` in src/updater.rs, src/updater/feed.rs, src/updater/tests.rs.
  Any PR touching src/updater must be cut from the pre-feed commits.
- Code comments citing our audit IDs: "(audit N1)" signature.rs, "(audit N3)" api.rs and peer.rs, "(audit N9)" and
  "(audit N12)" release.yml / gpui-build.yml, "(audit N12)" hid.rs; "upstream #79" in service.rs (say "#79").
- Commit messages: fork tags in titles would auto-link unrelated upstream issues/PRs: "(#1 #20)", "(#15)", "(#25)",
  "(#10)", "(#22)", "(#6b)", "(idea #7)", "(N1)" .. "(N12)", "P9-OMB..", "Owner approved (2026-11-11 00:10 PT)",
  "(upstream PR 60)". Reword every commit; keep the Co-Authored-By trailer.
- Fork policy bits inside otherwise-upstream commits: the "Require signing secrets" step and the no-clobber upload
  (3ee7f5a), the auto-merge removal and protocol-shortcut removal (762c2ea), CI on main (a73f057), the Rust pin in the
  workflows (c618e44).
- Secret names: only ones upstream already uses (WINDOWS_*, MACOS_*, APPLE_*, PROTOCOL_UPDATE_TOKEN). UPDATE_SIGNING_KEY
  only appears with PR 20, which is theirs to provision. No secret values anywhere in the diff (checked).
- Debugging leftovers: none found. Added `unwrap()`s are in tests; the one `console.log` is the release step's
  `node -e` output, intended.

## Checks run

- `git cherry-pick` of each fork commit alone and in stacks onto bd7035a (results above).
- `git apply --check` of the triage's pr58/60/62 diffs onto bd7035a: #60, #62 apply; #58 fails (install-linux.sh, src/drivers/mod.rs).
- Build of the security stack (164c7d3 1d21c91 0f2d9fa 582e685 7220ae7 28d2cf1 0d642f5 e91f3cb 6c8c082 6c51951 997d64d on
  bd7035a, repo branch sec-stack): `cargo test --locked` 267 + 39 + 11 passed, 0 failed; clippy -D warnings clean;
  fmt clean.
- Build of the bug/perf stack (997d64d 9174b33 46f837c 9848c2f 4051016 90fcc5a 4822aa6 cec280f 1267e7f 5e12a6f 8ccd795
  a1b097e 8445488, then ec7efa7 with the catalog hunk dropped; repo2 branch tier2): 268 + 41 + 11 passed, 0 failed;
  clippy and fmt clean. Each PR on its own was not built separately [Unverified per PR]; node tests for
  .github/scripts not rerun here.
- Workflows: not run on a GitHub runner [Unverified in CI], same as the fork.
