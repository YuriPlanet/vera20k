#[test]
fn tube_hierarchy_restore_prepares_detached_dummy_then_publishes_terminal_fields() {
    use crate::map::tube_facts::TubeFact;
    use crate::map::tubes::{ConstructedMapTube, NativeMapTubeReceipt, TubeNativeInit};
    use crate::rules::locomotor_type::MovementZone;
    use crate::sim::bridge_state::BridgeRuntimeState;
    use crate::sim::pathfinding::zone_hierarchy::{
        ZonePrecheckExclusions, ZonePrecheckOutcome, zone_precheck_flat,
    };
    let rules = load_fixture_rules();
    let registry = OverlayTypeRegistry::empty();
    let cells = (0..32)
        .flat_map(|y| {
            (0..32).map(move |x| {
                let mut c = compatibility_snapshot_cell(0);
                c.rx = x;
                c.ry = y;
                if x == 16 {
                    c.outside_playfield = true;
                    c.zone_type = crate::map::resolved_terrain::zone_class::OUTSIDE;
                }
                if [(1, 16), (2, 16), (3, 16)].contains(&(x, y)) {
                    c.yr_cell_land_type = 10;
                    c.base_yr_cell_land_type = 10;
                }
                c
            })
        })
        .collect();
    let mut template = ResolvedTerrainGrid::from_cells(32, 32, cells);
    template.test_set_native_allocated_cells(
        &(0..32)
            .flat_map(|y| (0..32).map(move |x| (x, y)))
            .filter(|c| ![(1, 15)].contains(c))
            .collect::<Vec<_>>(),
    );
    template
        .bind_native_map_tubes(NativeMapTubeReceipt {
            entries: [
                TubeFact::explicit((2, 16), (24, 16), 1, vec![2; 22]),
                TubeFact::explicit((3, 17), (24, 16), 0, vec![8]),
                TubeFact::explicit((1, 16), (1, 16), 0, vec![]),
                TubeFact::explicit((3, 16), (3, 16), 0, vec![]),
                TubeFact::explicit((65535, 0), (24, 16), 0, vec![8]),
            ]
            .into_iter()
            .enumerate()
            .map(|(i, fact)| ConstructedMapTube {
                fact,
                native_init: TubeNativeInit {
                    source_entry_ordinal: i,
                    native_unique_id: 100 + i as i32,
                },
            })
            .collect(),
        })
        .unwrap();
    let bridges =
        BridgeRuntimeState::from_resolved_terrain_with_map_size(&template, true, 100, (16, 16));
    assert_eq!(bridges.endpoint_records().len(), 1);
    let make_sim = || {
        let mut sim = load_fixture_simulation(false);
        sim.overlay_grid = Some(OverlayGrid::new(32, 32));
        sim.install_resolved_terrain_for_new_map(template.clone());
        sim.bridge_state = Some(bridges.clone());
        assert!(sim.rebuild_dynamic_navigation(&rules));
        sim
    };
    let connected = |sim: &Simulation| {
        let h = sim
            .zone_grid
            .as_ref()
            .unwrap()
            .hierarchy_for(MovementZone::Normal)
            .unwrap();
        let l = h.level(0).unwrap();
        matches!(
            zone_precheck_flat(
                h,
                l.zone_at(2, 16),
                l.zone_at(24, 16),
                MovementZone::Normal,
                &ZonePrecheckExclusions::default(),
                None,
            )
            .unwrap(),
            ZonePrecheckOutcome::Passed(_)
        )
    };
    let current = make_sim();
    assert!(
        connected(&current),
        "the live retained Tube enables the missing-side lookup"
    );
    let bytes = snapshot_bytes(&current, &rules);
    let live = current.effective_shared_cell_dummy();
    live.set_level_slope(-7, 11);
    live.stamp_coord(123, 456);
    live.write_overlay_identity_state(29, 33);
    let before = (
        live.snapshot(),
        live.overlay_identity_state(),
        live.raw_tube_index(),
        current.state_hash(),
    );
    let mut failing = make_sim();
    // A malformed presentation owner fails after map/hierarchy preparation.
    // FootLoad now correctly resets transient audio state, so it cannot be
    // used as a fabricated late sound-identity failure.
    let missing_owner = failing.interner.intern("MISSING");
    let sidebar = crate::sim::snapshot::SavedSidebarOrder::new(
        missing_owner,
        std::array::from_fn(|_| Vec::new()),
        [0; 4],
    );
    let failing_bytes = GameSnapshot::save_validated_with_presentation(
        &failing,
        LOAD_FIXTURE_MAP_HASH,
        rules.simulation_config_hash(),
        "late restore failure",
        0,
        &[],
        Some(&sidebar),
    );
    // The independent fixture shares the immutable template's dummy; restore
    // the live witness after fixture setup, before testing the transaction.
    live.set_level_slope(-7, 11);
    live.stamp_coord(123, 456);
    live.write_overlay_identity_state(29, 33);
    let failure = PreparedLoad::prepare_candidate(
        &failing_bytes,
        Some(&current),
        Some(LOAD_FIXTURE_MAP_HASH),
        Some(&rules),
        Some(&template),
        Some(&registry),
    );
    assert!(
        matches!(failure, Err(PreparedLoadError::SidebarOrder(_))),
        "must fail after map/hierarchy preparation"
    );
    assert_eq!(
        (
            live.snapshot(),
            live.overlay_identity_state(),
            live.raw_tube_index(),
            current.state_hash()
        ),
        before,
        "rejected candidate must not write any retained field"
    );
    let prepared = PreparedLoad::prepare_candidate(
        &bytes,
        Some(&current),
        Some(LOAD_FIXTURE_MAP_HASH),
        Some(&rules),
        Some(&template),
        Some(&registry),
    )
    .unwrap();
    assert!(
        !connected(&prepared.simulation),
        "native Resize Tube=-1 must affect the prepared hierarchy"
    );
    let prepared_dummy = prepared.simulation.effective_shared_cell_dummy();
    assert!(!prepared_dummy.same_identity(&live));
    assert_eq!(prepared_dummy.raw_tube_index(), -1);
    assert_ne!(
        prepared_dummy.snapshot().coord,
        (0, 0),
        "hierarchy misses survive the constructor reset"
    );
    let expected = (
        prepared_dummy.snapshot(),
        prepared_dummy.overlay_identity_state(),
    );
    let mut runtime = crate::sim::runtime::SimRuntime::from_simulation(current);
    prepared.commit_into(&mut runtime);
    assert!(
        runtime
            .simulation
            .effective_shared_cell_dummy()
            .same_identity(&live)
    );
    assert_eq!((live.snapshot(), live.overlay_identity_state()), expected);
    assert_eq!(live.raw_tube_index(), -1);
    assert!(!connected(&runtime.simulation));
    assert_eq!(
        runtime.simulation.substrate.base_reservations.dummy_mask(),
        0
    );
}
