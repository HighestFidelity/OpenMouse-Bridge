//! Presses keys and side buttons on macOS by posting CoreGraphics events.
//!
//! macOS only lets a program post input once the user has allowed it under
//! System Settings > Privacy & Security > Accessibility; without that the
//! events are dropped silently, so `prepare` checks and asks for it.

use std::sync::{Mutex, PoisonError};

use objc2_core_foundation::CGPoint;
use objc2_core_graphics::{
    CGEvent, CGEventField, CGEventTapLocation, CGEventType, CGMouseButton,
    CGPreflightPostEventAccess, CGRequestPostEventAccess,
};

use super::{Action, BACK, HeldKeys, KeyAction, MouseButton, Step, keymap};

/// The keys held down, counted, so a modifier two buttons share is not let go early.
static HELD: Mutex<HeldKeys> = Mutex::new(HeldKeys::new());

/// Asks macOS to let Bridge post input, which shows its permission prompt the first time.
pub fn prepare() {
    if !CGPreflightPostEventAccess() {
        CGRequestPostEventAccess();
        tracing::warn!(
            "macOS has not allowed Bridge to press keys, so button assignments do nothing until it is \
             allowed under System Settings > Privacy & Security > Accessibility"
        );
    }
}

pub fn execute(step: Step) {
    match step {
        Step::Down(action) => press(action, true),
        Step::Up(action) => press(action, false),
        Step::SideDown(bit) => click(keymap::mac_mouse_button(side(bit)), true),
        Step::SideUp(bit) => click(keymap::mac_mouse_button(side(bit)), false),
    }
}

fn side(bit: u8) -> MouseButton {
    if bit == BACK {
        MouseButton::Back
    } else {
        MouseButton::Forward
    }
}

/// Keeps mouse buttons apart from keycodes in the held count.
const MOUSE_HELD: u16 = 0x1000;

/// Modifiers go down before the key and come up after it.
fn press(action: Action, down: bool) {
    if let Action::Mouse(button) = action {
        let number = keymap::mac_mouse_button(button);
        let code = MOUSE_HELD + number as u16;
        let changes = {
            let mut held = HELD.lock().unwrap_or_else(PoisonError::into_inner);
            if down {
                held.press(code)
            } else {
                held.release(code)
            }
        };
        if changes {
            click(number, down);
        }
        return;
    }
    let Action::Key(KeyAction { usage, modifiers }) = action else {
        tracing::warn!("Media keys cannot be pressed by Bridge on macOS yet");
        return;
    };
    let Some(key) = keymap::mac_key(usage) else {
        tracing::warn!(usage, "That key cannot be pressed by Bridge");
        return;
    };
    let mut order = keymap::mac_modifier_keys(modifiers);
    order.push(key);
    if !down {
        order.reverse();
    }
    let mut held = HELD.lock().unwrap_or_else(PoisonError::into_inner);
    for code in order {
        let changes = if down {
            held.press(code)
        } else {
            held.release(code)
        };
        if !changes {
            continue;
        }
        match CGEvent::new_keyboard_event(None, code, down) {
            Some(event) => CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&event)),
            None => tracing::warn!("macOS did not create the key event"),
        }
    }
}

/// Clicks mouse button `button` (0 left, 1 right, 2 middle, 3 Back, 4 Forward)
/// where the pointer is.
fn click(button: u32, down: bool) {
    let position =
        CGEvent::new(None).map_or(CGPoint::ZERO, |event| CGEvent::location(Some(&event)));
    let (kind, which) = match (button, down) {
        (0, true) => (CGEventType::LeftMouseDown, CGMouseButton::Left),
        (0, false) => (CGEventType::LeftMouseUp, CGMouseButton::Left),
        (1, true) => (CGEventType::RightMouseDown, CGMouseButton::Right),
        (1, false) => (CGEventType::RightMouseUp, CGMouseButton::Right),
        (_, true) => (CGEventType::OtherMouseDown, CGMouseButton::Center),
        (_, false) => (CGEventType::OtherMouseUp, CGMouseButton::Center),
    };
    match CGEvent::new_mouse_event(None, kind, position, which) {
        Some(event) => {
            CGEvent::set_integer_value_field(
                Some(&event),
                CGEventField::MouseEventButtonNumber,
                i64::from(button),
            );
            CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&event));
        }
        None => tracing::warn!("macOS did not create the mouse event"),
    }
}
