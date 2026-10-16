use super::Ctx;
use crate::Gfx;
use crate::input::replay::Tape;

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

pub(super) type Stack = Vec<Box<dyn Scene>>;

pub(super) fn enter(stack: &mut Stack, mut scene: Box<dyn Scene>, ctx: &mut Ctx) {
    scene.on_enter(ctx);
    stack.push(scene);
}

pub(super) fn exit_top(stack: &mut Stack, ctx: &mut Ctx) {
    if let Some(mut scene) = stack.pop() {
        scene.on_exit(ctx);
    }
}

pub(super) fn apply(stack: &mut Stack, trans: Trans, ctx: &mut Ctx) {
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
pub(super) fn step(stack: &mut Stack, ctx: &mut Ctx, tape: &mut Tape, dt: f32) -> bool {
    if !tape.feed_input(&mut ctx.input) {
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
