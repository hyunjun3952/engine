// Builds syllables out of compatibility jamo as they arrive, the way a two-set (dubeolsik) keyboard does.

const CHO: &str = "ㄱㄲㄴㄷㄸㄹㅁㅂㅃㅅㅆㅇㅈㅉㅊㅋㅌㅍㅎ";
const JUNG: &str = "ㅏㅐㅑㅒㅓㅔㅕㅖㅗㅘㅙㅚㅛㅜㅝㅞㅟㅠㅡㅢㅣ";
// Index 0 is "no final consonant".
const JONG: &str = " ㄱㄲㄳㄴㄵㄶㄷㄹㄺㄻㄼㄽㄾㄿㅀㅁㅂㅄㅅㅆㅇㅈㅊㅋㅌㅍㅎ";
const VOWELS: [(char, char, char); 7] = [
    ('ㅗ', 'ㅏ', 'ㅘ'),
    ('ㅗ', 'ㅐ', 'ㅙ'),
    ('ㅗ', 'ㅣ', 'ㅚ'),
    ('ㅜ', 'ㅓ', 'ㅝ'),
    ('ㅜ', 'ㅔ', 'ㅞ'),
    ('ㅜ', 'ㅣ', 'ㅟ'),
    ('ㅡ', 'ㅣ', 'ㅢ'),
];
const FINALS: [(char, char, char); 11] = [
    ('ㄱ', 'ㅅ', 'ㄳ'),
    ('ㄴ', 'ㅈ', 'ㄵ'),
    ('ㄴ', 'ㅎ', 'ㄶ'),
    ('ㄹ', 'ㄱ', 'ㄺ'),
    ('ㄹ', 'ㅁ', 'ㄻ'),
    ('ㄹ', 'ㅂ', 'ㄼ'),
    ('ㄹ', 'ㅅ', 'ㄽ'),
    ('ㄹ', 'ㅌ', 'ㄾ'),
    ('ㄹ', 'ㅍ', 'ㄿ'),
    ('ㄹ', 'ㅎ', 'ㅀ'),
    ('ㅂ', 'ㅅ', 'ㅄ'),
];
const KEYS: &str = "qQwWeErRtTyuioOpPasdfghjklzxcvbnm";
const JAMO: &str = "ㅂㅃㅈㅉㄷㄸㄱㄲㅅㅆㅛㅕㅑㅐㅒㅔㅖㅁㄴㅇㄹㅎㅗㅓㅏㅣㅋㅌㅊㅍㅠㅜㅡ";

/// The jamo a letter key makes on a two-set keyboard (shift gives the doubled ones); other characters pass through.
pub(crate) fn latin_to_jamo(c: char) -> char {
    KEYS.chars()
        .position(|k| k == c)
        .or_else(|| KEYS.chars().position(|k| k == c.to_ascii_lowercase()))
        .map_or(c, |i| JAMO.chars().nth(i).unwrap())
}

fn at(set: &str, c: char) -> Option<usize> {
    set.chars().position(|x| x == c)
}

fn join(table: &[(char, char, char)], a: char, b: char) -> Option<char> {
    table.iter().find(|t| t.0 == a && t.1 == b).map(|t| t.2)
}

fn split(table: &[(char, char, char)], c: char) -> Option<(char, char)> {
    table.iter().find(|t| t.2 == c).map(|t| (t.0, t.1))
}

/// The syllable being typed: an initial consonant, a vowel and a final consonant, each possibly missing.
#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub(crate) struct Hangul {
    cho: Option<char>,
    jung: Option<char>,
    jong: Option<char>,
}

enum Step {
    Edit(Hangul),
    Split(Hangul, Hangul),
    Plain,
}

impl Hangul {
    fn glyph(&self) -> Option<char> {
        match *self {
            Hangul { cho: None, jung: None, .. } => None,
            Hangul { cho: Some(c), jung: None, .. } => Some(c),
            Hangul { cho: None, jung: Some(v), .. } => Some(v),
            Hangul { cho: Some(c), jung: Some(v), jong } => {
                let (c, v, j) = (at(CHO, c)?, at(JUNG, v)?, jong.map_or(Some(0), |j| at(JONG, j))?);
                char::from_u32(0xAC00 + ((c * 21 + v) * 28 + j) as u32)
            }
        }
    }

    fn step(&self, c: char) -> Step {
        let (cons, vowel) = (at(CHO, c).is_some() || at(JONG, c).is_some_and(|i| i > 0), at(JUNG, c).is_some());
        let start = |s: Hangul| Step::Split(*self, s);
        let (cho, jung, jong) = (self.cho, self.jung, self.jong);
        match (cons, vowel) {
            (true, _) if at(CHO, c).is_some() || jung.is_some() => match (cho, jung, jong) {
                (Some(_), Some(_), None) if at(JONG, c).is_some_and(|i| i > 0) => {
                    Step::Edit(Hangul { jong: Some(c), ..*self })
                }
                (Some(_), Some(_), Some(j)) => match join(&FINALS, j, c) {
                    Some(both) => Step::Edit(Hangul { jong: Some(both), ..*self }),
                    None => start(Hangul { cho: Some(c), ..Default::default() }),
                },
                _ if at(CHO, c).is_some() => start(Hangul { cho: Some(c), ..Default::default() }),
                _ => Step::Plain,
            },
            (_, true) => match (cho, jung, jong) {
                (_, None, _) => Step::Edit(Hangul { jung: Some(c), ..*self }),
                (_, Some(v), None) => match join(&VOWELS, v, c) {
                    Some(both) => Step::Edit(Hangul { jung: Some(both), ..*self }),
                    None => start(Hangul { jung: Some(c), ..Default::default() }),
                },
                (_, Some(_), Some(j)) => match split(&FINALS, j) {
                    Some((keep, carry)) => Step::Split(
                        Hangul { jong: Some(keep), ..*self },
                        Hangul { cho: Some(carry), jung: Some(c), jong: None },
                    ),
                    None => {
                        Step::Split(Hangul { jong: None, ..*self }, Hangul { cho: Some(j), jung: Some(c), jong: None })
                    }
                },
            },
            _ => Step::Plain,
        }
    }

    // The composing syllable is always the last character of `text`.
    fn put(&mut self, text: &mut String, next: Hangul, tail: Option<Hangul>) {
        if self.glyph().is_some() {
            text.pop();
        }
        text.extend(next.glyph());
        *self = next;
        if let Some(t) = tail {
            text.extend(t.glyph());
            *self = t;
        }
    }

    pub(crate) fn push(&mut self, text: &mut String, c: char) {
        match self.step(c) {
            Step::Edit(next) => self.put(text, next, None),
            Step::Split(first, second) => self.put(text, first, Some(second)),
            Step::Plain => {
                *self = Hangul::default();
                text.push(c);
            }
        }
    }

    /// Removes the last jamo of the syllable being typed, or the last character when none is.
    pub(crate) fn backspace(&mut self, text: &mut String) {
        let Hangul { cho, jung, jong } = *self;
        let next = match (cho, jung, jong) {
            (_, Some(_), Some(j)) => Hangul { jong: split(&FINALS, j).map(|s| s.0), ..*self },
            (_, Some(v), None) => Hangul { jung: split(&VOWELS, v).map(|s| s.0), ..*self },
            _ => Hangul::default(),
        };
        if self.glyph().is_some() {
            self.put(text, next, None);
        } else {
            text.pop();
        }
    }

    /// Ends the syllable being typed; the next jamo starts a new one.
    pub(crate) fn end(&mut self) {
        *self = Hangul::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(s: &str) -> (String, Hangul) {
        let (mut text, mut h) = (String::new(), Hangul::default());
        s.chars().for_each(|c| h.push(&mut text, latin_to_jamo(c)));
        (text, h)
    }

    #[test]
    fn two_set_keys_make_syllables_with_finals_and_vowel_pairs() {
        for (keys, want) in [
            ("gksrmf", "한글"),
            ("rkskek", "가나다"),
            ("ghk", "화"),
            ("dnjs", "원"),
            ("Rk", "까"),
            ("rkqtk", "갑사"),
            ("ekfrk", "달가"),
            ("ekfr", "닭"),
        ] {
            assert_eq!(typed(keys).0, want, "{keys}");
        }
    }

    #[test]
    fn jamo_from_a_korean_layout_compose_the_same_way() {
        let (mut text, mut h) = (String::new(), Hangul::default());
        "ㅎㅏㄴㄱㅡㄹ".chars().for_each(|c| h.push(&mut text, c));
        assert_eq!(text, "한글");
    }

    #[test]
    fn a_final_moves_to_the_next_syllable_when_a_vowel_follows_and_a_pair_splits() {
        assert_eq!(typed("rkq").0, "갑");
        assert_eq!(typed("rkqk").0, "가바");
        assert_eq!(typed("rkqt").0, "값");
        assert_eq!(typed("rkqtk").0, "갑사");
    }

    #[test]
    fn lone_jamo_stay_lone_and_other_characters_end_the_syllable() {
        assert_eq!(typed("rr").0, "ㄱㄱ");
        assert_eq!(typed("kk").0, "ㅏㅏ");
        assert_eq!(typed("hk").0, "ㅘ");
        let (text, h) = typed("gk 1");
        assert_eq!((text.as_str(), h), ("하 1", Hangul::default()));
    }

    #[test]
    fn backspace_takes_back_one_jamo_at_a_time_then_whole_characters() {
        let (mut text, mut h) = typed("ekfr");
        for want in ["달", "다", "ㄷ", ""] {
            h.backspace(&mut text);
            assert_eq!(text, want);
        }
        let (mut text, mut h) = typed("ghk");
        h.backspace(&mut text);
        assert_eq!(text, "호");
        let mut text = String::from("가");
        Hangul::default().backspace(&mut text);
        assert_eq!(text, "");
    }

    #[test]
    fn any_stream_of_jamo_letters_backspaces_and_other_characters_composes_without_panicking() {
        let pool: Vec<char> =
            "ㄱㄲㄳㄴㄵㄶㄷㄸㄹㄺㄻㄼㄽㄾㄿㅀㅁㅂㅃㅄㅅㅆㅇㅈㅉㅊㅋㅌㅍㅎㅏㅐㅑㅒㅓㅔㅕㅖㅗㅘㅙㅚㅛㅜㅝㅞㅟㅠㅡㅢㅣ가힣abcXYZ 1\u{3130}\u{3164}"
                .chars()
                .collect();
        let mut rng = crate::Rng::new(2);
        for _ in 0..300 {
            let (mut text, mut h) = (String::new(), Hangul::default());
            for _ in 0..200 {
                match rng.f32() {
                    r if r < 0.2 => h.backspace(&mut text),
                    r if r < 0.25 => h.end(),
                    _ => h.push(&mut text, pool[(rng.f32() * pool.len() as f32) as usize % pool.len()]),
                }
            }
            assert!(text.chars().count() <= 200, "a stream of 200 keys cannot make more than 200 characters");
        }
    }
}
