use super::cmd::SAMPLE_BIAS;
use super::{Blend, DrawCmd, Paint, Style, Transform, font};
use crate::{Rect, Sprite, Vec2};
use std::ops::Range;

mod blend;
mod blit;
mod fill;
#[cfg(test)]
mod tests;

use blend::*;
use fill::*;

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
                Paint::Color(col) if c.style.is_plain() => self.fill_with(r, |_| *col),
                Paint::Alpha(col, a) if c.style.is_plain() => self.fill_alpha(r, *col, *a as u32),
                Paint::Color(_) | Paint::Alpha(..) => self.fill_styled(c),
                Paint::Sprite(s) => self.sprite(c, s, s.bounds(), false),
                Paint::Region { sheet, src, flip } => self.sprite(c, sheet, *src, *flip),
                Paint::Mask { sheet, src, color } => self.mask(sheet, *src, r, *color),
                Paint::Outline(col) => self.outline(r, *col),
                Paint::Glyph { ch, color, scale } => self.glyph(r, *ch, *color, *scale),
                Paint::Tiles { sheet, size, tiles } => {
                    let sheet_w = sheet.size().0;
                    for &(gx, gy, id) in tiles {
                        let at = Rect { x: r.x + (gx * size) as f32, y: r.y + (gy * size) as f32, ..*r };
                        self.blit_region(sheet, Paint::tile_src(id, *size, sheet_w), &at, false, put);
                    }
                }
            }
        }
    }

    // The part of a rectangle inside the buffer as (x0, y0, x1, y1), with x0 <= x1 <= width and y0 <= y1 <= height
    // whatever the rectangle is (far off screen, backwards, not a number), so that slicing a row with it cannot fail.
    fn clip(&self, r: &Rect) -> (usize, usize, usize, usize) {
        let (x0, y0) = ((r.x.max(0.0) as usize).min(self.w), (r.y.max(0.0) as usize).min(self.h));
        let (x1, y1) = (((r.x + r.w) as usize).min(self.w).max(x0), ((r.y + r.h) as usize).min(self.h).max(y0));
        (x0, y0, x1, y1)
    }

    // The columns and rows of a `w` by `h` picture drawn at (`x`, `y`) that land inside the buffer.
    fn visible(&self, x: i32, y: i32, w: usize, h: usize) -> (Range<usize>, Range<usize>) {
        // In i64 so that a position saturated to i32::MIN or MAX (from infinity) cannot overflow.
        let span = |at: i32, len: usize, limit: usize| {
            let at = at as i64;
            (-at).max(0) as usize..((limit as i64 - at).max(0) as usize).min(len)
        };
        (span(x, w, self.w), span(y, h, self.h))
    }
}
