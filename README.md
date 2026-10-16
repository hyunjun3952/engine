# engine

A small 2D game engine in Rust: software and wgpu renderers, ECS, physics, tilemaps, audio and UI.

API docs: <https://hyunjun3952.github.io/engine/>

- **Rendering**: a software rasterizer and a wgpu backend that produce the same pixels. Camera (follow, shake, zoom), sprite transforms, blending, lighting, parallax, letterboxed and fullscreen (F11) windows.
- **ECS**: [hecs](https://crates.io/crates/hecs) plus animation, particles, collisions, raycasts and world save/load.
- **Physics**: in-house boxes and circles with rotation, joints, sleeping, one-way platforms and slopes. Deterministic, so input replay works.
- **Levels**: plain-text grids and Tiled `.tmx` maps, A* pathfinding, tile raycasts.
- **Audio** (rodio): WAV and OGG, buses, pan, pitch, fades.
- **Input**: action bindings from a text file, mouse, gamepad, record and replay.
- **UI and text**: immediate-mode widgets, BDF fonts with Korean support and Hangul composition.
- **Tools**: hot reload, F3 overlay, entity inspector and console.

## Quick start

```sh
cargo run --example game   # a platformer; `-- 3` starts at stage 4
```

Needs an X display (WSLg works). F3 toggles the debug overlay. CI builds and tests on Linux, Windows and macOS.

## Features

`gpu` (wgpu renderer) and `audio` (rodio) are on by default. `--no-default-features` builds the lean version: software renderer, no audio.

## Environment variables

| Variable | Effect |
|----------|--------|
| `ENGINE_RENDERER=soft\|gpu\|auto` | pick the renderer |
| `ENGINE_SCALE=n` | window is `n` times the internal resolution |
| `ENGINE_TICK_HZ=n` | fixed steps (and frames) per second |
| `ENGINE_MUTE=1` | silence audio |
| `ENGINE_STATS=1` | log frame timings |
| `ENGINE_RECORD=path` / `ENGINE_REPLAY=path` | record and replay input |

## Development

```sh
cargo test                                      # run from the repo root; tests load assets/ by relative path
cargo fmt && cargo clippy --all-targets -- -D warnings
cargo test --release -- --ignored --nocapture   # benchmarks in src/bench.rs
cargo doc --open
pre-commit install                              # Conventional Commits, fmt, clippy
```

See `CLAUDE.md` for the architecture and conventions.

## Licence

Apache-2.0, see `LICENSE.md`. The bundled `assets/fonts/galmuri9.bdf` is Galmuri9 under the SIL OFL.
