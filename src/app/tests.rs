use super::run::lag_cap;
use super::*;
use crate::input::replay::Tape;
use crate::{Audio, Gfx, Key, Rng};

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
fn a_step_longer_than_the_lag_cap_still_fits_in_it() {
    assert_eq!(lag_cap(1.0 / 60.0), 0.25);
    assert!(lag_cap(0.5) >= 0.5);
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

fn recorded(frames: Vec<crate::input::replay::Frame>) -> Tape {
    Tape::Play(crate::input::replay::Recording { seed: 1, hz: None, frames }, 0)
}

fn held(keys: &[Key]) -> crate::input::replay::Frame {
    crate::input::replay::Frame { keys: keys.to_vec(), pads: vec![], mouse: None, buttons: [false; 5] }
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
        down.borrow_mut().push(format!("down {} pressed {}", ctx.input().down("jump"), ctx.input().pressed("jump")));
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
    let record = Tape::Record(
        sink.to_string_lossy().into_owned(),
        crate::input::replay::Recording { seed: 7, hz: None, frames: vec![] },
    );
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
