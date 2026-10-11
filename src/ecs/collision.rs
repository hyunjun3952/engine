use crate::{Body, Collider, Entity, Rect, World};

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

pub(crate) fn overlapping_pairs(rects: &[Rect]) -> Vec<(usize, usize, f32, f32)> {
    let mut order: Vec<usize> = (0..rects.len()).collect();
    order.sort_unstable_by(|&a, &b| rects[a].x.total_cmp(&rects[b].x));
    let mut out = Vec::new();
    for (k, &a) in order.iter().enumerate() {
        for &b in &order[k + 1..] {
            if rects[b].x >= rects[a].x + rects[a].w {
                break;
            }
            if let Some((ox, oy)) = rects[a].penetration(&rects[b]) {
                out.push((a, b, ox, oy));
            }
        }
    }
    out
}

pub fn collisions(world: &World) -> Vec<Contact> {
    let (ents, rects): (Vec<Entity>, Vec<Rect>) =
        world.query::<(Entity, &Body, &Collider)>().iter().map(|(e, b, _)| (e, b.0)).unzip();
    overlapping_pairs(&rects).into_iter().map(|(a, b, ox, oy)| Contact { a: ents[a], b: ents[b], ox, oy }).collect()
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
