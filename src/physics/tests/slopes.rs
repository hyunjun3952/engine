use super::*;

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
    let b = boxed(&mut w, square(44.0, 0.0, 10.0), RigidBody::dynamic(1.0).friction(4.0), (0.0, 0.0));
    run(&mut p, &mut w, Some(&map), 300);
    let r = state(&w, b).0;
    assert!(p.asleep(b), "it came to rest: {r:?}");
    assert!((r.y + r.h - ramp_height(r.x + r.w)).abs() < 1.5, "its lower right corner is on the line: {r:?}");
}

#[test]
fn a_box_without_grip_slides_down_a_ramp_onto_the_floor() {
    let (mut w, mut p, map) = (World::new(), Physics::default(), ramp());
    let b = boxed(&mut w, square(44.0, 0.0, 10.0), RigidBody::dynamic(1.0).friction(0.0), (0.0, 0.0));
    run(&mut p, &mut w, Some(&map), 240);
    let r = state(&w, b).0;
    assert!(r.x < 30.0, "it went down to the left: {r:?}");
    assert!((r.y + r.h - 64.0).abs() < 1.0, "and rests on the floor: {r:?}");
}

#[test]
fn a_walking_body_climbs_a_two_tile_ramp_onto_the_plateau_without_bumping() {
    let (mut w, mut p, map) = (World::new(), Physics::default(), ramp());
    let b = boxed(&mut w, square(8.0, 54.0, 10.0), RigidBody::dynamic(1.0).friction(0.3), (60.0, 0.0));
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
    let b = boxed(&mut w, square(20.0, 0.0, 10.0), RigidBody::dynamic(1.0).circle().friction(0.4), (60.0, 0.0));
    w.insert_one(b, Transform::IDENTITY).unwrap();
    let mut on_the_diagonal = 0;
    for _ in 0..400 {
        run(&mut p, &mut w, Some(&map), 1);
        let Vec2 { x: cx, y: cy } = state(&w, b).0.center();
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
    let b = boxed(&mut w, square(44.0, 0.0, 12.0), RigidBody::dynamic(1.0).friction(4.0), (0.0, 0.0));
    w.insert_one(b, Transform { angle: 0.7, ..Transform::IDENTITY }).unwrap();
    run(&mut p, &mut w, Some(&map), 400);
    let r = state(&w, b).0;
    let Vec2 { x: cx, y: cy } = r.center();
    assert!(p.asleep(b), "it settled: {r:?}");
    assert!(cy < ramp_height(cx), "its centre is above the line: {r:?}");
}

#[test]
fn a_ceiling_slope_deflects_a_body_rising_into_it_along_the_diagonal() {
    let map = slope_map(&["^.......", "........", "........", "########"]);
    let (mut w, mut p) = (World::new(), Physics::default());
    p.gravity = Vec2::new(0.0, 0.0);
    let b = boxed(&mut w, square(2.0, 30.0, 8.0), RigidBody::dynamic(1.0).friction(0.0), (0.0, -200.0));
    run(&mut p, &mut w, Some(&map), 30);
    let r = state(&w, b).0;
    assert!(r.x > 10.0, "it was pushed sideways along the diagonal instead of rising through: {r:?}");
}
