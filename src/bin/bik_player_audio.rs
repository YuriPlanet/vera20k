//! Audio output for the bik-player binary: its own default device plus the
//! shared library Bink soundtrack stream (`vera20k::audio::movie_stream`).

use std::ops::Deref;

use rodio::{DeviceSinkBuilder, MixerDeviceSink};
use vera20k::audio::movie_stream::MovieAudioStream;

pub struct BinkAudioSink {
    /// Declared first so the stream's player stops before its device drops.
    stream: MovieAudioStream,
    _device: MixerDeviceSink,
}

impl BinkAudioSink {
    pub fn new(sample_rate: u32, channels: u16) -> Option<Self> {
        let device = DeviceSinkBuilder::open_default_sink()
            .map_err(|e| log::error!("bik-player: failed to open audio device: {}", e))
            .ok()?;
        let stream = MovieAudioStream::connect(device.mixer(), sample_rate, channels, 1.0)?;
        Some(Self {
            stream,
            _device: device,
        })
    }
}

impl Deref for BinkAudioSink {
    type Target = MovieAudioStream;

    fn deref(&self) -> &MovieAudioStream {
        &self.stream
    }
}
