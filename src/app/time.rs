/// Counts the fixed `dt` given to `tick`: `once(secs)` runs out, `looping(secs)` restarts.
#[derive(Clone, Copy, Debug)]
pub struct Timer {
    duration: f32,
    elapsed: f32,
    looping: bool,
    // A once-timer says it finished on one tick only; a zero-length one has finished before any tick, so this, not
    // the elapsed time, decides when it says so.
    reported: bool,
}

impl Timer {
    pub fn once(secs: f32) -> Self {
        assert!(secs >= 0.0, "timer duration must be a number, not negative: {secs}");
        Timer { duration: secs, elapsed: 0.0, looping: false, reported: false }
    }

    pub fn looping(secs: f32) -> Self {
        assert!(secs > 0.0, "a looping timer needs a positive duration");
        Timer { duration: secs, elapsed: 0.0, looping: true, reported: false }
    }

    pub fn tick(&mut self, dt: f32) -> bool {
        self.elapsed += dt;
        if self.looping && self.elapsed >= self.duration {
            self.elapsed %= self.duration;
            return true;
        }
        let first = !self.looping && !self.reported && self.done();
        self.reported |= first;
        first
    }

    pub fn done(&self) -> bool {
        !self.looping && self.elapsed >= self.duration
    }

    pub fn fraction(&self) -> f32 {
        if self.duration <= 0.0 { 1.0 } else { (self.elapsed / self.duration).min(1.0) }
    }

    pub fn remaining(&self) -> f32 {
        (self.duration - self.elapsed).max(0.0)
    }

    pub fn reset(&mut self) {
        self.elapsed = 0.0;
        self.reported = false;
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Ease {
    Linear,
    InQuad,
    OutQuad,
    InOutQuad,
    OutCubic,
    OutBack,
}

impl Ease {
    pub fn apply(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Ease::Linear => t,
            Ease::InQuad => t * t,
            Ease::OutQuad => 1.0 - (1.0 - t) * (1.0 - t),
            Ease::InOutQuad => {
                if t < 0.5 {
                    2.0 * t * t
                } else {
                    1.0 - (-2.0 * t + 2.0).powi(2) / 2.0
                }
            }
            Ease::OutCubic => 1.0 - (1.0 - t).powi(3),
            Ease::OutBack => {
                let (c1, c3) = (1.70158, 2.70158);
                1.0 + c3 * (t - 1.0).powi(3) + c1 * (t - 1.0).powi(2)
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Tween {
    from: f32,
    to: f32,
    ease: Ease,
    timer: Timer,
}

impl Tween {
    pub fn new(from: f32, to: f32, secs: f32, ease: Ease) -> Self {
        Tween { from, to, ease, timer: Timer::once(secs) }
    }

    pub fn tick(&mut self, dt: f32) -> f32 {
        self.timer.tick(dt);
        self.value()
    }

    pub fn value(&self) -> f32 {
        self.from + (self.to - self.from) * self.ease.apply(self.timer.fraction())
    }

    pub fn done(&self) -> bool {
        self.timer.done()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_once_timer_reports_the_tick_that_finishes_it_exactly_once() {
        let mut t = Timer::once(0.25);
        let finished: Vec<bool> = (0..6).map(|_| t.tick(0.1)).collect();
        assert_eq!(finished, [false, false, true, false, false, false]);
        assert!(t.done());
        assert_eq!((t.fraction(), t.remaining()), (1.0, 0.0));
        t.reset();
        assert!(!t.done() && t.fraction() == 0.0);
    }

    #[test]
    fn a_looping_timer_wraps_and_never_finishes() {
        let mut t = Timer::looping(0.3);
        let fired: Vec<bool> = (0..7).map(|_| t.tick(0.1)).collect();
        assert_eq!(fired.iter().filter(|&&f| f).count(), 2);
        assert!(!t.done() && t.fraction() < 1.0);
    }

    #[test]
    fn a_zero_length_timer_is_done_immediately_and_reports_it_on_its_first_tick_only() {
        let mut t = Timer::once(0.0);
        assert!(t.done() && t.fraction() == 1.0);
        assert_eq!([t.tick(0.1), t.tick(0.1), t.tick(0.1)], [true, false, false]);
        t.reset();
        assert!(t.tick(0.0), "a reset timer reports again, even with no time passing");
    }

    #[test]
    fn every_easing_starts_at_zero_and_ends_at_one_and_out_back_overshoots() {
        let all = [Ease::Linear, Ease::InQuad, Ease::OutQuad, Ease::InOutQuad, Ease::OutCubic, Ease::OutBack];
        for e in all {
            assert!((e.apply(0.0)).abs() < 1e-6 && (e.apply(1.0) - 1.0).abs() < 1e-6, "{e:?}");
        }
        assert!(Ease::OutQuad.apply(0.5) > 0.5 && Ease::InQuad.apply(0.5) < 0.5);
        assert!(Ease::OutBack.apply(0.7) > 1.0);
    }

    #[test]
    fn a_tween_interpolates_and_lands_exactly_on_its_target() {
        let mut t = Tween::new(10.0, 20.0, 1.0, Ease::Linear);
        assert_eq!(t.tick(0.5), 15.0);
        assert!(!t.done());
        assert_eq!(t.tick(5.0), 20.0);
        assert!(t.done());
    }
}
