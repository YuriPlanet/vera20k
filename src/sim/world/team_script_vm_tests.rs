use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::rules::team_ai_ini::{TeamAiDefinitionSource, TeamAiIniRegistry};
use crate::sim::house_state::HouseState;
use crate::sim::snapshot::GameSnapshot;
use crate::sim::team_script_vm::{TeamAiInstallDiagnostic, TeamScriptDefinition};
use crate::util::native_x87::NativeF64Bits;

use super::{FrameStep, Simulation};

fn zero_ai_trigger_comparison() -> String {
    "00".repeat(32)
}

/// A computer house `Computer` with two E1 at cells (10,10) and (12,10) on
/// flat ground, and a new team of TeamType `TT`: TaskForce `F` (2 E1),
/// script `S` (guard 15 frames, then success), AI trigger `A` (weight 40
/// within 10..60, success delta 5).
fn recruiting_team_fixture() -> (Simulation, RuleSet, u64, [u64; 2]) {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[General]\nAITriggerSuccessWeightDelta=5\n\
         [InfantryTypes]\n0=E1\n[E1]\nStrength=100\n",
    ))
    .expect("minimal rules");
    let comparison = zero_ai_trigger_comparison();
    let aimd = IniFile::from_str(&format!(
        "[TeamTypes]\n0=TT\n[TT]\nScript=S\nTaskForce=F\n\
         [ScriptTypes]\n0=S\n[S]\n0=5,1\n1=49,0\n\
         [TaskForces]\n0=F\n[F]\n0=2,E1\n\
         [AITriggerTypes]\nA=Trigger,TT,<all>,2,4,<none>,{comparison},40,10,60,1,0,1,0,<none>,1,1,1\n"
    ));
    let registry = TeamAiIniRegistry::from_sources(&aimd, &IniFile::from_str(""), true);
    let mut sim = Simulation::with_seed(0xA11CE);
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.install_team_ai_registry(&registry, &rules)
        .expect("clean fixed AIMD installs");
    let owner = sim.interner.intern("Computer");
    sim.houses
        .register(HouseState::new(owner, 0, None, false, 0, 10));
    let members = [(10, 10), (12, 10)].map(|(rx, ry)| {
        sim.spawn_object_at_height("E1", "Computer", rx, ry, 0, 0, &rules)
            .expect("E1 spawns")
    });
    let team_type = sim.interner.get("TT").expect("TT interned");
    let team = sim
        .create_team(team_type, Some(owner), &rules)
        .expect("no Max= limit");
    (sim, rules, team, members)
}

fn trigger_weight(sim: &Simulation) -> NativeF64Bits {
    let (_, record) = sim
        .team_script_vm
        .ai_triggers_in_order()
        .next()
        .expect("trigger A");
    record.weight()
}

#[test]
fn a_computer_team_recruits_forms_and_succeeds_in_the_master_frame() {
    let (mut sim, rules, team_id, members) = recruiting_team_fixture();

    sim.advance_tick(&[], Some(&rules), None, None, 67);
    let order = [
        FrameStep::SessionCommands,
        FrameStep::Triggers,
        FrameStep::TeamScript,
        FrameStep::DiskLasers,
        FrameStep::LightningStorm,
        FrameStep::LogicVector,
    ];
    let trace = sim.take_frame_step_trace();
    let visited: Vec<_> = trace
        .into_iter()
        .filter(|step| order.contains(step))
        .collect();
    assert_eq!(
        visited, order,
        "native TeamClass AI (0x0055B502) precedes the Disk walk (0x0055B5A1), \
         LightningStorm::Process (0x0055B5C8) and every live LogicClass object visit"
    );

    let mut formed_with = None;
    for _ in 0..40 {
        let Some(team) = sim.team_script_vm.team(team_id) else {
            break;
        };
        if team.formed() && formed_with.is_none() {
            formed_with = Some(team.members().collect::<Vec<_>>());
        }
        sim.advance_tick(&[], Some(&rules), None, None, 67);
    }
    let mut recruited = formed_with.expect("the team formed");
    recruited.sort_unstable();
    assert_eq!(recruited, members, "both E1 were recruited");
    assert!(
        sim.team_script_vm.team(team_id).is_none(),
        "the script's end destroys the team"
    );
    for member in members {
        assert_eq!(sim.team_script_vm.team_for_member(member), None);
    }
    assert_eq!(
        trigger_weight(&sim),
        NativeF64Bits::from_bits(45.0f64.to_bits()),
        "its destruction after action 49 counts as the trigger's success"
    );
}

#[test]
fn a_recruiting_team_survives_save_load() {
    let (mut original, rules, team_id, _) = recruiting_team_fixture();
    // The loaded copy hashes every frame; the control compares each one.
    original.hash_state_every(std::num::NonZeroU64::MIN);
    original.advance_tick(&[], Some(&rules), None, None, 67);
    assert_eq!(
        original
            .team_script_vm
            .team(team_id)
            .unwrap()
            .member_count(),
        1,
        "one recruit per short entry per update"
    );

    let bytes = GameSnapshot::save_validated(&original, 0, 0, "team_vm_test", 0);
    let mut restored = GameSnapshot::load_validated(&bytes, 0, 0, original.session.map_name())
        .expect("snapshot")
        .sim;
    restored
        .restore_after_snapshot_load()
        .expect("references resolve");
    // A save carries no map: the load re-installs the scenario's cells.
    crate::sim::arena_fixture::flat_ground(&mut restored, &rules);
    // Retail load reinitializes Scenario RNG and discards active planning
    // (ClearScene6370B0 / TechnoLoad70BF79); align the control before comparing.
    original.scenario_rng = crate::sim::rng::SimRng::new(0);
    original.reset_planning_after_player_load();
    assert_eq!(original.state_hash(), restored.state_hash());

    for _ in 0..40 {
        let expected = original.advance_tick(&[], Some(&rules), None, None, 67);
        let actual = restored.advance_tick(&[], Some(&rules), None, None, 67);
        assert_eq!(expected.state_hash, actual.state_hash);
    }
    assert!(restored.team_script_vm.team(team_id).is_none());
    assert_eq!(trigger_weight(&original), trigger_weight(&restored));
}

/// `TeamClass::Recruit @ 0x006EAA90` takes only its team's house's objects
/// (`Can_Add @ 0x006EA610` refuses another house's): an infantryman of
/// another house nearer the team's origin is passed over, one handed to the
/// team's house is recruited first, and a member handed away is not.
#[test]
fn recruitment_takes_the_house_s_objects_through_ownership_changes() {
    let (mut sim, rules, team_id, members) = recruiting_team_fixture();
    let owner = sim.interner.get("Computer").expect("Computer interned");
    let rival = sim.interner.intern("Rival");
    sim.houses
        .register(HouseState::new(rival, 1, None, false, 0, 10));
    // The team has no centre yet, so it recruits nearest the origin.
    let [_, handed_over] = [(2, 2), (4, 4)].map(|(rx, ry)| {
        sim.spawn_object_at_height("E1", "Rival", rx, ry, 0, 0, &rules)
            .expect("E1 spawns")
    });
    sim.change_owner(handed_over, owner);
    sim.change_owner(members[0], rival);

    sim.advance_tick(&[], Some(&rules), None, None, 67);
    let team = sim.team_script_vm.team(team_id).expect("the team recruits");
    assert_eq!(team.members().collect::<Vec<_>>(), [handed_over]);
    sim.advance_tick(&[], Some(&rules), None, None, 67);
    let mut recruited: Vec<u64> = sim
        .team_script_vm
        .team(team_id)
        .expect("the team is full")
        .members()
        .collect();
    recruited.sort_unstable();
    assert_eq!(recruited, [members[1], handed_over]);
}

#[test]
fn team_member_order_is_hashed() {
    let mut forward = Simulation::with_seed(7);
    let mut reverse = Simulation::with_seed(7);
    for (sim, members) in [
        (&mut forward, vec![19, 7, 11]),
        (&mut reverse, vec![11, 7, 19]),
    ] {
        let owner = sim.interner.intern("Americans");
        let script = sim.interner.intern("TEAM_OPENING");
        sim.team_script_vm.register_script(TeamScriptDefinition {
            id: script,
            source: TeamAiDefinitionSource::FixedAimd,
            actions: Vec::new(),
        });
        sim.team_script_vm.create_team(owner, script, members, 0);
    }

    assert_ne!(forward.state_hash(), reverse.state_hash());
}

#[test]
fn production_install_resolves_both_ai_team_buffers_in_fixed_and_map_passes() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=E1\n[E1]\nStrength=100\n",
    ))
    .expect("minimal rules");
    let comparison = zero_ai_trigger_comparison();
    let primary = "PRIMARY_123456789012345";
    let secondary = "SECOND__123456789012345";
    let fixed = IniFile::from_str(&format!(
        "[TeamTypes]\n0={primary}\n1={secondary}\n\
         [{primary}]\nScript=S\nTaskForce=F\n\
         [{secondary}]\nScript=S\nTaskForce=F\n\
         [ScriptTypes]\n0=S\n[S]\n0=2,0\n\
         [TaskForces]\n0=F\n[F]\n0=1,E1\n\
         [AITriggerTypes]\nA=Fixed,{primary}suffix,<all>,2,4,<none>,{comparison},40,10,40,1,0,1,0,{secondary}suffix,1,1,1\n"
    ));
    let scenario = IniFile::from_str(&format!(
        "[AITriggerTypes]\nB=Map,{secondary}suffix,<all>,2,4,<none>,{comparison},40,10,40,1,0,1,0,{primary}suffix,1,1,1\n"
    ));
    let registry = TeamAiIniRegistry::from_sources(&fixed, &scenario, true);
    let mut sim = Simulation::new();
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    assert!(
        sim.install_team_ai_registry(&registry, &rules)
            .expect("native team buffers resolve before fixed-source validation")
            .is_empty()
    );

    let triggers: Vec<_> = sim.team_script_vm.ai_triggers_in_order().collect();
    assert_eq!(triggers.len(), 2);
    let fixed = triggers[0].0;
    let map = triggers[1].0;
    assert_eq!(fixed.primary_team_type, sim.interner.get(primary));
    assert_eq!(fixed.secondary_team_type, sim.interner.get(secondary));
    assert_eq!(map.primary_team_type, sim.interner.get(secondary));
    assert_eq!(map.secondary_team_type, sim.interner.get(primary));
}

/// FindByIDOrName6F0FC0 compares each registered TeamType's ID and then Name.
/// An earlier Name therefore wins over a later ID in ordinary House AI teams.
#[test]
fn team_lookup_ordinary_ai_recruits_and_moves_first_name() {
    use crate::map::map_file::MapFile;
    use crate::rules::process_owner::NativeScenarioRulesPrefix;
    use crate::sim::components::NavTargetRef;
    use crate::sim::scenario_bootstrap::{
        MatchLaunchDescriptor, ScenarioBootstrapRng, prepare_stock_offline_scenario_prefix_plan,
    };
    use crate::sim::scenario_session::ScenarioDescriptor;

    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let Some(fixed) = crate::rules::retail_ini_fixture::retail_ini("aimd.ini") else {
        return;
    };
    // The no-collision scene first proves the natural activation/production
    // path. The second scene changes only FIRST.Name, keeping both complete
    // TeamTypes and their distinct TaskForces eligible.
    for (first_name, expected_team, expected_member) in
        [("FIRST", "SECOND", "FLAKT"), ("SECOND", "FIRST", "E2")]
    {
        let map = MapFile::from_ini(IniFile::from_str(&format!(
            "[Basic]\nIgnoreGlobalAITriggers=yes\n\
             [Map]\nTheater=TEMPERATE\nSize=0,0,32,32\nLocalSize=0,0,32,32\n\
             [IsoMapPack5]\n1=CAAEABUAAAAAEQAA\n\
             [General]\nTeamDelays=30,30,30\n\
             [Houses]\n0=Computer\n\
             [Computer]\nCountry=Russians\nIQ=5\nCredits=0\nTechLevel=10\nPlayerControl=no\n\
             [Waypoints]\n0=32032\n9=24008\n\
             [Units]\n0=Computer,SMCV,256,20,20,0,Guard,None,0,-1,0,-1,1,1\n\
             [Structures]\n0=Computer,NAHAND,256,10,6,0,None,0,0,1,0,0,None,None,None,0,0\n\
             1=Computer,NAPOWR,256,16,6,0,None,0,0,1,0,0,None,None,None,0,0\n\
             [Infantry]\n0=Computer,E2,256,12,12,0,Guard,0,None,0,-1,0,1,1\n\
             1=Computer,FLAKT,256,14,12,0,Guard,0,None,0,-1,0,1,1\n\
             [TeamTypes]\n0=FIRST\n1=SECOND\n\
             [FIRST]\nName={first_name}\nHouse=Russians\nScript=LOOKUP_MOVE\n\
             TaskForce=LOOKUP_E2\nPriority=7\nMax=1\nIsBaseDefense=yes\n\
             [SECOND]\nName=SECOND\nHouse=Russians\nScript=LOOKUP_MOVE\n\
             TaskForce=LOOKUP_FLAK\nPriority=7\nMax=1\nIsBaseDefense=yes\n\
             [TaskForces]\n0=LOOKUP_E2\n1=LOOKUP_FLAK\n\
             [LOOKUP_E2]\nGroup=-1\n0=1,E2\n[LOOKUP_FLAK]\nGroup=-1\n0=1,FLAKT\n\
             [ScriptTypes]\n0=LOOKUP_MOVE\n[LOOKUP_MOVE]\n0=3,9\n1=5,9999\n\
             [AITriggerTypes]\nLOOKUP_AI=Lookup movement,SECOND,<all>,0,-1,<none>,{},5000,5000,5000,1,0,2,0,<none>,1,1,1\n\
             [AITriggerTypesEnable]\nLOOKUP_AI=yes\n",
            zero_ai_trigger_comparison()
        )))
        .expect("authored map");
        let launch = MatchLaunchDescriptor::from_resolved(
            crate::headless_scenario::battle_session("team-lookup.map", &[]),
        )
        .unwrap();
        let prefix =
            prepare_stock_offline_scenario_prefix_plan(&launch, &map, &map.waypoints, 0x6F0FC0)
                .unwrap();
        let mode =
            crate::rules::retail_sources::select_mode_ini(&assets, Some("MPBattleMD.ini")).unwrap();
        let (mut rules, _, art, receipt) =
            crate::rules::retail_ini_fixture::retail_rules_owner(&assets)
                .load_scenario(
                    NativeScenarioRulesPrefix::NonCampaign(mode.as_ref()),
                    &map.ini,
                )
                .unwrap()
                .into_parts();
        let bound_prefix = prefix
            .bind_native_rules_receipt(receipt, &launch, &rules)
            .unwrap();
        rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
        rules.bind_animation_sequences(
            &crate::rules::infantry_sequence::parse_infantry_sequence_registry(&art),
        );
        let registry = TeamAiIniRegistry::from_sources(&fixed, &map.ini, true);
        // Use the production prefix admission for both RNG and native IDs;
        // the controlled terrain and named House are installed below.
        let (mut sim, _) = ScenarioBootstrapRng::new(0x6F0FC0)
            .into_stock_offline_staged_simulation(
                &ScenarioDescriptor {
                    seed: 0x6F0FC0,
                    game_mode_nonzero: true,
                    ignore_global_ai_triggers: map.basic.ignore_global_ai_triggers.unwrap_or(false),
                    ..Default::default()
                },
                bound_prefix,
            )
            .unwrap();
        crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let roster =
            crate::map::houses::parse_house_roster(&map.ini, &rules.color_schemes, Some(&rules));
        crate::sim::scenario_bootstrap::initialize_map_roster_houses(
            &mut sim,
            &roster,
            &rules,
            Some(&map.ini),
        );
        sim.session.install_waypoints(&map.waypoints);
        sim.initialize_trigger_definitions(&map, &registry, None, None, None);
        sim.install_team_ai_registry(&registry, &rules).unwrap();
        assert_eq!(sim.spawn_from_map(&map.entities, Some(&rules)), 5);
        let owner = sim.interner.get("Computer").unwrap();
        let script = sim.interner.get("LOOKUP_MOVE").unwrap();
        let expected_type = sim.interner.get(expected_team).unwrap();
        let member = sim
            .entities()
            .values()
            .find(|entity| sim.interner.resolve(entity.type_ref()) == expected_member)
            .unwrap()
            .stable_id();
        let mcv = sim
            .entities()
            .values()
            .find(|entity| sim.interner.resolve(entity.type_ref()) == "SMCV")
            .unwrap()
            .stable_id();
        assert_eq!(sim.houses[&owner].side_index(), 1);
        assert!(!sim.houses[&owner].ai_activation().ai_triggers_active);
        assert_eq!(sim.team_script_vm.teams_in_order().count(), 0);
        let limit = sim.houses[&owner].team_creation().timer().duration() + 600;
        let (mut sent_from, mut moved) = (None, false);
        for _ in 0..limit {
            let grid = sim.path_grid_snapshot();
            sim.advance_tick(
                &[],
                Some(&rules),
                grid.as_deref(),
                None,
                1000 / crate::util::fixed_math::SIM_TICK_HZ,
            );
            let Some(team) = sim
                .team_script_vm
                .teams_in_order()
                .find(|team| team.script_id() == script)
            else {
                continue;
            };
            assert_eq!(
                team.team_type_id(),
                Some(expected_type),
                "FIRST.Name={first_name}"
            );
            assert_eq!(team.owner(), owner);
            if !team.formed() || team.cursor() != 0 {
                continue;
            }
            assert_eq!(team.members().collect::<Vec<_>>(), [member]);
            assert_eq!(
                sim.team_script_vm.team_for_member(member).map(|(id, _)| id),
                Some(team.id())
            );
            let actor = sim.entities().get(member).unwrap();
            let cell = (actor.position.rx(), actor.position.ry());
            if let Some(origin) = sent_from {
                moved |= cell != origin;
            } else if actor.navigation.nav_com() == Some(NavTargetRef::Cell { rx: 8, ry: 24 }) {
                // The MCV also retasks ground forces. Tie this observation to
                // the formed Team's Script3 and its separate waypoint first.
                sent_from = Some(cell);
            }
            if moved {
                break;
            }
        }
        assert!(!sim.entities().contains(mcv), "ordinary AI deploys its MCV");
        assert!(sim.entities().values().any(|entity| {
            entity.owner() == owner && sim.interner.resolve(entity.type_ref()) == "NACNST"
        }));
        assert!(sim.houses[&owner].ai_activation().ai_triggers_active);
        assert!(
            sent_from.is_some(),
            "{expected_member} receives Script3's waypoint"
        );
        assert!(
            moved,
            "{expected_member} then moves through the ordinary frame"
        );
    }
}

#[test]
fn production_install_refuses_fixed_resolution_loss_but_keeps_scenario_omissions() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=E1\n[E1]\nStrength=100\n",
    ))
    .expect("minimal rules");
    let comparison = zero_ai_trigger_comparison();
    let fixed_with_unknown = IniFile::from_str(&format!(
        "[TeamTypes]\n0=TT\n[TT]\nScript=S\nTaskForce=F\n\
         [ScriptTypes]\n0=S\n[S]\n0=2,0\n\
         [TaskForces]\n0=F\n[F]\n0=1,GHOST\n\
         [AITriggerTypes]\nA=Trigger,TT,<all>,2,4,<none>,{comparison},40,10,40,1,0,1,0,<none>,1,1,1\n"
    ));
    let fixed_registry =
        TeamAiIniRegistry::from_sources(&fixed_with_unknown, &IniFile::from_str(""), true);
    assert!(fixed_registry.fixed_source_is_complete());

    let mut fixed_sim = Simulation::new();
    fixed_sim.intern_rule_type_ids(&rules);
    fixed_sim.resolve_type_handles(&rules);
    let fixed_diagnostics = fixed_sim
        .install_team_ai_registry(&fixed_registry, &rules)
        .expect_err("a fixed-origin resolution loss refuses the install");

    assert_eq!(
        fixed_diagnostics,
        vec![TeamAiInstallDiagnostic::UnknownTaskForceMember {
            task_force_id: "F".to_string(),
            member_type: "GHOST".to_string(),
            source: TeamAiDefinitionSource::FixedAimd,
        }]
    );
    assert!(fixed_diagnostics[0].is_fixed_source_refusal());
    assert_eq!(
        fixed_sim.team_script_vm.registry_counts(),
        (0, 0, 0, 0),
        "a fixed-origin resolution refusal must not install a partial registry"
    );

    let clean_fixed = IniFile::from_str(&format!(
        "[TeamTypes]\n0=TT\n[TT]\nScript=S\nTaskForce=F\n\
         [ScriptTypes]\n0=S\n[S]\n0=2,0\n\
         [TaskForces]\n0=F\n[F]\n0=1,E1\n\
         [AITriggerTypes]\nA=Trigger,TT,<all>,2,4,<none>,{comparison},40,10,40,1,0,1,0,<none>,1,1,1\n"
    ));
    let scenario = IniFile::from_str("[TaskForces]\n0=MAP_F\n[MAP_F]\n0=1,GHOST\n");
    let scenario_registry = TeamAiIniRegistry::from_sources(&clean_fixed, &scenario, true);
    assert!(scenario_registry.fixed_source_is_complete());

    let mut scenario_sim = Simulation::new();
    scenario_sim.intern_rule_type_ids(&rules);
    scenario_sim.resolve_type_handles(&rules);
    let scenario_diagnostics = scenario_sim
        .install_team_ai_registry(&scenario_registry, &rules)
        .expect("scenario-origin omissions still install");

    assert_eq!(
        scenario_diagnostics,
        vec![TeamAiInstallDiagnostic::UnknownTaskForceMember {
            task_force_id: "MAP_F".to_string(),
            member_type: "GHOST".to_string(),
            source: TeamAiDefinitionSource::Scenario,
        }]
    );
    assert!(!scenario_diagnostics[0].is_fixed_source_refusal());
    assert_eq!(
        scenario_sim.team_script_vm.registry_counts(),
        (2, 1, 1, 1),
        "scenario-origin omissions remain diagnosed, nonfatal overlays"
    );
}

#[test]
fn production_install_refuses_fixed_resolution_loss_masked_by_same_identity_map_overlays() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=E1\n[E1]\nStrength=100\n",
    ))
    .expect("minimal rules");
    let comparison = zero_ai_trigger_comparison();
    let fixed = IniFile::from_str(&format!(
        "[TeamTypes]\n0=TT\n[TT]\nScript=MISSING_SCRIPT\nTaskForce=F\nPriority=5\n\
         [ScriptTypes]\n0=S\n[S]\n0=2,0\n\
         [TaskForces]\n0=F\n[F]\n0=1,GHOST\n\
         [AITriggerTypes]\nA=Fixed bad,TT,<all>,2,4,GHOST,{comparison},40,10,40,1,0,1,0,<none>,1,1,1\n"
    ));
    let scenario = IniFile::from_str(&format!(
        "[TeamTypes]\n0=TT\n[TT]\nPriority=20\n\
         [TaskForces]\n0=F\n[F]\n0=1,E1\n\
         [AITriggerTypes]\nA=Map repair,TT,<all>,2,4,E1,{comparison},40,10,40,1,0,1,0,<none>,1,1,1\n"
    ));
    let registry = TeamAiIniRegistry::from_sources(&fixed, &scenario, true);
    assert!(registry.fixed_source_is_complete());
    let mut sim = Simulation::new();
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);

    let diagnostics = sim
        .install_team_ai_registry(&registry, &rules)
        .expect_err("a fixed-origin resolution loss refuses the install");

    assert_eq!(
        diagnostics,
        vec![
            TeamAiInstallDiagnostic::UnknownTaskForceMember {
                task_force_id: "F".to_string(),
                member_type: "GHOST".to_string(),
                source: TeamAiDefinitionSource::FixedAimd,
            },
            TeamAiInstallDiagnostic::UnknownAiTriggerObject {
                trigger_id: "A".to_string(),
                object_type: "GHOST".to_string(),
                source: TeamAiDefinitionSource::FixedAimd,
            },
        ]
    );
    assert!(
        diagnostics
            .iter()
            .all(TeamAiInstallDiagnostic::is_fixed_source_refusal)
    );
    assert_eq!(
        sim.team_script_vm.registry_counts(),
        (0, 0, 0, 0),
        "map repair/relabeling cannot erase fixed-AIMD resolution obligations"
    );
}

/// A computer house `Computer`, whose current enemy is `Human`, with two
/// `member`s at cells (10,10) and (12,10) on flat ground and a new team of
/// TeamType `TT` (TaskForce 2 `member`) running `script`; `Human` owns the
/// two `buildings`. The houses' base centres are (10,11) and (18,11).
fn team_fixture(
    rules: &str,
    member: &str,
    buildings: [(&str, u16, u16); 2],
    script: &str,
) -> (Simulation, RuleSet, u64, [u64; 2], [u64; 2]) {
    let rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(rules))
        .expect("minimal rules");
    let comparison = zero_ai_trigger_comparison();
    let aimd = IniFile::from_str(&format!(
        "[TeamTypes]\n0=TT\n[TT]\nScript=S\nTaskForce=F\n\
         [ScriptTypes]\n0=S\n[S]\n{script}\n\
         [TaskForces]\n0=F\n[F]\n0=2,{member}\n\
         [AITriggerTypes]\nA=Trigger,TT,<all>,2,4,<none>,{comparison},40,10,60,1,0,1,0,<none>,1,1,1\n"
    ));
    let registry = TeamAiIniRegistry::from_sources(&aimd, &IniFile::from_str(""), true);
    let mut sim = Simulation::with_seed(0xA77AC);
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.install_team_ai_registry(&registry, &rules)
        .expect("clean fixed AIMD installs");
    let owner = sim.interner.intern("Computer");
    let enemy = sim.interner.intern("Human");
    let mut computer = HouseState::new(owner, 0, None, false, 0, 10);
    computer.set_enemy_house_for_test(Some(enemy));
    computer.set_base_center_for_test(Some((10, 11)));
    sim.houses.register(computer);
    let mut human = HouseState::new(enemy, 1, None, true, 0, 10);
    human.set_base_center_for_test(Some((18, 11)));
    sim.houses.register(human);
    let members = [(10, 10), (12, 10)].map(|(rx, ry)| {
        sim.spawn_object_at_height(member, "Computer", rx, ry, 0, 0, &rules)
            .expect("member spawns")
    });
    let buildings = buildings.map(|(building, rx, ry)| {
        sim.spawn_object_at_height(building, "Human", rx, ry, 0, 0, &rules)
            .expect("building spawns")
    });
    let team_type = sim.interner.get("TT").expect("TT interned");
    let team = sim
        .create_team(team_type, Some(owner), &rules)
        .expect("no Max= limit");
    (sim, rules, team, members, buildings)
}

/// [`team_fixture`] with armed E1 members; `Human` owns the power plants
/// `PLANTA` (`Power=100`) at (18,8) and `PLANTB` (`Power=200`) at (18,13).
fn attack_team_fixture(script: &str) -> (Simulation, RuleSet, u64, [u64; 2], [u64; 2]) {
    team_fixture(
        "[General]\nAISafeDistance=4\n\
         [InfantryTypes]\n0=E1\n[E1]\nStrength=100\nPrimary=M60\nSpeed=4\n\
         Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}\n\
         [BuildingTypes]\n0=PLANTA\n1=PLANTB\n\
         [PLANTA]\nStrength=750\nPower=100\nFoundation=2x2\n\
         [PLANTB]\nStrength=750\nPower=200\nFoundation=2x2\n\
         [M60]\nDamage=15\nROF=20\nRange=4\nWarhead=SA\n[SA]\nVerses=100%\n",
        "E1",
        [("PLANTA", 18, 8), ("PLANTB", 18, 13)],
        script,
    )
}

/// Script action 0 with quarry 9 (power plants, mask `0x800`): the leader's
/// scan scores each plant by `Power=` × 1000, so the team goes for the
/// bigger one, and `Coordinate_Attack` sets each joined member on it.
#[test]
fn a_computer_team_attacks_the_bigger_power_plant() {
    let (mut sim, rules, team_id, members, [_, bigger]) = attack_team_fixture("0=0,9");
    for _ in 0..40 {
        sim.advance_tick(&[], Some(&rules), None, None, 67);
        if members.iter().all(|&member| {
            sim.entities()
                .get(member)
                .and_then(|entity| entity.attack_target.target())
                .is_some()
        }) {
            break;
        }
    }
    let team = sim.team_script_vm.team(team_id).expect("the team attacks");
    assert!(team.formed());
    for member in members {
        let entity = sim.entities().get(member).expect("member");
        assert_eq!(
            entity.attack_target.target(),
            Some(crate::sim::combat::TargetKind::Entity(bigger)),
            "member {member} attacks the 200-power plant"
        );
        let attack =
            crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Attack);
        assert!(
            entity.mission.effective() == attack || entity.mission.queued() == attack,
            "member {member} is on Attack"
        );
    }
}

/// Script action 53 with aircraft members: the team's `Set_Destination`
/// (vt+0x480) is `AircraftClass::Assign_Destination @ 0x0041AA80`, so the
/// aircraft fly to the gathering cell and the script goes on (#1111).
#[test]
fn a_computer_aircraft_team_flies_to_its_gathering_cell() {
    let (mut sim, rules, team_id, members, _) = team_fixture(
        "[General]\nAISafeDistance=4\n\
         [AircraftTypes]\n0=ORCA\n[ORCA]\nStrength=150\nSpeed=14\nAmmo=1\nPrimary=Gun\n\
         Landable=yes\nMovementZone=Fly\nSpeedType=Winged\n\
         Locomotor={4A582746-9839-11D1-B709-00A024DDAFD1}\n\
         [BuildingTypes]\n0=PLANTA\n1=PLANTB\n\
         [PLANTA]\nStrength=750\nPower=100\nFoundation=2x2\n\
         [PLANTB]\nStrength=750\nPower=200\nFoundation=2x2\n\
         [Gun]\nDamage=15\nROF=20\nRange=4\nWarhead=SA\n[SA]\nVerses=100%\n",
        "ORCA",
        [("PLANTA", 18, 8), ("PLANTB", 18, 13)],
        "0=53,0\n1=49,0",
    );
    // 4 cells from the enemy's centre (18,11) towards the house's (10,11).
    let seed = (14, 11);
    let near_seed = |rx: u16, ry: u16| {
        (i32::from(rx) - seed.0).abs() <= 2 && (i32::from(ry) - seed.1).abs() <= 2
    };
    let mut sent = false;
    let mut arrived = [false; 2];
    for _ in 0..900 {
        sim.advance_tick(&[], Some(&rules), None, None, 67);
        for (n, &member) in members.iter().enumerate() {
            let Some(entity) = sim.entities().get(member) else {
                continue;
            };
            sent |= matches!(
                entity.navigation.nav_com(),
                Some(crate::sim::components::NavTargetRef::Cell { rx, ry }) if near_seed(rx, ry)
            );
            arrived[n] |= near_seed(entity.position.rx(), entity.position.ry());
        }
        if sim.team_script_vm.team(team_id).is_none() {
            break;
        }
    }
    assert!(sent, "a member was sent near {seed:?}");
    assert_eq!(arrived, [true; 2], "both aircraft flew there");
    assert!(
        sim.team_script_vm.team(team_id).is_none(),
        "the team arrived and its script ran to the end"
    );
}

/// `Evaluate_Candidate`'s building gate (`0x006F85AB..0x006F8601`) holds
/// only for a human scanner outside a team: a computer E1 on Guard takes
/// the enemy's unarmed power plant, a human E1 beside the computer's own
/// plant passes it over.
#[test]
fn only_a_human_scanner_passes_over_an_unarmed_building() {
    let (mut sim, rules, _, _, [plant, _]) = attack_team_fixture("0=49,0");
    let computer = sim
        .spawn_object_at_height("E1", "Computer", 17, 7, 0, 0, &rules)
        .expect("E1 spawns");
    sim.spawn_object_at_height("PLANTA", "Computer", 6, 16, 0, 0, &rules)
        .expect("plant spawns");
    let human = sim
        .spawn_object_at_height("E1", "Human", 5, 15, 0, 0, &rules)
        .expect("E1 spawns");
    let mut scan = |id| {
        crate::sim::world::direct_greatest_threat(
            &mut sim,
            &rules,
            None,
            id,
            crate::sim::combat::ScanMission::Guard,
        )
    };
    assert_eq!(scan(computer), Some(plant));
    assert_eq!(scan(human), None);
}

/// Retail "Yuri Engineers" (`08B95EFC-G`) in miniature: computer engineers
/// armed as retail ones are (`DefuseKit`, `VirtualScanner`) run quarry 6
/// (factories, mask `0x1000`, which the Infantry override widens with the
/// engineer's `0x200`, `Capturable=`) and attack the enemy's barracks, not
/// its power plant. Whether an engineer then captures it is not native
/// behaviour this test claims: `DefuseKit` is ILLEGAL against a building
/// without a bomb (`0x006FCAFA`), and `InfantryClass::Mission_Attack @
/// 0x0051F3E0` turns only `Infiltrate=`, `Occupier=` and `Assaulter=`
/// infantry into Capture.
#[test]
fn a_computer_engineer_team_attacks_the_enemy_factory() {
    let (mut sim, rules, _, members, [barracks, _]) = team_fixture(
        "[General]\nAISafeDistance=4\n\
         [InfantryTypes]\n0=ENGI\n\
         [ENGI]\nStrength=75\nPrimary=DefuseKit\nSecondary=VirtualScanner\nEngineer=yes\n\
         Speed=4\nMovementZone=Infantry\nLocomotor={4A582744-9839-11D1-B709-00A024DDAFD1}\n\
         [BuildingTypes]\n0=BARR\n1=PLANT\n\
         [BARR]\nStrength=500\nCapturable=true\nFactory=InfantryType\nFoundation=2x2\n\
         [PLANT]\nStrength=750\nPower=100\nCapturable=true\nFoundation=2x2\n\
         [DefuseKit]\nDamage=1\nROF=20\nRange=1.5\nCellRangefinding=yes\n\
         Projectile=InvisibleAll\nWarhead=BombDisarm\nFireOnce=yes\n\
         [VirtualScanner]\nDamage=1\nRange=5\nNeverUse=yes\nProjectile=InvisibleAll\nWarhead=SA\n\
         [InvisibleAll]\nInviso=yes\nAA=yes\nAG=yes\n[BombDisarm]\nBombDisarm=yes\n\
         [SA]\nVerses=100%,80%,80%,50%,25%,25%,75%,50%,25%,100%,100%\n",
        "ENGI",
        [("BARR", 18, 8), ("PLANT", 18, 13)],
        "0=0,6",
    );
    let mut targets = Vec::new();
    for _ in 0..60 {
        sim.advance_tick(&[], Some(&rules), None, None, 67);
        for &member in &members {
            if let Some(attack) = sim
                .entities()
                .get(member)
                .and_then(|entity| entity.attack_target.target())
                && !targets.contains(&attack)
            {
                targets.push(attack);
            }
        }
    }
    assert_eq!(
        targets,
        vec![crate::sim::combat::TargetKind::Entity(barracks)],
        "the members attack the barracks"
    );
}

/// [`team_fixture`] with armed `HTNK` vehicles, the retail order of the first
/// five `[SuperWeaponTypes]`, the retail `IronCurtainDuration=750`, and the
/// computer's `IRON` (Iron Curtain) and `CHRONO` (Chronosphere and Chrono
/// Warp) at (4,4) and (4,8), each Super granted and charged; `Human` owns
/// the power plants `PLANTA` (`Power=100`) at (18,8) and `PLANTB`
/// (`Power=200`) at (18,13).
fn super_team_fixture(script: &str) -> (Simulation, RuleSet, u64, [u64; 2], [u64; 2]) {
    let (mut sim, rules, team, members, plants) = team_fixture(
        "[General]\nAISafeDistance=4\n[CombatDamage]\nIronCurtainDuration=750\n\
         [VehicleTypes]\n0=HTNK\n[HTNK]\nStrength=400\nPrimary=M60\nSpeed=6\n\
         Locomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n\
         [BuildingTypes]\n0=PLANTA\n1=PLANTB\n2=IRON\n3=CHRONO\n\
         [PLANTA]\nStrength=750\nPower=100\n[PLANTB]\nStrength=750\nPower=200\n\
         [IRON]\nStrength=750\nSuperWeapon=IronCurtainSpecial\n\
         [CHRONO]\nStrength=750\nSuperWeapon=ChronoSphereSpecial\n\
         [M60]\nDamage=15\nROF=20\nRange=4\nWarhead=SA\n[SA]\nVerses=100%\n\
         [SuperWeaponTypes]\n0=NukeSpecial\n1=IronCurtainSpecial\n2=LightningStormSpecial\n\
         3=ChronoSphereSpecial\n4=ChronoWarpSpecial\n\
         [NukeSpecial]\nType=MultiMissile\n[IronCurtainSpecial]\nType=IronCurtain\nRechargeTime=5\n\
         [LightningStormSpecial]\nType=LightningStorm\n\
         [ChronoSphereSpecial]\nType=ChronoSphere\nPreClick=yes\nRechargeTime=7\n\
         [ChronoWarpSpecial]\nType=ChronoWarp\nPostClick=yes\nPreDependent=ChronoSphere\n\
         RechargeTime=1\n",
        "HTNK",
        [("PLANTA", 18, 8), ("PLANTB", 18, 13)],
        script,
    );
    // House construction retains every stock entry, including the ungranted
    // ChronoWarp that Fire_SW reaches via PostClick. Clicking never constructs.
    for owner in sim.houses.order().to_vec() {
        for name in &rules.super_weapon_order {
            let id = sim.interner.intern(name);
            sim.super_weapons
                .map_mut_for_test()
                .entry(owner)
                .or_default()
                .insert(
                    id,
                    crate::sim::superweapon::SuperWeaponInstance::new(
                        id,
                        owner,
                        sim.session.binary_frame() as i32,
                    ),
                );
        }
    }
    for (building, rx, ry) in [("IRON", 4, 4), ("CHRONO", 4, 8)] {
        sim.spawn_object_at_height(building, "Computer", rx, ry, 0, 0, &rules)
            .expect("building spawns");
    }
    // As retail GACSPH, CHRONO grants no Chrono Warp: Fire_SW reaches that
    // Super ungranted.
    let owner = sim.interner.intern("Computer");
    for name in ["IronCurtainSpecial", "ChronoSphereSpecial"] {
        let id = sim.interner.intern(name);
        let mut instance = crate::sim::superweapon::SuperWeaponInstance::new(id, owner, 0);
        instance.set_is_active_for_test(true);
        instance.set_charge_start_tick_for_test(sim.session.binary_frame() as i32);
        instance
            .set_charge_duration_for_test(rules.super_weapon(name).unwrap().recharge_time_frames);
        instance.set_is_ready_for_test(true);
        sim.super_weapons
            .map_mut_for_test()
            .entry(owner)
            .or_default()
            .insert(id, instance);
    }
    (sim, rules, team, members, plants)
}

/// Each Fire_SW the frames make while the team runs (at most 40 frames, until
/// its script ends), as (`[SuperWeaponTypes]` name, cell).
fn super_fires(sim: &mut Simulation, rules: &RuleSet, team_id: u64) -> Vec<(String, (u16, u16))> {
    use crate::sim::superweapon::ai_fire::{AI_FIRE_LOG, AiFireEvent};
    AI_FIRE_LOG.set(Some(Vec::new()));
    for _ in 0..40 {
        sim.advance_tick(&[], Some(rules), None, None, 67);
        if sim.team_script_vm.team(team_id).is_none() {
            break;
        }
    }
    AI_FIRE_LOG
        .take()
        .unwrap()
        .into_iter()
        .filter_map(|event| match event {
            AiFireEvent::Fire(id, cell) => Some((sim.interner.resolve(id).to_string(), cell)),
            _ => None,
        })
        .collect()
}

/// Script action 55 with the Iron Curtain 30 frames short of its charge: the
/// team stays on the action while the Super charges and fires on the next
/// frame after it does, because the teams run before the house update that
/// charges it (`0x0055B502..0x0055B59F`).
#[test]
fn a_computer_team_waits_on_its_charging_iron_curtain() {
    use crate::sim::superweapon::ai_fire::{AI_FIRE_LOG, AiFireEvent};
    let (mut sim, rules, team_id, _, _) = super_team_fixture("0=55,0\n1=49,0");
    let owner = sim.interner.get("Computer").unwrap();
    let curtain = sim.interner.get("IronCurtainSpecial").unwrap();
    let frame = sim.session.binary_frame() as i32;
    let instance = sim
        .super_weapons
        .map_mut_for_test()
        .get_mut(&owner)
        .and_then(|weapons| weapons.get_mut(&curtain))
        .unwrap();
    instance.set_is_ready_for_test(false);
    instance.set_charge_start_tick_for_test(frame);
    instance.set_charge_duration_for_test(30);
    AI_FIRE_LOG.set(Some(Vec::new()));
    let (mut waited, mut charged, mut fired) = (false, None, None);
    for tick in 0..60 {
        sim.advance_tick(&[], Some(&rules), None, None, 67);
        let fire = AI_FIRE_LOG.with_borrow(|log| {
            log.as_ref()
                .unwrap()
                .iter()
                .any(|event| matches!(event, AiFireEvent::Fire(..)))
        });
        if fire {
            fired = Some(tick);
            break;
        }
        let ready = sim.super_weapons[&owner][&curtain].is_ready();
        let team = sim.team_script_vm.team(team_id).expect("the team waits");
        waited |= !ready && team.formed() && team.cursor() == 0 && !team.advance_pending();
        if ready && charged.is_none() {
            charged = Some(tick);
        }
    }
    AI_FIRE_LOG.set(None);
    assert!(
        waited,
        "the team stood on the action while the Super charged"
    );
    let charged = charged.expect("the Super charged without firing that frame");
    assert_eq!(fired, Some(charged + 1));
}

/// Script action 57 with quarry 9 (power plants) then 49: the leader's scan
/// picks the bigger plant, the house fires its Chronosphere at the team's
/// centre (11,10) and its Chrono Warp at the plant's cell, and the warp
/// carries each member from the source block to the same place in the
/// destination block, beside the plant.
#[test]
fn a_computer_team_chronoshifts_onto_the_enemy_power_plant() {
    let (mut sim, rules, team_id, members, [_, bigger]) = super_team_fixture("0=57,9\n1=49,0");
    let fires = super_fires(&mut sim, &rules, team_id);
    let plant = sim.entities().get(bigger).expect("plant");
    let plant_cell = (plant.position.rx(), plant.position.ry());
    assert_eq!(
        fires,
        vec![
            ("ChronoSphereSpecial".to_string(), (11, 10)),
            ("ChronoWarpSpecial".to_string(), plant_cell),
        ]
    );
    assert!(
        sim.team_script_vm.team(team_id).is_none(),
        "the script ran to its end"
    );
    let owner = sim.interner.get("Computer").unwrap();
    let sphere = sim.interner.get("ChronoSphereSpecial").unwrap();
    assert!(
        !sim.super_weapons[&owner][&sphere].is_ready(),
        "it recharges"
    );
    for _ in 0..200 {
        sim.advance_tick(&[], Some(&rules), None, None, 67);
    }
    // (10,10) and (12,10) lie west and east of the source block's centre.
    let expected = [
        (plant_cell.0 - 1, plant_cell.1),
        (plant_cell.0 + 1, plant_cell.1),
    ];
    for (member, cell) in members.into_iter().zip(expected) {
        let entity = sim.entities().get(member).expect("member");
        assert_eq!(
            (entity.position.rx(), entity.position.ry()),
            cell,
            "member {member} arrived beside the plant"
        );
    }
}
