//! `UnitClass::AI`'s stuck-harvester nudge (`0x0073681D..0x0073697B`) on
//! retail rules.

use crate::rules::ruleset::RuleSet;
use crate::sim::components::NavTargetRef;
use crate::sim::miner::miner_system::unit_ai_nudge_off_refinery_pad;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::pathfinding::PathGrid;
use crate::sim::radio::{RadioMessage, RadioPayload, RadioResponse, transmit};
use crate::sim::world::{FrameEffects, Simulation};

/// The rival arena on retail rules: `Russians` a computer house, made
/// passive so its Strategy does not send a base-less house hunting, and
/// `Americans` a human one.
fn scene() -> Option<(Simulation, RuleSet, PathGrid)> {
    let rules = crate::rules::retail_ini_fixture::retail_battle_rules()?.rules;
    let mut sim = crate::sim::arena_fixture::rival_arena_with_size(&rules, 0x0073_681D, 40);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    sim.session
        .set_current_house_for_test(sim.interner.get("Americans"));
    let russians = sim.interner.get("Russians").unwrap();
    sim.houses
        .get_mut(&russians)
        .unwrap()
        .set_multiplay_passive_for_test(true);
    sim.session.set_binary_frame_for_test(100);
    let grid = sim
        .path_grid_snapshot()
        .map(|grid| (*grid).clone())
        .unwrap();
    Some((sim, rules, grid))
}

fn spawn(sim: &mut Simulation, rules: &RuleSet, kind: &str, owner: &str, x: u16, y: u16) -> u64 {
    sim.spawn_object_at_height(kind, owner, x, y, 0, 0, rules)
        .unwrap_or_else(|| panic!("{kind} spawns at ({x}, {y})"))
}

/// A `kind` of `owner` on Harvest since frame 100, standing at `cell`.
fn miner_at(
    sim: &mut Simulation,
    rules: &RuleSet,
    kind: &str,
    owner: &str,
    cell: (u16, u16),
) -> u64 {
    let id = spawn(
        sim,
        rules,
        kind,
        owner,
        30,
        30 + sim.substrate.entities.len() as u16 % 8,
    );
    let _ = sim.mission_assign_exact(id, MissionId::from_known(MissionType::Harvest), 100);
    let position = &mut sim.substrate.entities.get_mut(id).unwrap().position;
    position.set_rx_for_test(cell.0);
    position.set_ry_for_test(cell.1);
    id
}

fn nudge(sim: &mut Simulation, rules: &RuleSet, id: u64) {
    unit_ai_nudge_off_refinery_pad(sim, id, rules, None, FrameEffects::default());
}

fn moving(sim: &Simulation, id: u64) -> bool {
    let unit = sim.substrate.entities.get(id).unwrap();
    let move_ = MissionId::from_known(MissionType::Move);
    unit.mission.current() == move_ || unit.mission.queued() == move_
}

/// A computer miner on its refinery's dock Cell (the refinery's Cell plus
/// (3, 1)) more than 300 frames into its mission, standing still, says
/// OVER_OUT to its refinery, drives to a Cell near it and moves now.
#[test]
fn a_stuck_computer_miner_drives_off_its_refinery_pad() {
    let Some((mut sim, rules, _)) = scene() else {
        return;
    };
    let refinery = spawn(&mut sim, &rules, "GAREFN", "Russians", 10, 10);
    let miner = miner_at(&mut sim, &rules, "HARV", "Russians", (13, 11));
    let hello = transmit(
        &mut sim,
        miner,
        refinery,
        RadioMessage::Hello,
        RadioPayload::default(),
        Some(&rules),
        FrameEffects::default(),
    );
    assert_eq!(hello, RadioResponse::Roger);
    assert_eq!(
        sim.substrate
            .entities
            .get(miner)
            .unwrap()
            .radio_contacts
            .slot(0),
        Some(refinery)
    );
    sim.session.set_binary_frame_for_test(401);
    let exit = sim
        .techno_nearby_location(miner, Some(refinery), &rules)
        .unwrap();

    nudge(&mut sim, &rules, miner);

    let unit = sim.substrate.entities.get(miner).unwrap();
    assert!(
        unit.radio_contacts.is_empty(),
        "OVER_OUT breaks the contact"
    );
    assert!(moving(&sim, miner));
    assert_eq!(
        unit.navigation.nav_com(),
        Some(NavTargetRef::cell(exit.0 as u16, exit.1 as u16))
    );
}

/// The gates of `0x0073681D..0x00736908`: `Harvester=`, a computer House,
/// more than 300 frames into the mission, and the dock Cell exactly.
#[test]
fn only_a_long_stuck_computer_miner_on_the_dock_cell_is_nudged() {
    let Some((mut sim, rules, _)) = scene() else {
        return;
    };
    spawn(&mut sim, &rules, "GAREFN", "Russians", 10, 10);
    spawn(&mut sim, &rules, "GAREFN", "Americans", 20, 10);
    sim.session.set_binary_frame_for_test(400);
    let early = miner_at(&mut sim, &rules, "HARV", "Russians", (13, 11));
    nudge(&mut sim, &rules, early);
    assert!(!moving(&sim, early), "300 frames is not enough");
    sim.session.set_binary_frame_for_test(401);
    nudge(&mut sim, &rules, early);
    assert!(moving(&sim, early));

    let human = miner_at(&mut sim, &rules, "HARV", "Americans", (23, 11));
    nudge(&mut sim, &rules, human);
    assert!(!moving(&sim, human));

    let beside = miner_at(&mut sim, &rules, "HARV", "Russians", (12, 11));
    nudge(&mut sim, &rules, beside);
    assert!(!moving(&sim, beside));

    let tank = miner_at(&mut sim, &rules, "HTNK", "Russians", (13, 11));
    nudge(&mut sim, &rules, tank);
    assert!(!moving(&sim, tank));
}

/// A miner on the pad whose locomotor is moving (Drive `Is_Moving`: a
/// destination) is left alone.
#[test]
fn a_moving_miner_on_the_pad_is_left_alone() {
    let Some((mut sim, rules, _)) = scene() else {
        return;
    };
    spawn(&mut sim, &rules, "GAREFN", "Russians", 10, 10);
    let miner = miner_at(&mut sim, &rules, "HARV", "Russians", (13, 11));
    let _ = sim.assign_destination_represented(
        miner,
        Some(NavTargetRef::cell(30, 20)),
        Some(&rules),
        None,
        FrameEffects::default(),
    );
    assert_eq!(
        crate::sim::movement::motion_query::is_moving(sim.substrate.entities.get(miner).unwrap()),
        Some(true)
    );
    sim.session.set_binary_frame_for_test(401);

    nudge(&mut sim, &rules, miner);

    assert!(!moving(&sim, miner));
}

/// Through the production frame: the block runs at the end of the Unit's
/// object turn.
#[test]
fn the_production_frame_nudges_a_stuck_computer_miner() {
    let Some((mut sim, rules, grid)) = scene() else {
        return;
    };
    // Unlimbo marks the miner into the pad Cell, as docking leaves it.
    spawn(&mut sim, &rules, "GAREFN", "Russians", 10, 10);
    let miner = spawn(&mut sim, &rules, "HARV", "Russians", 13, 11);
    let _ = sim.mission_assign_exact(miner, MissionId::from_known(MissionType::Sleep), 100);
    sim.session.set_binary_frame_for_test(401);

    let commands = sim.take_due_commands();
    sim.advance_tick(&commands, Some(&rules), Some(&grid), None, 33);

    assert!(moving(&sim, miner));
}
