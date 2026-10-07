use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::rules::team_ai_ini::{TeamAiDefinitionSource, TeamAiIniRegistry};
use crate::sim::house_state::HouseState;
use crate::sim::snapshot::GameSnapshot;
use crate::sim::team_script_vm::{TeamAiInstallDiagnostic, TeamScriptDefinition};
use crate::util::native_x87::NativeF64Bits;

use super::{MasterFrameTestRung, Simulation};

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
        .insert(owner, HouseState::new(owner, 0, None, false, 0, 10));
    sim.session.house_order = vec![owner];
    let members = [(10, 10), (12, 10)].map(|(rx, ry)| {
        sim.spawn_object_at_height("E1", "Computer", rx, ry, 0, 0, &rules)
            .expect("E1 spawns")
    });
    let team_type = sim.interner.get("TT").expect("TT interned");
    let team = sim
        .team_script_vm
        .construct_team(team_type, owner, true, sim.session.binary_frame as i32)
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
    let trace = sim.take_master_frame_test_trace();
    assert_eq!(
        &trace[..4],
        &[
            MasterFrameTestRung::SessionCommands,
            MasterFrameTestRung::Triggers,
            MasterFrameTestRung::TeamScript,
            MasterFrameTestRung::LogicVector,
        ],
        "native TeamClass AI must finish before any live LogicClass object visit"
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
    let mut restored = GameSnapshot::load(&bytes).expect("snapshot").sim;
    restored
        .restore_after_snapshot_load()
        .expect("references resolve");
    // A save carries no map: the load re-installs the scenario's cells.
    crate::sim::arena_fixture::flat_ground(&mut restored, &rules);
    // Retail's save reader reinitializes the Scenario RNG; align the control
    // run to that load contract before comparing the continuation.
    original.scenario_rng = crate::sim::rng::SimRng::new(0);
    assert_eq!(original.state_hash(), restored.state_hash());

    for _ in 0..40 {
        let expected = original.advance_tick(&[], Some(&rules), None, None, 67);
        let actual = restored.advance_tick(&[], Some(&rules), None, None, 67);
        assert_eq!(expected.state_hash, actual.state_hash);
    }
    assert!(restored.team_script_vm.team(team_id).is_none());
    assert_eq!(trigger_weight(&original), trigger_weight(&restored));
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
fn production_install_boundary_resolves_aimd_without_creating_a_team() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=E1\n[E1]\nStrength=100\n",
    ))
    .expect("minimal rules");
    let comparison = zero_ai_trigger_comparison();
    let aimd = IniFile::from_str(&format!(
        "[TeamTypes]\n0=TT\n[TT]\nScript=S\nTaskForce=F\n\
         [ScriptTypes]\n0=S\n[S]\n0=2,0\n\
         [TaskForces]\n0=F\n[F]\n0=1,E1\n\
         [AITriggerTypes]\nA=Trigger,TT,<all>,2,4,<none>,{comparison},40,10,40,1,0,1,0,<none>,1,1,1\n"
    ));
    let registry = TeamAiIniRegistry::from_sources(&aimd, &IniFile::from_str(""), true);
    let mut sim = Simulation::new();
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);

    let diagnostics = sim
        .install_team_ai_registry(&registry, &rules)
        .expect("clean fixed AIMD installs");

    assert!(diagnostics.is_empty());
    assert_eq!(sim.team_script_vm.registry_counts(), (1, 1, 1, 1));
    assert!(
        sim.team_script_vm.team(1).is_none(),
        "definition installation must not allocate a live TeamClass"
    );
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
fn production_install_refuses_unknown_fixed_ai_trigger_object() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=E1\n[E1]\nStrength=100\n",
    ))
    .expect("minimal rules");
    let comparison = zero_ai_trigger_comparison();
    let fixed = IniFile::from_str(&format!(
        "[TeamTypes]\n0=TT\n[TT]\nScript=S\nTaskForce=F\n\
         [ScriptTypes]\n0=S\n[S]\n0=2,0\n\
         [TaskForces]\n0=F\n[F]\n0=1,E1\n\
         [AITriggerTypes]\nA=Trigger,TT,<all>,2,4,GHOST,{comparison},40,10,40,1,0,1,0,<none>,1,1,1\n"
    ));
    let registry = TeamAiIniRegistry::from_sources(&fixed, &IniFile::from_str(""), true);
    assert!(registry.fixed_source_is_complete());
    let mut sim = Simulation::new();
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);

    let diagnostics = sim
        .install_team_ai_registry(&registry, &rules)
        .expect_err("a fixed-origin resolution loss refuses the install");

    assert_eq!(
        diagnostics,
        vec![TeamAiInstallDiagnostic::UnknownAiTriggerObject {
            trigger_id: "A".to_string(),
            object_type: "GHOST".to_string(),
            source: TeamAiDefinitionSource::FixedAimd,
        }]
    );
    assert!(diagnostics[0].is_fixed_source_refusal());
    assert_eq!(
        sim.team_script_vm.registry_counts(),
        (0, 0, 0, 0),
        "unknown fixed AITrigger token-6 references must refuse the whole registry install"
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
    let rules = RuleSet::from_ini(&IniFile::from_str(rules)).expect("minimal rules");
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
    computer.enemy_house = Some(enemy);
    computer.base_center = Some((10, 11));
    sim.houses.insert(owner, computer);
    let mut human = HouseState::new(enemy, 1, None, true, 0, 10);
    human.base_center = Some((18, 11));
    sim.houses.insert(enemy, human);
    sim.session.house_order = vec![owner, enemy];
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
        .team_script_vm
        .construct_team(team_type, owner, true, sim.session.binary_frame as i32)
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
                .and_then(|entity| entity.attack_target.as_ref())
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
            entity.attack_target.as_ref().map(|attack| attack.target),
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

/// Script action 53 then 49: on its first frame the team's mission target
/// becomes the passable cell `AISafeDistance=` cells from the enemy's base
/// centre towards its own; the team moves there and the script goes on.
#[test]
fn a_computer_team_gathers_outside_the_enemy_base() {
    let (mut sim, rules, team_id, members, _) = attack_team_fixture("0=53,0\n1=49,0");
    let mut destinations = Vec::new();
    for _ in 0..600 {
        sim.advance_tick(&[], Some(&rules), None, None, 67);
        for &member in &members {
            if let Some(nav) = sim
                .entities()
                .get(member)
                .and_then(|entity| entity.navigation.nav_com)
                && !destinations.contains(&nav)
            {
                destinations.push(nav);
            }
        }
        if sim.team_script_vm.team(team_id).is_none() {
            break;
        }
    }
    // 4 cells from the enemy's centre (18,11) towards the house's (10,11).
    let seed = (14, 11);
    assert!(
        destinations.iter().any(|nav| matches!(
            nav,
            crate::sim::components::NavTargetRef::Cell { rx, ry }
                if (i32::from(*rx) - seed.0).abs() <= 2 && (i32::from(*ry) - seed.1).abs() <= 2
        )),
        "a member was sent near {seed:?}: {destinations:?}"
    );
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
        crate::sim::world::team_leader_greatest_threat(
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
                .and_then(|entity| entity.attack_target.as_ref())
                && !targets.contains(&attack.target)
            {
                targets.push(attack.target);
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
/// five `[SuperWeaponTypes]`, and the computer's `IRON` (Iron Curtain) and
/// `CHRONO` (Chronosphere and Chrono Warp) at (4,4) and (4,8), each Super
/// granted and charged; `Human` owns the power plants `PLANTA`
/// (`Power=100`) at (18,8) and `PLANTB` (`Power=200`) at (18,13).
fn super_team_fixture(script: &str) -> (Simulation, RuleSet, u64, [u64; 2], [u64; 2]) {
    let (mut sim, rules, team, members, plants) = team_fixture(
        "[General]\nAISafeDistance=4\n\
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
    for (building, rx, ry) in [("IRON", 4, 4), ("CHRONO", 4, 8)] {
        sim.spawn_object_at_height(building, "Computer", rx, ry, 0, 0, &rules)
            .expect("building spawns");
    }
    // As retail GACSPH, CHRONO grants no Chrono Warp: Fire_SW reaches that
    // Super ungranted.
    let owner = sim.interner.intern("Computer");
    for name in ["IronCurtainSpecial", "ChronoSphereSpecial"] {
        let id = sim.interner.intern(name);
        let mut instance = crate::sim::superweapon::SuperWeaponInstance::new(id, owner);
        instance.activate(
            rules.super_weapon(name).unwrap().recharge_time_frames,
            sim.session.binary_frame,
        );
        instance.is_ready = true;
        sim.super_weapons
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

/// Script action 55 then 49: once formed, the team's house fires its charged
/// Iron Curtain at the team's centre, the cell (11,10) between its members,
/// whose 3x3 block holds both; the script then runs to its end.
#[test]
fn a_computer_team_iron_curtains_itself() {
    let (mut sim, rules, team_id, members, _) = super_team_fixture("0=55,0\n1=49,0");
    let fires = super_fires(&mut sim, &rules, team_id);
    assert_eq!(fires, vec![("IronCurtainSpecial".to_string(), (11, 10))]);
    assert!(
        sim.team_script_vm.team(team_id).is_none(),
        "the script ran to its end"
    );
    for member in members {
        let entity = sim.entities().get(member).expect("member");
        assert_eq!(
            entity.invulnerability.as_ref().map(|state| state.kind),
            Some(crate::sim::superweapon::invulnerability::InvulnKind::IronCurtain),
            "member {member} is under the Iron Curtain"
        );
    }
    let owner = sim.interner.get("Computer").unwrap();
    let curtain = sim.interner.get("IronCurtainSpecial").unwrap();
    assert!(
        !sim.super_weapons[&owner][&curtain].is_ready,
        "it recharges"
    );
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
    let frame = sim.session.binary_frame as i32;
    let instance = sim
        .super_weapons
        .get_mut(&owner)
        .and_then(|weapons| weapons.get_mut(&curtain))
        .unwrap();
    instance.is_ready = false;
    instance.charge_start_tick = frame;
    instance.charge_duration = 30;
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
        let ready = sim.super_weapons[&owner][&curtain].is_ready;
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
    let plant_cell = (plant.position.rx, plant.position.ry);
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
    assert!(!sim.super_weapons[&owner][&sphere].is_ready, "it recharges");
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
            (entity.position.rx, entity.position.ry),
            cell,
            "member {member} arrived beside the plant"
        );
    }
}
