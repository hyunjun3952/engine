use crate::{Rect, Sprite};
use std::sync::Arc;

/// Nudge towards the next texel so both backends break a pixel centre on a texel border the same way.
pub(crate) const SAMPLE_BIAS: f32 = 1.0 / 256.0;

#[derive(Clone)]
pub enum Paint {
    Clear(u32),
    Color(u32),
    Alpha(u32, u8),
    Sprite(Arc<Sprite>),
    Region { sheet: Arc<Sprite>, src: Rect, flip: bool },
    Mask { sheet: Arc<Sprite>, src: Rect, color: u32 },
    Outline(u32),
    Glyph { ch: u8, color: u32, scale: u32 },
    Tiles { sheet: Arc<Sprite>, size: u32, tiles: Vec<(u32, u32, u8)> },
}

impl Paint {
    /// Where tile `id` is on a sheet `sheet_w` pixels wide made of `size` pixel squares: ids count along the first
    /// row, then the next (the first tile is 1).
    pub(crate) fn tile_src(id: u8, size: u32, sheet_w: usize) -> Rect {
        let columns = (sheet_w as u32 / size).max(1);
        let n = id as u32 - 1;
        Rect { x: (n % columns * size) as f32, y: (n / columns * size) as f32, w: size as f32, h: size as f32 }
    }

    /// The sheet, source area and mirroring of a paint that copies pixels from a sprite as they are.
    pub(crate) fn region(&self) -> Option<(&Arc<Sprite>, Rect, bool)> {
        match self {
            Paint::Sprite(s) => Some((s, s.bounds(), false)),
            Paint::Region { sheet, src, flip } => Some((sheet, *src, *flip)),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    pub angle: f32,
    pub sx: f32,
    pub sy: f32,
}

impl Transform {
    pub const IDENTITY: Transform = Transform { angle: 0.0, sx: 1.0, sy: 1.0 };

    pub fn is_identity(&self) -> bool {
        *self == Transform::IDENTITY
    }
}

impl Default for Transform {
    fn default() -> Self {
        Transform::IDENTITY
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blend {
    /// The picture is laid over what is behind it, by its alpha.
    Alpha,
    /// The picture's colour, scaled by its alpha, is added to what is behind it and stops at white: glows, fire.
    Add,
    /// The picture's colour, mixed to white by its transparency, multiplies what is behind it: shadows, tints, dark
    /// overlays and the light maps of `LightMap`. White leaves the picture as it is, black makes it black.
    Multiply,
}

/// How a sprite or a flat colour is shown: a flash towards a colour (a hit), a fade, and how it joins the picture
/// behind. `Sprite`, `Region`, `Color` and `Alpha` paints follow it; the others (text, tiles, outlines) ignore it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Style {
    /// The colour (0xRRGGBB) the pixels are mixed towards, by `flash_amount` out of 255.
    pub flash: u32,
    pub flash_amount: u8,
    /// 255 shows everything as it is, 0 nothing.
    pub opacity: u8,
    pub blend: Blend,
}

impl Style {
    pub const PLAIN: Style = Style { flash: 0, flash_amount: 0, opacity: 255, blend: Blend::Alpha };

    pub fn is_plain(&self) -> bool {
        *self == Style::PLAIN
    }

    /// Mixes towards `color` by `amount` out of 255, e.g. `Style::PLAIN.flashed(0xffffff, 200)` for a white hit flash.
    pub fn flashed(self, color: u32, amount: u8) -> Style {
        Style { flash: color & 0xFF_FFFF, flash_amount: amount, ..self }
    }

    pub fn faded(self, opacity: u8) -> Style {
        Style { opacity, ..self }
    }

    pub fn additive(self) -> Style {
        Style { blend: Blend::Add, ..self }
    }

    pub fn multiplied(self) -> Style {
        Style { blend: Blend::Multiply, ..self }
    }
}

impl Default for Style {
    fn default() -> Self {
        Style::PLAIN
    }
}

#[derive(Clone)]
pub struct DrawCmd {
    pub layer: i32,
    pub rect: Rect,
    pub paint: Paint,
    pub debug_only: bool,
    pub xform: Transform,
    pub style: Style,
}

impl DrawCmd {
    /// A command on layer 0, always drawn, without rotation or scale, shown plainly.
    pub fn new(rect: Rect, paint: Paint) -> Self {
        DrawCmd { layer: 0, rect, paint, debug_only: false, xform: Transform::IDENTITY, style: Style::PLAIN }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_ids_start_at_one_and_walk_along_the_sheet() {
        assert_eq!(Paint::tile_src(1, 16, 64), Rect { x: 0.0, y: 0.0, w: 16.0, h: 16.0 });
        assert_eq!(Paint::tile_src(4, 8, 32), Rect { x: 24.0, y: 0.0, w: 8.0, h: 8.0 });
    }

    #[test]
    fn ids_wrap_onto_the_next_row_of_a_taller_sheet() {
        assert_eq!(Paint::tile_src(5, 8, 32), Rect { x: 0.0, y: 8.0, w: 8.0, h: 8.0 });
        assert_eq!(Paint::tile_src(7, 8, 32), Rect { x: 16.0, y: 8.0, w: 8.0, h: 8.0 });
        assert_eq!(
            Paint::tile_src(3, 8, 4),
            Rect { x: 0.0, y: 16.0, w: 8.0, h: 8.0 },
            "a sheet narrower than a tile is one column"
        );
    }
}
