use crate::{Body, Entity, Layer, Look, Rect, Rng, Timer, World};

pub struct Particle {
    vx: f32,
    vy: f32,
    life: Timer,
    gravity: f32,
}

pub struct Burst {
    pub count: usize,
    pub color: u32,
    pub speed: f32,
    pub life: f32,
    pub size: f32,
    pub gravity: f32,
}

pub fn burst(world: &mut World, rng: &mut Rng, at: (f32, f32), b: &Burst) {
    for _ in 0..b.count {
        let angle = rng.range(0.0, std::f32::consts::TAU);
        let speed = rng.range(0.3, 1.0) * b.speed;
        world.spawn((
            Body(Rect { x: at.0 - b.size / 2.0, y: at.1 - b.size / 2.0, w: b.size, h: b.size }),
            Look::Alpha(b.color, 255),
            Layer(10),
            Particle {
                vx: angle.cos() * speed,
                vy: angle.sin() * speed,
                life: Timer::once(b.life),
                gravity: b.gravity,
            },
        ));
    }
}

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
