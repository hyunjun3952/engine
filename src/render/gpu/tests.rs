use super::*;
use crate::render::Soft;
use crate::{Gfx, Rect, Style, Tile, Tilemap};

fn scene() -> Gfx {
    let sprite = |p: &str| Arc::new(Sprite::load(p).unwrap());
    let mut g = Gfx::new(640, 360);
    g.cam = Vec2::new(120.0, 0.0);
    g.clear(0x6fa8e0);
    let map = Tilemap::load("assets/levels/level1.txt", 16, sprite("assets/images/tiles.png")).unwrap();
    g.draw(map.draw_list(g.view()));
    let player = sprite("assets/images/player.png");
    let frame = Rect::new(12.0, 24.0, 12.0, 24.0);
    g.blit_region(&player, frame, 50, 40, false);
    g.blit_region(&player, frame, 80, 40, true);
    g.blit(&sprite("assets/images/ball.png"), -4, 100);
    g.fill(Rect::new(300.0, 200.0, 50.0, 30.0), 0xff0000);
    g.fill_alpha(Rect::new(320.0, 210.0, 50.0, 30.0), 0x0000ff, 128);
    g.outline(Rect::new(400.0, 50.0, 40.0, 20.0), 0x00ff00);
    g.fill(Rect::new(-5.0, 350.0, 30.0, 30.0), 0xffff00);
    g.text("HELLO 123", 10, 10, 2, 0xffffff);
    g
}

fn dump(px: &[u32], path: &str) {
    let mut img = image::RgbImage::new(640, 360);
    for (i, p) in px.iter().enumerate() {
        img.put_pixel(i as u32 % 640, i as u32 / 640, image::Rgb([(p >> 16) as u8, (p >> 8) as u8, *p as u8]));
    }
    img.save(path).unwrap();
}

// Renders on both backends and fails if any pixel differs by more than the blending rounding.
fn assert_same(gpu: &mut Gpu, g: &Gfx, name: &str) {
    let mut soft = Soft::new(640, 360, 1);
    soft.render(g.draw_cmds());
    let gpu_px = gpu.render_to_pixels(g.draw_cmds());
    if let Ok(dir) = std::env::var("ENGINE_DUMP") {
        dump(&soft.buf, &format!("{dir}/soft{name}.png"));
        dump(&gpu_px, &format!("{dir}/gpu{name}.png"));
    }
    let channel = |p: u32, sh: u32| (p >> sh & 255) as i32;
    let off: Vec<usize> = (0..soft.buf.len())
        .filter(|&i| [16, 8, 0].iter().any(|&sh| (channel(soft.buf[i], sh) - channel(gpu_px[i], sh)).abs() > 2))
        .collect();
    assert!(
        off.is_empty(),
        "{name}: {} of {} pixels differ on {}, in x {:?} y {:?}, first at {:?}",
        off.len(),
        gpu_px.len(),
        gpu.adapter,
        off.iter().map(|i| i % 640).min().zip(off.iter().map(|i| i % 640).max()),
        off.iter().map(|i| i / 640).min().zip(off.iter().map(|i| i / 640).max()),
        off.first().map(|i| (i % 640, i / 640))
    );
}

#[test]
fn tiles_cut_from_a_sheet_taller_than_a_tile_match_between_the_backends() {
    let Ok(mut gpu) = Gpu::offscreen((640, 360), true) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let px = (0..64 * 32).map(|i| {
        let (x, y) = (i % 64, i / 64);
        let shade = (x as u32 / 16 + 1) * 60;
        if y < 16 { 0xFF00_0000 | (shade << 16) | ((shade / 2) << 8) } else { 0xFF00_FF00 }
    });
    let sheet = Arc::new(Sprite::new(64, 32, px.collect()));
    let map = Tilemap::from_ascii(&["#+#+", "+#+#"], 16, sheet);
    let mut g = Gfx::new(640, 360);
    g.draw(map.draw_list(g.view()));
    assert_same(&mut gpu, &g, "");
}

#[test]
fn gpu_output_matches_the_software_rasterizer() {
    let Ok(mut gpu) = Gpu::offscreen((640, 360), true) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    assert_same(&mut gpu, &scene(), "");
}

// Everything the camera zooms: tiles, sprites (some mirrored, some turned), flat and blended rects, an outline and
// world text, drawn through `Gfx::draw` the way a game does, plus one screen-space line that must not zoom.
fn zoomed_scene(zoom: f32) -> Gfx {
    let sprite = |p: &str| Arc::new(Sprite::load(p).unwrap());
    let mut world = Gfx::new(640, 360);
    let player = sprite("assets/images/player.png");
    let frame = Rect::new(12.0, 24.0, 12.0, 24.0);
    world.blit_region(&player, frame, 150, 100, false);
    world.blit_region(&player, frame, 180, 100, true);
    world.blit(&sprite("assets/images/ball.png"), 130, 160);
    world.fill(Rect::new(200.0, 150.0, 50.0, 30.0), 0xff0000);
    world.fill(Rect::new(250.0, 150.0, 17.0, 30.0), 0x00ff00);
    world.fill_alpha(Rect::new(220.0, 160.0, 50.0, 30.0), 0x0000ff, 128);
    world.outline(Rect::new(300.0, 120.0, 40.0, 20.0), 0x00ff00);
    world.text("HELLO 123", 160, 80, 1, 0xffffff);
    let mut turned = Gfx::new(640, 360);
    turned.blit_region(&player, frame, 120, 130, false);
    turned.fill(Rect::new(280.0, 170.0, 30.0, 14.0), 0xffaa00);
    let spin = Transform { angle: 0.7, sx: 1.0, sy: 1.0 };
    let spun: Vec<DrawCmd> = turned.draw_cmds().iter().map(|c| DrawCmd { xform: spin, ..c.clone() }).collect();
    let mut g = Gfx::new(640, 360);
    (g.zoom, g.cam) = (zoom, Vec2::new(60.0, 40.0));
    g.clear(0x6fa8e0);
    let map = Tilemap::load("assets/levels/level1.txt", 16, sprite("assets/images/tiles.png")).unwrap();
    g.draw(map.draw_list(g.view()));
    g.draw(world.draw_cmds().to_vec());
    g.draw(spun);
    g.text("HUD", 10, 10, 2, 0xffffff);
    g.line((20.0, 40.0), (90.0, 61.0), 0xff00ff);
    g.line((20.0, 70.0), (25.0, 120.0), 0x00ffff);
    g.circle((120.0, 60.0), 12.5, 0xffff00);
    g
}

#[test]
fn a_zoomed_scene_matches_between_the_software_and_gpu_rasterizers() {
    let Ok(mut gpu) = Gpu::offscreen((640, 360), true) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    for (zoom, name) in [(2.0, "_zoom2"), (1.5, "_zoom1_5"), (0.5, "_zoom0_5")] {
        assert_same(&mut gpu, &zoomed_scene(zoom), name);
    }
}

#[test]
fn a_reloaded_sprite_is_uploaded_again_and_still_matches_the_software_output() {
    let Ok(mut gpu) = Gpu::offscreen((64, 64), true) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let dir = std::env::temp_dir().join(format!("engine-gpu-reload-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("s.png");
    let path = path.to_str().unwrap();
    std::fs::copy("assets/images/ball.png", path).unwrap();
    let sprite = Arc::new(Sprite::load(path).unwrap());
    let draw = |gpu: &mut Gpu| {
        let mut g = Gfx::new(64, 64);
        g.blit(&sprite, 4, 4);
        let mut soft = Soft::new(64, 64, 1);
        soft.render(g.draw_cmds());
        let rgb: Vec<u32> = soft.buf.iter().map(|p| p & 0x00ff_ffff).collect();
        (rgb, gpu.render_to_pixels(g.draw_cmds()))
    };
    let (soft, hard) = draw(&mut gpu);
    assert!(soft == hard, "first upload differs from the software output");
    std::fs::copy("assets/images/player.png", path).unwrap();
    sprite.reload(path).unwrap();
    let (soft2, hard2) = draw(&mut gpu);
    assert!(soft != soft2, "the new image really looks different");
    assert!(soft2 == hard2, "re-uploaded texture differs from the software output");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn textures_of_dropped_sprites_are_released() {
    let Ok(mut gpu) = Gpu::offscreen((64, 64), true) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let kept = Sprite::test_ball();
    for _ in 0..20 {
        let temp = Arc::new(Sprite::load("assets/images/player.png").unwrap());
        let mut g = Gfx::new(64, 64);
        g.blit(&kept, 0, 0);
        g.blit(&temp, 20, 0);
        gpu.render_to_pixels(g.draw_cmds());
    }
    gpu.render_to_pixels(Gfx::new(64, 64).draw_cmds());
    assert!(gpu.textures.len() <= 1, "{} textures cached", gpu.textures.len());
}

fn transformed_scene() -> Gfx {
    let sprite = |p: &str| Arc::new(Sprite::load(p).unwrap());
    let (ball, player) = (sprite("assets/images/ball.png"), sprite("assets/images/player.png"));
    let mut g = Gfx::new(160, 120);
    g.clear(0x203050);
    let cmd = |rect, paint, angle, sx, sy| DrawCmd { xform: Transform { angle, sx, sy }, ..DrawCmd::new(rect, paint) };
    let at = |x, y, w, h| Rect { x, y, w, h };
    let frame = Rect::new(12.0, 24.0, 12.0, 24.0);
    g.draw([
        cmd(at(10.0, 10.0, 16.0, 16.0), Paint::Sprite(ball.clone()), 0.5, 1.0, 1.0),
        cmd(at(40.0, 10.0, 16.0, 16.0), Paint::Sprite(ball.clone()), 0.0, 3.0, 2.0),
        cmd(at(90.0, 10.0, 16.0, 16.0), Paint::Sprite(ball), 2.0, -1.5, 1.5),
        cmd(
            at(20.0, 50.0, 12.0, 24.0),
            Paint::Region { sheet: player.clone(), src: frame, flip: false },
            1.0,
            1.0,
            1.0,
        ),
        cmd(at(50.0, 50.0, 12.0, 24.0), Paint::Region { sheet: player.clone(), src: frame, flip: true }, 0.7, 2.0, 2.0),
        cmd(at(100.0, 50.0, 30.0, 12.0), Paint::Color(0xff4040), 0.4, 1.0, 1.0),
        cmd(at(110.0, 60.0, 30.0, 30.0), Paint::Alpha(0x40ff80, 128), -0.8, 1.0, 0.6),
    ]);
    let font = crate::Font::load("assets/fonts/galmuri9.bdf").unwrap();
    g.text_font(&font, "한글 text", 4, 100, 0xffee88);
    g.text_font_scaled(&font, "큰 글", 80, 98, 2, 0x88eeff);
    g
}

#[test]
fn rotated_and_scaled_drawing_agrees_between_the_two_backends() {
    let Ok(mut gpu) = Gpu::offscreen((160, 120), true) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let g = transformed_scene();
    let mut soft = Soft::new(160, 120, 1);
    soft.render(g.draw_cmds());
    let soft_px: Vec<u32> = soft.buf.iter().map(|p| p & 0x00ff_ffff).collect();
    let gpu_px = gpu.render_to_pixels(g.draw_cmds());
    if let Ok(dir) = std::env::var("ENGINE_DUMP") {
        let save = |px: &[u32], name: &str| {
            let mut img = image::RgbImage::new(160, 120);
            for (i, p) in px.iter().enumerate() {
                img.put_pixel(i as u32 % 160, i as u32 / 160, image::Rgb([(p >> 16) as u8, (p >> 8) as u8, *p as u8]));
            }
            img.save(format!("{dir}/{name}")).unwrap();
        };
        save(&soft_px, "xform_soft.png");
        save(&gpu_px, "xform_gpu.png");
    }
    let channel = |p: u32, sh: u32| (p >> sh & 255) as i32;
    let differ = |i: usize| [16, 8, 0].iter().any(|&sh| (channel(soft_px[i], sh) - channel(gpu_px[i], sh)).abs() > 2);
    let drawn = soft_px.iter().filter(|&&p| p != 0x203050).count();
    let off = (0..soft_px.len()).filter(|&i| differ(i)).count();
    eprintln!("transform parity: {off} of {drawn} drawn pixels differ on {}", gpu.adapter);
    assert!(off * 100 <= drawn * 3, "{off} of {drawn} drawn pixels differ");
}

#[test]
fn a_window_of_another_shape_gets_the_picture_centred_with_black_around_it() {
    let Ok(mut gpu) = Gpu::offscreen((64, 32), true) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let mut g = crate::Gfx::new(64, 32);
    g.clear(0xff0000);
    gpu.fit((128, 128));
    let px = gpu.render_to_pixels(g.draw_cmds());
    assert_eq!(px.len(), 128 * 128);
    let at = |x: usize, y: usize| px[y * 128 + x];
    // 2:1 picture at 2x in a square window: 32 rows of bar above and below it.
    assert_eq!((at(64, 10), at(64, 120)), (0, 0), "the bars stay black");
    assert_eq!((at(0, 40), at(127, 90), at(64, 64)), (0xff0000, 0xff0000, 0xff0000), "the picture fills the middle");
    assert_eq!((at(5, 31), at(5, 32), at(5, 95), at(5, 96)), (0, 0xff0000, 0xff0000, 0), "edges fall on whole rows");
    gpu.fit((0, 0));
    assert_eq!(gpu.target, (128, 128), "a minimized window changes nothing");
}

#[test]
fn styled_sprites_and_flat_colours_match_between_the_software_and_gpu_rasterizers() {
    let Ok(mut gpu) = Gpu::offscreen((640, 360), true) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let sprite = |p: &str| Arc::new(Sprite::load(p).unwrap());
    let (ball, player) = (sprite("assets/images/ball.png"), sprite("assets/images/player.png"));
    let frame = Rect::new(12.0, 24.0, 12.0, 24.0);
    let mut g = Gfx::new(640, 360);
    g.clear(0x203050);
    g.fill(Rect::new(0.0, 60.0, 640.0, 40.0), 0x806040);
    let styles = [
        Style::PLAIN,
        Style::PLAIN.flashed(0xffffff, 128),
        Style::PLAIN.flashed(0xff0000, 255),
        Style::PLAIN.faded(100),
        Style::PLAIN.additive(),
        Style::PLAIN.additive().faded(120),
        Style::PLAIN.flashed(0x00ff80, 90).faded(200),
        Style::PLAIN.multiplied(),
        Style::PLAIN.multiplied().faded(120),
        Style::PLAIN.multiplied().flashed(0xff8000, 100),
    ];
    let turns =
        [Transform::IDENTITY, Transform { angle: 0.0, sx: 2.0, sy: 2.0 }, Transform { angle: 0.5, sx: 1.0, sy: 1.0 }];
    let mut cmds = Vec::new();
    for (row, turn) in turns.iter().enumerate() {
        for (i, style) in styles.iter().enumerate() {
            let (x, y) = (10.0 + i as f32 * 60.0, 10.0 + row as f32 * 90.0);
            let sheet_cmd = |at: Rect, paint| DrawCmd { xform: *turn, style: *style, ..DrawCmd::new(at, paint) };
            cmds.push(sheet_cmd(Rect { x, y, w: 16.0, h: 16.0 }, Paint::Sprite(ball.clone())));
            let region = Paint::Region { sheet: player.clone(), src: frame, flip: i % 2 == 1 };
            cmds.push(sheet_cmd(Rect { x: x + 20.0, y, w: 12.0, h: 24.0 }, region));
            cmds.push(sheet_cmd(Rect { x, y: y + 40.0, w: 20.0, h: 14.0 }, Paint::Color(0xc08040)));
            cmds.push(sheet_cmd(Rect::new(x + 24.0, y + 40.0, 20.0, 14.0), Paint::Alpha(0x4080c0, 150)));
        }
    }
    g.draw(cmds);
    assert_same(&mut gpu, &g, "_styles");
}

#[test]
fn a_lit_scene_with_a_vignette_and_scanlines_matches_between_the_backends_and_follows_a_moving_light() {
    let Ok(mut gpu) = Gpu::offscreen((640, 360), true) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let mut map = crate::LightMap::new(640, 360);
    let (vignette, lines) = (crate::vignette(640, 360, 0.8), crate::scanlines(640, 360, 50));
    for (frame, x) in [200.0, 420.0].into_iter().enumerate() {
        let mut g = zoomed_scene(1.0);
        map.clear(0x303850);
        map.add(crate::Light::new((x, 150.0), 90.0).colored(0xffd080));
        map.add(crate::Light::new((300.0, 250.0), 60.0).colored(0x4080ff).intensity(0.7));
        map.draw(&mut g);
        g.multiply(&vignette);
        g.multiply(&lines);
        g.text("HUD after the light", 10, 340, 1, 0xffffff);
        assert_same(&mut gpu, &g, &format!("_lit{frame}"));
    }
}

#[test]
fn tiles_from_the_second_row_of_a_sheet_match_between_the_backends() {
    let Ok(mut gpu) = Gpu::offscreen((640, 360), true) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let px = (0..64 * 32).map(|i| {
        let (x, y) = (i % 64, i / 64);
        let id = (y / 16) * 4 + x / 16;
        0xFF00_0000 | ((id as u32 * 31) << 16) | ((255 - id as u32 * 30) << 8) | ((x as u32 % 7) * 20)
    });
    let sheet = Arc::new(Sprite::new(64, 32, px.collect()));
    let legend: Vec<Tile> = b"12345678"
        .iter()
        .enumerate()
        .map(|(i, &ch)| Tile {
            ch,
            id: i as u8 + 1,
            solid: false,
            front: false,
            auto: false,
            one_way: false,
            slope: None,
        })
        .collect();
    let map = Tilemap::try_from_ascii_with(&["1234", "5678", "8765"], 16, sheet, &legend).unwrap();
    let mut g = Gfx::new(640, 360);
    g.draw(map.draw_list(g.view()));
    assert_same(&mut gpu, &g, "_rows");
    (g.zoom, g.cam) = (1.5, Vec2::new(4.0, 3.0));
    g.clear_cmds();
    g.draw(map.draw_list(g.view()));
    assert_same(&mut gpu, &g, "_rows_zoom");
}
