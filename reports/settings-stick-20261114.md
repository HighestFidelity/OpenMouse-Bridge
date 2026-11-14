# F1 settings stick (t3/settings-stick, 2026-11-14)

Motivation: openmouse #1473, where the MX Master 3S DPI resets to 1000 after sleep/wake. Approved by Victor in APPROVALS on 2026-11-14.

## Design
- Trigger: `record_battery`, only for native readings (Bridge read the mouse itself). A panel `PUT /v1/battery` never triggers it.
  - Wake: the reading removes the mouse from `state.asleep`.
  - Replug: the device scanner (not on its first scan) marks known native mice whose own device or receiver reappeared on the bus (`returned_mice` puts them in `state.replugged`). The next answered native read of that mouse fires. Enumeration alone does not fire, so a mouse that is not ready yet does not use up the window.
- Gate (`admit_reapply`): no control panel connected (`panel_connected`: HID session or a heartbeat in the last 20 s), `device_mode == Onboard`, a profile with DPI or polling in force whose device has the same brand as the mouse, and at most once per mouse per 10 s (`REAPPLY_MIN_INTERVAL`, per-device `reapplied_at` map, pruned on admit).
- Action: `native_automation.reapply_on_next_reconcile()`. The game-monitor tick (3 s) then writes the profile in force again through the normal serialized pass. That pass already defers under `device_access` while a panel session is open, and the write stays owed through a profile debounce. One info log per admitted reapply (device_id, cause woke/reconnected).
- Mode semantics follow the code: Bridge applies the profile on every tick whatever Hybrid is set to, and Hybrid only adds the reapply on panel disconnect. So this feature runs whenever Bridge drives the mouse (onboard), never in Live mode.

## Files
- src/service.rs: `REAPPLY_MIN_INTERVAL`, `replugged`/`reapplied_at` state, trigger in `record_battery`, scanner hook, `returned_mice`, `admit_reapply`, 5 tests.
- src/drivers/automation.rs: `take_reapply_owed` (cfg(test) only).

## Tests (service::tests)
- a_mouse_waking_owes_one_reapply_within_the_window: a wake owes one reapply, a plain read owes none, and a second wake within 10 s owes none.
- the_reapply_window_is_per_mouse_and_reopens_after_ten_seconds: per-mouse limit, reopens at 10 s, refuses a mouse of another brand.
- a_connected_control_panel_suppresses_the_wake_reapply: an open HID session, Live mode and a fresh heartbeat each block it.
- only_a_native_read_of_a_woken_mouse_owes_a_reapply: a panel-reported reading does not trigger it.
- a_replugged_receiver_owes_a_reapply_on_the_next_native_read: receiver gone then back marks the mouse, and the next native read owes one reapply. It fills `replugged` by hand, so the scanner hook itself is checked only by reading the code.
- The existing automation test a_requested_reapply_writes_the_same_profile_once_more proves the owed flag forces the second write.

## Not hardware-tested / limits
- Not run on any real mouse. Whether the MX Master 3S actually keeps the rewritten DPI is unverified.
- A wake is only seen if a poll caught the mouse asleep. Polls run every 5 min, and every 60 s once a mouse is marked asleep. A sleep/wake between polls is missed, and a detected wake reapplies up to about 60 s late. For #1473 that is the weak spot. PC suspend/resume is not detected as such and is untested: Bridge is frozen during it and may see neither an asleep read nor the mouse leaving the bus.
- Race: if a panel session starts within the 3 s tick after admission, the owed write lands when the panel leaves, even with Hybrid off. The window is narrow.
- Brand-level match: with two mice of the same brand, waking the one the profile is not for also rewrites the profile's mouse.
