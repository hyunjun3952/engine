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
}

#[derive(Clone)]
pub struct DrawCmd {
    pub layer: i32,
    pub rect: Rect,
    pub paint: Paint,
    pub debug_only: bool,
}
