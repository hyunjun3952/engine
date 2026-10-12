use crate::render::Soft;
use crate::{
    Body, Burst, Collider, DrawCmd, Font, Gfx, Look, Paint, Physics, RayIndex, Rect, RigidBody, Rng, Sprite, Tilemap,
    Transform, Vel, World, burst, collisions, draw_list, particles, raycast,
};
use std::hint::black_box;
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
    DrawCmd { layer: 0, rect, paint, debug_only: false, xform: Default::default() }
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

    time("raycast 3000 colliders, 100 rays", 10, || {
        for i in 0..100 {
            let a = i as f32 * 0.0628;
            black_box(raycast(&world, (320.0, 180.0), (a.cos(), a.sin()), 400.0, u32::MAX));
        }
    });

    time("raycast index build, 3000 colliders", 10, || {
        black_box(RayIndex::new(&world));
    });
    let index = RayIndex::new(&world);
    time("raycast grid, 100 rays", 100, || {
        for i in 0..100 {
            let a = i as f32 * 0.0628;
            black_box(index.cast((320.0, 180.0), (a.cos(), a.sin()), 400.0, u32::MAX));
        }
    });

    time("font load, 11324 glyphs", 5, || {
        black_box(Font::load("assets/fonts/galmuri9.bdf").unwrap());
    });

    let mut soft = Soft::new(640, 360, 1);
    let full = Rect { x: 0.0, y: 0.0, w: 640.0, h: 360.0 };
    let sprite = Sprite::test_ball();
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

    let map = Tilemap::load("assets/levels/level1.txt", 16, Sprite::test_tiles());
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

    for scale in [2, 3, 4] {
        let mut up = Soft::new(640, 360, scale);
        time(&format!("soft: upscale x{scale}"), 100, || {
            black_box(up.present());
        });
    }

    let turned: Vec<DrawCmd> = sprites
        .iter()
        .take(2000)
        .map(|c| DrawCmd { xform: Transform { angle: 0.6, sx: 1.0, sy: 1.0 }, ..c.clone() })
        .collect();
    time("soft: 2000 sprites, plain", 50, || soft.render(black_box(&sprites[..2000])));
    time("soft: 2000 sprites, rotated", 50, || soft.render(black_box(&turned)));

    for n in [100, 300, 1000] {
        let (mut arena, mut physics) = (World::new(), Physics::default());
        let wall = |x, y, w, h| (Body(Rect { x, y, w, h }), RigidBody::fixed());
        arena.spawn(wall(0.0, 300.0, 640.0, 60.0));
        arena.spawn(wall(0.0, 0.0, 10.0, 360.0));
        arena.spawn(wall(630.0, 0.0, 10.0, 360.0));
        for i in 0..n {
            let rect = Rect { x: 20.0 + rng(&mut seed) * 590.0, y: -(i as f32) * 0.5, w: 8.0, h: 8.0 };
            let body = RigidBody::dynamic(1.0).with_bounce(0.2);
            let body = if i % 2 == 0 { body.circle() } else { body };
            arena.spawn((Body(rect), Vel { x: 0.0, y: 0.0 }, body, Transform::IDENTITY));
        }
        for _ in 0..600 {
            physics.step(&mut arena, None, 1.0 / 60.0);
        }
        for _ in 0..600 {
            physics.step(&mut arena, None, 1.0 / 60.0);
        }
        let (mut fast, mut out, mut sum) = (0, 0, 0.0f32);
        for (v, b, _) in arena.query::<(&Vel, &Body, &RigidBody)>().iter() {
            sum += v.x.hypot(v.y);
            fast += (v.x.hypot(v.y) > 5.0) as u32;
            out += (b.0.y > 360.0 || b.0.x < 0.0 || b.0.x > 640.0) as u32;
        }
        println!("  {n}: mean speed {:.1}, over 5px/s: {fast}, outside the arena: {out}", sum / n as f32);
        time(&format!("physics step, {n} settled bodies"), 60, || physics.step(&mut arena, None, 1.0 / 60.0));
    }
}

#[test]
#[ignore]
fn shake() {
    // Visible shaking in the physics example's scene (bodies start apart): over 8 seeds, how often an awake body's
    // centre moves more than 1.5 px (or 6 px) in one step after the first second, when the pile is asleep, how many
    // bodies end under the floor, and what a step costs.
    for n in [100, 200, 300] {
        let (mut small, mut big, mut calm, mut below, mut worst_ms, mut total_ms) =
            (0u32, 0u32, 0.0f32, 0, 0.0f64, 0.0f64);
        for seed in 1..=8 {
            let (mut rng, mut w, mut p) = (Rng::new(seed), World::new(), Physics::default());
            let wall = |x, y, w, h| (Body(Rect { x, y, w, h }), RigidBody::fixed());
            w.spawn(wall(0.0, 340.0, 640.0, 20.0));
            w.spawn(wall(0.0, 0.0, 10.0, 360.0));
            w.spawn(wall(630.0, 0.0, 10.0, 360.0));
            let mut ents = Vec::new();
            for i in 0..n {
                let size = rng.range(14.0, 34.0);
                let (x, y) = (20.0 + (i % 16) as f32 * 37.0, -((i / 16) as f32) * 40.0);
                let body = RigidBody::dynamic(size * size / 100.0)
                    .with_bounce(rng.range(0.0, 0.7))
                    .with_friction(0.5)
                    .with_spin(rng.range(-4.0, 4.0));
                let turn = Transform { angle: rng.range(0.0, 3.0), ..Transform::IDENTITY };
                ents.push(w.spawn((Body(Rect { x, y, w: size, h: size }), Vel::default(), body, turn)));
            }
            let mut last: Vec<(f32, f32)> = ents.iter().map(|&e| w.get::<&Body>(e).unwrap().0.center()).collect();
            let mut calm_at = 20.0;
            for step in 0..1200 {
                let t = Instant::now();
                p.step(&mut w, None, 1.0 / 60.0);
                let ms = t.elapsed().as_secs_f64() * 1000.0;
                (worst_ms, total_ms) = (worst_ms.max(ms), total_ms + ms);
                let mut awake = 0;
                for (i, &e) in ents.iter().enumerate() {
                    let c = w.get::<&Body>(e).unwrap().0.center();
                    let moved = (c.0 - last[i].0).hypot(c.1 - last[i].1);
                    last[i] = c;
                    if p.asleep(e) || c.1 > 360.0 {
                        continue;
                    }
                    awake += 1;
                    if step >= 60 {
                        small += (moved > 1.5) as u32;
                        big += (moved > 6.0) as u32;
                    }
                }
                if awake == 0 && calm_at == 20.0 {
                    calm_at = step as f32 / 60.0;
                }
            }
            calm += calm_at;
            below += ents.iter().filter(|&&e| w.get::<&Body>(e).unwrap().0.center().1 > 360.0).count();
        }
        println!(
            "  n={n}: jumps > 1.5 px {small}, > 6 px {big}; asleep after {:.1} s on average; under the floor {below}; step {:.3} ms on average, {worst_ms:.1} ms at worst",
            calm / 8.0,
            total_ms / (8.0 * 1200.0)
        );
    }
}
