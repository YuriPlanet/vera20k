//! The group spread's height reads against the original `0x0064CDA0` slices:
//! tools/spatial_oracle/group_spread_gates.{py,json,meta.json}.

use super::{spread_height, within_spread_band};
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::sim::combat::TargetKind;
use crate::sim::command::Command;
use crate::sim::world::Simulation;
use crate::sim::world::common_raw_test_terrain_cell;

/// `0x0064CDA0` returns at once unless the destination token is a Cell
/// (`0x0064CDCA..0x0064CDDC`): a Move onto an object and an attack-move with a
/// target or an object destination have no run key.
#[test]
fn only_a_cell_destination_has_a_spread_key() {
    let attack_move = |target, destination| Command::AttackMove {
        entity_id: 1,
        target,
        destination,
    };
    let move_to_cell = Command::Move {
        entity_id: 1,
        target_rx: 4,
        target_ry: 5,
    };
    assert_eq!(Simulation::formation_key(&move_to_cell), Some((0, 4, 5)));
    assert_eq!(
        Simulation::formation_key(&attack_move(None, Some(TargetKind::Cell(4, 5)))),
        Some((1, 4, 5))
    );
    for command in [
        Command::MoveToObject {
            entity_id: 1,
            target_id: 2,
        },
        attack_move(None, Some(TargetKind::Entity(2))),
        attack_move(Some(TargetKind::Entity(2)), None),
    ] {
        assert_eq!(Simulation::formation_key(&command), None, "{command:?}");
    }
}

#[test]
fn spread_heights_and_band_match_the_original_slices() {
    let rows: Vec<serde_json::Value> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/group_spread_gates.json",
    ))
    .unwrap();
    assert_eq!(rows.len(), 524);
    for row in rows {
        let input = &row["input"];
        let cell = |x, level: &str, flags: &str| {
            let level = input[level].as_i64().unwrap() as i8 as u8;
            let mut cell = common_raw_test_terrain_cell(x, 0, level, false);
            cell.bridge_facts.raw_flags = input[flags].as_u64().unwrap() as u32;
            cell
        };
        let terrain = ResolvedTerrainGrid::from_cells(
            2,
            1,
            vec![
                cell(0, "target_level", "target_flags"),
                cell(1, "candidate_level", "candidate_flags"),
            ],
        );
        let target = spread_height(&terrain, terrain.native_cell_identity((0, 0)));
        assert_eq!(
            i64::from(target),
            row["target_height"].as_i64().unwrap(),
            "{input}"
        );
        // VERA keeps a reservation (`+0x140 & 0x8000`) in the distributor's
        // reserved set, which it tests before the band, as native does.
        if input["candidate_flags"].as_u64().unwrap() & 0x8000 != 0 {
            assert_eq!(row["outcome"], "reserved", "{input}");
            continue;
        }
        let candidate = spread_height(&terrain, terrain.native_cell_identity((1, 0)));
        assert_eq!(
            within_spread_band(target, candidate),
            row["outcome"] == "code_test",
            "{input}"
        );
    }
}
