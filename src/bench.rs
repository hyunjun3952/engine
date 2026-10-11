use crate::render::Soft;
use crate::{
    Body, Burst, Collider, DrawCmd, Gfx, Look, Paint, Rect, Rng, Sprite, Tilemap, World, burst, collisions, draw_list,
    particles,
};
use std::hint::black_box;
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

fn cmd(rect: Rect, paint: Paint) -> DrawCmd {
    DrawCmd { layer: 0, rect, paint, debug_only: false }
}

#[test]
#[ignore]
fn bench() {
    let mut seed = 1;
    let mut world = World::new();
    for _ in 0..3000 {
        let rect = Rect { x: rng(&mut seed) * 640.0, y: rng(&mut seed) * 360.0, w: 8.0, h: 8.0 };
        world.spawn((Body(rect), Collider));
    }
    let mut n = 0;
    time("collisions 3000", 10, || n = collisions(&world).len());
    println!("  contacts: {n}");

    let mut soft = Soft::new(640, 360);
    let full = Rect { x: 0.0, y: 0.0, w: 640.0, h: 360.0 };
    let sprite = Arc::new(Sprite::load("assets/ball.png"));
    let sprites: Vec<DrawCmd> = (0..5000)
        .map(|_| {
            let at = Rect {
                x: (rng(&mut seed) * 660.0).floor() - 10.0,
                y: (rng(&mut seed) * 380.0).floor() - 10.0,
                w: 16.0,
                h: 16.0,
            };
            cmd(at, Paint::Sprite(sprite.clone()))
        })
        .collect();
    time("soft: 5000 sprites", 20, || soft.render(black_box(&sprites)));

    let mut g = Gfx::new(640, 360);
    for i in 0..100 {
        g.text("ABCDEFGHIJKLMNOPQRST", 0, i * 3, 2, 0xffffff);
    }
    println!("  text cmds: {}", g.cmds().len());
    time("soft: text 100 x 20 chars", 20, || soft.render(black_box(g.cmds())));
    time("gfx: record text 100 x 20 chars", 20, || {
        g.reset();
        for i in 0..100 {
            g.text("ABCDEFGHIJKLMNOPQRST", 0, i * 3, 2, 0xffffff);
        }
    });

    time("soft: clear", 200, || soft.render(black_box(&[cmd(full, Paint::Clear(0x101018))])));
    time("soft: full-screen alpha", 200, || soft.render(black_box(&[cmd(full, Paint::Alpha(0, 150))])));

    let map = Tilemap::load("assets/level1.txt", 16, Arc::new(Sprite::load("assets/tiles.png")));
    let mut tiles = 0;
    time("tilemap draw_list (visible)", 200, || tiles = map.draw_list(full).len());
    println!("  tile cmds: {tiles}");
    let tile_cmds = map.draw_list(full);
    time("soft: tiles", 200, || soft.render(black_box(&tile_cmds)));

    let mut ents = World::new();
    for _ in 0..2000 {
        let rect = Rect { x: rng(&mut seed) * 640.0, y: rng(&mut seed) * 360.0, w: 8.0, h: 8.0 };
        ents.spawn((Body(rect), Look::Color(5)));
    }
    time("ecs draw_list 2000 entities", 100, || {
        black_box(draw_list(black_box(&ents)));
    });
    let ent_cmds = draw_list(&ents);
    time("soft: 2000 fills", 100, || soft.render(black_box(&ent_cmds)));

    let mut pw = World::new();
    let spec = Burst { count: 500, color: 0xffffff, speed: 100.0, life: 100.0, size: 3.0, gravity: 0.0 };
    burst(&mut pw, &mut Rng::new(3), (320.0, 180.0), &spec);
    time("particles update 500", 200, || particles(black_box(&mut pw), 1.0 / 60.0));
}
