//! Streaming Bink soundtrack output for the shell movie player.
//!
//! The decoder runs on the UI thread and pushes interleaved `f32` samples into
//! a lock-free single-producer/single-consumer ring; rodio's audio thread
//! pulls them through [`BinkAudioSource`], filling underruns with silence so
//! the stream never ends on its own.

use std::cell::UnsafeCell;
use std::num::NonZero;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use rodio::Player;
use rodio::Source;
use rodio::mixer::Mixer;

/// The storage a [`RingProducer`] and its [`RingConsumer`] share: a lock-free
/// single-producer single-consumer ring of f32 samples.
///
/// Capacity is rounded up to a power of two so head/tail wrap with masking.
struct SpscRing {
    capacity: usize,
    mask: usize,
    /// `UnsafeCell<f32>` lets the producer write slots while the consumer
    /// reads others. Safety follows from the head/tail discipline.
    buffer: Box<[UnsafeCell<f32>]>,
    /// Producer-incremented; consumer reads with Acquire.
    head: AtomicU64,
    /// Consumer-incremented; producer reads with Acquire.
    tail: AtomicU64,
    underrun: AtomicBool,
}

// SAFETY: the ring is reached only through one `RingProducer` and one
// `RingConsumer` (`ring`), neither of which is `Clone` and whose slot accesses
// take `&mut self`, so at most one thread writes slots and at most one reads
// them. The producer only writes slots outside [tail, head) and publishes
// them with a Release store of `head`; the consumer only reads slots inside
// [tail, head) and releases them with a Release store of `tail`. The two
// never touch the same slot at the same time.
unsafe impl Sync for SpscRing {}

/// A ring of at least `min_capacity` samples, as its one writing end and its
/// one reading end.
fn ring(min_capacity: usize) -> (RingProducer, RingConsumer) {
    let capacity = min_capacity.next_power_of_two().max(2);
    let buffer = (0..capacity)
        .map(|_| UnsafeCell::new(0.0f32))
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let ring = Arc::new(SpscRing {
        capacity,
        mask: capacity - 1,
        buffer,
        head: AtomicU64::new(0),
        tail: AtomicU64::new(0),
        underrun: AtomicBool::new(false),
    });
    (
        RingProducer {
            ring: Arc::clone(&ring),
        },
        RingConsumer { ring },
    )
}

/// The ring's only writing end.
struct RingProducer {
    ring: Arc<SpscRing>,
}

impl RingProducer {
    /// Push as many samples as fit; returns the number pushed.
    fn push(&mut self, samples: &[f32]) -> usize {
        let ring = &*self.ring;
        let head = ring.head.load(Ordering::Relaxed);
        let tail = ring.tail.load(Ordering::Acquire);
        let free = ring.capacity - head.wrapping_sub(tail) as usize;
        let n = samples.len().min(free);
        for (i, sample) in samples.iter().take(n).enumerate() {
            let idx = (head as usize).wrapping_add(i) & ring.mask;
            // SAFETY: this is the only producer (`&mut self`), and the slot is
            // outside [tail, head), which the consumer cannot read until the
            // store of `head` below publishes it.
            unsafe {
                *ring.buffer[idx].get() = *sample;
            }
        }
        ring.head
            .store(head.wrapping_add(n as u64), Ordering::Release);
        n
    }

    /// The samples the consumer has taken so far.
    fn consumed(&self) -> u64 {
        self.ring.tail.load(Ordering::Acquire)
    }

    /// Whether the consumer ran dry since the last call, clearing the flag.
    fn take_underrun(&self) -> bool {
        self.ring.underrun.swap(false, Ordering::AcqRel)
    }

    fn clear_underrun(&self) {
        self.ring.underrun.store(false, Ordering::Release);
    }
}

/// The ring's only reading end.
struct RingConsumer {
    ring: Arc<SpscRing>,
}

impl RingConsumer {
    /// Pop a single sample, returns None if empty.
    fn pop(&mut self) -> Option<f32> {
        let ring = &*self.ring;
        let tail = ring.tail.load(Ordering::Relaxed);
        let head = ring.head.load(Ordering::Acquire);
        if head == tail {
            return None;
        }
        let idx = tail as usize & ring.mask;
        // SAFETY: this is the only consumer (`&mut self`), and the slot is
        // inside [tail, head), published by the producer's Release store of
        // `head`; the producer cannot overwrite it until `tail` advances.
        let v = unsafe { *ring.buffer[idx].get() };
        ring.tail.store(tail.wrapping_add(1), Ordering::Release);
        Some(v)
    }

    fn note_underrun(&self) {
        self.ring.underrun.store(true, Ordering::Release);
    }
}

/// rodio Source pulling samples from the ring. Returns 0.0 (silence)
/// when the buffer is empty so the audio thread never stalls.
struct BinkAudioSource {
    ring: RingConsumer,
    sample_rate: NonZero<u32>,
    channels: NonZero<u16>,
    had_sample: bool,
}

impl BinkAudioSource {
    fn new(ring: RingConsumer, sample_rate: u32, channels: u16) -> Option<Self> {
        Some(Self {
            ring,
            sample_rate: NonZero::new(sample_rate)?,
            channels: NonZero::new(channels)?,
            had_sample: false,
        })
    }
}

impl Iterator for BinkAudioSource {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        // Always Some: silence-fill on underrun. The Source must never end
        // unless the owner drops the player.
        if let Some(sample) = self.ring.pop() {
            self.had_sample = true;
            return Some(sample);
        }
        if self.had_sample {
            self.ring.note_underrun();
            self.had_sample = false;
        }
        Some(0.0)
    }
}

impl Source for BinkAudioSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> NonZero<u16> {
        self.channels
    }
    fn sample_rate(&self) -> NonZero<u32> {
        self.sample_rate
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

/// Ring size per channel. Holds a Bink file's frame-0 audio preload burst
/// (up to about one second) plus headroom for render stalls: ~3 s at 88.2 kHz.
pub const RING_TARGET_SAMPLES_PER_CHANNEL: usize = 262_144;

/// One Bink soundtrack playing on an existing rodio mixer.
pub struct MovieAudioStream {
    player: Player,
    ring: RingProducer,
    sample_rate: u32,
    channels: u16,
}

impl MovieAudioStream {
    pub fn connect(mixer: &Mixer, sample_rate: u32, channels: u16, volume: f32) -> Option<Self> {
        Self::connect_with_activity(mixer, sample_rate, channels, volume, true)
    }

    pub(crate) fn connect_with_activity(
        mixer: &Mixer,
        sample_rate: u32,
        channels: u16,
        volume: f32,
        active: bool,
    ) -> Option<Self> {
        let (ring, consumer) = ring(RING_TARGET_SAMPLES_PER_CHANNEL * channels as usize);
        let source = BinkAudioSource::new(consumer, sample_rate, channels)?;
        // Queue the source before the mixer inspects its format. Connecting
        // an empty Player first lets rodio 0.22.2's UniformSourceIterator
        // cache the empty queue's mono/default rate for the initial span.
        let (player, output) = Player::new();
        if !active {
            player.pause();
        }
        player.set_volume(volume.clamp(0.0, 1.0));
        player.append(source);
        mixer.add(output);
        Some(Self {
            player,
            ring,
            sample_rate,
            channels,
        })
    }

    /// Queue decoded samples. A short push means the audio thread fell behind
    /// and samples were dropped (audible as A/V desync).
    pub fn push(&mut self, samples: &[f32]) -> usize {
        let n = self.ring.push(samples);
        if n < samples.len() {
            log::warn!(
                "movie audio ring overflow: dropped {} / {} samples",
                samples.len() - n,
                samples.len(),
            );
        }
        n
    }

    pub fn pause(&self) {
        self.player.pause();
    }

    pub fn resume(&self) {
        self.ring.clear_underrun();
        self.player.play();
    }

    /// Take an underrun notification and read the current media cursor. The
    /// producer may already have refilled the ring since the consumer went dry.
    pub(crate) fn take_underrun_position(&self) -> Option<Duration> {
        if !self.ring.take_underrun() {
            return None;
        }
        let samples = self.ring.consumed();
        let rate = u64::from(self.channels) * u64::from(self.sample_rate);
        Some(Duration::new(
            samples / rate,
            (u128::from(samples % rate) * 1_000_000_000 / u128::from(rate)) as u32,
        ))
    }

    pub fn position(&self) -> Duration {
        self.player.get_pos()
    }

    pub fn set_volume(&self, volume: f32) {
        self.player.set_volume(volume.clamp(0.0, 1.0));
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }
}

impl Drop for MovieAudioStream {
    fn drop(&mut self) {
        self.player.stop();
    }
}

#[cfg(all(test, tests_audio))]
mod tests {
    use super::*;

    #[test]
    fn retail_radio_peaks_clip_before_overlap_and_voice_volume() {
        use crate::assets::{bink_audio::BinkAudioDecoder, bink_file::BinkFile};
        let Some((_, mut assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        assets.load_nested("MOVIES01.MIX").unwrap();
        let archive = assets.archive("MOVIES01.MIX").unwrap();
        let clip = archive.get_by_name("A02_P01E.BIK").unwrap();
        let file = BinkFile::parse(clip.to_vec().into()).unwrap();
        let mut decoder = BinkAudioDecoder::new(file.header.audio_tracks[0]).unwrap();
        let (mixer, output) = rodio::mixer::mixer(1.try_into().unwrap(), 22050.try_into().unwrap());
        let mut stream = MovieAudioStream::connect(&mixer, 22050, 1, 0.5).unwrap();
        for frame in [9, 10] {
            let packets = file.audio_packets(frame).unwrap();
            let samples = decoder.decode_packet(packets[0].bytes).unwrap();
            assert_eq!(stream.push(&samples), samples.len());
        }
        // BINKW32 BinkGetTrackData (0x1000B1A0) on these packets returned
        // -32768 at frame 9 sample 1576 and -30691 at frame 10 sample 19.
        let heard: Vec<_> = output.take(1940).collect();
        assert_eq!(heard[1576], -0.5);
        assert!((heard[1920 + 19] * 65536.0 + 30691.0).abs() < 2.0);
    }

    #[test]
    fn push_pop_round_trip() {
        let (mut tx, mut rx) = ring(8);
        assert_eq!(tx.push(&[1.0, 2.0, 3.0]), 3);
        assert_eq!(rx.pop(), Some(1.0));
        assert_eq!(rx.pop(), Some(2.0));
        assert_eq!(rx.pop(), Some(3.0));
        assert_eq!(rx.pop(), None);
        assert_eq!(tx.consumed(), 3);
    }

    #[test]
    fn push_full_drops_overflow() {
        let (mut tx, _rx) = ring(4);
        assert_eq!(tx.push(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]), 4);
    }

    #[test]
    fn wrap_around_works() {
        let (mut tx, mut rx) = ring(4);
        for _ in 0..3 {
            tx.push(&[10.0, 20.0]);
            assert_eq!(rx.pop(), Some(10.0));
            assert_eq!(rx.pop(), Some(20.0));
        }
    }
}
