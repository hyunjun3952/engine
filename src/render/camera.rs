use crate::{Ease, Rect, Rng, Timer, Tween, Vec2};

const FOLLOW_SPEED: f32 = 10.0;
const MIN_ZOOM: f32 = 0.25;
const MAX_ZOOM: f32 = 8.0;

/// The one camera of the app, reached through `ctx.cam()`; it advances in the fixed update and persists across scenes.
///
/// `follow` eases towards a target and clamps to the world (the first call snaps; `cut` makes the next one snap),
/// `shake` jitters, `set_zoom`/`zoom_to` zoom about the middle of the view (0.25..=8). It moves what [`Gfx::draw`](crate::Gfx::draw)
/// draws; HUD drawn with `fill`, `blit` and `text` stays in screen space. Game scenes call `ctx.cam().reset()` in
/// `on_enter`.
///
/// ```
/// use engine::{Ctx, Rect};
///
/// fn follow_player(ctx: &mut Ctx, player: Rect) {
///     let dt = ctx.dt();
///     ctx.cam().follow(player, (1600.0, 900.0), dt);
/// }
/// ```
pub struct Camera {
    // The top-left of the part of the world in view, in world pixels.
    pos: (f32, f32),
    view: (f32, f32),
    placed: bool,
    shake: Option<(f32, Timer)>,
    jitter: (f32, f32),
    rng: Rng,
    zoom: f32,
    zooming: Option<Tween>,
}

impl Camera {
    pub(crate) fn new(view_w: f32, view_h: f32) -> Self {
        Camera {
            pos: (0.0, 0.0),
            view: (view_w, view_h),
            placed: false,
            shake: None,
            jitter: (0.0, 0.0),
            rng: Rng::new(1),
            zoom: 1.0,
            zooming: None,
        }
    }

    // The size of the part of the world in view: smaller than the screen when zoomed in.
    fn visible(&self) -> (f32, f32) {
        (self.view.0 / self.zoom, self.view.1 / self.zoom)
    }

    /// The current zoom.
    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    /// Magnifies the picture by `z` (2 shows half the width and height, 0.5 shows twice), clamped to 0.25..=8, around
    /// the middle of the view, and stops any `zoom_to` under way. Whole numbers keep pixel art even.
    pub fn set_zoom(&mut self, z: f32) {
        self.zooming = None;
        self.apply_zoom(z);
    }

    /// Eases the zoom to `z` over `secs` seconds, from the fixed update (`tick`).
    pub fn zoom_to(&mut self, z: f32, secs: f32) {
        if secs.is_nan() || secs <= 0.0 {
            return self.set_zoom(z);
        }
        self.zooming = Some(Tween::new(self.zoom, z.clamp(MIN_ZOOM, MAX_ZOOM), secs, Ease::OutQuad));
    }

    fn apply_zoom(&mut self, z: f32) {
        if !z.is_finite() {
            return;
        }
        let (vw, vh) = self.visible();
        let (cx, cy) = (self.pos.0 + vw / 2.0, self.pos.1 + vh / 2.0);
        self.zoom = z.clamp(MIN_ZOOM, MAX_ZOOM);
        let (vw, vh) = self.visible();
        self.pos = (cx - vw / 2.0, cy - vh / 2.0);
    }

    /// Eases towards `target` and stays inside a world of the given size; the first call snaps.
    pub fn follow(&mut self, target: Rect, world: impl Into<Vec2>, dt: f32) {
        if !dt.is_finite() {
            return;
        }
        let (tx, ty, world) = (target.center().x, target.center().y, world.into());
        let (vw, vh) = self.visible();
        let goal =
            ((tx - vw / 2.0).clamp(0.0, (world.x - vw).max(0.0)), (ty - vh / 2.0).clamp(0.0, (world.y - vh).max(0.0)));
        // One frame of a position that is not a number would otherwise leave the camera that way for good.
        if !(goal.0.is_finite() && goal.1.is_finite()) {
            return;
        }
        if !self.placed {
            (self.pos, self.placed) = (goal, true);
            return;
        }
        let k = 1.0 - (-FOLLOW_SPEED * dt).exp();
        self.pos = (self.pos.0 + (goal.0 - self.pos.0) * k, self.pos.1 + (goal.1 - self.pos.1) * k);
    }

    /// Forgets the position, zoom and shake; call it in `on_enter`.
    pub fn reset(&mut self) {
        *self = Camera::new(self.view.0, self.view.1);
    }

    /// Makes the next `follow` snap instead of ease.
    pub fn cut(&mut self) {
        self.placed = false;
    }

    /// Jitters the view by up to `amplitude` pixels for `secs`.
    pub fn shake(&mut self, amplitude: f32, secs: f32) {
        let amplitude = if amplitude.is_finite() { amplitude.max(0.0) } else { 0.0 };
        self.shake = Some((amplitude, Timer::once(secs.max(0.0))));
    }

    pub(crate) fn tick(&mut self, dt: f32) {
        if let Some(tween) = &mut self.zooming {
            let z = tween.tick(dt);
            if tween.done() {
                self.zooming = None;
            }
            self.apply_zoom(z);
        }
        let Some((amplitude, timer)) = &mut self.shake else {
            self.jitter = (0.0, 0.0);
            return;
        };
        timer.tick(dt);
        let strength = *amplitude * (1.0 - timer.fraction());
        if timer.done() {
            self.shake = None;
        }
        self.jitter = (self.rng.range(-strength, strength), self.rng.range(-strength, strength));
    }

    /// A screen position as a world position, ignoring shake so a pointer does not jitter with it.
    pub fn to_world(&self, p: impl Into<Vec2>) -> Vec2 {
        let p = p.into();
        Vec2::new(self.pos.0.floor() + p.x / self.zoom, self.pos.1.floor() + p.y / self.zoom)
    }

    pub(crate) fn view_offset(&self) -> (f32, f32) {
        ((self.pos.0 + self.jitter.0).floor(), (self.pos.1 + self.jitter.1).floor())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f32) -> Rect {
        Rect { x, y: 0.0, w: 0.0, h: 0.0 }
    }

    #[test]
    fn first_follow_snaps_then_eases_toward_the_target() {
        let mut c = Camera::new(100.0, 100.0);
        c.follow(at(500.0), (1000.0, 1000.0), 1.0 / 60.0);
        assert_eq!(c.view_offset().0, 450.0);
        c.follow(at(600.0), (1000.0, 1000.0), 1.0 / 60.0);
        assert!(c.view_offset().0 > 450.0 && c.view_offset().0 < 550.0);
        for _ in 0..600 {
            c.follow(at(600.0), (1000.0, 1000.0), 1.0 / 60.0);
        }
        assert!((c.view_offset().0 - 550.0).abs() <= 1.0);
    }

    #[test]
    fn screen_points_map_to_world_without_shake() {
        let mut c = Camera::new(100.0, 100.0);
        c.follow(at(500.0), (1000.0, 1000.0), 0.0);
        c.shake(5.0, 1.0);
        c.tick(0.1);
        assert_eq!(c.to_world((10.0, 4.0)), Vec2::new(460.0, 4.0));
    }

    #[test]
    fn zooming_keeps_the_middle_of_the_view_and_clamps_to_its_range() {
        let mut c = Camera::new(100.0, 80.0);
        c.follow(at(500.0), (1000.0, 1000.0), 0.0);
        let middle = (c.view_offset().0 + 50.0, c.view_offset().1 + 40.0);
        c.set_zoom(2.0);
        assert_eq!((c.view_offset().0 + 25.0, c.view_offset().1 + 20.0), middle);
        assert_eq!(c.to_world((50.0, 40.0)), middle.into());
        assert_eq!(c.to_world((0.0, 0.0)), Vec2::new(middle.0 - 25.0, middle.1 - 20.0));
        c.set_zoom(100.0);
        assert_eq!(c.zoom(), 8.0);
        c.set_zoom(0.0);
        assert_eq!(c.zoom(), 0.25);
        c.set_zoom(f32::NAN);
        assert_eq!(c.zoom(), 0.25, "not a number leaves the zoom alone");
        c.reset();
        assert_eq!(c.zoom(), 1.0);
    }

    #[test]
    fn follow_uses_the_part_of_the_world_in_view_when_zoomed() {
        let mut c = Camera::new(100.0, 100.0);
        c.set_zoom(2.0);
        c.follow(at(500.0), (1000.0, 1000.0), 0.0);
        assert_eq!(c.view_offset().0, 475.0);
        c.follow(at(2000.0), (1000.0, 1000.0), 10.0);
        assert_eq!(c.view_offset().0, 950.0, "the world ends 50 px of view from its edge");
        c.set_zoom(0.5);
        c.follow(at(500.0), (100.0, 100.0), 10.0);
        assert_eq!(c.view_offset(), (0.0, 0.0), "a view bigger than the world stays at its corner");
    }

    #[test]
    fn a_negative_or_missing_duration_is_not_a_crash() {
        let mut c = Camera::new(100.0, 100.0);
        c.shake(4.0, -1.0);
        c.shake(4.0, f32::NAN);
        c.zoom_to(2.0, f32::NAN);
        assert_eq!(c.zoom(), 2.0);
        c.tick(0.1);
    }

    #[test]
    fn zoom_to_eases_and_lands_exactly_on_the_target() {
        let mut c = Camera::new(100.0, 100.0);
        c.zoom_to(3.0, 1.0);
        c.tick(0.5);
        assert!(c.zoom() > 1.0 && c.zoom() < 3.0, "{}", c.zoom());
        c.tick(0.5);
        assert_eq!(c.zoom(), 3.0);
        c.tick(1.0);
        assert_eq!(c.zoom(), 3.0);
        c.zoom_to(1.0, 0.0);
        assert_eq!(c.zoom(), 1.0, "no time is a jump");
        c.zoom_to(4.0, 1.0);
        c.set_zoom(2.0);
        c.tick(1.0);
        assert_eq!(c.zoom(), 2.0, "set_zoom cancels the ease");
    }

    #[test]
    fn a_target_that_is_not_a_number_for_a_moment_does_not_ruin_the_camera() {
        let mut c = Camera::new(100.0, 100.0);
        c.follow(at(500.0), (1000.0, 1000.0), 0.0);
        c.follow(Rect::new(f32::NAN, 0.0, 0.0, 0.0), (1000.0, 1000.0), 0.1);
        assert_eq!(c.view_offset(), (450.0, 0.0), "NaN is ignored");
        for far in [f32::INFINITY, f32::NEG_INFINITY] {
            c.follow(Rect::new(far, 0.0, 0.0, 0.0), (1000.0, 1000.0), 10.0);
            assert!((0.0..=900.0).contains(&c.view_offset().0), "{far} is clamped into the world like any far target");
        }
        c.follow(at(600.0), (1000.0, 1000.0), 10.0);
        assert_eq!(c.view_offset().0, 550.0, "and the next real target is followed as usual");
        let mut fresh = Camera::new(100.0, 100.0);
        fresh.follow(Rect::new(f32::NAN, 0.0, 0.0, 0.0), (1000.0, 1000.0), 0.0);
        fresh.follow(at(500.0), (1000.0, 1000.0), 0.0);
        assert_eq!(fresh.view_offset().0, 450.0, "a first target that is not a number does not count as the first");
    }

    #[test]
    fn follow_stays_inside_the_world() {
        let mut c = Camera::new(100.0, 100.0);
        c.follow(at(5.0), (1000.0, 1000.0), 0.0);
        assert_eq!(c.view_offset(), (0.0, 0.0));
        c.follow(at(2000.0), (1000.0, 1000.0), 10.0);
        assert_eq!(c.view_offset().0, 900.0);
    }

    #[test]
    fn shake_moves_the_view_then_settles() {
        let mut c = Camera::new(100.0, 100.0);
        c.follow(at(500.0), (1000.0, 1000.0), 0.0);
        c.shake(8.0, 0.2);
        c.tick(0.05);
        assert_ne!(c.view_offset(), (450.0, 0.0));
        for _ in 0..30 {
            c.tick(0.02);
        }
        assert_eq!(c.view_offset(), (450.0, 0.0));
    }

    #[test]
    fn a_shake_that_is_not_a_finite_amount_does_nothing() {
        for amplitude in [f32::NAN, f32::INFINITY, -3.0] {
            let mut c = Camera::new(100.0, 100.0);
            c.shake(amplitude, 0.1);
            c.tick(0.05);
            c.tick(0.05);
            assert_eq!(c.view_offset(), (0.0, 0.0), "{amplitude}");
        }
    }
}
