/// Which renderer draws the frame; see `Config::renderer`.
///
/// ```
/// use engine::{Config, RenderMode};
///
/// let mut cfg = Config::default();
/// cfg.renderer = RenderMode::Software;
/// assert_eq!(cfg.renderer, RenderMode::Software);
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RenderMode {
    /// Picks the GPU if a non-CPU adapter can present, else the software renderer.
    Auto,
    /// Always the GPU; fails to start without one.
    Gpu,
    /// Always the CPU rasterizer.
    Software,
}

/// How [`run`](crate::run) opens the window and steps the game; start from `Config::default()` (640x360, 60 ticks a second).
///
/// ```
/// use engine::Config;
///
/// let cfg = Config { title: "demo", ..Config::default() };
/// assert_eq!(cfg.title, "demo");
/// ```
pub struct Config {
    /// The window title.
    pub title: &'static str,
    /// Width of the internal canvas in pixels.
    pub w: usize,
    /// Height of the internal canvas in pixels.
    pub h: usize,
    /// Fixed updates per second.
    pub tick_hz: f32,
    /// Which renderer draws the frames.
    pub renderer: RenderMode,
    /// Reload changed assets while the game runs.
    pub hot_reload: bool,
    /// The window is `scale` times the internal resolution.
    pub scale: usize,
    /// Lets the player drag the window to any size; the picture keeps its shape and the rest is black.
    pub resizable: bool,
    /// Starts fullscreen (X11 only; elsewhere it is ignored with a line on stderr). F11 and `Ctx::toggle_fullscreen`
    /// switch at any time; the picture keeps its shape and the rest is black.
    pub fullscreen: bool,
}

impl Config {
    // A zero rate makes every step infinitely long, so none ever runs; a negative one makes the step loop endless.
    pub(super) fn check(&self) {
        assert!(
            self.tick_hz.is_finite() && self.tick_hz > 0.0,
            "Config::tick_hz must be a positive number, not {}",
            self.tick_hz
        );
        assert!(self.w > 0 && self.h > 0, "Config::w and h must be positive, not {}x{}", self.w, self.h);
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            title: "engine",
            w: 640,
            h: 360,
            tick_hz: 60.0,
            renderer: RenderMode::Auto,
            hot_reload: cfg!(debug_assertions),
            scale: 1,
            resizable: true,
            fullscreen: false,
        }
    }
}
