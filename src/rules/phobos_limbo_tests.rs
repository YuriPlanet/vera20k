//! `SWTypeExt::LoadFromINIFile`'s `LimboDelivery.*`/`LimboKill.*` reads
//! (`Ext/SWType/Body.cpp:159-169`).

use super::PhobosSuperWeaponLimbo;
use crate::rules::ini_parser::IniFile;
use crate::rules::phobos_shields::AffectedHouse;

fn resolve(token: &str) -> Option<String> {
    ["POWER", "FACTORY"]
        .iter()
        .find(|name| name.eq_ignore_ascii_case(token))
        .map(|name| (*name).to_owned())
}

fn read(ini: &str, section: &str) -> PhobosSuperWeaponLimbo {
    read_pass(PhobosSuperWeaponLimbo::default(), ini, section)
}

/// One later rules pass into the same retained extension.
fn read_pass(previous: PhobosSuperWeaponLimbo, ini: &str, section: &str) -> PhobosSuperWeaponLimbo {
    let file = IniFile::from_str(ini);
    PhobosSuperWeaponLimbo::from_section(previous, file.section_or_empty(section), resolve)
}

#[test]
fn defaults_are_empty_and_owner() {
    let set = PhobosSuperWeaponLimbo::default();
    assert!(set.types().is_empty());
    assert!(set.ids().is_empty());
    assert!(set.roll_chances().is_empty());
    assert!(set.random_weights().is_empty());
    assert!(set.kill_ids().is_empty());
    assert!(set.kill_counts().is_empty());
    assert_eq!(set.kill_affects_house(), AffectedHouse::OWNER);
}

#[test]
fn reads_delivery_types_ids_and_chances_with_unknown_names_dropped() {
    let set = read(
        "[S]\nLimboDelivery.Types=power,Missing,factory\n\
         LimboDelivery.IDs=5,7\nLimboDelivery.RollChances=0.5,1\n",
        "S",
    );
    assert_eq!(set.types(), ["POWER".to_owned(), "FACTORY".to_owned()]);
    assert_eq!(set.ids(), [5, 7]);
    assert_eq!(set.roll_chances(), vec![0.5, 1.0]);
}

#[test]
fn reads_kill_ids_counts_and_the_house_mask_in_native_order() {
    let set = read(
        "[S]\nLimboKill.Affected=allies\nLimboKill.AffectsHouse=enemies\n\
         LimboKill.IDs=3,4,5\nLimboKill.Counts=1,2\n",
        "S",
    );
    assert_eq!(set.kill_ids(), [3, 4, 5]);
    assert_eq!(set.kill_counts(), [1, 2]);
    assert_eq!(
        set.kill_affects_house(),
        AffectedHouse::ENEMIES,
        "the canonical key overrides the deprecated spelling"
    );

    let legacy = read("[S]\nLimboKill.Affected=allies\n", "S");
    assert_eq!(legacy.kill_affects_house(), AffectedHouse::ALLIES);
    let rejected = read("[S]\nLimboKill.AffectsHouse=nonsense\n", "S");
    assert_eq!(
        rejected.kill_affects_house(),
        AffectedHouse::OWNER,
        "a rejected value keeps the constructor default"
    );
}

#[test]
fn numbered_weights_stop_at_the_first_empty_and_the_alias_rewrites_slot_zero() {
    let set = read(
        "[S]\nLimboDelivery.RandomWeights0=1,2\nLimboDelivery.RandomWeights1=3\n\
         LimboDelivery.RandomWeights2=\nLimboDelivery.RandomWeights=7,8\n",
        "S",
    );
    assert_eq!(set.random_weights(), [vec![7, 8], vec![3]]);
    let alias = read("[S]\nLimboDelivery.RandomWeights=4\n", "S");
    assert_eq!(alias.random_weights(), [vec![4]]);
}

/// `SWTypeExt` is one retained object across rules passes (`Body.cpp:159-169`
/// reads into `this`), so a key the later pass does not reach keeps the
/// earlier pass's value.
#[test]
fn a_later_pass_retains_absent_limbo_keys() {
    let first = read(
        "[S]\nLimboDelivery.Types=POWER\nLimboDelivery.IDs=5,7\n\
         LimboDelivery.RollChances=0.5\nLimboKill.IDs=3\nLimboKill.Counts=1\n\
         LimboKill.AffectsHouse=enemies\n",
        "S",
    );
    let second = read_pass(first, "[S]\nLimboDelivery.Types=FACTORY\n", "S");
    assert_eq!(second.types(), ["FACTORY".to_owned()]);
    assert_eq!(second.ids(), [5, 7]);
    assert_eq!(second.roll_chances(), vec![0.5]);
    assert_eq!(second.kill_ids(), [3]);
    assert_eq!(second.kill_counts(), [1]);
    assert_eq!(second.kill_affects_house(), AffectedHouse::ENEMIES);
}

/// `Body.cpp:184-209`: a numbered weight list below the retained length
/// replaces its slot, a slot past it appends, and the loop stops without
/// touching the retained tail.
#[test]
fn numbered_weights_update_the_retained_list_across_passes() {
    let first = read(
        "[S]\nLimboDelivery.RandomWeights0=1,2\nLimboDelivery.RandomWeights1=3\n",
        "S",
    );
    let shorter = read_pass(first.clone(), "[S]\nLimboDelivery.RandomWeights0=9\n", "S");
    assert_eq!(shorter.random_weights(), [vec![9], vec![3]]);
    let longer = read_pass(
        first,
        "[S]\nLimboDelivery.RandomWeights0=9\nLimboDelivery.RandomWeights1=3\n\
         LimboDelivery.RandomWeights2=5\n",
        "S",
    );
    assert_eq!(
        longer.random_weights(),
        [vec![9], vec![3], vec![5]],
        "the reached slots are replaced and slot 2 appends"
    );
}
