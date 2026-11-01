//! The indicator keys' native reads: `uimd.ini [Sidebar]`/`[ToolTips]` and the
//! per-side `rulesmd.ini [<Side>]` pass, each with its constructor default.
use super::{
    PowerDeltaSideSettings, PowerDeltaUi, SidebarIndicatorsUi, SidebarTextAlign,
    load_string_or_default,
};
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;

fn uimd(text: &str) -> SidebarIndicatorsUi {
    SidebarIndicatorsUi::from_ini(&IniFile::from_str(text), &|label| {
        (label == "UI:PHOBOS:PowerLabel").then(|| "PWR".to_string())
    })
}

#[test]
fn the_sidebar_switch_and_thresholds_default_to_off_and_the_source_fractions() {
    let absent = PowerDeltaUi::default();
    assert!(
        !absent.show,
        "PowerDelta.Show defaults to no: nothing is drawn by default"
    );
    assert_eq!((absent.condition_yellow, absent.condition_red), (0.75, 1.0));
    assert!(
        !uimd("[General]\n").power_delta.show,
        "a uimd file with no [Sidebar] keeps the switch off"
    );

    let on = uimd(
        "[Sidebar]\nPowerDelta.Show=yes\nPowerDelta.ConditionYellow=75%\n\
         PowerDelta.ConditionRed=100%\n",
    );
    assert!(on.power_delta.show);
    // ReadDouble scales a value holding '%' by the binary64 0.01.
    assert_eq!(on.power_delta.condition_yellow, 0.75);
    assert_eq!(on.power_delta.condition_red, 1.0);
}

#[test]
fn the_labels_fall_back_to_the_glyphs_unless_the_key_resolves() {
    // Absent section, absent key and `<none>` all keep the literal glyph.
    for text in [
        "",
        "[Sidebar]\n",
        "[ToolTips]\nPowerLabel=<none>\nPowerBlackoutLabel=\n",
    ] {
        let ui = uimd(text);
        assert_eq!(ui.power_label, "\u{26A1}");
        assert_eq!(ui.power_blackout_label, "\u{26A1}\u{274C}");
    }

    // A key the string table holds is resolved at load, as Phobos does.
    let ui = uimd("[ToolTips]\nPowerLabel=UI:PHOBOS:PowerLabel\n");
    assert_eq!(ui.power_label, "PWR");
    // An unknown key keeps the glyph default (VERA's documented stand-in for
    // native's MISSING: text).
    assert_eq!(
        uimd("[ToolTips]\nPowerLabel=UI:NO:SUCH\n").power_label,
        "\u{26A1}"
    );
}

#[test]
fn a_blank_label_never_reaches_the_string_table() {
    // IsValidString() rejects empty and `<none>` before LoadString is asked.
    let ini = IniFile::from_str("[ToolTips]\nPowerLabel= \nPowerBlackoutLabel=Other\n");
    let section = ini.section("ToolTips").expect("section");
    let asked = std::cell::Cell::new(false);
    let resolved = load_string_or_default(section, "PowerLabel", "D", &|_| {
        asked.set(true);
        Some("X".to_string())
    });
    assert_eq!(resolved, "D");
    assert!(!asked.get(), "a blank value must not query the table");
}

#[test]
fn the_side_settings_read_their_own_defaults_and_offset() {
    let ini = IniFile::from_str(
        "[North]\nSidebar.PowerDelta.Offset=-30,4\n\
         Sidebar.PowerDelta.ColorGreen=1,2,3\nSidebar.PowerDelta.Align=centre\n\
         [South]\nName=South\n",
    );
    let north = PowerDeltaSideSettings::from_section(ini.section("North").expect("section"));
    assert_eq!(north.offset, [-30, 4]);
    assert_eq!(north.color_green, [1, 2, 3]);
    assert_eq!(
        north.color_yellow,
        [255, 255, 0],
        "untouched keys stay default"
    );
    assert_eq!(north.color_red, [255, 0, 0]);
    assert_eq!(north.color_grey, [128, 128, 128]);
    assert_eq!(north.align, SidebarTextAlign::Center, "centre is Center");

    let south = PowerDeltaSideSettings::from_section(ini.section("South").expect("section"));
    assert_eq!(south, PowerDeltaSideSettings::default());

    // Rejected alignment text retains the default rather than clearing it.
    let bad = IniFile::from_str("[North]\nSidebar.PowerDelta.Align=sideways\n");
    assert_eq!(
        PowerDeltaSideSettings::from_section(bad.section("North").expect("section")).align,
        SidebarTextAlign::Left
    );
}

/// `uimd.ini [Sidebar] HarvesterCounter.*` and the side's own
/// `Sidebar.HarvesterCounter.*` (`Ext/Side/Body.cpp:33-38`, `Body.h:29-34`).
#[test]
fn the_harvester_counter_reads_both_layers_with_its_own_defaults() {
    let defaults = uimd("[Sidebar]\nHarvesterCounter.Show=yes\n");
    assert!(defaults.harvester.show);
    assert_eq!(defaults.harvester.label, "\u{26CF}");
    assert_eq!(
        (
            defaults.harvester.condition_yellow,
            defaults.harvester.condition_red
        ),
        (0.99, 0.5)
    );
    assert!(
        !uimd("").harvester.show,
        "the switch defaults to no without uimd"
    );

    let tuned = uimd(
        "[Sidebar]\nHarvesterCounter.Show=yes\nHarvesterCounter.Label=UI:PHOBOS:PowerLabel\n\
         HarvesterCounter.ConditionYellow=75%\nHarvesterCounter.ConditionRed=50%\n",
    );
    // The label key resolves through the string table, like LoadStringOrDefault.
    assert_eq!(tuned.harvester.label, "PWR");
    assert_eq!(tuned.harvester.condition_yellow, 0.75);
    assert_eq!(tuned.harvester.condition_red, 0.5);

    let ini = IniFile::from_str(
        "[North]\nSidebar.HarvesterCounter.Offset=12,-8\n\
         Sidebar.HarvesterCounter.HideMaxValue=yes\n\
         Sidebar.HarvesterCounter.ColorYellow=1,2,3\n",
    );
    let side =
        super::HarvesterCounterSideSettings::from_section(ini.section("North").expect("section"));
    assert_eq!(side.offset, [12, -8]);
    assert!(side.hide_max_value);
    assert!(!side.only_max_value);
    assert_eq!(side.color_yellow, [1, 2, 3]);
    assert_eq!(side.color_red, [255, 0, 0]);
    assert_eq!(
        side.color_green, None,
        "ColorGreen is a Nullable: unset falls back to the tooltip colour"
    );
    assert_eq!(
        super::HarvesterCounterSideSettings::default(),
        super::HarvesterCounterSideSettings {
            offset: [0, 0],
            hide_max_value: false,
            only_max_value: false,
            color_green: None,
            color_yellow: [255, 255, 0],
            color_red: [255, 0, 0],
        }
    );
}

/// Phobos `[TechnoType] Harvester.Counted=` with its two fallbacks, which run in
/// the source's read order: the explicit value, then `Enslaves=` for any
/// TechnoType, then `Harvester=` for a UnitType only.
#[test]
fn harvester_counted_falls_back_in_the_sources_read_order() {
    let ini = IniFile::from_str(
        "[VehicleTypes]\n0=WMIN\n1=FREE\n2=EXCL\n[BuildingTypes]\n0=DERICK\n1=YARD\n\
         [WMIN]\nStrength=600\nHarvester=yes\n\
         [FREE]\nStrength=600\nEnslaves=SMIN\n\
         [EXCL]\nStrength=600\nHarvester=yes\nHarvester.Counted=no\n\
         [DERICK]\nStrength=1000\nHarvester.Counted=yes\n\
         [YARD]\nStrength=1000\n\
         [SMIN]\nStrength=600\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("rules read");
    assert!(
        rules.object("WMIN").unwrap().harvester_counted(),
        "the UnitType pass falls back to Harvester=yes"
    );
    assert!(
        rules.object("FREE").unwrap().harvester_counted(),
        "the TechnoType pass falls back to Enslaves="
    );
    assert!(
        !rules.object("EXCL").unwrap().harvester_counted(),
        "an explicit no overrides both fallbacks"
    );
    assert!(
        rules.object("DERICK").unwrap().harvester_counted(),
        "a building counts only when the key says so"
    );
    assert!(
        !rules.object("YARD").unwrap().harvester_counted(),
        "a building with no key and no Enslaves= is left out"
    );
}
