//! Sound effect (SFX) playback using rodio.
//!
//! Plays short one-shot sounds triggered by game events: weapon fire, unit
//! voice responses, building placement, death explosions. Uses the SoundRegistry
//! (from the selected SOUNDMD.INI) to resolve sound IDs to .wav/.aud filenames, then loads
//! and plays them through rodio.
//!
//! ## Design
//! - Sample selection, pitch/volume shift, positional volume, stereo pan and
//!   the DirectSound loudness curve are reproduced from `gamemd.exe`
//!   (`VocClass`, `SoundEvent`, `DSoundBuffer`); see the provenance comments
//!   on each helper.
//! - A play request first enters [`arbiter::SoundArbiter`], which
//!   owns the native `SoundSystem::UpdateTick @ 0x004041D0` pass: the channel
//!   pool, `Priority=` arbitration, `Limit=`, the pre-delay wait, the looping
//!   leash and the volume ramps. Only an admitted channel draws its shifts;
//!   sample selection and decoding follow in the service pass. This file
//!   supplies that sample work and applies [`arbiter::ArbiterAction`]s to rodio.
//! - The app's shared audio service clock admits [`SfxPlayer::service_events`]
//!   regardless of whether the simulation stepped — native's
//!   `AudioSystem::Pump @ 0x00406F70` hangs off `Network_ServiceLoop @
//!   0x0048D080`, not the sim.
//!
//! ## Dependency rules
//! - Part of audio/ — depends on assets/ (AssetManager for .wav/.aud loading),
//!   rules/sound_ini (SoundRegistry for ID→filename mapping).
//! - Does NOT depend on render/, ui/, sim/.

use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZero;
use std::sync::{Arc, OnceLock};

use rodio::Player;
use rodio::source::EmptyCallback;
#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
use rodio::{DeviceSinkBuilder, MixerDeviceSink};

#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
type SfxOutput = MixerDeviceSink;
#[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
type SfxOutput = crate::audio::web_output::WebAudioSink;

use crate::assets::asset_manager::AssetManager;
use crate::assets::aud_file;
use crate::audio::arbiter::{
    self, ArbiterAction, EntryFacts, EventId, EventToken, HandleOwner, PlayRequest,
    PlaybackService, PlayoutEnd, SoundArbiter, VolumeInterp, combine_linear,
};
use crate::audio::events::SoundSource;
use crate::audio::voice_queue::VoiceQueue;
use crate::audio::vox::{VoxNode, VoxQueue, VoxRequest};
#[cfg(test)]
use crate::rules::sound_ini::EvaSide;
use crate::rules::sound_ini::{
    EvaPriority, EvaRegistry, EvaType, SoundEntry, SoundRegistry, VOLUME_SCALE, control, sound_type,
};
use crate::sim::snapshot::scenario_resources::SavedStaticSound;
use crate::util::native_x87::X87Chop53;

mod clip_cache;
mod observation;
mod playback;
mod sample_queue;
use clip_cache::ClipCache;
use playback::{PlaybackControl, PlaylistCursor, RingSource};
use sample_queue::SampleQueue;
#[cfg(all(test, tests_audio))]
mod ambient_playout_tests;
#[cfg(all(test, tests_audio))]
mod coordinate_playback_tests;
#[cfg(all(test, tests_audio))]
mod coordinate_sound_tests;
#[cfg(all(test, tests_audio))]
mod gattling_loop_tests;
#[cfg(all(test, tests_audio))]
mod house_cue_tests;
#[cfg(all(test, tests_audio))]
mod ring_resample_tests;
#[cfg(all(test, tests_audio))]
mod unit_voice_lifecycle_tests;
#[cfg(all(test, tests_audio))]
mod unit_voice_tests;
pub(crate) use observation::{PcmObservationConfig, PcmObservationContext, PcmObservationReport};
use observation::{PcmObserver, VoiceLatchState};

/// `VocClass::CalcVolumeAndPan @ 0x00750AC0` (`0x00750B0F..0x00750B17`):
/// `maxRange = Range * 0x3C` pixels.
const RANGE_MULTIPLIER: i32 = 0x3C;

/// Pan scale: `0..=0x4000` with `0x2000` centre (`0x00750D10..0x00750D24`,
/// constant `0x007F68E8` = 8192.0f).
pub const PAN_CENTRE: i32 = 0x2000;
pub const PAN_SCALE: i32 = 0x4000;

/// Audibility cutoff: volumes below the double at `0x007E8AE8` (0.05) are
/// silent (`0x00750CBD..0x00750CCC`).
const MIN_VOLUME_CUTOFF: f64 = 0.05;

/// Native integer absolute value: `CDQ; XOR EAX,EDX; SUB EAX,EDX`
/// (`0x00750BCF..0x00750BD2` and `0x00750BED..0x00750BF0`). The idiom **wraps**
/// — `i32::MIN` comes back as `i32::MIN` — where Rust's `i32::abs` panics in a
/// debug build, so the transcription must use the wrapping form.
fn native_abs(value: i32) -> i32 {
    value.wrapping_abs()
}

/// The listener: the tactical view rect and its top-left in the world-pixel
/// frame the sound positions use.
///
/// gamemd-derived: `CalcVolumeAndPan` reads the width/height globals
/// `0x00886FA8`/`0x00886FAC` (written by `Set_View_Dimensions`) and projects
/// the sound through `TacticalClass::CoordsToClient2 @ 0x006D2140`, which
/// scales leptons by the fixed native 60/30-pixel tile (`iVar3 = (x*0x3c)/2 +
/// (y*-0x3c)/2`, `>> 8`) and subtracts the view origin (`this+0xB0/+0xB4`).
/// That fixed scale is VERA's world-pixel frame — `map::terrain::TILE_WIDTH`
/// 60, `TILE_HEIGHT` 30. VERA adds `WORLD_ROW_BIAS_PX`15 to absolute Y
/// (`util::lepton`); its camera origin is in that same translated frame.
/// Native view(Vx,Vy) therefore maps to VERA origin(Vx,Vy+15). At zoom1,
/// subtracting matching origins preserves the native client point. Native
/// projection includes AdjustForZ and its startup-owned coefficient6D1BDD;
/// declared fixture coefficients must not substitute for that initializer.
///
/// **Zoom is VERA-internal; gamemd has no zoom.** `tactical_width`/`_height`
/// are the *device* pixels of the tactical viewport, and VERA's projection is
/// `device = (world - camera) * zoom`, so the viewport spans
/// `device / zoom` world pixels ([`SpatialListener::view_extent`]). Every
/// operand handed to [`calc_volume_and_pan`] is therefore expressed in the
/// world-pixel frame: the client point, the view extent, and `Range * 60`
/// alike. Scaling the *client point* into device pixels instead would be the
/// same algebra for the falloff shape and the pan, but it would silently
/// redefine `Range=` — a cell count in `sound(md).ini` — as a device-pixel
/// budget, so a `Range=10` cue would carry 2.5 cells at 4x zoom. Keeping the
/// world frame makes zoom behave like a native resolution change, which is
/// the only zoom-like thing gamemd actually does: a bigger view rect covers
/// more world, while `Range` stays a fixed cell distance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpatialListener {
    /// Tactical viewport width in device pixels (native `0x00886FA8`).
    pub tactical_width: i32,
    /// Tactical viewport height in device pixels (native `0x00886FAC`).
    pub tactical_height: i32,
    /// World-pixel position of the viewport's top-left corner.
    pub origin_x: f32,
    /// World-pixel position of the viewport's top-left corner.
    pub origin_y: f32,
    /// VERA-internal camera zoom (`app::input::camera`, 0.25..=4.0, default
    /// 1.0). `1.0` reproduces gamemd bit for bit.
    pub zoom: f32,
}

impl SpatialListener {
    /// Client (view-relative) pixel of a world-pixel position; the native
    /// client point is integer, so the fractional VERA camera is truncated.
    pub fn client_point(&self, screen_x: f32, screen_y: f32) -> (i32, i32) {
        (
            X87Chop53::ftol_f64_low_masked(f64::from(screen_x) - f64::from(self.origin_x)),
            X87Chop53::ftol_f64_low_masked(f64::from(screen_y) - f64::from(self.origin_y)),
        )
    }

    /// The tactical view size in the same world-pixel frame as
    /// [`Self::client_point`]. At `zoom == 1.0` this is the native
    /// `(float)[0x00886FA8]` / `(float)[0x00886FAC]` cast unchanged (dividing
    /// by exactly 1.0 is exact in IEEE-754).
    ///
    /// VERA-internal, gamemd equivalent UNCHECKED: the `max(EPSILON)` floor.
    /// `zoom_level` is clamped to `MIN_ZOOM` 0.5 in `app::input::camera`, so
    /// nothing in the app can reach it; it only stops a hand-built listener
    /// from dividing by zero. Same guard the visible-bounds code already uses
    /// (`presentation::instances::helpers`).
    pub fn view_extent(&self) -> (f32, f32) {
        let zoom = self.zoom.max(f32::EPSILON);
        (
            self.tactical_width as f32 / zoom,
            self.tactical_height as f32 / zoom,
        )
    }
}

/// The registry facts `CalcVolumeAndPan` reads from the event.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpatialSource {
    pub range_cells: i32,
    pub type_flags: u32,
    /// `MinVolume` as the stored fraction (`AudioEventClass+0x54`).
    pub min_volume: f32,
}

impl SpatialSource {
    pub fn from_entry(entry: &SoundEntry) -> Self {
        Self {
            range_cells: entry.range,
            type_flags: entry.type_flags,
            min_volume: entry.min_volume,
        }
    }

    /// The `[Defaults]` facts, for raw audio-bag names that have no event.
    /// Native never plays those positionally (an invalid `VocClass` index
    /// plays nothing); this is the VERA-internal fallback's listener model.
    pub fn from_registry_defaults(registry: &SoundRegistry) -> Self {
        let defaults = registry.defaults();
        Self {
            range_cells: defaults.range,
            type_flags: defaults.type_flags,
            min_volume: defaults.min_volume_fraction(),
        }
    }
}

/// Positional result: the spatial volume `0..=1` and the pan `0..=0x4000`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpatialGain {
    pub volume: f32,
    pub pan: i32,
}

impl SpatialGain {
    /// Non-positional playback: full volume, centred. This is what
    /// `TechnoClass` voices and UI cues use (volume 1.0, pan `0x2000`).
    pub const CENTRED_FULL: Self = Self {
        volume: 1.0,
        pan: PAN_CENTRE,
    };

    /// Native linear volume `min(ftol(volume * 0x4000), 0x4000)`
    /// (`VocClass::PlayAt @ 0x007509E0`, `0x00750A55..0x00750A6F`).
    pub fn volume_linear(&self) -> i32 {
        X87Chop53::ftol_f64_low_masked(f64::from(self.volume) * f64::from(VOLUME_SCALE))
            .min(VOLUME_SCALE)
    }
}

/// Positional volume and pan for one sound at one client point.
///
/// gamemd-derived: `VocClass::CalcVolumeAndPan @ 0x00750AC0`, transcribed
/// from `disassemble_function` — every rounding point below names its
/// instruction range.
/// - `halfW = W * 0.5f`, `halfH = H * 0.5f`, `fullW = halfW + halfW`
///   (`0x00750AD6..0x00750B06`), `maxRange = Range * 0x3C` as float.
/// - `Type=SHROUD` (`0x800`): a sound in a cell whose shroud flags
///   (`CellClass+0x12C & 0x18`) are both clear returns 0 (`0x00750B2D..
///   0x00750BAA`), as does the null cell `(0,0)` ahead of that lookup
///   (`0x00750B51..0x00750B73`). The cell test is the caller's: `shrouded`.
///   RESIDUAL: a cell outside the map reads native's shared Dummy cell
///   (`0x005657A0` returns `0x00ABDC50`); VERA treats it as never seen.
/// - `offsetX = clientX - halfW` kept as float (`FST [ESP+0x10]`); the
///   distances are `|ftol(clientX - halfW)|` and `|ftol(clientY - halfH)|`
///   (`0x00750BBE..0x00750BF6`), the abs taken by the wrapping `CDQ; XOR; SUB`
///   idiom ([`native_abs`]).
/// - Unless `Type=LOCAL` (`0x40`): subtract the half view from each and clamp
///   at 0 (`0x00750BFA..0x00750C37`). Then `distY *= 2` (`0x00750C3D`).
/// - `volume = (maxRange - max(distX, distY)) / maxRange` only when both are
///   below `maxRange` and `maxRange > 0`, else 0 (`0x00750C43..0x00750C99`).
///   The `max` is `distY` when `distX <= distY`.
/// - `Type=GLOBAL` (`0x10`): `volume = max(volume, MinVolume)`
///   (`0x00750C9B..0x00750CBD`).
/// - `volume < 0.05` (double compare) returns 0 with no pan (`0x00750CBD`).
/// - `pan = ftol(clamp(offsetX, -fullW, fullW) * 8192 / fullW + 8192)`
///   (`0x00750CDE..0x00750D24`); the `FCHS` builds `-fullW`, it does not
///   negate the offset.
///
/// `tactical_width`/`tactical_height` and `client_x`/`client_y` must be in the
/// same pixel frame — see [`SpatialListener::view_extent`], which is what puts
/// them there under VERA's zoom. Native passes the integer view globals here;
/// an integral `f32` reproduces the `(float)` cast exactly.
pub fn calc_volume_and_pan(
    client_x: i32,
    client_y: i32,
    tactical_width: f32,
    tactical_height: f32,
    source: SpatialSource,
    shrouded: bool,
) -> Option<SpatialGain> {
    let half_w: f32 = tactical_width * 0.5;
    let half_h: f32 = tactical_height * 0.5;
    let full_w: f32 = half_w + half_w;
    // `LEA EAX,[EAX+EAX*2]; LEA EAX,[EAX+EAX*4]; SHL EAX,2` at
    // `0x00750B0F..0x00750B17` — a 32-bit ×60 that wraps rather than trapping.
    // `AudioEventClass::SetRange @ 0x004065E0` stores `Range=` as a full int,
    // so `wrapping_mul` is the transcription of the native overflow, not a
    // guard: it is what keeps a `Range=` past 35_791_394 on the native
    // truncation instead of panicking in a debug build.
    let max_range: f32 = (source.range_cells.wrapping_mul(RANGE_MULTIPLIER)) as f32;

    if source.type_flags & sound_type::SHROUD != 0 && shrouded {
        return None;
    }

    let offset_x: f32 = (f64::from(client_x) - f64::from(half_w)) as f32;
    let mut dist_x: f32 = native_abs(X87Chop53::ftol_f64_low_masked(
        f64::from(client_x) - f64::from(half_w),
    )) as f32;
    let mut dist_y: f32 = native_abs(X87Chop53::ftol_f64_low_masked(
        f64::from(client_y) - f64::from(half_h),
    )) as f32;

    if source.type_flags & sound_type::LOCAL == 0 {
        dist_x -= half_w;
        dist_y -= half_h;
        if dist_x < 0.0 {
            dist_x = 0.0;
        }
        if dist_y < 0.0 {
            dist_y = 0.0;
        }
    }
    dist_y += dist_y;

    let mut volume: f32 = 0.0;
    if dist_x < max_range && dist_y < max_range && 0.0 < max_range {
        let dist = if dist_x <= dist_y { dist_y } else { dist_x };
        volume = (max_range - dist) / max_range;
    }
    if source.type_flags & sound_type::GLOBAL != 0 && volume < source.min_volume {
        volume = source.min_volume;
    }
    if f64::from(volume) < MIN_VOLUME_CUTOFF {
        return None;
    }

    let clamped = if offset_x < -full_w {
        -full_w
    } else if offset_x > full_w {
        full_w
    } else {
        offset_x
    };
    let pan = X87Chop53::ftol_f64_low_masked(
        f64::from(clamped) * f64::from(PAN_CENTRE) / f64::from(full_w) + f64::from(PAN_CENTRE),
    );
    Some(SpatialGain { volume, pan })
}

/// [`calc_volume_and_pan`] for a world-pixel position against a listener.
///
/// Both operands come out of the listener in the world-pixel frame, so the
/// falloff distance and the pan stay in one unit at every zoom.
pub fn spatial_gain(
    source: SpatialSource,
    screen_x: f32,
    screen_y: f32,
    listener: &SpatialListener,
    shrouded: bool,
) -> Option<SpatialGain> {
    let (client_x, client_y) = listener.client_point(screen_x, screen_y);
    let (view_w, view_h) = listener.view_extent();
    calc_volume_and_pan(client_x, client_y, view_w, view_h, source, shrouded)
}

/// DirectSound attenuation table, hundredths of a decibel, indexed by the
/// linear volume in percent. Machine-derived: `read_memory 0x00816380` (101
/// dwords, `-10000` at 0 through `0` at 100); the entries are
/// `round(1000 * log2(i / 100))`, i.e. 10 dB per halving, floored at -100 dB.
/// Applied by the `DSoundBuffer` apply routine `FUN_0040A6D0` as
/// `SetVolume(table[(volume >> 16) * 25 >> 12])` and as the per-side pan
/// attenuation.
const DSOUND_ATTENUATION_TABLE: [i16; 101] = [
    -10000, -6644, -5644, -5059, -4644, -4322, -4059, -3837, -3644, -3474, -3322, -3184, -3059,
    -2943, -2837, -2737, -2644, -2556, -2474, -2396, -2322, -2252, -2184, -2120, -2059, -2000,
    -1943, -1889, -1837, -1786, -1737, -1690, -1644, -1599, -1556, -1515, -1474, -1434, -1396,
    -1358, -1322, -1286, -1252, -1218, -1184, -1152, -1120, -1089, -1059, -1029, -1000, -971, -943,
    -916, -889, -862, -837, -811, -786, -761, -737, -713, -690, -667, -644, -621, -599, -578, -556,
    -535, -515, -494, -474, -454, -434, -415, -396, -377, -358, -340, -322, -304, -286, -269, -252,
    -234, -218, -201, -184, -168, -152, -136, -120, -105, -89, -74, -59, -44, -29, -14, 0,
];

/// Amplitude of one DirectSound attenuation (hundredths of a dB).
/// `DSBVOLUME_MIN` (-10000) is DirectSound's documented silence, so the
/// table's zero entry maps to exactly 0 rather than the -100 dB it names.
fn attenuation_amplitude(hundredths_db: i16) -> f32 {
    if hundredths_db <= DSOUND_ATTENUATION_TABLE[0] {
        return 0.0;
    }
    10f64.powf(f64::from(hundredths_db) / 2000.0) as f32
}

/// Amplitude the DirectSound layer produces for one combined linear volume:
/// `FUN_0040A6D0` indexes the table with `volume * 25 >> 12` (0..=100).
///
/// VERA-internal, gamemd equivalent UNCHECKED: the `.clamp`. `FUN_0040A6D0`
/// computes `(v >> 16) * 25 >> 12` and indexes `DAT_00816380` with it
/// unchecked, so an out-of-range volume reads past the 101-entry table in
/// gamemd; in Rust that is a panic, which is not a behaviour worth
/// reproducing. Trigger and frequency: as [`combine_linear`] — unreachable
/// from any VERA producer.
pub fn native_volume_amplitude(linear: i32) -> f32 {
    let index = (linear.clamp(0, VOLUME_SCALE) * 25) >> 12;
    attenuation_amplitude(DSOUND_ATTENUATION_TABLE[index as usize])
}

/// Left/right channel amplitudes for one pan (`0..=0x4000`).
///
/// gamemd-derived: `FUN_0040A6D0` (`0x0040A6F4..`) maps the combined pan to
/// `p = (pan * 25 >> 11) - 100` and calls `SetPan(table[100 - |p|])` for a
/// left pan (negative DirectSound pan attenuates the right channel) and
/// `SetPan(-table[100 - p])` for a right pan (attenuates the left channel).
///
/// VERA-internal, gamemd equivalent UNCHECKED: the `.clamp`, for the same
/// reason as [`native_volume_amplitude`] — native indexes the table
/// unchecked. `CalcVolumeAndPan` produces the pan as
/// `ftol(clamp(offsetX, -fullW, fullW) * 8192 / fullW + 8192)`, which is
/// already `0..=0x4000`, so nothing in VERA can reach the guard.
pub fn pan_channel_gains(pan: i32) -> (f32, f32) {
    let p = ((pan.clamp(0, PAN_SCALE) * 25) >> 11) - 100;
    let attenuated = attenuation_amplitude(DSOUND_ATTENUATION_TABLE[(100 - p.abs()) as usize]);
    if p < 0 {
        (1.0, attenuated)
    } else if p > 0 {
        (attenuated, 1.0)
    } else {
        (1.0, 1.0)
    }
}

/// The audio RNG contract: `Random::RandomRanged @ 0x0065C7E0` on the
/// non-scenario `g_MainRng @ 0x00886B88`, seeded from resolved g_RngSeed by
/// Init_Random_Number_System52FC20. Inclusive bounds; equal bounds return
/// without drawing. The app lends the one current process Main cursor.
pub trait SampleRng {
    fn ranged(&mut self, low: i32, high: i32) -> i32;
}

impl<F: FnMut(i32, i32) -> i32> SampleRng for F {
    fn ranged(&mut self, low: i32, high: i32) -> i32 {
        self(low, high)
    }
}

/// Lifetime-safe draw capability supplied by the app's process Main owner.
/// Clones call the same owner; audio never seeds, copies or reads its state.
#[derive(Clone)]
pub(crate) struct PlaybackDraws(Arc<dyn Fn(i32, i32) -> i32 + Send + Sync>);

impl PlaybackDraws {
    pub(crate) fn new(draw: impl Fn(i32, i32) -> i32 + Send + Sync + 'static) -> Self {
        Self(Arc::new(draw))
    }
}

impl SampleRng for PlaybackDraws {
    fn ranged(&mut self, low: i32, high: i32) -> i32 {
        (self.0)(low, high)
    }
}

/// Per-play randomised facts drawn before the samples are chosen.
///
/// gamemd-derived: `SoundEvent::UpdateState @ 0x004055C0` state 0
/// (`0x0040567F..0x004056A7`): `fshift = 100 + RandomRanged(FShift.min,
/// FShift.max)` and `vshift = RandomRanged(0, VShift)`; then, when
/// `Control & (PREDELAY|AMBIENT)`, `RandomRanged(AMBIENT ? 0x21 : Delay.min,
/// Delay.max)` for the pre-delay (`0x00405729..0x00405743`).
///
/// Native draws the pre-delay inside `UpdateState` state 0, *after* the
/// channel has been taken. The arbiter calls this helper at that boundary
/// through [`PlaybackService::channel_acquired`], and applies the pre-delay
/// it at native's place in the state machine — including the `0x21` ms floor
/// and the `Control & 0x88` gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayShifts {
    /// Frequency multiplier in percent (`SoundEvent+0x14C`).
    pub frequency_pct: i32,
    /// Volume reduction in percent (`SoundEvent+0x150`).
    pub volume_shift_pct: i32,
    /// The pre-delay draw in milliseconds; 0 when the entry authors neither
    /// `Control=predelay` nor `Control=ambient`.
    pub predelay_ms: i32,
}

impl PlayShifts {
    const UNITY: Self = Self {
        frequency_pct: 100,
        volume_shift_pct: 0,
        predelay_ms: 0,
    };

    pub fn draw(entry: &SoundEntry, rng: &mut impl SampleRng) -> Self {
        let frequency_pct = rng.ranged(entry.fshift.0, entry.fshift.1) + 100;
        let volume_shift_pct = rng.ranged(0, entry.vshift);
        let mut predelay_ms = 0;
        if entry.control & (control::PREDELAY | control::AMBIENT) != 0 {
            let min = if entry.control & control::AMBIENT != 0 {
                arbiter::PREDELAY_FLOOR_MS
            } else {
                entry.delay_ms.0
            };
            predelay_ms = rng.ranged(min, entry.delay_ms.1);
        }
        Self {
            frequency_pct,
            volume_shift_pct,
            predelay_ms,
        }
    }

    /// Buffer volume after the shift: `SoundEvent::StartPlayback @
    /// 0x004054A0` (`0x004054EB..0x00405519`), `0x4000 - ((vshift << 14) /
    /// 100)` when `vshift > 0`.
    pub fn volume_linear(&self) -> i32 {
        if self.volume_shift_pct > 0 {
            VOLUME_SCALE - ((self.volume_shift_pct << 14) / 100)
        } else {
            VOLUME_SCALE
        }
    }

    /// Playback rate: `FUN_00401190` returns `(pct * rate) / 100`
    /// (`SHR ECX,0x10; IMUL ECX,EDX;` then the `0x51EB851F` magic divide by
    /// 100, truncating toward zero).
    ///
    /// VERA-internal, gamemd equivalent UNCHECKED: the `.max(1)`. The native
    /// body has no floor — it hands the raw quotient to the DirectSound
    /// buffer's `SetFrequency`, which rejects 0 at the device layer. VERA
    /// instead feeds the rate to its own resampler, where 0 is not a
    /// well-defined input. Trigger: `frequency_pct <= 0`, i.e. a `FShift=`
    /// whose low bound is at or below -100; the widest stock `FShift=` is
    /// `-15 15`. Player effect: none reachable. Frequency: never on retail
    /// data. Downstream risk: none.
    pub fn shifted_sample_rate(&self, sample_rate: u32) -> u32 {
        ((i64::from(self.frequency_pct) * i64::from(sample_rate)) / 100).max(1) as u32
    }
}

/// Test-only finite inspection of the production one-buffer cursor.
///
/// gamemd-derived: `SoundEvent::LoadSamples @ 0x004048B0` for events whose
/// `Delay.min < 0x21` (`0x004048FB..0x00404ACD`); the above-floor loader
/// is `LoadedPlayback::load_delayed` (`sfx/sample_queue.rs`):
/// 1. `Attack > 0`: load `samples[RandomRanged(0, Attack - 1)]`.
/// 2. Without `ALL`: `RANDOM` loads `samples[RandomRanged(Attack, count -
///    Decay - 1)]`, otherwise `samples[Attack]` (the first body sample —
///    never a round-robin). With `ALL`: every body sample in order.
/// 3. `Decay > 0`: load `samples[RandomRanged(count - Decay, count - 1)]`.
///
/// Then `PreparePlayout @ 0x00404700` / `AdvancePlaylist @ 0x004047B0` play
/// the loaded buffers: the attack buffer first when `Control=ATTACK`, the
/// decay buffer after repetition ends when `Control=DECAY`, and the rest in `RandomRanged(0,
/// remaining - 1)` pick-and-remove order for `RANDOM` or in load order
/// otherwise.
///
/// **VERA-internal, gamemd equivalent UNCHECKED: every bounds guard below.**
/// Native indexes the fixed 32-slot sample array at `AudioEventClass+0xB4`
/// with no check at all, and `AudioEventClass::SetControlFlags @ 0x00406570`
/// (read this session) normalises the attack/decay counts against the
/// `Control=` flags *only* — never against how many names `Sounds=` actually
/// listed. So `Attack=5` on a two-sample entry, or `Sounds=` omitted
/// entirely, reads past the loaded pointers in gamemd; the observable result
/// is whatever that garbage pointer does, which VERA cannot reproduce and
/// must not pretend to. Trigger: only a hand-edited `sound(md).ini` whose
/// `Attack=`/`Decay=` exceed its own `Sounds=` list — no stock entry does.
/// Player effect: VERA plays a valid sample (or nothing) where gamemd would
/// misbehave. Frequency: never on retail data. Downstream risk: none.
#[cfg(test)]
pub fn select_playout(entry: &SoundEntry, rng: &mut impl SampleRng) -> Vec<usize> {
    select_playout_pass(entry, rng, true)
}

/// [`select_playout`] for one pass, with the `Control=attack` head under the
/// caller's control.
///
/// `SoundEvent::PreparePlayout @ 0x00404700` heads the playout with the
/// attack buffer only while `flags & 8` is clear — the flag
/// `SoundEvent::StartPlayback @ 0x004054A0` and
/// `SoundEvent::MarkStarted @ 0x004052E0` both set. So the attack sample is
/// played on a cue's *first* pass only, never on a loop restart, and never at
/// all on a750D40 reallocation, which invokes SkipAttack immediately.
/// An initial owner-bound PlayAt7509E0 does play attack.
///
/// The attack **index is still drawn** when it is not played: the draw lives
/// in `SoundEvent::LoadSamples @ 0x004048B0`, which runs before the decision,
/// and it still reserves `samples[0]` out of the body range either way.
#[cfg(test)]
pub fn select_playout_pass(
    entry: &SoundEntry,
    rng: &mut impl SampleRng,
    plays_attack: bool,
) -> Vec<usize> {
    LoadedSampleIndices::load(entry, rng).playout_order(entry.control, rng, plays_attack)
}

/// The low-delay LoadSamples4048B0 selection survives every playout pass.
/// Re-entering PreparePlayout404700 never picks new sample handles or shifts.
#[derive(Default, Clone)]
struct LoadedSampleIndices {
    attack: Option<usize>,
    middle: Vec<usize>,
    decay: Option<usize>,
}

impl LoadedSampleIndices {
    fn load(entry: &SoundEntry, rng: &mut impl SampleRng) -> Self {
        let count = entry.sounds.len() as i32;
        if count == 0 {
            return LoadedSampleIndices::default();
        }
        let body = entry.body_range();
        let (body_start, body_end) = (body.start as i32, body.end as i32);
        let mut attack = None;
        let mut decay = None;
        let mut middle: Vec<usize> = Vec::new();

        if entry.attack > 0 {
            // `.clamp`: VERA-internal, see the note above.
            attack = Some(rng.ranged(0, entry.attack - 1).clamp(0, count - 1) as usize);
        }
        if entry.control & control::ALL == 0 {
            let index = if entry.control & control::RANDOM != 0 {
                rng.ranged(body_start, body_end - 1)
            } else {
                body_start
            };
            // `.contains`: VERA-internal, see the note above. An empty body range
            // (`Attack + Decay >= count`) puts `body_start` at `count`.
            if (0..count).contains(&index) {
                middle.push(index as usize);
            }
        } else {
            middle.extend(body.clone());
        }
        if entry.decay > 0 {
            // `.clamp`: VERA-internal, see the note above.
            decay = Some(
                rng.ranged(count - entry.decay, count - 1)
                    .clamp(0, count - 1) as usize,
            );
        }

        Self {
            attack,
            middle,
            decay,
        }
    }

    #[cfg(test)]
    fn playout_order(
        &self,
        control_flags: u32,
        rng: &mut impl SampleRng,
        plays_attack: bool,
    ) -> Vec<usize> {
        // Test-only finite inspection uses the production one-buffer cursor.
        // No alternative eager selection implementation lives in playback.
        let mut cursor = PlaylistCursor::new(self.clone(), control_flags, (0, 0), 1);
        let mut order = Vec::new();
        let mut selected = cursor.prepare(rng, plays_attack);
        while let Some(index) = selected {
            order.push(index);
            selected = cursor.advance(rng);
        }
        order
    }
}

/// Decoded audio ready for rodio playback.
/// Holds interleaved f32 stereo samples, sample rate, and channel count.
/// A clone shares the samples, so every start of a clip the [`ClipCache`]
/// keeps plays one decode.
#[derive(Clone)]
pub(crate) struct DecodedAudio {
    /// Interleaved stereo f32 samples (L, R, L, R, ...).
    pub(crate) samples: Arc<[f32]>,
    pub(crate) sample_rate: u32,
    /// Always 2 (stereo) — we upmix mono sources for consistency.
    pub(crate) channels: u16,
    /// The native format's channel count and bytes per sample before mono is
    /// upmixed for rodio: descriptor `+0xC` and `+0x10`, filled by
    /// `WAV__ParseHeader @ 0x00408610` (`0x0040876A..0x00408792`) and
    /// `AudioIndex__GetFormat @ 0x00401640`. IMA ADPCM decodes to 2 bytes.
    pub(crate) native_channels: u16,
    pub(crate) native_sample_bytes: u16,
}

impl DecodedAudio {
    /// Append another clip; the native playlist chains buffers back to back,
    /// each in the first clip's format (see [`playback::chained_samples`]).
    #[cfg(test)]
    fn append(&mut self, other: DecodedAudio) {
        let layout = (self.native_channels, self.native_sample_bytes);
        let chained = playback::chained_samples(&other, layout);
        let mut samples = self.samples.to_vec();
        samples.extend_from_slice(chained.as_deref().unwrap_or(&other.samples));
        self.samples = samples.into();
    }
}

impl Drop for SfxPlayer {
    fn drop(&mut self) {
        // Player::stop/Drop is asynchronous. Retained callback capabilities
        // must be cancelled at the owner's lifetime boundary as well.
        self.stop_all();
    }
}

/// One decoded playout, prepared only after channel admission. Event gain
/// remains with the arbiter; channel-local shifts remain with PendingPlayback.
struct ResolvedPlayback {
    decoded: DecodedAudio,
    /// Diagnostic identity of successfully decoded samples, collected only
    /// when this entry was explicitly requested by a bounded PCM observer.
    observed_samples: Option<Vec<String>>,
}

/// Which native volume groups one secondary output is chained to.
///
/// gamemd-derived: `OptionsClass::SetDefaults @ 0x005FA350` and
/// `OptionsClass__ReadFromINI @ 0x005FA620` retain independent SoundVolume and
/// VoiceVolume settings. All ordinary Voc events, including Techno voices,
/// bind channel+94 to SoundVolume `[0x0087E748]` at `0x00405B91..0x00405B96`.
/// The Techno caller passes unity (`0x006F9EE5..0x006F9EF0`) to `750920`,
/// which creates that ordinary event at `0x007509A3 -> 0x00405190`.
/// `SetSoundVolume` updates its group at `0x005FA53E..0x005FA546`;
/// `SetVoiceVolume` instead updates the EVA group at `0x005FA5D3..0x005FA5DB`.
///
/// A Voc channel (`Sound`) is also chained to the many-sounds scaler
/// (`ch+0x98`, `0x0087E1B8`) and the audio master (`ch+0x9C`, the group
/// `[0x0087E758]` stored at `SoundEvent::UpdateState 0x0040571E` and
/// `0x00405B9C`). The EVA `StreamPlayer` is not: `VoxClass::Init @
/// 0x00752300` binds it to the Voice group (`[0x0087E740]`, `0x00752316`)
/// and the group `[0x0087E750]` (`0x00752327`) only, and the master's readers
/// (`0x0040571E`, `0x00405B9C`, the all-group pump loops at `0x00409770`,
/// `0x0040A9C0`, `0x0040AA20`, the score dialog and the scenario exits) never
/// hand it to the stream. The scenario exits fade that master to zero
/// (`0x00686605`), so an announcement they queue (`0x00686616`) sounds at
/// full volume while every Voc channel fades.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SfxChannel {
    Sound,
    /// The EVA stream: the Voice group `+0x94` and `[0x0087E750]` at `+0x9C`.
    EvaStream,
    /// The taunt stream `[0x00B1D4D8]`: SpeechSystem::Init (`0x00752AD0`)
    /// links none of its channel's groups (the constructor `0x00407860`
    /// zeroes `+0x90..+0x9C`, and its `0x00407B40` / `0x00407B50` calls at
    /// `0x00752B07..0x00752B1E` rebind the EVA stream `[0x00B1D4CC]`), so
    /// neither master nor the exit fade reaches it; CombineInterps skips the
    /// null groups (`0x00402252..0x00402292`).
    TauntStream,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct SfxOutputScales {
    sound_volume: f32,
    voice_volume: f32,
    /// The effects master `[0x0087E758]` (`ch+0x9C` of every Voc channel):
    /// the movie holds and scenario exits write it.
    sound_master_linear: i32,
    /// `[0x0087E750]` (`ch+0x9C` of the EVA stream): only the movie holds
    /// write it.
    eva_master_linear: i32,
    focus_output_scale: f32,
    /// The many-sounds master scaler, `g_ManySoundsVolumeGroup @ 0x0087E1B8`,
    /// chained into every channel at `ch+0x98` and multiplied in by
    /// `DSoundBuffer::CombineInterps FUN_004010C0` as `(a * b) >> 14`.
    many_sounds_linear: i32,
}

/// Master-independent gain retained beside one live secondary output.
///
/// `base_linear` is the native linear volume (`0..=0x4000`) of everything
/// below the user volume: spatial volume and entry volume (the event group,
/// `ch+0x90`). The separate buffer interp (`ch+0x10`) holds the per-play
/// `VShift=` reduction. `DSoundBuffer::CombineInterps @ 0x00402220`
/// multiplies `+0x10`, `+0x90`, `+0x94`, `+0x98` and `+0x9C` in that order
/// (`FUN_004010C0`, `(a * b) >> 14`) and only then `FUN_0040A6D0` converts to
/// decibels, so `effective` recomposes the product each time rather than
/// scaling an amplitude. The VERA-internal foreground gate multiplies the
/// amplitude afterwards (primary-buffer stop, see `set_focus_output_active`).
#[derive(Debug, Clone, Copy, PartialEq)]
struct SfxOutputGain {
    base_linear: i32,
    /// The channel's own `+0x10` interp. For Voc it holds the per-play
    /// VShift (`0x004054A0`), which never changes the event gain used by
    /// Limit/priority in4041D0; for the EVA and taunt streams it is the
    /// stream's own volume interp (see `SfxPlayer::eva_stream_volume`).
    buffer_linear: i32,
    channel: SfxChannel,
}

impl SfxOutputGain {
    fn new(base_linear: i32, channel: SfxChannel) -> Self {
        Self {
            base_linear,
            buffer_linear: VOLUME_SCALE,
            channel,
        }
    }

    fn effective(self, scales: SfxOutputScales) -> f32 {
        native_volume_amplitude(self.combined_linear(scales)) * scales.focus_output_scale
    }

    /// The linear product `0x0040A6D0` converts: for a Voc channel, the
    /// value of `ch+0xD8` that `FindAvailable` ranks (`[ch+0xDC] >> 16`).
    fn combined_linear(self, scales: SfxOutputScales) -> i32 {
        let user = match self.channel {
            SfxChannel::Sound => scales.sound_volume,
            SfxChannel::EvaStream => scales.voice_volume,
            SfxChannel::TauntStream => return combine_linear(self.buffer_linear, self.base_linear),
        };
        let user_linear = X87Chop53::ftol_f64_low_masked(
            f64::from(user.clamp(0.0, 1.0)) * f64::from(VOLUME_SCALE),
        );
        if self.channel == SfxChannel::EvaStream {
            // VoxClass::Init: Voice `[0x0087E740]` at `+0x94` (`0x00752316`)
            // and `[0x0087E750]` at `+0x9C` (`0x00752327`); no many-sounds
            // scaler and no effects master, so the scenario exits' fade
            // (`0x00686605`) leaves an announcement they queue (`0x00686616`)
            // at full volume.
            let own = combine_linear(self.buffer_linear, self.base_linear);
            combine_linear(combine_linear(own, user_linear), scales.eva_master_linear)
        } else {
            // Voc: Sound `[0x0087E748]` at `+0x94` (`0x00405B91..0x00405B96`),
            // many-sounds at `+0x98` (`0x0040570E`), the effects master
            // `[0x0087E758]` at `+0x9C` (`0x0040571E`, `0x00405B9C`).
            let event = combine_linear(self.buffer_linear, self.base_linear);
            let with_user = combine_linear(event, user_linear);
            let with_limiter = combine_linear(with_user, scales.many_sounds_linear);
            combine_linear(with_limiter, scales.sound_master_linear)
        }
    }
}

/// Device-independent construction shared by tests and the rodio startup path.
/// `initial_volume` is the exact volume applied before the decoded source is
/// appended; `gain` remains master-independent for later live recomposition.
struct PreparedSfxOutput {
    decoded: DecodedAudio,
    gain: SfxOutputGain,
    initial_volume: f32,
}

impl PreparedSfxOutput {
    fn new(decoded: DecodedAudio, gain: SfxOutputGain, scales: SfxOutputScales) -> Self {
        Self {
            decoded,
            gain,
            initial_volume: gain.effective(scales),
        }
    }
}

#[cfg(test)]
fn prepare_normal_sfx_output(
    decoded: DecodedAudio,
    base_linear: i32,
    scales: SfxOutputScales,
) -> PreparedSfxOutput {
    PreparedSfxOutput::new(
        decoded,
        SfxOutputGain::new(base_linear, SfxChannel::Sound),
        scales,
    )
}

#[cfg(test)]
fn prepare_eva_stream_output(
    decoded: DecodedAudio,
    base_linear: i32,
    scales: SfxOutputScales,
) -> PreparedSfxOutput {
    PreparedSfxOutput::new(
        decoded,
        SfxOutputGain::new(base_linear, SfxChannel::EvaStream),
        scales,
    )
}

struct LiveSfxOutput {
    player: Player,
    gain: SfxOutputGain,
    token: Option<EventToken>,
    control: Option<PlaybackControl>,
    /// The event's own sample state while this playout runs. OnChannelDone
    /// may give the event another start pass.
    parked: Option<PendingPlayback>,
}

impl LiveSfxOutput {
    fn new(player: Player, gain: SfxOutputGain, initial_volume: f32) -> Self {
        player.set_volume(initial_volume);
        Self {
            player,
            gain,
            token: None,
            control: None,
            parked: None,
        }
    }

    fn apply_scales(&self, scales: SfxOutputScales) {
        self.player.set_volume(self.gain.effective(scales));
    }

    /// One output cancellation owner. The boundary gate closes future Main
    /// draws synchronously, before rodio retires its queued source.
    fn stop(&self) {
        if let Some(control) = &self.control {
            control.cancel();
        }
        self.player.stop();
    }
}

/// One admitted event. Submission carries identity/gain only; shifts and
/// samples are filled at their original service phases by PendingSampleService.
/// The record outlives each playout (parked in its live output): the shifts,
/// buffers, cursor and queue belong to the event, not to one start pass.
struct PendingPlayback {
    token: EventToken,
    key: String,
    channel: SfxChannel,
    shifts: Option<PlayShifts>,
    loaded: Option<LoadedPlayback>,
    prepared: Option<ResolvedPlayback>,
    cursor: Option<PlaylistCursor>,
    initial: Option<usize>,
    source: Option<(RingSource, PlaybackControl)>,
    queue: SampleQueue,
}

impl PendingPlayback {
    fn new(token: EventToken, key: String, channel: SfxChannel) -> Self {
        Self {
            token,
            key,
            channel,
            shifts: None,
            loaded: None,
            prepared: None,
            cursor: None,
            initial: None,
            source: None,
            queue: SampleQueue::default(),
        }
    }

    /// LoadSamples4048B0 for a registered entry. Below the floor the first
    /// buffers stay for every later pass; from the floor up each call
    /// releases them and loads the queue's next sample (`sfx/sample_queue.rs`)
    /// on a fresh cursor that keeps flags0x10.
    fn load_entry_samples(
        &mut self,
        entry: &SoundEntry,
        plays_attack: bool,
        rng: &mut impl SampleRng,
        load: impl FnMut(&str) -> Option<DecodedAudio>,
    ) -> bool {
        if arbiter::delay_below_floor(entry.delay_ms.0) {
            if self.loaded.is_none() {
                self.loaded = LoadedPlayback::load(entry, rng, load);
            }
            return self.loaded.is_some();
        }
        let mut decay_used = self.cursor.take().is_some_and(|cursor| cursor.decay_used());
        self.loaded = LoadedPlayback::load_delayed(
            entry,
            &mut self.queue,
            plays_attack,
            &mut decay_used,
            rng,
            load,
        );
        let selected = self
            .loaded
            .as_ref()
            .map(|loaded| loaded.selected.clone())
            .unwrap_or_default();
        self.cursor = Some(
            PlaylistCursor::new(selected, entry.control, entry.delay_ms, entry.loop_count)
                .with_queue(self.queue.count(), decay_used),
        );
        self.loaded.is_some()
    }

    /// OnChannelDone405A00 kept the event: its buffers and cursor (with
    /// flags0x10) come back from the spent playout for the next start pass.
    fn resume_after(&mut self, loaded: LoadedPlayback, cursor: PlaylistCursor) {
        self.loaded = Some(loaded);
        self.cursor = Some(cursor);
    }

    /// PreparePlayout404700 on the cursor the event keeps.
    fn prepare_entry_playout(
        &mut self,
        entry: &SoundEntry,
        plays_attack: bool,
        rng: &mut impl SampleRng,
    ) -> bool {
        let Some(loaded) = &self.loaded else {
            return false;
        };
        let cursor = self.cursor.get_or_insert_with(|| {
            PlaylistCursor::new(
                loaded.selected.clone(),
                entry.control,
                entry.delay_ms,
                entry.loop_count,
            )
        });
        self.initial = cursor.prepare(rng, plays_attack);
        self.initial
            .is_some_and(|index| loaded.clips.contains_key(&index))
    }
}

/// Decoded sample handles selected by LoadSamples4048B0: once below the
/// floor, where a loop reuses them and only the playlist order may draw
/// again, and once per start pass from the floor up.
#[derive(Default)]
struct LoadedPlayback {
    selected: LoadedSampleIndices,
    clips: BTreeMap<usize, DecodedAudio>,
}

impl LoadedPlayback {
    fn load(
        entry: &SoundEntry,
        rng: &mut impl SampleRng,
        mut load: impl FnMut(&str) -> Option<DecodedAudio>,
    ) -> Option<Self> {
        if entry.sounds.is_empty() {
            return None;
        }
        let requested = LoadedSampleIndices::load(entry, rng);
        let mut clips = BTreeMap::new();
        let mut loaded_indices = Vec::new();
        for index in requested
            .attack
            .into_iter()
            .chain(requested.middle.iter().copied())
            .chain(requested.decay)
        {
            if let Some(clip) = entry.sounds.get(index).and_then(|name| load(name)) {
                clips.insert(index, clip);
                loaded_indices.push(index);
            }
        }
        // Original4048B0 retains successful sample handles compactly. The
        // Prepare404700 first/last exclusions address that loaded table,
        // not the missing names' original Sounds positions.
        let attack = (entry.control & control::ATTACK != 0)
            .then(|| loaded_indices.first().copied())
            .flatten();
        let decay = (entry.control & control::DECAY != 0)
            .then(|| loaded_indices.last().copied())
            .flatten();
        let first = usize::from(attack.is_some());
        let end = loaded_indices
            .len()
            .saturating_sub(usize::from(decay.is_some()));
        let middle = if first <= end {
            loaded_indices[first..end].to_vec()
        } else {
            Vec::new()
        };
        let selected = LoadedSampleIndices {
            attack,
            middle,
            decay,
        };
        (!clips.is_empty()).then_some(Self { selected, clips })
    }

    #[cfg(test)]
    fn prepare(
        &self,
        entry: &SoundEntry,
        rng: &mut impl SampleRng,
        shifts: PlayShifts,
        plays_attack: bool,
        observe_samples: bool,
    ) -> Option<ResolvedPlayback> {
        let order = self
            .selected
            .playout_order(entry.control, rng, plays_attack);
        let mut decoded: Option<DecodedAudio> = None;
        let mut observed_samples = observe_samples.then(Vec::new);
        for index in order {
            let Some(clip) = self.clips.get(&index) else {
                continue;
            };
            if let Some(names) = &mut observed_samples {
                names.push(entry.sounds[index].clone());
            }
            match decoded.as_mut() {
                Some(chain) => chain.append(clip.clone()),
                None => decoded = Some(clip.clone()),
            }
        }
        let mut decoded = decoded?;
        decoded.sample_rate = shifts.shifted_sample_rate(decoded.sample_rate);
        Some(ResolvedPlayback {
            decoded,
            observed_samples,
        })
    }
}

/// Borrowed service capability; it has no RNG cursor, event pool or device.
struct PendingSampleService<'a> {
    pending: &'a mut BTreeMap<EventId, PendingPlayback>,
    live: &'a BTreeMap<EventId, LiveSfxOutput>,
    /// The scales the live outputs last applied.
    scales: SfxOutputScales,
    registry: &'a SoundRegistry,
    assets: &'a AssetManager,
    audio_index: Option<&'a crate::assets::audio_bag::AudioIndex>,
    clips: &'a mut ClipCache,
    rng: &'a mut PlaybackDraws,
    observer: Option<&'a mut PcmObserver>,
}

impl PlaybackService for PendingSampleService<'_> {
    /// An allocation runs before this pass starts anything, so every playing
    /// channel already has its output; one without reads as silent.
    fn channel_volume(&self, event: EventId) -> i32 {
        self.live
            .get(&event)
            .map_or(0, |output| output.gain.combined_linear(self.scales))
    }

    fn stop_playout(&mut self, event: EventId) {
        if let Some(output) = self.live.get(&event) {
            output.stop();
        }
        if let Some((_, control)) = self
            .pending
            .get(&event)
            .and_then(|pending| pending.source.as_ref())
        {
            control.cancel();
        }
    }

    fn channel_acquired(&mut self, event: EventId) -> i32 {
        let Some(pending) = self.pending.get_mut(&event) else {
            return 0;
        };
        let shifts = self
            .registry
            .get(&pending.key)
            .map_or(PlayShifts::UNITY, |entry| PlayShifts::draw(entry, self.rng));
        pending.shifts = Some(shifts);
        shifts.predelay_ms
    }

    fn load_samples(&mut self, event: EventId, plays_attack: bool) -> bool {
        let Some(pending) = self.pending.get_mut(&event) else {
            return false;
        };
        if pending.prepared.is_some() {
            return true;
        }
        let Some(entry) = self.registry.get(&pending.key) else {
            pending.prepared = self
                .clips
                .load(&pending.key, self.assets, self.audio_index)
                .map(|decoded| ResolvedPlayback {
                    decoded,
                    observed_samples: self
                        .observer
                        .as_ref()
                        .is_some_and(|observer| observer.contains(pending.token))
                        .then(|| vec![pending.key.clone()]),
                });
            return pending.prepared.is_some();
        };
        pending.load_entry_samples(entry, plays_attack, self.rng, |name| {
            self.clips.load(name, self.assets, self.audio_index)
        })
    }

    fn prepare_playout(&mut self, event: EventId, plays_attack: bool) -> bool {
        let Some(pending) = self.pending.get_mut(&event) else {
            return false;
        };
        // The labelled raw fallback is a single buffer, outside registered
        // playlist semantics. Ordinary Voc entries share the cursor below.
        if pending.prepared.is_some() {
            return true;
        }
        let Some(entry) = self.registry.get(&pending.key) else {
            return false;
        };
        pending.prepare_entry_playout(entry, plays_attack, self.rng)
    }

    fn start_playout(&mut self, event: EventId, pan: i32) -> bool {
        let Some(pending) = self.pending.get_mut(&event) else {
            return false;
        };
        if pending.prepared.is_some() {
            return true;
        }
        let (Some(loaded), Some(cursor), Some(initial)) = (
            pending.loaded.take(),
            pending.cursor.take(),
            pending.initial,
        ) else {
            return false;
        };
        let shifts = pending.shifts.expect("admitted channel");
        let observed_names = self
            .observer
            .as_ref()
            .filter(|observer| observer.contains(pending.token))
            .and_then(|_| self.registry.get(&pending.key))
            .map(|entry| entry.sounds.as_slice());
        pending.source = RingSource::new(
            loaded,
            cursor,
            initial,
            self.rng.clone(),
            shifts,
            pan,
            observed_names,
        );
        if let Some((source, _)) = &pending.source
            && let Some(observation) = source.observation()
            && let Some(observer) = &mut self.observer
        {
            observer.playback(pending.token, observation);
        }
        pending.source.is_some()
    }
}

/// Shared ordinary Voc pool for effects and per-Techno acknowledgements,
/// plus the separate EVA streaming output.
pub struct SfxPlayer {
    /// rodio mixer device sink — must be kept alive or all audio stops.
    _device: SfxOutput,
    /// Set by the stream's error callback when the device goes away.
    #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
    health: crate::audio::output_health::OutputHealth,
    /// The decision half: channel pool, `Priority=`, `Limit=`, pre-delay,
    /// looping leash, ramps and the many-sounds limiter.
    arbiter: SoundArbiter,
    /// Voc750E20's independent 200-node pool, in native admission order.
    /// Coordinates survive source-object Stop/destruction. SoundArbiter owns
    /// the associated Voc handles and events; this list owns their lifecycle.
    coordinate_requests: Vec<CoordinateSoundRequest>,
    next_coordinate_request: u64,
    /// Decoded payloads the arbiter has not started yet.
    pending: BTreeMap<EventId, PendingPlayback>,
    /// Started outputs, keyed by the arbiter event holding the channel.
    live: BTreeMap<EventId, LiveSfxOutput>,
    /// The decoded samples every start and announcement loads through.
    clips: ClipCache,
    /// Presentation diagnostics only; absent in ordinary play. It never
    /// supplies an arbiter, sample-selection or simulation decision.
    pcm_observer: Option<PcmObserver>,
    /// Last service-pass timestamp handed in by the app.
    now_ms: u64,
    /// The EVA announcement queue (`VoxClass`), including the pause depth
    /// `DAT_00b1d428` and the suspend depth `DAT_00b1d3d8`.
    vox: VoxQueue,
    /// The announcement stream — native's `StreamPlayer` (`PlayFile @
    /// 0x0075295C`), a reserved DirectSound stream outside the13 ordinary Voc
    /// channels that unit lines take through `VocClass::PlayAtPos @
    /// 0x00750920`. Nothing but the queue itself (`StreamPlayer::Stop` from
    /// `QueueVoice`, `ResetAll`) stops it, so an acknowledgement or effect
    /// never cuts an announcement. `Some` from the line's start until
    /// [`Self::advance_voice_queue`] observes it finished and reports the end
    /// time to `vox` (`StreamPlayer::GetEndTime` stand-in).
    eva_player: Option<LiveSfxOutput>,
    /// Set on the mixer thread the moment the current EVA sample runs out,
    /// as the channel's done callback `0x00407A30` stamps the stream's end
    /// time `+0x20` on the audio thread rather than at the next pump.
    eva_ran_out: Option<Arc<OnceLock<crate::util::time::Instant>>>,
    /// The taunt stream `[0x00B1D4D8]` ("Taunt Stream", `0x00752AD0`): a
    /// second StreamPlayer beside EVA's. `PlayFile` stops what it plays, so
    /// a new taunt cuts the last one.
    taunt_player: Option<LiveSfxOutput>,
    /// The EVA and taunt streams' own `+0x10` interps, as Theme's in
    /// `MusicPlayer`: each `StreamPlayer__PlayFile` snaps one to full
    /// (`0x00407E7D`), the radar-movie pre-fade glides both to zero
    /// (`0x00408390`) and every hold release glides them back
    /// (`0x004083D0`). They outlive the lines they scale.
    eva_stream_volume: VolumeInterp,
    taunt_stream_volume: VolumeInterp,
    /// The per-object voice latch: `TechnoClass::Queue_Voice @ 0x00708D90`
    /// writes it, `TechnoClass::AI_Update @ 0x006F9EBB` drains it.
    voice_queue: VoiceQueue,
    /// Ordinary effects and unit acknowledgement master volume (0.0 to 1.0).
    sound_volume: f64,
    /// EVA announcement master volume (0.0 to 1.0).
    voice_volume: f64,
    /// Current `[0x0087E758]` / `[0x0087E750]` linears; see [`Self::set_master_groups`].
    sound_master_linear: i32,
    eva_master_linear: i32,
    /// Foreground-owned primary-output gate. Secondary Players stay running so
    /// their playback cursors continue while global output is suppressed.
    focus_output_scale: f32,
    /// Whether the game is paused, so [`Self::set_paused`] only acts on the
    /// edge the way `GamePause::Enter`/`Exit` do.
    paused: bool,
    movie_held: bool,
    movie_sounds_suspended: bool,
    /// `-X…Q` (`0x00A8ED64`), set once at startup: `PlayNextQueued
    /// @ 0x00752760` returns before its dequeue. Theme reads the same switch
    /// through `ThemeGates`.
    eva_quiet: bool,
    ///7518C0 gates Voc admission; it neither pauses Theme nor ends buffers.
    voc_enabled: bool,
    ///53BAD0 excludes the SFX half of GamePause, not its EVA stream half.
    chrono_screen_active: bool,
    /// Whether the open game pause suspended the sounds: `GamePause::Enter
    /// @ 0x00406F00` skips them while ChronoScreen runs (`0x0053BAD0`), and
    /// `Exit` resumes them whatever the effect does by then.
    game_pause_suspends_sounds: bool,
}

/// Native750E20 copies +20/+24/+28 and initializes nested suspend depth+30.
/// `coordinate`/`flags` are immutable signed native XYZ leptons and +2C.
/// `source` is their derived projection in VERA's world-pixel/cell frame.
/// The gain cache is derived by the app's existing spatial context before
/// every common pump and is applied only in the admitted750EC0 phase.
struct CoordinateSoundRequest {
    id: u64,
    coordinate: [i32; 3],
    flags: u32,
    source: SoundSource,
    gain: Option<SpatialGain>,
    suspend_depth: i32,
}

const MAX_COORDINATE_REQUESTS: usize = 200;

/// Device period the SFX sink asks for, about 10 ms of frames.
///
/// rodio 0.22 opens a fixed ~50 ms buffer by default (`DeviceSinkBuilder::
/// from_device`), so the mixer picks up a new Player only once per period.
/// The arbiter pumps every 33 ms (`AudioSystem::Pump @ 0x00406F70`) and a
/// `Limit=1` `Control=interrupt` cue resubmitted every frame stops its
/// previous instance there — so with the default period most `CreditTicks`
/// cues (`CreditUp`/`CreditDown`, ~24 ms samples) were stopped before the
/// device ever rendered them. DirectSound starts a secondary buffer as it is
/// played, so native ticks are audible. A period below the pump interval lets
/// each cue reach the device first. Clamped to the device's supported range.
#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
fn sfx_buffer_frames(sample_rate: u32, supported: &rodio::cpal::SupportedBufferSize) -> u32 {
    let target = (sample_rate / 100).max(1).next_power_of_two();
    match *supported {
        rodio::cpal::SupportedBufferSize::Range { min, max } => target.clamp(min, max.max(min)),
        rodio::cpal::SupportedBufferSize::Unknown => target,
    }
}

/// Open the default output with [`sfx_buffer_frames`], falling back to
/// rodio's own default sink when the device refuses that period.
#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
fn open_sfx_sink(
    health: &crate::audio::output_health::OutputHealth,
) -> Result<SfxOutput, rodio::DeviceSinkError> {
    use rodio::cpal::traits::{DeviceTrait, HostTrait};

    let low_latency = rodio::cpal::default_host()
        .default_output_device()
        .and_then(|device| {
            let config = device.default_output_config().ok()?;
            let frames = sfx_buffer_frames(config.sample_rate(), config.buffer_size());
            DeviceSinkBuilder::from_device(device)
                .ok()?
                .with_buffer_size(rodio::cpal::BufferSize::Fixed(frames))
                .with_error_callback(health.callback("SFX"))
                .open_stream()
                .inspect(|_| {
                    log::info!(
                        "SFX output buffer: {frames} frames at {} Hz",
                        config.sample_rate()
                    );
                })
                .map_err(|e| log::warn!("SFX low-latency output unavailable ({e}); using default"))
                .ok()
        });
    let mut sink = match low_latency {
        Some(sink) => Ok(sink),
        // With no usable default device, rodio's own opener tries every other.
        None => DeviceSinkBuilder::from_default_device()
            .and_then(|builder| {
                builder
                    .with_error_callback(health.callback("SFX"))
                    .open_sink_or_fallback()
            })
            .or_else(|_| DeviceSinkBuilder::open_default_sink()),
    }?;
    // Rodio's drop message can panic when stderr is a closed pipe.
    sink.log_on_drop(false);
    Ok(sink)
}

/// The browser keeps short blocks with a deeper queue; see
/// [`crate::audio::web_output`].
#[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
fn open_sfx_sink() -> Result<SfxOutput, String> {
    crate::audio::web_output::WebAudioSink::open().map_err(|error| format!("{error:?}"))
}

impl SfxPlayer {
    /// Create a new SfxPlayer. Returns None if audio output cannot be opened.
    pub fn new() -> Option<Self> {
        #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
        let health = crate::audio::output_health::OutputHealth::default();
        #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
        let opened = open_sfx_sink(&health);
        #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
        let opened = open_sfx_sink();
        let device = opened
            .map_err(|e| log::error!("Failed to initialize SFX audio: {}", e))
            .ok()?;

        Some(Self {
            _device: device,
            #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
            health,
            arbiter: SoundArbiter::new(0),
            coordinate_requests: Vec::with_capacity(MAX_COORDINATE_REQUESTS),
            next_coordinate_request: 1,
            pending: BTreeMap::new(),
            live: BTreeMap::new(),
            clips: ClipCache::default(),
            pcm_observer: None,
            now_ms: 0,
            vox: VoxQueue::new(),
            eva_player: None,
            eva_ran_out: None,
            taunt_player: None,
            eva_stream_volume: VolumeInterp::new(VOLUME_SCALE, 0),
            taunt_stream_volume: VolumeInterp::new(VOLUME_SCALE, 0),
            voice_queue: VoiceQueue::new(),
            // Until startup applies the Options profile
            // (`apply_startup_audio_profile`): `OptionsClass__SetDefaults
            // @ 0x005FA350`'s 0.7; the profile is the persisted authority.
            sound_volume: 0.7,
            voice_volume: 0.7,
            sound_master_linear: VOLUME_SCALE,
            eva_master_linear: VOLUME_SCALE,
            focus_output_scale: 1.0,
            paused: false,
            movie_held: false,
            movie_sounds_suspended: false,
            eva_quiet: false,
            voc_enabled: true,
            chrono_screen_active: false,
            game_pause_suspends_sounds: false,
        })
    }

    fn output_scales(&self) -> SfxOutputScales {
        SfxOutputScales {
            sound_volume: self.sound_volume as f32,
            voice_volume: self.voice_volume as f32,
            sound_master_linear: self.sound_master_linear,
            eva_master_linear: self.eva_master_linear,
            focus_output_scale: self.focus_output_scale,
            many_sounds_linear: self.arbiter.many_sounds_linear(),
        }
    }

    /// Queue a centred full-volume sound by its selected SOUNDMD registry ID,
    /// with the existing raw audio.bag-name fallback for unregistered callers.
    /// Returns whether the shared event pool admitted it. Sample selection and
    /// decoding happen only in the later audio service; admission can still
    /// lose a channel, hit Limit, or fail to load a sample before output starts.
    pub fn play_sound(&mut self, sound_id: &str, registry: &SoundRegistry) -> bool {
        self.play_sound_spatial(sound_id, SpatialGain::CENTRED_FULL, registry)
    }

    /// Play a sound with a plain volume multiplier and no pan — the launcher
    /// beep path, which scales a non-positional cue.
    pub fn play_sound_with_volume(
        &mut self,
        sound_id: &str,
        volume: f32,
        registry: &SoundRegistry,
    ) -> bool {
        self.play_sound_spatial(
            sound_id,
            SpatialGain {
                volume,
                pan: PAN_CENTRE,
            },
            registry,
        )
    }

    /// Play a sound at a positional gain from [`spatial_gain`].
    pub fn play_sound_spatial(
        &mut self,
        sound_id: &str,
        gain: SpatialGain,
        registry: &SoundRegistry,
    ) -> bool {
        let facts = entry_facts(sound_id, registry);
        let base_linear = combine_linear(gain.volume_linear(), facts.entry_volume_linear);
        self.submit_request(sound_id, facts, base_linear, gain.pan, SfxChannel::Sound)
            .is_some()
    }

    /// Play only a named `sound(md).ini` event and never reinterpret its ID as
    /// an audio-bag filename. `RulesClass::ReadAudioVisual @ 0x006691E0`
    /// resolves `[AudioVisual] CloakSound` through `VocClass::FindByName @
    /// 0x007514D0`; a failed lookup preserves the invalid constructor index,
    /// so `StartUncloaking @ 0x007036C0` produces no audible fallback.
    pub fn play_registered_sound_spatial(
        &mut self,
        sound_id: &str,
        gain: SpatialGain,
        registry: &SoundRegistry,
    ) -> bool {
        let Some(entry) = registered_entry(sound_id, registry) else {
            return false;
        };
        let facts = EntryFacts::from(entry);
        let base_linear = combine_linear(gain.volume_linear(), facts.entry_volume_linear);
        self.submit_request(sound_id, facts, base_linear, gain.pan, SfxChannel::Sound)
            .is_some()
    }

    /// Queue the cue held by an owner's positional VocHandle.
    ///
    /// Initial `VocClass::PlayAt7509E0` allocation leaves flags8 clear, so
    /// Prepare404700 can play the attack clip. Later750D40 reallocation
    /// skips it; that transition is owned by `update_looping_sound`.
    ///
    /// Inaudible requests use [`Self::bind_inaudible_animation_sound`]; ordinary
    /// positional re-drives use [`Self::update_looping_sound`].
    pub fn play_animation_sound_spatial(
        &mut self,
        owner: impl Into<HandleOwner>,
        sound_id: &str,
        gain: SpatialGain,
        registry: &SoundRegistry,
    ) -> bool {
        if !self.voc_enabled {
            return false;
        }
        let owner = owner.into();
        let Some(entry) = registered_entry(sound_id, registry) else {
            // Original7509E0 binds a null Voc for an invalid index; the raw
            // audio-bag fallback is not reachable from this native function.
            self.stop_animation_sound(owner);
            return false;
        };
        // `VocClass::PlayAt`'s handle-level interrupt: a live event that
        // belongs to a different entry is stopped before the new one starts.
        // This — not `Control=interrupt` — is what makes a re-issued cue cut
        // its predecessor.
        let key = registry_key(sound_id);
        let existing = self.arbiter.validate_loop_handle(owner);
        let event = if self.arbiter.loop_handle_key(owner) == Some(key.as_str()) {
            existing
        } else {
            self.stop_animation_sound(owner);
            None
        };
        let facts = EntryFacts::from(entry);
        let base_linear = combine_linear(gain.volume_linear(), facts.entry_volume_linear);
        let Some(event) = event.or_else(|| {
            self.submit_request(sound_id, facts, base_linear, gain.pan, SfxChannel::Sound)
        }) else {
            // Original750AAE/4060F0 keeps the valid Voc even when both pool
            // allocations fail. An uncounted loop can retry on a later visit.
            self.arbiter
                .keep_loop_sound(owner, &key, facts.is_loopable());
            return false;
        };
        self.arbiter
            .set_volume(event, gain.volume_linear(), self.now_ms);
        self.arbiter.set_pan(event, gain.pan, self.now_ms);
        // The handle is bound either way: a one-shot still belongs to its
        // owner so `stop_animation_sound` can find it, it is just not leashed
        // (`UpdateState` state 3 checks `Control & LOOP` first).
        self.arbiter.set_loop_handle(owner, Some(event), &key);
        if let Some(observer) = &mut self.pcm_observer {
            observer.bind_owner(self.arbiter.event_token(event).expect("new event"), owner);
        }
        true
    }

    /// Re-drive one owner's loop with its current positional gain, the way
    /// `AnimClass::UpdateLoopingSound @ 0x00750D40` runs on every owner
    /// update. A live event takes the new volume and pan; at zero volume it
    /// stops and the handle keeps its sound (`SetLoopHandle(handle, 0, voc)`,
    /// `0x00750E0C..0x00750E11`). With no live event, a loopable sound the
    /// handle kept is allocated again once the owner is audible
    /// (`0x00750D8C..0x00750DB3`: IsLoopable, the pool, MarkStarted), which is
    /// how a loop resumes after the camera comes back or a `Limit=` kill.
    ///
    /// Returns whether the owner holds a live event afterwards.
    pub fn update_looping_sound(
        &mut self,
        owner: impl Into<HandleOwner>,
        gain: Option<SpatialGain>,
        registry: &SoundRegistry,
    ) -> bool {
        let owner = owner.into();
        if let Some(event) = self.arbiter.validate_loop_handle(owner) {
            let Some(gain) = gain else {
                // `if (0.0 < fVar3) {...} else { SoundEvent__Stop; }` then
                // `SetLoopHandle(handle, 0, voc)`.
                let key = self.arbiter.loop_handle_key(owner).map(str::to_owned);
                self.arbiter.stop(event);
                self.release_output(event);
                match key {
                    Some(key) => {
                        let loopable = entry_facts(&key, registry).is_loopable();
                        self.arbiter.keep_loop_sound(owner, &key, loopable);
                    }
                    None => self.arbiter.clear_loop_handle(owner),
                }
                return false;
            };
            let now = self.now_ms;
            self.arbiter
                .set_volume(event, gain.volume_linear().min(VOLUME_SCALE), now);
            self.arbiter.set_pan(event, gain.pan, now);
            return true;
        }
        let (Some(gain), Some(key)) = (gain, self.arbiter.kept_loop_key(owner).map(str::to_owned))
        else {
            return false;
        };
        let started = self.play_animation_sound_spatial(owner, &key, gain, registry);
        if started && let Some(event) = self.arbiter.validate_loop_handle(owner) {
            // Original750DB3: SkipAttack4052E0 belongs to reallocation,
            // including an initial request whose positional gain was zero.
            self.arbiter.mark_started(event);
        }
        started
    }

    /// `VocClass::PlayAt @ 0x007509E0` with a handle while the owner is out of
    /// earshot: nothing plays, the handle's old event stops, and the handle
    /// names the new sound anyway (`SetLoopHandle` runs whenever a handle is
    /// passed), so the owner's update starts the loop once it is audible.
    pub fn bind_inaudible_animation_sound(
        &mut self,
        owner: impl Into<HandleOwner>,
        sound_id: &str,
        registry: &SoundRegistry,
    ) {
        let owner = owner.into();
        self.stop_animation_sound(owner);
        let Some(entry) = registered_entry(sound_id, registry) else {
            return;
        };
        let loopable = EntryFacts::from(entry).is_loopable();
        self.arbiter
            .keep_loop_sound(owner, &registry_key(sound_id), loopable);
    }

    /// `VocClass::PlayAtCoord750E20`: retain an independent coordinate
    /// request, including an initially inaudible one. The 200-node request
    /// pool is distinct from SoundArbiter's 300 events. Original750EC0 reaps
    /// it only after its handle has neither a live event nor an uncounted
    /// loop to retry. Actor Stop/destruction does not own this request.
    ///
    /// The return value reports Rust request admission, not native EAX (an
    /// inaudible native call returns zero even though its node is retained).
    pub(crate) fn play_sound_at_coord(
        &mut self,
        sound_id: &str,
        coordinate: [i32; 3],
        flags: u32,
        source: SoundSource,
        gain: Option<SpatialGain>,
        registry: &SoundRegistry,
    ) -> bool {
        if registered_entry(sound_id, registry).is_none()
            || self.coordinate_requests.len() == MAX_COORDINATE_REQUESTS
        {
            return false;
        }
        let id = self.next_coordinate_request;
        self.next_coordinate_request = id
            .checked_add(1)
            .expect("coordinate request identity exhausted");
        let owner = HandleOwner::Coordinate(id);
        match gain {
            Some(gain) => {
                self.play_animation_sound_spatial(owner, sound_id, gain, registry);
            }
            None => self.bind_inaudible_animation_sound(owner, sound_id, registry),
        }
        // Original750E20 initializes +30 to zero, even when existing
        // requests/events are already suspended by a modal or movie.
        self.coordinate_requests.push(CoordinateSoundRequest {
            id,
            coordinate,
            flags,
            source,
            gain,
            suspend_depth: 0,
        });
        true
    }

    /// `VocClass::StopAtCoord @ 0x00750F40`: visit retained request nodes in
    /// order, match all three signed coordinate words and intersect +2C with
    /// `flags`, then invoke the existing405FD0 handle detach. The node is not
    /// removed here;750EC0 retires it once its detached handle no longer owns
    /// an event or an uncounted-loop binding. Current buffers can finish/decay.
    pub(crate) fn stop_sound_at_coord(&mut self, coordinate: [i32; 3], flags: u32) {
        for index in 0..self.coordinate_requests.len() {
            let request = &self.coordinate_requests[index];
            if request.coordinate == coordinate && request.flags & flags != 0 {
                let owner = HandleOwner::Coordinate(request.id);
                self.detach_animation_sound(owner);
            }
        }
    }

    /// `VocClass::SaveStaticSounds @ 0x00751620`: every retained node's sound
    /// and Coord, in list order. A node whose handle names no sound any more
    /// (native writes no usable index for it) is left out.
    pub(crate) fn saved_static_sounds(&self) -> Vec<SavedStaticSound> {
        self.coordinate_requests
            .iter()
            .filter_map(|request| {
                let owner = HandleOwner::Coordinate(request.id);
                Some(SavedStaticSound {
                    sound_id: self.arbiter.handle_sound_key(owner)?.to_owned(),
                    coordinate: request.coordinate,
                })
            })
            .collect()
    }

    /// `VocClass::LoadStaticSounds @ 0x00751710`, after its clear
    /// ([`Self::stop_all`]): each saved node whose sound is still registered
    /// and loopable (`AudioEventClass::IsLoopable @ 0x00406650`) is retained
    /// again with flags 0 and depth 0 and played at its Coord. Flags 0 means
    /// StopAtCoord750F40 no longer matches a restored node, as in gamemd.
    ///
    /// RESIDUAL: native plays it at once (`VocClass::PlayAt @ 0x007509E0`).
    /// The listener is not restored yet here, so the node starts on the next
    /// sound service through 750D40, which skips an `attack` sample; heard
    /// once per load on a loop that has one and is in earshot.
    pub(crate) fn restore_static_sounds(
        &mut self,
        saved: &[SavedStaticSound],
        registry: &SoundRegistry,
    ) {
        for node in saved {
            if entry_facts(&node.sound_id, registry).is_loopable() {
                self.play_sound_at_coord(
                    &node.sound_id,
                    node.coordinate,
                    0,
                    SoundSource::from_coordinate(node.coordinate),
                    None,
                    registry,
                );
            }
        }
    }

    /// Refresh this derived gain cache from the current listener/shroud.
    /// The app owns that context; audio retains only immutable source facts.
    /// Applying the gain and retiring requests remain in admitted750EC0.
    pub(crate) fn refresh_coordinate_sound_gains(
        &mut self,
        mut gain_for: impl FnMut(&str, SoundSource) -> Option<SpatialGain>,
    ) {
        for request in &mut self.coordinate_requests {
            request.gain = self
                .arbiter
                .handle_sound_key(HandleOwner::Coordinate(request.id))
                .and_then(|key| gain_for(key, request.source));
        }
    }

    /// Passive capture diagnostics of the actual request owner. This creates
    /// no request, validates no handle, and influences no audio/RNG decision.
    pub(crate) fn coordinate_sound_observation(&self) -> Vec<serde_json::Value> {
        self.coordinate_requests
            .iter()
            .map(|request| {
                serde_json::json!({
                    "request_id": request.id,
                    "coordinate_leptons": request.coordinate,
                    "flags": request.flags,
                    "sound_id": self.arbiter.handle_sound_key(HandleOwner::Coordinate(request.id)),
                    "source_world_pixels": [request.source.screen_x, request.source.screen_y],
                    "source_cell": [request.source.rx, request.source.ry],
                    "suspend_depth": request.suspend_depth,
                })
            })
            .collect()
    }

    /// AudioPump406FC1's750EC0 phase follows Sound4041D0 and precedes Vox.
    /// A suspended node skips its re-drive but still validates and retires.
    fn service_coordinate_sounds(&mut self, registry: &SoundRegistry) {
        let mut index = 0;
        while index < self.coordinate_requests.len() {
            let request = &self.coordinate_requests[index];
            let owner = HandleOwner::Coordinate(request.id);
            let (gain, suspended) = (request.gain, request.suspend_depth > 0);
            if !suspended {
                self.update_looping_sound(owner, gain, registry);
            }
            if self.arbiter.validate_loop_handle(owner).is_some()
                || self.arbiter.kept_loop_key(owner).is_some()
            {
                index += 1;
            } else {
                self.arbiter.clear_loop_handle(owner);
                self.coordinate_requests.remove(index);
            }
        }
    }

    /// Original751040 increments each existing node's depth and suspends
    /// its resolved event through4053E0. This follows the global event pass,
    /// so a live coordinate event gains two suspend levels on a pause edge.
    fn suspend_coordinate_sounds(&mut self, now_ms: u64) {
        for request in &mut self.coordinate_requests {
            request.suspend_depth = request.suspend_depth.wrapping_add(1);
            if let Some(event) = self
                .arbiter
                .validate_loop_handle(HandleOwner::Coordinate(request.id))
            {
                self.arbiter.suspend_event(event, now_ms);
            }
        }
    }

    /// Original751080 resumes only nodes whose prior depth was nonzero,
    /// decrementing/clamping their own depth independently of event depth.
    fn resume_coordinate_sounds(&mut self, now_ms: u64) {
        for request in &mut self.coordinate_requests {
            if request.suspend_depth == 0 {
                continue;
            }
            request.suspend_depth = request.suspend_depth.wrapping_sub(1).max(0);
            if let Some(event) = self
                .arbiter
                .validate_loop_handle(HandleOwner::Coordinate(request.id))
            {
                self.arbiter.resume_event(event, now_ms);
            }
        }
    }

    /// `SoundEvent::Release @ 0x00406060` on one owner's handle: an uncounted
    /// loop stops repeating and switches to decay at the next sound service.
    /// The handle is cleared immediately; one-shots keep playing.
    /// Idempotent.
    pub fn release_animation_sound(&mut self, owner: impl Into<HandleOwner>) {
        let owner = owner.into();
        let event = self.arbiter.validate_loop_handle(owner);
        if let Some(observer) = &mut self.pcm_observer {
            observer.owner_action(owner, "release", self.now_ms);
        }
        let control = event
            .and_then(|event| self.live.get(&event))
            .and_then(|output| output.control.clone());
        let mut update_event = || {
            self.arbiter.release_owner(owner);
            event.is_some_and(|event| self.arbiter.playout_no_replay(event))
        };
        if let Some(control) = control {
            control.update_no_replay(update_event);
        } else {
            update_event();
        }
    }

    /// `VocHandle405FD0`: discard the handle and request the next state3
    /// service to stop the current buffer and select decay if available.
    /// The arbiter remains the handle and admission owner.
    pub fn detach_animation_sound(&mut self, owner: impl Into<HandleOwner>) {
        let owner = owner.into();
        let event = self.arbiter.validate_loop_handle(owner);
        if let Some(observer) = &mut self.pcm_observer {
            observer.owner_action(owner, "detach", self.now_ms);
        }
        let control = event
            .and_then(|event| self.live.get(&event))
            .and_then(|output| output.control.clone());
        let mut update_event = || {
            self.arbiter.detach_owner(owner);
            event.is_some_and(|event| self.arbiter.playout_no_replay(event))
        };
        if let Some(control) = control {
            control.update_no_replay(update_event);
        } else {
            update_event();
        }
    }

    /// Whether `owner`'s handle still holds a live event: `0x00406130` on a
    /// VocHandle, as a shell polls its voice.
    pub fn owner_sound_live(&mut self, owner: u64) -> bool {
        self.arbiter
            .validate_loop_handle(HandleOwner::Positional(owner))
            .is_some()
    }

    /// `VocHandle::StopAndClear405D40`: hard-stop the event owned by
    /// `anim_id`, then clear its handle. Idempotent.
    pub fn stop_animation_sound(&mut self, owner: impl Into<HandleOwner>) {
        let owner = owner.into();
        if let Some(event) = self.arbiter.validate_loop_handle(owner) {
            self.arbiter.stop(event);
            self.release_output(event);
        }
        self.arbiter.clear_loop_handle(owner);
    }

    /// `TechnoClass::Queue_Voice @ 0x00708D90` — latch one object's
    /// acknowledgement line (VoiceSelect, VoiceMove, VoiceAttack, ...).
    ///
    /// Nothing plays here. [`Self::visit_unit_voice`] is the drain half, and
    /// it is what decides whether a second click restarts the line, drops it,
    /// or waits — see [`crate::audio::voice_queue`] for the three outcomes.
    pub fn queue_unit_voice(&mut self, owner: u64, sound_id: &str) {
        let before = self
            .pcm_observer
            .as_ref()
            .map(|_| VoiceLatchState::read(&self.voice_queue, owner));
        self.voice_queue.queue(owner, &registry_key(sound_id));
        if let (Some(observer), Some(before)) = (&mut self.pcm_observer, before) {
            observer.voice_action(
                owner,
                "queued",
                before,
                VoiceLatchState::read(&self.voice_queue, owner),
                (None, None),
                self.now_ms,
            );
        }
    }

    /// A derived frame-local interest view. VoiceQueue remains the only
    /// pending authority; simulation uses these IDs only to emit reached heads.
    pub(crate) fn pending_unit_voice_owners(&self) -> BTreeSet<u64> {
        self.voice_queue.pending_owners().collect()
    }

    /// One reached Techno6F9EBB head, in the actual Logic/effect order.
    /// ValidateOrClear406130 treats a queued pool event as live as well.
    pub(crate) fn visit_unit_voice(&mut self, owner: u64, registry: &SoundRegistry) {
        self.report_finished_outputs();
        let handle = HandleOwner::UnitVoice(owner);
        let live_event = self.arbiter.validate_loop_handle(handle);
        let before = self
            .pcm_observer
            .as_ref()
            .map(|_| VoiceLatchState::read(&self.voice_queue, owner));
        let mut admitted = None;
        if let Some(decision) = self.voice_queue.visit(owner, live_event.is_some())
            && let Some(entry) = registered_entry(&decision.sound_id, registry)
        {
            admitted = self.submit_request(
                &entry.id,
                EntryFacts::from(entry),
                entry.volume_linear,
                PAN_CENTRE,
                SfxChannel::Sound,
            );
            self.arbiter
                .set_loop_handle(handle, admitted, &registry_key(&entry.id));
            if let (Some(observer), Some(event)) = (&mut self.pcm_observer, admitted) {
                observer.bind_owner(self.arbiter.event_token(event).expect("new event"), handle);
            }
        }
        if let (Some(observer), Some(before)) = (&mut self.pcm_observer, before) {
            observer.voice_action(
                owner,
                "reached_head",
                before,
                VoiceLatchState::read(&self.voice_queue, owner),
                (live_event, admitted),
                self.now_ms,
            );
        }
    }

    /// A deploy conversion's voice hand-off (`UnitClass::Deploy @
    /// 0x00739971..0x007399B2`, unit to building; `BuildingClass::
    /// Mission_Selling @ 0x0044A0D4..0x0044A115`, building to unit): the new
    /// object takes the handle, pending and playing slots and the old one is
    /// left empty, so its destructor stops nothing and the line plays on.
    pub(crate) fn move_unit_voice(&mut self, from: u64, to: u64) {
        self.arbiter
            .move_loop_handle(HandleOwner::UnitVoice(from), HandleOwner::UnitVoice(to));
        self.voice_queue.move_owner(from, to);
    }

    /// Techno destructor6F4607 -> VocHandle405C00 hard-stops this handle's
    /// event, then destroys pending/last with the Techno. Other owners survive.
    pub(crate) fn destroy_unit_voice(&mut self, owner: u64) {
        let before = self
            .pcm_observer
            .as_ref()
            .map(|_| VoiceLatchState::read(&self.voice_queue, owner));
        let handle = HandleOwner::UnitVoice(owner);
        if let Some(event) = self.arbiter.validate_loop_handle(handle) {
            self.arbiter.stop(event);
            self.release_output(event);
        }
        self.arbiter.clear_loop_handle(handle);
        self.voice_queue.forget(owner);
        if let (Some(observer), Some(before)) = (&mut self.pcm_observer, before) {
            observer.voice_action(
                owner,
                "destroyed",
                before,
                VoiceLatchState::read(&self.voice_queue, owner),
                (None, None),
                self.now_ms,
            );
        }
    }

    /// `VoxClass::PlayEVA @ 0x00752700`: find the entry by name (`stricmp`
    /// scan of `0xB1D4A4`; a miss becomes `QueueVoice(-1, ..)`, which its
    /// index guard rejects), then `QueueVoice(index, type_override, -1)` —
    /// priority is always the entry's own. QueueVoice owns the guarded
    /// PlayNextQueued call, including after a duplicate request.
    ///
    /// The process Vox owner retains the bound voice selection. The event's
    /// sample column is read only when PlayNextQueued dequeues its identity.
    /// An empty column is still queued and fails at the later PlayFile of
    /// `".WAV"`. Returns whether the request entered a queue/pending slot.
    pub fn play_eva(
        &mut self,
        event: &str,
        type_override: Option<EvaType>,
        eva_registry: &EvaRegistry,
        assets: &AssetManager,
        audio_index: Option<&crate::assets::audio_bag::AudioIndex>,
    ) -> bool {
        self.queue_eva(
            eva_registry.index_of(event).unwrap_or(-1),
            type_override,
            None,
            eva_registry,
            assets,
            audio_index,
        )
    }

    /// VoxClass::QueueVoice752480: the shared entry/index admission and
    /// announcement-stream effects for PlayEVA752700 and TAction21. The
    /// original action uses type2/priority-1; priority remains the entry's.
    /// Native comparison: tools/spatial_oracle/trigger_action_values.json,
    /// eva_read_dispatch and eva_history (retail470 entries, paused stream).
    pub fn queue_eva(
        &mut self,
        index: i32,
        type_override: Option<EvaType>,
        priority_override: Option<EvaPriority>,
        eva_registry: &EvaRegistry,
        assets: &AssetManager,
        audio_index: Option<&crate::assets::audio_bag::AudioIndex>,
    ) -> bool {
        let Some(entry) = eva_registry.entry_by_index(index) else {
            return false;
        };
        let effect = self.vox.queue_voice(
            VoxRequest {
                event: &entry.name,
                eva_type: entry.eva_type,
                priority: entry.priority,
            },
            type_override,
            priority_override,
        );
        if effect.stop_stream {
            // `StreamPlayer::Stop` (`0x00752480`, type 2 while current): only
            // the announcement stream; unit lines and SFX are untouched.
            self.stop_eva_stream();
        }
        if effect.advance_queue {
            self.advance_voice_queue(eva_registry, assets, audio_index);
        }
        effect.inserted
    }

    /// `SpeechSystem::PlayTaunt` `0x00752B70` for the taunt byte
    /// `(country << 4) | n`: n must be 1..8 and the country 0..9, which
    /// picks `taunts\tau{am,ko,fr,ge,br,li,ir,cu,ru,yu}%02i.wav` (jump table
    /// `0x00752C6C`). `StreamPlayer::PlayFile(name, 0)` `0x00407B60` tries
    /// the loose file first, then CCFile; a new taunt stops the last one.
    /// Under a movie's stop hold (`+0x88`) Stop and PlayFile both do
    /// nothing, so the paused taunt stays and the new one is dropped. A
    /// paused stream otherwise plays a new file at once: PlayFile sets the
    /// playing bit whatever the pause count `+0x80`.
    /// PlayTaunt's suppress counter `[0x00B1D480]` stays 0 in retail, so its
    /// `== 0` test always passes: SpeechSystem init zeroes it (`0x00752B23`),
    /// and the increment and decrement, `0x00753520`/`0x00753540`, have no
    /// references (no call, jump or stored pointer).
    pub(crate) fn play_taunt(&mut self, byte: u8, assets: &AssetManager, pattern: Option<&str>) {
        const COUNTRIES: [&str; 10] = ["am", "ko", "fr", "ge", "br", "li", "ir", "cu", "ru", "yu"];
        let number = byte & 0xF;
        if !(1..=8).contains(&number) {
            return;
        }
        let name = match pattern {
            Some(pattern) => taunt_filename(pattern, number),
            None => {
                let Some(country) = COUNTRIES.get(usize::from(byte >> 4)) else {
                    return;
                };
                format!("taunts\\tau{country}{number:02}.wav")
            }
        };
        if self.movie_held {
            return;
        }
        if let Some(output) = self.taunt_player.take() {
            output.player.stop();
        }
        let decoded = match assets.read_install_file(&name) {
            Some(bytes) => decode_wav(&bytes, &name),
            None => assets
                .get_ref(&name)
                .and_then(|bytes| decode_wav(bytes, &name)),
        };
        let Some(decoded) = decoded else {
            log::debug!("Taunt {name} is missing");
            return;
        };
        self.taunt_stream_volume.set_target_immediate(VOLUME_SCALE);
        self.taunt_stream_volume.tick(self.now_ms);
        let Some(output) = self.start_voice_output(decoded, VOLUME_SCALE, SfxChannel::TauntStream)
        else {
            return;
        };
        self.taunt_player = Some(output);
    }

    /// Whether the taunt stream has audio left to play.
    pub(crate) fn taunt_stream_busy(&self) -> bool {
        self.taunt_player
            .as_ref()
            .is_some_and(|output| !output.player.empty())
    }

    /// `StreamPlayer::Stop @ 0x00407F40` on the announcement stream, if one
    /// is open. A movie's stop hold (`+0x88`) refuses it: the paused line
    /// plays on after the movie. The stop stamps the stream's end time
    /// (`0x00407A60`), which the next line's gap counts from.
    fn stop_eva_stream(&mut self) {
        if !self.movie_held {
            self.drop_eva_stream();
        }
    }

    fn drop_eva_stream(&mut self) {
        if let Some(output) = self.eva_player.take() {
            output.player.stop();
            self.eva_ran_out = None;
            self.vox.stream_ended(self.now_ms);
        }
    }

    /// The `[DialogList]` index by name (`0x00753250`, a miss is -1) and
    /// `VoxClass::RemoveFromQueues @ 0x00752A40`, which ignores a miss.
    pub fn remove_eva(&mut self, event: &str, eva_registry: &EvaRegistry) {
        if let Some(entry) = eva_registry.entry(event) {
            self.vox.remove_from_queues(&entry.name);
        }
    }
    pub(crate) fn clear_eva_queues(&mut self) {
        self.vox.clear_all_queues();
    }
    pub(crate) fn stop_eva(&mut self, clear_queues: bool) {
        self.stop_eva_stream();
        self.vox.stop_all(clear_queues);
    }
    pub(crate) fn set_voc_enabled(&mut self, enabled: bool) {
        self.voc_enabled = enabled;
    }
    pub(crate) fn set_chrono_screen_active(&mut self, active: bool) {
        self.chrono_screen_active = active;
    }
    /// `RadarClass::AI`'s pass outside its prelude: `ResumeEVA @ 0x00753580`
    /// unless ChronoScreen holds the screen (`0x0053BAE0` at
    /// `0x006539AE..0x006539B7`).
    pub(crate) fn resume_eva_unless_chrono_screen(&mut self) {
        if !self.chrono_screen_active {
            self.vox.resume();
        }
    }
    pub(crate) fn set_eva_quiet(&mut self, quiet: bool) {
        self.eva_quiet = quiet;
    }
    pub(crate) fn suspend_eva(&mut self) {
        self.vox.suspend();
    }
    pub(crate) fn resume_eva(&mut self) {
        self.vox.resume();
    }

    /// InitSideMixFiles534FAC / successful-load67E761 bind the incoming
    /// Scenario.Side; ordinary requests never infer it from a live House.
    pub(crate) fn set_eva_side(&mut self, side: i32, rules: &crate::rules::ruleset::RuleSet) {
        self.vox.set_side(side, rules);
    }
    /// Ares trigger action 148's voice selection ([`super::vox::VoxQueue::set_trigger_voice`]).
    pub(crate) fn set_trigger_eva_voice(&mut self, index: i32, registry: &EvaRegistry) {
        self.vox.set_trigger_voice(index, registry);
    }
    pub(crate) fn eva_entry_done(&self, index: i32, registry: &EvaRegistry) -> bool {
        registry
            .entry_by_index(index)
            .is_none_or(|entry| self.vox.entry_done(&entry.name))
    }
    pub(crate) fn current_eva_event(&self) -> Option<&str> {
        self.vox.current().map(|node| node.event.as_str())
    }
    pub(crate) fn mixer(&self) -> &rodio::mixer::Mixer {
        self._device.mixer()
    }

    /// Whether the announcement stream has audio left to play
    /// (`StreamPlayer::IsPlaying @ 0x00408070` stand-in). Unit lines play
    /// elsewhere and do not count.
    fn eva_stream_busy(&self) -> bool {
        self.eva_player
            .as_ref()
            .is_some_and(|output| !output.player.empty())
    }

    /// `VoxClass::PlayNextQueued @ 0x00752760`, the per-pump dequeue.
    ///
    /// The native gate is `IsPlaying() == 0 && now > GetEndTime() + gap &&
    /// DAT_00b1d428 == 0` (`0x00752794..0x007527D5`); [`VoxQueue::take_next`]
    /// owns it. The stream's end time is reported here the first time the
    /// stream is seen empty after an EVA line started, back-dated to when the
    /// sample ran out, so the 500 ms gap runs from the real end (a paused
    /// line therefore still gets its gap after it resumes and finishes).
    ///
    /// Then the sample is loaded and started — `StreamPlayer::PlayFile(name,
    /// 1)` at `0x0075295C`; a failed load drops the node without touching
    /// the gap (`0x00752963 JZ`), exactly as a missing `.WAV` does natively
    /// (the stock `Dummy` columns of `EVA_PsychicDominatorActivated`).
    pub fn advance_voice_queue(
        &mut self,
        eva_registry: &EvaRegistry,
        assets: &AssetManager,
        audio_index: Option<&crate::assets::audio_bag::AudioIndex>,
    ) {
        if self.eva_quiet {
            return;
        }
        let busy = self.eva_stream_busy();
        if !busy && self.eva_player.take().is_some() {
            // Back-date to when the sample ran out: the pump sees it a pass
            // or a stalled frame later.
            let ran_out_ago = self
                .eva_ran_out
                .take()
                .and_then(|stamp| stamp.get().map(|at| at.elapsed().as_millis()))
                .map_or(0, |ago| u64::try_from(ago).unwrap_or(u64::MAX));
            self.vox
                .stream_ended(self.now_ms.saturating_sub(ran_out_ago));
        }
        let Some(node) = self.vox.take_next(self.now_ms, busy) else {
            return;
        };
        self.start_eva_node(node, eva_registry, assets, audio_index);
    }

    fn start_eva_node(
        &mut self,
        node: VoxNode,
        eva_registry: &EvaRegistry,
        assets: &AssetManager,
        audio_index: Option<&crate::assets::audio_bag::AudioIndex>,
    ) {
        // Native PlayFile(".WAV",1) fails after dequeue for an empty column.
        // It must not prevent QueueVoice from interrupting an existing line.
        let Some(sample) = self
            .vox
            .sample_for(&node, eva_registry)
            .filter(|sample| !sample.is_empty())
        else {
            return;
        };
        // Vox75295C plays the column's physical filename directly. It does
        // not resolve a Voc entry or consume Main sample-selection draws.
        let Some(decoded) = self.clips.load(sample, assets, audio_index) else {
            log::debug!("EVA {} has no sample {}", node.event, sample);
            return;
        };
        // `take_next` only hands out a node once the stream is not playing,
        // and `advance_voice_queue` has already retired it by then.
        debug_assert!(self.eva_player.is_none());
        self.eva_stream_volume.set_target_immediate(VOLUME_SCALE);
        self.eva_stream_volume.tick(self.now_ms);
        let Some(output) = self.start_voice_output(decoded, VOLUME_SCALE, SfxChannel::EvaStream)
        else {
            return;
        };
        let ran_out = Arc::new(OnceLock::new());
        let stamp = Arc::clone(&ran_out);
        output.player.append(EmptyCallback::new(Box::new(move || {
            let _ = stamp.set(crate::util::time::Instant::now());
        })));
        self.eva_ran_out = Some(ran_out);
        self.eva_player = Some(output);
        self.vox.started(node);
    }

    /// Build one centred, full-volume output (`VocClass::PlayAtPos`'s volume
    /// `1.0f`, pan `0x2000`, or the stream's `PlayFile`) on the given channel
    /// and start it on the mixer.
    fn start_voice_output(
        &self,
        decoded: DecodedAudio,
        base_linear: i32,
        channel: SfxChannel,
    ) -> Option<LiveSfxOutput> {
        let PreparedSfxOutput {
            decoded,
            gain,
            initial_volume,
        } = PreparedSfxOutput::new(
            decoded,
            SfxOutputGain::new(base_linear, channel),
            self.output_scales(),
        );
        let channels = NonZero::new(decoded.channels)?;
        let sample_rate = NonZero::new(decoded.sample_rate)?;
        let source = playback::ClipSource::new(decoded.samples, channels, sample_rate);
        let player: Player = Player::connect_new(self._device.mixer());
        let output = LiveSfxOutput::new(player, gain, initial_volume);
        output.player.append(source);
        Some(output)
    }

    /// AllocateFromPool405190 stores identity and gain only. Rejected pool
    /// requests spend no Main draws and do not decode any sample.
    fn submit_request(
        &mut self,
        sound_id: &str,
        facts: EntryFacts,
        base_linear: i32,
        pan: i32,
        channel: SfxChannel,
    ) -> Option<EventId> {
        if !self.voc_enabled {
            return None;
        }
        let key = registry_key(sound_id);
        let event = self.arbiter.submit(
            &PlayRequest {
                key: key.clone(),
                facts,
                volume_linear: base_linear,
                pan,
            },
            self.now_ms,
        )?;
        let token = self
            .arbiter
            .event_token(event)
            .expect("new allocation has a serial");
        if let Some(observer) = &mut self.pcm_observer {
            observer.submitted(token, &key, None, 0, self.now_ms);
        }
        self.pending
            .insert(event, PendingPlayback::new(token, key, channel));
        Some(event)
    }

    /// Observe the device queues between `AudioSystem::Pump` passes.
    ///
    /// The app calls this every frame, *unconditionally*, including the
    /// offline passes whose pump belongs to Main_Tick. Native's pump hangs off
    /// `Network_ServiceLoop @ 0x0048D080`, whose callers include `Main_Tick`,
    /// the network modes' `Main::ThrottleFrame @ 0x0055E160`,
    /// `ProcessModalServicePump @ 0x00623120`, `ShellDialog::RunUntilResult
    /// @ 0x0060D380` and the loading-screen paths, so the audio service keeps
    /// running in menus and modal dialogs. Pause is expressed separately, by
    /// suspending events ([`Self::set_paused`]).
    ///
    /// The app's shared `AudioServiceClock` separately gates Sound/Vox/Theme;
    /// completion observation also runs between admitted main-thread passes.
    /// PCM quarter refill is driven by the continuous source.
    pub(crate) fn service_device_outputs(&mut self, now_ms: u64) {
        self.now_ms = now_ms;
        #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
        self.reopen_lost_output(now_ms);
        self.report_finished_outputs();
        self.tick_stream_volumes(now_ms);
    }

    /// VERA-only: when the device went away, open the default device again.
    /// Sounds on the dead stream would never finish, so they are stopped the
    /// way a world change stops them; sounds asked for from here on are heard.
    #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
    fn reopen_lost_output(&mut self, now_ms: u64) {
        if !self.health.reopen_due(now_ms) {
            return;
        }
        let health = crate::audio::output_health::OutputHealth::default();
        let Ok(device) = open_sfx_sink(&health) else {
            return;
        };
        log::info!("SFX output reopened on the default device");
        self.clear_sound_pool();
        // The lost device took the stream with it, held or not.
        self.drop_eva_stream();
        if let Some(output) = self.taunt_player.take() {
            output.player.stop();
        }
        self._device = device;
        self.health = health;
    }

    fn tick_stream_volumes(&mut self, now_ms: u64) {
        let eva_moved = self.eva_stream_volume.tick(now_ms);
        let taunt_moved = self.taunt_stream_volume.tick(now_ms);
        if !(eva_moved || taunt_moved) {
            return;
        }
        let scales = self.output_scales();
        for (output, volume) in [
            (self.eva_player.as_mut(), &self.eva_stream_volume),
            (self.taunt_player.as_mut(), &self.taunt_stream_volume),
        ] {
            if let Some(output) = output {
                output.gain.buffer_linear = volume.value();
                output.apply_scales(scales);
            }
        }
    }

    /// The radar-movie pre-fade `0x00408390` for the EVA and taunt streams
    /// (Theme's is `MusicPlayer::fade_stream_out`).
    pub(crate) fn fade_streams_out(&mut self, now_ms: u64) {
        self.eva_stream_volume.set_target(0, now_ms);
        self.taunt_stream_volume.set_target(0, now_ms);
    }

    /// `StreamPlayers__RestoreVolumeTargets @ 0x004083D0` for the EVA and
    /// taunt streams.
    pub(crate) fn restore_stream_volumes(&mut self, now_ms: u64) {
        self.eva_stream_volume.set_target(VOLUME_SCALE, now_ms);
        self.taunt_stream_volume.set_target(VOLUME_SCALE, now_ms);
    }

    /// `0x00408410`: no playing stream's own interp is still moving.
    pub(crate) fn streams_settled(&self) -> bool {
        [
            (&self.eva_player, &self.eva_stream_volume),
            (&self.taunt_player, &self.taunt_stream_volume),
        ]
        .into_iter()
        .all(|(output, volume)| !(stream_playing(output) && volume.moving()))
    }

    /// The app's one AudioServiceClock admits Sound4041D0, then Vox752760,
    /// then Theme7209D0. No second SFX or Theme cadence exists.
    pub(crate) fn service_events(
        &mut self,
        now_ms: u64,
        registry: &SoundRegistry,
        eva_registry: &EvaRegistry,
        assets: &AssetManager,
        audio_index: Option<&crate::assets::audio_bag::AudioIndex>,
        rng: &mut PlaybackDraws,
    ) {
        self.now_ms = now_ms;
        let applied_scales = self.output_scales();
        let actions = self.arbiter.update_tick(
            now_ms,
            &mut PendingSampleService {
                pending: &mut self.pending,
                live: &self.live,
                scales: applied_scales,
                registry,
                assets,
                audio_index,
                clips: &mut self.clips,
                rng,
                observer: self.pcm_observer.as_mut(),
            },
        );
        let scales = self.output_scales();
        for action in actions {
            match action {
                ArbiterAction::Start {
                    event,
                    volume_linear,
                    pan,
                    ..
                } => self.start_output(event, volume_linear, pan, now_ms),
                ArbiterAction::Gain {
                    event,
                    volume_linear,
                    pan,
                } => {
                    if let Some(output) = self.live.get_mut(&event) {
                        output.gain.base_linear = volume_linear;
                        output.apply_scales(scales);
                        if let Some(control) = &output.control {
                            control.set_pan(pan);
                        }
                    }
                }
                ArbiterAction::Decay { event } => {
                    if let Some(output) = self.live.get(&event)
                        && let (Some(control), Some(token)) = (&output.control, output.token)
                        && !control.release_to_decay()
                    {
                        self.arbiter.notify_playout_stopped(token);
                    }
                }
                ArbiterAction::Stop { event } => self.release_output(event),
            }
        }
        // The many-sounds scaler moved, so every live output's amplitude has.
        self.apply_live_output_scales();
        self.service_coordinate_sounds(registry);
        // AudioPump406FBC/406FC1/406FC6 services Sound before Vox752760,
        // all behind the same strict >33ms wall-clock gate. Original stock
        // UnitReady controls retain a pending node at 1033 and start at 1034
        // after a 1000ms pass. Reproduce via the factory_infantry_output
        // oracle's UnitReady consumer controls; its single native selector
        // supplies the stock device regression below.
        // PlayEVA and explicit voice wait loops have their own immediate calls.
        self.advance_voice_queue(eva_registry, assets, audio_index);
    }

    /// Attach the source prepared synchronously in StartPlayback4054A0's
    /// service phase. Subsequent quarter fills use that same source owner.
    fn start_output(&mut self, event: EventId, volume_linear: i32, pan: i32, now_ms: u64) {
        let Some(mut pending) = self.pending.remove(&event) else {
            return;
        };
        let token = pending.token;
        if !self.arbiter.is_live_token(token) {
            return;
        }
        let shifts = pending.shifts.expect("channel admission precedes playback");
        let gain = SfxOutputGain {
            base_linear: volume_linear,
            buffer_linear: shifts.volume_linear(),
            channel: pending.channel,
        };
        let player = if let Some(observer) = self
            .pcm_observer
            .as_mut()
            .filter(|observer| observer.contains(token))
        {
            observer
                .connect_player(token, self._device.mixer(), now_ms)
                .expect("observed event")
        } else {
            Player::connect_new(self._device.mixer())
        };
        let mut output = LiveSfxOutput::new(player, gain, gain.effective(self.output_scales()));
        output.token = Some(token);
        if let Some((source, control)) = pending.source.take() {
            output.control = Some(control);
            output.player.append(source);
        } else if let Some(resolved) = pending.prepared.take() {
            let decoded = resolved.decoded;
            let (Some(channels), Some(rate)) = (
                NonZero::new(decoded.channels),
                NonZero::new(decoded.sample_rate),
            ) else {
                self.arbiter.stop(event);
                return;
            };
            if let Some(observer) = &mut self.pcm_observer {
                observer.samples(token, resolved.observed_samples, decoded.samples.len());
            }
            // The pan applies as the start reads the cached clip, which stays
            // as decoded.
            output
                .player
                .append(playback::ClipSource::new(decoded.samples, channels, rate).panned(pan));
        } else {
            self.arbiter.stop(event);
            return;
        }
        output.parked = Some(pending);
        self.live.insert(event, output);
    }

    /// The device telling the arbiter that a playout ran dry — native's
    /// `ch+0xB8` callback `SoundEvent::OnChannelDone @ 0x00405A00`. An event
    /// it ends reaches state 4 for the next pass to reap; one it keeps (a gap
    /// or a remaining above-floor queue) takes its sample state back into
    /// `pending` for its next start pass.
    fn report_finished_outputs(&mut self) {
        let finished: Vec<EventId> = self
            .live
            .iter()
            .filter(|(_, output)| output.player.empty())
            .map(|(event, _)| *event)
            .collect();
        for event in finished {
            let Some(token) = self.output_token(event) else {
                continue;
            };
            let ring = self
                .live
                .get(&event)
                .and_then(|output| output.control.as_ref())
                .map(PlaybackControl::finish);
            let end = ring
                .as_ref()
                .map_or_else(PlayoutEnd::default, |(end, ..)| *end);
            if self.arbiter.notify_playout_ended(token, end, self.now_ms)
                && let Some(output) = self.live.remove(&event)
            {
                output.stop();
                if let Some(mut pending) = output.parked {
                    if let Some((_, loaded, cursor)) = ring {
                        pending.resume_after(loaded, cursor);
                    }
                    self.pending.insert(event, pending);
                }
                continue;
            }
            if let Some(observer) = &mut self.pcm_observer {
                observer.action(token, "completed", self.now_ms);
            }
            self.release_output(event);
        }
    }

    fn output_token(&self, event: EventId) -> Option<EventToken> {
        self.pending
            .get(&event)
            .map(|pending| pending.token)
            .or_else(|| self.live.get(&event).and_then(|output| output.token))
    }

    fn release_output(&mut self, event: EventId) {
        let token = self.output_token(event);
        if let (Some(observer), Some(token)) = (&mut self.pcm_observer, token) {
            observer.action(token, "stopped", self.now_ms);
        }
        self.pending.remove(&event);
        if let Some(output) = self.live.remove(&event) {
            output.stop();
        }
    }

    /// `GamePause::Enter @ 0x00406F00` / `Exit @ 0x00406F40`: the service
    /// keeps pumping either way; pause is expressed by suspending every event
    /// (`SoundSystem::SuspendAll @ 0x00404FD0`) and stopping the actively
    /// playing channels (`DSoundChannel::PauseAll @ 0x00403770`).
    ///
    /// The EVA/speech stream is a **second, unconditional** half of the same
    /// edge: `Enter` calls `VoxClass::PauseEVA @ 0x007535B0` (which reaches
    /// `StreamPlayer::Pause` whenever an announcement is sounding, then
    /// raises the [`VoxQueue`] pause depth) and tail-calls `SpeechSystem::Pause @
    /// 0x00753500` (`StreamPlayer::Pause` again for the speech stream);
    /// `Exit` calls `SpeechSystem::Resume @ 0x00753510` then
    /// `VoxClass::UnpauseEVA @ 0x00753620`. Neither call sits behind the
    /// `0x0053BAD0` ChronoScreen gate that `Enter`'s SFX half does. `Exit`'s
    /// SFX half has no gate: its resumes (`0x004037C0`, `0x00405040`,
    /// `0x00751080`) each do nothing where nothing was paused. The SFX half covers the
    /// unit voice events (Voc channels natively); the stream half is
    /// `eva_player`, and the speech stream SpeechSystem pauses is
    /// `taunt_player`; movie holds pause both streams too
    /// ([`Self::set_movie_held`]).
    ///
    /// Idempotent — call it with the current pause state every frame.
    pub fn set_paused(&mut self, paused: bool, now_ms: u64) {
        self.now_ms = now_ms;
        if paused == self.paused {
            return;
        }
        self.paused = paused;
        let sounds = !paused || !self.chrono_screen_active;
        if sounds {
            self.game_pause_suspends_sounds = paused;
        }
        self.apply_pause_edge(paused, now_ms, sounds);
    }
    /// `Audio__PauseForMovie @ 0x005BF580` / `Audio__ResumeAfterMovie @
    /// 0x005BF530`, the hold every movie takes: `VoxClass__PauseEVA` and
    /// `Audio__PauseAllStreamPlayers @ 0x00408270`, which walks the stream
    /// list `0x0087E810` that every `StreamPlayer` joins (`0x0040799E`): the
    /// EVA and taunt streams here, Theme in `MusicPlayer`. Voc events keep
    /// playing (at the halved effects master the caller sets) and new ones
    /// still start; only full-screen `Play_Movie` also suspends them, through
    /// [`Self::set_movie_sounds_suspended`].
    pub(crate) fn set_movie_held(&mut self, held: bool, now_ms: u64) {
        self.now_ms = now_ms;
        if held == self.movie_held {
            return;
        }
        self.movie_held = held;
        self.vox.set_paused(held);
        self.apply_output_pauses();
    }

    /// `Audio__SuspendSoundEventsForMovie @ 0x00406EA0` /
    /// `Audio__ResumeSoundEventsAfterMovie @ 0x00406EC0`, called only by
    /// full-screen `Play_Movie` (`0x005BEE3E`/`0x005BEE62`,
    /// `0x005BF09F`/`0x005BF0C9`): the same kind-1 channel pause and
    /// `SoundSystem` / `VocClass` suspension as `GamePause::Enter`, without its
    /// ChronoScreen gate.
    pub(crate) fn set_movie_sounds_suspended(&mut self, suspended: bool, now_ms: u64) {
        self.now_ms = now_ms;
        if suspended == self.movie_sounds_suspended {
            return;
        }
        self.movie_sounds_suspended = suspended;
        self.set_sounds_suspended(suspended, now_ms);
        self.apply_output_pauses();
    }

    /// Both native callers enter the existing nested owners. The final output
    ///gate combines their reasons; neither caller owns another event pool.
    fn apply_pause_edge(&mut self, entering: bool, now_ms: u64, suspend_sounds: bool) {
        self.vox.set_paused(entering);
        if suspend_sounds {
            self.set_sounds_suspended(entering, now_ms);
        }
        self.apply_output_pauses();
    }

    fn set_sounds_suspended(&mut self, entering: bool, now_ms: u64) {
        if entering {
            self.arbiter.suspend_all(now_ms);
            self.suspend_coordinate_sounds(now_ms);
        } else {
            self.resume_coordinate_sounds(now_ms);
            self.arbiter.resume_all(now_ms);
        }
    }

    fn streams_paused(&self) -> bool {
        self.movie_held || self.paused
    }

    fn apply_output_pauses(&self) {
        let voc_paused = self.movie_sounds_suspended || self.game_pause_suspends_sounds;
        for output in self.live.values() {
            if voc_paused {
                output.player.pause();
            } else {
                output.player.play();
            }
        }
        let streams_paused = self.streams_paused();
        for output in self.eva_player.iter().chain(self.taunt_player.iter()) {
            if streams_paused {
                output.player.pause();
            } else {
                output.player.play();
            }
        }
    }

    /// Compatibility setter: apply one master to ordinary effects and EVA.
    pub fn set_volume(&mut self, volume: f64) {
        let volume = volume.clamp(0.0, 1.0);
        self.sound_volume = volume;
        self.voice_volume = volume;
        self.apply_live_output_scales();
    }

    /// Set the ordinary effects and unit acknowledgement master volume.
    pub fn set_sound_volume(&mut self, volume: f64) {
        self.sound_volume = volume.clamp(0.0, 1.0);
        self.apply_live_output_scales();
    }

    /// Set the EVA announcement master volume.
    pub fn set_voice_volume(&mut self, volume: f64) {
        self.voice_volume = volume.clamp(0.0, 1.0);
        self.apply_live_output_scales();
    }

    /// The process volume groups only lifecycle code writes: the effects
    /// master `[0x0087E758]` over every Voc channel (SFX and unit voices) and
    /// `[0x0087E750]` over the EVA stream, as current `0..=0x4000` linears.
    /// Neither changes the saved settings or the foreground gate.
    pub(crate) fn set_master_groups(&mut self, sound_master_linear: i32, eva_master_linear: i32) {
        if (sound_master_linear, eva_master_linear)
            == (self.sound_master_linear, self.eva_master_linear)
        {
            return;
        }
        self.sound_master_linear = sound_master_linear;
        self.eva_master_linear = eva_master_linear;
        self.apply_live_output_scales();
    }

    /// Gate global SFX/voice output on the application-activation edge.
    ///
    /// gamemd-derived: the active `WM_ACTIVATEAPP` changed edge at `0x007778AC`
    /// reaches primary-buffer Stop through `FUN_00407020 @
    /// 0x00407020` -> `FUN_0040A940 @ 0x0040A940`, and primary-buffer restore /
    /// looping Play through `FUN_00407040 @ 0x00407040` -> `FUN_0040A950 @
    /// 0x0040A950`. It does not pause the secondary buffers modelled here.
    pub fn set_focus_output_active(&mut self, active: bool) {
        self.focus_output_scale = if active { 1.0 } else { 0.0 };
        self.apply_live_output_scales();
    }

    fn apply_live_output_scales(&self) {
        let scales = self.output_scales();
        for output in self
            .live
            .values()
            .chain(self.eva_player.iter())
            .chain(self.taunt_player.iter())
        {
            output.apply_scales(scales);
        }
    }

    /// Pump the dedicated voice queue once and report whether any voice work
    /// remains, mirroring the poll performed inside native exit wait loops.
    pub fn pump_and_check_voices(
        &mut self,
        eva_registry: &EvaRegistry,
        assets: &AssetManager,
        audio_index: Option<&crate::assets::audio_bag::AudioIndex>,
    ) -> bool {
        self.advance_voice_queue(eva_registry, assets, audio_index);
        self.voices_active()
    }

    /// Hard-stop every SFX/voice source and discard queued announcements.
    pub fn stop_all(&mut self) {
        // Original750FA0: StopAndClear405D40, unlink and return every
        // coordinate node before clearing the common Sound system. The
        // existing world-transition callers share this owner.
        let requests = std::mem::take(&mut self.coordinate_requests);
        for request in requests {
            self.stop_animation_sound(HandleOwner::Coordinate(request.id));
        }
        self.clear_sound_pool();
        // World replacement owns the wider Techno/Vox reset; the screen
        // effect's406E80 call only clears the shared event pool.
        self.voice_queue = VoiceQueue::new();
        // The teardown drops a movie's stop hold with everything else, so
        // it no longer keeps the streams it stops.
        self.movie_held = false;
        self.stop_eva_stream();
        // SpeechSystem::Stop `0x00752CA0`: every scenario exit and the
        // restart stop the taunt stream.
        if let Some(output) = self.taunt_player.take() {
            output.player.stop();
        }
        // Every scenario end runs `0x0069BB40` (`ResetAll` at `0x0069BB82`)
        // before GameExit's `StopAll(1)`: `Route_Scenario_Termination
        // @ 0x0055D145`, `0x0055D1D3`, `0x0055D231`, `0x0055D27E`; so do
        // Main_Game's exit `0x0048CEB4` and LoadMission `0x00559DBC`. Both
        // depths go to zero.
        self.vox.reset_all();
        self.paused = false;
        self.game_pause_suspends_sounds = false;
        self.movie_sounds_suspended = false;
        self.voc_enabled = true;
        self.chrono_screen_active = false;
    }

    ///406E80, shared by world teardown and ChronoScreen53B501. Preserve
    ///Vox depths, owner voice latches and coordinate requests at this boundary.
    pub(crate) fn clear_sound_pool(&mut self) {
        for event in self.live.keys().copied().collect::<Vec<_>>() {
            let token = self.output_token(event);
            if let (Some(observer), Some(token)) = (&mut self.pcm_observer, token) {
                observer.action(token, "stopped", self.now_ms);
            }
        }
        for (_, output) in std::mem::take(&mut self.live) {
            output.stop();
        }
        for event in self.pending.keys().copied().collect::<Vec<_>>() {
            let token = self.output_token(event);
            if let (Some(observer), Some(token)) = (&mut self.pcm_observer, token) {
                observer.action(token, "stopped", self.now_ms);
            }
        }
        self.pending.clear();
        self.arbiter.clear_for_world_replacement();
    }

    pub(crate) fn observe_pcm(&mut self, config: PcmObservationConfig) -> Result<(), &'static str> {
        if self.pcm_observer.is_some() {
            return Err("PCM observation is already active");
        }
        self.pcm_observer = Some(PcmObserver::new(config)?);
        Ok(())
    }

    pub(crate) fn set_pcm_observation_context(&mut self, context: PcmObservationContext) {
        if let Some(observer) = &mut self.pcm_observer {
            observer.context(context);
        }
    }

    pub(crate) fn observed_pcm_settled(&self) -> bool {
        self.pcm_observer.as_ref().is_some_and(|observer| {
            observer.settled()
                && observer.observed_tokens().all(|token| {
                    self.arbiter.is_token_retired(token)
                        && !self.pending.values().any(|pending| pending.token == token)
                        && !self.live.values().any(|output| output.token == Some(token))
                })
        })
    }

    pub(crate) fn finish_pcm_observation(&mut self) -> Option<PcmObservationReport> {
        self.pcm_observer.take().map(|mut observer| {
            let pending: Vec<_> = self.pending.values().map(|value| value.token).collect();
            let live: Vec<_> = self.live.values().filter_map(|value| value.token).collect();
            observer.snapshot(&self.arbiter, &pending, &live);
            let mut report = observer.finish();
            report.coordinate_request_count_at_finish = Some(self.coordinate_requests.len());
            report
        })
    }

    /// Get the current SFX master volume.
    pub fn volume(&self) -> f64 {
        self.sound_volume
    }

    /// Get the current EVA announcement master volume.
    pub fn voice_volume(&self) -> f64 {
        self.voice_volume
    }

    /// Owners whose handle holds a live loop or keeps a loopable sound, so
    /// the app can re-drive each one with its object's current coordinate the
    /// way native's owner calls `AnimClass::UpdateLoopingSound @ 0x00750D40`
    /// on every update.
    pub fn looping_owners(&mut self) -> Vec<u64> {
        self.arbiter.loop_handle_owners()
    }

    /// The `[SoundList]` identity one owner's live loop handle names.
    pub fn loop_handle_sound_id(&self, owner: u64) -> Option<&str> {
        self.arbiter.loop_handle_key(HandleOwner::Positional(owner))
    }

    /// The identity one owner's handle names, live or kept.
    pub fn handle_sound_id(&self, owner: u64) -> Option<&str> {
        self.arbiter
            .handle_sound_key(HandleOwner::Positional(owner))
    }

    /// Number of live sound events — native `g_LiveSoundEventCount @
    /// 0x0087E28C`, which counts records in the pool, not busy channels.
    pub fn active_count(&self) -> usize {
        self.arbiter.live_event_count()
    }

    /// Sound events currently holding an ordinary channel.
    pub fn busy_channel_count(&self) -> usize {
        self.arbiter.busy_channel_count()
    }

    /// `VoxClass::PumpAndCheckActive @ 0x007529E0`'s answer: the stream is
    /// playing (a paused line is not, `0x00408070`) or a node waits in any
    /// queue. Non-blocking (rodio `Player::empty()` is a poll). Used by the
    /// quit cascade and the victory/defeat savour wait.
    pub fn voices_active(&self) -> bool {
        self.vox.is_active(stream_playing(&self.eva_player))
    }
}

/// `StreamPlayer::IsPlaying @ 0x00408070` reads the playing bit, which
/// `Pause @ 0x00407FB0` and the movie pause `0x00408270` move to the paused
/// bit: a stream with audio left plays only while unpaused.
fn stream_playing(output: &Option<LiveSfxOutput>) -> bool {
    output
        .as_ref()
        .is_some_and(|output| !output.player.empty() && !output.player.is_paused())
}

/// The registry entry for a Voc identity, or `None` for an unknown name — a
/// raw sample name is not an event.
fn registered_entry<'a>(sound_id: &str, registry: &'a SoundRegistry) -> Option<&'a SoundEntry> {
    registry.get(sound_id)
}

/// The arbiter's identity key for a sound id.
///
/// Native compares `VocClass*` pointers, so two plays of the same
/// `[SoundList]` entry share one `Limit=` counter and one priority-bucket
/// slot. VERA keys on the uppercased id, which is the same identity because
/// `VocClass::ReadSoundListINI @ 0x007510D0` dedupes list values
/// case-insensitively (`FUN_007C8D20`) into one event object.
fn registry_key(sound_id: &str) -> String {
    sound_id.to_ascii_uppercase()
}

impl From<&SoundEntry> for EntryFacts {
    fn from(entry: &SoundEntry) -> Self {
        Self {
            priority: i32::from(entry.priority),
            limit: entry.limit,
            control: entry.control,
            loop_count: entry.loop_count,
            delay_ms: entry.delay_ms,
            entry_volume_linear: entry.volume_linear,
            samples_ok: !entry.sounds.is_empty() && entry.samples_resolved,
            samples_resolved: entry.samples_resolved,
        }
    }
}

/// The arbiter facts for a sound id.
///
/// A name that is not a `[SoundList]` entry has no `VocClass` in gamemd and
/// therefore plays nothing at all (`VocClass::PlayAt` bails on an invalid
/// index). VERA keeps a labelled raw audio-bag fallback for EVA lines and
/// other bag-only names; those get the `[Defaults]` `Priority=`/`Limit=` and
/// no `Control=` bits, which is the closest thing to an entry they have.
/// **VERA-internal, gamemd has no equivalent.** Trigger: a play call naming a
/// bag sample rather than a `[SoundList]` id. Player effect: VERA plays it
/// where gamemd is silent — pre-existing, and the EVA path depends on it.
/// Frequency: every EVA line. Downstream risk: none beyond the extra cue.
fn entry_facts(sound_id: &str, registry: &SoundRegistry) -> EntryFacts {
    if let Some(entry) = registry.get(sound_id) {
        return EntryFacts::from(entry);
    }
    let defaults = registry.defaults();
    EntryFacts {
        priority: i32::from(defaults.priority),
        limit: defaults.limit,
        control: 0,
        loop_count: 0,
        delay_ms: (0, 0),
        entry_volume_linear: VOLUME_SCALE,
        samples_ok: true,
        samples_resolved: true,
    }
}

/// Device-free core of one play request: draw the shifts, select the
/// samples, load and chain them, and apply the pitch shift. Production calls
/// the same helpers separately at the arbiter's service boundaries.
#[cfg(test)]
fn resolve_entry_playback(
    entry: &SoundEntry,
    rng: &mut impl SampleRng,
    load: impl FnMut(&str) -> Option<DecodedAudio>,
) -> Option<ResolvedPlayback> {
    resolve_entry_playback_pass(entry, rng, load, true, false)
}

/// [`resolve_entry_playback`] for one pass; `plays_attack` is
/// `PreparePlayout`'s `flags & 8` test — see [`select_playout_pass`].
#[cfg(test)]
fn resolve_entry_playback_pass(
    entry: &SoundEntry,
    rng: &mut impl SampleRng,
    mut load: impl FnMut(&str) -> Option<DecodedAudio>,
    plays_attack: bool,
    observe_samples: bool,
) -> Option<ResolvedPlayback> {
    let shifts = PlayShifts::draw(entry, rng);
    LoadedPlayback::load(entry, rng, &mut load)?.prepare(
        entry,
        rng,
        shifts,
        plays_attack,
        observe_samples,
    )
}

/// A sample decoded from its file bytes, without the [`ClipCache`]: what
/// [`ClipCache::load`] gives for a name it has not kept.
#[cfg(test)]
fn load_sfx(
    filename: &str,
    assets: &AssetManager,
    audio_index: Option<&crate::assets::audio_bag::AudioIndex>,
) -> Option<DecodedAudio> {
    ClipCache::default().load(filename, assets, audio_index)
}

/// Decode a sample from the selected audio.bag index (sound effects, voices
/// and EVA announcements) to interleaved f32 stereo.
fn decode_index_clip(
    filename: &str,
    index: &crate::assets::audio_bag::AudioIndex,
) -> Option<DecodedAudio> {
    let (entry, data) = index.get(filename)?;
    let bag_audio = crate::assets::audio_bag::decode_bag_audio(entry, data)?;
    let mut stereo = Vec::new();
    upmix_i16_to_f32_stereo(&mut stereo, &bag_audio.samples_i16, bag_audio.channels);
    Some(DecodedAudio {
        samples: stereo.into(),
        sample_rate: bag_audio.sample_rate,
        channels: 2,
        native_channels: bag_audio.channels,
        native_sample_bytes: if entry.is_16bit() || entry.is_ima_adpcm() {
            2
        } else {
            1
        },
    })
}

/// Decode a sample the audio index lacks from the MIX archives or loose
/// files: the exact name, then with `.wav` appended; `.wav` (PCM or IMA
/// ADPCM) or `.aud`.
fn decode_archive_clip(filename: &str, assets: &AssetManager) -> Option<DecodedAudio> {
    let exact_name = format!("{}.wav", filename);
    let data: &[u8] = assets
        .get_ref(filename)
        .or_else(|| assets.get_ref(&exact_name))?;

    // Try WAV first (most SFX are .wav).
    if data.len() >= 44 && &data[0..4] == b"RIFF" {
        return decode_wav(data, filename);
    }

    // Fall back to .aud format.
    let (header, samples) = aud_file::decode_aud(data)?;
    if samples.is_empty() {
        return None;
    }

    // AUD is always mono — upmix to stereo for rodio.
    let mut stereo = Vec::new();
    upmix_i16_to_f32_stereo(&mut stereo, &samples, 1);
    Some(DecodedAudio {
        samples: stereo.into(),
        sample_rate: header.sample_rate as u32,
        channels: 2,
        native_channels: 1,
        native_sample_bytes: 2,
    })
}

/// Append i16 PCM samples to `out` as interleaved f32 stereo.
/// Mono input is duplicated to both channels.
pub(crate) fn upmix_i16_to_f32_stereo(out: &mut Vec<f32>, samples: &[i16], channels: u16) {
    if channels >= 2 {
        // Already stereo (or more) — just convert to f32.
        out.extend(samples.iter().map(|&s| s as f32 / 32768.0));
    } else {
        // Mono → stereo: duplicate each sample.
        out.reserve(samples.len() * 2);
        out.extend(samples.iter().flat_map(|&s| {
            let f = s as f32 / 32768.0;
            [f, f]
        }));
    }
}

/// Decode a WAV file into interleaved f32 stereo samples.
///
/// Supports uncompressed PCM (format tag 1) with 8-bit or 16-bit samples,
/// and IMA ADPCM (format tag 0x11) used by RA2 EVA announcements.
/// Mono or stereo. This covers all RA2 sound effects and EVA voices.
pub(crate) fn decode_wav(data: &[u8], filename: &str) -> Option<DecodedAudio> {
    let wav = crate::assets::wav_file::WavFile::parse(data)?;
    let Some(mut cursor) = crate::assets::wav_file::WavPcmCursor::new(&wav) else {
        log::trace!(
            "WAV: unsupported format tag {} ({}-bit) for {}",
            wav.format_tag,
            wav.bits_per_sample,
            filename
        );
        return None;
    };
    let mut pcm = Vec::new();
    while cursor.decode_next(wav.data, &mut pcm) {}
    if pcm.is_empty() {
        return None;
    }
    let mut samples = Vec::new();
    upmix_i16_to_f32_stereo(&mut samples, &pcm, wav.channels);
    Some(DecodedAudio {
        samples: samples.into(),
        sample_rate: wav.sample_rate,
        channels: 2,
        native_channels: wav.channels,
        native_sample_bytes: if wav.format_tag == 0x11 {
            2
        } else {
            wav.bits_per_sample / 8
        },
    })
}

/// Ares `File.Taunt=` uses the literal `~~` marker; the stock defaults use
/// `%02i`. Either placeholder receives the two-digit taunt number.
fn taunt_filename(pattern: &str, number: u8) -> String {
    let number = format!("{number:02}");
    if pattern.contains("~~") {
        pattern.replace("~~", &number)
    } else {
        pattern.replace("%02i", &number)
    }
}

#[cfg(all(test, tests_audio))]
mod tests;

#[cfg(all(test, tests_audio))]
mod country_taunt_tests {
    use super::taunt_filename;

    #[test]
    fn ares_and_stock_taunt_placeholders_both_take_the_number() {
        assert_eq!(
            taunt_filename("Taunts\tauam~~.wav", 3),
            "Taunts\tauam03.wav"
        );
        assert_eq!(
            taunt_filename("taunts\tauam%02i.wav", 8),
            "taunts\tauam08.wav"
        );
        assert_eq!(taunt_filename("taunts\\fixed.wav", 1), "taunts\\fixed.wav");
    }
}
