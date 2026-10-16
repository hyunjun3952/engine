use super::{Rect, SLOPE, Slide, Slope, TileHit, Tilemap, Vec2};

impl Tilemap {
    // `landing` is the bottom edge a falling body had before the move: a one-way tile blocks only from there.
    pub(super) fn blocked(&self, r: &Rect, landing: Option<f32>) -> bool {
        let s = self.size as f32;
        let (x0, x1) = ((r.x / s).floor() as i32, ((r.x + r.w - 0.001) / s).floor() as i32);
        let (y0, y1) = ((r.y / s).floor() as i32, ((r.y + r.h - 0.001) / s).floor() as i32);
        (y0..=y1).any(|ty| {
            (x0..=x1).any(|tx| {
                self.solid(tx, ty)
                    && self.slope_at(tx, ty).is_none()
                    && (!self.one_way_at(tx, ty) || landing.is_some_and(|b| b <= ty as f32 * s + 0.001))
            })
        })
    }

    /// Moves `r` by `dx`, then `dy`, stopping at solid tiles: the moved box and whether x and y were blocked. Slope
    /// tiles are climbed (a step up as long as the step sideways) and followed down when the box stood on one; a
    /// box that stands on a slope or lands on one reports a blocked y. A move longer than half a tile is done in
    /// pieces of at most that, so a fast box cannot skip a thin wall.
    pub fn slide(&self, r: Rect, dx: f32, dy: f32) -> Slide {
        let half = self.size as f32 / 2.0;
        let far = dx.abs().max(dy.abs());
        if !(far > half && far.is_finite()) {
            return self.slide_once(r, dx, dy);
        }
        let (px, py) = (dx / far * half, dy / far * half);
        let (mut rest, mut out) = ((dx, dy), Slide { rect: r, blocked_x: false, blocked_y: false });
        let walk = |out: &mut Slide, (mx, my): (f32, f32)| {
            let s = self.slide_once(out.rect, mx, my);
            (out.rect, out.blocked_x, out.blocked_y) =
                (s.rect, out.blocked_x | s.blocked_x, out.blocked_y | s.blocked_y);
        };
        for _ in 0..4096 {
            if rest.0.abs().max(rest.1.abs()) <= half {
                break;
            }
            walk(&mut out, (px, py));
            rest = (rest.0 - px, rest.1 - py);
        }
        walk(&mut out, rest);
        out
    }

    pub(super) fn slide_once(&self, r: Rect, dx: f32, dy: f32) -> Slide {
        let on_slope = dx != 0.0 && dy >= 0.0 && self.resting_on_slope(&r);
        let (mut r, hit_x, lifted) = self.step_x(r, dx);
        let mut grounded = lifted && dy >= 0.0;
        if on_slope && !hit_x && !lifted {
            let (down, landed) = self.step_y(r, dx.abs() + 1.0);
            if landed {
                (r, grounded) = (down, true);
            }
        }
        let (rect, hit_y) = self.step_y(r, dy);
        Slide { rect, blocked_x: hit_x, blocked_y: hit_y || grounded }
    }

    pub(super) fn step_x(&self, mut r: Rect, dx: f32) -> (Rect, bool, bool) {
        let (s, start) = (self.size as f32, r);
        r.x += dx;
        let mut lifted = false;
        for (slope, t) in self.slope_cells(&r) {
            let sink = slope.sink(&t, &r);
            if sink <= 0.0 {
                continue;
            }
            if sink <= dx.abs() + 0.01 {
                r.y += if slope.floor() { -sink } else { sink };
                lifted |= slope.floor();
            } else {
                r.x = if dx > 0.0 { t.x - r.w } else { t.x + t.w };
                return (r, true, lifted);
            }
        }
        if dx != 0.0 && self.blocked(&r, None) {
            r = Rect { x: start.x + dx, ..start };
            r.x = if dx > 0.0 { ((r.x + r.w - 0.001) / s).floor() * s - r.w } else { ((r.x / s).floor() + 1.0) * s };
            return (r, true, false);
        }
        (r, false, lifted)
    }

    pub(super) fn step_y(&self, mut r: Rect, dy: f32) -> (Rect, bool) {
        let (s, start) = (self.size as f32, r);
        r.y += dy;
        if dy == 0.0 {
            return (r, false);
        }
        let mut hit = false;
        for (slope, t) in self.slope_cells(&r) {
            let sink = slope.sink(&t, &r);
            if sink <= 0.0 {
                continue;
            }
            hit = true;
            r.y = match (slope.floor(), dy > 0.0) {
                (true, false) if start.y >= t.y + t.h - 0.001 => t.y + t.h,
                (true, _) => r.y - sink,
                (false, true) if start.y + start.h <= t.y + 0.001 => t.y - r.h,
                (false, _) => r.y + sink,
            };
        }
        if self.blocked(&r, (dy > 0.0).then_some(start.y + start.h)) {
            r = Rect { y: start.y + dy, ..start };
            r.y = if dy > 0.0 { ((r.y + r.h - 0.001) / s).floor() * s - r.h } else { ((r.y / s).floor() + 1.0) * s };
            return (r, true);
        }
        (r, hit)
    }

    // The slope tiles the box overlaps the cell of, with their rectangles.
    pub(super) fn slope_cells(&self, r: &Rect) -> Vec<(Slope, Rect)> {
        let s = self.size as f32;
        let mut out = Vec::new();
        if !self.flags.iter().any(|f| f & SLOPE != 0) {
            return out;
        }
        let (x0, x1) = ((r.x / s).floor() as i32, ((r.x + r.w - 0.001) / s).floor() as i32);
        let (y0, y1) = ((r.y / s).floor() as i32, ((r.y + r.h - 0.001) / s).floor() as i32);
        for ty in y0..=y1 {
            for tx in x0..=x1 {
                if let Some(slope) = self.slope_at(tx, ty) {
                    out.push((slope, Rect::new(tx as f32 * s, ty as f32 * s, s, s)));
                }
            }
        }
        out
    }

    // Whether the box stands on the diagonal of a floor slope (within a pixel of it).
    pub(super) fn resting_on_slope(&self, r: &Rect) -> bool {
        let lower = Rect { y: r.y + 0.6, ..*r };
        self.slope_cells(&lower).iter().any(|(slope, t)| slope.floor() && slope.sink(t, &lower) > 0.0)
    }

    /// The first solid tile the ray `from + t * dir`, `0 <= t <= max`, enters, seen the way bodies see the map (its
    /// left and right edges count as solid; a ray may leave it above or below). `dir` need not be a unit vector.
    pub fn raycast(&self, from: impl Into<Vec2>, dir: impl Into<Vec2>, max: f32) -> Option<TileHit> {
        let (from, dir): (Vec2, Vec2) = (from.into(), dir.into());
        let finite = from.x.is_finite() && from.y.is_finite() && dir.x.is_finite() && dir.y.is_finite();
        if !finite || max.is_nan() || max < 0.0 || dir == Vec2::ZERO {
            return None;
        }
        let (size, o) = (self.size as f32, from / self.size as f32);
        let (mut tx, mut ty) = (o.x.floor() as i32, o.y.floor() as i32);
        if self.solid(tx, ty) {
            return Some(TileHit { t: 0.0, tile: (tx, ty), normal: Vec2::ZERO });
        }
        // Per axis: the step to the next cell, the t of the next cell border and the t between two borders.
        let axis = |o: f32, d: f32, c: i32| match d {
            d if d > 0.0 => (1, ((c + 1) as f32 - o) * size / d, size / d),
            d if d < 0.0 => (-1, (o - c as f32) * size / -d, size / -d),
            _ => (0, f32::INFINITY, f32::INFINITY),
        };
        let ((sx, mut next_x, step_x), (sy, mut next_y, step_y)) = (axis(o.x, dir.x, tx), axis(o.y, dir.y, ty));
        loop {
            let along_x = next_x < next_y;
            let t = if along_x { next_x } else { next_y };
            if t > max {
                return None;
            }
            let normal = if along_x {
                (tx, next_x) = (tx + sx, next_x + step_x);
                Vec2::new(-sx as f32, 0.0)
            } else {
                (ty, next_y) = (ty + sy, next_y + step_y);
                Vec2::new(0.0, -sy as f32)
            };
            if self.solid(tx, ty) {
                return Some(TileHit { t, tile: (tx, ty), normal });
            }
            // Past the top or bottom and still going that way, with no side wall left to run into.
            if sx == 0 && ((ty < 0 && sy < 0) || (ty >= self.h as i32 && sy > 0)) {
                return None;
            }
        }
    }

    /// The solid tiles that touch `area`, as rectangles.
    pub fn solid_in(&self, area: Rect) -> Vec<Rect> {
        let s = self.size as f32;
        let (x0, x1) = ((area.x / s).floor() as i32, ((area.x + area.w) / s).ceil() as i32);
        let (y0, y1) = ((area.y / s).floor() as i32, ((area.y + area.h) / s).ceil() as i32);
        let mut out = Vec::new();
        for ty in y0..y1 {
            for tx in x0..x1 {
                if self.solid(tx, ty) {
                    out.push(Rect::new(tx as f32 * s, ty as f32 * s, s, s));
                }
            }
        }
        out
    }
}
