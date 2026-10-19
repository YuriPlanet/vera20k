//! Canonical RA2/YR direction ids and facing-byte helpers.
//!
//! The active `gamemd.exe` direction table is:
//! `0=N, 1=NE, 2=E, 3=SE, 4=S, 5=SW, 6=W, 7=NW`.
//! Direction byte `8` is a tube-step sentinel in path replay helpers, not a
//! ninth compass direction.
//!
//! ## Ownership split (three direction-math owners in util/, F14)
//! - **This module** is the single authority for the 8-way vocabulary:
//!   `DIRECTION_DELTAS` (with its unchecked step `cell_delta_unchecked`), the
//!   `Ra2Direction` ids, and the 8-bit facing→direction quantization
//!   (`direction_from_facing`).
//! - **`direction_tables/`** adds what this module does not own: 16-bit
//!   facings, lepton deltas, DRAGON frames, muzzle rotation.
//! - **`native_trig.rs`** owns facing→movement *vectors* and FLH rotations
//!   read from the retail sine table; it owns no direction quantization.
//!
//! Do not add a new facing/direction conversion without checking all three
//! headers — a fourth parallel helper is how definition drift starts.
//! `opposite_direction` below is the single checked `(d±4)&7` helper.

pub const DIRECTION_COUNT: usize = 8;
pub const TUBE_STEP_DIRECTION: u8 = 8;
pub const FACING_UNITS_PER_DIRECTION: u8 = 32;

/// Cell compass values initialized at `0x0049F2F0` into `0x0089F688`.
/// Original initializer execution through RET `0x0049F39B` is preserved in
/// `tools/spatial_oracle/locomotor_head_coordinates.json::initializer_controls`.
/// Compass order 0=N..7=NW, +X=east, +Y=south. The lepton table derives the
/// same order in its own coordinate units.
pub const DIRECTION_DELTAS: [(i32, i32); DIRECTION_COUNT] = [
    (0, -1),  // 0 = N
    (1, -1),  // 1 = NE
    (1, 0),   // 2 = E
    (1, 1),   // 3 = SE
    (0, 1),   // 4 = S
    (-1, 1),  // 5 = SW
    (-1, 0),  // 6 = W
    (-1, -1), // 7 = NW
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Ra2Direction {
    North = 0,
    NorthEast = 1,
    East = 2,
    SouthEast = 3,
    South = 4,
    SouthWest = 5,
    West = 6,
    NorthWest = 7,
}

impl Ra2Direction {
    pub fn from_index(index: u8) -> Option<Self> {
        match index {
            0 => Some(Self::North),
            1 => Some(Self::NorthEast),
            2 => Some(Self::East),
            3 => Some(Self::SouthEast),
            4 => Some(Self::South),
            5 => Some(Self::SouthWest),
            6 => Some(Self::West),
            7 => Some(Self::NorthWest),
            _ => None,
        }
    }

    pub fn index(self) -> u8 {
        self as u8
    }

    pub fn delta(self) -> (i32, i32) {
        DIRECTION_DELTAS[self.index() as usize]
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn short_name(self) -> &'static str {
        match self {
            Self::North => "N",
            Self::NorthEast => "NE",
            Self::East => "E",
            Self::SouthEast => "SE",
            Self::South => "S",
            Self::SouthWest => "SW",
            Self::West => "W",
            Self::NorthWest => "NW",
        }
    }
}

pub fn direction_delta(direction: u8) -> Option<(i32, i32)> {
    Ra2Direction::from_index(direction).map(Ra2Direction::delta)
}

/// Faithful mirror of gamemd's unchecked `MapCoord_Step_By_Direction` indexing
/// (no mask/bounds; callers sanitize upstream). Debug-asserts `dir <= 7` and
/// masks `&7` to stay memory-safe; use only when mirroring that contract.
pub fn cell_delta_unchecked(dir: u8) -> (i32, i32) {
    debug_assert!(
        dir <= 7,
        "cell_delta_unchecked: dir {dir} > 7 (gamemd OOB read)"
    );
    DIRECTION_DELTAS[(dir & 7) as usize]
}

/// Direction whose one-cell step is exactly `(dx, dy)`; `None` for a null or
/// longer step.
pub fn direction_from_delta(dx: i32, dy: i32) -> Option<u8> {
    DIRECTION_DELTAS
        .iter()
        .position(|&delta| delta == (dx, dy))
        .map(|index| index as u8)
}

pub fn direction_from_facing(facing: u8) -> u8 {
    (facing.wrapping_add(FACING_UNITS_PER_DIRECTION / 2) / FACING_UNITS_PER_DIRECTION)
        & (DIRECTION_COUNT as u8 - 1)
}

pub fn delta_from_facing(facing: u8) -> (i32, i32) {
    DIRECTION_DELTAS[direction_from_facing(facing) as usize]
}

pub fn opposite_direction(direction: u8) -> Option<u8> {
    Ra2Direction::from_index(direction).map(|d| (d.index().wrapping_add(4)) & 7)
}

pub fn is_tube_step_direction(direction: u8) -> bool {
    direction == TUBE_STEP_DIRECTION
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direction_ids_match_gamemd_compass_table() {
        let expected = [
            ("N", (0, -1)),
            ("NE", (1, -1)),
            ("E", (1, 0)),
            ("SE", (1, 1)),
            ("S", (0, 1)),
            ("SW", (-1, 1)),
            ("W", (-1, 0)),
            ("NW", (-1, -1)),
        ];
        for (idx, &(name, delta)) in expected.iter().enumerate() {
            let direction = Ra2Direction::from_index(idx as u8).unwrap();
            assert_eq!(direction.index(), idx as u8);
            assert_eq!(direction.short_name(), name);
            assert_eq!(direction.delta(), delta);
            assert_eq!(direction_delta(idx as u8), Some(delta));
        }
    }

    #[test]
    fn direction_8_is_tube_sentinel_not_compass() {
        assert!(is_tube_step_direction(8));
        assert_eq!(Ra2Direction::from_index(8), None);
        assert_eq!(direction_delta(8), None);
    }

    #[test]
    fn invalid_non_8_directions_do_not_wrap() {
        for direction in [9, 10, 15, 16, 255] {
            assert_eq!(Ra2Direction::from_index(direction), None);
            assert_eq!(direction_delta(direction), None);
        }
    }

    /// Original initializer execution through RET `0x0049F39B`, under the
    /// inherited `0000` and the supplied `0E7F` control words.
    #[test]
    fn cell_delta_table_equals_gamemd_dump() {
        let original: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/locomotor_head_coordinates.json",
        ))
        .unwrap();
        let controls: Vec<_> = original["initializer_controls"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["kind"] == "cell")
            .collect();
        assert_eq!(controls.len(), 2);
        for row in controls {
            let expected: Vec<_> = row["values"]
                .as_array()
                .unwrap()
                .iter()
                .map(|pair| {
                    (
                        pair[0].as_i64().unwrap() as i32,
                        pair[1].as_i64().unwrap() as i32,
                    )
                })
                .collect();
            assert_eq!(DIRECTION_DELTAS.as_slice(), expected.as_slice());
        }
    }

    /// gamemd's 8-bit form is `((f>>4)+1)>>1 & 7`; it rounds up at `16+32n`
    /// and `240..=255` wrap to North.
    #[test]
    fn facing_quantization_matches_gamemd_for_every_byte() {
        for f in 0u8..=255 {
            let gamemd = ((((f as u16) >> 4) + 1) >> 1) as u8 & 7;
            assert_eq!(direction_from_facing(f), gamemd, "f={f}");
        }
    }

    #[test]
    fn facing_quantization_matches_drive_locomotor_formula() {
        let samples = [
            (0, 0),
            (15, 0),
            (16, 1),
            (32, 1),
            (47, 1),
            (48, 2),
            (64, 2),
            (96, 3),
            (128, 4),
            (160, 5),
            (192, 6),
            (224, 7),
            (239, 7),
            (240, 0),
            (255, 0),
        ];
        for (facing, direction) in samples {
            assert_eq!(direction_from_facing(facing), direction);
            assert_eq!(
                delta_from_facing(facing),
                DIRECTION_DELTAS[direction as usize]
            );
        }
    }

    /// A compass facing's cell step converts back through the native angle
    /// path to within a few facing units of where it started.
    #[test]
    fn delta_from_facing_round_trips_through_the_native_facing() {
        for facing in (0u8..=224).step_by(32) {
            let (dx, dy) = delta_from_facing(facing);
            let recovered = crate::util::direction_tables::facing8_from_delta(dx, dy);
            let distance = recovered
                .wrapping_sub(facing)
                .min(facing.wrapping_sub(recovered));
            assert!(distance <= 4, "{facing} -> ({dx},{dy}) -> {recovered}");
        }
    }
}
