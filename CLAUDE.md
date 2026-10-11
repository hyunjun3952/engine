# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

    cargo run --example pong            # also: breakout, platformer (F3 toggles the debug overlay)
    cargo test                          # unit tests; run from the repo root, they load assets/ by relative path
    cargo test <name>                   # single test, e.g. cargo test falling_box
    cargo test --release -- --ignored --nocapture   # benchmarks in src/bench.rs
    cargo fmt && cargo clippy --all-targets -- -D warnings

Examples also load `assets/` by relative path, so run them from the repo root. The window needs an X display (WSLg works); `minifb` is built with the `x11` feature only, because its Wayland backend spams EGL/Zink errors under WSL.

Commits are checked by pre-commit (`pre-commit install`): Conventional Commits (`feat|fix|refactor|perf|test|docs|chore|ci`, optional scope), plus `cargo fmt --check` and clippy with `-D warnings`. There is no CI; run `cargo test` yourself.

## Project conventions

- Comments are allowed but rare: add one only where the code cannot show *why* (a non-obvious constraint, workaround or invariant). Keep it to one short, plain line. Never restate what the code does, and skip doc comments on self-explanatory items. The owner dislikes comment-heavy code.
- `rustfmt.toml` uses `max_width = 120` and `use_small_heuristics = "Max"`, so short bodies stay on one line. Run `cargo fmt` rather than formatting by hand.
- Keep changes minimal; measure before optimizing (see `src/bench.rs`).

## Architecture

`engine` is a library crate. Source is grouped by area (`app/`, `render/`, `ecs/`, `assets/`) and `src/lib.rs` re-exports the whole public API at the crate root, so games and internal code use flat `engine::X` / `crate::X` paths and never reach into the folders. Games live in `examples/` and use only that API, never `minifb` directly.

**Loop and scenes (`app/`).** `run(cfg, first)` owns the window, a fixed-timestep accumulator (`Config::tick_hz`, default 60) and a scene stack. A game implements `Scene { update(&mut self, &mut Ctx) -> Trans, draw(&self, &mut Gfx) }`. Only the top scene updates; every scene draws, bottom first, so overlays like pause draw over what is beneath. `Trans` (`None/Push/Pop/Replace/Quit`) drives the stack, and an empty stack ends the program. `first` is a closure taking `&mut Ctx` because scenes load assets in their constructors. The engine does not handle Esc; scenes do. F3 (debug overlay) is handled in `run` by tracking its previous state itself, because `Input::pressed` only refreshes once per fixed step and would misfire on frames that run no step.

**Rendering (`render/gfx.rs`).** Software renderer into a `Vec<u32>` (0xAARRGGBB sprites, 0xRRGGBB framebuffer). `Gfx` has `fill`, `fill_alpha` (0-255 blend over what is already drawn; plain `fill` ignores alpha), `outline`, `blit`, `blit_region`, `text` (8x8 bitmap font, ASCII only), plus `cam`. The camera is applied by `render` (entities) and `Tilemap::draw`, not by `fill`/`blit`/`text`, so HUD text drawn directly with `gfx.text` stays in screen space.

**Camera (`render/camera.rs`).** State lives in `ctx.cam` and is advanced in the fixed update, not in `draw`: scenes call `ctx.cam.follow(target, world_size, dt)` (eases toward the target with a frame-rate-independent factor, clamped to the world; the first call snaps) and `ctx.cam.shake(amplitude_px, secs)` (random jitter that decays linearly). `cut()` makes the next `follow` snap, for teleports and respawns. `run` copies the final whole-pixel offset into `gfx.cam` before drawing, so tiles and entities do not jitter against each other. The camera persists across scenes; nothing resets it on `Trans::Replace`.

**ECS (`ecs/`).** `World` and `Entity` come from `hecs`. The engine ships only components every game needs (`Body(Rect)`, `Vel`, `Look`, `Anim`, `Collider`, `Layer`) and these systems: `movement`, `animate`, `particles`, `render`; games define their own components and logic inside `Scene::update`. hecs requires components to be `Send + Sync`, which is why `Assets` hands out `Arc<Sprite>`, not `Rc`. In hecs 0.11, `query`/`query_mut` iterate plain items (no `(Entity, ...)` wrapper); ask for `Entity` explicitly in the query tuple when you need it.

**Collisions (`ecs/`, `render/tilemap.rs`).** Two separate mechanisms. Entity vs entity: `collisions(&world)` returns `Contact`s for overlapping `Body` + `Collider` pairs using sort-and-sweep on x; contacts carry penetration depth per axis, and `Contact::x_axis()` says which axis to bounce on. It only reports overlaps; resolving them is up to the game. Entity vs terrain: `Tilemap::slide` moves axis by axis and snaps to tile edges, assuming per-step movement smaller than a tile. Screen borders are handled by game code, not entities.

**Layers and particles.** `render` sorts drawables by `Layer(i32)` (default 0, stable within a layer, so spawn order breaks ties). Particles are ordinary entities (`Body`, `Look::Color`, `Layer(10)`, `Particle`) created by `burst(world, &mut ctx.rng, at, &Burst{..})` and advanced by `particles(world, dt)`, which fades them out (`Look::Alpha`) over their life, despawns them, and moves them itself: they carry their own velocity instead of `Vel`, so they are not double-moved by `movement` and games whose player uses `Vel` without `movement` still work. `ctx.rng` is the shared seeded `Rng`.

**Animation.** `Anim` treats a sprite sheet as a grid of equal frames: a row is a clip, `play(row, frames, fps)` selects it (and restarts only when the row changes), `flip` mirrors horizontally. Call `animate` each step; `render` draws `Anim` entities after `Look` ones.

**Audio (`assets/audio.rs`).** `ctx.audio.play(path)` fires a one-shot, `music(path)` loops one track. File bytes are cached, decoding happens per play. If no output device opens, audio disables itself with one stderr line and every call becomes a no-op, so games never fail on audio. Under WSL, ALSA needs the PulseAudio plugin (`libasound2-plugins`) to find WSLg's sound server.

**Levels.** `Tilemap::load(path, ...)` reads a plain-text grid, one row per line, all rows the same width: `#` ground and `+` brick are solid tiles, `.` or space is empty, and any other character is a marker the game queries with `map.marks(b'P')` (pixel position of the tile's top-left). The platformer takes the level path as its first CLI argument (default `assets/level1.txt`) and needs exactly one `P` for the player start.

**Assets and input.** `Ctx::assets` caches sprites by path (`Arc<Sprite>`, never evicted). `Input` wraps `minifb::Key`, exposing `down`, `pressed` (edge-triggered per fixed step) and `axis`.
