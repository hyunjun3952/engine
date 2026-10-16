use super::{
    CORRECTION, Hit, Node, Pt, Pts, REST_SPEED, SAME_NORMAL, SLOP, V2, WARM_RANGE, apply, arms, dot, rel_vel, shove,
    stiffness, sub,
};

// A contact's normal and, per point, where it was and the impulses it ended with.
pub(super) struct Memory {
    pub(super) normal: V2,
    pub(super) pts: Pts,
}

// How fast the bodies close along the normal when contact starts decides the bounce, once, before any impulse.
pub(super) fn prepare(a: &Node, b: &Node, hit: &mut Hit) {
    let bounce = a.restitution.min(b.restitution);
    let t = (-hit.normal.1, hit.normal.0);
    for pt in hit.pts.iter_mut() {
        let (ra, rb) = arms(a, b, pt.at);
        (pt.ra, pt.rb) = (ra, rb);
        (pt.kn, pt.kt) = (stiffness(a, b, ra, rb, hit.normal), stiffness(a, b, ra, rb, t));
        let closing = -dot(rel_vel(a, b, ra, rb), hit.normal);
        pt.bounce = if closing < REST_SPEED { 0.0 } else { bounce * closing };
    }
}

// A point inherits the impulses of the nearest point this pair had last step, if one is still close.
pub(super) fn warm_start(a: &mut Node, b: &mut Node, hit: &mut Hit, old: Option<&Memory>) {
    let n = hit.normal;
    let old = old.filter(|o| dot(o.normal, n) > SAME_NORMAL).map(|o| &o.pts[..]);
    let t = (-n.1, n.0);
    for pt in hit.pts.iter_mut() {
        let dist = |o: &&Pt| (o.at.0 - pt.at.0).hypot(o.at.1 - pt.at.1);
        let nearest = old.into_iter().flatten().min_by(|x, y| dist(x).total_cmp(&dist(y)));
        let Some(&Pt { jn, jt, .. }) = nearest.filter(|o| dist(o) < WARM_RANGE) else {
            continue;
        };
        let (ra, rb) = arms(a, b, pt.at);
        apply(a, b, ra, rb, (jn * n.0 + jt * t.0, jn * n.1 + jt * t.1));
        (pt.jn, pt.jt) = (jn, jt);
    }
}

pub(super) fn solve(a: &mut Node, b: &mut Node, hit: &mut Hit) {
    let n = hit.normal;
    let t = (-n.1, n.0);
    let grip = (a.friction * b.friction).sqrt();
    for pt in hit.pts.iter_mut() {
        let (ra, rb) = (pt.ra, pt.rb);
        let lambda = (pt.bounce - dot(rel_vel(a, b, ra, rb), n)) / pt.kn;
        let total = (pt.jn + lambda).max(0.0);
        apply(a, b, ra, rb, ((total - pt.jn) * n.0, (total - pt.jn) * n.1));
        pt.jn = total;

        let lambda = -dot(rel_vel(a, b, ra, rb), t) / pt.kt;
        let total = (pt.jt + lambda).clamp(-grip * pt.jn, grip * pt.jn);
        apply(a, b, ra, rb, ((total - pt.jt) * t.0, (total - pt.jt) * t.1));
        pt.jt = total;
    }
}

// Positional correction only moves bodies, not their angle; the contact's depth shrinks by how far they have
// moved together along the normal since it was found.
pub(super) fn separate(a: &mut Node, b: &mut Node, hit: &Hit) -> f32 {
    let (ca, cb) = (a.center(), b.center());
    let moved = sub(sub(cb, b.at), sub(ca, a.at));
    let closed = dot(moved, hit.normal);
    let depth = hit.pts.iter().map(|p| p.depth).fold(0.0, f32::max) - closed;
    let inv_sum = a.inv_mass + b.inv_mass;
    let excess = (depth - SLOP).max(0.0);
    shove(a, b, hit.normal, excess * CORRECTION / inv_sum);
    excess
}
