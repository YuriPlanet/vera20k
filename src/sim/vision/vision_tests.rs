//! Tests for the fog/shroud visibility system.

use super::*;
use crate::map::entities::EntityCategory;
use crate::sim::combat::Health;
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern;
use crate::util::save_codec;

/// Persistent production Simulation for source/frame regression fixtures.
/// Only fixture input construction lives here; all admission, Cell writes,
/// frame projection and synchronous discovery run through their real owners.
///
/// A store swap moves no sight by itself: a fixture that drops or adds an
/// object runs that object's sight event ([`fixture_frame_with_sight`]).
fn fixture_frame(
    world: &mut crate::sim::world::Simulation,
    fog: &mut FogState,
    entities: &EntityStore,
    path_grid: Option<&PathGrid>,
    alliances: &HouseAllianceMap,
    config: &VisionConfig,
    height_grid: Option<&[u8]>,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
    interner: &intern::StringInterner,
    rules: Option<&crate::rules::ruleset::RuleSet>,
) {
    fixture_frame_with_sight(
        world,
        fog,
        entities,
        path_grid,
        alliances,
        config,
        height_grid,
        terrain,
        interner,
        rules,
        &[],
    );
}

/// The sight event of an object a fixture's store swap drops or adds.
#[derive(Clone, Copy)]
enum FixtureSight {
    /// Its Limbo's ReReveal (`0x006F6B16`) takes its sight off the maps.
    Limbo(u64),
    /// Its Unlimbo's UpdateReveal (`0x006F6D27`) opens its sight.
    Unlimbo(u64),
}

/// [`fixture_frame`] that first runs `sight`, in order, through the sight
/// owner, after the swap installs the store.
fn fixture_frame_with_sight(
    world: &mut crate::sim::world::Simulation,
    fog: &mut FogState,
    entities: &EntityStore,
    path_grid: Option<&PathGrid>,
    alliances: &HouseAllianceMap,
    config: &VisionConfig,
    height_grid: Option<&[u8]>,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
    interner: &intern::StringInterner,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    sight: &[FixtureSight],
) {
    assert!(
        height_grid.is_none(),
        "fixture heights belong in its PathGrid"
    );
    world.fog = std::mem::take(fog);
    world.substrate.entities = entities.clone();
    world.interner = interner.clone();
    *world.house_alliances_mut_for_test() = alliances.clone();
    world.resolved_terrain = terrain.cloned();
    world.path_grid = path_grid.cloned().map(std::sync::Arc::new);
    let context = crate::sim::world::UninitContext::new(
        rules.unwrap_or(crate::rules::ruleset::RuleSet::empty_for_test()),
        None,
    );
    for &event in sight {
        match event {
            FixtureSight::Limbo(id) => world.release_object_sight_with_context(id, context),
            FixtureSight::Unlimbo(id) => {
                let heights = world.sight_height_grid(config);
                world.update_object_sight_in_view(id, 0, None, config, Some(&heights), context);
            }
        }
    }
    world.refresh_sight_sources(
        config,
        rules.unwrap_or(crate::rules::ruleset::RuleSet::empty_for_test()),
    );
    *fog = std::mem::take(&mut world.fog);
}

fn fixture_visibility(
    entities: &EntityStore,
    grid: Option<&PathGrid>,
    alliances: &HouseAllianceMap,
    config: &VisionConfig,
    interner: &intern::StringInterner,
) -> FogState {
    fixture_visibility_with_rules(entities, grid, alliances, config, interner, None)
}

fn fixture_visibility_with_rules(
    entities: &EntityStore,
    grid: Option<&PathGrid>,
    alliances: &HouseAllianceMap,
    config: &VisionConfig,
    interner: &intern::StringInterner,
    rules: Option<&crate::rules::ruleset::RuleSet>,
) -> FogState {
    let mut world = crate::sim::world::Simulation::default();
    let mut fog = FogState::default();
    fixture_frame(
        &mut world, &mut fog, entities, grid, alliances, config, None, None, interner, rules,
    );
    fog
}

fn fixture_source_event(
    world: &mut crate::sim::world::Simulation,
    fog: &mut FogState,
    entities: &EntityStore,
    config: &VisionConfig,
    interner: &intern::StringInterner,
    refresh: bool,
) {
    world.fog = std::mem::take(fog);
    world.substrate.entities = entities.clone();
    world.interner = interner.clone();
    let context = crate::sim::world::UninitContext::default();
    if refresh {
        world.release_object_sight_with_context(1, context);
    }
    world.update_object_sight_in_view(1, 0, None, config, None, context);
    *fog = std::mem::take(&mut world.fog);
}

fn spawn_with_vision(store: &mut EntityStore, id: u64, owner: &str, rx: u16, ry: u16, range: u16) {
    let mut entity = GameEntity::new_at_frame_zero_for_test(
        id,
        rx,
        ry,
        0,
        0,
        intern::test_intern(owner),
        Health::new(100),
        intern::test_intern("E1"),
        EntityCategory::Infantry,
        0,
        range,
        false,
    );
    entity.lifecycle.set_in_limbo_for_test(false);
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
fn fogged_footprints_unlink_shared_ids_in_reverse_order() {
    let viewer = intern::test_intern("Americans");
    let mut fog = FogState {
        width: 8,
        height: 8,
        ..FogState::default()
    };
    let first = fog.insert_fogged_object_footprint(viewer, 11, vec![(2, 2), (3, 2)]);
    let second = fog.insert_fogged_object_footprint(viewer, 22, vec![(2, 2)]);

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
fn owner_visibility_ground_bits_outlive_the_sight_that_set_them() {
    let mut vis = OwnerVisibility::new(2, 2);
    assert_eq!(vis.ground_flags_raw()[3], 0);
    vis.mark_visible(1, 1);
    vis.mark_visible(1, 1);
    assert_eq!(vis.ground_flags_raw()[3], 0x18);

    vis.clear_all_visible();

    assert_eq!(vis.ground_flags_raw()[3], 0x18);
    assert_eq!(vis.ground_flags_raw()[0], 0, "an unseen cell keeps none");
    assert!(!vis.is_visible(1, 1));
    assert!(
        vis.is_revealed(1, 1),
        "clearing sight never clears exploration"
    );
}

// -- Recompute visibility integration tests --

#[test]
fn techno_playfield_stored_membership_gates_same_frame_los() {
    let mut store = EntityStore::new();
    spawn_with_vision(&mut store, 1, "Americans", 5, 5, 2);
    let config = VisionConfig {
        require_playfield_membership: true,
        ..default_config()
    };

    let suppressed = fixture_visibility(
        &store,
        Some(&PathGrid::new(16, 16)),
        &Default::default(),
        &config,
        &ti(),
    );
    assert!(!suppressed.is_cell_visible(intern::test_intern("Americans"), 5, 5));

    store.get_mut(1).unwrap().set_in_playfield(true);
    let admitted = fixture_visibility(
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

    let fog = fixture_visibility(
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
fn test_allied_visibility_is_shared() {
    // The real House registry interns every viewer before source admission.
    let _alliance = intern::test_intern("Alliance");
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

    let fog = fixture_visibility(
        &store,
        Some(&PathGrid::new(16, 16)),
        &alliances,
        &default_config(),
        &ti(),
    );
    // The viewer plane already holds the direct ally's published knowledge.
    assert!(fog.is_cell_visible(intern::test_intern("Alliance"), 4, 4));
}

// -- Sight cap tests --

#[test]
fn test_sight_capped_at_max_range() {
    let mut store = EntityStore::new();
    // Spawn with sight=15, which exceeds MAX_SIGHT_RANGE (10).
    spawn_with_vision(&mut store, 1, "Americans", 20, 20, 15);

    let fog = fixture_visibility(
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
            Health::new(100),
            intern::test_intern(type_id),
            EntityCategory::Infantry,
            100, // veteran
            5,   // vision_range
            false,
        );
        entity.lifecycle.set_in_limbo_for_test(false);
        store.insert(entity);
        let config = VisionConfig {
            require_playfield_membership: false,
            veteran_sight,
            leptons_per_sight_increase: 0,
            reveal_by_height: false,
            ..default_config()
        };
        fixture_visibility_with_rules(
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
        Health::new(100),
        intern::test_intern("E1"),
        EntityCategory::Infantry,
        0,
        5,
        false,
    );
    entity.lifecycle.set_in_limbo_for_test(false);
    store.insert(entity);
    let config = VisionConfig {
        require_playfield_membership: false,
        veteran_sight: 0.0,
        leptons_per_sight_increase: 2000,
        reveal_by_height: false,
        ..default_config()
    };
    let fog = fixture_visibility(
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

    let mut fog = fixture_visibility(
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
    apply_gap_generators(
        &mut fog,
        &HouseAllianceMap::new(),
        &[(americans_id, 12, 10, 5)],
        &interner,
    );

    // Original current_sight keeps the admitted negative counter open even
    // inside a hostile gap. Merely knowing the cell would not protect it.
    assert!(fog.is_cell_visible(intern::test_intern("Soviet"), 13, 10));
    assert!(!fog.is_cell_gap_covered(intern::test_intern("Soviet"), 13, 10));
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

    apply_gap_generators(
        &mut fog,
        &HouseAllianceMap::new(),
        &[(gapper, 16, 16, 5)],
        &interner,
    );
    assert!(!fog.is_cell_visible(viewer, 16, 16));
    assert!(!fog.is_cell_revealed(viewer, 16, 16));
    assert!(fog.is_cell_gap_covered(viewer, 16, 16));

    fog.by_owner
        .get_mut(&viewer)
        .expect("viewer plane")
        .clear_all_visible();
    apply_gap_generators(&mut fog, &HouseAllianceMap::new(), &[], &interner);
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
    apply_gap_generators(
        &mut fog,
        &HouseAllianceMap::new(),
        &[(gapper, 16, 16, 5)],
        &interner,
    );
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
    apply_gap_generators(
        &mut fog,
        &HouseAllianceMap::new(),
        &[(gapper, 16, 16, 5)],
        &interner,
    );
    assert!(
        !fog.is_cell_revealed(viewer, 16, 16),
        "the active hostile gap remains the final writer"
    );

    fog.mark_visible_for_owner(gapper, 16, 16);
    apply_gap_generators(
        &mut fog,
        &HouseAllianceMap::new(),
        &[(gapper, 16, 16, 5)],
        &interner,
    );
    assert!(fog.is_cell_visible(gapper, 16, 16));
    assert!(fog.is_cell_revealed(gapper, 16, 16));
    assert!(fog.is_cell_gap_fog(gapper, 16, 16));
}

// -- In-place recompute tests --

#[test]
fn test_in_place_preserves_revealed() {
    let mut sight_world = crate::sim::world::Simulation::default();
    let mut store = EntityStore::new();
    spawn_with_vision(&mut store, 1, "Americans", 5, 5, 2);
    let grid = PathGrid::new(16, 16);
    let cfg = default_config();
    let alliances = HouseAllianceMap::default();

    // First compute: reveals cells around (5,5).
    let mut fog = FogState::default();
    fixture_frame(
        &mut sight_world,
        &mut fog,
        &store,
        Some(&grid),
        &alliances,
        &cfg,
        None,
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
    fixture_frame_with_sight(
        &mut sight_world,
        &mut fog,
        &store,
        Some(&grid),
        &alliances,
        &cfg,
        None,
        None,
        &ti(),
        None,
        &[FixtureSight::Limbo(1), FixtureSight::Unlimbo(2)],
    );
    assert!(fog.is_cell_revealed(intern::test_intern("Americans"), 5, 5));
    assert!(!fog.is_cell_visible(intern::test_intern("Americans"), 5, 5));
    assert!(fog.is_cell_visible(intern::test_intern("Americans"), 10, 10));
}

/// Rust regression test: a frame pass over a tracked map after a shroud-edge
/// sweep closed two of its cells leaves what a pass that visits every cell and
/// footprint leaves. One swept cell lies under a retained footprint (its pending
/// bit set directly: only the latch and alternate paths leave a covered cell
/// pending) and gets its ground bits back; the other lies under an alternate
/// footprint alone, which the pass does not mark, and keeps them off.
#[test]
fn a_tracked_pass_after_a_sweep_marks_the_swept_cells_a_footprint_covers() {
    let mut sight_world = crate::sim::world::Simulation::default();
    let americans = intern::test_intern("Americans");
    let mut store = EntityStore::new();
    spawn_with_vision(&mut store, 1, "Americans", 8, 8, 3);
    let grid = PathGrid::new(24, 24);
    let cfg = default_config();
    let alliances = HouseAllianceMap::default();
    let interner = ti();
    let pass = |sight_world: &mut crate::sim::world::Simulation, fog: &mut FogState| {
        fixture_frame(
            sight_world,
            fog,
            &store,
            Some(&grid),
            &alliances,
            &cfg,
            None,
            None,
            &interner,
            None,
        )
    };
    let mut fog = FogState::default();
    pass(&mut sight_world, &mut fog);
    sight_world.fog = std::mem::take(&mut fog);
    let alternate = SightAdmission {
        latched: true,
        owner: intern::test_intern("Soviet"),
        origin: (18, 18, 0),
        radius: 1,
        cells: collect_reveal_cells(18, 18, 1, 0, false, None, (24, 24), None),
    };
    sight_world
        .fog
        .retain_sight_admission(2, americans, alternate, true);
    let _ = sight_world.reveal_area(
        &[americans],
        (18, 18),
        0,
        1,
        None,
        false,
        &cfg,
        crate::sim::world::UninitContext::default(),
        crate::sim::vision::MapRevealProjection::Transient,
        sight_world.session.current_house(),
    );
    fog = std::mem::take(&mut sight_world.fog);
    let (covered, alternate) = ((9, 8), (18, 18));
    for (x, y) in [covered, alternate] {
        let vis = fog.by_owner.get_mut(&americans).unwrap();
        let index = vis.index(x, y).unwrap();
        vis.shroud_knowledge[index].set_pending_for_test(true);
        vis.publish_knowledge(index);
        fog.sweep_pending_map_cell(americans, (x, y), None);
    }
    let ground = |fog: &FogState, (x, y): (u16, u16)| {
        let vis = &fog.by_owner[&americans];
        vis.ground_flags_raw()[vis.index(x, y).unwrap()] & (GROUND_VISIBLE | GROUND_OPEN)
    };
    assert_eq!(ground(&fog, covered), 0, "the sweep closes the cell");
    let mut every_cell = fog.clone();
    pass(&mut sight_world, &mut fog);
    VISIT_EVERY_CELL.set(true);
    pass(&mut sight_world, &mut every_cell);
    VISIT_EVERY_CELL.set(false);
    assert_eq!(ground(&fog, covered), GROUND_VISIBLE | GROUND_OPEN);
    assert_eq!(ground(&fog, alternate), 0);
    let tracked = &fog.by_owner[&americans];
    let every_cell = &every_cell.by_owner[&americans];
    assert_eq!(tracked.cells_raw(), every_cell.cells_raw());
    assert_eq!(tracked.ground_flags_raw(), every_cell.ground_flags_raw());
    assert_eq!(
        tracked.shroud_knowledge_raw(),
        every_cell.shroud_knowledge_raw()
    );
}

// -- Height-based LOS (RevealByHeight) tests --

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
    reveal_radius_into(
        &mut vis,
        5,
        5,
        5,
        0,
        true,
        Some(CellLevels::Plane(&hg)),
        width,
        height,
    );
    assert!(
        vis.is_visible(8, 5),
        "naive (7,5) cliff must not block with the +2 offset"
    );

    // Cliff at the +2 location (9,7) must block (8,5).
    let mut vis2 = OwnerVisibility::new(20, 20);
    let mut hg2 = vec![0u8; width as usize * height as usize];
    hg2[7 * width as usize + 9] = 5; // (9,7) — the actual obstruction cell
    reveal_radius_into(
        &mut vis2,
        5,
        5,
        5,
        0,
        true,
        Some(CellLevels::Plane(&hg2)),
        width,
        height,
    );
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
    reveal_radius_into(
        &mut low,
        5,
        5,
        5,
        0,
        true,
        Some(CellLevels::Plane(&hg)),
        width,
        height,
    );
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
        Some(CellLevels::Plane(&hg)),
        width,
        height,
    );
    assert!(
        high.is_visible(6, 3),
        "high viewer sees past the cliff (reveal cell shifted to (6,3))"
    );
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
    apply_gap_generators(
        &mut fog,
        &HouseAllianceMap::new(),
        &[(gapper, 30, 30, 10)],
        &interner,
    );

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
    apply_gap_generators(
        &mut fog,
        &HouseAllianceMap::new(),
        &[(gapper, 30, 30, 10)],
        &interner,
    );

    assert!(fog.is_cell_gap_fog(gapper, 30, 35)); // friendly => fog
    assert!(!fog.is_cell_gap_covered(gapper, 30, 35)); // not black
    assert!(fog.is_cell_visible(gapper, 30, 35)); // own vision NOT suppressed
}

/// A Building's gap event asks from the generator's house whether the viewer
/// is its ally (`0x006FB2CF`, `HouseClass::IsAlliedWith @ 0x004F9A50`): the
/// viewer's own one-way alliance with it leaves the viewer under the gap.
#[test]
fn a_gap_event_asks_the_generators_house_about_the_viewer() {
    let viewer = intern::test_intern("Soviet");
    let gapper = intern::test_intern("Americans");
    let interner = ti();
    let source = GapGeneratorSource {
        stable_id: 1,
        owner: gapper,
        rx: 30,
        ry: 30,
        radius: 10,
    };
    let covered = |asker: &str, other: &str| {
        let mut fog = FogState {
            width: 64,
            height: 64,
            ..Default::default()
        };
        fog.mark_visible_for_owner(viewer, 30, 35);
        let alliances = HouseAllianceMap::from([(
            asker.to_ascii_uppercase(),
            [other.to_ascii_uppercase()].into(),
        )]);
        publish_gap_generator_event(
            &mut fog,
            &alliances,
            viewer,
            source,
            true,
            &interner,
            &std::collections::BTreeSet::new(),
        );
        fog.is_cell_gap_covered(viewer, 30, 35)
    };
    assert!(covered("Soviet", "Americans"));
    assert!(!covered("Americans", "Soviet"));
}

/// Materializing a viewer's retained deposits asks from each generator's house
/// as the event did: only the generator's alliance with the viewer projects
/// the friendly gap fog.
#[test]
fn materialized_gap_fog_asks_the_generators_house_about_the_viewer() {
    let viewer = intern::test_intern("Soviet");
    let gapper = intern::test_intern("Americans");
    let interner = ti();
    let source = GapGeneratorSource {
        stable_id: 1,
        owner: gapper,
        rx: 30,
        ry: 30,
        radius: 10,
    };
    let fogged = |asker: &str, other: &str| {
        let mut fog = FogState {
            width: 64,
            height: 64,
            ..Default::default()
        };
        fog.mark_visible_for_owner(viewer, 30, 35);
        let alliances = HouseAllianceMap::from([(
            asker.to_ascii_uppercase(),
            [other.to_ascii_uppercase()].into(),
        )]);
        materialize_gap_generator_sources(
            &mut fog,
            &alliances,
            &BTreeMap::from([(viewer, vec![source])]),
            &interner,
        );
        fog.is_cell_gap_fog(viewer, 30, 35)
    };
    assert!(fogged("Americans", "Soviet"));
    assert!(!fogged("Soviet", "Americans"));
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
            Some(CellLevels::Plane(&flat)),
            w,
            h,
        );
        super::reveal_radius_into(&mut without_los, 32, 32, range, 0, false, None, w, h);

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

// -- Reveal footprint: the engine's spiral table --

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
    super::reveal_radius_into(&mut vis, 32, 32, 10, 0, false, None, w, h);

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
    super::reveal_radius_into(
        &mut open,
        32,
        32,
        10,
        0,
        true,
        Some(CellLevels::Plane(&flat)),
        w,
        h,
    );
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
        Some(CellLevels::Plane(&heights)),
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
    super::reveal_radius_into(&mut vis, 8, 8, 0, 0, false, None, w, h);
    let any = (0..h).any(|ry| (0..w).any(|rx| vis.is_revealed(rx, ry)));
    assert!(!any, "a Sight=0 object must not open a hole in the shroud");
}

// -- Reveal centre height shift --

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
    for (x, y) in super::collect_reveal_cells(30, 30, 1, 1500, false, None, (w, h), None) {
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
    for (x, y) in super::collect_reveal_cells(
        30,
        30,
        5,
        0,
        true,
        Some(CellLevels::Plane(&heights)),
        (w, h),
        None,
    ) {
        grounded.mark_visible(x, y);
    }
    let mut flying = OwnerVisibility::new(w, h);
    for (x, y) in super::collect_reveal_cells(
        30,
        30,
        5,
        1500,
        true,
        Some(CellLevels::Plane(&heights)),
        (w, h),
        None,
    ) {
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

/// A Jumpjet whose Location Z lags below its cell's floor sees from that Z:
/// on flat ground, the stored level plus its negative GetHeight is the
/// Location Z.
#[test]
fn a_jumpjet_below_its_floor_sees_from_its_location_z() {
    let mut sight_world = crate::sim::world::Simulation::default();
    use crate::map::resolved_terrain::{ResolvedTerrainGrid, test_flat_cell};
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::sim::movement::locomotor::{LocomotorState, MovementLayer};

    let (w, h) = (64u16, 64u16);
    let terrain = ResolvedTerrainGrid::from_cells(
        w,
        h,
        (0..h)
            .flat_map(|y| {
                (0..w).map(move |x| {
                    let mut cell = test_flat_cell(x, y);
                    cell.level = 4;
                    cell
                })
            })
            .collect(),
    );
    let mut store = EntityStore::new();
    let mut entity = GameEntity::test_default(1, "JUMPJET", "Americans", 30, 30);
    entity.lifecycle.set_in_limbo_for_test(false);
    entity.sight.set_vision_range_for_test(1);
    entity.position.set_z_for_test(4);
    entity.position.set_exact_z_leptons_for_test(Some(208));
    let mut loco = LocomotorState::for_test_kind(LocomotorKind::Jumpjet);
    loco.set_layer_for_test(MovementLayer::Air);
    entity.locomotor = Some(loco);
    store.insert(entity);

    let mut fog = FogState::default();
    fixture_frame(
        &mut sight_world,
        &mut fog,
        &store,
        Some(&PathGrid::new(w, h)),
        &Default::default(),
        &default_config(),
        None,
        Some(&terrain),
        &ti(),
        None,
    );
    let owner = intern::test_intern("Americans");
    let expected = super::collect_reveal_cells(30, 30, 1, 208, false, None, (w, h), None);
    assert!(!expected.is_empty());
    for ry in 0..h {
        for rx in 0..w {
            assert_eq!(
                fog.is_cell_visible(owner, rx, ry),
                expected.contains(&(rx, ry)),
                "({rx}, {ry})"
            );
        }
    }
}

/// A ground viewer sees from its Location Z (`0x0070AF9D`), not its stored
/// level: raised four levels over level-0 ground, its disc lifts like a
/// level-4 viewer's.
#[test]
fn a_ground_viewer_sees_from_its_location_z() {
    let (w, h) = (64u16, 64u16);
    let mut store = EntityStore::new();
    spawn_with_vision(&mut store, 1, "Americans", 30, 30, 1);
    store
        .get_mut(1)
        .unwrap()
        .position
        .set_exact_z_leptons_for_test(Some(4 * 104));
    let fog = fixture_visibility(
        &store,
        Some(&PathGrid::new(w, h)),
        &Default::default(),
        &default_config(),
        &ti(),
    );
    let owner = intern::test_intern("Americans");
    let expected = super::collect_reveal_cells(30, 30, 1, 4 * 104, false, None, (w, h), None);
    assert_ne!(
        expected,
        super::collect_reveal_cells(30, 30, 1, 0, false, None, (w, h), None),
        "precondition: the lift moves the disc"
    );
    for ry in 0..h {
        for rx in 0..w {
            assert_eq!(
                fog.is_cell_visible(owner, rx, ry),
                expected.contains(&(rx, ry)),
                "({rx}, {ry})"
            );
        }
    }
}

/// F10: the view generation render dirty-gates on is runtime-only. A bincode
/// round trip (the snapshot serializer) restarts it, and each prepared view
/// bumps it once.
#[test]
fn fog_view_generation_restarts_after_load_and_bumps_per_view() {
    let mut fog = FogState::default();
    fog.bump_view_generation();
    assert_eq!(fog.view_generation(), 1);

    let bytes = save_codec::serialize(&fog).expect("fog serializes");
    let mut restored: FogState = save_codec::deserialize(&bytes).expect("fog deserializes");
    assert_eq!(
        restored.view_generation(),
        0,
        "the runtime view generation restarts after a load"
    );
    restored.bump_view_generation();
    restored.bump_view_generation();
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
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/shroud_current_sight.json",
    ))
    .unwrap();
    assert_eq!(corpus["cases"].as_array().unwrap().len(), 23);
    let owner = intern::test_intern("Americans");
    let gapper = intern::test_intern("Soviet");
    let interner = ti();
    for case in corpus["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let mut sight_world = crate::sim::world::Simulation::default();
        sight_world.interner = interner.clone();
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
                    // These corpus operations are actual native UpdateReveal/
                    // ReReveal events, not a Logic-frame source reconciliation.
                    // Keep one production Simulation/cache for the entire case,
                    // including leave followed by reuse of the same source ID.
                    if op == "reveal" {
                        count += 1;
                        spawn_with_vision(&mut store, count, "Americans", 10, 10, 2);
                    }
                    sight_world.fog = std::mem::take(&mut fog);
                    sight_world.substrate.entities = store.clone();
                    let context = crate::sim::world::UninitContext::default();
                    if op == "reveal" {
                        sight_world.update_object_sight_in_view(
                            count,
                            0,
                            None,
                            &default_config(),
                            None,
                            context,
                        );
                    } else {
                        sight_world.release_object_sight_with_context(count, context);
                        store.remove(count);
                        count -= 1;
                    }
                    fog = std::mem::take(&mut sight_world.fog);
                    apply_gap_generators(&mut fog, &HouseAllianceMap::new(), &gaps, &interner);
                }
                "unshroud" => reveal_radius(&mut fog, owner, 10, 10, 2),
                "map_reveal_bulk" => fog.reveal_all_for_owner(owner),
                "map_reset_bulk" => fog.reset_explored_for_owner(owner),
                "gap" => {
                    gaps.push((gapper, 10, 10, 3));
                    apply_gap_generators(&mut fog, &HouseAllianceMap::new(), &gaps, &interner);
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
                        &mut fog,
                        &HouseAllianceMap::new(),
                        &sources,
                        &interner,
                        &map_clear,
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
                state.counter(),
                observation["counter"].as_i64().unwrap() as i32,
                "{name}/{op}/counter"
            );
            assert_eq!(
                state.gap_counter(),
                observation["gap"].as_i64().unwrap() as i32,
                "{name}/{op}/gap"
            );
            assert_eq!(
                state.pending(),
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
    let mut sight_world = crate::sim::world::Simulation::default();
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
    fixture_frame_with_sight(
        &mut sight_world,
        &mut fog,
        &store,
        None,
        &alliances,
        &default_config(),
        None,
        None,
        &interner,
        None,
        &[FixtureSight::Unlimbo(1)],
    );
    apply_gap_generators(&mut fog, &alliances, &[(gapper, 10, 10, 3)], &interner);
    assert!(fog.is_cell_visible(owner, 10, 10));
    assert!(!fog.is_cell_gap_covered(owner, 10, 10));
    let encoded = save_codec::serialize(&fog).unwrap();
    let mut restored: FogState = save_codec::deserialize(&encoded).unwrap();
    apply_gap_generators(&mut restored, &alliances, &[(gapper, 10, 10, 3)], &interner);
    assert!(restored.is_cell_revealed(owner, 10, 10));
    store.remove(1);
    fixture_frame_with_sight(
        &mut sight_world,
        &mut restored,
        &store,
        None,
        &alliances,
        &default_config(),
        None,
        None,
        &interner,
        None,
        &[FixtureSight::Limbo(1)],
    );
    apply_gap_generators(&mut restored, &alliances, &[(gapper, 10, 10, 3)], &interner);
    let encoded = save_codec::serialize(&restored).unwrap();
    let mut restored: FogState = save_codec::deserialize(&encoded).unwrap();
    apply_gap_generators(&mut restored, &alliances, &[], &interner); // Removal does not cancel native pending20.
    restored.flush_pending_gap_conceal(119);
    assert!(restored.is_cell_revealed(owner, 10, 10));
    restored.flush_pending_gap_conceal(120);
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
    sim.fog = fixture_visibility(
        &store,
        Some(&PathGrid::new(32, 32)),
        &Default::default(),
        &default_config(),
        &sim.interner,
    );
    let sight_hash = sim.state_hash();
    let mut mapped = sim.fog.clone();
    // Keep the public observation identical, but replace the source's selected
    // state with a past mapping receipt.
    mapped.sight_admissions.clear();
    for state in &mut mapped.by_owner.get_mut(&owner).unwrap().shroud_knowledge {
        if state.local_sources() > 0 {
            state.set_local_sources_for_test(0);
            state.set_counter_for_test(0);
            state.set_transient_visible_for_test(true);
        }
    }
    let sight = std::mem::replace(&mut sim.fog, mapped);
    assert_ne!(sim.state_hash(), sight_hash);
    apply_gap_generators(
        &mut sim.fog,
        sim.house_alliances.as_map(),
        &[(gapper, 10, 10, 3)],
        &sim.interner,
    );
    assert!(!sim.fog.is_cell_revealed(owner, 10, 10));
    sim.fog = sight;
    apply_gap_generators(
        &mut sim.fog,
        sim.house_alliances.as_map(),
        &[(gapper, 10, 10, 3)],
        &sim.interner,
    );
    assert!(sim.fog.is_cell_revealed(owner, 10, 10));
}

#[test]
fn shroud_current_sight_direct_allies_do_not_reexport_immunity() {
    let mut sight_world = crate::sim::world::Simulation::default();
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
    fixture_frame_with_sight(
        &mut sight_world,
        &mut fog,
        &store,
        None,
        &alliances,
        &default_config(),
        None,
        None,
        &interner,
        None,
        &[FixtureSight::Unlimbo(1)],
    );
    for _ in 0..3 {
        apply_gap_generators(&mut fog, &alliances, &[(hostile, 10, 10, 3)], &interner);
        assert!(!fog.is_cell_gap_covered(b, 10, 10));
        assert!(
            fog.is_cell_gap_covered(c, 10, 10),
            "A's sight must not become B's local source on the next House publication"
        );
        assert!(!fog.is_cell_visible(c, 10, 10));
        assert!(!fog.is_cell_revealed(c, 10, 10));
    }
    apply_gap_generators(&mut fog, &alliances, &[], &interner);
    assert!(!fog.is_cell_visible(c, 10, 10));
    assert!(!fog.is_cell_revealed(c, 10, 10));
    assert!(!fog.is_cell_gap_covered(c, 10, 10));
}

#[test]
fn shroud_current_sight_queries_observe_fresh_viewer_writers() {
    let owner = intern::test_intern("Americans");
    let mut fog = FogState {
        width: 16,
        height: 16,
        ..Default::default()
    };
    assert!(!fog.is_cell_revealed(owner, 6, 6));
    fog.reveal_all_for_owner(owner);
    assert!(fog.is_cell_revealed(owner, 6, 6));
    fog.reset_explored_for_owner(owner);
    assert!(!fog.is_cell_revealed(owner, 6, 6));
    fog.reveal_cells_for_owner(owner, [(6, 6)]);
    assert!(fog.is_cell_revealed(owner, 6, 6));
    assert!(!fog.is_cell_revealed(owner, 7, 6));
    reveal_radius(&mut fog, owner, 7, 6, 2);
    assert!(fog.is_cell_visible(owner, 7, 6));
    assert!(fog.is_cell_revealed(owner, 7, 6));
}

#[test]
fn shroud_current_sight_unchanged_refresh_does_not_invent_a_return_event() {
    let mut sight_world = crate::sim::world::Simulation::default();
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
    let refresh = |sight_world: &mut crate::sim::world::Simulation,
                   fog: &mut FogState,
                   store: &EntityStore,
                   sight: &[FixtureSight]| {
        fixture_frame_with_sight(
            sight_world,
            fog,
            store,
            None,
            &Default::default(),
            &default_config(),
            None,
            None,
            &interner,
            None,
            sight,
        )
    };
    refresh(
        &mut sight_world,
        &mut fog,
        &store,
        &[FixtureSight::Unlimbo(1)],
    );
    let first = [(gapper, 10, 10, 3)];
    let both = [(gapper, 10, 10, 3), (gapper, 10, 10, 3)];
    apply_gap_generators(&mut fog, &HouseAllianceMap::new(), &first, &interner);
    store.remove(1);
    refresh(
        &mut sight_world,
        &mut fog,
        &store,
        &[FixtureSight::Limbo(1)],
    );
    apply_gap_generators(&mut fog, &HouseAllianceMap::new(), &both, &interner);
    assert!(fog.is_cell_gap_covered(owner, 10, 10));
    spawn_with_vision(&mut store, 1, "Americans", 10, 10, 2);
    fixture_source_event(
        &mut sight_world,
        &mut fog,
        &store,
        &default_config(),
        &interner,
        false,
    );
    assert!(fog.is_cell_revealed(owner, 10, 10));
    assert!(
        !fog.is_cell_gap_covered(owner, 10, 10),
        "writer publishes immediately, before House reconciliation"
    );
    let mut fog: FogState = save_codec::deserialize(&save_codec::serialize(&fog).unwrap()).unwrap();
    for _ in 0..3 {
        refresh(&mut sight_world, &mut fog, &store, &[]);
        apply_gap_generators(&mut fog, &HouseAllianceMap::new(), &both, &interner);
    }
    fog.flush_pending_gap_conceal(120);
    assert!(
        !fog.is_cell_revealed(owner, 10, 10),
        "original second-gap/return retains pending despite negative counter"
    );
    refresh(&mut sight_world, &mut fog, &store, &[]);
    apply_gap_generators(&mut fog, &HouseAllianceMap::new(), &both, &interner);
    assert!(
        !fog.is_cell_revealed(owner, 10, 10),
        "unchanged source cannot reopen after the sweep"
    );
    // Explicit native release/admit is a different event even with identical
    // geometry; this API does not claim the full FootAI timer producer is wired.
    fixture_source_event(
        &mut sight_world,
        &mut fog,
        &store,
        &default_config(),
        &interner,
        true,
    );
    assert!(fog.is_cell_revealed(owner, 10, 10));
    assert!(!fog.is_cell_gap_covered(owner, 10, 10));
}

#[test]
fn shroud_current_sight_first_fire_and_psychic_have_distinct_pending_history() {
    let owner = intern::test_intern("Americans");
    let mut fire = FogState {
        width: 32,
        height: 32,
        ..Default::default()
    };
    let psychic = fire.clone();
    reveal_radius(&mut fire, owner, 10, 10, 2);
    let mut sim = crate::sim::world::Simulation::default();
    sim.fog = psychic;
    let config = default_config();
    for final_pass in [false, true] {
        let _ = sim.reveal_area(
            &[owner],
            (10, 10),
            0,
            2,
            None,
            final_pass,
            &config,
            crate::sim::world::UninitContext::default(),
            crate::sim::vision::MapRevealProjection::Transient,
            sim.session.current_house(),
        );
    }
    let mut psychic = sim.fog;
    assert!(fire.is_cell_revealed(owner, 10, 10));
    assert!(psychic.is_cell_revealed(owner, 10, 10));
    fire.flush_pending_gap_conceal(120);
    psychic.flush_pending_gap_conceal(120);
    assert!(!fire.is_cell_revealed(owner, 10, 10));
    assert!(psychic.is_cell_revealed(owner, 10, 10));
}

#[test]
fn shroud_current_sight_timer_gate_matches_original_signed_frame_cases() {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/shroud_current_sight.json",
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

/// Consume the executed561910 table, including the radius11 border tail.
/// Ring membership alone cannot establish native order or numeric identity.
#[test]
fn reveal_spiral_and_border_match_original_startup_table() {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/procedural_drawing_oracle/shroud_frontier.json",
    ))
    .unwrap();
    let native = &corpus["spiral"];
    assert!(
        corpus["inputs"]["startup"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry.as_str() == Some("00561910"))
    );
    assert_eq!(
        serde_json::json!(super::REVEAL_RING_SIZES),
        native["counts"]
    );
    assert_eq!(super::REVEAL_SPIRAL.len(), 369);
    let pairs: Vec<[i8; 2]> = super::REVEAL_SPIRAL.iter().map(|&(x, y)| [x, y]).collect();
    assert_eq!(serde_json::json!(pairs), native["pairs"]);
    // Zero-height whole-disc output must use this same ordered owner. Offset
    // translation is fixture input construction; expected offsets are native.
    for radius in [3_usize, 9, 11] {
        let actual: Vec<[i32; 2]> = super::sight_border_cells((32, 32, 0), radius as i32, 0)
            .into_iter()
            .map(|(x, y)| [i32::from(x) - 32, i32::from(y) - 32])
            .collect();
        let count = native["counts"][radius].as_u64().unwrap() as usize;
        assert_eq!(
            serde_json::json!(actual),
            serde_json::json!(&native["pairs"].as_array().unwrap()[..count])
        );
    }
}

/// Selected original70AF50/70B1D0 cache-control values. The production source
/// events run on an explicitly supplied numeric source; this does not model
/// the native nested callback, Anytown startup, or its map-wide Cell effects.
#[test]
fn source_release_and_zero_sight_gate_preserve_original_cached_metadata() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art() else {
        return;
    };
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/procedural_drawing_oracle/shroud_frontier.json",
    ))
    .unwrap();
    let native = &corpus["controls"]["cached_release"];
    assert_eq!(native["startup"]["b0cd48"], "60493474535ec23f");
    let input = &native["release_calls"]["005678E0"]["coords"];
    let coord = crate::sim::components::DriveCoord {
        x: input[0].as_i64().unwrap() as i32,
        y: input[1].as_i64().unwrap() as i32,
        z: input[2].as_i64().unwrap() as i32,
    };
    let mut world = crate::sim::world::Simulation::default();
    world.interner = ti();
    world.fog = FogState::new(128, 128);
    let owner = world.interner.intern("Americans");
    let amcv = rules.object("AMCV").unwrap();
    let mut entity = GameEntity::new_at_frame_zero_for_test(
        1,
        0,
        0,
        0,
        0,
        owner,
        Health::new(amcv.strength),
        world.interner.intern("AMCV"),
        EntityCategory::Unit,
        0,
        0,
        false,
    );
    entity.lifecycle.set_in_limbo_for_test(false);
    entity.position.put_location(coord);
    entity.sight.stamp_type_sight(amcv.sight);
    world.substrate.entities.insert(entity);
    let config = default_config();
    let context = crate::sim::world::UninitContext::default();
    world.update_object_sight_in_view(1, 0, None, &config, None, context);
    let assert_cache = |world: &crate::sim::world::Simulation, expected: &serde_json::Value| {
        let cache = &world.fog.sight_admissions[&(1, owner)];
        assert_eq!(
            u64::from(cache.latched),
            expected["latch"].as_u64().unwrap()
        );
        assert_eq!(
            i64::from(cache.radius),
            expected["radius"].as_i64().unwrap()
        );
        assert_eq!(
            cache.origin,
            (
                crate::util::lepton::lepton_to_cell(expected["xyz"][0].as_i64().unwrap() as i32)
                    as u16,
                crate::util::lepton::lepton_to_cell(expected["xyz"][1].as_i64().unwrap() as i32)
                    as u16,
                expected["xyz"][2].as_i64().unwrap() as i32,
            ),
        );
        let location = crate::sim::movement::ground_pose::object_location(
            world.entities().get(1).unwrap(),
            None,
        );
        assert_eq!(
            serde_json::json!([location.x, location.y, location.z]),
            expected["xyz"]
        );
    };
    assert_cache(&world, &native["nested"][0]["before"]);
    world.release_object_sight_with_context(1, context);
    assert_cache(&world, &native["nested"][0]["after"]);
    assert!(world.fog.sight_admissions[&(1, owner)].cells.is_empty());
    world
        .substrate
        .entities
        .get_mut(1)
        .unwrap()
        .sight
        .stamp_type_sight(0);
    world.update_object_sight_in_view(1, 0, None, &config, None, context);
    assert_cache(&world, &native["after_sight0_gate"]);
}
