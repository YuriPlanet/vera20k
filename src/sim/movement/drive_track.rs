//! Drive locomotor track system — pre-computed curved paths for vehicle movement.
//!
//! The original RA2/YR engine uses pre-computed movement curves ("tracks") so
//! that vehicles turn smoothly while moving instead of stop-rotate-go. Track
//! data was extracted from the original engine.
//!
//! - `TurnTrack[72]` — maps octant pairs to curve indices
//! - `RawTrack[16]` — curve metadata (pointer, jump, entry, cell)
//! - `TrackType` point arrays — the actual curve points
//!
//! Each track is a sequence of (x, y, facing) points in lepton coordinates
//! (256 leptons = 1 cell). Vehicles advance through these points each tick
//! based on their speed, producing smooth curved movement and gradual facing
//! changes without separate rotation phases.
//!
//! **The 72-entry turn table at `0x007E7B28` is indexed `from_octant * 8 +
//! to_octant`, not by turn angle.** Entry `i`'s `+4` dword is `(i & 7) * 0x20`,
//! so the low three bits are the destination octant; entries 64..71 are
//! `Force_Track`-only specials. Any comment describing it as "one entry per 5
//! degrees" is wrong — the code selects correctly, the prose did not.
//!
//! ## How it works
//! 1. When a vehicle transitions between cells with a facing change, the turn
//!    octant pair selects a TurnTrack entry (72 entries, `from*8 + to` plus
//!    eight Force_Track specials).
//! 2. The TurnTrack references a RawTrack (normal or short curve variant).
//! 3. The RawTrack's point array defines the smooth path through the cell.
//! 4. Each tick, `point_index` advances based on speed; position and facing
//!    are read from the current TrackPoint.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on util/fixed_math.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use crate::util::fixed_math::SimFixed;

/// Fixed cost per track point step, matching the original engine's movement
/// budget system (subtracts 7 from the budget per step).
const TRACK_STEP_COST: i32 = 7;

/// Apply Transform_Track_Coords direction flags to a track point.
///
/// The same base track curve serves multiple turn orientations via mirror/flip:
/// - bit 0 (1): swap x and y
/// - bit 1 (2): negate x
/// - bit 2 (4): negate y
///
/// These are the lower 3 bits of the TurnTrack flags field. Applied in order:
/// first swap, then negate-x, then negate-y. Facing is adjusted to match.
/// Matches the original Transform_Track_Coords algorithm.
pub(super) fn transform_track_point(x: i16, y: i16, facing: u8, flags: u8) -> (i16, i16, u8) {
    let mut tx = x;
    let mut ty = y;
    let mut tf = facing;

    if flags & 1 != 0 {
        std::mem::swap(&mut tx, &mut ty);
        tf = (tf as i16).wrapping_neg().wrapping_sub(0x40) as u8;
    }
    if flags & 2 != 0 {
        tx = -tx;
        tf = (tf as i16).wrapping_neg() as u8;
    }
    if flags & 4 != 0 {
        ty = -ty;
        tf = (tf as i16).wrapping_neg().wrapping_sub(0x80i16) as u8;
    }
    (tx, ty, tf)
}

/// Integer floor division (rounds toward negative infinity).
/// Rust's `/` operator truncates toward zero, which gives wrong results
/// for negative dividends when we need floor division for cell coordinate
/// computation (e.g., -1 / 256 should be -1, not 0).
fn floor_div(a: i32, b: i32) -> i32 {
    let d = a / b;
    let r = a % b;
    if (r != 0) && ((r ^ b) < 0) { d - 1 } else { d }
}

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

/// One point on a pre-computed vehicle movement curve.
///
/// Position is in lepton offsets within the cell. In the original engine,
/// (0, 0) is the cell reference point; values range roughly -256..256
/// depending on whether the curve crosses cell boundaries.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct TrackPoint {
    /// Lepton offset X within cell.
    pub x: i16,
    /// Lepton offset Y within cell.
    pub y: i16,
    /// Body facing at this point (0-255, RA2 convention).
    pub facing: u8,
}

/// Metadata for one base curve definition.
///
/// There are 16 raw tracks covering different turn geometries.
/// Each references a slice of the global TRACK_POINTS array.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct RawTrack {
    /// Index into TRACK_POINTS where this track's point array starts.
    pub points_start: u16,
    /// Number of points in this track's point array.
    pub points_count: u16,
    /// Index within the point array where the vehicle starts following.
    /// Some tracks have lead-in points that are skipped.
    pub entry_index: u16,
    /// Point index where track chaining is attempted (binary +0x04).
    /// At this point the engine tries to start a follow-on track curve.
    /// -1 = no chaining for this track.
    pub chain_index: i16,
    /// Point index for the occupation handoff callback (binary +0x0C).
    /// This is distinct from coordinate-driven object-list cell crossing.
    /// -1 = no occupation handoff point for this track.
    pub occupation_handoff_point_index: i16,
}

/// Turn angle to track curve mapping.
///
/// There are 72 TurnTrack entries: 64 indexed `from_octant * 8 + to_octant`,
/// then eight `Force_Track`-only specials at 64..71.
/// Each maps to a RawTrack index for normal-speed and short/fast turns.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct TurnTrack {
    /// Index into RAW_TRACKS for the smooth (normal speed) curve.
    pub normal_track: u8,
    /// Index into RAW_TRACKS for the quick (high speed) curve.
    pub short_track: u8,
    /// Target facing after completing this turn (0-255).
    pub target_facing: u8,
    /// Control flags — encodes movement direction context.
    pub flags: u8,
}

/// Runtime state for a vehicle currently following a track curve.
///
/// Created when a Drive vehicle begins a cell transition that involves a
/// facing change. Consumed by tick_movement to advance position/facing along
/// the track. Cleared when the track completes or the vehicle is rerouted.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DriveTrackState {
    /// Index into RAW_TRACKS for the active curve.
    pub raw_track_index: u8,
    /// Current position within the track's point array.
    pub point_index: u16,
    /// The curve has not occupied its first point yet.
    ///
    /// Native's cursor (`[EBP+0x5C]`) is a signed dword that the step loop reads
    /// *before* incrementing (`0x004B1596` read, `0x004B1F4F INC` at the tail),
    /// so a fresh selection starting at zero (`0x004B4659`) occupies
    /// `points[0]` on its first paid step. VERA's cursor is unsigned and doubles
    /// as "the point we are standing on", so it has no way to spell native's
    /// pre-start state; this flag is that state.
    ///
    /// A chained curve installs `entry_index - 1` and leaves this clear, because
    /// native's chain rejoins the loop at the tail increment and so renders
    /// `points[entry_index]` first.
    // Deliberately NOT `#[serde(default)]`. This field sits mid-record and the
    // snapshot is bincode, which cannot default a missing mid-record field - see
    // the v165 note in `snapshot.rs`. The attribute would only mislead the next
    // reader into thinking a pre-v165 save decodes.
    pub before_first_point: bool,
    /// Movement budget remaining from the previous tick. The original engine
    /// carries leftover budget across ticks so
    /// faster vehicles process more track points and fractional progress isn't
    /// lost. Each tick adds `speed * dt` to this residual.
    pub residual: i32,
    /// Transform flags from the TurnTrack entry (lower 3 bits).
    /// Applied to raw track point coordinates via `transform_track_point`.
    pub transform_flags: u8,
    /// Lepton X offset from current cell origin to the head_to reference point.
    /// Set at track start: `dx * 256 + 128` where dx = head_to.x - current.x.
    /// Track points are offsets from head_to (destination cell center), so this
    /// maps them to sub-cell coordinates relative to the current cell.
    pub head_offset_x: i32,
    /// Lepton Y offset from current cell origin to the head_to reference point.
    pub head_offset_y: i32,
    /// Lepton X offset applied after a mid-track cell transition.
    /// When the vehicle crosses a cell boundary during a track curve,
    /// rx/ry are updated but head_offset remains fixed. This offset shifts
    /// subsequent track positions to the new cell's frame.
    pub cell_offset_x: i32,
    /// Lepton Y offset applied after a mid-track cell transition.
    pub cell_offset_y: i32,
    /// Post-turn facing (TURN_TRACKS[turn_track_index].target_facing).
    /// Read by the chain caller as the "from-dir" for chain-target track
    /// selection — using the live entity facing would pick the wrong curve
    /// mid-turn since entity.facing is interpolated along the track.
    pub target_facing: u8,
}

/// One-shot DriveLocomotion `Force_Track` state.
///
/// Unlike normal drive tracks, this is not tied to a `MovementTarget` path
/// step. Building undock/refinery-exit code can force a special TurnTrack
/// index directly, then ordinary movement resumes after the curve clears.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ForcedDriveTrackState {
    /// Index into TURN_TRACKS. `0x47` is the **Tank Bunker eject**, not a refinery
    /// exit: `BuildingClass::UndockUnit` @ `0x004593A0` and
    /// `BuildingClass::ReleaseDockedHarvester` @ `0x004595C0` both push it, and
    /// both early-return unless the building's `+0x2E4` dock link is set, which
    /// only the bunker installer @ `0x00458E50` sets. VERA nevertheless uses it on
    /// the refinery interrupt path — VERA-internal, gamemd equivalent UNCHECKED.
    /// Trigger: a refinery sold or destroyed with a miner on the pad. Player
    /// effect: VERA runs a forced curve gamemd may not run there at all.
    /// Frequency: a handful of visible moments per ordinary match. Downstream
    /// risk: `sell_refinery_interrupts_docked_miner_with_force_track_0x47` pins
    /// the VERA behaviour, so settling this means re-reading that gate first.
    pub turn_track_index: u8,
    /// Runtime RawTrack progress.
    pub track: DriveTrackState,
    /// Full movement speed used for the forced curve.
    pub speed: SimFixed,
}

// ---------------------------------------------------------------------------
// Extracted track data — TurnTrack[72]
// ---------------------------------------------------------------------------

/// 72 turn configurations extracted from the original engine.
/// Each entry is 12 bytes: {normal_idx: u8, short_idx: u8, pad: [u8;2], face: i32, flag: i32}.
/// Indexed `from_octant * 8 + to_octant` for 0..63, then eight
/// `Force_Track`-only specials at 64..71. NOT indexed by turn angle.
pub const TURN_TRACKS: [TurnTrack; 72] = [
    // Entry  0: straight ahead (no turn)
    TurnTrack {
        normal_track: 1,
        short_track: 0,
        target_facing: 0x00,
        flags: 0,
    },
    // Entry  1: slight right turn
    TurnTrack {
        normal_track: 3,
        short_track: 7,
        target_facing: 0x20,
        flags: 8,
    },
    // Entry  2: 45° right
    TurnTrack {
        normal_track: 4,
        short_track: 9,
        target_facing: 0x40,
        flags: 8,
    },
    // Entry  3
    TurnTrack {
        normal_track: 0,
        short_track: 0,
        target_facing: 0x60,
        flags: 0,
    },
    // Entry  4
    TurnTrack {
        normal_track: 0,
        short_track: 0,
        target_facing: 0x80,
        flags: 0,
    },
    // Entry  5
    TurnTrack {
        normal_track: 0,
        short_track: 0,
        target_facing: 0xA0,
        flags: 0,
    },
    // Entry  6
    TurnTrack {
        normal_track: 4,
        short_track: 9,
        target_facing: 0xC0,
        flags: 10,
    },
    // Entry  7
    TurnTrack {
        normal_track: 3,
        short_track: 7,
        target_facing: 0xE0,
        flags: 10,
    },
    // Entry  8
    TurnTrack {
        normal_track: 6,
        short_track: 8,
        target_facing: 0x00,
        flags: 15,
    },
    // Entry  9
    TurnTrack {
        normal_track: 2,
        short_track: 0,
        target_facing: 0x20,
        flags: 0,
    },
    // Entry 10
    TurnTrack {
        normal_track: 6,
        short_track: 8,
        target_facing: 0x40,
        flags: 8,
    },
    // Entry 11
    TurnTrack {
        normal_track: 5,
        short_track: 10,
        target_facing: 0x60,
        flags: 8,
    },
    // Entry 12
    TurnTrack {
        normal_track: 0,
        short_track: 0,
        target_facing: 0x80,
        flags: 0,
    },
    // Entry 13
    TurnTrack {
        normal_track: 0,
        short_track: 0,
        target_facing: 0xA0,
        flags: 0,
    },
    // Entry 14
    TurnTrack {
        normal_track: 0,
        short_track: 0,
        target_facing: 0xC0,
        flags: 0,
    },
    // Entry 15
    TurnTrack {
        normal_track: 5,
        short_track: 10,
        target_facing: 0xE0,
        flags: 15,
    },
    // Entry 16
    TurnTrack {
        normal_track: 4,
        short_track: 9,
        target_facing: 0x00,
        flags: 15,
    },
    // Entry 17
    TurnTrack {
        normal_track: 3,
        short_track: 7,
        target_facing: 0x20,
        flags: 15,
    },
    // Entry 18: 90° turn
    TurnTrack {
        normal_track: 1,
        short_track: 0,
        target_facing: 0x40,
        flags: 3,
    },
    // Entry 19
    TurnTrack {
        normal_track: 3,
        short_track: 7,
        target_facing: 0x60,
        flags: 11,
    },
    // Entry 20
    TurnTrack {
        normal_track: 4,
        short_track: 9,
        target_facing: 0x80,
        flags: 11,
    },
    // Entry 21
    TurnTrack {
        normal_track: 0,
        short_track: 0,
        target_facing: 0xA0,
        flags: 0,
    },
    // Entry 22
    TurnTrack {
        normal_track: 0,
        short_track: 0,
        target_facing: 0xC0,
        flags: 0,
    },
    // Entry 23
    TurnTrack {
        normal_track: 0,
        short_track: 0,
        target_facing: 0xE0,
        flags: 0,
    },
    // Entry 24
    TurnTrack {
        normal_track: 0,
        short_track: 0,
        target_facing: 0x00,
        flags: 0,
    },
    // Entry 25
    TurnTrack {
        normal_track: 5,
        short_track: 10,
        target_facing: 0x20,
        flags: 12,
    },
    // Entry 26
    TurnTrack {
        normal_track: 6,
        short_track: 8,
        target_facing: 0x40,
        flags: 12,
    },
    // Entry 27
    TurnTrack {
        normal_track: 2,
        short_track: 0,
        target_facing: 0x60,
        flags: 4,
    },
    // Entry 28
    TurnTrack {
        normal_track: 6,
        short_track: 8,
        target_facing: 0x80,
        flags: 11,
    },
    // Entry 29
    TurnTrack {
        normal_track: 5,
        short_track: 10,
        target_facing: 0xA0,
        flags: 11,
    },
    // Entry 30
    TurnTrack {
        normal_track: 0,
        short_track: 0,
        target_facing: 0xC0,
        flags: 0,
    },
    // Entry 31
    TurnTrack {
        normal_track: 0,
        short_track: 0,
        target_facing: 0xE0,
        flags: 0,
    },
    // Entry 32
    TurnTrack {
        normal_track: 0,
        short_track: 0,
        target_facing: 0x00,
        flags: 0,
    },
    // Entry 33
    TurnTrack {
        normal_track: 0,
        short_track: 0,
        target_facing: 0x20,
        flags: 0,
    },
    // Entry 34
    TurnTrack {
        normal_track: 4,
        short_track: 9,
        target_facing: 0x40,
        flags: 12,
    },
    // Entry 35
    TurnTrack {
        normal_track: 3,
        short_track: 7,
        target_facing: 0x60,
        flags: 12,
    },
    // Entry 36: 180° U-turn
    TurnTrack {
        normal_track: 1,
        short_track: 0,
        target_facing: 0x80,
        flags: 4,
    },
    // Entry 37
    TurnTrack {
        normal_track: 3,
        short_track: 7,
        target_facing: 0xA0,
        flags: 14,
    },
    // Entry 38
    TurnTrack {
        normal_track: 4,
        short_track: 9,
        target_facing: 0xC0,
        flags: 14,
    },
    // Entry 39
    TurnTrack {
        normal_track: 0,
        short_track: 0,
        target_facing: 0xE0,
        flags: 0,
    },
    // Entry 40
    TurnTrack {
        normal_track: 0,
        short_track: 0,
        target_facing: 0x00,
        flags: 0,
    },
    // Entry 41
    TurnTrack {
        normal_track: 0,
        short_track: 0,
        target_facing: 0x20,
        flags: 0,
    },
    // Entry 42
    TurnTrack {
        normal_track: 0,
        short_track: 0,
        target_facing: 0x40,
        flags: 0,
    },
    // Entry 43
    TurnTrack {
        normal_track: 5,
        short_track: 10,
        target_facing: 0x60,
        flags: 9,
    },
    // Entry 44
    TurnTrack {
        normal_track: 6,
        short_track: 8,
        target_facing: 0x80,
        flags: 9,
    },
    // Entry 45
    TurnTrack {
        normal_track: 2,
        short_track: 0,
        target_facing: 0xA0,
        flags: 1,
    },
    // Entry 46
    TurnTrack {
        normal_track: 6,
        short_track: 8,
        target_facing: 0xC0,
        flags: 14,
    },
    // Entry 47
    TurnTrack {
        normal_track: 5,
        short_track: 10,
        target_facing: 0xE0,
        flags: 14,
    },
    // Entry 48
    TurnTrack {
        normal_track: 4,
        short_track: 9,
        target_facing: 0x00,
        flags: 13,
    },
    // Entry 49
    TurnTrack {
        normal_track: 0,
        short_track: 0,
        target_facing: 0x20,
        flags: 0,
    },
    // Entry 50
    TurnTrack {
        normal_track: 0,
        short_track: 0,
        target_facing: 0x40,
        flags: 0,
    },
    // Entry 51
    TurnTrack {
        normal_track: 0,
        short_track: 0,
        target_facing: 0x60,
        flags: 0,
    },
    // Entry 52
    TurnTrack {
        normal_track: 4,
        short_track: 9,
        target_facing: 0x80,
        flags: 9,
    },
    // Entry 53
    TurnTrack {
        normal_track: 3,
        short_track: 7,
        target_facing: 0xA0,
        flags: 9,
    },
    // Entry 54
    TurnTrack {
        normal_track: 1,
        short_track: 0,
        target_facing: 0xC0,
        flags: 1,
    },
    // Entry 55
    TurnTrack {
        normal_track: 3,
        short_track: 7,
        target_facing: 0xE0,
        flags: 13,
    },
    // Entry 56
    TurnTrack {
        normal_track: 6,
        short_track: 8,
        target_facing: 0x00,
        flags: 13,
    },
    // Entry 57
    TurnTrack {
        normal_track: 5,
        short_track: 10,
        target_facing: 0x20,
        flags: 13,
    },
    // Entry 58
    TurnTrack {
        normal_track: 0,
        short_track: 0,
        target_facing: 0x40,
        flags: 0,
    },
    // Entry 59
    TurnTrack {
        normal_track: 0,
        short_track: 0,
        target_facing: 0x60,
        flags: 0,
    },
    // Entry 60
    TurnTrack {
        normal_track: 0,
        short_track: 0,
        target_facing: 0x80,
        flags: 0,
    },
    // Entry 61
    TurnTrack {
        normal_track: 5,
        short_track: 10,
        target_facing: 0xA0,
        flags: 10,
    },
    // Entry 62
    TurnTrack {
        normal_track: 6,
        short_track: 8,
        target_facing: 0xC0,
        flags: 10,
    },
    // Entry 63
    TurnTrack {
        normal_track: 2,
        short_track: 0,
        target_facing: 0xE0,
        flags: 2,
    },
    // Entry 64: special — tracks 11+
    TurnTrack {
        normal_track: 11,
        short_track: 11,
        target_facing: 0xA0,
        flags: 0,
    },
    // Entry 65
    TurnTrack {
        normal_track: 12,
        short_track: 12,
        target_facing: 0xA0,
        flags: 0,
    },
    // Entry 66
    TurnTrack {
        normal_track: 13,
        short_track: 13,
        target_facing: 0xA0,
        flags: 0,
    },
    // Entry 67
    TurnTrack {
        normal_track: 14,
        short_track: 14,
        target_facing: 0x20,
        flags: 0,
    },
    // Entry 68
    TurnTrack {
        normal_track: 14,
        short_track: 14,
        target_facing: 0x60,
        flags: 4,
    },
    // Entry 69
    TurnTrack {
        normal_track: 14,
        short_track: 14,
        target_facing: 0xA0,
        flags: 1,
    },
    // Entry 70
    TurnTrack {
        normal_track: 14,
        short_track: 14,
        target_facing: 0xE0,
        flags: 2,
    },
    // Entry 71
    TurnTrack {
        normal_track: 15,
        short_track: 15,
        target_facing: 0xC0,
        flags: 0,
    },
];

// ---------------------------------------------------------------------------
// Extracted track data — RawTrack[16]
// ---------------------------------------------------------------------------

/// 16 base curve definitions extracted from the original engine.
/// Each references a slice of TRACK_POINTS via points_start/points_count.
pub const RAW_TRACKS: [RawTrack; 16] = [
    // Track 0: null/empty (binary has f1=0, f2=192, f3=0 — unused garbage)
    RawTrack {
        points_start: 0,
        points_count: 0,
        entry_index: 192,
        chain_index: 0,
        occupation_handoff_point_index: 0,
    },
    // Track 1: straight north (23 points, sentinel-terminated)
    RawTrack {
        points_start: 0,
        points_count: 23,
        entry_index: 0,
        chain_index: -1,
        occupation_handoff_point_index: -1,
    },
    // Track 2: straight NE diagonal (31 points)
    RawTrack {
        points_start: 23,
        points_count: 31,
        entry_index: 0,
        chain_index: -1,
        occupation_handoff_point_index: -1,
    },
    // Track 3: turning curve N→NE (54 points, crosses cell)
    RawTrack {
        points_start: 54,
        points_count: 54,
        entry_index: 12,
        chain_index: 37,
        occupation_handoff_point_index: 22,
    },
    // Track 4: turning curve N→E (38 points, crosses cell)
    RawTrack {
        points_start: 108,
        points_count: 38,
        entry_index: 11,
        chain_index: 26,
        occupation_handoff_point_index: 19,
    },
    // Track 5: wide turn NE→E (61 points, crosses cell)
    RawTrack {
        points_start: 146,
        points_count: 61,
        entry_index: 15,
        chain_index: 45,
        occupation_handoff_point_index: 31,
    },
    // Track 6: wide turn NE→E variant (56 points, crosses cell)
    RawTrack {
        points_start: 207,
        points_count: 56,
        entry_index: 16,
        chain_index: 44,
        occupation_handoff_point_index: 27,
    },
    // Track 7: short curve A (27 points)
    RawTrack {
        points_start: 263,
        points_count: 27,
        entry_index: 0,
        chain_index: -1,
        occupation_handoff_point_index: -1,
    },
    // Track 8: short curve B (21 points)
    RawTrack {
        points_start: 290,
        points_count: 21,
        entry_index: 0,
        chain_index: -1,
        occupation_handoff_point_index: -1,
    },
    // Track 9: short curve C (30 points)
    RawTrack {
        points_start: 311,
        points_count: 30,
        entry_index: 0,
        chain_index: -1,
        occupation_handoff_point_index: -1,
    },
    // Track 10: short curve D (27 points)
    RawTrack {
        points_start: 341,
        points_count: 27,
        entry_index: 0,
        chain_index: -1,
        occupation_handoff_point_index: -1,
    },
    // Track 11: special A (13 points)
    RawTrack {
        points_start: 368,
        points_count: 13,
        entry_index: 0,
        chain_index: -1,
        occupation_handoff_point_index: -1,
    },
    // Track 12: special B (12 points)
    RawTrack {
        points_start: 381,
        points_count: 12,
        entry_index: 0,
        chain_index: -1,
        occupation_handoff_point_index: -1,
    },
    // Track 13: special C — long straight east (68 points)
    RawTrack {
        points_start: 393,
        points_count: 68,
        entry_index: 0,
        chain_index: -1,
        occupation_handoff_point_index: -1,
    },
    // Track 14: special D — short NE diagonal (15 points)
    RawTrack {
        points_start: 461,
        points_count: 15,
        entry_index: 0,
        chain_index: -1,
        occupation_handoff_point_index: -1,
    },
    // Track 15: special E — SE to S arc (16 points)
    RawTrack {
        points_start: 476,
        points_count: 16,
        entry_index: 0,
        chain_index: -1,
        occupation_handoff_point_index: -1,
    },
];

// ---------------------------------------------------------------------------
// Extracted track point data
// ---------------------------------------------------------------------------

/// Track 1 points (straight northward movement). Extracted from 0x7E6258.
/// X=0 throughout, Y decreases by ~11 per point, Face=0 (north).
const TRACK1_POINTS: [TrackPoint; 23] = [
    TrackPoint {
        x: 0,
        y: 245,
        facing: 0,
    },
    TrackPoint {
        x: 0,
        y: 234,
        facing: 0,
    },
    TrackPoint {
        x: 0,
        y: 223,
        facing: 0,
    },
    TrackPoint {
        x: 0,
        y: 212,
        facing: 0,
    },
    TrackPoint {
        x: 0,
        y: 201,
        facing: 0,
    },
    TrackPoint {
        x: 0,
        y: 190,
        facing: 0,
    },
    TrackPoint {
        x: 0,
        y: 179,
        facing: 0,
    },
    TrackPoint {
        x: 0,
        y: 168,
        facing: 0,
    },
    TrackPoint {
        x: 0,
        y: 157,
        facing: 0,
    },
    TrackPoint {
        x: 0,
        y: 146,
        facing: 0,
    },
    TrackPoint {
        x: 0,
        y: 135,
        facing: 0,
    },
    TrackPoint {
        x: 0,
        y: 124,
        facing: 0,
    },
    TrackPoint {
        x: 0,
        y: 113,
        facing: 0,
    },
    TrackPoint {
        x: 0,
        y: 102,
        facing: 0,
    },
    TrackPoint {
        x: 0,
        y: 91,
        facing: 0,
    },
    TrackPoint {
        x: 0,
        y: 80,
        facing: 0,
    },
    TrackPoint {
        x: 0,
        y: 69,
        facing: 0,
    },
    TrackPoint {
        x: 0,
        y: 58,
        facing: 0,
    },
    TrackPoint {
        x: 0,
        y: 47,
        facing: 0,
    },
    TrackPoint {
        x: 0,
        y: 36,
        facing: 0,
    },
    TrackPoint {
        x: 0,
        y: 25,
        facing: 0,
    },
    TrackPoint {
        x: 0,
        y: 14,
        facing: 0,
    },
    TrackPoint {
        x: 0,
        y: 3,
        facing: 0,
    },
];

/// Track 2 points (straight NE diagonal movement). Extracted from the original engine.
/// X increases and Y decreases toward zero, usually by8 (point15/16 use7/9).
/// Face=0x20 (NE) throughout.
/// 32 points x 12 bytes = 384 bytes.
const TRACK2_POINTS: [TrackPoint; 31] = [
    TrackPoint {
        x: -248,
        y: 248,
        facing: 32,
    },
    TrackPoint {
        x: -240,
        y: 240,
        facing: 32,
    },
    TrackPoint {
        x: -232,
        y: 232,
        facing: 32,
    },
    TrackPoint {
        x: -224,
        y: 224,
        facing: 32,
    },
    TrackPoint {
        x: -216,
        y: 216,
        facing: 32,
    },
    TrackPoint {
        x: -208,
        y: 208,
        facing: 32,
    },
    TrackPoint {
        x: -200,
        y: 200,
        facing: 32,
    },
    TrackPoint {
        x: -192,
        y: 192,
        facing: 32,
    },
    TrackPoint {
        x: -184,
        y: 184,
        facing: 32,
    },
    TrackPoint {
        x: -176,
        y: 176,
        facing: 32,
    },
    TrackPoint {
        x: -168,
        y: 168,
        facing: 32,
    },
    TrackPoint {
        x: -160,
        y: 160,
        facing: 32,
    },
    TrackPoint {
        x: -152,
        y: 152,
        facing: 32,
    },
    TrackPoint {
        x: -144,
        y: 144,
        facing: 32,
    },
    TrackPoint {
        x: -136,
        y: 136,
        facing: 32,
    },
    TrackPoint {
        // Retail RawTrack2[15] is deliberately one lepton past -128/128.
        // Original read-only point array; locomotor_track_cursor corpus.
        x: -129,
        y: 129,
        facing: 32,
    },
    TrackPoint {
        x: -120,
        y: 120,
        facing: 32,
    },
    TrackPoint {
        x: -112,
        y: 112,
        facing: 32,
    },
    TrackPoint {
        x: -104,
        y: 104,
        facing: 32,
    },
    TrackPoint {
        x: -96,
        y: 96,
        facing: 32,
    },
    TrackPoint {
        x: -88,
        y: 88,
        facing: 32,
    },
    TrackPoint {
        x: -80,
        y: 80,
        facing: 32,
    },
    TrackPoint {
        x: -72,
        y: 72,
        facing: 32,
    },
    TrackPoint {
        x: -64,
        y: 64,
        facing: 32,
    },
    TrackPoint {
        x: -56,
        y: 56,
        facing: 32,
    },
    TrackPoint {
        x: -48,
        y: 48,
        facing: 32,
    },
    TrackPoint {
        x: -40,
        y: 40,
        facing: 32,
    },
    TrackPoint {
        x: -32,
        y: 32,
        facing: 32,
    },
    TrackPoint {
        x: -24,
        y: 24,
        facing: 32,
    },
    TrackPoint {
        x: -16,
        y: 16,
        facing: 32,
    },
    TrackPoint {
        x: -8,
        y: 8,
        facing: 32,
    },
];

/// Track 3 points (turning curve: north → northeast). Extracted from 0x7E64F8.
/// 54 points. entry_index=12, jump_index=37, cell_index=22.
///
/// Three phases:
///   Points  0-13: straight north approach (x=-256, face=0, y step ~11)
///   Points 14-36: turning curve (x/y shift, face 1→31 gradual turn)
///   Points 37-54: straight NE exit (face=32, x/y step 8 diagonal)
/// Point 37 is the cell transition (jump_index).
const TRACK3_POINTS: [TrackPoint; 54] = [
    // Phase 1: straight north lead-in (face=0, x=-256)
    TrackPoint {
        x: -256,
        y: 501,
        facing: 0,
    }, //  0
    TrackPoint {
        x: -256,
        y: 490,
        facing: 0,
    }, //  1
    TrackPoint {
        x: -256,
        y: 479,
        facing: 0,
    }, //  2
    TrackPoint {
        x: -256,
        y: 468,
        facing: 0,
    }, //  3
    TrackPoint {
        x: -256,
        y: 457,
        facing: 0,
    }, //  4
    TrackPoint {
        x: -256,
        y: 446,
        facing: 0,
    }, //  5
    TrackPoint {
        x: -256,
        y: 435,
        facing: 0,
    }, //  6
    TrackPoint {
        x: -256,
        y: 424,
        facing: 0,
    }, //  7
    TrackPoint {
        x: -256,
        y: 413,
        facing: 0,
    }, //  8
    TrackPoint {
        x: -256,
        y: 402,
        facing: 0,
    }, //  9
    TrackPoint {
        x: -256,
        y: 391,
        facing: 0,
    }, // 10
    TrackPoint {
        x: -256,
        y: 383,
        facing: 0,
    }, // 11
    TrackPoint {
        x: -256,
        y: 373,
        facing: 0,
    }, // 12  ← entry_index
    TrackPoint {
        x: -256,
        y: 363,
        facing: 0,
    }, // 13
    // Phase 2: turning curve (face increases 0→31, x increases toward 0)
    TrackPoint {
        x: -254,
        y: 352,
        facing: 1,
    }, // 14
    TrackPoint {
        x: -252,
        y: 341,
        facing: 3,
    }, // 15
    TrackPoint {
        x: -250,
        y: 332,
        facing: 4,
    }, // 16
    TrackPoint {
        x: -248,
        y: 321,
        facing: 5,
    }, // 17
    TrackPoint {
        x: -245,
        y: 311,
        facing: 7,
    }, // 18
    TrackPoint {
        x: -241,
        y: 302,
        facing: 8,
    }, // 19
    TrackPoint {
        x: -237,
        y: 292,
        facing: 9,
    }, // 20
    TrackPoint {
        x: -233,
        y: 282,
        facing: 11,
    }, // 21
    TrackPoint {
        x: -229,
        y: 272,
        facing: 12,
    }, // 22
    TrackPoint {
        x: -225,
        y: 263,
        facing: 13,
    }, // 23
    TrackPoint {
        x: -220,
        y: 252,
        facing: 15,
    }, // 24
    TrackPoint {
        x: -216,
        y: 243,
        facing: 16,
    }, // 25
    TrackPoint {
        x: -212,
        y: 236,
        facing: 17,
    }, // 26
    TrackPoint {
        x: -206,
        y: 224,
        facing: 19,
    }, // 27
    TrackPoint {
        x: -202,
        y: 215,
        facing: 20,
    }, // 28
    TrackPoint {
        x: -195,
        y: 207,
        facing: 21,
    }, // 29
    TrackPoint {
        x: -190,
        y: 198,
        facing: 23,
    }, // 30
    TrackPoint {
        x: -183,
        y: 186,
        facing: 24,
    }, // 31
    TrackPoint {
        x: -179,
        y: 176,
        facing: 25,
    }, // 32
    TrackPoint {
        x: -168,
        y: 168,
        facing: 27,
    }, // 33
    TrackPoint {
        x: -160,
        y: 160,
        facing: 28,
    }, // 34
    TrackPoint {
        x: -152,
        y: 152,
        facing: 29,
    }, // 35
    TrackPoint {
        x: -144,
        y: 144,
        facing: 31,
    }, // 36
    // Phase 3: straight NE exit (face=32, diagonal step 8)
    TrackPoint {
        x: -136,
        y: 136,
        facing: 32,
    }, // 37  ← jump_index (cell transition)
    TrackPoint {
        x: -129,
        y: 129,
        facing: 32,
    }, // 38
    TrackPoint {
        x: -120,
        y: 120,
        facing: 32,
    }, // 39
    TrackPoint {
        x: -112,
        y: 112,
        facing: 32,
    }, // 40
    TrackPoint {
        x: -104,
        y: 104,
        facing: 32,
    }, // 41
    TrackPoint {
        x: -96,
        y: 96,
        facing: 32,
    }, // 42
    TrackPoint {
        x: -88,
        y: 88,
        facing: 32,
    }, // 43
    TrackPoint {
        x: -80,
        y: 80,
        facing: 32,
    }, // 44
    TrackPoint {
        x: -72,
        y: 72,
        facing: 32,
    }, // 45
    TrackPoint {
        x: -64,
        y: 64,
        facing: 32,
    }, // 46
    TrackPoint {
        x: -56,
        y: 56,
        facing: 32,
    }, // 47
    TrackPoint {
        x: -48,
        y: 48,
        facing: 32,
    }, // 48
    TrackPoint {
        x: -40,
        y: 40,
        facing: 32,
    }, // 49
    TrackPoint {
        x: -32,
        y: 32,
        facing: 32,
    }, // 50
    TrackPoint {
        x: -24,
        y: 24,
        facing: 32,
    }, // 51
    TrackPoint {
        x: -16,
        y: 16,
        facing: 32,
    }, // 52
    TrackPoint {
        x: -8,
        y: 8,
        facing: 32,
    }, // 53
];

/// Track 4 points (turning curve: north → east, 90°). Extracted from 0x7E6790.
/// 38 points. entry_index=11, jump_index=26, cell_index=19.
///
/// Three phases:
///   Points  0-5:  straight north approach (x≈-256, face=0)
///   Points  6-28: turning curve (face 1→63, wide 90° arc)
///   Points 29-37: straight east exit (face=64, y≈0)
/// Point 26 is the cell transition (jump_index).
const TRACK4_POINTS: [TrackPoint; 38] = [
    // Phase 1: straight north lead-in
    TrackPoint {
        x: -256,
        y: 245,
        facing: 0,
    }, //  0
    TrackPoint {
        x: -256,
        y: 235,
        facing: 0,
    }, //  1
    TrackPoint {
        x: -256,
        y: 224,
        facing: 0,
    }, //  2
    TrackPoint {
        x: -256,
        y: 213,
        facing: 0,
    }, //  3
    TrackPoint {
        x: -255,
        y: 203,
        facing: 0,
    }, //  4
    TrackPoint {
        x: -253,
        y: 192,
        facing: 0,
    }, //  5
    // Phase 2: turning curve (face increases 0→64)
    TrackPoint {
        x: -251,
        y: 181,
        facing: 1,
    }, //  6
    TrackPoint {
        x: -249,
        y: 171,
        facing: 1,
    }, //  7
    TrackPoint {
        x: -246,
        y: 160,
        facing: 2,
    }, //  8
    TrackPoint {
        x: -243,
        y: 149,
        facing: 3,
    }, //  9
    TrackPoint {
        x: -240,
        y: 139,
        facing: 4,
    }, // 10
    TrackPoint {
        x: -236,
        y: 127,
        facing: 5,
    }, // 11  ← entry_index
    TrackPoint {
        x: -232,
        y: 117,
        facing: 8,
    }, // 12
    TrackPoint {
        x: -228,
        y: 109,
        facing: 12,
    }, // 13
    TrackPoint {
        x: -222,
        y: 99,
        facing: 16,
    }, // 14
    TrackPoint {
        x: -219,
        y: 90,
        facing: 20,
    }, // 15
    TrackPoint {
        x: -213,
        y: 82,
        facing: 23,
    }, // 16
    TrackPoint {
        x: -206,
        y: 72,
        facing: 27,
    }, // 17
    TrackPoint {
        x: -201,
        y: 64,
        facing: 32,
    }, // 18
    TrackPoint {
        x: -195,
        y: 56,
        facing: 36,
    }, // 19
    TrackPoint {
        x: -186,
        y: 48,
        facing: 39,
    }, // 20
    TrackPoint {
        x: -177,
        y: 43,
        facing: 43,
    }, // 21
    TrackPoint {
        x: -168,
        y: 36,
        facing: 47,
    }, // 22
    TrackPoint {
        x: -160,
        y: 32,
        facing: 51,
    }, // 23
    TrackPoint {
        x: -147,
        y: 27,
        facing: 54,
    }, // 24
    TrackPoint {
        x: -135,
        y: 23,
        facing: 57,
    }, // 25
    TrackPoint {
        x: -126,
        y: 20,
        facing: 60,
    }, // 26  ← jump_index (cell transition)
    TrackPoint {
        x: -113,
        y: 17,
        facing: 62,
    }, // 27
    TrackPoint {
        x: -104,
        y: 13,
        facing: 63,
    }, // 28
    // Phase 3: straight east exit (face=64)
    TrackPoint {
        x: -94,
        y: 9,
        facing: 64,
    }, // 29
    TrackPoint {
        x: -84,
        y: 6,
        facing: 64,
    }, // 30
    TrackPoint {
        x: -75,
        y: 4,
        facing: 66,
    }, // 31
    TrackPoint {
        x: -64,
        y: 3,
        facing: 64,
    }, // 32
    TrackPoint {
        x: -53,
        y: 2,
        facing: 64,
    }, // 33
    TrackPoint {
        x: -43,
        y: 1,
        facing: 64,
    }, // 34
    TrackPoint {
        x: -32,
        y: 0,
        facing: 64,
    }, // 35
    TrackPoint {
        x: -21,
        y: 0,
        facing: 64,
    }, // 36
    TrackPoint {
        x: -11,
        y: 0,
        facing: 64,
    }, // 37
];

/// Track 5 points (wide turn: NE → E). Extracted from 0x7E6968.
/// 61 points. entry_index=15, jump_index=45, cell_index=31.
const TRACK5_POINTS: [TrackPoint; 61] = [
    TrackPoint {
        x: -504,
        y: -8,
        facing: 32,
    }, //  0
    TrackPoint {
        x: -496,
        y: -16,
        facing: 32,
    }, //  1
    TrackPoint {
        x: -488,
        y: -24,
        facing: 32,
    }, //  2
    TrackPoint {
        x: -480,
        y: -32,
        facing: 32,
    }, //  3
    TrackPoint {
        x: -472,
        y: -40,
        facing: 32,
    }, //  4
    TrackPoint {
        x: -464,
        y: -48,
        facing: 32,
    }, //  5
    TrackPoint {
        x: -456,
        y: -56,
        facing: 32,
    }, //  6
    TrackPoint {
        x: -448,
        y: -64,
        facing: 32,
    }, //  7
    TrackPoint {
        x: -440,
        y: -72,
        facing: 32,
    }, //  8
    TrackPoint {
        x: -432,
        y: -80,
        facing: 32,
    }, //  9
    TrackPoint {
        x: -424,
        y: -88,
        facing: 32,
    }, // 10
    TrackPoint {
        x: -416,
        y: -96,
        facing: 32,
    }, // 11
    TrackPoint {
        x: -408,
        y: -104,
        facing: 32,
    }, // 12
    TrackPoint {
        x: -400,
        y: -112,
        facing: 32,
    }, // 13
    TrackPoint {
        x: -392,
        y: -120,
        facing: 32,
    }, // 14
    TrackPoint {
        x: -385,
        y: -127,
        facing: 32,
    }, // 15  ← entry_index
    TrackPoint {
        x: -376,
        y: -136,
        facing: 32,
    }, // 16
    TrackPoint {
        x: -368,
        y: -143,
        facing: 32,
    }, // 17
    TrackPoint {
        x: -361,
        y: -150,
        facing: 32,
    }, // 18
    TrackPoint {
        x: -353,
        y: -158,
        facing: 32,
    }, // 19
    TrackPoint {
        x: -344,
        y: -166,
        facing: 32,
    }, // 20
    TrackPoint {
        x: -336,
        y: -173,
        facing: 35,
    }, // 21
    TrackPoint {
        x: -329,
        y: -181,
        facing: 38,
    }, // 22
    TrackPoint {
        x: -322,
        y: -188,
        facing: 41,
    }, // 23
    TrackPoint {
        x: -316,
        y: -194,
        facing: 44,
    }, // 24
    TrackPoint {
        x: -306,
        y: -199,
        facing: 47,
    }, // 25
    TrackPoint {
        x: -296,
        y: -204,
        facing: 50,
    }, // 26
    TrackPoint {
        x: -288,
        y: -208,
        facing: 53,
    }, // 27
    TrackPoint {
        x: -277,
        y: -211,
        facing: 56,
    }, // 28
    TrackPoint {
        x: -267,
        y: -212,
        facing: 59,
    }, // 29
    TrackPoint {
        x: -256,
        y: -213,
        facing: 62,
    }, // 30
    TrackPoint {
        x: -245,
        y: -212,
        facing: 66,
    }, // 31
    TrackPoint {
        x: -235,
        y: -211,
        facing: 69,
    }, // 32
    TrackPoint {
        x: -225,
        y: -208,
        facing: 72,
    }, // 33
    TrackPoint {
        x: -216,
        y: -204,
        facing: 75,
    }, // 34
    TrackPoint {
        x: -208,
        y: -199,
        facing: 78,
    }, // 35
    TrackPoint {
        x: -198,
        y: -194,
        facing: 81,
    }, // 36
    TrackPoint {
        x: -188,
        y: -188,
        facing: 84,
    }, // 37
    TrackPoint {
        x: -181,
        y: -181,
        facing: 87,
    }, // 38
    TrackPoint {
        x: -176,
        y: -173,
        facing: 90,
    }, // 39
    TrackPoint {
        x: -168,
        y: -166,
        facing: 93,
    }, // 40
    TrackPoint {
        x: -160,
        y: -158,
        facing: 96,
    }, // 41
    TrackPoint {
        x: -152,
        y: -150,
        facing: 96,
    }, // 42
    TrackPoint {
        x: -144,
        y: -143,
        facing: 96,
    }, // 43
    TrackPoint {
        x: -136,
        y: -136,
        facing: 96,
    }, // 44
    TrackPoint {
        x: -129,
        y: -129,
        facing: 96,
    }, // 45
    TrackPoint {
        x: -120,
        y: -120,
        facing: 96,
    }, // 46
    TrackPoint {
        x: -112,
        y: -112,
        facing: 96,
    }, // 47
    TrackPoint {
        x: -104,
        y: -104,
        facing: 96,
    }, // 48
    TrackPoint {
        x: -96,
        y: -96,
        facing: 96,
    }, // 49
    TrackPoint {
        x: -88,
        y: -88,
        facing: 96,
    }, // 50
    TrackPoint {
        x: -80,
        y: -80,
        facing: 96,
    }, // 51
    TrackPoint {
        x: -72,
        y: -72,
        facing: 96,
    }, // 52
    TrackPoint {
        x: -64,
        y: -64,
        facing: 96,
    }, // 53
    TrackPoint {
        x: -56,
        y: -56,
        facing: 96,
    }, // 54
    TrackPoint {
        x: -48,
        y: -48,
        facing: 96,
    }, // 55
    TrackPoint {
        x: -40,
        y: -40,
        facing: 96,
    }, // 56
    TrackPoint {
        x: -32,
        y: -32,
        facing: 96,
    }, // 57
    TrackPoint {
        x: -24,
        y: -24,
        facing: 96,
    }, // 58
    TrackPoint {
        x: -16,
        y: -16,
        facing: 96,
    }, // 59
    TrackPoint {
        x: -8,
        y: -8,
        facing: 96,
    }, // 60
];

/// Track 6 points (wide turn: NE → E variant). Extracted from 0x7E6C50.
/// 56 points. entry_index=16, jump_index=44, cell_index=27.
const TRACK6_POINTS: [TrackPoint; 56] = [
    TrackPoint {
        x: -512,
        y: 256,
        facing: 32,
    }, //  0
    TrackPoint {
        x: -504,
        y: 248,
        facing: 32,
    }, //  1
    TrackPoint {
        x: -496,
        y: 240,
        facing: 32,
    }, //  2
    TrackPoint {
        x: -488,
        y: 232,
        facing: 32,
    }, //  3
    TrackPoint {
        x: -480,
        y: 224,
        facing: 32,
    }, //  4
    TrackPoint {
        x: -472,
        y: 216,
        facing: 32,
    }, //  5
    TrackPoint {
        x: -464,
        y: 208,
        facing: 32,
    }, //  6
    TrackPoint {
        x: -456,
        y: 200,
        facing: 32,
    }, //  7
    TrackPoint {
        x: -448,
        y: 192,
        facing: 32,
    }, //  8
    TrackPoint {
        x: -440,
        y: 184,
        facing: 32,
    }, //  9
    TrackPoint {
        x: -432,
        y: 176,
        facing: 32,
    }, // 10
    TrackPoint {
        x: -424,
        y: 168,
        facing: 32,
    }, // 11
    TrackPoint {
        x: -416,
        y: 160,
        facing: 32,
    }, // 12
    TrackPoint {
        x: -408,
        y: 152,
        facing: 32,
    }, // 13
    TrackPoint {
        x: -400,
        y: 144,
        facing: 32,
    }, // 14
    TrackPoint {
        x: -392,
        y: 136,
        facing: 32,
    }, // 15
    TrackPoint {
        x: -385,
        y: 129,
        facing: 32,
    }, // 16  ← entry_index
    TrackPoint {
        x: -376,
        y: 120,
        facing: 32,
    }, // 17
    TrackPoint {
        x: -368,
        y: 112,
        facing: 32,
    }, // 18
    TrackPoint {
        x: -360,
        y: 104,
        facing: 32,
    }, // 19
    TrackPoint {
        x: -352,
        y: 96,
        facing: 32,
    }, // 20
    TrackPoint {
        x: -344,
        y: 88,
        facing: 32,
    }, // 21
    TrackPoint {
        x: -338,
        y: 85,
        facing: 32,
    }, // 22
    TrackPoint {
        x: -328,
        y: 78,
        facing: 35,
    }, // 23
    TrackPoint {
        x: -320,
        y: 72,
        facing: 37,
    }, // 24
    TrackPoint {
        x: -311,
        y: 66,
        facing: 40,
    }, // 25
    TrackPoint {
        x: -302,
        y: 59,
        facing: 43,
    }, // 26
    TrackPoint {
        x: -294,
        y: 55,
        facing: 45,
    }, // 27
    TrackPoint {
        x: -285,
        y: 50,
        facing: 48,
    }, // 28
    TrackPoint {
        x: -277,
        y: 43,
        facing: 51,
    }, // 29
    TrackPoint {
        x: -267,
        y: 38,
        facing: 53,
    }, // 30
    TrackPoint {
        x: -258,
        y: 34,
        facing: 56,
    }, // 31
    TrackPoint {
        x: -248,
        y: 28,
        facing: 59,
    }, // 32
    TrackPoint {
        x: -238,
        y: 25,
        facing: 61,
    }, // 33
    TrackPoint {
        x: -229,
        y: 21,
        facing: 64,
    }, // 34
    TrackPoint {
        x: -218,
        y: 17,
        facing: 64,
    }, // 35
    TrackPoint {
        x: -208,
        y: 14,
        facing: 64,
    }, // 36
    TrackPoint {
        x: -199,
        y: 11,
        facing: 64,
    }, // 37
    TrackPoint {
        x: -189,
        y: 9,
        facing: 64,
    }, // 38
    TrackPoint {
        x: -178,
        y: 7,
        facing: 64,
    }, // 39
    TrackPoint {
        x: -169,
        y: 5,
        facing: 64,
    }, // 40
    TrackPoint {
        x: -158,
        y: 3,
        facing: 64,
    }, // 41
    TrackPoint {
        x: -147,
        y: 1,
        facing: 64,
    }, // 42
    TrackPoint {
        x: -137,
        y: 0,
        facing: 64,
    }, // 43
    TrackPoint {
        x: -129,
        y: 0,
        facing: 64,
    }, // 44
    TrackPoint {
        x: -117,
        y: 0,
        facing: 64,
    }, // 45
    TrackPoint {
        x: -107,
        y: 0,
        facing: 64,
    }, // 46
    TrackPoint {
        x: -96,
        y: 0,
        facing: 64,
    }, // 47
    TrackPoint {
        x: -85,
        y: 0,
        facing: 64,
    }, // 48
    TrackPoint {
        x: -75,
        y: 0,
        facing: 64,
    }, // 49
    TrackPoint {
        x: -64,
        y: 0,
        facing: 64,
    }, // 50
    TrackPoint {
        x: -53,
        y: 0,
        facing: 64,
    }, // 51
    TrackPoint {
        x: -43,
        y: 0,
        facing: 64,
    }, // 52
    TrackPoint {
        x: -32,
        y: 0,
        facing: 64,
    }, // 53
    TrackPoint {
        x: -21,
        y: 0,
        facing: 64,
    }, // 54
    TrackPoint {
        x: -11,
        y: 0,
        facing: 64,
    }, // 55
];

/// Track 7 points (short curve A). Extracted from 0x7E6F00.
/// 27 points. No cell crossing.
const TRACK7_POINTS: [TrackPoint; 27] = [
    TrackPoint {
        x: -1,
        y: 6,
        facing: 0,
    }, //  0
    TrackPoint {
        x: -2,
        y: 12,
        facing: 4,
    }, //  1
    TrackPoint {
        x: -4,
        y: 17,
        facing: 8,
    }, //  2
    TrackPoint {
        x: -6,
        y: 24,
        facing: 12,
    }, //  3
    TrackPoint {
        x: -10,
        y: 31,
        facing: 16,
    }, //  4
    TrackPoint {
        x: -13,
        y: 36,
        facing: 19,
    }, //  5
    TrackPoint {
        x: -16,
        y: 43,
        facing: 22,
    }, //  6
    TrackPoint {
        x: -3,
        y: 48,
        facing: 23,
    }, //  7
    TrackPoint {
        x: -21,
        y: 53,
        facing: 24,
    }, //  8
    TrackPoint {
        x: -24,
        y: 56,
        facing: 25,
    }, //  9
    TrackPoint {
        x: -26,
        y: 60,
        facing: 26,
    }, // 10
    TrackPoint {
        x: -29,
        y: 64,
        facing: 27,
    }, // 11
    TrackPoint {
        x: -32,
        y: 67,
        facing: 28,
    }, // 12
    TrackPoint {
        x: -35,
        y: 70,
        facing: 29,
    }, // 13
    TrackPoint {
        x: -33,
        y: 67,
        facing: 30,
    }, // 14
    TrackPoint {
        x: -31,
        y: 64,
        facing: 30,
    }, // 15
    TrackPoint {
        x: -29,
        y: 60,
        facing: 30,
    }, // 16
    TrackPoint {
        x: -27,
        y: 56,
        facing: 30,
    }, // 17
    TrackPoint {
        x: -25,
        y: 53,
        facing: 31,
    }, // 18
    TrackPoint {
        x: -23,
        y: 48,
        facing: 31,
    }, // 19
    TrackPoint {
        x: -21,
        y: 43,
        facing: 31,
    }, // 20
    TrackPoint {
        x: -19,
        y: 36,
        facing: 31,
    }, // 21
    TrackPoint {
        x: -15,
        y: 31,
        facing: 31,
    }, // 22
    TrackPoint {
        x: -12,
        y: 24,
        facing: 32,
    }, // 23
    TrackPoint {
        x: -9,
        y: 17,
        facing: 32,
    }, // 24
    TrackPoint {
        x: -6,
        y: 12,
        facing: 32,
    }, // 25
    TrackPoint {
        x: -3,
        y: 6,
        facing: 32,
    }, // 26
];

/// Track 8 points (short curve B). Extracted from 0x7E7050.
/// 21 points. No cell crossing.
const TRACK8_POINTS: [TrackPoint; 21] = [
    TrackPoint {
        x: -4,
        y: 3,
        facing: 32,
    }, //  0
    TrackPoint {
        x: -9,
        y: 6,
        facing: 36,
    }, //  1
    TrackPoint {
        x: -15,
        y: 10,
        facing: 40,
    }, //  2
    TrackPoint {
        x: -21,
        y: 12,
        facing: 44,
    }, //  3
    TrackPoint {
        x: -28,
        y: 13,
        facing: 46,
    }, //  4
    TrackPoint {
        x: -36,
        y: 14,
        facing: 48,
    }, //  5
    TrackPoint {
        x: -43,
        y: 15,
        facing: 50,
    }, //  6
    TrackPoint {
        x: -48,
        y: 16,
        facing: 52,
    }, //  7
    TrackPoint {
        x: -55,
        y: 17,
        facing: 54,
    }, //  8
    TrackPoint {
        x: -62,
        y: 18,
        facing: 56,
    }, //  9
    TrackPoint {
        x: -64,
        y: 17,
        facing: 58,
    }, // 10
    TrackPoint {
        x: -62,
        y: 16,
        facing: 60,
    }, // 11
    TrackPoint {
        x: -55,
        y: 14,
        facing: 62,
    }, // 12
    TrackPoint {
        x: -49,
        y: 12,
        facing: 64,
    }, // 13
    TrackPoint {
        x: -43,
        y: 10,
        facing: 64,
    }, // 14
    TrackPoint {
        x: -38,
        y: 8,
        facing: 64,
    }, // 15
    TrackPoint {
        x: -30,
        y: 6,
        facing: 64,
    }, // 16
    TrackPoint {
        x: -23,
        y: 4,
        facing: 64,
    }, // 17
    TrackPoint {
        x: -17,
        y: 2,
        facing: 64,
    }, // 18
    TrackPoint {
        x: -11,
        y: 1,
        facing: 64,
    }, // 19
    TrackPoint {
        x: -7,
        y: 0,
        facing: 64,
    }, // 20
];

/// Track 9 points (short curve C). Extracted from 0x7E7158.
/// 30 points. No cell crossing.
const TRACK9_POINTS: [TrackPoint; 30] = [
    TrackPoint {
        x: 2,
        y: -11,
        facing: 0,
    }, //  0
    TrackPoint {
        x: 4,
        y: -21,
        facing: 2,
    }, //  1
    TrackPoint {
        x: 6,
        y: -32,
        facing: 4,
    }, //  2
    TrackPoint {
        x: 9,
        y: -43,
        facing: 6,
    }, //  3
    TrackPoint {
        x: 12,
        y: -50,
        facing: 9,
    }, //  4
    TrackPoint {
        x: 15,
        y: -56,
        facing: 11,
    }, //  5
    TrackPoint {
        x: 18,
        y: -64,
        facing: 13,
    }, //  6
    TrackPoint {
        x: 21,
        y: -72,
        facing: 16,
    }, //  7
    TrackPoint {
        x: 18,
        y: -64,
        facing: 18,
    }, //  8
    TrackPoint {
        x: 14,
        y: -56,
        facing: 20,
    }, //  9
    TrackPoint {
        x: 10,
        y: -50,
        facing: 22,
    }, // 10
    TrackPoint {
        x: 4,
        y: -43,
        facing: 24,
    }, // 11
    TrackPoint {
        x: 0,
        y: -34,
        facing: 26,
    }, // 12
    TrackPoint {
        x: -8,
        y: -23,
        facing: 28,
    }, // 13
    TrackPoint {
        x: -14,
        y: -18,
        facing: 30,
    }, // 14
    TrackPoint {
        x: -21,
        y: -11,
        facing: 32,
    }, // 15
    TrackPoint {
        x: -31,
        y: -3,
        facing: 34,
    }, // 16
    TrackPoint {
        x: -40,
        y: 2,
        facing: 36,
    }, // 17
    TrackPoint {
        x: -46,
        y: 7,
        facing: 39,
    }, // 18
    TrackPoint {
        x: -53,
        y: 11,
        facing: 41,
    }, // 19
    TrackPoint {
        x: -59,
        y: 16,
        facing: 43,
    }, // 20
    TrackPoint {
        x: -66,
        y: 19,
        facing: 45,
    }, // 21
    TrackPoint {
        x: -73,
        y: 21,
        facing: 48,
    }, // 22
    TrackPoint {
        x: -66,
        y: 19,
        facing: 50,
    }, // 23
    TrackPoint {
        x: -59,
        y: 17,
        facing: 52,
    }, // 24
    TrackPoint {
        x: -52,
        y: 11,
        facing: 54,
    }, // 25
    TrackPoint {
        x: -44,
        y: 8,
        facing: 56,
    }, // 26
    TrackPoint {
        x: -33,
        y: 5,
        facing: 58,
    }, // 27
    TrackPoint {
        x: -21,
        y: 3,
        facing: 62,
    }, // 28
    TrackPoint {
        x: -11,
        y: 1,
        facing: 64,
    }, // 29
];

/// Track 10 points (short curve D). Extracted from 0x7E72D0.
/// 27 points. No cell crossing.
const TRACK10_POINTS: [TrackPoint; 27] = [
    TrackPoint {
        x: 11,
        y: -10,
        facing: 32,
    }, //  0
    TrackPoint {
        x: 21,
        y: -16,
        facing: 37,
    }, //  1
    TrackPoint {
        x: 32,
        y: -21,
        facing: 42,
    }, //  2
    TrackPoint {
        x: 43,
        y: -23,
        facing: 47,
    }, //  3
    TrackPoint {
        x: 50,
        y: -27,
        facing: 52,
    }, //  4
    TrackPoint {
        x: 56,
        y: -29,
        facing: 57,
    }, //  5
    TrackPoint {
        x: 64,
        y: -32,
        facing: 60,
    }, //  6
    TrackPoint {
        x: 56,
        y: -30,
        facing: 62,
    }, //  7
    TrackPoint {
        x: 50,
        y: -28,
        facing: 64,
    }, //  8
    TrackPoint {
        x: 42,
        y: -27,
        facing: 68,
    }, //  9
    TrackPoint {
        x: 30,
        y: -26,
        facing: 70,
    }, // 10
    TrackPoint {
        x: 21,
        y: -25,
        facing: 72,
    }, // 11
    TrackPoint {
        x: 11,
        y: -24,
        facing: 74,
    }, // 12
    TrackPoint {
        x: 0,
        y: -23,
        facing: 76,
    }, // 13
    TrackPoint {
        x: -11,
        y: -24,
        facing: 78,
    }, // 14
    TrackPoint {
        x: -21,
        y: -25,
        facing: 80,
    }, // 15
    TrackPoint {
        x: -32,
        y: -26,
        facing: 82,
    }, // 16
    TrackPoint {
        x: -43,
        y: -27,
        facing: 84,
    }, // 17
    TrackPoint {
        x: -50,
        y: -28,
        facing: 86,
    }, // 18
    TrackPoint {
        x: -59,
        y: -30,
        facing: 88,
    }, // 19
    TrackPoint {
        x: -64,
        y: -32,
        facing: 90,
    }, // 20
    TrackPoint {
        x: -59,
        y: -29,
        facing: 92,
    }, // 21
    TrackPoint {
        x: -50,
        y: -27,
        facing: 94,
    }, // 22
    TrackPoint {
        x: -43,
        y: -23,
        facing: 95,
    }, // 23
    TrackPoint {
        x: -32,
        y: -21,
        facing: 96,
    }, // 24
    TrackPoint {
        x: -21,
        y: -16,
        facing: 96,
    }, // 25
    TrackPoint {
        x: -11,
        y: -10,
        facing: 96,
    }, // 26
];

/// Track 11 points (special A — partial south approach). Extracted from 0x7E7420.
/// 13 points. No cell crossing.
const TRACK11_POINTS: [TrackPoint; 13] = [
    TrackPoint {
        x: 0,
        y: 256,
        facing: 160,
    }, //  0
    TrackPoint {
        x: 8,
        y: 243,
        facing: 160,
    }, //  1
    TrackPoint {
        x: 16,
        y: 229,
        facing: 160,
    }, //  2
    TrackPoint {
        x: 24,
        y: 214,
        facing: 160,
    }, //  3
    TrackPoint {
        x: 32,
        y: 200,
        facing: 160,
    }, //  4
    TrackPoint {
        x: 40,
        y: 185,
        facing: 160,
    }, //  5
    TrackPoint {
        x: 48,
        y: 171,
        facing: 160,
    }, //  6
    TrackPoint {
        x: 56,
        y: 156,
        facing: 160,
    }, //  7
    TrackPoint {
        x: 64,
        y: 141,
        facing: 160,
    }, //  8
    TrackPoint {
        x: 72,
        y: 127,
        facing: 160,
    }, //  9
    TrackPoint {
        x: 80,
        y: 113,
        facing: 160,
    }, // 10
    TrackPoint {
        x: 88,
        y: 100,
        facing: 160,
    }, // 11
    TrackPoint {
        x: 96,
        y: 85,
        facing: 160,
    }, // 12
];

/// Track 12 points (special B — partial south reverse). Extracted from 0x7E74C8.
/// 12 points. No cell crossing.
const TRACK12_POINTS: [TrackPoint; 12] = [
    TrackPoint {
        x: 96,
        y: -171,
        facing: 160,
    }, //  0
    TrackPoint {
        x: 88,
        y: -156,
        facing: 160,
    }, //  1
    TrackPoint {
        x: 80,
        y: -143,
        facing: 160,
    }, //  2
    TrackPoint {
        x: 72,
        y: -129,
        facing: 160,
    }, //  3
    TrackPoint {
        x: 64,
        y: -115,
        facing: 160,
    }, //  4
    TrackPoint {
        x: 56,
        y: -100,
        facing: 160,
    }, //  5
    TrackPoint {
        x: 48,
        y: -85,
        facing: 160,
    }, //  6
    TrackPoint {
        x: 40,
        y: -71,
        facing: 160,
    }, //  7
    TrackPoint {
        x: 32,
        y: -56,
        facing: 160,
    }, //  8
    TrackPoint {
        x: 24,
        y: -42,
        facing: 160,
    }, //  9
    TrackPoint {
        x: 16,
        y: -27,
        facing: 160,
    }, // 10
    TrackPoint {
        x: 8,
        y: -13,
        facing: 160,
    }, // 11
];

/// Track 13 points (special C — long straight east). Extracted from 0x7E7568.
/// 68 points. No cell crossing.
const TRACK13_POINTS: [TrackPoint; 68] = [
    TrackPoint {
        x: -670,
        y: -68,
        facing: 64,
    }, //  0
    TrackPoint {
        x: -660,
        y: -67,
        facing: 64,
    }, //  1
    TrackPoint {
        x: -650,
        y: -66,
        facing: 64,
    }, //  2
    TrackPoint {
        x: -639,
        y: -65,
        facing: 64,
    }, //  3
    TrackPoint {
        x: -630,
        y: -64,
        facing: 64,
    }, //  4
    TrackPoint {
        x: -620,
        y: -63,
        facing: 64,
    }, //  5
    TrackPoint {
        x: -610,
        y: -62,
        facing: 64,
    }, //  6
    TrackPoint {
        x: -600,
        y: -61,
        facing: 64,
    }, //  7
    TrackPoint {
        x: -590,
        y: -60,
        facing: 64,
    }, //  8
    TrackPoint {
        x: -580,
        y: -59,
        facing: 64,
    }, //  9
    TrackPoint {
        x: -570,
        y: -58,
        facing: 64,
    }, // 10
    TrackPoint {
        x: -560,
        y: -57,
        facing: 64,
    }, // 11
    TrackPoint {
        x: -550,
        y: -56,
        facing: 64,
    }, // 12
    TrackPoint {
        x: -540,
        y: -55,
        facing: 64,
    }, // 13
    TrackPoint {
        x: -530,
        y: -54,
        facing: 64,
    }, // 14
    TrackPoint {
        x: -520,
        y: -53,
        facing: 64,
    }, // 15
    TrackPoint {
        x: -510,
        y: -52,
        facing: 64,
    }, // 16
    TrackPoint {
        x: -500,
        y: -51,
        facing: 64,
    }, // 17
    TrackPoint {
        x: -490,
        y: -50,
        facing: 64,
    }, // 18
    TrackPoint {
        x: -480,
        y: -49,
        facing: 64,
    }, // 19
    TrackPoint {
        x: -470,
        y: -48,
        facing: 64,
    }, // 20
    TrackPoint {
        x: -460,
        y: -47,
        facing: 64,
    }, // 21
    TrackPoint {
        x: -450,
        y: -46,
        facing: 64,
    }, // 22
    TrackPoint {
        x: -440,
        y: -45,
        facing: 64,
    }, // 23
    TrackPoint {
        x: -430,
        y: -44,
        facing: 64,
    }, // 24
    TrackPoint {
        x: -420,
        y: -43,
        facing: 64,
    }, // 25
    TrackPoint {
        x: -410,
        y: -42,
        facing: 64,
    }, // 26
    TrackPoint {
        x: -400,
        y: -41,
        facing: 64,
    }, // 27
    TrackPoint {
        x: -390,
        y: -40,
        facing: 64,
    }, // 28
    TrackPoint {
        x: -380,
        y: -39,
        facing: 64,
    }, // 29
    TrackPoint {
        x: -370,
        y: -38,
        facing: 64,
    }, // 30
    TrackPoint {
        x: -360,
        y: -37,
        facing: 64,
    }, // 31
    TrackPoint {
        x: -350,
        y: -36,
        facing: 64,
    }, // 32
    TrackPoint {
        x: -340,
        y: -35,
        facing: 64,
    }, // 33
    TrackPoint {
        x: -330,
        y: -34,
        facing: 64,
    }, // 34
    TrackPoint {
        x: -320,
        y: -33,
        facing: 64,
    }, // 35
    TrackPoint {
        x: -310,
        y: -32,
        facing: 64,
    }, // 36
    TrackPoint {
        x: -300,
        y: -31,
        facing: 64,
    }, // 37
    TrackPoint {
        x: -290,
        y: -30,
        facing: 64,
    }, // 38
    TrackPoint {
        x: -280,
        y: -29,
        facing: 64,
    }, // 39
    TrackPoint {
        x: -270,
        y: -28,
        facing: 64,
    }, // 40
    TrackPoint {
        x: -260,
        y: -27,
        facing: 64,
    }, // 41
    TrackPoint {
        x: -250,
        y: -26,
        facing: 64,
    }, // 42
    TrackPoint {
        x: -240,
        y: -25,
        facing: 64,
    }, // 43
    TrackPoint {
        x: -230,
        y: -24,
        facing: 64,
    }, // 44
    TrackPoint {
        x: -220,
        y: -23,
        facing: 64,
    }, // 45
    TrackPoint {
        x: -210,
        y: -22,
        facing: 64,
    }, // 46
    TrackPoint {
        x: -200,
        y: -21,
        facing: 64,
    }, // 47
    TrackPoint {
        x: -190,
        y: -20,
        facing: 64,
    }, // 48
    TrackPoint {
        x: -180,
        y: -19,
        facing: 64,
    }, // 49
    TrackPoint {
        x: -170,
        y: -18,
        facing: 64,
    }, // 50
    TrackPoint {
        x: -160,
        y: -17,
        facing: 64,
    }, // 51
    TrackPoint {
        x: -150,
        y: -16,
        facing: 64,
    }, // 52
    TrackPoint {
        x: -140,
        y: -15,
        facing: 64,
    }, // 53
    TrackPoint {
        x: -130,
        y: -14,
        facing: 64,
    }, // 54
    TrackPoint {
        x: -120,
        y: -13,
        facing: 64,
    }, // 55
    TrackPoint {
        x: -110,
        y: -12,
        facing: 64,
    }, // 56
    TrackPoint {
        x: -100,
        y: -11,
        facing: 64,
    }, // 57
    TrackPoint {
        x: -90,
        y: -10,
        facing: 64,
    }, // 58
    TrackPoint {
        x: -80,
        y: -9,
        facing: 64,
    }, // 59
    TrackPoint {
        x: -70,
        y: -8,
        facing: 64,
    }, // 60
    TrackPoint {
        x: -60,
        y: -7,
        facing: 64,
    }, // 61
    TrackPoint {
        x: -50,
        y: -6,
        facing: 64,
    }, // 62
    TrackPoint {
        x: -40,
        y: -5,
        facing: 64,
    }, // 63
    TrackPoint {
        x: -30,
        y: -4,
        facing: 64,
    }, // 64
    TrackPoint {
        x: -20,
        y: -3,
        facing: 64,
    }, // 65
    TrackPoint {
        x: -10,
        y: -2,
        facing: 64,
    }, // 66
    TrackPoint {
        x: 0,
        y: -1,
        facing: 64,
    }, // 67
];

/// Track 14 points (special D — short NE diagonal). Extracted from 0x7E78A8.
/// 15 points. No cell crossing.
const TRACK14_POINTS: [TrackPoint; 15] = [
    TrackPoint {
        x: -120,
        y: 120,
        facing: 32,
    }, //  0
    TrackPoint {
        x: -112,
        y: 112,
        facing: 32,
    }, //  1
    TrackPoint {
        x: -104,
        y: 104,
        facing: 32,
    }, //  2
    TrackPoint {
        x: -96,
        y: 96,
        facing: 32,
    }, //  3
    TrackPoint {
        x: -88,
        y: 88,
        facing: 32,
    }, //  4
    TrackPoint {
        x: -80,
        y: 80,
        facing: 32,
    }, //  5
    TrackPoint {
        x: -72,
        y: 72,
        facing: 32,
    }, //  6
    TrackPoint {
        x: -64,
        y: 64,
        facing: 32,
    }, //  7
    TrackPoint {
        x: -56,
        y: 56,
        facing: 32,
    }, //  8
    TrackPoint {
        x: -48,
        y: 48,
        facing: 32,
    }, //  9
    TrackPoint {
        x: -40,
        y: 40,
        facing: 32,
    }, // 10
    TrackPoint {
        x: -32,
        y: 32,
        facing: 32,
    }, // 11
    TrackPoint {
        x: -24,
        y: 24,
        facing: 32,
    }, // 12
    TrackPoint {
        x: -16,
        y: 16,
        facing: 32,
    }, // 13
    TrackPoint {
        x: -8,
        y: 8,
        facing: 32,
    }, // 14
];

/// Track 15 points (special E — SE to S arc). Extracted from 0x7E7968.
/// 16 points. No cell crossing.
const TRACK15_POINTS: [TrackPoint; 16] = [
    TrackPoint {
        x: 128,
        y: -128,
        facing: 128,
    }, //  0
    TrackPoint {
        x: 124,
        y: -112,
        facing: 132,
    }, //  1
    TrackPoint {
        x: 119,
        y: -96,
        facing: 136,
    }, //  2
    TrackPoint {
        x: 115,
        y: -80,
        facing: 140,
    }, //  3
    TrackPoint {
        x: 111,
        y: -64,
        facing: 144,
    }, //  4
    TrackPoint {
        x: 106,
        y: -57,
        facing: 148,
    }, //  5
    TrackPoint {
        x: 101,
        y: -50,
        facing: 152,
    }, //  6
    TrackPoint {
        x: 96,
        y: -44,
        facing: 156,
    }, //  7
    TrackPoint {
        x: 91,
        y: -37,
        facing: 160,
    }, //  8
    TrackPoint {
        x: 84,
        y: -32,
        facing: 164,
    }, //  9
    TrackPoint {
        x: 77,
        y: -27,
        facing: 168,
    }, // 10
    TrackPoint {
        x: 70,
        y: -22,
        facing: 172,
    }, // 11
    TrackPoint {
        x: 64,
        y: -17,
        facing: 176,
    }, // 12
    TrackPoint {
        x: 48,
        y: -12,
        facing: 180,
    }, // 13
    TrackPoint {
        x: 32,
        y: -8,
        facing: 184,
    }, // 14
    TrackPoint {
        x: 16,
        y: -4,
        facing: 188,
    }, // 15
];

// ---------------------------------------------------------------------------
// Track lookup functions
// ---------------------------------------------------------------------------

/// Get the point array for a RawTrack by index.
///
/// Returns an empty slice for track 0 (null track) or if the track data
/// hasn't been extracted yet.
pub fn raw_track_points(track_index: u8) -> &'static [TrackPoint] {
    match track_index {
        1 => &TRACK1_POINTS,
        2 => &TRACK2_POINTS,
        3 => &TRACK3_POINTS,
        4 => &TRACK4_POINTS,
        5 => &TRACK5_POINTS,
        6 => &TRACK6_POINTS,
        7 => &TRACK7_POINTS,
        8 => &TRACK8_POINTS,
        9 => &TRACK9_POINTS,
        10 => &TRACK10_POINTS,
        11 => &TRACK11_POINTS,
        12 => &TRACK12_POINTS,
        13 => &TRACK13_POINTS,
        14 => &TRACK14_POINTS,
        15 => &TRACK15_POINTS,
        _ => &[],
    }
}

/// Look up the TurnTrack for a given TurnTrack index (0-71).
///
/// Returns None if the index is out of range.
pub fn turn_track_at(index: usize) -> Option<&'static TurnTrack> {
    TURN_TRACKS.get(index)
}

/// ILocomotion slot `+0xA4` for Drive and Ship — `Can_Use_Track`.
///
/// Native: `DriveLocomotionClass 0x004B4B00` and `ShipLocomotionClass
/// 0x006A4130` are the same routine over their own tables (Drive turn table
/// `0x007E7B28` / raw `0x007E7A28`, Ship `0x007F2A40` / `0x007F2960`); every
/// other locomotor inherits the base slot at `0x004B6640`, which is
/// `XOR AL,AL / RET 4`. Read from the disassembly on 2026-09-15; `this` is the
/// ILocomotion interface, so `[this+0x54]` is the retained turn selector,
/// `[this+0x58]` the cursor and `[this+0x5C]` the short-track byte.
///
/// The answer is true only when the owner's path queue head `d`
/// (`Foot+0x5E0`) is a real octant, `d` differs from the exit octant of the
/// current turn track (`((target_facing << 8) >> 12 + 1 >> 1) & 7`), the
/// cursor sits exactly on the current raw track's chain point (`RawTrack+0x04`,
/// nonzero), and the turn track `d + exit * 8` names a raw track whose entry
/// index (`RawTrack+0x08`) is nonzero. `UnitClass::Can_Enter_Cell 0x0073FA46`
/// asks it about an allied occupant that is in transit (`Foot+0x6B6 == 0`) or
/// an infantryman: false skips the occupant, true raises the running code to 2.
pub(crate) fn occupant_can_use_track(
    track: &crate::sim::components::TrackProgress,
    path_head: Option<u8>,
) -> bool {
    // `CMP EAX,-1 / JL` and `CMP EAX,8 / JG`, then the explicit 8 and -1 exits.
    let Some(head) = path_head else {
        return false;
    };
    if head >= 8 {
        return false;
    }
    let Some(turn) = usize::try_from(track.turn_index)
        .ok()
        .and_then(turn_track_at)
    else {
        return false;
    };
    let raw = if track.reversed {
        turn.short_track
    } else {
        turn.normal_track
    };
    let exit_octant = ((u32::from(turn.target_facing) << 8) >> 12).wrapping_add(1) >> 1 & 7;
    if exit_octant == u32::from(head) {
        return false;
    }
    let Some(raw_meta) = RAW_TRACKS.get(usize::from(raw)) else {
        return false;
    };
    if i32::from(raw_meta.chain_index) != track.cursor || track.cursor == 0 {
        return false;
    }
    let chained_index = usize::from(head) + exit_octant as usize * FACING_DIRECTIONS;
    let Some(chained) = turn_track_at(chained_index) else {
        return false;
    };
    if chained.normal_track == 0 {
        return false;
    }
    RAW_TRACKS
        .get(usize::from(chained.normal_track))
        .is_some_and(|meta| meta.entry_index != 0)
}

/// What an allied occupant's locomotor answers on slot `+0xA4` when asked by
/// `UnitClass::Can_Enter_Cell` (`0x0073FA46`).
///
/// Drive and Ship answer [`occupant_can_use_track`] over their retained track
/// progress and the owner's path queue head; Walk, Hover and every other class
/// inherit the base slot `0x004B6640`, which always answers false.
pub(crate) fn occupant_slot_a4_answers_true(
    occupant: &crate::sim::game_entity::GameEntity,
) -> bool {
    use crate::rules::locomotor_type::LocomotorKind;
    let kind = occupant.locomotor.as_ref().map(|locomotor| locomotor.kind);
    let track = match kind {
        Some(LocomotorKind::Drive) => occupant.drive_locomotion.as_ref().map(|drive| &drive.track),
        Some(LocomotorKind::Ship) => occupant.ship_locomotion.as_ref().map(|ship| &ship.track),
        _ => None,
    };
    let Some(track) = track else {
        return false;
    };
    let path_head = occupant
        .navigation
        .path_replay
        .remaining_directions()
        .first()
        .copied();
    occupant_can_use_track(track, path_head)
}

/// Select the appropriate RawTrack index from a TurnTrack.
///
/// Uses the short track variant for fast vehicles.
pub fn select_raw_track_index(turn: &TurnTrack, use_short: bool) -> u8 {
    if use_short {
        turn.short_track
    } else {
        turn.normal_track
    }
}

/// Get the RawTrack metadata by index.
pub fn raw_track_meta(index: u8) -> Option<&'static RawTrack> {
    RAW_TRACKS.get(index as usize)
}

/// Cells tested by retail DriveLocomotion slot 40 (`Is_At_Coord`).
///
/// The ordinary head cell is always the fallback candidate.  Before the
/// track's occupation handoff point is consumed, retail first transforms that
/// raw point around the stored head and tests its cell instead.  Rust rebases
/// `current_cell` at curve crossings, so `cell_offset_*` is required to recover
/// the same absolute head that native stores directly.
pub(super) fn is_at_coord_track_cells(
    state: &DriveTrackState,
    current_cell: (u16, u16),
    include_handoff: bool,
) -> (Option<(i16, i16)>, (i16, i16)) {
    let head_abs_x = i32::from(current_cell.0) * 256 + state.head_offset_x + state.cell_offset_x;
    let head_abs_y = i32::from(current_cell.1) * 256 + state.head_offset_y + state.cell_offset_y;
    let head_cell = (
        crate::util::direction_tables::lepton_to_cell(head_abs_x) as i16,
        crate::util::direction_tables::lepton_to_cell(head_abs_y) as i16,
    );

    let Some(meta) = RAW_TRACKS.get(state.raw_track_index as usize) else {
        return (None, head_cell);
    };
    let handoff_index = meta.occupation_handoff_point_index;
    // Against the occupied count, not the occupied index: native compares its
    // stored cursor here (`0x004B49B6`), which is one ahead of the point being
    // stood on. See `DriveTrackState::occupied_points`.
    if !include_handoff || handoff_index < 0 || state.occupied_points() >= i32::from(handoff_index)
    {
        return (None, head_cell);
    }
    let Some(point) = raw_track_points(state.raw_track_index).get(handoff_index as usize) else {
        return (None, head_cell);
    };
    let (tx, ty, _) = transform_track_point(point.x, point.y, point.facing, state.transform_flags);
    let handoff_cell = (
        ((head_abs_x + i32::from(tx)) / 256) as i16,
        ((head_abs_y + i32::from(ty)) / 256) as i16,
    );
    (Some(handoff_cell), head_cell)
}

// ---------------------------------------------------------------------------
// Track selection — facing delta to TurnTrack
// ---------------------------------------------------------------------------

/// Number of discrete direction indices in RA2's facing system (8 compass points).
const FACING_DIRECTIONS: usize = 8;

/// Select the TurnTrack index for a vehicle moving from `current_facing` toward
/// a neighbor cell that requires `next_facing`.
///
/// Both facings are 0-255 (RA2 convention: 0=N, 32=NE, 64=E, ...).
/// The TurnTrack table is indexed as `from_dir * 8 + to_dir` where each
/// direction is the facing quantized to 8 compass points (0-7).
///
/// Returns `None` if the selected TurnTrack has a null raw track (track 0),
/// meaning the turn is too sharp for a smooth curve and requires stop-rotate-go.
/// Also returns `None` if track point data hasn't been extracted yet.
pub fn select_drive_track(
    current_facing: u8,
    next_facing: u8,
    use_short: bool,
) -> Option<DriveTrackSelection> {
    // Quantize facings to 8-direction indices (0-7).
    // Add half a direction for proper rounding before integer divide.
    let from_dir = facing_to_dir(current_facing);
    let to_dir = facing_to_dir(next_facing);
    let turn_index = from_dir * FACING_DIRECTIONS + to_dir;
    if turn_index >= TURN_TRACKS.len() {
        return None;
    }

    let turn_track = &TURN_TRACKS[turn_index];
    let raw_index = if use_short {
        turn_track.short_track
    } else {
        turn_track.normal_track
    };

    // Track 0 is the null track — this turn is too sharp for a smooth curve.
    if raw_index == 0 {
        return None;
    }

    let raw_meta = RAW_TRACKS.get(raw_index as usize)?;

    // Check if track point data is actually available.
    let points = raw_track_points(raw_index);
    if points.is_empty() {
        return None;
    }

    Some(DriveTrackSelection {
        turn_track_index: turn_index,
        raw_track_index: raw_index,
        entry_index: raw_meta.entry_index,
        chain_index: raw_meta.chain_index,
        occupation_handoff_point_index: raw_meta.occupation_handoff_point_index,
        points_count: raw_meta.points_count,
        target_facing: turn_track.target_facing,
        flags: turn_track.flags,
    })
}

/// Synthesize the substitute selection used when pathfinding produces a turn
/// too sharp for any precomputed curve (`select_drive_track` returned `None`).
/// Returns the `cur_dir * 9` TurnTrack entry — RawTrack 1 (cardinals) or 2
/// (diagonals) transformed for the unit's current facing. The unit drives
/// forward in `current_facing` for one cell; the caller is responsible for
/// consuming the impossible path step.
///
/// Returns `None` defensively if RawTrack 1 or 2 point data isn't loaded
/// (never expected in normal operation — these are foundational tracks).
pub fn build_sharp_turn_fallback(current_facing: u8) -> Option<DriveTrackSelection> {
    let cur_dir = facing_to_dir(current_facing);
    let turn_index = cur_dir * FACING_DIRECTIONS + cur_dir; // cur_dir * 9
    let turn_track = TURN_TRACKS.get(turn_index)?;
    if turn_track.normal_track == 0 {
        return None; // structurally impossible for cur_dir*9 entries; defensive
    }
    let raw_meta = RAW_TRACKS.get(turn_track.normal_track as usize)?;
    let points = raw_track_points(turn_track.normal_track);
    if points.is_empty() {
        return None;
    }
    Some(DriveTrackSelection {
        turn_track_index: turn_index,
        raw_track_index: turn_track.normal_track,
        entry_index: raw_meta.entry_index,
        chain_index: raw_meta.chain_index,
        occupation_handoff_point_index: raw_meta.occupation_handoff_point_index,
        points_count: raw_meta.points_count,
        target_facing: turn_track.target_facing,
        flags: turn_track.flags,
    })
}

// ---------------------------------------------------------------------------
// Path-window track selection — the retail basis
// ---------------------------------------------------------------------------

/// `TurnTrack.flags` bit 3. Set on exactly the 32 of 72 table entries whose
/// `to` direction differs from their `from` direction; gamemd tests it to pick
/// the two-node queue shift with a two-cell reserved head over the one-node
/// shift with a one-cell head.
pub const TURN_TRACK_TURNS_FLAG: u8 = 0x08;

/// Facing units per direction octant (256 / 8).
const OCTANT_FACING_STEP: u8 = 0x20;

/// Highest RawTrack index in the set ShipLocomotion shares with Drive.
const SHIP_MAX_SHARED_RAW_TRACK: u8 = 13;

/// Cell delta of each direction octant in the sim cell grid
/// (+X = east, +Y = south), indexed N=0, NE=1, E=2, SE=3, S=4, SW=5, W=6, NW=7.
/// Mirrors `crate::util::fixed_math::dir_to_cell_delta` without the facing-byte
/// round trip, so a path step maps to an octant exactly.
const OCTANT_CELL_DELTA: [(i32, i32); FACING_DIRECTIONS] = [
    (0, -1),
    (1, -1),
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
];

/// Direction octant of a one-cell path step. `None` for a null step.
fn octant_from_cell_delta(dx: i32, dy: i32) -> Option<usize> {
    let step = (dx.signum(), dy.signum());
    OCTANT_CELL_DELTA.iter().position(|&delta| delta == step)
}

/// A curve chosen from the path window, with the head cell it reserves.
#[derive(Debug, Clone, Copy)]
pub struct DriveTrackPlan {
    /// The chosen TurnTrack/RawTrack pair.
    pub selection: DriveTrackSelection,
    /// Cell delta from the mover's current cell to the reserved head cell.
    /// One cell for a non-turning curve, two for a `TURN_TRACK_TURNS_FLAG` curve.
    pub head_dx: i32,
    pub head_dy: i32,
    /// Path nodes this curve spans: 1 for the one-node shift, 2 for the
    /// two-node shift that a turning curve takes.
    pub nodes: usize,
}

impl DriveTrackPlan {
    /// True when the curve spans two path nodes.
    pub fn spans_two_nodes(&self) -> bool {
        self.nodes == 2
    }
}

/// What the Drive/Ship selection step decided for this frame.
#[derive(Debug, Clone, Copy)]
pub enum DriveTrackDecision {
    /// The body is not yet on the octant of the head path node. gamemd commands
    /// the turn and returns without touching the path queue or taking a step;
    /// the caller must install no curve and consume no node this frame.
    TurnFirst {
        /// Exact octant facing the body must reach before selection may run.
        desired_facing: u8,
    },
    /// Install this curve.
    Select(DriveTrackPlan),
    /// No curve is available (degenerate step, or track point data missing).
    Unavailable,
}

/// Choose the drive curve for a mover standing on its current cell with the
/// path window `current -> current+from_delta -> +to_delta`.
///
/// gamemd indexes the turn table by the two leading *path* directions —
/// `turn_index = path[1]_dir + path[0]_dir * 8` — never by the mover's body
/// facing. A null curve falls back to `path[0]_dir * 9`, the straight run in
/// the head node's own direction. The body facing enters only as the exact
/// precondition below: an in-place turn is commanded whenever it differs from
/// the head node's octant, and no curve is selected until it matches.
///
/// `to_delta` is `None` at the last step of a path, which gamemd normalises to
/// `to := from` (its queue terminator).
pub fn plan_drive_track_from_path(
    body_facing: u8,
    from_delta: (i32, i32),
    to_delta: Option<(i32, i32)>,
    is_ship: bool,
) -> DriveTrackDecision {
    let Some(from_dir) = octant_from_cell_delta(from_delta.0, from_delta.1) else {
        return DriveTrackDecision::Unavailable;
    };

    // Exact-facing precondition. Zero tolerance: gamemd compares the 16-bit
    // facing against `path[0]_dir << 13`, which in the 8-bit frame is
    // `path[0]_dir * 0x20`, and any non-zero delta commands the turn instead.
    let desired_facing = (from_dir as u8).wrapping_mul(OCTANT_FACING_STEP);
    if body_facing != desired_facing {
        return DriveTrackDecision::TurnFirst { desired_facing };
    }

    let to_dir = to_delta
        .and_then(|(dx, dy)| octant_from_cell_delta(dx, dy))
        .unwrap_or(from_dir);

    // `from * 9` is the straight-ahead entry on the table diagonal; it is the
    // null-curve substitute and is never itself null.
    let straight_index = from_dir * FACING_DIRECTIONS + from_dir;
    let mut turn_index = from_dir * FACING_DIRECTIONS + to_dir;
    if TURN_TRACKS[turn_index].normal_track == 0 {
        turn_index = straight_index;
    }
    // ShipLocomotion consumes only the RawTrack set it shares with Drive.
    if is_ship && TURN_TRACKS[turn_index].normal_track > SHIP_MAX_SHARED_RAW_TRACK {
        turn_index = straight_index;
    }

    let turn = &TURN_TRACKS[turn_index];
    let raw_index = turn.normal_track;
    if raw_index == 0 {
        return DriveTrackDecision::Unavailable;
    }
    let Some(raw_meta) = RAW_TRACKS.get(raw_index as usize) else {
        return DriveTrackDecision::Unavailable;
    };
    if raw_track_points(raw_index).is_empty() {
        return DriveTrackDecision::Unavailable;
    }

    let two_node = turn.flags & TURN_TRACK_TURNS_FLAG != 0;
    let (from_dx, from_dy) = OCTANT_CELL_DELTA[from_dir];
    let (head_dx, head_dy) = if two_node {
        let (to_dx, to_dy) = OCTANT_CELL_DELTA[to_dir];
        (from_dx + to_dx, from_dy + to_dy)
    } else {
        (from_dx, from_dy)
    };

    DriveTrackDecision::Select(DriveTrackPlan {
        selection: DriveTrackSelection {
            turn_track_index: turn_index,
            raw_track_index: raw_index,
            entry_index: raw_meta.entry_index,
            chain_index: raw_meta.chain_index,
            occupation_handoff_point_index: raw_meta.occupation_handoff_point_index,
            points_count: raw_meta.points_count,
            target_facing: turn.target_facing,
            flags: turn.flags,
        },
        head_dx,
        head_dy,
        nodes: if two_node { 2 } else { 1 },
    })
}

/// Install a curve chosen by `plan_drive_track_from_path`.
///
/// The cursor starts at point 0, not at `entry_index`: gamemd's selection
/// finalize writes 0 to the track cursor, and the lead-in points are exactly
/// what carry the mover from its current cell centre into the arc. The
/// non-zero `entry_index` is the *chain* entry point, used when a curve is
/// adopted mid-motion.
pub fn begin_selected_drive_track(plan: &DriveTrackPlan) -> Option<DriveTrackState> {
    let mut state = begin_drive_track(
        plan.selection.raw_track_index,
        plan.selection.flags,
        plan.head_dx,
        plan.head_dy,
        plan.selection.target_facing,
    )?;
    state.point_index = 0;
    state.before_first_point = true;
    Some(state)
}

impl DriveTrackState {
    /// Points occupied so far - native's stored cursor, `[EBP+0x5C]`.
    ///
    /// Native increments at the loop tail (`0x004B1F4F`), so its cursor counts
    /// points already occupied while the object stands on `points[cursor - 1]`.
    /// `point_index` is that `cursor - 1`. Anything comparing against native's
    /// cursor - the occupation handoff gate at `0x004B49B6`, and the owner-side
    /// `TrackProgress::cursor` mirror, which documents itself as
    /// next-to-consume - must read this rather than `point_index`.
    pub fn occupied_points(&self) -> i32 {
        if self.before_first_point {
            0
        } else {
            i32::from(self.point_index) + 1
        }
    }
}

/// Quantize a 0-255 facing to a direction index 0-7 (N, NE, E, SE, S, SW, W, NW).
fn facing_to_dir(facing: u8) -> usize {
    // Add half a direction (16) for rounding, then divide by 32.
    // Wrapping add handles the 255+16 → 15 case correctly.
    crate::util::direction::direction_from_facing(facing) as usize
}

/// Result of selecting a drive track for a facing change.
#[derive(Debug, Clone, Copy)]
pub struct DriveTrackSelection {
    /// Index into TURN_TRACKS (0-63 for normal, 64-71 for special).
    pub turn_track_index: usize,
    /// Index into RAW_TRACKS (1-15, 0 = null is filtered out).
    pub raw_track_index: u8,
    /// First active point index (points before this are lead-in).
    pub entry_index: u16,
    /// Point index where track chaining is attempted (-1 = none).
    pub chain_index: i16,
    /// RawTrack +0x0C occupation handoff point (-1 = none).
    /// Runtime wiring remains intentionally separate from coordinate crossing.
    pub occupation_handoff_point_index: i16,
    /// Total points in the track.
    pub points_count: u16,
    /// Target facing after completing the track (0-255).
    pub target_facing: u8,
    /// Direction flags from the TurnTrack entry.
    pub flags: u8,
}

// ---------------------------------------------------------------------------
// Track advancement — stepping through points each tick
// ---------------------------------------------------------------------------

/// Result of advancing one tick through a drive track.
#[derive(Debug, Clone, Copy)]
pub struct DriveTrackAdvance {
    /// Sub-cell X position (transformed track point + head_offset + cell_offset).
    pub sub_x: SimFixed,
    /// Sub-cell Y position (transformed track point + head_offset + cell_offset).
    pub sub_y: SimFixed,
    /// Body facing at the current track point (transformed).
    pub facing: u8,
    /// True if the vehicle's position crossed into a different cell this tick.
    /// Detected by coordinate-based boundary checking — every step checks
    /// if the world position lands in a new cell.
    pub cell_jump: bool,
    /// Cell delta the coordinate crossing actually applied, in cells
    /// (+X = east, +Y = south). `(0, 0)` whenever `cell_jump` is false.
    ///
    /// The caller must move the mover's cell by exactly this delta: it is the
    /// same value that shifted `cell_offset_*`, so `cell * 256 + sub` stays
    /// continuous across the crossing. A curve's crossings do not always match
    /// its path steps one for one — the straight diagonals whose transform puts
    /// a point exactly on a cell corner cross one axis, then the other, on
    /// consecutive points for a single path node.
    pub cell_jump_dx: i32,
    /// See `cell_jump_dx`.
    pub cell_jump_dy: i32,
    /// True if the track reached the chain_index point. The caller should
    /// attempt to chain into the next track curve (check Can_Enter_Cell on
    /// the next-next cell, select new track if passable).
    pub chain_ready: bool,
    /// True if the track has been fully traversed.
    pub finished: bool,
    /// Lepton-space delta from the just-consumed point to the next-to-consume
    /// point, transformed by the active flags. Zero when no next step exists
    /// (track end / sentinel hit). Used by sub-step interp to scale fractional
    /// progress from the residual budget.
    pub next_step_delta_x: i32,
    /// See `next_step_delta_x`.
    pub next_step_delta_y: i32,
    /// True iff a valid next step exists (not at last_index, not a sentinel).
    /// Sub-step interp is only applied when this is true.
    pub had_next_step: bool,
}

/// Begin following a drive track. Creates a new DriveTrackState starting at
/// the track's entry_index.
///
/// `transform_flags`: lower 3 bits of TurnTrack.flags — controls mirror/flip
/// of the base track curve for different turn orientations.
///
/// `head_dx`, `head_dy`: cell delta from the vehicle's current cell to the
/// destination cell (head_to). Track point coordinates are offsets from the
/// destination cell center, so these deltas map them to sub-cell space
/// relative to the current cell.
///
/// `target_facing`: post-turn facing of the TurnTrack entry that produced
/// this track. Stored on the state so the chain caller can read the
/// post-turn "from-dir" for chain-target track selection.
pub fn begin_drive_track(
    raw_track_index: u8,
    transform_flags: u8,
    head_dx: i32,
    head_dy: i32,
    target_facing: u8,
) -> Option<DriveTrackState> {
    begin_drive_track_with_head_offset(
        raw_track_index,
        transform_flags,
        head_dx * 256 + 128,
        head_dy * 256 + 128,
        target_facing,
    )
}

/// Begin following a drive track with an explicit head-to lepton reference.
///
/// This is the lower-level form used by DriveLocomotion `Force_Track`, whose
/// target coordinate can be sub-cell aligned rather than a neighboring cell
/// center.
pub fn begin_drive_track_with_head_offset(
    raw_track_index: u8,
    transform_flags: u8,
    head_offset_x: i32,
    head_offset_y: i32,
    target_facing: u8,
) -> Option<DriveTrackState> {
    let meta = RAW_TRACKS.get(raw_track_index as usize)?;
    let points = raw_track_points(raw_track_index);
    if points.is_empty() {
        return None;
    }
    Some(DriveTrackState {
        raw_track_index,
        point_index: meta.entry_index,
        // Chained adoption overwrites both of these at the install site; a
        // fresh curve goes through `track_head::begin_fresh`, which sets the
        // flag. Defaulting to clear keeps the chain path's meaning.
        before_first_point: false,
        residual: 0,
        transform_flags: transform_flags & 0x07, // only lower 3 bits
        head_offset_x,
        head_offset_y,
        cell_offset_x: 0,
        cell_offset_y: 0,
        target_facing,
    })
}

/// Begin a one-shot forced TurnTrack by direct index.
///
/// Special TurnTracks such as the Tank Bunker eject `0x47` are not selected from a
/// facing-pair formula; gamemd writes the table index directly through
/// DriveLocomotion::Force_Track.
pub fn begin_forced_turn_track(
    turn_track_index: u8,
    head_offset_x: i32,
    head_offset_y: i32,
    speed: SimFixed,
    use_short: bool,
) -> Option<ForcedDriveTrackState> {
    let turn = turn_track_at(turn_track_index as usize)?;
    let raw_track_index = select_raw_track_index(turn, use_short);
    let mut track = begin_drive_track_with_head_offset(
        raw_track_index,
        turn.flags,
        head_offset_x,
        head_offset_y,
        turn.target_facing,
    )?;
    // `Force_Track` zeroes the cursor outright - it does **not** start at the
    // RawTrack entry point: `0x004B0C53 MOV [EBP+0x54],EAX` stores the selector
    // and `0x004B0C56 MOV dword ptr [EBP+0x58],0x0` the cursor, with `EBP` the
    // class biased by 4 (`0x004B0C88 LEA ESI,[EBP-0x4]`), so `+0x54`/`+0x58` are
    // the selector at `+0x58` and the cursor at `+0x5C`.
    //
    // So a forced curve occupies `points[0]` on its first paid step, exactly as a
    // fresh one does, and needs the same pre-start state. Without this it takes
    // the terminal credit without the cursor fix and silently skips `points[0]`.
    track.point_index = 0;
    track.before_first_point = true;
    Some(ForcedDriveTrackState {
        turn_track_index,
        track,
        speed,
    })
}

/// Advance a drive track state by one tick.
///
/// Uses the original engine's movement budget system:
/// each tick gets a budget of `speed * dt` leptons plus any residual
/// from the previous tick. Each track point costs TRACK_STEP_COST (7).
/// Leftover budget is saved in `state.residual` for the next tick
/// so fractional progress is not lost between ticks.
///
/// **Coordinate-based cell detection**: after each step,
/// the transformed track point is mapped to sub-cell coordinates. If the
/// coordinates land in a different cell (sub_x outside [0,256) or sub_y
/// outside [0,256)), a cell_jump is signaled with the applied `(dx, dy)` and
/// the loop breaks so the caller can handle the transition. The original engine
/// keeps one absolute coordinate per object and derives the cell from it by a
/// sign-corrected shift, so the cell and the coordinate always move together;
/// the caller must use the reported delta rather than its own path cursor, or
/// the two disagree and the rendered position snaps.
///
/// **Track chaining** at chain_index (binary +0x04): when the stepping loop
/// reaches the chain_index point, chain_ready is set and the loop breaks
/// so the caller can attempt to select a follow-on track curve.
///
/// Returns the current position/facing after advancement, plus flags for
/// cell transitions, chain readiness, and track completion.
pub fn advance_drive_track(
    state: &mut DriveTrackState,
    speed: SimFixed,
    dt: SimFixed,
) -> DriveTrackAdvance {
    let mut residual = state.residual;
    let fresh_budget: i32 = (speed * dt).to_num::<i32>();
    advance_drive_track_with_budget(state, fresh_budget, &mut residual)
}

/// Advance a drive track using a caller-owned residual budget.
///
/// Normal `DriveLocomotion` stores residual at the locomotor object, not inside
/// the track table cursor. `DriveTrackState::residual` remains a synchronized
/// mirror so interpolation and forced-track callers can share the same code.
pub fn advance_drive_track_with_budget(
    state: &mut DriveTrackState,
    fresh_budget: i32,
    residual_budget: &mut i32,
) -> DriveTrackAdvance {
    advance_drive_track_with_budget_mode(state, fresh_budget, residual_budget, true)
}

/// Advance a forced track without interpreting its head-relative coordinates
/// as ordinary per-cell crossing coordinates. `Force_Track` transforms every
/// raw point from the stored absolute head and only relinks the object on the
/// paid terminal sentinel.
pub(crate) fn advance_forced_drive_track(
    state: &mut ForcedDriveTrackState,
    dt: SimFixed,
    residual_budget: &mut i32,
) -> DriveTrackAdvance {
    let fresh_budget: i32 = (state.speed * dt).to_num::<i32>();
    advance_drive_track_with_budget_mode(&mut state.track, fresh_budget, residual_budget, false)
}

fn advance_drive_track_with_budget_mode(
    state: &mut DriveTrackState,
    fresh_budget: i32,
    residual_budget: &mut i32,
    detect_cell_jump: bool,
) -> DriveTrackAdvance {
    let meta = &RAW_TRACKS[state.raw_track_index as usize];
    let points = raw_track_points(state.raw_track_index);
    let last_index = meta.points_count.saturating_sub(1);
    let mut cell_jump = false;
    let mut cell_jump_dx = 0;
    let mut cell_jump_dy = 0;
    let mut chain_ready = false;

    // Budget = this tick's speed + leftover from last tick.
    state.residual = *residual_budget;
    let mut budget: i32 = fresh_budget + *residual_budget;

    // Consume track points at TRACK_STEP_COST each.
    //
    // Native pays for the point it is about to read, reads `points[cursor]`
    // (`0x004B1596`, after `0x004B159D SUB EDI,0x7`), and increments only at the
    // loop tail (`0x004B1F4F`), so the first paid step of a fresh curve occupies
    // `points[0]`. The curve ends when that read lands on the stored `(0, 0)`
    // sentinel one slot past the real points (`0x004B15C0..0x004B15C8` tests x
    // and y both zero at a non-zero cursor); the sentinel read costs its 7 like
    // any other step and then credits part of it back. VERA's arrays stop before
    // the sentinel, so "read the sentinel" is "the next index is past the end".
    // A track with no points has no end to reach and must not be charged for
    // looking: the index guard this loop replaced never ran for one at all.
    let mut finished = points.is_empty();
    while budget > TRACK_STEP_COST && !points.is_empty() {
        budget -= TRACK_STEP_COST;
        let next = if state.before_first_point {
            0
        } else {
            state.point_index.saturating_add(1)
        };
        if next > last_index || usize::from(next) >= points.len() {
            budget += terminal_budget_credit(points, state.point_index, state.transform_flags);
            finished = true;
            break;
        }
        state.before_first_point = false;
        state.point_index = next;

        // Coordinate-based cell detection: transform the track point and
        // check if the resulting sub-cell position is outside [0, 256).
        // Every step checks whether the world position lands in a different cell.
        if detect_cell_jump && let Some(pt) = points.get(state.point_index as usize) {
            let (tx, ty, _) = transform_track_point(pt.x, pt.y, pt.facing, state.transform_flags);
            let sx = state.head_offset_x + tx as i32 + state.cell_offset_x;
            let sy = state.head_offset_y + ty as i32 + state.cell_offset_y;
            let cell_x = floor_div(sx, 256);
            let cell_y = floor_div(sy, 256);
            if cell_x != 0 || cell_y != 0 {
                // Position is in a different cell — apply offset so
                // subsequent points are in the new cell's coordinate frame.
                state.cell_offset_x -= cell_x * 256;
                state.cell_offset_y -= cell_y * 256;
                cell_jump = true;
                cell_jump_dx = cell_x;
                cell_jump_dy = cell_y;
                break;
            }
        }

        // Track chaining: at chain_index, signal the caller to attempt
        // chaining into a follow-on track curve.
        // Against `point_index`, and deliberately NOT against `occupied_points`.
        // The handoff gate compares native's STORED cursor because it is read
        // from outside the loop (`0x004B49B6`), but this one is read INSIDE it,
        // before the tail increment: `0x004B1B39 MOV EAX,[EBP+0x5C]` then
        // `0x004B1B3C CMP [ECX + 0x7E7A2C],EAX` / `JNZ`, so the value it sees is
        // the index of the point just occupied. Do not "correct" this to match
        // the handoff gate - they read the same field at different moments.
        if meta.chain_index >= 0 && state.point_index == meta.chain_index as u16 {
            chain_ready = true;
            break;
        }
    }

    // Save leftover budget for next tick.
    state.residual = budget;
    *residual_budget = budget;

    // `finished` is set by the sentinel branch above, not by arriving at the
    // last real point: native occupies that point and only ends the curve on the
    // following read, which is one more paid step.
    // Track completion retains the canonical owner residual and its synchronized
    // detached-state mirror for the caller's next movement decision.

    // Read position and facing from current track point, applying
    // transform flags and head/cell offsets.
    let idx = state.point_index as usize;
    if idx < points.len() {
        let pt = &points[idx];
        let (tx, ty, tf) = transform_track_point(pt.x, pt.y, pt.facing, state.transform_flags);

        // Peek the next-to-consume point for sub-step interp.
        // Skip if at last_index (no next step) or if the next point is the
        // (0, 0) sentinel at a non-zero index (matches L6 in design ledger).
        let next_idx = idx + 1;
        let (next_dx, next_dy, has_next) =
            if next_idx < points.len() && (next_idx as u16) <= last_index {
                let npt = &points[next_idx];
                let (ntx, nty, _) =
                    transform_track_point(npt.x, npt.y, npt.facing, state.transform_flags);
                let is_sentinel = npt.x == 0 && npt.y == 0 && next_idx != 0;
                if is_sentinel {
                    (0, 0, false)
                } else {
                    ((ntx as i32) - (tx as i32), (nty as i32) - (ty as i32), true)
                }
            } else {
                (0, 0, false)
            };

        DriveTrackAdvance {
            sub_x: SimFixed::from_num(state.head_offset_x + tx as i32 + state.cell_offset_x),
            sub_y: SimFixed::from_num(state.head_offset_y + ty as i32 + state.cell_offset_y),
            facing: tf,
            cell_jump,
            cell_jump_dx,
            cell_jump_dy,
            chain_ready,
            finished,
            next_step_delta_x: next_dx,
            next_step_delta_y: next_dy,
            had_next_step: has_next,
        }
    } else {
        DriveTrackAdvance {
            sub_x: SimFixed::from_num(128),
            sub_y: SimFixed::from_num(128),
            facing: 0,
            cell_jump: false,
            cell_jump_dx: 0,
            cell_jump_dy: 0,
            chain_ready: false,
            finished: true,
            next_step_delta_x: 0,
            next_step_delta_y: 0,
            had_next_step: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Sub-step interpolation
// ---------------------------------------------------------------------------

/// Budget credited back when the step loop reads the end-of-track sentinel.
///
/// `0x004B1FD0..0x004B1FF9`: native takes the manhattan distance from the object
/// to the track head in leptons, then `FILD` / `FMUL [0x007E7FB8]` (1/11) /
/// `FSUBR [0x007E1718]` (1.0) / `FMUL [0x007E7FB0]` (7.0) / `ftol`, and
/// `ADD EBX,EAX` puts the result back into the running budget. All three
/// constants were read from the binary.
///
/// The head is where the sentinel `(0, 0)` maps to, so an object standing on
/// `last_point` is exactly the transformed point's own offset away from it.
///
/// What the term means: 7 is what a point *costs* (`SUB EDI,0x7`) and 11 is what
/// a point *spans* - track 1's y runs 245, 234, 223 ... 3, exactly 11 leptons a
/// step, and the diagonals step 8 and 8. So a full step was paid for but only
/// `manhattan` leptons of it were really left, and the unused part comes back.
/// A curve whose last point is further out than 11 therefore credits a negative
/// number, which is a charge for ground the final snap still covers.
///
/// This is **not** native's operation order: native multiplies by the double
/// stored at `0x007E7FB8` where this divides by `11.0`. That constant is the
/// correctly-rounded double for 1/11 and the two forms agree after truncation
/// across every manhattan a shipped track can produce, so the result is the
/// same. Truncation itself is not an assumption about the ambient control word:
/// `Math::ftol @ 0x007C5F00` does `FLDCW [0x00822D80]` (`0x0E7F`, round toward
/// zero) before its `FISTP` when the ambient word differs (the load is behind a
/// compare at `0x007C5F13`), so it chops regardless of the process CW.
fn terminal_budget_credit(points: &[TrackPoint], last_point: u16, transform_flags: u8) -> i32 {
    let Some(point) = points.get(usize::from(last_point)) else {
        return 0;
    };
    let (tx, ty, _) = transform_track_point(point.x, point.y, point.facing, transform_flags);
    let manhattan = i32::from(tx).abs() + i32::from(ty).abs();
    // `Math::ftol @ 0x007C5F00` truncates toward zero, which is what `as i32`
    // does for an f64 in range.
    ((1.0_f64 - f64::from(manhattan) / 11.0) * 7.0) as i32
}

/// Step cost denominator. The original engine consumes 7 budget units per
/// track point; residual budget after the step loop is in `0..=7` **on a curve
/// that has not ended**, and feeds fractional progress into the next-to-consume
/// step. The terminal step breaks that range: its credit is added after the
/// step is paid for and is negative on most tracks, so a curve that just ended
/// can carry a residual as low as -107: the loop only runs on `budget > 7` and
/// subtracts 7 before crediting, so at least 1 remains when the most negative
/// shipped credit (-108, track 11) is applied. See `terminal_budget_credit`.
const TRACK_STEP_DENOM: i32 = 7;

/// Above-this residual triggers the L4 trust window — past the step midpoint,
/// the interp result is used even when its cell classification looks
/// unexpected (handles rounding edge cases at large fractions).
const INTERP_TRUST_BUDGET: i32 = 3;

/// Output of `interp_sub_step`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InterpSubStepResult {
    /// X relative to the saved cell. It may lie in the full-step cell;
    /// ordinary movement then normalizes it in a residual crossing transaction.
    pub sub_x: SimFixed,
    /// Sub-cell Y to write to `Position.sub_y`.
    pub sub_y: SimFixed,
}

/// Compute a sub-step interpolated position from the residual budget and the
/// next-step peek. Returns `None` when no interp should be applied this tick
/// (residual < 1, no next step, or any L5/L6 early-exit condition).
///
/// `saved_sub_x/saved_sub_y` are the lepton sub-cell coords AFTER the discrete
/// step loop ran (the result of `advance_drive_track`). The cell anchor
/// (`Position.rx/ry`) is implicit — the L4 safety gate uses cell-relative
/// offsets, so the absolute cell coords are not needed here. A result outside
/// the saved cell requires caller-owned cell/list/OnBridge updates while
/// retaining raw Z; this helper only computes the coordinate.
/// `next_delta_x/next_delta_y` are from `DriveTrackAdvance.next_step_delta_*`.
/// `residual` is `state.residual` - `0..=7` after a normal step-loop exit, but
/// possibly **negative** after a terminal one, which the `residual < 1` guard
/// below absorbs along with zero. That is load-bearing: a negative residual must
/// not interpolate backwards.
pub(crate) fn interp_sub_step(
    saved_sub_x: SimFixed,
    saved_sub_y: SimFixed,
    next_delta_x: i32,
    next_delta_y: i32,
    residual: i32,
    had_next_step: bool,
) -> Option<InterpSubStepResult> {
    // L5: no interp when residual is zero (vehicle made no fractional progress
    // this tick — either it consumed exactly 7-multiples of budget, or it had
    // none to begin with).
    if residual < 1 {
        return None;
    }
    // L5/L6: no interp without a valid next step (track end or sentinel).
    if !had_next_step {
        return None;
    }

    // L2: interp offset = next_delta * residual / 7 (truncate toward zero).
    let interp_dx: i32 = next_delta_x * residual / TRACK_STEP_DENOM;
    let interp_dy: i32 = next_delta_y * residual / TRACK_STEP_DENOM;

    // Full-step offset (what the next step would produce at residual = 7).
    let full_dx: i32 = next_delta_x;
    let full_dy: i32 = next_delta_y;

    // Convert saved sub-coords to absolute lepton-space (saved_cell-origin).
    let saved_lx: i32 = saved_sub_x.to_num::<i32>();
    let saved_ly: i32 = saved_sub_y.to_num::<i32>();

    // L4 cell membership: classify which cell the interp/full positions land in,
    // expressed as cell-relative offsets from the saved cell.
    let interp_cell_dx: i32 = floor_div(saved_lx + interp_dx, 256);
    let interp_cell_dy: i32 = floor_div(saved_ly + interp_dy, 256);
    let full_cell_dx: i32 = floor_div(saved_lx + full_dx, 256);
    let full_cell_dy: i32 = floor_div(saved_ly + full_dy, 256);

    let in_saved = interp_cell_dx == 0 && interp_cell_dy == 0;
    let in_full = interp_cell_dx == full_cell_dx && interp_cell_dy == full_cell_dy;

    // L4 gate: use interp if it lands in saved or full cell, OR residual > 3
    // (the trust window — past the step midpoint, trust the interp even if
    // cell classification looks off).
    let use_interp = in_saved || in_full || residual > INTERP_TRUST_BUDGET;

    if use_interp {
        Some(InterpSubStepResult {
            sub_x: SimFixed::from_num(saved_lx + interp_dx),
            sub_y: SimFixed::from_num(saved_ly + interp_dy),
        })
    } else {
        // L4 fallback: use full-step coords.
        Some(InterpSubStepResult {
            sub_x: SimFixed::from_num(saved_lx + full_dx),
            sub_y: SimFixed::from_num(saved_ly + full_dy),
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "drive_track_tests.rs"]
mod drive_track_tests;
