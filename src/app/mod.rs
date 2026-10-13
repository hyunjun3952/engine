mod fade;
mod input;
mod pad;
mod replay;
mod rng;
mod save;
mod time;

pub use input::{Input, Key, MouseButton};
pub use pad::{Pad, PadAxis};
pub use rng::Rng;
pub use save::Save;
pub use time::{Ease, Timer, Tween};

#[cfg(feature = "gpu")]
use crate::render::Gpu;
use crate::render::Soft;
use crate::{Assets, Audio, Camera, Gfx, Rect};
use fade::Fade;
use minifb::{ScaleMode, Window, WindowOptions};
use replay::Tape;
use std::time::Instant;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RenderMode {
    Auto,
    Gpu,
    Software,
}

pub struct Config {
    pub title: &'static str,
    pub w: usize,
    pub h: usize,
    pub tick_hz: f32,
    pub renderer: RenderMode,
    pub hot_reload: bool,
    pub scale: usize,
    /// Lets the player drag the window to any size; the picture keeps its shape and the rest is black.
    pub resizable: bool,
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
        }
    }
}

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
}

impl Ctx {
    pub(crate) fn new(w: usize, h: usize, dt: f32, audio: Audio) -> Self {
        Ctx {
            input: Input::default(),
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
        }
    }

    pub fn bind(&mut self, action: &str, keys: &[Key]) {
        self.input.bind(action, keys);
    }

    pub fn bind_pad(&mut self, action: &str, pads: &[Pad]) {
        self.input.bind_pad(action, pads);
    }

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
    pub fn save_bindings(&self, path: &str) -> std::io::Result<()> {
        let all = self.input.to_text();
        let kept = |line: &&str| self.defaults.as_ref().is_none_or(|d| !d.lines().any(|l| l == *line));
        crate::assets::write_atomic(path, &all.lines().filter(kept).map(|l| format!("{l}\n")).collect::<String>())
    }

    pub fn watch(&mut self, path: &str) {
        self.assets.watcher.watch(path);
    }

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

    pub fn input(&self) -> &Input {
        &self.input
    }

    /// The mouse in world coordinates under the current camera (`input().mouse()` is in screen pixels).
    pub fn mouse_world(&self) -> Option<(f32, f32)> {
        self.input.mouse().map(|p| self.cam.to_world(p))
    }

    pub fn assets(&mut self) -> &mut Assets {
        &mut self.assets
    }

    pub fn audio(&mut self) -> &mut Audio {
        &mut self.audio
    }

    pub fn cam(&mut self) -> &mut Camera {
        &mut self.cam
    }

    pub fn rng(&mut self) -> &mut Rng {
        &mut self.rng
    }

    pub fn dt(&self) -> f32 {
        self.dt
    }

    pub fn size(&self) -> (usize, usize) {
        (self.w, self.h)
    }
}

pub enum Trans {
    None,
    Push(Box<dyn Scene>),
    Pop,
    Replace(Box<dyn Scene>),
    Quit,
    /// Fades to black over half of `secs`, makes the inner change, and fades back over the other half. The scenes
    /// stand still while it darkens.
    Fade(Box<Trans>, f32),
}

pub trait Scene {
    fn update(&mut self, ctx: &mut Ctx) -> Trans;
    fn draw(&self, gfx: &mut Gfx);
    fn on_enter(&mut self, _ctx: &mut Ctx) {}
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

/// One fixed step, in this order: a recording replaces the live input, a fade advances (and may change scenes at its
/// darkest moment), the top scene updates unless a fade is darkening, then the step's bookkeeping. False when a
/// replay has run out, after quitting every scene.
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
    // Mesa only exposes the WSL GPU through its D3D12 driver, and only when asked.
    if std::env::var_os("GALLIUM_DRIVER").is_none() && std::path::Path::new("/dev/dxg").exists() {
        unsafe { std::env::set_var("GALLIUM_DRIVER", "d3d12") };
    }
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
        _ => mode,
    };
    let gpu = if mode == RenderMode::Software { None } else { try_gpu(mode, win, (w, h), scale) };
    gpu.unwrap_or_else(|| Backend::Soft(Soft::new(w, h, scale)))
}

pub fn run(cfg: Config, first: impl FnOnce(&mut Ctx) -> Box<dyn Scene>) {
    cfg.check();
    let scale = std::env::var("ENGINE_SCALE").ok().and_then(|v| v.parse().ok()).unwrap_or(cfg.scale).max(1);
    let options =
        WindowOptions { resize: cfg.resizable, scale_mode: ScaleMode::AspectRatioStretch, ..Default::default() };
    let mut win = Window::new(cfg.title, cfg.w * scale, cfg.h * scale, options).expect("window");
    let mut gfx = Gfx::new(cfg.w, cfg.h);
    let log_stats = std::env::var_os("ENGINE_STATS").is_some();
    let mut backend = pick_backend(cfg.renderer, &win, (cfg.w, cfg.h), scale);
    let dt = 1.0 / cfg.tick_hz;
    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.subsec_nanos());
    let mut ctx = Ctx::new(cfg.w, cfg.h, dt, Audio::new());
    ctx.input.listen(&mut win);
    let mut tape = Tape::from_env(seed).unwrap_or_else(|e| panic!("{e}"));
    ctx.rng = Rng::new(tape.seed().unwrap_or(seed));
    let mut stack = Stack::new();
    let scene = first(&mut ctx);
    enter(&mut stack, scene, &mut ctx);
    let mut last_poll = Instant::now();
    let (mut last, mut acc) = (Instant::now(), 0.0f32);
    let mut drawn = false;
    let mut win_size = win.get_size();
    let (mut stats, mut since, mut f3) = (Stats::default(), Instant::now(), false);
    while win.is_open() && !stack.is_empty() {
        let now = Instant::now();
        acc = (acc + (now - last).as_secs_f32()).min(0.25);
        last = now;
        if cfg.hot_reload && last_poll.elapsed().as_secs_f32() >= 0.5 {
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
        gfx.cam = ctx.cam.offset();
        gfx.zoom = ctx.cam.zoom();
        for s in &stack {
            s.draw(&mut gfx);
        }
        if ctx.fade.alpha() > 0 {
            gfx.fill_alpha(&Rect { x: 0.0, y: 0.0, w: cfg.w as f32, h: cfg.h as f32 }, 0x000000, ctx.fade.alpha());
        }
        if gfx.debug {
            let line = format!("FPS {} TPS {} {:.2}ms", stats.fps, stats.tps, stats.ms);
            gfx.fill(&Rect { x: 0.0, y: 0.0, w: line.len() as f32 * 8.0 + 8.0, h: 16.0 }, 0x000000);
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
        gfx.reset();
    }
    apply(&mut stack, Trans::Quit, &mut ctx);
    tape.finish();
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
        Tape::Play(replay::Recording { seed: 1, frames }, 0)
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
    fn bindings_save_and_load_through_a_file_and_a_missing_file_keeps_the_defaults() {
        let path = std::env::temp_dir().join(format!("engine-bindings-{}.txt", std::process::id()));
        let path = path.to_str().unwrap();
        let mut ctx = Ctx::new(64, 64, 1.0 / 60.0, Audio::disabled());
        ctx.bind("jump", &[Key::Space, Key::Up]);
        ctx.save_bindings(path).unwrap();

        let mut other = Ctx::new(64, 64, 1.0 / 60.0, Audio::disabled());
        other.load_bindings(path).unwrap();
        assert_eq!(other.input.to_text(), "jump = Space Up\n");

        std::fs::write(path, "jump = Nope\n").unwrap();
        let err = other.load_bindings(path).unwrap_err();
        assert!(err.starts_with(path) && err.contains("line 1"), "{err}");
        assert_eq!(other.input.to_text(), "jump = Space Up\n");

        std::fs::remove_file(path).unwrap();
        assert!(other.load_bindings(path).is_ok());
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
