use super::*;

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
    w.spawn((Body(Rect::new(-500.0, 100.0, 2000.0, 50.0)), RigidBody::fixed().bounce(0.9)));
    let ball = boxed(&mut w, square(0.0, 79.0, 20.0), RigidBody::dynamic(1.0).bounce(0.9), (0.0, 600.0));
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
