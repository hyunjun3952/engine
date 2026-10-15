use super::path::astar;
use crate::{DrawCmd, Paint, Rect, Sprite, Vec2};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

static STAMPS: AtomicU64 = AtomicU64::new(1);
const SOLID: u8 = 1;
const FRONT: u8 = 2;
const AUTO: u8 = 4;
const ONE_WAY: u8 = 8;
const SLOPE: u8 = 16;
const SLOPE_LEFT: u8 = 32;
const SLOPE_TOP: u8 = 64;
// Above entities (the debug outlines are on `i32::MAX`); tiles behind them sit at `i32::MIN`.
const FRONT_LAYER: i32 = i32::MAX - 1;

/// A tile cut along a diagonal into a solid right triangle, named by the corner that holds the right angle:
/// `BottomRight` is a floor rising to the right (`/`), `BottomLeft` one rising to the left (`\`), and the top
/// corners are ceilings. Physics and `slide` see the exact triangle; rays and paths still treat the tile as a full block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slope {
    BottomLeft,
    BottomRight,
    TopLeft,
    TopRight,
}

impl Slope {
    fn bits(self) -> u8 {
        SLOPE
            | if matches!(self, Slope::BottomLeft | Slope::TopLeft) { SLOPE_LEFT } else { 0 }
            | if matches!(self, Slope::TopLeft | Slope::TopRight) { SLOPE_TOP } else { 0 }
    }

    fn from_bits(flags: u8) -> Option<Slope> {
        (flags & SLOPE != 0).then_some(match (flags & SLOPE_LEFT != 0, flags & SLOPE_TOP != 0) {
            (true, false) => Slope::BottomLeft,
            (false, false) => Slope::BottomRight,
            (true, true) => Slope::TopLeft,
            (false, true) => Slope::TopRight,
        })
    }

    fn floor(self) -> bool {
        matches!(self, Slope::BottomLeft | Slope::BottomRight)
    }

    // The side of the tile that holds the right angle, which is where a box meets the diagonal first.
    fn at_right(self) -> bool {
        matches!(self, Slope::BottomRight | Slope::TopRight)
    }

    // The height of the diagonal above `x` in the tile `t`.
    fn surface(self, t: &Rect, x: f32) -> f32 {
        if matches!(self, Slope::BottomRight | Slope::TopLeft) { t.y + t.h - (x - t.x) } else { t.y + (x - t.x) }
    }

    // How far (vertically) the box `r` reaches into the solid triangle of the tile `t`; zero or less when it does not.
    fn sink(self, t: &Rect, r: &Rect) -> f32 {
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
#[derive(Clone, Copy, Debug)]
pub struct Tile {
    pub ch: u8,
    pub id: u8,
    pub solid: bool,
    pub front: bool,
    pub auto: bool,
    pub one_way: bool,
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
// as a single row, as it always did).
fn tile_count(sheet: &Sprite, size: usize) -> usize {
    let (w, h) = sheet.size();
    (w / size) * (h / size).max(1)
}

/// Where a ray entered a solid tile: the distance factor `t` (the point is `from + t * dir`), the tile, and the
/// direction the face it crossed looks (against the ray; zero when the ray began inside the tile).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TileHit {
    pub t: f32,
    pub tile: (i32, i32),
    pub normal: Vec2,
}

/// Where `Tilemap::slide` left a box, and whether tiles stopped it along each axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slide {
    pub rect: Rect,
    pub blocked_x: bool,
    pub blocked_y: bool,
}

pub struct Tilemap {
    pub w: usize,
    pub h: usize,
    pub size: usize,
    /// Restitution of every solid tile for physics (0 by default); a body bounces by the smaller of the two.
    pub bounce: f32,
    tiles: Vec<u8>,
    marks: Vec<(u8, usize, usize)>,
    sheet: Arc<Sprite>,
    flags: [u8; 256],
    // A number nobody else has: new for every map and after every `set`, so a reader can tell the tiles changed.
    stamp: u64,
}

impl Tilemap {
    pub fn from_ascii(rows: &[&str], size: usize, sheet: Arc<Sprite>) -> Self {
        Tilemap::try_from_ascii(rows, size, sheet).unwrap_or_else(|e| panic!("{e}"))
    }

    pub fn try_from_ascii(rows: &[&str], size: usize, sheet: Arc<Sprite>) -> Result<Self, String> {
        Tilemap::try_from_ascii_with(rows, size, sheet, &Tile::DEFAULT)
    }

    /// Like `try_from_ascii` with your own characters; `.` and space stay empty and any other character is a marker.
    /// A tile id that no entry names (one given to `set`, say) is solid and drawn behind entities.
    pub fn try_from_ascii_with(
        rows: &[&str],
        size: usize,
        sheet: Arc<Sprite>,
        legend: &[Tile],
    ) -> Result<Self, String> {
        if let Some(t) = legend.iter().find(|t| t.id == 0 || matches!(t.ch, b'.' | b' ')) {
            return Err(format!("legend entry {:?} would hide an empty tile or the id 0", t.ch as char));
        }
        if size == 0 {
            return Err("the tile size must be positive".into());
        }
        let columns = tile_count(&sheet, size);
        if let Some(t) = legend.iter().find(|t| t.id as usize > columns) {
            return Err(format!(
                "legend entry {:?} shows tile {} but the sheet has {columns} tiles",
                t.ch as char, t.id
            ));
        }
        for t in legend.iter().filter(|t| t.auto) {
            // The flag table has 256 entries, so ids past 240 cannot own 16.
            if t.id as usize + 15 > columns.min(255) {
                return Err(format!(
                    "autotile {:?} needs tiles {}..={} but the sheet has {columns}",
                    t.ch as char,
                    t.id,
                    t.id as usize + 15
                ));
            }
            if let Some(o) = legend.iter().find(|o| o.id != t.id && (t.id..t.id + 16).contains(&o.id)) {
                return Err(format!(
                    "legend entry {:?} uses tile {}, which autotile {:?} owns",
                    o.ch as char, o.id, t.ch as char
                ));
            }
        }
        let mut flags = [SOLID; 256];
        for t in legend {
            let own = if t.solid || t.one_way || t.slope.is_some() { SOLID } else { 0 }
                | t.slope.map_or(0, Slope::bits)
                | if t.front { FRONT } else { 0 }
                | if t.one_way { ONE_WAY } else { 0 };
            let span = if t.auto { 16 } else { 1 };
            flags[t.id as usize..t.id as usize + span].fill(own);
            if t.auto {
                flags[t.id as usize] |= AUTO;
            }
        }
        let w = rows.first().ok_or("empty tilemap")?.len();
        let mut tiles = Vec::with_capacity(w * rows.len());
        let mut marks = Vec::new();
        for (y, row) in rows.iter().enumerate() {
            if !row.is_ascii() {
                return Err(format!("tilemap row {} has a character that is not ASCII", y + 1));
            }
            if row.len() != w {
                return Err(format!("ragged tilemap row {} (width {}, expected {w})", y + 1, row.len()));
            }
            for (x, c) in row.bytes().enumerate() {
                let tile = legend.iter().find(|t| t.ch == c);
                tiles.push(tile.map_or(0, |t| t.id));
                if tile.is_none() && !matches!(c, b'.' | b' ') {
                    marks.push((c, x, y));
                }
            }
        }
        let stamp = STAMPS.fetch_add(1, Ordering::Relaxed);
        Ok(Tilemap { w, h: rows.len(), size, bounce: 0.0, tiles, marks, sheet, flags, stamp })
    }

    // A map from tile ids already read from a file (0 is empty); `flags` says per id whether it is solid and drawn in front.
    pub(crate) fn from_ids(
        w: usize,
        h: usize,
        size: usize,
        sheet: Arc<Sprite>,
        tiles: Vec<u8>,
        flags: impl Fn(u8) -> (bool, bool, bool, Option<Slope>),
    ) -> Self {
        let mut table = [0u8; 256];
        for id in 1..=255u8 {
            let (solid, front, one_way, slope) = flags(id);
            table[id as usize] = if solid || one_way || slope.is_some() { SOLID } else { 0 }
                | slope.map_or(0, Slope::bits)
                | if front { FRONT } else { 0 }
                | if one_way { ONE_WAY } else { 0 };
        }
        let stamp = STAMPS.fetch_add(1, Ordering::Relaxed);
        Tilemap { w, h, size, bounce: 0.0, tiles, marks: Vec::new(), sheet, flags: table, stamp }
    }

    pub fn load(path: &str, size: usize, sheet: Arc<Sprite>) -> Result<Self, String> {
        Tilemap::load_with(path, size, sheet, &Tile::DEFAULT)
    }

    pub fn load_with(path: &str, size: usize, sheet: Arc<Sprite>, legend: &[Tile]) -> Result<Self, String> {
        let text = crate::assets::read_text(path)?;
        // Only newlines are trimmed: a space is an empty tile, so a last row may end in them.
        let rows: Vec<&str> = text.trim_end_matches(['\n', '\r']).lines().collect();
        Tilemap::try_from_ascii_with(&rows, size, sheet, legend).map_err(|e| format!("{path}: {e}"))
    }

    pub fn marks(&self, c: u8) -> impl Iterator<Item = Vec2> + '_ {
        self.marks
            .iter()
            .filter(move |m| m.0 == c)
            .map(|&(_, x, y)| Vec2::new((x * self.size) as f32, (y * self.size) as f32))
    }

    pub(crate) fn stamp(&self) -> u64 {
        self.stamp
    }

    /// The tile id at a grid cell, 0 for empty (and for a cell outside the map).
    pub fn tile(&self, (tx, ty): (usize, usize)) -> u8 {
        if tx < self.w && ty < self.h { self.tiles[ty * self.w + tx] } else { 0 }
    }

    /// Replaces a tile, for digging or building (the id must be on the sheet, or 0 to empty the cell). Physics
    /// notices and wakes the bodies resting on the map.
    pub fn set(&mut self, (tx, ty): (usize, usize), id: u8) {
        assert!(tx < self.w && ty < self.h, "tile ({tx}, {ty}) is outside the {}x{} map", self.w, self.h);
        let columns = tile_count(&self.sheet, self.size);
        assert!(id as usize <= columns, "tile {id} does not exist: the sheet has {columns} tiles");
        self.tiles[ty * self.w + tx] = id;
        self.stamp = STAMPS.fetch_add(1, Ordering::Relaxed);
    }

    pub fn pixel_size(&self) -> Vec2 {
        Vec2::new((self.w * self.size) as f32, (self.h * self.size) as f32)
    }

    fn solid(&self, tx: i32, ty: i32) -> bool {
        if tx < 0 || tx >= self.w as i32 {
            return true;
        }
        if ty < 0 || ty >= self.h as i32 {
            return false;
        }
        let id = self.tiles[ty as usize * self.w + tx as usize];
        id != 0 && self.flags[id as usize] & SOLID != 0
    }

    /// Whether the tile is one-way (solid only from above); false outside the map.
    pub fn one_way(&self, (tx, ty): (usize, usize)) -> bool {
        tx < self.w && ty < self.h && self.flags[self.tiles[ty * self.w + tx] as usize] & ONE_WAY != 0
    }

    /// The slope of a tile, if it is one; none outside the map.
    pub fn slope(&self, (tx, ty): (usize, usize)) -> Option<Slope> {
        let inside = tx < self.w && ty < self.h;
        inside.then(|| Slope::from_bits(self.flags[self.tiles[ty * self.w + tx] as usize])).flatten()
    }

    pub(crate) fn one_way_at(&self, tx: i32, ty: i32) -> bool {
        tx >= 0 && ty >= 0 && self.one_way((tx as usize, ty as usize))
    }

    pub(crate) fn slope_at(&self, tx: i32, ty: i32) -> Option<Slope> {
        (tx >= 0 && ty >= 0).then(|| self.slope((tx as usize, ty as usize))).flatten()
    }

    // Whether the map has any one-way tile kind (a cheap, conservative check on the flag table).
    pub(crate) fn has_one_way(&self) -> bool {
        self.flags.iter().any(|f| f & ONE_WAY != 0)
    }

    // `landing` is the bottom edge a falling body had before the move: a one-way tile blocks only from there.
    fn blocked(&self, r: &Rect, landing: Option<f32>) -> bool {
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
    /// box that stands on a slope or lands on one reports a blocked y.
    pub fn slide(&self, r: Rect, dx: f32, dy: f32) -> Slide {
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

    fn step_x(&self, mut r: Rect, dx: f32) -> (Rect, bool, bool) {
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

    fn step_y(&self, mut r: Rect, dy: f32) -> (Rect, bool) {
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
    fn slope_cells(&self, r: &Rect) -> Vec<(Slope, Rect)> {
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
    fn resting_on_slope(&self, r: &Rect) -> bool {
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

    /// The tile under a point, or `None` outside the map.
    pub fn cell_of(&self, p: impl Into<Vec2>) -> Option<(usize, usize)> {
        let p: Vec2 = p.into();
        let (tx, ty) = ((p.x / self.size as f32).floor(), (p.y / self.size as f32).floor());
        (tx >= 0.0 && ty >= 0.0 && tx < self.w as f32 && ty < self.h as f32).then_some((tx as usize, ty as usize))
    }

    /// The middle of a tile in pixels.
    pub fn cell_center(&self, (tx, ty): (usize, usize)) -> Vec2 {
        let s = self.size as f32;
        Vec2::new((tx as f32 + 0.5) * s, (ty as f32 + 0.5) * s)
    }

    /// The shortest walk between two tiles (both ends included) over tiles that are not solid, by A*; `None` when
    /// `to` is solid or cannot be reached. A straight step costs 10 and a diagonal one 14; a diagonal never cuts
    /// past a solid corner.
    pub fn path(&self, from: (usize, usize), to: (usize, usize), diagonal: bool) -> Option<Vec<(usize, usize)>> {
        self.path_with(from, to, diagonal, |_, _| true)
    }

    /// Like `path`, with `open` also able to rule out tiles (a crowd, a trap) that are not solid.
    pub fn path_with(
        &self,
        from: (usize, usize),
        to: (usize, usize),
        diagonal: bool,
        mut open: impl FnMut(usize, usize) -> bool,
    ) -> Option<Vec<(usize, usize)>> {
        astar((self.w, self.h), from, to, diagonal, |x, y| !self.solid(x as i32, y as i32) && open(x, y))
    }

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

    // Which neighbours of an autotile cell are the same tile: up 1, right 2, down 4, left 8; outside the map counts.
    fn joins(&self, tx: usize, ty: usize, id: u8) -> u8 {
        let same = |dx: i32, dy: i32| {
            let (x, y) = (tx as i32 + dx, ty as i32 + dy);
            x < 0
                || y < 0
                || x >= self.w as i32
                || y >= self.h as i32
                || self.tiles[y as usize * self.w + x as usize] == id
        };
        same(0, -1) as u8 | (same(1, 0) as u8) << 1 | (same(0, 1) as u8) << 2 | (same(-1, 0) as u8) << 3
    }

    pub fn draw_list(&self, view: Rect) -> Vec<DrawCmd> {
        let s = self.size as f32;
        let (tx0, ty0) = ((view.x / s).floor().max(0.0) as usize, (view.y / s).floor().max(0.0) as usize);
        let tx1 = (((view.x + view.w) / s).ceil().max(0.0) as usize).min(self.w);
        let ty1 = (((view.y + view.h) / s).ceil().max(0.0) as usize).min(self.h);
        let (tx0, ty0) = (tx0.min(tx1), ty0.min(ty1));
        let (mut back, mut front) = (Vec::new(), Vec::new());
        for ty in ty0..ty1 {
            for tx in tx0..tx1 {
                let id = self.tiles[ty * self.w + tx];
                if id > 0 {
                    let shown = if self.flags[id as usize] & AUTO != 0 { id + self.joins(tx, ty, id) } else { id };
                    let cell = ((tx - tx0) as u32, (ty - ty0) as u32, shown);
                    if self.flags[id as usize] & FRONT != 0 { front.push(cell) } else { back.push(cell) }
                }
            }
        }
        let rect = Rect::new(tx0 as f32 * s, ty0 as f32 * s, (tx1 - tx0) as f32 * s, (ty1 - ty0) as f32 * s);
        [(back, i32::MIN), (front, FRONT_LAYER)]
            .into_iter()
            .filter(|(tiles, _)| !tiles.is_empty())
            .map(|(tiles, layer)| {
                let paint = Paint::Tiles { sheet: self.sheet.clone(), size: self.size as u32, tiles };
                DrawCmd { layer, ..DrawCmd::new(rect, paint) }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A sheet of 20 tiles of 2x2 pixels, enough for ids 1..=20.
    fn wide_sheet() -> Arc<Sprite> {
        Arc::new(Sprite::from_pixels(40, 2, vec![0xFFFF_FFFF; 80]))
    }

    fn shown(map: &Tilemap) -> Vec<Vec<u8>> {
        let mut grid = vec![vec![0; map.w]; map.h];
        for c in map.draw_list(Rect::new(0.0, 0.0, (map.w * map.size) as f32, (map.h * map.size) as f32)) {
            if let Paint::Tiles { tiles, .. } = &c.paint {
                tiles.iter().for_each(|&(x, y, id)| grid[y as usize][x as usize] = id);
            }
        }
        grid
    }

    #[test]
    fn an_autotile_shows_the_tile_that_matches_its_joined_neighbours() {
        let legend = [Tile { ch: b'#', id: 3, solid: true, front: false, auto: true, one_way: false, slope: None }];
        let map = Tilemap::try_from_ascii_with(&["###", "#.#", "###", "..."], 2, wide_sheet(), &legend).unwrap();
        // Masks, row by row: 15 11 15 / 13 - 7 / 11 10 11 (the map edge counts as joined, a gap or empty cell does not).
        let want = [[18, 14, 18], [16, 0, 10], [14, 13, 14], [0, 0, 0]];
        assert_eq!(shown(&map), want.map(|r| r.to_vec()).to_vec());
    }

    #[test]
    fn an_autotile_must_fit_the_sheet_and_own_its_sixteen_ids() {
        let auto = |id| Tile { ch: b'#', id, solid: true, front: false, auto: true, one_way: false, slope: None };
        let err = Tilemap::try_from_ascii_with(&["#"], 2, wide_sheet(), &[auto(6)]).err().unwrap();
        assert!(err.contains("needs tiles 6..=21"), "{err}");
        let other = Tile { ch: b'+', id: 9, solid: true, front: false, auto: false, one_way: false, slope: None };
        let err = Tilemap::try_from_ascii_with(&["#"], 2, wide_sheet(), &[auto(3), other]).err().unwrap();
        assert!(err.contains("owns"), "{err}");
        assert!(Tilemap::try_from_ascii_with(&["#"], 2, wide_sheet(), &[auto(5)]).is_ok());
    }

    #[test]
    fn an_autotile_in_front_draws_all_its_variants_in_front() {
        let front = Tile { ch: b'#', id: 3, solid: false, front: true, auto: true, one_way: false, slope: None };
        let map = Tilemap::try_from_ascii_with(&["#.#"], 2, wide_sheet(), &[front]).unwrap();
        let cmds = map.draw_list(Rect::new(0.0, 0.0, 6.0, 2.0));
        assert_eq!((cmds.len(), cmds[0].layer), (1, i32::MAX - 1));
    }

    #[test]
    fn a_last_row_that_ends_in_spaces_loads_and_non_ascii_text_is_refused() {
        let path = std::env::temp_dir().join(format!("engine-level-{}.txt", std::process::id()));
        let path = path.to_str().unwrap();
        std::fs::write(path, "....\r\n#.  \r\n    \n\n").unwrap();
        let map = Tilemap::load(path, 16, Sprite::test_tiles()).unwrap();
        assert_eq!((map.w, map.h), (4, 3));
        std::fs::remove_file(path).unwrap();
        let err = Tilemap::try_from_ascii(&["é.#"], 16, Sprite::test_tiles()).err().unwrap();
        assert!(err.contains("not ASCII"), "{err}");
    }

    #[test]
    fn letters_become_markers_with_pixel_positions() {
        let map = Tilemap::from_ascii(&["...", ".P.", "###"], 16, Sprite::test_tiles());
        assert_eq!(map.marks(b'P').collect::<Vec<_>>(), vec![Vec2::new(16.0, 16.0)]);
        assert_eq!(map.marks(b'E').count(), 0);
    }

    #[test]
    fn a_broken_level_is_an_error_not_a_crash() {
        let sheet = Sprite::test_tiles();
        let err = Tilemap::try_from_ascii(&["###", "##"], 16, sheet.clone()).err().unwrap();
        assert!(err.contains("row 2"), "{err}");
        assert!(Tilemap::try_from_ascii(&[], 16, sheet.clone()).is_err());
        let err = Tilemap::load("assets/nope.txt", 16, sheet).err().unwrap();
        assert!(err.starts_with("assets/nope.txt"), "{err}");
    }

    #[test]
    fn level_file_loads_with_a_start_marker() {
        let map = Tilemap::load("assets/levels/level1.txt", 16, Sprite::test_tiles()).unwrap();
        assert_eq!((map.w, map.h), (60, 23));
        assert_eq!(map.marks(b'P').count(), 1);
    }

    #[test]
    fn draw_list_covers_only_the_visible_solid_tiles_in_world_space() {
        let map = Tilemap::from_ascii(&["#.#", "...", "..."], 16, Sprite::test_tiles());
        let at = |view| {
            map.draw_list(view)
                .iter()
                .flat_map(|c| match &c.paint {
                    Paint::Tiles { size, tiles, .. } => tiles
                        .iter()
                        .map(|&(gx, gy, _)| (c.rect.x + (gx * size) as f32, c.rect.y + (gy * size) as f32))
                        .collect(),
                    _ => Vec::new(),
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(at(Rect::new(0.0, 0.0, 48.0, 48.0)), [(0.0, 0.0), (32.0, 0.0)]);
        assert_eq!(at(Rect::new(0.0, 0.0, 16.0, 16.0)), [(0.0, 0.0)]);
        assert_eq!(at(Rect::new(20.0, 0.0, 40.0, 16.0)), [(32.0, 0.0)]);
        assert!(map.draw_list(Rect::new(0.0, 32.0, 48.0, 16.0)).is_empty());
        assert_eq!(map.draw_list(Rect::new(0.0, 0.0, 48.0, 48.0)).len(), 1);
    }

    #[test]
    fn falling_box_lands_on_floor_and_wall_stops_it() {
        let map = Tilemap::from_ascii(&["...", "...", "###"], 16, Sprite::test_tiles());
        let Slide { rect: r, blocked_x: hx, blocked_y: hy } = map.slide(Rect::new(0.0, 20.0, 10.0, 10.0), 0.0, 5.0);
        assert_eq!((r.y, hx, hy), (22.0, false, true));
        let Slide { rect: r, blocked_x: hx, blocked_y: _ } = map.slide(Rect::new(1.0, 0.0, 10.0, 10.0), -3.0, 0.0);
        assert_eq!((r.x, hx), (0.0, true));
    }

    #[test]
    fn set_replaces_a_tile_and_gives_the_map_a_new_stamp() {
        let sheet = Sprite::test_tiles();
        let mut map = Tilemap::from_ascii(&["...", "###"], 16, sheet);
        let before = map.stamp();
        map.set((1, 0), 2);
        assert_eq!((map.tile((1, 0)), map.tile((1, 1)), map.tile((0, 0))), (2, 1, 0));
        assert!(map.stamp() != before);
        assert!(!map.solid_in(Rect::new(16.0, 0.0, 16.0, 16.0)).is_empty());
    }

    fn legend() -> [Tile; 3] {
        [
            Tile::DEFAULT[0],
            Tile { ch: b':', id: 3, solid: false, front: false, auto: false, one_way: false, slope: None },
            Tile { ch: b'v', id: 4, solid: false, front: true, auto: false, one_way: false, slope: None },
        ]
    }

    #[test]
    fn a_legend_makes_tiles_that_do_not_block_and_tiles_drawn_in_front() {
        let sheet = Sprite::test_tiles();
        let map = Tilemap::try_from_ascii_with(&["P.v", ":::", "###"], 16, sheet, &legend()).unwrap();
        assert_eq!(map.marks(b'P').count(), 1, "characters outside the legend are still markers");
        assert_eq!(map.solid_in(Rect::new(0.0, 0.0, 48.0, 48.0)).len(), 3, "only the ground blocks");
        let Slide { rect: moved, blocked_x: hit_x, blocked_y: hit_y } =
            map.slide(Rect::new(4.0, 20.0, 8.0, 8.0), 10.0, 0.0);
        assert!(!hit_x && !hit_y && moved.x == 14.0, "walks through the background wall");
        let cmds = map.draw_list(Rect::new(0.0, 0.0, 48.0, 48.0));
        assert_eq!(cmds.iter().map(|c| c.layer).collect::<Vec<_>>(), [i32::MIN, FRONT_LAYER]);
        let count = |c: &DrawCmd| match &c.paint {
            Paint::Tiles { tiles, .. } => tiles.len(),
            _ => 0,
        };
        assert_eq!((count(&cmds[0]), count(&cmds[1])), (6, 1), "behind: wall and ground, in front: the leaf");
    }

    #[test]
    fn a_one_way_tile_stops_what_falls_onto_it_and_lets_everything_else_through() {
        let one_way = Tile { ch: b'=', id: 3, solid: false, front: false, auto: false, one_way: true, slope: None };
        let map =
            Tilemap::try_from_ascii_with(&["...", "...", ".=.", "..."], 16, Sprite::test_tiles(), &[one_way]).unwrap();
        assert!(map.one_way((1, 2)) && !map.one_way((0, 2)) && !map.one_way_at(-1, 2) && !map.one_way((1, 9)));
        let body = |x: f32, y: f32| Rect { x, y, w: 8.0, h: 8.0 };
        let Slide { rect: landed, blocked_x: _, blocked_y: hit } = map.slide(body(20.0, 20.0), 0.0, 10.0);
        assert!(hit && landed.y == 24.0, "lands on the top face: {landed:?}");
        let Slide { rect: up, blocked_x: _, blocked_y: hit } = map.slide(body(20.0, 40.0), 0.0, -12.0);
        assert!(!hit && up.y == 28.0, "rises through it from below");
        let Slide { rect: side, blocked_x: hit, blocked_y: _ } = map.slide(body(4.0, 34.0), 20.0, 0.0);
        assert!(!hit && side.x == 24.0, "walks through it sideways");
        let Slide { rect: inside, blocked_x: _, blocked_y: hit } = map.slide(body(20.0, 28.0), 0.0, 6.0);
        assert!(!hit && inside.y == 34.0, "a body already sunk into it keeps falling");
    }

    #[test]
    fn a_slope_tile_reports_its_corner_and_counts_as_a_tile_for_the_rest() {
        let slope = Tile {
            ch: b'/',
            id: 3,
            solid: false,
            front: false,
            auto: false,
            one_way: false,
            slope: Some(Slope::BottomRight),
        };
        let map =
            Tilemap::try_from_ascii_with(&["...", ".//", "###"], 16, Sprite::test_tiles(), &[Tile::DEFAULT[0], slope])
                .unwrap();
        assert_eq!(map.slope((1, 1)), Some(Slope::BottomRight));
        assert_eq!(
            (map.slope((0, 1)), map.slope((0, 2)), map.slope_at(-1, 1), map.slope((1, 9))),
            (None, None, None, None)
        );
        assert_eq!(map.solid_in(Rect::new(16.0, 16.0, 32.0, 16.0)).len(), 2, "it still occupies its cell");
        let triangle = Slope::BottomRight.triangle(&Rect::new(0.0, 0.0, 16.0, 16.0));
        assert_eq!(triangle, [(16.0, 16.0), (0.0, 16.0), (16.0, 0.0)]);
        let top = Slope::TopLeft.triangle(&Rect::new(0.0, 0.0, 16.0, 16.0));
        assert_eq!(top, [(0.0, 0.0), (16.0, 0.0), (0.0, 16.0)]);
    }

    fn slope_map(rows: &[&str]) -> Tilemap {
        let tile = |ch, id, slope| Tile { ch, id, solid: false, front: false, auto: false, one_way: false, slope };
        let legend = [
            Tile::DEFAULT[0],
            tile(b'/', 2, Some(Slope::BottomRight)),
            tile(b'\\', 3, Some(Slope::BottomLeft)),
            tile(b'^', 4, Some(Slope::TopLeft)),
        ];
        Tilemap::try_from_ascii_with(rows, 16, Sprite::test_tiles(), &legend).unwrap()
    }

    // Rising to the right over two tiles from the floor (y = 64 at x = 32) to a plateau (y = 32 from x = 64).
    fn ramp() -> Tilemap {
        slope_map(&["........", "........", ".../####", "../#####", "########"])
    }

    #[test]
    fn a_box_walks_up_a_ramp_onto_the_plateau_standing_on_the_diagonal_all_the_way() {
        let map = ramp();
        let mut r = Rect::new(4.0, 54.0, 10.0, 10.0);
        let mut worst = 0.0f32;
        for _ in 0..40 {
            let Slide { rect: moved, blocked_x: hit_x, blocked_y: hit_y } = map.slide(r, 2.0, 2.0);
            let line = (64.0 - (moved.x + moved.w - 32.0)).clamp(32.0, 64.0);
            if moved.x + moved.w > 34.0 && moved.x + moved.w < 64.0 {
                worst = worst.max((moved.y + moved.h - line).abs());
            }
            assert!(!hit_x && hit_y, "grounded the whole way up: {moved:?}");
            r = moved;
        }
        assert!(worst < 0.02, "its lower right corner rode the line: {worst}");
        assert_eq!((r.y + r.h, r.x > 70.0), (32.0, true), "it ends on the plateau: {r:?}");
    }

    #[test]
    fn a_box_walks_down_a_ramp_without_leaving_the_diagonal() {
        let map = slope_map(&["........", "........", "####\\...", "#####\\..", "########"]);
        let mut r = Rect::new(40.0, 22.0, 10.0, 10.0);
        for _ in 0..40 {
            let Slide { rect: moved, blocked_x: _, blocked_y: hit_y } = map.slide(r, 2.0, 2.0);
            assert!(hit_y, "it never floats above the slope on the way down: {moved:?}");
            r = moved;
        }
        assert_eq!(r.y + r.h, 64.0, "it ends on the floor: {r:?}");
    }

    #[test]
    fn a_slope_is_climbed_from_its_low_end_and_its_tall_side_is_a_wall() {
        let Slide { rect: moved, blocked_x: hit_x, blocked_y: _ } =
            ramp().slide(Rect::new(20.0, 54.0, 10.0, 10.0), 8.0, 0.0);
        assert!(!hit_x && (moved.y + moved.h - 58.0).abs() < 0.01, "from the low end it climbs: {moved:?}");
        let wall = slope_map(&["........", ".......\\", "########"]);
        let Slide { rect: stopped, blocked_x: hit, blocked_y: _ } =
            wall.slide(Rect::new(96.0, 22.0, 10.0, 10.0), 10.0, 0.0);
        assert!(hit && stopped.x == 102.0, "the left leg of a falling slope stops a box walking into it: {stopped:?}");
    }

    #[test]
    fn a_box_falls_onto_a_slope_and_rises_into_the_flat_underside() {
        let map = ramp();
        let Slide { rect: landed, blocked_x: _, blocked_y: hit_y } =
            map.slide(Rect::new(44.0, 20.0, 10.0, 10.0), 0.0, 14.0);
        assert!(hit_y && (landed.y + landed.h - 42.0).abs() < 0.01, "lands on the line at its right edge: {landed:?}");
        let floating = slope_map(&["........", "........", "..///...", "........"]);
        let Slide { rect: stopped, blocked_x: _, blocked_y: hit } =
            floating.slide(Rect::new(40.0, 60.0, 10.0, 10.0), 0.0, -20.0);
        assert!(hit && stopped.y == 48.0, "stops below the flat bottom of the tile: {stopped:?}");
    }

    #[test]
    fn a_ceiling_slope_pushes_a_box_down_along_its_diagonal() {
        let map = slope_map(&["^.......", "........", "........", "########"]);
        let Slide { rect: moved, blocked_x: _, blocked_y: hit_y } =
            map.slide(Rect::new(2.0, 20.0, 8.0, 8.0), 0.0, -14.0);
        assert!(hit_y && (moved.y - 14.0).abs() < 0.01, "its top rests on the diagonal at its left edge: {moved:?}");
    }

    #[test]
    fn maps_without_slopes_slide_exactly_as_before() {
        let map = Tilemap::from_ascii(&["......", "......", "######"], 16, Sprite::test_tiles());
        let Slide { rect: moved, blocked_x: hit_x, blocked_y: hit_y } =
            map.slide(Rect::new(4.0, 20.0, 8.0, 8.0), 3.0, 9.0);
        assert_eq!((moved.x, moved.y, hit_x, hit_y), (7.0, 24.0, false, true));
    }

    #[test]
    fn a_legend_that_hides_empty_tiles_is_an_error() {
        let sheet = Sprite::test_tiles();
        let bad = [Tile { ch: b'.', id: 1, solid: true, front: false, auto: false, one_way: false, slope: None }];
        assert!(Tilemap::try_from_ascii_with(&["#"], 16, sheet.clone(), &bad).is_err());
        let zero = [Tile { ch: b'#', id: 0, solid: true, front: false, auto: false, one_way: false, slope: None }];
        assert!(Tilemap::try_from_ascii_with(&["#"], 16, sheet, &zero).is_err());
    }

    #[test]
    fn a_tile_the_sheet_does_not_have_is_refused_instead_of_crashing_the_renderer() {
        let sheet = Sprite::test_tiles();
        let columns = tile_count(&sheet, 16);
        let past = [Tile {
            ch: b'#',
            id: columns as u8 + 1,
            solid: true,
            front: false,
            auto: false,
            one_way: false,
            slope: None,
        }];
        let err = Tilemap::try_from_ascii_with(&["#"], 16, sheet.clone(), &past).err().unwrap();
        assert!(err.contains("the sheet has"), "{err}");
        let mut map = Tilemap::from_ascii(&["..", "##"], 16, sheet);
        map.set((0, 0), columns as u8);
        map.set((1, 0), 0);
        assert_eq!(map.tile((0, 0)), columns as u8);
    }

    #[test]
    #[should_panic(expected = "does not exist")]
    fn setting_a_tile_past_the_sheet_panics_at_the_call_not_when_drawn() {
        let sheet = Sprite::test_tiles();
        let mut map = Tilemap::from_ascii(&["."], 16, sheet);
        map.set((0, 0), 200);
    }

    #[test]
    fn a_view_past_the_map_a_cell_outside_it_and_bad_sizes_do_not_panic() {
        let map = Tilemap::from_ascii(&["#.", "#."], 2, wide_sheet());
        assert!(map.draw_list(Rect::new(1000.0, 1000.0, 64.0, 64.0)).is_empty());
        assert_eq!((map.tile((2, 0)), map.tile((0, 5))), (0, 0));
        assert!(Tilemap::try_from_ascii(&["#"], 0, wide_sheet()).is_err());
        let wide = Arc::new(Sprite::from_pixels(600, 2, vec![0; 1200]));
        let auto = Tile { ch: b'#', id: 250, solid: true, front: false, auto: true, one_way: false, slope: None };
        assert!(Tilemap::try_from_ascii_with(&["#"], 2, wide, &[auto]).is_err());
    }

    // Marching along the ray in small steps finds the same tile a little later than the exact walk does.
    #[test]
    fn a_ray_finds_the_tile_a_fine_march_finds_on_random_maps() {
        let mut rng = crate::Rng::new(9);
        for case in 0..200 {
            let rows: Vec<String> =
                (0..8).map(|_| (0..10).map(|_| if rng.f32() < 0.25 { '#' } else { '.' }).collect()).collect();
            let map = Tilemap::from_ascii(&rows.iter().map(String::as_str).collect::<Vec<_>>(), 4, wide_sheet());
            let from = Vec2::new(rng.range(-6.0, 46.0), rng.range(-6.0, 38.0));
            let dir = Vec2::from_angle(rng.range(0.0, 6.3)) * rng.range(0.5, 3.0);
            let max = rng.range(1.0, 60.0);
            let step = 0.01;
            let marched = (0..).map(|i| i as f32 * step).take_while(|t| *t <= max).find(|t| {
                let p = from + dir * *t;
                map.solid((p.x / 4.0).floor() as i32, (p.y / 4.0).floor() as i32)
            });
            let hit = map.raycast(from, dir, max);
            match (hit, marched) {
                (None, None) => {}
                (Some(h), Some(m)) => {
                    assert!(
                        m >= h.t - 1e-3 && m - h.t <= 0.05 / dir.length().min(1.0),
                        "case {case}: exact {} marched {m}",
                        h.t
                    );
                    let inside = from + dir * (h.t + 0.02);
                    assert_eq!(
                        ((inside.x / 4.0).floor() as i32, (inside.y / 4.0).floor() as i32),
                        h.tile,
                        "case {case}"
                    );
                }
                (h, m) => panic!("case {case}: exact {h:?} marched {m:?} for {from:?} {dir:?} {max}"),
            }
        }
    }

    #[test]
    fn a_ray_reports_the_face_it_crossed_and_stops_where_it_cannot_hit_anything() {
        let map = Tilemap::from_ascii(&["....", "..#.", "...."], 10, wide_sheet());
        let hit = map.raycast((5.0, 15.0), (1.0, 0.0), 100.0).unwrap();
        assert_eq!((hit.t, hit.tile, hit.normal), (15.0, (2, 1), Vec2::new(-1.0, 0.0)));
        let down = map.raycast((25.0, 0.0), (0.0, 1.0), 100.0).unwrap();
        assert_eq!((down.t, down.normal), (10.0, Vec2::new(0.0, -1.0)));
        assert_eq!(map.raycast((25.0, 15.0), (1.0, 0.0), 100.0).unwrap().t, 0.0, "starting inside");
        assert!(map.raycast((5.0, 15.0), (1.0, 0.0), 10.0).is_none(), "short of it");
        assert_eq!(map.raycast((5.0, 5.0), (-1.0, 0.0), f32::INFINITY).unwrap().tile, (-1, 0), "the map edge is solid");
        assert!(map.raycast((5.0, 5.0), (0.0, -1.0), f32::INFINITY).is_none(), "straight up out of the map");
        assert!(
            map.raycast((5.0, 5.0), (0.0, 0.0), 10.0).is_none()
                && map.raycast((f32::NAN, 0.0), (1.0, 0.0), 9.0).is_none()
        );
        assert_eq!(
            map.raycast((5.0, -50.0), (1.0, 0.0), f32::INFINITY).unwrap().tile,
            (4, -5),
            "the side walls reach up"
        );
    }

    #[test]
    fn paths_avoid_solid_tiles_and_can_be_refused_tiles_too() {
        let map = Tilemap::from_ascii(&["....", ".##.", "...."], 10, wide_sheet());
        let path = map.path((0, 0), (3, 2), false).unwrap();
        assert!(path.iter().all(|&(x, y)| map.tile((x, y)) == 0) && path.len() == 6, "{path:?}");
        assert!(map.path((0, 0), (1, 1), true).is_none(), "the goal is solid");
        let blocked = map.path_with((0, 0), (3, 0), false, |x, y| (x, y) != (1, 0));
        assert!(blocked.is_some_and(|p| p.len() > 4), "it goes around a tile the game ruled out");
        assert_eq!(
            (map.cell_of((25.0, 5.0)), map.cell_of((-1.0, 5.0)), map.cell_of((40.0, 5.0))),
            (Some((2, 0)), None, None)
        );
        assert_eq!(map.cell_center((1, 2)), Vec2::new(15.0, 25.0));
    }

    #[test]
    fn a_sheet_with_several_rows_shows_the_right_tile_for_each_id() {
        let mut px = vec![0; 16];
        for (n, color) in [0xFF0000, 0x00FF00, 0x0000FF, 0xFFFF00].into_iter().enumerate() {
            let (x, y) = (n % 2 * 2, n / 2 * 2);
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                px[(y + dy) * 4 + x + dx] = 0xFF00_0000 | color;
            }
        }
        let sheet = Arc::new(Sprite::from_pixels(4, 4, px));
        let legend: Vec<Tile> = b"abcd"
            .iter()
            .enumerate()
            .map(|(i, &ch)| Tile {
                ch,
                id: i as u8 + 1,
                solid: true,
                front: false,
                auto: false,
                one_way: false,
                slope: None,
            })
            .collect();
        let map = Tilemap::try_from_ascii_with(&["abcd"], 2, sheet, &legend).unwrap();
        let mut soft = crate::render::Soft::new(8, 2, 1);
        soft.render(&map.draw_list(Rect::new(0.0, 0.0, 8.0, 2.0)));
        let colors: Vec<u32> = [0, 2, 4, 6].iter().map(|&x| soft.buf[x] & 0xFF_FFFF).collect();
        assert_eq!(colors, [0xFF0000, 0x00FF00, 0x0000FF, 0xFFFF00]);
    }
}
