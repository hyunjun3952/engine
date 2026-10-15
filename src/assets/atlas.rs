use crate::ecs::fx::FxMap;
use crate::{Gfx, Rect, Sprite};
use std::sync::Arc;

/// Named regions of one sprite sheet, so a game asks for `"hero_jump"` instead of repeating pixel coordinates.
///
/// The text form is one `name x y w h` per line (pixels in the sheet); `#` starts a comment and blank lines are
/// skipped. A name cannot repeat and a region cannot reach outside the sheet.
pub struct Atlas {
    sheet: Arc<Sprite>,
    regions: FxMap<String, Rect>,
}

impl Atlas {
    pub fn parse(text: &str, sheet: Arc<Sprite>) -> Result<Atlas, String> {
        let (limit, mut regions) = (sheet.size(), FxMap::default());
        for (n, line) in text.lines().enumerate() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let bad = |why: &str| format!("line {}: {why}", n + 1);
            let mut parts = line.split_whitespace();
            let name = parts.next().ok_or_else(|| bad("no name"))?;
            let nums = parts
                .map(|p| p.parse::<u32>().map_err(|e| bad(&format!("{p:?}: {e}"))))
                .collect::<Result<Vec<_>, _>>()?;
            let [x, y, w, h] = nums[..] else { return Err(bad("expected `name x y w h`")) };
            let r = Rect::new(x as f32, y as f32, w as f32, h as f32);
            if !sheet.image().holds(&r) {
                return Err(bad(&format!("{name} is empty or outside the {}x{} sheet", limit.0, limit.1)));
            }
            if regions.insert(name.to_string(), r).is_some() {
                return Err(bad(&format!("{name} appears twice")));
            }
        }
        Ok(Atlas { sheet, regions })
    }

    pub fn load(path: &str, sheet: Arc<Sprite>) -> Result<Atlas, String> {
        Atlas::parse(&super::read_text(path)?, sheet).map_err(|e| format!("{path}: {e}"))
    }

    pub fn sheet(&self) -> &Arc<Sprite> {
        &self.sheet
    }

    /// The region called `name`; panics on an unknown name, like a missing asset.
    pub fn get(&self, name: &str) -> Rect {
        *self.regions.get(name).unwrap_or_else(|| panic!("no region called {name:?} in the atlas"))
    }

    pub fn draw(&self, gfx: &mut Gfx, name: &str, x: i32, y: i32) {
        gfx.blit_region(&self.sheet, self.get(name), x, y, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sheet() -> Arc<Sprite> {
        Arc::new(Sprite::from_pixels(32, 16, vec![0; 512]))
    }

    #[test]
    fn regions_are_read_by_name_and_drawn_where_asked() {
        let atlas = Atlas::parse("# heroes\nidle 0 0 16 16\n\njump 16 0 16 8  # half height\n", sheet()).unwrap();
        assert_eq!(atlas.get("jump"), Rect::new(16.0, 0.0, 16.0, 8.0));
        let mut g = Gfx::new(64, 64);
        atlas.draw(&mut g, "idle", 5, 6);
        let c = &g.cmds()[0];
        assert_eq!((c.rect.x, c.rect.y, c.rect.w, c.rect.h), (5.0, 6.0, 16.0, 16.0));
    }

    #[test]
    fn bad_lines_say_which_line_and_why() {
        for (text, line, why) in [
            ("a 0 0 4", "line 1", "expected"),
            ("a 0 0 4 x", "line 1", "invalid digit"),
            ("a 0 0 4 4\nb 30 0 4 4", "line 2", "outside"),
            ("a 0 0 0 4", "line 1", "empty"),
            ("a 0 0 4 4\na 4 0 4 4", "line 2", "twice"),
        ] {
            let e = Atlas::parse(text, sheet()).err().unwrap_or_else(|| panic!("{text:?} was accepted"));
            assert!(e.contains(line) && e.contains(why), "{text:?}: {e}");
        }
    }

    #[test]
    #[should_panic(expected = "no region called")]
    fn an_unknown_name_panics_like_a_missing_asset() {
        Atlas::parse("a 0 0 4 4", sheet()).unwrap().get("b");
    }
}
