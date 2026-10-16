use super::*;

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
fn a_joint_added_to_a_sleeper_wakes_it() {
    let (mut p, mut w) = (Physics::default(), World::new());
    floor(&mut w);
    let e = boxed(&mut w, square(50.0, 60.0, 20.0), RigidBody::dynamic(1.0), (0.0, 0.0));
    run(&mut p, &mut w, None, 180);
    assert!(p.asleep(e));
    let anchor = w.spawn((Body(square(150.0, -100.0, 4.0)), RigidBody::fixed()));
    p.rod(anchor, e, 60.0);
    run(&mut p, &mut w, None, 60);
    assert!(!p.asleep(e));
    assert!(state(&w, e).0.y < 40.0, "the rod lifted it off the floor");
}

#[test]
fn a_body_feels_its_own_share_of_gravity_and_a_change_wakes_it() {
    let (mut p, mut w) = (Physics::default(), World::new());
    floor(&mut w);
    let float = boxed(&mut w, square(0.0, 0.0, 10.0), RigidBody::dynamic(1.0).gravity(0.0), (0.0, 0.0));
    let up = boxed(&mut w, square(30.0, 0.0, 10.0), RigidBody::dynamic(1.0).gravity(-0.5), (0.0, 0.0));
    run(&mut p, &mut w, None, 60);
    assert_eq!(state(&w, float).0.y, 0.0);
    assert!((state(&w, up).1.1 + 450.0).abs() < 1.0, "half of gravity, upwards: {:?}", state(&w, up).1);
    run(&mut p, &mut w, None, 60);
    assert!(p.asleep(float));
    w.get::<&mut RigidBody>(float).unwrap().gravity = 1.0;
    run(&mut p, &mut w, None, 10);
    assert!(state(&w, float).0.y > 0.0, "it fell once gravity reached it");
    use crate::Persist;
    let old = RigidBody::read("1 0 0 0 0.2 0.4 0").unwrap();
    assert_eq!(old.gravity, 1.0, "a body saved before the scale existed");
    assert_eq!(RigidBody::read(&RigidBody::dynamic(2.0).gravity(0.25).write()).unwrap().gravity, 0.25);
}

#[test]
fn a_trapdoor_that_stops_meeting_a_sleeper_drops_it() {
    let (mut p, mut w) = (Physics::default(), World::new());
    let door = w.spawn((Body(Rect::new(0.0, 100.0, 200.0, 20.0)), RigidBody::fixed()));
    let e = boxed(&mut w, square(50.0, 60.0, 20.0), RigidBody::dynamic(1.0), (0.0, 0.0));
    run(&mut p, &mut w, None, 180);
    assert!(p.asleep(e));
    w.insert_one(door, Filter { group: 2, mask: 0 }).unwrap();
    run(&mut p, &mut w, None, 30);
    assert!(state(&w, e).0.y > 100.0, "still floating at {:?}", state(&w, e).0);
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
    p.gravity = Vec2::new(0.0, -900.0);
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
    map.set((3, 2), 0);
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
        let e = boxed(&mut w, square(24.0, 0.0, 8.0), RigidBody::dynamic(1.0).bounce(0.9), (0.0, 0.0));
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
    let plat = w.spawn((Body(Rect::new(0.0, 100.0, 200.0, 10.0)), RigidBody::fixed()));
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

#[test]
fn a_sleeper_wakes_when_its_filter_changes() {
    let (mut w, mut p, map) = (World::new(), Physics::default(), platformer_level());
    let b = player(&mut w, 660.0, 200.0);
    run(&mut p, &mut w, Some(&map), 120);
    assert!(p.asleep(b), "it rests on the plank long enough to sleep");
    w.insert_one(b, Filter { group: u32::MAX, mask: !Filter::PLATFORM }).unwrap();
    run(&mut p, &mut w, Some(&map), 90);
    assert!(state(&w, b).0.y > 300.0, "the plank let it go: {:?}", state(&w, b).0);
}
