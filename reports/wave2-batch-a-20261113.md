# Wave 2 batch A: upstream PR branches (2026-11-13)

Base: upstream/main 86974b1 (fetch found nothing newer). Each branch = 1 cherry-pick, applied clean, fix code only (no fork docs, no CHANGELOG.md).
All pushed to origin (fork). No PRs opened. Each commit has the Co-Authored-By: Claude Opus 5.5 trailer.

| Branch | Commit | From | Body file | Title (= commit subject) |
|---|---|---|---|---|
| pr/remote-image | c57645e | 28d2cf1 | reports/upstream-prs/remote-image.md | fix(remote-image): images from a URL can no longer reach your local network through older IPv6 ranges |
| pr/native-hid-path | 89a64b5 | 0d642f5 | reports/upstream-prs/native-hid-path.md | fix(native-hid): release builds never load the HID helper from the build machine's checkout path |
| pr/first-scan-battery | 48c5835 | 997d64d | reports/upstream-prs/first-scan-battery.md | fix(service): show the battery right away when the mouse shows up just after Bridge starts |
| pr/catalog-save | 48431cf | 9174b33 | reports/upstream-prs/catalog-save.md | fix(config): Bridge still starts when it can't save the downloaded game list |

Edits beyond the cherry-pick:
- first-scan-battery: commit says "Refs #79" (was "Fixes OpenMouse-Project/OpenMouse-Bridge#79", which would auto-close #79 on merge); 2 code comments "upstream #79" -> "#79". Body: "Refs #79", scan part only.
- catalog-save: dropped "(#6a)" and the sentence about the service reusing apply_fetched_catalog (fork-only code). Message amended after checks; tree unchanged (verified).
- native-hid-path body says it gates only the dev fallback; bundled paths untouched.
- Scopes kept as component names like upstream's own titles; weekly-release.mjs shows only linux/macos/windows scopes, so these render as plain sentences.
- Thanks line for the first 4 merges is in remote-image.md only (advisor item 9: once per batch).

Checks, each branch on its own (script temp/omb-wave2a-checks.sh = omb-checks.sh with CARGO_TARGET_DIR temp/omb-target-up):
| Branch | fmt | cargo test --locked | clippy -D warnings linux | clippy windows-gnu | node .github/scripts |
|---|---|---|---|---|---|
| remote-image | ok | 259+42+11 pass, 0 fail | clean | clean | 11 pass |
| native-hid-path | ok | 260+42+11 pass, 0 fail | clean | clean | 11 pass |
| first-scan-battery | ok | 260+42+11 pass, 0 fail | clean | clean | 11 pass |
| catalog-save | ok | 261+42+11 pass, 0 fail | clean | clean | 11 pass |
No branch touches native-hid/, so native-hid node tests not run. Not run on a GitHub runner or hardware [Unverified in CI].
Body "Tested" lines cite Victor's build-2 Windows test (STATE 2026-11-12: "it works and auto starts to, works"); it did not target these fixes specifically.

Leftovers: scratch worktree /home/claude/workspace/temp/omb-wave2a (detached) and local pr/* branches; not deleted (needs Victor's yes).
