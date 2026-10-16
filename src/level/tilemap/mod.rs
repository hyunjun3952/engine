use crate::base::in_file;
use crate::level::path::astar;
use crate::{DrawCmd, Paint, Rect, Sprite, Vec2};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

mod collide;
mod draw;
#[cfg(test)]
mod tests;
mod tile;

pub(crate) use tile::tile_count;
pub use tile::{Slide, Slope, Tile, TileHit};

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

/// A grid of square tiles from one sheet: drawing, collision for [`Physics`](crate::Physics) and [`Tilemap::slide`], raycasts and A*.
///
/// Load a plain-text grid with [`Tilemap::load`] (`#` and `+` are solid, `.` or space is empty, any other character is
/// a marker read with [`Tilemap::marks`]) or a Tiled map with [`Tiled::load`](crate::Tiled::load). Cells are `(usize, usize)`. Draw it with
/// `gfx.draw(map.draw_list(gfx.view()))`.
///
/// ```
/// use engine::{Rect, Sprite, Tilemap};
/// use std::sync::Arc;
///
/// let sheet = Arc::new(Sprite::new(16, 8, vec![0; 128]));
/// let map = Tilemap::from_ascii(&["P...", "####"], 8, sheet);
/// assert_eq!(map.marks(b'P').count(), 1);
/// assert_eq!(map.pixel_size(), (32.0, 16.0).into());
/// ```
pub struct Tilemap {
    /// Width in tiles.
    pub w: usize,
    /// Height in tiles.
    pub h: usize,
    /// Side of a tile in pixels.
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
    /// Builds a map from rows of characters; panics on a bad level, see `try_from_ascii`.
    pub fn from_ascii(rows: &[&str], size: usize, sheet: Arc<Sprite>) -> Self {
        Tilemap::try_from_ascii(rows, size, sheet).unwrap_or_else(|e| panic!("{e}"))
    }

    /// Builds a map from rows of characters, or says why it cannot.
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
            if let Some(o) = legend.iter().find(|o| o.id != t.id && (t.id..=t.id + 15).contains(&o.id)) {
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

    /// Reads a level file with the default legend.
    pub fn load(path: &str, size: usize, sheet: Arc<Sprite>) -> Result<Self, String> {
        Tilemap::load_with(path, size, sheet, &Tile::DEFAULT)
    }

    /// Reads a level file with a custom legend.
    pub fn load_with(path: &str, size: usize, sheet: Arc<Sprite>, legend: &[Tile]) -> Result<Self, String> {
        let text = crate::base::read_text(path)?;
        // Only newlines are trimmed: a space is an empty tile, so a last row may end in them.
        let rows: Vec<&str> = text.trim_end_matches(['\n', '\r']).lines().collect();
        Tilemap::try_from_ascii_with(&rows, size, sheet, legend).map_err(in_file(path))
    }

    /// The pixel position of every cell marked with `c`.
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

    /// Width and height of the map in pixels.
    pub fn pixel_size(&self) -> Vec2 {
        Vec2::new((self.w * self.size) as f32, (self.h * self.size) as f32)
    }

    pub(crate) fn solid(&self, tx: i32, ty: i32) -> bool {
        if tx < 0 || tx >= self.w as i32 {
            return true;
        }
        if ty < 0 || ty >= self.h as i32 {
            return false;
        }
        let id = self.tiles[ty as usize * self.w + tx as usize];
        id != 0 && self.flags[id as usize] & SOLID != 0
    }

    // The flags of a tile, 0 outside the map.
    fn flags_at(&self, tx: i32, ty: i32) -> u8 {
        match (usize::try_from(tx), usize::try_from(ty)) {
            (Ok(x), Ok(y)) if x < self.w && y < self.h => self.flags[self.tiles[y * self.w + x] as usize],
            _ => 0,
        }
    }

    /// Whether the tile is one-way (solid only from above); false outside the map.
    pub fn one_way(&self, (tx, ty): (usize, usize)) -> bool {
        self.one_way_at(i32::try_from(tx).unwrap_or(-1), i32::try_from(ty).unwrap_or(-1))
    }

    /// The slope of a tile, if it is one; none outside the map.
    pub fn slope(&self, (tx, ty): (usize, usize)) -> Option<Slope> {
        self.slope_at(i32::try_from(tx).unwrap_or(-1), i32::try_from(ty).unwrap_or(-1))
    }

    pub(crate) fn one_way_at(&self, tx: i32, ty: i32) -> bool {
        self.flags_at(tx, ty) & ONE_WAY != 0
    }

    pub(crate) fn slope_at(&self, tx: i32, ty: i32) -> Option<Slope> {
        Slope::from_bits(self.flags_at(tx, ty))
    }

    // Whether the map has any one-way tile kind (a cheap, conservative check on the flag table).
    pub(crate) fn has_one_way(&self) -> bool {
        self.flags.iter().any(|f| f & ONE_WAY != 0)
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
}
