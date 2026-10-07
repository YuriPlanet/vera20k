//! Original47DD70 producer + native AnimClass constructor comparisons.
//! Retail lists, ART and physical SHP frame counts are explicit corpus inputs;
//! these tests do not treat the earlier supplied20-pool fixture as retail.

use super::*;
use crate::rules::{art_data::ArtRegistry, retail_ini_fixture::retail_ini};
use serde_json::Value;

fn native() -> Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/bridge_debris_producer.json",
    ))
    .unwrap()
}

fn retail_rules(corpus: &Value) -> Option<RuleSet> {
    let ini = retail_ini("rulesmd.ini")?;
    let art = retail_ini("artmd.ini")?;
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
    rules.install_art_data(ArtRegistry::from_ini(&art));
    assert_eq!(
        serde_json::json!(rules.general.metallic_debris),
        corpus["metallic_pool"]
    );
    assert_eq!(
        serde_json::json!(rules.bridge_rules.explosions),
        corpus["explosion_pool"]
    );
    // Actual asset counts, not invented animation lengths. The source fixture
    // records the production loader/AssetManager export used by native runs.
    for input in corpus["retail_anim_types"].as_array().unwrap() {
        let name = input["name"].as_str().unwrap();
        if let Some(frames) = input["frames"].as_i64().filter(|frames| *frames > 0) {
            rules.bind_anim_frame_count_for_test(name, frames as i32);
        }
        let config = rules.art().anim_runtime_config(name).unwrap();
        assert_eq!(
            serde_json::json!({
                "bouncer": config.bouncer,
                "rate": config.rate_logic_frames,
                "random_rate": config.random_rate_logic_frames.unwrap_or((0, 0)),
                "loop_count": config.loop_count,
                "end": config.end,
                "loop_end": config.loop_end,
                "normalized": config.normalized,
            }),
            serde_json::json!({
                "bouncer": input["bouncer"],
                "rate": input["rate"],
                "random_rate": input["random_rate_stored"],
                "loop_count": input["loop_count"],
                "end": input["end"],
                "loop_end": input["loop_end"],
                "normalized": input["normalized"],
            }),
            "{name}"
        );
        for (actual, key) in [
            (config.elasticity.bits(), "elasticity"),
            (config.max_xy_vel.bits(), "max_xy"),
            (config.min_z_vel.bits(), "min_z"),
        ] {
            assert_eq!(
                actual,
                input[key].as_f64().unwrap().to_bits(),
                "{name}/{key}"
            );
        }
        // The missing D section remains the original AnimType constructor.
        // Every real ART section must agree with the exported producer inputs.
        let source = &input["source"];
        if source["missing_runtime_config"] != true {
            assert_eq!(
                config.art_body_read,
                source["art_body_read"].as_bool().unwrap()
            );
            assert_eq!(
                config.damage.bits(),
                source["damage_f64_bits"].as_u64().unwrap()
            );
            assert_eq!(
                i64::from(config.damage_radius),
                source["damage_radius"].as_i64().unwrap()
            );
            assert_eq!(serde_json::json!(config.warhead), source["warhead"]);
            assert_eq!(serde_json::json!(config.expire_anim), source["expire_anim"]);
            assert_eq!(
                serde_json::json!(config.trailer_anim),
                source["trailer_anim"]
            );
            assert_eq!(
                i64::from(config.trailer_seperation),
                source["trailer_seperation"].as_i64().unwrap()
            );
        } else {
            assert!(!config.art_body_read, "{name}");
            assert_eq!(config.raw_shp_frame_count, None, "{name}");
        }
    }
    Some(rules)
}

#[test]
fn retail_bridge_anim_inputs_match_original_full_art_reader() {
    let Some(ini) = retail_ini("rulesmd.ini") else {
        return;
    };
    let Some(art) = retail_ini("artmd.ini") else {
        return;
    };
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
    rules.install_art_data(ArtRegistry::from_ini(&art));
    // Original427530 -> full427D00/5F92E0 -> image427B50, supplied only raw
    // physical ART strings and complete retail SHP bytes at archive I/O.
    // This independent corpus establishes the inputs used by the producer and
    // flight oracles; no VERA scalar value initialized its native type fields.
    let native: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/rules_oracle/bridge_anim_inputs.json",
    ))
    .unwrap();
    let rows = native["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 24);
    for row in rows {
        let name = row["name"].as_str().unwrap();
        let frames = row["raw_shp_frame_count"].as_i64().unwrap();
        if frames > 0 {
            // Native original header reads independently establish this asset
            // boundary; the production release load validates actual binding.
            rules.bind_anim_frame_count_for_test(name, frames as i32);
        }
        let config = rules.art().anim_runtime_config(name).unwrap();
        let actual = serde_json::json!({
            "art_body_read": config.art_body_read,
            "image": rules.art().resolve_anim_image_id(name),
            "start": config.start,
            "loop_start": config.loop_start,
            "loop_end": config.loop_end,
            "end": config.end,
            "loop_count": config.loop_count,
            "rate": config.rate_logic_frames,
            "random_rate": config.random_rate_logic_frames.unwrap_or((0, 0)),
            "raw_shp_frame_count": config.raw_shp_frame_count.unwrap_or(0),
            "damage_f64_bits": config.damage.bits(),
            "elasticity_f64_bits": config.elasticity.bits(),
            "min_z_vel_f64_bits": config.min_z_vel.bits(),
            "max_xy_vel_f64_bits": config.max_xy_vel.bits(),
            "damage_radius": config.damage_radius,
            "trailer_seperation": config.trailer_seperation,
            "bouncer": config.bouncer,
            "normalized": config.normalized,
            "scorch": config.scorch,
            "crater": config.crater,
            "shadow": config.shadow,
            "expire_anim": config.expire_anim,
            "bounce_anim": config.bounce_anim,
            "trailer_anim": config.trailer_anim,
            "warhead": config.warhead,
        });
        for (field, value) in actual.as_object().unwrap() {
            assert_eq!(value, &row[field], "{name}/{field}");
        }
        if !config.art_body_read {
            assert_eq!(name, "D");
            assert_eq!(config.raw_shp_frame_count, None);
        }
    }
}

fn fixture(rules: &RuleSet, input: &Value) -> (Simulation, BTreeSet<(u16, u16)>) {
    let mut sim = Simulation::with_seed(input["seed"].as_u64().unwrap());
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    sim.metallic_debris = rules
        .general
        .metallic_debris
        .iter()
        .map(|name| sim.interner.intern(name))
        .collect();
    sim.bridge_explosions = rules
        .bridge_rules
        .explosions
        .iter()
        .map(|name| sim.interner.intern(name))
        .collect();
    let level = input["level"].as_i64().unwrap() as i8;
    let mut terrain = crate::map::resolved_terrain::test_flat_ground_grid(32);
    for cell in &mut terrain.cells {
        cell.level = level as u8;
        // The structural flag is already gone when collapse calls47DD70.
        // A stale derived deck value must not decide the producer's Z.
        cell.bridge_facts.raw_flags = 0;
        cell.bridge_deck_level = 99;
    }
    terrain.set_dummy_cell_level(level);
    sim.install_resolved_terrain_for_new_map(terrain);
    let x = input["cell"][0].as_i64().unwrap() as i16 as u16;
    let y = input["cell"][1].as_i64().unwrap() as i16 as u16;
    (sim, BTreeSet::from([(x, y)]))
}

#[test]
fn retail_bridge_debris_matches_original_producer_and_constructor_rng() {
    let corpus = native();
    let Some(rules) = retail_rules(&corpus) else {
        return;
    };
    for row in corpus["rows"].as_array().unwrap() {
        let input = &row["input"];
        // Native allocator failures and map-editor mode are characterization
        // rows, outside the running Rust simulation's successful allocation
        // domain. Ground receiver ordering has its own production comparisons.
        if input.get("fail_allocation").is_some()
            || input["map_editor"].as_i64().is_some_and(|value| value != 0)
            || input["synthetic_ground_objects"]
                .as_i64()
                .is_some_and(|value| value != 0)
        {
            continue;
        }
        let (mut sim, cells) = fixture(&rules, input);
        if input["explosion_count"]
            .as_i64()
            .is_some_and(|value| value <= 0)
        {
            sim.bridge_explosions.clear();
        }
        let main_before = sim.main_rng.logical_state();
        let mapgen_before = sim.mapgen_rng.logical_state();
        assert_eq!(
            sim.scenario_rng.native_state_hex(),
            row["rng_before"].as_str().unwrap()
        );
        spawn_bridge_debris(&mut sim, &rules, &cells);
        assert_eq!(
            sim.scenario_rng.native_state_hex(),
            row["rng_after"].as_str().unwrap(),
            "{input}"
        );
        assert_eq!(sim.main_rng.logical_state(), main_before, "{input}");
        assert_eq!(sim.mapgen_rng.logical_state(), mapgen_before, "{input}");
        let actual: Vec<_> = sim.substrate.anims.iter().map(|(_, anim)| anim).collect();
        let expected = row["anims"].as_array().unwrap();
        assert_eq!(actual.len(), expected.len(), "{input}");
        let constructors: Vec<_> = row["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| event["call"] == "anim_ctor")
            .collect();
        for ((anim, expected), constructor) in actual.iter().zip(expected).zip(constructors) {
            assert_eq!(
                sim.interner.resolve(anim.type_id),
                expected["type"].as_str().unwrap(),
                "{input}"
            );
            assert_eq!(
                serde_json::json!([anim.world_coord.x, anim.world_coord.y, anim.world_coord.z]),
                expected["location"],
                "{input}"
            );
            assert_eq!(
                u64::from(anim.runtime.delay_remaining),
                constructor["delay"].as_u64().unwrap(),
                "{input}"
            );
            assert_eq!(
                u64::from(anim.draw_flags),
                constructor["flags"].as_u64().unwrap(),
                "{input}"
            );
            assert_eq!(
                anim.bounce.is_some(),
                expected["is_bouncing"] == 1,
                "{input}"
            );
            if let Some(body) = anim.bounce {
                for (actual, key) in [
                    (body.position, "position_bits"),
                    (body.velocity, "velocity_bits"),
                ] {
                    assert_eq!(
                        serde_json::json!(actual.map(|value| value.bits())),
                        expected["bounce"][key],
                        "{input}/{key}"
                    );
                }
                assert_eq!(
                    body.elasticity.bits(),
                    expected["bounce"]["elasticity_bits"].as_u64().unwrap(),
                    "{input}"
                );
                assert_eq!(
                    body.gravity.bits(),
                    expected["bounce"]["gravity_bits"].as_u64().unwrap(),
                    "{input}"
                );
            }
        }
    }
}

#[path = "bridge_debris_flight_tests.rs"]
mod flight_tests;
