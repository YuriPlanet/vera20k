//! Hermetic stock-contract production oracles for miner outbound Drive commands.

use crate::sim::movement::locomotion::LocomotorSlot;
use crate::sim::movement::locomotion::piggyback::StashedLocomotor;

use std::collections::BTreeMap;

use crate::map::bridge_facts::BridgeCellFacts;
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid, zone_class};
use crate::rules::art_data::ArtRegistry;
use crate::rules::ini_parser::IniFile;
use crate::rules::locomotor_type::{LocomotorKind, MovementZone, SpeedType};
use crate::rules::ruleset::RuleSet;
use crate::rules::terrain_rules::{SpeedCostProfile, TerrainClass};
use crate::sim::components::{DriveCoord, NavTargetRef};
use crate::sim::house_state::HouseState;
use crate::sim::miner::{CargoBale, MinerConfig, MinerKind, MinerState, ResourceType};
use crate::sim::movement::locomotor::{GroundMovePhase, MovementLayer};
use crate::sim::overlay_grid::OverlayGrid;
use crate::sim::pathfinding::PathGrid;
use crate::sim::pathfinding::passability::LandType;
use crate::sim::pathfinding::terrain_cost::TerrainCostGrid;
use crate::sim::pathfinding::zone_hierarchy::{
    ZoneEdgeRecord, ZoneHierarchy, ZoneLevelGraph, ZoneRecord,
};
use crate::sim::rng::SimRngLogicalState;
use crate::sim::world::Simulation;
use crate::util::fixed_math::{SIM_ZERO, SimFixed, ra2_speed_to_leptons_per_second};

const GRID_SIZE: u16 = 64;
const START: (u16, u16) = (32, 32);

/// Jitter ceiling of the Harvest dispatch epilogue: `RandomRanged(0, 2)`.
const RATE_EPILOGUE_JITTER_MAX: u32 = 2;

struct OutboundContractOracle {
    rules: RuleSet,
    overlays: OverlayTypeRegistry,
    tib01: u8,
    clear_speed_costs: SpeedCostProfile,
    tiberium_speed_costs: SpeedCostProfile,
}

fn outbound_contract_oracle() -> OutboundContractOracle {
    let rules_ini = IniFile::from_str(include_str!(
        "../../../tests/fixtures/ini/miner_outbound_rules_contract.ini"
    ));
    let mut rules = RuleSet::from_ini(&rules_ini).expect("outbound contract rules");
    let art_ini = IniFile::from_str(include_str!(
        "../../../tests/fixtures/ini/miner_outbound_art_contract.ini"
    ));
    rules.merge_art_data(&ArtRegistry::from_ini(&art_ini));
    let overlays = OverlayTypeRegistry::from_ini(&rules_ini, None);
    let tib01 = overlays.id_for_name("TIB01").expect("retail TIB01");
    let clear_speed_costs = rules
        .terrain_rules
        .semantics_by_name("Clear")
        .expect("merged [Clear]")
        .speed_costs;
    let tiberium_speed_costs = rules
        .terrain_rules
        .semantics_by_name("Tiberium")
        .expect("merged [Tiberium]")
        .speed_costs;

    assert_eq!(tiberium_speed_costs.track, Some(70));
    assert!(overlays.flags(tib01).is_some_and(|flags| flags.tiberium));
    for (type_id, expected_locomotor, teleporter) in [
        ("HARV", LocomotorKind::Drive, false),
        ("CMIN", LocomotorKind::Teleport, true),
    ] {
        let object = rules
            .object(type_id)
            .unwrap_or_else(|| panic!("retail {type_id}"));
        assert!(object.harvester, "{type_id} Harvester=yes");
        assert_eq!(object.speed, 4, "{type_id} Speed=4");
        assert_eq!(object.turret_rot, 5, "{type_id} ROT=5");
        assert!(object.crusher, "{type_id} Crusher=yes");
        assert_eq!(object.movement_zone, MovementZone::Crusher);
        assert_eq!(object.speed_type, SpeedType::Track);
        assert_eq!(object.locomotor, expected_locomotor);
        assert_eq!(object.teleporter, teleporter);
        assert!(object.accelerates);
        assert_eq!(object.accel_factor, SimFixed::lit("0.03"));
        assert_eq!(object.decel_factor, SimFixed::lit("0.002"));
        assert_eq!(object.slowdown_distance, 500);
    }

    OutboundContractOracle {
        rules,
        overlays,
        tib01,
        clear_speed_costs,
        tiberium_speed_costs,
    }
}

fn production_sim(seed: u64, oracle: &OutboundContractOracle) -> Simulation {
    let mut sim = Simulation::with_seed(seed);
    sim.intern_rule_type_ids(&oracle.rules);
    sim.resolve_type_handles(&oracle.rules);
    sim
}

fn seed_human_house(sim: &mut Simulation, owner: &str) {
    let owner_id = sim.interner.intern(owner);
    sim.houses.insert(
        owner_id,
        HouseState::new(
            owner_id,
            0,
            Some(owner_id),
            true,
            crate::sim::production::STARTING_CREDITS,
            10,
        ),
    );
    sim.session.house_order.push(owner_id);
}

fn resolved_cell(
    rx: u16,
    ry: u16,
    terrain_class: TerrainClass,
    land_type: u8,
    speed_costs: SpeedCostProfile,
) -> ResolvedTerrainCell {
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
        land_type,
        yr_cell_land_type: land_type,
        slope_type: 0,
        template_height: 0,
        render_offset_x: 0,
        render_offset_y: 0,
        terrain_class,
        speed_costs,
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
        base_land_type: LandType::Clear.as_index(),
        base_yr_cell_land_type: LandType::Clear.as_index(),
        base_terrain_class: TerrainClass::Clear,
        base_speed_costs: speed_costs,
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

fn staged_terrain(
    oracle: &OutboundContractOracle,
    ore_cells: &[(u16, u16)],
) -> ResolvedTerrainGrid {
    let clear_land_type = LandType::Clear.as_index();
    let mut terrain = ResolvedTerrainGrid::from_cells(
        GRID_SIZE,
        GRID_SIZE,
        (0..GRID_SIZE)
            .flat_map(|ry| {
                (0..GRID_SIZE).map(move |rx| {
                    resolved_cell(
                        rx,
                        ry,
                        TerrainClass::Clear,
                        clear_land_type,
                        oracle.clear_speed_costs,
                    )
                })
            })
            .collect(),
    );
    for &(rx, ry) in ore_cells {
        let cell = terrain.cell_mut(rx, ry).expect("staged ore cell");
        let tiberium_land_type = LandType::Tiberium.as_index();
        cell.land_type = tiberium_land_type;
        cell.yr_cell_land_type = tiberium_land_type;
        cell.terrain_class = TerrainClass::Tiberium;
        cell.speed_costs = oracle.tiberium_speed_costs;
        cell.allows_tiberium = true;
    }
    terrain
}

fn install_world(
    sim: &mut Simulation,
    oracle: &OutboundContractOracle,
    grid: &PathGrid,
    ore_cells: &[(u16, u16)],
    install_zones: bool,
) {
    let terrain = staged_terrain(oracle, ore_cells);
    sim.playfield_bounds = Some(crate::map::playfield::PlayfieldBounds {
        base: 0,
        off_fc: -64,
        off_100: -1,
        off_104: 128,
        off_108: 65,
    });
    // Map-load authority retains Size height beside the bounds (the shared
    // Foot zone precheck reads both).
    sim.playfield_size_height = Some(i32::from(terrain.height()));
    sim.terrain_costs = SpeedType::ALL_WITH_COSTS
        .iter()
        .copied()
        .map(|speed_type| {
            (
                speed_type,
                TerrainCostGrid::from_resolved_terrain(&terrain, speed_type),
            )
        })
        .collect();
    sim.resolved_terrain = Some(terrain);
    sim.overlay_grid = Some(OverlayGrid::new(GRID_SIZE, GRID_SIZE));
    for &(rx, ry) in ore_cells {
        sim.overlay_grid
            .as_mut()
            .expect("overlay grid")
            .place_overlay(rx, ry, oracle.tib01, 0);
    }
    // Map load publishes the canonical PathGrid the frame and the Drive
    // destination search read.
    sim.path_grid = Some(std::sync::Arc::new(grid.clone()));
    if install_zones {
        sim.rebuild_zone_grid(grid);
        assert!(sim.zone_grid.is_some());
    } else {
        sim.zone_grid = None;
    }
    for &(rx, ry) in ore_cells {
        assert_eq!(
            sim.terrain_costs
                .get(&SpeedType::Track)
                .expect("Track terrain costs")
                .cost_at(rx, ry),
            70,
        );
    }
}

fn spawn_stock_miner(
    sim: &mut Simulation,
    oracle: &OutboundContractOracle,
    type_id: &str,
    expected_kind: MinerKind,
) -> u64 {
    let id = sim
        .spawn_object(
            type_id,
            "Americans",
            START.0,
            START.1,
            0,
            &oracle.rules,
            &BTreeMap::new(),
        )
        .unwrap_or_else(|| panic!("spawn {type_id}"));
    let entity = sim.substrate.entities.get(id).expect("spawned miner");
    assert!(entity.lifecycle.object_alive);
    assert!(!entity.lifecycle.in_limbo);
    assert!(entity.lifecycle.cell_marked);
    assert!(entity.in_logic_vector);
    assert!(sim.live_object_order_snapshot().contains(&id));
    assert_eq!(
        entity.miner.as_ref().expect("miner component").kind,
        expected_kind,
    );
    assert_eq!(
        entity
            .locomotor
            .as_ref()
            .expect("stock locomotor")
            .movement_zone,
        MovementZone::Crusher,
    );
    id
}

/// An owned `Dock=` instance with no cell mark and no LogicVector slot: the
/// Harvest preamble (`0x0073E5E0`) queues Guard for a house that owns no
/// instance of any `Dock=` type, and these outbound fixtures never return.
fn spawn_inert_dock_instance(sim: &mut Simulation) {
    const INERT_DOCK_ID: u64 = 900;
    let owner_id = sim.interner.intern("Americans");
    let type_id = sim.interner.intern("GAREFN");
    let mut ge = crate::sim::game_entity::GameEntity::new_at_frame_zero_for_test(
        INERT_DOCK_ID,
        1,
        1,
        0,
        0,
        owner_id,
        crate::sim::components::Health { current: 900 },
        type_id,
        crate::map::entities::EntityCategory::Structure,
        0,
        5,
        false,
    );
    ge.lifecycle.in_limbo = false;
    sim.substrate.entities.insert(ge);
    if sim.substrate.next_stable_object_id <= INERT_DOCK_ID {
        sim.substrate.next_stable_object_id = INERT_DOCK_ID + 1;
    }
}

fn spawn_stock_refinery(
    sim: &mut Simulation,
    oracle: &OutboundContractOracle,
    anchor: (u16, u16),
) -> u64 {
    let id = sim
        .spawn_object(
            "GAREFN",
            "Americans",
            anchor.0,
            anchor.1,
            0,
            &oracle.rules,
            &BTreeMap::new(),
        )
        .expect("spawn GAREFN");
    let entity = sim.substrate.entities.get(id).expect("spawned refinery");
    assert!(entity.lifecycle.object_alive);
    assert!(!entity.lifecycle.in_limbo);
    assert!(entity.lifecycle.cell_marked);
    assert!(entity.in_logic_vector);
    assert!(sim.live_object_order_snapshot().contains(&id));
    id
}

fn arm_full_ore_return(sim: &mut Simulation, entity_id: u64, config: &MinerConfig) {
    let entity = sim
        .substrate
        .entities
        .get_mut(entity_id)
        .expect("miner entity");
    let miner = entity.miner.as_mut().expect("miner component");
    miner.cargo = (0..miner.capacity_bales)
        .map(|_| CargoBale {
            resource_type: ResourceType::Ore,
            value: config.ore_bale_value,
        })
        .collect();
    miner.reserved_refinery = None;
    entity
        .mission
        .set_handler_state(MinerState::ReturnToRefinery.cursor());
}

fn arm_search(sim: &mut Simulation, entity_id: u64) {
    let entity = sim
        .substrate
        .entities
        .get_mut(entity_id)
        .expect("miner entity");
    let miner = entity.miner.as_mut().expect("miner component");
    entity
        .mission
        .set_handler_state(MinerState::SearchOre.cursor());
    miner.target_ore_cell = None;
    miner.harvest_timer.clear();
}

fn advance(sim: &mut Simulation, oracle: &OutboundContractOracle, grid: &PathGrid) {
    let _ = sim.advance_tick(
        &[],
        Some(&oracle.rules),
        &BTreeMap::new(),
        Some(grid),
        Some(&oracle.overlays),
        67,
    );
}

fn position_tuple(sim: &Simulation, entity_id: u64) -> (u16, u16, SimFixed, SimFixed) {
    let position = &sim
        .substrate
        .entities
        .get(entity_id)
        .expect("entity")
        .position;
    (position.rx, position.ry, position.sub_x, position.sub_y)
}

fn assert_ore_intact(sim: &Simulation, oracle: &OutboundContractOracle, target: (u16, u16)) {
    let overlay = sim
        .overlay_grid
        .as_ref()
        .expect("overlay grid")
        .cell(target.0, target.1);
    assert_eq!(overlay.overlay_id, Some(oracle.tib01));
    assert_eq!(overlay.overlay_data, 0);
}

fn assert_command_state(
    sim: &Simulation,
    oracle: &OutboundContractOracle,
    entity_id: u64,
    type_id: &str,
    target: (u16, u16),
) {
    let object = oracle.rules.object(type_id).expect("retail miner type");
    let entity = sim.substrate.entities.get(entity_id).expect("miner entity");
    let movement = entity.movement_target.as_ref().expect("movement target");
    assert_eq!(movement.path.first().copied(), Some(START));
    assert_eq!(movement.path.last().copied(), Some(target));
    assert_eq!(movement.final_goal, Some(target));
    assert_eq!(
        movement.speed,
        ra2_speed_to_leptons_per_second(object.speed),
    );
    assert_eq!(movement.accel_factor, object.accel_factor);
    assert_eq!(movement.decel_factor, object.decel_factor);
    assert_eq!(
        movement.slowdown_distance,
        SimFixed::from_num(object.slowdown_distance),
    );
    assert!(!movement.ignore_terrain_cost);
    assert!(!movement.bypass_grid);
    assert_eq!(
        entity.navigation.nav_com,
        Some(NavTargetRef::cell(target.0, target.1)),
    );
    let expected_coord = DriveCoord::cell(target.0, target.1, 0);
    let drive = entity.drive_locomotion.as_ref().expect("Drive runtime");
    assert_eq!(drive.destination, Some(expected_coord));
    // These fixtures depart north from START. Native Drive4B32AF commits
    // current XYZ + one direction offset, independently of the ore destination
    // (saved locomotor_head_coordinates corpus; production coverage in track_head_tests).
    let expected_head = DriveCoord::cell(START.0, START.1 - 1, 0);
    assert_eq!(drive.head_to, Some(expected_head));
    assert_eq!(
        drive.track.turn_index, 0,
        "accepted northbound straight track"
    );
    assert!(drive.track.cursor >= 0);
    assert!(!drive.track.reversed);
    assert_eq!(
        entity.navigation.path_replay.directions.len(),
        movement.path.len().saturating_sub(1),
    );
    assert!(!entity.navigation.path_replay.directions.is_empty());
    // The Harvest handler dispatches BEFORE Phase-1 ground movement (the
    // native handler→locomotion order), so by observation time the drive has
    // already begun accelerating in the same tick the command was issued.
    assert!(entity.foot_speed.applied_fraction > SIM_ZERO);
    assert_eq!(
        entity.locomotor.as_ref().expect("active locomotor").kind,
        LocomotorKind::Drive,
    );
}

fn locomotor_tuple(
    sim: &Simulation,
    entity_id: u64,
) -> (
    LocomotorKind,
    LocomotorSlot,
    Option<StashedLocomotor>,
    MovementLayer,
    GroundMovePhase,
) {
    let locomotor = sim
        .substrate
        .entities
        .get(entity_id)
        .and_then(|entity| entity.locomotor.as_ref())
        .expect("locomotor");
    (
        locomotor.kind,
        locomotor.slot,
        locomotor.piggyback.clone(),
        locomotor.layer,
        locomotor.phase,
    )
}

/// Mirror the single `RandomRanged(0, 2)` a Harvest dispatch draws when it exits
/// through the default `[Harvest] Rate` epilogue, and return the scenario-stream
/// state that draw must leave behind. The base lookup consumes no RNG, so one
/// epilogue exit is exactly one draw.
fn scenario_after_one_epilogue_draw(sim: &mut Simulation) -> SimRngLogicalState {
    let mut probe = sim.scenario_rng.clone();
    let _ = probe.next_range_u32_inclusive(0, RATE_EPILOGUE_JITTER_MAX);
    probe.logical_state()
}

/// Re-anchor the miner's Harvest dispatch timer so the next tick dispatches it.
///
/// The scan dispatch installs the Rate epilogue (~14-16 frames), so the ticks
/// immediately behind it carry no Harvest dispatch at all. A fixture that means
/// to observe the *next* dispatch has to ask for it rather than assume the
/// following tick brings one; it is the dispatch's behaviour under test here,
/// not its frame number, which `state_four_exit_draws_and_applies_resume_jitter`
/// already pins.
fn arm_dispatch_now(sim: &mut Simulation, entity_id: u64) {
    let now = sim.session.binary_frame as i32;
    sim.substrate
        .entities
        .get_mut(entity_id)
        .expect("miner entity")
        .mission
        .write_dispatch_epilogue(now, 0);
}

#[test]
fn production_stock_miners_use_drive_command_for_adjacent_ore() {
    let oracle = outbound_contract_oracle();
    let target = (32, 31);
    for (type_id, kind) in [("HARV", MinerKind::War), ("CMIN", MinerKind::Chrono)] {
        let mut sim = production_sim(0x0715_D001, &oracle);
        let grid = PathGrid::new(GRID_SIZE, GRID_SIZE);
        install_world(&mut sim, &oracle, &grid, &[target], true);
        let entity_id = spawn_stock_miner(&mut sim, &oracle, type_id, kind);
        spawn_inert_dock_instance(&mut sim);
        let start_position = position_tuple(&sim, entity_id);
        arm_search(&mut sim, entity_id);
        let rng_before_search = sim.rng_state();
        // The scan sets the destination inside its OWN dispatch and then falls
        // into the default Rate epilogue, so the drive command and exactly one
        // RandomRanged(0,2) both belong to the scan dispatch — not to a later one.
        let scenario_after_scan = scenario_after_one_epilogue_draw(&mut sim);

        advance(&mut sim, &oracle, &grid);

        let rng_after_search = sim.rng_state();
        assert_eq!(
            rng_after_search.scenario, scenario_after_scan,
            "{type_id} scan dispatch draws the Rate epilogue jitter exactly once"
        );
        assert_eq!(
            rng_after_search.main, rng_before_search.main,
            "{type_id} search main RNG"
        );
        assert_eq!(
            rng_after_search.mapgen, rng_before_search.mapgen,
            "{type_id} search mapgen RNG"
        );
        let entity = sim.substrate.entities.get(entity_id).expect("miner");
        let miner = entity.miner.as_ref().expect("miner");
        assert_eq!(entity.miner_state().unwrap(), MinerState::MoveToOre);
        assert_eq!(miner.target_ore_cell, Some(target));
        // The full outbound command is installed by the scan dispatch itself.
        assert_command_state(&sim, &oracle, entity_id, type_id, target);

        {
            let entity = sim.substrate.entities.get(entity_id).expect("miner");
            let locomotor = entity.locomotor.as_ref().expect("locomotor");
            if type_id == "CMIN" {
                // Teleport stays the PRIMARY locomotor and the outbound leg rides
                // a Drive piggyback on top of it — that is what makes a stock
                // chrono miner DRIVE to its first ore field instead of warping.
                assert_eq!(locomotor.kind, LocomotorKind::Drive);
                assert_eq!(
                    locomotor.slot,
                    LocomotorSlot::from_kind(LocomotorKind::Teleport)
                );
                assert_eq!(
                    locomotor
                        .piggyback
                        .as_ref()
                        .expect("CMIN Drive piggyback")
                        .kind,
                    LocomotorKind::Teleport,
                );
            } else {
                assert_eq!(
                    locomotor.slot,
                    LocomotorSlot::from_kind(LocomotorKind::Drive)
                );
                assert_eq!(locomotor.piggyback, None);
            }
            assert!(entity.teleport_state.is_none());
        }

        // The epilogue paces the next dispatch 14-16 frames out, so the ticks
        // right behind the scan run no Harvest dispatch at all: they move the
        // hull only. Every Harvest dispatch except the no-ore miss leaves through
        // an epilogue that draws, so a still scenario stream is the evidence that
        // no dispatch ran.
        advance(&mut sim, &oracle, &grid);
        advance(&mut sim, &oracle, &grid);
        assert_eq!(
            sim.rng_state().scenario,
            rng_after_search.scenario,
            "{type_id} undispatched ticks must not draw"
        );
        {
            let entity = sim.substrate.entities.get(entity_id).expect("miner");
            assert!(entity.drive_locomotion.is_some());
            let movement = entity.movement_target.as_ref().expect("movement");
            // One cell out is inside `SlowdownDistance=500`, so the ramp opens on
            // the destination brake floor and holds there for the whole hop.
            assert_eq!(entity.foot_speed.applied_fraction, SimFixed::lit("0.3"));
            assert_eq!(
                movement.current_speed,
                movement.speed * SimFixed::lit("0.3"),
            );
        }

        let mut physically_departed = position_tuple(&sim, entity_id) != start_position;
        let mut reached_harvest = false;
        for _ in 0..128 {
            advance(&mut sim, &oracle, &grid);
            let entity = sim.substrate.entities.get(entity_id).expect("miner");
            physically_departed |= position_tuple(&sim, entity_id) != start_position;
            reached_harvest |= entity.miner_state().expect("miner") == MinerState::Harvest;
            assert!(entity.teleport_state.is_none());
            if type_id == "CMIN" && entity.movement_target.is_some() {
                let locomotor = entity.locomotor.as_ref().expect("CMIN locomotor");
                assert_eq!(locomotor.kind, LocomotorKind::Drive);
                assert_eq!(
                    locomotor.slot,
                    LocomotorSlot::from_kind(LocomotorKind::Teleport)
                );
                assert!(locomotor.piggyback.is_some());
            }
            if reached_harvest {
                break;
            }
        }
        assert!(physically_departed, "{type_id} must leave {START:?}");
        assert!(reached_harvest, "{type_id} must reach Harvest");
        let entity = sim.substrate.entities.get(entity_id).expect("miner");
        assert_eq!(entity.navigation.nav_com, None);
        assert!(!entity.navigation.pending_arrival_clear);
        if type_id == "CMIN" {
            let locomotor = entity.locomotor.as_ref().expect("CMIN locomotor");
            assert_eq!(
                locomotor.kind,
                LocomotorKind::Teleport,
                "Drive={:?}; Foot={:?}; position={:?}; Nav={:?}; mission={:?}",
                entity.drive_locomotion,
                entity.foot_speed,
                entity.position,
                entity.navigation,
                entity.mission
            );
            assert_eq!(
                locomotor.slot,
                LocomotorSlot::from_kind(LocomotorKind::Teleport)
            );
            assert_eq!(locomotor.piggyback, None);
            assert!(
                entity.drive_locomotion.is_none(),
                "native FootClass::AI releases retired Drive"
            );
        }
        // The outbound leg legitimately consumes scenario RNG now: each
        // still-driving dispatch exits through the default Rate epilogue,
        // drawing one RandomRanged(0,2). Only the non-scenario streams stay
        // untouched across the leg.
        let rng_after = sim.rng_state();
        assert_eq!(rng_after.main, rng_before_search.main, "{type_id} main RNG");
        assert_eq!(
            rng_after.mapgen, rng_before_search.mapgen,
            "{type_id} mapgen RNG"
        );
        assert_ore_intact(&sim, &oracle, target);
    }
}

#[test]
fn production_harv_outbound_drive_uses_rule_profile() {
    let oracle = outbound_contract_oracle();
    let target = (32, 29);
    let mut sim = production_sim(0x0715_D002, &oracle);
    let grid = PathGrid::new(GRID_SIZE, GRID_SIZE);
    install_world(&mut sim, &oracle, &grid, &[target], true);
    let entity_id = spawn_stock_miner(&mut sim, &oracle, "HARV", MinerKind::War);
    spawn_inert_dock_instance(&mut sim);
    arm_search(&mut sim, entity_id);
    let rng_before = sim.rng_state();
    // Scan, Set_Destination and the Rate epilogue are one dispatch: one draw.
    let scenario_after_scan = scenario_after_one_epilogue_draw(&mut sim);

    advance(&mut sim, &oracle, &grid);
    let rng_after_scan = sim.rng_state();
    assert_eq!(rng_after_scan.scenario, scenario_after_scan);
    assert_eq!(rng_after_scan.main, rng_before.main);
    assert_eq!(rng_after_scan.mapgen, rng_before.mapgen);
    assert_command_state(&sim, &oracle, entity_id, "HARV", target);
    let harv = oracle.rules.object("HARV").expect("HARV");
    // Three cells out is farther than `SlowdownDistance=500`, so the rules accel
    // profile — not the destination brake floor — owns the ramp.
    assert!(3 * 256 > harv.slowdown_distance);
    let acceleration = harv.accel_factor;

    // Dispatch precedes movement, so the issuing tick already took one accel step.
    assert_eq!(
        sim.substrate
            .entities
            .get(entity_id)
            .expect("HARV")
            .foot_speed
            .applied_fraction,
        acceleration,
    );

    // The epilogue paces the next dispatch 14-16 frames out, so this tick moves
    // the hull one more accel step without running a Harvest dispatch at all.
    advance(&mut sim, &oracle, &grid);
    let entity = sim.substrate.entities.get(entity_id).expect("HARV");
    assert!(entity.drive_locomotion.is_some());
    let movement = entity.movement_target.as_ref().expect("movement");
    assert_eq!(
        entity.foot_speed.applied_fraction,
        acceleration + acceleration
    );
    assert_eq!(
        movement.current_speed,
        movement.speed * (acceleration + acceleration)
    );
    assert!(movement.current_speed > SIM_ZERO);
    assert_eq!(
        sim.rng_state().scenario,
        rng_after_scan.scenario,
        "a tick with no Harvest dispatch draws no epilogue jitter"
    );
}

#[test]
fn production_stock_harv_far_return_drive_uses_rule_profile() {
    let oracle = outbound_contract_oracle();
    let config = MinerConfig::from_rules(&oracle.rules);
    let refinery_anchor = (10, 10);
    let refinery_type = oracle.rules.object("GAREFN").expect("GAREFN");
    let queueing = refinery_type.queueing_cell;
    let staging = (
        refinery_anchor.0 + queueing[0] as u16,
        refinery_anchor.1 + queueing[1] as u16,
    );
    let accepted_dock = (refinery_anchor.0 + 3, refinery_anchor.1 + 1);
    assert_eq!(queueing, [4, 1]);
    assert_ne!(staging, accepted_dock);

    let dx = u32::from(START.0.abs_diff(refinery_anchor.0));
    let dy = u32::from(START.1.abs_diff(refinery_anchor.1));
    let threshold = oracle.rules.general.harvester_too_far_distance as u32;
    assert!(dx * dx + dy * dy > threshold * threshold);

    let mut sim = production_sim(0x0715_D008, &oracle);
    let mut grid = PathGrid::new(GRID_SIZE, GRID_SIZE);
    grid.block_building_movement_cells(
        refinery_anchor.0,
        refinery_anchor.1,
        &refinery_type.foundation,
        refinery_type.bib,
    );
    assert!(!grid.is_walkable(refinery_anchor.0, refinery_anchor.1));
    assert!(grid.is_walkable(staging.0, staging.1));
    install_world(&mut sim, &oracle, &grid, &[], true);
    let refinery_id = spawn_stock_refinery(&mut sim, &oracle, refinery_anchor);
    let entity_id = spawn_stock_miner(&mut sim, &oracle, "HARV", MinerKind::War);
    arm_full_ore_return(&mut sim, entity_id, &config);

    let start = position_tuple(&sim, entity_id);
    // Mission_Harvest's shared delay-jitter RNG tail remains a separate scheduler residual.
    advance(&mut sim, &oracle, &grid);

    let harv = oracle.rules.object("HARV").expect("HARV");
    let entity = sim.substrate.entities.get(entity_id).expect("HARV");
    let movement = entity.movement_target.as_ref().expect("movement target");
    let drive = entity.drive_locomotion.as_ref().expect("Drive runtime");
    assert_eq!(entity.miner_state().unwrap(), MinerState::ReturnToRefinery);
    assert!(
        !entity.radio_contacts.contains(refinery_id),
        "beyond HarvesterTooFarDistance the return sends no HELLO"
    );
    assert_eq!(movement.final_goal, Some(staging));
    assert_eq!(
        entity.navigation.nav_com,
        Some(NavTargetRef::cell(staging.0, staging.1)),
    );
    assert_eq!(
        drive.destination,
        Some(DriveCoord::cell(staging.0, staging.1, 0)),
    );
    assert_eq!(movement.speed, ra2_speed_to_leptons_per_second(harv.speed),);
    assert_eq!(movement.accel_factor, harv.accel_factor);
    assert_eq!(movement.decel_factor, harv.decel_factor);
    assert_eq!(
        movement.slowdown_distance,
        SimFixed::from_num(harv.slowdown_distance),
    );
    // The exact-facing precondition holds the hull on the spot until it reaches
    // the first path node's octant, and a frame spent rotating carries no speed
    // ramp — so the issuing tick leaves the drive fraction at zero and the ramp
    // only starts once the turn has finished.
    assert_eq!(entity.foot_speed.applied_fraction, SIM_ZERO);
    // Do_Turn 0x4B0EF0 sets the body FacingClass (0x4C9220).
    assert!(
        entity
            .body_facing
            .as_ref()
            .is_some_and(|body| body.is_rotating(sim.session.binary_frame)),
        "the hull is commanded onto the head path node's octant first"
    );

    let mut departed = position_tuple(&sim, entity_id) != start;
    for _ in 0..96 {
        if departed {
            break;
        }
        advance(&mut sim, &oracle, &grid);
        departed = position_tuple(&sim, entity_id) != start;
    }
    assert!(departed, "stock HARV must physically leave {start:?}");

    let entity = sim.substrate.entities.get(entity_id).expect("HARV");
    assert!(entity.drive_locomotion.is_some());
    let movement = entity.movement_target.as_ref().expect("movement target");
    assert!(
        entity.foot_speed.applied_fraction >= harv.accel_factor,
        "the rules accel profile ramps once the hull is under way"
    );
    assert_eq!(
        movement.current_speed,
        movement.speed * entity.foot_speed.applied_fraction,
    );
    assert!(movement.current_speed > SIM_ZERO);
}

#[test]
fn gsi_04_07_placement_miner_return_threads_live_wall_neighbor_authority() {
    let oracle = outbound_contract_oracle();
    let config = MinerConfig::from_rules(&oracle.rules);
    let refinery_anchor = (24, 31);
    let refinery_type = oracle.rules.object("GAREFN").expect("GAREFN");
    let queueing = refinery_type.queueing_cell;
    let staging = (
        refinery_anchor.0 + queueing[0] as u16,
        refinery_anchor.1 + queueing[1] as u16,
    );
    assert_eq!(staging, (28, 32));
    assert!(
        i32::from(START.0.abs_diff(refinery_anchor.0))
            > oracle.rules.general.harvester_too_far_distance
    );

    let overlay_ini = IniFile::from_str(
        "[OverlayTypes]\n0=WALL\n1=ROCK\n\
         [WALL]\nWall=yes\n\
         [ROCK]\nIsARock=yes\n",
    );
    let overlay_registry = OverlayTypeRegistry::from_ini(&overlay_ini, None);
    let wall_id = overlay_registry.id_for_name("WALL").expect("wall id");
    let rock_id = overlay_registry.id_for_name("ROCK").expect("rock id");

    let make_case = |overlay_id: u8, seed: u64| {
        let mut sim = production_sim(seed, &oracle);
        let mut grid = PathGrid::new(GRID_SIZE, GRID_SIZE);
        for ry in 0..GRID_SIZE {
            for rx in 0..GRID_SIZE {
                grid.set_blocked(rx, ry, true);
            }
        }
        for rx in staging.0..=START.0 {
            grid.set_blocked(rx, START.1, false);
        }
        grid.block_building_movement_cells(
            refinery_anchor.0,
            refinery_anchor.1,
            &refinery_type.foundation,
            refinery_type.bib,
        );
        assert!(grid.is_walkable(staging.0, staging.1));
        install_world(&mut sim, &oracle, &grid, &[], true);
        let refinery_id = spawn_stock_refinery(&mut sim, &oracle, refinery_anchor);
        let miner_id = spawn_stock_miner(&mut sim, &oracle, "HARV", MinerKind::War);
        arm_full_ore_return(&mut sim, miner_id, &config);

        sim.overlay_grid
            .as_mut()
            .expect("live overlay grid")
            .place_overlay(30, 33, overlay_id, 0);

        // Zone_precheck marks only the start/goal zones (1 and 2). The route
        // must cross off-marker zones 3 and 4. The miner itself supplies the
        // count beside zone 3; only the off-route wall at (30,33) may supply
        // zone 4's neighbor exception.
        let mut cell_zones = vec![1; usize::from(GRID_SIZE) * usize::from(GRID_SIZE)];
        for (rx, zone) in [(28, 2), (29, 4), (30, 1), (31, 3), (32, 1)] {
            cell_zones[usize::from(START.1) * usize::from(GRID_SIZE) + rx] = zone;
        }
        let mut level2 = ZoneLevelGraph::new(1);
        level2.set_record(ZoneRecord::new(1, 0, 0));
        let mut level1 = ZoneLevelGraph::new(1);
        level1.set_record(ZoneRecord::new(1, 1, 0));
        let mut level0 =
            ZoneLevelGraph::new(4).with_cell_zone_ids(cell_zones, GRID_SIZE, GRID_SIZE);
        for zone in 1..=4 {
            level0.set_record(ZoneRecord::new(zone, 1, 0));
        }
        level0.push_edge(1, ZoneEdgeRecord::new(2, 0));
        level0.push_edge(2, ZoneEdgeRecord::new(1, 0));
        sim.zone_grid
            .as_mut()
            .expect("zone grid")
            .set_hierarchy(ZoneHierarchy::new(level0, level1, level2));

        let _ = sim.advance_tick(
            &[],
            Some(&oracle.rules),
            &BTreeMap::new(),
            Some(&grid),
            Some(&overlay_registry),
            67,
        );
        // The far return sends no HELLO; it stages at the QueueingCell.
        let entity = sim.substrate.entities.get(miner_id).expect("miner");
        assert!(!entity.radio_contacts.contains(refinery_id));
        (sim, miner_id)
    };

    let (wall_case, wall_miner) = make_case(wall_id, 0x0407_0001);
    assert_eq!(
        wall_case
            .substrate
            .entities
            .get(wall_miner)
            .and_then(|entity| entity.movement_target.as_ref())
            .and_then(|movement| movement.final_goal),
        Some(staging),
        "Wall=yes must supply the off-marker neighbor exception to the live return route",
    );
    assert_eq!(
        wall_case
            .substrate
            .entities
            .get(wall_miner)
            .and_then(|entity| entity.navigation.nav_com),
        Some(NavTargetRef::cell(staging.0, staging.1)),
    );

    let (rock_case, rock_miner) = make_case(rock_id, 0x0407_0002);
    assert!(
        rock_case
            .substrate
            .entities
            .get(rock_miner)
            .and_then(|entity| entity.movement_target.as_ref())
            .is_none(),
        "a non-wall blocking overlay must not become a neighbor-plane source",
    );
}

#[test]
fn production_stock_harv_far_return_preserves_existing_navcom_owner() {
    let oracle = outbound_contract_oracle();
    let config = MinerConfig::from_rules(&oracle.rules);
    let refinery_anchor = (10, 10);
    let original = (32, 29);
    let refinery_type = oracle.rules.object("GAREFN").expect("GAREFN");
    let queueing = refinery_type.queueing_cell;
    let staging = (
        refinery_anchor.0 + queueing[0] as u16,
        refinery_anchor.1 + queueing[1] as u16,
    );
    let accepted_dock = (refinery_anchor.0 + 3, refinery_anchor.1 + 1);
    assert_eq!(queueing, [4, 1]);
    assert_ne!(staging, accepted_dock);
    let mut sim = production_sim(0x0715_D009, &oracle);
    let mut grid = PathGrid::new(GRID_SIZE, GRID_SIZE);
    grid.block_building_movement_cells(
        refinery_anchor.0,
        refinery_anchor.1,
        &refinery_type.foundation,
        refinery_type.bib,
    );
    assert!(!grid.is_walkable(refinery_anchor.0, refinery_anchor.1));
    assert!(grid.is_walkable(staging.0, staging.1));
    install_world(&mut sim, &oracle, &grid, &[original], true);
    let refinery_id = spawn_stock_refinery(&mut sim, &oracle, refinery_anchor);
    let entity_id = spawn_stock_miner(&mut sim, &oracle, "HARV", MinerKind::War);
    arm_search(&mut sim, entity_id);

    // One dispatch: the scan acquires `original` and installs the command.
    advance(&mut sim, &oracle, &grid);
    {
        let entity = sim.substrate.entities.get(entity_id).expect("HARV");
        assert_eq!(
            entity.navigation.nav_com,
            Some(NavTargetRef::cell(original.0, original.1)),
        );
        assert_eq!(
            entity
                .drive_locomotion
                .as_ref()
                .expect("Drive runtime")
                .destination,
            Some(DriveCoord::cell(original.0, original.1, 0)),
        );
    }

    {
        let entity = sim.substrate.entities.get_mut(entity_id).expect("HARV");
        entity.movement_target = None;
    }
    arm_full_ore_return(&mut sim, entity_id, &config);

    let (
        nav_before,
        drive_before,
        target_before,
        cargo_before,
        timers_before,
        miner_contacts_before,
        dock_entered_before,
    ) = {
        let entity = sim.substrate.entities.get(entity_id).expect("HARV");
        let miner = entity.miner.as_ref().expect("miner");
        assert_eq!(miner.reserved_refinery, None);
        (
            entity.navigation.nav_com,
            entity
                .drive_locomotion
                .as_ref()
                .expect("Drive runtime")
                .clone(),
            miner.target_ore_cell,
            miner.cargo.clone(),
            (
                miner.harvest_timer,
                miner.rescan_cooldown,
                miner.unload_cluster_timer,
            ),
            entity.radio_contacts.clone(),
            entity.dock_entered_with,
        )
    };
    let refinery_contacts_before = sim
        .substrate
        .entities
        .get(refinery_id)
        .expect("GAREFN")
        .radio_contacts
        .clone();
    let sound_count_before = sim.sound_events.len();
    assert!(!crate::sim::miner::miner_dock::test_support::dock_test_is_occupied(&sim, refinery_id));

    // The scan dispatch left the Rate epilogue on the dispatch timer, so ask for
    // the next dispatch explicitly — the gate under test is what that dispatch
    // does, not when it lands.
    arm_dispatch_now(&mut sim, entity_id);
    // This oracle covers the state-2 owner gate, not the shared mission-delay RNG tail.
    advance(&mut sim, &oracle, &grid);

    let entity = sim.substrate.entities.get(entity_id).expect("HARV");
    let miner = entity.miner.as_ref().expect("miner");
    let timers_after = (
        miner.harvest_timer,
        miner.rescan_cooldown,
        miner.unload_cluster_timer,
    );
    assert_eq!(entity.miner_state().unwrap(), MinerState::ReturnToRefinery);
    assert_eq!(miner.reserved_refinery, None);
    assert_eq!(entity.navigation.nav_com, nav_before);
    assert_eq!(entity.drive_locomotion.as_ref(), Some(&drive_before));
    assert!(entity.movement_target.is_none());
    assert_eq!(miner.target_ore_cell, target_before);
    assert_eq!(miner.cargo, cargo_before);
    assert_eq!(timers_after, timers_before);
    assert_eq!(entity.radio_contacts, miner_contacts_before);
    assert_eq!(entity.dock_entered_with, dock_entered_before);
    assert_eq!(
        sim.substrate
            .entities
            .get(refinery_id)
            .expect("GAREFN")
            .radio_contacts,
        refinery_contacts_before,
    );
    assert!(!crate::sim::miner::miner_dock::test_support::dock_test_is_occupied(&sim, refinery_id));
    assert!(!crate::sim::miner::miner_dock::has_contact(
        &sim,
        refinery_id,
        entity_id
    ),);
    assert!(!crate::sim::miner::miner_dock::test_support::has_entered(
        &sim,
        refinery_id,
        entity_id
    ),);
    assert_eq!(sim.sound_events.len(), sound_count_before);
}

#[test]
fn production_cmin_outbound_drive_keeps_teleport_primary() {
    let oracle = outbound_contract_oracle();
    let target = (32, 29);
    let mut sim = production_sim(0x0715_D003, &oracle);
    let grid = PathGrid::new(GRID_SIZE, GRID_SIZE);
    install_world(&mut sim, &oracle, &grid, &[target], true);
    let entity_id = spawn_stock_miner(&mut sim, &oracle, "CMIN", MinerKind::Chrono);
    spawn_inert_dock_instance(&mut sim);
    arm_search(&mut sim, entity_id);
    let rng_before = sim.rng_state();

    // The scan dispatch installs the outbound command itself. What must survive
    // that is the locomotor arrangement: Teleport stays the PRIMARY in its own
    // slot and Drive is only piggybacked on top, so the first outbound trip is a
    // drive and never a warp.
    advance(&mut sim, &oracle, &grid);
    assert_command_state(&sim, &oracle, entity_id, "CMIN", target);
    {
        let entity = sim.substrate.entities.get(entity_id).expect("CMIN");
        let locomotor = entity.locomotor.as_ref().expect("CMIN locomotor");
        assert_eq!(
            entity.navigation.nav_com,
            Some(NavTargetRef::cell(target.0, target.1)),
        );
        assert_eq!(locomotor.kind, LocomotorKind::Drive);
        assert_eq!(
            locomotor.slot,
            LocomotorSlot::from_kind(LocomotorKind::Teleport)
        );
        assert_eq!(
            locomotor
                .piggyback
                .as_ref()
                .expect("CMIN Drive piggyback")
                .kind,
            LocomotorKind::Teleport,
        );
        assert!(
            entity.teleport_state.is_none(),
            "the outbound leg must never start a warp"
        );
    }

    let mut reached_harvest = false;
    for _ in 0..240 {
        advance(&mut sim, &oracle, &grid);
        let entity = sim.substrate.entities.get(entity_id).expect("CMIN");
        assert!(entity.teleport_state.is_none());
        if entity.movement_target.is_some() {
            let locomotor = entity.locomotor.as_ref().expect("CMIN locomotor");
            assert_eq!(locomotor.kind, LocomotorKind::Drive);
            assert_eq!(
                locomotor.slot,
                LocomotorSlot::from_kind(LocomotorKind::Teleport)
            );
            assert!(locomotor.piggyback.is_some());
        }
        if entity.miner_state().expect("miner") == MinerState::Harvest {
            assert!(entity.movement_target.is_none());
            let locomotor = entity.locomotor.as_ref().expect("CMIN locomotor");
            assert_eq!(
                locomotor.kind,
                LocomotorKind::Teleport,
                "Drive={:?}; Foot={:?}; position={:?}; Nav={:?}; mission={:?}",
                entity.drive_locomotion,
                entity.foot_speed,
                entity.position,
                entity.navigation,
                entity.mission
            );
            assert_eq!(
                locomotor.slot,
                LocomotorSlot::from_kind(LocomotorKind::Teleport)
            );
            assert_eq!(locomotor.piggyback, None);
            assert_eq!(entity.navigation.nav_com, None);
            assert!(!entity.navigation.pending_arrival_clear);
            assert!(entity.drive_locomotion.is_none());
            reached_harvest = true;
            break;
        }
    }
    assert!(reached_harvest);
    // The still-driving dispatches draw one scenario RandomRanged(0,2) each
    // (the default Rate epilogue); only the non-scenario streams are
    // untouched across the outbound leg.
    let rng_after = sim.rng_state();
    assert_eq!(rng_after.main, rng_before.main);
    assert_eq!(rng_after.mapgen, rng_before.mapgen);
}

#[test]
fn production_cmin_unreachable_outbound_drive_returns_to_teleport() {
    let oracle = outbound_contract_oracle();
    let target = (32, 29);
    let mut sim = production_sim(0x0715_D004, &oracle);
    let mut grid = PathGrid::test_all_blocked(GRID_SIZE, GRID_SIZE);
    grid.set_blocked(START.0, START.1, false);
    grid.set_blocked(target.0, target.1, false);
    install_world(&mut sim, &oracle, &grid, &[target], true);
    let entity_id = spawn_stock_miner(&mut sim, &oracle, "CMIN", MinerKind::Chrono);
    spawn_inert_dock_instance(&mut sim);
    arm_search(&mut sim, entity_id);
    let rng_before = sim.rng_state();
    let scenario_after_scan = scenario_after_one_epilogue_draw(&mut sim);

    // The scan finds the ore and hands the destination to the mover in the same
    // dispatch. Unit741970 accepts it behind the Drive piggyback; the first
    // Process drops it (the isolated target fails Find_Path's zone precheck
    // and the continuation's recheck -> SetDestination(NULL)), and the idle
    // Drive then ENDs back to Teleport primary with no owner destination.
    advance(&mut sim, &oracle, &grid);
    let before = locomotor_tuple(&sim, entity_id);
    assert_eq!(before.0, LocomotorKind::Teleport);
    assert_eq!(before.1, LocomotorSlot::from_kind(LocomotorKind::Teleport));
    assert_eq!(before.2, None);
    assert_eq!(
        sim.substrate
            .entities
            .get(entity_id)
            .expect("CMIN")
            .navigation
            .nav_com,
        None,
    );
    // The scan dispatch arms the Rate epilogue: one scenario draw.
    let rng_after_scan = sim.rng_state();
    assert_eq!(rng_after_scan.scenario, scenario_after_scan);
    assert_eq!(rng_after_scan.main, rng_before.main);
    assert_eq!(rng_after_scan.mapgen, rng_before.mapgen);

    // A dropped destination must not strand the miner: the next dispatch
    // retries and ends in exactly the same locomotor state.
    arm_dispatch_now(&mut sim, entity_id);
    let scenario_after_retry = scenario_after_one_epilogue_draw(&mut sim);
    advance(&mut sim, &oracle, &grid);
    let entity = sim.substrate.entities.get(entity_id).expect("CMIN");
    assert!(entity.movement_target.is_none());
    assert_eq!(entity.navigation.nav_com, None);
    assert_eq!(locomotor_tuple(&sim, entity_id), before);
    let rng_after_retry = sim.rng_state();
    assert_eq!(rng_after_retry.scenario, scenario_after_retry);
    assert_eq!(rng_after_retry.main, rng_before.main);
    assert_eq!(rng_after_retry.mapgen, rng_before.mapgen);
}

#[test]
fn production_harv_navcom_without_movement_target_is_not_reissued() {
    let oracle = outbound_contract_oracle();
    let original = (32, 29);
    let preferable = (32, 31);
    let mut sim = production_sim(0x0715_D005, &oracle);
    let grid = PathGrid::new(GRID_SIZE, GRID_SIZE);
    // Only `original` carries ore at acquisition time; the nearer `preferable`
    // cell is staged mid-test below so it can tempt a scan that must not run.
    install_world(&mut sim, &oracle, &grid, &[original], true);
    let entity_id = spawn_stock_miner(&mut sim, &oracle, "HARV", MinerKind::War);
    spawn_inert_dock_instance(&mut sim);
    arm_search(&mut sim, entity_id);

    // One dispatch acquires `original` and takes NavCom ownership.
    advance(&mut sim, &oracle, &grid);
    assert_eq!(
        sim.substrate
            .entities
            .get(entity_id)
            .expect("HARV")
            .navigation
            .nav_com,
        Some(NavTargetRef::cell(original.0, original.1)),
    );

    {
        let entity = sim.substrate.entities.get_mut(entity_id).expect("HARV");
        entity.movement_target = None;
    }
    // A strictly nearer ore cell appears. Retail ore is the overlay, so the
    // temptation has to be planted there for the scan to be able to see it.
    sim.overlay_grid
        .as_mut()
        .expect("overlay grid")
        .place_overlay(preferable.0, preferable.1, oracle.tib01, 0);
    let rng_before = sim.rng_state();
    // The still-driving dispatch (non-null NavCom) exits through the default
    // Rate epilogue: exactly one scenario RandomRanged(0,2) draw, no scan.
    arm_dispatch_now(&mut sim, entity_id);
    let expected_scenario_after_draw = scenario_after_one_epilogue_draw(&mut sim);

    advance(&mut sim, &oracle, &grid);
    let entity = sim.substrate.entities.get(entity_id).expect("HARV");
    assert_eq!(
        entity.miner.as_ref().expect("miner").target_ore_cell,
        Some(original),
    );
    assert_eq!(
        entity.navigation.nav_com,
        Some(NavTargetRef::cell(original.0, original.1)),
    );
    assert!(
        entity.movement_target.is_none(),
        "non-null NavCom must suppress scan and command reissue",
    );
    let rng_after = sim.rng_state();
    assert_eq!(rng_after.scenario, expected_scenario_after_draw);
    assert_eq!(rng_after.main, rng_before.main);
    assert_eq!(rng_after.mapgen, rng_before.mapgen);
}

#[test]
fn production_harv_navcom_defers_removed_target_revalidation() {
    let oracle = outbound_contract_oracle();
    let original = (32, 29);
    let replacement = (32, 31);
    let mut sim = production_sim(0x0715_D006, &oracle);
    let grid = PathGrid::new(GRID_SIZE, GRID_SIZE);
    // Only `original` carries ore at acquisition time; `replacement` takes its
    // place mid-test below, after the NavCom is already owned.
    install_world(&mut sim, &oracle, &grid, &[original], true);
    let entity_id = spawn_stock_miner(&mut sim, &oracle, "HARV", MinerKind::War);
    spawn_inert_dock_instance(&mut sim);
    arm_search(&mut sim, entity_id);

    // One dispatch acquires `original` and takes NavCom ownership.
    advance(&mut sim, &oracle, &grid);
    assert_command_state(&sim, &oracle, entity_id, "HARV", original);

    {
        let entity = sim.substrate.entities.get_mut(entity_id).expect("HARV");
        entity.movement_target = None;
        assert_eq!(
            entity.navigation.nav_com,
            Some(NavTargetRef::cell(original.0, original.1)),
            "fixture must isolate the native NavCom owner gate",
        );
    }
    // The owned target is mined out and a nearer cell takes its place. Retail
    // ore is the overlay, so depletion and replacement are overlay edits.
    {
        let overlay_grid = sim.overlay_grid.as_mut().expect("overlay grid");
        overlay_grid.clear_overlay(original.0, original.1);
        overlay_grid.place_overlay(replacement.0, replacement.1, oracle.tib01, 0);
    }
    let rng_before = sim.rng_state();
    // The still-driving dispatch (non-null NavCom) exits through the default
    // Rate epilogue: exactly one scenario RandomRanged(0,2) draw, no scan.
    arm_dispatch_now(&mut sim, entity_id);
    let expected_scenario_after_draw = scenario_after_one_epilogue_draw(&mut sim);

    advance(&mut sim, &oracle, &grid);
    let entity = sim.substrate.entities.get(entity_id).expect("HARV");
    let miner = entity.miner.as_ref().expect("miner");
    assert_eq!(entity.miner_state().unwrap(), MinerState::MoveToOre);
    assert_eq!(miner.target_ore_cell, Some(original));
    assert_eq!(
        entity.navigation.nav_com,
        Some(NavTargetRef::cell(original.0, original.1)),
    );
    assert!(
        entity.movement_target.is_none(),
        "non-null NavCom must defer depletion validation and command reissue",
    );
    let rng_after = sim.rng_state();
    assert_eq!(rng_after.scenario, expected_scenario_after_draw);
    assert_eq!(rng_after.main, rng_before.main);
    assert_eq!(rng_after.mapgen, rng_before.mapgen);
}

#[test]
fn production_cmin_arrival_clears_navcom_same_tick_and_releases_drive() {
    let oracle = outbound_contract_oracle();
    let target = (32, 31);
    let mut sim = production_sim(0x0715_D007, &oracle);
    let grid = PathGrid::new(GRID_SIZE, GRID_SIZE);
    install_world(&mut sim, &oracle, &grid, &[target], true);
    let entity_id = spawn_stock_miner(&mut sim, &oracle, "CMIN", MinerKind::Chrono);
    spawn_inert_dock_instance(&mut sim);
    arm_search(&mut sim, entity_id);

    // The scan dispatch installs the outbound command.
    advance(&mut sim, &oracle, &grid);
    assert_command_state(&sim, &oracle, entity_id, "CMIN", target);

    // Drive leg → arrival tick. A track that ends at the owner destination
    // clears the owner NavCom pair on the SAME tick (no deferred interval),
    // and the same tick's piggyback-restore pass releases the retired Drive.
    let mut arrived = false;
    for _ in 0..128 {
        advance(&mut sim, &oracle, &grid);
        let entity = sim.substrate.entities.get(entity_id).expect("CMIN");
        assert!(entity.teleport_state.is_none());
        if (entity.position.rx, entity.position.ry) == target && entity.movement_target.is_none() {
            arrived = true;
            assert_eq!(entity.navigation.nav_com, None);
            assert!(!entity.navigation.pending_arrival_clear);
            let locomotor = entity.locomotor.as_ref().expect("CMIN locomotor");
            assert_eq!(
                locomotor.kind,
                LocomotorKind::Teleport,
                "Drive={:?}; Foot={:?}; position={:?}; Nav={:?}; mission={:?}",
                entity.drive_locomotion,
                entity.foot_speed,
                entity.position,
                entity.navigation,
                entity.mission
            );
            assert_eq!(
                locomotor.slot,
                LocomotorSlot::from_kind(LocomotorKind::Teleport)
            );
            assert_eq!(locomotor.piggyback, None);
            assert!(
                entity.drive_locomotion.is_none(),
                "restoring primary Teleport must release retired Drive runtime",
            );
            break;
        }
    }
    assert!(arrived, "CMIN must complete the outbound drive leg");

    // The MoveToOre→Harvest transition lands on the next due dispatch; the
    // still-driving dispatches are paced at the Rate-epilogue cadence, so the
    // next due dispatch can be up to ~16 frames after physical arrival.
    let mut reached_harvest = false;
    for _ in 0..32 {
        advance(&mut sim, &oracle, &grid);
        let entity = sim.substrate.entities.get(entity_id).expect("CMIN");
        if entity.miner_state().expect("miner") == MinerState::Harvest {
            reached_harvest = true;
            assert_eq!(entity.navigation.nav_com, None);
            assert!(!entity.navigation.pending_arrival_clear);
            assert!(entity.drive_locomotion.is_none());
            break;
        }
    }
    assert!(
        reached_harvest,
        "MoveToOre→Harvest lands on the next due dispatch"
    );
    assert_ore_intact(&sim, &oracle, target);
}

/// A full Chrono Miner within `ChronoHarvTooFarDistance` of its refinery
/// HELLOs from where it stands, queues Enter, and the refinery's MOVE_HERE
/// warps it onto the pad in one frame: the Unit setter's Teleporter arm keeps
/// the Teleport locomotor while the refinery is its radio contact
/// (`0x007423CD`), and the same object turn's Teleport Process relocates it
/// (`0x007192F0`) with ChronoOutSound at the old cell and ChronoInSound on the
/// pad. It then turns east, unloads and hands back to Harvest.
#[test]
fn cmin_full_close_return_warps_onto_the_pad_and_deposits() {
    let oracle = outbound_contract_oracle();
    let config = MinerConfig::from_rules(&oracle.rules);
    let refinery_anchor = (10, 10);
    let pad = crate::sim::radio::receive::dock_pad_cell(refinery_anchor.0, refinery_anchor.1);
    let mut sim = production_sim(0xDEB6, &oracle);
    seed_human_house(&mut sim, "Americans");
    let mut grid = PathGrid::new(GRID_SIZE, GRID_SIZE);
    let refinery_type = oracle.rules.object("GAREFN").expect("GAREFN");
    grid.block_building_movement_cells(
        refinery_anchor.0,
        refinery_anchor.1,
        &refinery_type.foundation,
        refinery_type.bib,
    );
    install_world(&mut sim, &oracle, &grid, &[], true);
    let refinery_id = spawn_stock_refinery(&mut sim, &oracle, refinery_anchor);
    let entity_id = spawn_stock_miner(&mut sim, &oracle, "CMIN", MinerKind::Chrono);
    arm_full_ore_return(&mut sim, entity_id, &config);
    let credits_before = crate::sim::production::credits_for_owner(&sim, "Americans");

    let mut warped_at = None;
    let mut deposited_at = None;
    for tick in 0..600u32 {
        let before = position_tuple(&sim, entity_id);
        let sounds_before = sim.sound_events.len();
        advance(&mut sim, &oracle, &grid);
        let e = sim.substrate.entities.get(entity_id).expect("miner");
        if warped_at.is_none() && (e.position.rx, e.position.ry) == pad {
            warped_at = Some(tick);
            assert_eq!(
                (before.0, before.1),
                START,
                "no drive: the miner leaves its harvest cell straight for the pad"
            );
            let chrono: Vec<_> = sim.sound_events[sounds_before..]
                .iter()
                .filter_map(|event| match event {
                    crate::sim::world::SimSoundEvent::ChronoTeleport { rx, ry, .. } => {
                        Some((*rx, *ry))
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(chrono, vec![START, pad], "ChronoOut, then ChronoIn");
            let locomotor = e.locomotor.as_ref().expect("CMIN locomotor");
            assert_eq!(locomotor.active_kind(), LocomotorKind::Teleport);
            assert!(locomotor.piggyback.is_none(), "the arrival Drive ended");
            assert_eq!(e.navigation.nav_com, None);
            assert_eq!(e.radio_contacts.slot(0), Some(refinery_id));
        }
        if e.miner.as_ref().expect("miner comp").cargo.is_empty() {
            deposited_at = Some(tick);
            break;
        }
    }
    let warped_at = warped_at.expect("the refinery's MOVE_HERE warps the miner onto the pad");
    let deposited_at = deposited_at.expect("the docked miner unloads its hold");
    assert!(deposited_at > warped_at);
    let paid = crate::sim::production::credits_for_owner(&sim, "Americans") - credits_before;
    assert_eq!(
        paid,
        i32::from(config.ore_bale_value) * i32::from(get_capacity(&sim, entity_id)),
        "the whole hold is paid"
    );

    // Let the state-4 departure and the delayed Harvest redispatch run.
    for _ in 0..100u32 {
        advance(&mut sim, &oracle, &grid);
    }
    let e = sim.substrate.entities.get(entity_id).expect("miner");
    let m = e.miner.as_ref().expect("miner comp");
    assert!(m.cargo.is_empty());
    assert!(!m.unload_active);
    assert!(m.reserved_refinery.is_none());
    assert!(
        e.radio_contacts.is_empty(),
        "OVER_OUT at the end of the unload"
    );
    assert!(
        matches!(
            e.miner_state().expect("cursor"),
            MinerState::SearchOre | MinerState::WaitNoOre
        ),
        "post-deposit cursor resumes harvest scheduling, got {:?}",
        e.miner_state(),
    );
}

fn get_capacity(sim: &Simulation, entity_id: u64) -> u16 {
    sim.substrate
        .entities
        .get(entity_id)
        .and_then(|entity| entity.miner.as_ref())
        .expect("miner")
        .capacity_bales
}

/// After the first deposit the Chrono Miner drives off the pad: Harvest state
/// 0's destination has no radio contact, so the Teleporter arm gives it a
/// Drive (`0x007425E6`). The departure turn from the east-facing pad is sharp
/// (>=135°); an earlier tube-gate regression froze the miner there.
#[test]
fn cmin_second_cycle_leaves_the_pad_and_reharvests() {
    let oracle = outbound_contract_oracle();
    let config = MinerConfig::from_rules(&oracle.rules);
    let refinery_anchor = (10, 10);
    let ore: &[(u16, u16)] = &[(22, 18), (23, 18), (22, 19), (23, 19)];
    let mut sim = production_sim(0xDEB7, &oracle);
    seed_human_house(&mut sim, "Americans");
    let mut grid = PathGrid::new(GRID_SIZE, GRID_SIZE);
    let refinery_type = oracle.rules.object("GAREFN").expect("GAREFN");
    grid.block_building_movement_cells(
        refinery_anchor.0,
        refinery_anchor.1,
        &refinery_type.foundation,
        refinery_type.bib,
    );
    install_world(&mut sim, &oracle, &grid, ore, true);
    // install_world places overlays at density 0 (the outbound suite never
    // extracts). Reduce_Tiberium reads the overlay density byte, so give the
    // patch real density or harvesting yields zero bales.
    for &(rx, ry) in ore {
        sim.overlay_grid
            .as_mut()
            .expect("overlay grid")
            .set_overlay_data(rx, ry, 11);
    }
    let _refinery_id = spawn_stock_refinery(&mut sim, &oracle, refinery_anchor);
    let entity_id = spawn_stock_miner(&mut sim, &oracle, "CMIN", MinerKind::Chrono);
    arm_full_ore_return(&mut sim, entity_id, &config);

    let mut deposited_at = None;
    for tick in 0..3000u32 {
        advance(&mut sim, &oracle, &grid);
        let e = sim.substrate.entities.get(entity_id).expect("miner");
        if e.miner.as_ref().expect("miner comp").cargo.is_empty() {
            deposited_at = Some(tick);
            break;
        }
    }
    deposited_at.expect("cycle 1 deposit completes");

    let mut reharvested = false;
    let mut last = position_tuple(&sim, entity_id);
    for _ in 0..2000u32 {
        advance(&mut sim, &oracle, &grid);
        let now = position_tuple(&sim, entity_id);
        assert!(
            now.0.abs_diff(last.0) <= 1 && now.1.abs_diff(last.1) <= 1,
            "the outbound leg drives; it never warps ({last:?} -> {now:?})"
        );
        last = now;
        let e = sim.substrate.entities.get(entity_id).expect("miner");
        if !e.miner.as_ref().expect("miner comp").cargo.is_empty() {
            reharvested = true;
            break;
        }
    }
    assert!(
        reharvested,
        "miner must drive back out and harvest again after the first deposit \
         (stall = sharp-turn fallback path killed by the tube gate)",
    );
}
