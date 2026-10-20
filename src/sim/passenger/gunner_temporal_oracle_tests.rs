//! Original Unit746420/7464E0 callbacks with real GetROF6FCFA0 and LetGo71ABC0.
//!
//! `ifv_turret_switching --temporal-gunner` supplies the consumed native rows.
//! Actor placement, retained timers/bursts and one already-live Temporal chain
//! are explicit component inputs. These are not boarding/Unlimbo comparisons;
//! the parent regressions exercise those ordinary production paths separately.
//! One original FireAt rearm-tail output is imported as a boundary, never
//! reproduced here or counted as a Rust FireAt comparison. Native timer+2F0,
//! charge delay+2F8 and unmodeled manager helper fields are outside this state
//! projection; their native writes remain in the corpus.

use super::super::{receive_gunner, remove_gunner};
use super::{arena, entity};
use crate::rules::object_type::Ability;
use crate::rules::ruleset::RuleSet;
use crate::sim::house_state::HouseDifficulty;
use crate::sim::rng::{SimRng, trace_draws};
use crate::sim::timer::CdTimer;
use crate::sim::world::Simulation;
use serde_json::{Value, json};

fn corpus() -> &'static Value {
    crate::test_fixture::json("tools/spatial_oracle/ifv_temporal_gunner.json")
}

fn signed(value: &Value) -> i32 {
    i32::try_from(value.as_i64().expect("native signed dword")).unwrap()
}

fn bits(value: &Value) -> u64 {
    u64::from_str_radix(value.as_str().expect("native hex integer"), 16).unwrap()
}

fn bytes(value: &Value) -> Vec<u8> {
    let hex = value.as_str().expect("native memory hex");
    assert!(hex.len().is_multiple_of(2));
    hex.as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

fn assert_stock_inputs(rules: &RuleSet, native: &Value) {
    let physical = &native["inputs"]["final"];
    for name in ["FV", "CLEG"] {
        let object = rules.object(name).expect("retail gunner type");
        let expected = &physical["types"][name];
        assert_eq!(object.ifv_mode, signed(&expected["ifv_mode"]), "{name}");
        assert_eq!(
            object.gunner,
            expected["gunner"].as_bool().unwrap(),
            "{name}"
        );
        assert_eq!(object.passengers, signed(&expected["passengers"]), "{name}");
        assert_eq!(
            object.weapon_count,
            signed(&expected["weapon_count"]),
            "{name}"
        );
        assert_eq!(
            object.turret_count,
            signed(&expected["turret_count"]),
            "{name}"
        );
        assert_eq!(
            object.veteran_abilities.has(Ability::Rof),
            expected["veteran_rof"].as_bool().unwrap(),
            "{name} veteran ROF ability"
        );
        assert_eq!(
            object.elite_abilities.has(Ability::Rof),
            expected["elite_rof"].as_bool().unwrap(),
            "{name} elite ROF ability"
        );
        for key in ["weapons", "elite_weapons"] {
            let slots = expected[key].as_array().unwrap();
            assert_eq!(slots.len(), 17, "{name} physical {key} prefix");
            let actual: Vec<_> = (0..slots.len())
                .map(|index| match key {
                    "weapons" => object.weapon_at(index),
                    _ => object.elite_weapon_at(index),
                })
                .collect();
            assert_eq!(
                serde_json::to_value(actual).unwrap(),
                expected[key],
                "{name} {key}"
            );
        }
        let turrets = expected["turrets"].as_array().unwrap();
        assert_eq!(turrets.len(), 18, "{name} physical turret mapping");
        for (index, expected) in turrets.iter().enumerate() {
            assert_eq!(
                object.gunner_turrets.turret_for_weapon(index as i32),
                signed(expected),
                "{name} turret slot{index}"
            );
        }
        if name == "FV" {
            assert_eq!(
                serde_json::to_value(object.burst_delays).unwrap(),
                expected["burst_delays"]
            );
        }
    }
    for name in ["CRNuke", "CRNeutronRifle", "NeutronRifle", "NeutronRifleE"] {
        let weapon = rules.weapon(name).expect("retail gunner weapon");
        let expected = &physical["weapons"][name];
        assert_eq!(weapon.rof, signed(&expected["rof"]), "{name} ROF");
        assert_eq!(weapon.burst, signed(&expected["burst"]), "{name} Burst");
        assert_eq!(
            weapon.warhead.as_deref(),
            expected["warhead"].as_str(),
            "{name} warhead"
        );
        let warhead = rules.warhead(weapon.warhead.as_deref().unwrap()).unwrap();
        assert_eq!(
            warhead.temporal,
            expected["temporal"].as_bool().unwrap(),
            "{name} Temporal"
        );
        for (key, actual) in [
            ("sonic", weapon.is_sonic),
            ("spark", weapon.use_spark_particles),
            ("fire", weapon.use_fire_particles),
            ("railgun", weapon.is_railgun),
        ] {
            assert_eq!(
                actual,
                expected["flags"][key].as_bool().unwrap(),
                "{name} {key}"
            );
        }
    }
    assert_eq!(
        rules.general.veteran_rof.to_bits(),
        bits(&physical["rules"]["veteran_rof_bits"])
    );
    assert_eq!(
        u64::from(rules.garrison_rules.occupy_rof_multiplier.to_bits()),
        bits(&physical["rules"]["occupy_rof_bits"])
    );
    assert_eq!(
        u64::from(rules.garrison_rules.bunker_rof_multiplier.to_bits()),
        bits(&physical["rules"]["bunker_rof_bits"])
    );
    let difficulty_rows = physical["rules"]["difficulty_rows"].as_array().unwrap();
    assert_eq!(difficulty_rows.len(), rules.general.difficulty_rows.len());
    for (row, expected) in rules.general.difficulty_rows.iter().zip(difficulty_rows) {
        let raw = bytes(expected);
        assert_eq!(raw.len(), 0x50);
        // ReadDifficulty66D270 stores these nine doubles in this order. This
        // decodes retained bytes; it does not calculate an expected multiplier.
        for (index, actual) in [
            row.firepower,
            row.ground_speed,
            row.air_speed,
            row.armor,
            row.rof,
            row.cost,
            row.build_time,
            row.repair_delay,
            row.build_delay,
        ]
        .into_iter()
        .enumerate()
        {
            let offset = index * 8;
            assert_eq!(
                actual.to_bits(),
                u64::from_le_bytes(raw[offset..offset + 8].try_into().unwrap())
            );
        }
    }
    for name in ["Americans", "Russians"] {
        let expected = &physical["countries"][name];
        assert_eq!(
            i32::from(rules.country_index(name).unwrap().0),
            signed(&expected["native_index"])
        );
        assert_eq!(
            rules.country_difficulty_biases(name).rof.bits(),
            bits(&expected["rof_bits"])
        );
    }
}

#[test]
fn native_temporal_gunner_inputs_match_retail_battle_readers() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    assert_stock_inputs(&retail.rules, corpus());
}

/// IDs stand for native object roles; the separate retained Temporal object
/// is represented by whichever actor owns its link, as in temporal.rs.
struct Actors {
    transport: u64,
    passenger: u64,
    victim: u64,
}

impl Actors {
    fn id(&self, role: &str) -> u64 {
        match role {
            "transport" => self.transport,
            "passenger" => self.passenger,
            _ => panic!("unknown native gunner role {role}"),
        }
    }
}

fn install_actor_boundary(sim: &mut Simulation, id: u64, native: &Value) {
    let actor = sim.substrate.entities.get_mut(id).unwrap();
    // The declared initial burst, including signed overflow controls, is a
    // native saved-state boundary. Do not simulate extra shots to obtain it.
    actor.weapon_burst = serde_json::from_value(json!({
        "index": signed(&native["burst_index"]), "radial_index": 0
    }))
    .unwrap();
    actor.rearm_timer.set_for_test(CdTimer::from_raw(
        signed(&native["rearm"][0]),
        signed(&native["rearm"][1]),
    ));
}

fn install_temporal_boundary(sim: &mut Simulation, actors: &Actors, native: &Value) {
    let owner = native["link"]["owner"].as_str();
    let target = native["link"]["target"]
        .as_bool()
        .unwrap()
        .then_some(actors.victim);
    for role in ["transport", "passenger"] {
        let link = (owner == Some(role)).then(|| {
            json!({
                "target": target, "prev": null, "next": null,
                "warp_remaining": signed(&native["link"]["fields"]["0x48"])
            })
        });
        // Use the existing serialized logical state rather than exposing the
        // Temporal owner's private links or adding fixture mutation APIs.
        sim.substrate
            .entities
            .get_mut(actors.id(role))
            .unwrap()
            .temporal = serde_json::from_value(json!({"link": link, "head": null})).unwrap();
    }
    let head = native["link"]["victim_head"]
        .as_bool()
        .unwrap()
        .then(|| actors.id(owner.expect("native head has a link owner")));
    sim.substrate
        .entities
        .get_mut(actors.victim)
        .unwrap()
        .temporal = serde_json::from_value(json!({"link": null, "head": head})).unwrap();
}

fn assert_state(sim: &Simulation, actors: &Actors, native: &Value, label: &str) {
    let owner = native["link"]["owner"].as_str();
    for role in ["transport", "passenger"] {
        let actor = entity(sim, actors.id(role));
        let expected = &native[role];
        assert_eq!(
            actor.weapon_burst.index(),
            signed(&expected["burst_index"]),
            "{label} {role} burst"
        );
        assert_eq!(
            [
                actor.rearm_timer.start_frame(),
                actor.rearm_timer.duration()
            ],
            [signed(&expected["rearm"][0]), signed(&expected["rearm"][1])],
            "{label} {role} rearm"
        );
        assert_eq!(
            actor.temporal.has_link(),
            expected["temporal"].as_bool().unwrap(),
            "{label} {role} manager"
        );
        let target = (owner == Some(role) && native["link"]["target"].as_bool().unwrap())
            .then_some(actors.victim);
        assert_eq!(
            actor.temporal.warp_target(),
            target,
            "{label} {role} target"
        );
        assert_eq!(
            actor.temporal.chain_head(),
            None,
            "{label} source is not a victim"
        );
        if owner == Some(role) {
            let serialized = serde_json::to_value(&actor.temporal).unwrap();
            assert!(serialized["link"]["prev"].is_null());
            assert!(serialized["link"]["next"].is_null());
            assert_eq!(
                signed(&serialized["link"]["warp_remaining"]),
                signed(&native["link"]["fields"]["0x48"]),
                "{label} retained warp progress"
            );
        }
    }
    let transport = entity(sim, actors.transport);
    assert_eq!(
        (
            transport.current_weapon_number(),
            transport.current_turret_index()
        ),
        (
            signed(&native["transport"]["weapon_index"]),
            signed(&native["transport"]["turret_index"])
        ),
        "{label} selected weapon/turret"
    );
    let victim = entity(sim, actors.victim);
    let expected_head = native["link"]["victim_head"]
        .as_bool()
        .unwrap()
        .then(|| actors.id(owner.expect("native victim head has an owner")));
    assert_eq!(
        victim.temporal.chain_head(),
        expected_head,
        "{label} victim head owner"
    );
    assert_eq!(
        victim.temporal.is_warped(),
        native["link"]["victim_warped"].as_bool().unwrap(),
        "{label} warped flag"
    );
}

fn prepare_case(rules: &RuleSet, row: &Value) -> (Simulation, Actors) {
    let input = &row["input"];
    let frame = signed(&input["frame"]);
    let (mut sim, _) = arena(rules);
    for (index, role) in ["transport", "passenger"].into_iter().enumerate() {
        let house = &row["houses"][role];
        let country = house["country"].as_str().unwrap();
        let id = sim.interner.intern(country);
        sim.houses.get_mut(&id).unwrap().set_difficulty(
            HouseDifficulty::from_native(signed(&house["difficulty"])).unwrap(),
            &rules.general,
            rules.country_difficulty_biases(country),
            signed(&house["mode"]) != 0,
            index as i32,
            frame,
        );
        assert_eq!(
            sim.houses[&id].rof_bias().bits(),
            bits(&house["rof_bits"]),
            "{role} House ROF"
        );
    }
    let actors = Actors {
        transport: sim
            .spawn_object(
                "FV",
                input["transport_country"].as_str().unwrap(),
                16,
                16,
                64,
                rules,
            )
            .unwrap(),
        passenger: sim
            .spawn_object(
                "CLEG",
                input["passenger_country"].as_str().unwrap(),
                16,
                15,
                64,
                rules,
            )
            .unwrap(),
        victim: sim
            .spawn_object("FV", "Russians", 20, 16, 192, rules)
            .unwrap(),
    };
    for role in ["transport", "passenger"] {
        install_actor_boundary(&mut sim, actors.id(role), &row["initial"][role]);
        let rank_key = format!("{role}_rank");
        let rank = match signed(&input[rank_key.as_str()]) {
            0 => 0,
            1 => 100,
            2 => 200,
            value => panic!("unsupported declared native rank {value}"),
        };
        sim.substrate
            .entities
            .get_mut(actors.id(role))
            .unwrap()
            .veterancy
            .set_rank_for_test(rank);
    }
    sim.substrate
        .entities
        .get_mut(actors.transport)
        .unwrap()
        .set_gunner_selection_for_test(
            signed(&row["initial"]["transport"]["weapon_index"]),
            signed(&row["initial"]["transport"]["turret_index"]),
        );
    install_temporal_boundary(&mut sim, &actors, &row["initial"]);
    sim.session.set_binary_frame_for_test(frame as u32);
    sim.scenario_rng = SimRng::new(input["seed"].as_u64().unwrap());
    assert_eq!(
        sim.scenario_rng.native_state_hex(),
        row["rng_initial"]["scenario"],
        "native initial Scenario"
    );
    assert_state(
        &sim,
        &actors,
        &row["initial"],
        input["name"].as_str().unwrap(),
    );
    (sim, actors)
}

#[test]
fn native_temporal_gunner_callbacks_match_retained_histories() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let native = corpus();
    assert_eq!(native["schema_version"], 1);
    assert_stock_inputs(rules, native);
    let rows = native["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 12);
    let mut callbacks = 0;
    let mut imported_fire_boundaries = 0;
    for row in rows {
        let name = row["input"]["name"].as_str().unwrap();
        let (mut sim, actors) = prepare_case(rules, row);
        // These actual Rust streams need only remain unchanged across the
        // component callbacks. Their setup was not the native whole Scenario
        // producer; no whole-app Main/MapGen initial-state parity is asserted.
        let main_before = sim.main_rng.native_state_hex();
        let mapgen_before = sim.mapgen_rng.native_state_hex();
        for stream in ["main", "mapgen"] {
            assert_eq!(
                SimRng::new(0).native_state_hex(),
                row["rng_initial"][stream],
                "native {stream} seed"
            );
        }
        for (index, operation) in row["operations"].as_array().unwrap().iter().enumerate() {
            let label = format!("{name} operation{index} {}", operation["name"]);
            assert_state(&sim, &actors, &operation["before"], &label);
            for stream in ["main", "mapgen"] {
                assert_eq!(
                    operation["rng_after"][stream], row["rng_initial"][stream],
                    "{label} native {stream}"
                );
                assert_eq!(
                    operation["raw_draw_count"][stream], 0,
                    "{label} native {stream} draws"
                );
            }
            match operation["name"].as_str().unwrap() {
                "fire_rearm" => {
                    // Import exactly one measured native post-shot boundary.
                    // No FireAt/GetROF arithmetic or burst remainder is copied
                    // into this test. Only subsequent callbacks are compared.
                    assert_eq!(name, "actual_fire_rearm_tail");
                    for role in ["transport", "passenger"] {
                        install_actor_boundary(
                            &mut sim,
                            actors.id(role),
                            &operation["after"][role],
                        );
                    }
                    sim.scenario_rng = SimRng::from_native_state_hex_for_test(
                        operation["rng_after"]["scenario"].as_str().unwrap(),
                    );
                    imported_fire_boundaries += 1;
                }
                callback => {
                    let (_, draws) = trace_draws(|| match callback {
                        "receive" | "retry" => receive_gunner(
                            &sim.interner,
                            &sim.session,
                            &mut sim.scenario_rng,
                            &mut sim.substrate,
                            &sim.houses,
                            rules,
                            actors.transport,
                            actors.passenger,
                        ),
                        "remove" => remove_gunner(
                            &sim.interner,
                            &sim.session,
                            &mut sim.scenario_rng,
                            &mut sim.substrate,
                            &mut sim.houses,
                            &mut sim.power_states,
                            rules,
                            actors.transport,
                            Some(actors.passenger),
                        ),
                        "remove_null" => remove_gunner(
                            &sim.interner,
                            &sim.session,
                            &mut sim.scenario_rng,
                            &mut sim.substrate,
                            &mut sim.houses,
                            &mut sim.power_states,
                            rules,
                            actors.transport,
                            None,
                        ),
                        other => panic!("unknown native operation {other}"),
                    });
                    let native_draws = operation["raw_draws"].as_array().unwrap();
                    assert!(native_draws.iter().all(|draw| draw["stream"] == "scenario"));
                    assert_eq!(
                        draws.len() as u64,
                        operation["raw_draw_count"]["scenario"].as_u64().unwrap(),
                        "{label} raw draw count"
                    );
                    assert_eq!(
                        draws
                            .iter()
                            .map(|draw| draw["value"].as_u64().unwrap())
                            .collect::<Vec<_>>(),
                        native_draws
                            .iter()
                            .map(|draw| draw["raw_u32"].as_u64().unwrap())
                            .collect::<Vec<_>>(),
                        "{label} ordered raw draws, including rejection"
                    );
                    callbacks += 1;
                }
            }
            assert_state(&sim, &actors, &operation["after"], &label);
            assert_eq!(
                sim.scenario_rng.native_state_hex(),
                operation["rng_after"]["scenario"],
                "{label} full Scenario"
            );
            assert_eq!(
                sim.main_rng.native_state_hex(),
                main_before,
                "{label} Rust Main preserved"
            );
            assert_eq!(
                sim.mapgen_rng.native_state_hex(),
                mapgen_before,
                "{label} Rust MapGen preserved"
            );
        }
    }
    assert_eq!(callbacks, 45);
    assert_eq!(imported_fire_boundaries, 1);
}
