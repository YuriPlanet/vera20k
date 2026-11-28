//! `DrainMoneyDisplay.*` and the consumed `DisplayIncome.*` reads
//! (`Ext/TechnoType/Body.cpp:1165-1169`, `Ext/BuildingType/Body.cpp:216-217`).
use super::{DisplayIncomeSettings, DrainMoneyDisplaySettings};
use crate::rules::ini_parser::IniFile;
use crate::rules::phobos_shields::AffectedHouse;

#[test]
fn drain_money_display_defaults_match_the_constructors() {
    let set = DrainMoneyDisplaySettings::from_section(
        IniFile::from_str("[DISK]\n").section_or_empty("DISK"),
    );
    assert_eq!(set, DrainMoneyDisplaySettings::default());
    assert_eq!(
        (set.display, set.houses, set.offset),
        (None, None, [0, 0]),
        "an unset Nullable keeps the global fallback, the offset is Point2D::Empty"
    );
    assert_eq!(
        (set.on_target, set.on_target_use_display_income),
        (None, None)
    );
}

#[test]
fn drain_money_display_reads_house_offset_and_target_arms() {
    let set = DrainMoneyDisplaySettings::from_section(
        IniFile::from_str(
            "[DISK]\nDrainMoneyDisplay=yes\nDrainMoneyDisplay.Houses=owner,enemies\n\
             DrainMoneyDisplay.Offset=6,-4\nDrainMoneyDisplay.OnTarget=no\n\
             DrainMoneyDisplay.OnTarget.UseDisplayIncome=yes\n",
        )
        .section_or_empty("DISK"),
    );
    assert_eq!(set.display, Some(true));
    assert!(set.houses.unwrap().contains(AffectedHouse::OWNER));
    assert!(set.houses.unwrap().contains(AffectedHouse::ENEMIES));
    assert!(!set.houses.unwrap().contains(AffectedHouse::ALLIES));
    assert_eq!(set.offset, [6, -4]);
    assert_eq!(set.on_target, Some(false));
    assert_eq!(set.on_target_use_display_income, Some(true));

    // A one-token Point2D read changes only X, as `Read2Integers` does.
    let partial = DrainMoneyDisplaySettings::from_section(
        IniFile::from_str("[DISK]\nDrainMoneyDisplay.Offset=9\n").section_or_empty("DISK"),
    );
    assert_eq!(partial.offset, [9, 0]);

    // `<default>` clears a Nullable read, so the global answers again. A later
    // layer overrides the first pass's key (a duplicate key inside one section
    // would keep the first value).
    let mut layered = IniFile::from_str(
        "[DISK]\nDrainMoneyDisplay=yes\nDrainMoneyDisplay.Houses=all\n\
         DrainMoneyDisplay.OnTarget=yes\n",
    );
    layered.merge(&IniFile::from_str(
        "[DISK]\nDrainMoneyDisplay=<default>\nDrainMoneyDisplay.Houses=<default>\n\
         DrainMoneyDisplay.OnTarget=none\n",
    ));
    let cleared = DrainMoneyDisplaySettings::from_section(layered.section_or_empty("DISK"));
    assert_eq!(cleared.display, None);
    assert_eq!(cleared.houses, None);
    assert_eq!(cleared.on_target, None);
}

#[test]
fn display_income_reads_only_the_houses_and_offset_the_drain_label_uses() {
    let set = DisplayIncomeSettings::from_section(
        IniFile::from_str("[NAREFN]\n").section_or_empty("NAREFN"),
    );
    assert_eq!(set, DisplayIncomeSettings::default());
    assert_eq!((set.houses, set.offset), (None, [0, 0]));

    let set = DisplayIncomeSettings::from_section(
        IniFile::from_str("[NAREFN]\nDisplayIncome.Houses=allies\nDisplayIncome.Offset=-3,2\n")
            .section_or_empty("NAREFN"),
    );
    assert_eq!(set.houses, Some(AffectedHouse::ALLIES));
    assert_eq!(set.offset, [-3, 2]);
}
