//! Script actions 18, 28, 29, 35, 36, 44 and 45 through the production script
//! step (`TeamClass::AI @ 0x006E9140`, table `0x006E9F74`).

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::rule_policy::RulePolicy;
use crate::rules::team_ai_ini::TeamAiIniRegistry;
use crate::sim::ore_growth::{OreGrowthConfig, OreGrowthState};
use crate::sim::overlay_grid::OverlayGrid;
use crate::sim::team_script_vm::TeamScriptRefusal;

const DRIVE: &str = "{4A582741-9839-11d1-B709-00A024DDAFD1}";

/// The 32x32 arena with ore rules, TRUCKA/TRUCKB/TANK and the team AI of
/// `scripts`: TeamType 0 OLDTEAM and 1 NEWTEAM, whose TaskForce takes trucks.
fn scene(scripts: &str) -> (Simulation, RuleSet, InternedId) {
    scene_with(scripts, "")
}

/// [`scene`] with `rules` merged over its rules.
fn scene_with(scripts: &str, rules: &str) -> (Simulation, RuleSet, InternedId) {
    let unit = format!("Speed=4\nSpeedType=Track\nMovementZone=Normal\nLocomotor={DRIVE}\n");
    let mut ini = crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[Countries]\n0=Americans\n1=Russians\n\
         [VehicleTypes]\n0=TRUCKA\n1=TRUCKB\n2=TANK\n\
         [TRUCKA]\nStrength=200\n{unit}[TRUCKB]\nStrength=300\n{unit}\
         [TANK]\nStrength=400\n{unit}"
    ));
    ini.merge(&IniFile::from_str(
        &crate::sim::tiberium::test_support::tiberium_rules_text(),
    ));
    ini.merge(&IniFile::from_str(rules));
    let rules = RuleSet::from_ini(&ini).unwrap();
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x6E_9140);
    sim.overlay_grid = Some(OverlayGrid::new(32, 32));
    sim.ore_growth_config = OreGrowthConfig::for_test(false, false, false);
    sim.ore_growth_state = OreGrowthState::new(32, 32);
    sim.fog.set_width_for_test(32);
    sim.fog.set_height_for_test(32);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let owner = sim.interner.intern("Americans");
    sim.session.set_current_house_for_test(Some(owner));
    let map = IniFile::from_str(&format!(
        "{scripts}\n\
         [TaskForces]\n0=TRUCKS\n[TRUCKS]\nGroup=-1\n0=2,TRUCKA\n1=2,TRUCKB\n\
         [TeamTypes]\n0=OLDTEAM\n1=NEWTEAM\n\
         [OLDTEAM]\nHouse=Americans\nScript=RUN\nTaskForce=TRUCKS\nMax=1\n\
         [NEWTEAM]\nHouse=Americans\nScript=IDLE\nTaskForce=TRUCKS\nMax=0\n"
    ));
    let registry =
        TeamAiIniRegistry::from_sources(&IniFile::from_str(""), &map, RulePolicy::stock_campaign());
    sim.install_team_ai_registry(&registry, &rules);
    (sim, rules, owner)
}

fn team(sim: &mut Simulation, owner: InternedId, members: &[u64]) -> u64 {
    let script = sim.interner.get("RUN").unwrap();
    sim.team_script_vm
        .create_team(owner, script, members.to_vec(), 0)
}

fn run(sim: &mut Simulation, rules: &RuleSet, team: u64) {
    let registry = crate::sim::tiberium::test_support::overlay_registry();
    sim.team_step_script(team, rules, Some(registry), &mut FrameEffects::empty());
}

const SCRIPTS: &str = "[ScriptTypes]\n0=RUN\n1=IDLE\n[IDLE]\n0=5,9\n";

#[test]
fn ares_team_script_67_drives_using_explicit_rules() {
    use crate::sim::components::NavTargetRef;
    use crate::sim::runtime::{SimResources, SimRuntime};

    let _stock_rules = crate::rules::ares_switch::read_ares_way(false);
    let (mut sim, rules, owner) = scene_with(
        &format!("{SCRIPTS}[RUN]\n0=67,0\n1=29,0\n"),
        "[InfantryTypes]\n0=DRIVER\n[DRIVER]\nStrength=75\nCanDrive=yes\nSpeed=4\n\
         SpeedType=Foot\nLocomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
         [Russians]\nCanBeDriven=yes\n",
    );
    assert!(!rules.ares_form());
    assert!(!rules.ares());
    let driver = sim
        .spawn_object("DRIVER", "Americans", 8, 8, 0, &rules)
        .unwrap();
    let vehicle = sim
        .spawn_object("TRUCKA", "Russians", 9, 8, 0, &rules)
        .unwrap();
    let team = team(&mut sim, owner, &[driver]);

    run(&mut sim, &rules, team);

    assert_eq!(
        sim.entities().get(driver).unwrap().navigation.nav_com(),
        Some(NavTargetRef::object(vehicle)),
    );
    let state = sim.team_script_vm.team(team).unwrap();
    assert!(state.members().next().is_none());
    assert!(state.advance_pending());
    run(&mut sim, &rules, team);
    assert!(sim.houses[&owner].ai_activation().production);

    sim.session.publish_started();
    let mut runtime = SimRuntime {
        simulation: sim,
        resources: std::sync::Arc::new(SimResources::new(rules)),
    };
    for _ in 0..150 {
        runtime.advance_frame_for_tooling(&[], 22).unwrap();
        if runtime.simulation.entities().get(vehicle).unwrap().owner() == owner {
            break;
        }
    }
    assert_eq!(
        runtime.simulation.entities().get(vehicle).unwrap().owner(),
        owner
    );
    assert!(runtime.simulation.entities().get(driver).is_none());
}

#[test]
fn ares_script_vehicle_capture_keeps_its_event_and_one_time_captor_rule() {
    use crate::map::map_file::MapFile;
    use crate::sim::combat::{EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags};
    use crate::sim::world::UninitContext;

    let _stock_rules = crate::rules::ares_switch::read_ares_way(false);
    let (mut sim, rules, owner) = scene_with(
        SCRIPTS,
        "[InfantryTypes]\n0=THIEF\n[THIEF]\nStrength=80\nVehicleThief=yes\n\
         VehicleThief.OneTime=yes\nSpeed=4\nSpeedType=Foot\n\
         Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
         [Warheads]\n0=CAPTUREWH\n[CAPTUREWH]\nVerses=100%\n",
    );
    assert!(!rules.ares());
    assert!(rules.object("THIEF").unwrap().driver_rules().one_time());
    let map = MapFile::from_bytes(
        b"[Map]\nTheater=TEMPERATE\nSize=0,0,2,1\nLocalSize=0,0,2,1\n\
          [IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n[Basic]\nNewINIFormat=4\n\
          [Triggers]\nTAKEN=Americans,<none>,Vehicle taken,0,1,1,1,0\n\
          [Events]\nTAKEN=1,69,0,0\n[Actions]\nTAKEN=1,28,0,17,0,0,0,0,A\n\
          [Tags]\nTAKEN_TAG=0,Vehicle taken,TAKEN\n",
    )
    .unwrap();
    let teams =
        TeamAiIniRegistry::from_sources(&IniFile::empty(), &map.ini, RulePolicy::stock_campaign());
    sim.initialize_trigger_definitions(&map, &teams, None, None, None);
    sim.classify_map_triggers(&rules);
    let thief = sim
        .spawn_object("THIEF", "Americans", 8, 8, 0, &rules)
        .unwrap();
    let vehicle = sim
        .spawn_object("TRUCKA", "Russians", 9, 8, 0, &rules)
        .unwrap();
    sim.attach_object_trigger_tag(vehicle, Some("TAKEN_TAG"));
    sim.session.publish_started();

    sim.dispatch_object_tag_event(vehicle, 48, Some(thief), None, UninitContext::new(&rules));
    let before_capture = sim.trigger_runtime.global_variable_is_set(17);
    let registry = Some(rules.overlay_registry());
    let mut effects = FrameEffects::empty();
    let outcome = sim
        .ares_capture_vehicle(thief, vehicle, &rules, registry, &mut effects)
        .unwrap();
    assert!(outcome.consumed);
    assert_eq!(sim.entities().get(vehicle).unwrap().owner(), owner);
    sim.uninit_with_context(thief, UninitContext::new(&rules));
    let after_capture = sim.trigger_runtime.global_variable_is_set(17);

    let warhead = sim.interner.intern("CAPTUREWH");
    let hit = EntityDamageEvent::direct_receiver(
        vehicle,
        100_000,
        0,
        RAD_NO_ATTACKER,
        None,
        warhead,
        ReceiverCallFlags {
            ignore_defenses: false,
            arg6: false,
        },
    );
    sim.commit_noncombat_aoe_hits(&rules, registry, &[hit], &mut effects);
    assert!(
        sim.entities()
            .get(vehicle)
            .is_none_or(|unit| !unit.lifecycle.object_alive())
    );
    let thief_type = sim.interner.get("THIEF").unwrap();
    let captor_returned = sim
        .entities()
        .values()
        .any(|entity| entity.stable_id() != thief && entity.type_ref() == thief_type);
    assert_eq!(
        (before_capture, after_capture, captor_returned),
        (false, true, false)
    );
}

#[test]
fn ares_team_script_65_applies_auxiliary_drain_once() {
    use crate::sim::runtime::{SimResources, SimRuntime};

    let _stock_rules = crate::rules::ares_switch::read_ares_way(false);
    let (mut sim, rules, owner) = scene(&format!("{SCRIPTS}[RUN]\n0=65,-50\n1=29,0\n"));
    assert!(!rules.ares());
    let member = sim
        .spawn_object("TRUCKA", "Americans", 8, 8, 0, &rules)
        .unwrap();
    sim.spawn_object("TRUCKB", "Russians", 14, 8, 0, &rules)
        .unwrap();
    let team = team(&mut sim, owner, &[member]);

    run(&mut sim, &rules, team);
    run(&mut sim, &rules, team);
    assert!(sim.houses[&owner].ai_activation().production);

    sim.session.publish_started();
    let mut runtime = SimRuntime {
        simulation: sim,
        resources: std::sync::Arc::new(SimResources::new(rules)),
    };
    runtime.advance_frame_for_tooling(&[], 22).unwrap();
    let sim = &runtime.simulation;
    assert_eq!(sim.power_states.get(&owner).unwrap().total_drain(), 50);
    let other = sim.interner.get("Russians").unwrap();
    let other_power = sim.power_states.get(&other);
    assert_eq!(other_power.map_or(0, |state| state.total_drain()), 0);
}

/// Case 18 builds TeamType::Array[1] for the team's House, moves every member
/// over and deletes the old team, ignoring the new TeamType's `Max=0`.
#[test]
fn script18_moves_members_into_a_new_team_and_deletes_the_old_one() {
    let (mut sim, rules, owner) = scene(&format!("{SCRIPTS}[RUN]\n0=18,1\n"));
    let trucks: Vec<u64> = [4, 6]
        .into_iter()
        .map(|x| {
            sim.spawn_object("TRUCKA", "Americans", x, 4, 0, &rules)
                .unwrap()
        })
        .collect();
    let old = team(&mut sim, owner, &trucks);

    run(&mut sim, &rules, old);

    assert!(sim.team_script_vm.team(old).is_none());
    let new_type = sim.interner.get("NEWTEAM").unwrap();
    let new: Vec<_> = sim
        .team_script_vm
        .teams_in_order()
        .filter(|team| team.team_type_id() == Some(new_type))
        .collect();
    assert_eq!(new.len(), 1);
    assert_eq!(new[0].owner(), owner);
    let mut members: Vec<u64> = new[0].members().collect();
    members.sort_unstable();
    assert_eq!(members, trucks);
}

/// Native indexes past the TeamType array; VERA keeps the team on the step.
#[test]
fn script18_keeps_a_team_whose_argument_names_no_team_type() {
    let (mut sim, rules, owner) = scene(&format!("{SCRIPTS}[RUN]\n0=18,7\n"));
    let truck = sim
        .spawn_object("TRUCKA", "Americans", 4, 4, 0, &rules)
        .unwrap();
    let old = team(&mut sim, owner, &[truck]);

    run(&mut sim, &rules, old);

    let state = sim.team_script_vm.team(old).unwrap();
    assert_eq!(state.members().collect::<Vec<_>>(), [truck]);
    assert!(!state.advance_pending());
    assert_eq!(
        state.refusal(),
        Some(TeamScriptRefusal::InvalidArgument {
            action_id: 18,
            argument: 7
        })
    );
}

/// Case 44 stores TRUCKA's Type on a TRUCKB Unit, and nothing else: Health
/// stays and other types are left alone. Case 29 then sets the House's
/// Production latch.
#[test]
fn script44_swaps_the_truck_type_only_and_script29_starts_production() {
    let (mut sim, mut rules, owner) = scene(&format!("{SCRIPTS}[RUN]\n0=44,0\n1=29,0\n"));
    let art = IniFile::from_str("[TRUCKB]\nVoxel=yes\n[TRUCKA]\nVoxel=no\n");
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
    let loaded = sim
        .spawn_object("TRUCKB", "Americans", 4, 4, 0, &rules)
        .unwrap();
    let tank = sim
        .spawn_object("TANK", "Americans", 6, 4, 0, &rules)
        .unwrap();
    sim.substrate
        .entities
        .get_mut(loaded)
        .unwrap()
        .health
        .set_for_test(77);
    let team = team(&mut sim, owner, &[loaded, tank]);
    assert!(sim.entities().get(loaded).unwrap().is_voxel());
    sim.take_looks();

    run(&mut sim, &rules, team);

    assert_eq!(
        sim.take_looks().entities,
        crate::sim::Touched::Ids(vec![loaded])
    );
    let truck = sim.entities().get(loaded).unwrap();
    assert!(!truck.is_voxel());
    assert_eq!(sim.interner.resolve(truck.type_ref()), "TRUCKA");
    assert_eq!(truck.health.current(), 77);
    let tank = sim.entities().get(tank).unwrap();
    assert_eq!(sim.interner.resolve(tank.type_ref()), "TANK");
    assert!(sim.team_script_vm.team(team).unwrap().advance_pending());

    assert!(!sim.houses[&owner].ai_activation().production);
    run(&mut sim, &rules, team);
    assert!(sim.houses[&owner].ai_activation().production);
}

/// Case 45 stores TRUCKB's Type on TRUCKA Units. Their kept Drives then
/// path by TRUCKB's MovementZone=, which native locomotors read from the
/// Type when they use it: across a river TRUCKA cannot cross. The second
/// truck's Drive waits under a Teleport through the store.
#[test]
fn script45_trucks_path_across_water_as_their_new_type() {
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::sim::components::DriveCoord;
    use crate::sim::movement::FindPathResult;
    use crate::sim::movement::movement_tick::FootPathRequest;
    let (mut sim, rules, owner) = scene_with(
        &format!("{SCRIPTS}[RUN]\n0=45,0\n"),
        "[Water]\nTrack=0%\nAmphibious=100%\n\
         [TRUCKB]\nSpeedType=Amphibious\nMovementZone=Amphibious\n",
    );
    crate::sim::arena_fixture::flood(&mut sim, &rules, 32, (0..32).map(|y| (8, y)));
    let trucks = [4, 6].map(|y| {
        sim.spawn_object("TRUCKA", "Americans", 4, y, 0, &rules)
            .unwrap()
    });
    let frame = sim.session.binary_frame();
    let warped = sim.substrate.entities.get_mut(trucks[1]).unwrap();
    let drive = warped.locomotor.as_mut().unwrap();
    assert!(drive.begin_piggyback(LocomotorKind::Teleport, frame));
    let team = team(&mut sim, owner, &trucks);

    run(&mut sim, &rules, team);
    let warped = sim.substrate.entities.get_mut(trucks[1]).unwrap();
    assert!(warped.locomotor.as_mut().unwrap().end_piggyback().is_some());

    let found = trucks.map(|truck| {
        let goal = DriveCoord::cell(12, 5, 0);
        let bounds = sim.playfield_bounds();
        let request = FootPathRequest::track(&sim.substrate.entities, truck, goal, 0, bounds);
        sim.foot_find_path(&request.unwrap(), &rules, None, &mut FrameEffects::empty())
    });
    assert_eq!(
        found,
        [Ok(FindPathResult::Route), Ok(FindPathResult::Route)]
    );
}

/// Case 28 reduces the ore under each member 109 times (Map 0x0057B790).
#[test]
fn script28_reduces_ore_under_each_member() {
    let (mut sim, rules, owner) = scene(&format!("{SCRIPTS}[RUN]\n0=28,0\n"));
    let registry = crate::sim::tiberium::test_support::overlay_registry();
    let ore = registry.id_for_name("TIB01").unwrap();
    let truck = sim
        .spawn_object("TRUCKA", "Americans", 8, 8, 0, &rules)
        .unwrap();
    for cell in [(8, 8), (8, 9)] {
        let grid = sim.overlay_grid.as_mut().unwrap();
        grid.place_overlay(cell.0, cell.1, ore, 255);
        grid.recalculate_runtime_cell(sim.resolved_terrain.as_mut().unwrap(), registry, cell);
    }
    let team = team(&mut sim, owner, &[truck]);

    run(&mut sim, &rules, team);

    let grid = sim.overlay_grid.as_ref().unwrap();
    assert_eq!(grid.cell(8, 8).overlay_data, 146);
    assert_eq!(grid.cell(8, 9).overlay_data, 255);
    assert!(sim.team_script_vm.team(team).unwrap().advance_pending());
}

/// Case 36 reveals the whole map to the player (0x00577F30, NULL); case 35
/// shrouds it again (0x00577BB0, NULL) and clears House+240, so the next
/// RevealEntireMap is admitted. The re-seeing pass (0x004ADFF0) is the one
/// Execute31's GrowShroud shares, tested with it.
#[test]
fn script36_reveals_and_script35_shrouds_the_whole_map_again() {
    let (mut sim, rules, owner) = scene(&format!("{SCRIPTS}[RUN]\n0=36,0\n1=35,0\n"));
    let truck = sim
        .spawn_object("TRUCKA", "Americans", 8, 8, 0, &rules)
        .unwrap();
    let team = team(&mut sim, owner, &[truck]);

    run(&mut sim, &rules, team);
    assert!(sim.fog.whole_map_revealed_for_owner(owner));
    let far: Vec<(u16, u16)> = (0..32u16)
        .flat_map(|x| (0..32u16).map(move |y| (x, y)))
        .filter(|&(x, y)| x.abs_diff(8) + y.abs_diff(8) > 6)
        .filter(|&(x, y)| sim.fog.is_cell_revealed(owner, x, y))
        .collect();
    assert!(!far.is_empty());

    run(&mut sim, &rules, team);
    assert!(!sim.fog.whole_map_revealed_for_owner(owner));
    for (x, y) in far {
        assert!(!sim.fog.is_cell_revealed(owner, x, y), "{x},{y}");
    }
    assert!(sim.team_script_vm.team(team).unwrap().advance_pending());
}
