use crate::{Rect, Sprite, World};
use std::sync::Arc;

pub struct Body(pub Rect);

#[derive(Default)]
pub struct Vel {
    pub x: f32,
    pub y: f32,
}

pub struct Collider;

pub struct Layer(pub i32);

pub enum Look {
    Color(u32),
    Alpha(u32, u8),
    Sprite(Arc<Sprite>),
}

pub fn movement(world: &mut World, dt: f32) {
    for (b, v) in world.query_mut::<(&mut Body, &Vel)>() {
        b.0.x += v.x * dt;
        b.0.y += v.y * dt;
    }
}
