// A module reaches only modules in lower layers. The check is textual: it misses paths built by a macro or renamed
// with `use ... as`, and it ignores test code, which may cross layers.
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const LAYERS: [(&str, u8); 11] = [
    ("base", 0),
    ("render", 1),
    ("audio", 1),
    ("level", 2),
    ("assets", 2),
    ("input", 2),
    ("ecs", 3),
    ("physics", 4),
    ("app", 5),
    ("ui", 6),
    ("debug", 7),
];

fn idents(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !(c.is_alphanumeric() || c == '_')).filter(|w| !w.is_empty())
}

fn layer(module: &str) -> Option<u8> {
    LAYERS.iter().find(|(name, _)| *name == module).map(|&(_, n)| n)
}

// Which top-level module each name in the crate root's `pub use` lines comes from.
fn exports(lib: &str) -> HashMap<&str, &str> {
    let mut map = HashMap::new();
    for statement in lib.split("pub use ").skip(1) {
        let statement = statement.split(';').next().unwrap_or("");
        let mut names = idents(statement);
        if let Some(module) = names.next() {
            map.extend(names.map(|n| (n, module)));
        }
    }
    map
}

// The code a file needs to run: no comments, and nothing from a `mod tests` on.
fn code(text: &str) -> String {
    let mut out = String::new();
    let mut lines = text.lines().peekable();
    while let Some(line) = lines.next() {
        if line == "#[cfg(test)]" {
            match lines.peek() {
                Some(next) if next.starts_with("mod tests {") => break,
                Some(_) => {
                    lines.next();
                    continue;
                }
                None => break,
            }
        }
        if !line.trim_start().starts_with("//") {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

// Every `crate::` path in the code; a `crate::{ ... }` list gives all its names.
fn references(code: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut rest = code;
    while let Some(at) = rest.find("crate::") {
        rest = &rest[at + "crate::".len()..];
        if rest.starts_with('{') {
            found.extend(idents(rest.split(';').next().unwrap_or("")));
        } else {
            found.extend(idents(rest).next());
        }
    }
    found
}

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            if path.file_name().unwrap() != "tests" {
                sources(&path, out);
            }
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn a_module_reaches_only_modules_below_it() {
    let src = Path::new("src");
    let lib = std::fs::read_to_string(src.join("lib.rs")).unwrap();
    let exports = exports(&lib);
    let mut files = Vec::new();
    sources(src, &mut files);
    let mut checked = 0;
    let mut wrong = Vec::new();
    for path in files {
        let rel = path.strip_prefix(src).unwrap();
        let module = rel.iter().next().unwrap().to_str().unwrap().trim_end_matches(".rs").to_string();
        let Some(mine) = layer(&module) else { continue };
        if path.file_name().is_some_and(|n| n == "tests.rs") {
            continue;
        }
        checked += 1;
        let text = std::fs::read_to_string(&path).unwrap();
        let code = code(&text);
        for name in references(&code) {
            let target = if layer(name).is_some() { Some(name) } else { exports.get(name).copied() };
            let Some(target) = target else { continue };
            if target != module && layer(target).is_some_and(|theirs| theirs >= mine) {
                wrong.push(format!("{} reaches {target} through `{name}`", path.display()));
            }
        }
    }
    assert!(checked > 40, "only {checked} files were looked at");
    assert!(wrong.is_empty(), "layer rule broken (see CLAUDE.md):\n{}", wrong.join("\n"));
}
