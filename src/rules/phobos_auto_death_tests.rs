//! The `AutoDeath.Behavior`/`AutoDeath.AfterDelay` reads
//! (`Ext/TechnoType/Body.cpp:817,825`) with the `Nullable` unset state that
//! turns the whole mechanism off (`Ext/Techno/Body.Update.cpp:142`).
use super::{AutoDeathBehavior, AutoDeathPowerStatus, AutoDeathTypeSettings};
use crate::rules::ini_parser::IniFile;

fn settings(text: &str) -> AutoDeathTypeSettings {
    let ini = IniFile::from_str(text);
    // An entryless `[TANK]` is discarded by the loader, and a section that
    // never existed reads the same way: every reader answers its default.
    AutoDeathTypeSettings::from_section(ini.section_or_empty("TANK"))
}

#[test]
fn an_absent_blank_or_unknown_behavior_leaves_the_mechanism_off() {
    // `_strcmpi` names, in any case (`Utilities/TemplateDef.h:1069-1071`).
    assert_eq!(
        settings("[TANK]\nAutoDeath.Behavior=KILL\n").behavior,
        Some(AutoDeathBehavior::Kill)
    );
    assert_eq!(
        settings("[TANK]\nAutoDeath.Behavior=sell\n").behavior,
        Some(AutoDeathBehavior::Sell)
    );
    assert_eq!(
        settings("[TANK]\nAutoDeath.Behavior=Vanish\n").behavior,
        Some(AutoDeathBehavior::Vanish)
    );

    // Absent, blank and unknown all fail `ReadString`/the name table, so the
    // `Nullable` stays unset and `CheckDeathConditions` returns at its first
    // test. `AfterDelay` alone must not arm anything.
    for text in [
        "[TANK]\n",
        "[TANK]\nAutoDeath.Behavior=\n",
        "[TANK]\nAutoDeath.Behavior=explode\n",
    ] {
        let s = settings(text);
        assert_eq!(s.behavior, None, "{text:?}");
        assert_eq!(s.after_delay, 0, "{text:?}");
    }
    assert_eq!(
        settings("[TANK]\nAutoDeath.AfterDelay=30\n").after_delay,
        30,
        "the countdown reads independently of the behavior switch"
    );

    // `:166`, `:210-221`, `:223-234` and the `PowerStatus` name table at
    // `Utilities/TemplateDef.h:1766-1773`, which accepts `consumer` for `low`
    // and `normal` for `full`.
    let full = settings(
        "[TANK]\nAutoDeath.Behavior=kill\nAutoDeath.OnAmmoDepletion=yes\n\
         AutoDeath.PlayerPowerState=normal\nAutoDeath.PlayerMoney.Max=10\n\
         AutoDeath.PlayerMoney.Min=5\n",
    );
    assert!(full.on_ammo_depletion);
    assert_eq!(full.player_power_state, AutoDeathPowerStatus::Full);
    assert_eq!((full.player_money_max, full.player_money_min), (10, 5));

    let low = settings("[TANK]\nAutoDeath.PlayerPowerState=consumer\n");
    assert_eq!(low.player_power_state, AutoDeathPowerStatus::Low);
    // Unset and unknown both keep the default `none`; the money bounds keep
    // their `-1` off value rather than the generic zero.
    let spare = settings("[TANK]\nAutoDeath.PlayerPowerState=sideways\n");
    assert_eq!(spare.player_power_state, AutoDeathPowerStatus::None);
    assert_eq!((spare.player_money_max, spare.player_money_min), (-1, -1));
}
