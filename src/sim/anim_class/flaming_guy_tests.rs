//! Original FLAMEGUY component histories plus the stock OilExplosion producer.
//! Register this as a child of `flaming_guy`, keeping production helpers private.

use super::*;
use crate::map::playfield::PlayfieldBounds;
use crate::map::resolved_terrain::{ResolvedTerrainGrid, test_flat_cell};
use crate::rules::art_data::ArtRegistry;
use crate::rules::ini_parser::IniFile;
use crate::sim::anim_class::{AnimClassSpawnDescriptor, AnimObject};
use crate::sim::rng::SimRng;
use crate::sim::world::FrameEffects;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::hash::Hasher;

fn corpus() -> &'static Value {
    crate::test_fixture::json("tools/spatial_oracle/flaming_guy.json")
}

fn rules_from_corpus(native: &Value) -> RuleSet {
    // Physical lexical ART and original reader frame counts are independent
    // inputs; no expected movement, stage, or RNG result becomes a fixture prior.
    let mut text = String::new();
    for (name, section) in native["lexical_art"].as_object().unwrap() {
        text.push_str(&format!("[{name}]\n"));
        for (key, value) in section.as_object().unwrap() {
            text.push_str(&format!("{key}={}\n", value.as_str().unwrap()));
        }
    }
    let art_ini = IniFile::from_str(&text);
    let rows = native["rows"].as_array().unwrap();
    let animations = rows[0]["reader"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .map(|(i, row)| format!("{i}={}\n", row["name"].as_str().unwrap()))
        .collect::<String>();
    let splashes = rows[0]["splash_list"]
        .as_array()
        .unwrap()
        .iter()
        .map(|name| name.as_str().unwrap())
        .collect::<Vec<_>>()
        .join(",");
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
        &IniFile::from_str(&format!(
            "[Animations]\n{animations}[CombatDamage]\nSplashList={splashes}\n"
        )),
        &art_ini,
    )
    .unwrap();
    let mut art = ArtRegistry::from_ini(&art_ini);
    for row in rows[0]["reader"].as_array().unwrap() {
        if row["image_present"] == true {
            art.bind_anim_frame_count_for_test(
                row["name"].as_str().unwrap(),
                row["raw_frames"].as_i64().unwrap() as i32,
            );
        }
    }
    rules.install_art_fixture(art);
    rules
}

fn xy(row: &Value) -> (u16, u16) {
    (
        row["xy"][0].as_u64().unwrap() as u16,
        row["xy"][1].as_u64().unwrap() as u16,
    )
}

fn scene(rules: &RuleSet, input: &Value) -> (Simulation, AnimId) {
    let mut sim = Simulation::with_seed(input["seed"].as_u64().unwrap_or(1));
    sim.session.game_options_mut_for_test().game_speed = 4;
    sim.session.set_map_width_for_test(48);
    sim.session.set_map_height_for_test(48);
    // The native sparse closure allocates 16..48. These histories remain in
    // that interior; the larger flat Rust allocation adds no admission data
    // used by the measured paths.
    let cells = (0..64)
        .flat_map(|y| (0..64).map(move |x| test_flat_cell(x, y)))
        .collect();
    sim.install_resolved_terrain_for_new_map(ResolvedTerrainGrid::from_cells(64, 64, cells));
    sim.set_playfield_bounds_for_test(Some(PlayfieldBounds {
        base: 48,
        off_fc: 0,
        off_100: 0,
        off_104: 48,
        off_108: 48,
    }));
    sim.set_playfield_size_height_for_test(Some(48));
    if input["blocked"] == true {
        for y in 16..=48 {
            for x in 16..=48 {
                sim.substrate.raw_cell_occupation.mark_ground(x, y, 0xE0);
            }
        }
    }
    for row in input["cells"].as_array().into_iter().flatten() {
        let (x, y) = xy(row);
        let cell = sim
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut_for_test(x, y)
            .unwrap();
        if let Some(land) = row["land"].as_u64() {
            cell.land_type = land as u8;
        }
        if let Some(level) = row["level"].as_i64() {
            cell.level = level as u8;
        }
        if let Some(flags) = row["flags"].as_u64() {
            cell.bridge_facts.raw_flags = flags as u32;
        }
        if let Some(bits) = row["ground_occupation"].as_u64() {
            sim.substrate
                .raw_cell_occupation
                .mark_ground(x, y, bits as u8);
        }
        if let Some(bits) = row["alt_occupation"].as_u64() {
            sim.substrate
                .raw_cell_occupation
                .mark_deck(x, y, bits as u8);
        }
    }
    let coord = AnimWorldCoord {
        x: 32 * 256 + 128,
        y: 32 * 256 + 128,
        z: input["z"].as_i64().unwrap_or(0) as i32,
    };
    let ty = sim.interner.intern("FLAMEGUY");
    let (rx, ry, sx, sy, z) = coord.to_cell_sub_z();
    if let Some(hex) = input["scenario_before_constructor"].as_str() {
        sim.scenario_rng = SimRng::from_native_state_hex_for_test(hex);
    }
    let id = sim
        .spawn_anim_at_world(
            rules,
            AnimClassSpawnDescriptor {
                draw_flags: 0x600,
                ..AnimClassSpawnDescriptor::new(ty, rx, ry, sx, sy, z)
            },
            coord,
        )
        .expect("reader-bound FLAMEGUY constructor");
    (sim, id)
}

fn state(sim: &Simulation, id: AnimId) -> Value {
    let anim = sim.anim(id).unwrap();
    let runner = anim.flaming_guy.clone().unwrap_or_default();
    let destination = runner
        .destination
        .unwrap_or(AnimWorldCoord { x: 0, y: 0, z: 0 });
    json!({
        "location": [anim.world_coord.x, anim.world_coord.y, anim.world_coord.z],
        "destination": [destination.x, destination.y, destination.z],
        "frame": anim.runtime.current_frame,
        "rate": anim.runtime.rate_reload,
        "step": anim.runtime.frame_step,
        "timer_start": anim.runtime.frame_timer.start_frame(),
        "timer_duration": anim.runtime.frame_timer.duration(),
        "retries": runner.retries,
        "falling": runner.fall.falling(),
        "fall_rate": runner.fall.rate(),
        "expired": runner.expired,
        "alive": !sim.substrate.pending_delete.contains_object(&id),
        "inactive": anim.runtime.inactive,
        "first_ai": anim.runtime.first_ai_guard,
        "in_logic": sim.logic_order().contains(&id),
    })
}

fn compare_state(actual: &Value, expected: &Value, label: &str) {
    for (key, value) in actual.as_object().unwrap() {
        assert_eq!(value, &expected[key], "{label}: {key}");
    }
}

fn visit(sim: &mut Simulation, rules: &RuleSet, id: AnimId, frame: u32) {
    sim.session.set_binary_frame_for_test(frame);
    sim.visit_anim(id, rules, None, FrameEffects::default());
}

#[test]
fn physical_flaming_guy_methods_and_full_ai_histories_match_original() {
    let native = corpus();
    let rules = rules_from_corpus(&native);
    let mut counts = [0; 3];
    for row in native["rows"].as_array().unwrap() {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        assert_eq!(row["guards"]["original_code_unchanged"], true, "{name}");
        assert_eq!(row["guards"]["vtables_unchanged"], true, "{name}");
        let mut scene_input = input.clone();
        scene_input["scenario_before_constructor"] =
            row["constructor_rng_before"]["scenario"].clone();
        let (mut sim, id) = scene(&rules, &scene_input);
        compare_state(&state(&sim, id), &row["constructor"], name);
        assert_eq!(
            sim.scenario_rng.native_state_hex(),
            row["constructor_rng_after"]["scenario"].as_str().unwrap(),
            "{name}: constructor Scenario stream"
        );
        // The constructor was checked independently above. Import the full
        // measured continuation cursor rather than infer its draw count.
        sim.scenario_rng =
            SimRng::from_native_state_hex_for_test(row["rng_before"]["scenario"].as_str().unwrap());
        match input["kind"].as_str().unwrap() {
            "enter" => {
                counts[0] += 1;
                let target = input["target"].as_array().map_or((33, 32), |v| {
                    (v[0].as_i64().unwrap() as i16, v[1].as_i64().unwrap() as i16)
                });
                assert_eq!(
                    sim.flaming_guy_can_enter(sim.anim(id).unwrap().world_coord, target, None),
                    row["output"].as_bool().unwrap(),
                    "{name}"
                );
            }
            "next" => {
                counts[1] += 1;
                let coord = sim.anim(id).unwrap().world_coord;
                let next = sim
                    .flaming_guy_next_coord(coord, None)
                    .unwrap_or(AnimWorldCoord { x: 0, y: 0, z: 0 });
                assert_eq!(json!([next.x, next.y, next.z]), row["output"], "{name}");
            }
            "ai" => {
                counts[2] += 1;
                for expected in row["visits"].as_array().unwrap() {
                    let frame = expected["tick"].as_u64().unwrap() as u32;
                    visit(&mut sim, &rules, id, frame);
                    compare_state(&state(&sim, id), expected, &format!("{name}@{frame}"));
                }
                assert!(sim.substrate.pending_delete.contains_object(&id), "{name}");
                assert!(!sim.logic_order().contains(&id), "{name}");
                // Only primary AI is scheduled in the original control. Its
                // child constructors must nevertheless use the shared owner.
                let expected_children: Vec<_> = row["events"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|event| event["kind"] == "constructor" && event["phase"] == "control")
                    .map(|event| (event["type"].clone(), event["coordinate"].clone()))
                    .collect();
                let children: Vec<_> = sim
                    .anims()
                    .filter(|(child, _)| **child != id)
                    .map(|(_, anim)| {
                        (
                            json!(sim.interner.resolve(anim.type_id())),
                            json!([anim.world_coord.x, anim.world_coord.y, anim.world_coord.z]),
                        )
                    })
                    .collect();
                assert_eq!(children, expected_children, "{name} child constructors");
            }
            other => panic!("unhandled native control {other}"),
        }
        assert_eq!(
            sim.scenario_rng.native_state_hex(),
            row["rng_after"]["scenario"].as_str().unwrap(),
            "{name}: complete Scenario stream"
        );
    }
    assert_eq!(counts, [10, 5, 5]);
}

#[test]
fn running_visits_use_all_eight_directional_sequences_before_first_ai_gate() {
    let native = corpus();
    let rules = rules_from_corpus(&native);
    let running = rules
        .art()
        .anim_runtime_config("FLAMEGUY")
        .unwrap()
        .running_frames;
    assert!(running > 1);
    let mut sequences = BTreeSet::new();
    for direction in 0..8 {
        let (mut sim, id) = scene(&rules, &json!({}));
        let coord = sim.anim(id).unwrap().world_coord;
        let (dx, dy) = direction_delta(direction).unwrap();
        sim.substrate.anims.get_mut(id).unwrap().flaming_guy = Some(FlamingGuyState {
            destination: Some(AnimWorldCoord {
                x: coord.x + 1024 * dx,
                y: coord.y + 1024 * dy,
                z: 0,
            }),
            ..Default::default()
        });
        visit(&mut sim, &rules, id, 0);
        let first = sim.anim(id).unwrap();
        assert_ne!(
            first.world_coord, coord,
            "direction {direction} moves on first AI"
        );
        assert!(!first.runtime.first_ai_guard);
        assert_eq!(first.runtime.rate_reload, 0);
        let start = first.runtime.current_frame;
        assert_eq!(start % running, 0);
        sequences.insert(start / running);
        visit(&mut sim, &rules, id, 3);
        assert_eq!(sim.anim(id).unwrap().runtime.current_frame, start + 1);
    }
    assert_eq!(sequences, (0..8).collect());
}

#[test]
fn admission_reads_wall_flag_and_raw_plane_without_reserving_a_cell() {
    let native = corpus();
    let rules = rules_from_corpus(&native);
    let (mut sim, id) = scene(&rules, &json!({}));
    let overlays = OverlayTypeRegistry::from_ini(
        &IniFile::from_str("[OverlayTypes]\n0=FENCE\n[FENCE]\nWall=yes\n"),
        None,
    );
    let wall = overlays.id_for_name("FENCE").unwrap();
    sim.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(64, 64));
    sim.overlay_grid
        .as_mut()
        .unwrap()
        .place_overlay(33, 32, wall, 0);
    let coord = sim.anim(id).unwrap().world_coord;
    let raw = sim.substrate.raw_cell_occupation.clone();
    assert!(!sim.flaming_guy_can_enter(coord, (33, 32), Some(&overlays)));
    assert_eq!(sim.substrate.raw_cell_occupation, raw);
    // The bridge branch precedes the wall/ground branch and uses alternate
    // occupation; a ground wall must not deny an otherwise clear high deck.
    sim.resolved_terrain
        .as_mut()
        .unwrap()
        .cell_mut_for_test(33, 32)
        .unwrap()
        .bridge_facts
        .raw_flags = 0x100;
    let high = AnimWorldCoord {
        z: BRIDGE_DECK_HEIGHT_LEPTONS,
        ..coord
    };
    assert!(sim.flaming_guy_can_enter(high, (33, 32), Some(&overlays)));
    sim.substrate.raw_cell_occupation.mark_deck(33, 32, 0x20);
    assert!(!sim.flaming_guy_can_enter(high, (33, 32), Some(&overlays)));
}

fn anim_hash(anim: &AnimObject) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    anim.hash_before_display(&mut hasher);
    hasher.finish()
}

#[test]
fn runner_destination_progress_and_fall_survive_serde_and_affect_world_hash() {
    let native = corpus();
    let rules = rules_from_corpus(&native);
    let (mut sim, id) = scene(&rules, &json!({"z": 500}));
    for frame in 0..5 {
        visit(&mut sim, &rules, id, frame);
    }
    let before = sim.anim(id).unwrap().clone();
    let retained = before
        .flaming_guy
        .as_ref()
        .expect("runner owns retained state");
    assert!(retained.destination.is_some() && retained.fall.falling());
    let encoded = serde_json::to_vec(&before).unwrap();
    let mut restored: AnimObject = serde_json::from_slice(&encoded).unwrap();
    // Normal load reconstructs this skipped cache from the LogicVector.
    restored.set_in_logic_vector_for_test(before.in_logic_vector);
    assert_eq!(restored.flaming_guy, before.flaming_guy);
    assert_eq!(anim_hash(&restored), anim_hash(&before));
    for mutation in 0..4 {
        let mut changed = before.clone();
        let state = changed.flaming_guy.as_mut().unwrap();
        match mutation {
            0 => state.destination.as_mut().unwrap().x += 1,
            1 => state.retries += 1,
            2 => state.expired = !state.expired,
            _ => state.fall.set_rate_for_test(state.fall.rate() - 1),
        }
        *sim.substrate.anims.get_mut(id).unwrap() = before.clone();
        let original = sim.state_hash();
        *sim.substrate.anims.get_mut(id).unwrap() = changed;
        assert_ne!(
            sim.state_hash(),
            original,
            "runner retained field {mutation}"
        );
    }
    *sim.substrate.anims.get_mut(id).unwrap() = restored.clone();
    assert_eq!(sim.anim(id).unwrap().flaming_guy, before.flaming_guy);
    let (mut continued, continued_id) = scene(&rules, &json!({"z": 500}));
    assert_eq!(continued_id, id);
    *continued.substrate.anims.get_mut(id).unwrap() = restored;
    continued.scenario_rng = sim.scenario_rng.clone();
    // Continue the serialized object through production AI on the same flat
    // input scene. This is Rust serde continuation, not native SaveGame parity.
    for frame in 5..300 {
        visit(&mut sim, &rules, id, frame);
        visit(&mut continued, &rules, id, frame);
        assert_eq!(
            state(&sim, id),
            state(&continued, id),
            "continued frame {frame}"
        );
        assert_eq!(
            sim.scenario_rng.logical_state(),
            continued.scenario_rng.logical_state()
        );
        if sim.substrate.pending_delete.contains_object(&id) {
            break;
        }
    }
    assert!(sim.substrate.pending_delete.contains_object(&id));
}

#[test]
fn stock_oil_derrick_death_weapon_produces_a_running_flameguy_in_master_frames() {
    use crate::sim::command::{Command, CommandEnvelope};
    use crate::sim::world::TickLane;

    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let mut rules = retail.rules;
    let (_, assets) = crate::rules::retail_ini_fixture::retail_assets().unwrap();
    let roots = crate::rules::effect_asset_catalog::anim_class_roots(&rules);
    rules.bind_anim_class_assets(&roots, &assets, "TEM", "TEMPERATE");
    rules
        .bind_scheduler_anim_assets(
            &rules
                .general
                .damage_fire_types
                .iter()
                .map(|anim| anim.name.clone())
                .collect::<Vec<_>>(),
            &assets,
            "TEM",
            "TEMPERATE",
        )
        .expect("physical damage-fire assets for the ordinary blast victims");
    rules.bind_effect_assets(&assets, "TEM", "TEMPERATE");
    assert_eq!(
        rules.object("CAOILD").unwrap().death_weapon.as_deref(),
        Some("OilExplosion")
    );
    assert_eq!(
        rules.weapon("OilExplosion").unwrap().warhead.as_deref(),
        Some("OilExplosionWH")
    );
    assert_eq!(rules.warhead("OilExplosionWH").unwrap().inf_death, 4);
    assert_eq!(rules.general.infantry_death_anim(4), Some("FLAMEGUY"));
    let runner = rules.art().anim_runtime_config("FLAMEGUY").unwrap();
    assert!(runner.is_flaming_guy && runner.running_frames > 0);

    let mut sim = crate::sim::arena_fixture::rival_arena_with_size(&rules, 2466, 64);
    // This encounter uses two human-controlled opponents.
    let opponent = sim.interner.intern("Russians");
    sim.houses
        .get_mut(&opponent)
        .unwrap()
        .set_is_human_for_test(true);
    sim.houses
        .get_mut(&opponent)
        .unwrap()
        .set_player_control_for_test(true);
    sim.fog = crate::sim::vision::FogState::new(64, 64);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.resolve_rule_animation_lists(&rules);
    let planter = sim
        .spawn_object("TANY", "Americans", 30, 32, 0, &rules)
        .unwrap();
    let oil = sim
        .spawn_object("CAOILD", "Russians", 32, 32, 0, &rules)
        .unwrap();
    let infantry = sim
        .spawn_object("E1", "Americans", 31, 32, 0, &rules)
        .unwrap();
    for (house, x, y) in [("Americans", 4, 4), ("Russians", 58, 58)] {
        sim.spawn_object("TRUCKA", house, x, y, 0, &rules).unwrap();
    }
    let owner = sim.substrate.entities.get(planter).unwrap().owner();
    sim.session.set_current_house_for_test(Some(owner));
    sim.fog.mark_visible_for_owner(owner, 32, 32);
    sim.queue_command(CommandEnvelope::new(
        owner,
        1,
        Command::Attack {
            attacker_id: planter,
            target_id: oil,
        },
    ));
    let mut first_runner = None;
    let mut moved = false;
    for _ in 0..2400 {
        let commands = sim.take_due_commands();
        let output = sim
            .advance_app_frame(
                &commands,
                &rules,
                None,
                33,
                TickLane::Ordinary,
                FrameEffects::default(),
            )
            .expect("ordinary oil combat frame");
        assert!(output.tick.frame_committed);
        if first_runner.is_none() {
            first_runner = sim.anims().find_map(|(id, anim)| {
                (sim.interner.resolve(anim.type_id()) == "FLAMEGUY")
                    .then_some((*id, anim.world_coord))
            });
        }
        if let Some((id, origin)) = first_runner {
            if let Some(anim) = sim.anim(id) {
                moved |= anim.world_coord != origin;
            } else if moved {
                break;
            }
        }
    }
    let actor_state = |id| {
        sim.substrate.entities.get(id).map(|actor| {
            (
                actor.health.current(),
                actor.lifecycle.object_alive(),
                format!("{:?}", actor.attack_target),
            )
        })
    };
    let (id, _) = first_runner.unwrap_or_else(|| {
        panic!(
            "stock OilExplosion must create FLAMEGUY: planter {:?}, oil {:?}, infantry {:?}",
            actor_state(planter),
            actor_state(oil),
            actor_state(infantry),
        )
    });
    assert!(
        sim.substrate
            .entities
            .get(oil)
            .is_none_or(|e| !e.lifecycle.object_alive())
    );
    assert!(
        sim.substrate
            .entities
            .get(infantry)
            .is_none_or(|e| !e.lifecycle.object_alive())
    );
    assert!(
        moved,
        "the ordinary LogicVector must move its retained FLAMEGUY"
    );
    assert!(
        sim.anim(id).is_none(),
        "the runner must naturally expire and drain"
    );
}
