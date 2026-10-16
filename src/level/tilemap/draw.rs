use super::{AUTO, DrawCmd, FRONT, FRONT_LAYER, Paint, Rect, Tilemap};

impl Tilemap {
    // Which neighbours of an autotile cell are the same tile: up 1, right 2, down 4, left 8; outside the map counts.
    pub(super) fn joins(&self, tx: usize, ty: usize, id: u8) -> u8 {
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

    /// The commands that draw the tiles in `view`, behind and in front of entities.
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
