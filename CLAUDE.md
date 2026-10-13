# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

    cargo test                          # unit tests; run from the repo root, they load assets/ by relative path
    cargo test <name>                   # single test, e.g. cargo test falling_box
    cargo test --release -- --ignored --nocapture   # benchmarks in src/bench.rs (`bench`, and `shake` with SEEDS=40 for more seeds)
    cargo run --example camera          # also: physics, raycast, text (F3 toggles the debug overlay)
    cargo fmt && cargo clippy --all-targets -- -D warnings
    cargo test --no-default-features    # lean build: software renderer, no audio

`ENGINE_MUTE=1` silences audio, `ENGINE_STATS=1` logs frame timings, `ENGINE_RENDERER=soft|gpu|auto` picks the renderer, `ENGINE_SCALE=n` scales the window, `ENGINE_RECORD=path` / `ENGINE_REPLAY=path` record and replay input. The window needs an X display (WSLg works); `minifb` is built with the `x11` feature only, because its Wayland backend spams EGL/Zink errors under WSL.

Commits are checked by pre-commit (`pre-commit install`): Conventional Commits (`feat|fix|refactor|perf|test|docs|chore|ci|build`, optional scope), plus `cargo fmt --check` and clippy with `-D warnings` for both the default and the `--no-default-features` build. There is no CI; run `cargo test` yourself.

## Project conventions

- Comments are allowed but rare: add one only where the code cannot show *why* (a non-obvious constraint, workaround or invariant). Keep it to one short, plain line. Never restate what the code does, and skip doc comments on self-explanatory items. The owner dislikes comment-heavy code.
- `rustfmt.toml` uses `max_width = 120` and `use_small_heuristics = "Max"`. Run `cargo fmt` rather than formatting by hand.
- Keep changes minimal; measure before optimizing (see `src/bench.rs`). Put measurements in commit messages, not here.
- Reuse before writing: `Rect::center/contains/edges/penetration/overlaps`, `Sprite::bounds`, `DrawCmd::new`, `Paint::region`, `assets::read_text(_if_exists)` and `write_atomic`, and the `sub/dot/apply/shove` helpers in `physics/mod.rs` already exist.
- Prefer in-house code or tiny crates over new dependencies: build time is a stated concern. Code touching `wgpu` or `rodio` stays behind `#[cfg(feature = "gpu" | "audio")]`; the lean build must keep compiling.

## Architecture

`engine` is a library crate. Source is grouped by area (`app/`, `render/`, `ecs/`, `assets/`) and `src/lib.rs` re-exports the public API at the crate root, so games and internal code use flat `engine::X` / `crate::X` paths. Games use only that API, never `minifb` directly.

**Loop and scenes (`app/`).** `run(cfg, first)` owns the window, a fixed-timestep accumulator (`Config::tick_hz`) and a scene stack. A scene implements `update(&mut Ctx) -> Trans` and `draw(&Gfx)`. Only the top scene updates; every scene draws, bottom first. `Trans` is `None/Push/Pop/Replace/Quit/Fade`. `on_enter` runs when a scene joins the stack and `on_exit` when it leaves; a `Push`ed overlay does not touch the scene beneath. Game scenes call `ctx.cam().reset()` in `on_enter`, and anything that outlives a scene (music) is stopped in `on_exit`. The engine does not handle Esc; scenes do.
- `Ctx` fields are crate-private and reached through accessors (`input()`, `assets()`, `audio()`, `cam()`, `rng()`, `dt()`, `size()`). Each borrows the whole `Ctx`: read `let dt = ctx.dt();` into a local before `ctx.cam().follow(.., dt)`, never nest them.
- One fixed step is the function `step` (replay input, fade tick, top scene update unless a fade is darkening, `end_step`, camera and audio ticks), so the order is tested with fake scenes without a window. `run` only adds window, drawing and timing.
- `Trans::Fade(inner, secs)` darkens over half of `secs`, applies `inner` at full black, then brightens. The top scene does not update while it darkens.
- `run` draws only on frames that ran a tick (and the first, and after a resize); otherwise it sleeps until the next tick. F3 is tracked in `run` itself because `Input::pressed` refreshes only once per step.
- `run` panics naming the field if `tick_hz` is not positive or `w`/`h` is 0.

**Rendering (`render/`).** `Gfx` does not draw: it records `DrawCmd`s into a per-frame list in screen space, and `run` hands the list to a backend. `DrawCmd` is the whole contract, so a backend only has to consume that list; `render/` knows nothing about the ECS.
- `Soft` rasterizes into a `Vec<u32>`; `Gpu` (wgpu) turns every command into quads. Both must give the same pixels: `gpu_output_matches_the_software_rasterizer` renders one scene on both and compares (skips without an adapter; `ENGINE_DUMP=<dir>` writes `soft.png` and `gpu.png`). Fills and sprite origins are truncated the same way, and scaled or turned sprites are sampled `SAMPLE_BIAS` towards the next texel so ties break the same way.
- `Config::renderer` is `Auto` (GPU only if a non-CPU adapter can present, else `Soft` with one stderr line), `Gpu` or `Software`. Under WSL `run` sets `GALLIUM_DRIVER=d3d12` itself when `/dev/dxg` exists. The GPU texture cache keys sprites by `Arc` pointer and drops entries nobody else references each frame.
- The camera is applied in `Gfx::draw`, not in `fill`/`blit`/`text`, so HUD drawn directly stays in screen space. `Gfx::draw` sorts by layer (stable).
- `Tilemap::draw_list(gfx.view())` gives one or two `Tiles` commands (behind and in front of entities), or none.
- Every `DrawCmd` has a `Transform { angle, sx, sy }` (radians, clockwise on screen; scale about the centre of the drawn quad; negative mirrors; zero draws nothing). `Glyph`, `Mask`, `Tiles`, `Outline` and `Clear` ignore it. `Soft` takes `scaled_blit` for scale without angle and `warp` for turned shapes; both must keep the same arithmetic (a test compares them).
- `Soft::fill_alpha` uses the SWAR `blend` compiled plain and with AVX2, picked at run time. The two must stay bit-identical (a test checks it).
- `Config::scale` (or `ENGINE_SCALE`) makes the window `scale x` the internal resolution; the game stays in internal pixels. `Config::resizable` letterboxes the picture (`letterbox` in `render/gfx.rs`). minifb has no fullscreen call.

**Camera (`render/camera.rs`).** State lives in `ctx.cam` and advances in the fixed update. `follow(target, world_size, dt)` eases and clamps (the first call snaps), `cut()` makes the next `follow` snap, `shake(amplitude, secs)` jitters, `set_zoom`/`zoom_to` zoom about the middle of the view (0.25..=8). `run` copies the final whole-pixel offset and zoom into `gfx.cam` / `gfx.zoom`. With zoom != 1 flat rects get each edge rounded so neighbours keep sharing edges, sprites scale about their centre to cover the rounded rect, text scale is `round(scale * zoom)` and at least 1, and a `Tiles` batch is split into one `Region` per tile. A NaN follow target is ignored. The camera is one per app and persists across scenes.

**ECS (`ecs/`).** `World` and `Entity` come from `hecs` (0.11: `query` iterates plain items; ask for `Entity` in the tuple when needed). Components must be `Send + Sync`, which is why `Assets` hands out `Arc<Sprite>`. The engine ships `Body(Rect)`, `Vel`, `Look`, `Anim`, `Collider`, `Layer`, `Filter` and the systems `movement`, `animate`, `particles`, `draw_list` (sorted by `Layer`, stable, so spawn order breaks ties; `Anim` entities follow `Look` ones in a layer). Games define their own components and logic inside `Scene::update`.
- Particles are ordinary entities made by `burst` and advanced by `particles(world, dt)`; they carry their own velocity instead of `Vel`.
- `Anim` treats a sheet as a grid of equal frames: a row is a clip, `play(row, frames, fps)` restarts only when the row changes.

**Collisions.** Entity vs entity: `collisions(&world)` reports overlapping `Body` + `Collider` pairs (sort-and-sweep on x) with penetration per axis; resolving is up to the game or `Physics`. Entity vs terrain: `Tilemap::slide` moves axis by axis and assumes less than a tile per step. `Filter { group, mask }` (optional) limits who meets whom: two entities meet only when each one's mask holds a bit of the other's group; tiles are group `Filter::TERRAIN` (bit 31). A `Collider` without a `RigidBody` is a trigger. `raycast` scans colliders; `RayIndex::new(&world)` is a grid snapshot for many rays (rebuild it after bodies move); `query_rect` lists colliders in an area. None of them look at tiles.

**Physics (`ecs/physics/`).** `mod.rs` (bodies, step, solver, sleeping, joints), `contact.rs` (contact generation: `touch` and the box, tilted box and circle cases) and `tests.rs`. In-house boxes and circles with rotation, no dependency.
- Give an entity `Body` + `Vel` + `RigidBody::dynamic(mass)` (`.circle()`, `.with_bounce`, `.with_friction`, `.with_spin`) and call `physics.step(&mut world, Some(&tilemap), dt)` once per fixed update. `RigidBody::fixed()` is a wall. Build `Physics::default()` and set fields afterwards (its fields are private). Entities without `RigidBody` are ignored.
- Rotation is opt-in by adding a `Transform`: physics reads and writes `angle` and turns the body about the centre of its (unturned) `Body` rect. A `fixed` body with a `Transform` is a tilted wall. `sx`/`sy` are ignored.
- Two bodies bounce by the smaller restitution, so walls need one too (`RigidBody::fixed().with_bounce(e)`, `Tilemap::bounce`).
- The step: gravity, semi-implicit Euler, solid tiles near each body joined as fixed boxes, pairs from `Sweep`, `touch`, sequential-impulse passes with accumulated impulses and Coulomb friction, then positional correction. Contacts are warm-started from `cache`; every `prepare` must run before any replay or piles explode.
- Untilted boxes take their contact direction from where the boxes were *before* the step, not from the current overlap (two regression tests; each fails without it). `step` cuts the step into substeps until no body moves more than `TRAVEL` (an eighth of its smaller side), up to `MAX_SUBSTEPS`; past that a fast body can still pass a thin wall. No true CCD.
- Tile contacts are filtered so floors have no ghost bumps (`behind` in `find_hits`, `is_hidden`); tiles always go second in a pair.
- Sleeping: a body below 5 px/s for half a second sleeps (acts as a wall, `Vel` zeroed). It wakes when a body faster than `WAKE_SPEED` overlaps it, when the game changes its `Body`/`Vel`/spin/angle, or when gravity, the tile map (`Tilemap::stamp`) or a supporting body changed. Jointed bodies never sleep. `physics.asleep(e)` / `wake(e)`.
- Joints: `rod(a, b, length)` and `rope(a, b, length)` between body centres, looked up by entity each step (a despawned end disables the joint), until `clear_joints()`.
- `ecs/fx.rs` is a small multiplicative hasher (`FxMap`, `FxSet`) for the engine's own keys. `Sweep` keeps its x order between calls and insertion-sorts it.
- The physics is bit-for-bit repeatable (a test checks it), which replay relies on. Tuning constants (`TRAVEL`, `HEAVY_RATIO`, correction passes, `WAKE_SPEED`) were set against the `shake` benchmark in `src/bench.rs`; rerun it with many seeds before changing them.

**Time helpers (`app/time.rs`).** `Timer::once/looping(secs)` accumulate the fixed `dt` given to `tick(dt)`; `fraction()` is progress. `Ease` curves and `Tween::new(from, to, secs, ease)`. `Timer::once` panics on a negative or NaN duration, so anything that takes a time from a game clamps it to 0 first.

**Panels and atlases.** `gfx.blit_nine(&sheet, src, margin, &dest)` draws a framed region as a panel of any size: corners at natural size, edges and middle *repeated* (not stretched), only pieces on screen recorded. `Atlas::parse/load(sheet, text)` names regions from `name x y w h` lines; `atlas.get(name)` panics on an unknown name.

**Audio (`assets/audio.rs`).** `ctx.audio().play(path)` fires a one-shot, `music(path)` loops one track. Three volumes (`set_volume` master, `set_music_volume`, `set_sfx_volume`, each clamped to 0..=1), `music_fade_in/out`, and `set_max_sounds` (a new sound cuts off the oldest). The numbers live in `Mix`, shared by the real and silent builds. Without the `audio` feature, with `ENGINE_MUTE=1`, or with no output device, every call is a no-op and sound files are not read. Under WSL, ALSA needs `libasound2-plugins` to find WSLg's sound server. Only the numbers are tested; nothing was listened to.

**Levels.** `Tilemap::load(path, size, sheet)` reads a plain-text grid: `#` ground and `+` brick are solid, `.` or space is empty, any other character is a marker (`map.marks(b'P')`). `try_load_with` takes a custom `Tile { ch, id, solid, front, auto }` legend; `auto` makes `id` the first of 16 sheet tiles chosen by joined neighbours (up 1, right 2, down 4, left 8). `front` tiles draw over entities. Rows must be ASCII and only trailing newlines are trimmed. `set(tx, ty, id)` panics for an id past the sheet. The `try_` variants return `Err` instead of panicking.

**Hot reload (`assets/watch.rs`).** `Config::hot_reload` (default on in debug builds) polls modified times twice a second. `Assets::sprite` and `ctx.load_bindings` watch their files; a changed image is swapped *inside* the existing `Sprite`, so every held `Arc<Sprite>` shows it, and the GPU re-uploads on a version change. Backends skip a frame or tile whose source rect no longer fits a smaller replacement sheet. Decode or parse errors keep the last good data and print one line. For other files: `ctx.watch(path)` once, then `if ctx.changed(path)`.

**Assets and input.** `ctx.assets()` caches sprites and fonts by path, never evicted. Input is latched between steps: a tap that ends before the step still counts. Games read input by action name, never by key: `ctx.bind("jump", &[Key::Space])` (once, usually in the closure passed to `run`), then `input().down/pressed/axis`. Querying an unbound action panics. `key_down(Key)` is the raw escape hatch. Bindings live in a text file of `action = Key Key pad.A` lines (`#` comments, `minifb::Key` variant names, case-insensitive): games bind defaults, then call `ctx.load_bindings("input.txt")`. A missing file is fine; a malformed one, or one naming an action the game never bound, returns `Err` and applies nothing. A reload starts from the defaults the game had before the first file. `ctx.save_bindings(path)` writes only what differs from those defaults.

**Mouse and gamepad.** `input().mouse()` is `Some((x, y))` in canvas pixels (`None` outside the picture or window), `ctx.mouse_world()` adds the camera, and `mouse_down/pressed/released`, `wheel()` use the same per-step edges. `Pad` buttons bind to actions with `ctx.bind_pad` (`pad.<Name>` in `input.txt`); `PadState` reads the Linux joystick API (`/dev/input/js0`, Xbox-style numbering) with no crate, and the d-pad and left stick both read as Up/Down/Left/Right. `input().pad_axis(PadAxis::..)` gives analog values. The decoding is tested with synthetic events only; no real pad was tried (WSL has no `/dev/input`).

**Record and replay (`app/replay.rs`).** `ENGINE_RECORD=path` writes the held keys, pad buttons, mouse and seed of each fixed step when the run ends normally; `ENGINE_REPLAY=path` feeds them back and quits when they run out. Not recorded: analog axes, the wheel, typed characters. It reproduces a run only if the game takes all randomness from `ctx.rng()` and reads no wall clock. A real recorded session has not been replayed.

**Save data (`app/save.rs`).** `Save` is a sorted `key = value` file owned by the game: `load` (missing file is empty, malformed is `Err`), `set`, `get::<T>`, `get_or`, `remove`, `save` (atomic write via `assets::write_atomic`). Keys may not contain `=` or `#` (`set` panics). Unparsable values read as missing.

**Fonts and Korean text.** `gfx.text` is the 8x8 ASCII font. `ctx.assets().font(path)` loads a BDF font (in-house parser in `render/bdf.rs`, packed into one atlas, drawn as `Paint::Mask`), and `gfx.text_font(&font, s, x, y, color)` / `text_font_scaled` draw it; multiply `font.width` and `line_height` by the scale yourself. `assets/fonts/galmuri9.bdf` is Galmuri9 (SIL OFL) cut down to ASCII, compatibility jamo and all 11172 syllables; a character the font lacks draws as `?`. The pre-commit large-file limit is 2000 KB for it.

**UI (`ui.rs`).** Immediate-mode helpers: `draw_panel`, `draw_bar`, `Menu`, `Button`, `Checkbox`, `Rebind`, `TextField`, and the stateless `hovered`, `clicked`, `slider`. Widgets that need both input and drawing split it: `update(ctx.input())` in `Scene::update`, `draw(gfx, &font)` in `Scene::draw`. Action names are the caller's and must be bound. `TextField` composes Hangul itself (`src/hangul.rs`) because minifb has no input method: jamo arrive from a Korean X layout or, after `set_korean(true)` (RightAlt), from letter keys. The composing logic is tested; typing in a real window was not tried.
