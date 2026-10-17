//! Original ordinary Techno status calls through the actual shared
//! producer, retail resources and production replay. Location, HP, selection,
//! hover and surface planes are consumer inputs, not lifecycle parity claims.
use super::draw_plan_lowering::{
    NativeDisplayOrder, PlannedObjectInstance, lower_ground_object_instances,
};
use crate::app::presentation::ui_overlays::{StatusViewport, techno_extras_for_parent};
use crate::assets::asset_manager::{AssetManager, MediaArchiveMode};
use crate::map::entities::EntityCategory;
use crate::render::batch::{BatchRenderer, InstanceBufferPool};
use crate::render::selection_overlay::SelectionOverlay;
use crate::render::shroud_buffer::ShroudBuffer;
use crate::render::tactical_draw_plan::SpriteEncoding;
use crate::render::terrain_draw::TerrainDrawRenderer;
use crate::render::terrain_draw_gpu_tests::{Gpu, camera, clear, encoded};
use crate::sim::{
    combat::Health, components::DriveCoord, game_entity::GameEntity, world::Simulation,
};
use serde_json::Value;

#[test]
#[ignore = "requires retail assets and GPU; original MTNK health strip A/Z and Display replay"]
fn native_mobile_health_strips_preserve_color_a_and_z() {
    native_status_controls(
        "tools/procedural_drawing_oracle/unit_health.json",
        "MTNK",
        EntityCategory::Unit,
    );
}

#[test]
#[ignore = "requires retail assets and GPU; original GAPOWR health colors, phases and unchanged A/Z"]
fn native_building_health_strips_preserve_color_a_and_z() {
    native_status_controls(
        "tools/procedural_drawing_oracle/building_health.json",
        "GAPOWR",
        EntityCategory::Structure,
    );
}

#[test]
#[ignore = "requires retail assets and GPU; original MTNK rank prefix A/Z and parent order"]
fn native_mobile_rank_pips_preserve_color_a_and_z() {
    native_status_controls(
        "tools/procedural_drawing_oracle/unit_rank.json",
        "MTNK",
        EntityCategory::Unit,
    );
}

#[test]
#[ignore = "requires retail assets and GPU; original FV passenger PIPS2 and affected scale consumers"]
fn native_passenger_pips_preserve_color_a_z_and_parent_order() {
    native_status_controls(
        "tools/procedural_drawing_oracle/passenger_pips.json",
        "FV",
        EntityCategory::Unit,
    );
}

#[test]
#[ignore = "requires retail assets and GPU; original ORCA ammo pips and native Aircraft projection"]
fn native_aircraft_ammo_pips_preserve_color_a_z_and_parent_order() {
    native_status_controls(
        "tools/procedural_drawing_oracle/ammo_pips.json",
        "ORCA",
        EntityCategory::Aircraft,
    );
}

#[test]
#[ignore = "requires retail assets and GPU; original MTNK group font pixels and parent composition"]
fn native_control_group_labels_preserve_color_a_z_and_parent_order() {
    native_status_controls(
        "tools/procedural_drawing_oracle/control_group_status.json",
        "MTNK",
        EntityCategory::Unit,
    );
}

#[test]
#[ignore = "requires retail assets and GPU; original GAPOWR wrench prefix, MOUSEPAL N1 and both Extras visits"]
fn native_repair_wrenches_preserve_color_a_z_and_parent_order() {
    native_status_controls(
        "tools/procedural_drawing_oracle/repair_wrench.json",
        "GAPOWR",
        EntityCategory::Structure,
    );
}

#[test]
#[ignore = "requires retail assets and GPU; original Ground2 PRIMARY loop, physical text and unchanged A/Z"]
fn native_primary_factory_labels_preserve_color_a_z_and_constructor_order() {
    use crate::app::presentation::ui_overlays::build_primary_factory_status_render_instances;
    use crate::sim::{command::Command, house_state::HouseState};
    use std::collections::BTreeSet;

    let native =
        crate::test_fixture::json("tools/procedural_drawing_oracle/primary_factory_status.json");
    let producer = crate::test_fixture::json("tools/spatial_oracle/building_manual_primary.json");
    assert_eq!(native["native_sha256"], producer["native_sha256"]);
    for (key, phase) in [("primary_on", 3), ("primary_off", 5)] {
        assert_eq!(
            native["producer_priors"][key]["value"],
            producer["cases"][0]["phases"][phase]["state"]["primary"][1],
            "the renderer's primary-byte priors come from original Event17 output"
        );
    }
    let retail = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
        .expect("physical Battle INIs");
    let root = std::path::PathBuf::from(std::env::var_os("RA2_DIR").expect("retail root"));
    let mut assets = AssetManager::new(&root, MediaArchiveMode::STOCK_DIGITAL).unwrap();
    crate::map::theater::load_theater(
        &mut assets,
        native["raw_file_inputs"]["theater"].as_str().unwrap(),
    )
    .expect("normal active theater assets");
    for name in native["raw_file_inputs"]["absent"].as_array().unwrap() {
        let name = name.as_str().unwrap();
        assert!(
            assets.get_ref(name).is_none(),
            "normal AssetManager confirms absent native filename probe {name}"
        );
    }
    for source in native["physical_inputs"].as_array().unwrap() {
        let name = source["name"].as_str().unwrap();
        assert_eq!(
            crate::util::sha256::sha256_hex(assets.get_ref(name).expect("physical input")),
            source["sha256"].as_str().unwrap(),
            "normal AssetManager winner for {name}"
        );
    }
    let csf_bytes = assets.get_ref("ra2md.csf").expect("active-YR CSF");
    assert_eq!(
        crate::util::sha256::sha256_hex(csf_bytes),
        native["retail_csf"]["sha256"].as_str().unwrap()
    );
    let csf = crate::assets::csf_file::CsfFile::from_bytes(csf_bytes).unwrap();
    for key in ["TXT_PRIMARY", "TXT_PRI"] {
        assert_eq!(
            csf.text(key).as_ref(),
            native["retail_csf"]["labels"][key].as_str().unwrap()
        );
    }
    let short_rules = primary_label_rules_with_fixed_art_override(&assets, &root);
    let gpu = Gpu::new();
    for format in [
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ] {
        let mut batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
        let overlay =
            SelectionOverlay::new_on_device(&gpu.device, &gpu.queue, &batch, Some(&assets));
        let fnt = crate::assets::fnt_file::FntFile::from_bytes(
            assets.get_ref("GAME.FNT").expect("physical GAME.FNT"),
        )
        .unwrap();
        let font =
            crate::render::bit_font::BitFont::from_fnt(&gpu.device, &gpu.queue, &batch, &fnt);
        let mut pool = InstanceBufferPool::new();
        for row in native["raster_controls"].as_array().unwrap() {
            let input = &row["input"];
            let rules = if input["foundation_override"].is_null() {
                &retail.rules
            } else {
                assert_eq!(input["foundation_override"], "1x1");
                &short_rules
            };
            let size = pair(&input["size"]);
            let clip: [u32; 4] =
                std::array::from_fn(|index| input["clip"][index].as_u64().unwrap() as u32);
            let mut cam = camera(size);
            cam.camera_pos = [
                input["camera"][0].as_i64().unwrap() as f32,
                input["camera"][1].as_i64().unwrap() as f32
                    + crate::util::lepton::absolute_leptons_to_screen(0, 0, 0).1,
            ];
            let mut sim = Simulation::new();
            sim.intern_rule_type_ids(rules);
            sim.resolve_type_handles(rules);
            let owner = sim.interner.intern("Owner");
            let ally = sim.interner.intern("Ally");
            sim.session.game_mode_nonzero = true;
            sim.session.current_house = Some(owner);
            for house in [owner, ally] {
                sim.houses
                    .insert(house, HouseState::new(house, 0, None, true, 0, 10));
                sim.session.house_order.push(house);
            }
            for (admitted, asker, other) in [
                (
                    input["owner_allied_to_current"].as_bool().unwrap(),
                    "ALLY",
                    "OWNER",
                ),
                (
                    input["current_allied_to_owner"].as_bool().unwrap(),
                    "OWNER",
                    "ALLY",
                ),
            ] {
                if admitted {
                    sim.house_alliances
                        .entry(asker.to_owned())
                        .or_default()
                        .insert(other.to_owned());
                }
            }
            let mut colors = crate::map::houses::HouseColorMap::new();
            let mut opportunities = BTreeSet::new();
            for (index, actor) in input["actors"].as_array().unwrap().iter().enumerate() {
                assert_eq!(
                    actor["display_bits"], 0,
                    "non-allied discovery producer is outside this control"
                );
                let id = index as u64 + 1;
                let name = actor["type_name"].as_str().unwrap();
                let object = rules.object(name).unwrap();
                assert_eq!(
                    crate::rules::foundation::foundation_dimensions(&object.foundation).0,
                    row["native_foundation_width"].as_u64().unwrap() as u16
                );
                let type_ref = sim.interner.intern(name);
                let actor_owner = if actor["owner"] == "Owner" {
                    owner
                } else {
                    ally
                };
                let xyz: [i32; 3] =
                    std::array::from_fn(|axis| actor["location"][axis].as_i64().unwrap() as i32);
                let mut entity = GameEntity::new_at_frame_zero_for_test(
                    id,
                    (xyz[0] / 256) as u16,
                    (xyz[1] / 256) as u16,
                    0,
                    0,
                    actor_owner,
                    Health::new(object.strength),
                    type_ref,
                    EntityCategory::Structure,
                    0,
                    5,
                    false,
                );
                entity
                    .type_facts
                    .set_foundation_for_test(object.foundation.clone());
                entity.position.put_location(DriveCoord {
                    x: xyz[0],
                    y: xyz[1],
                    z: xyz[2],
                });
                entity.lifecycle.set_in_limbo_for_test(false);
                entity
                    .lifecycle
                    .set_cell_marked(actor["marked"].as_bool().unwrap());
                entity
                    .selection
                    .set_selected_for_test(actor["selected"].as_bool().unwrap());
                sim.entities_mut().insert(entity);
                sim.append_house_base_building_for_test(id);
                if actor["primary"].as_bool().unwrap() {
                    assert!(sim.apply_command(
                        "Owner",
                        &Command::TogglePrimaryFactory { entity_id: id },
                        Some(rules),
                    ));
                }
                assert_eq!(
                    sim.production.is_primary_factory(id),
                    actor["primary"].as_bool().unwrap()
                );
                if actor["body_opportunity"].as_bool().unwrap() {
                    opportunities.insert(id);
                }
                let color = crate::rules::house_colors::HouseColorIndex(
                    rules
                        .color_schemes
                        .iter()
                        .position(|scheme| scheme.name == actor["color"].as_str().unwrap())
                        .unwrap() as u8,
                );
                colors.insert(actor["owner"].as_str().unwrap().to_owned(), color);
            }
            let labels = build_primary_factory_status_render_instances(
                &sim,
                &opportunities,
                owner,
                &colors,
                &rules.house_color_ramps,
                &font,
                &csf,
                cam.camera_pos,
                clip.map(|value| value as i32),
                rules,
            );
            assert_eq!(
                labels.runs.len(),
                row["text_draws"].as_array().unwrap().len(),
                "{} label admission",
                row["name"]
            );
            for (run, draw) in labels
                .runs
                .iter()
                .zip(row["text_draws"].as_array().unwrap())
            {
                let first = &labels.instances[run.text.start as usize];
                assert_eq!(
                    first.position,
                    [
                        draw["result_position"][0].as_i64().unwrap() as f32 + cam.camera_pos[0],
                        draw["result_position"][1].as_i64().unwrap() as f32 + cam.camera_pos[1],
                    ],
                    "{} original centered text origin",
                    row["name"]
                );
                assert!(draw["stages"].as_array().unwrap().iter().all(|stage| {
                    ["a_reads", "a_writes", "z_reads", "z_writes"]
                        .iter()
                        .all(|key| stage[*key].as_u64() == Some(0))
                }));
            }
            pool.upload_on_device(
                &gpu.device,
                &gpu.queue,
                "primary_factory_status",
                &labels.instances,
            );
            let a_input =
                vec![input["a_value"].as_u64().unwrap() as u8; (size[0] * size[1]) as usize];
            let a = ShroudBuffer::fixture(&gpu.device, &gpu.queue, size, cam.camera_pos, &a_input);
            batch.bind_shroud(&gpu.device, Some(&a));
            batch.write_camera(&gpu.queue, cam);
            let color = gpu.target(size, format);
            let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
            let cv = color.create_view(&Default::default());
            let dv = depth.create_view(&Default::default());
            let initial = encoded(input["background"].as_u64().unwrap() as u16, format)
                .repeat((size[0] * size[1]) as usize);
            write_color(&gpu, &color, size, &initial);
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            let old_z = input["old_z"].as_u64().unwrap() as u16;
            clear(&mut encoder, &cv, &dv, old_z, wgpu::LoadOp::Load);
            let mut terrain = TerrainDrawRenderer::new(&gpu.device, &gpu.queue, format, &batch);
            terrain.prepare(&gpu.device, &color, &dv, batch.camera_uniform());
            super::object_extras::draw_building_status_text(
                &mut encoder,
                &cv,
                &dv,
                &mut terrain,
                clip,
                &batch,
                Some(&overlay),
                pool.get("primary_factory_status").map(|(buffer, _)| buffer),
                Some(font.atlas()),
                &labels.runs,
            );
            let reads = [
                gpu.read(&mut encoder, &color),
                gpu.read(&mut encoder, &depth),
            ];
            let output = gpu.finish(encoder, &reads, size);
            assert_pixels(
                &output[0],
                &initial,
                &row["pixels"],
                size,
                format,
                false,
                row,
            );
            assert_z(&output[1], &vec![old_z; (size[0] * size[1]) as usize], row);
            for (actual, expected) in gpu
                .read_float_texels(a.gpu_source().0, [0, 0], size)
                .iter()
                .zip(&a_input)
            {
                assert_eq!(
                    (actual[0] * 255.0).round() as u8,
                    *expected,
                    "{} A unchanged",
                    row["name"]
                );
            }
        }
    }
}

/// The width1 control changes fixed ART before the production Rules process
/// starts; reparsing a merged scenario would lose native reader chronology.
fn primary_label_rules_with_fixed_art_override(
    assets: &AssetManager,
    root: &std::path::Path,
) -> crate::rules::ruleset::RuleSet {
    use crate::rules::{ini_parser::IniFile, retail_sources::RetailRulesSources};
    let mut sources = RetailRulesSources::select(assets).unwrap();
    sources
        .artmd
        .ini
        .merge(&IniFile::from_str("[GAWEAP]\nFoundation=1x1\n"));
    let audio = crate::rules::audio_sources::AudioDefinitions::select(assets);
    let mut owner = sources
        .into_owner(std::sync::Arc::clone(audio.sounds()))
        .unwrap();
    let mut scenario_assets = assets.clone();
    scenario_assets.register_neutral_archives().unwrap();
    crate::map::scenario_sources::list_skirmish_scenario_records_with_assets(
        root,
        &mut scenario_assets,
        None,
    )
    .unwrap();
    let map = crate::map::source::load_map_by_name_or_path_with_assets(
        root,
        "XMP03T4.MAP",
        &scenario_assets,
    )
    .unwrap();
    let mode =
        crate::rules::retail_sources::select_ini(&scenario_assets, "MPBattleMD.ini").unwrap();
    let (mut rules, _, art, _) = owner
        .load_scenario(
            crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(Some(&mode.ini)),
            &map.map.ini,
        )
        .unwrap()
        .into_parts();
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
    rules
}

fn native_status_controls(fixture: &'static str, type_name: &str, category: EntityCategory) {
    let native = crate::test_fixture::json(fixture);
    let retail = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
        .expect("physical Battle INIs");
    let rules = &retail.rules;
    let root = std::path::PathBuf::from(std::env::var_os("RA2_DIR").expect("retail root"));
    let assets = AssetManager::new(&root, MediaArchiveMode::STOCK_DIGITAL).unwrap();
    for source in native["physical_inputs"].as_array().unwrap() {
        let name = source["name"].as_str().unwrap();
        let (bytes, archive) = assets
            .get_with_source_ref(name)
            .expect("normal retail lookup");
        assert_eq!(
            crate::util::sha256::sha256_hex(bytes),
            source["sha256"].as_str().unwrap(),
            "physical input {name}"
        );
        if matches!(
            name.to_ascii_uppercase().as_str(),
            "WRENCH.SHP" | "MOUSEPAL.PAL"
        ) {
            eprintln!(
                "normal STOCK_DIGITAL {name}: {archive}, {} bytes, {}",
                bytes.len(),
                source["sha256"]
            );
        }
    }
    let gpu = Gpu::new();
    for format in [
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ] {
        let mut batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
        let overlay =
            SelectionOverlay::new_on_device(&gpu.device, &gpu.queue, &batch, Some(&assets));
        let mut pool = InstanceBufferPool::new();
        let font = native.get("native_font").map(|_| {
            let fnt = crate::assets::fnt_file::FntFile::from_bytes(
                assets.get_ref("GAME.FNT").expect("physical GAME.FNT"),
            )
            .expect("production font parser");
            crate::render::bit_font::BitFont::from_fnt(&gpu.device, &gpu.queue, &batch, &fnt)
        });
        for row in [
            "raster_controls",
            "composition_controls",
            "garrison_controls",
        ]
        .into_iter()
        .flat_map(|key| native[key].as_array().into_iter().flatten())
        {
            let composed = row.get("before_body_pixels").is_some();
            let wrench_composed = row.get("body_overwritten_wrench_pixels").is_some();
            let input = &row["input"];
            let missing_e1_pass = input["actors"]
                .as_array()
                .unwrap()
                .iter()
                .any(|actor| actor["unrelated_e1_pass"].as_bool().unwrap_or(false));
            let patched = input["scenario_patch"]
                .as_str()
                .or(missing_e1_pass.then_some("[E1]\nStrength=125\n"))
                .map(|source| {
                    crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch(
                        "XMP03T4.MAP",
                        &crate::rules::ini_parser::IniFile::from_str(source),
                    )
                    .expect("native admitted scenario section")
                });
            let rules = patched.as_ref().map_or(rules, |retail| &retail.rules);
            let size = pair(&input["size"]);
            let tactical_clip: [u32; 4] =
                input.get("clip").map_or([0, 0, size[0], size[1]], |clip| {
                    std::array::from_fn(|index| clip[index].as_u64().unwrap() as u32)
                });
            let mut cam = camera(size);
            // The shared projection's world-row origin is +15. Native camera
            // and original Location are supplied independently in the corpus.
            cam.camera_pos = [
                input["camera"][0].as_i64().unwrap() as f32,
                input["camera"][1].as_i64().unwrap() as f32
                    + crate::util::lepton::absolute_leptons_to_screen(0, 0, 0).1,
            ];
            let mut sim = Simulation::new();
            sim.session.binary_frame = input["frame"]
                .as_i64()
                .or(row["frame"].as_i64())
                .unwrap_or(0) as u32;
            sim.session.game_options.game_speed = input["speed"]
                .as_i64()
                .or(row["speed"].as_i64())
                .unwrap_or(1) as i32;
            if native.get("health_palette").is_some() {
                crate::sim::arena_fixture::flat_ground(&mut sim, rules);
            }
            let owner = sim.interner.intern("Owner");
            let enemy = sim.interner.intern("Enemy");
            if input.get("self_heal_infantry").is_some() || input.get("self_heal_units").is_some() {
                let mut house =
                    crate::sim::house_state::HouseState::new(owner, 0, None, true, 0, 10);
                crate::sim::arena_fixture::install_status_self_heal(
                    &mut house,
                    input["self_heal_infantry"].as_i64().unwrap_or(0) as i32,
                    input["self_heal_units"].as_i64().unwrap_or(0) as i32,
                );
                sim.houses.insert(owner, house);
            }
            sim.fog = crate::sim::vision::FogState::new(64, 64);
            sim.fog.reveal_all_for_owner(owner);
            let mut parents = Vec::new();
            let mut groups = vec![Vec::new(); 10];
            let mut house_colors = crate::map::houses::HouseColorMap::new();
            if font.is_some() {
                let gold = crate::rules::house_colors::HouseColorIndex(
                    rules
                        .color_schemes
                        .iter()
                        .position(|scheme| scheme.name == "Gold")
                        .expect("physical Gold scheme") as u8,
                );
                house_colors.insert("Owner".to_owned(), gold);
                house_colors.insert("Enemy".to_owned(), gold);
            }
            let mut row_has_building = false;
            let ids = if composed {
                row["body_admissions"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|body| if body["actor"] == 0 { 901 } else { 2 })
                    .collect()
            } else if input["actors"].as_array().unwrap().len() == 1 {
                vec![901]
            } else {
                vec![901, 2]
            };
            let display = NativeDisplayOrder::new(&ids);
            for (index, actor) in input["actors"].as_array().unwrap().iter().enumerate() {
                let id = if index == 0 { 901 } else { 2 };
                let name = actor["type_name"].as_str().unwrap_or(type_name);
                let object = rules.object(name).expect("native actor type");
                let type_ref = sim.interner.intern(name);
                let actor_category = EntityCategory::from(object.category);
                row_has_building |= actor_category == EntityCategory::Structure;
                let xyz: [i32; 3] =
                    std::array::from_fn(|i| actor["location"][i].as_i64().unwrap() as i32);
                let mut entity = GameEntity::new_at_frame_zero_for_test(
                    id,
                    (xyz[0] / 256) as u16,
                    (xyz[1] / 256) as u16,
                    0,
                    0,
                    if actor["owner_is_current"].as_bool().unwrap_or(true) {
                        owner
                    } else {
                        enemy
                    },
                    Health::new(
                        actor["health"]
                            .as_i64()
                            .unwrap_or(i64::from(object.strength)) as i32,
                    ),
                    type_ref,
                    actor_category,
                    0,
                    5,
                    false,
                );
                entity
                    .type_facts
                    .set_foundation_for_test(object.foundation.clone());
                entity.position.put_location(DriveCoord {
                    x: xyz[0],
                    y: xyz[1],
                    z: xyz[2],
                });
                entity.lifecycle.set_in_limbo_for_test(false);
                if let Some(repairing) = actor["repairing"].as_bool() {
                    crate::sim::arena_fixture::install_status_repair(&mut entity, repairing);
                }
                entity
                    .selection
                    .set_selected_for_test(actor["selected"].as_bool().unwrap());
                entity.discovery.set_owned_by_current_house_for_test(
                    actor["owned_by_current_house"].as_bool().unwrap_or(true),
                );
                if let Some(state) = actor["cloak_state"].as_i64() {
                    assert_eq!(
                        actor["cloak_progress"].as_i64().unwrap_or(0),
                        0,
                        "bounded cloak prior"
                    );
                    let mut cloak = crate::sim::cloak_disguise::CloakRuntime::new(0);
                    cloak.set_state_for_test(state as i32);
                    entity.cloak = Some(cloak);
                }
                if actor["disguised"].as_bool().unwrap_or(false) {
                    let mut disguise = crate::sim::cloak_disguise::DisguiseRuntime::new(0);
                    disguise.acquire(
                        0,
                        None,
                        actor["disguised_as_current_house"]
                            .as_bool()
                            .unwrap_or(false)
                            .then_some(owner),
                    );
                    entity.disguise = Some(disguise);
                }
                let cell = ((xyz[0] / 256) as u16, (xyz[1] / 256) as u16);
                if let Some(bits) = input["cell_bits"].as_u64() {
                    sim.fog
                        .set_ground_bits_for_test(owner, cell.0, cell.1, bits as u8);
                }
                for _ in 0..actor["sensor_count"].as_u64().unwrap_or(0) {
                    sim.fog.increment_sensor_at(owner, cell.0, cell.1);
                }
                for _ in 0..actor["disguise_detect_count"].as_u64().unwrap_or(0) {
                    sim.fog.disguise_detect_add_at(owner, cell, 1);
                }
                entity.veterancy.set_raw_for_test(
                    crate::util::native_x87::NativeF32Bits::from_bits(
                        actor["veterancy_raw_bits"].as_u64().unwrap_or(0) as u32,
                    ),
                );
                if object.passengers > 0 || object.max_number_occupants > 0 {
                    use crate::sim::passenger::{PassengerCargo, PassengerRole};
                    let capacity = if actor_category == EntityCategory::Structure {
                        object.max_number_occupants
                    } else {
                        object.passengers as u32
                    };
                    let mut cargo = PassengerCargo::new(capacity, object.size_limit);
                    if !actor["unload"].as_bool().unwrap_or(false) {
                        if let Some(name) = actor["passenger"].as_str() {
                            let passenger_type = rules.object(name).unwrap();
                            if let Some(native) = actor["occupy_pip_override"].as_u64() {
                                assert_eq!(
                                    passenger_type.occupy_pip(),
                                    native as u32,
                                    "admitted native reader state reaches the garrison consumer",
                                );
                            }
                            let passenger_ref = sim.interner.intern(name);
                            let passenger_id = 1_000 + index as u64;
                            let passenger = GameEntity::new_at_frame_zero_for_test(
                                passenger_id,
                                0,
                                0,
                                0,
                                0,
                                owner,
                                Health::new(passenger_type.strength),
                                passenger_ref,
                                EntityCategory::from(passenger_type.category),
                                0,
                                5,
                                false,
                            );
                            sim.entities_mut().insert(passenger);
                            assert!(cargo.board(passenger_id, passenger_type.size));
                        }
                    }
                    // Explicit drawing-consumer hold prior. Actual command,
                    // Limbo and departure lifecycle has separate production tests.
                    entity.passenger_role = PassengerRole::transport(cargo);
                }
                if let Some(count) = actor["capture_count"].as_u64() {
                    let weapon = object
                        .weapon_at(0)
                        .and_then(|name| rules.weapon(name))
                        .unwrap();
                    crate::sim::arena_fixture::install_status_capture_count(
                        &mut entity,
                        weapon.damage,
                        weapon.infinite_mind_control,
                        count as u32,
                    );
                }
                if actor.get("ore_bales").is_some() || actor.get("gem_bales").is_some() {
                    crate::sim::arena_fixture::install_status_miner_cargo(
                        &mut entity,
                        object,
                        rules,
                        actor["ore_bales"].as_u64().unwrap_or(0) as usize,
                        actor["gem_bales"].as_u64().unwrap_or(0) as usize,
                    );
                }
                if let Some(stage) = actor["ammo_initialization"].as_str() {
                    let (initialized, current, pending) =
                        crate::sim::arena_fixture::install_status_ammo_state(
                            &mut entity,
                            object,
                            stage,
                            actor["current_ammo"].as_i64().map(|value| {
                                i32::try_from(value).expect("native signed Ammo prior")
                            }),
                        );
                    let result = &row["actor_results"][index];
                    assert_eq!(
                        i64::from(entity.techno_pip_fill_level(object, rules)),
                        result["fill_count"].as_i64().unwrap(),
                    );
                    assert_eq!(
                        i64::from(object.pip_max(rules)),
                        result["pip_max"].as_i64().unwrap()
                    );
                    assert_eq!(
                        i64::from(initialized),
                        result["initialized_ammo"].as_i64().unwrap()
                    );
                    assert_eq!(i64::from(current), result["current_ammo"].as_i64().unwrap());
                    assert_eq!(pending, result["pending_release"].as_bool().unwrap());
                }
                if font.is_some() {
                    let native_actor = &row["actor_results"][index];
                    let slot = native_actor["group_slot"].as_i64().unwrap();
                    if (0..10).contains(&slot) {
                        // Native TeamCreate1..10 stores0..9; the existing
                        // input owner indexes displayed digits1..9,0.
                        groups[((slot + 1) % 10) as usize].push(id);
                    }
                    let rgb = crate::render::palette_light::house_color_rgb(
                        &rules.house_color_ramps,
                        house_colors["Owner"],
                    );
                    assert_eq!(serde_json::json!(rgb), native_actor["house_rgb"]);
                }
                sim.entities_mut().insert(entity);
                sim.resubmit_entity_display_for_test(id);
                if actor["sinking"].as_bool().unwrap_or(false) {
                    sim.begin_ship_sinking(id, rules, crate::sim::world::FrameEffects::default());
                }
                let mut strip = techno_extras_for_parent(
                    &sim,
                    sim.entities().get(id).unwrap(),
                    Some(rules),
                    &overlay,
                    actor["hovered"].as_bool().unwrap_or(false).then_some(id),
                    Some("Owner"),
                    StatusViewport {
                        camera: cam.camera_pos,
                        size: size.map(|x| x as f32),
                        ignore_visibility: false,
                    },
                );
                if input["entry_boundary"] == "extras_prefix" {
                    // The original component stops6F534E before rank/health.
                    // Consume only that produced prefix, keeping complete
                    // Extras replay for the separate native body compositions.
                    strip = crate::app::presentation::ui_overlays::TechnoExtras {
                        wrench: strip.wrench,
                        ..Default::default()
                    };
                }
                let wrenches: Vec<_> = row["draws"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|draw| {
                        draw["asset"] == "WRENCH.SHP"
                            && draw["actor"].as_u64() == Some(index as u64)
                            && (!composed
                                || draw["phase"]
                                    .as_str()
                                    .is_some_and(|phase| phase.ends_with("first_extras")))
                    })
                    .collect();
                assert_eq!(
                    usize::from(strip.wrench.is_some()),
                    wrenches.len(),
                    "{} actor{index} wrench admission",
                    row["name"]
                );
                if let (Some(instance), Some(draw)) = (&strip.wrench, wrenches.first()) {
                    assert_eq!(draw["flags"], 0xE00);
                    let expected = overlay
                        .repair_wrench()
                        .unwrap()
                        .instance(
                            draw["frame"].as_u64().unwrap() as usize,
                            (
                                draw["point"][0].as_i64().unwrap() as f32 + cam.camera_pos[0],
                                draw["point"][1].as_i64().unwrap() as f32 + cam.camera_pos[1],
                            ),
                            instance.depth,
                        )
                        .unwrap();
                    assert_eq!(
                        instance.position, expected.position,
                        "{} native wrench render point",
                        row["name"]
                    );
                    assert_eq!(
                        instance.uv_origin, expected.uv_origin,
                        "{} native wrench frame",
                        row["name"]
                    );
                    assert_eq!(instance.z_adjust, draw["z_adjust"].as_i64().unwrap() as f32);
                    assert_eq!(instance.alpha, 1.0);
                }
                let native_pips: Vec<_> = row["draws"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|draw| {
                        draw["asset"] == "PIPS.SHP"
                            && !matches!(draw["frame"].as_i64(), Some(14 | 15 | 19))
                            && draw["actor"].as_u64() == Some(index as u64)
                            && (!composed
                                || draw["phase"]
                                    .as_str()
                                    .is_some_and(|phase| phase.ends_with("first_extras")))
                    })
                    .collect();
                assert_eq!(
                    strip.fills.len() + usize::from(strip.self_heal.is_some()),
                    native_pips.len(),
                    "{} actor{index} pip commands",
                    row["name"]
                );
                for (instance, draw) in strip
                    .fills
                    .iter()
                    .chain(strip.self_heal.iter())
                    .zip(native_pips)
                {
                    assert_eq!(draw["flags"], 0x600);
                    let expected = overlay
                        .pips()
                        .unwrap()
                        .instance(
                            draw["frame"].as_u64().unwrap() as usize,
                            (
                                draw["point"][0].as_i64().unwrap() as f32 + cam.camera_pos[0],
                                draw["point"][1].as_i64().unwrap() as f32 + cam.camera_pos[1],
                            ),
                            instance.depth,
                        )
                        .unwrap();
                    assert_eq!(
                        instance.position, expected.position,
                        "{} native point",
                        row["name"]
                    );
                    assert_eq!(
                        instance.uv_origin, expected.uv_origin,
                        "{} native frame",
                        row["name"]
                    );
                }
                let scales: Vec<_> = row["draws"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|draw| {
                        draw["asset"] == "PIPS2.SHP" && draw["actor"].as_u64() == Some(index as u64)
                    })
                    .collect();
                assert_eq!(
                    strip.scale.len(),
                    scales.len(),
                    "{} PIPS2 commands",
                    row["name"]
                );
                for (instance, draw) in strip.scale.iter().zip(scales) {
                    assert_eq!(draw["flags"], 0x600);
                    let expected = overlay
                        .scale_pips()
                        .unwrap()
                        .instance(
                            draw["frame"].as_u64().unwrap() as usize,
                            (
                                draw["point"][0].as_i64().unwrap() as f32 + cam.camera_pos[0],
                                draw["point"][1].as_i64().unwrap() as f32 + cam.camera_pos[1],
                            ),
                            instance.depth,
                        )
                        .unwrap();
                    assert_eq!(
                        instance.position, expected.position,
                        "{} scale point",
                        row["name"]
                    );
                    assert_eq!(
                        instance.uv_origin, expected.uv_origin,
                        "{} scale frame",
                        row["name"]
                    );
                }
                let ranks: Vec<_> = row["draws"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|draw| {
                        draw["asset"] == "PIPS.SHP"
                            && draw["actor"].as_u64() == Some(index as u64)
                            && matches!(draw["frame"].as_i64(), Some(14 | 15 | 19))
                    })
                    .collect();
                assert_eq!(
                    usize::from(strip.rank.is_some()),
                    ranks.len(),
                    "{} actor{index} rank admission",
                    row["name"]
                );
                if let (Some(instance), Some(draw)) = (&strip.rank, ranks.first()) {
                    assert_eq!(draw["flags"], 0xE00);
                    let expected = overlay
                        .pips()
                        .unwrap()
                        .instance(
                            draw["frame"].as_u64().unwrap() as usize,
                            (
                                draw["point"][0].as_i64().unwrap() as f32 + cam.camera_pos[0],
                                draw["point"][1].as_i64().unwrap() as f32 + cam.camera_pos[1],
                            ),
                            instance.depth,
                        )
                        .unwrap();
                    assert_eq!(
                        instance.position, expected.position,
                        "{} rank native point",
                        row["name"]
                    );
                    assert_eq!(
                        instance.uv_origin, expected.uv_origin,
                        "{} rank native frame",
                        row["name"]
                    );
                    assert_eq!(instance.z_adjust, draw["z_adjust"].as_i64().unwrap() as f32);
                    assert_eq!(
                        instance.alpha, 1.0,
                        "rank does not inherit body cloak opacity"
                    );
                }
                let draw = display.object_draw(id, SpriteEncoding::Plain).unwrap();
                let mut parent = if actor_category == EntityCategory::Structure {
                    PlannedObjectInstance::building(draw, vec![])
                } else {
                    PlannedObjectInstance::object(draw, vec![])
                };
                parent.status = Some(strip);
                if wrench_composed && actor["selected"] == true {
                    let center = crate::sim::movement::ground_pose::object_get_coords(
                        sim.entities().get(id).unwrap(),
                        sim.resolved_terrain.as_ref(),
                    );
                    // Native6F53F5..542B uses87F6C4/index15 on these
                    // non-submerged parents. The existing bracket corpus pins
                    // that independent PALETTE N53 input; MOUSEPAL is N1.
                    let bracket = crate::test_fixture::json(
                        "tools/procedural_drawing_oracle/building_brackets.json",
                    );
                    parent.brackets = Some(
                        crate::app::presentation::selection_brackets::building_bracket_lines(
                            [center.x, center.y, center.z],
                            rules.building_dimensions(object),
                            cam.camera_pos.map(|value| value as i32),
                            bracket["palette"]["native_convert"]["selected_word"]
                                .as_u64()
                                .unwrap() as u16,
                        ),
                    );
                }
                parents.push(parent);
            }
            if let Some(font) = &font {
                let labels = crate::app::presentation::ui_overlays::build_control_group_status_render_instances(
                    &sim, &groups,
                    input["actors"].as_array().unwrap().iter().enumerate()
                        .find(|(_, actor)| actor["hovered"].as_bool().unwrap_or(false))
                        .map(|(index, _)| if index == 0 { 901 } else { 2 }),
                    Some(owner), false, &house_colors, &rules.house_color_ramps,
                    font, cam.camera_pos, size[0] as f32, size[1] as f32,
                    tactical_clip.map(|value| value as i32), Some(rules),
                );
                let mut labels = labels;
                for parent in &mut parents {
                    parent.status.as_mut().unwrap().group =
                        labels.remove(&parent.parent.id).unwrap_or_default();
                }
            }
            let a_input =
                vec![input["a_value"].as_u64().unwrap() as u8; (size[0] * size[1]) as usize];
            let a = ShroudBuffer::fixture(&gpu.device, &gpu.queue, size, cam.camera_pos, &a_input);
            batch.bind_shroud(&gpu.device, Some(&a));
            batch.write_camera(&gpu.queue, cam);
            // Builders can discover these in ID/atlas order. The exact same
            // lowering used by the app restores retained Display order.
            parents.reverse();
            let mut layer = lower_ground_object_instances(parents);
            pool.upload_on_device(
                &gpu.device,
                &gpu.queue,
                "object_extras",
                &layer.extras_instances,
            );
            let color = gpu.target(size, format);
            let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
            let cv = color.create_view(&Default::default());
            let dv = depth.create_view(&Default::default());
            let background = input["background"].as_u64().unwrap_or(0) as u16;
            let mut initial = encoded(background, format).repeat((size[0] * size[1]) as usize);
            if composed {
                install_pixels(&mut initial, &row["ground_pixels"], size, format);
            }
            write_color(&gpu, &color, size, &initial);
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            let old_z = input["old_z"].as_u64().unwrap_or(u64::from(u16::MAX)) as u16;
            let mut z = vec![old_z; (size[0] * size[1]) as usize];
            if composed {
                install_z(&mut z, &row["ground_z"], size);
                crate::render::terrain_draw_gpu_tests::seed_depth_grid(
                    &gpu,
                    &mut encoder,
                    &dv,
                    &z,
                    size[0],
                );
            } else {
                clear(&mut encoder, &cv, &dv, old_z, wgpu::LoadOp::Load);
            }
            let mut terrain = TerrainDrawRenderer::new(&gpu.device, &gpu.queue, format, &batch);
            terrain.prepare(&gpu.device, &color, &dv, batch.camera_uniform());
            if wrench_composed {
                terrain.begin_surface_lines();
                let viewport = crate::render::surface_line::SurfaceLineViewport {
                    camera: cam.camera_pos.map(|value| value as i32),
                    clip: tactical_clip.map(|value| value as i32),
                    z_origin_y: 0,
                    zoom: 1.0,
                };
                layer.line_batches = layer
                    .before_lines
                    .iter()
                    .map(|lines| {
                        terrain.prepare_surface_lines(
                            &gpu.device,
                            &gpu.queue,
                            std::iter::empty(),
                            lines,
                            &[],
                            viewport,
                            || false,
                            |_| input["a_value"].as_u64().unwrap() as u16,
                        )
                    })
                    .collect();
                for run in &mut layer.extras_runs {
                    if let super::draw_plan_lowering::ObjectExtrasRun::Lines {
                        lines, batch, ..
                    } = run
                    {
                        *batch = terrain.prepare_surface_lines(
                            &gpu.device,
                            &gpu.queue,
                            std::iter::empty(),
                            &layer.extras_lines[lines.clone()],
                            &[],
                            viewport,
                            || false,
                            |_| input["a_value"].as_u64().unwrap() as u16,
                        );
                    }
                }
            }
            if row_has_building {
                // This native snapshot is immediately before actor0's body.
                // A later selected neighbor has not reached its initial visit
                // yet. Retain the actual production ranges through that
                // parent, then restore the full layer for the late sweep.
                let deferred_initial = if wrench_composed {
                    let end = layer
                        .runs
                        .iter()
                        .rposition(|run| {
                            if let super::draw_plan_lowering::ObjectTexture::Status(index) =
                                run.target
                            {
                                matches!(
                                    layer.extras_runs[index],
                                    super::draw_plan_lowering::ObjectExtrasRun::Status {
                                        parent: 901,
                                        ..
                                    }
                                )
                            } else {
                                false
                            }
                        })
                        .expect("first parent retains its initial status")
                        + 1;
                    layer.runs.split_off(end)
                } else {
                    Vec::new()
                };
                // Replay the actual initial Building Extras marker with the
                // same instance range used by the app's later Extras sweep.
                super::merge_passes::draw_native_object_pass(
                    &mut encoder,
                    &cv,
                    &dv,
                    &mut terrain,
                    tactical_clip,
                    &batch,
                    None,
                    &layer,
                    None,
                    None,
                    None,
                    None,
                    None,
                    batch.default_zshape_bind_group(),
                    overlay_resource(&overlay, &pool).map(|(overlay, buffer)| {
                        (overlay, buffer, font.as_ref().map(|font| font.atlas()))
                    }),
                );
                let reads = [
                    gpu.read(&mut encoder, &color),
                    gpu.read(&mut encoder, &depth),
                ];
                let output = gpu.finish(encoder, &reads, size);
                layer.runs.extend(deferred_initial);
                assert_pixels(
                    &output[0],
                    &initial,
                    if composed {
                        &row["before_body_pixels"]
                    } else {
                        &row["pixels"]
                    },
                    size,
                    format,
                    composed,
                    row,
                );
                assert_z(&output[1], &z, row);
                encoder = gpu.device.create_command_encoder(&Default::default());
                if composed {
                    // Original body admission, shape and BUILDNGZ execution
                    // supplied these sampled color/Z values. This seam checks
                    // the health consumer across a real body interval; it
                    // makes no GPU-body or whole-frame parity claim.
                    install_pixels(&mut initial, &row["before_late_pixels"], size, format);
                    write_color(&gpu, &color, size, &initial);
                    install_z(&mut z, &row["before_late_z"], size);
                    crate::render::terrain_draw_gpu_tests::seed_depth_grid(
                        &gpu,
                        &mut encoder,
                        &dv,
                        &z,
                        size[0],
                    );
                    assert!(row["body_admissions"].as_array().unwrap().len() > 0);
                }
            }
            super::object_extras::draw_object_extras(
                &mut encoder,
                &cv,
                &dv,
                &mut terrain,
                tactical_clip,
                &batch,
                Some(&overlay),
                pool.get("object_extras").map(|(buffer, _)| buffer),
                font.as_ref().map(|font| font.atlas()),
                &layer,
            );
            if category == EntityCategory::Unit {
                if row["name"] == "healthy_a127" {
                    use super::draw_plan_lowering::{ObjectExtrasRun, ObjectLayerPass};
                    let mut fenced = ObjectLayerPass::default();
                    let ObjectExtrasRun::Status {
                        rank,
                        background,
                        fills,
                        scale,
                        ..
                    } = &layer.extras_runs[0]
                    else {
                        panic!("native healthy strip must retain its parent run");
                    };
                    // A clipped building's empty line batch still separates
                    // adjacent health groups. Keep this below Metal's pool limit
                    // so the missing-accounting regression fails without hanging.
                    // These opaque native stores are idempotent; final color/A/Z
                    // must still match the same original healthy control.
                    for parent in 0..129 {
                        fenced.extras_runs.push(ObjectExtrasRun::Status {
                            parent,
                            wrench: None,
                            rank: *rank,
                            background: *background,
                            fills: fills.clone(),
                            scale: scale.clone(),
                            group_rectangles: 0..0,
                            group_text: 0..0,
                            self_heal: None,
                        });
                        fenced.extras_runs.push(ObjectExtrasRun::Lines {
                            parent: parent + 1000,
                            lines: 0..0,
                            batch: Default::default(),
                        });
                    }
                    super::object_extras::draw_object_extras(
                        &mut encoder,
                        &cv,
                        &dv,
                        &mut terrain,
                        tactical_clip,
                        &batch,
                        Some(&overlay),
                        pool.get("object_extras").map(|(buffer, _)| buffer),
                        font.as_ref().map(|font| font.atlas()),
                        &fenced,
                    );
                    assert!(
                        terrain.submissions() > 0,
                        "closed health passes must reach the shared submission owner before encoder finish"
                    );
                }
            }
            let reads = [
                gpu.read(&mut encoder, &color),
                gpu.read(&mut encoder, &depth),
            ];
            let output = gpu.finish(encoder, &reads, size);
            assert_pixels(
                &output[0],
                &initial,
                &row["pixels"],
                size,
                format,
                composed,
                row,
            );
            assert_z(&output[1], &z, row);
            let a_actual = gpu.read_float_texels(a.gpu_source().0, [0, 0], size);
            for (actual, expected) in a_actual.iter().zip(&a_input) {
                assert_eq!(
                    (actual[0] * 255.0).round() as u8,
                    *expected,
                    "{} A unchanged",
                    row["name"]
                );
            }
        }
    }
}

fn overlay_resource<'a>(
    overlay: &'a SelectionOverlay,
    pool: &'a InstanceBufferPool,
) -> Option<(&'a SelectionOverlay, &'a wgpu::Buffer)> {
    pool.get("object_extras")
        .map(|(buffer, _)| (overlay, buffer))
}

fn write_color(gpu: &Gpu, color: &wgpu::Texture, size: [u32; 2], bytes: &[u8]) {
    gpu.queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: color,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(size[0] * 4),
            rows_per_image: Some(size[1]),
        },
        color.size(),
    );
}

fn install_pixels(bytes: &mut [u8], pixels: &Value, size: [u32; 2], format: wgpu::TextureFormat) {
    for pixel in pixels.as_array().unwrap() {
        let offset = (pixel[1].as_u64().unwrap() as usize * size[0] as usize
            + pixel[0].as_u64().unwrap() as usize)
            * 4;
        bytes[offset..offset + 4]
            .copy_from_slice(&encoded(pixel[2].as_u64().unwrap() as u16, format));
    }
}

fn install_z(z: &mut [u16], pixels: &Value, size: [u32; 2]) {
    for pixel in pixels.as_array().unwrap() {
        z[pixel[1].as_u64().unwrap() as usize * size[0] as usize
            + pixel[0].as_u64().unwrap() as usize] = pixel[2].as_u64().unwrap() as u16;
    }
}

fn assert_pixels(
    actual: &[u8],
    initial: &[u8],
    pixels: &Value,
    size: [u32; 2],
    format: wgpu::TextureFormat,
    sampled: bool,
    row: &Value,
) {
    let mut expected = initial.to_vec();
    install_pixels(&mut expected, pixels, size, format);
    if sampled {
        for pixel in pixels.as_array().unwrap() {
            let (x, y) = (
                pixel[0].as_u64().unwrap() as usize,
                pixel[1].as_u64().unwrap() as usize,
            );
            let offset = (y * size[0] as usize + x) * 4;
            assert_eq!(
                &actual[offset..offset + 4],
                &expected[offset..offset + 4],
                "{} {format:?} sample({x},{y})",
                row["name"]
            );
        }
    } else {
        for (index, pixel) in actual.chunks_exact(4).enumerate() {
            assert_eq!(
                pixel,
                &expected[index * 4..index * 4 + 4],
                "{} {format:?} color({},{})",
                row["name"],
                index % size[0] as usize,
                index / size[0] as usize
            );
        }
    }
}

fn assert_z(actual: &[u8], expected: &[u16], row: &Value) {
    for (index, pixel) in actual.chunks_exact(4).enumerate() {
        assert_eq!(
            crate::render::native_z::stored_z(f32::from_le_bytes(pixel.try_into().unwrap())),
            expected[index],
            "{} Z{index}",
            row["name"]
        );
    }
}

fn pair(value: &Value) -> [u32; 2] {
    std::array::from_fn(|i| value[i].as_u64().unwrap() as u32)
}

#[test]
fn retail_gapowr_repair_fields_match_original_layered_readers() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let native = crate::test_fixture::json("tools/procedural_drawing_oracle/repair_wrench.json");
    let result = &native["readers"]["result"];
    let object = retail.rules.object("GAPOWR").unwrap();
    assert_eq!(object.repairable, result["repairable"].as_bool().unwrap());
    assert_eq!(
        object.click_repairable,
        result["click_repairable"].as_bool().unwrap()
    );
    assert_eq!(
        i64::from(object.strength),
        result["strength"].as_i64().unwrap()
    );
    assert_eq!(i64::from(object.cost), result["cost"].as_i64().unwrap());
    let default = crate::rules::object_type::ObjectType::from_ini_section(
        "REPAIR_DEFAULT",
        &crate::rules::ini_parser::IniFile::from_str("[REPAIR_DEFAULT]\nFixture=1\n")
            .section("REPAIR_DEFAULT")
            .unwrap(),
        crate::rules::object_type::ObjectCategory::Building,
    );
    assert_eq!(
        default.repairable,
        native["readers"]["constructor"]["repairable"]
            .as_bool()
            .unwrap()
    );
    assert_eq!(
        default.click_repairable,
        native["readers"]["constructor"]["click_repairable"]
            .as_bool()
            .unwrap()
    );
}

#[test]
fn retail_mtnk_health_fields_match_original_layered_readers() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let native = crate::test_fixture::json("tools/procedural_drawing_oracle/unit_health.json");
    let result = &native["readers"]["result"];
    let object = retail.rules.object("MTNK").unwrap();
    assert_eq!(object.strength, result["strength"].as_i64().unwrap() as i32);
    assert_eq!(
        object.pixel_selection_bracket_delta,
        result["delta"].as_i64().unwrap() as i32
    );
    assert_eq!(object.pip_scale, crate::rules::object_type::PipScale::None);
    assert_eq!(
        retail.rules.general.condition_red,
        result["condition_red"].as_f64().unwrap()
    );
    assert_eq!(
        retail.rules.general.condition_yellow,
        result["condition_yellow"].as_f64().unwrap()
    );
}

#[test]
fn retail_gapowr_health_fields_match_original_layered_readers() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let native = crate::test_fixture::json("tools/procedural_drawing_oracle/building_health.json");
    let result = &native["readers"]["result"];
    let object = retail.rules.object("GAPOWR").unwrap();
    assert_eq!(object.strength, result["strength"].as_i64().unwrap() as i32);
    assert_eq!(
        object.pixel_selection_bracket_delta,
        result["delta"].as_i64().unwrap() as i32
    );
    assert_eq!(object.pip_scale, crate::rules::object_type::PipScale::None);
    assert_eq!(
        retail.rules.general.condition_red,
        result["condition_red"].as_f64().unwrap()
    );
    assert_eq!(
        retail.rules.general.condition_yellow,
        result["condition_yellow"].as_f64().unwrap()
    );
    let dimensions = retail.rules.building_dimensions(object);
    let expected: [i32; 3] =
        std::array::from_fn(|i| native["readers"]["dimensions"][i].as_i64().unwrap() as i32);
    assert_eq!(dimensions, expected);
}
