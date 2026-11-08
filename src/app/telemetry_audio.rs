//! Live audio for the dashboard.
//!
//! The shared mixer copies every frame it plays into a lock-free ring (see
//! [`crate::audio::tap_output`]). This pump drains that ring on its own
//! thread and publishes fixed 40 ms blocks, so audio never depends on the
//! render loop and never waits behind a multi-megabyte picture frame.
//!
//! The block format is deliberately trivial — no codec, no container:
//!
//! ```text
//! "AMAG" | sample_rate u32 | channels u16 | frames u16 | sequence u32 | PCM16LE
//! ```
//!
//! A dashboard that falls behind loses blocks rather than slides out of sync:
//! the pump skips ahead to the newest audio whenever the backlog grows past a
//! few blocks, which keeps the stream live instead of late.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::app::telemetry_server::AudioSink;
use crate::audio::tap_output::PcmRing;

const MAGIC: &[u8; 4] = b"AMAG";
const HEADER_BYTES: usize = 16;
const CHANNELS: u16 = 2;
/// One block of audio. 40 ms is long enough that a 25 Hz cadence is cheap and
/// short enough that a lost block is a blip, not a gap.
const BLOCK_MS: u32 = 40;
/// How often the pump checks for work. Well under the block period, so a
/// block is published as soon as the mixer has produced it.
const WAKE: Duration = Duration::from_millis(10);
/// Blocks the pump lets accumulate before skipping to the newest audio.
/// Four blocks is 160 ms: enough to ride out a stalled socket write, little
/// enough that the listener never hears sound from the past.
const MAX_BACKLOG_BLOCKS: usize = 4;
/// Keep the speakers on even while a dashboard plays the same mix. Two copies
/// of the match a fraction of a second apart is worse than either alone, so
/// the speaker is muted by default; this is the way out for a machine whose
/// browser is muted, or that captures the game's own output instead.
const ENV_LOCAL_AUDIO: &str = "VERA_SPECTATE_LOCAL_AUDIO";

/// Truthiness of [`ENV_LOCAL_AUDIO`], read once per process.
fn local_audio_forced() -> bool {
    std::env::var(ENV_LOCAL_AUDIO).is_ok_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "on" | "true" | "yes"
        )
    })
}

/// A running pump. Dropping it stops the thread.
pub struct AudioPump {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl AudioPump {
    /// Start publishing the shared mixer's audio to `sink`. Call once per
    /// process: a second pump would publish every block twice.
    ///
    /// The mixer may not exist yet — audio players open lazily — so the thread
    /// waits for it instead of failing, which makes the start order irrelevant.
    pub fn spawn(sink: AudioSink) -> Self {
        crate::audio::tap_output::set_local_audio_forced(local_audio_forced());
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let handle = thread::Builder::new()
            .name("vera-audio-pump".to_owned())
            .spawn(move || wait_for_mixer(sink, &flag))
            .ok();
        Self { stop, handle }
    }
}

/// Wait until a player has opened the shared output, then pump it forever.
fn wait_for_mixer(sink: AudioSink, stop: &AtomicBool) {
    while !stop.load(Ordering::Acquire) {
        match crate::audio::tap_output::AudioTap::tap_if_open() {
            Some((ring, sample_rate)) => {
                log::info!("Live audio pump following the shared mixer at {sample_rate} Hz");
                pump(ring, sample_rate, sink, stop);
                return;
            }
            None => thread::sleep(Duration::from_millis(200)),
        }
    }
}

impl Drop for AudioPump {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        // Nobody is carrying the mix any more: the speaker follows the window
        // again.
        crate::audio::tap_output::set_browser_listening(false);
    }
}

fn pump(ring: Arc<PcmRing>, sample_rate: u32, sink: AudioSink, stop: &AtomicBool) {
    let block_frames = (sample_rate as u64 * u64::from(BLOCK_MS) / 1000) as usize;
    if block_frames == 0 {
        return;
    }
    let backlog_limit = block_frames * MAX_BACKLOG_BLOCKS;
    let mut samples = vec![0_i16; block_frames * CHANNELS as usize];
    let mut sequence = 0_u32;
    let mut next = Instant::now();
    let mut listening = false;
    while !stop.load(Ordering::Acquire) {
        let now = Instant::now();
        if next > now {
            thread::sleep(next - now);
        } else if now.duration_since(next) > WAKE {
            // Fell behind (a slow subscribe, a busy machine): restart the
            // cadence instead of bursting to catch up.
            next = now;
        }
        let wanted = sink.wanted();
        if wanted != listening {
            listening = wanted;
            // While a dashboard plays this mix, the speaker stops: hearing the
            // match twice, once live and once from the browser a moment later,
            // sounds like a fault.
            crate::audio::tap_output::set_browser_listening(wanted);
            log::info!(
                "Dashboard audio {}",
                if wanted {
                    "subscribed: the local speaker is muted until it stops"
                } else {
                    "ended: the local speaker follows the window again"
                }
            );
        }
        if !wanted {
            // Nothing is listening. Drop what accumulated rather than replay
            // it to the next subscriber as a burst of stale sound.
            ring.clear();
            next = Instant::now() + Duration::from_millis(u64::from(BLOCK_MS));
            continue;
        }
        // Keep the stream current: skip ahead when the listener fell behind.
        let mut queued = ring.queued();
        while queued > backlog_limit {
            if ring.pop().is_none() {
                break;
            }
            queued -= 1;
        }
        let take = queued.min(block_frames);
        if take == 0 {
            next += Duration::from_millis(u64::from(BLOCK_MS));
            continue;
        }
        let mut written = 0;
        for _ in 0..take {
            let Some((left, right)) = ring.pop() else {
                break;
            };
            samples[written] = left;
            samples[written + 1] = right;
            written += 2;
        }
        if written == 0 {
            next += Duration::from_millis(u64::from(BLOCK_MS));
            continue;
        }
        sink.publish(&encode_block(sample_rate, (written / 2) as u16, sequence, &samples[..written]));
        sequence = sequence.wrapping_add(1);
        next += Duration::from_millis(u64::from(BLOCK_MS));
    }
}

/// One `AMAG` block. The dashboard reads the header, then the PCM as
/// little-endian 16-bit stereo.
fn encode_block(sample_rate: u32, frames: u16, sequence: u32, samples: &[i16]) -> Vec<u8> {
    let mut block = Vec::with_capacity(HEADER_BYTES + samples.len() * 2);
    block.extend_from_slice(MAGIC);
    block.extend_from_slice(&sample_rate.to_le_bytes());
    block.extend_from_slice(&CHANNELS.to_le_bytes());
    block.extend_from_slice(&frames.to_le_bytes());
    block.extend_from_slice(&sequence.to_le_bytes());
    for sample in samples {
        block.extend_from_slice(&sample.to_le_bytes());
    }
    block
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_header_announces_the_pcm_that_follows() {
        let samples = [1_i16, -1, 32767, -32767];
        let block = encode_block(44_100, 2, 7, &samples);
        assert_eq!(&block[..4], MAGIC);
        assert_eq!(&block[4..8], &44_100_u32.to_le_bytes());
        assert_eq!(&block[8..10], &2_u16.to_le_bytes());
        assert_eq!(&block[10..12], &2_u16.to_le_bytes());
        assert_eq!(&block[12..16], &7_u32.to_le_bytes());
        assert_eq!(block.len(), HEADER_BYTES + 8);
        assert_eq!(&block[16..18], &1_i16.to_le_bytes());
        assert_eq!(&block[18..20], &(-1_i16).to_le_bytes());
    }
}
