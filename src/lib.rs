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
//!
//! # Map
//!
//! Everything is at the crate root; this is where each part lives.
//!
//! | Area | Items |
//! |---|---|
//! | Loop and scenes | [`run`], [`Config`], [`Scene`], [`Trans`], [`Ctx`] |
//! | Time | [`Timer`], [`Tween`], [`Ease`], [`Rng`] |
//! | Drawing | [`Gfx`], [`DrawCmd`], [`Paint`], [`Style`], [`Blend`], [`Transform`], [`Sprite`], [`Rect`], [`Vec2`] |
//! | Camera and light | [`Camera`], [`LightMap`], [`Light`], [`vignette`], [`scanlines`] |
//! | Text | [`Gfx::text`], [`Font`], [`Gfx::text_font`], [`wrap`], [`draw_wrapped`] |
//! | Levels | [`Tilemap`], [`Tile`], [`Slope`], [`Slide`], [`TileHit`], [`Tiled`], [`TiledObject`], [`Spawner`] |
//! | ECS | [`World`], [`Entity`], [`Body`], [`Vel`], [`Look`], [`Layer`], [`movement`], [`draw_list`] |
//! | Animation and particles | [`Anim`], [`Clip`], [`AnimStates`], [`animate`], [`Burst`], [`burst`], [`particles`], [`Particle`] |
//! | Collisions and rays | [`Collider`], [`Filter`], [`collisions`], [`Contact`], [`Overlaps`], [`query_rect`], [`raycast`], [`raycasts`], [`Ray`], [`RayHit`], [`RayIndex`] |
//! | Physics | [`Physics`], [`RigidBody`], [`ContactEvent`] |
//! | Saving | [`Save`], [`WorldFile`], [`Persist`] |
//! | Input | [`Input`], [`Key`], [`MouseButton`], [`Pad`], [`PadAxis`] |
//! | Assets and audio | [`Assets`], [`Atlas`], [`Audio`], [`Sound`], [`Voice`], [`Bus`] |
//! | UI | [`Theme`], [`Button`], [`Checkbox`], [`Menu`], [`ScrollList`], [`TextField`], [`Focus`], [`Rebind`], [`Layout`], [`grid`], [`draw_panel`], [`draw_bar`], [`hovered`], [`clicked`], [`slider`] |
//! | Debug | [`Inspector`], [`Console`], [`Command`] (F3 shows the frame overlay) |
//!
//! # Conventions
//!
//! - World points and directions are [`Vec2`]; arguments take `impl Into<Vec2>`, so `(x, y)` works too.
//! - Drawing straight to the screen (`fill`, `blit`, `text`, widgets) uses whole canvas pixels: `i32` positions.
//! - Areas are a [`Rect`], always passed by value.
//! - Tile cells are `(usize, usize)`.
//! - Colours are `0xRRGGBB` and sprite pixels `0xAARRGGBB`.
//! - Anything that reads or writes a file returns `Result<_, String>`, and the message names the file; a `load` that
//!   makes a value (`Sprite::load`, `Tilemap::load`, `Save::load`, ...) takes the path first.
//! - A mistake in the game's own code panics with a message: an unbound action, an unknown atlas region or animation
//!   state, a sprite file missing from [`Assets`].
//! - Builders chain plain names (`RigidBody::dynamic(1.0).bounce(0.5)`, `Sound::new().volume(0.8)`); getters are
//!   the bare name and setters start with `set_`.
//! - Systems over a [`World`] are free functions (`movement`, `animate`, `particles`, `collisions`); stateful parts
//!   ([`Physics`], [`Camera`], widgets) are types with `update`/`step` and `draw`.

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
pub use assets::{Assets, Atlas, Audio, Bus, Sound, Voice};
pub use debug::{Command, Console, Inspector};
pub use ecs::{
    Anim, AnimStates, Body, Burst, Clip, Collider, Contact, ContactEvent, Entity, Filter, Layer, Look, OverlapChanges,
    Overlaps, Particle, Persist, Physics, Ray, RayHit, RayIndex, RigidBody, Spawner, Vel, World, WorldFile, animate,
    burst, collisions, draw_list, movement, particles, query_rect, raycast, raycasts,
};
pub use render::{
    Blend, Camera, DrawCmd, Font, Gfx, Light, LightMap, Paint, Rect, Slide, Slope, Sprite, Style, Tile, TileHit, Tiled,
    TiledObject, Tilemap, Transform, scanlines, vignette,
};
pub use ui::{
    Button, Checkbox, Focus, Layout, Menu, Rebind, ScrollList, TextField, Theme, clicked, draw_bar, draw_panel,
    draw_wrapped, grid, hovered, slider, wrap,
};
pub use vec2::Vec2;
