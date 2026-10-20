//! TAction140/141: Execute6E4460/6E44C0 -> House50DA20/50DA50.
//! Original gamemd.exe.
//! Writes/admission are instruction-established; timing goldens below reuse
//! original AI_TryFireSW execution, not a second implementation of its timer.
use super::*;
use crate::map::waypoints::Waypoint;
use crate::rules::ini_parser::IniFile;
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::snapshot::GameSnapshot;
use crate::sim::superweapon::ai_fire::{AI_FIRE_LOG, AiFireEvent};

fn fixture() -> (Simulation, InternedId, InternedId) {
    let mut sim = Simulation::with_seed(0);
    let owner = sim.interner.intern("IncomingHouse");
    let other = sim.interner.intern("UnrelatedHouse");
    for id in [owner, other] {
        sim.houses
            .register(HouseState::new(id, 0, None, false, 0, 10));
    }
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    (sim, owner, other)
}

fn waypoint(sim: &mut Simulation, cell: (u16, u16)) {
    sim.session.install_waypoints(
        &[(
            0,
            Waypoint {
                index: 0,
                rx: cell.0,
                ry: cell.1,
            },
        )]
        .into_iter()
        .collect(),
    );
}

fn apply(sim: &mut Simulation, owner: Option<InternedId>, kind: i32, tail: &str) -> bool {
    // The deliberately unrelated scalar must not choose the House or cell.
    let ini = IniFile::from_str(&format!("[Actions]\nCASE=1,{kind},0,777,0,0,0,0,{tail}\n"));
    let entry = crate::map::actions::parse_actions(&ini)
        .remove("CASE")
        .unwrap()
        .entries()[0]
        .clone();
    let action = RuntimeDefinitions::default().resolve_action(entry, None, None);
    sim.apply_live_trigger_action(
        &action,
        TriggerActionContext {
            live_index: None,
            incoming_house: owner,
            input: TagEventInput::default(),
        },
        &mut Vec::new(),
        None,
        None,
        FrameEffects::default(),
    )
}

fn rngs(sim: &Simulation) -> [String; 3] {
    [
        sim.main_rng.native_state_hex(),
        sim.scenario_rng.native_state_hex(),
        sim.mapgen_rng.native_state_hex(),
    ]
}

#[test]
fn defense_actions_store_raw_cell_and_frame_without_changing_other_targets_or_rng() {
    for frame in [17, i32::MAX as u32, i32::MIN as u32, u32::MAX] {
        for cell in [(0, 22), (11, 0), (u16::MAX, 32768)] {
            let (mut sim, owner, other) = fixture();
            sim.session.set_binary_frame_for_test(frame);
            waypoint(&mut sim, cell);
            let house = sim.houses.get_mut(&owner).unwrap();
            house.set_preferred_super_weapon_target((7, 8));
            house.set_nuke_target((9, 10));
            sim.houses
                .get_mut(&other)
                .unwrap()
                .alert_super_weapon_defense((13, 14), 15);
            let before = rngs(&sim);
            assert!(apply(&mut sim, Some(owner), 140, "A"));
            assert_eq!(
                sim.houses[&owner].super_weapon_defense(),
                (cell, frame as i32)
            );
            // Clear ignores its unused/malformed waypoint, unlike Set.
            assert!(apply(&mut sim, Some(owner), 141, "<none>"));
            assert_eq!(sim.houses[&owner].super_weapon_defense(), ((0, 0), -100));
            assert_eq!(sim.houses[&owner].preferred_super_weapon_target(), (7, 8));
            assert_eq!(sim.houses[&owner].nuke_target(), (9, 10));
            assert_eq!(sim.houses[&other].super_weapon_defense(), ((13, 14), 15));
            assert_eq!(rngs(&sim), before);
        }
    }
}

#[test]
fn refused_defense_actions_preserve_the_existing_alert() {
    let (mut sim, owner, _) = fixture();
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .alert_super_weapon_defense((10, 11), 123);
    for (cell, tail) in [
        ((0, 0), "A"),
        ((21, 22), "B"),
        ((21, 22), "ZZZ"),
        ((21, 22), "<none>"),
    ] {
        waypoint(&mut sim, cell);
        assert!(!apply(&mut sim, Some(owner), 140, tail));
        assert_eq!(sim.houses[&owner].super_weapon_defense(), ((10, 11), 123));
    }
    waypoint(&mut sim, (21, 22));
    for kind in [140, 141] {
        assert!(!apply(&mut sim, None, kind, "A"));
        let missing = sim.interner.intern("NoRegisteredHouse");
        assert!(!apply(&mut sim, Some(missing), kind, "A"));
        assert_eq!(sim.houses[&owner].super_weapon_defense(), ((10, 11), 123));
    }
}

fn ai_fire_cells(
    sim: &mut Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    owner: InternedId,
) -> Vec<(u16, u16)> {
    AI_FIRE_LOG.with(|log| *log.borrow_mut() = Some(Vec::new()));
    crate::sim::superweapon::ai_fire::try_fire(sim, rules, owner, None, FrameEffects::default());
    AI_FIRE_LOG.with(|log| {
        log.borrow_mut()
            .take()
            .unwrap()
            .into_iter()
            .filter_map(|e| {
                if let AiFireEvent::Fire(_, cell) = e {
                    Some(cell)
                } else {
                    None
                }
            })
            .collect()
    })
}

#[test]
fn scripted_alerts_drive_the_original_force_shield_timing_controls() {
    let Some(mut rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules() else {
        return;
    };
    // The consumer's key is checked through the physical retail MD reader.
    assert_eq!(rules.general.ai_super_defense_frames, 50);
    let shield = rules
        .super_weapon_order
        .iter()
        .find(|name| {
            rules.super_weapon(name).unwrap().kind
                == crate::rules::superweapon_type::SuperWeaponKind::ForceShield
        })
        .unwrap()
        .clone();
    let corpus = crate::test_fixture::json("tools/superweapon_oracle.json");
    let rows = corpus["ai_try_fire"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["kinds"] == serde_json::json!([[10, true]]));
    let mut compared = 0;
    for row in rows {
        compared += 1;
        let (mut sim, owner, _) = fixture();
        sim.intern_rule_type_ids(&rules);
        crate::sim::superweapon::construct_house_supers(&mut sim, &rules, owner, false);
        let shield = sim.interner.get(&shield).unwrap();
        // Original controls supply ready, ungranted Supers: observe the actual
        // AI -> Fire_SW call; ClickFire then refuses, so nothing launches.
        sim.super_weapons
            .map_mut_for_test()
            .get_mut(&owner)
            .unwrap()
            .get_mut(&shield)
            .unwrap()
            .set_is_ready_for_test(true);
        let cell = (
            row["defense"][0].as_i64().unwrap() as u16,
            row["defense"][1].as_i64().unwrap() as u16,
        );
        waypoint(&mut sim, cell);
        sim.session
            .set_binary_frame_for_test(row["defense_frame"].as_i64().unwrap() as u32);
        assert_eq!(apply(&mut sim, Some(owner), 140, "A"), cell != (0, 0));
        rules.general.ai_super_defense_frames = row["defense_frames"].as_i64().unwrap() as i32;
        sim.session
            .set_binary_frame_for_test(row["frame"].as_i64().unwrap() as u32);
        let expected: Vec<_> = row["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|event| {
                (
                    event[2][0].as_i64().unwrap() as u16,
                    event[2][1].as_i64().unwrap() as u16,
                )
            })
            .collect();
        assert_eq!(ai_fire_cells(&mut sim, &rules, owner), expected, "{row}");
        assert!(apply(&mut sim, Some(owner), 141, "A"));
        assert!(
            ai_fire_cells(&mut sim, &rules, owner).is_empty(),
            "clear: {row}"
        );
    }
    assert_eq!(compared, 13);
}

#[test]
fn scripted_defense_state_survives_snapshot_then_clears_through_the_same_owner() {
    let (mut original, owner, _) = fixture();
    original.session.set_binary_frame_for_test(i32::MIN as u32);
    waypoint(&mut original, (u16::MAX, 32768));
    assert!(apply(&mut original, Some(owner), 140, "A"));
    let hash = original.state_hash();
    let bytes = GameSnapshot::save(&original, 0, 0, "Defense targets", 0);
    let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
    restored.restore_after_snapshot_load().unwrap();
    assert_eq!(restored.state_hash(), hash);
    assert_eq!(
        restored.houses[&owner].super_weapon_defense(),
        ((u16::MAX, 32768), i32::MIN)
    );
    for sim in [&mut original, &mut restored] {
        assert!(apply(sim, Some(owner), 141, "A"));
        assert_eq!(sim.houses[&owner].super_weapon_defense(), ((0, 0), -100));
    }
    assert_eq!(restored.state_hash(), original.state_hash());
    assert_ne!(restored.state_hash(), hash);
}
