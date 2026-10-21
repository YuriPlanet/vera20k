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
use std::sync::Arc;

use rodio::buffer::SamplesBuffer;
use rodio::{DeviceSinkBuilder, MixerDeviceSink, Player};

use crate::assets::asset_manager::AssetManager;
use crate::assets::aud_file;
use crate::audio::arbiter::{
    self, ArbiterAction, EntryFacts, EventId, EventToken, HandleOwner, PlayRequest,
    PlaybackService, PlayoutEnd, SoundArbiter, combine_linear,
};
use crate::audio::events::SoundSource;
use crate::audio::voice_queue::VoiceQueue;
use crate::audio::vox::{VoxNode, VoxQueue, VoxRequest};
#[cfg(test)]
use crate::rules::sound_ini::EvaSide;
use crate::rules::sound_ini::{
    EvaPriority, EvaRegistry, EvaType, SoundEntry, SoundRegistry, VOLUME_SCALE, control, sound_type,
};
use crate::util::native_x87::X87Chop53;

mod observation;
mod playback;
mod sample_queue;
use playback::{PlaybackControl, PlaylistCursor, RingSource};
use sample_queue::SampleQueue;
#[cfg(test)]
mod ambient_playout_tests;
#[cfg(test)]
mod coordinate_playback_tests;
#[cfg(test)]
mod coordinate_sound_tests;
#[cfg(test)]
mod gattling_loop_tests;
#[cfg(test)]
mod house_cue_tests;
#[cfg(test)]
mod ring_resample_tests;
#[cfg(test)]
mod unit_voice_lifecycle_tests;
#[cfg(test)]
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
    /// `zoom_level` is clamped to `MIN_ZOOM` 0.25 in `app::input::camera`, so
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
///   0x00750BAA`). The cell test is the caller's: `shrouded`.
///   RESIDUAL — the sentinel-cell early-out ahead of it is not modelled.
///   `0x00750B51 CMP CX,[0x00b1d310]` / `0x00750B6C CMP AX,[0x00b1d312]`
///   return 0.0f when the sound's cell equals that `CellStruct` pair, i.e.
///   *before* the `CellClass` lookup at `0x00750B8F`. `get_xrefs_to` on both
///   words shows one read (this site) and one write — the three-instruction
///   setter at `0x007502B0` (`XOR EAX,EAX; MOV [0x00b1d310],AX; MOV
///   [0x00b1d312],AX`), reached only through the vtable slot at `0x0081562C`
///   — and the image bytes are already `00 00 00 00`, so the pair is the null
///   cell `(0,0)` for the whole process lifetime. Trigger: a `Type=SHROUD`
///   cue whose object sits on map cell `(0,0)`. Player effect: that one cue is
///   silent. Frequency: never in ordinary play — cell `(0,0)` is outside every
///   map's playable rectangle, and the 11 stock `SHROUD` cues are super-weapon
///   ready/open sounds that VERA does not emit yet. Downstream risk: none; it
///   is an early-out, so modelling it can only add silence.
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

/// Apply per-channel pan gains to interleaved stereo samples.
fn apply_pan(samples: &mut [f32], pan: i32) {
    let (left, right) = pan_channel_gains(pan);
    if left == 1.0 && right == 1.0 {
        return;
    }
    for frame in samples.as_chunks_mut::<2>().0 {
        frame[0] *= left;
        frame[1] *= right;
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
#[derive(Clone)]
pub(crate) struct DecodedAudio {
    /// Interleaved stereo f32 samples (L, R, L, R, ...).
    pub(crate) samples: Vec<f32>,
    pub(crate) sample_rate: u32,
    /// Always 2 (stereo) — we upmix mono sources for consistency.
    pub(crate) channels: u16,
    /// Native decoded PCM frame width before mono is upmixed for rodio.
    pub(crate) native_frame_bytes: u16,
}

impl DecodedAudio {
    /// Append another clip; the native playlist chains buffers back to back.
    /// A rate mismatch keeps the first clip (RESIDUAL: native streams each
    /// buffer at its own rate; no stock attack/decay set mixes rates).
    #[cfg(test)]
    fn append(&mut self, mut other: DecodedAudio) {
        if other.sample_rate != self.sample_rate || other.channels != self.channels {
            log::warn!(
                "SFX: dropped chained sample with mismatched format ({} Hz vs {} Hz)",
                other.sample_rate,
                self.sample_rate
            );
            return;
        }
        self.samples.append(&mut other.samples);
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
    /// The EVA stream: Voice master only. `[0x0087E750]` is taken as unity —
    /// RESIDUAL: its only writers are the score dialog's fades
    /// (`0x005BEE27..0x005BF5A8`), which VERA's score screen does not run.
    /// Trigger: an EVA line during the score dialog. Player effect: none
    /// reachable today. Frequency: never. Downstream risk: none.
    EvaStream,
    /// The taunt stream `[0x00B1D4D8]`: SpeechSystem::Init (`0x00752AD0`)
    /// links none of its channel's groups (the constructor `0x00407860`
    /// zeroes `+0x90..+0x9C` and only the EVA stream gets `0x00407B40` /
    /// `0x00407B50`), so neither master nor the exit fade reaches it;
    /// CombineInterps skips the null groups (`0x00402252..0x00402292`).
    TauntStream,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct SfxOutputScales {
    sound_volume: f32,
    voice_volume: f32,
    lifecycle_scale: f32,
    focus_output_scale: f32,
    /// The many-sounds master scaler, `g_ManySoundsVolumeGroup @ 0x0087E1B8`,
    /// chained into every channel at `ch+0x98` and multiplied in by
    /// `DSoundBuffer::CombineInterps FUN_004010C0` as `(a * b) >> 14`.
    many_sounds_linear: i32,
}

/// Master-independent gain retained beside one live secondary output.
///
/// `base_linear` is the native linear volume (`0..=0x4000`) of everything
/// below the user master: spatial volume and entry volume. The separate
/// buffer interp holds the per-play `VShift=` reduction. The master joins that product
/// before the DirectSound curve — `DSoundBuffer::CombineInterps FUN_004010C0`
/// multiplies the buffer, event and group interps (`FUN_00402220`; the sound
/// group at `DAT_0087E758`, `SoundEvent::UpdateState 0x0040571E`) and only
/// then `FUN_0040A6D0` converts to decibels — so `effective` recomposes the
/// product each time rather than scaling an amplitude. The VERA-internal
/// lifecycle and foreground gates multiply the amplitude afterwards.
#[derive(Debug, Clone, Copy, PartialEq)]
struct SfxOutputGain {
    base_linear: i32,
    /// Channel-local VShift4054A0; it never changes the event gain used by
    /// Limit/priority in4041D0.
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
        let master = match self.channel {
            SfxChannel::Sound => scales.sound_volume,
            SfxChannel::EvaStream => scales.voice_volume,
            SfxChannel::TauntStream => {
                return native_volume_amplitude(self.base_linear) * scales.focus_output_scale;
            }
        };
        let master_linear = X87Chop53::ftol_f64_low_masked(
            f64::from(master.clamp(0.0, 1.0)) * f64::from(VOLUME_SCALE),
        );
        if self.channel == SfxChannel::EvaStream {
            // The stream has no many-sounds scaler and no audio master, so
            // the scenario-exit fade (`lifecycle_scale`) leaves it alone.
            return native_volume_amplitude(combine_linear(self.base_linear, master_linear))
                * scales.focus_output_scale;
        }
        // The channel multiplies the event group (`ch+0x90`), the many-sounds
        // scaler (`ch+0x98`) and the user volume group (`ch+0x9C`) together
        // before `FUN_0040A6D0` converts to decibels.
        let with_buffer = combine_linear(self.buffer_linear, self.base_linear);
        let with_limiter = combine_linear(with_buffer, scales.many_sounds_linear);
        native_volume_amplitude(combine_linear(with_limiter, master_linear))
            * scales.lifecycle_scale
            * scales.focus_output_scale
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
    registry: &'a SoundRegistry,
    assets: &'a AssetManager,
    audio_index: Option<&'a crate::assets::audio_bag::AudioIndex>,
    rng: &'a mut PlaybackDraws,
    observer: Option<&'a mut PcmObserver>,
}

impl PlaybackService for PendingSampleService<'_> {
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
            pending.prepared =
                load_sfx(&pending.key, self.assets, self.audio_index).map(|decoded| {
                    ResolvedPlayback {
                        decoded,
                        observed_samples: self
                            .observer
                            .as_ref()
                            .is_some_and(|observer| observer.contains(pending.token))
                            .then(|| vec![pending.key.clone()]),
                    }
                });
            return pending.prepared.is_some();
        };
        pending.load_entry_samples(entry, plays_attack, self.rng, |name| {
            load_sfx(name, self.assets, self.audio_index)
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
        let rate = pending
            .shifts
            .expect("admitted channel")
            .shifted_sample_rate(loaded.clips[&initial].sample_rate);
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
            rate,
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
    _device: MixerDeviceSink,
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
    /// The taunt stream `[0x00B1D4D8]` ("Taunt Stream", `0x00752AD0`): a
    /// second StreamPlayer beside EVA's. `PlayFile` stops what it plays, so
    /// a new taunt cuts the last one.
    taunt_player: Option<LiveSfxOutput>,
    /// The per-object voice latch: `TechnoClass::Queue_Voice @ 0x00708D90`
    /// writes it, `TechnoClass::AI_Update @ 0x006F9EBB` drains it.
    voice_queue: VoiceQueue,
    /// Ordinary effects and unit acknowledgement master volume (0.0 to 1.0).
    sound_volume: f64,
    /// EVA announcement master volume (0.0 to 1.0).
    voice_volume: f64,
    /// Temporary app-lifecycle multiplier over every live SFX/voice output.
    output_scale: f32,
    /// Foreground-owned primary-output gate. Secondary Players stay running so
    /// their playback cursors continue while global output is suppressed.
    focus_output_scale: f32,
    /// Whether the game is paused, so [`Self::set_paused`] only acts on the
    /// edge the way `GamePause::Enter`/`Exit` do.
    paused: bool,
    movie_held: bool,
    ///7518C0 gates Voc admission; it neither pauses Theme nor ends buffers.
    voc_enabled: bool,
    ///53BAD0 excludes the SFX half of GamePause, not its EVA stream half.
    chrono_screen_active: bool,
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
fn sfx_buffer_frames(sample_rate: u32, supported: &rodio::cpal::SupportedBufferSize) -> u32 {
    let target = (sample_rate / 100).max(1).next_power_of_two();
    match *supported {
        rodio::cpal::SupportedBufferSize::Range { min, max } => target.clamp(min, max.max(min)),
        rodio::cpal::SupportedBufferSize::Unknown => target,
    }
}

/// Open the default output with [`sfx_buffer_frames`], falling back to
/// rodio's own default sink when the device refuses that period.
fn open_sfx_sink() -> Result<MixerDeviceSink, rodio::DeviceSinkError> {
    use rodio::cpal::traits::{DeviceTrait, HostTrait};

    let low_latency = rodio::cpal::default_host()
        .default_output_device()
        .and_then(|device| {
            let config = device.default_output_config().ok()?;
            let frames = sfx_buffer_frames(config.sample_rate(), config.buffer_size());
            DeviceSinkBuilder::from_device(device)
                .ok()?
                .with_buffer_size(rodio::cpal::BufferSize::Fixed(frames))
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
    match low_latency {
        Some(sink) => Ok(sink),
        None => DeviceSinkBuilder::open_default_sink(),
    }
}

impl SfxPlayer {
    /// Create a new SfxPlayer. Returns None if audio output cannot be opened.
    pub fn new() -> Option<Self> {
        let device = open_sfx_sink()
            .map_err(|e| log::error!("Failed to initialize SFX audio: {}", e))
            .ok()?;

        Some(Self {
            _device: device,
            arbiter: SoundArbiter::new(0),
            coordinate_requests: Vec::with_capacity(MAX_COORDINATE_REQUESTS),
            next_coordinate_request: 1,
            pending: BTreeMap::new(),
            live: BTreeMap::new(),
            pcm_observer: None,
            now_ms: 0,
            vox: VoxQueue::new(),
            eva_player: None,
            taunt_player: None,
            voice_queue: VoiceQueue::new(),
            sound_volume: 0.7,
            voice_volume: 0.7,
            output_scale: 1.0,
            focus_output_scale: 1.0,
            paused: false,
            movie_held: false,
            voc_enabled: true,
            chrono_screen_active: false,
        })
    }

    fn output_scales(&self) -> SfxOutputScales {
        SfxOutputScales {
            sound_volume: self.sound_volume as f32,
            voice_volume: self.voice_volume as f32,
            lifecycle_scale: self.output_scale,
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
    /// RESIDUAL: the suppress counter `[0x00B1D480]` is taken as 0; its
    /// writers `0x00753520`/`0x00753540` have no direct caller.
    pub(crate) fn play_taunt(&mut self, byte: u8, assets: &AssetManager) {
        const COUNTRIES: [&str; 10] = ["am", "ko", "fr", "ge", "br", "li", "ir", "cu", "ru", "yu"];
        let number = byte & 0xF;
        let Some(country) = COUNTRIES.get(usize::from(byte >> 4)) else {
            return;
        };
        if !(1..=8).contains(&number) {
            return;
        }
        let name = format!("taunts\\tau{country}{number:02}.wav");
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
        let Some(output) = self.start_voice_output(decoded, VOLUME_SCALE, SfxChannel::TauntStream)
        else {
            return;
        };
        if self.paused {
            output.player.pause();
        }
        self.taunt_player = Some(output);
    }

    /// Whether the taunt stream has audio left to play.
    pub(crate) fn taunt_stream_busy(&self) -> bool {
        self.taunt_player
            .as_ref()
            .is_some_and(|output| !output.player.empty())
    }

    /// `StreamPlayer::Stop` on the announcement stream, if one is open.
    fn stop_eva_stream(&mut self) {
        if let Some(output) = self.eva_player.take() {
            output.player.stop();
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
    pub(crate) fn eva_entry_done(&self, index: i32, registry: &EvaRegistry) -> bool {
        registry
            .entry_by_index(index)
            .is_none_or(|entry| self.vox.entry_done(&entry.name))
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
    /// stream is seen empty after an EVA line started, so the 500 ms gap runs
    /// from the observed end (a paused line therefore still gets its gap
    /// after it resumes and finishes).
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
        let busy = self.eva_stream_busy();
        if !busy && self.eva_player.take().is_some() {
            self.vox.stream_ended(self.now_ms);
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
        let Some(decoded) = load_sfx(sample, assets, audio_index) else {
            log::debug!("EVA {} has no sample {}", node.event, sample);
            return;
        };
        // `take_next` only hands out a node once the stream is not playing,
        // and `advance_voice_queue` has already retired it by then.
        debug_assert!(self.eva_player.is_none());
        let Some(output) = self.start_voice_output(decoded, VOLUME_SCALE, SfxChannel::EvaStream)
        else {
            return;
        };
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
        let source = SamplesBuffer::new(channels, sample_rate, decoded.samples);
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
    /// The app calls this every frame, *unconditionally* — native's pump
    /// hangs off `Network_ServiceLoop @ 0x0048D080`, whose callers include
    /// `Main::ThrottleFrame @ 0x0055E160`, `ProcessModalServicePump @
    /// 0x00623120`, `ShellDialog::RunUntilResult @ 0x0060D380` and the
    /// loading-screen paths, so the audio service keeps running in menus,
    /// modal dialogs and while the frame pacer idles. Pause is expressed
    /// separately, by suspending events ([`Self::set_paused`]).
    ///
    /// The app's shared `AudioServiceClock` separately gates Sound/Vox/Theme;
    /// completion observation also runs between admitted main-thread passes.
    /// PCM quarter refill is driven by the continuous source.
    pub(crate) fn service_device_outputs(&mut self, now_ms: u64) {
        self.now_ms = now_ms;
        self.report_finished_outputs();
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
        let actions = self.arbiter.update_tick(
            now_ms,
            &mut PendingSampleService {
                pending: &mut self.pending,
                live: &self.live,
                registry,
                assets,
                audio_index,
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
            let mut decoded = resolved.decoded;
            apply_pan(&mut decoded.samples, pan);
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
            output
                .player
                .append(SamplesBuffer::new(channels, rate, decoded.samples));
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
    /// `FUN_0053bad0` gate that the SFX half does. The SFX half covers the
    /// unit voice events (Voc channels natively); the stream half is
    /// `eva_player`, and the speech stream SpeechSystem pauses is
    /// `taunt_player` (GamePause::Enter is its only pauser, so a movie hold
    /// leaves it playing).
    ///
    /// Idempotent — call it with the current pause state every frame.
    pub fn set_paused(&mut self, paused: bool, now_ms: u64) {
        self.now_ms = now_ms;
        if paused == self.paused {
            return;
        }
        self.paused = paused;
        self.apply_pause_edge(paused, now_ms, !self.chrono_screen_active);
    }
    ///5BF580/5BF530 own a distinct idempotent hold. Each source enters the
    ///existing nested Sound/Vox owners; releasing a movie cannot resume a menu.
    pub(crate) fn set_movie_held(&mut self, held: bool, now_ms: u64) {
        if held == self.movie_held {
            return;
        }
        self.movie_held = held;
        self.apply_pause_edge(held, now_ms, true);
    }
    /// Both native callers enter the existing nested owners. The final output
    ///gate combines their reasons; neither caller owns another event pool.
    fn apply_pause_edge(&mut self, entering: bool, now_ms: u64, suspend_sounds: bool) {
        self.vox.set_paused(entering);
        if entering && suspend_sounds {
            self.arbiter.suspend_all(now_ms);
            self.suspend_coordinate_sounds(now_ms);
        } else if suspend_sounds {
            self.resume_coordinate_sounds(now_ms);
            self.arbiter.resume_all(now_ms);
        }
        for output in self.live.values() {
            if self.movie_held || (self.paused && !self.chrono_screen_active) {
                output.player.pause();
            } else {
                output.player.play();
            }
        }
        if let Some(output) = &self.eva_player {
            if self.movie_held || self.paused {
                output.player.pause();
            } else {
                output.player.play();
            }
        }
        if let Some(output) = &self.taunt_player {
            if self.paused {
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

    /// The audio master `[0x0087E758]`: a multiplier over every live Voc
    /// channel output (SFX and unit voices) without changing the saved
    /// settings or foreground gate. The EVA stream is not chained to it (see
    /// [`SfxChannel`]), so the scenario exits' fade to zero leaves an
    /// announcement they queue audible to its end.
    pub fn set_output_scale(&mut self, scale: f64) {
        self.output_scale = scale.clamp(0.0, 1.0) as f32;
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
        self.stop_eva_stream();
        // SpeechSystem::Stop `0x00752CA0`: every scenario exit and the
        // restart stop the taunt stream.
        if let Some(output) = self.taunt_player.take() {
            output.player.stop();
        }
        self.vox.reset_all();
        self.paused = false;
        self.movie_held = false;
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
    pub fn loop_handle_sound_id(&self, owner: u64) -> Option<String> {
        self.arbiter
            .loop_handle_key(HandleOwner::Positional(owner))
            .map(str::to_owned)
    }

    /// The identity one owner's handle names, live or kept.
    pub fn handle_sound_id(&self, owner: u64) -> Option<String> {
        self.arbiter
            .handle_sound_key(HandleOwner::Positional(owner))
            .map(str::to_owned)
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
    /// playing or a node waits in any queue. Non-blocking (rodio
    /// `Player::empty()` is a poll). Used by the quit cascade and the
    /// victory/defeat savour wait.
    pub fn voices_active(&self) -> bool {
        self.vox.is_active(self.eva_stream_busy())
    }
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
            // VERA reads `Sounds=` names without the audio index, so `+0x0C`
            // (every name resolved, `AddSample @ 0x004064A0`) is not modelled.
            samples_ok: !entry.sounds.is_empty(),
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

/// Load a sound effect file and decode it to interleaved f32 stereo samples.
///
/// Resolution order:
/// 1. Try the selected audio.bag index (most voice/EVA sounds live here)
/// 2. Try MIX asset lookup by exact name
/// 3. Try MIX asset lookup with .wav extension appended
///
/// Supports .wav (raw PCM), .aud (IMA ADPCM), and audio.bag formats.
fn load_sfx(
    filename: &str,
    assets: &AssetManager,
    audio_index: Option<&crate::assets::audio_bag::AudioIndex>,
) -> Option<DecodedAudio> {
    // Try the selected audio.bag index first (voices, EVA announcements).
    if let Some(index) = audio_index
        && let Some((entry, data)) = index.get(filename)
        && let Some(bag_audio) = crate::assets::audio_bag::decode_bag_audio(entry, data)
    {
        // Convert i16 → f32 stereo.
        let stereo = upmix_i16_to_f32_stereo(&bag_audio.samples_i16, bag_audio.channels);
        return Some(DecodedAudio {
            samples: stereo,
            sample_rate: bag_audio.sample_rate,
            channels: 2,
            native_frame_bytes: bag_audio.channels
                * if entry.is_16bit() || entry.is_ima_adpcm() {
                    2
                } else {
                    1
                },
        });
    }

    // Try MIX asset lookup (exact name, then with .wav extension).
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
    let stereo = upmix_i16_to_f32_stereo(&samples, 1);
    Some(DecodedAudio {
        samples: stereo,
        sample_rate: header.sample_rate as u32,
        channels: 2,
        native_frame_bytes: 2,
    })
}

/// Convert i16 PCM samples to interleaved f32 stereo.
/// Mono input is duplicated to both channels.
fn upmix_i16_to_f32_stereo(samples: &[i16], channels: u16) -> Vec<f32> {
    if channels >= 2 {
        // Already stereo (or more) — just convert to f32.
        samples.iter().map(|&s| s as f32 / 32768.0).collect()
    } else {
        // Mono → stereo: duplicate each sample.
        samples
            .iter()
            .flat_map(|&s| {
                let f = s as f32 / 32768.0;
                [f, f]
            })
            .collect()
    }
}

/// Decode a WAV file into interleaved f32 stereo samples.
///
/// Supports uncompressed PCM (format tag 1) with 8-bit or 16-bit samples,
/// and IMA ADPCM (format tag 0x11) used by RA2 EVA announcements.
/// Mono or stereo. This covers all RA2 sound effects and EVA voices.
pub(crate) fn decode_wav(data: &[u8], filename: &str) -> Option<DecodedAudio> {
    let wav = crate::assets::wav_file::WavFile::parse(data)?;
    let samples = match wav.format_tag {
        1 => decode_pcm(wav.data, wav.channels, wav.bits_per_sample),
        0x11 => decode_ima_adpcm(wav.data, wav.channels, wav.block_align),
        _ => {
            log::trace!(
                "WAV: unsupported format tag {} for {}",
                wav.format_tag,
                filename
            );
            return None;
        }
    };
    if samples.is_empty() {
        return None;
    }
    let stereo = if wav.channels == 1 {
        samples.iter().flat_map(|&s| [s, s]).collect()
    } else {
        samples
    };
    Some(DecodedAudio {
        samples: stereo,
        sample_rate: wav.sample_rate,
        channels: 2,
        native_frame_bytes: wav.channels
            * if wav.format_tag == 0x11 {
                2
            } else {
                wav.bits_per_sample / 8
            },
    })
}

/// Decode IMA ADPCM WAV data into interleaved f32 samples.
///
/// `WAV__ParseHeader @ 0x00408610` maps `wFormatTag == 0x11` onto the same audio
/// format descriptor the bag index fills, taking the block stride from the fmt
/// chunk's `nBlockAlign` (`psVar6[6]`). `FUN_00409C40 @ 0x00409C40` then installs
/// the one block decoder in the image, `IMA_ADPCM__DecodeBlock @ 0x0040AA70`, so
/// WAV and bag IMA share a decoder natively and share `ima_adpcm::decode_blocks`
/// here.
fn decode_ima_adpcm(data: &[u8], channels: u16, block_align: u16) -> Vec<f32> {
    crate::assets::ima_adpcm::decode_blocks(data, channels, block_align as u32)
        .into_iter()
        .map(|s| s as f32 / 32768.0)
        .collect()
}

/// Convert raw PCM bytes to f32 samples. Output channel count matches input.
fn decode_pcm(pcm: &[u8], channels: u16, bits_per_sample: u16) -> Vec<f32> {
    match (bits_per_sample, channels) {
        (16, _) => pcm
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0)
            .collect(),
        (8, _) => pcm.iter().map(|&b| (b as f32 - 128.0) / 128.0).collect(),
        _ => {
            log::trace!("WAV: unsupported {}bit {}ch PCM", bits_per_sample, channels);
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drive the production clock/service owners without app/window setup.
    /// This supplies neither sample choices nor independent RNG logic.
    struct TestAudioService {
        clock: arbiter::AudioServiceClock,
        main: crate::sim::rng::MainRng,
    }
    impl Default for TestAudioService {
        fn default() -> Self {
            Self {
                clock: arbiter::AudioServiceClock::default(),
                main: crate::sim::rng::MainRng::new(31),
            }
        }
    }
    impl TestAudioService {
        fn pump(
            &mut self,
            player: &mut SfxPlayer,
            now: u64,
            registry: &SoundRegistry,
            eva_registry: &EvaRegistry,
            assets: &AssetManager,
            index: Option<&crate::assets::audio_bag::AudioIndex>,
        ) {
            let main = self.main.draws();
            let mut draw = PlaybackDraws::new(move |low, high| main.ranged(low, high));
            player.service_device_outputs(now);
            if self.clock.admit(now) {
                player.service_events(now, registry, eva_registry, assets, index, &mut draw);
            }
        }
    }

    /// About 10 ms of frames, below the 33 ms arbiter pump, clamped to what
    /// the device supports.
    #[test]
    fn sfx_buffer_period_stays_below_the_pump_interval() {
        use rodio::cpal::SupportedBufferSize;
        assert_eq!(
            sfx_buffer_frames(48_000, &SupportedBufferSize::Unknown),
            512
        );
        assert_eq!(
            sfx_buffer_frames(44_100, &SupportedBufferSize::Unknown),
            512
        );
        let range = SupportedBufferSize::Range {
            min: 1024,
            max: 4096,
        };
        assert_eq!(sfx_buffer_frames(48_000, &range), 1024);
        let range = SupportedBufferSize::Range { min: 64, max: 256 };
        assert_eq!(sfx_buffer_frames(48_000, &range), 256);
    }
    use crate::rules::ini_parser::IniFile;

    /// A scripted RNG: hands out the listed draws in order and records every
    /// request so a test can pin the native draw sequence.
    struct ScriptedRng {
        draws: Vec<i32>,
        requests: Vec<(i32, i32)>,
    }

    impl ScriptedRng {
        fn new(draws: &[i32]) -> Self {
            Self {
                draws: draws.iter().rev().copied().collect(),
                requests: Vec::new(),
            }
        }
    }

    impl SampleRng for ScriptedRng {
        fn ranged(&mut self, low: i32, high: i32) -> i32 {
            if low == high {
                return low;
            }
            self.requests.push((low, high));
            self.draws.pop().unwrap_or(low)
        }
    }

    /// `[SoundList]` is what registers an id (`VocClass::ReadSoundListINI @
    /// 0x007510D0`), so the fixture names `[T]` there the way the stock file
    /// names every sound.
    fn entry(body: &str) -> SoundEntry {
        let registry =
            SoundRegistry::from_ini(&IniFile::from_str(&format!("[SoundList]\n1=T\n{body}")));
        registry.get("T").expect("entry").clone()
    }

    const LISTENER_W: f32 = 1024.0;
    const LISTENER_H: f32 = 600.0;

    fn source(range_cells: i32, type_flags: u32, min_volume: f32) -> SpatialSource {
        SpatialSource {
            range_cells,
            type_flags,
            min_volume,
        }
    }

    fn gain(client_x: i32, client_y: i32, src: SpatialSource) -> Option<SpatialGain> {
        calc_volume_and_pan(client_x, client_y, LISTENER_W, LISTENER_H, src, false)
    }

    /// Vectors walked through the `0x00750AC0` transcription: a 1024x600
    /// tactical view (halfW 512, halfH 300, fullW 1024) and Range 10
    /// (maxRange 600).
    #[test]
    fn calc_volume_and_pan_screen_type_matches_the_native_transcription() {
        let screen = source(10, sound_type::SCREEN, 0.5);
        // View centre: on screen is full volume, centred pan.
        assert_eq!(
            gain(512, 300, screen),
            Some(SpatialGain {
                volume: 1.0,
                pan: PAN_CENTRE
            })
        );
        // Anywhere on screen is still full volume; pan follows the offset.
        let edge = gain(1023, 599, screen).unwrap();
        assert_eq!(edge.volume, 1.0);
        assert_eq!(
            edge.pan,
            X87Chop53::ftol_f64_low_masked(511.0 * 8192.0 / 1024.0 + 8192.0)
        );
        // 300 px right of the view edge: half volume, pan 8192 + 812 * 8.
        assert_eq!(
            gain(1324, 300, screen),
            Some(SpatialGain {
                volume: 0.5,
                pan: 14688
            })
        );
        // Y distances count double: 100 px below the view edge is 200.
        assert_eq!(gain(512, 700, screen).unwrap().volume, 400.0 / 600.0);
        // The larger axis wins: 200 px right (200) vs 150 px below (300).
        assert_eq!(gain(1224, 750, screen).unwrap().volume, 300.0 / 600.0);
        // Below the 0.05 cutoff is silent; just above is not.
        assert_eq!(gain(1600, 300, screen), None);
        assert_eq!(gain(1584, 300, screen).unwrap().volume, 40.0 / 600.0);
        // At and beyond maxRange the volume is exactly 0 (silent).
        assert_eq!(gain(1624, 300, screen), None);
        assert_eq!(gain(-1000, 300, screen), None);
        // A zero range is never audible.
        assert_eq!(gain(512, 300, source(0, sound_type::SCREEN, 0.0)), None);
    }

    #[test]
    fn calc_volume_and_pan_local_global_and_shroud_gates() {
        // LOCAL skips the half-view subtraction: 100 px from centre is 100.
        let local = source(10, sound_type::LOCAL, 0.5);
        assert_eq!(gain(612, 300, local).unwrap().volume, 500.0 / 600.0);
        // LOCAL with a large range reaches the pan clamps at both ends.
        let far = source(100, sound_type::LOCAL, 0.0);
        assert_eq!(gain(-1500, 300, far).unwrap().pan, 0);
        assert_eq!(gain(3000, 300, far).unwrap().pan, PAN_SCALE);
        // GLOBAL raises a 10/600 volume to the MinVolume floor; SCREEN alone
        // falls below the cutoff instead.
        let global = source(10, sound_type::SCREEN | sound_type::GLOBAL, 0.5);
        assert_eq!(gain(1614, 300, global).unwrap().volume, 0.5);
        assert_eq!(gain(1614, 300, source(10, sound_type::SCREEN, 0.5)), None);
        // GLOBAL keeps a louder computed volume.
        assert_eq!(gain(1324, 300, global).unwrap().volume, 0.5);
        assert_eq!(gain(1224, 300, global).unwrap().volume, 400.0 / 600.0);
        // SHROUD silences a shrouded cell only when the flag is set.
        let shroud = source(10, sound_type::SCREEN | sound_type::SHROUD, 0.5);
        assert_eq!(
            calc_volume_and_pan(512, 300, LISTENER_W, LISTENER_H, shroud, true),
            None
        );
        assert!(calc_volume_and_pan(512, 300, LISTENER_W, LISTENER_H, shroud, false).is_some());
        let unshrouded = source(10, sound_type::SCREEN | sound_type::UNSHROUD, 0.5);
        assert!(calc_volume_and_pan(512, 300, LISTENER_W, LISTENER_H, unshrouded, true).is_some());
    }

    /// Odd view widths put the centre on a half pixel; the distances are
    /// `ftol`-truncated before `abs`, the pan offset is not.
    #[test]
    fn calc_volume_and_pan_truncates_distances_like_ftol() {
        let src = source(10, sound_type::LOCAL, 0.0);
        // clientX - 511.5 = 0.5 -> ftol 0 -> full volume; pan keeps the 0.5.
        let g = calc_volume_and_pan(512, 300, 1023.0, LISTENER_H, src, false).unwrap();
        assert_eq!(g.volume, 1.0);
        assert_eq!(
            g.pan,
            X87Chop53::ftol_f64_low_masked(0.5 * 8192.0 / 1023.0 + 8192.0)
        );
        // clientX - 511.5 = -0.5 -> ftol 0 as well (truncation toward zero).
        let g = calc_volume_and_pan(511, 300, 1023.0, LISTENER_H, src, false).unwrap();
        assert_eq!(g.volume, 1.0);
        assert_eq!(
            g.pan,
            X87Chop53::ftol_f64_low_masked(-0.5 * 8192.0 / 1023.0 + 8192.0)
        );
    }

    fn zoom_listener(zoom: f32) -> SpatialListener {
        SpatialListener {
            tactical_width: LISTENER_W as i32,
            tactical_height: LISTENER_H as i32,
            origin_x: 0.0,
            origin_y: 0.0,
            zoom,
        }
    }

    /// The listener rect and the sound positions must stay in one frame at
    /// every zoom. VERA projects `device = (world - camera) * zoom`, so a
    /// 1024x600 device viewport covers `1024/zoom` x `600/zoom` world pixels;
    /// `SpatialListener::view_extent` is what puts the rect in the world frame
    /// the positions already use.
    ///
    /// gamemd has no zoom, so nothing here is a native golden. What the
    /// vectors pin is that zoom behaves like the one zoom-shaped thing gamemd
    /// *does* have — a resolution change, which alters how much world the view
    /// rect covers while `VocClass::CalcVolumeAndPan @ 0x00750AC0` keeps
    /// `Range * 0x3C` as a fixed cell distance.
    #[test]
    fn spatial_gain_measures_one_frame_at_every_zoom() {
        let src = source(10, sound_type::SCREEN, 0.5);
        // At zoom 1.0 the world frame *is* the device frame, so the listener
        // must reproduce the raw-device-size call bit for bit.
        for x in [0.0f32, 256.0, 512.0, 1324.0, 1584.0] {
            assert_eq!(
                spatial_gain(src, x, 300.0, &zoom_listener(1.0), false),
                calc_volume_and_pan(x as i32, 300, LISTENER_W, LISTENER_H, src, false),
                "zoom 1.0 must equal the native device-pixel call at x={x}"
            );
        }

        // `Range=10` is 600 world pixels — 10 cells — at every zoom. A sound
        // exactly 300 world pixels past the right edge of the visible area is
        // half volume whether the view covers 1024, 512 or 2048 world pixels.
        for (zoom, view_w, centre_y) in [
            (1.0f32, 1024.0f32, 300.0f32),
            (2.0, 512.0, 150.0),
            (0.5, 2048.0, 600.0),
        ] {
            let gain = spatial_gain(src, view_w + 300.0, centre_y, &zoom_listener(zoom), false)
                .unwrap_or_else(|| panic!("audible at zoom {zoom}"));
            assert_eq!(gain.volume, 0.5, "zoom {zoom} falloff");
        }

        // Pan is the position *within* the view, so a sound a quarter of the
        // way across the visible area sits at the same 6144 at every zoom —
        // and stays inside the view, hence full volume.
        for (zoom, view_w, centre_y) in [
            (1.0f32, 1024.0f32, 300.0f32),
            (2.0, 512.0, 150.0),
            (0.5, 2048.0, 600.0),
        ] {
            let gain = spatial_gain(src, view_w * 0.25, centre_y, &zoom_listener(zoom), false)
                .unwrap_or_else(|| panic!("audible at zoom {zoom}"));
            assert_eq!(
                (gain.volume, gain.pan),
                (1.0, 6144),
                "zoom {zoom} quarter-width pan"
            );
        }

        // The regression the frame mismatch hid: one fixed world position must
        // move with the zoom. Ignoring zoom made all three of these 0.5/14688.
        let fixed = (1324.0f32, 300.0f32);
        assert_eq!(
            spatial_gain(src, fixed.0, fixed.1, &zoom_listener(1.0), false),
            Some(SpatialGain {
                volume: 0.5,
                pan: 14688
            })
        );
        // Zoomed 2x the view covers 512x300 world px, so the same point is
        // 812 px outside it — past `Range * 60` and silent.
        assert_eq!(
            spatial_gain(src, fixed.0, fixed.1, &zoom_listener(2.0), false),
            None
        );
        // Zoomed out to 0.5x the view covers 2048x1200 world px, so the point
        // is on screen: full volume, panned by its offset from the centre.
        assert_eq!(
            spatial_gain(src, fixed.0, fixed.1, &zoom_listener(0.5), false),
            Some(SpatialGain {
                volume: 1.0,
                pan: 9392
            })
        );

        // A degenerate zoom must neither divide by zero (the VERA-internal
        // EPSILON floor) nor panic: the resulting half-view saturates the
        // `ftol` cast to `i32::MIN`, which `native_abs` wraps the way
        // `CDQ; XOR; SUB` does instead of trapping like `i32::abs`.
        assert!(spatial_gain(src, 0.0, 0.0, &zoom_listener(0.0), false).is_some());
    }

    #[test]
    fn spatial_listener_projects_world_pixels_to_client_points() {
        let listener = SpatialListener {
            tactical_width: LISTENER_W as i32,
            tactical_height: LISTENER_H as i32,
            origin_x: 1000.25,
            origin_y: 2000.0,
            zoom: 1.0,
        };
        assert_eq!(listener.client_point(1512.25, 2300.0), (512, 300));
        assert_eq!(listener.client_point(999.75, 1999.0), (0, -1));
        let src = source(10, sound_type::SCREEN, 0.5);
        assert_eq!(
            spatial_gain(src, 1512.25, 2300.0, &listener, false),
            Some(SpatialGain {
                volume: 1.0,
                pan: PAN_CENTRE
            })
        );
    }

    /// The `0x00816380` table (machine-read) against `round(1000 * log2(i /
    /// 100))` and the index arithmetic of `FUN_0040A6D0`.
    #[test]
    fn dsound_attenuation_table_and_index_follow_the_binary() {
        for (i, &entry) in DSOUND_ATTENUATION_TABLE.iter().enumerate().skip(1) {
            let expected = (1000.0 * (i as f64 / 100.0).log2()).round() as i16;
            assert_eq!(entry, expected, "table[{i}]");
        }
        assert_eq!(DSOUND_ATTENUATION_TABLE[0], -10000);
        assert_eq!(DSOUND_ATTENUATION_TABLE[100], 0);
        assert_eq!(native_volume_amplitude(VOLUME_SCALE), 1.0);
        assert!((native_volume_amplitude(8192) - 10f32.powf(-0.5)).abs() < 1e-6);
        assert_eq!(native_volume_amplitude(0), 0.0);
        // 16383 * 25 >> 12 = 99, so anything short of full scale loses 0.14 dB.
        assert!((native_volume_amplitude(16383) - 10f32.powf(-14.0 / 2000.0)).abs() < 1e-6);
        assert_eq!(combine_linear(13107, VOLUME_SCALE), 13107);
        assert_eq!(combine_linear(8192, 8192), 4096);
        assert_eq!(combine_linear(VOLUME_SCALE, VOLUME_SCALE), VOLUME_SCALE);
    }

    #[test]
    fn pan_channel_gains_attenuate_the_far_side_through_the_table() {
        assert_eq!(pan_channel_gains(PAN_CENTRE), (1.0, 1.0));
        // Full right: p = 100, the left channel takes table[0].
        assert_eq!(pan_channel_gains(PAN_SCALE), (0.0, 1.0));
        assert_eq!(pan_channel_gains(0), (1.0, 0.0));
        // 12288 -> 150 - 100 = 50 -> left attenuated by table[50] = -10 dB.
        let (left, right) = pan_channel_gains(12288);
        assert!((left - 10f32.powf(-0.5)).abs() < 1e-6);
        assert_eq!(right, 1.0);
        let (left, right) = pan_channel_gains(4096);
        assert_eq!(left, 1.0);
        assert!((right - 10f32.powf(-0.5)).abs() < 1e-6);
        let mut samples = vec![1.0, 1.0, 0.5, 0.5];
        apply_pan(&mut samples, 12288);
        assert!((samples[0] - 10f32.powf(-0.5)).abs() < 1e-6);
        assert_eq!(samples[1], 1.0);
        assert_eq!(samples[3], 0.5);
    }

    #[test]
    fn play_shifts_draw_fshift_vshift_then_predelay_in_native_order() {
        let e = entry(
            "[T]\nSounds=a\nFShift= -10 10\nVShift=20\nControl= random predelay\nDelay=0 400\n",
        );
        let mut rng = ScriptedRng::new(&[7, 13, 250]);
        let shifts = PlayShifts::draw(&e, &mut rng);
        assert_eq!(rng.requests, vec![(-10, 10), (0, 20), (0, 400)]);
        assert_eq!(shifts.frequency_pct, 107);
        assert_eq!(shifts.volume_shift_pct, 13);
        // 0x4000 - (13 << 14) / 100 = 16384 - 2129.
        assert_eq!(shifts.volume_linear(), 16384 - 2129);
        assert_eq!(shifts.shifted_sample_rate(22050), 22050 * 107 / 100);
        // The draw is carried, not discarded: the arbiter parks the event in
        // state 2 for this long.
        assert_eq!(shifts.predelay_ms, 250);

        // AMBIENT pre-delays draw from 0x21 regardless of Delay.min.
        let ambient = entry("[T]\nSounds=a\nControl= loop ambient\nDelay=5000 8000\n");
        let mut rng = ScriptedRng::new(&[6000]);
        let shifts = PlayShifts::draw(&ambient, &mut rng);
        assert_eq!(rng.requests, vec![(0x21, 8000)]);
        assert_eq!(shifts.frequency_pct, 100);
        assert_eq!(shifts.volume_linear(), VOLUME_SCALE);
        assert_eq!(shifts.shifted_sample_rate(22050), 22050);
        assert_eq!(shifts.predelay_ms, 6000);

        // No shifts, no pre-delay control: nothing is drawn.
        let plain = entry("[T]\nSounds=a\nDelay=0 400\n");
        let mut rng = ScriptedRng::new(&[]);
        let shifts = PlayShifts::draw(&plain, &mut rng);
        assert!(rng.requests.is_empty());
        assert_eq!(shifts.predelay_ms, 0);
    }

    /// The registry facts the arbiter arbitrates on come straight off the
    /// `[SoundList]` entry, and a name that is not one falls back to
    /// `[Defaults]`.
    #[test]
    fn entry_facts_carry_the_registry_priority_limit_control_and_loop() {
        let registry = SoundRegistry::from_ini(&IniFile::from_str(
            "[Defaults]\nLimit=5\nPriority=NORMAL\n\
             [SoundList]\n1=RocketeerMoveLoop\n\
             [RocketeerMoveLoop]\nSounds=a b c d e\n\
             Control= loop random all decay attack\nPriority=Low\nLimit=3\nVolume=25\n",
        ));
        let facts = entry_facts("RocketeerMoveLoop", &registry);
        assert_eq!(facts.priority, 1);
        assert_eq!(facts.limit, 3);
        assert_eq!(facts.loop_count, 0);
        assert!(facts.control & control::LOOP != 0);
        // `Loop=` absent with `Control=loop` is the owner-driven sustain that
        // `AudioEventClass::IsLoopable @ 0x00406650` reports.
        assert!(facts.is_loopable());

        // `[Defaults] Limit=5` reaches a raw bag name through the fallback.
        let bag = entry_facts("ImNotAnEntry", &registry);
        assert_eq!(bag.limit, 5);
        assert_eq!(bag.priority, 2);
        assert!(!bag.is_loopable());
    }

    /// A one-shot `Control= random predelay` entry is not loopable, which is
    /// what keeps a Grizzly's `MoveSound` a single start-up sample rather
    /// than an engine hum: `[GTNK] MoveSound=GrizzlyTankMoveStart` and
    /// `[GrizzlyTankMoveStart] Control= random predelay`, `Delay=0 400`.
    #[test]
    fn a_random_predelay_move_sound_is_not_a_sustained_loop() {
        let registry = SoundRegistry::from_ini(&IniFile::from_str(
            "[SoundList]\n1=GrizzlyTankMoveStart\n\
             [GrizzlyTankMoveStart]\nSounds=vgristaa vgristab vgristac\n\
             Control= random predelay\nDelay=0 400\nPriority=low\nVolume=40\n",
        ));
        let facts = entry_facts("GrizzlyTankMoveStart", &registry);
        assert!(facts.control & control::LOOP == 0);
        assert!(!facts.is_loopable());
        assert_eq!(facts.delay_ms, (0, 400));
    }

    /// `Control=random` over three samples: one `RandomRanged(0, 2)` draw and
    /// the equal-bounds pick that follows draws nothing.
    #[test]
    fn select_playout_random_picks_one_body_sample() {
        let e = entry("[T]\nSounds=a b c\nControl=random\n");
        let mut rng = ScriptedRng::new(&[1]);
        assert_eq!(select_playout(&e, &mut rng), vec![1]);
        assert_eq!(rng.requests, vec![(0, 2)]);
    }

    /// Without `random` the first body sample always plays — there is no
    /// round-robin — and `all` plays the whole body in order.
    #[test]
    fn select_playout_sequential_forms_play_first_or_all() {
        let first = entry("[T]\nSounds=a b c\n");
        let mut rng = ScriptedRng::new(&[]);
        assert_eq!(select_playout(&first, &mut rng), vec![0]);
        assert_eq!(select_playout(&first, &mut rng), vec![0]);
        assert!(rng.requests.is_empty());

        let all = entry("[T]\nSounds=a b c\nControl=all\n");
        assert_eq!(select_playout(&all, &mut rng), vec![0, 1, 2]);
        assert!(rng.requests.is_empty());
    }

    /// `random all`: the body is loaded whole and played in pick-and-remove
    /// order, `RandomRanged(0, remaining - 1)` each step.
    #[test]
    fn select_playout_random_all_shuffles_by_pick_and_remove() {
        let e = entry("[T]\nSounds=a b c\nControl= random all\n");
        let mut rng = ScriptedRng::new(&[2, 0]);
        assert_eq!(select_playout(&e, &mut rng), vec![2, 0, 1]);
        assert_eq!(rng.requests, vec![(0, 2), (0, 1)]);
    }

    /// Attack/decay envelopes: a random attack sample first, the body, and a
    /// random decay sample last, with the native draw order.
    #[test]
    fn select_playout_attack_body_decay_order_and_draws() {
        let e = entry(
            "[T]\nSounds=a1 a2 a3 b1 b2 d1 d2 d3\nControl= random all attack decay\nAttack=3\nDecay=3\n",
        );
        let mut rng = ScriptedRng::new(&[1, 6, 1]);
        assert_eq!(select_playout(&e, &mut rng), vec![1, 4, 3, 6]);
        assert_eq!(rng.requests, vec![(0, 2), (5, 7), (0, 1)]);

        let single =
            entry("[T]\nSounds=a b1 b2 d\nControl= random attack decay\nAttack=1\nDecay=1\n");
        // Attack 1 and Decay 1 are equal-bounds draws: only the body draws.
        let mut rng = ScriptedRng::new(&[2]);
        assert_eq!(select_playout(&single, &mut rng), vec![0, 2, 3]);
        assert_eq!(rng.requests, vec![(1, 2)]);
    }

    fn clip(value: f32, frames: usize, sample_rate: u32) -> DecodedAudio {
        DecodedAudio {
            samples: vec![value; frames * 2],
            sample_rate,
            channels: 2,
            native_frame_bytes: 2,
        }
    }

    #[test]
    fn resolve_entry_playback_chains_samples_and_applies_the_shifts() {
        let e = entry("[T]\nSounds=a b c\nVolume=80\nControl= all\nFShift=10 10\nVShift=50\n");
        let mut rng = ScriptedRng::new(&[50]);
        let resolved = resolve_entry_playback(&e, &mut rng, |name| match name {
            "a" => Some(clip(0.1, 2, 22050)),
            "b" => None,
            "c" => Some(clip(0.3, 1, 22050)),
            _ => unreachable!(),
        })
        .expect("resolved");
        assert_eq!(resolved.decoded.samples, vec![0.1, 0.1, 0.1, 0.1, 0.3, 0.3]);
        assert_eq!(resolved.decoded.sample_rate, 22050 * 110 / 100);
        assert_eq!(rng.requests, vec![(0, 50)]);

        // A rate mismatch keeps the first clip only.
        let mut rng = ScriptedRng::new(&[0]);
        let resolved = resolve_entry_playback(&e, &mut rng, |name| match name {
            "a" => Some(clip(0.1, 1, 22050)),
            _ => Some(clip(0.2, 1, 11025)),
        })
        .expect("resolved");
        assert_eq!(resolved.decoded.samples, vec![0.1, 0.1]);

        // Nothing loadable resolves to nothing.
        assert!(resolve_entry_playback(&e, &mut rng, |_| None).is_none());
    }

    #[test]
    fn cloak_sound_registered_resolution_rejects_raw_sample_and_invalid_names() {
        let registry = SoundRegistry::from_ini(&IniFile::from_str(
            "[SoundList]\n1=NavalUnitEmerge\n[NavalUnitEmerge]\nSounds=vnavupa\nVolume=55\n",
        ));
        let entry = registered_entry("NavalUnitEmerge", &registry)
            .expect("the retail Voc/event identity resolves to its registered entry");
        assert_eq!(entry.sounds, vec!["vnavupa"]);
        assert_eq!(entry.volume_linear, 9011); // X87Chop53::ftol_f64_low_masked(0.55f * 16384)
        for invalid in ["", "MissingCloakEvent", "vnavupa"] {
            assert!(
                registered_entry(invalid, &registry).is_none(),
                "an invalid Voc identity must not become a raw-bag fallback: {invalid}"
            );
        }
    }

    fn test_decoded_audio() -> DecodedAudio {
        DecodedAudio {
            samples: vec![0.0, 0.0],
            sample_rate: 22_050,
            channels: 2,
            native_frame_bytes: 2,
        }
    }

    fn test_output_scales(
        sound_volume: f32,
        voice_volume: f32,
        lifecycle_scale: f32,
        focus_output_scale: f32,
    ) -> SfxOutputScales {
        SfxOutputScales {
            sound_volume,
            voice_volume,
            lifecycle_scale,
            focus_output_scale,
            // The limiter idles at full scale until the summed authored
            // `Volume=` budget of the live events passes 100.
            many_sounds_linear: VOLUME_SCALE,
        }
    }

    #[test]
    fn gsi_01_02_focus_gate_restores_distinct_live_sfx_gains_absolutely() {
        let inactive = test_output_scales(1.0, 0.4, 0.6, 0.0);
        let quiet = prepare_normal_sfx_output(test_decoded_audio(), 3277, inactive);
        let loud = prepare_normal_sfx_output(test_decoded_audio(), 12288, inactive);

        assert_eq!(quiet.initial_volume, 0.0);
        assert_eq!(loud.initial_volume, 0.0);
        let active = test_output_scales(1.0, 0.4, 0.6, 1.0);
        assert!(
            (quiet.gain.effective(active) - native_volume_amplitude(3277) * 0.6).abs()
                < f32::EPSILON
        );
        assert!(
            (loud.gain.effective(active) - native_volume_amplitude(12288) * 0.6).abs()
                < f32::EPSILON
        );
    }

    #[test]
    fn gsi_01_02_sfx_armed_while_inactive_retains_gain_for_restore() {
        // Construction while inactive still creates/starts the Player in the
        // production path; only this independently retained output gain is 0.
        let armed_while_inactive = prepare_normal_sfx_output(
            test_decoded_audio(),
            5734,
            test_output_scales(1.0, 0.4, 1.0, 0.0),
        );

        assert_eq!(armed_while_inactive.initial_volume, 0.0);
        assert!(
            (armed_while_inactive
                .gain
                .effective(test_output_scales(1.0, 0.4, 1.0, 1.0))
                - native_volume_amplitude(5734))
            .abs()
                < f32::EPSILON
        );
    }

    /// The user master is chained into the linear product before the
    /// DirectSound curve, so half master is -10 dB, not half amplitude.
    #[test]
    fn options_profile_production_routes_sound_and_eva_independently() {
        let half = native_volume_amplitude(8192);
        for (sound_volume, voice_volume, expected_sound, expected_voice) in
            [(0.0, 1.0, 0.0, half), (1.0, 0.0, half, 0.0)]
        {
            let scales = test_output_scales(sound_volume, voice_volume, 1.0, 1.0);
            let sound = prepare_normal_sfx_output(test_decoded_audio(), 8192, scales);
            let eva = prepare_eva_stream_output(test_decoded_audio(), 8192, scales);

            assert_eq!(sound.initial_volume, expected_sound);
            assert_eq!(eva.initial_volume, expected_voice);
        }
        let full = prepare_normal_sfx_output(
            test_decoded_audio(),
            VOLUME_SCALE,
            test_output_scales(0.5, 1.0, 1.0, 1.0),
        );
        assert_eq!(full.initial_volume, half);
    }

    /// A queued EVA node carries only the entry and its sample name; the
    /// sample is resolved and the Voice master applied when `PlayNextQueued`
    /// starts it, so the scales at dequeue alone decide the startup volume.
    #[test]
    fn options_profile_queued_eva_uses_current_voice_master_at_dequeue() {
        let started_with_voice_enabled = prepare_eva_stream_output(
            test_decoded_audio(),
            13107,
            test_output_scales(0.0, 1.0, 1.0, 1.0),
        );
        assert!(
            (started_with_voice_enabled.initial_volume - native_volume_amplitude(13107)).abs()
                < f32::EPSILON
        );
        let started_with_voice_muted = prepare_eva_stream_output(
            test_decoded_audio(),
            13107,
            test_output_scales(1.0, 0.0, 1.0, 1.0),
        );
        assert_eq!(started_with_voice_muted.initial_volume, 0.0);
    }

    /// The scenario exits fade the audio master `[0x0087E758]` to zero before
    /// queueing their announcement (`0x00686605` then `0x00686616`). That
    /// master is chained into Voc channels only, so a unit voice follows it
    /// and the EVA stream does not; the many-sounds scaler is Voc-only too.
    #[test]
    fn scenario_exit_master_fade_leaves_the_eva_stream_alone() {
        let full = native_volume_amplitude(13107);
        let faded = test_output_scales(1.0, 1.0, 0.25, 1.0);
        let voice = prepare_normal_sfx_output(test_decoded_audio(), 13107, faded);
        assert!((voice.initial_volume - full * 0.25).abs() < f32::EPSILON);
        let eva = prepare_eva_stream_output(test_decoded_audio(), 13107, faded);
        assert!((eva.initial_volume - full).abs() < f32::EPSILON);

        let silenced = test_output_scales(1.0, 1.0, 0.0, 1.0);
        assert_eq!(
            prepare_normal_sfx_output(test_decoded_audio(), 13107, silenced).initial_volume,
            0.0
        );
        assert!(
            (prepare_eva_stream_output(test_decoded_audio(), 13107, silenced).initial_volume
                - full)
                .abs()
                < f32::EPSILON
        );

        let limited = SfxOutputScales {
            many_sounds_linear: VOLUME_SCALE / 2,
            ..test_output_scales(1.0, 1.0, 1.0, 1.0)
        };
        assert!(
            prepare_normal_sfx_output(test_decoded_audio(), 13107, limited).initial_volume < full
        );
        assert!(
            (prepare_eva_stream_output(test_decoded_audio(), 13107, limited).initial_volume - full)
                .abs()
                < f32::EPSILON
        );

        // The foreground gate is the primary buffer: it still silences both.
        let background = test_output_scales(1.0, 1.0, 1.0, 0.0);
        assert_eq!(
            prepare_eva_stream_output(test_decoded_audio(), 13107, background).initial_volume,
            0.0
        );
    }

    #[test]
    fn the_taunt_stream_takes_no_master_and_no_exit_fade() {
        let silent_masters = test_output_scales(0.0, 0.0, 0.0, 1.0);
        let gain = SfxOutputGain::new(VOLUME_SCALE, SfxChannel::TauntStream);
        assert_eq!(
            gain.effective(silent_masters),
            native_volume_amplitude(VOLUME_SCALE)
        );
        // The foreground gate is the primary buffer: it still silences it.
        assert_eq!(gain.effective(test_output_scales(1.0, 1.0, 1.0, 0.0)), 0.0);
    }

    /// Original action21 lookup/admission controls, through the process's
    /// production retail audio reader and the actual Sfx admission owner.
    /// The captured stream is paused; no hardware completion is inferred.
    #[test]
    fn retail_trigger_indices_and_named_eva_share_sfx_admission_including_empty_columns() {
        let Some((_root, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let Some(mut player) = SfxPlayer::new() else {
            return;
        };
        let definitions = crate::rules::retail_ini_fixture::retail_audio_definitions()
            .expect("retail audio definitions");
        assert_eq!(definitions.eva().len(), 470);
        let rows: Vec<serde_json::Value> = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/trigger_action_values.json",
        ))
        .unwrap();
        let mut compared = 0;
        for row in rows
            .iter()
            .filter(|row| row["case_kind"] == "eva_read_dispatch")
        {
            let index = row["value"].as_i64().unwrap() as i32;
            let name = row["raw"].as_str().unwrap().split(',').nth(2).unwrap();
            let after = &row["after"];
            let expected_count = after["critical"].as_array().unwrap().len()
                + after["interrupts"].as_array().unwrap().len()
                + usize::from(!after["pending"].is_null())
                + (0..4)
                    .map(|priority| {
                        after["queues"][priority.to_string()]
                            .as_array()
                            .unwrap()
                            .len()
                    })
                    .sum::<usize>();
            assert!(expected_count <= 1);
            let rules = crate::rules::ruleset::RuleSet::from_ini(
                &crate::rules::ini_parser::IniFile::from_str(
                    "[Sides]\nGDI=ONE\nNod=TWO\nThirdSide=THREE\n",
                ),
            )
            .unwrap();
            for side_index in 0..3 {
                player.set_eva_side(side_index, &rules);
                player.stop_all();
                player.set_paused(false, 1);
                player.set_paused(true, 1);
                assert_eq!(
                    player.queue_eva(
                        index,
                        Some(EvaType::Interrupt),
                        None,
                        definitions.eva(),
                        &assets,
                        None
                    ),
                    expected_count != 0,
                    "{} side{side_index}",
                    row["raw"],
                );
                assert_eq!(player.vox.queued_count(), expected_count);
                assert!(player.eva_player.is_none());
                // The named route resolves to the same entry already queued
                // by its index, so its duplicate call cannot create a copy.
                assert!(!player.play_eva(
                    name,
                    Some(EvaType::Interrupt),
                    definitions.eva(),
                    &assets,
                    None
                ));
                assert_eq!(player.vox.queued_count(), expected_count);
                player.set_paused(false, 1);
                if expected_count != 0 {
                    let node = player.vox.take_next(1, false).unwrap();
                    assert_eq!(
                        node.event,
                        row["entry"]["name"].as_str().unwrap().to_ascii_uppercase()
                    );
                    assert_eq!(node.eva_type, EvaType::Interrupt);
                    assert_eq!(
                        node.priority as i32,
                        row["entry"]["priority"].as_i64().unwrap() as i32
                    );
                    assert_eq!(
                        player
                            .vox
                            .sample_for(&node, definitions.eva())
                            .unwrap_or(""),
                        row["entry"]["columns"][side_index.to_string()]
                            .as_str()
                            .unwrap()
                    );
                }
                assert_eq!(player.vox.queued_count(), 0);
                assert!(player.eva_player.is_none());
                compared += 1;
            }
        }
        assert_eq!(compared, 45);
    }

    // One test per master setting, so their real-time playbacks overlap.
    #[test]
    fn reached_unit_voice_uses_sound_master_with_sound_muted() {
        assert_reached_unit_voice_uses_sound_master(1_000, 0.0, 1.0, false);
    }

    #[test]
    fn reached_unit_voice_uses_sound_master_with_voice_muted() {
        assert_reached_unit_voice_uses_sound_master(2_000, 1.0, 0.0, true);
    }

    /// The original ordinary event binds channel+94 to SoundVolume at
    /// 405B91..405B96, also for Techno's unity-gain call at6F9EE5..6F9EF0.
    /// Exercise actual queued/reached playback, live setters and device pulls
    /// with opposite masters; equal defaults would hide a wrong group.
    fn assert_reached_unit_voice_uses_sound_master(
        now: u64,
        sound_volume: f64,
        voice_volume: f64,
        audible: bool,
    ) {
        let Some((_root, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let Some(mut player) = SfxPlayer::new() else {
            return;
        };
        let definitions = crate::rules::retail_ini_fixture::retail_audio_definitions()
            .expect("retail audio definitions");
        let loaded_index = crate::rules::retail_ini_fixture::retail_audio_index();
        let index = loaded_index.map(|loaded| &loaded.index);
        let mut service = TestAudioService::default();
        player.set_sound_volume(sound_volume);
        player.set_voice_volume(voice_volume);
        player
            .observe_pcm(PcmObservationConfig {
                sound_ids: vec!["GIMove".into()],
                max_events: 1,
                max_samples_per_event: 262_144,
            })
            .unwrap();
        player.queue_unit_voice(7, "GIMove");
        player.visit_unit_voice(7, definitions.sounds());
        service.pump(
            &mut player,
            now,
            definitions.sounds(),
            definitions.eva(),
            &assets,
            index,
        );
        assert!(player.play_eva("EVA_UnitReady", None, definitions.eva(), &assets, index));
        assert_eq!(
            player.eva_player.as_ref().unwrap().player.volume() > 0.0,
            voice_volume > 0.0,
            "EVA retains its independent VoiceVolume group"
        );
        let event = player
            .arbiter
            .validate_loop_handle(HandleOwner::UnitVoice(7))
            .expect("reached head has an admitted handle");
        let started_gain = player.live[&event].player.volume();
        assert_eq!(
            started_gain > 0.0,
            audible,
            "unit voice must use SoundVolume={sound_volume}, not VoiceVolume={voice_volume}"
        );
        player.set_voice_volume(1.0 - voice_volume);
        assert_eq!(player.live[&event].player.volume(), started_gain);
        assert_eq!(
            player.eva_player.as_ref().unwrap().player.volume() > 0.0,
            voice_volume == 0.0
        );
        player.set_voice_volume(voice_volume);
        if audible {
            player.set_sound_volume(0.0);
            assert_eq!(player.live[&event].player.volume(), 0.0);
            player.set_sound_volume(sound_volume);
            assert_eq!(player.live[&event].player.volume(), started_gain);
        }
        player.live[&event].player.sleep_until_end();
        player.report_finished_outputs();
        let report = player.finish_pcm_observation().unwrap();
        assert!(!report.truncated);
        assert_eq!(report.outputs.len(), 1);
        let output = &report.outputs[0];
        assert!(output.sample_bits.len() >= output.source_sample_count);
        assert!(output.source_sample_count > 0);
        assert!(
            output
                .sample_bits
                .iter()
                .all(|bits| f32::from_bits(*bits).is_finite())
        );
        let nonzero = output
            .sample_bits
            .iter()
            .filter(|bits| f32::from_bits(**bits) != 0.0)
            .count();
        assert_eq!(nonzero > 0, audible, "post-Player device pulls");
        eprintln!(
            "GI SoundVolume={sound_volume} VoiceVolume={voice_volume}: {} pulls, {nonzero} nonzero",
            output.sample_bits.len()
        );
        player.stop_all();
        // The same player re-arms observation and admits the voice again
        // after the hard stop.
        player
            .observe_pcm(PcmObservationConfig {
                sound_ids: vec!["GIMove".into()],
                max_events: 1,
                max_samples_per_event: 262_144,
            })
            .unwrap();
        player.queue_unit_voice(7, "GIMove");
        player.visit_unit_voice(7, definitions.sounds());
        service.pump(
            &mut player,
            now + 1_000,
            definitions.sounds(),
            definitions.eva(),
            &assets,
            index,
        );
        assert!(
            player
                .arbiter
                .validate_loop_handle(HandleOwner::UnitVoice(7))
                .is_some()
        );
        player.stop_all();
    }

    /// Original two-E1 QueueVoice708D90 -> AI6F9EBB -> PlayAtPos750920
    /// admits two distinct state-0 SoundEvents before either sample starts.
    #[test]
    fn two_e1_acknowledgements_hold_two_shared_pool_events() {
        let Some(definitions) = crate::rules::retail_ini_fixture::retail_audio_definitions() else {
            return;
        };
        let Some(mut player) = SfxPlayer::new() else {
            return;
        };
        player.set_volume(0.0);
        player.queue_unit_voice(3, "GIMove");
        player.queue_unit_voice(2, "GIMove");
        player.visit_unit_voice(2, definitions.sounds());
        player.visit_unit_voice(3, definitions.sounds());
        assert_eq!(
            player.arbiter.live_event_count(),
            2,
            "both Techno+4DC handles must own shared-pool events before service"
        );
        assert_eq!(player.arbiter.busy_channel_count(), 0);
        player.stop_all();
    }

    /// A unit acknowledgement starting while an announcement sounds leaves
    /// the announcement playing: native's `StreamPlayer` is not one of the Voc
    /// channels `VocClass::PlayAtPos @ 0x00750920` takes, and only
    /// `VoxClass` itself (`QueueVoice` type 2, `ResetAll`) stops it. Needs
    /// retail assets and an audio device; skips without either.
    #[test]
    fn unit_voice_does_not_cut_eva_stream() {
        let Some((_root, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let Some(mut player) = SfxPlayer::new() else {
            return;
        };
        player.set_volume(0.0);
        let mut service = TestAudioService::default();
        let definitions = crate::rules::retail_ini_fixture::retail_audio_definitions()
            .expect("retail audio definitions");
        let loaded_index = crate::rules::retail_ini_fixture::retail_audio_index();
        let audio_index = loaded_index.map(|loaded| &loaded.index);
        // `PlayNextQueued` needs `now > end_time + gap`, so the clock must
        // have moved off zero before the first line can start.
        service.pump(
            &mut player,
            1_000,
            definitions.sounds(),
            definitions.eva(),
            &assets,
            audio_index,
        );
        assert!(player.play_eva(
            "EVA_UnitReady",
            None,
            definitions.eva(),
            &assets,
            audio_index
        ));
        assert!(player.eva_stream_busy());
        let eva_current = player.vox.current().cloned();
        assert!(eva_current.is_some());

        player.queue_unit_voice(7, "GISelect");
        player.visit_unit_voice(7, definitions.sounds());
        assert!(
            player
                .arbiter
                .validate_loop_handle(HandleOwner::UnitVoice(7))
                .is_some()
        );
        assert_eq!(player.voice_queue.playing_for(7), Some("GISELECT"));
        assert!(player.eva_stream_busy(), "unit voice cut the EVA stream");
        assert_eq!(player.vox.current().cloned(), eva_current);
        assert!(player.voices_active());

        // The queue's own stop path is the only one that cuts the stream, and
        // it leaves the unit line alone.
        player.stop_eva_stream();
        assert!(!player.eva_stream_busy());
        assert!(
            player
                .arbiter
                .validate_loop_handle(HandleOwner::UnitVoice(7))
                .is_some()
        );
        player.stop_all();
    }

    /// Whole stock UnitReady waveform from original40AA70 callback returns,
    /// literal sample writes and409DE0 copy ranges. The portable factory oracle
    /// selects those bytes once; this drives the actual EVAMD/MIX/index/decoder
    /// path before presentation gain, with no second decoder or Rust golden.
    #[test]
    fn retail_unit_ready_pcm_matches_original_decode_and_buffer_writes() {
        let Some((_root, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let fixture = crate::rules::retail_ini_fixture::factory_unit_ready_native();
        let native = &fixture["controls"]["buffer"]["decoded_pcm"];
        let selected = &fixture["controls"]["buffer"]["registered_prior"]["notification_suffix"][0]
            ["after"]["selected"][0];
        let definitions = crate::rules::retail_ini_fixture::retail_audio_definitions()
            .expect("retail audio definitions");
        let entry = definitions.eva().entry("EVA_UnitReady").unwrap();
        let sample = entry.column(EvaSide::Allied).unwrap();
        assert_eq!(sample, selected["allied"].as_str().unwrap());
        let loaded_index = crate::rules::retail_ini_fixture::retail_audio_index();
        let decoded = load_sfx(sample, &assets, loaded_index.map(|loaded| &loaded.index))
            .expect("production stock UnitReady decoder");
        assert_eq!(
            u64::from(decoded.sample_rate),
            native["samples_per_second"].as_u64().unwrap()
        );
        assert_eq!(native["channels"], 1, "original mono source");
        assert_eq!(native["sample_bytes"], 2, "original signed PCM16 output");
        assert_eq!(decoded.channels, 2, "presentation upmix");
        assert_eq!(decoded.samples.len() % 2, 0);
        // The production owner represents each i16 exactly as f32/32768 and
        // duplicates mono into stereo. Undo only that lossless representation;
        // the expected length and every expected sample remain native outputs.
        let actual: Vec<u8> = decoded
            .samples
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|pair| {
                assert_eq!(pair[0].to_bits(), pair[1].to_bits(), "mono upmix");
                let scaled = pair[0] * 32768.0;
                let sample = scaled as i16;
                assert_eq!(scaled, f32::from(sample), "lossless PCM representation");
                sample.to_le_bytes()
            })
            .collect();
        let hex = native["bytes_hex"].as_str().unwrap();
        assert_eq!(hex.len() % 2, 0);
        let expected: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|offset| u8::from_str_radix(&hex[offset..offset + 2], 16).unwrap())
            .collect();
        assert_eq!(actual.len() as u64, native["bytes"].as_u64().unwrap());
        assert_eq!(actual, expected, "all original callback/copy PCM bytes");
        assert_eq!(
            crate::util::sha256::sha256_hex(&actual),
            native["sha256"].as_str().unwrap()
        );
    }

    #[test]
    #[ignore = "requires active-retail RA2_DIR and an audio output device"]
    fn retail_squid_one_shot_device_release_detach_and_hard_stop() {
        let mut service = TestAudioService::default();
        use std::io::Write;
        use std::time::{Duration, Instant};

        // Original gattling_loop shared one-shot controls establish Release
        // 406060 finishing the current buffer, while generic405FD0 requests
        // DriverStop at the next state3 service. Stop405D40 acts immediately.
        // This is the production device consumer, not a supplied native channel
        // callback or an invented idle-lapse trigger. Sample RNG stays untouched.
        let (_root, assets) = crate::rules::retail_ini_fixture::retail_assets()
            .expect("explicit device witness requires active-retail assets");
        let definitions = crate::rules::retail_ini_fixture::retail_audio_definitions()
            .expect("retail audio definitions");
        let loaded_index = crate::rules::retail_ini_fixture::retail_audio_index();
        let audio_index = loaded_index.map(|loaded| &loaded.index);
        let entry = definitions.sounds().get("SquidMove").unwrap();
        assert_eq!(entry.control, control::RANDOM);
        assert_eq!(entry.loop_count, 0);
        assert_eq!(entry.sounds, ["vsqumova", "vsqumovb"]);
        let mut player = SfxPlayer::new().expect("explicit witness requires an output device");
        let epoch = Instant::now();
        let mut reports = Vec::new();
        for operation in ["release", "detach", "hard_stop"] {
            player.stop_all();
            player
                .observe_pcm(PcmObservationConfig {
                    sound_ids: vec!["SquidMove".to_owned()],
                    max_events: 1,
                    max_samples_per_event: 262_144,
                })
                .unwrap();
            assert!(player.play_animation_sound_spatial(
                7_u64,
                "SquidMove",
                SpatialGain::CENTRED_FULL,
                definitions.sounds()
            ));
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                service.pump(
                    &mut player,
                    epoch.elapsed().as_millis() as u64,
                    definitions.sounds(),
                    definitions.eva(),
                    &assets,
                    audio_index,
                );
                if player.pcm_observer.as_ref().unwrap().pulled_nonzero() {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "{operation}: real device did not pull nonzero PCM"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            let before = player.pcm_observer.as_ref().unwrap().recorded_samples();
            match operation {
                "release" => player.release_animation_sound(7_u64),
                "detach" => player.detach_animation_sound(7_u64),
                _ => player.stop_animation_sound(7_u64),
            }
            assert!(player.handle_sound_id(7).is_none());
            while !player.observed_pcm_settled() {
                service.pump(
                    &mut player,
                    epoch.elapsed().as_millis() as u64,
                    definitions.sounds(),
                    definitions.eva(),
                    &assets,
                    audio_index,
                );
                assert!(
                    Instant::now() < deadline,
                    "{operation}: real device queue did not end"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            let report = player.finish_pcm_observation().unwrap();
            assert!(!report.truncated);
            assert_eq!(report.outputs.len(), 1);
            let output = &report.outputs[0];
            assert!(output.source_ended && !output.truncated);
            assert_eq!(output.owner, Some(7));
            assert_eq!(output.resolved_samples.len(), 1);
            assert!(entry.sounds.contains(&output.resolved_samples[0]));
            assert!(
                output
                    .sample_bits
                    .iter()
                    .all(|bits| f32::from_bits(*bits).is_finite())
            );
            if operation != "release" {
                assert!(
                    output.sample_bits.len() < output.source_sample_count,
                    "device continued the entire one-shot after {operation}"
                );
                assert!(
                    matches!(output.actions.last().unwrap().kind, "stopped" | "completed"),
                    "device completion and next state3 retirement may race"
                );
            } else {
                assert!(output.sample_bits.len() >= output.source_sample_count);
                assert!(
                    output.sample_bits[before..]
                        .iter()
                        .any(|bits| f32::from_bits(*bits) != 0.0),
                    "current playout must remain audible after {operation}"
                );
                assert_eq!(output.actions.last().unwrap().kind, "completed");
            }
            eprintln!(
                "SquidMove {operation}: sample={} before={before} pulled={} decoded={} source_ended={}",
                output.resolved_samples[0],
                output.sample_bits.len(),
                output.source_sample_count,
                output.source_ended
            );
            reports.push(
                serde_json::json!({"operation": operation, "samples_before_operation": before,
                "observation": report.into_json()}),
            );
        }
        player.queue_unit_voice(7, "SquidSelect");
        player.stop_all();
        assert!(player.voice_queue.pending_for(7).is_none());
        assert!(player.voice_queue.playing_for(7).is_none());
        assert!(player.looping_owners().is_empty());
        if let Some(path) = std::env::var_os("VERA20K_SFX_PCM_TEST_OUTPUT") {
            let path = std::path::PathBuf::from(path);
            assert!(
                path.is_absolute(),
                "witness output must name a scratch absolute path"
            );
            let bytes = serde_json::to_vec(
                &serde_json::json!({"schema_version": "vera20k.sfx-device-control.v1",
                "point": "post_player_pre_device_mixer", "controls": reports}),
            )
            .unwrap();
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .unwrap()
                .write_all(&bytes)
                .unwrap();
        }
    }

    /// An above-floor entry through the production player and device. The
    /// first start pass parks the event's sample record in its live output.
    /// The ring's sample-end lookahead draws OnChannelSampleEnd's gap
    /// (`0x00405AC0`). The drained output hands it to OnChannelDone
    /// (`0x00405A00`), which leaves the event waiting in state 2 with the
    /// record back in `pending`. The next start pass then plays the record's
    /// next queued sample on the same token. Retail `_Amb_OceanLight` is
    /// RANDOM|ALL with four samples, so its first pass leaves three.
    /// Device-backed; ordinary lib CI has no device.
    #[test]
    #[ignore = "requires active-retail RA2_DIR and an audio output device"]
    fn retail_above_floor_gap_parks_the_record_until_the_next_start_pass() {
        use std::time::{Duration, Instant};
        let mut service = TestAudioService::default();
        let (_root, assets) = crate::rules::retail_ini_fixture::retail_assets()
            .expect("explicit device witness requires active-retail assets");
        let definitions = crate::rules::retail_ini_fixture::retail_audio_definitions()
            .expect("retail audio definitions");
        let loaded_index = crate::rules::retail_ini_fixture::retail_audio_index();
        let audio_index = loaded_index.map(|loaded| &loaded.index);
        let sounds = definitions.sounds();
        let entry = sounds.get("_Amb_OceanLight").unwrap();
        assert_eq!(
            entry.control,
            control::RANDOM | control::LOOP | control::ALL | control::AMBIENT
        );
        assert_eq!((entry.delay_ms, entry.sounds.len()), ((2000, 4000), 4));
        let mut player = SfxPlayer::new().expect("explicit witness requires an output device");
        assert!(player.play_animation_sound_spatial(
            7_u64,
            "_Amb_OceanLight",
            SpatialGain::CENTRED_FULL,
            sounds
        ));
        let event = player
            .arbiter
            .validate_loop_handle(HandleOwner::Positional(7))
            .unwrap();
        let token = player.arbiter.event_token(event).unwrap();
        let epoch = Instant::now();
        let deadline = epoch + Duration::from_secs(40);
        let mut pump = |player: &mut SfxPlayer| {
            service.pump(
                player,
                epoch.elapsed().as_millis() as u64,
                sounds,
                definitions.eva(),
                &assets,
                audio_index,
            );
            assert!(Instant::now() < deadline, "the device did not progress");
            std::thread::sleep(Duration::from_millis(5));
        };
        let parked_queue = |player: &SfxPlayer| {
            let output = &player.live[&event];
            assert_eq!(output.token, Some(token));
            output.parked.as_ref().unwrap().queue.count()
        };

        while !player.live.contains_key(&event) {
            pump(&mut player);
        }
        assert!(!player.pending.contains_key(&event));
        assert_eq!(parked_queue(&player), 3);

        while player.live.contains_key(&event) {
            pump(&mut player);
        }
        assert_eq!(
            player.arbiter.native_event_snapshot(event).unwrap()["state"],
            2,
            "OnChannelDone keeps the event through its gap"
        );
        assert_eq!(player.pending[&event].token, token);

        while !player.live.contains_key(&event) {
            pump(&mut player);
        }
        assert_eq!(parked_queue(&player), 2);
        player.stop_all();
    }

    /// Original AudioSystem406F70 with physical EVAMD DialogList62 and
    /// ceva062.WAV: paused queue at 1000, Unpause753620, no dequeue at 1033,
    /// actual PlayNext752760/PlayFile407B60 at 1034, payload completion,
    /// the strict inter-line gap and cleanup. All compared outcomes come from
    /// the factory oracle's single original-byte consumer selector.
    /// Device-backed production consumer check; ordinary lib CI has no device.
    #[test]
    #[ignore = "requires active-retail RA2_DIR and an audio output device"]
    fn retail_unit_ready_waits_for_the_native_periodic_audio_service() {
        let mut service = TestAudioService::default();
        let fixture = crate::rules::retail_ini_fixture::factory_unit_ready_native();
        let step = |control: &str, label: &str| {
            fixture["controls"][control]["steps"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["label"] == label)
                .expect("selected original consumer boundary")
        };
        // Adapt this control's explicit OS clock input to milliseconds. The
        // original clock initializer/division and its returned value are
        // independently retained in registered_prior; this supplies inputs,
        // never a playback, queue or completion answer.
        let now_ms = |row: &serde_json::Value| {
            let prior = &row["clock_prior"];
            assert_eq!(prior["frequency"], 1_000_000);
            prior["counter"].as_u64().unwrap() / 1_000
        };
        let compare = |player: &SfxPlayer, row: &serde_json::Value, registry: &EvaRegistry| {
            let after = &row["after"];
            let label = row["label"].as_str().unwrap();
            assert_eq!(
                player.vox.queued_count(),
                usize::from(after["pending_standard_is_set"].as_bool().unwrap()),
                "{label}: native pending slot"
            );
            assert_eq!(
                player.vox.current().is_some(),
                after["current_is_set"].as_bool().unwrap(),
                "{label}: native current entry"
            );
            assert_eq!(
                player.vox.dequeue_allowed(),
                after["pause_depth"] == 0,
                "{label}: native pause depth"
            );
            let stream_open = after["stream"]["flags"].as_u64().unwrap() != 0;
            assert_eq!(
                player.eva_player.is_some(),
                stream_open,
                "{label}: native stream"
            );
            let selected = &after["selected"][0];
            if let Some(current) = player.vox.current() {
                assert_eq!(
                    current.event,
                    selected["name"].as_str().unwrap().to_ascii_uppercase()
                );
                assert_eq!(
                    player.vox.sample_for(current, registry).unwrap_or(""),
                    selected["allied"].as_str().unwrap()
                );
            }
            // The stream is not a Voc channel: the shared Voc pool stays free.
            assert!(
                player.arbiter.live_event_count() == 0,
                "{label}: EVA line in the Voc pool"
            );
        };
        let selected = &fixture["controls"]["cadence"]["registered_prior"]["clock_steps"][0]["after"]
            ["selected"][0];
        let (_root, assets) = crate::rules::retail_ini_fixture::retail_assets()
            .expect("this explicit consumer check requires active-retail assets");
        let definitions = crate::rules::retail_ini_fixture::retail_audio_definitions()
            .expect("retail audio definitions");
        let loaded_index = crate::rules::retail_ini_fixture::retail_audio_index();
        let audio_index = loaded_index.map(|loaded| &loaded.index);
        let entry = definitions.eva().entry("EVA_UnitReady").unwrap();
        assert_eq!(entry.column(EvaSide::Allied), Some("ceva062"));
        assert_eq!(entry.eva_type, EvaType::Standard);
        assert_eq!(entry.priority, crate::rules::sound_ini::EvaPriority::Low);
        assert_eq!(entry.name, selected["name"].as_str().unwrap());
        assert_eq!(
            entry.volume.to_bits(),
            selected["volume_f32_bits"].as_u64().unwrap() as u32
        );
        let (wav, source) = assets.get_with_source("ceva062.wav").unwrap();
        assert_eq!(wav.len(), 9354, "physical native consumer input");
        assert_eq!(
            crate::util::sha256::sha256_hex(&wav),
            "6567dae06a071ab4985cad0b20b0c2596f923baf04a6bec097c8a9f0d937ee67"
        );
        eprintln!(
            "UnitReady WAV source={source}, sha256={}",
            crate::util::sha256::sha256_hex(&wav)
        );

        let mut player = SfxPlayer::new().expect("this explicit check requires an audio device");
        // Keep the test's private output inaudible; it still creates the real
        // decoder, mixer Player and stream queue without changing app settings.
        player.set_voice_volume(0.0);
        let paused = step("cadence", "paused_whole_periodic_service_1000ms");
        player.set_paused(true, now_ms(paused));
        assert!(player.play_eva(
            "EVA_UnitReady",
            None,
            definitions.eva(),
            &assets,
            audio_index
        ));
        service.pump(
            &mut player,
            now_ms(paused),
            definitions.sounds(),
            definitions.eva(),
            &assets,
            audio_index,
        );
        compare(&player, paused, definitions.eva());
        let unpaused = step("cadence", "original_unpause_keeps_native_pending");
        player.set_paused(false, now_ms(unpaused));
        compare(&player, unpaused, definitions.eva());
        for label in [
            "whole_periodic_service_exact_33ms",
            "whole_periodic_service_34ms_real_stock_wave",
        ] {
            let native = step("cadence", label);
            service.pump(
                &mut player,
                now_ms(native),
                definitions.sounds(),
                definitions.eva(),
                &assets,
                audio_index,
            );
            compare(&player, native, definitions.eva());
        }
        // Let the actual stock decoder/mixer queue finish. The controlled
        // wall-clock input2089 matches the native completed-stream witness;
        // no simulated playback-completion answer is supplied to this owner.
        player.eva_player.as_ref().unwrap().player.sleep_until_end();
        assert!(!player.eva_stream_busy());
        for label in [
            "original_payload_worker_2089ms",
            "original_payload_end_exact_gap_queue_visit",
        ] {
            let native = step("buffer", label);
            service.pump(
                &mut player,
                now_ms(native),
                definitions.sounds(),
                definitions.eva(),
                &assets,
                audio_index,
            );
            compare(&player, native, definitions.eva());
        }
        // The explicit voice wait-loop caller advances independently of the
        // periodic pass. Native752760 at2590 retires current/state2 even though
        // that is only1ms after the last periodic pass.
        let retired = step("buffer", "original_payload_end_after_gap_queue_visit");
        player.set_paused(false, now_ms(retired));
        assert!(!player.pump_and_check_voices(definitions.eva(), &assets, audio_index));
        compare(&player, retired, definitions.eva());
        player.stop_all();
        assert_eq!(player.vox.queued_count(), 0);
        assert!(player.vox.current().is_none());
        assert!(!player.voices_active());
        assert!(player.arbiter.live_event_count() == 0);
    }

    fn build_test_wav(sample_rate: u32, bits: u16, channels: u16, samples: &[u8]) -> Vec<u8> {
        let data_size: u32 = samples.len() as u32;
        let fmt_size: u32 = 16;
        let byte_rate: u32 = sample_rate * channels as u32 * bits as u32 / 8;
        let block_align: u16 = channels * bits / 8;
        let riff_size: u32 = 4 + (8 + fmt_size) + (8 + data_size);

        let mut wav: Vec<u8> = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&riff_size.to_le_bytes());
        wav.extend_from_slice(b"WAVE");
        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&fmt_size.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&channels.to_le_bytes());
        wav.extend_from_slice(&sample_rate.to_le_bytes());
        wav.extend_from_slice(&byte_rate.to_le_bytes());
        wav.extend_from_slice(&block_align.to_le_bytes());
        wav.extend_from_slice(&bits.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_size.to_le_bytes());
        wav.extend_from_slice(samples);
        wav
    }

    #[test]
    fn test_decode_wav_16bit_mono() {
        // 4 samples of 16-bit mono silence — upmixed to 8 stereo f32 values.
        let pcm: Vec<u8> = vec![0, 0, 0, 0, 0, 0, 0, 0];
        let wav = build_test_wav(22050, 16, 1, &pcm);
        let decoded = decode_wav(&wav, "test.wav").expect("should decode");
        assert_eq!(decoded.sample_rate, 22050);
        assert_eq!(decoded.channels, 2);
        assert_eq!(decoded.samples.len(), 8); // 4 mono → 4 stereo pairs
    }

    /// Build a `wFormatTag == 0x11` (IMA ADPCM) WAV around a raw block payload.
    fn build_test_ima_wav(
        sample_rate: u32,
        channels: u16,
        block_align: u16,
        blocks: &[u8],
    ) -> Vec<u8> {
        let data_size: u32 = blocks.len() as u32;
        let fmt_size: u32 = 20; // WAVEFORMATEX + cbSize(2) + wSamplesPerBlock(2)
        let riff_size: u32 = 4 + (8 + fmt_size) + (8 + data_size);
        let samples_per_block: u16 = 1 + (block_align - 4 * channels) * 2 / channels;

        let mut wav: Vec<u8> = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&riff_size.to_le_bytes());
        wav.extend_from_slice(b"WAVE");
        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&fmt_size.to_le_bytes());
        wav.extend_from_slice(&0x0011u16.to_le_bytes()); // wFormatTag
        wav.extend_from_slice(&channels.to_le_bytes());
        wav.extend_from_slice(&sample_rate.to_le_bytes());
        wav.extend_from_slice(&(sample_rate / 2).to_le_bytes()); // nAvgBytesPerSec
        wav.extend_from_slice(&block_align.to_le_bytes()); // psVar6[6] @ 0x00408610
        wav.extend_from_slice(&4u16.to_le_bytes()); // wBitsPerSample
        wav.extend_from_slice(&2u16.to_le_bytes()); // cbSize
        wav.extend_from_slice(&samples_per_block.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_size.to_le_bytes());
        wav.extend_from_slice(blocks);
        wav
    }

    /// The retail final block of AUDIOMD `ABIRJ01A` (bag offset 11,770,646 +
    /// 28 x 512): the same bytes the bag path decodes, run through the WAV path.
    const ABIRJ01A_TAIL_BLOCK: &[u8] = &[
        0xf8, 0xff, 0x01, 0x00, 0x17, 0xbc, 0x33, 0xa1, 0x1b, 0x00, 0x90, 0x3a, 0xb5, 0x2c, 0x92,
        0x10, 0xb2, 0x9c, 0x32, 0xb3, 0x2d, 0x21, 0xcb, 0x79, 0xa1, 0x0a, 0x31, 0xc0, 0x1a, 0x23,
        0xbb, 0x4b, 0xb2, 0x1b, 0x13, 0x11, 0xd9, 0x39, 0x92, 0xa9, 0x30, 0x90, 0x0a, 0x11, 0x11,
        0x1a, 0x91, 0x09, 0x11, 0x09, 0x00, 0x00,
    ];

    #[test]
    fn decode_wav_ima_adpcm_mono_matches_the_shared_block_decoder() {
        let wav = build_test_ima_wav(22050, 1, 52, ABIRJ01A_TAIL_BLOCK);
        let decoded = decode_wav(&wav, "ima.wav").expect("format tag 0x11 must decode");
        assert_eq!(decoded.sample_rate, 22050);
        assert_eq!(decoded.channels, 2, "mono is upmixed for playback");
        // 97 mono samples duplicated into stereo pairs.
        assert_eq!(decoded.samples.len(), 97 * 2);
        let want = crate::assets::ima_adpcm::decode_blocks(ABIRJ01A_TAIL_BLOCK, 1, 52);
        for (i, &s) in want.iter().enumerate() {
            let f = s as f32 / 32768.0;
            assert_eq!(decoded.samples[i * 2], f, "left at {i}");
            assert_eq!(decoded.samples[i * 2 + 1], f, "right at {i}");
        }
    }

    #[test]
    fn decode_wav_ima_adpcm_stereo_keeps_the_channels_apart() {
        // Two channels whose preambles differ, so a collapsed or swapped
        // interleave cannot pass. 8-byte preamble pair + one 8-byte L/R group.
        let mut block: Vec<u8> = vec![0x00, 0x10, 0x00, 0x00, 0x00, 0xF0, 0x00, 0x00];
        block.extend_from_slice(&[0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x00]);
        let wav = build_test_ima_wav(22050, 2, 16, &block);
        let decoded = decode_wav(&wav, "ima2.wav").expect("stereo IMA must decode");
        assert_eq!(decoded.channels, 2);
        // 1 preamble frame + 8 group frames = 9 frames.
        assert_eq!(decoded.samples.len(), 18);
        assert_eq!(decoded.samples[0], 0x1000 as f32 / 32768.0);
        assert_eq!(decoded.samples[1], -4096.0 / 32768.0);
        let want = crate::assets::ima_adpcm::decode_blocks(&block, 2, 16);
        let got: Vec<i16> = decoded
            .samples
            .iter()
            .map(|&f| (f * 32768.0) as i16)
            .collect();
        assert_eq!(got, want);
    }
}
