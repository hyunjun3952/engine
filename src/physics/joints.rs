use super::{Body, CORRECTION, Entity, Node, Physics, Transform, V2, Vec2, World, apply, dot, rel_vel, shove, sub};

#[derive(Clone, Copy)]
pub(super) enum Kind {
    // The centres stay `length` apart.
    Rod(f32),
    // The centres stay at most `length` apart.
    Rope(f32),
    // A soft pull towards `length` apart, as a mass on a spring with this frequency and damping ratio.
    Spring { length: f32, hz: f32, damping: f32 },
    // One point of each body (given in the body's own frame, turned with its angle) is held on the same spot.
    Hinge { la: V2, lb: V2 },
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

pub(super) struct Joint {
    pub(super) id: JointId,
    pub(super) a: Entity,
    pub(super) b: Entity,
    pub(super) kind: Kind,
}

impl Physics {
    /// A rigid rod between the centres of `a` and `b`. Torque is ignored: the ends are the centres.
    pub fn rod(&mut self, a: Entity, b: Entity, length: f32) -> JointId {
        self.join(a, b, Kind::Rod(length))
    }

    // Every call hands out a fresh id, also one that adds nothing (`has_joint` then says so).
    pub(super) fn join(&mut self, a: Entity, b: Entity, kind: Kind) -> JointId {
        self.joint_ids += 1;
        let id = JointId(self.joint_ids);
        self.joints.push(Joint { id, a, b, kind });
        id
    }

    pub(super) fn no_joint(&mut self) -> JointId {
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
}

// A joint resolved to node indices for one step. `gamma`, `beta` and `acc` are the softness of a spring and the
// impulse it has applied so far in this substep.
pub(super) struct Link {
    pub(super) a: usize,
    pub(super) b: usize,
    pub(super) kind: Kind,
    pub(super) gamma: f32,
    pub(super) beta: f32,
    pub(super) acc: f32,
}

pub(super) fn link_axis(a: &Node, b: &Node) -> Option<(V2, f32)> {
    let (ca, cb) = (a.center(), b.center());
    let (dx, dy) = sub(cb, ca);
    let dist = dx.hypot(dy);
    (dist > 1e-4).then_some(((dx / dist, dy / dist), dist))
}

// Gives a spring its softness for this substep (the formulation of a soft constraint: frequency and damping ratio
// of the mass it moves) and clears the impulse it has applied.
pub(super) fn soften(a: &Node, b: &Node, l: &mut Link, dt: f32) {
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

pub(super) fn turn(v: V2, angle: f32) -> V2 {
    let v = Vec2::new(v.0, v.1).rotated(angle);
    (v.x, v.y)
}

// Where the pin of a hinge is on each body (as an arm from its centre) and the gap between the two.
pub(super) fn pins(a: &Node, b: &Node, la: V2, lb: V2) -> (V2, V2, V2) {
    let (ra, rb) = (turn(la, a.angle), turn(lb, b.angle));
    let gap = sub((b.center().0 + rb.0, b.center().1 + rb.1), (a.center().0 + ra.0, a.center().1 + ra.1));
    (ra, rb, gap)
}

// Cancels the speed at which the ends move apart (and, for a rod, together) along the joint.
pub(super) fn pull(a: &mut Node, b: &mut Node, l: &mut Link) {
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

// Pulls the bodies of a joint back to where it wants them; a spring is soft and has no such correction.
pub(super) fn correct(a: &mut Node, b: &mut Node, l: &Link) {
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
