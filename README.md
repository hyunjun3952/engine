# engine

A small 2D game engine in Rust: software and wgpu renderers, ECS, physics, tilemaps, audio and UI.

API docs: <https://hyunjun3952.github.io/engine/>

- **Rendering**: a software rasterizer and a wgpu backend that produce the same pixels. Camera with follow, shake and zoom, sprite transforms, blend styles, lighting, parallax.
- **ECS**: [hecs](https://crates.io/crates/hecs) plus animation, particles, collisions, raycasts and world save/load.
- **Physics**: in-house boxes and circles with rotation, joints, sleeping, one-way platforms and slopes. Deterministic, so input replay works.
- **Levels**: plain-text grids and Tiled `.tmx` maps, A* pathfinding, tile raycasts.
- **Audio** (rodio): WAV and OGG, buses, pan, pitch, fades.
- **Input**: action bindings loaded from a text file, mouse, gamepad, record and replay.
- **UI and text**: immediate-mode widgets, BDF fonts with Korean support and Hangul composition.
- **Tools**: hot reload, F3 overlay, entity inspector and console.

## Quick start

```sh
cargo run --example platformer   # also: camera, physics, raycast, text
cargo test                       # run from the repo root, tests load assets/ by relative path
```

A window needs an X display (WSLg works).

## Features

| Feature | Default | Needs |
|---------|---------|-------|
| `gpu`   | yes | wgpu renderer |
| `audio` | yes | rodio sound |

`cargo test --no-default-features` builds the lean version: software renderer, no audio.

## Environment variables

| Variable | Effect |
|----------|--------|
| `ENGINE_RENDERER=soft\|gpu\|auto` | pick the renderer |
| `ENGINE_SCALE=n` | window is `n` times the internal resolution |
| `ENGINE_MUTE=1` | silence audio |
| `ENGINE_STATS=1` | log frame timings |
| `ENGINE_RECORD=path` / `ENGINE_REPLAY=path` | record and replay input |

## Development

```sh
cargo fmt && cargo clippy --all-targets -- -D warnings
cargo test --release -- --ignored --nocapture   # benchmarks in src/bench.rs
cargo doc --open                                # API reference
pre-commit install                              # Conventional Commits, fmt, clippy
```

See `CLAUDE.md` for the architecture and conventions.

## Layout

```
src/app/      loop, scenes, input, time, save data, replay
src/render/   Gfx, Soft and Gpu backends, camera, tilemap, Tiled, light
src/ecs/      components, systems, physics, persistence
src/assets/   sprite and font cache, atlas, audio, hot reload
examples/     runnable demos
assets/       fonts, images, levels
```

## Licence

Apache-2.0, see `LICENSE`. The bundled `assets/fonts/galmuri9.bdf` is Galmuri9 under the SIL OFL.
