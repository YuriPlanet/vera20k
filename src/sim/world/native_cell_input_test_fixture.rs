//! Supplied Cell/zone fixture shared by original Foot4DE1D0 and Building443860 inputs.
use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid, zone_class};
use crate::rules::terrain_rules::TerrainRules;
use crate::sim::bridge_state::{BridgeEndpointRecord, BridgeRecordKind};
use crate::sim::cell_rect::PlayfieldBounds;
use crate::sim::components::DriveCoord;
use crate::sim::occupancy::RawCellOccupationGrid;
use crate::sim::pathfinding::{PathGrid, zone_map::ZoneGrid};
use serde_json::Value;
pub(super) fn terrain_cell(rx: u16, ry: u16) -> ResolvedTerrainCell {
    ResolvedTerrainCell {
        zone_type: zone_class::GROUND,
        ..crate::map::resolved_terrain::test_flat_cell(rx, ry)
    }
}

pub(super) fn bounds() -> PlayfieldBounds {
    PlayfieldBounds {
        base: 8,
        off_fc: 0,
        off_100: 0,
        off_104: 8,
        off_108: 8,
    }
}

pub(super) fn pair(value: &Value) -> (u16, u16) {
    (
        value[0].as_i64().unwrap() as u16,
        value[1].as_i64().unwrap() as u16,
    )
}

pub(super) fn coord(value: &Value) -> DriveCoord {
    DriveCoord {
        x: value[0].as_i64().unwrap() as i32,
        y: value[1].as_i64().unwrap() as i32,
        z: value[2].as_i64().unwrap() as i32,
    }
}

/// Exactly the supplied cell/zone input of the native corpus; this does not
/// turn supplied cluster rows into a test of native flood construction.
pub(super) fn native_fixture(
    row: &Value,
) -> (ResolvedTerrainGrid, ZoneGrid, RawCellOccupationGrid) {
    let zero_speed_table = row["zero_speed_table"].as_bool().unwrap_or(false);
    let land_rules = TerrainRules::from_ini(&crate::rules::ini_parser::IniFile::from_str(
        if zero_speed_table {
            "[Clear]\nFoot=0%\nTrack=0%\nWheel=0%\nFloat=0%\n\
             Amphibious=0%\nFloatBeach=0%\nHover=0%\n"
        } else {
            // The native fixture seeds every supplied speed-table slot with
            // 1.0. Give the production reader actual keys: it deliberately
            // drops an empty section, so `[Clear]` alone has no semantics row.
            "[Clear]\nFoot=100%\nTrack=100%\nWheel=100%\nFloat=100%\n\
             Amphibious=100%\nFloatBeach=100%\nHover=100%\n"
        },
    ));
    let costs = land_rules.semantics_for_land_type(0).unwrap().speed_costs;
    let mut cells: Vec<_> = (0..16)
        .flat_map(|y| {
            (0..16).map(move |x| ResolvedTerrainCell {
                speed_costs: costs,
                base_speed_costs: costs,
                ..terrain_cell(x, y)
            })
        })
        .collect();
    let mut raw = RawCellOccupationGrid::default();
    for change in row["cells"].as_array().into_iter().flatten() {
        let xy = pair(&change["xy"]);
        let cell = &mut cells[usize::from(xy.1) * 16 + usize::from(xy.0)];
        cell.level = change["level"].as_u64().unwrap_or(0) as u8;
        cell.bridge_facts.raw_flags = change["flags"].as_u64().unwrap_or(0) as u32;
        raw.mark_ground(xy.0, xy.1, change["raw"].as_u64().unwrap_or(0) as u8);
        raw.mark_deck(xy.0, xy.1, change["deck_raw"].as_u64().unwrap_or(0) as u8);
    }
    let mut terrain = ResolvedTerrainGrid::from_cells(16, 16, cells);
    // The same native supplied table governs the shared Dummy's Clear land
    // row. Real cells alone are not a complete zero-table fixture.
    terrain.set_land_speed_rules_for_test(&land_rules);
    terrain.stamp_dummy_cell_requested_coord(99, 98);
    let records: Vec<_> = row["records"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|r| BridgeEndpointRecord {
            endpoint_a: pair(&r[0]),
            endpoint_b: pair(&r[1]),
            active: r[2].as_i64().unwrap() != 0,
            bridge_kind: BridgeRecordKind::High,
        })
        .collect();
    let mut zones = ZoneGrid::build_with_native_bridge_geometry(
        &PathGrid::new(16, 16),
        &terrain,
        &records,
        16,
        16,
        Some((8, 8)),
    );
    let base = zones.base_topology_mut();
    for y in 0..16 {
        for x in 0..16 {
            base.zone_ids[y * 16 + x] =
                u16::from(row["split"].as_bool().unwrap_or(false) && x >= 8);
        }
    }
    for row in &mut base.raw_zone_ids_by_row {
        *row = vec![2, 3];
    }
    (terrain, zones, raw)
}
