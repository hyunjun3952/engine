use super::{Blend, DrawCmd, Paint, Rect, Soft, Sprite, add_flat, blend, blend_row, font, mul_flat, put};

impl Soft {
    // A flat colour with a style: its flash and fade folded into one colour and alpha, laid or added.
    pub(super) fn fill_styled(&mut self, c: &DrawCmd) {
        let Some((col, a, blend)) = flat(c) else { return };
        match (blend, a) {
            (Blend::Alpha, 255) => self.fill_with(&c.rect, |_| col),
            (Blend::Alpha, _) => self.fill_alpha(&c.rect, col, a),
            (Blend::Add, _) => self.fill_with(&c.rect, |d| add_flat(d, col, a)),
            (Blend::Multiply, _) => self.fill_with(&c.rect, |d| mul_flat(d, col, a)),
        }
    }

    pub(super) fn fill_with(&mut self, r: &Rect, f: impl Fn(u32) -> u32) {
        let (x0, y0, x1, y1) = self.clip(r);
        for y in y0..y1 {
            for px in &mut self.buf[y * self.w + x0..y * self.w + x1] {
                *px = f(*px);
            }
        }
    }

    pub(super) fn fill_alpha(&mut self, r: &Rect, color: u32, a: u32) {
        let (x0, y0, x1, y1) = self.clip(r);
        for y in y0..y1 {
            blend_row(&mut self.buf[y * self.w + x0..y * self.w + x1], color, a);
        }
    }

    pub(super) fn glyph(&mut self, r: &Rect, ch: u8, color: u32, scale: u32) {
        let s = scale as f32;
        for (row, bits) in font::glyph_rows(ch).into_iter().enumerate() {
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
                let run = Rect::new(r.x + start as f32 * s, r.y + row as f32 * s, (col - start) as f32 * s, s);
                self.fill_with(&run, |_| color);
            }
        }
    }

    pub(super) fn outline(&mut self, r: &Rect, c: u32) {
        for edge in r.edges() {
            self.fill_with(&edge, |_| c);
        }
    }

    // The destination rect is the source scaled by a whole number, so each source pixel covers k by k.
    pub(super) fn mask(&mut self, s: &Sprite, src: Rect, at: &Rect, color: u32) {
        let img = s.image();
        if !img.contains_rect(&src) {
            return;
        }
        let (x, y) = (at.x.floor() as i32, at.y.floor() as i32);
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
}

// The colour (0xRRGGBB), alpha and blend a flat paint shows with once its style is applied.
pub(super) fn flat(c: &DrawCmd) -> Option<(u32, u32, Blend)> {
    let (col, a) = match c.paint {
        Paint::Color(col) => (col & 0xFF_FFFF, 255),
        Paint::Alpha(col, a) => (col & 0xFF_FFFF, a as u32),
        _ => return None,
    };
    let st = &c.style;
    let col = if st.flash_amount == 0 { col } else { blend(st.flash, col, st.flash_amount as u32) };
    Some((col, (a * st.opacity as u32 + 127) / 255, st.blend))
}
