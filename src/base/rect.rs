use super::Vec2;

/// An axis-aligned rectangle in pixels; `x`, `y` is the top-left corner.
///
/// ```
/// use engine::Rect;
///
/// let a = Rect::new(0.0, 0.0, 10.0, 10.0);
/// assert!(a.overlaps(Rect::new(5.0, 5.0, 10.0, 10.0)));
/// assert!(a.contains((3.0, 3.0)));
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width.
    pub w: f32,
    /// Height.
    pub h: f32,
}

impl Rect {
    /// A rectangle from its top-left corner and size.
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Rect { x, y, w, h }
    }

    /// The middle point.
    pub fn center(&self) -> Vec2 {
        Vec2::new(self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    /// How far `o` overlaps this rectangle on each axis, or `None` if they do not touch.
    pub fn penetration(&self, o: Rect) -> Option<Vec2> {
        let ox = (self.x + self.w).min(o.x + o.w) - self.x.max(o.x);
        let oy = (self.y + self.h).min(o.y + o.h) - self.y.max(o.y);
        (ox > 0.0 && oy > 0.0).then_some(Vec2::new(ox, oy))
    }

    /// True if the rectangles share some area.
    pub fn overlaps(&self, o: Rect) -> bool {
        self.penetration(o).is_some()
    }

    /// True if the point is inside.
    pub fn contains(&self, p: impl Into<Vec2>) -> bool {
        let p = p.into();
        p.x >= self.x && p.x < self.x + self.w && p.y >= self.y && p.y < self.y + self.h
    }

    /// The one-pixel top, bottom, left and right borders, as an outline is drawn.
    pub fn edges(&self) -> [Rect; 4] {
        [
            Rect { h: 1.0, ..*self },
            Rect { y: self.y + self.h - 1.0, h: 1.0, ..*self },
            Rect { w: 1.0, ..*self },
            Rect { x: self.x + self.w - 1.0, w: 1.0, ..*self },
        ]
    }
}
