use super::*;

#[test]
fn fast_bodies_do_not_pass_through_a_thin_floor() {
    for circle in [false, true] {
        for tilt in [0.0, 0.5] {
            let (mut p, mut w) = (Physics::default(), World::new());
            w.spawn((Body(Rect::new(-5000.0, 300.0, 10000.0, 4.0)), RigidBody::fixed()));
            let rb = if circle { RigidBody::dynamic(1.0).circle() } else { RigidBody::dynamic(1.0) };
            let e = turning(&mut w, Rect::new(100.0, 0.0, 8.0, 8.0), rb, tilt);
            w.get::<&mut Vel>(e).unwrap().y = 2500.0;
            run(&mut p, &mut w, None, 60);
            let (r, _) = state(&w, e);
            assert!(r.y + r.h <= 301.0, "circle {circle} tilt {tilt} went through: {r:?}");
        }
    }
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
    w.spawn((Body(Rect::new(90.0, 40.0, 30.0, 5.0)), RigidBody::fixed()));
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
