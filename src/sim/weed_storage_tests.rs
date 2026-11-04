//! Regression tests for the House+0x314 weed store chain: the `[General]
//! WeedCapacity` reader and `HouseClass::GiveWeed @ 0x004F9700`. The
//! production integration at the refinery unload gate is pinned in
//! `world::refinery_dock_oracle_tests`.

use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::economy::Economy;
use crate::util::native_x87::NativeF32Bits;

fn f32_bits(value: f32) -> NativeF32Bits {
    NativeF32Bits::from_bits(value.to_bits())
}

fn rules_with_capacity(line: &str) -> RuleSet {
    let ini = IniFile::from_str(&format!(
        "[General]\n{line}\n[BuildingTypes]\n0=B\n[B]\nStrength=100\n"
    ));
    RuleSet::from_ini_with_fixed_art_for_test(&ini, &IniFile::from_str(""))
        .expect("a minimal rules INI reads")
}

#[test]
fn weed_capacity_follows_read_general_67194d() {
    // Retail rulesmd.ini supplies 56; the constructor 0x006676FD clears the
    // field, and ReadGeneral 0x0067194D..0x00671967 stores ReadInteger's
    // signed result without a clamp.
    assert_eq!(
        rules_with_capacity("WeedCapacity=56").general.weed_capacity,
        56
    );
    assert_eq!(
        rules_with_capacity("WeedCapacity=-3").general.weed_capacity,
        -3
    );
    assert_eq!(
        rules_with_capacity("FlightLevel=500").general.weed_capacity,
        0
    );
    let no_general = IniFile::from_str("[BuildingTypes]\n0=B\n[B]\nStrength=100\n");
    assert_eq!(
        RuleSet::from_ini_with_fixed_art_for_test(&no_general, &IniFile::from_str(""))
            .unwrap()
            .general
            .weed_capacity,
        0
    );
}

#[test]
fn give_weed_4f9700_rechecks_the_whole_store_before_every_unit() {
    let mut wallet = Economy::new(1000);
    // The native loop re-reads GetTotalAmount before EACH Add_Amount(1.0):
    // five requested units against a capacity of two deposit exactly two.
    wallet.give_weed(5, 2, 2);
    assert_eq!(wallet.owned_weed_total(), 2);
    assert_eq!(
        wallet.owned_weed_amounts(),
        [
            NativeF32Bits::POSITIVE_ZERO,
            NativeF32Bits::POSITIVE_ZERO,
            f32_bits(2.0),
            NativeF32Bits::POSITIVE_ZERO,
        ]
    );
    // A full store refuses the next deposit, and a non-positive count or a
    // zero/negative capacity writes nothing at all.
    wallet.give_weed(3, 0, 2);
    assert_eq!(wallet.owned_weed_total(), 2);
    wallet.give_weed(0, 0, 99);
    wallet.give_weed(-4, 0, 99);
    assert_eq!(wallet.owned_weed_total(), 2);
    let mut empty = Economy::new(0);
    empty.give_weed(7, 0, 0);
    empty.give_weed(7, 0, -1);
    assert_eq!(empty.owned_weed_total(), 0);
    // Credits and score are untouched: the Weeder arm does not call
    // Add_Tiberium_Credits 0x004F9610.
    assert_eq!(wallet.credits(), 1000);
    assert_eq!(wallet.score(), 0);
    assert_eq!(wallet.spent_credits(), 0);
}
