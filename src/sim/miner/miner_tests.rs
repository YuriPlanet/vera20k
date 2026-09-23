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
use crate::sim::miner::{
    CargoBale, Miner, MinerConfig, MinerKind, MinerState, RefineryDockPhase, ResourceType,
};
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
         Refinery=yes\n\
         FreeUnit=CMIN\n",
        crate::sim::tiberium::test_support::tiberium_rules_text(),
    ));
    RuleSet::from_ini(&ini).expect("miner rules")
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
         Refinery=yes\n\
         [OTHERPROC]\n\
         Name=Other Refinery\n\
         Foundation=4x3\n\
         Refinery=yes\n",
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
    if kind == MinerKind::Chrono {
        ge.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Teleport));
    }
    ge.miner = Some(Miner::new(kind, &MinerConfig::default(), 0));
    ge.lifecycle.in_limbo = false;
    sim.substrate.entities.insert(ge);
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
    ge.lifecycle.in_limbo = false;
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
fn place_ore(sim: &mut Simulation, rx: u16, ry: u16, amount: u16) {
    crate::sim::tiberium::test_support::place_stock_amount(
        sim,
        (rx, ry),
        ResourceType::Ore,
        amount,
    );
}

/// Tick the miner system `n` times.
///
/// Matches advance_tick ordering: teleport (Phase 2) → miners (Phase 7) →
/// ground movement. Teleport must run before miners so that Relocate/ChronoDelay
/// updates are visible to the miner snapshot.
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

    // Miner at dock cell, refinery at (10, 10) with 4x3 foundation.
    // Dock cell = (rx + width, ry + height/2) = (10 + 4, 10 + 1) = (14, 11) — east platform.
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 14, 11);
    spawn_refinery(&mut sim, 2, 10, 10);

    // Pre-load cargo: 40 ore bales.
    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        let miner = entity.miner.as_mut().expect("miner component");
        for _ in 0..40 {
            miner.cargo.push(CargoBale {
                resource_type: ResourceType::Ore,
                value: 25,
            });
        }
        // Put miner in Dock state so it proceeds to Unload.
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Unloading;
        miner.reserved_refinery = Some(2);
    }
    // The unload mission's In_Radio_Contact gate needs the admitted contact
    // a real approach leaves behind.
    assert!(crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, miner_id));

    let before = credits_for_owner(&sim, "Americans");
    // Tick enough times to fully unload: 40 bales * unload_interval=57 = 2280 ticks.
    tick_miners_n(&mut sim, &rules, 2400);

    let after = credits_for_owner(&sim, "Americans");
    assert_eq!(after - before, 1000, "War Miner full ore = 1000 credits");
}

// ==========================================================================
// Test 2: War Miner full gem load = 2000 credits
// ==========================================================================
#[test]
fn war_miner_full_gem_payout_is_2000() {
    let mut sim = Simulation::new();
    let rules = miner_rules();

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 14, 11);
    spawn_refinery(&mut sim, 2, 10, 10);

    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        let miner = entity.miner.as_mut().expect("miner component");
        for _ in 0..40 {
            miner.cargo.push(CargoBale {
                resource_type: ResourceType::Gem,
                value: 50,
            });
        }
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Unloading;
        miner.reserved_refinery = Some(2);
    }
    // The unload mission's In_Radio_Contact gate needs the admitted contact
    // a real approach leaves behind.
    assert!(crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, miner_id));

    let before = credits_for_owner(&sim, "Americans");
    tick_miners_n(&mut sim, &rules, 2400);
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

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::Chrono, 14, 11);
    spawn_refinery(&mut sim, 2, 10, 10);

    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        let miner = entity.miner.as_mut().expect("miner component");
        for _ in 0..20 {
            miner.cargo.push(CargoBale {
                resource_type: ResourceType::Ore,
                value: 25,
            });
        }
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Unloading;
        miner.reserved_refinery = Some(2);
    }
    // The unload mission's In_Radio_Contact gate needs the admitted contact
    // a real approach leaves behind.
    assert!(crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, miner_id));

    let before = credits_for_owner(&sim, "Americans");
    // 20 bales * unload_interval=57 = 1140 ticks.
    tick_miners_n(&mut sim, &rules, 1200);
    let after = credits_for_owner(&sim, "Americans");
    assert_eq!(after - before, 500, "Chrono Miner full ore = 500 credits");
}

// ==========================================================================
// Test 4: Chrono Miner full gem load = 1000 credits
// ==========================================================================
#[test]
fn chrono_miner_full_gem_payout_is_1000() {
    let mut sim = Simulation::new();
    let rules = miner_rules();

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::Chrono, 14, 11);
    spawn_refinery(&mut sim, 2, 10, 10);

    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        let miner = entity.miner.as_mut().expect("miner component");
        for _ in 0..20 {
            miner.cargo.push(CargoBale {
                resource_type: ResourceType::Gem,
                value: 50,
            });
        }
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Unloading;
        miner.reserved_refinery = Some(2);
    }
    // The unload mission's In_Radio_Contact gate needs the admitted contact
    // a real approach leaves behind.
    assert!(crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, miner_id));

    let before = credits_for_owner(&sim, "Americans");
    tick_miners_n(&mut sim, &rules, 1200);
    let after = credits_for_owner(&sim, "Americans");
    assert_eq!(
        after - before,
        1000,
        "Chrono Miner full gems = 1000 credits"
    );
}

// ==========================================================================
// Test 5: Chrono Miner teleports on far return (position snaps to QueueingCell)
// ==========================================================================
#[test]
fn chrono_miner_teleports_to_refinery_on_return() {
    let mut sim = Simulation::new();
    let rules = miner_rules();

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::Chrono, 80, 80);
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

    let entity = sim.substrate.entities.get(miner_id).expect("entity");
    assert!(
        entity.teleport_state.is_some(),
        "Chrono Miner should have an active teleport after first return tick"
    );
    let teleport = entity.teleport_state.as_ref().expect("teleport state");
    assert_eq!(
        (teleport.target_rx, teleport.target_ry),
        (14, 11),
        "Far return should stage at QueueingCell, not the refinery pad"
    );
    assert_eq!(
        entity.miner.as_ref().and_then(|m| m.reserved_refinery),
        Some(2),
        "Return target should be selected before docking contact"
    );
    let loco = entity.locomotor.as_ref().expect("locomotor");
    assert_eq!(loco.active_kind(), LocomotorKind::Teleport);
    assert_eq!(loco.effective_kind(), LocomotorKind::Teleport);
    assert!(loco.piggyback.is_none());
    assert!(!loco.is_overridden());

    crate::sim::movement::teleport_movement::tick_teleport_movement(
        &mut sim.substrate.entities,
        &mut OccupancyGrid::new(),
        &[],
        sim.session.tick,
        None,
        None,
    );

    let entity = sim.substrate.entities.get(miner_id).expect("entity");
    assert_eq!(
        (entity.position.rx, entity.position.ry),
        (14, 11),
        "Position should snap to the QueueingCell staging cell after Relocate"
    );
    assert!(
        entity.teleport_state.is_none(),
        "Harvester teleport cleanup should clear TeleportState in the relocate tick"
    );
    let loco = entity.locomotor.as_ref().expect("locomotor");
    assert_eq!(loco.active_kind(), LocomotorKind::Teleport);
    assert_eq!(loco.effective_kind(), LocomotorKind::Teleport);
    assert!(loco.piggyback.is_none());
    assert!(!loco.is_overridden());
}

#[test]
fn chrono_far_return_uses_passable_search_from_queueing_cell() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let config = MinerConfig::default();
    let mut grid = PathGrid::new(64, 64);
    grid.set_blocked(14, 11, true);

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::Chrono, 80, 80);
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

    super::miner_system::tick_miners(&mut sim, &rules, &config, Some(&grid));

    let entity = sim.substrate.entities.get(miner_id).expect("entity");
    let teleport = entity.teleport_state.as_ref().expect("teleport state");
    assert_eq!(
        (teleport.target_rx, teleport.target_ry),
        (13, 10),
        "blocked QueueingCell should use passable search, not the refinery pad"
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
// Test 7: Dock queuing — only one miner at a refinery at a time
// ==========================================================================
#[test]
fn return_within_too_far_distance_hands_off_to_enter_on_the_same_dispatch() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let config = MinerConfig::default();

    // GAREFN at (85,180) has the radio dock target at (88,181). A miner
    // approaching from the south can be stopped by movement CloseEnough at
    // (88,183), two cells away, after the footprint blocks the next step.
    let miner_id = spawn_miner(&mut sim, 100, MinerKind::War, 88, 183);
    spawn_refinery(&mut sim, 99, 85, 180);

    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        let miner = entity.miner.as_mut().expect("miner component");
        for _ in 0..20 {
            miner.cargo.push(CargoBale {
                resource_type: ResourceType::Ore,
                value: 25,
            });
        }
        entity
            .mission
            .set_handler_state(MinerState::ReturnToRefinery.cursor());
        miner.reserved_refinery = Some(99);
    }

    let grid = PathGrid::new(276, 276);
    super::miner_system::tick_miners(&mut sim, &rules, &config, Some(&grid));

    // Inside `HarvesterTooFarDistance`, state 2 sends HELLO on this same
    // dispatch (`0x0073EE51`) and the accepted reply is the Enter hand-off —
    // no approach phase, no adjacency requirement.
    let entity = sim.substrate.entities.get(miner_id).expect("miner entity");
    let miner = entity.miner.as_ref().expect("miner component");
    assert_eq!(entity.miner_state().unwrap(), MinerState::Dock);
    assert_eq!(miner.dock_phase, RefineryDockPhase::MissionEnter);
    assert!(crate::sim::miner::miner_dock::has_contact(
        &sim, 99, miner_id
    ));
}

#[test]
fn chrono_return_close_enough_enters_radio_dock_without_can_dock_move() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let config = MinerConfig::default();

    let miner_id = spawn_miner(&mut sim, 100, MinerKind::Chrono, 88, 183);
    spawn_refinery(&mut sim, 99, 85, 180);

    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        let miner = entity.miner.as_mut().expect("miner component");
        for _ in 0..20 {
            miner.cargo.push(CargoBale {
                resource_type: ResourceType::Ore,
                value: 25,
            });
        }
        entity
            .mission
            .set_handler_state(MinerState::ReturnToRefinery.cursor());
        miner.reserved_refinery = Some(99);
    }

    let grid = PathGrid::new(276, 276);
    sim.sound_events.clear();
    super::miner_system::tick_miners(&mut sim, &rules, &config, Some(&grid));

    let entity = sim.substrate.entities.get(miner_id).expect("miner entity");
    let miner = entity.miner.as_ref().expect("miner component");
    assert_eq!(
        entity.miner_state().unwrap(),
        MinerState::Dock,
        "close chrono return should enter the radio dock sequence immediately"
    );
    assert_eq!(
        miner.dock_phase,
        RefineryDockPhase::MissionEnter,
        "accepted close-return HELLO queues Mission_Enter for the next tick"
    );
    assert!(entity.teleport_state.is_none());
    assert!(
        entity.movement_target.is_none(),
        "HELLO acceptance must not issue the accepted-cell move in the same tick"
    );
    assert!(
        crate::sim::miner::miner_dock::has_contact(&sim, 99, miner_id),
        "close-return HELLO should populate the refinery contact list"
    );
    assert!(
        sim.sound_events.iter().all(|event| !matches!(
            event,
            crate::sim::world::SimSoundEvent::ChronoTeleport { .. }
        )),
        "near return must not emit chrono teleport sounds"
    );
}

#[test]
fn chrono_return_exact_dock_cell_enters_dock() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let config = MinerConfig::default();

    let miner_id = spawn_miner(&mut sim, 100, MinerKind::Chrono, 88, 181);
    spawn_refinery(&mut sim, 99, 85, 180);

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
        miner.reserved_refinery = Some(99);
    }

    let grid = PathGrid::new(276, 276);
    super::miner_system::tick_miners(&mut sim, &rules, &config, Some(&grid));

    let miner = get_miner(&sim, miner_id);
    assert_eq!(miner.state, MinerState::Dock);
    assert_eq!(miner.dock_phase, RefineryDockPhase::MissionEnter);
}

#[test]
fn dock_queuing_one_at_a_time() {
    let mut sim = Simulation::new();
    let rules = miner_rules();

    // Two miners at the dock cell, both ready to unload.
    let m1 = spawn_miner(&mut sim, 1, MinerKind::War, 14, 11);
    let m2 = spawn_miner(&mut sim, 3, MinerKind::War, 14, 11);
    spawn_refinery(&mut sim, 2, 10, 10);

    // Pre-load both with cargo, put in Dock Approach state (poll-and-link).
    for entity_id in [m1, m2] {
        let entity = sim
            .substrate
            .entities
            .get_mut(entity_id)
            .expect("miner entity");
        let miner = entity.miner.as_mut().expect("miner component");
        miner.cargo.push(CargoBale {
            resource_type: ResourceType::Ore,
            value: 25,
        });
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Approach;
        miner.reserved_refinery = Some(2);
    }

    // First tick: one should get the dock, other should wait.
    tick_miners_n(&mut sim, &rules, 1);

    let m1_miner = get_miner(&sim, m1);
    let m2_miner = get_miner(&sim, m2);

    // Miner with lower stable_id (1) processes first, wins HELLO contact,
    // and queues Mission_Enter. m2 is denied HELLO/contact but keeps the
    // receiver-style CAN_DOCK retry path; the refinery contact list is not
    // evicted/replaced.
    assert_eq!(
        m1_miner.state,
        MinerState::Dock,
        "First miner should still be docking"
    );
    assert_eq!(
        m1_miner.dock_phase,
        RefineryDockPhase::MissionEnter,
        "First miner should queue Mission_Enter after HELLO/ROGER"
    );
    assert_eq!(
        m2_miner.state,
        MinerState::Dock,
        "Second miner should still be docking"
    );
    assert_eq!(
        m2_miner.dock_phase,
        RefineryDockPhase::Approach,
        "Second miner should remain in HELLO retry/staging until the refinery contact frees"
    );
    assert!(
        crate::sim::miner::miner_dock::has_contact(&sim, 2, m1),
        "busy refinery must keep the current HELLO contact"
    );
    assert!(
        !crate::sim::miner::miner_dock::has_contact(&sim, 2, m2),
        "incoming full HELLO must not evict or replace Contacts[0]"
    );
    // V3: no stored wait-queue. m2 is denied — absent from the refinery's radio
    // contacts — and re-probes HELLO each tick (dock_queued stays set).
    assert!(
        !sim.substrate
            .entities
            .get(2)
            .expect("refinery")
            .radio_contacts
            .contains(m2),
        "denied HELLO must not place m2 in the refinery's radio contacts"
    );
    assert!(m2_miner.dock_queued, "denied miner keeps re-probing");
}

/// G6: a denied waiter's re-HELLO is gated to one dispatch per Harvest mission
/// cadence (~14-16f), not one per sim tick. Two full War Miners contest a
/// single-dock refinery; m1 wins the contact and m2 stays in Approach. Over a
/// contested window m2 re-anchors its `approach_hello_timer` once per Harvest
/// cadence — distinct anchor frames == HELLO dispatches — which must be far
/// below the number of contested ticks.
#[test]
fn approach_re_hello_gated_to_one_per_harvest_window() {
    let mut sim = Simulation::new();
    let rules = miner_rules();

    let m1 = spawn_miner(&mut sim, 1, MinerKind::War, 14, 11);
    let m2 = spawn_miner(&mut sim, 3, MinerKind::War, 14, 11);
    spawn_refinery(&mut sim, 2, 10, 10);

    for entity_id in [m1, m2] {
        let entity = sim
            .substrate
            .entities
            .get_mut(entity_id)
            .expect("miner entity");
        let miner = entity.miner.as_mut().expect("miner component");
        miner.cargo.push(CargoBale {
            resource_type: ResourceType::Ore,
            value: 25,
        });
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Approach;
        miner.reserved_refinery = Some(2);
    }

    let mut hello_arms = std::collections::BTreeSet::new();
    let mut contested_ticks = 0;
    for _ in 0..20 {
        tick_miners_n(&mut sim, &rules, 1);
        let m2m = get_miner(&sim, m2);
        if m2m.dock_phase == RefineryDockPhase::Approach {
            contested_ticks += 1;
            if m2m.approach_hello_timer.is_armed() {
                hello_arms.insert(m2m.approach_hello_timer.start_frame);
            }
        }
    }

    // m2 stays denied long enough to span at least one full Harvest window.
    assert!(
        contested_ticks >= 14,
        "m2 should stay contested across at least one cadence window (got {contested_ticks})"
    );
    assert!(
        !hello_arms.is_empty(),
        "the contested waiter must re-HELLO at least once"
    );
    // The gate: one HELLO per ~14-16f window. The pre-gate behavior re-HELLO'd
    // every tick, which would produce ~contested_ticks distinct anchors.
    assert!(
        hello_arms.len() <= 3,
        "re-HELLO must be gated to the Harvest cadence, not per tick: {} anchors over {} contested ticks",
        hello_arms.len(),
        contested_ticks,
    );
}

/// G5: the accepted HELLO must ARM the Enter cadence (base 14 + RandomRanged
/// (0,2) jitter), anchored at the accept frame — not clear the retry timer to
/// always-due, which collapses the first CAN_DOCK to the next tick and skips
/// the dispatch's RNG draw.
#[test]
fn accepted_hello_arms_enter_cadence_not_always_due() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let config = MinerConfig::default();
    let grid = PathGrid::new(64, 64);

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 14, 11);
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
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Approach;
        miner.reserved_refinery = Some(2);
    }

    // Uncontested single-dock slot: this dispatch accepts the HELLO.
    super::miner_system::tick_miners(&mut sim, &rules, &config, Some(&grid));
    let accept_frame = sim.session.binary_frame;

    let miner = get_miner(&sim, miner_id);
    assert_eq!(
        miner.dock_phase,
        RefineryDockPhase::MissionEnter,
        "accepted HELLO queues Mission_Enter"
    );
    // A real cadence, not an always-due clear.
    assert!(
        miner.dock_enter_retry.is_armed(),
        "accepted HELLO must ARM the Enter cadence, not clear it to always-due"
    );
    assert_eq!(
        miner.dock_enter_retry.start_frame, accept_frame,
        "the cadence is anchored at the accept frame"
    );
    let dur = miner.dock_enter_retry.duration;
    assert!(
        (14..=16).contains(&dur),
        "first CAN_DOCK waits base 14 + RandomRanged(0,2) jitter, got {dur}"
    );
    // Concretely: NOT next-tick, but due within the 14-16f window.
    assert!(
        !miner.dock_enter_retry.due(accept_frame + 1),
        "first CAN_DOCK must not fire the tick after accept"
    );
    assert!(
        miner.dock_enter_retry.due(accept_frame + 16),
        "first CAN_DOCK is due within the Enter cadence window"
    );
}

/// L20: `EnterDock(0x18)` fires one-per-due-dispatch, not on every arrived tick.
/// A miner sitting in FaceSync with an un-elapsed Enter cadence must not re-send
/// 0x18 (which sets the idempotent `dock_entered_with`); only the due dispatch
/// sends it.
#[test]
fn enter_dock_0x18_gated_to_due_dispatch_not_per_arrived_tick() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let config = MinerConfig::default();
    let grid = PathGrid::new(64, 64);

    // Miner parked at the accepted dock cell (13,11), registered as an entered
    // contact, sitting in FaceSync with the Enter cadence still counting down.
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 13, 11);
    spawn_refinery(&mut sim, 2, 10, 10);
    assert!(crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, miner_id));
    crate::sim::miner::miner_dock::enter_dock(&mut sim, miner_id, 2);
    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        entity.movement_target = None; // arrived
        let miner = entity.miner.as_mut().expect("miner component");
        miner.cargo.push(CargoBale {
            resource_type: ResourceType::Ore,
            value: 25,
        });
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::FaceSync;
        miner.reserved_refinery = Some(2);
        // Arm the Enter cadence so the next arrived ticks are NOT due.
        miner.dock_enter_retry.arm(sim.session.binary_frame, 14);
    }
    // Clear the radio-entered flag so any spurious 0x18 re-send is observable.
    sim.substrate
        .entities
        .get_mut(miner_id)
        .expect("entity")
        .dock_entered_with = None;

    // A not-due arrived tick must NOT re-send EnterDock.
    super::miner_system::tick_miners(&mut sim, &rules, &config, Some(&grid));
    assert_eq!(
        sim.substrate
            .entities
            .get(miner_id)
            .expect("entity")
            .dock_entered_with,
        None,
        "L20: EnterDock(0x18) must not fire on a non-due arrived tick"
    );
    assert_eq!(
        get_miner(&sim, miner_id).dock_phase,
        RefineryDockPhase::FaceSync,
        "still waiting the Enter cadence"
    );

    // Cross the cadence: the due Enter dispatch sends exactly one 0x18.
    sim.session.binary_frame += 15;
    super::miner_system::tick_miners(&mut sim, &rules, &config, Some(&grid));
    assert_eq!(
        sim.substrate
            .entities
            .get(miner_id)
            .expect("entity")
            .dock_entered_with,
        Some(2),
        "L20: the due Enter dispatch sends EnterDock(0x18)"
    );
}

/// L9: the accepted FaceSync->MissionQueued handoff is still a Mission_Enter
/// dispatch in gamemd and draws exactly one `RandomRanged(0,2)` from the scenario
/// RNG; Rust previously cleared the timer with no draw, dropping one draw per
/// dock cycle and desyncing the jitter stream.
#[test]
fn accepted_face_sync_handoff_draws_one_scenario_rng() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let config = MinerConfig::default();
    let grid = PathGrid::new(64, 64);

    // Miner at the accepted dock cell, already facing East (0x40) so the pivot
    // accepts immediately, arrived, an entered contact, in FaceSync with a
    // due Enter cadence (default/always-due) so the handoff fires this tick.
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 13, 11);
    spawn_refinery(&mut sim, 2, 10, 10);
    assert!(crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, miner_id));
    crate::sim::miner::miner_dock::enter_dock(&mut sim, miner_id, 2);
    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        entity.movement_target = None;
        entity.facing = 0x40; // East → sync_dock_facing accepts on the first pass
        let miner = entity.miner.as_mut().expect("miner component");
        miner.cargo.push(CargoBale {
            resource_type: ResourceType::Ore,
            value: 25,
        });
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::FaceSync;
        miner.reserved_refinery = Some(2);
    }

    // Probe: cloning the scenario RNG and drawing once gives the exact state a
    // single RandomRanged(0,2) reaches. No other draw happens on this tick.
    let mut probe = sim.miner_jitter_rng().clone();
    let _ = probe.next_range_u32_inclusive(0, 2);
    let expected_after_one_draw = probe.state();

    super::miner_system::tick_miners(&mut sim, &rules, &config, Some(&grid));

    assert_eq!(
        get_miner(&sim, miner_id).dock_phase,
        RefineryDockPhase::MissionQueued,
        "the accepted handoff advances FaceSync -> MissionQueued"
    );
    assert_eq!(
        sim.miner_jitter_rng().state(),
        expected_after_one_draw,
        "L9: the accepted FaceSync handoff must draw exactly one scenario RandomRanged(0,2)"
    );
}

/// The Mission_Deploy state-4 exit returns through the dispatch epilogue:
/// one `RandomRanged(0,2)` (Scen->Random) on top of the `[Harvest] Rate`
/// base, written into the mission dispatch timer — so the resumed ore search
/// waits the full base + jitter, and the internal harvest timer is untouched.
#[test]
fn state_four_exit_draws_and_applies_resume_jitter() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let config = MinerConfig::default();
    let grid = PathGrid::new(64, 64);

    // Miner sitting at the end of the dock sequence (state-4 Departing), cargo
    // already unloaded so the resumed search is not short-circuited by is_full().
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 13, 11);
    spawn_refinery(&mut sim, 2, 10, 10);
    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        let miner = entity.miner.as_mut().expect("miner component");
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Departing;
        miner.reserved_refinery = Some(2);
    }

    let exit_frame = sim.session.binary_frame;
    // Probe: the tick draws one RandomRanged(0,2); mirror it to learn the exact
    // jitter value and the post-draw RNG state.
    let mut probe = sim.miner_jitter_rng().clone();
    let jitter = probe.next_range_u32_inclusive(0, 2);
    let expected_after_one_draw = probe.state();

    super::miner_system::tick_miners(&mut sim, &rules, &config, Some(&grid));

    let miner = get_miner(&sim, miner_id);
    assert_eq!(
        miner.state,
        MinerState::SearchOre,
        "state-4 exit hands back to SearchOre"
    );
    // Exactly one scenario RNG draw at the exit.
    assert_eq!(
        sim.miner_jitter_rng().state(),
        expected_after_one_draw,
        "state-4 exit must draw exactly one scenario RandomRanged(0,2)"
    );
    // The resume is paced through the dispatch epilogue: delay = the
    // [Harvest] Rate base + the drawn jitter, anchored at the exit frame.
    let base = super::miner_dock_sequence::mission_base_frames(
        &rules,
        crate::sim::mission::MissionType::Harvest,
        14,
    );
    let timer = sim
        .substrate
        .entities
        .get(miner_id)
        .expect("miner entity")
        .mission
        .dispatch_timer();
    assert_eq!(
        timer.start_frame(),
        exit_frame as i32,
        "dispatch epilogue anchored at the state-4 exit frame"
    );
    assert_eq!(
        timer.delay(),
        i32::from(base) + jitter as i32,
        "dispatch delay is the [Harvest] Rate base plus the drawn jitter"
    );
    assert!(
        !miner.harvest_timer.is_armed(),
        "the internal harvest timer is no longer armed at the state-4 exit"
    );
}

// ==========================================================================
// Test 8: Credits arrive per slot drain (whole-slot dump per timer tick)
// ==========================================================================
/// gamemd dumps an entire StorageClass slot (all bales of one resource type)
/// per HarvesterDumpRate threshold crossing. Pure-ore cargo drains in one
/// dump tick (~15 frames after dock-link); mixed ore+gems drains in two.
/// Test pure-ore (1 slot) and mixed (2 slots) and assert each slot fully
/// arrives on a single tick.
#[test]
fn credits_arrive_per_slot_during_unload() {
    // --- Pure ore (1 slot) ---
    let mut sim = Simulation::new();
    let rules = miner_rules();

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 14, 11);
    spawn_refinery(&mut sim, 2, 10, 10);

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
        miner.dock_phase = RefineryDockPhase::Unloading;
        miner.reserved_refinery = Some(2);
    }
    crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, miner_id);

    let before = credits_for_owner(&sim, "Americans");

    // Single tick at timer=0 drains the entire ore slot → 10 × 25 = 250
    // credits in one shot.
    tick_miners_n(&mut sim, &rules, 1);
    let after = credits_for_owner(&sim, "Americans");
    assert_eq!(
        after - before,
        250,
        "pure-ore cargo must drain in one slot dump (250 cr in one tick)",
    );

    // --- Mixed ore + gems (2 slots) ---
    let mut sim = Simulation::new();
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 14, 11);
    spawn_refinery(&mut sim, 2, 10, 10);

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
        for _ in 0..5 {
            miner.cargo.push(CargoBale {
                resource_type: ResourceType::Gem,
                value: 50,
            });
        }
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Unloading;
        miner.reserved_refinery = Some(2);
    }
    crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, miner_id);

    let before = credits_for_owner(&sim, "Americans");
    tick_miners_n(&mut sim, &rules, 1);
    let after_first_drain = credits_for_owner(&sim, "Americans");
    assert_eq!(
        after_first_drain - before,
        250,
        "first drain must be ORE slot (slot 0) = 10 × 25 = 250 cr",
    );

    // Second drain fires one full unload_tick_interval later. With the
    // decrement-then-check structure (timer -= 10 happens BEFORE the drain
    // check), timer crosses ≤ 0 on the 16th tick after the first drain
    // (144 → 134 → ... → 4 → -6 → drain).
    tick_miners_n(&mut sim, &rules, 16);
    let after_second_drain = credits_for_owner(&sim, "Americans");
    assert_eq!(
        after_second_drain - before,
        250 + 250,
        "second drain must be GEM slot = 5 × 50 = 250 cr (total 500)",
    );
}

// ==========================================================================
// Test 9: After ore cell empties, miner searches for more (local continuation)
// ==========================================================================
#[test]
fn local_continuation_after_cell_depletes() {
    let mut sim = Simulation::new();
    let rules = miner_rules();

    // Miner at (20, 20). Two ore cells: one small (will deplete), one nearby.
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 20, 20);
    spawn_refinery(&mut sim, 2, 10, 10);
    // 2 density levels at (20, 20) and a richer patch nearby (within local
    // continuation radius of 6 cells).
    place_ore(&mut sim, 20, 20, 2 * 120);
    place_ore(&mut sim, 22, 20, 100 * 120);

    // Put miner in Harvest state at its position.
    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        let miner = entity.miner.as_mut().expect("miner component");
        entity
            .mission
            .set_handler_state(MinerState::Harvest.cursor());
        miner.target_ore_cell = Some((20, 20));
        miner.harvest_timer.clear();
    }

    // Tick enough to deplete the small cell and search for the next. One
    // level per 19-frame gate (Harvest_Ore_Tick @ 0x0073D450 requests
    // min(1, free)): bales at frames 1 and 20, the empty-cell gate at 39.
    tick_miners_n(&mut sim, &rules, 40);

    let miner = get_miner(&sim, miner_id);
    // After (20,20) depletes, the short-scan continuation must pick (22,20)
    // and the miner transitions to MoveToOre / Harvest (gamemd State 1
    // depletion path: stay harvesting, move to new cell within
    // TiberiumShortScan radius).
    assert_eq!(
        miner.target_ore_cell,
        Some((22, 20)),
        "Short-scan continuation should pick the nearby ore at (22, 20)"
    );
    assert!(
        matches!(miner.state, MinerState::MoveToOre | MinerState::Harvest),
        "Miner should be moving to / harvesting the new cell; state was {:?}",
        miner.state,
    );
}

// ==========================================================================
// Test 9a: Cell depletes with PARTIAL cargo → miner continues to nearby ore
//          (the short-scan-before-return behavior, gamemd State 1)
// ==========================================================================
#[test]
fn harvest_continues_to_nearby_ore_when_cell_depletes_partial_cargo() {
    let mut sim = Simulation::new();
    let rules = miner_rules();

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 20, 20);
    spawn_refinery(&mut sim, 2, 10, 10);
    // Cell at miner's position: 2 density levels (2 × ore-base 120 = 240).
    place_ore(&mut sim, 20, 20, 2 * 120);
    // Nearby ore well within TiberiumShortScan (radius 6 cells).
    place_ore(&mut sim, 23, 20, 100 * 120);

    {
        let entity = sim.substrate.entities.get_mut(miner_id).expect("miner");
        let miner = entity.miner.as_mut().expect("miner component");
        entity
            .mission
            .set_handler_state(MinerState::Harvest.cursor());
        miner.target_ore_cell = Some((20, 20));
        miner.harvest_timer.clear();
    }

    // Tick enough to deplete (20,20) and trigger the continuation scan: one
    // level per 19-frame gate, so the empty-cell gate fires at frame 39.
    tick_miners_n(&mut sim, &rules, 40);

    let miner = get_miner(&sim, miner_id);
    assert!(
        !miner.cargo.is_empty(),
        "Miner should have extracted bales before cell depleted"
    );
    assert_eq!(
        miner.target_ore_cell,
        Some((23, 20)),
        "After cell depleted, miner should pick the nearby ore via short scan"
    );
    assert!(
        matches!(miner.state, MinerState::MoveToOre | MinerState::Harvest),
        "Miner should move to / be harvesting the new ore cell, not return-to-refinery; \
         state was {:?}",
        miner.state,
    );
    assert!(
        !matches!(miner.state, MinerState::ReturnToRefinery | MinerState::Dock),
        "Miner with ore nearby must NOT head to refinery on partial cargo"
    );
}

// ==========================================================================
// Test 9b: Cell depletes with PARTIAL cargo + no ore nearby → miner returns
// ==========================================================================
#[test]
fn harvest_returns_when_no_ore_within_short_scan() {
    let mut sim = Simulation::new();
    let rules = miner_rules();

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 20, 20);
    spawn_refinery(&mut sim, 2, 10, 10);
    // Only the miner's cell has ore (2 density levels = 240 base units).
    // Nothing within the short-scan radius (default 6 cells). The further
    // ore patch is well outside.
    place_ore(&mut sim, 20, 20, 2 * 120);
    place_ore(&mut sim, 50, 50, 100 * 120); // far outside local_continuation_radius

    {
        let entity = sim.substrate.entities.get_mut(miner_id).expect("miner");
        let miner = entity.miner.as_mut().expect("miner component");
        entity
            .mission
            .set_handler_state(MinerState::Harvest.cursor());
        miner.target_ore_cell = Some((20, 20));
        miner.harvest_timer.clear();
    }

    // One level per 19-frame gate: the empty-cell gate fires at frame 39.
    tick_miners_n(&mut sim, &rules, 40);

    let miner = get_miner(&sim, miner_id);
    assert!(
        !miner.cargo.is_empty(),
        "Miner should have extracted bales before depletion"
    );
    assert!(
        matches!(miner.state, MinerState::ReturnToRefinery | MinerState::Dock),
        "With cargo but no nearby ore, miner must head to refinery; state was {:?}",
        miner.state,
    );
}

// ==========================================================================
// Test 9c: EMPTY-cargo cell depletion + short-scan miss → return to refinery
//          (gamemd case-1 parity: cargo is irrelevant to the miss → state 2
//           transition. Empty miners detour home before re-scanning, matching
//           gamemd's observable travel path.)
// ==========================================================================
#[test]
fn empty_cargo_cell_depletion_returns_to_refinery() {
    let mut sim = Simulation::new();
    let rules = miner_rules();

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 20, 20);
    spawn_refinery(&mut sim, 2, 10, 10);
    // No ore on the miner's cell. Nothing within short-scan radius (6 cells).
    // The far ore patch is outside short-scan; gamemd does NOT run a long
    // scan from case 1 — it transitions to state 2 (return) on miss.
    place_ore(&mut sim, 40, 20, 100);

    {
        let entity = sim.substrate.entities.get_mut(miner_id).expect("miner");
        let miner = entity.miner.as_mut().expect("miner component");
        entity
            .mission
            .set_handler_state(MinerState::Harvest.cursor());
        miner.target_ore_cell = Some((20, 20));
        miner.harvest_timer.clear();
        // Cargo intentionally empty — extract_bale will fail on first tick.
        assert!(miner.cargo.is_empty());
    }

    tick_miners_n(&mut sim, &rules, 5);

    let miner = get_miner(&sim, miner_id);
    assert!(
        miner.cargo.is_empty(),
        "No ore was on the cell, so no bales should have been extracted"
    );
    assert!(
        matches!(miner.state, MinerState::ReturnToRefinery | MinerState::Dock),
        "Empty-cargo miner on a depleted cell with no short-scan hit should \
         head to the refinery (gamemd state 2); state was {:?}",
        miner.state,
    );
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
// Test 11: After unload, home_refinery rebinds to the refinery used
// ==========================================================================
#[test]
fn home_refinery_rebinds_after_unload() {
    let mut sim = Simulation::new();
    let rules = miner_rules();

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 14, 11);
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
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Unloading;
        miner.reserved_refinery = Some(2);
        miner.home_refinery = None; // Start without a home
    }
    // The unload mission's In_Radio_Contact gate needs the admitted contact
    // a real approach leaves behind.
    assert!(crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, miner_id));

    // Tick until unload completes: 1 bale × unload_interval=57 ticks.
    tick_miners_n(&mut sim, &rules, 70);

    let miner = get_miner(&sim, miner_id);
    assert_eq!(
        miner.home_refinery,
        Some(2),
        "Home refinery should rebind to the refinery used for unloading"
    );
}

// ==========================================================================
// Test 12: Forced return (MinerReturn command) triggers Chrono teleport
// ==========================================================================
#[test]
fn forced_return_chrono_teleports() {
    let mut sim = Simulation::new();
    let rules = miner_rules();

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::Chrono, 80, 80);
    spawn_refinery(&mut sim, 2, 10, 10);

    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        let miner = entity.miner.as_mut().expect("miner component");
        entity
            .mission
            .set_handler_state(MinerState::ForcedReturn.cursor());
        miner.forced_return = true;
    }

    tick_miners_n(&mut sim, &rules, 1);

    let entity = sim.substrate.entities.get(miner_id).expect("entity");
    assert!(
        entity.teleport_state.is_some(),
        "Forced return should issue an inbound chrono teleport"
    );
    let teleport = entity.teleport_state.as_ref().expect("teleport state");
    assert_eq!((teleport.target_rx, teleport.target_ry), (14, 11));
    assert_eq!(
        entity.miner.as_ref().and_then(|m| m.reserved_refinery),
        Some(2),
        "Forced return should select a refinery target before docking contact"
    );
}

#[test]
fn chrono_return_within_too_far_threshold_uses_close_radio_path() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let config = MinerConfig::from_general_rules(&rules.general);
    let grid = PathGrid::new(64, 64);

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::Chrono, 40, 40);
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

    super::miner_system::tick_miners(&mut sim, &rules, &config, Some(&grid));
    // G5: the accepted close-return HELLO now arms the Enter cadence; cross the
    // ~14-16f window so the deferred CAN_DOCK dispatch becomes due.
    sim.session.binary_frame += 17;
    super::miner_system::tick_miners(&mut sim, &rules, &config, Some(&grid));

    let entity = sim.substrate.entities.get(miner_id).expect("entity");
    assert!(
        entity.teleport_state.is_none(),
        "Chrono Miner inside ChronoHarvTooFarDistance should not take the far QueueingCell fallback"
    );
    assert_eq!(
        entity.miner.as_ref().and_then(|m| m.reserved_refinery),
        Some(2)
    );
    let movement = entity
        .movement_target
        .as_ref()
        .expect("close return should path toward the accepted dock cell");
    assert_eq!(
        movement
            .final_goal
            .or_else(|| movement.path.last().copied()),
        Some((13, 11)),
        "close return should use the refinery CAN_DOCK accepted cell, not QueueingCell"
    );
}

// ==========================================================================
// Chrono close/far return radio threshold pins. The distance is measured to
// `BuildingClass::GetCoords @ 0x00447AC0` = the 4x3 foundation centre: NW
// (10, 10) -> centre (3072, 2944) leptons, i.e. the centre of cell (12, 11).
// A miner in cell (62, 11) with sub_x = 0 sits at x = 15872, exactly 50 cells
// (12800 leptons) east of that point with dy = dz = 0. Native runs
// `ftol(Sqrt_Approx(d²)) <= 50*256`; the `Sqrt_Approx @ 0x004CAC40` table
// rounds down, so d = 12801 -> 12800.64 -> 12800 is still close and d = 12802
// is the first far distance.
// ==========================================================================
#[test]
fn chrono_return_at_sqrt_approx_too_far_edge_uses_close_radio_path() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let config = MinerConfig::from_general_rules(&rules.general);
    let grid = PathGrid::new(96, 96);

    spawn_refinery(&mut sim, 2, 10, 10);
    // 12801 leptons: one past the exact threshold, inside the table edge.
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::Chrono, 62, 11);
    sim.substrate
        .entities
        .get_mut(miner_id)
        .expect("miner entity")
        .position
        .sub_x = crate::util::fixed_math::SimFixed::from_num(1);

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

    super::miner_system::tick_miners(&mut sim, &rules, &config, Some(&grid));

    let entity = sim.substrate.entities.get(miner_id).expect("entity");
    let miner = entity.miner.as_ref().expect("miner component");
    assert!(
        entity.teleport_state.is_none(),
        "12801 leptons truncates to 12800 through Sqrt_Approx: still the close radio path"
    );
    assert_eq!(entity.miner_state().unwrap(), MinerState::Dock);
    assert_eq!(miner.dock_phase, RefineryDockPhase::MissionEnter);
    assert!(crate::sim::miner::miner_dock::has_contact(
        &sim, 2, miner_id
    ));
}

#[test]
fn chrono_return_over_too_far_threshold_uses_queueingcell_teleport() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let config = MinerConfig::from_general_rules(&rules.general);
    let grid = PathGrid::new(96, 96);

    spawn_refinery(&mut sim, 2, 10, 10);
    // 12802 leptons: the first distance Sqrt_Approx + ftol reads as > 12800.
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::Chrono, 62, 11);
    sim.substrate
        .entities
        .get_mut(miner_id)
        .expect("miner entity")
        .position
        .sub_x = crate::util::fixed_math::SimFixed::from_num(2);

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

    super::miner_system::tick_miners(&mut sim, &rules, &config, Some(&grid));

    let entity = sim.substrate.entities.get(miner_id).expect("entity");
    let teleport = entity
        .teleport_state
        .as_ref()
        .expect("over-threshold chrono return should teleport");
    assert_eq!(
        (teleport.target_rx, teleport.target_ry),
        (14, 11),
        "far return should land at QueueingCell staging"
    );
    assert!(!crate::sim::miner::miner_dock::has_contact(
        &sim, 2, miner_id
    ));
}

#[test]
fn chrono_close_hello_refused_stages_at_queueingcell_without_receiver_eviction() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let config = MinerConfig::from_general_rules(&rules.general);
    let grid = PathGrid::new(64, 64);

    let occupant = spawn_miner(&mut sim, 1, MinerKind::Chrono, 13, 11);
    spawn_refinery(&mut sim, 2, 10, 10);
    let waiter = spawn_miner(&mut sim, 3, MinerKind::Chrono, 20, 10);
    assert!(crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, occupant));

    {
        let entity = sim
            .substrate
            .entities
            .get_mut(waiter)
            .expect("waiter entity");
        let miner = entity.miner.as_mut().expect("waiter miner");
        miner.cargo.push(CargoBale {
            resource_type: ResourceType::Ore,
            value: 25,
        });
        entity
            .mission
            .set_handler_state(MinerState::ReturnToRefinery.cursor());
    }

    super::miner_system::tick_miners(&mut sim, &rules, &config, Some(&grid));

    let waiter_entity = sim.substrate.entities.get(waiter).expect("waiter entity");
    let waiter_miner = waiter_entity.miner.as_ref().expect("waiter miner");
    assert!(crate::sim::miner::miner_dock::has_contact(
        &sim, 2, occupant
    ));
    assert!(
        !crate::sim::miner::miner_dock::has_contact(&sim, 2, waiter),
        "refused HELLO must not evict or replace the receiver-side contact"
    );
    assert!(
        !sim.substrate
            .entities
            .get(2)
            .expect("refinery")
            .radio_contacts
            .contains(waiter),
        "denied waiter is absent from the refinery's radio contacts (no wait-queue)"
    );
    assert_eq!(waiter_entity.miner_state().unwrap(), MinerState::Dock);
    assert_eq!(waiter_miner.dock_phase, RefineryDockPhase::Approach);
    assert!(waiter_miner.dock_queued);
    let movement = waiter_entity
        .movement_target
        .as_ref()
        .expect("refused close-return miner should stage at QueueingCell");
    assert_eq!(
        movement
            .final_goal
            .or_else(|| movement.path.last().copied()),
        Some((14, 11)),
        "QueueingCell staging must stay distinct from accepted cell (13,11)"
    );
}

#[test]
fn cmin_close_hello_success_defers_can_dock_to_mission_enter() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let config = MinerConfig::from_general_rules(&rules.general);
    let grid = PathGrid::new(64, 64);

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::Chrono, 40, 40);
    spawn_refinery(&mut sim, 2, 10, 10);

    {
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
        entity
            .mission
            .set_handler_state(MinerState::ReturnToRefinery.cursor());
    }

    super::miner_system::tick_miners(&mut sim, &rules, &config, Some(&grid));

    let entity = sim.substrate.entities.get(miner_id).expect("miner entity");
    let miner = entity.miner.as_ref().expect("miner component");
    assert_eq!(entity.miner_state().unwrap(), MinerState::Dock);
    assert_eq!(miner.dock_phase, RefineryDockPhase::MissionEnter);
    assert!(crate::sim::miner::miner_dock::has_contact(
        &sim, 2, miner_id
    ));
    assert!(
        !crate::sim::miner::miner_dock::has_entered(&sim, 2, miner_id),
        "HELLO success must not set the entered flag"
    );
    assert!(
        entity.movement_target.is_none(),
        "HELLO success must not issue CAN_DOCK movement in the same tick"
    );

    // G5: the accepted HELLO arms the Enter cadence; cross the ~14-16f window so
    // the deferred Mission_Enter/CAN_DOCK dispatch is due.
    sim.session.binary_frame += 17;
    super::miner_system::tick_miners(&mut sim, &rules, &config, Some(&grid));

    let entity = sim.substrate.entities.get(miner_id).expect("miner entity");
    let movement = entity
        .movement_target
        .as_ref()
        .expect("MissionEnter should now issue CAN_DOCK movement");
    assert_eq!(
        movement
            .final_goal
            .or_else(|| movement.path.last().copied()),
        Some((13, 11)),
        "CAN_DOCK must use accepted cell, not QueueingCell"
    );
    assert!(!crate::sim::miner::miner_dock::has_entered(
        &sim, 2, miner_id
    ));
}

#[test]
fn cmin_refused_close_return_stages_at_queueingcell_then_can_dock_uses_accepted_cell() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let config = MinerConfig::from_general_rules(&rules.general);
    let grid = PathGrid::new(64, 64);

    let occupant = spawn_miner(&mut sim, 1, MinerKind::Chrono, 13, 11);
    spawn_refinery(&mut sim, 2, 10, 10);
    let waiter = spawn_miner(&mut sim, 3, MinerKind::Chrono, 20, 10);
    assert!(crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, occupant));
    crate::sim::miner::miner_dock::enter_dock(&mut sim, occupant, 2);

    {
        let entity = sim
            .substrate
            .entities
            .get_mut(waiter)
            .expect("waiter entity");
        let miner = entity.miner.as_mut().expect("waiter miner");
        for _ in 0..miner.capacity_bales {
            miner.cargo.push(CargoBale {
                resource_type: ResourceType::Ore,
                value: 25,
            });
        }
        entity
            .mission
            .set_handler_state(MinerState::ReturnToRefinery.cursor());
    }

    super::miner_system::tick_miners(&mut sim, &rules, &config, Some(&grid));

    let waiter_entity = sim.substrate.entities.get(waiter).expect("waiter entity");
    let waiter_miner = waiter_entity.miner.as_ref().expect("waiter miner");
    assert_eq!(waiter_entity.miner_state().unwrap(), MinerState::Dock);
    assert_eq!(waiter_miner.dock_phase, RefineryDockPhase::Approach);
    assert!(waiter_miner.dock_queued);
    assert!(
        !crate::sim::miner::miner_dock::has_contact(&sim, 2, waiter),
        "denied waiter is not admitted (no wait-queue); it keeps re-probing"
    );
    let movement = waiter_entity
        .movement_target
        .as_ref()
        .expect("refused close-return miner should stage at QueueingCell");
    assert_eq!(
        movement
            .final_goal
            .or_else(|| movement.path.last().copied()),
        Some((14, 11)),
        "refused close return stages at QueueingCell"
    );

    crate::sim::miner::miner_dock::break_contact(&mut sim, occupant, 2);
    {
        let entity = sim
            .substrate
            .entities
            .get_mut(waiter)
            .expect("waiter entity");
        entity.position.rx = 14;
        entity.position.ry = 11;
        entity.movement_target = None;
    }

    // The state-2 dispatch exits through the Rate epilogue (~14-16f); cross
    // the full window so the waiter's next dispatch is due again.
    sim.session.binary_frame += 17;
    super::miner_system::tick_miners(&mut sim, &rules, &config, Some(&grid));
    let waiter_miner = get_miner(&sim, waiter);
    assert_eq!(
        waiter_miner.dock_phase,
        RefineryDockPhase::MissionEnter,
        "Approach after release performs HELLO only"
    );
    assert!(crate::sim::miner::miner_dock::has_contact(&sim, 2, waiter));
    assert!(!crate::sim::miner::miner_dock::has_entered(&sim, 2, waiter));

    // G5: the accepted HELLO arms the Enter cadence; cross the ~14-16f window so
    // the deferred CAN_DOCK dispatch is due.
    sim.session.binary_frame += 17;
    super::miner_system::tick_miners(&mut sim, &rules, &config, Some(&grid));
    let waiter_entity = sim.substrate.entities.get(waiter).expect("waiter entity");
    let movement = waiter_entity
        .movement_target
        .as_ref()
        .expect("MissionEnter should move from QueueingCell to accepted cell");
    assert_eq!(
        movement
            .final_goal
            .or_else(|| movement.path.last().copied()),
        Some((13, 11)),
        "accepted CAN_DOCK uses NW+(3,1), not QueueingCell"
    );
    assert!(!crate::sim::miner::miner_dock::has_entered(&sim, 2, waiter));
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
    let miner = entity.miner.as_ref().expect("miner");
    assert_eq!(miner.target_ore_cell, Some((50, 50)));
    assert_eq!(entity.miner_state().unwrap(), MinerState::MoveToOre);
}

// ==========================================================================
// Test: Chrono teleport emits ChronoInSound + ChronoOutSound at correct cells
// ==========================================================================
/// On a chrono miner return-warp, the sim must emit two `ChronoTeleport` sound
/// events:
///   - one at the source cell with the unit's `ChronoOutSound=`
///   - one at the destination cell with the unit's `ChronoInSound=`
#[test]
fn chrono_teleport_emits_in_and_out_sounds_at_correct_cells() {
    use crate::sim::world::SimSoundEvent;

    let mut sim = Simulation::new();
    let rules = miner_rules();

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::Chrono, 80, 80);
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

    sim.sound_events.clear();
    tick_miners_n(&mut sim, &rules, 1);

    let chrono_events: Vec<_> = sim
        .sound_events
        .iter()
        .filter(|e| matches!(e, SimSoundEvent::ChronoTeleport { .. }))
        .collect();

    assert_eq!(
        chrono_events.len(),
        2,
        "chrono return should emit one ChronoOut and one ChronoIn sound"
    );
    assert!(
        chrono_events
            .iter()
            .any(|event| matches!(event, SimSoundEvent::ChronoTeleport { rx: 14, ry: 11, .. })),
        "ChronoIn sound should be anchored at the QueueingCell staging cell"
    );
}

/// Stock zero-link refinery completion does not emit the conditional
/// `ReleaseDockedHarvester` departure sound.
#[test]
fn stock_dock_exit_does_not_emit_refinery_exit_sfx() {
    use crate::sim::world::SimSoundEvent;

    let mut sim = Simulation::new();
    let ini = IniFile::from_str(
        "[InfantryTypes]\n\
         [VehicleTypes]\n\
         0=HARV\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         0=GAREFN\n\
         [General]\n\
         FixtureOnly=1\n\
         [AudioVisual]\n\
         BunkerWallsDownSound=TankBunkerDown\n\
         [HARV]\n\
         Name=War Miner\n\
         Speed=4\n\
         Owner=Americans\n\
         Harvester=yes\n\
         Dock=GAREFN\n\
         [GAREFN]\n\
         Name=Ore Refinery\n\
         Foundation=4x3\n\
         Owner=Americans\n\
         Refinery=yes\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("rules with BunkerWallsDownSound");
    assert_eq!(
        rules.general.bunker_walls_down_sound.as_deref(),
        Some("TankBunkerDown"),
        "parser must read BunkerWallsDownSound from [AudioVisual]"
    );

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 13, 11);
    spawn_refinery(&mut sim, 2, 10, 10);
    {
        let entity = sim.substrate.entities.get_mut(miner_id).expect("miner");
        let miner = entity.miner.as_mut().expect("miner component");
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Departing;
        miner.reserved_refinery = Some(2);
        assert!(
            miner.exit_cell.is_none(),
            "precondition: stock handoff starts without a cached exit cell"
        );
    }
    crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, miner_id);
    sim.sound_events.clear();

    // Single tick: stock state-4 handoff. No ReleaseDockedHarvester SFX.
    let config = MinerConfig::default();
    let grid = PathGrid::new(64, 64);
    super::miner_system::tick_miners(&mut sim, &rules, &config, Some(&grid));

    let refinery_exit_events: Vec<_> = sim
        .sound_events
        .iter()
        .filter(|e| matches!(e, SimSoundEvent::RefineryExitSfx { .. }))
        .collect();
    assert!(
        refinery_exit_events.is_empty(),
        "stock zero-link dock completion must not emit RefineryExitSfx"
    );
    let miner = get_miner(&sim, miner_id);
    assert_eq!(miner.state, MinerState::SearchOre);
    assert!(miner.exit_cell.is_none());
}

/// Variant of `miner_rules()` where CMIN omits the per-unit `ChronoInSound`
/// and `ChronoOutSound` keys, and `[AudioVisual]` sets distinctive fallback
/// values. Used by the fallback-path test to prove the resolver reads from
/// Rules when the per-unit field is absent.
fn miner_rules_fallback_only() -> RuleSet {
    let ini = IniFile::from_str(
        "[General]\n\
         FixtureOnly=1\n\
         [AudioVisual]\n\
         ChronoInSound=FALLBACKIN\n\
         ChronoOutSound=FALLBACKOUT\n\
         [InfantryTypes]\n\
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
         Dock=GAREFN\n\
         [GAREFN]\n\
         Name=Ore Refinery\n\
         Cost=2000\n\
         Strength=900\n\
         Armor=wood\n\
         TechLevel=1\n\
         Owner=Americans\n\
         Foundation=4x3\n\
         Refinery=yes\n\
         FreeUnit=CMIN\n",
    );
    RuleSet::from_ini(&ini).expect("miner fallback rules")
}

/// A2/L1: the miner Enter/Unload base cadences are sourced from the parsed
/// `MissionControl` Rate table (not a hardcode), falling back to the passed
/// constant only for a keyless mission (U5 guard). Stock `[Enter]/[Unload]/
/// [Harvest] Rate=.016` all resolve to 14 frames, so wiring the table leaves the
/// stock cadence byte-identical to the old constant.
#[test]
fn mission_base_frames_reads_rate_table_stock_identical() {
    use crate::sim::miner::miner_dock_sequence::mission_base_frames;
    use crate::sim::mission::MissionType;

    let ini = IniFile::from_str(
        "[General]\n\
         FixtureOnly=1\n\
         [AudioVisual]\n\
         ChronoInSound=FALLBACKIN\n\
         ChronoOutSound=FALLBACKOUT\n\
         [InfantryTypes]\n\
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
         Dock=GAREFN\n\
         [GAREFN]\n\
         Name=Ore Refinery\n\
         Cost=2000\n\
         Strength=900\n\
         Armor=wood\n\
         TechLevel=1\n\
         Owner=Americans\n\
         Foundation=4x3\n\
         Refinery=yes\n\
         FreeUnit=CMIN\n\
         [Enter]\n\
         Rate=.016\n\
         [Unload]\n\
         Rate=.016\n\
         [Harvest]\n\
         Rate=.016\n\
         [Guard]\n\
         Rate=.030\n\
         AARate=.016\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("mission-rate rules");

    // U5 guard: the miner missions all carry a non-zero cadence in the table.
    assert_ne!(rules.mission_control.rate_frames(MissionType::Enter), 0);
    assert_ne!(rules.mission_control.rate_frames(MissionType::Unload), 0);
    assert_ne!(rules.mission_control.rate_frames(MissionType::Harvest), 0);

    // Table-sourced base == the stock 14-frame value. A non-14 fallback (99)
    // proves the value came from the table, not the fallback branch.
    assert_eq!(mission_base_frames(&rules, MissionType::Enter, 99), 14);
    assert_eq!(mission_base_frames(&rules, MissionType::Unload, 99), 14);
    assert_eq!(mission_base_frames(&rules, MissionType::Harvest, 99), 14);

    // Sanity: the table also carries Guard's distinct 26/14 Rate/AARate split.
    assert_eq!(rules.mission_control.rate_frames(MissionType::Guard), 26);

    // Keyless mission (no [Stop] section) → the slot keeps its constructed
    // 0.016-minute rate, i.e. 14 frames. Not zero — so an ABSENT section never
    // reaches the zero-sentinel fallback in `mission_base_frames` (recorded:
    // that fallback is a VERA-internal gate the original does not have). A
    // PRESENT `Rate=0`, or a `Rate=` VERA cannot parse, does reach it.
    assert_eq!(rules.mission_control.rate_frames(MissionType::Stop), 14);
    assert_eq!(mission_base_frames(&rules, MissionType::Stop, 99), 14);
}

/// When the per-unit `ChronoInSound=` / `ChronoOutSound=` are absent, the
/// resolver must fall back to the `[General]` values from Rules. Confirms
/// the two-level lookup matches the original engine's behavior.
#[test]
fn chrono_teleport_sound_falls_back_to_rules_general() {
    use crate::sim::world::SimSoundEvent;

    let mut sim = Simulation::new();
    let rules = miner_rules_fallback_only();

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::Chrono, 80, 80);
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

    sim.sound_events.clear();
    tick_miners_n(&mut sim, &rules, 1);

    let chrono_events: Vec<_> = sim
        .sound_events
        .iter()
        .filter(|e| matches!(e, SimSoundEvent::ChronoTeleport { .. }))
        .collect();

    assert_eq!(
        chrono_events.len(),
        2,
        "fallback path should emit one ChronoOut and one ChronoIn sound"
    );
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

    // Set up: miner knows about ore, state = MoveToOre.
    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        let miner = entity.miner.as_mut().expect("miner component");
        miner.target_ore_cell = Some((12, 10));
        entity
            .mission
            .set_handler_state(MinerState::MoveToOre.cursor());
    }

    // After one tick, chrono miner should NOT have a teleport — it drives.
    tick_miners_n(&mut sim, &rules, 1);

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
    let wait = u32::from(config.rescan_cooldown_ticks);
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
        entity
            .miner
            .as_mut()
            .expect("miner component")
            .rescan_cooldown
            .arm(start_frame, wait);
    }

    // rescan_cooldown_ticks = 105 (0x69 frames from the original engine).
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
        entity.miner.as_ref().expect("miner").target_ore_cell,
        None,
        "state 4 does not look at the ore that appeared during the wait",
    );

    // Until the host promotes the queue, every further dispatch re-runs
    // state 4 on the Rate cadence — still no scan.
    let base = super::miner_dock_sequence::mission_base_frames(
        &rules,
        crate::sim::mission::MissionType::Harvest,
        super::miner_system::HARVEST_RATE_FALLBACK_FRAMES,
    );
    tick_miners_n(
        &mut sim,
        &rules,
        usize::from(base) + super::miner_system::RATE_EPILOGUE_JITTER_MAX_FRAMES as usize,
    );
    let m = get_miner(&sim, miner_id);
    assert_eq!(m.state, MinerState::WaitNoOre);
    assert_eq!(m.target_ore_cell, None);
}

// ==========================================================================
// Test 14b (L6): the no-ore retry gate is armed for exactly
// rescan_cooldown_ticks (0x69 = 105) frames — the production arm site must
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
    assert_eq!(
        miner.rescan_cooldown.duration,
        u32::from(config.rescan_cooldown_ticks),
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

    tick_miners_n(&mut sim, &rules, 1);

    let miner = get_miner(&sim, miner_id);
    assert_eq!(miner.reserved_refinery, Some(3));
    assert_eq!(miner.state, MinerState::ReturnToRefinery);
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

    tick_miners_n(&mut sim, &rules, 1);

    assert_eq!(get_miner(&sim, miner_id).reserved_refinery, Some(3));
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
    let dock = super::miner_system::refinery_dock_cell(10, 10, 4, 3, None);
    assert_eq!(dock, (13, 11));
}

// ==========================================================================
// Dock sequence tests
// ==========================================================================

/// Verify the dock sequence progresses through all phases when given enough ticks.
#[test]
fn dock_sequence_progresses_through_phases() {
    let mut sim = Simulation::new();
    let rules = miner_rules();

    // Miner at queue cell (14, 11), refinery at (10, 10) with 4x3 foundation.
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 14, 11);
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
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Approach;
        miner.reserved_refinery = Some(2);
    }

    // Tick 1: Approach -> MissionEnter. HELLO has populated Contacts[], but
    // CAN_DOCK has not yet issued the accepted-cell move.
    tick_miners_n(&mut sim, &rules, 1);
    let m = get_miner(&sim, miner_id);
    assert_eq!(m.dock_phase, RefineryDockPhase::MissionEnter);

    // Tick enough for movement onto pad + per-bale unload + state-4 handoff.
    // 1 bale * 14.4 ticks/bale is about 15 ticks unload, plus pad entry.
    tick_miners_n(&mut sim, &rules, 200);
    let m = get_miner(&sim, miner_id);
    // With only 1 bale (unload_tick_interval=14), unloading takes ~15 ticks.
    // After that, Departing -> SearchOre.
    // After docking, miner transitions to SearchOre. Since there's no ore
    // on the map, it immediately goes to WaitNoOre. Both are valid endpoints.
    assert!(
        m.state == MinerState::SearchOre || m.state == MinerState::WaitNoOre,
        "Miner should complete dock sequence, got state={:?} phase={:?}",
        m.state,
        m.dock_phase,
    );
}

/// Verify the Approach phase grants HELLO contact when free and queues
/// Mission_Enter instead of immediately linking/unloading.
#[test]
fn dock_wait_grants_reservation_when_free() {
    let mut sim = Simulation::new();
    let rules = miner_rules();

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 14, 11);
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
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Approach;
        miner.reserved_refinery = Some(2);
    }

    tick_miners_n(&mut sim, &rules, 1);

    // Contacts[] should contain this miner; pad/contact-entered state has not
    // started yet because CAN_DOCK runs in Mission_Enter.
    assert!(crate::sim::miner::miner_dock::test_support::dock_test_is_occupied(&sim, 2));
    assert!(crate::sim::miner::miner_dock::has_contact(
        &sim, 2, miner_id
    ));
    assert!(!crate::sim::miner::miner_dock::has_entered(
        &sim, 2, miner_id
    ));
    let m = get_miner(&sim, miner_id);
    assert_eq!(m.dock_phase, RefineryDockPhase::MissionEnter);
    assert!(!m.dock_queued);
}

/// Verify stock pad cell and conditional reciprocal-link release cell helpers.
#[test]
fn refinery_pad_and_conditional_release_cells() {
    use super::miner_dock_sequence::{
        refinery_can_dock_queue_cell, refinery_exit_cell, refinery_pad_cell, refinery_queue_cell,
    };

    let grid = PathGrid::test_all_passable(64, 64);

    // 4×3 foundation at (10, 10), no art.ini overrides:
    //   queue = (14, 11), pad = (13, 11), conditional release = queue.
    // Stock zero-link unload completion does not call this release helper.
    assert_eq!(refinery_queue_cell(10, 10, 4, 3, None), (14, 11));
    assert_eq!(refinery_pad_cell(10, 10, 4, 3, None), (13, 11));
    assert_eq!(
        refinery_exit_cell(10, 10, 4, 3, None, Some(&grid), None, 0),
        (14, 11),
    );

    // 3×3 foundation at (5, 5), no art.ini overrides:
    //   queue = (8, 6), pad = (8, 6), conditional release = queue.
    assert_eq!(refinery_queue_cell(5, 5, 3, 3, None), (8, 6));
    assert_eq!(refinery_pad_cell(5, 5, 3, 3, None), (8, 6));
    assert_eq!(
        refinery_exit_cell(5, 5, 3, 3, None, Some(&grid), None, 0),
        (8, 6),
    );

    // 2×2 foundation at (20, 20): queue/release = (22, 21).
    assert_eq!(
        refinery_exit_cell(20, 20, 2, 2, None, Some(&grid), None, 0),
        (22, 21)
    );

    // QueueingCell override unchanged:
    assert_eq!(refinery_queue_cell(10, 10, 4, 3, Some((4, 1))), (14, 11));
    assert_eq!(refinery_queue_cell(10, 10, 4, 3, Some((3, 2))), (13, 12));
    assert_eq!(
        refinery_can_dock_queue_cell(10, 10),
        (13, 11),
        "CAN_DOCK receiver target is hardcoded NW+(3,1), not art QueueingCell=4,1",
    );

    // Fallback: no path grid → return QueueingCell.
    assert_eq!(
        refinery_exit_cell(10, 10, 4, 3, None, None, None, 0),
        (14, 11)
    );
    assert_eq!(
        refinery_exit_cell(10, 10, 4, 3, Some((3, 2)), None, None, 0),
        (13, 12)
    );
}

/// Verify the Unloading phase awards credits like the old handle_unload.
#[test]
fn dock_unloading_phase_awards_credits() {
    let mut sim = Simulation::new();
    let rules = miner_rules();

    // Place miner directly in Unloading phase at pad cell (13, 11).
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 13, 11);
    spawn_refinery(&mut sim, 2, 10, 10);

    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        let miner = entity.miner.as_mut().expect("miner component");
        for _ in 0..5 {
            miner.cargo.push(CargoBale {
                resource_type: ResourceType::Ore,
                value: 25,
            });
        }
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Unloading;
        miner.reserved_refinery = Some(2);
    }

    // Pre-reserve the dock so release works correctly.
    crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, miner_id);

    let before = credits_for_owner(&sim, "Americans");
    // 5 bales × unload_interval=14 = ~70 ticks + margin.
    tick_miners_n(&mut sim, &rules, 100);
    let after = credits_for_owner(&sim, "Americans");

    assert_eq!(after - before, 125, "5 ore bales × 25 = 125 credits");
}

/// gamemd parity: credits from a harvester deposit go to the REFINERY OWNER,
/// not to the harvester's current controller. Simulates a mind-control
/// scenario by spawning a refinery owned by "Americans" and overriding the
/// harvester's owner to "Russians" (as Yuri's mind-control would do). The
/// ore drop must credit "Americans" (the refinery owner), and "Russians"
/// (the harvester's current owner) must see zero delta.
///
/// Verified against `MISSION_DEPLOY_BUILDING_REFINERY_UNLOAD_GHIDRA_REPORT.md`
/// §3d: `vtable+0x3C` (GetOwner on the building, address `EBX` in the
/// disassembly) is used as the credits recipient, then
/// `HouseClass__Add_Tiberium_Credits` at `0x004F9610` adds to that house.
#[test]
fn unloading_credits_refinery_owner_under_mind_control() {
    let mut sim = Simulation::new();
    let rules = miner_rules();

    // Refinery owned by Americans.
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 13, 11);
    spawn_refinery(&mut sim, 2, 10, 10);

    // The refinery admitted the miner while both were Americans.
    assert!(crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, miner_id));

    // Mind-control: rewrite the harvester's owner to a different house.
    let mc_owner = sim.interner.intern("Russians");
    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        entity.owner = mc_owner;
        let miner = entity.miner.as_mut().expect("miner component");
        for _ in 0..5 {
            miner.cargo.push(CargoBale {
                resource_type: ResourceType::Ore,
                value: 25,
            });
        }
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Unloading;
        miner.reserved_refinery = Some(2);
    }

    let americans_before = credits_for_owner(&sim, "Americans");
    let russians_before = credits_for_owner(&sim, "Russians");
    tick_miners_n(&mut sim, &rules, 100);
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

/// Verify that after unloading finishes, the miner exits and returns to SearchOre.
#[test]
fn dock_exit_returns_to_search_ore() {
    let mut sim = Simulation::new();
    let rules = miner_rules();

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 13, 11);
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
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Unloading;
        miner.reserved_refinery = Some(2);
    }

    crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, miner_id);

    // Tick enough for unload (1 bale at 14 ticks), state-4 handoff, and
    // SearchOre/WaitNoOre with margin.
    tick_miners_n(&mut sim, &rules, 150);

    let m = get_miner(&sim, miner_id);
    // After unloading, miner goes to SearchOre → WaitNoOre (no ore on map).
    assert!(
        m.state == MinerState::SearchOre || m.state == MinerState::WaitNoOre,
        "Should finish dock sequence, got {:?}",
        m.state,
    );
    assert_eq!(m.home_refinery, Some(2), "Home refinery should be set");
    assert!(m.cargo.is_empty(), "Cargo should be empty");
}

/// After Departing arrival: `target_ore_cell` is cleared (the pending
/// pre-dock target has been consumed), but `last_harvest_cell` is
/// PRESERVED — gamemd's `+0x218` ghost-cell archive survives the
/// entire dock cycle (Mission_Deploy_Building and UndockUnit leave
/// it untouched), so the next SearchOre can return directly to the
/// nearby productive patch saved when this miner became full.
#[test]
fn exit_pad_preserves_archive_on_arrival() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let config = MinerConfig::default();
    let path_grid = PathGrid::new(64, 64);

    // 4×3 refinery at (10, 10). Place miner at queue cell (14, 11) and
    // pre-cache that as the exit cell so the test exercises the arrival
    // contract without depending on the spiral-search result for this
    // specific test grid.
    spawn_refinery(&mut sim, 100, 10, 10);
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::Chrono, 14, 11);

    // Set up the miner mid-Departing with an archive populated (as if a
    // prior State 1 full-path saved a nearby productive patch).
    let entity = sim
        .substrate
        .entities
        .get_mut(miner_id)
        .expect("miner entity");
    let miner = entity.miner.as_mut().expect("miner component");
    entity.mission.set_handler_state(MinerState::Dock.cursor());
    miner.dock_phase = RefineryDockPhase::Departing;
    miner.reserved_refinery = Some(100);
    miner.dock_queued = false;
    miner.target_ore_cell = Some((20, 20)); // pre-dock target
    miner.last_harvest_cell = Some((20, 20)); // archive from State 1 full-path
    miner.exit_cell = Some((14, 11)); // pre-cache exit to (14, 11) where miner is placed

    // Tick the miner system — should detect arrival and run the cleanup.
    crate::sim::miner::miner_system::tick_miners(&mut sim, &rules, &config, Some(&path_grid));

    let entity = sim.substrate.entities.get(miner_id).expect("miner entity");
    let miner = entity.miner.as_ref().expect("miner component");
    assert_eq!(
        entity.miner_state().unwrap(),
        MinerState::SearchOre,
        "must transition to SearchOre"
    );
    assert!(
        miner.target_ore_cell.is_none(),
        "target_ore_cell must be cleared (pre-dock target consumed)"
    );
    assert_eq!(
        miner.last_harvest_cell,
        Some((20, 20)),
        "last_harvest_cell (archive) must SURVIVE the dock cycle — \
         gamemd's +0x218 is untouched by the unload state machine",
    );
    assert!(
        miner.reserved_refinery.is_none(),
        "reserved_refinery must be cleared"
    );
}

/// Departing must NOT transition to SearchOre while a teleport is in progress
/// (`entity.teleport_state.is_some()`). Without this gate a chrono miner
/// mid-warp could leave the dock sub-state machine prematurely.
#[test]
fn exit_pad_blocks_transition_during_teleport() {
    use crate::sim::movement::teleport_movement::{TeleportPhase, TeleportState};

    let mut sim = Simulation::new();
    let rules = miner_rules();
    let config = MinerConfig::default();
    let path_grid = PathGrid::new(64, 64);

    spawn_refinery(&mut sim, 100, 10, 10);
    // Exit cell for the 4×3 refinery at (10, 10) is the queue cell (14, 11)
    // — the cell directly outside the pad, on the same axis the miner
    // entered through.
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::Chrono, 14, 11);

    // Set up miner at the exit cell, in Departing, with a teleport in progress.
    let entity = sim
        .substrate
        .entities
        .get_mut(miner_id)
        .expect("miner entity");
    let miner = entity.miner.as_mut().expect("miner component");
    entity.mission.set_handler_state(MinerState::Dock.cursor());
    miner.dock_phase = RefineryDockPhase::Departing;
    miner.reserved_refinery = Some(100);
    miner.target_ore_cell = Some((20, 20));
    // Inject an active teleport state to trip the gate.
    entity.teleport_state = Some(TeleportState {
        phase: TeleportPhase::ChronoDelay,
        target_rx: 20,
        target_ry: 20,
        being_warped_ticks: 16,
    });

    crate::sim::miner::miner_system::tick_miners(&mut sim, &rules, &config, Some(&path_grid));

    let entity = sim.substrate.entities.get(miner_id).expect("miner entity");
    let miner = entity.miner.as_ref().expect("miner component");
    assert_eq!(
        entity.miner_state().unwrap(),
        MinerState::Dock,
        "must stay in Dock state during teleport"
    );
    assert_eq!(
        miner.dock_phase,
        RefineryDockPhase::Departing,
        "must stay in Departing"
    );
    assert_eq!(
        miner.target_ore_cell,
        Some((20, 20)),
        "ore target must NOT be cleared while teleport is active"
    );
}

/// Smoke test for the post-undock flow with a stale archive.
///
/// Sets up a chrono miner mid-Departing with `last_harvest_cell` pointing
/// to a depleted cell. The archive survives the dock cycle (gamemd parity)
/// but the next SearchOre's archive-consumption check sees no ore at the
/// archived location, clears the archive, and falls through to the long
/// scan which finds the only patch on the map.
#[test]
fn chrono_miner_archive_cleared_after_undock_picks_new_target() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let config = MinerConfig::default();
    let path_grid = PathGrid::new(64, 64);

    // Refinery at (10, 10), 4x3 foundation. Exit cell = queue cell (14, 11).
    spawn_refinery(&mut sim, 100, 10, 10);

    // Place ONE ore patch at (13, 13): within local_continuation_radius
    // (default 6) of exit cell (14, 11). This is what the fresh local scan
    // from current position should pick.
    crate::sim::tiberium::test_support::place_stock_amount(
        &mut sim,
        (13, 13),
        ResourceType::Ore,
        1200,
    );

    // Spawn miner at exit cell (14, 11), mid-Departing. Stale archive points
    // far away (50, 50) — outside any scan radius from current position,
    // and no ore at that cell. If the archive were NOT cleared, the search
    // would start from (50, 50), the local scan would find nothing, the
    // archive check would also find nothing, and only the long scan would
    // eventually fall back to current position. With the fix the local scan
    // from current position immediately picks (13, 13).
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::Chrono, 14, 11);
    let entity = sim
        .substrate
        .entities
        .get_mut(miner_id)
        .expect("miner entity");
    let miner = entity.miner.as_mut().expect("miner component");
    entity.mission.set_handler_state(MinerState::Dock.cursor());
    miner.dock_phase = RefineryDockPhase::Departing;
    miner.reserved_refinery = Some(100);
    miner.target_ore_cell = Some((50, 50));
    miner.last_harvest_cell = Some((50, 50));
    miner.cargo.clear();
    miner.exit_cell = Some((14, 11)); // pre-cache exit to match miner spawn pos

    // Tick twice: (1) Departing → SearchOre with cleared archive,
    // (2) SearchOre → MoveToOre with target picked.
    crate::sim::miner::miner_system::tick_miners(&mut sim, &rules, &config, Some(&path_grid));
    crate::sim::miner::miner_system::tick_miners(&mut sim, &rules, &config, Some(&path_grid));

    let entity = sim.substrate.entities.get(miner_id).expect("miner entity");
    let miner = entity.miner.as_ref().expect("miner component");

    // The stale (50, 50) target must be replaced. Any other value (None or
    // Some((13, 13))) is acceptable — the precise target depends on which
    // tick SearchOre ran in. The key property: the stale archive does not
    // survive the dock cycle.
    assert_ne!(
        miner.target_ore_cell,
        Some((50, 50)),
        "stale archive must be replaced after Departing → SearchOre. \
         Got state={:?}, target={:?}",
        entity.miner_state().unwrap(),
        miner.target_ore_cell,
    );

    // After the second tick, the only available ore should be the picked target.
    if let Some(target) = miner.target_ore_cell {
        assert_eq!(
            target,
            (13, 13),
            "the only ore at (13, 13) should be picked. Got {:?}",
            target
        );
    }
}

/// Ore in a disconnected zone (cut off by impassable terrain) must be
/// filtered out by the reachability check. With no reachable ore on the
/// map, the harvester transitions to WaitNoOre rather than picking the
/// unreachable cell.
#[test]
fn unreachable_ore_filtered_out() {
    use crate::sim::pathfinding::zone_map::ZoneGrid;
    use std::collections::BTreeMap;

    let mut sim = Simulation::new();

    spawn_inert_dock_instance(&mut sim);
    let rules = miner_rules();

    // Build a 16x16 path grid with an impassable wall column at x=8 that
    // splits the map into two zones (left and right halves).
    let mut grid = PathGrid::new(16, 16);
    for y in 0..16u16 {
        grid.set_blocked(8, y, true);
    }
    let zone_grid = ZoneGrid::build(&grid, &BTreeMap::new(), 16, 16);
    sim.zone_grid = Some(zone_grid);

    // Harvester on the LEFT side at (3, 8). Ore on the RIGHT side at (12, 8).
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 3, 8);
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
        m.target_ore_cell.is_none(),
        "must not have targeted unreachable ore, got {:?}",
        m.target_ore_cell,
    );
}

/// When a closer ore cell is unreachable (different zone) but a farther
/// one is reachable, the harvester must pick the farther reachable cell
/// rather than fall through to WaitNoOre.
#[test]
fn reachable_ore_picked_over_closer_unreachable() {
    use crate::sim::pathfinding::zone_map::ZoneGrid;
    use std::collections::BTreeMap;

    let mut sim = Simulation::new();

    spawn_inert_dock_instance(&mut sim);
    let rules = miner_rules();

    // 16x16 grid with an impassable wall column at x=8.
    let mut grid = PathGrid::new(16, 16);
    for y in 0..16u16 {
        grid.set_blocked(8, y, true);
    }
    let zone_grid = ZoneGrid::build(&grid, &BTreeMap::new(), 16, 16);
    sim.zone_grid = Some(zone_grid);

    // Harvester at (3, 8) on the LEFT side.
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 3, 8);
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
    assert_eq!(m.state, MinerState::MoveToOre);
    assert_eq!(
        m.target_ore_cell,
        Some((1, 1)),
        "reachable farther ore at (1,1) must be picked over unreachable closer ore at (10,8). \
         Got {:?}",
        m.target_ore_cell,
    );
}

/// When the harvester is standing on a cell marked impassable in the path
/// grid (mirrors mid-harvest on Tiberium), the effective-zone probe must
/// find a valid zone via a neighbor and the filter must still apply.
/// Specifically: nearby reachable ore is picked, distant unreachable ore
/// is filtered.
#[test]
fn harvester_on_tiberium_falls_back_to_neighbor_zone() {
    use crate::sim::pathfinding::zone_map::ZoneGrid;
    use std::collections::BTreeMap;

    let mut sim = Simulation::new();

    spawn_inert_dock_instance(&mut sim);
    let rules = miner_rules();

    // 16x16 grid. Wall column at x=8 splits LEFT and RIGHT zones.
    // Harvester's cell at (3, 8) is also blocked (simulates standing on
    // Tiberium that the path grid marks impassable).
    let mut grid = PathGrid::new(16, 16);
    for y in 0..16u16 {
        grid.set_blocked(8, y, true);
    }
    grid.set_blocked(3, 8, true);
    let zone_grid = ZoneGrid::build(&grid, &BTreeMap::new(), 16, 16);
    sim.zone_grid = Some(zone_grid);

    // Harvester at (3, 8) on the blocked cell.
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 3, 8);
    // Reachable ore at (5, 8) on the LEFT side.
    place_ore(&mut sim, 5, 8, 1200);
    // Unreachable ore at (10, 8) on the RIGHT side.
    place_ore(&mut sim, 10, 8, 1200);

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
    assert_eq!(m.state, MinerState::MoveToOre);
    assert_eq!(
        m.target_ore_cell,
        Some((5, 8)),
        "left-side reachable ore must be picked even with the harvester on a \
         blocked cell — the effective-zone probe finds a passable neighbor. \
         Got {:?}",
        m.target_ore_cell,
    );
}

/// End-to-end pin for the head-butt-after-unload fix. Exercises stock
/// state-4 handoff -> SearchOre -> A* from a blocked-start cell -> MoveToOre.
/// Uses a real PathGrid with the refinery foundation blocked, so the test
/// would fail without the A* start-relaxation.
#[test]
fn harvester_undocks_through_foundation_to_outside_ore() {
    use crate::map::houses::HouseAllianceMap;
    use crate::sim::pathfinding::PathGrid;
    use crate::sim::rng::SimRng;
    use std::collections::BTreeMap;

    let mut sim = Simulation::new();
    let rules = miner_rules();
    let config = MinerConfig::default();

    // 4x3 GAREFN at (10, 10) — foundation occupies (10..=13, 10..=12).
    spawn_refinery(&mut sim, 100, 10, 10);

    // Ore patch at (11, 14) — south of the foundation, reachable once the
    // harvester clears the south edge.
    place_ore(&mut sim, 11, 14, 1200);

    // PathGrid with the foundation footprint blocked. This is the critical
    // setup that makes the test meaningful: SearchOre must be able to path
    // from the blocked pad start after state-4 handoff.
    let mut path_grid = PathGrid::new(32, 32);
    path_grid.block_building_footprint(10, 10, "4x3", &[], &[], false);

    // Harvester at the dock pad (13, 11), cargo emptied, dock_phase=Departing.
    // Simulates "just finished unloading".
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 13, 11);
    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("harvester entity");
        let miner = entity.miner.as_mut().expect("miner component");
        miner.cargo.clear();
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Departing;
        miner.reserved_refinery = Some(100);
    }
    crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 100, miner_id);

    // Tick the full pipeline: miner state machine + movement with the
    // blocked-footprint path_grid. Use enough ticks for state-4 handoff,
    // SearchOre, A*, and drive south toward ore.
    let alliances = HouseAllianceMap::new();
    let terrain_costs = BTreeMap::new();
    let mut occupancy = OccupancyGrid::new();
    let mut rng = SimRng::new(0);

    // Phase A: tick until the miner exits the Dock state. This is when the
    // stock state-4 handoff clears reserved_refinery. Asserting at that
    // exact tick avoids racing the
    // subsequent harvest cycle (which legitimately re-reserves the
    // refinery once the cell is drained).
    //
    // The handoff should happen immediately; 200 ticks is a comfortable
    // upper bound that also covers subsequent search-ore movement if timing
    // changes.
    let mut departed_at: Option<usize> = None;
    let mut reservation_observed_clear = false;
    for tick in 0..200 {
        crate::sim::miner::miner_system::tick_miners(&mut sim, &rules, &config, Some(&path_grid));
        crate::sim::movement::tick_movement_with_grid(
            &mut sim.substrate.entities,
            Some(&path_grid),
            &terrain_costs,
            &alliances,
            &mut occupancy,
            &mut rng,
            sim.session.tick,
            &mut sim.interner,
            &mut sim.pending_lifecycle_requests,
        );
        sim.session.tick += 1;
        // Advance the frame clock so the L10 post-unload resume jitter (and the
        // other dock cadence timers) become due across the run.
        sim.session.binary_frame += 1;

        let miner = sim
            .substrate
            .entities
            .get(miner_id)
            .and_then(|e| e.miner.as_ref())
            .expect("miner alive");
        if sim.substrate.entities.get(miner_id).unwrap().miner_state() != Some(MinerState::Dock) {
            if departed_at.is_none() {
                departed_at = Some(tick);
                // phase_departing's arrival branch clears reserved_refinery
                // before transitioning state; observe it exactly here.
                reservation_observed_clear = miner.reserved_refinery.is_none();
            }
            break;
        }
    }
    assert!(
        departed_at.is_some(),
        "harvester should have transitioned out of Dock within 60 ticks",
    );
    assert!(
        reservation_observed_clear,
        "phase_departing should have cleared reserved_refinery when state left Dock",
    );

    // Phase B: continue ticking. The miner now runs SearchOre → MoveToOre
    // toward the ore patch, proving the foundation-blocked path_grid did
    // not strand it on the pad. After enough ticks it either reaches the
    // ore cell or is in transit toward it.
    for _ in 0..120 {
        crate::sim::miner::miner_system::tick_miners(&mut sim, &rules, &config, Some(&path_grid));
        crate::sim::movement::tick_movement_with_grid(
            &mut sim.substrate.entities,
            Some(&path_grid),
            &terrain_costs,
            &alliances,
            &mut occupancy,
            &mut rng,
            sim.session.tick,
            &mut sim.interner,
            &mut sim.pending_lifecycle_requests,
        );
        sim.session.tick += 1;
        sim.session.binary_frame += 1;
    }

    let entity = sim
        .substrate
        .entities
        .get(miner_id)
        .expect("harvester still alive");
    let miner = entity.miner.as_ref().expect("miner component");

    // Harvester either escaped the foundation south edge, is targeting the
    // ore patch, or has already harvested it and started returning — any
    // of these proves SearchOre + A* succeeded from the (formerly blocked)
    // pad cell.
    let escaped = entity.position.ry > 12 || entity.position.rx < 10 || entity.position.rx > 13;
    let targeting = miner.target_ore_cell == Some((11, 14));
    let returning = matches!(
        entity.miner_state().unwrap(),
        MinerState::ReturnToRefinery | MinerState::Dock
    );
    assert!(
        escaped || targeting || returning,
        "harvester should have escaped foundation, be targeting ore, or be \
         returning after harvest; pos=({},{}) target_ore={:?} state={:?}",
        entity.position.rx,
        entity.position.ry,
        miner.target_ore_cell,
        entity.miner_state().unwrap(),
    );
}

/// End-to-end pin for the foundation-bump bug. Places a refinery at (10, 10)
/// with its foundation cells registered in OccupancyGrid (the real-game
/// configuration), then drives a harvester into the pad. Asserts the refinery's
/// position is unchanged and it never receives a movement_target — i.e. the
/// bypass_grid filter prevents the building from being treated as a scatter
/// candidate when the harvester crosses into a foundation cell.
#[test]
fn harvester_drives_into_refinery_foundation_without_bumping_it() {
    use crate::map::houses::HouseAllianceMap;
    use crate::sim::movement::locomotor::MovementLayer;
    use crate::sim::occupancy::OccupancyGrid;
    use crate::sim::pathfinding::PathGrid;
    use crate::sim::rng::SimRng;
    use std::collections::BTreeMap;

    let mut sim = Simulation::new();
    let rules = miner_rules();
    let config = MinerConfig::default();

    // 4x3 GAREFN at (10, 10) — foundation occupies (10..=13, 10..=12).
    // spawn_refinery returns (); EntityStore is keyed by stable_id, so we use
    // the sid we passed in (100) as the entity_id directly.
    spawn_refinery(&mut sim, 100, 10, 10);
    let refinery_id: u64 = 100;
    // Capture initial position fields. Position is Clone but not Copy, so we
    // can't `let p = entity.position` through a borrow — read individual
    // fields into primitives instead.
    let (rx_before, ry_before, sub_x_before, sub_y_before) = {
        let r = sim
            .substrate
            .entities
            .get(refinery_id)
            .expect("refinery just spawned");
        (
            r.position.rx,
            r.position.ry,
            r.position.sub_x,
            r.position.sub_y,
        )
    };

    // Register foundation cells in OccupancyGrid (the real-game configuration —
    // this is what the existing undock test omits, which is why it didn't catch
    // the bump bug).
    let mut occupancy = OccupancyGrid::new();
    for ry in 10u16..=12 {
        for rx in 10u16..=13 {
            occupancy.add(
                rx,
                ry,
                refinery_id,
                MovementLayer::Ground,
                None,
                CellListInsertion::AppendBuilding,
            );
        }
    }

    let mut path_grid = PathGrid::new(32, 32);
    path_grid.block_building_footprint(10, 10, "4x3", &[], &[], false);

    // Harvester at queue cell (14, 11), state=Dock, dock_phase=Approach.
    // Reservation already held; first tick re-targets the pad and goes Linked.
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 14, 11);
    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("harvester entity");
        let miner = entity.miner.as_mut().expect("miner component");
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Approach;
        miner.reserved_refinery = Some(100);
    }
    crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 100, miner_id);

    let alliances = HouseAllianceMap::new();
    let terrain_costs = BTreeMap::new();
    let mut rng = SimRng::new(0);

    // Tick enough for: drive 1 cell west onto the pad. 60 ticks gives plenty of slack.
    for _ in 0..60 {
        crate::sim::miner::miner_system::tick_miners(&mut sim, &rules, &config, Some(&path_grid));
        crate::sim::movement::tick_movement_with_grid(
            &mut sim.substrate.entities,
            Some(&path_grid),
            &terrain_costs,
            &alliances,
            &mut occupancy,
            &mut rng,
            sim.session.tick,
            &mut sim.interner,
            &mut sim.pending_lifecycle_requests,
        );
        sim.session.tick += 1;
        // The dock cadence timers (G5/G6) key on `binary_frame`; advance it so
        // the deferred CAN_DOCK / deploy dispatches become due across the run.
        sim.session.binary_frame += 1;
    }

    let refinery = sim
        .substrate
        .entities
        .get(refinery_id)
        .expect("refinery still alive");

    // (1) Refinery position is exactly unchanged.
    assert_eq!(
        refinery.position.rx, rx_before,
        "refinery rx must not change when harvester docks; got rx={}",
        refinery.position.rx,
    );
    assert_eq!(
        refinery.position.ry, ry_before,
        "refinery ry must not change when harvester docks; got ry={}",
        refinery.position.ry,
    );
    assert_eq!(
        refinery.position.sub_x, sub_x_before,
        "refinery sub_x must not change",
    );
    assert_eq!(
        refinery.position.sub_y, sub_y_before,
        "refinery sub_y must not change",
    );

    // (2) Refinery never received a movement_target.
    assert!(
        refinery.movement_target.is_none(),
        "refinery must not have a movement_target — buildings cannot scatter",
    );

    // (3) Harvester drove past the queue cell. After 60 ticks it should be
    // at the pad cell or further along the dock sequence — definitely not
    // still at queue (14, 11) which would indicate sub-cell oscillation
    // when crossing into a foundation cell.
    let harvester = sim
        .substrate
        .entities
        .get(miner_id)
        .expect("harvester still alive");
    assert_ne!(
        (harvester.position.rx, harvester.position.ry),
        (14u16, 11u16),
        "harvester must have driven past the queue cell into the foundation; \
         oscillating in place at queue means a deferred-occupancy check is \
         bouncing it back. phase={:?}",
        harvester.miner.as_ref().map(|m| m.dock_phase),
    );
}

// ===========================================================================
// Focused pins for the stock refinery inbound radio FSM.
// ===========================================================================

/// Approach sends HELLO first. Mission_Enter/CAN_DOCK runs on a later tick
/// and only then issues movement to the accepted cell.
#[test]
fn hello_before_mission_enter_then_can_dock_move() {
    let mut sim = Simulation::new();
    let rules = miner_rules();

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 14, 11);
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
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Approach;
        miner.reserved_refinery = Some(2);
    }

    tick_miners_n(&mut sim, &rules, 1);

    let m = get_miner(&sim, miner_id);
    assert_eq!(m.dock_phase, RefineryDockPhase::MissionEnter);
    assert!(
        crate::sim::miner::miner_dock::has_contact(&sim, 2, miner_id),
        "HELLO/ROGER should populate Contacts[]"
    );
    assert!(
        !crate::sim::miner::miner_dock::has_entered(&sim, 2, miner_id),
        "0x18/+0x418-style contact-entered flag must not be set by HELLO"
    );
    assert!(
        sim.substrate
            .entities
            .get(miner_id)
            .expect("entity")
            .movement_target
            .is_none(),
        "HELLO acceptance must not issue the CAN_DOCK move in the same tick"
    );

    // G5: the accepted HELLO arms the Enter cadence; advance the frame clock
    // past the ~14-16f window so the next pass's CAN_DOCK dispatch is due.
    sim.session.binary_frame = sim.session.binary_frame.wrapping_add(18);
    tick_miners_n(&mut sim, &rules, 1);

    let m = get_miner(&sim, miner_id);
    assert_eq!(m.dock_phase, RefineryDockPhase::AwaitingAcceptedCell);
    assert!(
        !crate::sim::miner::miner_dock::has_entered(&sim, 2, miner_id),
        "not at accepted cell yet: no 0x18/0x16 admission"
    );

    let entity = sim.substrate.entities.get(miner_id).expect("entity");
    let accepted_cell_move_issued = entity
        .movement_target
        .as_ref()
        .and_then(|target| target.path.last().copied())
        == Some((13, 11));
    assert!(
        accepted_cell_move_issued || (entity.position.rx, entity.position.ry) == (13, 11),
        "CAN_DOCK should move toward accepted cell (13,11)"
    );
}

/// Reaching the accepted cell only satisfies the move requested by 0x12. The
/// pivot/link handshake starts on the next Mission_Enter pass, when 0x12
/// returns already-there.
#[test]
fn accepted_cell_arrival_rechecks_can_dock_before_entered_flag() {
    let mut sim = Simulation::new();
    let rules = miner_rules();

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 13, 11);
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
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::AwaitingAcceptedCell;
        miner.reserved_refinery = Some(2);
    }
    assert!(crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, miner_id));

    tick_miners_n(&mut sim, &rules, 1);

    let m = get_miner(&sim, miner_id);
    assert_eq!(
        m.dock_phase,
        RefineryDockPhase::MissionEnter,
        "arrival at accepted cell must re-enter CAN_DOCK before pivot/link"
    );
    assert!(
        !crate::sim::miner::miner_dock::has_entered(&sim, 2, miner_id),
        "accepted-cell movement alone must not set the entered flag"
    );

    tick_miners_n(&mut sim, &rules, 1);

    let m = get_miner(&sim, miner_id);
    assert_eq!(m.dock_phase, RefineryDockPhase::FaceSync);
    assert!(
        crate::sim::miner::miner_dock::has_entered(&sim, 2, miner_id),
        "the next already-there 0x12 pass starts the entered handshake"
    );
}

#[test]
fn waiter_moves_from_queueingcell_to_accepted_cell_before_entered() {
    let mut sim = Simulation::new();
    let rules = miner_rules();

    let waiter = spawn_miner(&mut sim, 1, MinerKind::War, 14, 11);
    spawn_refinery(&mut sim, 2, 10, 10);

    {
        let entity = sim
            .substrate
            .entities
            .get_mut(waiter)
            .expect("waiter entity");
        let miner = entity.miner.as_mut().expect("waiter miner");
        miner.cargo.push(CargoBale {
            resource_type: ResourceType::Ore,
            value: 25,
        });
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::MissionEnter;
        miner.reserved_refinery = Some(2);
    }

    tick_miners_n(&mut sim, &rules, 1);

    let waiter_miner = get_miner(&sim, waiter);
    assert_eq!(
        waiter_miner.dock_phase,
        RefineryDockPhase::AwaitingAcceptedCell
    );
    assert!(crate::sim::miner::miner_dock::has_contact(&sim, 2, waiter));
    assert!(
        !crate::sim::miner::miner_dock::has_entered(&sim, 2, waiter),
        "QueueingCell position must not count as entered"
    );
    let entity = sim.substrate.entities.get(waiter).expect("waiter entity");
    let accepted_cell_move_issued = entity
        .movement_target
        .as_ref()
        .and_then(|target| target.path.last().copied())
        == Some((13, 11));
    assert!(
        accepted_cell_move_issued || (entity.position.rx, entity.position.ry) == (13, 11),
        "CAN_DOCK should move from QueueingCell (14,11) to accepted cell (13,11)"
    );

    {
        let entity = sim
            .substrate
            .entities
            .get_mut(waiter)
            .expect("waiter entity");
        entity.position.rx = 13;
        entity.position.ry = 11;
        entity.movement_target = None;
    }

    tick_miners_n(&mut sim, &rules, 1);

    let waiter_miner = get_miner(&sim, waiter);
    assert_eq!(
        waiter_miner.dock_phase,
        RefineryDockPhase::MissionEnter,
        "accepted-cell move completion must re-enter CAN_DOCK before linking"
    );
    assert!(!crate::sim::miner::miner_dock::has_entered(&sim, 2, waiter));

    tick_miners_n(&mut sim, &rules, 16);

    let waiter_miner = get_miner(&sim, waiter);
    assert_eq!(waiter_miner.dock_phase, RefineryDockPhase::FaceSync);
    assert!(crate::sim::miner::miner_dock::has_entered(&sim, 2, waiter));
}

#[test]
fn occupied_can_dock_defers_without_clearing_waiting_miner_target() {
    let mut sim = Simulation::new();
    let rules = miner_rules();

    let occupant = spawn_miner(&mut sim, 1, MinerKind::War, 13, 11);
    let waiter = spawn_miner(&mut sim, 3, MinerKind::War, 13, 11);
    spawn_refinery(&mut sim, 2, 10, 10);

    {
        let entity = sim
            .substrate
            .entities
            .get_mut(occupant)
            .expect("occupant entity");
        let miner = entity.miner.as_mut().expect("occupant miner");
        miner.cargo.push(CargoBale {
            resource_type: ResourceType::Ore,
            value: 25,
        });
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Unloading;
        miner.reserved_refinery = Some(2);
    }
    assert!(crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, occupant));
    crate::sim::miner::miner_dock::enter_dock(&mut sim, occupant, 2);

    {
        let entity = sim
            .substrate
            .entities
            .get_mut(waiter)
            .expect("waiter entity");
        let miner = entity.miner.as_mut().expect("waiter miner");
        miner.cargo.push(CargoBale {
            resource_type: ResourceType::Ore,
            value: 25,
        });
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::MissionEnter;
        miner.reserved_refinery = Some(2);
    }

    tick_miners_n(&mut sim, &rules, 1);

    let miner = get_miner(&sim, waiter);
    assert_eq!(miner.state, MinerState::Dock);
    assert_eq!(
        miner.dock_phase,
        RefineryDockPhase::MissionEnter,
        "busy CAN_DOCK should defer in MissionEnter, not clear the target or enter"
    );
    assert_eq!(miner.reserved_refinery, Some(2));
    assert!(miner.dock_queued);
    assert!(!crate::sim::miner::miner_dock::has_contact(&sim, 2, waiter));
    assert!(!crate::sim::miner::miner_dock::has_entered(&sim, 2, waiter));
}

#[test]
fn queued_miner_enters_after_contact_and_pad_are_released() {
    let mut sim = Simulation::new();
    let rules = miner_rules();

    let occupant = spawn_miner(&mut sim, 1, MinerKind::War, 13, 11);
    let waiter = spawn_miner(&mut sim, 3, MinerKind::War, 13, 11);
    spawn_refinery(&mut sim, 2, 10, 10);

    assert!(crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, occupant));
    crate::sim::miner::miner_dock::enter_dock(&mut sim, occupant, 2);

    {
        let entity = sim
            .substrate
            .entities
            .get_mut(waiter)
            .expect("waiter entity");
        let miner = entity.miner.as_mut().expect("waiter miner");
        miner.cargo.push(CargoBale {
            resource_type: ResourceType::Ore,
            value: 25,
        });
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::MissionEnter;
        miner.reserved_refinery = Some(2);
    }

    tick_miners_n(&mut sim, &rules, 1);
    assert_eq!(
        get_miner(&sim, waiter).dock_phase,
        RefineryDockPhase::MissionEnter,
        "precondition: occupied pad defers the queued miner"
    );

    crate::sim::miner::miner_dock::break_contact(&mut sim, occupant, 2);

    tick_miners_n(&mut sim, &rules, 16);

    let miner = get_miner(&sim, waiter);
    assert_eq!(miner.dock_phase, RefineryDockPhase::FaceSync);
    assert!(!miner.dock_queued);
    assert!(crate::sim::miner::miner_dock::has_contact(&sim, 2, waiter));
    assert!(crate::sim::miner::miner_dock::has_entered(&sim, 2, waiter));
}

#[test]
fn two_miners_waiter_after_releaser_same_tick_claims_on_own_mission_enter() {
    let mut sim = Simulation::new();
    let rules = miner_rules();

    let occupant = spawn_miner(&mut sim, 1, MinerKind::War, 13, 11);
    let waiter = spawn_miner(&mut sim, 3, MinerKind::War, 13, 11);
    spawn_refinery(&mut sim, 2, 10, 10);

    {
        let entity = sim
            .substrate
            .entities
            .get_mut(occupant)
            .expect("occupant entity");
        let miner = entity.miner.as_mut().expect("occupant miner");
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Departing;
        miner.reserved_refinery = Some(2);
    }
    assert!(crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, occupant));
    crate::sim::miner::miner_dock::enter_dock(&mut sim, occupant, 2);

    {
        let entity = sim
            .substrate
            .entities
            .get_mut(waiter)
            .expect("waiter entity");
        let miner = entity.miner.as_mut().expect("waiter miner");
        miner.cargo.push(CargoBale {
            resource_type: ResourceType::Ore,
            value: 25,
        });
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::MissionEnter;
        miner.reserved_refinery = Some(2);
        miner.dock_queued = true;
    }
    assert_eq!(
        crate::sim::miner::miner_dock::hello(&mut sim, waiter, 2, 1),
        crate::sim::miner::miner_dock::ContactAdmission::Waiting
    );

    tick_miners_n(&mut sim, &rules, 1);

    let occupant_miner = get_miner(&sim, occupant);
    assert_eq!(occupant_miner.state, MinerState::SearchOre);
    assert!(!crate::sim::miner::miner_dock::has_contact(
        &sim, 2, occupant
    ));

    let waiter_miner = get_miner(&sim, waiter);
    assert_eq!(
        waiter_miner.dock_phase,
        RefineryDockPhase::FaceSync,
        "mission-dispatch-eligible waiter should claim only during its own MissionEnter pass"
    );
    assert!(!waiter_miner.dock_queued);
    assert!(crate::sim::miner::miner_dock::has_contact(&sim, 2, waiter));
    assert!(crate::sim::miner::miner_dock::has_entered(&sim, 2, waiter));

    let occupant_entity = sim
        .substrate
        .entities
        .get(occupant)
        .expect("occupant entity");
    assert!(!has_bunker_release_track(occupant_entity));
    assert!(occupant_entity.movement_target.is_none());
}

#[test]
#[ignore = "WIP: miner dock release sequence not yet landed"]
fn two_miners_waiter_after_releaser_approach_hello_only() {
    let mut sim = Simulation::new();
    let rules = miner_rules();

    let occupant = spawn_miner(&mut sim, 1, MinerKind::War, 13, 11);
    let waiter = spawn_miner(&mut sim, 3, MinerKind::War, 14, 11);
    spawn_refinery(&mut sim, 2, 10, 10);

    {
        let entity = sim
            .substrate
            .entities
            .get_mut(occupant)
            .expect("occupant entity");
        let miner = entity.miner.as_mut().expect("occupant miner");
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Departing;
        miner.reserved_refinery = Some(2);
    }
    assert!(crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, occupant));
    crate::sim::miner::miner_dock::enter_dock(&mut sim, occupant, 2);

    {
        let entity = sim
            .substrate
            .entities
            .get_mut(waiter)
            .expect("waiter entity");
        let miner = entity.miner.as_mut().expect("waiter miner");
        miner.cargo.push(CargoBale {
            resource_type: ResourceType::Ore,
            value: 25,
        });
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Approach;
        miner.reserved_refinery = Some(2);
        miner.dock_queued = true;
    }
    assert_eq!(
        crate::sim::miner::miner_dock::hello(&mut sim, waiter, 2, 1),
        crate::sim::miner::miner_dock::ContactAdmission::Waiting
    );

    tick_miners_n(&mut sim, &rules, 16);

    let waiter_miner = get_miner(&sim, waiter);
    assert_eq!(
        waiter_miner.dock_phase,
        RefineryDockPhase::MissionEnter,
        "Approach must perform only HELLO, even after an earlier same-tick release"
    );
    assert!(!waiter_miner.dock_queued);
    assert!(crate::sim::miner::miner_dock::has_contact(&sim, 2, waiter));
    assert!(!crate::sim::miner::miner_dock::has_entered(&sim, 2, waiter));
    assert!(
        sim.substrate
            .entities
            .get(waiter)
            .expect("waiter entity")
            .movement_target
            .is_none(),
        "HELLO acceptance must not collapse into CAN_DOCK movement"
    );

    tick_miners_n(&mut sim, &rules, 1);

    let waiter_miner = get_miner(&sim, waiter);
    assert_eq!(waiter_miner.dock_phase, RefineryDockPhase::FaceSync);
    assert!(crate::sim::miner::miner_dock::has_entered(&sim, 2, waiter));
}

#[test]
fn two_miners_waiter_before_releaser_not_retroactively_promoted() {
    let mut sim = Simulation::new();
    let rules = miner_rules();

    let waiter = spawn_miner(&mut sim, 1, MinerKind::War, 13, 11);
    spawn_refinery(&mut sim, 2, 10, 10);
    let occupant = spawn_miner(&mut sim, 3, MinerKind::War, 13, 11);

    {
        let entity = sim
            .substrate
            .entities
            .get_mut(waiter)
            .expect("waiter entity");
        let miner = entity.miner.as_mut().expect("waiter miner");
        miner.cargo.push(CargoBale {
            resource_type: ResourceType::Ore,
            value: 25,
        });
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::MissionEnter;
        miner.reserved_refinery = Some(2);
        miner.dock_queued = true;
    }

    {
        let entity = sim
            .substrate
            .entities
            .get_mut(occupant)
            .expect("occupant entity");
        let miner = entity.miner.as_mut().expect("occupant miner");
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Departing;
        miner.reserved_refinery = Some(2);
    }
    assert!(crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, occupant));
    crate::sim::miner::miner_dock::enter_dock(&mut sim, occupant, 2);
    assert_eq!(
        crate::sim::miner::miner_dock::hello(&mut sim, waiter, 2, 1),
        crate::sim::miner::miner_dock::ContactAdmission::Waiting
    );

    tick_miners_n(&mut sim, &rules, 1);

    let waiter_miner = get_miner(&sim, waiter);
    assert_eq!(
        waiter_miner.dock_phase,
        RefineryDockPhase::MissionEnter,
        "waiter already processed before release; no retroactive promotion"
    );
    assert!(waiter_miner.dock_queued);
    // No wait-queue (V3): "still not admitted" is the only observable state.
    assert!(!crate::sim::miner::miner_dock::has_contact(&sim, 2, waiter));
    assert!(!crate::sim::miner::miner_dock::has_entered(&sim, 2, waiter));
    assert_eq!(get_miner(&sim, occupant).state, MinerState::SearchOre);

    tick_miners_n(&mut sim, &rules, 16);

    let waiter_miner = get_miner(&sim, waiter);
    assert_eq!(
        waiter_miner.dock_phase,
        RefineryDockPhase::FaceSync,
        "waiter enters only on its next own MissionEnter pass"
    );
    assert!(!waiter_miner.dock_queued);
    assert!(crate::sim::miner::miner_dock::has_contact(&sim, 2, waiter));
    assert!(crate::sim::miner::miner_dock::has_entered(&sim, 2, waiter));
}

#[test]
fn two_miners_refinery_takeover_uses_live_object_order_not_stable_id() {
    let mut sim = Simulation::new();
    let rules = miner_rules();

    let waiter = spawn_miner(&mut sim, 1, MinerKind::War, 13, 11);
    spawn_refinery(&mut sim, 2, 10, 10);
    let occupant = spawn_miner(&mut sim, 3, MinerKind::War, 13, 11);

    // Stable-id order is [waiter(1), refinery(2), occupant(3)], which would
    // process the waiter before the releaser and leave it queued. Native
    // LogicClass order is reveal/insert order, so force the opposite
    // order-visible case: releaser first, waiter second.
    sim.set_logic_order_for_test(vec![occupant, waiter, 2]);

    {
        let entity = sim
            .substrate
            .entities
            .get_mut(waiter)
            .expect("waiter entity");
        let miner = entity.miner.as_mut().expect("waiter miner");
        miner.cargo.push(CargoBale {
            resource_type: ResourceType::Ore,
            value: 25,
        });
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::MissionEnter;
        miner.reserved_refinery = Some(2);
        miner.dock_queued = true;
    }

    {
        let entity = sim
            .substrate
            .entities
            .get_mut(occupant)
            .expect("occupant entity");
        let miner = entity.miner.as_mut().expect("occupant miner");
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Departing;
        miner.reserved_refinery = Some(2);
    }
    assert!(crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, occupant));
    crate::sim::miner::miner_dock::enter_dock(&mut sim, occupant, 2);
    assert_eq!(
        crate::sim::miner::miner_dock::hello(&mut sim, waiter, 2, 1),
        crate::sim::miner::miner_dock::ContactAdmission::Waiting
    );

    tick_miners_n(&mut sim, &rules, 1);

    assert_eq!(get_miner(&sim, occupant).state, MinerState::SearchOre);
    assert!(!crate::sim::miner::miner_dock::has_contact(
        &sim, 2, occupant
    ));

    let waiter_miner = get_miner(&sim, waiter);
    assert_eq!(
        waiter_miner.dock_phase,
        RefineryDockPhase::FaceSync,
        "live order [occupant, waiter] must let the waiter claim after release even though stable-id order would not"
    );
    assert!(!waiter_miner.dock_queued);
    assert!(crate::sim::miner::miner_dock::has_contact(&sim, 2, waiter));
    assert!(crate::sim::miner::miner_dock::has_entered(&sim, 2, waiter));
}

/// Once CAN_DOCK's accepted-cell move is already satisfied, the stock path
/// sets the 0x18/+0x418-style entered flag and runs ordinary 0x16 facing
/// sync. It does not turn that first handshake into radio 0x15 or unload
/// startup side effects.
#[test]
fn accepted_cell_arrival_sets_contact_entered_then_0x15_starts_unload_fsm() {
    let mut sim = Simulation::new();
    let rules = miner_rules();

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 13, 11);
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
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::MissionEnter;
        miner.reserved_refinery = Some(2);
    }
    assert!(crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, miner_id));

    tick_miners_n(&mut sim, &rules, 1);

    let m = get_miner(&sim, miner_id);
    assert_eq!(m.dock_phase, RefineryDockPhase::FaceSync);
    assert!(
        crate::sim::miner::miner_dock::has_entered(&sim, 2, miner_id),
        "already-there 0x12 reply should set the +0x418-like entered flag"
    );

    tick_miners_n(&mut sim, &rules, 1);

    let m = get_miner(&sim, miner_id);
    assert_eq!(
        m.dock_phase,
        RefineryDockPhase::FaceSync,
        "the first ordinary 0x16 only syncs facing; it must not queue deploy"
    );
}

/// Unloading emits one BaleDepositEvent per StorageClass slot drained
/// (matches gamemd: SpecialAnim fires per slot, not per bale).
#[test]
fn unloading_emits_one_event_per_slot_drain() {
    // --- 5 ore bales = 1 slot → 1 event ---
    let mut sim = Simulation::new();
    let rules = miner_rules();

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 13, 11);
    spawn_refinery(&mut sim, 2, 10, 10);

    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        let miner = entity.miner.as_mut().expect("miner component");
        for _ in 0..5 {
            miner.cargo.push(CargoBale {
                resource_type: ResourceType::Ore,
                value: 25,
            });
        }
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Unloading;
        miner.reserved_refinery = Some(2);
    }
    crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, miner_id);

    tick_miners_n(&mut sim, &rules, 200);

    let drained: Vec<_> = sim.bale_events.iter().filter(|e| e.drained).collect();
    assert_eq!(
        drained.len(),
        1,
        "pure-ore cargo must drain in one slot dump = one drain BaleDepositEvent",
    );
    assert_eq!(drained[0].building_id, 2);
    assert_eq!(
        sim.bale_events.len(),
        2,
        "the empty gate that ends state 3 emits its own (smoke-only) event",
    );
    assert!(sim.bale_events[1].empty && !sim.bale_events[1].drained);

    // --- 5 ore + 3 gems = 2 slots → 2 events ---
    let mut sim = Simulation::new();
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 13, 11);
    spawn_refinery(&mut sim, 2, 10, 10);

    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        let miner = entity.miner.as_mut().expect("miner component");
        for _ in 0..5 {
            miner.cargo.push(CargoBale {
                resource_type: ResourceType::Ore,
                value: 25,
            });
        }
        for _ in 0..3 {
            miner.cargo.push(CargoBale {
                resource_type: ResourceType::Gem,
                value: 50,
            });
        }
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Unloading;
        miner.reserved_refinery = Some(2);
    }
    crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, miner_id);

    tick_miners_n(&mut sim, &rules, 200);

    assert_eq!(
        sim.bale_events.iter().filter(|e| e.drained).count(),
        2,
        "ore + gem cargo must produce two drain BaleDepositEvents (one per slot)",
    );
    assert_eq!(
        sim.bale_events.len(),
        3,
        "two drain gates plus the empty gate",
    );
    for event in &sim.bale_events {
        assert_eq!(event.building_id, 2);
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
         Owner=Americans\nFoundation=4x3\nRefinery=yes\n\
         [GAPURI]\n\
         Name=Ore Purifier\nCost=2500\nStrength=1000\nArmor=wood\nTechLevel=1\n\
         Owner=Americans\nFoundation=2x2\nOrePurifier=yes\n",
        // Rules expects PurifierBonus= as a fraction; we use the integer-pct path
        // by writing the fraction value (e.g., 0.25 → 25%). Use the float string.
        bonus_pct as f32 / 100.0,
    ));
    RuleSet::from_ini(&ini).expect("purifier rules")
}

/// Purifier bonus is applied per slot drain on the full slot value
/// (matches gamemd's `bonus = slot_value × purifier_count × PurifierBonus`).
/// With one ore slot of value 100 and a 25% PurifierBonus, total credits
/// gain = 100 + (100 × 1 × 25 / 100) = 125.
#[test]
fn unloading_applies_per_slot_purifier_bonus() {
    let mut sim = Simulation::new();
    let rules = purifier_rules(25);

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 13, 11);
    spawn_refinery(&mut sim, 2, 10, 10);
    // Spawn an OrePurifier-flagged building owned by the same player.
    spawn_structure(&mut sim, 3, "GAPURI", 20, 20);

    let credits_before = credits_for_owner(&sim, "Americans");

    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        let miner = entity.miner.as_mut().expect("miner component");
        miner.cargo.push(CargoBale {
            resource_type: ResourceType::Ore,
            value: 100,
        });
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Unloading;
        miner.reserved_refinery = Some(2);
    }
    crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, miner_id);

    tick_miners_n(&mut sim, &rules, 200);

    let credits_after = credits_for_owner(&sim, "Americans");
    assert_eq!(
        credits_after - credits_before,
        125,
        "100 base + 25 (25% purifier) = 125, got delta {}",
        credits_after - credits_before,
    );
}

/// Conditional reciprocal-link release geometry is anchored at the queue cell.
/// Stock zero-link unload completion does not use it. For a 4x3 GAREFN at
/// (10, 10), the queue cell (14, 11) sits east of the foundation and is the
/// release target on a normal map where only the foundation itself is blocked.
/// The spiral only expands to ring 1+ when the queue cell is also blocked.
/// `tick % count` picks one of the ring's candidates each cycle.
#[test]
fn conditional_release_anchors_at_queue_cell() {
    use super::miner_dock_sequence::refinery_exit_cell;

    // Build a grid with the 4×3 foundation at (10, 10) blocked, simulating
    // real building occupancy. Queue cell (14, 11) is outside the
    // foundation and remains passable.
    let mut grid_garefn = PathGrid::test_all_passable(64, 64);
    for fx in 10..14 {
        for fy in 10..13 {
            grid_garefn.set_blocked(fx, fy, true);
        }
    }

    // Only foundation blocked: queue (14, 11) is itself walkable, so
    // ring 0 returns it deterministically for every tick.
    for tick in 0..6 {
        assert_eq!(
            refinery_exit_cell(10, 10, 4, 3, None, Some(&grid_garefn), None, tick),
            (14, 11),
            "exit must land at queue cell when it is passable (tick {tick})"
        );
    }

    // Now also block the queue cell, simulating another miner queued
    // there. Ring 1 around (14, 11) yields candidates in iteration order
    // (top + bottom rows per delta = -1..=1, then left + right columns):
    //   (13,10) FND, (13,12) FND, (14,10), (14,12), (15,10), (15,12),
    //   (13,11) FND, (15,11)
    // FND = blocked by foundation. Passable candidates, in order:
    //   (14, 10), (14, 12), (15, 10), (15, 12), (15, 11)
    let mut grid_blocked_queue = grid_garefn.clone();
    grid_blocked_queue.set_blocked(14, 11, true);

    assert_eq!(
        refinery_exit_cell(10, 10, 4, 3, None, Some(&grid_blocked_queue), None, 0),
        (14, 10)
    );
    assert_eq!(
        refinery_exit_cell(10, 10, 4, 3, None, Some(&grid_blocked_queue), None, 1),
        (14, 12)
    );
    assert_eq!(
        refinery_exit_cell(10, 10, 4, 3, None, Some(&grid_blocked_queue), None, 2),
        (15, 10)
    );
    assert_eq!(
        refinery_exit_cell(10, 10, 4, 3, None, Some(&grid_blocked_queue), None, 3),
        (15, 12)
    );
    assert_eq!(
        refinery_exit_cell(10, 10, 4, 3, None, Some(&grid_blocked_queue), None, 4),
        (15, 11)
    );
    // tick=5 → wraps (5 % 5 = 0) → (14, 10).
    assert_eq!(
        refinery_exit_cell(10, 10, 4, 3, None, Some(&grid_blocked_queue), None, 5),
        (14, 10)
    );

    // Clean grid (no foundation blocking) — anchor (queue cell) is
    // itself walkable, so ring 0 returns it directly.
    let clean_grid = PathGrid::test_all_passable(64, 64);
    // 4×3 at (10, 10): queue (14, 11).
    assert_eq!(
        refinery_exit_cell(10, 10, 4, 3, None, Some(&clean_grid), None, 0),
        (14, 11)
    );
    // 3×3 at (5, 5): queue (8, 6).
    assert_eq!(
        refinery_exit_cell(5, 5, 3, 3, None, Some(&clean_grid), None, 0),
        (8, 6)
    );
    // 2×2 at (12, 8): queue (14, 9).
    assert_eq!(
        refinery_exit_cell(12, 8, 2, 2, None, Some(&clean_grid), None, 0),
        (14, 9)
    );

    // Anchor + every cell within the max radius blocked → fallback to
    // QueueingCell. Block a 33×33 region covering radius 16.
    let mut fully_blocked = PathGrid::test_all_passable(64, 64);
    for x in 0..32 {
        for y in 0..32 {
            fully_blocked.set_blocked(x, y, true);
        }
    }
    assert_eq!(
        refinery_exit_cell(10, 10, 4, 3, Some((3, 2)), Some(&fully_blocked), None, 0),
        (13, 12),
        "exhausted spiral must fall back to art.ini QueueingCell"
    );
}

/// Stock zero-link Departing is a state-4 cleanup/handoff, not a cached
/// queue-cell exit drive.
#[test]
fn stock_departing_hands_directly_to_search_without_exit_move() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let config = MinerConfig::default();
    let path_grid = PathGrid::new(64, 64);

    spawn_refinery(&mut sim, 100, 10, 10);
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 13, 11);
    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        let miner = entity.miner.as_mut().expect("miner component");
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Departing;
        miner.reserved_refinery = Some(100);
        assert!(
            miner.exit_cell.is_none(),
            "stock path must start without a cached release destination"
        );
    }
    crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 100, miner_id);

    crate::sim::miner::miner_system::tick_miners(&mut sim, &rules, &config, Some(&path_grid));

    let entity = sim.substrate.entities.get(miner_id).expect("entity");
    let m = entity.miner.as_ref().expect("miner");
    assert_eq!(entity.miner_state().unwrap(), MinerState::SearchOre);
    assert_eq!((entity.position.rx, entity.position.ry), (13, 11));
    assert!(entity.movement_target.is_none());
    assert!(!has_bunker_release_track(entity));
    assert!(m.exit_cell.is_none());
    assert!(m.reserved_refinery.is_none());
    assert!(!crate::sim::miner::miner_dock::test_support::dock_test_is_occupied(&sim, 100));
}

#[test]
fn stock_departing_does_not_start_force_track_0x47() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let config = MinerConfig::default();
    let path_grid = PathGrid::new(64, 64);

    spawn_refinery(&mut sim, 100, 10, 10);
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::Chrono, 13, 11);
    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        let miner = entity.miner.as_mut().expect("miner component");
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Departing;
        miner.reserved_refinery = Some(100);
    }
    crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 100, miner_id);

    crate::sim::miner::miner_system::tick_miners(&mut sim, &rules, &config, Some(&path_grid));

    let entity = sim.substrate.entities.get(miner_id).expect("miner entity");
    let miner = entity.miner.as_ref().expect("miner component");
    assert_eq!(entity.miner_state().unwrap(), MinerState::SearchOre);
    assert!(miner.exit_cell.is_none());
    assert!(entity.movement_target.is_none());
    assert!(
        !has_bunker_release_track(entity),
        "stock Departing must not seed Force_Track(0x47)"
    );
    assert_ne!(entity.facing, 0x47);
    assert_ne!(entity.facing_target, Some(0x47));
}

#[test]
fn stock_departing_does_not_start_explicit_exit_move() {
    let mut sim = Simulation::new();
    let rules = miner_rules();

    spawn_refinery(&mut sim, 100, 10, 10);
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::Chrono, 13, 11);
    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        let miner = entity.miner.as_mut().expect("miner component");
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Departing;
        miner.reserved_refinery = Some(100);
    }
    crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 100, miner_id);

    tick_miners_n(&mut sim, &rules, 1);
    let after_first = sim.substrate.entities.get(miner_id).expect("miner entity");
    assert_eq!((after_first.position.rx, after_first.position.ry), (13, 11));
    assert!(!has_bunker_release_track(after_first));
    assert!(after_first.movement_target.is_none());
    assert!(matches!(
        after_first.miner_state().expect("miner cursor"),
        MinerState::SearchOre
    ));
    assert!(
        after_first
            .miner
            .as_ref()
            .expect("miner")
            .exit_cell
            .is_none()
    );
}

#[test]
fn sell_refinery_adapter_preserves_docked_miner_track_speed_and_cargo() {
    let mut sim = Simulation::new();
    let rules = miner_rules();

    spawn_refinery(&mut sim, 100, 10, 10);
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::Chrono, 13, 11);
    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        entity.display_type_override = Some(sim.interner.intern("CMON"));
        // Retained movement is supplied to catch both an unsupported Force
        // and an invented universal cancel. This test does not model native
        // sale radio0x17's later scatter/mission behavior.
        entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
        entity.drive_locomotion = Some(crate::sim::components::DriveLocomotionRuntime {
            head_to: Some(crate::sim::components::DriveCoord::cell(13, 12, 0)),
            track: crate::sim::components::TrackProgress {
                turn_index: 0,
                cursor: 2,
                reversed: true,
                residual: 3,
            },
            track_valid: true,
            ..Default::default()
        });
        entity.foot_speed.applied_fraction = crate::util::fixed_math::SimFixed::lit("0.25");
        entity.foot_speed.cached_current_speed = 7;
        let miner = entity.miner.as_mut().expect("miner component");
        for _ in 0..miner.capacity_bales {
            miner.cargo.push(CargoBale {
                resource_type: ResourceType::Ore,
                value: 25,
            });
        }
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Unloading;
        miner.reserved_refinery = Some(100);
        miner.dock_queued = true;
    }
    crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 100, miner_id);

    let (position_before, drive_before, speed_before, cargo_before) = {
        let entity = sim.substrate.entities.get(miner_id).unwrap();
        (
            crate::sim::movement::ground_pose::position_world_coord(&entity.position),
            entity.drive_locomotion.clone(),
            entity.foot_speed.clone(),
            entity.miner.as_ref().unwrap().cargo.clone(),
        )
    };
    assert!(crate::sim::production::sell_building(&mut sim, &rules, 100));

    // Deferred-delete: sell_building enqueues; the end-of-tick P9 flush (invoked
    // directly here) frees the slot. Dock links are cleared synchronously in sell.
    sim.flush_pending_delete();
    assert!(sim.substrate.entities.get(100).is_none(), "refinery sold");
    assert!(
        !crate::sim::miner::miner_dock::test_support::dock_test_is_occupied(&sim, 100),
        "sell interrupt must clear dock links"
    );
    let entity = sim.substrate.entities.get(miner_id).expect("miner entity");
    let miner = entity.miner.as_ref().expect("miner component");
    assert_eq!(entity.miner_state().unwrap(), MinerState::ReturnToRefinery);
    assert_eq!(miner.dock_phase, RefineryDockPhase::Approach);
    assert_eq!(miner.reserved_refinery, None);
    assert_eq!(miner.exit_cell, None);
    assert_eq!(entity.display_type_override, None);
    assert!(entity.movement_target.is_none());
    assert!(
        !has_bunker_release_track(entity),
        "a refinery contact does not authorize bunker Force_Track(0x47)"
    );
    assert_eq!(
        crate::sim::movement::ground_pose::position_world_coord(&entity.position),
        position_before
    );
    assert_eq!(entity.drive_locomotion, drive_before);
    assert_eq!(entity.foot_speed, speed_before);
    assert_eq!(miner.cargo, cargo_before);
}

#[test]
fn sell_refinery_cancels_contact_miner_without_force_track_0x47() {
    let mut sim = Simulation::new();
    let rules = miner_rules();

    spawn_refinery(&mut sim, 100, 10, 10);
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::Chrono, 14, 11);
    {
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
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::MissionEnter;
        miner.reserved_refinery = Some(100);
        miner.dock_queued = true;
    }
    assert!(crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 100, miner_id));
    assert!(crate::sim::miner::miner_dock::has_contact(
        &sim, 100, miner_id
    ));

    assert!(crate::sim::production::sell_building(&mut sim, &rules, 100));

    // Deferred-delete: sell_building enqueues; the end-of-tick P9 flush (invoked
    // directly here) frees the slot. Dock links are cleared synchronously in sell.
    sim.flush_pending_delete();
    assert!(sim.substrate.entities.get(100).is_none(), "refinery sold");
    assert!(
        !crate::sim::miner::miner_dock::has_contact(&sim, 100, miner_id),
        "sell interrupt must clear plain refinery contacts"
    );
    let entity = sim.substrate.entities.get(miner_id).expect("miner entity");
    let miner = entity.miner.as_ref().expect("miner component");
    assert_eq!(entity.miner_state().unwrap(), MinerState::ReturnToRefinery);
    assert_eq!(miner.dock_phase, RefineryDockPhase::Approach);
    assert_eq!(miner.reserved_refinery, None);
    assert_eq!(miner.exit_cell, None);
    assert!(entity.movement_target.is_none());
    assert!(
        !has_bunker_release_track(entity),
        "refinery contacts do not authorize bunker Force_Track(0x47)"
    );
}

/// Stock state-4 handoff must not depend on driving through the queue cell.
/// A waiting miner parked there cannot block cleanup because no explicit
/// exit movement is issued.
#[test]
fn departing_handoff_ignores_blocked_queue_cell() {
    let mut sim = Simulation::new();
    let rules = miner_rules();

    spawn_refinery(&mut sim, 100, 10, 10);
    // Miner A on the pad cell, ready to depart.
    let miner_a = spawn_miner(&mut sim, 1, MinerKind::War, 13, 11);
    {
        let entity = sim.substrate.entities.get_mut(miner_a).expect("miner A");
        let miner = entity.miner.as_mut().expect("miner A component");
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Departing;
        miner.reserved_refinery = Some(100);
    }
    crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 100, miner_a);

    // Miner B parked at the QueueingCell (14, 11) — blocks miner A's only
    // adjacent walkable exit from the pad.
    let miner_b = spawn_miner(&mut sim, 2, MinerKind::War, 14, 11);
    {
        let entity = sim.substrate.entities.get_mut(miner_b).expect("miner B");
        let miner = entity.miner.as_mut().expect("miner B component");
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Approach;
        miner.dock_queued = true;
    }
    // Register B's occupancy at the queue cell so the deferred check sees it.
    sim.substrate.occupancy.add(
        14,
        11,
        miner_b,
        crate::sim::movement::locomotor::MovementLayer::Ground,
        None,
        crate::sim::occupancy::CellListInsertion::PrependNonBuilding,
    );

    tick_miners_n(&mut sim, &rules, 1);

    let entity = sim.substrate.entities.get(miner_a).expect("miner A entity");
    let m = entity.miner.as_ref().expect("miner A");
    assert_eq!(entity.miner_state().unwrap(), MinerState::SearchOre);
    assert!(
        m.reserved_refinery.is_none(),
        "miner A's dock reservation must be released during state-4 handoff",
    );
    assert_eq!(
        (entity.position.rx, entity.position.ry),
        (13, 11),
        "state-4 handoff must not issue a stock queue-cell exit move",
    );
}

/// Departing releases the dock reservation, clears any stale exit-cell cache,
/// and transitions back to SearchOre without pinning facing to 0x47.
#[test]
fn departing_handoff_releases_dock_and_returns_to_search() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let config = MinerConfig::default();
    let path_grid = PathGrid::new(64, 64);

    spawn_refinery(&mut sim, 100, 10, 10);
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 13, 11);
    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        let miner = entity.miner.as_mut().expect("miner component");
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Departing;
        miner.reserved_refinery = Some(100);
        miner.exit_cell = Some((14, 11));
    }
    crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 100, miner_id);

    crate::sim::miner::miner_system::tick_miners(&mut sim, &rules, &config, Some(&path_grid));

    let m = get_miner(&sim, miner_id);
    assert_eq!(m.state, MinerState::SearchOre);
    assert!(m.reserved_refinery.is_none(), "dock reservation released");
    assert!(m.exit_cell.is_none(), "stale exit-cell cache cleared");
    assert!(
        !crate::sim::miner::miner_dock::test_support::dock_test_is_occupied(&sim, 100),
        "dock slot freed for next miner",
    );
}

/// Linked sets the UnloadingClass display override, emits a DockDeploy
/// sound on pad arrival, kicks off the pivot to facing East (0x40), and
/// transitions to Pivoting. The pivot runs in phase_pivoting; once facing
/// converges the FSM advances to Unloading and seeds `unload_timer`.
/// Mirrors gamemd's radio 0x16 (FACE_AND_SYNC) RateTimer pivot which fires
/// before the dump cascade (radio 0x15 → SetMission(Mission_Unload)).
#[test]
fn linked_to_pivoting_then_unloading_on_pad_arrival() {
    use crate::sim::world::SimSoundEvent;
    let mut sim = Simulation::new();
    // Custom rules with UnloadingClass=HORV on HARV so the override path runs.
    let rules = {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n0=HARV\n[AircraftTypes]\n\
             [BuildingTypes]\n0=GAREFN\n\
             [HARV]\nName=War Miner\nCost=1400\nStrength=600\nArmor=heavy\nSpeed=4\n\
             ROT=5\nSight=5\nTechLevel=1\nOwner=Americans\nHarvester=yes\n\
             Dock=GAREFN\nUnloadingClass=HORV\n\
             [GAREFN]\nName=Ore Refinery\nCost=2000\nStrength=900\nArmor=wood\n\
             TechLevel=1\nOwner=Americans\nFoundation=4x3\nRefinery=yes\n",
        );
        RuleSet::from_ini(&ini).expect("custom rules")
    };
    let config = MinerConfig::default();
    let path_grid = PathGrid::new(64, 64);

    spawn_refinery(&mut sim, 2, 10, 10);
    // Place miner at the pad cell already facing 0x40 (East). This isolates
    // the Linked → Pivoting → Unloading transition from the per-tick
    // rotation step, so the test pins exactly the two-phase handshake
    // without depending on a precise per-frame rotation delta.
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 13, 11);
    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        entity.movement_target = None;
        entity.display_type_override = None;
        entity.facing = 0x40;
        let miner = entity.miner.as_mut().expect("miner component");
        miner.cargo.push(CargoBale {
            resource_type: ResourceType::Ore,
            value: 25,
        });
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::MissionQueued;
        miner.reserved_refinery = Some(2);
    }
    crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, miner_id);

    // Tick 1: radio 0x15 has only queued mission 0x10, so this advances to
    // the deploy mission without unload presentation side effects.
    crate::sim::miner::miner_system::tick_miners(&mut sim, &rules, &config, Some(&path_grid));

    {
        let m = get_miner(&sim, miner_id);
        assert_eq!(m.dock_phase, RefineryDockPhase::Pivoting);

        let entity = sim.substrate.entities.get(miner_id).expect("entity");
        assert_eq!(entity.facing_target, None);
        assert_eq!(entity.display_type_override, None);
        assert!(
            sim.sound_events
                .iter()
                .all(|e| !matches!(e, SimSoundEvent::DockDeploy { building_id: 2 })),
            "0x15 must not emit DockDeploy before mission 0x10 starts unload"
        );
    }

    // Tick 2: phase_pivoting sees facing already at the target — the
    // "close enough" branch fires immediately, snaps facing, seeds
    // unload_timer, and transitions to Unloading.
    tick_miners_n(&mut sim, &rules, 1);

    {
        let m = get_miner(&sim, miner_id);
        assert_eq!(
            m.dock_phase,
            RefineryDockPhase::Unloading,
            "Pivoting must transition to Unloading once facing reaches 0x40",
        );
        assert!(m.unload_active, "unload-active latch should be set");
        assert_eq!(m.unload_accumulator, 0);
        assert_eq!(m.unload_cluster_timer.start_frame, sim.session.binary_frame);
        assert_eq!(m.unload_cluster_timer.duration, 1);
        assert_eq!(m.unload_cluster_repeat, 1);
        assert_eq!(m.unload_accumulator_step, 1);
        assert!(
            (14..=16).contains(&m.mission_deploy_timer.duration),
            "accepted unload-start should schedule stock 14..16 frames, got {}",
            m.mission_deploy_timer.duration
        );

        let entity = sim.substrate.entities.get(miner_id).expect("entity");
        assert_eq!(
            entity.facing, 0x40,
            "pre-aligned facing should remain unchanged; unload-start must not snap it",
        );
        assert!(
            entity.facing_target.is_none(),
            "facing_target must be cleared once the pivot completes",
        );
        let override_id = entity
            .display_type_override
            .expect("UnloadingClass override should be set when unload starts");
        assert_eq!(sim.interner.resolve(override_id), "HORV");
        let dock_deploy_count = sim
            .sound_events
            .iter()
            .filter(|e| matches!(e, SimSoundEvent::DockDeploy { building_id: 2 }))
            .count();
        assert_eq!(
            dock_deploy_count, 0,
            "stock unload-start emits no DockDeploy"
        );
    }
}

/// Pivoting phase advances facing toward 0x40 (East) one rotation step at
/// a time and only transitions to Unloading once facing reaches the target.
/// Verifies the smooth-rotation path (not the pre-aligned shortcut tested
/// in `linked_to_pivoting_then_unloading_on_pad_arrival`).
#[test]
fn pivoting_phase_smoothly_rotates_to_east() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let config = MinerConfig::default();
    let path_grid = PathGrid::new(64, 64);

    spawn_refinery(&mut sim, 2, 10, 10);
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 13, 11);
    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        entity.movement_target = None;
        entity.facing = 0; // North — must rotate 64 facing units clockwise to reach 0x40.
        entity.facing_target = Some(0x40);
        let miner = entity.miner.as_mut().expect("miner component");
        miner.cargo.push(CargoBale {
            resource_type: ResourceType::Ore,
            value: 25,
        });
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Pivoting;
        miner.reserved_refinery = Some(2);
    }
    crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, miner_id);

    let initial_facing = sim.substrate.entities.get(miner_id).expect("entity").facing;
    let rng_before = sim.scenario_rng.state();
    assert_eq!(initial_facing, 0);

    // The first direct tick initializes the FacingClass timer and samples its
    // current 16-bit facing; this is timer-derived motion, not manual 8-bit
    // facing stepping.
    crate::sim::miner::miner_system::tick_miners(&mut sim, &rules, &config, Some(&path_grid));
    {
        let entity = sim.substrate.entities.get(miner_id).expect("entity");
        let m = entity.miner.as_ref().expect("miner");
        assert_eq!(
            entity.facing, initial_facing,
            "dock facing timer must not write visible body facing"
        );
        assert_eq!(m.dock_phase, RefineryDockPhase::Pivoting);
        assert_eq!(entity.facing_target, Some(0x40));
        assert!(m.dock_pivot_facing.is_some());
        assert_eq!(m.mission_deploy_timer.duration, 5);
        assert_eq!(m.mission_deploy_timer.start_frame, sim.session.binary_frame);
        assert_eq!(
            sim.scenario_rng.state(),
            rng_before,
            "facing wait consumes no RNG"
        );
    }

    tick_miners_n(&mut sim, &rules, 1);
    {
        let entity = sim.substrate.entities.get(miner_id).expect("entity");
        let m = entity.miner.as_ref().expect("miner");
        assert_eq!(
            entity.facing, initial_facing,
            "passive mission delay must not advance visible facing"
        );
        assert_eq!(m.dock_phase, RefineryDockPhase::Pivoting);
        assert_eq!(entity.facing_target, Some(0x40));
    }

    // Tick until the pivot resolves. Cap is generous; stock harvester ROT=
    // remains the parsed INI value, so low-ROT cases can take up to 64 ticks.
    let mut ticks_until_done = 0;
    for _ in 0..128 {
        tick_miners_n(&mut sim, &rules, 1);
        ticks_until_done += 1;
        if get_miner(&sim, miner_id).dock_phase == RefineryDockPhase::Unloading {
            break;
        }
    }

    let entity = sim.substrate.entities.get(miner_id).expect("entity");
    let m = entity.miner.as_ref().expect("miner");
    assert_eq!(
        m.dock_phase,
        RefineryDockPhase::Unloading,
        "pivot must reach Unloading within 128 ticks (took {})",
        ticks_until_done,
    );
    assert_eq!(
        entity.facing, initial_facing,
        "dock mission must not force the visible body facing to East"
    );
    assert!(entity.facing_target.is_none());
    assert!(m.unload_active);
}

/// End-to-end dock cycle: war miner forced-returns to a refinery, drives onto
/// the pad, deposits N bales, and completes stock state-4 handoff. Verifies
/// bale event count, total credits, final position, and dock release.
#[test]
fn full_dock_cycle_war_miner() {
    let mut sim = Simulation::new();
    let rules = miner_rules();

    spawn_refinery(&mut sim, 100, 10, 10);
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 14, 11);

    // Pre-load 10 bales (smaller than full capacity to keep test fast).
    let bale_count: i32 = 10;
    let bale_value: i32 = 25;
    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        let miner = entity.miner.as_mut().expect("miner component");
        for _ in 0..bale_count {
            miner.cargo.push(CargoBale {
                resource_type: ResourceType::Ore,
                value: bale_value as u16,
            });
        }
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Approach;
        miner.reserved_refinery = Some(100);
    }

    let credits_before = credits_for_owner(&sim, "Americans");

    // Tick enough for: HELLO, MissionEnter/CAN_DOCK, accepted-cell/pad handoff,
    // 10 bales x ~14 ticks unload, then stock state-4 handoff.
    tick_miners_n(&mut sim, &rules, 400);

    // Bale events: one per due dump gate. Pure ore → 1 slot drain + the empty
    // gate that ends state 3 (both fire the refinery smoke burst, 0x0073E37E).
    let drained = sim.bale_events.iter().filter(|e| e.drained).count();
    assert_eq!(
        drained,
        1,
        "expected 1 slot-drain event, got {} of {}",
        drained,
        sim.bale_events.len(),
    );
    assert_eq!(
        sim.bale_events.len(),
        2,
        "expected the drain gate plus the empty gate, got {}",
        sim.bale_events.len(),
    );
    assert!(sim.bale_events[1].empty, "the last gate found no cargo");

    // Credits: bale_count * bale_value (no purifier in miner_rules).
    let credits_after = credits_for_owner(&sim, "Americans");
    assert_eq!(
        credits_after - credits_before,
        bale_count * bale_value,
        "expected +{} credits, got delta {}",
        bale_count * bale_value,
        credits_after - credits_before,
    );

    let entity = sim.substrate.entities.get(miner_id).expect("entity");
    // The Mission_Deploy state-4 hand-off itself installs no exit move, but
    // with no ore on the map the following scan misses and, 105 frames
    // later, Mission_Harvest state 4 finds the miner on a refinery cell and
    // sets `FUN_00703590`'s nearby passable cell as its destination before
    // queueing Guard — so by now the miner has stepped off the pad.
    let inside_footprint =
        (10..14).contains(&entity.position.rx) && (10..13).contains(&entity.position.ry);
    assert!(
        !inside_footprint,
        "the Harvest idle tail moved the miner off the refinery footprint, got {:?}",
        (entity.position.rx, entity.position.ry)
    );
    assert_eq!(
        entity.mission.queued().known(),
        Some(crate::sim::mission::MissionType::Guard),
        "state 4 queued Guard behind the exit move"
    );
    assert!(!has_bunker_release_track(entity));

    let m = entity.miner.as_ref().expect("miner");
    // After Departing → SearchOre, with no ore on the map the miner falls
    // through to WaitNoOre. Either is a valid post-dock state.
    assert!(
        matches!(
            entity.miner_state().unwrap(),
            MinerState::SearchOre | MinerState::WaitNoOre
        ),
        "post-dock state must be SearchOre or WaitNoOre, got {:?}",
        entity.miner_state().unwrap(),
    );
    assert!(m.cargo.is_empty(), "cargo must be drained");
    assert!(
        m.reserved_refinery.is_none(),
        "reservation must be released"
    );
    assert!(
        !crate::sim::miner::miner_dock::test_support::dock_test_is_occupied(&sim, 100),
        "dock must be free for the next miner"
    );
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
// Per-bite extraction integration tests (parity contract for handle_harvest)
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
         Refinery=yes\n\
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
        crate::sim::movement::tick_movement(
            &mut sim.substrate.entities,
            &mut sim.interner,
            &mut sim.pending_lifecycle_requests,
        );
        sim.session.tick += 1;
    }
}

/// Drives the full handle_harvest path on the legacy node model: a War Miner
/// on an 11-density ore cell takes exactly one bale per gate. The first gate
/// fires on the cleared timer; every later bale waits the native
/// `HarvesterLoadRate` cadence (`harvest_tick_interval + 1` = 19 frames).
#[test]
fn harvester_takes_one_bale_per_gate_over_eleven_gates() {
    let mut sim = Simulation::new();
    spawn_inert_dock_instance(&mut sim);
    let rules = miner_rules();
    let config = MinerConfig::default();
    let gate = usize::from(config.harvest_tick_interval) + 1;

    place_ore(&mut sim, 20, 20, 11 * 120);

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 20, 20);
    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        let miner = entity.miner.as_mut().expect("miner component");
        entity
            .mission
            .set_handler_state(MinerState::Harvest.cursor());
        miner.target_ore_cell = Some((20, 20));
        miner.harvest_timer.clear();
    }

    // Gate 1 fires immediately on the cleared timer: one bale, density 10.
    tick_miners_n(&mut sim, &rules, 1);
    {
        let miner = get_miner(&sim, miner_id);
        assert_eq!(miner.cargo.len(), 1, "first gate removes one level");
        assert_eq!(miner.state, MinerState::Harvest);
        assert_eq!(
            miner.harvest_timer.duration,
            u32::from(config.harvest_tick_interval) + 1,
            "success re-arms the native F+19 gate"
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
    let config = MinerConfig::default();
    let gate = usize::from(config.harvest_tick_interval) + 1;
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

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, cell.0, cell.1);
    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        let miner = entity.miner.as_mut().expect("miner component");
        entity
            .mission
            .set_handler_state(MinerState::Harvest.cursor());
        miner.target_ore_cell = Some(cell);
        miner.harvest_timer.clear();
    }

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
            miner.harvest_timer.duration,
            u32::from(config.harvest_tick_interval) + 1,
            "gate {bale}: success re-arms F+19"
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
    // bale, miner retargets the neighbouring patch.
    tick_miners_overlay_n(&mut sim, &rules, &registry, 1);
    let miner = get_miner(&sim, miner_id);
    assert_eq!(miner.cargo.len(), 11, "density-0 gate credits nothing");
    assert_eq!(density(&sim), (None, 0), "density-0 overlay is cleared");
    assert_eq!(miner.state, MinerState::MoveToOre, "miner moves on");
    assert_eq!(
        miner.target_ore_cell,
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
        entity
            .mission
            .set_handler_state(MinerState::Harvest.cursor());
        miner.target_ore_cell = Some((20, 20));
        miner.harvest_timer.clear();
    }

    tick_miners_n(&mut sim, &rules, 1);

    let miner = get_miner(&sim, miner_id);
    assert_eq!(miner.cargo.len(), 39, "one level per gate even with 2 free");
    assert_eq!(
        miner.state,
        MinerState::Harvest,
        "positive extraction remains a successful Harvest tick"
    );
    assert_eq!(
        miner.harvest_timer.duration,
        u32::from(config.harvest_tick_interval) + 1,
        "success-reset gate remains due at the native F+19 observation"
    );
    let after_remaining = crate::sim::tiberium::test_support::stock_amount_at(&sim, (20, 20));
    assert_eq!(after_remaining, 10 * 120, "cell drops to density 10");

    // The next gate takes the fortieth bale: filling is still a success.
    tick_miners_n(
        &mut sim,
        &rules,
        usize::from(config.harvest_tick_interval) + 1,
    );

    let miner = get_miner(&sim, miner_id);
    assert_eq!(miner.cargo.len(), 40, "capped at capacity");
    assert_eq!(
        miner.state,
        MinerState::Harvest,
        "positive filling extraction remains a successful Harvest tick"
    );
    assert_eq!(
        miner.last_harvest_cell, None,
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
        entity
            .mission
            .set_handler_state(MinerState::Harvest.cursor());
        miner.target_ore_cell = Some((30, 30));
        miner.harvest_timer.clear();
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

    tick_miners_n(&mut sim, &rules, 1);
    let fill_frame = sim.session.binary_frame;
    {
        let entity = sim.substrate.entities.get(miner_id).expect("miner entity");
        let miner = entity.miner.as_ref().expect("miner component");
        assert_eq!(miner.cargo.len(), 40);
        assert_eq!(entity.miner_state().unwrap(), MinerState::Harvest);
        assert_eq!(miner.harvest_timer.start_frame, fill_frame);
        assert_eq!(
            miner.harvest_timer.duration,
            u32::from(config.harvest_tick_interval) + 1
        );
        assert_eq!(miner.last_harvest_cell, None);
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

    tick_miners_n(&mut sim, &rules, config.harvest_tick_interval as usize);
    assert_eq!(
        sim.session.binary_frame.wrapping_sub(fill_frame),
        u32::from(config.harvest_tick_interval)
    );
    {
        let entity = sim.substrate.entities.get(miner_id).expect("miner entity");
        let miner = entity.miner.as_ref().expect("miner component");
        assert_eq!(
            entity.miner_state().unwrap(),
            MinerState::Harvest,
            "F+18 remains pending"
        );
        assert_eq!(miner.last_harvest_cell, None);
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
        assert_eq!(
            full_gate_frame.wrapping_sub(fill_frame),
            u32::from(config.harvest_tick_interval) + 1
        );
        assert_eq!(entity.miner_state().unwrap(), MinerState::ReturnToRefinery);
        assert_eq!(miner.harvest_timer.start_frame, full_gate_frame);
        assert_eq!(miner.harvest_timer.duration, 0);
        assert_eq!(miner.last_harvest_cell, Some((31, 30)));
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

    tick_miners_n(&mut sim, &rules, 1);
    {
        let entity = sim.substrate.entities.get(miner_id).expect("miner entity");
        let miner = entity.miner.as_ref().expect("miner component");
        assert_eq!(miner.reserved_refinery, Some(2));
        assert!(
            entity.movement_target.is_some(),
            "F+20 state-2 dispatch issues the existing far HARV return move"
        );
    }
}

#[test]
fn chrono_filling_extraction_does_not_warp_before_state2_tick() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let config = MinerConfig::default();

    place_ore(&mut sim, 63, 63, 11 * 120);
    spawn_refinery(&mut sim, 2, 10, 10);
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::Chrono, 63, 63);

    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        let miner = entity.miner.as_mut().expect("miner component");
        // 19 of 20 loaded: the single-level gate request fills on one bite.
        for _ in 0..19 {
            miner.cargo.push(CargoBale {
                resource_type: ResourceType::Ore,
                value: config.ore_bale_value,
            });
        }
        entity
            .mission
            .set_handler_state(MinerState::Harvest.cursor());
        miner.target_ore_cell = Some((63, 63));
        miner.harvest_timer.clear();
    }

    sim.sound_events.clear();
    tick_miners_n(&mut sim, &rules, 1);
    let fill_frame = sim.session.binary_frame;
    {
        let entity = sim.substrate.entities.get(miner_id).expect("miner entity");
        let miner = entity.miner.as_ref().expect("miner component");
        assert_eq!(miner.cargo.len(), 20);
        assert_eq!(entity.miner_state().unwrap(), MinerState::Harvest);
        assert_eq!(miner.harvest_timer.start_frame, fill_frame);
        assert_eq!(
            miner.harvest_timer.duration,
            u32::from(config.harvest_tick_interval) + 1
        );
        assert_eq!(miner.last_harvest_cell, None);
        assert_eq!(miner.reserved_refinery, None);
        assert!(entity.movement_target.is_none());
        assert!(entity.teleport_state.is_none());
        assert!(sim.sound_events.iter().all(|event| !matches!(
            event,
            crate::sim::world::SimSoundEvent::ChronoTeleport { .. }
        )));
    }
    assert_eq!(
        crate::sim::tiberium::test_support::stock_amount_at(&sim, (63, 63)),
        10 * 120,
        "one level per gate: the filling bite drops 11 -> 10"
    );

    tick_miners_n(&mut sim, &rules, config.harvest_tick_interval as usize);
    {
        let entity = sim.substrate.entities.get(miner_id).expect("miner entity");
        let miner = entity.miner.as_ref().expect("miner component");
        assert_eq!(
            sim.session.binary_frame.wrapping_sub(fill_frame),
            u32::from(config.harvest_tick_interval)
        );
        assert_eq!(
            entity.miner_state().unwrap(),
            MinerState::Harvest,
            "F+18 remains pending"
        );
        assert_eq!(miner.last_harvest_cell, None);
        assert_eq!(miner.reserved_refinery, None);
        assert!(entity.movement_target.is_none());
        assert!(entity.teleport_state.is_none());
        assert!(sim.sound_events.iter().all(|event| !matches!(
            event,
            crate::sim::world::SimSoundEvent::ChronoTeleport { .. }
        )));
    }

    tick_miners_n(&mut sim, &rules, 1);
    let full_gate_frame = sim.session.binary_frame;
    {
        let entity = sim.substrate.entities.get(miner_id).expect("miner entity");
        let miner = entity.miner.as_ref().expect("miner component");
        assert_eq!(
            full_gate_frame.wrapping_sub(fill_frame),
            u32::from(config.harvest_tick_interval) + 1
        );
        assert_eq!(miner.cargo.len(), 20);
        assert_eq!(entity.miner_state().unwrap(), MinerState::ReturnToRefinery);
        assert_eq!(miner.harvest_timer.start_frame, full_gate_frame);
        assert_eq!(miner.harvest_timer.duration, 0);
        assert_eq!(
            miner.last_harvest_cell,
            Some((63, 63)),
            "archive is selected from the productive F+19 source cell"
        );
        assert_eq!(miner.reserved_refinery, None);
        assert!(entity.movement_target.is_none());
        assert!(entity.teleport_state.is_none());
        assert!(sim.sound_events.iter().all(|event| !matches!(
            event,
            crate::sim::world::SimSoundEvent::ChronoTeleport { .. }
        )));
    }
    assert_eq!(
        crate::sim::tiberium::test_support::stock_amount_at(&sim, (63, 63)),
        10 * 120
    );

    tick_miners_n(&mut sim, &rules, 1);
    {
        let entity = sim.substrate.entities.get(miner_id).expect("miner entity");
        let miner = entity.miner.as_ref().expect("miner component");
        assert_eq!(miner.reserved_refinery, Some(2));
        assert!(entity.teleport_state.is_some());
    }
    assert_eq!(
        sim.sound_events
            .iter()
            .filter(|event| matches!(
                event,
                crate::sim::world::SimSoundEvent::ChronoTeleport { .. }
            ))
            .count(),
        2
    );
}

/// After a partial-density cell is fully drained but the miner still has
/// capacity, the next harvest cycle's empty-cell branch should kick a
/// TiberiumShortScan continuation that picks up the neighbouring patch.
#[test]
fn harvester_continues_to_short_scan_when_partial_then_empty() {
    let mut sim = Simulation::new();
    spawn_inert_dock_instance(&mut sim);
    let rules = miner_rules();
    let config = MinerConfig::default();

    // Density-5 cell at (20, 20). Another density-5 cell at (21, 20),
    // safely within the local continuation radius (6 cells).
    place_ore(&mut sim, 20, 20, 5 * 120);
    place_ore(&mut sim, 21, 20, 5 * 120);

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 20, 20);
    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        let miner = entity.miner.as_mut().expect("miner component");
        entity
            .mission
            .set_handler_state(MinerState::Harvest.cursor());
        miner.target_ore_cell = Some((20, 20));
        miner.harvest_timer.clear();
    }

    // Five gates drain (20, 20): one level per gate (Harvest_Ore_Tick
    // @ 0x0073D450 requests min(1, free)). Each success re-arms the
    // harvest_tick_interval + 1 gate and the miner stays in Harvest.
    tick_miners_n(&mut sim, &rules, 1);
    for _ in 1..5 {
        tick_miners_n(&mut sim, &rules, config.harvest_tick_interval as usize + 1);
    }
    {
        let miner = get_miner(&sim, miner_id);
        assert_eq!(miner.cargo.len(), 5, "5 bales after 5 gates");
        assert_eq!(
            miner.state,
            MinerState::Harvest,
            "stays in Harvest, timer reset"
        );
        assert_eq!(
            crate::sim::tiberium::test_support::bales_at(&sim, 20, 20),
            0,
            "cell drained"
        );
    }

    // Tick out the harvest_tick_interval wait; the next extraction attempt
    // hits an empty cell and the short-scan picks up (21, 20).
    tick_miners_n(&mut sim, &rules, config.harvest_tick_interval as usize + 1);
    {
        let miner = get_miner(&sim, miner_id);
        assert_eq!(
            miner.state,
            MinerState::MoveToOre,
            "transitions to MoveToOre after empty-cell short scan"
        );
        assert_eq!(miner.target_ore_cell, Some((21, 20)));
    }
}

/// gamemd parity: the first dock bale must wait
/// `ceil(HarvesterDumpRate × 900) = 15` frames after the Linked →
/// Unloading transition, not fire immediately. The dump counter starts
/// at 0 on dock-link and a bale deposits only once the counter reaches
/// 14.4. With our tenths-of-a-tick precision (timer decrements by 10
/// per tick before the drain check) the first slot drain fires 15
/// unloading ticks after Linked, dumping ALL bales of the first
/// non-empty resource type at once (matches gamemd's per-slot dump).
#[test]
fn dock_first_slot_drain_waits_one_unload_interval() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let config = MinerConfig::default();

    spawn_refinery(&mut sim, 2, 10, 10);
    // Place miner at the pad cell facing 0x40 (East) so the dock pivot
    // (Linked → Pivoting → Unloading) completes in two ticks and the
    // 14.4-frame dump gate timing this test pins lines up cleanly.
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 13, 11);
    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        entity.movement_target = None;
        entity.facing = 0x40;
        let miner = entity.miner.as_mut().expect("miner component");
        for _ in 0..5 {
            miner.cargo.push(CargoBale {
                resource_type: ResourceType::Ore,
                value: config.ore_bale_value,
            });
        }
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::MissionQueued;
        miner.reserved_refinery = Some(2);
    }
    crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, miner_id);

    // Tick 1: phase_linked transitions to Pivoting. No drain yet.
    // Tick 2: phase_pivoting sees facing already at 0x40, transitions to
    // Unloading and seeds the unload_timer. No drain yet.
    tick_miners_n(&mut sim, &rules, 2);

    let initial_cargo = get_miner(&sim, miner_id).cargo.len();
    assert_eq!(initial_cargo, 5, "no drain should fire before Unloading");
    assert_eq!(
        get_miner(&sim, miner_id).dock_phase,
        RefineryDockPhase::Unloading,
        "pivot should complete in one tick when facing is pre-aligned",
    );

    // Ticks 3..16 (14 unloading ticks): timer decrements past zero, no drain
    // yet (decrement-then-check returns before drain on the tick the
    // timer crosses ≤ 0).
    let mut drain_tick = None;
    for elapsed in 1..=20 {
        tick_miners_n(&mut sim, &rules, 1);
        if get_miner(&sim, miner_id).cargo.is_empty() {
            drain_tick = Some(elapsed);
            break;
        }
        assert_eq!(
            get_miner(&sim, miner_id).cargo.len(),
            initial_cargo,
            "no partial drain should fire before the slot dump gate"
        );
    }

    let drain_tick = drain_tick.expect("slot should drain within Plan C timing window");
    assert!(
        (15..=16).contains(&drain_tick),
        "Plan C first slot drain should be gated by accepted mission delay plus accumulator threshold, got tick {}",
        drain_tick
    );
    assert_eq!(get_miner(&sim, miner_id).cargo.len(), 0);
}

/// Verify the empty-slot gate + stock state-4 dock release:
/// 1. Cargo is already empty when the dump gate fires.
/// 2. The same tick advances to Departing, with the dock still occupied.
/// 3. The next tick runs the stock state-4 handoff and releases the dock.
///
/// Sets the miner up in Unloading with empty cargo so the cargo-empty branch
/// fires on the very first tick.
#[test]
fn empty_unload_gate_releases_dock_on_next_stock_state4_handoff() {
    let mut sim = Simulation::new();
    let rules = miner_rules();

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 13, 11);
    spawn_refinery(&mut sim, 2, 10, 10);
    let unloading_type = sim.interner.intern("HORV");

    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        entity.display_type_override = Some(unloading_type);
        let miner = entity.miner.as_mut().expect("miner component");
        // Empty cargo + zero timer → first tick hits the cargo-empty branch.
        miner.cargo.clear();
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Unloading;
        miner.reserved_refinery = Some(2);
    }
    // Mark the dock occupied so we can assert release timing directly.
    assert!(crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, miner_id));
    assert!(crate::sim::miner::miner_dock::test_support::dock_test_is_occupied(&sim, 2));

    // First tick: phase_unloading sees empty cargo and advances to the
    // state-4 handoff without seeding another dump-gate cooldown.
    tick_miners_n(&mut sim, &rules, 1);

    let m = get_miner(&sim, miner_id);
    assert_eq!(
        m.dock_phase,
        RefineryDockPhase::Departing,
        "empty-slot gate should transition directly to Departing",
    );
    assert!(
        crate::sim::miner::miner_dock::test_support::dock_test_is_occupied(&sim, 2),
        "dock is still occupied until the Departing handler runs",
    );

    // Next tick runs the stock state-4 handoff.
    tick_miners_n(&mut sim, &rules, 1);

    let m = get_miner(&sim, miner_id);
    assert!(
        m.state == MinerState::SearchOre,
        "miner should have returned to search at state-4 handoff, got {:?}",
        m.state,
    );
    assert!(
        !crate::sim::miner::miner_dock::test_support::dock_test_is_occupied(&sim, 2),
        "dock must be released by the stock state-4 handoff",
    );
    assert!(
        !m.unload_active,
        "state-4 handoff must clear the Unit+0x6D1 unload-active latch",
    );
    let entity = sim.substrate.entities.get(miner_id).expect("miner entity");
    assert_eq!(
        entity.display_type_override, None,
        "state-4 handoff must clear the unloading display override",
    );
}

#[test]
fn unload_state3_uses_west_cell_building_not_reserved_refinery() {
    let mut sim = Simulation::new();
    let rules = miner_rules();

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 13, 11);
    spawn_refinery(&mut sim, 2, 30, 30);
    spawn_structure_owned(&mut sim, 3, "GAREFN", "Germans", 12, 11);

    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        let miner = entity.miner.as_mut().expect("miner component");
        miner.cargo.push(CargoBale {
            resource_type: ResourceType::Ore,
            value: 100,
        });
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Unloading;
        miner.reserved_refinery = Some(2);
    }
    assert!(crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, miner_id));

    let americans_before = credits_for_owner(&sim, "Americans");
    let germans_before = credits_for_owner(&sim, "Germans");
    tick_miners_n(&mut sim, &rules, 1);

    assert_eq!(credits_for_owner(&sim, "Americans"), americans_before);
    assert_eq!(credits_for_owner(&sim, "Germans") - germans_before, 100);
    assert_eq!(sim.bale_events.len(), 1);
    assert_eq!(sim.bale_events[0].building_id, 3);
}

#[test]
fn missing_west_cell_building_does_not_credit_or_emit_deposit_event() {
    let mut sim = Simulation::new();
    let rules = miner_rules();

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 13, 11);
    spawn_refinery(&mut sim, 2, 30, 30);

    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        let miner = entity.miner.as_mut().expect("miner component");
        miner.cargo.push(CargoBale {
            resource_type: ResourceType::Ore,
            value: 100,
        });
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Unloading;
        miner.reserved_refinery = Some(2);
    }
    assert!(crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, miner_id));

    let credits_before = credits_for_owner(&sim, "Americans");
    tick_miners_n(&mut sim, &rules, 1);

    let miner = get_miner(&sim, miner_id);
    assert_eq!(miner.cargo.len(), 1);
    assert_eq!(credits_for_owner(&sim, "Americans"), credits_before);
    assert!(sim.bale_events.is_empty());
}

#[test]
fn state3_null_lookup_preserves_full_cargo_and_returns_to_refinery_selection() {
    let mut sim = Simulation::new();
    let rules = miner_rules();

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 13, 11);
    spawn_refinery(&mut sim, 2, 30, 30);

    {
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
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Unloading;
        miner.reserved_refinery = Some(2);
    }
    assert!(crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, miner_id));

    tick_miners_n(&mut sim, &rules, 1);

    let miner = get_miner(&sim, miner_id);
    assert_eq!(miner.cargo.len(), miner.capacity_bales as usize);
    assert_eq!(miner.state, MinerState::ReturnToRefinery);
    assert_eq!(miner.dock_phase, RefineryDockPhase::Approach);
    assert_eq!(miner.reserved_refinery, None);
}

#[test]
fn state3_null_lookup_does_not_clear_unload_display_latch() {
    let mut sim = Simulation::new();
    let rules = miner_rules();

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 13, 11);
    spawn_refinery(&mut sim, 2, 30, 30);
    let unloading_type = sim.interner.intern("HORV");

    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        entity.display_type_override = Some(unloading_type);
        let miner = entity.miner.as_mut().expect("miner component");
        miner.cargo.push(CargoBale {
            resource_type: ResourceType::Ore,
            value: 100,
        });
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Unloading;
        miner.reserved_refinery = Some(2);
    }
    assert!(crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, miner_id));

    tick_miners_n(&mut sim, &rules, 1);

    let entity = sim.substrate.entities.get(miner_id).expect("miner entity");
    let miner = entity.miner.as_ref().expect("miner component");
    assert!(
        miner.unload_active,
        "state-3 null lookup must preserve the Unit+0x6D1 unload-active latch",
    );
    assert_eq!(entity.display_type_override, Some(unloading_type));
}

#[test]
fn reserved_refinery_released_but_not_used_for_unload_credit_identity() {
    let mut sim = Simulation::new();
    let rules = miner_rules();

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 13, 11);
    spawn_refinery(&mut sim, 2, 30, 30);
    spawn_structure_owned(&mut sim, 3, "GAREFN", "Germans", 12, 11);

    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        let miner = entity.miner.as_mut().expect("miner component");
        miner.cargo.push(CargoBale {
            resource_type: ResourceType::Ore,
            value: 100,
        });
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Unloading;
        miner.reserved_refinery = Some(2);
    }
    assert!(crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, miner_id));
    crate::sim::miner::miner_dock::enter_dock(&mut sim, miner_id, 2);

    let germans_before = credits_for_owner(&sim, "Germans");
    tick_miners_n(&mut sim, &rules, 18);

    assert_eq!(credits_for_owner(&sim, "Germans") - germans_before, 100);
    assert_eq!(sim.bale_events[0].building_id, 3);
    assert!(!crate::sim::miner::miner_dock::has_contact(
        &sim, 2, miner_id
    ));
    assert_eq!(get_miner(&sim, miner_id).reserved_refinery, None);
}

#[test]
fn state4_refinery_yes_guard_is_caller_owned() {
    let mut sim = Simulation::new();
    let rules = miner_rules();

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 13, 11);
    spawn_refinery(&mut sim, 2, 30, 30);
    spawn_structure(&mut sim, 3, "GAPOWR", 12, 11);

    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        let miner = entity.miner.as_mut().expect("miner component");
        miner.cargo.clear();
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Departing;
        miner.reserved_refinery = Some(2);
    }
    assert!(crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, miner_id));
    crate::sim::miner::miner_dock::enter_dock(&mut sim, miner_id, 2);

    tick_miners_n(&mut sim, &rules, 1);

    let miner = get_miner(&sim, miner_id);
    assert_eq!(miner.state, MinerState::SearchOre);
    assert_eq!(miner.reserved_refinery, None);
    assert!(!crate::sim::miner::miner_dock::has_contact(
        &sim, 2, miner_id
    ));
}

#[test]
fn queued_miner_takes_over_immediately_after_empty_gate_handoff() {
    let mut sim = Simulation::new();
    let rules = miner_rules();

    let occupant = spawn_miner(&mut sim, 1, MinerKind::War, 13, 11);
    let waiter = spawn_miner(&mut sim, 3, MinerKind::War, 13, 11);
    spawn_refinery(&mut sim, 2, 10, 10);

    {
        let entity = sim
            .substrate
            .entities
            .get_mut(occupant)
            .expect("occupant entity");
        let miner = entity.miner.as_mut().expect("occupant miner");
        miner.cargo.clear();
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Unloading;
        miner.reserved_refinery = Some(2);
    }
    assert!(crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, occupant));
    crate::sim::miner::miner_dock::enter_dock(&mut sim, occupant, 2);

    {
        let entity = sim
            .substrate
            .entities
            .get_mut(waiter)
            .expect("waiter entity");
        let miner = entity.miner.as_mut().expect("waiter miner");
        miner.cargo.push(CargoBale {
            resource_type: ResourceType::Ore,
            value: 25,
        });
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::MissionEnter;
        miner.reserved_refinery = Some(2);
        miner.dock_queued = true;
    }
    assert!(!crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, waiter));

    tick_miners_n(&mut sim, &rules, 1);
    assert_eq!(
        get_miner(&sim, occupant).dock_phase,
        RefineryDockPhase::Departing,
        "empty gate should reach state-4 handoff before release",
    );
    assert!(
        !crate::sim::miner::miner_dock::has_contact(&sim, 2, waiter),
        "waiter is not admitted while the occupant holds the only slot (no wait-queue)",
    );

    tick_miners_n(&mut sim, &rules, 1);

    let occupant_miner = get_miner(&sim, occupant);
    assert_eq!(occupant_miner.state, MinerState::SearchOre);
    assert!(!crate::sim::miner::miner_dock::has_contact(
        &sim, 2, occupant
    ));
    let waiter_miner = get_miner(&sim, waiter);
    assert_eq!(
        waiter_miner.dock_phase,
        RefineryDockPhase::MissionEnter,
        "queued miner waits for the stock Enter retry after the freed contact tick",
    );
    tick_miners_n(&mut sim, &rules, 16);
    let waiter_miner = get_miner(&sim, waiter);
    assert_eq!(
        waiter_miner.dock_phase,
        RefineryDockPhase::FaceSync,
        "queued miner takes the freed contact on its next due MissionEnter pass",
    );
    assert!(!waiter_miner.dock_queued);
    assert!(crate::sim::miner::miner_dock::has_contact(&sim, 2, waiter));
    assert!(crate::sim::miner::miner_dock::has_entered(&sim, 2, waiter));
}

/// Verify the purifier bonus scales linearly with the number of purifiers
/// owned. Two purifiers must produce 2× the bonus of one (regression for
/// the old boolean-based formula that capped the bonus at +25% regardless
/// of count).
#[test]
fn two_purifiers_stack_the_bonus_linearly() {
    let mut sim = Simulation::new();
    let rules = purifier_rules(25);

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 13, 11);
    spawn_refinery(&mut sim, 2, 10, 10);
    // Two OrePurifier buildings owned by the same player.
    spawn_structure(&mut sim, 3, "GAPURI", 20, 20);
    spawn_structure(&mut sim, 4, "GAPURI", 24, 20);

    let credits_before = credits_for_owner(&sim, "Americans");

    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        let miner = entity.miner.as_mut().expect("miner component");
        miner.cargo.push(CargoBale {
            resource_type: ResourceType::Ore,
            value: 100,
        });
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Unloading;
        miner.reserved_refinery = Some(2);
    }
    crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, miner_id);

    tick_miners_n(&mut sim, &rules, 200);

    // 100 base + (100 × 2 purifiers × 25 / 100) = 100 + 50 = 150.
    let delta = credits_for_owner(&sim, "Americans") - credits_before;
    assert_eq!(
        delta, 150,
        "2 purifiers @ 25% each should stack to +50% (got {} cr)",
        delta,
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

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 13, 11);
    spawn_refinery(&mut sim, 2, 10, 10);
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
    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        let miner = entity.miner.as_mut().expect("miner component");
        miner.cargo.push(CargoBale {
            resource_type: ResourceType::Ore,
            value: 100,
        });
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Unloading;
        miner.reserved_refinery = Some(2);
    }
    crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, miner_id);
    tick_miners_n(&mut sim, &rules, 200);
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

/// AI player with `is_human=false` should receive the virtual-purifier
/// bonus from `rules.general.ai_virtual_purifiers[house.difficulty]`. With the
/// default `[4, 2, 0]` and per-house Hard difficulty (native index 0),
/// no real purifiers, and a 100-credit bale, credits = 100 + (100 × 4 × 25 / 100) = 200.
#[test]
fn ai_brutal_gets_virtual_purifier_bonus() {
    use crate::sim::house_state::{HouseDifficulty, HouseState};

    let mut sim = Simulation::new();
    let rules = purifier_rules(25);

    // Mark the Americans house as AI, Brutal difficulty.
    let owner_id = sim.interner.intern("Americans");
    let mut house = HouseState::new(owner_id, 0, None, false, 0, 10);
    house.difficulty = HouseDifficulty::Hard;
    sim.houses.insert(owner_id, house);
    // The retired `2 - global` workaround maps this value to Easy. The
    // refinery owner's native field must still select the Hard table entry.
    sim.session.game_options.ai_difficulty = 0;

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 13, 11);
    spawn_refinery(&mut sim, 2, 10, 10);
    // No real purifiers — bonus should come entirely from the AI table.

    let credits_before = credits_for_owner(&sim, "Americans");

    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        let miner = entity.miner.as_mut().expect("miner component");
        miner.cargo.push(CargoBale {
            resource_type: ResourceType::Ore,
            value: 100,
        });
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Unloading;
        miner.reserved_refinery = Some(2);
    }
    crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, miner_id);

    tick_miners_n(&mut sim, &rules, 200);

    let delta = credits_for_owner(&sim, "Americans") - credits_before;
    assert_eq!(
        delta, 200,
        "Brutal AI with 0 real purifiers should get +4 virtual × 25% = +100% (got {} cr)",
        delta,
    );
}

/// Human player with `is_human=true` must NOT get the AI virtual bonus
/// even though the table is configured. Guards against accidentally
/// rewarding the human in skirmish.
#[test]
fn human_player_does_not_get_ai_virtual_bonus() {
    use crate::sim::house_state::HouseState;

    let mut sim = Simulation::new();
    let rules = purifier_rules(25);

    let owner_id = sim.interner.intern("Americans");
    sim.houses.insert(
        owner_id,
        HouseState::new(owner_id, 0, None, true, 0, 10), // is_human=true
    );
    sim.session.game_options.ai_difficulty = 0;

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 13, 11);
    spawn_refinery(&mut sim, 2, 10, 10);

    let credits_before = credits_for_owner(&sim, "Americans");

    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        let miner = entity.miner.as_mut().expect("miner component");
        miner.cargo.push(CargoBale {
            resource_type: ResourceType::Ore,
            value: 100,
        });
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Unloading;
        miner.reserved_refinery = Some(2);
    }
    crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, miner_id);

    tick_miners_n(&mut sim, &rules, 200);

    let delta = credits_for_owner(&sim, "Americans") - credits_before;
    assert_eq!(
        delta, 100,
        "human player with no real purifiers gets base credits only (got {} cr)",
        delta,
    );
}

/// Easy AI uses native index 2, the bottom of the hardest-first
/// `AIVirtualPurifiers` table (`[4, 2, 0]` -> 0), i.e. no virtual bonus.
/// Regression guard for using the legacy global lobby difficulty instead of
/// the refinery owner's authoritative HouseState field.
#[test]
fn ai_easy_gets_no_virtual_purifier_bonus() {
    use crate::sim::house_state::{HouseDifficulty, HouseState};

    let mut sim = Simulation::new();
    let rules = purifier_rules(25);

    let owner_id = sim.interner.intern("Americans");
    let mut house = HouseState::new(owner_id, 0, None, false, 0, 10);
    house.difficulty = HouseDifficulty::Easy;
    sim.houses.insert(owner_id, house);
    // The retired `2 - global` workaround maps this value to Hard. It must not
    // turn the Easy refinery owner into a Hard owner for deposit arithmetic.
    sim.session.game_options.ai_difficulty = 2;

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 13, 11);
    spawn_refinery(&mut sim, 2, 10, 10);

    let credits_before = credits_for_owner(&sim, "Americans");

    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        let miner = entity.miner.as_mut().expect("miner component");
        miner.cargo.push(CargoBale {
            resource_type: ResourceType::Ore,
            value: 100,
        });
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Unloading;
        miner.reserved_refinery = Some(2);
    }
    crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, miner_id);

    tick_miners_n(&mut sim, &rules, 200);

    let delta = credits_for_owner(&sim, "Americans") - credits_before;
    assert_eq!(
        delta, 100,
        "Easy AI with 0 real purifiers gets base credits only (got {} cr)",
        delta,
    );
}

#[test]
fn virtual_purifier_table_is_indexed_per_refinery_owner() {
    use crate::sim::house_state::{HouseDifficulty, HouseState};

    let mut sim = Simulation::new();
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
}

/// Legacy DepositCooldown save states pass straight through to Departing. The
/// old per-tick `deposit_cooldown_ticks` countdown is retired (Slice 5), so the
/// phase no longer holds — it advances on the first tick.
#[test]
fn legacy_deposit_cooldown_passes_through_to_departing() {
    let mut sim = Simulation::new();
    let rules = miner_rules();

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 13, 11);
    spawn_refinery(&mut sim, 2, 10, 10);

    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        let miner = entity.miner.as_mut().expect("miner component");
        miner.cargo.clear();
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::DepositCooldown;
        miner.reserved_refinery = Some(2);
    }
    crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, miner_id);

    tick_miners_n(&mut sim, &rules, 1);
    let m = get_miner(&sim, miner_id);
    assert_eq!(
        m.dock_phase,
        RefineryDockPhase::Departing,
        "legacy DepositCooldown should pass straight through to Departing",
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
    crate::sim::miner::miner_dock::enter_dock(&mut sim, occupant, 2);

    // What the damage receiver does for a unit with a death animation.
    sim.substrate.entities.get_mut(occupant).unwrap().dying = true;
    crate::sim::radio::broadcast_break(&mut sim, occupant);

    let corpse = sim.substrate.entities.get(occupant).expect("corpse stays");
    assert!(!corpse.radio_contacts.contains(2));
    assert_eq!(corpse.dock_entered_with, None);
    assert!(crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, waiter));
}

/// `refinery_hello` refuses another house for as long as that holds, so a
/// reservation on a refinery the miner's house lost (engineer capture) must be
/// dropped and reselected, not retried forever.
#[test]
fn reservation_on_a_captured_refinery_is_dropped_and_reselected() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    // Beyond `HarvesterTooFarDistance`: the miner reserves the nearer refinery
    // and drives, holding no contact yet.
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 5, 10);
    spawn_refinery(&mut sim, 2, 20, 10);
    spawn_refinery(&mut sim, 3, 40, 10);
    fill_and_return(&mut sim, miner_id);

    tick_miners_n(&mut sim, &rules, 1);
    assert_eq!(get_miner(&sim, miner_id).reserved_refinery, Some(2));
    assert!(!crate::sim::miner::miner_dock::has_contact(
        &sim, 2, miner_id
    ));

    let captor = sim.interner.intern("Russians");
    sim.change_owner(2, captor);
    // HARV state 2 re-evaluates its refinery only once its NavCom is spent, so
    // finish the drive the fixture cannot perform.
    {
        let entity = sim.substrate.entities.get_mut(miner_id).unwrap();
        entity.movement_target = None;
        entity.navigation.nav_com = None;
    }

    let mut reselected = false;
    for _ in 0..200 {
        tick_miners_n(&mut sim, &rules, 1);
        if get_miner(&sim, miner_id).reserved_refinery == Some(3) {
            reselected = true;
            break;
        }
    }
    assert!(
        reselected,
        "the miner must give up the foreign refinery and pick its house's other one"
    );
    assert!(!crate::sim::miner::miner_dock::has_contact(
        &sim, 2, miner_id
    ));
}

/// `EventClass::Execute`'s MEGAMISSION arm (`0x004C72E8..0x004C7342`) BREAKs the
/// radio link of an untethered unit, and of a tethered one whose contact is a
/// refinery. A miner ordered away before the unload frees the refinery and
/// restarts its handshake from HELLO.
#[test]
fn megamission_before_the_unload_breaks_the_refinery_contact() {
    for entered in [false, true] {
        let mut sim = Simulation::new();
        let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 14, 11);
        let waiter = spawn_miner(&mut sim, 3, MinerKind::War, 14, 12);
        spawn_refinery(&mut sim, 2, 10, 10);
        {
            let entity = sim.substrate.entities.get_mut(miner_id).unwrap();
            let miner = entity.miner.as_mut().unwrap();
            miner.reserved_refinery = Some(2);
            miner.dock_phase = if entered {
                RefineryDockPhase::FaceSync
            } else {
                RefineryDockPhase::MissionEnter
            };
        }
        assert!(
            crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, miner_id)
        );
        if entered {
            crate::sim::miner::miner_dock::enter_dock(&mut sim, miner_id, 2);
        }

        sim.queue_megamission_with_teardown(
            miner_id,
            crate::sim::mission::MissionType::Move,
            crate::sim::mission::DockTeardown::All,
        );

        let entity = sim.substrate.entities.get(miner_id).unwrap();
        assert!(!entity.radio_contacts.contains(2), "entered={entered}");
        assert_eq!(entity.dock_entered_with, None, "entered={entered}");
        assert_eq!(
            entity.miner.as_ref().unwrap().dock_phase,
            RefineryDockPhase::Approach,
            "the handshake restarts from HELLO; entered={entered}"
        );
        assert!(
            crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, waiter),
            "entered={entered}"
        );
    }
}

/// A Move ordered mid-unload BREAKs the contact but leaves the unload phase to
/// the Unload mission's own `In_Radio_Contact` gate (`0x0073DEE7`): the next
/// dispatch drops the unload latch and image and commences the queued order.
/// Resetting the phase instead would keep the latch, block the order's
/// readiness and send the miner through a second dock.
#[test]
fn megamission_mid_unload_abandons_the_unload_and_commences_the_order() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let cargo = vec![(ResourceType::Ore, 25u16); 20];
    let miner_id = spawn_queued_unload_miner(&mut sim, &cargo);
    crate::sim::miner::miner_dock::enter_dock(&mut sim, miner_id, 2);

    for _ in 0..200 {
        tick_miners_n(&mut sim, &rules, 1);
        let miner = get_miner(&sim, miner_id);
        if miner.dock_phase == RefineryDockPhase::Unloading && miner.unload_active {
            break;
        }
    }
    let before = get_miner(&sim, miner_id);
    assert_eq!(before.dock_phase, RefineryDockPhase::Unloading);
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
    );
    assert!(!crate::sim::miner::miner_dock::has_contact(
        &sim, 2, miner_id
    ));
    assert_eq!(
        get_miner(&sim, miner_id).dock_phase,
        RefineryDockPhase::Unloading,
        "the retask leaves the unload phase for the contact gate to end"
    );

    let mut commenced = false;
    for _ in 0..60 {
        tick_miners_n(&mut sim, &rules, 1);
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
    let cargo = vec![(ResourceType::Ore, 25u16); 20];
    let miner_id = spawn_queued_unload_miner(&mut sim, &cargo);
    crate::sim::miner::miner_dock::enter_dock(&mut sim, miner_id, 2);
    for _ in 0..200 {
        tick_miners_n(&mut sim, &rules, 1);
        let miner = get_miner(&sim, miner_id);
        if miner.dock_phase == RefineryDockPhase::Unloading && miner.unload_active {
            break;
        }
    }
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
    assert!(!miner.unload_cluster_timer.is_armed());
    assert_eq!(miner.dock_phase, RefineryDockPhase::Approach);
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
            miner.dock_phase = RefineryDockPhase::MissionEnter;
        }
        assert!(
            crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, miner_id)
        );
        if entered {
            crate::sim::miner::miner_dock::enter_dock(&mut sim, miner_id, 2);
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

/// A full miner whose reserved refinery enters its death animation must not
/// fall back into ore search. Mission_Harvest checks full storage before
/// scanning for ore, so the miner keeps looking for a refinery target.
#[test]
fn full_miner_losing_dying_refinery_keeps_returning() {
    let mut sim = Simulation::new();
    let rules = miner_rules();

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 5, 10);
    spawn_refinery(&mut sim, 2, 10, 10);
    spawn_refinery(&mut sim, 3, 24, 10);

    {
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
        entity
            .mission
            .set_handler_state(MinerState::ReturnToRefinery.cursor());
        miner.reserved_refinery = Some(2);
        miner.target_ore_cell = Some((6, 10));
        miner.dock_queued = true;
    }
    sim.substrate.entities.get_mut(2).expect("refinery").dying = true;

    tick_miners_n(&mut sim, &rules, 1);

    let m = get_miner(&sim, miner_id);
    assert_eq!(
        m.state,
        MinerState::ReturnToRefinery,
        "full miner must keep returning after a reserved refinery becomes invalid",
    );
    assert_eq!(
        m.reserved_refinery, None,
        "invalid dying refinery reservation must be cleared before re-selection",
    );
    assert_eq!(
        m.cargo.len(),
        m.capacity_bales as usize,
        "cargo must be preserved when the refinery disappears",
    );
    assert_eq!(
        m.target_ore_cell, None,
        "full return fallback must not keep a stale ore target",
    );
    assert!(!m.dock_queued, "stale dock queue state must be cleared");

    // The state-2 dispatch exits through the Rate epilogue (~14-16f); run
    // past the full window so the next due dispatch performs re-selection.
    tick_miners_n(&mut sim, &rules, 17);

    let m = get_miner(&sim, miner_id);
    assert_eq!(
        m.reserved_refinery,
        Some(3),
        "the next due return dispatch must choose the remaining live refinery, not the dying one",
    );
}

/// If the refinery is sold/destroyed while a miner is visually unloading,
/// Rust must abort the dock sequence instead of crediting more bales to a
/// dying building or leaving the miner rendered as its unloading class.
#[test]
fn dying_refinery_aborts_unload_without_credit_or_stuck_visual() {
    let mut sim = Simulation::new();
    let rules = miner_rules();

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 13, 11);
    spawn_refinery(&mut sim, 2, 10, 10);

    let credits_before = credits_for_owner(&sim, "Americans");
    let unloading_type = sim.interner.intern("HORV");

    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        entity.display_type_override = Some(unloading_type);
        entity.facing_target = Some(0x40);
        let miner = entity.miner.as_mut().expect("miner component");
        for _ in 0..miner.capacity_bales {
            miner.cargo.push(CargoBale {
                resource_type: ResourceType::Ore,
                value: 25,
            });
        }
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Unloading;
        miner.reserved_refinery = Some(2);
        miner.dock_queued = true;
        miner.exit_cell = Some((14, 11));
    }
    assert!(crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, miner_id));
    sim.substrate.entities.get_mut(2).expect("refinery").dying = true;

    tick_miners_n(&mut sim, &rules, 1);

    let m = get_miner(&sim, miner_id);
    assert_eq!(
        credits_for_owner(&sim, "Americans"),
        credits_before,
        "dying refinery must not receive unload credits",
    );
    assert_eq!(
        m.cargo.len(),
        m.capacity_bales as usize,
        "abort must preserve the miner cargo instead of draining bales",
    );
    assert_eq!(
        m.state,
        MinerState::ReturnToRefinery,
        "full miner must return to refinery selection after an unload abort",
    );
    assert_eq!(m.dock_phase, RefineryDockPhase::Approach);
    assert_eq!(m.reserved_refinery, None);
    assert!(!m.dock_queued, "queued flag must be cleared on abort");
    assert_eq!(m.exit_cell, None, "exit cache must be cleared on abort");
    assert!(
        !crate::sim::miner::miner_dock::test_support::dock_test_is_occupied(&sim, 2),
        "dock reservation must be removed for a dying refinery",
    );

    let entity = sim.substrate.entities.get(miner_id).expect("miner entity");
    assert_eq!(
        entity.display_type_override,
        Some(unloading_type),
        "state-3 missing-building cleanup preserves the unload display latch until state-4/abort cleanup owns it",
    );
    assert_eq!(
        entity.facing_target, None,
        "dock pivot target must be cleared on abort",
    );
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
    use crate::sim::pathfinding::zone_map::ZoneGrid;
    use std::collections::BTreeMap;

    let mut sim = Simulation::new();

    spawn_inert_dock_instance(&mut sim);
    let rules = miner_rules();

    // 32×32 all-passable grid except for one tree on the would-be best ore
    // cell at (10, 10). The other ore at (12, 10) is also reachable but
    // farther, so without the path-grid filter the scan would pick (10, 10).
    let mut grid = PathGrid::new(32, 32);
    grid.set_blocked(10, 10, true);
    let zone_grid = ZoneGrid::build(&grid, &BTreeMap::new(), 32, 32);
    sim.zone_grid = Some(zone_grid);

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 5, 10);
    place_ore(&mut sim, 10, 10, 1200);
    place_ore(&mut sim, 12, 10, 1200);

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

    // Use a path_grid that matches the blocked cell so build_scan_filter
    // sees the tree. tick_miners_n's default 64×64 all-passable grid would
    // miss it, so call tick_miners directly with the right grid.
    let config = MinerConfig::default();
    super::miner_system::tick_miners(&mut sim, &rules, &config, Some(&grid));

    let m = get_miner(&sim, miner_id);
    assert_ne!(
        m.target_ore_cell,
        Some((10, 10)),
        "must not target tree-blocked ore cell (10,10)",
    );
    assert_eq!(
        m.target_ore_cell,
        Some((12, 10)),
        "must fall through to the next-best clear ore cell",
    );
}

/// An ore cell occupied by another vehicle (e.g. a war miner sitting on
/// it harvesting) is rejected by ring 1+ scan; harvester targets a
/// different cell.
#[test]
fn scan_skips_cell_occupied_by_other_miner() {
    use crate::sim::pathfinding::zone_map::ZoneGrid;
    use std::collections::BTreeMap;

    let mut sim = Simulation::new();

    spawn_inert_dock_instance(&mut sim);
    let rules = miner_rules();

    let grid = PathGrid::new(32, 32);
    let zone_grid = ZoneGrid::build(&grid, &BTreeMap::new(), 32, 32);
    sim.zone_grid = Some(zone_grid);

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

    let m = get_miner(&sim, miner_b);
    assert_ne!(
        m.target_ore_cell,
        Some((10, 10)),
        "must not target cell occupied by another miner",
    );
    assert_eq!(
        m.target_ore_cell,
        Some((12, 10)),
        "must fall through to the next clear ore cell",
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
        m.target_ore_cell,
        Some((10, 10)),
        "ring-0 fast path must return the harvester's own ore cell",
    );
}

// ---------------------------------------------------------------------------
// MoveToOre destination guard (gamemd parity for Mission_Harvest state 0)
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

/// If a tree blocks the initially-chosen ore cell, the miner must NOT
/// target it on first scan — the scan filter rejects it, and a different
/// ore cell is picked from the start.
#[test]
fn move_to_ore_avoids_tree_blocked_cell_from_start() {
    use crate::sim::pathfinding::zone_map::ZoneGrid;
    use std::collections::BTreeMap;

    let mut sim = Simulation::new();

    spawn_inert_dock_instance(&mut sim);
    let rules = miner_rules();

    let mut grid = PathGrid::new(32, 32);
    grid.set_blocked(12, 12, true);
    let zone_grid = ZoneGrid::build(&grid, &BTreeMap::new(), 32, 32);
    sim.zone_grid = Some(zone_grid);

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 8, 12);
    place_ore(&mut sim, 12, 12, 1200);
    place_ore(&mut sim, 13, 13, 1200);

    {
        let entity = sim.substrate.entities.get_mut(miner_id).expect("miner");
        entity
            .mission
            .set_handler_state(MinerState::SearchOre.cursor());
    }

    let config = MinerConfig::default();
    super::miner_system::tick_miners(&mut sim, &rules, &config, Some(&grid));

    let m = get_miner(&sim, miner_id);
    assert_ne!(
        m.target_ore_cell,
        Some((12, 12)),
        "tree-blocked cell rejected"
    );
    assert!(
        matches!(m.state, MinerState::MoveToOre | MinerState::Harvest),
        "must transition out of SearchOre — got {:?}",
        m.state,
    );
}

/// Blocking the cell an already-commanded drive is aimed at must change
/// nothing while the destination is still held.
///
/// Mission_Harvest state 0 wraps its whole body — the ore scan, the cell
/// lookup and the destination write — in a "no destination held" guard. While
/// a destination IS held the state is a no-op that re-arms the Rate cadence
/// and returns; it never looks at ore. Only one candidate fast-retarget path
/// was checked against a *distant* destination going impassable (the
/// destination repair inside the locomotor's path search) and it cannot fire
/// here; whether anything else in the engine reacts to that trigger is
/// UNCHECKED. So on the checked paths the miner keeps driving at the tree and
/// only re-picks once the drive ends.
///
/// Non-vacuity: the blocked cell is the one the scan chose, and a second ore
/// patch sits one ring further out, so a body that re-ran the scan here would
/// visibly move the target.
#[test]
fn move_to_ore_holds_target_while_destination_is_held() {
    use crate::sim::pathfinding::zone_map::ZoneGrid;
    use std::collections::BTreeMap;

    let mut sim = Simulation::new();

    spawn_inert_dock_instance(&mut sim);
    let rules = miner_rules();

    let mut grid = PathGrid::new(32, 32);
    let zone_grid = ZoneGrid::build(&grid, &BTreeMap::new(), 32, 32);
    sim.zone_grid = Some(zone_grid);

    let miner_id = spawn_drive_miner(&mut sim, 1, 8, 12);
    place_ore(&mut sim, 12, 12, 1200);
    place_ore(&mut sim, 11, 12, 1200);

    {
        let entity = sim.substrate.entities.get_mut(miner_id).expect("miner");
        entity
            .mission
            .set_handler_state(MinerState::SearchOre.cursor());
    }

    let config = MinerConfig::default();
    super::miner_system::tick_miners(&mut sim, &rules, &config, Some(&grid));

    let initial_target = get_miner(&sim, miner_id).target_ore_cell;
    let blocked_cell = initial_target.expect("the scan must pick an initial target");
    {
        let entity = sim.substrate.entities.get(miner_id).expect("miner entity");
        assert!(
            entity.navigation.nav_com.is_some(),
            "the scan dispatch must leave the OWNER destination in place — this \
             is the half of the guard that survives the Drive host migration, \
             and without it the guard under test is never reached",
        );
        assert!(
            entity.movement_target.is_some(),
            "the scan dispatch must leave the transitional destination in place \
             too — the guard reads both while MovementTarget is still a second \
             owner",
        );
    }

    // Block the cell the drive is aimed at, and rebuild the zone map with it,
    // so a scan re-run from here would reject it and answer (12, 12) instead.
    grid.set_blocked(blocked_cell.0, blocked_cell.1, true);
    sim.zone_grid = Some(ZoneGrid::build(&grid, &BTreeMap::new(), 32, 32));

    // Move off the scan's own frame so the epilogue anchor below is observable,
    // then ask for the next dispatch explicitly.
    sim.session.binary_frame += 1;
    let dispatch_frame = sim.session.binary_frame;
    arm_dispatch_now(&mut sim, miner_id);

    // The held-destination return exits through the default Rate epilogue, so
    // it draws exactly one RandomRanged(0, 2). Mirror it twice: once for the
    // value the delay must carry, once for the scenario-stream position that
    // draw must leave behind. The value alone cannot tell "drew and added 0"
    // from "never drew", and cannot see a second draw at all — and stream
    // position is the thing lockstep actually depends on.
    let expected_scenario = {
        let mut probe = sim.miner_jitter_rng().clone();
        let _ =
            probe.next_range_u32_inclusive(0, super::miner_system::RATE_EPILOGUE_JITTER_MAX_FRAMES);
        probe.logical_state()
    };
    let jitter = {
        let mut probe = sim.miner_jitter_rng().clone();
        probe.next_range_u32_inclusive(0, super::miner_system::RATE_EPILOGUE_JITTER_MAX_FRAMES)
    };

    super::miner_system::tick_miners(&mut sim, &rules, &config, Some(&grid));

    let m = get_miner(&sim, miner_id);
    assert_eq!(
        m.state,
        MinerState::MoveToOre,
        "the held-destination return leaves the cursor where it was",
    );
    assert_eq!(
        m.target_ore_cell, initial_target,
        "state 0 must not look at ore while a destination is held — the target \
         stays on the now-blocked cell until the drive itself ends",
    );

    // ...and the refusal to re-scan is paced by the mission cadence, not retried
    // every frame: the dispatch timer is re-anchored at this dispatch with the
    // [Harvest] Rate base plus the drawn jitter, so the next frame carries no
    // Harvest dispatch at all.
    let base = super::miner_dock_sequence::mission_base_frames(
        &rules,
        crate::sim::mission::MissionType::Harvest,
        super::miner_system::HARVEST_RATE_FALLBACK_FRAMES,
    );
    let timer = sim
        .substrate
        .entities
        .get(miner_id)
        .expect("miner entity")
        .mission
        .dispatch_timer();
    assert_eq!(
        timer.start_frame(),
        dispatch_frame as i32,
        "the epilogue re-anchors at the dispatch that ran",
    );
    assert_eq!(
        timer.delay(),
        i32::from(base) + jitter as i32,
        "held-destination return arms the [Harvest] Rate base plus the drawn jitter",
    );
    assert_eq!(
        sim.rng_state().scenario,
        expected_scenario,
        "the held-destination return draws exactly one epilogue jitter — no draw \
         leaves the stream short, a second one leaves it long, and either \
         desyncs every later scenario consumer in lockstep",
    );
    assert!(
        !timer.due(dispatch_frame + 1),
        "the Rate cadence must gate the next dispatch — a per-frame retry here \
         would be the pre-retiming VERA drift",
    );
}

/// Once the drive ends, the next due dispatch runs the state-0 body for real:
/// it re-runs the ore scan, the scan filter rejects the now-blocked cell, and
/// the miner is retargeted and re-commanded to the next-best patch.
///
/// This is the other half of the guard pinned by
/// `move_to_ore_holds_target_while_destination_is_held`: the retarget is real,
/// it is just gated on the destination clearing rather than on a per-tick
/// rescan.
#[test]
fn move_to_ore_rescans_and_rejects_blocked_cell_once_destination_clears() {
    use crate::sim::pathfinding::zone_map::ZoneGrid;
    use std::collections::BTreeMap;

    let mut sim = Simulation::new();

    spawn_inert_dock_instance(&mut sim);
    let rules = miner_rules();

    let mut grid = PathGrid::new(32, 32);
    let zone_grid = ZoneGrid::build(&grid, &BTreeMap::new(), 32, 32);
    sim.zone_grid = Some(zone_grid);

    let miner_id = spawn_drive_miner(&mut sim, 1, 8, 12);
    place_ore(&mut sim, 12, 12, 1200);
    place_ore(&mut sim, 11, 12, 1200);

    {
        let entity = sim.substrate.entities.get_mut(miner_id).expect("miner");
        entity
            .mission
            .set_handler_state(MinerState::SearchOre.cursor());
    }

    let config = MinerConfig::default();
    super::miner_system::tick_miners(&mut sim, &rules, &config, Some(&grid));

    let initial_target = get_miner(&sim, miner_id).target_ore_cell;
    let blocked_cell = initial_target.expect("the scan must pick an initial target");

    grid.set_blocked(blocked_cell.0, blocked_cell.1, true);
    sim.zone_grid = Some(ZoneGrid::build(&grid, &BTreeMap::new(), 32, 32));

    // The native trigger for re-entering the state-0 body: the destination is
    // gone (arrival, or an aborted drive), not merely a frame having passed.
    clear_outbound_drive(&mut sim, miner_id);
    sim.session.binary_frame += 1;
    arm_dispatch_now(&mut sim, miner_id);

    super::miner_system::tick_miners(&mut sim, &rules, &config, Some(&grid));

    let m = get_miner(&sim, miner_id);
    let new_target = m.target_ore_cell;
    assert_eq!(
        m.state,
        MinerState::MoveToOre,
        "a hit keeps the miner on the move-to-ore cursor",
    );
    assert_ne!(
        new_target, initial_target,
        "with the destination cleared the scan re-runs, and its filter rejects \
         the blocked cell",
    );
    assert!(new_target.is_some(), "must pick an alternative cell");

    // The retarget is not bookkeeping: the same dispatch commands the drive to
    // the newly chosen cell. This also proves the body ran at all — the
    // fixture cleared the destination immediately before the dispatch.
    let movement = sim
        .substrate
        .entities
        .get(miner_id)
        .expect("miner entity")
        .movement_target
        .as_ref()
        .expect("the retargeting dispatch must re-command the drive");
    assert_eq!(
        movement.final_goal, new_target,
        "the drive is commanded to the cell the rescan chose",
    );
}

/// The rescan must NOT thrash. Three ore cells sit on the same row, one ring
/// apart; when the destination clears and the state-0 body genuinely re-runs
/// the scan from an unmoved position in an unchanged world, it has to answer
/// the same cell it answered the first time — not flip between candidates.
///
/// Non-vacuity: state 0 keeps the current target when the scan answers
/// nothing (`new_target.unwrap_or(current_target)`), so a fixture that left
/// the first answer in place would pass on a scan that had stopped returning
/// anything at all. The target is therefore poisoned to the FARTHEST of the
/// three ore cells before the second dispatch: only a scan that genuinely
/// re-picks the nearest can put the original answer back.
#[test]
fn move_to_ore_target_stable_when_world_unchanged() {
    use crate::sim::pathfinding::zone_map::ZoneGrid;
    use std::collections::BTreeMap;

    // Farthest of the three ore cells — a live ore cell (so the depletion
    // branch does not fire) that the scan will never answer from (8, 12).
    const POISON_TARGET: (u16, u16) = (16, 12);

    let mut sim = Simulation::new();

    spawn_inert_dock_instance(&mut sim);
    let rules = miner_rules();

    let grid = PathGrid::new(32, 32);
    let zone_grid = ZoneGrid::build(&grid, &BTreeMap::new(), 32, 32);
    sim.zone_grid = Some(zone_grid);

    let miner_id = spawn_drive_miner(&mut sim, 1, 8, 12);
    place_ore(&mut sim, 14, 12, 1200);
    place_ore(&mut sim, 15, 12, 1200);
    place_ore(&mut sim, 16, 12, 1200);

    {
        let entity = sim.substrate.entities.get_mut(miner_id).expect("miner");
        entity
            .mission
            .set_handler_state(MinerState::SearchOre.cursor());
    }

    let config = MinerConfig::default();
    super::miner_system::tick_miners(&mut sim, &rules, &config, Some(&grid));
    let t1 = get_miner(&sim, miner_id).target_ore_cell;
    assert!(t1.is_some(), "the scan must pick an initial target");
    assert_ne!(
        t1,
        Some(POISON_TARGET),
        "the poison must not be the scan's own answer, or the re-scan pin below \
         is vacuous again",
    );

    // Two things are needed for the second dispatch to reach the scan at all:
    // the destination has to be gone (state 0 is a no-op while one is held),
    // and the Rate epilogue the first dispatch installed has to be re-anchored.
    // Without both, this fixture would pass on a skipped dispatch and prove
    // nothing about the scan.
    clear_outbound_drive(&mut sim, miner_id);
    // Poison the target so a scan that answers nothing can no longer be
    // mistaken for a scan that answered the same cell twice: state 0 falls
    // back to the current target on a `None`, so `t1 == t2` would otherwise
    // hold even if the scan had stopped returning anything.
    sim.substrate
        .entities
        .get_mut(miner_id)
        .expect("miner entity")
        .miner
        .as_mut()
        .expect("miner component")
        .target_ore_cell = Some(POISON_TARGET);
    sim.session.binary_frame += 1;
    arm_dispatch_now(&mut sim, miner_id);

    super::miner_system::tick_miners(&mut sim, &rules, &config, Some(&grid));
    let t2 = get_miner(&sim, miner_id).target_ore_cell;

    assert_eq!(
        t1, t2,
        "stable world → stable target across dispatches; a scan that answered \
         nothing would leave the poisoned cell in place instead",
    );
    // The dispatch really did run the body: it re-commanded the drive the
    // fixture had just cleared, and aimed it at the same cell.
    let movement = sim
        .substrate
        .entities
        .get(miner_id)
        .expect("miner entity")
        .movement_target
        .as_ref()
        .expect("the rescanning dispatch must re-command the drive");
    assert_eq!(
        movement.final_goal, t1,
        "the re-issued drive keeps the original destination",
    );
}

/// The radio bus is the only record of the dock handshake. Across a full
/// cycle its two ends must agree every tick (a one-sided contact is what let a
/// redirected miner keep pathing through its old refinery), the miner must
/// actually enter the dock, and the unload cadence and release must be clean.
#[test]
fn refinery_cycle_keeps_both_radio_ends_in_step() {
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
        miner.dock_phase = RefineryDockPhase::Approach;
        miner.reserved_refinery = Some(100);
    }

    let before = credits_for_owner(&sim, "Americans");

    let mut saw_contact = false;
    let mut saw_entered = false;
    for _ in 0..400 {
        tick_miners_n(&mut sim, &rules, 1);

        let refinery_holds_miner = crate::sim::miner::miner_dock::has_contact(&sim, 100, miner_id);
        let miner = sim.substrate.entities.get(miner_id).expect("miner");
        assert_eq!(
            refinery_holds_miner,
            miner.radio_contacts.contains(100),
            "HELLO and BREAK must update the refinery slot and the miner's contact together"
        );
        let entered = miner.dock_entered_with == Some(100);
        assert!(
            !entered || refinery_holds_miner,
            "the entered flag never outlives the contact"
        );
        saw_contact |= refinery_holds_miner;
        saw_entered |= entered;
    }

    assert!(
        saw_contact,
        "the refinery must admit the miner during the cycle"
    );
    assert!(
        saw_entered,
        "the miner must enter the dock (dock_entered_with set) during the cycle"
    );

    // Cadence unchanged: the whole ore slot deposits once (10 × 25 = 250).
    assert_eq!(
        credits_for_owner(&sim, "Americans") - before,
        250,
        "the whole ore slot deposits exactly once per cycle",
    );

    // Clean release: no lingering contact on either end, flag cleared.
    assert!(!crate::sim::miner::miner_dock::has_contact(
        &sim, 100, miner_id
    ));
    assert_eq!(
        sim.substrate
            .entities
            .get(miner_id)
            .expect("miner")
            .dock_entered_with,
        None,
    );
}

/// A full unload over the radio-bus handshake pays the exact cargo value.
#[test]
fn full_unload_credits_unchanged_over_bus() {
    let mut sim = Simulation::new();
    let rules = miner_rules();

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 14, 11);
    spawn_refinery(&mut sim, 2, 10, 10);
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
        miner.dock_phase = RefineryDockPhase::Unloading;
        miner.reserved_refinery = Some(2);
    }
    crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, miner_id);

    let before = credits_for_owner(&sim, "Americans");
    tick_miners_n(&mut sim, &rules, 1);
    assert_eq!(
        credits_for_owner(&sim, "Americans") - before,
        250,
        "the whole ore slot must still drain in one dump tick over the bus",
    );
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

/// Full harvest→dock→unload→depart cycle driven through the seam reproduces the
/// canonical miner-FSM outcome (one slot drain, full payout, cargo drained,
/// reservation released), pinning the end-to-end dispatch path.
#[test]
fn harvest_seam_dispatch_matches_miner_fsm() {
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
        miner.dock_phase = RefineryDockPhase::Approach;
        miner.reserved_refinery = Some(100);
    }

    let credits_before = credits_for_owner(&sim, "Americans");
    tick_miners_n(&mut sim, &rules, 400);

    // One slot drain → one drain event (plus the empty gate); full ore payout (10 × 25).
    assert_eq!(
        sim.bale_events.iter().filter(|e| e.drained).count(),
        1,
        "seam: one slot drain → one drain event"
    );
    assert_eq!(
        sim.bale_events.len(),
        2,
        "seam: drain gate plus the empty gate"
    );
    assert_eq!(
        credits_for_owner(&sim, "Americans") - credits_before,
        250,
        "seam: full ore payout credited (unchanged)",
    );

    let entity = sim.substrate.entities.get(miner_id).expect("entity");
    let m = entity.miner.as_ref().expect("miner");
    assert!(
        matches!(
            entity.miner_state().unwrap(),
            MinerState::SearchOre | MinerState::WaitNoOre
        ),
        "seam: post-dock state must be SearchOre or WaitNoOre, got {:?}",
        entity.miner_state().unwrap(),
    );
    assert!(m.cargo.is_empty(), "seam: cargo drained");
    assert!(m.reserved_refinery.is_none(), "seam: reservation released");
    assert!(
        !crate::sim::miner::miner_dock::test_support::dock_test_is_occupied(&sim, 100),
        "seam: dock free for the next miner",
    );
}

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
        miner.dock_phase = RefineryDockPhase::Approach;
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

/// Through the seam, the inbound dock handshake follows the verified order —
/// HELLO admits to Contacts[] (no contact-entered flag, no same-tick CAN_DOCK
/// move) → next pass advances to the accepted-cell wait — and a capacity-1
/// refinery refuses a second miner's HELLO without evicting the first (no FIFO,
/// receiver never evicts). Mirrors `hello_before_mission_enter_then_can_dock_move`
/// plus the §8 V3 no-wait-queue contract, now routed through the seam.
#[test]
fn dock_handshake_hello_enter_over_seam() {
    let mut sim = Simulation::new();
    let rules = miner_rules();

    spawn_refinery(&mut sim, 2, 10, 10);
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 14, 11);
    // Second miner (higher id → processed after miner 1 in the id-ascending
    // fallback order) contends for the same capacity-1 refinery.
    let waiter_id = spawn_miner(&mut sim, 3, MinerKind::War, 15, 11);
    for &id in &[miner_id, waiter_id] {
        let entity = sim.substrate.entities.get_mut(id).expect("miner entity");
        let miner = entity.miner.as_mut().expect("miner component");
        miner.cargo.push(CargoBale {
            resource_type: ResourceType::Ore,
            value: 25,
        });
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Approach;
        miner.reserved_refinery = Some(2);
    }

    tick_miners_n(&mut sim, &rules, 1);

    // Miner 1: HELLO accepted → MissionEnter, in Contacts[], NOT entered, no
    // same-tick CAN_DOCK move.
    assert_eq!(
        get_miner(&sim, miner_id).dock_phase,
        RefineryDockPhase::MissionEnter
    );
    assert!(
        crate::sim::miner::miner_dock::has_contact(&sim, 2, miner_id),
        "seam: HELLO/ROGER must populate Contacts[]",
    );
    assert!(
        !crate::sim::miner::miner_dock::has_entered(&sim, 2, miner_id),
        "seam: contact-entered flag must not be set by HELLO",
    );
    assert!(
        sim.substrate
            .entities
            .get(miner_id)
            .expect("entity")
            .movement_target
            .is_none(),
        "seam: HELLO acceptance must not issue the CAN_DOCK move the same tick",
    );

    // Capacity-1 refusal: the waiter gets no contact and the first miner is NOT
    // evicted (no FIFO, receiver never evicts).
    assert!(
        !crate::sim::miner::miner_dock::has_contact(&sim, 2, waiter_id),
        "seam: a saturated refinery must refuse the second HELLO (no second contact)",
    );
    assert!(
        crate::sim::miner::miner_dock::has_contact(&sim, 2, miner_id),
        "seam: the first contact must NOT be evicted by the second HELLO",
    );

    // Next pass: miner 1 advances to the accepted-cell wait, still not entered.
    // G5: the accepted HELLO arms the Enter cadence; advance the frame clock past
    // the ~14-16f window so this pass's CAN_DOCK dispatch is due.
    sim.session.binary_frame = sim.session.binary_frame.wrapping_add(18);
    tick_miners_n(&mut sim, &rules, 1);
    assert_eq!(
        get_miner(&sim, miner_id).dock_phase,
        RefineryDockPhase::AwaitingAcceptedCell,
    );
    assert!(
        !crate::sim::miner::miner_dock::has_entered(&sim, 2, miner_id),
        "seam: not at accepted cell yet — no contact-entered admission",
    );
}

/// Through the seam, the first slot drain is gated by the 14.4-tick accumulator
/// (`acc*10 >= unload_tick_interval`): with tenths precision the first ore-slot
/// drain lands in the 15–16 unloading-tick window, not immediately. Mirrors
/// `dock_first_slot_drain_waits_one_unload_interval`, routed through the seam.
#[test]
fn deposit_cadence_14_4_ticks_over_seam() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let config = MinerConfig::default();

    spawn_refinery(&mut sim, 2, 10, 10);
    // Pad cell facing East so the pivot completes in two ticks and the dump
    // gate timing lines up cleanly.
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 13, 11);
    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        entity.movement_target = None;
        entity.facing = 0x40;
        let miner = entity.miner.as_mut().expect("miner component");
        for _ in 0..5 {
            miner.cargo.push(CargoBale {
                resource_type: ResourceType::Ore,
                value: config.ore_bale_value,
            });
        }
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::MissionQueued;
        miner.reserved_refinery = Some(2);
    }
    crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, miner_id);

    // Two ticks: MissionQueued → Pivoting → Unloading (no drain yet).
    tick_miners_n(&mut sim, &rules, 2);
    assert_eq!(
        get_miner(&sim, miner_id).cargo.len(),
        5,
        "no drain before Unloading"
    );
    assert_eq!(
        get_miner(&sim, miner_id).dock_phase,
        RefineryDockPhase::Unloading
    );

    let mut drain_tick = None;
    for elapsed in 1..=20 {
        tick_miners_n(&mut sim, &rules, 1);
        if get_miner(&sim, miner_id).cargo.is_empty() {
            drain_tick = Some(elapsed);
            break;
        }
    }
    let drain_tick = drain_tick.expect("slot must drain within the timing window");
    assert!(
        (15..=16).contains(&drain_tick),
        "seam: first slot drain gated by the 14.4-tick accumulator, got tick {drain_tick}",
    );
}

/// Through the seam, `tick_unload_accumulator` runs AFTER `phase_unloading`
/// samples the accumulator (call order `handle_dock_sequence` :792 then :802).
/// Proof: at the tick the slot drains, the accumulator value present at the
/// START of that tick (the previous tick's post-increment value) ALREADY meets
/// the gate by itself — under increment-before-sample the start-of-tick value
/// would still be below the gate (the same-tick increment would push it over),
/// firing the drain one tick earlier. Resolves the §8 NEEDS-PROOF ordering.
#[test]
fn unload_accumulator_sample_before_increment() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let config = MinerConfig::default();
    let interval = i32::from(config.unload_tick_interval);
    let gate_met = |acc: i32| acc >= interval;

    spawn_refinery(&mut sim, 2, 10, 10);
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 13, 11);
    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        entity.movement_target = None;
        entity.facing = 0x40;
        let miner = entity.miner.as_mut().expect("miner component");
        for _ in 0..5 {
            miner.cargo.push(CargoBale {
                resource_type: ResourceType::Ore,
                value: config.ore_bale_value,
            });
        }
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::MissionQueued;
        miner.reserved_refinery = Some(2);
    }
    crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, miner_id);

    // Reach Unloading via the real pivot path (seeds the accumulator at 0 and
    // arms the cluster timer — NOT the save-compat fast-seed branch).
    tick_miners_n(&mut sim, &rules, 2);
    assert_eq!(
        get_miner(&sim, miner_id).dock_phase,
        RefineryDockPhase::Unloading
    );

    // accs[i] = accumulator AFTER the i-th unloading tick (i.e. the value the
    // (i+1)-th tick's phase will sample, since the increment is the tick tail).
    let mut accs: Vec<i32> = Vec::new();
    let mut drain_index: Option<usize> = None;
    for i in 0..30 {
        let cargo_before = get_miner(&sim, miner_id).cargo.len();
        tick_miners_n(&mut sim, &rules, 1);
        accs.push(get_miner(&sim, miner_id).unload_accumulator);
        if cargo_before > 0 && get_miner(&sim, miner_id).cargo.is_empty() {
            drain_index = Some(i);
            break;
        }
    }
    let d = drain_index.expect("slot must drain within the window");
    assert!(
        d >= 2,
        "need at least two prior ticks to compare gate crossings"
    );

    // Value the drain tick's phase sampled = accumulator at the start of tick d
    // = accs[d-1] (previous tick's post-increment value).
    assert!(
        gate_met(accs[d - 1]),
        "sample-before-increment: the start-of-drain-tick accumulator ({}) must \
         already meet the gate on its own",
        accs[d - 1],
    );
    // One tick earlier the sampled value was below the gate, so no earlier drain
    // was possible — confirms the drain fires at the first true gate crossing.
    assert!(
        !gate_met(accs[d - 2]),
        "no early drain: the accumulator sampled one tick before ({}) must be \
         below the gate",
        accs[d - 2],
    );
}

/// Through the seam, mixed cargo drains in the fixed slot order Ore-then-Gem:
/// the first dump-gate crossing drains all ore (leaving only gems), the second
/// drains all gems, one `BaleDepositEvent` per slot, credited to the refinery
/// owner. Mirrors `unloading_emits_one_event_per_slot_drain` with an explicit
/// order assertion, routed through the seam.
#[test]
fn deposit_slot_order_ore_then_gem_over_seam() {
    let mut sim = Simulation::new();
    let rules = miner_rules();

    spawn_refinery(&mut sim, 2, 10, 10);
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 13, 11);
    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        let miner = entity.miner.as_mut().expect("miner component");
        for _ in 0..5 {
            miner.cargo.push(CargoBale {
                resource_type: ResourceType::Ore,
                value: 25,
            });
        }
        for _ in 0..3 {
            miner.cargo.push(CargoBale {
                resource_type: ResourceType::Gem,
                value: 50,
            });
        }
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Unloading;
        miner.reserved_refinery = Some(2);
    }
    crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, miner_id);

    let credits_before = credits_for_owner(&sim, "Americans");

    // Single-step until the first slot drains (cargo 8 → 3).
    let mut after_first = None;
    for _ in 0..60 {
        tick_miners_n(&mut sim, &rules, 1);
        if get_miner(&sim, miner_id).cargo.len() == 3 {
            after_first = Some(());
            break;
        }
    }
    after_first.expect("first slot must drain");
    let m = get_miner(&sim, miner_id);
    assert!(
        m.cargo.iter().all(|b| b.resource_type == ResourceType::Gem),
        "seam: Ore slot drains first — only Gems remain after the first crossing",
    );
    assert_eq!(
        sim.bale_events.len(),
        1,
        "seam: one event after the first slot drain"
    );
    assert_eq!(
        credits_for_owner(&sim, "Americans") - credits_before,
        5 * 25,
        "seam: ore slot credits the refinery owner first (5 × 25)",
    );

    // Continue until the gem slot drains (cargo 3 → 0).
    let mut after_second = None;
    for _ in 0..60 {
        tick_miners_n(&mut sim, &rules, 1);
        if get_miner(&sim, miner_id).cargo.is_empty() {
            after_second = Some(());
            break;
        }
    }
    after_second.expect("second slot must drain");
    assert_eq!(
        sim.bale_events.len(),
        2,
        "seam: one event per slot (ore, then gem)"
    );
    assert_eq!(
        credits_for_owner(&sim, "Americans") - credits_before,
        5 * 25 + 3 * 50,
        "seam: gem slot credits second (total 125 + 150)",
    );
}

/// Regression for the reported "miner removes ore one cell behind" symptom.
/// Drive onto the same ore cell from all four cardinal directions and verify
/// both the simulation resource and its render overlay use the arrived cell.
#[test]
fn coordinate_runtime_trace_miner_arrival_and_extraction_four_directions() {
    let target = (20_u16, 20_u16);
    let approaches = [
        ("west", (19_u16, 20_u16), (21_u16, 20_u16)),
        ("east", (21_u16, 20_u16), (19_u16, 20_u16)),
        ("north", (20_u16, 19_u16), (20_u16, 21_u16)),
        ("south", (20_u16, 21_u16), (20_u16, 19_u16)),
    ];

    for (label, start, behind) in approaches {
        let mut sim = Simulation::new();
        spawn_inert_dock_instance(&mut sim);
        let rules = miner_rules();
        place_ore(&mut sim, target.0, target.1, 120);
        place_ore(&mut sim, behind.0, behind.1, 120);

        let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, start.0, start.1);
        {
            let entity = sim
                .substrate
                .entities
                .get_mut(miner_id)
                .expect("miner entity");
            let miner = entity.miner.as_mut().expect("miner component");
            entity
                .mission
                .set_handler_state(MinerState::MoveToOre.cursor());
            miner.target_ore_cell = Some(target);
            miner.harvest_timer.clear();
        }

        let mut first_harvest_tick = None;
        let mut extraction_tick = None;

        for trace_tick in 0..512_u32 {
            let target_before = crate::sim::tiberium::test_support::has_tiberium(&sim, target);
            tick_miners_n(&mut sim, &rules, 1);

            let entity = sim
                .substrate
                .entities
                .get(miner_id)
                .expect("miner remains alive");
            let state = entity.miner_state().expect("valid miner state");
            let sub_x = entity.position.sub_x.to_num::<i32>();
            let sub_y = entity.position.sub_y.to_num::<i32>();

            if state == MinerState::Harvest && first_harvest_tick.is_none() {
                eprintln!(
                    "MINER_TRACE approach={label} event=enter_harvest trace_tick={trace_tick} \
                     sim_tick={} cell=({},{}) sub=({sub_x},{sub_y}) \
                     target={target:?} movement_target={}",
                    sim.session.tick,
                    entity.position.rx,
                    entity.position.ry,
                    entity.movement_target.is_some(),
                );
                assert_eq!(
                    (entity.position.rx, entity.position.ry),
                    target,
                    "{label}: Harvest must begin on the selected ore cell"
                );
                assert_eq!(
                    (sub_x, sub_y),
                    (128, 128),
                    "{label}: Harvest must begin at cell center"
                );
                assert!(
                    entity.movement_target.is_none(),
                    "{label}: Harvest must not begin while movement is still active"
                );
                first_harvest_tick = Some(trace_tick);
            }

            let target_after = crate::sim::tiberium::test_support::has_tiberium(&sim, target);
            if target_before && !target_after {
                let overlay = sim.overlay_grid.as_ref().expect("overlay grid");
                let target_overlay_cleared = overlay.cell(target.0, target.1).overlay_id.is_none();
                let behind_overlay_preserved =
                    overlay.cell(behind.0, behind.1).overlay_id.is_some();
                eprintln!(
                    "MINER_TRACE approach={label} event=extract trace_tick={trace_tick} \
                     sim_tick={} cell=({},{}) sub=({sub_x},{sub_y}) target_removed={target:?} \
                     target_overlay_cleared={target_overlay_cleared} \
                     behind_resource_preserved={} behind_overlay_preserved={behind_overlay_preserved} \
                     cargo_bales={}",
                    sim.session.tick,
                    entity.position.rx,
                    entity.position.ry,
                    crate::sim::tiberium::test_support::has_tiberium(&sim, behind),
                    entity.miner.as_ref().expect("miner component").cargo.len(),
                );
                assert_eq!(
                    (entity.position.rx, entity.position.ry),
                    target,
                    "{label}: extraction must use the miner's current cell"
                );
                assert_eq!(
                    (sub_x, sub_y),
                    (128, 128),
                    "{label}: extraction must occur at cell center"
                );
                assert!(
                    crate::sim::tiberium::test_support::has_tiberium(&sim, behind),
                    "{label}: ore behind the target must remain untouched"
                );
                assert!(
                    target_overlay_cleared,
                    "{label}: the renderer's target overlay cell must clear"
                );
                assert!(
                    behind_overlay_preserved,
                    "{label}: the renderer's behind-cell overlay must remain occupied"
                );
                extraction_tick = Some(trace_tick);
                break;
            }
        }

        assert!(
            first_harvest_tick.is_some(),
            "{label}: miner never entered Harvest"
        );
        assert!(
            extraction_tick.is_some(),
            "{label}: miner never extracted the target ore"
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

    tick_miners_n(&mut sim, &rules, 1);

    assert_eq!(
        get_miner(&sim, miner_id).reserved_refinery,
        Some(3),
        "own-house refinery must win over a nearer foreign one",
    );
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

    tick_miners_n(&mut sim, &rules, 1);

    assert_eq!(
        get_miner(&sim, miner_id).reserved_refinery,
        Some(3),
        "own-house refinery must win over a nearer allied one",
    );
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

/// Narrow pass inside HarvesterTooFarDistance (5 cells): the nearer refinery
/// with a docked miner (holding its single `Contacts[]` slot and the pad;
/// `FUN_0065ADF0` false → scanner skip / Receive_Radio 0xF returns 10) loses
/// to the farther free one.
#[test]
fn refinery_selection_narrow_pass_skips_docked_refinery_within_close_radius() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 5, 10);
    spawn_refinery(&mut sim, 2, 6, 10);
    spawn_refinery(&mut sim, 3, 8, 10);
    occupy_refinery(&mut sim, 2, 99);
    fill_and_return(&mut sim, miner_id);

    tick_miners_n(&mut sim, &rules, 1);

    assert_eq!(
        get_miner(&sim, miner_id).reserved_refinery,
        Some(3),
        "docked near refinery must lose to the farther free refinery",
    );
}

/// Same as the docked case for a HARV: full `Contacts[]` alone rejects the
/// nearer refinery in the narrow pass.
#[test]
fn refinery_selection_narrow_pass_skips_full_contacts_for_harv() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 5, 10);
    spawn_refinery(&mut sim, 2, 6, 10);
    spawn_refinery(&mut sim, 3, 8, 10);
    occupy_refinery(&mut sim, 2, 99);
    fill_and_return(&mut sim, miner_id);

    tick_miners_n(&mut sim, &rules, 1);

    assert_eq!(get_miner(&sim, miner_id).reserved_refinery, Some(3));
}

/// Narrow pass: full `Contacts[]` (`FUN_0065ADF0` false) rejects the nearer
/// refinery; chrono miners use ChronoHarvTooFarDistance (50 cells) so the
/// farther free refinery is still a narrow-pass HELLO target.
#[test]
fn refinery_selection_narrow_pass_skips_full_contacts_for_chrono() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::Chrono, 5, 10);
    spawn_refinery(&mut sim, 2, 8, 10);
    spawn_refinery(&mut sim, 3, 20, 10);
    occupy_refinery(&mut sim, 2, 99);
    fill_and_return(&mut sim, miner_id);

    tick_miners_n(&mut sim, &rules, 1);

    assert_eq!(
        get_miner(&sim, miner_id).reserved_refinery,
        Some(3),
        "saturated near refinery must lose to the farther free one",
    );
}

/// The narrow result is only used inside the too-far distance. A HARV whose
/// only free refinery is 15 cells away falls to the wide pass, which picks
/// the nearest own refinery even with a miner docked (drive up and wait).
#[test]
fn refinery_selection_wide_pass_when_free_refinery_is_beyond_too_far() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 5, 10);
    spawn_refinery(&mut sim, 2, 8, 10);
    spawn_refinery(&mut sim, 3, 20, 10);
    occupy_refinery(&mut sim, 2, 99);
    fill_and_return(&mut sim, miner_id);

    tick_miners_n(&mut sim, &rules, 1);

    assert_eq!(
        get_miner(&sim, miner_id).reserved_refinery,
        Some(2),
        "beyond HarvesterTooFarDistance the wide pass takes the nearest refinery",
    );
}

/// Wide fallback: every own refinery saturated and occupied still yields the
/// nearest one; the miner keeps returning instead of idling.
#[test]
fn refinery_selection_wide_pass_falls_back_to_occupied_refinery() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 5, 10);
    spawn_refinery(&mut sim, 2, 10, 10);
    occupy_refinery(&mut sim, 2, 99);
    fill_and_return(&mut sim, miner_id);

    tick_miners_n(&mut sim, &rules, 1);

    let m = get_miner(&sim, miner_id);
    assert_eq!(m.reserved_refinery, Some(2));
    assert_ne!(m.state, MinerState::WaitNoOre);
}

/// A miner already in a refinery's `Contacts[]` passes the narrow probe at
/// capacity (`FUN_0065ADF0` matches the caller), so it is not evicted to a
/// farther refinery.
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

    tick_miners_n(&mut sim, &rules, 1);

    assert_eq!(get_miner(&sim, miner_id).reserved_refinery, Some(2));
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

    tick_miners_n(&mut sim, &rules, 1);

    assert_eq!(
        get_miner(&sim, miner_id).reserved_refinery,
        Some(3),
        "unreachable-zone refinery must be skipped for the reachable one",
    );
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

    assert_eq!(
        get_miner(&sim, miner_id).reserved_refinery,
        Some(3),
        "free refinery inside HarvesterTooFarDistance by centre distance must win the narrow pass",
    );
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
         Refinery=yes\n\
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

/// Miner on the pad facing East, `MissionQueued`, with the given cargo and a
/// radio contact (the HELLO admission the dock FSM reads).
fn spawn_queued_unload_miner(sim: &mut Simulation, cargo: &[(ResourceType, u16)]) -> u64 {
    spawn_refinery(sim, 2, 10, 10);
    let miner_id = spawn_miner(sim, 1, MinerKind::War, 13, 11);
    {
        let entity = sim
            .substrate
            .entities
            .get_mut(miner_id)
            .expect("miner entity");
        entity.movement_target = None;
        entity.facing = 0x40;
        let miner = entity.miner.as_mut().expect("miner component");
        for (resource_type, value) in cargo {
            miner.cargo.push(CargoBale {
                resource_type: *resource_type,
                value: *value,
            });
        }
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::MissionQueued;
        miner.reserved_refinery = Some(2);
    }
    assert!(crate::sim::miner::miner_dock::test_support::dock_test_hello(sim, 2, miner_id));
    // The frame tail's smoke spawn registers the particle system in the live
    // object order; once that order is non-empty `tick_miners` walks only it,
    // so both fixture objects must be members too.
    for id in [2, miner_id] {
        sim.substrate.logic.try_push(id).expect("logic slot");
        sim.substrate
            .entities
            .get_mut(id)
            .expect("fixture entity")
            .in_logic_vector = true;
    }
    miner_id
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
    ticks: usize,
) -> UnloadPresentationTrace {
    let special = sim.interner.intern("GAREFNOR");
    let mut trace = UnloadPresentationTrace {
        smoke_count: Vec::with_capacity(ticks),
        slot_live: Vec::with_capacity(ticks),
    };
    for _ in 0..ticks {
        tick_miners_n(sim, rules, 1);
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
    let miner_id = spawn_queued_unload_miner(&mut sim, &[(ResourceType::Ore, 25); 5]);

    let trace = trace_unload_presentation(&mut sim, &rules, 60);

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
    let miner_id = spawn_queued_unload_miner(&mut sim, &cargo);

    let trace = trace_unload_presentation(&mut sim, &rules, 80);

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

/// Contact gone between MissionQueued and unload start (`0x0073DEE0`):
/// `In_Radio_Contact` false → `Enter_Idle_Mode(0,1)` (no mission assigned
/// while the current mission is Unload), `+0x6D1 = 0`, Stop_Moving, Commence of
/// a queued mission only, `return 1`. The miner abandons the unload without
/// depositing and parks, re-dispatching each frame, until a mission is queued.
#[test]
fn contact_gone_at_unload_start_abandons_unload_without_deposit() {
    use crate::sim::mission::{MissionId, MissionType};

    let mut sim = Simulation::new();
    let rules = miner_rules_with_refinery_art();
    let capacity = {
        let m = Miner::new(MinerKind::War, &MinerConfig::default(), 0);
        m.capacity_bales as usize
    };
    let cargo = vec![(ResourceType::Ore, 25u16); capacity];
    let miner_id = spawn_queued_unload_miner(&mut sim, &cargo);
    let credits_before = credits_for_owner(&sim, "Americans");

    // Tick 1: MissionQueued → Pivoting (radio 0x15 only queued mission 0x10).
    tick_miners_n(&mut sim, &rules, 1);
    assert_eq!(
        get_miner(&sim, miner_id).dock_phase,
        RefineryDockPhase::Pivoting
    );

    // The fixture HELLOed over the bus, so the exit's release is observable
    // on both ends: the refinery's radio slot and the miner's own contact.
    assert!(
        sim.substrate
            .entities
            .get(2)
            .expect("refinery")
            .radio_contacts
            .contains(miner_id)
    );

    // The refinery drops the contact before the Unload dispatch runs.
    crate::sim::miner::miner_dock::break_contact(&mut sim, miner_id, 2);

    tick_miners_n(&mut sim, &rules, 40);
    crate::sim::world::building_anim::finalize(&mut sim, &[], true, Some(&rules));

    let m = get_miner(&sim, miner_id);
    assert_eq!(m.cargo.len(), capacity, "no slot drained without a contact");
    assert_eq!(
        credits_for_owner(&sim, "Americans"),
        credits_before,
        "no deposit credited"
    );
    assert!(sim.bale_events.is_empty(), "no dump-gate event emitted");
    assert!(
        sim.particle_systems().is_empty(),
        "no refinery smoke burst without a contact"
    );
    assert!(!m.unload_active, "+0x6D1 unload latch cleared");
    assert_eq!(
        m.dock_phase,
        RefineryDockPhase::Pivoting,
        "parked in the Unload-equivalent, re-dispatching each frame"
    );
    assert_eq!(m.state, MinerState::Dock);
    let entity = sim.substrate.entities.get(miner_id).expect("miner");
    assert_eq!(
        entity.display_type_override, None,
        "UnloadingClass image dropped with the latch"
    );
    assert!(entity.movement_target.is_none(), "Stop_Moving");

    // A queued mission is what ends the loop: Is_Ready_To_Commence → Commence
    // promotes it and the dock sequence is left through the zeroed cursor.
    let now = sim.session.binary_frame;
    sim.mission_queue_exact(
        miner_id,
        MissionId::from_known(MissionType::Harvest),
        0,
        now,
        &crate::sim::mission::authority::EntityReadyInputProvider,
    )
    .expect("miner exists");
    tick_miners_n(&mut sim, &rules, 1);
    let m = get_miner(&sim, miner_id);
    assert_ne!(
        m.state,
        MinerState::Dock,
        "commenced mission leaves the dock"
    );
    assert_eq!(m.dock_phase, RefineryDockPhase::Approach);
    assert_eq!(m.reserved_refinery, None);
    assert_eq!(m.cargo.len(), capacity, "cargo still intact");
    // The exit releases the contact the way the state-4 exit does: bus BREAK
    // and the miner's live-contact mirror both drop.
    let entity = sim.substrate.entities.get(miner_id).expect("miner");
    assert!(
        !entity.has_live_contact_with(2),
        "commenced exit clears the miner's live contact"
    );
    assert_eq!(
        entity.dock_entered_with, None,
        "bus BREAK clears dock_entered_with"
    );
    assert!(
        !sim.substrate
            .entities
            .get(2)
            .expect("refinery")
            .radio_contacts
            .contains(miner_id),
        "bus BREAK frees the refinery radio slot"
    );
    assert!(!crate::sim::miner::miner_dock::has_contact(
        &sim, 2, miner_id
    ));
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
    let miner_id = spawn_queued_unload_miner(&mut sim, &[(ResourceType::Ore, 25); 5]);
    {
        let refinery = sim.substrate.entities.get_mut(2).expect("refinery");
        refinery.health.current = rules.object("GAREFN").expect("refinery type").strength / 2;
    }

    let trace = trace_unload_presentation(&mut sim, &rules, 60);

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
/// exact-zero Destroy (`0x005F57AF`, Detach_All(1)) drops the reservation and
/// the radio slot at the killing hit, touching neither cargo, motion nor
/// credits, and the miner's own next dock visit aborts to Approach. Native4424A2
/// gates Force release4593A0 on reciprocal bunker+2E4, which refinery contacts
/// do not satisfy. The building NowDead contact loop (`0x00442511`: radio 0x17,
/// or a C4 kill within 0x100 leptons of the centre or on a Helipad=yes
/// building, over the pre-hit contact copy) is not ported yet.
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
         Owner=Americans\nFoundation=4x3\nRefinery=yes\n\
         [KILLWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("refinery kill rules");

    let mut sim = Simulation::new();
    let owner = sim.interner.intern("Americans");
    sim.houses
        .insert(owner, HouseState::new(owner, 0, None, false, 0, 10));
    sim.session.house_order = vec![owner];
    sim.session.binary_frame = 40;

    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 11, 11);
    spawn_refinery(&mut sim, 2, 10, 10);
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
        let miner = entity.miner.as_mut().expect("miner component");
        for _ in 0..4 {
            miner.cargo.push(CargoBale {
                resource_type: ResourceType::Ore,
                value: 25,
            });
        }
        entity.mission.set_handler_state(MinerState::Dock.cursor());
        miner.dock_phase = RefineryDockPhase::Unloading;
        miner.reserved_refinery = Some(2);
        miner.unload_active = true;
    }
    crate::sim::miner::miner_dock::test_support::dock_test_hello(&mut sim, 2, miner_id);
    let credits_before = credits_for_owner(&sim, "Americans");
    let (position_before, drive_before, speed_before, cargo_before) = {
        let entity = sim.substrate.entities.get(miner_id).unwrap();
        (
            crate::sim::movement::ground_pose::position_world_coord(&entity.position),
            entity.drive_locomotion.clone(),
            entity.foot_speed.clone(),
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
    assert_eq!(
        miner.reserved_refinery, None,
        "Detach_All drops the reservation at the kill"
    );
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
    assert_eq!(miner_entity.drive_locomotion, drive_before);
    assert_eq!(miner_entity.foot_speed, speed_before);
    assert_eq!(miner.cargo, cargo_before);
    assert_eq!(
        credits_for_owner(&sim, "Americans"),
        credits_before,
        "the cargo on the pad is not deposited"
    );

    // The miner's own dock visit finds its refinery gone and stops unloading.
    tick_miners_n(&mut sim, &rules, 1);
    let miner = get_miner(&sim, miner_id);
    assert_eq!(miner.dock_phase, RefineryDockPhase::Approach);
    assert!(!miner.unload_active, "no deposit continues");
    assert_eq!(miner.cargo.len(), 4, "remaining cargo stays aboard");

    // Nothing pays the bales later either: no refinery is left to dock at.
    tick_miners_n(&mut sim, &rules, 60);
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
        .set_handler_state(MinerState::MoveToOre.cursor());
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
        e.miner.as_ref().unwrap().target_ore_cell.is_some()
            || MinerState::from_cursor(e.mission.handler_state()).is_some(),
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
