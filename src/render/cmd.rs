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
    /// Where tile `id` (the sheet column plus one) is on a sheet that is a row of `size` pixel squares.
    pub(crate) fn tile_src(id: u8, size: u32) -> Rect {
        Rect { x: ((id as u32 - 1) * size) as f32, y: 0.0, w: size as f32, h: size as f32 }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_ids_start_at_one_and_walk_along_the_sheet() {
        assert_eq!(Paint::tile_src(1, 16), Rect { x: 0.0, y: 0.0, w: 16.0, h: 16.0 });
        assert_eq!(Paint::tile_src(4, 8), Rect { x: 24.0, y: 0.0, w: 8.0, h: 8.0 });
    }
}
