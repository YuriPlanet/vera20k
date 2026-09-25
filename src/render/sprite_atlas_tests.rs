//! Tests for SHP sprite atlas key collection and deduplication.

use super::*;

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
        .insert(("PRIOR".to_string(), HouseColorIndex(1)));

    let restored =
        abort_sprite_atlas_refresh(Some(prior), "required refresh sprite failed".to_string())
            .expect("a refresh failure must retain the prior atlas");

    assert_eq!(restored.sprite_count(), 1);
    assert!(restored.get(&prior_key).is_some());
    assert_eq!(restored.make_frame_counts["PRIOR"], 3);
    assert_eq!(restored.active_anim_frame_counts["PRIOR"], 4);
    assert!(restored.unrenderable.contains(&make_shp_key("EMPTY", 0)));
    assert!(restored.covers(
        &HashSet::from([("PRIOR".to_string(), HouseColorIndex(1))]),
        &HashSet::new()
    ));
}

#[test]
#[should_panic(expected = "required initial sprite failed")]
fn required_initial_build_failure_remains_fail_fast() {
    let _ = abort_sprite_atlas_refresh(None, "required initial sprite failed".to_string());
}

#[test]
fn deploy_targets_get_the_same_keys_as_placed_structures() {
    let rules =
        crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[BuildingTypes]\n0=CAHOSP\n1=GACNST\n\n\
             [CAHOSP]\nCanBeOccupied=yes\n\n[GACNST]\nStrength=1000\n",
        ))
        .expect("rules");
    let mut occupied = HashSet::new();
    insert_object_keys(
        &mut occupied,
        "CAHOSP",
        EntityCategory::Structure,
        HouseColorIndex(2),
        Some(&rules),
    );
    let frames: HashSet<u16> = occupied.iter().map(|key| key.frame).collect();
    assert_eq!(frames, HashSet::from([0, 1, 2, 3]), "occupancy frame swap");

    let mut plain = HashSet::new();
    insert_object_keys(
        &mut plain,
        "GACNST",
        EntityCategory::Structure,
        HouseColorIndex(2),
        Some(&rules),
    );
    assert_eq!(
        plain,
        HashSet::from([{
            let mut key = make_shp_key("GACNST", 0);
            key.house_color = HouseColorIndex(2);
            key
        }])
    );
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
    let mut needed = HashSet::new();

    insert_anim_remap_frame_keys(&mut needed, "CRATE_SPARK", 3, &remaps);

    assert_eq!(needed.len(), 3);
    for frame in 0..3 {
        assert!(needed.contains(&ShpSpriteKey {
            palette_context: crate::render::sprite_atlas::ShpPaletteContext::SelectedScheme,
            type_id: "CRATE_SPARK".to_string(),
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
    rules.art_registry = art;
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
            Some(&rules.art_registry),
            &effects,
            &cell_drawers,
        ),
        SpritePaletteChoice::CellIso,
        "cell-drawer palette authority overrides ordinary Anim.PAL/AltPalette selection"
    );

    let candidates = effect_anim_shp_candidates(
        "CUSTOM_TILE_ANIM",
        Some(&rules.art_registry),
        "tem",
        "TEMPERATE",
    );
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
fn palette_context_refresh_coverage_preserves_loaded_remaps_and_detects_missing_ones() {
    let pair = HashSet::from([("CRATE_SPARK".to_string(), HouseColorIndex(3))]);
    let none = HashSet::new();
    let mut atlas = SpriteAtlas::new(Vec::new(), HashMap::new());
    atlas.covered_objects.extend(pair.iter().cloned());
    assert!(atlas.covers(&pair, &none));
    assert!(
        !atlas.covers(&none, &pair),
        "a coincident entity must not suppress animation preload"
    );
    atlas.covered_anim_remaps.extend(pair.iter().cloned());
    assert!(
        atlas.covers(&pair, &pair),
        "loaded animation must not force repeated atlas uploads"
    );
    atlas.covered_objects.clear();
    assert!(atlas.covers(&none, &pair));
    assert!(!atlas.covers(&pair, &none));
}

#[test]
fn a_collected_pair_is_covered_even_when_nothing_of_it_is_drawable() {
    // A type whose frame 0 is empty or whose SHP is missing has no atlas entry
    // at all. Coverage by collected pairs keeps it from forcing a refresh on
    // every spawn, death or Limbo, as a frame-0 entry probe would.
    let pair = HashSet::from([("NOSHAPE".to_string(), HouseColorIndex(1))]);
    let mut atlas = SpriteAtlas::new(Vec::new(), HashMap::new());
    atlas.unrenderable.insert(make_shp_key("NOSHAPE", 0));
    assert!(!atlas.covers(&pair, &HashSet::new()));
    atlas.covered_objects.extend(pair.iter().cloned());
    assert!(atlas.covers(&pair, &HashSet::new()));
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
    let indices = growth.source_indices.create_view(&Default::default());
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
    let mut assets = AssetManager::new(&root).expect("retail archives");
    let theater_name = scenario.map.header.theater.clone();
    let theater =
        crate::map::theater::load_theater(&mut assets, &theater_name).expect("theater loads");
    let rules = &scenario.runtime.resources.rules;
    let art = &rules.art_registry;

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
