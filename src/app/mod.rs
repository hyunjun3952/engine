mod input;
mod rng;

pub use input::{Input, Key};
pub use rng::Rng;

use crate::{Assets, Audio, Camera, Gfx, Rect};
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
    pub(crate) input: Input,
    pub(crate) assets: Assets,
    pub(crate) audio: Audio,
    pub(crate) cam: Camera,
    pub(crate) rng: Rng,
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
            cam: Camera::new(w as f32, h as f32),
            rng: Rng::new(1),
            dt,
            w,
            h,
        }
    }

    pub fn input(&self) -> &Input {
        &self.input
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
    }
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
    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.subsec_nanos());
    let mut ctx = Ctx::new(cfg.w, cfg.h, dt, Audio::new());
    ctx.rng = Rng::new(seed);
    let mut stack = Stack::new();
    let scene = first(&mut ctx);
    enter(&mut stack, scene, &mut ctx);
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
            let trans = stack.last_mut().unwrap().update(&mut ctx);
            apply(&mut stack, trans, &mut ctx);
            ctx.input.end_step();
            ctx.cam.tick(dt);
            stats.ticks += 1;
            acc -= dt;
        }
        if stack.is_empty() {
            break;
        }
        gfx.cam = ctx.cam.offset();
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
    apply(&mut stack, Trans::Quit, &mut ctx);
}

#[cfg(test)]
mod tests {
    use super::*;
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
