use crate::{DrawCmd, Paint, Rect, Sprite};
use std::sync::Arc;

pub struct Tilemap {
    pub w: usize,
    pub h: usize,
    pub size: usize,
    tiles: Vec<u8>,
    marks: Vec<(u8, usize, usize)>,
    sheet: Arc<Sprite>,
}

impl Tilemap {
    pub fn from_ascii(rows: &[&str], size: usize, sheet: Arc<Sprite>) -> Self {
        Tilemap::try_from_ascii(rows, size, sheet).unwrap_or_else(|e| panic!("{e}"))
    }

    pub fn try_from_ascii(rows: &[&str], size: usize, sheet: Arc<Sprite>) -> Result<Self, String> {
        let w = rows.first().ok_or("empty tilemap")?.len();
        let mut tiles = Vec::with_capacity(w * rows.len());
        let mut marks = Vec::new();
        for (y, row) in rows.iter().enumerate() {
            if row.len() != w {
                return Err(format!("ragged tilemap row {} (width {}, expected {w})", y + 1, row.len()));
            }
            for (x, c) in row.bytes().enumerate() {
                tiles.push(match c {
                    b'#' => 1,
                    b'+' => 2,
                    _ => 0,
                });
                if !matches!(c, b'.' | b' ' | b'#' | b'+') {
                    marks.push((c, x, y));
                }
            }
        }
        Ok(Tilemap { w, h: rows.len(), size, tiles, marks, sheet })
    }

    pub fn load(path: &str, size: usize, sheet: Arc<Sprite>) -> Self {
        Tilemap::try_load(path, size, sheet).unwrap_or_else(|e| panic!("{e}"))
    }

    pub fn try_load(path: &str, size: usize, sheet: Arc<Sprite>) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
        let rows: Vec<&str> = text.trim_end().lines().collect();
        Tilemap::try_from_ascii(&rows, size, sheet).map_err(|e| format!("{path}: {e}"))
    }

    pub fn marks(&self, c: u8) -> impl Iterator<Item = (f32, f32)> + '_ {
        self.marks.iter().filter(move |m| m.0 == c).map(|&(_, x, y)| ((x * self.size) as f32, (y * self.size) as f32))
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
        self.tiles[ty as usize * self.w + tx as usize] != 0
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

    pub fn draw_list(&self, view: Rect) -> Vec<DrawCmd> {
        let s = self.size as f32;
        let (tx0, ty0) = ((view.x / s).floor().max(0.0) as usize, (view.y / s).floor().max(0.0) as usize);
        let tx1 = (((view.x + view.w) / s).ceil().max(0.0) as usize).min(self.w);
        let ty1 = (((view.y + view.h) / s).ceil().max(0.0) as usize).min(self.h);
        let mut tiles = Vec::new();
        for ty in ty0..ty1 {
            for tx in tx0..tx1 {
                let id = self.tiles[ty * self.w + tx];
                if id > 0 {
                    tiles.push(((tx - tx0) as u32, (ty - ty0) as u32, id));
                }
            }
        }
        if tiles.is_empty() {
            return Vec::new();
        }
        let rect = Rect { x: tx0 as f32 * s, y: ty0 as f32 * s, w: (tx1 - tx0) as f32 * s, h: (ty1 - ty0) as f32 * s };
        let paint = Paint::Tiles { sheet: self.sheet.clone(), size: self.size as u32, tiles };
        vec![DrawCmd { layer: i32::MIN, ..DrawCmd::new(rect, paint) }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_become_markers_with_pixel_positions() {
        let map = Tilemap::from_ascii(&["...", ".P.", "###"], 16, Arc::new(Sprite::load("assets/images/tiles.png")));
        assert_eq!(map.marks(b'P').collect::<Vec<_>>(), vec![(16.0, 16.0)]);
        assert_eq!(map.marks(b'E').count(), 0);
    }

    #[test]
    fn a_broken_level_is_an_error_not_a_crash() {
        let sheet = Arc::new(Sprite::load("assets/images/tiles.png"));
        let err = Tilemap::try_from_ascii(&["###", "##"], 16, sheet.clone()).err().unwrap();
        assert!(err.contains("row 2"), "{err}");
        assert!(Tilemap::try_from_ascii(&[], 16, sheet.clone()).is_err());
        let err = Tilemap::try_load("assets/nope.txt", 16, sheet).err().unwrap();
        assert!(err.starts_with("assets/nope.txt"), "{err}");
    }

    #[test]
    fn level_file_loads_with_a_start_marker() {
        let map = Tilemap::load("assets/levels/level1.txt", 16, Arc::new(Sprite::load("assets/images/tiles.png")));
        assert_eq!((map.w, map.h), (60, 23));
        assert_eq!(map.marks(b'P').count(), 1);
    }

    #[test]
    fn draw_list_covers_only_the_visible_solid_tiles_in_world_space() {
        let map = Tilemap::from_ascii(&["#.#", "...", "..."], 16, Arc::new(Sprite::load("assets/images/tiles.png")));
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
        let map = Tilemap::from_ascii(&["...", "...", "###"], 16, Arc::new(Sprite::load("assets/images/tiles.png")));
        let (r, hx, hy) = map.slide(&Rect { x: 0.0, y: 20.0, w: 10.0, h: 10.0 }, 0.0, 5.0);
        assert_eq!((r.y, hx, hy), (22.0, false, true));
        let (r, hx, _) = map.slide(&Rect { x: 1.0, y: 0.0, w: 10.0, h: 10.0 }, -3.0, 0.0);
        assert_eq!((r.x, hx), (0.0, true));
    }
}
