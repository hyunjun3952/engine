use crate::{Body, Collider, Entity, Filter, Rect, Transform, World};

/// The box that holds `r` turned about its centre by an angle given as `(sin, cos)`, grown by `pad` on every side.
pub(crate) fn turned_box(r: Rect, (s, c): (f32, f32), pad: f32) -> Rect {
    let (hw, hh) = (r.w / 2.0, r.h / 2.0);
    let (ex, ey) = (hw * c.abs() + hh * s.abs() + pad, hw * s.abs() + hh * c.abs() + pad);
    let (cx, cy) = r.center();
    Rect { x: cx - ex, y: cy - ey, w: ex * 2.0, h: ey * 2.0 }
}

// The box a body covers once turned by its `Transform`, as physics and `raycast` see it.
fn turned(r: Rect, t: Option<&Transform>) -> Rect {
    t.filter(|t| t.angle != 0.0).map_or(r, |t| turned_box(r, t.angle.sin_cos(), 0.0))
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

/// Sweep and prune along x that remembers the order it found: bodies move a little between calls, so the next call
/// finds the list nearly sorted and puts it right with an insertion sort in about one pass.
#[derive(Default)]
pub(crate) struct Sweep {
    order: Vec<u32>,
    found: Vec<(usize, usize, f32, f32)>,
}

impl Sweep {
    fn sort(&mut self, rects: &[Rect]) {
        let n = rects.len();
        let have = self.order.len();
        let by_x = |a: &u32, b: &u32| rects[*a as usize].x.total_cmp(&rects[*b as usize].x);
        if have == 0 {
            self.order = (0..n as u32).collect();
            return self.order.sort_unstable_by(by_x);
        }
        self.order.retain(|&i| (i as usize) < n);
        self.order.extend((have as u32..n as u32).filter(|_| have < n));
        let (mut moved, order) = (0, &mut self.order);
        for i in 1..n {
            let (cur, key) = (order[i], rects[order[i] as usize].x);
            let mut j = i;
            while j > 0 && rects[order[j - 1] as usize].x > key {
                order[j] = order[j - 1];
                j -= 1;
            }
            order[j] = cur;
            moved += i - j;
            // A list that was far from sorted (the first call, or a new set of bodies) is quicker to sort outright.
            if moved > 8 * n + 64 {
                order.sort_unstable_by(by_x);
                return;
            }
        }
    }

    /// Every pair of rects that overlap, as `(a, b, overlap in x, overlap in y)` with `a` the one that starts first,
    /// except pairs where both are `inert` (two walls, or a wall and a sleeper, never move each other).
    pub(crate) fn pairs(&mut self, rects: &[Rect], inert: impl Fn(usize) -> bool) -> &[(usize, usize, f32, f32)] {
        self.sort(rects);
        self.found.clear();
        for (k, &a) in self.order.iter().enumerate() {
            let (a, ra) = (a as usize, &rects[a as usize]);
            let a_inert = inert(a);
            for &b in &self.order[k + 1..] {
                let (b, rb) = (b as usize, &rects[b as usize]);
                if rb.x >= ra.x + ra.w {
                    break;
                }
                if (a_inert && inert(b)) || rb.y + rb.h <= ra.y || ra.y + ra.h <= rb.y {
                    continue;
                }
                if let Some((ox, oy)) = ra.penetration(rb) {
                    self.found.push((a, b, ox, oy));
                }
            }
        }
        &self.found
    }
}

pub(crate) fn overlapping_pairs(rects: &[Rect]) -> Vec<(usize, usize, f32, f32)> {
    Sweep::default().pairs(rects, |_| false).to_vec()
}

pub fn collisions(world: &World) -> Vec<Contact> {
    let mut ents = Vec::new();
    let (mut rects, mut filters) = (Vec::new(), Vec::new());
    for (e, b, f, _, t) in world.query::<(Entity, &Body, Option<&Filter>, &Collider, Option<&Transform>)>().iter() {
        ents.push(e);
        rects.push(turned(b.0, t));
        filters.push(f.copied().unwrap_or_default());
    }
    overlapping_pairs(&rects)
        .into_iter()
        .filter(|&(a, b, ..)| filters[a].meets(&filters[b]))
        .map(|(a, b, ox, oy)| Contact { a: ents[a], b: ents[b], ox, oy })
        .collect()
}

pub fn query_rect(world: &World, area: &Rect) -> Vec<Entity> {
    world
        .query::<(Entity, &Body, &Collider, Option<&Transform>)>()
        .iter()
        .filter(|(_, b, _, t)| turned(b.0, *t).overlaps(area))
        .map(|(e, ..)| e)
        .collect()
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

    // Every overlapping pair found the slow way, as (smaller index, larger index) with the overlap.
    fn brute(rects: &[Rect], inert: &dyn Fn(usize) -> bool) -> Vec<(usize, usize, f32, f32)> {
        let mut out = Vec::new();
        for a in 0..rects.len() {
            for b in a + 1..rects.len() {
                if let Some((ox, oy)) = rects[a].penetration(&rects[b]).filter(|_| !(inert(a) && inert(b))) {
                    out.push((a, b, ox, oy));
                }
            }
        }
        out
    }

    fn normalized(pairs: &[(usize, usize, f32, f32)]) -> Vec<(usize, usize, u32, u32)> {
        let mut v: Vec<_> = pairs.iter().map(|&(a, b, x, y)| (a.min(b), a.max(b), x.to_bits(), y.to_bits())).collect();
        v.sort();
        v
    }

    #[test]
    fn a_remembered_sweep_finds_the_same_pairs_as_checking_every_pair_as_bodies_move_and_come_and_go() {
        let mut rng = crate::Rng::new(3);
        let mut rects: Vec<Rect> = (0..150)
            .map(|_| Rect {
                x: rng.range(0.0, 300.0),
                y: rng.range(0.0, 200.0),
                w: rng.range(2.0, 40.0),
                h: rng.range(2.0, 40.0),
            })
            .collect();
        let inert = |i: usize| i.is_multiple_of(3);
        let mut sweep = Sweep::default();
        for round in 0..40 {
            let got = normalized(sweep.pairs(&rects, inert));
            assert_eq!(got, normalized(&brute(&rects, &inert)), "round {round}");
            for r in rects.iter_mut() {
                if rng.f32() < 0.5 {
                    (r.x, r.y) = (r.x + rng.range(-3.0, 3.0), r.y + rng.range(-3.0, 3.0));
                }
            }
            match round % 10 {
                3 => rects.truncate(100),
                6 => rects.extend((0..30).map(|_| Rect {
                    x: rng.range(0.0, 300.0),
                    y: rng.range(0.0, 200.0),
                    w: 10.0,
                    h: 10.0,
                })),
                8 => rects.iter_mut().for_each(|r| r.x = 300.0 - r.x),
                _ => {}
            }
        }
        assert!(!sweep.pairs(&rects, |_| false).is_empty(), "the test world has overlaps to find");
        assert_eq!(normalized(&overlapping_pairs(&rects)), normalized(&brute(&rects, &|_| false)));
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

    #[test]
    fn query_rect_lists_the_colliders_inside_an_area() {
        let mut w = World::new();
        let a = boxed(&mut w, 0.0, true);
        boxed(&mut w, 5.0, false);
        boxed(&mut w, 50.0, true);
        assert_eq!(query_rect(&w, &Rect { x: -5.0, y: 0.0, w: 10.0, h: 10.0 }), vec![a]);
    }

    #[test]
    fn a_filter_needs_both_sides_to_agree() {
        let mut w = World::new();
        let (a, b, c) = (boxed(&mut w, 0.0, true), boxed(&mut w, 2.0, true), boxed(&mut w, 4.0, true));
        w.insert_one(a, Filter { group: 1, mask: 2 }).unwrap();
        w.insert_one(b, Filter { group: 2, mask: 1 }).unwrap();
        w.insert_one(c, Filter { group: 4, mask: 1 }).unwrap();
        let pairs: Vec<_> = collisions(&w).iter().map(|k| (k.a, k.b)).collect();
        assert_eq!(pairs.len(), 1);
        assert!(pairs.contains(&(a, b)) || pairs.contains(&(b, a)));
    }

    #[test]
    fn a_collider_without_a_rigid_body_is_a_trigger_that_still_reports_overlaps() {
        let mut w = World::new();
        let body = w.spawn((Body(Rect { x: 0.0, y: 0.0, w: 10.0, h: 10.0 }), Collider, crate::RigidBody::fixed()));
        let zone = boxed(&mut w, 5.0, true);
        assert_eq!(collisions(&w)[0].other(body), Some(zone));
    }

    #[test]
    fn collisions_and_queries_see_a_body_as_turned() {
        let mut w = World::new();
        let plank = w.spawn((
            Body(Rect { x: 0.0, y: 0.0, w: 100.0, h: 10.0 }),
            Collider,
            Transform { angle: std::f32::consts::FRAC_PI_2, ..Default::default() },
        ));
        let probe = Rect { x: 45.0, y: 40.0, w: 10.0, h: 10.0 };
        assert_eq!(query_rect(&w, &probe), vec![plank]);
    }
}
