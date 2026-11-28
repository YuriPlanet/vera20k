//! The `HealthBar.*` reads (`Ext/TechnoType/Body.cpp:761-764`), all `no` by
//! default.
use super::HealthBarSettings;
use crate::rules::ini_parser::IniFile;

fn read(text: &str) -> HealthBarSettings {
    HealthBarSettings::from_section(IniFile::from_str(text).section_or_empty("TANK"))
}

#[test]
fn the_health_bar_switches_default_to_no_and_read_independently() {
    assert_eq!(read(""), HealthBarSettings::default());
    assert_eq!(read("[TANK]\n"), HealthBarSettings::default());

    let all = read(
        "[TANK]\nHealthBar.Hide=yes\nHealthBar.HidePips=yes\n\
         HealthBar.Permanent=yes\nHealthBar.Permanent.PipScale=yes\n",
    );
    assert!(all.hide && all.hide_pips && all.permanent && all.permanent_pip_scale);

    // One key set leaves the others at their default.
    let half = read("[TANK]\nHealthBar.Permanent=yes\n");
    assert!(!half.hide && !half.hide_pips && half.permanent && !half.permanent_pip_scale);
}
