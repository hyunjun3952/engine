use super::Trans;
use crate::Timer;

enum Phase {
    Idle,
    // Darkening towards the change; the scenes stand still until it lands.
    Out(Timer, Box<Trans>, f32),
    Back(Timer),
}

/// A fade to black, the change of scenes at its darkest moment, and a fade back.
pub(crate) struct Fade(Phase);

impl Fade {
    pub(crate) fn new() -> Self {
        Fade(Phase::Idle)
    }

    // `secs` is the whole fade, half going dark and half coming back.
    pub(crate) fn start(&mut self, trans: Trans, secs: f32) {
        let half = secs.max(0.0) / 2.0;
        self.0 = Phase::Out(Timer::once(half), Box::new(trans), half);
    }

    pub(crate) fn blocking(&self) -> bool {
        matches!(self.0, Phase::Out(..))
    }

    /// Advances one step and returns the change to make now, if the screen has just gone fully dark.
    pub(crate) fn tick(&mut self, dt: f32) -> Option<Trans> {
        match &mut self.0 {
            Phase::Idle => None,
            Phase::Out(t, ..) => {
                if !t.tick(dt) {
                    return None;
                }
                let Phase::Out(_, trans, half) = std::mem::replace(&mut self.0, Phase::Idle) else { unreachable!() };
                self.0 = Phase::Back(Timer::once(half));
                Some(*trans)
            }
            Phase::Back(t) => {
                if t.tick(dt) {
                    self.0 = Phase::Idle;
                }
                None
            }
        }
    }

    pub(crate) fn alpha(&self) -> u8 {
        let k = match &self.0 {
            Phase::Idle => 0.0,
            Phase::Out(t, ..) => t.fraction(),
            Phase::Back(t) => 1.0 - t.fraction(),
        };
        (k * 255.0).round() as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fade_of_no_time_still_hands_over_the_change_and_ends() {
        let mut f = Fade::new();
        f.start(Trans::Pop, 0.0);
        assert!(f.blocking());
        assert!(matches!(f.tick(1.0 / 60.0), Some(Trans::Pop)), "the change comes on the first step");
        assert!(f.tick(1.0 / 60.0).is_none());
        assert_eq!((f.alpha(), f.blocking()), (0, false), "and the fade is over a step later");
    }

    #[test]
    fn a_fade_goes_dark_hands_over_the_change_once_then_comes_back() {
        let mut f = Fade::new();
        assert_eq!((f.alpha(), f.blocking()), (0, false));
        f.start(Trans::Pop, 1.0);
        let mut seen = Vec::new();
        let mut handed = 0;
        for _ in 0..12 {
            if let Some(t) = f.tick(0.1) {
                assert!(matches!(t, Trans::Pop));
                handed += 1;
            }
            seen.push((f.alpha(), f.blocking()));
        }
        assert_eq!(handed, 1);
        assert_eq!(seen[3], (204, true), "0.4 s of the 0.5 s going dark");
        assert_eq!(seen[4], (255, false), "dark, the change is handed over and the scenes run again");
        assert_eq!(seen[6], (153, false));
        assert_eq!(seen[9], (0, false));
        assert_eq!(seen[11], (0, false));
    }
}
