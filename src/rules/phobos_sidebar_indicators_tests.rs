//! The indicator keys' native reads: `uimd.ini [Sidebar]`/`[ToolTips]` and the
//! per-side `rulesmd.ini [<Side>]` pass, each with its constructor default.
use super::{
    PowerDeltaSideSettings, PowerDeltaUi, SidebarIndicatorsUi, SidebarTextAlign,
    load_string_or_default,
};
use crate::rules::ini_parser::IniFile;

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
