//! `RateDown.*` reads (`Ext/TechnoType/Body.cpp:1049-1052`).
use super::PhobosRateDown;
use crate::rules::ini_parser::IniFile;

#[test]
fn rate_down_defaults_match_the_constructor() {
    let set = PhobosRateDown::from_section(IniFile::from_str("[GUN]\n").section_or_empty("GUN"));
    assert_eq!(set, PhobosRateDown::default());
    assert_eq!((set.delay, set.reset), (0, false));
    assert_eq!((set.cover_value, set.cover_ammo_below), (0, -2));
}

#[test]
fn rate_down_reads_delay_reset_and_cover() {
    let set = PhobosRateDown::from_section(
        IniFile::from_str(
            "[GUN]\nRateDown.Delay=5\nRateDown.Reset=yes\n\
             RateDown.Cover.Value=8\nRateDown.Cover.AmmoBelow=1\n",
        )
        .section_or_empty("GUN"),
    );
    assert_eq!(set.delay, 5);
    assert!(set.reset);
    assert_eq!(set.cover_value, 8);
    assert_eq!(set.cover_ammo_below, 1);
    assert_eq!(set.rate_down(3, 1), 8, "ammo at the threshold is covered");
    assert_eq!(set.rate_down(3, 2), 3, "ammo above keeps the stock rate");
}
