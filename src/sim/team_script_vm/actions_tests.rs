//! Rust regression tests for script actions that act on every member, and
//! for the building actions 46 and 47.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::team_ai_ini::TeamAiDefinitionSource;
use crate::sim::combat::TargetKind;
use crate::sim::house_state::HouseState;
use crate::sim::team_script_vm::{
    TeamScriptAction, TeamScriptDefinition, TeamScriptVm, TeamTaskForceDefinition,
    TeamTaskForceEntry, TeamTypeDefinition, member_type_identity,
};
use crate::util::save_codec;

#[test]
fn wall_sale_detach_clears_only_matching_team_target_fields() {
    use crate::sim::command::Command;
    use crate::sim::components::NavTargetRef;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::world::tests::{gsi_04_07_wall_sell_rules, gsi_04_07_wall_sell_seed_houses};

    let (rules, overlays) = gsi_04_07_wall_sell_rules(false, false);
    let mut sim = Simulation::with_seed(77);
    let (wall_owner, receiver) = gsi_04_07_wall_sell_seed_houses(&mut sim);
    let mut grid = crate::sim::overlay_grid::OverlayGrid::new(8, 8);
    grid.place_owned_wall(4, 4, 2, 0, wall_owner);
    sim.overlay_grid = Some(grid);
    sim.set_playfield_bounds_for_test(Some(crate::sim::arena_fixture::OPEN_PLAYFIELD));
    let script = sim.interner.intern("DETACH");
    let sold = Some(TeamTarget::Cell { x: 4, y: 4 });
    let other = Some(TeamTarget::Cell { x: 6, y: 6 });
    let mut teams = Vec::new();
    for (id, target, focus, expected_target, expected_focus) in [
        (10, sold, sold, None, None),
        (20, sold, other, None, other),
        (30, other, sold, other, None),
    ] {
        let mut actor = GameEntity::test_default(id, "MTNK", "Receiver", 2, 2);
        actor.set_owner_for_test(receiver);
        actor
            .navigation
            .set_nav_com_for_test(Some(NavTargetRef::Cell { rx: 4, ry: 4 }));
        sim.substrate.entities.insert(actor);
        let team = sim
            .team_script_vm
            .create_team(receiver, script, vec![id], 0);
        sim.team_assign_mission_target(team, target, &rules, None, FrameEffects::default());
        let state = sim.team_script_vm.teams.get_mut(&team).unwrap();
        state.focus = focus;
        state.zone = sold;
        teams.push((team, expected_target, expected_focus, id));
    }
    assert!(sim.apply_command_with_overlays(
        "Receiver",
        &Command::SellWallAtCell { x: 4, y: 4 },
        Some(&rules),
        Some(&overlays),
        FrameEffects::default(),
    ));
    for (team, expected_target, expected_focus, id) in teams {
        let state = sim.team_script_vm.team(team).unwrap();
        assert_eq!(state.mission_target, expected_target);
        assert_eq!(state.focus, expected_focus);
        assert_eq!(state.zone, sold, "detach does not broadcast pointer expiry");
        assert_eq!(state.members().collect::<Vec<_>>(), [id]);
        let actor = sim.substrate.entities.get(id).unwrap();
        assert_eq!(
            actor.navigation.nav_com(),
            Some(NavTargetRef::Cell { rx: 4, ry: 4 }),
            "70D562..57A clears Team pointers directly, without AssignMissionTarget member orders"
        );
    }
}

#[test]
fn object_detach_clears_team_targets_without_expiring_zone_or_members() {
    let mut sim = Simulation::new();
    let owner = sim.interner.intern("Owner");
    let script = sim.interner.intern("DETACH");
    let team = sim.team_script_vm.create_team(owner, script, vec![44], 0);
    let state = sim.team_script_vm.teams.get_mut(&team).unwrap();
    state.mission_target = Some(TeamTarget::Object(44));
    state.focus = Some(TeamTarget::Cell { x: 4, y: 4 });
    state.zone = state.mission_target;
    sim.stop_all_targeting_on_detach(44, None, None, FrameEffects::default());
    let state = sim.team_script_vm.team(team).unwrap();
    assert_eq!(state.mission_target, None);
    assert_eq!(state.focus, Some(TeamTarget::Cell { x: 4, y: 4 }));
    assert_eq!(state.zone, Some(TeamTarget::Object(44)));
    assert_eq!(state.members().collect::<Vec<_>>(), [44]);
}

#[test]
fn tesla_charger_team_recalls_an_automatic_target_but_keeps_an_explicit_attack() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = retail.rules;
    for automatic in [true, false] {
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x7E51A);
        let coil = sim
            .spawn_object("TESLA", "Americans", 12, 12, 0, &rules)
            .unwrap();
        let source = sim
            .spawn_object("SHK", "Americans", 11, 12, 0, &rules)
            .unwrap();
        assert!(sim.acquire_adjacent_tesla_coil(source, &rules, None, FrameEffects::default()));
        if !automatic {
            sim.assign_target_represented(
                source,
                Some(TargetKind::Entity(coil)),
                Some(&rules),
                None,
                FrameEffects::default(),
            )
            .unwrap();
        }
        let owner = sim.substrate.entities.get(source).unwrap().owner();
        let script = sim.interner.intern("TESLA-RECALL-SCRIPT");
        let type_id = sim.interner.intern("TESLA-RECALL-TYPE");
        let force = sim.interner.intern("TESLA-RECALL-FORCE");
        sim.team_script_vm.register_script(TeamScriptDefinition {
            id: script,
            source: TeamAiDefinitionSource::FixedAimd,
            actions: vec![TeamScriptAction {
                action_id: 3,
                argument: 0,
            }],
        });
        sim.team_script_vm
            .register_task_force(TeamTaskForceDefinition {
                id: force,
                group: -1,
                source: TeamAiDefinitionSource::FixedAimd,
                entries: vec![TeamTaskForceEntry {
                    count: 1,
                    member_type: member_type_identity(sim.substrate.entities.get(source).unwrap()),
                }],
            });
        sim.team_script_vm.register_team_type(TeamTypeDefinition {
            id: type_id,
            script_id: script,
            task_force_id: force,
            priority: 0,
            is_base_defense: false,
            suicide: false,
            aggressive: true,
            combined_movement_zone: crate::rules::locomotor_type::MovementZone::Normal,
            base_zone_relation_enforced: true,
            transport_crossing_required: false,
        });
        let team = sim
            .team_script_vm
            .create_team(owner, script, vec![source], 0);
        let state = sim.team_script_vm.teams.get_mut(&team).unwrap();
        state.team_type_id = Some(type_id);
        state.mission_target = Some(TeamTarget::Cell { x: 25, y: 25 });
        state.focus = state.mission_target;
        sim.team_coordinate_move(team, &rules, None, FrameEffects::default());
        let actor = sim.substrate.entities.get(source).unwrap();
        if automatic {
            assert_eq!(actor.attack_target.target(), None);
            assert!(!actor.tesla_charger.is_active());
            assert_eq!(
                actor.mission.effective(),
                MissionId::from_known(MissionType::Move)
            );
            assert!(actor.navigation.nav_com().is_some());
        } else {
            assert_eq!(actor.attack_target.target(), Some(TargetKind::Entity(coil)));
            assert!(actor.navigation.nav_com().is_none());
        }
    }
}

fn patrol_corpus() -> &'static serde_json::Value {
    crate::test_fixture::json("tools/team_patrol.json")
}

/// The expected retained bits and periods are executed native outputs, including
/// the decimal-float/percentage distinction. This also exercises the production
/// process reader, rather than independently parsing a scalar for the test.
#[test]
fn patrol_scan_production_reader_and_period_match_original() {
    use crate::rules::native_processing::{
        NativeRulesRegistryState, RulesLayerKind, RulesLayerStack,
    };
    use crate::util::native_x87::NativeF64Bits;
    let native = patrol_corpus();
    let default_bits = u64::from_str_radix(native["default_bits"].as_str().unwrap(), 16).unwrap();
    assert_eq!(
        crate::rules::ruleset::GeneralRules::default()
            .patrol_scan_minutes
            .bits(),
        default_bits
    );
    for row in native["rules"].as_array().unwrap() {
        // The oracle supplies this prior directly. Its 0.1 bits are not the
        // original ReadDouble("10%") result (which is one ulp lower).
        let initial = NativeF64Bits::from_bits(
            u64::from_str_radix(row["initial_bits"].as_str().unwrap(), 16).unwrap(),
        );
        let registry =
            NativeRulesRegistryState::default().with_patrol_scan_minutes_for_test(initial);
        let mut layers = RulesLayerStack::new(IniFile::empty());
        let pass = row["raw"].as_str().map_or_else(
            || "[AI]\nFixtureOnly=1\n".to_owned(),
            |raw| format!("[AI]\nPatrolScan={raw}\n"),
        );
        layers.push(RulesLayerKind::Scenario, IniFile::from_str(&pass));
        let processed = layers
            .process_with_fixed_art_and_registry_state(&IniFile::empty(), registry)
            .unwrap();
        let rules = RuleSet::from_processed_rules(&processed).unwrap();
        assert_eq!(
            rules.general.patrol_scan_minutes.bits(),
            u64::from_str_radix(row["output_bits"].as_str().unwrap(), 16).unwrap(),
            "{row}"
        );
        assert_eq!(
            i64::from(rules.general.patrol_scan_frames()),
            row["ticks"].as_i64().unwrap(),
            "{row}"
        );
    }
    // A General-only spelling must not enter the independently gated AI read.
    let misplaced = RuleSet::from_ini(&IniFile::from_str("[General]\nPatrolScan=10%\n")).unwrap();
    assert_eq!(misplaced.general.patrol_scan_minutes.bits(), default_bits);
}

#[test]
fn patrol_scan_retains_across_type_reset_and_changes_configuration_identity() {
    use crate::rules::native_processing::RulesLayerStack;
    let mut hashes = Vec::new();
    for raw in ["1%", "10%"] {
        let first = RulesLayerStack::new(IniFile::from_str(&format!("[AI]\nPatrolScan={raw}\n")))
            .process()
            .unwrap();
        let expected = RuleSet::from_processed_rules(&first)
            .unwrap()
            .general
            .patrol_scan_minutes;
        let (_, trace) = first.into_ini_and_native_type_construction_trace();
        let processed = RulesLayerStack::new(IniFile::empty())
            .process_with_fixed_art_and_registry_state(
                &IniFile::empty(),
                trace
                    .into_registry_state_discarding_events()
                    .destructive_reset(),
            )
            .unwrap();
        let rules = RuleSet::from_processed_rules(&processed).unwrap();
        assert_eq!(rules.general.patrol_scan_minutes, expected);
        hashes.push((rules.source_ini_hash(), rules.simulation_config_hash()));
    }
    assert_eq!(hashes[0].0, hashes[1].0);
    assert_ne!(hashes[0].1, hashes[1].1);
}

fn patrol_team_fixture(armed: bool) -> (Simulation, RuleSet, u64, Vec<u64>) {
    let text = if armed {
        let base = RULES.replace(
            "Strength=125\n",
            "Strength=125\nLeadershipRating=5\nPrimary=Rifle\n",
        );
        format!(
            "{base}\n\
          [Rifle]\nDamage=20\nROF=15\nRange=8\nProjectile=Invisible\nWarhead=SA\n\
          [Invisible]\nInvisible=yes\nAG=yes\n[SA]\nVerses=100%\n\
          [AI]\nPatrolScan=1%\n"
        )
    } else {
        format!("{RULES}\n[AI]\nPatrolScan=1%\n")
    };
    let rules = RuleSet::from_ini_with_fixed_art_for_test(
        &crate::sim::arena_fixture::clear_ground_ini(&text),
        &IniFile::from_str("[GAPOWR]\nFoundation=2x2\n"),
    )
    .unwrap();
    let mut sim = Simulation::with_seed(0x6ECCE0);
    crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
    let (team, members, _) = building_team(&mut sim, &rules, &[(16, 11), (2, 0)], &[]);
    sim.session.install_waypoints(
        &[(
            11,
            crate::map::waypoints::Waypoint {
                index: 11,
                rx: 30,
                ry: 20,
            },
        )]
        .into(),
    );
    for &member in &members {
        sim.team_script_vm.team_set_member_initiated(team, member);
    }
    let state = sim.team_script_vm.teams.get_mut(&team).unwrap();
    state.cursor = -1;
    state.advance_pending = true;
    (sim, rules, team, members)
}

#[test]
fn patrol_clears_a_lost_target_then_returns_to_the_waypoint_on_the_next_visit() {
    let (mut sim, rules, team, members) = patrol_team_fixture(false);
    let old = sim
        .spawn_object_at_height("GAPOWR", "Americans", 20, 10, 0, 0, &rules)
        .unwrap();
    {
        let state = sim.team_script_vm.teams.get_mut(&team).unwrap();
        state.cursor = 0;
        state.advance_pending = false;
    }
    sim.team_assign_mission_target(
        team,
        Some(TeamTarget::Object(old)),
        &rules,
        None,
        FrameEffects::default(),
    );
    for &member in &members {
        sim.team_member_assign_target(
            member,
            TeamTarget::Object(old),
            &rules,
            None,
            FrameEffects::default(),
        );
        sim.team_member_set_destination(
            member,
            Some(TeamTarget::Object(old)),
            &rules,
            None,
            FrameEffects::default(),
        );
    }
    sim.session
        .set_binary_frame_for_test(rules.general.patrol_scan_frames() as u32);
    sim.team_step_script(team, &rules, None, FrameEffects::default());
    let state = sim.team_script_vm.team(team).unwrap();
    assert_eq!(state.mission_target, None);
    assert_eq!(
        state.focus, None,
        "NULL scan does not reseed the waypoint in the same visit"
    );
    assert!(members.iter().all(|&member| {
        let entity = sim.substrate.entities.get(member).unwrap();
        entity.attack_target.target().is_none() && entity.navigation.nav_com().is_none()
    }));
    sim.session
        .set_binary_frame_for_test(sim.session.binary_frame() + 1);
    sim.team_step_script(team, &rules, None, FrameEffects::default());
    assert_eq!(
        sim.team_script_vm.team(team).unwrap().mission_target,
        Some(TeamTarget::Cell { x: 30, y: 20 })
    );
}

#[test]
fn patrol_scans_on_the_global_period_and_orders_members_to_attack() {
    let (mut sim, rules, team, members) = patrol_team_fixture(true);
    let enemy = sim
        .spawn_object_at_height("E1", "Americans", 14, 10, 0, 0, &rules)
        .unwrap();
    let owner = sim.interner.get("Russians").unwrap();
    crate::sim::vision::reveal_radius(&mut sim.fog, owner, 14, 10, 8);
    sim.session
        .set_binary_frame_for_test((rules.general.patrol_scan_frames() - 1) as u32);
    sim.team_step_script(team, &rules, None, FrameEffects::default());
    assert_eq!(
        sim.team_script_vm.team(team).unwrap().mission_target,
        Some(TeamTarget::Cell { x: 30, y: 20 })
    );
    sim.session
        .set_binary_frame_for_test(sim.session.binary_frame() + 1);
    sim.team_step_script(team, &rules, None, FrameEffects::default());
    assert_eq!(
        sim.team_script_vm.team(team).unwrap().mission_target,
        Some(TeamTarget::Object(enemy))
    );
    assert!(members.iter().all(|&member| {
        sim.substrate
            .entities
            .get(member)
            .unwrap()
            .attack_target
            .target()
            == Some(crate::sim::combat::TargetKind::Entity(enemy))
    }));
}

#[test]
fn patrol_reuses_the_native_leadership_rating_member_selection() {
    let corpus = patrol_corpus();
    for row in corpus["controls"].as_array().unwrap() {
        let scan = row["events"]
            .as_array()
            .unwrap()
            .iter()
            .find(|event| event[0] == "scan");
        let Some(scan) = scan else { continue };
        let inputs = row["members"].as_array().unwrap();
        let mut text = String::new();
        for (aircraft, section) in [(false, "InfantryTypes"), (true, "AircraftTypes")] {
            text.push_str(&format!("[{section}]\n"));
            for (index, input) in inputs.iter().enumerate() {
                if input["aircraft"].as_bool().unwrap() == aircraft {
                    text.push_str(&format!("{index}=MEMBER{index}\n"));
                }
            }
        }
        for (index, input) in inputs.iter().enumerate() {
            text.push_str(&format!(
                "[MEMBER{index}]\nStrength=100\nLeadershipRating={}\n",
                input["rating"]
            ));
        }
        let rules = RuleSet::from_ini(&IniFile::from_str(&text)).unwrap();
        let mut sim = Simulation::new();
        let ids: Vec<_> = inputs
            .iter()
            .enumerate()
            .map(|(index, input)| {
                let id = index as u64 + 1;
                let category = if input["aircraft"].as_bool().unwrap() {
                    EntityCategory::Aircraft
                } else {
                    EntityCategory::Infantry
                };
                let mut entity = GameEntity::test_default_of_category(
                    id,
                    &format!("MEMBER{index}"),
                    "OWNER",
                    10,
                    10,
                    category,
                );
                entity
                    .lifecycle
                    .set_object_alive_for_test(input["active"].as_bool().unwrap());
                entity
                    .lifecycle
                    .set_in_limbo_for_test(input["limbo"].as_bool().unwrap());
                entity
                    .health
                    .set_for_test(input["health"].as_i64().unwrap() as i32);
                sim.substrate.entities.insert(entity);
                id
            })
            .collect();
        sim.interner = crate::sim::intern::test_interner();
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let owner = sim.interner.get("OWNER").unwrap();
        let script = sim.interner.intern("SCRIPT");
        let team = sim
            .team_script_vm
            .create_team(owner, script, ids.clone(), 0);
        for (member, input) in sim
            .team_script_vm
            .teams
            .get_mut(&team)
            .unwrap()
            .members
            .iter_mut()
            .zip(inputs)
        {
            member.initiated = input["initiated"].as_bool().unwrap();
        }
        let actual = if row["placement_scope"].as_bool().unwrap() {
            sim.with_object_placement_scope(|sim| sim.team_leader(team, &rules))
        } else {
            sim.team_leader(team, &rules)
        };
        assert_eq!(
            actual,
            Some(ids[scan[1].as_u64().unwrap() as usize]),
            "{row}"
        );
        assert_eq!(scan[2], 1);
        assert_eq!(scan[4], 0, "Patrol ignores OnlyTargetHouseEnemy");
    }
}

#[test]
fn retail_all01_patrol_scripts_and_rules_use_the_production_readers() {
    use crate::rules::process_owner::NativeScenarioRulesPrefix;
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let map =
        crate::map::map_file::MapFile::from_bytes(assets.get_ref("ALL01UMD.MAP").unwrap()).unwrap();
    let named = assets.resolve_ref("ALL01UMD.INI").map(|_| {
        crate::rules::retail_sources::select_ini(&assets, "ALL01UMD.INI")
            .unwrap()
            .ini
    });
    let mut owner = crate::rules::retail_ini_fixture::retail_rules_owner(&assets);
    let (rules, _, _, _) = owner
        .load_scenario(
            NativeScenarioRulesPrefix::Campaign(named.as_ref()),
            &map.ini,
        )
        .unwrap()
        .into_parts();
    let corpus = patrol_corpus();
    let row = corpus["rules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["initial_bits"] == corpus["default_bits"] && row["raw"] == "0.016")
        .unwrap();
    assert_eq!(
        rules.general.patrol_scan_minutes.bits(),
        u64::from_str_radix(row["output_bits"].as_str().unwrap(), 16).unwrap()
    );
    assert_eq!(
        i64::from(rules.general.patrol_scan_frames()),
        row["ticks"].as_i64().unwrap()
    );
    let teams = crate::rules::team_ai_ini::TeamAiIniRegistry::load_retail(&assets, &map.ini, false)
        .unwrap();
    assert_eq!(
        teams
            .scripts
            .iter()
            .flat_map(|script| &script.actions)
            .filter(|action| action.action_id == 16)
            .count(),
        27
    );
    assert!(action_is_ported(16));
}

const RULES: &str = "\
[InfantryTypes]
0=E1
[BuildingTypes]
0=GAPOWR
[E1]
Strength=125
Speed=4
Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}
[GAPOWR]
Strength=750
";

fn rules() -> RuleSet {
    RuleSet::from_ini_with_fixed_art_for_test(
        &crate::sim::arena_fixture::clear_ground_ini(RULES),
        &IniFile::from_str("[GAPOWR]\nFoundation=2x2\n"),
    )
    .expect("team action rules")
}

#[test]
fn script17_replacement_and_next_visit_match_original_controls() {
    let native: serde_json::Value =
        serde_json::from_str(crate::test_fixture::text("tools/team_change_script.json")).unwrap();
    for row in native.as_array().unwrap() {
        // A live Rust Team always owns a Script attachment; TeamAI clears
        // StepCompleted before dispatch. The extra native controls establish
        // the NULL old-Script gate and lack of a completion write separately.
        if !row["old_script"].as_bool().unwrap() || row["advance_before"].as_bool().unwrap() {
            continue;
        }
        let rules = rules();
        let mut sim = Simulation::with_seed(0x6E972C);
        crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
        let ordinal = row["ordinal"].as_u64().unwrap() as usize;
        let (team, members, _) = building_team(&mut sim, &rules, &[(17, ordinal as i32)], &[]);
        while sim.team_script_vm.script_order.len() <= ordinal {
            let id = sim
                .interner
                .intern(&format!("SCRIPT_{}", sim.team_script_vm.script_order.len()));
            sim.team_script_vm.register_script(TeamScriptDefinition {
                id,
                actions: vec![TeamScriptAction {
                    action_id: 2,
                    argument: 0,
                }],
                source: TeamAiDefinitionSource::Scenario,
            });
        }
        let target = TeamTarget::Cell { x: 20, y: 21 };
        let expected_script = sim.team_script_vm.script_order[ordinal];
        let state = sim.team_script_vm.teams.get_mut(&team).unwrap();
        state.cursor = 0;
        state.advance_pending = row["advance_before"].as_bool().unwrap();
        state.mission_target = Some(target);
        state.focus = Some(target);
        let old_timer = state.guard_timer;
        let rng_before = sim.scenario_rng.native_state_hex();
        sim.team_step_script(team, &rules, None, FrameEffects::default());
        let state = sim.team_script_vm.team(team).unwrap();
        assert_eq!(state.script_id(), expected_script, "{row}");
        assert_eq!(i64::from(state.cursor()), row["cursor"].as_i64().unwrap());
        assert_eq!(
            state.advance_pending(),
            row["advance_after"].as_bool().unwrap()
        );
        assert_eq!(state.mission_target, Some(target), "{row}");
        assert_eq!(state.focus, Some(target), "{row}");
        assert_eq!(state.guard_timer, old_timer, "{row}");
        assert_eq!(state.members().collect::<Vec<_>>(), members, "{row}");
        if !state.advance_pending() {
            sim.team_step_script(team, &rules, None, FrameEffects::default());
            let state = sim.team_script_vm.team(team).unwrap();
            assert_eq!(
                i64::from(state.cursor()),
                row["next_cursor"].as_i64().unwrap()
            );
            assert_eq!(
                state.advance_pending(),
                row["next_advance"].as_bool().unwrap()
            );
            assert!(state.refusal().is_none());
        }
        assert_eq!(sim.scenario_rng.native_state_hex(), rng_before);
        let restored: TeamScriptVm =
            save_codec::deserialize(&save_codec::serialize(&sim.team_script_vm).unwrap()).unwrap();
        assert_eq!(restored.team(team), sim.team_script_vm.team(team));
    }
    assert!(action_is_ported(17));
}

#[test]
fn retail_sov01_script17_uses_the_production_script_registry() {
    use crate::rules::process_owner::NativeScenarioRulesPrefix;
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let bytes = assets.get_ref("SOV01UMD.MAP").unwrap();
    assert_eq!(
        crate::util::sha256::sha256_hex(bytes),
        "498b989a4b6f579dee0904a77f1fabf982c14786db21a46c41900fc8344ec72b"
    );
    let map = crate::map::map_file::MapFile::from_bytes(bytes).unwrap();
    let named = assets.resolve_ref("SOV01UMD.INI").map(|_| {
        crate::rules::retail_sources::select_ini(&assets, "SOV01UMD.INI")
            .unwrap()
            .ini
    });
    let mut rules_owner = crate::rules::retail_ini_fixture::retail_rules_owner(&assets);
    let (rules, _, _, _) = rules_owner
        .load_scenario(
            NativeScenarioRulesPrefix::Campaign(named.as_ref()),
            &map.ini,
        )
        .unwrap()
        .into_parts();
    let registry =
        crate::rules::team_ai_ini::TeamAiIniRegistry::load_retail(&assets, &map.ini, false)
            .unwrap();
    let mut sim = Simulation::new();
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.install_team_ai_registry(&registry, &rules).unwrap();
    let vm = &sim.team_script_vm;
    let mut switches = Vec::new();
    for (name, ordinal) in [
        ("0442D4BC", 107),
        ("0442F04C", 108),
        ("0442D8DC", 109),
        ("0442F52C", 110),
        ("044340FC", 111),
        ("0442D05C", 112),
    ] {
        let id = sim.interner.get(name).unwrap();
        let script = &vm.scripts[&id];
        assert_eq!(
            script.actions,
            [
                TeamScriptAction {
                    action_id: 50,
                    argument: 120
                },
                TeamScriptAction {
                    action_id: 17,
                    argument: ordinal
                },
            ]
        );
        let target = vm.script_order[ordinal as usize];
        assert!(vm.scripts.contains_key(&target));
        switches.push((id, target));
    }
    // Resolve numeric references through the installed fixed+scenario owner,
    // rather than treating 107..112 as map-local ScriptTypes list positions.
    for (source, expected) in switches {
        let owner = sim.interner.intern("Americans");
        let team = sim.team_script_vm.create_team(owner, source, Vec::new(), 0);
        {
            let state = sim.team_script_vm.teams.get_mut(&team).unwrap();
            state.cursor = 0;
            state.advance_pending = true;
        }
        sim.team_step_script(team, &rules, None, FrameEffects::default());
        let state = sim.team_script_vm.team(team).unwrap();
        assert_eq!(state.script_id(), expected);
        assert_eq!(state.cursor(), -1);
        assert!(!state.advance_pending());
        assert!(state.refusal().is_none());
    }
}

/// The building tests' power plants: the Russians' own, the Americans' near
/// and far ones, and the Germans'.
const PLANTS: [(&str, u16, u16); 4] = [
    ("Russians", 13, 13),
    ("Americans", 20, 10),
    ("Americans", 30, 10),
    ("Germans", 16, 14),
];

/// A Russian team of three E1 at (10..=12, 10) running `actions`, with the
/// Americans as the Russians' enemy and power plants of the given owners at
/// the given cells. Answers the team, its members and the plants in order.
fn building_team(
    sim: &mut Simulation,
    rules: &RuleSet,
    actions: &[(i32, i32)],
    plants: &[(&str, u16, u16)],
) -> (u64, Vec<u64>, Vec<u64>) {
    let mut house = |name: &str, side| {
        let id = sim.interner.intern(name);
        sim.houses
            .register(HouseState::new(id, side, None, false, 0, 10));
        id
    };
    let russians = house("Russians", 1);
    let americans = house("Americans", 0);
    house("Germans", 0);
    sim.houses
        .get_mut(&russians)
        .unwrap()
        .set_enemy_house_for_test(Some(americans));
    let plants = plants
        .iter()
        .map(|&(owner, rx, ry)| {
            sim.spawn_object_at_height("GAPOWR", owner, rx, ry, 0, 0, rules)
                .expect("GAPOWR spawns")
        })
        .collect();
    let members: Vec<u64> = (10..=12)
        .map(|rx| {
            sim.spawn_object_at_height("E1", "Russians", rx, 10, 0, 0, rules)
                .expect("E1 spawns")
        })
        .collect();
    let script = sim.interner.intern("BUILDING SCRIPT");
    sim.team_script_vm.register_script(TeamScriptDefinition {
        id: script,
        source: TeamAiDefinitionSource::FixedAimd,
        actions: actions
            .iter()
            .map(|&(action_id, argument)| TeamScriptAction {
                action_id,
                argument,
            })
            .collect(),
    });
    let team = sim
        .team_script_vm
        .create_team(russians, script, members.clone(), 0);
    (team, members, plants)
}

#[test]
fn script59_building_gate_and_retained_ammo_route_match_original_controls() {
    let rows: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/team_attack_waypoint_building.json",
    ))
    .unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 6);
    for row in rows.as_array().unwrap() {
        let input = &row["input"];
        let type_ammo = input["type_ammo"].as_i64().unwrap();
        // One physical E1 section must carry the corpus's Type+684 prior.
        // Appending a second [E1] body leaves the first body's default Ammo.
        let mut ini = crate::sim::arena_fixture::clear_ground_ini(RULES);
        ini.merge(&IniFile::from_str(&format!("[E1]\nAmmo={type_ammo}\n")));
        let rules = RuleSet::from_ini_with_fixed_art_for_test(
            &ini,
            &IniFile::from_str("[GAPOWR]\nFoundation=2x2\n"),
        )
        .unwrap();
        assert_eq!(
            i64::from(rules.object("E1").unwrap().ammo),
            type_ammo,
            "{input}: supplied native type ammo"
        );
        let mut sim = Simulation::new();
        sim.session.set_binary_frame_for_test(1);
        crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
        let first = input["first"].as_bool().unwrap();
        let has_building = input["blocker"] == "building" || !first;
        let plants = if has_building {
            vec![("Americans", 20, 10)]
        } else {
            vec![]
        };
        let (team, members, buildings) = building_team(&mut sim, &rules, &[(59, 11)], &plants);
        if input["blocker"] == "other" {
            sim.spawn_object_at_height("E1", "Americans", 20, 10, 0, 0, &rules)
                .unwrap();
        }
        sim.session.install_waypoints(
            &[(
                11,
                crate::map::waypoints::Waypoint {
                    index: 11,
                    rx: 20,
                    ry: 10,
                },
            )]
            .into(),
        );
        for &member in &members {
            sim.team_script_vm.team_set_member_initiated(team, member);
            // Supplied native raw+2FC prior for this control. Production
            // non-Aircraft ammunition writers remain the documented owner
            // residual; this fixture does not invent a working reload loop.
            sim.substrate
                .entities
                .get_mut(member)
                .unwrap()
                .install_ammo_for_test(Some(crate::sim::ammo::TechnoAmmo::new(
                    input["ammo"].as_i64().unwrap() as i32,
                )));
        }
        if input["count"] == 0 {
            for &member in &members {
                sim.team_remove_member(team, member, false, Some(&rules), FrameEffects::default());
            }
        }
        if !first {
            sim.team_assign_mission_target(
                team,
                Some(TeamTarget::Object(buildings[0])),
                &rules,
                None,
                FrameEffects::default(),
            );
            // Changed waypoint now contains no building. A repeated visit
            // must retain the old target without running the first query.
            sim.session.install_waypoints(
                &[(
                    11,
                    crate::map::waypoints::Waypoint {
                        index: 11,
                        rx: 30,
                        ry: 20,
                    },
                )]
                .into(),
            );
        }
        let state = sim.team_script_vm.teams.get_mut(&team).unwrap();
        state.cursor = if first { -1 } else { 0 };
        state.advance_pending = first;
        sim.team_step_script(team, &rules, None, FrameEffects::default());
        let state = sim.team_script_vm.team(team).unwrap();
        assert_eq!(
            state.mission_target.is_some(),
            row["target_present"].as_bool().unwrap(),
            "{input}"
        );
        assert_eq!(
            state.advance_pending,
            row["advance"].as_bool().unwrap(),
            "{input}"
        );
        assert!(state.refusal().is_none(), "{input}");
        if row["coordinate_attack"].as_bool().unwrap() {
            assert_eq!(
                state.mission_target,
                Some(TeamTarget::Object(buildings[0])),
                "{input}"
            );
            for &member in &members {
                assert_eq!(
                    sim.substrate
                        .entities
                        .get(member)
                        .unwrap()
                        .attack_target
                        .target(),
                    Some(crate::sim::combat::TargetKind::Entity(buildings[0])),
                    "{input}: existing CoordinateAttack consumer"
                );
            }
        }
        // The native corpus establishes selector/ammo control routing. This
        // real Rust consumer check does not claim whole joined native scene
        // parity for its separately tested mission/blocker/attack owners.
    }
}

#[test]
fn retail_all01_script59_inputs_fit_the_existing_ammunition_owner() {
    use crate::rules::process_owner::NativeScenarioRulesPrefix;
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let map =
        crate::map::map_file::MapFile::from_bytes(assets.get_ref("ALL01UMD.MAP").unwrap()).unwrap();
    let named = assets.resolve_ref("ALL01UMD.INI").map(|_| {
        crate::rules::retail_sources::select_ini(&assets, "ALL01UMD.INI")
            .unwrap()
            .ini
    });
    let mut owner = crate::rules::retail_ini_fixture::retail_rules_owner(&assets);
    let (rules, _, _, _) = owner
        .load_scenario(
            NativeScenarioRulesPrefix::Campaign(named.as_ref()),
            &map.ini,
        )
        .unwrap()
        .into_parts();
    let registry =
        crate::rules::team_ai_ini::TeamAiIniRegistry::load_retail(&assets, &map.ini, false)
            .unwrap();
    let mut sim = Simulation::new();
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.install_team_ai_registry(&registry, &rules).unwrap();
    let programs: Vec<_> = sim
        .team_script_vm
        .scripts
        .values()
        .filter(|script| script.actions.iter().any(|action| action.action_id == 59))
        .map(|script| script.id)
        .collect();
    let count: usize = sim
        .team_script_vm
        .scripts
        .values()
        .flat_map(|script| &script.actions)
        .filter(|action| action.action_id == 59)
        .count();
    assert_eq!(programs.len(), 10, "physical Script59 programs");
    assert_eq!(count, 11, "physical referenced Script59 steps");
    for definition in sim
        .team_script_vm
        .team_types
        .values()
        .filter(|definition| programs.contains(&definition.script_id))
    {
        for entry in &sim.team_script_vm.task_forces[&definition.task_force_id].entries {
            let name = sim.interner.resolve(entry.member_type.id);
            let object = rules
                .object_in_category(entry.member_type.category, name)
                .unwrap();
            assert!(
                entry.member_type.category == crate::rules::object_type::ObjectCategory::Aircraft
                    || object.ammo <= 0,
                "ALL01 Script59 {name} needs the still-missing non-Aircraft current ammunition owner; do not assume it is full"
            );
        }
    }
    assert!(action_is_ported(59));
}

/// Advance until `done` holds for the team, at most `frames` frames.
fn run_team(
    sim: &mut Simulation,
    rules: &RuleSet,
    grid: &crate::sim::pathfinding::PathGrid,
    team: u64,
    frames: usize,
    done: impl Fn(&crate::sim::team_script_vm::TeamScriptState) -> bool,
) {
    for _ in 0..frames {
        if sim.team_script_vm.team(team).is_none_or(&done) {
            return;
        }
        let commands = sim.take_due_commands();
        sim.advance_tick(&commands, Some(rules), Some(grid), None, 33);
    }
}

/// Action 21 (`0x006E9650`): every member scatters from the null coordinate,
/// forced, so each takes a neighbouring cell to walk to, and the action is
/// finished at once.
#[test]
fn scatter_sends_every_member_to_a_neighbouring_cell() {
    let rules = rules();
    let mut sim = Simulation::with_seed(0x6E9650);
    let owner = sim.interner.intern("Russians");
    sim.houses
        .register(HouseState::new(owner, 1, None, false, 0, 10));
    let grid = crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
    let starts = [(10, 10), (11, 10), (12, 10)];
    let members: Vec<u64> = starts
        .iter()
        .map(|&(rx, ry)| {
            sim.spawn_object_at_height("E1", "Russians", rx, ry, 0, 0, &rules)
                .expect("E1 spawns")
        })
        .collect();
    let script = sim.interner.intern("SCATTER SCRIPT");
    sim.team_script_vm.register_script(TeamScriptDefinition {
        id: script,
        source: TeamAiDefinitionSource::FixedAimd,
        actions: vec![
            TeamScriptAction {
                action_id: 21,
                argument: 0,
            },
            TeamScriptAction {
                action_id: 49,
                argument: 0,
            },
        ],
    });
    let team = sim
        .team_script_vm
        .create_team(owner, script, members.clone(), 0);

    for _ in 0..30 {
        if sim
            .team_script_vm
            .team(team)
            .is_some_and(|team| team.cursor() >= 1)
        {
            break;
        }
        let commands = sim.take_due_commands();
        sim.advance_tick(&commands, Some(&rules), Some(&grid), None, 33);
    }

    let state = sim.team_script_vm.team(team).expect("the team lives");
    assert!(state.cursor() >= 1, "action 21 finished");
    assert!(state.refusal().is_none());
    for (member, start) in members.iter().zip(starts) {
        match sim
            .substrate
            .entities
            .get(*member)
            .unwrap()
            .navigation
            .nav_com()
        {
            Some(NavTargetRef::Cell { rx, ry }) => assert!(
                rx.abs_diff(start.0) <= 1 && ry.abs_diff(start.1) <= 1 && (rx, ry) != start,
                "member {member} heads for ({rx}, {ry}) from {start:?}"
            ),
            other => panic!("member {member} did not scatter: {other:?}"),
        }
    }
}

/// Through the production readers, the one retail script naming action 21
/// (`0CA2192C-G` "Soviet Flak Defense": 58, 21, 11) runs only ported actions,
/// so the one TeamType running it no longer idles on 21.
#[test]
fn the_retail_scatter_script_runs_ported_actions() {
    use crate::rules::retail_ini_fixture::retail_ini;
    use crate::rules::team_ai_ini::TeamAiIniRegistry;
    let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules() else {
        return;
    };
    let aimd = retail_ini("aimd.ini").expect("retail AIMD");
    let registry = TeamAiIniRegistry::from_sources(&aimd, &IniFile::from_str(""), true);
    let mut sim = Simulation::new();
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.install_team_ai_registry(&registry, &rules)
        .expect("retail AIMD installs");
    let vm = &sim.team_script_vm;
    let scripts: Vec<_> = vm
        .scripts
        .values()
        .filter(|script| script.actions.iter().any(|action| action.action_id == 21))
        .collect();
    assert_eq!(scripts.len(), 1);
    let actions: Vec<i32> = scripts[0]
        .actions
        .iter()
        .map(|action| action.action_id)
        .collect();
    assert_eq!(actions, [58, 21, 11]);
    assert!(actions.iter().all(|&action| action_is_ported(action)));
    let running = vm
        .team_types
        .values()
        .filter(|team_type| team_type.script_id == scripts[0].id)
        .count();
    assert_eq!(running, 1);
}

/// Action 46 (`0x006EE310`): entered with `first` (after action 24), the team
/// takes the power plant its first member's search picks, skipping its own
/// house's: by mode 2 the nearest, the Germans' (a house it is not allied
/// with); by mode 3 the farthest, the Americans' far one. Its members are
/// sent at it. Mode 4 picks none and the action finishes at once.
#[test]
fn the_team_attacks_the_building_its_first_member_finds() {
    let rules = rules();
    for (mode, pick) in [(2, Some(3)), (3, Some(2)), (4, None)] {
        let mut sim = Simulation::with_seed(0x6EE310);
        let grid = crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
        let (team, members, plants) = building_team(
            &mut sim,
            &rules,
            &[(24, 0), (46, mode << 16), (5, 100)],
            &PLANTS,
        );
        run_team(&mut sim, &rules, &grid, team, 4, |team| {
            team.cursor() >= 2 || team.mission_target.is_some()
        });
        let state = sim.team_script_vm.team(team).expect("the team lives");
        assert!(state.refusal().is_none(), "mode {mode}");
        let Some(pick) = pick else {
            assert_eq!(state.cursor(), 2, "mode {mode}: the action finished");
            assert_eq!(state.mission_target, None, "mode {mode}");
            continue;
        };
        assert_eq!(state.cursor(), 1, "mode {mode}");
        assert_eq!(
            state.mission_target,
            Some(TeamTarget::Object(plants[pick])),
            "mode {mode}"
        );
        let first = sim.substrate.entities.get(members[0]).unwrap();
        assert_eq!(
            first.attack_target.target(),
            Some(crate::sim::combat::TargetKind::Entity(plants[pick])),
            "mode {mode}: the first member's target"
        );
    }
}

/// Action 47 (`0x006EE3F0`): the team's mission target becomes a cell by the
/// nearest power plant not its own (mode 2: the Germans' at (16, 14)), and
/// its members move. Mode 4 finds none, and the action finishes at once.
#[test]
fn the_team_moves_to_the_building_its_first_member_finds() {
    let rules = rules();
    for mode in [2, 4] {
        let mut sim = Simulation::with_seed(0x6EE3F0);
        let grid = crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
        let (team, members, _) = building_team(
            &mut sim,
            &rules,
            &[(24, 0), (47, mode << 16), (5, 100)],
            &PLANTS,
        );
        run_team(&mut sim, &rules, &grid, team, 4, |team| team.cursor() >= 2);
        let state = sim.team_script_vm.team(team).expect("the team lives");
        assert!(state.refusal().is_none(), "mode {mode}");
        if mode == 4 {
            assert_eq!(state.cursor(), 2, "the action finished");
            continue;
        }
        let Some(TeamTarget::Cell { x, y }) = state.mission_target else {
            panic!("mode {mode}: {:?}", state.mission_target);
        };
        assert!(
            x.abs_diff(16) <= 2 && y.abs_diff(14) <= 2,
            "the cell ({x}, {y}) lies by the Germans' plant"
        );
        assert_eq!(state.cursor(), 1, "the team is on its way");
        assert!(
            members.iter().any(|&member| sim
                .substrate
                .entities
                .get(member)
                .unwrap()
                .navigation
                .nav_com()
                .is_some()),
            "the members move"
        );
    }
}

/// Run action 46 with `argument` (after action 24) until the team takes a
/// mission target or finishes the action; answers the team's state.
fn attack_building_target(
    sim: &mut Simulation,
    rules: &RuleSet,
    grid: &crate::sim::pathfinding::PathGrid,
    team: u64,
) -> (i32, Option<TeamTarget>) {
    run_team(sim, rules, grid, team, 4, |team| {
        team.cursor() >= 2 || team.mission_target.is_some()
    });
    let state = sim.team_script_vm.team(team).expect("the team lives");
    assert!(state.refusal().is_none());
    (state.cursor(), state.mission_target)
}

/// The threat modes read the threat the Russians hold at each plant's
/// GetCoords cell, one cell south-east of a 2x2 plant's Location
/// (`0x006EEC83..0x006EECDB`), in 4x4 blocks: plant A at (11, 20) reads
/// block (3, 5), plant B at (23, 20) block (6, 5). Without threat the first
/// plant wins either mode, a tie keeping the first. Threat 1000 at block
/// (1, 5) and 400 at (7, 5) spill 500 onto A's Location block and 200 onto
/// B's GetCoords block, so mode 1 (most threat) takes B and mode 0 (least)
/// takes A, where Location reads would pick the other.
#[test]
fn the_threat_modes_read_each_building_s_getcoords_cell() {
    let rules = rules();
    for (threat, mode, pick) in [(false, 0, 0), (false, 1, 0), (true, 0, 0), (true, 1, 1)] {
        let mut sim = Simulation::with_seed(0x6EEBD0);
        let grid = crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
        let plants = [("Americans", 11, 20), ("Americans", 23, 20)];
        let (team, _, plants) = building_team(
            &mut sim,
            &rules,
            &[(24, 0), (46, mode << 16), (5, 100)],
            &plants,
        );
        let russians = sim.interner.intern("Russians");
        let house = sim.houses.get_mut(&russians).unwrap();
        house.clear_spatial_threat();
        if threat {
            house.adjust_spatial_threat((5, 21), 1000).unwrap();
            house.adjust_spatial_threat((29, 21), 400).unwrap();
        }
        let (_, target) = attack_building_target(&mut sim, &rules, &grid, team);
        assert_eq!(
            target,
            Some(TeamTarget::Object(plants[pick])),
            "threat {threat}, mode {mode}"
        );
    }
}

/// The search's admission inputs (`0x006EEC27..0x006EEC6A`, `0x006EE32B`):
/// a house the Russians are allied with is skipped unless it is
/// `MultiplayPassive=`, and with `OnlyTargetHouseEnemy=` only the current
/// enemy's count, none without an enemy. Mode 2 takes the nearest admitted:
/// the Germans' plant at (16, 14), else the Americans' at (20, 10).
#[test]
fn the_building_search_admits_by_alliance_passivity_and_enemy() {
    use crate::sim::team_script_vm::{TeamTaskForceDefinition, TeamTypeDefinition};
    let rules = rules();
    // (allied with the Germans, Germans passive, only the enemy, an enemy)
    let cases = [
        ((false, false, false, true), Some(3)),
        ((true, false, false, true), Some(1)),
        ((true, true, false, true), Some(3)),
        ((false, false, true, true), Some(1)),
        ((false, false, true, false), None),
    ];
    for ((allied, passive, only_enemy, enemy), pick) in cases {
        let case = (allied, passive, only_enemy, enemy);
        let mut sim = Simulation::with_seed(0x6EE310);
        let grid = crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
        let (team, _, plants) = building_team(
            &mut sim,
            &rules,
            &[(24, 0), (46, 2 << 16), (5, 100)],
            &PLANTS,
        );
        let russians = sim.interner.intern("Russians");
        let germans = sim.interner.intern("Germans");
        if allied {
            sim.house_alliances_mut_for_test()
                .entry("RUSSIANS".into())
                .or_default()
                .insert("GERMANS".into());
        }
        sim.houses
            .get_mut(&germans)
            .unwrap()
            .set_multiplay_passive_for_test(passive);
        if !enemy {
            sim.houses
                .get_mut(&russians)
                .unwrap()
                .set_enemy_house_for_test(None);
        }
        if only_enemy {
            let team_type = sim.interner.intern("BUILDING TEAMTYPE");
            let task_force = sim.interner.intern("BUILDING TASKFORCE");
            let vm = &mut sim.team_script_vm;
            let script_id = vm.teams[&team].script_id;
            vm.register_task_force(TeamTaskForceDefinition {
                id: task_force,
                source: TeamAiDefinitionSource::FixedAimd,
                group: -1,
                entries: Vec::new(),
            });
            vm.register_team_type(TeamTypeDefinition {
                id: team_type,
                script_id,
                task_force_id: task_force,
                priority: 0,
                is_base_defense: false,
                suicide: false,
                aggressive: false,
                combined_movement_zone: crate::rules::locomotor_type::MovementZone::Normal,
                base_zone_relation_enforced: true,
                transport_crossing_required: false,
            });
            vm.team_type_ini
                .get_mut(&team_type)
                .unwrap()
                .only_target_house_enemy = true;
            vm.teams.get_mut(&team).unwrap().team_type_id = Some(team_type);
        }
        let (cursor, target) = attack_building_target(&mut sim, &rules, &grid, team);
        match pick {
            Some(pick) => assert_eq!(target, Some(TeamTarget::Object(plants[pick])), "{case:?}"),
            None => assert_eq!((cursor, target), (2, None), "{case:?}"),
        }
    }
}

/// Through the production readers: the retail tech-capture scripts' building
/// arguments name the Tech Oil Derrick (`131143` = mode 2, index 71) and the
/// Tech Airport (`131142`, index 70), and the TeamTypes that run them reach
/// only ported actions.
#[test]
fn the_retail_tech_capture_scripts_run_ported_actions() {
    use crate::rules::retail_ini_fixture::retail_ini;
    use crate::rules::team_ai_ini::TeamAiIniRegistry;
    let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules() else {
        return;
    };
    for (name, index) in [("CAOILD", 71), ("CAAIRP", 70)] {
        assert_eq!(rules.object(name).unwrap().base_plan_type_index, index);
    }
    let aimd = retail_ini("aimd.ini").expect("retail AIMD");
    let registry = TeamAiIniRegistry::from_sources(&aimd, &IniFile::from_str(""), true);
    let mut sim = Simulation::new();
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.install_team_ai_registry(&registry, &rules)
        .expect("retail AIMD installs");
    let vm = &sim.team_script_vm;
    let capture: Vec<_> = vm
        .scripts
        .values()
        .filter(|script| {
            script
                .actions
                .first()
                .is_some_and(|action| action.action_id == 46)
                && matches!(script.actions[0].argument, 131142 | 131143)
        })
        .collect();
    assert!(!capture.is_empty());
    for script in &capture {
        assert!(
            script
                .actions
                .iter()
                .all(|action| action_is_ported(action.action_id))
        );
    }
    let running = vm
        .team_types
        .values()
        .filter(|team_type| {
            capture
                .iter()
                .any(|script| script.id == team_type.script_id)
        })
        .count();
    assert_eq!(running, 6);
}

/// Action 9's types: an MCV that deploys into a 4x4 yard (as retail's), a simple
/// deployer, a `Deployer=` infantryman and a plain one, on buildable ground.
const DEPLOY_RULES: &str = "\
[InfantryTypes]
0=E1
1=GI
[VehicleTypes]
0=AMCV
1=SIEGE
[BuildingTypes]
0=YARD
[Clear]
Buildable=yes
[E1]
Strength=125
Speed=4
Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}
[GI]
Strength=125
Speed=4
Deployer=yes
Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}
[AMCV]
Strength=1000
Speed=5
ROT=5
DeploysInto=YARD
Locomotor={4A582741-9839-11D1-B709-00A024DDAFD1}
[SIEGE]
Strength=300
Speed=6
IsSimpleDeployer=yes
Locomotor={4A582741-9839-11D1-B709-00A024DDAFD1}
[YARD]
Strength=1000
ConstructionYard=yes
[Guard]
Rate=0.016
[Unload]
Rate=0.016
";

struct DeployTeam {
    sim: Simulation,
    rules: RuleSet,
    grid: crate::sim::pathfinding::PathGrid,
    team: u64,
    members: Vec<u64>,
}

impl DeployTeam {
    /// The Russians, a computer house that does not build its own base, on
    /// a flat arena, with one object per `(type, cell)` as a team running
    /// action 9, then action 2 (stay).
    fn new(spawns: &[(&str, u16, u16)]) -> Self {
        let rules = RuleSet::from_ini_with_fixed_art_for_test(
            &crate::sim::arena_fixture::clear_ground_ini(DEPLOY_RULES),
            &IniFile::from_str("[YARD]\nFoundation=4x4\n"),
        )
        .expect("deploy rules");
        let mut sim = Simulation::with_seed(0x6ED4D0);
        let owner = sim.interner.intern("Russians");
        sim.houses
            .register(HouseState::new(owner, 1, None, false, 0, 10));
        let grid = crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
        let members: Vec<u64> = spawns
            .iter()
            .map(|&(name, rx, ry)| {
                sim.spawn_object_at_height(name, "Russians", rx, ry, 0, 0, &rules)
                    .expect("member spawns")
            })
            .collect();
        let script = sim.interner.intern("DEPLOY SCRIPT");
        sim.team_script_vm.register_script(TeamScriptDefinition {
            id: script,
            source: TeamAiDefinitionSource::FixedAimd,
            actions: [(9, 0), (2, 0)]
                .into_iter()
                .map(|(action_id, argument)| TeamScriptAction {
                    action_id,
                    argument,
                })
                .collect(),
        });
        let team = sim
            .team_script_vm
            .create_team(owner, script, members.clone(), 0);
        Self {
            sim,
            rules,
            grid,
            team,
            members,
        }
    }

    fn tick(&mut self) {
        let commands = self.sim.take_due_commands();
        self.sim
            .advance_tick(&commands, Some(&self.rules), Some(&self.grid), None, 33);
    }

    /// Runs action 9 for the team once, outside a frame.
    fn deploy_action(&mut self) {
        self.sim
            .team_action_deploy(self.team, &self.rules, None, FrameEffects::default());
    }

    /// Whether `id` was ordered to Unload: queued, or already its mission.
    fn unloading(&self, id: u64) -> bool {
        let unload = MissionId::from_known(MissionType::Unload);
        self.sim.substrate.entities.get(id).is_some_and(|entity| {
            entity.mission.queued() == unload || entity.mission.current() == unload
        })
    }

    /// The cells of the yards standing.
    fn yards(&self) -> Vec<(u16, u16)> {
        self.sim
            .substrate
            .entities
            .values()
            .filter(|entity| {
                !entity.death.is_dying() && self.sim.interner.resolve(entity.type_ref()) == "YARD"
            })
            .map(|entity| (entity.position.rx(), entity.position.ry()))
            .collect()
    }

    /// The team is gone or past action 9.
    fn past_deploy(&self) -> bool {
        self.sim
            .team_script_vm
            .team(self.team)
            .is_none_or(|team| team.cursor() >= 1)
    }
}

/// Action 9 (`0x006ED4D0`) on a team's MCV standing on clear ground: the
/// yard may stand from the MCV's cell, so the MCV drops its destination and
/// queues Unload, and its Mission_Unload deploys it (the yard one cell up
/// and left of it, as Deploy places). The action holds while the MCV is in
/// the team; the MCV leaves it as it becomes the yard, and the team moves on.
#[test]
fn script_action_9_deploys_the_team_s_mcv() {
    let mut team = DeployTeam::new(&[("AMCV", 20, 22)]);
    let mcv = team.members[0];
    team.tick();
    assert!(team.unloading(mcv), "the MCV was ordered to Unload");
    assert!(!team.past_deploy(), "the MCV holds the action");
    for _ in 0..120 {
        if !team.yards().is_empty() && team.past_deploy() {
            break;
        }
        team.tick();
    }
    assert_eq!(team.yards(), [(19, 21)]);
    assert!(team.past_deploy(), "the team moved on");
}

/// A Russian E1 on the yard's origin cell, which only Deploy's (-1, -1)
/// offset reaches, passes action 9's own-cell test, so the MCV is ordered to
/// Unload and Deploy refuses. For an owner no human controls, the refusal
/// clears the site (`Flush_For_Placement`, `0x0073950F..0x00739536`): the E1
/// is told to leave, and the MCV deploys on a later try. A human owner's
/// refusal leaves the E1 where it stands, and the MCV never deploys.
#[test]
fn deploy_refused_for_a_computer_owner_clears_the_site() {
    for human in [false, true] {
        let mut team = DeployTeam::new(&[("AMCV", 20, 22)]);
        let owner = team.sim.interner.intern("Russians");
        team.sim
            .houses
            .get_mut(&owner)
            .unwrap()
            .set_is_human_for_test(human);
        let e1 = team
            .sim
            .spawn_object_at_height("E1", "Russians", 19, 21, 0, 0, &team.rules)
            .expect("E1 spawns");
        team.deploy_action();
        assert!(team.unloading(team.members[0]), "human {human}");
        let mut told_to_leave = false;
        for _ in 0..300 {
            team.tick();
            told_to_leave |= team.sim.substrate.entities.get(e1).is_some_and(|entity| {
                entity.navigation.nav_com().is_some()
                    || crate::sim::movement::motion_query::is_moving(&entity) == Some(true)
            });
            if !team.yards().is_empty() {
                break;
            }
        }
        assert_eq!(told_to_leave, !human, "human {human}");
        let yards = if human { vec![] } else { vec![(19, 21)] };
        assert_eq!(team.yards(), yards, "human {human}");
    }
}
