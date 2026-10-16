use super::{Theme, clicked, hovered, row_metrics};
use crate::{Ctx, Font, Gfx, Input, Key, MouseButton, Rect, Vec2};

/// A column of rows from `x`, `y`, one of them highlighted.
///
/// ```
/// use engine::Menu;
///
/// let mut menu = Menu::new(10, 10, &["Start", "Options", "Quit"]);
/// menu.select(1);
/// assert_eq!(menu.selected(), 1);
/// ```
pub struct Menu {
    /// Left edge.
    pub x: i32,
    /// Top edge.
    pub y: i32,
    pub(super) items: Vec<String>,
    sel: usize,
}

impl Menu {
    /// A menu of `items`, the first highlighted.
    pub fn new(x: i32, y: i32, items: &[&str]) -> Self {
        assert!(!items.is_empty(), "a menu needs at least one item");
        Menu { x, y, items: items.iter().map(std::string::ToString::to_string).collect(), sel: 0 }
    }

    /// The highlighted row.
    pub fn selected(&self) -> usize {
        self.sel
    }

    /// Replaces the text of row `i`.
    pub fn set_item(&mut self, i: usize, text: String) {
        self.items[i] = text;
    }

    /// `up`/`down` presses move the highlight (wrapping) and moving the mouse over a row highlights it; returns the
    /// chosen index on `accept` or a left click on a row.
    pub fn update(&mut self, input: &Input, font: &Font, up: &str, down: &str, accept: &str) -> Option<usize> {
        if input.pressed(up) {
            self.select(-1);
        }
        if input.pressed(down) {
            self.select(1);
        }
        let over = (0..self.items.len()).find(|&i| hovered(input, self.row(font, i)));
        if let Some(over) = over {
            if input.mouse_moved() {
                self.sel = over;
            }
            if input.mouse_pressed(MouseButton::Left) {
                self.sel = over;
                return Some(over);
            }
        }
        input.pressed(accept).then_some(self.sel)
    }

    /// Moves the highlight by `delta` rows, wrapping.
    pub fn select(&mut self, delta: isize) {
        self.sel = (self.sel as isize + delta).rem_euclid(self.items.len() as isize) as usize;
    }

    fn row(&self, font: &Font, i: usize) -> Rect {
        let (step, indent) = row_metrics(font);
        let w = (indent + font.width(&self.items[i])) as f32;
        Rect::new(self.x as f32, (self.y + i as i32 * step) as f32, w, step as f32)
    }

    /// Draws the rows with the highlighted one marked.
    pub fn draw(&self, gfx: &mut Gfx, font: &Font, theme: &Theme) {
        let (step, indent) = row_metrics(font);
        for (i, item) in self.items.iter().enumerate() {
            let row = self.y + i as i32 * step;
            if i == self.sel {
                gfx.text_font(font, ">", self.x, row, theme.active);
                gfx.text_font(font, item, self.x + indent, row, theme.active);
            } else {
                gfx.text_font(font, item, self.x + indent, row, theme.text);
            }
        }
    }
}

/// A list too long for its rectangle: arrow keys (as actions) move the highlight, the wheel scrolls, a click picks.
///
/// ```
/// use engine::{Rect, ScrollList};
///
/// let list = ScrollList::new(Rect::new(10.0, 10.0, 100.0, 48.0), &["a", "b", "c"]);
/// assert_eq!(list.len(), 3);
/// assert_eq!(list.selected(), 0);
/// ```
pub struct ScrollList {
    /// The area the list fills.
    pub rect: Rect,
    items: Vec<String>,
    sel: usize,
    pub(super) top: usize,
}

impl ScrollList {
    /// A list of `items` in `rect`, the first highlighted.
    pub fn new(rect: Rect, items: &[&str]) -> Self {
        ScrollList { rect, items: items.iter().map(std::string::ToString::to_string).collect(), sel: 0, top: 0 }
    }

    /// The highlighted row.
    pub fn selected(&self) -> usize {
        self.sel
    }

    /// How many rows there are.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// True when there are no rows.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    fn rows(&self, font: &Font) -> usize {
        ((self.rect.h as i32) / row_metrics(font).0).max(1) as usize
    }

    // Keeps the highlight and the window on the list.
    fn clamp(&mut self, rows: usize) {
        self.sel = self.sel.min(self.items.len().saturating_sub(1));
        self.top = self.top.min(self.items.len().saturating_sub(rows));
    }

    // Also brings the highlighted row into the window.
    fn settle(&mut self, rows: usize) {
        self.clamp(rows);
        if self.sel < self.top {
            self.top = self.sel;
        } else if self.sel >= self.top + rows {
            self.top = self.sel + 1 - rows;
        }
    }

    /// Moves the highlight by `delta` rows (not wrapping), keeping it in view.
    pub fn select(&mut self, delta: isize, font: &Font) {
        self.sel = (self.sel as isize + delta).clamp(0, self.items.len().saturating_sub(1) as isize) as usize;
        self.settle(self.rows(font));
    }

    /// `up`/`down` move the highlight and bring it into view, the wheel over `rect` scrolls three rows a notch (the
    /// highlight may leave the view), a click on a row highlights it and returns its index, as does `accept` for the
    /// highlighted row.
    pub fn update(&mut self, input: &Input, font: &Font, up: &str, down: &str, accept: &str) -> Option<usize> {
        let (r, rows, step) = (self.rect, self.rows(font), row_metrics(font).0);
        if input.pressed(up) {
            self.select(-1, font);
        }
        if input.pressed(down) {
            self.select(1, font);
        }
        if hovered(input, r) && input.wheel() != 0.0 {
            let max = self.items.len().saturating_sub(rows) as isize;
            self.top = (self.top as isize - 3 * input.wheel().signum() as isize).clamp(0, max) as usize;
        }
        self.clamp(rows);
        if let Some(Vec2 { y, .. }) = input.mouse().filter(|_| clicked(input, r)) {
            let row = self.top + ((y - r.y) as i32 / step) as usize;
            if row < self.items.len() {
                self.sel = row;
                return Some(row);
            }
        }
        input.pressed(accept).then_some(self.sel).filter(|_| !self.items.is_empty())
    }

    /// Draws the rows that fit with the highlighted one marked.
    pub fn draw(&self, gfx: &mut Gfx, font: &Font, theme: &Theme) {
        let (r, rows, (step, indent)) = (self.rect, self.rows(font), row_metrics(font));
        for (n, item) in self.items.iter().enumerate().skip(self.top).take(rows) {
            let y = r.y as i32 + (n - self.top) as i32 * step;
            let (mark, shade) = if n == self.sel { (">", theme.active) } else { (" ", theme.text) };
            gfx.text_font(font, mark, r.x as i32, y, shade);
            gfx.text_font(font, item, r.x as i32 + indent, y, shade);
        }
        if self.items.len() > rows {
            let (track, total) = (r.h, self.items.len() as f32);
            let thumb = Rect {
                x: r.x + r.w - 2.0,
                y: r.y + track * self.top as f32 / total,
                w: 2.0,
                h: (track * rows as f32 / total).max(3.0),
            };
            gfx.fill(thumb, theme.edge);
        }
    }
}

/// Which of several widgets has the keyboard or pad focus: arrows (as actions) jump to the nearest rectangle in
/// that direction, hovering one with the mouse takes the focus, and accept (or a click) picks it.
///
/// ```
/// use engine::{Focus, Input, Rect};
///
/// fn update(focus: &mut Focus, input: &Input, buttons: &[Rect]) -> Option<usize> {
///     focus.update(input, buttons, ["up", "down", "left", "right"], "accept")
/// }
/// ```
#[derive(Default)]
pub struct Focus {
    index: usize,
}

impl Focus {
    /// Focus starts on the first rectangle.
    pub fn new() -> Self {
        Focus::default()
    }

    /// The index of the focused rectangle.
    pub fn selected(&self) -> usize {
        self.index
    }

    /// `dirs` are the actions for left, right, up and down. Returns the focused index on accept, or on a click.
    pub fn update(&mut self, input: &Input, rects: &[Rect], dirs: [&str; 4], accept: &str) -> Option<usize> {
        if rects.is_empty() {
            return None;
        }
        self.index = self.index.min(rects.len() - 1);
        let steps = [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)];
        for (action, dir) in dirs.iter().zip(steps) {
            if input.pressed(action) {
                self.index = Self::nearest(rects, self.index, dir).unwrap_or(self.index);
            }
        }
        if input.mouse_moved()
            && let Some(over) = rects.iter().position(|r| hovered(input, *r))
        {
            self.index = over;
        }
        // The rect under the click, not the focused one: keys may have moved the focus while the cursor sat still.
        let click = input.mouse().and_then(|_| rects.iter().position(|r| clicked(input, *r)));
        self.index = click.unwrap_or(self.index);
        (input.pressed(accept) || click.is_some()).then_some(self.index)
    }

    // The rectangle ahead in `dir` whose centre is closest, counting sideways drift double so a neighbour in line wins.
    fn nearest(rects: &[Rect], from: usize, dir: (f32, f32)) -> Option<usize> {
        let Vec2 { x: cx, y: cy } = rects[from].center();
        rects
            .iter()
            .enumerate()
            .filter(|&(i, _)| i != from)
            .filter_map(|(i, r)| {
                let Vec2 { x, y } = r.center();
                let (ahead, aside) = ((x - cx) * dir.0 + (y - cy) * dir.1, (x - cx) * dir.1 + (y - cy) * dir.0);
                (ahead > 0.5).then_some((i, ahead + 2.0 * aside.abs()))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }

    /// An outline around the focused rectangle, two pixels out.
    pub fn draw(&self, gfx: &mut Gfx, rects: &[Rect], theme: &Theme) {
        if let Some(r) = rects.get(self.index) {
            gfx.outline(Rect::new(r.x - 2.0, r.y - 2.0, r.w + 4.0, r.h + 4.0), theme.active);
        }
    }
}

/// A key remapping screen with fixed Up/Down/Enter/Escape so it cannot lock the player out; `update` is true on a change.
/// It takes the whole `Ctx` because it changes the bindings.
///
/// ```
/// use engine::{Ctx, Rebind};
///
/// fn update(rebind: &mut Rebind, ctx: &mut Ctx) -> bool {
///     rebind.update(ctx)
/// }
/// ```
pub struct Rebind {
    pub(super) menu: Menu,
    actions: Vec<String>,
    waiting: bool,
}

impl Rebind {
    /// Panics when no action is bound, like `Menu::new` without rows.
    pub fn new(input: &Input, x: i32, y: i32) -> Self {
        let actions: Vec<String> = input.actions().into_iter().map(String::from).collect();
        assert!(!actions.is_empty(), "bind actions before opening a Rebind screen");
        let mut screen = Rebind {
            menu: Menu::new(x, y, &actions.iter().map(String::as_str).collect::<Vec<_>>()),
            actions,
            waiting: false,
        };
        screen.show(input);
        screen
    }

    /// True while it waits for the key to bind.
    pub fn waiting(&self) -> bool {
        self.waiting
    }

    pub(super) fn show(&mut self, input: &Input) {
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
            } else if let Some(key) = ctx.input().any_key_pressed() {
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

    /// Draws the actions and the keys bound to them.
    pub fn draw(&self, gfx: &mut Gfx, font: &Font, theme: &Theme) {
        self.menu.draw(gfx, font, theme);
    }
}
