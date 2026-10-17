# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

    cargo test                          # run from the repo root, tests load assets/ by relative path
    cargo test <name>                   # single test, e.g. cargo test falling_box
    cargo test --no-default-features    # lean build: software renderer, no audio
    cargo test --release -- --ignored --nocapture   # benchmarks in src/bench.rs (`bench`, `systems`, `shake` with SEEDS=40)
    cargo run --example game [stage]    # F3 toggles the debug overlay
    cargo fmt && cargo clippy --all-targets -- -D warnings
    RUSTDOCFLAGS="-D warnings" cargo doc --no-deps   # broken doc links fail CI

Env: `ENGINE_TICK_HZ`, `ENGINE_SCALE`, `ENGINE_RENDERER=soft|gpu|auto`, `ENGINE_MUTE=1`, `ENGINE_STATS=1`, `ENGINE_RECORD=path` / `ENGINE_REPLAY=path`, `ENGINE_DUMP=<dir>` (the GPU/Soft test writes `soft.png` and `gpu.png`).

The window needs an X display (WSLg works). `minifb` is built with the `x11` feature only, because its Wayland backend spams EGL/Zink errors under WSL.

## Workflow

- pre-commit and CI check Conventional Commits, fmt, clippy (default and `--no-default-features`), rustdoc and tests; the MSRV is Rust 1.90.
- When asked to commit and push, commit on `main` and push `main` directly (no feature branch, no PR).
- Keep changes minimal; measure before optimizing (see `src/bench.rs`). Put measurements in commit messages, not here.

## Conventions

- **Comments** are rare: only where the code cannot show *why*, in a line or two. Write the current rule, not its history.
- **Docs.** Every pub item gets a one-line `///` and each pub type and function a doctest. A pub fn that can panic or return `Err` says so in its `///` ("Panics on ..."). A `Result` error names the file; a mistake in the game's own code panics.
- **Lints.** Prefer `#[expect]` to `#[allow]`. Imports are explicit, except `use super::*` in tests and `use file::*` in a split directory's `mod.rs`. A rule a lint can enforce goes in `[lints.clippy]`.
- **Dependencies.** Prefer in-house code or tiny crates; build time matters. Code touching `wgpu` or `rodio` stays behind `#[cfg(feature = "gpu" | "audio")]`; the lean build must keep compiling.

## Architecture

Library crate; `src/lib.rs` re-exports the public API and games use only that, never `minifb`. `World` and `Entity` come from `hecs` 0.11. Components must be `Send + Sync`, which is why `Assets` hands out `Arc<Sprite>`.

**Layers.** A module reaches only lower layers (the `layers` test checks it); anything crossing a boundary stays `pub(crate)`.

    base  →  render, audio  →  level, assets, input  →  ecs  →  physics  →  app  →  ui  →  debug.rs

- **Loop (`app/`).** Fixed timestep, scene stack; only the top scene updates, every scene draws. Scenes handle Esc, call `ctx.cam().reset()` in `on_enter` and stop music in `on_exit`. `Ctx` accessors each borrow the whole `Ctx`: read `let dt = ctx.dt();` into a local before `ctx.cam().follow(.., dt)`. Fullscreen is an EWMH message through `x11-dl`, not minifb.
- **Rendering.** `Gfx` only records `DrawCmd`s; the camera is applied in `Gfx::draw`, so HUD drawn directly stays in screen space. `Soft` and `Gpu` must give identical pixels (`gpu_output_matches_the_software_rasterizer`), so change both together.
- **ECS.** `raycast` and `query_rect` see colliders only, `Tilemap::raycast` and `path` tiles only. `Filter { group, mask }`: two entities meet only when each mask holds a bit of the other's group. A `Collider` without a `RigidBody` is a trigger. Saving a world skips `Physics`, an `Anim` itself (register its `AnimState` and `restore` it after starting the clip) and links between entities, so rebuild `Physics` after a load or `Rewind::back`.
- **Physics.** Rotation is opt-in via `Transform`. Walls need a restitution too (bounce uses the smaller). Contacts are warm-started, so every `prepare` must run before any replay. Untilted boxes take contact direction from positions *before* the step (two regression tests). Fast bodies with something in their path get the step cut into up to 16 substeps instead of true CCD, so a body faster than that still tunnels. The physics is bit-for-bit repeatable and replay relies on it; tuning constants were set against the `shake` benchmark, so rerun it with many seeds before changing them.
- **Levels.** Cells are `(usize, usize)`; only `TileHit::tile` is `(i32, i32)`. `Tilemap::slide` walks a move longer than half a tile in half-tile steps. Every `load` returns a `Result`; `from_ascii` panics where `try_from_ascii` returns `Err`.
- **Input and replay.** Games read input by action name, never by key; an unbound action panics. A run replays only if the game takes all randomness from `ctx.rng()` and reads no wall clock. `Timer::once` panics on a negative or NaN duration, so clamp game-supplied times to 0.
- **Audio, fonts.** Without the `audio` feature, with `ENGINE_MUTE=1`, or with no device, audio calls are no-ops. Under WSL, ALSA needs `libasound2-plugins`. `galmuri9.bdf` is cut down to ASCII, jamo and syllables; the pre-commit large-file limit is 2000 KB for it.

## Not verified

Audio was never listened to, no real gamepad was tried, a real recorded session was never replayed, and typing in a real window was not tried.
