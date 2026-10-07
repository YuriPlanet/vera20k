//! Joined original47DD70 -> primary AnimAI423AC0 -> Bounce/landing comparisons.
//! Only the primary Anim is visited, matching the native scheduler boundary.
//! Children construct synchronously but their later AI and audio are not covered.

use super::*;
use crate::map::resolved_terrain::{ResolvedTerrainGrid, test_flat_cell};
use crate::sim::bounce::BounceState;

fn flight_rules(producer: &Value, inputs: &Value) -> Option<RuleSet> {
    let mut rules = retail_rules(producer)?;
    for row in inputs["rows"].as_array().unwrap() {
        let name = row["name"].as_str().unwrap();
        if let Some(frames) = row["raw_shp_frame_count"].as_i64() {
            rules.bind_anim_frame_count_for_test(name, frames as i32);
        }
        if row["missing_runtime_config"] == true {
            continue; // D is supplied by its original constructor in the oracle.
        }
        let config = rules.art().anim_runtime_config(name).unwrap();
        assert_eq!(
            serde_json::json!(config.raw_shp_frame_count),
            row["raw_shp_frame_count"],
            "{name}"
        );
        assert_eq!(
            serde_json::json!(config.trailer_anim),
            row["trailer_anim"],
            "{name}"
        );
        assert_eq!(
            serde_json::json!(config.expire_anim),
            row["expire_anim"],
            "{name}"
        );
        assert_eq!(
            i64::from(config.trailer_seperation),
            row["trailer_seperation"].as_i64().unwrap(),
            "{name}"
        );
    }
    assert_eq!(serde_json::json!(rules.general.wake.name), inputs["wake"]);
    assert_eq!(
        serde_json::json!(rules.combat_damage.splash_list),
        inputs["splash_list"]
    );
    assert_eq!(
        i64::from(rules.bridge_rules.strength),
        inputs["bridge_strength"].as_i64().unwrap()
    );
    assert_eq!(
        rules.warhead("HE").unwrap().wall,
        inputs["he_wall"].as_bool().unwrap()
    );
    Some(rules)
}

// Independent original readers: tools/rules_oracle/bridge_landing_inputs.
// This compares actual production RuleSet values; VERA's exports did not seed
// the native readers. Every HE/Super entry and sampled damage result must match.
fn assert_native_landing_inputs(rules: &RuleSet) {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/rules_oracle/bridge_landing_inputs.json",
    ))
    .unwrap();
    let retail = corpus["layers"].as_array().unwrap().last().unwrap();
    let globals = &retail["rules"];
    assert_eq!(
        serde_json::json!(rules.general.tree_strength),
        globals["tree_strength"]
    );
    assert_eq!(serde_json::json!(rules.general.wake.name), globals["wake"]);
    assert_eq!(
        serde_json::json!(rules.combat_damage.splash_list),
        globals["splash_list"]
    );
    assert_eq!(
        serde_json::json!(rules.combat_damage.max_damage),
        globals["max_damage"]
    );
    assert_eq!(
        serde_json::json!(rules.bridge_rules.strength),
        globals["bridge_strength"]
    );
    assert_eq!(
        serde_json::json!(rules.bridge_warheads.c4_name),
        globals["c4_warhead"]
    );
    for (value, key) in [
        (rules.general.condition_red, "condition_red_bits"),
        (rules.general.condition_yellow, "condition_yellow_bits"),
    ] {
        assert_eq!(format!("{:016x}", value.to_bits()), globals[key]);
    }
    for name in ["HE", "Super"] {
        let actual = rules.warhead(name).unwrap();
        let expected = &retail["warheads"][name];
        assert_eq!(serde_json::json!(actual.wall), expected["wall"], "{name}");
        assert_eq!(serde_json::json!(actual.wood), expected["wood"], "{name}");
        for (value, key) in [
            (actual.cell_spread_f64, "cell_spread_bits"),
            (actual.percent_at_max_f64, "percent_at_max_bits"),
        ] {
            let native =
                f32::from_bits(u32::from_str_radix(expected[key].as_str().unwrap(), 16).unwrap());
            assert_eq!(value.to_bits(), f64::from(native).to_bits(), "{name}/{key}");
        }
        for (index, value) in actual.verses_f64.iter().enumerate() {
            let native =
                u64::from_str_radix(expected["verses_bits"][index].as_str().unwrap(), 16).unwrap();
            assert_eq!(value.to_bits(), native, "{name}/armor{index}");
        }
    }
    for name in ["TREE01", "TIBTRE01"] {
        let actual = rules.terrain_object_type_case_insensitive(name).unwrap();
        let expected = &retail["terrain_types"][name];
        assert_eq!(
            serde_json::json!(actual.strength),
            expected["strength"],
            "{name}"
        );
        assert_eq!(actual.armor, "wood", "native Armor6: {name}");
        assert_eq!(expected["armor"], 6);
        assert_eq!(
            serde_json::json!(actual.immune),
            expected["immune"],
            "{name}"
        );
        assert_eq!(
            serde_json::json!(actual.spawns_tiberium),
            expected["spawns_tiberium"],
            "{name}"
        );
        assert_eq!(
            serde_json::json!(actual.temperate_occupation_bits),
            expected["temperate_occupation_bits"],
            "{name}"
        );
        assert_eq!(
            serde_json::json!(actual.snow_occupation_bits),
            expected["snow_occupation_bits"],
            "{name}"
        );
        let foundation = corpus["terrain_art"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["name"] == name)
            .unwrap();
        assert_eq!(
            serde_json::json!(actual.foundation),
            foundation["foundation_name"]
        );
        assert_eq!(foundation["occupy_offsets"][0], serde_json::json!([0, 0]));
        assert_eq!(
            foundation["occupy_offsets"][1],
            serde_json::json!([32767, 32767])
        );
    }
    let he = rules.warhead("HE").unwrap();
    for row in corpus["damage_sensitivity"]["rows"].as_array().unwrap() {
        let armor = row["armor"].as_u64().unwrap() as u8;
        let actual = crate::sim::combat::damage::kernel::apply_warhead_damage(
            row["damage"].as_i64().unwrap() as i32,
            he.cell_spread_f64,
            he.percent_at_max_f64,
            &he.verses_f64,
            crate::sim::combat::damage::ArmorClass(armor),
            row["distance"].as_i64().unwrap() as i32,
            false,
            rules.combat_damage.max_damage,
        );
        let expected = row["native"].as_i64().unwrap() as i32;
        assert_eq!(actual, expected, "native HE damage: {row}");
    }
}

#[test]
fn bridge_landing_constructor_and_absent_keys_use_native_defaults() {
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::GeneralRules;
    use crate::rules::terrain_object_type::TerrainObjectType;

    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/rules_oracle/bridge_landing_inputs.json",
    ))
    .unwrap();
    let native = &corpus["constructor"]["rules"];
    let expected_strength = native["tree_strength"].as_i64().unwrap() as i32;
    let expected_red =
        u64::from_str_radix(native["condition_red_bits"].as_str().unwrap(), 16).unwrap();
    let defaults = GeneralRules::default();
    assert_eq!(defaults.tree_strength, expected_strength);
    assert_eq!(defaults.condition_red.to_bits(), expected_red);
    for additional in [
        "",
        "[General]\nFixtureOnly=1\n[AudioVisual]\nFixtureOnly=1\n",
    ] {
        let ini = IniFile::from_str(&format!(
            "[TerrainTypes]\n0=TREE01\n[TREE01]\nFixtureOnly=1\n{additional}"
        ));
        let rules = RuleSet::from_ini(&ini).unwrap();
        assert_eq!(rules.general.tree_strength, expected_strength);
        assert_eq!(rules.general.condition_red.to_bits(), expected_red);
        assert_eq!(
            rules
                .terrain_object_type_case_insensitive("TREE01")
                .unwrap()
                .strength,
            expected_strength
        );
        assert_eq!(
            TerrainObjectType::from_ini_section("TREE01", ini.section("TREE01").unwrap()).strength,
            expected_strength
        );
    }
}

#[test]
fn retail_bridge_landing_inputs_match_original_readers() {
    let Some(rules) = retail_rules(&native()) else {
        return;
    };
    assert_native_landing_inputs(&rules);
}

#[test]
#[ignore = "requires the configured retail install and stock Hills.mmx"]
fn retail_hills_layered_bridge_landing_inputs_match_original_readers() {
    use crate::assets::asset_manager::{AssetManager, MediaArchiveMode};
    use crate::rules::ini_parser::IniFile;
    use crate::rules::process_owner::NativeRulesProcessOwner;

    let retail = std::env::var("RA2_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            crate::util::config::GameConfig::load()
                .unwrap()
                .paths
                .ra2_dir
        });
    let assets = AssetManager::new(&retail, MediaArchiveMode::STOCK_DIGITAL).unwrap();
    let read = |name: &str| IniFile::from_bytes(&assets.get(name).unwrap()).unwrap();
    let root = read("RULESMD.INI");
    let art = read("ARTMD.INI");
    let mode = read("MPBattleMD.ini");
    assert!(
        assets.get("LANGRULE.INI").is_none(),
        "native selected installation boundary"
    );
    let map = crate::map::map_file::load_from_path(&retail.join("Hills.mmx")).unwrap();
    let audio_definitions = crate::rules::audio_sources::AudioDefinitions::select(&assets);
    let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
        root,
        None,
        art,
        std::sync::Arc::clone(audio_definitions.sounds()),
    )
    .unwrap();
    let (mut rules, _, art, _) = owner
        .load_noncampaign_scenario(Some(&mode), &map.ini)
        .unwrap()
        .into_parts();
    let registry = crate::rules::art_data::ArtRegistry::from_ini(&art);
    rules.install_art_data(registry);
    assert_native_landing_inputs(&rules);
}

fn flight_fixture(rules: &RuleSet, input: &Value) -> (Simulation, BTreeSet<(u16, u16)>) {
    let (mut sim, source) = fixture(rules, input);
    let shape = input["terrain"].as_str().unwrap();
    let bridge = matches!(shape, "bridge" | "waterbridge");
    let water = matches!(shape, "water" | "waterbridge");
    let cells = (0..30_u16)
        .flat_map(|y| {
            (0..32_u16).map(move |x| {
                let mut cell = test_flat_cell(x, y);
                let distance = (i32::from(x) - 12).abs().max((i32::from(y) - 9).abs());
                cell.level = match shape {
                    "ground0" => 0,
                    "mesa" if distance > 1 => 0,
                    "pit" if distance <= 2 => 0,
                    "cliff" if distance != 0 => 12,
                    _ => 4,
                };
                cell.final_tile_index = if bridge { 1020 } else { -1 };
                cell.bridge_facts.raw_flags = if bridge { 0x100 } else { 0 };
                cell.land_type = if water { 2 } else { 0 };
                cell.yr_cell_land_type = cell.land_type;
                cell.is_water = water;
                cell
            })
        })
        .collect();
    let mut terrain = ResolvedTerrainGrid::from_cells(32, 30, cells);
    terrain.set_dummy_cell_level(0);
    terrain.test_set_high_bridge_set_starts(Some(1000), Some(2000));
    terrain.test_set_high_bridge_rim_tiles(
        crate::map::bridge_rim_tiles::HighBridgeRimTiles::from_ini(
            1000,
            b"[General]\nBridgeMiddle1=20\nBridgeMiddle2=40\n",
        ),
    );
    if bridge {
        // Native supplies a concrete anchor and false driver returns. Represent
        // that boundary with real no-transition state255, not a Rust callback
        // hook. Tile1020/base1000/middle20 still admits the original A block.
        let anchor = terrain.native_cell_identity((0, 0));
        terrain.cell_mut(0, 0).unwrap().bridge_facts.overlay_id = Some(0x18);
        terrain.write_native_cell_state(anchor, 255);
        for cell in &mut terrain.cells {
            cell.bridge_facts.native_anchor = Some(anchor);
        }
    }
    sim.bridge_state = Some(
        crate::sim::bridge_state::BridgeRuntimeState::from_resolved_terrain(
            &terrain,
            true,
            rules.bridge_rules.strength,
        ),
    );
    sim.install_resolved_terrain_for_new_map(terrain);
    sim.session.binary_frame = 1000;
    (sim, source)
}

fn compare_body(body: &BounceState, native: &str, terminal: bool, context: &str) {
    let bytes = (0..native.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&native[index..index + 2], 16).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(bytes.len(), 0x50);
    let dword = |offset| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
    let qword = |offset| u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap());
    assert_eq!(body.elasticity.bits(), qword(0), "{context}: elasticity");
    assert_eq!(body.gravity.bits(), qword(8), "{context}: gravity");
    assert_eq!(
        body.angular_velocity_magnitude.bits(),
        qword(16),
        "{context}: clamp"
    );
    for axis in 0..3 {
        assert_eq!(
            body.position[axis].bits(),
            dword(24 + axis * 4),
            "{context}: position{axis}"
        );
        let actual = body.velocity[axis].bits();
        let expected = dword(36 + axis * 4);
        if actual != expected && actual & 0x7fff_ffff == 0 && expected & 0x7fff_ffff == 0 {
            // Existing flat reflection/cliff-zero shortcuts can retain a
            // different zero sign than native matrix arithmetic. Bound this
            // exception to the terminal e=0 visit: integer stop magnitude,
            // landing coordinates and destruction consume no velocity sign,
            // and this Anim receives no subsequent physics visit. No tolerance
            // applies to nonzero velocity, position, timers or Scenario state.
            assert!(terminal, "{context}: airborne signed-zero difference");
            assert_eq!(body.elasticity.bits() & 0x7fff_ffff_ffff_ffff, 0);
        } else {
            assert_eq!(actual, expected, "{context}: velocity{axis}");
        }
    }
    // SHP DBRIS has zero spin and its draw path does not consume quaternions.
    // BounceState does not represent native quaternion bytes; no comparison of
    // that unrepresented display-only state is claimed here.
    assert_eq!(
        body.spin_angle.bits(),
        0,
        "{context}: zero-spin fixture boundary"
    );
}

#[test]
fn native_bridge_producer_primary_flight_landing_and_rng_continuation() {
    let rows: Vec<Value> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/bridge_debris_flight.json",
    ))
    .unwrap();
    let inputs: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/bridge_debris_flight.inputs.json",
    ))
    .unwrap();
    let Some(rules) = flight_rules(&native(), &inputs) else {
        return;
    };
    assert_eq!(rows.len(), 32);
    for row in rows {
        let input = &row["input"];
        let (mut sim, source) = flight_fixture(&rules, input);
        assert_eq!(
            sim.scenario_rng.native_state_hex(),
            row["producer_rng_before"].as_str().unwrap()
        );
        spawn_bridge_debris(&mut sim, &rules, &source);
        assert_eq!(
            sim.scenario_rng.native_state_hex(),
            row["producer_rng_after"].as_str().unwrap(),
            "{input}"
        );
        let id = sim
            .anims()
            .find_map(|(&id, anim)| {
                (sim.interner.resolve(anim.type_id) == row["type"].as_str().unwrap()).then_some(id)
            })
            .expect("native-selected metallic type constructs");
        let birth = sim.anim(id).unwrap();
        assert_eq!(
            serde_json::json!([
                birth.world_coord.x,
                birth.world_coord.y,
                birth.world_coord.z
            ]),
            row["birth_state"]["location"],
            "{input}: birth location"
        );
        if let Some(body) = &birth.bounce {
            compare_body(
                body,
                row["birth_state"]["bounce_hex"].as_str().unwrap(),
                false,
                "birth",
            );
        }
        let mut known = sim.anims().map(|(&id, _)| id).collect::<BTreeSet<_>>();
        let mut children = Vec::new();
        for tick in row["ticks"].as_array().unwrap() {
            let number = tick["tick"].as_u64().unwrap();
            let context = format!("{input}, tick {number}");
            sim.session.binary_frame = 1000 + number as u32;
            assert!(
                !sim.visit_anim(id, &rules, None),
                "false bridge-driver fixture: {context}"
            );
            let anim = sim
                .anim(id)
                .expect("deferred delete retains storage before drain");
            assert_eq!(
                serde_json::json!([anim.world_coord.x, anim.world_coord.y, anim.world_coord.z]),
                tick["location"],
                "{context}"
            );
            assert_eq!(
                anim.runtime.inactive,
                tick["inactive"] != 0,
                "{context}: native19B"
            );
            assert_eq!(
                !sim.substrate.pending_delete.contains(&id),
                tick["alive"] == 1,
                "{context}: retained logical Alive"
            );
            assert_eq!(
                i64::from(anim.runtime.current_frame),
                tick["current_frame"].as_i64().unwrap(),
                "{context}: frame"
            );
            assert_eq!(
                i64::from(anim.runtime.delay_remaining),
                tick["delay_remaining"].as_i64().unwrap(),
                "{context}: delay"
            );
            assert_eq!(
                i64::from(anim.runtime.loop_remaining),
                tick["loop_remaining"].as_i64().unwrap(),
                "{context}: loop"
            );
            assert_eq!(
                anim.runtime.first_ai_guard,
                tick["first_ai_guard"] == 1,
                "{context}: first guard"
            );
            assert_eq!(
                i64::from(anim.runtime.frame_step),
                tick["frame_step"].as_i64().unwrap(),
                "{context}: frame step"
            );
            assert_eq!(
                i64::from(anim.runtime.frame_timer.start_frame()),
                tick["timer_start"].as_i64().unwrap(),
                "{context}: timer start"
            );
            assert_eq!(
                i64::from(anim.runtime.frame_timer.duration()),
                tick["timer_duration"].as_i64().unwrap(),
                "{context}: timer duration"
            );
            assert_eq!(
                i64::from(anim.runtime.rate_reload),
                tick["rate_reload"].as_i64().unwrap(),
                "{context}: rate reload"
            );
            if let Some(body) = &anim.bounce {
                compare_body(
                    body,
                    tick["body"].as_str().unwrap(),
                    tick["alive"] == 0,
                    &context,
                );
            } else {
                assert_eq!(row["type"], "D", "only the unread type has no Bounce body");
            }
            // Full Scenario state, not just the cursor, bounds every primary
            // visit including trailer/landing constructor and bridge draws.
            assert_eq!(
                sim.scenario_rng.native_state_hex(),
                tick["rng_state"].as_str().unwrap(),
                "{context}: Scenario"
            );
            for (&child_id, child) in sim.anims() {
                if known.insert(child_id) {
                    children.push(serde_json::json!({
                        "tick": number,
                        "type": sim.interner.resolve(child.type_id),
                        "coord": [child.world_coord.x, child.world_coord.y, child.world_coord.z],
                        "delay": child.runtime.delay_remaining,
                        "flags": child.draw_flags,
                    }));
                }
            }
        }
        let native_children = row["flight_events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| event["call"] == "anim_ctor")
            .map(|event| {
                serde_json::json!({
                    "tick": event["tick"], "type": event["type"], "coord": event["coord"],
                    "delay": event["delay"], "flags": event["flags"],
                })
            })
            .collect::<Vec<_>>();
        assert_eq!(
            children, native_children,
            "{input}: trailer/landing constructor order"
        );
        let anim = sim.anim(id).unwrap();
        assert!(sim.substrate.pending_delete.contains(&id), "{input}");
        assert!(!anim.in_logic_vector, "{input}");
        assert!(
            sim.display_layers().layer_of(id).is_none(),
            "{input}: display unregistered"
        );
        assert_eq!(row["limbo"], 1);
        assert_eq!(row["in_logic"], 0);
        assert!(sim.substrate.pending_delete.contains(&id), "{input}");
        // Native may queue the pointer twice (Stopped before/after landing).
        // Rust queues once. This assertion bounds logical destruction; it does
        // not assert equal observer calls or pending-list multiplicity.
        assert_eq!(
            sim.scenario_rng.native_state_hex(),
            row["rng_after"].as_str().unwrap(),
            "{input}"
        );
        let next = (0..4)
            .map(|_| sim.scenario_rng.next_u32())
            .collect::<Vec<_>>();
        assert_eq!(
            serde_json::json!(next),
            row["next_rng"],
            "{input}: continuation"
        );
        assert_eq!(row["drain"]["queue_after"], 0);
        assert_eq!(
            row["drain"]["scalar_deletions"].as_array().unwrap().len(),
            1
        );
        sim.clear_lifecycle_test_events_for_test();
        sim.process_pending_delete();
        assert!(sim.anim(id).is_none(), "{input}: physical removal");
        assert!(sim.substrate.pending_delete.is_empty(), "{input}: drain");
        assert_eq!(
            sim.lifecycle_test_events_for_test()
                .iter()
                .filter(|event| matches!(
                    event,
                    crate::sim::world::LifecycleTestEvent::FinalizedCommon { stable_id }
                        if *stable_id == id
                ))
                .count(),
            1,
            "{input}: one physical finalization"
        );
    }
}
