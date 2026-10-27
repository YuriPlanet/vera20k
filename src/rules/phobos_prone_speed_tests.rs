//! Phobos `ProneSpeed` resolution: the two `[General]` defaults, the
//! InfantryType key, and the `Crawls=` selection between them.
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;

fn rules(global: &str, techno: &str) -> RuleSet {
    let ini = format!("[General]\n{global}[InfantryTypes]\n0=TEST\n[TEST]\nStrength=100\n{techno}");
    RuleSet::from_ini(&IniFile::from_str(&ini)).expect("rules read")
}

/// `Crawls=` is an ARTMD key, so the type only takes the crawl multiplier when
/// the art registry projects it. `from_ini_with_fixed_art_for_test` merges the
/// art for the read side; the projection into `ObjectType::crawls` happens in
/// `install_art_data`, as it does in production.
fn rules_with_crawls(global: &str, techno: &str) -> RuleSet {
    let ini = format!("[General]\n{global}[InfantryTypes]\n0=TEST\n[TEST]\nStrength=100\n{techno}");
    let art = IniFile::from_str("[TEST]\nCrawls=yes\n");
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&IniFile::from_str(&ini), &art)
        .expect("rules with art read");
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
    rules
}

#[test]
fn prone_speed_follows_the_general_defaults_and_the_crawls_flag() {
    // Untouched rules: Phobos's own defaults, 0.67 crawling and 1.5 not.
    let plain = rules("", "");
    let ty = plain.object("TEST").expect("TEST type");
    assert!(!ty.crawls);
    assert_eq!(plain.prone_speed_for(ty), 1.5);

    let crawling = rules_with_crawls("", "");
    let ty = crawling.object("TEST").expect("TEST type");
    assert!(ty.crawls, "art Crawls=yes reaches the type");
    assert_eq!(crawling.prone_speed_for(ty), 0.67);
}

#[test]
fn the_general_keys_are_per_flag_and_the_type_key_wins() {
    let plain = rules("ProneSpeed.NoCrawls=1.25\nProneSpeed.Crawls=0.5\n", "");
    let ty = plain.object("TEST").expect("TEST type");
    assert_eq!(plain.prone_speed_for(ty), 1.25);

    let crawling = rules_with_crawls("ProneSpeed.Crawls=0.5\nProneSpeed.NoCrawls=1.25\n", "");
    let ty = crawling.object("TEST").expect("TEST type");
    assert_eq!(crawling.prone_speed_for(ty), 0.5);

    // The TechnoType key overrides whichever global its Crawls= would select.
    let both = rules_with_crawls(
        "ProneSpeed.Crawls=0.5\nProneSpeed.NoCrawls=1.25\n",
        "ProneSpeed=2.0\n",
    );
    let ty = both.object("TEST").expect("TEST type");
    assert_eq!(both.prone_speed_for(ty), 2.0);
}

/// Phobos holds the key in `InfantryTypeExt`, so a section outside
/// `[InfantryTypes]` carries no value even when it repeats the key.
#[test]
fn only_infantry_sections_carry_the_key() {
    let ini = IniFile::from_str(
        "[General]\nProneSpeed.NoCrawls=1.25\n[VehicleTypes]\n0=TANK\n[TANK]\nProneSpeed=2.0\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("rules read");
    let ty = rules.object("TANK").expect("TANK type");
    assert_eq!(rules.prone_speed_for(ty), 1.25);
}

/// Phobos reads the globals as `Valueable<double>`: a value `strtod` cannot
/// convert keeps the default, where stock ReadDouble would store 0.
#[test]
fn an_unparsable_global_keeps_the_phobos_default() {
    let plain = rules("ProneSpeed.NoCrawls=fast\n", "");
    let ty = plain.object("TEST").expect("TEST type");
    assert_eq!(plain.prone_speed_for(ty), 1.5);
}
