use super::Theme;
use super::hangul::{Hangul, latin_to_jamo};
use crate::{Font, Gfx, Input, Key, Rect};

/// A single-line text box at `x`, `y`: call `update` once per fixed step with the typed characters and Backspace.
///
/// Composes Hangul itself (no input method): Korean-layout jamo join into syllables, and `set_korean(true)` maps letters.
///
/// ```
/// use engine::TextField;
///
/// let mut name = TextField::new(10, 10, 12);
/// name.set("Ada");
/// assert_eq!(name.text(), "Ada");
/// name.clear();
/// assert_eq!(name.text(), "");
/// ```
pub struct TextField {
    /// Left edge.
    pub x: i32,
    /// Top edge.
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

    /// What has been typed.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Replaces the text (cut to the limit) and ends any syllable being composed.
    pub fn set(&mut self, text: &str) {
        self.text = text.chars().take(self.max).collect();
        self.hangul.end();
    }

    /// Empties the field.
    pub fn clear(&mut self) {
        self.text.clear();
        self.hangul.end();
    }

    /// True while letter keys type Korean jamo.
    pub fn korean(&self) -> bool {
        self.korean
    }

    /// Turns Korean typing on or off.
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

/// Breaks `text` into lines at most `max` pixels wide: at spaces, at line breaks, and inside a word that is too long
/// for a line (a run of Korean without spaces, say). A line always holds at least one character.
///
/// ```
/// use engine::{Font, wrap};
///
/// let font = Font::load("assets/fonts/galmuri9.bdf").unwrap();
/// let lines = wrap(&font, "one two three four", font.width("one two"));
/// assert!(lines.len() >= 2);
/// ```
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
///
/// ```
/// use engine::{Font, Gfx, Rect, draw_wrapped};
///
/// fn say(gfx: &mut Gfx, font: &Font) -> usize {
///     draw_wrapped(gfx, font, "A long line of speech.", Rect::new(10.0, 10.0, 80.0, 40.0), 0xffffff)
/// }
/// ```
pub fn draw_wrapped(gfx: &mut Gfx, font: &Font, text: &str, r: Rect, color: u32) -> usize {
    let lines = wrap(font, text, r.w as i32);
    let fit = ((r.h as i32) / font.line_height().max(1)).max(0) as usize;
    for (i, line) in lines.iter().take(fit).enumerate() {
        gfx.text_font(font, line, r.x as i32, r.y as i32 + i as i32 * font.line_height(), color);
    }
    lines.len()
}
