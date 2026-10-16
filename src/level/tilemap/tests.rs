use super::*;

// A sheet of 20 tiles of 2x2 pixels, enough for ids 1..=20.
fn wide_sheet() -> Arc<Sprite> {
    Arc::new(Sprite::new(40, 2, vec![0xFFFF_FFFF; 80]))
}

fn shown(map: &Tilemap) -> Vec<Vec<u8>> {
    let mut grid = vec![vec![0; map.w]; map.h];
    for c in map.draw_list(Rect::new(0.0, 0.0, (map.w * map.size) as f32, (map.h * map.size) as f32)) {
        if let Paint::Tiles { tiles, .. } = &c.paint {
            tiles.iter().for_each(|&(x, y, id)| grid[y as usize][x as usize] = id);
        }
    }
    grid
}

#[test]
fn an_autotile_shows_the_tile_that_matches_its_joined_neighbours() {
    let legend = [Tile { ch: b'#', id: 3, solid: true, front: false, auto: true, one_way: false, slope: None }];
    let map = Tilemap::try_from_ascii_with(&["###", "#.#", "###", "..."], 2, wide_sheet(), &legend).unwrap();
    // Masks, row by row: 15 11 15 / 13 - 7 / 11 10 11 (the map edge counts as joined, a gap or empty cell does not).
    let want = [[18, 14, 18], [16, 0, 10], [14, 13, 14], [0, 0, 0]];
    assert_eq!(shown(&map), want.map(|r| r.to_vec()).to_vec());
}

#[test]
fn an_autotile_must_fit_the_sheet_and_own_its_sixteen_ids() {
    let auto = |id| Tile { ch: b'#', id, solid: true, front: false, auto: true, one_way: false, slope: None };
    let err = Tilemap::try_from_ascii_with(&["#"], 2, wide_sheet(), &[auto(6)]).err().unwrap();
    assert!(err.contains("needs tiles 6..=21"), "{err}");
    let other = Tile { ch: b'+', id: 9, solid: true, front: false, auto: false, one_way: false, slope: None };
    let err = Tilemap::try_from_ascii_with(&["#"], 2, wide_sheet(), &[auto(3), other]).err().unwrap();
    assert!(err.contains("owns"), "{err}");
    assert!(Tilemap::try_from_ascii_with(&["#"], 2, wide_sheet(), &[auto(5)]).is_ok());
    let full = Arc::new(Sprite::new(510, 2, vec![0xFFFF_FFFF; 1020]));
    assert!(Tilemap::try_from_ascii_with(&["#"], 2, full, &[auto(240)]).is_ok(), "the last 16 ids fit");
}

#[test]
fn an_autotile_in_front_draws_all_its_variants_in_front() {
    let front = Tile { ch: b'#', id: 3, solid: false, front: true, auto: true, one_way: false, slope: None };
    let map = Tilemap::try_from_ascii_with(&["#.#"], 2, wide_sheet(), &[front]).unwrap();
    let cmds = map.draw_list(Rect::new(0.0, 0.0, 6.0, 2.0));
    assert_eq!((cmds.len(), cmds[0].layer), (1, i32::MAX - 1));
}

#[test]
fn a_last_row_that_ends_in_spaces_loads_and_non_ascii_text_is_refused() {
    let path = std::env::temp_dir().join(format!("engine-level-{}.txt", std::process::id()));
    let path = path.to_str().unwrap();
    std::fs::write(path, "....\r\n#.  \r\n    \n\n").unwrap();
    let map = Tilemap::load(path, 16, Sprite::test_tiles()).unwrap();
    assert_eq!((map.w, map.h), (4, 3));
    std::fs::remove_file(path).unwrap();
    let err = Tilemap::try_from_ascii(&["é.#"], 16, Sprite::test_tiles()).err().unwrap();
    assert!(err.contains("not ASCII"), "{err}");
}

#[test]
fn letters_become_markers_with_pixel_positions() {
    let map = Tilemap::from_ascii(&["...", ".P.", "###"], 16, Sprite::test_tiles());
    assert_eq!(map.marks(b'P').collect::<Vec<_>>(), vec![Vec2::new(16.0, 16.0)]);
    assert_eq!(map.marks(b'E').count(), 0);
}

#[test]
fn a_broken_level_is_an_error_not_a_crash() {
    let sheet = Sprite::test_tiles();
    let err = Tilemap::try_from_ascii(&["###", "##"], 16, sheet.clone()).err().unwrap();
    assert!(err.contains("row 2"), "{err}");
    assert!(Tilemap::try_from_ascii(&[], 16, sheet.clone()).is_err());
    let err = Tilemap::load("assets/nope.txt", 16, sheet).err().unwrap();
    assert!(err.starts_with("assets/nope.txt"), "{err}");
}

#[test]
fn level_file_loads_with_a_start_marker() {
    let map = Tilemap::load("assets/levels/level1.txt", 16, Sprite::test_tiles()).unwrap();
    assert_eq!((map.w, map.h), (60, 23));
    assert_eq!(map.marks(b'P').count(), 1);
}

#[test]
fn draw_list_covers_only_the_visible_solid_tiles_in_world_space() {
    let map = Tilemap::from_ascii(&["#.#", "...", "..."], 16, Sprite::test_tiles());
    let at = |view| {
        map.draw_list(view)
            .iter()
            .flat_map(|c| match &c.paint {
                Paint::Tiles { size, tiles, .. } => tiles
                    .iter()
                    .map(|&(gx, gy, _)| (c.rect.x + (gx * size) as f32, c.rect.y + (gy * size) as f32))
                    .collect(),
                _ => Vec::new(),
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(at(Rect::new(0.0, 0.0, 48.0, 48.0)), [(0.0, 0.0), (32.0, 0.0)]);
    assert_eq!(at(Rect::new(0.0, 0.0, 16.0, 16.0)), [(0.0, 0.0)]);
    assert_eq!(at(Rect::new(20.0, 0.0, 40.0, 16.0)), [(32.0, 0.0)]);
    assert!(map.draw_list(Rect::new(0.0, 32.0, 48.0, 16.0)).is_empty());
    assert_eq!(map.draw_list(Rect::new(0.0, 0.0, 48.0, 48.0)).len(), 1);
}

#[test]
fn falling_box_lands_on_floor_and_wall_stops_it() {
    let map = Tilemap::from_ascii(&["...", "...", "###"], 16, Sprite::test_tiles());
    let Slide { rect: r, blocked_x: hx, blocked_y: hy } = map.slide(Rect::new(0.0, 20.0, 10.0, 10.0), 0.0, 5.0);
    assert_eq!((r.y, hx, hy), (22.0, false, true));
    let Slide { rect: r, blocked_x: hx, blocked_y: _ } = map.slide(Rect::new(1.0, 0.0, 10.0, 10.0), -3.0, 0.0);
    assert_eq!((r.x, hx), (0.0, true));
}

#[test]
fn set_replaces_a_tile_and_gives_the_map_a_new_stamp() {
    let sheet = Sprite::test_tiles();
    let mut map = Tilemap::from_ascii(&["...", "###"], 16, sheet);
    let before = map.stamp();
    map.set((1, 0), 2);
    assert_eq!((map.tile((1, 0)), map.tile((1, 1)), map.tile((0, 0))), (2, 1, 0));
    assert!(map.stamp() != before);
    assert!(!map.solid_in(Rect::new(16.0, 0.0, 16.0, 16.0)).is_empty());
}

fn legend() -> [Tile; 3] {
    [
        Tile::DEFAULT[0],
        Tile { ch: b':', id: 3, solid: false, front: false, auto: false, one_way: false, slope: None },
        Tile { ch: b'v', id: 4, solid: false, front: true, auto: false, one_way: false, slope: None },
    ]
}

#[test]
fn a_legend_makes_tiles_that_do_not_block_and_tiles_drawn_in_front() {
    let sheet = Sprite::test_tiles();
    let map = Tilemap::try_from_ascii_with(&["P.v", ":::", "###"], 16, sheet, &legend()).unwrap();
    assert_eq!(map.marks(b'P').count(), 1, "characters outside the legend are still markers");
    assert_eq!(map.solid_in(Rect::new(0.0, 0.0, 48.0, 48.0)).len(), 3, "only the ground blocks");
    let Slide { rect: moved, blocked_x: hit_x, blocked_y: hit_y } =
        map.slide(Rect::new(4.0, 20.0, 8.0, 8.0), 10.0, 0.0);
    assert!(!hit_x && !hit_y && moved.x == 14.0, "walks through the background wall");
    let cmds = map.draw_list(Rect::new(0.0, 0.0, 48.0, 48.0));
    assert_eq!(cmds.iter().map(|c| c.layer).collect::<Vec<_>>(), [i32::MIN, FRONT_LAYER]);
    let count = |c: &DrawCmd| match &c.paint {
        Paint::Tiles { tiles, .. } => tiles.len(),
        _ => 0,
    };
    assert_eq!((count(&cmds[0]), count(&cmds[1])), (6, 1), "behind: wall and ground, in front: the leaf");
}

#[test]
fn a_one_way_tile_stops_what_falls_onto_it_and_lets_everything_else_through() {
    let one_way = Tile { ch: b'=', id: 3, solid: false, front: false, auto: false, one_way: true, slope: None };
    let map =
        Tilemap::try_from_ascii_with(&["...", "...", ".=.", "..."], 16, Sprite::test_tiles(), &[one_way]).unwrap();
    assert!(map.one_way((1, 2)) && !map.one_way((0, 2)) && !map.one_way_at(-1, 2) && !map.one_way((1, 9)));
    let body = |x: f32, y: f32| Rect { x, y, w: 8.0, h: 8.0 };
    let Slide { rect: landed, blocked_x: _, blocked_y: hit } = map.slide(body(20.0, 20.0), 0.0, 10.0);
    assert!(hit && landed.y == 24.0, "lands on the top face: {landed:?}");
    let Slide { rect: up, blocked_x: _, blocked_y: hit } = map.slide(body(20.0, 40.0), 0.0, -12.0);
    assert!(!hit && up.y == 28.0, "rises through it from below");
    let Slide { rect: side, blocked_x: hit, blocked_y: _ } = map.slide(body(4.0, 34.0), 20.0, 0.0);
    assert!(!hit && side.x == 24.0, "walks through it sideways");
    let Slide { rect: inside, blocked_x: _, blocked_y: hit } = map.slide(body(20.0, 28.0), 0.0, 6.0);
    assert!(!hit && inside.y == 34.0, "a body already sunk into it keeps falling");
}

#[test]
fn a_slope_tile_reports_its_corner_and_counts_as_a_tile_for_the_rest() {
    let slope = Tile {
        ch: b'/',
        id: 3,
        solid: false,
        front: false,
        auto: false,
        one_way: false,
        slope: Some(Slope::BottomRight),
    };
    let map =
        Tilemap::try_from_ascii_with(&["...", ".//", "###"], 16, Sprite::test_tiles(), &[Tile::DEFAULT[0], slope])
            .unwrap();
    assert_eq!(map.slope((1, 1)), Some(Slope::BottomRight));
    assert_eq!(
        (map.slope((0, 1)), map.slope((0, 2)), map.slope_at(-1, 1), map.slope((1, 9))),
        (None, None, None, None)
    );
    assert_eq!(map.solid_in(Rect::new(16.0, 16.0, 32.0, 16.0)).len(), 2, "it still occupies its cell");
    let triangle = Slope::BottomRight.triangle(&Rect::new(0.0, 0.0, 16.0, 16.0));
    assert_eq!(triangle, [(16.0, 16.0), (0.0, 16.0), (16.0, 0.0)]);
    let top = Slope::TopLeft.triangle(&Rect::new(0.0, 0.0, 16.0, 16.0));
    assert_eq!(top, [(0.0, 0.0), (16.0, 0.0), (0.0, 16.0)]);
}

fn slope_map(rows: &[&str]) -> Tilemap {
    let tile = |ch, id, slope| Tile { ch, id, solid: false, front: false, auto: false, one_way: false, slope };
    let legend = [
        Tile::DEFAULT[0],
        tile(b'/', 2, Some(Slope::BottomRight)),
        tile(b'\\', 3, Some(Slope::BottomLeft)),
        tile(b'^', 4, Some(Slope::TopLeft)),
    ];
    Tilemap::try_from_ascii_with(rows, 16, Sprite::test_tiles(), &legend).unwrap()
}

// Rising to the right over two tiles from the floor (y = 64 at x = 32) to a plateau (y = 32 from x = 64).
fn ramp() -> Tilemap {
    slope_map(&["........", "........", ".../####", "../#####", "########"])
}

#[test]
fn a_box_walks_up_a_ramp_onto_the_plateau_standing_on_the_diagonal_all_the_way() {
    let map = ramp();
    let mut r = Rect::new(4.0, 54.0, 10.0, 10.0);
    let mut worst = 0.0f32;
    for _ in 0..40 {
        let Slide { rect: moved, blocked_x: hit_x, blocked_y: hit_y } = map.slide(r, 2.0, 2.0);
        let line = (64.0 - (moved.x + moved.w - 32.0)).clamp(32.0, 64.0);
        if moved.x + moved.w > 34.0 && moved.x + moved.w < 64.0 {
            worst = worst.max((moved.y + moved.h - line).abs());
        }
        assert!(!hit_x && hit_y, "grounded the whole way up: {moved:?}");
        r = moved;
    }
    assert!(worst < 0.02, "its lower right corner rode the line: {worst}");
    assert_eq!((r.y + r.h, r.x > 70.0), (32.0, true), "it ends on the plateau: {r:?}");
}

#[test]
fn a_box_walks_down_a_ramp_without_leaving_the_diagonal() {
    let map = slope_map(&["........", "........", "####\\...", "#####\\..", "########"]);
    let mut r = Rect::new(40.0, 22.0, 10.0, 10.0);
    for _ in 0..40 {
        let Slide { rect: moved, blocked_x: _, blocked_y: hit_y } = map.slide(r, 2.0, 2.0);
        assert!(hit_y, "it never floats above the slope on the way down: {moved:?}");
        r = moved;
    }
    assert_eq!(r.y + r.h, 64.0, "it ends on the floor: {r:?}");
}

#[test]
fn a_slope_is_climbed_from_its_low_end_and_its_tall_side_is_a_wall() {
    let Slide { rect: moved, blocked_x: hit_x, blocked_y: _ } =
        ramp().slide(Rect::new(20.0, 54.0, 10.0, 10.0), 8.0, 0.0);
    assert!(!hit_x && (moved.y + moved.h - 58.0).abs() < 0.01, "from the low end it climbs: {moved:?}");
    let wall = slope_map(&["........", ".......\\", "########"]);
    let Slide { rect: stopped, blocked_x: hit, blocked_y: _ } =
        wall.slide(Rect::new(96.0, 22.0, 10.0, 10.0), 10.0, 0.0);
    assert!(hit && stopped.x == 102.0, "the left leg of a falling slope stops a box walking into it: {stopped:?}");
}

#[test]
fn a_box_falls_onto_a_slope_and_rises_into_the_flat_underside() {
    let map = ramp();
    let Slide { rect: landed, blocked_x: _, blocked_y: hit_y } =
        map.slide(Rect::new(44.0, 20.0, 10.0, 10.0), 0.0, 14.0);
    assert!(hit_y && (landed.y + landed.h - 42.0).abs() < 0.01, "lands on the line at its right edge: {landed:?}");
    let floating = slope_map(&["........", "........", "..///...", "........"]);
    let Slide { rect: stopped, blocked_x: _, blocked_y: hit } =
        floating.slide(Rect::new(40.0, 60.0, 10.0, 10.0), 0.0, -20.0);
    assert!(hit && stopped.y == 48.0, "stops below the flat bottom of the tile: {stopped:?}");
}

#[test]
fn a_ceiling_slope_pushes_a_box_down_along_its_diagonal() {
    let map = slope_map(&["^.......", "........", "........", "########"]);
    let Slide { rect: moved, blocked_x: _, blocked_y: hit_y } = map.slide(Rect::new(2.0, 20.0, 8.0, 8.0), 0.0, -14.0);
    assert!(hit_y && (moved.y - 14.0).abs() < 0.01, "its top rests on the diagonal at its left edge: {moved:?}");
}

#[test]
fn maps_without_slopes_slide_exactly_as_before() {
    let map = Tilemap::from_ascii(&["......", "......", "######"], 16, Sprite::test_tiles());
    let Slide { rect: moved, blocked_x: hit_x, blocked_y: hit_y } = map.slide(Rect::new(4.0, 20.0, 8.0, 8.0), 3.0, 9.0);
    assert_eq!((moved.x, moved.y, hit_x, hit_y), (7.0, 24.0, false, true));
}

#[test]
fn a_legend_that_hides_empty_tiles_is_an_error() {
    let sheet = Sprite::test_tiles();
    let bad = [Tile { ch: b'.', id: 1, solid: true, front: false, auto: false, one_way: false, slope: None }];
    assert!(Tilemap::try_from_ascii_with(&["#"], 16, sheet.clone(), &bad).is_err());
    let zero = [Tile { ch: b'#', id: 0, solid: true, front: false, auto: false, one_way: false, slope: None }];
    assert!(Tilemap::try_from_ascii_with(&["#"], 16, sheet, &zero).is_err());
}

#[test]
fn a_tile_the_sheet_does_not_have_is_refused_instead_of_crashing_the_renderer() {
    let sheet = Sprite::test_tiles();
    let columns = tile_count(&sheet, 16);
    let past =
        [Tile { ch: b'#', id: columns as u8 + 1, solid: true, front: false, auto: false, one_way: false, slope: None }];
    let err = Tilemap::try_from_ascii_with(&["#"], 16, sheet.clone(), &past).err().unwrap();
    assert!(err.contains("the sheet has"), "{err}");
    let mut map = Tilemap::from_ascii(&["..", "##"], 16, sheet);
    map.set((0, 0), columns as u8);
    map.set((1, 0), 0);
    assert_eq!(map.tile((0, 0)), columns as u8);
}

#[test]
#[should_panic(expected = "does not exist")]
fn setting_a_tile_past_the_sheet_panics_at_the_call_not_when_drawn() {
    let sheet = Sprite::test_tiles();
    let mut map = Tilemap::from_ascii(&["."], 16, sheet);
    map.set((0, 0), 200);
}

#[test]
fn a_view_past_the_map_a_cell_outside_it_and_bad_sizes_do_not_panic() {
    let map = Tilemap::from_ascii(&["#.", "#."], 2, wide_sheet());
    assert!(map.draw_list(Rect::new(1000.0, 1000.0, 64.0, 64.0)).is_empty());
    assert_eq!((map.tile((2, 0)), map.tile((0, 5))), (0, 0));
    assert!(Tilemap::try_from_ascii(&["#"], 0, wide_sheet()).is_err());
    let wide = Arc::new(Sprite::new(600, 2, vec![0; 1200]));
    let auto = Tile { ch: b'#', id: 250, solid: true, front: false, auto: true, one_way: false, slope: None };
    assert!(Tilemap::try_from_ascii_with(&["#"], 2, wide, &[auto]).is_err());
}

// Marching along the ray in small steps finds the same tile a little later than the exact walk does.
#[test]
fn a_ray_finds_the_tile_a_fine_march_finds_on_random_maps() {
    let mut rng = crate::Rng::new(9);
    for case in 0..200 {
        let rows: Vec<String> =
            (0..8).map(|_| (0..10).map(|_| if rng.f32() < 0.25 { '#' } else { '.' }).collect()).collect();
        let map = Tilemap::from_ascii(&rows.iter().map(String::as_str).collect::<Vec<_>>(), 4, wide_sheet());
        let from = Vec2::new(rng.range(-6.0, 46.0), rng.range(-6.0, 38.0));
        let dir = Vec2::from_angle(rng.range(0.0, 6.3)) * rng.range(0.5, 3.0);
        let max = rng.range(1.0, 60.0);
        let step = 0.01;
        let marched = (0..).map(|i| i as f32 * step).take_while(|t| *t <= max).find(|t| {
            let p = from + dir * *t;
            map.solid((p.x / 4.0).floor() as i32, (p.y / 4.0).floor() as i32)
        });
        let hit = map.raycast(from, dir, max);
        match (hit, marched) {
            (None, None) => {}
            (Some(h), Some(m)) => {
                assert!(
                    m >= h.t - 1e-3 && m - h.t <= 0.05 / dir.length().min(1.0),
                    "case {case}: exact {} marched {m}",
                    h.t
                );
                let inside = from + dir * (h.t + 0.02);
                assert_eq!(((inside.x / 4.0).floor() as i32, (inside.y / 4.0).floor() as i32), h.tile, "case {case}");
            }
            (h, m) => panic!("case {case}: exact {h:?} marched {m:?} for {from:?} {dir:?} {max}"),
        }
    }
}

#[test]
fn a_ray_reports_the_face_it_crossed_and_stops_where_it_cannot_hit_anything() {
    let map = Tilemap::from_ascii(&["....", "..#.", "...."], 10, wide_sheet());
    let hit = map.raycast((5.0, 15.0), (1.0, 0.0), 100.0).unwrap();
    assert_eq!((hit.t, hit.tile, hit.normal), (15.0, (2, 1), Vec2::new(-1.0, 0.0)));
    let down = map.raycast((25.0, 0.0), (0.0, 1.0), 100.0).unwrap();
    assert_eq!((down.t, down.normal), (10.0, Vec2::new(0.0, -1.0)));
    assert_eq!(map.raycast((25.0, 15.0), (1.0, 0.0), 100.0).unwrap().t, 0.0, "starting inside");
    assert!(map.raycast((5.0, 15.0), (1.0, 0.0), 10.0).is_none(), "short of it");
    assert_eq!(map.raycast((5.0, 5.0), (-1.0, 0.0), f32::INFINITY).unwrap().tile, (-1, 0), "the map edge is solid");
    assert!(map.raycast((5.0, 5.0), (0.0, -1.0), f32::INFINITY).is_none(), "straight up out of the map");
    assert!(
        map.raycast((5.0, 5.0), (0.0, 0.0), 10.0).is_none() && map.raycast((f32::NAN, 0.0), (1.0, 0.0), 9.0).is_none()
    );
    assert_eq!(map.raycast((5.0, -50.0), (1.0, 0.0), f32::INFINITY).unwrap().tile, (4, -5), "the side walls reach up");
}

#[test]
fn paths_avoid_solid_tiles_and_can_be_refused_tiles_too() {
    let map = Tilemap::from_ascii(&["....", ".##.", "...."], 10, wide_sheet());
    let path = map.path((0, 0), (3, 2), false).unwrap();
    assert!(path.iter().all(|&(x, y)| map.tile((x, y)) == 0) && path.len() == 6, "{path:?}");
    assert!(map.path((0, 0), (1, 1), true).is_none(), "the goal is solid");
    let blocked = map.path_with((0, 0), (3, 0), false, |x, y| (x, y) != (1, 0));
    assert!(blocked.is_some_and(|p| p.len() > 4), "it goes around a tile the game ruled out");
    assert_eq!(
        (map.cell_of((25.0, 5.0)), map.cell_of((-1.0, 5.0)), map.cell_of((40.0, 5.0))),
        (Some((2, 0)), None, None)
    );
    assert_eq!(map.cell_center((1, 2)), Vec2::new(15.0, 25.0));
}

#[test]
fn a_sheet_with_several_rows_shows_the_right_tile_for_each_id() {
    let mut px = vec![0; 16];
    for (n, color) in [0xFF0000, 0x00FF00, 0x0000FF, 0xFFFF00].into_iter().enumerate() {
        let (x, y) = (n % 2 * 2, n / 2 * 2);
        for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            px[(y + dy) * 4 + x + dx] = 0xFF00_0000 | color;
        }
    }
    let sheet = Arc::new(Sprite::new(4, 4, px));
    let legend: Vec<Tile> = b"abcd"
        .iter()
        .enumerate()
        .map(|(i, &ch)| Tile {
            ch,
            id: i as u8 + 1,
            solid: true,
            front: false,
            auto: false,
            one_way: false,
            slope: None,
        })
        .collect();
    let map = Tilemap::try_from_ascii_with(&["abcd"], 2, sheet, &legend).unwrap();
    let mut soft = crate::render::Soft::new(8, 2, 1);
    soft.render(&map.draw_list(Rect::new(0.0, 0.0, 8.0, 2.0)));
    let colors: Vec<u32> = [0, 2, 4, 6].iter().map(|&x| soft.buf[x] & 0xFF_FFFF).collect();
    assert_eq!(colors, [0xFF0000, 0x00FF00, 0x0000FF, 0xFFFF00]);
}

#[test]
fn a_long_move_does_not_skip_a_thin_floor() {
    let sheet = Arc::new(Sprite::new(16, 8, vec![0; 128]));
    let map = Tilemap::from_ascii(&["....", "....", "####", "...."], 8, sheet);
    let s = map.slide(Rect::new(4.0, 0.0, 6.0, 6.0), 0.0, 40.0);
    assert_eq!((s.rect.y, s.blocked_y), (10.0, true));
}
