//! Tests for art.ini parsing and art-resolution layering.

use super::*;

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
    let rows: Vec<Row> = serde_json::from_str(include_str!(
        "../../tools/spatial_oracle/building_slot_power_rules.json"
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
fn test_apply_theater_letter() {
    assert_eq!(apply_theater_letter("GAPOWR", "TEMPERATE"), "GTPOWR");
    assert_eq!(apply_theater_letter("GAPOWR", "SNOW"), "GAPOWR");
    assert_eq!(apply_theater_letter("NAHAND", "TEMPERATE"), "NTHAND");
    assert_eq!(apply_theater_letter("X", "TEMPERATE"), "X");
    assert_eq!(apply_theater_letter("GAPOWR", "UNKNOWN"), "GAPOWR");
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
fn test_apply_generic_letter() {
    assert_eq!(apply_generic_letter("GAPOWR"), "GGPOWR");
    assert_eq!(apply_generic_letter("NAHAND"), "NGHAND");
    assert_eq!(apply_generic_letter("X"), "X");
}

#[test]
fn test_voxel_asset_names() {
    let (vxl, hva) = voxel_asset_names("HTNK");
    assert_eq!(vxl, "HTNK.VXL");
    assert_eq!(hva, "HTNK.HVA");
}

#[test]
fn test_from_ini_parses_entries() {
    let ini: IniFile = IniFile::from_str(
        "[GAPOWR]\nNewTheater=yes\nCameo=GAPICON\n\n[HTNK]\nVoxel=yes\nAltCameo=HTKALT\n\n[NACNST]\nImage=CIVNC\n\n[GI]\nCrawls=yes\nFireUp=2\nSecondaryFire=4\n",
    );
    let reg: ArtRegistry = ArtRegistry::from_ini(&ini);
    assert_eq!(reg.len(), 4);

    let gapowr: &ArtEntry = reg.get("GAPOWR").expect("GAPOWR exists");
    assert!(gapowr.new_theater);
    assert!(!gapowr.voxel);
    assert!(gapowr.image.is_none());
    assert_eq!(gapowr.cameo.as_deref(), Some("GAPICON"));

    let htnk: &ArtEntry = reg.get("HTNK").expect("HTNK exists");
    assert!(!htnk.new_theater);
    assert!(htnk.voxel);
    assert_eq!(htnk.alt_cameo.as_deref(), Some("HTKALT"));

    let nacnst: &ArtEntry = reg.get("NACNST").expect("NACNST exists");
    assert_eq!(nacnst.image.as_deref(), Some("CIVNC"));

    let gi: &ArtEntry = reg.get("GI").expect("GI exists");
    assert!(gi.crawls);
    assert_eq!(gi.fire_up, 2);
    assert_eq!(gi.fire_prone, 2);
    assert_eq!(gi.secondary_fire, 4);
    assert_eq!(gi.secondary_prone, 4);
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
fn parses_anim_start_sound_and_report() {
    let ini: IniFile = IniFile::from_str(
        "[TWLT026]\nReport=ExplosionShard\n\n[TWLT036]\nStartSound=ExplosionStart\nReport=Explosion06\n",
    );
    let reg: ArtRegistry = ArtRegistry::from_ini(&ini);

    let twlt026 = reg.get("TWLT026").expect("TWLT026 exists");
    assert_eq!(twlt026.report.as_deref(), Some("ExplosionShard"));
    assert!(twlt026.start_sound.is_none());

    let twlt036 = reg.get("TWLT036").expect("TWLT036 exists");
    assert_eq!(twlt036.start_sound.as_deref(), Some("ExplosionStart"));
    assert_eq!(twlt036.report.as_deref(), Some("Explosion06"));
}

#[test]
fn parses_generic_animtype_rate_with_constructor_default() {
    let ini: IniFile =
        IniFile::from_str("[UCFAST]\nRate=300\n\n[UCDEFAULT]\nFixtureOnly=1\n\n[UCSTOP]\nRate=0\n");
    let reg: ArtRegistry = ArtRegistry::from_ini(&ini);

    assert_eq!(reg.rate_ms("UCFAST"), Some(200));
    assert_eq!(reg.rate_ms("UCDEFAULT"), Some(DEFAULT_ART_RATE_MS));
    assert_eq!(reg.rate_ms("UCSTOP"), Some(0));
    assert_eq!(reg.rate_ms("MISSING"), None);
    assert_eq!(reg.rate_logic_frames("UCFAST"), Some(3));
    assert_eq!(
        reg.rate_logic_frames("UCDEFAULT"),
        Some(DEFAULT_ART_RATE_LOGIC_FRAMES)
    );
    assert_eq!(reg.rate_logic_frames("UCSTOP"), Some(0));
    assert_eq!(reg.rate_logic_frames("MISSING"), None);
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
    assert_eq!(entry.building_anims.len(), 1);
    assert_eq!(entry.building_anims[0].anim_type, "GAAIRC_A");
}

#[test]
fn test_resolve_metadata_entry_keeps_type_section_when_it_owns_metadata() {
    let ini: IniFile = IniFile::from_str("[NACNST]\nImage=CIVNC\nBibShape=NACNSTBB\n");
    let reg: ArtRegistry = ArtRegistry::from_ini(&ini);

    let entry: &ArtEntry = reg
        .resolve_metadata_entry("NACNST", "NACNST")
        .expect("NACNST metadata should stay on its own section");
    assert_eq!(entry.bib_shape.as_deref(), Some("NACNSTBB"));
    assert_eq!(entry.image.as_deref(), Some("CIVNC"));
}

#[test]
fn test_resolve_declared_cameo_id_prefers_art_data() {
    let ini: IniFile = IniFile::from_str(
        "[E1]\nCameo=E1CAMEO\n\n[MTNK]\nAltCameo=MTNKALT\n\n[NACNST]\nImage=CIVNC\n[CIVNC]\nCameo=CIVICON\n",
    );
    let reg: ArtRegistry = ArtRegistry::from_ini(&ini);

    assert_eq!(reg.resolve_declared_cameo_id("E1", "E1"), "E1CAMEO");
    assert_eq!(reg.resolve_declared_cameo_id("MTNK", "MTNK"), "MTNKALT");
    assert_eq!(reg.resolve_declared_cameo_id("NACNST", "NACNST"), "CIVICON");
    assert_eq!(reg.resolve_declared_cameo_id("UNKNOWN", "FOO"), "FOO");
}

#[test]
fn test_new_theater_from_ini_key() {
    let ini: IniFile = IniFile::from_str("[MYCIVBLD]\nNewTheater=yes\n");
    let reg: ArtRegistry = ArtRegistry::from_ini(&ini);
    assert!(reg.should_use_new_theater("MYCIVBLD"));
}

#[test]
fn test_parse_building_anims() {
    let ini: IniFile = IniFile::from_str(
        "[CAOILD]\nActiveAnim=CAOILD_A\nActiveAnimDamaged=CAOILD_AD\nActiveAnimGarrisoned=CAOILD_AG\nActiveAnimYSort=362\nActiveAnimTwo=CAOILD_F\nActiveAnimTwoZAdjust=-50\n",
    );
    let reg: ArtRegistry = ArtRegistry::from_ini(&ini);
    let entry: &ArtEntry = reg.get("CAOILD").expect("CAOILD exists");
    assert_eq!(entry.building_anims.len(), 2);

    assert_eq!(entry.building_anims[0].anim_type, "CAOILD_A");
    assert_eq!(
        entry.building_anims[0]
            .damaged_variant
            .as_ref()
            .map(|v| v.anim_type.as_str()),
        Some("CAOILD_AD")
    );
    assert_eq!(
        entry.building_anims[0]
            .garrisoned_variant
            .as_ref()
            .map(|v| v.anim_type.as_str()),
        Some("CAOILD_AG")
    );
    assert_eq!(entry.building_anims[0].y_sort, 362);
    assert_eq!(entry.building_anims[0].z_adjust, 0);

    assert_eq!(entry.building_anims[1].anim_type, "CAOILD_F");
    assert_eq!(entry.building_anims[1].y_sort, 0);
    assert_eq!(entry.building_anims[1].z_adjust, -50);
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
        .filter(|a| matches!(a.kind, BuildingAnimKind::Special))
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
        .filter(|a| matches!(a.kind, BuildingAnimKind::Special))
        .map(|a| a.is_primary)
        .collect();
    assert_eq!(primaries, vec![true, false, false, false]);

    // SpecialAnimFour's …Damaged variant rides along for the health gate.
    let four = entry
        .building_anims
        .iter()
        .filter(|a| matches!(a.kind, BuildingAnimKind::Special))
        .nth(3)
        .unwrap();
    assert_eq!(
        four.damaged_variant.as_ref().map(|v| v.anim_type.as_str()),
        Some("NATBNK_B2D")
    );
}

#[test]
fn active_anim_garrisoned_replaces_base_slot_not_extra_overlay() {
    let ini: IniFile = IniFile::from_str(
        "[CAWASH19]\nActiveAnim=CAWA19_A\nActiveAnimGarrisoned=CAWA19_AG\nActiveAnimDamaged=CAWA19_AD\n",
    );
    let reg: ArtRegistry = ArtRegistry::from_ini(&ini);
    let entry: &ArtEntry = reg.get("CAWASH19").expect("CAWASH19 exists");

    assert_eq!(entry.building_anims.len(), 1);
    assert_eq!(entry.building_anims[0].anim_type, "CAWA19_A");
    assert_eq!(
        entry.building_anims[0]
            .garrisoned_variant
            .as_ref()
            .map(|v| v.anim_type.as_str()),
        Some("CAWA19_AG")
    );
    assert_eq!(
        entry.building_anims[0]
            .damaged_variant
            .as_ref()
            .map(|v| v.anim_type.as_str()),
        Some("CAWA19_AD")
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
    let anim = &entry.building_anims[0];
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
fn test_no_building_anims_for_regular_entry() {
    let ini: IniFile = IniFile::from_str("[E1]\nVoxel=no\n");
    let reg: ArtRegistry = ArtRegistry::from_ini(&ini);
    let entry: &ArtEntry = reg.get("E1").expect("E1 exists");
    assert!(entry.building_anims.is_empty());
}

#[test]
fn test_parse_turret_offset() {
    let ini: IniFile = IniFile::from_str("[HTK]\nVoxel=yes\nTurretOffset=-80\n");
    let reg: ArtRegistry = ArtRegistry::from_ini(&ini);
    let entry: &ArtEntry = reg.get("HTK").expect("HTK exists");
    assert_eq!(entry.turret_offset, -80);
}

#[test]
fn parses_building_fire_pixel_offsets() {
    let ini: IniFile = IniFile::from_str(
        "[ATESLA]\nPrimaryFirePixelOffset=11,-26\nSecondaryFirePixelOffset=-4,9\nPrimaryFireDualOffset=true\n",
    );
    let reg: ArtRegistry = ArtRegistry::from_ini(&ini);
    let entry: &ArtEntry = reg.get("ATESLA").expect("ATESLA exists");
    assert_eq!(entry.primary_fire_pixel_offset, Some((11, -26)));
    assert_eq!(entry.secondary_fire_pixel_offset, Some((-4, 9)));
    assert!(entry.primary_fire_dual_offset);
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
fn parses_add_occupy_from_ini() {
    let ini: IniFile =
        IniFile::from_str("[GAREFN]\nAddOccupy1=-1,0\nAddOccupy2=-1,-1\nRemoveOccupy1=3,1\n");
    let registry: ArtRegistry = ArtRegistry::from_ini(&ini);
    let entry: &ArtEntry = registry.get("GAREFN").expect("GAREFN");
    assert_eq!(entry.add_occupy[0], Some((-1, 0)));
    assert_eq!(entry.add_occupy[1], Some((-1, -1)));
    assert!(entry.add_occupy[2..].iter().all(Option::is_none));
    assert_eq!(entry.remove_occupy[0], Some((3, 1)));
    assert!(entry.remove_occupy[1..].iter().all(Option::is_none));
}

#[test]
fn add_remove_occupy_empty_when_no_keys() {
    let ini: IniFile = IniFile::from_str("[FOO]\nFoundation=2x2\n");
    let registry: ArtRegistry = ArtRegistry::from_ini(&ini);
    let entry: &ArtEntry = registry.get("FOO").expect("FOO");
    assert!(entry.add_occupy.iter().all(Option::is_none));
    assert!(entry.remove_occupy.iter().all(Option::is_none));
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
fn parses_anim_smudge_flags() {
    let ini = IniFile::from_bytes(
        b"[ANIMA]\n\
          Scorch=yes\n\
          \n\
          [ANIMB]\n\
          Crater=yes\n\
          ForceBigCraters=yes\n\
          \n\
          [ANIMC]\n\
          FixtureOnly=1\n",
    )
    .unwrap();
    let reg = ArtRegistry::from_ini(&ini);
    let a = reg.get("ANIMA").unwrap();
    assert!(a.scorch);
    assert!(!a.crater);
    let b = reg.get("ANIMB").unwrap();
    assert!(!b.scorch);
    assert!(b.crater);
    assert!(b.force_big_craters);
    let c = reg.get("ANIMC").unwrap();
    assert!(!c.scorch);
    assert!(!c.crater);
    assert!(!c.force_big_craters);
}

#[test]
fn parse_gaairc_four_pads() {
    let ini: IniFile = IniFile::from_str(
        "[GAAIRC]\n\
         DockingOffset0=0,-128,0\n\
         DockingOffset1=0,128,0\n\
         DockingOffset2=256,-128,0\n\
         DockingOffset3=256,128,0\n",
    );
    let reg: ArtRegistry = ArtRegistry::from_ini(&ini);
    let entry: &ArtEntry = reg.get("GAAIRC").expect("GAAIRC entry");
    assert_eq!(entry.pads.len(), 4, "should parse all 4 offsets");
    assert_eq!(entry.pads[0].lepton_offset, (0, -128, 0));
    assert_eq!(entry.pads[1].lepton_offset, (0, 128, 0));
    assert_eq!(entry.pads[2].lepton_offset, (256, -128, 0));
    assert_eq!(entry.pads[3].lepton_offset, (256, 128, 0));
}

#[test]
fn parse_no_docking_offsets_yields_empty_vec() {
    let ini: IniFile = IniFile::from_str("[GAHPAD]\nHeight=1\n");
    let reg: ArtRegistry = ArtRegistry::from_ini(&ini);
    let entry: &ArtEntry = reg.get("GAHPAD").expect("GAHPAD entry");
    assert!(entry.pads.is_empty(), "no offsets → empty pads vec");
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
fn test_sequence_frames_default_none_when_sequence_missing() {
    // No Sequence= key -> no lookup -> None for all three.
    let ini: IniFile = IniFile::from_str("[CIVHOSP]\nCameo=HOSPICON\n");
    let reg: ArtRegistry = ArtRegistry::from_ini(&ini);
    let entry: &ArtEntry = reg.get("CIVHOSP").expect("entry");
    assert_eq!(entry.deploy_frames, None);
    assert_eq!(entry.undeploy_frames, None);
    assert_eq!(entry.deployed_fire_frames, None);
}

#[test]
fn test_sequence_frames_partial_some_missing() {
    // Sequence section exists but only defines a subset of the three keys.
    let ini: IniFile = IniFile::from_str(
        "[E1]\n\
         Sequence=GISequence\n\
         \n\
         [GISequence]\n\
         Deploy=100,8,0\n",
    );
    let reg: ArtRegistry = ArtRegistry::from_ini(&ini);
    let entry: &ArtEntry = reg.get("E1").expect("E1 entry");
    assert_eq!(entry.deploy_frames, Some(8));
    assert_eq!(entry.undeploy_frames, None);
    assert_eq!(entry.deployed_fire_frames, None);
}

#[test]
fn test_parse_sequence_frames_helper() {
    assert_eq!(parse_sequence_frames("300,15,0"), Some(15));
    assert_eq!(parse_sequence_frames(" 8 , 6 , 6 "), Some(6));
    assert_eq!(parse_sequence_frames("only-one"), None);
    assert_eq!(parse_sequence_frames("a,b,c"), None);
    assert_eq!(parse_sequence_frames(""), None);
}

#[test]
fn gsi_05_10_delayed_building_fire_art_fields_keep_defaults_and_signed_delay() {
    let ini = IniFile::from_str(
        "[DEFAULTED]\nHeight=1\n\
         [SIGNED]\nIsAnimDelayedFire=yes\nDelayedFireDelay=-7\n",
    );
    let registry = ArtRegistry::from_ini(&ini);

    let defaulted = registry.get("DEFAULTED").expect("defaulted art entry");
    assert!(!defaulted.is_anim_delayed_fire);
    assert_eq!(defaulted.delayed_fire_delay, 0);

    let signed = registry.get("SIGNED").expect("signed art entry");
    assert!(signed.is_anim_delayed_fire);
    assert_eq!(signed.delayed_fire_delay, -7);
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
