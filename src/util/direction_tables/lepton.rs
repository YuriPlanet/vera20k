//! Eight compass deltas in leptons: cell delta ×256, one cell per axis.
//! Original initializer `0x0049F3A0` through RET `0x0049F413` writes the
//! table at `0x0089F6D8`. The saved executable controls are compared below.
//! Consumers own speed scaling and signed-coordinate arithmetic; these
//! initialized values do not establish their complete movement behavior.

use super::cell::CELL_DELTAS;

const LEPTONS_PER_CELL: i32 = crate::util::lepton::LEPTONS_PER_CELL_I32;

/// 8-direction lepton-delta table = `CELL_DELTAS[i] * 256`, compass order.
/// Const-derived from the cell owner; both initializers have original controls.
pub const LEPTON_DELTAS: [(i32, i32); 8] = {
    let mut out = [(0i32, 0i32); 8];
    let mut i = 0;
    while i < 8 {
        out[i] = (
            CELL_DELTAS[i].0 * LEPTONS_PER_CELL,
            CELL_DELTAS[i].1 * LEPTONS_PER_CELL,
        );
        i += 1;
    }
    out
};

/// Checked lepton-delta for a direction. `None` for `dir > 7`.
pub fn lepton_delta(dir: u8) -> Option<(i32, i32)> {
    LEPTON_DELTAS.get(dir as usize).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lepton_delta_table_equals_gamemd_dump() {
        let original: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/locomotor_head_coordinates.json",
        ))
        .unwrap();
        let controls: Vec<_> = original["initializer_controls"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["kind"] == "lepton")
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
            assert_eq!(LEPTON_DELTAS.as_slice(), expected.as_slice());
            for (i, &e) in expected.iter().enumerate() {
                assert_eq!(lepton_delta(i as u8), Some(e));
            }
        }
        assert_eq!(lepton_delta(8), None);
    }

    #[test]
    fn lepton_is_cell_times_256() {
        for i in 0..8 {
            assert_eq!(
                LEPTON_DELTAS[i],
                (CELL_DELTAS[i].0 * 256, CELL_DELTAS[i].1 * 256)
            );
        }
    }
}
