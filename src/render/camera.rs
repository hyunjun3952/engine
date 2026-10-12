use crate::{Rect, Rng, Timer};

const FOLLOW_SPEED: f32 = 10.0;

pub struct Camera {
    pos: (f32, f32),
    view: (f32, f32),
    placed: bool,
    shake: Option<(f32, Timer)>,
    jitter: (f32, f32),
    rng: Rng,
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
        }
    }

    pub fn follow(&mut self, target: &Rect, world: (f32, f32), dt: f32) {
        let (tx, ty) = target.center();
        let goal = (
            (tx - self.view.0 / 2.0).clamp(0.0, (world.0 - self.view.0).max(0.0)),
            (ty - self.view.1 / 2.0).clamp(0.0, (world.1 - self.view.1).max(0.0)),
        );
        if !self.placed {
            (self.pos, self.placed) = (goal, true);
            return;
        }
        let k = 1.0 - (-FOLLOW_SPEED * dt).exp();
        self.pos = (self.pos.0 + (goal.0 - self.pos.0) * k, self.pos.1 + (goal.1 - self.pos.1) * k);
    }

    pub fn reset(&mut self) {
        *self = Camera::new(self.view.0, self.view.1);
    }

    pub fn cut(&mut self) {
        self.placed = false;
    }

    pub fn shake(&mut self, amplitude: f32, secs: f32) {
        self.shake = Some((amplitude, Timer::once(secs)));
    }

    pub(crate) fn tick(&mut self, dt: f32) {
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
    pub fn to_world(&self, (x, y): (f32, f32)) -> (f32, f32) {
        (x + self.pos.0.floor(), y + self.pos.1.floor())
    }

    pub(crate) fn offset(&self) -> (f32, f32) {
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
        c.follow(&at(500.0), (1000.0, 1000.0), 1.0 / 60.0);
        assert_eq!(c.offset().0, 450.0);
        c.follow(&at(600.0), (1000.0, 1000.0), 1.0 / 60.0);
        assert!(c.offset().0 > 450.0 && c.offset().0 < 550.0);
        for _ in 0..600 {
            c.follow(&at(600.0), (1000.0, 1000.0), 1.0 / 60.0);
        }
        assert!((c.offset().0 - 550.0).abs() <= 1.0);
    }

    #[test]
    fn screen_points_map_to_world_without_shake() {
        let mut c = Camera::new(100.0, 100.0);
        c.follow(&at(500.0), (1000.0, 1000.0), 0.0);
        c.shake(5.0, 1.0);
        c.tick(0.1);
        assert_eq!(c.to_world((10.0, 4.0)), (460.0, 4.0));
    }

    #[test]
    fn follow_stays_inside_the_world() {
        let mut c = Camera::new(100.0, 100.0);
        c.follow(&at(5.0), (1000.0, 1000.0), 0.0);
        assert_eq!(c.offset(), (0.0, 0.0));
        c.follow(&at(2000.0), (1000.0, 1000.0), 10.0);
        assert_eq!(c.offset().0, 900.0);
    }

    #[test]
    fn shake_moves_the_view_then_settles() {
        let mut c = Camera::new(100.0, 100.0);
        c.follow(&at(500.0), (1000.0, 1000.0), 0.0);
        c.shake(8.0, 0.2);
        c.tick(0.05);
        assert_ne!(c.offset(), (450.0, 0.0));
        for _ in 0..30 {
            c.tick(0.02);
        }
        assert_eq!(c.offset(), (450.0, 0.0));
    }
}
