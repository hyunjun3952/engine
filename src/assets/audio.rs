#[cfg(feature = "audio")]
mod real {
    use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, Source};
    use std::{collections::HashMap, io::Cursor, sync::Arc};

    pub struct Audio {
        out: Option<MixerDeviceSink>,
        files: HashMap<String, Arc<[u8]>>,
        music: Option<Player>,
        volume: f32,
        music_volume: f32,
    }

    impl Audio {
        pub(crate) fn new() -> Self {
            if std::env::var_os("ENGINE_MUTE").is_some() {
                return Audio::disabled();
            }
            match DeviceSinkBuilder::open_default_sink() {
                Ok(mut out) => {
                    out.log_on_drop(false);
                    Audio { out: Some(out), ..Audio::disabled() }
                }
                Err(e) => {
                    eprintln!("audio disabled: {e}");
                    Audio::disabled()
                }
            }
        }

        pub(crate) fn disabled() -> Self {
            Audio { out: None, files: HashMap::new(), music: None, volume: 1.0, music_volume: 1.0 }
        }

        fn open(&mut self, path: &str) -> Cursor<Arc<[u8]>> {
            let bytes = self
                .files
                .entry(path.to_string())
                .or_insert_with(|| std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}")).into());
            Cursor::new(bytes.clone())
        }

        pub fn play(&mut self, path: &str) {
            let data = self.open(path);
            let Some(out) = &self.out else { return };
            match rodio::play(out.mixer(), data) {
                Ok(p) => {
                    p.set_volume(self.volume);
                    p.detach();
                }
                Err(e) => eprintln!("{path}: {e}"),
            }
        }

        pub fn music(&mut self, path: &str) {
            let data = self.open(path);
            let Some(out) = &self.out else { return };
            match Decoder::new(data) {
                Ok(src) => {
                    let player = Player::connect_new(out.mixer());
                    player.set_volume(self.volume * self.music_volume);
                    player.append(src.repeat_infinite());
                    self.music = Some(player);
                }
                Err(e) => eprintln!("{path}: {e}"),
            }
        }

        pub fn stop_music(&mut self) {
            self.music = None;
        }

        pub fn volume(&self) -> f32 {
            self.volume
        }

        /// Master volume, 0..=1, for sounds started from now on and for the music.
        pub fn set_volume(&mut self, v: f32) {
            self.volume = v.clamp(0.0, 1.0);
            self.retune_music();
        }

        pub fn music_volume(&self) -> f32 {
            self.music_volume
        }

        /// The music's own volume, 0..=1, on top of the master volume.
        pub fn set_music_volume(&mut self, v: f32) {
            self.music_volume = v.clamp(0.0, 1.0);
            self.retune_music();
        }

        fn retune_music(&self) {
            if let Some(m) = &self.music {
                m.set_volume(self.volume * self.music_volume);
            }
        }
    }
}

#[cfg(feature = "audio")]
pub use real::Audio;

#[cfg(not(feature = "audio"))]
pub struct Audio {
    volume: f32,
    music_volume: f32,
}

#[cfg(not(feature = "audio"))]
impl Audio {
    pub(crate) fn new() -> Self {
        Audio::disabled()
    }

    pub(crate) fn disabled() -> Self {
        Audio { volume: 1.0, music_volume: 1.0 }
    }

    pub fn play(&mut self, _path: &str) {}

    pub fn music(&mut self, _path: &str) {}

    pub fn stop_music(&mut self) {}
    pub fn volume(&self) -> f32 {
        self.volume
    }
    pub fn set_volume(&mut self, v: f32) {
        self.volume = v.clamp(0.0, 1.0);
    }
    pub fn music_volume(&self) -> f32 {
        self.music_volume
    }
    pub fn set_music_volume(&mut self, v: f32) {
        self.music_volume = v.clamp(0.0, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volumes_start_full_and_are_kept_within_zero_and_one() {
        let mut a = Audio::disabled();
        assert_eq!((a.volume(), a.music_volume()), (1.0, 1.0));
        a.set_volume(1.7);
        a.set_music_volume(-0.2);
        assert_eq!((a.volume(), a.music_volume()), (1.0, 0.0));
        a.set_volume(0.4);
        assert_eq!(a.volume(), 0.4);
    }
}
