//! Bridge's own button assignments, which apply whichever game runs, and the
//! fixed list of actions any assignment may hold.
//!
//! Assignments reach Bridge from two places: the panel's editor (saved as
//! `buttonRemap` in config.json) and a web page's game profile
//! (`snapshot.hostButtons`, through the profile API). Both are checked here
//! against one allow-list: one key with optional modifiers, one media key, or
//! one mouse button. Nothing else: no sequences, no text, no macros, no
//! repeat. A key is only ever pressed while the mouse's own button is held,
//! from a button event of the mouse the helper opened.
//!
//! config.json: `"buttonRemap": {"back": {"key": "F13", "modifiers": ["Ctrl"]},
//! "forward": {"media": "VolumeUp"}}`, or `{"mouse": "Middle"}`.

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize, Serializer, ser::SerializeMap};
use serde_json::{Map, Value};

use super::{Action, BACK, FRONT, KeyAction, MouseButton, pressable};

/// A mouse button an assignment can be given to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RemapButton {
    /// Its bit in the mouse's button report.
    pub bit: u8,
    /// Its name in config.json.
    pub name: &'static str,
    pub label: &'static str,
}

/// Only the side buttons: in Host mode the mouse stops clicking them itself and
/// leaves them to Bridge. Left, right and middle still click on their own, so
/// a key given to them would come on top of the click. No DPI button is
/// reported by the button events.
pub const BUTTONS: [RemapButton; 2] = [
    RemapButton {
        bit: BACK,
        name: "back",
        label: "Back button",
    },
    RemapButton {
        bit: FRONT,
        name: "forward",
        label: "Forward button",
    },
];

/// The modifiers, by name and mask bit (the mask profiles store).
pub const MODIFIERS: [(&str, u8); 4] = [
    ("Ctrl", 0x01),
    ("Shift", 0x02),
    ("Alt", 0x04),
    ("Win", 0x08),
];
const ALL_MODIFIERS: u8 = 0x0f;

/// The keys an assignment may press, by name and HID keyboard usage. Every one
/// has a key on Windows and Linux; `buttons::virtual_key` is the same list.
pub const KEYS: &[(&str, u8)] = &[
    ("A", 0x04),
    ("B", 0x05),
    ("C", 0x06),
    ("D", 0x07),
    ("E", 0x08),
    ("F", 0x09),
    ("G", 0x0a),
    ("H", 0x0b),
    ("I", 0x0c),
    ("J", 0x0d),
    ("K", 0x0e),
    ("L", 0x0f),
    ("M", 0x10),
    ("N", 0x11),
    ("O", 0x12),
    ("P", 0x13),
    ("Q", 0x14),
    ("R", 0x15),
    ("S", 0x16),
    ("T", 0x17),
    ("U", 0x18),
    ("V", 0x19),
    ("W", 0x1a),
    ("X", 0x1b),
    ("Y", 0x1c),
    ("Z", 0x1d),
    ("1", 0x1e),
    ("2", 0x1f),
    ("3", 0x20),
    ("4", 0x21),
    ("5", 0x22),
    ("6", 0x23),
    ("7", 0x24),
    ("8", 0x25),
    ("9", 0x26),
    ("0", 0x27),
    ("Enter", 0x28),
    ("Escape", 0x29),
    ("Backspace", 0x2a),
    ("Tab", 0x2b),
    ("Space", 0x2c),
    ("Minus", 0x2d),
    ("Equal", 0x2e),
    ("LeftBracket", 0x2f),
    ("RightBracket", 0x30),
    ("Backslash", 0x31),
    ("Semicolon", 0x33),
    ("Quote", 0x34),
    ("Backquote", 0x35),
    ("Comma", 0x36),
    ("Period", 0x37),
    ("Slash", 0x38),
    ("F1", 0x3a),
    ("F2", 0x3b),
    ("F3", 0x3c),
    ("F4", 0x3d),
    ("F5", 0x3e),
    ("F6", 0x3f),
    ("F7", 0x40),
    ("F8", 0x41),
    ("F9", 0x42),
    ("F10", 0x43),
    ("F11", 0x44),
    ("F12", 0x45),
    ("PrintScreen", 0x46),
    ("ScrollLock", 0x47),
    ("Pause", 0x48),
    ("Insert", 0x49),
    ("Home", 0x4a),
    ("PageUp", 0x4b),
    ("Delete", 0x4c),
    ("End", 0x4d),
    ("PageDown", 0x4e),
    ("Right", 0x4f),
    ("Left", 0x50),
    ("Down", 0x51),
    ("Up", 0x52),
    ("F13", 0x68),
    ("F14", 0x69),
    ("F15", 0x6a),
    ("F16", 0x6b),
    ("F17", 0x6c),
    ("F18", 0x6d),
    ("F19", 0x6e),
    ("F20", 0x6f),
    ("F21", 0x70),
    ("F22", 0x71),
    ("F23", 0x72),
    ("F24", 0x73),
];

/// The media keys, by name, label and HID consumer usage.
pub const MEDIA: &[(&str, &str, u16)] = &[
    ("PlayPause", "Play / pause", 0xcd),
    ("NextTrack", "Next track", 0xb5),
    ("PreviousTrack", "Previous track", 0xb6),
    ("Stop", "Stop", 0xb7),
    ("Mute", "Mute", 0xe2),
    ("VolumeUp", "Volume up", 0xe9),
    ("VolumeDown", "Volume down", 0xea),
];

/// The mouse buttons, by name and label.
pub const MOUSE: &[(&str, &str, MouseButton)] = &[
    ("Left", "Left click", MouseButton::Left),
    ("Right", "Right click", MouseButton::Right),
    ("Middle", "Middle click", MouseButton::Middle),
    ("Back", "Back click", MouseButton::Back),
    ("Forward", "Forward click", MouseButton::Forward),
];

pub fn button_named(name: &str) -> Option<RemapButton> {
    BUTTONS.iter().copied().find(|button| button.name == name)
}

pub fn button_at(bit: u8) -> Option<RemapButton> {
    BUTTONS.iter().copied().find(|button| button.bit == bit)
}

fn key_named(name: &str) -> Option<u8> {
    KEYS.iter()
        .find(|(key, _)| *key == name)
        .map(|&(_, usage)| usage)
}

fn key_name(usage: u8) -> Option<&'static str> {
    KEYS.iter()
        .find(|(_, key)| *key == usage)
        .map(|&(name, _)| name)
}

/// Whether a key is on the allow-list.
pub fn allowed_key(usage: u8) -> bool {
    key_name(usage).is_some()
}

/// Whether a media key is on the allow-list.
pub fn allowed_media(usage: u16) -> bool {
    MEDIA.iter().any(|&(_, _, media)| media == usage)
}

/// Bridge's own assignments, by button bit. Empty is today's behaviour: the
/// side buttons keep their Back and Forward clicks.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ButtonRemap(BTreeMap<u8, Action>);

impl ButtonRemap {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn get(&self, bit: u8) -> Option<Action> {
        self.0.get(&bit).copied()
    }

    pub fn actions(&self) -> &BTreeMap<u8, Action> {
        &self.0
    }

    /// Gives a button an action, or clears it with `None`. Refuses a button or
    /// an action that is not on the lists.
    pub fn set(&mut self, bit: u8, action: Option<Action>) -> Result<(), String> {
        if button_at(bit).is_none() {
            return Err(format!("button {bit} cannot be assigned"));
        }
        match action {
            Some(action) => {
                check_action(action)?;
                self.0.insert(bit, action);
            }
            None => {
                self.0.remove(&bit);
            }
        }
        Ok(())
    }
}

fn check_action(action: Action) -> Result<(), String> {
    match action {
        Action::Key(KeyAction { usage, modifiers }) => {
            if !allowed_key(usage) {
                return Err(format!("key {usage} is not on Bridge's list of keys"));
            }
            if modifiers & !ALL_MODIFIERS != 0 {
                return Err(format!(
                    "modifier mask {modifiers} names an unknown modifier"
                ));
            }
        }
        Action::Media(usage) if !allowed_media(usage) => {
            return Err(format!("media key {usage} is not on Bridge's list"));
        }
        Action::Media(_) | Action::Mouse(_) => {}
    }
    Ok(())
}

/// One assignment as config.json holds it.
pub fn action_json(action: Action) -> Value {
    let mut object = Map::new();
    match action {
        Action::Key(KeyAction { usage, modifiers }) => {
            object.insert("key".into(), key_name(usage).unwrap_or_default().into());
            let names: Vec<Value> = MODIFIERS
                .iter()
                .filter(|(_, bit)| modifiers & bit != 0)
                .map(|(name, _)| Value::from(*name))
                .collect();
            if !names.is_empty() {
                object.insert("modifiers".into(), names.into());
            }
        }
        Action::Media(usage) => {
            let name = MEDIA
                .iter()
                .find(|&&(_, _, media)| media == usage)
                .map_or("", |&(name, _, _)| name);
            object.insert("media".into(), name.into());
        }
        Action::Mouse(button) => {
            let name = MOUSE
                .iter()
                .find(|&&(_, _, mouse)| mouse == button)
                .map_or("", |&(name, _, _)| name);
            object.insert("mouse".into(), name.into());
        }
    }
    Value::Object(object)
}

/// Reads one assignment as config.json holds it. Anything off the lists, an
/// extra field, or more than one action is refused, with the reason.
pub fn parse_action(value: &Value) -> Result<Action, String> {
    let object = value.as_object().ok_or("an assignment must be an object")?;
    if let Some(field) = object
        .keys()
        .find(|field| !matches!(field.as_str(), "key" | "modifiers" | "media" | "mouse"))
    {
        return Err(format!(
            "unexpected field \"{field}\": an assignment is one key with modifiers, one media key or one mouse button"
        ));
    }
    let named = |field: &str| -> Result<Option<&str>, String> {
        object
            .get(field)
            .map(|value| value.as_str().ok_or(format!("\"{field}\" must be a name")))
            .transpose()
    };
    let (key, media, mouse) = (named("key")?, named("media")?, named("mouse")?);
    if usize::from(key.is_some()) + usize::from(media.is_some()) + usize::from(mouse.is_some()) != 1
    {
        return Err("an assignment holds exactly one of \"key\", \"media\" or \"mouse\"".into());
    }
    if key.is_none() && object.contains_key("modifiers") {
        return Err("only a key takes modifiers".into());
    }
    if let Some(name) = key {
        let usage = key_named(name).ok_or(format!("\"{name}\" is not on Bridge's list of keys"))?;
        let mut modifiers = 0;
        if let Some(list) = object.get("modifiers") {
            let list = list
                .as_array()
                .ok_or("\"modifiers\" must be a list of names")?;
            for entry in list {
                let name = entry.as_str().unwrap_or_default();
                let bit = MODIFIERS
                    .iter()
                    .find(|(modifier, _)| *modifier == name)
                    .map(|&(_, bit)| bit)
                    .ok_or(format!("{entry} is not a modifier (Ctrl, Shift, Alt, Win)"))?;
                if modifiers & bit != 0 {
                    return Err(format!("{name} is named twice"));
                }
                modifiers |= bit;
            }
        }
        return Ok(Action::Key(KeyAction { usage, modifiers }));
    }
    if let Some(name) = media {
        return MEDIA
            .iter()
            .find(|(media, _, _)| *media == name)
            .map(|&(_, _, usage)| Action::Media(usage))
            .ok_or(format!("\"{name}\" is not on Bridge's list of media keys"));
    }
    let name = mouse.unwrap_or_default();
    MOUSE
        .iter()
        .find(|(mouse, _, _)| *mouse == name)
        .map(|&(_, _, button)| Action::Mouse(button))
        .ok_or(format!(
            "\"{name}\" is not a mouse button (Left, Right, Middle, Back, Forward)"
        ))
}

impl Serialize for ButtonRemap {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (bit, action) in &self.0 {
            if let Some(button) = button_at(*bit) {
                map.serialize_entry(button.name, &action_json(*action))?;
            }
        }
        map.end()
    }
}

/// Lenient on purpose: config.json that fails to parse is set aside whole, so
/// an entry Bridge cannot use (a hand edit, a newer release) is dropped with a
/// warning and the rest of the settings stay.
impl<'de> Deserialize<'de> for ButtonRemap {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        let mut remap = Self::default();
        let Some(object) = value.as_object() else {
            if !value.is_null() {
                tracing::warn!("buttonRemap in config.json is not an object; ignored");
            }
            return Ok(remap);
        };
        for (name, entry) in object {
            let Some(button) = button_named(name) else {
                tracing::warn!(name = %name, "buttonRemap names a button Bridge cannot assign; ignored");
                continue;
            };
            match parse_action(entry) {
                Ok(action) => {
                    remap.0.insert(button.bit, action);
                }
                Err(error) => {
                    tracing::warn!(name = %name, %error, "a buttonRemap entry was ignored")
                }
            }
        }
        Ok(remap)
    }
}

/// Checks a profile's `snapshot.hostButtons` from the profile API against the
/// same lists: `{"8": {"key": 58, "modifiers": 0}, "16": {"consumer": 233}}`,
/// keyed by button bit, each one HID keyboard usage with a modifier mask, or one
/// HID consumer usage. Absent or null is no assignments.
pub fn validate_host_buttons(snapshot: Option<&Value>) -> Result<(), String> {
    let Some(buttons) = snapshot.and_then(|snapshot| snapshot.get("hostButtons")) else {
        return Ok(());
    };
    if buttons.is_null() {
        return Ok(());
    }
    let buttons = buttons
        .as_object()
        .ok_or("hostButtons must be an object keyed by button")?;
    for (bit, action) in buttons {
        let at = |error: String| format!("hostButtons[\"{bit}\"]: {error}");
        bit.parse::<u8>()
            .ok()
            .and_then(button_at)
            .ok_or_else(|| at("only 8 (Back) and 16 (Forward) can be assigned".into()))?;
        parse_host_action(action).map_err(at)?;
    }
    Ok(())
}

/// One `hostButtons` entry, strictly: `{"key", "modifiers"}` or `{"consumer"}`.
pub fn parse_host_action(value: &Value) -> Result<Action, String> {
    let object = value.as_object().ok_or("an assignment must be an object")?;
    if let Some(field) = object
        .keys()
        .find(|field| !matches!(field.as_str(), "key" | "modifiers" | "consumer"))
    {
        return Err(format!(
            "unexpected field \"{field}\": an assignment is one key with modifiers or one media key"
        ));
    }
    let number = |field: &str| -> Result<Option<u64>, String> {
        object
            .get(field)
            .map(|value| {
                value
                    .as_u64()
                    .ok_or(format!("\"{field}\" must be a whole number"))
            })
            .transpose()
    };
    match (number("key")?, number("consumer")?) {
        (Some(key), None) => {
            let usage = u8::try_from(key)
                .ok()
                .filter(|usage| allowed_key(*usage))
                .ok_or(format!("key {key} is not on Bridge's list of keys"))?;
            let modifiers = number("modifiers")?.unwrap_or(0);
            let modifiers = u8::try_from(modifiers)
                .ok()
                .filter(|modifiers| modifiers & !ALL_MODIFIERS == 0)
                .ok_or(format!(
                    "modifier mask {modifiers} is not Ctrl 1, Shift 2, Alt 4, Win 8 combined"
                ))?;
            Ok(Action::Key(KeyAction { usage, modifiers }))
        }
        (None, Some(consumer)) => {
            if object.contains_key("modifiers") {
                return Err("only a key takes modifiers".into());
            }
            u16::try_from(consumer)
                .ok()
                .filter(|usage| allowed_media(*usage))
                .map(Action::Media)
                .ok_or(format!("media key {consumer} is not on Bridge's list"))
        }
        _ => Err("an assignment holds exactly one of \"key\" or \"consumer\"".into()),
    }
}

/// How the panel names an action: "Ctrl+Shift+F13", "Volume up", "Middle click".
pub fn action_label(action: Action) -> String {
    match action {
        Action::Key(KeyAction { usage, modifiers }) => {
            let mut parts: Vec<&str> = MODIFIERS
                .iter()
                .filter(|(_, bit)| modifiers & bit != 0)
                .map(|&(name, _)| {
                    if name == "Win" && cfg!(target_os = "macos") {
                        "Cmd"
                    } else {
                        name
                    }
                })
                .collect();
            parts.push(key_name(usage).unwrap_or("?"));
            parts.join("+")
        }
        Action::Media(usage) => MEDIA
            .iter()
            .find(|&&(_, _, media)| media == usage)
            .map_or_else(|| "Media key".into(), |&(_, label, _)| label.into()),
        Action::Mouse(button) => MOUSE
            .iter()
            .find(|&&(_, _, mouse)| mouse == button)
            .map_or_else(|| "Mouse button".into(), |&(_, label, _)| label.into()),
    }
}

/// Every action the panel offers, in the order its arrows step through them:
/// the mouse buttons, the media keys, then the keys (modifiers are added apart).
/// Only what this system can press: a Mac has no Print Screen, for one.
pub fn choices() -> Vec<Action> {
    MOUSE
        .iter()
        .map(|&(_, _, button)| Action::Mouse(button))
        .chain(MEDIA.iter().map(|&(_, _, usage)| Action::Media(usage)))
        .chain(KEYS.iter().map(|&(_, usage)| {
            Action::Key(KeyAction {
                usage,
                modifiers: 0,
            })
        }))
        .filter(|action| pressable(*action))
        .collect()
}

/// The action `steps` places from `current` in [`choices`], wrapping round;
/// a key keeps its modifiers when it steps to another key. No action steps to
/// the first (or, going back, the last) choice.
pub fn step_choice(current: Option<Action>, steps: i32) -> Action {
    let list = choices();
    let modifiers = match current {
        Some(Action::Key(key)) => key.modifiers,
        _ => 0,
    };
    let plain = current.map(|action| match action {
        Action::Key(key) => Action::Key(KeyAction {
            usage: key.usage,
            modifiers: 0,
        }),
        other => other,
    });
    let len = list.len() as i32;
    let index = match plain.and_then(|plain| list.iter().position(|choice| *choice == plain)) {
        Some(index) => (index as i32 + steps).rem_euclid(len),
        None if steps < 0 => len - 1,
        None => 0,
    };
    match list[index as usize] {
        Action::Key(key) => Action::Key(KeyAction {
            usage: key.usage,
            modifiers,
        }),
        other => other,
    }
}

/// Turns one modifier of a key on or off; other actions take no modifiers.
pub fn toggle_modifier(action: Action, bit: u8) -> Action {
    match action {
        Action::Key(key) => Action::Key(KeyAction {
            usage: key.usage,
            modifiers: (key.modifiers ^ bit) & ALL_MODIFIERS,
        }),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::buttons::{keymap, media_virtual_key, virtual_key};

    fn key(usage: u8, modifiers: u8) -> Action {
        Action::Key(KeyAction { usage, modifiers })
    }

    #[test]
    fn every_listed_key_and_media_key_can_be_pressed_on_windows_and_linux() {
        for &(name, usage) in KEYS {
            assert!(virtual_key(usage).is_some(), "{name} has a Windows key");
            assert!(keymap::linux_key(usage).is_some(), "{name} has a Linux key");
        }
        for &(name, _, usage) in MEDIA {
            assert!(
                media_virtual_key(usage).is_some(),
                "{name} has a Windows key"
            );
            assert!(
                keymap::linux_media_key(usage).is_some(),
                "{name} has a Linux key"
            );
        }
        assert_eq!(
            KEYS.len(),
            (0x04..=0x73u8)
                .filter(|usage| virtual_key(*usage).is_some())
                .count()
        );
    }

    #[test]
    fn the_assignable_buttons_are_the_side_buttons() {
        assert_eq!(
            BUTTONS.map(|button| button.bit),
            crate::buttons::SIDE_BUTTONS
        );
    }

    #[test]
    fn an_assignment_round_trips_through_its_config_form() {
        for action in [
            key(0x68, 0x01 | 0x02),
            key(0x04, 0),
            key(0x52, 0x0f),
            Action::Media(0xe9),
            Action::Mouse(MouseButton::Middle),
            Action::Mouse(MouseButton::Forward),
        ] {
            assert_eq!(parse_action(&action_json(action)), Ok(action), "{action:?}");
        }
        assert_eq!(
            action_json(key(0x68, 0x03)),
            json!({"key": "F13", "modifiers": ["Ctrl", "Shift"]})
        );
        assert_eq!(
            action_json(Action::Mouse(MouseButton::Middle)),
            json!({"mouse": "Middle"})
        );
    }

    #[test]
    fn anything_off_the_lists_is_refused_with_a_reason() {
        for (value, reason) in [
            (json!("F13"), "must be an object"),
            (json!({}), "exactly one"),
            (json!({"key": "F13", "media": "Mute"}), "exactly one"),
            (json!({"key": "F25"}), "not on Bridge's list"),
            (json!({"key": "f13"}), "not on Bridge's list"),
            (json!({"key": 104}), "must be a name"),
            (
                json!({"key": "A", "modifiers": ["Hyper"]}),
                "not a modifier",
            ),
            (
                json!({"key": "A", "modifiers": ["Ctrl", "Ctrl"]}),
                "named twice",
            ),
            (json!({"key": "A", "modifiers": "Ctrl"}), "list of names"),
            (
                json!({"media": "Mute", "modifiers": ["Ctrl"]}),
                "only a key",
            ),
            (json!({"media": "Eject"}), "media keys"),
            (json!({"mouse": "Button6"}), "not a mouse button"),
            (
                json!({"key": "A", "sequence": ["B", "C"]}),
                "unexpected field",
            ),
            (json!({"key": "A", "text": "hello"}), "unexpected field"),
            (json!({"key": "A", "repeat": 10}), "unexpected field"),
            (json!({"macro": [1, 2]}), "unexpected field"),
        ] {
            let error = parse_action(&value).expect_err(&value.to_string());
            assert!(error.contains(reason), "{value}: {error}");
        }
    }

    #[test]
    fn the_config_form_drops_what_it_cannot_use_and_keeps_the_rest() {
        let remap: ButtonRemap = serde_json::from_value(json!({
            "back": {"key": "F13", "modifiers": ["Alt"]},
            "forward": {"key": "A", "macro": "evil"},
            "middle": {"key": "B"},
        }))
        .expect("never fails");
        assert_eq!(remap.get(BACK), Some(key(0x68, 0x04)));
        assert_eq!(remap.get(FRONT), None, "a macro field drops the entry");
        assert_eq!(
            remap.actions().len(),
            1,
            "the middle button is not assignable"
        );
        assert_eq!(
            serde_json::to_value(&remap).unwrap(),
            json!({"back": {"key": "F13", "modifiers": ["Alt"]}})
        );
        let nothing: ButtonRemap = serde_json::from_value(json!([1, 2])).expect("never fails");
        assert!(nothing.is_empty());
    }

    #[test]
    fn setting_refuses_a_button_or_action_off_the_lists() {
        let mut remap = ButtonRemap::default();
        assert!(remap.set(0x04, Some(key(0x04, 0))).is_err(), "middle");
        assert!(remap.set(BACK, Some(key(0x01, 0))).is_err());
        assert!(remap.set(BACK, Some(key(0x04, 0x10))).is_err());
        assert!(remap.set(BACK, Some(Action::Media(0x01))).is_err());
        assert!(remap.is_empty());
        remap
            .set(BACK, Some(Action::Mouse(MouseButton::Middle)))
            .unwrap();
        assert_eq!(remap.get(BACK), Some(Action::Mouse(MouseButton::Middle)));
        remap.set(BACK, None).unwrap();
        assert!(remap.is_empty());
    }

    #[test]
    fn profile_host_buttons_take_only_listed_keys_on_the_side_buttons() {
        let ok = |buttons: Value| validate_host_buttons(Some(&json!({"hostButtons": buttons})));
        assert_eq!(validate_host_buttons(None), Ok(()));
        assert_eq!(validate_host_buttons(Some(&json!({"dpi": 800}))), Ok(()));
        assert_eq!(ok(Value::Null), Ok(()));
        assert_eq!(ok(json!({})), Ok(()));
        assert_eq!(
            ok(json!({"8": {"key": 58, "modifiers": 0}, "16": {"consumer": 233}})),
            Ok(())
        );
        assert_eq!(
            ok(json!({"8": {"key": 4}})),
            Ok(()),
            "modifiers default to none"
        );
        for (buttons, reason) in [
            (json!([]), "must be an object"),
            (json!({"4": {"key": 4}}), "only 8 (Back) and 16 (Forward)"),
            (
                json!({"back": {"key": 4}}),
                "only 8 (Back) and 16 (Forward)",
            ),
            (json!({"8": {"key": 999}}), "not on Bridge's list"),
            (json!({"8": {"key": 0}}), "not on Bridge's list"),
            (json!({"8": {"key": 0x32}}), "not on Bridge's list"),
            (json!({"8": {"key": "A"}}), "whole number"),
            (json!({"8": {"key": 4, "modifiers": 16}}), "modifier mask"),
            (json!({"8": {"key": 4, "modifiers": -1}}), "whole number"),
            (json!({"8": {"consumer": 1}}), "media key 1"),
            (
                json!({"8": {"consumer": 233, "modifiers": 1}}),
                "only a key",
            ),
            (json!({"8": {"key": 4, "consumer": 233}}), "exactly one"),
            (json!({"8": {"key": 4, "keys": [5, 6]}}), "unexpected field"),
            (json!({"8": {"text": "rm -rf"}}), "unexpected field"),
            (json!({"8": {"key": 4, "repeat": 50}}), "unexpected field"),
            (json!({"8": null}), "must be an object"),
        ] {
            let error = ok(buttons.clone()).expect_err(&buttons.to_string());
            assert!(error.contains(reason), "{buttons}: {error}");
            assert!(error.starts_with("hostButtons"), "{error}");
        }
    }

    #[test]
    fn the_panel_steps_through_every_choice_and_keeps_a_keys_modifiers() {
        let list = choices();
        if !cfg!(target_os = "macos") {
            assert_eq!(list.len(), MOUSE.len() + MEDIA.len() + KEYS.len());
        }
        assert_eq!(step_choice(None, 1), list[0]);
        assert_eq!(step_choice(None, -1), *list.last().unwrap());
        assert_eq!(
            step_choice(Some(list[0]), -1),
            *list.last().unwrap(),
            "wraps round"
        );
        assert_eq!(step_choice(Some(key(0x04, 0x01)), 1), key(0x05, 0x01));
        assert_eq!(toggle_modifier(key(0x04, 0x01), 0x02), key(0x04, 0x03));
        assert_eq!(toggle_modifier(key(0x04, 0x03), 0x01), key(0x04, 0x02));
        assert_eq!(
            toggle_modifier(Action::Media(0xe9), 0x01),
            Action::Media(0xe9),
            "a media key takes no modifiers"
        );
        if !cfg!(target_os = "macos") {
            assert_eq!(action_label(key(0x68, 0x0f)), "Ctrl+Shift+Alt+Win+F13");
        }
        assert_eq!(action_label(Action::Media(0xcd)), "Play / pause");
        assert_eq!(
            action_label(Action::Mouse(MouseButton::Middle)),
            "Middle click"
        );
    }
}
