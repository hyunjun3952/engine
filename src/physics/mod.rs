use crate::base::fx::{FxMap, FxSet};
use crate::ecs::{Sweep, turned_box};
use crate::{Body, Entity, Filter, Rect, Tilemap, Transform, Vec2, Vel, World};

mod body;
mod contact;
mod events;
mod joints;
mod node;
mod solver;
mod tiles;

pub use body::RigidBody;
pub use events::ContactEvent;
pub use joints::JointId;

use contact::touch;
use events::*;
use joints::*;
use node::*;
use solver::*;
use std::ops::{Deref, DerefMut};
use tiles::*;

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

const SLEEP_TIME: f32 = 0.5;

/// The rigid-body simulation: boxes and circles with rotation, joints, sleeping, one-way platforms and slopes.
///
/// Call [`Physics::step`] once per fixed update. Only entities with a [`RigidBody`] take part; give them a [`Body`]
/// and a [`Vel`], and a [`Transform`] to let them turn. Set `gravity` and `iterations` after
/// `Physics::default()`. The simulation is bit-for-bit repeatable, which input replay relies on.
///
/// A body slower than 5 px/s for half a second sleeps (acts as a wall) until something fast touches it or the game
/// changes its `Body`, `Vel`, angle or `Filter`. Contacts of the last step are in [`Physics::events`]. Joints
/// ([`rod`](Physics::rod), [`rope`](Physics::rope), [`spring`](Physics::spring), [`hinge`](Physics::hinge)) last until
/// [`remove_joint`](Physics::remove_joint) or [`clear_joints`](Physics::clear_joints), and are dropped when an end is
/// despawned.
///
/// ```
/// use engine::{Body, Physics, Rect, RigidBody, Vel, World};
///
/// let mut world = World::new();
/// let floor = world.spawn((Body(Rect::new(0.0, 100.0, 200.0, 10.0)), Vel::default(), RigidBody::fixed()));
/// let ball = world.spawn((Body(Rect::new(50.0, 0.0, 10.0, 10.0)), Vel::default(), RigidBody::dynamic(1.0)));
/// let mut physics = Physics::default();
/// for _ in 0..120 {
///     physics.step(&mut world, None, 1.0 / 60.0);
/// }
/// assert!((world.get::<&Body>(ball).unwrap().0.y - 90.0).abs() < 1.0);
/// # let _ = floor;
/// ```
pub struct Physics {
    /// Acceleration in pixels per second squared.
    pub gravity: Vec2,
    /// Solver passes per substep.
    pub iterations: usize,
    // Last step's contact impulses per body pair, replayed first so resting piles start the solve near its answer.
    cache: FxMap<(u64, u64), Memory>,
    joints: Vec<Joint>,
    joint_ids: u64,
    rest: FxMap<Entity, Rest>,
    // What the last step saw, to notice a body that vanished or a gravity change: sleepers must then be woken.
    known: Vec<Entity>,
    last_gravity: Vec2,
    last_tiles: Option<u64>,
    // Where the immovable bodies were last step: one the game moved must wake what sleeps near it.
    walls: FxMap<Entity, (Rect, Filter, bool)>,
    sweep: Sweep,
    raw: Vec<Raw>,
    events: Vec<ContactEvent>,
    // Finds which fast bodies have anything in their way, apart from `sweep` because its rects differ.
    scout: Sweep,
    pub(crate) last_substeps: usize,
}

impl Default for Physics {
    fn default() -> Self {
        Physics {
            gravity: Vec2::new(0.0, 900.0),
            iterations: 10,
            cache: FxMap::default(),
            joints: Vec::new(),
            joint_ids: 0,
            rest: FxMap::default(),
            known: Vec::new(),
            last_gravity: Vec2::new(0.0, 900.0),
            last_tiles: None,
            walls: FxMap::default(),
            sweep: Sweep::default(),
            raw: Vec::new(),
            events: Vec::new(),
            scout: Sweep::default(),
            last_substeps: 0,
        }
    }
}

impl Physics {
    /// Moves every body by `dt`; call once per fixed update.
    pub fn step(&mut self, world: &mut World, tiles: Option<&Tilemap>, dt: f32) {
        let (ents, mut nodes) = gather(world);
        let bodies = nodes.len();
        let index: FxMap<Entity, usize> = ents.iter().enumerate().map(|(i, &e)| (e, i)).collect();
        self.freeze(&ents, &index, &mut nodes, tiles.map(Tilemap::revision), dt);
        let substeps = self.substeps(&nodes, tiles, dt);
        self.last_substeps = substeps;
        self.joints.retain(|j| index.contains_key(&j.a) && index.contains_key(&j.b));
        let mut links: Vec<Link> = self
            .joints
            .iter()
            .filter_map(|j| {
                Some(Link { a: *index.get(&j.a)?, b: *index.get(&j.b)?, kind: j.kind, gamma: 0.0, beta: 0.0, acc: 0.0 })
            })
            .collect();
        self.raw.clear();
        for _ in 0..substeps {
            nodes.truncate(bodies);
            self.substep(&ents, &mut nodes, &mut links, tiles, dt / substeps as f32);
        }
        self.collect_events();
        self.doze(&ents, &mut nodes, &links, dt);
        write_back(world, &ents, &nodes);
    }

    // Turns sleepers into walls for this step, after waking any the game touched or a fast body is about to hit.
    fn freeze(
        &mut self,
        ents: &[Entity],
        index: &FxMap<Entity, usize>,
        nodes: &mut [Node],
        tiles: Option<u64>,
        dt: f32,
    ) {
        // A sleeper has no mass, so `doze` would never see a joint added to it later.
        let jointed: FxSet<Entity> = self.joints.iter().flat_map(|j| [j.a, j.b]).collect();
        self.rest.retain(|e, r| {
            let Some(&i) = index.get(e) else { return false };
            if jointed.contains(e) {
                return false;
            }
            let n = &nodes[i];
            r.asleep.is_none_or(|(at, angle, filter, gravity)| {
                let same = at == n.rect && angle == n.angle && filter == n.filter && gravity == n.gravity;
                same && n.vel == (0.0, 0.0) && n.spin == 0.0
            })
        });
        // Whatever a sleeper rested on may be gone (a despawned body or wall, a dug tile) or pulling another way.
        let moved = self.known != ents;
        let changed = (moved && self.known.iter().any(|e| !index.contains_key(e)))
            || self.gravity != self.last_gravity
            || tiles != self.last_tiles;
        if moved {
            self.known = ents.to_vec();
        }
        self.last_gravity = self.gravity;
        self.last_tiles = tiles;
        // Only the sleeping stops: the time a body has been still keeps counting, so one whose support is still there
        // goes back to sleep within the step, and a game that despawns bodies often still gets sleepers.
        if changed {
            self.rest.values_mut().for_each(|r| r.asleep = None);
        }
        let mut movers: Vec<Rect> = ents
            .iter()
            .zip(nodes.iter())
            .filter(|(e, n)| n.inv_mass > 0.0 && !self.asleep(**e) && speed(n) > WAKE_SPEED)
            .map(|(_, n)| grow(n.bounds(), speed(n) * dt + 1.0))
            .collect();
        let mut walls = FxMap::default();
        for (e, n) in ents.iter().zip(nodes.iter()).filter(|(_, n)| n.inv_mass == 0.0) {
            // A wall that moved, or one that stopped holding things (a trapdoor clearing its filter), wakes them.
            if let Some((old, ..)) = self.walls.get(e).filter(|old| **old != (n.rect, n.filter, n.one_way)) {
                let (x, y) = (old.x.min(n.rect.x), old.y.min(n.rect.y));
                let (x1, y1) = ((old.x + old.w).max(n.rect.x + n.rect.w), (old.y + old.h).max(n.rect.y + n.rect.h));
                movers.push(grow(Rect { x, y, w: x1 - x, h: y1 - y }, 1.0));
            }
            walls.insert(*e, (n.rect, n.filter, n.one_way));
        }
        self.walls = walls;
        // Sorted by x, so a sleeper only meets the movers whose left edge falls in its reach.
        movers.sort_by(|a, b| a.x.total_cmp(&b.x));
        let widest = movers.iter().fold(0.0f32, |w, m| w.max(m.w));
        for (e, n) in ents.iter().zip(nodes.iter_mut()) {
            if self.asleep(*e) && {
                let b = n.bounds();
                let from = movers.partition_point(|m| m.x <= b.x - widest);
                movers[from..].iter().take_while(|m| m.x < b.x + b.w).any(|m| m.overlaps(b))
            } {
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
            if joined.contains(&i)
                || speed(n) > SLEEP_SPEED
                || n.spin.abs() * n.rect.w.hypot(n.rect.h) / 2.0 > SLEEP_SPEED
            {
                r.calm = 0.0;
                continue;
            }
            r.calm += dt;
            if r.calm >= SLEEP_TIME {
                (n.vel, n.spin) = ((0.0, 0.0), 0.0);
                r.asleep = Some((n.rect, n.angle, n.filter, n.gravity));
                r.contacts = self
                    .events
                    .iter()
                    .filter(|c| c.involves(*e))
                    .map(|c| ContactEvent { began: false, ..*c })
                    .collect();
            }
        }
    }

    // A body that crosses more than an eighth of its own size in one step can pass through a thin wall, so the whole
    // step is cut into shorter ones until none does. Only a fast body with something in its way counts (one that fell
    // out of the world or flies through empty space would otherwise slow every step for good); the cap keeps a stray
    // huge velocity from freezing the game.
    fn substeps(&mut self, nodes: &[Node], tiles: Option<&Tilemap>, dt: f32) -> usize {
        let gravity = self.gravity.length() * dt;
        let reach = |n: &Node| (speed(n) + gravity * n.gravity.abs()) * dt;
        let need = |n: &Node| if n.mobile() { reach(n) / (n.rect.w.min(n.rect.h) * TRAVEL) } else { 0.0 };
        if nodes.iter().all(|n| need(n) <= 1.0) {
            return 1;
        }
        let need: Vec<f32> = nodes.iter().map(need).collect();
        let swept: Vec<Rect> =
            nodes.iter().map(|n| if n.mobile() { grow(n.bounds(), reach(n)) } else { n.bounds() }).collect();
        let fast = |i: usize| need[i] > 1.0;
        let mut worst = 0.0f32;
        for &(a, b, ..) in self.scout.overlaps(&swept, |i| !fast(i)) {
            if nodes[a].filter.meets(&nodes[b].filter) {
                worst = worst.max(if fast(a) { need[a] } else { 0.0 }).max(if fast(b) { need[b] } else { 0.0 });
            }
        }
        if let Some(map) = tiles {
            let (gx, gy, size) = (self.gravity.x * dt, self.gravity.y * dt, map.size as f32);
            let Vec2 { x: mw, y: mh } = map.pixel_size();
            let meets = Filter::TERRAIN | if map.has_one_way() { Filter::PLATFORM } else { 0 };
            for i in (0..nodes.len()).filter(|&i| fast(i) && nodes[i].filter.mask & meets != 0) {
                if need[i] <= worst {
                    continue;
                }
                // Its path, not a box grown on every side, and kept near the map: the side walls are solid at any
                // height, so a body falling past the map must not reach them or make this query huge.
                let (r, v) = (nodes[i].bounds(), nodes[i].vel);
                let k = nodes[i].gravity;
                let (dx, dy) = ((v.0 + gx * k) * dt, (v.1 + gy * k) * dt);
                let (x0, x1) = (r.x.min(r.x + dx).max(-size), (r.x + r.w).max(r.x + r.w + dx).min(mw + size));
                let (y0, y1) = (r.y.min(r.y + dy).max(-size), (r.y + r.h).max(r.y + r.h + dy).min(mh + size));
                if x1 > x0 && y1 > y0 && !map.solid_in(Rect::new(x0, y0, x1 - x0, y1 - y0)).is_empty() {
                    worst = need[i];
                }
            }
        }
        (worst.ceil() as usize).clamp(1, MAX_SUBSTEPS)
    }

    fn substep(
        &mut self,
        ents: &[Entity],
        nodes: &mut Vec<Node>,
        links: &mut [Link],
        tiles: Option<&Tilemap>,
        dt: f32,
    ) {
        let bodies = ents.len();
        for l in links.iter_mut() {
            soften(&nodes[l.a], &nodes[l.b], l, dt);
        }
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
            for l in links.iter_mut() {
                let (a, b) = pair_mut(nodes, l.a, l.b);
                pull(a, b, l);
            }
        }
        self.raw.extend(hits.iter().map(|h| Raw {
            key: h.key,
            a: ents[h.a],
            b: ents.get(h.b).copied(),
            normal: h.normal,
            impulse: h.pts.iter().map(|p| p.jn).sum(),
        }));
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
            for l in links.iter() {
                let (a, b) = pair_mut(nodes, l.a, l.b);
                correct(a, b, l);
            }
            if pass + 1 >= CORRECTION_PASSES && deepest < UNSETTLED {
                break;
            }
        }
        self.cache.clear();
        self.cache.extend(hits.iter().map(|h| (h.key, Memory { normal: h.normal, pts: h.pts })));
    }

    fn integrate(&self, nodes: &mut [Node], dt: f32) {
        for n in nodes.iter_mut().filter(|n| n.mobile()) {
            n.before = n.rect;
            if n.inv_mass > 0.0 {
                n.vel.0 += self.gravity.x * n.gravity * dt;
                n.vel.1 += self.gravity.y * n.gravity * dt;
            }
            n.rect.x += n.vel.0 * dt;
            n.rect.y += n.vel.1 * dt;
            n.angle += n.spin * dt;
        }
    }
}

#[cfg(test)]
mod tests;
