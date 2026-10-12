use std::collections::HashSet;
use std::io::Read;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Pad {
    A,
    B,
    X,
    Y,
    LB,
    RB,
    Back,
    Start,
    Up,
    Down,
    Left,
    Right,
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
const BUTTONS: [Pad; 8] = [Pad::A, Pad::B, Pad::X, Pad::Y, Pad::LB, Pad::RB, Pad::Back, Pad::Start];

/// First pad through the Linux joystick API (`/dev/input/js0`, Xbox-style layout). Elsewhere, or with no pad
/// plugged in, it simply reports nothing held. The d-pad and left stick both count as Up/Down/Left/Right.
#[derive(Default)]
pub(crate) struct PadState {
    file: Option<std::fs::File>,
    retry: u32,
    buttons: HashSet<Pad>,
    dpad: [i16; 2],
    stick: [i16; 2],
}

impl PadState {
    pub(crate) fn poll(&mut self) -> HashSet<Pad> {
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
                    Ok(_) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        self.file = Some(f);
                        break;
                    }
                    Err(_) => {
                        (self.buttons, self.dpad, self.stick) = Default::default();
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
                    self.buttons.insert(BUTTONS[n]);
                } else {
                    self.buttons.remove(&BUTTONS[n]);
                }
            }
            (2, 0 | 1) => self.stick[n] = value,
            (2, 6 | 7) => self.dpad[n - 6] = value,
            _ => {}
        }
    }

    pub(crate) fn held(&self) -> HashSet<Pad> {
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
        assert_eq!(p.held(), HashSet::from([Pad::A, Pad::Start]));
        p.apply(ev(0, 1, 0));
        p.apply(ev(-32767, 2, 6));
        p.apply(ev(20000, 2, 1));
        assert_eq!(p.held(), HashSet::from([Pad::Start, Pad::Left, Pad::Down]));
        p.apply(ev(0, 2, 6));
        p.apply(ev(5000, 2, 1));
        p.apply(ev(1, 0x81, 3));
        assert_eq!(p.held(), HashSet::from([Pad::Start, Pad::Y]), "init events count, a small stick tilt does not");
    }

    #[test]
    fn unknown_buttons_and_axes_are_ignored() {
        let mut p = PadState::default();
        p.apply(ev(1, 1, 9));
        p.apply(ev(30000, 2, 2));
        assert!(p.held().is_empty());
    }
}
