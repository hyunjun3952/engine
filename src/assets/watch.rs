use crate::base::fx::{FxMap, FxSet};
use std::{fs, time::SystemTime};

#[derive(Default)]
pub(crate) struct Watcher {
    seen: FxMap<String, Option<(SystemTime, u64)>>,
    dirty: FxSet<String>,
}

// The length too: a coarse clock (FAT, some network drives) can give a half-written file and the whole one the same time.
fn modified(path: &str) -> Option<(SystemTime, u64)> {
    fs::metadata(path).and_then(|m| Ok((m.modified()?, m.len()))).ok()
}

impl Watcher {
    pub(crate) fn watch(&mut self, path: &str) {
        self.seen.entry(path.to_string()).or_insert_with(|| modified(path));
    }

    pub(crate) fn poll(&mut self) {
        let Watcher { seen, dirty } = self;
        for (path, last) in seen.iter_mut() {
            let now = modified(path);
            if now != *last {
                *last = now;
                dirty.insert(path.clone());
            }
        }
    }

    pub(crate) fn take(&mut self, path: &str) -> bool {
        self.dirty.remove(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs::File, time::Duration};

    fn touch(path: &str, secs: u64) {
        let file = File::options().write(true).open(path).unwrap();
        file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(secs)).unwrap();
    }

    #[test]
    fn a_change_is_reported_once_and_a_file_appearing_later_counts() {
        let dir = std::env::temp_dir().join(format!("engine-watch-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.txt");
        let path = path.to_str().unwrap();
        fs::write(path, "x").unwrap();
        touch(path, 1000);

        let mut w = Watcher::default();
        w.watch(path);
        w.poll();
        assert!(!w.take(path), "nothing changed since watching started");

        touch(path, 2000);
        w.poll();
        assert!(w.take(path));
        assert!(!w.take(path), "a change is consumed once");
        w.poll();
        assert!(!w.take(path));

        fs::write(path, "xy").unwrap();
        touch(path, 2000);
        w.poll();
        assert!(w.take(path), "a rewrite within the same clock tick shows in the length");

        let late = dir.join("later.txt");
        let late = late.to_str().unwrap();
        w.watch(late);
        w.poll();
        assert!(!w.take(late));
        fs::write(late, "y").unwrap();
        w.poll();
        assert!(w.take(late));
        fs::remove_dir_all(&dir).unwrap();
    }
}
