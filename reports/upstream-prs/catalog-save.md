fix(config): Bridge still starts when it can't save the downloaded game list

At startup Bridge fetches the game catalog and, when it changed, saves it to config.json. That save used `?`, so if config.json couldn't be written (read-only file, full disk, a sync tool holding a lock), `load_with_catalog` failed and Bridge didn't start at all, even though it had a perfectly good catalog in memory.

What changed:
- A failed save now logs a warning and Bridge keeps going with the fetched catalog for this run. It gets written with the next settings change.
- The fetch-and-apply step moved into `apply_fetched_catalog` (normalizes the games, saves only when they changed). The "catalog unavailable" log moved into `log_catalog_unavailable`. Same messages as before.

Tested:
- New unit tests: `a_catalog_that_cannot_be_saved_is_still_used` (save forced to fail, catalog still applied) and `an_unchanged_catalog_is_not_saved_again` (same games in another order and case write nothing; a new game is saved).
- `cargo fmt --check`, `cargo test --locked`, and clippy with `-D warnings` for Linux and x86_64-pc-windows-gnu all pass.
- A fork build with this change passed a quick Windows test. I didn't make config.json unwritable on a real install.
