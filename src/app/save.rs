use std::collections::BTreeMap;
use std::str::FromStr;

#[derive(Default, Debug, PartialEq)]
pub struct Save {
    map: BTreeMap<String, String>,
}

impl Save {
    pub fn new() -> Self {
        Save::default()
    }

    /// A missing file is an empty save; a malformed one is an error so a hand-edited file is never silently dropped.
    pub fn load(path: &str) -> Result<Save, String> {
        match crate::assets::read_text_if_exists(path)? {
            Some(text) => Save::parse(&text).map_err(|e| format!("{path}: {e}")),
            None => Ok(Save::new()),
        }
    }

    /// Writes beside the target and renames, so a crash mid-write cannot destroy the previous save.
    pub fn save(&self, path: &str) -> std::io::Result<()> {
        crate::assets::write_atomic(path, &self.to_text())
    }

    pub fn set(&mut self, key: &str, value: impl ToString) {
        assert!(valid_key(key), "bad save key {key:?}");
        self.map.insert(key.to_string(), value.to_string());
    }

    pub fn get<T: FromStr>(&self, key: &str) -> Option<T> {
        self.map.get(key)?.parse().ok()
    }

    pub fn get_or<T: FromStr>(&self, key: &str, default: T) -> T {
        self.get(key).unwrap_or(default)
    }

    pub fn remove(&mut self, key: &str) {
        self.map.remove(key);
    }

    fn to_text(&self) -> String {
        let mut out = String::new();
        for (k, v) in &self.map {
            out += &format!("{k} = {}\n", v.replace('\\', "\\\\").replace('\n', "\\n").replace('\r', "\\r"));
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

fn unescape(s: &str) -> String {
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
}
