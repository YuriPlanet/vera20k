//! Tests for the house defeat. `native_defeat_gate_corpus` and
//! `native_blowup_all_corpus` compare against
//! `tools/spatial_oracle/house_defeat_gate.json` and `house_blowup_all.json`,
//! produced by running the original code under Unicorn; the rest are Rust
//! regression tests of the production pass.

use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::capture_manager::{CaptureManagerState, MindControlLink};
use crate::sim::combat::{RAD_NO_ATTACKER, ReceiverCallFlags};
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::world::Simulation;
use crate::util::save_codec;

/// `[Sides]` listing country `Country{n}` as side `n`, the one at
/// `civilian_side` named `Civilian` (absent past the list, as when no house
/// has that side).
fn rules(civilian_side: usize) -> RuleSet {
    let mut sides = String::from("[Countries]\n0=Country0\n1=Country1\n2=Country2\n3=Country3\n");
    sides.push_str("[Country0]\n[Country1]\n[Country2]\n[Country3]\n[Sides]\n");
    for index in 0..4 {
        if index == civilian_side {
            sides.push_str(&format!("Civilian=Country{index}\n"));
        } else {
            sides.push_str(&format!("Side{index}=Country{index}\n"));
        }
    }
    let text = format!(
        "{sides}\
[General]
BaseUnit=AMCV,SMCV,PCV
[AI]
BuildRefinery=NAREFN,GAREFN,YAREFN
[CombatDamage]
C4Warhead=Super
[InfantryTypes]
0=CLEG
[VehicleTypes]
0=TANK
1=AMCV
2=SMCV
3=PCV
4=SHIP
[AircraftTypes]
0=AIR
[BuildingTypes]
0=NAREFN
1=GAREFN
2=YAREFN
[CLEG]
Strength=125
Primary=NeutronRifle
[TANK]
Strength=1000
Armor=heavy
[AMCV]
Strength=1000
[SMCV]
Strength=1000
[PCV]
Strength=1000
[SHIP]
Strength=1000
Naval=yes
[AIR]
Strength=1000
[NAREFN]
Strength=1000
[GAREFN]
Strength=1000
[YAREFN]
Strength=1000
[NeutronRifle]
Damage=8
Projectile=Invisible
Warhead=ChronoBeam
[Invisible]
Inviso=yes
[Warheads]
0=Super
1=ChronoBeam
[Super]
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
[ChronoBeam]
Temporal=yes
"
    );
    RuleSet::from_ini(&IniFile::from_str(&text)).expect("house defeat rules")
}

/// Houses `house0..` of type `Country{side}` (side `side` in [`rules`]) for
/// the given side indices, in that array order, in a non-campaign game past
/// frame zero.
fn sim_with_houses(sides: &[u8]) -> (Simulation, Vec<InternedId>) {
    let mut sim = Simulation::with_seed(5);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    sim.session.set_binary_frame_for_test(100);
    let houses = sides
        .iter()
        .enumerate()
        .map(|(n, &side)| {
            let id = sim.interner.intern(&format!("house{n}"));
            let country = sim.interner.intern(&format!("Country{side}"));
            sim.houses
                .register(HouseState::new(id, side, Some(country), false, 0, 10));
            id
        })
        .collect();
    (sim, houses)
}

#[test]
fn observer_defeat_waits_for_hostile_players_then_finishes_without_loss_voice() {
    use crate::sim::scenario_session::ScenarioGameMode;
    use crate::sim::world::SimSoundEvent;
    let (mut sim, owners) = sim_with_houses(&[0, 1, 2]);
    sim.session.set_game_mode(ScenarioGameMode::Lan);
    sim.session.install_network_timing(10);
    sim.session.set_current_house_for_test(Some(owners[0]));
    sim.session.install_multiplayer_house_identity(
        crate::skirmish_modes::MpModeClass::Battle,
        Some(owners[0]),
    );
    for (index, owner) in owners.iter().enumerate() {
        let house = sim.houses.get_mut(owner).unwrap();
        house.set_is_human_for_test(true);
        house
            .tracking_mut()
            .set_for_test(i32::from(index != 0), &[], (0, 0, 0), &[]);
    }
    sim.check_defeat(None, None);
    assert!(sim.houses[&owners[0]].is_defeated());
    assert!(
        !sim.houses[&owners[0]].has_lost(),
        "observer watches the two hostile human players"
    );
    assert!(sim.houses[&owners[0]].map_is_clear());
    assert!(!sim.sound_events.iter().any(|event| matches!(event,
        SimSoundEvent::PlayerDefeated { house } if *house == owners[0])));
    sim.houses
        .get_mut(&owners[1])
        .unwrap()
        .tracking_mut()
        .set_for_test(0, &[], (0, 0, 0), &[]);
    sim.check_defeat(None, None);
    assert!(sim.houses[&owners[0]].has_lost());
    assert_eq!(
        sim.houses[&owners[0]].outcome_timer(),
        crate::sim::timer::CdTimer::started(100, 30)
    );
    assert!(
        !sim.houses[&owners[1]].has_lost(),
        "only PlayerPtr gets the result"
    );
    assert!(
        !sim.houses[&owners[2]].has_won(),
        "remote survivor does not acquire this client's result"
    );
    assert!(
        !sim.sound_events
            .iter()
            .any(|event| matches!(event, SimSoundEvent::MatchOutcome { .. }))
    );
}

#[test]
fn cooperative_defeated_local_waits_for_partner_and_shares_completed_victory() {
    use crate::sim::scenario_session::ScenarioGameMode;
    let (mut sim, owners) = sim_with_houses(&[0, 1, 2]);
    sim.session.set_game_mode(ScenarioGameMode::Lan);
    sim.session.install_network_timing(10);
    sim.session.set_current_house_for_test(Some(owners[0]));
    sim.session
        .install_multiplayer_house_identity(crate::skirmish_modes::MpModeClass::Cooperative, None);
    for (index, owner) in owners.iter().enumerate() {
        let house = sim.houses.get_mut(owner).unwrap();
        house.set_is_human_for_test(index < 2);
        house
            .tracking_mut()
            .set_for_test(i32::from(index != 0), &[], (0, 0, 0), &[]);
    }
    sim.check_defeat(None, None);
    assert!(sim.houses[&owners[0]].is_defeated());
    assert!(!sim.houses[&owners[0]].has_lost());
    sim.houses
        .get_mut(&owners[2])
        .unwrap()
        .tracking_mut()
        .set_for_test(0, &[], (0, 0, 0), &[]);
    sim.check_defeat(None, None);
    assert!(sim.houses[&owners[0]].has_won());
    assert!(!sim.houses[&owners[1]].has_won());
}

/// Added selected mode in the existing House oracle. These rows execute the
/// whole Flag bodies (including7C5F00), excluding only recorded UI/audio
/// sinks; the second group executes the original House timer/AllowWin heads.
/// No renderer, wall-voice completion, full world or campaign-next-page claim.
#[test]
fn native_flag_result_and_savour_controls() {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/house_outcomes.json",
    ))
    .unwrap();
    for row in corpus["flag_controls"].as_array().unwrap() {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        let delay = input["delay"].as_str().unwrap_or("constructor");
        let delay_line = match delay {
            "constructor" => String::new(),
            "retail" => format!(
                "SavourDelay={}\n",
                corpus["retail"]["token"].as_str().unwrap()
            ),
            token => format!("SavourDelay={token}\n"),
        };
        let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[Countries]\n0=Country0\n[Country0]\n[Sides]\nAllied=Country0\n[AudioVisual]\n{delay_line}"
        ))).unwrap();
        assert_eq!(
            format!("{:016x}", rules.general.savour_delay_minutes.to_bits()),
            row["savour_bits"].as_str().unwrap(),
            "{name}: reader input"
        );
        let mut sim = Simulation::new();
        sim.session
            .set_binary_frame_for_test(input["frame"].as_i64().unwrap_or(100) as u32);
        let mode = crate::sim::scenario_session::ScenarioGameMode::from_game_mode(
            input["game_mode"].as_i64().unwrap_or(0) as i32,
        );
        sim.session.set_game_mode(mode);
        if mode.is_network() {
            sim.session
                .install_network_timing(input["max_ahead"].as_i64().unwrap_or(0) as i32);
        }
        let owner = sim.interner.intern("Player");
        let country = sim.interner.intern("Country0");
        let mut house = HouseState::new(owner, 0, Some(country), true, 0, 10);
        let supplied = input["timer"].as_array();
        let timer = crate::sim::timer::CdTimer::from_raw(
            supplied.map_or(-1, |timer| timer[0].as_i64().unwrap() as i32),
            supplied.map_or(0, |timer| timer[2].as_i64().unwrap() as i32),
        );
        house.supply_outcome_state_for_test(
            input["won"].as_i64().unwrap_or(0) != 0,
            input["lost"].as_i64().unwrap_or(0) != 0,
            None,
            timer,
        );
        sim.houses.register(house);
        sim.session.set_current_house_for_test(Some(owner));
        if input["observer"].as_bool().unwrap_or(false) {
            sim.session.install_multiplayer_house_identity(
                crate::skirmish_modes::MpModeClass::Battle,
                Some(owner),
            );
        }
        let skip = input["skip_timer"].as_bool().unwrap_or(false);
        let returned = match input["operation"].as_str().unwrap() {
            "win" => sim.flag_house_to_win(owner, &rules, skip, Default::default()),
            "lose" => sim.flag_house_to_lose(owner, &rules, skip, Default::default()),
            "end" => sim.trigger_house_outcome(69, 0, &rules, Default::default()),
            other => panic!("unhandled control {other}"),
        };
        let house = &sim.houses[&owner];
        assert_eq!(house.has_won(), row["won"].as_u64().unwrap() != 0, "{name}");
        assert_eq!(
            house.has_lost(),
            row["lost"].as_u64().unwrap() != 0,
            "{name}"
        );
        assert_eq!(
            house.outcome_timer().start_frame(),
            row["timer"][0].as_i64().unwrap() as i32,
            "{name}"
        );
        assert_eq!(
            house.outcome_timer().duration(),
            row["timer"][2].as_i64().unwrap() as i32,
            "{name}"
        );
        if input["operation"] != "end" {
            assert_eq!(
                returned,
                row["al"].as_u64().unwrap() != 0,
                "{name}: Flag AL"
            );
        }
        let sinks = row["calls"].as_array().unwrap();
        assert_eq!(
            sim.session.input_locked(),
            sinks.iter().any(|call| call["address"] == "0x684240"),
            "{name}: campaign input lock"
        );
        let native_eva = sinks
            .iter()
            .filter(|call| call["address"] == "0x752700")
            .count();
        assert_eq!(
            sim.sound_events
                .iter()
                .filter(|event| matches!(
                    event,
                    crate::sim::world::SimSoundEvent::MatchOutcome { .. }
                ))
                .count(),
            native_eva,
            "{name}: accepted EVA edge"
        );
    }
}

#[test]
fn native_house_result_zero_and_allow_win_heads() {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/house_outcomes.json",
    ))
    .unwrap();
    for row in corpus["result_gates"].as_array().unwrap() {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        let won = input["won"].as_u64().unwrap() != 0;
        let timer = crate::sim::timer::CdTimer::from_raw(
            input["timer"][0].as_i64().unwrap() as i32,
            input["timer"][2].as_i64().unwrap() as i32,
        );
        let mut house = HouseState::new(Default::default(), 0, None, true, 0, 10);
        house.supply_outcome_state_for_test(
            won,
            !won,
            Some(crate::sim::house_state::HouseOutcomeState {
                kind: if won {
                    crate::sim::house_state::HouseOutcomeKind::Victory
                } else {
                    crate::sim::house_state::HouseOutcomeKind::Defeat
                },
                exit_ready: false,
            }),
            timer,
        );
        house.supply_allow_win_tag_state(input["allow_win"].as_i64().unwrap() as i32, timer);
        let ready = house.advance_outcome_savour(
            input["frame"].as_i64().unwrap() as i32,
            input["game_mode"].as_i64().unwrap() != 0,
        );
        assert_eq!(ready, row["gate"] == "result_wait", "{name}");
    }
}

#[derive(serde::Deserialize)]
#[cfg(not(feature = "ares"))]
struct NativeGateCase {
    input: serde_json::Value,
    events: Vec<String>,
}

/// `HouseClass::Update`'s gate (`0x004F8E86..0x004F8F82`) against the
/// original, case by case: the campaign, Defeated, frame and
/// MultiplayPassive refusals, the short game's `+0x2F0 > 0` and BaseUnit
/// entries 1, 2 and 0 (a negative building count defeats, cancelling MCV
/// counts defeat, on-map units do not count), and the normal game's zero sum
/// (a cancelling sum defeats; BuildRefinery's third type counts only when
/// set).
#[test]
#[cfg(not(feature = "ares"))]
fn native_defeat_gate_corpus() {
    let cases: Vec<NativeGateCase> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/house_defeat_gate.json",
    ))
    .unwrap();
    assert_eq!(cases.len(), 21);
    for case in cases {
        let input = &case.input;
        let name = input["name"].as_str().unwrap();
        let int = |key: &str| input[key].as_i64().unwrap_or(0) as i32;
        let mut rules = rules(3);
        if input["build_refinery_2"].as_bool() == Some(false) {
            rules.build_refinery_types.truncate(2);
        }
        let (mut sim, houses) = sim_with_houses(&[0]);
        let house = houses[0];
        sim.session.set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(
                input["game_mode"].as_i64().unwrap_or(1) != 0,
            ),
        );
        sim.session
            .set_binary_frame_for_test(input["frame"].as_i64().unwrap_or(100) as u32);
        sim.session.game_options_mut_for_test().short_game =
            input["short_game"].as_i64().unwrap_or(1) != 0;
        let types = ["AMCV", "SMCV", "PCV"].map(|name| sim.interner.intern(name));
        let yarefn = sim.interner.intern("YAREFN");
        let base_units: Vec<(InternedId, i32)> = input["base_units"]
            .as_object()
            .map(|slots| {
                slots
                    .iter()
                    .map(|(slot, n)| {
                        (
                            types[slot.parse::<usize>().unwrap()],
                            n.as_i64().unwrap() as i32,
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        {
            let state = sim.houses.get_mut(&house).unwrap();
            state.set_is_defeated_for_test(int("defeated") != 0);
            state.set_multiplay_passive_for_test(int("passive") != 0);
            state.tracking_mut().set_for_test(
                int("buildings"),
                &base_units,
                (
                    int("active_units"),
                    int("active_infantry"),
                    int("active_aircraft"),
                ),
                &[(yarefn, int("active_yarefn"))],
            );
        }
        let was_defeated = sim.houses[&house].is_defeated();

        sim.check_defeat(Some(&rules), None);

        let native_defeats = case.events == ["blowup_all", "mplayer_defeated"];
        assert!(
            native_defeats || case.events.is_empty(),
            "{name}: {:?}",
            case.events
        );
        assert_eq!(
            !was_defeated && sim.houses[&house].is_defeated(),
            native_defeats,
            "{name}"
        );
    }
}

/// Regression for the observed fourth-side opening: construction-to-destruction
/// MCV tracking, even in limbo, reaches the real House rung. Retail deliberately
/// considers only the first three BaseUnit slots; Ares considers the fourth too.
#[test]
fn fourth_base_unit_survival_reaches_the_production_defeat_gate() {
    let mut rules = rules(3);
    rules.general.base_unit_types.push("TANK".into());
    let (mut sim, houses) = sim_with_houses(&[0]);
    let owner = houses[0];
    sim.session.game_options_mut_for_test().short_game = true;
    let extra_mcv = sim
        .spawn_object_limbo_at_height("TANK", "house0", 10, 10, 0, 0, &rules)
        .unwrap();
    sim.check_defeat(Some(&rules), None);
    assert_eq!(
        sim.houses[&owner].is_defeated(),
        !crate::rules::ARES_ENABLED
    );
    if crate::rules::ARES_ENABLED {
        sim.uninit_with_rules(extra_mcv, &rules);
        sim.flush_pending_delete();
        sim.check_defeat(Some(&rules), None);
        assert!(sim.houses[&owner].is_defeated());
    }
}

/// An explicit false on a building must override Ares's building default in
/// both modes; the retail binary ignores the extension and keeps the building.
#[test]
fn explicit_keep_alive_false_reaches_both_production_mode_arms() {
    for short_game in [false, true] {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[BuildingTypes]\n0=MARKER\n[MARKER]\nStrength=100\nKeepAlive=no\n",
        ))
        .unwrap();
        let (mut sim, houses) = sim_with_houses(&[0]);
        sim.session.game_options_mut_for_test().short_game = short_game;
        sim.spawn_object_limbo_at_height("MARKER", "house0", 10, 10, 0, 0, &rules)
            .unwrap();
        sim.check_defeat(Some(&rules), None);
        assert_eq!(
            sim.houses[&houses[0]].is_defeated(),
            crate::rules::ARES_ENABLED
        );
    }
}

/// Rebuild only the native gate's supplied counter/object boundary, then use
/// the real World presence query and House rung. Parasite input is an allocated
/// class, not a victim/eater link; this is not a whole parasite lifecycle test.
#[cfg(feature = "ares")]
#[test]
fn ares_normal_defeat_gate_matches_executed_object_array_controls() {
    let corpus = crate::test_fixture::json("tools/spatial_oracle/ares_house_survival.json");
    for row in corpus["survival_cases"].as_array().unwrap() {
        let input = &row["input"];
        if input["short_game"] != false || input["keep_alive_count"] != 0 {
            continue;
        }
        let rules = rules(3);
        let (mut sim, houses) = sim_with_houses(&[0, 1]);
        sim.houses
            .get_mut(&houses[1])
            .unwrap()
            .set_multiplay_passive_for_test(true);
        sim.session.game_options_mut_for_test().short_game = false;
        for (class, kind) in [("unit", "TANK"), ("infantry", "CLEG"), ("aircraft", "AIR")] {
            for object in input["objects"][class].as_array().unwrap() {
                let owner = if object["owner"] == "ours" {
                    "house0"
                } else {
                    "house1"
                };
                let id = sim
                    .spawn_object_limbo_at_height(kind, owner, 10, 10, 0, 0, &rules)
                    .unwrap();
                let entity = sim.substrate.entities.get_mut(id).unwrap();
                entity
                    .lifecycle
                    .set_in_limbo_for_test(object["limbo"].as_bool().unwrap());
                if object["parasite"] == true {
                    entity.parasite.allocate_class(100);
                }
            }
        }
        let int = |key: &str| input[key].as_i64().unwrap_or(0) as i32;
        let refinery = sim.interner.intern("YAREFN");
        sim.houses
            .get_mut(&houses[0])
            .unwrap()
            .tracking_mut()
            .set_for_test(
                int("buildings"),
                &[],
                (
                    int("active_units"),
                    int("active_infantry"),
                    int("active_aircraft"),
                ),
                &[(refinery, int("active_yarefn"))],
            );
        sim.check_defeat(Some(&rules), None);
        assert_eq!(
            sim.houses[&houses[0]].is_defeated(),
            !row["events"].as_array().unwrap().is_empty(),
            "{}",
            input["name"]
        );
    }
}

#[derive(serde::Deserialize)]
struct NativeBlowupCase {
    input: serde_json::Value,
    events: Vec<serde_json::Value>,
    original_owners: std::collections::BTreeMap<String, Vec<String>>,
}

/// `HouseClass::Blowup_All @ 0x004FC6D0` against the original, case by case
/// (with GetOriginalOwner, the CaptureManager node scan and
/// SetOriginalOwnerToCivilian executed; the Civilian side lookup supplied):
/// VERA's own receiver calls, in order, against the native ones (target,
/// damage = its Health, distance, C4Warhead, no attacker, ignoreDefenses,
/// no escape, no source house); a captive of another house handed to the
/// Civilian-side house, or killed when none exists; our own captive spared;
/// a warped object released first. The trigger-held original owner
/// (`trigger_owned_elsewhere_dies`) has no VERA state and is skipped. VERA's
/// deferred deletion never shifts the array; the removal case shows native
/// re-reading the slot, so the call order is the same.
#[test]
fn native_blowup_subsets_corpus() {
    let cases: Vec<NativeBlowupCase> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/house_blowup_subsets.json",
    ))
    .unwrap();
    assert_eq!(cases.len(), 9);
    for case in cases {
        let input = &case.input;
        let name = input["name"].as_str().unwrap();
        let rules = rules(input["civilian_side"].as_u64().unwrap_or(99) as usize);
        let sides: Vec<_> = input["houses"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_u64().unwrap() as u8)
            .collect();
        let (mut sim, houses) = sim_with_houses(&sides);
        let objects = input["objects"].as_array().unwrap();
        let ids: Vec<_> = objects
            .iter()
            .enumerate()
            .map(|(n, object)| {
                let owner = houses[object["owner"].as_u64().unwrap() as usize];
                let type_id = match object["rtti"].as_u64().unwrap() {
                    6 => "NAREFN",
                    15 => "CLEG",
                    2 => "AIR",
                    1 if object["naval"].as_bool() == Some(true) => "SHIP",
                    1 => "TANK",
                    rtti => panic!("unknown native fixture RTTI{rtti}"),
                };
                let owner_name = sim.resolve(owner).to_owned();
                let id = sim
                    .spawn_object_at_height(type_id, &owner_name, 10 + n as u16, 10, 0, 0, &rules)
                    .expect("fixture constructor");
                let entity = sim.substrate.entities.get_mut(id).unwrap();
                entity
                    .health
                    .set_for_test(object["health"].as_i64().unwrap_or(100) as i32);
                //The native corpus also supplies byte81 directly. This checks
                //sweep admission, not the unrelated lifecycle producing this byte.
                entity
                    .lifecycle
                    .set_in_limbo_for_test(object["limbo"].as_bool().unwrap_or(false));
                id
            })
            .collect();
        if let Some(controllers) = input["controllers"].as_object() {
            for (controller, nodes) in controllers {
                let controller = ids[controller.parse::<usize>().unwrap()];
                let nodes: Vec<_> = nodes
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|node| {
                        (
                            ids[node[0].as_u64().unwrap() as usize],
                            houses[node[1].as_u64().unwrap() as usize],
                        )
                    })
                    .collect();
                for &(victim, _) in &nodes {
                    sim.substrate.entities.get_mut(victim).unwrap().mind_control =
                        MindControlLink::controlled_by_for_test(controller);
                }
                sim.substrate
                    .entities
                    .get_mut(controller)
                    .unwrap()
                    .capture_manager = Some(CaptureManagerState::with_nodes_for_test(&nodes));
            }
        }
        let subset = match input["subset"].as_str().unwrap() {
            "buildings" => super::HouseBlowupSubset::Buildings,
            "land" => super::HouseBlowupSubset::Land,
            "naval" => super::HouseBlowupSubset::Naval,
            _ => unreachable!(),
        };
        let _ = super::take_blowup_trace();
        sim.house_blowup(houses[0], subset, &rules, None, Default::default());
        let calls = super::take_blowup_trace();
        let native: Vec<_> = case
            .events
            .iter()
            .filter(|e| e[0] == "receive_damage")
            .collect();
        assert_eq!(calls.len(), native.len(), "{name}: count");
        let c4 = sim.interner.intern("Super");
        for (call, event) in calls.iter().zip(native) {
            let n = event[1]
                .as_str()
                .unwrap()
                .strip_prefix("obj")
                .unwrap()
                .parse::<usize>()
                .unwrap();
            assert_eq!(
                (
                    call.target_id,
                    call.damage,
                    call.distance_leptons,
                    call.warhead_ref,
                    call.attacker_id,
                    call.source_house,
                    call.receiver_flags
                ),
                (
                    ids[n],
                    event[2].as_i64().unwrap() as i32,
                    event[3].as_u64().unwrap() as i32,
                    c4,
                    RAD_NO_ATTACKER,
                    None,
                    ReceiverCallFlags {
                        ignore_defenses: true,
                        arg6: true
                    }
                ),
                "{name}: receiver"
            );
        }
    }
}

#[test]
fn native_blowup_all_corpus() {
    let cases: Vec<NativeBlowupCase> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/house_blowup_all.json",
    ))
    .unwrap();
    assert_eq!(cases.len(), 10);
    for case in cases {
        let input = &case.input;
        let name = input["name"].as_str().unwrap();
        if name == "trigger_owned_elsewhere_dies" {
            continue;
        }
        let sides: Vec<u8> = input["houses"]
            .as_array()
            .unwrap()
            .iter()
            .map(|side| side.as_u64().unwrap() as u8)
            .collect();
        let civilian_side = input["civilian_side"].as_u64().unwrap_or(99) as usize;
        let rules = rules(civilian_side);
        let (mut sim, houses) = sim_with_houses(&sides);
        let objects = input["objects"].as_array().unwrap();
        let ids: Vec<u64> = objects
            .iter()
            .enumerate()
            .map(|(n, object)| {
                let owner = houses[object["owner"].as_u64().unwrap() as usize];
                let owner_name = sim.interner.resolve(owner).to_string();
                let id = 100 + n as u64;
                let mut entity =
                    GameEntity::test_default(id, "TANK", &owner_name, 10 + n as u16, 10);
                entity.set_owner_for_test(owner);
                entity.set_type_ref_for_test(sim.interner.intern("TANK"));
                entity.set_category_for_test(EntityCategory::Unit);
                entity
                    .health
                    .set_for_test(object["health"].as_i64().unwrap_or(100) as i32);
                sim.substrate.entities.insert(entity);
                id
            })
            .collect();
        if let Some(controllers) = input["controllers"].as_object() {
            for (controller, nodes) in controllers {
                let controller = ids[controller.parse::<usize>().unwrap()];
                let nodes: Vec<(u64, InternedId)> = nodes
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|node| {
                        (
                            ids[node[0].as_u64().unwrap() as usize],
                            houses[node[1].as_u64().unwrap() as usize],
                        )
                    })
                    .collect();
                for &(victim, _) in &nodes {
                    sim.substrate.entities.get_mut(victim).unwrap().mind_control =
                        MindControlLink::controlled_by_for_test(controller);
                }
                sim.substrate
                    .entities
                    .get_mut(controller)
                    .unwrap()
                    .capture_manager = Some(CaptureManagerState::with_nodes_for_test(&nodes));
            }
        }
        let mut warpers = Vec::new();
        for (n, object) in objects.iter().enumerate() {
            if object["warped"].as_bool() == Some(true) {
                let cleg = sim
                    .spawn_object_at_height("CLEG", "house1", 2, 2 + n as u16, 0, 0, &rules)
                    .unwrap();
                sim.temporal_initiate_warp(
                    cleg,
                    Some(ids[n]),
                    &rules,
                    None,
                    crate::sim::world::FrameEffects::default(),
                );
                assert!(sim.substrate.entities.get(ids[n]).unwrap().is_warped_out());
                warpers.push((ids[n], cleg));
            }
        }
        let health_before: Vec<i32> = ids
            .iter()
            .map(|&id| sim.substrate.entities.get(id).unwrap().health.current())
            .collect();
        let blown = houses[input["house"].as_u64().unwrap() as usize];
        let c4 = sim.interner.intern("Super");
        let _ = super::take_blowup_trace();

        sim.house_blowup(
            blown,
            super::HouseBlowupSubset::All,
            &rules,
            None,
            crate::sim::world::FrameEffects::default(),
        );

        // Native: ReceiveDamage(&damage, distance, warhead, attacker,
        // ignoreDefenses, arg6, sourceHouse), in array order.
        let calls = super::take_blowup_trace();
        let native_calls: Vec<&serde_json::Value> = case
            .events
            .iter()
            .filter(|event| event[0] == "receive_damage")
            .collect();
        assert_eq!(calls.len(), native_calls.len(), "{name}: receiver calls");
        let mut native_victims = Vec::new();
        for (call, event) in calls.iter().zip(&native_calls) {
            let object = event[1].as_str().unwrap();
            let n = object["obj".len()..].parse::<usize>().unwrap();
            native_victims.push(n);
            assert_eq!(event[4], "c4", "{name}: the native warhead");
            assert_eq!(call.target_id, ids[n], "{name}: who, in order");
            assert_eq!(
                Some(i64::from(call.damage)),
                event[2].as_i64(),
                "{name}: obj{n}'s damage"
            );
            assert_eq!(
                Some(i64::from(call.distance_leptons)),
                event[3].as_i64(),
                "{name}: distance"
            );
            assert_eq!(call.warhead_ref, c4, "{name}: C4Warhead");
            assert_eq!(event[5].as_u64(), Some(0), "{name}: no native attacker");
            assert_eq!(call.attacker_id, RAD_NO_ATTACKER, "{name}: no attacker");
            assert_eq!(
                call.receiver_flags,
                ReceiverCallFlags {
                    ignore_defenses: event[6].as_u64() == Some(1),
                    arg6: event[7].as_u64() == Some(1),
                },
                "{name}: ignoreDefenses, no escape"
            );
            assert_eq!(event[8].as_u64(), Some(0), "{name}: no native source house");
            assert_eq!(call.source_house, None, "{name}: no source house");
        }
        for (n, &id) in ids.iter().enumerate() {
            let entity = sim.substrate.entities.get(id).unwrap();
            let hit = native_victims.contains(&n);
            assert_eq!(
                entity.health.current() == 0,
                hit || health_before[n] == 0,
                "{name}: obj{n} dies"
            );
            if hit {
                assert!(
                    entity.kill_record.killer().is_none(),
                    "{name}: no kill credit"
                );
            }
        }
        for (target, _) in &warpers {
            assert!(
                !sim.substrate.entities.get(*target).unwrap().is_warped_out(),
                "{name}: the chain let go"
            );
        }
        // The oracle stubs ReceiveDamage, so its nodes outlive the deaths; in
        // VERA a dead controller frees its captives and a dead captive's node
        // expires. Compare the nodes of the survivors.
        for (controller, native_owners) in &case.original_owners {
            let n = controller.parse::<usize>().unwrap();
            if native_victims.contains(&n) {
                continue;
            }
            let nodes = input["controllers"][controller].as_array().unwrap();
            let manager = sim
                .substrate
                .entities
                .get(ids[n])
                .and_then(|entity| entity.capture_manager.as_ref())
                .expect("a living controller keeps its manager");
            for (node, native_owner) in nodes.iter().zip(native_owners) {
                let victim = node[0].as_u64().unwrap() as usize;
                if native_victims.contains(&victim) {
                    continue;
                }
                let owner = manager.original_owner(ids[victim]).unwrap();
                assert_eq!(
                    sim.interner.resolve(owner),
                    native_owner,
                    "{name}: obj{victim}'s original owner"
                );
            }
        }
    }
}

/// MPlayer_Defeated (`0x004FC0BC..0x004FC10D`): a computer house at
/// `MaxIQLevels`, defeated under `[AI] Paranoid`, turns the computers left
/// paranoid. Computer_Paranoid skips the defeated house.
#[test]
fn a_computer_defeat_turns_the_computers_left_paranoid() {
    let mut rules = rules(3);
    rules.general.allies_allowed = true;
    let (mut sim, houses) = sim_with_houses(&[0, 1, 1, 1]);
    sim.session.game_options_mut_for_test().short_game = true;
    sim.houses
        .get_mut(&houses[0])
        .unwrap()
        .set_is_human_for_test(true);
    for computer in &houses[1..] {
        sim.houses
            .get_mut(computer)
            .unwrap()
            .set_current_iq_for_test(rules.general.max_iq_levels);
    }
    // Every house but house3 holds a counted building.
    for (n, refinery) in ["GAREFN", "NAREFN", "NAREFN"].into_iter().enumerate() {
        sim.spawn_object_at_height(
            refinery,
            &format!("house{n}"),
            10 + 10 * n as u16,
            10,
            0,
            0,
            &rules,
        )
        .unwrap();
    }
    for _ in 0..3 {
        if sim.houses[&houses[3]].is_defeated() {
            break;
        }
        sim.advance_tick(&[], Some(&rules), None, None, 67);
    }
    assert!(sim.houses[&houses[3]].is_defeated());
    let allies = |n: usize| {
        sim.house_alliances()
            .get(&format!("HOUSE{n}"))
            .cloned()
            .unwrap_or_default()
    };
    assert_eq!(
        allies(1),
        std::collections::BTreeSet::from(["HOUSE2".to_string()])
    );
    assert_eq!(
        allies(2),
        std::collections::BTreeSet::from(["HOUSE1".to_string()])
    );
    assert!(allies(0).is_empty() && allies(3).is_empty());
    assert_eq!(sim.houses[&houses[1]].enemy_house(), Some(houses[0]));
}

/// Two houses on a bare 40x40 plane in a short game, `house0` the current
/// house; each owns one counted building.
fn two_house_plane(rules: &RuleSet) -> (Simulation, Vec<InternedId>, [u64; 2]) {
    let (mut sim, houses) = sim_with_houses(&[0, 1]);
    sim.fog.set_width_for_test(40);
    sim.fog.set_height_for_test(40);
    sim.session.game_options_mut_for_test().short_game = true;
    sim.session.set_current_house_for_test(Some(houses[0]));
    let own = sim
        .spawn_object_at_height("GAREFN", "house0", 10, 10, 0, 0, rules)
        .unwrap();
    let enemy = sim
        .spawn_object_at_height("NAREFN", "house1", 30, 30, 0, 0, rules)
        .unwrap();
    (sim, houses, [own, enemy])
}

const CORNERS: [(u16, u16); 4] = [(0, 0), (39, 0), (0, 39), (39, 39)];

/// MPlayer_Defeated's local branch through the production frame: the current
/// house's defeat explores its whole plane (RevealEntireMap), writes its
/// map-clear byte and switches its radar on, which no later House update
/// rechecks (`0x004F84FB`).
#[test]
fn a_local_defeat_reveals_the_whole_map_and_keeps_the_radar_on() {
    let rules = rules(3);
    let (mut sim, houses, [own, _]) = two_house_plane(&rules);
    let step = |sim: &mut Simulation| {
        sim.advance_tick(&[], Some(&rules), None, None, 67);
    };
    step(&mut sim);
    for (x, y) in CORNERS {
        assert!(!sim.fog.is_cell_revealed(houses[0], x, y));
    }
    assert!(!crate::sim::radar::has_radar_for_owner(
        &sim, &rules, "house0"
    ));

    sim.uninit_with_rules(own, &rules);
    step(&mut sim);
    step(&mut sim);
    let local = &sim.houses[&houses[0]];
    assert!(local.is_defeated() && local.map_is_clear());
    assert!(sim.local_player_defeated());
    assert!(sim.fog.whole_map_revealed_for_owner(houses[0]));
    for (x, y) in CORNERS {
        assert!(sim.fog.is_cell_revealed(houses[0], x, y), "{x},{y}");
        assert!(sim.fog.is_ground_unshrouded(houses[0], x, y), "{x},{y}");
        assert!(!sim.fog.is_cell_revealed(houses[1], x, y), "{x},{y}");
    }
    assert!(!sim.fog.whole_map_revealed_for_owner(houses[1]));
    assert!(crate::sim::radar::has_radar_for_owner(
        &sim, &rules, "house0"
    ));
    sim.power_states
        .get_mut(&houses[0])
        .unwrap()
        .recheck_radar();
    step(&mut sim);
    assert!(
        crate::sim::radar::has_radar_for_owner(&sim, &rules, "house0"),
        "no House rechecks radar after the local defeat"
    );
}

/// RevealEntireMap's guards (`0x00577F30..0x00577FAD`): NULL reveals to the
/// current house without a map-clear byte; once House+240 is set a second
/// call only writes the named house's byte; another house gets its byte and
/// nothing else.
#[test]
fn reveal_entire_map_reveals_once_and_only_to_the_current_house() {
    let (mut sim, houses) = sim_with_houses(&[0, 1]);
    sim.fog.set_width_for_test(8);
    sim.fog.set_height_for_test(8);
    let context = crate::sim::world::UninitContext::default();
    sim.reveal_entire_map(None, context);
    assert!(
        !sim.fog.whole_map_revealed_for_owner(houses[0]),
        "no current house, no PlayerPtr"
    );

    sim.session.set_current_house_for_test(Some(houses[0]));
    sim.reveal_entire_map(None, context);
    assert!(!sim.houses[&houses[0]].map_is_clear());
    assert!(sim.fog.whole_map_revealed_for_owner(houses[0]));
    assert!(sim.fog.is_cell_revealed(houses[0], 7, 7));
    let plane = save_codec::serialize(&sim.fog.by_owner()[&houses[0]]).unwrap();

    sim.reveal_entire_map(Some(houses[0]), context);
    assert!(sim.houses[&houses[0]].map_is_clear());
    assert_eq!(
        save_codec::serialize(&sim.fog.by_owner()[&houses[0]]).unwrap(),
        plane
    );

    sim.reveal_entire_map(Some(houses[1]), context);
    assert!(sim.houses[&houses[1]].map_is_clear());
    assert!(!sim.fog.whole_map_revealed_for_owner(houses[1]));
    assert!(!sim.fog.is_cell_revealed(houses[1], 7, 7));
}

/// The Civilian-side house is asked of each house's HouseType side
/// (`[house+0x34]+0xBC` against `Find_Index("Civilian")`, every `0x006A46D0`
/// caller). A map country outside `[Sides]` keeps the constructor's -1 even
/// when the scenario also supplies a declared Civilian side name, so it is
/// passed over for the Neutral house. With
/// no `Civilian` side, Find_Index's -1 matches that sideless type instead.
#[test]
fn the_civilian_house_is_asked_of_its_house_type() {
    for (civilian_side, expected) in [("Civilian", 1), ("Other", 0)] {
        let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[Countries]\n0=Americans\n1=MapCountry\n2=Neutral\n\
             [Americans]\n[MapCountry]\n[Neutral]\n\
             [Sides]\nAllied=Americans\n{civilian_side}=Neutral\n"
        )))
        .expect("civilian house rules");
        let mut sim = Simulation::with_seed(5);
        let houses: Vec<InternedId> = [("MapHouse", "MapCountry"), ("Neutral", "Neutral")]
            .into_iter()
            .map(|(name, country)| {
                let side = crate::sim::house_state::resolve_house_side_index(
                    &rules,
                    Some(country),
                    Some(civilian_side),
                    0,
                );
                let id = sim.interner.intern(name);
                let country = sim.interner.intern(country);
                sim.houses
                    .register(HouseState::new(id, side, Some(country), false, 0, 10));
                id
            })
            .collect();
        assert_eq!(
            sim.houses[&houses[0]].side_index(),
            -1,
            "a registered sideless Country must not adopt the declared fallback"
        );
        assert_eq!(sim.houses[&houses[1]].side_index(), 1);
        assert_eq!(
            sim.civilian_side_house(&rules),
            Some(houses[expected]),
            "{civilian_side}"
        );
    }
}
