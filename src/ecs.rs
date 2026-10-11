use crate::{Gfx, Rect, Sprite};
pub use hecs::{Entity, World};
use std::sync::Arc;

pub struct Body(pub Rect);
pub struct Vel {
    pub x: f32,
    pub y: f32,
}
pub struct Collider;
pub enum Look {
    Color(u32),
    Sprite(Arc<Sprite>),
}

pub fn movement(world: &mut World, dt: f32) {
    for (b, v) in world.query_mut::<(&mut Body, &Vel)>() {
        b.0.x += v.x * dt;
        b.0.y += v.y * dt;
    }
}

pub fn render(world: &World, gfx: &mut Gfx) {
    let (cx, cy) = gfx.cam;
    let view = |r: &Rect| Rect { x: r.x - cx, y: r.y - cy, ..*r };
    for (b, look) in world.query::<(&Body, &Look)>().iter() {
        let r = view(&b.0);
        match look {
            Look::Color(c) => gfx.fill(&r, *c),
            Look::Sprite(s) => gfx.blit(s, r.x as i32, r.y as i32),
        }
    }
    if gfx.debug {
        for (b, _) in world.query::<(&Body, &Collider)>().iter() {
            gfx.outline(&view(&b.0), 0x00ff00);
        }
    }
}

pub struct Contact {
    pub a: Entity,
    pub b: Entity,
    pub ox: f32,
    pub oy: f32,
}

impl Contact {
    pub fn other(&self, e: Entity) -> Option<Entity> {
        if self.a == e {
            Some(self.b)
        } else if self.b == e {
            Some(self.a)
        } else {
            None
        }
    }

    pub fn x_axis(&self) -> bool {
        self.ox < self.oy
    }
}

pub fn collisions(world: &World) -> Vec<Contact> {
    let mut boxes: Vec<(Entity, Rect)> =
        world.query::<(Entity, &Body, &Collider)>().iter().map(|(e, b, _)| (e, b.0)).collect();
    boxes.sort_unstable_by(|a, b| a.1.x.total_cmp(&b.1.x));
    let mut out = Vec::new();
    for (i, (a, ra)) in boxes.iter().enumerate() {
        for (b, rb) in &boxes[i + 1..] {
            if rb.x >= ra.x + ra.w {
                break;
            }
            if let Some((ox, oy)) = ra.penetration(rb) {
                out.push(Contact { a: *a, b: *b, ox, oy });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boxed(w: &mut World, x: f32, collide: bool) -> Entity {
        let e = w.spawn((Body(Rect { x, y: 0.0, w: 10.0, h: 10.0 }),));
        if collide {
            w.insert_one(e, Collider).unwrap();
        }
        e
    }

    #[test]
    fn only_overlapping_colliders_make_contacts() {
        let mut w = World::new();
        let a = boxed(&mut w, 0.0, true);
        let b = boxed(&mut w, 8.0, true);
        boxed(&mut w, 5.0, false);
        boxed(&mut w, 50.0, true);
        let c = collisions(&w);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].other(a), Some(b));
        assert_eq!((c[0].ox, c[0].oy), (2.0, 10.0));
        assert!(c[0].x_axis());
    }
}
