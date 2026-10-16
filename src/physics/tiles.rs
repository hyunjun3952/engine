use super::{Filter, FxSet, Hit, Node, Rect, Sweep, Tilemap, V2, Vec2, touch};

pub(super) fn add_tiles(nodes: &mut Vec<Node>, bodies: usize, map: &Tilemap) {
    let mut seen = FxSet::default();
    for i in 0..bodies {
        if nodes[i].inv_mass == 0.0 {
            continue;
        }
        let r = nodes[i].bounds();
        let near = Rect::new(r.x - 1.0, r.y - 1.0, r.w + 2.0, r.h + 2.0);
        for tile in map.solid_in(near) {
            if seen.insert((tile.x as i32, tile.y as i32)) {
                let s = map.size as f32;
                let one_way = map.one_way_at((tile.x / s).floor() as i32, (tile.y / s).floor() as i32);
                let group = if one_way { Filter::PLATFORM } else { Filter::TERRAIN };
                let filter = Filter { group, mask: u32::MAX };
                let slope = map.slope_at((tile.x / s).floor() as i32, (tile.y / s).floor() as i32);
                nodes.push(Node { restitution: map.bounce, one_way, slope, filter, ..Node::wall(tile) });
            }
        }
    }
}

pub(super) fn find_hits(nodes: &[Node], bodies: usize, tiles: Option<&Tilemap>, sweep: &mut Sweep) -> Vec<Hit> {
    let bounds: Vec<Rect> = nodes.iter().map(Node::bounds).collect();
    sweep
        .overlaps(&bounds, |i| nodes[i].inv_mass == 0.0)
        .iter()
        .copied()
        .filter(|&(a, b, ..)| nodes[a].inv_mass + nodes[b].inv_mass > 0.0 && nodes[a].filter.meets(&nodes[b].filter))
        // A tile always goes second, which is where the seam filters below expect it.
        .map(|(a, b, ..)| if a >= bodies && b < bodies { (b, a) } else { (a, b) })
        .filter_map(|(a, b)| {
            let mut touch = touch(&nodes[a], &nodes[b], false)?;
            if !passes_one_way(&nodes[a], &nodes[b], touch.normal) {
                return None;
            }
            if let Some(map) = tiles.filter(|_| b >= bodies && nodes[b].slope.is_none()) {
                let tile = &nodes[b].rect;
                if touch.whole_face {
                    let (nx, ny) = touch.normal;
                    let Vec2 { x: tx, y: ty } = tile.center();
                    let behind = Rect { x: tx - nx * tile.w, y: ty - ny * tile.h, ..*tile };
                    if hides(map, behind.x, behind.y, ny > 0.5) {
                        return None;
                    }
                } else {
                    touch.pts.retain(|p| !is_hidden(map, tile, p.at, touch.normal));
                    if touch.pts.is_empty() && !nodes[a].circle {
                        // The body's face met a seam corner of the floor; its vertex hitting the tile's face is the real contact.
                        touch = self::touch(&nodes[a], &nodes[b], true)?;
                        touch.pts.retain(|p| !is_hidden(map, tile, p.at, touch.normal));
                    }
                }
            }
            (!touch.pts.is_empty()).then_some(Hit { a, b, normal: touch.normal, pts: touch.pts, key: (0, 0) })
        })
        .collect()
}

// How far into a platform's top a body that was above it may already have sunk (positional correction leaves some).
pub(super) const ONE_WAY_GRACE: f32 = 2.0;

// A one-way platform only holds what is pressed onto its top, and only what was above it before the step.
pub(super) fn passes_one_way(a: &Node, b: &Node, normal: V2) -> bool {
    let (platform, other, down) = match (a.one_way, b.one_way) {
        (false, false) => return true,
        (_, true) => (b, a, normal.1),
        (true, false) => (a, b, -normal.1),
    };
    down > 0.5 && other.before.y + other.before.h <= platform.before.y + ONE_WAY_GRACE
}

// Whether the tile at a point covers a neighbour's face; a one-way tile above covers nothing, or a body dropping
// through it would also pass the ground it sits on.
pub(super) fn hides(map: &Tilemap, x: f32, y: f32, above: bool) -> bool {
    let s = map.size as f32;
    let (tx, ty) = ((x / s).floor() as i32, (y / s).floor() as i32);
    map.is_solid(tx, ty) && !(above && map.one_way_at(tx, ty))
}

// A tile face or corner hidden by a solid neighbour is dropped, or a body walking the floor catches on seams.
pub(super) fn is_hidden(map: &Tilemap, tile: &Rect, (px, py): V2, normal: V2) -> bool {
    let Vec2 { x: mx, y: my } = tile.center();
    let (dx, dy) = (px - mx, py - my);
    let toward = (-normal.0, -normal.1);
    let blocked = |d: f32, t: f32, half: f32| d.abs() >= half - 0.05 && d.signum() * t > 1e-6;
    (blocked(dx, toward.0, tile.w / 2.0) && hides(map, mx + dx.signum() * tile.w, my, false))
        || (blocked(dy, toward.1, tile.h / 2.0) && hides(map, mx, my + dy.signum() * tile.h, dy < 0.0))
}
