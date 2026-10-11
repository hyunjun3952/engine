use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, Source};
use std::{collections::HashMap, io::Cursor, sync::Arc};

pub struct Audio {
    out: Option<MixerDeviceSink>,
    files: HashMap<String, Arc<[u8]>>,
    music: Option<Player>,
}

impl Audio {
    pub(crate) fn new() -> Self {
        let out = DeviceSinkBuilder::open_default_sink()
            .map(|mut o| {
                o.log_on_drop(false);
                o
            })
            .map_err(|e| eprintln!("audio disabled: {e}"))
            .ok();
        Audio { out, files: HashMap::new(), music: None }
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
            Ok(p) => p.detach(),
            Err(e) => eprintln!("{path}: {e}"),
        }
    }

    pub fn music(&mut self, path: &str) {
        let data = self.open(path);
        let Some(out) = &self.out else { return };
        match Decoder::new(data) {
            Ok(src) => {
                let player = Player::connect_new(out.mixer());
                player.append(src.repeat_infinite());
                self.music = Some(player);
            }
            Err(e) => eprintln!("{path}: {e}"),
        }
    }

    pub fn stop_music(&mut self) {
        self.music = None;
    }
}
