use crate::{Rect, Sprite};
use std::{collections::HashMap, sync::Arc};

struct Glyph {
    src: Rect,
    xoff: i32,
    yoff: i32,
    advance: i32,
}

/// A bitmap font read from a BDF file. Glyphs are packed once into a white-on-transparent atlas that
/// text is tinted from, so any colour costs nothing extra and the GPU can batch a whole string.
pub struct Font {
    pub(crate) atlas: Arc<Sprite>,
    glyphs: HashMap<char, Glyph>,
    line: i32,
}

const ATLAS_W: usize = 1024;

impl Font {
    pub fn load(path: &str) -> Result<Font, String> {
        let text = crate::assets::read_text(path)?;
        Font::parse(&text).map_err(|e| format!("{path}: {e}"))
    }

    pub fn parse(text: &str) -> Result<Font, String> {
        struct Raw {
            ch: char,
            advance: i32,
            bbx: [i32; 4],
            rows: Vec<u32>,
        }
        let (mut ascent, mut descent) = (0, 0);
        let (mut raws, mut cur, mut in_bitmap) = (Vec::<Raw>::new(), None::<Raw>, false);
        let nums = |v: &str| {
            v.split_whitespace()
                .map(|n| n.parse::<i32>().map_err(|e| format!("{v:?}: {e}")))
                .collect::<Result<Vec<_>, _>>()
        };
        for line in text.lines() {
            let (key, val) = line.split_once(' ').unwrap_or((line, ""));
            match key {
                "FONT_ASCENT" => ascent = val.trim().parse().map_err(|e| format!("FONT_ASCENT: {e}"))?,
                "FONT_DESCENT" => descent = val.trim().parse().map_err(|e| format!("FONT_DESCENT: {e}"))?,
                "ENCODING" => {
                    let code: i64 = val.trim().parse().map_err(|e| format!("ENCODING: {e}"))?;
                    let ch = u32::try_from(code).ok().and_then(char::from_u32);
                    cur = ch.map(|ch| Raw { ch, advance: 0, bbx: [0; 4], rows: Vec::new() });
                    in_bitmap = false;
                }
                "DWIDTH" => {
                    if let Some(g) = &mut cur {
                        g.advance = *nums(val)?.first().ok_or("DWIDTH: empty")?;
                    }
                }
                "BBX" => {
                    if let Some(g) = &mut cur {
                        g.bbx = nums(val)?.try_into().map_err(|_| format!("BBX {val:?}: expected 4 numbers"))?;
                    }
                }
                "BITMAP" => in_bitmap = true,
                "ENDCHAR" => raws.extend(cur.take().filter(|g| g.rows.len() as i32 == g.bbx[1])),
                _ if in_bitmap => {
                    if let Some(g) = &mut cur {
                        let bytes = line.trim();
                        if bytes.len() > 8 {
                            return Err(format!("bitmap row {bytes:?}: wider than 32 pixels"));
                        }
                        let row = u32::from_str_radix(bytes, 16).map_err(|e| format!("bitmap row {bytes:?}: {e}"))?;
                        g.rows.push(row << (32 - 4 * bytes.len()));
                    }
                }
                _ => {}
            }
        }
        if raws.is_empty() || ascent + descent <= 0 {
            return Err("no glyphs or no FONT_ASCENT/FONT_DESCENT".into());
        }
        let (mut x, mut y, row_h) = (0, 0, (ascent + descent) as usize);
        let mut placed = Vec::new();
        for g in &raws {
            let w = g.bbx[0] as usize;
            if x + w > ATLAS_W {
                (x, y) = (0, y + row_h);
            }
            placed.push((x, y));
            x += w;
        }
        let height = (y + row_h).max(1);
        let mut px = vec![0u32; ATLAS_W * height];
        let mut glyphs = HashMap::new();
        for (g, &(gx, gy)) in raws.iter().zip(&placed) {
            let [w, h, xoff, yoff] = g.bbx;
            for (r, row) in g.rows.iter().enumerate() {
                for c in 0..w as usize {
                    if row >> (31 - c) & 1 == 1 {
                        px[(gy + r) * ATLAS_W + gx + c] = 0xFFFF_FFFF;
                    }
                }
            }
            let src = Rect { x: gx as f32, y: gy as f32, w: w as f32, h: h as f32 };
            glyphs.insert(g.ch, Glyph { src, xoff, yoff: ascent - yoff - h, advance: g.advance });
        }
        Ok(Font { atlas: Arc::new(Sprite::from_pixels(ATLAS_W, height, px)), glyphs, line: ascent + descent })
    }

    pub fn line_height(&self) -> i32 {
        self.line
    }

    pub fn width(&self, s: &str) -> i32 {
        s.chars().filter_map(|c| self.glyph(c)).map(|g| g.advance).sum()
    }

    // A character the font lacks shows as `?` (if it has one) rather than silently vanishing.
    fn glyph(&self, c: char) -> Option<&Glyph> {
        self.glyphs.get(&c).or_else(|| self.glyphs.get(&'?'))
    }

    /// One `(atlas region, pen offset)` per drawable char; unknown chars draw nothing but still step nothing.
    pub(crate) fn layout<'a>(&'a self, s: &'a str) -> impl Iterator<Item = (Rect, i32, i32)> + 'a {
        let mut pen = 0;
        s.chars().filter_map(move |c| {
            let g = self.glyph(c)?;
            let at = pen + g.xoff;
            pen += g.advance;
            (g.src.w > 0.0).then_some((g.src, at, g.yoff))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TINY: &str = "FONT_ASCENT 2\nFONT_DESCENT 1\nENCODING 65\nDWIDTH 3 0\nBBX 2 2 0 0\nBITMAP\nC0\n40\nENDCHAR\nENCODING 32\nDWIDTH 2 0\nBBX 0 0 0 0\nBITMAP\nENDCHAR\n";

    #[test]
    fn glyph_bits_land_in_the_atlas_and_widths_add_up() {
        let f = Font::parse(TINY).unwrap();
        assert_eq!(f.line_height(), 3);
        assert_eq!(f.width("A A"), 8);
        let img = f.atlas.image();
        let on: Vec<usize> = (0..4).map(|i| (img.px[i % 2 + i / 2 * ATLAS_W] != 0) as usize).collect();
        assert_eq!(on, [1, 1, 0, 1]);
    }

    #[test]
    fn layout_skips_spaces_and_unknown_chars_and_places_by_pen() {
        let f = Font::parse(TINY).unwrap();
        let got: Vec<_> = f.layout("A?A").map(|(_, x, y)| (x, y)).collect();
        assert_eq!(got, [(0, 0), (3, 0)], "no `?` in the font, so the unknown char takes no room");
        assert_eq!(f.width("A?A"), 6);
    }

    #[test]
    fn the_bundled_font_has_hangul_and_ascii() {
        let f = Font::load("assets/fonts/galmuri9.bdf").unwrap();
        assert_eq!(f.layout("한글 abc 가힣").count(), 7);
        assert!(f.width("한글") > f.width("ab"));
        let all: String = ('\u{AC00}'..='\u{D7A3}').collect();
        assert_eq!(f.layout(&all).count(), 11172, "every syllable has a glyph, so none falls back to `?`");
    }

    #[test]
    fn a_bad_file_is_an_error_not_a_panic() {
        assert!(Font::parse("garbage").is_err());
        assert!(Font::parse("FONT_ASCENT 1\nFONT_DESCENT 0\nENCODING 65\nBBX 1 1 0 0\nBITMAP\nZZ\nENDCHAR\n").is_err());
    }
}
