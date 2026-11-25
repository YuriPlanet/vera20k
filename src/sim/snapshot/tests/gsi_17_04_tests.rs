//! Post-load rebuild of the native tiberium growth and spread queues: saved
//! queues are discarded and rebuilt over the serialized map rect from the
//! ground object list, without touching the shared Dummy cell.

use super::flat_terrain;
use crate::map::basic::BasicSection;
use crate::map::entities::EntityCategory;
use crate::map::map_file::MapHeader;
use crate::rules::ini_parser::IniFile;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::Health;
use crate::sim::game_entity::GameEntity;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::ore_growth::{OreGrowthConfig, OreGrowthState};
use crate::sim::overlay_grid::OverlayGrid;
use crate::sim::rng::SimRng;
use crate::sim::snapshot::{GameSnapshot, SnapshotRestoreError};
use crate::sim::terrain_object::TerrainObjectState;
use crate::sim::world::Simulation;
use std::collections::BTreeSet;

fn tiberium_fixture() -> (RuleSet, OverlayTypeRegistry, u8) {
    tiberium_fixture_with_general("")
}

/// [`tiberium_fixture`] with `general` added to `[General]`.
fn tiberium_fixture_with_general(general: &str) -> (RuleSet, OverlayTypeRegistry, u8) {
    let ini = IniFile::from_str(&format!(
        "[General]\n\
         TiberiumGrows=yes\n\
         TiberiumSpreads=yes\n\
         {general}\
         [InfantryTypes]\n\
         [VehicleTypes]\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         [Warheads]\n\
         [TerrainTypes]\n0=TREE01\n\
         [TREE01]\nFixtureOnly=1\n\
         [OverlayTypes]\n\
         0=ORE\n\
         [ORE]\n\
         Tiberium=yes\n\
         [Tiberiums]\n\
         0=Riparius\n\
         [Riparius]\n\
         Image=1\n\
         Growth=17\n\
         GrowthPercentage=1\n\
         Spread=23\n\
         SpreadPercentage=1\n",
    ));
    let ini = crate::rules::tiberium_type::with_tiberium_image_slots(&ini);
    let rules = RuleSet::from_ini(&ini).expect("post-load tiberium fixture rules");
    let registry = OverlayTypeRegistry::from_ini(&ini, None);
    let ore_id = registry.id_for_name("ORE").expect("fixture ORE id");
    (rules, registry, ore_id)
}

fn base_sim(ore_id: u8, cells: &[(u16, u16)]) -> Simulation {
    base_sim_sized(ore_id, 8, cells)
}

fn base_sim_sized(ore_id: u8, size: u16, cells: &[(u16, u16)]) -> Simulation {
    let mut sim = Simulation::with_seed(0x17_04);
    sim.session.set_binary_frame_for_test(91);
    sim.session.set_map_width_for_test(size);
    sim.session.set_map_height_for_test(size);
    *sim.ore_growth_config = OreGrowthConfig::for_test(true, true, false);
    *sim.ore_growth_state = OreGrowthState::new(size, size);
    let mut overlays = OverlayGrid::new(size, size);
    for &(rx, ry) in cells {
        overlays.place_overlay(rx, ry, ore_id, 5);
    }
    *sim.overlay_grid = Some(overlays);
    sim.install_resolved_terrain_for_new_map(flat_terrain(size, size));
    sim
}

fn map_header(width: u32, height: u32) -> MapHeader {
    MapHeader {
        theater: "TEMPERATE".to_string(),
        fill: "Clear".to_string(),
        level: 0,
        width,
        height,
        local_left: 0,
        local_top: 0,
        local_width: width,
        local_height: height,
    }
}

fn add_marked_unit(sim: &mut Simulation, cell: (u16, u16), on_bridge: bool) {
    let stable_id = sim.substrate.allocate_stable_id();
    let owner = sim.interner.intern("Americans");
    let type_ref = sim.interner.intern("TESTUNIT");
    let mut entity = GameEntity::new_at_frame_zero_for_test(
        stable_id,
        cell.0,
        cell.1,
        0,
        0,
        owner,
        Health::new(100),
        type_ref,
        EntityCategory::Unit,
        0,
        5,
        true,
    );
    entity.lifecycle.set_in_limbo_for_test(false);
    entity.seldom.in_logic_vector.set_for_test(true);
    entity.set_on_bridge(on_bridge);
    sim.substrate.entities.insert(entity);
    sim.add_entity_occupancy(stable_id);
    sim.substrate
        .logic
        .try_push(stable_id, crate::sim::world::ObjectKind::Entity)
        .expect("fixture LogicClass registration");
}

fn add_live_terrain_object(sim: &mut Simulation, cell: (u16, u16)) {
    let stable_id = sim.substrate.allocate_stable_id();
    let type_ref = sim.interner.intern("TREE01");
    sim.terrain_objects
        .objects_mut_for_test()
        .insert(stable_id, {
            let mut terrain = TerrainObjectState::for_test(stable_id, type_ref, cell.0, cell.1);
            terrain.set_occupation_bits_for_test(7);
            terrain
        });
    sim.terrain_objects
        .cell_index_mut_for_test()
        .insert(cell, stable_id);
}

fn seed_serialized_queue_state(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: &OverlayTypeRegistry,
    ore_cell: (u16, u16),
) {
    sim.ore_growth_state
        .reset_native_tiberium_classes(rules.tiberium_types.len(), 7);
    let overlay = sim.overlay_grid.as_ref().expect("fixture overlay grid");
    let resolved = sim.resolved_terrain.as_ref();
    let mut throwaway_rng = SimRng::new(13);
    assert!(
        sim.ore_growth_state
            .add_native_growth_queue_cell(
                overlay,
                registry,
                &rules.tiberium_types,
                resolved,
                true,
                registry
                    .tiberium_type_for_overlay(
                        &rules.tiberium_types,
                        overlay.cell(ore_cell.0, ore_cell.1).overlay_id.unwrap()
                    )
                    .unwrap()
                    .unwrap(),
                ore_cell.0,
                ore_cell.1,
                41,
                &mut throwaway_rng,
            )
            .unwrap()
            .is_some()
    );
    assert!(
        sim.ore_growth_state
            .add_native_spread_queue_cell(
                registry
                    .tiberium_type_for_overlay(
                        &rules.tiberium_types,
                        overlay.cell(ore_cell.0, ore_cell.1).overlay_id.unwrap()
                    )
                    .unwrap()
                    .unwrap(),
                overlay,
                registry,
                &rules.tiberium_types,
                resolved,
                &std::collections::BTreeSet::new(),
                None,
                ore_cell.0,
                ore_cell.1,
                41,
                true,
                &mut throwaway_rng,
            )
            .unwrap()
            .is_some()
    );
}

fn restore_before_map_authority(sim: &Simulation) -> Simulation {
    let resolved = (*sim.resolved_terrain)
        .clone()
        .expect("fixture resolved terrain cache");
    let bytes = GameSnapshot::save(sim, 0, 0, "gsi_17_04", 0);
    let mut restored = GameSnapshot::load(&bytes).expect("current snapshot").sim;
    restored
        .restore_after_snapshot_load()
        .expect("stable references and substrate re-register");
    restored.rebuild_caches_after_load(
        resolved,
        Default::default(),
        &crate::sim::runtime::SimResources::empty().rules,
    );
    restored
}

#[test]
fn gsi_17_04_postload_rebuild_discards_saved_queues_and_uses_ground_object_list() {
    let (rules, registry, ore_id) = tiberium_fixture();
    let mut sim = base_sim(ore_id, &[(4, 5), (5, 5), (6, 5), (7, 5)]);
    add_marked_unit(&mut sim, (5, 5), false);
    add_marked_unit(&mut sim, (6, 5), true);
    add_live_terrain_object(&mut sim, (7, 5));
    seed_serialized_queue_state(&mut sim, &rules, &registry, (4, 5));

    let mut restored = restore_before_map_authority(&sim);
    let saved_class = &restored.ore_growth_state.native_tiberium_state().classes[0];
    assert_eq!(saved_class.growth_timer.start_frame(), 7);
    assert_ne!(
        saved_class.growth.heap_entry(0).unwrap().priority_bits,
        0.0f32.to_bits()
    );
    assert_eq!(
        restored
            .substrate
            .occupancy
            .count_on_layer(5, 5, MovementLayer::Ground),
        1
    );
    assert_eq!(
        restored
            .substrate
            .occupancy
            .count_on_layer(6, 5, MovementLayer::Bridge),
        1
    );

    let rng_before = restored.rng_state();
    let restore_output = restored
        .restore_map_authority_after_snapshot_load(&rules, &registry, None)
        .expect("all map-authority rebuild dependencies");
    assert_eq!(
        restored.rng_state(),
        rng_before,
        "load rebuild draws no RNG"
    );
    assert_eq!(restore_output.native_tiberium_stats.growth_entries, 4);
    assert_eq!(restore_output.native_tiberium_stats.spread_entries, 2);

    let class = &restored.ore_growth_state.native_tiberium_state().classes[0];
    assert_eq!(
        (
            class.growth_timer.start_frame(),
            class.growth_timer.duration()
        ),
        (91, 0)
    );
    assert_eq!(
        (
            class.spread_timer.start_frame(),
            class.spread_timer.duration()
        ),
        (91, 0)
    );
    assert!(
        class
            .growth
            .iter_heap()
            .all(|entry| entry.priority_bits == 0.0f32.to_bits())
    );
    assert!(
        class
            .spread
            .iter_heap()
            .all(|entry| entry.priority_bits == 0.0f32.to_bits())
    );
    let growth_cells: BTreeSet<_> = class
        .growth
        .iter_heap()
        .map(|entry| (entry.rx, entry.ry))
        .collect();
    let spread_cells: BTreeSet<_> = class
        .spread
        .iter_heap()
        .map(|entry| (entry.rx, entry.ry))
        .collect();
    assert_eq!(
        growth_cells,
        BTreeSet::from([(4, 5), (5, 5), (6, 5), (7, 5)])
    );
    assert_eq!(spread_cells, BTreeSet::from([(4, 5), (6, 5)]));
    assert_eq!(class.growth_bitmap, growth_cells);
    assert_eq!(class.spread_bitmap, spread_cells);
}

/// The post-load rebuild walks the serialized native `MapRect`
/// (`0x0087F8DC` / `0x0087F8E0`), never the cell-array extent in
/// `session.map_*`. With 12x12 storage over an 8x8 `[Map] Size`,
/// (5, 5) lies on anti-diagonal 10: inside the (8, 8) walk (first
/// diagonal 9), outside a (12, 12) walk (first diagonal 13).
#[test]
fn gsi_17_04_postload_rebuild_walks_the_serialized_native_rect() {
    let (rules, registry, ore_id) = tiberium_fixture();
    let mut sim = base_sim_sized(ore_id, 12, &[(5, 5), (7, 7)]);
    sim.install_playfield_from_map_header(&map_header(8, 8));
    {
        let overlay = sim.overlay_grid.as_ref().expect("fixture overlay grid");
        let fresh = sim
            .ore_growth_state
            .rebuild_native_tiberium_queues_from_overlays(
                overlay,
                &registry,
                &rules.tiberium_types,
                sim.resolved_terrain.as_ref(),
                &BTreeSet::new(),
                true,
                true,
                7,
                (8, 8),
            )
            .unwrap();
        assert_eq!(fresh.growth_entries, 2);
    }
    assert_eq!(sim.ore_growth_state.native_rect(), (8, 8));

    let mut restored = restore_before_map_authority(&sim);
    assert_eq!(
        restored.ore_growth_state.native_rect(),
        (8, 8),
        "the rect survives the snapshot"
    );
    assert_eq!(restored.session.map_width(), 12);
    let output = restored
        .restore_map_authority_after_snapshot_load(&rules, &registry, None)
        .expect("all map-authority rebuild dependencies");
    assert_eq!(output.native_tiberium_stats.growth_entries, 2);
    assert_eq!(output.native_tiberium_stats.spread_entries, 2);
    let class = &restored.ore_growth_state.native_tiberium_state().classes[0];
    assert_eq!(class.growth_bitmap, BTreeSet::from([(5, 5), (7, 7)]));
    assert_eq!(
        class.growth.capacity(),
        crate::sim::ore_growth::native_tiberium_queue_capacity((8, 8))
    );
}

/// No load function calls `CellClass::RecalcAttributes`
/// (`0x0047D2B0`), and MapClass::Resize's whole-map pass
/// (`0x00567110`, through `0x00568BB0`) comes before it reconstructs
/// the Dummy (`0x005670E7..0x005670F2`). The rebuild's overlay sweep
/// stands for no Recalc, so it leaves the shared Dummy cell where it
/// was. Ore cell (0, 5) lacks the neighbours (-1, 5) and (-1, 6).
#[test]
fn postload_overlay_sweep_reads_no_cell_through_the_dummy() {
    let (rules, registry, ore_id) = tiberium_fixture_with_general("CliffBackImpassability=2\n");
    let sim = base_sim_sized(ore_id, 12, &[(0, 5)]);
    let mut restored = restore_before_map_authority(&sim);
    restored.effective_shared_cell_dummy().stamp_coord(7, 9);
    restored
        .restore_map_authority_after_snapshot_load(&rules, &registry, None)
        .expect("all map-authority rebuild dependencies");
    assert_eq!(
        restored.effective_shared_cell_dummy().snapshot().coord,
        (7, 9)
    );
}

/// A snapshot written before the rect was serialized deserializes it
/// as `(0, 0)`; the rebuild then falls back to the retained MapClass
/// `Size` (`+0xF4` width plus the separately retained height), never
/// the cell-array extent.
#[test]
fn gsi_17_04_postload_rebuild_zero_rect_falls_back_to_map_size() {
    let (rules, registry, ore_id) = tiberium_fixture();
    let mut sim = base_sim_sized(ore_id, 12, &[(5, 5), (7, 7)]);
    sim.install_playfield_from_map_header(&map_header(8, 8));
    sim.ore_growth_state.set_native_rect_for_tests((0, 0));

    let mut restored = restore_before_map_authority(&sim);
    assert_eq!(restored.ore_growth_state.native_rect(), (0, 0));
    let output = restored
        .restore_map_authority_after_snapshot_load(&rules, &registry, None)
        .expect("all map-authority rebuild dependencies");
    assert_eq!(output.native_tiberium_stats.growth_entries, 2);
    assert_eq!(restored.ore_growth_state.native_rect(), (8, 8));
}

/// Fresh-load `InitGrowthQueues_All @ 0x00722D00` /
/// `InitSpreadQueues_All @ 0x00722240` read `Cell+0xE4 FirstObject`,
/// which `TerrainClass::Mark @ 0x0071BFF8` links for every terrain
/// object, spawning or not. A plain tree over ore keeps the cell out
/// of the spread store while `CanGrowTiberium @ 0x00483620` still
/// admits it to growth.
#[test]
fn oq_38_fresh_load_queue_init_counts_plain_terrain_objects_as_first_object() {
    let (rules, registry, ore_id) = tiberium_fixture();
    let mut sim = base_sim(ore_id, &[(4, 5), (5, 5)]);
    add_live_terrain_object(&mut sim, (5, 5));
    assert!(
        !sim.terrain_objects.spawning_cells().contains(&(5, 5)),
        "the tree is not an ore spawner"
    );

    sim.session.set_tiberium_grows_flag_for_test(true);
    sim.session.set_tiberium_spreads_flag_for_test(true);
    let stats = crate::sim::runtime::initialize_native_tiberium_queues(
        &mut sim,
        &BasicSection::default(),
        &rules,
        &registry,
        (8, 8),
    )
    .unwrap()
    .expect("overlay grid present");

    assert_eq!(stats.growth_entries, 2);
    assert_eq!(stats.spread_entries, 1);
    let class = &sim.ore_growth_state.native_tiberium_state().classes[0];
    assert_eq!(class.growth_bitmap, BTreeSet::from([(4, 5), (5, 5)]));
    assert_eq!(class.spread_bitmap, BTreeSet::from([(4, 5)]));
}

#[test]
fn gsi_17_04_first_resumed_pass_reloads_type_growth_and_spread_intervals() {
    let (rules, registry, ore_id) = tiberium_fixture();
    let mut sim = base_sim(ore_id, &[(4, 5)]);
    seed_serialized_queue_state(&mut sim, &rules, &registry, (4, 5));
    let mut restored = restore_before_map_authority(&sim);
    restored
        .restore_map_authority_after_snapshot_load(&rules, &registry, None)
        .expect("all map-authority rebuild dependencies");
    restored.resolve_type_handles(&rules);

    let timer_before = restored.ore_growth_state.native_tiberium_state().classes[0].growth_timer;
    assert_eq!(
        (timer_before.start_frame(), timer_before.duration()),
        (91, 0)
    );
    restored.advance_tick(&[], Some(&rules), None, Some(&registry), 67);

    let class = &restored.ore_growth_state.native_tiberium_state().classes[0];
    assert_eq!(
        (
            class.growth_timer.start_frame(),
            class.growth_timer.duration()
        ),
        (91, 17)
    );
    assert_eq!(
        (
            class.spread_timer.start_frame(),
            class.spread_timer.duration()
        ),
        (91, 23)
    );
    assert_eq!(restored.session.binary_frame(), 92);
}

#[test]
fn gsi_17_04_missing_dependency_rejects_postload_queue_admission() {
    let (rules, registry, ore_id) = tiberium_fixture();
    let mut sim = base_sim(ore_id, &[(4, 5)]);
    sim.ore_growth_state
        .reset_native_tiberium_classes(rules.tiberium_types.len(), 7);

    let overlays = sim.overlay_grid.take();
    assert!(matches!(
        sim.restore_map_authority_after_snapshot_load(&rules, &registry, None),
        Err(SnapshotRestoreError::MissingMapAuthorityComponent {
            component: "OverlayGrid",
        })
    ));
    *sim.overlay_grid = overlays;
    *sim.resolved_terrain = None;
    assert!(matches!(
        sim.restore_map_authority_after_snapshot_load(&rules, &registry, None),
        Err(SnapshotRestoreError::MissingMapAuthorityComponent {
            component: "ResolvedTerrainGrid",
        })
    ));
    let class = &sim.ore_growth_state.native_tiberium_state().classes[0];
    assert_eq!(
        (
            class.growth_timer.start_frame(),
            class.growth_timer.duration()
        ),
        (7, 0)
    );
}
