use crate::render::Soft;
use crate::{
    Body, Burst, Collider, DrawCmd, Font, Gfx, Look, Paint, Physics, Ray, RayIndex, Rect, RigidBody, Rng, Sprite,
    Tilemap, Transform, Vec2, Vel, World, burst, collisions, draw_list, particles, raycast, raycasts,
};
use std::hint::black_box;
use std::time::Instant;

fn rng(seed: &mut u64) -> f32 {
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    (*seed >> 40) as f32 / (1u64 << 24) as f32
}

fn time(name: &str, runs: u32, mut f: impl FnMut()) {
    if runs > 1 {
        f();
    }
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
    let mut boxes = |w: f32, h: f32| {
        let mut world = World::new();
        for _ in 0..3000 {
            let rect = Rect::new(rng(&mut seed) * w, rng(&mut seed) * h, 8.0, 8.0);
            world.spawn((Body(rect), Collider));
        }
        world
    };
    let world = boxes(640.0, 360.0);
    let column = boxes(64.0, 3600.0);
    let mut n = 0;
    time("collisions 3000", 10, || n = collisions(&world).len());
    println!("  contacts: {n}");
    time("collisions 3000 in a tall column", 10, || n = collisions(&column).len());
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
    for i in 0..100 {
        let (dir, from) = (i as f32 * 0.0628, (320.0, 180.0));
        let dir = (dir.cos(), dir.sin());
        let (scan, grid) = (raycast(&world, from, dir, 400.0, u32::MAX), index.cast(from, dir, 400.0, u32::MAX));
        assert_eq!(scan.map(|h| h.t), grid.map(|h| h.t), "ray {i}: the index and the scan disagree");
    }
    time("raycast grid, 100 rays", 100, || {
        for i in 0..100 {
            let a = i as f32 * 0.0628;
            black_box(index.cast((320.0, 180.0), (a.cos(), a.sin()), 400.0, u32::MAX));
        }
    });

    let rays: Vec<_> = (0..100)
        .map(|i| {
            let a = i as f32 * 0.0628;
            Ray::new((320.0, 180.0), (a.cos(), a.sin()), 400.0, u32::MAX)
        })
        .collect();
    time("raycasts 3000 colliders, 100 rays (builds the index)", 100, || {
        black_box(raycasts(&world, black_box(&rays)));
    });

    time("font load, 11324 glyphs", 5, || {
        black_box(Font::load("assets/fonts/galmuri9.bdf").unwrap());
    });

    let mut soft = Soft::new(640, 360, 1);
    let full = Rect::new(0.0, 0.0, 640.0, 360.0);
    let sprite = Sprite::test_ball();
    let sprites: Vec<DrawCmd> = (0..5000)
        .map(|_| {
            let at = Rect {
                x: (rng(&mut seed) * 660.0).floor() - 10.0,
                y: (rng(&mut seed) * 380.0).floor() - 10.0,
                w: 16.0,
                h: 16.0,
            };
            DrawCmd::new(at, Paint::Sprite(sprite.clone()))
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

    time("soft: clear", 200, || soft.render(black_box(&[DrawCmd::new(full, Paint::Clear(0x101018))])));
    time("soft: full-screen alpha", 200, || soft.render(black_box(&[DrawCmd::new(full, Paint::Alpha(0, 150))])));

    let map = Tilemap::load("assets/levels/level1.txt", 16, Sprite::test_tiles()).unwrap();
    let mut tiles = 0;
    time("tilemap draw_list (visible)", 200, || tiles = map.draw_list(full).len());
    println!("  tile cmds: {tiles}");
    let tile_cmds = map.draw_list(full);
    time("soft: tiles", 200, || soft.render(black_box(&tile_cmds)));

    let mut zoomed = Gfx::new(640, 360);
    zoomed.zoom = 1.5;
    let full: Vec<String> = (0..23).map(|_| "#".repeat(60)).collect();
    let wide = Tilemap::from_ascii(&full.iter().map(String::as_str).collect::<Vec<_>>(), 16, Sprite::test_tiles());
    time("gfx: draw a screen of tiles at zoom 1.5", 100, || {
        zoomed.reset();
        zoomed.draw(wide.draw_list(zoomed.view()));
    });
    println!("  zoomed cmds: {}", zoomed.cmds().len());
    time("soft: render them", 100, || soft.render(black_box(zoomed.cmds())));

    let mut ents = World::new();
    for _ in 0..2000 {
        let rect = Rect::new(rng(&mut seed) * 640.0, rng(&mut seed) * 360.0, 8.0, 8.0);
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
    assert_eq!(pw.len(), 500, "particles died during the timing, so it measured a shrinking set");

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
            let rect = Rect::new(20.0 + rng(&mut seed) * 590.0, -(i as f32) * 0.5, 8.0, 8.0);
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
        // Any part of a body inside a wall (floor top 300, side walls 10 and 630) by more than a pixel counts.
        let (mut fast, mut out, mut sum, mut count) = (0, 0, 0.0f32, 0);
        for (v, b, _) in arena.query::<(&Vel, &Body, &RigidBody)>().iter() {
            let r = b.0;
            sum += v.x.hypot(v.y);
            count += 1;
            fast += (v.x.hypot(v.y) > 5.0) as u32;
            out += (r.x < 9.0 || r.x + r.w > 631.0 || r.y + r.h > 301.0) as u32;
        }
        assert_eq!(count, n, "every body should have been counted");
        println!("  {n}: mean speed {:.1}, over 5px/s: {fast}, in or past a wall: {out}", sum / count as f32);
        time(&format!("physics step, {n} settled bodies"), 60, || physics.step(&mut arena, None, 1.0 / 60.0));
    }

    let floor: Vec<String> = (0..23).map(|r| if r < 21 { ".".repeat(40) } else { "#".repeat(40) }).collect();
    let ground = Tilemap::from_ascii(&floor.iter().map(String::as_str).collect::<Vec<_>>(), 16, Sprite::test_tiles());
    for n in [100, 300] {
        let (mut arena, mut physics) = (World::new(), Physics::default());
        for i in 0..n {
            let rect = Rect::new(20.0 + rng(&mut seed) * 590.0, -(i as f32) * 0.5, 8.0, 8.0);
            let body = RigidBody::dynamic(1.0).with_bounce(0.2);
            let body = if i % 2 == 0 { body.circle() } else { body };
            arena.spawn((Body(rect), Vel { x: 0.0, y: 0.0 }, body, Transform::IDENTITY));
        }
        time(&format!("tile physics, {n} bodies falling and settling (600 steps)"), 1, || {
            for _ in 0..600 {
                physics.step(&mut arena, Some(&ground), 1.0 / 60.0);
            }
        });
        for _ in 0..600 {
            physics.step(&mut arena, Some(&ground), 1.0 / 60.0);
        }
        time(&format!("tile physics step, {n} settled bodies"), 60, || {
            physics.step(&mut arena, Some(&ground), 1.0 / 60.0)
        });
    }
}

#[test]
#[ignore]
fn shake() {
    // Visible shaking in the physics example's scene (bodies start apart): over 8 seeds, how often an awake body's
    // centre moves more than 1.5 px (or 6 px) in one step after the first second, when the pile is asleep for good
    // (a body under the floor does not count as awake; `under the floor` reports those), how many bodies end under
    // the floor, and what a step costs.
    let seeds: u32 = std::env::var("SEEDS").ok().and_then(|v| v.parse().ok()).unwrap_or(8).max(1);
    for n in [100, 200, 300] {
        let (mut small, mut big, mut calm, mut below, mut worst_ms, mut total_ms) =
            (0u32, 0u32, 0.0f32, 0, 0.0f64, 0.0f64);
        let (mut worst_at, mut slow, mut substeps) = ((0, 0, 0), 0, [0u32; 17]);
        let mut settled = 0;
        for seed in 1..=seeds {
            let (mut rng, mut w, mut p) = (Rng::new(seed), World::new(), Physics::default());
            let wall = |x, y, w, h| (Body(Rect { x, y, w, h }), RigidBody::fixed());
            w.spawn(wall(0.0, 340.0, 640.0, 20.0));
            w.spawn(wall(0.0, -2000.0, 10.0, 2360.0));
            w.spawn(wall(630.0, -2000.0, 10.0, 2360.0));
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
            let mut last: Vec<Vec2> = ents.iter().map(|&e| w.get::<&Body>(e).unwrap().0.center()).collect();
            let mut last_awake = None;
            for step in 0..1200 {
                let t = Instant::now();
                p.step(&mut w, None, 1.0 / 60.0);
                let ms = t.elapsed().as_secs_f64() * 1000.0;
                if ms > worst_ms {
                    (worst_ms, worst_at) = (ms, (seed, step, p.last_substeps));
                }
                total_ms += ms;
                slow += (ms > 2.0) as u32;
                substeps[p.last_substeps] += 1;
                let mut awake = 0;
                for (i, &e) in ents.iter().enumerate() {
                    let c = w.get::<&Body>(e).unwrap().0.center();
                    let moved = c.distance(last[i]);
                    last[i] = c;
                    if p.asleep(e) || c.y > 360.0 {
                        continue;
                    }
                    awake += 1;
                    if step >= 60 {
                        small += (moved > 1.5) as u32;
                        big += (moved > 6.0) as u32;
                    }
                }
                if awake > 0 {
                    last_awake = Some(step);
                }
            }
            // Settled means asleep from some step to the end, not just asleep once: a sleeper can be woken again.
            if let Some(at) = last_awake.map_or(Some(0), |s| (s + 1 < 1200).then_some(s + 1)) {
                (calm, settled) = (calm + at as f32 / 60.0, settled + 1);
            }
            below += ents.iter().filter(|&&e| w.get::<&Body>(e).unwrap().0.center().y > 360.0).count();
        }
        println!(
            "  n={n}: jumps > 1.5 px {small}, > 6 px {big}; asleep for good after {:.1} s on average ({settled} of {seeds} seeds got there); under the floor {below}; step {:.3} ms on average, {worst_ms:.1} ms at worst",
            calm / settled.max(1) as f32,
            total_ms / (seeds as f64 * 1200.0)
        );
        let (seed, step, subs) = worst_at;
        let spread: Vec<String> =
            substeps.iter().enumerate().filter(|&(_, &c)| c > 0).map(|(k, c)| format!("{k}:{c}")).collect();
        println!(
            "    worst at seed {seed} step {step} with {subs} substeps; {slow} steps over 2 ms; substeps:steps {}",
            spread.join(" ")
        );
    }
}

#[test]
#[ignore]
fn joints() {
    for (bodies, joints) in [(1000, 0), (1000, 200), (3000, 1000)] {
        let mut seed = 7;
        let (mut world, mut physics) = (World::new(), Physics::default());
        physics.gravity = Vec2::new(0.0, 0.0);
        let ents: Vec<_> = (0..bodies)
            .map(|i| {
                let rect = Rect::new((i % 100) as f32 * 12.0, (i / 100) as f32 * 12.0, 8.0, 8.0);
                let vel = Vel { x: rng(&mut seed) * 4.0 - 2.0, y: rng(&mut seed) * 4.0 - 2.0 };
                world.spawn((Body(rect), vel, RigidBody::dynamic(1.0)))
            })
            .collect();
        for j in 0..joints {
            physics.rope(ents[j], ents[(j + 1) % bodies], 14.0);
        }
        time(&format!("physics step, {bodies} bodies, {joints} joints"), 200, || {
            physics.step(&mut world, None, 1.0 / 60.0)
        });
    }
}
