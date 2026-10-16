use crate::{Rect, Sprite};
use std::sync::Arc;

/// Nudge towards the next texel so both backends break a pixel centre on a texel border the same way.
pub(crate) const SAMPLE_BIAS: f32 = 1.0 / 256.0;

/// What a [`DrawCmd`] puts in its rectangle. Colours are 0xRRGGBB.
///
/// ```
/// use engine::{DrawCmd, Paint, Rect};
///
/// let cmd = DrawCmd::new(Rect::new(0.0, 0.0, 8.0, 8.0), Paint::Color(0xff0000));
/// assert!(matches!(cmd.paint, Paint::Color(0xff0000)));
/// ```
#[derive(Clone)]
pub enum Paint {
    /// Fills the whole canvas.
    Clear(u32),
    /// A flat 0xRRGGBB colour.
    Color(u32),
    /// A flat colour with an alpha, 0xRRGGBB and 0 to 255.
    Alpha(u32, u8),
    /// An image, stretched to the rectangle.
    Sprite(Arc<Sprite>),
    /// Part of a sheet.
    Region {
        /// The image to copy from.
        sheet: Arc<Sprite>,
        /// The area of the sheet.
        src: Rect,
        /// Mirror it left to right.
        flip: bool,
    },
    /// Part of a sheet whose opaque pixels take one colour; how fonts are drawn.
    Mask {
        /// The image to copy from.
        sheet: Arc<Sprite>,
        /// The area of the sheet.
        src: Rect,
        /// The colour of the opaque pixels, 0xRRGGBB.
        color: u32,
    },
    /// A one pixel frame.
    Outline(u32),
    /// One character of the built-in 8x8 font.
    Glyph {
        /// The ASCII code.
        ch: u8,
        /// Colour, 0xRRGGBB.
        color: u32,
        /// Pixels per font pixel.
        scale: u32,
    },
    /// Many tiles of one sheet in one command.
    Tiles {
        /// The tile sheet.
        sheet: Arc<Sprite>,
        /// Side of a tile in pixels.
        size: u32,
        /// Each tile's x, y and id (counted from 1).
        tiles: Vec<(u32, u32, u8)>,
    },
}

impl Paint {
    /// Where tile `id` is on a sheet `sheet_w` pixels wide made of `size` pixel squares: ids count along the first
    /// row, then the next (the first tile is 1).
    pub(crate) fn tile_src(id: u8, size: u32, sheet_w: usize) -> Rect {
        let columns = (sheet_w as u32 / size).max(1);
        let n = id as u32 - 1;
        Rect::new((n % columns * size) as f32, (n / columns * size) as f32, size as f32, size as f32)
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

/// Turn and scale of a [`DrawCmd`]: clockwise radians and a scale per axis about the centre; negative mirrors.
///
/// ```
/// use engine::Transform;
///
/// let mirrored = Transform { sx: -1.0, ..Transform::IDENTITY };
/// assert_eq!(mirrored.sy, 1.0);
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    /// Clockwise turn in radians.
    pub angle: f32,
    /// Horizontal scale; negative mirrors.
    pub sx: f32,
    /// Vertical scale; negative mirrors.
    pub sy: f32,
}

impl Transform {
    /// No turn and a scale of one.
    pub const IDENTITY: Transform = Transform { angle: 0.0, sx: 1.0, sy: 1.0 };

    /// True if it turns and scales nothing.
    pub fn is_identity(&self) -> bool {
        *self == Transform::IDENTITY
    }
}

impl Default for Transform {
    fn default() -> Self {
        Transform::IDENTITY
    }
}

/// How a draw command is combined with what is behind it; set through [`Style`].
///
/// ```
/// use engine::{Blend, Style};
///
/// assert_eq!(Style::PLAIN.additive().blend, Blend::Add);
/// ```
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
///
/// ```
/// use engine::Style;
///
/// let hurt = Style::PLAIN.flashed(0xffffff, 200).faded(120);
/// assert!(!hurt.is_plain());
/// assert_eq!(hurt.opacity, 120);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Style {
    /// The colour (0xRRGGBB) the pixels are mixed towards, by `flash_amount` out of 255.
    pub flash: u32,
    /// How much of the flash colour is mixed in, 0 to 255.
    pub flash_amount: u8,
    /// 255 shows everything as it is, 0 nothing.
    pub opacity: u8,
    /// How the pixels combine with what is behind them.
    pub blend: Blend,
}

impl Style {
    /// No flash, fully opaque, normal blending.
    pub const PLAIN: Style = Style { flash: 0, flash_amount: 0, opacity: 255, blend: Blend::Alpha };

    /// True if it changes nothing.
    pub fn is_plain(&self) -> bool {
        *self == Style::PLAIN
    }

    /// Mixes towards `color` by `amount` out of 255, e.g. `Style::PLAIN.flashed(0xffffff, 200)` for a white hit flash.
    pub fn flashed(self, color: u32, amount: u8) -> Style {
        Style { flash: color & 0xFF_FFFF, flash_amount: amount, ..self }
    }

    /// Sets the opacity.
    pub fn faded(self, opacity: u8) -> Style {
        Style { opacity, ..self }
    }

    /// Adds the colour to what is behind, for glows.
    pub fn additive(self) -> Style {
        Style { blend: Blend::Add, ..self }
    }

    /// Multiplies what is behind by the colour, for shadows and light.
    pub fn multiplied(self) -> Style {
        Style { blend: Blend::Multiply, ..self }
    }
}

impl Default for Style {
    fn default() -> Self {
        Style::PLAIN
    }
}

/// One thing to draw: where (`rect`), what (`paint`), on which `layer`, turned or scaled by `xform` and shown with
/// `style`. Start from [`DrawCmd::new`] and set the rest with struct update syntax.
///
/// ```
/// use engine::{DrawCmd, Paint, Rect};
///
/// let mut cmd = DrawCmd::new(Rect::new(10.0, 10.0, 16.0, 16.0), Paint::Color(0x3070ff));
/// cmd.layer = 5;
/// assert_eq!(cmd.layer, 5);
/// ```
#[derive(Clone)]
pub struct DrawCmd {
    /// Draw order; higher layers are drawn later.
    pub layer: i32,
    /// Where and how big, in world pixels.
    pub rect: Rect,
    /// What fills the rectangle.
    pub paint: Paint,
    /// Shown only while the debug overlay is on.
    pub debug_only: bool,
    /// Turn and scale.
    pub xform: Transform,
    /// Flash, opacity and blend.
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
        assert_eq!(Paint::tile_src(1, 16, 64), Rect::new(0.0, 0.0, 16.0, 16.0));
        assert_eq!(Paint::tile_src(4, 8, 32), Rect::new(24.0, 0.0, 8.0, 8.0));
    }

    #[test]
    fn ids_wrap_onto_the_next_row_of_a_taller_sheet() {
        assert_eq!(Paint::tile_src(5, 8, 32), Rect::new(0.0, 8.0, 8.0, 8.0));
        assert_eq!(Paint::tile_src(7, 8, 32), Rect::new(16.0, 8.0, 8.0, 8.0));
        assert_eq!(
            Paint::tile_src(3, 8, 4),
            Rect::new(0.0, 16.0, 8.0, 8.0),
            "a sheet narrower than a tile is one column"
        );
    }
}
