pub use minifb::Key;
use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub struct Input {
    down: HashSet<Key>,
    prev: HashSet<Key>,
    actions: HashMap<String, Vec<Key>>,
}

impl Input {
    pub(crate) fn poll(&mut self, win: &minifb::Window) {
        self.down = win.get_keys().into_iter().collect();
    }
    #[cfg(test)]
    pub(crate) fn hold(&mut self, keys: &[Key]) {
        self.down = keys.iter().copied().collect();
    }
    pub(crate) fn end_step(&mut self) {
        self.prev = self.down.clone();
    }
    pub(crate) fn bind(&mut self, action: &str, keys: &[Key]) {
        self.actions.insert(action.to_string(), keys.to_vec());
    }

    pub(crate) fn load_text(&mut self, text: &str) -> Result<(), String> {
        for (action, keys) in parse(text)? {
            self.bind(&action, &keys);
        }
        Ok(())
    }

    pub(crate) fn to_text(&self) -> String {
        let mut actions: Vec<_> = self.actions.iter().collect();
        actions.sort();
        let line = |(name, keys): &(&String, &Vec<Key>)| {
            let keys: Vec<String> = keys.iter().map(|k| format!("{k:?}")).collect();
            format!("{name} = {}\n", keys.join(" "))
        };
        actions.iter().map(line).collect()
    }

    fn keys(&self, action: &str) -> &[Key] {
        self.actions.get(action).unwrap_or_else(|| panic!("unknown input action {action:?}"))
    }

    pub fn down(&self, action: &str) -> bool {
        self.keys(action).iter().any(|k| self.down.contains(k))
    }
    pub fn pressed(&self, action: &str) -> bool {
        self.down(action) && !self.keys(action).iter().any(|k| self.prev.contains(k))
    }
    pub fn axis(&self, neg: &str, pos: &str) -> f32 {
        self.down(pos) as i32 as f32 - self.down(neg) as i32 as f32
    }
    pub fn key_down(&self, key: Key) -> bool {
        self.down.contains(&key)
    }
}

macro_rules! key_names {
    ($($k:ident),* $(,)?) => {
        const KEYS: &[(&str, Key)] = &[$((stringify!($k), Key::$k)),*];
    };
}

key_names!(
    Key0,
    Key1,
    Key2,
    Key3,
    Key4,
    Key5,
    Key6,
    Key7,
    Key8,
    Key9,
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    F13,
    F14,
    F15,
    Down,
    Left,
    Right,
    Up,
    Apostrophe,
    Backquote,
    Backslash,
    Comma,
    Equal,
    LeftBracket,
    Minus,
    Period,
    RightBracket,
    Semicolon,
    Slash,
    Backspace,
    Delete,
    End,
    Enter,
    Escape,
    Home,
    Insert,
    Menu,
    PageDown,
    PageUp,
    Pause,
    Space,
    Tab,
    NumLock,
    CapsLock,
    ScrollLock,
    LeftShift,
    RightShift,
    LeftCtrl,
    RightCtrl,
    NumPad0,
    NumPad1,
    NumPad2,
    NumPad3,
    NumPad4,
    NumPad5,
    NumPad6,
    NumPad7,
    NumPad8,
    NumPad9,
    NumPadDot,
    NumPadSlash,
    NumPadAsterisk,
    NumPadMinus,
    NumPadPlus,
    NumPadEnter,
    LeftAlt,
    RightAlt,
    LeftSuper,
    RightSuper,
);

fn parse(text: &str) -> Result<Vec<(String, Vec<Key>)>, String> {
    let mut out = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let n = i + 1;
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let (name, keys) = line.split_once('=').ok_or(format!("line {n}: expected `action = keys`"))?;
        let name = name.trim();
        if name.is_empty() || name.contains(char::is_whitespace) {
            return Err(format!("line {n}: bad action name {name:?}"));
        }
        let keys = keys
            .split_whitespace()
            .map(|k| {
                let found = KEYS.iter().find(|(known, _)| known.eq_ignore_ascii_case(k));
                found.map(|&(_, key)| key).ok_or(format!("line {n}: unknown key {k:?}"))
            })
            .collect::<Result<Vec<Key>, String>>()?;
        if keys.is_empty() {
            return Err(format!("line {n}: action {name:?} has no keys"));
        }
        out.push((name.to_string(), keys));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input_with(down: &[Key]) -> Input {
        let mut i = Input::default();
        i.bind("jump", &[Key::Space, Key::Up]);
        i.bind("left", &[Key::Left, Key::A]);
        i.bind("right", &[Key::Right, Key::D]);
        i.down = down.iter().copied().collect();
        i
    }

    #[test]
    fn any_bound_key_triggers_the_action_and_pressed_fires_once() {
        let mut i = input_with(&[Key::Up]);
        assert!(i.down("jump") && i.pressed("jump"));
        i.end_step();
        assert!(i.down("jump") && !i.pressed("jump"));
        i.down.insert(Key::Space);
        assert!(!i.pressed("jump"), "a second key joining a held action is not a new press");
        assert!(!i.down("left"));
    }

    #[test]
    fn axis_combines_alternative_keys_and_cancels_when_both_sides_are_held() {
        assert_eq!(input_with(&[Key::A]).axis("left", "right"), -1.0);
        assert_eq!(input_with(&[Key::Right]).axis("left", "right"), 1.0);
        assert_eq!(input_with(&[Key::A, Key::D]).axis("left", "right"), 0.0);
    }

    #[test]
    fn rebinding_replaces_the_old_keys() {
        let mut i = input_with(&[Key::Space]);
        i.bind("jump", &[Key::Z]);
        assert!(!i.down("jump"));
        i.down.insert(Key::Z);
        assert!(i.down("jump"));
    }

    #[test]
    #[should_panic(expected = "unknown input action")]
    fn querying_an_unbound_action_is_a_bug_not_a_silent_false() {
        input_with(&[]).down("jmup");
    }

    #[test]
    fn every_listed_key_name_matches_the_variant_it_names() {
        for (name, key) in KEYS {
            assert_eq!(format!("{key:?}"), *name);
        }
        let mut names: Vec<_> = KEYS.iter().map(|k| k.0).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), KEYS.len());
    }

    #[test]
    fn text_roundtrips_and_ignores_comments_blank_lines_and_case() {
        let mut i = Input::default();
        i.load_text("# controls\n\njump = space UP  # either\nleft = Left A\n").unwrap();
        i.down = [Key::Up].into_iter().collect();
        assert!(i.down("jump") && !i.down("left"));
        let saved = i.to_text();
        assert_eq!(saved, "jump = Space Up\nleft = Left A\n");
        let mut j = Input::default();
        j.load_text(&saved).unwrap();
        assert_eq!(j.to_text(), saved);
    }

    #[test]
    fn a_bad_file_names_the_line_and_changes_nothing() {
        let mut i = input_with(&[]);
        let before = i.to_text();
        for (text, expect) in [
            ("jump = Z\nfire = Spce\n", "line 2: unknown key \"Spce\""),
            ("jump Z\n", "line 1: expected"),
            ("jump =\n", "line 1: action \"jump\" has no keys"),
            ("two words = Z\n", "line 1: bad action name"),
        ] {
            let err = i.load_text(text).unwrap_err();
            assert!(err.starts_with(expect), "{err}");
            assert_eq!(i.to_text(), before);
        }
    }
}
