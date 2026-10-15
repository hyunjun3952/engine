use crate::{Rect, Sprite, World};
use std::sync::Arc;

/// Where an entity is and how big, in world pixels.
#[derive(Debug)]
pub struct Body(pub Rect);

#[derive(Default, Debug)]
pub struct Vel {
    pub x: f32,
    pub y: f32,
}

/// Makes a [`Body`] collide in [`collisions`](crate::collisions), [`raycast`](crate::raycast) and [`query_rect`](crate::query_rect); without a [`RigidBody`](crate::RigidBody) it is a trigger.
#[derive(Debug)]
pub struct Collider;

#[derive(Debug)]
pub struct Layer(pub i32);

// Two entities meet only if each one's mask holds a bit of the other's group; no Filter means every group and mask.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Filter {
    pub group: u32,
    pub mask: u32,
}

impl Default for Filter {
    fn default() -> Self {
        Filter { group: u32::MAX, mask: u32::MAX }
    }
}

impl Filter {
    /// The group of every solid tile; clear this bit in a mask to pass through the map.
    pub const TERRAIN: u32 = 1 << 31;
    /// The group of one-way tiles: clear this bit to drop through them while still standing on solid ground.
    pub const PLATFORM: u32 = 1 << 30;

    pub fn meets(&self, other: &Filter) -> bool {
        self.group & other.mask != 0 && other.group & self.mask != 0
    }
}

/// How [`draw_list`](crate::draw_list) draws an entity over its [`Body`].
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
