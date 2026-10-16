use super::*;

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
    w.spawn((Body(Rect::new(-500.0, 100.0, 2000.0, 50.0)), RigidBody::fixed(), Filter { group: 1, mask: 1 }));
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
        w.spawn((Body(Rect::new(-500.0, 500.0, 2000.0, 50.0)), RigidBody::fixed().bounce(bounce)));
        let e = boxed(&mut w, square(0.0, 0.0, 10.0), RigidBody::dynamic(1.0).bounce(bounce), (0.0, 0.0));
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
    let rb = || RigidBody::dynamic(1.0).friction(0.5);
    w.spawn((Body(Rect::new(-500.0, 100.0, 2000.0, 50.0)), RigidBody::fixed().friction(0.5)));
    let slow = boxed(&mut w, square(0.0, 80.0, 20.0), rb(), (100.0, 0.0));
    run(&mut p, &mut w, None, 90);
    let (r, v) = state(&w, slow);
    let expected = 100.0_f32.powi(2) / (2.0 * 0.5 * 900.0);
    assert!(v.0.abs() < 0.5, "still sliding at {}", v.0);
    assert!((r.x - expected).abs() < 1.5, "slid {} expected {expected}", r.x);

    let mut slick = World::new();
    slick.spawn((Body(Rect::new(-500.0, 100.0, 2000.0, 50.0)), RigidBody::fixed().friction(0.0)));
    let e = boxed(&mut slick, square(0.0, 80.0, 20.0), RigidBody::dynamic(1.0).friction(0.0), (100.0, 0.0));
    run(&mut p, &mut slick, None, 90);
    assert!((state(&slick, e).1.0 - 100.0).abs() < 0.5);
}

#[test]
fn a_hard_drop_onto_a_thin_floor_does_not_tunnel_through_it() {
    let (mut p, mut w) = (Physics::default(), World::new());
    w.spawn((Body(Rect::new(-500.0, 100.0, 2000.0, 20.0)), RigidBody::fixed()));
    let plank = Rect::new(0.0, -900.0, 6.0, 20.0);
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
            let rb = RigidBody::dynamic(size * size / 100.0).bounce(rng.range(0.0, 0.7)).friction(0.5);
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
    let e = boxed(&mut w, Rect::new(20.0, 0.0, 12.0, 24.0), RigidBody::dynamic(1.0).friction(0.0), (0.0, 0.0));
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
    let mut p = Physics { gravity: Vec2::ZERO, ..Physics::default() };
    let mut w = World::new();
    let rb = |m| RigidBody::dynamic(m).bounce(1.0).friction(0.0);
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
    let mut p = Physics { gravity: Vec2::ZERO, ..Physics::default() };
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
    let mut p = Physics { gravity: Vec2::ZERO, ..Physics::default() };
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
    let mut q = Physics { gravity: Vec2::ZERO, ..Physics::default() };
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

#[test]
fn a_body_walks_left_along_a_tile_floor_without_catching_on_the_seams() {
    let map = tile_floor();
    let (mut p, mut w) = (Physics::default(), World::new());
    let rb = RigidBody::dynamic(1.0).friction(0.0);
    let e = boxed(&mut w, Rect::new(280.0, 0.0, 12.0, 24.0), rb, (0.0, 0.0));
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
        let rb = RigidBody::dynamic(1.0).friction(0.0).circle();
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
    w.spawn((Body(Rect::new(-500.0, 500.0, 2000.0, 50.0)), RigidBody::fixed().bounce(0.5)));
    let e = boxed(&mut w, square(0.0, 0.0, 10.0), RigidBody::dynamic(1.0).bounce(0.5).circle(), (0.0, 0.0));
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
    let mut p = Physics { gravity: Vec2::ZERO, ..Physics::default() };
    let rb = || RigidBody::dynamic(1.0).bounce(1.0).friction(0.0).circle();
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
            let rb = RigidBody::dynamic(size * size / 100.0).bounce(rng.range(0.0, 0.6)).friction(0.4);
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
            let rb = RigidBody::dynamic(1.0).bounce(0.2);
            let rb = if i % 2 == 0 { rb.circle() } else { rb };
            w.spawn((Body(rect), Vel { x: 0.0, y: 0.0 }, rb, Transform::IDENTITY))
        })
        .collect();
    run(&mut p, &mut w, None, 900);
    let speed: f32 = bodies.iter().map(|&e| state(&w, e).1.0.hypot(state(&w, e).1.1)).sum::<f32>() / 300.0;
    assert!(speed < 2.0, "mean speed after 20 s: {speed}");
}

#[test]
fn a_negative_or_nan_friction_is_no_friction_instead_of_a_panic() {
    for bad in [-1.0, f32::NAN, f32::INFINITY] {
        let mut w = World::new();
        floor(&mut w);
        boxed(&mut w, square(0.0, 60.0, 20.0), RigidBody::dynamic(1.0).friction(bad), (50.0, 0.0));
        run(&mut Physics::default(), &mut w, None, 60);
    }
}
