use std::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Neg, Sub, SubAssign};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub const ZERO: Vec2 = Vec2 { x: 0.0, y: 0.0 };

    pub const fn new(x: f32, y: f32) -> Self {
        Vec2 { x, y }
    }

    /// The unit vector at `angle` radians, clockwise on screen like `Transform::angle`.
    pub fn from_angle(angle: f32) -> Self {
        let (s, c) = angle.sin_cos();
        Vec2 { x: c, y: s }
    }

    /// The angle `from_angle` would make it from, in radians (0 along +x, clockwise on screen).
    pub fn angle(self) -> f32 {
        self.y.atan2(self.x)
    }

    /// The same vector, shortened to `max` if it is longer (a speed limit).
    pub fn clamp_length(self, max: f32) -> Vec2 {
        let len = self.length();
        if len > max { self * (max.max(0.0) / len) } else { self }
    }

    pub fn min(self, o: Vec2) -> Vec2 {
        Vec2 { x: self.x.min(o.x), y: self.y.min(o.y) }
    }

    pub fn max(self, o: Vec2) -> Vec2 {
        Vec2 { x: self.x.max(o.x), y: self.y.max(o.y) }
    }

    pub fn abs(self) -> Vec2 {
        Vec2 { x: self.x.abs(), y: self.y.abs() }
    }

    pub fn dot(self, o: Vec2) -> f32 {
        self.x * o.x + self.y * o.y
    }

    /// The z of the 3D cross product: positive when `o` is clockwise of `self` on screen.
    pub fn cross(self, o: Vec2) -> f32 {
        self.x * o.y - self.y * o.x
    }

    pub fn length_sq(self) -> f32 {
        self.dot(self)
    }

    pub fn length(self) -> f32 {
        self.x.hypot(self.y)
    }

    pub fn distance(self, o: Vec2) -> f32 {
        (self - o).length()
    }

    /// The same direction with length 1; the zero vector stays zero.
    pub fn normalized(self) -> Vec2 {
        let len = self.length();
        if len > 0.0 { self / len } else { Vec2::ZERO }
    }

    /// Turned a quarter turn clockwise on screen.
    pub fn perp(self) -> Vec2 {
        Vec2 { x: -self.y, y: self.x }
    }

    pub fn lerp(self, o: Vec2, t: f32) -> Vec2 {
        self + (o - self) * t
    }

    /// Turned by `angle` radians, clockwise on screen.
    pub fn rotated(self, angle: f32) -> Vec2 {
        let (s, c) = angle.sin_cos();
        Vec2 { x: self.x * c - self.y * s, y: self.x * s + self.y * c }
    }
}

impl Add for Vec2 {
    type Output = Vec2;
    fn add(self, o: Vec2) -> Vec2 {
        Vec2 { x: self.x + o.x, y: self.y + o.y }
    }
}

impl Sub for Vec2 {
    type Output = Vec2;
    fn sub(self, o: Vec2) -> Vec2 {
        Vec2 { x: self.x - o.x, y: self.y - o.y }
    }
}

impl Neg for Vec2 {
    type Output = Vec2;
    fn neg(self) -> Vec2 {
        Vec2 { x: -self.x, y: -self.y }
    }
}

impl Mul<f32> for Vec2 {
    type Output = Vec2;
    fn mul(self, k: f32) -> Vec2 {
        Vec2 { x: self.x * k, y: self.y * k }
    }
}

impl Mul<Vec2> for f32 {
    type Output = Vec2;
    fn mul(self, v: Vec2) -> Vec2 {
        v * self
    }
}

impl Div<f32> for Vec2 {
    type Output = Vec2;
    fn div(self, k: f32) -> Vec2 {
        Vec2 { x: self.x / k, y: self.y / k }
    }
}

impl AddAssign for Vec2 {
    fn add_assign(&mut self, o: Vec2) {
        *self = *self + o;
    }
}

impl SubAssign for Vec2 {
    fn sub_assign(&mut self, o: Vec2) {
        *self = *self - o;
    }
}

impl MulAssign<f32> for Vec2 {
    fn mul_assign(&mut self, k: f32) {
        *self = *self * k;
    }
}

impl DivAssign<f32> for Vec2 {
    fn div_assign(&mut self, k: f32) {
        *self = *self / k;
    }
}

impl std::iter::Sum for Vec2 {
    fn sum<I: Iterator<Item = Vec2>>(it: I) -> Vec2 {
        it.fold(Vec2::ZERO, Add::add)
    }
}

impl From<(f32, f32)> for Vec2 {
    fn from((x, y): (f32, f32)) -> Self {
        Vec2 { x, y }
    }
}

impl From<Vec2> for (f32, f32) {
    fn from(v: Vec2) -> Self {
        (v.x, v.y)
    }
}

impl From<[f32; 2]> for Vec2 {
    fn from([x, y]: [f32; 2]) -> Self {
        Vec2 { x, y }
    }
}

impl From<Vec2> for [f32; 2] {
    fn from(v: Vec2) -> Self {
        [v.x, v.y]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_and_products_follow_the_screen_convention() {
        let (a, b) = (Vec2::new(3.0, 4.0), Vec2::new(1.0, 2.0));
        assert_eq!(
            (a + b, a - b, -a, a * 2.0, 2.0 * a, a / 2.0),
            (
                Vec2::new(4.0, 6.0),
                Vec2::new(2.0, 2.0),
                Vec2::new(-3.0, -4.0),
                Vec2::new(6.0, 8.0),
                Vec2::new(6.0, 8.0),
                Vec2::new(1.5, 2.0)
            )
        );
        assert_eq!((a.dot(b), a.cross(b), a.length(), a.length_sq()), (11.0, 2.0, 5.0, 25.0));
        assert_eq!(Vec2::new(1.0, 0.0).cross(Vec2::new(0.0, 1.0)), 1.0, "down is clockwise of right on screen");
        assert_eq!(Vec2::new(1.0, 0.0).perp(), Vec2::new(0.0, 1.0));
        let mut c = a;
        c += b;
        c -= a;
        c *= 3.0;
        assert_eq!(c, Vec2::new(3.0, 6.0));
    }

    #[test]
    fn normalizing_rotating_and_mixing() {
        assert_eq!(Vec2::ZERO.normalized(), Vec2::ZERO);
        assert!((Vec2::new(3.0, 4.0).normalized().length() - 1.0).abs() < 1e-6);
        let turned = Vec2::new(1.0, 0.0).rotated(std::f32::consts::FRAC_PI_2);
        assert!(turned.distance(Vec2::new(0.0, 1.0)) < 1e-6);
        assert!(Vec2::from_angle(0.7).distance(Vec2::new(1.0, 0.0).rotated(0.7)) < 1e-6);
        assert_eq!(Vec2::new(0.0, 0.0).lerp(Vec2::new(10.0, -10.0), 0.25), Vec2::new(2.5, -2.5));
        assert!((Vec2::from_angle(-2.0).angle() + 2.0).abs() < 1e-6);
        assert_eq!(Vec2::new(30.0, 40.0).clamp_length(5.0), Vec2::new(3.0, 4.0));
        assert_eq!(Vec2::new(3.0, 4.0).clamp_length(9.0), Vec2::new(3.0, 4.0));
        assert_eq!(Vec2::new(3.0, 4.0).clamp_length(-1.0), Vec2::ZERO);
    }

    #[test]
    fn per_component_helpers_and_sums() {
        let (a, b) = (Vec2::new(1.0, -5.0), Vec2::new(-2.0, 3.0));
        assert_eq!((a.min(b), a.max(b), a.abs()), (Vec2::new(-2.0, -5.0), Vec2::new(1.0, 3.0), Vec2::new(1.0, 5.0)));
        assert_eq!([a, b, a].into_iter().sum::<Vec2>(), Vec2::new(0.0, -7.0));
        let mut c = Vec2::new(4.0, 2.0);
        c /= 2.0;
        assert_eq!(c, Vec2::new(2.0, 1.0));
    }

    #[test]
    fn it_converts_to_and_from_tuples_and_arrays() {
        let v: Vec2 = (1.0, 2.0).into();
        let t: (f32, f32) = v.into();
        let a: [f32; 2] = v.into();
        assert_eq!((t, a, Vec2::from([1.0, 2.0])), ((1.0, 2.0), [1.0, 2.0], v));
    }
}
