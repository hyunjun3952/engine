mod atlas;
mod watch;

pub use atlas::Atlas;

use crate::base::fx::FxMap;
use crate::{Font, Sprite};
use std::sync::Arc;
pub(crate) use watch::Watcher;

/// Sprites and fonts by path, loaded once and kept, reached through `ctx.assets()`.
///
/// A file that cannot be read panics with its path; load with [`Sprite::load`] or [`Font::load`] to handle the error
/// yourself. Sprites are reloaded in place when their file changes if `Config::hot_reload` is on.
///
/// ```
/// use engine::Ctx;
///
/// fn load(ctx: &mut Ctx) {
///     let ball = ctx.assets().sprite("assets/images/ball.png");
///     let font = ctx.assets().font("assets/fonts/galmuri9.bdf");
///     # let _ = (ball, font);
/// }
/// ```
#[derive(Default)]
pub struct Assets {
    sprites: FxMap<String, Arc<Sprite>>,
    fonts: FxMap<String, Arc<Font>>,
    pub(crate) watcher: Watcher,
}

impl Assets {
    /// Loads a sprite by path, or hands out the cached one; panics if it cannot be read.
    pub fn sprite(&mut self, path: &str) -> Arc<Sprite> {
        if let Some(s) = self.sprites.get(path) {
            return s.clone();
        }
        self.watcher.watch(path);
        self.sprites
            .entry(path.to_string())
            .or_insert_with(|| Arc::new(Sprite::load(path).unwrap_or_else(|e| panic!("{e}"))))
            .clone()
    }

    /// Loads a BDF font by path, or hands out the cached one; panics if it cannot be read.
    pub fn font(&mut self, path: &str) -> Arc<Font> {
        let load = || Arc::new(Font::load(path).unwrap_or_else(|e| panic!("{e}")));
        self.fonts.entry(path.to_string()).or_insert_with(load).clone()
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
        std::fs::copy("assets/images/ball.png", path).unwrap();
        bump(1000);

        let mut a = Assets::default();
        let held = a.sprite(path);
        let version = held.version();
        assert_eq!(held.size(), (16, 16));
        let sheet_size = Sprite::test_tiles().size();

        std::fs::copy("assets/images/tiles.png", path).unwrap();
        bump(2000);
        a.watcher.poll();
        assert_eq!(a.reload_changed().len(), 1);
        assert_eq!(held.size(), sheet_size);
        assert!(held.version() > version);

        std::fs::write(path, "not a png").unwrap();
        bump(3000);
        a.watcher.poll();
        let results = a.reload_changed();
        assert!(results[0].as_ref().unwrap_err().starts_with(path));
        assert_eq!(held.size(), sheet_size, "a broken file keeps the last good image");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn same_path_loads_once() {
        let mut a = Assets::default();
        assert!(Arc::ptr_eq(&a.sprite("assets/images/ball.png"), &a.sprite("assets/images/ball.png")));
    }
}
