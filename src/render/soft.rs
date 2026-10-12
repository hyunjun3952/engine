use super::cmd::SAMPLE_BIAS;
use super::{DrawCmd, Paint, Transform, font};
use crate::{Rect, Sprite};
use std::ops::Range;

pub(crate) struct Soft {
    w: usize,
    h: usize,
    scale: usize,
    pub(crate) buf: Vec<u32>,
    scaled: Vec<u32>,
    // Scratch for `scaled_blit`: the source column of each destination column and the source row of each row.
    cols: Vec<u32>,
    rows: Vec<u32>,
}

impl Soft {
    pub(crate) fn new(w: usize, h: usize, scale: usize) -> Self {
        let scaled = if scale > 1 { vec![0; w * h * scale * scale] } else { Vec::new() };
        Soft { w, h, scale, buf: vec![0; w * h], scaled, cols: Vec::new(), rows: Vec::new() }
    }

    pub(crate) fn present(&mut self) -> &[u32] {
        if self.scale == 1 {
            return &self.buf;
        }
        upscale(&self.buf, self.w, self.scale, &mut self.scaled);
        &self.scaled
    }

    pub(crate) fn render(&mut self, cmds: &[DrawCmd]) {
        self.buf.fill(0);
        for c in cmds {
            let r = &c.rect;
            if !c.xform.is_identity() && self.transformed(c) {
                continue;
            }
            match &c.paint {
                Paint::Clear(col) => self.buf.fill(*col),
                Paint::Color(col) => self.fill_with(r, |_| *col),
                Paint::Alpha(col, a) => self.fill_alpha(r, *col, *a as u32),
                Paint::Sprite(s) => self.blit_region(s, s.bounds(), r, false),
                Paint::Region { sheet, src, flip } => self.blit_region(sheet, *src, r, *flip),
                Paint::Mask { sheet, src, color } => self.mask(sheet, *src, r, *color),
                Paint::Outline(col) => self.outline(r, *col),
                Paint::Glyph { ch, color, scale } => self.glyph(r, *ch, *color, *scale),
                Paint::Tiles { sheet, size, tiles } => {
                    for &(gx, gy, id) in tiles {
                        let at = Rect { x: r.x + (gx * size) as f32, y: r.y + (gy * size) as f32, ..*r };
                        self.blit_region(sheet, Paint::tile_src(id, *size), &at, false);
                    }
                }
            }
        }
    }

    fn transformed(&mut self, c: &DrawCmd) -> bool {
        let (r, t) = (&c.rect, &c.xform);
        match &c.paint {
            Paint::Color(col) => {
                let col = *col;
                self.warp(*r, t, |d, _, _| *d = col);
            }
            Paint::Alpha(col, a) => {
                let (col, a) = (*col, *a as u32);
                self.warp(*r, t, |d, _, _| *d = blend(col, *d, a));
            }
            _ => {
                let Some((sheet, src, flip)) = c.paint.region() else { return false };
                let img = sheet.image();
                if !img.holds(&src) {
                    return true;
                }
                let (sx0, sy0, w, h) = (src.x as usize, src.y as usize, src.w as usize, src.h as usize);
                let quad = Rect { x: r.x, y: r.y, w: src.w, h: src.h };
                if t.angle == 0.0 && t.sx != 0.0 && t.sy != 0.0 {
                    self.scaled_blit(&img, src, flip, quad, t);
                    return true;
                }
                self.warp(quad, t, |d, u, v| {
                    let u = if flip { src.w - u } else { u } + SAMPLE_BIAS;
                    let (u, v) = ((u as usize).min(w - 1), ((v + SAMPLE_BIAS) as usize).min(h - 1));
                    put(d, img.px[(sy0 + v) * img.w + sx0 + u]);
                });
            }
        }
        true
    }

    // A sprite scaled about its centre without turning, the common case of a zoom. It picks the same texel for every
    // pixel as `warp` does, but works out the column of each destination column and the row of each row once, so the
    // loop over pixels is only a table look-up and a copy.
    fn scaled_blit(&mut self, img: &super::gfx::Image, src: Rect, flip: bool, quad: Rect, t: &Transform) {
        const NONE: u32 = u32::MAX;
        let (cx, cy) = quad.center();
        let (hw, hh) = (quad.w / 2.0 * t.sx.abs(), quad.h / 2.0 * t.sy.abs());
        let (x0, x1) = ((cx - hw).floor().max(0.0) as usize, ((cx + hw).ceil().max(0.0) as usize).min(self.w));
        let (y0, y1) = ((cy - hh).floor().max(0.0) as usize, ((cy + hh).ceil().max(0.0) as usize).min(self.h));
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        let (sx0, sy0, w, h) = (src.x as usize, src.y as usize, src.w as usize, src.h as usize);
        let (iu, iv) = (1.0 / t.sx, 1.0 / t.sy);
        let (mut cols, mut rows) = (std::mem::take(&mut self.cols), std::mem::take(&mut self.rows));
        cols.clear();
        rows.clear();
        cols.extend((x0..x1).map(|px| {
            let u = (px as f32 + 0.5 - cx) * iu + quad.w / 2.0;
            if u < 0.0 || u >= quad.w {
                return NONE;
            }
            let u = if flip { src.w - u } else { u } + SAMPLE_BIAS;
            (u as usize).min(w - 1) as u32
        }));
        rows.extend((y0..y1).map(|py| {
            let v = (py as f32 + 0.5 - cy) * iv + quad.h / 2.0;
            if v < 0.0 || v >= quad.h { NONE } else { ((v + SAMPLE_BIAS) as usize).min(h - 1) as u32 }
        }));
        for (py, &v) in (y0..y1).zip(&rows) {
            if v == NONE {
                continue;
            }
            let from = (sy0 + v as usize) * img.w + sx0;
            let (line, dst) = (&img.px[from..from + w], &mut self.buf[py * self.w + x0..py * self.w + x1]);
            for (d, &u) in dst.iter_mut().zip(&cols) {
                if u != NONE {
                    put(d, line[u as usize]);
                }
            }
        }
        (self.cols, self.rows) = (cols, rows);
    }

    fn warp(&mut self, quad: Rect, t: &Transform, mut shade: impl FnMut(&mut u32, f32, f32)) {
        if t.sx == 0.0 || t.sy == 0.0 {
            return;
        }
        let (cx, cy) = quad.center();
        let (hw, hh) = (quad.w / 2.0 * t.sx.abs(), quad.h / 2.0 * t.sy.abs());
        let (s, c) = t.angle.sin_cos();
        let (ex, ey) = (hw * c.abs() + hh * s.abs(), hw * s.abs() + hh * c.abs());
        let (x0, x1) = ((cx - ex).floor().max(0.0) as usize, ((cx + ex).ceil().max(0.0) as usize).min(self.w));
        let (y0, y1) = ((cy - ey).floor().max(0.0) as usize, ((cy + ey).ceil().max(0.0) as usize).min(self.h));
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        let (iu, iv) = (1.0 / t.sx, 1.0 / t.sy);
        let (du, dv) = (c * iu, -s * iv);
        for py in y0..y1 {
            let (dx, dy) = (x0 as f32 + 0.5 - cx, py as f32 + 0.5 - cy);
            let mut u = (dx * c + dy * s) * iu + quad.w / 2.0;
            let mut v = (dy * c - dx * s) * iv + quad.h / 2.0;
            for d in &mut self.buf[py * self.w + x0..py * self.w + x1] {
                if u >= 0.0 && u < quad.w && v >= 0.0 && v < quad.h {
                    shade(d, u, v);
                }
                u += du;
                v += dv;
            }
        }
    }

    fn clip(&self, r: &Rect) -> (usize, usize, usize, usize) {
        let (x0, y0) = (r.x.max(0.0) as usize, r.y.max(0.0) as usize);
        let (x1, y1) = (((r.x + r.w) as usize).min(self.w), ((r.y + r.h) as usize).min(self.h));
        (x0, y0, x1.max(x0), y1)
    }

    // The columns and rows of a `w` by `h` picture drawn at (`x`, `y`) that land inside the buffer.
    fn visible(&self, x: i32, y: i32, w: usize, h: usize) -> (Range<usize>, Range<usize>) {
        let cols = (-x).max(0) as usize..((self.w as i32 - x).max(0) as usize).min(w);
        let rows = (-y).max(0) as usize..((self.h as i32 - y).max(0) as usize).min(h);
        (cols, rows)
    }

    fn fill_with(&mut self, r: &Rect, f: impl Fn(u32) -> u32) {
        let (x0, y0, x1, y1) = self.clip(r);
        for y in y0..y1 {
            for px in &mut self.buf[y * self.w + x0..y * self.w + x1] {
                *px = f(*px);
            }
        }
    }

    fn fill_alpha(&mut self, r: &Rect, color: u32, a: u32) {
        let (x0, y0, x1, y1) = self.clip(r);
        for y in y0..y1 {
            blend_row(&mut self.buf[y * self.w + x0..y * self.w + x1], color, a);
        }
    }

    fn glyph(&mut self, r: &Rect, ch: u8, color: u32, scale: u32) {
        let s = scale as f32;
        for (row, bits) in font::bits(ch).into_iter().enumerate() {
            let mut col = 0;
            while col < 8 {
                if bits >> col & 1 == 0 {
                    col += 1;
                    continue;
                }
                let start = col;
                while col < 8 && bits >> col & 1 == 1 {
                    col += 1;
                }
                let run =
                    Rect { x: r.x + start as f32 * s, y: r.y + row as f32 * s, w: (col - start) as f32 * s, h: s };
                self.fill_with(&run, |_| color);
            }
        }
    }

    fn outline(&mut self, r: &Rect, c: u32) {
        for edge in r.edges() {
            self.fill_with(&edge, |_| c);
        }
    }

    // The destination rect is the source scaled by a whole number, so each source pixel covers k by k.
    fn mask(&mut self, s: &Sprite, src: Rect, at: &Rect, color: u32) {
        let img = s.image();
        if !img.holds(&src) {
            return;
        }
        let (x, y) = (at.x as i32, at.y as i32);
        let (sx0, sy0, w, h) = (src.x as usize, src.y as usize, src.w as usize, src.h as usize);
        let k = ((at.w / src.w) as usize).max(1);
        let (cols, rows) = self.visible(x, y, w * k, h * k);
        for dy in rows {
            let from = (sy0 + dy / k) * img.w + sx0;
            let dst_from = (y + dy as i32) as usize * self.w;
            for dx in cols.clone() {
                put(
                    &mut self.buf[dst_from + (x + dx as i32) as usize],
                    img.px[from + dx / k] & 0xFF00_0000 | color & 0xFF_FFFF,
                );
            }
        }
    }

    fn blit_region(&mut self, s: &Sprite, src: Rect, at: &Rect, flip: bool) {
        let img = s.image();
        if !img.holds(&src) {
            return;
        }
        let (x, y) = (at.x as i32, at.y as i32);
        let (sx0, sy0, w, h) = (src.x as usize, src.y as usize, src.w as usize, src.h as usize);
        let (cols, rows) = self.visible(x, y, w, h);
        for sy in rows {
            let from = (sy0 + sy) * img.w + sx0;
            let src_row = &img.px[from..from + w];
            let dst_from = (y + sy as i32) as usize * self.w;
            let dst_row = &mut self.buf[dst_from..dst_from + self.w];
            for sx in cols.clone() {
                let p = src_row[if flip { w - 1 - sx } else { sx }];
                put(&mut dst_row[(x + sx as i32) as usize], p);
            }
        }
    }
}

#[inline(always)]
fn put(d: &mut u32, p: u32) {
    match p >> 24 {
        255 => *d = p,
        0 => {}
        a => *d = blend(p, *d, a),
    }
}

#[inline(always)]
fn blend_row_plain(row: &mut [u32], src: u32, a: u32) {
    for px in row {
        *px = blend(src, *px, a);
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
fn blend_row_avx2(row: &mut [u32], src: u32, a: u32) {
    blend_row_plain(row, src, a);
}

fn blend_row(row: &mut [u32], src: u32, a: u32) {
    #[cfg(target_arch = "x86_64")]
    if std::is_x86_feature_detected!("avx2") {
        // SAFETY: the CPU just reported AVX2 support.
        return unsafe { blend_row_avx2(row, src, a) };
    }
    blend_row_plain(row, src, a);
}

fn upscale(src: &[u32], w: usize, scale: usize, dst: &mut [u32]) {
    let out_w = w * scale;
    for (y, row) in src.chunks_exact(w).enumerate() {
        let first = y * scale * out_w;
        for (px, run) in row.iter().zip(dst[first..first + out_w].chunks_exact_mut(scale)) {
            run.fill(*px);
        }
        for k in 1..scale {
            dst.copy_within(first..first + out_w, first + k * out_w);
        }
    }
}

#[inline(always)]
fn blend(src: u32, dst: u32, a: u32) -> u32 {
    let a = a + (a >> 7);
    let inv = 256 - a;
    let rb = ((src & 0x00ff_00ff) * a + (dst & 0x00ff_00ff) * inv) >> 8 & 0x00ff_00ff;
    let g = ((src & 0x0000_ff00) * a + (dst & 0x0000_ff00) * inv) >> 8 & 0x0000_ff00;
    rb | g
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DrawCmd, Gfx};

    fn run(w: usize, h: usize, f: impl FnOnce(&mut Gfx)) -> Soft {
        let (mut g, mut s) = (Gfx::new(w, h), Soft::new(w, h, 1));
        f(&mut g);
        s.render(g.cmds());
        s
    }

    #[test]
    fn scaled_font_text_covers_k_by_k_per_source_pixel_and_clips_at_the_edge() {
        let font = crate::Font::load("assets/fonts/galmuri9.bdf").unwrap();
        let lit = |s: &Soft| s.buf.iter().filter(|&&p| p & 0xFF_FFFF == 0xFF_FFFF).count();
        let one = run(60, 40, |g| g.text_font(&font, "ㅁ", 2, 2, 0xffffff));
        let two = run(60, 40, |g| g.text_font_scaled(&font, "ㅁ", 2, 2, 2, 0xffffff));
        assert!(lit(&one) > 0 && lit(&two) == lit(&one) * 4);
        run(8, 8, |g| g.text_font_scaled(&font, "한", -5, -5, 3, 1));
        run(8, 8, |g| g.text_font_scaled(&font, "한", 5, 5, 3, 1));
    }

    #[test]
    fn blit_clips_at_every_edge_and_flip_mirrors_columns() {
        let sprite = Sprite::test_ball();
        let s = run(16, 16, |g| g.blit(&sprite, -8, -8));
        assert_eq!((s.buf[0], s.buf[8 * 16 + 8]), (sprite.image().px[8 * 16 + 8], 0));
        let s = run(16, 16, |g| g.blit_region(&sprite, Rect { x: 0.0, y: 0.0, w: 16.0, h: 16.0 }, 0, 0, true));
        assert_eq!(s.buf[8 * 16 + 3], sprite.image().px[8 * 16 + 12]);
        run(16, 16, |g| {
            for (x, y) in [(-100, 5), (100, 100), (5, -100), (15, 15), (-15, -15)] {
                g.blit(&sprite, x, y);
            }
        });
    }

    #[test]
    fn text_lights_exactly_the_glyph_bits_scaled() {
        let lit: u32 = font::bits(b'A').iter().map(|b| b.count_ones()).sum();
        let s = run(40, 40, |g| g.text("A", 3, 5, 2, 7));
        assert_eq!(s.buf.iter().filter(|&&p| p == 7).count() as u32, lit * 4);
        let s = run(8, 8, |g| g.text("A", -4, -4, 1, 7));
        assert!(s.buf.contains(&7));
    }

    #[test]
    fn blend_stays_within_one_of_the_exact_division_and_is_exact_at_the_ends() {
        let colors = [0x000000, 0xffffff, 0x102030, 0xff0080, 0x80ff00, 0x0080ff, 0xdeadbe];
        for &src in &colors {
            for &dst in &colors {
                assert_eq!((blend(src, dst, 0), blend(src, dst, 255)), (dst, src));
                for a in 0..=255u32 {
                    let got = blend(src, dst, a);
                    for sh in [16, 8, 0] {
                        let (s, d) = ((src >> sh & 255) as i32, (dst >> sh & 255) as i32);
                        let exact = (s * a as i32 + d * (255 - a as i32)) / 255;
                        assert!(((got >> sh & 255) as i32 - exact).abs() <= 1, "src {src:x} dst {dst:x} a {a}");
                    }
                    assert_eq!(got >> 24, 0, "the alpha byte stays clear");
                }
            }
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn the_avx2_path_gives_exactly_the_same_pixels_as_the_plain_one() {
        if !std::is_x86_feature_detected!("avx2") {
            eprintln!("no AVX2, skipping");
            return;
        }
        let base: Vec<u32> = (0..4099u32).map(|i| i.wrapping_mul(2654435761) & 0x00ff_ffff).collect();
        for (src, a) in
            [(0x336699, 150), (0xffffff, 1), (0x000000, 254), (0xdeadbe, 128), (0x123456, 0), (0x123456, 255)]
        {
            let (mut plain, mut fast) = (base.clone(), base.clone());
            blend_row_plain(&mut plain, src, a);
            unsafe { blend_row_avx2(&mut fast, src, a) };
            assert!(plain == fast, "src {src:x} a {a}");
        }
    }

    fn drawn(w: usize, h: usize, rect: Rect, paint: Paint, xform: Transform) -> Soft {
        run(w, h, |g| g.draw([DrawCmd { layer: 0, rect, paint, debug_only: false, xform }]))
    }

    fn lit(s: &Soft, w: usize) -> Vec<(usize, usize)> {
        s.buf.iter().enumerate().filter(|(_, p)| **p != 0).map(|(i, _)| (i % w, i / w)).collect()
    }

    #[test]
    fn a_source_rect_that_is_not_inside_its_sheet_draws_nothing_instead_of_panicking() {
        // What hot reload leaves behind when a sheet is replaced by a smaller picture.
        let sheet = std::sync::Arc::new(Sprite::from_pixels(4, 4, vec![0xFFFF_FFFF; 16]));
        let at = Rect { x: 2.0, y: 2.0, w: 8.0, h: 8.0 };
        for src in [
            Rect { x: 8.0, y: 0.0, w: 4.0, h: 4.0 },
            Rect { x: 2.0, y: 2.0, w: 4.0, h: 4.0 },
            Rect { x: -1.0, y: 0.0, w: 4.0, h: 4.0 },
        ] {
            for xform in [
                Transform::IDENTITY,
                Transform { angle: 0.0, sx: 2.0, sy: 2.0 },
                Transform { angle: 0.6, sx: 1.0, sy: 1.0 },
            ] {
                let region = Paint::Region { sheet: sheet.clone(), src, flip: false };
                assert!(drawn(32, 32, at, region, xform).buf.iter().all(|&p| p == 0), "{src:?} {xform:?}");
            }
            let mask = Paint::Mask { sheet: sheet.clone(), src, color: 0xffffff };
            assert!(drawn(32, 32, at, mask, Transform::IDENTITY).buf.iter().all(|&p| p == 0), "mask {src:?}");
        }
        let inside = Paint::Region { sheet, src: Rect { x: 0.0, y: 0.0, w: 4.0, h: 4.0 }, flip: false };
        assert!(
            drawn(32, 32, at, inside, Transform::IDENTITY).buf.iter().any(|&p| p != 0),
            "a rect inside still draws"
        );
    }

    #[test]
    fn a_turned_or_scaled_sprite_beyond_the_right_or_bottom_edge_draws_nothing_and_does_not_panic() {
        let sheet = std::sync::Arc::new(Sprite::from_pixels(4, 4, vec![0xFFFF_FFFF; 16]));
        for (x, y) in [(60.0, 30.0), (47.5, 40.0), (54.0, 40.0), (100.0, 100.0)] {
            for angle in [0.0, 0.7] {
                let at = Rect { x, y, w: 4.0, h: 4.0 };
                let s = drawn(48, 36, at, Paint::Sprite(sheet.clone()), Transform { angle, sx: 2.0, sy: 2.0 });
                assert!(s.buf.iter().all(|&p| p == 0), "nothing is on the screen at ({x}, {y}), angle {angle}");
            }
        }
    }

    #[test]
    fn the_unturned_scaling_path_picks_the_same_texel_as_the_general_one_for_every_pixel() {
        // An angle of 1e-30 is zero as far as the maths goes but sends the sprite down the general path.
        let mut rng = crate::Rng::new(5);
        let texels: Vec<u32> = (0..7 * 5).map(|i| 0xFF00_0000 | (i as u32 * 0x0003_1713 + 1)).collect();
        let sheet = std::sync::Arc::new(Sprite::from_pixels(7, 5, texels));
        for case in 0..300 {
            let (sx, sy) =
                (rng.range(0.3, 5.0) * [1.0, -1.0][case % 2], rng.range(0.3, 5.0) * [1.0, -1.0][case / 2 % 2]);
            let rect = Rect { x: rng.range(-30.0, 50.0), y: rng.range(-30.0, 40.0), w: 7.0, h: 5.0 };
            let flip = case % 3 == 0;
            let paint = Paint::Region { sheet: sheet.clone(), src: Rect { x: 1.0, y: 0.0, w: 5.0, h: 5.0 }, flip };
            let fast = drawn(48, 36, rect, paint.clone(), Transform { angle: 0.0, sx, sy });
            let slow = drawn(48, 36, rect, paint, Transform { angle: 1e-30, sx, sy });
            assert!(fast.buf == slow.buf, "case {case}: scale {sx} x {sy} at {rect:?}, flip {flip}");
        }
    }

    #[test]
    fn rotating_a_rect_a_quarter_turn_swaps_its_width_and_height_around_the_centre() {
        let quarter = Transform { angle: std::f32::consts::FRAC_PI_2, ..Transform::IDENTITY };
        let s = drawn(8, 8, Rect { x: 2.0, y: 3.0, w: 4.0, h: 2.0 }, Paint::Color(7), quarter);
        let want: Vec<(usize, usize)> = (2..6).flat_map(|y| (3..5).map(move |x| (x, y))).collect();
        assert_eq!(lit(&s, 8), want);
    }

    #[test]
    fn scaling_grows_around_the_centre_and_a_zero_scale_draws_nothing() {
        let double = Transform { sx: 2.0, sy: 2.0, ..Transform::IDENTITY };
        let s = drawn(8, 8, Rect { x: 3.0, y: 3.0, w: 2.0, h: 2.0 }, Paint::Color(7), double);
        let want: Vec<(usize, usize)> = (2..6).flat_map(|y| (2..6).map(move |x| (x, y))).collect();
        assert_eq!(lit(&s, 8), want);
        let none = Transform { sx: 0.0, ..Transform::IDENTITY };
        assert!(
            drawn(8, 8, Rect { x: 3.0, y: 3.0, w: 2.0, h: 2.0 }, Paint::Color(7), none).buf.iter().all(|&p| p == 0)
        );
    }

    #[test]
    fn a_square_turned_45_degrees_keeps_its_area() {
        let eighth = Transform { angle: std::f32::consts::FRAC_PI_4, ..Transform::IDENTITY };
        let s = drawn(48, 48, Rect { x: 14.0, y: 14.0, w: 20.0, h: 20.0 }, Paint::Color(7), eighth);
        let area = lit(&s, 48).len();
        assert!((380..=420).contains(&area), "{area}");
    }

    #[test]
    fn a_half_turn_flips_a_sprite_both_ways_and_a_negative_scale_mirrors_like_flip() {
        let sprite = Sprite::test_ball();
        let img = sprite.image();
        let at = Rect { x: 0.0, y: 0.0, w: 16.0, h: 16.0 };
        let half = Transform { angle: std::f32::consts::PI, ..Transform::IDENTITY };
        let s = drawn(16, 16, at, Paint::Sprite(sprite.clone()), half);
        for y in 0..16 {
            for x in 0..16 {
                let p = img.px[(15 - y) * 16 + 15 - x];
                assert_eq!(s.buf[y * 16 + x], if p >> 24 == 255 { p } else { 0 }, "({x},{y})");
            }
        }
        let region =
            |flip| Paint::Region { sheet: sprite.clone(), src: Rect { x: 2.0, y: 0.0, w: 10.0, h: 16.0 }, flip };
        let flipped = drawn(16, 16, at, region(true), Transform::IDENTITY);
        let mirrored = drawn(16, 16, at, region(false), Transform { sx: -1.0, ..Transform::IDENTITY });
        assert!(flipped.buf == mirrored.buf);
    }

    #[test]
    fn upscaling_repeats_every_pixel_scale_times_in_both_directions() {
        let mut s = Soft::new(3, 2, 3);
        s.buf = vec![1, 2, 3, 4, 5, 6];
        let out = s.present().to_vec();
        assert_eq!(out.len(), 9 * 6);
        let row = |y: usize| &out[y * 9..y * 9 + 9];
        assert_eq!(row(0), [1, 1, 1, 2, 2, 2, 3, 3, 3]);
        assert_eq!(row(0), row(1));
        assert_eq!(row(1), row(2));
        assert_eq!(row(3), [4, 4, 4, 5, 5, 5, 6, 6, 6]);
        assert_eq!(row(5), row(3));
        let mut one = Soft::new(3, 2, 1);
        one.buf = vec![1, 2, 3, 4, 5, 6];
        assert_eq!(one.present(), [1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn fill_alpha_blends_with_what_is_beneath() {
        let all = Rect { x: 0.0, y: 0.0, w: 2.0, h: 1.0 };
        let s = run(2, 1, |g| g.fill_alpha(&all, 0xffffff, 128));
        assert_eq!(s.buf[0], 0x808080);
        let s = run(2, 1, |g| {
            g.fill(&all, 0x808080);
            g.fill_alpha(&all, 0x000000, 0);
        });
        assert_eq!(s.buf[1], 0x808080);
        let s = run(2, 1, |g| g.fill_alpha(&all, 0x102030, 255));
        assert_eq!(s.buf[0], 0x102030);
    }

    #[test]
    fn outline_leaves_interior_untouched_and_clear_fills_everything() {
        let s = run(10, 10, |g| g.outline(&Rect { x: 2.0, y: 2.0, w: 5.0, h: 5.0 }, 1));
        assert_eq!((s.buf[2 * 10 + 2], s.buf[6 * 10 + 6], s.buf[4 * 10 + 4]), (1, 1, 0));
        let s = run(4, 4, |g| g.clear(9));
        assert!(s.buf.iter().all(|&p| p == 9));
    }
}
