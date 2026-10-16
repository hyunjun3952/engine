use crate::Rect;

/// Splits an area into consecutive rectangles along one axis, `gap` pixels apart, each as wide (or tall) as the
/// area is across.
///
/// ```
/// use engine::{Layout, Rect};
///
/// let mut column = Layout::column(Rect::new(0.0, 0.0, 100.0, 100.0), 4.0);
/// assert_eq!(column.next(20.0), Rect::new(0.0, 0.0, 100.0, 20.0));
/// assert_eq!(column.next(20.0), Rect::new(0.0, 24.0, 100.0, 20.0));
/// assert_eq!(column.rest().y, 48.0);
/// ```
pub struct Layout {
    area: Rect,
    vertical: bool,
    gap: f32,
    used: f32,
}

impl Layout {
    /// Rectangles stacked from the top.
    pub fn column(area: Rect, gap: f32) -> Self {
        Layout { area, vertical: true, gap, used: 0.0 }
    }

    /// Rectangles laid from the left.
    pub fn row(area: Rect, gap: f32) -> Self {
        Layout { area, vertical: false, gap, used: 0.0 }
    }

    /// The next `size` along the axis, cut short by what is left (and empty once nothing is).
    pub fn next(&mut self, size: f32) -> Rect {
        let along = if self.vertical { self.area.h } else { self.area.w };
        let size = size.min((along - self.used).max(0.0)).max(0.0);
        let r = if self.vertical {
            Rect { y: self.area.y + self.used, h: size, ..self.area }
        } else {
            Rect { x: self.area.x + self.used, w: size, ..self.area }
        };
        self.used += size + self.gap;
        r
    }

    /// Whatever is left.
    pub fn rest(&mut self) -> Rect {
        let along = if self.vertical { self.area.h } else { self.area.w };
        self.next(along - self.used)
    }
}

/// `cols` by `rows` equal cells filling `area`, left to right and then down, `gap` apart.
///
/// ```
/// use engine::{Rect, grid};
///
/// let cells = grid(Rect::new(0.0, 0.0, 100.0, 100.0), 2, 2, 0.0);
/// assert_eq!(cells.len(), 4);
/// assert_eq!(cells[3], Rect::new(50.0, 50.0, 50.0, 50.0));
/// ```
pub fn grid(area: Rect, cols: usize, rows: usize, gap: f32) -> Vec<Rect> {
    let (cols, rows) = (cols.max(1), rows.max(1));
    let w = (area.w - gap * (cols - 1) as f32) / cols as f32;
    let h = (area.h - gap * (rows - 1) as f32) / rows as f32;
    (0..cols * rows)
        .map(|i| Rect { x: area.x + (i % cols) as f32 * (w + gap), y: area.y + (i / cols) as f32 * (h + gap), w, h })
        .collect()
}
