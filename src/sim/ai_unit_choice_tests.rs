//! Replay of the chooser and harvester rows of `tools/ai_team_oracle.json`
//! against the computer's unit choices.
//!
//! Each row carries what the native run's stubs answered (CanBuild, Cost_Of
//! and the money, IsRecruitable, the draws); the Rust chooser must ask in
//! the same order and reach the same choice. The teams it tallies are those
//! the original asked for their needs (`Get_Needed_Types`); which teams
//! qualify (`TeamScriptState::wants_members`) rests on instruction reading.

use super::*;
use crate::sim::ai_team_creation::tests::{Draws, dword, events, flag, index, native_draws, rows};
use crate::sim::house_state::HouseState;
use serde_json::Value;

/// HouseChooseInfantry4FEEE0 counts Foot4DA230's free objects without an HP
/// gate. SA's retained Die1 corpse therefore covers a stock team's GI need
/// until DoType_Sequencer520CAD UnInits it. The lethal receiver is invoked
/// explicitly; the sequence then completes through ordinary simulation ticks.
#[test]
fn retail_infantry_choice_waits_for_the_retained_corpse_to_finish() {
    use crate::sim::world::FrameEffects;

    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let mut rules = retail.rules;
    let (_, assets) = crate::rules::retail_ini_fixture::retail_assets().unwrap();
    let roots = crate::rules::effect_asset_catalog::anim_class_roots(&rules);
    rules.bind_anim_class_assets(&roots, &assets, "TEM", "TEMPERATE");
    rules.bind_effect_assets(&assets, "TEM", "TEMPERATE");
    let mut sim = Simulation::with_seed(0x4FF050);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::Skirmish);
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    sim.fog = crate::sim::vision::FogState::new(32, 32);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.resolve_rule_animation_lists(&rules);
    let owner = sim.interner.intern("Americans");
    sim.houses
        .register(HouseState::new(owner, 0, Some(owner), false, 10_000, 10));
    let registry = crate::rules::team_ai_ini::TeamAiIniRegistry::load_retail(
        &assets,
        &crate::rules::ini_parser::IniFile::from_str(""),
        sim.session.game_mode_nonzero(),
    )
    .unwrap();
    assert!(
        sim.install_team_ai_registry(&registry, &rules)
            .unwrap()
            .is_empty()
    );
    // Physical AIMD: Allied Base Guard - Refine needs three E1 and three GGI.
    let team_type = sim.interner.get("06175EFC-G").unwrap();
    sim.create_team(team_type, Some(owner), &rules).unwrap();
    let mut victim = None;
    for (index, type_name) in ["E1", "E1", "E1", "GGI", "GGI", "GGI"]
        .into_iter()
        .enumerate()
    {
        let id = sim
            .spawn_object(type_name, "Americans", 8 + index as u16 * 2, 8, 0, &rules)
            .unwrap();
        victim.get_or_insert(id);
    }
    let victim = victim.unwrap();
    let hit = crate::sim::combat::EntityDamageEvent::direct_receiver(
        victim,
        100_000,
        0,
        crate::sim::combat::RAD_NO_ATTACKER,
        None,
        sim.interner.intern("SA"),
        crate::sim::combat::ReceiverCallFlags {
            ignore_defenses: false,
            arg6: false,
        },
    );
    assert_eq!(rules.warhead("SA").unwrap().inf_death, 1);
    sim.commit_noncombat_aoe_hits(&rules, None, &[hit], FrameEffects::default());
    let corpse = sim.substrate.entities.get(victim).unwrap();
    assert_eq!(corpse.health.current(), 0);
    assert!(corpse.lifecycle.object_alive());
    assert!(!corpse.lifecycle.in_limbo());
    assert!(!corpse.death.is_dying());
    assert_eq!(corpse.mission_leaf.as_infantry().unwrap().doing(), 11);
    assert_eq!(
        corpse.mission.effective(),
        crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Guard)
    );
    assert!(corpse.base_defense_response.recruitable_a());
    assert!(sim.team_script_vm.team_for_member(victim).is_none());
    crate::sim::ai_base_building::update_production_choices(&mut sim, &rules, owner, None);
    assert_eq!(
        sim.houses
            .get(&owner)
            .unwrap()
            .ai_unit_choices()
            .get(UnitChoiceKind::Infantry),
        -1,
        "the retained body still covers the GI need"
    );
    let barracks_rtti = rules.object("GAPILE").unwrap().factory_rtti();
    assert!(
        crate::sim::ai_base_building::suggest_new_object(&sim, &rules, owner, barracks_rtti, false)
            .is_none()
    );

    for _ in 0..180 {
        let grid = sim.path_grid_snapshot().unwrap();
        sim.advance_tick(&[], Some(&rules), Some(&grid), None, 33);
        let retained = sim
            .substrate
            .entities
            .get(victim)
            .is_some_and(|entity| entity.lifecycle.object_alive());
        let choice = sim
            .houses
            .get(&owner)
            .unwrap()
            .ai_unit_choices()
            .get(UnitChoiceKind::Infantry);
        if !retained && choice != -1 {
            assert_eq!(
                crate::sim::ai_base_building::suggest_new_object(
                    &sim,
                    &rules,
                    owner,
                    barracks_rtti,
                    false
                )
                .map(|object| object.id.as_str()),
                Some("E1"),
                "the barracks now receives the replacement GI choice"
            );
            return;
        }
        if retained {
            assert_eq!(choice, -1, "no replacement while the body remains");
        }
    }
    panic!("the physical GI Die1 sequence must retire and permit its replacement");
}

#[test]
fn the_choosers_match_the_original() {
    for (number, row) in rows("choosers").iter().enumerate() {
        let context = format!("{} chooser row {number}", row["kind"]);
        let teams = row["teams"].as_array().unwrap();
        let types = row["types"].as_array().unwrap();
        let tallied: Vec<(i32, Vec<usize>)> = events(row, "needed")
            .into_iter()
            .map(|asked| {
                let team = &teams[index(&asked[1])];
                // A needed type of another class (`["other", n]`) is not
                // this chooser's.
                let needed = team["needed"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(Value::as_u64)
                    .map(|class_index| class_index as usize)
                    .collect();
                (dword(&team["created"]), needed)
            })
            .collect();
        let objects = row["objects"].as_array().unwrap();
        let mut asked_recruitable = Vec::new();
        let mut asked_can_build = Vec::new();
        let candidates = tally(
            types.len(),
            tallied,
            objects
                .iter()
                .enumerate()
                .map(|(slot, object)| (index(&object[0]), slot)),
            |slot| {
                asked_recruitable.push(slot);
                flag(&objects[slot][1])
            },
            dword(&row["money"]),
            |class_index| {
                asked_can_build.push(class_index);
                match dword(&types[class_index][0]) {
                    0 => CanBuild::No,
                    1 => CanBuild::Yes,
                    -1 => CanBuild::AtLimit,
                    other => panic!("CanBuild answers {other}"),
                }
            },
            |class_index| dword(&types[class_index][1]),
        );
        let native_can_build: Vec<usize> = events(row, "can_build")
            .into_iter()
            .map(|asked| index(&asked[1]))
            .collect();
        let native_recruitable: Vec<usize> = events(row, "recruitable")
            .into_iter()
            .map(|asked| index(&asked[1]))
            .collect();
        assert_eq!(
            asked_recruitable, native_recruitable,
            "{context}: IsRecruitable"
        );
        assert_eq!(asked_can_build, native_can_build, "{context}: CanBuild");
        let fill = dword(&row["fill"][index(&row["difficulty"])]);
        let mut draws = Draws::new(row);
        let choice = pick(&candidates, fill, |low, high| draws.draw(low, high));
        assert_eq!(draws.asked, native_draws(row), "{context}: draws");
        assert_eq!(choice.unwrap_or(-1), dword(&row["choice"]), "{context}");
    }
}

/// The executed gate (`0x004FEE59..0x004FEE89`) at the boundary draws of
/// each `FillEarliestTeamProbability`, the stock 100 among them: the earliest
/// team's type exactly when the native compare falls through.
#[test]
fn the_fill_earliest_gate_takes_the_native_draws() {
    let corpus = crate::test_fixture::json("tools/unit_draw_oracle.json");
    // The earliest team needs type 4; type 7 has the greater need.
    let candidates = [
        Candidate {
            index: 4,
            need: 1,
            earliest: 10,
        },
        Candidate {
            index: 7,
            need: 2,
            earliest: 20,
        },
    ];
    for row in corpus["fill_earliest"].as_array().unwrap() {
        let fill = dword(&row["fill_percent"]);
        let roll = dword(&row["draw"]);
        let mut draws = [roll, 0].into_iter();
        let choice = pick(&candidates, fill, |_, _| draws.next().unwrap());
        let earliest = row["earliest"].as_bool().unwrap();
        assert_eq!(
            choice,
            Some(if earliest { 4 } else { 7 }),
            "{fill}%, draw {roll:#x}"
        );
    }
}

#[test]
fn the_harvester_branch_matches_the_original() {
    let mut sim = Simulation::new();
    let name = sim.interner.intern("H0");
    for (number, row) in rows("harvester").iter().enumerate() {
        let mut house = HouseState::new(name, 0, None, flag(&row["human"]), 0, dword(&row["tech"]));
        house.set_player_control_for_test(flag(&row["control"]));
        // The fixture's harvester is vehicle 7; its refinery undeploys into
        // vehicle 9.
        let facts = HarvesterFacts {
            harvester: (flag(&row["harvester"]) && flag(&row["owned"]))
                .then(|| (7, dword(&row["harvester_tech"]))),
            refinery: flag(&row["refinery"]).then(|| flag(&row["undeploys"]).then_some(9)),
            gatherers: dword(&row["gatherers"]),
            destinations: dword(&row["destinations"]),
            harvesters_per_refinery: dword(&row["per_refinery"]),
            slave_miners: dword(&row["slave_miners"]),
            current_iq: dword(&row["iq"]),
            iq_harvester: dword(&row["iq_harvester"]),
            no_ore: flag(&row["no_ore"]),
            human: house.is_controlled_by_human(dword(&row["game_mode"]) != 0),
            tech_level: house.tech_level(),
        };
        let decision = harvester_decision(&facts);
        assert_eq!(
            decision.unwrap_or(-1),
            dword(&row["choice"]),
            "harvester row {number}"
        );
        // Past the branch, the team pass (no candidates here) draws once.
        assert_eq!(
            native_draws(row).len(),
            usize::from(decision.is_none()),
            "harvester row {number}: draws"
        );
    }
}

/// A state saved before the NPSE naval slot existed keeps its land channel and
/// loads the naval one as "no choice".
#[test]
fn a_saved_state_without_the_naval_slot_loads_as_none() {
    let choices: HouseAiUnitChoices =
        serde_json::from_str(r#"{"unit":0,"infantry":-1,"aircraft":-1}"#).unwrap();
    assert_eq!(choices.get(UnitChoiceKind::Unit), 0);
    assert_eq!(choices.get(UnitChoiceKind::NavalUnit), -1);
    let round_trip: HouseAiUnitChoices =
        serde_json::from_str(&serde_json::to_string(&choices).unwrap()).unwrap();
    assert_eq!(round_trip, choices);
    let mut restarted = choices;
    restarted.set(UnitChoiceKind::NavalUnit, 1);
    restarted.clear_all();
    assert_eq!(restarted.get(UnitChoiceKind::Unit), -1);
    assert_eq!(
        restarted.get(UnitChoiceKind::NavalUnit),
        1,
        "native restart preserves an unconsumed naval choice"
    );
}

/// NPSE 2.0 House Update C33AB0 asks both channels without a shipyard
/// gate; both calls share the harvester return at C339D0..C33A34.
#[cfg(feature = "npatch")]
#[test]
fn naval_choice_runs_the_shared_harvester_branch_without_a_shipyard() {
    use crate::rules::ini_parser::IniFile;

    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[General]\nHarvesterUnit=SHIP\nAISlaveMinerNumber=1,1,1\n\
         [IQ]\nHarvester=0\n[Countries]\n0=AI\n[AI]\nSide=GDI\n\
         [VehicleTypes]\n0=SHIP\n[SHIP]\nOwner=AI\nNaval=yes\nTechLevel=1\n",
    ))
    .unwrap();
    let mut sim = Simulation::with_seed(39);
    let owner = sim.interner.intern("AI");
    sim.houses
        .register(HouseState::new(owner, 0, Some(owner), false, 1_000, 10));
    crate::sim::ai_base_building::update_production_choices(&mut sim, &rules, owner, None);
    let choices = sim.houses[&owner].ai_unit_choices();
    assert_eq!(choices.get(UnitChoiceKind::Unit), -1);
    assert_eq!(choices.get(UnitChoiceKind::NavalUnit), 0);
}
