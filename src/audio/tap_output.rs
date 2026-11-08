//! Native audio output that can also be observed.
//!
//! Music and SFX used to open two independent CPAL streams, so nothing outside
//! the device callback could see what the player actually heard. Here both
//! players feed one rodio mixer and the single [`MixerSource`] is pulled once
//! per device period: every sample goes to the speaker and into a lock-free
//! PCM ring that the telemetry dashboard streams to its browser.
//!
//! One mix, one tap. The ring is single-producer (this callback) and
//! single-consumer (the dashboard pump), so it needs no lock and never blocks
//! the real-time thread; when it is full the newest frames are dropped, which
//! costs a fraction of a millisecond of stream audio instead of a glitch.
//!
//! The device period matches the SFX low-latency value (512 frames, ~10.7 ms
//! at 48 kHz) so short `CreditTicks` cues still reach the device before the
//! arbiter stops them. A device that refuses that period, or whose format is
//! neither f32 nor i16, leaves the caller on rodio's own sink: the tap is an
//! addition, never a reason for silence.
//!
//! Silence is applied here rather than in the players: the speaker gate below
//! covers the window-activation edge and the "a dashboard is already playing
//! this" case, and neither of them may touch the mix the ring carries.

use std::num::NonZero;
use std::sync::atomic::{AtomicU8, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use rodio::Source;
use rodio::cpal;
use rodio::cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rodio::mixer::{Mixer, MixerSource};

/// Stereo frames the tap keeps queued, ~0.68 s at 48 kHz. Deep enough that a
/// stalled dashboard pump never truncates live audio, shallow enough that a
/// resumed pump does not replay stale sound.
const RING_FRAMES: usize = 1 << 15;
/// Device period, the SFX low-latency value (see `sfx_buffer_frames`).
const PERIOD_FRAMES: u32 = 512;
/// Channels in the mix. The device may have more or fewer; the callback folds
/// the stereo mix into whatever the device takes.
const MIX_CHANNELS: u16 = 2;

/// Speaker gate bits, one word so the device callback reads a single atomic
/// and everything that decides audibility is a plain store.
///
/// The gate silences the *speakers*, never the mix. Retail stops its primary
/// DirectSound buffers when `WM_ACTIVATEAPP` says the window is gone (see
/// `music::set_focus_output_active`), which would also silence the dashboard:
/// the tap sits downstream of the players. Moving the gate into the device
/// writer keeps the mix live for the browser while the speaker behaves exactly
/// as before.
const GATE_WINDOW_ACTIVE: u8 = 1 << 0;
const GATE_BROWSER_LISTENING: u8 = 1 << 1;
const GATE_LOCAL_FORCED: u8 = 1 << 2;

/// Shared by the app, the dashboard pump and the device callback.
static GATE: AtomicU8 = AtomicU8::new(GATE_WINDOW_ACTIVE);

fn set_gate_bit(bit: u8, on: bool) {
    if on {
        GATE.fetch_or(bit, Ordering::Relaxed);
    } else {
        GATE.fetch_and(!bit, Ordering::Relaxed);
    }
}

/// The activation edge. A deactivated window goes silent, as in retail.
pub fn set_window_active(active: bool) {
    set_gate_bit(GATE_WINDOW_ACTIVE, active);
}

/// Whether a dashboard is playing this audio right now. While one is, the
/// speakers stay quiet unless the player forced them back on, so the match is
/// never heard twice — once live, once from the browser a moment later.
pub fn set_browser_listening(listening: bool) {
    set_gate_bit(GATE_BROWSER_LISTENING, listening);
}

/// Keep the speakers on even while a dashboard listens (`VERA_SPECTATE_LOCAL_AUDIO`).
pub fn set_local_audio_forced(forced: bool) {
    set_gate_bit(GATE_LOCAL_FORCED, forced);
}

/// Speaker gain for a gate word. `1.0` only when the window owns the
/// foreground — or the player asked for local audio regardless — and the
/// browser is not already carrying the mix.
fn gate_gain(gate: u8) -> f32 {
    let window_active = gate & GATE_WINDOW_ACTIVE != 0;
    let listening = gate & GATE_BROWSER_LISTENING != 0;
    let forced = gate & GATE_LOCAL_FORCED != 0;
    if window_active && (!listening || forced) {
        1.0
    } else {
        0.0
    }
}

/// Whether the process output is this module's tapped stream. When it is, the
/// players keep their volume and [`set_window_active`] owns the silence;
/// otherwise the players' own focus gate is the only thing that can silence
/// them, so the caller must keep using it.
pub fn device_output_live() -> bool {
    OUTPUT
        .get()
        .and_then(|cell| cell.lock().ok())
        .is_some_and(|slot| slot.is_some())
}

/// Lock-free stereo frame ring: the real-time callback writes, the dashboard
/// pump reads. One `u32` per frame keeps the pub/sub free of a length counter.
pub struct PcmRing {
    frames: Box<[AtomicU32]>,
    /// Next frame to write; only the audio callback advances it.
    head: AtomicUsize,
    /// Next frame to read; only the dashboard pump advances it.
    tail: AtomicUsize,
}

impl PcmRing {
    fn new() -> Self {
        Self {
            frames: (0..RING_FRAMES).map(|_| AtomicU32::new(0)).collect(),
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
        }
    }

    /// Write one frame, dropping it when the ring is full.
    fn push(&self, left: f32, right: f32) {
        let head = self.head.load(Ordering::Relaxed);
        if head.wrapping_sub(self.tail.load(Ordering::Acquire)) >= RING_FRAMES {
            return;
        }
        let packed = u32::from(quantize(left) as u16) << 16 | u32::from(quantize(right) as u16);
        self.frames[head & (RING_FRAMES - 1)].store(packed, Ordering::Relaxed);
        self.head.store(head.wrapping_add(1), Ordering::Release);
    }

    /// Take one frame, or `None` when the reader has caught up.
    pub fn pop(&self) -> Option<(i16, i16)> {
        let tail = self.tail.load(Ordering::Relaxed);
        if tail == self.head.load(Ordering::Acquire) {
            return None;
        }
        let packed = self.frames[tail & (RING_FRAMES - 1)].load(Ordering::Relaxed);
        self.tail.store(tail.wrapping_add(1), Ordering::Release);
        Some(((packed >> 16) as u16 as i16, packed as u16 as i16))
    }

    /// Frames waiting to be read.
    pub fn queued(&self) -> usize {
        self.head
            .load(Ordering::Acquire)
            .wrapping_sub(self.tail.load(Ordering::Acquire))
    }

    /// Drop everything queued. Used when no dashboard is listening, so audio
    /// never accumulates into a backlog that plays late.
    pub fn clear(&self) {
        let head = self.head.load(Ordering::Acquire);
        self.tail.store(head, Ordering::Release);
    }
}

/// `f32` mix sample to the 16-bit PCM the dashboard streams.
fn quantize(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16
}

/// Handle to the shared output. Cheap to clone; the device stream itself lives
/// for the whole process.
#[derive(Clone)]
pub struct AudioTap {
    mixer: Mixer,
    ring: Arc<PcmRing>,
    sample_rate: u32,
}

impl AudioTap {
    /// Open the shared output on first use, or join the one already running.
    pub fn open() -> Result<Self, String> {
        let cell = OUTPUT.get_or_init(|| Mutex::new(None));
        let mut slot = cell.lock().map_err(|_| "audio output lock poisoned")?;
        if slot.is_none() {
            *slot = Some(Shared::start()?);
        }
        let shared = slot.as_ref().expect("opened above");
        Ok(Self {
            mixer: shared.mixer.clone(),
            ring: Arc::clone(&shared.ring),
            sample_rate: shared.sample_rate,
        })
    }

    /// The mix every player connects to.
    pub fn mixer(&self) -> &Mixer {
        &self.mixer
    }

    /// Frames the dashboard pump reads from.
    pub fn ring(&self) -> Arc<PcmRing> {
        Arc::clone(&self.ring)
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// The running output's ring and rate, for a consumer that is not one of
    /// the players (the dashboard audio pump). `None` before any player has
    /// opened the output.
    pub fn tap_if_open() -> Option<(Arc<PcmRing>, u32)> {
        let cell = OUTPUT.get()?;
        let slot = cell.lock().ok()?;
        let shared = slot.as_ref()?;
        Some((Arc::clone(&shared.ring), shared.sample_rate))
    }
}

struct Shared {
    mixer: Mixer,
    ring: Arc<PcmRing>,
    sample_rate: u32,
    /// Held for the process. Dropping it stops all audio.
    _stream: cpal::Stream,
}

/// The process-wide output. A closed device cannot be reopened in place, and
/// both players must mix into the same source for the tap to be the real mix.
static OUTPUT: OnceLock<Mutex<Option<Shared>>> = OnceLock::new();

impl Shared {
    fn start() -> Result<Self, String> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or_else(|| "no default output device".to_owned())?;
        let supported = device
            .default_output_config()
            .map_err(|error| format!("output config: {error}"))?;
        let sample_rate = supported.sample_rate();
        let channels = supported.channels();
        let format = supported.sample_format();
        let rate = NonZero::new(sample_rate).ok_or_else(|| "zero sample rate".to_owned())?;
        let (mixer, mut source) = rodio::mixer::mixer(
            NonZero::new(MIX_CHANNELS).expect("two channels"),
            rate,
        );
        let ring = Arc::new(PcmRing::new());
        let config = cpal::StreamConfig {
            channels,
            sample_rate,
            buffer_size: cpal::BufferSize::Fixed(PERIOD_FRAMES),
        };
        let error_callback = |error| log::warn!("Audio output stream error: {error}");
        let writer = Arc::clone(&ring);
        let stream = match format {
            cpal::SampleFormat::F32 => device.build_output_stream(
                &config,
                move |data: &mut [f32], _| fill_f32(data, channels, &mut source, &writer),
                error_callback,
                None,
            ),
            cpal::SampleFormat::I16 => device.build_output_stream(
                &config,
                move |data: &mut [i16], _| fill_i16(data, channels, &mut source, &writer),
                error_callback,
                None,
            ),
            other => {
                return Err(format!("unsupported sample format {other:?}"));
            }
        }
        .map_err(|error| format!("build output stream: {error}"))?;
        stream
            .play()
            .map_err(|error| format!("start output stream: {error}"))?;
        log::info!(
            "Shared audio output: {PERIOD_FRAMES}-frame periods, {channels} ch {sample_rate} Hz {format:?}, tap ring {} frames",
            RING_FRAMES
        );
        Ok(Self {
            mixer,
            ring,
            sample_rate,
            _stream: stream,
        })
    }
}

/// Pull one device period from the mix, write it to the device, and copy it
/// into the ring. Runs on the real-time thread: no allocation, no lock, no
/// logging.
///
/// The ring takes the mix *before* the speaker gate, so a deactivated window
/// or a listening dashboard never changes what the stream carries — the
/// dashboard has its own volume control.
fn fill_f32(data: &mut [f32], channels: cpal::ChannelCount, source: &mut MixerSource, ring: &PcmRing) {
    let gain = gate_gain(GATE.load(Ordering::Relaxed));
    for frame in data.chunks_mut(channels as usize) {
        let (left, right) = next_stereo(source);
        match frame {
            [mono] => *mono = (left + right) * 0.5 * gain,
            [l, r, rest @ ..] => {
                *l = left * gain;
                *r = right * gain;
                rest.fill(0.0);
            }
            [] => {}
        }
        ring.push(left, right);
    }
}

/// Same as [`fill_f32`] for devices that take 16-bit output.
fn fill_i16(data: &mut [i16], channels: cpal::ChannelCount, source: &mut MixerSource, ring: &PcmRing) {
    let gain = gate_gain(GATE.load(Ordering::Relaxed));
    for frame in data.chunks_mut(channels as usize) {
        let (left, right) = next_stereo(source);
        match frame {
            [mono] => *mono = quantize((left + right) * 0.5 * gain),
            [l, r, rest @ ..] => {
                *l = quantize(left * gain);
                *r = quantize(right * gain);
                rest.fill(0);
            }
            [] => {}
        }
        ring.push(left, right);
    }
}

/// One stereo frame from the mix. A finished mix reads as silence.
fn next_stereo(source: &mut MixerSource) -> (f32, f32) {
    (
        source.next().unwrap_or(0.0),
        source.next().unwrap_or(0.0),
    )
}

/// The output a player holds: the shared tapped mixer, or rodio's own sink
/// when the tap could not open. Every player that can reach the tap must use
/// it, or the stream carries only part of the mix.
pub enum SharedOutput {
    Tap(AudioTap),
    Device(rodio::MixerDeviceSink),
}

impl SharedOutput {
    pub fn open() -> Option<Self> {
        open_tap().or_else(|| {
            rodio::DeviceSinkBuilder::open_default_sink()
                .map_err(|error| log::warn!("Default audio sink unavailable: {error}"))
                .ok()
                .map(Self::Device)
        })
    }

    pub fn mixer(&self) -> &Mixer {
        match self {
            Self::Tap(tap) => tap.mixer(),
            Self::Device(device) => device.mixer(),
        }
    }
}

#[cfg(not(test))]
fn open_tap() -> Option<SharedOutput> {
    match AudioTap::open() {
        Ok(tap) => {
            log::info!("Audio output is shared and tapped for the dashboard");
            Some(SharedOutput::Tap(tap))
        }
        Err(error) => {
            log::warn!("Shared audio output unavailable ({error}); using the default sink");
            None
        }
    }
}

/// Tests open and close devices on short-lived threads, where CPAL's cached
/// COM apartment can outlive them; they keep the plain sink.
#[cfg(test)]
fn open_tap() -> Option<SharedOutput> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_returns_frames_in_order_and_drops_when_full() {
        let ring = PcmRing::new();
        ring.push(0.5, -0.5);
        ring.push(-0.25, 0.25);
        assert_eq!(ring.queued(), 2);
        assert_eq!(ring.pop(), Some((16383, -16383)));
        assert_eq!(ring.pop(), Some((-8191, 8191)));
        assert_eq!(ring.pop(), None);
    }

    #[test]
    fn ring_keeps_the_frames_it_already_accepted_when_it_overflows() {
        let ring = PcmRing::new();
        for _ in 0..RING_FRAMES + 8 {
            ring.push(1.0, 1.0);
        }
        // A full ring rejects new frames rather than overwriting unread ones.
        assert_eq!(ring.queued(), RING_FRAMES);
        ring.clear();
        assert_eq!(ring.queued(), 0);
    }

    #[test]
    fn quantize_clamps_out_of_range_samples() {
        assert_eq!(quantize(2.0), i16::MAX);
        assert_eq!(quantize(-2.0), -i16::MAX);
        assert_eq!(quantize(0.0), 0);
    }

    #[test]
    fn speaker_is_silent_only_when_the_window_is_not_active() {
        assert_eq!(gate_gain(GATE_WINDOW_ACTIVE), 1.0);
        assert_eq!(gate_gain(0), 0.0);
    }

    #[test]
    fn a_listening_dashboard_mutes_the_speakers_until_local_audio_is_forced() {
        let listening = GATE_WINDOW_ACTIVE | GATE_BROWSER_LISTENING;
        assert_eq!(gate_gain(listening), 0.0);
        assert_eq!(gate_gain(listening | GATE_LOCAL_FORCED), 1.0);
        // Forcing local audio does not resurrect a deactivated window.
        assert_eq!(gate_gain(GATE_LOCAL_FORCED), 0.0);
        assert_eq!(gate_gain(GATE_BROWSER_LISTENING), 0.0);
    }

    #[test]
    fn gate_bits_toggle_independently() {
        // The gate is process-wide and other tests share it: put it back.
        let saved = GATE.load(Ordering::Relaxed);
        set_window_active(true);
        set_browser_listening(false);
        set_local_audio_forced(false);
        assert_eq!(gate_gain(GATE.load(Ordering::Relaxed)), 1.0);
        set_browser_listening(true);
        assert_eq!(gate_gain(GATE.load(Ordering::Relaxed)), 0.0);
        set_browser_listening(false);
        set_window_active(false);
        assert_eq!(gate_gain(GATE.load(Ordering::Relaxed)), 0.0);
        GATE.store(saved, Ordering::Relaxed);
    }
}
