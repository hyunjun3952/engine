use super::{Rect, SLOPE, SLOPE_LEFT, SLOPE_TOP, Sprite, Vec2};

/// A tile cut along a diagonal into a solid right triangle, named by the corner that holds the right angle:
/// `BottomRight` is a floor rising to the right (`/`), `BottomLeft` one rising to the left (`\`), and the top
/// corners are ceilings. Physics and `slide` see the exact triangle; rays and paths still treat the tile as a full block.
///
/// ```
/// use engine::{Slope, Tile};
///
/// let ramp = Tile { slope: Some(Slope::BottomRight), ..Tile::DEFAULT[0] };
/// assert!(ramp.slope.is_some());
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slope {
    /// The right angle is at the bottom left.
    BottomLeft,
    /// The right angle is at the bottom right.
    BottomRight,
    /// The right angle is at the top left.
    TopLeft,
    /// The right angle is at the top right.
    TopRight,
}

impl Slope {
    pub(super) fn bits(self) -> u8 {
        SLOPE
            | if matches!(self, Slope::BottomLeft | Slope::TopLeft) { SLOPE_LEFT } else { 0 }
            | if matches!(self, Slope::TopLeft | Slope::TopRight) { SLOPE_TOP } else { 0 }
    }

    pub(super) fn from_bits(flags: u8) -> Option<Slope> {
        (flags & SLOPE != 0).then_some(match (flags & SLOPE_LEFT != 0, flags & SLOPE_TOP != 0) {
            (true, false) => Slope::BottomLeft,
            (false, false) => Slope::BottomRight,
            (true, true) => Slope::TopLeft,
            (false, true) => Slope::TopRight,
        })
    }

    pub(super) fn floor(self) -> bool {
        matches!(self, Slope::BottomLeft | Slope::BottomRight)
    }

    // The side of the tile that holds the right angle, which is where a box meets the diagonal first.
    pub(super) fn at_right(self) -> bool {
        matches!(self, Slope::BottomRight | Slope::TopRight)
    }

    // The height of the diagonal above `x` in the tile `t`.
    pub(super) fn surface(self, t: &Rect, x: f32) -> f32 {
        if matches!(self, Slope::BottomRight | Slope::TopLeft) { t.y + t.h - (x - t.x) } else { t.y + (x - t.x) }
    }

    // How far (vertically) the box `r` reaches into the solid triangle of the tile `t`; zero or less when it does not.
    pub(super) fn sink(self, t: &Rect, r: &Rect) -> f32 {
        const E: f32 = 0.001;
        if r.x + r.w <= t.x + E || r.x >= t.x + t.w - E || r.y + r.h <= t.y + E || r.y >= t.y + t.h - E {
            return 0.0;
        }
        let x = if self.at_right() { (r.x + r.w).min(t.x + t.w) } else { r.x.max(t.x) };
        let line = self.surface(t, x);
        if self.floor() { r.y + r.h - line } else { line - r.y }
    }

    /// The triangle inside the tile `r`: the right-angle corner first, then the two others.
    pub(crate) fn triangle(self, r: &Rect) -> [(f32, f32); 3] {
        let left = matches!(self, Slope::BottomLeft | Slope::TopLeft);
        let top = matches!(self, Slope::TopLeft | Slope::TopRight);
        let (cx, ax) = if left { (r.x, r.x + r.w) } else { (r.x + r.w, r.x) };
        let (cy, ay) = if top { (r.y, r.y + r.h) } else { (r.y + r.h, r.y) };
        [(cx, cy), (ax, cy), (cx, ay)]
    }
}

/// What a level-file character means: sheet tile id (from 1), solid or not, drawn over entities or behind.
/// A `one_way` tile is solid only from above: bodies pass it sideways and from below, and land on its top.
/// A `slope` tile is a solid triangle for physics.
/// With `auto`, `id` starts 16 sheet tiles chosen by joined neighbours (up 1, right 2, down 4, left 8).
///
/// ```
/// use engine::Tile;
///
/// let spikes = Tile { ch: b'^', id: 2, solid: false, front: true, auto: false, one_way: false, slope: None };
/// assert!(Tile::DEFAULT[0].solid && !spikes.solid);
/// ```
#[derive(Clone, Copy, Debug)]
pub struct Tile {
    /// The character that stands for the tile in a level file.
    pub ch: u8,
    /// The sheet tile it shows, counted from 1.
    pub id: u8,
    /// Blocks bodies.
    pub solid: bool,
    /// Drawn over entities instead of behind them.
    pub front: bool,
    /// `id` starts 16 tiles picked by joined neighbours.
    pub auto: bool,
    /// Solid only from above.
    pub one_way: bool,
    /// A right triangle instead of a full block.
    pub slope: Option<Slope>,
}

impl Tile {
    /// `#` ground and `+` brick, both solid and drawn behind entities: what `Tilemap::load` uses.
    pub const DEFAULT: [Tile; 2] = [
        Tile { ch: b'#', id: 1, solid: true, front: false, auto: false, one_way: false, slope: None },
        Tile { ch: b'+', id: 2, solid: true, front: false, auto: false, one_way: false, slope: None },
    ];
}

// How many `size` pixel squares the sheet holds: ids run along each row in turn (a sheet shorter than one tile counts
// as a single row).
pub(crate) fn tile_count(sheet: &Sprite, size: usize) -> usize {
    let (w, h) = sheet.size();
    (w / size) * (h / size).max(1)
}

/// Where a ray entered a solid tile: the distance factor `t` (the point is `from + t * dir`), the tile, and the
/// direction the face it crossed looks (against the ray; zero when the ray began inside the tile).
///
/// ```
/// use engine::{Rect, Sprite, Tilemap};
/// use std::sync::Arc;
///
/// let sheet = Arc::new(Sprite::new(16, 8, vec![0; 128]));
/// let map = Tilemap::from_ascii(&["P...", "####"], 8, sheet);
/// let hit = map.raycast((4.0, 0.0), (0.0, 1.0), 100.0).unwrap();
/// assert_eq!(hit.tile, (0, 1));
/// assert_eq!(hit.t, 8.0);
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TileHit {
    /// Distance along the ray, in lengths of `dir`.
    pub t: f32,
    /// The tile that was hit.
    pub tile: (i32, i32),
    /// The direction the crossed face looks.
    pub normal: Vec2,
}

/// Where `Tilemap::slide` left a box, and whether tiles stopped it along each axis.
///
/// ```
/// use engine::{Rect, Sprite, Tilemap};
/// use std::sync::Arc;
///
/// let sheet = Arc::new(Sprite::new(16, 8, vec![0; 128]));
/// let map = Tilemap::from_ascii(&["P...", "####"], 8, sheet);
/// let landed = map.slide(Rect::new(4.0, 0.0, 6.0, 6.0), 0.0, 4.0);
/// assert!(landed.blocked_y);
/// assert_eq!(landed.rect.y, 2.0);
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slide {
    /// Where the box ended up.
    pub rect: Rect,
    /// Something stopped it sideways.
    pub blocked_x: bool,
    /// Something stopped it vertically.
    pub blocked_y: bool,
}
