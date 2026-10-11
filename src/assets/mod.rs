mod audio;

pub use audio::Audio;

use crate::Sprite;
use std::{collections::HashMap, sync::Arc};

#[derive(Default)]
pub struct Assets {
    sprites: HashMap<String, Arc<Sprite>>,
}

impl Assets {
    pub fn sprite(&mut self, path: &str) -> Arc<Sprite> {
        self.sprites.entry(path.to_string()).or_insert_with(|| Arc::new(Sprite::load(path))).clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_path_loads_once() {
        let mut a = Assets::default();
        assert!(Arc::ptr_eq(&a.sprite("assets/ball.png"), &a.sprite("assets/ball.png")));
    }
}
