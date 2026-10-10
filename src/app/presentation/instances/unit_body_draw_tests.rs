//! Voxel type -> production constructor -> body draw -> the layers the unit
//! atlas seeds for the drawn model.
//!
//! `UnitClass::DrawVoxelBody` (`0x0073B470`) takes its turret arm on the draw
//! type's `Turret=` (`0x0073B7A3`), and `UnitClass::DrawIt` makes a
//! `Harvester=` unit's `UnloadingClass=` the draw type while `Unit+0x6D1` is
//! set (`0x0073D29C..0x0073D2C4`). `AircraftClass::Draw_It` (`0x004144B0`)
//! draws the main voxel alone.
use super::*;
use crate::assets::asset_manager::AssetManager;
use crate::map::source::test_support::TestDirectory;
use crate::rules::ini_parser::IniFile;
use crate::rules::retail_ini_fixture::{retail_assets, retail_battle_rules};
use crate::rules::ruleset::RuleSet;
use crate::sim::voxel_frame_catalog::{
    detect_hva_frame_count, seed_layers_for, unit_atlas_variants, voxel_image_id,
};
use crate::sim::world::Simulation;

/// One body draw of `id`: its model, and whether it asks the atlas for
/// separate hull and turret sprites. A model the atlas does not seed for the
/// object's type, or seeds without the sprite asked for, is an `Err`.
fn body_draw_against_seed(
    sim: &Simulation,
    assets: &AssetManager,
    rules: &RuleSet,
    id: u64,
) -> Result<Option<(String, bool)>, String> {
    let entity = sim.entities().get(id).expect("the object was just built");
    let base_type = sim.interner.resolve(entity.type_ref());
    let Some((model, body)) = unit_body_draw(
        entity,
        &sim.interner,
        Some(rules),
        EntityDrawBand::Ground,
        sim.session.binary_frame,
        crate::render::draw_state::ObserverDrawContext::default(),
    ) else {
        return Ok(None);
    };
    if !unit_atlas_variants(base_type, Some(rules)).contains(&model.to_string()) {
        return Err(format!(
            "{base_type} draws {model}, which the atlas never seeds"
        ));
    }
    let seeded = seed_layers_for(assets, &model, Some(rules));
    let (asked, parts) = match body {
        BodyDraw::Turret { .. } => (VxlLayer::Body, true),
        BodyDraw::Composite => (
            if draws_turret_parts(&model, Some(rules), 0) {
                VxlLayer::Body
            } else {
                VxlLayer::Composite
            },
            false,
        ),
        BodyDraw::Pose(_) => return Err(format!("{base_type} takes a locomotor pose")),
    };
    if !seeded.iter().any(|&(layer, _)| layer == asked) {
        return Err(format!(
            "{base_type} draws {model} from its {asked:?} sprite, but the atlas seeds {seeded:?}"
        ));
    }
    Ok(Some((model.into_owned(), parts)))
}

/// The dock latch's presentation write (`refinery_dock::set_unload_latch`).
fn latch_unloading_image(sim: &mut Simulation, id: u64, image: &str) {
    let image = sim.interner.intern(image);
    sim.entities_mut()
        .get_mut(id)
        .expect("the object was just built")
        .display_type_override = Some(image);
}

#[test]
fn the_turret_split_follows_the_drawn_model_not_the_objects_turret() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "\
[VehicleTypes]
0=MINER
1=EMPTY
2=PLAIN
3=GUNNED
[AircraftTypes]
0=JET
[MINER]
Harvester=yes
Turret=yes
UnloadingClass=EMPTY
[EMPTY]
[PLAIN]
Harvester=yes
UnloadingClass=GUNNED
[GUNNED]
Turret=yes
[JET]
Turret=yes
",
    ))
    .expect("the draw-type rules parse");
    let directory = TestDirectory::new("unit-body-draw");
    let assets = AssetManager::from_loose_root_for_test(directory.path());
    let mut sim = Simulation::new();
    let spawn = |sim: &mut Simulation, type_id: &str| {
        sim.spawn_object_limbo_at_height(type_id, "Americans", 10, 10, 0x40, 0, &rules)
            .unwrap_or_else(|| panic!("build {type_id}"))
    };
    let draw = |sim: &Simulation, id: u64| body_draw_against_seed(sim, &assets, &rules, id);

    // A turreted miner draws its turret until it unloads as a turretless type.
    let miner = spawn(&mut sim, "MINER");
    assert!(sim.entities().get(miner).unwrap().barrel_facing.is_some());
    assert_eq!(draw(&sim, miner), Ok(Some(("MINER".to_string(), true))));
    latch_unloading_image(&mut sim, miner, "EMPTY");
    assert_eq!(draw(&sim, miner), Ok(Some(("EMPTY".to_string(), false))));

    // An aircraft keeps a Secondary facing and draws one voxel, `Turret=` or not.
    let jet = spawn(&mut sim, "JET");
    assert!(sim.entities().get(jet).unwrap().barrel_facing.is_some());
    assert_eq!(draw(&sim, jet), Ok(Some(("JET".to_string(), false))));

    // RESIDUAL pinned: an object with no turret facing of its own draws a
    // turreted model's turret at its hull's facing.
    let plain = spawn(&mut sim, "PLAIN");
    assert!(sim.entities().get(plain).unwrap().barrel_facing.is_none());
    assert_eq!(draw(&sim, plain), Ok(Some(("PLAIN".to_string(), false))));
    latch_unloading_image(&mut sim, plain, "GUNNED");
    assert_eq!(draw(&sim, plain), Ok(Some(("GUNNED".to_string(), true))));
    let entity = sim.entities().get(plain).unwrap();
    let frame = sim.session.binary_frame;
    let ground = EntityDrawBand::Ground;
    let (_, body) = unit_body_draw(
        entity,
        &sim.interner,
        Some(&rules),
        ground,
        frame,
        crate::render::draw_state::ObserverDrawContext::default(),
    )
    .expect("the miner is not in a deploy transition");
    let hull = entity.body_facing_current(frame);
    assert!(matches!(body, BodyDraw::Turret { turret, .. } if turret == hull));
}

/// DrawIt suppresses both transition bodies (73CF46/73CF54), and its settled
/// deployed body uses UnloadingClass (73D2F2..73D30C). This exercises the
/// production draw decision against the atlas's model/layer coverage.
#[test]
fn simple_deploy_transitions_hide_the_body_and_settled_state_selects_its_model() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=HELI\n1=GUNNED\n\
         [HELI]\nIsSimpleDeployer=yes\nUnloadingClass=GUNNED\n\
         [GUNNED]\nTurret=yes\n",
    ))
    .expect("simple deploy draw rules");
    let directory = TestDirectory::new("simple-deploy-body-draw");
    let assets = AssetManager::from_loose_root_for_test(directory.path());
    let mut sim = Simulation::new();
    let id = sim
        .spawn_object_limbo_at_height("HELI", "Americans", 10, 10, 0x40, 0, &rules)
        .expect("the simple deployer is built");

    for (deployed, begin, reverse, expected) in [
        (false, false, false, Some(("HELI", false))),
        (false, true, false, None),
        (true, false, false, Some(("GUNNED", true))),
        (true, false, true, None),
        (false, false, false, Some(("HELI", false))),
    ] {
        sim.entities_mut()
            .get_mut(id)
            .unwrap()
            .set_unit_simple_deploy_for_test(deployed, begin, reverse);
        assert_eq!(
            body_draw_against_seed(&sim, &assets, &rules, id),
            Ok(expected.map(|(model, parts)| (model.to_string(), parts))),
            "Unit6E0={deployed}, 6E1={begin}, 6E2={reverse}"
        );
    }
}

#[test]
fn retail_siege_chopper_draws_schd_only_after_deployment_completes() {
    let Some(battle) = retail_battle_rules() else {
        return;
    };
    let (_, assets) = retail_assets().expect("the battle rules came from RA2_DIR");
    let rules = &battle.rules;
    let chopper = rules.object("SCHP").expect("retail Siege Chopper");
    assert!(chopper.is_simple_deployer);
    assert_eq!(chopper.unloading_class.as_deref(), Some("SCHD"));
    assert!(chopper.has_turret);
    assert!(rules.object("SCHD").unwrap().has_turret);
    // Physical retail HVA headers, recorded by unit_simple_deploy.json.
    assert_eq!(
        detect_hva_frame_count(&assets, "SCHP", VxlLayer::Body, 0, Some(rules)),
        2
    );
    assert_eq!(
        detect_hva_frame_count(&assets, "SCHD", VxlLayer::Body, 0, Some(rules)),
        1
    );
    let mut sim = Simulation::new();
    let id = sim
        .spawn_object_limbo_at_height("SCHP", "Americans", 10, 10, 0, 0, rules)
        .expect("retail Siege Chopper builds");
    assert!(sim.entities().get(id).unwrap().is_voxel);
    sim.entities_mut().get_mut(id).unwrap().body_frame_counter = 1;
    let frame_counts = std::collections::BTreeMap::from([
        (("SCHP".to_string(), VxlLayer::Body, 0), 2),
        (("SCHD".to_string(), VxlLayer::Body, 0), 1),
    ]);
    for (deployed, begin, reverse, expected) in [
        (false, false, false, Some(("SCHP", true))),
        (false, true, false, None),
        (true, false, false, Some(("SCHD", true))),
        (true, false, true, None),
        (false, false, false, Some(("SCHP", true))),
    ] {
        sim.entities_mut()
            .get_mut(id)
            .unwrap()
            .set_unit_simple_deploy_for_test(deployed, begin, reverse);
        assert_eq!(
            body_draw_against_seed(&sim, &assets, rules, id),
            Ok(expected.map(|(model, parts)| (model.to_string(), parts))),
        );
        if let Some((model, _)) = expected {
            assert_eq!(
                unit_animation_frames(sim.entities().get(id).unwrap(), model, 0, &frame_counts).0,
                if model == "SCHP" { 1 } else { 0 },
                "an odd SCHP counter must still draw the sole SCHD HVA frame"
            );
        }
    }
}

#[test]
fn unit_hva_frames_match_native_selected_model_remainders() {
    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/unit_simple_deploy.json",
    ))
    .expect("native simple deployment draw controls");
    let frames_by_name = native["retail_art"]["physical_hva"].as_array().unwrap();
    let physical_frames = |name| {
        frames_by_name
            .iter()
            .find(|row| row["name"] == name)
            .unwrap()["frames"]
            .as_u64()
            .unwrap() as u32
    };
    let mut frame_counts = std::collections::BTreeMap::new();
    for (model, frames) in [
        ("actual", physical_frames("schp.hva")),
        ("unloading", physical_frames("schd.hva")),
        ("disguise", 3), // supplied native control HVA header
    ] {
        frame_counts.insert((model.to_string(), VxlLayer::Body, 0), frames);
        frame_counts.insert((model.to_string(), VxlLayer::Turret, 0), 4);
    }
    let mut entity =
        crate::sim::game_entity::GameEntity::test_default(1, "SCHP", "Americans", 10, 10);
    let rows = native["draw_frames"].as_array().unwrap();
    assert_eq!(rows.len(), 56);
    for row in rows {
        entity.body_frame_counter = row["body_counter"].as_i64().unwrap() as u32;
        entity.turret_anim_frame = row["turret_counter"].as_i64().unwrap() as i32;
        let (body, turret) =
            unit_animation_frames(&entity, row["selected"].as_str().unwrap(), 0, &frame_counts);
        assert_eq!(
            body as i32,
            row["body_frame"].as_i64().unwrap() as i32,
            "{row}"
        );
        assert_eq!(
            turret as i32,
            row["turret_frame"].as_i64().unwrap() as i32,
            "{row}"
        );
    }
}

/// The deployed override follows disguise in both DrawIt and DrawVoxelBody;
/// a null UnloadingClass leaves their previous model choice intact.
#[test]
fn deployed_model_overrides_disguise_no_spawn_alt_and_miner_display_hints() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=HELI\n1=GUNNED\n2=DECOY\n3=MINERHINT\n4=NOMODEL\n5=HINTED\n\
         [AircraftTypes]\n0=DRONE\n\
         [HELI]\nIsSimpleDeployer=yes\nUnloadingClass=GUNNED\n\
         NoSpawnAlt=yes\nSpawns=DRONE\nSpawnsNumber=1\n\
         [GUNNED]\nTurret=yes\n[DECOY]\n[MINERHINT]\n\
         [NOMODEL]\nIsSimpleDeployer=yes\n\
         [HINTED]\nIsSimpleDeployer=yes\nUnloadingClass=GUNNED\n[DRONE]\n",
    ))
    .expect("deployed model precedence rules");
    let mut sim = Simulation::new();
    let id = sim
        .spawn_object_limbo_at_height("HELI", "Americans", 10, 10, 0, 0, &rules)
        .expect("the simple deployer is built");
    let no_model = sim
        .spawn_object_limbo_at_height("NOMODEL", "Americans", 12, 10, 0, 0, &rules)
        .expect("the simple deployer without an unloading model is built");
    let hinted = sim
        .spawn_object_limbo_at_height("HINTED", "Americans", 14, 10, 0, 0, &rules)
        .expect("the simple deployer with a display hint is built");
    let disguise = sim.interner.intern("DECOY");
    latch_unloading_image(&mut sim, id, "MINERHINT");
    {
        let entity = sim.entities_mut().get_mut(id).unwrap();
        entity.spawn_manager.as_mut().unwrap().slots.clear();
        entity
            .disguise
            .get_or_insert_with(Default::default)
            .acquire(0, Some(disguise), None);
    }
    let model = |sim: &Simulation, id| {
        drawn_model_id(
            sim.entities().get(id).unwrap(),
            &sim.interner,
            Some(&rules),
            0,
            crate::render::draw_state::ObserverDrawContext {
                disguise_cell_present: true,
                ..Default::default()
            },
        )
        .into_owned()
    };
    latch_unloading_image(&mut sim, hinted, "MINERHINT");
    assert_eq!(model(&sim, hinted), "MINERHINT");
    sim.entities_mut()
        .get_mut(hinted)
        .unwrap()
        .set_unit_simple_deploy_for_test(true, false, false);
    assert_eq!(model(&sim, hinted), "GUNNED");
    assert_eq!(model(&sim, id), "DECOY");
    sim.entities_mut()
        .get_mut(id)
        .unwrap()
        .set_unit_simple_deploy_for_test(true, false, false);
    assert_eq!(model(&sim, id), "GUNNED");
    sim.entities_mut()
        .get_mut(id)
        .unwrap()
        .disguise
        .as_mut()
        .unwrap()
        .clear_unit();
    assert_eq!(model(&sim, id), "GUNNED");
    sim.entities_mut()
        .get_mut(id)
        .unwrap()
        .set_unit_simple_deploy_for_test(false, false, false);
    assert_eq!(model(&sim, id), "HELIWO");

    let entity = sim.entities_mut().get_mut(no_model).unwrap();
    entity
        .disguise
        .get_or_insert_with(Default::default)
        .acquire(0, Some(disguise), None);
    entity.set_unit_simple_deploy_for_test(true, false, false);
    assert_eq!(model(&sim, no_model), "DECOY");
}

/// Every voxel body in the stock Hills/Battle fixture must ask the atlas for
/// the sprites its drawn model was seeded with: the unloading War Miner for
/// HORV's one body sprite, an aircraft for its one body sprite. Other map,
/// mode and campaign layers are not covered by this fixture.
#[test]
fn retail_voxel_bodies_draw_the_sprites_their_model_is_seeded_with() {
    let Some(battle) = retail_battle_rules() else {
        return;
    };
    let (_, assets) = retail_assets().expect("the battle rules came from RA2_DIR");
    let rules = &battle.rules;
    let mut sim = Simulation::new();
    let mut failures = Vec::new();
    let mut voxel_vehicles = 0;
    let mut aircraft = Vec::new();
    let mut unloading = Vec::new();
    for type_id in rules.vehicle_ids.iter().chain(&rules.aircraft_ids) {
        let object = rules.object(type_id).expect("a listed type");
        let Some(id) = sim.spawn_object_limbo_at_height(type_id, "Americans", 10, 10, 0, 0, rules)
        else {
            continue;
        };
        let entity = sim.entities().get(id).expect("the object was just built");
        if !entity.is_voxel {
            continue;
        }
        let is_aircraft = entity.category == EntityCategory::Aircraft;
        match body_draw_against_seed(&sim, &assets, rules, id) {
            Ok(Some((model, parts))) => {
                assert_eq!(&model, type_id);
                if is_aircraft {
                    aircraft.push((type_id.clone(), parts));
                } else {
                    voxel_vehicles += 1;
                }
            }
            Ok(None) => failures.push(format!("{type_id} unexpectedly hides its body")),
            Err(failure) => failures.push(failure),
        }
        let Some(image) = object
            .unloading_class
            .as_deref()
            .filter(|image| object.harvester && rules.object(image).is_some())
        else {
            continue;
        };
        latch_unloading_image(&mut sim, id, image);
        match body_draw_against_seed(&sim, &assets, rules, id) {
            Ok(Some((model, parts))) => unloading.push((type_id.clone(), model, parts)),
            Ok(None) => failures.push(format!("unloading: {type_id} unexpectedly hides its body")),
            Err(failure) => failures.push(format!("unloading: {failure}")),
        }
    }
    assert_eq!(failures, Vec::<String>::new());
    assert!(
        voxel_vehicles > 40,
        "{voxel_vehicles} retail voxel vehicles"
    );
    // Every aircraft in this stock Hills/Battle fixture is one body sprite.
    assert_eq!(aircraft.len(), 12, "{aircraft:?}");
    assert!(aircraft.iter().all(|(_, parts)| !parts), "{aircraft:?}");
    // The War Miner's HARV has `Turret=yes`; its unloading HORV has none.
    unloading.sort();
    assert_eq!(
        unloading,
        [
            ("CMIN".to_string(), "CMON".to_string(), false),
            ("HARV".to_string(), "HORV".to_string(), false),
        ]
    );
    // Only a vehicle model appends `%sTUR` and `%sBARL`. Aircraft never draw
    // those parts. Buildings instead resolve native B8/C0 slots by rewriting
    // TUR inside TurretAnim (45FA90); their own gun loader has separate retail
    // coverage in unit_atlas_tests. None uses these appended filenames.
    let turret_models: Vec<&str> = rules
        .building_ids
        .iter()
        .filter_map(|type_id| rules.object(type_id))
        .filter(|object| object.turret_anim_is_voxel)
        .filter_map(|object| object.turret_anim.as_deref())
        .collect();
    assert_eq!(turret_models.len(), 8, "{turret_models:?}");
    assert!(turret_models.contains(&"YAGGUN"), "{turret_models:?}");
    let aircraft_models = rules.aircraft_ids.iter().map(String::as_str);
    for model in aircraft_models.chain(turret_models) {
        let image = voxel_image_id(model, Some(rules));
        for part in ["TUR", "BARL"] {
            let file = format!("{image}{part}.VXL");
            assert!(assets.get_ref(&file).is_none(), "{file}");
        }
    }
    // The body's HVA frame is `Unit+0x538` modulo the draw type's frame count
    // (`0x0073B4DA..0x0073B4E7`); both retail unloading models therefore
    // select frame 0 independently of their persistent body counter.
    for model in ["HORV", "CMON"] {
        let frames = detect_hva_frame_count(&assets, model, VxlLayer::Composite, 0, Some(rules));
        assert_eq!(frames, 1, "{model}");
    }
    // The TurretCount admission is covered separately by native IFV controls.
    // Every stock Hills/Battle vehicle using it also sets Turret=yes.
    for type_id in &rules.vehicle_ids {
        let object = rules.object(type_id).expect("a listed type");
        assert!(
            object.turret_count <= 0 || object.has_turret,
            "{type_id} has TurretCount= without Turret="
        );
    }
}

#[test]
fn retail_mirage_unit_route_matches_original_observer_dispatch() {
    let Some(retail) = retail_battle_rules() else {
        return;
    };
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/mirage_disguise.json",
    ))
    .unwrap();
    let actual_name = corpus["drawing"][0]["actual_type"]["name"]
        .as_str()
        .unwrap();
    let disguise_name = corpus["initialization"]["tree_images"][0]["name"]
        .as_str()
        .unwrap();
    let rules = &retail.rules;
    let mut sim = Simulation::new();
    let id = sim
        .spawn_object_limbo_at_height(actual_name, "Americans", 10, 10, 0, 0, rules)
        .unwrap();
    let disguise_type = sim.interner.intern(disguise_name);
    let mut compared = 0;
    for row in corpus["observer"].as_array().unwrap() {
        let input = &row["input"];
        let frame = input["frame"].as_i64().unwrap() as u32;
        let blink = crate::sim::timer::CdTimer::from_raw(
            input["blink_start"].as_i64().unwrap() as i32,
            input["blink_duration"].as_i64().unwrap() as i32,
        );
        if !blink.expired(frame as i32) {
            // The separate blink producer is not retained in GameEntity yet.
            // DrawState's original-byte test covers that supplied input leaf.
            continue;
        }
        let actor = sim.entities_mut().get_mut(id).unwrap();
        let disguise = actor.disguise.as_mut().unwrap();
        if input["raw_disguised"].as_bool().unwrap() {
            disguise.acquire(
                input["creation"].as_i64().unwrap() as u32,
                Some(disguise_type),
                None,
            );
        } else {
            disguise.clear_unit();
        }
        let observer = crate::render::draw_state::ObserverDrawContext {
            owner_is_current_player: input["owner"].as_bool().unwrap(),
            disguise_cell_present: input["cell_present"].as_bool().unwrap(),
            detects_disguise: input["sensor_count"].as_i64().unwrap() > 0,
            ..Default::default()
        };
        let actor = sim.entities().get(id).unwrap();
        let model = drawn_model_id(actor, &sim.interner, Some(rules), frame, observer);
        let pointer = |field: &str| {
            u64::from_str_radix(row[field].as_str().unwrap().strip_prefix("0x").unwrap(), 16)
                .unwrap()
        };
        let native_selected = row["outputs"]["draw_route_type"].as_u64().unwrap();
        let expected = if native_selected == pointer("actual_type") {
            actual_name
        } else {
            assert_eq!(native_selected, pointer("disguise_type"));
            disguise_name
        };
        assert_eq!(model, expected, "{}", row["name"]);
        let voxel =
            super::super::helpers::drawn_type_uses_voxel(actor, &model, &sim.interner, Some(rules));
        assert_eq!(
            u64::from(voxel),
            row["outputs"]["draw_route_voxel"].as_u64().unwrap(),
            "{}",
            row["name"]
        );
        assert_eq!(
            unit_body_draw(
                actor,
                &sim.interner,
                Some(rules),
                EntityDrawBand::Ground,
                frame,
                observer
            )
            .is_some(),
            voxel,
            "voxel builder emits only its selected encoding"
        );
        compared += 1;
    }
    assert!(
        compared >= 30,
        "only the explicit active-blink producer is excluded"
    );
}

#[test]
fn retail_mirage_body_frame_matches_original_unit_shp() {
    let Some(retail) = retail_battle_rules() else {
        return;
    };
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/mirage_disguise.json",
    ))
    .unwrap();
    let rules = &retail.rules;
    let mut compared = 0;
    for row in corpus["drawing"].as_array().unwrap() {
        let body = row["draws"]
            .as_array()
            .unwrap()
            .first()
            .expect("the selected original UnitSHP call reaches its body blit");
        let name = row["actual_type"]["name"].as_str().unwrap();
        let actual = rules.object(name).unwrap();
        let art = rules
            .art()
            .resolve_metadata_entry(name, &actual.image)
            .unwrap();
        assert!(art.voxel, "the ordinary image enters the voxel builder");
        // Unit73C5F0 reads the actual UnitType layout and Foot+538 even
        // though GetImage4DED70 selects the physical TerrainType SHP.
        let set = rules.animation_sequence(name).unwrap();
        assert_eq!(
            i64::from(
                set.get(&crate::sim::animation::SequenceKind::Stand)
                    .unwrap()
                    .facings
            ),
            row["actual_type"]["facings"].as_i64().unwrap(),
            "{name}: retained original UnitType ART reader feeds the final layout"
        );
        let input = &row["input"];
        let body_frame = crate::sim::animation::resolve_shp_vehicle_body_frame(
            set,
            (input["facing_u16"].as_u64().unwrap() >> 8) as u8,
            input["body_counter"].as_u64().unwrap() as u32,
            input["is_moving"].as_bool().unwrap(),
            true,
        )
        .unwrap();
        assert_eq!(
            u64::from(body_frame),
            body["frame"].as_u64().unwrap(),
            "{}",
            row["name"]
        );
        assert_eq!(row["rng_before"], row["rng_after"], "{}", row["name"]);
        compared += 1;
    }
    assert_eq!(compared, 6, "all admitted stock Unit SHP rows");
}

/// The selected gun's HVA count belongs to its index, rather than the base
/// gun's count. Arithmetic/control vectors remain in the native frame test
/// above; this regression checks its catalogue binding at presentation.
#[test]
fn indexed_turret_animation_uses_its_selected_hva_count() {
    let mut entity =
        crate::sim::game_entity::GameEntity::test_default(1, "FV", "Americans", 10, 10);
    entity.turret_anim_frame = 1;
    let frames = std::collections::BTreeMap::from([
        (("FV".to_string(), VxlLayer::Body, 0), 1),
        (("FV".to_string(), VxlLayer::Turret, 0), 1),
        (("FV".to_string(), VxlLayer::Turret, 1), 2),
    ]);
    assert_eq!(unit_animation_frames(&entity, "FV", 0, &frames), (0, 0));
    assert_eq!(unit_animation_frames(&entity, "FV", 1, &frames), (0, 1));
}
