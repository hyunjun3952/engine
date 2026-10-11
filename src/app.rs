use crate::{Assets, Audio, Gfx, Input, Key, Rect};
use minifb::{Window, WindowOptions};
use std::time::Instant;

pub struct Config {
    pub title: &'static str,
    pub w: usize,
    pub h: usize,
    pub tick_hz: f32,
}

impl Default for Config {
    fn default() -> Self {
        Config { title: "engine", w: 640, h: 360, tick_hz: 60.0 }
    }
}

pub struct Ctx {
    pub input: Input,
    pub assets: Assets,
    pub audio: Audio,
    pub dt: f32,
    pub w: usize,
    pub h: usize,
}

pub enum Trans {
    None,
    Push(Box<dyn Scene>),
    Pop,
    Replace(Box<dyn Scene>),
    Quit,
}

pub trait Scene {
    fn update(&mut self, ctx: &mut Ctx) -> Trans;
    fn draw(&self, gfx: &mut Gfx);
}

#[derive(Default)]
struct Stats {
    frames: u32,
    ticks: u32,
    work: f32,
    fps: u32,
    tps: u32,
    ms: f32,
}

pub fn run(cfg: Config, first: impl FnOnce(&mut Ctx) -> Box<dyn Scene>) {
    let mut win = Window::new(cfg.title, cfg.w, cfg.h, WindowOptions::default()).expect("window");
    let mut gfx = Gfx::new(cfg.w, cfg.h);
    let dt = 1.0 / cfg.tick_hz;
    let mut ctx =
        Ctx { input: Input::default(), assets: Assets::default(), audio: Audio::new(), dt, w: cfg.w, h: cfg.h };
    let mut stack = vec![first(&mut ctx)];
    let (mut last, mut acc) = (Instant::now(), 0.0f32);
    let (mut stats, mut since, mut f3) = (Stats::default(), Instant::now(), false);
    while win.is_open() && !stack.is_empty() {
        let now = Instant::now();
        acc = (acc + (now - last).as_secs_f32()).min(0.25);
        last = now;
        ctx.input.poll(&win);
        let f3_down = ctx.input.down(Key::F3);
        if f3_down && !f3 {
            gfx.debug = !gfx.debug;
        }
        f3 = f3_down;
        while acc >= dt && !stack.is_empty() {
            match stack.last_mut().unwrap().update(&mut ctx) {
                Trans::None => {}
                Trans::Push(s) => stack.push(s),
                Trans::Pop => {
                    stack.pop();
                }
                Trans::Replace(s) => {
                    stack.pop();
                    stack.push(s);
                }
                Trans::Quit => stack.clear(),
            }
            ctx.input.end_step();
            stats.ticks += 1;
            acc -= dt;
        }
        if stack.is_empty() {
            break;
        }
        for s in &stack {
            s.draw(&mut gfx);
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
                ..Stats::default()
            };
            since = Instant::now();
        }
        win.update_with_buffer(&gfx.buf, cfg.w, cfg.h).expect("present");
    }
}
