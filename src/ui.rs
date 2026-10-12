use crate::{Font, Gfx, Input, Rect};

pub fn draw_panel(gfx: &mut Gfx, r: &Rect, fill: u32, border: u32) {
    gfx.fill(r, fill);
    gfx.outline(r, border);
}

/// Fills `frac` (clamped to 0..=1) of `r` from the left over a `back` track.
pub fn draw_bar(gfx: &mut Gfx, r: &Rect, frac: f32, color: u32, back: u32) {
    gfx.fill(r, back);
    gfx.fill(&Rect { w: (r.w * frac.clamp(0.0, 1.0)).floor(), ..*r }, color);
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
