use super::input::{Input, KEYS, Key};
use super::pad::{PADS, Pad};

/// What the player was holding for one fixed step. Analog axes, the wheel and typed characters are not recorded.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Frame {
    pub keys: Vec<Key>,
    pub pads: Vec<Pad>,
    pub mouse: Option<(f32, f32)>,
    pub buttons: [bool; 5],
}

/// A seed and one `Frame` per fixed step: enough to run a deterministic game again.
#[derive(Debug, PartialEq)]
pub(crate) struct Recording {
    pub seed: u32,
    pub frames: Vec<Frame>,
}

fn names<T: Copy + PartialEq>(items: &[T], table: &[(&str, T)]) -> String {
    let mut words: Vec<&str> = items.iter().filter_map(|i| table.iter().find(|t| t.1 == *i).map(|t| t.0)).collect();
    words.sort();
    if words.is_empty() { "-".into() } else { words.join(",") }
}

fn items<T: Copy>(word: &str, table: &[(&str, T)], what: &str) -> Result<Vec<T>, String> {
    if word == "-" {
        return Ok(Vec::new());
    }
    word.split(',')
        .map(|w| table.iter().find(|t| t.0 == w).map(|t| t.1).ok_or(format!("unknown {what} {w:?}")))
        .collect()
}

impl Recording {
    /// `seed N`, then a line per step: keys, pad buttons, mouse x,y (or `-`) and five 0/1 mouse buttons.
    pub(crate) fn to_text(&self) -> String {
        let mut out = format!("seed {}\n", self.seed);
        for f in &self.frames {
            let mouse = f.mouse.map_or("-".to_string(), |(x, y)| format!("{x},{y}"));
            let buttons: String = f.buttons.iter().map(|&b| if b { '1' } else { '0' }).collect();
            out += &format!("{} {} {mouse} {buttons}\n", names(&f.keys, KEYS), names(&f.pads, PADS));
        }
        out
    }

    pub(crate) fn parse(text: &str) -> Result<Recording, String> {
        let mut lines = text.lines();
        let seed = lines
            .next()
            .and_then(|l| l.strip_prefix("seed "))
            .and_then(|n| n.trim().parse().ok())
            .ok_or("the first line must be `seed N`")?;
        let mut frames = Vec::new();
        for (i, line) in lines.enumerate() {
            let bad = |why: String| format!("line {}: {why}", i + 2);
            let words: Vec<&str> = line.split_whitespace().collect();
            let [keys, pads, mouse, buttons] = words[..] else { return Err(bad("expected four words".into())) };
            let mouse = match mouse {
                "-" => None,
                m => {
                    let (x, y) = m.split_once(',').ok_or_else(|| bad(format!("bad mouse {m:?}")))?;
                    Some((
                        x.parse().map_err(|_| bad(format!("bad mouse {m:?}")))?,
                        y.parse().map_err(|_| bad(format!("bad mouse {m:?}")))?,
                    ))
                }
            };
            if buttons.len() != 5 || !buttons.chars().all(|c| c == '0' || c == '1') {
                return Err(bad(format!("bad mouse buttons {buttons:?}")));
            }
            let mut down = [false; 5];
            buttons.chars().enumerate().for_each(|(n, c)| down[n] = c == '1');
            frames.push(Frame {
                keys: items(keys, KEYS, "key").map_err(bad)?,
                pads: items(pads, PADS, "pad button").map_err(bad)?,
                mouse,
                buttons: down,
            });
        }
        Ok(Recording { seed, frames })
    }
}

/// Records the steps of a run to a file (`ENGINE_RECORD=path`) or feeds a recorded run back in (`ENGINE_REPLAY=path`).
pub(crate) enum Tape {
    Off,
    Record(String, Recording),
    Play(Recording, usize),
}

impl Tape {
    pub(crate) fn from_env(seed: u32) -> Result<Tape, String> {
        if let Some(path) = std::env::var_os("ENGINE_REPLAY") {
            let path = path.to_string_lossy().into_owned();
            let text = crate::assets::read_text(&path)?;
            return Ok(Tape::Play(Recording::parse(&text).map_err(|e| format!("{path}: {e}"))?, 0));
        }
        Ok(match std::env::var_os("ENGINE_RECORD") {
            Some(path) => Tape::Record(path.to_string_lossy().into_owned(), Recording { seed, frames: Vec::new() }),
            None => Tape::Off,
        })
    }

    /// The seed the run must use, when a replay dictates it.
    pub(crate) fn seed(&self) -> Option<u32> {
        match self {
            Tape::Play(r, _) => Some(r.seed),
            _ => None,
        }
    }

    /// Called before each fixed step: records what is held, or replaces it with the recorded step. False when a
    /// replay has run out.
    pub(crate) fn step(&mut self, input: &mut Input) -> bool {
        match self {
            Tape::Off => true,
            Tape::Record(_, r) => {
                r.frames.push(input.snapshot());
                true
            }
            Tape::Play(r, at) => {
                let Some(f) = r.frames.get(*at) else { return false };
                input.restore(f);
                *at += 1;
                true
            }
        }
    }

    // Runs on drop, so a run that panics still leaves its recording.
    fn finish(&self) {
        if let Tape::Record(path, r) = self {
            match crate::assets::write_atomic(path, &r.to_text()) {
                Ok(()) => eprintln!("recorded {} steps to {path}", r.frames.len()),
                Err(e) => eprintln!("{path}: {e}"),
            }
        }
    }
}

impl Drop for Tape {
    fn drop(&mut self) {
        self.finish();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Vec2;

    fn frame(keys: &[Key], mouse: Option<(f32, f32)>) -> Frame {
        Frame { keys: keys.to_vec(), pads: vec![Pad::A], mouse, buttons: [true, false, false, false, true] }
    }

    #[test]
    fn a_recording_survives_a_round_trip_through_text() {
        let rec = Recording {
            seed: 77,
            frames: vec![
                frame(&[Key::Space, Key::Left], Some((12.5, 30.0))),
                Frame { keys: vec![], pads: vec![], mouse: None, buttons: [false; 5] },
            ],
        };
        let text = rec.to_text();
        assert_eq!(text, "seed 77\nLeft,Space A 12.5,30 10001\n- - - 00000\n");
        assert_eq!(Recording::parse(&text).unwrap().frames.len(), 2);
        let back = Recording::parse(&text).unwrap();
        assert_eq!(back.frames[0].mouse, Some((12.5, 30.0)));
        assert_eq!((back.seed, back.frames[1].keys.len()), (77, 0));
    }

    #[test]
    fn a_bad_recording_names_the_line() {
        for (text, want) in [
            ("nope\n", "seed"),
            ("seed 1\nSpace A\n", "line 2: expected four"),
            ("seed 1\n- - - 00000\nWarp - - 00000\n", "line 3: unknown key"),
            ("seed 1\n- Z - 00000\n", "unknown pad button"),
            ("seed 1\n- - 1;2 00000\n", "bad mouse"),
            ("seed 1\n- - - 0001\n", "bad mouse buttons"),
        ] {
            let e = Recording::parse(text).unwrap_err();
            assert!(e.contains(want), "{text:?}: {e}");
        }
    }

    #[test]
    fn a_tape_records_each_step_and_plays_it_back_until_it_runs_out() {
        let mut live = Input::default();
        let sink = std::env::temp_dir().join(format!("engine-replay-sink-{}.rec", std::process::id()));
        let mut tape = Tape::Record(sink.to_string_lossy().into_owned(), Recording { seed: 9, frames: vec![] });
        live.hold(&[Key::Space]);
        live.point(Some((3.0, 4.0)), true);
        assert!(tape.step(&mut live));
        live.hold(&[]);
        assert!(tape.step(&mut live));
        let Tape::Record(_, recorded) = &mut tape else { panic!() };
        let rec = std::mem::replace(recorded, Recording { seed: 0, frames: vec![] });
        drop(tape);
        std::fs::remove_file(&sink).ok();
        let mut play = Tape::Play(rec, 0);
        assert_eq!(play.seed(), Some(9));
        let mut fresh = Input::default();
        assert!(play.step(&mut fresh));
        assert!(
            fresh.key_down(Key::Space)
                && fresh.mouse() == Some(Vec2::new(3.0, 4.0))
                && fresh.mouse_down(super::super::input::MouseButton::Left)
        );
        assert!(play.step(&mut fresh));
        assert!(!fresh.key_down(Key::Space));
        assert!(!play.step(&mut fresh), "no steps left");
        assert!(Tape::Off.step(&mut fresh) && Tape::Off.seed().is_none());
    }

    #[test]
    fn finishing_a_recording_writes_a_file_that_reads_back() {
        let path = std::env::temp_dir().join(format!("engine-replay-test-{}.rec", std::process::id()));
        let rec = Recording { seed: 3, frames: vec![frame(&[Key::A], None)] };
        let want = rec.to_text();
        drop(Tape::Record(path.to_string_lossy().into_owned(), rec));
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(text, want);
        assert_eq!(Recording::parse(&text).unwrap().seed, 3);
    }
}
