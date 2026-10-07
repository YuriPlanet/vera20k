//! Tests for SHP sprite atlas key collection and deduplication.

use super::*;
use crate::assets::asset_manager::MediaArchiveMode;

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
            let rendered = render_shp_sprite(
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
            assert_eq!(rendered.rgba.len(), (size[0] * size[1] * 4) as usize);
            let colored: Vec<_> = rendered
                .rgba
                .chunks_exact(4)
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
                    &rendered.rgba, raw,
                    "raw format 1 and RLE format 3 must agree"
                );
            } else {
                raw_rgba = Some(rendered.rgba);
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

#[test]
fn test_shp_sprite_key_hash_equality() {
    let hc = HouseColorIndex::default();
    let k1 = ShpSpriteKey {
        palette_context: crate::render::sprite_atlas::ShpPaletteContext::Legacy,
        type_id: "E1".into(),
        facing: 64,
        frame: 10,
        house_color: hc,
    };
    let k2 = ShpSpriteKey {
        palette_context: crate::render::sprite_atlas::ShpPaletteContext::Legacy,
        type_id: "E1".into(),
        facing: 64,
        frame: 10,
        house_color: hc,
    };
    let k3 = ShpSpriteKey {
        palette_context: crate::render::sprite_atlas::ShpPaletteContext::Legacy,
        type_id: "E1".into(),
        facing: 64,
        frame: 11,
        house_color: hc,
    };
    assert_eq!(k1, k2);
    assert_ne!(k1, k3);
    let mut set: HashSet<ShpSpriteKey> = HashSet::new();
    set.insert(k1);
    set.insert(k2);
    set.insert(k3);
    assert_eq!(set.len(), 2);
}

#[test]
fn test_empty_world_returns_none() {
    let needed: HashSet<ShpSpriteKey> = HashSet::new();
    assert!(needed.is_empty());
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
        page,
    }
}

#[test]
fn refresh_failure_returns_the_prior_atlas_unchanged() {
    let prior_key = make_shp_key("PRIOR", 0);
    let mut prior = SpriteAtlas::new(
        Vec::new(),
        HashMap::from([(prior_key.clone(), test_entry(0))]),
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
    entity.category = category;
    entity.is_voxel = false;
    let mut store = crate::sim::entity_store::EntityStore::new();
    store.insert(entity);
    (store, crate::sim::intern::test_interner())
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
    insert_object_keys(
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
        let frames: HashSet<_> = needed.iter().map(|key| key.frame).collect();
        assert_eq!(frames, expected, "{row}");
        assert!(needed.iter().all(|key| key.frame < 2), "{row}");
    }
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
    assert!(render_shp_frame(&source, &palette, false, &key, Some(&rules)).is_none());
    // Preserve the existing generic class adapter; this does not establish
    // original invalid-frame shape access or full Infantry drawing parity.
    key.type_id = "LEGACY".to_string();
    assert!(render_shp_frame(&source, &palette, false, &key, Some(&rules)).is_some());
}

#[test]
fn building_animation_frames_render_from_the_file_they_were_counted_in() {
    // [YAGRND_B] authors no NewTheater=, and no TEMPERATE archive holds
    // YAGRND_B.SHP: AnimTypeClass forces the second letter to G and loads
    // YGGRND_B.SHP. The object naming never reaches that file, so rendering
    // must reuse the file the frame count came from.
    let directory = StoredFrameTestDirectory::new();
    directory.write_raw_and_rle_frames("YGGRND_B.SHP", [20, 10], [2, 1, 6, 4]);
    let assets = AssetManager::from_loose_root_for_test(&directory.0);
    let art = ArtRegistry::from_ini(&crate::rules::ini_parser::IniFile::from_str(
        "[YAGRND_B]\nImage=YAGRND_B\nLoopEnd=1\n",
    ));

    let (count, file) =
        scan_building_anim_frame_count(&assets, &art, "YAGRND_B", 1, "tem", "TEMPERATE")
            .expect("the anim naming finds the forced-G file");
    assert_eq!(file, "YGGRND_B.SHP");
    assert_eq!(count, 1, "two stored frames: one body frame, one shadow");

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
    let sprite = render_shp_frame(&source, &palette, true, &key, None).expect("frame 0 has pixels");
    assert_eq!([sprite.width, sprite.height], [6, 4]);
}

#[test]
fn test_key_collection_deduplicates() {
    let hc = HouseColorIndex::default();
    let mut needed: HashSet<ShpSpriteKey> = HashSet::new();
    // Two identical keys + one different facing.
    needed.insert(make_shp_key("E1", 64));
    needed.insert(make_shp_key("E1", 64)); // duplicate
    needed.insert(make_shp_key("E1", 128));
    assert_eq!(needed.len(), 2);
    let _ = hc;
}

#[test]
fn test_structure_facing_collapse() {
    let hc = HouseColorIndex::default();
    // Structures always use facing 0 (no rotation).
    let mut needed: HashSet<ShpSpriteKey> = HashSet::new();
    for facing_raw in [64u8, 192u8] {
        let eff = 0u8; // structures collapse to facing 0
        needed.insert(ShpSpriteKey {
            palette_context: crate::render::sprite_atlas::ShpPaletteContext::Legacy,
            type_id: "GAPOWR".to_string(),
            facing: eff,
            frame: 0,
            house_color: hc,
        });
        let _ = facing_raw;
    }
    assert_eq!(needed.len(), 1);
}

#[test]
fn test_different_houses_create_separate_keys() {
    let hc0 = HouseColorIndex(0); // [Colors] entry 0 (LightGold)
    let hc1 = HouseColorIndex(1); // [Colors] entry 1 (Gold)
    let k1 = ShpSpriteKey {
        palette_context: crate::render::sprite_atlas::ShpPaletteContext::Legacy,
        type_id: "E1".into(),
        facing: 64,
        frame: 10,
        house_color: hc0,
    };
    let k2 = ShpSpriteKey {
        palette_context: crate::render::sprite_atlas::ShpPaletteContext::Legacy,
        type_id: "E1".into(),
        facing: 64,
        frame: 10,
        house_color: hc1,
    };
    assert_ne!(
        k1, k2,
        "Same type+facing but different house should be distinct keys"
    );
    let mut set: HashSet<ShpSpriteKey> = HashSet::new();
    set.insert(k1);
    set.insert(k2);
    assert_eq!(set.len(), 2);
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
fn sprite_palette_choice_leaves_non_effect_and_unknown_art_on_the_unit_palette() {
    let ini = crate::rules::ini_parser::IniFile::from_str("[GAPOWR]\nRemapable=yes\n");
    let art = ArtRegistry::from_ini(&ini);
    let effects: HashSet<String> = HashSet::new();
    let cell_drawers = HashSet::new();

    // A structure that is not a world effect keeps the unit palette.
    assert_eq!(
        sprite_palette_choice("GAPOWR", Some(&art), &effects, &cell_drawers),
        SpritePaletteChoice::Unit
    );
    // No art registry at all must not change the previous name-set behaviour.
    assert_eq!(
        sprite_palette_choice("GAPOWR", None, &effects, &cell_drawers),
        SpritePaletteChoice::Unit
    );
    let mut with_effect: HashSet<String> = HashSet::new();
    with_effect.insert("FBALL1".to_string());
    assert_eq!(
        sprite_palette_choice("FBALL1", None, &with_effect, &cell_drawers),
        SpritePaletteChoice::Anim
    );
}

#[test]
fn declared_special_animation_frames_are_all_preloaded() {
    let mut needed = HashSet::new();
    insert_building_anim_frame_keys(
        &mut needed,
        "GAREFNOR",
        3,
        HouseColorIndex(2),
        ShpPaletteContext::SelectedScheme,
    );

    for frame in 0..3 {
        assert!(needed.contains(&ShpSpriteKey {
            palette_context: crate::render::sprite_atlas::ShpPaletteContext::SelectedScheme,
            type_id: "GAREFNOR".to_string(),
            facing: 0,
            frame,
            house_color: HouseColorIndex(2),
        }));
    }
    assert_eq!(needed.len(), 3);
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
fn cell_anim_remap_registration_covers_every_bound_frame_for_its_color() {
    let remaps = HashSet::from([
        ("CRATE_SPARK".to_string(), HouseColorIndex(1)),
        ("OTHER".to_string(), HouseColorIndex(2)),
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
                forced_uses_75: false,
                translucency_detail_level: warpout.translucency_detail_level,
                game_detail_level: 2,
                translucent_ramp: warpout.translucent,
                current_frame: frame,
                frame_count: i32::from(frame_count),
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

fn make_raw_test_shp(frame_count: u16) -> Vec<u8> {
    let headers_end = 8usize + usize::from(frame_count) * 24;
    let mut data = Vec::with_capacity(headers_end + usize::from(frame_count));
    data.extend_from_slice(&0u16.to_le_bytes());
    data.extend_from_slice(&1u16.to_le_bytes());
    data.extend_from_slice(&1u16.to_le_bytes());
    data.extend_from_slice(&frame_count.to_le_bytes());
    for frame in 0..frame_count {
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&1u16.to_le_bytes());
        data.extend_from_slice(&1u16.to_le_bytes());
        data.extend_from_slice(&[0u8; 12]);
        let offset = u32::try_from(headers_end + usize::from(frame)).unwrap();
        data.extend_from_slice(&offset.to_le_bytes());
    }
    data.extend(std::iter::repeat_n(1u8, usize::from(frame_count)));
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
    let tem_only = HashMap::from([("CUSTOM_TILE_ANIM.TEM".to_string(), make_raw_test_shp(4))]);
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

/// Real loose SHP decoding, palette application and production atlas page copy.
pub(crate) fn native_palette_probe_page() -> (Vec<u8>, Vec<u8>, [u32; 2]) {
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
    let palette = Palette {
        colors: std::array::from_fn(|i| {
            let i = i as u8;
            crate::assets::pal_file::Color {
                r: i,
                g: i.wrapping_mul(73),
                b: 255 - i,
                a: if i == 0 { 0 } else { 255 },
            }
        }),
    };
    let key = ShpSpriteKey {
        palette_context: ShpPaletteContext::SelectedScheme,
        type_id: "PALPROBE".into(),
        facing: 0,
        frame: 0,
        house_color: crate::rules::house_colors::NO_REMAP,
    };
    let sprite = render_shp_sprite(
        &assets,
        &palette,
        true,
        &key,
        "tem",
        "TEMPERATE",
        None,
        None,
    )
    .unwrap();
    assert_eq!(sprite.indices, (0..=255u8).collect::<Vec<_>>());
    let size = [262, 3];
    let mut rgba = vec![0; size[0] * size[1] * 4];
    let mut indices = vec![0; size[0] * size[1]];
    blit_sprite_pixels(&sprite, [3, 1], size[0] as u32, &mut rgba, &mut indices);
    assert_eq!(&indices[265..521], (0..=255u8).collect::<Vec<_>>());
    (rgba, indices, size.map(|v| v as u32))
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
            sprite_palette_for_key(&key, Some(&art), &effects, &cells),
            palette
        );
    }
    assert_eq!(keys.len(), 3);
    let (rgba, indices, _) = native_palette_probe_page();
    assert_eq!(&indices[505..520], (240..=254u8).collect::<Vec<_>>());
    assert_eq!(rgba[505 * 4], 240);
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
    let mut atlas = SpriteAtlas::new(Vec::new(), HashMap::new());
    let colors = HouseColorMap::from([("Americans".to_string(), HouseColorIndex(3))]);
    assert!(atlas_covers_anim_remaps(Some(&atlas), &sim, &colors));
    let house = sim.interner.intern("Americans");
    let type_id = sim.interner.intern("DEPLOY");
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
    assert!(atlas_covers_anim_remaps(Some(&atlas), &sim, &colors));
    assert!(sim.set_deploy_anim_remap(deployed, house));
    assert!(sim.set_cell_anim_draw_authority(cell, Some(HouseColorIndex(5)), 0));
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
        anim_remap_color(sim.anim(deployed).unwrap(), &sim.interner, &colors),
        Some(HouseColorIndex(3))
    );
    let remaps = collect_anim_remap_base_keys(&sim, &colors);
    assert_eq!(
        remaps,
        HashSet::from([
            ("DEPLOY".to_string(), HouseColorIndex(3)),
            ("DEPLOY".to_string(), HouseColorIndex(5)),
        ])
    );
    let mut keys = HashSet::new();
    let effects = EffectRegistry {
        frames: HashMap::from([("DEPLOY".to_string(), ("DEPLOY".to_string(), 3))]),
        type_ids: HashSet::from(["DEPLOY".to_string()]),
    };
    insert_anim_remap_frame_keys(&mut keys, &effects, &remaps);
    assert_eq!(keys.len(), 6);
    assert!(keys.iter().all(|key| {
        key.palette_context == ShpPaletteContext::SelectedScheme
            && key.frame < 3
            && matches!(key.house_color, HouseColorIndex(3 | 5))
    }));
    atlas.covered_anim_remaps.extend(remaps.iter().cloned());
    assert!(
        atlas_covers_anim_remaps(Some(&atlas), &sim, &colors),
        "covered live animations do not request another upload"
    );

    let bytes = bincode::serialize(&sim).expect("save frozen animation remaps");
    let restored: Simulation = bincode::deserialize(&bytes).expect("restore animation remaps");
    assert_eq!(collect_anim_remap_base_keys(&restored, &colors), remaps);
    assert_eq!(
        restored.anim(deployed).unwrap().remap(),
        Some(AnimRemap::House(house))
    );
}

#[test]
fn object_and_remap_coverage_are_separate_and_follow_the_world() {
    let (world, interner) = one_object_world("CRATE_SPARK", EntityCategory::Structure);
    let colors = HouseColorMap::from([("Americans".to_string(), HouseColorIndex(3))]);
    let remap = HashSet::from([("CRATE_SPARK".to_string(), HouseColorIndex(3))]);
    let none = HashSet::new();
    let covers = |atlas: Option<&SpriteAtlas>, remaps| {
        atlas_covers_world(atlas, &world, &colors, &[], remaps, Some(&interner))
    };
    assert!(
        !covers(None, &none),
        "no atlas yet, and the world draws a sprite"
    );

    let mut atlas = SpriteAtlas::new(Vec::new(), HashMap::new());
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
    let mut atlas = SpriteAtlas::new(Vec::new(), HashMap::new());
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
    let sprite = |type_id: &str, index: u8, [width, height]: [u32; 2]| RenderedShpSprite {
        key: make_shp_key(type_id, 0),
        rgba: vec![index; (width * height * 4) as usize],
        indices: vec![index; (width * height) as usize],
        width,
        height,
        offset_x: 0.0,
        offset_y: 0.0,
        canvas_rect: [0.0, 0.0, width as f32, height as f32],
        extended: false,
    };
    let mut atlas = pack_sprites(
        &gpu.device,
        &gpu.queue,
        &batch,
        &[sprite("RESIDENT", 7, [3, 2])],
    );
    let resident = *atlas.get(&make_shp_key("RESIDENT", 0)).unwrap();

    atlas.append_sprites(
        &gpu.device,
        &gpu.queue,
        &batch,
        vec![sprite("WIDE", 11, [4, 3]), sprite("TALL", 9, [2, 5])],
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
    let growth = atlas.growth.as_ref().expect("growth page");
    let page = &atlas.pages[growth.page].texture;
    let indices = atlas.growth_indices[&growth.page].create_view(&Default::default());
    for (type_id, index, size, origin) in [
        // Tallest first: TALL opens the shelf, WIDE follows after 1px padding.
        ("TALL", 9u8, [2u32, 5u32], [0u32, 0u32]),
        ("WIDE", 11, [4, 3], [3, 0]),
    ] {
        let entry = atlas.get(&make_shp_key(type_id, 0)).unwrap();
        assert_eq!(usize::from(entry.page), growth.page);
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
