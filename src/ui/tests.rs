use super::*;
use crate::{Ctx, Font, Key};

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
    let glyph = g.cmds().iter().find_map(|c| matches!(c.paint, crate::Paint::Mask { .. }).then_some(c.rect)).unwrap();
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
