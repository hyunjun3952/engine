//! A small 2D game engine: software and wgpu renderers, ECS, physics, tilemaps, audio and UI.
//!
//! A game is a [`Scene`] handed to [`run`]. Only the top scene updates, every scene draws.
//!
//! ```no_run
//! use engine::{Config, Ctx, Gfx, Key, Rect, Scene, Trans};
//!
//! struct Game;
//!
//! impl Scene for Game {
//!     fn update(&mut self, ctx: &mut Ctx) -> Trans {
//!         if ctx.input().pressed("back") { Trans::Quit } else { Trans::None }
//!     }
//!
//!     fn draw(&self, g: &mut Gfx) {
//!         g.clear(0x101018);
//!         g.fill(Rect::new(20.0, 20.0, 40.0, 40.0), 0xffd040);
//!         g.text("hello", 16, 80, 1, 0xffffff);
//!     }
//! }
//!
//! engine::run(Config::default(), |ctx| {
//!     ctx.bind("back", &[Key::Escape]);
//!     Box::new(Game)
//! });
//! ```

mod app;
mod assets;
#[cfg(test)]
mod bench;
mod debug;
mod ecs;
mod hangul;
mod render;
mod ui;
mod vec2;

pub use app::{
    Config, Ctx, Ease, Input, Key, MouseButton, Pad, PadAxis, RenderMode, Rng, Save, Scene, Timer, Trans, Tween, run,
};
pub use assets::{Assets, Atlas, Audio, Bus, Sound};
pub use debug::{Command, Console, Inspector};
pub use ecs::{
    Anim, AnimStates, Body, Burst, Clip, Collider, Contact, ContactEvent, Entity, Filter, Layer, Look, Persist,
    Physics, Ray, RayHit, RayIndex, RigidBody, Spawner, Vel, World, WorldFile, animate, burst, collisions, draw_list,
    movement, particles, query_rect, raycast, raycasts,
};
pub use render::{
    Blend, Camera, DrawCmd, Font, Gfx, Light, LightMap, Paint, Rect, Slope, Sprite, Style, Tile, TileHit, Tiled,
    TiledObject, Tilemap, Transform, scanlines, vignette,
};
pub use ui::{
    Button, Checkbox, Focus, Layout, Menu, Rebind, ScrollList, TextField, clicked, draw_bar, draw_panel, draw_wrapped,
    grid, hovered, slider, wrap,
};
pub use vec2::Vec2;
