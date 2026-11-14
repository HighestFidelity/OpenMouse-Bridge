# Tray click panic "RefCell already borrowed" (upstream #120), 2026-11-14

Branch t3/tray-reentrancy, fix commit 39d23ec. Checks: fmt ok, clippy -D warnings ok, cargo test 341 passed / 0 failed / 4 ignored. Runtime fix [Unverified]: not run (fork rule).

## Root cause (confirmed in source; the fix itself is unverified until a run)
1. Poll loop `src/gpui_shell/mod.rs` "showing the panel" called `cx.update_window(handle, |_, w, _| tray::show(w, anchor))`. `AsyncApp::update_window` holds `app.try_borrow_mut()` for the whole closure (gpui-pre-0.3.9 src/app/async_context.rs:93).
2. `tray::show` on Windows ran `SetWindowPos`, `ShowWindow` and `SetForegroundWindow`, which call GPUI's window procedure synchronously.
3. `WM_MOVE` goes to `handle_move_msg` (gpui-pre-windows-0.3.9 src/events.rs:99, 174). If the window centre is now on another monitor, it calls `set_display` (events.rs:196, 947-953), which fires `on_display_changed`. WM_DPICHANGED reaches the same place through its own SetWindowPos and the WM_MOVE that follows (events.rs:915). The other set_display caller, events.rs:943, handles WM_DISPLAYCHANGE.
4. That callback is `move || cx.update(|cx| Window::display_changed(handle, cx))` (gpui-pre-0.3.9 src/window.rs:1971-1974). It is a plain `AsyncApp::update`, so `borrow_mut` (async_context.rs:178) panics.
5. Correction to the earlier guess: the resize, moved, active, visibility, input and request_frame callbacks all use `handle.update` (`try_borrow_mut`). Re-entered, those log an error and do not panic. The only plain `cx.update` callbacks are on_close (window.rs:1755) and on_display_changed.
6. Trigger: the panel's current monitor differs from the tray icon's monitor at click time. A tray on a monitor left of the primary does that every time, because the panel opens on the primary.

## panel_wanted=false is not a bug
`panel_wanted` is `drained.panel_requested`, the API or second-launch request (mod.rs:250). A tray click arrives as `Toggle` in `tray_requests`, and `panel_wanted_after` turns it into `show`. No change.

## Fix
- `tray::show_from_loop` (Windows only). First `update_window` runs `platform::plan_near_tray`, which reads the HWND, size and monitor work area and sends no messages. After it returns, `platform::place_and_raise` runs `SetWindowPos`, `ShowWindow` and `SetForegroundWindow` with no App borrow. A second `update_window` calls `activate_window()` (already deferred by GPUI), then VISIBLE is set. There is no await between plan and apply.
- macOS and Linux keep the old `tray::show` line (`#[cfg(not(windows))]`).
- Placement math moved to the pure `origin_near_tray`, with 4 tests: monitor at negative x, left-edge clamp, no-click corner, top taskbar.
- Other Win32 calls under a borrow, left as is because none can change the window's monitor:
  - Hide (`SW_HIDE`): mod.rs:195, 201, 295; panel.rs:971, 1671. `WM_SHOWWINDOW` only draws via try_borrow (events.rs:1260).
  - Startup `place(None)`, mod.rs:205: moves from centred on primary to the primary corner.
  - `keep_on_screen`, panel.rs:1648: vertical move on the same monitor, inside render.
  - tray.rs:732 (on_window_should_close): registers a handler and makes no Win32 call.

## Repro for Victor (Windows, 2 monitors)
1. Old exe: put the taskbar tray (or the overflow) on the secondary monitor and left-click the icon. It should crash, with log "showing the panel" then exit.
2. No tray on the secondary monitor [Unverified]: open the panel, Win+Shift+Arrow it to the other monitor, hide it with Esc, then click the tray.
3. New exe `Desktop\openmouse-bridge-tray-fix.exe` (sha256 4a3897d653a1987b183895cb4c7238e0da71c6efe60d404656c1ddc981900925): same steps. The panel should open by the icon on that monitor; click again to hide.
