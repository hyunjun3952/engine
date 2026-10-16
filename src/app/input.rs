use super::pad::{PADS, Pad, PadAxis, PadState};
use crate::Vec2;
use crate::ecs::fx::{FxMap, FxSet};
use crate::render::letterbox;
pub use minifb::{Key, MouseButton};
use std::sync::{Arc, Mutex};

/// Keys, mouse and pad as of this fixed step, read by action name.
///
/// Bind actions once with `ctx.bind("jump", &[Key::Space])` (and `bind_pad`), then read `down`, `pressed` or `axis`;
/// asking for an action that was never bound panics. `ctx.load_bindings(path)` lets the player rebind them in a text
/// file of `action = Key Key pad.A` lines. A tap shorter than a step still counts.
///
/// ```
/// use engine::Input;
///
/// fn read(input: &Input) -> f32 {
///     if input.pressed("jump") {
///         return -1.0;
///     }
///     input.axis("left", "right")
/// }
/// ```
#[derive(Default)]
pub struct Input {
    down: FxSet<Key>,
    prev: FxSet<Key>,
    actions: FxMap<String, Vec<Key>>,
    pad: PadState,
    pad_down: FxSet<Pad>,
    pad_prev: FxSet<Pad>,
    pad_actions: FxMap<String, Vec<Pad>>,
    mouse: Option<(f32, f32)>,
    mouse_prev: Option<(f32, f32)>,
    buttons: [bool; 5],
    buttons_prev: [bool; 5],
    // What was seen held at any poll since the last step, so a tap that ends between two steps still counts.
    latched: FxSet<Key>,
    latched_pad: FxSet<Pad>,
    latched_buttons: [bool; 5],
    wheel: f32,
    // Steps in a row each held key or button had been down before this step, for `repeated`.
    held: FxMap<Key, u32>,
    pad_held: FxMap<Pad, u32>,
    step: f32,
    typed: Vec<char>,
    typing: Arc<Mutex<Vec<char>>>,
}

/// A window pixel as a canvas pixel, or `None` over the black bars around a letterboxed picture.
fn canvas_pos((x, y): (f32, f32), win: (f32, f32), canvas: (f32, f32)) -> Option<(f32, f32)> {
    let view = letterbox(win, canvas);
    let k = view.w / canvas.0;
    view.contains((x, y)).then(|| ((x - view.x) / k, (y - view.y) / k))
}

struct Typing(Arc<Mutex<Vec<char>>>);

impl minifb::InputCallback for Typing {
    fn add_char(&mut self, c: u32) {
        if let (Some(c), Ok(mut chars)) = (char::from_u32(c), self.0.lock()) {
            chars.push(c);
        }
    }
}

// How long a key is held before it repeats and how often it repeats then, as keyboards do for typing.
const REPEAT_DELAY: f32 = 0.4;
const REPEAT_EVERY: f32 = 1.0 / 30.0;

fn count_held<T: Copy + Eq + std::hash::Hash>(down: &FxSet<T>, before: &FxMap<T, u32>) -> FxMap<T, u32> {
    down.iter().map(|k| (*k, before.get(k).map_or(1, |n| n + 1))).collect()
}

const MOUSE: [MouseButton; 5] =
    [MouseButton::Left, MouseButton::Right, MouseButton::Middle, MouseButton::Back, MouseButton::Forward];

fn slot(b: MouseButton) -> usize {
    MOUSE.iter().position(|&m| m == b).unwrap()
}

impl Input {
    pub(crate) fn listen(&self, win: &mut minifb::Window) {
        win.set_input_callback(Box::new(Typing(self.typing.clone())));
    }

    pub(crate) fn poll(&mut self, win: &minifb::Window, canvas: (usize, usize)) {
        let (keys, pads) = (win.get_keys().into_iter().collect(), self.pad.poll());
        let buttons = MOUSE.map(|b| win.get_mouse_down(b));
        self.observe(keys, pads, buttons);
        let (ww, wh) = win.get_size();
        let (win_size, canvas) = ((ww as f32, wh as f32), (canvas.0 as f32, canvas.1 as f32));
        let at = win.get_unscaled_mouse_pos(minifb::MouseMode::Discard);
        self.mouse = at.and_then(|p| canvas_pos(p, win_size, canvas));
        self.wheel += win.get_scroll_wheel().map_or(0.0, |(_, y)| y);
        if let Ok(mut chars) = self.typing.lock() {
            self.typed.extend(chars.drain(..).filter(|c| !c.is_control()));
        }
    }
    // The step sees everything held since the last one, not just what is held at the final poll.
    fn observe(&mut self, keys: FxSet<Key>, pads: FxSet<Pad>, buttons: [bool; 5]) {
        self.latched.extend(keys);
        self.latched_pad.extend(pads);
        self.latched_buttons = std::array::from_fn(|i| self.latched_buttons[i] || buttons[i]);
        (self.down, self.pad_down, self.buttons) =
            (self.latched.clone(), self.latched_pad.clone(), self.latched_buttons);
    }

    #[cfg(test)]
    pub(crate) fn hold(&mut self, keys: &[Key]) {
        self.down = keys.iter().copied().collect();
    }
    #[cfg(test)]
    pub(crate) fn point(&mut self, at: Option<(f32, f32)>, left: bool) {
        self.mouse = at;
        self.buttons[0] = left;
    }
    pub(crate) fn snapshot(&self) -> super::replay::Frame {
        super::replay::Frame {
            keys: self.down.iter().copied().collect(),
            pads: self.pad_down.iter().copied().collect(),
            mouse: self.mouse,
            buttons: self.buttons,
        }
    }

    pub(crate) fn restore(&mut self, f: &super::replay::Frame) {
        self.down = f.keys.iter().copied().collect();
        self.pad_down = f.pads.iter().copied().collect();
        self.mouse = f.mouse;
        self.buttons = f.buttons;
    }

    pub(crate) fn end_step(&mut self) {
        (self.held, self.pad_held) = (count_held(&self.down, &self.held), count_held(&self.pad_down, &self.pad_held));
        self.prev = self.down.clone();
        self.pad_prev = self.pad_down.clone();
        self.buttons_prev = self.buttons;
        (self.latched, self.latched_pad, self.latched_buttons) = Default::default();
        self.mouse_prev = self.mouse;
        self.wheel = 0.0;
        self.typed.clear();
    }
    // The length of the fixed step, so repeats come at the same rate whatever `tick_hz` is.
    pub(crate) fn set_step(&mut self, dt: f32) {
        self.step = dt;
    }

    // True on the step a key goes down, then after `REPEAT_DELAY` once every `REPEAT_EVERY` while it stays down.
    fn repeats(&self, steps: Option<&u32>) -> bool {
        let dt = if self.step > 0.0 { self.step } else { 1.0 / 60.0 };
        let (delay, every) = (((REPEAT_DELAY / dt).round() as u32).max(1), ((REPEAT_EVERY / dt).round() as u32).max(1));
        steps.is_none_or(|&n| n >= delay && (n - delay) % every == 0)
    }

    pub(crate) fn bind(&mut self, action: &str, keys: &[Key]) {
        self.actions.insert(action.to_string(), keys.to_vec());
    }

    pub(crate) fn bind_pad(&mut self, action: &str, pads: &[Pad]) {
        self.actions.entry(action.to_string()).or_default();
        self.pad_actions.insert(action.to_string(), pads.to_vec());
    }

    #[cfg(test)]
    pub(crate) fn load_text(&mut self, text: &str) -> Result<(), String> {
        for (action, ..) in parse(text)? {
            self.actions.entry(action).or_default();
        }
        self.load_over(text, &[])
    }

    /// Puts `base` back first, so an action whose line left the file gets its default again; nothing changes if the
    /// text does not parse.
    pub(crate) fn load_over(&mut self, text: &str, base: &[Binding]) -> Result<(), String> {
        let parsed = parse(text)?;
        if let Some((action, ..)) = parsed.iter().find(|(a, ..)| !self.actions.contains_key(a)) {
            return Err(format!("unknown action {action:?}"));
        }
        for (action, keys, pads) in base.iter().chain(&parsed) {
            self.bind(action, keys);
            self.bind_pad(action, pads);
        }
        Ok(())
    }

    pub(crate) fn bindings(&self) -> Vec<Binding> {
        self.actions().into_iter().map(|a| (a.to_string(), self.keys(a).to_vec(), self.pads(a).to_vec())).collect()
    }

    pub(crate) fn to_text(&self) -> String {
        self.actions().iter().map(|a| format!("{a} = {}\n", self.describe(a))).collect()
    }

    /// Every bound action, sorted by name.
    pub fn actions(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.actions.keys().map(String::as_str).collect();
        names.sort();
        names
    }

    /// The keys and pad buttons of `action` as the bindings file spells them, like `Space Up pad.A`.
    pub fn describe(&self, action: &str) -> String {
        let pads = self.pads(action).iter().map(|p| format!("pad.{p:?}"));
        let words: Vec<String> = self.keys(action).iter().map(|k| format!("{k:?}")).chain(pads).collect();
        words.join(" ")
    }

    fn keys(&self, action: &str) -> &[Key] {
        self.actions.get(action).unwrap_or_else(|| panic!("unknown input action {action:?}"))
    }

    fn pads(&self, action: &str) -> &[Pad] {
        self.pad_actions.get(action).map_or(&[], Vec::as_slice)
    }

    fn held(&self, action: &str, keys: &FxSet<Key>, pads: &FxSet<Pad>) -> bool {
        self.keys(action).iter().any(|k| keys.contains(k)) || self.pads(action).iter().any(|p| pads.contains(p))
    }

    /// True while the action is held.
    pub fn down(&self, action: &str) -> bool {
        self.held(action, &self.down, &self.pad_down)
    }

    /// True on the step the action goes down.
    pub fn pressed(&self, action: &str) -> bool {
        self.down(action) && !self.held(action, &self.prev, &self.pad_prev)
    }

    /// True on the step the action stops being held.
    pub fn released(&self, action: &str) -> bool {
        !self.down(action) && self.held(action, &self.prev, &self.pad_prev)
    }

    /// Like `pressed`, then again and again while it stays held (after 0.4 s, 30 times a second), as typing repeats.
    pub fn repeated(&self, action: &str) -> bool {
        self.keys(action).iter().any(|&k| self.key_repeated(k))
            || self.pads(action).iter().any(|p| self.pad_down.contains(p) && self.repeats(self.pad_held.get(p)))
    }
    /// `1` while `pos` is held, `-1` while `neg` is, `0` for both or neither.
    pub fn axis(&self, neg: &str, pos: &str) -> f32 {
        self.down(pos) as i32 as f32 - self.down(neg) as i32 as f32
    }
    /// Cursor in screen pixels of the logical canvas; `None` while it is outside the window.
    pub fn mouse(&self) -> Option<Vec2> {
        self.mouse.map(Vec2::from)
    }
    /// True when the cursor is somewhere else than it was at the previous step.
    pub fn mouse_moved(&self) -> bool {
        self.mouse != self.mouse_prev
    }
    /// True while the mouse button is held.
    pub fn mouse_down(&self, b: MouseButton) -> bool {
        self.buttons[slot(b)]
    }
    /// True on the step the mouse button goes down.
    pub fn mouse_pressed(&self, b: MouseButton) -> bool {
        self.buttons[slot(b)] && !self.buttons_prev[slot(b)]
    }
    /// True on the step the mouse button comes up.
    pub fn mouse_released(&self, b: MouseButton) -> bool {
        !self.buttons[slot(b)] && self.buttons_prev[slot(b)]
    }
    /// Wheel movement since the last tick, positive away from the user.
    pub fn wheel(&self) -> f32 {
        self.wheel
    }
    /// Analog stick (-1..=1, dead zone removed) or trigger (0..=1) of the pad; 0 with no pad plugged in.
    pub fn pad_axis(&self, axis: PadAxis) -> f32 {
        self.pad.axis(axis)
    }
    /// Printable characters typed since the last tick, as the window system delivers them.
    pub fn typed(&self) -> &[char] {
        &self.typed
    }
    /// True on the step the raw key goes down; prefer actions.
    pub fn key_pressed(&self, key: Key) -> bool {
        self.down.contains(&key) && !self.prev.contains(&key)
    }
    #[cfg(test)]
    pub(crate) fn scroll(&mut self, amount: f32) {
        self.wheel = amount;
    }
    #[cfg(test)]
    pub(crate) fn type_chars(&mut self, s: &str) {
        self.typed = s.chars().collect();
    }
    /// A key that went down this step, for screens that wait for the player to press one.
    pub fn any_key_pressed(&self) -> Option<Key> {
        // Only keys `input.txt` can name, or a rebind would save a file that no longer loads.
        let named = |k: &&Key| KEYS.iter().any(|(_, known)| known == *k);
        self.down.iter().filter(|k| !self.prev.contains(k)).filter(named).min_by_key(|k| **k as usize).copied()
    }

    /// True while the raw key is held; prefer actions.
    pub fn key_down(&self, key: Key) -> bool {
        self.down.contains(&key)
    }

    /// `repeated` for one key.
    pub fn key_repeated(&self, key: Key) -> bool {
        self.down.contains(&key) && self.repeats(self.held.get(&key))
    }
}

macro_rules! key_names {
    ($($k:ident),* $(,)?) => {
        pub(super) const KEYS: &[(&str, Key)] = &[$((stringify!($k), Key::$k)),*];
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

pub(crate) type Binding = (String, Vec<Key>, Vec<Pad>);
type Parsed = Vec<Binding>;

fn parse(text: &str) -> Result<Parsed, String> {
    let mut out = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let n = i + 1;
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let (name, words) = line.split_once('=').ok_or(format!("line {n}: expected `action = keys`"))?;
        let name = name.trim();
        if name.is_empty() || name.contains(char::is_whitespace) {
            return Err(format!("line {n}: bad action name {name:?}"));
        }
        let (mut keys, mut pads) = (Vec::new(), Vec::new());
        for word in words.split_whitespace() {
            let pad = word.get(..4).filter(|p| p.eq_ignore_ascii_case("pad.")).map(|_| &word[4..]);
            if let Some(pad) = pad {
                let found = PADS.iter().find(|(known, _)| known.eq_ignore_ascii_case(pad));
                pads.push(found.map(|&(_, p)| p).ok_or(format!("line {n}: unknown pad button {word:?}"))?);
            } else {
                let found = KEYS.iter().find(|(known, _)| known.eq_ignore_ascii_case(word));
                keys.push(found.map(|&(_, key)| key).ok_or(format!("line {n}: unknown key {word:?}"))?);
            }
        }
        if keys.is_empty() && pads.is_empty() {
            return Err(format!("line {n}: action {name:?} has no keys"));
        }
        out.push((name.to_string(), keys, pads));
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
    fn a_held_key_repeats_after_a_delay_and_letting_go_is_released_once() {
        let mut i = input_with(&[]);
        i.set_step(0.1);
        let mut seen = Vec::new();
        for _ in 0..8 {
            i.hold(&[Key::Space]);
            seen.push(i.repeated("jump"));
            i.end_step();
        }
        // pressed at step 0; a 0.4 s delay is 4 steps, then every step (1/30 s rounds to less than one).
        assert_eq!(seen, [true, false, false, false, true, true, true, true]);
        assert!(!i.released("jump"));
        i.hold(&[]);
        assert!(i.released("jump") && !i.repeated("jump"));
        i.end_step();
        assert!(!i.released("jump"));
        i.hold(&[Key::Space]);
        assert!(i.key_repeated(Key::Space), "pressed again starts over");
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
    fn a_tap_that_ends_before_the_step_still_counts_once() {
        let mut i = input_with(&[]);
        i.observe(FxSet::from_iter([Key::Space]), FxSet::default(), [true, false, false, false, false]);
        i.observe(FxSet::default(), FxSet::default(), [false; 5]);
        assert!(i.pressed("jump") && i.mouse_pressed(MouseButton::Left));
        i.end_step();
        i.observe(FxSet::default(), FxSet::default(), [false; 5]);
        assert!(!i.down("jump") && !i.pressed("jump") && !i.mouse_down(MouseButton::Left));
        assert!(i.mouse_released(MouseButton::Left));
    }

    #[test]
    fn a_reloaded_file_puts_back_the_default_of_an_action_it_no_longer_names() {
        let (mut i, base) = (input_with(&[]), input_with(&[]).bindings());
        i.load_over("jump = Z\nleft = A\n", &base).unwrap();
        i.load_over("jump = Z\n", &base).unwrap();
        assert_eq!((i.describe("jump"), i.describe("left")), ("Z".into(), "Left A".into()));
        assert!(i.load_over("jump = nonsense\n", &base).is_err());
        assert_eq!(i.describe("jump"), "Z", "a file that does not parse changes nothing");
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
    fn mouse_buttons_report_edges_between_steps() {
        let mut i = Input::default();
        i.point(Some((3.0, 4.0)), true);
        assert!(i.mouse_pressed(MouseButton::Left) && i.mouse_down(MouseButton::Left));
        assert!(!i.mouse_down(MouseButton::Right));
        i.end_step();
        assert!(!i.mouse_pressed(MouseButton::Left) && i.mouse_down(MouseButton::Left));
        i.point(None, false);
        assert!(i.mouse_released(MouseButton::Left) && i.mouse().is_none());
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

    #[test]
    fn pad_buttons_work_beside_keys_and_roundtrip_through_text() {
        let mut i = Input::default();
        i.load_text("jump = Space pad.a\nfire = pad.RB\n").unwrap();
        assert_eq!(i.to_text(), "fire = pad.RB\njump = Space pad.A\n");
        assert!(!i.down("fire"));
        i.pad_down = [Pad::RB].into_iter().collect();
        assert!(i.down("fire") && i.pressed("fire") && !i.down("jump"));
        i.end_step();
        assert!(i.down("fire") && !i.pressed("fire"));
        i.bind("jump", &[Key::X]);
        assert_eq!(i.to_text(), "fire = pad.RB\njump = X pad.A\n", "bind replaces keys, not pad buttons");
        assert!(i.load_text("jump = pad.Z\n").unwrap_err().starts_with("line 1: unknown pad button"));
    }

    #[test]
    fn the_mouse_maps_through_the_letterboxed_picture_and_is_none_over_the_bars() {
        let canvas = (640.0, 360.0);
        assert_eq!(canvas_pos((100.0, 50.0), canvas, canvas), Some((100.0, 50.0)));
        assert_eq!(canvas_pos((200.0, 100.0), (1280.0, 720.0), canvas), Some((100.0, 50.0)));
        // 1280x1000 fits the picture at 2x and leaves 140 px of bar above and below it.
        let wide = (1280.0, 1000.0);
        assert_eq!(canvas_pos((200.0, 140.0 + 100.0), wide, canvas), Some((100.0, 50.0)));
        assert_eq!(canvas_pos((200.0, 100.0), wide, canvas), None);
    }
}
