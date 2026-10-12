use crate::{Rect, Sprite};
use std::sync::Arc;

/// Where a scaled or turned sprite is sampled, nudged by this much of a texel in the sprite's own pixels by both
/// backends: a pixel centre that lands exactly on the border of two texels (a zoom of 1.5 does that on every third
/// pixel) would otherwise pick one the software way and the other whichever way the GPU rounds.
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

#[derive(Clone)]
pub struct DrawCmd {
    pub layer: i32,
    pub rect: Rect,
    pub paint: Paint,
    pub debug_only: bool,
    pub xform: Transform,
}

impl DrawCmd {
    /// A command on layer 0, always drawn, without rotation or scale.
    pub fn new(rect: Rect, paint: Paint) -> Self {
        DrawCmd { layer: 0, rect, paint, debug_only: false, xform: Transform::IDENTITY }
    }
}
