//! Original executable values, replayed through the shared production owners.
//! Scope/provenance: tools/spatial_oracle/base_defense_response.md.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::test_interner;
use crate::sim::passenger::{PassengerCargo, PassengerRole};
use serde_json::Value;

fn install_supplied_base_defense_team(sim: &mut Simulation, member: u64) {
    use crate::rules::object_type::ObjectCategory;
    use crate::rules::team_ai_ini::TeamAiDefinitionSource;
    use crate::sim::team_script_vm::{
        TeamMemberTypeIdentity, TeamScriptDefinition, TeamScriptMember, TeamTaskForceDefinition,
        TeamTaskForceEntry, TeamTypeDefinition,
    };
    let entity = sim.substrate.entities.get(member).unwrap();
    let owner = entity.owner();
    let member_type = TeamMemberTypeIdentity {
        category: ObjectCategory::Vehicle,
        id: entity.type_ref(),
    };
    let name = sim.interner.intern(&format!("CORPUS_TEAM_{member}"));
    sim.team_script_vm.register_script(TeamScriptDefinition {
        id: name,
        source: TeamAiDefinitionSource::FixedAimd,
        actions: Vec::new(),
    });
    sim.team_script_vm
        .register_task_force(TeamTaskForceDefinition {
            id: name,
            source: TeamAiDefinitionSource::FixedAimd,
            group: -1,
            entries: vec![TeamTaskForceEntry {
                member_type,
                count: 1,
            }],
        });
    sim.team_script_vm.register_team_type(TeamTypeDefinition {
        id: name,
        script_id: name,
        task_force_id: name,
        priority: 0,
        is_base_defense: true,
        suicide: false,
        aggressive: false,
        combined_movement_zone: crate::rules::locomotor_type::MovementZone::Normal,
        base_zone_relation_enforced: false,
        transport_crossing_required: false,
    });
    sim.team_script_vm.create_team_from_type(
        owner,
        name,
        &[TeamScriptMember {
            entity_id: member,
            member_type,
        }],
        0,
    );
}

/// Supplied selected-list seam into the actual production transaction. This
/// deliberately excludes the native stack's nonzero initial accumulator and
/// a non-Foot attacker flag: full708080 initializes the former to0 and this
/// caller's attacker is Infantry/Unit. It also excludes the timer's stale
/// auxiliary stack word, which Rust's timer does not consume.
#[test]
fn native_base_response_dispatch_matches_14_represented_original_tails() {
    use crate::sim::components::Health;
    use crate::sim::rng::{SimRng, trace_draws};
    use crate::sim::timer::CdTimer;
    let Some(ini) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
        return;
    };
    let Some(art) = crate::rules::retail_ini_fixture::retail_ini("artmd.ini") else {
        return;
    };
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
    rules.bind_animation_sequences(
        &crate::rules::infantry_sequence::parse_infantry_sequence_registry(&art),
    );
    let base_delay = rules.general.base_defense_delay_minutes;
    let native = corpus();
    let mut compared = 0;
    for row in native["dispatch"].as_array().unwrap() {
        let input = &row["input"];
        if input.get("initial_assigned").is_some() || input.get("attacker_flags").is_some() {
            continue;
        }
        let name = input["name"].as_str().unwrap();
        rules.general.base_defense_delay_minutes = base_delay;
        if let Some(bits) = input["delay_bits"].as_str() {
            rules.general.base_defense_delay_minutes =
                f64::from_bits(u64::from_str_radix(bits, 16).unwrap());
        }
        let mut sim = Simulation::new();
        sim.session.binary_frame = 41;
        let owner = sim.interner.intern("Receiver");
        let enemy = sim.interner.intern("Enemy");
        let mut ids = std::collections::BTreeMap::new();
        let mut actors = vec![("victim", "HARV", owner), ("attacker", "HTNK", enemy)];
        for object in input["objects"].as_array().unwrap() {
            actors.push((
                object[0].as_str().unwrap(),
                object[1].as_str().unwrap(),
                owner,
            ));
            assert_eq!(
                rules
                    .object(object[1].as_str().unwrap())
                    .unwrap()
                    .threat_posed,
                signed(&object[2]),
                "physical type threat: {name}"
            );
        }
        for (index, (label, type_name, house)) in actors.into_iter().enumerate() {
            let id = index as u64 + 1;
            ids.insert(label, id);
            let type_id = sim.interner.intern(type_name);
            let category = if type_name == "E1" {
                EntityCategory::Infantry
            } else {
                EntityCategory::Unit
            };
            let mut actor = GameEntity::new_at_frame_zero_for_test(
                id,
                5,
                5,
                0,
                0,
                house,
                Health { current: 100 },
                type_id,
                category,
                0,
                0,
                category != EntityCategory::Infantry,
            );
            actor.passively_acquired_target = true;
            if category == EntityCategory::Infantry {
                actor.mission_leaf.set_infantry_doing_verified(-1).unwrap();
            }
            sim.substrate.entities.insert(actor);
        }
        for label in input["base_defense_team"].as_array().into_iter().flatten() {
            install_supplied_base_defense_team(&mut sim, ids[label.as_str().unwrap()]);
        }
        sim.scenario_rng = SimRng::new(input["seed"].as_u64().unwrap());
        assert_eq!(
            sim.scenario_rng.native_state_hex(),
            row["rng_before"],
            "{name}"
        );
        let selection = ResponseSelection {
            remaining_budget: signed(&input["budget"]),
            minimum_score: 0,
            responders: input["selected"]
                .as_array()
                .unwrap()
                .iter()
                .zip(input["scores"].as_array().unwrap())
                .map(|(label, score)| RankedResponder {
                    entity_id: ids[label.as_str().unwrap()],
                    score: signed(score),
                })
                .collect(),
        };
        let (assigned, draws) =
            trace_draws(|| selection.dispatch(&mut sim, &rules, ids["victim"], ids["attacker"]));
        assert_eq!(assigned, signed(&row["assigned"]), "{name}");
        assert_eq!(
            sim.scenario_rng.native_state_hex(),
            row["rng_after"],
            "{name}"
        );
        let native_draws: Vec<_> = row["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| event.get("raw_draw").is_some())
            .collect();
        assert_eq!(draws.len(), native_draws.len(), "{name}");
        for (draw, native) in draws.iter().zip(native_draws) {
            assert_eq!(draw["value"], native["raw_draw"], "{name}");
            assert_eq!(draw["before_indices"], native["indices"], "{name}");
        }
        let cooldown = &row["cooldown"];
        assert_eq!(
            sim.substrate
                .entities
                .get(ids["attacker"])
                .unwrap()
                .base_defense_response
                .cooldown,
            CdTimer::from_raw(signed(&cooldown[0]), signed(&cooldown[2])),
            "{name}"
        );
        for (label, state) in row["final"].as_object().unwrap() {
            let actor = sim.substrate.entities.get(ids[label.as_str()]).unwrap();
            assert_eq!(
                actor.mission.queued().raw(),
                signed(&state["queue"]),
                "{name}:{label}"
            );
            let target = |value: &Value| {
                (value != "null").then(|| TargetKind::Entity(ids[value.as_str().unwrap()]))
            };
            assert_eq!(
                actor.archive_target(),
                target(&state["archive"]),
                "{name}:{label}"
            );
            assert_eq!(
                actor.attack_target.as_ref().map(|target| target.target),
                target(&state["target"]),
                "{name}:{label}"
            );
            assert_eq!(
                actor.passively_acquired_target,
                state["passive"] != 0,
                "{name}:{label}"
            );
            if let Some(doing) = state["doing"].as_i64() {
                assert_eq!(
                    i64::from(actor.mission_leaf.as_infantry().unwrap().doing()),
                    doing,
                    "{name}:{label}"
                );
            }
        }
        compared += 1;
    }
    assert_eq!(compared, 14);
}

fn corpus() -> Value {
    let rows: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/base_defense_response.json",
    ))
    .unwrap();
    assert_eq!(rows["schema_version"], 1);
    assert_eq!(
        rows["native_sha256"],
        "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c",
    );
    rows
}

fn signed(value: &Value) -> i32 {
    i32::try_from(value.as_i64().unwrap()).unwrap()
}

fn coord(value: &Value) -> [i32; 3] {
    std::array::from_fn(|axis| signed(&value[axis]))
}

fn responder_id(value: &Value) -> u64 {
    value
        .as_u64()
        .unwrap_or_else(|| value.as_str().unwrap().parse().unwrap())
}

fn supplied_ini(input: &Value) -> IniFile {
    let mut text = String::new();
    for (section, keys) in input.as_object().unwrap() {
        text.push_str(&format!("[{section}]\n"));
        for (key, raw) in keys.as_object().unwrap() {
            text.push_str(&format!("{key}={}\n", raw.as_str().unwrap()));
        }
    }
    IniFile::from_str(&text)
}

fn assert_response_rules(rules: &RuleSet, native: &Value) {
    let general = &rules.general;
    assert_eq!(
        general.computer_base_defense_response,
        signed(&native["computer_base_defense_response"])
    );
    assert_eq!(
        general.threat_per_occupant,
        signed(&native["threat_per_occupant"])
    );
    assert_eq!(
        general.suspend_priority,
        signed(&native["suspend_priority"])
    );
    assert_eq!(
        format!("{:016X}", general.base_defense_delay_minutes.to_bits()),
        native["base_defense_delay_bits"].as_str().unwrap()
    );
    assert_eq!(
        format!("{:016X}", general.suspend_delay_minutes.to_bits()),
        native["suspend_delay_bits"].as_str().unwrap()
    );
}

#[test]
fn native_base_response_retail_and_sequential_readers_match_original_fields() {
    let Some(ini) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
        return;
    };
    let native = corpus();
    let mut layers = RulesLayerStack::new(ini);
    let rules = RuleSet::from_rules_layers(&layers).unwrap();
    let physical = &native["physical"]["layers"][0]["output"];
    assert_response_rules(&rules, &physical["rules"]);
    for (name, output) in physical["types"].as_object().unwrap() {
        let object = rules.object(name).unwrap();
        assert_eq!(object.threat_posed, signed(&output["threat"]), "{name}");
        assert_eq!(
            ra2_speed_to_leptons_per_frame(object.speed),
            signed(&output["speed"]),
            "{name}"
        );
    }
    // The shared native fixture applies the type controls first, then the
    // AI/General controls. Each output retains all earlier live fields.
    for row in native["type_reader_controls"]
        .as_array()
        .unwrap()
        .iter()
        .chain(native["reader_controls"].as_array().unwrap())
    {
        layers.push(RulesLayerKind::Scenario, supplied_ini(&row["input"]));
        let rules = RuleSet::from_rules_layers(&layers).unwrap();
        assert_response_rules(&rules, &row["output"]["rules"]);
        for (name, output) in row["output"]["types"].as_object().unwrap() {
            let object = rules.object(name).unwrap();
            assert_eq!(
                object.threat_posed,
                signed(&output["threat"]),
                "{}:{name}",
                row["layer"]
            );
            assert_eq!(
                ra2_speed_to_leptons_per_frame(object.speed),
                signed(&output["speed"]),
                "{}:{name}",
                row["layer"]
            );
            if let Some(bunker) = output["bunker"].as_i64() {
                assert_eq!(object.bunker, bunker != 0, "{}:{name}", row["layer"]);
            }
        }
    }
}

#[test]
fn native_base_response_speed_history_matches_original_type_speed() {
    let native = corpus();
    let rows = native["speed_history"].as_array().unwrap();
    assert_eq!(rows.len(), 27);
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[VehicleTypes]\n0=MTNK\n[MTNK]\nStrength=100\nSpeed=7\n",
    ));
    let mut compared = 0;
    for row in rows {
        let raw = row["raw"].as_str();
        // Native's scalar cache can contain an empty value. Both lexical
        // loaders omit it; that scalar-only control is outside this replay.
        if raw == Some("") {
            continue;
        }
        let text = raw.map_or_else(
            || "[MTNK]\nOther=1\n".to_owned(),
            |raw| format!("[MTNK]\nSpeed={raw}\n"),
        );
        layers.push(RulesLayerKind::Scenario, IniFile::from_str(&text));
        let rules = RuleSet::from_rules_layers(&layers).unwrap();
        assert_eq!(
            ra2_speed_to_leptons_per_frame(rules.object("MTNK").unwrap().speed),
            signed(&row["output"]["types"]["MTNK"]["speed"]),
            "{raw:?}"
        );
        compared += 1;
    }
    assert_eq!(compared, 26);
}

#[test]
fn native_base_response_scorer_matches_all_original_23_rows() {
    let native = corpus();
    let rows = native["scoring"].as_array().unwrap();
    assert_eq!(rows.len(), 23);
    for row in rows {
        let input = &row["input"];
        let facts = ThreatFacts {
            threat_posed: signed(&input["threat"]),
            speed_leptons_per_frame: signed(&input["speed"]),
            current_coord: coord(&input["current"]),
            attacker_coord: (!input["null_attacker"].as_bool().unwrap_or(false))
                .then(|| coord(&input["attacker"])),
            primary_range_leptons: signed(&input["range"]),
            existing_target: if input["target"] == "attacker" {
                ExistingTargetDisposition::RequestedAttacker
            } else {
                ExistingTargetDisposition::NoneOrUnarmed
            },
            in_non_base_defense_team: input["team_base_defense"].as_bool() == Some(false),
            mission_is_harvest: input["harvest"].as_bool().unwrap_or(false),
        };
        assert_eq!(
            evaluate_target_threat(facts),
            signed(&row["output"]),
            "{input}"
        );
    }
}

#[test]
fn native_base_response_recruitment_matches_original_intermediate_slots() {
    let native = corpus();
    let rows = native["selection"].as_array().unwrap();
    assert_eq!(rows.len(), 8);
    for row in rows {
        let input = &row["input"];
        let mut selection = ResponseSelection::new(signed(&input["budget"]));
        for (entry, output) in input["entries"]
            .as_array()
            .unwrap()
            .iter()
            .zip(row["output"].as_array().unwrap())
        {
            let class = match entry[2].as_str().unwrap() {
                "infantry" => ResponderClass::Infantry,
                "unit" => ResponderClass::Unit,
                other => panic!("class {other}"),
            };
            selection.consider(
                entry[0].as_u64().unwrap(),
                signed(&entry[1]),
                class,
                input["victim_is_self_anchor"].as_bool().unwrap(),
            );
            assert_eq!(
                selection.remaining_budget,
                signed(&output["remaining"]),
                "{input}:{entry}"
            );
            assert_eq!(
                selection.minimum_score,
                signed(&output["minimum"]),
                "{input}:{entry}"
            );
            assert_eq!(
                selection
                    .responders
                    .iter()
                    .map(|entry| entry.entity_id)
                    .collect::<Vec<_>>(),
                output["ids"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(responder_id)
                    .collect::<Vec<_>>(),
                "{input}:{entry}"
            );
            assert_eq!(
                selection
                    .responders
                    .iter()
                    .map(|entry| entry.score)
                    .collect::<Vec<_>>(),
                output["scores"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(signed)
                    .collect::<Vec<_>>(),
                "{input}:{entry}"
            );
        }
    }
}

#[test]
fn native_base_response_sort_matches_original_pairwise_exchanges() {
    let native = corpus();
    let rows = native["sort"].as_array().unwrap();
    assert_eq!(rows.len(), 8);
    for row in rows {
        let responders = row["input"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(index, score)| RankedResponder {
                entity_id: index as u64 + 1,
                score: signed(score),
            })
            .collect();
        let (_, ranked) = ResponseSelection {
            remaining_budget: 1,
            minimum_score: 0,
            responders,
        }
        .into_ranked();
        assert_eq!(
            ranked
                .iter()
                .map(|entry| entry.entity_id)
                .collect::<Vec<_>>(),
            row["ids"]
                .as_array()
                .unwrap()
                .iter()
                .map(responder_id)
                .collect::<Vec<_>>(),
            "{}",
            row["input"]
        );
        assert_eq!(
            ranked.iter().map(|entry| entry.score).collect::<Vec<_>>(),
            row["scores"]
                .as_array()
                .unwrap()
                .iter()
                .map(signed)
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn native_base_response_live_threat_matches_representable_original_rows() {
    let native = corpus();
    let mut compared = 0;
    for row in native["threat_posed"].as_array().unwrap() {
        let input = &row["input"];
        let count = input["garrison_count"].as_i64().unwrap_or(0);
        // The corrupt/supplied i32MAX count is not a Rust cargo vector.
        // The native two-occupant/max-multiplier control covers wrapping.
        if count == i64::from(i32::MAX) {
            continue;
        }
        let structure = input["type"] == "NATBNK";
        let header = if structure {
            "[BuildingTypes]\n0=OWNER\n[VehicleTypes]\n0=LINK\n"
        } else {
            "[VehicleTypes]\n0=OWNER\n1=LINK\n"
        };
        let rules = RuleSet::from_ini(&IniFile::from_str(&format!("[General]\nThreatPerOccupant={}\n{header}[OWNER]\nStrength=100\nThreatPosed={}\n[LINK]\nStrength=100\nThreatPosed={}\n", input["threat_per_occupant"].as_i64().unwrap_or(5), signed(&input["own_threat"]), input["linked_threat"].as_i64().unwrap_or(0)))).unwrap();
        let mut owner = GameEntity::test_default(1, "OWNER", "House", 5, 5);
        owner.category = if structure {
            EntityCategory::Structure
        } else {
            EntityCategory::Unit
        };
        if input["linked_threat"].is_number() {
            owner.bunker_occupant = Some(2);
        }
        if count > 0 {
            let mut cargo = PassengerCargo::new(count as u32, 1);
            for id in 0..count as u64 {
                assert!(cargo.board(id + 10, 1));
            }
            owner.passenger_role = PassengerRole::Transport { cargo };
        }
        let mut entities = EntityStore::new();
        entities.insert(GameEntity::test_default(2, "LINK", "House", 5, 5));
        let object = if input["null_type"].as_bool().unwrap_or(false) {
            None
        } else {
            rules.object("OWNER")
        };
        assert_eq!(
            live_threat_posed(&owner, object, &entities, &rules, &test_interner()),
            signed(&row["output"]),
            "{input}"
        );
        compared += 1;
    }
    assert_eq!(compared, 10);
}
