mod atlas;
mod audio;
mod watch;

pub use atlas::Atlas;
pub use audio::{Audio, Bus, Sound};

use crate::ecs::fx::FxMap;
use crate::{Font, Sprite};
use std::sync::Arc;
pub(crate) use watch::Watcher;

/// The text of a file, with the path in the error.
pub(crate) fn read_text(path: &str) -> Result<String, String> {
    std::fs::read_to_string(path).map(strip_bom).map_err(|e| format!("{path}: {e}"))
}

fn strip_bom(text: String) -> String {
    text.strip_prefix('\u{feff}').map(str::to_string).unwrap_or(text)
}

/// Writes beside the target, flushes to disk and renames over it, so a crash leaves the old file or the whole new
/// one, never half of it (and never an empty one: a rename alone can reach the disk before the data does).
pub(crate) fn write_atomic(path: &str, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    let tmp = format!("{path}.tmp");
    let mut file = std::fs::File::create(&tmp)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    std::fs::rename(tmp, path)
}

/// Like `read_text`, but a file that does not exist is `Ok(None)`.
pub(crate) fn read_text_if_exists(path: &str) -> Result<Option<String>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(strip_bom(text))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{path}: {e}")),
    }
}

#[derive(Default)]
pub struct Assets {
    sprites: FxMap<String, Arc<Sprite>>,
    fonts: FxMap<String, Arc<Font>>,
    pub(crate) watcher: Watcher,
}

impl Assets {
    pub fn sprite(&mut self, path: &str) -> Arc<Sprite> {
        if let Some(s) = self.sprites.get(path) {
            return s.clone();
        }
        self.watcher.watch(path);
        self.sprites.entry(path.to_string()).or_insert_with(|| Arc::new(Sprite::load(path))).clone()
    }

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
    fn an_atomic_write_replaces_the_file_whole_and_leaves_no_temp_file() {
        let dir = std::env::temp_dir().join(format!("engine-atomic-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("save.txt");
        let path = path.to_str().unwrap();
        write_atomic(path, "first\n").unwrap();
        write_atomic(path, "second\n").unwrap();
        assert_eq!(std::fs::read_to_string(path).unwrap(), "second\n");
        assert!(!std::path::Path::new(&format!("{path}.tmp")).exists());
        assert!(
            write_atomic(dir.join("missing").join("x.txt").to_str().unwrap(), "x").is_err(),
            "no directory is an error"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn same_path_loads_once() {
        let mut a = Assets::default();
        assert!(Arc::ptr_eq(&a.sprite("assets/images/ball.png"), &a.sprite("assets/images/ball.png")));
    }

    #[test]
    fn a_byte_order_mark_is_not_part_of_the_text() {
        let path = std::env::temp_dir().join(format!("engine-bom-{}.txt", std::process::id()));
        let path = path.to_str().unwrap();
        std::fs::write(path, "\u{feff}a = 1\n").unwrap();
        assert_eq!(read_text(path).unwrap(), "a = 1\n");
        assert_eq!(read_text_if_exists(path).unwrap().unwrap(), "a = 1\n");
        std::fs::remove_file(path).unwrap();
    }
}
