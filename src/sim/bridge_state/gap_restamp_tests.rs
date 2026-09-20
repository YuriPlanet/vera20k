fn gap_fixture(case: &serde_json::Value) -> (ResolvedTerrainGrid, Vec<BridgeEndpointRecord>) {
    let int = |v: &serde_json::Value| v.as_i64().unwrap();
    let base = make_bridge_terrain().cell(4, 0).unwrap().clone();
    let mut allocated = Vec::new();
    let mut cells = Vec::new();
    for y in 0..12u16 {
        for x in 0..12u16 {
            let mut cell = base.clone();
            cell.rx = x;
            cell.ry = y;
            cell.bridge_facts.raw_flags = case["default"].as_u64().unwrap() as u32;
            for row in case["cells"].as_array().unwrap() {
                if (int(&row[0]), int(&row[1])) == (i64::from(x), i64::from(y)) {
                    cell.bridge_facts.raw_flags = row[2].as_u64().unwrap() as u32;
                }
            }
            if !case["holes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| (int(&p[0]), int(&p[1])) == (i64::from(x), i64::from(y)))
            {
                allocated.push((x, y));
            }
            cells.push(cell);
        }
    }
    let mut terrain = ResolvedTerrainGrid::from_cells(12, 12, cells);
    terrain.test_set_native_allocated_cells(&allocated);
    terrain
        .shared_cell_dummy()
        .test_set_retained_bridge_flags(case["dummy"].as_u64().unwrap() as u32);
    terrain.stamp_dummy_cell_requested_coord(1234, -2345);
    let records = case["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| BridgeEndpointRecord {
            endpoint_a: (int(&r[0]) as u16, int(&r[1]) as u16),
            endpoint_b: (int(&r[2]) as u16, int(&r[3]) as u16),
            group_id: 0,
            active: int(&r[4]) != 0,
            bridge_kind: if int(&r[5]) == 0 {
                BridgeRecordKind::High
            } else {
                BridgeRecordKind::Low
            },
        })
        .collect();
    (terrain, records)
}

#[test]
fn native_gap_restamp_matches_order_bits_aliases_and_retained_dummy() {
    let corpus: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/bridge_gap_flags.json"
    ))
    .unwrap();
    assert_eq!(corpus["cases"].as_array().unwrap().len(), 20);
    for case in corpus["cases"].as_array().unwrap() {
        let (mut terrain, records) = gap_fixture(case);
        let dummy = terrain.shared_cell_dummy();
        let mut writes = Vec::new();
        super::gap_restamp::restamp_inactive_high_records(
            &mut terrain,
            &records,
            |_, index, flags| {
                writes.push(serde_json::json!({"target":index.map(|i|[i%12,i/12]),
                "dummy_coord":if index.is_none(){Some(dummy.snapshot().coord)}else{None},"flags":flags}));
            },
        );
        assert_eq!(
            serde_json::json!(writes),
            case["writes"],
            "{}: ordered writes",
            case["name"]
        );
        let final_flags: Vec<_> = terrain
            .iter()
            .map(|c| serde_json::json!([c.rx, c.ry, c.bridge_facts.raw_flags]))
            .collect();
        assert_eq!(
            serde_json::json!(final_flags),
            case["final_flags"],
            "{}: real flags",
            case["name"]
        );
        assert_eq!(
            serde_json::json!(dummy.snapshot().coord),
            case["dummy_coord"],
            "{}: dummy coord",
            case["name"]
        );
        assert_eq!(
            serde_json::json!(dummy.retained_bridge_flags()),
            case["dummy_flags"],
            "{}: dummy flags",
            case["name"]
        );
    }
}

#[test]
fn gap_flags_snapshot_hash_and_later_setter_preserve_value_authority() {
    use crate::sim::snapshot::GameSnapshot;
    use crate::sim::world::Simulation;
    let corpus: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/bridge_gap_flags.json"
    ))
    .unwrap();
    let (terrain, records) = gap_fixture(&corpus["cases"][1]);
    let pristine = terrain.clone();
    let mut sim = Simulation::new();
    let mut bridge_state = super::BridgeRuntimeState::from_resolved_terrain(&terrain, true, 100);
    bridge_state.test_set_endpoint_records(records.clone());
    sim.bridge_state = Some(bridge_state);
    sim.overlay_grid =
        Some(crate::sim::overlay_grid::OverlayGrid::new_with_retained_wall_plane(12, 12));
    sim.install_resolved_terrain_for_new_map(terrain);
    let before = sim.state_hash();
    let updates = &mut sim.real_cell_bridge_flags_0x1180;
    super::gap_restamp::restamp_inactive_high_records(
        sim.resolved_terrain.as_mut().unwrap(),
        &records,
        |_, index, flags| {
            if let Some(index) = index {
                updates.set_allocated_cell(index, flags);
            }
        },
    );
    assert_ne!(
        sim.state_hash(),
        before,
        "real gap state affects future placement"
    );
    assert_eq!(
        sim.resolved_terrain
            .as_ref()
            .unwrap()
            .cell(4, 3)
            .unwrap()
            .bridge_flags()
            & 0xC00,
        0xC00
    );
    // A saved zero must also win over reconstruction; restore cannot just replay
    // fresh-load records. Keep an inactive record but erase one saved cell's gap.
    let index = 3 * 12 + 4;
    sim.resolved_terrain
        .as_mut()
        .unwrap()
        .cell_mut(4, 3)
        .unwrap()
        .bridge_facts
        .raw_flags &= !0xC00;
    sim.real_cell_bridge_flags_0x1180
        .set_allocated_cell(index, 0x80);
    let bytes = GameSnapshot::save(&sim, 0, 0, "gap.map", 0);
    let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
    restored.restore_after_snapshot_load().unwrap();
    restored.rebuild_caches_after_load(
        pristine,
        crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
        Vec::new(),
        Vec::new(),
    );
    let ini = crate::rules::ini_parser::IniFile::from_str(
        "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n[OverlayTypes]\n",
    );
    let rules = crate::rules::ruleset::RuleSet::from_ini(&ini).unwrap();
    restored
        .restore_map_authority_after_snapshot_load(
            &rules,
            &crate::map::overlay_types::OverlayTypeRegistry::empty(),
        )
        .unwrap();
    assert_eq!(
        restored
            .resolved_terrain
            .as_ref()
            .unwrap()
            .cell(4, 3)
            .unwrap()
            .bridge_flags()
            & 0xC00,
        0
    );
    assert_eq!(
        restored
            .resolved_terrain
            .as_ref()
            .unwrap()
            .cell(5, 3)
            .unwrap()
            .bridge_flags()
            & 0xC00,
        0xC00
    );
    let dummy = restored.shared_cell_dummy.clone();
    let clean = restored.state_hash();
    dummy.test_set_retained_bridge_flags(0xC00);
    assert_ne!(restored.state_hash(), clean);
    let identity = dummy.clone();
    dummy.reconstruct_for_map_resize();
    assert!(dummy.same_identity(&identity));
    assert_eq!(dummy.retained_bridge_flags(), 0);
    // A later native setter owns400/800 too; retaining them cannot prevent
    // intact Mark from clearing400 and selecting its direction bit.
    restored.apply_runtime_bridge_flag_stamp(crate::map::bridge_facts::BridgeFlagStamp::new(
        (5, 3),
        6,
        true,
    ));
    assert_eq!(
        restored
            .resolved_terrain
            .as_ref()
            .unwrap()
            .cell(5, 3)
            .unwrap()
            .bridge_flags()
            & 0xC00,
        0
    );
    assert_eq!(
        restored.real_cell_bridge_flags_0x1180,
        restored
            .resolved_terrain
            .as_ref()
            .unwrap()
            .capture_real_cell_bridge_flags_0x1180()
    );
}

#[test]
fn gap_retention_native_setter_anchor_flags_keep_direction_independent_of_destroy() {
    use crate::map::bridge_facts::{
        BridgeAnchorRelation, BridgeCellFacts, BridgeStampFamily, BridgeStampSlot,
        RETAINED_CELLCLASS_BRIDGE_FLAG_MASK, apply_bridge_fact_slot,
        apply_retained_cellclass_bridge_slot,
    };
    let corpus: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/bridge_gap_flags.json"
    ))
    .unwrap();
    let cases = corpus["setter_prefixes"].as_array().unwrap();
    assert_eq!(cases.len(), 48);
    for case in cases {
        let initial = case["initial"].as_u64().unwrap() as u32;
        let expected = case["flags"].as_u64().unwrap() as u32;
        let direction = case["direction"].as_u64().unwrap() as u8;
        let set = case["set"].as_u64().unwrap() != 0;
        let mut retained = initial;
        apply_retained_cellclass_bridge_slot(
            &mut retained,
            BridgeStampSlot::Anchor,
            set,
            direction,
        );
        assert_eq!(
            retained & RETAINED_CELLCLASS_BRIDGE_FLAG_MASK,
            expected & RETAINED_CELLCLASS_BRIDGE_FLAG_MASK,
            "{case}"
        );
        let mut facts = BridgeCellFacts {
            raw_flags: initial,
            ..Default::default()
        };
        apply_bridge_fact_slot(
            &mut facts,
            BridgeStampSlot::Anchor,
            BridgeAnchorRelation {
                anchor: (4, 4),
                slot: BridgeStampSlot::Anchor,
                family: if case["family"] == "nesw" {
                    BridgeStampFamily::Nesw
                } else {
                    BridgeStampFamily::Nwse
                },
                direction,
            },
            set,
        );
        assert_eq!(facts.raw_flags, expected, "full fact word: {case}");
    }
}
