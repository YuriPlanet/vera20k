//! Tests for art.ini parsing and art-resolution layering.

use super::*;

#[test]
fn native_discharge_art_reader_matches_original_signed_independent_keys() {
    // Original InfantryType ART reads 5246BE..52473A call ReadInt5276D0
    // independently over E40/E44/E48/E4C. Replay every cached-input row
    // through the production read owner, then fresh file/registry inputs.
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/infantry_discharge_rules.json",
    ))
    .unwrap();
    let rows = corpus["cases"].as_array().unwrap();
    assert_eq!(rows.len(), 150);
    let mut compared = 0;
    for row in rows {
        let layers = row["layers"].as_array().unwrap();
        let section = row["section"].as_str().unwrap();
        let mut retained: [i32; 4] = serde_json::from_value(row["before"].clone()).unwrap();
        for (index, layer) in layers.iter().enumerate() {
            // The original oracle supplies cached values. `set` preserves
            // present-empty/raw text, unlike physical INI loading, which
            // omits empty entries and trims values before these reads.
            let mut cached = IniSection::new(section.to_owned());
            for (key, value) in layer.as_object().unwrap() {
                if let Some(raw) = value.as_str() {
                    cached.set(key, raw);
                }
            }
            read_infantry_discharge_frames(&cached, &mut retained);
            let expected: [i32; 4] =
                serde_json::from_value(row["after_layers"][index].clone()).unwrap();
            assert_eq!(
                retained, expected,
                "original row {} pass{index}",
                row["case"]
            );
        }
        if !row["initial"].is_null() || layers.len() != 1 {
            continue;
        }
        // A harmless unknown key keeps all-absent/empty sections in the
        // physical loader; it is never a discharge read input.
        let mut text = format!("[{section}]\nFixtureOnly=1\n");
        for (key, value) in layers[0].as_object().unwrap() {
            if let Some(raw) = value.as_str() {
                text.push_str(&format!("{key}={raw}\n"));
            }
        }
        let registry = ArtRegistry::from_ini(&IniFile::from_str(&text));
        let entry = registry.get(section).unwrap();
        let actual = [
            entry.fire_up,
            entry.fire_prone,
            entry.secondary_fire,
            entry.secondary_prone,
        ];
        let expected: [i32; 4] = serde_json::from_value(row["after"].clone()).unwrap();
        assert_eq!(actual, expected, "original row {}", row["case"]);
        compared += 1;
    }
    assert_eq!(compared, 75);
}

#[test]
fn animation_layer_matches_original_name_reader() {
    let rows: Vec<serde_json::Value> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/anim_layer_rules.json",
    ))
    .unwrap();
    assert_eq!(rows.len(), 19);
    for row in rows {
        let mut text = "[A]\nLoopCount=1\n".to_owned();
        if let Some(raw) = row["raw"].as_str() {
            text.push_str(&format!("Layer={raw}\n"));
        }
        let ini = IniFile::from_str(&text);
        let actual = match AnimLayer::from_ini(ini.section("A").unwrap()) {
            AnimLayer::Ground => 2,
            AnimLayer::Top => 4,
            AnimLayer::Other(value) => value,
        };
        assert_eq!(actual, row["layer"].as_i64().unwrap() as i32, "{row}");
    }
}

#[test]
fn original_1080_building_slot_power_reads_preserve_native_defaults() {
    #[derive(serde::Deserialize)]
    struct Row {
        slot: usize,
        flag: usize,
        name: String,
        raw: Option<String>,
        initial: bool,
        output: bool,
    }
    let rows: Vec<Row> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/building_slot_power_rules.json",
    ))
    .unwrap();
    assert_eq!(rows.len(), 1080);
    for (index, row) in rows.into_iter().enumerate() {
        let mut section = IniSection::new("B".into());
        if let Some(raw) = row.raw {
            section.set(&row.name, &raw);
        }
        let initial = BuildingAnimPowerFlags {
            powered: row.initial,
            powered_light: row.initial,
            powered_effect: row.initial,
            powered_special: row.initial,
        };
        let mut records = [initial; 21];
        read_building_anim_power(&section, &mut records);
        let actual = records[row.slot];
        assert_eq!(
            [
                actual.powered,
                actual.powered_light,
                actual.powered_effect,
                actual.powered_special
            ][row.flag],
            row.output,
            "native row {index}"
        );
    }
    let registry = ArtRegistry::from_ini(&IniFile::from_str(
        "[B]\nTurretAnimPowered=no\nActiveAnimPowered=no\n",
    ));
    let records = &registry.get("B").unwrap().building_anim_power;
    assert_eq!(records[9], BuildingAnimPowerFlags::default());
    assert!(
        !records[3].powered,
        "power metadata exists without an animation name"
    );
}

#[test]
fn test_hardcoded_new_theater_prefixes() {
    let reg: ArtRegistry = ArtRegistry::empty();
    assert!(reg.should_use_new_theater("GAPOWR"));
    assert!(reg.should_use_new_theater("GTPOWR"));
    assert!(reg.should_use_new_theater("NAHAND"));
    assert!(reg.should_use_new_theater("NTHAND"));
    assert!(reg.should_use_new_theater("CAOUTP"));
    assert!(reg.should_use_new_theater("CTOUTP"));
    assert!(!reg.should_use_new_theater("MTNK"));
    assert!(!reg.should_use_new_theater("E1"));
    assert!(!reg.should_use_new_theater("HTNK"));
}

#[test]
fn test_object_shp_candidates_no_new_theater() {
    let reg: ArtRegistry = ArtRegistry::empty();
    let candidates: Vec<String> = object_shp_candidates(Some(&reg), "E1", "tem", "TEMPERATE");
    assert_eq!(candidates, vec!["E1.SHP", "E1.TEM"]);
}

#[test]
fn test_object_shp_candidates_with_new_theater() {
    let reg: ArtRegistry = ArtRegistry::empty();
    let candidates: Vec<String> = object_shp_candidates(Some(&reg), "GAPOWR", "tem", "TEMPERATE");
    assert_eq!(
        candidates,
        vec![
            "GTPOWR.SHP",
            "GTPOWR.TEM",
            "GGPOWR.SHP",
            "GGPOWR.TEM",
            "GAPOWR.SHP",
            "GAPOWR.TEM",
        ]
    );
}

#[test]
fn test_object_shp_candidates_dedupe_when_substitution_matches_original() {
    let reg: ArtRegistry = ArtRegistry::empty();
    let candidates: Vec<String> = object_shp_candidates(Some(&reg), "GAPOWR", "sno", "SNOW");
    assert_eq!(
        candidates,
        vec!["GAPOWR.SHP", "GAPOWR.SNO", "GGPOWR.SHP", "GGPOWR.SNO",]
    );
}

#[test]
fn gsi_04_05_hidden_absent_occupy_height_inherits_resolved_height() {
    let ini: IniFile = IniFile::from_str(
        "[GAREFN]\nCanHideThings=no\nOccupyHeight=4\n\n[GAPOWR]\nHeight=3\n\n[NAPOWR]\nFixtureOnly=1\n",
    );
    let reg: ArtRegistry = ArtRegistry::from_ini(&ini);

    assert!(!reg.can_hide_things("GAREFN"));
    assert_eq!(reg.occupy_height("GAREFN"), 4);
    assert!(reg.can_hide_things("GAPOWR"));
    assert_eq!(reg.occupy_height("GAPOWR"), 3);
    assert!(reg.can_hide_things("NAPOWR"));
    // Neither key is authored, so both resolved fields retain constructor 2.
    assert_eq!(reg.occupy_height("NAPOWR"), 2);
    assert!(reg.can_hide_things("MISSING"));
    assert_eq!(reg.occupy_height("MISSING"), 2);
}

#[test]
fn test_resolve_effective_image_id_chain() {
    let ini: IniFile =
        IniFile::from_str("[NACNST]\nImage=CIVNC\n\n[E1]\nFixtureOnly=1\n\n[MTNK]\nImage=MTNK\n");
    let reg: ArtRegistry = ArtRegistry::from_ini(&ini);

    assert_eq!(reg.resolve_effective_image_id("NACNST", "NACNST"), "CIVNC");
    assert_eq!(reg.resolve_effective_image_id("E1", "E1"), "E1");
    assert_eq!(reg.resolve_effective_image_id("MTNK", "MTNK"), "MTNK");
    assert_eq!(reg.resolve_effective_image_id("UNKNOWN", "FOO"), "FOO");
}

#[test]
fn test_resolve_object_art_exposes_exact_layers() {
    let ini: IniFile = IniFile::from_str("[NACNST]\nImage=CIVNC\nBibShape=NACNSTBB\n");
    let reg: ArtRegistry = ArtRegistry::from_ini(&ini);

    let resolved: ResolvedObjectArt<'_> = reg.resolve_object_art("NACNST", "NACNST");
    assert_eq!(resolved.base_art_id, "NACNST");
    assert_eq!(resolved.image_id, "CIVNC");
    assert_eq!(resolved.metadata_section_id, "NACNST");
    assert_eq!(
        resolved.entry.and_then(|e| e.bib_shape.as_deref()),
        Some("NACNSTBB")
    );
}

#[test]
fn test_resolve_metadata_entry_prefers_rules_image_section() {
    let ini: IniFile = IniFile::from_str(
        "[GAAIRC]\nBibShape=GAAIRCBB\nActiveAnim=GAAIRC_A\n\n[GAAIRC_A]\nLoopStart=0\nLoopEnd=4\nLoopCount=-1\n",
    );
    let reg: ArtRegistry = ArtRegistry::from_ini(&ini);

    let entry: &ArtEntry = reg
        .resolve_metadata_entry("AMRADR", "GAAIRC")
        .expect("AMRADR should use GAAIRC art metadata");
    assert_eq!(entry.bib_shape.as_deref(), Some("GAAIRCBB"));
    let active = entry
        .building_anims
        .iter()
        .find(|config| config.native_slot == 3)
        .unwrap();
    assert_eq!(active.anim_type, "GAAIRC_A");
}

#[test]
fn declared_cameo_comes_from_the_rules_image_section_before_the_type_section() {
    // `[MTNK]` keeps RA2's Apocalypse cameo while its rules `Image=GTNK`
    // points at the Grizzly section: native reads `[GTNK].Cameo`.
    let ini: IniFile = IniFile::from_str(
        "[MTNK]\nCameo=MTNKICON\nAltCameo=MTNKUICO\n\n\
         [GTNK]\nCameo=GTNKICON\nAltCameo=GTNKUICO\n\n\
         [SREF]\nCameo=SREFICON\n\n\
         [BFRT]\nImage=SREF\nCameo=BFRTICON\n\n\
         [E1]\nCameo=E1CAMEO\n\n\
         [NOCAMEO]\nAltCameo=NOCAMEOALT\n",
    );
    let reg: ArtRegistry = ArtRegistry::from_ini(&ini);

    assert_eq!(reg.resolve_declared_cameo_id("MTNK", "GTNK"), "GTNKICON");
    assert_eq!(reg.resolve_declared_cameo_id("MTNK", "MTNK"), "MTNKICON");
    // The art section's own `Image=` redirect never selects the cameo section.
    assert_eq!(reg.resolve_declared_cameo_id("BFRT", "BFRT"), "BFRTICON");
    assert_eq!(reg.resolve_declared_cameo_id("E1", "E1"), "E1CAMEO");
    // Savegame-reload fallback: the image section has no `Cameo=`, so the
    // type-ID section is consulted.
    assert_eq!(reg.resolve_declared_cameo_id("E1", "NOCAMEO"), "E1CAMEO");
    // `AltCameo=` is a separate field, never a substitute for `Cameo=`.
    assert_eq!(
        reg.resolve_declared_cameo_id("NOCAMEO", "NOCAMEO"),
        "NOCAMEO"
    );
    assert_eq!(reg.resolve_declared_cameo_id("UNKNOWN", "FOO"), "FOO");
}

#[test]
#[ignore = "slow; run when changing what it covers"]
fn retail_sidebar_cameos_follow_the_rules_image_section() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let cameo = |type_id: &str| {
        let rules_image = rules
            .object(type_id)
            .map(|object| object.image.clone())
            .unwrap_or_else(|| type_id.to_string());
        rules.art().resolve_declared_cameo_id(type_id, &rules_image)
    };
    assert_eq!(cameo("MTNK"), "GTNKICON");
    assert_eq!(cameo("HTNK"), "HTNKICON");
    assert_eq!(cameo("BFRT"), "BFRTICON");
    assert_eq!(cameo("YENGINEER"), "ENGNICON");
    assert_eq!(cameo("GAPILE"), "BRRKICON");
}

#[test]
fn test_parse_building_anims() {
    let ini: IniFile = IniFile::from_str(
        "[CAOILD]\nActiveAnim=CAOILD_A\nActiveAnimDamaged=CAOILD_AD\nActiveAnimGarrisoned=CAOILD_AG\nActiveAnimYSort=362\nActiveAnimTwo=CAOILD_F\nActiveAnimTwoZAdjust=-50\n",
    );
    let reg: ArtRegistry = ArtRegistry::from_ini(&ini);
    let entry: &ArtEntry = reg.get("CAOILD").expect("CAOILD exists");
    let active = entry
        .building_anims
        .iter()
        .find(|config| config.native_slot == 3)
        .unwrap();
    let flag = entry
        .building_anims
        .iter()
        .find(|config| config.native_slot == 4)
        .unwrap();
    assert_eq!(active.anim_type, "CAOILD_A");
    assert_eq!(
        active
            .damaged_variant
            .as_ref()
            .map(|v| v.anim_type.as_str()),
        Some("CAOILD_AD")
    );
    assert_eq!(
        active
            .garrisoned_variant
            .as_ref()
            .map(|v| v.anim_type.as_str()),
        Some("CAOILD_AG")
    );
    assert_eq!(active.y_sort, 362);
    assert_eq!(active.z_adjust, 0);

    assert_eq!(flag.anim_type, "CAOILD_F");
    assert_eq!(flag.y_sort, 0);
    assert_eq!(flag.z_adjust, -50);
}

#[test]
fn unnamed_power_up_slots_preserve_offsets_for_install_supplied_images() {
    let ini = IniFile::from_str(
        "[HOST]\nPowerUp1LocXX=-24\nPowerUp1LocYY=-12\n\
         PowerUp1LocZZ=-42\nPowerUp1YSort=50\nPowerUp2LocXX=17\n",
    );
    let art = ArtRegistry::from_ini(&ini);
    let entry = art.get("HOST").unwrap();
    assert_eq!(entry.building_anims.len(), 3);
    for slot in 0..3 {
        let config = entry
            .building_anims
            .iter()
            .find(|config| config.native_slot == slot)
            .unwrap();
        assert!(config.anim_type.is_empty());
        assert!(config.damaged_variant.is_none());
        assert!(config.garrisoned_variant.is_none());
    }
    let first = &entry.building_anims[0];
    assert_eq!(
        (first.x, first.y, first.z_adjust, first.y_sort),
        (-24, -12, -42, 50)
    );
    assert_eq!(entry.building_anims[1].x, 17);
    let third = &entry.building_anims[2];
    assert_eq!(
        (third.x, third.y, third.z_adjust, third.y_sort),
        (0, 0, 0, 0)
    );
    assert!(art.building_anim_roots().is_empty());
}

#[test]
fn power_up_config_uses_the_retained_record_without_an_art_section() {
    use crate::rules::building_upgrade::BuildingPowerUpArt;
    let art = ArtRegistry::empty();
    let config = art.building_power_up_config(2, &BuildingPowerUpArt::default());
    assert_eq!(config.native_slot, 2);
    assert!(config.anim_type.is_empty());
    assert!(config.damaged_variant.is_none());
    assert!(config.garrisoned_variant.is_none());
    assert_eq!(
        (config.x, config.y, config.z_adjust, config.y_sort),
        (0, 0, 0, 0)
    );
}

#[test]
fn parses_all_four_special_anim_suffixes_in_document_order() {
    // The tank bunker's walls are four SpecialAnim slots (up pair + down pair);
    // SpecialAnimFour must parse — it was previously dropped.
    let ini: IniFile = IniFile::from_str(
        "[NATBNK]\n\
         SpecialAnim=NATBNK_A\n\
         SpecialAnimTwo=NATBNK_A2\n\
         SpecialAnimThree=NATBNK_B\n\
         SpecialAnimFour=NATBNK_B2\n\
         SpecialAnimFourDamaged=NATBNK_B2D\n",
    );
    let reg: ArtRegistry = ArtRegistry::from_ini(&ini);
    let entry: &ArtEntry = reg.get("NATBNK").expect("NATBNK exists");

    let specials: Vec<&str> = entry
        .building_anims
        .iter()
        .filter(|a| matches!(a.kind, BuildingAnimKind::Special) && a.native_slot >= 3)
        .map(|a| a.anim_type.as_str())
        .collect();
    assert_eq!(
        specials,
        vec!["NATBNK_A", "NATBNK_A2", "NATBNK_B", "NATBNK_B2"],
        "all four SpecialAnim suffixes parse in document order"
    );

    // Only the base key is primary; the suffixed slots are not.
    let primaries: Vec<bool> = entry
        .building_anims
        .iter()
        .filter(|a| matches!(a.kind, BuildingAnimKind::Special) && a.native_slot >= 3)
        .map(|a| a.is_primary)
        .collect();
    assert_eq!(primaries, vec![true, false, false, false]);

    // SpecialAnimFour's …Damaged variant rides along for the health gate.
    let four = entry
        .building_anims
        .iter()
        .filter(|a| matches!(a.kind, BuildingAnimKind::Special) && a.native_slot >= 3)
        .nth(3)
        .unwrap();
    assert_eq!(
        four.damaged_variant.as_ref().map(|v| v.anim_type.as_str()),
        Some("NATBNK_B2D")
    );
}

#[test]
fn damaged_active_anim_variant_uses_own_frame_metadata() {
    let ini: IniFile = IniFile::from_str(
        "[CASEAT02]\n\
         ActiveAnim=CASEAT02_A\n\
         ActiveAnimDamaged=CASEAT02_AD\n\
         \n\
         [CASEAT02_A]\n\
         Start=0\n\
         LoopStart=0\n\
         LoopEnd=20\n\
         LoopCount=-1\n\
         Rate=150\n\
         \n\
         [CASEAT02_AD]\n\
         Image=CASEAT02_A\n\
         Start=21\n\
         LoopStart=21\n\
         LoopEnd=39\n\
         LoopCount=-1\n\
         Rate=150\n",
    );
    let reg: ArtRegistry = ArtRegistry::from_ini(&ini);
    let entry: &ArtEntry = reg.get("CASEAT02").expect("CASEAT02 exists");
    let anim = entry
        .building_anims
        .iter()
        .find(|config| config.native_slot == 3)
        .unwrap();
    let damaged = anim.damaged_variant.as_ref().expect("damaged variant");

    assert_eq!(anim.anim_type, "CASEAT02_A");
    assert_eq!(anim.start_frame, 0);
    assert_eq!(anim.loop_start, 0);
    assert_eq!(anim.loop_end, 20);
    assert_eq!(damaged.anim_type, "CASEAT02_AD");
    assert_eq!(damaged.start_frame, 21);
    assert_eq!(damaged.loop_start, 21);
    assert_eq!(damaged.loop_end, 39);
}

#[test]
fn test_resolve_declared_palette_id() {
    let ini: IniFile = IniFile::from_str("[TEST]\nImage=TESTART\n\n[TESTART]\nPalette=anim\n");
    let reg: ArtRegistry = ArtRegistry::from_ini(&ini);
    assert_eq!(
        reg.resolve_declared_palette_id("TEST", "TEST"),
        Some("anim".to_string())
    );
}

#[test]
fn test_anim_candidates_use_anim_section_flags() {
    let ini: IniFile = IniFile::from_str("[CAOILD_A]\nImage=CAOILDX\nNewTheater=yes\n");
    let reg: ArtRegistry = ArtRegistry::from_ini(&ini);
    let image_id: String = reg.resolve_effective_image_id("CAOILD_A", "CAOILD_A");
    let candidates: Vec<String> =
        anim_shp_candidates(Some(&reg), "CAOILD_A", &image_id, "urb", "NEWURBAN");
    // Native order: the theater-letter name, then the forced `G` name. The
    // rest is VERA leniency.
    assert_eq!(
        candidates,
        vec![
            "CNOILDX.SHP",
            "CGOILDX.SHP",
            "CNOILDX.URB",
            "CAOILDX.SHP",
            "CAOILDX.URB",
        ]
    );
}

/// `[GAPOWR_AD] Image=GAPOWR_A` authors no `NewTheater=`. The per-theater
/// reload (`0x00428CD6`) then skips the theater letter, fails on
/// `GAPOWR_A.SHP`, and `FUN_005F9710` forces the second letter to `G`
/// unconditionally, which is the file retail ships.
#[test]
fn anim_candidates_force_the_generic_letter_without_new_theater() {
    let reg = ArtRegistry::from_ini(&IniFile::from_str("[GAPOWR_AD]\nImage=GAPOWR_A\n"));
    let image_id = reg.resolve_effective_image_id("GAPOWR_AD", "GAPOWR_AD");
    let candidates = anim_shp_candidates(Some(&reg), "GAPOWR_AD", &image_id, "tem", "TEMPERATE");
    assert_eq!(&candidates[..2], ["GAPOWR_A.SHP", "GGPOWR_A.SHP"]);
}

/// `FUN_005F96B0` substitutes only `[GNCY][AT]` names, so a `NewTheater=yes`
/// type outside that pattern keeps its name before the `G` fallback.
#[test]
fn anim_theater_letter_needs_the_native_name_pattern() {
    let reg = ArtRegistry::from_ini(&IniFile::from_str(
        "[XAFOO]\nNewTheater=yes\n[YAGRND_B]\nNewTheater=yes\n",
    ));
    let plain = anim_shp_candidates(Some(&reg), "XAFOO", "XAFOO", "tem", "TEMPERATE");
    assert_eq!(&plain[..2], ["XAFOO.SHP", "XGFOO.SHP"]);
    let yuri = anim_shp_candidates(Some(&reg), "YAGRND_B", "YAGRND_B", "tem", "TEMPERATE");
    assert_eq!(&yuri[..2], ["YTGRND_B.SHP", "YGGRND_B.SHP"]);
}

#[test]
fn test_make_candidates_use_deduped_uppercase_names() {
    let reg: ArtRegistry = ArtRegistry::empty();
    let candidates: Vec<String> = make_shp_candidates(Some(&reg), "GAPOWR", "sno", "SNOW");
    assert_eq!(
        candidates,
        vec![
            "GAPOWRMK.SHP",
            "GAPOWRMK.SNO",
            "GGPOWRMK.SHP",
            "GGPOWRMK.SNO",
        ]
    );
}

#[test]
fn test_resolve_overlay_image_id_and_candidates() {
    let art_ini: IniFile = IniFile::from_str("[LOBRDG27]\nImage=LOBRDGX\nTheater=yes\n");
    let rules_ini: IniFile = IniFile::from_str("[LOBRDG27]\nImage=LOBRDGY\n");
    let reg: ArtRegistry = ArtRegistry::from_ini(&art_ini);

    let image_id: String = reg.resolve_overlay_image_id("LOBRDG27", &rules_ini);
    assert_eq!(image_id, "LOBRDGY");

    let candidates: Vec<String> =
        overlay_shp_candidates(Some(&reg), "LOBRDG27", &image_id, "tem", "TEMPERATE");
    assert_eq!(candidates, vec!["LOBRDGY.TEM", "LOBRDGY.SHP"]);
    assert!(!candidates.iter().any(|name| name.contains("LOBRDG26")));
}

#[test]
fn exact_foundation_sections_preserve_folded_visual_lookup_and_mutation() {
    let ini = IniFile::from_str(
        "[HUT]\nFoundation=2x2\nYDrawOffset=4\n[hut]\nFoundation=3x3\nYDrawOffset=9\n",
    );
    let mut registry = ArtRegistry::from_ini(&ini);
    assert_eq!(registry.read_foundation("HUT", 0), 3);
    assert_eq!(registry.read_foundation("hut", 0), 6);
    assert_eq!(registry.read_foundation("HuT", 1), 1);
    assert_eq!(
        registry.len(),
        1,
        "visual aliases retain the former folded count"
    );
    assert_eq!(registry.get("HuT").unwrap().y_draw_offset, 9);
    registry.get_mut("HUT").unwrap().y_draw_offset = 12;
    let entries: Vec<_> = registry.iter_entries().collect();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].0, "HUT");
    assert_eq!(entries[0].1.y_draw_offset, 12);
    assert_eq!(registry.get("hut").unwrap().y_draw_offset, 12);
    assert_eq!(registry.read_foundation("HUT", 0), 3);
}

#[test]
fn add_remove_occupy_scans_sparse_numbered_keys() {
    let ini: IniFile = IniFile::from_str(
        "[FOO]\n\
         AddOccupy1=-1,0\n\
         AddOccupy3=2,3\n\
         RemoveOccupy1=4,5\n\
         RemoveOccupy4=-2,-3\n",
    );
    let registry: ArtRegistry = ArtRegistry::from_ini(&ini);
    let entry: &ArtEntry = registry.get("FOO").expect("FOO");
    assert_eq!(entry.add_occupy[0], Some((-1, 0)));
    assert_eq!(entry.add_occupy[1], None);
    assert_eq!(entry.add_occupy[2], Some((2, 3)));
    assert_eq!(entry.remove_occupy[0], Some((4, 5)));
    assert_eq!(entry.remove_occupy[3], Some((-2, -3)));
}

#[test]
fn add_occupy_skips_malformed_entries() {
    let ini: IniFile =
        IniFile::from_str("[FOO]\nAddOccupy1=not_a_pair\nAddOccupy2=1,2\nAddOccupy4=3,4\n");
    let registry: ArtRegistry = ArtRegistry::from_ini(&ini);
    let entry: &ArtEntry = registry.get("FOO").expect("FOO");
    assert_eq!(entry.add_occupy[0], None);
    assert_eq!(entry.add_occupy[1], Some((1, 2)));
    assert_eq!(entry.add_occupy[3], Some((3, 4)));
}

#[test]
fn gsi_04_05_hidden_object_id_gate_is_distinct_from_image_metadata() {
    let ini = IniFile::from_str(
        "[TESTBLD]\nImage=TESTART\nCanHideThings=no\nOccupyHeight=99\nAddOccupy1=9,9\n\
         \n[TESTART]\nCanHideThings=yes\nHeight=5\nAddOccupy3=-2,1\nRemoveOccupy8=3,1\n",
    );
    let registry = ArtRegistry::from_ini(&ini);
    let profile = registry.building_hidden_occupancy_profile("TESTBLD", "TESTART");

    assert!(
        !profile.can_hide_things,
        "gate comes from object-ID section"
    );
    assert_eq!(
        profile.occupy_height, 5,
        "image Height is the absent-key OccupyHeight default"
    );
    assert_eq!(
        profile.add_occupy[0], None,
        "type-ID offsets are not consumed"
    );
    assert_eq!(profile.add_occupy[2], Some((-2, 1)));
    assert_eq!(profile.remove_occupy[7], Some((3, 1)));
}

#[test]
fn parse_partial_offsets_collects_what_exists() {
    // art has only DockingOffset0 and DockingOffset2 (gap at 1).
    // Parser collects what's present. The art→rules merge handles sizing to NumberOfDocks.
    let ini: IniFile = IniFile::from_str(
        "[ODD]\n\
         DockingOffset0=64,0,0\n\
         DockingOffset2=192,0,0\n",
    );
    let reg: ArtRegistry = ArtRegistry::from_ini(&ini);
    let entry: &ArtEntry = reg.get("ODD").expect("ODD entry");
    assert_eq!(entry.pads.len(), 2, "filter_map skips missing index 1");
    assert_eq!(entry.pads[0].lepton_offset, (64, 0, 0));
    assert_eq!(entry.pads[1].lepton_offset, (192, 0, 0));
}

#[test]
fn test_parses_guardian_gi_sequence_frames() {
    // GGI's GuardianGISequence has Deploy=300,15,0; Undeploy=180,2,2;
    // DeployedFire=323,6,6. ArtEntry should pull the middle integer of each.
    let ini: IniFile = IniFile::from_str(
        "[GGI]\n\
         Sequence=GuardianGISequence\n\
         \n\
         [GuardianGISequence]\n\
         Ready=0,1,1\n\
         Walk=8,6,6\n\
         Deploy=300,15,0\n\
         Undeploy=180,2,2\n\
         Deployed=315,1,1\n\
         DeployedFire=323,6,6\n",
    );
    let reg: ArtRegistry = ArtRegistry::from_ini(&ini);
    let entry: &ArtEntry = reg.get("GGI").expect("GGI entry");
    assert_eq!(entry.sequence.as_deref(), Some("GuardianGISequence"));
    assert_eq!(entry.deploy_frames, Some(15));
    assert_eq!(entry.undeploy_frames, Some(2));
    assert_eq!(entry.deployed_fire_frames, Some(6));
}

#[test]
fn test_sequence_frames_follow_the_native_sequence_scan() {
    // 0x00523D00 scans `%d,%d,%d,%s` over the constructor's zero record; a
    // literal comma skips no whitespace, so `8 , 6` stops after one field.
    let deploy_frames = |value: &str| {
        let ini = IniFile::from_str(&format!("[E1]\nSequence=S\n[S]\nDeploy={value}\n"));
        ArtRegistry::from_ini(&ini)
            .get("E1")
            .expect("E1 entry")
            .deploy_frames
    };
    assert_eq!(deploy_frames("300,15,0"), Some(15));
    assert_eq!(deploy_frames(" 8 , 6 , 6 "), Some(0));
    assert_eq!(deploy_frames("only-one"), Some(0));
    assert_eq!(deploy_frames("a,b,c"), Some(0));
}

#[test]
fn populate_anim_frame_dims_binds_smudge_inputs_without_a_renderer() {
    let root = AnimDimTestRoot::new();
    std::fs::write(root.path().join("BIGIMAGE.SHP"), single_frame_shp(61, 51))
        .expect("write big smudge anim SHP");
    let assets = crate::assets::asset_manager::AssetManager::from_loose_root_for_test(root.path());
    let mut registry = ArtRegistry::from_ini(&IniFile::from_str(
        "[BIG]\nImage=BIGIMAGE\nCrater=yes\n\
         [MISSING]\nScorch=yes\n\
         [UNFLAGGED]\nImage=BIGIMAGE\n",
    ));

    let (populated, fallback) = registry.populate_anim_frame_dims(&assets, "TEM", "TEMPERATE");

    assert_eq!((populated, fallback), (1, 1));
    let big = registry.get("BIG").expect("bound big anim");
    assert_eq!((big.frame_width, big.frame_height), (61, 51));
    let missing = registry.get("MISSING").expect("fallback anim");
    assert_eq!((missing.frame_width, missing.frame_height), (30, 30));
    let unflagged = registry.get("UNFLAGGED").expect("unflagged anim");
    assert_eq!((unflagged.frame_width, unflagged.frame_height), (30, 30));
}

fn single_frame_shp(width: u16, height: u16) -> Vec<u8> {
    let headers_end = 8 + 24;
    let pixel_count = usize::from(width) * usize::from(height);
    let mut data = Vec::with_capacity(headers_end + pixel_count);
    data.extend_from_slice(&0_u16.to_le_bytes());
    data.extend_from_slice(&width.to_le_bytes());
    data.extend_from_slice(&height.to_le_bytes());
    data.extend_from_slice(&1_u16.to_le_bytes());
    data.extend_from_slice(&0_u16.to_le_bytes());
    data.extend_from_slice(&0_u16.to_le_bytes());
    data.extend_from_slice(&width.to_le_bytes());
    data.extend_from_slice(&height.to_le_bytes());
    data.extend_from_slice(&[0_u8; 12]);
    data.extend_from_slice(&(headers_end as u32).to_le_bytes());
    data.resize(headers_end + pixel_count, 1);
    data
}

static NEXT_ANIM_DIM_TEST_ROOT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

struct AnimDimTestRoot(std::path::PathBuf);

impl AnimDimTestRoot {
    fn new() -> Self {
        let serial = NEXT_ANIM_DIM_TEST_ROOT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "vera20k-anim-dim-bind-{}-{serial}",
            std::process::id()
        ));
        std::fs::create_dir(&path).expect("create anim-dim test root");
        Self(path)
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for AnimDimTestRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn animation_binding_keeps_literal_types_and_exact_image_sections() {
    use crate::rules::{native_processing::RulesLayerStack, ruleset::RuleSet};
    let root = AnimDimTestRoot::new();
    for name in ["REAL", "WRONG", "D"] {
        std::fs::write(
            root.path().join(format!("{name}.SHP")),
            single_frame_shp(3, 5),
        )
        .unwrap();
    }
    let assets = crate::assets::asset_manager::AssetManager::from_loose_root_for_test(root.path());
    let art_ini = IniFile::from_str("[ FX]\nImage=REAL\n[FX]\nImage=WRONG\n");
    let processed = RulesLayerStack::new(IniFile::from_str(
        "[General]\nMetallicDebris=FX, FX,D, none\n",
    ))
    .process_with_fixed_art(&art_ini)
    .unwrap();
    let rules = RuleSet::from_processed_rules(&processed).unwrap();
    let mut art = ArtRegistry::from_ini(&art_ini);
    art.apply_anim_type_read_states(rules.anim_type_art_read_states());
    assert_eq!(art.resolve_anim_image_id(" FX"), "REAL");
    assert_eq!(art.resolve_anim_image_id("FX"), "WRONG");
    // Generic object resolution intentionally retains its established behavior.
    assert_eq!(art.resolve_effective_image_id(" FX", " FX"), "WRONG");
    let roots = rules.general.metallic_debris.clone();
    art.bind_scheduler_anim_assets(&roots, &assets, "TEM", "TEMPERATE")
        .unwrap();
    for name in ["FX", " FX", "D", " NONE"] {
        assert!(art.scheduler_anim_types().contains(name));
    }
    assert_eq!(
        art.anim_runtime_config(" FX").unwrap().raw_shp_frame_count,
        Some(1)
    );
    let d = art.anim_runtime_config("D").unwrap();
    assert!(!d.art_body_read);
    assert!(!d.bouncer);
    assert_eq!(d.raw_shp_frame_count, None, "orphan D.SHP must not bind");
    assert_eq!((d.end, d.loop_end, d.rate_logic_frames), (0, 0, 1));
    let mut tolerant = ArtRegistry::from_ini(&art_ini);
    tolerant.apply_anim_type_read_states(rules.anim_type_art_read_states());
    assert_eq!(
        tolerant.bind_anim_class_assets(&roots, &assets, "TEM", "TEMPERATE"),
        0
    );
    assert_eq!(tolerant.scheduler_anim_types(), art.scheduler_anim_types());
}

#[test]
fn animation_image25_matches_original_reader_and_loader_selection() {
    let rows: Vec<serde_json::Value> = serde_json::from_str(crate::test_fixture::text(
        "tools/rules_oracle/anim_image.json",
    ))
    .unwrap();
    for row in rows {
        let name = row["type"].as_str().unwrap();
        let mut section = IniSection::new(name.to_owned());
        if let Some(raw) = row["raw"].as_str() {
            section.set("Image", raw);
        }
        let mut ini = IniFile::empty();
        ini.replace_first_section(section);
        let art = ArtRegistry::from_ini(&ini);
        assert_eq!(
            art.anim_runtime_config(name).unwrap().image,
            row["image"].as_str().unwrap(),
            "{row}"
        );
        assert_eq!(
            art.resolve_anim_image_id(name),
            row["selected"].as_str().unwrap().to_ascii_uppercase(),
            "{row}"
        );
    }
}

/// TechnoTypeClass::ReadINI reads `UseBuffer=` from the ART section named by
/// the type's own ID (`0x0071577B`), never the `Image=` section it draws.
#[test]
fn use_buffer_reads_the_type_id_section() {
    let ini = IniFile::from_str(
        "[SCHP]\nUseBuffer=yes\n[ALIAS]\nImage=SCHP\n[OFF]\nUseBuffer=no\n[ODD]\nUseBuffer=on\n",
    );
    let reg = ArtRegistry::from_ini(&ini);
    assert!(reg.use_buffer("SCHP"));
    assert!(!reg.use_buffer("ALIAS"), "the Image= section is not read");
    assert!(!reg.use_buffer("OFF"));
    // ReadBool keeps the constructor's false for a first character outside 1TY0FN.
    assert!(!reg.use_buffer("ODD"));
    assert!(!reg.use_buffer("MISSING"));
}

#[test]
fn retail_use_buffer_types_are_the_four_helicopters() {
    let Some((_, art_ini)) = crate::rules::retail_ini_fixture::retail_rules_and_art() else {
        return;
    };
    let reg = ArtRegistry::from_ini(&art_ini);
    let mut flagged: Vec<&str> = art_ini
        .section_names()
        .into_iter()
        .filter(|name| reg.use_buffer(name))
        .collect();
    flagged.sort_unstable();
    assert_eq!(flagged, ["HIND", "SCHD", "SCHP", "SHAD"]);
}
