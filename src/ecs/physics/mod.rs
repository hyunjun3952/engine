use super::collision::{Sweep, turned_box};
use super::fx::{FxMap, FxSet};
use crate::{Body, Entity, Filter, Rect, Tilemap, Transform, Vec2, Vel, World};

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
const SLEEP_TIME: f32 = 0.5;

/// Makes an entity take part in [`Physics`]: `dynamic(mass)` moves, `fixed()` is a wall, `kinematic()` is a platform
/// moved only by its `Vel`.
///
/// Chain `.circle()`, `.bounce(e)`, `.friction(f)`, `.spin(rad_per_sec)`, `.gravity(scale)` and, for fixed or kinematic ones,
/// `.one_way()` (holds a body only from above). Two bodies bounce by the smaller restitution, so walls need one too.
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
/// assert!(physics.grounded(ball));
/// # let _ = floor;
/// ```
#[derive(Debug)]
pub struct RigidBody {
    inv_mass: f32,
    circle: bool,
    kinematic: bool,
    one_way: bool,
    /// Bounciness, 0 to 1; two bodies use the smaller one.
    pub restitution: f32,
    /// Friction coefficient; a pair uses the geometric mean.
    pub friction: f32,
    /// Turning speed in radians per second.
    pub spin: f32,
    /// How much of `Physics::gravity` pulls it: 1 by default, 0 floats, negative falls up.
    pub gravity: f32,
}

impl crate::Persist for RigidBody {
    fn write(&self) -> String {
        let flag = |b: bool| b as u8;
        format!(
            "{} {} {} {} {} {} {} {}",
            self.inv_mass,
            flag(self.circle),
            flag(self.kinematic),
            flag(self.one_way),
            self.restitution,
            self.friction,
            self.spin,
            self.gravity
        )
    }

    fn read(text: &str) -> Option<Self> {
        use crate::ecs::persist::numbers;
        // Files saved before `gravity` existed have seven numbers.
        let [inv_mass, circle, kinematic, one_way, restitution, friction, spin, gravity] =
            numbers(text).or_else(|| numbers::<7>(text).map(|[a, b, c, d, e, f, g]| [a, b, c, d, e, f, g, 1.0]))?;
        let valid = inv_mass >= 0.0 && inv_mass.is_finite();
        valid.then_some(RigidBody {
            inv_mass,
            circle: circle != 0.0,
            kinematic: kinematic != 0.0,
            one_way: one_way != 0.0,
            restitution,
            friction,
            spin,
            gravity,
        })
    }
}

impl RigidBody {
    /// A body that moves, with this mass.
    pub fn dynamic(mass: f32) -> Self {
        assert!(mass > 0.0, "a dynamic body needs a positive mass");
        RigidBody {
            inv_mass: 1.0 / mass,
            circle: false,
            kinematic: false,
            one_way: false,
            restitution: 0.0,
            friction: 0.4,
            spin: 0.0,
            gravity: 1.0,
        }
    }

    /// A wall: it never moves.
    pub fn fixed() -> Self {
        RigidBody { inv_mass: 0.0, ..RigidBody::dynamic(1.0) }
    }

    /// A platform that physics moves by its `Vel` (no gravity, never pushed): bodies resting on it are carried and
    /// anything in its way is shoved. Without a `Vel` it is a wall.
    pub fn kinematic() -> Self {
        RigidBody { kinematic: true, ..RigidBody::fixed() }
    }

    /// Holds a body only when it lands on top of it (it must have been above the top before the step); a body
    /// coming from below or the side passes through. Clear a `Filter` bit to drop through on purpose.
    pub fn one_way(mut self) -> Self {
        self.one_way = true;
        self
    }

    /// Makes the body a circle inside its `Body` box.
    pub fn circle(mut self) -> Self {
        self.circle = true;
        self
    }

    /// Scales `Physics::gravity` for this body alone (0 for a balloon or a bullet that flies straight).
    pub fn gravity(mut self, scale: f32) -> Self {
        self.gravity = scale;
        self
    }

    /// Sets the bounciness.
    pub fn bounce(mut self, restitution: f32) -> Self {
        self.restitution = restitution;
        self
    }

    /// Sets the friction.
    pub fn friction(mut self, friction: f32) -> Self {
        self.friction = friction;
        self
    }

    /// Sets the turning speed in radians per second.
    pub fn spin(mut self, radians_per_sec: f32) -> Self {
        self.spin = radians_per_sec;
        self
    }
}

// A contact's normal and, per point, where it was and the impulses it ended with.
struct Memory {
    normal: V2,
    pts: Pts,
}

#[derive(Clone, Copy)]
enum Kind {
    // The centres stay `length` apart.
    Rod(f32),
    // The centres stay at most `length` apart.
    Rope(f32),
    // A soft pull towards `length` apart, as a mass on a spring with this frequency and damping ratio.
    Spring { length: f32, hz: f32, damping: f32 },
    // One point of each body (given in the body's own frame, turned with its angle) is held on the same spot.
    Hinge { la: V2, lb: V2 },
}

// One pair touching at one substep, before the substeps of a step are merged into `ContactEvent`s.
struct Raw {
    key: (u64, u64),
    a: Entity,
    b: Option<Entity>,
    normal: V2,
    impulse: f32,
}

/// Two things touching during the last `step`: `a` is a body and `b` another body, or `None` for a tile.
/// `normal` points from `a` to `b` and `impulse` is the hardest push the pair gave (mass times speed) in any
/// substep, so a hit that bounced away within the step still shows.
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
/// let ground = physics.ground(ball).unwrap();
/// assert_eq!(ground.other(ball), Some(floor));
/// ```
#[derive(Clone, Copy, Debug)]
pub struct ContactEvent {
    /// The entity that was touched.
    pub a: Entity,
    /// The other entity, or `None` for a tile.
    pub b: Option<Entity>,
    /// The contact direction, from `a` to `b`.
    pub normal: Vec2,
    /// The hardest push of any substep.
    pub impulse: f32,
    /// The pair was not touching at the previous step.
    pub began: bool,
    key: (u64, u64),
}

impl ContactEvent {
    /// True if `e` is `a` or `b`.
    pub fn involves(&self, e: Entity) -> bool {
        self.a == e || self.b == Some(e)
    }

    /// The other body of the pair seen from `e`: `None` when it is a tile (or `e` is not in the pair).
    pub fn other(&self, e: Entity) -> Option<Entity> {
        if self.a == e {
            self.b
        } else if self.b == Some(e) {
            Some(self.a)
        } else {
            None
        }
    }

    /// The normal pointing away from `e`, towards what it touches.
    pub fn normal_for(&self, e: Entity) -> Vec2 {
        if self.b == Some(e) { -self.normal } else { self.normal }
    }
}

/// One joint, as `rod`, `rope`, `spring` and `hinge` return it, to `remove_joint` it later.
///
/// ```
/// use engine::{Body, Physics, Rect, RigidBody, Vel, World};
///
/// let mut world = World::new();
/// let mut ball = |x| world.spawn((Body(Rect::new(x, 0.0, 10.0, 10.0)), Vel::default(), RigidBody::dynamic(1.0)));
/// let (a, b) = (ball(0.0), ball(40.0));
/// let mut physics = Physics::default();
/// let rod = physics.rod(a, b, 40.0);
/// assert!(physics.has_joint(rod));
/// physics.remove_joint(rod);
/// assert!(!physics.has_joint(rod));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct JointId(u64);

struct Joint {
    id: JointId,
    a: Entity,
    b: Entity,
    kind: Kind,
}

// How long a body has been nearly still, and where it lies once it sleeps (a changed rect means the game moved it).
#[derive(Default)]
struct Rest {
    calm: f32,
    asleep: Option<(Rect, f32, Filter, f32)>,
    // What it touched when it fell asleep, which stays true while it sleeps (a sleeper makes no contacts of its own).
    contacts: Vec<ContactEvent>,
}

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
    moves: bool,
    one_way: bool,
    // A tile cut into a triangle; only tiles have one.
    slope: Option<crate::Slope>,
    gravity: f32,
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
            moves: false,
            one_way: false,
            slope: None,
            gravity: 0.0,
        }
    }

    // Whether physics moves it: a dynamic body, or a kinematic one with a velocity.
    fn mobile(&self) -> bool {
        self.inv_mass > 0.0 || self.moves
    }

    fn center(&self) -> V2 {
        self.rect.center().into()
    }

    fn before_center(&self) -> V2 {
        self.before.center().into()
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
        turned_box(self.rect, self.angle.sin_cos(), 0.0)
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
    Rect::new(r.x - by, r.y - by, r.w + by * 2.0, r.h + by * 2.0)
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
    pub fn rod(&mut self, a: Entity, b: Entity, length: f32) -> JointId {
        self.join(a, b, Kind::Rod(length))
    }

    // Every call hands out a fresh id, also one that adds nothing (`has_joint` then says so).
    fn join(&mut self, a: Entity, b: Entity, kind: Kind) -> JointId {
        self.joint_ids += 1;
        let id = JointId(self.joint_ids);
        self.joints.push(Joint { id, a, b, kind });
        id
    }

    fn no_joint(&mut self) -> JointId {
        self.joint_ids += 1;
        JointId(self.joint_ids)
    }

    /// Like `rod`, but the bodies may come closer than `length`.
    pub fn rope(&mut self, a: Entity, b: Entity, length: f32) -> JointId {
        self.join(a, b, Kind::Rope(length))
    }

    /// A spring pulling the centres of `a` and `b` towards `length` apart, softly: `hz` is how often a mass on it
    /// would swing per second and `damping` how fast the swing dies (0 never, 1 not at all). Torque is ignored.
    /// A `hz` of 0 is no spring at all.
    pub fn spring(&mut self, a: Entity, b: Entity, length: f32, hz: f32, damping: f32) -> JointId {
        let (hz, damping) = (hz.max(0.0), damping.max(0.0));
        // The soft constraint with no stiffness works out to a rigid rod.
        if hz == 0.0 {
            return self.no_joint();
        }
        self.join(a, b, Kind::Spring { length, hz, damping })
    }

    /// A pin through `anchor` (a world position) holding `a` and `b` together there while they turn about it, as
    /// they are placed now. Bodies turn only when they have a `Transform`; a fixed body makes a pin in the world.
    /// Does nothing if either entity has no `Body`.
    pub fn hinge(&mut self, world: &World, a: Entity, b: Entity, anchor: impl Into<Vec2>) -> JointId {
        let anchor: Vec2 = anchor.into();
        let local = |e: Entity| {
            let body = world.get::<&Body>(e).ok()?.0;
            let angle = world.get::<&Transform>(e).map_or(0.0, |t| t.angle);
            let v = (anchor - body.center()).rotated(-angle);
            Some((v.x, v.y))
        };
        match (local(a), local(b)) {
            (Some(la), Some(lb)) => self.join(a, b, Kind::Hinge { la, lb }),
            _ => self.no_joint(),
        }
    }

    /// Drops the joint; an id that is gone is fine.
    pub fn remove_joint(&mut self, id: JointId) {
        self.joints.retain(|j| j.id != id);
    }

    /// Whether a joint is still there: not removed, not dropped with a despawned end, and not one that added nothing.
    pub fn has_joint(&self, id: JointId) -> bool {
        self.joints.iter().any(|j| j.id == id)
    }

    /// Drops every joint.
    pub fn clear_joints(&mut self) {
        self.joints.clear();
    }

    /// A body that stayed nearly still for half a second sleeps: it costs nothing and acts as a wall until something
    /// fast touches it or the game changes its `Body` or `Vel`. Jointed bodies never sleep.
    pub fn asleep(&self, e: Entity) -> bool {
        self.rest.get(&e).is_some_and(|r| r.asleep.is_some())
    }

    /// Everything that touched during the last `step`, ordered by pair. A pair of which every body is asleep or
    /// immovable makes no contact to find, so it is not here: `events_of` and `ground` still know what a sleeper touches.
    pub fn events(&self) -> &[ContactEvent] {
        &self.events
    }

    /// What `e` touched during the last `step`, and what a sleeping `e` was touching when it fell asleep.
    pub fn events_of(&self, e: Entity) -> impl Iterator<Item = &ContactEvent> + '_ {
        let asleep: &[ContactEvent] = self.rest.get(&e).filter(|r| r.asleep.is_some()).map_or(&[], |r| &r.contacts);
        let fresh = |c: &&ContactEvent| self.events.binary_search_by_key(&c.key, |f| f.key).is_err();
        self.events.iter().filter(move |c| c.involves(e)).chain(asleep.iter().filter(fresh))
    }

    /// What `e` touched on the side gravity pulls it towards during the last `step` (what it stands on), if anything;
    /// `other(e)` of it is the body underneath, or `None` for a tile.
    pub fn ground(&self, e: Entity) -> Option<&ContactEvent> {
        let down = self.gravity.normalized();
        (down != Vec2::ZERO).then(|| self.events_of(e).find(|c| c.normal_for(e).dot(down) > 0.7)).flatten()
    }

    /// Whether `e` stands on something.
    pub fn grounded(&self, e: Entity) -> bool {
        self.ground(e).is_some()
    }

    // Merges the substeps' touching pairs into this step's events.
    fn collect_events(&mut self) {
        self.raw.sort_by_key(|r| r.key);
        let old = std::mem::take(&mut self.events);
        let mut out: Vec<ContactEvent> = Vec::with_capacity(self.raw.len());
        for r in &self.raw {
            match out.last_mut() {
                Some(e) if e.key == r.key => {
                    if r.impulse > e.impulse {
                        (e.normal, e.impulse) = (Vec2::new(r.normal.0, r.normal.1), r.impulse);
                    }
                }
                _ => out.push(ContactEvent {
                    a: r.a,
                    b: r.b,
                    normal: Vec2::new(r.normal.0, r.normal.1),
                    impulse: r.impulse,
                    began: old.binary_search_by_key(&r.key, |o| o.key).is_err(),
                    key: r.key,
                }),
            }
        }
        self.events = out;
    }

    /// Wakes a sleeping body, for when the game changes what it rests on (a tile removed, say).
    pub fn wake(&mut self, e: Entity) {
        self.rest.remove(&e);
    }

    /// Moves every body by `dt`; call once per fixed update.
    pub fn step(&mut self, world: &mut World, tiles: Option<&Tilemap>, dt: f32) {
        let (ents, mut nodes) = gather(world);
        let bodies = nodes.len();
        let index: FxMap<Entity, usize> = ents.iter().enumerate().map(|(i, &e)| (e, i)).collect();
        self.freeze(&ents, &index, &mut nodes, tiles.map(Tilemap::stamp), dt);
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
            // A wall that moved, or one that stopped holding things (a trapdoor clearing its filter), wakes them.
            if let Some((old, ..)) = self.walls.get(e).filter(|old| **old != (n.rect, n.filter, n.one_way)) {
                let (x, y) = (old.x.min(n.rect.x), old.y.min(n.rect.y));
                let (x1, y1) = ((old.x + old.w).max(n.rect.x + n.rect.w), (old.y + old.h).max(n.rect.y + n.rect.h));
                movers.push(grow(Rect { x, y, w: x1 - x, h: y1 - y }, 1.0));
            }
            walls.insert(*e, (n.rect, n.filter, n.one_way));
        }
        self.walls = walls;
        for (e, n) in ents.iter().zip(nodes.iter_mut()) {
            if self.asleep(*e) && movers.iter().any(|m| m.overlaps(n.bounds())) {
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
        for &(a, b, ..) in self.scout.pairs(&swept, |i| !fast(i)) {
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
                friction: if rb.friction.is_finite() { rb.friction.max(0.0) } else { 0.4 },
                filter: filter.copied().unwrap_or_default(),
                moves: vel.is_some() && rb.kinematic,
                one_way: rb.one_way,
                slope: None,
                gravity: if rb.gravity.is_finite() { rb.gravity } else { 1.0 },
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
        let near = Rect::new(r.x - 1.0, r.y - 1.0, r.w + 2.0, r.h + 2.0);
        for tile in map.solid_in(near) {
            if seen.insert((tile.x as i32, tile.y as i32)) {
                let s = map.size as f32;
                let one_way = map.one_way_at((tile.x / s).floor() as i32, (tile.y / s).floor() as i32);
                let group = if one_way { Filter::PLATFORM } else { Filter::TERRAIN };
                let filter = Filter { group, mask: u32::MAX };
                let slope = map.slope_at((tile.x / s).floor() as i32, (tile.y / s).floor() as i32);
                nodes.push(Node { restitution: map.bounce, one_way, slope, filter, ..Node::wall(tile) });
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
            if !passes_one_way(&nodes[a], &nodes[b], touch.normal) {
                return None;
            }
            if let Some(map) = tiles.filter(|_| b >= bodies && nodes[b].slope.is_none()) {
                let tile = &nodes[b].rect;
                if touch.whole_face {
                    let (nx, ny) = touch.normal;
                    let Vec2 { x: tx, y: ty } = tile.center();
                    let behind = Rect { x: tx - nx * tile.w, y: ty - ny * tile.h, ..*tile };
                    if hides(map, behind.x, behind.y, ny > 0.5) {
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

// How far into a platform's top a body that was above it may already have sunk (positional correction leaves some).
const ONE_WAY_GRACE: f32 = 2.0;

// A one-way platform only holds what is pressed onto its top, and only what was above it before the step.
fn passes_one_way(a: &Node, b: &Node, normal: V2) -> bool {
    let (platform, other, down) = match (a.one_way, b.one_way) {
        (false, false) => return true,
        (_, true) => (b, a, normal.1),
        (true, false) => (a, b, -normal.1),
    };
    down > 0.5 && other.before.y + other.before.h <= platform.before.y + ONE_WAY_GRACE
}

// Whether the tile at a point covers a neighbour's face; a one-way tile above covers nothing, or a body dropping
// through it would also pass the ground it sits on.
fn hides(map: &Tilemap, x: f32, y: f32, above: bool) -> bool {
    let s = map.size as f32;
    let (tx, ty) = ((x / s).floor() as i32, (y / s).floor() as i32);
    map.solid(tx, ty) && !(above && map.one_way_at(tx, ty))
}

// A tile face or corner hidden by a solid neighbour is dropped, or a body walking the floor catches on seams.
fn is_hidden(map: &Tilemap, tile: &Rect, (px, py): V2, normal: V2) -> bool {
    let Vec2 { x: mx, y: my } = tile.center();
    let (dx, dy) = (px - mx, py - my);
    let toward = (-normal.0, -normal.1);
    let blocked = |d: f32, t: f32, half: f32| d.abs() >= half - 0.05 && d.signum() * t > 1e-6;
    (blocked(dx, toward.0, tile.w / 2.0) && hides(map, mx + dx.signum() * tile.w, my, false))
        || (blocked(dy, toward.1, tile.h / 2.0) && hides(map, mx, my + dy.signum() * tile.h, dy < 0.0))
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

// A joint resolved to node indices for one step. `gamma`, `beta` and `acc` are the softness of a spring and the
// impulse it has applied so far in this substep.
struct Link {
    a: usize,
    b: usize,
    kind: Kind,
    gamma: f32,
    beta: f32,
    acc: f32,
}

fn link_axis(a: &Node, b: &Node) -> Option<(V2, f32)> {
    let (ca, cb) = (a.center(), b.center());
    let (dx, dy) = sub(cb, ca);
    let dist = dx.hypot(dy);
    (dist > 1e-4).then_some(((dx / dist, dy / dist), dist))
}

// Gives a spring its softness for this substep (the formulation of a soft constraint: frequency and damping ratio
// of the mass it moves) and clears the impulse it has applied.
fn soften(a: &Node, b: &Node, l: &mut Link, dt: f32) {
    l.acc = 0.0;
    let Kind::Spring { hz, damping, .. } = l.kind else { return };
    let inv = a.inv_mass + b.inv_mass;
    if inv <= 0.0 {
        return;
    }
    let (mass, omega) = (1.0 / inv, std::f32::consts::TAU * hz);
    let (d, k) = (2.0 * mass * damping * omega, mass * omega * omega);
    let denom = dt * (d + dt * k);
    l.gamma = if denom > 0.0 { 1.0 / denom } else { 0.0 };
    l.beta = dt * k * l.gamma;
}

fn turn(v: V2, angle: f32) -> V2 {
    let v = Vec2::new(v.0, v.1).rotated(angle);
    (v.x, v.y)
}

// Where the pin of a hinge is on each body (as an arm from its centre) and the gap between the two.
fn pins(a: &Node, b: &Node, la: V2, lb: V2) -> (V2, V2, V2) {
    let (ra, rb) = (turn(la, a.angle), turn(lb, b.angle));
    let gap = sub((b.center().0 + rb.0, b.center().1 + rb.1), (a.center().0 + ra.0, a.center().1 + ra.1));
    (ra, rb, gap)
}

// Cancels the speed at which the ends move apart (and, for a rod, together) along the joint.
fn pull(a: &mut Node, b: &mut Node, l: &mut Link) {
    let inv = a.inv_mass + b.inv_mass;
    if inv <= 0.0 {
        return;
    }
    if let Kind::Hinge { la, lb } = l.kind {
        let (ra, rb, _) = pins(a, b, la, lb);
        let v = rel_vel(a, b, ra, rb);
        let (k11, k22) = (
            inv + ra.1 * ra.1 * a.inv_inertia + rb.1 * rb.1 * b.inv_inertia,
            inv + ra.0 * ra.0 * a.inv_inertia + rb.0 * rb.0 * b.inv_inertia,
        );
        let k12 = -ra.1 * ra.0 * a.inv_inertia - rb.1 * rb.0 * b.inv_inertia;
        let det = k11 * k22 - k12 * k12;
        if det > 1e-12 {
            apply(a, b, ra, rb, (-(k22 * v.0 - k12 * v.1) / det, -(k11 * v.1 - k12 * v.0) / det));
        }
        return;
    }
    let Some((n, dist)) = link_axis(a, b) else { return };
    let along = dot(sub(b.vel, a.vel), n);
    let j = match l.kind {
        Kind::Rod(_) => -along / inv,
        Kind::Rope(length) if dist > length && along > 0.0 => -along / inv,
        Kind::Spring { length, .. } => {
            let lambda = -(along + l.beta * (dist - length) + l.gamma * l.acc) / (inv + l.gamma);
            l.acc += lambda;
            lambda
        }
        _ => return,
    };
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

// Pulls the bodies of a joint back to where it wants them; a spring is soft and has no such correction.
fn correct(a: &mut Node, b: &mut Node, l: &Link) {
    let inv = a.inv_mass + b.inv_mass;
    if inv <= 0.0 {
        return;
    }
    if let Kind::Hinge { la, lb } = l.kind {
        let gap = pins(a, b, la, lb).2;
        let dist = gap.0.hypot(gap.1);
        if dist > 1e-4 {
            shove(a, b, (-gap.0 / dist, -gap.1 / dist), dist * CORRECTION / inv);
        }
        return;
    }
    let Some((n, dist)) = link_axis(a, b) else { return };
    let error = match l.kind {
        Kind::Rod(length) => dist - length,
        Kind::Rope(length) => (dist - length).max(0.0),
        _ => return,
    };
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
    for (e, node) in ents.iter().zip(nodes).filter(|(_, n)| n.mobile()) {
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
