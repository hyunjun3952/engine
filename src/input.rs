pub use minifb::Key;
use std::collections::HashSet;

#[derive(Default)]
pub struct Input {
    down: HashSet<Key>,
    prev: HashSet<Key>,
}

impl Input {
    pub(crate) fn poll(&mut self, win: &minifb::Window) {
        self.down = win.get_keys().into_iter().collect();
    }
    pub(crate) fn end_step(&mut self) {
        self.prev = self.down.clone();
    }

    pub fn down(&self, k: Key) -> bool {
        self.down.contains(&k)
    }
    pub fn pressed(&self, k: Key) -> bool {
        self.down.contains(&k) && !self.prev.contains(&k)
    }
    pub fn axis(&self, neg: Key, pos: Key) -> f32 {
        self.down(pos) as i32 as f32 - self.down(neg) as i32 as f32
    }
}
