fix(linux): give hidraw to the installing account, not everyone, without logind

On systems without logind (OpenRC without elogind, for one), the udev rule falls back to `MODE="0666"`. That opens every hidraw node of the mouse's USB device to every local account. #84 already said it well: "A combo receiver also exposes its keyboard interface here, so world-readable is no longer acceptable." The logind path got fixed there, but the fallback still hands that keyboard interface to any account on the machine.

What changed:
- The no-logind fallback is now `OWNER="<uid>", MODE="0600"`, with the uid of the account installing the rule. Same change in the rule the panel writes (`src/hid_access.rs`) and in `install-linux.sh` (which uses `$SUDO_USER` when run through sudo).
- Systems with logind keep the seat-only `TAG+="uaccess"` rule, unchanged.

Trade-off: on a no-logind machine, other accounts lose access to the mouse through Bridge. That seemed like the right default to me, but say so if you see it differently.

Tested:
- Updated the rule unit test: the no-logind rule has the owner and 0600 and never 0666, the logind rule has no OWNER.
- Ran `write_udev_rule` from `install-linux.sh` on its own, both paths, and checked the rule it wrote. `bash -n` on the script is clean.
- `cargo fmt --check`, `cargo test --locked`, and clippy with `-D warnings` for Linux and x86_64-pc-windows-gnu all pass.
- Not tested on a real no-logind system with a mouse plugged in.

Sending this along with three other small security fixes; a few more (CI hardening, some bug fixes) are ready whenever you've had a chance to look at these.
