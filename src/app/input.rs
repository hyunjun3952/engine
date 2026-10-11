pub use minifb::Key;
use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub struct Input {
    down: HashSet<Key>,
    prev: HashSet<Key>,
    actions: HashMap<&'static str, Vec<Key>>,
}

impl Input {
    pub(crate) fn poll(&mut self, win: &minifb::Window) {
        self.down = win.get_keys().into_iter().collect();
    }
    pub(crate) fn end_step(&mut self) {
        self.prev = self.down.clone();
    }
    pub(crate) fn bind(&mut self, action: &'static str, keys: &[Key]) {
        self.actions.insert(action, keys.to_vec());
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
}
