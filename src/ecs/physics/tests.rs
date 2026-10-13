use super::*;
use crate::Sprite;

const DT: f32 = 1.0 / 60.0;

fn boxed(w: &mut World, rect: Rect, rb: RigidBody, vel: (f32, f32)) -> Entity {
    w.spawn((Body(rect), Vel { x: vel.0, y: vel.1 }, rb))
}

fn square(x: f32, y: f32, size: f32) -> Rect {
    Rect { x, y, w: size, h: size }
}

fn floor(w: &mut World) {
    w.spawn((Body(Rect { x: -500.0, y: 100.0, w: 2000.0, h: 50.0 }), RigidBody::fixed()));
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
    w.spawn((Body(Rect { x: 0.0, y: -100.0, w: 10.0, h: 300.0 }), RigidBody::fixed()));
    w.spawn((Body(Rect { x: 190.0, y: -100.0, w: 10.0, h: 300.0 }), RigidBody::fixed()));
    for i in 0..60 {
        let size = rng.range(6.0, 14.0);
        let rect = Rect { x: rng.range(15.0, 170.0), y: -(i as f32) * 12.0, w: size, h: size };
        let body = RigidBody::dynamic(1.0).with_bounce(0.2).with_spin(rng.range(-3.0, 3.0));
        let body = if i % 3 == 0 { body.circle() } else { body };
        w.spawn((Body(rect), Vel::default(), body, Transform::IDENTITY));
    }
    (Physics::default(), w)
}

#[test]
fn the_same_world_steps_to_exactly_the_same_bits_every_time() {
    // A pile that bounces, rests and falls asleep covers contacts, warm starting and sleeping, which all keep
    // hash maps; their iteration order must not leak into the result.
    let results: Vec<_> = (0..3)
        .map(|_| {
            let (mut p, mut w) = pile(5);
            run(&mut p, &mut w, None, 400);
            fingerprint(&w)
        })
        .collect();
    assert_eq!(results[0], results[1]);
    assert_eq!(results[1], results[2]);
    let (mut p, mut w) = pile(6);
    run(&mut p, &mut w, None, 400);
    assert_ne!(fingerprint(&w), results[0], "a different seed is a different pile, so the check can fail");
}

#[test]
fn free_fall_follows_semi_implicit_euler() {
    let (mut p, mut w) = (Physics::default(), World::new());
    // Big enough that no substeps are needed, since those would change the stepping this test measures.
    let e = boxed(&mut w, square(0.0, 0.0, 400.0), RigidBody::dynamic(1.0), (0.0, 0.0));
    run(&mut p, &mut w, None, 60);
    let expected = 900.0 * DT * DT * (60.0 * 61.0 / 2.0);
    assert!((state(&w, e).0.y - expected).abs() < 0.01, "{}", state(&w, e).0.y);
}

#[test]
fn a_box_dropped_on_the_floor_comes_to_rest_exactly_on_top_of_it() {
    let (mut p, mut w) = (Physics::default(), World::new());
    floor(&mut w);
    let e = boxed(&mut w, square(50.0, 60.0, 20.0), RigidBody::dynamic(1.0), (0.0, 0.0));
    run(&mut p, &mut w, None, 120);
    let (r, v) = state(&w, e);
    assert!((r.y + r.h - 100.0).abs() < 0.2, "bottom at {}", r.y + r.h);
    assert!(v.1.abs() < 1.0, "still moving at {}", v.1);
}

#[test]
fn a_box_whose_mask_leaves_out_the_floors_group_falls_through_it() {
    let (mut p, mut w) = (Physics::default(), World::new());
    w.spawn((Body(Rect { x: -500.0, y: 100.0, w: 2000.0, h: 50.0 }), RigidBody::fixed(), Filter { group: 1, mask: 1 }));
    let e = boxed(&mut w, square(50.0, 60.0, 20.0), RigidBody::dynamic(1.0), (0.0, 0.0));
    w.insert_one(e, Filter { group: 2, mask: 2 }).unwrap();
    run(&mut p, &mut w, None, 120);
    assert!(state(&w, e).0.y > 150.0);
}

#[test]
fn clearing_the_terrain_bit_lets_a_body_through_the_map_but_not_other_bodies() {
    let tiles = Tilemap::from_ascii(&["....", "....", "####"], 20, Sprite::test_tiles());
    let (mut p, mut w) = (Physics::default(), World::new());
    let ghost = boxed(&mut w, square(10.0, 0.0, 10.0), RigidBody::dynamic(1.0), (0.0, 0.0));
    w.insert_one(ghost, Filter { group: 1, mask: !Filter::TERRAIN }).unwrap();
    let solid = boxed(&mut w, square(50.0, 0.0, 10.0), RigidBody::dynamic(1.0), (0.0, 0.0));
    run(&mut p, &mut w, Some(&tiles), 120);
    assert!(state(&w, ghost).0.y > 60.0, "stopped at {}", state(&w, ghost).0.y);
    assert!(state(&w, solid).0.y < 60.0, "fell to {}", state(&w, solid).0.y);
}

#[test]
fn restitution_decides_how_much_speed_comes_back() {
    for bounce in [0.25_f32, 0.5, 0.9] {
        let (mut p, mut w) = (Physics::default(), World::new());
        w.spawn((Body(Rect { x: -500.0, y: 500.0, w: 2000.0, h: 50.0 }), RigidBody::fixed().with_bounce(bounce)));
        let e = boxed(&mut w, square(0.0, 0.0, 10.0), RigidBody::dynamic(1.0).with_bounce(bounce), (0.0, 0.0));
        let mut result = None;
        for _ in 0..200 {
            let before = state(&w, e).1.1;
            p.step(&mut w, None, DT);
            let after = state(&w, e).1.1;
            if before > 0.0 && after < 0.0 {
                result = Some((before + 900.0 * DT, after));
                break;
            }
        }
        let (incoming, outgoing) = result.expect("the box never bounced");
        assert!((outgoing / -incoming - bounce).abs() < 0.03, "bounce {bounce}: {outgoing} vs {incoming}");
    }
}

#[test]
fn friction_stops_a_sliding_box_in_the_distance_coulomb_predicts() {
    let (mut p, mut w) = (Physics::default(), World::new());
    let rb = || RigidBody::dynamic(1.0).with_friction(0.5);
    w.spawn((Body(Rect { x: -500.0, y: 100.0, w: 2000.0, h: 50.0 }), RigidBody::fixed().with_friction(0.5)));
    let slow = boxed(&mut w, square(0.0, 80.0, 20.0), rb(), (100.0, 0.0));
    run(&mut p, &mut w, None, 90);
    let (r, v) = state(&w, slow);
    let expected = 100.0_f32.powi(2) / (2.0 * 0.5 * 900.0);
    assert!(v.0.abs() < 0.5, "still sliding at {}", v.0);
    assert!((r.x - expected).abs() < 1.5, "slid {} expected {expected}", r.x);

    let mut slick = World::new();
    slick.spawn((Body(Rect { x: -500.0, y: 100.0, w: 2000.0, h: 50.0 }), RigidBody::fixed().with_friction(0.0)));
    let e = boxed(&mut slick, square(0.0, 80.0, 20.0), RigidBody::dynamic(1.0).with_friction(0.0), (100.0, 0.0));
    run(&mut p, &mut slick, None, 90);
    assert!((state(&slick, e).1.0 - 100.0).abs() < 0.5);
}

#[test]
fn a_hard_drop_onto_a_thin_floor_does_not_tunnel_through_it() {
    let (mut p, mut w) = (Physics::default(), World::new());
    w.spawn((Body(Rect { x: -500.0, y: 100.0, w: 2000.0, h: 20.0 }), RigidBody::fixed()));
    let plank = Rect { x: 0.0, y: -900.0, w: 6.0, h: 20.0 };
    let e = boxed(&mut w, plank, RigidBody::dynamic(1.0), (0.0, 0.0));
    run(&mut p, &mut w, None, 300);
    let (r, _) = state(&w, e);
    assert!((r.y + r.h - 100.0).abs() < 0.5, "ended at y {} (floor top is 100)", r.y);
}

#[test]
fn a_pile_of_boxes_dropped_from_high_up_stays_inside_the_arena() {
    let (mut p, mut w) = (Physics::default(), World::new());
    let mut rng = crate::Rng::new(7);
    let wall = |x, y, w_, h| (Body(Rect { x, y, w: w_, h }), RigidBody::fixed());
    w.spawn(wall(0.0, 340.0, 640.0, 20.0));
    w.spawn(wall(0.0, 0.0, 10.0, 360.0));
    w.spawn(wall(630.0, 0.0, 10.0, 360.0));
    let boxes: Vec<Entity> = (0..36)
        .map(|i| {
            let size = rng.range(14.0, 34.0);
            let rect = Rect { x: rng.range(20.0, 620.0 - size), y: -20.0 * i as f32, w: size, h: size };
            let rb = RigidBody::dynamic(size * size / 100.0).with_bounce(rng.range(0.0, 0.7)).with_friction(0.5);
            boxed(&mut w, rect, rb, (0.0, 0.0))
        })
        .collect();
    run(&mut p, &mut w, None, 600);
    for e in boxes {
        let (r, _) = state(&w, e);
        assert!(r.x >= 9.0 && r.x + r.w <= 631.0, "left the arena sideways: {r:?}");
        assert!(r.y + r.h <= 341.0, "sank into or through the floor: {r:?}");
    }
}

#[test]
fn a_body_walks_along_a_tile_floor_without_catching_on_the_seams() {
    let map = Tilemap::from_ascii(
        &["....................", "....................", "####################"],
        16,
        Sprite::test_tiles(),
    );
    let (mut p, mut w) = (Physics::default(), World::new());
    let e = boxed(
        &mut w,
        Rect { x: 20.0, y: 0.0, w: 12.0, h: 24.0 },
        RigidBody::dynamic(1.0).with_friction(0.0),
        (0.0, 0.0),
    );
    for _ in 0..120 {
        w.query_one_mut::<&mut Vel>(e).unwrap().x = 100.0;
        p.step(&mut w, Some(&map), 1.0 / 60.0);
    }
    let (r, _) = state(&w, e);
    assert!(r.x > 219.0, "caught on a seam at {r:?}");
}

#[test]
fn a_stack_of_boxes_settles_without_sinking_or_jittering() {
    let (mut p, mut w) = (Physics::default(), World::new());
    floor(&mut w);
    let stack: Vec<Entity> = (0..4)
        .map(|i| boxed(&mut w, square(50.0, 78.0 - 21.0 * i as f32, 20.0), RigidBody::dynamic(1.0), (0.0, 0.0)))
        .collect();
    run(&mut p, &mut w, None, 240);
    let settled: Vec<f32> = stack.iter().map(|e| state(&w, *e).0.y).collect();
    run(&mut p, &mut w, None, 60);
    for (i, e) in stack.iter().enumerate() {
        let (r, v) = state(&w, *e);
        let ideal = 80.0 - 20.0 * i as f32;
        assert!((r.y - ideal).abs() < 1.0, "box {i} at {} wants {ideal}", r.y);
        assert!((r.y - settled[i]).abs() < 0.05, "box {i} still drifting: {} -> {}", settled[i], r.y);
        assert!(v.1.abs() < 5.0 && v.0.abs() < 0.5, "box {i} moves at {v:?}");
    }
}

#[test]
fn an_elastic_collision_conserves_momentum_and_energy_for_unequal_masses() {
    let mut p = Physics { gravity: (0.0, 0.0), ..Physics::default() };
    let mut w = World::new();
    let rb = |m| RigidBody::dynamic(m).with_bounce(1.0).with_friction(0.0);
    let heavy = boxed(&mut w, square(0.0, 0.0, 20.0), rb(3.0), (100.0, 0.0));
    let light = boxed(&mut w, square(60.0, 0.0, 20.0), rb(1.0), (-100.0, 0.0));
    let (momentum, energy) = (3.0 * 100.0 - 100.0, 0.5 * 3.0 * 100.0_f32.powi(2) + 0.5 * 100.0_f32.powi(2));
    run(&mut p, &mut w, None, 90);
    let (a, b) = (state(&w, heavy).1.0, state(&w, light).1.0);
    assert!((3.0 * a + b - momentum).abs() < 1.0, "momentum {}", 3.0 * a + b);
    assert!((0.5 * 3.0 * a * a + 0.5 * b * b - energy).abs() < energy * 0.01, "energy");
    assert!(a < 100.0 && b > 0.0, "they really did collide: {a} {b}");
}

#[test]
fn a_heavy_box_drags_a_light_one_along_in_an_inelastic_hit() {
    let mut p = Physics { gravity: (0.0, 0.0), ..Physics::default() };
    let mut w = World::new();
    let heavy = boxed(&mut w, square(0.0, 0.0, 20.0), RigidBody::dynamic(10.0), (100.0, 0.0));
    let light = boxed(&mut w, square(30.0, 0.0, 20.0), RigidBody::dynamic(1.0), (0.0, 0.0));
    run(&mut p, &mut w, None, 20);
    let common = 1000.0 / 11.0;
    for e in [heavy, light] {
        let v = state(&w, e).1.0;
        assert!((v - common).abs() < 3.0, "speed {v}, shared speed should be {common}");
    }
}

#[test]
fn a_fixed_box_never_moves_however_hard_it_is_hit() {
    let mut p = Physics { gravity: (0.0, 0.0), ..Physics::default() };
    let mut w = World::new();
    let wall = w.spawn((Body(square(100.0, 0.0, 20.0)), RigidBody::fixed()));
    let ram = boxed(&mut w, square(0.0, 0.0, 20.0), RigidBody::dynamic(1000.0), (500.0, 0.0));
    run(&mut p, &mut w, None, 60);
    assert_eq!(w.get::<&Body>(wall).unwrap().0.x, 100.0);
    let (r, v) = state(&w, ram);
    assert!(r.x + r.w <= 100.2 && v.0.abs() < 1.0, "{r:?} {v:?}");
}

#[test]
fn tiles_act_as_floor_and_walls_for_dynamic_bodies() {
    let sheet = Sprite::test_tiles();
    let map = Tilemap::from_ascii(&["....", "....", "####"], 16, sheet);
    let (mut p, mut w) = (Physics::default(), World::new());
    let e = boxed(&mut w, square(20.0, 0.0, 10.0), RigidBody::dynamic(1.0), (0.0, 0.0));
    run(&mut p, &mut w, Some(&map), 120);
    let (r, v) = state(&w, e);
    assert!((r.y + r.h - 32.0).abs() < 0.2 && v.1.abs() < 1.0, "{r:?} {v:?}");

    let mut side = World::new();
    let mut q = Physics { gravity: (0.0, 0.0), ..Physics::default() };
    let b = boxed(&mut side, square(20.0, 5.0, 10.0), RigidBody::dynamic(1.0), (-100.0, 0.0));
    run(&mut q, &mut side, Some(&map), 60);
    assert!(state(&side, b).0.x >= -0.2, "left the map: {}", state(&side, b).0.x);
}

#[test]
fn bodies_without_velocity_stay_where_they_are() {
    let (mut p, mut w) = (Physics::default(), World::new());
    let e = w.spawn((Body(square(0.0, 0.0, 10.0)), RigidBody::dynamic(1.0)));
    run(&mut p, &mut w, None, 30);
    assert_eq!(w.get::<&Body>(e).unwrap().0.y, 0.0);
}

fn tile_floor() -> Tilemap {
    let sheet = Sprite::test_tiles();
    Tilemap::from_ascii(&["....................", "....................", "####################"], 16, sheet)
}

#[test]
fn a_body_walks_left_along_a_tile_floor_without_catching_on_the_seams() {
    let map = tile_floor();
    let (mut p, mut w) = (Physics::default(), World::new());
    let rb = RigidBody::dynamic(1.0).with_friction(0.0);
    let e = boxed(&mut w, Rect { x: 280.0, y: 0.0, w: 12.0, h: 24.0 }, rb, (0.0, 0.0));
    for _ in 0..120 {
        w.query_one_mut::<&mut Vel>(e).unwrap().x = -100.0;
        p.step(&mut w, Some(&map), DT);
    }
    assert!(state(&w, e).0.x < 100.0, "caught on a seam at {:?}", state(&w, e).0);
}

#[test]
fn a_ball_rolls_over_tile_seams_either_way_without_bumping() {
    let map = tile_floor();
    for (start, speed) in [(20.0, 120.0), (280.0, -120.0)] {
        let (mut p, mut w) = (Physics::default(), World::new());
        let rb = RigidBody::dynamic(1.0).with_friction(0.0).circle();
        let e = boxed(&mut w, square(start, 8.0, 16.0), rb, (0.0, 0.0));
        run(&mut p, &mut w, Some(&map), 30);
        let rest = state(&w, e).0.y;
        let mut worst: f32 = 0.0;
        for _ in 0..120 {
            w.query_one_mut::<&mut Vel>(e).unwrap().x = speed;
            p.step(&mut w, Some(&map), DT);
            worst = worst.max((state(&w, e).0.y - rest).abs());
        }
        assert!((state(&w, e).0.x - start).abs() > 200.0, "stuck at {:?}", state(&w, e).0);
        assert!(worst < 0.3, "bumped {worst} px over a seam going {speed}");
    }
}

#[test]
fn a_ball_dropped_on_the_floor_rests_on_top_and_bounces_by_its_restitution() {
    let (mut p, mut w) = (Physics::default(), World::new());
    floor(&mut w);
    let e = boxed(&mut w, square(50.0, 40.0, 20.0), RigidBody::dynamic(1.0).circle(), (0.0, 0.0));
    run(&mut p, &mut w, None, 120);
    let (r, v) = state(&w, e);
    assert!((r.y + r.h - 100.0).abs() < 0.2 && v.1.abs() < 1.0, "{r:?} {v:?}");

    let mut w = World::new();
    w.spawn((Body(Rect { x: -500.0, y: 500.0, w: 2000.0, h: 50.0 }), RigidBody::fixed().with_bounce(0.5)));
    let e = boxed(&mut w, square(0.0, 0.0, 10.0), RigidBody::dynamic(1.0).with_bounce(0.5).circle(), (0.0, 0.0));
    let (mut incoming, mut outgoing) = (0.0, 0.0);
    for _ in 0..200 {
        let before = state(&w, e).1.1;
        p.step(&mut w, None, DT);
        let after = state(&w, e).1.1;
        if before > 0.0 && after < 0.0 {
            (incoming, outgoing) = (before + 900.0 * DT, after);
            break;
        }
    }
    assert!(incoming > 0.0, "the ball never bounced");
    assert!((outgoing / -incoming - 0.5).abs() < 0.03, "{outgoing} vs {incoming}");
}

#[test]
fn equal_balls_meeting_head_on_swap_velocities_and_off_centre_ones_deflect() {
    let mut p = Physics { gravity: (0.0, 0.0), ..Physics::default() };
    let rb = || RigidBody::dynamic(1.0).with_bounce(1.0).with_friction(0.0).circle();
    let mut w = World::new();
    let a = boxed(&mut w, square(0.0, 0.0, 20.0), rb(), (100.0, 0.0));
    let b = boxed(&mut w, square(80.0, 0.0, 20.0), rb(), (-100.0, 0.0));
    run(&mut p, &mut w, None, 60);
    assert!((state(&w, a).1.0 + 100.0).abs() < 1.0 && (state(&w, b).1.0 - 100.0).abs() < 1.0);

    let mut w = World::new();
    let a = boxed(&mut w, square(0.0, 0.0, 20.0), rb(), (100.0, 0.0));
    let b = boxed(&mut w, square(50.0, 14.0, 20.0), rb(), (0.0, 0.0));
    run(&mut p, &mut w, None, 40);
    let (va, vb) = (state(&w, a).1, state(&w, b).1);
    assert!(va.1 < -5.0 && vb.1 > 5.0, "no sideways deflection: {va:?} {vb:?}");
    assert!((va.0 + vb.0 - 100.0).abs() < 1.0 && (va.1 + vb.1).abs() < 1.0, "momentum: {va:?} {vb:?}");
}

#[test]
fn balls_and_boxes_piled_into_an_arena_stay_inside_it() {
    let (mut p, mut w) = (Physics::default(), World::new());
    let mut rng = crate::Rng::new(11);
    let wall = |x, y, w_, h| (Body(Rect { x, y, w: w_, h }), RigidBody::fixed());
    w.spawn(wall(0.0, 340.0, 640.0, 20.0));
    w.spawn(wall(0.0, 0.0, 10.0, 360.0));
    w.spawn(wall(630.0, 0.0, 10.0, 360.0));
    let bodies: Vec<Entity> = (0..40)
        .map(|i| {
            let size = rng.range(14.0, 30.0);
            let rect = Rect { x: rng.range(20.0, 620.0 - size), y: -20.0 * i as f32, w: size, h: size };
            let rb = RigidBody::dynamic(size * size / 100.0).with_bounce(rng.range(0.0, 0.6)).with_friction(0.4);
            boxed(&mut w, rect, if i % 2 == 0 { rb.circle() } else { rb }, (0.0, 0.0))
        })
        .collect();
    run(&mut p, &mut w, None, 600);
    for e in bodies {
        let (r, _) = state(&w, e);
        assert!(r.x >= 9.0 && r.x + r.w <= 631.0 && r.y + r.h <= 341.0, "escaped or sank: {r:?}");
    }
}

#[test]
fn a_dense_pile_comes_to_rest_instead_of_jittering() {
    let (mut p, mut w) = (Physics::default(), World::new());
    let mut rng = crate::Rng::new(5);
    let wall = |x, y, w_, h| (Body(Rect { x, y, w: w_, h }), RigidBody::fixed());
    w.spawn(wall(0.0, 300.0, 640.0, 60.0));
    w.spawn(wall(0.0, 0.0, 10.0, 360.0));
    w.spawn(wall(630.0, 0.0, 10.0, 360.0));
    let bodies: Vec<Entity> = (0..300)
        .map(|i| {
            let rect = Rect { x: rng.range(20.0, 600.0), y: -(i as f32) * 0.5, w: 8.0, h: 8.0 };
            let rb = RigidBody::dynamic(1.0).with_bounce(0.2);
            let rb = if i % 2 == 0 { rb.circle() } else { rb };
            w.spawn((Body(rect), Vel { x: 0.0, y: 0.0 }, rb, Transform::IDENTITY))
        })
        .collect();
    run(&mut p, &mut w, None, 900);
    let speed: f32 = bodies.iter().map(|&e| state(&w, e).1.0.hypot(state(&w, e).1.1)).sum::<f32>() / 300.0;
    assert!(speed < 2.0, "mean speed after 20 s: {speed}");
}

#[test]
fn a_box_at_rest_falls_asleep_and_stays_exactly_where_it_is() {
    let (mut p, mut w) = (Physics::default(), World::new());
    floor(&mut w);
    let e = boxed(&mut w, square(50.0, 60.0, 20.0), RigidBody::dynamic(1.0), (0.0, 0.0));
    run(&mut p, &mut w, None, 10);
    assert!(!p.asleep(e), "asleep while still falling");
    run(&mut p, &mut w, None, 170);
    assert!(p.asleep(e));
    let at = state(&w, e);
    run(&mut p, &mut w, None, 60);
    assert_eq!(state(&w, e), at);
}

#[test]
fn a_body_creeping_beside_a_sleeper_does_not_keep_waking_it() {
    let (mut p, mut w) = (Physics::default(), World::new());
    floor(&mut w);
    let sleeper = boxed(&mut w, square(50.0, 60.0, 20.0), RigidBody::dynamic(1.0), (0.0, 0.0));
    run(&mut p, &mut w, None, 180);
    assert!(p.asleep(sleeper));
    // Faster than a body may sleep at, slower than one that wakes things, and within reach of the sleeper.
    boxed(&mut w, square(70.5, 80.0, 20.0), RigidBody::dynamic(1.0), (8.0, 0.0));
    p.step(&mut w, None, DT);
    assert!(p.asleep(sleeper), "a creeping neighbour woke it, so the pair would never rest");
}

#[test]
fn a_fast_body_wakes_a_sleeper_it_lands_on() {
    let (mut p, mut w) = (Physics::default(), World::new());
    floor(&mut w);
    let low = boxed(&mut w, square(50.0, 60.0, 20.0), RigidBody::dynamic(1.0), (0.0, 0.0));
    run(&mut p, &mut w, None, 180);
    assert!(p.asleep(low));
    let top = boxed(&mut w, square(50.0, -200.0, 20.0), RigidBody::dynamic(1.0), (0.0, 0.0));
    run(&mut p, &mut w, None, 180);
    assert!(!p.asleep(low) || p.asleep(top), "the faller stopped on a sleeper without waking it");
    let (r, _) = state(&w, top);
    assert!((r.y + r.h - 80.0).abs() < 1.0, "top rests on the lower box, bottom at {}", r.y + r.h);
}

#[test]
fn the_game_moving_a_sleeper_wakes_it() {
    let (mut p, mut w) = (Physics::default(), World::new());
    floor(&mut w);
    let e = boxed(&mut w, square(50.0, 60.0, 20.0), RigidBody::dynamic(1.0), (0.0, 0.0));
    run(&mut p, &mut w, None, 180);
    assert!(p.asleep(e));
    w.get::<&mut Body>(e).unwrap().0.y -= 50.0;
    run(&mut p, &mut w, None, 5);
    assert!(!p.asleep(e));
    assert!(state(&w, e).1.1 > 0.0, "gravity pulls it again");
}

#[test]
fn fast_bodies_do_not_pass_through_a_thin_floor() {
    for circle in [false, true] {
        for tilt in [0.0, 0.5] {
            let (mut p, mut w) = (Physics::default(), World::new());
            w.spawn((Body(Rect { x: -5000.0, y: 300.0, w: 10000.0, h: 4.0 }), RigidBody::fixed()));
            let rb = if circle { RigidBody::dynamic(1.0).circle() } else { RigidBody::dynamic(1.0) };
            let e = turning(&mut w, Rect { x: 100.0, y: 0.0, w: 8.0, h: 8.0 }, rb, tilt);
            w.get::<&mut Vel>(e).unwrap().y = 2500.0;
            run(&mut p, &mut w, None, 60);
            let (r, _) = state(&w, e);
            assert!(r.y + r.h <= 301.0, "circle {circle} tilt {tilt} went through: {r:?}");
        }
    }
}

fn swing(rope: bool, start: (f32, f32)) -> (Physics, World, Entity, Entity) {
    let (mut p, mut w) = (Physics::default(), World::new());
    let anchor = w.spawn((Body(square(300.0, 100.0, 10.0)), RigidBody::fixed()));
    let bob = boxed(&mut w, square(start.0, start.1, 10.0), RigidBody::dynamic(1.0), (0.0, 0.0));
    if rope {
        p.rope(anchor, bob, 100.0);
    } else {
        p.rod(anchor, bob, 100.0);
    }
    (p, w, anchor, bob)
}

fn gap(w: &World, a: Entity, b: Entity) -> f32 {
    let rect = |e| w.get::<&Body>(e).unwrap().0;
    let (ra, rb) = (rect(a), rect(b));
    (ra.x - rb.x).hypot(ra.y - rb.y)
}

#[test]
fn a_rod_keeps_its_length_while_the_bob_swings_through_the_bottom() {
    let (mut p, mut w, anchor, bob) = swing(false, (400.0, 100.0));
    let mut lowest = 0.0f32;
    for _ in 0..240 {
        run(&mut p, &mut w, None, 1);
        assert!((gap(&w, anchor, bob) - 100.0).abs() < 1.5, "length drifted to {}", gap(&w, anchor, bob));
        lowest = lowest.max(state(&w, bob).0.y);
    }
    assert!(lowest > 190.0, "the bob never swung down: {lowest}");
}

#[test]
fn a_rope_goes_slack_up_close_and_holds_once_taut() {
    let (mut p, mut w, anchor, bob) = swing(true, (300.0, 130.0));
    run(&mut p, &mut w, None, 6);
    assert!(state(&w, bob).0.y > 135.0, "a slack rope lets the bob fall freely");
    run(&mut p, &mut w, None, 120);
    assert!(gap(&w, anchor, bob) < 101.5, "taut rope holds: {}", gap(&w, anchor, bob));
    assert!(state(&w, bob).0.y > 190.0);
}

#[test]
fn joints_to_despawned_bodies_are_skipped() {
    let (mut p, mut w, anchor, bob) = swing(false, (400.0, 100.0));
    w.despawn(anchor).unwrap();
    run(&mut p, &mut w, None, 30);
    assert!(state(&w, bob).0.y > 200.0, "free fall without its anchor");
    p.clear_joints();
}

fn turning(w: &mut World, rect: Rect, rb: RigidBody, angle: f32) -> Entity {
    w.spawn((Body(rect), Vel { x: 0.0, y: 0.0 }, rb, Transform { angle, sx: 1.0, sy: 1.0 }))
}

fn angle_of(w: &World, e: Entity) -> f32 {
    w.get::<&Transform>(e).unwrap().angle
}

#[test]
fn a_tilted_box_dropped_on_the_floor_ends_up_flat_and_still() {
    for tilt in [0.3_f32, -0.6, 0.78] {
        let (mut p, mut w) = (Physics::default(), World::new());
        floor(&mut w);
        let e = turning(&mut w, Rect { x: 50.0, y: 20.0, w: 30.0, h: 20.0 }, RigidBody::dynamic(1.0), tilt);
        run(&mut p, &mut w, None, 400);
        let (r, v) = state(&w, e);
        let off = angle_of(&w, e).rem_euclid(std::f32::consts::FRAC_PI_2);
        let off = off.min(std::f32::consts::FRAC_PI_2 - off);
        assert!(off < 0.03, "tilt {tilt}: rests at {} rad", angle_of(&w, e));
        assert!(v.0.abs() < 1.0 && v.1.abs() < 2.0, "tilt {tilt}: still moving {v:?}");
        let centre = r.center().1;
        assert!((centre - 90.0).abs() < 0.6 || (centre - 85.0).abs() < 0.6, "tilt {tilt}: centre at {centre}");
    }
}

#[test]
fn a_resting_box_that_can_turn_does_not_creep_or_spin_up() {
    let (mut p, mut w) = (Physics::default(), World::new());
    floor(&mut w);
    let e = turning(&mut w, Rect { x: 50.0, y: 79.0, w: 30.0, h: 20.0 }, RigidBody::dynamic(1.0), 0.0);
    run(&mut p, &mut w, None, 600);
    let (r, _) = state(&w, e);
    assert!((r.x - 50.0).abs() < 0.5 && (r.y + r.h - 100.0).abs() < 0.3, "{r:?}");
    assert!(angle_of(&w, e).abs() < 0.01, "turned to {}", angle_of(&w, e));
    assert!(w.get::<&RigidBody>(e).unwrap().spin.abs() < 0.05);
}

#[test]
fn a_ball_rolls_down_a_slope_at_two_thirds_of_the_slide_acceleration() {
    let (mut p, mut w) = (Physics::default(), World::new());
    let angle = 30.0_f32.to_radians();
    let plank = Rect { x: -400.0, y: 100.0, w: 800.0, h: 20.0 };
    w.spawn((Body(plank), RigidBody::fixed().with_friction(1.0), Transform { angle, sx: 1.0, sy: 1.0 }));
    // On the slope's surface: along the plank's normal, clear of it by the radius.
    let (nx, ny) = (angle.sin(), -angle.cos());
    let (cx, cy) =
        (plank.x + 400.0 + 10.0 * nx - 150.0 * angle.cos(), plank.y + 10.0 + (10.0 + 10.0) * ny - 150.0 * angle.sin());
    let rect = Rect { x: cx - 10.0, y: cy - 10.0, w: 20.0, h: 20.0 };
    let ball = turning(&mut w, rect, RigidBody::dynamic(1.0).with_friction(1.0).circle(), 0.0);
    run(&mut p, &mut w, None, 30);
    let (v0, t0) = (state(&w, ball).1, 30.0 * DT);
    run(&mut p, &mut w, None, 30);
    let v1 = state(&w, ball).1;
    let (s0, s1) = ((v0.0.powi(2) + v0.1.powi(2)).sqrt(), (v1.0.powi(2) + v1.1.powi(2)).sqrt());
    let accel = (s1 - s0) / (30.0 * DT);
    let expected = 900.0 * angle.sin() / 1.5;
    assert!(t0 > 0.0 && (accel / expected - 1.0).abs() < 0.15, "accel {accel}, rolling should give {expected}");
    let spin = w.get::<&RigidBody>(ball).unwrap().spin;
    assert!((spin * 10.0 / s1 - 1.0).abs() < 0.15, "spin {spin} does not match rolling at speed {s1}");
}

#[test]
fn an_off_centre_hit_spins_the_box_and_conserves_both_momenta() {
    let mut p = Physics { gravity: (0.0, 0.0), ..Physics::default() };
    let mut w = World::new();
    let rb = |m: f32| RigidBody::dynamic(m).with_friction(0.0).with_bounce(0.5);
    let ball = turning(&mut w, Rect { x: 0.0, y: 0.0, w: 10.0, h: 10.0 }, rb(1.0).circle(), 0.0);
    let boxed_ = turning(&mut w, Rect { x: 40.0, y: 6.0, w: 30.0, h: 30.0 }, rb(4.0), 0.0);
    w.query_one_mut::<&mut Vel>(ball).unwrap().x = 25.0;
    let momentum = |w: &World| {
        let (mut lin, mut ang) = ((0.0, 0.0), 0.0);
        for (e, m, i) in [(ball, 1.0, 0.5 * 25.0), (boxed_, 4.0, 4.0 * 1800.0 / 12.0)] {
            let (r, v) = state(w, e);
            let (cx, cy) = r.center();
            lin = (lin.0 + m * v.0, lin.1 + m * v.1);
            ang += m * (cx * v.1 - cy * v.0) + i * w.get::<&RigidBody>(e).unwrap().spin;
        }
        (lin, ang)
    };
    let (lin0, ang0) = momentum(&w);
    run(&mut p, &mut w, None, 120);
    let (lin1, ang1) = momentum(&w);
    assert!(w.get::<&RigidBody>(boxed_).unwrap().spin.abs() > 0.1, "the box did not start turning");
    assert!((lin1.0 - lin0.0).abs() < 2.0 && (lin1.1 - lin0.1).abs() < 2.0, "linear {lin0:?} -> {lin1:?}");
    assert!((ang1 - ang0).abs() < ang0.abs() * 0.03, "angular {ang0} -> {ang1}");
}

#[test]
fn tilted_boxes_land_flat_on_a_tile_floor_across_its_seams() {
    let map = tile_floor();
    for x in [16.0_f32, 29.0, 100.0] {
        let (mut p, mut w) = (Physics::default(), World::new());
        let e = turning(&mut w, Rect { x, y: 0.0, w: 24.0, h: 14.0 }, RigidBody::dynamic(1.0), 0.5);
        run(&mut p, &mut w, Some(&map), 300);
        let r = state(&w, e).0;
        let centre = r.center().1;
        assert!(
            (centre - 25.0).abs() < 0.6 || (centre - 20.0).abs() < 0.6,
            "x {x}: centre at {centre} on a floor at 32"
        );
        assert!(state(&w, e).1.1.abs() < 2.0, "x {x}: still falling or bouncing");
    }
}

#[test]
fn spinning_boxes_and_balls_piled_into_an_arena_stay_inside_it() {
    let (mut p, mut w) = (Physics::default(), World::new());
    let mut rng = crate::Rng::new(5);
    let wall = |x, y, w_, h| (Body(Rect { x, y, w: w_, h }), RigidBody::fixed());
    w.spawn(wall(0.0, 340.0, 640.0, 20.0));
    w.spawn(wall(0.0, 0.0, 10.0, 360.0));
    w.spawn(wall(630.0, 0.0, 10.0, 360.0));
    let bodies: Vec<Entity> = (0..40)
        .map(|i| {
            let (wd, ht) = (rng.range(14.0, 34.0), rng.range(14.0, 34.0));
            let rect = Rect { x: rng.range(20.0, 600.0), y: -20.0 * i as f32, w: wd, h: ht };
            let rb = RigidBody::dynamic(wd * ht / 100.0).with_bounce(rng.range(0.0, 0.5)).with_friction(0.5);
            let rb = rb.with_spin(rng.range(-6.0, 6.0));
            if i % 3 == 0 {
                turning(&mut w, Rect { h: wd, ..rect }, rb.circle(), 0.0)
            } else {
                turning(&mut w, rect, rb, rng.range(0.0, 3.0))
            }
        })
        .collect();
    run(&mut p, &mut w, None, 900);
    for e in bodies {
        let (r, v) = state(&w, e);
        // The rect is the unturned box, so judge the centre against the smallest half-size.
        let ((cx, cy), near) = (r.center(), r.w.min(r.h) / 2.0);
        assert!(cx >= 9.0 + near && cx <= 631.0 - near && cy <= 341.0 - near, "escaped or sank: {r:?}");
        assert!(v.0.hypot(v.1) < 60.0, "still flying after 15 s: {v:?}");
    }
}

fn sleeping_pair(p: &mut Physics, w: &mut World) -> (Entity, Entity) {
    floor(w);
    let low = boxed(w, square(50.0, 60.0, 20.0), RigidBody::dynamic(1.0), (0.0, 0.0));
    let top = boxed(w, square(50.0, 30.0, 20.0), RigidBody::dynamic(1.0), (0.0, 0.0));
    run(p, w, None, 240);
    assert!(p.asleep(low) && p.asleep(top));
    (low, top)
}

#[test]
fn a_sleeper_falls_when_the_body_under_it_is_despawned() {
    let (mut p, mut w) = (Physics::default(), World::new());
    let (low, top) = sleeping_pair(&mut p, &mut w);
    w.despawn(low).unwrap();
    run(&mut p, &mut w, None, 120);
    let r = state(&w, top).0;
    assert!((r.y + r.h - 100.0).abs() < 0.5, "bottom at {}", r.y + r.h);
}

#[test]
fn a_sleeper_moves_when_gravity_changes_and_wake_wakes_one() {
    let (mut p, mut w) = (Physics::default(), World::new());
    let (low, top) = sleeping_pair(&mut p, &mut w);
    p.wake(low);
    p.step(&mut w, None, DT);
    assert!(!p.asleep(low) && p.asleep(top));
    p.gravity = (0.0, -900.0);
    run(&mut p, &mut w, None, 30);
    assert!(!p.asleep(top) && state(&w, top).1.1 < 0.0, "flipped gravity lifts it");
}

#[test]
fn a_heavy_box_does_not_squash_a_light_one_into_the_floor() {
    for mass in [10.0, 100.0] {
        let (mut p, mut w) = (Physics::default(), World::new());
        floor(&mut w);
        let light = boxed(&mut w, square(50.0, 60.0, 20.0), RigidBody::dynamic(1.0), (0.0, 0.0));
        let heavy = boxed(&mut w, square(50.0, 0.0, 20.0), RigidBody::dynamic(mass), (0.0, 0.0));
        run(&mut p, &mut w, None, 240);
        let (l, h) = (state(&w, light).0, state(&w, heavy).0);
        assert!(l.y + l.h - 100.0 < 1.0, "mass {mass}: light sunk {} into the floor", l.y + l.h - 100.0);
        assert!(h.y + h.h - l.y < 1.0, "mass {mass}: heavy sunk {} into the light one", h.y + h.h - l.y);
    }
}

#[test]
fn a_sleeper_falls_when_the_tile_under_it_is_removed() {
    let (mut p, mut w) = (Physics::default(), World::new());
    let mut map = tile_floor();
    let e = boxed(&mut w, square(48.0, 20.0, 12.0), RigidBody::dynamic(1.0), (0.0, 0.0));
    run(&mut p, &mut w, Some(&map), 240);
    assert!(p.asleep(e));
    let rest = state(&w, e).0.y;
    map.set(3, 2, 0);
    run(&mut p, &mut w, Some(&map), 60);
    assert!(!p.asleep(e) && state(&w, e).0.y > rest + 8.0, "still at {} (was {rest})", state(&w, e).0.y);
}

#[test]
fn bodies_coming_and_going_elsewhere_do_not_keep_a_resting_box_awake() {
    let (mut p, mut w) = (Physics::default(), World::new());
    floor(&mut w);
    let e = boxed(&mut w, square(50.0, 60.0, 20.0), RigidBody::dynamic(1.0), (0.0, 0.0));
    for i in 0..400 {
        if i % 20 == 0 {
            let passing = boxed(&mut w, square(300.0, 0.0, 8.0), RigidBody::dynamic(1.0), (0.0, 0.0));
            p.step(&mut w, None, DT);
            w.despawn(passing).unwrap();
        }
        p.step(&mut w, None, DT);
    }
    assert!(p.asleep(e), "a body despawned every 20 steps kept the box awake");
    assert!((state(&w, e).0.y + 20.0 - 100.0).abs() < 0.5);
}

#[test]
fn a_bouncy_body_bounces_off_tiles_only_when_the_map_has_a_bounce() {
    let upward = |bounce: f32| {
        let mut map = Tilemap::from_ascii(&["....", "....", "....", "####"], 16, Sprite::test_tiles());
        map.bounce = bounce;
        let (mut w, mut p) = (World::new(), Physics::default());
        let e = boxed(&mut w, square(24.0, 0.0, 8.0), RigidBody::dynamic(1.0).with_bounce(0.9), (0.0, 0.0));
        let mut up = 0.0f32;
        for _ in 0..240 {
            p.step(&mut w, Some(&map), DT);
            up = up.max(-state(&w, e).1.1);
        }
        up
    };
    assert_eq!(upward(0.0), 0.0);
    assert!(upward(0.9) > 100.0, "bounced at {}", upward(0.9));
}

#[test]
fn turning_a_sleeping_body_wakes_it() {
    let (mut w, mut p) = (World::new(), Physics::default());
    floor(&mut w);
    let e = w.spawn((Body(square(40.0, 80.0, 20.0)), Vel::default(), RigidBody::dynamic(1.0), Transform::IDENTITY));
    run(&mut p, &mut w, None, 120);
    assert!(p.asleep(e));
    w.get::<&mut Transform>(e).unwrap().angle = 0.7;
    run(&mut p, &mut w, None, 1);
    assert!(!p.asleep(e), "a body the game turned stayed asleep inside the floor");
}

#[test]
fn a_sleeper_wakes_when_the_game_moves_the_fixed_body_it_rests_on() {
    let (mut w, mut p) = (World::new(), Physics::default());
    let plat = w.spawn((Body(Rect { x: 0.0, y: 100.0, w: 200.0, h: 10.0 }), RigidBody::fixed()));
    let b = boxed(&mut w, square(50.0, 80.0, 20.0), RigidBody::dynamic(1.0), (0.0, 0.0));
    for _ in 0..120 {
        p.step(&mut w, None, DT);
    }
    assert!(p.asleep(b));
    w.get::<&mut Body>(plat).unwrap().0.x = 1000.0;
    for _ in 0..60 {
        p.step(&mut w, None, DT);
    }
    assert!(state(&w, b).0.y > 200.0, "it must fall once the platform is gone");
}

fn substeps_of(w: &World, p: &mut Physics, tiles: Option<&Tilemap>) -> usize {
    let (_, nodes) = gather(w);
    p.substeps(&nodes, tiles, DT)
}

#[test]
fn only_a_fast_body_with_something_in_its_way_makes_the_world_substep() {
    let mut p = Physics::default();
    let mut w = World::new();
    boxed(&mut w, square(0.0, 0.0, 10.0), RigidBody::dynamic(1.0), (0.0, 0.0));
    assert_eq!(substeps_of(&w, &mut p, None), 1);
    // A body that fell out of the world, far from everything, costs nothing extra.
    let lost = boxed(&mut w, square(5000.0, 9000.0, 10.0), RigidBody::dynamic(1.0), (0.0, 4000.0));
    assert_eq!(substeps_of(&w, &mut p, None), 1);
    // The same speed with a wall in its path cuts the step.
    w.despawn(lost).unwrap();
    boxed(&mut w, square(100.0, 0.0, 10.0), RigidBody::dynamic(1.0), (0.0, 4000.0));
    w.spawn((Body(Rect { x: 90.0, y: 40.0, w: 30.0, h: 5.0 }), RigidBody::fixed()));
    assert!(substeps_of(&w, &mut p, None) > 1);
}

#[test]
fn a_fast_body_substeps_for_tiles_it_can_reach_but_not_for_a_masked_out_neighbour() {
    let (mut p, tiles) = (Physics::default(), tile_floor());
    let mut w = World::new();
    let fast = boxed(&mut w, square(100.0, 0.0, 10.0), RigidBody::dynamic(1.0), (0.0, 4000.0));
    assert!(substeps_of(&w, &mut p, Some(&tiles)) > 1, "the floor is within one step");
    w.get::<&mut Body>(fast).unwrap().0.y = 6000.0;
    assert_eq!(substeps_of(&w, &mut p, Some(&tiles)), 1, "far below the map");
    w.get::<&mut Body>(fast).unwrap().0.y = 0.0;
    w.insert_one(fast, Filter { group: 1, mask: !Filter::TERRAIN }).unwrap();
    assert_eq!(substeps_of(&w, &mut p, Some(&tiles)), 1, "it passes through the map");
}

fn platform(w: &mut World, rect: Rect, vel: (f32, f32)) -> Entity {
    boxed(w, rect, RigidBody::kinematic(), vel)
}

#[test]
fn a_kinematic_platform_moves_by_its_velocity_with_no_gravity_and_is_never_pushed() {
    let (mut p, mut w) = (Physics::default(), World::new());
    let plat = platform(&mut w, Rect { x: 0.0, y: 100.0, w: 100.0, h: 10.0 }, (30.0, -12.0));
    let heavy = boxed(&mut w, square(40.0, 70.0, 30.0), RigidBody::dynamic(1000.0), (0.0, 0.0));
    run(&mut p, &mut w, None, 60);
    let (r, v) = state(&w, plat);
    assert!((r.x - 30.0).abs() < 0.01 && (r.y - 88.0).abs() < 0.01, "moved to {r:?}");
    assert_eq!(v, (30.0, -12.0), "its velocity is its own");
    let (b, _) = state(&w, heavy);
    assert!((b.y + b.h - r.y).abs() < 1.0, "the box rides on top: {b:?} on {r:?}");
}

#[test]
fn a_platform_carries_a_box_sideways_by_friction_and_lifts_it() {
    let (mut p, mut w) = (Physics::default(), World::new());
    let plat = platform(&mut w, Rect { x: 0.0, y: 100.0, w: 400.0, h: 10.0 }, (60.0, 0.0));
    let crate_ = boxed(&mut w, square(20.0, 70.0, 30.0), RigidBody::dynamic(1.0).with_friction(0.8), (0.0, 0.0));
    run(&mut p, &mut w, None, 120);
    let (b, _) = state(&w, crate_);
    assert!(b.x > 80.0, "carried along, now at x {}", b.x);
    assert!((b.y + b.h - 100.0).abs() < 1.0, "still standing on it: {b:?}");
    w.get::<&mut Vel>(plat).unwrap().y = -60.0;
    w.get::<&mut Vel>(plat).unwrap().x = 0.0;
    run(&mut p, &mut w, None, 60);
    let (b, _) = state(&w, crate_);
    let (r, _) = state(&w, plat);
    assert!((b.y + b.h - r.y).abs() < 1.5, "lifted with the platform: box bottom {} platform top {}", b.y + b.h, r.y);
}

#[test]
fn a_sleeping_box_on_a_moving_platform_wakes_and_is_carried() {
    let (mut p, mut w) = (Physics::default(), World::new());
    let plat = platform(&mut w, Rect { x: 0.0, y: 100.0, w: 300.0, h: 10.0 }, (0.0, 0.0));
    let b = boxed(&mut w, square(20.0, 70.0, 30.0), RigidBody::dynamic(1.0).with_friction(0.8), (0.0, 0.0));
    run(&mut p, &mut w, None, 90);
    assert!(p.asleep(b), "it settled and slept");
    w.get::<&mut Vel>(plat).unwrap().x = 80.0;
    run(&mut p, &mut w, None, 60);
    assert!(state(&w, b).0.x > 40.0, "it was woken and carried");
}

#[test]
fn a_one_way_platform_holds_what_lands_on_it_and_lets_the_rest_through() {
    let one_way = |w: &mut World| {
        boxed(w, Rect { x: -200.0, y: 100.0, w: 600.0, h: 10.0 }, RigidBody::fixed().one_way(), (0.0, 0.0))
    };
    let (mut p, mut w) = (Physics::default(), World::new());
    one_way(&mut w);
    let lander = boxed(&mut w, square(0.0, 20.0, 20.0), RigidBody::dynamic(1.0), (0.0, 0.0));
    run(&mut p, &mut w, None, 120);
    let (r, _) = state(&w, lander);
    assert!((r.y + r.h - 100.0).abs() < 0.5, "landed on top: {r:?}");

    // From below it goes up through, then lands on the top.
    let (mut p, mut w) = (Physics::default(), World::new());
    one_way(&mut w);
    let jumper = boxed(&mut w, square(0.0, 140.0, 20.0), RigidBody::dynamic(1.0), (0.0, -700.0));
    let mut peaked_above = false;
    for _ in 0..240 {
        p.step(&mut w, None, DT);
        peaked_above |= state(&w, jumper).0.y + 20.0 < 100.0;
    }
    let (r, _) = state(&w, jumper);
    assert!(peaked_above, "it got through to the top");
    assert!((r.y + r.h - 100.0).abs() < 0.5, "then landed on it: {r:?}");

    // Walking into its edge from the side is not blocked either.
    let (mut p, mut w) = (Physics::default(), World::new());
    p.gravity = (0.0, 0.0);
    one_way(&mut w);
    let walker = boxed(&mut w, square(-300.0, 95.0, 20.0), RigidBody::dynamic(1.0), (200.0, 0.0));
    run(&mut p, &mut w, None, 60);
    assert!(state(&w, walker).0.x > -100.0, "the side of a one-way platform lets it pass");
}

#[test]
fn a_one_way_platform_moving_up_lifts_what_is_above_it_and_passes_through_what_is_not() {
    let rising = |w: &mut World| {
        let plat = platform(w, Rect { x: -50.0, y: 200.0, w: 100.0, h: 10.0 }, (0.0, -300.0));
        w.insert_one(plat, RigidBody::kinematic().one_way()).unwrap();
        plat
    };
    let (mut p, mut w) = (Physics::default(), World::new());
    rising(&mut w);
    let above = boxed(&mut w, square(0.0, 170.0, 20.0), RigidBody::dynamic(1.0), (0.0, 0.0));
    run(&mut p, &mut w, None, 20);
    assert!(state(&w, above).0.y < 120.0, "an elevator: the box above it is taken up");

    let (mut p, mut w) = (Physics::default(), World::new());
    p.gravity = (0.0, 0.0);
    rising(&mut w);
    let inside = boxed(&mut w, square(0.0, 195.0, 20.0), RigidBody::dynamic(1.0), (0.0, 0.0));
    run(&mut p, &mut w, None, 20);
    assert!((state(&w, inside).0.y - 195.0).abs() < 0.5, "the platform passed through a body that was not above it");
}

#[test]
fn a_spring_swings_at_the_frequency_it_was_given_whatever_the_mass() {
    for mass in [0.5, 4.0] {
        let (mut p, mut w) = (Physics::default(), World::new());
        p.gravity = (0.0, 0.0);
        let anchor = w.spawn((Body(square(300.0, 100.0, 10.0)), RigidBody::fixed()));
        let bob = boxed(&mut w, square(440.0, 100.0, 10.0), RigidBody::dynamic(mass), (0.0, 0.0));
        p.spring(anchor, bob, 100.0, 2.0, 0.0);
        let mut crossings = Vec::new();
        let mut last = gap(&w, anchor, bob) - 100.0;
        for step in 1..=180 {
            p.step(&mut w, None, DT);
            let now = gap(&w, anchor, bob) - 100.0;
            if last.signum() != now.signum() {
                crossings.push(step as f32 * DT);
            }
            last = now;
        }
        assert!(crossings.len() >= 5, "mass {mass} oscillates: {crossings:?}");
        let period = 2.0 * (crossings[crossings.len() - 1] - crossings[0]) / (crossings.len() - 1) as f32;
        assert!((period - 0.5).abs() < 0.08, "mass {mass}: period {period} against 0.5 for 2 Hz");
    }
}

#[test]
fn damping_makes_a_spring_settle_at_its_length() {
    let (mut p, mut w) = (Physics::default(), World::new());
    p.gravity = (0.0, 0.0);
    let anchor = w.spawn((Body(square(300.0, 100.0, 10.0)), RigidBody::fixed()));
    let bob = boxed(&mut w, square(440.0, 100.0, 10.0), RigidBody::dynamic(1.0), (0.0, 0.0));
    p.spring(anchor, bob, 100.0, 2.0, 0.7);
    run(&mut p, &mut w, None, 300);
    assert!((gap(&w, anchor, bob) - 100.0).abs() < 1.0, "it came to rest at its length: {}", gap(&w, anchor, bob));
}

#[test]
fn a_hinge_lets_a_body_swing_about_its_pin_without_coming_off() {
    let (mut p, mut w) = (Physics::default(), World::new());
    let wall = w.spawn((Body(square(300.0, 100.0, 10.0)), RigidBody::fixed()));
    let bob = w.spawn((
        Body(Rect { x: 355.0, y: 95.0, w: 20.0, h: 20.0 }),
        Vel::default(),
        RigidBody::dynamic(1.0),
        Transform::IDENTITY,
    ));
    let pin = (305.0, 105.0);
    p.hinge(&w, wall, bob, pin);
    let (mut lowest, mut worst_gap) = (0.0f32, 0.0f32);
    for _ in 0..240 {
        p.step(&mut w, None, DT);
        let (r, angle) = (w.get::<&Body>(bob).unwrap().0, w.get::<&Transform>(bob).unwrap().angle);
        let (c, arm) = (r.center(), Vec2::new(-60.0, 0.0).rotated(angle));
        let at_pin = Vec2::new(c.0, c.1) + arm;
        worst_gap = worst_gap.max(at_pin.distance(Vec2::new(pin.0, pin.1)));
        lowest = lowest.max(c.1);
    }
    assert!(worst_gap < 1.5, "the pin stayed together to within {worst_gap}");
    assert!(lowest > 150.0, "it swung down about the pin: lowest centre {lowest}");
}

#[test]
fn a_hinge_with_nothing_to_hold_to_is_ignored() {
    let (mut p, mut w) = (Physics::default(), World::new());
    let bob = boxed(&mut w, square(0.0, 0.0, 10.0), RigidBody::dynamic(1.0), (0.0, 0.0));
    let ghost = w.spawn(());
    w.despawn(ghost).unwrap();
    p.hinge(&w, ghost, bob, (5.0, 5.0));
    run(&mut p, &mut w, None, 10);
    assert!(state(&w, bob).0.y > 0.0, "it just falls");
}

#[test]
fn a_landing_is_reported_once_as_new_and_then_as_resting_even_while_asleep() {
    let (mut p, mut w) = (Physics::default(), World::new());
    floor(&mut w);
    let b = boxed(&mut w, square(0.0, 20.0, 20.0), RigidBody::dynamic(2.0), (0.0, 0.0));
    assert!(p.events().is_empty() && !p.grounded(b));
    let mut began = Vec::new();
    for step in 0..40 {
        p.step(&mut w, None, DT);
        began.extend(p.events_of(b).filter(|c| c.began).map(|_| step));
    }
    assert_eq!(began.len(), 1, "one new contact, at step {began:?}");
    assert!(p.grounded(b));
    let hit = p.events_of(b).next().unwrap();
    assert!(hit.normal_for(b).y > 0.9 && hit.b.is_some() && hit.impulse > 0.0, "{hit:?}");
    run(&mut p, &mut w, None, 120);
    assert!(p.asleep(b), "it fell asleep");
    assert!(p.grounded(b) && p.events_of(b).count() == 1, "the resting pair is still listed while asleep");
    assert!(p.events_of(b).all(|c| !c.began));
    let floor_hit = p.ground(b).unwrap();
    assert!(floor_hit.other(b).is_some() && floor_hit.involves(b), "the floor is an entity, so it is named");
    assert_eq!(floor_hit.other(Entity::DANGLING), None, "an entity that is not in the pair has no other");
}

#[test]
fn a_hard_hit_that_bounces_within_one_step_is_still_seen() {
    let (mut p, mut w) = (Physics::default(), World::new());
    w.spawn((Body(Rect { x: -500.0, y: 100.0, w: 2000.0, h: 50.0 }), RigidBody::fixed().with_bounce(0.9)));
    let ball = boxed(&mut w, square(0.0, 79.0, 20.0), RigidBody::dynamic(1.0).with_bounce(0.9), (0.0, 600.0));
    let mut peak = 0.0f32;
    for _ in 0..4 {
        p.step(&mut w, None, DT);
        peak = peak.max(p.events_of(ball).map(|c| c.impulse).fold(0.0, f32::max));
    }
    assert!(peak > 1000.0, "mass 1 hitting at 600 px/s and bouncing at 0.9 gives an impulse near 1140: {peak}");
}

#[test]
fn a_body_standing_on_a_tile_floor_reports_the_tile_and_leaving_it_ends_the_contact() {
    let (mut p, mut w, tiles) = (Physics::default(), World::new(), tile_floor());
    let b = boxed(&mut w, square(60.0, 10.0, 12.0), RigidBody::dynamic(1.0), (0.0, 0.0));
    for _ in 0..90 {
        p.step(&mut w, Some(&tiles), DT);
    }
    assert!(p.grounded(b) && p.events_of(b).any(|c| c.b.is_none()), "{:?}", p.events());
    assert_eq!(p.ground(b).unwrap().other(b), None, "a tile has no entity");
    w.get::<&mut Vel>(b).unwrap().y = -500.0;
    p.wake(b);
    p.step(&mut w, Some(&tiles), DT);
    p.step(&mut w, Some(&tiles), DT);
    assert!(!p.grounded(b), "in the air after jumping");
}

#[test]
fn a_body_falling_past_the_map_does_not_reach_its_side_walls_however_fast_it_gets() {
    let (mut p, tiles) = (Physics::default(), tile_floor());
    let mut w = World::new();
    boxed(&mut w, square(100.0, 1e9, 10.0), RigidBody::dynamic(1.0), (0.0, 2e7));
    assert_eq!(substeps_of(&w, &mut p, Some(&tiles)), 1);
}

#[test]
fn a_joint_to_a_despawned_body_is_dropped() {
    let (mut p, mut w) = (Physics::default(), World::new());
    let a = boxed(&mut w, square(0.0, 0.0, 10.0), RigidBody::dynamic(1.0), (0.0, 0.0));
    let b = boxed(&mut w, square(30.0, 0.0, 10.0), RigidBody::dynamic(1.0), (0.0, 0.0));
    p.rod(a, b, 30.0);
    p.step(&mut w, None, DT);
    assert_eq!(p.joints.len(), 1);
    w.despawn(b).unwrap();
    p.step(&mut w, None, DT);
    assert!(p.joints.is_empty());
}

fn platform_map() -> Tilemap {
    let one_way = crate::Tile { ch: b'=', id: 3, solid: false, front: false, auto: false, one_way: true, slope: None };
    let legend = [crate::Tile::DEFAULT[0], one_way];
    let rows = ["......", "......", "......", ".====.", "......", "......", "######"];
    Tilemap::try_from_ascii_with(&rows, 16, Sprite::test_tiles(), &legend).unwrap()
}

#[test]
fn a_body_lands_on_a_one_way_tile_and_rests_there() {
    let (mut w, mut p, map) = (World::new(), Physics::default(), platform_map());
    let b = boxed(&mut w, square(40.0, 0.0, 12.0), RigidBody::dynamic(1.0), (0.0, 0.0));
    run(&mut p, &mut w, Some(&map), 240);
    assert!((state(&w, b).0.y + 12.0 - 48.0).abs() < 1.0, "rests on the platform top: {:?}", state(&w, b).0);
    assert!(p.grounded(b));
}

#[test]
fn a_body_rises_through_a_one_way_tile_and_lands_on_top() {
    let (mut w, mut p, map) = (World::new(), Physics::default(), platform_map());
    let b = boxed(&mut w, square(40.0, 64.0, 12.0), RigidBody::dynamic(1.0), (0.0, -400.0));
    run(&mut p, &mut w, Some(&map), 300);
    assert!((state(&w, b).0.y + 12.0 - 48.0).abs() < 1.0, "came up through it and settled on it: {:?}", state(&w, b).0);
}

#[test]
fn a_body_walks_into_a_one_way_tile_from_the_side() {
    let (mut w, mut p, map) = (World::new(), Physics::default(), platform_map());
    p.gravity = (0.0, 0.0);
    let b = boxed(&mut w, square(0.0, 40.0, 12.0), RigidBody::dynamic(1.0), (200.0, 0.0));
    run(&mut p, &mut w, Some(&map), 30);
    assert!(state(&w, b).0.x > 60.0, "it passed through the platform's row: {:?}", state(&w, b).0);
}

#[test]
fn clearing_the_platform_bit_drops_a_body_through_but_not_through_the_ground() {
    let (mut w, mut p, map) = (World::new(), Physics::default(), platform_map());
    let b = boxed(&mut w, square(40.0, 0.0, 12.0), RigidBody::dynamic(1.0), (0.0, 0.0));
    w.insert_one(b, Filter { group: 1, mask: !Filter::PLATFORM }).unwrap();
    run(&mut p, &mut w, Some(&map), 240);
    assert!(
        (state(&w, b).0.y + 12.0 - 96.0).abs() < 1.0,
        "fell past the platform onto the ground: {:?}",
        state(&w, b).0
    );
}

#[test]
fn a_ghost_that_ignores_terrain_ignores_one_way_tiles_with_the_platform_bit_cleared_too() {
    let (mut w, mut p, map) = (World::new(), Physics::default(), platform_map());
    let b = boxed(&mut w, square(40.0, 0.0, 12.0), RigidBody::dynamic(1.0), (0.0, 0.0));
    w.insert_one(b, Filter { group: 1, mask: !(Filter::TERRAIN | Filter::PLATFORM) }).unwrap();
    run(&mut p, &mut w, Some(&map), 240);
    assert!(state(&w, b).0.y > 100.0, "no tile held it: {:?}", state(&w, b).0);
}

// '/' rises to the right (right angle at the bottom right), '\' to the left.
fn slope_map(rows: &[&str]) -> Tilemap {
    let tile = |ch, id, slope| crate::Tile { ch, id, solid: false, front: false, auto: false, one_way: false, slope };
    let legend = [
        crate::Tile::DEFAULT[0],
        tile(b'/', 2, Some(crate::Slope::BottomRight)),
        tile(b'\\', 3, Some(crate::Slope::BottomLeft)),
        tile(b'^', 4, Some(crate::Slope::TopLeft)),
    ];
    Tilemap::try_from_ascii_with(rows, 16, Sprite::test_tiles(), &legend).unwrap()
}

// A ramp rising to the right over two tiles from the floor (y = 64 at x = 32) to a plateau (y = 32 from x = 64).
fn ramp() -> Tilemap {
    slope_map(&["........", "........", ".../####", "../#####", "########"])
}

// The same ramp seen in a mirror: a plateau on the left and a way down to the floor on the right.
fn ramp_down() -> Tilemap {
    slope_map(&["........", "........", "####\\...", "#####\\..", "########"])
}

fn ramp_height(x: f32) -> f32 {
    (64.0 - (x - 32.0)).clamp(32.0, 64.0)
}

#[test]
fn a_box_dropped_on_a_ramp_with_grip_rests_on_the_diagonal() {
    let (mut w, mut p, map) = (World::new(), Physics::default(), ramp());
    let b = boxed(&mut w, square(44.0, 0.0, 10.0), RigidBody::dynamic(1.0).with_friction(4.0), (0.0, 0.0));
    run(&mut p, &mut w, Some(&map), 300);
    let r = state(&w, b).0;
    assert!(p.asleep(b), "it came to rest: {r:?}");
    assert!((r.y + r.h - ramp_height(r.x + r.w)).abs() < 1.5, "its lower right corner is on the line: {r:?}");
}

#[test]
fn a_box_without_grip_slides_down_a_ramp_onto_the_floor() {
    let (mut w, mut p, map) = (World::new(), Physics::default(), ramp());
    let b = boxed(&mut w, square(44.0, 0.0, 10.0), RigidBody::dynamic(1.0).with_friction(0.0), (0.0, 0.0));
    run(&mut p, &mut w, Some(&map), 240);
    let r = state(&w, b).0;
    assert!(r.x < 30.0, "it went down to the left: {r:?}");
    assert!((r.y + r.h - 64.0).abs() < 1.0, "and rests on the floor: {r:?}");
}

#[test]
fn a_walking_body_climbs_a_two_tile_ramp_onto_the_plateau_without_bumping() {
    let (mut w, mut p, map) = (World::new(), Physics::default(), ramp());
    let b = boxed(&mut w, square(8.0, 54.0, 10.0), RigidBody::dynamic(1.0).with_friction(0.3), (60.0, 0.0));
    let (mut last, mut worst_jump) = (state(&w, b).0, 0.0f32);
    for _ in 0..200 {
        w.get::<&mut Vel>(b).unwrap().x = 60.0;
        run(&mut p, &mut w, Some(&map), 1);
        let now = state(&w, b).0;
        worst_jump = worst_jump.max((now.y - last.y).abs());
        last = now;
    }
    assert!(last.x > 90.0, "it got past the ramp: {last:?}");
    assert!((last.y + last.h - 32.0).abs() < 1.0, "and stands on the plateau: {last:?}");
    assert!(worst_jump < 3.0, "no step bigger than a pixel or two: {worst_jump}");
}

#[test]
fn a_ball_rolls_down_a_ramp_and_off_onto_the_floor() {
    let (mut w, mut p, map) = (World::new(), Physics::default(), ramp_down());
    let b = boxed(&mut w, square(20.0, 0.0, 10.0), RigidBody::dynamic(1.0).circle().with_friction(0.4), (60.0, 0.0));
    w.insert_one(b, Transform::IDENTITY).unwrap();
    let mut on_the_diagonal = 0;
    for _ in 0..400 {
        run(&mut p, &mut w, Some(&map), 1);
        let (cx, cy) = state(&w, b).0.center();
        // The line runs from (64, 32) down to (96, 64); a ball of radius 5 touching it has its centre 5 * sqrt 2 above.
        let line = (32.0 + (cx - 64.0)).clamp(32.0, 64.0);
        on_the_diagonal += (cx > 70.0 && cx < 90.0 && (cy - (line - 5.0 * 2f32.sqrt())).abs() < 1.5) as u32;
    }
    assert!(on_the_diagonal > 3, "it followed the diagonal for {on_the_diagonal} steps");
    let r = state(&w, b).0;
    assert!(r.x > 90.0, "it rolled to the right: {r:?}");
    assert!((r.y + r.h - 64.0).abs() < 1.0, "and stopped on the floor, not in it: {r:?}");
}

#[test]
fn a_tilted_box_comes_to_rest_on_a_ramp_without_sinking() {
    let (mut w, mut p, map) = (World::new(), Physics::default(), ramp());
    let b = boxed(&mut w, square(44.0, 0.0, 12.0), RigidBody::dynamic(1.0).with_friction(4.0), (0.0, 0.0));
    w.insert_one(b, Transform { angle: 0.7, ..Transform::IDENTITY }).unwrap();
    run(&mut p, &mut w, Some(&map), 400);
    let r = state(&w, b).0;
    let (cx, cy) = r.center();
    assert!(p.asleep(b), "it settled: {r:?}");
    assert!(cy < ramp_height(cx), "its centre is above the line: {r:?}");
}

#[test]
fn a_ceiling_slope_deflects_a_body_rising_into_it_along_the_diagonal() {
    let map = slope_map(&["^.......", "........", "........", "########"]);
    let (mut w, mut p) = (World::new(), Physics::default());
    p.gravity = (0.0, 0.0);
    let b = boxed(&mut w, square(2.0, 30.0, 8.0), RigidBody::dynamic(1.0).with_friction(0.0), (0.0, -200.0));
    run(&mut p, &mut w, Some(&map), 30);
    let r = state(&w, b).0;
    assert!(r.x > 10.0, "it was pushed sideways along the diagonal instead of rising through: {r:?}");
}
