use crate::{Font, Gfx, Input, MouseButton, Rect};

pub fn draw_panel(gfx: &mut Gfx, r: &Rect, fill: u32, border: u32) {
    gfx.fill(r, fill);
    gfx.outline(r, border);
}

/// Fills `frac` (clamped to 0..=1) of `r` from the left over a `back` track.
pub fn draw_bar(gfx: &mut Gfx, r: &Rect, frac: f32, color: u32, back: u32) {
    gfx.fill(r, back);
    gfx.fill(&Rect { w: (r.w * frac.clamp(0.0, 1.0)).floor(), ..*r }, color);
}

pub fn hovered(input: &Input, r: &Rect) -> bool {
    input.mouse().is_some_and(|(x, y)| x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h)
}

/// True on the step the left button goes down over `r`.
pub fn clicked(input: &Input, r: &Rect) -> bool {
    input.mouse_pressed(MouseButton::Left) && hovered(input, r)
}

/// While the left button is held over `r`, the value (0..=1) under the cursor (first pixel 0, last pixel 1); otherwise `value` unchanged.
/// Dragging off the rectangle stops it following, so make the rectangle taller than the drawn track.
pub fn slider(input: &Input, r: &Rect, value: f32) -> f32 {
    match input.mouse() {
        Some((x, _)) if input.mouse_down(MouseButton::Left) && hovered(input, r) => {
            ((x.floor() - r.x) / (r.w - 1.0).max(1.0)).clamp(0.0, 1.0)
        }
        _ => value,
    }
}

pub struct Menu {
    items: Vec<String>,
    sel: usize,
}

impl Menu {
    pub fn new(items: &[&str]) -> Self {
        assert!(!items.is_empty(), "a menu needs at least one item");
        Menu { items: items.iter().map(|s| s.to_string()).collect(), sel: 0 }
    }

    pub fn selected(&self) -> usize {
        self.sel
    }

    /// Moves the highlight (wrapping) on `up`/`down` presses and returns the chosen index on `accept`.
    pub fn update(&mut self, input: &Input, up: &str, down: &str, accept: &str) -> Option<usize> {
        let n = self.items.len();
        if input.pressed(up) {
            self.sel = (self.sel + n - 1) % n;
        }
        if input.pressed(down) {
            self.sel = (self.sel + 1) % n;
        }
        input.pressed(accept).then_some(self.sel)
    }

    /// Hovering a row highlights it and a left click on it returns its index; `x` and `y` must match `draw`.
    pub fn mouse(&mut self, input: &Input, font: &Font, x: i32, y: i32) -> Option<usize> {
        let step = font.line_height() + 3;
        let indent = font.width("> ");
        let row = |i: usize, item: &str| Rect {
            x: x as f32,
            y: (y + i as i32 * step) as f32,
            w: (indent + font.width(item)) as f32,
            h: step as f32,
        };
        let over = self.items.iter().enumerate().position(|(i, item)| hovered(input, &row(i, item)))?;
        if input.mouse_moved() {
            self.sel = over;
        }
        clicked(input, &row(over, &self.items[over])).then_some(over)
    }

    pub fn draw(&self, gfx: &mut Gfx, font: &Font, x: i32, y: i32, color: u32, active: u32) {
        let step = font.line_height() + 3;
        let indent = font.width("> ");
        for (i, item) in self.items.iter().enumerate() {
            let row = y + i as i32 * step;
            if i == self.sel {
                gfx.text_font(font, ">", x, row, active);
                gfx.text_font(font, item, x + indent, row, active);
            } else {
                gfx.text_font(font, item, x + indent, row, color);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Key;

    fn press(input: &mut Input, key: Key) {
        input.hold(&[]);
        input.end_step();
        input.hold(&[key]);
    }

    fn menu_input() -> Input {
        let mut i = Input::default();
        i.bind("up", &[Key::Up]);
        i.bind("down", &[Key::Down]);
        i.bind("ok", &[Key::Enter]);
        i
    }

    #[test]
    fn the_highlight_wraps_and_accept_returns_it_once() {
        let (mut input, mut menu) = (menu_input(), Menu::new(&["a", "b", "c"]));
        press(&mut input, Key::Up);
        assert_eq!(menu.update(&input, "up", "down", "ok"), None);
        assert_eq!(menu.selected(), 2);
        press(&mut input, Key::Down);
        menu.update(&input, "up", "down", "ok");
        assert_eq!(menu.selected(), 0);
        press(&mut input, Key::Down);
        menu.update(&input, "up", "down", "ok");
        press(&mut input, Key::Enter);
        assert_eq!(menu.update(&input, "up", "down", "ok"), Some(1));
        input.end_step();
        assert_eq!(menu.update(&input, "up", "down", "ok"), None, "held is not pressed again");
    }

    fn at(input: &mut Input, x: f32, y: f32, left: bool) {
        input.end_step();
        input.point(Some((x, y)), left);
    }

    #[test]
    fn clicks_and_sliders_follow_the_cursor() {
        let (mut input, r) = (Input::default(), Rect { x: 10.0, y: 10.0, w: 100.0, h: 8.0 });
        at(&mut input, 20.0, 12.0, false);
        assert!(hovered(&input, &r) && !clicked(&input, &r));
        at(&mut input, 20.0, 12.0, true);
        assert!(clicked(&input, &r));
        assert!((slider(&input, &r, 0.9) - 10.0 / 99.0).abs() < 1e-6);
        at(&mut input, 109.9, 12.0, true);
        assert_eq!(slider(&input, &r, 0.0), 1.0, "the last pixel is the full value");
        at(&mut input, 10.2, 12.0, true);
        assert_eq!(slider(&input, &r, 1.0), 0.0);
        at(&mut input, 20.0, 12.0, true);
        at(&mut input, 20.0, 12.0, true);
        assert!(!clicked(&input, &r), "holding is not a new click");
        at(&mut input, 500.0, 12.0, true);
        assert_eq!(slider(&input, &r, 0.9), 0.9, "outside leaves the value alone");
        at(&mut input, 20.0, 40.0, true);
        assert!(!hovered(&input, &r));
    }

    #[test]
    fn a_menu_row_highlights_on_hover_and_returns_on_click_but_ignores_a_resting_cursor() {
        let font = Font::load("assets/fonts/galmuri9.bdf").unwrap();
        let (mut input, mut menu) = (menu_input(), Menu::new(&["시작", "끝"]));
        let second_row = font.line_height() as f32 + 3.0 + 2.0;
        at(&mut input, 4.0, second_row, false);
        assert_eq!(menu.mouse(&input, &font, 0, 0), None);
        assert_eq!(menu.selected(), 1);
        press(&mut input, Key::Up);
        menu.update(&input, "up", "down", "ok");
        assert_eq!(menu.selected(), 0);
        at(&mut input, 4.0, second_row, false);
        menu.mouse(&input, &font, 0, 0);
        assert_eq!(menu.selected(), 0, "a cursor that did not move leaves the keyboard choice");
        at(&mut input, 5.0, second_row, true);
        assert_eq!(menu.mouse(&input, &font, 0, 0), Some(1));
    }

    #[test]
    fn bar_clamps_and_menu_draws_one_command_per_glyph() {
        let mut g = Gfx::new(100, 50);
        draw_bar(&mut g, &Rect { x: 0.0, y: 0.0, w: 50.0, h: 4.0 }, 7.0, 1, 2);
        assert_eq!(g.cmds().len(), 2);
        let font = Font::load("assets/fonts/galmuri9.bdf").unwrap();
        let mut g = Gfx::new(100, 50);
        Menu::new(&["시작", "끝"]).draw(&mut g, &font, 0, 0, 1, 2);
        assert_eq!(g.cmds().len(), 1 + 2 + 1);
    }
}
