use crate::{Ease, Tween};

#[cfg(feature = "audio")]
use crate::ecs::fx::FxMap;
#[cfg(feature = "audio")]
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, Source, source::ChannelVolume};
#[cfg(feature = "audio")]
use std::{io::Cursor, sync::Arc};

const MAX_SOUNDS: usize = 32;

// `clamp` lets NaN through.
fn unit(v: f32) -> f32 {
    if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) }
}

/// How one sound is played: on top of the master and sound-effect volumes, `volume` (0..=1); `pan` from -1 (left) to
/// 1 (right), which plays it as mono through both speakers at the balance; and `pitch`, a speed multiplier (0.25 to
/// 4, 1 as recorded) that raises or lowers pitch together with speed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sound {
    pub volume: f32,
    pub pan: f32,
    pub pitch: f32,
}

impl Sound {
    pub fn new() -> Self {
        Sound { volume: 1.0, pan: 0.0, pitch: 1.0 }
    }

    pub fn volume(self, volume: f32) -> Self {
        Sound { volume, ..self }
    }

    pub fn pan(self, pan: f32) -> Self {
        Sound { pan, ..self }
    }

    pub fn pitch(self, pitch: f32) -> Self {
        Sound { pitch, ..self }
    }

    // The settings brought into range (not-a-number counts as the neutral value).
    #[cfg_attr(not(feature = "audio"), allow(dead_code))]
    fn clamped(self) -> Sound {
        let or = |v: f32, neutral: f32| if v.is_nan() { neutral } else { v };
        Sound {
            volume: or(self.volume, 1.0).clamp(0.0, 1.0),
            pan: or(self.pan, 0.0).clamp(-1.0, 1.0),
            pitch: or(self.pitch, 1.0).clamp(0.25, 4.0),
        }
    }
}

impl Default for Sound {
    fn default() -> Self {
        Sound::new()
    }
}

// How loud each speaker plays a sound panned to `pan`, keeping the total power the same all the way across.
#[cfg_attr(not(feature = "audio"), allow(dead_code))]
fn pan_gains(pan: f32) -> (f32, f32) {
    let angle = (pan.clamp(-1.0, 1.0) + 1.0) * std::f32::consts::FRAC_PI_4;
    (angle.cos(), angle.sin())
}

// The numbers behind the sound, kept apart from the device so the silent build and the tests share them.
struct Mix {
    volume: f32,
    sfx_volume: f32,
    music_volume: f32,
    max_sounds: usize,
    // 1 normally; a fade moves it, and `true` stops the music when the fade ends.
    fade_gain: f32,
    fade: Option<(Tween, bool)>,
}

impl Mix {
    fn new() -> Self {
        Mix { volume: 1.0, sfx_volume: 1.0, music_volume: 1.0, max_sounds: MAX_SOUNDS, fade_gain: 1.0, fade: None }
    }

    // Only a real player has anything to set it on; the silent build still reads it in tests.
    #[cfg_attr(not(feature = "audio"), allow(dead_code))]
    fn music(&self) -> f32 {
        self.volume * self.music_volume * self.fade_gain
    }

    fn fade_in(&mut self, secs: f32) {
        self.fade_gain = 0.0;
        self.fade = Some((Tween::new(0.0, 1.0, secs.max(0.0), Ease::Linear), false));
    }

    fn fade_out(&mut self, secs: f32) {
        self.fade = Some((Tween::new(self.fade_gain, 0.0, secs.max(0.0), Ease::Linear), true));
    }

    // Advances a fade and says whether the music should stop now.
    fn tick(&mut self, dt: f32) -> bool {
        let Some((tween, stop)) = &mut self.fade else { return false };
        self.fade_gain = tween.tick(dt);
        let (done, stop) = (tween.done(), *stop);
        if done {
            self.fade = None;
            if stop {
                self.fade_gain = 1.0;
            }
        }
        done && stop
    }
}

pub struct Audio {
    mix: Mix,
    #[cfg(feature = "audio")]
    dev: Dev,
}

#[cfg(feature = "audio")]
struct Dev {
    out: Option<MixerDeviceSink>,
    files: FxMap<String, Arc<[u8]>>,
    music: Option<Player>,
    sounds: Vec<Player>,
}

impl Audio {
    pub(crate) fn new() -> Self {
        #[cfg(feature = "audio")]
        if std::env::var_os("ENGINE_MUTE").is_none() {
            match DeviceSinkBuilder::open_default_sink() {
                Ok(mut out) => {
                    out.log_on_drop(false);
                    let mut audio = Audio::disabled();
                    audio.dev.out = Some(out);
                    return audio;
                }
                Err(e) => eprintln!("audio disabled: {e}"),
            }
        }
        Audio::disabled()
    }

    pub(crate) fn disabled() -> Self {
        Audio {
            mix: Mix::new(),
            #[cfg(feature = "audio")]
            dev: Dev { out: None, files: FxMap::default(), music: None, sounds: Vec::new() },
        }
    }

    pub fn volume(&self) -> f32 {
        self.mix.volume
    }

    /// Master volume, 0..=1, for sounds started from now on and for the music.
    pub fn set_volume(&mut self, v: f32) {
        self.mix.volume = unit(v);
        self.retune();
    }

    pub fn sfx_volume(&self) -> f32 {
        self.mix.sfx_volume
    }

    /// The volume of sounds started from now on, 0..=1, on top of the master volume; a sound already playing keeps
    /// the volume it started with.
    pub fn set_sfx_volume(&mut self, v: f32) {
        self.mix.sfx_volume = unit(v);
    }

    pub fn music_volume(&self) -> f32 {
        self.mix.music_volume
    }

    /// The music's own volume, 0..=1, on top of the master volume.
    pub fn set_music_volume(&mut self, v: f32) {
        self.mix.music_volume = unit(v);
        self.retune();
    }

    /// At most this many one-shots play at once (default 32, at least 1); a new one cuts off the oldest.
    pub fn set_max_sounds(&mut self, n: usize) {
        self.mix.max_sounds = n.max(1);
    }

    /// Starts the music from silence and brings it to full over `secs`.
    pub fn music_fade_in(&mut self, path: &str, secs: f32) {
        self.music(path);
        self.mix.fade_in(secs);
        self.retune();
    }

    /// Takes the music down to silence over `secs` and then stops it.
    pub fn music_fade_out(&mut self, secs: f32) {
        self.mix.fade_out(secs);
    }

    // Called once per fixed step by `run`.
    pub(crate) fn tick(&mut self, dt: f32) {
        if self.mix.tick(dt) {
            self.stop_music();
        }
        self.retune();
    }

    #[cfg(feature = "audio")]
    fn open(&mut self, path: &str) -> Cursor<Arc<[u8]>> {
        let bytes = self
            .dev
            .files
            .entry(path.to_string())
            .or_insert_with(|| std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}")).into());
        Cursor::new(bytes.clone())
    }

    #[cfg(feature = "audio")]
    pub fn play(&mut self, path: &str) {
        self.play_with(path, Sound::new());
    }

    /// Like `play` with a volume, pan and pitch of its own.
    #[cfg(feature = "audio")]
    pub fn play_with(&mut self, path: &str, sound: Sound) {
        if self.dev.out.is_none() {
            return;
        }
        let data = self.open(path);
        let Some(out) = &self.dev.out else { return };
        self.dev.sounds.retain(|p| !p.empty());
        if self.dev.sounds.len() >= self.mix.max_sounds {
            self.dev.sounds.remove(0);
        }
        let sound = sound.clamped();
        match Decoder::new(data) {
            Ok(src) => {
                let player = Player::connect_new(out.mixer());
                player.set_volume(self.mix.volume * self.mix.sfx_volume * sound.volume);
                player.set_speed(sound.pitch);
                if sound.pan == 0.0 {
                    player.append(src);
                } else {
                    let (left, right) = pan_gains(sound.pan);
                    player.append(ChannelVolume::new(src, vec![left, right]));
                }
                self.dev.sounds.push(player);
            }
            Err(e) => eprintln!("{path}: {e}"),
        }
    }

    #[cfg(feature = "audio")]
    pub fn music(&mut self, path: &str) {
        if self.dev.out.is_none() {
            return;
        }
        let data = self.open(path);
        let Some(out) = &self.dev.out else { return };
        match Decoder::new(data) {
            Ok(src) => {
                let player = Player::connect_new(out.mixer());
                self.mix.fade = None;
                self.mix.fade_gain = 1.0;
                player.set_volume(self.mix.music());
                player.append(src.repeat_infinite());
                self.dev.music = Some(player);
            }
            Err(e) => eprintln!("{path}: {e}"),
        }
    }

    #[cfg(feature = "audio")]
    pub fn stop_music(&mut self) {
        self.dev.music = None;
    }

    #[cfg(feature = "audio")]
    fn retune(&self) {
        if let Some(m) = &self.dev.music {
            m.set_volume(self.mix.music());
        }
    }

    #[cfg(not(feature = "audio"))]
    pub fn play(&mut self, _path: &str) {}

    #[cfg(not(feature = "audio"))]
    pub fn play_with(&mut self, _path: &str, _sound: Sound) {}

    #[cfg(not(feature = "audio"))]
    pub fn music(&mut self, _path: &str) {
        self.mix.fade = None;
        self.mix.fade_gain = 1.0;
    }

    #[cfg(not(feature = "audio"))]
    pub fn stop_music(&mut self) {}

    #[cfg(not(feature = "audio"))]
    fn retune(&self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volumes_start_full_and_are_kept_within_zero_and_one() {
        let mut a = Audio::disabled();
        assert_eq!((a.volume(), a.sfx_volume(), a.music_volume()), (1.0, 1.0, 1.0));
        a.set_volume(1.7);
        a.set_music_volume(-0.2);
        a.set_sfx_volume(0.3);
        assert_eq!((a.volume(), a.sfx_volume(), a.music_volume()), (1.0, 0.3, 0.0));
        a.set_volume(0.4);
        assert_eq!(a.volume(), 0.4);
    }

    #[test]
    fn a_fade_in_rises_to_full_and_a_fade_out_falls_then_stops_and_resets() {
        let mut m = Mix::new();
        m.volume = 0.5;
        m.fade_in(1.0);
        assert_eq!(m.music(), 0.0);
        assert!(!m.tick(0.5));
        assert!((m.music() - 0.25).abs() < 1e-6, "{}", m.music());
        assert!(!m.tick(0.5));
        assert_eq!((m.music(), m.fade.is_none()), (0.5, true));
        m.fade_out(1.0);
        assert!(!m.tick(0.5));
        assert!((m.music() - 0.25).abs() < 1e-6);
        assert!(m.tick(0.5), "finished: the music stops");
        assert_eq!(m.music(), 0.5, "gain is back to 1 for the next track");
    }

    #[test]
    fn a_fade_out_starts_from_wherever_a_fade_in_had_got() {
        let mut m = Mix::new();
        m.fade_in(1.0);
        m.tick(0.25);
        m.fade_out(1.0);
        m.tick(0.5);
        assert!((m.fade_gain - 0.125).abs() < 1e-6, "{}", m.fade_gain);
    }

    #[test]
    fn at_least_one_sound_is_allowed_at_once() {
        let mut a = Audio::disabled();
        a.set_max_sounds(0);
        assert_eq!(a.mix.max_sounds, 1);
    }

    #[test]
    fn a_volume_that_is_not_a_number_is_silence() {
        let mut a = Audio::disabled();
        a.set_volume(f32::NAN);
        assert_eq!(a.volume(), 0.0);
    }

    #[test]
    fn panning_keeps_the_power_and_a_sound_is_kept_within_range() {
        let power = |pan: f32| {
            let (l, r) = pan_gains(pan);
            l * l + r * r
        };
        for pan in [-1.0, -0.4, 0.0, 0.7, 1.0] {
            assert!((power(pan) - 1.0).abs() < 1e-5, "pan {pan}");
        }
        let (l, r) = pan_gains(-1.0);
        assert!((l - 1.0).abs() < 1e-6 && r.abs() < 1e-6, "all the way left is the left speaker only");
        let (l, r) = pan_gains(0.0);
        assert!((l - r).abs() < 1e-6, "the middle plays both speakers alike");
        let wild = Sound::new().volume(7.0).pan(f32::NAN).pitch(100.0).clamped();
        assert_eq!(wild, Sound { volume: 1.0, pan: 0.0, pitch: 4.0 });
        assert_eq!(
            Sound::new().pan(-3.0).pitch(0.0).volume(-1.0).clamped(),
            Sound { volume: 0.0, pan: -1.0, pitch: 0.25 }
        );
        let mut a = Audio::disabled();
        a.play_with("no/such/file.wav", Sound::new().pan(1.0));
    }
}
