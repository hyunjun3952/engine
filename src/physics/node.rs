use super::{Body, Deref, DerefMut, Entity, Filter, Rect, RigidBody, Transform, Vel, World, turned_box};

// One pair touching at one substep, before the substeps of a step are merged into `ContactEvent`s.
pub(super) struct Raw {
    pub(super) key: (u64, u64),
    pub(super) a: Entity,
    pub(super) b: Option<Entity>,
    pub(super) normal: V2,
    pub(super) impulse: f32,
}

pub(super) type V2 = (f32, f32);

pub(super) struct Node {
    pub(super) rect: Rect,
    pub(super) before: Rect,
    pub(super) at: V2,
    pub(super) vel: V2,
    pub(super) spin: f32,
    pub(super) angle: f32,
    pub(super) inv_mass: f32,
    pub(super) inv_inertia: f32,
    pub(super) circle: bool,
    pub(super) restitution: f32,
    pub(super) friction: f32,
    pub(super) filter: Filter,
    pub(super) moves: bool,
    pub(super) one_way: bool,
    // A tile cut into a triangle; only tiles have one.
    pub(super) slope: Option<crate::Slope>,
    pub(super) gravity: f32,
}

impl Node {
    pub(super) fn wall(rect: Rect) -> Self {
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
    pub(super) fn mobile(&self) -> bool {
        self.inv_mass > 0.0 || self.moves
    }

    pub(super) fn center(&self) -> V2 {
        self.rect.center().into()
    }

    pub(super) fn before_center(&self) -> V2 {
        self.before.center().into()
    }

    pub(super) fn radius(&self) -> f32 {
        self.rect.w.min(self.rect.h) / 2.0
    }

    pub(super) fn half(&self) -> V2 {
        (self.rect.w / 2.0, self.rect.h / 2.0)
    }

    // Local x and y axes in world space.
    pub(super) fn axes(&self) -> (V2, V2) {
        let (s, c) = self.angle.sin_cos();
        ((c, s), (-s, c))
    }

    pub(super) fn bounds(&self) -> Rect {
        if self.angle == 0.0 || self.circle {
            return self.rect;
        }
        turned_box(self.rect, self.angle.sin_cos(), 0.0)
    }

    pub(super) fn velocity_at(&self, r: V2) -> V2 {
        (self.vel.0 - self.spin * r.1, self.vel.1 + self.spin * r.0)
    }

    pub(super) fn push(&mut self, impulse: V2, r: V2, sign: f32) {
        self.vel = (self.vel.0 + sign * impulse.0 * self.inv_mass, self.vel.1 + sign * impulse.1 * self.inv_mass);
        self.spin += sign * cross(r, impulse) * self.inv_inertia;
    }
}

pub(super) fn speed(n: &Node) -> f32 {
    n.vel.0.hypot(n.vel.1)
}

pub(super) fn grow(r: Rect, by: f32) -> Rect {
    Rect::new(r.x - by, r.y - by, r.w + by * 2.0, r.h + by * 2.0)
}

pub(super) fn cross(a: V2, b: V2) -> f32 {
    a.0 * b.1 - a.1 * b.0
}

pub(super) fn sub(a: V2, b: V2) -> V2 {
    (a.0 - b.0, a.1 - b.1)
}

pub(super) fn len(v: V2) -> f32 {
    dot(v, v).sqrt()
}

pub(super) fn dot(a: V2, b: V2) -> f32 {
    a.0 * b.0 + a.1 * b.1
}

#[derive(Clone, Copy)]
pub(super) struct Pt {
    pub(super) at: V2,
    pub(super) depth: f32,
    pub(super) jn: f32,
    pub(super) jt: f32,
    pub(super) bounce: f32,
    // Fixed for the step, so measured once in `prepare`: the arms and the effective masses along normal and tangent.
    pub(super) ra: V2,
    pub(super) rb: V2,
    pub(super) kn: f32,
    pub(super) kt: f32,
}

impl Pt {
    pub(super) fn new(at: V2, depth: f32) -> Self {
        Pt { at, depth, jn: 0.0, jt: 0.0, bounce: 0.0, ra: (0.0, 0.0), rb: (0.0, 0.0), kn: 0.0, kt: 0.0 }
    }
}

// The contact points of one pair: a box face has two at most, so no heap.
#[derive(Clone, Copy)]
pub(super) struct Pts {
    pub(super) items: [Pt; 2],
    pub(super) len: usize,
}

impl Pts {
    pub(super) fn one(p: Pt) -> Self {
        Pts { items: [p, p], len: 1 }
    }

    pub(super) fn retain(&mut self, mut keep: impl FnMut(&Pt) -> bool) {
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

pub(super) struct Hit {
    pub(super) a: usize,
    pub(super) b: usize,
    pub(super) normal: V2,
    pub(super) pts: Pts,
    pub(super) key: (u64, u64),
}

// Tiles have no entity, so they are named by where they are; the top bit keeps the two kinds apart.
pub(super) fn node_id(ents: &[Entity], nodes: &[Node], i: usize) -> u64 {
    ents.get(i).map_or_else(
        || 1 << 63 | (nodes[i].rect.x as i32 as u32 as u64) << 32 | nodes[i].rect.y as i32 as u32 as u64,
        |e| e.to_bits().get(),
    )
}

pub(super) fn gather(world: &World) -> (Vec<Entity>, Vec<Node>) {
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

// How many times heavier the heavier of two movable bodies is; 1 when either is a wall.
pub(super) fn mass_ratio(a: &Node, b: &Node) -> f32 {
    if a.inv_mass == 0.0 || b.inv_mass == 0.0 {
        return 1.0;
    }
    (a.inv_mass / b.inv_mass).max(b.inv_mass / a.inv_mass)
}

pub(super) fn pair_mut(nodes: &mut [Node], a: usize, b: usize) -> (&mut Node, &mut Node) {
    if a < b {
        let (left, right) = nodes.split_at_mut(b);
        (&mut left[a], &mut right[0])
    } else {
        let (left, right) = nodes.split_at_mut(a);
        (&mut right[0], &mut left[b])
    }
}

pub(super) fn arms(a: &Node, b: &Node, at: V2) -> (V2, V2) {
    let (ca, cb) = (a.center(), b.center());
    (sub(at, ca), sub(at, cb))
}

// Velocity of b's contact point relative to a's.
pub(super) fn rel_vel(a: &Node, b: &Node, ra: V2, rb: V2) -> V2 {
    sub(b.velocity_at(rb), a.velocity_at(ra))
}

// Pushes b along `impulse` and a against it, at their contact arms.
pub(super) fn apply(a: &mut Node, b: &mut Node, ra: V2, rb: V2, impulse: V2) {
    a.push(impulse, ra, -1.0);
    b.push(impulse, rb, 1.0);
}

// Effective mass of a contact along `dir`: both linear masses plus how much each body's spin resists.
pub(super) fn stiffness(a: &Node, b: &Node, ra: V2, rb: V2, dir: V2) -> f32 {
    a.inv_mass + b.inv_mass + cross(ra, dir).powi(2) * a.inv_inertia + cross(rb, dir).powi(2) * b.inv_inertia
}

// Moves b along `dir` and a against it, each by its share of `shift`.
pub(super) fn shove(a: &mut Node, b: &mut Node, dir: V2, shift: f32) {
    a.rect.x -= dir.0 * shift * a.inv_mass;
    a.rect.y -= dir.1 * shift * a.inv_mass;
    b.rect.x += dir.0 * shift * b.inv_mass;
    b.rect.y += dir.1 * shift * b.inv_mass;
}

pub(super) fn write_back(world: &mut World, ents: &[Entity], nodes: &[Node]) {
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
