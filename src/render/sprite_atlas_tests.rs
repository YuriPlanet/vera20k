//! Tests for SHP sprite atlas key collection and deduplication.

use super::*;
use crate::assets::asset_manager::MediaArchiveMode;
use crate::util::save_codec;

struct StoredFrameTestDirectory(std::path::PathBuf);

impl StoredFrameTestDirectory {
    fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "vera20k-shp-stored-frame-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir(&path).expect("create SHP loader fixture directory");
        Self(path)
    }

    fn write_raw_and_rle_frames(&self, name: &str, canvas: [u16; 2], frame: [u16; 4]) {
        let [x, y, width, height] = frame;
        let mut pixels = vec![0; usize::from(width) * usize::from(height)];
        // Keep transparent margins inside the stored frame: its dimensions
        // must come from the SHP header, not a new alpha-tight bounding box.
        pixels[usize::from(width) + 2] = 1;
        pixels[usize::from(height - 2) * usize::from(width) + usize::from(width - 3)] = 2;
        let mut rle = Vec::new();
        for row in pixels.chunks_exact(usize::from(width)) {
            let mut encoded = Vec::new();
            for &index in row {
                if index == 0 {
                    encoded.extend_from_slice(&[0, 1]);
                } else {
                    encoded.push(index);
                }
            }
            rle.extend_from_slice(&((encoded.len() + 2) as u16).to_le_bytes());
            rle.extend_from_slice(&encoded);
        }

        let mut bytes = Vec::new();
        for value in [0, canvas[0], canvas[1], 2] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        let mut offset = 8 + 2 * 24;
        for (format, payload) in [(1u8, &pixels), (3u8, &rle)] {
            let mut header = [0; 24];
            for (slot, value) in [x, y, width, height].into_iter().enumerate() {
                header[slot * 2..slot * 2 + 2].copy_from_slice(&value.to_le_bytes());
            }
            header[8] = format;
            header[20..24].copy_from_slice(&(offset as u32).to_le_bytes());
            bytes.extend_from_slice(&header);
            offset += payload.len();
        }
        bytes.extend_from_slice(&pixels);
        bytes.extend_from_slice(&rle);
        std::fs::write(self.0.join(name), bytes).expect("write raw/RLE SHP loader fixture");
    }
}

impl Drop for StoredFrameTestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn loader_uses_stored_shp_rect_for_depth_but_retains_logical_canvas_for_picking() {
    // Retail-shaped headers: GI canvas 78x66, stored frame (32,7,13,29);
    // building canvas 284x226, stored frame (37,74,212,148). Payloads are
    // synthetic markers, so this is loader regression coverage, not a retail
    // visual parity claim. CC_Draw_Shape 0x4AED70 centers the logical canvas
    // and then adds the stored frame origin before handing it to the blitter.
    let directory = StoredFrameTestDirectory::new();
    directory.write_raw_and_rle_frames("DEPTHGI.SHP", [78, 66], [32, 7, 13, 29]);
    directory.write_raw_and_rle_frames("DEPTHBUILDING.SHP", [284, 226], [37, 74, 212, 148]);
    let assets = AssetManager::from_loose_root_for_test(&directory.0);
    let mut palette_bytes = [0; 768];
    palette_bytes[3..6].copy_from_slice(&[4, 8, 12]);
    palette_bytes[6..9].copy_from_slice(&[14, 18, 22]);
    let palette = Palette::from_bytes(&palette_bytes).expect("fixture palette");

    for (name, size, offset, canvas, first_marker, last_marker) in [
        (
            "DEPTHGI",
            [13, 29],
            [-7.0, -26.0],
            [-39.0, -33.0, 78.0, 66.0],
            [-5.0, -25.0],
            [3.0, 1.0],
        ),
        (
            "DEPTHBUILDING",
            [212, 148],
            [-105.0, -39.0],
            [-142.0, -113.0, 284.0, 226.0],
            [-103.0, -38.0],
            [104.0, 107.0],
        ),
    ] {
        let mut raw_rgba = None;
        for frame in 0..2 {
            let key = ShpSpriteKey {
                palette_context: crate::render::sprite_atlas::ShpPaletteContext::Legacy,
                type_id: name.to_string(),
                facing: 0,
                frame,
                house_color: crate::rules::house_colors::NO_REMAP,
            };
            let (rendered, rendered_rgba) = render_shp_sprite(
                &assets,
                &palette,
                true,
                &key,
                "tem",
                "TEMPERATE",
                None,
                None,
            )
            .expect("actual loose-asset SHP loader must accept fixture");
            assert_eq!(
                [rendered.width, rendered.height],
                size,
                "{name} frame {frame}"
            );
            assert_eq!([rendered.offset_x, rendered.offset_y], offset);
            assert_eq!(
                rendered.canvas_rect, canvas,
                "preserve logical picking/sort rect"
            );
            assert_eq!(rendered.extended, frame == 1, "dispatch from format bit 1");
            assert_eq!(rendered_rgba.len(), (size[0] * size[1] * 4) as usize);
            let colored: Vec<_> = rendered_rgba
                .as_chunks::<4>()
                .0
                .iter()
                .enumerate()
                .filter(|(_, rgba)| rgba[3] != 0)
                .map(|(index, rgba)| {
                    (
                        [
                            rendered.offset_x + (index as u32 % rendered.width) as f32,
                            rendered.offset_y + (index as u32 / rendered.width) as f32,
                        ],
                        rgba.to_vec(),
                    )
                })
                .collect();
            assert_eq!(
                colored,
                vec![
                    (first_marker, vec![16, 32, 48, 255]),
                    (last_marker, vec![56, 72, 88, 255]),
                ],
                "stored-frame upload must preserve former canvas-relative color placement"
            );
            if let Some(raw) = &raw_rgba {
                assert_eq!(
                    &rendered_rgba, raw,
                    "raw format 1 and RLE format 3 must agree"
                );
            } else {
                raw_rgba = Some(rendered_rgba);
            }
        }
    }
}

fn make_shp_key(type_id: &str, facing: u8) -> ShpSpriteKey {
    ShpSpriteKey {
        palette_context: crate::render::sprite_atlas::ShpPaletteContext::Legacy,
        type_id: type_id.to_string(),
        facing,
        frame: 0,
        house_color: HouseColorIndex::default(),
    }
}

fn test_entry(page: u8) -> ShpSpriteEntry {
    ShpSpriteEntry {
        uv_origin: [0.0, 0.0],
        uv_size: [1.0, 1.0],
        pixel_size: [1.0, 1.0],
        offset_x: -1.0,
        offset_y: -2.0,
        canvas_rect: [-1.0, -2.0, 1.0, 1.0],
        extended: false,
        source_palette: 1,
        page,
    }
}

#[test]
fn refresh_failure_returns_the_prior_atlas_unchanged() {
    let prior_key = make_shp_key("PRIOR", 0);
    let mut prior = SpriteAtlas::new(
        Vec::new(),
        HashMap::from([(prior_key.clone(), test_entry(0))]),
        ShpPalettes::default(),
    );
    prior.make_frame_counts.insert("PRIOR".to_string(), 3);
    prior
        .active_anim_frame_counts
        .insert("PRIOR".to_string(), 4);
    prior.unrenderable.insert(make_shp_key("EMPTY", 0));
    prior
        .covered_objects
        .entry("PRIOR".to_string())
        .or_default()
        .insert(HouseColorIndex(1));

    let restored =
        abort_sprite_atlas_refresh(Some(prior), "required refresh sprite failed".to_string())
            .expect("a refresh failure must retain the prior atlas");

    assert_eq!(restored.sprite_count(), 1);
    assert!(restored.get(&prior_key).is_some());
    assert_eq!(restored.make_frame_counts["PRIOR"], 3);
    assert_eq!(restored.active_anim_frame_counts["PRIOR"], 4);
    assert!(restored.unrenderable.contains(&make_shp_key("EMPTY", 0)));
    assert!(restored.covers_object("PRIOR", HouseColorIndex(1)));
}

#[test]
#[should_panic(expected = "required initial sprite failed")]
fn required_initial_build_failure_remains_fail_fast() {
    let _ = abort_sprite_atlas_refresh(None, "required initial sprite failed".to_string());
}

/// One placed object of `type_id` owned by "Americans", and the interner
/// that resolves it.
fn one_object_world(
    type_id: &str,
    category: EntityCategory,
) -> (
    crate::sim::entity_store::EntityStore,
    crate::sim::intern::StringInterner,
) {
    let mut entity =
        crate::sim::game_entity::GameEntity::test_default(1, type_id, "Americans", 5, 5);
    entity.set_category_for_test(category);
    entity.set_is_voxel_for_test(false);
    let mut store = crate::sim::entity_store::EntityStore::new();
    store.insert(entity);
    (store, crate::sim::intern::test_interner())
}

#[test]
fn retail_shp_vehicle_keys_include_the_native_embedded_shadow_half() {
    let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("C3Y03MD.MAP")
    else {
        return;
    };
    let rules = &battle.rules;
    let (_, assets) = crate::rules::retail_ini_fixture::retail_assets()
        .expect("Battle rules came from the explicit retail install");
    for (type_id, count, no_shadow) in [
        ("DRON", 176, false),
        ("SQD", 296, true),
        ("DLPH", 232, false),
    ] {
        let (world, interner) = one_object_world(type_id, EntityCategory::Unit);
        let colors = [HouseColorIndex(2), HouseColorIndex(5)];
        let pairs = colors
            .map(|color| (type_id.to_owned(), color))
            .into_iter()
            .collect();
        let mut needed = HashSet::new();
        let sources = insert_new_object_keys(
            &mut needed,
            &pairs,
            &world,
            Some(&interner),
            &assets,
            "tem",
            "TEMPERATE",
            Some(&rules),
            Some(rules.art()),
        );
        let source = sources
            .get(type_id)
            .and_then(Option::as_ref)
            .expect("Unit keys retain the actual physical SHP for shadow frame selection");
        assert_eq!(source.shp.frames.len(), count);
        assert_eq!(
            rules.object(type_id).unwrap().no_shadow,
            no_shadow,
            "{type_id}"
        );
        // Techno7064C2..7064CD adds the signed SHP header count / 2
        // to the caller's body frame. Frame zero is an admitted stand frame
        // for each of these retail Unit images, independently of its colour.
        for color in colors {
            for frame in [0, (count / 2) as u16] {
                assert!(
                    needed.contains(&ShpSpriteKey {
                        palette_context: ShpPaletteContext::Legacy,
                        type_id: type_id.to_owned(),
                        facing: 0,
                        frame,
                        house_color: color,
                    }),
                    "{type_id} colour {color:?} is missing native frame {frame}"
                );
            }
        }
        if type_id == "DRON" {
            let native =
                crate::test_fixture::json("tools/spatial_oracle/unit_shp_fire_frames.json");
            let firing_frames = native["first_by_facing"].as_array().unwrap().iter().chain(
                native["countdown"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .flat_map(|row| row["frames"].as_array().unwrap()),
            );
            for frame in firing_frames {
                let frame = frame.as_u64().unwrap() as u16;
                for house_color in colors {
                    for selected in [frame, frame + 88] {
                        assert!(
                            needed.contains(&ShpSpriteKey {
                                palette_context: ShpPaletteContext::Legacy,
                                type_id: type_id.to_owned(),
                                facing: 0,
                                frame: selected,
                                house_color,
                            }),
                            "native DRON firing pose {frame} lacks body/shadow frame {selected}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn deploy_targets_get_the_same_keys_as_placed_structures() {
    let directory = StoredFrameTestDirectory::new();
    directory.write_raw_and_rle_frames("BOUNDGI.SHP", [8, 8], [0, 0, 6, 4]);
    let assets = AssetManager::from_loose_root_for_test(&directory.0);
    let art_ini = crate::rules::ini_parser::IniFile::from_str(
        "[BOUNDGI]\nSequence=AtlasControl\n[AtlasControl]\nReady=0,1,0\nWalk=1,1,0\n",
    );
    let art = ArtRegistry::from_ini(&art_ini);
    let mut rules = crate::rules::ruleset::RuleSet::from_ini_with_fixed_art_for_test(
        &crate::rules::ini_parser::IniFile::from_str(
            "[BuildingTypes]\n0=CAHOSP\n1=GACNST\n[InfantryTypes]\n0=E1\n\n\
             [CAHOSP]\nCanBeOccupied=yes\n\n[GACNST]\nStrength=1000\n\n\
             [E1]\nImage=BOUNDGI\n",
        ),
        &art_ini,
    )
    .expect("rules");
    // Fixed ART supplies native processed reader inputs; the runtime metadata
    // registry is installed separately, as in production map loading.
    rules.install_art_data(art.clone());
    rules.bind_animation_sequences(
        &crate::rules::infantry_sequence::parse_infantry_sequence_registry(&art_ini),
    );
    assert_eq!(
        rules
            .animation_sequence("E1")
            .unwrap()
            .infantry_action(3)
            .unwrap()
            .start_frame,
        1,
        "fixture Walk record is bound before key preparation"
    );
    // Only an infantryman is placed; the two buildings arrive as deploy
    // targets, which have no placed object to take a category from.
    let (world, interner) = one_object_world("E1", EntityCategory::Infantry);
    let color = HouseColorIndex(2);
    let pairs: HashSet<(String, HouseColorIndex)> = ["E1", "CAHOSP", "GACNST"]
        .into_iter()
        .map(|type_id| (type_id.to_string(), color))
        .collect();
    let mut needed = HashSet::new();
    insert_new_object_keys(
        &mut needed,
        &pairs,
        &world,
        Some(&interner),
        &assets,
        "tem",
        "TEMPERATE",
        Some(&rules),
        Some(&art),
    );

    let frames = |type_id: &str| -> HashSet<u16> {
        needed
            .iter()
            .filter(|key| key.type_id == type_id && key.house_color == color)
            .map(|key| key.frame)
            .collect()
    };
    assert_eq!(
        frames("CAHOSP"),
        HashSet::from([0, 1, 2, 3]),
        "occupancy frame swap"
    );
    assert_eq!(
        frames("GACNST"),
        HashSet::from([0, 1]),
        "native healthy and damaged completed structure bodies"
    );
    assert_eq!(
        frames("E1"),
        HashSet::from([0, 1]),
        "the Infantry records are bounded by the selected SHP"
    );
}

#[test]
fn ordinary_building_atlas_retains_native_completed_health_frames() {
    // The production draw path uses GetCurrentFrame43EF90 for completed
    // buildings. Retain every ordinary idle frame reached by original native
    // execution, including the damaged body needed before fatal damage.
    let fixture: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/building_body_transition.json",
    ))
    .unwrap();
    let mut needed = HashSet::new();
    let color = HouseColorIndex(2);
    let _ = insert_object_keys(
        &mut needed,
        "GAPOWR",
        EntityCategory::Structure,
        color,
        None,
        None,
    );
    let mut compared = 0;
    for row in fixture["rows"].as_array().unwrap() {
        let input = &row["input"];
        if input["state"] != 1
            || input["base_frame"] != 0
            || ["gate", "laser_fence", "firestorm_wall", "can_be_occupied"]
                .into_iter()
                .any(|flag| input[flag] != false)
        {
            continue;
        }
        for field in ["before_frame", "after_frame"] {
            let frame = u16::try_from(row["output"][field].as_u64().unwrap()).unwrap();
            assert!(
                needed.contains(&ShpSpriteKey {
                    palette_context: ShpPaletteContext::Legacy,
                    type_id: "GAPOWR".to_string(),
                    facing: 0,
                    frame,
                    house_color: color,
                }),
                "native completed body frame {frame} is missing: {row}"
            );
            compared += 1;
        }
    }
    assert!(
        compared > 0,
        "ordinary native frame controls must be compared"
    );
}

#[test]
#[ignore = "requires the retail archives and a headless wgpu adapter"]
fn retail_gapowr_atlas_registers_native_embedded_shadow_frames() {
    let root = std::path::PathBuf::from(
        std::env::var("RA2_DIR").expect("set RA2_DIR to the retail game directory"),
    );
    let scenario = crate::headless_scenario::load(&root, "Dustbowl.mmx", 0x0B21_D6E5)
        .expect("retail TEMPERATE scenario loads");
    let mut assets =
        AssetManager::new(&root, MediaArchiveMode::STOCK_DIGITAL).expect("retail archives load");
    let theater_name = scenario.map.header.theater.clone();
    let theater = crate::map::theater::load_theater(&mut assets, &theater_name)
        .expect("retail theater loads");
    let rules = &scenario.runtime.resources.rules;
    let art = rules.art();
    let source = load_shp_source(
        &assets,
        "GAPOWR",
        None,
        theater.extension,
        &theater_name,
        Some(rules),
        Some(art),
    )
    .expect("retail GAPOWR SHP resolves through its rules and theater image");
    assert_eq!(source.found_name, "GGPOWR.SHP");
    assert_eq!(source.shp.frames.len(), 6);

    let gpu = crate::render::terrain_draw_gpu_tests::Gpu::new();
    let batch = BatchRenderer::new_with_device(
        &gpu.device,
        &gpu.queue,
        wgpu::TextureFormat::Bgra8UnormSrgb,
    );
    let house_colors = HouseColorMap::from([("Americans".to_string(), HouseColorIndex(2))]);
    let remaps = HashSet::new();
    let cell_drawers = HashSet::new();
    let extra_building_types = ["GAPOWR"];
    let sprites = build_sprite_atlas(
        &gpu.device,
        &gpu.queue,
        &batch,
        scenario.runtime.simulation.entities(),
        &assets,
        &theater.unit_palette,
        theater.extension,
        &theater_name,
        Some(rules),
        Some(art),
        &house_colors,
        &extra_building_types,
        &remaps,
        &cell_drawers,
        Some(&theater.iso_palette),
        None,
        Some(scenario.runtime.simulation.interner()),
    )
    .expect("the production object atlas builds from retail rules, ART, theater and SHPs");
    assert_eq!(sprites.object_shp_frame_count("GAPOWR"), Some(6));

    let key = |frame| ShpSpriteKey {
        palette_context: ShpPaletteContext::Legacy,
        type_id: "GAPOWR".to_string(),
        facing: 0,
        frame,
        house_color: HouseColorIndex(2),
    };
    assert!(
        sprites.get(&key(0)).is_some(),
        "retail body frame 0 is resident"
    );
    assert!(
        sprites.get(&key(1)).is_some(),
        "retail body frame 1 is resident"
    );
    for (body_frame, shadow_frame) in [(0, 3), (1, 4)] {
        assert!(
            sprites.get(&key(shadow_frame)).is_some(),
            "native body frame {body_frame} requires retail GGPOWR shadow frame {shadow_frame}"
        );
    }
}

fn building_body_atlas_rules(art_text: &str) -> RuleSet {
    use crate::rules::ini_parser::IniFile;
    let art_ini = IniFile::from_str(art_text);
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
        &IniFile::from_str(
            "[BuildingTypes]\n0=YAYARD\n1=BODYALIAS\n2=XAFOO\n\
             [YAYARD]\nStrength=1500\n[BODYALIAS]\nImage=BODYART\n\
             [XAFOO]\nStrength=100\n[VehicleTypes]\n0=YAMOBILE\n\
             [YAMOBILE]\nImage=YAYARD\nStrength=100\n",
        ),
        &art_ini,
    )
    .expect("building body rules");
    rules.install_art_data(ArtRegistry::from_ini(&art_ini));
    rules
}

/// BuildingType45F230's final image pass, after the base ObjectType load.
/// Stock YAYARD omits NewTheater: the original requests YTYARD.SHP, then
/// YGYARD.SHP. A plain-name file or a different extension is not a third try.
#[test]
fn building_body_without_new_theater_uses_theater_then_generic() {
    let rules = building_body_atlas_rules("[YAYARD]\nImage=YAYARD\nFoundation=4x4\n");
    assert!(!rules.art().get("YAYARD").unwrap().new_theater);
    for (files, expected) in [
        (
            vec!["YTYARD.SHP", "YGYARD.SHP", "YAYARD.SHP"],
            Some("YTYARD.SHP"),
        ),
        (vec!["YGYARD.SHP", "YAYARD.SHP"], Some("YGYARD.SHP")),
        (vec!["YAYARD.SHP", "YGYARD.TEM"], None),
    ] {
        let directory = StoredFrameTestDirectory::new();
        for name in files {
            directory.write_raw_and_rle_frames(name, [304, 236], [90, 98, 98, 108]);
        }
        let assets = AssetManager::from_loose_root_for_test(&directory.0);
        let source = load_shp_source(
            &assets,
            "YAYARD",
            None,
            "tem",
            "TEMPERATE",
            Some(&rules),
            Some(rules.art()),
        );
        assert_eq!(source.as_ref().map(|s| s.found_name.as_str()), expected);
    }
}

#[test]
fn building_body_lookup_uses_rules_image_theater_before_redirect() {
    let rules = building_body_atlas_rules(
        "[BODYART]\nImage=YABODY\nTheater=no\nNewTheater=no\n\
         [YABODY]\nTheater=yes\n",
    );
    for (files, expected) in [
        (
            vec!["YTBODY.SHP", "YGBODY.SHP", "YTBODY.TEM", "YABODY.SHP"],
            Some("YTBODY.SHP"),
        ),
        (vec!["YGBODY.SHP", "YTBODY.TEM"], Some("YGBODY.SHP")),
        (vec!["YTBODY.TEM", "YGBODY.TEM", "YABODY.SHP"], None),
    ] {
        let directory = StoredFrameTestDirectory::new();
        for name in files {
            directory.write_raw_and_rle_frames(name, [20, 10], [2, 1, 6, 4]);
        }
        let assets = AssetManager::from_loose_root_for_test(&directory.0);
        let source = load_shp_source(
            &assets,
            "BODYALIAS",
            None,
            "tem",
            "TEMPERATE",
            Some(&rules),
            Some(rules.art()),
        );
        assert_eq!(source.as_ref().map(|s| s.found_name.as_str()), expected);
    }
}

#[test]
fn building_body_lookup_respects_name_pattern_category_and_bound_file() {
    let rules = building_body_atlas_rules("[YAYARD]\n[XAFOO]\nNewTheater=yes\n");
    let directory = StoredFrameTestDirectory::new();
    for name in ["XAFOO.SHP", "XTFOO.SHP", "XGFOO.SHP", "YGYARD.SHP"] {
        directory.write_raw_and_rle_frames(name, [20, 10], [2, 1, 6, 4]);
    }
    let assets = AssetManager::from_loose_root_for_test(&directory.0);
    let load = |name, file| {
        load_shp_source(
            &assets,
            name,
            file,
            "tem",
            "TEMPERATE",
            Some(&rules),
            Some(rules.art()),
        )
    };
    assert_eq!(load("XAFOO", None).unwrap().found_name, "XAFOO.SHP");
    assert!(
        load("YAMOBILE", None).is_none(),
        "only a Building gets the final pass"
    );
    assert_eq!(
        load("YAYARD", Some("XAFOO.SHP")).unwrap().found_name,
        "XAFOO.SHP",
        "an already bound animation or buildup file keeps its source"
    );
}

#[test]
fn retail_yayard_body_resolves_without_new_theater() {
    let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let rules = &battle.rules;
    let (_, assets) = crate::rules::retail_ini_fixture::retail_assets()
        .expect("Battle rules came from the explicit retail install");
    assert!(!rules.art().get("YAYARD").unwrap().new_theater);
    let source = load_shp_source(
        &assets,
        "YAYARD",
        None,
        "tem",
        "TEMPERATE",
        Some(rules),
        Some(rules.art()),
    )
    .expect("stock YAYARD body must load independently of its slot animations");
    assert_eq!(source.found_name, "YGYARD.SHP");
    assert_eq!([source.shp.width, source.shp.height], [304, 236]);
    assert_eq!(source.shp.frames.len(), 6);
    let frame = &source.shp.frames[0];
    assert_eq!(
        [
            frame.frame_x,
            frame.frame_y,
            frame.frame_width,
            frame.frame_height
        ],
        [90, 98, 98, 108]
    );
    assert!(frame.pixels.iter().any(|&pixel| pixel != 0));
}

fn raw_infantry_atlas_rules(sequence_rows: &str) -> (RuleSet, ArtRegistry) {
    use crate::rules::ini_parser::IniFile;
    let art_ini = IniFile::from_str(&format!(
        "[BOUNDGI]\nSequence=AtlasControl\n[AtlasControl]\nReady=0,1,0\n{sequence_rows}\n",
    ));
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
        &IniFile::from_str(
            "[InfantryTypes]\n0=RAWGI\n[RAWGI]\nImage=BOUNDGI\n\
             [VehicleTypes]\n0=LEGACY\n[LEGACY]\nImage=BOUNDGI\n",
        ),
        &art_ini,
    )
    .expect("fixed ART atlas rules");
    let art = ArtRegistry::from_ini(&art_ini);
    // Processing with fixed ART does not install RuleSet's metadata registry.
    // Bind the same image/Sequence owner used by the source loader first.
    rules.install_art_data(art.clone());
    rules.bind_animation_sequences(
        &crate::rules::infantry_sequence::parse_infantry_sequence_registry(&art_ini),
    );
    assert_eq!(
        rules
            .art()
            .resolve_metadata_entry("RAWGI", "BOUNDGI")
            .unwrap()
            .sequence
            .as_deref(),
        Some("AtlasControl"),
        "fixture metadata owns the selected Sequence name"
    );
    assert_eq!(
        rules
            .animation_sequence("RAWGI")
            .unwrap()
            .infantry_action(0)
            .unwrap()
            .frames_per_facing,
        1,
        "fixture Ready record is bound before key preparation"
    );
    (rules, art)
}

#[test]
fn raw_infantry_atlas_keeps_guard_and_actions_without_sequence_kinds() {
    // Original523D00 reads all42 records. Guard1, WetDie20/21, Tumble25 and
    // Carry39 must not disappear through the generic SequenceKind projection.
    // These marker SHPs validate production binding/key preparation, not pixels
    // or the full native class sequence which selects these actions.
    let directory = StoredFrameTestDirectory::new();
    directory.write_raw_and_rle_frames("BOUNDGI.SHP", [8, 8], [0, 0, 6, 4]);
    let assets = AssetManager::from_loose_root_for_test(&directory.0);
    let (world, interner) = one_object_world("RAWGI", EntityCategory::Infantry);
    let colors = [HouseColorIndex(2), HouseColorIndex(5)];
    let pairs = colors
        .map(|color| ("RAWGI".to_string(), color))
        .into_iter()
        .collect();
    for name in ["Guard", "WetDie1", "WetDie2", "Tumble", "Carry"] {
        let (rules, art) = raw_infantry_atlas_rules(&format!("{name}=1,1,0"));
        let mut needed = HashSet::new();
        let sources = insert_new_object_keys(
            &mut needed,
            &pairs,
            &world,
            Some(&interner),
            &assets,
            "tem",
            "TEMPERATE",
            Some(&rules),
            Some(&art),
        );
        assert_eq!(sources.len(), 1, "one decoded source for both colours");
        let source = sources["RAWGI"].as_ref().unwrap();
        assert_eq!(source.found_name, "BOUNDGI.SHP", "rules Image binding");
        assert_eq!(source.shp.frames.len(), 2);
        for color in colors {
            let frames: HashSet<_> = needed
                .iter()
                .filter(|key| key.type_id == "RAWGI" && key.house_color == color)
                .map(|key| key.frame)
                .collect();
            assert_eq!(frames, HashSet::from([0, 1]), "{name}, {color:?}");
        }
    }
}

#[test]
fn raw_infantry_atlas_bounds_signed_records_without_aliasing_indices() {
    // These signed reader inputs exercise cache admission through the original
    // record owner and the shared frame resolver. Native modulo/composition
    // receipts live in anytown_damage/foot_missions; no second frame arithmetic
    // implementation computes these test expectations.
    let directory = StoredFrameTestDirectory::new();
    directory.write_raw_and_rle_frames("BOUNDGI.SHP", [8, 8], [0, 0, 6, 4]);
    let assets = AssetManager::from_loose_root_for_test(&directory.0);
    let (world, interner) = one_object_world("RAWGI", EntityCategory::Infantry);
    let pairs = HashSet::from([("RAWGI".to_string(), HouseColorIndex(2))]);
    for (row, expected) in [
        ("Guard=65537,1,0", HashSet::from([0])),
        ("Guard=-65535,1,0", HashSet::from([0])),
        ("Guard=0,1,-65535", HashSet::from([0])),
        ("Guard=0,1,65536", HashSet::from([0])),
        ("Guard=-1,65537,-3", HashSet::from([0, 1])),
        ("Guard=65537,2147483647,0", HashSet::from([0, 1])),
        ("Guard=1,-2147483648,0", HashSet::from([0, 1])),
    ] {
        let (rules, art) = raw_infantry_atlas_rules(row);
        let mut needed = HashSet::new();
        let sources = insert_new_object_keys(
            &mut needed,
            &pairs,
            &world,
            Some(&interner),
            &assets,
            "tem",
            "TEMPERATE",
            Some(&rules),
            Some(&art),
        );
        assert_eq!(sources["RAWGI"].as_ref().unwrap().shp.frames.len(), 2);
        assert_eq!(
            infantry_asset_frames(
                rules
                    .animation_sequence("RAWGI")
                    .unwrap()
                    .infantry_actions()
                    .unwrap(),
                2,
            )
            .into_iter()
            .collect::<HashSet<_>>(),
            expected,
            "{row}: signed records still select the same body frames"
        );
        let frames: HashSet<_> = needed.iter().map(|key| key.frame).collect();
        assert_eq!(
            frames,
            HashSet::from([0, 1]),
            "{row}: body and shadow residency"
        );
        assert!(needed.iter().all(|key| key.frame < 2), "{row}");
    }
}

#[test]
#[ignore = "requires GPU; synthetic nonempty Infantry shadow through the production atlas and shared Techno shadow owner"]
fn infantry_nonempty_shadow_is_uploaded_and_selected_from_the_second_half() {
    use crate::app::presentation::instances::embedded_shp_shadow_piece;
    use crate::render::batch::{BatchRenderer, SpriteInstance};
    use crate::render::draw_state::{DrawDecision, DrawState};
    use crate::render::tactical_draw_plan::{BlitPolicy, SpriteEncoding};
    use crate::render::terrain_draw_gpu_tests::Gpu;

    let directory = StoredFrameTestDirectory::new();
    directory.write_raw_and_rle_frames("BOUNDGI.SHP", [8, 8], [0, 0, 6, 4]);
    let assets = AssetManager::from_loose_root_for_test(&directory.0);
    let (world, interner) = one_object_world("RAWGI", EntityCategory::Infantry);
    let (rules, art) = raw_infantry_atlas_rules("");
    let gpu = Gpu::new();
    let batch = BatchRenderer::new_with_device(
        &gpu.device,
        &gpu.queue,
        wgpu::TextureFormat::Bgra8UnormSrgb,
    );
    let palette = Palette::from_bytes(&[0; 768]).unwrap();
    let colors = HouseColorMap::from([("Americans".to_string(), HouseColorIndex(0))]);
    let atlas = build_sprite_atlas(
        &gpu.device,
        &gpu.queue,
        &batch,
        &world,
        &assets,
        &palette,
        "tem",
        "TEMPERATE",
        Some(&rules),
        Some(&art),
        &colors,
        &[],
        &HashSet::new(),
        &HashSet::new(),
        None,
        None,
        Some(&interner),
    )
    .unwrap();
    let key = ShpSpriteKey {
        palette_context: ShpPaletteContext::Legacy,
        type_id: "RAWGI".to_string(),
        facing: 0,
        frame: 0,
        house_color: HouseColorIndex(0),
    };
    let body = atlas.get(&key).unwrap();
    let selected = embedded_shp_shadow_piece(
        &atlas,
        rules.object("RAWGI"),
        world.values().next().unwrap(),
        None,
        &key,
        body,
        SpriteInstance::default(),
        DrawDecision {
            visible: true,
            state: DrawState::default(),
            native_visual_state: 0,
        },
        BlitPolicy::z_read(SpriteEncoding::Plain),
    )
    .expect("ground Infantry draws the same embedded stencil as other SHP Technos");
    let shadow_key = ShpSpriteKey { frame: 1, ..key };
    let shadow = atlas
        .get(&shadow_key)
        .expect("physical second half is resident");
    assert_eq!(selected.instance.uv_origin, shadow.uv_origin);
    let page = &atlas.page(shadow.page as usize).unwrap().texture;
    let origin = [
        (shadow.uv_origin[0] * page.width as f32).round() as u32,
        (shadow.uv_origin[1] * page.height as f32).round() as u32,
    ];
    let size = shadow.pixel_size.map(|value| value as u32);
    let pixels = gpu.read_uint_texels(&page.view, origin, size);
    assert_eq!(pixels.iter().filter(|&&index| index != 0).count(), 2);
    assert!(
        pixels.contains(&0),
        "transparent mask holes remain transparent"
    );
}

#[test]
fn raw_infantry_atlas_covers_saved_native_signed_remainders() {
    // The existing80-row receipt executes only signed native modulo, not
    // facing/composition or drawing. With supplied start/stride0, these saved
    // in-asset remainders must be included by cache membership; a narrow marker
    // asset never grows to the declared sequence count. This is not a complete
    // native frame-selection comparison.
    let receipt: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/anytown_damage/foot_missions.json",
    ))
    .unwrap();
    let rows = receipt["draw_stage_modulo_receipt"]["rows"]
        .as_array()
        .unwrap();
    for row in rows {
        let input = &row["input"];
        let count = input["count"].as_i64().unwrap() as i32;
        let remainder = row["result_registers"]["edx"].as_i64().unwrap();
        let action = crate::rules::infantry_sequence::InfantrySequenceEntry {
            start_frame: 0,
            frames_per_facing: count,
            facings: 0,
            facing_hint: None,
        };
        let frames = infantry_asset_frames(&[action], 2);
        assert!(frames.iter().all(|&frame| frame < 2));
        if (0..2).contains(&remainder) {
            assert!(
                frames.contains(&(remainder as u16)),
                "native count={count}, stage={}",
                input["stage"],
            );
        }
    }
    assert_eq!(rows.len(), 80);
}

#[test]
fn raw_infantry_atlas_missing_source_has_no_invented_frame_fallback() {
    let directory = StoredFrameTestDirectory::new();
    let assets = AssetManager::from_loose_root_for_test(&directory.0);
    let (rules, art) = raw_infantry_atlas_rules("Guard=1,1,0");
    let (world, interner) = one_object_world("RAWGI", EntityCategory::Infantry);
    let pairs = HashSet::from([
        ("RAWGI".to_string(), HouseColorIndex(2)),
        ("RAWGI".to_string(), HouseColorIndex(5)),
    ]);
    let mut needed = HashSet::new();
    let sources = insert_new_object_keys(
        &mut needed,
        &pairs,
        &world,
        Some(&interner),
        &assets,
        "tem",
        "TEMPERATE",
        Some(&rules),
        Some(&art),
    );
    assert_eq!(sources.len(), 1);
    assert!(sources["RAWGI"].is_none());
    assert!(needed.is_empty());
}

#[test]
fn raw_infantry_render_does_not_substitute_an_out_of_asset_key() {
    let directory = StoredFrameTestDirectory::new();
    directory.write_raw_and_rle_frames("BOUNDGI.SHP", [8, 8], [0, 0, 6, 4]);
    let assets = AssetManager::from_loose_root_for_test(&directory.0);
    let (rules, art) = raw_infantry_atlas_rules("Guard=1,1,0");
    let palette = Palette::from_bytes(&[0; 768]).unwrap();
    let source = load_shp_source(
        &assets,
        "RAWGI",
        None,
        "tem",
        "TEMPERATE",
        Some(&rules),
        Some(&art),
    )
    .unwrap();
    let mut key = make_shp_key("RAWGI", 0);
    key.frame = 2;
    assert!(
        render_shp_frame(
            &source,
            &palette,
            false,
            &key,
            Some(&rules),
            &mut ShpPalettes::default()
        )
        .is_none()
    );
    // Preserve the existing generic class adapter; this does not establish
    // original invalid-frame shape access or full Infantry drawing parity.
    key.type_id = "LEGACY".to_string();
    assert!(
        render_shp_frame(
            &source,
            &palette,
            false,
            &key,
            Some(&rules),
            &mut ShpPalettes::default()
        )
        .is_some()
    );
}

#[test]
fn building_animation_frames_render_from_the_file_they_were_counted_in() {
    // [YAGRND_B] authors no NewTheater=, and no TEMPERATE archive holds
    // YAGRND_B.SHP: AnimTypeClass forces the second letter to G and loads
    // YGGRND_B.SHP. This unclassified legacy object path does not reach that
    // file; rendering reuses the file the animation frame count came from.
    let directory = StoredFrameTestDirectory::new();
    directory.write_raw_and_rle_frames("YGGRND_B.SHP", [20, 10], [2, 1, 6, 4]);
    let assets = AssetManager::from_loose_root_for_test(&directory.0);
    let art = ArtRegistry::from_ini(&crate::rules::ini_parser::IniFile::from_str(
        "[YAGRND_B]\nImage=YAGRND_B\nLoopEnd=1\n",
    ));

    let (count, raw_count, file) =
        scan_building_anim_frame_count(&assets, &art, "YAGRND_B", 1, "tem", "TEMPERATE")
            .expect("the anim naming finds the forced-G file");
    assert_eq!(file, "YGGRND_B.SHP");
    assert_eq!(count, 1, "two stored frames: one body frame, one shadow");
    assert_eq!(raw_count, 2);

    assert!(
        load_shp_source(
            &assets,
            "YAGRND_B",
            None,
            "tem",
            "TEMPERATE",
            None,
            Some(&art)
        )
        .is_none(),
        "the object naming does not reach the forced-G file"
    );
    let source = load_shp_source(
        &assets,
        "YAGRND_B",
        Some(&file),
        "tem",
        "TEMPERATE",
        None,
        Some(&art),
    )
    .expect("the counted file decodes");
    let mut key = make_shp_key("YAGRND_B", 0);
    key.palette_context = ShpPaletteContext::SelectedScheme;
    let palette = Palette::from_bytes(&[0; 768]).expect("fixture palette");
    let sprite = render_shp_frame(
        &source,
        &palette,
        true,
        &key,
        None,
        &mut ShpPalettes::default(),
    )
    .expect("frame 0 has pixels");
    assert_eq!([sprite.width, sprite.height], [6, 4]);
}

#[test]
fn alt_palette_art_takes_the_unit_palette_even_when_it_is_a_world_effect() {
    // WCCLOUD1 (Weather Storm) and SQDG (squid grapple) are registered as world
    // effects, so the name set alone would bake them against anim.pal. Both set
    // AltPalette=yes, which selects the unit palette instead. FBALL1 sets nothing
    // and must stay on anim.pal.
    let ini = crate::rules::ini_parser::IniFile::from_str(
        "[WCCLOUD1]\nAltPalette=yes\n\
         [SQDG]\nAltPalette=yes\n\
         [FBALL1]\nLayer=ground\n",
    );
    let art = ArtRegistry::from_ini(&ini);
    let effects: HashSet<String> = ["WCCLOUD1", "SQDG", "FBALL1"]
        .iter()
        .map(|name| name.to_string())
        .collect();
    let cell_drawers = HashSet::new();

    assert_eq!(
        sprite_palette_choice("WCCLOUD1", Some(&art), &effects, &cell_drawers),
        SpritePaletteChoice::Unit
    );
    assert_eq!(
        sprite_palette_choice("SQDG", Some(&art), &effects, &cell_drawers),
        SpritePaletteChoice::Unit
    );
    assert_eq!(
        sprite_palette_choice("FBALL1", Some(&art), &effects, &cell_drawers),
        SpritePaletteChoice::Anim
    );
}

#[test]
fn gsi_13_08_effect_frame_count_halves_only_shadowed_non_scheduler_assets() {
    assert_eq!(available_effect_anim_frame_count(21, false, false), 21);
    assert_eq!(available_effect_anim_frame_count(21, false, true), 10);
    assert_eq!(available_effect_anim_frame_count(20, false, true), 10);
    assert_eq!(available_effect_anim_frame_count(20, true, true), 20);
    assert_eq!(available_effect_anim_frame_count(1, false, true), 1);
    assert_eq!(available_effect_anim_frame_count(0, false, true), 0);
}

#[test]
fn effect_shadow_frames_are_resident_without_extending_body_timing() {
    for scheduler_owned in [false, true] {
        let mut needed = HashSet::new();
        let mut counts = HashMap::new();
        let count = register_effect_anim_frames(
            &mut needed,
            &mut counts,
            "SHADOWFX",
            4,
            scheduler_owned,
            true,
        );
        let expected_count = if scheduler_owned { 4 } else { 2 };
        assert_eq!(count, expected_count);
        assert_eq!(counts["SHADOWFX"], expected_count);
        assert_eq!(needed.len(), 4);
        for frame in 0..4 {
            assert!(needed.contains(&ShpSpriteKey {
                palette_context: ShpPaletteContext::Legacy,
                type_id: "SHADOWFX".into(),
                facing: 0,
                frame,
                house_color: HouseColorIndex(0),
            }));
        }
    }
}

#[test]
fn building_anim_shadow_frames_follow_each_attached_palette_context() {
    let directory = StoredFrameTestDirectory::new();
    std::fs::write(directory.0.join("YGTEST.SHP"), raw_shp(&[([2, 2], 1); 4]))
        .expect("write four physical animation frames");
    let assets = AssetManager::from_loose_root_for_test(&directory.0);
    for (cell_drawer, context) in [
        ("yes", ShpPaletteContext::SelectedScheme),
        ("no", ShpPaletteContext::GlobalAnim),
    ] {
        let art = ArtRegistry::from_ini(&crate::rules::ini_parser::IniFile::from_str(&format!(
            "[YATEST]\nShadow=yes\nLoopEnd=2\nShouldUseCellDrawer={cell_drawer}\n"
        )));
        let mut needed = HashSet::new();
        let mut counts = HashMap::new();
        let mut files = HashMap::new();
        for color in [HouseColorIndex(1), HouseColorIndex(2)] {
            register_building_anim(
                &mut needed,
                &mut counts,
                &mut files,
                &assets,
                &art,
                "YATEST",
                2,
                color,
                "tem",
                "TEMPERATE",
            );
            for frame in 0..4 {
                assert!(needed.contains(&ShpSpriteKey {
                    palette_context: context,
                    type_id: "YATEST".into(),
                    facing: 0,
                    frame,
                    house_color: if context == ShpPaletteContext::GlobalAnim {
                        HouseColorIndex(0)
                    } else {
                        color
                    },
                }));
            }
        }
        assert_eq!(counts["YATEST"], (2, 4));
        assert_eq!(files["YATEST"], "YGTEST.SHP");
        assert_eq!(needed.len(), if cell_drawer == "yes" { 8 } else { 4 });
    }
}

#[test]
fn retail_refinery_anim_loads_its_physical_shadow_companions() {
    let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let (_, assets) = crate::rules::retail_ini_fixture::retail_assets()
        .expect("Battle rules came from the explicit retail install");
    let mut needed = HashSet::new();
    let mut counts = HashMap::new();
    let mut files = HashMap::new();
    register_building_anim(
        &mut needed,
        &mut counts,
        &mut files,
        &assets,
        battle.rules.art(),
        "YAREFN_A",
        2,
        HouseColorIndex(1),
        "tem",
        "TEMPERATE",
    );
    assert_eq!(counts["YAREFN_A"], (2, 4));
    assert_eq!(files["YAREFN_A"], "YGREFN_A.SHP");
    let source = load_shp_source(
        &assets,
        "YAREFN_A",
        Some(&files["YAREFN_A"]),
        "tem",
        "TEMPERATE",
        Some(&battle.rules),
        Some(battle.rules.art()),
    )
    .expect("physical refinery animation");
    let palette = Palette::from_bytes(assets.get_ref("unittem.pal").unwrap()).unwrap();
    for frame in 0..4 {
        let key = ShpSpriteKey {
            palette_context: ShpPaletteContext::SelectedScheme,
            type_id: "YAREFN_A".into(),
            facing: 0,
            frame,
            house_color: HouseColorIndex(1),
        };
        assert!(needed.contains(&key), "missing refinery frame {frame}");
        let sprite = render_shp_frame(
            &source,
            &palette,
            true,
            &key,
            Some(&battle.rules),
            &mut ShpPalettes::default(),
        )
        .expect("refinery body and shadow frames contain pixels");
        assert_eq!(
            [sprite.width, sprite.height],
            if frame < 2 { [24, 20] } else { [26, 20] }
        );
        assert!(sprite.indices().iter().any(|&index| index != 0));
    }
}

#[test]
fn cell_anim_remap_registration_covers_every_bound_frame_for_its_color() {
    let selected = ShpPaletteContext::SelectedScheme;
    let remaps = HashSet::from([
        ("CRATE_SPARK".to_string(), selected, HouseColorIndex(1)),
        ("OTHER".to_string(), selected, HouseColorIndex(2)),
    ]);
    let mut effects = EffectRegistry::default();
    effects
        .frames
        .insert("CRATE_SPARK".to_string(), ("Crate_Spark".to_string(), 3));
    let mut needed = HashSet::new();

    insert_anim_remap_frame_keys(&mut needed, &effects, &remaps);

    assert_eq!(needed.len(), 3, "OTHER is no registered effect");
    for frame in 0..3 {
        assert!(needed.contains(&ShpSpriteKey {
            palette_context: crate::render::sprite_atlas::ShpPaletteContext::SelectedScheme,
            type_id: "Crate_Spark".to_string(),
            facing: 0,
            frame,
            house_color: HouseColorIndex(1),
        }));
    }
    assert!(
        needed
            .iter()
            .all(|key| key.house_color != HouseColorIndex(2))
    );
}

#[test]
fn gsi_13_08_warpout_keeps_all_frames_and_drives_the_progressive_alpha_ladder() {
    let ini = crate::rules::ini_parser::IniFile::from_str("[WARPOUT]\nTranslucent=yes\nRate=120\n");
    let art = ArtRegistry::from_ini(&ini);
    let warpout = art
        .anim_runtime_config("WARPOUT")
        .expect("parsed WARPOUT animation type");
    assert!(warpout.translucent);
    assert!(!warpout.shadow);
    assert_eq!(warpout.explicit_end, None);
    assert_eq!(warpout.explicit_loop_end, None);

    let mut needed = HashSet::new();
    let mut active_anim_frame_counts = HashMap::new();
    let frame_count = register_effect_anim_frames(
        &mut needed,
        &mut active_anim_frame_counts,
        "WARPOUT",
        21,
        art.scheduler_anim_types().contains("WARPOUT"),
        warpout.shadow,
    );

    assert_eq!(active_anim_frame_counts["WARPOUT"], 21);
    assert_eq!(needed.len(), 21);
    for frame in 0..=20 {
        assert!(needed.contains(&ShpSpriteKey {
            palette_context: crate::render::sprite_atlas::ShpPaletteContext::Legacy,
            type_id: "WARPOUT".to_string(),
            facing: 0,
            frame,
            house_color: HouseColorIndex(0),
        }));
    }

    for (frame, expected_alpha) in [
        (4, 1.0),
        (5, 0.75),
        (8, 0.75),
        (9, 0.5),
        (12, 0.5),
        (13, 0.25),
        (20, 0.25),
    ] {
        let selection = crate::sim::anim_class::anim_translucency_selection(
            crate::sim::anim_class::AnimTranslucencyInput {
                base_flags: 0,
                forced_translucent: false,
                double_thick: false,
                translucency_detail_level: warpout.translucency_detail_level,
                game_detail_level: 2,
                translucent_ramp: warpout.translucent,
                current_frame: frame,
                end_frame: warpout.loaded_end(i32::from(frame_count)),
                explicit_translucency: warpout.translucency,
                instance_ramp: 0,
            },
        );
        assert!(selection.draw);
        assert_eq!(
            crate::rules::art_data::anim_translucency_source_alpha(selection.flags),
            expected_alpha,
            "WARPOUT frame {frame}",
        );
    }
}

/// An SHP(TS) of raw frames at the canvas origin, each `size` filled with its
/// palette index; the canvas is the largest frame.
fn raw_shp(frames: &[([u32; 2], u8)]) -> Vec<u8> {
    let canvas = |axis: usize| frames.iter().map(|(size, _)| size[axis]).max().unwrap_or(0);
    let mut data = Vec::new();
    for word in [0, canvas(0), canvas(1), frames.len() as u32] {
        data.extend_from_slice(&(word as u16).to_le_bytes());
    }
    let mut offset = 8 + frames.len() * 24;
    for &([width, height], _) in frames {
        for word in [0, 0, width, height] {
            data.extend_from_slice(&(word as u16).to_le_bytes());
        }
        data.extend_from_slice(&[0u8; 12]);
        data.extend_from_slice(&u32::try_from(offset).unwrap().to_le_bytes());
        offset += (width * height) as usize;
    }
    for &([width, height], index) in frames {
        data.extend(std::iter::repeat_n(index, (width * height) as usize));
    }
    data
}

#[test]
fn gsi_13_04_tem_only_tile_root_uses_iso_palette_and_registers_every_frame() {
    let mut rules = crate::rules::ruleset::RuleSet::from_ini(
        &crate::rules::ini_parser::IniFile::from_str("[General]\nDamageFireTypes=\n"),
    )
    .expect("rules");
    let mut art = ArtRegistry::from_ini(&crate::rules::ini_parser::IniFile::from_str(
        "[CUSTOM_TILE_ANIM]\nTheater=yes\nAltPalette=yes\nLoopCount=-1\n",
    ));
    art.bind_anim_frame_count_for_test("CUSTOM_TILE_ANIM", 4);
    rules.replace_art_registry_for_test(art);
    let effects: HashSet<String> = ["CUSTOM_TILE_ANIM".to_string()].into_iter().collect();
    let cell_drawers: HashSet<String> = ["CUSTOM_TILE_ANIM".to_string()].into_iter().collect();

    assert!(
        collect_effect_names(&rules)
            .iter()
            .any(|name| name == "CUSTOM_TILE_ANIM")
    );
    assert_eq!(
        sprite_palette_choice(
            "CUSTOM_TILE_ANIM",
            Some(rules.art()),
            &effects,
            &cell_drawers,
        ),
        SpritePaletteChoice::CellIso,
        "cell-drawer palette authority overrides ordinary Anim.PAL/AltPalette selection"
    );

    let candidates =
        effect_anim_shp_candidates("CUSTOM_TILE_ANIM", Some(rules.art()), "tem", "TEMPERATE");
    assert_eq!(
        candidates,
        vec!["CUSTOM_TILE_ANIM.TEM", "CUSTOM_TILE_ANIM.SHP"]
    );
    let tem_only = HashMap::from([(
        "CUSTOM_TILE_ANIM.TEM".to_string(),
        raw_shp(&[([1, 1], 1); 4]),
    )]);
    assert!(!tem_only.contains_key("CUSTOM_TILE_ANIM.SHP"));
    let data = candidates
        .iter()
        .find_map(|candidate| tem_only.get(candidate))
        .expect("theater-aware preload must resolve the TEM-only SHP");
    let shp = ShpFile::from_bytes(data).expect("TEM candidate is valid SHP(TS) data");

    let mut needed = HashSet::new();
    let mut counts = HashMap::new();
    let count = register_effect_anim_frames(
        &mut needed,
        &mut counts,
        "CUSTOM_TILE_ANIM",
        shp.frames.len() as u16,
        true,
        false,
    );
    assert_eq!(count, 4);
    assert_eq!(counts["CUSTOM_TILE_ANIM"], 4);
    for frame in 0..4 {
        assert!(needed.contains(&ShpSpriteKey {
            palette_context: crate::render::sprite_atlas::ShpPaletteContext::Legacy,
            type_id: "CUSTOM_TILE_ANIM".to_string(),
            facing: 0,
            frame,
            house_color: HouseColorIndex(0),
        }));
    }
}

#[test]
fn collect_effect_names_includes_weapon_anim_entries() {
    let ini = crate::rules::ini_parser::IniFile::from_str(
        "\
[InfantryTypes]\n0=E1\n\n\
[VehicleTypes]\n\n\
[AircraftTypes]\n\n\
[BuildingTypes]\n\n\
[E1]\nStrength=125\nArmor=flak\nSpeed=4\nPrimary=TestWeapon\n\n\
[TestWeapon]\nDamage=1\nWarhead=TestWH\nAnim=MGUN-N,MGUN-NE,MGUN-E,MGUN-SE,MGUN-S,MGUN-SW,MGUN-W,MGUN-NW\nOccupantAnim=UCFLASH\n\n\
[TestWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    );
    let rules = crate::rules::ruleset::RuleSet::from_ini(&ini).expect("rules parse");
    let names = collect_effect_names(&rules);
    assert!(names.iter().any(|name| name == "MGUN-N"));
    assert!(names.iter().any(|name| name == "MGUN-NW"));
    assert!(names.iter().any(|name| name == "UCFLASH"));
}

/// A real loose SHP frame (indices 0..=255) laid out at (3, 1) on a 262x3
/// atlas page.
pub(crate) struct NativePaletteProbe {
    /// The palette row its indices resolve through ([`ShpPalettes`]).
    pub palette_row: Vec<u8>,
    /// The page's palette indices.
    pub indices: Vec<u8>,
    /// The same page in RGBA, as `ShpFile::frame_to_rgba` converts the frame.
    pub rgba: Vec<u8>,
    pub size: [u32; 2],
}

/// The synthetic palette `tools/palette_oracle` built its native LightConvert
/// fixtures from: RGB(index, index * 73 mod 256, 255 - index).
pub(crate) fn native_probe_rgb(index: u8) -> [u8; 3] {
    [index, index.wrapping_mul(73), 255 - index]
}

/// The probe page in the native fixtures' palette.
pub(crate) fn native_palette_probe_page() -> NativePaletteProbe {
    palette_probe_page(&Palette::from_rgb(native_probe_rgb))
}

/// Real loose SHP decoding and palette registration, laid out on an atlas page.
pub(crate) fn palette_probe_page(palette: &Palette) -> NativePaletteProbe {
    let directory = StoredFrameTestDirectory::new();
    let mut bytes = Vec::new();
    for value in [0u16, 256, 1, 1] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    let mut header = [0u8; 24];
    header[4..6].copy_from_slice(&256u16.to_le_bytes());
    header[6..8].copy_from_slice(&1u16.to_le_bytes());
    header[8] = 1;
    header[20..24].copy_from_slice(&32u32.to_le_bytes());
    bytes.extend_from_slice(&header);
    bytes.extend(0..=255u8);
    std::fs::write(directory.0.join("PALPROBE.SHP"), bytes).unwrap();
    let assets = AssetManager::from_loose_root_for_test(&directory.0);
    let key = ShpSpriteKey {
        palette_context: ShpPaletteContext::SelectedScheme,
        type_id: "PALPROBE".into(),
        facing: 0,
        frame: 0,
        house_color: crate::rules::house_colors::NO_REMAP,
    };
    let source = load_shp_source(&assets, "PALPROBE", None, "tem", "TEMPERATE", None, None)
        .expect("probe SHP decodes");
    let mut palettes = ShpPalettes::default();
    let sprite = render_shp_frame(&source, palette, true, &key, None, &mut palettes).unwrap();
    assert_eq!(sprite.indices(), (0..=255u8).collect::<Vec<_>>());
    let size = [262, 3];
    let mut indices = vec![0; size[0] * size[1]];
    let mut rgba = vec![0; size[0] * size[1] * 4];
    // The 256x1 frame sits at (3, 1).
    let start = size[0] + 3;
    indices[start..start + 256].copy_from_slice(&sprite.indices());
    rgba[start * 4..(start + 256) * 4]
        .copy_from_slice(&source.shp.frame_to_rgba(0, palette).unwrap());
    assert_eq!(&indices[265..521], (0..=255u8).collect::<Vec<_>>());
    NativePaletteProbe {
        palette_row: palettes.colors[sprite.palette as usize].clone(),
        indices,
        rgba,
        size: size.map(|v| v as u32),
    }
}

#[test]
fn palette_context_preserves_simultaneous_global_cell_and_attached_sprite_keys() {
    let art = ArtRegistry::from_ini(&crate::rules::ini_parser::IniFile::from_str(
        "[SHARED]\nShouldUseCellDrawer=yes\n",
    ));
    let mut key = make_shp_key("SHARED", 0);
    let mut keys = HashSet::new();
    let effects = HashSet::from(["SHARED".to_string()]);
    let cells = HashSet::from(["SHARED".to_string()]);
    for (context, palette) in [
        (ShpPaletteContext::GlobalAnim, SpritePaletteChoice::Anim),
        (ShpPaletteContext::SelectedScheme, SpritePaletteChoice::Unit),
        (ShpPaletteContext::Cell, SpritePaletteChoice::CellIso),
    ] {
        key.palette_context = context;
        keys.insert(key.clone());
        assert_eq!(
            sprite_palette_for_key(&key, Some(&art), &effects, &cells, None),
            palette
        );
    }
    assert_eq!(keys.len(), 3);
    let probe = native_palette_probe_page();
    assert_eq!(&probe.indices[505..520], (240..=254u8).collect::<Vec<_>>());
    assert_eq!(probe.palette_row[240 * 4], 240);
}

/// TechnoClass::DrawSHP: TerrainPalette first (`0x00705EC7`), then
/// GetRemapColour (`0x00705F81`), whose `+0xDF0` arm takes the extra building
/// palette. An ExtraScheme anim remap needs only the loaded palette.
#[test]
fn a_custom_palette_type_bakes_against_the_extra_building_palette() {
    let art = ArtRegistry::from_ini(&crate::rules::ini_parser::IniFile::from_str(""));
    let none = HashSet::new();
    let custom = HashSet::from(["STATUE".to_string()]);
    let choose = |type_id: &str, context, cells: &HashSet<String>, extra| {
        let mut key = make_shp_key(type_id, 0);
        key.palette_context = context;
        sprite_palette_for_key(&key, Some(&art), &none, cells, extra)
    };
    let legacy = ShpPaletteContext::Legacy;
    assert_eq!(
        choose("STATUE", legacy, &none, Some(&custom)),
        SpritePaletteChoice::ExtraBuilding
    );
    assert_eq!(
        choose("GAPOWR", legacy, &none, Some(&custom)),
        SpritePaletteChoice::Unit
    );
    assert_eq!(
        choose("STATUE", legacy, &none, None),
        SpritePaletteChoice::Unit,
        "no palette file: +0xDF0 stays null"
    );
    assert_eq!(
        choose("STATUE", legacy, &custom, Some(&custom)),
        SpritePaletteChoice::CellIso,
        "TerrainPalette is tested first"
    );
    let mut interner = crate::sim::intern::StringInterner::new();
    let extra = ShpPaletteContext::ExtraScheme(interner.intern("lib"));
    assert_eq!(
        choose("DESTA", extra, &none, Some(&none)),
        SpritePaletteChoice::ExtraBuilding
    );
    assert_eq!(
        choose("DESTA", extra, &none, None),
        SpritePaletteChoice::Unit
    );
}

/// Retail Manhattan: the Statue of Liberty (`[CANEWY04] Palette=lib`) is the
/// one custom-palette type; every theater has its `LIB<suffix>.PAL`
/// (`0x00627590`), and the statue's body bakes against `LIBUBN.PAL` with the
/// house ramp over 16..31 (`0x0068C3B0`), not against the unit palette.
#[test]
fn retail_statue_bakes_against_the_theater_lib_palette() {
    let Some(battle) =
        crate::rules::retail_ini_fixture::retail_battle_rules_for_map("MANHATTA.MAP")
    else {
        return;
    };
    let rules = &battle.rules;
    let (_, assets) = crate::rules::retail_ini_fixture::retail_assets()
        .expect("Battle rules came from the explicit retail install");
    let custom: Vec<&str> = rules
        .all_objects()
        .filter(|o| o.custom_palette_name().is_some())
        .map(|o| o.id.as_str())
        .collect();
    assert_eq!(custom, ["CANEWY04"]);
    for suffix in ["tem", "sno", "urb", "des", "ubn", "lun"] {
        assert!(
            rules
                .load_extra_building_palette(&assets, suffix, None)
                .is_some(),
            "LIB{suffix}.PAL"
        );
    }

    let (name, lib) = rules
        .load_extra_building_palette(&assets, "ubn", None)
        .unwrap();
    assert_eq!(name, "lib");
    assert_eq!(
        lib.to_rgba_bytes(),
        Palette::from_bytes(assets.get_ref("LIBUBN.PAL").unwrap())
            .unwrap()
            .to_rgba_bytes(),
        "<name><theater suffix>.PAL"
    );
    let unit = Palette::from_bytes(assets.get_ref("unitubn.pal").unwrap()).unwrap();
    let key = ShpSpriteKey {
        palette_context: ShpPaletteContext::Legacy,
        type_id: "CANEWY04".to_string(),
        facing: 0,
        frame: 0,
        house_color: HouseColorIndex(3),
    };
    let choice = sprite_palette_for_key(
        &key,
        Some(rules.art()),
        &HashSet::new(),
        &HashSet::new(),
        Some(&HashSet::from(["CANEWY04".to_string()])),
    );
    assert_eq!(choice, SpritePaletteChoice::ExtraBuilding);
    let source = load_shp_source(
        &assets,
        "CANEWY04",
        None,
        "ubn",
        "NEWURBAN",
        Some(rules),
        Some(rules.art()),
    )
    .expect("CGNEWY04.SHP");
    let mut palettes = ShpPalettes::default();
    let sprite = render_shp_frame(&source, &lib, true, &key, Some(rules), &mut palettes)
        .expect("statue frame 0");
    let row = &palettes.colors[sprite.palette as usize];
    let ramp = rules.house_color_ramps.ramp(HouseColorIndex(3));
    assert_eq!(row, &lib.with_house_colors(ramp).to_rgba_bytes());
    assert_ne!(row, &unit.with_house_colors(ramp).to_rgba_bytes());
}

#[test]
fn deployed_anim_palette_and_atlas_coverage_share_the_frozen_house() {
    use crate::rules::ini_parser::IniFile;
    use crate::sim::anim_class::AnimRemap;
    use crate::sim::components::AnimClassSpawnDescriptor;
    use crate::sim::world::Simulation;
    use crate::util::fixed_math::SimFixed;

    let art_ini = IniFile::from_str("[DEPLOY]\nRate=9\n");
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
        &IniFile::from_str("[Animations]\n0=DEPLOY\n"),
        &art_ini,
    )
    .expect("deploy animation rules");
    let mut art = ArtRegistry::from_ini(&art_ini);
    art.bind_anim_frame_count_for_test("DEPLOY", 3);
    rules.install_art_fixture(art);
    let mut sim = Simulation::new();
    let mut atlas = SpriteAtlas::new(Vec::new(), HashMap::new(), ShpPalettes::default());
    let colors = HouseColorMap::from([("Americans".to_string(), HouseColorIndex(3))]);
    assert!(atlas_covers_anim_remaps(Some(&atlas), &sim, &colors));
    let house = sim.intern("Americans");
    let palette = sim.intern("lib");
    let type_id = sim.intern("DEPLOY");
    let spawn = |sim: &mut Simulation| {
        sim.spawn_anim_object(
            &rules,
            AnimClassSpawnDescriptor::new(
                type_id,
                10,
                10,
                SimFixed::from_num(128),
                SimFixed::from_num(128),
                0,
            ),
        )
        .expect("deploy animation constructs")
    };
    let deployed = spawn(&mut sim);
    let cell = spawn(&mut sim);
    let wreck = spawn(&mut sim);
    assert!(atlas_covers_anim_remaps(Some(&atlas), &sim, &colors));
    assert!(
        sim.anims_mut_for_test()
            .set_anim_house_remap(deployed, house)
    );
    assert!(sim.anims_mut_for_test().set_cell_anim_draw_authority(
        cell,
        Some(HouseColorIndex(5)),
        0
    ));
    assert!(
        sim.anims_mut_for_test()
            .set_anim_remap(wreck, AnimRemap::ExtraBuildingScheme { house, palette })
    );
    assert!(
        !atlas_covers_anim_remaps(Some(&atlas), &sim, &colors),
        "producer palette assignment must request a refresh without an entity change"
    );
    assert!(!atlas_covers_anim_remaps(None, &sim, &colors));
    assert_eq!(
        sim.anim(deployed).unwrap().remap(),
        Some(AnimRemap::House(house))
    );
    assert_eq!(
        anim_remap_color(sim.anim(deployed).unwrap(), sim.interner(), &colors),
        Some(HouseColorIndex(3))
    );
    let remaps = collect_anim_remap_base_keys(&sim, &colors);
    let selected = ShpPaletteContext::SelectedScheme;
    assert_eq!(
        remaps,
        HashSet::from([
            ("DEPLOY".to_string(), selected, HouseColorIndex(3)),
            ("DEPLOY".to_string(), selected, HouseColorIndex(5)),
            (
                "DEPLOY".to_string(),
                ShpPaletteContext::ExtraScheme(palette),
                HouseColorIndex(3)
            ),
        ])
    );
    let mut keys = HashSet::new();
    let effects = EffectRegistry {
        frames: HashMap::from([("DEPLOY".to_string(), ("DEPLOY".to_string(), 3))]),
        type_ids: HashSet::from(["DEPLOY".to_string()]),
    };
    insert_anim_remap_frame_keys(&mut keys, &effects, &remaps);
    assert_eq!(keys.len(), 9);
    assert!(keys.iter().all(|key| {
        key.frame < 3
            && match key.palette_context {
                ShpPaletteContext::SelectedScheme => {
                    matches!(key.house_color, HouseColorIndex(3 | 5))
                }
                ShpPaletteContext::ExtraScheme(name) => {
                    name == palette && key.house_color == HouseColorIndex(3)
                }
                _ => false,
            }
    }));
    atlas.covered_anim_remaps.extend(remaps.iter().cloned());
    assert!(
        atlas_covers_anim_remaps(Some(&atlas), &sim, &colors),
        "covered live animations do not request another upload"
    );

    let bytes = save_codec::serialize(&sim).expect("save frozen animation remaps");
    let restored: Simulation = save_codec::deserialize(&bytes).expect("restore animation remaps");
    assert_eq!(collect_anim_remap_base_keys(&restored, &colors), remaps);
    assert_eq!(
        restored.anim(deployed).unwrap().remap(),
        Some(AnimRemap::House(house))
    );
    assert_eq!(
        restored.anim(wreck).unwrap().remap(),
        Some(AnimRemap::ExtraBuildingScheme { house, palette })
    );
}

#[test]
fn object_and_remap_coverage_are_separate_and_follow_the_world() {
    let (world, interner) = one_object_world("CRATE_SPARK", EntityCategory::Structure);
    let colors = HouseColorMap::from([("Americans".to_string(), HouseColorIndex(3))]);
    let remap = HashSet::from([(
        "CRATE_SPARK".to_string(),
        ShpPaletteContext::SelectedScheme,
        HouseColorIndex(3),
    )]);
    let none = HashSet::new();
    let covers = |atlas: Option<&SpriteAtlas>, remaps| {
        atlas_covers_world(atlas, &world, &colors, &[], remaps, Some(&interner))
    };
    assert!(
        !covers(None, &none),
        "no atlas yet, and the world draws a sprite"
    );

    let mut atlas = SpriteAtlas::new(Vec::new(), HashMap::new(), ShpPalettes::default());
    assert!(!covers(Some(&atlas), &none));
    atlas
        .covered_objects
        .entry("CRATE_SPARK".to_string())
        .or_default()
        .insert(HouseColorIndex(3));
    assert!(covers(Some(&atlas), &none));
    assert!(
        !covers(Some(&atlas), &remap),
        "a coincident entity must not suppress animation preload"
    );
    atlas.covered_anim_remaps.extend(remap.iter().cloned());
    assert!(
        covers(Some(&atlas), &remap),
        "loaded animation must not force repeated atlas uploads"
    );

    let recoloured = HouseColorMap::from([("Americans".to_string(), HouseColorIndex(4))]);
    assert!(
        !atlas_covers_world(
            Some(&atlas),
            &world,
            &recoloured,
            &[],
            &remap,
            Some(&interner)
        ),
        "a new house colour is a new pair"
    );
    assert!(
        !atlas_covers_world(
            Some(&atlas),
            &world,
            &colors,
            &["GACNST"],
            &remap,
            Some(&interner)
        ),
        "deploy targets are pairs in every house colour"
    );
}

#[test]
fn a_collected_pair_is_covered_even_when_nothing_of_it_is_drawable() {
    // A type whose frame 0 is empty or whose SHP is missing has no atlas entry
    // at all. Coverage by collected pairs keeps it from forcing a refresh on
    // every spawn, death or Limbo, as a frame-0 entry probe would.
    let (world, interner) = one_object_world("NOSHAPE", EntityCategory::Structure);
    let colors = HouseColorMap::from([("Americans".to_string(), HouseColorIndex(1))]);
    let mut atlas = SpriteAtlas::new(Vec::new(), HashMap::new(), ShpPalettes::default());
    let covers = |atlas: &SpriteAtlas| {
        atlas_covers_world(
            Some(atlas),
            &world,
            &colors,
            &[],
            &HashSet::new(),
            Some(&interner),
        )
    };
    atlas.unrenderable.insert(make_shp_key("NOSHAPE", 0));
    assert!(!covers(&atlas));
    atlas
        .covered_objects
        .entry("NOSHAPE".to_string())
        .or_default()
        .insert(HouseColorIndex(1));
    assert!(covers(&atlas));
    assert!(atlas.get(&make_shp_key("NOSHAPE", 0)).is_none());
}

#[test]
#[ignore = "requires a wgpu adapter; actual growth-page uploads"]
fn refreshed_sprites_upload_to_a_growth_page_and_leave_resident_ones_in_place() {
    let gpu = crate::render::terrain_draw_gpu_tests::Gpu::new();
    let batch = BatchRenderer::new_with_device(
        &gpu.device,
        &gpu.queue,
        wgpu::TextureFormat::Bgra8UnormSrgb,
    );
    // One raw frame a sprite, filled with its palette index.
    let frames = [([3, 2], 7), ([4, 3], 11), ([2, 5], 9)];
    let shp = raw_shp(&frames);
    let sprite = |type_id: &str, frame: usize, palette| RenderedShpSprite {
        key: make_shp_key(type_id, 0),
        palette,
        shp: &shp,
        frame,
        width: frames[frame].0[0],
        height: frames[frame].0[1],
        offset_x: 0.0,
        offset_y: 0.0,
        canvas_rect: [
            0.0,
            0.0,
            frames[frame].0[0] as f32,
            frames[frame].0[1] as f32,
        ],
        extended: false,
    };
    let resident_palette = Palette::from_rgb(|i| [i, 255 - i, 7]);
    let new_palette = Palette::from_rgb(|i| [3, i, i / 2]);
    let mut palettes = ShpPalettes::default();
    let resident_row = palettes.row(&resident_palette).unwrap();
    let mut atlas = pack_sprites(
        &gpu.device,
        &gpu.queue,
        &batch,
        vec![sprite("RESIDENT", 0, resident_row)],
        palettes,
    );
    let resident = *atlas.get(&make_shp_key("RESIDENT", 0)).unwrap();
    let rows_texture = atlas.palettes.texture.clone().unwrap();

    // A refresh renders against a copy of the atlas's palettes, which a new
    // house colour extends by a row.
    let mut palettes = atlas.palettes.clone();
    let new_row = palettes.row(&new_palette).unwrap();
    assert_eq!(palettes.row(&resident_palette), Some(resident_row));
    atlas.append_sprites(
        &gpu.device,
        &gpu.queue,
        &batch,
        vec![sprite("WIDE", 1, new_row), sprite("TALL", 2, resident_row)],
        palettes,
    );

    assert_eq!(
        atlas.page_count(),
        2,
        "one growth page after the packed one"
    );
    let after = *atlas.get(&make_shp_key("RESIDENT", 0)).unwrap();
    assert_eq!(
        (after.page, after.uv_origin, after.uv_size),
        (resident.page, resident.uv_origin, resident.uv_size),
        "resident sprites never move"
    );
    assert_eq!(after.source_palette, resident_row + 1);
    assert!(
        atlas.palettes.texture.as_ref() == Some(&rows_texture),
        "resident pages keep binding the palette rows the refresh wrote"
    );
    let reference = {
        use wgpu::util::DeviceExt;
        let rows = [resident_palette, new_palette].map(|palette| palette.to_rgba_bytes());
        gpu.device
            .create_texture_with_data(
                &gpu.queue,
                &wgpu::TextureDescriptor {
                    label: None,
                    size: crate::render::terrain_draw_gpu_tests::extent([256, 2]),
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8UnormSrgb,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
                wgpu::util::TextureDataOrder::LayerMajor,
                &rows.concat(),
            )
            .create_view(&Default::default())
    };
    assert_eq!(
        gpu.read_float_texels(&rows_texture.1, [0, 0], [256, 2]),
        gpu.read_float_texels(&reference, [0, 0], [256, 2]),
        "the pack uploaded the resident row and the refresh appended the new one"
    );
    let growth = atlas.growth.as_ref().expect("growth page");
    let page = &atlas.pages[growth.page].texture;
    let indices = atlas.growth_indices[&growth.page].create_view(&Default::default());
    for (type_id, index, size, origin, row) in [
        // Tallest first: TALL opens the shelf, WIDE follows after 1px padding.
        ("TALL", 9u8, [2u32, 5u32], [0u32, 0u32], resident_row),
        ("WIDE", 11, [4, 3], [3, 0], new_row),
    ] {
        let entry = atlas.get(&make_shp_key(type_id, 0)).unwrap();
        assert_eq!(usize::from(entry.page), growth.page);
        assert_eq!(entry.source_palette, row + 1, "{type_id} palette row");
        assert_eq!(
            [
                (entry.uv_origin[0] * page.width as f32).round() as u32,
                (entry.uv_origin[1] * page.height as f32).round() as u32,
            ],
            origin
        );
        assert_eq!(entry.pixel_size, size.map(|v| v as f32));
        assert_eq!(
            gpu.read_uint_texels(&indices, origin, size),
            vec![index; (size[0] * size[1]) as usize],
            "{type_id} indices uploaded to its rectangle"
        );
    }
}

#[test]
#[ignore = "requires a wgpu adapter; actual band uploads"]
fn map_load_pages_upload_in_shelf_bands_decoding_each_sprite_frame() {
    // Shelves at rows 0, 4 and 7 of a 10x8 page, as the pack lays them out.
    // A zero- or one-byte band uploads each shelf on its own, 31 bytes (one
    // past the first shelf's 30) the first two shelves together, and an
    // unbounded band all of them in one write; each must decode every
    // sprite's frame onto its rectangle and leave the rest zero.
    let gpu = crate::render::terrain_draw_gpu_tests::Gpu::new();
    let batch = BatchRenderer::new_with_device(
        &gpu.device,
        &gpu.queue,
        wgpu::TextureFormat::Bgra8UnormSrgb,
    );
    let palette_rows = ShpPalettes::default().upload(&gpu.device, &gpu.queue);
    let size = [10u32, 8];
    let shelves: [(u8, [u32; 2], [u32; 2]); 4] = [
        (1, [4, 3], [0, 0]),
        (2, [3, 2], [5, 0]),
        (3, [6, 2], [0, 4]),
        (4, [2, 1], [0, 7]),
    ];
    let mut expected = vec![0u8; (size[0] * size[1]) as usize];
    for &(index, [width, height], [x, y]) in &shelves {
        for row in y..y + height {
            let start = (row * size[0] + x) as usize;
            expected[start..start + width as usize].fill(index);
        }
    }
    let shp = raw_shp(&shelves.map(|(index, size, _)| (size, index)));
    for band_bytes in [0, 1, 31, u64::MAX] {
        let sprites: Vec<RenderedShpSprite> = shelves
            .iter()
            .enumerate()
            .map(|(frame, &(index, [width, height], _))| RenderedShpSprite {
                key: make_shp_key(&format!("SHELF{index}"), 0),
                palette: 0,
                shp: &shp,
                frame,
                width,
                height,
                offset_x: 0.0,
                offset_y: 0.0,
                canvas_rect: [0.0, 0.0, width as f32, height as f32],
                extended: false,
            })
            .collect();
        let placed: Vec<(usize, [u32; 2])> = shelves
            .iter()
            .enumerate()
            .map(|(idx, &(_, _, origin))| (idx, origin))
            .collect();
        let (_, indices) = batch.create_indexed_page(&gpu.device, size[0], size[1], &palette_rows);
        upload_shelves(
            &gpu.device,
            &gpu.queue,
            &indices,
            &sprites,
            &placed,
            band_bytes,
        );
        assert_eq!(
            gpu.read_uint_texels(&indices.create_view(&Default::default()), [0, 0], size),
            expected,
            "band {band_bytes}: sprites on their rectangles, zero elsewhere"
        );
    }
}

#[test]
#[ignore = "requires retail archives; lists animations the object naming cannot reach"]
fn retail_animations_render_from_the_file_their_frames_are_counted_in() {
    // Before, every key rendered through the object naming while animation
    // frames were counted through AnimTypeClass's. Where the two disagree the
    // frames were registered but never drawable, and every refresh retried
    // them. Lists every such animation on a stock TEMPERATE map.
    //
    // Run: RA2_DIR=<retail root> cargo test -p vera20k --lib \
    //     retail_animations_render_from_the_file -- --ignored --nocapture
    let root = std::env::var("RA2_DIR")
        .ok()
        .filter(|path| !path.trim().is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            crate::util::config::GameConfig::load()
                .expect("set RA2_DIR or provide config.toml for this ignored test")
                .paths
                .ra2_dir
        });
    let scenario =
        crate::headless_scenario::load(&root, "Dustbowl.mmx", 0x0B21_D6E5).expect("Dustbowl loads");
    let mut assets =
        AssetManager::new(&root, MediaArchiveMode::STOCK_DIGITAL).expect("retail archives");
    let theater_name = scenario.map.header.theater.clone();
    let theater =
        crate::map::theater::load_theater(&mut assets, &theater_name).expect("theater loads");
    let rules = &scenario.runtime.resources.rules;
    let art = rules.art();

    let mut names: std::collections::BTreeSet<String> =
        collect_effect_names(rules).into_iter().collect();
    names.extend(art.building_anim_roots());
    let mut unreachable = Vec::new();
    for name in &names {
        let candidates =
            effect_anim_shp_candidates(name, Some(art), theater.extension, &theater_name);
        let Some((counted, _)) = find_shp(&assets, &candidates) else {
            continue;
        };
        let object = load_shp_source(
            &assets,
            name,
            None,
            theater.extension,
            &theater_name,
            Some(rules),
            Some(art),
        )
        .map(|source| source.found_name);
        if object.as_deref() != Some(counted) {
            unreachable.push(format!(
                "{name}: counted in {counted}, object naming {object:?}"
            ));
        }
    }
    eprintln!(
        "{} of {} animations on {theater_name} now render from the file their frames are counted in:\n{}",
        unreachable.len(),
        names.len(),
        unreachable.join("\n")
    );
    assert!(
        unreachable
            .iter()
            .any(|line| line.starts_with("YAGRND_B: counted in YGGRND_B.SHP")),
        "the Grinder's grinding animation is the known case"
    );
}

#[test]
fn animation_atlas_candidates_keep_exact_type_image_and_skip_unread_d() {
    let mut art = ArtRegistry::from_ini(&crate::rules::ini_parser::IniFile::from_str(
        "[ FX]\nImage=REAL\n[FX]\nImage=WRONG\n",
    ));
    art.apply_anim_type_read_states(&[(" FX".to_owned(), true), ("D".to_owned(), false)]);
    assert_eq!(
        effect_anim_shp_candidates(" FX", Some(&art), "TEM", "TEMPERATE"),
        vec!["REAL.SHP", "RGAL.SHP", "REAL.TEM"]
    );
    assert_eq!(
        effect_anim_shp_candidates("FX", Some(&art), "TEM", "TEMPERATE"),
        vec!["WRONG.SHP", "WGONG.SHP", "WRONG.TEM"]
    );
    assert!(effect_anim_shp_candidates("D", Some(&art), "TEM", "TEMPERATE").is_empty());
}

/// Production Rules layering, atlas load/refresh, texture growth and a real
/// indexed Batch sprite with a row beyond 255. Synthetic regression, not a
/// rendered retail/native parity claim. Actual mod resources are unnecessary.
#[test]
#[ignore = "requires a wgpu adapter; production indexed draw and palette texture readback"]
fn building_palette_groups_survive_refresh_and_palette_texture_growth() {
    use crate::render::{
        batch::{InstanceBufferPool, SpriteInstance},
        terrain_draw_gpu_tests::{Gpu, camera, clear, sprite},
    };
    use crate::rules::ini_parser::IniFile;
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
    let directory = StoredFrameTestDirectory::new();
    let shp = raw_shp(&[([2, 2], 7), ([2, 2], 7)]);
    for name in [
        "A.SHP",
        "B.SHP",
        "MISS.SHP",
        "CELL.SHP",
        "AMK.SHP",
        "BMK.SHP",
        "WRECK.SHP",
    ] {
        std::fs::write(directory.0.join(name), &shp).unwrap();
    }
    for (name, rgb) in [
        ("FIRSTTEM.PAL", [63u8, 0, 0]),
        ("SECONDTEM.PAL", [0, 63, 0]),
    ] {
        let mut bytes = [0u8; 768];
        bytes[21..24].copy_from_slice(&rgb);
        std::fs::write(directory.0.join(name), bytes).unwrap();
    }
    let assets = AssetManager::from_loose_root_for_test(&directory.0);
    let art = IniFile::from_str(
        "[FIRST]\nPalette=first\nImage=A\nBuildup=AMK\n[EMPTY]\nImage=A\nBuildup=AMK\n\
         [B]\nPalette=second\nBuildup=BMK\n[MISS]\nPalette=absent\n[CELL]\nPalette=second\nTerrainPalette=yes\n[WRECK]\nEnd=2\n",
    );
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[BuildingTypes]\n0=A\n1=B\n2=MISS\n3=CELL\n[A]\nImage=FIRST\nStrength=10\nDestroyAnim=WRECK\n[B]\nStrength=10\nDestroyAnim=WRECK\n[MISS]\nStrength=10\n[CELL]\nStrength=10\n[Animations]\n0=WRECK\n",
    ));
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[A]\nImage=EMPTY\n"),
    );
    let processed = layers.process_with_fixed_art(&art).unwrap();
    let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
    let mut registry = ArtRegistry::from_ini(&art);
    registry.bind_anim_frame_count_for_test("WRECK", 2);
    rules.install_art_data(registry);
    assert_eq!(
        rules.object("A").unwrap().custom_palette_name(),
        Some("first")
    );
    let unit = Palette::from_rgb(|_| [0, 0, 252]);
    let cell = Palette::from_rgb(|_| [252, 252, 0]);
    let gpu = Gpu::new();
    let format = wgpu::TextureFormat::Bgra8UnormSrgb;
    let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
    let entities = crate::sim::entity_store::EntityStore::new();
    let colors =
        HouseColorMap::from([("Americans".to_owned(), crate::rules::house_colors::NO_REMAP)]);
    let no_remaps = HashSet::new();
    let no_cell_drawers = HashSet::new();
    let load = |types: &[&str], existing| {
        build_sprite_atlas(
            &gpu.device,
            &gpu.queue,
            &batch,
            &entities,
            &assets,
            &unit,
            "tem",
            "TEMPERATE",
            Some(&rules),
            Some(rules.art()),
            &colors,
            types,
            &no_remaps,
            &no_cell_drawers,
            Some(&cell),
            existing,
            None,
        )
        .unwrap()
    };
    let key = |name: &str| ShpSpriteKey {
        house_color: crate::rules::house_colors::NO_REMAP,
        ..make_shp_key(name, 0)
    };
    let mut first = load(&["A"], None);
    let resident = *first.get(&key("A")).unwrap();
    let resident_unit_row = first.palettes.rows.get(&unit.to_rgba_bytes()).copied();
    let old_texture = first.palettes.texture.as_ref().unwrap().0.clone();
    let old_binding = first.pages[resident.page as usize]
        .texture
        .bind_group
        .clone();
    // Fill real atlas row storage before the late types arrive. B's distinct
    // Ares palette and CELL's terrain palette get high rows; OFF B correctly
    // reuses the already-resident Unit fallback row.
    let mut palettes = first.palettes.clone();
    for n in 0..300u16 {
        palettes
            .row(&Palette::from_rgb(|i| [i, n as u8, (n >> 8) as u8]))
            .unwrap();
    }
    first.append_sprites(&gpu.device, &gpu.queue, &batch, Vec::new(), palettes);
    assert_ne!(first.palettes.texture.as_ref().unwrap().0, old_texture);
    assert_ne!(
        first.pages[resident.page as usize].texture.bind_group,
        old_binding
    );
    let atlas = load(&["A", "B", "MISS", "CELL"], Some(first));
    assert_eq!(
        atlas.get(&key("A")).unwrap().source_palette,
        resident.source_palette
    );
    let red = Palette::from_rgb(|i| if i == 7 { [252, 0, 0] } else { [0; 3] });
    let green = Palette::from_rgb(|i| if i == 7 { [0, 252, 0] } else { [0; 3] });
    let b = if cfg!(feature = "ares") {
        &green
    } else {
        &unit
    };
    for (name, expected) in [
        ("A", &red),
        ("A_MAKE", &red),
        ("B", b),
        ("B_MAKE", b),
        ("MISS", &unit),
        ("CELL", &cell),
    ] {
        let row = atlas.get(&key(name)).unwrap().source_palette - 1;
        assert_eq!(
            atlas.palettes.colors[row as usize],
            expected.to_rgba_bytes(),
            "{name}"
        );
    }
    if !crate::rules::ARES_ENABLED {
        assert_eq!(
            atlas.get(&key("B")).unwrap().source_palette,
            resident_unit_row.expect("the Unit fallback was already resident") + 1,
            "OFF B reuses the existing Unit palette row"
        );
    }
    let high_type = if crate::rules::ARES_ENABLED {
        "B"
    } else {
        "CELL"
    };
    let high = *atlas.get(&key(high_type)).unwrap();
    assert!(
        high.source_palette > 256,
        "real sprite must reference a row beyond 255"
    );
    let reference = load(&[high_type], None);
    let low = *reference.get(&key(high_type)).unwrap();
    assert!(low.source_palette <= 256);
    // Exercise the real SpriteInstance u32 attribute, Batch shader and indexed
    // page binding, not just textureLoad at a manually selected high row.
    let draw = |atlas: &SpriteAtlas, entry: &ShpSpriteEntry| {
        let size = [2, 2];
        batch.write_camera(&gpu.queue, camera(size));
        let instance = SpriteInstance {
            uv_origin: entry.uv_origin,
            uv_size: entry.uv_size,
            source_palette: entry.source_palette,
            ..sprite([0.0; 2], entry.pixel_size, 0.0)
        };
        let mut pool = InstanceBufferPool::new();
        pool.upload_on_device(&gpu.device, &gpu.queue, "high_palette_row", &[instance]);
        let (buffer, count) = pool.get("high_palette_row").unwrap();
        let target = gpu.target(size, format);
        let color = target.create_view(&Default::default());
        let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
        let depth_view = depth.create_view(&Default::default());
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        clear(
            &mut encoder,
            &color,
            &depth_view,
            65535,
            wgpu::LoadOp::Clear(wgpu::Color::BLACK),
        );
        {
            let mut pass = crate::app::presentation::sidebar_render::begin_main_load_pass(
                &mut encoder,
                &color,
                &depth_view,
            );
            batch.draw_with_buffer_no_depth(
                &mut pass,
                &atlas.page(entry.page as usize).unwrap().texture,
                buffer,
                count,
            );
        }
        let read = gpu.read(&mut encoder, &target);
        gpu.finish(encoder, &[read], size).remove(0)
    };
    let high_pixels = draw(&atlas, &high);
    let b_pixels = draw(&atlas, atlas.get(&key("B")).unwrap());
    assert_eq!(
        high_pixels,
        draw(&reference, &low),
        "high row indexed draw must match the same sprite at a low row"
    );
    assert!(
        high_pixels
            .chunks_exact(4)
            .any(|pixel| pixel[..3] != [0, 0, 0]),
        "the draw must cover a nonblack pixel"
    );
    let resident_pixels = draw(&atlas, atlas.get(&key("A")).unwrap());
    assert!(
        resident_pixels
            .chunks_exact(4)
            .any(|pixel| pixel[..3] != [0, 0, 0])
    );
    let resident_reference = load(&["A"], None);
    assert_eq!(
        resident_pixels,
        draw(
            &resident_reference,
            resident_reference.get(&key("A")).unwrap()
        ),
        "old resident page must still bind its colors after texture replacement"
    );

    // The same destroyed-building animation can need two palette groups for
    // one house. Only the surviving animations drive these refreshes; no donor
    // building or extra-building request remains in the world.
    use crate::sim::anim_class::AnimRemap;
    use crate::sim::components::AnimClassSpawnDescriptor;
    use crate::sim::world::Simulation;
    use crate::util::fixed_math::SimFixed;
    let mut sim = Simulation::new();
    let house = sim.intern("Americans");
    let first_palette = sim.intern("FIRST");
    let second_palette = sim.intern("second");
    let wreck = sim.intern("WRECK");
    let spawn = |sim: &mut Simulation| {
        sim.spawn_anim_object(
            &rules,
            AnimClassSpawnDescriptor::new(
                wreck,
                10,
                10,
                SimFixed::from_num(0),
                SimFixed::from_num(0),
                0,
            ),
        )
        .unwrap()
    };
    let first_anim = spawn(&mut sim);
    sim.anims_mut_for_test().set_anim_remap(
        first_anim,
        AnimRemap::ExtraBuildingScheme {
            house,
            palette: first_palette,
        },
    );
    let refresh = |sim: &Simulation, previous| {
        let remaps = collect_anim_remap_base_keys(sim, &colors);
        build_sprite_atlas(
            &gpu.device,
            &gpu.queue,
            &batch,
            sim.entities(),
            &assets,
            &unit,
            "tem",
            "TEMPERATE",
            Some(&rules),
            Some(rules.art()),
            &colors,
            &[],
            &remaps,
            &no_cell_drawers,
            Some(&cell),
            previous,
            Some(sim.interner()),
        )
        .unwrap()
    };
    let first_remapped = refresh(&sim, Some(atlas));
    assert!(atlas_covers_anim_remaps(
        Some(&first_remapped),
        &sim,
        &colors
    ));
    let second_anim = spawn(&mut sim);
    sim.anims_mut_for_test().set_anim_remap(
        second_anim,
        if crate::rules::ARES_ENABLED {
            AnimRemap::ExtraBuildingScheme {
                house,
                palette: second_palette,
            }
        } else {
            AnimRemap::House(house)
        },
    );
    assert!(!atlas_covers_anim_remaps(
        Some(&first_remapped),
        &sim,
        &colors
    ));
    let saved = save_codec::serialize(&sim).unwrap();
    let restored: Simulation = save_codec::deserialize(&saved).unwrap();
    let remapped = refresh(&restored, Some(first_remapped));
    assert!(atlas_covers_anim_remaps(
        Some(&remapped),
        &restored,
        &colors
    ));
    for (id, expected, pixels) in [
        (
            first_anim,
            &red,
            draw(
                &resident_reference,
                resident_reference.get(&key("A")).unwrap(),
            ),
        ),
        (second_anim, b, b_pixels),
    ] {
        let anim = restored.anim(id).unwrap();
        let anim_key = ShpSpriteKey {
            palette_context: anim_remap_context(anim),
            type_id: "WRECK".into(),
            facing: 0,
            frame: 0,
            house_color: crate::rules::house_colors::NO_REMAP,
        };
        let entry = remapped.get(&anim_key).unwrap();
        assert_eq!(
            remapped.palettes.colors[(entry.source_palette - 1) as usize],
            expected.to_rgba_bytes()
        );
        assert_eq!(
            draw(&remapped, entry),
            pixels,
            "surviving animation keeps its own palette"
        );
    }
}
