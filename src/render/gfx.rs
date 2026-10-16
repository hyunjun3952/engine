use super::{DrawCmd, Font, Paint, Style, Transform, font};
use crate::{Rect, Sprite, Vec2};
use std::sync::Arc;

/// Where a `canvas`-sized picture lands in a `win`-sized window when it is scaled up as far as it fits and centred.
pub(crate) fn letterbox((ww, wh): (f32, f32), (w, h): (f32, f32)) -> Rect {
    let k = (ww / w).min(wh / h);
    Rect::new((ww - w * k) / 2.0, (wh - h * k) / 2.0, w * k, h * k)
}

// A plain quad wholly outside the view, with a pixel to spare for the rounding zoom does; anything turned or scaled, and
// every other kind of command, is kept.
fn off_screen(c: &DrawCmd, view: Rect) -> bool {
    let (w, h) = match &c.paint {
        Paint::Color(_) | Paint::Alpha(..) => (c.rect.w, c.rect.h),
        Paint::Sprite(s) => (s.size().0 as f32, s.size().1 as f32),
        Paint::Region { src, .. } => (src.w, src.h),
        _ => return false,
    };
    let seen = Rect::new(view.x - 1.0, view.y - 1.0, view.w + 2.0, view.h + 2.0);
    c.xform.is_identity() && !Rect::new(c.rect.x, c.rect.y, w, h).overlaps(seen)
}

/// What a scene draws on: it records [`DrawCmd`]s for the frame and the renderer draws them afterwards.
///
/// `fill`, `blit`, `text` and the other methods draw straight to the screen in canvas pixels (`i32` positions), for
/// HUD and menus. [`Gfx::draw`] takes world-space commands (from [`draw_list`](crate::draw_list), [`Tilemap::draw_list`](crate::Tilemap::draw_list) or built with
/// [`DrawCmd::new`]), applies the camera and zoom, and sorts them by layer.
///
/// ```
/// use engine::{Gfx, Rect};
///
/// fn draw(gfx: &mut Gfx) {
///     gfx.clear(0x101018);
///     gfx.fill(Rect::new(20.0, 20.0, 40.0, 40.0), 0xffd040);
///     gfx.text("hello", 4, 4, 1, 0xffffff);
/// }
/// ```
pub struct Gfx {
    /// Width of the canvas in pixels.
    pub w: usize,
    /// Height of the canvas in pixels.
    pub h: usize,
    /// True while the debug overlay is on.
    pub debug: bool,
    /// The camera offset that `draw` subtracts.
    pub cam: Vec2,
    /// Magnification of what `draw` is given (the camera's zoom); 1 leaves it as it is.
    pub zoom: f32,
    cmds: Vec<DrawCmd>,
}

impl Gfx {
    pub(crate) fn new(w: usize, h: usize) -> Self {
        Gfx { w, h, debug: false, cam: Vec2::ZERO, zoom: 1.0, cmds: Vec::new() }
    }

    pub(crate) fn draw_cmds(&self) -> &[DrawCmd] {
        &self.cmds
    }

    pub(crate) fn clear_cmds(&mut self) {
        self.cmds.clear();
    }

    fn push(&mut self, rect: Rect, paint: Paint) {
        self.cmds.push(DrawCmd::new(rect, paint));
    }

    /// The part of the world on screen, in world pixels.
    pub fn view(&self) -> Rect {
        Rect::new(self.cam.x, self.cam.y, self.w as f32 / self.zoom, self.h as f32 / self.zoom)
    }

    /// Fills the whole canvas.
    pub fn clear(&mut self, c: u32) {
        self.push(Rect::new(0.0, 0.0, self.w as f32, self.h as f32), Paint::Clear(c));
    }

    /// Fills a rectangle in screen space.
    pub fn fill(&mut self, r: Rect, c: u32) {
        self.push(r, Paint::Color(c));
    }

    /// Fills a rectangle with an alpha from 0 to 255.
    pub fn fill_alpha(&mut self, r: Rect, c: u32, alpha: u8) {
        self.push(r, Paint::Alpha(c, alpha));
    }

    /// Draws a one pixel frame.
    pub fn outline(&mut self, r: Rect, c: u32) {
        self.push(r, Paint::Outline(c));
    }

    /// A line a pixel thick through the pixels at `a` and `b`, ends included (screen pixels, no camera).
    pub fn line(&mut self, a: impl Into<Vec2>, b: impl Into<Vec2>, c: u32) {
        let (a, b) = (a.into(), b.into());
        let d = b - a;
        let len = d.length() + 1.0;
        let mid = (a + b) * 0.5 + Vec2::new(0.5, 0.5);
        let rect = Rect::new(mid.x - len / 2.0, mid.y - 0.5, len, 1.0);
        // A turned flat rect: both backends already draw it alike, and a band a pixel wide leaves no gaps.
        let xform = crate::Transform { angle: d.y.atan2(d.x), ..crate::Transform::IDENTITY };
        self.cmds.push(DrawCmd { xform, ..DrawCmd::new(rect, Paint::Color(c)) });
    }

    /// A filled circle: every pixel whose centre is within `radius` of `center` (screen pixels, no camera).
    pub fn circle(&mut self, center: impl Into<Vec2>, radius: f32, c: u32) {
        let (o, r) = (center.into(), radius);
        if !(r > 0.0 && o.x.is_finite() && o.y.is_finite()) {
            return;
        }
        let (top, bottom) = ((o.y - r - 0.5).ceil().max(0.0), (o.y + r - 0.5).floor().min(self.h as f32 - 1.0));
        for y in top as i32..=bottom as i32 {
            let dy = y as f32 + 0.5 - o.y;
            let half = (r * r - dy * dy).max(0.0).sqrt();
            let (x0, x1) = ((o.x - half - 0.5).ceil(), (o.x + half - 0.5).floor() + 1.0);
            if x1 > x0 {
                self.fill(Rect::new(x0, y as f32, x1 - x0, 1.0), c);
            }
        }
    }

    /// How wide `text` draws `s` at `scale`, in pixels.
    pub fn text_width(s: &str, scale: i32) -> i32 {
        s.chars().count() as i32 * 8 * scale.max(1)
    }

    /// Draws a sprite with its top-left corner at `x`, `y`.
    pub fn blit(&mut self, s: &Arc<Sprite>, x: i32, y: i32) {
        self.push(Rect { x: x as f32, y: y as f32, ..s.bounds() }, Paint::Sprite(s.clone()));
    }

    /// `blit` with a `Style`, for a flash or fade on something drawn straight to the screen.
    pub fn blit_styled(&mut self, s: &Arc<Sprite>, x: i32, y: i32, style: Style) {
        self.cmds.push(DrawCmd {
            style,
            ..DrawCmd::new(Rect { x: x as f32, y: y as f32, ..s.bounds() }, Paint::Sprite(s.clone()))
        });
    }

    /// Multiplies the whole picture drawn so far by `s` (a `vignette`, `scanlines` or any image of the canvas's size).
    pub fn multiply(&mut self, s: &Arc<Sprite>) {
        self.blit_styled(s, 0, 0, Style::PLAIN.multiplied());
    }

    /// Draws the part `src` of a sheet, optionally mirrored.
    pub fn blit_region(&mut self, s: &Arc<Sprite>, src: Rect, x: i32, y: i32, flip: bool) {
        let paint = Paint::Region { sheet: s.clone(), src, flip };
        self.push(Rect::new(x as f32, y as f32, src.w, src.h), paint);
    }

    /// Draws `src` as a panel filling `dest`: `margin` corners at natural size, edges and middle repeated, clipped to the screen.
    pub fn blit_nine(&mut self, s: &Arc<Sprite>, src: Rect, margin: i32, dest: Rect) {
        let m = margin as f32;
        assert!(
            margin >= 0 && src.w >= 2.0 * m + 1.0 && src.h >= 2.0 * m + 1.0,
            "a nine-slice margin must leave a middle"
        );
        assert!(dest.w >= 2.0 * m && dest.h >= 2.0 * m, "a nine-slice panel must hold both margins");
        let seen = |at: f32, len: f32| (-at, len - at);
        let (xs, ys) = (
            spans(src.w, m, dest.w, seen(dest.x, self.w as f32)),
            spans(src.h, m, dest.h, seen(dest.y, self.h as f32)),
        );
        for &(sy, dy, h) in &ys {
            for &(sx, dx, w) in &xs {
                let part = Rect { x: src.x + sx, y: src.y + sy, w, h };
                self.blit_region(s, part, (dest.x + dx).floor() as i32, (dest.y + dy).floor() as i32, false);
            }
        }
    }

    /// `draw` for a far or near layer: the camera counts for `factor` of its movement on each axis, whole pixels (0
    /// keeps the commands still on the screen like a sky, 0.5 scrolls them at half speed, 1 is `draw`).
    pub fn draw_parallax(&mut self, cmds: impl IntoIterator<Item = DrawCmd>, factor: impl Into<Vec2>) {
        let factor = factor.into();
        let cam = self.cam;
        self.cam = Vec2::new((cam.x * factor.x).floor(), (cam.y * factor.y).floor());
        self.draw(cmds);
        self.cam = cam;
    }

    /// Fills the screen with a picture that scrolls at `factor` of the camera's movement, repeating along each axis
    /// where `repeat` is true (a sky, hills). It goes straight to the screen, so call it before the world is drawn.
    pub fn background(&mut self, s: &Arc<Sprite>, factor: impl Into<Vec2>, repeat: (bool, bool)) {
        let factor = factor.into();
        let (sw, sh) = s.size();
        if sw == 0 || sh == 0 {
            return;
        }
        let axis = |cam: f32, k: f32, len: usize, view: usize, repeat: bool| -> Vec<i32> {
            let shift = -(cam * k).floor();
            if !repeat {
                return vec![shift as i32];
            }
            let first = shift.rem_euclid(len as f32) as i32 - len as i32;
            (0..)
                .map(|i| first + i * len as i32)
                .skip_while(|&p| p + (len as i32) <= 0)
                .take_while(|&p| p < view as i32)
                .collect()
        };
        let (xs, ys) =
            (axis(self.cam.x, factor.x, sw, self.w, repeat.0), axis(self.cam.y, factor.y, sh, self.h, repeat.1));
        for &y in &ys {
            for &x in &xs {
                self.blit(s, x, y);
            }
        }
    }

    /// Draws the commands lowest layer first (ties keep their order), so tiles behind and in front of entities mix.
    pub fn draw(&mut self, cmds: impl IntoIterator<Item = DrawCmd>) {
        let view = self.view();
        let mut cmds: Vec<DrawCmd> = cmds.into_iter().filter(|c| !off_screen(c, view)).collect();
        cmds.sort_by_key(|c| c.layer);
        for mut c in cmds {
            if c.debug_only && !self.debug {
                continue;
            }
            if self.zoom == 1.0 {
                c.rect.x -= self.cam.x;
                c.rect.y -= self.cam.y;
                self.cmds.push(c);
            } else {
                self.push_zoomed(c);
            }
        }
    }

    // A world-space command as it lands on screen at this zoom, edges rounded so neighbours keep sharing them.
    fn push_zoomed(&mut self, c: DrawCmd) {
        let (z, cam) = (self.zoom, self.cam);
        let edge = |v: f32, o: f32| ((v - o) * z).round();
        match &c.paint {
            Paint::Clear(_) => self.cmds.push(c),
            Paint::Tiles { sheet, size, tiles } => {
                let (k, sheet_w) = (*size as f32, sheet.size().0);
                for &(gx, gy, id) in tiles {
                    let src = Paint::tile_src(id, *size, sheet_w);
                    let at = Rect::new(c.rect.x + gx as f32 * k, c.rect.y + gy as f32 * k, k, k);
                    let paint = Paint::Region { sheet: sheet.clone(), src, flip: false };
                    self.push_zoomed(DrawCmd { layer: c.layer, ..DrawCmd::new(at, paint) });
                }
            }
            Paint::Mask { src, .. } => {
                let k = ((c.rect.w / src.w).round().max(1.0) * z).round().max(1.0);
                let rect = Rect { x: edge(c.rect.x, cam.x), y: edge(c.rect.y, cam.y), w: src.w * k, h: src.h * k };
                self.cmds.push(DrawCmd { rect, ..c });
            }
            Paint::Glyph { ch, color, scale } => {
                let k = (*scale as f32 * z).round().max(1.0);
                let rect = Rect { x: edge(c.rect.x, cam.x), y: edge(c.rect.y, cam.y), w: 8.0 * k, h: 8.0 * k };
                self.cmds.push(DrawCmd { rect, paint: Paint::Glyph { ch: *ch, color: *color, scale: k as u32 }, ..c });
            }
            Paint::Outline(_) => {
                let (x0, y0) = (edge(c.rect.x, cam.x), edge(c.rect.y, cam.y));
                let rect = Rect {
                    x: x0,
                    y: y0,
                    w: edge(c.rect.x + c.rect.w, cam.x) - x0,
                    h: edge(c.rect.y + c.rect.h, cam.y) - y0,
                };
                self.cmds.push(DrawCmd { rect, ..c });
            }
            Paint::Color(_) | Paint::Alpha(..) | Paint::Sprite(_) | Paint::Region { .. } => {
                // The quad a command draws: its rect for flat colour, the sprite's own size at the rect's corner.
                let (qw, qh) = c.paint.region().map_or((c.rect.w, c.rect.h), |(_, src, _)| (src.w, src.h));
                let (cx, cy) = (c.rect.x + qw / 2.0, c.rect.y + qh / 2.0);
                let t = c.xform;
                // Rounded edges below keep a quad at least a pixel wide, which would draw a zero scale.
                if t.sx == 0.0 || t.sy == 0.0 {
                    return;
                }
                let flat = matches!(c.paint, Paint::Color(_) | Paint::Alpha(..));
                if t.angle != 0.0 {
                    let (nx, ny) = ((cx - cam.x) * z, (cy - cam.y) * z);
                    let rect = Rect::new(nx - qw / 2.0, ny - qh / 2.0, qw, qh);
                    self.cmds.push(DrawCmd { rect, xform: Transform { sx: t.sx * z, sy: t.sy * z, ..t }, ..c });
                    return;
                }
                let (ex, ey) = (qw * t.sx.abs() / 2.0, qh * t.sy.abs() / 2.0);
                let (x0, y0) = (edge(cx - ex, cam.x), edge(cy - ey, cam.y));
                let (x1, y1) = (edge(cx + ex, cam.x).max(x0 + 1.0), edge(cy + ey, cam.y).max(y0 + 1.0));
                if flat {
                    let rect = Rect::new(x0, y0, x1 - x0, y1 - y0);
                    return self.cmds.push(DrawCmd { rect, xform: Transform::IDENTITY, ..c });
                }
                let scale =
                    Transform { angle: 0.0, sx: t.sx.signum() * (x1 - x0) / qw, sy: t.sy.signum() * (y1 - y0) / qh };
                let (nx, ny) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
                let rect = Rect::new(nx - qw / 2.0, ny - qh / 2.0, qw, qh);
                let xform = if scale.is_identity() { Transform::IDENTITY } else { scale };
                self.cmds.push(DrawCmd { rect, xform, ..c });
            }
        }
    }

    /// Draws text with a BDF font.
    pub fn text_font(&mut self, font: &Font, s: &str, x: i32, y: i32, color: u32) {
        self.text_font_scaled(font, s, x, y, 1, color);
    }

    /// Like `text_font` with every font pixel drawn `scale` (at least 1) pixels wide and tall;
    /// `font.width` and `line_height` need multiplying by `scale` to lay out around it.
    pub fn text_font_scaled(&mut self, font: &Font, s: &str, x: i32, y: i32, scale: i32, color: u32) {
        let k = scale.max(1);
        for (src, dx, dy) in font.layout(s) {
            let rect = Rect::new((x + dx * k) as f32, (y + dy * k) as f32, src.w * k as f32, src.h * k as f32);
            self.push(rect, Paint::Mask { sheet: font.atlas.clone(), src, color });
        }
    }

    /// Draws ASCII text with the built-in 8x8 font, `scale` times larger.
    pub fn text(&mut self, s: &str, x: i32, y: i32, scale: i32, c: u32) {
        let (scale, size) = (scale.max(1) as u32, 8.0 * scale.max(1) as f32);
        for (i, ch) in s.chars().enumerate() {
            let code = u32::from(ch);
            if code >= font::GLYPHS as u32 || font::glyph_rows(code as u8) == [0; 8] {
                continue;
            }
            let rect = Rect::new(x as f32 + i as f32 * size, y as f32, size, size);
            self.push(rect, Paint::Glyph { ch: code as u8, color: c, scale });
        }
    }
}

// Along one axis: (offset in the source, offset in the panel, length) for the start margin, the middle repeated up to
// the end margin, and the end margin, keeping only what overlaps `lo..hi` in panel coordinates (the screen).
fn spans(src_len: f32, m: f32, dst_len: f32, (lo, hi): (f32, f32)) -> Vec<(f32, f32, f32)> {
    let (mid, room) = (src_len - 2.0 * m, dst_len - 2.0 * m);
    let mut out = Vec::new();
    let mut keep = |s: (f32, f32, f32)| {
        if s.2 > 0.0 && s.1 + s.2 > lo && s.1 < hi {
            out.push(s);
        }
    };
    keep((0.0, 0.0, m));
    // Start at the first repeat that can be in view instead of counting from the panel's edge.
    let mut at = ((lo - m) / mid).floor().max(0.0) * mid;
    while at < room && m + at < hi {
        keep((m, m + at, mid.min(room - at)));
        at += mid;
    }
    keep((src_len - m, dst_len - m, m));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Transform;

    #[test]
    fn draw_applies_the_camera_and_skips_debug_commands_unless_enabled() {
        let cmd = |debug_only| DrawCmd { debug_only, ..DrawCmd::new(Rect::new(5.0, 2.0, 1.0, 1.0), Paint::Color(7)) };
        let mut g = Gfx::new(8, 8);
        g.cam = Vec2::new(3.0, 1.0);
        g.draw([cmd(false), cmd(true)]);
        assert_eq!(g.draw_cmds().len(), 1);
        assert_eq!((g.draw_cmds()[0].rect.x, g.draw_cmds()[0].rect.y), (2.0, 1.0));
        g.debug = true;
        g.draw([cmd(true)]);
        assert_eq!(g.draw_cmds().len(), 2);
    }

    #[test]
    fn draw_drops_plain_quads_outside_the_view_and_keeps_turned_ones() {
        let quad = |x| DrawCmd::new(Rect::new(x, 2.0, 4.0, 4.0), Paint::Color(7));
        let turned = DrawCmd { xform: Transform { angle: 1.0, ..Transform::IDENTITY }, ..quad(40.0) };
        let mut g = Gfx::new(8, 8);
        g.cam = Vec2::new(10.0, 0.0);
        g.draw([quad(2.0), quad(7.0), quad(17.0), quad(25.0), turned]);
        let xs: Vec<f32> = g.draw_cmds().iter().map(|c| c.rect.x).collect();
        assert_eq!(xs, [-3.0, 7.0, 30.0], "left of the view, inside it, at its edge, and the kept turned one");
    }

    fn zoomed(zoom: f32, cam: (f32, f32)) -> Gfx {
        let mut g = Gfx::new(100, 60);
        (g.zoom, g.cam) = (zoom, cam.into());
        g
    }

    fn at(g: &Gfx, i: usize) -> (f32, f32, f32, f32) {
        let r = g.draw_cmds()[i].rect;
        (r.x, r.y, r.w, r.h)
    }

    #[test]
    fn the_view_shrinks_with_the_zoom_and_flat_rects_land_on_whole_pixels() {
        let mut g = zoomed(2.0, (8.0, 8.0));
        assert_eq!((g.view().w, g.view().h), (50.0, 30.0));
        g.draw([DrawCmd::new(Rect::new(10.0, 10.0, 4.0, 4.0), Paint::Color(7))]);
        assert_eq!(at(&g, 0), (4.0, 4.0, 8.0, 8.0));
        assert!(g.draw_cmds()[0].xform.is_identity(), "a flat rect is just a bigger rect, no per-pixel warp");
    }

    #[test]
    fn rects_that_shared_an_edge_still_share_it_at_a_fractional_zoom() {
        let mut g = zoomed(1.5, (0.0, 0.0));
        let rects = (0..6).map(|i| DrawCmd::new(Rect::new(i as f32 * 3.0, 0.0, 3.0, 3.0), Paint::Color(i)));
        g.draw(rects);
        for i in 1..6 {
            let (prev, now) = (at(&g, i - 1), at(&g, i));
            assert_eq!(prev.0 + prev.2, now.0, "gap or overlap between rect {} and {i}", i - 1);
        }
        let tiny = DrawCmd::new(Rect::new(0.0, 0.0, 0.2, 0.2), Paint::Color(1));
        let mut g = zoomed(0.5, (0.0, 0.0));
        g.draw([tiny]);
        assert_eq!((at(&g, 0).2, at(&g, 0).3), (1.0, 1.0), "a small thing stays at least a pixel");
        let flat = Transform { sx: 0.0, ..Transform::IDENTITY };
        g.draw([DrawCmd { xform: flat, ..DrawCmd::new(Rect::new(0.0, 0.0, 4.0, 4.0), Paint::Color(1)) }]);
        assert_eq!(g.draw_cmds().len(), 1, "a zero scale draws nothing");
    }

    #[test]
    fn a_sprite_is_scaled_about_its_centre_onto_exactly_the_rounded_pixels() {
        let sheet = Arc::new(Sprite::new(4, 4, vec![0xFFFF_FFFF; 16]));
        let mut g = zoomed(2.5, (0.0, 0.0));
        g.blit(&sheet, 10, 10);
        let mut z = zoomed(2.5, (0.0, 0.0));
        z.draw(g.draw_cmds().to_vec());
        let c = &z.draw_cmds()[0];
        // The sprite covers world 10..14, so 25..35 on the screen: ten pixels, a scale of 2.5 on a 4 px quad.
        assert_eq!((c.xform.sx, c.xform.sy, c.xform.angle), (2.5, 2.5, 0.0));
        let (cx, cy) = (c.rect.x + 2.0, c.rect.y + 2.0);
        assert_eq!((cx, cy), (30.0, 30.0));
        let mut whole = zoomed(1.0, (0.0, 0.0));
        whole.draw([DrawCmd::new(Rect::new(3.0, 3.0, 4.0, 4.0), Paint::Sprite(sheet))]);
        assert!(whole.draw_cmds()[0].xform.is_identity(), "no zoom leaves a sprite on the fast path");
    }

    #[test]
    fn a_turned_and_mirrored_command_keeps_its_turn_and_grows_about_its_centre() {
        let mut g = zoomed(2.0, (0.0, 0.0));
        let turn = Transform { angle: 0.5, sx: -1.0, sy: 1.0 };
        g.draw([DrawCmd { xform: turn, ..DrawCmd::new(Rect::new(10.0, 10.0, 6.0, 4.0), Paint::Color(1)) }]);
        let c = &g.draw_cmds()[0];
        assert_eq!((c.xform.angle, c.xform.sx, c.xform.sy), (0.5, -2.0, 2.0));
        assert_eq!((c.rect.x + c.rect.w / 2.0, c.rect.y + c.rect.h / 2.0), (26.0, 24.0));
    }

    #[test]
    fn a_tile_batch_becomes_one_region_per_tile_at_the_zoomed_grid() {
        let sheet = Arc::new(Sprite::new(8, 4, vec![0xFFFF_FFFF; 32]));
        let tiles = Paint::Tiles { sheet, size: 4, tiles: vec![(0, 0, 1), (1, 0, 2), (0, 1, 2)] };
        let mut g = zoomed(2.0, (0.0, 0.0));
        g.draw([DrawCmd::new(Rect::new(8.0, 8.0, 8.0, 8.0), tiles)]);
        assert_eq!(g.draw_cmds().len(), 3);
        let places: Vec<(f32, f32)> =
            g.draw_cmds().iter().map(|c| (c.rect.x + 2.0 - 4.0, c.rect.y + 2.0 - 4.0)).collect();
        // Each is a 4 px tile scaled to 8: the centres of the first row sit 8 apart.
        assert_eq!(g.draw_cmds()[1].rect.x - g.draw_cmds()[0].rect.x, 8.0);
        assert_eq!(g.draw_cmds()[2].rect.y - g.draw_cmds()[0].rect.y, 8.0);
        assert!(places.iter().all(|p| p.0 >= 16.0 && p.1 >= 16.0));
        assert!(
            matches!(&g.draw_cmds()[1].paint, Paint::Region { src, .. } if src.x == 4.0),
            "tile id 2 is the second column"
        );
    }

    #[test]
    fn world_text_grows_by_whole_numbers_and_outlines_follow_the_rect() {
        let font = Font::load("assets/fonts/galmuri9.bdf").unwrap();
        let mut plain = Gfx::new(100, 60);
        plain.text_font(&font, "가", 10, 10, 0xffffff);
        plain.text("A", 20, 20, 2, 0xffffff);
        plain.outline(Rect::new(4.0, 4.0, 10.0, 6.0), 3);
        let mut g = zoomed(1.6, (0.0, 0.0));
        g.draw(plain.draw_cmds().to_vec());
        let sizes: Vec<f32> = g.draw_cmds().iter().map(|c| c.rect.w).collect();
        assert_eq!(sizes[0], plain.draw_cmds()[0].rect.w * 2.0, "1.6 rounds to 2 for font pixels");
        assert_eq!(sizes[1], 8.0 * 3.0, "scale 2 times 1.6 is 3.2, so 3");
        assert_eq!(
            at(&g, 2),
            (6.0, 6.0, 16.0, 10.0),
            "an outline is the rect with each edge rounded: 4 to 6.4, 14 to 22.4"
        );
    }

    #[test]
    fn a_nine_slice_covers_the_panel_exactly_once_with_corners_at_natural_size() {
        let sheet = Arc::new(Sprite::new(12, 12, vec![0xFFFF_FFFF; 144]));
        let src = sheet.bounds();
        for (w, h) in [(8.0, 8.0), (20.0, 14.0), (31.0, 9.0), (50.0, 50.0)] {
            let mut g = Gfx::new(64, 64);
            let dest = Rect { x: 5.0, y: 7.0, w, h };
            g.blit_nine(&sheet, src, 4, dest);
            let area: f32 = g.draw_cmds().iter().map(|c| c.rect.w * c.rect.h).sum();
            assert_eq!(area, w * h, "{w}x{h}: every pixel once");
            for c in g.draw_cmds() {
                let r = c.rect;
                assert!(r.x >= dest.x && r.y >= dest.y && r.x + r.w <= dest.x + w && r.y + r.h <= dest.y + h, "{r:?}");
            }
            let corners = g.draw_cmds().iter().filter(|c| c.rect.w == 4.0 && c.rect.h == 4.0).count();
            assert!(corners >= 4, "{w}x{h}");
        }
    }

    #[test]
    fn a_huge_nine_slice_records_only_the_pieces_on_screen_and_still_covers_it_once() {
        let sheet = Arc::new(Sprite::new(12, 12, vec![0xFFFF_FFFF; 144]));
        let record = |x: f32, w: f32| {
            let mut g = Gfx::new(64, 48);
            g.blit_nine(&sheet, sheet.bounds(), 4, Rect { x, y: -10.0, w, h: 1e9 });
            g
        };
        // Positions an f32 can still tell a pixel apart at: the part of the panel in view is covered exactly once.
        for (x, w) in [(-5e6, 1e7), (-1e6, 1e6 + 20.0), (10.0, 1e7), (-30.0, 1e6), (-3e6, f32::INFINITY)] {
            let g = record(x, w);
            assert!(g.draw_cmds().len() < 400, "{} commands for a panel at {x} of width {w}", g.draw_cmds().len());
            let seen: f32 = g
                .draw_cmds()
                .iter()
                .map(|c| {
                    let r = c.rect;
                    ((r.x + r.w).min(64.0) - r.x.max(0.0)).max(0.0) * ((r.y + r.h).min(48.0) - r.y.max(0.0)).max(0.0)
                })
                .sum();
            let wanted = ((x + w).min(64.0) - x.max(0.0)).max(0.0) * 48.0;
            assert_eq!(seen, wanted, "panel at {x}, width {w}");
        }
        // Absurd sizes no f32 can place pixels in: nothing hangs or fills memory.
        for (x, w) in [(-30.0, 1e30), (-1e30, f32::INFINITY), (0.0, f32::INFINITY)] {
            assert!(record(x, w).draw_cmds().len() < 400, "{x} {w}");
        }
    }

    #[test]
    #[should_panic(expected = "hold both margins")]
    fn a_nine_slice_panel_smaller_than_its_margins_is_refused() {
        let sheet = Arc::new(Sprite::new(12, 12, vec![0; 144]));
        Gfx::new(64, 64).blit_nine(&sheet, sheet.bounds(), 4, Rect::new(0.0, 0.0, 7.0, 20.0));
    }

    #[test]
    fn text_records_one_command_per_visible_ascii_char_at_the_right_offset() {
        let mut g = Gfx::new(64, 16);
        g.text("A B\u{ac00}", 4, 2, 2, 5);
        let at: Vec<f32> = g.draw_cmds().iter().map(|c| c.rect.x).collect();
        assert_eq!(at, [4.0, 4.0 + 2.0 * 16.0]);
        assert!(matches!(g.draw_cmds()[0].paint, Paint::Glyph { ch: b'A', color: 5, scale: 2 }));
        assert_eq!(g.draw_cmds()[0].rect.w, 16.0);
    }

    #[test]
    fn the_picture_is_scaled_up_as_far_as_it_fits_and_centred() {
        let canvas = (640.0, 360.0);
        assert_eq!(letterbox(canvas, canvas), Rect::new(0.0, 0.0, 640.0, 360.0));
        assert_eq!(letterbox((1280.0, 1000.0), canvas), Rect::new(0.0, 140.0, 1280.0, 720.0));
        assert_eq!(letterbox((1600.0, 720.0), canvas), Rect::new(160.0, 0.0, 1280.0, 720.0));
    }

    #[test]
    fn draw_puts_lower_layers_first_and_keeps_the_order_within_a_layer() {
        let cmd = |layer, color| DrawCmd { layer, ..DrawCmd::new(Rect::new(0.0, 0.0, 1.0, 1.0), Paint::Color(color)) };
        let mut g = Gfx::new(8, 8);
        g.draw([cmd(5, 1), cmd(-3, 2), cmd(5, 3), cmd(0, 4)]);
        let colors: Vec<u32> =
            g.draw_cmds().iter().filter_map(|c| if let Paint::Color(x) = c.paint { Some(x) } else { None }).collect();
        assert_eq!(colors, [2, 4, 1, 3]);
    }

    #[test]
    fn a_parallax_layer_follows_the_camera_by_its_factor_and_the_camera_is_left_alone() {
        let mut g = Gfx::new(200, 200);
        g.cam = Vec2::new(100.0, 40.0);
        let mark = DrawCmd::new(Rect::new(150.0, 50.0, 4.0, 4.0), Paint::Color(1));
        g.draw_parallax([mark.clone()], (0.5, 1.0));
        g.draw_parallax([mark.clone()], (0.0, 0.0));
        g.draw([mark]);
        let at: Vec<_> = g.draw_cmds().iter().map(|c| (c.rect.x, c.rect.y)).collect();
        assert_eq!(at, [(100.0, 10.0), (150.0, 50.0), (50.0, 10.0)]);
        assert_eq!(g.cam, Vec2::new(100.0, 40.0));
    }

    #[test]
    fn a_background_scrolls_by_its_factor_and_repeats_to_cover_the_screen() {
        let sky = Arc::new(Sprite::new(8, 4, vec![0xFFFF_FFFF; 32]));
        let xs = |cam: f32, factor: f32, repeat: bool| {
            let mut g = Gfx::new(20, 4);
            g.cam = Vec2::new(cam, 0.0);
            g.background(&sky, (factor, 0.0), (repeat, false));
            g.draw_cmds().iter().map(|c| c.rect.x as i32).collect::<Vec<_>>()
        };
        assert_eq!(xs(0.0, 1.0, true), [0, 8, 16]);
        assert_eq!(xs(10.0, 0.5, true), [-5, 3, 11, 19]);
        assert_eq!(xs(-3.0, 1.0, true), [-5, 3, 11, 19], "scrolling backwards shows the end of the picture first");
        assert_eq!(xs(100.0, 0.0, true), [0, 8, 16], "a factor of 0 does not move");
        assert_eq!(xs(10.0, 0.5, false), [-5], "without repeat it is placed once");
    }
}
