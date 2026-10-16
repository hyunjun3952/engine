mod button;
mod hangul;
mod layout;
mod menu;
#[cfg(test)]
mod tests;
mod text;

pub use button::{Button, Checkbox};
pub use layout::{Layout, grid};
pub use menu::{Focus, Rebind, ScrollList};
pub use text::{TextField, draw_wrapped, wrap};

use crate::{Font, Gfx, Input, MouseButton, Rect, Vec2};

/// ```
/// use engine::{Gfx, Rect, draw_panel};
///
/// fn draw(gfx: &mut Gfx) {
///     draw_panel(gfx, Rect::new(10.0, 10.0, 100.0, 40.0), 0x303048, 0x8080a0);
/// }
/// ```
pub fn draw_panel(gfx: &mut Gfx, r: Rect, fill: u32, border: u32) {
    gfx.fill(r, fill);
    gfx.outline(r, border);
}

/// Fills `frac` (clamped to 0..=1) of `r` from the left over a `back` track.
///
/// ```
/// use engine::{Gfx, Rect, draw_bar};
///
/// fn draw(gfx: &mut Gfx, hp: f32) {
///     draw_bar(gfx, Rect::new(10.0, 4.0, 60.0, 6.0), hp, 0xff4040, 0x401010);
/// }
/// ```
pub fn draw_bar(gfx: &mut Gfx, r: Rect, frac: f32, color: u32, back: u32) {
    gfx.fill(r, back);
    gfx.fill(Rect { w: (r.w * frac.clamp(0.0, 1.0)).floor(), ..r }, color);
}

/// ```
/// use engine::{Input, Rect, hovered};
///
/// fn over(input: &Input) -> bool {
///     hovered(input, Rect::new(10.0, 10.0, 40.0, 16.0))
/// }
/// ```
pub fn hovered(input: &Input, r: Rect) -> bool {
    input.mouse().is_some_and(|p| r.contains(p))
}

/// True on the step the left button goes down over `r`.
///
/// ```
/// use engine::{Input, Rect, clicked};
///
/// fn press(input: &Input) -> bool {
///     clicked(input, Rect::new(10.0, 10.0, 40.0, 16.0))
/// }
/// ```
pub fn clicked(input: &Input, r: Rect) -> bool {
    input.mouse_pressed(MouseButton::Left) && hovered(input, r)
}

/// While the left button is held over `r`, the value (0..=1) under the cursor (first pixel 0, last pixel 1); otherwise `value` unchanged.
/// Dragging off the rectangle stops it following, so make the rectangle taller than the drawn track.
///
/// ```
/// use engine::{Input, Rect, slider};
///
/// fn volume(input: &Input, value: f32) -> f32 {
///     slider(input, Rect::new(10.0, 10.0, 80.0, 8.0), value)
/// }
/// ```
pub fn slider(input: &Input, r: Rect, value: f32) -> f32 {
    match input.mouse() {
        Some(Vec2 { x, .. }) if input.mouse_down(MouseButton::Left) && hovered(input, r) => {
            ((x.floor() - r.x) / (r.w - 1.0).max(1.0)).clamp(0.0, 1.0)
        }
        _ => value,
    }
}

const BOX: f32 = 10.0;

/// The colours every widget draws with; `Theme::default()` is the engine's own look.
///
/// ```
/// use engine::Theme;
///
/// let mut theme = Theme::default();
/// theme.active = 0x40ff80;
/// assert_ne!(theme, Theme::DEFAULT);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    /// Labels, typed text and the rows that are not highlighted.
    pub text: u32,
    /// The highlighted row of a list and the focus outline.
    pub active: u32,
    /// The colour of a box or button.
    pub fill: u32,
    /// A button or box under the cursor.
    pub hot: u32,
    /// A button held down.
    pub down: u32,
    /// The colour of borders.
    pub edge: u32,
}

impl Theme {
    /// The engine's own colours.
    pub const DEFAULT: Theme =
        Theme { text: 0xffffff, active: 0xffd040, fill: 0x303048, hot: 0x454568, down: 0x202030, edge: 0x8080a0 };
}

impl Default for Theme {
    fn default() -> Self {
        Theme::DEFAULT
    }
}

#[derive(Clone, Copy, Default)]
enum Shade {
    #[default]
    Idle,
    Hot,
    Down,
}

impl Shade {
    fn of(input: &Input, r: Rect) -> Shade {
        match (hovered(input, r), input.mouse_down(MouseButton::Left)) {
            (true, true) => Shade::Down,
            (true, false) => Shade::Hot,
            _ => Shade::Idle,
        }
    }

    fn color(self, theme: &Theme) -> u32 {
        match self {
            Shade::Idle => theme.fill,
            Shade::Hot => theme.hot,
            Shade::Down => theme.down,
        }
    }
}

// Row height and the width of the `> ` marker, shared by drawing and mouse hit tests so they cannot disagree.
fn row_metrics(font: &Font) -> (i32, i32) {
    (font.line_height() + 3, font.width("> "))
}
