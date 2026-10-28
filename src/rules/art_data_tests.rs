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

#[cfg(feature = "ares")]
#[test]
fn pcx_cameos_read_only_the_rules_image_art_section() {
    let ini = IniFile::from_str(
        "[TYPE]\nCameoPCX=wrong-type.pcx\n\
         [IMAGE]\nImage=BODY\nCameoPCX=Normal.PCX\nAltCameoPCX=Elite.PCX\n\
         [BODY]\nCameoPCX=wrong-body.pcx\n",
    );
    let registry = ArtRegistry::from_ini(&ini);
    assert_eq!(
        registry.resolve_declared_pcx_cameos("TYPE", "IMAGE"),
        [Some("normal.pcx"), Some("elite.pcx")]
    );
    assert_eq!(
        registry.resolve_declared_pcx_cameos("TYPE", "MISSING"),
        [None, None]
    );
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

/// A building slot's normal, damaged and garrisoned names.
fn slot_names(entry: &ArtEntry, slot: u8) -> (&str, Option<&str>, Option<&str>) {
    let config = entry
        .building_anims
        .iter()
        .find(|config| config.native_slot == slot)
        .unwrap();
    (
        &config.anim_type,
        config
            .damaged_variant
            .as_ref()
            .map(|v| v.anim_type.as_str()),
        config
            .garrisoned_variant
            .as_ref()
            .map(|v| v.anim_type.as_str()),
    )
}

/// ReadINI copies a slot's normal name into its empty damaged and garrisoned
/// fields (`parse_building_anims` cites each copy); an authored variant stays,
/// and the PowerUp loop makes no copy.
#[test]
fn absent_variant_names_take_the_normal_name_except_power_ups() {
    let ini = IniFile::from_str(
        "[B]\n\
         IdleAnim=IDLE\n\
         ActiveAnim=ACT\n\
         ActiveAnimDamaged=ACTD\n\
         SpecialAnimGarrisoned=SPECG\n\
         PowerUp1Anim=PUP\n",
    );
    let reg = ArtRegistry::from_ini(&ini);
    let entry = reg.get("B").unwrap();
    assert_eq!(slot_names(entry, 18), ("IDLE", Some("IDLE"), Some("IDLE")));
    assert_eq!(slot_names(entry, 3), ("ACT", Some("ACTD"), Some("ACT")));
    assert_eq!(slot_names(entry, 10), ("", None, Some("SPECG")));
    assert_eq!(slot_names(entry, 0), ("PUP", None, None));
}

/// Retail refineries through the production ART reader: the Slave Miner
/// refinery's IdleAnim, GAREFN's ore lights and both unload anims declare no
/// variant and keep their normal names; NAREFN's authored damaged lights stay.
#[test]
fn retail_refinery_anims_without_variants_keep_their_normal_names() {
    let Some(art_ini) = crate::rules::retail_ini_fixture::retail_ini("artmd.ini") else {
        return;
    };
    let reg = crate::rules::retail_ini_fixture::art_registry(&art_ini);
    let names = |section: &str, slot| slot_names(reg.get(section).unwrap(), slot);
    let unchanged = |name| (name, Some(name), Some(name));
    assert_eq!(names("YAREFN", 18), unchanged("YAREFN_A"));
    for (slot, light) in (3..=6).zip(["GAREFNL1", "GAREFNL2", "GAREFNL3", "GAREFNL4"]) {
        assert_eq!(names("GAREFN", slot), unchanged(light));
    }
    assert_eq!(names("GAREFN", 10), unchanged("GAREFNOR"));
    assert_eq!(names("NAREFN", 10), unchanged("NAREFNOR"));
    assert_eq!(
        names("NAREFN", 3),
        ("NAREFNL1", Some("NAREFNL1D"), Some("NAREFNL1"))
    );
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
fn test_anim_candidates_use_explicit_own_flags_without_an_image_section() {
    let ini: IniFile = IniFile::from_str("[CAOILD_A]\nImage=CAOILDX\nNewTheater=yes\n");
    let reg: ArtRegistry = ArtRegistry::from_ini(&ini);
    let image_id: String = reg.resolve_effective_image_id("CAOILD_A", "CAOILD_A");
    let candidates: Vec<String> = effect_shp_candidates(
        crate::rules::art_data::EffectImageKind::Animation,
        Some(&reg),
        "CAOILD_A",
        &image_id,
        "urb",
        "NEWURBAN",
    );
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

/// When neither section supplies NewTheater, the initial name is unchanged
/// and FUN_005F9710 still forces the second letter to G on a miss. This omits
/// the Image section deliberately; it is not the complete retail ART input.
#[test]
fn anim_candidates_force_the_generic_letter_without_new_theater() {
    let reg = ArtRegistry::from_ini(&IniFile::from_str("[GAPOWR_AD]\nImage=GAPOWR_A\n"));
    let image_id = reg.resolve_effective_image_id("GAPOWR_AD", "GAPOWR_AD");
    let candidates = effect_shp_candidates(
        crate::rules::art_data::EffectImageKind::Animation,
        Some(&reg),
        "GAPOWR_AD",
        &image_id,
        "tem",
        "TEMPERATE",
    );
    assert_eq!(&candidates[..2], ["GAPOWR_A.SHP", "GGPOWR_A.SHP"]);
}

#[test]
fn animation_flags_inherit_image_then_apply_own_read_bool() {
    for (own, expected_when_false, expected_when_true) in [
        (None, false, true),
        (Some("yes"), true, true),
        (Some("no"), false, false),
        (Some(""), false, true),
        (Some("on"), false, true),
    ] {
        for inherited in [false, true] {
            let own_keys = own.map_or_else(String::new, |value| {
                format!("Theater={value}\nNewTheater={value}\n")
            });
            let ini = IniFile::from_str(&format!(
                "[DAMAGED]\nImage=CAIMAGE\n{own_keys}\n[CAIMAGE]\nTheater={inherited}\nNewTheater={inherited}\n"
            ));
            let art = ArtRegistry::from_ini(&ini);
            let expected = if inherited {
                expected_when_true
            } else {
                expected_when_false
            };
            assert_eq!(
                art.anim_theater_flags["DAMAGED"],
                (expected, expected),
                "own={own:?}, inherited={inherited}"
            );
            // Generic object metadata retains its own-section interpretation.
            assert_eq!(art.get("DAMAGED").unwrap().new_theater, own == Some("yes"));
        }
    }
}

#[test]
fn animation_flags_use_one_image_hop_and_exact_section_identity() {
    let ini = IniFile::from_str(
        "[DAMAGED]\nImage=CAIMAGE\n\
         [CAIMAGE]\nImage=CAOTHER\nNewTheater=yes\n\
         [CAOTHER]\nTheater=yes\nNewTheater=no\n\
         [caimage]\nTheater=yes\nNewTheater=no\n",
    );
    let art = ArtRegistry::from_ini(&ini);
    assert_eq!(art.anim_theater_flags["DAMAGED"], (false, true));
    let image = art.resolve_anim_image_id("DAMAGED");
    assert_eq!(image, "CAIMAGE");
    assert_eq!(
        &effect_shp_candidates(
            crate::rules::art_data::EffectImageKind::Animation,
            Some(&art),
            "DAMAGED",
            &image,
            "URB",
            "URBAN"
        )[..2],
        ["CUIMAGE.SHP", "CGIMAGE.SHP"]
    );
}

/// `FUN_005F96B0` substitutes only `[GNCY][AT]` names, so a `NewTheater=yes`
/// type outside that pattern keeps its name before the `G` fallback.
#[test]
fn anim_theater_letter_needs_the_native_name_pattern() {
    let reg = ArtRegistry::from_ini(&IniFile::from_str(
        "[XAFOO]\nNewTheater=yes\n[YAGRND_B]\nNewTheater=yes\n",
    ));
    let plain = effect_shp_candidates(
        crate::rules::art_data::EffectImageKind::Animation,
        Some(&reg),
        "XAFOO",
        "XAFOO",
        "tem",
        "TEMPERATE",
    );
    assert_eq!(&plain[..2], ["XAFOO.SHP", "XGFOO.SHP"]);
    let yuri = effect_shp_candidates(
        crate::rules::art_data::EffectImageKind::Animation,
        Some(&reg),
        "YAGRND_B",
        "YAGRND_B",
        "tem",
        "TEMPERATE",
    );
    assert_eq!(&yuri[..2], ["YTGRND_B.SHP", "YGGRND_B.SHP"]);
}

#[test]
fn overlay_image_projection_ignores_own_art_redirect_and_flags() {
    let art = IniFile::from_str("[LOBRDG27]\nImage=LOBRDGX\nTheater=yes\n");
    let rules = IniFile::from_str("[OverlayTypes]\n0=LOBRDG27\n[LOBRDG27]\nImage=LOBRDGY\n");
    let overlays = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(&rules, Some(&art));
    // OverlayRead5FE78B inherits ObjectRead5F933B: only Rules Image is
    // read, then flags from ART[that Image], not ART[the type's own ID].
    assert_eq!(
        overlays.shp_candidates("LOBRDG27", "TEM", "TEMPERATE"),
        ["LOBRDGY.SHP"]
    );
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
fn parse_partial_offsets_keeps_each_pad_at_its_index() {
    // art has only DockingOffset0 and DockingOffset2 (gap at 1). Native reads
    // each slot by index (0x004649AF..0x00464A41); slot 1 keeps its zero.
    let ini: IniFile = IniFile::from_str(
        "[ODD]\n\
         DockingOffset0=64,0,0\n\
         DockingOffset2=192,0,0\n",
    );
    let reg: ArtRegistry = ArtRegistry::from_ini(&ini);
    let entry: &ArtEntry = reg.get("ODD").expect("ODD entry");
    assert_eq!(entry.pads.len(), 3);
    assert_eq!(entry.pads[0].lepton_offset, (64, 0, 0));
    assert_eq!(entry.pads[1].lepton_offset, (0, 0, 0));
    assert_eq!(entry.pads[2].lepton_offset, (192, 0, 0));
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
fn animation_image_flag_binding_preserves_own_runtime_metadata() {
    use crate::rules::{native_processing::RulesLayerStack, ruleset::RuleSet};

    let root = AnimDimTestRoot::new();
    for (name, frames) in [
        ("CUIMAGE.SHP", 8_i16),
        ("CAIMAGE.SHP", 2),
        ("CGIMAGE.SHP", 6),
    ] {
        // Loader-only inputs: bounds are read from the signed SHP header.
        let mut header = vec![0_u8; 8];
        header[2..4].copy_from_slice(&1_u16.to_le_bytes());
        header[4..6].copy_from_slice(&1_u16.to_le_bytes());
        header[6..8].copy_from_slice(&frames.to_le_bytes());
        std::fs::write(root.path().join(name), header).unwrap();
    }
    let assets = crate::assets::asset_manager::AssetManager::from_loose_root_for_test(root.path());
    let ini = IniFile::from_str(
        "[DAMAGED]\nImage=CAIMAGE\nStart=2\nLoopStart=2\nLoopEnd=4\nShadow=yes\n\
         [OVERRIDE]\nImage=CAIMAGE\nNewTheater=no\nShadow=no\n\
         [CAIMAGE]\nNewTheater=yes\nStart=0\nLoopStart=0\nLoopEnd=2\nShadow=no\n",
    );
    let processed = RulesLayerStack::new(IniFile::from_str(
        "[General]\nMetallicDebris=DAMAGED,OVERRIDE\n",
    ))
    .process_with_fixed_art(&ini)
    .unwrap();
    let rules = RuleSet::from_processed_rules(&processed).unwrap();
    let mut art = ArtRegistry::from_ini(&ini);
    art.apply_anim_type_read_states(rules.anim_type_art_read_states());
    art.bind_scheduler_anim_assets(&rules.general.metallic_debris, &assets, "URB", "URBAN")
        .unwrap();
    let damaged = art.anim_runtime_config("DAMAGED").unwrap();
    assert_eq!(damaged.raw_shp_frame_count, Some(8));
    assert_eq!(
        (
            damaged.start,
            damaged.loop_start,
            damaged.loop_end,
            damaged.end
        ),
        (2, 2, 4, 4)
    );
    assert!(damaged.shadow);
    let explicit = art.anim_runtime_config("OVERRIDE").unwrap();
    assert_eq!(explicit.raw_shp_frame_count, Some(2));
    assert_eq!(explicit.end, 2);
    assert!(!explicit.shadow);
}

#[test]
fn scenario_anim_theater_replaces_only_the_shape_and_retries_the_stored_id() {
    use crate::rules::native_processing::{RulesLayerStack, RulesProcessContext};
    use crate::rules::object_image::ImageTheater;
    use crate::rules::ruleset::RuleSet;

    let root = AnimDimTestRoot::new();
    for (name, frames) in [
        ("ALIAA.SNO", 6_i16),
        ("FX.SHP", 2),
        ("CTTILE.SHP", 4),
        ("CATILE.SHP", 8),
        ("CTPLAIN.SHP", 3),
        ("CAPLAIN.SHP", 5),
    ] {
        let mut bytes = single_frame_shp(1, 1);
        bytes[6..8].copy_from_slice(&frames.to_le_bytes());
        std::fs::write(root.path().join(name), bytes).unwrap();
    }
    let assets = crate::assets::asset_manager::AssetManager::from_loose_root_for_test(root.path());
    let fixed_art = IniFile::from_str(
        "[FX]\nImage=ALIA\nTheater=yes\nAlternateArcticArt=yes\n[CTTILE]\nNewTheater=yes\n[CTPLAIN]\nRate=1\n",
    );
    let layers = RulesLayerStack::new(IniFile::from_str(
        "[Animations]\n0=FX\n1=CTTILE\n2=CTPLAIN\n",
    ));
    let processed = layers
        .process_with_fixed_art_and_registry_state_recovering(
            &fixed_art,
            Default::default(),
            RulesProcessContext::ScenarioReset {
                theater: ImageTheater::Snow,
                skip_root_color_add_and_mode: false,
            },
            &Default::default(),
        )
        .unwrap();
    let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
    rules.install_art_data(ArtRegistry::from_ini(&fixed_art));
    let image = rules.art().resolve_anim_image_id("FX");
    assert_eq!(
        effect_shp_candidates(
            crate::rules::art_data::EffectImageKind::Animation,
            Some(rules.art()),
            "FX",
            &image,
            "SNO",
            "SNOW"
        ),
        ["FX.SNO", "FX.SHP"],
        "Read_Map4AD092 -> InitTheater427940 replaces the resolver's image plan"
    );
    rules
        .bind_scheduler_anim_assets(
            &["FX".into(), "CTTILE".into(), "CTPLAIN".into()],
            &assets,
            "SNO",
            "SNOW",
        )
        .unwrap();
    let config = rules.art().anim_runtime_config("FX").unwrap();
    assert_eq!(config.raw_shp_frame_count, Some(2));
    assert_eq!(config.middle_frame, Some(3));
    assert_eq!(
        (config.end, config.loop_end),
        (6, 6),
        "427940 does not repeat427B50's numeric writes"
    );
    let tile = rules.art().anim_runtime_config("CTTILE").unwrap();
    assert_eq!(
        (
            tile.raw_shp_frame_count,
            tile.end,
            tile.loop_end,
            tile.middle_frame
        ),
        (Some(8), 4, 4, Some(4)),
        "ReadINI's-1 request seeds bounds before the real-theater resolver replaces Shape and Middle"
    );
    let plain = rules.art().anim_runtime_config("CTPLAIN").unwrap();
    assert_eq!(
        (plain.raw_shp_frame_count, plain.end),
        (Some(3), 3),
        "without NewTheater, the generic read image stays resident"
    );
}

#[test]
fn animation_theater_flags_match_original_fresh_read_and_postpass() {
    let corpus = crate::test_fixture::json("tools/rules_oracle/anim_theater_flags.json");
    assert_eq!(corpus["schema"], 1);
    let cases = corpus["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 26);
    let imageless = crate::test_fixture::json("tools/rules_oracle/imageless_anim.json");
    let imageless_cases = imageless["cases"].as_array().unwrap();
    assert_eq!(imageless_cases.len(), 3);
    let mut bound_cases = 0;
    let mut missing_cases = 0;
    for case in cases.iter().chain(imageless_cases) {
        let input = &case["input"];
        let label = input["case"].as_str().unwrap();
        let name = input["name"].as_str().unwrap();
        // The native fixture supplies cached entries, including Image="".
        // A physical file load deliberately omits empty values, so construct
        // the same cache boundary as the original Image25 reader test below.
        let mut ini = IniFile::empty();
        for (section_name, values) in input["art_sections"].as_object().unwrap() {
            let mut section = IniSection::new(section_name.clone());
            for (key, value) in values.as_object().unwrap() {
                section.set(key, value.as_str().unwrap());
            }
            ini.replace_first_section(section);
        }
        let mut art = ArtRegistry::from_ini(&ini);
        let admitted = case["reader_admitted"].as_bool().unwrap();
        art.apply_anim_type_read_states(&[(name.to_string(), admitted)]);
        let expected = &case["after_postpass"];
        let flags = (
            expected["theater"].as_bool().unwrap(),
            expected["new_theater"].as_bool().unwrap(),
        );
        assert_eq!(
            art.anim_runtime_config(name).unwrap().art_body_read,
            admitted,
            "{label}"
        );
        if admitted {
            assert_eq!(art.anim_theater_flags[name], flags, "{label}");
        } else {
            // This map is orphan resource metadata, not a constructor-only
            // Anim's image authority. Its real draw contract is no request.
            assert!(
                effect_shp_candidates(
                    EffectImageKind::Animation,
                    Some(&art),
                    name,
                    name,
                    "SNO",
                    "SNOW"
                )
                .is_empty(),
                "{label}"
            );
        }
        assert_eq!(
            art.anim_runtime_config(name).unwrap().image,
            expected["image"].as_str().unwrap(),
            "{label}"
        );
        // These rows also record Theater=yes filename behavior. Only flag
        // resolution is compared for that cohort: the candidate helper's
        // existing plain-SHP leniency is outside this change.
        if flags.0 || !admitted {
            continue;
        }
        let root = AnimDimTestRoot::new();
        for (filename, header) in input["available_images"].as_object().unwrap() {
            let mut bytes = Vec::with_capacity(8);
            for word in [
                0,
                header["width"].as_u64().unwrap() as u16,
                header["height"].as_u64().unwrap() as u16,
                header["raw_frame_count"].as_u64().unwrap() as u16,
            ] {
                bytes.extend_from_slice(&word.to_le_bytes());
            }
            std::fs::write(root.path().join(filename), bytes).unwrap();
        }
        let assets =
            crate::assets::asset_manager::AssetManager::from_loose_root_for_test(root.path());
        let (extension, theater) = match input["theater"].as_u64().unwrap() {
            1 => ("SNO", "SNOW"),
            2 => ("URB", "URBAN"),
            other => panic!("unexpected corpus theater {other}"),
        };
        let image = art.resolve_anim_image_id(name);
        let candidates = effect_shp_candidates(
            crate::rules::art_data::EffectImageKind::Animation,
            Some(&art),
            name,
            &image,
            extension,
            theater,
        );
        let selected = candidates
            .iter()
            .find(|filename| assets.get_ref(filename).is_some());
        assert_eq!(
            selected.map(String::as_str),
            expected["loaded_filename"].as_str(),
            "{label}"
        );
        let result =
            art.bind_scheduler_anim_assets(&[name.to_string()], &assets, extension, theater);
        if expected["image_present"].as_bool().unwrap() {
            result.unwrap_or_else(|error| panic!("{label}: {error}"));
            let config = art.anim_runtime_config(name).unwrap();
            assert_eq!(
                config.raw_shp_frame_count,
                Some(expected["raw_shp_frame_count"].as_i64().unwrap() as i32),
                "{label}"
            );
            assert_eq!(
                config.end,
                expected["end"].as_i64().unwrap() as i32,
                "{label}"
            );
            assert_eq!(
                config.loop_end,
                expected["loop_end"].as_i64().unwrap() as i32,
                "{label}"
            );
            bound_cases += 1;
        } else {
            result.unwrap_or_else(|error| panic!("{label}: {error}"));
            let config = art.anim_runtime_config(name).unwrap();
            assert_eq!(config.raw_shp_frame_count, admitted.then_some(0), "{label}");
            assert_eq!(
                config.end,
                expected["end"].as_i64().unwrap() as i32,
                "{label}"
            );
            assert_eq!(
                config.loop_end,
                expected["loop_end"].as_i64().unwrap() as i32,
                "{label}"
            );
            missing_cases += 1;
        }
    }
    assert!(bound_cases > 0 && missing_cases > 0);
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

    // AnimTypeLoad428855 can load D's own file without ReadINI or the
    // distinct bounds resolver427B50. Fresh orphan suppression stays above.
    let mut restored = processed;
    restored.restore_object_image_loads("TEMPERATE").unwrap();
    let mut rules = RuleSet::from_processed_rules(&restored).unwrap();
    rules.install_art_data(ArtRegistry::from_ini(&art_ini));
    rules
        .bind_scheduler_anim_assets(&["D".into(), " NONE".into()], &assets, "TEM", "TEMPERATE")
        .unwrap();
    let d = rules.art().anim_runtime_config("D").unwrap();
    assert!(!d.art_body_read);
    assert_eq!(d.raw_shp_frame_count, Some(1));
    assert_eq!((d.end, d.loop_end, d.rate_logic_frames), (0, 0, 1));
    assert_eq!(
        rules
            .art()
            .anim_runtime_config(" NONE")
            .unwrap()
            .raw_shp_frame_count,
        None
    );
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

/// ReadINI reads `CanBeHidden=` from the type ID's ART section with ReadBool
/// (`0x007121EB`); without that section or key the constructor's true stays.
#[test]
fn can_be_hidden_reads_the_type_id_section() {
    let ini = IniFile::from_str(
        "[GAPOWR]\nCanBeHidden=False\n[ALIAS]\nImage=GAPOWR\n[YAREFN]\nCanBeHidden=True\n[ODD]\nCanBeHidden=off\n",
    );
    let reg = ArtRegistry::from_ini(&ini);
    assert!(!reg.can_be_hidden("GAPOWR"));
    assert!(reg.can_be_hidden("ALIAS"), "the Image= section is not read");
    assert!(reg.can_be_hidden("YAREFN"));
    assert!(reg.can_be_hidden("ODD"), "ReadBool's default for 'o'");
    assert!(reg.can_be_hidden("MISSING"));
}

/// ReadINI reads `Palette=` with ReadString (capacity 0x20) from the ART
/// section its ImageFile names (`0x0071582B`, `ESI = this+0x1F8`); an ART
/// `Image=` is not followed, and an empty value names nothing.
#[test]
fn techno_palette_reads_the_named_section() {
    let ini =
        IniFile::from_str("[CANEWY04]\nPalette=lib\n[ALIAS]\nImage=CANEWY04\n[BLANK]\nPalette=\n");
    let reg = ArtRegistry::from_ini(&ini);
    assert_eq!(reg.techno_palette("CANEWY04"), Some("lib"));
    assert_eq!(reg.techno_palette("ALIAS"), None);
    assert_eq!(reg.techno_palette("BLANK"), None);
    assert_eq!(reg.techno_palette("MISSING"), None);
}

/// A reached TechnoType reader binds the ART Palette under Rules Image,
/// not under the type's own ID. ART installation only projects other visuals.
#[test]
fn custom_palette_projects_through_the_rules_image() {
    let ini = IniFile::from_str(
        "[BuildingTypes]\n0=STATUE\n1=COPY\n2=PLAIN\n\
         [STATUE]\nStrength=10\n[COPY]\nStrength=10\nImage=STATUE\n\
         [PLAIN]\nStrength=10\nImage=OWN\n",
    );
    let art = IniFile::from_str("[STATUE]\nPalette=lib\n[PLAIN]\nPalette=other\n[OWN]\n");
    let mut rules = crate::rules::ruleset::RuleSet::from_ini_with_fixed_art_for_test(&ini, &art)
        .expect("rules");
    rules.install_art_data(ArtRegistry::from_ini(&art));
    let custom = |id: &str| rules.object(id).unwrap().custom_palette_name();
    assert_eq!(custom("STATUE"), Some("lib"));
    assert_eq!(custom("COPY"), Some("lib"), "Rules Image names the section");
    assert_eq!(custom("PLAIN"), None, "the type ID's section is not read");
}

/// Regression: the image loader must not erase an invisible animation's real
/// constructor-time Middle/particle producer. Native null-image reader values
/// come from imageless_anim; particle arithmetic remains with its existing owner.
#[test]
fn imageless_retail_animation_reaches_existing_gas_particle_owner() {
    let Some(fixture) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let expected = fixture
        .fixed_art
        .section_or_empty("VIRUSD")
        .read_int("NumParticles", 0);
    assert!(expected > 0, "selected stock particle animation");
    let root = AnimDimTestRoot::new();
    let assets = crate::assets::asset_manager::AssetManager::from_loose_root_for_test(root.path());
    let mut rules = fixture.rules;
    rules
        .bind_scheduler_anim_assets(&["VIRUSD".to_string()], &assets, "TEM", "TEMPERATE")
        .unwrap();
    let config = rules.art().anim_runtime_config("VIRUSD").unwrap();
    assert_eq!(config.raw_shp_frame_count, Some(0));
    let particle = rules
        .p_type_id_by_name(config.spawns_particle.as_deref().unwrap())
        .unwrap();
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 7);
    sim.native_unique_ids =
        Some(crate::sim::native_identity::NativeUniqueIdCursor::test_at_current_value(1000));
    sim.construct_post_load_particle_system_id(&rules);
    let count = |sim: &crate::sim::world::Simulation| {
        sim.particle_systems()
            .iter()
            .flat_map(|(_, system)| system.particles())
            .filter(|p| p.type_id() == particle)
            .count()
    };
    let before = count(&sim);
    let kind = sim.interner.intern("VIRUSD");
    let anim = sim
        .spawn_combat_explosion_anim(
            &rules,
            kind,
            10,
            10,
            crate::util::fixed_math::SIM_ZERO,
            crate::util::fixed_math::SIM_ZERO,
            0,
            0,
        )
        .expect("missing image does not reject a real particle animation");
    assert_eq!(count(&sim) - before, expected as usize);
    sim.object_ai_visit_one(
        anim,
        Some(&rules),
        crate::sim::world::ObjectAiCtx::default(),
    );
    assert!(sim.anim(anim).is_some(), "constructor first-AI guard");
    sim.session.set_binary_frame_for_test(1 << 16); // expire any u16 rate timer
    sim.object_ai_visit_one(
        anim,
        Some(&rules),
        crate::sim::world::ObjectAiCtx::default(),
    );
    // AI UnInit conceals and queues the physical receiver. The ordinary
    // end-of-frame deletion owner, rather than AI itself, releases storage.
    sim.process_pending_delete();
    assert!(
        sim.anim(anim).is_none(),
        "zero-End animation retires through existing AI and deferred drain"
    );
}
