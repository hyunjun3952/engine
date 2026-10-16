use super::*;

#[test]
fn the_platformer_player_walks_over_a_hill_and_back_down_without_jerks() {
    let (mut w, mut p, map) = (World::new(), Physics::default(), platformer_level());
    let b = player(&mut w, 440.0, 328.0);
    let (mut last, mut worst, mut highest) = (state(&w, b).0, 0.0f32, f32::MAX);
    for _ in 0..200 {
        let up = if p.grounded(b) { 0.0 } else { f32::MIN };
        let mut vel = w.get::<&mut Vel>(b).unwrap();
        (vel.x, vel.y) = (120.0, vel.y.max(up));
        drop(vel);
        p.step(&mut w, Some(&map), DT);
        let now = state(&w, b).0;
        worst = worst.max(last.y - now.y);
        highest = highest.min(now.y + now.h);
        last = now;
    }
    assert!(last.x > 640.0, "it crossed the hill: {last:?}");
    assert!((last.y + last.h - 352.0).abs() < 1.0, "and stands on the floor again: {last:?}");
    assert!((highest - 320.0).abs() < 1.5, "it went up to the plateau: {highest}");
    assert!(worst < 2.5, "it never jumps upwards by more than the 2 px a 45 degree climb needs: {worst}");
}

#[test]
fn the_platformer_player_standing_on_a_slope_stays_put() {
    let (mut w, mut p, map) = (World::new(), Physics::default(), platformer_level());
    let b = player(&mut w, 498.0, 280.0);
    run(&mut p, &mut w, Some(&map), 60);
    let settled = state(&w, b).0;
    for _ in 0..180 {
        w.get::<&mut Vel>(b).unwrap().x = 0.0;
        p.step(&mut w, Some(&map), DT);
    }
    let now = state(&w, b).0;
    assert!((now.x - settled.x).abs() < 6.0 && (now.y - settled.y).abs() < 6.0, "{settled:?} -> {now:?}");
    assert!(p.grounded(b));
}

#[test]
fn the_platformer_planks_hold_the_player_until_the_drop_bit_is_cleared() {
    let (mut w, mut p, map) = (World::new(), Physics::default(), platformer_level());
    let b = player(&mut w, 660.0, 200.0);
    run(&mut p, &mut w, Some(&map), 90);
    assert!((state(&w, b).0.y + 24.0 - 272.0).abs() < 1.0, "on the plank: {:?}", state(&w, b).0);
    w.insert_one(b, Filter { group: u32::MAX, mask: !Filter::PLATFORM }).unwrap();
    run(&mut p, &mut w, Some(&map), 90);
    assert!((state(&w, b).0.y + 24.0 - 352.0).abs() < 1.0, "dropped to the floor: {:?}", state(&w, b).0);
}
