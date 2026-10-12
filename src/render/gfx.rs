use super::{DrawCmd, Font, Paint, font};
use std::sync::{
    Arc, RwLock, RwLockReadGuard,
    atomic::{AtomicU64, Ordering},
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn center(&self) -> (f32, f32) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    pub fn penetration(&self, o: &Rect) -> Option<(f32, f32)> {
        let ox = (self.x + self.w).min(o.x + o.w) - self.x.max(o.x);
        let oy = (self.y + self.h).min(o.y + o.h) - self.y.max(o.y);
        (ox > 0.0 && oy > 0.0).then_some((ox, oy))
    }

    pub fn overlaps(&self, o: &Rect) -> bool {
        self.penetration(o).is_some()
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }

    /// The one-pixel top, bottom, left and right borders, as an outline is drawn.
    pub fn edges(&self) -> [Rect; 4] {
        [
            Rect { h: 1.0, ..*self },
            Rect { y: self.y + self.h - 1.0, h: 1.0, ..*self },
            Rect { w: 1.0, ..*self },
            Rect { x: self.x + self.w - 1.0, w: 1.0, ..*self },
        ]
    }
}

/// Where a `canvas`-sized picture lands in a `win`-sized window when it is scaled up as far as it fits and centred.
pub(crate) fn letterbox((ww, wh): (f32, f32), (w, h): (f32, f32)) -> Rect {
    let k = (ww / w).min(wh / h);
    Rect { x: (ww - w * k) / 2.0, y: (wh - h * k) / 2.0, w: w * k, h: h * k }
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
        self.cmds.push(DrawCmd::new(rect, paint));
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
        self.push(Rect { x: x as f32, y: y as f32, ..s.bounds() }, Paint::Sprite(s.clone()));
    }

    pub fn blit_region(&mut self, s: &Arc<Sprite>, src: Rect, x: i32, y: i32, flip: bool) {
        let paint = Paint::Region { sheet: s.clone(), src, flip };
        self.push(Rect { x: x as f32, y: y as f32, w: src.w, h: src.h }, paint);
    }

    /// Draws the commands lowest layer first (ties keep their order), so tiles behind and in front of entities mix.
    pub fn draw(&mut self, cmds: impl IntoIterator<Item = DrawCmd>) {
        let mut cmds: Vec<DrawCmd> = cmds.into_iter().collect();
        cmds.sort_by_key(|c| c.layer);
        for mut c in cmds {
            if c.debug_only && !self.debug {
                continue;
            }
            c.rect.x -= self.cam.0;
            c.rect.y -= self.cam.1;
            self.cmds.push(c);
        }
    }

    pub fn text_font(&mut self, font: &Font, s: &str, x: i32, y: i32, color: u32) {
        self.text_font_scaled(font, s, x, y, 1, color);
    }

    /// Like `text_font` with every font pixel drawn `scale` (at least 1) pixels wide and tall;
    /// `font.width` and `line_height` need multiplying by `scale` to lay out around it.
    pub fn text_font_scaled(&mut self, font: &Font, s: &str, x: i32, y: i32, scale: i32, color: u32) {
        let k = scale.max(1);
        for (src, dx, dy) in font.layout(s) {
            let rect =
                Rect { x: (x + dx * k) as f32, y: (y + dy * k) as f32, w: src.w * k as f32, h: src.h * k as f32 };
            self.push(rect, Paint::Mask { sheet: font.atlas.clone(), src, color });
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

pub(crate) struct Image {
    pub(crate) w: usize,
    pub(crate) h: usize,
    pub(crate) px: Vec<u32>,
}

pub struct Sprite {
    image: RwLock<Image>,
    version: AtomicU64,
}

impl Sprite {
    pub fn load(path: &str) -> Self {
        Sprite::try_load(path).unwrap_or_else(|e| panic!("{e}"))
    }

    pub fn try_load(path: &str) -> Result<Self, String> {
        Ok(Sprite { image: RwLock::new(decode(path)?), version: AtomicU64::new(0) })
    }

    pub(crate) fn from_pixels(w: usize, h: usize, px: Vec<u32>) -> Self {
        Sprite { image: RwLock::new(Image { w, h, px }), version: AtomicU64::new(0) }
    }

    pub fn size(&self) -> (usize, usize) {
        let img = self.image();
        (img.w, img.h)
    }

    /// The whole image as a rect at the origin.
    pub fn bounds(&self) -> Rect {
        let (w, h) = self.size();
        Rect { x: 0.0, y: 0.0, w: w as f32, h: h as f32 }
    }

    #[cfg(test)]
    pub(crate) fn test_tiles() -> Arc<Sprite> {
        Arc::new(Sprite::load("assets/images/tiles.png"))
    }

    #[cfg(test)]
    pub(crate) fn test_ball() -> Arc<Sprite> {
        Arc::new(Sprite::load("assets/images/ball.png"))
    }

    pub(crate) fn image(&self) -> RwLockReadGuard<'_, Image> {
        self.image.read().unwrap()
    }

    #[cfg(any(feature = "gpu", test))]
    pub(crate) fn version(&self) -> u64 {
        self.version.load(Ordering::Acquire)
    }

    pub(crate) fn reload(&self, path: &str) -> Result<(), String> {
        let image = decode(path)?;
        *self.image.write().unwrap() = image;
        self.version.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }
}

fn decode(path: &str) -> Result<Image, String> {
    let img = image::open(path).map_err(|e| format!("{path}: {e}"))?.to_rgba8();
    let px = img.pixels().map(|p| u32::from_be_bytes([p[3], p[0], p[1], p[2]])).collect();
    Ok(Image { w: img.width() as usize, h: img.height() as usize, px })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Transform;

    #[test]
    fn draw_applies_the_camera_and_skips_debug_commands_unless_enabled() {
        let cmd = |debug_only| DrawCmd {
            layer: 0,
            rect: Rect { x: 5.0, y: 2.0, w: 1.0, h: 1.0 },
            paint: Paint::Color(7),
            debug_only,
            xform: Transform::IDENTITY,
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

    #[test]
    fn the_picture_is_scaled_up_as_far_as_it_fits_and_centred() {
        let canvas = (640.0, 360.0);
        assert_eq!(letterbox(canvas, canvas), Rect { x: 0.0, y: 0.0, w: 640.0, h: 360.0 });
        assert_eq!(letterbox((1280.0, 1000.0), canvas), Rect { x: 0.0, y: 140.0, w: 1280.0, h: 720.0 });
        assert_eq!(letterbox((1600.0, 720.0), canvas), Rect { x: 160.0, y: 0.0, w: 1280.0, h: 720.0 });
    }

    #[test]
    fn draw_puts_lower_layers_first_and_keeps_the_order_within_a_layer() {
        let cmd = |layer, color| DrawCmd {
            layer,
            ..DrawCmd::new(Rect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 }, Paint::Color(color))
        };
        let mut g = Gfx::new(8, 8);
        g.draw([cmd(5, 1), cmd(-3, 2), cmd(5, 3), cmd(0, 4)]);
        let colors: Vec<u32> =
            g.cmds().iter().filter_map(|c| if let Paint::Color(x) = c.paint { Some(x) } else { None }).collect();
        assert_eq!(colors, [2, 4, 1, 3]);
    }
}
