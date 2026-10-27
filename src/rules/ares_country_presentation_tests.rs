//! Ares 3.0 CountryExt presentation defaults, reads and ParentCountry copy.

use super::*;
use crate::rules::ruleset::RuleSet;

fn processed(ini: &str) -> RuleSet {
    let processed = RulesLayerStack::new(IniFile::from_str(ini))
        .process()
        .expect("country presentation rules parse");
    RuleSet::from_processed_rules(&processed).expect("country presentation projection")
}

const COUNTRIES: &str = "\
[Countries]\n\
0=Americans\n1=Alliance\n2=French\n3=Germans\n4=British\n5=Africans\n6=Arabs\n\
7=Confederation\n8=Russians\n9=YuriCountry\n10=Custom\n";

/// `HouseTypeExt::Initialize @ 0x1001C0F0` selects its row by stored ID
/// (`0x1001C107..0x1001C135`), not by registry position.
#[test]
fn ares_country_defaults_match_the_installed_initializer() {
    let rules = processed("[Countries]\n0=YuriCountry\n1=C1\n2=Americans\n3=Custom\n[Custom]\n");
    assert_eq!(rules.country_flag("Americans"), Some("usai.pcx"));
    assert_eq!(
        rules.country_status_key("Americans"),
        Some("STT:PlayerSideAmerica")
    );
    assert_eq!(
        rules.country_taunt_pattern("Americans"),
        Some("taunts\\tauam%02i.wav")
    );
    assert_eq!(rules.country_flag("YuriCountry"), Some("yrii.pcx"));
    assert_eq!(
        rules.country_status_key("YuriCountry"),
        Some("STT:PlayerSideYuriCountry")
    );
    assert!(rules.country_observer_flag_alt_palette("YuriCountry"));
    // An unlisted ID at a stock position takes the unknown branch.
    assert_eq!(rules.country_flag("C1"), Some("rani.pcx"));
    // The unknown branch at 0x1001C413.
    assert_eq!(rules.country_flag("Custom"), Some("rani.pcx"));
    assert_eq!(rules.country_status_key("Custom"), Some("GUI:Unknown"));
    assert_eq!(
        rules.country_taunt_pattern("Custom"),
        Some("taunts\\tauam%02i.wav")
    );
    assert_eq!(rules.country_list_index("Custom"), 100);
    assert_eq!(rules.country_random_weight("Custom"), 1);
    assert_eq!(rules.country_ai_random_weight("Custom"), 1);
}

#[test]
fn ares_country_reads_flags_status_taunts_order_and_weights() {
    let rules = processed(&format!(
        "{COUNTRIES}\
         [Custom]\n\
         File.Flag=MyFlag.PCX\n\
         MenuText.Status=STT:Custom\n\
         File.Taunt=Taunts\\custom~~.wav\n\
         File.ObserverFlag=obsflag.shp\n\
         File.ObserverBackground=obsbkg.shp\n\
         File.ObserverFlagAltPalette=yes\n\
         ListIndex=3\n\
         RandomSelectionWeight=4\n\
         AIRandomSelectionWeight=7\n"
    ));
    // `0x10076470` lowercases the stored flag file.
    assert_eq!(rules.country_flag("Custom"), Some("myflag.pcx"));
    assert_eq!(rules.country_status_key("Custom"), Some("STT:Custom"));
    assert_eq!(
        rules.country_taunt_pattern("Custom"),
        Some("Taunts\\custom~~.wav")
    );
    assert_eq!(rules.country_observer_flag("Custom"), Some("obsflag.shp"));
    assert_eq!(
        rules.country_observer_background("Custom"),
        Some("obsbkg.shp")
    );
    assert!(rules.country_observer_flag_alt_palette("Custom"));
    assert_eq!(rules.country_list_index("Custom"), 3);
    assert_eq!(rules.country_random_weight("Custom"), 4);
    assert_eq!(rules.country_ai_random_weight("Custom"), 7);
}

/// The primary-Rules block `0x1001C770` runs before the one-shot copy
/// `0x1001BDC0`, so the parent overwrites the child's own file keys; only the
/// common weights and `ListIndex` read afterwards override it. The optional
/// AI weight is not copied.
#[test]
fn ares_country_parent_copy_overwrites_primary_keys_and_common_keys_override() {
    let rules = processed(
        "[Countries]\n0=Parent\n1=Child\n\
         [Parent]\n\
         File.Flag=parent.pcx\n\
         MenuText.Status=STT:Parent\n\
         ListIndex=7\n\
         RandomSelectionWeight=2\n\
         AIRandomSelectionWeight=9\n\
         [Child]\n\
         ParentCountry=Parent\n\
         File.Flag=child.pcx\n\
         File.Taunt=Taunts\\child~~.wav\n\
         RandomSelectionWeight=5\n",
    );
    assert_eq!(rules.country_flag("Child"), Some("parent.pcx"));
    assert_eq!(rules.country_status_key("Child"), Some("STT:Parent"));
    assert_eq!(
        rules.country_taunt_pattern("Child"),
        Some("taunts\\tauam%02i.wav")
    );
    assert_eq!(rules.country_list_index("Child"), 8);
    assert_eq!(rules.country_random_weight("Child"), 5);
    assert_eq!(rules.country_ai_random_weight("Child"), 5);
    assert_eq!(rules.country_ai_random_weight("Parent"), 9);
}

/// `File.Flag` (`0x1001C79D..0x1001C7E5`): empty keeps the current file;
/// `none` clears it, and the flag hook answers the cleared file with the
/// `rani.pcx` branch (`0x4E3567`).
#[test]
fn ares_country_empty_flag_retains_and_none_clears_to_rani() {
    let parsed = IniFile::from_str(COUNTRIES);
    let countries = parsed.section("Countries").unwrap().clone();
    let mut americans = IniSection::new("Americans".into());
    americans.set("File.Flag", "none");
    let mut alliance = IniSection::new("Alliance".into());
    alliance.set("File.Flag", "");
    let processed = RulesLayerStack::new(IniFile::from_sections_for_test([
        countries, americans, alliance,
    ]))
    .process()
    .expect("cleared flag rules parse");
    let rules = RuleSet::from_processed_rules(&processed).expect("cleared flag projection");
    assert_eq!(rules.country_flag("Americans"), None);
    assert_eq!(rules.country_insignia_pcx(0).as_deref(), Some("rani.pcx"));
    assert_eq!(rules.country_flag("Alliance"), Some("japi.pcx"));
    // Only indices outside the HouseType array keep the stock chain.
    assert_eq!(rules.country_insignia_pcx(11), None);
}

#[test]
fn ares_custom_country_insignia_replaces_the_stock_loading_chain() {
    let rules = processed(&format!("{COUNTRIES}[Custom]\nFile.Flag=MyFlag.PCX\n"));
    assert_eq!(
        rules.country_insignia_pcx(10).as_deref(),
        Some("myflag.pcx")
    );
    assert_eq!(rules.country_insignia_pcx(9).as_deref(), Some("yrii.pcx"));
}
