//! Committed order voices through the production Simulation owner. Native
//! Capture708DC0/Enter709020/Harvest709060 fallback to Move708FC0; the
//! QueueMegaMission6FFCBD enable gate precedes the reached sound function.

use super::Simulation;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::rules::sound_ini::SoundRegistry;
use crate::sim::rng::{SimRng, trace_draws};

fn native_voice_corpus() -> serde_json::Value {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/allied_object_action.json",
    ))
    .unwrap();
    corpus
}

#[test]
fn retail_attack_acknowledgement_spends_the_original_singleton_main_draw() {
    use crate::map::entities::EntityCategory;
    use crate::sim::combat::Health;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::house_state::HouseState;

    let corpus = crate::test_fixture::json("tools/spatial_oracle/attack_order_voice.json");
    let row = corpus["voice_controls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == "stock_mtnk_target")
        .unwrap();
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let mut sim = Simulation::with_seed(row["input"]["seed"].as_u64().unwrap());
    let local = sim.interner.intern("Local");
    sim.session.set_current_house_for_test(Some(local));
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    sim.houses
        .register(HouseState::new(local, 0, None, true, 0, 10));
    let type_ref = sim.interner.intern("MTNK");
    sim.substrate
        .entities
        .insert(GameEntity::new_at_frame_zero_for_test(
            1,
            10,
            10,
            0,
            0,
            local,
            Health::new(retail.rules.object("MTNK").unwrap().strength),
            type_ref,
            EntityCategory::Unit,
            0,
            5,
            false,
        ));
    assert_eq!(
        sim.main_rng.native_state_hex(),
        row["output"]["main_before_hex"].as_str().unwrap()
    );
    let scenario = sim.scenario_rng.logical_state();
    let mapgen = sim.mapgen_rng.logical_state();
    let (_, draws) =
        trace_draws(|| sim.order_voice_request(&retail.rules, 1, "VoiceAttack", None, true));
    // Original6FFBE0 ->7090A0 calls Main65C780 at709193, even for
    // the actual stock singleton. The original selector's secondary-NULL
    // return occurs before any target read; no Rust modulo supplies this golden.
    assert_eq!(
        serde_json::json!(draws.iter().map(|draw| &draw["value"]).collect::<Vec<_>>()),
        row["output"]["main_draws"]
    );
    assert_eq!(
        sim.main_rng.native_state_hex(),
        row["output"]["main_after_hex"].as_str().unwrap()
    );
    assert_eq!(sim.scenario_rng.logical_state(), scenario);
    assert_eq!(sim.mapgen_rng.logical_state(), mapgen);
}

fn scene(body: &str) -> (Simulation, RuleSet, u64) {
    scene_with_input(body, &serde_json::json!({}))
}

fn scene_with_input(body: &str, input: &serde_json::Value) -> (Simulation, RuleSet, u64) {
    let ini = IniFile::from_str(&format!(
        "[InfantryTypes]\n0=ENGINEER\n[ENGINEER]\nStrength=100\n{body}"
    ));
    let mut rules = RuleSet::from_ini(&ini).unwrap();
    let native_sounds: String = input["voice_ids"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|id| id.as_i64().filter(|id| *id >= 0))
        .enumerate()
        .map(|(index, id)| format!("Native{index}=Sound{id}\n"))
        .collect();
    let sounds = SoundRegistry::from_ini(&IniFile::from_str(&format!(
        "[SoundList]\n0=Move\n1=Enter\n2=Capture\n3=Harvest\n4=Attack\n5=Special\n{native_sounds}"
    )));
    rules.bind_type_sound_references(&ini, &sounds);
    scene_for_rules(
        rules,
        input,
        "ENGINEER",
        crate::map::entities::EntityCategory::Infantry,
    )
}

fn scene_for_rules(
    rules: RuleSet,
    input: &serde_json::Value,
    type_name: &str,
    category: crate::map::entities::EntityCategory,
) -> (Simulation, RuleSet, u64) {
    use crate::sim::combat::Health;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::house_state::HouseState;

    let mut sim = Simulation::with_seed(input["seed"].as_u64().unwrap_or(1));
    let local = sim.interner.intern("Local");
    let other = sim.interner.intern("Other");
    let actor_owner = match input["actor_owner"].as_str().unwrap_or("local") {
        "local" => local,
        "other" => other,
        other => panic!("unknown native actor owner {other}"),
    };
    let type_ref = sim.interner.intern(type_name);
    sim.session.set_current_house_for_test(Some(local));
    sim.session.set_game_mode(
        crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(
            input["mode"].as_u64().unwrap_or(1) != 0,
        ),
    );
    sim.houses
        .register(HouseState::new(local, 0, None, true, 0, 10));
    let mut other_house = HouseState::new(
        other,
        1,
        None,
        input["other_human"].as_bool().unwrap_or(false),
        0,
        10,
    );
    other_house
        .set_player_control_for_test(input["other_player_control"].as_bool().unwrap_or(false));
    sim.houses.register(other_house);
    let id = 1;
    sim.substrate
        .entities
        .insert(GameEntity::new_at_frame_zero_for_test(
            id,
            0,
            0,
            0,
            0,
            actor_owner,
            Health::new(rules.object(type_name).unwrap().strength),
            type_ref,
            category,
            0,
            5,
            false,
        ));
    (sim, rules, id)
}

fn move_control_rules(corpus: &serde_json::Value, reader_case: &str) -> RuleSet {
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
    use crate::test_fixture::ini_sections;

    // The action fixture supplies a positive-health MTNK actor shell. Its
    // voice fields come only from the linked original raw reader history.
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[VehicleTypes]\n0=MTNK\n[MTNK]\nStrength=300\n",
    ));
    for row in corpus["readers"]["layers"].as_array().unwrap() {
        if row["absent"] != true {
            layers.push(RulesLayerKind::Scenario, ini_sections(&row["selected"]));
        }
    }
    if reader_case != "stock" {
        let mut found = false;
        for row in corpus["readers"]["retained_controls"].as_array().unwrap() {
            layers.push(
                RulesLayerKind::Scenario,
                ini_sections(&row["input"]["sections"]),
            );
            if row["name"] == reader_case {
                found = true;
                break;
            }
        }
        assert!(found, "native raw reader history {reader_case}");
    }
    let processed = layers.process().unwrap();
    let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
    let sounds = SoundRegistry::from_ini(&ini_sections(&corpus["readers"]["sound"]["selected"]));
    rules.bind_type_sound_references(processed.ini(), &sounds);
    rules
}

fn assert_move_voice_request(corpus: &serde_json::Value, row: &serde_json::Value, rules: RuleSet) {
    let label = row["name"].as_str().unwrap();
    let input = &row["input"];
    let output = &row["output"];
    let sound_name = |index: i64| {
        corpus["readers"]["sound"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|sound| sound["fixture_index"] == index)
            .unwrap()["name"]
            .as_str()
            .unwrap()
    };
    let voices: Vec<_> = input["voice_ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|index| sound_name(index.as_i64().unwrap()).to_owned())
        .collect();
    assert_eq!(
        rules.object("MTNK").unwrap().voice_move(),
        voices,
        "{label}"
    );
    let (mut sim, rules, actor) = scene_for_rules(
        rules,
        input,
        "MTNK",
        crate::map::entities::EntityCategory::Unit,
    );
    // A declared native action boundary, before event encoding/audio. These
    // complete states are installed once, never reseeded between draws.
    sim.main_rng =
        SimRng::from_native_state_hex_for_test(output["main_before_hex"].as_str().unwrap()).into();
    sim.scenario_rng =
        SimRng::from_native_state_hex_for_test(output["scenario_before_hex"].as_str().unwrap());
    sim.mapgen_rng =
        SimRng::from_native_state_hex_for_test(output["mapgen_before_hex"].as_str().unwrap());
    let enabled = input["voices_enabled"].as_bool().unwrap();
    let entry = input["caller_entry"].as_str().unwrap();
    let field = match entry {
        "0x006FFBE0" | "0x00708FC0" => "VoiceMove",
        "0x00709020" => "VoiceEnter",
        "0x00708DC0" => "VoiceCapture",
        "0x00709060" => "VoiceHarvest",
        other => panic!("unhandled native order voice {other}"),
    };
    let human = sim.house_is_human_player(sim.entities().get(actor).unwrap().owner());
    let world = sim.state_hash();
    let (request, draws) = trace_draws(|| {
        if entry == "0x00708FC0" && !enabled {
            // Direct disabled VoiceMove bypasses QueueMegaMission's earlier
            // caller gate, while still consuming the production-bound list.
            sim.voice_request_from_list(rules.object("MTNK").unwrap().voice_move(), enabled, human)
        } else {
            sim.order_voice_request(&rules, actor, field, None, enabled)
        }
    });
    let queued = request.map_or_else(
        || output["queued_voice_before"].as_i64().unwrap(),
        |name| {
            corpus["readers"]["sound"]["rows"]
                .as_array()
                .unwrap()
                .iter()
                .find(|sound| sound["name"] == name)
                .expect("queued name belongs to the fixed native catalog")["fixture_index"]
                .as_i64()
                .unwrap()
        },
    );
    assert_eq!(
        queued, output["queued_voice_after"],
        "{label}: native request"
    );
    assert_eq!(
        serde_json::json!(draws.iter().map(|draw| &draw["value"]).collect::<Vec<_>>()),
        output["main_draws"],
        "{label}: raw draw count/order"
    );
    for (stream, actual, key) in [
        ("main", sim.main_rng.native_state_hex(), "main_after_hex"),
        (
            "scenario",
            sim.scenario_rng.native_state_hex(),
            "scenario_after_hex",
        ),
        (
            "mapgen",
            sim.mapgen_rng.native_state_hex(),
            "mapgen_after_hex",
        ),
    ] {
        assert_eq!(
            actual,
            output[key].as_str().unwrap(),
            "{label}: full {stream}"
        );
    }
    assert_eq!(
        sim.state_hash(),
        world,
        "{label}: voice has no world effect"
    );
    for expected in output["main_next_four"].as_array().unwrap() {
        assert_eq!(
            u64::from(sim.main_rng.next_u32()),
            expected.as_u64().unwrap(),
            "{label}"
        );
    }
}

#[test]
fn move_voice_matches_original_linked_readers_fallbacks_and_all_streams() {
    let corpus = crate::test_fixture::json("tools/spatial_oracle/move_order_voice.json");
    let rows = corpus["voice_controls"].as_array().unwrap();
    assert_eq!(rows.len(), 18, "all original action controls");
    for row in rows {
        assert_move_voice_request(
            corpus,
            row,
            move_control_rules(corpus, row["input"]["reader_case"].as_str().unwrap()),
        );
    }
}

#[test]
#[ignore = "slow; requires retail Battle map rules and fixed sound catalog"]
fn retail_move_voice_matches_original_fixed_reader_and_request() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let corpus = crate::test_fixture::json("tools/spatial_oracle/move_order_voice.json");
    let row = corpus["voice_controls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == "stock_caller")
        .unwrap();
    assert_move_voice_request(corpus, row, retail.rules.clone());
}

fn attack_control_rules(corpus: &serde_json::Value, input: &serde_json::Value) -> RuleSet {
    use std::collections::BTreeMap;

    let names: BTreeMap<_, _> = corpus["readers"]["sound"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            (
                row["fixture_index"].as_i64().unwrap(),
                row["name"].as_str().unwrap(),
            )
        })
        .collect();
    let mut catalog = String::from("[SoundList]\n");
    for (index, name) in &names {
        catalog.push_str(&format!("{index}={name}\n"));
    }
    let sounds = SoundRegistry::from_ini(&IniFile::from_str(&catalog));
    let type_name = input["type_name"].as_str().unwrap();
    let voices = input["voice_ids"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|id| names.get(&id.as_i64().unwrap()).copied())
        .collect::<Vec<_>>()
        .join(",");
    let mut body = format!("[VehicleTypes]\n0={type_name}\n");
    if type_name != "MTNK" {
        body.push_str("1=MTNK\n[MTNK]\nStrength=300\n");
    }
    body.push_str(&format!(
        "[BuildingTypes]\n0=GAPOWR\n[GAPOWR]\nStrength=750\n\
         [{type_name}]\nStrength=300\nVoiceAttack={voices}\n"
    ));
    for (key, id) in [
        "VoicePrimaryWeaponAttack",
        "VoicePrimaryEliteWeaponAttack",
        "VoiceSecondaryWeaponAttack",
        "VoiceSecondaryEliteWeaponAttack",
    ]
    .into_iter()
    .zip(input["weapon_voice_slots"].as_array().unwrap())
    {
        if let Some(name) = names.get(&id.as_i64().unwrap()) {
            body.push_str(&format!("{key}={name}\n"));
        }
    }
    if input["null_weapon"] != true {
        body.push_str("Primary=VOICE_CONTROL\n");
    }
    match input["selector_boundary"].as_i64() {
        Some(1) => {
            // The native component supplies index1 at its declared selector
            // seam. Use the existing gunner owner to reach the same index,
            // rather than duplicating selection in the voice implementation.
            body.push_str("TurretCount=1\nWeapon1=VOICE_CONTROL\nWeapon2=VOICE_CONTROL\n");
        }
        Some(-1) => body.push_str("DeployFire=yes\nDeployFireWeapon=-1\n"),
        None | Some(0) => {}
        other => panic!("unhandled native selector boundary {other:?}"),
    }
    body.push_str(&format!(
        "[VOICE_CONTROL]\nDamage={}\nAmbientDamage={}\nProjectile=VOICE_PROJECTILE\n",
        input["damage"].as_i64().unwrap(),
        input["ambient_damage"].as_i64().unwrap(),
    ));
    if input["missing_warhead"] != true {
        body.push_str("Warhead=VOICE_WARHEAD\n[VOICE_WARHEAD]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n");
    }
    body.push_str("[VOICE_PROJECTILE]\nAG=yes\nAA=yes\n");
    if let Some(name) = names.get(&input["voice_ifv_repair"].as_i64().unwrap()) {
        body.push_str(&format!("[AudioVisual]\nVoiceIFVRepair={name}\n"));
    }
    let ini = IniFile::from_str(&body);
    let mut rules = RuleSet::from_ini(&ini).unwrap();
    rules.bind_type_sound_references(&ini, &sounds);
    rules
}

#[test]
fn attack_voice_matches_native_target_slots_rng_and_queue_admission() {
    use crate::map::entities::EntityCategory;
    use crate::sim::combat::{Health, TargetKind};
    use crate::sim::game_entity::GameEntity;
    use crate::util::native_x87::NativeF32Bits;
    use std::panic::{AssertUnwindSafe, catch_unwind};

    let corpus = crate::test_fixture::json("tools/spatial_oracle/attack_order_voice.json");
    let sound_name = |index: i64| {
        corpus["readers"]["sound"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["fixture_index"].as_i64() == Some(index))
            .unwrap()["name"]
            .as_str()
            .unwrap()
    };
    let mut full_requests = 0;
    let mut unbound_vector_kernel = 0;
    let mut native_faults = 0;
    for row in corpus["voice_controls"].as_array().unwrap() {
        let name = row["name"].as_str().unwrap();
        let input = &row["input"];
        let output = &row["output"];
        let type_name = input["type_name"].as_str().unwrap();
        let (mut sim, rules, actor) = scene_for_rules(
            attack_control_rules(&corpus, input),
            input,
            type_name,
            EntityCategory::Unit,
        );
        let object = rules.object(type_name).unwrap();
        let entity = sim.substrate.entities.get_mut(actor).unwrap();
        entity.veterancy.set_raw_for_test(NativeF32Bits::from_bits(
            input["raw_veterancy_bits"].as_u64().unwrap() as u32,
        ));
        // A stale opposite cache cannot choose the weapon voice tier.
        entity
            .veterancy
            .set_rank_cache_for_test(if input["raw_veterancy_bits"] == 0 {
                0
            } else {
                2
            });
        if let Some(index) = input["selector_boundary"].as_i64() {
            if index == -1 {
                entity.set_unit_simple_deploy_for_test(true, false, false);
            } else {
                entity.set_gunner_weapon(index as i32, object);
            }
        }
        let target = if input["null_target"] == true {
            None
        } else {
            let type_ref = sim.interner.intern(input["target_type"].as_str().unwrap());
            let owner = sim.interner.get("Other").unwrap();
            let category = if input["target_type"] == "GAPOWR" {
                EntityCategory::Structure
            } else {
                EntityCategory::Unit
            };
            sim.substrate
                .entities
                .insert(GameEntity::new_at_frame_zero_for_test(
                    2,
                    4,
                    0,
                    0,
                    0,
                    owner,
                    Health::new(100),
                    type_ref,
                    category,
                    0,
                    5,
                    false,
                ));
            Some(TargetKind::Entity(2))
        };
        assert_eq!(
            sim.main_rng.native_state_hex(),
            output["main_before_hex"].as_str().unwrap(),
            "{name}"
        );
        let scenario = sim.scenario_rng.native_state_hex();
        let mapgen = sim.mapgen_rng.native_state_hex();
        let hash = sim.state_hash();
        let enabled = input["voices_enabled"].as_bool().unwrap();
        let direct = input["caller_entry"] == "0x007090A0";
        let unbound = input["voice_ids"] == serde_json::json!([-1]);
        let unbound_voices = [String::new()];
        let human = sim.house_is_human_player(sim.entities().get(actor).unwrap().owner());
        let (result, draws) = trace_draws(|| {
            catch_unwind(AssertUnwindSafe(|| {
                if unbound {
                    // Original ReadSoundList never binds -1. This deliberately
                    // prepared vector belongs only to the shared list kernel.
                    sim.voice_request_from_list(&unbound_voices, enabled, human)
                } else if direct {
                    sim.attack_voice_request(&rules, actor, target, enabled)
                } else {
                    sim.order_voice_request(&rules, actor, "VoiceAttack", target, enabled)
                }
            }))
        });
        if output["fault"].is_object() {
            native_faults += 1;
            assert!(
                result.is_err(),
                "{name}: original NULL slot faults before a draw"
            );
        } else {
            let request = result.unwrap();
            let queued = output["queued_voice_after"].as_i64().unwrap();
            let expected = (queued != output["queued_voice_before"].as_i64().unwrap())
                .then(|| sound_name(queued));
            assert_eq!(request, expected, "{name}: native QueueVoice admission");
            if unbound {
                unbound_vector_kernel += 1;
            } else {
                full_requests += 1;
            }
        }
        assert_eq!(
            serde_json::json!(draws.iter().map(|draw| &draw["value"]).collect::<Vec<_>>()),
            output["main_draws"],
            "{name}: raw Main draws"
        );
        assert_eq!(
            sim.main_rng.native_state_hex(),
            output["main_after_hex"].as_str().unwrap(),
            "{name}: full Main continuation"
        );
        assert_eq!(
            sim.scenario_rng.native_state_hex(),
            scenario,
            "{name}: Scenario unchanged"
        );
        assert_eq!(
            sim.mapgen_rng.native_state_hex(),
            mapgen,
            "{name}: MapGen unchanged"
        );
        assert_eq!(
            sim.state_hash(),
            hash,
            "{name}: voice owns no simulation state"
        );
        assert_eq!(output["timer_start_calls"], 0, "{name}");
        assert_eq!(output["detach_calls"], 0, "{name}");
        for expected in output["main_next_four"].as_array().unwrap() {
            assert_eq!(
                u64::from(sim.main_rng.next_u32()),
                expected.as_u64().unwrap(),
                "{name}: continuation"
            );
        }
    }
    assert_eq!(
        (full_requests, unbound_vector_kernel, native_faults),
        (19, 1, 1)
    );
}

#[test]
fn attack_voice_selects_for_the_committed_target_without_retargeting_the_actor() {
    use crate::map::entities::EntityCategory;
    use crate::sim::combat::{Health, TargetKind};
    use crate::sim::game_entity::GameEntity;

    let corpus = crate::test_fixture::json("tools/spatial_oracle/attack_order_voice.json");
    let native_voice = |name: &str| {
        let index = corpus["voice_controls"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["name"] == name)
            .unwrap()["output"]["queued_voice_after"]
            .as_i64()
            .unwrap();
        corpus["readers"]["sound"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["fixture_index"] == index)
            .unwrap()["name"]
            .as_str()
            .unwrap()
    };
    let primary = native_voice("primary_override");
    let secondary = native_voice("secondary_override");
    // These authored target/Verses priors exercise the existing selection
    // owner's live target arm. The voice values are the original7090A0
    // slot controls, not a new spatial/weapon-selection golden.
    let ini = IniFile::from_str(&format!(
        "[VehicleTypes]\n0=MTNK\n1=OLD\n2=NEW\n\
         [MTNK]\nStrength=300\nPrimary=P\nSecondary=S\n\
         VoicePrimaryWeaponAttack={primary}\nVoiceSecondaryWeaponAttack={secondary}\n\
         [OLD]\nStrength=300\nArmor=none\n[NEW]\nStrength=300\nArmor=heavy\n\
         [P]\nDamage=65\nProjectile=Q\nWarhead=PRIMARY_WH\n\
         [S]\nDamage=65\nProjectile=Q\nWarhead=SECONDARY_WH\n\
         [Q]\nAG=yes\nAA=no\n\
         [PRIMARY_WH]\nVerses=100%,100%,100%,100%,100%,0%,100%,100%,100%,100%,100%\n\
         [SECONDARY_WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n"
    ));
    let sounds = SoundRegistry::from_ini(&IniFile::from_str(&format!(
        "[SoundList]\n0={primary}\n1={secondary}\n"
    )));
    let mut rules = RuleSet::from_ini(&ini).unwrap();
    rules.bind_type_sound_references(&ini, &sounds);
    let (mut sim, rules, actor) =
        scene_for_rules(rules, &serde_json::json!({}), "MTNK", EntityCategory::Unit);
    let owner = sim.interner.get("Other").unwrap();
    for (id, type_name) in [(2, "OLD"), (3, "NEW")] {
        let kind = sim.interner.intern(type_name);
        sim.substrate
            .entities
            .insert(GameEntity::new_at_frame_zero_for_test(
                id,
                id as u16,
                0,
                0,
                0,
                owner,
                Health::new(300),
                kind,
                EntityCategory::Unit,
                0,
                5,
                false,
            ));
    }
    sim.assign_target_represented(
        actor,
        Some(TargetKind::Entity(2)),
        Some(&rules),
        None,
        crate::sim::world::FrameEffects::empty(),
    )
    .unwrap();
    for (target, expected) in [
        (None, primary),
        (Some(TargetKind::Entity(2)), primary),
        (Some(TargetKind::Entity(3)), secondary),
        (Some(TargetKind::Cell(3, 3)), primary),
    ] {
        let main = sim.main_rng.native_state_hex();
        let world = sim.state_hash();
        let request = sim.order_voice_request(&rules, actor, "VoiceAttack", target, true);
        assert_eq!(request, Some(expected), "committed target {target:?}");
        assert_eq!(
            sim.entities().get(actor).unwrap().attack_target.target(),
            Some(TargetKind::Entity(2))
        );
        assert_eq!(
            sim.main_rng.native_state_hex(),
            main,
            "scalar override draws none"
        );
        assert_eq!(sim.state_hash(), world, "voice must not retarget the actor");
    }
}

#[test]
fn capture_enter_and_harvest_preserve_scalar_precedence_and_move_fallback() {
    let native = native_voice_corpus();
    let move_draws = &native["voice_controls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == "move_singleton")
        .unwrap()["output"]["main_draws"];
    for (body, capture, enter, harvest) in [
        (
            "VoiceMove=Move\nVoiceEnter=Enter\nVoiceCapture=Capture\nVoiceHarvest=Harvest\n",
            Some("Capture"),
            Some("Enter"),
            Some("Harvest"),
        ),
        (
            "VoiceMove=Move\nVoiceEnter=Enter\n",
            Some("Enter"),
            Some("Enter"),
            Some("Move"),
        ),
        ("VoiceMove=Move\n", Some("Move"), Some("Move"), Some("Move")),
        ("", None, None, None),
    ] {
        for (field, expected) in [
            ("VoiceCapture", capture),
            ("VoiceEnter", enter),
            ("VoiceHarvest", harvest),
        ] {
            let (sim, rules, id) = scene(body);
            let (request, draws) =
                trace_draws(|| sim.order_voice_request(&rules, id, field, None, true));
            assert_eq!(request, expected, "{field} with {body:?}");
            let expected_draws = if expected == Some("Move") {
                move_draws.clone()
            } else {
                serde_json::json!([])
            };
            assert_eq!(
                serde_json::json!(draws.iter().map(|draw| &draw["value"]).collect::<Vec<_>>()),
                expected_draws,
                "{field}: only the reached native Move fallback spends Main"
            );
        }
    }
}

#[test]
fn disabled_order_voice_rejects_before_move_or_fallback_is_reached() {
    let (sim, rules, id) = scene("VoiceMove=Move\n");
    for field in [
        "VoiceMove",
        "VoiceAttack",
        "VoiceCapture",
        "VoiceEnter",
        "VoiceHarvest",
    ] {
        let before = sim.main_rng.native_state_hex();
        let (sound, draws) =
            trace_draws(|| sim.order_voice_request(&rules, id, field, None, false));
        assert_eq!(sound, None, "{field}");
        assert!(draws.is_empty(), "{field}");
        assert_eq!(sim.main_rng.native_state_hex(), before, "{field}");
    }
}

#[test]
fn allied_object_move_voice_matches_native_rng_and_queue() {
    let corpus = native_voice_corpus();
    let mut singleton = 0;
    let mut multiple = 0;
    let mut empty = 0;
    let mut caller_disabled = 0;
    let mut nonhuman_draw_then_reject = 0;
    let mut direct_disabled = 0;
    let mut list_kernel = 0;
    for row in corpus["voice_controls"].as_array().unwrap() {
        let name = row["name"].as_str().unwrap();
        let input = &row["input"];
        let output = &row["output"];
        let ids = input["voice_ids"].as_array().unwrap();
        let voices: Vec<_> = ids
            .iter()
            .map(|id| match id.as_i64().unwrap() {
                -1 => String::new(),
                id => format!("Sound{id}"),
            })
            .collect();
        let enabled = input["voices_enabled"].as_bool().unwrap();
        let entry = input["caller_entry"].as_str().unwrap();
        let entry = u32::from_str_radix(entry.trim_start_matches("0x"), 16).unwrap();
        assert!(matches!(entry, 0x006f_fbe0 | 0x0070_8fc0), "{name}");
        let is_direct = entry == 0x0070_8fc0;
        let body = format!("VoiceMove={}\n", voices.join(","));
        let (sim, rules, id) = scene_with_input(&body, input);
        let human = sim.house_is_human_player(sim.entities().get(id).unwrap().owner());
        assert_eq!(
            sim.main_rng.native_state_hex(),
            output["main_before_hex"].as_str().unwrap(),
            "{name}: seeded native caller prior"
        );
        let scenario = sim.scenario_rng.logical_state();
        let mapgen = sim.mapgen_rng.logical_state();
        let world_hash = sim.state_hash();
        // Every reader-producible vector reaches the production rules/API.
        // Only a direct function entry or an explicit unbound native ID -1
        // needs the existing kernel seam; ReadSoundList cannot produce -1.
        let kernel_only = is_direct || voices == [String::new()];
        let (request, draws) = trace_draws(|| {
            if kernel_only {
                if enabled || is_direct {
                    sim.voice_request_from_list(&voices, enabled, human)
                } else {
                    None
                }
            } else {
                sim.order_voice_request(&rules, id, "VoiceMove", None, enabled)
            }
        });
        let queued = request.map_or_else(
            || output["queued_voice_before"].as_i64().unwrap(),
            |sound| sound.strip_prefix("Sound").unwrap().parse::<i64>().unwrap(),
        );
        assert_eq!(
            queued,
            output["queued_voice_after"].as_i64().unwrap(),
            "{name}: native QueueVoice result"
        );
        assert_eq!(
            serde_json::json!(draws.iter().map(|draw| &draw["value"]).collect::<Vec<_>>()),
            output["main_draws"],
            "{name}: actual raw Main draws"
        );
        assert_eq!(
            sim.main_rng.native_state_hex(),
            output["main_after_hex"].as_str().unwrap(),
            "{name}: complete Main continuation"
        );
        assert_eq!(sim.scenario_rng.logical_state(), scenario, "{name}");
        assert_eq!(sim.mapgen_rng.logical_state(), mapgen, "{name}");
        assert_eq!(sim.state_hash(), world_hash, "{name}");
        assert_eq!(output["scenario_unchanged"], true, "{name}");
        assert_eq!(output["mapgen_unchanged"], true, "{name}");
        for expected in output["main_next_four"].as_array().unwrap() {
            assert_eq!(
                u64::from(sim.main_rng.next_u32()),
                expected.as_u64().unwrap()
            );
        }
        if is_direct && !enabled {
            direct_disabled += 1;
        } else if !enabled {
            caller_disabled += 1;
        } else if voices.is_empty() {
            empty += 1;
        } else if kernel_only {
            list_kernel += 1;
        } else if !human {
            nonhuman_draw_then_reject += 1;
        } else if voices.len() > 1 {
            multiple += 1;
        } else {
            singleton += 1;
        }
    }
    for (name, count) in [
        ("singleton production request", singleton),
        ("multiple-entry production request", multiple),
        ("empty production list", empty),
        ("disabled caller before VoiceMove", caller_disabled),
        (
            "nonhuman caller rejection after draw",
            nonhuman_draw_then_reject,
        ),
        ("disabled direct VoiceMove after draw", direct_disabled),
        ("broader native list kernel controls", list_kernel),
    ] {
        assert!(count > 0, "missing native {name} control");
    }
}
