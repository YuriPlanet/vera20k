//! gamemd 8-direction cell-delta table + stepping accessors.
//!
//! Single sim-facing entry point for the "which adjacent cell" primitive. The
//! values come from `util::direction::DIRECTION_DELTAS`. Original initializer
//! `0x0049F2F0` through RET `0x0049F39B` establishes the eight table values
//! at `0x0089F688`; the saved executable controls are compared below.
//! Compass order 0=N..7=NW, +X=east, +Y=south. These controls cover table
//! initialization, not every consumer's arithmetic or movement behavior.

use crate::util::direction::DIRECTION_DELTAS;

/// gamemd 8-direction cell-delta table, compass order. Canonical reference
/// (identical to `util::direction::DIRECTION_DELTAS`).
pub const CELL_DELTAS: [(i32, i32); 8] = DIRECTION_DELTAS;

/// Checked cell-delta. `None` for `dir > 7` (incl. the tube sentinel 8) — the
/// safe sim accessor. Delegates to the canonical
/// `util::direction::direction_delta` so the checked lookup exists once.
pub fn cell_delta(dir: u8) -> Option<(i32, i32)> {
    crate::util::direction::direction_delta(dir)
}

/// Faithful mirror of gamemd's unchecked `MapCoord_Step_By_Direction` indexing
/// (no mask/bounds; callers sanitize upstream). Debug-asserts `dir <= 7` and
/// masks `&7` to stay memory-safe; use only when mirroring that contract.
pub fn cell_delta_unchecked(dir: u8) -> (i32, i32) {
    debug_assert!(
        dir <= 7,
        "cell_delta_unchecked: dir {dir} > 7 (gamemd OOB read)"
    );
    CELL_DELTAS[(dir & 7) as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(controls.len(), 2); // inherited 0000 and supplied 0E7F FPCW
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
            assert_eq!(CELL_DELTAS.as_slice(), expected.as_slice());
            for (i, &e) in expected.iter().enumerate() {
                assert_eq!(cell_delta(i as u8), Some(e));
            }
        }
        assert_eq!(cell_delta(8), None); // tube sentinel, not a 9th compass dir
        assert_eq!(cell_delta(255), None);
    }
}
