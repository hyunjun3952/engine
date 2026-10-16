use super::*;
use crate::{Sprite, Vec2};

mod basics;
mod events;
mod joints;
mod platformer;
mod platforms;
mod sleep;
mod slopes;
mod speed;
mod spin;

const DT: f32 = 1.0 / 60.0;

fn boxed(w: &mut World, rect: Rect, rb: RigidBody, vel: (f32, f32)) -> Entity {
    w.spawn((Body(rect), Vel { x: vel.0, y: vel.1 }, rb))
}

fn square(x: f32, y: f32, size: f32) -> Rect {
    Rect { x, y, w: size, h: size }
}

fn floor(w: &mut World) {
    w.spawn((Body(Rect::new(-500.0, 100.0, 2000.0, 50.0)), RigidBody::fixed()));
}

fn state(w: &World, e: Entity) -> (Rect, (f32, f32)) {
    let body = w.get::<&Body>(e).unwrap().0;
    let v = w.get::<&Vel>(e).unwrap();
    (body, (v.x, v.y))
}

fn run(p: &mut Physics, w: &mut World, tiles: Option<&Tilemap>, steps: usize) {
    for _ in 0..steps {
        p.step(w, tiles, DT);
    }
}

// Every body's position, speed, spin and turn as raw bits, so "equal" means identical and not just close.
fn fingerprint(w: &World) -> Vec<[u32; 6]> {
    let mut out: Vec<_> = w
        .query::<(&Body, &Vel, &RigidBody, &Transform)>()
        .iter()
        .map(|(b, v, rb, t)| [b.0.x, b.0.y, v.x, v.y, rb.spin, t.angle].map(f32::to_bits))
        .collect();
    out.sort();
    out
}

fn pile(seed: u32) -> (Physics, World) {
    let (mut rng, mut w) = (crate::Rng::new(seed), World::new());
    floor(&mut w);
    w.spawn((Body(Rect::new(0.0, -100.0, 10.0, 300.0)), RigidBody::fixed()));
    w.spawn((Body(Rect::new(190.0, -100.0, 10.0, 300.0)), RigidBody::fixed()));
    for i in 0..60 {
        let size = rng.range(6.0, 14.0);
        let rect = Rect { x: rng.range(15.0, 170.0), y: -(i as f32) * 12.0, w: size, h: size };
        let body = RigidBody::dynamic(1.0).bounce(0.2).spin(rng.range(-3.0, 3.0));
        let body = if i % 3 == 0 { body.circle() } else { body };
        w.spawn((Body(rect), Vel::default(), body, Transform::IDENTITY));
    }
    (Physics::default(), w)
}

fn tile_floor() -> Tilemap {
    let sheet = Sprite::test_tiles();
    Tilemap::from_ascii(&["....................", "....................", "####################"], 16, sheet)
}

fn substeps_of(w: &World, p: &mut Physics, tiles: Option<&Tilemap>) -> usize {
    let (_, nodes) = gather(w);
    p.substeps(&nodes, tiles, DT)
}

// The level and legend of examples/game.rs.
fn platformer_level() -> Tilemap {
    let tile = |ch, id, one_way, slope| crate::Tile { ch, id, solid: false, front: false, auto: false, one_way, slope };
    let legend = [
        crate::Tile::DEFAULT[0],
        tile(b'=', 2, true, None),
        tile(b'/', 3, false, Some(crate::Slope::BottomRight)),
        tile(b'\\', 4, false, Some(crate::Slope::BottomLeft)),
        crate::Tile { ch: b'%', id: 5, ..crate::Tile::DEFAULT[0] },
    ];
    let sheet = std::sync::Arc::new(Sprite::load("assets/images/platformer_tiles.png").unwrap());
    Tilemap::load_with("assets/levels/platformer.txt", 16, sheet, &legend).unwrap()
}

fn player(w: &mut World, x: f32, y: f32) -> Entity {
    boxed(w, Rect { x, y, w: 12.0, h: 24.0 }, RigidBody::dynamic(1.0).friction(0.0), (0.0, 0.0))
}

fn turning(w: &mut World, rect: Rect, rb: RigidBody, angle: f32) -> Entity {
    w.spawn((Body(rect), Vel { x: 0.0, y: 0.0 }, rb, Transform { angle, sx: 1.0, sy: 1.0 }))
}
