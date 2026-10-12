use crate::{Ctx, Font, Gfx, Input, Key, MouseButton, Rect};

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
    input.mouse().is_some_and(|(x, y)| r.contains(x, y))
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

/// A single-line text box: call `update` once per fixed step with the typed characters and Backspace.
pub struct TextField {
    text: String,
    max: usize,
}

impl TextField {
    /// Holds at most `max` characters (not bytes).
    pub fn new(max: usize) -> Self {
        TextField { text: String::new(), max }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn clear(&mut self) {
        self.text.clear();
    }

    /// Returns true when Enter was pressed this step.
    pub fn update(&mut self, input: &Input) -> bool {
        if input.key_pressed(Key::Backspace) {
            self.text.pop();
        }
        for &c in input.typed() {
            if self.text.chars().count() < self.max {
                self.text.push(c);
            }
        }
        input.key_pressed(Key::Enter)
    }

    /// Draws the text and, while `caret` is true, a `|` after it; callers blink it with a `Timer`.
    pub fn draw(&self, gfx: &mut Gfx, font: &Font, x: i32, y: i32, color: u32, caret: bool) {
        gfx.text_font(font, &self.text, x, y, color);
        if caret {
            gfx.text_font(font, "|", x + font.width(&self.text), y, color);
        }
    }
}

// Row height and the width of the `> ` marker, shared by drawing and mouse hit tests so they cannot disagree.
fn row_metrics(font: &Font) -> (i32, i32) {
    (font.line_height() + 3, font.width("> "))
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

    pub fn set_item(&mut self, i: usize, text: String) {
        self.items[i] = text;
    }

    /// Moves the highlight (wrapping) on `up`/`down` presses and returns the chosen index on `accept`.
    pub fn update(&mut self, input: &Input, up: &str, down: &str, accept: &str) -> Option<usize> {
        if input.pressed(up) {
            self.select(-1);
        }
        if input.pressed(down) {
            self.select(1);
        }
        input.pressed(accept).then_some(self.sel)
    }

    /// Moves the highlight by `delta` rows, wrapping.
    pub fn select(&mut self, delta: isize) {
        self.sel = (self.sel as isize + delta).rem_euclid(self.items.len() as isize) as usize;
    }

    /// Hovering a row highlights it and a left click on it returns its index; `x` and `y` must match `draw`.
    pub fn mouse(&mut self, input: &Input, font: &Font, x: i32, y: i32) -> Option<usize> {
        let (step, indent) = row_metrics(font);
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
        let (step, indent) = row_metrics(font);
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

/// A key remapping screen: one row per bound action. Up and Down move and Enter on a row waits for a key and binds
/// it (keys of that action are replaced, pad buttons stay). These screen keys are fixed, not actions, so remapping
/// cannot lock the player out of it. Escape cancels the wait, so the caller should leave its own Escape handling
/// alone while `waiting()` is true, and save with `ctx.save_bindings` when `update` returns true.
pub struct Rebind {
    menu: Menu,
    actions: Vec<String>,
    waiting: bool,
}

impl Rebind {
    /// Panics when no action is bound, like `Menu::new` without rows.
    pub fn new(input: &Input) -> Self {
        let actions: Vec<String> = input.actions().into_iter().map(String::from).collect();
        assert!(!actions.is_empty(), "bind actions before opening a Rebind screen");
        let mut screen = Rebind {
            menu: Menu::new(&actions.iter().map(String::as_str).collect::<Vec<_>>()),
            actions,
            waiting: false,
        };
        screen.show(input);
        screen
    }

    pub fn waiting(&self) -> bool {
        self.waiting
    }

    fn show(&mut self, input: &Input) {
        let keys = |a: &String| -> Vec<String> {
            input.describe(a).split_whitespace().filter(|w| !w.starts_with("pad.")).map(String::from).collect()
        };
        let all: Vec<Vec<String>> = self.actions.iter().map(keys).collect();
        for (i, action) in self.actions.iter().enumerate() {
            let text =
                if self.waiting && i == self.menu.selected() { "press a key".into() } else { input.describe(action) };
            // Sharing a key is allowed (a menu and a game can use Enter for different things); it is only pointed out.
            let clash = (0..all.len()).find(|&j| j != i && all[j].iter().any(|k| all[i].contains(k)));
            let note = clash.map_or(String::new(), |j| format!("  (also {})", self.actions[j]));
            self.menu.set_item(i, format!("{action}: {text}{note}"));
        }
    }

    /// Returns true on the step a binding changed.
    pub fn update(&mut self, ctx: &mut Ctx) -> bool {
        let mut changed = false;
        if self.waiting {
            if ctx.input().key_pressed(Key::Escape) {
                self.waiting = false;
            } else if let Some(key) = ctx.input().key_just_pressed() {
                ctx.bind(&self.actions[self.menu.selected()], &[key]);
                (self.waiting, changed) = (false, true);
            }
        } else {
            let input = ctx.input();
            self.menu.select(input.key_pressed(Key::Down) as isize - input.key_pressed(Key::Up) as isize);
            self.waiting = input.key_pressed(Key::Enter);
        }
        self.show(ctx.input());
        changed
    }

    pub fn draw(&self, gfx: &mut Gfx, font: &Font, x: i32, y: i32, color: u32, active: u32) {
        self.menu.draw(gfx, font, x, y, color, active);
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
    fn a_text_field_takes_typed_characters_up_to_its_limit_and_backspace_removes_one() {
        let (mut input, mut field) = (Input::default(), TextField::new(3));
        input.type_chars("가a나다");
        assert!(!field.update(&input));
        assert_eq!(field.text(), "가a나");
        press(&mut input, Key::Backspace);
        field.update(&input);
        assert_eq!(field.text(), "가a");
        press(&mut input, Key::Enter);
        assert!(field.update(&input));
        field.clear();
        assert_eq!(field.text(), "");
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

    #[test]
    fn a_rebind_row_waits_for_a_key_binds_it_and_escape_cancels() {
        let mut ctx = Ctx::new(64, 64, 1.0 / 60.0, crate::Audio::disabled());
        ctx.bind("jump", &[Key::Space]);
        let mut screen = Rebind::new(ctx.input());
        let step = |ctx: &mut Ctx, screen: &mut Rebind, key: Option<Key>| {
            ctx.input.end_step();
            ctx.input.hold(key.as_slice());
            screen.update(ctx)
        };
        assert!(!step(&mut ctx, &mut screen, Some(Key::Enter)) && screen.waiting(), "Accept on `jump` starts waiting");
        assert!(!step(&mut ctx, &mut screen, Some(Key::Enter)), "the held Accept key is not the new binding");
        assert!(step(&mut ctx, &mut screen, Some(Key::K)) && !screen.waiting());
        assert_eq!(ctx.input().describe("jump"), "K");
        step(&mut ctx, &mut screen, None);
        step(&mut ctx, &mut screen, Some(Key::Enter));
        assert!(screen.waiting());
        assert!(!step(&mut ctx, &mut screen, Some(Key::Escape)) && !screen.waiting());
        assert_eq!(ctx.input().describe("jump"), "K", "cancelled, so unchanged");
    }

    #[test]
    fn a_row_says_which_other_action_shares_one_of_its_keys() {
        let mut ctx = Ctx::new(64, 64, 1.0 / 60.0, crate::Audio::disabled());
        ctx.bind("jump", &[Key::Space]);
        ctx.bind("left", &[Key::Left, Key::A]);
        ctx.bind("fire", &[Key::Z]);
        let mut screen = Rebind::new(ctx.input());
        assert!(screen.menu.items.iter().all(|row| !row.contains("also")), "{:?}", screen.menu.items);
        ctx.bind("jump", &[Key::A]);
        screen.show(ctx.input());
        let rows = &screen.menu.items;
        assert_eq!((rows[0].as_str(), rows[1].as_str()), ("fire: Z", "jump: A  (also left)"), "{rows:?}");
        assert_eq!(rows[2], "left: Left A  (also jump)");
    }
}
