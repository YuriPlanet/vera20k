//! `TabIndex=` on a `SuperWeaponType`.

use crate::rules::ini_parser::IniFile;
use crate::rules::phobos_sw_tab_index::PhobosSwTabIndex;

fn read(keys: &str) -> PhobosSwTabIndex {
    PhobosSwTabIndex::from_section(IniFile::from_str(&format!("[Q]\n{keys}")).section_or_empty("Q"))
}

/// `Ext/SWType/Body.h:187`: the constructor default is `1`.
#[test]
fn an_absent_key_defaults_to_the_defense_tab() {
    assert_eq!(read("").index, 1);
    assert_eq!(read("").index(), 1);
}

/// `Body.cpp:285-286`: `TabIndex` reads on its own and keeps 0..=3.
#[test]
fn every_valid_index_reads() {
    for value in 0..=3 {
        assert_eq!(
            read(&format!("TabIndex={value}\n")).index,
            value,
            "TabIndex={value}"
        );
    }
}

/// `GeneralUtils::IntValidCheck` (`Utilities/GeneralUtils.cpp:19-26`) resets
/// an out-of-range value to `defaultValue` — it does not clamp.
#[test]
fn an_out_of_range_index_resets_to_one_rather_than_clamping() {
    for value in [-1, 4, 5, 100, i32::MIN, i32::MAX] {
        assert_eq!(
            read(&format!("TabIndex={value}\n")).index,
            1,
            "TabIndex={value} resets to the default"
        );
    }
}

/// A blank read is `ReadInteger`'s default (`crt_atoi("")` is a concrete 0),
/// which `IntValidCheck` then resets to `1`.
#[test]
fn a_blank_read_resets_to_the_default() {
    assert_eq!(read("TabIndex=\n").index, 1);
}
