//! Wakeup queues Guard through the existing Mission authority. Native
//! Execute6DDB67 ->6E01C0 uses the live Trigger's Tag membership, not a House.
use super::*;
use crate::sim::mission::{MissionId, MissionType};
use crate::util::save_codec;

fn corpus() -> &'static serde_json::Value {
    let value = crate::test_fixture::json("tools/spatial_oracle/trigger_wakeup.json");
    assert_eq!(
        value["native_sha256"],
        "3e81a61775d2745d1dabe397325ef663cd994ffc194da4e998e3bf5d2d308600"
    );
    assert_eq!(value["controls"].as_array().unwrap().len(), 57);
    assert!(value["code_unchanged"].as_bool().unwrap());
    value
}

fn int(value: &serde_json::Value) -> i32 {
    value.as_i64().unwrap().try_into().unwrap()
}

#[test]
fn wakeup_selection_and_deferred_mission_state_match_original_controls() {
    use crate::map::entities::EntityCategory;
    use crate::sim::combat::Health;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::mission::{MissionDispatchTimer, state::MissionTestFixture};
    let native = corpus();
    let prior = &native["mission_priors"];
    for control in native["controls"].as_array().unwrap() {
        let name = control["name"].as_str().unwrap();
        let (mut sim, rules, _, houses) = super::tests::event_ingress_fixture();
        let kind = int(&control["action_kind"]);
        super::tests::install_event_ingress_map(
            &mut sim,
            &rules,
            &format!(
                "[Triggers]\nWAKE=Country0,<none>,Wake,0,1,1,1,0\n\
                 [Events]\nWAKE=1,8,0,0\n\
                 [Actions]\nWAKE=1,{}\n\
                 [Tags]\nTAG=2,Wake,WAKE\n",
                control["raw_record"].as_str().unwrap()
            ),
        );
        // Supplied Tag links, not an original Tag construction comparison.
        // All identities deliberately share ONE TriggerType, as in native.
        let tag_type = sim.trigger_runtime.definitions.tag_type("TAG").unwrap();
        let live: Vec<_> = (0..8)
            .map(|_| {
                let tag = sim.construct_trigger_tag(tag_type);
                sim.trigger_runtime.tag(tag).unwrap().triggers[0]
            })
            .collect();
        let typ = sim.trigger_runtime.trigger(live[0]).unwrap().type_index;
        let action = sim.trigger_runtime.definitions.trigger_types[typ].actions[0].clone();
        assert_eq!(action.value, Some(int(&control["native_read"]["value"])));
        let actors = control["actors"].as_array().unwrap();
        let order = control[if kind == 81 {
            "techno_order"
        } else {
            "foot_order"
        }]
        .as_array()
        .unwrap();
        // Translate declared native registration order to monotonic stable
        // IDs; an allocated actor absent from the list stays outside the store.
        let mut construction: Vec<_> = order.iter().map(|v| int(v) as usize).collect();
        for index in 0..actors.len() {
            if !construction.contains(&index) {
                construction.push(index);
            }
        }
        let mut originals = Vec::new();
        let mut ids = vec![0; actors.len()];
        for index in construction {
            let input = &actors[index];
            let id = sim.allocate_stable_id();
            ids[index] = id;
            let category = match input["category"].as_str().unwrap() {
                "infantry" => EntityCategory::Infantry,
                "unit" => EntityCategory::Unit,
                "aircraft" => EntityCategory::Aircraft,
                "building" => EntityCategory::Structure,
                other => panic!("unknown native class {other}"),
            };
            // No Wakeup receiver reads a Type; these are declared leaf priors.
            let type_id = sim.interner.intern("NATIVE_WAKEUP_CONTROL");
            let mut entity = GameEntity::new_at_frame_zero_for_test(
                id,
                10,
                10,
                0,
                0,
                houses[int(&input["house"]) as usize],
                Health::new(int(&input["health"])),
                type_id,
                category,
                0,
                5,
                false,
            );
            entity
                .lifecycle
                .set_object_alive_for_test(input["alive"].as_bool().unwrap());
            entity
                .lifecycle
                .set_cell_marked(input["cell_marked"].as_bool().unwrap());
            entity
                .lifecycle
                .set_in_limbo_for_test(input["limbo"].as_bool().unwrap());
            entity.mission.apply_test_fixture(MissionTestFixture {
                current: MissionId::from_raw(int(&input["current"])),
                suspended: MissionId::from_raw(int(&prior["suspended"])),
                queued: MissionId::from_raw(int(&input["queued"])),
                movement_bypass_latch: int(&input["bypass"]) as u8,
                handler_state: int(&prior["handler_state"]) as u32,
                mission_start_frame: int(&prior["mission_start_frame"]) as u32,
                ai_counter: int(&prior["ai_counter"]) as u32,
                dispatch_timer: MissionDispatchTimer::from_raw(
                    int(&prior["dispatch_start"]),
                    int(&prior["dispatch_delay"]),
                ),
            });
            if let Some(chain) = input["tag_chain"].as_array() {
                let tag = sim.trigger_runtime.tags.len();
                sim.trigger_runtime.tags.push(Some(LiveTag {
                    type_index: tag_type,
                    triggers: chain.iter().map(|v| live[int(v) as usize]).collect(),
                    refs: 0,
                    attached_cell: (0, 0),
                    pending: false,
                    dispatch_guard: false,
                }));
                sim.trigger_runtime.tag_order.push(tag);
                sim.replace_trigger_tag_attachment(TagAttachment::Object(id), Some(tag));
            }
            originals.push((index, entity.clone()));
            if order.iter().any(|v| int(v) as usize == index) {
                sim.substrate.entities.insert(entity);
            }
        }
        let rng = sim.rng_state();
        let result = sim.apply_live_trigger_action(
            &action,
            TriggerActionContext {
                live_index: control["trigger"].as_u64().map(|v| live[v as usize]),
                incoming_house: control["incoming_house"]
                    .as_u64()
                    .map(|v| houses[v as usize]),
                input: TagEventInput::default(),
            },
            &mut Vec::new(),
            None,
            None,
            FrameEffects::default(),
        );
        assert_eq!(result, int(&control["ret"]) != 0, "{name}");
        for (index, mut expected) in originals {
            let actual = sim.substrate.entities.get(ids[index]).unwrap_or(&expected);
            let after = &control["after"][index];
            let fixture = MissionTestFixture {
                current: MissionId::from_raw(int(&after["current"])),
                suspended: expected.mission.suspended(),
                queued: MissionId::from_raw(int(&after["queued"])),
                movement_bypass_latch: int(&after["bypass"]) as u8,
                handler_state: expected.mission.handler_state(),
                mission_start_frame: expected.mission.mission_start_frame(),
                ai_counter: expected.mission.ai_counter(),
                dispatch_timer: expected.mission.dispatch_timer(),
            };
            // Serialize the observed actor before mutating the expected prior.
            let actual = save_codec::serialize(actual).unwrap();
            expected.mission.apply_test_fixture(fixture);
            assert_eq!(
                actual,
                save_codec::serialize(&expected).unwrap(),
                "{name} actor{index}"
            );
        }
        assert_eq!(sim.rng_state(), rng, "{name}");
        assert_eq!(control["rng_before"], control["rng_after_read"], "{name}");
        assert_eq!(control["rng_before"], control["rng_after"], "{name}");
        assert!(control["rng_calls"].as_array().unwrap().is_empty());
        assert_eq!(control["mission_timers_unchanged"], true);
        assert_eq!(control["other_actor_bytes_unchanged"], true);
    }
}

#[test]
fn wakeup_tagged_sleep_queues_guard_without_commencing() {
    let (mut sim, rules, _, houses) = super::tests::event_ingress_fixture();
    super::tests::install_event_ingress_map(
        &mut sim,
        &rules,
        "[Triggers]\nWAKE=Country0,<none>,Wake,0,1,1,1,0\n\
         [Events]\nWAKE=1,8,0,0\n\
         [Actions]\nWAKE=1,81,0,0,0,0,0,0,A\n\
         [Tags]\nTAG=2,Wake,WAKE\n",
    );
    let actor = super::tests::spawn_event_ingress_unit(&mut sim, &rules, houses[1], (15, 15));
    sim.attach_object_trigger_tag(actor, Some("TAG"));
    sim.mission_assign_exact(actor, MissionId::from_known(MissionType::Sleep), 17)
        .unwrap();
    let before = sim.substrate.entities.get(actor).unwrap().mission;
    let rng = sim.scenario_rng.native_state_hex();
    let live = sim.trigger_runtime.trigger_order[0];
    let typ = sim.trigger_runtime.trigger(live).unwrap().type_index;
    let action = sim.trigger_runtime.definitions.trigger_types[typ].actions[0].clone();
    sim.apply_live_trigger_action(
        &action,
        TriggerActionContext {
            live_index: Some(live),
            incoming_house: Some(houses[0]),
            input: TagEventInput::default(),
        },
        &mut Vec::new(),
        Some(&rules),
        None,
        FrameEffects::default(),
    );
    let after = sim.substrate.entities.get(actor).unwrap().mission;
    assert_eq!(after.current(), before.current());
    assert_eq!(after.queued().known(), Some(MissionType::Guard));
    assert_eq!(after.dispatch_timer(), before.dispatch_timer());
    assert_eq!(after.mission_start_frame(), before.mission_start_frame());
    assert_eq!(sim.scenario_rng.native_state_hex(), rng);
}
#[test]
fn retail_all02_prisoner_damage_enable_elapsed_wakeup_survives_restore() {
    use crate::map::{entities::EntityCategory, map_file::MapFile};
    use crate::rules::{process_owner::NativeScenarioRulesPrefix, team_ai_ini::TeamAiIniRegistry};
    use crate::sim::mission::MissionType;
    use crate::sim::snapshot::GameSnapshot;

    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/trigger_wakeup.json",
    ))
    .unwrap();
    let native = &corpus["retail"];
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let raw_map = assets.get_ref("ALL02UMD.MAP").unwrap();
    assert_eq!(
        crate::util::sha256::sha256_hex(raw_map),
        native["map_sha256"]
    );
    assert_eq!(
        crate::util::sha256::sha256_hex(assets.get_ref("RULESMD.INI").unwrap()),
        native["rules_sha256"]
    );
    let map = MapFile::from_bytes(raw_map).unwrap();
    let named = assets.resolve_ref("ALL02UMD.INI").map(|_| {
        crate::rules::retail_sources::select_ini(&assets, "ALL02UMD.INI")
            .unwrap()
            .ini
    });
    let mut owner = crate::rules::retail_ini_fixture::retail_rules_owner(&assets);
    let (rules, processed, art, _) = owner
        .load_scenario(
            NativeScenarioRulesPrefix::Campaign(named.as_ref()),
            &map.ini,
        )
        .unwrap()
        .into_parts();
    let overlays =
        crate::rules::overlay_types::OverlayTypeRegistry::from_ini(&processed, Some(&art));
    let ai = crate::rules::retail_sources::select_ini(&assets, "AIMD.INI")
        .unwrap()
        .ini;
    let teams = TeamAiIniRegistry::from_sources(&ai, &map.ini, false);
    for row in native["mission_from_name"].as_array().unwrap() {
        assert_eq!(
            MissionType::from_map_name(row["name"].as_str().unwrap()).unwrap() as i32,
            row["native_id"].as_i64().unwrap() as i32
        );
    }

    // Keep the source's second Enable target so its actual reference resolves.
    // Its elapsed1 -> panic Team is a separate later mechanism: this cropped
    // replay ends at frame1, before that 15-frame timer can expire.
    let chain = ["095D099C", "0958149C", "095D033C"];
    let mut body = String::new();
    for section in ["Triggers", "Events", "Actions", "Tags"] {
        body.push_str(&format!("[{section}]\n"));
        for (key, value) in map.ini.section(section).unwrap().raw_entries() {
            let selected = if section == "Tags" {
                map.tags
                    .get(key)
                    .is_some_and(|tag| tag.trigger().is_some_and(|id| chain.contains(&id)))
            } else {
                chain.contains(&key)
            };
            if selected {
                body.push_str(&format!("{key}={value}\n"));
            }
        }
    }
    for id in chain {
        for (section, field) in [
            ("Triggers", "trigger"),
            ("Events", "events_raw"),
            ("Actions", "actions_raw"),
        ] {
            assert_eq!(
                map.ini.section(section).unwrap().get_for_test(id),
                native["routes"][id][field].as_str()
            );
        }
    }
    let roster =
        crate::map::houses::parse_house_roster(&map.ini, &rules.color_schemes, Some(&rules));
    let mut sim = Simulation::with_seed(31);
    crate::sim::scenario_bootstrap::initialize_map_roster_houses(
        &mut sim,
        &roster,
        Some(&rules),
        Some(&map.ini),
    );
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.session.game_mode_nonzero = false;
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    for row in native["houses"].as_array().unwrap() {
        assert_eq!(
            sim.house_by_country_index(
                crate::rules::ruleset::CountryIdx(row["native_country"].as_u64().unwrap() as u16),
                &rules,
            ),
            sim.interner.get(row["name"].as_str().unwrap())
        );
    }
    let cropped = super::tests::install_event_ingress_map(&mut sim, &rules, &body);
    sim.install_team_ai_registry(&teams, &rules).unwrap();
    sim.initialize_trigger_definitions(&cropped, &teams, None, None);
    sim.classify_map_triggers(Some(&rules));

    for id in [chain[0], chain[1]] {
        for raw in native["routes"][id]["attachments"].as_array().unwrap() {
            assert_eq!(
                map.ini
                    .section(raw["section"].as_str().unwrap())
                    .unwrap()
                    .get_for_test(raw["key"].as_str().unwrap()),
                raw["raw"].as_str()
            );
        }
    }
    let mut placements: Vec<_> = map
        .entities
        .iter()
        .filter(|entity| matches!(entity.trigger_tag.as_deref(), Some("0958AF4C" | "095D042C")))
        .cloned()
        .collect();
    assert_eq!(placements.len(), 15);
    // Explicit geometry prior: translate the authored15 actors into the clear
    // 32x32 arena. Keep type/owner/health256/subcell/facing/mission/Tag and
    // recruitment columns. This is not original terrain or whole ALL02.
    for placement in &mut placements {
        assert_eq!(placement.category, EntityCategory::Infantry);
        placement.cell_x -= 140;
        placement.cell_y -= 85;
    }
    assert_eq!(sim.spawn_from_map(&placements, Some(&rules)), 15);
    let attached_ids = |sim: &Simulation, tag: usize| -> Vec<u64> {
        sim.substrate
            .entities
            .values()
            .filter_map(|entity| {
                (sim.trigger_runtime
                    .attachments
                    .get(&TagAttachment::Object(entity.stable_id()))
                    == Some(&tag))
                .then_some(entity.stable_id())
            })
            .collect()
    };
    let live = |sim: &Simulation, name: &str| {
        sim.trigger_runtime
            .trigger_order
            .iter()
            .copied()
            .find(|&index| {
                sim.trigger_runtime.trigger(index).is_some_and(|trigger| {
                    sim.trigger_runtime.definitions.trigger_types[trigger.type_index].id == name
                })
            })
            .unwrap()
    };
    let source_live = live(&sim, chain[0]);
    let wake_live = live(&sim, chain[1]);
    for (id, index) in [(chain[0], source_live), (chain[1], wake_live)] {
        assert_eq!(
            sim.trigger_runtime.trigger(index).unwrap().enabled,
            native["routes"][id]["initial_enabled"].as_u64().unwrap() != 0
        );
    }
    let source_tag = sim
        .trigger_runtime
        .attachments
        .iter()
        .find_map(|(attachment, &tag)| {
            matches!(attachment, TagAttachment::Object(id) if sim.substrate.entities.get(*id)
            .is_some_and(|entity| sim.interner.resolve(entity.type_ref()) == "INIT"))
            .then_some(tag)
        })
        .unwrap();
    let wake_tag = sim
        .trigger_runtime
        .tag_order
        .iter()
        .copied()
        .find(|&tag| {
            sim.trigger_runtime
                .tag(tag)
                .unwrap()
                .triggers
                .contains(&wake_live)
        })
        .unwrap();
    let prisoners = attached_ids(&sim, wake_tag);
    let guards = attached_ids(&sim, source_tag);
    assert_eq!(prisoners.len(), 7);
    assert_eq!(guards.len(), 8);
    assert_eq!(sim.trigger_runtime.tag(wake_tag).unwrap().refs, 7);
    let poll = |sim: &mut Simulation| {
        sim.advance_triggers(Some(&rules), Some(&overlays), FrameEffects::default());
    };
    poll(&mut sim);
    assert!(sim.trigger_runtime.action_trace.is_empty());
    for &id in &prisoners {
        assert_eq!(
            sim.substrate
                .entities
                .get(id)
                .unwrap()
                .mission
                .current()
                .known(),
            Some(MissionType::Sleep)
        );
    }
    let attacker = sim
        .spawn_object_at_height("E1", "Americans", 16, 16, 0, 0, &rules)
        .unwrap();
    super::tests::event_ingress_damage(
        &mut sim,
        &rules,
        &overlays,
        guards[0],
        attacker,
        1,
        FrameEffects::default(),
    );
    assert!(sim.trigger_runtime.trigger(wake_live).unwrap().enabled);
    assert_eq!(
        sim.trigger_runtime
            .action_trace
            .iter()
            .map(|row| row.0)
            .collect::<Vec<_>>(),
        [53, 53]
    );
    assert!(sim.trigger_runtime.trigger(source_live).unwrap().pending);

    // Use the existing native in-scenario load seed0 boundary. This checkpoint
    // deliberately retains pending source Tag/Trigger finalization and all7
    // shared prisoner references, before the enabled elapsed0 poll.
    sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let bytes = GameSnapshot::save(&sim, 0, 0, "ALL02UMD.MAP", 0);
    let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
    restored.restore_after_snapshot_load().unwrap();
    crate::sim::arena_fixture::flat_ground(&mut restored, &rules);
    assert_eq!(sim.trigger_runtime, restored.trigger_runtime);
    for candidate in [&mut sim, &mut restored] {
        let before: Vec<_> = prisoners
            .iter()
            .map(|&id| candidate.substrate.entities.get(id).unwrap().mission)
            .collect();
        let rng = candidate.rng_state();
        poll(candidate);
        assert_eq!(
            candidate
                .trigger_runtime
                .action_trace
                .iter()
                .filter(|row| row.0 == 81)
                .count(),
            1
        );
        for (&id, before) in prisoners.iter().zip(before) {
            let entity = candidate.substrate.entities.get(id).unwrap();
            assert_eq!(entity.mission.current(), before.current());
            assert_eq!(entity.mission.queued().known(), Some(MissionType::Guard));
            assert_eq!(entity.mission.dispatch_timer(), before.dispatch_timer());
            assert_eq!(entity.owner(), candidate.interner.get("Neutral").unwrap());
            assert!(
                !candidate
                    .trigger_runtime
                    .attachments
                    .contains_key(&TagAttachment::Object(id))
            );
        }
        assert_eq!(candidate.rng_state(), rng);
        candidate.process_pending_delete_with(
            Some(&rules),
            Some(&overlays),
            FrameEffects::default(),
            1,
            false,
        );
        assert!(!candidate.trigger_runtime.has_live_tag(source_tag));
        assert!(!candidate.trigger_runtime.has_live_tag(wake_tag));
        assert!(candidate.trigger_runtime.trigger(source_live).is_none());
        assert!(candidate.trigger_runtime.trigger(wake_live).is_none());
        candidate.session.binary_frame = 1;
        for &id in &prisoners {
            candidate.mission_host_promote(id, 1, &rules);
            let mission = candidate.substrate.entities.get(id).unwrap().mission;
            assert_eq!(mission.current().known(), Some(MissionType::Guard));
            assert_eq!(mission.queued(), crate::sim::mission::MissionId::NONE);
        }
        poll(candidate);
        assert_eq!(
            candidate
                .trigger_runtime
                .action_trace
                .iter()
                .filter(|row| row.0 == 81)
                .count(),
            1
        );
    }
    assert_eq!(sim.trigger_runtime, restored.trigger_runtime);
    for &id in &prisoners {
        assert_eq!(
            sim.substrate.entities.get(id).unwrap().mission,
            restored.substrate.entities.get(id).unwrap().mission
        );
    }
}
