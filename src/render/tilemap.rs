use crate::{DrawCmd, Paint, Rect, Sprite};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

static STAMPS: AtomicU64 = AtomicU64::new(1);
const SOLID: u8 = 1;
const FRONT: u8 = 2;
const AUTO: u8 = 4;
// Above entities (the debug outlines are on `i32::MAX`); tiles behind them sit at `i32::MIN`.
const FRONT_LAYER: i32 = i32::MAX - 1;

/// What a character in a level file means: the sheet tile it shows (ids start at 1, the sheet is a row of
/// square tiles), whether bodies collide with it, and whether it is drawn over entities instead of behind them.
/// The flags belong to the id, so two characters sharing an id share them.
///
/// With `auto` the id is the first of 16 consecutive sheet tiles that show how the tile joins its neighbours of the
/// same id: tile `id + m` where `m` adds 1 for a joined tile above, 2 to the right, 4 below and 8 to the left (the
/// edge of the map counts as joined). The other 15 ids belong to it and no other entry may use them.
#[derive(Clone, Copy, Debug)]
pub struct Tile {
    pub ch: u8,
    pub id: u8,
    pub solid: bool,
    pub front: bool,
    pub auto: bool,
}

impl Tile {
    /// `#` ground and `+` brick, both solid and drawn behind entities: what `Tilemap::load` uses.
    pub const DEFAULT: [Tile; 2] = [
        Tile { ch: b'#', id: 1, solid: true, front: false, auto: false },
        Tile { ch: b'+', id: 2, solid: true, front: false, auto: false },
    ];
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
        let columns = sheet.size().0 / size;
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
            let own = if t.solid { SOLID } else { 0 } | if t.front { FRONT } else { 0 };
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

    pub fn load(path: &str, size: usize, sheet: Arc<Sprite>) -> Self {
        Tilemap::try_load(path, size, sheet).unwrap_or_else(|e| panic!("{e}"))
    }

    pub fn try_load(path: &str, size: usize, sheet: Arc<Sprite>) -> Result<Self, String> {
        Tilemap::try_load_with(path, size, sheet, &Tile::DEFAULT)
    }

    pub fn try_load_with(path: &str, size: usize, sheet: Arc<Sprite>, legend: &[Tile]) -> Result<Self, String> {
        let text = crate::assets::read_text(path)?;
        // Only newlines are trimmed: a space is an empty tile, so a last row may end in them.
        let rows: Vec<&str> = text.trim_end_matches(['\n', '\r']).lines().collect();
        Tilemap::try_from_ascii_with(&rows, size, sheet, legend).map_err(|e| format!("{path}: {e}"))
    }

    pub fn marks(&self, c: u8) -> impl Iterator<Item = (f32, f32)> + '_ {
        self.marks.iter().filter(move |m| m.0 == c).map(|&(_, x, y)| ((x * self.size) as f32, (y * self.size) as f32))
    }

    pub(crate) fn stamp(&self) -> u64 {
        self.stamp
    }

    /// The tile id at a grid cell, 0 for empty (and for a cell outside the map).
    pub fn tile(&self, tx: usize, ty: usize) -> u8 {
        if tx < self.w && ty < self.h { self.tiles[ty * self.w + tx] } else { 0 }
    }

    /// Replaces a tile, for digging or building (the id must be on the sheet, or 0 to empty the cell). Physics
    /// notices and wakes the bodies resting on the map.
    pub fn set(&mut self, tx: usize, ty: usize, id: u8) {
        assert!(tx < self.w && ty < self.h, "tile ({tx}, {ty}) is outside the {}x{} map", self.w, self.h);
        let columns = self.sheet.size().0 / self.size;
        assert!(id as usize <= columns, "tile {id} does not exist: the sheet has {columns} tiles");
        self.tiles[ty * self.w + tx] = id;
        self.stamp = STAMPS.fetch_add(1, Ordering::Relaxed);
    }

    pub fn pixel_size(&self) -> (f32, f32) {
        ((self.w * self.size) as f32, (self.h * self.size) as f32)
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

    fn blocked(&self, r: &Rect) -> bool {
        let s = self.size as f32;
        let (x0, x1) = ((r.x / s).floor() as i32, ((r.x + r.w - 0.001) / s).floor() as i32);
        let (y0, y1) = ((r.y / s).floor() as i32, ((r.y + r.h - 0.001) / s).floor() as i32);
        (y0..=y1).any(|ty| (x0..=x1).any(|tx| self.solid(tx, ty)))
    }

    pub fn slide(&self, r: &Rect, dx: f32, dy: f32) -> (Rect, bool, bool) {
        let s = self.size as f32;
        let mut r = *r;
        let (mut hit_x, mut hit_y) = (false, false);
        r.x += dx;
        if dx != 0.0 && self.blocked(&r) {
            hit_x = true;
            r.x = if dx > 0.0 { ((r.x + r.w - 0.001) / s).floor() * s - r.w } else { ((r.x / s).floor() + 1.0) * s };
        }
        r.y += dy;
        if dy != 0.0 && self.blocked(&r) {
            hit_y = true;
            r.y = if dy > 0.0 { ((r.y + r.h - 0.001) / s).floor() * s - r.h } else { ((r.y / s).floor() + 1.0) * s };
        }
        (r, hit_x, hit_y)
    }

    pub fn solid_in(&self, area: Rect) -> Vec<Rect> {
        let s = self.size as f32;
        let (x0, x1) = ((area.x / s).floor() as i32, ((area.x + area.w) / s).ceil() as i32);
        let (y0, y1) = ((area.y / s).floor() as i32, ((area.y + area.h) / s).ceil() as i32);
        let mut out = Vec::new();
        for ty in y0..y1 {
            for tx in x0..x1 {
                if self.solid(tx, ty) {
                    out.push(Rect { x: tx as f32 * s, y: ty as f32 * s, w: s, h: s });
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
        let rect = Rect { x: tx0 as f32 * s, y: ty0 as f32 * s, w: (tx1 - tx0) as f32 * s, h: (ty1 - ty0) as f32 * s };
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
        for c in map.draw_list(Rect { x: 0.0, y: 0.0, w: (map.w * map.size) as f32, h: (map.h * map.size) as f32 }) {
            if let Paint::Tiles { tiles, .. } = &c.paint {
                tiles.iter().for_each(|&(x, y, id)| grid[y as usize][x as usize] = id);
            }
        }
        grid
    }

    #[test]
    fn an_autotile_shows_the_tile_that_matches_its_joined_neighbours() {
        let legend = [Tile { ch: b'#', id: 3, solid: true, front: false, auto: true }];
        let map = Tilemap::try_from_ascii_with(&["###", "#.#", "###", "..."], 2, wide_sheet(), &legend).unwrap();
        // Masks, row by row: 15 11 15 / 13 - 7 / 11 10 11 (the map edge counts as joined, a gap or empty cell does not).
        let want = [[18, 14, 18], [16, 0, 10], [14, 13, 14], [0, 0, 0]];
        assert_eq!(shown(&map), want.map(|r| r.to_vec()).to_vec());
    }

    #[test]
    fn an_autotile_must_fit_the_sheet_and_own_its_sixteen_ids() {
        let auto = |id| Tile { ch: b'#', id, solid: true, front: false, auto: true };
        let err = Tilemap::try_from_ascii_with(&["#"], 2, wide_sheet(), &[auto(6)]).err().unwrap();
        assert!(err.contains("needs tiles 6..=21"), "{err}");
        let other = Tile { ch: b'+', id: 9, solid: true, front: false, auto: false };
        let err = Tilemap::try_from_ascii_with(&["#"], 2, wide_sheet(), &[auto(3), other]).err().unwrap();
        assert!(err.contains("owns"), "{err}");
        assert!(Tilemap::try_from_ascii_with(&["#"], 2, wide_sheet(), &[auto(5)]).is_ok());
    }

    #[test]
    fn an_autotile_in_front_draws_all_its_variants_in_front() {
        let front = Tile { ch: b'#', id: 3, solid: false, front: true, auto: true };
        let map = Tilemap::try_from_ascii_with(&["#.#"], 2, wide_sheet(), &[front]).unwrap();
        let cmds = map.draw_list(Rect { x: 0.0, y: 0.0, w: 6.0, h: 2.0 });
        assert_eq!((cmds.len(), cmds[0].layer), (1, i32::MAX - 1));
    }

    #[test]
    fn a_last_row_that_ends_in_spaces_loads_and_non_ascii_text_is_refused() {
        let path = std::env::temp_dir().join(format!("engine-level-{}.txt", std::process::id()));
        let path = path.to_str().unwrap();
        std::fs::write(path, "....\r\n#.  \r\n    \n\n").unwrap();
        let map = Tilemap::try_load(path, 16, Sprite::test_tiles()).unwrap();
        assert_eq!((map.w, map.h), (4, 3));
        std::fs::remove_file(path).unwrap();
        let err = Tilemap::try_from_ascii(&["é.#"], 16, Sprite::test_tiles()).err().unwrap();
        assert!(err.contains("not ASCII"), "{err}");
    }

    #[test]
    fn letters_become_markers_with_pixel_positions() {
        let map = Tilemap::from_ascii(&["...", ".P.", "###"], 16, Sprite::test_tiles());
        assert_eq!(map.marks(b'P').collect::<Vec<_>>(), vec![(16.0, 16.0)]);
        assert_eq!(map.marks(b'E').count(), 0);
    }

    #[test]
    fn a_broken_level_is_an_error_not_a_crash() {
        let sheet = Sprite::test_tiles();
        let err = Tilemap::try_from_ascii(&["###", "##"], 16, sheet.clone()).err().unwrap();
        assert!(err.contains("row 2"), "{err}");
        assert!(Tilemap::try_from_ascii(&[], 16, sheet.clone()).is_err());
        let err = Tilemap::try_load("assets/nope.txt", 16, sheet).err().unwrap();
        assert!(err.starts_with("assets/nope.txt"), "{err}");
    }

    #[test]
    fn level_file_loads_with_a_start_marker() {
        let map = Tilemap::load("assets/levels/level1.txt", 16, Sprite::test_tiles());
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
        assert_eq!(at(Rect { x: 0.0, y: 0.0, w: 48.0, h: 48.0 }), [(0.0, 0.0), (32.0, 0.0)]);
        assert_eq!(at(Rect { x: 0.0, y: 0.0, w: 16.0, h: 16.0 }), [(0.0, 0.0)]);
        assert_eq!(at(Rect { x: 20.0, y: 0.0, w: 40.0, h: 16.0 }), [(32.0, 0.0)]);
        assert!(map.draw_list(Rect { x: 0.0, y: 32.0, w: 48.0, h: 16.0 }).is_empty());
        assert_eq!(map.draw_list(Rect { x: 0.0, y: 0.0, w: 48.0, h: 48.0 }).len(), 1);
    }

    #[test]
    fn falling_box_lands_on_floor_and_wall_stops_it() {
        let map = Tilemap::from_ascii(&["...", "...", "###"], 16, Sprite::test_tiles());
        let (r, hx, hy) = map.slide(&Rect { x: 0.0, y: 20.0, w: 10.0, h: 10.0 }, 0.0, 5.0);
        assert_eq!((r.y, hx, hy), (22.0, false, true));
        let (r, hx, _) = map.slide(&Rect { x: 1.0, y: 0.0, w: 10.0, h: 10.0 }, -3.0, 0.0);
        assert_eq!((r.x, hx), (0.0, true));
    }

    #[test]
    fn set_replaces_a_tile_and_gives_the_map_a_new_stamp() {
        let sheet = Sprite::test_tiles();
        let mut map = Tilemap::from_ascii(&["...", "###"], 16, sheet);
        let before = map.stamp();
        map.set(1, 0, 2);
        assert_eq!((map.tile(1, 0), map.tile(1, 1), map.tile(0, 0)), (2, 1, 0));
        assert!(map.stamp() != before);
        assert!(!map.solid_in(Rect { x: 16.0, y: 0.0, w: 16.0, h: 16.0 }).is_empty());
    }

    fn legend() -> [Tile; 3] {
        [
            Tile::DEFAULT[0],
            Tile { ch: b':', id: 3, solid: false, front: false, auto: false },
            Tile { ch: b'v', id: 4, solid: false, front: true, auto: false },
        ]
    }

    #[test]
    fn a_legend_makes_tiles_that_do_not_block_and_tiles_drawn_in_front() {
        let sheet = Sprite::test_tiles();
        let map = Tilemap::try_from_ascii_with(&["P.v", ":::", "###"], 16, sheet, &legend()).unwrap();
        assert_eq!(map.marks(b'P').count(), 1, "characters outside the legend are still markers");
        assert_eq!(map.solid_in(Rect { x: 0.0, y: 0.0, w: 48.0, h: 48.0 }).len(), 3, "only the ground blocks");
        let (moved, hit_x, hit_y) = map.slide(&Rect { x: 4.0, y: 20.0, w: 8.0, h: 8.0 }, 10.0, 0.0);
        assert!(!hit_x && !hit_y && moved.x == 14.0, "walks through the background wall");
        let cmds = map.draw_list(Rect { x: 0.0, y: 0.0, w: 48.0, h: 48.0 });
        assert_eq!(cmds.iter().map(|c| c.layer).collect::<Vec<_>>(), [i32::MIN, FRONT_LAYER]);
        let count = |c: &DrawCmd| match &c.paint {
            Paint::Tiles { tiles, .. } => tiles.len(),
            _ => 0,
        };
        assert_eq!((count(&cmds[0]), count(&cmds[1])), (6, 1), "behind: wall and ground, in front: the leaf");
    }

    #[test]
    fn a_legend_that_hides_empty_tiles_is_an_error() {
        let sheet = Sprite::test_tiles();
        let bad = [Tile { ch: b'.', id: 1, solid: true, front: false, auto: false }];
        assert!(Tilemap::try_from_ascii_with(&["#"], 16, sheet.clone(), &bad).is_err());
        let zero = [Tile { ch: b'#', id: 0, solid: true, front: false, auto: false }];
        assert!(Tilemap::try_from_ascii_with(&["#"], 16, sheet, &zero).is_err());
    }

    #[test]
    fn a_tile_the_sheet_does_not_have_is_refused_instead_of_crashing_the_renderer() {
        let sheet = Sprite::test_tiles();
        let columns = sheet.size().0 / 16;
        let past = [Tile { ch: b'#', id: columns as u8 + 1, solid: true, front: false, auto: false }];
        let err = Tilemap::try_from_ascii_with(&["#"], 16, sheet.clone(), &past).err().unwrap();
        assert!(err.contains("the sheet has"), "{err}");
        let mut map = Tilemap::from_ascii(&["..", "##"], 16, sheet);
        map.set(0, 0, columns as u8);
        map.set(1, 0, 0);
        assert_eq!(map.tile(0, 0), columns as u8);
    }

    #[test]
    #[should_panic(expected = "does not exist")]
    fn setting_a_tile_past_the_sheet_panics_at_the_call_not_when_drawn() {
        let sheet = Sprite::test_tiles();
        let mut map = Tilemap::from_ascii(&["."], 16, sheet);
        map.set(0, 0, 200);
    }

    #[test]
    fn a_view_past_the_map_a_cell_outside_it_and_bad_sizes_do_not_panic() {
        let map = Tilemap::from_ascii(&["#.", "#."], 2, wide_sheet());
        assert!(map.draw_list(Rect { x: 1000.0, y: 1000.0, w: 64.0, h: 64.0 }).is_empty());
        assert_eq!((map.tile(2, 0), map.tile(0, 5)), (0, 0));
        assert!(Tilemap::try_from_ascii(&["#"], 0, wide_sheet()).is_err());
        let wide = Arc::new(Sprite::from_pixels(600, 2, vec![0; 1200]));
        let auto = Tile { ch: b'#', id: 250, solid: true, front: false, auto: true };
        assert!(Tilemap::try_from_ascii_with(&["#"], 2, wide, &[auto]).is_err());
    }
}
