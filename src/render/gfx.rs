use super::{DrawCmd, Paint, font};
use std::sync::Arc;

#[derive(Clone, Copy)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn penetration(&self, o: &Rect) -> Option<(f32, f32)> {
        let ox = (self.x + self.w).min(o.x + o.w) - self.x.max(o.x);
        let oy = (self.y + self.h).min(o.y + o.h) - self.y.max(o.y);
        (ox > 0.0 && oy > 0.0).then_some((ox, oy))
    }

    pub fn overlaps(&self, o: &Rect) -> bool {
        self.penetration(o).is_some()
    }
}

pub struct Gfx {
    pub w: usize,
    pub h: usize,
    pub debug: bool,
    pub cam: (f32, f32),
    cmds: Vec<DrawCmd>,
}

impl Gfx {
    pub(crate) fn new(w: usize, h: usize) -> Self {
        Gfx { w, h, debug: false, cam: (0.0, 0.0), cmds: Vec::new() }
    }

    pub(crate) fn cmds(&self) -> &[DrawCmd] {
        &self.cmds
    }

    pub(crate) fn reset(&mut self) {
        self.cmds.clear();
    }

    fn push(&mut self, rect: Rect, paint: Paint) {
        self.cmds.push(DrawCmd { layer: 0, rect, paint, debug_only: false });
    }

    pub fn view(&self) -> Rect {
        Rect { x: self.cam.0, y: self.cam.1, w: self.w as f32, h: self.h as f32 }
    }

    pub fn clear(&mut self, c: u32) {
        self.push(Rect { x: 0.0, y: 0.0, w: self.w as f32, h: self.h as f32 }, Paint::Clear(c));
    }

    pub fn fill(&mut self, r: &Rect, c: u32) {
        self.push(*r, Paint::Color(c));
    }

    pub fn fill_alpha(&mut self, r: &Rect, c: u32, alpha: u8) {
        self.push(*r, Paint::Alpha(c, alpha));
    }

    pub fn outline(&mut self, r: &Rect, c: u32) {
        self.push(*r, Paint::Outline(c));
    }

    pub fn blit(&mut self, s: &Arc<Sprite>, x: i32, y: i32) {
        self.push(Rect { x: x as f32, y: y as f32, w: s.w as f32, h: s.h as f32 }, Paint::Sprite(s.clone()));
    }

    pub fn blit_region(&mut self, s: &Arc<Sprite>, src: Rect, x: i32, y: i32, flip: bool) {
        let paint = Paint::Region { sheet: s.clone(), src, flip };
        self.push(Rect { x: x as f32, y: y as f32, w: src.w, h: src.h }, paint);
    }

    pub fn draw(&mut self, cmds: impl IntoIterator<Item = DrawCmd>) {
        for mut c in cmds {
            if c.debug_only && !self.debug {
                continue;
            }
            c.rect.x -= self.cam.0;
            c.rect.y -= self.cam.1;
            self.cmds.push(c);
        }
    }

    pub fn text(&mut self, s: &str, x: i32, y: i32, scale: i32, c: u32) {
        let (scale, size) = (scale.max(1) as u32, 8.0 * scale.max(1) as f32);
        for (i, ch) in s.chars().enumerate() {
            let code = u32::from(ch);
            if code >= font::GLYPHS as u32 || font::bits(code as u8) == [0; 8] {
                continue;
            }
            let rect = Rect { x: x as f32 + i as f32 * size, y: y as f32, w: size, h: size };
            self.push(rect, Paint::Glyph { ch: code as u8, color: c, scale });
        }
    }
}

pub struct Sprite {
    pub w: usize,
    pub h: usize,
    pub(crate) px: Vec<u32>,
}

impl Sprite {
    pub fn load(path: &str) -> Self {
        let img = image::open(path).unwrap_or_else(|e| panic!("{path}: {e}")).to_rgba8();
        let px = img.pixels().map(|p| u32::from_be_bytes([p[3], p[0], p[1], p[2]])).collect();
        Sprite { w: img.width() as usize, h: img.height() as usize, px }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draw_applies_the_camera_and_skips_debug_commands_unless_enabled() {
        let cmd = |debug_only| DrawCmd {
            layer: 0,
            rect: Rect { x: 5.0, y: 2.0, w: 1.0, h: 1.0 },
            paint: Paint::Color(7),
            debug_only,
        };
        let mut g = Gfx::new(8, 8);
        g.cam = (3.0, 1.0);
        g.draw([cmd(false), cmd(true)]);
        assert_eq!(g.cmds().len(), 1);
        assert_eq!((g.cmds()[0].rect.x, g.cmds()[0].rect.y), (2.0, 1.0));
        g.debug = true;
        g.draw([cmd(true)]);
        assert_eq!(g.cmds().len(), 2);
    }

    #[test]
    fn text_records_one_command_per_visible_ascii_char_at_the_right_offset() {
        let mut g = Gfx::new(64, 16);
        g.text("A B\u{ac00}", 4, 2, 2, 5);
        let at: Vec<f32> = g.cmds().iter().map(|c| c.rect.x).collect();
        assert_eq!(at, [4.0, 4.0 + 2.0 * 16.0]);
        assert!(matches!(g.cmds()[0].paint, Paint::Glyph { ch: b'A', color: 5, scale: 2 }));
        assert_eq!(g.cmds()[0].rect.w, 16.0);
    }
}
