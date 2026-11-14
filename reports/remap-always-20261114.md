# F4.1 Always-on button remap (t3/remap-always, 2026-11-14)

## Buttons
- Assignable: Back (bit 8) and Forward (bit 16) only. Protocol v0.59.0 `onButtonChange` documents 0x01 left, 0x02 right, 0x04 middle, 0x08 back, 0x10 forward (PRO X 3 Superstrike capture). No DPI bit is reported.
- In Host mode only the side buttons stop clicking. Left, right and middle still click by themselves, so a key on them would fire on top of the click. They are left alone. [Inference] Other mice (MX 3S) may report more bits. Bridge now logs the helper's `ready.buttons` list so hardware can settle it.
## Allowed actions (src/buttons/remap.rs)
- One key from a fixed list of 89 (A-Z, 0-9, punctuation, F1-F24, navigation, Enter/Esc/Tab/Space/Backspace, PrtSc/ScrLk/Pause), plus optional Ctrl/Shift/Alt/Win (Cmd on macOS).
- One media key: play/pause, next, previous, stop, mute, volume up, volume down.
- One mouse click: left, right, middle, back, forward. It is held while the button is held.
- On macOS the list drops keys a Mac lacks and media keys, which Bridge cannot press there yet.
## Precedence and config
- config.json gets `"buttonRemap": {"back": {"key": "F13", "modifiers": ["Ctrl"]}, "forward": {"mouse": "Middle"}}`. It is omitted when empty, so old configs stay byte-identical and behave exactly as before.
- `buttons::resolve`: Bridge's own assignments apply with no game running. When a game profile's `hostButtons` assigns a button, the game wins for that button and the other button falls through to Bridge's own.
- Bridge's own assignments alone use `requireHost: true` and no device name. They never switch a mouse to Host and use whichever Logitech mouse answers first.
## Security
- `PUT /v1/profiles` and `PUT /v1/default-profile` check `snapshot.hostButtons` strictly against the same lists: bits 8/16 only, `{key, modifiers<=15}` or `{consumer}`, and no other fields (no sequences, text, macros or repeat). Anything else gets a 400 naming the profile and the reason, and nothing is saved. Router tests cover both routes.
- The dispatcher (`Assignments::from_profile`) parses with the same function, so the two cannot drift. An unreadable `buttonRemap` entry in config.json is dropped with a warning.
- No API writes `buttonRemap`; only the panel does. `/v1/status` shows it read-only. A key is still pressed only from a button event of the mouse the helper opened, never from an API call.
- Behaviour change: one stored profile with an off-list `hostButtons` now fails the whole `PUT /v1/profiles`. Before, its assignments were silently ignored.
## UI (src/gpui_shell/panel.rs, Settings)
- "Button assignments" card. Each button row has its label, arrows that step through the allowed actions, the current action ("Normal click" when unset) and a clear button. A key also gets Ctrl/Shift/Alt/Win toggles.
## Not tested
- The app was never run and the panel never rendered (layout, `ChevronLeft`/`Close` icons, 104 px label column).
- `src/buttons/linux.rs` and `src/buttons/macos.rs` were edited but never compiled (only the msvc target is installed). The pure mappings in `keymap.rs` are tested.
- Injection was never run on hardware: Windows left/right/middle via SendInput, macOS left/right CGEvents, the Linux uinput mouse now declaring BTN_RIGHT/BTN_MIDDLE.
- Limits:
  - Bridge's own assignments do nothing on a mouse in Onboard or "Unknown" mode.
  - While the mouse is not on Host, the helper is retried every 15 s (as the default profile already does).
  - With several Logitech mice, the first one that answers is used.
## Gestures next (F4.2, needs MX hardware)
- Divert the gesture button and raw XY through HID++ 0x1b04 (reprogControls) in mouse-protocol; nothing in the protocol does this yet.
- Helper: report gesture start/move/end. Bridge: classify swipes (up/down/left/right/tap), then map each direction through the same `remap` allow-list.
- Capture an MX Master 3/3S first: the button list, CIDs and raw XY events.
