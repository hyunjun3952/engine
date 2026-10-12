use crate::{Body, Collider, Entity, Filter, Rect, Transform, World};

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
    let mut ents = Vec::new();
    let (mut rects, mut filters) = (Vec::new(), Vec::new());
    for (e, b, f, _) in world.query::<(Entity, &Body, Option<&Filter>, &Collider)>().iter() {
        ents.push(e);
        rects.push(b.0);
        filters.push(f.copied().unwrap_or_default());
    }
    overlapping_pairs(&rects)
        .into_iter()
        .filter(|&(a, b, ..)| filters[a].meets(&filters[b]))
        .map(|(a, b, ox, oy)| Contact { a: ents[a], b: ents[b], ox, oy })
        .collect()
}

// First collider in `mask`'s groups hit by the ray `from + t * dir`, 0 <= t <= max, with its t; `dir` need not be a
// unit vector. A body with a `Transform` is hit where it is turned, as physics sees it (scale is ignored).
pub fn raycast(world: &World, from: (f32, f32), dir: (f32, f32), max: f32, mask: u32) -> Option<(Entity, f32)> {
    let mut best: Option<(Entity, f32)> = None;
    for (e, b, f, xform, _) in world.query::<(Entity, &Body, Option<&Filter>, Option<&Transform>, &Collider)>().iter() {
        if f.is_some_and(|f| f.group & mask == 0) {
            continue;
        }
        let r = &b.0;
        let (c, (s, k)) = (r.center(), xform.map_or(0.0, |t| t.angle).sin_cos());
        let turn = |x: f32, y: f32| (x * k + y * s, y * k - x * s);
        let (o, d) = (turn(from.0 - c.0, from.1 - c.1), turn(dir.0, dir.1));
        let (mut t0, mut t1) = (0.0_f32, max);
        for (o, d, half) in [(o.0, d.0, r.w / 2.0), (o.1, d.1, r.h / 2.0)] {
            if d == 0.0 {
                if o.abs() > half {
                    t1 = -1.0;
                }
                continue;
            }
            let (a, b) = ((-half - o) / d, (half - o) / d);
            t0 = t0.max(a.min(b));
            t1 = t1.min(a.max(b));
        }
        if t0 <= t1 && best.is_none_or(|(_, t)| t0 < t) {
            best = Some((e, t0));
        }
    }
    best
}

pub fn query_rect(world: &World, area: &Rect) -> Vec<Entity> {
    world.query::<(Entity, &Body, &Collider)>().iter().filter(|(_, b, _)| b.0.overlaps(area)).map(|(e, ..)| e).collect()
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

    #[test]
    fn a_ray_hits_the_nearest_collider_in_front_and_stops_at_max() {
        let mut w = World::new();
        let near = boxed(&mut w, 20.0, true);
        boxed(&mut w, 50.0, true);
        boxed(&mut w, -30.0, true);
        boxed(&mut w, 80.0, false);
        let (e, t) = raycast(&w, (0.0, 5.0), (1.0, 0.0), 100.0, u32::MAX).unwrap();
        assert_eq!((e, t), (near, 20.0));
        assert!(raycast(&w, (0.0, 5.0), (1.0, 0.0), 10.0, u32::MAX).is_none());
        assert!(raycast(&w, (0.0, 50.0), (1.0, 0.0), 100.0, u32::MAX).is_none());
        assert_eq!(raycast(&w, (25.0, 5.0), (0.0, 1.0), 5.0, u32::MAX).map(|h| h.1), Some(0.0));
    }

    #[test]
    fn a_ray_skips_groups_outside_its_mask_and_sees_a_turned_box_where_it_is() {
        let mut w = World::new();
        let wall = boxed(&mut w, 20.0, true);
        w.insert_one(wall, Filter { group: 2, mask: u32::MAX }).unwrap();
        assert!(raycast(&w, (0.0, 5.0), (1.0, 0.0), 100.0, 1).is_none());
        assert_eq!(raycast(&w, (0.0, 5.0), (1.0, 0.0), 100.0, 2).map(|h| h.0), Some(wall));
        // a 10x10 box turned 45 degrees about (25, 5) reaches 7.07 either side of it, so a ray along y = 5 meets it at 17.9
        let turned = Transform { angle: std::f32::consts::FRAC_PI_4, ..Transform::IDENTITY };
        w.insert_one(wall, turned).unwrap();
        let t = raycast(&w, (0.0, 5.0), (1.0, 0.0), 100.0, 2).unwrap().1;
        assert!((t - 17.93).abs() < 0.01, "{t}");
        // along y = 0.5 the unturned box would be met at 20; the diamond is only 2.57 wide either side there
        let t = raycast(&w, (0.0, 0.5), (1.0, 0.0), 100.0, 2).unwrap().1;
        assert!((t - 22.43).abs() < 0.01, "{t}");
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
}
