mod audio;
mod watch;

pub use audio::Audio;

use crate::Sprite;
use std::{collections::HashMap, sync::Arc};
pub(crate) use watch::Watcher;

#[derive(Default)]
pub struct Assets {
    sprites: HashMap<String, Arc<Sprite>>,
    pub(crate) watcher: Watcher,
}

impl Assets {
    pub fn sprite(&mut self, path: &str) -> Arc<Sprite> {
        self.watcher.watch(path);
        self.sprites.entry(path.to_string()).or_insert_with(|| Arc::new(Sprite::load(path))).clone()
    }

    pub(crate) fn reload_changed(&mut self) -> Vec<Result<String, String>> {
        let mut done = Vec::new();
        for (path, sprite) in &self.sprites {
            if self.watcher.take(path) {
                done.push(sprite.reload(path).map(|()| path.clone()));
            }
        }
        done
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_changed_image_file_replaces_the_pixels_everyone_already_holds() {
        use std::time::{Duration, SystemTime};
        let dir = std::env::temp_dir().join(format!("engine-assets-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("s.png");
        let path = path.to_str().unwrap();
        let bump = |secs| {
            let f = std::fs::File::options().write(true).open(path).unwrap();
            f.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(secs)).unwrap();
        };
        std::fs::copy("assets/ball.png", path).unwrap();
        bump(1000);

        let mut a = Assets::default();
        let held = a.sprite(path);
        let version = held.version();
        assert_eq!(held.size(), (16, 16));

        std::fs::copy("assets/tiles.png", path).unwrap();
        bump(2000);
        a.watcher.poll();
        assert_eq!(a.reload_changed().len(), 1);
        assert_eq!(held.size(), (32, 16));
        assert!(held.version() > version);

        std::fs::write(path, "not a png").unwrap();
        bump(3000);
        a.watcher.poll();
        let results = a.reload_changed();
        assert!(results[0].as_ref().unwrap_err().starts_with(path));
        assert_eq!(held.size(), (32, 16), "a broken file keeps the last good image");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn same_path_loads_once() {
        let mut a = Assets::default();
        assert!(Arc::ptr_eq(&a.sprite("assets/ball.png"), &a.sprite("assets/ball.png")));
    }
}
