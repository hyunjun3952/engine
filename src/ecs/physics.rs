use super::collision::overlapping_pairs;
use crate::{Body, Entity, Rect, Tilemap, Vel, World};
use std::collections::HashSet;

const REST_SPEED: f32 = 30.0;
const SLOP: f32 = 0.02;
const CORRECTION: f32 = 0.8;
const CORRECTION_PASSES: usize = 4;

pub struct RigidBody {
    inv_mass: f32,
    pub restitution: f32,
    pub friction: f32,
}

impl RigidBody {
    pub fn dynamic(mass: f32) -> Self {
        assert!(mass > 0.0, "a dynamic body needs a positive mass");
        RigidBody { inv_mass: 1.0 / mass, restitution: 0.0, friction: 0.4 }
    }

    pub fn fixed() -> Self {
        RigidBody { inv_mass: 0.0, restitution: 0.0, friction: 0.4 }
    }

    pub fn with_bounce(mut self, restitution: f32) -> Self {
        self.restitution = restitution;
        self
    }

    pub fn with_friction(mut self, friction: f32) -> Self {
        self.friction = friction;
        self
    }
}

pub struct Physics {
    pub gravity: (f32, f32),
    pub iterations: usize,
}

impl Default for Physics {
    fn default() -> Self {
        Physics { gravity: (0.0, 900.0), iterations: 10 }
    }
}

struct Node {
    rect: Rect,
    before: Rect,
    vel: (f32, f32),
    inv_mass: f32,
    restitution: f32,
    friction: f32,
}

struct Hit {
    a: usize,
    b: usize,
    normal: (f32, f32),
}

impl Physics {
    pub fn step(&self, world: &mut World, tiles: Option<&Tilemap>, dt: f32) {
        let (ents, mut nodes) = gather(world);
        let bodies = nodes.len();
        self.integrate(&mut nodes, dt);
        if let Some(map) = tiles {
            add_tiles(&mut nodes, bodies, map);
        }
        let hits = find_hits(&nodes, bodies, tiles);
        for _ in 0..self.iterations {
            for hit in &hits {
                let (a, b) = pair_mut(&mut nodes, hit.a, hit.b);
                push_apart(a, b, hit.normal);
            }
        }
        for _ in 0..CORRECTION_PASSES {
            for hit in &hits {
                let (a, b) = pair_mut(&mut nodes, hit.a, hit.b);
                separate(a, b, hit.normal);
            }
        }
        write_back(world, &ents, &nodes);
    }

    fn integrate(&self, nodes: &mut [Node], dt: f32) {
        for n in nodes.iter_mut().filter(|n| n.inv_mass > 0.0) {
            n.before = n.rect;
            n.vel.0 += self.gravity.0 * dt;
            n.vel.1 += self.gravity.1 * dt;
            n.rect.x += n.vel.0 * dt;
            n.rect.y += n.vel.1 * dt;
        }
    }
}

fn gather(world: &World) -> (Vec<Entity>, Vec<Node>) {
    let mut query = world.query::<(Entity, &Body, Option<&Vel>, &RigidBody)>();
    query
        .iter()
        .map(|(e, body, vel, rb)| {
            let movable = vel.is_some() && rb.inv_mass > 0.0;
            let node = Node {
                rect: body.0,
                before: body.0,
                vel: vel.map_or((0.0, 0.0), |v| (v.x, v.y)),
                inv_mass: if movable { rb.inv_mass } else { 0.0 },
                restitution: rb.restitution,
                friction: rb.friction,
            };
            (e, node)
        })
        .unzip()
}

fn add_tiles(nodes: &mut Vec<Node>, bodies: usize, map: &Tilemap) {
    let mut seen = HashSet::new();
    for i in 0..bodies {
        if nodes[i].inv_mass == 0.0 {
            continue;
        }
        let r = nodes[i].rect;
        let near = Rect { x: r.x - 1.0, y: r.y - 1.0, w: r.w + 2.0, h: r.h + 2.0 };
        for tile in map.solid_in(near) {
            if seen.insert((tile.x as i32, tile.y as i32)) {
                nodes.push(Node {
                    rect: tile,
                    before: tile,
                    vel: (0.0, 0.0),
                    inv_mass: 0.0,
                    restitution: 0.0,
                    friction: 0.4,
                });
            }
        }
    }
}

fn find_hits(nodes: &[Node], bodies: usize, tiles: Option<&Tilemap>) -> Vec<Hit> {
    let rects: Vec<Rect> = nodes.iter().map(|n| n.rect).collect();
    overlapping_pairs(&rects)
        .into_iter()
        .filter(|&(a, b, ..)| nodes[a].inv_mass + nodes[b].inv_mass > 0.0)
        .map(|(a, b, ox, oy)| {
            let (now_a, now_b) = (&nodes[a].rect, &nodes[b].rect);
            let (was_a, was_b) = (&nodes[a].before, &nodes[b].before);
            let normal = if hit_along_x(was_a, was_b, ox, oy) {
                let side = (was_a.x, was_a.x + was_a.w, was_b.x, was_b.x + was_b.w);
                (push_direction(side, now_a.x + now_a.w / 2.0, now_b.x + now_b.w / 2.0), 0.0)
            } else {
                let side = (was_a.y, was_a.y + was_a.h, was_b.y, was_b.y + was_b.h);
                (0.0, push_direction(side, now_a.y + now_a.h / 2.0, now_b.y + now_b.h / 2.0))
            };
            Hit { a, b, normal }
        })
        .filter(|hit| !tiles.is_some_and(|map| hit.b >= bodies && is_inner_face(map, &nodes[hit.b].rect, hit.normal)))
        .collect()
}

// A face between two solid tiles cannot be touched; a body sliding along the floor would otherwise
// catch on the seam, because the resting overlap of 0.02 px makes the seam look like a wall.
fn is_inner_face(map: &Tilemap, tile: &Rect, (nx, ny): (f32, f32)) -> bool {
    let probe = Rect { x: tile.x + tile.w / 2.0 - nx * tile.w, y: tile.y + tile.h / 2.0 - ny * tile.h, w: 0.1, h: 0.1 };
    !map.solid_in(probe).is_empty()
}

fn hit_along_x(a: &Rect, b: &Rect, ox: f32, oy: f32) -> bool {
    let gap_x = (b.x - (a.x + a.w)).max(a.x - (b.x + b.w));
    let gap_y = (b.y - (a.y + a.h)).max(a.y - (b.y + b.h));
    if gap_x > 0.0 || gap_y > 0.0 { gap_x > gap_y } else { ox < oy }
}

fn push_direction((a_lo, a_hi, b_lo, b_hi): (f32, f32, f32, f32), now_a: f32, now_b: f32) -> f32 {
    if b_lo >= a_hi {
        1.0
    } else if b_hi <= a_lo {
        -1.0
    } else if now_b >= now_a {
        1.0
    } else {
        -1.0
    }
}

fn pair_mut(nodes: &mut [Node], a: usize, b: usize) -> (&mut Node, &mut Node) {
    if a < b {
        let (left, right) = nodes.split_at_mut(b);
        (&mut left[a], &mut right[0])
    } else {
        let (left, right) = nodes.split_at_mut(a);
        (&mut right[0], &mut left[b])
    }
}

fn push_apart(a: &mut Node, b: &mut Node, (nx, ny): (f32, f32)) {
    let inv_sum = a.inv_mass + b.inv_mass;
    let closing = (b.vel.0 - a.vel.0) * nx + (b.vel.1 - a.vel.1) * ny;
    if closing > 0.0 {
        return;
    }
    let bounce = if -closing < REST_SPEED { 0.0 } else { a.restitution.min(b.restitution) };
    let j = -(1.0 + bounce) * closing / inv_sum;
    a.vel = (a.vel.0 - j * a.inv_mass * nx, a.vel.1 - j * a.inv_mass * ny);
    b.vel = (b.vel.0 + j * b.inv_mass * nx, b.vel.1 + j * b.inv_mass * ny);

    let (tx, ty) = (-ny, nx);
    let sliding = (b.vel.0 - a.vel.0) * tx + (b.vel.1 - a.vel.1) * ty;
    let grip = (a.friction * b.friction).sqrt() * j;
    let jt = (-sliding / inv_sum).clamp(-grip, grip);
    a.vel = (a.vel.0 - jt * a.inv_mass * tx, a.vel.1 - jt * a.inv_mass * ty);
    b.vel = (b.vel.0 + jt * b.inv_mass * tx, b.vel.1 + jt * b.inv_mass * ty);
}

fn separate(a: &mut Node, b: &mut Node, (nx, ny): (f32, f32)) {
    let Some((ox, oy)) = a.rect.penetration(&b.rect) else { return };
    let depth = if nx != 0.0 { ox } else { oy };
    let inv_sum = a.inv_mass + b.inv_mass;
    let shift = (depth - SLOP).max(0.0) * CORRECTION / inv_sum;
    a.rect.x -= nx * shift * a.inv_mass;
    a.rect.y -= ny * shift * a.inv_mass;
    b.rect.x += nx * shift * b.inv_mass;
    b.rect.y += ny * shift * b.inv_mass;
}

fn write_back(world: &mut World, ents: &[Entity], nodes: &[Node]) {
    for (e, node) in ents.iter().zip(nodes).filter(|(_, n)| n.inv_mass > 0.0) {
        if let Ok((body, Some(vel))) = world.query_one_mut::<(&mut Body, Option<&mut Vel>)>(*e) {
            body.0 = node.rect;
            (vel.x, vel.y) = node.vel;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Sprite;
    use std::sync::Arc;

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

    fn run(p: &Physics, w: &mut World, tiles: Option<&Tilemap>, steps: usize) {
        for _ in 0..steps {
            p.step(w, tiles, DT);
        }
    }

    #[test]
    fn free_fall_follows_semi_implicit_euler() {
        let (p, mut w) = (Physics::default(), World::new());
        let e = boxed(&mut w, square(0.0, 0.0, 10.0), RigidBody::dynamic(1.0), (0.0, 0.0));
        run(&p, &mut w, None, 60);
        let expected = 900.0 * DT * DT * (60.0 * 61.0 / 2.0);
        assert!((state(&w, e).0.y - expected).abs() < 0.01, "{}", state(&w, e).0.y);
    }

    #[test]
    fn a_box_dropped_on_the_floor_comes_to_rest_exactly_on_top_of_it() {
        let (p, mut w) = (Physics::default(), World::new());
        floor(&mut w);
        let e = boxed(&mut w, square(50.0, 60.0, 20.0), RigidBody::dynamic(1.0), (0.0, 0.0));
        run(&p, &mut w, None, 120);
        let (r, v) = state(&w, e);
        assert!((r.y + r.h - 100.0).abs() < 0.2, "bottom at {}", r.y + r.h);
        assert!(v.1.abs() < 1.0, "still moving at {}", v.1);
    }

    #[test]
    fn restitution_decides_how_much_speed_comes_back() {
        for bounce in [0.25_f32, 0.5, 0.9] {
            let (p, mut w) = (Physics::default(), World::new());
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
        let (p, mut w) = (Physics::default(), World::new());
        let rb = || RigidBody::dynamic(1.0).with_friction(0.5);
        w.spawn((Body(Rect { x: -500.0, y: 100.0, w: 2000.0, h: 50.0 }), RigidBody::fixed().with_friction(0.5)));
        let slow = boxed(&mut w, square(0.0, 80.0, 20.0), rb(), (100.0, 0.0));
        run(&p, &mut w, None, 90);
        let (r, v) = state(&w, slow);
        let expected = 100.0_f32.powi(2) / (2.0 * 0.5 * 900.0);
        assert!(v.0.abs() < 0.5, "still sliding at {}", v.0);
        assert!((r.x - expected).abs() < 1.5, "slid {} expected {expected}", r.x);

        let mut slick = World::new();
        slick.spawn((Body(Rect { x: -500.0, y: 100.0, w: 2000.0, h: 50.0 }), RigidBody::fixed().with_friction(0.0)));
        let e = boxed(&mut slick, square(0.0, 80.0, 20.0), RigidBody::dynamic(1.0).with_friction(0.0), (100.0, 0.0));
        run(&p, &mut slick, None, 90);
        assert!((state(&slick, e).1.0 - 100.0).abs() < 0.5);
    }

    #[test]
    fn a_hard_drop_onto_a_thin_floor_does_not_tunnel_through_it() {
        let (p, mut w) = (Physics::default(), World::new());
        w.spawn((Body(Rect { x: -500.0, y: 100.0, w: 2000.0, h: 20.0 }), RigidBody::fixed()));
        let plank = Rect { x: 0.0, y: -900.0, w: 6.0, h: 20.0 };
        let e = boxed(&mut w, plank, RigidBody::dynamic(1.0), (0.0, 0.0));
        run(&p, &mut w, None, 300);
        let (r, _) = state(&w, e);
        assert!((r.y + r.h - 100.0).abs() < 0.5, "ended at y {} (floor top is 100)", r.y);
    }

    #[test]
    fn a_pile_of_boxes_dropped_from_high_up_stays_inside_the_arena() {
        let (p, mut w) = (Physics::default(), World::new());
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
        run(&p, &mut w, None, 600);
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
            std::sync::Arc::new(crate::Sprite::load("assets/tiles.png")),
        );
        let (p, mut w) = (Physics::default(), World::new());
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
        let (p, mut w) = (Physics::default(), World::new());
        floor(&mut w);
        let stack: Vec<Entity> = (0..4)
            .map(|i| boxed(&mut w, square(50.0, 78.0 - 21.0 * i as f32, 20.0), RigidBody::dynamic(1.0), (0.0, 0.0)))
            .collect();
        run(&p, &mut w, None, 240);
        let settled: Vec<f32> = stack.iter().map(|e| state(&w, *e).0.y).collect();
        run(&p, &mut w, None, 60);
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
        let p = Physics { gravity: (0.0, 0.0), ..Physics::default() };
        let mut w = World::new();
        let rb = |m| RigidBody::dynamic(m).with_bounce(1.0).with_friction(0.0);
        let heavy = boxed(&mut w, square(0.0, 0.0, 20.0), rb(3.0), (100.0, 0.0));
        let light = boxed(&mut w, square(60.0, 0.0, 20.0), rb(1.0), (-100.0, 0.0));
        let (momentum, energy) = (3.0 * 100.0 - 100.0, 0.5 * 3.0 * 100.0_f32.powi(2) + 0.5 * 100.0_f32.powi(2));
        run(&p, &mut w, None, 90);
        let (a, b) = (state(&w, heavy).1.0, state(&w, light).1.0);
        assert!((3.0 * a + b - momentum).abs() < 1.0, "momentum {}", 3.0 * a + b);
        assert!((0.5 * 3.0 * a * a + 0.5 * b * b - energy).abs() < energy * 0.01, "energy");
        assert!(a < 100.0 && b > 0.0, "they really did collide: {a} {b}");
    }

    #[test]
    fn a_heavy_box_drags_a_light_one_along_in_an_inelastic_hit() {
        let p = Physics { gravity: (0.0, 0.0), ..Physics::default() };
        let mut w = World::new();
        let heavy = boxed(&mut w, square(0.0, 0.0, 20.0), RigidBody::dynamic(10.0), (100.0, 0.0));
        let light = boxed(&mut w, square(30.0, 0.0, 20.0), RigidBody::dynamic(1.0), (0.0, 0.0));
        run(&p, &mut w, None, 20);
        let common = 1000.0 / 11.0;
        for e in [heavy, light] {
            let v = state(&w, e).1.0;
            assert!((v - common).abs() < 3.0, "speed {v}, shared speed should be {common}");
        }
    }

    #[test]
    fn a_fixed_box_never_moves_however_hard_it_is_hit() {
        let p = Physics { gravity: (0.0, 0.0), ..Physics::default() };
        let mut w = World::new();
        let wall = w.spawn((Body(square(100.0, 0.0, 20.0)), RigidBody::fixed()));
        let ram = boxed(&mut w, square(0.0, 0.0, 20.0), RigidBody::dynamic(1000.0), (500.0, 0.0));
        run(&p, &mut w, None, 60);
        assert_eq!(w.get::<&Body>(wall).unwrap().0.x, 100.0);
        let (r, v) = state(&w, ram);
        assert!(r.x + r.w <= 100.2 && v.0.abs() < 1.0, "{r:?} {v:?}");
    }

    #[test]
    fn tiles_act_as_floor_and_walls_for_dynamic_bodies() {
        let sheet = Arc::new(Sprite::load("assets/tiles.png"));
        let map = Tilemap::from_ascii(&["....", "....", "####"], 16, sheet);
        let (p, mut w) = (Physics::default(), World::new());
        let e = boxed(&mut w, square(20.0, 0.0, 10.0), RigidBody::dynamic(1.0), (0.0, 0.0));
        run(&p, &mut w, Some(&map), 120);
        let (r, v) = state(&w, e);
        assert!((r.y + r.h - 32.0).abs() < 0.2 && v.1.abs() < 1.0, "{r:?} {v:?}");

        let mut side = World::new();
        let q = Physics { gravity: (0.0, 0.0), ..Physics::default() };
        let b = boxed(&mut side, square(20.0, 5.0, 10.0), RigidBody::dynamic(1.0), (-100.0, 0.0));
        run(&q, &mut side, Some(&map), 60);
        assert!(state(&side, b).0.x >= -0.2, "left the map: {}", state(&side, b).0.x);
    }

    #[test]
    fn bodies_without_velocity_stay_where_they_are() {
        let (p, mut w) = (Physics::default(), World::new());
        let e = w.spawn((Body(square(0.0, 0.0, 10.0)), RigidBody::dynamic(1.0)));
        run(&p, &mut w, None, 30);
        assert_eq!(w.get::<&Body>(e).unwrap().0.y, 0.0);
    }
}
