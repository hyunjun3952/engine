use super::*;

fn angle_of(w: &World, e: Entity) -> f32 {
    w.get::<&Transform>(e).unwrap().angle
}

#[test]
fn a_tilted_box_dropped_on_the_floor_ends_up_flat_and_still() {
    for tilt in [0.3_f32, -0.6, 0.78] {
        let (mut p, mut w) = (Physics::default(), World::new());
        floor(&mut w);
        let e = turning(&mut w, Rect::new(50.0, 20.0, 30.0, 20.0), RigidBody::dynamic(1.0), tilt);
        run(&mut p, &mut w, None, 400);
        let (r, v) = state(&w, e);
        let off = angle_of(&w, e).rem_euclid(std::f32::consts::FRAC_PI_2);
        let off = off.min(std::f32::consts::FRAC_PI_2 - off);
        assert!(off < 0.03, "tilt {tilt}: rests at {} rad", angle_of(&w, e));
        assert!(v.0.abs() < 1.0 && v.1.abs() < 2.0, "tilt {tilt}: still moving {v:?}");
        let centre = r.center().y;
        assert!((centre - 90.0).abs() < 0.6 || (centre - 85.0).abs() < 0.6, "tilt {tilt}: centre at {centre}");
    }
}

#[test]
fn a_resting_box_that_can_turn_does_not_creep_or_spin_up() {
    let (mut p, mut w) = (Physics::default(), World::new());
    floor(&mut w);
    let e = turning(&mut w, Rect::new(50.0, 79.0, 30.0, 20.0), RigidBody::dynamic(1.0), 0.0);
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
    let plank = Rect::new(-400.0, 100.0, 800.0, 20.0);
    w.spawn((Body(plank), RigidBody::fixed().friction(1.0), Transform { angle, sx: 1.0, sy: 1.0 }));
    // On the slope's surface: along the plank's normal, clear of it by the radius.
    let (nx, ny) = (angle.sin(), -angle.cos());
    let (cx, cy) =
        (plank.x + 400.0 + 10.0 * nx - 150.0 * angle.cos(), plank.y + 10.0 + (10.0 + 10.0) * ny - 150.0 * angle.sin());
    let rect = Rect::new(cx - 10.0, cy - 10.0, 20.0, 20.0);
    let ball = turning(&mut w, rect, RigidBody::dynamic(1.0).friction(1.0).circle(), 0.0);
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
    let mut p = Physics { gravity: Vec2::ZERO, ..Physics::default() };
    let mut w = World::new();
    let rb = |m: f32| RigidBody::dynamic(m).friction(0.0).bounce(0.5);
    let ball = turning(&mut w, Rect::new(0.0, 0.0, 10.0, 10.0), rb(1.0).circle(), 0.0);
    let boxed_ = turning(&mut w, Rect::new(40.0, 6.0, 30.0, 30.0), rb(4.0), 0.0);
    w.query_one_mut::<&mut Vel>(ball).unwrap().x = 25.0;
    let momentum = |w: &World| {
        let (mut lin, mut ang) = ((0.0, 0.0), 0.0);
        for (e, m, i) in [(ball, 1.0, 0.5 * 25.0), (boxed_, 4.0, 4.0 * 1800.0 / 12.0)] {
            let (r, v) = state(w, e);
            let Vec2 { x: cx, y: cy } = r.center();
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
        let centre = r.center().y;
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
            let rb = RigidBody::dynamic(wd * ht / 100.0).bounce(rng.range(0.0, 0.5)).friction(0.5);
            let rb = rb.spin(rng.range(-6.0, 6.0));
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
        let (Vec2 { x: cx, y: cy }, near) = (r.center(), r.w.min(r.h) / 2.0);
        assert!(cx >= 9.0 + near && cx <= 631.0 - near && cy <= 341.0 - near, "escaped or sank: {r:?}");
        assert!(v.0.hypot(v.1) < 60.0, "still flying after 15 s: {v:?}");
    }
}
