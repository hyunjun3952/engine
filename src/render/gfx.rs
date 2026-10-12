use super::{DrawCmd, Font, Paint, Transform, font};
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
    /// Magnification of what `draw` is given (the camera's zoom); 1 leaves it as it is.
    pub zoom: f32,
    cmds: Vec<DrawCmd>,
}

impl Gfx {
    pub(crate) fn new(w: usize, h: usize) -> Self {
        Gfx { w, h, debug: false, cam: (0.0, 0.0), zoom: 1.0, cmds: Vec::new() }
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
        Rect { x: self.cam.0, y: self.cam.1, w: self.w as f32 / self.zoom, h: self.h as f32 / self.zoom }
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

    /// Draws the region `src` of a sheet as a panel filling `dest`: a `margin` wide frame at natural size and the
    /// parts between repeated (not stretched, which suits pixel art), the last repeat cut short to fit. The middle of
    /// `src` is what repeats, so it must be wider and taller than nothing; `dest` must hold both margins.
    pub fn blit_nine(&mut self, s: &Arc<Sprite>, src: Rect, margin: i32, dest: &Rect) {
        let m = margin as f32;
        assert!(margin >= 0 && src.w > 2.0 * m && src.h > 2.0 * m, "a nine-slice margin must leave a middle");
        assert!(dest.w >= 2.0 * m && dest.h >= 2.0 * m, "a nine-slice panel must hold both margins");
        let (xs, ys) = (spans(src.w, m, dest.w), spans(src.h, m, dest.h));
        for &(sy, dy, h) in &ys {
            for &(sx, dx, w) in &xs {
                let part = Rect { x: src.x + sx, y: src.y + sy, w, h };
                self.blit_region(s, part, (dest.x + dx) as i32, (dest.y + dy) as i32, false);
            }
        }
    }

    /// Draws the commands lowest layer first (ties keep their order), so tiles behind and in front of entities mix.
    pub fn draw(&mut self, cmds: impl IntoIterator<Item = DrawCmd>) {
        let mut cmds: Vec<DrawCmd> = cmds.into_iter().collect();
        cmds.sort_by_key(|c| c.layer);
        for mut c in cmds {
            if c.debug_only && !self.debug {
                continue;
            }
            if self.zoom == 1.0 {
                c.rect.x -= self.cam.0;
                c.rect.y -= self.cam.1;
                self.cmds.push(c);
            } else {
                self.push_zoomed(c);
            }
        }
    }

    // A world-space command as it lands on the screen at the current zoom. Edges are rounded to whole pixels, so
    // neighbours that shared an edge still share it. Sprites and turned or scaled shapes are scaled by their
    // `Transform`, which both backends already draw; text can only grow by whole numbers, so it is rounded.
    fn push_zoomed(&mut self, c: DrawCmd) {
        let (z, cam) = (self.zoom, self.cam);
        let edge = |v: f32, o: f32| ((v - o) * z).round();
        match &c.paint {
            Paint::Clear(_) => self.cmds.push(c),
            Paint::Tiles { sheet, size, tiles } => {
                let k = *size as f32;
                for &(gx, gy, id) in tiles {
                    let src = Paint::tile_src(id, *size);
                    let at = Rect { x: c.rect.x + gx as f32 * k, y: c.rect.y + gy as f32 * k, w: k, h: k };
                    let paint = Paint::Region { sheet: sheet.clone(), src, flip: false };
                    self.push_zoomed(DrawCmd {
                        layer: c.layer,
                        rect: at,
                        paint,
                        debug_only: false,
                        xform: Transform::IDENTITY,
                    });
                }
            }
            Paint::Mask { src, .. } => {
                let k = ((c.rect.w / src.w).round().max(1.0) * z).round().max(1.0);
                let rect = Rect { x: edge(c.rect.x, cam.0), y: edge(c.rect.y, cam.1), w: src.w * k, h: src.h * k };
                self.cmds.push(DrawCmd { rect, ..c });
            }
            Paint::Glyph { ch, color, scale } => {
                let k = (*scale as f32 * z).round().max(1.0);
                let rect = Rect { x: edge(c.rect.x, cam.0), y: edge(c.rect.y, cam.1), w: 8.0 * k, h: 8.0 * k };
                self.cmds.push(DrawCmd { rect, paint: Paint::Glyph { ch: *ch, color: *color, scale: k as u32 }, ..c });
            }
            Paint::Outline(_) => {
                let (x0, y0) = (edge(c.rect.x, cam.0), edge(c.rect.y, cam.1));
                let rect = Rect {
                    x: x0,
                    y: y0,
                    w: edge(c.rect.x + c.rect.w, cam.0) - x0,
                    h: edge(c.rect.y + c.rect.h, cam.1) - y0,
                };
                self.cmds.push(DrawCmd { rect, ..c });
            }
            Paint::Color(_) | Paint::Alpha(..) | Paint::Sprite(_) | Paint::Region { .. } => {
                // The quad a command draws: its rect for flat colour, the sprite's own size at the rect's corner.
                let (qw, qh) = c.paint.region().map_or((c.rect.w, c.rect.h), |(_, src, _)| (src.w, src.h));
                let (cx, cy) = (c.rect.x + qw / 2.0, c.rect.y + qh / 2.0);
                let t = c.xform;
                let flat = matches!(c.paint, Paint::Color(_) | Paint::Alpha(..));
                if t.angle != 0.0 {
                    let (nx, ny) = ((cx - cam.0) * z, (cy - cam.1) * z);
                    let rect = Rect { x: nx - qw / 2.0, y: ny - qh / 2.0, w: qw, h: qh };
                    self.cmds.push(DrawCmd { rect, xform: Transform { sx: t.sx * z, sy: t.sy * z, ..t }, ..c });
                    return;
                }
                let (ex, ey) = (qw * t.sx.abs() / 2.0, qh * t.sy.abs() / 2.0);
                let (x0, y0) = (edge(cx - ex, cam.0), edge(cy - ey, cam.1));
                let (x1, y1) = (edge(cx + ex, cam.0).max(x0 + 1.0), edge(cy + ey, cam.1).max(y0 + 1.0));
                if flat {
                    let rect = Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
                    return self.cmds.push(DrawCmd { rect, xform: Transform::IDENTITY, ..c });
                }
                let scale =
                    Transform { angle: 0.0, sx: t.sx.signum() * (x1 - x0) / qw, sy: t.sy.signum() * (y1 - y0) / qh };
                let (nx, ny) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
                let rect = Rect { x: nx - qw / 2.0, y: ny - qh / 2.0, w: qw, h: qh };
                let xform = if scale.is_identity() { Transform::IDENTITY } else { scale };
                self.cmds.push(DrawCmd { rect, xform, ..c });
            }
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

impl Image {
    /// Whether a source rectangle is at least a pixel big and lies wholly inside the picture. A sheet swapped for a smaller one
    /// by hot reload leaves frames and tiles pointing outside it; a rectangle that is not a number holds nothing.
    pub(crate) fn holds(&self, r: &Rect) -> bool {
        r.w >= 1.0 && r.h >= 1.0 && r.x >= 0.0 && r.y >= 0.0 && r.x + r.w <= self.w as f32 && r.y + r.h <= self.h as f32
    }
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

// Along one axis: (offset in the source, offset in the panel, length) for the start margin, the middle repeated up to
// the end margin, and the end margin.
fn spans(src_len: f32, m: f32, dst_len: f32) -> Vec<(f32, f32, f32)> {
    let (mid, room) = (src_len - 2.0 * m, dst_len - 2.0 * m);
    let mut out = vec![(0.0, 0.0, m)];
    let mut at = 0.0;
    while at < room {
        out.push((m, m + at, mid.min(room - at)));
        at += mid;
    }
    out.push((src_len - m, dst_len - m, m));
    out.retain(|s| s.2 > 0.0);
    out
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

    fn zoomed(zoom: f32, cam: (f32, f32)) -> Gfx {
        let mut g = Gfx::new(100, 60);
        (g.zoom, g.cam) = (zoom, cam);
        g
    }

    fn at(g: &Gfx, i: usize) -> (f32, f32, f32, f32) {
        let r = g.cmds()[i].rect;
        (r.x, r.y, r.w, r.h)
    }

    #[test]
    fn the_view_shrinks_with_the_zoom_and_flat_rects_land_on_whole_pixels() {
        let mut g = zoomed(2.0, (8.0, 8.0));
        assert_eq!((g.view().w, g.view().h), (50.0, 30.0));
        g.draw([DrawCmd::new(Rect { x: 10.0, y: 10.0, w: 4.0, h: 4.0 }, Paint::Color(7))]);
        assert_eq!(at(&g, 0), (4.0, 4.0, 8.0, 8.0));
        assert!(g.cmds()[0].xform.is_identity(), "a flat rect is just a bigger rect, no per-pixel warp");
    }

    #[test]
    fn rects_that_shared_an_edge_still_share_it_at_a_fractional_zoom() {
        let mut g = zoomed(1.5, (0.0, 0.0));
        let rects = (0..6).map(|i| DrawCmd::new(Rect { x: i as f32 * 3.0, y: 0.0, w: 3.0, h: 3.0 }, Paint::Color(i)));
        g.draw(rects);
        for i in 1..6 {
            let (prev, now) = (at(&g, i - 1), at(&g, i));
            assert_eq!(prev.0 + prev.2, now.0, "gap or overlap between rect {} and {i}", i - 1);
        }
        let tiny = DrawCmd::new(Rect { x: 0.0, y: 0.0, w: 0.2, h: 0.2 }, Paint::Color(1));
        let mut g = zoomed(0.5, (0.0, 0.0));
        g.draw([tiny]);
        assert_eq!((at(&g, 0).2, at(&g, 0).3), (1.0, 1.0), "a small thing stays at least a pixel");
    }

    #[test]
    fn a_sprite_is_scaled_about_its_centre_onto_exactly_the_rounded_pixels() {
        let sheet = Arc::new(Sprite::from_pixels(4, 4, vec![0xFFFF_FFFF; 16]));
        let mut g = zoomed(2.5, (0.0, 0.0));
        g.blit(&sheet, 10, 10);
        let mut z = zoomed(2.5, (0.0, 0.0));
        z.draw(g.cmds().to_vec());
        let c = &z.cmds()[0];
        // The sprite covers world 10..14, so 25..35 on the screen: ten pixels, a scale of 2.5 on a 4 px quad.
        assert_eq!((c.xform.sx, c.xform.sy, c.xform.angle), (2.5, 2.5, 0.0));
        let (cx, cy) = (c.rect.x + 2.0, c.rect.y + 2.0);
        assert_eq!((cx, cy), (30.0, 30.0));
        let mut whole = zoomed(1.0, (0.0, 0.0));
        whole.draw([DrawCmd::new(Rect { x: 3.0, y: 3.0, w: 4.0, h: 4.0 }, Paint::Sprite(sheet))]);
        assert!(whole.cmds()[0].xform.is_identity(), "no zoom leaves a sprite on the fast path");
    }

    #[test]
    fn a_turned_and_mirrored_command_keeps_its_turn_and_grows_about_its_centre() {
        let mut g = zoomed(2.0, (0.0, 0.0));
        let turn = Transform { angle: 0.5, sx: -1.0, sy: 1.0 };
        g.draw([DrawCmd { xform: turn, ..DrawCmd::new(Rect { x: 10.0, y: 10.0, w: 6.0, h: 4.0 }, Paint::Color(1)) }]);
        let c = &g.cmds()[0];
        assert_eq!((c.xform.angle, c.xform.sx, c.xform.sy), (0.5, -2.0, 2.0));
        assert_eq!((c.rect.x + c.rect.w / 2.0, c.rect.y + c.rect.h / 2.0), (26.0, 24.0));
    }

    #[test]
    fn a_tile_batch_becomes_one_region_per_tile_at_the_zoomed_grid() {
        let sheet = Arc::new(Sprite::from_pixels(8, 4, vec![0xFFFF_FFFF; 32]));
        let tiles = Paint::Tiles { sheet, size: 4, tiles: vec![(0, 0, 1), (1, 0, 2), (0, 1, 2)] };
        let mut g = zoomed(2.0, (0.0, 0.0));
        g.draw([DrawCmd::new(Rect { x: 8.0, y: 8.0, w: 8.0, h: 8.0 }, tiles)]);
        assert_eq!(g.cmds().len(), 3);
        let places: Vec<(f32, f32)> = g.cmds().iter().map(|c| (c.rect.x + 2.0 - 4.0, c.rect.y + 2.0 - 4.0)).collect();
        // Each is a 4 px tile scaled to 8: the centres of the first row sit 8 apart.
        assert_eq!(g.cmds()[1].rect.x - g.cmds()[0].rect.x, 8.0);
        assert_eq!(g.cmds()[2].rect.y - g.cmds()[0].rect.y, 8.0);
        assert!(places.iter().all(|p| p.0 >= 16.0 && p.1 >= 16.0));
        assert!(
            matches!(&g.cmds()[1].paint, Paint::Region { src, .. } if src.x == 4.0),
            "tile id 2 is the second column"
        );
    }

    #[test]
    fn world_text_grows_by_whole_numbers_and_outlines_follow_the_rect() {
        let font = Font::load("assets/fonts/galmuri9.bdf").unwrap();
        let mut plain = Gfx::new(100, 60);
        plain.text_font(&font, "가", 10, 10, 0xffffff);
        plain.text("A", 20, 20, 2, 0xffffff);
        plain.outline(&Rect { x: 4.0, y: 4.0, w: 10.0, h: 6.0 }, 3);
        let mut g = zoomed(1.6, (0.0, 0.0));
        g.draw(plain.cmds().to_vec());
        let sizes: Vec<f32> = g.cmds().iter().map(|c| c.rect.w).collect();
        assert_eq!(sizes[0], plain.cmds()[0].rect.w * 2.0, "1.6 rounds to 2 for font pixels");
        assert_eq!(sizes[1], 8.0 * 3.0, "scale 2 times 1.6 is 3.2, so 3");
        assert_eq!(
            at(&g, 2),
            (6.0, 6.0, 16.0, 10.0),
            "an outline is the rect with each edge rounded: 4 to 6.4, 14 to 22.4"
        );
    }

    #[test]
    fn a_nine_slice_covers_the_panel_exactly_once_with_corners_at_natural_size() {
        let sheet = Arc::new(Sprite::from_pixels(12, 12, vec![0xFFFF_FFFF; 144]));
        let src = sheet.bounds();
        for (w, h) in [(8.0, 8.0), (20.0, 14.0), (31.0, 9.0), (50.0, 50.0)] {
            let mut g = Gfx::new(64, 64);
            let dest = Rect { x: 5.0, y: 7.0, w, h };
            g.blit_nine(&sheet, src, 4, &dest);
            let area: f32 = g.cmds().iter().map(|c| c.rect.w * c.rect.h).sum();
            assert_eq!(area, w * h, "{w}x{h}: every pixel once");
            for c in g.cmds() {
                let r = c.rect;
                assert!(r.x >= dest.x && r.y >= dest.y && r.x + r.w <= dest.x + w && r.y + r.h <= dest.y + h, "{r:?}");
            }
            let corners = g.cmds().iter().filter(|c| c.rect.w == 4.0 && c.rect.h == 4.0).count();
            assert!(corners >= 4, "{w}x{h}");
        }
    }

    #[test]
    #[should_panic(expected = "hold both margins")]
    fn a_nine_slice_panel_smaller_than_its_margins_is_refused() {
        let sheet = Arc::new(Sprite::from_pixels(12, 12, vec![0; 144]));
        Gfx::new(64, 64).blit_nine(&sheet, sheet.bounds(), 4, &Rect { x: 0.0, y: 0.0, w: 7.0, h: 20.0 });
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
