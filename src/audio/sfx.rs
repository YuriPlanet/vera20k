//! Sound effect (SFX) playback using rodio.
//!
//! Plays short one-shot sounds triggered by game events: weapon fire, unit
//! voice responses, building placement, death explosions. Uses the SoundRegistry
//! (from sound.ini) to resolve sound IDs to .wav/.aud filenames, then loads
//! and plays them through rodio.
//!
//! ## Design
//! - Sample selection, pitch/volume shift, positional volume, stereo pan and
//!   the DirectSound loudness curve are reproduced from `gamemd.exe`
//!   (`VocClass`, `SoundEvent`, `DSoundBuffer`); see the provenance comments
//!   on each helper.
//! - **Which cue gets one of the 16 channels is not decided here.** A play
//!   request is decoded, then submitted to [`arbiter::SoundArbiter`], which
//!   owns the native `SoundSystem::UpdateTick @ 0x004041D0` pass: the channel
//!   pool, `Priority=` arbitration, `Limit=`, the pre-delay wait, the looping
//!   leash and the volume ramps. This file applies the arbiter's
//!   [`arbiter::ArbiterAction`]s to rodio and nothing more, so the decision
//!   half stays reachable from `cargo test --lib` where there is no device.
//! - The service pass runs from [`SfxPlayer::pump`], which the app calls every
//!   frame regardless of whether the simulation stepped — native's
//!   `AudioSystem::Pump @ 0x00406F70` hangs off `Network_ServiceLoop @
//!   0x0048D080`, not the sim.
//!
//! ## Dependency rules
//! - Part of audio/ — depends on assets/ (AssetManager for .wav/.aud loading),
//!   rules/sound_ini (SoundRegistry for ID→filename mapping).
//! - Does NOT depend on render/, ui/, sidebar/, sim/.

use std::collections::BTreeMap;
use std::num::NonZero;

use rodio::buffer::SamplesBuffer;
use rodio::{DeviceSinkBuilder, MixerDeviceSink, Player};

use crate::assets::asset_manager::AssetManager;
use crate::assets::aud_file;
use crate::audio::arbiter::{self, ArbiterAction, EntryFacts, EventId, PlayRequest, SoundArbiter};
use crate::audio::voice_queue::VoiceQueue;
use crate::audio::vox::{VoxNode, VoxQueue, VoxRequest};
use crate::rules::sound_ini::{
    EvaRegistry, EvaSide, EvaType, SoundEntry, SoundRegistry, VOLUME_SCALE, control, sound_type,
};

/// How many passes of a sustaining cue are kept queued on its rodio player:
/// the one that is sounding plus one waiting behind it.
///
/// **VERA-internal, gamemd equivalent UNCHECKED.** Native never queues ahead:
/// `FUN_00405AC0`, installed at `ch+0xB4`, is the DirectSound
/// buffer-needs-data callback, and it calls
/// `SoundEvent::AdvancePlaylist @ 0x004047B0` the moment the device asks, so
/// the chain is gapless by construction. rodio's `Player` exposes no such
/// callback — only `append` and a queue length — so [`SfxPlayer::pump`] keeps
/// one pass queued behind the sounding one instead. Trigger for the
/// divergence: a `Loop=N` cue reads one pass further ahead than native does,
/// so its final pass is decoded (and its `Control=random` order drawn) one
/// pass earlier. Player effect: none audible; the same passes play in the
/// same order. Frequency: every looping cue. Downstream risk: the extra draw
/// shifts VERA's presentation RNG, which is a clock-seeded non-scenario
/// generator (`g_MainRng @ 0x00886B88`) and feeds no deterministic state.
const LOOP_QUEUE_DEPTH: usize = 2;

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

/// Truncating `Math::ftol @ 0x007C5F00` (control word `0x0E7F`: round toward
/// zero, 53-bit precision), applied to a double intermediate.
///
/// RESIDUAL (UNCHECKED) — out-of-`i32` inputs differ: `FISTP` stores the x87
/// indefinite `0x80000000` for both signs, while Rust's `as i32` saturates to
/// `i32::MIN` *or* `i32::MAX`. Trigger: a client point more than 2^31 pixels
/// from the view centre. Player effect: none — the largest YR map is under
/// 2^16 pixels across, so only a hand-built [`SpatialListener`] reaches it.
/// Frequency: never in play. Downstream risk: none.
fn ftol(value: f64) -> i32 {
    value.trunc() as i32
}

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
/// 60, `TILE_HEIGHT` 30 — so at `zoom == 1.0` `client_point` is the native
/// client point exactly.
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
            ftol(f64::from(screen_x) - f64::from(self.origin_x)),
            ftol(f64::from(screen_y) - f64::from(self.origin_y)),
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
        ftol(f64::from(self.volume) * f64::from(VOLUME_SCALE)).min(VOLUME_SCALE)
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
    let mut dist_x: f32 = native_abs(ftol(f64::from(client_x) - f64::from(half_w))) as f32;
    let mut dist_y: f32 = native_abs(ftol(f64::from(client_y) - f64::from(half_h))) as f32;

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
    let pan = ftol(
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

/// Product of two native linear volumes: `DSoundBuffer::CombineInterps
/// FUN_004010C0` (`0x004010C7..0x004010D6`), `(a * b) >> 14`.
///
/// **VERA-internal, gamemd equivalent UNCHECKED: both `.clamp`s.** The native
/// volume path is `SHR EDX,0x10; SHR EAX,0x10; IMUL EDX,EAX; SHR EDX,0xe`
/// (`disassemble_function 0x004010C0`, read this session) — no bound at all;
/// the two operands are merely the top halves of 32-bit fixed-point fields, so
/// native accepts anything up to `0xFFFF` on each side and can return well
/// past `0x4000`. (The *pan* path at `0x00401116..0x0040114C` does clamp to
/// `0..0x4000`; the volume path does not, so this is not borrowed from there.)
/// Trigger: an operand outside `0..=0x4000`, which no VERA producer can make —
/// `SoundEntry::volume_linear` comes out of the native `[0, 1]` clamp times
/// 16384, and [`PlayShifts::volume_linear`] is `0x4000 - ((vshift << 14)/100)`
/// with `vshift` already held to `0..=100` by native `SetVShift @ 0x00406620`.
/// Player effect: none reachable. Frequency: never. Downstream risk: the guard
/// is what keeps [`native_volume_amplitude`]'s fixed 101-entry table index in
/// bounds, where Rust would panic and gamemd would read past the table.
pub fn combine_linear(a: i32, b: i32) -> i32 {
    (a.clamp(0, VOLUME_SCALE) * b.clamp(0, VOLUME_SCALE)) >> 14
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
    for frame in samples.chunks_exact_mut(2) {
        frame[0] *= left;
        frame[1] *= right;
    }
}

/// The audio RNG contract: `Random::RandomRanged @ 0x0065C7E0` on the
/// non-scenario `g_MainRng @ 0x00886B88` (seeded from the system clock in
/// `Init_Random_Number_System @ 0x0052FC20`, never synchronised). Inclusive
/// bounds; equal bounds return without drawing.
pub trait SampleRng {
    fn ranged(&mut self, low: i32, high: i32) -> i32;
}

/// Presentation-side RNG for sample choice, pitch and volume shift. The
/// native generator is clock-seeded, so only its draw contract matters; this
/// is SplitMix64 with rejection sampling for an unbiased inclusive range.
#[derive(Debug, Clone)]
pub struct SfxRng {
    state: u64,
}

impl SfxRng {
    pub fn seeded(seed: u64) -> Self {
        Self { state: seed }
    }

    pub fn from_clock() -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0x9E37_79B9_7F4A_7C15, |d| d.as_nanos() as u64);
        Self::seeded(nanos)
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

impl SampleRng for SfxRng {
    fn ranged(&mut self, low: i32, high: i32) -> i32 {
        if low == high {
            return low;
        }
        let (low, high) = if high < low { (high, low) } else { (low, high) };
        let span = (i64::from(high) - i64::from(low) + 1) as u64;
        let zone = u64::MAX - (u64::MAX % span);
        loop {
            let draw = self.next_u64();
            if draw < zone {
                return (i64::from(low) + (draw % span) as i64) as i32;
            }
        }
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
/// channel has been taken. VERA draws all three here so the RNG sequence
/// matches, and hands the result to [`arbiter::SoundArbiter`], which applies
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

/// Sample indices, in play order, for one pass of an event.
///
/// gamemd-derived: `SoundEvent::LoadSamples @ 0x004048B0` for events whose
/// `Delay.min < 0x21` (`0x004048FB..0x00404ACD`, all 588 stock `Control=`
/// entries except the 60 with a longer pre-delay, which reach the same
/// first-pass result through `SelectNextSample @ 0x00404BB0`):
/// 1. `Attack > 0`: load `samples[RandomRanged(0, Attack - 1)]`.
/// 2. Without `ALL`: `RANDOM` loads `samples[RandomRanged(Attack, count -
///    Decay - 1)]`, otherwise `samples[Attack]` (the first body sample —
///    never a round-robin). With `ALL`: every body sample in order.
/// 3. `Decay > 0`: load `samples[RandomRanged(count - Decay, count - 1)]`.
/// Then `PreparePlayout @ 0x00404700` / `AdvancePlaylist @ 0x004047B0` play
/// the loaded buffers: the attack buffer first when `Control=ATTACK`, the
/// decay buffer last when `Control=DECAY`, and the rest in `RandomRanged(0,
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
/// all on an owner-driven loop (`AnimClass::UpdateLoopingSound @ 0x00750D40`
/// marks the event started the instant it allocates it).
///
/// The attack **index is still drawn** when it is not played: the draw lives
/// in `SoundEvent::LoadSamples @ 0x004048B0`, which runs before the decision,
/// and it still reserves `samples[0]` out of the body range either way.
pub fn select_playout_pass(
    entry: &SoundEntry,
    rng: &mut impl SampleRng,
    plays_attack: bool,
) -> Vec<usize> {
    let count = entry.sounds.len() as i32;
    if count == 0 {
        return Vec::new();
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

    let mut order = Vec::with_capacity(middle.len() + 2);
    // Native keeps the attack buffer first only under the ATTACK control flag,
    // and the decay buffer last only under DECAY; without the flag the count is
    // zero (see `SoundEntry::attack`), so both agree.
    if plays_attack {
        order.extend(attack);
    }
    if entry.control & control::RANDOM != 0 {
        while !middle.is_empty() {
            let pick = rng.ranged(0, middle.len() as i32 - 1) as usize;
            // `.min`: VERA-internal, see the note above — `SampleRng` is a
            // public trait, so an out-of-contract impl must not panic here.
            order.push(middle.remove(pick.min(middle.len() - 1)));
        }
    } else {
        order.append(&mut middle);
    }
    order.extend(decay);
    order
}

/// Decoded audio ready for rodio playback.
/// Holds interleaved f32 stereo samples, sample rate, and channel count.
pub(crate) struct DecodedAudio {
    /// Interleaved stereo f32 samples (L, R, L, R, ...).
    pub(crate) samples: Vec<f32>,
    pub(crate) sample_rate: u32,
    /// Always 2 (stereo) — we upmix mono sources for consistency.
    pub(crate) channels: u16,
}

impl DecodedAudio {
    /// Append another clip; the native playlist chains buffers back to back.
    /// A rate mismatch keeps the first clip (RESIDUAL: native streams each
    /// buffer at its own rate; no stock attack/decay set mixes rates).
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

/// A resolved playback request: the decoded audio, the event's linear volume
/// (entry `Volume=` combined with the per-play `VShift=` reduction) and the
/// spatial gain.
struct ResolvedPlayback {
    decoded: DecodedAudio,
    event_linear: i32,
    /// The per-play draws, kept so the pre-delay reaches the arbiter.
    shifts: PlayShifts,
}

/// User-controlled master channel for one secondary output.
///
/// gamemd-derived: `OptionsClass::SetDefaults @ 0x005FA350` and
/// `OptionsClass__ReadFromINI @ 0x005FA620` retain independent SoundVolume and
/// VoiceVolume settings; ordinary/animation effects use Sound while unit and
/// EVA speech use Voice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SfxChannel {
    Sound,
    Voice,
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
/// below the user master: spatial volume, entry volume and the per-play
/// `VShift=` reduction. The master is chained into the same linear product
/// before the DirectSound curve — `DSoundBuffer::CombineInterps FUN_004010C0`
/// multiplies the buffer, event and group interps (`FUN_00402220`; the sound
/// group at `DAT_0087E758`, `SoundEvent::UpdateState 0x0040571E`) and only
/// then `FUN_0040A6D0` converts to decibels — so `effective` recomposes the
/// product each time rather than scaling an amplitude. The VERA-internal
/// lifecycle and foreground gates multiply the amplitude afterwards.
#[derive(Debug, Clone, Copy, PartialEq)]
struct SfxOutputGain {
    base_linear: i32,
    channel: SfxChannel,
}

impl SfxOutputGain {
    fn new(base_linear: i32, channel: SfxChannel) -> Self {
        Self {
            base_linear,
            channel,
        }
    }

    fn effective(self, scales: SfxOutputScales) -> f32 {
        let master = match self.channel {
            SfxChannel::Sound => scales.sound_volume,
            SfxChannel::Voice => scales.voice_volume,
        };
        let master_linear = ftol(f64::from(master.clamp(0.0, 1.0)) * f64::from(VOLUME_SCALE));
        // The channel multiplies the event group (`ch+0x90`), the many-sounds
        // scaler (`ch+0x98`) and the user volume group (`ch+0x9C`) together
        // before `FUN_0040A6D0` converts to decibels.
        let with_limiter = combine_linear(self.base_linear, scales.many_sounds_linear);
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

fn prepare_direct_voice_output(
    decoded: DecodedAudio,
    base_linear: i32,
    scales: SfxOutputScales,
) -> PreparedSfxOutput {
    PreparedSfxOutput::new(
        decoded,
        SfxOutputGain::new(base_linear, SfxChannel::Voice),
        scales,
    )
}

struct LiveSfxOutput {
    player: Player,
    gain: SfxOutputGain,
}

impl LiveSfxOutput {
    fn new(player: Player, gain: SfxOutputGain, initial_volume: f32) -> Self {
        player.set_volume(initial_volume);
        Self { player, gain }
    }

    fn apply_scales(&self, scales: SfxOutputScales) {
        self.player.set_volume(self.gain.effective(scales));
    }
}

/// One submitted cue's payload, waiting for the arbiter's start pass.
struct PendingPlayback {
    decoded: DecodedAudio,
    base_linear: i32,
    /// The `[SoundList]` identity, so a sustaining cue can re-resolve its
    /// playout for each loop pass the way `PreparePlayout` does.
    key: String,
}

/// Bookkeeping for a cue the arbiter reported as `sustaining`.
struct LoopQueue {
    key: String,
    /// The pan the next queued pass is baked with.
    ///
    /// RESIDUAL (device expressiveness) — native re-drives pan continuously
    /// through the channel's `ch+0x90` interp group, so a unit crossing the
    /// screen pans smoothly mid-buffer. rodio's `Player::set_volume` is a
    /// scalar with no per-channel form, so VERA bakes the pan into each
    /// buffer and a sustaining cue's pan therefore steps at each loop pass.
    /// Trigger: any moving looping emitter (Rocketeer, Terror Drone, Mig,
    /// Floating Disc). Player effect: the stereo image updates in steps of
    /// one loop pass rather than continuously; volume still glides. Frequency:
    /// whenever such a unit moves. Downstream risk: none. Fixing it needs a
    /// per-channel gain on a live source, which this sink does not offer.
    pan: i32,
    /// The loop budget is exhausted; stop topping up and let the buffer run
    /// dry so the arbiter is told the playout ended.
    finished: bool,
}

/// Manages sound effect playback with separate SFX pool and voice slot.
///
/// Matches the original engine's architecture:
/// - a 16-channel SFX pool arbitrated by [`arbiter::SoundArbiter`]
/// - 1 dedicated voice slot for unit responses (cuts off previous)
pub struct SfxPlayer {
    /// rodio mixer device sink — must be kept alive or all audio stops.
    _device: MixerDeviceSink,
    /// The decision half: channel pool, `Priority=`, `Limit=`, pre-delay,
    /// looping leash, ramps and the many-sounds limiter.
    arbiter: SoundArbiter,
    /// Decoded payloads the arbiter has not started yet.
    pending: BTreeMap<EventId, PendingPlayback>,
    /// Started outputs, keyed by the arbiter event holding the channel.
    live: BTreeMap<EventId, LiveSfxOutput>,
    /// Queue bookkeeping for the sustaining subset of [`Self::live`].
    loops: BTreeMap<EventId, LoopQueue>,
    /// Last service-pass timestamp handed in by the app.
    now_ms: u64,
    /// Dedicated voice player — unit responses cut off the previous voice.
    /// Separate from SFX pool so voices never compete with weapon sounds.
    ///
    /// VERA-internal, gamemd equivalent UNCHECKED: native routes voices
    /// through the same 16 channels via `VocClass::PlayAt @ 0x007509E0`, and
    /// its "cut the previous line" behaviour is the handle-level interrupt
    /// (`VocHandle::ValidateOrClear` then `SoundEvent::Stop` when the live
    /// event names a different entry), not a 17th channel. Trigger: any voice
    /// line while 16 SFX channels are busy. Player effect: VERA's voice is
    /// never denied a channel and never displaces an effect. Frequency:
    /// common in a busy fight. Downstream risk: the EVA queue's own
    /// semantics (`VoxClass`) are a separate parity surface that owns this
    /// slot, so folding voices into the pool is deferred to it.
    voice_player: Option<LiveSfxOutput>,
    /// The EVA announcement queue (`VoxClass`), including the pause depth
    /// `DAT_00b1d428` and the suspend depth `DAT_00b1d3d8`.
    vox: VoxQueue,
    /// An EVA line was started on `voice_player` and its end has not been
    /// reported to `vox` yet (`StreamPlayer::GetEndTime` stand-in).
    eva_stream_open: bool,
    /// Sound id currently occupying the dedicated voice slot, when known.
    current_voice_id: Option<String>,
    /// Stable id of the object whose acknowledgement line owns the voice slot,
    /// i.e. the object whose `TechnoClass+0x4DC` handle is live. `None` for an
    /// EVA cue or an ownerless voice.
    current_voice_owner: Option<u64>,
    /// The per-object voice latch: `TechnoClass::Queue_Voice @ 0x00708D90`
    /// writes it, `TechnoClass::AI_Update @ 0x006F9EBB` drains it.
    voice_queue: VoiceQueue,
    /// Ordinary and animation SFX master volume (0.0 to 1.0).
    sound_volume: f64,
    /// Unit and EVA voice master volume (0.0 to 1.0).
    voice_volume: f64,
    /// Temporary app-lifecycle multiplier over every live SFX/voice output.
    output_scale: f32,
    /// Foreground-owned primary-output gate. Secondary Players stay running so
    /// their playback cursors continue while global output is suppressed.
    focus_output_scale: f32,
    /// Whether the game is paused, so [`Self::set_paused`] only acts on the
    /// edge the way `GamePause::Enter`/`Exit` do.
    paused: bool,
    /// Presentation-side RNG standing in for `g_MainRng @ 0x00886B88`.
    rng: SfxRng,
}

impl SfxPlayer {
    /// Create a new SfxPlayer. Returns None if audio output cannot be opened.
    pub fn new() -> Option<Self> {
        let device = DeviceSinkBuilder::open_default_sink()
            .map_err(|e| log::error!("Failed to initialize SFX audio: {}", e))
            .ok()?;

        Some(Self {
            _device: device,
            arbiter: SoundArbiter::new(0),
            pending: BTreeMap::new(),
            live: BTreeMap::new(),
            loops: BTreeMap::new(),
            now_ms: 0,
            voice_player: None,
            vox: VoxQueue::new(),
            eva_stream_open: false,
            current_voice_id: None,
            current_voice_owner: None,
            voice_queue: VoiceQueue::new(),
            sound_volume: 0.7,
            voice_volume: 0.7,
            output_scale: 1.0,
            focus_output_scale: 1.0,
            paused: false,
            rng: SfxRng::from_clock(),
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

    /// Resolve a registry event to decoded audio: draw the per-play shifts,
    /// pick the sample sequence, load and chain it, apply the pitch shift.
    fn resolve_entry(
        &mut self,
        entry: &SoundEntry,
        assets: &AssetManager,
        audio_indices: &[crate::assets::audio_bag::AudioIndex],
    ) -> Option<ResolvedPlayback> {
        resolve_entry_playback(entry, &mut self.rng, |name| {
            load_sfx(name, assets, audio_indices)
        })
    }

    /// Resolve a sound id through the registry, else as a raw audio-bag name
    /// (EVA lines and other bag-only entries) at full linear volume.
    fn resolve_any(
        &mut self,
        sound_id: &str,
        registry: &SoundRegistry,
        assets: &AssetManager,
        audio_indices: &[crate::assets::audio_bag::AudioIndex],
    ) -> Option<ResolvedPlayback> {
        if let Some(entry) = registry.get(sound_id) {
            return self.resolve_entry(entry, assets, audio_indices);
        }
        load_sfx(sound_id, assets, audio_indices).map(|decoded| ResolvedPlayback {
            decoded,
            event_linear: VOLUME_SCALE,
            // A raw bag name has no `VocClass`, so there is nothing to draw.
            shifts: PlayShifts {
                frequency_pct: 100,
                volume_shift_pct: 0,
                predelay_ms: 0,
            },
        })
    }

    /// Play a sound by its sound.ini ID (e.g., "VGCannon1") or audio.bag name,
    /// non-positionally (full volume, centred).
    ///
    /// Resolution order:
    /// 1. Look up `sound_id` in the SoundRegistry (sound.ini sections)
    /// 2. If found, pick the samples and load via audio bags then MIX assets
    /// 3. If NOT found in registry, try `sound_id` directly as an audio.bag name
    ///    (for EVA sounds and other bag-only entries)
    ///
    /// Returns true if the sound was successfully started.
    pub fn play_sound(
        &mut self,
        sound_id: &str,
        registry: &SoundRegistry,
        assets: &AssetManager,
        audio_indices: &[crate::assets::audio_bag::AudioIndex],
    ) -> bool {
        self.play_sound_spatial(
            sound_id,
            SpatialGain::CENTRED_FULL,
            registry,
            assets,
            audio_indices,
        )
    }

    /// Play a sound with a plain volume multiplier and no pan — the launcher
    /// beep path, which scales a non-positional cue.
    pub fn play_sound_with_volume(
        &mut self,
        sound_id: &str,
        volume: f32,
        registry: &SoundRegistry,
        assets: &AssetManager,
        audio_indices: &[crate::assets::audio_bag::AudioIndex],
    ) -> bool {
        self.play_sound_spatial(
            sound_id,
            SpatialGain {
                volume,
                pan: PAN_CENTRE,
            },
            registry,
            assets,
            audio_indices,
        )
    }

    /// Play a sound at a positional gain from [`spatial_gain`].
    pub fn play_sound_spatial(
        &mut self,
        sound_id: &str,
        gain: SpatialGain,
        registry: &SoundRegistry,
        assets: &AssetManager,
        audio_indices: &[crate::assets::audio_bag::AudioIndex],
    ) -> bool {
        let Some(resolved) = self.resolve_any(sound_id, registry, assets, audio_indices) else {
            log::trace!("SFX: could not resolve '{}'", sound_id);
            return false;
        };
        let facts = entry_facts(sound_id, registry);
        let base_linear = combine_linear(gain.volume_linear(), resolved.event_linear);
        self.submit_decoded(sound_id, facts, resolved, base_linear, gain.pan)
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
        assets: &AssetManager,
        audio_indices: &[crate::assets::audio_bag::AudioIndex],
    ) -> bool {
        let Some(entry) = registered_entry(sound_id, registry) else {
            return false;
        };
        let Some(resolved) = self.resolve_entry(entry, assets, audio_indices) else {
            return false;
        };
        let facts = EntryFacts::from(entry);
        let base_linear = combine_linear(gain.volume_linear(), resolved.event_linear);
        self.submit_decoded(sound_id, facts, resolved, base_linear, gain.pan)
            .is_some()
    }

    /// Start (or re-point) the cue an owner object holds a loop handle for.
    ///
    /// gamemd-derived: `AnimClass::UpdateLoopingSound @ 0x00750D40`, the
    /// canonical driver of every sustained sound. The owner calls it with its
    /// current coordinate; when the positional volume is above zero and no
    /// live event is bound, a loopable entry allocates one and is immediately
    /// marked started (`SoundEvent::MarkStarted @ 0x004052E0`, which is why an
    /// owner-driven loop never replays its `Control=attack` sample); the
    /// volume and pan are then re-driven and the handle re-pointed. When the
    /// volume drops to zero the event is stopped and the handle cleared —
    /// that clearing is what ends the loop, through the state-3 leash in
    /// `SoundEvent::UpdateState @ 0x004057DC`.
    ///
    /// `gain: None` is the `CalcVolumeAndPan <= 0` arm.
    pub fn play_animation_sound_spatial(
        &mut self,
        anim_id: u64,
        sound_id: &str,
        gain: SpatialGain,
        registry: &SoundRegistry,
        assets: &AssetManager,
        audio_indices: &[crate::assets::audio_bag::AudioIndex],
    ) -> bool {
        // `VocClass::PlayAt`'s handle-level interrupt: a live event that
        // belongs to a different entry is stopped before the new one starts.
        // This — not `Control=interrupt` — is what makes a re-issued cue cut
        // its predecessor.
        self.stop_animation_sound(anim_id);
        let facts = entry_facts(sound_id, registry);
        // An owner-driven loop is marked started at allocation, so its very
        // first pass already skips the `Control=attack` sample.
        let plays_attack = !facts.is_loopable();
        let resolved = match registry.get(sound_id) {
            Some(entry) => resolve_entry_playback_pass(
                entry,
                &mut self.rng,
                |name| load_sfx(name, assets, audio_indices),
                plays_attack,
            ),
            None => self.resolve_any(sound_id, registry, assets, audio_indices),
        };
        let Some(resolved) = resolved else {
            return false;
        };
        let base_linear = combine_linear(gain.volume_linear(), resolved.event_linear);
        let Some(event) = self.submit_decoded(sound_id, facts, resolved, base_linear, gain.pan)
        else {
            return false;
        };
        if facts.is_loopable() {
            self.arbiter.mark_started(event);
        }
        // The handle is bound either way: a one-shot still belongs to its
        // owner so `stop_animation_sound` can find it, it is just not leashed
        // (`UpdateState` state 3 checks `Control & LOOP` first).
        self.arbiter
            .set_loop_handle(anim_id, Some(event), &registry_key(sound_id));
        true
    }

    /// Re-drive one owner's live loop with its current positional gain, the
    /// way `AnimClass::UpdateLoopingSound` runs on every owner update.
    ///
    /// Returns false when the owner holds no live event.
    pub fn update_looping_sound(&mut self, anim_id: u64, gain: Option<SpatialGain>) -> bool {
        let Some(event) = self.arbiter.validate_loop_handle(anim_id) else {
            return false;
        };
        match gain {
            Some(gain) => {
                let now = self.now_ms;
                self.arbiter
                    .set_volume(event, gain.volume_linear().min(VOLUME_SCALE), now);
                self.arbiter.set_pan(event, gain.pan, now);
                if let Some(queue) = self.loops.get_mut(&event) {
                    queue.pan = gain.pan;
                }
                true
            }
            None => {
                // `if (0.0 < fVar3) {...} else { SoundEvent__Stop; }` then
                // `SetLoopHandle(handle, 0, voc)`.
                self.stop_animation_sound(anim_id);
                false
            }
        }
    }

    /// Release only the handle owned by `anim_id`. Idempotent.
    pub fn stop_animation_sound(&mut self, anim_id: u64) {
        if let Some(event) = self.arbiter.validate_loop_handle(anim_id) {
            self.arbiter.stop(event);
            self.release_output(event);
        }
        self.arbiter.clear_loop_handle(anim_id);
    }

    /// `TechnoClass::Queue_Voice @ 0x00708D90` — latch one object's
    /// acknowledgement line (VoiceSelect, VoiceMove, VoiceAttack, ...).
    ///
    /// Nothing plays here. [`Self::drain_unit_voices`] is the drain half, and
    /// it is what decides whether a second click restarts the line, drops it,
    /// or waits — see [`crate::audio::voice_queue`] for the three outcomes.
    pub fn queue_unit_voice(&mut self, owner: u64, sound_id: &str) {
        self.voice_queue.queue(owner, sound_id);
    }

    /// `TechnoClass::AI_Update @ 0x006F9EBB` — drain every latched line.
    ///
    /// Voices are non-positional: `0x006F9EE0`/`0x006F9EE5` pass pan `0x2000`
    /// and volume `1.0f` to `VocClass::PlayAtPos @ 0x00750920`, which is what
    /// the dedicated voice slot already does.
    pub fn drain_unit_voices(
        &mut self,
        registry: &SoundRegistry,
        assets: &AssetManager,
        audio_indices: &[crate::assets::audio_bag::AudioIndex],
    ) {
        // `VocHandle::ValidateOrClear @ 0x00406130` for each object, resolved
        // once for the pass: VERA's single voice slot means at most one
        // object's handle can be live at a time.
        let live_owner = self.live_voice_owner();
        let decisions = self.voice_queue.drain(|owner| live_owner == Some(owner));
        for decision in decisions {
            let Some(resolved) =
                self.resolve_any(&decision.sound_id, registry, assets, audio_indices)
            else {
                continue;
            };
            if self.play_voice(
                resolved.decoded,
                resolved.event_linear,
                Some(decision.sound_id),
            ) {
                self.current_voice_owner = Some(decision.owner);
            }
        }
    }

    /// The object whose voice handle is still live, if any.
    ///
    /// VERA-internal shape, gamemd equivalent read: native stores the handle
    /// on the techno (`+0x4DC`) so any number of objects can be speaking at
    /// once; VERA has one voice slot, so at most the slot's current owner can
    /// answer `true`.
    ///
    /// Two consequences, both recorded:
    /// 1. An EVA line taking the slot kills a unit's handle mid-word.
    /// 2. [`Self::drain_unit_voices`] resolves this **once** before its loop,
    ///    so if two objects both have a line latched in the same pass, both
    ///    play and the second cuts the first — native would let them overlap,
    ///    because each object probes its own handle. Re-resolving inside the
    ///    loop would not fix it either; one slot cannot hold two lines.
    ///
    /// Trigger: two objects with a latched line in the same
    /// `drain_sound_events` pass, or an EVA line landing on a unit line.
    /// Player effect: the second line cuts the first. Frequency: not reachable
    /// from ordinary player input while A1's one-voice-per-batch latch
    /// (`g_SelectionVoice_Enable @ 0x00822CF2`) holds — it lets only one
    /// object speak per dispatch — but a selection voice and an order voice
    /// from *different* objects arriving in the same pass would hit it.
    /// Downstream risk: none; folding voices into the 16-channel pool is the
    /// `voice_player` residual's job, and that is what closes both cases.
    fn live_voice_owner(&self) -> Option<u64> {
        let owner = self.current_voice_owner?;
        self.voice_player
            .as_ref()
            .filter(|output| !output.player.empty())
            .map(|_| owner)
    }

    /// Draw an index in `0..count` from the presentation RNG.
    ///
    /// The app layer's stand-in for a native `rand % count` list pick — the
    /// `LightningSounds=` choice at `0x0053A48D DIV [Rules+0x744]` is the one
    /// caller. `count == 0` yields 0 and the caller must not index with it.
    pub fn pick_index(&mut self, count: usize) -> usize {
        if count <= 1 {
            return 0;
        }
        self.rng.ranged(0, count as i32 - 1) as usize
    }

    /// Draw `RandomRanged(0, 99)` from the presentation RNG.
    ///
    /// The stand-in for gamemd's percentage gates on `g_MainRng @ 0x00886B88`.
    /// `TechnoClass::ReceiveDamage @ 0x007026AF..0x007026C0` is the one caller:
    /// `PUSH 0x63 ; PUSH 0x0 ; MOV ECX,0x886B88 ; CALL 0x0065C7E0`, then
    /// `CMP EAX,0x1E ; JGE` — so the cue speaks for a draw of 0..=29.
    ///
    /// Unlike [`Self::pick_index`] this always draws, matching native: the
    /// `RandomRanged` body at `0x0065C7E0` only skips the draw when its two
    /// endpoints are equal, and `0` and `99` are not.
    pub fn roll_percent(&mut self) -> i32 {
        self.rng.ranged(0, 99)
    }

    /// `VoxClass::PlayEVA @ 0x00752700`: find the entry by name (`stricmp`
    /// scan of `0xB1D4A4`; a miss becomes `QueueVoice(-1, ..)`, which its
    /// index guard rejects), then `QueueVoice(index, type_override, -1)` —
    /// priority is always the entry's own — and `PlayNextQueued`.
    ///
    /// `side` is the session column `VoxClass::SetSide @ 0x007534E0` stored;
    /// an empty column is a `PlayFile` of `".WAV"`, which fails, so it is
    /// treated as a miss here. Returns whether the request entered a queue
    /// (or the pending slot).
    pub fn play_eva(
        &mut self,
        event: &str,
        type_override: Option<EvaType>,
        eva_registry: &EvaRegistry,
        side: EvaSide,
        registry: &SoundRegistry,
        assets: &AssetManager,
        audio_indices: &[crate::assets::audio_bag::AudioIndex],
    ) -> bool {
        let Some(entry) = eva_registry.entry(event) else {
            return false;
        };
        let Some(sample) = entry.column(side) else {
            return false;
        };
        let effect = self.vox.queue_voice(
            VoxRequest {
                event: &entry.name,
                sample,
                eva_type: entry.eva_type,
                priority: entry.priority,
            },
            type_override,
        );
        if effect.stop_stream {
            // `StreamPlayer::Stop` (`0x00752480`, type 2 while current): only
            // the dedicated voice channel; ordinary and animation SFX are
            // untouched.
            if let Some(output) = self.voice_player.take() {
                output.player.stop();
            }
            self.current_voice_id = None;
            self.eva_stream_open = false;
        }
        self.advance_voice_queue(registry, assets, audio_indices);
        effect.inserted
    }

    /// Whether the dedicated voice slot has audio left to play
    /// (`StreamPlayer::IsPlaying @ 0x00408070` stand-in; VERA serves unit
    /// acknowledgements from the same slot, so a unit line counts too).
    fn voice_slot_busy(&self) -> bool {
        self.voice_player
            .as_ref()
            .is_some_and(|output| !output.player.empty())
    }

    /// `VoxClass::PlayNextQueued @ 0x00752760`, the per-pump dequeue.
    ///
    /// The native gate is `IsPlaying() == 0 && now > GetEndTime() + gap &&
    /// DAT_00b1d428 == 0` (`0x00752794..0x007527D5`); [`VoxQueue::take_next`]
    /// owns it. The stream's end time is reported here the first time the
    /// slot is seen empty after an EVA line started, so the 500 ms gap runs
    /// from the observed end (a paused line therefore still gets its gap
    /// after it resumes and finishes).
    ///
    /// Then the sample is loaded and started — `StreamPlayer::PlayFile(name,
    /// 1)` at `0x0075295C`; a failed load drops the node without touching
    /// the gap (`0x00752963 JZ`), exactly as a missing `.WAV` does natively
    /// (the stock `Dummy` columns of `EVA_PsychicDominatorActivated`).
    pub fn advance_voice_queue(
        &mut self,
        registry: &SoundRegistry,
        assets: &AssetManager,
        audio_indices: &[crate::assets::audio_bag::AudioIndex],
    ) {
        let busy = self.voice_slot_busy();
        if !busy {
            if self.eva_stream_open {
                self.vox.stream_ended(self.now_ms);
                self.eva_stream_open = false;
            }
            if self.voice_player.is_some() {
                self.voice_player = None;
                self.current_voice_id = None;
            }
        }
        let Some(node) = self.vox.take_next(self.now_ms, busy) else {
            return;
        };
        self.start_eva_node(node, registry, assets, audio_indices);
    }

    fn start_eva_node(
        &mut self,
        node: VoxNode,
        registry: &SoundRegistry,
        assets: &AssetManager,
        audio_indices: &[crate::assets::audio_bag::AudioIndex],
    ) {
        let Some(resolved) = self.resolve_any(&node.sample, registry, assets, audio_indices) else {
            log::debug!("EVA {} has no sample {}", node.event, node.sample);
            return;
        };
        let sample = node.sample.clone();
        if self.play_voice(resolved.decoded, resolved.event_linear, Some(sample)) {
            self.eva_stream_open = true;
            self.vox.started(node);
        }
    }

    /// Play decoded audio on the dedicated voice slot, cutting off any current voice.
    fn play_voice(
        &mut self,
        decoded: DecodedAudio,
        base_linear: i32,
        sound_id: Option<String>,
    ) -> bool {
        let prepared = prepare_direct_voice_output(decoded, base_linear, self.output_scales());
        self.play_prepared_voice(prepared, sound_id)
    }

    fn play_prepared_voice(
        &mut self,
        prepared: PreparedSfxOutput,
        sound_id: Option<String>,
    ) -> bool {
        // Cut off previous voice immediately.
        if let Some(old) = self.voice_player.take() {
            old.player.stop();
        }
        self.current_voice_id = None;

        let PreparedSfxOutput {
            decoded,
            gain,
            initial_volume,
        } = prepared;

        let channels = match NonZero::new(decoded.channels) {
            Some(c) => c,
            None => return false,
        };
        let sample_rate = match NonZero::new(decoded.sample_rate) {
            Some(r) => r,
            None => return false,
        };

        let source = SamplesBuffer::new(channels, sample_rate, decoded.samples);
        let player: Player = Player::connect_new(self._device.mixer());
        let output = LiveSfxOutput::new(player, gain, initial_volume);
        output.player.append(source);
        self.voice_player = Some(output);
        self.current_voice_id = sound_id;
        // Every other route into the slot (EVA queue, STANDARD cue, interrupt)
        // is ownerless; `drain_unit_voices` re-stamps the owner right after it
        // starts an object's line. An EVA cue taking the slot therefore kills
        // the live techno handle — VERA-internal, and the same single-slot
        // limitation the `voice_player` field records.
        self.current_voice_owner = None;
        true
    }

    /// Hand a decoded cue to the arbiter. Nothing is audible yet: native's
    /// `SoundEvent::AllocateFromPool @ 0x00405190` only creates the record in
    /// state 0, and the channel, pre-delay and playback all happen on the
    /// next `SoundSystem::UpdateTick` pass (at most `0x21` ms later).
    fn submit_decoded(
        &mut self,
        sound_id: &str,
        facts: EntryFacts,
        resolved: ResolvedPlayback,
        base_linear: i32,
        pan: i32,
    ) -> Option<EventId> {
        let key = registry_key(sound_id);
        let request = PlayRequest {
            key: key.clone(),
            facts,
            volume_linear: base_linear,
            pan,
            predelay_ms: resolved.shifts.predelay_ms,
        };
        let event = self.arbiter.submit(&request, self.now_ms)?;
        self.pending.insert(
            event,
            PendingPlayback {
                decoded: resolved.decoded,
                base_linear,
                key,
            },
        );
        Some(event)
    }

    /// One `AudioSystem::Pump @ 0x00406F70` service pass.
    ///
    /// The app calls this every frame, *unconditionally* — native's pump
    /// hangs off `Network_ServiceLoop @ 0x0048D080`, whose callers include
    /// `Main::ThrottleFrame @ 0x0055E160`, `ProcessModalServicePump @
    /// 0x00623120`, `ShellDialog::RunUntilResult @ 0x0060D380` and the
    /// loading-screen paths, so the audio service keeps running in menus,
    /// modal dialogs and while the frame pacer idles. Pause is expressed
    /// separately, by suspending events ([`Self::set_paused`]).
    ///
    /// The `> 33 ms` gate lives here, as it does in native.
    pub fn pump(
        &mut self,
        now_ms: u64,
        registry: &SoundRegistry,
        assets: &AssetManager,
        audio_indices: &[crate::assets::audio_bag::AudioIndex],
    ) {
        self.now_ms = now_ms;
        self.advance_voice_queue(registry, assets, audio_indices);
        self.top_up_loop_queues(now_ms, registry, assets, audio_indices);
        self.report_finished_outputs();
        if !self.arbiter.pump_due(now_ms) {
            return;
        }
        let actions = self.arbiter.update_tick(now_ms);
        let scales = self.output_scales();
        for action in actions {
            match action {
                ArbiterAction::Start {
                    event,
                    volume_linear,
                    pan,
                    sustaining,
                } => self.start_output(event, volume_linear, pan, sustaining, now_ms),
                ArbiterAction::Gain {
                    event,
                    volume_linear,
                    pan,
                } => {
                    if let Some(output) = self.live.get_mut(&event) {
                        output.gain.base_linear = volume_linear;
                        output.apply_scales(scales);
                    }
                    if let Some(queue) = self.loops.get_mut(&event) {
                        queue.pan = pan;
                    }
                }
                ArbiterAction::Stop { event } => self.release_output(event),
            }
        }
        // The many-sounds scaler moved, so every live output's amplitude has.
        self.apply_live_output_scales();
    }

    /// Apply an `ArbiterAction::Start`: build the rodio player, bake the pan
    /// into the buffer and queue the first pass.
    fn start_output(
        &mut self,
        event: EventId,
        volume_linear: i32,
        pan: i32,
        sustaining: bool,
        now_ms: u64,
    ) {
        let Some(pending) = self.pending.remove(&event) else {
            return;
        };
        let PendingPlayback {
            mut decoded,
            base_linear,
            key,
        } = pending;
        let _ = base_linear;
        apply_pan(&mut decoded.samples, pan);
        let prepared = prepare_normal_sfx_output(decoded, volume_linear, self.output_scales());
        let PreparedSfxOutput {
            decoded,
            gain,
            initial_volume,
        } = prepared;
        let (Some(channels), Some(sample_rate)) = (
            NonZero::new(decoded.channels),
            NonZero::new(decoded.sample_rate),
        ) else {
            self.arbiter.stop(event);
            return;
        };
        let source = SamplesBuffer::new(channels, sample_rate, decoded.samples);
        let player: Player = Player::connect_new(self._device.mixer());
        let output = LiveSfxOutput::new(player, gain, initial_volume);
        output.player.append(source);
        self.live.insert(event, output);
        let _ = now_ms;
        if sustaining {
            self.loops.insert(
                event,
                LoopQueue {
                    key,
                    pan,
                    finished: false,
                },
            );
        }
    }

    /// Keep every sustaining cue's buffer queue filled at least
    /// [`LOOP_QUEUE_LOOKAHEAD_MS`] ahead, re-resolving the playout for each
    /// pass the way `AdvancePlaylist`'s LOOP branch re-enters
    /// `SoundEvent::PreparePlayout @ 0x00404700` — so a `Control=random`
    /// entry reshuffles its body order every pass, and the `Control=attack`
    /// sample is not replayed (`flags & 8` is already set).
    fn top_up_loop_queues(
        &mut self,
        now_ms: u64,
        registry: &SoundRegistry,
        assets: &AssetManager,
        audio_indices: &[crate::assets::audio_bag::AudioIndex],
    ) {
        let _ = now_ms;
        for event in self.loops.keys().copied().collect::<Vec<_>>() {
            loop {
                let Some(queue) = self.loops.get(&event) else {
                    break;
                };
                if queue.finished {
                    break;
                }
                let queued = self
                    .live
                    .get(&event)
                    .map_or(0, |output| output.player.len());
                if queued >= LOOP_QUEUE_DEPTH {
                    break;
                }
                if !self.arbiter.advance_loop(event) {
                    if let Some(queue) = self.loops.get_mut(&event) {
                        queue.finished = true;
                    }
                    break;
                }
                let key = queue.key.clone();
                let pan = queue.pan;
                let Some(entry) = registry.get(&key).cloned() else {
                    if let Some(queue) = self.loops.get_mut(&event) {
                        queue.finished = true;
                    }
                    break;
                };
                // `flags & 8` is set by now (`StartPlayback` at the latest), so
                // `PreparePlayout` takes the `AdvancePlaylist` arm and the
                // attack sample never heads a restarted pass.
                let plays_attack = self.arbiter.plays_attack_sample(event);
                let Some(resolved) = resolve_entry_playback_pass(
                    &entry,
                    &mut self.rng,
                    |name| load_sfx(name, assets, audio_indices),
                    plays_attack,
                ) else {
                    if let Some(queue) = self.loops.get_mut(&event) {
                        queue.finished = true;
                    }
                    break;
                };
                let mut decoded = resolved.decoded;
                apply_pan(&mut decoded.samples, pan);
                let (Some(channels), Some(sample_rate)) = (
                    NonZero::new(decoded.channels),
                    NonZero::new(decoded.sample_rate),
                ) else {
                    if let Some(queue) = self.loops.get_mut(&event) {
                        queue.finished = true;
                    }
                    break;
                };
                if decoded.samples.is_empty() {
                    // A zero-length pass would never raise the queue length
                    // and would spin this loop.
                    if let Some(queue) = self.loops.get_mut(&event) {
                        queue.finished = true;
                    }
                    break;
                }
                let Some(output) = self.live.get(&event) else {
                    break;
                };
                output
                    .player
                    .append(SamplesBuffer::new(channels, sample_rate, decoded.samples));
            }
        }
    }

    /// The device telling the arbiter that a playout ran dry with nothing
    /// left to play — native's `ch+0xB8` callback `LAB_00405A00`, which sets
    /// state 4 so the next pass reaps the event. This is also the reaping
    /// cadence native runs every pass and VERA previously only ran inside a
    /// play call.
    fn report_finished_outputs(&mut self) {
        let finished: Vec<EventId> = self
            .live
            .iter()
            .filter(|(_, output)| output.player.empty())
            .map(|(event, _)| *event)
            .collect();
        for event in finished {
            self.arbiter.notify_playout_ended(event);
            self.release_output(event);
        }
    }

    fn release_output(&mut self, event: EventId) {
        self.pending.remove(&event);
        self.loops.remove(&event);
        if let Some(output) = self.live.remove(&event) {
            output.player.stop();
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
    /// `FUN_0053bad0` gate that the SFX half does. VERA serves EVA and unit
    /// voices from one slot, so both halves land on `voice_player`.
    ///
    /// Idempotent — call it with the current pause state every frame.
    pub fn set_paused(&mut self, paused: bool, now_ms: u64) {
        self.now_ms = now_ms;
        if paused == self.paused {
            return;
        }
        self.paused = paused;
        self.vox.set_paused(paused);
        if paused {
            self.arbiter.suspend_all(now_ms);
            for output in self.live.values() {
                output.player.pause();
            }
            if let Some(output) = self.voice_player.as_ref() {
                output.player.pause();
            }
        } else {
            self.arbiter.resume_all(now_ms);
            for output in self.live.values() {
                output.player.play();
            }
            if let Some(output) = self.voice_player.as_ref() {
                output.player.play();
            }
        }
    }

    /// Compatibility setter: apply one master to both SFX and voice channels.
    pub fn set_volume(&mut self, volume: f64) {
        let volume = volume.clamp(0.0, 1.0);
        self.sound_volume = volume;
        self.voice_volume = volume;
        self.apply_live_output_scales();
    }

    /// Set the ordinary and animation SFX master volume.
    pub fn set_sound_volume(&mut self, volume: f64) {
        self.sound_volume = volume.clamp(0.0, 1.0);
        self.apply_live_output_scales();
    }

    /// Set the unit and EVA voice master volume.
    pub fn set_voice_volume(&mut self, volume: f64) {
        self.voice_volume = volume.clamp(0.0, 1.0);
        self.apply_live_output_scales();
    }

    /// Apply a temporary multiplier to all live outputs without changing the
    /// saved SFX setting or foreground gate.
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
        for output in self.live.values() {
            output.apply_scales(scales);
        }
        if let Some(output) = self.voice_player.as_ref() {
            output.apply_scales(scales);
        }
    }

    /// Pump the dedicated voice queue once and report whether any voice work
    /// remains, mirroring the poll performed inside native exit wait loops.
    pub fn pump_and_check_voices(
        &mut self,
        registry: &SoundRegistry,
        assets: &AssetManager,
        audio_indices: &[crate::assets::audio_bag::AudioIndex],
    ) -> bool {
        self.advance_voice_queue(registry, assets, audio_indices);
        self.voices_active()
    }

    /// Hard-stop every SFX/voice source and discard queued announcements.
    pub fn stop_all(&mut self) {
        for event in self.live.keys().copied().collect::<Vec<_>>() {
            self.arbiter.stop(event);
        }
        for (_, output) in std::mem::take(&mut self.live) {
            output.player.stop();
        }
        for event in self.pending.keys().copied().collect::<Vec<_>>() {
            self.arbiter.stop(event);
        }
        self.pending.clear();
        self.loops.clear();
        if let Some(output) = self.voice_player.take() {
            output.player.stop();
        }
        self.current_voice_id = None;
        // `VoxClass::ResetAll @ 0x007535D0`: current entry done, stop the
        // stream, `ClearAllQueues`, then `DAT_00b1d428 = 0` and
        // `DAT_00b1d3d8 = 0`. Both depths are reset here, not left to unwind
        // on the next pause edge.
        self.vox.reset_all();
        self.eva_stream_open = false;
    }

    /// Get the current SFX master volume.
    pub fn volume(&self) -> f64 {
        self.sound_volume
    }

    /// Get the current unit and EVA voice master volume.
    pub fn voice_volume(&self) -> f64 {
        self.voice_volume
    }

    /// Owners that currently hold a live loop handle, so the app can re-drive
    /// each one with its object's current coordinate the way native's owner
    /// calls `AnimClass::UpdateLoopingSound @ 0x00750D40` on every update.
    pub fn looping_owners(&mut self) -> Vec<u64> {
        self.arbiter.loop_handle_owners()
    }

    /// The `[SoundList]` identity one owner's live loop handle names.
    pub fn loop_handle_sound_id(&self, owner: u64) -> Option<String> {
        self.arbiter.loop_handle_key(owner).map(str::to_owned)
    }

    /// Number of live sound events — native `g_LiveSoundEventCount @
    /// 0x0087E28C`, which counts records in the pool, not busy channels.
    pub fn active_count(&self) -> usize {
        self.arbiter.live_event_count()
    }

    /// Sound events currently holding one of the 16 channels.
    pub fn busy_channel_count(&self) -> usize {
        self.arbiter.busy_channel_count()
    }

    /// `VoxClass::PumpAndCheckActive @ 0x007529E0`'s answer: the stream is
    /// playing or a node waits in any queue. Non-blocking (rodio
    /// `Player::empty()` is a poll). Used by the quit cascade and the
    /// victory/defeat savour wait.
    pub fn voices_active(&self) -> bool {
        self.vox.is_active(self.voice_slot_busy())
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
    }
}

/// Device-free core of one play request: draw the shifts, select the
/// samples, load and chain them, apply the pitch shift, and combine the entry
/// volume with the `VShift=` reduction into the event's linear volume.
fn resolve_entry_playback(
    entry: &SoundEntry,
    rng: &mut impl SampleRng,
    load: impl FnMut(&str) -> Option<DecodedAudio>,
) -> Option<ResolvedPlayback> {
    resolve_entry_playback_pass(entry, rng, load, true)
}

/// [`resolve_entry_playback`] for one pass; `plays_attack` is
/// `PreparePlayout`'s `flags & 8` test — see [`select_playout_pass`].
fn resolve_entry_playback_pass(
    entry: &SoundEntry,
    rng: &mut impl SampleRng,
    mut load: impl FnMut(&str) -> Option<DecodedAudio>,
    plays_attack: bool,
) -> Option<ResolvedPlayback> {
    if entry.sounds.is_empty() {
        return None;
    }
    let shifts = PlayShifts::draw(entry, rng);
    let order = select_playout_pass(entry, rng, plays_attack);
    let mut decoded: Option<DecodedAudio> = None;
    for index in order {
        let Some(name) = entry.sounds.get(index) else {
            continue;
        };
        let Some(clip) = load(name) else {
            continue;
        };
        match decoded.as_mut() {
            Some(chain) => chain.append(clip),
            None => decoded = Some(clip),
        }
    }
    let mut decoded = decoded?;
    decoded.sample_rate = shifts.shifted_sample_rate(decoded.sample_rate);
    Some(ResolvedPlayback {
        decoded,
        event_linear: combine_linear(entry.volume_linear, shifts.volume_linear()),
        shifts,
    })
}

/// Load a sound effect file and decode it to interleaved f32 stereo samples.
///
/// Resolution order:
/// 1. Try audio.bag indices (most voice/EVA sounds live here)
/// 2. Try MIX asset lookup by exact name
/// 3. Try MIX asset lookup with .wav extension appended
///
/// Supports .wav (raw PCM), .aud (IMA ADPCM), and audio.bag formats.
fn load_sfx(
    filename: &str,
    assets: &AssetManager,
    audio_indices: &[crate::assets::audio_bag::AudioIndex],
) -> Option<DecodedAudio> {
    // Try audio.bag indices first (voices, EVA announcements).
    for index in audio_indices {
        if let Some((entry, data)) = index.get(filename) {
            if let Some(bag_audio) = crate::assets::audio_bag::decode_bag_audio(entry, data) {
                // Convert i16 → f32 stereo.
                let stereo = upmix_i16_to_f32_stereo(&bag_audio.samples_i16, bag_audio.channels);
                return Some(DecodedAudio {
                    samples: stereo,
                    sample_rate: bag_audio.sample_rate,
                    channels: 2,
                });
            }
        }
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
            .chunks_exact(2)
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
        assert_eq!(edge.pan, ftol(511.0 * 8192.0 / 1024.0 + 8192.0));
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
        assert_eq!(g.pan, ftol(0.5 * 8192.0 / 1023.0 + 8192.0));
        // clientX - 511.5 = -0.5 -> ftol 0 as well (truncation toward zero).
        let g = calc_volume_and_pan(511, 300, 1023.0, LISTENER_H, src, false).unwrap();
        assert_eq!(g.volume, 1.0);
        assert_eq!(g.pan, ftol(-0.5 * 8192.0 / 1023.0 + 8192.0));
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

    #[test]
    fn spatial_gain_volume_linear_is_ftol_capped_at_the_scale() {
        assert_eq!(SpatialGain::CENTRED_FULL.volume_linear(), VOLUME_SCALE);
        let half = SpatialGain {
            volume: 0.5,
            pan: PAN_CENTRE,
        };
        assert_eq!(half.volume_linear(), 8192);
        let odd = SpatialGain {
            volume: 400.0 / 600.0,
            pan: PAN_CENTRE,
        };
        assert_eq!(
            odd.volume_linear(),
            ftol(f64::from(400.0f32 / 600.0) * 16384.0)
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
        // Volume=80 -> 13107; VShift draw 50 -> 16384 - 8192; combined >> 14.
        assert_eq!(resolved.event_linear, combine_linear(13107, 8192));
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
        assert_eq!(entry.volume_linear, 9011); // ftol(0.55f * 16384)
        for invalid in ["", "MissingCloakEvent", "vnavupa"] {
            assert!(
                registered_entry(invalid, &registry).is_none(),
                "an invalid Voc identity must not become a raw-bag fallback: {invalid}"
            );
        }
    }

    #[test]
    fn sfx_rng_honours_inclusive_bounds_and_equal_bounds() {
        let mut rng = SfxRng::seeded(42);
        assert_eq!(rng.ranged(5, 5), 5);
        for _ in 0..1000 {
            let draw = rng.ranged(0, 2);
            assert!((0..=2).contains(&draw));
            let reversed = rng.ranged(3, -3);
            assert!((-3..=3).contains(&reversed));
        }
    }

    fn test_decoded_audio() -> DecodedAudio {
        DecodedAudio {
            samples: vec![0.0, 0.0],
            sample_rate: 22_050,
            channels: 2,
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
    fn options_profile_production_routes_sound_and_direct_voice_independently() {
        let half = native_volume_amplitude(8192);
        for (sound_volume, voice_volume, expected_sound, expected_voice) in
            [(0.0, 1.0, 0.0, half), (1.0, 0.0, half, 0.0)]
        {
            let scales = test_output_scales(sound_volume, voice_volume, 1.0, 1.0);
            let sound = prepare_normal_sfx_output(test_decoded_audio(), 8192, scales);
            let direct_voice = prepare_direct_voice_output(test_decoded_audio(), 8192, scales);

            assert_eq!(sound.initial_volume, expected_sound);
            assert_eq!(direct_voice.initial_volume, expected_voice);
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
        let started_with_voice_enabled = prepare_direct_voice_output(
            test_decoded_audio(),
            13107,
            test_output_scales(0.0, 1.0, 1.0, 1.0),
        );
        assert!(
            (started_with_voice_enabled.initial_volume - native_volume_amplitude(13107)).abs()
                < f32::EPSILON
        );
        let started_with_voice_muted = prepare_direct_voice_output(
            test_decoded_audio(),
            13107,
            test_output_scales(1.0, 0.0, 1.0, 1.0),
        );
        assert_eq!(started_with_voice_muted.initial_volume, 0.0);
    }

    /// An idle voice slot with an empty queue reports no active voices. Skips
    /// gracefully when no audio device is available (CI).
    #[test]
    fn voices_active_false_when_idle() {
        let Some(player) = SfxPlayer::new() else {
            return;
        };
        assert!(!player.voices_active());
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

    #[test]
    fn test_decode_wav_8bit_mono() {
        let pcm: Vec<u8> = vec![128, 128, 128, 128];
        let wav = build_test_wav(11025, 8, 1, &pcm);
        let decoded = decode_wav(&wav, "test.wav").expect("should decode");
        assert_eq!(decoded.sample_rate, 11025);
        for s in &decoded.samples {
            assert!(s.abs() < 0.01);
        }
    }

    #[test]
    fn test_decode_wav_16bit_stereo() {
        let pcm: Vec<u8> = vec![0xE8, 0x03, 0x18, 0xFC, 0x00, 0x00, 0x00, 0x00];
        let wav = build_test_wav(44100, 16, 2, &pcm);
        let decoded = decode_wav(&wav, "test.wav").expect("should decode");
        assert_eq!(decoded.samples.len(), 4); // 2 stereo frames, already 2ch
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

    #[test]
    fn test_decode_wav_too_short() {
        assert!(decode_wav(&[0u8; 10], "short.wav").is_none());
    }

    #[test]
    fn test_decode_pcm_empty() {
        let samples = decode_pcm(&[], 1, 16);
        assert!(samples.is_empty());
    }
}
