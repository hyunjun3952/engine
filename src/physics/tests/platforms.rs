use super::*;

fn platform(w: &mut World, rect: Rect, vel: (f32, f32)) -> Entity {
    boxed(w, rect, RigidBody::kinematic(), vel)
}

#[test]
fn a_kinematic_platform_moves_by_its_velocity_with_no_gravity_and_is_never_pushed() {
    let (mut p, mut w) = (Physics::default(), World::new());
    let plat = platform(&mut w, Rect::new(0.0, 100.0, 100.0, 10.0), (30.0, -12.0));
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
    let plat = platform(&mut w, Rect::new(0.0, 100.0, 400.0, 10.0), (60.0, 0.0));
    let crate_ = boxed(&mut w, square(20.0, 70.0, 30.0), RigidBody::dynamic(1.0).friction(0.8), (0.0, 0.0));
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
    let plat = platform(&mut w, Rect::new(0.0, 100.0, 300.0, 10.0), (0.0, 0.0));
    let b = boxed(&mut w, square(20.0, 70.0, 30.0), RigidBody::dynamic(1.0).friction(0.8), (0.0, 0.0));
    run(&mut p, &mut w, None, 90);
    assert!(p.asleep(b), "it settled and slept");
    w.get::<&mut Vel>(plat).unwrap().x = 80.0;
    run(&mut p, &mut w, None, 60);
    assert!(state(&w, b).0.x > 40.0, "it was woken and carried");
}

#[test]
fn a_one_way_platform_holds_what_lands_on_it_and_lets_the_rest_through() {
    let one_way =
        |w: &mut World| boxed(w, Rect::new(-200.0, 100.0, 600.0, 10.0), RigidBody::fixed().one_way(), (0.0, 0.0));
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
    p.gravity = Vec2::new(0.0, 0.0);
    one_way(&mut w);
    let walker = boxed(&mut w, square(-300.0, 95.0, 20.0), RigidBody::dynamic(1.0), (200.0, 0.0));
    run(&mut p, &mut w, None, 60);
    assert!(state(&w, walker).0.x > -100.0, "the side of a one-way platform lets it pass");
}

#[test]
fn a_one_way_platform_moving_up_lifts_what_is_above_it_and_passes_through_what_is_not() {
    let rising = |w: &mut World| {
        let plat = platform(w, Rect::new(-50.0, 200.0, 100.0, 10.0), (0.0, -300.0));
        w.insert_one(plat, RigidBody::kinematic().one_way()).unwrap();
        plat
    };
    let (mut p, mut w) = (Physics::default(), World::new());
    rising(&mut w);
    let above = boxed(&mut w, square(0.0, 170.0, 20.0), RigidBody::dynamic(1.0), (0.0, 0.0));
    run(&mut p, &mut w, None, 20);
    assert!(state(&w, above).0.y < 120.0, "an elevator: the box above it is taken up");

    let (mut p, mut w) = (Physics::default(), World::new());
    p.gravity = Vec2::new(0.0, 0.0);
    rising(&mut w);
    let inside = boxed(&mut w, square(0.0, 195.0, 20.0), RigidBody::dynamic(1.0), (0.0, 0.0));
    run(&mut p, &mut w, None, 20);
    assert!((state(&w, inside).0.y - 195.0).abs() < 0.5, "the platform passed through a body that was not above it");
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
    p.gravity = Vec2::new(0.0, 0.0);
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
fn dropping_through_a_one_way_tile_that_sits_on_the_ground_lands_on_the_ground() {
    let one_way = crate::Tile { ch: b'=', id: 3, solid: false, front: false, auto: false, one_way: true, slope: None };
    let rows = ["......", "......", "......", ".====.", "######"];
    let map =
        Tilemap::try_from_ascii_with(&rows, 16, Sprite::test_tiles(), &[crate::Tile::DEFAULT[0], one_way]).unwrap();
    let (mut w, mut p) = (World::new(), Physics::default());
    let b = boxed(&mut w, square(40.0, 0.0, 12.0), RigidBody::dynamic(1.0), (0.0, 0.0));
    w.insert_one(b, Filter { group: 1, mask: !Filter::PLATFORM }).unwrap();
    run(&mut p, &mut w, Some(&map), 240);
    assert!((state(&w, b).0.y + 12.0 - 64.0).abs() < 1.0, "rests on the ground under it: {:?}", state(&w, b).0);
}

#[test]
fn a_ghost_that_ignores_terrain_ignores_one_way_tiles_with_the_platform_bit_cleared_too() {
    let (mut w, mut p, map) = (World::new(), Physics::default(), platform_map());
    let b = boxed(&mut w, square(40.0, 0.0, 12.0), RigidBody::dynamic(1.0), (0.0, 0.0));
    w.insert_one(b, Filter { group: 1, mask: !(Filter::TERRAIN | Filter::PLATFORM) }).unwrap();
    run(&mut p, &mut w, Some(&map), 240);
    assert!(state(&w, b).0.y > 100.0, "no tile held it: {:?}", state(&w, b).0);
}
