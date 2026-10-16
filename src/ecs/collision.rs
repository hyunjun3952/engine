use crate::base::fx::FxSet;
use crate::{Body, Collider, Entity, Filter, Rect, Transform, Vec2, World};

/// The box that holds `r` turned about its centre by an angle given as `(sin, cos)`, grown by `pad` on every side.
pub(crate) fn turned_box(r: Rect, (s, c): (f32, f32), pad: f32) -> Rect {
    let (hw, hh) = (r.w / 2.0, r.h / 2.0);
    let (ex, ey) = (hw * c.abs() + hh * s.abs() + pad, hw * s.abs() + hh * c.abs() + pad);
    let Vec2 { x: cx, y: cy } = r.center();
    Rect::new(cx - ex, cy - ey, ex * 2.0, ey * 2.0)
}

// The box a body covers once turned by its `Transform`, as physics and `raycast` see it.
fn turned(r: Rect, t: Option<&Transform>) -> Rect {
    t.filter(|t| t.angle != 0.0).map_or(r, |t| turned_box(r, t.angle.sin_cos(), 0.0))
}

/// Two overlapping colliders and how deep they overlap on each axis; see [`collisions`](crate::collisions).
///
/// ```
/// use engine::{Contact, World};
///
/// let mut world = World::new();
/// let (a, b) = (world.spawn(()), world.spawn(()));
/// let hit = Contact { a, b, ox: 1.0, oy: 3.0 };
/// assert_eq!(hit.other(a), Some(b));
/// assert!(hit.x_axis());
/// ```
pub struct Contact {
    /// One of the two entities.
    pub a: Entity,
    /// The other entity.
    pub b: Entity,
    /// How deep the boxes overlap along x.
    pub ox: f32,
    /// How deep the boxes overlap along y.
    pub oy: f32,
}

impl Contact {
    /// The entity that is not `e`, or `None` if `e` is in neither.
    pub fn other(&self, e: Entity) -> Option<Entity> {
        if self.a == e {
            Some(self.b)
        } else if self.b == e {
            Some(self.a)
        } else {
            None
        }
    }

    /// True when the overlap is shallower along x, so the way out is sideways.
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
            while j > 0 && rects[order[j - 1] as usize].x.total_cmp(&key).is_gt() {
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
    pub(crate) fn overlaps(&mut self, rects: &[Rect], inert: impl Fn(usize) -> bool) -> &[(usize, usize, f32, f32)] {
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
                if let Some(Vec2 { x: ox, y: oy }) = ra.penetration(*rb) {
                    self.found.push((a, b, ox, oy));
                }
            }
        }
        &self.found
    }
}

pub(crate) fn overlapping_pairs(rects: &[Rect]) -> Vec<(usize, usize, f32, f32)> {
    let span = |at: fn(&Rect) -> f32| {
        let (lo, hi) = rects.iter().map(at).fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)));
        hi - lo
    };
    // Sweeping along x alone degrades towards every pair when the bodies are stacked in a tall column.
    if span(|r| r.y) <= span(|r| r.x) {
        return Sweep::default().overlaps(rects, |_| false).to_vec();
    }
    let flipped: Vec<Rect> = rects.iter().map(|r| Rect::new(r.y, r.x, r.h, r.w)).collect();
    Sweep::default().overlaps(&flipped, |_| false).iter().map(|&(a, b, ox, oy)| (a, b, oy, ox)).collect()
}

/// ```
/// use engine::{Body, Collider, Rect, World};
/// let mut world = World::new();
/// let a = world.spawn((Body(Rect::new(0.0, 0.0, 10.0, 10.0)), Collider));
/// let b = world.spawn((Body(Rect::new(5.0, 5.0, 10.0, 10.0)), Collider));
/// let hits = engine::collisions(&world);
/// assert_eq!(hits.len(), 1);
/// assert_eq!(hits[0].other(a), Some(b));
/// ```
pub fn collisions(world: &World) -> Vec<Contact> {
    let mut ents = Vec::new();
    let (mut rects, mut filters) = (Vec::new(), Vec::new());
    for (e, b, f, _, t) in &mut world.query::<(Entity, &Body, Option<&Filter>, &Collider, Option<&Transform>)>() {
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

/// Remembers which pairs overlapped at the last `update`, to tell when two colliders start and stop touching (a
/// player entering a trigger zone). Feed it `collisions(&world)` once per step.
///
/// ```
/// use engine::{Body, Collider, Rect, World};
/// let mut world = World::new();
/// let a = world.spawn((Body(Rect::new(0.0, 0.0, 10.0, 10.0)), Collider));
/// let b = world.spawn((Body(Rect::new(5.0, 5.0, 10.0, 10.0)), Collider));
/// use engine::{Overlaps, collisions};
///
/// let mut overlaps = Overlaps::new();
/// let changes = overlaps.update(&collisions(&world));
/// assert_eq!(changes.began, vec![(a, b)]);
/// assert!(overlaps.touching(a, b));
/// ```
#[derive(Default)]
pub struct Overlaps {
    pairs: FxSet<(Entity, Entity)>,
}

/// The pairs that started and stopped overlapping since the last `Overlaps::update`, each pair as `(a, b)` with
/// `a < b`, sorted. A despawned entity's pairs come out as ended.
///
/// ```
/// use engine::{Body, Collider, Rect, World};
/// let mut world = World::new();
/// let a = world.spawn((Body(Rect::new(0.0, 0.0, 10.0, 10.0)), Collider));
/// let b = world.spawn((Body(Rect::new(5.0, 5.0, 10.0, 10.0)), Collider));
/// use engine::{Overlaps, collisions};
///
/// let mut overlaps = Overlaps::new();
/// overlaps.update(&collisions(&world));
/// world.get::<&mut Body>(b).unwrap().0.x = 100.0;
/// let changes = overlaps.update(&collisions(&world));
/// assert_eq!(changes.ended, vec![(a, b)]);
/// ```
#[derive(Debug, Default, PartialEq)]
pub struct OverlapChanges {
    /// Pairs that started to overlap this step.
    pub began: Vec<(Entity, Entity)>,
    /// Pairs that stopped overlapping this step.
    pub ended: Vec<(Entity, Entity)>,
}

// Two entities as an unordered pair: the smaller first.
fn pair(a: Entity, b: Entity) -> (Entity, Entity) {
    (a.min(b), a.max(b))
}

impl Overlaps {
    /// Nothing is touching yet.
    pub fn new() -> Self {
        Overlaps::default()
    }

    /// Takes this step's contacts and returns what began and ended since the last call.
    pub fn update(&mut self, contacts: &[Contact]) -> OverlapChanges {
        let now: FxSet<(Entity, Entity)> = contacts.iter().map(|c| pair(c.a, c.b)).collect();
        let mut began: Vec<_> = now.difference(&self.pairs).copied().collect();
        let mut ended: Vec<_> = self.pairs.difference(&now).copied().collect();
        began.sort();
        ended.sort();
        self.pairs = now;
        OverlapChanges { began, ended }
    }

    /// True if the two overlapped at the last `update`.
    pub fn touching(&self, a: Entity, b: Entity) -> bool {
        self.pairs.contains(&pair(a, b))
    }
}

/// ```
/// use engine::{Body, Collider, Rect, World};
/// let mut world = World::new();
/// let a = world.spawn((Body(Rect::new(0.0, 0.0, 10.0, 10.0)), Collider));
/// let b = world.spawn((Body(Rect::new(5.0, 5.0, 10.0, 10.0)), Collider));
/// let near = engine::query_rect(&world, Rect::new(14.0, 14.0, 1.0, 1.0));
/// assert_eq!(near, vec![b]);
/// ```
pub fn query_rect(world: &World, area: Rect) -> Vec<Entity> {
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

    #[test]
    fn overlaps_report_pairs_that_begin_and_end_once() {
        let mut w = World::new();
        let (a, b) = (boxed(&mut w, 0.0, true), boxed(&mut w, 50.0, true));
        let mut o = Overlaps::new();
        assert_eq!(o.update(&collisions(&w)), OverlapChanges::default());
        w.get::<&mut Body>(b).unwrap().0.x = 5.0;
        assert_eq!(o.update(&collisions(&w)), OverlapChanges { began: vec![pair(a, b)], ended: vec![] });
        assert!(o.touching(b, a));
        assert_eq!(o.update(&collisions(&w)), OverlapChanges::default(), "still touching is no news");
        w.despawn(b).unwrap();
        assert_eq!(o.update(&collisions(&w)), OverlapChanges { began: vec![], ended: vec![pair(a, b)] });
        assert!(!o.touching(a, b));
    }

    // Every overlapping pair found the slow way, as (smaller index, larger index) with the overlap.
    fn brute(rects: &[Rect], inert: &dyn Fn(usize) -> bool) -> Vec<(usize, usize, f32, f32)> {
        let mut out = Vec::new();
        for a in 0..rects.len() {
            for b in a + 1..rects.len() {
                if let Some(Vec2 { x: ox, y: oy }) = rects[a].penetration(rects[b]).filter(|_| !(inert(a) && inert(b)))
                {
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
            let got = normalized(sweep.overlaps(&rects, inert));
            assert_eq!(got, normalized(&brute(&rects, &inert)), "round {round}");
            for r in &mut rects {
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
        assert!(!sweep.overlaps(&rects, |_| false).is_empty(), "the test world has overlaps to find");
        assert_eq!(normalized(&overlapping_pairs(&rects)), normalized(&brute(&rects, &|_| false)));
        let tall: Vec<Rect> = rects.iter().map(|r| Rect::new(r.y * 0.1, r.x * 10.0, r.h, r.w * 10.0)).collect();
        assert_eq!(normalized(&overlapping_pairs(&tall)), normalized(&brute(&tall, &|_| false)));
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
        assert_eq!(query_rect(&w, Rect::new(-5.0, 0.0, 10.0, 10.0)), vec![a]);
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
        let body = w.spawn((Body(Rect::new(0.0, 0.0, 10.0, 10.0)), Collider, crate::RigidBody::fixed()));
        let zone = boxed(&mut w, 5.0, true);
        assert_eq!(collisions(&w)[0].other(body), Some(zone));
    }

    #[test]
    fn collisions_and_queries_see_a_body_as_turned() {
        let mut w = World::new();
        let plank = w.spawn((
            Body(Rect::new(0.0, 0.0, 100.0, 10.0)),
            Collider,
            Transform { angle: std::f32::consts::FRAC_PI_2, ..Default::default() },
        ));
        let probe = Rect::new(45.0, 40.0, 10.0, 10.0);
        assert_eq!(query_rect(&w, probe), vec![plank]);
    }
}
