use crate::{Ease, Tween};

#[cfg(feature = "audio")]
use crate::ecs::fx::FxMap;
#[cfg(feature = "audio")]
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, source::ChannelVolume};
#[cfg(feature = "audio")]
use std::{io::Cursor, sync::Arc};

const MAX_SOUNDS: usize = 32;

// `clamp` lets NaN through.
fn unit(v: f32) -> f32 {
    if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) }
}

/// A volume that sounds are sent to, on top of the master volume: `Bus::SFX` for one-shots and `Bus::MUSIC` for the
/// music are built in, and `Audio::bus("voice")` makes others. Changing a bus changes what is already playing, and
/// `Audio::fade_bus` moves it over time (to duck the music under speech, say).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bus(usize);

impl Bus {
    pub const SFX: Bus = Bus(0);
    pub const MUSIC: Bus = Bus(1);
}

/// One sound that `play` or `play_with` started, to `stop` it or ask whether it `is_playing`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Voice(u64);

/// How one sound is played: on the `Bus::SFX` unless `on` says another, with `volume` (0..=1); `pan` from -1 (left) to
/// 1 (right), which plays it as mono through both speakers at the balance; `pitch`, a speed multiplier (0.25 to
/// 4, 1 as recorded) that raises or lowers pitch together with speed; and `looped`, over and over until stopped.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sound {
    pub volume: f32,
    pub pan: f32,
    pub pitch: f32,
    pub bus: Bus,
    pub looped: bool,
}

impl Sound {
    pub fn new() -> Self {
        Sound { volume: 1.0, pan: 0.0, pitch: 1.0, bus: Bus::SFX, looped: false }
    }

    /// Plays it over and over until `Audio::stop` (an engine hum, an alarm).
    pub fn looped(self) -> Self {
        Sound { looped: true, ..self }
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

    /// Sends the sound to `bus` instead of the sound effects.
    pub fn on(self, bus: Bus) -> Self {
        Sound { bus, ..self }
    }

    // The settings brought into range (not-a-number counts as the neutral value).
    #[cfg_attr(not(feature = "audio"), allow(dead_code))]
    fn clamped(self) -> Sound {
        let or = |v: f32, neutral: f32| if v.is_nan() { neutral } else { v };
        Sound {
            volume: or(self.volume, 1.0).clamp(0.0, 1.0),
            pan: or(self.pan, 0.0).clamp(-1.0, 1.0),
            pitch: or(self.pitch, 1.0).clamp(0.25, 4.0),
            ..self
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

// One bus: its name, its volume and the fade that is moving it.
struct Level {
    name: String,
    volume: f32,
    // The move and the volume it ends on, so the end is exact.
    fade: Option<(Tween, f32)>,
}

// The numbers behind the sound, kept apart from the device so the silent build and the tests share them.
struct Mix {
    volume: f32,
    buses: Vec<Level>,
    max_sounds: usize,
    // 1 normally; a fade moves it, and `true` stops the music when the fade ends.
    fade_gain: f32,
    fade: Option<(Tween, bool)>,
    voices: u64,
}

impl Mix {
    fn new() -> Self {
        let level = |name: &str| Level { name: name.to_string(), volume: 1.0, fade: None };
        Mix {
            volume: 1.0,
            buses: vec![level("sfx"), level("music")],
            max_sounds: MAX_SOUNDS,
            fade_gain: 1.0,
            fade: None,
            voices: 0,
        }
    }

    // A fresh handle for every sound asked for, played or not, so the silent build hands them out too.
    fn voice(&mut self) -> Voice {
        self.voices += 1;
        Voice(self.voices)
    }

    fn bus(&mut self, name: &str) -> Bus {
        let at = self.buses.iter().position(|b| b.name == name).unwrap_or_else(|| {
            self.buses.push(Level { name: name.to_string(), volume: 1.0, fade: None });
            self.buses.len() - 1
        });
        Bus(at)
    }

    fn set_bus(&mut self, bus: Bus, v: f32) {
        let level = &mut self.buses[bus.0];
        (level.volume, level.fade) = (unit(v), None);
    }

    fn fade_bus(&mut self, bus: Bus, to: f32, secs: f32) {
        let (to, level) = (unit(to), &mut self.buses[bus.0]);
        level.fade = Some((Tween::new(level.volume, to, secs.max(0.0), Ease::Linear), to));
    }

    // What a sound on `bus` is played at, before its own volume.
    fn level(&self, bus: Bus) -> f32 {
        self.volume * self.buses[bus.0].volume
    }

    // Only a real player has anything to set it on; the silent build still reads it in tests.
    #[cfg_attr(not(feature = "audio"), allow(dead_code))]
    fn music(&self) -> f32 {
        self.level(Bus::MUSIC) * self.fade_gain
    }

    fn fade_in(&mut self, secs: f32) {
        self.fade_gain = 0.0;
        self.fade = Some((Tween::new(0.0, 1.0, secs.max(0.0), Ease::Linear), false));
    }

    fn fade_out(&mut self, secs: f32) {
        self.fade = Some((Tween::new(self.fade_gain, 0.0, secs.max(0.0), Ease::Linear), true));
    }

    // Advances the fades and says whether the music should stop now.
    fn tick(&mut self, dt: f32) -> bool {
        for level in &mut self.buses {
            if let Some((tween, to)) = &mut level.fade {
                level.volume = tween.tick(dt);
                if tween.done() {
                    (level.volume, level.fade) = (*to, None);
                }
            }
        }
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

/// Sound effects and one music track, reached through `ctx.audio()`.
///
/// Volumes are the master times a [`Bus`]: `Bus::SFX`, `Bus::MUSIC` or one made with `bus(name)`. Without the `audio`
/// feature, with `ENGINE_MUTE=1` or with no output device every call does nothing. WAV and OGG Vorbis are read.
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
    sounds: Vec<Playing>,
}

// A one-shot that is playing, with what it needs to be retuned when its bus moves.
#[cfg(feature = "audio")]
struct Playing {
    voice: Voice,
    player: Player,
    bus: Bus,
    gain: f32,
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

    /// Master volume, 0..=1, for everything, what is already playing included.
    pub fn set_volume(&mut self, v: f32) {
        self.mix.volume = unit(v);
        self.retune();
    }

    /// The bus called `name`, made if there is none yet (`"sfx"` and `"music"` are the built-in ones).
    pub fn bus(&mut self, name: &str) -> Bus {
        self.mix.bus(name)
    }

    pub fn bus_volume(&self, bus: Bus) -> f32 {
        self.mix.buses[bus.0].volume
    }

    /// The volume of a bus, 0..=1, on top of the master volume; sounds already playing on it follow.
    pub fn set_bus_volume(&mut self, bus: Bus, v: f32) {
        self.mix.set_bus(bus, v);
        self.retune();
    }

    /// Moves a bus to `to` over `secs`, from where it is (`set_bus_volume` stops the move).
    pub fn fade_bus(&mut self, bus: Bus, to: f32, secs: f32) {
        self.mix.fade_bus(bus, to, secs);
    }

    /// At most this many one-shots play at once (default 32, at least 1); a new one cuts off the oldest.
    pub fn set_max_sounds(&mut self, n: usize) {
        self.mix.max_sounds = n.max(1);
    }

    /// Starts the music from silence and brings it to full over `secs`.
    pub fn fade_in_music(&mut self, path: &str, secs: f32) {
        if self.start_music(path) {
            self.mix.fade_in(secs);
            self.retune();
        }
    }

    /// Takes the music down to silence over `secs` and then stops it.
    pub fn fade_out_music(&mut self, secs: f32) {
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
    fn open(&mut self, path: &str) -> Option<Cursor<Arc<[u8]>>> {
        if !self.dev.files.contains_key(path) {
            let bytes: Arc<[u8]> = std::fs::read(path).inspect_err(|e| eprintln!("{path}: {e}")).ok()?.into();
            self.dev.files.insert(path.to_string(), bytes);
        }
        self.dev.files.get(path).cloned().map(Cursor::new)
    }

    pub fn play(&mut self, path: &str) -> Voice {
        self.play_with(path, Sound::new())
    }

    /// Like `play` with a volume, pan, pitch and looping of its own.
    #[cfg(feature = "audio")]
    pub fn play_with(&mut self, path: &str, sound: Sound) -> Voice {
        let voice = self.mix.voice();
        if self.dev.out.is_none() {
            return voice;
        }
        let Some(data) = self.open(path) else { return voice };
        let Some(out) = &self.dev.out else { return voice };
        self.dev.sounds.retain(|p| !p.player.empty());
        if self.dev.sounds.len() >= self.mix.max_sounds {
            self.dev.sounds.remove(0);
        }
        let sound = sound.clamped();
        let player = Player::connect_new(out.mixer());
        player.set_volume(self.mix.level(sound.bus) * sound.volume);
        player.set_speed(sound.pitch);
        let started = match sound.looped {
            true => Decoder::new_looped(data).map(|src| start(&player, src, sound.pan)),
            false => Decoder::new(data).map(|src| start(&player, src, sound.pan)),
        };
        match started {
            Ok(()) => self.dev.sounds.push(Playing { voice, player, bus: sound.bus, gain: sound.volume }),
            Err(e) => eprintln!("{path}: {e}"),
        }
        voice
    }

    /// Stops a sound `play` started; one that already ended or was cut off is left alone.
    #[cfg(feature = "audio")]
    pub fn stop(&mut self, voice: Voice) {
        self.dev.sounds.retain(|p| p.voice != voice);
    }

    /// Whether a sound `play` started is still playing (never without a sound device).
    #[cfg(feature = "audio")]
    pub fn is_playing(&self, voice: Voice) -> bool {
        self.dev.sounds.iter().any(|p| p.voice == voice && !p.player.empty())
    }

    /// Holds the music where it is until `resume_music`.
    #[cfg(feature = "audio")]
    pub fn pause_music(&mut self) {
        self.dev.music.iter().for_each(Player::pause);
    }

    #[cfg(feature = "audio")]
    pub fn resume_music(&mut self) {
        self.dev.music.iter().for_each(Player::play);
    }

    pub fn music(&mut self, path: &str) {
        self.start_music(path);
    }

    // Whether the track started; when it did not, whatever was playing is left alone.
    #[cfg(feature = "audio")]
    fn start_music(&mut self, path: &str) -> bool {
        if self.dev.out.is_none() {
            return false;
        }
        let Some(data) = self.open(path) else { return false };
        let Some(out) = &self.dev.out else { return false };
        match Decoder::new_looped(data) {
            Ok(src) => {
                let player = Player::connect_new(out.mixer());
                self.mix.fade = None;
                self.mix.fade_gain = 1.0;
                player.set_volume(self.mix.music());
                player.append(src);
                self.dev.music = Some(player);
                true
            }
            Err(e) => {
                eprintln!("{path}: {e}");
                false
            }
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
        for s in &self.dev.sounds {
            s.player.set_volume(self.mix.level(s.bus) * s.gain);
        }
    }

    #[cfg(not(feature = "audio"))]
    pub fn play_with(&mut self, _path: &str, _sound: Sound) -> Voice {
        self.mix.voice()
    }

    #[cfg(not(feature = "audio"))]
    pub fn stop(&mut self, _voice: Voice) {}

    #[cfg(not(feature = "audio"))]
    pub fn is_playing(&self, _voice: Voice) -> bool {
        false
    }

    #[cfg(not(feature = "audio"))]
    pub fn pause_music(&mut self) {}

    #[cfg(not(feature = "audio"))]
    pub fn resume_music(&mut self) {}

    #[cfg(not(feature = "audio"))]
    fn start_music(&mut self, _path: &str) -> bool {
        self.mix.fade = None;
        self.mix.fade_gain = 1.0;
        true
    }

    #[cfg(not(feature = "audio"))]
    pub fn stop_music(&mut self) {}

    #[cfg(not(feature = "audio"))]
    fn retune(&self) {}
}

// Puts a decoded sound on its player, through both speakers at `pan` unless it is centred.
#[cfg(feature = "audio")]
fn start<S: rodio::Source + Send + 'static>(player: &Player, src: S, pan: f32) {
    if pan == 0.0 {
        player.append(src);
    } else {
        let (left, right) = pan_gains(pan);
        player.append(ChannelVolume::new(src, vec![left, right]));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "audio")]
    #[test]
    fn a_fade_in_of_music_that_did_not_start_leaves_the_current_music_alone() {
        let mut a = Audio::disabled();
        a.fade_in_music("assets/sounds/missing.ogg", 1.0);
        assert_eq!((a.mix.fade_gain, a.mix.fade.is_none()), (1.0, true));
    }

    #[test]
    fn volumes_start_full_and_are_kept_within_zero_and_one() {
        let mut a = Audio::disabled();
        assert_eq!((a.volume(), a.bus_volume(Bus::SFX), a.bus_volume(Bus::MUSIC)), (1.0, 1.0, 1.0));
        a.set_volume(1.7);
        a.set_bus_volume(Bus::MUSIC, -0.2);
        a.set_bus_volume(Bus::SFX, 0.3);
        assert_eq!((a.volume(), a.bus_volume(Bus::SFX), a.bus_volume(Bus::MUSIC)), (1.0, 0.3, 0.0));
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
        assert_eq!(wild, Sound { volume: 1.0, pan: 0.0, pitch: 4.0, bus: Bus::SFX, looped: false });
        assert_eq!(
            Sound::new().pan(-3.0).pitch(0.0).volume(-1.0).clamped(),
            Sound { volume: 0.0, pan: -1.0, pitch: 0.25, bus: Bus::SFX, looped: false }
        );
        let mut a = Audio::disabled();
        a.play_with("no/such/file.wav", Sound::new().pan(1.0));
    }

    #[test]
    fn every_sound_gets_its_own_voice_and_one_that_never_played_is_not_playing() {
        let mut a = Audio::disabled();
        let (one, two) = (a.play("no/such/file.wav"), a.play_with("no/such/file.wav", Sound::new().looped()));
        assert_ne!(one, two);
        assert!(!a.is_playing(one) && !a.is_playing(two));
        a.stop(two);
        a.pause_music();
        a.resume_music();
    }

    #[test]
    fn a_bus_is_made_once_by_name_and_the_built_in_ones_are_the_old_volumes() {
        let mut a = Audio::disabled();
        let voice = a.bus("voice");
        assert_eq!(a.bus("voice"), voice, "the same name is the same bus");
        assert_ne!(voice, a.bus("ambience"));
        assert_eq!((a.bus("sfx"), a.bus("music")), (Bus::SFX, Bus::MUSIC));
        a.set_bus_volume(Bus::SFX, 0.25);
        a.set_bus_volume(Bus::MUSIC, 0.5);
        assert_eq!((a.bus_volume(Bus::SFX), a.bus_volume(Bus::MUSIC)), (0.25, 0.5));
        a.set_bus_volume(voice, f32::NAN);
        assert_eq!(a.bus_volume(voice), 0.0);
        a.set_bus_volume(voice, 9.0);
        assert_eq!(a.bus_volume(voice), 1.0);
    }

    #[test]
    fn a_sound_is_played_at_the_master_times_its_bus_and_music_follows_its_bus_too() {
        let mut m = Mix::new();
        let voice = m.bus("voice");
        m.volume = 0.5;
        m.set_bus(voice, 0.4);
        assert!((m.level(voice) - 0.2).abs() < 1e-6);
        m.set_bus(Bus::MUSIC, 0.5);
        assert!((m.music() - 0.25).abs() < 1e-6);
        assert_eq!(Sound::new().on(voice).bus, voice);
        assert_eq!(Sound::new().bus, Bus::SFX);
    }

    #[test]
    fn a_bus_fade_moves_it_over_time_and_a_set_stops_it() {
        let mut m = Mix::new();
        m.fade_bus(Bus::MUSIC, 0.2, 1.0);
        m.tick(0.5);
        assert!((m.buses[1].volume - 0.6).abs() < 1e-6, "{}", m.buses[1].volume);
        m.tick(0.5);
        assert_eq!((m.buses[1].volume, m.buses[1].fade.is_none()), (0.2, true));
        m.fade_bus(Bus::SFX, 0.0, 1.0);
        m.tick(0.25);
        m.set_bus(Bus::SFX, 0.9);
        m.tick(0.5);
        assert_eq!(m.buses[0].volume, 0.9, "setting it cancelled the fade");
        m.fade_bus(Bus::SFX, 0.5, 0.0);
        m.tick(0.0);
        assert_eq!(m.buses[0].volume, 0.5, "a zero-length fade arrives at once");
        m.fade_bus(Bus::SFX, f32::NAN, 1.0);
        m.tick(1.0);
        assert_eq!(m.buses[0].volume, 0.0, "not a number is silence");
    }

    #[test]
    fn a_bus_fade_and_the_music_fade_are_independent() {
        let mut m = Mix::new();
        m.fade_in(1.0);
        m.fade_bus(Bus::MUSIC, 0.5, 1.0);
        m.tick(0.5);
        assert!((m.music() - 0.5 * 0.75).abs() < 1e-6, "{}", m.music());
    }
}
