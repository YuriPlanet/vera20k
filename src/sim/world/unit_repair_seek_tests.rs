use super::*;
use crate::sim::arena_fixture::{flood, rival_arena_with_size};
use crate::sim::pathfinding::PathGrid;

const ARENA: u16 = 48;

/// The retail-rules rival arena (`Russians` a computer house, `Americans`
/// human) at frame 160, a multiple of 16, with its path grid.
fn scene() -> Option<(Simulation, RuleSet, PathGrid)> {
    let rules = crate::rules::retail_ini_fixture::retail_battle_rules()?.rules;
    let mut sim = rival_arena_with_size(&rules, 0x0073_671F, ARENA);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    sim.session
        .set_current_house_for_test(sim.interner.get("Americans"));
    sim.session.set_binary_frame_for_test(160);
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

/// A stock HARV naturally misses every ore overlay, receives an explicit
/// Repair order and establishes the depot contact while still human-owned.
/// The next caller can exercise `BuildingClass::MissionRepairAndProduce`'s
/// current-owner gate without manufacturing the native useless latch.
fn useless_harvester_contact() -> Option<(Simulation, RuleSet, PathGrid, u64, u64, u64)> {
    let Some((mut sim, rules, grid)) = scene() else {
        return None;
    };
    let americans = sim.interner.get("Americans").unwrap();
    let russians = sim.interner.get("Russians").unwrap();
    sim.houses
        .get_mut(&russians)
        .unwrap()
        .set_multiplay_passive_for_test(true);
    sim.houses
        .get_mut(&russians)
        .unwrap()
        .economy_mut()
        .set_credits_for_test(10_000);
    sim.houses
        .get_mut(&americans)
        .unwrap()
        .economy_mut()
        .set_credits_for_test(10_000);
    sim.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(48, 48));
    let targeter = spawn(&mut sim, &rules, "GAPOWR", "Americans", 40, 40);
    spawn(&mut sim, &rules, "NAPOWR", "Russians", 43, 43);
    spawn(&mut sim, &rules, "GAREFN", "Americans", 10, 10);
    let depot = spawn(&mut sim, &rules, "GADEPT", "Americans", 24, 20);
    let harvester = spawn(&mut sim, &rules, "HARV", "Americans", 13, 11);
    let _ = sim.substrate.mission_assign_exact(
        harvester,
        MissionId::from_known(MissionType::Harvest),
        sim.session.binary_frame(),
    );
    let registry = crate::sim::tiberium::test_support::overlay_registry_with_land();
    for _ in 0..500 {
        let commands = sim.take_due_commands();
        sim.advance_tick(&commands, Some(&rules), Some(&grid), Some(registry), 33);
    }
    assert!(
        sim.substrate.entities.get(harvester)?.is_useless(),
        "a real no-ore Harvest miss writes Techno+3D0"
    );
    let saved = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "useless-harvester-sale", 0);
    let restored = crate::sim::snapshot::GameSnapshot::load(&saved)
        .expect("production save preserves the common Techno useless latch")
        .sim;
    assert!(
        restored.substrate.entities.get(harvester)?.is_useless(),
        "the serialized Techno+3D0 latch remains set after save/load"
    );
    // Continue the end-to-end service path on the original simulation; this
    // loaded copy is inspected only for serialization of the new field.
    let ap = sim.interner.intern("AP");
    let harvester_strength = rules.object("HARV")?.strength;
    sim.commit_direct_damage_receiver(
        &rules,
        None,
        crate::sim::combat::EntityDamageEvent::direct_receiver(
            harvester,
            harvester_strength / 4,
            0,
            0,
            None,
            ap,
            crate::sim::combat::ReceiverCallFlags {
                ignore_defenses: true,
                arg6: false,
            },
        ),
        FrameEffects::default(),
    );
    assert!(
        !sim.substrate
            .entities
            .get(harvester)?
            .health
            .is_full(harvester_strength),
        "the contacted HARV satisfies MissionRepairAndProduce's native damaged gate"
    );
    let command = crate::sim::command::Command::clicked_mission(
        harvester,
        MissionType::Repair,
        TargetKind::Entity(depot),
    );
    let record = sim.encode_megamission_record(americans, &command).unwrap();
    let directed = sim
        .decode_native_command_record(&record, sim.session.tick())
        .expect("Repair event decodes with its depot destination");
    sim.advance_tick(&[directed], Some(&rules), Some(&grid), Some(registry), 33);
    assert_eq!(
        sim.substrate
            .entities
            .get(harvester)?
            .radio_contacts
            .slot(0),
        Some(depot),
        "explicit Repair HELLO establishes the native sale contact"
    );
    assert_eq!(
        sim.substrate.entities.get(depot)?.radio_contacts.slot(0),
        Some(harvester),
        "the depot owns the reciprocal contact"
    );
    assert_eq!(
        sim.substrate.entities.get(harvester)?.navigation.nav_com(),
        Some(crate::sim::components::NavTargetRef::object(depot)),
        "the native Repair event installs its depot destination"
    );
    // Do not change ownership while the Unit is still travelling. The
    // building must receive DockNow and enter its real Repair state first;
    // otherwise ChangeOwner sends the Unit to Guard and no RepairTick can
    // reach the native sale gate.
    let mut repair_service_ready = false;
    for _ in 0..800 {
        let commands = sim.take_due_commands();
        sim.advance_tick(&commands, Some(&rules), Some(&grid), Some(registry), 33);
        if sim.substrate.entities.get(depot).is_some_and(|building| {
            building.mission.current().known() == Some(MissionType::Repair)
                && building.mission.handler_state() == 1
        }) {
            repair_service_ready = true;
            break;
        }
    }
    assert!(
        repair_service_ready,
        "a real HELLO/Enter/DockNow path reaches the depot's Repair state 1"
    );
    assert!(
        sim.substrate.entities.get(harvester)?.health.current() < harvester_strength,
        "the dock is ready before the first RepairTick heals the damaged HARV"
    );
    Some((sim, rules, grid, harvester, depot, targeter))
}

/// A no-ore HARV contact sold by a repair depot is refunded to its current AI
/// owner exactly once, then follows the shared Stun/Limbo/UnInit cleanup.
#[test]
fn a_useless_harvester_contact_is_sold_to_its_current_ai_owner() {
    let Some((mut sim, rules, grid, harvester, depot, targeter)) = useless_harvester_contact()
    else {
        return;
    };
    let americans = sim.interner.get("Americans").unwrap();
    let russians = sim.interner.get("Russians").unwrap();
    let old_owner_credits = sim.houses[&americans].economy().credits();
    sim.change_owner_with_rules(harvester, russians, &rules, None, FrameEffects::default());
    let _ = crate::sim::team_script_vm::join_team_for_test(&mut sim, &[harvester], false, false);
    assert!(
        sim.assign_target_represented(
            targeter,
            Some(TargetKind::Entity(harvester)),
            &rules,
            None,
            FrameEffects::default(),
        )
        .is_ok()
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(harvester)
            .unwrap()
            .radio_contacts
            .slot(0),
        Some(depot),
        "ChangeOwner retains the explicit Repair contact"
    );
    let expected_refund = crate::sim::production::techno_refund(&sim, &rules, harvester).unwrap();
    let repair_cost =
        crate::sim::production::repair_step_cost(&rules, rules.object("HARV").unwrap());
    let credits_before = sim.houses[&russians].economy().credits();

    for _ in 0..8 {
        let commands = sim.take_due_commands();
        sim.advance_tick(&commands, Some(&rules), Some(&grid), None, 33);
        if sim
            .substrate
            .entities
            .get(harvester)
            .is_none_or(|entity| !entity.is_object_alive())
        {
            break;
        }
    }
    let stop_state = sim.substrate.entities.get(depot).map(|building| {
        format!(
            "depot current={:?} queued={:?} effective={:?} state={} contact={:?}",
            building.mission.current().known(),
            building.mission.queued().known(),
            building.mission.effective().known(),
            building.mission.handler_state(),
            building.radio_contacts.slot(0),
        )
    });
    let unit_state = sim.substrate.entities.get(harvester).map(|unit| {
        format!(
            "owner={:?} useless={} health={} current={:?} queued={:?} nav={:?} contact={:?}",
            unit.owner(),
            unit.is_useless(),
            unit.health.current(),
            unit.mission.current().known(),
            unit.mission.queued().known(),
            unit.navigation.nav_com(),
            unit.radio_contacts.slot(0),
        )
    });
    assert!(
        sim.substrate
            .entities
            .get(harvester)
            .is_none_or(|entity| !entity.is_object_alive()),
        "native sale gate should consume the first accepted RepairTick; {stop_state:?} {unit_state:?}"
    );
    assert_eq!(
        sim.houses[&russians].economy().credits(),
        credits_before - repair_cost + expected_refund,
        "RepairTick bills once, then Techno GetRefund and House Add_Credits pay the current owner once"
    );
    assert_eq!(
        sim.houses[&americans].economy().credits(),
        old_owner_credits,
        "the former owner is not paid"
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(depot)
            .unwrap()
            .radio_contacts
            .slot(0),
        None,
        "Limbo broadcasts BREAK before UnInit"
    );
    let depot_state = sim.substrate.entities.get(depot).unwrap();
    assert_eq!(
        depot_state.mission.handler_state(),
        0,
        "MissionRepairAndProduce resets the native +0xBC service state after On_Sold"
    );
    assert_eq!(
        depot_state.building_ready_latch(),
        1,
        "MissionRepairAndProduce sets the native +0x6DD ready latch after On_Sold"
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(targeter)
            .unwrap()
            .attack_target
            .target(),
        None,
        "Techno Stun Detach_All expires external targeters"
    );
    assert!(
        sim.team_script_vm()
            .teams_in_order()
            .all(|team| !team.members().any(|member| member == harvester)),
        "Foot UnInit removes the sold object from its team"
    );
    // Native 0x0044C819 returns Repair Rate; the no-contact visit is timer-gated.
    let next_visit_delay = sim
        .substrate
        .entities
        .get(depot)
        .unwrap()
        .mission
        .dispatch_timer()
        .remaining_if_pending(sim.session.binary_frame())
        .unwrap_or(0)
        .max(0) as u32;
    for _ in 0..=next_visit_delay {
        let commands = sim.take_due_commands();
        sim.advance_tick(&commands, Some(&rules), Some(&grid), None, 33);
        if sim
            .substrate
            .entities
            .get(depot)
            .is_some_and(|building| building.mission.current().known() == Some(MissionType::Guard))
        {
            break;
        }
    }
    assert_eq!(
        sim.substrate
            .entities
            .get(depot)
            .unwrap()
            .mission
            .current()
            .known(),
        Some(MissionType::Guard),
        "the ready, state-zero repair depot promotes to Guard on its next no-contact visit"
    );
}

/// The same explicit Repair contact is not sold while its current owner is
/// human (`HouseClass::IsControlledByHuman @ 0x0050B730`).
#[test]
fn a_human_owned_useless_harvester_is_not_sold() {
    let Some((mut sim, rules, grid, harvester, _depot, _targeter)) = useless_harvester_contact()
    else {
        return;
    };
    let americans = sim.interner.get("Americans").unwrap();
    let credits_before = sim.houses[&americans].economy().credits();
    let repair_cost =
        crate::sim::production::repair_step_cost(&rules, rules.object("HARV").unwrap());
    let health_before = sim
        .substrate
        .entities
        .get(harvester)
        .unwrap()
        .health
        .current();
    let mut first_repair_step = false;

    for _ in 0..8 {
        let commands = sim.take_due_commands();
        sim.advance_tick(&commands, Some(&rules), Some(&grid), None, 33);
        if sim
            .substrate
            .entities
            .get(harvester)
            .is_some_and(|entity| entity.health.current() > health_before)
        {
            first_repair_step = true;
            break;
        }
        if sim
            .substrate
            .entities
            .get(harvester)
            .is_none_or(|entity| !entity.is_object_alive())
        {
            break;
        }
    }

    assert!(
        first_repair_step,
        "the depot's first production RepairTick reached the human-owned contact"
    );
    assert!(
        sim.substrate
            .entities
            .get(harvester)
            .unwrap()
            .is_object_alive()
    );
    assert_eq!(
        sim.houses[&americans].economy().credits(),
        credits_before - repair_cost,
        "the human repair is billed once and IsControlledByHuman withholds the sale refund"
    );
}

/// `Mission_Harvest` installs the existing ArchiveTarget destination before
/// its ore scan. A subsequent miss therefore reads a live NavCom and clears a
/// stale useless latch at the native post-scan branch.
#[test]
fn an_archive_destination_clears_a_stale_useless_latch_after_a_scan_miss() {
    let Some((mut sim, rules, _grid, harvester, _depot, _targeter)) = useless_harvester_contact()
    else {
        return;
    };
    assert!(sim.substrate.entities.get(harvester).unwrap().is_useless());
    sim.substrate
        .entities
        .get_mut(harvester)
        .unwrap()
        .set_archive_target(Some(TargetKind::Cell(15, 15)));
    let now = sim.session.binary_frame();
    let readiness = crate::sim::mission::authority::LiveReadyInputProvider { rules: &rules };
    sim.mission_queue_exact(
        harvester,
        MissionId::from_known(MissionType::Harvest),
        1,
        now,
        &readiness,
    )
    .unwrap();
    assert_eq!(
        sim.substrate
            .entities
            .get(harvester)
            .unwrap()
            .mission
            .current()
            .known(),
        Some(MissionType::Harvest),
        "the real mission queue recommences Harvest"
    );
    crate::sim::world::dispatch_foot_mission(
        &mut sim,
        harvester,
        &rules,
        crate::sim::world::ObjectAiCtx {
            overlay_registry: Some(
                crate::sim::tiberium::test_support::overlay_registry_with_land(),
            ),
            ..Default::default()
        },
    );
    let entity = sim.substrate.entities.get(harvester).unwrap();
    assert_eq!(
        entity.navigation.nav_com(),
        Some(crate::sim::components::NavTargetRef::cell(15, 15)),
        "the archive setter installs a destination before the scan miss"
    );
    assert_eq!(entity.archive_target(), None);
    assert!(
        !entity.is_useless(),
        "the post-scan-miss NavCom read clears Techno+3D0"
    );
}

/// A tank of `owner` at `cell` on `mission`, at half its Strength.
fn damaged_tank(
    sim: &mut Simulation,
    rules: &RuleSet,
    kind: &str,
    owner: &str,
    cell: (u16, u16),
    mission: MissionType,
) -> u64 {
    let tank = spawn(sim, rules, kind, owner, cell.0, cell.1);
    let strength = rules.object(kind).unwrap().strength;
    let now = sim.session.binary_frame();
    let _ = sim
        .substrate
        .mission_assign_exact(tank, MissionId::from_known(mission), now);
    sim.substrate
        .entities
        .get_mut(tank)
        .unwrap()
        .health
        .set_for_test(strength / 2);
    tank
}

fn seek(sim: &mut Simulation, rules: &RuleSet, id: u64) {
    sim.unit_ai_seek_repair(id, rules, None, FrameEffects::default());
}

/// Unit743270 accepts Repair; Unit740EF0 establishes contact and queues Enter
/// without inventing a destination. A destination-bearing Repair event then
/// uses the existing Enter/depot service chain, even for a human-owned tank.
#[test]
fn map_repair_waits_without_nav_and_a_directed_event_completes_service() {
    let Some((mut sim, rules, grid)) = scene() else {
        return;
    };
    // The generic seek arena starts both houses with no money. Depot service
    // charges its existing economy owner; supply ordinary skirmish funds.
    let owner = sim.interner.get("Americans").unwrap();
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .economy_mut()
        .set_credits_for_test(10_000);
    spawn(&mut sim, &rules, "GAPOWR", "Americans", 34, 30);
    // Keep the rival house alive so the ordinary skirmish victory path does
    // not stop the binary-frame clock before the service timers finish.
    spawn(&mut sim, &rules, "NAPOWR", "Russians", 40, 40);
    let depot = spawn(&mut sim, &rules, "GADEPT", "Americans", 24, 20);
    let placements =
        crate::map::entities::parse_map_entities(&crate::rules::ini_parser::IniFile::from_str(
            "[Units]\n0=Americans,MTNK,128,14,20,0,Repair,None,0,-1,0,-1,1,1\n",
        ));
    assert_eq!(placements[0].mission, Some(MissionType::Repair));
    assert_eq!(sim.spawn_from_map(&placements, Some(&rules)), 1);
    let tank = sim
        .substrate
        .entities
        .values()
        .find(|entity| sim.interner.resolve(entity.type_ref()) == "MTNK")
        .unwrap()
        .stable_id();
    let initial_health = sim.substrate.entities.get(tank).unwrap().health.current();
    let mut entered = false;
    for _ in 0..64 {
        let commands = sim.take_due_commands();
        assert!(
            sim.advance_tick(&commands, Some(&rules), Some(&grid), None, 33)
                .frame_committed
        );
        let unit = sim.substrate.entities.get(tank).unwrap();
        entered |= unit.mission.current().known() == Some(MissionType::Enter);
    }
    let unit = sim.substrate.entities.get(tank).unwrap();
    assert!(entered);
    assert_eq!(unit.radio_contacts.slot(0), Some(depot));
    assert_eq!(unit.navigation.nav_com(), None);
    assert_eq!((unit.position.rx(), unit.position.ry()), (14, 20));
    assert_eq!(unit.health.current(), initial_health);
    let spent_before = sim.houses[&owner].economy().spent_credits();
    // The native MEGAMISSION envelope can supply both Repair and a depot
    // destination. Unlike the map's mission column, it also runs vt+480.
    let directed = crate::sim::command::Command::clicked_mission(
        tank,
        MissionType::Repair,
        TargetKind::Entity(depot),
    );
    let record = sim.encode_megamission_record(owner, &directed).unwrap();
    let directed = sim
        .decode_native_command_record(&record, sim.session.tick())
        .expect("native Repair event decodes with its depot destination");
    assert!(
        sim.advance_tick(&[directed], Some(&rules), Some(&grid), None, 33)
            .frame_committed
    );
    entered = sim
        .substrate
        .entities
        .get(tank)
        .unwrap()
        .mission
        .current()
        .known()
        == Some(MissionType::Enter);
    for _ in 0..3000 {
        let commands = sim.take_due_commands();
        assert!(
            sim.advance_tick(&commands, Some(&rules), Some(&grid), None, 33)
                .frame_committed
        );
        let unit = sim.substrate.entities.get(tank).unwrap();
        entered |= unit.mission.current().known() == Some(MissionType::Enter);
        if unit.health.current() == rules.object("MTNK").unwrap().strength
            && unit.radio_contacts.slot(0).is_none()
        {
            assert!(entered, "Repair must hand off to Enter before service");
            assert!(unit.health.current() > initial_health);
            assert!(sim.houses[&owner].economy().spent_credits() > spent_before);
            assert!(
                !sim.substrate
                    .entities
                    .get(depot)
                    .unwrap()
                    .radio_contacts
                    .iter_live()
                    .any(|id| id == tank)
            );
            return;
        }
    }
    let unit = sim.substrate.entities.get(tank).unwrap();
    panic!(
        "Repair did not finish: health={}, mission={:?}, queued={:?}, contact={:?}, cell=({}, {}), nav={:?}, depot_mission={:?}, depot_status={}",
        unit.health.current(),
        unit.mission.current(),
        unit.mission.queued(),
        unit.radio_contacts.slot(0),
        unit.position.rx(),
        unit.position.ry(),
        unit.navigation.nav_com(),
        sim.substrate.entities.get(depot).unwrap().mission.current(),
        sim.substrate
            .entities
            .get(depot)
            .unwrap()
            .mission
            .handler_state()
    );
}

#[test]
fn repair_mission_hello_queues_enter_without_commencing_or_setting_destination() {
    let Some((mut sim, rules, _)) = scene() else {
        return;
    };
    let depot = spawn(&mut sim, &rules, "GADEPT", "Americans", 24, 20);
    let tank = damaged_tank(
        &mut sim,
        &rules,
        "MTNK",
        "Americans",
        (14, 20),
        MissionType::Repair,
    );
    let before_rng = sim.scenario_rng.logical_state();
    assert_eq!(
        sim.unit_mission_repair(tank, &rules, FrameEffects::default()),
        1
    );
    let unit = sim.substrate.entities.get(tank).unwrap();
    assert_eq!(unit.mission.current().known(), Some(MissionType::Repair));
    assert_eq!(unit.mission.queued().known(), Some(MissionType::Enter));
    assert_eq!(unit.radio_contacts.slot(0), Some(depot));
    assert_eq!(unit.navigation.nav_com(), None);
    assert_eq!(sim.scenario_rng.logical_state(), before_rng);
}

#[test]
fn repair_mission_without_a_bay_waits_at_its_unjittered_rate() {
    let Some((mut sim, rules, _)) = scene() else {
        return;
    };
    let tank = damaged_tank(
        &mut sim,
        &rules,
        "MTNK",
        "Americans",
        (14, 20),
        MissionType::Repair,
    );
    let before_rng = sim.scenario_rng.logical_state();
    assert_eq!(
        sim.unit_mission_repair(tank, &rules, FrameEffects::default()),
        rules.mission_control.rate_frames(MissionType::Repair)
    );
    let unit = sim.substrate.entities.get(tank).unwrap();
    assert_eq!(unit.mission.current().known(), Some(MissionType::Repair));
    assert_eq!(unit.mission.queued(), MissionId::NONE);
    assert_eq!(unit.radio_contacts.slot(0), None);
    assert_eq!(sim.scenario_rng.logical_state(), before_rng);
}

#[test]
fn repair_mission_keeps_waiting_for_an_occupied_retail_depot() {
    let Some((mut sim, rules, _)) = scene() else {
        return;
    };
    let depot = spawn(&mut sim, &rules, "GADEPT", "Americans", 24, 20);
    let waiting = damaged_tank(
        &mut sim,
        &rules,
        "MTNK",
        "Americans",
        (14, 20),
        MissionType::Repair,
    );
    let occupant = damaged_tank(
        &mut sim,
        &rules,
        "MTNK",
        "Americans",
        (16, 20),
        MissionType::Enter,
    );
    sim.substrate
        .entities
        .get_mut(depot)
        .unwrap()
        .radio_contacts
        .set_slot(0, occupant);
    sim.substrate
        .entities
        .get_mut(occupant)
        .unwrap()
        .radio_contacts
        .set_slot(0, depot);
    let before_rng = sim.scenario_rng.logical_state();
    let delay = sim.unit_mission_repair(waiting, &rules, FrameEffects::default());
    assert_eq!(
        sim.substrate
            .entities
            .get(waiting)
            .unwrap()
            .mission
            .current()
            .known(),
        Some(MissionType::Repair)
    );
    assert_eq!(
        delay,
        rules.mission_control.rate_frames(MissionType::Repair)
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(waiting)
            .unwrap()
            .mission
            .queued()
            .known(),
        None
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(depot)
            .unwrap()
            .radio_contacts
            .slot(0),
        Some(occupant)
    );
    assert_eq!(sim.scenario_rng.logical_state(), before_rng);
}

fn queued(sim: &Simulation, id: u64) -> MissionId {
    sim.substrate.entities.get(id).unwrap().mission.queued()
}

fn heads_for(sim: &Simulation, id: u64, station: u64) -> bool {
    let unit = sim.substrate.entities.get(id).unwrap();
    unit.navigation.nav_com() == Some(NavTargetRef::object(station))
        || unit.pending_entry() == Some(station)
        || unit.radio_contacts.slot(0) == Some(station)
}

#[test]
fn the_station_reach_is_twenty_cell_diagonals() {
    // ftol(Sqrt_Approx(131072) * 20): 20 * 362.04 leptons.
    assert_eq!(unit_repair_reach(), 7240);
}

/// A damaged computer tank on Guard archives its Cell, queues Enter, drops
/// its Target and takes its House's Service Depot as its destination.
#[test]
fn a_damaged_computer_tank_on_guard_heads_for_its_depot() {
    let Some((mut sim, rules, _)) = scene() else {
        return;
    };
    let depot = spawn(&mut sim, &rules, "GADEPT", "Russians", 24, 20);
    let tank = damaged_tank(
        &mut sim,
        &rules,
        "HTNK",
        "Russians",
        (14, 20),
        MissionType::Guard,
    );
    let _ = sim.assign_target_represented(
        tank,
        Some(TargetKind::Cell(30, 30)),
        &rules,
        None,
        FrameEffects::default(),
    );

    seek(&mut sim, &rules, tank);

    let unit = sim.substrate.entities.get(tank).unwrap();
    assert_eq!(unit.archive_target(), Some(TargetKind::Cell(14, 20)));
    assert_eq!(unit.attack_target.target(), None);
    assert_eq!(
        sim.substrate.entities.get(tank).unwrap().mission.queued(),
        MissionId::from_known(MissionType::Enter)
    );
    assert!(heads_for(&sim, tank, depot));
}

/// The gates of `0x0073671F..0x0073679A`: the 16-frame cadence, a computer
/// House, Guard or AreaGuard with nothing queued, damage, no ArchiveTarget
/// unless on AreaGuard, and no `Harvester=`.
#[test]
fn only_a_damaged_idle_computer_vehicle_seeks_repair() {
    let Some((mut sim, rules, _)) = scene() else {
        return;
    };
    let _depot = spawn(&mut sim, &rules, "GADEPT", "Russians", 24, 20);
    let _player_depot = spawn(&mut sim, &rules, "GADEPT", "Americans", 24, 30);
    let enter = MissionId::from_known(MissionType::Enter);

    let off_cadence = damaged_tank(
        &mut sim,
        &rules,
        "HTNK",
        "Russians",
        (14, 14),
        MissionType::Guard,
    );
    sim.session.set_binary_frame_for_test(161);
    seek(&mut sim, &rules, off_cadence);
    assert_eq!(queued(&sim, off_cadence), MissionId::NONE);
    sim.session.set_binary_frame_for_test(176);
    seek(&mut sim, &rules, off_cadence);
    assert_eq!(queued(&sim, off_cadence), enter);

    let human = damaged_tank(
        &mut sim,
        &rules,
        "MTNK",
        "Americans",
        (14, 30),
        MissionType::Guard,
    );
    seek(&mut sim, &rules, human);
    assert_eq!(queued(&sim, human), MissionId::NONE);

    let whole = damaged_tank(
        &mut sim,
        &rules,
        "HTNK",
        "Russians",
        (14, 16),
        MissionType::Guard,
    );
    let strength = rules.object("HTNK").unwrap().strength;
    sim.substrate
        .entities
        .get_mut(whole)
        .unwrap()
        .health
        .set_for_test(strength);
    seek(&mut sim, &rules, whole);
    assert_eq!(queued(&sim, whole), MissionId::NONE);

    let hunting = damaged_tank(
        &mut sim,
        &rules,
        "HTNK",
        "Russians",
        (14, 18),
        MissionType::Hunt,
    );
    seek(&mut sim, &rules, hunting);
    assert_eq!(queued(&sim, hunting), MissionId::NONE);

    let posted = damaged_tank(
        &mut sim,
        &rules,
        "HTNK",
        "Russians",
        (14, 22),
        MissionType::Guard,
    );
    let post = Some(TargetKind::Cell(3, 3));
    sim.substrate
        .entities
        .get_mut(posted)
        .unwrap()
        .set_archive_target(post);
    seek(&mut sim, &rules, posted);
    assert_eq!(queued(&sim, posted), MissionId::NONE);

    // Something already queued bars it.
    let busy = damaged_tank(
        &mut sim,
        &rules,
        "HTNK",
        "Russians",
        (14, 28),
        MissionType::Guard,
    );
    let move_ = MissionId::from_known(MissionType::Move);
    let now = sim.session.binary_frame();
    let _ = sim.mission_queue_exact(
        busy,
        move_,
        0,
        now,
        &LiveReadyInputProvider { rules: &rules },
    );
    seek(&mut sim, &rules, busy);
    assert_eq!(queued(&sim, busy), move_);

    let miner = damaged_tank(
        &mut sim,
        &rules,
        "HARV",
        "Russians",
        (14, 26),
        MissionType::Guard,
    );
    seek(&mut sim, &rules, miner);
    assert_eq!(queued(&sim, miner), MissionId::NONE);
}

/// On AreaGuard an ArchiveTarget does not bar the seek, and the seek keeps
/// it: only a missing one becomes the vehicle's own Cell (`0x007367B7`).
#[test]
fn an_area_guard_keeps_its_post_when_it_seeks_repair() {
    let Some((mut sim, rules, _)) = scene() else {
        return;
    };
    let depot = spawn(&mut sim, &rules, "GADEPT", "Russians", 24, 20);
    let tank = damaged_tank(
        &mut sim,
        &rules,
        "HTNK",
        "Russians",
        (14, 20),
        MissionType::AreaGuard,
    );
    let post = Some(TargetKind::Cell(3, 3));
    sim.substrate
        .entities
        .get_mut(tank)
        .unwrap()
        .set_archive_target(post);

    seek(&mut sim, &rules, tank);

    assert_eq!(
        queued(&sim, tank),
        MissionId::from_known(MissionType::Enter)
    );
    assert_eq!(
        sim.substrate.entities.get(tank).unwrap().archive_target(),
        post
    );
    assert!(heads_for(&sim, tank, depot));
}

/// `0x00455DD0` admits any nonzero CAN_LOAD answer: the Naval Yard answers a
/// land tank NEGATORY (`0x0043C3F3`), and being nearer it is the pick. A
/// station beyond the reach is not.
#[test]
fn a_refusing_naval_yard_nearer_than_the_depot_is_the_pick() {
    let Some((mut sim, rules, _)) = scene() else {
        return;
    };
    flood(
        &mut sim,
        &rules,
        ARENA,
        (30..40).flat_map(|y| (4..14).map(move |x| (x, y))),
    );
    let yard = spawn(&mut sim, &rules, "GAYARD", "Russians", 6, 32);
    let depot = spawn(&mut sim, &rules, "GADEPT", "Russians", 30, 20);
    let tank = damaged_tank(
        &mut sim,
        &rules,
        "HTNK",
        "Russians",
        (16, 28),
        MissionType::Guard,
    );
    assert_eq!(
        transmit(
            &mut sim,
            tank,
            yard,
            RadioMessage::CanEnter,
            RadioPayload::default(),
            &rules,
            FrameEffects::default(),
        ),
        RadioResponse::Negatory
    );

    assert_eq!(
        sim.nearest_unit_repair_station(tank, &rules, FrameEffects::default()),
        Some(yard)
    );

    // Beyond 7240 leptons of both, nothing is chosen.
    let far = damaged_tank(
        &mut sim,
        &rules,
        "HTNK",
        "Russians",
        (46, 46),
        MissionType::Guard,
    );
    let _ = depot;
    assert_eq!(
        sim.nearest_unit_repair_station(far, &rules, FrameEffects::default()),
        None
    );
}

/// Through the production frame: the seek runs on a frame that is a
/// multiple of 16, after the Unit's second Ready/Commence, and the next
/// frame commences Enter toward the depot.
#[test]
fn the_production_frame_sends_a_damaged_computer_tank_to_its_depot() {
    let Some((mut sim, rules, grid)) = scene() else {
        return;
    };
    // A computer House with no base sells off and sends everyone hunting
    // (Strategy `0x004FD879..0x004FD904`); a passive one runs no Strategy.
    let russians = sim.interner.get("Russians").unwrap();
    sim.houses
        .get_mut(&russians)
        .unwrap()
        .set_multiplay_passive_for_test(true);
    let depot = spawn(&mut sim, &rules, "GADEPT", "Russians", 24, 20);
    let tank = damaged_tank(
        &mut sim,
        &rules,
        "HTNK",
        "Russians",
        (14, 20),
        MissionType::Guard,
    );
    let enter = MissionId::from_known(MissionType::Enter);
    let guard = MissionId::from_known(MissionType::Guard);
    // The seek follows the frame's second Ready/Commence (`0x007366FD`), so
    // its Enter waits in the queue for the next frame's first Commence
    // (`0x00736473`).
    let mut seek_frame = None;
    for _ in 0..40 {
        let commands = sim.take_due_commands();
        sim.advance_tick(&commands, Some(&rules), Some(&grid), None, 33);
        let unit = sim.substrate.entities.get(tank).unwrap();
        if unit.mission.queued() == enter {
            assert_eq!(unit.mission.current(), guard);
            seek_frame = Some(sim.session.binary_frame());
            break;
        }
        assert_eq!(unit.mission.current(), guard);
    }
    let seek_frame = seek_frame.expect("the seek runs within 16 frames");
    assert_eq!((seek_frame - 1) & 0xF, 0, "frame {seek_frame}");
    assert_eq!(
        sim.substrate.entities.get(tank).unwrap().archive_target(),
        Some(TargetKind::Cell(14, 20))
    );
    let commands = sim.take_due_commands();
    sim.advance_tick(&commands, Some(&rules), Some(&grid), None, 33);
    assert_eq!(
        sim.substrate.entities.get(tank).unwrap().mission.current(),
        enter
    );
    assert!(heads_for(&sim, tank, depot));
}
