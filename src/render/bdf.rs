use crate::base::in_file;
use crate::{Rect, Sprite};
use std::{ops::Range, sync::Arc};

struct Glyph {
    src: Rect,
    xoff: i32,
    yoff: i32,
    advance: i32,
}

/// A bitmap font read from a BDF file. Glyphs are packed once into a white-on-transparent atlas that
/// text is tinted from, so any colour costs nothing extra and the GPU can batch a whole string.
///
/// ```
/// use engine::Font;
///
/// let font = Font::load("assets/fonts/galmuri9.bdf").unwrap();
/// assert!(font.width("ab") > 0);
/// assert!(font.line_height() > 0);
/// ```
pub struct Font {
    pub(crate) atlas: Arc<Sprite>,
    // Sorted by character, so a lookup is a binary search; hashing 11k glyphs at load cost more than it saved.
    glyphs: Vec<(char, Glyph)>,
    line: i32,
}

const ATLAS_W: usize = 1024;

impl Font {
    /// Reads a BDF file.
    pub fn load(path: &str) -> Result<Font, String> {
        let text = crate::base::read_text(path)?;
        Font::parse(&text).map_err(in_file(path))
    }

    /// Reads BDF text.
    pub fn parse(text: &str) -> Result<Font, String> {
        struct Raw {
            ch: char,
            advance: i32,
            bbx: [i32; 4],
            rows: Range<usize>,
        }
        let (mut ascent, mut descent) = (0, 0);
        let (mut raws, mut cur, mut in_bitmap) = (Vec::<Raw>::new(), None::<Raw>, false);
        // Every glyph's rows live in one buffer, so a glyph costs no allocation of its own.
        let mut bits = Vec::<u32>::new();
        // Up to the first four numbers of a line and how many there were.
        let nums = |v: &str| {
            let (mut out, mut n) = ([0; 4], 0);
            for tok in v.split_whitespace() {
                let x = tok.parse::<i32>().map_err(|e| format!("{v:?}: {e}"))?;
                if n < 4 {
                    out[n] = x;
                }
                n += 1;
            }
            Ok::<_, String>((out, n))
        };
        for line in text.lines() {
            // Bitmap rows are most of the file, so they skip the keyword match.
            if in_bitmap && !line.starts_with("ENDCHAR") {
                if let Some(g) = &mut cur {
                    let bytes = line.trim();
                    if bytes.len() > 8 {
                        return Err(format!("bitmap row {bytes:?}: wider than 32 pixels"));
                    }
                    let row = u32::from_str_radix(bytes, 16).map_err(|e| format!("bitmap row {bytes:?}: {e}"))?;
                    bits.push(row << (32 - 4 * bytes.len()));
                    g.rows.end = bits.len();
                }
                continue;
            }
            let (key, val) = line.split_once(' ').unwrap_or((line, ""));
            match key {
                "FONT_ASCENT" => ascent = val.trim().parse().map_err(|e| format!("FONT_ASCENT: {e}"))?,
                "FONT_DESCENT" => descent = val.trim().parse().map_err(|e| format!("FONT_DESCENT: {e}"))?,
                "ENCODING" => {
                    let code: i64 =
                        val.split_whitespace().next().unwrap_or("").parse().map_err(|e| format!("ENCODING: {e}"))?;
                    let ch = u32::try_from(code).ok().and_then(char::from_u32);
                    cur = ch.map(|ch| Raw { ch, advance: 0, bbx: [0; 4], rows: bits.len()..bits.len() });
                    in_bitmap = false;
                }
                "DWIDTH" => {
                    if let Some(g) = &mut cur {
                        let (first, n) = nums(val)?;
                        if n == 0 {
                            return Err("DWIDTH: empty".into());
                        }
                        g.advance = first[0];
                    }
                }
                "BBX" => {
                    if let Some(g) = &mut cur {
                        let (bbx, n) = nums(val)?;
                        if n != 4 {
                            return Err(format!("BBX {val:?}: expected 4 numbers"));
                        }
                        // A bitmap row is at most 32 pixels, and a negative size has no pixels to draw.
                        if !(0..=32).contains(&bbx[0]) || bbx[1] < 0 {
                            return Err(format!("BBX {val:?}: the width must be 0 to 32 and the height not negative"));
                        }
                        g.bbx = bbx;
                    }
                }
                "BITMAP" => in_bitmap = true,
                "ENDCHAR" => {
                    in_bitmap = false;
                    if let Some(g) = &cur
                        && g.rows.len() as i32 != g.bbx[1]
                    {
                        return Err(format!(
                            "U+{:04X}: {} bitmap rows for a height of {}",
                            g.ch as u32,
                            g.rows.len(),
                            g.bbx[1]
                        ));
                    }
                    raws.extend(cur.take());
                }
                _ => {}
            }
        }
        if raws.is_empty() || ascent + descent <= 0 {
            return Err("no glyphs or no FONT_ASCENT/FONT_DESCENT".into());
        }
        // A glyph taller than the font box would write past its atlas row.
        let tallest = raws.iter().map(|g| g.bbx[1]).max().unwrap_or(0);
        let (mut x, mut y, row_h) = (0, 0, (ascent + descent).max(tallest) as usize);
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
        let mut glyphs = Vec::with_capacity(raws.len());
        for (g, &(gx, gy)) in raws.iter().zip(&placed) {
            let [w, h, xoff, yoff] = g.bbx;
            for (r, row) in bits[g.rows.clone()].iter().enumerate() {
                let dst = &mut px[(gy + r) * ATLAS_W + gx..][..w as usize];
                for (c, p) in dst.iter_mut().enumerate() {
                    *p = 0u32.wrapping_sub(row >> (31 - c) & 1);
                }
            }
            let src = Rect::new(gx as f32, gy as f32, w as f32, h as f32);
            glyphs.push((g.ch, Glyph { src, xoff, yoff: ascent - yoff - h, advance: g.advance }));
        }
        // A character listed twice keeps its later entry.
        glyphs.reverse();
        glyphs.sort_by_key(|g| g.0);
        glyphs.dedup_by_key(|g| g.0);
        Ok(Font { atlas: Arc::new(Sprite::new(ATLAS_W, height, px)), glyphs, line: ascent + descent })
    }

    /// Height of a line in pixels.
    pub fn line_height(&self) -> i32 {
        self.line
    }

    /// Width of `s` in pixels.
    pub fn width(&self, s: &str) -> i32 {
        s.chars().filter_map(|c| self.glyph(c)).map(|g| g.advance).sum()
    }

    // A character the font lacks shows as `?` (if it has one) rather than silently vanishing.
    fn glyph(&self, c: char) -> Option<&Glyph> {
        let find = |c| self.glyphs.binary_search_by_key(&c, |g| g.0).ok().map(|i| &self.glyphs[i].1);
        find(c).or_else(|| find('?'))
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
    fn glyph_boxes_a_row_cannot_hold_are_errors_not_panics() {
        for bbx in ["-1 1 0 0", "33 1 0 0", "2000 1 0 0", "1 -1 0 0"] {
            let text =
                format!("FONT_ASCENT 1\nFONT_DESCENT 0\nENCODING 65\nDWIDTH 1 0\nBBX {bbx}\nBITMAP\n80\nENDCHAR\n");
            let err = Font::parse(&text).err().unwrap_or_else(|| panic!("BBX {bbx} was accepted"));
            assert!(err.contains("BBX"), "{bbx}: {err}");
        }
        let ok = "FONT_ASCENT 1\nFONT_DESCENT 0\nENCODING 65\nDWIDTH 32 0\nBBX 32 1 0 0\nBITMAP\n80000000\nENDCHAR\n";
        assert!(Font::parse(ok).is_ok(), "32 pixels is the widest a row holds");
    }

    #[test]
    fn a_bad_file_is_an_error_not_a_panic() {
        assert!(Font::parse("garbage").is_err());
        assert!(Font::parse("FONT_ASCENT 1\nFONT_DESCENT 0\nENCODING 65\nBBX 1 1 0 0\nBITMAP\nZZ\nENDCHAR\n").is_err());
    }

    #[test]
    fn a_tall_glyph_and_an_unencoded_glyph_with_an_index_load() {
        let tall = "FONT_ASCENT 1\nFONT_DESCENT 0\nENCODING 65\nDWIDTH 1 0\nBBX 1 3 0 0\nBITMAP\n80\n80\n80\nENDCHAR\n";
        assert!(Font::parse(tall).is_ok());
        let none = "FONT_ASCENT 1\nFONT_DESCENT 0\nENCODING -1 5\nDWIDTH 1 0\nBBX 1 1 0 0\nBITMAP\n80\nENDCHAR\n";
        let text = format!("{none}ENCODING 65\nDWIDTH 1 0\nBBX 1 1 0 0\nBITMAP\n80\nENDCHAR\n");
        assert_eq!(Font::parse(&text).unwrap().width("A"), 1);
    }

    #[test]
    fn a_glyph_whose_rows_do_not_match_its_height_is_an_error() {
        let bad = "FONT_ASCENT 1\nFONT_DESCENT 0\nENCODING 65\nDWIDTH 1 0\nBBX 1 2 0 0\nBITMAP\n80\nENDCHAR\n";
        assert!(Font::parse(bad).err().unwrap().contains("U+0041"));
    }
}
