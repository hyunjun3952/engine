use crate::{Gfx, Rect, Sprite};
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
        let w = rows[0].len();
        let mut tiles = Vec::with_capacity(w * rows.len());
        let mut marks = Vec::new();
        for (y, row) in rows.iter().enumerate() {
            assert_eq!(row.len(), w, "ragged tilemap row {y}");
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
        Tilemap { w, h: rows.len(), size, tiles, marks, sheet }
    }

    pub fn load(path: &str, size: usize, sheet: Arc<Sprite>) -> Self {
        let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let rows: Vec<&str> = text.trim_end().lines().collect();
        Self::from_ascii(&rows, size, sheet)
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

    pub fn draw(&self, g: &mut Gfx) {
        let (cx, cy) = (g.cam.0 as i32, g.cam.1 as i32);
        let s = self.size as i32;
        let (tx0, ty0) = ((cx / s).max(0), (cy / s).max(0));
        let (tx1, ty1) =
            (((cx + g.w as i32) / s + 1).min(self.w as i32), ((cy + g.h as i32) / s + 1).min(self.h as i32));
        for ty in ty0..ty1 {
            for tx in tx0..tx1 {
                let id = self.tiles[ty as usize * self.w + tx as usize] as usize;
                if id > 0 {
                    g.blit_region(
                        &self.sheet,
                        Rect { x: ((id - 1) * self.size) as f32, y: 0.0, w: self.size as f32, h: self.size as f32 },
                        tx * s - cx,
                        ty * s - cy,
                        false,
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_become_markers_with_pixel_positions() {
        let map = Tilemap::from_ascii(&["...", ".P.", "###"], 16, Arc::new(Sprite::load("assets/tiles.png")));
        assert_eq!(map.marks(b'P').collect::<Vec<_>>(), vec![(16.0, 16.0)]);
        assert_eq!(map.marks(b'E').count(), 0);
    }

    #[test]
    fn level_file_loads_with_a_start_marker() {
        let map = Tilemap::load("assets/level1.txt", 16, Arc::new(Sprite::load("assets/tiles.png")));
        assert_eq!((map.w, map.h), (60, 23));
        assert_eq!(map.marks(b'P').count(), 1);
    }

    #[test]
    fn falling_box_lands_on_floor_and_wall_stops_it() {
        let map = Tilemap::from_ascii(&["...", "...", "###"], 16, Arc::new(Sprite::load("assets/tiles.png")));
        let (r, hx, hy) = map.slide(&Rect { x: 0.0, y: 20.0, w: 10.0, h: 10.0 }, 0.0, 5.0);
        assert_eq!((r.y, hx, hy), (22.0, false, true));
        let (r, hx, _) = map.slide(&Rect { x: 1.0, y: 0.0, w: 10.0, h: 10.0 }, -3.0, 0.0);
        assert_eq!((r.x, hx), (0.0, true));
    }
}
