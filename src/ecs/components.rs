use crate::{Rect, Sprite, World};
use std::sync::Arc;

/// Where an entity is and how big, in world pixels.
///
/// ```
/// use engine::{Body, Rect};
///
/// let body = Body(Rect::new(10.0, 20.0, 8.0, 8.0));
/// assert_eq!(body.0.center(), (14.0, 24.0).into());
/// ```
#[derive(Debug)]
pub struct Body(pub Rect);

/// Velocity in pixels per second, moved by [`movement`](crate::movement) or `Physics`.
///
/// ```
/// use engine::Vel;
///
/// let v = Vel { x: 60.0, ..Default::default() };
/// assert_eq!(v.y, 0.0);
/// ```
#[derive(Default, Debug)]
pub struct Vel {
    /// Horizontal speed in pixels per second.
    pub x: f32,
    /// Vertical speed in pixels per second.
    pub y: f32,
}

/// Makes a [`Body`] collide in [`collisions`](crate::collisions), [`raycast`](crate::raycast) and [`query_rect`](crate::query_rect); without a [`RigidBody`](crate::RigidBody) it is a trigger.
///
/// ```
/// use engine::{Body, Collider, Rect, World};
/// let mut world = World::new();
/// let a = world.spawn((Body(Rect::new(0.0, 0.0, 10.0, 10.0)), Collider));
/// let b = world.spawn((Body(Rect::new(5.0, 5.0, 10.0, 10.0)), Collider));
/// assert_eq!(engine::collisions(&world).len(), 1);
/// ```
#[derive(Debug)]
pub struct Collider;

/// Draw order for [`draw_list`](crate::draw_list): a higher layer is drawn later, on top.
///
/// ```
/// use engine::{Body, Layer, Rect, World};
///
/// let mut world = World::new();
/// world.spawn((Body(Rect::new(0.0, 0.0, 8.0, 8.0)), Layer(2)));
/// ```
#[derive(Debug)]
pub struct Layer(pub i32);

/// Two entities meet only if each one's mask holds a bit of the other's group; no Filter means every group and mask.
///
/// ```
/// use engine::Filter;
///
/// let all = Filter::default();
/// let ghost = Filter { group: 1, mask: 0 };
/// assert_eq!(all.mask, u32::MAX);
/// assert_eq!(ghost.mask & all.group, 0);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Filter {
    /// The bits of the groups this entity belongs to.
    pub group: u32,
    /// The bits of the groups this entity meets.
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

    /// True if each one's mask holds a bit of the other's group.
    pub fn meets(&self, other: &Filter) -> bool {
        self.group & other.mask != 0 && other.group & self.mask != 0
    }
}

/// How [`draw_list`](crate::draw_list) draws an entity over its [`Body`].
///
/// ```
/// use engine::{Body, Layer, Look, Rect, World, draw_list};
///
/// let mut world = World::new();
/// world.spawn((Body(Rect::new(0.0, 0.0, 8.0, 8.0)), Look::Color(0xff0000), Layer(1)));
/// assert_eq!(draw_list(&world).len(), 1);
/// ```
pub enum Look {
    /// A flat 0xRRGGBB colour.
    Color(u32),
    /// A flat colour with an alpha, 0xRRGGBB and 0 to 255.
    Alpha(u32, u8),
    /// An image, stretched to the body.
    Sprite(Arc<Sprite>),
}

/// ```
/// use engine::{Body, Rect, Vel, World, movement};
///
/// let mut world = World::new();
/// let e = world.spawn((Body(Rect::new(0.0, 0.0, 8.0, 8.0)), Vel { x: 10.0, y: 0.0 }));
/// movement(&mut world, 0.5);
/// assert_eq!(world.get::<&Body>(e).unwrap().0.x, 5.0);
/// ```
pub fn movement(world: &mut World, dt: f32) {
    for (b, v) in world.query_mut::<(&mut Body, &Vel)>() {
        b.0.x += v.x * dt;
        b.0.y += v.y * dt;
    }
}
