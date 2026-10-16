/// Makes an entity take part in [`Physics`](crate::Physics): `dynamic(mass)` moves, `fixed()` is a wall, `kinematic()` is a platform
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
    pub(super) inv_mass: f32,
    pub(super) circle: bool,
    pub(super) kinematic: bool,
    pub(super) one_way: bool,
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
        use crate::ecs::parse_numbers;
        // Files saved before `gravity` existed have seven parse_numbers.
        let [inv_mass, circle, kinematic, one_way, restitution, friction, spin, gravity] = parse_numbers(text)
            .or_else(|| parse_numbers::<7>(text).map(|[a, b, c, d, e, f, g]| [a, b, c, d, e, f, g, 1.0]))?;
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
