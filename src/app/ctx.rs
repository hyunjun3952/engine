use super::fade::Fade;
use crate::{Assets, Audio, Camera, Input, Key, Pad, Rng, Vec2};

/// What a scene reaches during `update`: input, assets, audio, the camera, the random generator and the step length.
///
/// Each accessor borrows the whole `Ctx`, so read values into locals first: `let dt = ctx.dt();` then
/// `ctx.cam().follow(.., dt)`.
///
/// ```
/// use engine::{Ctx, Rect};
///
/// fn update(ctx: &mut Ctx, player: Rect) {
///     let dt = ctx.dt();
///     ctx.cam().follow(player, (1600.0, 900.0), dt);
///     let roll = ctx.rng().f32();
///     # let _ = roll;
/// }
/// ```
pub struct Ctx {
    pub(crate) input: Input,
    pub(crate) assets: Assets,
    pub(crate) audio: Audio,
    pub(crate) fade: Fade,
    pub(crate) cam: Camera,
    pub(crate) rng: Rng,
    pub(crate) bindings_path: Option<String>,
    // The bindings as the game set them before the first file was applied, so a save keeps only the player's changes.
    pub(super) defaults: Option<String>,
    // The same defaults as bindings, to put back what a reloaded file no longer mentions.
    pub(super) base: Option<Vec<crate::input::Binding>>,
    pub(crate) dt: f32,
    pub(crate) w: usize,
    pub(crate) h: usize,
    // Whether the window should be fullscreen, and whether `run` still has to ask for it.
    pub(super) fullscreen: bool,
    pub(super) fullscreen_asked: bool,
}

impl Ctx {
    pub(crate) fn new(w: usize, h: usize, dt: f32, audio: Audio) -> Self {
        let mut input = Input::default();
        input.set_step(dt);
        Ctx {
            input,
            assets: Assets::default(),
            audio,
            fade: Fade::new(),
            cam: Camera::new(w as f32, h as f32),
            rng: Rng::new(1),
            bindings_path: None,
            defaults: None,
            base: None,
            dt,
            w,
            h,
            fullscreen: false,
            fullscreen_asked: false,
        }
    }

    /// Whether the window is (as far as has been asked) fullscreen.
    pub fn fullscreen(&self) -> bool {
        self.fullscreen
    }

    /// Asks for the window to go fullscreen or come back; the window manager has the last word.
    pub fn set_fullscreen(&mut self, on: bool) {
        if on != self.fullscreen {
            (self.fullscreen, self.fullscreen_asked) = (on, true);
        }
    }

    /// Flips fullscreen; see `set_fullscreen`.
    pub fn toggle_fullscreen(&mut self) {
        self.set_fullscreen(!self.fullscreen);
    }

    /// Binds an action name to keys; call it once, before the action is read.
    pub fn bind(&mut self, action: &str, keys: &[Key]) {
        self.input.bind(action, keys);
    }

    /// Binds an action name to gamepad buttons.
    pub fn bind_pad(&mut self, action: &str, pads: &[Pad]) {
        self.input.bind_pad(action, pads);
    }

    /// Applies an `input.txt` over the defaults; a missing file is fine, a malformed one is an `Err` and changes nothing.
    pub fn load_bindings(&mut self, path: &str) -> Result<(), String> {
        self.bindings_path = Some(path.to_string());
        self.defaults.get_or_insert_with(|| self.input.to_text());
        self.watch(path);
        let base = self.base.get_or_insert_with(|| self.input.bindings());
        let text = crate::base::read_text_if_exists(path)?.unwrap_or_default();
        self.input.load_over(&text, base).map_err(|e| format!("{path}: {e}"))
    }

    /// Writes the bindings, only those that differ from what the game had bound when it first called
    /// `load_bindings`, so changing a default later still reaches players who remapped something else.
    pub fn save_bindings(&self, path: &str) -> Result<(), String> {
        let all = self.input.to_text();
        let kept = |line: &&str| self.defaults.as_ref().is_none_or(|d| !d.lines().any(|l| l == *line));
        crate::base::write_atomic(path, &all.lines().filter(kept).map(|l| format!("{l}\n")).collect::<String>())
    }

    /// Starts watching `path` for changes; read them with `changed`.
    pub fn watch(&mut self, path: &str) {
        self.assets.watcher.watch(path);
    }

    /// True once after the watched file changed.
    pub fn changed(&mut self, path: &str) -> bool {
        self.assets.watcher.take(path)
    }

    pub(crate) fn poll_files(&mut self) {
        self.assets.watcher.poll();
        for result in self.assets.reload_changed() {
            match result {
                Ok(path) => eprintln!("reloaded {path}"),
                Err(e) => eprintln!("{e}"),
            }
        }
        if let Some(path) = self.bindings_path.clone()
            && self.changed(&path)
        {
            match self.load_bindings(&path) {
                Ok(()) => eprintln!("reloaded {path}"),
                Err(e) => eprintln!("{e}"),
            }
        }
    }

    /// The input of this step.
    pub fn input(&self) -> &Input {
        &self.input
    }

    /// The mouse in world coordinates under the current camera (`input().mouse()` is in screen pixels).
    pub fn mouse_world(&self) -> Option<Vec2> {
        self.input.mouse().map(|p| self.cam.to_world(p))
    }

    /// The sprite and font cache.
    pub fn assets(&mut self) -> &mut Assets {
        &mut self.assets
    }

    /// The sound mixer.
    pub fn audio(&mut self) -> &mut Audio {
        &mut self.audio
    }

    /// The camera, which advances in the fixed update.
    pub fn cam(&mut self) -> &mut Camera {
        &mut self.cam
    }

    /// The seeded random generator; take all randomness from here so a replay reproduces the run.
    pub fn rng(&mut self) -> &mut Rng {
        &mut self.rng
    }

    /// Seconds in one fixed step.
    pub fn dt(&self) -> f32 {
        self.dt
    }

    /// Size of the internal canvas in pixels.
    pub fn size(&self) -> (usize, usize) {
        (self.w, self.h)
    }
}
