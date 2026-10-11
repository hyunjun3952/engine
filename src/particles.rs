use crate::{Body, Entity, Layer, Look, Rect, Rng, World};

pub struct Particle {
    vx: f32,
    vy: f32,
    life: f32,
    total: f32,
    size: f32,
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
            Look::Color(b.color),
            Layer(10),
            Particle {
                vx: angle.cos() * speed,
                vy: angle.sin() * speed,
                life: b.life,
                total: b.life,
                size: b.size,
                gravity: b.gravity,
            },
        ));
    }
}

pub fn particles(world: &mut World, dt: f32) {
    let mut dead = Vec::new();
    for (e, body, p) in world.query_mut::<(Entity, &mut Body, &mut Particle)>() {
        p.life -= dt;
        if p.life <= 0.0 {
            dead.push(e);
            continue;
        }
        p.vy += p.gravity * dt;
        let size = p.size * p.life / p.total;
        let (cx, cy) = (body.0.x + body.0.w / 2.0 + p.vx * dt, body.0.y + body.0.h / 2.0 + p.vy * dt);
        body.0 = Rect { x: cx - size / 2.0, y: cy - size / 2.0, w: size, h: size };
    }
    for e in dead {
        world.despawn(e).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn particles_shrink_over_their_life_then_disappear() {
        let mut w = World::new();
        let spec = Burst { count: 5, color: 1, speed: 10.0, life: 1.0, size: 4.0, gravity: 0.0 };
        burst(&mut w, &mut Rng::new(7), (50.0, 50.0), &spec);
        assert_eq!(w.len(), 5);
        particles(&mut w, 0.5);
        assert_eq!(w.len(), 5);
        assert!(w.query::<&Body>().iter().all(|b| (b.0.w - 2.0).abs() < 0.01));
        particles(&mut w, 0.6);
        assert_eq!(w.len(), 0);
    }
}
