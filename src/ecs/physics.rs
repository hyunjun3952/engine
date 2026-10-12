use super::collision::overlapping_pairs;
use crate::{Body, Entity, Rect, Tilemap, Transform, Vel, World};
use std::collections::{HashMap, HashSet};

const REST_SPEED: f32 = 30.0;
const SLOP: f32 = 0.02;
const CORRECTION: f32 = 0.8;
const CORRECTION_PASSES: usize = 4;
const WARM_RANGE: f32 = 4.0;
const SAME_NORMAL: f32 = 0.95;
const MAX_SUBSTEPS: usize = 16;

pub struct RigidBody {
    inv_mass: f32,
    circle: bool,
    pub restitution: f32,
    pub friction: f32,
    pub spin: f32,
}

impl RigidBody {
    pub fn dynamic(mass: f32) -> Self {
        assert!(mass > 0.0, "a dynamic body needs a positive mass");
        RigidBody { inv_mass: 1.0 / mass, circle: false, restitution: 0.0, friction: 0.4, spin: 0.0 }
    }

    pub fn fixed() -> Self {
        RigidBody { inv_mass: 0.0, circle: false, restitution: 0.0, friction: 0.4, spin: 0.0 }
    }

    pub fn circle(mut self) -> Self {
        self.circle = true;
        self
    }

    pub fn with_bounce(mut self, restitution: f32) -> Self {
        self.restitution = restitution;
        self
    }

    pub fn with_friction(mut self, friction: f32) -> Self {
        self.friction = friction;
        self
    }

    pub fn with_spin(mut self, radians_per_sec: f32) -> Self {
        self.spin = radians_per_sec;
        self
    }
}

// A contact's normal and, per point, where it was and the impulses it ended with.
struct Memory {
    normal: V2,
    pts: Vec<(V2, f32, f32)>,
}

// Keeps the centres of two bodies `length` apart (a rod) or at most that far apart (a rope).
struct Joint {
    a: Entity,
    b: Entity,
    length: f32,
    rope: bool,
}

pub struct Physics {
    pub gravity: (f32, f32),
    pub iterations: usize,
    // Last step's contact impulses per body pair, replayed first so resting piles start the solve near its answer.
    cache: HashMap<(u64, u64), Memory>,
    joints: Vec<Joint>,
}

impl Default for Physics {
    fn default() -> Self {
        Physics { gravity: (0.0, 900.0), iterations: 10, cache: HashMap::new(), joints: Vec::new() }
    }
}

type V2 = (f32, f32);

struct Node {
    rect: Rect,
    before: Rect,
    at: V2,
    vel: V2,
    spin: f32,
    angle: f32,
    inv_mass: f32,
    inv_inertia: f32,
    circle: bool,
    restitution: f32,
    friction: f32,
}

impl Node {
    fn center(&self) -> V2 {
        (self.rect.x + self.rect.w / 2.0, self.rect.y + self.rect.h / 2.0)
    }

    fn before_center(&self) -> V2 {
        (self.before.x + self.before.w / 2.0, self.before.y + self.before.h / 2.0)
    }

    fn radius(&self) -> f32 {
        self.rect.w.min(self.rect.h) / 2.0
    }

    fn half(&self) -> V2 {
        (self.rect.w / 2.0, self.rect.h / 2.0)
    }

    // Local x and y axes in world space.
    fn axes(&self) -> (V2, V2) {
        let (s, c) = self.angle.sin_cos();
        ((c, s), (-s, c))
    }

    fn bounds(&self) -> Rect {
        if self.angle == 0.0 || self.circle {
            return self.rect;
        }
        let ((ux, uy), _) = self.axes();
        let (hw, hh) = self.half();
        let (ex, ey) = (hw * ux.abs() + hh * uy.abs(), hw * uy.abs() + hh * ux.abs());
        let (cx, cy) = self.center();
        Rect { x: cx - ex, y: cy - ey, w: ex * 2.0, h: ey * 2.0 }
    }

    fn velocity_at(&self, r: V2) -> V2 {
        (self.vel.0 - self.spin * r.1, self.vel.1 + self.spin * r.0)
    }

    fn push(&mut self, impulse: V2, r: V2, sign: f32) {
        self.vel = (self.vel.0 + sign * impulse.0 * self.inv_mass, self.vel.1 + sign * impulse.1 * self.inv_mass);
        self.spin += sign * cross(r, impulse) * self.inv_inertia;
    }
}

fn cross(a: V2, b: V2) -> f32 {
    a.0 * b.1 - a.1 * b.0
}

fn dot(a: V2, b: V2) -> f32 {
    a.0 * b.0 + a.1 * b.1
}

struct Pt {
    at: V2,
    depth: f32,
    jn: f32,
    jt: f32,
    bounce: f32,
}

impl Pt {
    fn new(at: V2, depth: f32) -> Self {
        Pt { at, depth, jn: 0.0, jt: 0.0, bounce: 0.0 }
    }
}

struct Hit {
    a: usize,
    b: usize,
    normal: V2,
    pts: Vec<Pt>,
    key: (u64, u64),
}

// Tiles have no entity, so they are named by where they are; the top bit keeps the two kinds apart.
fn node_id(ents: &[Entity], nodes: &[Node], i: usize) -> u64 {
    ents.get(i).map_or_else(
        || 1 << 63 | (nodes[i].rect.x as i32 as u32 as u64) << 32 | nodes[i].rect.y as i32 as u32 as u64,
        |e| e.to_bits().get(),
    )
}

impl Physics {
    /// A rigid rod between the centres of `a` and `b`. Torque is ignored: the ends are the centres.
    pub fn rod(&mut self, a: Entity, b: Entity, length: f32) {
        self.joints.push(Joint { a, b, length, rope: false });
    }

    /// Like `rod`, but the bodies may come closer than `length`.
    pub fn rope(&mut self, a: Entity, b: Entity, length: f32) {
        self.joints.push(Joint { a, b, length, rope: true });
    }

    pub fn clear_joints(&mut self) {
        self.joints.clear();
    }

    pub fn step(&mut self, world: &mut World, tiles: Option<&Tilemap>, dt: f32) {
        let (ents, mut nodes) = gather(world);
        let bodies = nodes.len();
        let substeps = self.substeps(&nodes, dt);
        let at = |e| ents.iter().position(|&x| x == e);
        let links: Vec<Link> = self
            .joints
            .iter()
            .filter_map(|j| Some(Link { a: at(j.a)?, b: at(j.b)?, length: j.length, rope: j.rope }))
            .collect();
        for _ in 0..substeps {
            nodes.truncate(bodies);
            self.substep(&ents, &mut nodes, &links, tiles, dt / substeps as f32);
        }
        write_back(world, &ents, &nodes);
    }

    // A body that crosses more than half its own size in one step can pass through a thin wall, so the whole
    // step is cut into shorter ones until none does; the cap keeps a stray huge velocity from freezing the game.
    fn substeps(&self, nodes: &[Node], dt: f32) -> usize {
        let gravity = self.gravity.0.hypot(self.gravity.1) * dt;
        let worst = nodes
            .iter()
            .filter(|n| n.inv_mass > 0.0)
            .map(|n| (n.vel.0.hypot(n.vel.1) + gravity) * dt / (n.rect.w.min(n.rect.h) / 2.0))
            .fold(0.0, f32::max);
        (worst.ceil() as usize).clamp(1, MAX_SUBSTEPS)
    }

    fn substep(&mut self, ents: &[Entity], nodes: &mut Vec<Node>, links: &[Link], tiles: Option<&Tilemap>, dt: f32) {
        let bodies = ents.len();
        self.integrate(nodes, dt);
        if let Some(map) = tiles {
            add_tiles(nodes, bodies, map);
        }
        let mut hits = find_hits(nodes, bodies, tiles);
        for n in nodes.iter_mut() {
            n.at = n.center();
        }
        for hit in &mut hits {
            hit.key = (node_id(ents, nodes, hit.a), node_id(ents, nodes, hit.b));
            let (a, b) = pair_mut(nodes, hit.a, hit.b);
            prepare(a, b, hit);
        }
        // After every contact has measured its bounce: replayed impulses change the velocities it reads.
        for hit in &mut hits {
            let (a, b) = pair_mut(nodes, hit.a, hit.b);
            warm_start(a, b, hit, self.cache.get(&hit.key));
        }
        for _ in 0..self.iterations {
            for hit in &mut hits {
                let (a, b) = pair_mut(nodes, hit.a, hit.b);
                solve(a, b, hit);
            }
            for l in links {
                let (a, b) = pair_mut(nodes, l.a, l.b);
                pull(a, b, l);
            }
        }
        for _ in 0..CORRECTION_PASSES {
            for hit in &hits {
                let (a, b) = pair_mut(nodes, hit.a, hit.b);
                separate(a, b, hit);
            }
            for l in links {
                let (a, b) = pair_mut(nodes, l.a, l.b);
                keep_length(a, b, l);
            }
        }
        self.cache = hits
            .iter()
            .map(|h| {
                let pts = h.pts.iter().map(|p| (p.at, p.jn, p.jt)).collect();
                (h.key, Memory { normal: h.normal, pts })
            })
            .collect();
    }

    fn integrate(&self, nodes: &mut [Node], dt: f32) {
        for n in nodes.iter_mut().filter(|n| n.inv_mass > 0.0) {
            n.before = n.rect;
            n.vel.0 += self.gravity.0 * dt;
            n.vel.1 += self.gravity.1 * dt;
            n.rect.x += n.vel.0 * dt;
            n.rect.y += n.vel.1 * dt;
            n.angle += n.spin * dt;
        }
    }
}

fn gather(world: &World) -> (Vec<Entity>, Vec<Node>) {
    let mut query = world.query::<(Entity, &Body, Option<&Vel>, &RigidBody, Option<&Transform>)>();
    query
        .iter()
        .map(|(e, body, vel, rb, xform)| {
            let movable = vel.is_some() && rb.inv_mass > 0.0;
            let inv_mass = if movable { rb.inv_mass } else { 0.0 };
            let (w, h) = (body.0.w, body.0.h);
            let inertia = if rb.circle { 0.5 * w.min(h).powi(2) / 4.0 } else { (w * w + h * h) / 12.0 } / inv_mass;
            let rotates = movable && xform.is_some();
            let node = Node {
                rect: body.0,
                before: body.0,
                at: (0.0, 0.0),
                vel: vel.map_or((0.0, 0.0), |v| (v.x, v.y)),
                spin: if rotates { rb.spin } else { 0.0 },
                angle: xform.map_or(0.0, |t| t.angle),
                inv_mass,
                inv_inertia: if rotates { 1.0 / inertia } else { 0.0 },
                circle: rb.circle,
                restitution: rb.restitution,
                friction: rb.friction,
            };
            (e, node)
        })
        .unzip()
}

fn add_tiles(nodes: &mut Vec<Node>, bodies: usize, map: &Tilemap) {
    let mut seen = HashSet::new();
    for i in 0..bodies {
        if nodes[i].inv_mass == 0.0 {
            continue;
        }
        let r = nodes[i].bounds();
        let near = Rect { x: r.x - 1.0, y: r.y - 1.0, w: r.w + 2.0, h: r.h + 2.0 };
        for tile in map.solid_in(near) {
            if seen.insert((tile.x as i32, tile.y as i32)) {
                nodes.push(Node {
                    rect: tile,
                    before: tile,
                    at: (0.0, 0.0),
                    vel: (0.0, 0.0),
                    spin: 0.0,
                    angle: 0.0,
                    inv_mass: 0.0,
                    inv_inertia: 0.0,
                    circle: false,
                    restitution: 0.0,
                    friction: 0.4,
                });
            }
        }
    }
}

fn find_hits(nodes: &[Node], bodies: usize, tiles: Option<&Tilemap>) -> Vec<Hit> {
    let bounds: Vec<Rect> = nodes.iter().map(Node::bounds).collect();
    overlapping_pairs(&bounds)
        .into_iter()
        .filter(|&(a, b, ..)| nodes[a].inv_mass + nodes[b].inv_mass > 0.0)
        // A tile always goes second, which is where the seam filters below expect it.
        .map(|(a, b, ..)| if a >= bodies && b < bodies { (b, a) } else { (a, b) })
        .filter_map(|(a, b)| {
            let mut touch = touch(&nodes[a], &nodes[b], false)?;
            if let Some(map) = tiles.filter(|_| b >= bodies) {
                let tile = &nodes[b].rect;
                if touch.whole_face {
                    let (nx, ny) = touch.normal;
                    let behind = Rect {
                        x: tile.x + tile.w / 2.0 - nx * tile.w,
                        y: tile.y + tile.h / 2.0 - ny * tile.h,
                        ..*tile
                    };
                    if is_solid(map, behind.x, behind.y) {
                        return None;
                    }
                } else {
                    touch.pts.retain(|p| !is_hidden(map, tile, p.at, touch.normal));
                    if touch.pts.is_empty() && !nodes[a].circle {
                        // The body's face met a seam corner of the floor; its vertex hitting the tile's face is the real contact.
                        touch = self::touch(&nodes[a], &nodes[b], true)?;
                        touch.pts.retain(|p| !is_hidden(map, tile, p.at, touch.normal));
                    }
                }
            }
            (!touch.pts.is_empty()).then_some(Hit { a, b, normal: touch.normal, pts: touch.pts, key: (0, 0) })
        })
        .collect()
}

fn is_solid(map: &Tilemap, x: f32, y: f32) -> bool {
    !map.solid_in(Rect { x, y, w: 0.1, h: 0.1 }).is_empty()
}

// A tile face or corner that a neighbouring solid tile hides cannot be touched; a body sliding along the
// floor would otherwise catch on the seam, because the resting overlap of 0.02 px makes the seam look like a wall.
// A neighbour only hides the point when the body is on that neighbour's side of it (`toward` is tile to body).
fn is_hidden(map: &Tilemap, tile: &Rect, (px, py): V2, normal: V2) -> bool {
    let (mx, my) = (tile.x + tile.w / 2.0, tile.y + tile.h / 2.0);
    let (dx, dy) = (px - mx, py - my);
    let toward = (-normal.0, -normal.1);
    let blocked = |d: f32, t: f32, half: f32| d.abs() >= half - 0.05 && d.signum() * t > 1e-6;
    (blocked(dx, toward.0, tile.w / 2.0) && is_solid(map, mx + dx.signum() * tile.w, my))
        || (blocked(dy, toward.1, tile.h / 2.0) && is_solid(map, mx, my + dy.signum() * tile.h))
}

struct Touch {
    normal: V2,
    pts: Vec<Pt>,
    // Axis-aligned boxes: the normal alone says which tile face is touched.
    whole_face: bool,
}

// Normal points from a to b. Untilted boxes use where they were before the step (see `hit_along_x`);
// everything else only has current positions, so a fast circle or tilted box can still tunnel.
fn touch(a: &Node, b: &Node, tile_axes_only: bool) -> Option<Touch> {
    match (a.circle, b.circle) {
        (false, false) if a.angle == 0.0 && b.angle == 0.0 => axis_aligned(a, b),
        (false, false) => tilted_boxes(a, b, tile_axes_only),
        (true, true) => {
            let (ca, cb) = (a.center(), b.center());
            let (dx, dy) = (cb.0 - ca.0, cb.1 - ca.1);
            let reach = a.radius() + b.radius();
            let dist = (dx * dx + dy * dy).sqrt();
            if dist >= reach {
                return None;
            }
            let normal = if dist > 1e-6 { (dx / dist, dy / dist) } else { (0.0, 1.0) };
            let depth = reach - dist;
            let at = (ca.0 + normal.0 * (a.radius() - depth / 2.0), ca.1 + normal.1 * (a.radius() - depth / 2.0));
            Some(Touch { normal, pts: vec![Pt::new(at, depth)], whole_face: false })
        }
        (true, false) => circle_box(a, b).map(|t| Touch { normal: (-t.normal.0, -t.normal.1), ..t }),
        (false, true) => circle_box(b, a),
    }
}

fn axis_aligned(a: &Node, b: &Node) -> Option<Touch> {
    let (ox, oy) = a.rect.penetration(&b.rect)?;
    let along_x = hit_along_x(&a.before, &b.before, ox, oy);
    let (x0, x1) = (a.rect.x.max(b.rect.x), (a.rect.x + a.rect.w).min(b.rect.x + b.rect.w));
    let (y0, y1) = (a.rect.y.max(b.rect.y), (a.rect.y + a.rect.h).min(b.rect.y + b.rect.h));
    let (normal, depth, pts) = if along_x {
        let side = (a.before.x, a.before.x + a.before.w, b.before.x, b.before.x + b.before.w);
        let at = (x0 + x1) / 2.0;
        ((push_direction(side, a.center().0, b.center().0), 0.0), ox, [(at, y0), (at, y1)])
    } else {
        let side = (a.before.y, a.before.y + a.before.h, b.before.y, b.before.y + b.before.h);
        let at = (y0 + y1) / 2.0;
        ((0.0, push_direction(side, a.center().1, b.center().1)), oy, [(x0, at), (x1, at)])
    };
    Some(Touch { normal, pts: pts.map(|at| Pt::new(at, depth)).into(), whole_face: true })
}

// Separating axis test over the four face normals, then clipping the incident edge against the reference face.
fn tilted_boxes(a: &Node, b: &Node, only_b_axes: bool) -> Option<Touch> {
    let (ca, cb) = (a.center(), b.center());
    let d = (cb.0 - ca.0, cb.1 - ca.1);
    let (aa, ab) = (a.axes(), b.axes());
    let (ha, hb) = (a.half(), b.half());
    let reach = |axes: (V2, V2), h: V2, n: V2| h.0 * dot(axes.0, n).abs() + h.1 * dot(axes.1, n).abs();
    let mut best = [(f32::MAX, 0usize); 2];
    for (owner, axes) in [aa, ab].into_iter().enumerate() {
        for (i, n) in [axes.0, axes.1].into_iter().enumerate() {
            let pen = reach(aa, ha, n) + reach(ab, hb, n) - dot(d, n).abs();
            if pen <= 0.0 {
                return None;
            }
            if pen < best[owner].0 {
                best[owner] = (pen, i);
            }
        }
    }
    let ref_is_b = only_b_axes || best[1].0 < 0.95 * best[0].0 - 0.01;
    let (rn, inc, axes_r, axes_i, half_r) = if ref_is_b { (&b, &a, ab, aa, hb) } else { (&a, &b, aa, ab, ha) };
    let pick = |axes: (V2, V2), i: usize| if i == 0 { axes.0 } else { axes.1 };
    let i = best[ref_is_b as usize].1;
    let axis = pick(axes_r, i);
    let toward_inc = if ref_is_b { (-d.0, -d.1) } else { d };
    // Outward normal of the reference face, pointing at the incident box.
    let nr = if dot(axis, toward_inc) < 0.0 { (-axis.0, -axis.1) } else { axis };
    let normal = if ref_is_b { (-nr.0, -nr.1) } else { nr };
    let (face_half, side_half) = if i == 0 { (half_r.0, half_r.1) } else { (half_r.1, half_r.0) };
    let (cr, ci) = (rn.center(), inc.center());
    let t = (-nr.1, nr.0);

    let faces = [(axes_i.0, 0), (axes_i.1, 1)];
    let (fi, k) = faces
        .into_iter()
        .flat_map(|(n, k)| [(n, k), ((-n.0, -n.1), k)])
        .min_by(|x, y| dot(x.0, nr).total_cmp(&dot(y.0, nr)))
        .unwrap();
    let half_i = inc.half();
    let (along, across) = if k == 0 { (half_i.0, half_i.1) } else { (half_i.1, half_i.0) };
    let edge = (-fi.1, fi.0);
    let mid = (ci.0 + fi.0 * along, ci.1 + fi.1 * along);
    let mut seg =
        [(mid.0 + edge.0 * across, mid.1 + edge.1 * across), (mid.0 - edge.0 * across, mid.1 - edge.1 * across)];
    for sign in [1.0, -1.0] {
        let dist = |p: V2| side_half - sign * dot((p.0 - cr.0, p.1 - cr.1), t);
        let (d0, d1) = (dist(seg[0]), dist(seg[1]));
        if d0 < 0.0 && d1 < 0.0 {
            return None;
        }
        if d0 < 0.0 || d1 < 0.0 {
            let (keep, cut, dk, dc) = if d0 < 0.0 { (1, 0, d1, d0) } else { (0, 1, d0, d1) };
            let f = dk / (dk - dc);
            let (pk, pc) = (seg[keep], seg[cut]);
            seg[cut] = (pk.0 + (pc.0 - pk.0) * f, pk.1 + (pc.1 - pk.1) * f);
        }
    }
    let pts: Vec<Pt> = seg
        .into_iter()
        .map(|p| Pt::new(p, face_half - dot((p.0 - cr.0, p.1 - cr.1), nr)))
        .filter(|p| p.depth > 0.0)
        .collect();
    (!pts.is_empty()).then_some(Touch { normal, pts, whole_face: false })
}

// Normal points from the box to the circle; works in the box's own frame so a tilted box needs no special case.
fn circle_box(circle: &Node, boxed: &Node) -> Option<Touch> {
    let (cc, bc, r) = (circle.center(), boxed.center(), circle.radius());
    let (ux, uy) = boxed.axes();
    let w = (cc.0 - bc.0, cc.1 - bc.1);
    let (lx, ly) = (dot(w, ux), dot(w, uy));
    let (hw, hh) = boxed.half();
    let (qx, qy) = (lx.clamp(-hw, hw), ly.clamp(-hh, hh));
    let (dx, dy) = (lx - qx, ly - qy);
    let d2 = dx * dx + dy * dy;
    let (n, depth, q) = if d2 > 0.0 {
        if d2 >= r * r {
            return None;
        }
        let dist = d2.sqrt();
        ((dx / dist, dy / dist), r - dist, (qx, qy))
    } else {
        // A centre that ended up inside came from outside: leave by the side it was on before the step.
        let ((px, py), was) = (circle.before_center(), boxed.before);
        let flat = boxed.angle == 0.0;
        let exits = [
            (lx + hw, (-1.0, 0.0), (-hw, ly), flat && px < was.x),
            (hw - lx, (1.0, 0.0), (hw, ly), flat && px > was.x + was.w),
            (ly + hh, (0.0, -1.0), (lx, -hh), flat && py < was.y),
            (hh - ly, (0.0, 1.0), (lx, hh), flat && py > was.y + was.h),
        ];
        let nearest = exits.iter().min_by(|x, y| x.0.total_cmp(&y.0)).unwrap();
        let &(inside, n, q, _) = exits.iter().find(|e| e.3).unwrap_or(nearest);
        (n, r + inside, q)
    };
    let world = |v: V2| (v.0 * ux.0 + v.1 * uy.0, v.0 * ux.1 + v.1 * uy.1);
    let (nw, qw) = (world(n), world(q));
    Some(Touch { normal: nw, pts: vec![Pt::new((bc.0 + qw.0, bc.1 + qw.1), depth)], whole_face: false })
}

fn hit_along_x(a: &Rect, b: &Rect, ox: f32, oy: f32) -> bool {
    let gap_x = (b.x - (a.x + a.w)).max(a.x - (b.x + b.w));
    let gap_y = (b.y - (a.y + a.h)).max(a.y - (b.y + b.h));
    if gap_x > 0.0 || gap_y > 0.0 { gap_x > gap_y } else { ox < oy }
}

fn push_direction((a_lo, a_hi, b_lo, b_hi): (f32, f32, f32, f32), now_a: f32, now_b: f32) -> f32 {
    if b_lo >= a_hi {
        1.0
    } else if b_hi <= a_lo {
        -1.0
    } else if now_b >= now_a {
        1.0
    } else {
        -1.0
    }
}

fn pair_mut(nodes: &mut [Node], a: usize, b: usize) -> (&mut Node, &mut Node) {
    if a < b {
        let (left, right) = nodes.split_at_mut(b);
        (&mut left[a], &mut right[0])
    } else {
        let (left, right) = nodes.split_at_mut(a);
        (&mut right[0], &mut left[b])
    }
}

fn arms(a: &Node, b: &Node, at: V2) -> (V2, V2) {
    let (ca, cb) = (a.center(), b.center());
    ((at.0 - ca.0, at.1 - ca.1), (at.0 - cb.0, at.1 - cb.1))
}

// Effective mass of a contact along `dir`: both linear masses plus how much each body's spin resists.
fn stiffness(a: &Node, b: &Node, ra: V2, rb: V2, dir: V2) -> f32 {
    a.inv_mass + b.inv_mass + cross(ra, dir).powi(2) * a.inv_inertia + cross(rb, dir).powi(2) * b.inv_inertia
}

// How fast the bodies close along the normal when contact starts decides the bounce, once, before any impulse.
fn prepare(a: &Node, b: &Node, hit: &mut Hit) {
    let bounce = a.restitution.min(b.restitution);
    for pt in &mut hit.pts {
        let (ra, rb) = arms(a, b, pt.at);
        let (va, vb) = (a.velocity_at(ra), b.velocity_at(rb));
        let closing = -dot((vb.0 - va.0, vb.1 - va.1), hit.normal);
        pt.bounce = if closing < REST_SPEED { 0.0 } else { bounce * closing };
    }
}

// A point inherits the impulses of the nearest point this pair had last step, if one is still close.
fn warm_start(a: &mut Node, b: &mut Node, hit: &mut Hit, old: Option<&Memory>) {
    let n = hit.normal;
    let old = old.filter(|o| dot(o.normal, n) > SAME_NORMAL).map(|o| &o.pts);
    let t = (-n.1, n.0);
    for pt in &mut hit.pts {
        let dist = |o: &&(V2, f32, f32)| (o.0.0 - pt.at.0).hypot(o.0.1 - pt.at.1);
        let nearest = old.into_iter().flatten().min_by(|x, y| dist(x).total_cmp(&dist(y)));
        let Some(&(_, jn, jt)) = nearest.filter(|o| dist(o) < WARM_RANGE) else {
            continue;
        };
        let (ra, rb) = arms(a, b, pt.at);
        let impulse = (jn * n.0 + jt * t.0, jn * n.1 + jt * t.1);
        a.push(impulse, ra, -1.0);
        b.push(impulse, rb, 1.0);
        (pt.jn, pt.jt) = (jn, jt);
    }
}

fn solve(a: &mut Node, b: &mut Node, hit: &mut Hit) {
    let n = hit.normal;
    let t = (-n.1, n.0);
    let grip = (a.friction * b.friction).sqrt();
    for pt in &mut hit.pts {
        let (ra, rb) = arms(a, b, pt.at);
        let rel = |a: &Node, b: &Node| {
            let (va, vb) = (a.velocity_at(ra), b.velocity_at(rb));
            (vb.0 - va.0, vb.1 - va.1)
        };
        let lambda = (pt.bounce - dot(rel(a, b), n)) / stiffness(a, b, ra, rb, n);
        let total = (pt.jn + lambda).max(0.0);
        let impulse = ((total - pt.jn) * n.0, (total - pt.jn) * n.1);
        pt.jn = total;
        a.push(impulse, ra, -1.0);
        b.push(impulse, rb, 1.0);

        let lambda = -dot(rel(a, b), t) / stiffness(a, b, ra, rb, t);
        let total = (pt.jt + lambda).clamp(-grip * pt.jn, grip * pt.jn);
        let impulse = ((total - pt.jt) * t.0, (total - pt.jt) * t.1);
        pt.jt = total;
        a.push(impulse, ra, -1.0);
        b.push(impulse, rb, 1.0);
    }
}

// A joint resolved to node indices for one step.
struct Link {
    a: usize,
    b: usize,
    length: f32,
    rope: bool,
}

fn link_axis(a: &Node, b: &Node) -> Option<(V2, f32)> {
    let (ca, cb) = (a.center(), b.center());
    let (dx, dy) = (cb.0 - ca.0, cb.1 - ca.1);
    let dist = dx.hypot(dy);
    (dist > 1e-4).then_some(((dx / dist, dy / dist), dist))
}

// Cancels the speed at which the ends move apart (and, for a rod, together) along the joint.
fn pull(a: &mut Node, b: &mut Node, l: &Link) {
    let inv = a.inv_mass + b.inv_mass;
    let Some((n, dist)) = link_axis(a, b).filter(|_| inv > 0.0) else {
        return;
    };
    let along = dot((b.vel.0 - a.vel.0, b.vel.1 - a.vel.1), n);
    if l.rope && (dist <= l.length || along <= 0.0) {
        return;
    }
    let j = -along / inv;
    a.vel = (a.vel.0 - j * n.0 * a.inv_mass, a.vel.1 - j * n.1 * a.inv_mass);
    b.vel = (b.vel.0 + j * n.0 * b.inv_mass, b.vel.1 + j * n.1 * b.inv_mass);
}

fn keep_length(a: &mut Node, b: &mut Node, l: &Link) {
    let inv = a.inv_mass + b.inv_mass;
    let Some((n, dist)) = link_axis(a, b).filter(|_| inv > 0.0) else {
        return;
    };
    let error = if l.rope { (dist - l.length).max(0.0) } else { dist - l.length };
    let shift = error * CORRECTION / inv;
    a.rect.x += n.0 * shift * a.inv_mass;
    a.rect.y += n.1 * shift * a.inv_mass;
    b.rect.x -= n.0 * shift * b.inv_mass;
    b.rect.y -= n.1 * shift * b.inv_mass;
}

// Positional correction only moves bodies, not their angle; the contact's depth shrinks by how far they have
// moved together along the normal since it was found.
fn separate(a: &mut Node, b: &mut Node, hit: &Hit) {
    let (ca, cb) = (a.center(), b.center());
    let moved = (cb.0 - b.at.0 - (ca.0 - a.at.0), cb.1 - b.at.1 - (ca.1 - a.at.1));
    let closed = dot(moved, hit.normal);
    let depth = hit.pts.iter().map(|p| p.depth).fold(0.0, f32::max) - closed;
    let inv_sum = a.inv_mass + b.inv_mass;
    let shift = (depth - SLOP).max(0.0) * CORRECTION / inv_sum;
    let (nx, ny) = hit.normal;
    a.rect.x -= nx * shift * a.inv_mass;
    a.rect.y -= ny * shift * a.inv_mass;
    b.rect.x += nx * shift * b.inv_mass;
    b.rect.y += ny * shift * b.inv_mass;
}

fn write_back(world: &mut World, ents: &[Entity], nodes: &[Node]) {
    for (e, node) in ents.iter().zip(nodes).filter(|(_, n)| n.inv_mass > 0.0) {
        if let Ok((body, Some(vel), rb, xform)) =
            world.query_one_mut::<(&mut Body, Option<&mut Vel>, &mut RigidBody, Option<&mut Transform>)>(*e)
        {
            body.0 = node.rect;
            (vel.x, vel.y) = node.vel;
            if let Some(xform) = xform {
                xform.angle = node.angle;
                rb.spin = node.spin;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Sprite;
    use std::sync::Arc;

    const DT: f32 = 1.0 / 60.0;

    fn boxed(w: &mut World, rect: Rect, rb: RigidBody, vel: (f32, f32)) -> Entity {
        w.spawn((Body(rect), Vel { x: vel.0, y: vel.1 }, rb))
    }

    fn square(x: f32, y: f32, size: f32) -> Rect {
        Rect { x, y, w: size, h: size }
    }

    fn floor(w: &mut World) {
        w.spawn((Body(Rect { x: -500.0, y: 100.0, w: 2000.0, h: 50.0 }), RigidBody::fixed()));
    }

    fn state(w: &World, e: Entity) -> (Rect, (f32, f32)) {
        let body = w.get::<&Body>(e).unwrap().0;
        let v = w.get::<&Vel>(e).unwrap();
        (body, (v.x, v.y))
    }

    fn run(p: &mut Physics, w: &mut World, tiles: Option<&Tilemap>, steps: usize) {
        for _ in 0..steps {
            p.step(w, tiles, DT);
        }
    }

    #[test]
    fn free_fall_follows_semi_implicit_euler() {
        let (mut p, mut w) = (Physics::default(), World::new());
        let e = boxed(&mut w, square(0.0, 0.0, 40.0), RigidBody::dynamic(1.0), (0.0, 0.0));
        run(&mut p, &mut w, None, 60);
        let expected = 900.0 * DT * DT * (60.0 * 61.0 / 2.0);
        assert!((state(&w, e).0.y - expected).abs() < 0.01, "{}", state(&w, e).0.y);
    }

    #[test]
    fn a_box_dropped_on_the_floor_comes_to_rest_exactly_on_top_of_it() {
        let (mut p, mut w) = (Physics::default(), World::new());
        floor(&mut w);
        let e = boxed(&mut w, square(50.0, 60.0, 20.0), RigidBody::dynamic(1.0), (0.0, 0.0));
        run(&mut p, &mut w, None, 120);
        let (r, v) = state(&w, e);
        assert!((r.y + r.h - 100.0).abs() < 0.2, "bottom at {}", r.y + r.h);
        assert!(v.1.abs() < 1.0, "still moving at {}", v.1);
    }

    #[test]
    fn restitution_decides_how_much_speed_comes_back() {
        for bounce in [0.25_f32, 0.5, 0.9] {
            let (mut p, mut w) = (Physics::default(), World::new());
            w.spawn((Body(Rect { x: -500.0, y: 500.0, w: 2000.0, h: 50.0 }), RigidBody::fixed().with_bounce(bounce)));
            let e = boxed(&mut w, square(0.0, 0.0, 10.0), RigidBody::dynamic(1.0).with_bounce(bounce), (0.0, 0.0));
            let mut result = None;
            for _ in 0..200 {
                let before = state(&w, e).1.1;
                p.step(&mut w, None, DT);
                let after = state(&w, e).1.1;
                if before > 0.0 && after < 0.0 {
                    result = Some((before + 900.0 * DT, after));
                    break;
                }
            }
            let (incoming, outgoing) = result.expect("the box never bounced");
            assert!((outgoing / -incoming - bounce).abs() < 0.03, "bounce {bounce}: {outgoing} vs {incoming}");
        }
    }

    #[test]
    fn friction_stops_a_sliding_box_in_the_distance_coulomb_predicts() {
        let (mut p, mut w) = (Physics::default(), World::new());
        let rb = || RigidBody::dynamic(1.0).with_friction(0.5);
        w.spawn((Body(Rect { x: -500.0, y: 100.0, w: 2000.0, h: 50.0 }), RigidBody::fixed().with_friction(0.5)));
        let slow = boxed(&mut w, square(0.0, 80.0, 20.0), rb(), (100.0, 0.0));
        run(&mut p, &mut w, None, 90);
        let (r, v) = state(&w, slow);
        let expected = 100.0_f32.powi(2) / (2.0 * 0.5 * 900.0);
        assert!(v.0.abs() < 0.5, "still sliding at {}", v.0);
        assert!((r.x - expected).abs() < 1.5, "slid {} expected {expected}", r.x);

        let mut slick = World::new();
        slick.spawn((Body(Rect { x: -500.0, y: 100.0, w: 2000.0, h: 50.0 }), RigidBody::fixed().with_friction(0.0)));
        let e = boxed(&mut slick, square(0.0, 80.0, 20.0), RigidBody::dynamic(1.0).with_friction(0.0), (100.0, 0.0));
        run(&mut p, &mut slick, None, 90);
        assert!((state(&slick, e).1.0 - 100.0).abs() < 0.5);
    }

    #[test]
    fn a_hard_drop_onto_a_thin_floor_does_not_tunnel_through_it() {
        let (mut p, mut w) = (Physics::default(), World::new());
        w.spawn((Body(Rect { x: -500.0, y: 100.0, w: 2000.0, h: 20.0 }), RigidBody::fixed()));
        let plank = Rect { x: 0.0, y: -900.0, w: 6.0, h: 20.0 };
        let e = boxed(&mut w, plank, RigidBody::dynamic(1.0), (0.0, 0.0));
        run(&mut p, &mut w, None, 300);
        let (r, _) = state(&w, e);
        assert!((r.y + r.h - 100.0).abs() < 0.5, "ended at y {} (floor top is 100)", r.y);
    }

    #[test]
    fn a_pile_of_boxes_dropped_from_high_up_stays_inside_the_arena() {
        let (mut p, mut w) = (Physics::default(), World::new());
        let mut rng = crate::Rng::new(7);
        let wall = |x, y, w_, h| (Body(Rect { x, y, w: w_, h }), RigidBody::fixed());
        w.spawn(wall(0.0, 340.0, 640.0, 20.0));
        w.spawn(wall(0.0, 0.0, 10.0, 360.0));
        w.spawn(wall(630.0, 0.0, 10.0, 360.0));
        let boxes: Vec<Entity> = (0..36)
            .map(|i| {
                let size = rng.range(14.0, 34.0);
                let rect = Rect { x: rng.range(20.0, 620.0 - size), y: -20.0 * i as f32, w: size, h: size };
                let rb = RigidBody::dynamic(size * size / 100.0).with_bounce(rng.range(0.0, 0.7)).with_friction(0.5);
                boxed(&mut w, rect, rb, (0.0, 0.0))
            })
            .collect();
        run(&mut p, &mut w, None, 600);
        for e in boxes {
            let (r, _) = state(&w, e);
            assert!(r.x >= 9.0 && r.x + r.w <= 631.0, "left the arena sideways: {r:?}");
            assert!(r.y + r.h <= 341.0, "sank into or through the floor: {r:?}");
        }
    }

    #[test]
    fn a_body_walks_along_a_tile_floor_without_catching_on_the_seams() {
        let map = Tilemap::from_ascii(
            &["....................", "....................", "####################"],
            16,
            std::sync::Arc::new(crate::Sprite::load("assets/images/tiles.png")),
        );
        let (mut p, mut w) = (Physics::default(), World::new());
        let e = boxed(
            &mut w,
            Rect { x: 20.0, y: 0.0, w: 12.0, h: 24.0 },
            RigidBody::dynamic(1.0).with_friction(0.0),
            (0.0, 0.0),
        );
        for _ in 0..120 {
            w.query_one_mut::<&mut Vel>(e).unwrap().x = 100.0;
            p.step(&mut w, Some(&map), 1.0 / 60.0);
        }
        let (r, _) = state(&w, e);
        assert!(r.x > 219.0, "caught on a seam at {r:?}");
    }

    #[test]
    fn a_stack_of_boxes_settles_without_sinking_or_jittering() {
        let (mut p, mut w) = (Physics::default(), World::new());
        floor(&mut w);
        let stack: Vec<Entity> = (0..4)
            .map(|i| boxed(&mut w, square(50.0, 78.0 - 21.0 * i as f32, 20.0), RigidBody::dynamic(1.0), (0.0, 0.0)))
            .collect();
        run(&mut p, &mut w, None, 240);
        let settled: Vec<f32> = stack.iter().map(|e| state(&w, *e).0.y).collect();
        run(&mut p, &mut w, None, 60);
        for (i, e) in stack.iter().enumerate() {
            let (r, v) = state(&w, *e);
            let ideal = 80.0 - 20.0 * i as f32;
            assert!((r.y - ideal).abs() < 1.0, "box {i} at {} wants {ideal}", r.y);
            assert!((r.y - settled[i]).abs() < 0.05, "box {i} still drifting: {} -> {}", settled[i], r.y);
            assert!(v.1.abs() < 5.0 && v.0.abs() < 0.5, "box {i} moves at {v:?}");
        }
    }

    #[test]
    fn an_elastic_collision_conserves_momentum_and_energy_for_unequal_masses() {
        let mut p = Physics { gravity: (0.0, 0.0), ..Physics::default() };
        let mut w = World::new();
        let rb = |m| RigidBody::dynamic(m).with_bounce(1.0).with_friction(0.0);
        let heavy = boxed(&mut w, square(0.0, 0.0, 20.0), rb(3.0), (100.0, 0.0));
        let light = boxed(&mut w, square(60.0, 0.0, 20.0), rb(1.0), (-100.0, 0.0));
        let (momentum, energy) = (3.0 * 100.0 - 100.0, 0.5 * 3.0 * 100.0_f32.powi(2) + 0.5 * 100.0_f32.powi(2));
        run(&mut p, &mut w, None, 90);
        let (a, b) = (state(&w, heavy).1.0, state(&w, light).1.0);
        assert!((3.0 * a + b - momentum).abs() < 1.0, "momentum {}", 3.0 * a + b);
        assert!((0.5 * 3.0 * a * a + 0.5 * b * b - energy).abs() < energy * 0.01, "energy");
        assert!(a < 100.0 && b > 0.0, "they really did collide: {a} {b}");
    }

    #[test]
    fn a_heavy_box_drags_a_light_one_along_in_an_inelastic_hit() {
        let mut p = Physics { gravity: (0.0, 0.0), ..Physics::default() };
        let mut w = World::new();
        let heavy = boxed(&mut w, square(0.0, 0.0, 20.0), RigidBody::dynamic(10.0), (100.0, 0.0));
        let light = boxed(&mut w, square(30.0, 0.0, 20.0), RigidBody::dynamic(1.0), (0.0, 0.0));
        run(&mut p, &mut w, None, 20);
        let common = 1000.0 / 11.0;
        for e in [heavy, light] {
            let v = state(&w, e).1.0;
            assert!((v - common).abs() < 3.0, "speed {v}, shared speed should be {common}");
        }
    }

    #[test]
    fn a_fixed_box_never_moves_however_hard_it_is_hit() {
        let mut p = Physics { gravity: (0.0, 0.0), ..Physics::default() };
        let mut w = World::new();
        let wall = w.spawn((Body(square(100.0, 0.0, 20.0)), RigidBody::fixed()));
        let ram = boxed(&mut w, square(0.0, 0.0, 20.0), RigidBody::dynamic(1000.0), (500.0, 0.0));
        run(&mut p, &mut w, None, 60);
        assert_eq!(w.get::<&Body>(wall).unwrap().0.x, 100.0);
        let (r, v) = state(&w, ram);
        assert!(r.x + r.w <= 100.2 && v.0.abs() < 1.0, "{r:?} {v:?}");
    }

    #[test]
    fn tiles_act_as_floor_and_walls_for_dynamic_bodies() {
        let sheet = Arc::new(Sprite::load("assets/images/tiles.png"));
        let map = Tilemap::from_ascii(&["....", "....", "####"], 16, sheet);
        let (mut p, mut w) = (Physics::default(), World::new());
        let e = boxed(&mut w, square(20.0, 0.0, 10.0), RigidBody::dynamic(1.0), (0.0, 0.0));
        run(&mut p, &mut w, Some(&map), 120);
        let (r, v) = state(&w, e);
        assert!((r.y + r.h - 32.0).abs() < 0.2 && v.1.abs() < 1.0, "{r:?} {v:?}");

        let mut side = World::new();
        let mut q = Physics { gravity: (0.0, 0.0), ..Physics::default() };
        let b = boxed(&mut side, square(20.0, 5.0, 10.0), RigidBody::dynamic(1.0), (-100.0, 0.0));
        run(&mut q, &mut side, Some(&map), 60);
        assert!(state(&side, b).0.x >= -0.2, "left the map: {}", state(&side, b).0.x);
    }

    #[test]
    fn bodies_without_velocity_stay_where_they_are() {
        let (mut p, mut w) = (Physics::default(), World::new());
        let e = w.spawn((Body(square(0.0, 0.0, 10.0)), RigidBody::dynamic(1.0)));
        run(&mut p, &mut w, None, 30);
        assert_eq!(w.get::<&Body>(e).unwrap().0.y, 0.0);
    }

    fn tile_floor() -> Tilemap {
        let sheet = Arc::new(Sprite::load("assets/images/tiles.png"));
        Tilemap::from_ascii(&["....................", "....................", "####################"], 16, sheet)
    }

    #[test]
    fn a_body_walks_left_along_a_tile_floor_without_catching_on_the_seams() {
        let map = tile_floor();
        let (mut p, mut w) = (Physics::default(), World::new());
        let rb = RigidBody::dynamic(1.0).with_friction(0.0);
        let e = boxed(&mut w, Rect { x: 280.0, y: 0.0, w: 12.0, h: 24.0 }, rb, (0.0, 0.0));
        for _ in 0..120 {
            w.query_one_mut::<&mut Vel>(e).unwrap().x = -100.0;
            p.step(&mut w, Some(&map), DT);
        }
        assert!(state(&w, e).0.x < 100.0, "caught on a seam at {:?}", state(&w, e).0);
    }

    #[test]
    fn a_ball_rolls_over_tile_seams_either_way_without_bumping() {
        let map = tile_floor();
        for (start, speed) in [(20.0, 120.0), (280.0, -120.0)] {
            let (mut p, mut w) = (Physics::default(), World::new());
            let rb = RigidBody::dynamic(1.0).with_friction(0.0).circle();
            let e = boxed(&mut w, square(start, 8.0, 16.0), rb, (0.0, 0.0));
            run(&mut p, &mut w, Some(&map), 30);
            let rest = state(&w, e).0.y;
            let mut worst: f32 = 0.0;
            for _ in 0..120 {
                w.query_one_mut::<&mut Vel>(e).unwrap().x = speed;
                p.step(&mut w, Some(&map), DT);
                worst = worst.max((state(&w, e).0.y - rest).abs());
            }
            assert!((state(&w, e).0.x - start).abs() > 200.0, "stuck at {:?}", state(&w, e).0);
            assert!(worst < 0.3, "bumped {worst} px over a seam going {speed}");
        }
    }

    #[test]
    fn a_ball_dropped_on_the_floor_rests_on_top_and_bounces_by_its_restitution() {
        let (mut p, mut w) = (Physics::default(), World::new());
        floor(&mut w);
        let e = boxed(&mut w, square(50.0, 40.0, 20.0), RigidBody::dynamic(1.0).circle(), (0.0, 0.0));
        run(&mut p, &mut w, None, 120);
        let (r, v) = state(&w, e);
        assert!((r.y + r.h - 100.0).abs() < 0.2 && v.1.abs() < 1.0, "{r:?} {v:?}");

        let mut w = World::new();
        w.spawn((Body(Rect { x: -500.0, y: 500.0, w: 2000.0, h: 50.0 }), RigidBody::fixed().with_bounce(0.5)));
        let e = boxed(&mut w, square(0.0, 0.0, 10.0), RigidBody::dynamic(1.0).with_bounce(0.5).circle(), (0.0, 0.0));
        let (mut incoming, mut outgoing) = (0.0, 0.0);
        for _ in 0..200 {
            let before = state(&w, e).1.1;
            p.step(&mut w, None, DT);
            let after = state(&w, e).1.1;
            if before > 0.0 && after < 0.0 {
                (incoming, outgoing) = (before + 900.0 * DT, after);
                break;
            }
        }
        assert!(incoming > 0.0, "the ball never bounced");
        assert!((outgoing / -incoming - 0.5).abs() < 0.03, "{outgoing} vs {incoming}");
    }

    #[test]
    fn equal_balls_meeting_head_on_swap_velocities_and_off_centre_ones_deflect() {
        let mut p = Physics { gravity: (0.0, 0.0), ..Physics::default() };
        let rb = || RigidBody::dynamic(1.0).with_bounce(1.0).with_friction(0.0).circle();
        let mut w = World::new();
        let a = boxed(&mut w, square(0.0, 0.0, 20.0), rb(), (100.0, 0.0));
        let b = boxed(&mut w, square(80.0, 0.0, 20.0), rb(), (-100.0, 0.0));
        run(&mut p, &mut w, None, 60);
        assert!((state(&w, a).1.0 + 100.0).abs() < 1.0 && (state(&w, b).1.0 - 100.0).abs() < 1.0);

        let mut w = World::new();
        let a = boxed(&mut w, square(0.0, 0.0, 20.0), rb(), (100.0, 0.0));
        let b = boxed(&mut w, square(50.0, 14.0, 20.0), rb(), (0.0, 0.0));
        run(&mut p, &mut w, None, 40);
        let (va, vb) = (state(&w, a).1, state(&w, b).1);
        assert!(va.1 < -5.0 && vb.1 > 5.0, "no sideways deflection: {va:?} {vb:?}");
        assert!((va.0 + vb.0 - 100.0).abs() < 1.0 && (va.1 + vb.1).abs() < 1.0, "momentum: {va:?} {vb:?}");
    }

    #[test]
    fn balls_and_boxes_piled_into_an_arena_stay_inside_it() {
        let (mut p, mut w) = (Physics::default(), World::new());
        let mut rng = crate::Rng::new(11);
        let wall = |x, y, w_, h| (Body(Rect { x, y, w: w_, h }), RigidBody::fixed());
        w.spawn(wall(0.0, 340.0, 640.0, 20.0));
        w.spawn(wall(0.0, 0.0, 10.0, 360.0));
        w.spawn(wall(630.0, 0.0, 10.0, 360.0));
        let bodies: Vec<Entity> = (0..40)
            .map(|i| {
                let size = rng.range(14.0, 30.0);
                let rect = Rect { x: rng.range(20.0, 620.0 - size), y: -20.0 * i as f32, w: size, h: size };
                let rb = RigidBody::dynamic(size * size / 100.0).with_bounce(rng.range(0.0, 0.6)).with_friction(0.4);
                boxed(&mut w, rect, if i % 2 == 0 { rb.circle() } else { rb }, (0.0, 0.0))
            })
            .collect();
        run(&mut p, &mut w, None, 600);
        for e in bodies {
            let (r, _) = state(&w, e);
            assert!(r.x >= 9.0 && r.x + r.w <= 631.0 && r.y + r.h <= 341.0, "escaped or sank: {r:?}");
        }
    }

    #[test]
    fn a_dense_pile_comes_to_rest_instead_of_jittering() {
        let (mut p, mut w) = (Physics::default(), World::new());
        let mut rng = crate::Rng::new(5);
        let wall = |x, y, w_, h| (Body(Rect { x, y, w: w_, h }), RigidBody::fixed());
        w.spawn(wall(0.0, 300.0, 640.0, 60.0));
        w.spawn(wall(0.0, 0.0, 10.0, 360.0));
        w.spawn(wall(630.0, 0.0, 10.0, 360.0));
        let bodies: Vec<Entity> = (0..300)
            .map(|i| {
                let rect = Rect { x: rng.range(20.0, 600.0), y: -(i as f32) * 0.5, w: 8.0, h: 8.0 };
                let rb = RigidBody::dynamic(1.0).with_bounce(0.2);
                let rb = if i % 2 == 0 { rb.circle() } else { rb };
                w.spawn((Body(rect), Vel { x: 0.0, y: 0.0 }, rb, Transform::IDENTITY))
            })
            .collect();
        run(&mut p, &mut w, None, 900);
        let speed: f32 = bodies.iter().map(|&e| state(&w, e).1.0.hypot(state(&w, e).1.1)).sum::<f32>() / 300.0;
        assert!(speed < 2.0, "mean speed after 20 s: {speed}");
    }

    #[test]
    fn fast_bodies_do_not_pass_through_a_thin_floor() {
        for circle in [false, true] {
            for tilt in [0.0, 0.5] {
                let (mut p, mut w) = (Physics::default(), World::new());
                w.spawn((Body(Rect { x: -5000.0, y: 300.0, w: 10000.0, h: 4.0 }), RigidBody::fixed()));
                let rb = if circle { RigidBody::dynamic(1.0).circle() } else { RigidBody::dynamic(1.0) };
                let e = turning(&mut w, Rect { x: 100.0, y: 0.0, w: 8.0, h: 8.0 }, rb, tilt);
                w.get::<&mut Vel>(e).unwrap().y = 2500.0;
                run(&mut p, &mut w, None, 60);
                let (r, _) = state(&w, e);
                assert!(r.y + r.h <= 301.0, "circle {circle} tilt {tilt} went through: {r:?}");
            }
        }
    }

    fn swing(rope: bool, start: (f32, f32)) -> (Physics, World, Entity, Entity) {
        let (mut p, mut w) = (Physics::default(), World::new());
        let anchor = w.spawn((Body(square(300.0, 100.0, 10.0)), RigidBody::fixed()));
        let bob = boxed(&mut w, square(start.0, start.1, 10.0), RigidBody::dynamic(1.0), (0.0, 0.0));
        if rope {
            p.rope(anchor, bob, 100.0);
        } else {
            p.rod(anchor, bob, 100.0);
        }
        (p, w, anchor, bob)
    }

    fn gap(w: &World, a: Entity, b: Entity) -> f32 {
        let rect = |e| w.get::<&Body>(e).unwrap().0;
        let (ra, rb) = (rect(a), rect(b));
        (ra.x - rb.x).hypot(ra.y - rb.y)
    }

    #[test]
    fn a_rod_keeps_its_length_while_the_bob_swings_through_the_bottom() {
        let (mut p, mut w, anchor, bob) = swing(false, (400.0, 100.0));
        let mut lowest = 0.0f32;
        for _ in 0..240 {
            run(&mut p, &mut w, None, 1);
            assert!((gap(&w, anchor, bob) - 100.0).abs() < 1.5, "length drifted to {}", gap(&w, anchor, bob));
            lowest = lowest.max(state(&w, bob).0.y);
        }
        assert!(lowest > 190.0, "the bob never swung down: {lowest}");
    }

    #[test]
    fn a_rope_goes_slack_up_close_and_holds_once_taut() {
        let (mut p, mut w, anchor, bob) = swing(true, (300.0, 130.0));
        run(&mut p, &mut w, None, 6);
        assert!(state(&w, bob).0.y > 135.0, "a slack rope lets the bob fall freely");
        run(&mut p, &mut w, None, 120);
        assert!(gap(&w, anchor, bob) < 101.5, "taut rope holds: {}", gap(&w, anchor, bob));
        assert!(state(&w, bob).0.y > 190.0);
    }

    #[test]
    fn joints_to_despawned_bodies_are_skipped() {
        let (mut p, mut w, anchor, bob) = swing(false, (400.0, 100.0));
        w.despawn(anchor).unwrap();
        run(&mut p, &mut w, None, 30);
        assert!(state(&w, bob).0.y > 200.0, "free fall without its anchor");
        p.clear_joints();
    }

    fn turning(w: &mut World, rect: Rect, rb: RigidBody, angle: f32) -> Entity {
        w.spawn((Body(rect), Vel { x: 0.0, y: 0.0 }, rb, Transform { angle, sx: 1.0, sy: 1.0 }))
    }

    fn angle_of(w: &World, e: Entity) -> f32 {
        w.get::<&Transform>(e).unwrap().angle
    }

    #[test]
    fn a_tilted_box_dropped_on_the_floor_ends_up_flat_and_still() {
        for tilt in [0.3_f32, -0.6, 0.78] {
            let (mut p, mut w) = (Physics::default(), World::new());
            floor(&mut w);
            let e = turning(&mut w, Rect { x: 50.0, y: 20.0, w: 30.0, h: 20.0 }, RigidBody::dynamic(1.0), tilt);
            run(&mut p, &mut w, None, 400);
            let (r, v) = state(&w, e);
            let off = angle_of(&w, e).rem_euclid(std::f32::consts::FRAC_PI_2);
            let off = off.min(std::f32::consts::FRAC_PI_2 - off);
            assert!(off < 0.03, "tilt {tilt}: rests at {} rad", angle_of(&w, e));
            assert!(v.0.abs() < 1.0 && v.1.abs() < 2.0, "tilt {tilt}: still moving {v:?}");
            let centre = r.y + r.h / 2.0;
            assert!((centre - 90.0).abs() < 0.6 || (centre - 85.0).abs() < 0.6, "tilt {tilt}: centre at {centre}");
        }
    }

    #[test]
    fn a_resting_box_that_can_turn_does_not_creep_or_spin_up() {
        let (mut p, mut w) = (Physics::default(), World::new());
        floor(&mut w);
        let e = turning(&mut w, Rect { x: 50.0, y: 79.0, w: 30.0, h: 20.0 }, RigidBody::dynamic(1.0), 0.0);
        run(&mut p, &mut w, None, 600);
        let (r, _) = state(&w, e);
        assert!((r.x - 50.0).abs() < 0.5 && (r.y + r.h - 100.0).abs() < 0.3, "{r:?}");
        assert!(angle_of(&w, e).abs() < 0.01, "turned to {}", angle_of(&w, e));
        assert!(w.get::<&RigidBody>(e).unwrap().spin.abs() < 0.05);
    }

    #[test]
    fn a_ball_rolls_down_a_slope_at_two_thirds_of_the_slide_acceleration() {
        let (mut p, mut w) = (Physics::default(), World::new());
        let angle = 30.0_f32.to_radians();
        let plank = Rect { x: -400.0, y: 100.0, w: 800.0, h: 20.0 };
        w.spawn((Body(plank), RigidBody::fixed().with_friction(1.0), Transform { angle, sx: 1.0, sy: 1.0 }));
        // On the slope's surface: along the plank's normal, clear of it by the radius.
        let (nx, ny) = (angle.sin(), -angle.cos());
        let (cx, cy) = (
            plank.x + 400.0 + 10.0 * nx - 150.0 * angle.cos(),
            plank.y + 10.0 + (10.0 + 10.0) * ny - 150.0 * angle.sin(),
        );
        let rect = Rect { x: cx - 10.0, y: cy - 10.0, w: 20.0, h: 20.0 };
        let ball = turning(&mut w, rect, RigidBody::dynamic(1.0).with_friction(1.0).circle(), 0.0);
        run(&mut p, &mut w, None, 30);
        let (v0, t0) = (state(&w, ball).1, 30.0 * DT);
        run(&mut p, &mut w, None, 30);
        let v1 = state(&w, ball).1;
        let (s0, s1) = ((v0.0.powi(2) + v0.1.powi(2)).sqrt(), (v1.0.powi(2) + v1.1.powi(2)).sqrt());
        let accel = (s1 - s0) / (30.0 * DT);
        let expected = 900.0 * angle.sin() / 1.5;
        assert!(t0 > 0.0 && (accel / expected - 1.0).abs() < 0.15, "accel {accel}, rolling should give {expected}");
        let spin = w.get::<&RigidBody>(ball).unwrap().spin;
        assert!((spin * 10.0 / s1 - 1.0).abs() < 0.15, "spin {spin} does not match rolling at speed {s1}");
    }

    #[test]
    fn an_off_centre_hit_spins_the_box_and_conserves_both_momenta() {
        let mut p = Physics { gravity: (0.0, 0.0), ..Physics::default() };
        let mut w = World::new();
        let rb = |m: f32| RigidBody::dynamic(m).with_friction(0.0).with_bounce(0.5);
        let ball = turning(&mut w, Rect { x: 0.0, y: 0.0, w: 10.0, h: 10.0 }, rb(1.0).circle(), 0.0);
        let boxed_ = turning(&mut w, Rect { x: 40.0, y: 6.0, w: 30.0, h: 30.0 }, rb(4.0), 0.0);
        w.query_one_mut::<&mut Vel>(ball).unwrap().x = 25.0;
        let momentum = |w: &World| {
            let (mut lin, mut ang) = ((0.0, 0.0), 0.0);
            for (e, m, i) in [(ball, 1.0, 0.5 * 25.0), (boxed_, 4.0, 4.0 * 1800.0 / 12.0)] {
                let (r, v) = state(w, e);
                let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
                lin = (lin.0 + m * v.0, lin.1 + m * v.1);
                ang += m * (cx * v.1 - cy * v.0) + i * w.get::<&RigidBody>(e).unwrap().spin;
            }
            (lin, ang)
        };
        let (lin0, ang0) = momentum(&w);
        run(&mut p, &mut w, None, 120);
        let (lin1, ang1) = momentum(&w);
        assert!(w.get::<&RigidBody>(boxed_).unwrap().spin.abs() > 0.1, "the box did not start turning");
        assert!((lin1.0 - lin0.0).abs() < 2.0 && (lin1.1 - lin0.1).abs() < 2.0, "linear {lin0:?} -> {lin1:?}");
        assert!((ang1 - ang0).abs() < ang0.abs() * 0.03, "angular {ang0} -> {ang1}");
    }

    #[test]
    fn tilted_boxes_land_flat_on_a_tile_floor_across_its_seams() {
        let map = tile_floor();
        for x in [16.0_f32, 29.0, 100.0] {
            let (mut p, mut w) = (Physics::default(), World::new());
            let e = turning(&mut w, Rect { x, y: 0.0, w: 24.0, h: 14.0 }, RigidBody::dynamic(1.0), 0.5);
            run(&mut p, &mut w, Some(&map), 300);
            let r = state(&w, e).0;
            let centre = r.y + r.h / 2.0;
            assert!(
                (centre - 25.0).abs() < 0.6 || (centre - 20.0).abs() < 0.6,
                "x {x}: centre at {centre} on a floor at 32"
            );
            assert!(state(&w, e).1.1.abs() < 2.0, "x {x}: still falling or bouncing");
        }
    }

    #[test]
    fn spinning_boxes_and_balls_piled_into_an_arena_stay_inside_it() {
        let (mut p, mut w) = (Physics::default(), World::new());
        let mut rng = crate::Rng::new(5);
        let wall = |x, y, w_, h| (Body(Rect { x, y, w: w_, h }), RigidBody::fixed());
        w.spawn(wall(0.0, 340.0, 640.0, 20.0));
        w.spawn(wall(0.0, 0.0, 10.0, 360.0));
        w.spawn(wall(630.0, 0.0, 10.0, 360.0));
        let bodies: Vec<Entity> = (0..40)
            .map(|i| {
                let (wd, ht) = (rng.range(14.0, 34.0), rng.range(14.0, 34.0));
                let rect = Rect { x: rng.range(20.0, 600.0), y: -20.0 * i as f32, w: wd, h: ht };
                let rb = RigidBody::dynamic(wd * ht / 100.0).with_bounce(rng.range(0.0, 0.5)).with_friction(0.5);
                let rb = rb.with_spin(rng.range(-6.0, 6.0));
                if i % 3 == 0 {
                    turning(&mut w, Rect { h: wd, ..rect }, rb.circle(), 0.0)
                } else {
                    turning(&mut w, rect, rb, rng.range(0.0, 3.0))
                }
            })
            .collect();
        run(&mut p, &mut w, None, 900);
        for e in bodies {
            let (r, v) = state(&w, e);
            // The rect is the unturned box, so judge the centre against the smallest half-size.
            let (cx, cy, near) = (r.x + r.w / 2.0, r.y + r.h / 2.0, r.w.min(r.h) / 2.0);
            assert!(cx >= 9.0 + near && cx <= 631.0 - near && cy <= 341.0 - near, "escaped or sank: {r:?}");
            assert!(v.0.hypot(v.1) < 60.0, "still flying after 15 s: {v:?}");
        }
    }
}
