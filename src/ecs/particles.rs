use crate::{Body, Entity, Layer, Look, Rect, Rng, Timer, Vec2, World};

/// Marks an entity made by `burst`; query for it to find or clear the particles.
///
/// ```
/// use engine::{Burst, Particle, Rng, World, burst};
///
/// let mut world = World::new();
/// let spec = Burst { count: 3, color: 0xffffff, speed: 40.0, life: 1.0, size: 2.0, gravity: 0.0 };
/// burst(&mut world, &mut Rng::new(1), (0.0, 0.0), &spec);
/// assert_eq!(world.query::<&Particle>().iter().count(), 3);
/// ```
pub struct Particle {
    vx: f32,
    vy: f32,
    life: Timer,
    gravity: f32,
}

/// What a [`burst`] throws: how many specks, their colour, top speed, lifetime in seconds, size and gravity.
///
/// ```
/// use engine::{Burst, Particle, Rng, World, burst};
///
/// let mut world = World::new();
/// let spec = Burst { count: 5, color: 0xffd040, speed: 80.0, life: 0.5, size: 2.0, gravity: 100.0 };
/// burst(&mut world, &mut Rng::new(1), (10.0, 10.0), &spec);
/// assert_eq!(world.query::<&Particle>().iter().count(), 5);
/// ```
pub struct Burst {
    /// How many specks.
    pub count: usize,
    /// Their colour, 0xRRGGBB.
    pub color: u32,
    /// Top speed in pixels per second.
    pub speed: f32,
    /// Seconds a speck lives.
    pub life: f32,
    /// Side of a speck in pixels.
    pub size: f32,
    /// Downward pull in pixels per second squared.
    pub gravity: f32,
}

/// ```
/// use engine::{Burst, Particle, Rng, World, burst};
///
/// let mut world = World::new();
/// let spec = Burst { count: 8, color: 0xffd040, speed: 60.0, life: 0.4, size: 2.0, gravity: 0.0 };
/// burst(&mut world, &mut Rng::new(1), (50.0, 50.0), &spec);
/// assert_eq!(world.query::<&Particle>().iter().count(), 8);
/// ```
pub fn burst(world: &mut World, rng: &mut Rng, at: impl Into<Vec2>, b: &Burst) {
    let at = at.into();
    for _ in 0..b.count {
        let angle = rng.range(0.0, std::f32::consts::TAU);
        let speed = rng.range(0.3, 1.0) * b.speed;
        world.spawn((
            Body(Rect::new(at.x - b.size / 2.0, at.y - b.size / 2.0, b.size, b.size)),
            Look::Alpha(b.color, 255),
            Layer(10),
            Particle {
                vx: angle.cos() * speed,
                vy: angle.sin() * speed,
                life: Timer::once(b.life.max(0.0)),
                gravity: b.gravity,
            },
        ));
    }
}

/// ```
/// use engine::{Burst, Particle, Rng, World, burst, particles};
///
/// let mut world = World::new();
/// let spec = Burst { count: 4, color: 0xffffff, speed: 40.0, life: 0.5, size: 2.0, gravity: 0.0 };
/// burst(&mut world, &mut Rng::new(1), (0.0, 0.0), &spec);
/// particles(&mut world, 1.0);
/// assert_eq!(world.query::<&Particle>().iter().count(), 0);
/// ```
pub fn particles(world: &mut World, dt: f32) {
    let mut dead = Vec::new();
    for (e, body, look, p) in world.query_mut::<(Entity, &mut Body, &mut Look, &mut Particle)>() {
        p.life.tick(dt);
        if p.life.done() {
            dead.push(e);
            continue;
        }
        p.vy += p.gravity * dt;
        body.0.x += p.vx * dt;
        body.0.y += p.vy * dt;
        if let Look::Alpha(_, a) = look {
            *a = (255.0 * (1.0 - p.life.fraction())) as u8;
        }
    }
    for e in dead {
        world.despawn(e).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn particles_fade_over_their_life_then_disappear() {
        let mut w = World::new();
        let spec = Burst { count: 5, color: 1, speed: 10.0, life: 1.0, size: 4.0, gravity: 0.0 };
        burst(&mut w, &mut Rng::new(7), (50.0, 50.0), &spec);
        assert_eq!(w.len(), 5);
        particles(&mut w, 0.5);
        assert_eq!(w.len(), 5);
        assert!(w.query::<&Look>().iter().all(|l| matches!(l, Look::Alpha(1, 126..=128))));
        particles(&mut w, 0.6);
        assert_eq!(w.len(), 0);
    }
}
