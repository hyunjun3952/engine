/// For `map_err`: names `path` in an error.
pub(crate) fn in_file<E: std::fmt::Display>(path: &str) -> impl Fn(E) -> String + '_ {
    move |e| format!("{path}: {e}")
}

/// The text of a file, with the path in the error.
pub(crate) fn read_text(path: &str) -> Result<String, String> {
    std::fs::read_to_string(path).map(strip_bom).map_err(in_file(path))
}

fn strip_bom(text: String) -> String {
    text.strip_prefix('\u{feff}').map(str::to_string).unwrap_or(text)
}

/// Writes beside the target, flushes to disk and renames over it, so a crash leaves the old file or the whole new
/// one, never half of it (and never an empty one: a rename alone can reach the disk before the data does).
pub(crate) fn write_atomic(path: &str, text: &str) -> Result<(), String> {
    use std::io::Write;
    static COUNT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = format!("{path}.{}.{n}.tmp", std::process::id());
    let write = || {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&tmp, path)?;
        // The rename lives in the directory, which needs its own flush; Windows cannot open a directory, so unix only.
        #[cfg(unix)]
        if let Ok(dir) = std::fs::File::open(
            std::path::Path::new(path).parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(".".as_ref()),
        ) {
            let _ = dir.sync_all();
        }
        Ok(())
    };
    write().map_err(|e: std::io::Error| {
        let _ = std::fs::remove_file(&tmp);
        format!("{path}: {e}")
    })
}

/// Like `read_text`, but a file that does not exist is `Ok(None)`.
pub(crate) fn read_text_if_exists(path: &str) -> Result<Option<String>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(strip_bom(text))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{path}: {e}")),
    }
}

// A switch like `ENGINE_MUTE=1`: unset, empty and `0` are off.
pub(crate) fn env_flag(name: &str) -> bool {
    std::env::var(name).is_ok_and(|v| !v.is_empty() && v != "0")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_atomic_write_replaces_the_file_whole_and_leaves_no_temp_file() {
        let dir = std::env::temp_dir().join(format!("engine-atomic-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("save.txt");
        let path = path.to_str().unwrap();
        write_atomic(path, "first\n").unwrap();
        write_atomic(path, "second\n").unwrap();
        assert_eq!(std::fs::read_to_string(path).unwrap(), "second\n");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "no temp file is left beside the save");
        let missing = dir.join("missing").join("x.txt");
        let missing = missing.to_str().unwrap();
        let err = write_atomic(missing, "x").unwrap_err();
        assert!(err.starts_with(missing), "no directory is an error naming the file: {err}");
        std::fs::remove_dir_all(&dir).unwrap();
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
