//! Native supplied-row comparison and connected input fixtures.
use super::*;
use crate::map::bridge_facts::BridgeCellFacts;
use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid, zone_class};
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::rules::terrain_rules::{SpeedCostProfile, TerrainClass};
use crate::sim::bridge_state::{BridgeEndpointRecord, BridgeRecordKind};
use crate::sim::pathfinding::PathGrid;
use serde_json::{Value, json};
use std::collections::BTreeMap;
fn terrain_cell(rx: u16, ry: u16) -> ResolvedTerrainCell {
    ResolvedTerrainCell {
        rx,
        ry,
        source_tile_index: 0,
        source_sub_tile: 0,
        final_tile_index: 0,
        final_sub_tile: 0,
        is_wood_bridge_repair_tile: false,
        level: 0,
        filled_clear: false,
        tileset_index: Some(0),
        land_type: 0,
        yr_cell_land_type: 0,
        slope_type: 0,
        template_height: 0,
        render_offset_x: 0,
        render_offset_y: 0,
        terrain_class: TerrainClass::Clear,
        speed_costs: SpeedCostProfile::default(),
        is_water: false,
        is_cliff_like: false,
        is_rough: false,
        is_road: false,
        accepts_smudge: false,
        allows_tiberium: false,
        height_in_pixels: 0,
        variant: 0,
        has_ramp: false,
        canonical_ramp: None,
        ground_walk_blocked: false,
        terrain_object_blocks: false,
        terrain_object_occupation: None,
        overlay_blocks: false,
        overlay_zone_type: None,
        outside_playfield: false,
        zone_type: zone_class::GROUND,
        base_ground_walk_blocked: false,
        base_build_blocked: false,
        base_land_type: 0,
        base_yr_cell_land_type: 0,
        base_terrain_class: TerrainClass::Clear,
        base_speed_costs: SpeedCostProfile::default(),
        build_blocked: false,
        has_bridge_deck: false,
        bridge_walkable: false,
        bridge_transition: false,
        bridge_deck_level: 0,
        bridge_layer: None,
        bridge_facts: BridgeCellFacts::default(),
        tube_index: None,
        radar_left: [0, 0, 0],
        radar_right: [0, 0, 0],
        has_damaged_data: false,
        bridgehead_anchor_class_at_load: None,
    }
}

fn bounds() -> PlayfieldBounds {
    PlayfieldBounds {
        base: 8,
        off_fc: 0,
        off_100: 0,
        off_104: 8,
        off_108: 8,
    }
}

fn pair(value: &Value) -> (u16, u16) {
    (
        value[0].as_i64().unwrap() as u16,
        value[1].as_i64().unwrap() as u16,
    )
}

fn coord(value: &Value) -> DriveCoord {
    DriveCoord {
        x: value[0].as_i64().unwrap() as i32,
        y: value[1].as_i64().unwrap() as i32,
        z: value[2].as_i64().unwrap() as i32,
    }
}

/// Exactly the supplied cell/zone input of the native corpus; this does not
/// turn supplied cluster rows into a test of native flood construction.
fn native_fixture(row: &Value) -> (ResolvedTerrainGrid, ZoneGrid, RawCellOccupationGrid) {
    let mut cells: Vec<_> = (0..16)
        .flat_map(|y| (0..16).map(move |x| terrain_cell(x, y)))
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
    let terrain = ResolvedTerrainGrid::from_cells(16, 16, cells);
    terrain.stamp_dummy_cell_requested_coord(99, 98);
    let records: Vec<_> = row["records"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|r| BridgeEndpointRecord {
            endpoint_a: pair(&r[0]),
            endpoint_b: pair(&r[1]),
            group_id: 0,
            active: r[2].as_i64().unwrap() != 0,
            bridge_kind: BridgeRecordKind::High,
        })
        .collect();
    let mut zones = ZoneGrid::build_with_native_bridge_geometry(
        &PathGrid::new(16, 16),
        &BTreeMap::new(),
        Some(&terrain),
        &records,
        16,
        16,
        Some((8, 8)),
    );
    let base = zones.base_topology_mut().unwrap();
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

/// 20 Infantry/Walk, 7 Unit/Drive and 2 Unit/Ship receivers of the same
/// 4DE1D0 corpus: the resolver is class-generic, so one Rust owner serves all.
#[test]
fn ordinary_foot_input_matches_native_admission_rows() {
    let rows: Value = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/walk_move_admission.json"
    ))
    .unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 29);
    for (index, row) in rows.as_array().unwrap().iter().enumerate() {
        let input = &row["input"];
        let (terrain, zones, raw) = native_fixture(input);
        let cells = NativeCellQuery::isolated(&terrain);
        let current = coord(input.get("xyz").unwrap_or(&json!([1344, 1344, 0])));
        let head = coord(input.get("head").unwrap_or(&json!([0, 0, 0])));
        let clicked = pair(input.get("clicked").unwrap_or(&json!([11, 5])));
        let click = FootCellClick {
            clicked: (clicked.0 as i16, clicked.1 as i16),
            action: input["action"].as_u64().unwrap_or(1) as u32,
            current,
            coordinate: if head == (DriveCoord { x: 0, y: 0, z: 0 }) {
                current
            } else {
                head
            },
            marked: input["marked"].as_bool().unwrap_or(true),
            on_bridge: input["on_bridge"].as_bool().unwrap_or(false),
            in_tube: false,
            move_to_shroud: input["move_to_shroud"].as_bool().unwrap_or(true),
            teleporter: false,
            jumpjet_type: false,
            movement_zone: match input["movement_zone"].as_u64().unwrap_or(4) {
                0 => MovementZone::Normal,
                1 => MovementZone::Crusher,
                3 => MovementZone::AmphibiousDestroyer,
                4 => MovementZone::AmphibiousCrusher,
                10 => MovementZone::Water,
                other => panic!("unmapped movement zone {other}"),
            },
            speed_type: match input["receiver"].as_str() {
                Some("unit") => SpeedType::Track,
                Some("ship") => SpeedType::Float,
                _ => SpeedType::Foot,
            },
        };
        let result = resolve_foot_cell_click(
            &cells,
            &zones,
            &raw,
            bounds(),
            (8, 8),
            input["frame"].as_u64().unwrap_or(100) as u32,
            &click,
            |id| {
                Ok(match id {
                    NativeCellIdentity::Real(_) => {
                        !input["shrouded"].as_array().into_iter().flatten().any(|p| {
                            let c = cells.coord(id);
                            pair(p) == (c.0 as u16, c.1 as u16)
                        })
                    }
                    NativeCellIdentity::Dummy => false,
                })
            },
        )
        .unwrap();
        let expected = pair(&row["cell"]);
        assert_eq!(
            result,
            (expected != (0, 0)).then_some(expected),
            "row {index}: {input}"
        );
        assert_eq!(
            cells.coord(NativeCellIdentity::Dummy),
            (pair(&row["dummy"]).0 as i16, pair(&row["dummy"]).1 as i16),
            "Dummy row {index}"
        );
        assert_eq!(
            terrain.dummy_cell_requested_coord(),
            (99, 98),
            "input must not stamp canonical Dummy row {index}"
        );
    }
}

#[test]
fn isolated_cell_queries_preserve_all_modeled_dummy_inputs_and_identity() {
    let terrain = ResolvedTerrainGrid::from_cells(1, 1, vec![terrain_cell(0, 0)]);
    let canonical = terrain.shared_cell_dummy();
    canonical.stamp_coord(99, 98);
    canonical.set_level_slope(-3, 2);
    canonical.write_raw_flags(0x10d80);
    canonical.write_native_anchor(Some(NativeCellIdentity::Real(0)));
    canonical.set_overlay_fields(Some(0x18), 7);
    canonical.write_raw_tube_index(23);
    let before = format!("{canonical:?}");
    let cells = NativeCellQuery::isolated(&terrain);
    let copied = cells.dummy();
    assert_eq!(copied.snapshot(), canonical.snapshot());
    assert_eq!(copied.raw_flags(), canonical.raw_flags());
    assert_eq!(copied.native_anchor(), canonical.native_anchor());
    assert_eq!(copied.overlay_fields(), canonical.overlay_fields());
    assert_eq!(copied.raw_tube_index(), canonical.raw_tube_index());
    let retained = cells.lookup((20, 21));
    assert_eq!(retained, NativeCellIdentity::Dummy);
    assert_eq!(cells.lookup_world(22 * 256, 23 * 256), retained);
    assert_eq!(cells.coord(retained), (22, 23));
    assert_eq!(copied.snapshot().coord, (22, 23));
    assert_eq!(cells.flags(retained), 0x10d80);
    assert_eq!(format!("{canonical:?}"), before);
}

impl Simulation {
    /// Synthetic incoming terrain/bridge records; movement, input, codec and
    /// scheduled-frame owners remain production. Native flood/path goldens
    /// are deliberately not inferred from this geometry regression.
    pub(crate) fn walk_cell_input_test_scene(bridge: bool) -> (Self, RuleSet, u64) {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n0=E1\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
             [E1]\nStrength=100\nSpeed=4\nSight=8\nMovementZone=Infantry\nSpeedType=Foot\n\
             Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
             [Clear]\nFoot=100%\nTrack=100%\nFloat=0%\n\
             [Water]\nFoot=0%\nTrack=0%\nFloat=100%\n",
        ))
        .unwrap();
        let mut sim = Self::with_seed(0x4de1d0);
        sim.intern_rule_type_ids(&rules);
        let owner = sim.interner.intern("Local");
        sim.houses.insert(
            owner,
            crate::sim::house_state::HouseState::new(owner, 0, None, true, 0, 10),
        );
        sim.session.house_order.push(owner);
        sim.session.current_house = Some(owner);
        sim.session.binary_frame = 100;
        // Infantry-only contenders need Long Game: Short Game correctly
        // defeats both Houses without a building or BaseUnit.
        sim.session.game_options.short_game = false;
        sim.session.map_width = 16;
        sim.session.map_height = 16;
        sim.playfield_bounds = Some(bounds());
        sim.playfield_size_height = Some(8);
        let clear = rules
            .terrain_rules
            .semantics_by_name("Clear")
            .unwrap()
            .speed_costs;
        let water = rules
            .terrain_rules
            .semantics_by_name("Water")
            .unwrap()
            .speed_costs;
        let cells = (0..16)
            .flat_map(|y| {
                (0..16).map(move |x| {
                    let mut cell = terrain_cell(x, y);
                    let river = (8..=10).contains(&x);
                    cell.level = if bridge && !river { 4 } else { 0 };
                    cell.speed_costs = if river { water } else { clear };
                    cell.base_speed_costs = cell.speed_costs;
                    if river {
                        cell.is_water = true;
                        cell.terrain_class = TerrainClass::Water;
                        cell.base_terrain_class = TerrainClass::Water;
                        cell.yr_cell_land_type = 2;
                        cell.base_yr_cell_land_type = 2;
                        cell.land_type = 2;
                        cell.base_land_type = 2;
                        cell.zone_type = crate::map::resolved_terrain::zone_class::WATER;
                        cell.ground_walk_blocked = true;
                        cell.base_ground_walk_blocked = true;
                    }
                    if bridge && river && y == 6 {
                        // 583209's direction bit preserves the lane offset for
                        // the x-directed (7,6)..(11,6) record. The traversable
                        // lane carries 0x200 throughout: 4D9D9A requires it on
                        // entry and 4D9E08 for deck→deck at raw level 0.
                        // It distinguishes stamped lanes, not span endpoints.
                        cell.bridge_facts.raw_flags = 0x100 | 0x800 | 0x200;
                        cell.has_bridge_deck = cell.bridge_facts.has_structural_bridge();
                        cell.bridge_walkable = true;
                        cell.bridge_transition = cell.bridge_facts.raw_flags & 0x200 != 0;
                        cell.bridge_deck_level = 4;
                    }
                    cell
                })
            })
            .collect();
        sim.install_resolved_terrain_for_new_map(ResolvedTerrainGrid::from_cells(16, 16, cells));
        assert!(sim.rebuild_dynamic_navigation(&rules));
        if bridge {
            let records = [BridgeEndpointRecord {
                endpoint_a: (7, 6),
                endpoint_b: (11, 6),
                group_id: 0,
                active: true,
                bridge_kind: BridgeRecordKind::High,
            }];
            sim.zone_grid = Some(ZoneGrid::build_with_native_bridge_geometry(
                sim.path_grid().unwrap(),
                &sim.terrain_costs,
                sim.resolved_terrain.as_ref(),
                &records,
                16,
                16,
                Some((8, 8)),
            ));
        }
        let heights = sim
            .resolved_terrain
            .as_ref()
            .unwrap()
            .cells()
            .iter()
            .map(|c| ((c.rx, c.ry), c.level))
            .collect::<BTreeMap<_, _>>();
        let start = if bridge { (7, 6) } else { (5, 5) };
        let actor = sim
            .spawn_object("E1", "Local", start.0, start.1, 0, &rules, &heights)
            .unwrap();
        // Keep two live Long Game contenders while the real frame host runs.
        let enemy = sim.interner.intern("Enemy");
        sim.houses.insert(
            enemy,
            crate::sim::house_state::HouseState::new(enemy, 1, None, false, 0, 10),
        );
        sim.session.house_order.push(enemy);
        sim.spawn_object("E1", "Enemy", 12, 11, 0, &rules, &heights)
            .unwrap();
        sim.resolve_type_handles(&rules);
        sim.fog.width = 16;
        sim.fog.height = 16;
        sim.fog.reveal_all_for_owner(owner);
        assert!(sim.entities().get(actor).unwrap().lifecycle.cell_marked);
        (sim, rules, actor)
    }

    pub(crate) fn close_walk_input_test_bridge(&mut self, rules: &RuleSet) {
        let terrain = self.resolved_terrain.as_mut().unwrap();
        for x in 8..=10 {
            let cell = terrain
                .cells
                .iter_mut()
                .find(|c| c.rx == x && c.ry == 6)
                .unwrap();
            cell.bridge_facts.raw_flags = 0;
            cell.has_bridge_deck = false;
            cell.bridge_walkable = false;
            cell.bridge_transition = false;
        }
        self.real_cell_bridge_flags_0x1180 = terrain.capture_real_cell_bridge_flags_0x1180();
        assert!(self.rebuild_dynamic_navigation(rules));
    }
}
