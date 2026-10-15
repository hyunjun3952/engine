use super::collision::turned_box;
use crate::{Body, Collider, Entity, Filter, Rect, Transform, Vec2, World};

type V2 = (f32, f32);

struct Item {
    e: Entity,
    rect: Rect,
    // Sine and cosine of the turn, taken once so casting does not repeat them.
    turn: V2,
    group: u32,
}

type Row<'a> = (Entity, &'a Body, Option<&'a Filter>, Option<&'a Transform>, &'a Collider);

impl Item {
    fn new((e, b, f, x, _): Row) -> Self {
        let turn = x.map_or((0.0, 1.0), |t| if t.angle == 0.0 { (0.0, 1.0) } else { t.angle.sin_cos() });
        Item { e, rect: b.0, turn, group: f.map_or(u32::MAX, |f| f.group) }
    }

    // Where the ray enters the box (turned about its centre), if it does within 0..=max.
    fn hit(&self, from: V2, dir: V2, max: f32) -> Option<f32> {
        let r = &self.rect;
        let (c, (s, k)) = (r.center(), self.turn);
        let c = (c.x, c.y);
        let turn = |x: f32, y: f32| (x * k + y * s, y * k - x * s);
        let (o, d) = (turn(from.0 - c.0, from.1 - c.1), turn(dir.0, dir.1));
        slab((o.0, d.0, -r.w / 2.0, r.w / 2.0), (o.1, d.1, -r.h / 2.0, r.h / 2.0), max)
    }

    // The box's bounds once turned, a little larger so a ray along a cell edge cannot slip between cells.
    fn bounds(&self) -> Rect {
        turned_box(self.rect, self.turn, 0.01)
    }
}

// The ray `o + t * d` against the box `lo..hi` on every axis: the entry t if it is within 0..=max.
fn slab(x: (f32, f32, f32, f32), y: (f32, f32, f32, f32), max: f32) -> Option<f32> {
    let (mut t0, mut t1) = (0.0_f32, max);
    for (o, d, lo, hi) in [x, y] {
        if d == 0.0 {
            if o < lo || o > hi {
                return None;
            }
            continue;
        }
        let (a, b) = ((lo - o) / d, (hi - o) / d);
        t0 = t0.max(a.min(b));
        t1 = t1.min(a.max(b));
    }
    (t0 <= t1).then_some(t0)
}

/// What a ray met: the entity and how far along the ray, in multiples of `dir`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RayHit {
    pub entity: Entity,
    pub t: f32,
}

/// One ray for `raycasts`: `from + t * dir` for `0 <= t <= max`, meeting only colliders whose group is in `mask`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ray {
    pub from: Vec2,
    pub dir: Vec2,
    pub max: f32,
    pub mask: u32,
}

impl Ray {
    pub fn new(from: impl Into<Vec2>, dir: impl Into<Vec2>, max: f32, mask: u32) -> Self {
        Ray { from: from.into(), dir: dir.into(), max, mask }
    }
}

fn v2(v: impl Into<Vec2>) -> V2 {
    let v = v.into();
    (v.x, v.y)
}

fn hit((entity, t): (Entity, f32)) -> RayHit {
    RayHit { entity, t }
}

/// Nearest collider in `mask` hit by `from + t * dir`, `0 <= t <= max`; follows `Transform::angle`, ignores scale.
pub fn raycast(world: &World, from: impl Into<Vec2>, dir: impl Into<Vec2>, max: f32, mask: u32) -> Option<RayHit> {
    let (from, dir) = (v2(from), v2(dir));
    let mut best: Option<(Entity, f32)> = None;
    for item in world.query::<Row>().iter().map(Item::new).filter(|i| i.group & mask != 0) {
        // A box farther than the best hit so far cannot win, so the search range shrinks as hits come in.
        if let Some(t) = item.hit(from, dir, best.map_or(max, |b| b.1)) {
            best = Some((item.e, t));
        }
    }
    best.map(hit)
}

// Building a `RayIndex` costs about as much as five scans, so fewer rays than this are scanned.
const INDEX_FROM: usize = 8;

/// `raycast` for many rays at once; from a handful of rays on it builds a `RayIndex`.
pub fn raycasts(world: &World, rays: &[Ray]) -> Vec<Option<RayHit>> {
    if rays.len() < INDEX_FROM {
        return rays.iter().map(|r| raycast(world, r.from, r.dir, r.max, r.mask)).collect();
    }
    let index = RayIndex::new(world);
    rays.iter().map(|r| index.cast(r.from, r.dir, r.max, r.mask)).collect()
}

/// The colliders on a uniform grid for many rays against one snapshot; rebuild it after bodies move.
pub struct RayIndex {
    items: Vec<Item>,
    origin: V2,
    cell: f32,
    cols: usize,
    rows: usize,
    // Cell `i` holds `ids[start[i]..start[i + 1]]`; a box spanning several cells is in each of them.
    start: Vec<u32>,
    ids: Vec<u32>,
}

const MAX_CELLS: usize = 1 << 16;

impl RayIndex {
    pub fn new(world: &World) -> Self {
        let items: Vec<Item> = world.query::<Row>().iter().map(Item::new).collect();
        let boxes: Vec<Rect> = items.iter().map(Item::bounds).collect();
        let (mut lo, mut hi) = ((f32::MAX, f32::MAX), (f32::MIN, f32::MIN));
        for r in &boxes {
            lo = (lo.0.min(r.x), lo.1.min(r.y));
            hi = (hi.0.max(r.x + r.w), hi.1.max(r.y + r.h));
        }
        if items.is_empty() {
            (lo, hi) = ((0.0, 0.0), (1.0, 1.0));
        }
        // A cell about as big as a typical box keeps both the boxes per cell and the cells per box small.
        let mean = boxes.iter().map(|r| r.w.max(r.h)).sum::<f32>() / boxes.len().max(1) as f32;
        let mut cell = mean.max(1.0);
        let dims = |cell: f32| {
            (((hi.0 - lo.0) / cell).ceil().max(1.0) as usize, ((hi.1 - lo.1) / cell).ceil().max(1.0) as usize)
        };
        while dims(cell).0 * dims(cell).1 > MAX_CELLS {
            cell *= 2.0;
        }
        let (cols, rows) = dims(cell);
        let mut index =
            RayIndex { items, origin: lo, cell, cols, rows, start: vec![0; cols * rows + 1], ids: Vec::new() };
        let spans: Vec<_> = boxes.iter().map(|r| index.span(r)).collect();
        for &(x0, y0, x1, y1) in &spans {
            for (x, y) in (y0..=y1).flat_map(|y| (x0..=x1).map(move |x| (x, y))) {
                index.start[y * cols + x + 1] += 1;
            }
        }
        for i in 0..cols * rows {
            index.start[i + 1] += index.start[i];
        }
        let mut fill = index.start.clone();
        index.ids = vec![0; index.start[cols * rows] as usize];
        for (id, &(x0, y0, x1, y1)) in spans.iter().enumerate() {
            for (x, y) in (y0..=y1).flat_map(|y| (x0..=x1).map(move |x| (x, y))) {
                index.ids[fill[y * cols + x] as usize] = id as u32;
                fill[y * cols + x] += 1;
            }
        }
        index
    }

    // The columns and rows a rectangle covers, as (first column, first row, last column, last row).
    fn span(&self, r: &Rect) -> (usize, usize, usize, usize) {
        let at = |v: f32, o: f32, n: usize| (((v - o) / self.cell).floor().max(0.0) as usize).min(n - 1);
        let (x0, y0) = (at(r.x, self.origin.0, self.cols), at(r.y, self.origin.1, self.rows));
        (x0, y0, at(r.x + r.w, self.origin.0, self.cols), at(r.y + r.h, self.origin.1, self.rows))
    }

    /// Same answer as `raycast` over the world this was built from (the nearest of equal hits may be another one).
    pub fn cast(&self, from: impl Into<Vec2>, dir: impl Into<Vec2>, max: f32, mask: u32) -> Option<RayHit> {
        let (from, dir) = (v2(from), v2(dir));
        let (w, h) = (self.cols as f32 * self.cell, self.rows as f32 * self.cell);
        let (ox, oy) = (from.0 - self.origin.0, from.1 - self.origin.1);
        // Walk only the part of the ray inside the grid.
        let enter = slab((ox, dir.0, 0.0, w), (oy, dir.1, 0.0, h), max)?;
        let (px, py) = (ox + dir.0 * enter, oy + dir.1 * enter);
        let mut cx = ((px / self.cell).floor().max(0.0) as usize).min(self.cols - 1);
        let mut cy = ((py / self.cell).floor().max(0.0) as usize).min(self.rows - 1);
        // Per axis: the t of the next cell border and the t it takes to cross one cell.
        let axis = |o: f32, d: f32, c: usize| match d {
            d if d > 0.0 => (((c + 1) as f32 * self.cell - o) / d, self.cell / d, 1),
            d if d < 0.0 => ((c as f32 * self.cell - o) / d, -self.cell / d, -1),
            _ => (f32::INFINITY, f32::INFINITY, 0),
        };
        let ((mut tx, dx, sx), (mut ty, dy, sy)) = (axis(ox, dir.0, cx), axis(oy, dir.1, cy));
        let mut best: Option<(Entity, f32)> = None;
        loop {
            let cell = cy * self.cols + cx;
            for &id in &self.ids[self.start[cell] as usize..self.start[cell + 1] as usize] {
                let item = &self.items[id as usize];
                if item.group & mask == 0 {
                    continue;
                }
                if let Some(t) = item.hit(from, dir, best.map_or(max, |b| b.1)) {
                    best = Some((item.e, t));
                }
            }
            let exit = tx.min(ty);
            if best.is_some_and(|b| b.1 <= exit) || exit > max {
                return best.map(hit);
            }
            let (c, n, step, t, d) =
                if tx < ty { (&mut cx, self.cols, sx, &mut tx, dx) } else { (&mut cy, self.rows, sy, &mut ty, dy) };
            let next = *c as i32 + step;
            if next < 0 || next >= n as i32 {
                return best.map(hit);
            }
            (*c, *t) = (next as usize, *t + d);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Rng;

    fn boxed(w: &mut World, x: f32, collide: bool) -> Entity {
        let e = w.spawn((Body(Rect { x, y: 0.0, w: 10.0, h: 10.0 }),));
        if collide {
            w.insert_one(e, Collider).unwrap();
        }
        e
    }

    #[test]
    fn a_ray_hits_the_nearest_collider_in_front_and_stops_at_max() {
        let mut w = World::new();
        let near = boxed(&mut w, 20.0, true);
        boxed(&mut w, 50.0, true);
        boxed(&mut w, -30.0, true);
        boxed(&mut w, 80.0, false);
        let h = raycast(&w, (0.0, 5.0), (1.0, 0.0), 100.0, u32::MAX).unwrap();
        assert_eq!(h, RayHit { entity: near, t: 20.0 });
        assert!(raycast(&w, (0.0, 5.0), (1.0, 0.0), 10.0, u32::MAX).is_none());
        assert!(raycast(&w, (0.0, 50.0), (1.0, 0.0), 100.0, u32::MAX).is_none());
        assert_eq!(raycast(&w, (25.0, 5.0), (0.0, 1.0), 5.0, u32::MAX).map(|h| h.t), Some(0.0));
    }

    #[test]
    fn a_ray_skips_groups_outside_its_mask_and_sees_a_turned_box_where_it_is() {
        let mut w = World::new();
        let wall = boxed(&mut w, 20.0, true);
        w.insert_one(wall, Filter { group: 2, mask: u32::MAX }).unwrap();
        assert!(raycast(&w, (0.0, 5.0), (1.0, 0.0), 100.0, 1).is_none());
        assert_eq!(raycast(&w, (0.0, 5.0), (1.0, 0.0), 100.0, 2).map(|h| h.entity), Some(wall));
        // a 10x10 box turned 45 degrees about (25, 5) reaches 7.07 either side of it, so a ray along y = 5 meets it at 17.9
        let turned = Transform { angle: std::f32::consts::FRAC_PI_4, ..Transform::IDENTITY };
        w.insert_one(wall, turned).unwrap();
        let t = raycast(&w, (0.0, 5.0), (1.0, 0.0), 100.0, 2).unwrap().t;
        assert!((t - 17.93).abs() < 0.01, "{t}");
        // along y = 0.5 the unturned box would be met at 20; the diamond is only 2.57 wide either side there
        let t = raycast(&w, (0.0, 0.5), (1.0, 0.0), 100.0, 2).unwrap().t;
        assert!((t - 22.43).abs() < 0.01, "{t}");
    }

    #[test]
    fn the_grid_answers_like_a_scan_for_random_boxes_turns_groups_and_rays() {
        let (mut rng, mut w) = (Rng::new(7), World::new());
        for _ in 0..300 {
            let rect = Rect {
                x: rng.range(-200.0, 800.0),
                y: rng.range(-200.0, 600.0),
                w: rng.range(2.0, 60.0),
                h: rng.range(2.0, 30.0),
            };
            let turn = Transform { angle: rng.range(0.0, 6.3), ..Transform::IDENTITY };
            let group = 1 << (rng.range(0.0, 3.0) as u32);
            w.spawn((Body(rect), Collider, turn, Filter { group, mask: u32::MAX }));
        }
        let index = RayIndex::new(&w);
        let mut hits = 0;
        for i in 0..2000 {
            let from = (rng.range(-300.0, 900.0), rng.range(-300.0, 700.0));
            let a = rng.range(0.0, 6.3);
            let dir = match i % 9 {
                0 => (1.0, 0.0),
                1 => (0.0, -1.0),
                _ => (a.cos() * rng.range(0.5, 3.0), a.sin()),
            };
            let (max, mask) = (rng.range(10.0, 400.0), 1 << (i % 3));
            let (scan, grid) = (raycast(&w, from, dir, max, mask), index.cast(from, dir, max, mask));
            assert_eq!(scan.map(|h| h.t), grid.map(|h| h.t), "ray {i}: {from:?} {dir:?} {max}");
            hits += scan.is_some() as u32;
        }
        assert!((300..1700).contains(&hits), "{hits} of 2000 rays hit, so the test checks too little");
    }

    #[test]
    fn raycasts_agree_with_raycast_below_and_above_the_index_threshold() {
        let (mut rng, mut w) = (Rng::new(3), World::new());
        for _ in 0..100 {
            let rect = Rect { x: rng.range(0.0, 600.0), y: rng.range(0.0, 400.0), w: 20.0, h: 20.0 };
            w.spawn((Body(rect), Collider));
        }
        let rays: Vec<_> = (0..40)
            .map(|i| {
                let a = i as f32 * 0.157;
                Ray::new((300.0, 200.0), (a.cos(), a.sin()), 300.0, u32::MAX)
            })
            .collect();
        for n in [0, 1, INDEX_FROM - 1, INDEX_FROM, 40] {
            let got = raycasts(&w, &rays[..n]);
            assert_eq!(got.len(), n);
            for (r, h) in rays.iter().zip(&got) {
                assert_eq!(h.map(|h| h.t), raycast(&w, r.from, r.dir, r.max, r.mask).map(|h| h.t));
            }
        }
    }

    #[test]
    fn an_empty_grid_and_a_ray_missing_the_grid_find_nothing() {
        let mut w = World::new();
        assert!(RayIndex::new(&w).cast((0.0, 0.0), (1.0, 0.0), 100.0, u32::MAX).is_none());
        boxed(&mut w, 0.0, true);
        let index = RayIndex::new(&w);
        assert!(index.cast((0.0, 100.0), (1.0, 0.0), 1000.0, u32::MAX).is_none());
        assert!(index.cast((-50.0, 5.0), (-1.0, 0.0), 1000.0, u32::MAX).is_none());
        assert_eq!(index.cast((-50.0, 5.0), (1.0, 0.0), 1000.0, u32::MAX).map(|h| h.t), Some(50.0));
    }
}
