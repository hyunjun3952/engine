use super::collision::Sweep;
use super::fx::{FxMap, FxSet};
use crate::{Body, Entity, Filter, Rect, Tilemap, Transform, Vel, World};

mod contact;

use contact::touch;
use std::ops::{Deref, DerefMut};

const REST_SPEED: f32 = 30.0;
// How far, as a share of its smaller side, a body may move in one substep. Two bodies closing on each other
// double it, and a hit can speed one up mid-step, so half a side left new contacts 6 px deep in 14 px boxes.
const TRAVEL: f32 = 0.125;
const SLOP: f32 = 0.02;
const CORRECTION: f32 = 0.5;
const CORRECTION_PASSES: usize = 6;
const MAX_CORRECTION_PASSES: usize = 50;
const UNSETTLED: f32 = 0.05;
const HEAVY_RATIO: f32 = 8.0;
const WARM_RANGE: f32 = 4.0;
const SAME_NORMAL: f32 = 0.95;
const MAX_SUBSTEPS: usize = 16;
const SLEEP_SPEED: f32 = 5.0;
// Above SLEEP_SPEED, so a body still creeping at the edge of sleeping does not keep waking the ones it touches.
const WAKE_SPEED: f32 = 20.0;
const SLEEP_SPIN: f32 = 0.1;
const SLEEP_TIME: f32 = 0.5;

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
    pts: Pts,
}

// Keeps the centres of two bodies `length` apart (a rod) or at most that far apart (a rope).
struct Joint {
    a: Entity,
    b: Entity,
    length: f32,
    rope: bool,
}

// How long a body has been nearly still, and where it lies once it sleeps (a changed rect means the game moved it).
#[derive(Default)]
struct Rest {
    calm: f32,
    asleep: Option<(Rect, f32)>,
}

pub struct Physics {
    pub gravity: (f32, f32),
    pub iterations: usize,
    // Last step's contact impulses per body pair, replayed first so resting piles start the solve near its answer.
    cache: FxMap<(u64, u64), Memory>,
    joints: Vec<Joint>,
    rest: FxMap<Entity, Rest>,
    // What the last step saw, to notice a body that vanished or a gravity change: sleepers must then be woken.
    known: Vec<Entity>,
    last_gravity: V2,
    last_tiles: Option<u64>,
    // Where the immovable bodies were last step: one the game moved must wake what sleeps near it.
    walls: FxMap<Entity, Rect>,
    sweep: Sweep,
}

impl Default for Physics {
    fn default() -> Self {
        Physics {
            gravity: (0.0, 900.0),
            iterations: 10,
            cache: FxMap::default(),
            joints: Vec::new(),
            rest: FxMap::default(),
            known: Vec::new(),
            last_gravity: (0.0, 900.0),
            last_tiles: None,
            walls: FxMap::default(),
            sweep: Sweep::default(),
        }
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
    filter: Filter,
}

impl Node {
    fn wall(rect: Rect) -> Self {
        Node {
            rect,
            before: rect,
            at: (0.0, 0.0),
            vel: (0.0, 0.0),
            spin: 0.0,
            angle: 0.0,
            inv_mass: 0.0,
            inv_inertia: 0.0,
            circle: false,
            restitution: 0.0,
            friction: 0.4,
            filter: Filter { group: Filter::TERRAIN, mask: u32::MAX },
        }
    }

    fn center(&self) -> V2 {
        self.rect.center()
    }

    fn before_center(&self) -> V2 {
        self.before.center()
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

fn speed(n: &Node) -> f32 {
    n.vel.0.hypot(n.vel.1)
}

fn grow(r: Rect, by: f32) -> Rect {
    Rect { x: r.x - by, y: r.y - by, w: r.w + by * 2.0, h: r.h + by * 2.0 }
}

fn cross(a: V2, b: V2) -> f32 {
    a.0 * b.1 - a.1 * b.0
}

fn sub(a: V2, b: V2) -> V2 {
    (a.0 - b.0, a.1 - b.1)
}

fn dot(a: V2, b: V2) -> f32 {
    a.0 * b.0 + a.1 * b.1
}

#[derive(Clone, Copy)]
struct Pt {
    at: V2,
    depth: f32,
    jn: f32,
    jt: f32,
    bounce: f32,
    // Fixed for the step, so measured once in `prepare`: the arms and the effective masses along normal and tangent.
    ra: V2,
    rb: V2,
    kn: f32,
    kt: f32,
}

impl Pt {
    fn new(at: V2, depth: f32) -> Self {
        Pt { at, depth, jn: 0.0, jt: 0.0, bounce: 0.0, ra: (0.0, 0.0), rb: (0.0, 0.0), kn: 0.0, kt: 0.0 }
    }
}

// The contact points of one pair: a box face has two at most, so no heap.
#[derive(Clone, Copy)]
struct Pts {
    items: [Pt; 2],
    len: usize,
}

impl Pts {
    fn one(p: Pt) -> Self {
        Pts { items: [p, p], len: 1 }
    }

    fn retain(&mut self, mut keep: impl FnMut(&Pt) -> bool) {
        let mut kept = 0;
        for i in 0..self.len {
            if keep(&self.items[i]) {
                self.items[kept] = self.items[i];
                kept += 1;
            }
        }
        self.len = kept;
    }
}

impl Deref for Pts {
    type Target = [Pt];
    fn deref(&self) -> &[Pt] {
        &self.items[..self.len]
    }
}

impl DerefMut for Pts {
    fn deref_mut(&mut self) -> &mut [Pt] {
        &mut self.items[..self.len]
    }
}

impl FromIterator<Pt> for Pts {
    fn from_iter<I: IntoIterator<Item = Pt>>(iter: I) -> Self {
        let mut it = iter.into_iter();
        let first = it.next().expect("a contact has a point");
        let mut pts = Pts::one(first);
        if let Some(second) = it.next() {
            pts.items[1] = second;
            pts.len = 2;
        }
        pts
    }
}

struct Hit {
    a: usize,
    b: usize,
    normal: V2,
    pts: Pts,
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

    /// A body that stayed nearly still for half a second sleeps: it costs nothing and acts as a wall until something
    /// fast touches it or the game changes its `Body` or `Vel`. Jointed bodies never sleep.
    pub fn asleep(&self, e: Entity) -> bool {
        self.rest.get(&e).is_some_and(|r| r.asleep.is_some())
    }

    /// Wakes a sleeping body, for when the game changes what it rests on (a tile removed, say).
    pub fn wake(&mut self, e: Entity) {
        self.rest.remove(&e);
    }

    pub fn step(&mut self, world: &mut World, tiles: Option<&Tilemap>, dt: f32) {
        let (ents, mut nodes) = gather(world);
        let bodies = nodes.len();
        self.freeze(&ents, &mut nodes, tiles.map(Tilemap::stamp), dt);
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
        self.doze(&ents, &mut nodes, &links, dt);
        write_back(world, &ents, &nodes);
    }

    // Turns sleepers into walls for this step, after waking any the game touched or a fast body is about to hit.
    fn freeze(&mut self, ents: &[Entity], nodes: &mut [Node], tiles: Option<u64>, dt: f32) {
        let index: FxMap<Entity, usize> = ents.iter().enumerate().map(|(i, &e)| (e, i)).collect();
        self.rest.retain(|e, r| {
            let Some(&i) = index.get(e) else { return false };
            let n = &nodes[i];
            r.asleep.is_none_or(|(at, angle)| at == n.rect && angle == n.angle && n.vel == (0.0, 0.0) && n.spin == 0.0)
        });
        // Whatever a sleeper rested on may be gone (a despawned body or wall, a dug tile) or pulling another way.
        let changed = (self.known != ents && self.known.iter().any(|e| !index.contains_key(e)))
            || self.gravity != self.last_gravity
            || tiles != self.last_tiles;
        if self.known != ents {
            self.known = ents.to_vec();
        }
        self.last_gravity = self.gravity;
        self.last_tiles = tiles;
        // Only the sleeping stops: the time a body has been still keeps counting, so one whose support is still there
        // goes back to sleep within the step, and a game that despawns bodies often still gets sleepers.
        if changed {
            self.rest.values_mut().for_each(|r| r.asleep = None);
        }
        let movers: Vec<Rect> = ents
            .iter()
            .zip(nodes.iter())
            .filter(|(e, n)| n.inv_mass > 0.0 && !self.asleep(**e) && speed(n) > WAKE_SPEED)
            .map(|(_, n)| grow(n.bounds(), speed(n) * dt + 1.0))
            .collect();
        let mut movers = movers;
        let mut walls = FxMap::default();
        for (e, n) in ents.iter().zip(nodes.iter()).filter(|(_, n)| n.inv_mass == 0.0) {
            if let Some(old) = self.walls.get(e).filter(|old| **old != n.rect) {
                let (x, y) = (old.x.min(n.rect.x), old.y.min(n.rect.y));
                let (x1, y1) = ((old.x + old.w).max(n.rect.x + n.rect.w), (old.y + old.h).max(n.rect.y + n.rect.h));
                movers.push(grow(Rect { x, y, w: x1 - x, h: y1 - y }, 1.0));
            }
            walls.insert(*e, n.rect);
        }
        self.walls = walls;
        for (e, n) in ents.iter().zip(nodes.iter_mut()) {
            if self.asleep(*e) && movers.iter().any(|m| m.overlaps(&n.bounds())) {
                self.rest.remove(e);
            }
            if self.asleep(*e) {
                (n.inv_mass, n.inv_inertia) = (0.0, 0.0);
            }
        }
    }

    fn doze(&mut self, ents: &[Entity], nodes: &mut [Node], links: &[Link], dt: f32) {
        let joined: FxSet<usize> = links.iter().flat_map(|l| [l.a, l.b]).collect();
        for (i, (e, n)) in ents.iter().zip(nodes.iter_mut()).enumerate() {
            if n.inv_mass == 0.0 {
                continue;
            }
            let r = self.rest.entry(*e).or_default();
            if joined.contains(&i) || speed(n) > SLEEP_SPEED || n.spin.abs() > SLEEP_SPIN {
                r.calm = 0.0;
                continue;
            }
            r.calm += dt;
            if r.calm >= SLEEP_TIME {
                (n.vel, n.spin) = ((0.0, 0.0), 0.0);
                r.asleep = Some((n.rect, n.angle));
            }
        }
    }

    // A body that crosses more than half its own size in one step can pass through a thin wall, so the whole
    // step is cut into shorter ones until none does; the cap keeps a stray huge velocity from freezing the game.
    fn substeps(&self, nodes: &[Node], dt: f32) -> usize {
        let gravity = self.gravity.0.hypot(self.gravity.1) * dt;
        let worst = nodes
            .iter()
            .filter(|n| n.inv_mass > 0.0)
            .map(|n| (speed(n) + gravity) * dt / (n.rect.w.min(n.rect.h) * TRAVEL))
            .fold(0.0, f32::max);
        (worst.ceil() as usize).clamp(1, MAX_SUBSTEPS)
    }

    fn substep(&mut self, ents: &[Entity], nodes: &mut Vec<Node>, links: &[Link], tiles: Option<&Tilemap>, dt: f32) {
        let bodies = ents.len();
        self.integrate(nodes, dt);
        if let Some(map) = tiles {
            add_tiles(nodes, bodies, map);
        }
        let mut hits = find_hits(nodes, bodies, tiles, &mut self.sweep);
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
        // A light body squeezed between a floor and a much heavier one converges slowly, so with such a contact the
        // passes go on while it is still deep, up to a limit. More passes on every pile would shake it apart.
        let lopsided = hits.iter().any(|h| mass_ratio(&nodes[h.a], &nodes[h.b]) > HEAVY_RATIO);
        let passes = if lopsided { MAX_CORRECTION_PASSES } else { CORRECTION_PASSES };
        for pass in 0..passes {
            let mut deepest = 0.0f32;
            for hit in &hits {
                let (a, b) = pair_mut(nodes, hit.a, hit.b);
                deepest = deepest.max(separate(a, b, hit));
            }
            for l in links {
                let (a, b) = pair_mut(nodes, l.a, l.b);
                keep_length(a, b, l);
            }
            if pass + 1 >= CORRECTION_PASSES && deepest < UNSETTLED {
                break;
            }
        }
        self.cache.clear();
        self.cache.extend(hits.iter().map(|h| (h.key, Memory { normal: h.normal, pts: h.pts })));
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
    let mut query = world.query::<(Entity, &Body, Option<&Vel>, &RigidBody, Option<&Transform>, Option<&Filter>)>();
    query
        .iter()
        .map(|(e, body, vel, rb, xform, filter)| {
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
                filter: filter.copied().unwrap_or_default(),
            };
            (e, node)
        })
        .unzip()
}

fn add_tiles(nodes: &mut Vec<Node>, bodies: usize, map: &Tilemap) {
    let mut seen = FxSet::default();
    for i in 0..bodies {
        if nodes[i].inv_mass == 0.0 {
            continue;
        }
        let r = nodes[i].bounds();
        let near = Rect { x: r.x - 1.0, y: r.y - 1.0, w: r.w + 2.0, h: r.h + 2.0 };
        for tile in map.solid_in(near) {
            if seen.insert((tile.x as i32, tile.y as i32)) {
                nodes.push(Node { restitution: map.bounce, ..Node::wall(tile) });
            }
        }
    }
}

fn find_hits(nodes: &[Node], bodies: usize, tiles: Option<&Tilemap>, sweep: &mut Sweep) -> Vec<Hit> {
    let bounds: Vec<Rect> = nodes.iter().map(Node::bounds).collect();
    sweep
        .pairs(&bounds, |i| nodes[i].inv_mass == 0.0)
        .iter()
        .copied()
        .filter(|&(a, b, ..)| nodes[a].inv_mass + nodes[b].inv_mass > 0.0 && nodes[a].filter.meets(&nodes[b].filter))
        // A tile always goes second, which is where the seam filters below expect it.
        .map(|(a, b, ..)| if a >= bodies && b < bodies { (b, a) } else { (a, b) })
        .filter_map(|(a, b)| {
            let mut touch = touch(&nodes[a], &nodes[b], false)?;
            if let Some(map) = tiles.filter(|_| b >= bodies) {
                let tile = &nodes[b].rect;
                if touch.whole_face {
                    let (nx, ny) = touch.normal;
                    let (tx, ty) = tile.center();
                    let behind = Rect { x: tx - nx * tile.w, y: ty - ny * tile.h, ..*tile };
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
    let (mx, my) = tile.center();
    let (dx, dy) = (px - mx, py - my);
    let toward = (-normal.0, -normal.1);
    let blocked = |d: f32, t: f32, half: f32| d.abs() >= half - 0.05 && d.signum() * t > 1e-6;
    (blocked(dx, toward.0, tile.w / 2.0) && is_solid(map, mx + dx.signum() * tile.w, my))
        || (blocked(dy, toward.1, tile.h / 2.0) && is_solid(map, mx, my + dy.signum() * tile.h))
}

// How many times heavier the heavier of two movable bodies is; 1 when either is a wall.
fn mass_ratio(a: &Node, b: &Node) -> f32 {
    if a.inv_mass == 0.0 || b.inv_mass == 0.0 {
        return 1.0;
    }
    (a.inv_mass / b.inv_mass).max(b.inv_mass / a.inv_mass)
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
    (sub(at, ca), sub(at, cb))
}

// Velocity of b's contact point relative to a's.
fn rel_vel(a: &Node, b: &Node, ra: V2, rb: V2) -> V2 {
    sub(b.velocity_at(rb), a.velocity_at(ra))
}

// Pushes b along `impulse` and a against it, at their contact arms.
fn apply(a: &mut Node, b: &mut Node, ra: V2, rb: V2, impulse: V2) {
    a.push(impulse, ra, -1.0);
    b.push(impulse, rb, 1.0);
}

// Effective mass of a contact along `dir`: both linear masses plus how much each body's spin resists.
fn stiffness(a: &Node, b: &Node, ra: V2, rb: V2, dir: V2) -> f32 {
    a.inv_mass + b.inv_mass + cross(ra, dir).powi(2) * a.inv_inertia + cross(rb, dir).powi(2) * b.inv_inertia
}

// How fast the bodies close along the normal when contact starts decides the bounce, once, before any impulse.
fn prepare(a: &Node, b: &Node, hit: &mut Hit) {
    let bounce = a.restitution.min(b.restitution);
    let t = (-hit.normal.1, hit.normal.0);
    for pt in hit.pts.iter_mut() {
        let (ra, rb) = arms(a, b, pt.at);
        (pt.ra, pt.rb) = (ra, rb);
        (pt.kn, pt.kt) = (stiffness(a, b, ra, rb, hit.normal), stiffness(a, b, ra, rb, t));
        let closing = -dot(rel_vel(a, b, ra, rb), hit.normal);
        pt.bounce = if closing < REST_SPEED { 0.0 } else { bounce * closing };
    }
}

// A point inherits the impulses of the nearest point this pair had last step, if one is still close.
fn warm_start(a: &mut Node, b: &mut Node, hit: &mut Hit, old: Option<&Memory>) {
    let n = hit.normal;
    let old = old.filter(|o| dot(o.normal, n) > SAME_NORMAL).map(|o| &o.pts[..]);
    let t = (-n.1, n.0);
    for pt in hit.pts.iter_mut() {
        let dist = |o: &&Pt| (o.at.0 - pt.at.0).hypot(o.at.1 - pt.at.1);
        let nearest = old.into_iter().flatten().min_by(|x, y| dist(x).total_cmp(&dist(y)));
        let Some(&Pt { jn, jt, .. }) = nearest.filter(|o| dist(o) < WARM_RANGE) else {
            continue;
        };
        let (ra, rb) = arms(a, b, pt.at);
        apply(a, b, ra, rb, (jn * n.0 + jt * t.0, jn * n.1 + jt * t.1));
        (pt.jn, pt.jt) = (jn, jt);
    }
}

fn solve(a: &mut Node, b: &mut Node, hit: &mut Hit) {
    let n = hit.normal;
    let t = (-n.1, n.0);
    let grip = (a.friction * b.friction).sqrt();
    for pt in hit.pts.iter_mut() {
        let (ra, rb) = (pt.ra, pt.rb);
        let lambda = (pt.bounce - dot(rel_vel(a, b, ra, rb), n)) / pt.kn;
        let total = (pt.jn + lambda).max(0.0);
        apply(a, b, ra, rb, ((total - pt.jn) * n.0, (total - pt.jn) * n.1));
        pt.jn = total;

        let lambda = -dot(rel_vel(a, b, ra, rb), t) / pt.kt;
        let total = (pt.jt + lambda).clamp(-grip * pt.jn, grip * pt.jn);
        apply(a, b, ra, rb, ((total - pt.jt) * t.0, (total - pt.jt) * t.1));
        pt.jt = total;
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
    let (dx, dy) = sub(cb, ca);
    let dist = dx.hypot(dy);
    (dist > 1e-4).then_some(((dx / dist, dy / dist), dist))
}

// Cancels the speed at which the ends move apart (and, for a rod, together) along the joint.
fn pull(a: &mut Node, b: &mut Node, l: &Link) {
    let inv = a.inv_mass + b.inv_mass;
    let Some((n, dist)) = link_axis(a, b).filter(|_| inv > 0.0) else {
        return;
    };
    let along = dot(sub(b.vel, a.vel), n);
    if l.rope && (dist <= l.length || along <= 0.0) {
        return;
    }
    let j = -along / inv;
    a.vel = (a.vel.0 - j * n.0 * a.inv_mass, a.vel.1 - j * n.1 * a.inv_mass);
    b.vel = (b.vel.0 + j * n.0 * b.inv_mass, b.vel.1 + j * n.1 * b.inv_mass);
}

// Moves b along `dir` and a against it, each by its share of `shift`.
fn shove(a: &mut Node, b: &mut Node, dir: V2, shift: f32) {
    a.rect.x -= dir.0 * shift * a.inv_mass;
    a.rect.y -= dir.1 * shift * a.inv_mass;
    b.rect.x += dir.0 * shift * b.inv_mass;
    b.rect.y += dir.1 * shift * b.inv_mass;
}

fn keep_length(a: &mut Node, b: &mut Node, l: &Link) {
    let inv = a.inv_mass + b.inv_mass;
    let Some((n, dist)) = link_axis(a, b).filter(|_| inv > 0.0) else {
        return;
    };
    let error = if l.rope { (dist - l.length).max(0.0) } else { dist - l.length };
    shove(a, b, (-n.0, -n.1), error * CORRECTION / inv);
}

// Positional correction only moves bodies, not their angle; the contact's depth shrinks by how far they have
// moved together along the normal since it was found.
fn separate(a: &mut Node, b: &mut Node, hit: &Hit) -> f32 {
    let (ca, cb) = (a.center(), b.center());
    let moved = sub(sub(cb, b.at), sub(ca, a.at));
    let closed = dot(moved, hit.normal);
    let depth = hit.pts.iter().map(|p| p.depth).fold(0.0, f32::max) - closed;
    let inv_sum = a.inv_mass + b.inv_mass;
    let excess = (depth - SLOP).max(0.0);
    shove(a, b, hit.normal, excess * CORRECTION / inv_sum);
    excess
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
mod tests;
