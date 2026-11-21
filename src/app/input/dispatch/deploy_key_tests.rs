//! Tests for the Deploy key's per-object command choice.

use super::{deploy_key_command, deploy_key_commands};
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::command::Command;
use crate::sim::passenger::{PassengerCargo, PassengerRole};
use crate::sim::world::Simulation;

/// A battle whose player is `Americans`, so the issue walk's control
/// and activity tests can admit the fixture's objects.
fn player_sim() -> Simulation {
    let mut sim = Simulation::new();
    sim.session_mut_for_test()
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    let house = Some(sim.intern("Americans"));
    sim.session_mut_for_test().set_current_house_for_test(house);
    sim
}

#[test]
fn simple_deployer_key_preserves_infantry_and_transport_routes() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=HELI\n1=TRANSPORT\n2=PLAIN\n3=FIRER\n\
         [AircraftTypes]\n0=AIRCRAFT\n[InfantryTypes]\n0=GI\n\
         [HELI]\nIsSimpleDeployer=yes\n\
         [TRANSPORT]\nIsSimpleDeployer=yes\nPassengers=5\n\
         [PLAIN]\nDeployer=yes\n[FIRER]\nDeployFire=yes\n\
         [AIRCRAFT]\nIsSimpleDeployer=yes\n\
         [GI]\nIsSimpleDeployer=yes\nDeployFire=yes\nDeployer=yes\n",
    ))
    .expect("deploy key rules");
    let mut sim = player_sim();
    let mut spawn = |type_id| {
        sim.spawn_object_limbo_at_height(type_id, "Americans", 10, 10, 0, 0, &rules)
            .expect("the deploy key fixture builds")
    };
    let heli = spawn("HELI");
    let transport = spawn("TRANSPORT");
    let plain = spawn("PLAIN");
    let firer = spawn("FIRER");
    let aircraft = spawn("AIRCRAFT");
    let infantry = spawn("GI");
    let mut cargo = PassengerCargo::new(5, 0.0);
    cargo.board_forced_chain(&[(999, 1.0)]);
    sim.entity_mut_for_test(transport)
        .unwrap()
        .set_passenger_role_for_test(PassengerRole::transport(cargo));

    assert_eq!(
        deploy_key_command(&sim, Some(&rules), heli, sim.session().current_house()),
        Some(Command::DeployMcv { entity_id: heli })
    );
    assert_eq!(
        deploy_key_command(&sim, Some(&rules), transport, sim.session().current_house()),
        Some(Command::DeployMcv {
            entity_id: transport
        })
    );
    assert_eq!(
        deploy_key_command(&sim, Some(&rules), infantry, sim.session().current_house()),
        Some(Command::ToggleInfantryDeploy {
            entity_id: infantry
        })
    );
    assert_eq!(
        deploy_key_command(&sim, Some(&rules), aircraft, sim.session().current_house()),
        None
    );
    assert_eq!(
        deploy_key_command(&sim, Some(&rules), plain, sim.session().current_house()),
        None
    );
    // A `DeployFire=` Unit takes the DEPLOY event (Mission_Unload's
    // deploy-fire arm, `0x0073DCD3`).
    assert_eq!(
        deploy_key_command(&sim, Some(&rules), firer, sim.session().current_house()),
        Some(Command::DeployMcv { entity_id: firer })
    );
    sim.entity_mut_for_test(heli).unwrap().low_bridge_tube_state = Some(
        crate::sim::movement::tube_movement::LowBridgeTubeMovementState::for_test(
            crate::map::tube_facts::TubeId(0),
            0,
            crate::sim::components::DriveCoord::cell(10, 10, 0),
        ),
    );
    // CanDeploySlashUnload refuses a unit in a tube (`0x00700E59`).
    assert!(
        deploy_key_commands(&sim, Some(&rules), &[heli], sim.session().current_house()).is_empty()
    );
}

#[test]
fn ground_transport_deploy_key_matches_native_producer_and_keeps_self_click_separate() {
    use crate::app::input::context_order::{self_answers_no_deploy, self_click_command};
    use crate::app::input::entity_pick::HoverTargetKindWithId;
    use crate::app::types::HoverTargetKind;

    let native = crate::test_fixture::json("tools/spatial_oracle/ground_transport_deploy.json");
    let capacity = native["fixture"]["passengers"]
        .as_u64()
        .expect("native transport capacity") as u32;
    let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
        "[VehicleTypes]\n0=TRANSPORT\n[TRANSPORT]\nStrength=200\nPassengers={capacity}\n"
    )))
    .expect("transport D input rules");
    let producers = native["deploy_key"].as_array().expect("native D producers");
    assert_eq!(producers.len(), 2, "empty and loaded native D controls");
    for row in producers {
        let count = row["cargo"].as_u64().expect("native cargo count") as usize;
        let mut sim = player_sim();
        let transport = sim
            .spawn_object_limbo_at_height("TRANSPORT", "Americans", 10, 10, 0, 0, &rules)
            .expect("transport input fixture builds");
        let mut cargo = PassengerCargo::new(capacity, 0.0);
        let head: Vec<_> = (0..count).map(|index| (999 + index as u64, 1.0)).collect();
        cargo.board_forced_chain(&head);
        sim.entity_mut_for_test(transport)
            .unwrap()
            .set_passenger_role_for_test(PassengerRole::transport(cargo));
        let owner = sim
            .session()
            .current_house()
            .expect("local transport owner");
        // Opcode9 is the native event, not a Mission number. Both native
        // cargo rows send it; the shared receiver owns Mission Unload16.
        let event = match row["event_opcode"].as_u64().expect("native event opcode") {
            9 => Command::DeployMcv {
                entity_id: transport,
            },
            opcode => panic!("unexpected native transport D event {opcode}"),
        };
        assert_eq!(
            deploy_key_commands(&sim, Some(&rules), &[transport], Some(owner)),
            [event],
            "native cargo count {count} still sends the D event"
        );
        let hover = HoverTargetKindWithId {
            kind: HoverTargetKind::FriendlyUnit,
            stable_id: transport,
        };
        let clicked =
            self_click_command(&sim, &rules, None, owner, &[transport], Some(&hover), false);
        if count == 0 {
            assert_eq!(clicked, None, "empty self-click does not send the D event");
            assert!(self_answers_no_deploy(
                &sim,
                &rules,
                None,
                owner,
                &[transport],
                &hover,
                false
            ));
        } else {
            assert_eq!(
                clicked,
                Some(Command::UnloadPassengers {
                    transport_id: transport
                }),
                "loaded self-click retains its separate mission command"
            );
        }
    }
}

/// A unit in a tank bunker passes CanDeploySlashUnload and deploys out of
/// it, a loaded transport too: its Unload's bunker arm (`0x0073D63B`)
/// precedes the transport arm. Outside a bunker a plain tank is refused.
/// Both are armed: vt+0xA0 refuses a bunker link, so the walk admits them
/// through IsActive (vt+0x9C), which asks for a weapon.
#[test]
fn the_deploy_key_deploys_a_unit_out_of_its_tank_bunker() {
    use crate::sim::docking::bunker_link::BunkerLink;
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=TANK\n1=TRANSPORT\n\
         [TANK]\nStrength=300\nPrimary=GUN\n\
         [TRANSPORT]\nPassengers=2\nPrimary=GUN\n\
         [WeaponTypes]\n0=GUN\n[GUN]\nDamage=10\nRange=5\nWarhead=AP\n\
         [Warheads]\n0=AP\n[AP]\n",
    ))
    .expect("bunker deploy rules");
    let mut sim = player_sim();
    let mut spawn = |type_id| {
        sim.spawn_object_limbo_at_height(type_id, "Americans", 10, 10, 0, 0, &rules)
            .expect("the deploy key fixture builds")
    };
    let tank = spawn("TANK");
    let transport = spawn("TRANSPORT");
    let mut cargo = PassengerCargo::new(2, 0.0);
    cargo.board_forced_chain(&[(999, 1.0)]);
    sim.entity_mut_for_test(transport)
        .unwrap()
        .set_passenger_role_for_test(PassengerRole::transport(cargo));
    let owner = sim.session().current_house();
    assert!(deploy_key_commands(&sim, Some(&rules), &[tank], owner).is_empty());
    assert_eq!(
        deploy_key_command(&sim, Some(&rules), transport, owner),
        Some(Command::DeployMcv {
            entity_id: transport
        })
    );
    for id in [tank, transport] {
        sim.entity_mut_for_test(id)
            .unwrap()
            .bunker
            .set_bunker_link_for_test(BunkerLink::Installed(99));
    }
    assert_eq!(
        deploy_key_commands(&sim, Some(&rules), &[tank, transport], owner),
        [
            Command::DeployMcv { entity_id: tank },
            Command::DeployMcv {
                entity_id: transport
            }
        ]
    );
}

/// A building never passes CanDeploySlashUnload (`0x00465D30`), so the
/// deploy key does not undeploy a construction yard.
#[test]
fn the_deploy_key_does_not_undeploy_a_construction_yard() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=MCV\n[BuildingTypes]\n0=YARD\n\
         [MCV]\nDeploysInto=YARD\n\
         [YARD]\nStrength=1000\nConstructionYard=yes\nUndeploysInto=MCV\n",
    ))
    .expect("construction yard rules");
    let mut sim = player_sim();
    let yard = sim
        .spawn_object("YARD", "Americans", 10, 10, 0, &rules)
        .expect("the yard builds");
    assert!(
        deploy_key_commands(&sim, Some(&rules), &[yard], sim.session().current_house()).is_empty()
    );
}

#[test]
// Selection730AF0/730B28 reads Occupants, not the absorber's Cargo.
// Building CanDeploySlashUnload465D30 refuses the ordinary D walk.
fn the_deploy_key_does_nothing_for_a_loaded_bio_reactor() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[BuildingTypes]\n0=YAPOWR\n[YAPOWR]\nStrength=1000\n\
         InfantryAbsorb=yes\nPassengers=5\n",
    ))
    .expect("Bio Reactor deploy-key rules");
    let mut sim = player_sim();
    let reactor = sim
        .spawn_object("YAPOWR", "Americans", 10, 10, 0, &rules)
        .expect("Bio Reactor builds");
    let mut cargo = PassengerCargo::new(5, 0.0);
    cargo.board_forced_chain(&[(99, 1.0)]);
    sim.entity_mut_for_test(reactor)
        .unwrap()
        .set_passenger_role_for_test(PassengerRole::transport(cargo));

    assert!(
        deploy_key_commands(
            &sim,
            Some(&rules),
            &[reactor],
            sim.session().current_house()
        )
        .is_empty()
    );
}

/// `Selection::IssueDeployCommand`'s issue walk skips a berserk object
/// (`0x00730C82`).
#[test]
fn the_deploy_key_skips_a_berserk_object() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=HELI\n[HELI]\nIsSimpleDeployer=yes\n",
    ))
    .expect("deploy key rules");
    let mut sim = player_sim();
    let heli = sim
        .spawn_object_limbo_at_height("HELI", "Americans", 10, 10, 0, 0, &rules)
        .expect("the deploy key fixture builds");
    assert_eq!(
        deploy_key_commands(&sim, Some(&rules), &[heli], sim.session().current_house()),
        [Command::DeployMcv { entity_id: heli }]
    );
    sim.entity_mut_for_test(heli)
        .unwrap()
        .berserk
        .set_active_for_test(true);
    assert!(
        deploy_key_commands(&sim, Some(&rules), &[heli], sim.session().current_house()).is_empty()
    );
}

/// The issue walk admits an object the player may control (vt+0xA0) or
/// that is active (vt+0x9C, `0x00730C38`, `0x00730C46`): an offline one
/// is neither, and nor is another house's.
#[test]
fn the_deploy_key_skips_an_object_neither_controllable_nor_active() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=HELI\n[HELI]\nIsSimpleDeployer=yes\n",
    ))
    .expect("deploy key rules");
    let mut sim = player_sim();
    let mut spawn = |owner| {
        sim.spawn_object_limbo_at_height("HELI", owner, 10, 10, 0, 0, &rules)
            .expect("the deploy key fixture builds")
    };
    let (heli, theirs) = (spawn("Americans"), spawn("Russians"));
    assert_eq!(
        deploy_key_commands(
            &sim,
            Some(&rules),
            &[heli, theirs],
            sim.session().current_house()
        ),
        [Command::DeployMcv { entity_id: heli }]
    );
    sim.entity_mut_for_test(heli)
        .unwrap()
        .deactivated
        .set_for_test(true);
    assert!(
        deploy_key_commands(&sim, Some(&rules), &[heli], sim.session().current_house()).is_empty()
    );
}

/// The first walk (`0x00730B8C..0x00730C0E`) counts deployed and
/// undeployed `Deployer=` infantry, berserk ones included; a mixed
/// selection deploys only its undeployed members.
#[test]
fn a_mixed_selection_deploys_only_its_undeployed_deployers() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=GI\n[GI]\nDeployer=yes\nDeployFire=yes\n",
    ))
    .expect("deploy key rules");
    let mut sim = player_sim();
    let mut spawn = |rx: u16| {
        sim.spawn_object("GI", "Americans", rx, 5, 0, &rules)
            .expect("the deploy key fixture builds")
    };
    let (standing, deployed, gassed) = (spawn(3), spawn(4), spawn(5));
    sim.entity_mut_for_test(deployed)
        .unwrap()
        .mission_leaf
        .set_infantry_doing_verified(28)
        .unwrap();
    sim.entity_mut_for_test(gassed)
        .unwrap()
        .berserk
        .set_active_for_test(true);
    let toggle = |entity_id| Command::ToggleInfantryDeploy { entity_id };
    let commands = |selection: &[u64]| {
        deploy_key_commands(&sim, Some(&rules), selection, sim.session().current_house())
    };
    assert_eq!(commands(&[deployed]), [toggle(deployed)]);
    assert_eq!(commands(&[deployed, standing]), [toggle(standing)]);
    // The gassed man counts as undeployed but is not ordered.
    assert!(commands(&[gassed, deployed]).is_empty());
}

#[test]
fn retail_siege_chopper_deploy_key_uses_the_existing_command_in_both_directions() {
    let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &battle.rules;
    let mut sim = Simulation::new();
    let id = sim
        .spawn_object_limbo_at_height("SCHP", "Americans", 10, 10, 0, 0, rules)
        .expect("the retail Siege Chopper builds");
    for (deployed, begin, reverse) in [
        (false, false, false),
        (false, true, false),
        (true, false, false),
        (true, false, true),
    ] {
        sim.entity_mut_for_test(id)
            .unwrap()
            .set_unit_simple_deploy_for_test(deployed, begin, reverse);
        assert_eq!(
            deploy_key_command(&sim, Some(rules), id, sim.session().current_house()),
            Some(Command::DeployMcv { entity_id: id }),
        );
    }
}
