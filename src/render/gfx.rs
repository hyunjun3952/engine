use super::{DrawCmd, Paint};
use font8x8::UnicodeFonts;

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
    pub(crate) buf: Vec<u32>,
}

impl Gfx {
    pub(crate) fn new(w: usize, h: usize) -> Self {
        Gfx { w, h, debug: false, cam: (0.0, 0.0), buf: vec![0; w * h] }
    }

    pub fn clear(&mut self, c: u32) {
        self.buf.fill(c);
    }

    pub fn fill(&mut self, r: &Rect, c: u32) {
        self.fill_with(r, |_| c);
    }

    pub fn fill_alpha(&mut self, r: &Rect, c: u32, alpha: u8) {
        self.fill_with(r, |d| blend(c, d, alpha as u32));
    }

    fn fill_with(&mut self, r: &Rect, f: impl Fn(u32) -> u32) {
        let (x0, y0) = (r.x.max(0.0) as usize, r.y.max(0.0) as usize);
        let (x1, y1) = (((r.x + r.w) as usize).min(self.w), ((r.y + r.h) as usize).min(self.h));
        for y in y0..y1 {
            for px in &mut self.buf[y * self.w + x0..y * self.w + x1.max(x0)] {
                *px = f(*px);
            }
        }
    }

    pub fn draw(&mut self, cmds: &[DrawCmd]) {
        let (cx, cy) = self.cam;
        for c in cmds {
            if c.debug_only && !self.debug {
                continue;
            }
            let r = Rect { x: c.rect.x - cx, y: c.rect.y - cy, ..c.rect };
            match &c.paint {
                Paint::Color(col) => self.fill(&r, *col),
                Paint::Alpha(col, a) => self.fill_alpha(&r, *col, *a),
                Paint::Sprite(s) => self.blit(s, r.x as i32, r.y as i32),
                Paint::Region { sheet, src, flip } => self.blit_region(sheet, *src, r.x as i32, r.y as i32, *flip),
                Paint::Outline(col) => self.outline(&r, *col),
            }
        }
    }

    pub fn outline(&mut self, r: &Rect, c: u32) {
        self.fill(&Rect { h: 1.0, ..*r }, c);
        self.fill(&Rect { y: r.y + r.h - 1.0, h: 1.0, ..*r }, c);
        self.fill(&Rect { w: 1.0, ..*r }, c);
        self.fill(&Rect { x: r.x + r.w - 1.0, w: 1.0, ..*r }, c);
    }

    pub fn blit(&mut self, s: &Sprite, x: i32, y: i32) {
        self.blit_region(s, Rect { x: 0.0, y: 0.0, w: s.w as f32, h: s.h as f32 }, x, y, false);
    }

    pub fn blit_region(&mut self, s: &Sprite, src: Rect, x: i32, y: i32, flip: bool) {
        let (sx0, sy0, w, h) = (src.x as usize, src.y as usize, src.w as usize, src.h as usize);
        for sy in 0..h {
            let dy = y + sy as i32;
            if dy < 0 || dy >= self.h as i32 {
                continue;
            }
            for sx in 0..w {
                let dx = x + sx as i32;
                if dx < 0 || dx >= self.w as i32 {
                    continue;
                }
                let p = s.px[(sy0 + sy) * s.w + sx0 + if flip { w - 1 - sx } else { sx }];
                let (a, d) = (p >> 24, &mut self.buf[dy as usize * self.w + dx as usize]);
                if a == 255 {
                    *d = p;
                } else if a > 0 {
                    *d = blend(p, *d, a);
                }
            }
        }
    }

    pub fn text(&mut self, s: &str, x: i32, y: i32, scale: i32, c: u32) {
        for (i, ch) in s.chars().enumerate() {
            let Some(g) = font8x8::BASIC_FONTS.get(ch) else { continue };
            for (row, bits) in g.iter().enumerate() {
                for col in 0..8 {
                    if bits >> col & 1 == 1 {
                        let px = x + (i as i32 * 8 + col) * scale;
                        let r =
                            Rect { x: px as f32, y: (y + row as i32 * scale) as f32, w: scale as f32, h: scale as f32 };
                        self.fill(&r, c);
                    }
                }
            }
        }
    }
}

fn blend(src: u32, dst: u32, a: u32) -> u32 {
    let mix = |sh: u32| ((((src >> sh) & 255) * a + ((dst >> sh) & 255) * (255 - a)) / 255) << sh;
    mix(16) | mix(8) | mix(0)
}

pub struct Sprite {
    pub w: usize,
    pub h: usize,
    px: Vec<u32>,
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
            rect: Rect { x: 5.0, y: 0.0, w: 1.0, h: 1.0 },
            paint: Paint::Color(7),
            debug_only,
        };
        let mut g = Gfx::new(8, 1);
        g.cam = (3.0, 0.0);
        g.draw(&[cmd(false), cmd(true)]);
        assert_eq!(g.buf.iter().filter(|&&p| p == 7).count(), 1);
        assert_eq!(g.buf[2], 7);
        g.buf.fill(0);
        g.debug = true;
        g.draw(&[cmd(true)]);
        assert_eq!(g.buf[2], 7);
    }

    #[test]
    fn fill_alpha_blends_with_what_is_beneath() {
        let mut g = Gfx::new(2, 1);
        let all = Rect { x: 0.0, y: 0.0, w: 2.0, h: 1.0 };
        g.fill_alpha(&all, 0xffffff, 128);
        assert_eq!(g.buf[0], 0x808080);
        g.fill_alpha(&all, 0x000000, 0);
        assert_eq!(g.buf[1], 0x808080);
        g.fill_alpha(&all, 0x102030, 255);
        assert_eq!(g.buf[0], 0x102030);
    }

    #[test]
    fn outline_leaves_interior_untouched() {
        let mut g = Gfx::new(10, 10);
        g.outline(&Rect { x: 2.0, y: 2.0, w: 5.0, h: 5.0 }, 1);
        assert_eq!((g.buf[2 * 10 + 2], g.buf[6 * 10 + 6], g.buf[4 * 10 + 4]), (1, 1, 0));
    }
}
