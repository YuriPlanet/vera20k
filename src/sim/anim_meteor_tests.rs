//! Original Execute58, numeric meteor flight, and AnimAI ore controls.
use super::*;
use crate::map::resolved_terrain::{test_flat_cell, test_grid};
use crate::map::waypoints::Waypoint;
use crate::rules::art_data::ArtRegistry;
use crate::rules::ini_parser::IniFile;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::sim::rng::SimRng;
use crate::util::sha256::sha256_hex;
use serde_json::Value;
use std::collections::HashMap;

pub(in crate::sim) fn formed_rules(extra: &str) -> RuleSet {
    let ini = IniFile::from_str(
        "[Animations]\n0=METSMALL\n1=METLARGE\n2=CHILD\n3=EXPLODE\n4=WAKE\n5=SPLASH1\n6=SPLASH2\n[General]\nWake=WAKE\n[CombatDamage]\nSplashList=SPLASH1,SPLASH2\n[Warheads]\n0=HE\n[HE]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\nCellSpread=0\n[Clear]\nBuildable=yes\n",
    );
    let mut art = IniFile::from_str(
        "[METSMALL]\nIsMeteor=yes\nMinZVel=-50\nMaxXYVel=100\nElasticity=0\n[METLARGE]\nIsMeteor=yes\nMinZVel=-50\nMaxXYVel=100\nElasticity=0\n[CHILD]\nStart=0\n[EXPLODE]\nStart=0\n[WAKE]\nStart=0\n[SPLASH1]\nStart=0\n[SPLASH2]\nStart=0\n",
    );
    art.merge(&IniFile::from_str(extra));
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
    let mut registry = ArtRegistry::from_ini(&art);
    for name in [
        "METSMALL", "METLARGE", "CHILD", "EXPLODE", "WAKE", "SPLASH1", "SPLASH2",
    ] {
        registry.bind_anim_frame_count_for_test(name, 16);
    }
    rules.install_art_fixture(registry);
    rules
}

pub(in crate::sim) fn formed_sim(seed: u64, level: u8) -> Simulation {
    let mut sim = Simulation::with_seed(seed);
    let terrain = test_grid(128, 128, |x, y| {
        let mut c = test_flat_cell(x, y);
        c.level = level;
        c.allows_tiberium = true;
        //Declared passable Clear-land input, including the actual 89EA40 row.
        c.speed_costs.track = Some(100);
        c.base_speed_costs.track = Some(100);
        c
    });
    terrain.shared_cell_dummy().set_level_slope(level as i8, 0);
    sim.install_resolved_terrain_for_new_map(terrain);
    sim.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(128, 128));
    sim.session.map_width = 128;
    sim.session.map_height = 128;
    sim.session.binary_frame = 1000;
    sim
}

fn assert_rng(sim: &Simulation, expected: &Value, when: &str) {
    assert_eq!(
        sha256_hex(sim.scenario_rng.native_state_hex().as_bytes()),
        expected[format!("rng_{when}_sha256")].as_str().unwrap()
    );
}

#[test]
fn original_meteor_shower_controls_preserve_signed_spread_and_interleaved_constructors() {
    let corpus = crate::test_fixture::json("tools/spatial_oracle/trigger_meteor_shower.json");
    let rules = formed_rules("");
    let rows = corpus["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 18);
    for row in rows {
        let input = &row["input"];
        let expected = &row["expected"];
        let mut sim = formed_sim(input["seed"].as_u64().unwrap(), 2);
        let mut native = sim.scenario_rng.native_state_hex();
        for change in input["changes"].as_array().unwrap() {
            let index = change["index"].as_u64().unwrap() as usize;
            let bytes = (change["value"].as_u64().unwrap() as u32).to_le_bytes();
            let replacement: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
            let offset = (12 + 4 * index) * 2;
            native.replace_range(offset..offset + 8, &replacement);
        }
        sim.scenario_rng = SimRng::from_native_state_hex_for_test(&native);
        let waypoint = input["waypoint"].as_u64().unwrap() as u32;
        sim.session.install_waypoints(&HashMap::from([(
            waypoint,
            Waypoint {
                index: waypoint,
                rx: input["cell"][0].as_i64().unwrap() as u16,
                ry: input["cell"][1].as_i64().unwrap() as u16,
            },
        )]));
        assert_rng(&sim, expected, "before");
        assert_eq!(
            sim.trigger_meteor_shower(
                &rules,
                input["intensity"].as_i64().unwrap() as i32,
                waypoint as i32
            ),
            expected["result"] == 1
        );
        let actual:Vec<_>=sim.anims().map(|(_, anim)|serde_json::json!({"type":sim.interner.resolve(anim.type_id),"location":[anim.world_coord.x,anim.world_coord.y,anim.world_coord.z],"velocity_bits":anim.bounce.unwrap().velocity.map(|v|v.bits())})).collect();
        let pieces:Vec<_>=expected["pieces"].as_array().unwrap().iter().map(|p|serde_json::json!({"type":p["type"],"location":p["location"],"velocity_bits":p["velocity_bits"]})).collect();
        assert_eq!(actual, pieces, "{input}");
        assert_rng(&sim, expected, "after");
        assert!(
            sim.anims()
                .all(|(_, a)| a.draw_flags == 0x600 && !a.no_effects() && a.owner_house.is_none())
        );
        assert!(sim.voxel_anims().is_empty());
    }
}

#[test]
fn original_anim_ore_loop_keeps_direction_aliasing_density_and_constructor_refusals() {
    let corpus = crate::test_fixture::json("tools/spatial_oracle/trigger_meteor_ore.json");
    let rows = corpus["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 14);
    let text = "[OverlayTypes]\n0=TIB01\n1=TIB02\n2=TIB03\n3=TIB04\n[TIB01]\nTiberium=yes\nLand=Clear\n[TIB02]\nTiberium=yes\nLand=Clear\n[TIB03]\nTiberium=yes\nLand=Clear\n[TIB04]\nTiberium=yes\nLand=Clear\n[Clear]\nTrack=100%\n";
    let ini = IniFile::from_str(text);
    let registry = OverlayTypeRegistry::from_ini(&ini, None);
    let rules = formed_rules("");
    for row in rows {
        let input = &row["input"];
        let expected = &row["expected"];
        let mut sim = formed_sim(input["seed"].as_u64().unwrap(), 0);
        let mut config = rules.art().anim_runtime_config("METSMALL").unwrap().clone();
        config.tiberium_spread_radius = input["radius"].as_i64().unwrap() as i32;
        config.is_tiberium = input["ore"].as_bool().unwrap();
        config.tiberium_spawn_type = input["spawn"]
            .as_bool()
            .unwrap()
            .then(|| "TIB01".to_string());
        if input["refused"] == true {
            for y in 36..45 {
                for x in 36..45 {
                    sim.substrate.raw_cell_occupation.mark_ground(x, y, 0x40);
                }
            }
        }
        let mut position = glam::IVec3::from_array(std::array::from_fn(|i| {
            input["coord"][i].as_i64().unwrap() as i32
        }));
        if input["above"] == true {
            position.z = BRIDGE_DECK_HEIGHT_LEPTONS;
        }
        assert_rng(&sim, expected, "before");
        assert!(!sim.anim_bounce_landing(
            &config,
            &rules,
            Some(&registry),
            position,
            FrameEffects::empty()
        ));
        assert_rng(&sim, expected, "after");
        for cell in expected["cells"].as_array().unwrap() {
            let x = cell["cell"][0].as_u64().unwrap() as u16;
            let y = cell["cell"][1].as_u64().unwrap() as u16;
            let actual = sim.overlay_grid.as_ref().unwrap().cell(x, y);
            let overlay = cell["overlay"].as_i64().unwrap();
            assert_eq!(
                actual.overlay_id,
                (overlay >= 0).then_some(overlay as u8),
                "{input} at{x},{y}"
            );
            assert_eq!(
                actual.overlay_data,
                cell["density"].as_u64().unwrap() as u8,
                "{input} at{x},{y}"
            );
        }
        //Every native allocation spends an Abstract identity even if Mark
        //refuses; no child animations or other allocations exist in this case.
        assert_eq!(
            sim.native_identity_cursor().unwrap_or(0),
            expected["construct"].as_array().unwrap().len() as u32
        );
        assert!(sim.anims().next().is_none());
    }
}

#[test]
fn meteor_landing_runs_children_without_expire_and_uses_last_water_splash() {
    let rules = formed_rules("[METSMALL]\nIsMeteor=yes\nSpawns=CHILD\nSpawnCount=3\n");
    let mut sim = formed_sim(31, 0);
    let config = rules.art().anim_runtime_config("METSMALL").unwrap();
    let before = sim.scenario_rng.clone();
    let mut expected = before.clone();
    let count = expected
        .next_range_i32_inclusive(0, 3)
        .wrapping_add(expected.next_range_i32_inclusive(0, 3));
    assert!(count > 0);
    assert_eq!(config.spawns.as_deref(), Some("CHILD"));
    assert_eq!(config.spawn_count, 3);
    assert!(!sim.anim_bounce_landing(
        config,
        &rules,
        None,
        glam::IVec3::new(10368, 10368, 0),
        FrameEffects::empty()
    ));
    assert_eq!(sim.anims().count(), count as usize);
    assert_eq!(
        sim.scenario_rng.native_state_hex(),
        expected.native_state_hex()
    );
    let mut water = formed_sim(31, 0);
    water
        .resolved_terrain
        .as_mut()
        .unwrap()
        .cell_mut(40, 40)
        .unwrap()
        .land_type = 2;
    assert!(!water.anim_bounce_landing(
        config,
        &rules,
        None,
        glam::IVec3::new(10368, 10368, 0),
        FrameEffects::empty()
    ));
    let anims: Vec<_> = water.anims().map(|(_, a)| a).collect();
    assert_eq!(anims.len(), 1);
    assert_eq!(water.interner.resolve(anims[0].type_id), "SPLASH2");
    assert_eq!(anims[0].world_coord.z, 3);
    assert_eq!(
        water.scenario_rng.native_state_hex(),
        before.native_state_hex()
    );
}

#[test]
fn retail_meteor_profiles_keep_missing_stock_images_explicit() {
    let Some((rules_ini, art)) = crate::rules::retail_ini_fixture::retail_rules_and_art() else {
        return;
    };
    let Some(mut fixture) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let (_, assets) = crate::rules::retail_ini_fixture::retail_assets().unwrap();
    for name in [
        "METSMALL", "METLARGE", "METDEBRI", "CRYSTAL1", "CRYSTAL2", "CRYSTAL3", "CRYSTAL4",
    ] {
        let cfg = fixture.rules.art().anim_runtime_config(name).unwrap();
        let section = art.section_or_empty(name);
        assert_eq!(cfg.is_meteor, section.read_bool("IsMeteor", false));
        assert_eq!(cfg.is_tiberium, section.read_bool("IsTiberium", false));
        assert_eq!(cfg.spawn_count, section.read_int("SpawnCount", 0));
        assert_eq!(
            cfg.tiberium_spread_radius,
            section.read_int("TiberiumSpreadRadius", 0)
        );
        assert_eq!(
            cfg.spawns.as_deref(),
            section
                .read_name("Spawns", 0x80)
                .map(str::to_ascii_uppercase)
                .as_deref()
        );
    }
    assert!(
        fixture
            .rules
            .art()
            .anim_runtime_config("METSMALL")
            .unwrap()
            .is_meteor
    );
    for name in ["METSMALL", "METLARGE"] {
        let error = fixture
            .rules
            .bind_scheduler_anim_assets(&[name.to_string()], &assets, "TEM", "TEMPERATE")
            .unwrap_err();
        assert!(
            matches!(error, crate::rules::art_data::AnimAssetBindError::MissingShp(ref missing) if missing == name),
            "{error}"
        );
        assert!(
            fixture
                .rules
                .art()
                .anim_runtime_config(name)
                .unwrap()
                .raw_shp_frame_count
                .is_none()
        );
    }
    //The bounded stock Battle install lacks these SHPs. No substitute images
    //or invented frame counts are admitted; numeric/flight tests supply their
    //own explicit formed profiles and image bounds.
    let registry = OverlayTypeRegistry::from_ini(&rules_ini, Some(&art));
    let roots = crate::map::resolved_terrain::scheduler_anim_roots(&fixture.rules, &registry, &[]);
    for name in ["CRYSTAL1", "CRYSTAL2", "CRYSTAL3", "CRYSTAL4"] {
        let cfg = fixture.rules.art().anim_runtime_config(name).unwrap();
        if let Some(spawn) = cfg.tiberium_spawn_type.as_deref() {
            let id = registry.id_for_name(spawn).unwrap();
            if let Some(cell_anim) = registry.flags(id).unwrap().cell_anim.as_deref() {
                assert!(roots.contains(&cell_anim.to_ascii_uppercase()));
            }
        }
    }
}

#[test]
fn meteor_flight_restores_native_rng_prior_and_cleans_up() {
    let rules = formed_rules(
        "[METSMALL]\nIsMeteor=yes\nMinZVel=-50\nMaxXYVel=100\nElasticity=0\nSpawns=CHILD\nSpawnCount=3\n[METLARGE]\nIsMeteor=yes\nMinZVel=-50\nMaxXYVel=100\nElasticity=0\nSpawns=CHILD\nSpawnCount=3\n",
    );
    let mut sim = formed_sim(31, 2);
    sim.session.install_waypoints(&HashMap::from([(
        7,
        Waypoint {
            index: 7,
            rx: 40,
            ry: 40,
        },
    )]));
    assert!(sim.trigger_meteor_shower(&rules, 0, 7));
    let ids: Vec<_> = sim.anims().map(|(&id, _)| id).collect();
    assert!(!ids.is_empty());
    sim.visit_anim(ids[0], &rules, None, FrameEffects::empty());
    let bytes =
        crate::sim::snapshot::GameSnapshot::save_validated(&sim, 17, 23, "meteor flight", 0);
    let mut restored = crate::sim::snapshot::GameSnapshot::load_validated(&bytes, 17, 23, "")
        .unwrap()
        .sim;
    //Terrain is an asset-derived resource skipped by Simulation's snapshot.
    //This flight has not mutated the formed template; bind that same resource
    //before comparing hashes or continuing its ground queries after loading.
    restored.retain_process_state_for_load(crate::sim::world::SnapshotProcessState::Live(&sim));
    restored.resolved_terrain = sim.resolved_terrain.clone();
    restored.restore_after_snapshot_load().unwrap();
    assert_eq!(
        sim.live_object_order_snapshot(),
        restored.live_object_order_snapshot()
    );
    assert_eq!(
        sim.anims().map(|(_, a)| a.clone()).collect::<Vec<_>>(),
        restored.anims().map(|(_, a)| a.clone()).collect::<Vec<_>>(),
        "restored meteor bodies"
    );
    //ScenarioLoad689470 ->post-read683560 resets the saved Scenario RNG to
    //Seed(0). Compare the continued flight to that native load prior, rather
    //than requiring the deliberately discarded pre-save cursor to survive.
    assert_eq!(
        restored.scenario_rng.native_state_hex(),
        SimRng::new(0).native_state_hex()
    );
    sim.scenario_rng = SimRng::new(0);
    assert_eq!(sim.state_hash(), restored.state_hash());
    let other_streams = [
        sim.main_rng.native_state_hex(),
        sim.mapgen_rng.native_state_hex(),
    ];
    for _ in 0..100 {
        for world in [&mut sim, &mut restored] {
            world.session.binary_frame = world.session.binary_frame.wrapping_add(1);
            for &id in &ids {
                world.visit_anim(id, &rules, None, FrameEffects::empty());
            }
            world.flush_pending_delete();
        }
    }
    assert_eq!(sim.state_hash(), restored.state_hash());
    assert!(ids.iter().all(|&id| sim.anim(id).is_none()));
    assert_eq!(
        [
            sim.main_rng.native_state_hex(),
            sim.mapgen_rng.native_state_hex()
        ],
        other_streams
    );
}

#[test]
fn strict_and_tolerant_spawn_closures_bind_cycles_with_explicit_custom_images() {
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let art = IniFile::from_str(
        "[METSMALL]\nImage=BEACON\nSpawns=METLARGE\n[METLARGE]\nImage=BEACON\nSpawns=CHILD\n[CHILD]\nImage=BEACON\nSpawns=METSMALL\n",
    );
    let mut rules = formed_rules("");
    for strict in [true, false] {
        rules.install_art_fixture(ArtRegistry::from_ini(&art));
        let roots = vec!["METSMALL".to_string()];
        if strict {
            rules
                .bind_scheduler_anim_assets(&roots, &assets, "TEM", "TEMPERATE")
                .unwrap();
        } else {
            assert_eq!(
                rules.bind_anim_class_assets(&roots, &assets, "TEM", "TEMPERATE"),
                0
            );
        }
        let bound = rules.art().scheduler_anim_types();
        assert_eq!(bound.len(), 3);
        for name in ["METSMALL", "METLARGE", "CHILD"] {
            assert!(bound.contains(name));
            assert_eq!(
                rules
                    .art()
                    .anim_runtime_config(name)
                    .unwrap()
                    .raw_shp_frame_count,
                Some(10)
            );
        }
    }
    let ini = IniFile::from_str(
        "[Animations]\n0=CHILD\n[OverlayTypes]\n0=TIB01\n[TIB01]\nTiberium=yes\nCellAnim=CHILD\n",
    );
    let registry = OverlayTypeRegistry::from_ini(&ini, Some(&art));
    assert!(
        crate::map::resolved_terrain::scheduler_anim_roots(&rules, &registry, &[])
            .contains(&"CHILD".to_string())
    );
}
