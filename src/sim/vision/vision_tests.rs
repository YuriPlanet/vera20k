//! Tests for the fog/shroud visibility system.

use super::*;
use crate::map::entities::EntityCategory;
use crate::sim::components::Health;
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern;

fn spawn_with_vision(store: &mut EntityStore, id: u64, owner: &str, rx: u16, ry: u16, range: u16) {
    let mut entity = GameEntity::new_at_frame_zero_for_test(
        id,
        rx,
        ry,
        0,
        0,
        intern::test_intern(owner),
        Health { current: 100 },
        intern::test_intern("E1"),
        EntityCategory::Infantry,
        0,
        range,
        false,
    );
    entity.lifecycle.in_limbo = false;
    store.insert(entity);
}

fn ti() -> intern::StringInterner {
    intern::test_interner()
}

/// Helper: default VisionConfig for tests.
fn default_config() -> VisionConfig {
    VisionConfig::default()
}

// -- Flat grid unit tests --

#[test]
fn test_owner_visibility_basic() {
    let mut vis = OwnerVisibility::new(10, 10);
    assert!(!vis.is_visible(3, 3));
    assert!(!vis.is_revealed(3, 3));

    vis.mark_visible(3, 3);
    assert!(vis.is_visible(3, 3));
    assert!(vis.is_revealed(3, 3));

    // Out of bounds returns false.
    assert!(!vis.is_visible(10, 0));
    assert!(!vis.is_revealed(0, 10));
}

#[test]
fn cell_visibility_shroud_counter_preserves_native_sentinel_edges() {
    let mut cell = CellVisibilityRuntime {
        gap_shroud_counter: 3,
        ..CellVisibilityRuntime::default()
    };

    cell.increase_shroud_counter();
    assert_eq!(cell.shroud_counter, 1, "-1 normalizes through zero to one");
    assert_ne!(cell.flags & 0x20, 0, "positive transition sets flag 0x20");
    cell.increase_shroud_counter();
    cell.increase_shroud_counter();
    cell.increase_shroud_counter();
    assert_eq!(cell.shroud_counter, 3, "gap counter is an upper clamp");

    cell.reduce_shroud_counter();
    cell.reduce_shroud_counter();
    assert_eq!(cell.shroud_counter, 1);
    cell.reduce_shroud_counter();
    assert_eq!(
        cell.shroud_counter, -1,
        "native reduce maps one through zero to -1"
    );
    assert_eq!(
        cell.alt_flags & 0x18,
        0x18,
        "closed ground flags reopen at nonpositive"
    );
}

#[test]
fn cell_visibility_map_orders_redraw_reveal_then_clean_fog() {
    let mut cell = CellVisibilityRuntime {
        flags: 0x40 | 0x400000,
        ..CellVisibilityRuntime::default()
    };
    let mut events = Vec::new();

    cell.map_visible(true, |event| events.push(event));

    assert_eq!(
        events,
        vec![
            CellVisibilityEvent::RegisterCellAsVisible,
            CellVisibilityEvent::RevealCheck,
            CellVisibilityEvent::CleanFog,
        ]
    );
    assert_eq!(cell.flags & 0x42, 0x02, "map installs 0x02 and clears 0x40");
    assert_eq!(cell.alt_flags & 0x18, 0x18);
    assert_eq!((cell.visibility, cell.foggedness), (-1, -1));
    assert_eq!(
        cell.flags & 0x400000,
        0,
        "CleanFog clears the snapshot flag"
    );

    let mut repeat = Vec::new();
    cell.map_visible(true, |event| repeat.push(event));
    assert_eq!(
        repeat,
        vec![
            CellVisibilityEvent::RegisterCellAsVisible,
            CellVisibilityEvent::RevealCheck,
        ],
        "a later map update may redraw, but CleanFog is first-transition only"
    );
}

#[test]
fn fogged_footprints_unlink_shared_ids_in_reverse_order() {
    let viewer = intern::test_intern("Americans");
    let mut fog = FogState {
        width: 8,
        height: 8,
        ..FogState::default()
    };
    let first = fog.insert_fogged_object_footprint(viewer, (2, 2), 11, vec![(2, 2), (3, 2)]);
    let second = fog.insert_fogged_object_footprint(viewer, (2, 2), 22, vec![(2, 2)]);

    assert_eq!(fog.fogged_object_cells[&(viewer, 2, 2)], [first, second]);
    let removed = fog.clear_fogged_objects_at(viewer, 2, 2);
    assert_eq!(
        removed
            .iter()
            .map(|record| record.source_entity_id)
            .collect::<Vec<_>>(),
        vec![22, 11]
    );
    assert!(fog.fogged_objects.is_empty());
    assert_eq!(fog.fogged_object_cells.get(&(viewer, 3, 2)), Some(&vec![]));
    assert!(!fog.fogged_object_cells.contains_key(&(viewer, 2, 2)));
}

#[test]
fn sensor_house_counters_drive_cloaked_draw_without_observer_bypass() {
    let detector = intern::test_intern("Americans");
    let object_owner = intern::test_intern("Soviet");
    let mut fog = FogState {
        width: 8,
        height: 8,
        ..FogState::default()
    };

    let touched = fog.sensors_add_at(detector, (3, 3), 2);
    assert_eq!(
        touched,
        vec![
            (2, 2),
            (3, 2),
            (4, 2),
            (2, 3),
            (3, 3),
            (4, 3),
            (2, 4),
            (3, 4),
            (4, 4),
        ]
    );
    assert!(fog.set_cloaked_by_house(4, 3, 3));
    assert!(!fog.set_cloaked_by_house(36, 3, 3));
    assert!(!fog.draw_objects_cloaked(Some(detector), object_owner, 4, 3, 3));
    assert!(fog.draw_objects_cloaked(Some(object_owner), object_owner, 4, 3, 3));
    assert!(!fog.draw_objects_cloaked(None, object_owner, 4, 3, 3));

    fog.sensors_remove_at(detector, (3, 3), 2);
    assert!(fog.draw_objects_cloaked(Some(detector), object_owner, 4, 3, 3));
    assert!(fog.clear_cloaked_by_house(36, 3, 3));
    assert!(!fog.is_cloaked_by_house(4, 3, 3));
}

#[test]
fn owner_visibility_balances_each_frame_visibility_contributor() {
    let mut vis = OwnerVisibility::new(2, 2);
    vis.mark_visible(1, 1);
    vis.mark_visible(1, 1);
    assert_eq!(vis.cell_runtime_raw()[3].shroud_counter, 2);
    assert_eq!(vis.visibility_marks_raw()[3], 2);

    vis.clear_all_visible();

    assert_eq!(vis.cell_runtime_raw()[3].shroud_counter, -1);
    assert_eq!(vis.visibility_marks_raw()[3], 0);
    assert!(!vis.is_visible(1, 1));
    assert!(
        vis.is_revealed(1, 1),
        "counter cleanup never clears exploration"
    );
}

#[test]
fn test_merge_revealed_preserves_bits() {
    let mut old = OwnerVisibility::new(8, 8);
    old.mark_visible(2, 2);
    old.mark_visible(4, 4);

    // New grid has no revealed bits yet.
    let mut new = OwnerVisibility::new(8, 8);
    assert!(!new.is_revealed(2, 2));

    new.merge_revealed_from(&old);
    // Revealed bits carried over.
    assert!(new.is_revealed(2, 2));
    assert!(new.is_revealed(4, 4));
    // Visible bits were NOT carried (only revealed).
    assert!(!new.is_visible(2, 2));
}

#[test]
fn test_merge_revealed_different_dimensions() {
    let mut old = OwnerVisibility::new(10, 10);
    old.mark_visible(5, 5);

    let mut new = OwnerVisibility::new(8, 8);
    new.merge_revealed_from(&old);
    assert!(new.is_revealed(5, 5));

    // Cell (9,9) was in old but outside new's bounds — silently skipped.
    assert!(!new.is_revealed(9, 9));
}

// -- Recompute visibility integration tests --

#[test]
fn test_recompute_visibility_reveals_expected_cells() {
    let mut store = EntityStore::new();
    spawn_with_vision(&mut store, 1, "Americans", 5, 5, 2);

    let fog = recompute_owner_visibility(
        &store,
        Some(&PathGrid::new(16, 16)),
        &Default::default(),
        &default_config(),
        &ti(),
    );
    assert!(fog.is_cell_visible(intern::test_intern("Americans"), 5, 5));
    assert!(fog.is_cell_visible(intern::test_intern("Americans"), 7, 5));
    assert!(fog.is_cell_visible(intern::test_intern("Americans"), 5, 7));
    assert!(!fog.is_cell_visible(intern::test_intern("Americans"), 8, 5));
    assert!(fog.is_cell_revealed(intern::test_intern("Americans"), 6, 6));
}

#[test]
fn techno_playfield_stored_membership_gates_same_frame_los() {
    let mut store = EntityStore::new();
    spawn_with_vision(&mut store, 1, "Americans", 5, 5, 2);
    let config = VisionConfig {
        require_playfield_membership: true,
        ..default_config()
    };

    let suppressed = recompute_owner_visibility(
        &store,
        Some(&PathGrid::new(16, 16)),
        &Default::default(),
        &config,
        &ti(),
    );
    assert!(!suppressed.is_cell_visible(intern::test_intern("Americans"), 5, 5));

    store.get_mut(1).unwrap().in_playfield = true;
    let admitted = recompute_owner_visibility(
        &store,
        Some(&PathGrid::new(16, 16)),
        &Default::default(),
        &config,
        &ti(),
    );
    assert!(admitted.is_cell_visible(intern::test_intern("Americans"), 5, 5));
}

#[test]
fn test_recompute_visibility_clamps_to_grid_bounds() {
    let mut store = EntityStore::new();
    spawn_with_vision(&mut store, 1, "Americans", 0, 0, 4);

    let fog = recompute_owner_visibility(
        &store,
        Some(&PathGrid::new(3, 3)),
        &Default::default(),
        &default_config(),
        &ti(),
    );
    assert!(fog.is_cell_visible(intern::test_intern("Americans"), 0, 0));
    assert!(fog.is_cell_visible(intern::test_intern("Americans"), 2, 2));
    assert!(!fog.is_cell_visible(intern::test_intern("Americans"), 3, 0));
    assert_eq!(fog.width, 3);
    assert_eq!(fog.height, 3);
}

#[test]
fn test_recompute_visibility_tracks_multiple_owners() {
    let mut store = EntityStore::new();
    spawn_with_vision(&mut store, 1, "Americans", 2, 2, 1);
    spawn_with_vision(&mut store, 2, "Soviet", 10, 10, 1);

    let fog = recompute_owner_visibility(
        &store,
        Some(&PathGrid::new(16, 16)),
        &Default::default(),
        &default_config(),
        &ti(),
    );
    assert!(fog.is_cell_visible(intern::test_intern("Americans"), 2, 2));
    assert!(!fog.is_cell_visible(intern::test_intern("Americans"), 10, 10));
    assert!(fog.is_cell_visible(intern::test_intern("Soviet"), 10, 10));
}

#[test]
fn test_allied_visibility_is_shared() {
    // The real House registry interns every viewer before source admission.
    let alliance = intern::test_intern("Alliance");
    let mut store = EntityStore::new();
    spawn_with_vision(&mut store, 1, "Americans", 4, 4, 1);
    let mut alliances = HouseAllianceMap::new();
    alliances
        .entry("AMERICANS".to_string())
        .or_default()
        .insert("ALLIANCE".to_string());
    alliances
        .entry("ALLIANCE".to_string())
        .or_default()
        .insert("AMERICANS".to_string());

    let mut fog = recompute_owner_visibility(
        &store,
        Some(&PathGrid::new(16, 16)),
        &alliances,
        &default_config(),
        &ti(),
    );
    // Cache construction copies the already-published direct viewer knowledge.
    fog.build_merged_for(alliance, &ti());
    assert!(fog.is_cell_visible(intern::test_intern("Alliance"), 4, 4));
    assert!(fog.is_friendly("Alliance", "Americans"));
}

// -- Sight cap tests --

#[test]
fn test_sight_capped_at_max_range() {
    let mut store = EntityStore::new();
    // Spawn with sight=15, which exceeds MAX_SIGHT_RANGE (10).
    spawn_with_vision(&mut store, 1, "Americans", 20, 20, 15);

    let fog = recompute_owner_visibility(
        &store,
        Some(&PathGrid::new(50, 50)),
        &Default::default(),
        &default_config(),
        &ti(),
    );
    // Cell at distance 10 should be visible (exactly at MAX_SIGHT_RANGE).
    assert!(fog.is_cell_visible(intern::test_intern("Americans"), 30, 20));
    // Cell at distance 11 should NOT be visible (capped).
    assert!(!fog.is_cell_visible(intern::test_intern("Americans"), 31, 20));
}

/// `TechnoClass::UpdateReveal @ 0x0070AF50`: a `SIGHT`-ability veteran sees
/// `ftol(Sight * VeteranSight)`; the same veteran of a type without the
/// ability, or any rank under stock `VeteranSight=0.0`, keeps `Sight`.
#[test]
fn gsi_08_12_veteran_sight_multiplies_only_for_sight_ability_holders() {
    let rules =
        crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[InfantryTypes]\n0=E1\n1=E2\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
         [E1]\nStrength=125\nSight=5\nVeteranAbilities=SIGHT\n\
         [E2]\nStrength=125\nSight=5\nVeteranAbilities=STRONGER\n\
         [General]\nVeteranSight=1.5\n",
        ))
        .expect("sight fixture parses");
    let fog_for = |type_id: &str, veteran_sight: f64| {
        let mut store = EntityStore::new();
        let mut entity = GameEntity::new_at_frame_zero_for_test(
            1,
            10,
            10,
            0,
            0,
            intern::test_intern("Americans"),
            Health { current: 100 },
            intern::test_intern(type_id),
            EntityCategory::Infantry,
            100, // veteran
            5,   // vision_range
            false,
        );
        entity.lifecycle.in_limbo = false;
        store.insert(entity);
        let config = VisionConfig {
            require_playfield_membership: false,
            veteran_sight,
            leptons_per_sight_increase: 0,
            reveal_by_height: false,
            fog_of_war: false,
        };
        recompute_owner_visibility_with_rules(
            &store,
            Some(&PathGrid::new(30, 30)),
            &Default::default(),
            &config,
            &ti(),
            Some(&rules),
        )
    };
    let owner = intern::test_intern("Americans");
    // ftol(5 * 1.5) = 7: distance 7 visible, 8 not.
    let fog = fog_for("E1", 1.5);
    assert!(fog.is_cell_visible(owner, 17, 10));
    assert!(!fog.is_cell_visible(owner, 18, 10));
    // No SIGHT ability: plain 5.
    let fog = fog_for("E2", 1.5);
    assert!(fog.is_cell_visible(owner, 15, 10));
    assert!(!fog.is_cell_visible(owner, 16, 10));
    // Stock 0.0 disables the multiply rather than zeroing the sight.
    let fog = fog_for("E1", 0.0);
    assert!(fog.is_cell_visible(owner, 15, 10));
    assert!(!fog.is_cell_visible(owner, 16, 10));
}

/// A level-8 plateau buys no extra sight, because the engine measures elevation
/// in leptons of world height and multiplies rather than adding cells.
///
/// Level 8 is 8*104 = 832 leptons, and `trunc(832 / 2000)` is 0 steps, so the
/// multiplier is exactly 1.0. VERA used to convert the level with the *cells*
/// factor of 256 and add whole cells, handing every unit on high ground a free
/// ring of vision the engine never grants.
#[test]
fn elevation_grants_no_sight_bonus_at_any_reachable_terrain_level() {
    let mut store = EntityStore::new();
    let mut entity = GameEntity::new_at_frame_zero_for_test(
        1,
        10,
        10,
        8,
        0,
        intern::test_intern("Americans"),
        Health { current: 100 },
        intern::test_intern("E1"),
        EntityCategory::Infantry,
        0,
        5,
        false,
    );
    entity.lifecycle.in_limbo = false;
    store.insert(entity);
    let config = VisionConfig {
        require_playfield_membership: false,
        veteran_sight: 0.0,
        leptons_per_sight_increase: 2000,
        reveal_by_height: false,
        fog_of_war: false,
    };
    let fog = recompute_owner_visibility(
        &store,
        Some(&PathGrid::new(30, 30)),
        &Default::default(),
        &config,
        &ti(),
    );
    // Effective sight stays 5. The z=8 unit still shifts its reveal center by
    // 4 cells toward iso-north, so the foot cell (10,10) reveals around (6,6):
    // (11,6) is 5 east of the shifted center and visible, (12,6) is 6 and is not.
    assert!(fog.is_cell_visible(intern::test_intern("Americans"), 11, 6));
    assert!(
        !fog.is_cell_visible(intern::test_intern("Americans"), 12, 6),
        "level 8 must not buy a sixth cell of sight"
    );
}

#[test]
fn test_elevation_sight_bonus_z0_gives_no_bonus() {
    let mut store = EntityStore::new();
    let mut entity = GameEntity::new_at_frame_zero_for_test(
        1,
        10,
        10,
        0,
        0,
        intern::test_intern("Americans"),
        Health { current: 100 },
        intern::test_intern("E1"),
        EntityCategory::Infantry,
        0,
        5,
        false,
    );
    entity.lifecycle.in_limbo = false;
    store.insert(entity);
    let config = VisionConfig {
        require_playfield_membership: false,
        veteran_sight: 0.0,
        leptons_per_sight_increase: 2000,
        reveal_by_height: false,
        fog_of_war: false,
    };
    let fog = recompute_owner_visibility(
        &store,
        Some(&PathGrid::new(30, 30)),
        &Default::default(),
        &config,
        &ti(),
    );
    // z=0 → bonus = 0. Effective = 5. Cell at distance 5 visible, 6 not.
    assert!(fog.is_cell_visible(intern::test_intern("Americans"), 15, 10));
    assert!(!fog.is_cell_visible(intern::test_intern("Americans"), 16, 10));
}

#[test]
fn test_elevation_sight_bonus_disabled_when_zero() {
    let mut store = EntityStore::new();
    // High z — would give large bonus if enabled.
    let mut entity = GameEntity::new_at_frame_zero_for_test(
        1,
        10,
        10,
        16,
        0,
        intern::test_intern("Americans"),
        Health { current: 100 },
        intern::test_intern("E1"),
        EntityCategory::Infantry,
        0,
        5,
        false,
    );
    entity.lifecycle.in_limbo = false;
    store.insert(entity);
    // leptons_per_sight_increase=0 → elevation bonus disabled.
    let config = VisionConfig {
        require_playfield_membership: false,
        veteran_sight: 0.0,
        leptons_per_sight_increase: 0,
        reveal_by_height: false,
        fog_of_war: false,
    };
    let fog = recompute_owner_visibility(
        &store,
        Some(&PathGrid::new(30, 30)),
        &Default::default(),
        &config,
        &ti(),
    );
    // Effective = 5 only (elevation sight bonus disabled). The z=16 unit still
    // shifts its reveal center by z/2 = 8 cells, so (10,10) reveals around (2,2).
    // Cell at distance 5 east of the shifted center (7,2) is visible; 6 (8,2) not.
    assert!(fog.is_cell_visible(intern::test_intern("Americans"), 7, 2));
    assert!(!fog.is_cell_visible(intern::test_intern("Americans"), 8, 2));
}

#[test]
fn test_merged_visibility_fast_path() {
    let mut store = EntityStore::new();
    spawn_with_vision(&mut store, 1, "Americans", 5, 5, 3);

    let mut fog = recompute_owner_visibility(
        &store,
        Some(&PathGrid::new(16, 16)),
        &Default::default(),
        &default_config(),
        &ti(),
    );

    // Before building merged, queries still work (slow fallback).
    assert!(fog.is_cell_visible(intern::test_intern("Americans"), 5, 5));

    // Build merged cache for "Americans".
    fog.build_merged_for(intern::test_intern("Americans"), &ti());

    // Fast path should return the same results.
    assert!(fog.is_cell_visible(intern::test_intern("Americans"), 5, 5));
    assert!(fog.is_cell_visible(intern::test_intern("Americans"), 7, 5));
    assert!(!fog.is_cell_visible(intern::test_intern("Americans"), 9, 5));
    assert!(fog.is_cell_revealed(intern::test_intern("Americans"), 6, 6));
}

#[test]
fn test_reset_explored_for_owner() {
    let mut fog = FogState::default();
    fog.width = 10;
    fog.height = 10;
    fog.mark_visible_for_owner(intern::test_intern("Americans"), 3, 3);
    assert!(fog.is_cell_revealed(intern::test_intern("Americans"), 3, 3));

    fog.reset_explored_for_owner(intern::test_intern("Americans"));
    assert!(!fog.is_cell_revealed(intern::test_intern("Americans"), 3, 3));
    assert!(!fog.is_cell_visible(intern::test_intern("Americans"), 3, 3));
}

// -- Neighbor mask tests --

#[test]
fn test_shroud_edge_mask_interior_cell() {
    // All neighbors also shrouded → mask = 0b1111 (all bits set).
    let fog = FogState::default();
    let mask = fog.shroud_edge_mask(intern::test_intern("Americans"), 5, 5);
    assert_eq!(mask, 0b1111, "all neighbors shrouded → all bits set");
}

#[test]
fn test_shroud_edge_mask_with_revealed_neighbors() {
    let mut fog = FogState {
        width: 16,
        height: 16,
        ..Default::default()
    };
    // Reveal the SE neighbor (rx+1, ry) of cell (5,5) → that's (6,5).
    fog.mark_visible_for_owner(intern::test_intern("Americans"), 6, 5);

    let mask = fog.shroud_edge_mask(intern::test_intern("Americans"), 5, 5);
    // SE bit (bit 1) should be CLEAR because the SE neighbor IS revealed.
    assert_eq!(mask & 0x02, 0, "SE neighbor revealed → bit 1 clear");
    // Other bits should still be set.
    assert_eq!(mask & 0x01, 0x01, "NE neighbor still shrouded");
    assert_eq!(mask & 0x04, 0x04, "SW neighbor still shrouded");
    assert_eq!(mask & 0x08, 0x08, "NW neighbor still shrouded");
}

#[test]
fn test_shroud_edge_mask_at_grid_edge() {
    let fog = FogState::default();
    // Cell at (0,0): NE neighbor is (0, -1) which is OOB (ry underflow) → bit set.
    // NW neighbor is (-1, 0) which is OOB (rx underflow) → bit set.
    let mask = fog.shroud_edge_mask(intern::test_intern("Americans"), 0, 0);
    assert_eq!(mask & 0x01, 0x01, "NE OOB → bit set");
    assert_eq!(mask & 0x08, 0x08, "NW OOB → bit set");
}

#[test]
fn test_shroud_edge_mask_ne_uses_correct_neighbor() {
    // Verify NE checks (rx, ry-1), the edge-sharing neighbor, not (rx+1, ry-1).
    let mut fog = FogState {
        width: 16,
        height: 16,
        ..Default::default()
    };
    // Reveal the NE edge-sharing neighbor of (5,5) → that's (5, 4).
    fog.mark_visible_for_owner(intern::test_intern("Americans"), 5, 4);

    let mask = fog.shroud_edge_mask(intern::test_intern("Americans"), 5, 5);
    assert_eq!(mask & 0x01, 0, "NE neighbor (5,4) revealed → bit 0 clear");

    // The vertex-sharing cell (6, 4) should NOT affect the NE bit.
    let mut fog2 = FogState {
        width: 16,
        height: 16,
        ..Default::default()
    };
    fog2.mark_visible_for_owner(intern::test_intern("Americans"), 6, 4);
    let mask2 = fog2.shroud_edge_mask(intern::test_intern("Americans"), 5, 5);
    assert_eq!(
        mask2 & 0x01,
        0x01,
        "vertex neighbor (6,4) should NOT affect NE bit"
    );
}

// -- SpySat tests --

/// The uplink lifts the shroud off the whole map and does nothing else.
///
/// gamemd's whole-map reveal writes only the explored bit; with `FogOfWar=no`
/// there is no per-cell "currently in sight" state for it to write. VERA used
/// to set its visible bit map-wide as well, which fed the combat acquisition
/// gates and let every unit target across the entire map while the uplink
/// stood.
#[test]
fn spy_sat_reveals_the_map_without_making_it_currently_visible() {
    let mut fog = FogState {
        width: 20,
        height: 20,
        ..Default::default()
    };
    assert!(!fog.is_cell_revealed(intern::test_intern("Americans"), 10, 10));

    let americans_id = intern::test_intern("Americans");
    let interner = ti();
    apply_spy_sat(&mut fog, &[americans_id], &interner);

    for (rx, ry) in [(0u16, 0u16), (10, 10), (19, 19), (15, 15)] {
        assert!(
            fog.is_cell_revealed(intern::test_intern("Americans"), rx, ry),
            "({rx},{ry}) must be explored"
        );
        assert!(
            !fog.is_cell_visible(intern::test_intern("Americans"), rx, ry),
            "({rx},{ry}) must not be in current sight"
        );
    }
}

/// The pure materializer does not treat an absent owner as a transition. The
/// House-rung aggregate latch owns restoration when the last provider is lost.
#[test]
fn spy_sat_reveal_helper_does_not_implicitly_reshroud_an_absent_owner() {
    let owner = intern::test_intern("Americans");
    let interner = ti();
    let mut fog = FogState {
        width: 20,
        height: 20,
        ..Default::default()
    };
    apply_spy_sat(&mut fog, &[owner], &interner);
    assert!(fog.is_cell_revealed(owner, 19, 19));

    // Next tick: visibility is cleared and this materialization pass is empty.
    if let Some(vis) = fog.by_owner.get_mut(&owner) {
        vis.clear_all_visible();
    }
    apply_spy_sat(&mut fog, &[], &interner);
    assert!(
        fog.is_cell_revealed(owner, 19, 19),
        "the map stays lifted until the last uplink is gone"
    );
}

// -- Gap Generator tests --

#[test]
fn test_gap_generator_preserves_admitted_enemy_sight() {
    let mut store = EntityStore::new();
    // Spawn a Soviet unit at (10, 10) with sight 8.
    spawn_with_vision(&mut store, 1, "Soviet", 10, 10, 8);

    let mut fog = recompute_owner_visibility(
        &store,
        Some(&PathGrid::new(30, 30)),
        &Default::default(),
        &default_config(),
        &ti(),
    );
    // Soviet can see (10, 10) and nearby.
    assert!(fog.is_cell_visible(intern::test_intern("Soviet"), 10, 10));
    assert!(fog.is_cell_visible(intern::test_intern("Soviet"), 13, 10));

    // Allied gap generator at (12, 10) with radius 5.
    let americans_id = intern::test_intern("Americans");
    let interner = ti();
    apply_gap_generators(&mut fog, &[(americans_id, 12, 10, 5)], &interner);

    // Original current_sight keeps the admitted negative counter open even
    // inside a hostile gap. Merely knowing the cell would not protect it.
    assert!(fog.is_cell_visible(intern::test_intern("Soviet"), 13, 10));
    assert!(!fog.is_cell_gap_covered(intern::test_intern("Soviet"), 13, 10));
}

#[test]
fn test_gap_generator_does_not_suppress_friendly() {
    let mut fog = FogState {
        width: 20,
        height: 20,
        ..Default::default()
    };
    fog.mark_visible_for_owner(intern::test_intern("Americans"), 10, 10);

    // Gap generator owned by Americans — should NOT suppress American vision.
    let americans_id = intern::test_intern("Americans");
    let interner = ti();
    apply_gap_generators(&mut fog, &[(americans_id, 10, 10, 5)], &interner);
    assert!(fog.is_cell_visible(intern::test_intern("Americans"), 10, 10));
}

#[test]
fn gsi_04_18_hostile_gap_erases_map_knowledge_until_current_sight_returns() {
    let viewer = intern::test_intern("Soviet");
    let gapper = intern::test_intern("Americans");
    let interner = ti();
    let mut fog = FogState {
        width: 32,
        height: 32,
        ..Default::default()
    };
    fog.mark_visible_for_owner(viewer, 16, 16);
    assert!(fog.is_cell_revealed(viewer, 16, 16));

    apply_gap_generators(&mut fog, &[(gapper, 16, 16, 5)], &interner);
    assert!(!fog.is_cell_visible(viewer, 16, 16));
    assert!(!fog.is_cell_revealed(viewer, 16, 16));
    assert!(fog.is_cell_gap_covered(viewer, 16, 16));

    fog.by_owner
        .get_mut(&viewer)
        .expect("viewer plane")
        .clear_all_visible();
    apply_gap_generators(&mut fog, &[], &interner);
    assert!(!fog.is_cell_gap_covered(viewer, 16, 16));
    assert!(
        !fog.is_cell_revealed(viewer, 16, 16),
        "destroying the hostile gap must not restore erased knowledge"
    );

    reveal_radius(&mut fog, viewer, 16, 16, 1);
    assert!(fog.is_cell_visible(viewer, 16, 16));
    assert!(fog.is_cell_revealed(viewer, 16, 16));
}

#[test]
fn gsi_04_18_spy_sat_repeat_is_not_a_new_map_reveal_event() {
    let viewer = intern::test_intern("Soviet");
    let gapper = intern::test_intern("Americans");
    let interner = ti();
    let mut fog = FogState {
        width: 32,
        height: 32,
        ..Default::default()
    };

    apply_spy_sat(&mut fog, &[viewer], &interner);
    apply_gap_generators(&mut fog, &[(gapper, 16, 16, 5)], &interner);
    assert!(!fog.is_cell_revealed(viewer, 16, 16));

    fog.by_owner
        .get_mut(&viewer)
        .expect("viewer plane")
        .clear_all_visible();
    apply_spy_sat(&mut fog, &[viewer], &interner);
    assert!(
        !fog.is_cell_revealed(viewer, 16, 16),
        "577D90 House240 guard: passive visible-clear did not remove the gap or reset the map latch"
    );
    apply_gap_generators(&mut fog, &[(gapper, 16, 16, 5)], &interner);
    assert!(
        !fog.is_cell_revealed(viewer, 16, 16),
        "the active hostile gap remains the final writer"
    );

    fog.mark_visible_for_owner(gapper, 16, 16);
    apply_gap_generators(&mut fog, &[(gapper, 16, 16, 5)], &interner);
    assert!(fog.is_cell_visible(gapper, 16, 16));
    assert!(fog.is_cell_revealed(gapper, 16, 16));
    assert!(fog.is_cell_gap_fog(gapper, 16, 16));
}

// -- In-place recompute tests --

#[test]
fn test_in_place_preserves_revealed() {
    let mut store = EntityStore::new();
    spawn_with_vision(&mut store, 1, "Americans", 5, 5, 2);
    let grid = PathGrid::new(16, 16);
    let cfg = default_config();
    let alliances = HouseAllianceMap::default();

    // First compute: reveals cells around (5,5).
    let mut fog = FogState::default();
    recompute_owner_visibility_in_place(
        &mut fog,
        &store,
        Some(&grid),
        &alliances,
        &cfg,
        None,
        &ti(),
        None,
    );
    assert!(fog.is_cell_revealed(intern::test_intern("Americans"), 5, 5));
    assert!(fog.is_cell_visible(intern::test_intern("Americans"), 5, 5));

    // Move unit: remove old, spawn at (10, 10).
    store.remove(1);
    spawn_with_vision(&mut store, 2, "Americans", 10, 10, 2);

    // Second compute in-place: (5,5) should still be revealed but not visible.
    recompute_owner_visibility_in_place(
        &mut fog,
        &store,
        Some(&grid),
        &alliances,
        &cfg,
        None,
        &ti(),
        None,
    );
    assert!(fog.is_cell_revealed(intern::test_intern("Americans"), 5, 5));
    assert!(!fog.is_cell_visible(intern::test_intern("Americans"), 5, 5));
    assert!(fog.is_cell_visible(intern::test_intern("Americans"), 10, 10));
}

#[test]
fn test_dead_owner_keeps_revealed() {
    let mut store = EntityStore::new();
    spawn_with_vision(&mut store, 1, "Soviet", 5, 5, 2);
    let grid = PathGrid::new(16, 16);
    let cfg = default_config();
    let alliances = HouseAllianceMap::default();

    let mut fog = FogState::default();
    recompute_owner_visibility_in_place(
        &mut fog,
        &store,
        Some(&grid),
        &alliances,
        &cfg,
        None,
        &ti(),
        None,
    );
    assert!(fog.is_cell_revealed(intern::test_intern("Soviet"), 5, 5));

    // Remove all Soviet entities.
    store.remove(1);
    recompute_owner_visibility_in_place(
        &mut fog,
        &store,
        Some(&grid),
        &alliances,
        &cfg,
        None,
        &ti(),
        None,
    );

    // Soviet's revealed state persists, but nothing is visible.
    assert!(fog.is_cell_revealed(intern::test_intern("Soviet"), 5, 5));
    assert!(!fog.is_cell_visible(intern::test_intern("Soviet"), 5, 5));
}

#[test]
fn test_in_place_matches_fresh() {
    let mut store = EntityStore::new();
    spawn_with_vision(&mut store, 1, "Americans", 5, 5, 3);
    spawn_with_vision(&mut store, 2, "Soviet", 10, 10, 2);
    let grid = PathGrid::new(20, 20);
    let cfg = default_config();
    let alliances = HouseAllianceMap::default();

    // Fresh allocation path.
    let fresh = recompute_owner_visibility(&store, Some(&grid), &alliances, &cfg, &ti());

    // In-place path (from default).
    let mut in_place = FogState::default();
    recompute_owner_visibility_in_place(
        &mut in_place,
        &store,
        Some(&grid),
        &alliances,
        &cfg,
        None,
        &ti(),
        None,
    );

    // Both should have identical by_owner contents.
    assert_eq!(fresh.by_owner.len(), in_place.by_owner.len());
    for (owner, fresh_vis) in &fresh.by_owner {
        let ip_vis = in_place
            .by_owner
            .get(owner)
            .expect("owner missing in in-place result");
        assert_eq!(
            fresh_vis.cells_raw(),
            ip_vis.cells_raw(),
            "mismatch for {}",
            owner
        );
    }
}

// -- FLAG_GAP_COVERED tests --

#[test]
fn test_gap_generator_sets_gap_covered_flag() {
    let mut store = EntityStore::new();
    spawn_with_vision(&mut store, 1, "Soviet", 10, 10, 8);

    let mut fog = recompute_owner_visibility(
        &store,
        Some(&PathGrid::new(30, 30)),
        &Default::default(),
        &default_config(),
        &ti(),
    );

    // Before gap: cell is revealed and visible, NOT gap-covered.
    assert!(fog.is_cell_revealed(intern::test_intern("Soviet"), 12, 10));
    assert!(fog.is_cell_visible(intern::test_intern("Soviet"), 12, 10));
    fog.build_merged_for(intern::test_intern("Soviet"), &ti());
    assert!(!fog.is_cell_gap_covered(intern::test_intern("Soviet"), 12, 10));

    // Release the sustained source first: original past_sight retains mapped
    // knowledge at counter0, which the next hostile write can conceal.
    store.remove(1);
    recompute_owner_visibility_in_place(
        &mut fog,
        &store,
        Some(&PathGrid::new(30, 30)),
        &Default::default(),
        &default_config(),
        None,
        &ti(),
        None,
    );
    assert!(fog.is_cell_revealed(intern::test_intern("Soviet"), 12, 10));
    assert!(!fog.is_cell_visible(intern::test_intern("Soviet"), 12, 10));

    // American gap generator at (12, 10) with radius 5.
    let americans_id = intern::test_intern("Americans");
    let interner = ti();
    apply_gap_generators(&mut fog, &[(americans_id, 12, 10, 5)], &interner);
    fog.build_merged_for(intern::test_intern("Soviet"), &ti());

    // Cell should now be gap-covered AND not visible for Soviet.
    assert!(fog.is_cell_gap_covered(intern::test_intern("Soviet"), 12, 10));
    assert!(!fog.is_cell_visible(intern::test_intern("Soviet"), 12, 10));
    // Hostile gap coverage erases current map knowledge as well as sight.
    assert!(!fog.is_cell_revealed(intern::test_intern("Soviet"), 12, 10));
}

#[test]
fn test_gap_covered_not_set_for_friendly() {
    let mut fog = FogState {
        width: 20,
        height: 20,
        ..Default::default()
    };
    fog.mark_visible_for_owner(intern::test_intern("Americans"), 10, 10);

    // Gap owned by Americans — should NOT gap-cover American cells.
    let americans_id = intern::test_intern("Americans");
    let interner = ti();
    apply_gap_generators(&mut fog, &[(americans_id, 10, 10, 5)], &interner);
    fog.build_merged_for(intern::test_intern("Americans"), &ti());

    assert!(!fog.is_cell_gap_covered(intern::test_intern("Americans"), 10, 10));
}

#[test]
fn test_gap_covered_cleared_each_tick() {
    let mut vis = OwnerVisibility::new(10, 10);
    vis.mark_visible(5, 5);
    // Manually set gap-covered bit.
    if let Some(i) = vis.index(5, 5) {
        vis.cells[i] |= 0x04; // FLAG_GAP_COVERED
    }
    assert!(vis.is_gap_covered(5, 5));

    // clear_all_visible should also clear gap-covered.
    vis.clear_all_visible();
    assert!(!vis.is_gap_covered(5, 5));
    // But revealed persists.
    assert!(vis.is_revealed(5, 5));
}

// -- Height-based LOS (RevealByHeight) tests --

#[test]
fn test_height_los_blocks_sight_behind_cliff() {
    // Unit at (5,5) height 0, sight 5. Target (8,5) = spiral offset (3,0); its
    // mirror is (-1,0), so the original engine samples the obstruction cell at
    // target + mirror + (2,2) = (8,5) + (-1,0) + (2,2) = (9,7). A tall cliff there
    // (level 5 > viewer_level 0 + 3) blocks sight to (8,5).
    let mut vis = OwnerVisibility::new(20, 20);
    let width: u16 = 20;
    let height: u16 = 20;
    let mut hg = vec![0u8; width as usize * height as usize];
    hg[7 * width as usize + 9] = 5; // obstruction cell (9,7)

    reveal_radius_into(&mut vis, 5, 5, 5, 0, true, true, Some(&hg), width, height);

    // (6,5)'s obstruction is (7,7) (no cliff) — visible.
    assert!(vis.is_visible(6, 5));
    // (8,5)'s obstruction (9,7) is the cliff — blocked.
    assert!(!vis.is_visible(8, 5));
}

#[test]
fn test_height_los_plus_two_obstruction_offset() {
    // Pins the +2 obstruction offset. For target (8,5) the obstruction is
    // target + mirror(-1,0) + (2,2) = (9,7), NOT the naive target + mirror = (7,5).
    let width: u16 = 20;
    let height: u16 = 20;

    // Cliff at the naive (no-+2) location (7,5) must NOT block (8,5).
    let mut vis = OwnerVisibility::new(20, 20);
    let mut hg = vec![0u8; width as usize * height as usize];
    hg[5 * width as usize + 7] = 5; // (7,5) — the pre-fix obstruction guess
    reveal_radius_into(&mut vis, 5, 5, 5, 0, true, true, Some(&hg), width, height);
    assert!(
        vis.is_visible(8, 5),
        "naive (7,5) cliff must not block with the +2 offset"
    );

    // Cliff at the +2 location (9,7) must block (8,5).
    let mut vis2 = OwnerVisibility::new(20, 20);
    let mut hg2 = vec![0u8; width as usize * height as usize];
    hg2[7 * width as usize + 9] = 5; // (9,7) — the actual obstruction cell
    reveal_radius_into(&mut vis2, 5, 5, 5, 0, true, true, Some(&hg2), width, height);
    assert!(
        !vis2.is_visible(8, 5),
        "+2 obstruction cell (9,7) must block"
    );
}

#[test]
fn test_height_los_high_viewer_sees_past_cliff() {
    // Viewer at (5,5), sight 5. A cliff (level 5) sits at obstruction cell (9,7),
    // which gates spiral index 29 (offset (3,0), mirror (-1,0)). The obstruction is
    // relative to the raw foot cell: (5,5) + (3,0) + (-1,0) + (2,2) = (9,7),
    // independent of the elevation Z-shift.
    let width: u16 = 20;
    let height: u16 = 20;
    let mut hg = vec![0u8; width as usize * height as usize];
    hg[7 * width as usize + 9] = 5;

    // Low viewer (z=0, no shift): index-29 reveal cell is (8,5); 0+3 < 5 → blocked.
    let mut low = OwnerVisibility::new(width, height);
    reveal_radius_into(&mut low, 5, 5, 5, 0, true, true, Some(&hg), width, height);
    assert!(!low.is_visible(8, 5), "low viewer is blocked by the cliff");

    // High viewer (level 4 = 4*104 leptons, shift=2): index-29 reveal cell
    // shifts to (6,3); the SAME obstruction (9,7) is checked, but 4+3 = 7 >= 5
    // → LOS passes.
    let mut high = OwnerVisibility::new(width, height);
    reveal_radius_into(
        &mut high,
        5,
        5,
        5,
        4 * 104,
        true,
        true,
        Some(&hg),
        width,
        height,
    );
    assert!(
        high.is_visible(6, 3),
        "high viewer sees past the cliff (reveal cell shifted to (6,3))"
    );
}

#[test]
fn test_reveal_center_z_shift() {
    // An elevated unit reveals around its *screen* cell, not its raw foot cell:
    // the spiral center is shifted -z_level/2 on each axis (toward isometric
    // north). z=4 → shift 2 cells. Pins the elevation reveal-center fix.
    let width: u16 = 20;
    let height: u16 = 20;

    // Ground unit (z=0): center stays at the foot cell (10,10).
    let mut ground = OwnerVisibility::new(width, height);
    reveal_radius_into(&mut ground, 10, 10, 1, 0, false, true, None, width, height);
    assert!(
        ground.is_visible(10, 10),
        "ground unit centers on its foot cell"
    );
    assert!(
        !ground.is_visible(8, 8),
        "ground reveal does not reach (8,8)"
    );

    // Elevated unit (level 4 = 4*104 leptons): center shifts to (8,8). The raw
    // foot cell (10,10) is offset (2,2) from the shifted center → outside the
    // sight-1 footprint.
    let mut elevated = OwnerVisibility::new(width, height);
    reveal_radius_into(
        &mut elevated,
        10,
        10,
        1,
        4 * 104,
        false,
        true,
        None,
        width,
        height,
    );
    assert!(
        elevated.is_visible(8, 8),
        "elevated reveal centers on the Z-shifted cell (8,8)"
    );
    assert!(
        !elevated.is_visible(10, 10),
        "raw foot cell (10,10) is no longer the reveal center"
    );
}

#[test]
fn test_height_los_disabled_when_false() {
    // Same cliff scenario but reveal_by_height=false — should NOT block.
    let mut vis = OwnerVisibility::new(20, 20);
    let width: u16 = 20;
    let height: u16 = 20;
    let mut hg = vec![0u8; width as usize * height as usize];
    hg[5 * width as usize + 7] = 5;

    reveal_radius_into(&mut vis, 5, 5, 5, 0, false, true, Some(&hg), width, height);

    // With reveal_by_height=false, the cliff doesn't block.
    assert!(vis.is_visible(8, 5));
}

#[test]
fn test_height_los_none_grid_disables_check() {
    // reveal_by_height=true but no height grid — should NOT block.
    let mut vis = OwnerVisibility::new(20, 20);
    let width: u16 = 20;
    let height: u16 = 20;

    reveal_radius_into(&mut vis, 5, 5, 5, 0, true, true, None, width, height);

    // Without a height grid, all cells in range are visible.
    assert!(vis.is_visible(8, 5));
}

// -- Gap Generator footprint + hostile/friendly branches --

#[test]
fn gap_radius_uses_strict_radius_plus_one_squared() {
    // radius 10 -> threshold (10+1)^2 = 121. A cell at d^2 = 120 is covered,
    // d^2 = 121 is not. Pins the native strict `< (r+1)^2` footprint.
    let enemy = intern::test_intern("Soviet");
    let gapper = intern::test_intern("Americans");
    let interner = ti();
    let mut fog = FogState {
        width: 64,
        height: 64,
        ..Default::default()
    };
    // Pre-reveal a cell so suppression is observable (also creates the enemy grid).
    fog.mark_visible_for_owner(enemy, 30, 40);
    // No alliance entries => enemy is hostile to gapper.
    apply_gap_generators(&mut fog, &[(gapper, 30, 30, 10)], &interner);

    assert!(fog.is_cell_gap_covered(enemy, 32, 40)); // dx=2,dy=10 -> 104 < 121
    assert!(!fog.is_cell_gap_covered(enemy, 30, 41)); // dx=0,dy=11 -> 121 not < 121
    assert!(fog.is_cell_gap_covered(enemy, 30, 40)); // dx=0,dy=10 -> 100 < 121
    assert!(!fog.is_cell_visible(enemy, 30, 40)); // hostile gap clears visibility
}

#[test]
fn gap_marks_friendly_viewer_as_fog_not_covered() {
    // A gap generator over its own/allied territory fogs (half-bright), it does
    // not black out or suppress the owner's vision.
    let gapper = intern::test_intern("Americans");
    let interner = ti();
    let mut fog = FogState {
        width: 64,
        height: 64,
        ..Default::default()
    };
    fog.mark_visible_for_owner(gapper, 30, 35);
    apply_gap_generators(&mut fog, &[(gapper, 30, 30, 10)], &interner);

    assert!(fog.is_cell_gap_fog(gapper, 30, 35)); // friendly => fog
    assert!(!fog.is_cell_gap_covered(gapper, 30, 35)); // not black
    assert!(fog.is_cell_visible(gapper, 30, 35)); // own vision NOT suppressed
}

#[test]
fn gap_coverage_clears_when_no_generator_present() {
    // dev recomputes the transient coverage flag each tick (no reference
    // counter); removing the generator does not restore erased map knowledge.
    let enemy = intern::test_intern("Soviet");
    let gapper = intern::test_intern("Americans");
    let interner = ti();
    let mut fog = FogState {
        width: 64,
        height: 64,
        ..Default::default()
    };
    fog.mark_visible_for_owner(enemy, 30, 35);
    apply_gap_generators(&mut fog, &[(gapper, 30, 30, 10)], &interner);
    assert!(fog.is_cell_gap_covered(enemy, 30, 35));

    // New tick: clear_all_visible drops the gap flags; no generator => stays clear.
    if let Some(vis) = fog.by_owner.get_mut(&enemy) {
        vis.clear_all_visible();
    }
    apply_gap_generators(&mut fog, &[], &interner);
    assert!(!fog.is_cell_gap_covered(enemy, 30, 35));
}

/// A stationary viewer must reveal a solid disc — no unrevealed cells trapped inside it.
///
/// Scattered black cells surrounded by lit terrain look exactly like a spiral table with
/// gaps in it, so this pins the shape directly rather than inferring it from a screenshot.
#[test]
fn reveal_covers_a_solid_disc_with_no_interior_holes() {
    let owner = intern::test_intern("Americans");
    for range in 1..=MAX_SIGHT_RANGE {
        let mut fog = FogState {
            width: 64,
            height: 64,
            ..Default::default()
        };
        reveal_radius(&mut fog, owner, 32, 32, range);

        let mut holes: Vec<(u16, u16)> = Vec::new();
        let reach = i32::from(range);
        for dy in -reach..=reach {
            for dx in -reach..=reach {
                // Stay well inside the outer edge: the boundary is a discretization
                // choice, but anything comfortably within the radius must be lit.
                if dx * dx + dy * dy > (reach - 1).max(0) * (reach - 1).max(0) {
                    continue;
                }
                let (rx, ry) = ((32 + dx) as u16, (32 + dy) as u16);
                if !fog.is_cell_revealed(owner, rx, ry) {
                    holes.push((rx, ry));
                }
            }
        }
        assert!(
            holes.is_empty(),
            "sight {range} left {} interior cell(s) unrevealed: {:?}",
            holes.len(),
            &holes[..holes.len().min(20)]
        );
    }
}

/// Driving in a straight line must leave a continuous swept corridor behind.
///
/// If reveal were applied only at whole-cell arrivals, or the spiral were re-centred
/// wrongly per step, the trail would come out dashed — lit patches with black gaps
/// between them, which is what the reported artifact looks like.
#[test]
fn a_moving_viewer_leaves_no_gaps_along_its_path() {
    let owner = intern::test_intern("Americans");
    let mut fog = FogState {
        width: 64,
        height: 64,
        ..Default::default()
    };
    let range = 4u16;
    for step in 0..40u16 {
        reveal_radius(&mut fog, owner, 10 + step, 32, range);
    }
    let mut holes: Vec<(u16, u16)> = Vec::new();
    for step in 0..40u16 {
        let rx = 10 + step;
        for dy in -1i32..=1 {
            let ry = (32 + dy) as u16;
            if !fog.is_cell_revealed(owner, rx, ry) {
                holes.push((rx, ry));
            }
        }
    }
    assert!(
        holes.is_empty(),
        "the swept corridor has {} gap(s): {:?}",
        holes.len(),
        &holes[..holes.len().min(20)]
    );
}

/// With height-LOS enabled but perfectly flat terrain, reveal must be identical to
/// reveal with LOS disabled.
///
/// Nothing can block sight when nothing is raised, so any difference here is the
/// obstruction math misfiring rather than terrain doing its job.
#[test]
fn height_los_on_flat_terrain_blocks_nothing() {
    let owner = intern::test_intern("Americans");
    let (w, h) = (64u16, 64u16);
    let flat: Vec<u8> = vec![0; usize::from(w) * usize::from(h)];

    for range in 1..=MAX_SIGHT_RANGE {
        let mut with_los = OwnerVisibility::new(w, h);
        let mut without_los = OwnerVisibility::new(w, h);
        super::reveal_radius_into(
            &mut with_los,
            32,
            32,
            range,
            0,
            true,
            true,
            Some(&flat),
            w,
            h,
        );
        super::reveal_radius_into(&mut without_los, 32, 32, range, 0, false, true, None, w, h);

        let mut blocked: Vec<(u16, u16)> = Vec::new();
        for ry in 0..h {
            for rx in 0..w {
                if without_los.is_revealed(rx, ry) && !with_los.is_revealed(rx, ry) {
                    blocked.push((rx, ry));
                }
            }
        }
        assert!(
            blocked.is_empty(),
            "sight {range}: height-LOS blocked {} cell(s) on flat ground: {:?}",
            blocked.len(),
            &blocked[..blocked.len().min(20)]
        );
        let _ = owner;
    }
}

/// A raised cell well outside the sight radius must not block anything inside it.
///
/// The obstruction sample is derived from the target cell plus a mirror step, a fixed
/// +2 on each axis, and the z-shift undo. If that arithmetic lands on the wrong cell,
/// unrelated terrain starts blocking sight — which shows up as black cells scattered
/// through otherwise-lit ground rather than as a shadow behind a cliff.
#[test]
fn distant_terrain_cannot_block_nearby_sight() {
    let (w, h) = (64u16, 64u16);
    let mut heights: Vec<u8> = vec![0; usize::from(w) * usize::from(h)];
    // A tall cell 20 cells away — far outside a sight-5 circle centred at (32,32).
    heights[usize::from(52u16) * usize::from(w) + usize::from(52u16)] = 12;

    let mut with_obstacle = OwnerVisibility::new(w, h);
    let mut flat_only = OwnerVisibility::new(w, h);
    let flat: Vec<u8> = vec![0; usize::from(w) * usize::from(h)];
    super::reveal_radius_into(
        &mut with_obstacle,
        32,
        32,
        5,
        0,
        true,
        true,
        Some(&heights),
        w,
        h,
    );
    super::reveal_radius_into(&mut flat_only, 32, 32, 5, 0, true, true, Some(&flat), w, h);

    let mut differing: Vec<(u16, u16)> = Vec::new();
    for ry in 0..h {
        for rx in 0..w {
            if flat_only.is_revealed(rx, ry) != with_obstacle.is_revealed(rx, ry) {
                differing.push((rx, ry));
            }
        }
    }
    assert!(
        differing.is_empty(),
        "a cell 20 away changed reveal for {} cell(s): {:?}",
        differing.len(),
        &differing[..differing.len().min(20)]
    );
}

// -- Reveal footprint: the engine's spiral table --

/// The engine's ring membership rule, recovered from its table initialiser:
/// a cell belongs to ring `r` when `max(|dx|,|dy|) + min(|dx|,|dy|)/2 == r`,
/// with truncating division. This is the classic C&C distance approximation.
fn ring_of(dx: i32, dy: i32) -> usize {
    let far = dx.abs().max(dy.abs());
    let near = dx.abs().min(dy.abs());
    (far + near / 2) as usize
}

/// Every spiral entry sits in the ring the cumulative table says it does.
///
/// This is what makes the table checkable rather than a wall of magic numbers:
/// the same rule that places each of the 309 offsets also reproduces all twelve
/// of the engine's cumulative counts, so a transcription slip in any single
/// entry fails here.
#[test]
fn spiral_ring_membership_matches_the_engines_distance_rule() {
    let mut start = 0usize;
    for ring in 0..=MAX_SIGHT_RANGE as usize {
        let end = super::REVEAL_RING_SIZES[ring];
        for i in start..end {
            let (dx, dy) = super::REVEAL_SPIRAL[i];
            assert_eq!(
                ring_of(i32::from(dx), i32::from(dy)),
                ring,
                "spiral[{i}] = ({dx},{dy}) is not in ring {ring}"
            );
        }
        start = end;
    }
}

/// The three tables are indexed in lockstep, so they must not drift apart.
#[test]
fn spiral_and_mirror_tables_cover_every_reachable_sight() {
    assert_eq!(super::REVEAL_SPIRAL.len(), super::REVEAL_RING_SIZES[10]);
    assert_eq!(super::REVEAL_MIRROR.len(), super::REVEAL_RING_SIZES[10]);
    let mut seen: Vec<(i8, i8)> = super::REVEAL_SPIRAL.to_vec();
    seen.sort_unstable();
    let before = seen.len();
    seen.dedup();
    assert_eq!(before, seen.len(), "the spiral table repeats a cell");
}

/// Sight 10 reveals exactly the 309 cells the engine's ring table names.
///
/// VERA used to abandon the table at sight 10 and sweep a `d <= 10.5` bounding
/// disc instead: 349 cells, ~40 more than the engine ever reveals, around every
/// naval yard, radar, Grand Cannon, Psychic Sensor, Gattling Cannon/Tank and
/// Magnetron for the whole match.
#[test]
fn sight_ten_reveals_the_retail_cell_count() {
    let (w, h) = (64u16, 64u16);
    let mut vis = OwnerVisibility::new(w, h);
    super::reveal_radius_into(&mut vis, 32, 32, 10, 0, false, true, None, w, h);

    let revealed = (0..h)
        .flat_map(|ry| (0..w).map(move |rx| (rx, ry)))
        .filter(|&(rx, ry)| vis.is_revealed(rx, ry))
        .count();
    assert_eq!(
        revealed, 309,
        "sight 10 is the ring table's cumulative count"
    );
    // Boundary members and non-members of the outermost ring.
    assert!(vis.is_revealed(42, 32), "(10,0) is in ring 10");
    assert!(vis.is_revealed(39, 39), "(7,7) is in ring 10");
    assert!(!vis.is_revealed(40, 39), "(8,7) is ring 11 — out of reach");
    assert!(!vis.is_revealed(43, 32), "(11,0) is ring 11 — out of reach");
}

/// Ring 10 goes through the same terrain height gate as every inner ring.
///
/// The old bounding-disc sweep re-marked every cell it touched with no
/// line-of-sight test at all, so a Sight=10 building next to a cliff
/// permanently lit ground the engine leaves black — and it overwrote the
/// rejections the spiral pass had just made.
#[test]
fn sight_ten_outer_ring_is_still_blocked_by_terrain_height() {
    let (w, h) = (64u16, 64u16);
    let flat: Vec<u8> = vec![0; usize::from(w) * usize::from(h)];
    let mut open = OwnerVisibility::new(w, h);
    super::reveal_radius_into(&mut open, 32, 32, 10, 0, true, true, Some(&flat), w, h);
    assert!(open.is_revealed(42, 32), "flat ground hides nothing");

    // Ring-10 target (42,32) is spiral offset (10,0), whose mirror is (-1,0);
    // the engine samples target + mirror + (2,2) = (43,34). A cliff there is
    // more than the viewer's level + 3, so the target must stay dark.
    let mut heights = flat.clone();
    heights[usize::from(34u16) * usize::from(w) + usize::from(43u16)] = 8;
    let mut blocked = OwnerVisibility::new(w, h);
    super::reveal_radius_into(
        &mut blocked,
        32,
        32,
        10,
        0,
        true,
        true,
        Some(&heights),
        w,
        h,
    );
    assert!(
        !blocked.is_revealed(42, 32),
        "a ring-10 cell must be gated by the same height test as ring 9"
    );
    assert!(
        blocked.is_revealed(42, 31),
        "the neighbouring ring-10 cell samples a different obstruction and stays lit"
    );
}

/// `Sight=0` reveals nothing at all — not even the cell the object stands on.
///
/// 36 stock types carry it: fences and walls, the 24 map lamp posts, and the
/// spy/cargo/paradrop planes that cross the map while airborne.
#[test]
fn sight_zero_reveals_nothing() {
    let (w, h) = (16u16, 16u16);
    let mut vis = OwnerVisibility::new(w, h);
    super::reveal_radius_into(&mut vis, 8, 8, 0, 0, false, true, None, w, h);
    let any = (0..h).any(|ry| (0..w).any(|rx| vis.is_revealed(rx, ry)));
    assert!(!any, "a Sight=0 object must not open a hole in the shroud");
}

// -- Reveal centre height shift --

/// The shift is derived from a lepton height, and for every terrain level a
/// retail map can carry it still lands on the old `level / 2` shorthand.
#[test]
fn the_height_shift_reduces_to_half_the_terrain_level() {
    for level in 0..=19i32 {
        assert_eq!(
            super::iso_height_shift_cells(level * 104),
            level / 2,
            "terrain level {level}"
        );
    }
}

/// An airborne viewer's reveal disc sits under its sprite, not under its ground
/// shadow.
///
/// The engine feeds the object's world Z — flight altitude included — to the
/// same shift a hill uses. At stock `FlightLevel=1500` that is 216px of screen
/// lift, i.e. 7 whole cells toward isometric north, which is exactly how far
/// above its ground cell the renderer draws the aircraft.
#[test]
fn an_airborne_viewer_reveals_under_its_sprite() {
    let (w, h) = (64u16, 64u16);
    let mut vis = OwnerVisibility::new(w, h);
    for (x, y) in super::collect_reveal_cells(30, 30, 1, 1500, false, None, w, h) {
        vis.mark_visible(x, y);
    }
    assert!(
        vis.is_revealed(23, 23),
        "centre shifts 7 cells toward iso-north"
    );
    assert!(
        !vis.is_revealed(30, 30),
        "the aircraft's ground cell is 7 cells from the centre of a sight-1 disc"
    );
}

/// A cliff cannot hide anything from an aircraft flying over it: the LOS viewer
/// level is the object's world Z in height levels, which at cruise altitude is
/// far above any terrain.
#[test]
fn an_airborne_viewer_sees_past_a_cliff() {
    let (w, h) = (64u16, 64u16);
    // Level-12 ground everywhere: more than 3 above a viewer standing at level
    // 0, and far below one cruising at 1500 leptons (level 14).
    let heights: Vec<u8> = vec![12; usize::from(w) * usize::from(h)];
    let mut grounded = OwnerVisibility::new(w, h);
    for (x, y) in super::collect_reveal_cells(30, 30, 5, 0, true, Some(&heights), w, h) {
        grounded.mark_visible(x, y);
    }
    let mut flying = OwnerVisibility::new(w, h);
    for (x, y) in super::collect_reveal_cells(30, 30, 5, 1500, true, Some(&heights), w, h) {
        flying.mark_visible(x, y);
    }

    let count = |vis: &OwnerVisibility| {
        (0..h)
            .flat_map(|ry| (0..w).map(move |rx| (rx, ry)))
            .filter(|&(rx, ry)| vis.is_revealed(rx, ry))
            .count()
    };
    assert_eq!(
        count(&grounded),
        0,
        "the terrain towers over a ground viewer"
    );
    assert_eq!(
        count(&flying),
        89,
        "nothing blocks a viewer above the terrain"
    );
}

/// The altitude actually reaches the reveal from a live entity, not just from a
/// direct call to the kernel.
#[test]
fn an_aircrafts_altitude_moves_its_revealed_disc() {
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::sim::movement::locomotor::{LocomotorState, MovementLayer};
    use crate::util::fixed_math::SimFixed;

    let mut store = EntityStore::new();
    let mut entity = GameEntity::test_default(1, "BEAG", "Americans", 30, 30);
    entity.category = EntityCategory::Aircraft;
    entity.lifecycle.in_limbo = false;
    entity.vision_range = 1;
    let mut loco = LocomotorState::for_test_kind(LocomotorKind::Fly);
    loco.layer = MovementLayer::Air;
    loco.altitude = SimFixed::from_num(1500);
    entity.locomotor = Some(loco);
    store.insert(entity);

    let fog = recompute_owner_visibility(
        &store,
        Some(&PathGrid::new(64, 64)),
        &Default::default(),
        &default_config(),
        &ti(),
    );
    let owner = intern::test_intern("Americans");
    assert!(fog.is_cell_visible(owner, 23, 23));
    assert!(!fog.is_cell_visible(owner, 30, 30));
}

/// F10: the merged view lives in a nonserialized cache — a bincode round trip
/// (the snapshot serializer) discards it while the wire shadow survives in the
/// old `generation` slot, and building for a different owner replaces the
/// cached owner and bumps only the runtime view generation.
#[test]
fn fog_view_cache_is_discarded_and_rebuilt_after_load_or_owner_change() {
    let mut store = EntityStore::new();
    spawn_with_vision(&mut store, 1, "Americans", 4, 4, 3);
    let mut fog = recompute_owner_visibility(
        &store,
        Some(&PathGrid::new(16, 16)),
        &Default::default(),
        &default_config(),
        &ti(),
    );
    let americans = intern::test_intern("Americans");
    fog.build_merged_for(americans, &ti());
    assert!(fog.view_cache.merged.is_some());
    let built_generation = fog.view_generation();
    assert!(built_generation > 0);
    let shadow_before = fog.generation_wire_shadow;

    // Snapshot-style round trip: the cache is discarded, the shadow survives.
    let bytes = bincode::serialize(&fog).expect("fog serializes");
    let restored: FogState = bincode::deserialize(&bytes).expect("fog deserializes");
    assert!(
        restored.view_cache.merged.is_none(),
        "the merged view cache must not survive a load"
    );
    assert_eq!(
        restored.view_generation(),
        0,
        "the runtime view generation restarts after a load"
    );
    assert_eq!(
        restored.generation_wire_shadow, shadow_before,
        "the v81 wire shadow survives in the old generation slot"
    );

    // Rebuild after the load: queries work again through the fast path.
    let mut restored = restored;
    restored.build_merged_for(americans, &ti());
    assert!(restored.is_cell_visible(americans, 4, 4));
    assert_eq!(restored.view_generation(), 1);

    // Owner change: the cache is replaced for the new owner and bumped.
    let russians = intern::test_intern("Russians");
    restored.build_merged_for(russians, &ti());
    assert_eq!(
        restored.view_cache.merged.as_ref().map(|(owner, _)| *owner),
        Some(russians),
        "an owner change replaces the cached owner"
    );
    assert_eq!(restored.view_generation(), 2);
}

/// F10: view-cache builds are presentation-only — repeated rebuilds, for the
/// same or different owners, cannot alter the deterministic state hash.
#[test]
fn repeated_fog_view_builds_leave_state_hash_unchanged() {
    use crate::sim::world::Simulation;

    let mut sim = Simulation::with_seed(0xF10);
    let owner = sim.interner.intern("Americans");
    let other = sim.interner.intern("Russians");
    sim.fog.width = 8;
    sim.fog.height = 8;
    sim.fog.by_owner.insert(owner, OwnerVisibility::new(8, 8));
    sim.fog.by_owner.insert(other, OwnerVisibility::new(8, 8));

    let before = sim.state_hash();
    for _ in 0..5 {
        assert!(sim.prepare_fog_view_for("Americans"));
        assert!(sim.prepare_fog_view_for("Russians"));
    }
    assert_eq!(
        sim.state_hash(),
        before,
        "fog view preparation is invisible to the deterministic hash"
    );
}

/// Original MapCell + hostile Gap cell blocks + real early-Logic 120-frame sweep.
/// Compare the selected knowledge consumer, not the legacy projected raw counters.
#[test]
fn shroud_current_sight_matches_original_native_sequences() {
    let corpus: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/shroud_current_sight.json"
    ))
    .unwrap();
    assert_eq!(corpus["cases"].as_array().unwrap().len(), 23);
    let owner = intern::test_intern("Americans");
    let gapper = intern::test_intern("Soviet");
    let interner = ti();
    for case in corpus["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let mut fog = FogState {
            width: 32,
            height: 32,
            ..Default::default()
        };
        fog.by_owner.insert(owner, OwnerVisibility::new(32, 32));
        let mut store = EntityStore::new();
        let mut count = 0u64;
        let mut gaps = Vec::new();
        for observation in case["observations"].as_array().unwrap() {
            let op = observation["op"].as_str().unwrap();
            match op {
                "constructed" => {}
                "reveal" | "leave" => {
                    if op == "reveal" {
                        count += 1;
                        spawn_with_vision(&mut store, count, "Americans", 10, 10, 2);
                    } else {
                        store.remove(count);
                        count -= 1;
                    }
                    recompute_owner_visibility_in_place(
                        &mut fog,
                        &store,
                        None,
                        &Default::default(),
                        &default_config(),
                        None,
                        &interner,
                        None,
                    );
                    apply_gap_generators(&mut fog, &gaps, &interner);
                }
                "unshroud" => reveal_radius(&mut fog, owner, 10, 10, 2),
                "map_reveal_bulk" => fog.reveal_all_for_owner(owner),
                "map_reset_bulk" => fog.reset_explored_for_owner(owner),
                "gap" => {
                    gaps.push((gapper, 10, 10, 3));
                    apply_gap_generators(&mut fog, &gaps, &interner);
                }
                "remove" | "remove_mapclear" => {
                    gaps.pop();
                    let sources: Vec<_> = gaps
                        .iter()
                        .enumerate()
                        .map(|(index, &(owner, rx, ry, radius))| GapGeneratorSource {
                            stable_id: index as u64,
                            owner,
                            rx,
                            ry,
                            radius,
                        })
                        .collect();
                    let map_clear = if op == "remove_mapclear" {
                        std::collections::BTreeSet::from([owner])
                    } else {
                        Default::default()
                    };
                    apply_gap_generator_sources_with_spy_sat(
                        &mut fog, &sources, &interner, &map_clear,
                    );
                }
                frame if frame.starts_with("frame") => {
                    fog.flush_pending_gap_conceal(frame[5..].parse().unwrap())
                }
                _ => panic!("unknown native operation {op}"),
            }
            let vis = &fog.by_owner[&owner];
            let state = vis.shroud_knowledge[vis.index(10, 10).unwrap()];
            assert_eq!(
                state.counter,
                observation["counter"].as_i64().unwrap() as i32,
                "{name}/{op}/counter"
            );
            assert_eq!(
                state.gap_counter,
                observation["gap"].as_i64().unwrap() as i32,
                "{name}/{op}/gap"
            );
            assert_eq!(
                state.pending,
                observation["flags"].as_u64().unwrap() & 0x20 != 0,
                "{name}/{op}/pending"
            );
            let shrouded =
                !fog.is_cell_revealed(owner, 10, 10) || fog.is_cell_gap_covered(owner, 10, 10);
            assert_eq!(
                shrouded,
                observation["shrouded"].as_u64().unwrap() != 0,
                "{name}/{op}"
            );
        }
    }
}

#[test]
fn shroud_current_sight_allied_gap_and_pending_survive_serialization() {
    let owner = intern::test_intern("Americans");
    let ally = intern::test_intern("Alliance");
    let gapper = intern::test_intern("Soviet");
    let interner = ti();
    let mut alliances = HouseAllianceMap::new();
    alliances
        .entry("AMERICANS".into())
        .or_default()
        .insert("ALLIANCE".into());
    alliances
        .entry("ALLIANCE".into())
        .or_default()
        .insert("AMERICANS".into());
    let mut store = EntityStore::new();
    spawn_with_vision(&mut store, 1, "Alliance", 10, 10, 3);
    let mut fog = FogState {
        width: 32,
        height: 32,
        ..Default::default()
    };
    fog.by_owner.insert(owner, OwnerVisibility::new(32, 32));
    recompute_owner_visibility_in_place(
        &mut fog,
        &store,
        None,
        &alliances,
        &default_config(),
        None,
        &interner,
        None,
    );
    apply_gap_generators(&mut fog, &[(gapper, 10, 10, 3)], &interner);
    fog.build_merged_for(owner, &interner);
    assert!(fog.is_cell_visible(owner, 10, 10));
    assert!(!fog.is_cell_gap_covered(owner, 10, 10));
    let encoded = bincode::serialize(&fog).unwrap();
    let mut restored: FogState = bincode::deserialize(&encoded).unwrap();
    apply_gap_generators(&mut restored, &[(gapper, 10, 10, 3)], &interner);
    assert!(restored.is_cell_revealed(owner, 10, 10));
    store.remove(1);
    recompute_owner_visibility_in_place(
        &mut restored,
        &store,
        None,
        &alliances,
        &default_config(),
        None,
        &interner,
        None,
    );
    apply_gap_generators(&mut restored, &[(gapper, 10, 10, 3)], &interner);
    let encoded = bincode::serialize(&restored).unwrap();
    let mut restored: FogState = bincode::deserialize(&encoded).unwrap();
    apply_gap_generators(&mut restored, &[], &interner); // Removal does not cancel native pending20.
    restored.flush_pending_gap_conceal(119);
    assert!(restored.is_cell_revealed(owner, 10, 10));
    restored.flush_pending_gap_conceal(120);
    restored.build_merged_for(owner, &interner);
    assert!(!restored.is_cell_revealed(owner, 10, 10));
    assert!(!restored.is_cell_revealed(ally, 10, 10));
}

#[test]
fn shroud_current_sight_provenance_changes_future_conceal_and_hash() {
    let owner = intern::test_intern("Americans");
    let gapper = intern::test_intern("Soviet");
    let mut store = EntityStore::new();
    spawn_with_vision(&mut store, 1, "Americans", 10, 10, 3);
    let mut sim = crate::sim::world::Simulation::with_seed(142);
    sim.interner = ti();
    sim.fog = recompute_owner_visibility(
        &store,
        Some(&PathGrid::new(32, 32)),
        &Default::default(),
        &default_config(),
        &sim.interner,
    );
    let sight_hash = sim.state_hash();
    let historical_hash = sim.state_hash_without_sustained_gap_sight_v142();
    let mut mapped = sim.fog.clone();
    // Keep the public observation identical, but replace the source's selected
    // state with a past mapping receipt. Historical projections remain equal.
    mapped.sight_admissions.clear();
    for state in &mut mapped.by_owner.get_mut(&owner).unwrap().shroud_knowledge {
        if state.local_sources > 0 {
            state.local_sources = 0;
            state.counter = 0;
            state.transient_visible = true;
        }
    }
    let sight = std::mem::replace(&mut sim.fog, mapped);
    assert_ne!(sim.state_hash(), sight_hash);
    assert_eq!(
        sim.state_hash_without_sustained_gap_sight_v142(),
        historical_hash
    );
    apply_gap_generators(&mut sim.fog, &[(gapper, 10, 10, 3)], &sim.interner);
    assert!(!sim.fog.is_cell_revealed(owner, 10, 10));
    sim.fog = sight;
    apply_gap_generators(&mut sim.fog, &[(gapper, 10, 10, 3)], &sim.interner);
    assert!(sim.fog.is_cell_revealed(owner, 10, 10));
}

#[test]
fn shroud_current_sight_direct_allies_do_not_reexport_immunity() {
    let a = intern::test_intern("Americans");
    let b = intern::test_intern("Alliance");
    let c = intern::test_intern("Soviet");
    let hostile = intern::test_intern("Neutral");
    let mut alliances = HouseAllianceMap::new();
    for (left, right) in [
        ("AMERICANS", "ALLIANCE"),
        ("ALLIANCE", "AMERICANS"),
        ("ALLIANCE", "SOVIET"),
        ("SOVIET", "ALLIANCE"),
    ] {
        alliances
            .entry(left.into())
            .or_default()
            .insert(right.into());
    }
    let mut store = EntityStore::new();
    spawn_with_vision(&mut store, 1, "Americans", 10, 10, 3);
    let interner = ti();
    let mut fog = FogState {
        width: 32,
        height: 32,
        ..Default::default()
    };
    for owner in [a, b, c] {
        fog.by_owner.insert(owner, OwnerVisibility::new(32, 32));
    }
    recompute_owner_visibility_in_place(
        &mut fog,
        &store,
        None,
        &alliances,
        &default_config(),
        None,
        &interner,
        None,
    );
    for _ in 0..3 {
        apply_gap_generators(&mut fog, &[(hostile, 10, 10, 3)], &interner);
        fog.build_merged_for(b, &interner);
        assert!(!fog.is_cell_gap_covered(b, 10, 10));
        fog.build_merged_for(c, &interner);
        assert!(
            fog.is_cell_gap_covered(c, 10, 10),
            "A's sight must not become B's local source on the next House publication"
        );
        assert!(!fog.is_cell_visible(c, 10, 10));
        assert!(!fog.is_cell_revealed(c, 10, 10));
    }
    apply_gap_generators(&mut fog, &[], &interner);
    fog.build_merged_for(c, &interner);
    assert!(!fog.is_cell_visible(c, 10, 10));
    assert!(!fog.is_cell_revealed(c, 10, 10));
    assert!(!fog.is_cell_gap_covered(c, 10, 10));
}

#[test]
fn shroud_current_sight_view_cache_observes_fresh_viewer_writers() {
    let owner = intern::test_intern("Americans");
    let interner = ti();
    let mut fog = FogState {
        width: 16,
        height: 16,
        ..Default::default()
    };
    fog.build_merged_for(owner, &interner);
    assert!(!fog.is_cell_revealed(owner, 6, 6));
    fog.reveal_all_for_owner(owner);
    assert!(fog.is_cell_revealed(owner, 6, 6));
    fog.build_merged_for(owner, &interner);
    fog.reset_explored_for_owner(owner);
    assert!(!fog.is_cell_revealed(owner, 6, 6));
    fog.build_merged_for(owner, &interner);
    fog.reveal_cells_for_owner(owner, [(6, 6)]);
    assert!(fog.is_cell_revealed(owner, 6, 6));
    assert!(!fog.is_cell_revealed(owner, 7, 6));
    fog.build_merged_for(owner, &interner);
    reveal_radius(&mut fog, owner, 7, 6, 2);
    assert!(fog.is_cell_visible(owner, 7, 6));
    assert!(fog.is_cell_revealed(owner, 7, 6));
}

#[test]
fn shroud_current_sight_unchanged_refresh_does_not_invent_a_return_event() {
    let owner = intern::test_intern("Americans");
    let gapper = intern::test_intern("Soviet");
    let interner = ti();
    let mut fog = FogState {
        width: 32,
        height: 32,
        ..Default::default()
    };
    let mut store = EntityStore::new();
    spawn_with_vision(&mut store, 1, "Americans", 10, 10, 2);
    let refresh = |fog: &mut FogState, store: &EntityStore| {
        recompute_owner_visibility_in_place(
            fog,
            store,
            None,
            &Default::default(),
            &default_config(),
            None,
            &interner,
            None,
        )
    };
    refresh(&mut fog, &store);
    let first = [(gapper, 10, 10, 3)];
    let both = [(gapper, 10, 10, 3), (gapper, 10, 10, 3)];
    apply_gap_generators(&mut fog, &first, &interner);
    store.remove(1);
    refresh(&mut fog, &store);
    apply_gap_generators(&mut fog, &both, &interner);
    assert!(fog.is_cell_gap_covered(owner, 10, 10));
    spawn_with_vision(&mut store, 1, "Americans", 10, 10, 2);
    reveal_entity_vision(
        &mut fog,
        store.get(1).unwrap(),
        &default_config(),
        None,
        false,
        &interner,
    );
    assert!(fog.is_cell_revealed(owner, 10, 10));
    assert!(
        !fog.is_cell_gap_covered(owner, 10, 10),
        "writer publishes immediately, before House reconciliation"
    );
    let mut fog: FogState = bincode::deserialize(&bincode::serialize(&fog).unwrap()).unwrap();
    for _ in 0..3 {
        refresh(&mut fog, &store);
        apply_gap_generators(&mut fog, &both, &interner);
        fog.build_merged_for(owner, &interner);
    }
    fog.flush_pending_gap_conceal(120);
    assert!(
        !fog.is_cell_revealed(owner, 10, 10),
        "original second-gap/return retains pending despite negative counter"
    );
    refresh(&mut fog, &store);
    apply_gap_generators(&mut fog, &both, &interner);
    assert!(
        !fog.is_cell_revealed(owner, 10, 10),
        "unchanged source cannot reopen after the sweep"
    );
    // Explicit native release/admit is a different event even with identical
    // geometry; this API does not claim the full FootAI timer producer is wired.
    force_refresh_entity_vision(
        &mut fog,
        store.get(1).unwrap(),
        &default_config(),
        None,
        false,
        &interner,
    );
    assert!(fog.is_cell_revealed(owner, 10, 10));
    assert!(!fog.is_cell_gap_covered(owner, 10, 10));
}

#[test]
fn shroud_current_sight_first_fire_and_psychic_have_distinct_pending_history() {
    let owner = intern::test_intern("Americans");
    let interner = ti();
    let mut fire = FogState {
        width: 32,
        height: 32,
        ..Default::default()
    };
    let mut psychic = fire.clone();
    reveal_radius(&mut fire, owner, 10, 10, 2);
    reveal_radius_for_direct_allies(&mut psychic, owner, 10, 10, 2, &interner);
    assert!(fire.is_cell_revealed(owner, 10, 10));
    assert!(psychic.is_cell_revealed(owner, 10, 10));
    fire.flush_pending_gap_conceal(120);
    psychic.flush_pending_gap_conceal(120);
    assert!(!fire.is_cell_revealed(owner, 10, 10));
    assert!(psychic.is_cell_revealed(owner, 10, 10));
}

#[test]
fn shroud_current_sight_timer_gate_matches_original_signed_frame_cases() {
    let corpus: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/shroud_current_sight.json"
    ))
    .unwrap();
    let cases = corpus["timer_cases"].as_array().unwrap();
    assert_eq!(cases.len(), 9);
    for case in cases {
        let timer = crate::sim::timer::CdTimer::from_raw(
            case["start"].as_i64().unwrap() as i32,
            case["duration"].as_i64().unwrap() as i32,
        );
        assert_eq!(
            timer.expired(case["frame"].as_i64().unwrap() as i32),
            case["due"].as_bool().unwrap(),
            "{case}"
        );
    }
}
