use crate::{Rect, Rng};

const FOLLOW_SPEED: f32 = 10.0;

pub struct Camera {
    pos: (f32, f32),
    view: (f32, f32),
    placed: bool,
    shake_amp: f32,
    shake_left: f32,
    shake_total: f32,
    jitter: (f32, f32),
    rng: Rng,
}

impl Camera {
    pub(crate) fn new(view_w: f32, view_h: f32) -> Self {
        Camera {
            pos: (0.0, 0.0),
            view: (view_w, view_h),
            placed: false,
            shake_amp: 0.0,
            shake_left: 0.0,
            shake_total: 1.0,
            jitter: (0.0, 0.0),
            rng: Rng::new(1),
        }
    }

    pub fn follow(&mut self, target: &Rect, world: (f32, f32), dt: f32) {
        let goal = (
            (target.x + target.w / 2.0 - self.view.0 / 2.0).clamp(0.0, (world.0 - self.view.0).max(0.0)),
            (target.y + target.h / 2.0 - self.view.1 / 2.0).clamp(0.0, (world.1 - self.view.1).max(0.0)),
        );
        if !self.placed {
            (self.pos, self.placed) = (goal, true);
            return;
        }
        let k = 1.0 - (-FOLLOW_SPEED * dt).exp();
        self.pos = (self.pos.0 + (goal.0 - self.pos.0) * k, self.pos.1 + (goal.1 - self.pos.1) * k);
    }

    pub fn cut(&mut self) {
        self.placed = false;
    }

    pub fn shake(&mut self, amplitude: f32, secs: f32) {
        (self.shake_amp, self.shake_left, self.shake_total) = (amplitude, secs, secs.max(f32::EPSILON));
    }

    pub(crate) fn tick(&mut self, dt: f32) {
        if self.shake_left <= 0.0 {
            self.jitter = (0.0, 0.0);
            return;
        }
        self.shake_left = (self.shake_left - dt).max(0.0);
        let strength = self.shake_amp * self.shake_left / self.shake_total;
        self.jitter = (self.rng.range(-strength, strength), self.rng.range(-strength, strength));
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
