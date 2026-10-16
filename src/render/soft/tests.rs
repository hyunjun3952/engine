use super::*;
use crate::{DrawCmd, Gfx};

fn run(w: usize, h: usize, f: impl FnOnce(&mut Gfx)) -> Soft {
    let (mut g, mut s) = (Gfx::new(w, h), Soft::new(w, h, 1));
    f(&mut g);
    s.render(g.cmds());
    s
}

#[test]
fn scaled_font_text_covers_k_by_k_per_source_pixel_and_clips_at_the_edge() {
    let font = crate::Font::load("assets/fonts/galmuri9.bdf").unwrap();
    let lit = |s: &Soft| s.buf.iter().filter(|&&p| p & 0xFF_FFFF == 0xFF_FFFF).count();
    let one = run(60, 40, |g| g.text_font(&font, "ㅁ", 2, 2, 0xffffff));
    let two = run(60, 40, |g| g.text_font_scaled(&font, "ㅁ", 2, 2, 2, 0xffffff));
    assert!(lit(&one) > 0 && lit(&two) == lit(&one) * 4);
    run(8, 8, |g| g.text_font_scaled(&font, "한", -5, -5, 3, 1));
    run(8, 8, |g| g.text_font_scaled(&font, "한", 5, 5, 3, 1));
}

#[test]
fn blit_clips_at_every_edge_and_flip_mirrors_columns() {
    let sprite = Sprite::test_ball();
    let s = run(16, 16, |g| g.blit(&sprite, -8, -8));
    assert_eq!((s.buf[0], s.buf[8 * 16 + 8]), (sprite.image().px[8 * 16 + 8], 0));
    let s = run(16, 16, |g| g.blit_region(&sprite, Rect::new(0.0, 0.0, 16.0, 16.0), 0, 0, true));
    assert_eq!(s.buf[8 * 16 + 3], sprite.image().px[8 * 16 + 12]);
    run(16, 16, |g| {
        for (x, y) in [(-100, 5), (100, 100), (5, -100), (15, 15), (-15, -15)] {
            g.blit(&sprite, x, y);
        }
    });
    let two = std::sync::Arc::new(Sprite::new(2, 1, vec![0xFF00_0001, 0xFF00_0002]));
    let s = run(4, 1, |g| g.draw([DrawCmd::new(Rect::new(-0.5, 0.0, 2.0, 1.0), Paint::Sprite(two))]));
    assert_eq!(s.buf[0], 0xFF00_0002, "half a pixel off the left edge is a pixel off, as on the right");
}

#[test]
fn lines_and_circles_light_the_pixels_they_pass_through() {
    let lit = |s: &Soft| -> Vec<(usize, usize)> {
        (0..s.buf.len()).filter(|&i| s.buf[i] == 7).map(|i| (i % s.w, i / s.w)).collect()
    };
    let s = run(8, 8, |g| g.line((1.0, 2.0), (5.0, 2.0), 7));
    assert_eq!(lit(&s), (1..=5).map(|x| (x, 2)).collect::<Vec<_>>(), "ends included");
    let s = run(8, 8, |g| g.line((0.0, 0.0), (7.0, 7.0), 7));
    assert!((0..8).all(|i| lit(&s).contains(&(i, i))), "the diagonal, no gaps: {:?}", lit(&s));
    let s = run(8, 8, |g| g.line((6.0, 1.0), (1.0, 3.0), 7));
    let rows: Vec<usize> = lit(&s).iter().map(|p| p.1).collect();
    assert!((1..=6).all(|x| lit(&s).iter().any(|p| p.0 == x)) && rows.iter().all(|y| (1..=3).contains(y)));
    let s = run(12, 12, |g| g.circle((6.0, 6.0), 3.0, 7));
    let want: Vec<(usize, usize)> = (0..144)
        .map(|i| (i % 12, i / 12))
        .filter(|&(x, y)| (x as f32 + 0.5 - 6.0).hypot(y as f32 + 0.5 - 6.0) <= 3.0)
        .collect();
    assert_eq!(lit(&s), want);
    run(8, 8, |g| g.circle((4.0, 4.0), 1e9, 7));
    assert_eq!(lit(&run(8, 8, |g| g.circle((f32::NAN, 4.0), 2.0, 7))), vec![]);
    assert_eq!(Gfx::text_width("한글 ab", 2), 5 * 16);
}

#[test]
fn text_lights_exactly_the_glyph_bits_scaled() {
    let lit: u32 = font::bits(b'A').iter().map(|b| b.count_ones()).sum();
    let s = run(40, 40, |g| g.text("A", 3, 5, 2, 7));
    assert_eq!(s.buf.iter().filter(|&&p| p == 7).count() as u32, lit * 4);
    let s = run(8, 8, |g| g.text("A", -4, -4, 1, 7));
    assert!(s.buf.contains(&7));
}

#[test]
fn blend_stays_within_one_of_the_exact_division_and_is_exact_at_the_ends() {
    let colors = [0x000000, 0xffffff, 0x102030, 0xff0080, 0x80ff00, 0x0080ff, 0xdeadbe];
    for &src in &colors {
        for &dst in &colors {
            assert_eq!((blend(src, dst, 0), blend(src, dst, 255)), (dst, src));
            for a in 0..=255u32 {
                let got = blend(src, dst, a);
                for sh in [16, 8, 0] {
                    let (s, d) = ((src >> sh & 255) as i32, (dst >> sh & 255) as i32);
                    let exact = (s * a as i32 + d * (255 - a as i32)) / 255;
                    assert!(((got >> sh & 255) as i32 - exact).abs() <= 1, "src {src:x} dst {dst:x} a {a}");
                }
                assert_eq!(got >> 24, 0, "the alpha byte stays clear");
            }
        }
    }
}

#[cfg(target_arch = "x86_64")]
#[test]
fn the_avx2_path_gives_exactly_the_same_pixels_as_the_plain_one() {
    if !std::is_x86_feature_detected!("avx2") {
        eprintln!("no AVX2, skipping");
        return;
    }
    let base: Vec<u32> = (0..4099u32).map(|i| i.wrapping_mul(2654435761) & 0x00ff_ffff).collect();
    for (src, a) in [(0x336699, 150), (0xffffff, 1), (0x000000, 254), (0xdeadbe, 128), (0x123456, 0), (0x123456, 255)] {
        let (mut plain, mut fast) = (base.clone(), base.clone());
        blend_row_plain(&mut plain, src, a);
        unsafe { blend_row_avx2(&mut fast, src, a) };
        assert!(plain == fast, "src {src:x} a {a}");
    }
}

fn drawn(w: usize, h: usize, rect: Rect, paint: Paint, xform: Transform) -> Soft {
    run(w, h, |g| g.draw([DrawCmd { xform, ..DrawCmd::new(rect, paint) }]))
}

fn lit(s: &Soft, w: usize) -> Vec<(usize, usize)> {
    s.buf.iter().enumerate().filter(|(_, p)| **p != 0).map(|(i, _)| (i % w, i / w)).collect()
}

#[test]
fn a_source_rect_that_is_not_inside_its_sheet_draws_nothing_instead_of_panicking() {
    // What hot reload leaves behind when a sheet is replaced by a smaller picture.
    let sheet = std::sync::Arc::new(Sprite::new(4, 4, vec![0xFFFF_FFFF; 16]));
    let at = Rect::new(2.0, 2.0, 8.0, 8.0);
    for src in [Rect::new(8.0, 0.0, 4.0, 4.0), Rect::new(2.0, 2.0, 4.0, 4.0), Rect::new(-1.0, 0.0, 4.0, 4.0)] {
        for xform in [
            Transform::IDENTITY,
            Transform { angle: 0.0, sx: 2.0, sy: 2.0 },
            Transform { angle: 0.6, sx: 1.0, sy: 1.0 },
        ] {
            let region = Paint::Region { sheet: sheet.clone(), src, flip: false };
            assert!(drawn(32, 32, at, region, xform).buf.iter().all(|&p| p == 0), "{src:?} {xform:?}");
        }
        let mask = Paint::Mask { sheet: sheet.clone(), src, color: 0xffffff };
        assert!(drawn(32, 32, at, mask, Transform::IDENTITY).buf.iter().all(|&p| p == 0), "mask {src:?}");
    }
    let inside = Paint::Region { sheet, src: Rect::new(0.0, 0.0, 4.0, 4.0), flip: false };
    assert!(drawn(32, 32, at, inside, Transform::IDENTITY).buf.iter().any(|&p| p != 0), "a rect inside still draws");
}

#[test]
fn rectangles_and_numbers_of_any_kind_never_panic_the_renderer() {
    // What a physics blow-up or a divide by zero hands the renderer: not a number, infinite, huge, backwards.
    let sheet = std::sync::Arc::new(Sprite::new(6, 4, (0..24).map(|i| 0xFF00_0000 | (i * 977)).collect()));
    let odd = [0.0, -1.0, 1e-9, 0.5, 1.0, 7.5, 31.9, 1e9, -1e9, f32::NAN, f32::INFINITY, f32::NEG_INFINITY];
    let mut rng = crate::Rng::new(9);
    let mut pick = move || odd[(rng.f32() * odd.len() as f32) as usize % odd.len()];
    for case in 0..8000 {
        let rect = Rect::new(pick(), pick(), pick(), pick());
        let xform = Transform { angle: pick(), sx: pick(), sy: pick() };
        let src = Rect::new(pick(), pick(), pick(), pick());
        let paint = match case % 8 {
            0 => Paint::Color(1),
            1 => Paint::Alpha(0xff0000, 128),
            2 => Paint::Sprite(sheet.clone()),
            3 => Paint::Region { sheet: sheet.clone(), src, flip: case % 2 == 0 },
            4 => Paint::Mask { sheet: sheet.clone(), src, color: 0xffffff },
            5 => Paint::Outline(2),
            6 => Paint::Glyph { ch: b'A', color: 3, scale: (case % 5) as u32 },
            _ => Paint::Tiles { sheet: sheet.clone(), size: 2, tiles: vec![(0, 0, 1), (1, 1, 3)] },
        };
        let shown = format!("{rect:?} {xform:?} {src:?}");
        let ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drawn(32, 24, rect, paint, xform)));
        assert!(ok.is_ok(), "case {case} panicked: {shown}");
    }
}

#[test]
fn a_zoom_and_a_camera_of_any_kind_never_panic_the_path_from_draw_to_pixels() {
    let sheet = std::sync::Arc::new(Sprite::new(8, 4, (0..32).map(|i| 0xFF00_0000 | (i * 977)).collect()));
    let odd = [0.0, -1.0, 1e-9, 0.5, 1.0, 7.5, 31.9, 1e9, -1e9, f32::NAN, f32::INFINITY, f32::NEG_INFINITY];
    let zooms = [0.25, 0.5, 1.0, 1.5, 2.0, 3.3, 8.0, f32::NAN, f32::INFINITY, 0.0, -1.0, 1e-30, 1e30];
    let mut rng = crate::Rng::new(4);
    let mut pick = move || odd[(rng.f32() * odd.len() as f32) as usize % odd.len()];
    for case in 0..3000 {
        let mut g = crate::Gfx::new(32, 24);
        (g.zoom, g.cam) = (zooms[case % zooms.len()], Vec2::new(pick(), pick()));
        let rect = Rect::new(pick(), pick(), pick(), pick());
        let src = Rect::new(pick(), pick(), pick(), pick());
        let xform = Transform { angle: pick(), sx: pick(), sy: pick() };
        let paint = match case % 7 {
            0 => Paint::Color(1),
            1 => Paint::Alpha(0xff0000, 128),
            2 => Paint::Sprite(sheet.clone()),
            3 => Paint::Region { sheet: sheet.clone(), src, flip: false },
            4 => Paint::Mask { sheet: sheet.clone(), src, color: 1 },
            5 => Paint::Outline(2),
            _ => Paint::Tiles { sheet: sheet.clone(), size: 4, tiles: vec![(0, 0, 1), (1, 1, 2)] },
        };
        let shown = format!("zoom {} cam {:?} {rect:?} {xform:?}", g.zoom, g.cam);
        let ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            g.draw([DrawCmd { xform, ..DrawCmd::new(rect, paint) }]);
            Soft::new(32, 24, 1).render(g.cmds());
        }));
        assert!(ok.is_ok(), "case {case} panicked: {shown}");
    }
}

#[test]
fn a_turned_or_scaled_sprite_beyond_the_right_or_bottom_edge_draws_nothing_and_does_not_panic() {
    let sheet = std::sync::Arc::new(Sprite::new(4, 4, vec![0xFFFF_FFFF; 16]));
    for (x, y) in [(60.0, 30.0), (47.5, 40.0), (54.0, 40.0), (100.0, 100.0)] {
        for angle in [0.0, 0.7] {
            let at = Rect { x, y, w: 4.0, h: 4.0 };
            let s = drawn(48, 36, at, Paint::Sprite(sheet.clone()), Transform { angle, sx: 2.0, sy: 2.0 });
            assert!(s.buf.iter().all(|&p| p == 0), "nothing is on the screen at ({x}, {y}), angle {angle}");
        }
    }
}

#[test]
fn the_unturned_scaling_path_picks_the_same_texel_as_the_general_one_for_every_pixel() {
    // An angle of 1e-30 is zero as far as the maths goes but sends the sprite down the general path.
    let mut rng = crate::Rng::new(5);
    let texels: Vec<u32> = (0..7 * 5).map(|i| 0xFF00_0000 | (i as u32 * 0x0003_1713 + 1)).collect();
    let sheet = std::sync::Arc::new(Sprite::new(7, 5, texels));
    for case in 0..300 {
        let (sx, sy) = (rng.range(0.3, 5.0) * [1.0, -1.0][case % 2], rng.range(0.3, 5.0) * [1.0, -1.0][case / 2 % 2]);
        let rect = Rect { x: rng.range(-30.0, 50.0), y: rng.range(-30.0, 40.0), w: 7.0, h: 5.0 };
        let flip = case % 3 == 0;
        let paint = Paint::Region { sheet: sheet.clone(), src: Rect::new(1.0, 0.0, 5.0, 5.0), flip };
        let fast = drawn(48, 36, rect, paint.clone(), Transform { angle: 0.0, sx, sy });
        let slow = drawn(48, 36, rect, paint, Transform { angle: 1e-30, sx, sy });
        assert!(fast.buf == slow.buf, "case {case}: scale {sx} x {sy} at {rect:?}, flip {flip}");
    }
}

#[test]
fn rotating_a_rect_a_quarter_turn_swaps_its_width_and_height_around_the_centre() {
    let quarter = Transform { angle: std::f32::consts::FRAC_PI_2, ..Transform::IDENTITY };
    let s = drawn(8, 8, Rect::new(2.0, 3.0, 4.0, 2.0), Paint::Color(7), quarter);
    let want: Vec<(usize, usize)> = (2..6).flat_map(|y| (3..5).map(move |x| (x, y))).collect();
    assert_eq!(lit(&s, 8), want);
}

#[test]
fn scaling_grows_around_the_centre_and_a_zero_scale_draws_nothing() {
    let double = Transform { sx: 2.0, sy: 2.0, ..Transform::IDENTITY };
    let s = drawn(8, 8, Rect::new(3.0, 3.0, 2.0, 2.0), Paint::Color(7), double);
    let want: Vec<(usize, usize)> = (2..6).flat_map(|y| (2..6).map(move |x| (x, y))).collect();
    assert_eq!(lit(&s, 8), want);
    let none = Transform { sx: 0.0, ..Transform::IDENTITY };
    assert!(drawn(8, 8, Rect::new(3.0, 3.0, 2.0, 2.0), Paint::Color(7), none).buf.iter().all(|&p| p == 0));
}

#[test]
fn a_square_turned_45_degrees_keeps_its_area() {
    let eighth = Transform { angle: std::f32::consts::FRAC_PI_4, ..Transform::IDENTITY };
    let s = drawn(48, 48, Rect::new(14.0, 14.0, 20.0, 20.0), Paint::Color(7), eighth);
    let area = lit(&s, 48).len();
    assert!((380..=420).contains(&area), "{area}");
}

#[test]
fn a_half_turn_flips_a_sprite_both_ways_and_a_negative_scale_mirrors_like_flip() {
    let sprite = Sprite::test_ball();
    let img = sprite.image();
    let at = Rect::new(0.0, 0.0, 16.0, 16.0);
    let half = Transform { angle: std::f32::consts::PI, ..Transform::IDENTITY };
    let s = drawn(16, 16, at, Paint::Sprite(sprite.clone()), half);
    for y in 0..16 {
        for x in 0..16 {
            let p = img.px[(15 - y) * 16 + 15 - x];
            assert_eq!(s.buf[y * 16 + x], if p >> 24 == 255 { p } else { 0 }, "({x},{y})");
        }
    }
    let region = |flip| Paint::Region { sheet: sprite.clone(), src: Rect::new(2.0, 0.0, 10.0, 16.0), flip };
    let flipped = drawn(16, 16, at, region(true), Transform::IDENTITY);
    let mirrored = drawn(16, 16, at, region(false), Transform { sx: -1.0, ..Transform::IDENTITY });
    assert!(flipped.buf == mirrored.buf);
}

#[test]
fn upscaling_repeats_every_pixel_scale_times_in_both_directions() {
    let mut s = Soft::new(3, 2, 3);
    s.buf = vec![1, 2, 3, 4, 5, 6];
    let out = s.present().to_vec();
    assert_eq!(out.len(), 9 * 6);
    let row = |y: usize| &out[y * 9..y * 9 + 9];
    assert_eq!(row(0), [1, 1, 1, 2, 2, 2, 3, 3, 3]);
    assert_eq!(row(0), row(1));
    assert_eq!(row(1), row(2));
    assert_eq!(row(3), [4, 4, 4, 5, 5, 5, 6, 6, 6]);
    assert_eq!(row(5), row(3));
    let mut one = Soft::new(3, 2, 1);
    one.buf = vec![1, 2, 3, 4, 5, 6];
    assert_eq!(one.present(), [1, 2, 3, 4, 5, 6]);
}

#[test]
fn fill_alpha_blends_with_what_is_beneath() {
    let all = Rect::new(0.0, 0.0, 2.0, 1.0);
    let s = run(2, 1, |g| g.fill_alpha(all, 0xffffff, 128));
    assert_eq!(s.buf[0], 0x808080);
    let s = run(2, 1, |g| {
        g.fill(all, 0x808080);
        g.fill_alpha(all, 0x000000, 0);
    });
    assert_eq!(s.buf[1], 0x808080);
    let s = run(2, 1, |g| g.fill_alpha(all, 0x102030, 255));
    assert_eq!(s.buf[0], 0x102030);
}

#[test]
fn outline_leaves_interior_untouched_and_clear_fills_everything() {
    let s = run(10, 10, |g| g.outline(Rect::new(2.0, 2.0, 5.0, 5.0), 1));
    assert_eq!((s.buf[2 * 10 + 2], s.buf[6 * 10 + 6], s.buf[4 * 10 + 4]), (1, 1, 0));
    let s = run(4, 4, |g| g.clear(9));
    assert!(s.buf.iter().all(|&p| p == 9));
}

// Every pixel of the bounding box tested, none skipped: what the walk over just the inside of each row must equal.
fn warp_every_pixel(soft: &mut Soft, quad: Rect, t: &Transform, mut shade: impl FnMut(&mut u32, f32, f32)) {
    if t.sx == 0.0 || t.sy == 0.0 {
        return;
    }
    let Vec2 { x: cx, y: cy } = quad.center();
    let (hw, hh) = (quad.w / 2.0 * t.sx.abs(), quad.h / 2.0 * t.sy.abs());
    let (s, c) = t.angle.sin_cos();
    let (ex, ey) = (hw * c.abs() + hh * s.abs(), hw * s.abs() + hh * c.abs());
    let (x0, x1) = ((cx - ex).floor().max(0.0) as usize, ((cx + ex).ceil().max(0.0) as usize).min(soft.w));
    let (y0, y1) = ((cy - ey).floor().max(0.0) as usize, ((cy + ey).ceil().max(0.0) as usize).min(soft.h));
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    let (iu, iv) = (1.0 / t.sx, 1.0 / t.sy);
    let (du, dv) = (c * iu, -s * iv);
    for py in y0..y1 {
        let (dx, dy) = (x0 as f32 + 0.5 - cx, py as f32 + 0.5 - cy);
        let (u0, v0) = ((dx * c + dy * s) * iu + quad.w / 2.0, (dy * c - dx * s) * iv + quad.h / 2.0);
        let mut step = 0.0f32;
        for d in &mut soft.buf[py * soft.w + x0..py * soft.w + x1] {
            let (u, v) = (u0 + du * step, v0 + dv * step);
            step += 1.0;
            if u >= 0.0 && u < quad.w && v >= 0.0 && v < quad.h {
                shade(d, u, v);
            }
        }
    }
}

#[test]
fn walking_only_the_part_of_each_row_inside_the_quad_draws_exactly_what_walking_all_of_it_draws() {
    let mut rng = crate::Rng::new(11);
    let (w, h) = (64, 48);
    let mut rows_skipped = 0u32;
    for case in 0..4000 {
        let quad = Rect {
            x: rng.range(-40.0, 90.0),
            y: rng.range(-40.0, 70.0),
            w: rng.range(1.0, 40.0),
            h: rng.range(1.0, 40.0),
        };
        let angle = match case % 5 {
            0 => 0.0,
            1 => std::f32::consts::FRAC_PI_2,
            _ => rng.range(-7.0, 7.0),
        };
        let t = Transform { angle, sx: rng.range(-3.0, 3.0), sy: rng.range(-3.0, 3.0) };
        let paint = |d: &mut u32, u: f32, v: f32| *d = 0xFF00_0000 | (((u * 64.0) as u32 * 4099) ^ (v * 64.0) as u32);
        let (mut a, mut b) = (Soft::new(w, h, 1), Soft::new(w, h, 1));
        a.warp(quad, &t, paint);
        warp_every_pixel(&mut b, quad, &t, paint);
        assert!(a.buf == b.buf, "case {case}: {quad:?} {t:?}");
        rows_skipped += a.buf.iter().filter(|&&p| p == 0).count() as u32;
    }
    assert!(rows_skipped > 0);
}

fn styled_cmd(rect: Rect, paint: Paint, style: Style) -> DrawCmd {
    DrawCmd { style, ..DrawCmd::new(rect, paint) }
}

#[test]
fn a_style_flashes_fades_and_adds_flat_colours_and_a_plain_one_changes_nothing() {
    let at = Rect::new(1.0, 1.0, 2.0, 2.0);
    let draw = |cmds: &[DrawCmd]| {
        let mut s = Soft::new(4, 4, 1);
        let back = DrawCmd::new(Rect::new(0.0, 0.0, 4.0, 4.0), Paint::Color(0x204060));
        s.render(&[&[back][..], cmds].concat());
        s.buf[5]
    };
    let mut s = Soft::new(4, 4, 1);
    s.render(&[DrawCmd::new(at, Paint::Color(0x123456))]);
    assert_eq!(s.buf[5], 0x123456);
    let mut s = Soft::new(4, 4, 1);
    s.render(&[styled_cmd(at, Paint::Color(0xff0000), Style::PLAIN.faded(0))]);
    assert_eq!(s.buf[5], 0, "fully faded draws nothing");
    let mut s = Soft::new(4, 4, 1);
    s.render(&[styled_cmd(at, Paint::Color(0xff0000), Style::PLAIN.flashed(0x0000ff, 255))]);
    assert_eq!(s.buf[5], 0x0000ff, "a full flash is the flash colour");
    let mut s = Soft::new(4, 4, 1);
    s.render(&[styled_cmd(at, Paint::Color(0x000000), Style::PLAIN.flashed(0xffffff, 128))]);
    assert!((s.buf[5] & 255).abs_diff(128) <= 1, "{:x}", s.buf[5]);
    let faded = draw(&[styled_cmd(at, Paint::Color(0xff0000), Style::PLAIN.faded(128))]);
    assert!((faded >> 16 & 255).abs_diff(0x20 + (0xff - 0x20) / 2) <= 2, "{faded:x}");
    let lit = draw(&[styled_cmd(at, Paint::Color(0x808080), Style::PLAIN.additive())]);
    assert_eq!(lit, 0xA0C0E0, "added to what is behind");
    let lit = draw(&[
        styled_cmd(at, Paint::Color(0xffffff), Style::PLAIN.additive()),
        styled_cmd(at, Paint::Alpha(0xffffff, 255), Style::PLAIN.additive()),
    ]);
    assert_eq!(lit, 0xffffff, "stops at white");
    let half = draw(&[styled_cmd(at, Paint::Alpha(0xffffff, 128), Style::PLAIN.additive().faded(128))]);
    assert!((half & 255).abs_diff(0x60 + 64) <= 2, "a quarter of white is added: {half:x}");
}

#[test]
fn a_style_reaches_sprites_on_every_path_and_keeps_their_alpha() {
    let sheet = std::sync::Arc::new(Sprite::new(2, 1, vec![0xFF80_4020, 0x00FF_FFFF]));
    let sprite = |style: Style, xform: Transform| {
        let mut s = Soft::new(8, 4, 1);
        let cmd = DrawCmd { xform, ..styled_cmd(Rect::new(2.0, 1.0, 2.0, 1.0), Paint::Sprite(sheet.clone()), style) };
        s.render(&[cmd]);
        s
    };
    let plain = sprite(Style::PLAIN, Transform::IDENTITY);
    assert_eq!(
        (plain.buf[8 + 2] & 0xFF_FFFF, plain.buf[8 + 3]),
        (0x804020, 0),
        "the clear texel leaves the picture alone"
    );
    let flashed = sprite(Style::PLAIN.flashed(0xffffff, 255), Transform::IDENTITY);
    assert_eq!(
        (flashed.buf[8 + 2] & 0xFF_FFFF, flashed.buf[8 + 3]),
        (0xffffff, 0),
        "flashing keeps alpha, so the clear texel stays clear"
    );
    let faded = sprite(Style::PLAIN.faded(0), Transform::IDENTITY);
    assert_eq!(faded.buf[8 + 2] & 0xFF_FFFF, 0);
    // Scaled (no turn) and turned paths see the same style.
    for xform in [Transform { angle: 0.0, sx: 2.0, sy: 2.0 }, Transform { angle: 0.3, sx: 2.0, sy: 2.0 }] {
        let s = sprite(Style::PLAIN.flashed(0x00ff00, 255), xform);
        assert!(s.buf.iter().any(|&p| p & 0xFF_FFFF == 0x00ff00), "{xform:?}");
        assert!(
            s.buf.iter().all(|&p| p & 0xFF_FFFF == 0 || p & 0xFF_FFFF == 0x00ff00),
            "only the flash colour shows: {xform:?}"
        );
    }
    let lit = sprite(Style::PLAIN.additive(), Transform { angle: 0.0, sx: 2.0, sy: 2.0 });
    assert!(lit.buf.iter().any(|&p| p & 0xFF_FFFF == 0x804020));
}
