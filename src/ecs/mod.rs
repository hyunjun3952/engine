mod particles;

pub use particles::{Burst, burst, particles};

use crate::{Gfx, Rect, Sprite};
pub use hecs::{Entity, World};
use std::sync::Arc;

pub struct Body(pub Rect);
pub struct Vel {
    pub x: f32,
    pub y: f32,
}
pub struct Collider;

pub struct Layer(pub i32);

pub struct Anim {
    sheet: Arc<Sprite>,
    size: (usize, usize),
    row: usize,
    frames: usize,
    fps: f32,
    t: f32,
    pub flip: bool,
}

impl Anim {
    pub fn new(sheet: Arc<Sprite>, frame_w: usize, frame_h: usize) -> Self {
        Anim { sheet, size: (frame_w, frame_h), row: 0, frames: 1, fps: 1.0, t: 0.0, flip: false }
    }

    pub fn play(&mut self, row: usize, frames: usize, fps: f32) {
        assert!(frames > 0 && fps > 0.0, "animation needs frames and fps");
        if self.row != row {
            self.t = 0.0;
        }
        (self.row, self.frames, self.fps) = (row, frames, fps);
    }

    fn frame(&self) -> usize {
        (self.t * self.fps) as usize % self.frames
    }
}

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

pub fn animate(world: &mut World, dt: f32) {
    for a in world.query_mut::<&mut Anim>() {
        a.t = (a.t + dt) % (a.frames as f32 / a.fps);
    }
}

enum Draw<'a> {
    Look(&'a Look),
    Anim(&'a Anim),
}

pub fn render(world: &World, gfx: &mut Gfx) {
    let (cx, cy) = gfx.cam;
    let view = |r: &Rect| Rect { x: r.x - cx, y: r.y - cy, ..*r };
    let mut query = world.query::<(&Body, Option<&Layer>, Option<&Look>, Option<&Anim>)>();
    let mut items = Vec::new();
    for (body, layer, look, anim) in query.iter() {
        let (z, r) = (layer.map_or(0, |l| l.0), view(&body.0));
        if let Some(l) = look {
            items.push((z, r, Draw::Look(l)));
        }
        if let Some(a) = anim {
            items.push((z, r, Draw::Anim(a)));
        }
    }
    items.sort_by_key(|i| i.0);
    for (_, r, draw) in items {
        match draw {
            Draw::Look(Look::Color(c)) => gfx.fill(&r, *c),
            Draw::Look(Look::Alpha(c, a)) => gfx.fill_alpha(&r, *c, *a),
            Draw::Look(Look::Sprite(s)) => gfx.blit(s, r.x as i32, r.y as i32),
            Draw::Anim(a) => {
                let (fw, fh) = a.size;
                let src = Rect { x: (a.frame() * fw) as f32, y: (a.row * fh) as f32, w: fw as f32, h: fh as f32 };
                gfx.blit_region(&a.sheet, src, r.x as i32, r.y as i32, a.flip);
            }
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

    #[test]
    fn higher_layers_draw_on_top_regardless_of_spawn_order() {
        let mut w = World::new();
        let square = |c| (Body(Rect { x: 0.0, y: 0.0, w: 4.0, h: 4.0 }), Look::Color(c));
        let (top, bottom) = (square(0xff0000), square(0x0000ff));
        w.spawn((top.0, top.1, Layer(5)));
        w.spawn((bottom.0, bottom.1));
        let mut g = Gfx::new(4, 4);
        render(&w, &mut g);
        assert_eq!(g.buf[0], 0xff0000);
    }

    #[test]
    fn animation_advances_loops_and_resets_on_clip_change() {
        let mut w = World::new();
        let mut a = Anim::new(Arc::new(Sprite::load("assets/player.png")), 12, 24);
        a.play(1, 4, 10.0);
        let e = w.spawn((a,));
        animate(&mut w, 0.25);
        assert_eq!(w.get::<&Anim>(e).unwrap().frame(), 2);
        animate(&mut w, 0.2);
        assert_eq!(w.get::<&Anim>(e).unwrap().frame(), 0);
        w.query_one_mut::<&mut Anim>(e).unwrap().play(0, 2, 2.0);
        assert_eq!(w.get::<&Anim>(e).unwrap().frame(), 0);
    }

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
