use super::{Theme, clicked, hovered, row_metrics};
use crate::{Ctx, Font, Gfx, Input, Key, Rect, Vec2};

/// A list of rows with one highlighted: arrow keys (as actions) move the highlight, the mouse highlights and picks a
/// row, and in a list too long for its rectangle the wheel scrolls. [`ScrollList::menu`] makes a short wrapping one.
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
    pub(super) items: Vec<String>,
    sel: usize,
    pub(super) top: usize,
    wrap: bool,
    // Set by `select`; the next `update` scrolls the highlight into view, as that needs the font.
    follow: bool,
}

impl ScrollList {
    /// A list of `items` in `rect`, the first highlighted.
    pub fn new(rect: Rect, items: &[&str]) -> Self {
        let items = items.iter().map(std::string::ToString::to_string).collect();
        ScrollList { rect, items, sel: 0, top: 0, wrap: false, follow: false }
    }

    /// Every row from `x`, `y` down with no scrolling, the highlight wrapping at the ends. Panics without `items`.
    ///
    /// ```
    /// use engine::ScrollList;
    ///
    /// let mut menu = ScrollList::menu(10, 10, &["Start", "Options", "Quit"]);
    /// menu.select(-1);
    /// assert_eq!(menu.selected(), 2);
    /// ```
    pub fn menu(x: i32, y: i32, items: &[&str]) -> Self {
        assert!(!items.is_empty(), "a menu needs at least one item");
        let rect = Rect::new(x as f32, y as f32, f32::INFINITY, f32::INFINITY);
        ScrollList { wrap: true, ..ScrollList::new(rect, items) }
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

    /// Replaces the text of row `i`.
    pub fn set_item(&mut self, i: usize, text: String) {
        self.items[i] = text;
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

    /// Moves the highlight by `delta` rows, wrapping in a menu and stopping at the ends in a list; the next `update`
    /// scrolls it into view.
    pub fn select(&mut self, delta: isize) {
        let (last, to) = (self.items.len().saturating_sub(1) as isize, self.sel as isize + delta);
        self.sel = if self.wrap { to.rem_euclid(last + 1) } else { to.clamp(0, last) } as usize;
        self.follow = true;
    }

    /// `up`/`down` move the highlight and bring it into view, the wheel over `rect` scrolls three rows a notch (the
    /// highlight may leave the view), moving the mouse over a row highlights it, a click on a row highlights it and
    /// returns its index, as does `accept` for the highlighted row.
    pub fn update(&mut self, input: &Input, font: &Font, up: &str, down: &str, accept: &str) -> Option<usize> {
        let (r, rows, step) = (self.rect, self.rows(font), row_metrics(font).0);
        if input.pressed(up) {
            self.select(-1);
        }
        if input.pressed(down) {
            self.select(1);
        }
        if std::mem::take(&mut self.follow) {
            self.settle(rows);
        }
        if hovered(input, r) && input.wheel() != 0.0 {
            let max = self.items.len().saturating_sub(rows) as isize;
            self.top = (self.top as isize - 3 * input.wheel().signum() as isize).clamp(0, max) as usize;
        }
        self.clamp(rows);
        let row_at = |y: f32| self.top + ((y - r.y) as i32 / step) as usize;
        if let Some(Vec2 { y, .. }) = input.mouse().filter(|_| hovered(input, r)) {
            let row = row_at(y);
            if row < self.items.len() && (input.mouse_moved() || clicked(input, r)) {
                self.sel = row;
                if clicked(input, r) {
                    return Some(row);
                }
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
    pub(super) menu: ScrollList,
    actions: Vec<String>,
    waiting: bool,
}

impl Rebind {
    /// Panics when no action is bound, like `ScrollList::menu` without rows.
    pub fn new(input: &Input, x: i32, y: i32) -> Self {
        let actions: Vec<String> = input.actions().into_iter().map(String::from).collect();
        assert!(!actions.is_empty(), "bind actions before opening a Rebind screen");
        let mut screen = Rebind {
            menu: ScrollList::menu(x, y, &actions.iter().map(String::as_str).collect::<Vec<_>>()),
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
