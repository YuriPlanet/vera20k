//! Executed original Super timer/flag controls, not a second charge model.
//!
//! `trigger_action_values.py --super-charge` runs 6CC1E0 and the
//! concrete constructor/Grant/Suspend/AI histories on the identified retail
//! executable. The retained corpus bounds the instance state and three RNG
//! streams; its cameo stage and native timer-vector allocation are presentation
//! observations, not simulation state added by these tests.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::house_state::HouseState;
use crate::sim::rng::SimRng;
use serde_json::Value;

fn corpus() -> Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/trigger_super_charge_percent.json",
    ))
    .unwrap()
}

fn stock_rules() -> RuleSet {
    // Scalar fixture inputs are the original type reader's selected stock
    // values; full retail layers and ALL01 dispatch are tested by that owner.
    RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
         [BuildingTypes]\n0=YAPPET\n\
         [YAPPET]\nStrength=1000\nPower=-1000\n\
         SuperWeapon=PsychicDominatorSpecial\n\
         [SuperWeaponTypes]\n0=PsychicDominatorSpecial\n\
         [PsychicDominatorSpecial]\nType=PsychicDominator\n\
         RechargeTime=10\nIsPowered=yes\nShowTimer=yes\n",
    ))
    .unwrap()
}

fn int(row: &Value, key: &str) -> i32 {
    row[key].as_i64().unwrap().try_into().unwrap()
}

fn assert_state(instance: &SuperWeaponInstance, expected: &Value, label: &str) {
    assert_eq!(
        instance.charge_start_tick,
        int(expected, "start"),
        "{label}: start"
    );
    assert_eq!(
        instance.charge_duration,
        int(expected, "duration"),
        "{label}: duration"
    );
    assert_eq!(
        instance.is_active,
        expected["granted"].as_bool().unwrap(),
        "{label}: grant"
    );
    assert_eq!(
        instance.is_ready,
        expected["ready"].as_bool().unwrap(),
        "{label}: ready"
    );
    assert_eq!(
        instance.is_suspended,
        expected["held"].as_bool().unwrap(),
        "{label}: hold"
    );
}

#[test]
fn percentage_setter_matches_original_signed_timer_and_prior_flags() {
    let data = corpus();
    let rules = stock_rules();
    let mut sw = rules
        .super_weapon("PsychicDominatorSpecial")
        .unwrap()
        .clone();
    let id = InternedId::from_index(1);
    for row in data["setter_controls"].as_array().unwrap() {
        let input = &row["input"];
        // Action128's override storage/getter is a separate mechanism. These
        // controls compare the type-time route that this owner implements.
        if int(input, "custom_recharge") != -1 {
            continue;
        }
        sw.recharge_time_frames = int(input, "recharge");
        let mut instance = SuperWeaponInstance::new(id, id, 0);
        instance.charge_start_tick = int(input, "start");
        instance.charge_duration = int(input, "duration");
        instance.is_active = input["granted"].as_bool().unwrap();
        instance.is_ready = input["ready"].as_bool().unwrap();
        instance.is_suspended = input["held"].as_bool().unwrap();
        instance.set_charge_percentage(int(input, "percentage"), &sw, int(input, "frame") as u32);
        assert_state(&instance, &row["output"], row["name"].as_str().unwrap());
        assert_eq!(
            row["rng_before"], row["rng_after"],
            "original setter has no RNG draws"
        );
        assert!(row["rng_calls"].as_array().unwrap().is_empty());
    }
}

#[test]
fn grant_percentage_hold_release_and_ai_match_original_histories() {
    let data = corpus();
    let rules = stock_rules();
    let sw = rules.super_weapon("PsychicDominatorSpecial").unwrap();
    for sequence in data["sequence_controls"].as_array().unwrap() {
        let grant_frame = int(sequence, "grant_frame");
        let low_power = sequence["input_low_power_at_grant"].as_bool().unwrap();
        let mut sim = Simulation::with_seed(0);
        sim.session.binary_frame = grant_frame as u32;
        let owner = sim.interner.intern("Confederation");
        let type_id = sim.interner.intern("PsychicDominatorSpecial");
        sim.houses
            .insert(owner, HouseState::new(owner, 2, None, false, 0, 10));
        sim.super_weapons.entry(owner).or_default().insert(
            type_id,
            SuperWeaponInstance::new(type_id, owner, grant_frame),
        );
        sim.spawn_object("YAPPET", "Confederation", 10, 10, 0, &rules)
            .unwrap();
        sim.power_states.entry(owner).or_default().is_low_power = low_power;
        // Admit the harness's incoming stream states after the supplied
        // Building fixture; construction draws are outside this history.
        let seeds = &data["rng_seed_states"];
        sim.main_rng =
            SimRng::from_native_state_hex_for_test(seeds["main"].as_str().unwrap()).into();
        sim.scenario_rng =
            SimRng::from_native_state_hex_for_test(seeds["scenario"].as_str().unwrap());
        sim.mapgen_rng = SimRng::from_native_state_hex_for_test(seeds["mapgen"].as_str().unwrap());
        let rng_before = (
            sim.main_rng.native_state_hex(),
            sim.scenario_rng.native_state_hex(),
            sim.mapgen_rng.native_state_hex(),
        );
        for row in sequence["rows"].as_array().unwrap() {
            sim.session.binary_frame = int(row, "frame") as u32;
            let stage = row["stage"].as_str().unwrap();
            match stage {
                "concrete_constructor" => {}
                "grant" => grant_provided_supers(&mut sim, &rules, owner),
                "execute129" => {
                    let changed = sim
                        .super_weapons
                        .get_mut(&owner)
                        .unwrap()
                        .get_mut(&type_id)
                        .unwrap()
                        .set_charge_percentage(90, sw, sim.session.binary_frame);
                    assert_eq!(i32::from(changed), int(row, "returned"));
                }
                "first_hold" | "repeat_hold" | "release_hold" => {
                    let changed = sim
                        .super_weapons
                        .get_mut(&owner)
                        .unwrap()
                        .get_mut(&type_id)
                        .unwrap()
                        .suspend(
                            stage != "release_hold",
                            sw.manual_control,
                            sim.session.binary_frame,
                        );
                    assert_eq!(i32::from(changed), int(row, "returned"));
                }
                "held_ai" | "released_ai" | "elapsed_ai" => {
                    tick_house_superweapon_instances(&mut sim, &rules, owner);
                }
                _ => panic!("unhandled original history stage {stage}"),
            }
            let instance = &sim.super_weapons[&owner][&type_id];
            let label = format!("frame{grant_frame}/low{low_power}/{stage}");
            assert_state(instance, &row["state"], &label);
            assert_eq!(
                instance.ready_tick,
                int(row, "ready_tick"),
                "{label}: ready frame"
            );
            assert_eq!(
                instance.timer_place.is_some(),
                row["own_timer_registered"].as_bool().unwrap(),
                "{label}: timer list"
            );
            assert_eq!(
                super_timer_views(&sim, &rules).len(),
                row["granted_timer_count"].as_u64().unwrap() as usize,
                "{label}: timer-list count",
            );
        }
        assert_eq!(sequence["rng_before"], sequence["rng_after"]);
        assert!(sequence["rng_calls"].as_array().unwrap().is_empty());
        assert_eq!(
            (
                sim.main_rng.native_state_hex(),
                sim.scenario_rng.native_state_hex(),
                sim.mapgen_rng.native_state_hex()
            ),
            rng_before
        );
    }
}

#[test]
fn house_hold_pass_matches_original_retained_power_histories() {
    let data = corpus();
    let rules = stock_rules();
    for sequence in data["house_refresh_controls"].as_array().unwrap() {
        let mut sim = Simulation::with_seed(0);
        let owner = sim.interner.intern("Confederation");
        let type_id = sim.interner.intern("PsychicDominatorSpecial");
        sim.houses
            .insert(owner, HouseState::new(owner, 2, None, false, 0, 10));
        sim.spawn_object("YAPPET", "Confederation", 10, 10, 0, &rules)
            .unwrap();
        let first = &sequence["rows"][0]["input"];
        let mut instance = SuperWeaponInstance::new(type_id, owner, 0);
        instance.charge_start_tick = int(first, "start");
        instance.charge_duration = int(first, "duration");
        instance.is_active = first["granted"].as_bool().unwrap();
        instance.is_ready = first["ready"].as_bool().unwrap();
        instance.is_suspended = first["held"].as_bool().unwrap();
        sim.super_weapons
            .entry(owner)
            .or_default()
            .insert(type_id, instance);
        let seeds = &data["rng_seed_states"];
        sim.main_rng =
            SimRng::from_native_state_hex_for_test(seeds["main"].as_str().unwrap()).into();
        sim.scenario_rng =
            SimRng::from_native_state_hex_for_test(seeds["scenario"].as_str().unwrap());
        sim.mapgen_rng = SimRng::from_native_state_hex_for_test(seeds["mapgen"].as_str().unwrap());
        let rng_before = (
            sim.main_rng.native_state_hex(),
            sim.scenario_rng.native_state_hex(),
            sim.mapgen_rng.native_state_hex(),
        );
        for row in sequence["rows"].as_array().unwrap() {
            let label = format!(
                "power-drop-frame{}/low-grant{}/frame{}",
                sequence["power_drop_frame"], sequence["input_low_power_at_grant"], row["frame"]
            );
            assert_state(&sim.super_weapons[&owner][&type_id], &row["input"], &label);
            sim.session.binary_frame = int(row, "frame") as u32;
            // The original pass reads retained House output/drain. Its
            // low branch is observed in native control flow, not recomputed
            // by a second ratio implementation in this comparison fixture.
            let power = sim.power_states.entry(owner).or_default();
            power.total_output = int(row, "retained_output");
            power.total_drain = int(row, "retained_drain");
            power.is_low_power = row["low_power_before_call"].as_bool().unwrap();
            sim.houses.get_mut(&owner).unwrap().take_tech_recheck();
            update_owned_supers(&mut sim, &rules, owner);
            assert_state(&sim.super_weapons[&owner][&type_id], &row["output"], &label);
            assert_eq!(
                sim.houses.get_mut(&owner).unwrap().take_tech_recheck(),
                row["house_sidebar_dirty"].as_bool().unwrap(),
                "{label}: House tail request"
            );
        }
        assert_eq!(sequence["rng_before"], sequence["rng_after"]);
        assert!(sequence["rng_calls"].as_array().unwrap().is_empty());
        assert_eq!(
            (
                sim.main_rng.native_state_hex(),
                sim.scenario_rng.native_state_hex(),
                sim.mapgen_rng.native_state_hex()
            ),
            rng_before
        );
    }
}

#[test]
fn pending_house_tech_tree_tail_is_hashed_and_saved() {
    let mut sim = Simulation::with_seed(0);
    let owner = sim.interner.intern("Confederation");
    sim.houses
        .insert(owner, HouseState::new(owner, 2, None, false, 0, 10));
    assert!(sim.houses.get_mut(&owner).unwrap().take_tech_recheck());
    let clean_hash = sim.state_hash();
    sim.request_house_tech_tree_recheck(owner);
    let dirty_hash = sim.state_hash();
    assert_ne!(dirty_hash, clean_hash);
    let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "pending-super-recheck", 0);
    let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    assert_eq!(restored.state_hash(), dirty_hash);
    assert!(restored.houses.get_mut(&owner).unwrap().take_tech_recheck());
    assert!(!restored.houses.get_mut(&owner).unwrap().take_tech_recheck());
    assert_eq!(restored.state_hash(), clean_hash);
}

#[test]
fn house_power_edge_holds_after_reactor_sample_in_the_same_frame() {
    let native: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/trigger_damage_at_waypoint.json",
    ))
    .unwrap();
    let receiver = &native["receiver_controls"][0];
    let health_before = int(&receiver["authored_health_native"]["output"], "actual");
    let health_after = int(
        receiver["rows"].as_array().unwrap().last().unwrap(),
        "health",
    );
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
         [BuildingTypes]\n0=NANRCT\n1=YAPPET\n\
         [NANRCT]\nStrength=1000\nPower=2000\n\
         [YAPPET]\nStrength=1000\nPower=-1000\n\
         SuperWeapon=PsychicDominatorSpecial\n\
         [SuperWeaponTypes]\n0=PsychicDominatorSpecial\n\
         [PsychicDominatorSpecial]\nType=PsychicDominator\n\
         RechargeTime=10\nIsPowered=yes\nShowTimer=yes\n",
    ))
    .unwrap();
    let mut sim = Simulation::with_seed(0);
    sim.session.game_mode_nonzero = false;
    let owner = sim.interner.intern("Confederation");
    let type_id = sim.interner.intern("PsychicDominatorSpecial");
    sim.houses
        .insert(owner, HouseState::new(owner, 2, None, false, 0, 10));
    sim.session.house_order.push(owner);
    sim.super_weapons
        .entry(owner)
        .or_default()
        .insert(type_id, SuperWeaponInstance::new(type_id, owner, 0));
    let reactor = sim
        .spawn_object("NANRCT", "Confederation", 10, 10, 0, &rules)
        .unwrap();
    sim.spawn_object("YAPPET", "Confederation", 20, 20, 0, &rules)
        .unwrap();
    sim.substrate
        .entities
        .get_mut(reactor)
        .unwrap()
        .health
        .set_for_test(health_before);
    sim.advance_tick(&[], Some(&rules), None, None, 67);
    assert_eq!(
        sim.power_states[&owner].total_output,
        int(&receiver["power_consumer"], "initial_power")
    );
    assert_eq!(sim.power_states[&owner].total_drain, 1000);
    assert!(sim.super_weapons[&owner][&type_id].is_active);
    assert!(!sim.super_weapons[&owner][&type_id].is_suspended);

    sim.session.binary_frame = int(receiver, "frame") as u32;
    let sw = rules.super_weapon("PsychicDominatorSpecial").unwrap();
    sim.super_weapons
        .get_mut(&owner)
        .unwrap()
        .get_mut(&type_id)
        .unwrap()
        .set_charge_percentage(90, sw, sim.session.binary_frame);
    // The Action63 receiver is tested by its owner. Here its executed final
    // health is an admitted input to the live Building sample -> House rung.
    sim.substrate
        .entities
        .get_mut(reactor)
        .unwrap()
        .health
        .set_for_test(health_after);
    assert_eq!(
        sim.power_states[&owner].total_output,
        int(&receiver["power_consumer"]["before_sample"], "house_power")
    );
    sim.advance_tick(&[], Some(&rules), None, None, 67);
    assert_eq!(
        sim.power_states[&owner].total_output,
        int(&receiver["power_consumer"]["after_power"], "house_power")
    );
    assert!(sim.power_states[&owner].is_low_power);
    let instance = &sim.super_weapons[&owner][&type_id];
    let charge_data = corpus();
    let charged_then_held = &charge_data["sequence_controls"][0]["rows"][3]["state"];
    assert_state(instance, charged_then_held, "same-frame power edge");
    sim.advance_tick(&[], Some(&rules), None, None, 67);
    assert_state(
        &sim.super_weapons[&owner][&type_id],
        charged_then_held,
        "repeat low-power frame",
    );
}
