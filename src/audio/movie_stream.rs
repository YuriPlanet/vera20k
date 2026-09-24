//! Streaming Bink soundtrack output shared by the shell movie player and the
//! standalone `bik-player` tool.
//!
//! The decoder runs on the UI thread and pushes interleaved `f32` samples into
//! a lock-free single-producer/single-consumer ring; rodio's audio thread
//! pulls them through [`BinkAudioSource`], filling underruns with silence so
//! the stream never ends on its own.

use std::cell::UnsafeCell;
use std::num::NonZero;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use rodio::Player;
use rodio::Source;
use rodio::mixer::Mixer;

/// Lock-free single-producer-single-consumer ring buffer of f32 samples.
///
/// Capacity is rounded up to a power of two so head/tail wrap with masking.
pub struct SpscRing {
    capacity: usize,
    mask: usize,
    /// `UnsafeCell<f32>` lets the producer write slots while the consumer
    /// reads others. Safety follows from the head/tail discipline.
    buffer: Box<[UnsafeCell<f32>]>,
    /// Producer-incremented; consumer reads with Acquire.
    head: AtomicUsize,
    /// Consumer-incremented; producer reads with Acquire.
    tail: AtomicUsize,
}

// SAFETY: the producer only writes slots outside [tail, head) and publishes
// them with a Release store of `head`; the consumer only reads slots inside
// [tail, head) and releases them with a Release store of `tail`.
unsafe impl Sync for SpscRing {}
unsafe impl Send for SpscRing {}

impl SpscRing {
    pub fn new(min_capacity: usize) -> Arc<Self> {
        let capacity = min_capacity.next_power_of_two().max(2);
        let buffer = (0..capacity)
            .map(|_| UnsafeCell::new(0.0f32))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Arc::new(Self {
            capacity,
            mask: capacity - 1,
            buffer,
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
        })
    }

    /// Producer-side: push as many samples as fit; returns the number pushed.
    pub fn push(&self, samples: &[f32]) -> usize {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        let free = self.capacity - head.wrapping_sub(tail);
        let n = samples.len().min(free);
        for (i, sample) in samples.iter().take(n).enumerate() {
            let idx = (head + i) & self.mask;
            // SAFETY: the consumer cannot read this slot until `head` is published.
            unsafe {
                *self.buffer[idx].get() = *sample;
            }
        }
        self.head.store(head.wrapping_add(n), Ordering::Release);
        n
    }

    /// Consumer-side: pop a single sample, returns None if empty.
    pub fn pop(&self) -> Option<f32> {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        if head == tail {
            return None;
        }
        let idx = tail & self.mask;
        // SAFETY: the producer cannot overwrite this slot until `tail` advances.
        let v = unsafe { *self.buffer[idx].get() };
        self.tail.store(tail.wrapping_add(1), Ordering::Release);
        Some(v)
    }

    /// Producer-side: drop every queued sample. Call only while the consumer
    /// is paused; otherwise it races the audio thread.
    pub fn drain(&self) {
        let head = self.head.load(Ordering::Acquire);
        self.tail.store(head, Ordering::Release);
    }
}

/// rodio Source pulling samples from an [`SpscRing`]. Returns 0.0 (silence)
/// when the buffer is empty so the audio thread never stalls.
pub struct BinkAudioSource {
    ring: Arc<SpscRing>,
    sample_rate: NonZero<u32>,
    channels: NonZero<u16>,
}

impl BinkAudioSource {
    pub fn new(ring: Arc<SpscRing>, sample_rate: u32, channels: u16) -> Option<Self> {
        Some(Self {
            ring,
            sample_rate: NonZero::new(sample_rate)?,
            channels: NonZero::new(channels)?,
        })
    }
}

impl Iterator for BinkAudioSource {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        // Always Some: silence-fill on underrun. The Source must never end
        // unless the owner drops the player.
        Some(self.ring.pop().unwrap_or(0.0))
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
    ring: Arc<SpscRing>,
    sample_rate: u32,
    channels: u16,
}

impl MovieAudioStream {
    pub fn connect(mixer: &Mixer, sample_rate: u32, channels: u16, volume: f32) -> Option<Self> {
        let ring = SpscRing::new(RING_TARGET_SAMPLES_PER_CHANNEL * channels as usize);
        let source = BinkAudioSource::new(ring.clone(), sample_rate, channels)?;
        let player = Player::connect_new(mixer);
        player.set_volume(volume.clamp(0.0, 1.0));
        player.append(source);
        Some(Self {
            player,
            ring,
            sample_rate,
            channels,
        })
    }

    /// Queue decoded samples. A short push means the audio thread fell behind
    /// and samples were dropped (audible as A/V desync).
    pub fn push(&self, samples: &[f32]) -> usize {
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

    /// Clear queued samples (for a seek). Call only while paused.
    pub fn drain(&self) {
        self.ring.drain();
    }

    pub fn pause(&self) {
        self.player.pause();
    }

    pub fn resume(&self) {
        self.player.play();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_pop_round_trip() {
        let r = SpscRing::new(8);
        assert_eq!(r.push(&[1.0, 2.0, 3.0]), 3);
        assert_eq!(r.pop(), Some(1.0));
        assert_eq!(r.pop(), Some(2.0));
        assert_eq!(r.pop(), Some(3.0));
        assert_eq!(r.pop(), None);
    }

    #[test]
    fn push_full_drops_overflow() {
        let r = SpscRing::new(4);
        assert_eq!(r.push(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]), 4);
    }

    #[test]
    fn wrap_around_works() {
        let r = SpscRing::new(4);
        for _ in 0..3 {
            r.push(&[10.0, 20.0]);
            assert_eq!(r.pop(), Some(10.0));
            assert_eq!(r.pop(), Some(20.0));
        }
    }

    #[test]
    fn drain_empties_buffer() {
        let r = SpscRing::new(8);
        r.push(&[1.0, 2.0, 3.0, 4.0]);
        r.drain();
        assert_eq!(r.pop(), None);
    }
}
