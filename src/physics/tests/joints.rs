use super::*;

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

#[test]
fn a_spring_swings_at_the_frequency_it_was_given_whatever_the_mass() {
    for mass in [0.5, 4.0] {
        let (mut p, mut w) = (Physics::default(), World::new());
        p.gravity = Vec2::new(0.0, 0.0);
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
    p.gravity = Vec2::new(0.0, 0.0);
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
        Body(Rect::new(355.0, 95.0, 20.0, 20.0)),
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
        let at_pin = c + arm;
        worst_gap = worst_gap.max(at_pin.distance(Vec2::new(pin.0, pin.1)));
        lowest = lowest.max(c.y);
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
fn a_joint_to_a_despawned_body_is_dropped() {
    let (mut p, mut w) = (Physics::default(), World::new());
    let a = boxed(&mut w, square(0.0, 0.0, 10.0), RigidBody::dynamic(1.0), (0.0, 0.0));
    let b = boxed(&mut w, square(30.0, 0.0, 10.0), RigidBody::dynamic(1.0), (0.0, 0.0));
    let rod = p.rod(a, b, 30.0);
    p.step(&mut w, None, DT);
    assert!(p.has_joint(rod));
    w.despawn(b).unwrap();
    p.step(&mut w, None, DT);
    assert!(p.joints.is_empty() && !p.has_joint(rod));
}

#[test]
fn a_joint_can_be_removed_by_its_id_and_one_that_added_nothing_is_not_there() {
    let (mut p, mut w) = (Physics::default(), World::new());
    let a = boxed(&mut w, square(0.0, 0.0, 10.0), RigidBody::dynamic(1.0), (0.0, 0.0));
    let b = boxed(&mut w, square(30.0, 0.0, 10.0), RigidBody::dynamic(1.0), (0.0, 0.0));
    let (rod, rope) = (p.rod(a, b, 30.0), p.rope(a, b, 40.0));
    assert_ne!(rod, rope);
    p.remove_joint(rod);
    assert!(!p.has_joint(rod) && p.has_joint(rope));
    let limp = p.spring(a, b, 30.0, 0.0, 1.0);
    assert!(!p.has_joint(limp), "hz 0 adds nothing");
    let ghost = w.spawn(());
    let pin = p.hinge(&w, a, ghost, (5.0, 5.0));
    assert!(!p.has_joint(pin), "no Body, no hinge");
}
