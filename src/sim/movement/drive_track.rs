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
//! 4. The retained class cursor and TrackProcess world host own stepping;
//!    this module contains immutable geometry and selection only.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on util/fixed_math.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

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
pub(crate) fn transform_track_point(x: i16, y: i16, facing: u8, flags: u8) -> (i16, i16, u8) {
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
#[cfg(test)]
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
#[cfg(test)]
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
    #[cfg(test)]
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
    body_facing: u16,
    from_delta: (i32, i32),
    to_delta: Option<(i32, i32)>,
    is_ship: bool,
) -> DriveTrackDecision {
    let Some(from_dir) = octant_from_cell_delta(from_delta.0, from_delta.1) else {
        return DriveTrackDecision::Unavailable;
    };

    // Exact-facing precondition. Zero tolerance: gamemd compares the 16-bit
    // facing against `path[0]_dir << 13`; do not truncate the live sample.
    // Original gate and one-bit cases: drive_fresh_turn.json (4B3408).
    let desired_facing = (from_dir as u8).wrapping_mul(OCTANT_FACING_STEP);
    if body_facing != u16::from(desired_facing) << 8 {
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
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "drive_track_tests.rs"]
mod drive_track_tests;
