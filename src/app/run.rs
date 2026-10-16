use super::{Config, Ctx, RenderMode, Scene, Stack, Trans, apply, enter, fullscreen, step};
use crate::base::env_flag;
use crate::input::replay::Tape;
#[cfg(feature = "gpu")]
use crate::render::Gpu;
use crate::render::Soft;
use crate::{Audio, Gfx, Key, Rect, Rng};
use minifb::{ScaleMode, Window, WindowOptions};
use std::time::Instant;

#[derive(Default)]
pub(super) struct Stats {
    pub(super) frames: u32,
    pub(super) ticks: u32,
    pub(super) work: f32,
    pub(super) present: f32,
    pub(super) fps: u32,
    pub(super) tps: u32,
    pub(super) ms: f32,
    pub(super) present_ms: f32,
}

pub(super) enum Backend {
    Soft(Soft),
    #[cfg(feature = "gpu")]
    Gpu(Box<Gpu>),
}

#[cfg(feature = "gpu")]
pub(super) fn try_gpu(mode: RenderMode, win: &Window, (w, h): (usize, usize), scale: usize) -> Option<Backend> {
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
pub(super) fn try_gpu(mode: RenderMode, _win: &Window, _size: (usize, usize), _scale: usize) -> Option<Backend> {
    if mode == RenderMode::Gpu {
        eprintln!("renderer: software (built without the gpu feature)");
    }
    None
}

pub(super) fn pick_backend(mode: RenderMode, win: &Window, (w, h): (usize, usize), scale: usize) -> Backend {
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

// The most time a frame may owe the fixed steps; a step longer than 0.25 s must still fit.
pub(super) fn lag_cap(dt: f32) -> f32 {
    0.25f32.max(2.0 * dt)
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
        acc = (acc + (now - last).as_secs_f32()).min(lag_cap(dt));
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
        gfx.cam = ctx.cam.view_offset().into();
        gfx.zoom = ctx.cam.zoom();
        for s in &stack {
            s.draw(&mut gfx);
        }
        if ctx.fade.alpha() > 0 {
            gfx.fill_alpha(Rect::new(0.0, 0.0, cfg.w as f32, cfg.h as f32), 0x000000, ctx.fade.alpha());
        }
        if gfx.debug {
            let line = format!("FPS {} TPS {} {:.2}ms cmds {}", stats.fps, stats.tps, stats.ms, gfx.draw_cmds().len());
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
                soft.render(gfx.draw_cmds());
                win.update_with_buffer(soft.present(), cfg.w * scale, cfg.h * scale).expect("present");
            }
            #[cfg(feature = "gpu")]
            Backend::Gpu(gpu) => {
                gpu.render(gfx.draw_cmds());
                win.update();
            }
        }
        stats.present += presenting.elapsed().as_secs_f32();
        presented += 1;
        gfx.clear_cmds();
    }
    apply(&mut stack, Trans::Quit, &mut ctx);
}
