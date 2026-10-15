// Making contacts: where two shapes touch, the normal between them and how deep, for every pair of shapes.

use super::{Node, Pt, Pts, V2, dot, sub};
use crate::{Rect, Vec2};

pub(super) struct Touch {
    pub(super) normal: V2,
    pub(super) pts: Pts,
    // Axis-aligned boxes: the normal alone says which tile face is touched.
    pub(super) whole_face: bool,
}

// Normal points from a to b. Untilted boxes use where they were before the step (see `hit_along_x`);
// everything else only has current positions, so a fast circle or tilted box can still tunnel.
pub(super) fn touch(a: &Node, b: &Node, tile_axes_only: bool) -> Option<Touch> {
    if let Some(slope) = b.slope {
        return slope_touch(a, slope.triangle(&b.rect));
    }
    match (a.circle, b.circle) {
        (false, false) if a.angle == 0.0 && b.angle == 0.0 => axis_aligned(a, b),
        (false, false) => tilted_boxes(a, b, tile_axes_only),
        (true, true) => {
            let (ca, cb) = (a.center(), b.center());
            let (dx, dy) = sub(cb, ca);
            let reach = a.radius() + b.radius();
            let dist = (dx * dx + dy * dy).sqrt();
            if dist >= reach {
                return None;
            }
            let normal = if dist > 1e-6 { (dx / dist, dy / dist) } else { (0.0, 1.0) };
            let depth = reach - dist;
            let at = (ca.0 + normal.0 * (a.radius() - depth / 2.0), ca.1 + normal.1 * (a.radius() - depth / 2.0));
            Some(Touch { normal, pts: Pts::one(Pt::new(at, depth)), whole_face: false })
        }
        (true, false) => circle_box(a, b).map(|t| Touch { normal: (-t.normal.0, -t.normal.1), ..t }),
        (false, true) => circle_box(b, a),
    }
}

fn axis_aligned(a: &Node, b: &Node) -> Option<Touch> {
    let Vec2 { x: ox, y: oy } = a.rect.penetration(b.rect)?;
    let along_x = hit_along_x(&a.before, &b.before, ox, oy);
    let (x0, x1) = (a.rect.x.max(b.rect.x), (a.rect.x + a.rect.w).min(b.rect.x + b.rect.w));
    let (y0, y1) = (a.rect.y.max(b.rect.y), (a.rect.y + a.rect.h).min(b.rect.y + b.rect.h));
    let (normal, depth, pts) = if along_x {
        let side = (a.before.x, a.before.x + a.before.w, b.before.x, b.before.x + b.before.w);
        let at = (x0 + x1) / 2.0;
        ((push_direction(side, a.center().0, b.center().0), 0.0), ox, [(at, y0), (at, y1)])
    } else {
        let side = (a.before.y, a.before.y + a.before.h, b.before.y, b.before.y + b.before.h);
        let at = (y0 + y1) / 2.0;
        ((0.0, push_direction(side, a.center().1, b.center().1)), oy, [(x0, at), (x1, at)])
    };
    Some(Touch { normal, pts: pts.map(|at| Pt::new(at, depth)).into_iter().collect(), whole_face: true })
}

// Separating axis test over the four face normals, then clipping the incident edge against the reference face.
fn tilted_boxes(a: &Node, b: &Node, only_b_axes: bool) -> Option<Touch> {
    let (ca, cb) = (a.center(), b.center());
    let d = sub(cb, ca);
    let (aa, ab) = (a.axes(), b.axes());
    let (ha, hb) = (a.half(), b.half());
    let reach = |axes: (V2, V2), h: V2, n: V2| h.0 * dot(axes.0, n).abs() + h.1 * dot(axes.1, n).abs();
    let mut best = [(f32::MAX, 0usize); 2];
    for (owner, axes) in [aa, ab].into_iter().enumerate() {
        for (i, n) in [axes.0, axes.1].into_iter().enumerate() {
            let pen = reach(aa, ha, n) + reach(ab, hb, n) - dot(d, n).abs();
            if pen <= 0.0 {
                return None;
            }
            if pen < best[owner].0 {
                best[owner] = (pen, i);
            }
        }
    }
    let ref_is_b = only_b_axes || best[1].0 < 0.95 * best[0].0 - 0.01;
    let (rn, inc, axes_r, axes_i, half_r) = if ref_is_b { (&b, &a, ab, aa, hb) } else { (&a, &b, aa, ab, ha) };
    let pick = |axes: (V2, V2), i: usize| if i == 0 { axes.0 } else { axes.1 };
    let i = best[ref_is_b as usize].1;
    let axis = pick(axes_r, i);
    let toward_inc = if ref_is_b { (-d.0, -d.1) } else { d };
    // Outward normal of the reference face, pointing at the incident box.
    let nr = if dot(axis, toward_inc) < 0.0 { (-axis.0, -axis.1) } else { axis };
    let normal = if ref_is_b { (-nr.0, -nr.1) } else { nr };
    let (face_half, side_half) = if i == 0 { (half_r.0, half_r.1) } else { (half_r.1, half_r.0) };
    let (cr, ci) = (rn.center(), inc.center());
    let t = (-nr.1, nr.0);

    let faces = [(axes_i.0, 0), (axes_i.1, 1)];
    let (fi, k) = faces
        .into_iter()
        .flat_map(|(n, k)| [(n, k), ((-n.0, -n.1), k)])
        .min_by(|x, y| dot(x.0, nr).total_cmp(&dot(y.0, nr)))
        .unwrap();
    let half_i = inc.half();
    let (along, across) = if k == 0 { (half_i.0, half_i.1) } else { (half_i.1, half_i.0) };
    let edge = (-fi.1, fi.0);
    let mid = (ci.0 + fi.0 * along, ci.1 + fi.1 * along);
    let mut seg =
        [(mid.0 + edge.0 * across, mid.1 + edge.1 * across), (mid.0 - edge.0 * across, mid.1 - edge.1 * across)];
    for sign in [1.0, -1.0] {
        let dist = |p: V2| side_half - sign * dot(sub(p, cr), t);
        let (d0, d1) = (dist(seg[0]), dist(seg[1]));
        if d0 < 0.0 && d1 < 0.0 {
            return None;
        }
        if d0 < 0.0 || d1 < 0.0 {
            let (keep, cut, dk, dc) = if d0 < 0.0 { (1, 0, d1, d0) } else { (0, 1, d0, d1) };
            let f = dk / (dk - dc);
            let (pk, pc) = (seg[keep], seg[cut]);
            seg[cut] = (pk.0 + (pc.0 - pk.0) * f, pk.1 + (pc.1 - pk.1) * f);
        }
    }
    let mut pts: Pts = seg.into_iter().map(|p| Pt::new(p, face_half - dot(sub(p, cr), nr))).collect();
    pts.retain(|p| p.depth > 0.0);
    (!pts.is_empty()).then_some(Touch { normal, pts, whole_face: false })
}

// Normal points from the box to the circle; works in the box's own frame so a tilted box needs no special case.
fn circle_box(circle: &Node, boxed: &Node) -> Option<Touch> {
    let (cc, bc, r) = (circle.center(), boxed.center(), circle.radius());
    let (ux, uy) = boxed.axes();
    let w = sub(cc, bc);
    let (lx, ly) = (dot(w, ux), dot(w, uy));
    let (hw, hh) = boxed.half();
    let (qx, qy) = (lx.clamp(-hw, hw), ly.clamp(-hh, hh));
    let (dx, dy) = (lx - qx, ly - qy);
    let d2 = dx * dx + dy * dy;
    let (n, depth, q) = if d2 > 0.0 {
        if d2 >= r * r {
            return None;
        }
        let dist = d2.sqrt();
        ((dx / dist, dy / dist), r - dist, (qx, qy))
    } else {
        // A centre that ended up inside came from outside: leave by the side it was on before the step.
        let ((px, py), was) = (circle.before_center(), boxed.before);
        let flat = boxed.angle == 0.0;
        let exits = [
            (lx + hw, (-1.0, 0.0), (-hw, ly), flat && px < was.x),
            (hw - lx, (1.0, 0.0), (hw, ly), flat && px > was.x + was.w),
            (ly + hh, (0.0, -1.0), (lx, -hh), flat && py < was.y),
            (hh - ly, (0.0, 1.0), (lx, hh), flat && py > was.y + was.h),
        ];
        let nearest = exits.iter().min_by(|x, y| x.0.total_cmp(&y.0)).unwrap();
        let &(inside, n, q, _) = exits.iter().find(|e| e.3).unwrap_or(nearest);
        (n, r + inside, q)
    };
    let world = |v: V2| (v.0 * ux.0 + v.1 * uy.0, v.0 * ux.1 + v.1 * uy.1);
    let (nw, qw) = (world(n), world(q));
    Some(Touch { normal: nw, pts: Pts::one(Pt::new((bc.0 + qw.0, bc.1 + qw.1), depth)), whole_face: false })
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

// A convex polygon of up to four corners.
struct Poly {
    v: [V2; 4],
    n: usize,
}

impl Poly {
    fn of_box(node: &Node) -> Poly {
        let (c, (ux, uy), (hw, hh)) = (node.center(), node.axes(), node.half());
        let corner = |sx: f32, sy: f32| (c.0 + ux.0 * hw * sx + uy.0 * hh * sy, c.1 + ux.1 * hw * sx + uy.1 * hh * sy);
        Poly { v: [corner(-1.0, -1.0), corner(1.0, -1.0), corner(1.0, 1.0), corner(-1.0, 1.0)], n: 4 }
    }

    fn of_triangle(t: [V2; 3]) -> Poly {
        Poly { v: [t[0], t[1], t[2], t[2]], n: 3 }
    }

    fn corners(&self) -> &[V2] {
        &self.v[..self.n]
    }

    // The unit normal of edge `i` (corner i to the next), pointing out of the polygon.
    fn normal(&self, i: usize) -> V2 {
        let (p, q) = (self.v[i], self.v[(i + 1) % self.n]);
        let (ex, ey) = sub(q, p);
        let len = (ex * ex + ey * ey).sqrt().max(1e-9);
        let n = (ey / len, -ex / len);
        let centre = self.corners().iter().fold((0.0, 0.0), |s, c| (s.0 + c.0, s.1 + c.1));
        let inside = sub((centre.0 / self.n as f32, centre.1 / self.n as f32), p);
        if dot(n, inside) > 0.0 { (-n.0, -n.1) } else { n }
    }

    // How far the other polygon is past the deepest edge of this one: the least overlap, and the edge that gives it.
    fn gap(&self, other: &Poly) -> (f32, usize) {
        let mut best = (f32::MIN, 0);
        for i in 0..self.n {
            let (n, p) = (self.normal(i), self.v[i]);
            let apart = other.corners().iter().map(|&c| dot(sub(c, p), n)).fold(f32::MAX, f32::min);
            if apart > best.0 {
                best = (apart, i);
            }
        }
        best
    }
}

// A box or circle against the triangle of a slope tile (the normal points from `a` to the tile `b`).
fn slope_touch(a: &Node, triangle: [V2; 3]) -> Option<Touch> {
    let tri = Poly::of_triangle(triangle);
    if a.circle {
        return circle_triangle(a, &tri);
    }
    let boxed = Poly::of_box(a);
    let ((gap_a, ia), (gap_b, ib)) = (boxed.gap(&tri), tri.gap(&boxed));
    if gap_a >= 0.0 || gap_b >= 0.0 {
        return None;
    }
    // The tile's own face is the reference unless the box's face is clearly the shallower way out.
    let ref_is_b = -gap_b < 0.95 * -gap_a - 0.01;
    let (r, i, edge) = if ref_is_b { (&tri, &boxed, ib) } else { (&boxed, &tri, ia) };
    let nr = r.normal(edge);
    let (r0, r1) = (r.v[edge], r.v[(edge + 1) % r.n]);
    let incident = (0..i.n).min_by(|&x, &y| dot(i.normal(x), nr).total_cmp(&dot(i.normal(y), nr))).unwrap();
    let mut seg = [i.v[incident], i.v[(incident + 1) % i.n]];
    let (tx, ty) = (r1.0 - r0.0, r1.1 - r0.1);
    let len = (tx * tx + ty * ty).sqrt().max(1e-9);
    let t = (tx / len, ty / len);
    for (from, sign) in [(r0, 1.0), (r1, -1.0)] {
        let inside = |p: V2| sign * dot(sub(p, from), t);
        let (d0, d1) = (inside(seg[0]), inside(seg[1]));
        if d0 < 0.0 && d1 < 0.0 {
            return None;
        }
        if d0 < 0.0 || d1 < 0.0 {
            let (keep, cut, dk, dc) = if d0 < 0.0 { (1, 0, d1, d0) } else { (0, 1, d0, d1) };
            let f = dk / (dk - dc);
            let (pk, pc) = (seg[keep], seg[cut]);
            seg[cut] = (pk.0 + (pc.0 - pk.0) * f, pk.1 + (pc.1 - pk.1) * f);
        }
    }
    let normal = if ref_is_b { (-nr.0, -nr.1) } else { nr };
    let mut pts: Pts = seg.into_iter().map(|p| Pt::new(p, -dot(sub(p, r0), nr))).collect();
    pts.retain(|p| p.depth > 0.0);
    (!pts.is_empty()).then_some(Touch { normal, pts, whole_face: false })
}

// The closest point of the triangle to the circle's centre decides; a centre inside leaves by the nearest edge.
fn circle_triangle(circle: &Node, tri: &Poly) -> Option<Touch> {
    let (c, r) = (circle.center(), circle.radius());
    let mut nearest = (f32::MAX, (0.0, 0.0));
    for i in 0..tri.n {
        let (p, q) = (tri.v[i], tri.v[(i + 1) % tri.n]);
        let (ex, ey) = sub(q, p);
        let f = (dot(sub(c, p), (ex, ey)) / (ex * ex + ey * ey).max(1e-9)).clamp(0.0, 1.0);
        let at = (p.0 + ex * f, p.1 + ey * f);
        let (dx, dy) = sub(c, at);
        let d2 = dx * dx + dy * dy;
        if d2 < nearest.0 {
            nearest = (d2, at);
        }
    }
    let (d2, at) = nearest;
    let inside = (0..tri.n).all(|i| dot(sub(c, tri.v[i]), tri.normal(i)) <= 0.0);
    if inside {
        let (depth_to_edge, edge) =
            (0..tri.n).map(|i| (-dot(sub(c, tri.v[i]), tri.normal(i)), i)).min_by(|x, y| x.0.total_cmp(&y.0)).unwrap();
        let out = tri.normal(edge);
        return Some(Touch {
            normal: (-out.0, -out.1),
            pts: Pts::one(Pt::new(at, r + depth_to_edge)),
            whole_face: false,
        });
    }
    if d2 >= r * r {
        return None;
    }
    let dist = d2.sqrt().max(1e-9);
    let normal = ((at.0 - c.0) / dist, (at.1 - c.1) / dist);
    Some(Touch { normal, pts: Pts::one(Pt::new(at, r - dist)), whole_face: false })
}
