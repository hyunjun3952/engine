mod fade;
mod fullscreen;
mod input;
mod pad;
mod replay;
mod rng;
pub(crate) mod save;
mod time;

pub use input::{Input, Key, MouseButton};
pub use pad::{Pad, PadAxis};
pub use rng::Rng;
pub use save::{Save, data_dir};
pub use time::{Ease, Timer, Tween};

#[cfg(feature = "gpu")]
use crate::render::Gpu;
use crate::render::Soft;
use crate::{Assets, Audio, Camera, Gfx, Rect, Vec2};
use fade::Fade;
use minifb::{ScaleMode, Window, WindowOptions};
use replay::Tape;
use std::time::Instant;

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

/// How [`run`] opens the window and steps the game; start from `Config::default()` (640x360, 60 ticks a second).
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
    fn check(&self) {
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
    defaults: Option<String>,
    // The same defaults as bindings, to put back what a reloaded file no longer mentions.
    base: Option<Vec<input::Binding>>,
    pub(crate) dt: f32,
    pub(crate) w: usize,
    pub(crate) h: usize,
    // Whether the window should be fullscreen, and whether `run` still has to ask for it.
    fullscreen: bool,
    fullscreen_asked: bool,
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
        let text = crate::assets::read_text_if_exists(path)?.unwrap_or_default();
        self.input.load_over(&text, base).map_err(|e| format!("{path}: {e}"))
    }

    /// Writes the bindings, only those that differ from what the game had bound when it first called
    /// `load_bindings`, so changing a default later still reaches players who remapped something else.
    pub fn save_bindings(&self, path: &str) -> Result<(), String> {
        let all = self.input.to_text();
        let kept = |line: &&str| self.defaults.as_ref().is_none_or(|d| !d.lines().any(|l| l == *line));
        crate::assets::write_atomic(path, &all.lines().filter(kept).map(|l| format!("{l}\n")).collect::<String>())
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

/// What the scene stack does after an `update`.
///
/// ```
/// use engine::Trans;
///
/// let leave = Trans::Fade(Box::new(Trans::Pop), 0.5);
/// assert!(matches!(leave, Trans::Fade(_, _)));
/// ```
pub enum Trans {
    /// Stay on this scene.
    None,
    /// Put a scene on top; the one beneath stays as it is.
    Push(Box<dyn Scene>),
    /// Remove the top scene.
    Pop,
    /// Swap the top scene for another.
    Replace(Box<dyn Scene>),
    /// End the game.
    Quit,
    /// Fades to black over half of `secs`, makes the inner change, and fades back over the other half. The scenes
    /// stand still while it darkens.
    Fade(Box<Trans>, f32),
}

/// One screen of the game. Only the top scene of the stack updates; every scene draws, bottom first.
///
/// `update` runs once per fixed step and returns a [`Trans`]; `on_enter` runs when the scene joins the stack and
/// `on_exit` when it leaves (stop music there). A pushed overlay does not touch the scene beneath. The engine does
/// not handle Esc; scenes do.
///
/// ```
/// use engine::{Config, Ctx, Gfx, Key, Scene, Trans};
///
/// struct Title;
///
/// impl Scene for Title {
///     fn update(&mut self, ctx: &mut Ctx) -> Trans {
///         if ctx.input().pressed("quit") { Trans::Quit } else { Trans::None }
///     }
///     fn draw(&self, gfx: &mut Gfx) {
///         gfx.clear(0x101018);
///     }
/// }
/// # let _ = Title;
/// ```
pub trait Scene {
    /// Advances the scene by one fixed step and says what the stack does next.
    fn update(&mut self, ctx: &mut Ctx) -> Trans;
    /// Records what the scene looks like; runs for every scene on the stack, bottom first.
    fn draw(&self, gfx: &mut Gfx);
    /// Runs when the scene joins the stack.
    fn on_enter(&mut self, _ctx: &mut Ctx) {}
    /// Runs when the scene leaves the stack; stop music here.
    fn on_exit(&mut self, _ctx: &mut Ctx) {}
}

type Stack = Vec<Box<dyn Scene>>;

fn enter(stack: &mut Stack, mut scene: Box<dyn Scene>, ctx: &mut Ctx) {
    scene.on_enter(ctx);
    stack.push(scene);
}

fn exit_top(stack: &mut Stack, ctx: &mut Ctx) {
    if let Some(mut scene) = stack.pop() {
        scene.on_exit(ctx);
    }
}

fn apply(stack: &mut Stack, trans: Trans, ctx: &mut Ctx) {
    match trans {
        Trans::None => {}
        Trans::Push(s) => enter(stack, s, ctx),
        Trans::Pop => exit_top(stack, ctx),
        Trans::Replace(s) => {
            exit_top(stack, ctx);
            enter(stack, s, ctx);
        }
        Trans::Quit => {
            while !stack.is_empty() {
                exit_top(stack, ctx);
            }
        }
        Trans::Fade(inner, secs) => ctx.fade.start(*inner, secs),
    }
}

/// One fixed step: replay input, fade, top scene update (not while darkening), bookkeeping; false when a replay ends.
fn step(stack: &mut Stack, ctx: &mut Ctx, tape: &mut Tape, dt: f32) -> bool {
    if !tape.step(&mut ctx.input) {
        apply(stack, Trans::Quit, ctx);
        return false;
    }
    if let Some(trans) = ctx.fade.tick(dt) {
        apply(stack, trans, ctx);
    }
    if !ctx.fade.blocking()
        && let Some(top) = stack.last_mut()
    {
        let trans = top.update(ctx);
        apply(stack, trans, ctx);
    }
    ctx.input.end_step();
    ctx.cam.tick(dt);
    ctx.audio.tick(dt);
    true
}

#[derive(Default)]
struct Stats {
    frames: u32,
    ticks: u32,
    work: f32,
    present: f32,
    fps: u32,
    tps: u32,
    ms: f32,
    present_ms: f32,
}

enum Backend {
    Soft(Soft),
    #[cfg(feature = "gpu")]
    Gpu(Box<Gpu>),
}

#[cfg(feature = "gpu")]
fn try_gpu(mode: RenderMode, win: &Window, (w, h): (usize, usize), scale: usize) -> Option<Backend> {
    match Gpu::for_window(win, (w as u32, h as u32), scale as u32, mode == RenderMode::Gpu) {
        Ok(gpu) => {
            eprintln!("renderer: gpu, {}", gpu.adapter);
            Some(Backend::Gpu(Box::new(gpu)))
        }
        Err(e) => {
            eprintln!("renderer: software ({e})");
            None
        }
    }
}

#[cfg(not(feature = "gpu"))]
fn try_gpu(mode: RenderMode, _win: &Window, _size: (usize, usize), _scale: usize) -> Option<Backend> {
    if mode == RenderMode::Gpu {
        eprintln!("renderer: software (built without the gpu feature)");
    }
    None
}

fn pick_backend(mode: RenderMode, win: &Window, (w, h): (usize, usize), scale: usize) -> Backend {
    let mode = match std::env::var("ENGINE_RENDERER").as_deref() {
        Ok("gpu") => RenderMode::Gpu,
        Ok("soft") => RenderMode::Software,
        Ok("auto") => RenderMode::Auto,
        Ok(other) => {
            eprintln!("ENGINE_RENDERER={other:?} is not auto, gpu or soft; ignored");
            mode
        }
        Err(_) => mode,
    };
    let gpu = if mode == RenderMode::Software { None } else { try_gpu(mode, win, (w, h), scale) };
    gpu.unwrap_or_else(|| Backend::Soft(Soft::new(w, h, scale)))
}

// A switch like `ENGINE_MUTE=1`: unset, empty and `0` are off.
pub(crate) fn env_flag(name: &str) -> bool {
    std::env::var(name).is_ok_and(|v| !v.is_empty() && v != "0")
}

/// ```no_run
/// use engine::{Config, Ctx, Gfx, Key, Scene, Trans};
///
/// struct Title;
///
/// impl Scene for Title {
///     fn update(&mut self, ctx: &mut Ctx) -> Trans {
///         if ctx.input().pressed("quit") { Trans::Quit } else { Trans::None }
///     }
///     fn draw(&self, gfx: &mut Gfx) {
///         gfx.clear(0x101018);
///     }
/// }
///
/// let cfg = Config { title: "demo", ..Config::default() };
/// engine::run(cfg, |ctx| {
///     ctx.bind("quit", &[Key::Escape]);
///     Box::new(Title)
/// });
/// ```
pub fn run(mut cfg: Config, first: impl FnOnce(&mut Ctx) -> Box<dyn Scene>) {
    if let Ok(v) = std::env::var("ENGINE_TICK_HZ") {
        match v.parse::<f32>() {
            Ok(hz) if hz.is_finite() && hz > 0.0 => cfg.tick_hz = hz,
            _ => eprintln!("ENGINE_TICK_HZ={v:?} is not a positive number; ignored"),
        }
    }
    cfg.check();
    // Mesa only exposes the WSL GPU through its D3D12 driver, and only when asked.
    #[cfg(feature = "gpu")]
    if std::env::var_os("GALLIUM_DRIVER").is_none() && std::path::Path::new("/dev/dxg").exists() {
        // SAFETY: done first in `run`, before the window, audio or renderer can start a thread.
        unsafe { std::env::set_var("GALLIUM_DRIVER", "d3d12") };
    }
    let env_scale = std::env::var("ENGINE_SCALE")
        .ok()
        .and_then(|v| v.parse().inspect_err(|_| eprintln!("ENGINE_SCALE={v:?} is not a whole number; ignored")).ok());
    let scale = env_scale.unwrap_or(cfg.scale).max(1);
    let options =
        WindowOptions { resize: cfg.resizable, scale_mode: ScaleMode::AspectRatioStretch, ..Default::default() };
    let mut win = Window::new(cfg.title, cfg.w * scale, cfg.h * scale, options)
        .unwrap_or_else(|e| panic!("cannot open a window ({e}); it needs an X display (DISPLAY, or WSLg under WSL)"));
    let mut gfx = Gfx::new(cfg.w, cfg.h);
    let log_stats = env_flag("ENGINE_STATS");
    let mut backend = pick_backend(cfg.renderer, &win, (cfg.w, cfg.h), scale);
    let dt = 1.0 / cfg.tick_hz;
    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.subsec_nanos());
    let mut ctx = Ctx::new(cfg.w, cfg.h, dt, Audio::new());
    ctx.set_fullscreen(cfg.fullscreen);
    ctx.input.listen(&mut win);
    let mut tape = Tape::from_env(seed, cfg.tick_hz).unwrap_or_else(|e| panic!("{e}"));
    ctx.rng = Rng::new(tape.seed().unwrap_or(seed));
    let mut stack = Stack::new();
    let scene = first(&mut ctx);
    enter(&mut stack, scene, &mut ctx);
    let mut last_poll = Instant::now();
    let (mut last, mut acc) = (Instant::now(), 0.0f32);
    let mut drawn = false;
    let mut win_size = win.get_size();
    let (mut stats, mut since, mut f3, mut f11) = (Stats::default(), Instant::now(), false, false);
    let mut presented = 0u32;
    while win.is_open() && !stack.is_empty() {
        let now = Instant::now();
        acc = (acc + (now - last).as_secs_f32()).min(0.25);
        last = now;
        // A file changed mid-replay would make the run differ from the recording.
        if cfg.hot_reload && !matches!(tape, Tape::Play(..)) && last_poll.elapsed().as_secs_f32() >= 0.5 {
            ctx.poll_files();
            last_poll = Instant::now();
        }
        let size = win.get_size();
        if size != win_size {
            win_size = size;
            drawn = false;
            #[cfg(feature = "gpu")]
            if let Backend::Gpu(gpu) = &mut backend {
                gpu.resize((size.0 as u32, size.1 as u32));
            }
        }
        ctx.input.poll(&win, (cfg.w, cfg.h));
        let f3_down = ctx.input.key_down(Key::F3);
        if f3_down && !f3 {
            gfx.debug = !gfx.debug;
        }
        f3 = f3_down;
        let f11_down = ctx.input.key_down(Key::F11);
        if f11_down && !f11 {
            ctx.toggle_fullscreen();
        }
        f11 = f11_down;
        // The window has to be on screen before the window manager takes the request.
        if ctx.fullscreen_asked && presented >= 2 {
            ctx.fullscreen_asked = false;
            if let Err(e) = fullscreen::set(win.get_window_handle(), ctx.fullscreen) {
                eprintln!("fullscreen: {e}");
                ctx.fullscreen = !ctx.fullscreen;
            }
        }
        let mut ticked = false;
        while acc >= dt && !stack.is_empty() {
            ticked = true;
            if !step(&mut stack, &mut ctx, &mut tape, dt) {
                break;
            }
            stats.ticks += 1;
            acc -= dt;
        }
        if stack.is_empty() {
            break;
        }
        // Nothing moves between ticks, so drawing and presenting again would only burn CPU (and, over a remote
        // display, bandwidth) on an identical picture.
        if !ticked && drawn {
            win.update();
            std::thread::sleep(std::time::Duration::from_secs_f32((dt - acc).max(0.001).min(dt)));
            continue;
        }
        drawn = true;
        gfx.cam = ctx.cam.offset().into();
        gfx.zoom = ctx.cam.zoom();
        for s in &stack {
            s.draw(&mut gfx);
        }
        if ctx.fade.alpha() > 0 {
            gfx.fill_alpha(Rect::new(0.0, 0.0, cfg.w as f32, cfg.h as f32), 0x000000, ctx.fade.alpha());
        }
        if gfx.debug {
            let line = format!("FPS {} TPS {} {:.2}ms cmds {}", stats.fps, stats.tps, stats.ms, gfx.cmds().len());
            #[cfg(feature = "gpu")]
            let line = match &backend {
                Backend::Gpu(gpu) => format!("{line} batches {}", gpu.batches()),
                Backend::Soft(_) => line,
            };
            gfx.fill(Rect::new(0.0, 0.0, line.len() as f32 * 8.0 + 8.0, 16.0), 0x000000);
            gfx.text(&line, 4, 4, 1, 0x66ff66);
        }
        stats.frames += 1;
        stats.work += now.elapsed().as_secs_f32();
        if since.elapsed().as_secs_f32() >= 1.0 {
            stats = Stats {
                fps: stats.frames,
                tps: stats.ticks,
                ms: stats.work / stats.frames as f32 * 1000.0,
                present_ms: stats.present / stats.frames as f32 * 1000.0,
                ..Stats::default()
            };
            if log_stats {
                eprintln!(
                    "fps {} tps {} work {:.2}ms present {:.2}ms",
                    stats.fps, stats.tps, stats.ms, stats.present_ms
                );
            }
            since = Instant::now();
        }
        let presenting = Instant::now();
        match &mut backend {
            Backend::Soft(soft) => {
                soft.render(gfx.cmds());
                win.update_with_buffer(soft.present(), cfg.w * scale, cfg.h * scale).expect("present");
            }
            #[cfg(feature = "gpu")]
            Backend::Gpu(gpu) => {
                gpu.render(gfx.cmds());
                win.update();
            }
        }
        stats.present += presenting.elapsed().as_secs_f32();
        presented += 1;
        gfx.reset();
    }
    apply(&mut stack, Trans::Quit, &mut ctx);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_config_with_no_usable_step_rate_or_size_is_refused_with_its_name() {
        Config::default().check();
        for (bad, field) in [
            (Config { tick_hz: 0.0, ..Config::default() }, "tick_hz"),
            (Config { tick_hz: -60.0, ..Config::default() }, "tick_hz"),
            (Config { tick_hz: f32::NAN, ..Config::default() }, "tick_hz"),
            (Config { tick_hz: f32::INFINITY, ..Config::default() }, "tick_hz"),
            (Config { w: 0, ..Config::default() }, "w and h"),
            (Config { h: 0, ..Config::default() }, "w and h"),
        ] {
            let message = std::panic::catch_unwind(|| bad.check()).unwrap_err();
            let message = message.downcast_ref::<String>().cloned().unwrap_or_default();
            assert!(message.contains(field), "{message}");
        }
    }
    #[test]
    fn fullscreen_is_asked_for_only_when_it_changes() {
        let mut ctx = Ctx::new(10, 10, 0.016, Audio::disabled());
        assert!(!ctx.fullscreen() && !ctx.fullscreen_asked);
        ctx.set_fullscreen(false);
        assert!(!ctx.fullscreen_asked, "no change, no request");
        ctx.toggle_fullscreen();
        assert!(ctx.fullscreen() && ctx.fullscreen_asked);
        ctx.fullscreen_asked = false;
        ctx.set_fullscreen(true);
        assert!(!ctx.fullscreen_asked, "asking for what it already is does nothing");
        ctx.toggle_fullscreen();
        assert!(!ctx.fullscreen() && ctx.fullscreen_asked);
    }

    use std::{cell::RefCell, rc::Rc};

    struct Probe(&'static str, Rc<RefCell<Vec<String>>>);

    impl Scene for Probe {
        fn update(&mut self, _: &mut Ctx) -> Trans {
            Trans::None
        }
        fn draw(&self, _: &mut Gfx) {}
        fn on_enter(&mut self, _: &mut Ctx) {
            self.1.borrow_mut().push(format!("{}+", self.0));
        }
        fn on_exit(&mut self, _: &mut Ctx) {
            self.1.borrow_mut().push(format!("{}-", self.0));
        }
    }

    type Log = Rc<RefCell<Vec<String>>>;

    // A scene that logs what happens to it and decides its own transitions from a closure.
    struct Script(&'static str, Log, Box<dyn FnMut(&mut Ctx) -> Trans>);

    impl Scene for Script {
        fn update(&mut self, ctx: &mut Ctx) -> Trans {
            self.1.borrow_mut().push(format!("{}:update", self.0));
            (self.2)(ctx)
        }
        fn draw(&self, _: &mut Gfx) {}
        fn on_enter(&mut self, _: &mut Ctx) {
            self.1.borrow_mut().push(format!("{}+", self.0));
        }
        fn on_exit(&mut self, _: &mut Ctx) {
            self.1.borrow_mut().push(format!("{}-", self.0));
        }
    }

    fn script(name: &'static str, log: &Log, f: impl FnMut(&mut Ctx) -> Trans + 'static) -> Box<dyn Scene> {
        Box::new(Script(name, log.clone(), Box::new(f)))
    }

    const DT: f32 = 0.05;

    fn world(first: Box<dyn Scene>) -> (Stack, Ctx) {
        let mut ctx = Ctx::new(64, 64, DT, Audio::disabled());
        ctx.bind("jump", &[Key::Space]);
        let mut stack = Stack::new();
        enter(&mut stack, first, &mut ctx);
        (stack, ctx)
    }

    fn recorded(frames: Vec<replay::Frame>) -> Tape {
        Tape::Play(replay::Recording { seed: 1, hz: None, frames }, 0)
    }

    fn held(keys: &[Key]) -> replay::Frame {
        replay::Frame { keys: keys.to_vec(), pads: vec![], mouse: None, buttons: [false; 5] }
    }

    #[test]
    fn a_fade_freezes_the_scene_changes_it_at_black_and_lets_the_new_scene_run() {
        let log: Log = Rc::default();
        let next = Rc::new(RefCell::new(Some(script("B", &log, |_| Trans::None))));
        let mut asked = false;
        let first = script("A", &log, move |_| {
            let scene = next.borrow_mut().take();
            match (asked, scene) {
                (false, Some(b)) => {
                    asked = true;
                    Trans::Fade(Box::new(Trans::Replace(b)), 0.2)
                }
                _ => Trans::None,
            }
        });
        let (mut stack, mut ctx) = world(first);
        let mut tape = Tape::Off;
        for _ in 0..4 {
            assert!(step(&mut stack, &mut ctx, &mut tape, DT));
        }
        // Step 1: A asks. Step 2: darkening, so A is not updated. Step 3: black, B takes over and runs at once.
        assert_eq!(*log.borrow(), ["A+", "A:update", "A-", "B+", "B:update", "B:update"]);
    }

    #[test]
    fn a_fade_of_no_time_and_a_quit_inside_a_fade_both_finish() {
        let log: Log = Rc::default();
        let mut asked = false;
        let first = script("A", &log, move |_| {
            if std::mem::replace(&mut asked, true) { Trans::None } else { Trans::Fade(Box::new(Trans::Quit), 0.0) }
        });
        let (mut stack, mut ctx) = world(first);
        let mut tape = Tape::Off;
        assert!(step(&mut stack, &mut ctx, &mut tape, DT));
        assert!(step(&mut stack, &mut ctx, &mut tape, DT));
        assert!(stack.is_empty(), "the zero-length fade handed over its Quit on the next step");
        assert_eq!(*log.borrow(), ["A+", "A:update", "A-"]);
        assert!(step(&mut stack, &mut ctx, &mut tape, DT), "stepping an empty stack does nothing");
    }

    #[test]
    fn a_replay_feeds_the_scene_its_recorded_input_and_ends_the_run_when_it_runs_out() {
        let log: Log = Rc::default();
        let seen: Log = Rc::default();
        let down = seen.clone();
        let first = script("A", &log, move |ctx| {
            down.borrow_mut().push(format!(
                "down {} pressed {}",
                ctx.input().down("jump"),
                ctx.input().pressed("jump")
            ));
            Trans::None
        });
        let (mut stack, mut ctx) = world(first);
        let mut tape = recorded(vec![held(&[Key::Space]), held(&[Key::Space]), held(&[])]);
        for _ in 0..3 {
            assert!(step(&mut stack, &mut ctx, &mut tape, DT));
        }
        assert_eq!(
            *seen.borrow(),
            ["down true pressed true", "down true pressed false", "down false pressed false"],
            "the live keyboard is ignored, and an edge shows once because the step ends after the update"
        );
        assert!(!step(&mut stack, &mut ctx, &mut tape, DT), "no recorded steps left");
        assert!(stack.is_empty());
        assert_eq!(log.borrow().last().map(String::as_str), Some("A-"), "scenes are told they are leaving");
    }

    #[test]
    fn a_recorded_run_with_physics_and_random_spawns_replays_to_the_same_world() {
        use crate::{Body, Physics, Rect, RigidBody, Vel, World, WorldFile};

        let file = WorldFile::new().component::<Body>("body").component::<Vel>("vel");
        let play = |mut tape: Tape| {
            let bodies = Rc::new(RefCell::new(World::new()));
            let log: Log = Rc::default();
            let shared = bodies.clone();
            let mut physics = Physics::default();
            let first = script("A", &log, move |ctx| {
                let mut w = shared.borrow_mut();
                if w.is_empty() {
                    w.spawn((Body(Rect::new(0.0, 60.0, 200.0, 10.0)), RigidBody::fixed()));
                }
                if ctx.input().pressed("jump") {
                    let x = ctx.rng().range(0.0, 180.0);
                    w.spawn((Body(Rect::new(x, 0.0, 8.0, 8.0)), Vel { x: 0.0, y: 0.0 }, RigidBody::dynamic(1.0)));
                }
                physics.step(&mut w, None, DT);
                Trans::None
            });
            let (mut stack, mut ctx) = world(first);
            ctx.rng = Rng::new(7);
            for i in 0..120 {
                let keys: &[Key] = if i % 7 < 2 { &[Key::Space] } else { &[] };
                ctx.input.hold(keys);
                assert!(step(&mut stack, &mut ctx, &mut tape, DT));
            }
            let sum = file.checksum(&bodies.borrow());
            (sum, tape)
        };
        let sink = std::env::temp_dir().join(format!("engine-replay-run-{}.rec", std::process::id()));
        let record =
            Tape::Record(sink.to_string_lossy().into_owned(), replay::Recording { seed: 7, hz: None, frames: vec![] });
        let (first, mut tape) = play(record);
        let Tape::Record(_, rec) = &mut tape else { panic!() };
        let frames = std::mem::take(&mut rec.frames);
        drop(tape);
        std::fs::remove_file(&sink).ok();
        assert_eq!(frames.len(), 120);
        let (second, _) = play(recorded(frames));
        assert_eq!(first, second);
        assert_ne!(first, file.checksum(&World::new()), "something fell");
    }

    #[test]
    fn bindings_save_and_load_through_a_file_and_a_missing_file_keeps_the_defaults() {
        let path = std::env::temp_dir().join(format!("engine-bindings-{}.txt", std::process::id()));
        let path = path.to_str().unwrap();
        let mut ctx = Ctx::new(64, 64, 1.0 / 60.0, Audio::disabled());
        ctx.bind("jump", &[Key::Space, Key::Up]);
        ctx.save_bindings(path).unwrap();

        let mut other = Ctx::new(64, 64, 1.0 / 60.0, Audio::disabled());
        other.bind("jump", &[Key::Z]);
        other.load_bindings(path).unwrap();
        assert_eq!(other.input.to_text(), "jump = Space Up\n");

        std::fs::write(path, "jump = Nope\n").unwrap();
        let err = other.load_bindings(path).unwrap_err();
        assert!(err.starts_with(path) && err.contains("line 1"), "{err}");
        assert_eq!(other.input.to_text(), "jump = Space Up\n");

        std::fs::remove_file(path).unwrap();
        assert!(other.load_bindings(path).is_ok());

        std::fs::write(path, "jmp = Space\n").unwrap();
        assert!(other.load_bindings(path).unwrap_err().contains("unknown action"));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn saving_keeps_only_the_bindings_the_player_changed() {
        let path = std::env::temp_dir().join(format!("engine-diff-{}.txt", std::process::id()));
        let path = path.to_str().unwrap();
        let mut ctx = Ctx::new(64, 64, 1.0 / 60.0, Audio::disabled());
        ctx.bind("jump", &[Key::Space]);
        ctx.bind("left", &[Key::Left]);
        ctx.load_bindings(path).unwrap();
        ctx.bind("left", &[Key::A]);
        ctx.save_bindings(path).unwrap();
        assert_eq!(std::fs::read_to_string(path).unwrap(), "left = A\n");

        let mut next = Ctx::new(64, 64, 1.0 / 60.0, Audio::disabled());
        next.bind("jump", &[Key::Z]);
        next.bind("left", &[Key::Left]);
        next.load_bindings(path).unwrap();
        assert_eq!(next.input.to_text(), "jump = Z\nleft = A\n", "a new default reaches this player");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn scenes_are_told_when_they_join_and_leave_the_stack() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let probe = |name| Box::new(Probe(name, log.clone()));
        let mut ctx = Ctx::new(64, 64, 1.0 / 60.0, Audio::disabled());
        let mut stack = Stack::new();
        apply(&mut stack, Trans::Push(probe("A")), &mut ctx);
        apply(&mut stack, Trans::Push(probe("B")), &mut ctx);
        apply(&mut stack, Trans::Pop, &mut ctx);
        apply(&mut stack, Trans::Replace(probe("C")), &mut ctx);
        apply(&mut stack, Trans::Quit, &mut ctx);
        assert_eq!(*log.borrow(), ["A+", "B+", "B-", "A-", "C+", "C-"]);
        assert!(stack.is_empty());
    }
}
