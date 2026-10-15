use crate::hangul::{Hangul, latin_to_jamo};
use crate::{Ctx, Font, Gfx, Input, Key, MouseButton, Rect, Vec2};

pub fn draw_panel(gfx: &mut Gfx, r: Rect, fill: u32, border: u32) {
    gfx.fill(r, fill);
    gfx.outline(r, border);
}

/// Fills `frac` (clamped to 0..=1) of `r` from the left over a `back` track.
pub fn draw_bar(gfx: &mut Gfx, r: Rect, frac: f32, color: u32, back: u32) {
    gfx.fill(r, back);
    gfx.fill(Rect { w: (r.w * frac.clamp(0.0, 1.0)).floor(), ..r }, color);
}

pub fn hovered(input: &Input, r: Rect) -> bool {
    input.mouse().is_some_and(|p| r.contains(p))
}

/// True on the step the left button goes down over `r`.
pub fn clicked(input: &Input, r: Rect) -> bool {
    input.mouse_pressed(MouseButton::Left) && hovered(input, r)
}

/// While the left button is held over `r`, the value (0..=1) under the cursor (first pixel 0, last pixel 1); otherwise `value` unchanged.
/// Dragging off the rectangle stops it following, so make the rectangle taller than the drawn track.
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    /// Labels, typed text and the rows that are not highlighted.
    pub text: u32,
    /// The highlighted row of a list and the focus outline.
    pub active: u32,
    pub fill: u32,
    /// A button or box under the cursor.
    pub hot: u32,
    /// A button held down.
    pub down: u32,
    pub edge: u32,
}

impl Theme {
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

/// A labelled button: `update` once per step (true on the step it is clicked), then `draw`; the look follows the
/// cursor as of the last `update`.
pub struct Button {
    pub rect: Rect,
    pub label: String,
    shade: Shade,
}

impl Button {
    pub fn new(rect: Rect, label: &str) -> Self {
        Button { rect, label: label.to_string(), shade: Shade::Idle }
    }

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
pub struct Checkbox {
    pub checked: bool,
    pub rect: Rect,
    label: String,
    shade: Shade,
}

impl Checkbox {
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

    pub fn draw(&self, gfx: &mut Gfx, font: &Font, theme: &Theme) {
        let r = Rect::new(self.rect.x, self.rect.y, BOX, BOX);
        draw_panel(gfx, r, self.shade.color(theme), theme.edge);
        if self.checked {
            gfx.fill(Rect::new(r.x + 3.0, r.y + 3.0, BOX - 6.0, BOX - 6.0), theme.text);
        }
        gfx.text_font(font, &self.label, r.x as i32 + BOX as i32 + 4, r.y as i32, theme.text);
    }
}

/// A single-line text box at `x`, `y`: call `update` once per fixed step with the typed characters and Backspace.
///
/// Composes Hangul itself (no input method): Korean-layout jamo join into syllables, and `set_korean(true)` maps letters.
pub struct TextField {
    pub x: i32,
    pub y: i32,
    text: String,
    max: usize,
    korean: bool,
    hangul: Hangul,
}

impl TextField {
    /// Holds at most `max` characters (not bytes).
    pub fn new(x: i32, y: i32, max: usize) -> Self {
        TextField { x, y, text: String::new(), max, korean: false, hangul: Hangul::default() }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// Replaces the text (cut to the limit) and ends any syllable being composed.
    pub fn set(&mut self, text: &str) {
        self.text = text.chars().take(self.max).collect();
        self.hangul.end();
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.hangul.end();
    }

    pub fn korean(&self) -> bool {
        self.korean
    }

    pub fn set_korean(&mut self, on: bool) {
        self.korean = on;
        self.hangul.end();
    }

    /// Returns true when Enter was pressed this step.
    pub fn update(&mut self, input: &Input) -> bool {
        if input.key_pressed(Key::RightAlt) {
            self.set_korean(!self.korean);
        }
        if input.key_repeated(Key::Backspace) {
            self.hangul.backspace(&mut self.text);
        }
        for &c in input.typed() {
            let before = (self.text.clone(), self.hangul);
            self.hangul.push(&mut self.text, if self.korean { latin_to_jamo(c) } else { c });
            if self.text.chars().count() > self.max {
                (self.text, self.hangul) = before;
            }
        }
        let done = input.key_pressed(Key::Enter);
        if done {
            self.hangul.end();
        }
        done
    }

    /// Draws the text and, while `caret` is true, a `|` after it; callers blink it with a `Timer`.
    pub fn draw(&self, gfx: &mut Gfx, font: &Font, theme: &Theme, caret: bool) {
        self.draw_at(gfx, font, self.x, self.y, theme.text, caret);
    }

    pub(crate) fn draw_at(&self, gfx: &mut Gfx, font: &Font, x: i32, y: i32, color: u32, caret: bool) {
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

/// A column of rows from `x`, `y`, one of them highlighted.
pub struct Menu {
    pub x: i32,
    pub y: i32,
    items: Vec<String>,
    sel: usize,
}

impl Menu {
    pub fn new(x: i32, y: i32, items: &[&str]) -> Self {
        assert!(!items.is_empty(), "a menu needs at least one item");
        Menu { x, y, items: items.iter().map(|s| s.to_string()).collect(), sel: 0 }
    }

    pub fn selected(&self) -> usize {
        self.sel
    }

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

/// Splits an area into consecutive rectangles along one axis, `gap` pixels apart, each as wide (or tall) as the
/// area is across.
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
pub fn grid(area: Rect, cols: usize, rows: usize, gap: f32) -> Vec<Rect> {
    let (cols, rows) = (cols.max(1), rows.max(1));
    let w = (area.w - gap * (cols - 1) as f32) / cols as f32;
    let h = (area.h - gap * (rows - 1) as f32) / rows as f32;
    (0..cols * rows)
        .map(|i| Rect { x: area.x + (i % cols) as f32 * (w + gap), y: area.y + (i / cols) as f32 * (h + gap), w, h })
        .collect()
}

/// Breaks `text` into lines at most `max` pixels wide: at spaces, at line breaks, and inside a word that is too long
/// for a line (a run of Korean without spaces, say). A line always holds at least one character.
pub fn wrap(font: &Font, text: &str, max: i32) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        for word in paragraph.split(' ') {
            let joined = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
            if font.width(&joined) <= max {
                line = joined;
                continue;
            }
            if !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            for ch in word.chars() {
                let longer = format!("{line}{ch}");
                if !line.is_empty() && font.width(&longer) > max {
                    lines.push(std::mem::replace(&mut line, ch.to_string()));
                } else {
                    line = longer;
                }
            }
        }
        lines.push(line);
    }
    lines
}

/// Draws `text` wrapped to the width of `r` and as many lines as fit its height; returns the number of lines the
/// whole text takes, so a caller can tell it was cut off.
pub fn draw_wrapped(gfx: &mut Gfx, font: &Font, text: &str, r: Rect, color: u32) -> usize {
    let lines = wrap(font, text, r.w as i32);
    let fit = ((r.h as i32) / font.line_height().max(1)).max(0) as usize;
    for (i, line) in lines.iter().take(fit).enumerate() {
        gfx.text_font(font, line, r.x as i32, r.y as i32 + i as i32 * font.line_height(), color);
    }
    lines.len()
}

/// A list too long for its rectangle: arrow keys (as actions) move the highlight, the wheel scrolls, a click picks.
pub struct ScrollList {
    pub rect: Rect,
    items: Vec<String>,
    sel: usize,
    top: usize,
}

impl ScrollList {
    pub fn new(rect: Rect, items: &[&str]) -> Self {
        ScrollList { rect, items: items.iter().map(|s| s.to_string()).collect(), sel: 0, top: 0 }
    }

    pub fn selected(&self) -> usize {
        self.sel
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    fn rows(&self, font: &Font) -> usize {
        ((self.rect.h as i32) / row_metrics(font).0).max(1) as usize
    }

    // Keeps the window on the list and the highlighted row inside it.
    fn settle(&mut self, rows: usize) {
        self.sel = self.sel.min(self.items.len().saturating_sub(1));
        self.top = self.top.min(self.items.len().saturating_sub(rows));
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
        self.sel = self.sel.min(self.items.len().saturating_sub(1));
        self.top = self.top.min(self.items.len().saturating_sub(rows));
        if let Some(Vec2 { y, .. }) = input.mouse().filter(|_| clicked(input, r)) {
            let row = self.top + ((y - r.y) as i32 / step) as usize;
            if row < self.items.len() {
                self.sel = row;
                return Some(row);
            }
        }
        input.pressed(accept).then_some(self.sel).filter(|_| !self.items.is_empty())
    }

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
#[derive(Default)]
pub struct Focus {
    index: usize,
}

impl Focus {
    pub fn new() -> Self {
        Focus::default()
    }

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
        if input.mouse_moved() {
            if let Some(over) = rects.iter().position(|r| hovered(input, *r)) {
                self.index = over;
            }
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
pub struct Rebind {
    menu: Menu,
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

    pub fn draw(&self, gfx: &mut Gfx, font: &Font, theme: &Theme) {
        self.menu.draw(gfx, font, theme);
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
        let font = Font::load("assets/fonts/galmuri9.bdf").unwrap();
        let (mut input, mut menu) = (menu_input(), Menu::new(0, 0, &["a", "b", "c"]));
        press(&mut input, Key::Up);
        assert_eq!(menu.update(&input, &font, "up", "down", "ok"), None);
        assert_eq!(menu.selected(), 2);
        press(&mut input, Key::Down);
        menu.update(&input, &font, "up", "down", "ok");
        assert_eq!(menu.selected(), 0);
        press(&mut input, Key::Down);
        menu.update(&input, &font, "up", "down", "ok");
        press(&mut input, Key::Enter);
        assert_eq!(menu.update(&input, &font, "up", "down", "ok"), Some(1));
        input.end_step();
        assert_eq!(menu.update(&input, &font, "up", "down", "ok"), None, "held is not pressed again");
    }

    fn at(input: &mut Input, x: f32, y: f32, left: bool) {
        input.end_step();
        input.point(Some((x, y)), left);
    }

    #[test]
    fn clicks_and_sliders_follow_the_cursor() {
        let (mut input, r) = (Input::default(), Rect::new(10.0, 10.0, 100.0, 8.0));
        at(&mut input, 20.0, 12.0, false);
        assert!(hovered(&input, r) && !clicked(&input, r));
        at(&mut input, 20.0, 12.0, true);
        assert!(clicked(&input, r));
        assert!((slider(&input, r, 0.9) - 10.0 / 99.0).abs() < 1e-6);
        at(&mut input, 109.9, 12.0, true);
        assert_eq!(slider(&input, r, 0.0), 1.0, "the last pixel is the full value");
        at(&mut input, 10.2, 12.0, true);
        assert_eq!(slider(&input, r, 1.0), 0.0);
        at(&mut input, 20.0, 12.0, true);
        at(&mut input, 20.0, 12.0, true);
        assert!(!clicked(&input, r), "holding is not a new click");
        at(&mut input, 500.0, 12.0, true);
        assert_eq!(slider(&input, r, 0.9), 0.9, "outside leaves the value alone");
        at(&mut input, 20.0, 40.0, true);
        assert!(!hovered(&input, r));
    }

    #[test]
    fn a_button_reports_a_click_once_and_centres_its_label() {
        let font = Font::load("assets/fonts/galmuri9.bdf").unwrap();
        let (mut input, mut b) = (Input::default(), Button::new(Rect::new(10.0, 10.0, 60.0, 20.0), "확인"));
        at(&mut input, 200.0, 12.0, false);
        assert!(!b.update(&input));
        at(&mut input, 20.0, 12.0, true);
        assert!(b.update(&input));
        at(&mut input, 20.0, 12.0, true);
        assert!(!b.update(&input), "holding is not a new click");
        assert_eq!(b.shade.color(&Theme::DEFAULT), Theme::DEFAULT.down);
        let mut g = Gfx::new(100, 50);
        b.draw(&mut g, &font, &Theme::DEFAULT);
        let glyph =
            g.cmds().iter().find_map(|c| matches!(c.paint, crate::Paint::Mask { .. }).then_some(c.rect)).unwrap();
        assert_eq!(glyph.x as i32, 10 + (60 - font.width("확인")) / 2 + font.layout("확인").next().unwrap().1);
    }

    #[test]
    fn a_checkbox_flips_on_a_click_on_the_box_or_its_label_and_shows_its_state() {
        let font = Font::load("assets/fonts/galmuri9.bdf").unwrap();
        let (mut input, mut c) = (Input::default(), Checkbox::new(&font, 10, 10, "소리", false));
        let count = |c: &Checkbox| {
            let mut g = Gfx::new(100, 50);
            c.draw(&mut g, &font, &Theme::DEFAULT);
            g.cmds().len()
        };
        let off = count(&c);
        at(&mut input, 200.0, 12.0, false);
        assert!(!c.update(&input) && !c.checked);
        at(&mut input, 12.0, 12.0, true);
        assert!(c.update(&input) && c.checked, "on the box");
        assert_eq!(count(&c), off + 1, "a tick is one more fill");
        at(&mut input, 28.0, 12.0, false);
        at(&mut input, 28.0, 12.0, true);
        assert!(c.update(&input) && !c.checked, "on the label");
    }

    #[test]
    fn a_menu_row_highlights_on_hover_and_returns_on_click_but_ignores_a_resting_cursor() {
        let font = Font::load("assets/fonts/galmuri9.bdf").unwrap();
        let (mut input, mut menu) = (menu_input(), Menu::new(0, 0, &["시작", "끝"]));
        let second_row = font.line_height() as f32 + 3.0 + 2.0;
        at(&mut input, 4.0, second_row, false);
        assert_eq!(menu.update(&input, &font, "up", "down", "ok"), None);
        assert_eq!(menu.selected(), 1);
        press(&mut input, Key::Up);
        menu.update(&input, &font, "up", "down", "ok");
        assert_eq!(menu.selected(), 0);
        at(&mut input, 4.0, second_row, false);
        menu.update(&input, &font, "up", "down", "ok");
        assert_eq!(menu.selected(), 0, "a cursor that did not move leaves the keyboard choice");
        at(&mut input, 5.0, second_row, true);
        assert_eq!(menu.update(&input, &font, "up", "down", "ok"), Some(1));
    }

    #[test]
    fn a_text_field_takes_typed_characters_up_to_its_limit_and_backspace_removes_one() {
        let (mut input, mut field) = (Input::default(), TextField::new(0, 0, 3));
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
    fn a_text_field_composes_hangul_from_jamo_or_from_letter_keys_and_keeps_its_limit() {
        let (mut input, mut field) = (Input::default(), TextField::new(0, 0, 2));
        input.type_chars("ㅎㅏㄴㄱㅡㄹ");
        field.update(&input);
        assert_eq!(field.text(), "한글", "jamo from a Korean layout");
        input.type_chars("ㅅ");
        field.update(&input);
        assert_eq!(field.text(), "한긄", "joining the final needs no room");
        input.type_chars("ㅏ");
        field.update(&input);
        assert_eq!(field.text(), "한긄", "a third syllable does not fit");
        field.clear();
        input.type_chars("ㅎㅏㄴㄱㅡㄹ");
        field.update(&input);
        field.clear();
        field.set_korean(true);
        input.type_chars("gksrmf");
        field.update(&input);
        assert_eq!(field.text(), "한글", "letter keys as a two-set keyboard");
        press(&mut input, Key::Backspace);
        field.update(&input);
        assert_eq!(field.text(), "한그");
        press(&mut input, Key::RightAlt);
        field.update(&input);
        assert!(!field.korean());
        input.type_chars("k");
        field.update(&input);
        assert_eq!(field.text(), "한그", "toggled off, and the box is full");
    }

    #[test]
    fn bar_clamps_and_menu_draws_one_command_per_glyph() {
        let mut g = Gfx::new(100, 50);
        draw_bar(&mut g, Rect::new(0.0, 0.0, 50.0, 4.0), 7.0, 1, 2);
        assert_eq!(g.cmds().len(), 2);
        let font = Font::load("assets/fonts/galmuri9.bdf").unwrap();
        let mut g = Gfx::new(100, 50);
        Menu::new(0, 0, &["시작", "끝"]).draw(&mut g, &font, &Theme::DEFAULT);
        assert_eq!(g.cmds().len(), 1 + 2 + 1);
    }

    #[test]
    fn a_rebind_row_waits_for_a_key_binds_it_and_escape_cancels() {
        let mut ctx = Ctx::new(64, 64, 1.0 / 60.0, crate::Audio::disabled());
        ctx.bind("jump", &[Key::Space]);
        let mut screen = Rebind::new(ctx.input(), 0, 0);
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
        let mut screen = Rebind::new(ctx.input(), 0, 0);
        assert!(screen.menu.items.iter().all(|row| !row.contains("also")), "{:?}", screen.menu.items);
        ctx.bind("jump", &[Key::A]);
        screen.show(ctx.input());
        let rows = &screen.menu.items;
        assert_eq!((rows[0].as_str(), rows[1].as_str()), ("fire: Z", "jump: A  (also left)"), "{rows:?}");
        assert_eq!(rows[2], "left: Left A  (also jump)");
    }

    #[test]
    fn a_column_hands_out_rectangles_down_the_area_and_runs_dry_politely() {
        let mut col = Layout::column(Rect::new(10.0, 20.0, 100.0, 50.0), 4.0);
        assert_eq!(col.next(10.0), Rect::new(10.0, 20.0, 100.0, 10.0));
        assert_eq!(col.next(10.0), Rect::new(10.0, 34.0, 100.0, 10.0));
        assert_eq!(col.rest(), Rect::new(10.0, 48.0, 100.0, 22.0));
        assert_eq!(col.next(10.0).h, 0.0, "nothing left");
        let mut row = Layout::row(Rect::new(0.0, 0.0, 30.0, 8.0), 0.0);
        assert_eq!(row.next(20.0).w, 20.0);
        assert_eq!(row.next(20.0), Rect::new(20.0, 0.0, 10.0, 8.0), "cut short at the edge");
    }

    #[test]
    fn a_grid_fills_the_area_with_equal_cells() {
        let cells = grid(Rect::new(0.0, 0.0, 22.0, 10.0), 2, 2, 2.0);
        assert_eq!(cells.len(), 4);
        assert_eq!(cells[0], Rect::new(0.0, 0.0, 10.0, 4.0));
        assert_eq!(cells[3], Rect::new(12.0, 6.0, 10.0, 4.0));
        assert_eq!(grid(Rect::new(0.0, 0.0, 5.0, 5.0), 0, 0, 1.0).len(), 1, "never empty from a zero count");
    }

    #[test]
    fn wrapped_lines_fit_keep_the_words_and_break_a_long_run_of_korean_anywhere() {
        let font = Font::load("assets/fonts/galmuri9.bdf").unwrap();
        let max = font.width("aaaa bbbb");
        assert_eq!(wrap(&font, "aaaa bbbb cccc", max), ["aaaa bbbb", "cccc"]);
        assert_eq!(wrap(&font, "a\n\nb", 100), ["a", "", "b"], "line breaks stay");
        assert_eq!(wrap(&font, "", 100), [""]);
        let long = "가나다라마바사아자차";
        let lines = wrap(&font, long, font.width("가나다"));
        assert!(lines.len() >= 4 && lines.iter().all(|l| font.width(l) <= font.width("가나다")), "{lines:?}");
        assert_eq!(lines.concat(), long, "no character lost or added");
        assert_eq!(wrap(&font, "가나", 1), ["가", "나"], "a width below one glyph still moves on");
    }

    #[test]
    fn drawing_wrapped_text_stops_at_the_height_and_reports_the_full_length() {
        let font = Font::load("assets/fonts/galmuri9.bdf").unwrap();
        let mut g = Gfx::new(200, 100);
        let r = Rect::new(0.0, 0.0, font.width("aaaa") as f32, font.line_height() as f32 * 2.0);
        let total = draw_wrapped(&mut g, &font, "aaaa bbbb cccc dddd", r, 0xffffff);
        assert_eq!(total, 4);
        let ys: std::collections::BTreeSet<_> = g.cmds().iter().map(|c| c.rect.y as i32).collect();
        assert_eq!(ys.len(), 2, "only two lines fit: {ys:?}");
    }

    fn list(n: usize) -> (ScrollList, Font) {
        let font = Font::load("assets/fonts/galmuri9.bdf").unwrap();
        let items: Vec<String> = (0..n).map(|i| format!("item {i}")).collect();
        let r = Rect::new(0.0, 0.0, 100.0, (row_metrics(&font).0 * 3) as f32);
        (ScrollList::new(r, &items.iter().map(String::as_str).collect::<Vec<_>>()), font)
    }

    #[test]
    fn a_scroll_list_keeps_the_highlight_in_view_and_does_not_wrap() {
        let (mut l, font) = list(10);
        let mut input = menu_input();
        for _ in 0..5 {
            press(&mut input, Key::Down);
            l.update(&input, &font, "up", "down", "ok");
        }
        assert_eq!((l.selected(), l.top), (5, 3), "scrolled so row 5 is the last of three");
        l.select(100, &font);
        assert_eq!((l.selected(), l.top), (9, 7), "clamped at the end");
        l.select(-100, &font);
        assert_eq!((l.selected(), l.top), (0, 0));
    }

    #[test]
    fn the_wheel_scrolls_a_list_under_the_cursor_and_a_click_picks_the_row_under_it() {
        let (mut l, font) = list(10);
        let mut input = menu_input();
        at(&mut input, 5.0, 5.0, false);
        input.scroll(-1.0);
        l.update(&input, &font, "up", "down", "ok");
        assert_eq!(l.top, 3, "a notch down moves three rows");
        input.scroll(-1.0);
        l.update(&input, &font, "up", "down", "ok");
        input.scroll(-1.0);
        l.update(&input, &font, "up", "down", "ok");
        assert_eq!(l.top, 7, "and stops at the last window");
        let step = row_metrics(&font).0 as f32;
        input.scroll(0.0);
        at(&mut input, 5.0, step + 1.0, true);
        assert_eq!(l.update(&input, &font, "up", "down", "ok"), Some(8), "the second visible row");
        at(&mut input, 500.0, 5.0, false);
        input.scroll(1.0);
        l.update(&input, &font, "up", "down", "ok");
        assert_eq!(l.top, 7, "the wheel off the list does nothing");
    }

    fn four_buttons() -> [Rect; 4] {
        let r = |x, y| Rect { x, y, w: 40.0, h: 12.0 };
        [r(0.0, 0.0), r(60.0, 0.0), r(0.0, 30.0), r(60.0, 30.0)]
    }

    fn pad_input() -> Input {
        let mut i = Input::default();
        i.bind("left", &[Key::Left]);
        i.bind("right", &[Key::Right]);
        i.bind("up", &[Key::Up]);
        i.bind("down", &[Key::Down]);
        i.bind("ok", &[Key::Enter]);
        i
    }

    #[test]
    fn focus_jumps_to_the_neighbour_in_the_pressed_direction_and_picks_on_accept() {
        let (rects, mut focus, mut input) = (four_buttons(), Focus::new(), pad_input());
        let dirs = ["left", "right", "up", "down"];
        press(&mut input, Key::Right);
        assert_eq!(focus.update(&input, &rects, dirs, "ok"), None);
        assert_eq!(focus.selected(), 1);
        press(&mut input, Key::Down);
        focus.update(&input, &rects, dirs, "ok");
        assert_eq!(focus.selected(), 3);
        press(&mut input, Key::Right);
        focus.update(&input, &rects, dirs, "ok");
        assert_eq!(focus.selected(), 3, "nothing further right: it stays");
        press(&mut input, Key::Left);
        focus.update(&input, &rects, dirs, "ok");
        press(&mut input, Key::Enter);
        assert_eq!(focus.update(&input, &rects, dirs, "ok"), Some(2));
    }

    #[test]
    fn the_mouse_takes_the_focus_and_a_click_picks_and_an_empty_set_is_harmless() {
        let (rects, mut focus, mut input) = (four_buttons(), Focus::new(), pad_input());
        at(&mut input, 70.0, 35.0, false);
        assert_eq!(focus.update(&input, &rects, ["left", "right", "up", "down"], "ok"), None);
        assert_eq!(focus.selected(), 3);
        at(&mut input, 70.0, 35.0, true);
        assert_eq!(focus.update(&input, &rects, ["left", "right", "up", "down"], "ok"), Some(3));
        assert_eq!(focus.update(&input, &[], ["left", "right", "up", "down"], "ok"), None);
    }

    #[test]
    fn a_click_picks_what_is_under_the_cursor_after_keys_moved_the_focus() {
        let (rects, mut focus, mut input) = (four_buttons(), Focus::new(), pad_input());
        let dirs = ["left", "right", "up", "down"];
        at(&mut input, 70.0, 35.0, false);
        focus.update(&input, &rects, dirs, "ok");
        press(&mut input, Key::Left);
        focus.update(&input, &rects, dirs, "ok");
        assert_eq!(focus.selected(), 2);
        at(&mut input, 70.0, 35.0, true);
        assert_eq!(focus.update(&input, &rects, dirs, "ok"), Some(3));
    }
}
