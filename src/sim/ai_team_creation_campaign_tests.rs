//! Campaign House team selection through its production recruitment and script owners.
use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::team_ai_ini::TeamAiIniRegistry;
use crate::sim::house_state::HouseState;
use crate::sim::rng::SimRng;
use crate::sim::snapshot::GameSnapshot;
use crate::sim::world::FrameEffects;

fn campaign(difficulty: i32) -> (Simulation, RuleSet, InternedId, InternedId) {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[General]\nTeamDelays=120,120,120\nTotalAITeamCap=30,30,30\n\
         [InfantryTypes]\n0=E1\n[E1]\nStrength=100\nTechLevel=1\n",
    ))
    .unwrap();
    let fixed = IniFile::from_str(&format!(
        "[TeamTypes]\n0=TT\n[TT]\nScript=S\nTaskForce=F\nIsBaseDefense=yes\nMax=1\n\
         [ScriptTypes]\n0=S\n[S]\n0=5,1\n1=49,0\n\
         [TaskForces]\n0=F\n[F]\n0=2,E1\n\
         [AITriggerTypes]\nA=Guard,TT,<all>,0,-1,<none>,{},40,10,60,0,0,1,0,<none>,1,0,0\n",
        "00".repeat(32)
    ));
    let map = IniFile::from_str("[AITriggerTypesEnable]\nA=yes\n");
    let registry = TeamAiIniRegistry::from_sources(&fixed, &map, false);
    let mut sim = Simulation::with_seed(0xA11CE);
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.install_team_ai_registry(&registry, &rules).unwrap();
    sim.session
        .initialize_campaign_startup(
            difficulty,
            1,
            None,
            0,
            crate::sim::campaign_carry_over::CampaignCarryOver::CLEARED,
        )
        .unwrap();
    let owner = sim.interner.intern("Computer");
    let mut house = HouseState::new(owner, 0, None, false, 0, 10);
    house.set_ai_triggers_active(true);
    sim.houses.register(house);
    let team_type = sim.interner.get("TT").unwrap();
    (sim, rules, owner, team_type)
}

#[test]
fn campaign_trigger_difficulty_uses_the_scenario_row() {
    let (mut easy, rules, owner, _) = campaign(0);
    easy.spawn_object("E1", "Computer", 10, 10, 0, &rules)
        .unwrap();
    let trigger = easy.team_script_vm.ai_triggers_in_order().next().unwrap().0;
    assert!(is_eligible(&easy, &rules, trigger, owner, None, false));

    let (mut hard, rules, owner, _) = campaign(2);
    hard.spawn_object("E1", "Computer", 10, 10, 0, &rules)
        .unwrap();
    let trigger = hard.team_script_vm.ai_triggers_in_order().next().unwrap().0;
    assert!(!is_eligible(&hard, &rules, trigger, owner, None, false));
}

#[test]
fn campaign_without_a_factory_creates_recruits_and_finishes_after_reload() {
    let (mut sim, rules, owner, team_type) = campaign(0);
    let members = [(10, 10), (12, 10)]
        .map(|(x, y)| sim.spawn_object("E1", "Computer", x, y, 0, &rules).unwrap());
    sim.advance_tick(&[], Some(&rules), None, None, 67);
    assert!(
        sim.team_script_vm.first_team_of_type(team_type).is_none(),
        "constructor timer has one frame remaining"
    );
    sim.advance_tick(&[], Some(&rules), None, None, 67);
    let team = sim
        .team_script_vm
        .first_team_of_type(team_type)
        .unwrap_or_else(|| {
            panic!(
                "no campaign team at frame {}: {:?}",
                sim.session.binary_frame(),
                sim.houses[&owner].team_creation()
            )
        });
    let team_id = team.id();
    assert_eq!(team.member_count(), 0, "House creates after the Team rung");
    assert_eq!(sim.houses[&owner].team_creation().timer().duration(), 120);
    sim.advance_tick(&[], Some(&rules), None, None, 67);
    assert_eq!(sim.team_script_vm.team(team_id).unwrap().member_count(), 1);

    let bytes = GameSnapshot::save_validated(&sim, 0, 0, "campaign_ai", 0);
    let mut loaded = GameSnapshot::load(&bytes).unwrap().sim;
    loaded.restore_after_snapshot_load().unwrap();
    crate::sim::arena_fixture::flat_ground(&mut loaded, &rules);
    // Native load resets Scenario RNG and discards active planning tokens
    // (ClearScene6370B0 / TechnoLoad70BF79); apply both to the control.
    sim.scenario_rng = SimRng::new(0);
    sim.reset_planning_after_player_load();
    assert_eq!(sim.state_hash(), loaded.state_hash());
    let mut formed = false;
    for _ in 0..40 {
        if let Some(team) = loaded.team_script_vm.team(team_id) {
            formed |= team.formed() && team.member_count() == 2;
        }
        sim.advance_tick(&[], Some(&rules), None, None, 67);
        loaded.advance_tick(&[], Some(&rules), None, None, 67);
        assert_eq!(sim.state_hash(), loaded.state_hash());
    }
    assert!(formed, "the selected team recruits both preplaced E1");
    assert!(loaded.team_script_vm.team(team_id).is_none(), "script ends");
    for member in members {
        assert_eq!(loaded.team_script_vm.team_for_member(member), None);
    }
}

#[test]
fn campaign_recruitment_fallback_checks_autocreate_flags_and_team_identity() {
    let (mut sim, rules, owner, team_type) = campaign(0);
    let member = sim
        .spawn_object("E1", "Computer", 10, 10, 0, &rules)
        .unwrap();
    let admitted = |sim: &Simulation| {
        factories_admit(
            sim,
            &rules,
            sim.team_script_vm.team_type_definition(team_type).unwrap(),
            owner,
        )
    };
    assert!(
        admitted(&sim),
        "one recruit suffices even for a two-member entry"
    );
    sim.substrate
        .entities
        .get_mut(member)
        .unwrap()
        .base_defense_response
        .author_recruitable(false, true);
    assert!(
        !admitted(&sim),
        "non-Autocreate requires the map recruit flag"
    );
    sim.team_script_vm.mark_autocreate(team_type);
    assert!(admitted(&sim));
    sim.substrate
        .entities
        .get_mut(member)
        .unwrap()
        .base_defense_response
        .set_team_recruitable(false);
    assert!(!admitted(&sim), "Autocreate requires the team recruit flag");
    sim.substrate
        .entities
        .get_mut(member)
        .unwrap()
        .base_defense_response
        .author_recruitable(true, true);
    let team = sim.create_team(team_type, Some(owner), &rules).unwrap();
    assert!(sim.team_add_member(team, member, false, &rules, None, FrameEffects::default()));
    assert!(
        !admitted(&sim),
        "an existing team of the same type is no source"
    );
}

#[test]
fn retail_all05_initiate_team_accepts_its_campaign_barracks() {
    use crate::rules::process_owner::NativeScenarioRulesPrefix;
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let map =
        crate::map::map_file::MapFile::from_bytes(assets.get_ref("ALL05UMD.MAP").unwrap()).unwrap();
    let named = assets.resolve_ref("ALL05UMD.INI").map(|_| {
        crate::rules::retail_sources::select_ini(&assets, "ALL05UMD.INI")
            .unwrap()
            .ini
    });
    let (rules, _, _, _) = crate::rules::retail_ini_fixture::retail_rules_owner(&assets)
        .load_scenario(
            NativeScenarioRulesPrefix::Campaign(named.as_ref()),
            &map.ini,
        )
        .unwrap()
        .into_parts();
    let registry = TeamAiIniRegistry::load_retail(&assets, &map.ini, false).unwrap();
    let roster =
        crate::map::houses::parse_house_roster(&map.ini, &rules.color_schemes, Some(&rules));
    let mut sim = Simulation::with_seed(31);
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    crate::sim::scenario_bootstrap::initialize_map_roster_houses(
        &mut sim,
        &roster,
        &rules,
        Some(&map.ini),
    );
    sim.install_team_ai_registry(&registry, &rules).unwrap();
    let owner = sim
        .house_by_country_index(rules.country_index("BadGuy1").unwrap(), &rules)
        .unwrap();
    let owner_name = sim.interner.resolve(owner).to_owned();
    let team_type_id = sim.interner.get("090E667C").unwrap();
    let team_type = sim
        .team_script_vm
        .team_type_definition(team_type_id)
        .unwrap();
    assert!(!factories_admit(&sim, &rules, team_type, owner));
    // The ordinary factory arm, using the stock map's country and complete
    // campaign rules/AI passes; this controlled arena does not play ALL05.
    sim.spawn_object("YAPOWR", &owner_name, 8, 8, 0, &rules)
        .unwrap();
    sim.spawn_object("YABRCK", &owner_name, 14, 8, 0, &rules)
        .unwrap();
    let team_type = sim
        .team_script_vm
        .team_type_definition(team_type_id)
        .unwrap();
    assert!(factories_admit(&sim, &rules, team_type, owner));
}
