use super::{BOX, Shade, Theme, clicked, draw_panel, hovered};
use crate::{Font, Gfx, Input, Rect};

/// A labelled button: `update` once per step (true on the step it is clicked), then `draw`; the look follows the
/// cursor as of the last `update`.
///
/// ```
/// use engine::{Button, Input, Rect};
///
/// let mut start = Button::new(Rect::new(10.0, 10.0, 60.0, 16.0), "Start");
/// fn update(start: &mut Button, input: &Input) -> bool {
///     start.update(input)
/// }
/// # let _ = &mut start;
/// ```
pub struct Button {
    /// Where the button is.
    pub rect: Rect,
    /// What it says.
    pub label: String,
    pub(super) shade: Shade,
}

impl Button {
    /// A button with a label.
    pub fn new(rect: Rect, label: &str) -> Self {
        Button { rect, label: label.to_string(), shade: Shade::Idle }
    }

    /// True on the step it is clicked.
    pub fn update(&mut self, input: &Input) -> bool {
        self.shade = Shade::of(input, self.rect);
        clicked(input, self.rect)
    }

    /// The label is centred in the rectangle.
    pub fn draw(&self, gfx: &mut Gfx, font: &Font, theme: &Theme) {
        draw_panel(gfx, self.rect, self.shade.color(theme), theme.edge);
        let r = &self.rect;
        let (x, y) = (
            r.x as i32 + (r.w as i32 - font.width(&self.label)) / 2,
            r.y as i32 + (r.h as i32 - font.line_height()) / 2,
        );
        gfx.text_font(font, &self.label, x, y, theme.text);
    }
}

/// A box with a label to its right that flips when the box or the label is clicked. `rect` covers both.
///
/// ```
/// use engine::{Checkbox, Font, Input};
///
/// fn update(sound: &mut Checkbox, input: &Input) -> bool {
///     sound.update(input)
/// }
/// fn make(font: &Font) -> Checkbox {
///     Checkbox::new(font, 10, 10, "Sound", true)
/// }
/// ```
pub struct Checkbox {
    /// Whether the box is ticked.
    pub checked: bool,
    /// Covers the box and its label.
    pub rect: Rect,
    label: String,
    shade: Shade,
}

impl Checkbox {
    /// A box with a label to its right.
    pub fn new(font: &Font, x: i32, y: i32, label: &str, checked: bool) -> Self {
        let w = BOX + 4.0 + font.width(label) as f32;
        let rect = Rect::new(x as f32, y as f32, w, BOX.max(font.line_height() as f32));
        Checkbox { checked, rect, label: label.to_string(), shade: Shade::Idle }
    }

    /// Returns true on the step it flips.
    pub fn update(&mut self, input: &Input) -> bool {
        self.shade = if hovered(input, self.rect) { Shade::Hot } else { Shade::Idle };
        let flip = clicked(input, self.rect);
        self.checked ^= flip;
        flip
    }

    /// Draws the box and its label.
    pub fn draw(&self, gfx: &mut Gfx, font: &Font, theme: &Theme) {
        let r = Rect::new(self.rect.x, self.rect.y, BOX, BOX);
        draw_panel(gfx, r, self.shade.color(theme), theme.edge);
        if self.checked {
            gfx.fill(Rect::new(r.x + 3.0, r.y + 3.0, BOX - 6.0, BOX - 6.0), theme.text);
        }
        gfx.text_font(font, &self.label, r.x as i32 + BOX as i32 + 4, r.y as i32, theme.text);
    }
}
