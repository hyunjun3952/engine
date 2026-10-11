use crate::{Rect, Sprite};
use std::sync::Arc;

#[derive(Clone)]
pub enum Paint {
    Clear(u32),
    Color(u32),
    Alpha(u32, u8),
    Sprite(Arc<Sprite>),
    Region { sheet: Arc<Sprite>, src: Rect, flip: bool },
    Outline(u32),
    Glyph { ch: u8, color: u32, scale: u32 },
    Tiles { sheet: Arc<Sprite>, size: u32, tiles: Vec<(u32, u32, u8)> },
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
