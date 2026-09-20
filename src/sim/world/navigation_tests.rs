//! Production navigation reconstruction must respect Mark-owned cell presence.

use super::{empty_heights, gsi_04_10_clear_terrain, make_test_entity};
use crate::map::entities::EntityCategory;
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::overlay_grid::OverlayGrid;
use crate::sim::production::{ProductionCategory, enqueue_by_type};
use crate::sim::snapshot::GameSnapshot;
use crate::sim::world::{Simulation, TickLane};
use std::collections::BTreeMap;

#[test]
fn native_bridge_record_geometry_changes_rebuild_and_restore_navigation() {
    use crate::map::resolved_terrain::zone_class;
    use crate::rules::locomotor_type::MovementZone;
    use crate::sim::bridge_state::{BridgeEndpointRecord, BridgeRecordKind, BridgeRuntimeState};
    use crate::sim::movement::locomotor::MovementLayer;
    let (rules, _) = rules_and_overlays();
    let mut sim = Simulation::with_seed(0x56c510);
    let mut terrain = gsi_04_10_clear_terrain(16, 16);
    for y in 0..16 {
        let cell = terrain.cell_mut(8, y).unwrap();
        cell.zone_type = zone_class::OUTSIDE;
        cell.ground_walk_blocked = true;
        cell.base_ground_walk_blocked = true;
    }
    let reachable = |sim: &Simulation| {
        sim.zone_grid.as_ref().unwrap().can_reach(
            MovementZone::Normal,
            (5, 5),
            MovementLayer::Ground,
            (12, 5),
            MovementLayer::Ground,
        )
    };
    sim.install_resolved_terrain_for_new_map(terrain.clone());
    sim.bridge_state = Some(BridgeRuntimeState::from_resolved_terrain_with_map_size(
        &terrain,
        true,
        300,
        (8, 8),
    ));
    assert!(sim.rebuild_dynamic_navigation(&rules));
    assert!(
        !reachable(&sim),
        "the outside-class strip separates two ground regions"
    );
    let path = sim.path_grid_snapshot().unwrap();
    let record = BridgeEndpointRecord {
        endpoint_a: (5, 5),
        endpoint_b: (26, 0),
        group_id: 0,
        active: true,
        bridge_kind: BridgeRecordKind::Low,
    };
    // Side17 maps this out-of-rectangle endpoint to (9,1), across the strip.
    sim.bridge_state
        .as_mut()
        .unwrap()
        .test_set_endpoint_records(vec![record]);
    sim.rebuild_zone_grid(&path);
    assert!(
        reachable(&sim),
        "a record-only change must join the regions"
    );
    assert!(
        path.diff_cells(&sim.path_grid_snapshot().unwrap())
            .unwrap()
            .is_empty()
    );

    let hash_before = sim.state_hash();
    let mut replacement =
        BridgeRuntimeState::from_resolved_terrain_with_map_size(&terrain, true, 300, (8, 10));
    replacement.test_set_endpoint_records(vec![record]);
    sim.bridge_state = Some(replacement);
    assert_ne!(hash_before, sim.state_hash());
    sim.rebuild_zone_grid(&path);
    // Side19 maps the identical endpoint to (7,1), on the source side.
    assert!(
        !reachable(&sim),
        "geometry-only change must remove the connection"
    );

    let bytes = GameSnapshot::save(&sim, 0, 0, "native-bridge-records.map", 0);
    let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
    restored.restore_after_snapshot_load().unwrap();
    assert_eq!(
        restored
            .bridge_state
            .as_ref()
            .unwrap()
            .native_zone_source_size(),
        Some((8, 10))
    );
    assert_eq!(
        restored.bridge_state.as_ref().unwrap().endpoint_records(),
        &[record]
    );
    restored.install_resolved_terrain_for_new_map(terrain.clone());
    assert!(restored.rebuild_dynamic_navigation(&rules));
    assert!(
        !reachable(&restored),
        "restore must retain the native source geometry"
    );

    let mut replacement =
        BridgeRuntimeState::from_resolved_terrain_with_map_size(&terrain, true, 300, (8, 8));
    replacement.test_set_endpoint_records(vec![record]);
    restored.bridge_state = Some(replacement);
    restored.rebuild_zone_grid(&path);
    assert!(reachable(&restored));
    let mut inactive = record;
    inactive.active = false;
    restored
        .bridge_state
        .as_mut()
        .unwrap()
        .test_set_endpoint_records(vec![inactive]);
    restored.rebuild_zone_grid(&path);
    assert!(
        !reachable(&restored),
        "activity-only change must remove the connection"
    );
}

fn rules_and_overlays() -> (RuleSet, OverlayTypeRegistry) {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
         [BuildingTypes]\n0=YARD\n1=HELD\n2=UPGRADE\n\
         [YARD]\nStrength=500\nFoundation=2x2\nBib=yes\nFactory=BuildingType\n\
         [HELD]\nStrength=300\nCost=100\nTechLevel=1\nOwner=Americans\nFoundation=3x3\n\
         [UPGRADE]\nStrength=100\nFoundation=4x4\n\
         [OverlayTypes]\n0=ROAD\n[ROAD]\nLand=Road\n\
         [Road]\nFoot=37%\nTrack=100%\n",
    );
    (
        RuleSet::from_ini(&ini).unwrap(),
        OverlayTypeRegistry::from_ini(&ini, None),
    )
}

#[test]
fn recalc_keeps_marked_structure_over_partial_terrain_occupation_and_bridge_deck() {
    use crate::sim::movement::locomotor::MovementLayer;
    let (rules, _) = rules_and_overlays();
    let mut sim = Simulation::with_seed(47);
    let mut terrain = gsi_04_10_clear_terrain(16, 16);
    let tree_cell = terrain.cell_mut(6, 6).unwrap();
    tree_cell.terrain_object_occupation = Some(1);
    tree_cell.terrain_object_blocks = true;
    let ground = crate::sim::pathfinding::PathGrid::from_resolved_terrain(&terrain);
    assert!(
        ground.is_walkable_for_infantry(6, 6),
        "partial tree leaves infantry room"
    );
    sim.install_resolved_terrain_for_new_map(terrain);
    let mut structure =
        crate::sim::game_entity::GameEntity::test_default(47, "YARD", "Americans", 6, 6);
    structure.category = EntityCategory::Structure;
    structure.type_ref = sim.interner.intern("YARD");
    structure.lifecycle.cell_marked = true;
    structure.dying = true; // Dying still owns its marked footprint.
    sim.substrate.entities.insert(structure);
    assert!(sim.rebuild_dynamic_navigation(&rules));
    assert!(!sim.path_grid().unwrap().is_walkable_for_infantry(6, 6));
    assert!(
        sim.path_grid().unwrap().is_walkable(7, 6),
        "bib remains open"
    );
    let before = sim.path_grid_snapshot().unwrap();
    let terrain = sim.resolved_terrain.as_mut().unwrap();
    let cell = terrain.cell_mut(6, 6).unwrap();
    cell.level = 4;
    cell.slope_type = 1;
    cell.bridge_deck_level = 8;
    cell.has_bridge_deck = true;
    cell.bridge_walkable = true;
    cell.bridge_transition = true;
    crate::sim::world::navigation::NavigationCaches {
        terrain_costs: &mut sim.terrain_costs,
        zones: &mut sim.zone_grid,
        path: &mut sim.path_grid,
        playfield_bounds: sim.playfield_bounds,
    }
    .publish_recalculated_cell(
        terrain,
        None,
        &sim.substrate.entities,
        &sim.interner,
        &rules,
        (6, 6),
    )
    .unwrap();
    let after = sim.path_grid().unwrap();
    assert!(!after.is_walkable(6, 6));
    assert!(!after.is_walkable_for_infantry(6, 6));
    assert!(after.is_walkable_on_layer(6, 6, MovementLayer::Bridge));
    assert_eq!(after.cell(6, 6).unwrap().ground_level, 4);
    assert_eq!(after.cell(6, 6).unwrap().slope_type, 1);
    assert_eq!(after.cell(6, 6).unwrap().bridge_deck_level, 8);
    assert_eq!(after.cell(7, 6), before.cell(7, 6));
    assert_eq!(before.cell(6, 6).unwrap().ground_level, 0);
}

fn assert_only_marked_foundation(sim: &Simulation) {
    let path = sim.path_grid_snapshot().unwrap();
    for ry in 0..3 {
        for rx in 0..3 {
            assert!(
                path.is_walkable(rx, ry),
                "held factory coordinate has no footprint"
            );
        }
    }
    assert!(!path.is_walkable(6, 6), "marked parent remains a blocker");
    assert!(!path.is_walkable(6, 7));
    assert!(
        path.is_walkable(7, 6),
        "the parent's HasBib relaxation is preserved"
    );
    assert!(path.is_walkable(7, 7));
    for (rx, ry) in [(8, 6), (9, 6), (6, 8), (6, 9), (9, 9)] {
        assert!(
            path.is_walkable(rx, ry),
            "unmarked upgrade adds no foundation at {rx},{ry}"
        );
    }
}

fn assert_retained_roles(sim: &Simulation, parent_id: u64, upgrade_id: u64, held_id: u64) {
    assert!(
        sim.substrate
            .entities
            .get(parent_id)
            .unwrap()
            .lifecycle
            .cell_marked
    );
    let upgrade = sim.substrate.entities.get(upgrade_id).unwrap();
    assert!(!upgrade.lifecycle.in_limbo && !upgrade.lifecycle.cell_marked);
    assert!(
        upgrade
            .structure_upgrade_link
            .is_some_and(|link| link.parent_stable_id == parent_id)
    );
    let held = sim.substrate.entities.get(held_id).unwrap();
    assert!(held.lifecycle.in_limbo && !held.lifecycle.cell_marked);
    assert_eq!(
        sim.production
            .factory_shadow
            .view(held.owner(), ProductionCategory::Building)
            .unwrap()
            .object
            .as_ref()
            .unwrap()
            .entity_id,
        Some(held_id)
    );
}

#[test]
fn held_factory_and_attached_upgrade_stay_off_navigation_through_frame_and_restore() {
    let (rules, overlays) = rules_and_overlays();
    let mut sim = Simulation::with_seed(0x4D41_524B);
    sim.session.map_width = 16;
    sim.session.map_height = 16;
    sim.session.game_options.crates = false;
    sim.production.ore_growth_config.grows = false;
    sim.production.ore_growth_config.spreads = false;
    let terrain = gsi_04_10_clear_terrain(16, 16);
    sim.install_resolved_terrain_for_new_map(terrain.clone());
    let mut overlay_grid = OverlayGrid::from_overlay_entries(&[], 16, 16);
    overlay_grid.retain_zero_wall_plane_for_tests();
    sim.overlay_grid = Some(overlay_grid);
    sim.intern_rule_type_ids(&rules);
    let owner = sim.interner.intern("Americans");
    sim.houses.insert(
        owner,
        crate::sim::house_state::HouseState::new(owner, 0, None, true, 50_000, 10),
    );
    let mut yard = make_test_entity("YARD", EntityCategory::Structure);
    yard.cell_x = 6;
    yard.cell_y = 6;
    yard.structure_upgrades = [Some("UPGRADE".to_owned()), None, None];
    assert_eq!(
        sim.spawn_from_map(&[yard], Some(&rules), &empty_heights()),
        2
    );
    sim.resolve_type_handles(&rules);
    let parent_id = sim
        .substrate
        .entities
        .values()
        .find(|entity| entity.structure_upgrade_link.is_none())
        .unwrap()
        .stable_id();
    let upgrade = sim
        .substrate
        .entities
        .values()
        .find(|entity| entity.structure_upgrade_link.is_some())
        .unwrap();
    assert!(!upgrade.lifecycle.in_limbo && !upgrade.lifecycle.cell_marked);
    let upgrade_id = upgrade.stable_id();
    assert!(enqueue_by_type(&mut sim, &rules, "Americans", "HELD"));
    let held_id = sim
        .production
        .factory_shadow
        .view(owner, ProductionCategory::Building)
        .unwrap()
        .object
        .as_ref()
        .unwrap()
        .entity_id
        .unwrap();
    let held = sim.substrate.entities.get(held_id).unwrap();
    assert!(held.lifecycle.in_limbo && !held.lifecycle.cell_marked);
    assert_eq!((held.position.rx, held.position.ry), (0, 0));

    assert!(sim.rebuild_dynamic_navigation(&rules));
    assert_only_marked_foundation(&sim);
    assert_retained_roles(&sim, parent_id, upgrade_id, held_id);
    let before = sim.path_grid_snapshot().unwrap();
    // A real dirty-overlay frame triggers the same canonical rebuild used by
    // ordinary terrain mutation, independent of the held building's location.
    sim.overlay_grid
        .as_mut()
        .unwrap()
        .place_overlay(12, 12, 0, 0);
    let output = sim
        .advance_app_frame(
            &[],
            Some(&rules),
            &BTreeMap::new(),
            Some(&overlays),
            67,
            TickLane::Ordinary,
            None,
        )
        .expect("fixture frame must complete");
    assert_eq!(output.overlay_updates.len(), 1);
    assert_eq!(
        (output.overlay_updates[0].rx, output.overlay_updates[0].ry),
        (12, 12)
    );
    assert!(!std::sync::Arc::ptr_eq(
        &before,
        &sim.path_grid_snapshot().unwrap()
    ));
    assert_only_marked_foundation(&sim);
    assert_retained_roles(&sim, parent_id, upgrade_id, held_id);

    let bytes = GameSnapshot::save(&sim, 0, 0, "marked-navigation.map", 0);
    let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
    restored.restore_after_snapshot_load().unwrap();
    restored.rebuild_caches_after_load(terrain, Default::default(), Vec::new(), Vec::new());
    restored
        .restore_map_authority_after_snapshot_load(&rules, &overlays)
        .unwrap();
    assert!(
        restored
            .substrate
            .entities
            .get(held_id)
            .unwrap()
            .lifecycle
            .in_limbo
    );
    assert_only_marked_foundation(&restored);
    assert_retained_roles(&restored, parent_id, upgrade_id, held_id);

    // Death is distinct from cell unlinking. Static navigation keeps a marked
    // footprint until the real lifecycle owner removes it.
    restored
        .substrate
        .entities
        .get_mut(parent_id)
        .unwrap()
        .dying = true;
    assert!(restored.rebuild_dynamic_navigation(&rules));
    assert_only_marked_foundation(&restored);
    restored.uninit_with_rules(parent_id, &rules);
    assert!(
        !restored
            .substrate
            .entities
            .get(parent_id)
            .unwrap()
            .lifecycle
            .cell_marked
    );
    assert!(restored.rebuild_dynamic_navigation(&rules));
    assert!(restored.path_grid_snapshot().unwrap().is_walkable(6, 6));
}
