use crate::base::in_file;
use std::collections::BTreeMap;
use std::str::FromStr;

/// A folder for the player's own files (saves, settings, `input.txt`), made if it is missing: `$XDG_DATA_HOME/<app>`
/// or `~/.local/share/<app>` on Linux, `%APPDATA%\<app>` on Windows, `~/Library/Application Support/<app>` on macOS.
/// Join file names onto it, as in `format!("{dir}/save.txt")`.
///
/// ```no_run
/// let dir = engine::data_dir("mygame")?;
/// let path = format!("{dir}/save.txt");
/// # let _ = path;
/// # Ok::<(), String>(())
/// ```
pub fn data_dir(app: &str) -> Result<String, String> {
    let dir = data_dir_in(app, std::env::consts::OS, |k| std::env::var(k).ok().filter(|v| !v.is_empty()))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("{dir}: {e}"))?;
    Ok(dir)
}

// Where `data_dir` points on `os`, with `env` standing in for the environment so tests need not change it.
fn data_dir_in(app: &str, os: &str, env: impl Fn(&str) -> Option<String>) -> Result<String, String> {
    if app.is_empty() || app.contains(['/', '\\', ':']) || app == "." || app == ".." {
        return Err(format!("{app:?} is not a folder name"));
    }
    let missing = |k: &str| format!("no {k} to find the data folder by");
    let base = match os {
        "windows" => env("APPDATA").ok_or_else(|| missing("APPDATA"))?,
        "macos" => format!("{}/Library/Application Support", env("HOME").ok_or_else(|| missing("HOME"))?),
        _ => match env("XDG_DATA_HOME").filter(|x| x.starts_with('/')) {
            Some(xdg) => xdg,
            None => format!("{}/.local/share", env("HOME").ok_or_else(|| missing("HOME"))?),
        },
    };
    let sep = if os == "windows" { '\\' } else { '/' };
    Ok(format!("{}{sep}{app}", base.trim_end_matches(['/', '\\'])))
}

/// A game's own `key = value` file: settings, progress, high scores. `load` treats a missing file as empty and
/// `save` writes atomically, so a crash cannot leave half a file.
///
/// ```
/// use engine::Save;
/// let mut save = Save::new();
/// save.set("coins", 3);
/// assert_eq!(save.get::<i32>("coins"), Some(3));
/// assert_eq!(save.get_or("lives", 5), 5);
/// save.remove("coins");
/// assert_eq!(save.get::<i32>("coins"), None);
/// ```
#[derive(Default, Debug, PartialEq)]
pub struct Save {
    map: BTreeMap<String, String>,
}

impl Save {
    /// An empty save.
    pub fn new() -> Self {
        Save::default()
    }

    /// A missing file is an empty save; a malformed one is an error so a hand-edited file is never silently dropped.
    pub fn load(path: &str) -> Result<Save, String> {
        match crate::base::read_text_if_exists(path)? {
            Some(text) => Save::parse(&text).map_err(in_file(path)),
            None => Ok(Save::new()),
        }
    }

    /// Writes beside the target and renames, so a crash mid-write cannot destroy the previous save.
    pub fn save(&self, path: &str) -> Result<(), String> {
        crate::base::write_atomic(path, &self.to_text())
    }

    /// Stores `value` under `key`; panics if the key holds `=` or `#`.
    pub fn set(&mut self, key: &str, value: impl ToString) {
        assert!(valid_key(key), "bad save key {key:?}");
        self.map.insert(key.to_string(), value.to_string());
    }

    /// The value under `key`, or `None` if it is missing or does not parse as `T`.
    pub fn get<T: FromStr>(&self, key: &str) -> Option<T> {
        self.map.get(key)?.parse().ok()
    }

    /// Like `get`, with `default` for a missing or unparsable value.
    pub fn get_or<T: FromStr>(&self, key: &str, default: T) -> T {
        self.get(key).unwrap_or(default)
    }

    /// Forgets `key`.
    pub fn remove(&mut self, key: &str) {
        self.map.remove(key);
    }

    fn to_text(&self) -> String {
        let mut out = String::new();
        for (k, v) in &self.map {
            out += &format!("{k} = {}\n", escape(v));
        }
        out
    }

    fn parse(text: &str) -> Result<Save, String> {
        let mut save = Save::new();
        for (i, line) in text.lines().enumerate() {
            if line.trim().is_empty() || line.trim_start().starts_with('#') {
                continue;
            }
            let n = i + 1;
            let (key, value) = line.split_once('=').ok_or(format!("line {n}: expected `key = value`"))?;
            let key = key.trim();
            if !valid_key(key) {
                return Err(format!("line {n}: bad key {key:?}"));
            }
            save.map.insert(key.to_string(), unescape(value.strip_prefix(' ').unwrap_or(value)));
        }
        Ok(save)
    }
}

fn valid_key(key: &str) -> bool {
    !key.is_empty() && !key.contains(['=', '#', '\n', '\r']) && key.trim() == key
}

pub(crate) fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\n', "\\n").replace('\r', "\\r")
}

pub(crate) fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match (c, chars.clone().next()) {
            ('\\', Some('n')) => {
                out.push('\n');
                chars.next();
            }
            ('\\', Some('r')) => {
                out.push('\r');
                chars.next();
            }
            ('\\', Some('\\')) => {
                out.push('\\');
                chars.next();
            }
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_roundtrip_through_text_including_awkward_strings() {
        let mut s = Save::new();
        s.set("score", 120);
        s.set("speed", 1.5);
        s.set("name", "한글 이름 = a # b\nsecond \\n line ");
        let back = Save::parse(&s.to_text()).unwrap();
        assert_eq!(back, s);
        assert_eq!(back.get::<u32>("score"), Some(120));
        assert_eq!(back.get::<f32>("speed"), Some(1.5));
        assert_eq!(back.get::<String>("name").as_deref(), Some("한글 이름 = a # b\nsecond \\n line "));
    }

    #[test]
    fn a_carriage_return_survives_and_a_key_with_a_line_break_is_refused() {
        let mut s = Save::new();
        s.set("k", "x\r\ny\r");
        assert_eq!(Save::parse(&s.to_text()).unwrap().get::<String>("k").as_deref(), Some("x\r\ny\r"));
        assert!(!valid_key("a\nb") && !valid_key("a\rb"));
    }

    #[test]
    fn missing_or_unparsable_values_fall_back() {
        let s = Save::parse("level = x\n").unwrap();
        assert_eq!(s.get_or("level", 3), 3);
        assert_eq!(s.get_or("nope", 7), 7);
    }

    #[test]
    fn a_bad_line_names_itself() {
        assert!(Save::parse("a = 1\njunk\n").unwrap_err().starts_with("line 2"));
    }

    #[test]
    fn save_replaces_the_file_and_a_missing_file_loads_empty() {
        let path = std::env::temp_dir().join(format!("engine-save-{}.txt", std::process::id()));
        let path = path.to_str().unwrap();
        let _ = std::fs::remove_file(path);
        assert_eq!(Save::load(path).unwrap(), Save::new());
        let mut s = Save::new();
        s.set("hi", 9);
        s.save(path).unwrap();
        s.set("hi", 10);
        s.save(path).unwrap();
        assert_eq!(Save::load(path).unwrap().get::<i32>("hi"), Some(10));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn the_data_folder_follows_each_system_and_refuses_a_path_for_a_name() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |k: &str| pairs.iter().find(|p| p.0 == k).map(|p| p.1.to_string())
        };
        let home = env(&[("HOME", "/home/a")]);
        assert_eq!(data_dir_in("g", "linux", home).unwrap(), "/home/a/.local/share/g");
        let relative = env(&[("HOME", "/home/a"), ("XDG_DATA_HOME", "data")]);
        assert_eq!(data_dir_in("g", "linux", relative).unwrap(), "/home/a/.local/share/g");
        let xdg = env(&[("HOME", "/home/a"), ("XDG_DATA_HOME", "/data/")]);
        assert_eq!(data_dir_in("g", "linux", xdg).unwrap(), "/data/g");
        assert_eq!(data_dir_in("g", "macos", home).unwrap(), "/home/a/Library/Application Support/g");
        let appdata = env(&[("APPDATA", "C:\\Users\\a\\AppData\\Roaming")]);
        assert_eq!(data_dir_in("g", "windows", appdata).unwrap(), "C:\\Users\\a\\AppData\\Roaming\\g");
        assert!(data_dir_in("g", "windows", home).unwrap_err().contains("APPDATA"));
        for bad in ["", "..", "a/b", "a\\b", "c:"] {
            assert!(data_dir_in(bad, "linux", home).is_err(), "{bad:?}");
        }
    }
}
