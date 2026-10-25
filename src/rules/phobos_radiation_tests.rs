//! Phobos custom radiation type reads and effective resolution.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;

fn catalog(ini: &str) -> RadiationTypeCatalog {
    let mut catalog = RadiationTypeCatalog::with_default();
    catalog.apply_pass(&IniFile::from_str(ini));
    catalog
}

#[test]
fn registry_walk_reads_duplicate_keys_through_name_reader() {
    let stored = "B".repeat(31);
    let incoming = format!("{stored}x");
    let catalog = catalog(&format!(
        "[RadiationTypes]\nROOT=SHORT\nROOT={incoming}\nSHOW=TAIL\n"
    ));
    assert_eq!(
        catalog
            .entries()
            .iter()
            .map(|entry| entry.name())
            .collect::<Vec<_>>(),
        ["Radiation", stored.as_str(), stored.as_str(), "TAIL"]
    );
}

#[test]
fn list_appends_entries_after_the_seeded_default() {
    let catalog = catalog("[RadiationTypes]\n0=ToxicField\n1=Second\n");
    assert_eq!(
        catalog
            .entries()
            .iter()
            .map(|e| e.name())
            .collect::<Vec<_>>(),
        ["Radiation", "ToxicField", "Second"],
        "AddDefaults seeds Radiation first and the pass list appends"
    );
    assert!(!catalog.entries().is_empty());
}

#[test]
fn absent_keys_fall_back_to_globals_and_present_keys_override() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[Radiation]\nRadLevelMax=250\nRadLevelFactor=0.5\nRadColor=0,255,0\n",
    ))
    .expect("rules");
    let catalog = catalog("[RadiationTypes]\n0=ToxicField\n[ToxicField]\nRadLevelFactor=1.0\n");
    let effective = catalog.effective(&rules.radiation, Some("ToxicField"));
    assert_eq!(effective.level_factor, 1.0, "the type's own read wins");
    assert_eq!(effective.level_max, 250, "an absent key keeps the global");
    assert_eq!(
        effective.color,
        (0, 255, 0),
        "an absent key keeps the global"
    );
    // The default entry read the vanilla [Radiation] section, so its
    // overrides equal the globals.
    let default = catalog.effective(&rules.radiation, None);
    assert_eq!(default.level_factor, 0.5);
    assert_eq!(default.level_max, 250);
}

#[test]
fn a_later_pass_retains_earlier_reads_and_replaces_present_keys() {
    let mut catalog = RadiationTypeCatalog::with_default();
    catalog.apply_pass(&IniFile::from_str(
        "[RadiationTypes]\n0=ToxicField\n[ToxicField]\nRadLevelFactor=1.0\n",
    ));
    catalog.apply_pass(&IniFile::from_str("[ToxicField]\nRadLevelMax=250\n"));
    let rules = RuleSet::from_ini(&IniFile::from_str("")).expect("rules");
    let effective = catalog.effective(&rules.radiation, Some("ToxicField"));
    assert_eq!(
        effective.level_factor, 1.0,
        "an omitted key retains the earlier pass's read"
    );
    assert_eq!(effective.level_max, 250, "a present key replaces it");
}

#[test]
fn unknown_and_absent_names_bind_the_default_entry() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[Radiation]\nRadLevelFactor=0.5\n[RadiationTypes]\n0=ToxicField\n",
    ))
    .expect("rules");
    let catalog = rules_radiation_types(&rules);
    for requested in [None, Some("NoSuchType")] {
        let effective = catalog.effective(&rules.radiation, requested);
        assert_eq!(
            effective.level_factor, 0.5,
            "{requested:?} binds the default Radiation entry"
        );
    }
}

#[test]
fn long_names_store_truncated_and_distinct_long_keys_stay_separate() {
    let long_a = "Aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let long_b = "Aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaab";
    assert!(long_a.len() > NAME_PAYLOAD_BYTES);
    let catalog = catalog(&format!("[RadiationTypes]\n0={long_a}\n1={long_b}\n"));
    let entries = catalog.entries();
    assert_eq!(entries.len(), 3, "both long keys allocate their own slot");
    assert_eq!(entries[1].name().len(), NAME_PAYLOAD_BYTES);
    assert_eq!(
        entries[1].name(),
        entries[2].name(),
        "the stored names collide, as native Name[32] truncation does"
    );
    // The full incoming key never matches a stored truncated name, so the
    // lookup keeps failing and each pass would allocate again.
    assert!(catalog.entry(long_a).is_none());
}

#[test]
fn lookup_is_case_insensitive_and_retains_the_first_spelling() {
    let catalog = catalog("[RadiationTypes]\n0=ToxicField\n");
    assert_eq!(
        catalog.entry("toxicfield").map(|e| e.name()),
        Some("ToxicField")
    );
}

#[test]
fn effective_resolves_every_documented_key() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[Radiation]\nRadLevelFactor=0.5\nRadApplicationDelay=16\n",
    ))
    .expect("rules");
    let catalog = catalog(
        "[RadiationTypes]\n0=ToxicField\n\
         [ToxicField]\n\
         RadDurationMultiple=3\n\
         RadApplicationDelay=32\n\
         RadApplicationDelay.Building=4\n\
         RadBuildingDamageMaxCount=2\n\
         RadLevelMax=250\n\
         RadLevelDelay=45\n\
         RadLightDelay=12\n\
         RadLevelFactor=1.0\n\
         RadLightFactor=0.25\n\
         RadTintFactor=0.5\n\
         RadColor=128,0,128\n\
         RadSiteWarhead=Toxic\n\
         RadSiteWarhead.Detonate=yes\n\
         RadSiteWarhead.Detonate.Full=no\n\
         RadHasOwner=yes\n\
         RadHasInvoker=yes\n",
    );
    let effective = catalog.effective(&rules.radiation, Some("ToxicField"));
    assert_eq!(effective.duration_multiple, 3);
    assert_eq!(effective.application_delay, 32);
    assert_eq!(effective.application_delay_building, 4);
    assert_eq!(effective.building_damage_max_count, 2);
    assert_eq!(effective.level_max, 250);
    assert_eq!(effective.level_delay, 45);
    assert_eq!(effective.light_delay, 12);
    assert_eq!(effective.level_factor, 1.0);
    assert_eq!(effective.light_factor, SimFixed::from_num(0.25_f32));
    assert_eq!(effective.tint_factor, SimFixed::from_num(0.5_f32));
    assert_eq!(effective.color, (128, 0, 128));
    assert_eq!(effective.site_warhead, "Toxic");
    assert!(effective.site_warhead_detonate);
    assert!(!effective.site_warhead_detonate_full);
    assert!(effective.has_owner);
    assert!(effective.has_invoker);
}

#[test]
fn global_radiation_keys_read_with_phobos_defaults() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[Radiation]\n\
         UseGlobalRadApplicationDelay=false\n\
         RadApplicationDelay.Building=5\n\
         RadBuildingDamageMaxCount=3\n\
         RadSiteWarhead.Detonate=yes\n\
         RadSiteWarhead.Detonate.Full=no\n\
         RadHasOwner=yes\n\
         RadHasInvoker=yes\n",
    ))
    .expect("rules");
    let radiation = &rules.radiation;
    assert!(!radiation.use_global_rad_application_delay);
    assert_eq!(radiation.application_delay_building, 5);
    assert_eq!(radiation.building_damage_max_count, 3);
    assert!(radiation.site_warhead_detonate);
    assert!(!radiation.site_warhead_detonate_full);
    assert!(radiation.has_owner);
    assert!(radiation.has_invoker);
    // Absent keys keep the native defaults.
    let stock = RuleSet::from_ini(&IniFile::from_str("[Radiation]\nRadLevelMax=500\n"))
        .expect("stock rules");
    let stock = &stock.radiation;
    assert!(stock.use_global_rad_application_delay);
    assert_eq!(stock.application_delay_building, 0);
    assert_eq!(stock.building_damage_max_count, -1);
    assert!(!stock.site_warhead_detonate);
    assert!(stock.site_warhead_detonate_full);
    assert!(!stock.has_owner);
    assert!(!stock.has_invoker);
}

#[test]
fn projection_carries_the_catalog_and_the_weapon_binding() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[Radiation]\nRadLevelMax=250\n\
         [RadiationTypes]\n0=ToxicField\n\
         [ToxicField]\nRadLevelFactor=1.0\nRadColor=128,0,128\n\
         [InfantryTypes]\n0=DESO\n\
         [DESO]\nStrength=200\nPrimary=RadEruptionWeapon\n\
         [RadEruptionWeapon]\nRadLevel=500\nRadType=ToxicField\n",
    ))
    .expect("rules");
    let weapon = rules.weapon("RadEruptionWeapon").expect("weapon");
    assert_eq!(weapon.rad_type.as_deref(), Some("ToxicField"));
    let effective = effective_radiation(&rules, weapon.rad_type.as_deref());
    assert_eq!(effective.level_factor, 1.0, "the bound type's read");
    assert_eq!(effective.level_max, 250, "the global fallback");
    assert_eq!(effective.color, (128, 0, 128));
    // An unbound weapon resolves the default entry.
    assert_eq!(effective_radiation(&rules, None).level_factor, 0.2);
}

#[test]
fn an_unknown_bound_name_resolves_the_default_entry() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[Radiation]\nRadLevelFactor=0.5\n\
         [RadiationTypes]\n0=ToxicField\n\
         [InfantryTypes]\n0=DESO\n\
         [DESO]\nStrength=200\nPrimary=W\n\
         [W]\nRadLevel=100\nRadType=NoSuchType\n",
    ))
    .expect("rules");
    let weapon = rules.weapon("W").expect("weapon");
    assert_eq!(weapon.rad_type.as_deref(), Some("NoSuchType"));
    assert_eq!(
        effective_radiation(&rules, weapon.rad_type.as_deref()).level_factor,
        0.5,
        "an unknown RadType keeps the constructor's default binding"
    );
}

#[test]
fn the_catalog_survives_a_processed_rules_round_trip() {
    use crate::rules::native_processing::RulesLayerStack;
    let layers = RulesLayerStack::new(IniFile::from_str(
        "[RadiationTypes]\n0=ToxicField\n[ToxicField]\nRadLevelFactor=1.0\n",
    ))
    .process()
    .expect("process");
    let bytes = crate::util::save_codec::serialize(&layers).expect("serialize");
    let restored: crate::rules::native_processing::ProcessedRulesLayers =
        crate::util::save_codec::deserialize(&bytes).expect("deserialize");
    let rules = RuleSet::from_processed_rules(&restored).expect("project");
    assert_eq!(
        effective_radiation(&rules, Some("ToxicField")).level_factor,
        1.0,
        "the retained catalog crosses the save boundary"
    );
}

/// The projected catalog of a rules value, for tests that hold one.
fn rules_radiation_types(rules: &RuleSet) -> RadiationTypeCatalog {
    rules.radiation.radiation_types.clone()
}

/// The demo mod shape: retail-style `[Radiation]` globals, an appended
/// `[RadiationTypes]` list, two custom entries and a weapon that binds one.
const DEMO_MOD: &str = "[Radiation]\n\
RadDurationMultiple=1\t\t; frames per level\n\
RadApplicationDelay=16\n\
RadLevelMax=500\n\
RadLevelDelay=90\n\
RadLevelFactor=.2\n\
RadLightFactor=.1\n\
RadTintFactor=1.0\n\
RadColor=0,255,0\n\
RadSiteWarhead=RadSite\n\
\n\
[RadEruptionWeapon]\n\
Damage=1\t\t; comment\n\
ROF=60\n\
Range=4\n\
AreaFire=yes\n\
Warhead=RadEruptionWarhead\n\
RadLevel=500\n\
\n\
; Fire by Prism Cannon\n\
RadType=PurpleRad\n\
\n\
[InfantryTypes]\n\
0=DESO\n\
\n\
[DESO]\n\
Strength=150\n\
Secondary=RadEruptionWeapon\n\
Deployer=yes\n\
\n\
[RadiationTypes]\n\
0=GreenRad\n\
1=PurpleRad\n\
\n\
[GreenRad]\n\
RadDurationMultiple=2\n\
RadLevelMax=250\n\
RadLevelFactor=0.5\n\
RadLightFactor=0.2\n\
RadColor=0,255,0\n\
\n\
[PurpleRad]\n\
RadDurationMultiple=3\n\
RadLevelMax=400\n\
RadLevelFactor=0.1\n\
RadLightFactor=0.3\n\
RadTintFactor=1.5\n\
RadColor=128,0,255\n";

#[test]
fn a_weapon_binds_its_rad_type_through_the_ordered_rules_read() {
    let rules = RuleSet::from_ini(&IniFile::from_str(DEMO_MOD)).expect("demo rules");
    let weapon = rules.weapon("RadEruptionWeapon").expect("bound weapon");
    assert_eq!(
        weapon.rad_type.as_deref(),
        Some("PurpleRad"),
        "`RadType=` reads even after a trailing comment line in the section"
    );
    let deso = rules.object("DESO").expect("DESO");
    assert_eq!(deso.secondary().as_deref(), Some("RadEruptionWeapon"));
}

#[test]
fn the_demo_mods_purple_type_resolves_every_key_it_sets() {
    let rules = RuleSet::from_ini(&IniFile::from_str(DEMO_MOD)).expect("demo rules");
    let effective = effective_radiation(&rules, Some("purplerad"));
    assert_eq!(effective.duration_multiple, 3);
    assert_eq!(effective.level_max, 400);
    assert_eq!(
        effective.level_factor,
        f64::from(0.1_f32),
        "native ReadDouble scans through f32 before widening"
    );
    assert_eq!(effective.color, (128, 0, 255));
    assert!((effective.tint_factor.to_num::<f64>() - 1.5).abs() < 1e-4);
    assert!((effective.light_factor.to_num::<f64>() - 0.3).abs() < 1e-4);
    assert_eq!(
        effective.application_delay, 16,
        "keys the entry omits keep the [Radiation] global"
    );
}

#[test]
fn the_demo_mod_keeps_the_green_type_separate() {
    let rules = RuleSet::from_ini(&IniFile::from_str(DEMO_MOD)).expect("demo rules");
    let green = effective_radiation(&rules, Some("GreenRad"));
    assert_eq!(green.level_factor, 0.5);
    assert_eq!(green.color, (0, 255, 0));
    assert_eq!(green.duration_multiple, 2);
}
