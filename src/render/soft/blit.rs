use super::{
    Blend, DrawCmd, Paint, Rect, SAMPLE_BIAS, Soft, Sprite, Transform, Vec2, add_flat, blend, flat, mul_flat, put,
    put_with,
};

impl Soft {
    // A sprite or region at its place, shown with the command's style.
    pub(super) fn sprite(&mut self, c: &DrawCmd, sheet: &Sprite, src: Rect, flip: bool) {
        if c.style.is_plain() {
            self.blit_region(sheet, src, &c.rect, flip, put);
        } else {
            let st = c.style;
            self.blit_region(sheet, src, &c.rect, flip, move |d, p| put_with(d, p, &st));
        }
    }

    pub(super) fn transformed(&mut self, c: &DrawCmd) -> bool {
        let (r, t) = (&c.rect, &c.xform);
        if matches!(c.paint, Paint::Color(_) | Paint::Alpha(..)) {
            if c.style.is_plain() {
                match c.paint {
                    Paint::Color(col) => self.warp(*r, t, |d, _, _| *d = col),
                    Paint::Alpha(col, a) => self.warp(*r, t, |d, _, _| *d = blend(col, *d, a as u32)),
                    _ => {}
                }
            } else if let Some((col, a, mode)) = flat(c) {
                match (mode, a) {
                    (Blend::Alpha, 255) => self.warp(*r, t, |d, _, _| *d = col),
                    (Blend::Alpha, _) => self.warp(*r, t, |d, _, _| *d = blend(col, *d, a)),
                    (Blend::Add, _) => self.warp(*r, t, |d, _, _| *d = add_flat(*d, col, a)),
                    (Blend::Multiply, _) => self.warp(*r, t, |d, _, _| *d = mul_flat(*d, col, a)),
                }
            }
            return true;
        }
        if c.style.is_plain() {
            self.transformed_sprite(c, put)
        } else {
            let st = c.style;
            self.transformed_sprite(c, move |d, p| put_with(d, p, &st))
        }
    }

    pub(super) fn transformed_sprite(&mut self, c: &DrawCmd, px: impl Fn(&mut u32, u32) + Copy) -> bool {
        let (r, t) = (&c.rect, &c.xform);
        let Some((sheet, src, flip)) = c.paint.region() else { return false };
        let img = sheet.image();
        if !img.holds(&src) {
            return true;
        }
        let (sx0, sy0, w, h) = (src.x as usize, src.y as usize, src.w as usize, src.h as usize);
        let quad = Rect::new(r.x, r.y, src.w, src.h);
        if t.angle == 0.0 && t.sx != 0.0 && t.sy != 0.0 {
            self.scaled_blit(&img, src, flip, quad, t, px);
            return true;
        }
        self.warp(quad, t, |d, u, v| {
            let u = if flip { src.w - u } else { u } + SAMPLE_BIAS;
            // Through i32: a float to usize cast is a long sequence on x86_64, and u and v are small and not negative here.
            let (u, v) = ((u as i32 as usize).min(w - 1), ((v + SAMPLE_BIAS) as i32 as usize).min(h - 1));
            px(d, img.px[(sy0 + v) * img.w + sx0 + u]);
        });
        true
    }

    // Scaling without turning: source columns and rows are looked up from two tables, 3.6x faster than `warp`.
    pub(super) fn scaled_blit(
        &mut self,
        img: &crate::base::Image,
        src: Rect,
        flip: bool,
        quad: Rect,
        t: &Transform,
        px: impl Fn(&mut u32, u32),
    ) {
        const NONE: u32 = u32::MAX;
        let Vec2 { x: cx, y: cy } = quad.center();
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
                    px(d, line[u as usize]);
                }
            }
        }
        (self.cols, self.rows) = (cols, rows);
    }

    pub(super) fn warp(&mut self, quad: Rect, t: &Transform, mut shade: impl FnMut(&mut u32, f32, f32)) {
        if t.sx == 0.0 || t.sy == 0.0 {
            return;
        }
        let Vec2 { x: cx, y: cy } = quad.center();
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
        let n = x1 - x0;
        let (ru, rv) = (du.recip(), dv.recip());
        for py in y0..y1 {
            let (dx, dy) = (x0 as f32 + 0.5 - cx, py as f32 + 0.5 - cy);
            let (u0, v0) = ((dx * c + dy * s) * iu + quad.w / 2.0, (dy * c - dx * s) * iv + quad.h / 2.0);
            // Only the stretch of the row that can lie inside the quad is walked; the test below stays exact.
            let ((lo_u, hi_u), (lo_v, hi_v)) = (inside(u0, du, ru, quad.w, n), inside(v0, dv, rv, quad.h, n));
            let (lo, hi) = (lo_u.max(lo_v), hi_u.min(hi_v));
            if lo >= hi {
                continue;
            }
            let mut step = lo as f32;
            for d in &mut self.buf[py * self.w + x0 + lo..py * self.w + x0 + hi] {
                let (u, v) = (u0 + du * step, v0 + dv * step);
                step += 1.0;
                if u >= 0.0 && u < quad.w && v >= 0.0 && v < quad.h {
                    shade(d, u, v);
                }
            }
        }
    }

    pub(super) fn blit_region(&mut self, s: &Sprite, src: Rect, at: &Rect, flip: bool, px: impl Fn(&mut u32, u32)) {
        let img = s.image();
        if !img.holds(&src) {
            return;
        }
        let (x, y) = (at.x.floor() as i32, at.y.floor() as i32);
        let (sx0, sy0, w, h) = (src.x as usize, src.y as usize, src.w as usize, src.h as usize);
        let (cols, rows) = self.visible(x, y, w, h);
        for sy in rows {
            let from = (sy0 + sy) * img.w + sx0;
            let src_row = &img.px[from..from + w];
            let dst_from = (y + sy as i32) as usize * self.w;
            let dst_row = &mut self.buf[dst_from..dst_from + self.w];
            for sx in cols.clone() {
                let p = src_row[if flip { w - 1 - sx } else { sx }];
                px(&mut dst_row[(x + sx as i32) as usize], p);
            }
        }
    }
}

// The steps `0..n` at which `o + step * d` can lie in `0..len` (`inv` is `1 / d`), widened so rounding never loses one.
pub(super) fn inside(o: f32, d: f32, inv: f32, len: f32, n: usize) -> (usize, usize) {
    if d == 0.0 {
        return if o >= 0.0 && o < len { (0, n) } else { (0, 0) };
    }
    let (a, b) = (-o * inv, (len - o) * inv);
    // Truncating stands in for floor and ceil (a library call without SSE4.1): the slack of a pixel each way covers it.
    let (lo, hi) = ((a.min(b) - 1.0).max(0.0) as usize, ((a.max(b) + 2.0).max(0.0) as usize).min(n));
    (lo.min(hi), hi)
}
