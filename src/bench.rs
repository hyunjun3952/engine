use crate::{
    Body, Burst, Collider, Gfx, Look, Rect, Rng, Sprite, Tilemap, World, burst, collisions, draw_list, particles,
};
use std::sync::Arc;
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

    let full = Rect { x: 0.0, y: 0.0, w: 640.0, h: 360.0 };
    time("clear 640x360", 200, || g.clear(0x101018));
    time("fill_alpha full screen", 200, || g.fill_alpha(&full, 0, 150));

    let sheet = Arc::new(Sprite::load("assets/tiles.png"));
    let map = Tilemap::load("assets/level1.txt", 16, sheet);
    let view = Rect { x: 0.0, y: 0.0, w: 640.0, h: 360.0 };
    let mut tiles = 0;
    time("tilemap draw_list (visible)", 200, || tiles = map.draw_list(view).len());
    println!("  visible tile cmds: {tiles}");
    let tile_cmds = map.draw_list(view);
    time("Gfx::draw tiles", 200, || g.draw(std::hint::black_box(&tile_cmds)));

    let mut ents = World::new();
    for _ in 0..2000 {
        ents.spawn((
            Body(Rect { x: rng(&mut seed) * 640.0, y: rng(&mut seed) * 360.0, w: 8.0, h: 8.0 }),
            Look::Color(5),
        ));
    }
    time("ecs draw_list 2000 entities", 100, || {
        draw_list(&ents);
    });
    let ent_cmds = draw_list(&ents);
    time("Gfx::draw 2000 fills", 100, || g.draw(&ent_cmds));

    let mut pw = World::new();
    let mut r = Rng::new(3);
    let spec = Burst { count: 500, color: 0xffffff, speed: 100.0, life: 100.0, size: 3.0, gravity: 0.0 };
    burst(&mut pw, &mut r, (320.0, 180.0), &spec);
    time("particles update 500", 200, || particles(std::hint::black_box(&mut pw), 1.0 / 60.0));
}
