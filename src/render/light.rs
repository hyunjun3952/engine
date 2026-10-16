use super::{Gfx, Sprite, Style};
use crate::Vec2;
use std::sync::Arc;

/// A point of light on a `LightMap`: where it is, how far it reaches (pixels), its colour (0xRRGGBB) and how strongly
/// it shines (1 at its centre restores the colour of what is there to itself; more only saturates).
///
/// ```
/// use engine::Light;
///
/// let torch = Light::new((40.0, 30.0), 64.0).colored(0xffc060).intensity(0.8);
/// assert_eq!(torch.radius, 64.0);
/// assert_eq!(torch.intensity, 0.8);
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Light {
    /// Centre x in pixels.
    pub x: f32,
    /// Centre y in pixels.
    pub y: f32,
    /// How far the light reaches.
    pub radius: f32,
    /// Colour, 0xRRGGBB.
    pub color: u32,
    /// Strength; 1 is full.
    pub intensity: f32,
}

impl Light {
    /// A white light of full strength.
    pub fn new(at: impl Into<Vec2>, radius: f32) -> Self {
        let at = at.into();
        Light { x: at.x, y: at.y, radius, color: 0xFF_FFFF, intensity: 1.0 }
    }

    /// Sets the colour.
    pub fn colored(self, color: u32) -> Self {
        Light { color: color & 0xFF_FFFF, ..self }
    }

    /// Sets the strength.
    pub fn intensity(self, intensity: f32) -> Self {
        Light { intensity, ..self }
    }
}

/// Darkness with lit places, laid over the picture by multiplying: the map starts at an ambient colour, each light
/// adds to it, and drawing it multiplies the screen by the result, so a light brings what it falls on back to its own
/// colours but never beyond them (use `Style::additive` sprites for a glow). Per frame: `clear`, `add` or `add_world`
/// for each light, then `draw` once, after the world and before the HUD.
///
/// The map is made on the CPU and drawn as one multiplied sprite, so the software and GPU renderers show the same
/// picture.
///
/// ```
/// use engine::{Gfx, Light, LightMap};
///
/// fn lit(gfx: &mut Gfx) {
///     let mut map = LightMap::new(gfx.w, gfx.h);
///     map.clear(0x202020);
///     map.add(Light::new((80.0, 60.0), 48.0));
///     map.draw(gfx);
/// }
/// ```
pub struct LightMap {
    w: usize,
    h: usize,
    sprite: Arc<Sprite>,
    px: Vec<u32>,
}

impl LightMap {
    /// A map of the size of the canvas (`Config::w` by `h`), all dark.
    pub fn new(w: usize, h: usize) -> Self {
        let (w, h) = (w.max(1), h.max(1));
        LightMap { w, h, sprite: Arc::new(Sprite::new(w, h, vec![0xFF00_0000; w * h])), px: vec![0xFF00_0000; w * h] }
    }

    /// Starts a frame: everything at the ambient colour (0xRRGGBB; black is full darkness, white no darkness).
    pub fn clear(&mut self, ambient: u32) {
        self.px.fill(0xFF00_0000 | ambient & 0xFF_FFFF);
    }

    /// Adds a light at screen pixels. A radius that is zero, negative or not a number adds nothing.
    pub fn add(&mut self, light: Light) {
        let Light { x, y, radius, color, intensity } = light;
        if !(radius > 0.0 && x.is_finite() && y.is_finite() && radius.is_finite() && intensity > 0.0) {
            return;
        }
        let (x0, x1) = ((x - radius).floor().max(0.0) as usize, ((x + radius).ceil().max(0.0) as usize).min(self.w));
        let (y0, y1) = ((y - radius).floor().max(0.0) as usize, ((y + radius).ceil().max(0.0) as usize).min(self.h));
        let (cr, cg, cb) = ((color >> 16 & 255) as f32, (color >> 8 & 255) as f32, (color & 255) as f32);
        let r2 = radius * radius;
        // `+ 0.5` and truncation round a non-negative value; `round` is a library call without SSE4.1.
        let lit = |now: u32, c: f32, gain: f32| (((now & 255) as i32 as f32 + c * gain + 0.5).min(255.0) as i32) as u32;
        for py in y0..y1 {
            let dy = py as f32 + 0.5 - y;
            // Only the columns the circle reaches on this row, give or take a pixel (those add nothing).
            let half = (r2 - dy * dy).max(0.0).sqrt();
            let (a, b) = ((x - half).floor().max(x0 as f32) as usize, ((x + half).ceil() as usize + 1).min(x1));
            if a >= b {
                continue;
            }
            let row = &mut self.px[py * self.w..][a..b];
            for (px, at) in (a..b).zip(row) {
                let dx = px as f32 + 0.5 - x;
                // No branch, so the loop vectorizes: outside the circle the gain is 0 and the pixel stays as it was.
                let falloff = (1.0 - (dx * dx + dy * dy) / r2).max(0.0);
                let gain = falloff * falloff * intensity;
                let p = *at;
                *at = p & 0xFF00_0000 | lit(p >> 16, cr, gain) << 16 | lit(p >> 8, cg, gain) << 8 | lit(p, cb, gain);
            }
        }
    }

    /// Adds a light at a place in the world, as `gfx` sees the world through its camera and zoom.
    pub fn add_world(&mut self, gfx: &Gfx, light: Light) {
        let (z, cam) = (gfx.zoom, gfx.cam);
        self.add(Light { x: (light.x - cam.x) * z, y: (light.y - cam.y) * z, radius: light.radius * z, ..light });
    }

    /// Multiplies the picture by the map. Call once per `clear`.
    pub fn draw(&mut self, gfx: &mut Gfx) {
        self.sprite.swap_pixels(&mut self.px);
        gfx.blit_styled(&self.sprite, 0, 0, Style::PLAIN.multiplied());
    }

    /// The colour of the map at a pixel as last drawn (0xRRGGBB), to see what a light does.
    pub fn at(&self, x: usize, y: usize) -> u32 {
        if x >= self.w {
            return 0;
        }
        self.sprite.image().px.get(y * self.w + x).map_or(0, |p| p & 0xFF_FFFF)
    }
}

/// A picture to multiply over the whole screen that darkens the corners: `strength` 0 does nothing and 1 makes the
/// corners black. Make it once and draw it each frame with `Gfx::multiply`.
///
/// ```
/// let v = engine::vignette(32, 32, 0.5);
/// assert_eq!(v.size(), (32, 32));
/// ```
pub fn vignette(w: usize, h: usize, strength: f32) -> Arc<Sprite> {
    let (w, h) = (w.max(1), h.max(1));
    let strength = if strength.is_nan() { 0.0 } else { strength.clamp(0.0, 1.0) };
    let px = (0..w * h).map(|i| {
        let (nx, ny) = (((i % w) as f32 + 0.5) / w as f32 * 2.0 - 1.0, ((i / w) as f32 + 0.5) / h as f32 * 2.0 - 1.0);
        let r = ((nx * nx + ny * ny) / 2.0).sqrt();
        let t = ((r - 0.35) / 0.65).clamp(0.0, 1.0);
        let gain = 1.0 - strength * t * t * (3.0 - 2.0 * t);
        let v = (gain * 255.0).round() as u32;
        0xFF00_0000 | v << 16 | v << 8 | v
    });
    Arc::new(Sprite::new(w, h, px.collect()))
}

/// A picture to multiply over the whole screen that darkens every second row by `dark` out of 255.
///
/// ```
/// let s = engine::scanlines(32, 32, 40);
/// assert_eq!(s.size(), (32, 32));
/// ```
pub fn scanlines(w: usize, h: usize, dark: u8) -> Arc<Sprite> {
    let (w, h) = (w.max(1), h.max(1));
    let line = 0xFF00_0000 | ((255 - dark as u32) * 0x01_0101);
    let px = (0..w * h).map(|i| if (i / w) % 2 == 1 { line } else { 0xFFFF_FFFF });
    Arc::new(Sprite::new(w, h, px.collect()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::Soft;
    use crate::{Blend, Rect};

    // The plain per-pixel loop; the fast `add` must give the same bits.
    fn reference(px: &mut [u32], w: usize, h: usize, l: Light) {
        let (cr, cg, cb) = ((l.color >> 16 & 255) as f32, (l.color >> 8 & 255) as f32, (l.color & 255) as f32);
        for (i, at) in px.iter_mut().enumerate().take(w * h) {
            let (dx, dy) = ((i % w) as f32 + 0.5 - l.x, (i / w) as f32 + 0.5 - l.y);
            let d2 = dx * dx + dy * dy;
            if d2 >= l.radius * l.radius {
                continue;
            }
            let falloff = 1.0 - d2 / (l.radius * l.radius);
            let gain = falloff * falloff * l.intensity;
            let ch =
                |shift: u32, c: f32| ((((*at >> shift) & 255) as f32 + c * gain).round().min(255.0) as u32) << shift;
            *at = *at & 0xFF00_0000 | ch(16, cr) | ch(8, cg) | ch(0, cb);
        }
    }

    #[test]
    fn lights_match_the_plain_per_pixel_loop_bit_for_bit() {
        let mut rng = crate::Rng::new(7);
        let (w, h) = (97, 61);
        let mut map = LightMap::new(w, h);
        map.clear(0x102030);
        let mut want = map.px.clone();
        for _ in 0..40 {
            let light = Light::new((rng.range(-30.0, 130.0), rng.range(-30.0, 90.0)), rng.range(0.3, 50.0))
                .colored(rng.below(0x100_0000) as u32)
                .intensity(rng.range(0.1, 3.0));
            map.add(light);
            reference(&mut want, w, h, light);
        }
        assert!(map.px == want, "a pixel differs from the plain loop");
    }

    #[test]
    fn a_light_brings_the_ambient_up_to_full_at_its_centre_and_fades_to_nothing_at_its_edge() {
        let mut map = LightMap::new(100, 100);
        map.clear(0x202020);
        map.add(Light::new((50.0, 50.0), 40.0));
        map.draw(&mut Gfx::new(100, 100));
        assert_eq!(map.at(49, 49) & 255, 255, "full at the centre");
        assert_eq!(map.at(10, 50) & 255, 0x20, "ambient at the edge");
        assert_eq!(map.at(0, 0), 0x202020, "and far away");
        let mid = map.at(50 + 20, 50) & 255;
        assert!(mid > 0x20 && mid < 255, "between: {mid}");
        assert!(map.at(50 + 10, 50) & 255 > mid, "stronger nearer the centre");
    }

    #[test]
    fn a_coloured_light_adds_only_its_channels_and_lights_stack_up_to_white() {
        let mut map = LightMap::new(64, 64);
        map.clear(0);
        map.add(Light::new((32.0, 32.0), 30.0).colored(0xff4000));
        map.draw(&mut Gfx::new(64, 64));
        let c = map.at(31, 31);
        assert!(c >> 16 == 255 && (c >> 8 & 255) > 50 && (c & 255) == 0, "{c:06x}");
        map.clear(0x808080);
        for _ in 0..4 {
            map.add(Light::new((32.0, 32.0), 30.0));
        }
        map.draw(&mut Gfx::new(64, 64));
        assert_eq!(map.at(31, 31), 0xffffff, "saturates instead of wrapping");
    }

    #[test]
    fn a_light_off_the_edge_a_dud_or_a_weak_one_is_harmless() {
        let mut map = LightMap::new(32, 32);
        map.clear(0x101010);
        for l in [
            Light::new((-500.0, 10.0), 20.0),
            Light::new((16.0, 16.0), 0.0),
            Light::new((16.0, 16.0), -5.0),
            Light::new((16.0, 16.0), f32::NAN),
            Light::new((f32::NAN, 16.0), 10.0),
            Light::new((16.0, f32::INFINITY), 10.0),
            Light::new((16.0, 16.0), 10.0).intensity(0.0),
            Light::new((16.0, 16.0), f32::INFINITY),
        ] {
            map.add(l);
        }
        map.add(Light::new((0.0, 0.0), 20.0));
        map.draw(&mut Gfx::new(32, 32));
        assert_eq!(map.at(16, 16), 0x101010, "only the corner light did anything");
        assert!(map.at(0, 0) > 0x101010);
    }

    #[test]
    fn a_world_light_follows_the_camera_and_the_zoom() {
        let mut g = Gfx::new(200, 100);
        (g.cam, g.zoom) = (Vec2::new(100.0, 20.0), 2.0);
        let mut map = LightMap::new(200, 100);
        map.clear(0);
        map.add_world(&g, Light::new((130.0, 50.0), 20.0));
        map.draw(&mut Gfx::new(200, 100));
        assert!(map.at(59, 59) & 255 > 200, "at ((130 - 100) * 2, (50 - 20) * 2): {:06x}", map.at(59, 59));
        assert_eq!(map.at(59 + 45, 59), 0, "the radius doubled to 40, so 45 away is dark");
        assert!(map.at(59 + 35, 59) > 0);
    }

    #[test]
    fn drawing_the_map_puts_one_multiplied_full_screen_command_and_each_frame_changes_the_same_sprite() {
        let mut map = LightMap::new(40, 30);
        let mut g = Gfx::new(40, 30);
        map.clear(0x404040);
        map.draw(&mut g);
        assert_eq!(g.cmds().len(), 1);
        let c = &g.cmds()[0];
        assert_eq!((c.style.blend, c.rect), (Blend::Multiply, Rect::new(0.0, 0.0, 40.0, 30.0)));
        let version = map.sprite.version();
        map.clear(0x808080);
        map.draw(&mut g);
        assert_eq!(map.sprite.version(), version + 1, "the GPU uploads it again");
        assert_eq!(map.at(5, 5), 0x808080);
    }

    #[test]
    fn through_the_software_renderer_a_lit_scene_is_dark_except_where_the_light_falls() {
        let mut g = Gfx::new(100, 100);
        g.clear(0xffffff);
        let mut map = LightMap::new(100, 100);
        map.clear(0x404040);
        map.add(Light::new((50.0, 50.0), 30.0));
        map.draw(&mut g);
        let mut soft = Soft::new(100, 100, 1);
        soft.render(g.cmds());
        let at = |x: usize, y: usize| soft.buf[y * 100 + x] & 0xFF_FFFF;
        assert_eq!(at(5, 5), 0x404040, "the white floor is dark");
        assert!(at(49, 49) >= 0xfdfdfd, "and bright under the light: {:06x}", at(49, 49));
    }

    #[test]
    fn the_vignette_is_clear_in_the_middle_dark_in_the_corners_and_nothing_at_zero_strength() {
        let v = vignette(64, 36, 1.0);
        let px = |x: usize, y: usize| v.image().px[y * 64 + x] & 255;
        assert_eq!(px(32, 18), 255);
        assert!(px(0, 0) < 10, "{}", px(0, 0));
        assert!(px(0, 18) > px(0, 0) && px(0, 18) < 255);
        let none = vignette(64, 36, 0.0);
        assert!(none.image().px.iter().all(|&p| p == 0xFFFF_FFFF));
        assert!(vignette(8, 8, f32::NAN).image().px.iter().all(|&p| p == 0xFFFF_FFFF));
    }

    #[test]
    fn scanlines_darken_every_second_row_only() {
        let s = scanlines(4, 4, 60);
        let px = |y: usize| s.image().px[y * 4] & 0xFF_FFFF;
        assert_eq!((px(0), px(1), px(2), px(3)), (0xffffff, 0xc3c3c3, 0xffffff, 0xc3c3c3));
    }
}
