use super::{Entity, Filter, Physics, Rect, Vec2};

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
    pub(super) key: (u64, u64),
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

// How long a body has been nearly still, and where it lies once it sleeps (a changed rect means the game moved it).
#[derive(Default)]
pub(super) struct Rest {
    pub(super) calm: f32,
    pub(super) asleep: Option<(Rect, f32, Filter, f32)>,
    // What it touched when it fell asleep, which stays true while it sleeps (a sleeper makes no contacts of its own).
    pub(super) contacts: Vec<ContactEvent>,
}

impl Physics {
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
    pub(super) fn collect_events(&mut self) {
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
}
