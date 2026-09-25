//! Acceptance tests for the miner (harvester) state machine system.
//!
//! Tests exercise the miner_system::tick_miners() pipeline with a minimal
//! EntityStore: miner entity + refinery structure + tiberium overlays. Verifies
//! payout math, dock queuing, Chrono teleport rules, incremental unloading,
//! local continuation, pip display, and refinery rebinding.

use std::collections::BTreeMap;

use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::{HarvestOverlay, Health, VoxelAnimation};
use crate::sim::game_entity::GameEntity;
use crate::sim::miner::{CargoBale, Miner, MinerConfig, MinerKind, MinerState, ResourceType};
use crate::sim::movement::locomotor::{LocomotorState, MovementLayer};
use crate::sim::occupancy::{CellListInsertion, OccupancyGrid};
use crate::sim::overlay_grid::OverlayGrid;
use crate::sim::pathfinding::PathGrid;
use crate::sim::production::credits_for_owner;
use crate::sim::world::Simulation;

/// Selector0x47 belongs to reciprocal bunker release. Stock refinery
/// contacts must not cause this track to be installed.
fn has_bunker_release_track(entity: &GameEntity) -> bool {
    entity
        .locomotor
        .as_ref()
        .is_some_and(|state| state.kind == LocomotorKind::Drive)
        && entity
            .drive_locomotion
            .as_ref()
            .is_some_and(|drive| drive.track.turn_index == 0x47 && drive.head_to.is_some())
}

/// Minimal rules that know about HARV, CMIN, and GAREFN.
fn miner_rules() -> RuleSet {
    let ini = IniFile::from_str(&format!(
        "{}{}",
        "[InfantryTypes]\n\
         [VehicleTypes]\n\
         0=HARV\n\
         1=CMIN\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         0=GAREFN\n\
         [HARV]\n\
         Name=War Miner\n\
         Cost=1400\n\
         Strength=600\n\
         Armor=heavy\n\
         Speed=4\n\
         ROT=5\n\
         Sight=5\n\
         TechLevel=1\n\
         Owner=Americans\n\
         Harvester=yes\n\
         Dock=GAREFN\n\
         [CMIN]\n\
         Name=Chrono Miner\n\
         Cost=1400\n\
         Strength=400\n\
         Armor=light\n\
         Speed=4\n\
         Sight=5\n\
         TechLevel=1\n\
         Owner=Americans\n\
         Harvester=yes\n\
         Teleporter=yes\n\
         ChronoInSound=ChronoMinerTeleport\n\
         ChronoOutSound=ChronoMinerTeleport\n\
         Dock=GAREFN\n\
         [GAREFN]\n\
         Name=Ore Refinery\n\
         Cost=2000\n\
         Strength=900\n\
         Armor=wood\n\
         TechLevel=1\n\
         Owner=Americans\n\
         Foundation=4x3\n\
         Refinery=yes\nDockUnload=yes\n\
         FreeUnit=CMIN\n\
         [TerrainTypes]\n0=TREE01\n\
         [TREE01]\nStrength=800\nArmor=wood\n\
         [General]\n\
         TiberiumShortScan=6\n\
         TiberiumLongScan=48\n",
        crate::sim::tiberium::test_support::tiberium_rules_text(),
    ));
    let mut rules = RuleSet::from_ini(&ini).expect("miner rules");
    // The retail art section (ARTMD GAREFN) carries `QueueingCell=4,1`.
    rules.merge_art_data(&crate::rules::art_data::ArtRegistry::from_ini(
        &IniFile::from_str("[GAREFN]\nFoundation=4x3\nQueueingCell=4,1\n"),
    ));
    rules
}

fn dock_rules() -> RuleSet {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n\
         [VehicleTypes]\n\
         0=MODHARV\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         0=MODPROC\n\
         1=OTHERPROC\n\
         [MODHARV]\n\
         Name=Mod Harvester\n\
         Harvester=yes\n\
         Dock=MODPROC\n\
         Speed=4\n\
         [MODPROC]\n\
         Name=Mod Refinery\n\
         Foundation=4x3\n\
         Refinery=yes\nDockUnload=yes\n\
         [OTHERPROC]\n\
         Name=Other Refinery\n\
         Foundation=4x3\n\
         Refinery=yes\nDockUnload=yes\n",
    );
    RuleSet::from_ini(&ini).expect("dock rules")
}

/// Spawn a miner entity at (rx, ry), returning its stable_id.
fn spawn_miner(sim: &mut Simulation, sid: u64, kind: MinerKind, rx: u16, ry: u16) -> u64 {
    let type_id = match kind {
        MinerKind::War => "HARV",
        MinerKind::Chrono => "CMIN",
        MinerKind::Slave => "SMIN",
    };
    let health_val: i32 = match kind {
        MinerKind::War => 600,
        MinerKind::Chrono => 400,
        MinerKind::Slave => 2000,
    };
    let owner_id = sim.interner.intern("Americans");
    let type_id_interned = sim.interner.intern(type_id);
    let mut ge = GameEntity::new_at_frame_zero_for_test(
        sid,
        rx,
        ry,
        0,
        0,
        owner_id,
        Health {
            current: health_val,
        },
        type_id_interned,
        EntityCategory::Unit,
        0,
        5,
        true,
    );
    // The locomotor each stock miner has in a match: the class setter
    // (`vt+0x480`) that Mission_Harvest drives it through needs one.
    match kind {
        MinerKind::Chrono => {
            ge.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Teleport));
        }
        MinerKind::War => {
            ge.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
            ge.drive_locomotion = Some(Default::default());
        }
        MinerKind::Slave => {}
    }
    ge.miner = Some(Miner::new(kind, &MinerConfig::default(), 0));
    ge.lifecycle.in_limbo = false;
    sim.substrate.entities.insert(ge);
    // A playfield holding the 64x64 fixture (Is_Cell_Harvestable's first gate).
    sim.playfield_bounds
        .get_or_insert(crate::map::playfield::PlayfieldBounds {
            base: 0,
            off_fc: -64,
            off_100: -1,
            off_104: 128,
            off_108: 65,
        });
    // Update the shared object allocator if needed so test IDs do not collide.
    if sim.substrate.next_stable_object_id <= sid {
        sim.substrate.next_stable_object_id = sid + 1;
    }
    sid
}

/// Spawn a refinery structure at (rx, ry) with a given stable_id.
fn spawn_refinery(sim: &mut Simulation, sid: u64, rx: u16, ry: u16) {
    let owner_id = sim.interner.intern("Americans");
    let type_id = sim.interner.intern("GAREFN");
    let mut ge = GameEntity::new_at_frame_zero_for_test(
        sid,
        rx,
        ry,
        0,
        0,
        owner_id,
        Health { current: 900 },
        type_id,
        EntityCategory::Structure,
        0,
        5,
        false,
    );
    // Placed in its cells' lists: on the map, out of limbo and marked.
    ge.lifecycle.in_limbo = false;
    ge.lifecycle.cell_marked = true;
    sim.substrate.entities.insert(ge);
    occupy_structure_cells(sim, sid, rx, ry, 4, 3);
    if sim.substrate.next_stable_object_id <= sid {
        sim.substrate.next_stable_object_id = sid + 1;
    }
}

fn spawn_structure(sim: &mut Simulation, sid: u64, type_id: &str, rx: u16, ry: u16) {
    spawn_structure_owned(sim, sid, type_id, "Americans", rx, ry);
}

fn spawn_structure_owned(
    sim: &mut Simulation,
    sid: u64,
    type_id: &str,
    owner: &str,
    rx: u16,
    ry: u16,
) {
    let owner_id = sim.interner.intern(owner);
    let type_id_interned = sim.interner.intern(type_id);
    let mut ge = GameEntity::new_at_frame_zero_for_test(
        sid,
        rx,
        ry,
        0,
        0,
        owner_id,
        Health { current: 900 },
        type_id_interned,
        EntityCategory::Structure,
        0,
        5,
        false,
    );
    ge.lifecycle.in_limbo = false;
    sim.substrate.entities.insert(ge);
    occupy_structure_cells(sim, sid, rx, ry, 1, 1);
    if sim.substrate.next_stable_object_id <= sid {
        sim.substrate.next_stable_object_id = sid + 1;
    }
}

/// An owned `Dock=` instance far from the action, with no occupancy and no
/// LogicVector slot: the Harvest preamble (`0x0073E5E0`) queues Guard for a
/// house that owns no instance of any `Dock=` type, so fixtures that only
/// exercise the scan/harvest states still need one on the books.
fn spawn_inert_dock_instance(sim: &mut Simulation) {
    const INERT_DOCK_ID: u64 = 900;
    if sim.substrate.entities.get(INERT_DOCK_ID).is_some() {
        return;
    }
    let owner_id = sim.interner.intern("Americans");
    let type_id = sim.interner.intern("GAREFN");
    let mut ge = GameEntity::new_at_frame_zero_for_test(
        INERT_DOCK_ID,
        60,
        60,
        0,
        0,
        owner_id,
        Health { current: 900 },
        type_id,
        EntityCategory::Structure,
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

fn occupy_structure_cells(
    sim: &mut Simulation,
    sid: u64,
    rx: u16,
    ry: u16,
    width: u16,
    height: u16,
) {
    for y in ry..ry.saturating_add(height) {
        for x in rx..rx.saturating_add(width) {
            sim.substrate.occupancy.add(
                x,
                y,
                sid,
                MovementLayer::Ground,
                None,
                CellListInsertion::AppendBuilding,
            );
        }
    }
}

/// Place ore on a cell. `amount` is in the 120-per-bale units the fixtures
/// were written in; the cell receives the matching bale count (1..=11), which
/// is all a native overlay cell can yield.
/// Ore from an amount in the retired stock units (120 per bale) on a map:
/// the harvest scan reads an ore cell through its map cell (LandType 5,
/// `Is_Cell_Harvestable`), so a fixture without one gets flat ground.
fn place_ore(sim: &mut Simulation, rx: u16, ry: u16, amount: u16) {
    sim.resolved_terrain
        .get_or_insert_with(|| crate::map::resolved_terrain::test_flat_ground_grid(64));
    crate::sim::tiberium::test_support::place_tiberium_on_map(
        sim,
        (rx, ry),
        ResourceType::Ore,
        amount.div_ceil(120).clamp(1, 11) as u8,
    );
}

/// Tick the miner system `n` times.
///
/// Matches advance_tick ordering: teleport (Phase 2) → miners (Phase 7) →
/// ground movement. Teleport must run before miners so that Relocate/ChronoDelay
/// updates are visible to the miner snapshot.
/// TechnoClass::AI's StageClass step (`0x006FABC4`) for every miner, after
/// the mission dispatch as the production host runs it.
fn tick_stages(sim: &mut Simulation) {
    for id in sim.substrate.entities.keys_sorted() {
        super::tick_stage(sim, id);
    }
}

fn tick_miners_n(sim: &mut Simulation, rules: &RuleSet, n: usize) {
    let config = MinerConfig::default();
    let grid = PathGrid::new(64, 64);
    for _ in 0..n {
        sim.session.total_sim_ms = sim.session.total_sim_ms.saturating_add(67);
        sim.session.binary_frame = sim.session.binary_frame.wrapping_add(1);
        crate::sim::movement::teleport_movement::tick_teleport_movement(
            &mut sim.substrate.entities,
            &mut OccupancyGrid::new(),
            &[],
            sim.session.tick,
            None,
            None,
        );
        super::miner_system::tick_miners(sim, rules, &config, Some(&grid));
        tick_stages(sim);
        // Also tick movement so issue_direct_move targets are consumed
        // (Linked/Departing wait for movement_target to be None).
        crate::sim::movement::tick_movement(
            &mut sim.substrate.entities,
            &mut sim.interner,
            &mut sim.pending_lifecycle_requests,
        );
        sim.session.tick += 1;
    }
}

/// Test view over one miner: the cloned Miner component plus the FSM cursor
/// of record (decoded from `MissionCom.handler_state`), so assertions keep
/// reading `.state` alongside the component fields.
struct MinerView {
    miner: Miner,
    state: MinerState,
}

impl std::ops::Deref for MinerView {
    type Target = Miner;
    fn deref(&self) -> &Miner {
        &self.miner
    }
}

impl std::ops::DerefMut for MinerView {
    fn deref_mut(&mut self) -> &mut Miner {
        &mut self.miner
    }
}

/// Read the Miner component + FSM cursor from an entity by stable_id.
/// The miner's NavCom cell, the destination Mission_Harvest's scan gave it.
fn nav_cell(sim: &Simulation, entity_id: u64) -> Option<(u16, u16)> {
    match sim.substrate.entities.get(entity_id)?.navigation.nav_com? {
        crate::sim::components::NavTargetRef::Cell { rx, ry } => Some((rx, ry)),
        _ => None,
    }
}

/// A native zone grid, as map load builds it, over a flat `size` x `size`
/// map split by a water column at `x = wall_x`.
fn native_zones_split_at(sim: &mut Simulation, size: u16, wall_x: u16) {
    use crate::map::resolved_terrain::zone_class;
    use crate::rules::terrain_rules::TerrainClass;
    let mut terrain = crate::sim::tiberium::test_support::flat_terrain(size, size);
    for y in 0..size {
        let cell = terrain.cell_mut(wall_x, y).expect("wall cell");
        cell.is_water = true;
        cell.terrain_class = TerrainClass::Water;
        cell.base_terrain_class = TerrainClass::Water;
        cell.land_type = 2;
        cell.base_land_type = 2;
        cell.yr_cell_land_type = 2;
        cell.base_yr_cell_land_type = 2;
        cell.zone_type = zone_class::WATER;
        cell.ground_walk_blocked = true;
        cell.base_ground_walk_blocked = true;
    }
    let path = PathGrid::from_resolved_terrain(&terrain);
    sim.zone_grid = Some(
        crate::sim::pathfinding::zone_map::ZoneGrid::build_with_native_map_context(
            &path,
            &BTreeMap::new(),
            &terrain,
            &[],
            Some((i32::from(size), i32::from(size))),
            sim.playfield_bounds,
        ),
    );
}

fn get_miner(sim: &Simulation, entity_id: u64) -> MinerView {
    let entity = sim
        .substrate
        .entities
        .get(entity_id)
        .expect("miner entity should exist");
    MinerView {
        miner: entity
            .miner
            .as_ref()
            .cloned()
            .expect("miner component should exist"),
        state: entity.miner_state().expect("miner cursor should decode"),
    }
}

// ==========================================================================
// Test 1: War Miner full ore load = 1000 credits
// ==========================================================================
#[test]
fn war_miner_full_ore_payout_is_1000() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let miner_id = spawn_docked_miner(&mut sim, &[(ResourceType::Ore, 25); 40]);

    let before = credits_for_owner(&sim, "Americans");
    run_unload(&mut sim, &rules, miner_id, 200);

    let after = credits_for_owner(&sim, "Americans");
    assert_eq!(after - before, 1000, "War Miner full ore = 1000 credits");
    assert!(get_miner(&sim, miner_id).cargo.is_empty());
}

// ==========================================================================
// Test 2: War Miner full gem load = 2000 credits
// ==========================================================================
#[test]
fn war_miner_full_gem_payout_is_2000() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let miner_id = spawn_docked_miner(&mut sim, &[(ResourceType::Gem, 50); 40]);

    let before = credits_for_owner(&sim, "Americans");
    run_unload(&mut sim, &rules, miner_id, 200);
    let after = credits_for_owner(&sim, "Americans");
    assert_eq!(after - before, 2000, "War Miner full gems = 2000 credits");
}

// ==========================================================================
// Test 3: Chrono Miner full ore load = 500 credits
// ==========================================================================
#[test]
fn chrono_miner_full_ore_payout_is_500() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let miner_id =
        spawn_docked_miner_of(&mut sim, MinerKind::Chrono, &[(ResourceType::Ore, 25); 20]);

    let before = credits_for_owner(&sim, "Americans");
    run_unload(&mut sim, &rules, miner_id, 200);
    let after = credits_for_owner(&sim, "Americans");
    assert_eq!(after - before, 500, "Chrono Miner full ore = 500 credits");
    assert!(get_miner(&sim, miner_id).cargo.is_empty());
}

// ==========================================================================
// Test 4: Chrono Miner full gem load = 1000 credits
// ==========================================================================
#[test]
fn chrono_miner_full_gem_payout_is_1000() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let miner_id =
        spawn_docked_miner_of(&mut sim, MinerKind::Chrono, &[(ResourceType::Gem, 50); 20]);

    let before = credits_for_owner(&sim, "Americans");
    run_unload(&mut sim, &rules, miner_id, 200);
    let after = credits_for_owner(&sim, "Americans");
    assert_eq!(
        after - before,
        1000,
        "Chrono Miner full gems = 1000 credits"
    );
}

// ==========================================================================
// Test 6: War Miner does NOT teleport (stays where it is on first return tick)
// ==========================================================================
#[test]
fn war_miner_does_not_teleport() {
    let mut sim = Simulation::new();
    let rules = miner_rules();

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 30, 30);
    spawn_refinery(&mut sim, 2, 10, 10);

    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        let miner = entity.miner.as_mut().expect("miner component");
        miner.cargo.push(CargoBale {
            resource_type: ResourceType::Ore,
            value: 25,
        });
        entity
            .mission
            .set_handler_state(MinerState::ReturnToRefinery.cursor());
    }

    tick_miners_n(&mut sim, &rules, 1);

    let pos = &sim
        .substrate
        .entities
        .get(miner_id)
        .expect("entity")
        .position;
    // War miner should NOT have teleported — still at (30, 30).
    assert_eq!((pos.rx, pos.ry), (30, 30), "War Miner should not teleport");
}

// ==========================================================================
// Test 10: Cargo pips always show 5 steps of 20%
// ==========================================================================
#[test]
fn cargo_pips_five_steps() {
    let config = MinerConfig::default();
    let mut miner = Miner::new(MinerKind::War, &config, 0);
    // War Miner capacity = 40 bales
    assert_eq!(miner.cargo_pips(), 0);

    // 20% = 8 bales → 1 pip
    for _ in 0..8 {
        miner.cargo.push(CargoBale {
            resource_type: ResourceType::Ore,
            value: 25,
        });
    }
    assert_eq!(miner.cargo_pips(), 1);

    // 40% = 16 bales → 2 pips
    for _ in 0..8 {
        miner.cargo.push(CargoBale {
            resource_type: ResourceType::Ore,
            value: 25,
        });
    }
    assert_eq!(miner.cargo_pips(), 2);

    // 60% = 24 bales → 3 pips
    for _ in 0..8 {
        miner.cargo.push(CargoBale {
            resource_type: ResourceType::Ore,
            value: 25,
        });
    }
    assert_eq!(miner.cargo_pips(), 3);

    // 80% = 32 bales → 4 pips
    for _ in 0..8 {
        miner.cargo.push(CargoBale {
            resource_type: ResourceType::Ore,
            value: 25,
        });
    }
    assert_eq!(miner.cargo_pips(), 4);

    // 100% = 40 bales → 5 pips
    for _ in 0..8 {
        miner.cargo.push(CargoBale {
            resource_type: ResourceType::Ore,
            value: 25,
        });
    }
    assert_eq!(miner.cargo_pips(), 5);
}

// ==========================================================================
// Test: Chrono miner does NOT warp outbound -- only inbound to refinery
// ==========================================================================
/// Regression: chrono miners warp ONLY on the inbound (ore -> refinery)
/// trip. Outbound (refinery -> ore) is a normal drive, matching the
/// original engine's Mission_Harvest state-0 behaviour (which forces a
/// DriveLocomotion piggyback before Set_Destination so the warp branch
/// is skipped). Reintroducing an outbound warp would be observable as
/// a chrono miner vanishing the instant it leaves the pad.
#[test]
fn chrono_miner_does_not_warp_outbound() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let config = MinerConfig::default();
    let grid = PathGrid::new(64, 64);

    // Chrono miner at the refinery exit cell, empty cargo, entering SearchOre.
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::Chrono, 14, 11);
    spawn_refinery(&mut sim, 2, 10, 10);
    place_ore(&mut sim, 50, 50, 100);

    {
        let entity = sim.substrate.entities.get_mut(miner_id).expect("miner");
        let miner = entity.miner.as_mut().expect("miner component");
        entity
            .mission
            .set_handler_state(MinerState::SearchOre.cursor());
        miner.cargo.clear();
    }

    super::miner_system::tick_miners(&mut sim, &rules, &config, Some(&grid));

    let entity = sim.substrate.entities.get(miner_id).expect("entity");
    assert!(
        entity.teleport_state.is_none(),
        "chrono miner must NOT issue a teleport on outbound SearchOre — \
         only the inbound (ore → refinery) leg warps"
    );
    assert_eq!(nav_cell(&sim, miner_id), Some((50, 50)));
    assert_eq!(entity.miner_state().unwrap(), MinerState::SearchOre);
}

// ==========================================================================
// Test: Chrono Miner drives to ore (does NOT warp — only warps on return)
// ==========================================================================
#[test]
fn chrono_miner_drives_to_ore() {
    let mut sim = Simulation::new();
    let rules = miner_rules();

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::Chrono, 10, 10);
    place_ore(&mut sim, 12, 10, 1200);

    // Mission_Harvest state 0 finds the ore two cells east.
    spawn_inert_dock_instance(&mut sim);

    // After one tick, chrono miner should NOT have a teleport — it drives.
    tick_miners_n(&mut sim, &rules, 1);
    assert_eq!(nav_cell(&sim, miner_id), Some((12, 10)));

    let entity = sim.substrate.entities.get(miner_id).expect("entity");
    assert!(
        entity.teleport_state.is_none(),
        "Chrono Miner should drive to ore, not warp"
    );
}

// ==========================================================================
// Test 13: SearchOre transitions to WaitNoOre when map has no resources
// ==========================================================================
#[test]
fn search_ore_becomes_wait_when_empty() {
    let mut sim = Simulation::new();
    let rules = miner_rules();

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 20, 20);
    spawn_refinery(&mut sim, 2, 10, 10);
    // No ore placed!

    tick_miners_n(&mut sim, &rules, 1);

    let miner = get_miner(&sim, miner_id);
    assert_eq!(
        miner.state,
        MinerState::WaitNoOre,
        "Miner should enter WaitNoOre when no resources exist"
    );
}

// ==========================================================================
// Test 14: the no-ore park sits out the whole wait, then queues Guard
// ==========================================================================
/// gamemd's scan-miss return carries the whole 105-frame wait as the dispatch
/// delay itself, so the frame the wait expires *is* the next dispatch, and
/// that dispatch is `Mission_Harvest` state 4 (`miner_system::
/// handle_going_to_idle`): `Queue_Mission(Guard, 0)` plus the Rate epilogue,
/// with no re-scan. What this pins is: nothing happens early, and the exit is
/// the Guard queue on the exact expiry frame.
#[test]
fn wait_no_ore_queues_guard_when_the_wait_expires() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let config = MinerConfig::default();

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 20, 20);
    spawn_refinery(&mut sim, 2, 10, 10);

    let start_frame = sim.session.binary_frame;
    // The fixed `return 0x69` of the state-0 scan miss (`0x0073E91C`).
    let wait = 0x69u32;
    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        entity
            .mission
            .set_handler_state(MinerState::WaitNoOre.cursor());
        // Reproduce the scan-miss arm exactly: the wait is carried by the
        // dispatch delay and the internal gate mirrors the same expiry, so the
        // two never double-count. Arming only the internal gate would leave the
        // dispatch timer due every frame, making the miner poll at the Rate
        // epilogue's ~14-16 frame cadence and putting the exit frame at the
        // mercy of whichever jitter values that run happened to draw.
        entity
            .mission
            .write_dispatch_epilogue(start_frame as i32, wait as i32);
    }

    // Half way through, still parked.
    let half_cooldown = (wait / 2) as usize;
    tick_miners_n(&mut sim, &rules, half_cooldown);
    assert_eq!(
        get_miner(&sim, miner_id).state,
        MinerState::WaitNoOre,
        "Should still be waiting mid-cooldown"
    );

    // Ore appears inside the scan radius while the miner is parked.
    place_ore(&mut sim, 20, 20, 100);

    // The last frame before the gate opens: fresh ore must not cut the wait short.
    tick_miners_n(&mut sim, &rules, (wait - 1) as usize - half_cooldown);
    assert_eq!(sim.session.binary_frame, start_frame + wait - 1);
    assert_eq!(
        get_miner(&sim, miner_id).state,
        MinerState::WaitNoOre,
        "the 105-frame wait is not shortened by ore appearing during it"
    );

    // The expiry frame is the next dispatch, and that dispatch is gamemd's
    // state 4: `Queue_Mission(Guard, 0)` and the Rate epilogue — never a
    // re-scan, whatever grew during the wait.
    tick_miners_n(&mut sim, &rules, 1);
    assert_eq!(sim.session.binary_frame, start_frame + wait);
    let entity = sim.substrate.entities.get(miner_id).expect("miner");
    assert_eq!(
        entity.mission.queued().known(),
        Some(crate::sim::mission::MissionType::Guard),
        "the wait expiring is the Guard hand-off, on that exact frame"
    );
    assert_eq!(entity.miner_state(), Some(MinerState::WaitNoOre));
    assert_eq!(
        nav_cell(&sim, miner_id),
        None,
        "state 4 does not look at the ore that appeared during the wait",
    );

    // Until the host promotes the queue, every further dispatch re-runs
    // state 4 on the Rate cadence — still no scan.
    let base = rules
        .mission_control
        .rate_frames(crate::sim::mission::MissionType::Harvest);
    tick_miners_n(
        &mut sim,
        &rules,
        (base + crate::sim::mission::authority::RATE_EPILOGUE_JITTER_MAX_FRAMES) as usize,
    );
    let m = get_miner(&sim, miner_id);
    assert_eq!(m.state, MinerState::WaitNoOre);
    assert_eq!(nav_cell(&sim, miner_id), None);
}

// ==========================================================================
// Test 14b (L6): the no-ore retry gate is armed for exactly
// 0x69 = 105 frames — the production arm site must
// not add a fencepost. Exercises the real SearchOre->WaitNoOre transition.
// ==========================================================================
#[test]
fn wait_no_ore_retry_gate_is_exactly_105_frames() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let config = MinerConfig::default();

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 20, 20);
    spawn_refinery(&mut sim, 2, 10, 10);
    // No ore placed anywhere -> SearchOre finds nothing -> WaitNoOre.
    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        let _miner = entity.miner.as_mut().expect("miner component");
        entity
            .mission
            .set_handler_state(MinerState::SearchOre.cursor());
    }
    tick_miners_n(&mut sim, &rules, 1);

    let miner = get_miner(&sim, miner_id);
    assert_eq!(
        miner.state,
        MinerState::WaitNoOre,
        "no reachable ore must drop the miner into WaitNoOre"
    );
    let entity = sim.substrate.entities.get(miner_id).expect("miner");
    assert_eq!(
        entity.mission.dispatch_timer().delay(),
        0x69,
        "no-ore retry gate must be exactly 0x69=105 frames, not 106"
    );
}

#[test]
fn harvester_uses_dock_list_for_refinery_selection() {
    let mut sim = Simulation::new();
    let rules = dock_rules();
    let miner_id = sim
        .spawn_object("MODHARV", "Americans", 30, 30, 64, &rules, &BTreeMap::new())
        .expect("spawn harvester");
    spawn_structure(&mut sim, 2, "OTHERPROC", 28, 28);
    spawn_structure(&mut sim, 3, "MODPROC", 10, 10);

    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        let miner = entity.miner.as_mut().expect("miner component");
        miner.cargo.push(CargoBale {
            resource_type: ResourceType::Ore,
            value: 25,
        });
        entity
            .mission
            .set_handler_state(MinerState::ReturnToRefinery.cursor());
    }

    for wide in [false, true] {
        assert_eq!(
            docking_bay(&sim, &rules, miner_id, wide),
            Some(3),
            "wide={wide}"
        );
    }
}

/// `Receive_Radio(0xF)` `0x0043C422`: a refinery whose online latch a
/// Temporal warp cleared answers 10, so a returning harvester takes the other
/// one even though it is farther away.
#[test]
fn a_warped_refinery_is_passed_over() {
    let mut sim = Simulation::new();
    let rules = dock_rules();
    let miner_id = sim
        .spawn_object("MODHARV", "Americans", 30, 30, 64, &rules, &BTreeMap::new())
        .expect("spawn harvester");
    spawn_structure(&mut sim, 2, "MODPROC", 26, 26);
    spawn_structure(&mut sim, 3, "MODPROC", 10, 10);
    sim.substrate.entities.get_mut(2).unwrap().temporal =
        crate::sim::temporal::TemporalState::warped_by_for_test(999);

    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        let miner = entity.miner.as_mut().expect("miner component");
        miner.cargo.push(CargoBale {
            resource_type: ResourceType::Ore,
            value: 25,
        });
        entity
            .mission
            .set_handler_state(MinerState::ReturnToRefinery.cursor());
    }

    for wide in [false, true] {
        assert_eq!(
            docking_bay(&sim, &rules, miner_id, wide),
            Some(3),
            "wide={wide}"
        );
    }
}

#[test]
fn harvester_queues_guard_when_no_dock_compatible_refinery_exists() {
    let mut sim = Simulation::new();
    let rules = dock_rules();
    let miner_id = sim
        .spawn_object("MODHARV", "Americans", 30, 30, 64, &rules, &BTreeMap::new())
        .expect("spawn harvester");
    spawn_structure(&mut sim, 2, "OTHERPROC", 10, 10);

    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        let miner = entity.miner.as_mut().expect("miner component");
        miner.cargo.push(CargoBale {
            resource_type: ResourceType::Ore,
            value: 25,
        });
        entity
            .mission
            .set_handler_state(MinerState::ReturnToRefinery.cursor());
    }

    tick_miners_n(&mut sim, &rules, 1);

    // No owned instance of any `Dock=` type: the Harvest preamble queues
    // Guard before the state switch, so selection never runs.
    let miner = get_miner(&sim, miner_id);
    assert_eq!(miner.reserved_refinery, None);
    assert_eq!(miner.state, MinerState::ReturnToRefinery);
    assert_eq!(
        sim.substrate
            .entities
            .get(miner_id)
            .expect("miner")
            .mission
            .queued()
            .known(),
        Some(crate::sim::mission::MissionType::Guard)
    );
}

// ==========================================================================
// Test 15: Dock cell calculation for 3x3 foundation
// ==========================================================================
#[test]
fn dock_cell_for_4x3_refinery() {
    // refinery_dock_cell(rx, ry, width, height)
    // Dock is just outside the east edge, vertically centered: (rx + width, ry + height/2).
    // For 4x3 at (10, 10): (10 + 4, 10 + 1) = (14, 11).
    // None = no art.ini QueueingCell override, falls back to geometric computation.
    let dock = super::miner_system::refinery_dock_cell(10, 10);
    assert_eq!(dock, (13, 11));
}

// ==========================================================================
// Dock sequence tests
// ==========================================================================

/// gamemd parity: credits from a harvester deposit go to the REFINERY OWNER,
/// not to the harvester's current controller. The miner docks while both
/// are Americans, then its owner is rewritten to "Russians" (as Yuri's mind
/// control would). The dump credits "Americans" (the owner of the building
/// west of the pad, `0x0073E2BF` → `HouseClass::GiveTiberium @ 0x004F9610`)
/// and "Russians" sees zero delta.
#[test]
fn unloading_credits_refinery_owner_under_mind_control() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let miner_id = spawn_docked_miner(&mut sim, &[(ResourceType::Ore, 25); 5]);

    // Mind-control: rewrite the harvester's owner to a different house.
    let mc_owner = sim.interner.intern("Russians");
    sim.substrate
        .entities
        .get_mut(miner_id)
        .expect("miner entity")
        .owner = mc_owner;

    let americans_before = credits_for_owner(&sim, "Americans");
    let russians_before = credits_for_owner(&sim, "Russians");
    run_unload(&mut sim, &rules, miner_id, 100);
    let americans_after = credits_for_owner(&sim, "Americans");
    let russians_after = credits_for_owner(&sim, "Russians");

    assert_eq!(
        americans_after - americans_before,
        125,
        "refinery owner (Americans) must receive 5 × 25 = 125 credits",
    );
    assert_eq!(
        russians_after - russians_before,
        0,
        "mind-control controller (Russians) must receive zero credits",
    );
}

/// Ore in a disconnected zone (cut off by impassable terrain) must be
/// filtered out by the reachability check. With no reachable ore on the
/// map, the harvester transitions to WaitNoOre rather than picking the
/// unreachable cell.
#[test]
fn unreachable_ore_filtered_out() {
    let mut sim = Simulation::new();

    spawn_inert_dock_instance(&mut sim);
    let rules = miner_rules();

    // Harvester on the LEFT side at (3, 8). Ore on the RIGHT side at (12, 8),
    // beyond a water column at x = 8 (native zones).
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 3, 8);
    native_zones_split_at(&mut sim, 16, 8);
    place_ore(&mut sim, 12, 8, 1200);

    // Drive the miner into SearchOre state.
    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        let _miner = entity.miner.as_mut().expect("miner component");
        entity
            .mission
            .set_handler_state(MinerState::SearchOre.cursor());
    }

    // Tick once — search runs, finds nothing reachable, transitions to WaitNoOre.
    tick_miners_n(&mut sim, &rules, 1);

    let m = get_miner(&sim, miner_id);
    assert_eq!(
        m.state,
        MinerState::WaitNoOre,
        "must wait — only ore on the map is in a disconnected zone, so unreachable",
    );
    assert!(
        nav_cell(&sim, miner_id).is_none(),
        "must not have targeted unreachable ore, got {:?}",
        nav_cell(&sim, miner_id),
    );
}

/// When a closer ore cell is unreachable (different zone) but a farther
/// one is reachable, the harvester must pick the farther reachable cell
/// rather than fall through to WaitNoOre.
#[test]
fn reachable_ore_picked_over_closer_unreachable() {
    let mut sim = Simulation::new();

    spawn_inert_dock_instance(&mut sim);
    let rules = miner_rules();

    // Harvester at (3, 8) on the LEFT side of a water column at x = 8.
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 3, 8);
    native_zones_split_at(&mut sim, 16, 8);
    // Closer ore at (10, 8) is on the RIGHT side (unreachable).
    place_ore(&mut sim, 10, 8, 1200);
    // Farther ore at (1, 1) is on the LEFT side (reachable).
    place_ore(&mut sim, 1, 1, 1200);

    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        let _miner = entity.miner.as_mut().expect("miner component");
        entity
            .mission
            .set_handler_state(MinerState::SearchOre.cursor());
    }

    tick_miners_n(&mut sim, &rules, 1);

    let m = get_miner(&sim, miner_id);
    assert_eq!(m.state, MinerState::SearchOre);
    assert_eq!(
        nav_cell(&sim, miner_id),
        Some((1, 1)),
        "reachable farther ore at (1,1) must be picked over unreachable closer ore at (10,8). \
         Got {:?}",
        nav_cell(&sim, miner_id),
    );
}

// ===========================================================================
// Focused pins for the stock refinery inbound radio FSM.
// ===========================================================================

/// Unloading emits one BaleDepositEvent per StorageClass slot drained — ore
/// first, then gems, one slot per dump gate — plus the empty gate's own
/// (smoke-only) event that ends the dumping state.
#[test]
fn unloading_emits_one_event_per_slot_drain() {
    let rules = miner_rules();
    let mut mixed = vec![(ResourceType::Ore, 25u16); 5];
    mixed.extend([(ResourceType::Gem, 50u16); 3]);
    for (cargo, slot_credits) in [
        (vec![(ResourceType::Ore, 25u16); 5], vec![125]),
        (mixed, vec![125, 150]),
    ] {
        let mut sim = Simulation::new();
        let miner_id = spawn_docked_miner(&mut sim, &cargo);
        let before = credits_for_owner(&sim, "Americans");
        let mut paid = Vec::new();
        for _ in 0..200 {
            let drains = sim.bale_events.iter().filter(|e| e.drained).count();
            let credits = credits_for_owner(&sim, "Americans");
            if run_unload(&mut sim, &rules, miner_id, 1) == 0 {
                break;
            }
            if sim.bale_events.iter().filter(|e| e.drained).count() > drains {
                paid.push(credits_for_owner(&sim, "Americans") - credits);
            }
        }
        assert_eq!(paid, slot_credits, "one drain per slot, ore before gems");
        assert_eq!(
            credits_for_owner(&sim, "Americans") - before,
            slot_credits.iter().sum::<i32>()
        );
        assert_eq!(
            sim.bale_events.len(),
            slot_credits.len() + 1,
            "the drain gates plus the empty gate",
        );
        assert!(
            sim.bale_events
                .last()
                .is_some_and(|e| e.empty && !e.drained)
        );
        assert!(sim.bale_events.iter().all(|e| e.building_id == 2));
    }
}

/// Build a minimal rules with HARV + GAREFN (Refinery) + GAPURI (OrePurifier).
fn purifier_rules(bonus_pct: i32) -> RuleSet {
    let ini = IniFile::from_str(&format!(
        "[General]\nPurifierBonus={}\n\
         [InfantryTypes]\n\
         [VehicleTypes]\n0=HARV\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n0=GAREFN\n1=GAPURI\n\
         [HARV]\n\
         Name=War Miner\nCost=1400\nStrength=600\nArmor=heavy\nSpeed=4\nROT=5\nSight=5\n\
         TechLevel=1\nOwner=Americans\nHarvester=yes\nDock=GAREFN\n\
         [GAREFN]\n\
         Name=Ore Refinery\nCost=2000\nStrength=900\nArmor=wood\nTechLevel=1\n\
         Owner=Americans\nFoundation=4x3\nRefinery=yes\nDockUnload=yes\n\
         [GAPURI]\n\
         Name=Ore Purifier\nCost=2500\nStrength=1000\nArmor=wood\nTechLevel=1\n\
         Owner=Americans\nFoundation=2x2\nOrePurifier=yes\n",
        // Rules expects PurifierBonus= as a fraction; we use the integer-pct path
        // by writing the fraction value (e.g., 0.25 → 25%). Use the float string.
        bonus_pct as f32 / 100.0,
    ));
    RuleSet::from_ini(&ini).expect("purifier rules")
}

// ==========================================================================
// extract_bales_max — test-only bulk-drain primitive over Reduce_Tiberium
// model. It exercises Reduce_Tiberium's clamp for an arbitrary request; the
// harvester itself requests ONE level per gate (see the per-bite block below).
// ==========================================================================

#[test]
fn extract_max_empty_cell() {
    let mut sim = Simulation::new();
    let config = MinerConfig::default();
    let bales =
        super::miner_system::extract_bales_max(&mut sim, &miner_rules(), (5, 5), &config, 40);
    assert!(bales.is_empty(), "no node at cell → no bales");
}

#[test]
fn extract_max_full_drain_ore() {
    let mut sim = Simulation::new();
    let config = MinerConfig::default();
    // 11 density levels of ore at base 120: remaining = 11 * 120 = 1320.
    crate::sim::tiberium::test_support::place_stock_amount(
        &mut sim,
        (5, 5),
        ResourceType::Ore,
        11 * 120,
    );
    let bales =
        super::miner_system::extract_bales_max(&mut sim, &miner_rules(), (5, 5), &config, 40);
    assert_eq!(bales.len(), 11, "full drain extracts 11 bales");
    assert!(
        bales
            .iter()
            .all(|b| b.resource_type == ResourceType::Ore && b.value == config.ore_bale_value),
        "all bales are ore-type with configured value"
    );
    assert!(
        !crate::sim::tiberium::test_support::has_tiberium(&sim, (5, 5)),
        "node removed after full drain"
    );
}

#[test]
fn extract_max_partial_capacity() {
    let mut sim = Simulation::new();
    let config = MinerConfig::default();
    crate::sim::tiberium::test_support::place_stock_amount(
        &mut sim,
        (5, 5),
        ResourceType::Ore,
        11 * 120,
    );
    let bales =
        super::miner_system::extract_bales_max(&mut sim, &miner_rules(), (5, 5), &config, 3);
    assert_eq!(bales.len(), 3, "capacity-limited to 3 bales");
    let after_remaining = crate::sim::tiberium::test_support::stock_amount_at(&sim, (5, 5));
    assert_eq!(
        after_remaining,
        (11 - 3) * 120,
        "remaining decremented by 3 density levels"
    );
}

#[test]
fn extract_max_partial_density_exact_match() {
    let mut sim = Simulation::new();
    let config = MinerConfig::default();
    // 5 density levels of ore: remaining = 600. Empty capacity higher than
    // available density → drain exactly 5, node removed.
    crate::sim::tiberium::test_support::place_stock_amount(
        &mut sim,
        (5, 5),
        ResourceType::Ore,
        5 * 120,
    );
    let bales =
        super::miner_system::extract_bales_max(&mut sim, &miner_rules(), (5, 5), &config, 40);
    assert_eq!(bales.len(), 5, "extracts all 5 available density levels");
    assert!(
        !crate::sim::tiberium::test_support::has_tiberium(&sim, (5, 5)),
        "exact match drains the cell"
    );
}

#[test]
fn extract_max_gem_cell() {
    let mut sim = Simulation::new();
    let config = MinerConfig::default();
    // 4 density levels of gems at base 180.
    crate::sim::tiberium::test_support::place_stock_amount(
        &mut sim,
        (5, 5),
        ResourceType::Gem,
        4 * 180,
    );
    let bales =
        super::miner_system::extract_bales_max(&mut sim, &miner_rules(), (5, 5), &config, 40);
    assert_eq!(bales.len(), 4, "gem cell yields 4 bales");
    assert!(
        bales
            .iter()
            .all(|b| b.resource_type == ResourceType::Gem && b.value == config.gem_bale_value),
        "all bales are gem-type with configured value"
    );
}

#[test]
fn extract_max_zero_capacity() {
    let mut sim = Simulation::new();
    let config = MinerConfig::default();
    crate::sim::tiberium::test_support::place_stock_amount(
        &mut sim,
        (5, 5),
        ResourceType::Ore,
        11 * 120,
    );
    let bales =
        super::miner_system::extract_bales_max(&mut sim, &miner_rules(), (5, 5), &config, 0);
    assert!(bales.is_empty(), "zero capacity → no bales");
    let after_remaining = crate::sim::tiberium::test_support::stock_amount_at(&sim, (5, 5));
    assert_eq!(after_remaining, 11 * 120, "node remaining untouched");
}

#[test]
fn extract_max_node_remaining_zero() {
    let mut sim = Simulation::new();
    let config = MinerConfig::default();
    // A density-0 overlay: `CellClass::ReduceTiberium @ 0x00480A80` takes the
    // full-removal path and returns the density byte, 0.
    crate::sim::tiberium::test_support::place_tiberium(&mut sim, 5, 5, ResourceType::Ore, 1);
    sim.overlay_grid
        .as_mut()
        .expect("overlay grid")
        .set_overlay_data(5, 5, 0);
    let bales =
        super::miner_system::extract_bales_max(&mut sim, &miner_rules(), (5, 5), &config, 40);
    assert!(bales.is_empty(), "density 0 → no bales");
    assert!(!crate::sim::tiberium::test_support::has_tiberium(
        &sim,
        (5, 5)
    ));
}

// ==========================================================================
// Per-bite extraction integration tests (parity contract for harvest_ore_tick)
//
// `UnitClass::Harvest_Ore_Tick` @ 0x0073D450 requests
// `ftol(min(1.0f, Storage - GetTotalAmount()))` from `Reduce_Tiberium`
// (0x0073D556..0x0073D5A1: FILD Storage, FSUBR, FCOMP 1.0f, FLD 1.0f, ftol),
// so each 19-frame gate removes ONE density level, never the free capacity.
// ==========================================================================

/// Minimal stock-shaped tiberium rules plus an overlay registry so a miner
/// test can run the production overlay-grid resource path
/// (real `CellClass::Reduce_Tiberium` shape, including the density-0 overlay).
fn miner_rules_with_tiberium() -> (RuleSet, crate::map::overlay_types::OverlayTypeRegistry) {
    let mut text = String::from(
        "[InfantryTypes]\n\
         [VehicleTypes]\n\
         0=HARV\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         0=GAREFN\n\
         [HARV]\n\
         Name=War Miner\n\
         Cost=1400\n\
         Strength=600\n\
         Speed=4\n\
         Owner=Americans\n\
         Harvester=yes\n\
         Dock=GAREFN\n\
         [GAREFN]\n\
         Name=Ore Refinery\n\
         Foundation=4x3\n\
         Refinery=yes\nDockUnload=yes\n\
         [Tiberiums]\n0=Riparius\n\
         [Riparius]\nImage=1\nValue=25\nGrowth=2200\nGrowthPercentage=.06\n\
         Spread=2200\nSpreadPercentage=.06\n[OverlayTypes]\n",
    );
    let mut tiberium_names = Vec::new();
    for raw_key in (1..=124).filter(|key| *key != 40 && *key != 41) {
        let name = if (105..=116).contains(&raw_key) {
            format!("TIB{:02}", raw_key - 104)
        } else {
            format!("FILL{raw_key:03}")
        };
        text.push_str(&format!("{raw_key}={name}\n"));
        if name.starts_with("TIB") {
            tiberium_names.push(name);
        }
    }
    for name in tiberium_names {
        text.push_str(&format!("[{name}]\nTiberium=yes\n"));
    }
    let ini = IniFile::from_str(&text);
    (
        RuleSet::from_ini(&ini).expect("miner+tiberium rules"),
        crate::map::overlay_types::OverlayTypeRegistry::from_ini(&ini, None),
    )
}

/// Advance `n` frames driving the production overlay-grid resource authority.
fn tick_miners_overlay_n(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: &crate::map::overlay_types::OverlayTypeRegistry,
    n: usize,
) {
    let config = MinerConfig::default();
    let grid = PathGrid::new(64, 64);
    for _ in 0..n {
        sim.session.total_sim_ms = sim.session.total_sim_ms.saturating_add(67);
        sim.session.binary_frame = sim.session.binary_frame.wrapping_add(1);
        super::miner_system::tick_miners_test_walk(
            sim,
            rules,
            &config,
            Some(&grid),
            Some(registry),
        );
        tick_stages(sim);
        crate::sim::movement::tick_movement(
            &mut sim.substrate.entities,
            &mut sim.interner,
            &mut sim.pending_lifecycle_requests,
        );
        sim.session.tick += 1;
    }
}

/// Mission_Harvest state 1 with the StageClass at 9 on the stock
/// `HarvesterLoadRate` 2, so the next dispatch cuts.
fn arm_cutting(sim: &mut Simulation, id: u64) {
    let entity = sim.substrate.entities.get_mut(id).expect("miner entity");
    entity
        .mission
        .set_handler_state(MinerState::Harvest.cursor());
    let miner = entity.miner.as_mut().expect("miner component");
    miner.harvesting = true;
    miner.stage_value = 9;
    miner.stage_rate = 2;
    miner.stage_timer.arm(0, 2);
}

/// Frames between two cuts: the stage re-arms at `HarvesterLoadRate` 2 and
/// counts 9 steps after each dispatch; the dispatch after the ninth cuts.
const GATE: usize = 9 * 2 + 1;

/// A War Miner on an 11-density ore cell takes exactly one bale per gate. The
/// first gate fires on a stage already at 9; every later bale waits the
/// StageClass cadence (`GATE` = 19 frames).
#[test]
fn harvester_takes_one_bale_per_gate_over_eleven_gates() {
    let mut sim = Simulation::new();
    spawn_inert_dock_instance(&mut sim);
    let rules = miner_rules();
    let gate = GATE;

    place_ore(&mut sim, 20, 20, 11 * 120);

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 20, 20);
    arm_cutting(&mut sim, miner_id);

    // Gate 1 fires immediately on the ready stage: one bale, density 10.
    tick_miners_n(&mut sim, &rules, 1);
    {
        let miner = get_miner(&sim, miner_id);
        assert_eq!(miner.cargo.len(), 1, "first gate removes one level");
        assert_eq!(miner.state, MinerState::Harvest);
        assert_eq!(
            (miner.stage_value, miner.stage_rate),
            (0, 2),
            "success re-arms the StageClass at HarvesterLoadRate"
        );
        let after_remaining = crate::sim::tiberium::test_support::stock_amount_at(&sim, (20, 20));
        assert_eq!(after_remaining, 10 * 120, "cell drops by one level");
    }

    // The frames strictly inside a gate extract nothing.
    tick_miners_n(&mut sim, &rules, gate - 1);
    assert_eq!(
        get_miner(&sim, miner_id).cargo.len(),
        1,
        "no extraction before the gate is due"
    );

    // Gates 2..=11: one bale each, 19 frames apart.
    for bale in 2..=11usize {
        tick_miners_n(&mut sim, &rules, 1);
        let miner = get_miner(&sim, miner_id);
        assert_eq!(
            miner.cargo.len(),
            bale,
            "gate {bale} yields exactly one bale"
        );
        if bale < 11 {
            assert_eq!(
                crate::sim::tiberium::test_support::stock_amount_at(&sim, (20, 20)),
                (11 - bale as u16) * 120,
                "cell density tracks bales taken"
            );
            tick_miners_n(&mut sim, &rules, gate - 1);
        }
    }

    let miner = get_miner(&sim, miner_id);
    assert_eq!(miner.cargo.len(), 11, "11 bales after 11 gates");
    assert_eq!(miner.state, MinerState::Harvest, "still cutting ore");
    // The last level leaves a density-0 overlay behind (its clearing bite is
    // covered by `harvester_clears_density_zero_overlay_without_bale_and_moves_on`).
    assert!(crate::sim::tiberium::test_support::has_tiberium(
        &sim,
        (20, 20)
    ));
    assert_eq!(
        crate::sim::tiberium::test_support::bales_at(&sim, 20, 20),
        0
    );
    assert_eq!(
        sim.session.binary_frame,
        1 + 10 * gate as u32,
        "11 gates span 1 + 10 * 19 frames"
    );

    // Gate 12 finds nothing to cut: no bale, no re-arm, miner moves on
    // (short scan miss with no refinery -> return path, not Harvest).
    tick_miners_n(&mut sim, &rules, gate);
    let miner = get_miner(&sim, miner_id);
    assert_eq!(miner.cargo.len(), 11, "exhausted cell yields no bale");
    assert_ne!(miner.state, MinerState::Harvest, "miner leaves the cell");
}

/// Production overlay path: after the eleventh bite the overlay sits at
/// density 0 (`Reduce_Tiberium(1)` on data 1 is the partial path, leaving
/// data 0). The next gate's `Reduce_Tiberium(1)` on data 0 takes the full
/// removal path and returns 0: the overlay clears, no bale is credited, the
/// timer is not re-armed, and Mission_Harvest moves the miner on.
#[test]
fn harvester_clears_density_zero_overlay_without_bale_and_moves_on() {
    let (rules, registry) = miner_rules_with_tiberium();
    let tib01 = registry.id_for_name("TIB01").expect("TIB01");
    let gate = GATE;
    let cell = (20u16, 20u16);
    let next = (21u16, 20u16);

    let mut sim = Simulation::new();

    spawn_inert_dock_instance(&mut sim);
    sim.overlay_grid = Some(OverlayGrid::new(64, 64));
    {
        let overlay = sim.overlay_grid.as_mut().expect("overlay grid");
        overlay.place_overlay(cell.0, cell.1, tib01, 11);
        // A neighbouring patch so the post-exhaustion short scan has a hit.
        overlay.place_overlay(next.0, next.1, tib01, 3);
    }
    // Map cells: the scan and the ore tick read the ore cells' LandType.
    sim.resolved_terrain = Some(crate::map::resolved_terrain::test_flat_ground_grid(64));
    for at in [cell, next] {
        let (Some(overlay), Some(terrain)) =
            (sim.overlay_grid.as_mut(), sim.resolved_terrain.as_mut())
        else {
            unreachable!("both installed above");
        };
        overlay.recalculate_runtime_cell(
            terrain,
            crate::sim::tiberium::test_support::overlay_registry_with_land(),
            at,
            crate::sim::overlay_grid::NavigationPublication::FrameBoundary,
        );
    }

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, cell.0, cell.1);
    arm_cutting(&mut sim, miner_id);

    let density = |sim: &Simulation| {
        let overlay = sim.overlay_grid.as_ref().expect("overlay grid");
        let c = overlay.cell(cell.0, cell.1);
        (c.overlay_id, c.overlay_data)
    };

    // Eleven gates: one level each, overlay stays present down to data 0.
    for bale in 1..=11u8 {
        tick_miners_overlay_n(&mut sim, &rules, &registry, 1);
        let miner = get_miner(&sim, miner_id);
        assert_eq!(
            miner.cargo.len(),
            usize::from(bale),
            "gate {bale}: one bale"
        );
        assert_eq!(
            miner.state,
            MinerState::Harvest,
            "gate {bale}: still cutting"
        );
        assert_eq!(
            density(&sim),
            (Some(tib01), 11 - bale),
            "gate {bale}: overlay present, one level lower"
        );
        assert_eq!(
            (miner.stage_value, miner.stage_rate),
            (0, 2),
            "gate {bale}: success re-arms the StageClass"
        );
        tick_miners_overlay_n(&mut sim, &rules, &registry, gate - 1);
        assert_eq!(
            get_miner(&sim, miner_id).cargo.len(),
            usize::from(bale),
            "gate {bale}: nothing between gates"
        );
    }
    assert_eq!(density(&sim), (Some(tib01), 0), "density-0 overlay remains");
    assert_eq!(
        sim.session.binary_frame,
        11 * gate as u32,
        "11 gates plus 11 waits"
    );

    // Gate 12: full-removal path on data 0 returns 0 -> overlay cleared, no
    // bale; state 1 sends the miner to the neighbouring patch and stays.
    tick_miners_overlay_n(&mut sim, &rules, &registry, 1);
    let miner = get_miner(&sim, miner_id);
    assert_eq!(miner.cargo.len(), 11, "density-0 gate credits nothing");
    assert_eq!(density(&sim), (None, 0), "density-0 overlay is cleared");
    assert_eq!(
        miner.state,
        MinerState::Harvest,
        "state 1 holds for the hop"
    );
    assert!(miner.harvesting, "Unit+0x6D2 stays up for the hop");
    assert_eq!(
        nav_cell(&sim, miner_id),
        Some(next),
        "short scan picked the neighbour"
    );
    assert_eq!(
        (
            sim.overlay_grid
                .as_ref()
                .expect("grid")
                .cell(next.0, next.1)
                .overlay_id,
            sim.overlay_grid
                .as_ref()
                .expect("grid")
                .cell(next.0, next.1)
                .overlay_data
        ),
        (Some(tib01), 3),
        "the neighbouring patch is untouched"
    );
}

/// A nearly full miner still takes exactly one level per gate; capacity never
/// widens the request. 38/40 loaded -> 39 after one gate, cell 11 -> 10.
#[test]
fn harvester_caps_extraction_at_remaining_capacity() {
    let mut sim = Simulation::new();
    spawn_inert_dock_instance(&mut sim);
    let rules = miner_rules();
    let config = MinerConfig::default();

    place_ore(&mut sim, 20, 20, 11 * 120);

    // War Miner with 38 of 40 bales already loaded — only 2 free slots.
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 20, 20);
    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        let miner = entity.miner.as_mut().expect("miner component");
        for _ in 0..38 {
            miner.cargo.push(CargoBale {
                resource_type: ResourceType::Ore,
                value: config.ore_bale_value,
            });
        }
    }
    arm_cutting(&mut sim, miner_id);

    tick_miners_n(&mut sim, &rules, 1);

    let miner = get_miner(&sim, miner_id);
    assert_eq!(miner.cargo.len(), 39, "one level per gate even with 2 free");
    assert_eq!(
        miner.state,
        MinerState::Harvest,
        "positive extraction remains a successful Harvest tick"
    );
    assert_eq!(
        (miner.stage_value, miner.stage_rate),
        (0, 2),
        "success re-arms the StageClass"
    );
    let after_remaining = crate::sim::tiberium::test_support::stock_amount_at(&sim, (20, 20));
    assert_eq!(after_remaining, 10 * 120, "cell drops to density 10");

    // The next gate takes the fortieth bale: filling is still a success.
    tick_miners_n(&mut sim, &rules, GATE);

    let miner = get_miner(&sim, miner_id);
    assert_eq!(miner.cargo.len(), 40, "capped at capacity");
    assert_eq!(
        miner.state,
        MinerState::Harvest,
        "positive filling extraction remains a successful Harvest tick"
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(miner_id)
            .unwrap()
            .archive_target(),
        None,
        "archive is not selected on fill"
    );
    assert_eq!(
        miner.reserved_refinery, None,
        "return does not begin on fill"
    );

    let entity = sim.substrate.entities.get(miner_id).expect("miner entity");
    assert!(entity.movement_target.is_none());
    assert!(entity.teleport_state.is_none());

    let after_remaining = crate::sim::tiberium::test_support::stock_amount_at(&sim, (20, 20));
    assert_eq!(after_remaining, 9 * 120, "cell drops to density 9");
}

#[test]
fn filling_extraction_waits_for_full_gate_before_war_return() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let config = MinerConfig::default();
    place_ore(&mut sim, 30, 30, 11 * 120);
    place_ore(&mut sim, 31, 30, 5 * 120);
    spawn_refinery(&mut sim, 2, 10, 10);
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 30, 30);

    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        let miner = entity.miner.as_mut().expect("miner component");
        // 39 of 40 loaded: the single-level gate request fills on one bite.
        for _ in 0..39 {
            miner.cargo.push(CargoBale {
                resource_type: ResourceType::Ore,
                value: config.ore_bale_value,
            });
        }
        let mut voxel = VoxelAnimation::new(15, 1);
        voxel.frame = 7;
        voxel.elapsed_frames = 1;
        voxel.playing = true;
        entity.voxel_animation = Some(voxel);
        entity.harvest_overlay = Some(HarvestOverlay {
            frame: 6,
            visible: true,
            elapsed_frames: 0,
        });
    }
    arm_cutting(&mut sim, miner_id);

    tick_miners_n(&mut sim, &rules, 1);
    let fill_frame = sim.session.binary_frame;
    {
        let entity = sim.substrate.entities.get(miner_id).expect("miner entity");
        let miner = entity.miner.as_ref().expect("miner component");
        assert_eq!(miner.cargo.len(), 40);
        assert_eq!(entity.miner_state().unwrap(), MinerState::Harvest);
        assert_eq!(miner.stage_timer.start_frame, fill_frame);
        assert_eq!((miner.stage_value, miner.stage_rate), (0, 2));
        assert_eq!(entity.archive_target(), None);
        assert_eq!(miner.reserved_refinery, None);
        assert!(entity.movement_target.is_none());
        assert!(entity.teleport_state.is_none());
        let voxel = entity.voxel_animation.expect("voxel anim");
        assert!(voxel.playing);
        assert_eq!((voxel.frame, voxel.elapsed_frames), (7, 1));
        let overlay = entity.harvest_overlay.expect("harvest overlay");
        assert!(overlay.visible);
        assert_eq!((overlay.frame, overlay.elapsed_frames), (6, 0));
    }

    tick_miners_n(&mut sim, &rules, GATE - 1);
    assert_eq!(
        sim.session.binary_frame.wrapping_sub(fill_frame),
        (GATE - 1) as u32
    );
    {
        let entity = sim.substrate.entities.get(miner_id).expect("miner entity");
        let miner = entity.miner.as_ref().expect("miner component");
        assert_eq!(
            entity.miner_state().unwrap(),
            MinerState::Harvest,
            "F+18 remains pending"
        );
        assert_eq!(
            miner.stage_value, 9,
            "the ninth step lands after F+18's dispatch"
        );
        assert_eq!(entity.archive_target(), None);
        assert_eq!(miner.reserved_refinery, None);
        assert!(entity.movement_target.is_none());
        let voxel = entity.voxel_animation.expect("voxel anim");
        assert!(voxel.playing);
        assert_eq!(
            (voxel.frame, voxel.elapsed_frames),
            (7, 1),
            "nonzero visual state remains live through F+18"
        );
        let overlay = entity.harvest_overlay.expect("harvest overlay");
        assert!(overlay.visible);
        assert_eq!(
            (overlay.frame, overlay.elapsed_frames),
            (6, 0),
            "nonzero overlay state remains live through F+18"
        );
    }

    crate::sim::tiberium::test_support::clear_tiberium(&mut sim, (30, 30));
    tick_miners_n(&mut sim, &rules, 1);
    let full_gate_frame = sim.session.binary_frame;
    {
        let entity = sim.substrate.entities.get(miner_id).expect("miner entity");
        let miner = entity.miner.as_ref().expect("miner component");
        assert_eq!(full_gate_frame.wrapping_sub(fill_frame), GATE as u32);
        assert_eq!(entity.miner_state().unwrap(), MinerState::ReturnToRefinery);
        assert_eq!(miner.stage_timer.start_frame, full_gate_frame);
        assert_eq!(miner.stage_timer.duration, 0);
        assert_eq!(miner.stage_rate, 0, "the full gate resets the StageClass");
        assert_eq!(
            entity.archive_target(),
            Some(crate::sim::combat::TargetKind::Cell(31, 30))
        );
        assert_eq!(miner.reserved_refinery, None);
        assert!(entity.movement_target.is_none());
        assert!(entity.teleport_state.is_none());
        let voxel = entity.voxel_animation.expect("voxel anim");
        assert!(!voxel.playing);
        assert_eq!((voxel.frame, voxel.elapsed_frames), (0, 0));
        let overlay = entity.harvest_overlay.expect("harvest overlay");
        assert!(!overlay.visible);
        assert_eq!((overlay.frame, overlay.elapsed_frames), (0, 0));
    }

    open_playfield(&mut sim);
    give_drive(&mut sim, miner_id);
    tick_miners_n(&mut sim, &rules, 1);
    {
        let entity = sim.substrate.entities.get(miner_id).expect("miner entity");
        assert!(
            entity.radio_contacts.is_empty(),
            "no HELLO beyond the too-far distance"
        );
        assert_eq!(
            entity.navigation.nav_com,
            Some(crate::sim::components::NavTargetRef::cell(14, 11)),
            "F+20 state-2 dispatch stages the far return at NW + QueueingCell"
        );
    }
}

/// Mission_Unload's dump reads the building WEST of the miner's cell on every
/// dispatch (`0x0073E2BF`), not its radio contact: the credit goes to that
/// building's owner.
#[test]
fn unload_pays_the_west_cell_building_not_the_radio_contact() {
    let mut sim = Simulation::new();
    let rules = miner_rules();

    spawn_refinery(&mut sim, 2, 30, 30);
    spawn_structure_owned(&mut sim, 3, "GAREFN", "Germans", 12, 11);
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 13, 11);
    load_cargo(&mut sim, miner_id, &[(ResourceType::Ore, 100)]);
    dock_for_unload(&mut sim, miner_id, 2);

    let americans_before = credits_for_owner(&sim, "Americans");
    let germans_before = credits_for_owner(&sim, "Germans");
    run_unload(&mut sim, &rules, miner_id, 200);

    assert_eq!(credits_for_owner(&sim, "Americans"), americans_before);
    assert_eq!(credits_for_owner(&sim, "Germans") - germans_before, 100);
    let drained: Vec<_> = sim.bale_events.iter().filter(|e| e.drained).collect();
    assert_eq!(drained.len(), 1);
    assert_eq!(drained[0].building_id, 3);
}

/// No building west of the miner (`0x0073E311..0x0073E350`): the dump state
/// sends OVER_OUT and queues Harvest; nothing is paid and no deposit event
/// fires, whatever the radio contact is.
#[test]
fn missing_west_cell_building_does_not_credit_or_emit_deposit_event() {
    let mut sim = Simulation::new();
    let rules = miner_rules();

    spawn_refinery(&mut sim, 2, 30, 30);
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 13, 11);
    load_cargo(&mut sim, miner_id, &[(ResourceType::Ore, 100)]);
    dock_for_unload(&mut sim, miner_id, 2);

    let credits_before = credits_for_owner(&sim, "Americans");
    for _ in 0..60 {
        visit_miner(&mut sim, &rules, miner_id);
    }

    let entity = sim.substrate.entities.get(miner_id).expect("miner");
    assert_eq!(entity.miner.as_ref().unwrap().cargo.len(), 1);
    assert_eq!(credits_for_owner(&sim, "Americans"), credits_before);
    assert!(sim.bale_events.is_empty());
    assert!(
        !entity.radio_contacts.contains(2),
        "OVER_OUT left the contact"
    );
    assert_ne!(
        entity.mission.current().known(),
        Some(crate::sim::mission::MissionType::Unload),
        "Harvest was queued and took over"
    );
}

/// `House+0x538C` is incremented only by `BuildingClass::OnConstructionComplete`
/// (0x0044636C..0x0044637C), so a purifier still in its build-up anim pays no
/// deposit bonus; once the build-up completes it pays. One purifier building
/// up plus one completed ⇒ exactly +25% (count 1), not +50%.
#[test]
fn purifier_under_construction_pays_no_bonus_until_complete() {
    use crate::sim::components::BuildingUp;

    let mut sim = Simulation::new();
    let rules = purifier_rules(25);

    let miner_id = spawn_docked_miner(&mut sim, &[(ResourceType::Ore, 100)]);
    spawn_structure(&mut sim, 3, "GAPURI", 20, 20);
    spawn_structure(&mut sim, 4, "GAPURI", 24, 20);
    sim.substrate
        .entities
        .get_mut(4)
        .expect("purifier 4")
        .building_up = Some(BuildingUp {
        elapsed_ticks: 0,
        total_ticks: 1000,
    });
    assert_eq!(
        super::miner_system::count_purifiers_for_owner(&sim, &rules, "Americans"),
        1,
        "a building-up purifier is not in House+0x538C yet"
    );

    let credits_before = credits_for_owner(&sim, "Americans");
    run_unload(&mut sim, &rules, miner_id, 200);
    let delta = credits_for_owner(&sim, "Americans") - credits_before;
    assert_eq!(
        delta, 125,
        "only the completed purifier pays: 100 + 25, got {delta}"
    );

    // Build-up complete ⇒ the second purifier is counted.
    sim.substrate
        .entities
        .get_mut(4)
        .expect("purifier 4")
        .building_up = None;
    assert_eq!(
        super::miner_system::count_purifiers_for_owner(&sim, &rules, "Americans"),
        2
    );
}

/// `AIVirtualPurifiers=` is indexed by the paying house's own difficulty, and
/// only outside campaigns (`[0x00A8B238] != 0`, 0x0073E3F6).
#[test]
fn virtual_purifier_table_is_indexed_per_refinery_owner() {
    use crate::sim::house_state::{HouseDifficulty, HouseState};

    let mut sim = Simulation::new();
    sim.session.game_mode_nonzero = true;
    let rules = purifier_rules(25);

    for (name, difficulty) in [
        ("HardOwner", HouseDifficulty::Hard),
        ("NormalOwner", HouseDifficulty::Normal),
        ("EasyOwner", HouseDifficulty::Easy),
    ] {
        let owner = sim.interner.intern(name);
        let mut house = HouseState::new(owner, 0, None, false, 0, 10);
        house.difficulty = difficulty;
        sim.houses.insert(owner, house);
    }

    assert_eq!(
        super::miner_system::effective_purifier_count(&sim, &rules, "HardOwner"),
        4,
    );
    assert_eq!(
        super::miner_system::effective_purifier_count(&sim, &rules, "NormalOwner"),
        2,
    );
    assert_eq!(
        super::miner_system::effective_purifier_count(&sim, &rules, "EasyOwner"),
        0,
    );

    sim.session.game_mode_nonzero = false;
    assert_eq!(
        super::miner_system::effective_purifier_count(&sim, &rules, "HardOwner"),
        0,
        "campaign AI houses get no virtual purifiers"
    );
}

/// A miner killed while it holds the refinery's contact slot frees it at once.
/// A voxel miner has no death animation, so the damage receiver uninits it in
/// the same transaction (`immediate_uninit_ids` -> `uninit_with_rules` ->
/// `techno_limbo_with_context`), whose `broadcast_break` reaches the refinery.
/// No per-frame sweep backs this up.
#[test]
fn killed_occupant_releases_dock_to_queued_miner() {
    let mut sim = Simulation::new();
    let rules = miner_rules();

    let occupant = spawn_miner(&mut sim, 1, MinerKind::War, 14, 11);
    let waiter = spawn_miner(&mut sim, 3, MinerKind::War, 14, 12);
    spawn_refinery(&mut sim, 2, 10, 10);

    assert!(crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, occupant));
    assert!(!crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, waiter));

    sim.uninit_with_rules(occupant, &rules);

    assert!(!crate::sim::miner::miner_dock::has_contact(
        &sim, 2, occupant
    ));
    assert!(
        crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, waiter),
        "the waiter wins the freed slot on its next probe"
    );
}

/// An object that keeps a corpse for its death animation is still Limbo'd
/// natively at the moment it dies. The BREAK is sent at that edge, not when the
/// animation ends, so the corpse never holds the dock.
#[test]
fn dying_corpse_break_frees_the_dock_before_its_animation_ends() {
    let mut sim = Simulation::new();

    let occupant = spawn_miner(&mut sim, 1, MinerKind::War, 14, 11);
    let waiter = spawn_miner(&mut sim, 3, MinerKind::War, 14, 12);
    spawn_refinery(&mut sim, 2, 10, 10);
    assert!(crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, occupant));
    crate::sim::miner::miner_dock::test_support::enter_dock(&mut sim, occupant, 2);

    // What the damage receiver does for a unit with a death animation.
    sim.substrate.entities.get_mut(occupant).unwrap().dying = true;
    crate::sim::radio::broadcast_break(&mut sim, occupant, None);

    let corpse = sim.substrate.entities.get(occupant).expect("corpse stays");
    assert!(!corpse.radio_contacts.contains(2));
    assert_eq!(corpse.dock_entered_with, None);
    assert!(crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, waiter));
}

/// A refinery captured by another house leaves the miner house's building
/// list, so the return passes it over for the house's other one.
#[test]
fn a_captured_refinery_is_no_longer_a_return_target() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 5, 10);
    spawn_refinery(&mut sim, 2, 20, 10);
    spawn_refinery(&mut sim, 3, 40, 10);
    fill_and_return(&mut sim, miner_id);
    assert_eq!(docking_bay(&sim, &rules, miner_id, true), Some(2));

    let captor = sim.interner.intern("Russians");
    sim.change_owner(2, captor);

    for wide in [false, true] {
        assert_eq!(
            docking_bay(&sim, &rules, miner_id, wide),
            Some(3),
            "wide={wide}"
        );
    }
}

/// `EventClass::Execute`'s MEGAMISSION arm (`0x004C72E8..0x004C7342`) BREAKs the
/// radio link of an untethered unit, and of a tethered one whose contact is a
/// refinery (untethering both ends through the refinery's OVER_OUT reply). A
/// miner ordered away before the unload frees the refinery's slot for the
/// next miner's HELLO.
#[test]
fn megamission_before_the_unload_breaks_the_refinery_contact() {
    let rules = miner_rules();
    for tethered in [false, true] {
        let mut sim = Simulation::new();
        let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 13, 11);
        let waiter = spawn_miner(&mut sim, 3, MinerKind::War, 14, 12);
        spawn_refinery(&mut sim, 2, 10, 10);
        for (id, partner) in [(miner_id, 2), (2, miner_id)] {
            let entity = sim.substrate.entities.get_mut(id).unwrap();
            entity.radio_contacts.set_slot(0, partner);
            if tethered {
                entity.dock_entered_with = Some(partner);
            }
        }

        sim.queue_megamission_with_teardown(
            miner_id,
            crate::sim::mission::MissionType::Move,
            crate::sim::mission::DockTeardown::All,
            Some(&rules),
        );

        let entity = sim.substrate.entities.get(miner_id).unwrap();
        assert!(!entity.radio_contacts.contains(2), "tethered={tethered}");
        assert_eq!(entity.dock_entered_with, None, "tethered={tethered}");
        let refinery = sim.substrate.entities.get(2).unwrap();
        assert!(
            !refinery.radio_contacts.contains(miner_id),
            "tethered={tethered}"
        );
        assert_eq!(refinery.dock_entered_with, None, "tethered={tethered}");
        assert!(
            crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, waiter),
            "tethered={tethered}"
        );
    }
}

/// A Move ordered mid-unload BREAKs the contact but leaves the unload to the
/// Unload mission's own contact gate (`0x0073DEE0`): the next dispatch drops
/// the unload latch and image and commences the queued order, with no
/// further slot dumped.
#[test]
fn megamission_mid_unload_abandons_the_unload_and_commences_the_order() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let miner_id = spawn_docked_miner(&mut sim, &[(ResourceType::Ore, 25u16); 20]);
    run_unload(&mut sim, &rules, miner_id, 1);
    let before = get_miner(&sim, miner_id);
    assert!(before.unload_active);
    let cargo_before = before.cargo.len();
    assert!(
        cargo_before > 0,
        "the order arrives with cargo still aboard"
    );

    sim.queue_megamission_with_teardown(
        miner_id,
        crate::sim::mission::MissionType::Move,
        crate::sim::mission::DockTeardown::All,
        Some(&rules),
    );
    assert!(!crate::sim::miner::miner_dock::has_contact(
        &sim, 2, miner_id
    ));

    let mut commenced = false;
    for _ in 0..60 {
        visit_miner(&mut sim, &rules, miner_id);
        let entity = sim.substrate.entities.get(miner_id).unwrap();
        if entity.mission.current().known() == Some(crate::sim::mission::MissionType::Move) {
            commenced = true;
            break;
        }
    }
    assert!(
        commenced,
        "the queued Move must commence, not wait out a re-dock"
    );
    let entity = sim.substrate.entities.get(miner_id).unwrap();
    let miner = entity.miner.as_ref().unwrap();
    assert!(!miner.unload_active, "the unload latch is dropped");
    assert_eq!(
        entity.display_type_override, None,
        "and the unload image with it"
    );
    assert_eq!(
        miner.cargo.len(),
        cargo_before,
        "no further slot was dumped"
    );
    assert!(!crate::sim::miner::miner_dock::has_contact(
        &sim, 2, miner_id
    ));
}

/// `Command::HarvestCell` assigns Harvest directly, so the Unload mission's
/// contact gate never runs again for it. The command itself must drop the
/// unload latch and image, or the miner drives to the ore wearing its
/// UnloadingClass image and every later queued order waits for its next dock.
#[test]
fn harvest_order_mid_unload_drops_the_unload_latch_and_image() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let miner_id = spawn_docked_miner(&mut sim, &[(ResourceType::Ore, 25u16); 20]);
    run_unload(&mut sim, &rules, miner_id, 1);
    assert!(get_miner(&sim, miner_id).unload_active);

    assert!(sim.apply_command(
        "Americans",
        &crate::sim::command::Command::HarvestCell {
            entity_id: miner_id,
            target_rx: 30,
            target_ry: 30,
        },
        Some(&rules),
        None,
        &BTreeMap::new(),
    ));

    let entity = sim.substrate.entities.get(miner_id).unwrap();
    let miner = entity.miner.as_ref().unwrap();
    assert!(!miner.unload_active);
    assert_eq!(entity.display_type_override, None);
    assert_eq!(entity.dock_entered_with, None);
    assert!(!crate::sim::miner::miner_dock::has_contact(
        &sim, 2, miner_id
    ));
}

/// The IDLE arm returns at `0x004C7504..0x004C750C` for a tethered object, so a
/// miner that has entered its dock ignores Stop; an untethered one has every
/// radio link broken (`0x004C75DC`).
#[test]
fn stop_breaks_an_untethered_refinery_contact_and_is_ignored_once_entered() {
    for entered in [false, true] {
        let mut sim = Simulation::new();
        let rules = miner_rules();
        let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 14, 11);
        spawn_refinery(&mut sim, 2, 10, 10);
        {
            let entity = sim.substrate.entities.get_mut(miner_id).unwrap();
            let miner = entity.miner.as_mut().unwrap();
            miner.reserved_refinery = Some(2);
        }
        assert!(
            crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, miner_id)
        );
        if entered {
            crate::sim::miner::miner_dock::test_support::enter_dock(&mut sim, miner_id, 2);
        }

        assert!(sim.apply_command(
            "Americans",
            &crate::sim::command::Command::Stop {
                entity_id: miner_id
            },
            Some(&rules),
            None,
            &BTreeMap::new(),
        ));

        assert_eq!(
            crate::sim::miner::miner_dock::has_contact(&sim, 2, miner_id),
            entered,
            "entered={entered}"
        );
    }
}

/// `Find_Docking_Bay` passes over a refinery in its death animation (native
/// removes a dead building from the house list through Limbo), so a full
/// miner's return picks the remaining live one in either pass.
#[test]
fn full_miner_return_passes_over_a_dying_refinery() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 5, 10);
    spawn_refinery(&mut sim, 2, 10, 10);
    spawn_refinery(&mut sim, 3, 24, 10);
    fill_and_return(&mut sim, miner_id);
    sim.substrate.entities.get_mut(2).expect("refinery").dying = true;

    for wide in [false, true] {
        assert_eq!(
            docking_bay(&sim, &rules, miner_id, wide),
            Some(3),
            "wide={wide}"
        );
    }
}

// ---------------------------------------------------------------------------
// Scan filter: cell-occupancy + path-grid exclusion (gamemd parity)
//
// Mirrors gamemd's `Scan_For_Tiberium` → `Is_Cell_Harvestable` →
// `UnitClass::Can_Enter_Cell` (vtable+0x1AC at 0x0073F0A0): rings 1+
// reject cells with vehicle occupants, terrain objects, or building
// footprints. Ring 0 (the harvester's own cell) is always allowed.
// ---------------------------------------------------------------------------

/// Path-grid-blocked ore cell (e.g. tree on tiberium) is rejected by the
/// scan; harvester targets the next-best clear cell instead.
#[test]
fn scan_skips_tree_blocked_ore_cell() {
    let mut sim = Simulation::new();

    spawn_inert_dock_instance(&mut sim);
    let rules = miner_rules();

    // A tree on the would-be best ore cell at (10, 10). The other ore at
    // (12, 10) is also reachable but farther, so without the tree the scan
    // would pick (10, 10).
    let grid = PathGrid::new(32, 32);
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 5, 10);
    place_ore(&mut sim, 10, 10, 1200);
    place_ore(&mut sim, 12, 10, 1200);
    plant_tree(&mut sim, (10, 10));

    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        entity
            .mission
            .set_handler_state(MinerState::SearchOre.cursor());
    }

    let config = MinerConfig::default();
    super::miner_system::tick_miners(&mut sim, &rules, &config, Some(&grid));

    assert_eq!(
        nav_cell(&sim, miner_id),
        Some((12, 10)),
        "must pass over the tree-blocked (10,10) to the next-best clear ore cell",
    );
}

/// A live TREE01 on `cell`, as map loading plants one: the Terrain object
/// the cell lists (`terrain_object_cells`) and its raw occupation, which the
/// Unit Can_Enter_Cell of Is_Cell_Harvestable reads.
fn plant_tree(sim: &mut Simulation, cell: (u16, u16)) {
    use crate::sim::terrain_object::{
        TerrainObjectLifecycle, TerrainObjectState, mark_terrain_raw_occupation,
    };
    let id = 900;
    let type_ref = sim.interner.intern("TREE01");
    sim.production.terrain_objects.insert(
        id,
        TerrainObjectState {
            stable_id: id,
            native_unique_id: None,
            in_logic_vector: false,
            type_ref,
            rx: cell.0,
            ry: cell.1,
            health: 800,
            max_health: 800,
            occupation_bits: 4,
            lifecycle: TerrainObjectLifecycle::Live,
        },
    );
    sim.production.terrain_object_cells.insert(cell, id);
    mark_terrain_raw_occupation(&mut sim.substrate.raw_cell_occupation, cell, 4);
}

/// An ore cell occupied by another vehicle (e.g. a war miner sitting on
/// it harvesting) is rejected by ring 1+ scan; harvester targets a
/// different cell.
#[test]
fn scan_skips_cell_occupied_by_other_miner() {
    let mut sim = Simulation::new();

    spawn_inert_dock_instance(&mut sim);
    let rules = miner_rules();

    let grid = PathGrid::new(32, 32);

    // Miner A sits on ore at (10, 10). Miner B at (5, 10) is the scanner.
    let _miner_a = spawn_miner(&mut sim, 1, MinerKind::War, 10, 10);
    sim.substrate.occupancy.add(
        10,
        10,
        1,
        MovementLayer::Ground,
        None,
        CellListInsertion::PrependNonBuilding,
    );
    let miner_b = spawn_miner(&mut sim, 2, MinerKind::War, 5, 10);
    sim.substrate.occupancy.add(
        5,
        10,
        2,
        MovementLayer::Ground,
        None,
        CellListInsertion::PrependNonBuilding,
    );

    place_ore(&mut sim, 10, 10, 1200);
    place_ore(&mut sim, 12, 10, 1200);

    {
        let entity = sim.substrate.entities.get_mut(miner_b).expect("miner B");
        entity
            .mission
            .set_handler_state(MinerState::SearchOre.cursor());
    }

    let config = MinerConfig::default();
    super::miner_system::tick_miners(&mut sim, &rules, &config, Some(&grid));

    assert_eq!(
        nav_cell(&sim, miner_b),
        Some((12, 10)),
        "must pass over the cell another miner occupies to the next clear ore cell",
    );
}

/// Ring 0 (harvester's own cell) is unfiltered — a harvester standing on
/// its own ore cell continues to harvest it even though it appears as a
/// blocker in OccupancyGrid.
#[test]
fn scan_ring_0_allows_harvesters_own_cell() {
    use crate::sim::pathfinding::zone_map::ZoneGrid;
    use std::collections::BTreeMap;

    let mut sim = Simulation::new();

    spawn_inert_dock_instance(&mut sim);
    let rules = miner_rules();

    let grid = PathGrid::new(32, 32);
    let zone_grid = ZoneGrid::build(&grid, &BTreeMap::new(), 32, 32);
    sim.zone_grid = Some(zone_grid);

    // Miner on ore at (10, 10). Register itself as occupant — ring 0
    // must still return (10, 10).
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 10, 10);
    sim.substrate.occupancy.add(
        10,
        10,
        1,
        MovementLayer::Ground,
        None,
        CellListInsertion::PrependNonBuilding,
    );
    place_ore(&mut sim, 10, 10, 1200);

    {
        let entity = sim.substrate.entities.get_mut(miner_id).expect("miner");
        entity
            .mission
            .set_handler_state(MinerState::SearchOre.cursor());
    }

    let config = MinerConfig::default();
    super::miner_system::tick_miners(&mut sim, &rules, &config, Some(&grid));

    let m = get_miner(&sim, miner_id);
    assert_eq!(
        (m.state, nav_cell(&sim, miner_id)),
        (MinerState::Harvest, None),
        "the own-cell answer starts cutting where the harvester stands",
    );
}

// ---------------------------------------------------------------------------
// Mission_Harvest state 0 destination guard
// ---------------------------------------------------------------------------

/// Re-anchor the miner's Harvest dispatch timer so the very next dispatch runs.
///
/// A productive scan exits through the Rate epilogue (~14-16 frames), so the
/// frames immediately behind it carry no Harvest dispatch at all. A fixture that
/// means to observe the *next* dispatch has to ask for it rather than assume the
/// following tick brings one: what that dispatch does is under test here, not
/// which frame it lands on. Mirrors the helper of the same name in
/// `outbound_drive_tests` — sibling test modules cannot share it.
fn arm_dispatch_now(sim: &mut Simulation, entity_id: u64) {
    let now = sim.session.binary_frame as i32;
    sim.substrate
        .entities
        .get_mut(entity_id)
        .expect("miner entity")
        .mission
        .write_dispatch_epilogue(now, 0);
}

/// End the outbound drive the way arrival or an abort does: the owner
/// destination and the transitional MovementTarget both go null.
///
/// Both halves matter, and only because these fixtures spawn their miner
/// through `spawn_drive_miner`: a move command writes `navigation.nav_com`
/// only for a Drive or Ship locomotor, so on a locomotor-less miner this
/// would be one real clear and one no-op.
fn clear_outbound_drive(sim: &mut Simulation, entity_id: u64) {
    let entity = sim
        .substrate
        .entities
        .get_mut(entity_id)
        .expect("miner entity");
    entity.navigation.nav_com = None;
    entity.movement_target = None;
}

/// A stock War Miner with the Drive locomotor it actually has in a match.
///
/// The destination guard reads the owner `navigation.nav_com` first and takes
/// `movement_target` only as Rust's transitional second owner. A move command
/// writes nav_com solely for Drive/Ship locomotors, and the shared
/// `spawn_miner` attaches a locomotor only for the Chrono kind — so a bare
/// fixture would hold the guard on the transitional field alone, never
/// exercising the field that owns it once the Drive host migration lands and
/// the transitional half goes away. Mirrors `spawn_search_miner` in
/// `miner_system`'s own test module.
fn spawn_drive_miner(sim: &mut Simulation, sid: u64, rx: u16, ry: u16) -> u64 {
    let miner_id = spawn_miner(sim, sid, MinerKind::War, rx, ry);
    let entity = sim
        .substrate
        .entities
        .get_mut(miner_id)
        .expect("miner entity");
    entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
    entity.drive_locomotion = Some(Default::default());
    miner_id
}

// ==========================================================================
// Slice L5 — Harvest mission handler dispatch
//
// The seam routes the miner FSM through the Harvest dispatcher and its explicit
// resource authority. These tests pin that routing and baseline the
// derived-mission ↔ FSM-cursor invariant for the later substate-authority flip
// (shell S5). The whole existing miner suite already runs through that dispatch,
// so it is the collective bit-identical proof; these add explicit named pins.
// ==========================================================================

/// Across a full dock cycle through the seam, the entity's `derived_mission()`
/// is `(Harvest, miner.state as u8)` every tick — the Task-2 invariant that the
/// shadow MissionCom selector tracks the FSM cursor, which the later
/// substate-authority flip (shell S5) depends on.
#[test]
fn harvest_seam_derived_mission_is_harvest_each_tick() {
    use crate::sim::mission::MissionType;

    let mut sim = Simulation::new();
    let rules = miner_rules();

    spawn_refinery(&mut sim, 100, 10, 10);
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 14, 11);
    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        let miner = entity.miner.as_mut().expect("miner component");
        for _ in 0..10 {
            miner.cargo.push(CargoBale {
                resource_type: ResourceType::Ore,
                value: 25,
            });
        }
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.reserved_refinery = Some(100);
    }

    for _ in 0..400 {
        tick_miners_n(&mut sim, &rules, 1);
        let entity = sim.substrate.entities.get(miner_id).expect("miner entity");
        let state = entity.miner_state().expect("miner cursor");
        assert_eq!(
            entity.derived_mission(),
            (MissionType::Harvest, state as u8),
            "derived mission must be Harvest with the FSM cursor as sub-phase every tick",
        );
    }
}

// ===== Stop must actually stop a miner =====

/// The retail IDLE event handler ends with, for a vehicle carrying the ore-miner
/// type flag whose committed mission is Harvest or Return:
/// `Queue_Mission(Guard, 0); Commence();`. The miner is off the harvest loop the
/// same tick and stays off until it is re-ordered.
///
/// Before this landed, `Command::Stop` committed mission 13 while the harvest
/// dispatch ignored the mission entirely, so the miner stalled for a beat and
/// then drove straight back to the ore field.
#[test]
fn stop_commits_guard_and_takes_a_harvesting_miner_off_the_loop() {
    use crate::sim::command::{Command, CommandEnvelope};
    use crate::sim::mission::{MissionId, MissionType};

    let rules = miner_rules();
    let mut sim = Simulation::new();
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 10, 10);
    // Test entities are inserted straight into the store, which leaves the
    // native in-limbo byte set; the order-admission gate refuses a limboed
    // actor outright.
    sim.substrate
        .entities
        .get_mut(miner_id)
        .expect("miner present")
        .lifecycle
        .in_limbo = false;
    sim.mission_assign_exact(miner_id, MissionId::from_known(MissionType::Harvest), 0)
        .expect("miner exists");
    let owner = sim.interner.get("Americans").expect("owner interned");

    let before = sim
        .substrate
        .entities
        .get(miner_id)
        .expect("miner present")
        .miner_state();

    let stop = CommandEnvelope::new(
        owner,
        1,
        Command::Stop {
            entity_id: miner_id,
        },
    );
    let heights: BTreeMap<(u16, u16), u8> = BTreeMap::new();
    let _ = sim.advance_tick(&[stop], Some(&rules), &heights, None, None, 33);

    let miner = sim.substrate.entities.get(miner_id).expect("miner present");
    assert_eq!(
        miner.mission.current().known(),
        Some(MissionType::Guard),
        "Stop force-assigns Guard to a miner that was on Harvest"
    );
    assert_eq!(miner.mission.queued(), MissionId::NONE);
    assert!(miner.movement_target.is_none());

    // And the harvest handler must decline it from here on, so the FSM cursor
    // stops advancing.
    let after_stop = miner.miner_state();
    for tick in 2..40u64 {
        let _ = sim.advance_tick(&[], Some(&rules), &heights, None, None, tick as u32);
    }
    let miner = sim.substrate.entities.get(miner_id).expect("miner present");
    assert_eq!(
        miner.miner_state(),
        after_stop,
        "the harvest FSM must not advance once Stop has committed Guard \
         (was {before:?} before the order)"
    );
    assert_eq!(miner.mission.current().known(), Some(MissionType::Guard));
}

/// The mission write is the miner arm ONLY. Retail's Stop leaves every other
/// object's committed mission untouched; VERA still commits mission 13 there
/// (a recorded drift), but it must never write Guard.
#[test]
fn stop_does_not_force_guard_on_a_non_miner() {
    use crate::sim::command::{Command, CommandEnvelope};
    use crate::sim::mission::{MissionId, MissionType};

    let rules = miner_rules();
    let mut sim = Simulation::new();
    let owner_id = sim.interner.intern("Americans");
    let type_id = sim.interner.intern("HARV");
    let mut tank = GameEntity::new_at_frame_zero_for_test(
        7,
        10,
        10,
        0,
        0,
        owner_id,
        Health { current: 300 },
        type_id,
        EntityCategory::Unit,
        0,
        5,
        true,
    );
    tank.miner = None;
    tank.lifecycle.in_limbo = false;
    sim.substrate.entities.insert(tank);
    sim.substrate.next_stable_object_id = 8;
    sim.mission_assign_exact(7, MissionId::from_known(MissionType::Move), 0)
        .expect("tank exists");

    let stop = CommandEnvelope::new(owner_id, 1, Command::Stop { entity_id: 7 });
    let heights: BTreeMap<(u16, u16), u8> = BTreeMap::new();
    let _ = sim.advance_tick(&[stop], Some(&rules), &heights, None, None, 33);

    let tank = sim.substrate.entities.get(7).expect("tank present");
    assert_ne!(
        tank.mission.current().known(),
        Some(MissionType::Guard),
        "the Guard force-assign is the ore-miner arm only"
    );
}

// ==========================================================================
// GSI-09.05 refinery selection: Mission_Harvest @ 0x0073E5E0 state 2 →
// Find_Docking_Bay @ 0x004DF040 → FUN_004DEE80 (own-house scan) →
// Receive_Radio(0xF) @ 0x0043C2D0. The narrow pass (free contact slot) is
// used only inside the kind's too-far distance; otherwise the wide pass
// (g_MapEditorMode++) picks the nearest own refinery regardless. A docked
// miner occupies its refinery through Contacts[] (HELLO) — the +0x118
// passenger gate of case 0xF is inert for stock refineries.
// ==========================================================================

/// Fill the miner and park its cursor on ReturnToRefinery with no
/// reservation, so the next dispatch runs refinery selection.
fn fill_and_return(sim: &mut Simulation, miner_id: u64) {
    let entity = sim
        .substrate
        .entities
        .get_mut(miner_id)
        .expect("miner entity");
    let miner = entity.miner.as_mut().expect("miner component");
    for _ in 0..miner.capacity_bales {
        miner.cargo.push(CargoBale {
            resource_type: ResourceType::Ore,
            value: 25,
        });
    }
    miner.reserved_refinery = None;
    entity
        .mission
        .set_handler_state(MinerState::ReturnToRefinery.cursor());
}

/// `Find_Docking_Bay(Type->Dock, 0, wide)` as the miner's state-2 dispatch
/// calls it.
fn docking_bay(sim: &Simulation, rules: &RuleSet, miner_id: u64, wide: bool) -> Option<u64> {
    super::miner_system::find_docking_bay_for_test(sim, rules, miner_id, wide)
}

/// An open 64x64 playfield: the War return's staging search
/// (`Find_Nearby_Passable_Cell`) reads the bounds and the path grid.
fn open_playfield(sim: &mut Simulation) {
    sim.playfield_bounds = Some(crate::map::playfield::PlayfieldBounds {
        base: 0,
        off_fc: -64,
        off_100: -1,
        off_104: 128,
        off_108: 65,
    });
    sim.playfield_size_height = Some(64);
    sim.path_grid = Some(std::sync::Arc::new(PathGrid::new(64, 64)));
}

/// A Drive locomotor: `Assign_Destination` (Unit 0x741970) plans the track
/// only for a ground mover.
fn give_drive(sim: &mut Simulation, miner_id: u64) {
    let entity = sim.substrate.entities.get_mut(miner_id).expect("miner");
    entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
    entity.drive_locomotion = Some(Default::default());
}

/// Put a live occupant into a refinery's `Contacts[]` (HELLO accepted) and
/// optionally onto its pad. The occupant is a plain alive unit entity (no
/// miner component) so `cleanup_dead` keeps the contact and the occupant's
/// own dispatch cannot release it.
fn occupy_refinery(sim: &mut Simulation, refinery_sid: u64, occupant_sid: u64) {
    let owner_id = sim.interner.intern("Americans");
    let type_id = sim.interner.intern("HARV");
    let mut ge = GameEntity::new_at_frame_zero_for_test(
        occupant_sid,
        40,
        40,
        0,
        0,
        owner_id,
        Health { current: 600 },
        type_id,
        EntityCategory::Unit,
        0,
        5,
        true,
    );
    ge.lifecycle.in_limbo = false;
    sim.substrate.entities.insert(ge);
    if sim.substrate.next_stable_object_id <= occupant_sid {
        sim.substrate.next_stable_object_id = occupant_sid + 1;
    }
    assert_eq!(
        crate::sim::miner::miner_dock::hello(sim, occupant_sid, refinery_sid, 1),
        crate::sim::miner::miner_dock::ContactAdmission::Accepted,
    );
}

/// `FUN_004DEE80` walks only the owner house's building list (House+0x6C):
/// a nearer refinery of another house is never a return target.
#[test]
fn refinery_selection_ignores_other_house_refinery() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 5, 10);
    spawn_structure_owned(&mut sim, 2, "GAREFN", "French", 7, 10);
    spawn_refinery(&mut sim, 3, 30, 10);
    fill_and_return(&mut sim, miner_id);

    for wide in [false, true] {
        assert_eq!(
            docking_bay(&sim, &rules, miner_id, wide),
            Some(3),
            "own-house refinery must win over a nearer foreign one (wide={wide})",
        );
    }
}

/// An ALLIED house's nearer refinery is ignored too: the scan reads
/// `Owner+0x6C`, never an ally's list (the old Rust accepted allies).
#[test]
fn refinery_selection_ignores_nearer_allied_refinery() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    sim.house_alliances
        .entry("AMERICANS".to_string())
        .or_default()
        .insert("FRENCH".to_string());
    sim.house_alliances
        .entry("FRENCH".to_string())
        .or_default()
        .insert("AMERICANS".to_string());
    assert!(crate::map::houses::are_houses_friendly(
        &sim.house_alliances,
        "Americans",
        "French"
    ));
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 5, 10);
    spawn_structure_owned(&mut sim, 2, "GAREFN", "French", 7, 10);
    spawn_refinery(&mut sim, 3, 30, 10);
    fill_and_return(&mut sim, miner_id);

    for wide in [false, true] {
        assert_eq!(
            docking_bay(&sim, &rules, miner_id, wide),
            Some(3),
            "own-house refinery must win over a nearer allied one (wide={wide})",
        );
    }
}

/// With only another house's refinery on the map the scan finds nothing:
/// the miner does not convoy to (and pay) the other house.
#[test]
fn refinery_selection_with_only_foreign_refinery_queues_guard() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 5, 10);
    spawn_structure_owned(&mut sim, 2, "GAREFN", "French", 7, 10);
    fill_and_return(&mut sim, miner_id);

    tick_miners_n(&mut sim, &rules, 1);

    // The other house's refinery is not an owned dock instance either: the
    // preamble queues Guard and selection never runs.
    let m = get_miner(&sim, miner_id);
    assert_eq!(m.reserved_refinery, None);
    assert_eq!(m.state, MinerState::ReturnToRefinery);
    assert_eq!(
        sim.substrate
            .entities
            .get(miner_id)
            .expect("miner")
            .mission
            .queued()
            .known(),
        Some(crate::sim::mission::MissionType::Guard)
    );
}

/// The narrow pass (`wide=0`) requires a free contact slot or one already
/// holding the miner; the wide pass admits a full refinery, so it keeps the
/// nearer docked one.
#[test]
fn refinery_selection_narrow_pass_skips_docked_refinery_within_close_radius() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 5, 10);
    spawn_refinery(&mut sim, 2, 6, 10);
    spawn_refinery(&mut sim, 3, 8, 10);
    occupy_refinery(&mut sim, 2, 99);
    fill_and_return(&mut sim, miner_id);

    assert_eq!(
        docking_bay(&sim, &rules, miner_id, false),
        Some(3),
        "docked near refinery must lose to the farther free refinery",
    );
    assert_eq!(docking_bay(&sim, &rules, miner_id, true), Some(2));
}

/// A free refinery beyond `HarvesterTooFarDistance` gets no HELLO: the wide
/// pass takes the nearest refinery (busy or not) and, beyond 0x300 leptons,
/// the miner is staged at its `QueueingCell` (`0x0073ECD0..0x0073ED75`).
#[test]
fn refinery_selection_wide_pass_when_free_refinery_is_beyond_too_far() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 5, 10);
    spawn_refinery(&mut sim, 2, 8, 10);
    spawn_refinery(&mut sim, 3, 20, 10);
    occupy_refinery(&mut sim, 2, 99);
    fill_and_return(&mut sim, miner_id);
    open_playfield(&mut sim);
    give_drive(&mut sim, miner_id);

    tick_miners_n(&mut sim, &rules, 1);

    let entity = sim.substrate.entities.get(miner_id).expect("miner");
    assert!(
        entity.radio_contacts.is_empty(),
        "no HELLO beyond the too-far distance"
    );
    assert_eq!(entity.miner_state(), Some(MinerState::ReturnToRefinery));
    assert_eq!(
        entity.navigation.nav_com,
        Some(crate::sim::components::NavTargetRef::cell(12, 11)),
        "staged at the nearest refinery's NW + QueueingCell (4, 1)",
    );
}

#[test]
fn refinery_selection_wide_pass_falls_back_to_occupied_refinery() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 5, 10);
    spawn_refinery(&mut sim, 2, 10, 10);
    occupy_refinery(&mut sim, 2, 99);
    fill_and_return(&mut sim, miner_id);

    assert_eq!(docking_bay(&sim, &rules, miner_id, false), None);
    assert_eq!(docking_bay(&sim, &rules, miner_id, true), Some(2));
}

/// A refinery whose slot already holds the miner passes the narrow pass's
/// capacity gate.
#[test]
fn refinery_selection_keeps_already_tracked_refinery() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 5, 10);
    spawn_refinery(&mut sim, 2, 6, 10);
    spawn_refinery(&mut sim, 3, 8, 10);
    assert_eq!(
        crate::sim::miner::miner_dock::hello(&mut sim, miner_id, 2, 1),
        crate::sim::miner::miner_dock::ContactAdmission::Accepted,
    );
    fill_and_return(&mut sim, miner_id);

    assert_eq!(docking_bay(&sim, &rules, miner_id, false), Some(2));
}

/// `Can_Reach_Zone` gate of `FUN_004DEE80`: a nearer refinery in a zone the
/// miner cannot reach is skipped in both passes.
#[test]
fn refinery_selection_skips_unreachable_zone_refinery() {
    use crate::sim::pathfinding::zone_map::ZoneGrid;

    let mut sim = Simulation::new();
    let rules = miner_rules();
    // Wall down column x=12 splits the 32x32 map into two zones.
    let mut grid = PathGrid::new(32, 32);
    for y in 0..32 {
        grid.set_blocked(12, y, true);
    }
    sim.zone_grid = Some(ZoneGrid::build(&grid, &BTreeMap::new(), 32, 32));

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 5, 10);
    // Nearer, but across the wall (dock cell (17, 11)).
    spawn_refinery(&mut sim, 2, 14, 10);
    // Farther, same side (dock cell (5, 23)).
    spawn_refinery(&mut sim, 3, 2, 22);
    fill_and_return(&mut sim, miner_id);

    for wide in [false, true] {
        assert_eq!(
            docking_bay(&sim, &rules, miner_id, wide),
            Some(3),
            "unreachable-zone refinery must be skipped for the reachable one (wide={wide})",
        );
    }
}

/// The state-2 too-far test measures to `BuildingClass::GetCoords @
/// 0x00447AC0` = the foundation centre, not the NW cell. Miner at (16, 12);
/// refinery 2 at NW (10, 10) is occupied, refinery 3 at NW (10, 13) is free.
/// Centre distances (cells²): 2 -> 21.25 (dx = 1152, dy = 256), 3 -> 24.25
/// (dx = 1152, dy = -512 -> 1,589,248 leptons²) — both inside 5² = 25, so the
/// narrow pass keeps refinery 3. Measured to the NW cell, refinery 3 would be
/// sqrt(37) away (too far), the wide pass would run and hand the miner the
/// nearer, occupied refinery 2 instead.
#[test]
fn refinery_selection_too_far_test_uses_foundation_centre() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 16, 12);
    spawn_refinery(&mut sim, 2, 10, 10);
    spawn_refinery(&mut sim, 3, 10, 13);
    occupy_refinery(&mut sim, 2, 99);
    fill_and_return(&mut sim, miner_id);

    tick_miners_n(&mut sim, &rules, 1);

    let entity = sim.substrate.entities.get(miner_id).expect("miner");
    assert_eq!(
        entity.radio_contacts.slot(0),
        Some(3),
        "free refinery inside HarvesterTooFarDistance by centre distance gets the HELLO",
    );
    assert_eq!(entity.miner_state(), Some(MinerState::Dock));
}

// ---------------------------------------------------------------------------
// GSI-07.39: refinery-side unload presentation (Mission_Unload state 3 gates)
// and the contact-gone abandonment at unload start.
// ---------------------------------------------------------------------------

/// `miner_rules()` plus a refinery smoke particle system and a long-running
/// SpecialAnim so the overlay is still alive at the next 15-frame gate.
fn miner_rules_with_refinery_art() -> RuleSet {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n\
         [VehicleTypes]\n\
         0=HARV\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         0=GAREFN\n\
         [Particles]\n\
         0=RefSmokeParticle\n\
         [ParticleSystems]\n\
         0=RefSmokeSystem\n\
         [HARV]\n\
         Name=War Miner\n\
         Cost=1400\n\
         Strength=600\n\
         Armor=heavy\n\
         Speed=4\n\
         ROT=5\n\
         Sight=5\n\
         TechLevel=1\n\
         Owner=Americans\n\
         Harvester=yes\n\
         Dock=GAREFN\n\
         [GAREFN]\n\
         Name=Ore Refinery\n\
         Image=GAREFN\n\
         Cost=2000\n\
         Strength=900\n\
         Armor=wood\n\
         TechLevel=1\n\
         Owner=Americans\n\
         Foundation=4x3\n\
         Refinery=yes\nDockUnload=yes\n\
         RefinerySmokeParticleSystem=RefSmokeSystem\n\
         RefinerySmokeOffsetOne=10,-20,30\n\
         [RefSmokeParticle]\n\
         BehavesLike=Smoke\n\
         MaxEC=10\n\
         MaxDC=4\n\
         StartStateAI=0\n\
         EndStateAI=10\n\
         StateAIAdvance=4\n\
         [RefSmokeSystem]\n\
         BehavesLike=Smoke\n\
         HoldsWhat=RefSmokeParticle\n\
         Spawns=yes\n\
         ParticleCap=10\n\
         SpawnFrames=1\n\
         Lifetime=200\n",
    );
    let art_ini = IniFile::from_str(
        "[GAREFN]\n\
         SpecialAnim=GAREFNOR\n\
         [GAREFNOR]\n\
         Start=0\n\
         LoopStart=0\n\
         LoopEnd=200\n\
         Rate=100\n",
    );
    let mut registry_ini = ini.clone();
    registry_ini.merge(&IniFile::from_str("[Animations]\n0=GAREFNOR\n"));
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&registry_ini, &art_ini)
        .expect("miner rules with refinery art");
    let mut art = crate::rules::art_data::ArtRegistry::from_ini(&art_ini);
    art.bind_anim_frame_count_for_test("GAREFNOR", 400);
    rules.merge_art_data(&art);
    rules
}

/// A War Miner with `cargo` docked on refinery 2's pad at (13, 11) (see
/// [`dock_for_unload`]).
fn spawn_docked_miner(sim: &mut Simulation, cargo: &[(ResourceType, u16)]) -> u64 {
    spawn_docked_miner_of(sim, MinerKind::War, cargo)
}

/// A `kind` miner on the pad of a refinery at (10, 10), docked for Unload.
fn spawn_docked_miner_of(
    sim: &mut Simulation,
    kind: MinerKind,
    cargo: &[(ResourceType, u16)],
) -> u64 {
    spawn_refinery(sim, 2, 10, 10);
    let miner_id = spawn_miner(sim, 1, kind, 13, 11);
    load_cargo(sim, miner_id, cargo);
    dock_for_unload(sim, miner_id, 2);
    miner_id
}

fn load_cargo(sim: &mut Simulation, miner_id: u64, cargo: &[(ResourceType, u16)]) {
    let miner = sim
        .substrate
        .entities
        .get_mut(miner_id)
        .and_then(|entity| entity.miner.as_mut())
        .expect("miner component");
    miner
        .cargo
        .extend(cargo.iter().map(|&(resource_type, value)| CargoBale {
            resource_type,
            value,
        }));
}

/// Leave `miner` on its pad as the native docking handshake does before
/// Mission_Unload: contacts and tethers both ways with `refinery`, the hull
/// east (`DOCK_FACING`) and Unload current at its first pass.
fn dock_for_unload(sim: &mut Simulation, miner: u64, refinery: u64) {
    use crate::sim::radio::receive::DOCK_FACING;
    let frame = sim.session.binary_frame;
    for (id, partner) in [(miner, refinery), (refinery, miner)] {
        let entity = sim.substrate.entities.get_mut(id).expect("dock end");
        entity.radio_contacts.set_slot(0, partner);
        entity.dock_entered_with = Some(partner);
    }
    let entity = sim.substrate.entities.get_mut(miner).expect("miner");
    let mut hull = crate::sim::movement::FacingClass::new(DOCK_FACING, 5);
    hull.snap(DOCK_FACING, frame);
    entity.body_facing = Some(hull);
    entity.facing = (DOCK_FACING >> 8) as u8;
    entity.mission.set_handler_state(0);
    sim.mission_assign_exact(
        miner,
        crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Unload),
        frame,
    )
    .expect("Unload assigned");
}

/// One frame of the miner's object-AI visit: its mission dispatch when the
/// timer is due, then the StageClass tick.
fn visit_miner(sim: &mut Simulation, rules: &RuleSet, miner: u64) {
    sim.session.binary_frame = sim.session.binary_frame.wrapping_add(1);
    sim.session.tick += 1;
    sim.object_ai_visit_one(
        miner,
        Some(rules),
        crate::sim::world::ObjectAiCtx::default(),
    );
}

/// Visit the miner frame by frame while Unload is current, at most `frames`
/// times. Returns the frames run (0 when Unload was not current).
fn run_unload(sim: &mut Simulation, rules: &RuleSet, miner: u64, frames: usize) -> usize {
    let unload =
        crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Unload);
    let mut run = 0;
    while run < frames
        && sim
            .substrate
            .entities
            .get(miner)
            .is_some_and(|entity| entity.mission.current() == unload)
    {
        visit_miner(sim, rules, miner);
        run += 1;
    }
    run
}

/// Per-tick observation of the refinery presentation: smoke system count and
/// whether the SpecialAnim slot is live after this tick's frame tail.
struct UnloadPresentationTrace {
    smoke_count: Vec<usize>,
    slot_live: Vec<bool>,
}

fn trace_unload_presentation(
    sim: &mut Simulation,
    rules: &RuleSet,
    miner: u64,
    ticks: usize,
) -> UnloadPresentationTrace {
    let special = sim.interner.intern("GAREFNOR");
    let mut trace = UnloadPresentationTrace {
        smoke_count: Vec::with_capacity(ticks),
        slot_live: Vec::with_capacity(ticks),
    };
    for _ in 0..ticks {
        visit_miner(sim, rules, miner);
        // The authoritative frame tail that consumes the bale events.
        crate::sim::world::building_anim::finalize(sim, &[], true, Some(rules));
        trace.smoke_count.push(sim.particle_systems().len());
        trace.slot_live.push(
            sim.substrate
                .entities
                .get(2)
                .expect("refinery")
                .building_anim_slots[10]
                .and_then(|id| sim.anim(id))
                .is_some_and(|anim| anim.type_id == special),
        );
    }
    trace
}

/// Ticks at which the SpecialAnim slot went absent → live (a `SetAnimSlotImage(10)`).
fn slot_starts(trace: &UnloadPresentationTrace) -> Vec<usize> {
    let mut starts = Vec::new();
    let mut prev = false;
    for (i, live) in trace.slot_live.iter().enumerate() {
        if *live && !prev {
            starts.push(i + 1);
        }
        prev = *live;
    }
    starts
}

/// Ticks at which the smoke count rose (one refinery burst per due gate).
fn smoke_bursts(trace: &UnloadPresentationTrace) -> Vec<usize> {
    let mut bursts = Vec::new();
    let mut prev = 0usize;
    for (i, count) in trace.smoke_count.iter().enumerate() {
        if *count > prev {
            bursts.push(i + 1);
        }
        prev = *count;
    }
    bursts
}

/// Ore-only cargo: two due gates (ore drain, then the empty gate). Native
/// `0x0073E37E` fires the vtable+0x468 smoke burst on both, starts the
/// SpecialAnim once (`+0x584 == NULL`), and `ClearAnimSlot(0xA)` cuts it on the
/// empty gate ~15 frames after it started.
#[test]
fn ore_only_unload_smokes_twice_and_cuts_special_anim_on_empty_gate() {
    let mut sim = Simulation::new();
    let rules = miner_rules_with_refinery_art();
    let miner_id = spawn_docked_miner(&mut sim, &[(ResourceType::Ore, 25); 5]);

    let trace = trace_unload_presentation(&mut sim, &rules, miner_id, 60);

    let bursts = smoke_bursts(&trace);
    assert_eq!(
        bursts.len(),
        2,
        "ore-only: one smoke burst per due gate (ore drain + empty gate), got {bursts:?}"
    );
    assert_eq!(
        *trace.smoke_count.last().expect("trace"),
        2,
        "two RefinerySmokeParticleSystem instances (one offset defined)"
    );
    let starts = slot_starts(&trace);
    assert_eq!(
        starts.len(),
        1,
        "SpecialAnim starts exactly once, got starts at {starts:?}"
    );
    assert_eq!(
        starts[0], bursts[0],
        "SpecialAnim starts on the first (ore) gate"
    );
    let gap = bursts[1] - bursts[0];
    assert!(
        (14..=16).contains(&gap),
        "empty gate follows the drain by one HarvesterDumpRate×900 gate (~15 frames), got {gap}"
    );
    assert!(
        trace.slot_live[bursts[1] - 2],
        "SpecialAnim still live the tick before the empty gate (not played out)"
    );
    assert!(
        !trace.slot_live[bursts[1] - 1],
        "empty gate cuts the SpecialAnim (ClearAnimSlot 0xA @ 0x0073E534)"
    );
    assert!(
        !trace.slot_live.last().expect("trace"),
        "SpecialAnim stays cleared after the unload"
    );
    assert!(get_miner(&sim, miner_id).cargo.is_empty(), "cargo drained");
}

/// Ore + gem cargo: three due gates (ore, gem, empty). Smoke ×3; the
/// SpecialAnim started on the ore gate is still alive on the gem gate, so
/// `+0x584 != NULL` skips the restart (`0x0073E38C`), and the empty gate cuts it.
#[test]
fn ore_and_gem_unload_smokes_thrice_and_starts_special_anim_once() {
    let mut sim = Simulation::new();
    let rules = miner_rules_with_refinery_art();
    let mut cargo = vec![(ResourceType::Ore, 25u16); 5];
    cargo.extend([(ResourceType::Gem, 50u16); 3]);
    let miner_id = spawn_docked_miner(&mut sim, &cargo);

    let trace = trace_unload_presentation(&mut sim, &rules, miner_id, 80);

    let bursts = smoke_bursts(&trace);
    assert_eq!(
        bursts.len(),
        3,
        "ore+gem: one smoke burst per due gate (ore, gem, empty), got {bursts:?}"
    );
    assert_eq!(*trace.smoke_count.last().expect("trace"), 3);
    for pair in bursts.windows(2) {
        let gap = pair[1] - pair[0];
        assert!(
            (14..=16).contains(&gap),
            "gates are one HarvesterDumpRate×900 apart (~15 frames), got {gap} in {bursts:?}"
        );
    }
    let starts = slot_starts(&trace);
    assert_eq!(
        starts,
        vec![bursts[0]],
        "SpecialAnim starts on the ore gate only; the gem gate finds +0x584 live and does not restart"
    );
    assert!(
        trace.slot_live[bursts[1] - 1],
        "SpecialAnim is live through the gem gate"
    );
    assert!(
        !trace.slot_live[bursts[2] - 1],
        "empty gate cuts the SpecialAnim"
    );
    assert!(get_miner(&sim, miner_id).cargo.is_empty());
}

/// Retail GAREFN/NAREFN define `SpecialAnim` but no `SpecialAnimDamaged`.
/// `SetAnimSlotImage(10, damaged=1, …) @ 0x00451750` reads only the slot-local
/// damaged name (`+0xF5C`) and creates nothing when it is empty, so a refinery
/// at/below ConditionYellow (`GetHealthRatio <= Rules+0x1700`, `0x0073E39B`)
/// unloads with the smoke bursts only.
#[test]
fn damaged_refinery_ore_only_unload_smokes_twice_without_special_anim() {
    let mut sim = Simulation::new();
    let rules = miner_rules_with_refinery_art();
    let miner_id = spawn_docked_miner(&mut sim, &[(ResourceType::Ore, 25); 5]);
    {
        let refinery = sim.substrate.entities.get_mut(2).expect("refinery");
        refinery.health.current = rules.object("GAREFN").expect("refinery type").strength / 2;
    }

    let trace = trace_unload_presentation(&mut sim, &rules, miner_id, 60);

    let bursts = smoke_bursts(&trace);
    assert_eq!(
        bursts.len(),
        2,
        "damaged refinery still smokes on both due gates, got {bursts:?}"
    );
    assert_eq!(*trace.smoke_count.last().expect("trace"), 2);
    assert!(
        slot_starts(&trace).is_empty(),
        "no SpecialAnimDamaged defined: slot 10 never starts"
    );
    assert!(
        trace.slot_live.iter().all(|live| !live),
        "base SpecialAnim is never used as a fallback"
    );
    assert!(get_miner(&sim, miner_id).cargo.is_empty(), "cargo drained");
}

/// A refinery killed under an unloading miner: `ObjectClass::ReceiveDamage`'s
/// exact-zero Destroy (`0x005F57AF`, Detach_All(1)) drops the radio slot at
/// the killing hit, and the Building NowDead contact loop (`0x00442511`, the
/// pre-hit contact copy, 0x100 leptons or more from the centre) sends the
/// miner RUN_AWAY: the unload latch drops, it scatters off the pad and Harvest
/// takes over (`0x00737A98`) — cargo and credits untouched. Native4424A2
/// gates Force release4593A0 on reciprocal bunker+2E4, which refinery contacts
/// do not satisfy.
#[test]
fn refinery_death_drops_the_unloading_miner_at_the_kill() {
    use crate::sim::combat::EntityDamageEvent;
    use crate::sim::house_state::HouseState;

    let ini = IniFile::from_str(
        "[InfantryTypes]\n\
         [VehicleTypes]\n0=HARV\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n0=GAREFN\n\
         [Warheads]\n0=KILLWH\n\
         [HARV]\n\
         Name=War Miner\nCost=1400\nStrength=600\nArmor=heavy\nSpeed=4\nROT=5\nSight=5\n\
         TechLevel=1\nOwner=Americans\nHarvester=yes\nDock=GAREFN\n\
         [GAREFN]\n\
         Name=Ore Refinery\nCost=2000\nStrength=900\nArmor=wood\nTechLevel=1\n\
         Owner=Americans\nFoundation=4x3\nRefinery=yes\nDockUnload=yes\n\
         [KILLWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("refinery kill rules");

    let mut sim = Simulation::new();
    let owner = sim.interner.intern("Americans");
    sim.houses
        .insert(owner, HouseState::new(owner, 0, None, false, 0, 10));
    sim.session.house_order = vec![owner];
    sim.session.binary_frame = 40;

    let miner_id = spawn_docked_miner(&mut sim, &[(ResourceType::Ore, 25); 4]);
    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
        entity.drive_locomotion = Some(Default::default());
        entity.foot_speed.applied_fraction = crate::util::fixed_math::SimFixed::lit("0.25");
        entity.foot_speed.cached_current_speed = 7;
    }
    // The first pass raises the unload latch; the dump gate is 15 frames out.
    run_unload(&mut sim, &rules, miner_id, 1);
    assert!(get_miner(&sim, miner_id).unload_active);
    let credits_before = credits_for_owner(&sim, "Americans");
    let (position_before, drive_before, cargo_before) = {
        let entity = sim.substrate.entities.get(miner_id).unwrap();
        (
            crate::sim::movement::ground_pose::position_world_coord(&entity.position),
            entity.drive_locomotion.clone(),
            entity.miner.as_ref().unwrap().cargo.clone(),
        )
    };

    // Kill the refinery through the damage transaction (not a sale).
    let warhead = sim.interner.intern("KILLWH");
    let event = EntityDamageEvent::area(2, 5000, 0, miner_id, Some(owner), warhead);
    sim.commit_noncombat_aoe_hits(&rules, None, &[event]);

    assert!(
        sim.substrate
            .entities
            .get(2)
            .is_none_or(|refinery| refinery.dying || refinery.health.current == 0),
        "the refinery died"
    );
    let miner_entity = sim
        .substrate
        .entities
        .get(miner_id)
        .expect("miner survives");
    let miner = miner_entity.miner.as_ref().expect("miner component");
    assert!(
        miner_entity.radio_contacts.is_empty(),
        "contact released at the kill"
    );
    assert!(!crate::sim::miner::miner_dock::has_contact(
        &sim, 2, miner_id
    ));
    assert!(
        !has_bunker_release_track(miner_entity),
        "refinery loss does not authorize bunker Force_Track(0x47)"
    );
    assert_eq!(
        crate::sim::movement::ground_pose::position_world_coord(&miner_entity.position),
        position_before
    );
    assert_ne!(
        miner_entity.drive_locomotion, drive_before,
        "RUN_AWAY scattered the miner off the pad"
    );
    assert_eq!(miner.cargo, cargo_before);
    assert!(!miner.unload_active, "RUN_AWAY dropped the latch");
    assert_eq!(miner_entity.display_type_override, None);
    assert!(
        miner_entity.mission.current()
            == crate::sim::mission::MissionId::from_known(
                crate::sim::mission::MissionType::Harvest
            )
            || miner_entity.mission.queued()
                == crate::sim::mission::MissionId::from_known(
                    crate::sim::mission::MissionType::Harvest
                ),
        "Harvest queued for the miner"
    );
    assert_eq!(
        credits_for_owner(&sim, "Americans"),
        credits_before,
        "the cargo on the pad is not deposited"
    );

    // Nothing pays the bales later either: no refinery is left to dock at.
    for _ in 0..60 {
        visit_miner(&mut sim, &rules, miner_id);
    }
    assert_eq!(credits_for_owner(&sim, "Americans"), credits_before);
    assert_eq!(get_miner(&sim, miner_id).cargo.len(), 4);
}
/// One object-AI visit of `id` with the miner config wired (the Harvest
/// dispatch needs it), rules present.
fn visit_object_ai(sim: &mut Simulation, rules: &RuleSet, id: u64) {
    let config = MinerConfig::default();
    sim.session.binary_frame = sim.session.binary_frame.wrapping_add(1);
    sim.object_ai_visit_one(
        id,
        Some(rules),
        crate::sim::world::ObjectAiCtx {
            miner_config: Some(&config),
            ..crate::sim::world::ObjectAiCtx::default()
        },
    );
}

/// `FootClass::Mission_Move @ 0x004D4242` → `UnitClass::Enter_Idle_Mode @
/// 0x00738970` harvester arm: a war miner whose player Move has finished
/// (Move committed, NavCom clear, nothing queued) is put back on Harvest by
/// its own arrival dispatch when it stopped on ore (`LandType == Tiberium`),
/// and the promoted Harvest restarts the handler from state 0 (`+0xBC = 0`).
#[test]
fn player_move_arrival_returns_a_war_miner_to_harvest_on_ore() {
    use crate::sim::mission::{MissionId, MissionType};

    let mut sim = Simulation::new();
    let rules = miner_rules();
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 20, 20);
    place_ore(&mut sim, 20, 20, 5);
    install_land_types_for_placed_ore(&mut sim);
    // Mid-harvest cursor, then the player Move takes over (Command::Move's
    // `queue_megamission_with_teardown(Move)` promoted).
    let now = sim.session.binary_frame;
    sim.mission_assign_exact(miner_id, MissionId::from_known(MissionType::Harvest), now)
        .expect("assign Harvest");
    sim.substrate
        .entities
        .get_mut(miner_id)
        .unwrap()
        .mission
        .set_handler_state(MinerState::Harvest.cursor());
    sim.mission_assign_exact(miner_id, MissionId::from_known(MissionType::Move), now)
        .expect("assign Move");
    let cursor_on_move = sim
        .substrate
        .entities
        .get(miner_id)
        .unwrap()
        .mission
        .handler_state();

    // Arrival dispatch: the harvester arm queues Harvest (no RNG draw).
    let rng_before = sim.scenario_rng.state();
    visit_object_ai(&mut sim, &rules, miner_id);
    let e = sim.substrate.entities.get(miner_id).unwrap();
    assert_eq!(e.mission.current().known(), Some(MissionType::Move));
    assert_eq!(
        e.mission.queued().known(),
        Some(MissionType::Harvest),
        "Enter_Idle_Mode harvester arm commits Harvest on ore"
    );
    assert_eq!(
        e.mission.handler_state(),
        cursor_on_move,
        "Harvest FSM declined the Move dispatch"
    );
    assert_eq!(sim.scenario_rng.state(), rng_before, "the arm draws no RNG");

    // Next visit: Ready-to-Commence promotes Harvest with a zeroed cursor and
    // the Harvest handler runs again from state 0.
    visit_object_ai(&mut sim, &rules, miner_id);
    let e = sim.substrate.entities.get(miner_id).unwrap();
    assert_eq!(e.mission.current().known(), Some(MissionType::Harvest));
    assert!(
        MinerState::from_cursor(e.mission.handler_state()).is_some(),
        "the Harvest handler dispatched from state 0"
    );
}

/// The same arm for a HUMAN house whose miner stopped on non-ore land
/// (`CellClass+0xEC != 5`) selects Guard (5), not Harvest — and an AI house
/// on the same cell selects Harvest.
#[test]
fn player_move_arrival_off_ore_parks_a_human_miner_on_guard_and_an_ai_miner_on_harvest() {
    use crate::sim::house_state::HouseState;
    use crate::sim::mission::{MissionId, MissionType};

    for (is_human, expected) in [(true, MissionType::Guard), (false, MissionType::Harvest)] {
        let mut sim = Simulation::new();
        let rules = miner_rules();
        let owner = sim.interner.intern("Americans");
        sim.houses
            .insert(owner, HouseState::new(owner, 0, None, is_human, 0, 10));
        let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 20, 20);
        // Ore elsewhere, none under the miner.
        place_ore(&mut sim, 30, 30, 5);
        let now = sim.session.binary_frame;
        sim.mission_assign_exact(miner_id, MissionId::from_known(MissionType::Move), now)
            .expect("assign Move");
        visit_object_ai(&mut sim, &rules, miner_id);
        let e = sim.substrate.entities.get(miner_id).unwrap();
        assert_eq!(
            e.mission.queued().known(),
            Some(expected),
            "human={is_human}: selector on non-ore land"
        );
    }
}

/// A 64x64 sim carrying the production `CellClass+0xEC` authority: a flat
/// clear `ResolvedTerrainGrid` plus an `OverlayGrid` with one TIB01 patch on
/// `cell`, folded into `land_type` by the same `recalc_overlay_passability`
/// the map loader and every overlay mutation run.
fn sim_with_resolved_tiberium_cell(
    registry: &crate::map::overlay_types::OverlayTypeRegistry,
    cell: (u16, u16),
) -> Simulation {
    use crate::map::resolved_terrain::ResolvedTerrainGrid;
    use crate::rules::terrain_rules::LandType;
    use crate::sim::house_state::HouseState;

    let tib01 = registry.id_for_name("TIB01").expect("TIB01");
    let mut sim = Simulation::new();
    let owner = sim.interner.intern("Americans");
    sim.houses
        .insert(owner, HouseState::new(owner, 0, None, true, 0, 10));
    let mut cells = Vec::with_capacity(64 * 64);
    for ry in 0..64u16 {
        for rx in 0..64u16 {
            cells.push(crate::sim::deploy_tests::clear_terrain_cell(rx, ry));
        }
    }
    let mut terrain = ResolvedTerrainGrid::from_cells(64, 64, cells);
    let mut overlay = OverlayGrid::new(64, 64);
    overlay.place_overlay(cell.0, cell.1, tib01, 3);
    assert!(crate::sim::overlay_grid::recalc_overlay_passability(
        &mut overlay,
        &mut terrain,
        registry,
        cell.0,
        cell.1,
    ));
    assert_eq!(
        terrain.cell(cell.0, cell.1).unwrap().land_type,
        LandType::Tiberium.as_index(),
        "overlay recalc wrote LandType Tiberium (5)"
    );
    sim.resolved_terrain = Some(terrain);
    sim.overlay_grid = Some(overlay);
    sim
}

/// Give a fixture that seeded ore through [`place_ore`] the `CellClass+0xEC`
/// authority the idle-mode harvester arm reads: a flat clear
/// `ResolvedTerrainGrid` with every tiberium overlay folded into `land_type`
/// by `recalc_overlay_passability`, as the map loader does.
fn install_land_types_for_placed_ore(sim: &mut Simulation) {
    use crate::map::resolved_terrain::ResolvedTerrainGrid;

    let size = crate::sim::tiberium::test_support::TEST_GRID_SIZE;
    let mut cells = Vec::with_capacity(usize::from(size) * usize::from(size));
    for ry in 0..size {
        for rx in 0..size {
            cells.push(crate::sim::deploy_tests::clear_terrain_cell(rx, ry));
        }
    }
    let mut terrain = ResolvedTerrainGrid::from_cells(size, size, cells);
    let overlay = sim.overlay_grid.as_mut().expect("place_ore ran first");
    let ore_cells: Vec<(u16, u16)> = overlay
        .iter_occupied()
        .map(|(rx, ry, _)| (rx, ry))
        .collect();
    for (rx, ry) in ore_cells {
        crate::sim::overlay_grid::recalc_overlay_passability(
            overlay,
            &mut terrain,
            crate::sim::tiberium::test_support::overlay_registry(),
            rx,
            ry,
        );
    }
    sim.resolved_terrain = Some(terrain);
}

/// Assign Move (no destination) and run one arrival dispatch; returns the
/// queued selector.
fn move_arrival_selector(
    sim: &mut Simulation,
    rules: &RuleSet,
    miner_id: u64,
) -> Option<crate::sim::mission::MissionType> {
    use crate::sim::mission::{MissionId, MissionType};

    let now = sim.session.binary_frame;
    sim.mission_assign_exact(miner_id, MissionId::from_known(MissionType::Move), now)
        .expect("assign Move");
    visit_object_ai(sim, rules, miner_id);
    sim.substrate
        .entities
        .get(miner_id)
        .unwrap()
        .mission
        .queued()
        .known()
}

/// The arm reads `land_type` from the resolved terrain, and a human war miner
/// arriving on a TIB01 overlay cell resumes Harvest.
#[test]
fn player_move_arrival_reads_tiberium_land_type_from_resolved_terrain() {
    use crate::sim::mission::MissionType;

    let (rules, registry) = miner_rules_with_tiberium();
    let cell = (20u16, 20u16);
    let mut sim = sim_with_resolved_tiberium_cell(&registry, cell);
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, cell.0, cell.1);
    assert_eq!(
        move_arrival_selector(&mut sim, &rules, miner_id),
        Some(MissionType::Harvest),
        "resolved terrain LandType 5 selects Harvest"
    );
}

/// The same cell after the production reducer fully removes its overlay:
/// `reduce_tiberium`'s full-removal boundary runs `recalc_overlay_passability`
/// synchronously, `land_type` drops back to Clear, and the human miner's next
/// arrival parks on Guard.
#[test]
fn player_move_arrival_after_full_harvest_parks_a_human_miner_on_guard() {
    use crate::rules::terrain_rules::LandType;
    use crate::sim::mission::MissionType;
    use crate::sim::tiberium::{ReduceTiberiumContext, reduce_tiberium};

    let (rules, registry) = miner_rules_with_tiberium();
    let cell = (20u16, 20u16);
    let mut sim = sim_with_resolved_tiberium_cell(&registry, cell);
    sim.production
        .ore_growth_state
        .reset_native_tiberium_classes(rules.tiberium_types.len(), 0);
    let outcome = {
        let mut ctx = ReduceTiberiumContext {
            overlay_grid: sim.overlay_grid.as_mut(),
            ore_growth_state: &mut sim.production.ore_growth_state,
            overlay_registry: Some(&registry),
            tiberium_types: Some(&rules.tiberium_types),
            resolved_terrain: sim.resolved_terrain.as_mut(),
            source_object_cells: None,
            live_objects: None,
            rng: None,
            binary_frame: 0,
            spread_enabled: false,
            radar_dirty_cells: None,
            radar_dirty_generation: None,
            tactical_dirty_cells: None,
        };
        reduce_tiberium(&mut ctx, cell, 4)
    };
    assert!(
        outcome.fully_removed,
        "density 3 against 4 is a full removal"
    );
    assert_eq!(
        sim.overlay_grid
            .as_ref()
            .unwrap()
            .cell(cell.0, cell.1)
            .overlay_id,
        None
    );
    assert_eq!(
        sim.resolved_terrain
            .as_ref()
            .unwrap()
            .cell(cell.0, cell.1)
            .unwrap()
            .land_type,
        LandType::Clear.as_index(),
        "full removal recalc restored the base land type"
    );

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, cell.0, cell.1);
    assert_eq!(
        move_arrival_selector(&mut sim, &rules, miner_id),
        Some(MissionType::Guard),
        "LandType no longer 5 ⇒ a human miner parks on Guard"
    );
}

/// `In_Radio_Contact` ⇒ the arm returns without assigning anything: a miner
/// still linked to a refinery keeps Move and no mission is queued.
#[test]
fn player_move_arrival_in_radio_contact_assigns_nothing() {
    use crate::sim::mission::{MissionId, MissionType};

    let mut sim = Simulation::new();
    let rules = miner_rules();
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 20, 20);
    spawn_refinery(&mut sim, 2, 10, 10);
    place_ore(&mut sim, 20, 20, 5);
    install_land_types_for_placed_ore(&mut sim);
    sim.substrate
        .entities
        .get_mut(miner_id)
        .unwrap()
        .radio_contacts
        .insert(2);
    let now = sim.session.binary_frame;
    sim.mission_assign_exact(miner_id, MissionId::from_known(MissionType::Move), now)
        .expect("assign Move");
    visit_object_ai(&mut sim, &rules, miner_id);
    let e = sim.substrate.entities.get(miner_id).unwrap();
    assert_eq!(e.mission.current().known(), Some(MissionType::Move));
    assert_eq!(e.mission.queued(), MissionId::NONE);
}

/// Registers `Americans` (the `spawn_miner` owner) and `YuriCountry` (the
/// captor) with explicit `is_human` so the selector's
/// `houses.get(owner)` gate reads a real house, not the absent-house
/// fallback.
fn register_capture_houses(
    sim: &mut Simulation,
    old_is_human: bool,
    new_is_human: bool,
) -> crate::sim::intern::InternedId {
    use crate::sim::house_state::HouseState;
    let old = sim.interner.intern("Americans");
    sim.houses
        .insert(old, HouseState::new(old, 0, None, old_is_human, 0, 10));
    let captor = sim.interner.intern("YuriCountry");
    sim.houses.insert(
        captor,
        HouseState::new(captor, 1, None, new_is_human, 0, 10),
    );
    captor
}

/// `TechnoClass::ChangeOwner @ 0x007014A0` on a harvesting miner standing on
/// ore: the forced `Queue_Mission(Guard, commence_now = 1)` commits Guard on
/// the spot, then the `Enter_Idle_Mode(0, 1)` call at 0x00701849 takes the
/// harvester arm of `UnitClass::Enter_Idle_Mode @ 0x00738970` and re-queues
/// Harvest for the NEW owner (the arm reads the owner after the `+0x21C`
/// swap). Target and destination are cleared on the way. Both houses are
/// human here: on ore the land check passes for either kind of owner.
#[test]
fn captured_harvesting_miner_requeues_harvest_for_the_new_owner() {
    use crate::sim::mission::{MissionId, MissionType};

    let rules = miner_rules();
    let mut sim = Simulation::new();
    let captor = register_capture_houses(&mut sim, true, true);
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 10, 10);
    place_ore(&mut sim, 10, 10, 100);
    install_land_types_for_placed_ore(&mut sim);
    sim.mission_assign_exact(miner_id, MissionId::from_known(MissionType::Harvest), 0)
        .expect("miner exists");
    assert_eq!(
        sim.substrate
            .entities
            .get(miner_id)
            .unwrap()
            .mission
            .current()
            .known(),
        Some(MissionType::Harvest)
    );

    sim.change_owner_with_rules(miner_id, captor, &rules);

    let miner = sim.substrate.entities.get(miner_id).expect("miner present");
    assert_eq!(miner.owner, captor);
    assert_eq!(
        miner.mission.current().known(),
        Some(MissionType::Guard),
        "ChangeOwner force-queues Guard and commences it on a ready unit"
    );
    assert_eq!(
        miner.mission.queued(),
        MissionId::from_known(MissionType::Harvest),
        "the idle-mode harvester arm re-queues Harvest on ore"
    );
    assert!(miner.attack_target.is_none());
    assert!(miner.navigation.nav_com.is_none());
    assert!(miner.movement_target.is_none());
}

/// Same owner change, miner standing off ore, captured by a HUMAN house
/// (the old owner is AI, so the outcome can only come from the NEW owner's
/// `IsControlledByHuman`): the arm's `IsControlledByHuman && LandType != 5`
/// branch selects Guard, which is already current, so nothing is queued —
/// the captured miner parks.
#[test]
fn captured_miner_off_ore_under_a_human_house_parks_on_guard() {
    use crate::sim::mission::{MissionId, MissionType};

    let rules = miner_rules();
    let mut sim = Simulation::new();
    let captor = register_capture_houses(&mut sim, false, true);
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 10, 10);
    place_ore(&mut sim, 20, 20, 100);
    sim.mission_assign_exact(miner_id, MissionId::from_known(MissionType::Harvest), 0)
        .expect("miner exists");

    sim.change_owner_with_rules(miner_id, captor, &rules);

    let miner = sim.substrate.entities.get(miner_id).expect("miner present");
    assert_eq!(miner.owner, captor);
    assert_eq!(miner.mission.current().known(), Some(MissionType::Guard));
    assert_eq!(miner.mission.queued(), MissionId::NONE);
}

/// The mirror case: a HUMAN player's miner off ore captured by an AI house.
/// The Guard branch at 0x00738970 requires the NEW owner to pass
/// `IsControlledByHuman` (the arm runs after the `+0x21C` swap); an AI
/// captor always takes Harvest, so the miner re-queues Harvest even though
/// the human it was taken from would have parked it.
#[test]
fn captured_miner_off_ore_under_an_ai_house_requeues_harvest() {
    use crate::sim::mission::{MissionId, MissionType};

    let rules = miner_rules();
    let mut sim = Simulation::new();
    let captor = register_capture_houses(&mut sim, true, false);
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 10, 10);
    place_ore(&mut sim, 20, 20, 100);
    sim.mission_assign_exact(miner_id, MissionId::from_known(MissionType::Harvest), 0)
        .expect("miner exists");

    sim.change_owner_with_rules(miner_id, captor, &rules);

    let miner = sim.substrate.entities.get(miner_id).expect("miner present");
    assert_eq!(miner.owner, captor);
    assert_eq!(miner.mission.current().known(), Some(MissionType::Guard));
    assert_eq!(
        miner.mission.queued(),
        MissionId::from_known(MissionType::Harvest),
        "an AI captor skips the human land check and re-queues Harvest off ore"
    );
}

/// A miner in radio contact (docked at its refinery) when captured by a
/// human house: the harvester arm's `In_Radio_Contact` early return assigns
/// nothing beyond the forced Guard.
#[test]
fn captured_miner_in_radio_contact_gets_only_the_forced_guard() {
    use crate::sim::mission::{MissionId, MissionType};

    let rules = miner_rules();
    let mut sim = Simulation::new();
    let captor = register_capture_houses(&mut sim, true, true);
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 10, 10);
    spawn_refinery(&mut sim, 2, 12, 12);
    place_ore(&mut sim, 10, 10, 100);
    install_land_types_for_placed_ore(&mut sim);
    sim.mission_assign_exact(miner_id, MissionId::from_known(MissionType::Harvest), 0)
        .expect("miner exists");
    sim.substrate
        .entities
        .get_mut(miner_id)
        .unwrap()
        .mark_live_contact_with(2);

    sim.change_owner_with_rules(miner_id, captor, &rules);

    let miner = sim.substrate.entities.get(miner_id).expect("miner present");
    assert_eq!(miner.owner, captor);
    assert_eq!(miner.mission.current().known(), Some(MissionType::Guard));
    assert_eq!(miner.mission.queued(), MissionId::NONE);
}
