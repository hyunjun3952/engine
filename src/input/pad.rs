use crate::base::fx::FxSet;
use std::io::Read;

/// A gamepad button, named as on an Xbox pad; bind it to an action with `Ctx::bind_pad`.
///
/// ```
/// use engine::Pad;
///
/// assert_ne!(Pad::A, Pad::B);
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Pad {
    /// The bottom face button.
    A,
    /// The right face button.
    B,
    /// The left face button.
    X,
    /// The top face button.
    Y,
    /// The left shoulder button.
    LB,
    /// The right shoulder button.
    RB,
    /// The back (select) button.
    Back,
    /// The start button.
    Start,
    /// The d-pad or left stick pushed up.
    Up,
    /// The d-pad or left stick pushed down.
    Down,
    /// The d-pad or left stick pushed left.
    Left,
    /// The d-pad or left stick pushed right.
    Right,
}

/// An analog stick or trigger, read with `Input::pad_axis`.
///
/// ```
/// use engine::PadAxis;
///
/// assert_ne!(PadAxis::LeftX, PadAxis::LeftY);
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PadAxis {
    /// Left stick, -1 (left) to 1 (right).
    LeftX,
    /// Left stick, -1 (up) to 1 (down).
    LeftY,
    /// Right stick, -1 (left) to 1 (right).
    RightX,
    /// Right stick, -1 (up) to 1 (down).
    RightY,
    /// Left trigger, how far it is pressed.
    LeftTrigger,
    /// Right trigger, how far it is pressed.
    RightTrigger,
}

pub(crate) const PADS: &[(&str, Pad)] = &[
    ("A", Pad::A),
    ("B", Pad::B),
    ("X", Pad::X),
    ("Y", Pad::Y),
    ("LB", Pad::LB),
    ("RB", Pad::RB),
    ("Back", Pad::Back),
    ("Start", Pad::Start),
    ("Up", Pad::Up),
    ("Down", Pad::Down),
    ("Left", Pad::Left),
    ("Right", Pad::Right),
];

const STICK_ON: i16 = 16000;
const DEADZONE: f32 = 0.2;

/// First pad through the Linux joystick API (`/dev/input/js0`, Xbox-style layout). Elsewhere, or with no pad
/// plugged in, it simply reports nothing held. The d-pad and left stick both count as Up/Down/Left/Right.
#[derive(Default)]
pub(crate) struct PadState {
    file: Option<std::fs::File>,
    retry: u32,
    buttons: FxSet<Pad>,
    dpad: [i16; 2],
    stick: [i16; 2],
    right: [i16; 2],
    // Raw 0..=65534; the driver reports a released trigger as -32767, which would read as half pressed before any event.
    trigger: [u16; 2],
}

impl PadState {
    pub(crate) fn poll(&mut self) -> FxSet<Pad> {
        if self.file.is_none() {
            self.retry = self.retry.wrapping_add(1);
            if self.retry % 60 == 1 {
                self.file = open();
            }
        }
        if let Some(mut f) = self.file.take() {
            let mut ev = [0u8; 8];
            loop {
                match f.read(&mut ev) {
                    Ok(8) => self.apply(ev),
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        self.file = Some(f);
                        break;
                    }
                    // Includes end of file, which would otherwise spin here forever.
                    Ok(_) | Err(_) => {
                        (self.buttons, self.dpad, self.stick, self.right, self.trigger) = Default::default();
                        break;
                    }
                }
            }
        }
        self.held()
    }

    pub(crate) fn apply(&mut self, ev: [u8; 8]) {
        let value = i16::from_le_bytes([ev[4], ev[5]]);
        let (kind, n) = (ev[6] & 0x7f, ev[7] as usize);
        match (kind, n) {
            (1, 0..8) => {
                if value != 0 {
                    self.buttons.insert(PADS[n].1);
                } else {
                    self.buttons.remove(&PADS[n].1);
                }
            }
            (2, 0 | 1) => self.stick[n] = value,
            (2, 3 | 4) => self.right[n - 3] = value,
            (2, 2 | 5) => self.trigger[n / 5] = (i32::from(value) + 32767).clamp(0, 65534) as u16,
            (2, 6 | 7) => self.dpad[n - 6] = value,
            _ => {}
        }
    }

    /// Sticks run -1..=1 with the dead zone cut out and rescaled; triggers run 0..=1.
    pub(crate) fn axis(&self, axis: PadAxis) -> f32 {
        let stick = |raw: i16| {
            let v = f32::from(raw) / 32767.0;
            v.signum() * ((v.abs() - DEADZONE) / (1.0 - DEADZONE)).clamp(0.0, 1.0)
        };
        let pull = |raw: u16| f32::from(raw) / 65534.0;
        match axis {
            PadAxis::LeftX => stick(self.stick[0]),
            PadAxis::LeftY => stick(self.stick[1]),
            PadAxis::RightX => stick(self.right[0]),
            PadAxis::RightY => stick(self.right[1]),
            PadAxis::LeftTrigger => pull(self.trigger[0]),
            PadAxis::RightTrigger => pull(self.trigger[1]),
        }
    }

    pub(crate) fn held(&self) -> FxSet<Pad> {
        let mut held = self.buttons.clone();
        let axis = |i: usize| self.dpad[i].saturating_add(self.stick[i]);
        let (x, y) = (axis(0), axis(1));
        for (pad, on) in [
            (Pad::Left, x < -STICK_ON),
            (Pad::Right, x > STICK_ON),
            (Pad::Up, y < -STICK_ON),
            (Pad::Down, y > STICK_ON),
        ] {
            if on {
                held.insert(pad);
            }
        }
        held
    }
}

#[cfg(target_os = "linux")]
fn open() -> Option<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    const O_NONBLOCK: i32 = 0o4000;
    std::fs::OpenOptions::new().read(true).custom_flags(O_NONBLOCK).open("/dev/input/js0").ok()
}

#[cfg(not(target_os = "linux"))]
fn open() -> Option<std::fs::File> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(value: i16, kind: u8, n: u8) -> [u8; 8] {
        let v = value.to_le_bytes();
        [0, 0, 0, 0, v[0], v[1], kind, n]
    }

    #[test]
    fn buttons_dpad_and_stick_map_to_pads() {
        let mut p = PadState::default();
        p.apply(ev(1, 1, 0));
        p.apply(ev(1, 1, 7));
        assert_eq!(p.held(), FxSet::from_iter([Pad::A, Pad::Start]));
        p.apply(ev(0, 1, 0));
        p.apply(ev(-32767, 2, 6));
        p.apply(ev(20000, 2, 1));
        assert_eq!(p.held(), FxSet::from_iter([Pad::Start, Pad::Left, Pad::Down]));
        p.apply(ev(0, 2, 6));
        p.apply(ev(5000, 2, 1));
        p.apply(ev(1, 0x81, 3));
        assert_eq!(p.held(), FxSet::from_iter([Pad::Start, Pad::Y]), "init events count, a small stick tilt does not");
    }

    #[test]
    fn unknown_buttons_and_axes_are_ignored() {
        let mut p = PadState::default();
        p.apply(ev(1, 1, 9));
        p.apply(ev(30000, 2, 9));
        assert!(p.held().is_empty());
    }

    #[test]
    fn sticks_cut_the_dead_zone_and_triggers_start_at_zero() {
        let mut p = PadState::default();
        assert_eq!(p.axis(PadAxis::LeftTrigger), 0.0, "no event yet reads as released");
        p.apply(ev(-32767, 2, 2));
        p.apply(ev(32767, 2, 5));
        assert_eq!((p.axis(PadAxis::LeftTrigger), p.axis(PadAxis::RightTrigger)), (0.0, 1.0));
        p.apply(ev(3000, 2, 3));
        p.apply(ev(-32767, 2, 4));
        p.apply(ev(32767, 2, 0));
        assert_eq!((p.axis(PadAxis::RightX), p.axis(PadAxis::RightY), p.axis(PadAxis::LeftX)), (0.0, -1.0, 1.0));
        p.apply(ev(19660, 2, 1));
        assert!((p.axis(PadAxis::LeftY) - 0.5).abs() < 0.01, "0.6 deflection is 0.5 after a 0.2 dead zone");
    }

    #[test]
    fn a_trigger_at_the_lowest_raw_value_reads_released() {
        let mut p = PadState::default();
        p.apply(ev(i16::MIN, 2, 2));
        assert_eq!(p.axis(PadAxis::LeftTrigger), 0.0);
    }
}
