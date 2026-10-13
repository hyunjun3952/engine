// Making contacts: where two shapes touch, the normal between them and how deep, for every pair of shapes.

use super::{Node, Pt, Pts, V2, dot, sub};
use crate::Rect;

pub(super) struct Touch {
    pub(super) normal: V2,
    pub(super) pts: Pts,
    // Axis-aligned boxes: the normal alone says which tile face is touched.
    pub(super) whole_face: bool,
}

// Normal points from a to b. Untilted boxes use where they were before the step (see `hit_along_x`);
// everything else only has current positions, so a fast circle or tilted box can still tunnel.
pub(super) fn touch(a: &Node, b: &Node, tile_axes_only: bool) -> Option<Touch> {
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
    let (ox, oy) = a.rect.penetration(&b.rect)?;
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
        let dist = |p: V2| side_half - sign * dot((p.0 - cr.0, p.1 - cr.1), t);
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
    let mut pts: Pts = seg.into_iter().map(|p| Pt::new(p, face_half - dot((p.0 - cr.0, p.1 - cr.1), nr))).collect();
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
