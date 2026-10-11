use super::{DrawCmd, Paint, font};
use crate::{Rect, Sprite};

pub(crate) struct Soft {
    w: usize,
    h: usize,
    scale: usize,
    pub(crate) buf: Vec<u32>,
    scaled: Vec<u32>,
}

impl Soft {
    pub(crate) fn new(w: usize, h: usize, scale: usize) -> Self {
        let scaled = if scale > 1 { vec![0; w * h * scale * scale] } else { Vec::new() };
        Soft { w, h, scale, buf: vec![0; w * h], scaled }
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
            match &c.paint {
                Paint::Clear(col) => self.buf.fill(*col),
                Paint::Color(col) => self.fill_with(r, |_| *col),
                Paint::Alpha(col, a) => self.fill_with(r, |d| blend(*col, d, *a as u32)),
                Paint::Sprite(s) => {
                    let (w, h) = s.size();
                    self.blit_region(s, Rect { x: 0.0, y: 0.0, w: w as f32, h: h as f32 }, r, false)
                }
                Paint::Region { sheet, src, flip } => self.blit_region(sheet, *src, r, *flip),
                Paint::Outline(col) => self.outline(r, *col),
                Paint::Glyph { ch, color, scale } => self.glyph(r, *ch, *color, *scale),
                Paint::Tiles { sheet, size, tiles } => {
                    for &(gx, gy, id) in tiles {
                        let at = Rect { x: r.x + (gx * size) as f32, y: r.y + (gy * size) as f32, ..*r };
                        let src = Rect { x: ((id as u32 - 1) * size) as f32, y: 0.0, w: *size as f32, h: *size as f32 };
                        self.blit_region(sheet, src, &at, false);
                    }
                }
            }
        }
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
        self.fill_with(&Rect { h: 1.0, ..*r }, |_| c);
        self.fill_with(&Rect { y: r.y + r.h - 1.0, h: 1.0, ..*r }, |_| c);
        self.fill_with(&Rect { w: 1.0, ..*r }, |_| c);
        self.fill_with(&Rect { x: r.x + r.w - 1.0, w: 1.0, ..*r }, |_| c);
    }

    fn blit_region(&mut self, s: &Sprite, src: Rect, at: &Rect, flip: bool) {
        let img = s.image();
        let (x, y) = (at.x as i32, at.y as i32);
        let (sx0, sy0, w, h) = (src.x as usize, src.y as usize, src.w as usize, src.h as usize);
        let cols = (-x).max(0) as usize..((self.w as i32 - x).max(0) as usize).min(w);
        let rows = (-y).max(0) as usize..((self.h as i32 - y).max(0) as usize).min(h);
        for sy in rows {
            let from = (sy0 + sy) * img.w + sx0;
            let src_row = &img.px[from..from + w];
            let dst_from = (y + sy as i32) as usize * self.w;
            let dst_row = &mut self.buf[dst_from..dst_from + self.w];
            for sx in cols.clone() {
                let p = src_row[if flip { w - 1 - sx } else { sx }];
                let d = &mut dst_row[(x + sx as i32) as usize];
                match p >> 24 {
                    255 => *d = p,
                    0 => {}
                    a => *d = blend(p, *d, a),
                }
            }
        }
    }
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

fn blend(src: u32, dst: u32, a: u32) -> u32 {
    let mix = |sh: u32| ((((src >> sh) & 255) * a + ((dst >> sh) & 255) * (255 - a)) / 255) << sh;
    mix(16) | mix(8) | mix(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Gfx;
    use std::sync::Arc;

    fn run(w: usize, h: usize, f: impl FnOnce(&mut Gfx)) -> Soft {
        let (mut g, mut s) = (Gfx::new(w, h), Soft::new(w, h, 1));
        f(&mut g);
        s.render(g.cmds());
        s
    }

    #[test]
    fn blit_clips_at_every_edge_and_flip_mirrors_columns() {
        let sprite = Arc::new(Sprite::load("assets/ball.png"));
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
