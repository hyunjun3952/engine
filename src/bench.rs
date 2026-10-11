use crate::{Body, Collider, Gfx, Rect, Sprite, World, collisions};
use std::time::Instant;

fn rng(seed: &mut u64) -> f32 {
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    (*seed >> 40) as f32 / (1u64 << 24) as f32
}

fn time(name: &str, runs: u32, mut f: impl FnMut()) {
    let t = Instant::now();
    for _ in 0..runs {
        f();
    }
    println!("{name}: {:.3} ms/run", t.elapsed().as_secs_f64() * 1000.0 / runs as f64);
}

#[test]
#[ignore]
fn bench() {
    let mut seed = 1;
    let mut world = World::new();
    for _ in 0..3000 {
        world.spawn((Body(Rect { x: rng(&mut seed) * 640.0, y: rng(&mut seed) * 360.0, w: 8.0, h: 8.0 }), Collider));
    }
    let mut n = 0;
    time("collisions 3000", 10, || n = collisions(&world).len());
    println!("  contacts: {n}");

    let mut g = Gfx::new(640, 360);
    let s = Sprite::load("assets/ball.png");
    let pos: Vec<(i32, i32)> =
        (0..5000).map(|_| ((rng(&mut seed) * 660.0) as i32 - 10, (rng(&mut seed) * 380.0) as i32 - 10)).collect();
    time("blit 5000 sprites", 20, || {
        for &(x, y) in &pos {
            g.blit(&s, x, y)
        }
    });
    time("text 100 x 20 chars scale 2", 20, || {
        for i in 0..100 {
            g.text("ABCDEFGHIJKLMNOPQRST", 0, i * 3, 2, 0xffffff)
        }
    });
}
