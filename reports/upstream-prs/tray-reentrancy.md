fix(tray): move and raise the Windows panel outside the App borrow

Fixes #120.

Clicking the tray icon when the panel sits on a different monitor than the icon (the reporter's tray is on a monitor left of the primary, so it happens on every click) panics with "RefCell already borrowed" and Bridge exits.

Why it panics:
- The poll loop shows the panel with `cx.update_window(handle, |_, window, _| tray::show(window, anchor))`. `AsyncApp::update_window` holds the App borrow for the whole closure (gpui-pre 0.3.9 `src/app/async_context.rs:93`).
- On Windows `tray::show` calls `SetWindowPos`, `ShowWindow` and `SetForegroundWindow`. These run GPUI's window procedure synchronously.
- When the move puts the window on another monitor, `WM_MOVE` reaches `set_display` (gpui-pre-windows 0.3.9 `src/events.rs:196`, `947-953`), which fires `on_display_changed`. A `WM_DPICHANGED` gets there too, through the move it causes (`events.rs:915`).
- That callback is a plain `cx.update(...)` (gpui-pre `src/window.rs:1971-1974`), so `borrow_mut` at `async_context.rs:178` panics. Most other window callbacks use `try_borrow_mut` and only log an error when re-entered.

What changed:
- Windows only: a new `tray::show_from_loop` first works out the target position inside `update_window`. That step reads the HWND, the panel size and the monitor work area, and sends no window messages. After `update_window` returns, it calls `SetWindowPos`, `ShowWindow` and `SetForegroundWindow` with no App borrow held. A second `update_window` then calls `activate_window()`.
- macOS and Linux keep the existing `tray::show` call.
- The placement math moved into a pure function, `origin_near_tray`, so it can be tested.

Not changed: the other Win32 calls made under a borrow (hide, the startup placement, `keep_on_screen`) cannot change which monitor the window is on, so they never reach the panicking callback.

On the log in the issue: `panel_wanted=false` is expected. That flag is the API or second-launch request; a tray click arrives separately as a toggle.

Tested:
- 4 new unit tests for `origin_near_tray`: a monitor at negative x (the reporter's layout), the left-edge clamp, the no-click corner, and a top taskbar.
- `cargo fmt --check`, `cargo test --locked`, and clippy with `-D warnings` pass on Windows (MSVC), Linux and x86_64-pc-windows-gnu.
- Not reproduced on hardware: I only have a single-monitor setup, so the fix is verified against the GPUI source, not against a live crash. A two-monitor test before merging would be welcome.
