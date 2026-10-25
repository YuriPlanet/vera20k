//! One Prepare/AdvancePlaylist/OnChannelSampleEnd owner and its PCM ring.
//!
//! gamemd404700/4047B0 select one buffer at a time. Fill409880 invokes
//! OnChannelSampleEnd405AC0 while filling the initial ring and consumed
//! quarters; this is lookahead in PCM bytes, not a queued complete playlist
//! pass. Sources: tools/input_oracle/gattling_loop.{py,json,meta.json,md} and
//! tools/spatial_oracle/ambient_playout.{py,json,meta.json}.

use super::arbiter::{PlayoutEnd, delay_below_floor};
use super::{
    DecodedAudio, LoadedPlayback, LoadedSampleIndices, PlayShifts, PlaybackDraws, SampleRng,
};
use crate::rules::sound_ini::control;
use rodio::Source;
use std::collections::BTreeMap;
use std::num::NonZero;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

/// Owns native+AC/+160/+1E0/+1E4, flags0x10 and the gap +0x13C/+0x148 one
/// playout leaves. Admission and handles stay in the arbiter.
#[derive(Default)]
pub(super) struct PlaylistCursor {
    selected: LoadedSampleIndices,
    remaining: Vec<usize>,
    control: u32,
    delay_ms: (i32, i32),
    loop_count: i32,
    iteration: i32,
    decay_used: bool,
    no_replay: bool,
    /// SelectNextSample's `+0x268` for this playout (0 below the floor).
    queue_count: i32,
    /// `+0x13C` and whether `+0x148` holds a buffer, from the sample end
    /// that drew a gap.
    gap: Option<(u32, bool)>,
}

impl PlaylistCursor {
    pub(super) fn new(
        selected: LoadedSampleIndices,
        control: u32,
        delay_ms: (i32, i32),
        loop_count: i32,
    ) -> Self {
        let remaining = Vec::with_capacity(selected.middle.len());
        Self {
            selected,
            remaining,
            control,
            delay_ms,
            loop_count,
            ..Self::default()
        }
    }

    /// An above-floor playout: the queue LoadSamples left and the event's
    /// flags0x10, which outlives each pass's buffers.
    pub(super) fn with_queue(mut self, queue_count: i32, decay_used: bool) -> Self {
        self.queue_count = queue_count;
        self.decay_used = decay_used;
        self
    }

    pub(super) fn decay_used(&self) -> bool {
        self.decay_used
    }

    /// Both4045D1 and404673 reset the same remaining list. Attack bypasses
    /// RandomRanged; SkipAttack reallocation selects one body here instead.
    /// From the floor up (`0x00404727`) PreparePlayout returns the first
    /// loaded buffer and builds no playlist.
    pub(super) fn prepare(
        &mut self,
        rng: &mut impl SampleRng,
        plays_attack: bool,
    ) -> Option<usize> {
        if !delay_below_floor(self.delay_ms.0) {
            return self.selected.middle.first().copied();
        }
        self.remaining.clear();
        self.remaining.extend_from_slice(&self.selected.middle);
        // Prepare404700 does not clear flags0x10. Only allocation creates
        // an unused decay; another preparation must not replay it.
        if plays_attack && let Some(attack) = self.selected.attack {
            return Some(attack);
        }
        self.advance(rng)
    }

    pub(super) fn prevent_replay(&mut self) {
        self.no_replay = true;
    }

    pub(super) fn advance(&mut self, rng: &mut impl SampleRng) -> Option<usize> {
        // Original4047BC..4047C5: every arm needs Delay.min below the floor
        // or NO_REPLAY, which leaves only the decay arm.
        if !delay_below_floor(self.delay_ms.0) && !self.no_replay {
            return None;
        }
        if !self.no_replay && !self.remaining.is_empty() {
            let pick = if self.control & control::RANDOM != 0 {
                rng.ranged(0, self.remaining.len() as i32 - 1) as usize
            } else {
                0
            };
            return Some(self.remaining.remove(pick.min(self.remaining.len() - 1)));
        }
        // Original40483D..404879 checks LOOP before DECAY. This counter is
        // authoritative here; the arbiter must not spend passes in advance.
        if !self.no_replay
            && self.control & control::LOOP != 0
            && (self.loop_count == 0 || self.iteration < self.loop_count - 1)
        {
            self.iteration += 1;
            // Empty loaded bodies cannot sustain a zero-byte callback loop.
            if !self.selected.middle.is_empty() {
                return self.prepare(rng, false);
            }
        }
        if !self.decay_used
            && let Some(decay) = self.selected.decay
        {
            // Original40487E..40489A writes flags0x10 only when a
            // Control=decay buffer actually exists. A one-shot exhaustion
            // must not invent that lifecycle flag.
            self.decay_used = true;
            return Some(decay);
        }
        None
    }

    /// `SoundEvent::OnChannelSampleEnd @ 0x00405AC0`: the buffer after the
    /// one that ended. AdvancePlaylist picks it; from a `Delay=` high bound of
    /// `0x21` up the channel stops instead, unless a gap is drawn (any
    /// pending buffer, queue, or neither `Control=predelay` nor `ambient`)
    /// and falls under the floor. A larger gap is kept for
    /// `OnChannelDone`. Both floor tests are unsigned (`JNC`).
    pub(super) fn sample_end(&mut self, rng: &mut impl SampleRng) -> Option<usize> {
        let next = self.advance(rng);
        if delay_below_floor(self.delay_ms.1) {
            return next;
        }
        if self.control & (control::PREDELAY | control::AMBIENT) == 0
            || next.is_some()
            || self.queue_count != 0
        {
            let gap = rng.ranged(self.delay_ms.0, self.delay_ms.1);
            if delay_below_floor(gap) {
                return next;
            }
            self.gap = Some((gap as u32, next.is_some()));
        }
        None
    }

    /// OnChannelDone405A00's inputs; it consumes the gap (`+0x13C = 0`).
    pub(super) fn playout_end(&mut self) -> PlayoutEnd {
        let (gap_ms, next) = self.gap.take().unwrap_or_default();
        PlayoutEnd {
            gap_ms,
            next,
            queue_count: self.queue_count,
        }
    }
}

/// Native40A4A6..40A551: round_up_1024((avgBytes*3 /100)*8), then four
/// quarters. The source rate, not its shifted playback frequency, is used.
fn quarter_samples(clip: &DecodedAudio) -> Option<usize> {
    let frame_bytes = u32::from(clip.native_channels) * u32::from(clip.native_sample_bytes);
    let bytes_per_second = clip.sample_rate.checked_mul(frame_bytes)?;
    let nominal = (bytes_per_second.checked_mul(3)? / 100).checked_mul(8)?;
    let quarter_bytes = nominal.checked_add(1023)? & !1023;
    let frames = quarter_bytes.checked_div(frame_bytes)?;
    usize::try_from(frames.checked_mul(u32::from(clip.channels))?)
        .ok()
        .filter(|n| *n != 0)
}

/// The buffer format `DSoundBuffer::StartPlayback @ 0x0040A340` compares:
/// rate, channels and bytes per sample (`0x0040A380..0x0040A3A4`).
fn native_format(clip: &DecodedAudio) -> (u32, u16, u16) {
    (
        clip.sample_rate,
        clip.native_channels,
        clip.native_sample_bytes,
    )
}

/// A chained clip's samples as a ring of another channel count or width
/// (`layout`: channels, bytes per sample) plays them; `None` when the layouts
/// match and its samples copy unchanged.
///
/// Mid-ring, `OnChannelSampleEnd @ 0x00405AC0` only moves the data cursor
/// (`DSoundBuffer::SetSample @ 0x00402790`), and `Fill @ 0x00409880`
/// (`0x00409A9D`) → `ConfigureSampleDecoder @ 0x00409C40` re-selects only
/// the decode callback: a byte copy for PCM of any width (`0x00409D90`), or
/// `IMA_ADPCM__DecodeBlock @ 0x0040AA70`, which writes 16-bit samples in the
/// clip's own channel order (`0x0040AB8D`, channels from `0x00409C94`).
/// Nothing re-creates the buffer or sets its frequency, so the ring reads
/// the clip's PCM bytes at its own rate, channels and width. The decoders'
/// maps `(b - 128) / 128` and `s / 32768` are exact, so the bytes rebuild
/// exactly.
///
/// RESIDUAL: an odd byte count read as 16-bit drops its last byte. Native's
/// ring is one byte stream, so every later 16-bit sample in that playout is
/// read one byte off and plays as noise; VERA plays the following clips
/// cleanly. Trigger: an odd-length 8-bit mono clip chained into a 16-bit
/// ring; no retail entry chains one.
pub(super) fn chained_samples(clip: &DecodedAudio, layout: (u16, u16)) -> Option<Vec<f32>> {
    if (clip.native_channels, clip.native_sample_bytes) == layout {
        return None;
    }
    let step = if clip.native_channels == 1 { 2 } else { 1 };
    let mut bytes = Vec::new();
    for &value in clip.samples.iter().step_by(step) {
        if clip.native_sample_bytes == 1 {
            bytes.push((value * 128.0 + 128.0) as u8);
        } else {
            bytes.extend_from_slice(&((value * 32768.0) as i16).to_le_bytes());
        }
    }
    let (channels, width) = layout;
    let mut pcm = Vec::new();
    if let Some(mut cursor) = crate::assets::wav_file::WavPcmCursor::pcm(width * 8) {
        while cursor.decode_next(&bytes, &mut pcm) {}
    }
    let mut samples = Vec::new();
    super::upmix_i16_to_f32_stereo(&mut samples, &pcm, channels);
    Some(samples)
}

struct RingState {
    loaded: LoadedPlayback,
    cursor: PlaylistCursor,
    rng: PlaybackDraws,
    shifts: PlayShifts,
    current: Option<usize>,
    clip_offset: usize,
    ring: Vec<f32>,
    valid: [usize; 4],
    quarter_samples: usize,
    read_quarter: usize,
    first_pull: bool,
    cancelled: bool,
    /// The buffer's [`native_format`] and shifted frequency, set per start.
    format: (u32, u16, u16),
    rate: NonZero<u32>,
    /// [`chained_samples`] of each loaded clip whose layout differs from
    /// the buffer's.
    reinterpreted: BTreeMap<usize, Vec<f32>>,
    observation: Option<Arc<PlaybackTrace>>,
    // Diagnostic provenance follows every actual ring write, including
    // replacement on release. It never supplies a playback decision.
    provenance: Option<Vec<usize>>,
}

impl RingState {
    /// `DSoundBuffer::StartPlayback @ 0x0040A340` for one start: a playout's
    /// first clip and the release→decay restart (`SoundEvent::UpdateState`
    /// `0x004058DC` → `SoundEvent::StartPlayback @ 0x004054A0`). A clip
    /// whose format differs from the buffer's releases it and creates one in
    /// its own (`0x0040A48A..0x0040A587`). `0x00402800` (at `0x0040A36D`)
    /// copies the clip's format into the channel (`+0x17C`, rate `+0x184`),
    /// and `ApplyVolumePanFrequency @ 0x0040A6D0` sets the frequency from that
    /// rate and the event's pitch (`+0xD8`). Only a start (`0x0040A36D`) and
    /// init (`0x0040A2C6`) write `+0x184`, so the rate never changes mid-ring.
    /// Either way the ring refills whole (`0x0040A463`). False when the clip's
    /// format admits no ring.
    fn start(&mut self, index: Option<usize>) -> bool {
        let mut usable = true;
        // With no buffer yet (0x0040A378) it creates one without comparing.
        if let Some(clip) = index.and_then(|index| self.loaded.clips.get(&index))
            && (self.ring.is_empty() || native_format(clip) != self.format)
        {
            let format = native_format(clip);
            let rate = NonZero::new(self.shifts.shifted_sample_rate(clip.sample_rate));
            let quarter = quarter_samples(clip).filter(|quarter| quarter.checked_mul(4).is_some());
            if let (Some(rate), Some(quarter)) = (rate, quarter) {
                self.format = format;
                self.rate = rate;
                self.quarter_samples = quarter;
                self.ring = vec![0.0; quarter * 4];
                if let Some(provenance) = &mut self.provenance {
                    *provenance = vec![usize::MAX; quarter * 4];
                }
                self.reinterpreted = self
                    .loaded
                    .clips
                    .iter()
                    .filter_map(|(&index, clip)| {
                        Some((index, chained_samples(clip, (format.1, format.2))?))
                    })
                    .collect();
            } else {
                usable = false;
            }
        }
        self.current = index.filter(|_| usable);
        self.clip_offset = 0;
        self.fill_initial();
        usable
    }

    fn fill(&mut self, start: usize, capacity: usize) -> usize {
        let mut copied = 0;
        let mut empty_selections = 0;
        while copied < capacity {
            let Some(index) = self.current else { break };
            let Some(clip) = self.loaded.clips.get(&index) else {
                empty_selections += 1;
                if empty_selections
                    > self.loaded.clips.len() + self.loaded.selected.middle.len() + 2
                {
                    // Missing retail payloads may not spin a device callback.
                    // Successful loaded paths never enter this guard.
                    self.current = None;
                    break;
                }
                self.current = self.cursor.sample_end(&mut self.rng);
                self.clip_offset = 0;
                continue;
            };
            // The clip plays in the buffer's format (see chained_samples).
            let samples = self.reinterpreted.get(&index).unwrap_or(&clip.samples);
            let available = samples.len().saturating_sub(self.clip_offset);
            if available == 0 {
                empty_selections += 1;
                if empty_selections
                    > self.loaded.clips.len() + self.loaded.selected.middle.len() + 2
                {
                    self.current = None;
                    break;
                }
                self.current = self.cursor.sample_end(&mut self.rng);
                self.clip_offset = 0;
                continue;
            }
            let count = available.min(capacity - copied);
            if let Some(trace) = &self.observation {
                let slot = trace
                    .clips
                    .iter()
                    .position(|clip| clip.index == index)
                    .unwrap();
                if self.clip_offset == 0 {
                    trace.started(slot);
                }
                trace.clips[slot]
                    .filled_samples
                    .fetch_add(count, Ordering::Relaxed);
                self.provenance.as_mut().unwrap()[start + copied..start + copied + count]
                    .fill(slot);
            }
            self.ring[start + copied..start + copied + count]
                .copy_from_slice(&samples[self.clip_offset..self.clip_offset + count]);
            self.clip_offset += count;
            copied += count;
            empty_selections = 0;
            // Original409BCA returns when capacity is exhausted. Selection
            // at an exact clip end waits for the next actual fill request.
        }
        self.ring[start + copied..start + capacity].fill(0.0);
        if let Some(provenance) = &mut self.provenance {
            provenance[start + copied..start + capacity].fill(usize::MAX);
        }
        copied
    }

    fn fill_initial(&mut self) {
        let copied = self.fill(0, self.ring.len());
        for (q, valid) in self.valid.iter_mut().enumerate() {
            *valid = copied
                .saturating_sub(q * self.quarter_samples)
                .min(self.quarter_samples);
        }
        self.read_quarter = 0;
        self.first_pull = true;
    }

    fn next_quarter(&mut self, target: &mut [f32], provenance: Option<&mut [usize]>) -> usize {
        if self.cancelled {
            return 0;
        }
        if self.first_pull {
            self.first_pull = false;
        } else {
            let previous = self.read_quarter;
            let ended = self.valid[previous] < self.quarter_samples;
            self.valid[previous] = self.fill(previous * self.quarter_samples, self.quarter_samples);
            // The native last refill also overwrites the consumed padded
            // quarter with silence. It cannot select another clip, but its
            // real buffer write remains observable before endpoint delivery.
            if ended {
                return 0;
            }
            self.read_quarter = (previous + 1) % 4;
        }
        let count = self.valid[self.read_quarter];
        let start = self.read_quarter * self.quarter_samples;
        if count == 0 {
            return 0;
        }
        // Original worker stops only once decoded_remaining becomes negative
        // on the quarter crossing. Keep the last quarter's zero padding.
        target.copy_from_slice(&self.ring[start..start + self.quarter_samples]);
        if let Some(target) = provenance {
            target.copy_from_slice(
                &self.provenance.as_ref().unwrap()[start..start + self.quarter_samples],
            );
        }
        self.quarter_samples
    }
}

const MAX_OBSERVED_CLIP_STARTS: usize = 128;

struct ClipCounters {
    index: usize,
    name: String,
    filled_samples: AtomicUsize,
    pulled_samples: AtomicUsize,
}

struct PlaybackTrace {
    clips: Vec<ClipCounters>,
    starts: [AtomicUsize; MAX_OBSERVED_CLIP_STARTS],
    start_count: AtomicUsize,
}

impl PlaybackTrace {
    fn started(&self, slot: usize) {
        // Fills are serialized by the existing output boundary gate. Publish
        // the slot before its count; snapshots also hold that same gate.
        let count = self.start_count.load(Ordering::Relaxed);
        if let Some(start) = self.starts.get(count) {
            start.store(slot, Ordering::Relaxed);
        }
        self.start_count.store(count + 1, Ordering::Release);
    }
}

#[derive(Clone, serde::Serialize)]
pub(super) struct PlaybackClipObservation {
    pub(super) name: String,
    pub(super) filled_samples: usize,
    pub(super) pulled_samples: usize,
}

pub(super) struct PlaybackObservationSnapshot {
    pub(super) resolved_samples: Vec<String>,
    pub(super) source_sample_count: usize,
    pub(super) clips: Vec<PlaybackClipObservation>,
    pub(super) source_state_alive: bool,
    pub(super) truncated: bool,
}

/// Passive counters outlive the source; a weak reference independently
/// witnesses whether its loaded clips, cursor and RNG capability still live.
#[derive(Clone)]
pub(super) struct PlaybackObservation {
    trace: Arc<PlaybackTrace>,
    state: Weak<SharedOutput>,
}

impl PlaybackObservation {
    pub(super) fn snapshot(&self) -> PlaybackObservationSnapshot {
        // A live fill publishes its copied counts before cache pulls. Hold
        // the existing fill gate while copying counters so a future refill
        // cannot make a newer pull exceed an older copied count in this row.
        // Once the weak reference expires every counter is stable.
        let state = self.state.upgrade();
        let _gate = state
            .as_ref()
            .map(|state| state.state.lock().expect("SFX boundary gate poisoned"));
        let starts = self.trace.start_count.load(Ordering::Acquire);
        let resolved_samples = self.trace.starts[..starts.min(MAX_OBSERVED_CLIP_STARTS)]
            .iter()
            .map(|slot| self.trace.clips[slot.load(Ordering::Relaxed)].name.clone())
            .collect();
        let clips: Vec<_> = self
            .trace
            .clips
            .iter()
            .map(|clip| PlaybackClipObservation {
                name: clip.name.clone(),
                filled_samples: clip.filled_samples.load(Ordering::Relaxed),
                pulled_samples: clip.pulled_samples.load(Ordering::Relaxed),
            })
            .collect();
        PlaybackObservationSnapshot {
            resolved_samples,
            source_sample_count: clips.iter().map(|clip| clip.filled_samples).sum(),
            clips,
            source_state_alive: state.is_some(),
            truncated: starts > MAX_OBSERVED_CLIP_STARTS,
        }
    }
}

struct SharedOutput {
    state: Mutex<RingState>,
    cancelled: AtomicBool,
    generation: AtomicU64,
    pan: AtomicI32,
}

/// Main-thread controls share the same boundary gate as buffer selection.
/// Lock order is output gate → one Main operation. No Main guard survives a
/// draw, asset loading, player submission or control acquisition.
#[derive(Clone)]
pub(super) struct PlaybackControl(Arc<SharedOutput>);

impl PlaybackControl {
    pub(super) fn cancel(&self) {
        let mut state = self.0.state.lock().expect("SFX boundary gate poisoned");
        state.cancelled = true;
        self.0.cancelled.store(true, Ordering::Release);
    }

    /// Native Release406060/Detach405FD0 bracket the live channel while
    /// mutating its event flags. The arbiter supplies that sole decision;
    /// the source gate makes it atomic with respect to quarter selection.
    pub(super) fn update_no_replay(&self, update_event: impl FnOnce() -> bool) {
        let mut state = self.0.state.lock().expect("SFX boundary gate poisoned");
        if update_event() {
            state.cursor.prevent_replay();
        }
    }

    /// The release→decay restart: UpdateState4058DC advances to the decay
    /// clip and starts the channel again (`RingState::start`).
    pub(super) fn release_to_decay(&self) -> bool {
        let mut state = self.0.state.lock().expect("SFX boundary gate poisoned");
        if state.cancelled {
            return false;
        }
        state.cursor.prevent_replay();
        let next = {
            let RingState { cursor, rng, .. } = &mut *state;
            cursor.advance(rng)
        };
        state.start(next);
        self.0.generation.fetch_add(1, Ordering::Release);
        state.valid.iter().any(|n| *n != 0)
    }

    pub(super) fn set_pan(&self, pan: i32) {
        self.0.pan.store(pan, Ordering::Relaxed);
    }

    /// The channel ran dry: OnChannelDone405A00's inputs, and the loaded
    /// buffers and cursor the sample owner keeps for the event's next start
    /// pass. The source is spent; any later pull ends at once.
    pub(super) fn finish(&self) -> (PlayoutEnd, LoadedPlayback, PlaylistCursor) {
        let mut state = self.0.state.lock().expect("SFX boundary gate poisoned");
        state.cancelled = true;
        self.0.cancelled.store(true, Ordering::Release);
        let mut cursor = std::mem::take(&mut state.cursor);
        let loaded = std::mem::take(&mut state.loaded);
        (cursor.playout_end(), loaded, cursor)
    }
}

/// One continuous rodio source. Ordinary sample pulls are lock-free; only
/// quarter refill and controls take the gate. The cache is storage, not a
/// second playlist or RNG cursor; a release epoch discards old cached PCM.
pub(super) struct RingSource {
    shared: Arc<SharedOutput>,
    cache: Vec<f32>,
    valid: usize,
    position: usize,
    generation: u64,
    channels: NonZero<u16>,
    sample_rate: NonZero<u32>,
    last_pan: i32,
    pan_gain: (f32, f32),
    observation: Option<Arc<PlaybackTrace>>,
    provenance: Option<Vec<usize>>,
}

impl RingSource {
    pub(super) fn new(
        loaded: LoadedPlayback,
        cursor: PlaylistCursor,
        initial: usize,
        rng: PlaybackDraws,
        shifts: PlayShifts,
        pan: i32,
        observed_names: Option<&[String]>,
    ) -> Option<(Self, PlaybackControl)> {
        let channels = NonZero::new(loaded.clips.get(&initial)?.channels)?;
        let observation = observed_names.map(|names| {
            Arc::new(PlaybackTrace {
                clips: loaded
                    .clips
                    .keys()
                    .map(|&index| ClipCounters {
                        index,
                        name: names[index].clone(),
                        filled_samples: AtomicUsize::new(0),
                        pulled_samples: AtomicUsize::new(0),
                    })
                    .collect(),
                starts: std::array::from_fn(|_| AtomicUsize::new(usize::MAX)),
                start_count: AtomicUsize::new(0),
            })
        });
        let mut state = RingState {
            loaded,
            cursor,
            rng,
            shifts,
            current: None,
            clip_offset: 0,
            ring: Vec::new(),
            valid: [0; 4],
            quarter_samples: 0,
            read_quarter: 0,
            first_pull: true,
            cancelled: false,
            // The first start creates the buffer and sets these.
            format: (0, 0, 0),
            rate: NonZero::<u32>::MIN,
            reinterpreted: BTreeMap::new(),
            observation: observation.clone(),
            provenance: observation.as_ref().map(|_| Vec::new()),
        };
        // Native40A463 fills the complete ring synchronously at start. Later
        // callbacks refill consumed quarters on the same Main capability.
        if !state.start(Some(initial)) {
            return None;
        }
        let (quarter, sample_rate) = (state.quarter_samples, state.rate);
        let shared = Arc::new(SharedOutput {
            state: Mutex::new(state),
            cancelled: AtomicBool::new(false),
            generation: AtomicU64::new(0),
            pan: AtomicI32::new(pan),
        });
        let control = PlaybackControl(Arc::clone(&shared));
        Some((
            Self {
                shared,
                cache: vec![0.0; quarter],
                valid: 0,
                position: 0,
                generation: 0,
                channels,
                sample_rate,
                last_pan: pan,
                pan_gain: super::pan_channel_gains(pan),
                observation: observation.clone(),
                provenance: observation.map(|_| vec![usize::MAX; quarter]),
            },
            control,
        ))
    }

    pub(super) fn observation(&self) -> Option<PlaybackObservation> {
        self.observation.as_ref().map(|trace| PlaybackObservation {
            trace: Arc::clone(trace),
            state: Arc::downgrade(&self.shared),
        })
    }

    #[cfg(test)]
    pub(super) fn native_snapshot(&self) -> serde_json::Value {
        let state = self
            .shared
            .state
            .lock()
            .expect("SFX boundary gate poisoned");
        serde_json::json!({
            "ring": state.ring,
            "current": state.current,
            "clip_offset": state.clip_offset,
            "remaining": state.cursor.remaining,
            "iteration": state.cursor.iteration,
            "decay_used": state.cursor.decay_used,
            "no_replay": state.cursor.no_replay,
            "cancelled": state.cancelled,
            "quarter_samples": state.quarter_samples,
        })
    }

    #[cfg(test)]
    pub(super) fn fill_capacity_for_test(&self, start: usize, capacity: usize) {
        // Supplied component capacity in the native exact-end control.
        // Execute the production fill; this adapter makes no selection.
        self.shared
            .state
            .lock()
            .expect("SFX boundary gate poisoned")
            .fill(start, capacity);
    }
}

impl Iterator for RingSource {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.shared.cancelled.load(Ordering::Acquire) {
            return None;
        }
        let channels = usize::from(self.channels.get());
        let generation = self.shared.generation.load(Ordering::Acquire);
        if self.generation != generation {
            self.generation = generation;
            self.valid = 0;
            self.position %= channels;
        }
        if self.position >= self.valid {
            let mut state = self
                .shared
                .state
                .lock()
                .expect("SFX boundary gate poisoned");
            // Pair the fetched quarter with its reset epoch under the gate;
            // otherwise release during acquisition could skip new decay PCM.
            self.generation = self.shared.generation.load(Ordering::Acquire);
            // A restart in another format created a new buffer: take its
            // quarter and frequency (RingState::start). RESIDUAL: rodio
            // re-reads a source's rate only where the span it last read ends
            // (rodio 0.22.2 `UniformSourceIterator::bootstrap`), so the rest
            // of that span, at most one quarter (about 0.24 s), plays the
            // decay clip at the previous rate; native's new buffer starts at
            // its own (0x40A587). Trigger: a Control=decay clip of another
            // rate than the playing one; no retail entry has one.
            if self.cache.len() != state.quarter_samples {
                self.cache.resize(state.quarter_samples, 0.0);
                if let Some(provenance) = &mut self.provenance {
                    provenance.resize(state.quarter_samples, usize::MAX);
                }
            }
            self.sample_rate = state.rate;
            self.valid = state.next_quarter(&mut self.cache, self.provenance.as_deref_mut());
            // Native `DSoundBuffer::StartPlayback @ 0x0040A340` refills the
            // whole buffer (`0x0040A463`). Rodio can pull one stereo sample
            // before that restart and the next after it, so keep the pending
            // channel offset. A normal quarter ends at a frame boundary, so
            // this also resets its next cache offset to zero.
            self.position %= channels;
            if self.valid == 0 {
                return None;
            }
        }
        let value = self.cache[self.position];
        let channel = self.position % channels;
        if let Some(trace) = &self.observation {
            let slot = self.provenance.as_ref().unwrap()[self.position];
            if slot != usize::MAX {
                trace.clips[slot]
                    .pulled_samples
                    .fetch_add(1, Ordering::Relaxed);
            }
        }
        self.position += 1;
        let pan = self.shared.pan.load(Ordering::Relaxed);
        if pan != self.last_pan {
            self.last_pan = pan;
            self.pan_gain = super::pan_channel_gains(pan);
        }
        let (left, right) = self.pan_gain;
        Some(value * if channel == 0 { left } else { right })
    }
}

/// Longest span this source reports, in frames. VERA-owned, not native.
///
/// rodio 0.22.2 (and 0.23.0) `Mixer::add` converts every source with one
/// `SampleRateConverter` per span, whose `from * next_output_span_pos_in_chunk`
/// (`conversions/sample_rate.rs:158`, `:173`) is `u32`. It overflows once a
/// span outputs more than `u32::MAX / from` frames: a cue over about a second
/// at a shifted rate coprime with a 192 kHz device (103% of 22 050 Hz is
/// 22 711 Hz). Overflow-checked builds panic in the device callback; release
/// builds spin about 2^32 converter steps there, stalling every live cue.
/// A span outputs about `frames * device_rate` frames' worth of that product,
/// so 8192 frames stays below `u32::MAX` up to a 384 kHz device.
const MAX_SPAN_FRAMES: usize = 8192;

impl Source for RingSource {
    fn current_span_len(&self) -> Option<usize> {
        // rodio0.22.2 Source::current_span_len permits Some(0) only at
        // exhaustion. Only a restart in another rate changes this source's
        // format (see Iterator::next).
        // A bounded span only restarts the mixer's converter (see
        // MAX_SPAN_FRAMES). At a quarter boundary the next quarter, if any,
        // holds exactly one cache length; an ended source still ends early
        // through Iterator::next.
        let remaining = if self.position < self.valid {
            self.valid - self.position
        } else {
            self.cache.len()
        };
        Some(remaining.min(MAX_SPAN_FRAMES * usize::from(self.channels.get())))
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

#[cfg(test)]
mod tests {
    use super::super::tests::build_test_wav;
    use super::super::{SoundRegistry, decode_wav};
    use super::*;
    use crate::rules::ini_parser::IniFile;

    fn wav(rate: u32, bits: u16, channels: u16, pcm: &[u8]) -> DecodedAudio {
        decode_wav(&build_test_wav(rate, bits, channels, pcm), "test.wav").unwrap()
    }

    /// The production ring for one `[Test]` entry over `clips`, in order.
    fn ring(control: &str, clips: &[(&str, DecodedAudio)]) -> (RingSource, PlaybackControl) {
        let names: Vec<_> = clips.iter().map(|(name, _)| *name).collect();
        let registry = SoundRegistry::from_ini(&IniFile::from_str(&format!(
            "[SoundList]\n1=Test\n[Test]\nSounds={}\nControl={control}\n",
            names.join(" ")
        )));
        let entry = registry.get("Test").unwrap();
        let mut draws = PlaybackDraws::new(|low, _| low);
        let loaded = LoadedPlayback::load(entry, &mut draws, |name| {
            clips
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, clip)| clip.clone())
        })
        .unwrap();
        let mut cursor = PlaylistCursor::new(loaded.selected.clone(), entry.control, (0, 0), 0);
        let initial = cursor.prepare(&mut draws, true).unwrap();
        RingSource::new(
            loaded,
            cursor,
            initial,
            draws,
            PlayShifts::UNITY,
            0x2000,
            None,
        )
        .unwrap()
    }

    #[test]
    fn a_chained_clip_plays_its_pcm_bytes_in_the_rings_format() {
        // A stereo 16-bit ring copies the next clip's mono 8-bit bytes
        // 00 40 00 C0 (ConfigureSampleDecoder409C40 → memcpy 409D90) and
        // reads them as one frame (0x4000, 0xC000), at its own rate.
        let (source, _) = ring(
            "all",
            &[
                ("first", wav(22_050, 16, 2, &[0x00, 0x20, 0x00, 0xE0])),
                ("second", wav(11_025, 8, 1, &[0x00, 0x40, 0x00, 0xC0])),
            ],
        );
        assert_eq!(source.sample_rate().get(), 22_050);
        let state = source.shared.state.lock().unwrap();
        assert_eq!(state.ring[..6], [0.25, -0.25, 0.5, -0.5, 0.0, 0.0]);
    }

    #[test]
    fn release_restarts_a_decay_clip_in_its_own_format() {
        // StartPlayback40A340 creates a new buffer for a decay clip of
        // another rate and width (0x40A48A..0x40A587) and plays it at its own.
        let (mut source, control) = ring(
            "loop decay",
            &[
                ("body", wav(22_050, 16, 1, &[0x00, 0x20])),
                ("tail", wav(11_025, 8, 1, &[0xC0])),
            ],
        );
        assert!(control.release_to_decay());
        source.next();
        assert_eq!(source.sample_rate().get(), 11_025);
        let state = source.shared.state.lock().unwrap();
        assert_eq!(state.ring[..4], [0.5, 0.5, 0.0, 0.0]);
    }

    #[test]
    fn release_mid_stereo_frame_keeps_decay_channels_aligned() {
        let mut tail_pcm = vec![
            0x00, 0x10, 0x00, 0x20, 0x00, 0x30, 0x00, 0x40, 0x00, 0x50, 0x00, 0x60,
        ];
        for _ in 3..256 {
            tail_pcm.extend_from_slice(&[0x00, 0x10, 0x00, 0x20]);
        }
        tail_pcm.extend_from_slice(&[0x00, 0x70, 0x00, 0x50, 0x00, 0x60, 0x00, 0x40]);
        let tail = wav(30, 16, 2, &tail_pcm);
        let (mut source, control) = ring(
            "loop decay",
            &[
                ("body", wav(30, 16, 2, &[0x00, 0x70, 0x00, 0x80])),
                ("tail", tail.clone()),
            ],
        );
        control.set_pan(super::super::PAN_SCALE);

        source.next();
        assert!(control.release_to_decay());

        assert_eq!(source.next(), Some(tail.samples[1]));
        assert_eq!(source.next(), Some(0.0));
        assert_eq!(source.next(), Some(tail.samples[3]));
        assert_eq!(source.next(), Some(0.0));
        assert_eq!(source.next(), Some(tail.samples[5]));

        source.by_ref().take(506).for_each(drop);
        assert_eq!(source.next(), Some(0.0));
        assert_eq!(source.next(), Some(tail.samples[513]));
    }
}
