//! Key bindings (port_map `gui`, the options window's CONTROLS tab): Kenshi reads controls.cfg,
//! "action=key" lines, an action bound to several keys listed once per key, a key written with
//! its modifiers ("Shift+F12", "Ctrl+Shift+F11"). The game asks for an action, never a key, so a
//! rebinding saved from the options window takes effect at once.
//!
//! (assumption) Key names are OIS's as controls.cfg writes them; names that file does not use
//! (e.g. "Right Alt", "NUM*") are this module's own guesses at the same scheme. A binding without
//! modifiers does not fire while a binding of the same key with modifiers does ("toggle_fps_camera=;"
//! against "toggle_camera_grid=Shift+;").

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use kenshi_io::cfg::Cfg;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Button {
    Key(KeyCode),
    Mouse(MouseButton),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Mods {
    shift: bool,
    ctrl: bool,
    alt: bool,
}

impl Mods {
    fn any(self) -> bool {
        self.shift || self.ctrl || self.alt
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Combo {
    pub mods: Mods,
    pub button: Button,
}

const NAMED: &[(&str, KeyCode)] = &[
    ("Space", KeyCode::Space),
    ("Backspace", KeyCode::Backspace),
    ("Tab", KeyCode::Tab),
    ("Return", KeyCode::Enter),
    ("Escape", KeyCode::Escape),
    ("Delete", KeyCode::Delete),
    ("Insert", KeyCode::Insert),
    ("Home", KeyCode::Home),
    ("End", KeyCode::End),
    ("PgUp", KeyCode::PageUp),
    ("PgDn", KeyCode::PageDown),
    ("Up", KeyCode::ArrowUp),
    ("Down", KeyCode::ArrowDown),
    ("Left", KeyCode::ArrowLeft),
    ("Right", KeyCode::ArrowRight),
    ("-", KeyCode::Minus),
    ("=", KeyCode::Equal),
    (",", KeyCode::Comma),
    (".", KeyCode::Period),
    ("[", KeyCode::BracketLeft),
    ("]", KeyCode::BracketRight),
    (";", KeyCode::Semicolon),
    ("'", KeyCode::Quote),
    ("/", KeyCode::Slash),
    ("\\", KeyCode::Backslash),
    ("GRAVE", KeyCode::Backquote),
    ("SYSRQ", KeyCode::PrintScreen),
    ("NUM-", KeyCode::NumpadSubtract),
    ("NUM+", KeyCode::NumpadAdd),
    ("NUM*", KeyCode::NumpadMultiply),
    ("NUM/", KeyCode::NumpadDivide),
    ("NUM.", KeyCode::NumpadDecimal),
    ("NUMEnter", KeyCode::NumpadEnter),
    ("Left Alt", KeyCode::AltLeft),
    ("Right Alt", KeyCode::AltRight),
    ("Left Control", KeyCode::ControlLeft),
    ("Right Control", KeyCode::ControlRight),
    ("Left Shift", KeyCode::ShiftLeft),
    ("Right Shift", KeyCode::ShiftRight),
];

const LETTERS: [KeyCode; 26] = [
    KeyCode::KeyA, KeyCode::KeyB, KeyCode::KeyC, KeyCode::KeyD, KeyCode::KeyE, KeyCode::KeyF, KeyCode::KeyG,
    KeyCode::KeyH, KeyCode::KeyI, KeyCode::KeyJ, KeyCode::KeyK, KeyCode::KeyL, KeyCode::KeyM, KeyCode::KeyN,
    KeyCode::KeyO, KeyCode::KeyP, KeyCode::KeyQ, KeyCode::KeyR, KeyCode::KeyS, KeyCode::KeyT, KeyCode::KeyU,
    KeyCode::KeyV, KeyCode::KeyW, KeyCode::KeyX, KeyCode::KeyY, KeyCode::KeyZ,
];
const DIGITS: [KeyCode; 10] = [
    KeyCode::Digit0, KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4,
    KeyCode::Digit5, KeyCode::Digit6, KeyCode::Digit7, KeyCode::Digit8, KeyCode::Digit9,
];
const NUMPAD: [KeyCode; 10] = [
    KeyCode::Numpad0, KeyCode::Numpad1, KeyCode::Numpad2, KeyCode::Numpad3, KeyCode::Numpad4,
    KeyCode::Numpad5, KeyCode::Numpad6, KeyCode::Numpad7, KeyCode::Numpad8, KeyCode::Numpad9,
];
const FKEYS: [KeyCode; 12] = [
    KeyCode::F1, KeyCode::F2, KeyCode::F3, KeyCode::F4, KeyCode::F5, KeyCode::F6,
    KeyCode::F7, KeyCode::F8, KeyCode::F9, KeyCode::F10, KeyCode::F11, KeyCode::F12,
];
const MOUSE: [MouseButton; 3] = [MouseButton::Left, MouseButton::Right, MouseButton::Middle];

/// A key's name as controls.cfg writes it (F1, NUM0, Left Alt, A).
pub fn key_name(k: KeyCode) -> String {
    if let Some(i) = LETTERS.iter().position(|&c| c == k) {
        return ((b'A' + i as u8) as char).to_string();
    }
    if let Some(i) = DIGITS.iter().position(|&c| c == k) {
        return i.to_string();
    }
    if let Some(i) = NUMPAD.iter().position(|&c| c == k) {
        return format!("NUM{i}");
    }
    if let Some(i) = FKEYS.iter().position(|&c| c == k) {
        return format!("F{}", i + 1);
    }
    NAMED.iter().find(|(_, c)| *c == k).map_or_else(|| format!("{k:?}"), |(n, _)| n.to_string())
}

fn parse_button(name: &str) -> Option<Button> {
    if let Some(n) = name.strip_prefix("Mouse").and_then(|n| n.parse::<usize>().ok()) {
        return MOUSE.get(n.checked_sub(1)?).map(|&m| Button::Mouse(m)).or(Some(Button::Mouse(MouseButton::Other(n as u16))));
    }
    if let Some((_, k)) = NAMED.iter().find(|(n, _)| *n == name) {
        return Some(Button::Key(*k));
    }
    let b = name.as_bytes();
    if b.len() == 1 && b[0].is_ascii_alphabetic() {
        return Some(Button::Key(LETTERS[(b[0].to_ascii_uppercase() - b'A') as usize]));
    }
    if b.len() == 1 && b[0].is_ascii_digit() {
        return Some(Button::Key(DIGITS[(b[0] - b'0') as usize]));
    }
    if let Some(n) = name.strip_prefix("NUM").and_then(|n| n.parse::<usize>().ok()) {
        return NUMPAD.get(n).map(|&k| Button::Key(k));
    }
    if let Some(n) = name.strip_prefix('F').and_then(|n| n.parse::<usize>().ok()) {
        return FKEYS.get(n.checked_sub(1)?).map(|&k| Button::Key(k));
    }
    None
}

/// "Ctrl+Shift+F11" -> the combo. A lone "+" or a name ending in "+" ("NUM+") is a key.
pub fn parse(s: &str) -> Option<Combo> {
    let mut mods = Mods::default();
    let mut rest = s.trim();
    loop {
        if let Some(r) = rest.strip_prefix("Shift+").filter(|r| !r.is_empty()) {
            mods.shift = true;
            rest = r;
        } else if let Some(r) = rest.strip_prefix("Ctrl+").filter(|r| !r.is_empty()) {
            mods.ctrl = true;
            rest = r;
        } else if let Some(r) = rest.strip_prefix("Alt+").filter(|r| !r.is_empty()) {
            mods.alt = true;
            rest = r;
        } else {
            break;
        }
    }
    Some(Combo { mods, button: parse_button(rest)? })
}

/// The bindings, controls.cfg's (the install's, the rewrite's own saved copy over it).
#[derive(Resource, Default)]
pub struct Bindings {
    actions: HashMap<String, Vec<Combo>>,
}

impl Bindings {
    pub fn from_cfg(cfg: &Cfg) -> Bindings {
        let mut actions: HashMap<String, Vec<Combo>> = HashMap::new();
        for (k, v) in &cfg.lines {
            if let Some(c) = parse(v) {
                actions.entry(k.clone()).or_default().push(c);
            }
        }
        Bindings { actions }
    }

    pub fn load(root: &std::path::Path) -> Bindings {
        Bindings::from_cfg(&kenshi_io::cfg::load(root, "controls.cfg"))
    }
}

/// The input as actions: `just("toggle_map")`, `held("camera_left")`. Keyboard actions do not
/// fire while an edit box has the keyboard.
#[derive(SystemParam)]
pub struct Actions<'w, 's> {
    keys: Res<'w, ButtonInput<KeyCode>>,
    mouse: Res<'w, ButtonInput<MouseButton>>,
    binds: Res<'w, Bindings>,
    typing: Query<'w, 's, &'static kenshi_gui::EditText>,
    ui: Option<Res<'w, UiPresses>>,
}

/// Actions pressed by the HUD's buttons (its INV and STA shortcuts press the keys' own actions):
/// pushed to `next`, seen by [`Actions::just`] all through the next frame, whatever runs first.
/// (A pretend key press used to stay held, so a second click did nothing.)
#[derive(Resource, Default)]
pub struct UiPresses {
    pub next: Vec<String>,
    now: Vec<String>,
}

/// PreUpdate: the buttons' presses of the last frame are this frame's.
pub fn turn_presses(mut p: ResMut<UiPresses>) {
    if !p.next.is_empty() || !p.now.is_empty() {
        p.now = std::mem::take(&mut p.next);
    }
}

impl Actions<'_, '_> {
    fn mods(&self) -> Mods {
        let k = &self.keys;
        Mods {
            shift: k.pressed(KeyCode::ShiftLeft) || k.pressed(KeyCode::ShiftRight),
            ctrl: k.pressed(KeyCode::ControlLeft) || k.pressed(KeyCode::ControlRight),
            alt: k.pressed(KeyCode::AltLeft) || k.pressed(KeyCode::AltRight),
        }
    }

    /// The combo's modifiers are held (exactly those, for a combo with modifiers; any, without:
    /// shift-panning still pans).
    fn mods_ok(&self, c: &Combo, held: Mods) -> bool {
        (!c.mods.shift || held.shift) && (!c.mods.ctrl || held.ctrl) && (!c.mods.alt || held.alt)
    }

    fn typing(&self) -> bool {
        kenshi_gui::ui::typing_now(&self.typing)
    }

    fn down(&self, b: Button, just: bool) -> bool {
        match (b, just) {
            (Button::Key(k), true) => self.keys.just_pressed(k),
            (Button::Key(k), false) => self.keys.pressed(k),
            (Button::Mouse(m), true) => self.mouse.just_pressed(m),
            (Button::Mouse(m), false) => self.mouse.pressed(m),
        }
    }

    /// Pressed this frame.
    pub fn just(&self, action: &str) -> bool {
        if self.ui.as_ref().is_some_and(|u| u.now.iter().any(|a| a == action)) {
            return true;
        }
        let Some(combos) = self.binds.actions.get(action) else { return false };
        let held = self.mods();
        let typing = self.typing();
        combos.iter().any(|c| {
            if typing && matches!(c.button, Button::Key(_)) {
                return false;
            }
            if !self.down(c.button, true) || !self.mods_ok(c, held) {
                return false;
            }
            // a plain binding gives way to another action's binding of the key with held modifiers
            c.mods.any()
                || !self.binds.actions.values().flatten().any(|o| o.button == c.button && o.mods.any() && self.mods_ok(o, held))
        })
    }

    /// Held down.
    pub fn held(&self, action: &str) -> bool {
        let Some(combos) = self.binds.actions.get(action) else { return false };
        let held = self.mods();
        let typing = self.typing();
        combos.iter().any(|c| !(typing && matches!(c.button, Button::Key(_))) && self.down(c.button, false) && self.mods_ok(c, held))
    }

    /// Raw keys, for what controls.cfg does not bind (Escape, dialogue choices, debug keys).
    pub fn keys(&self) -> &ButtonInput<KeyCode> {
        &self.keys
    }

    pub fn is_typing(&self) -> bool {
        self.typing()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for n in ["A", "0", "NUM6", "F12", "Left Alt", "PgDn", "GRAVE", ",", "NUM+"] {
            let Some(Combo { button: Button::Key(k), mods }) = parse(n) else { panic!("{n}") };
            assert!(!mods.any());
            assert_eq!(key_name(k), n);
        }
        let c = parse("Ctrl+Shift+F11").unwrap();
        assert!(c.mods.ctrl && c.mods.shift && !c.mods.alt);
        assert_eq!(c.button, Button::Key(KeyCode::F11));
        assert_eq!(parse("Mouse3").unwrap().button, Button::Mouse(MouseButton::Middle));
        assert_eq!(parse("Shift+;").unwrap().button, Button::Key(KeyCode::Semicolon));
    }
}
