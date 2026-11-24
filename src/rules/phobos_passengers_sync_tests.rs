//! Reader tests for the two `Passengers.SyncOwner.*` type keys.

use super::PhobosPassengersSync;
use crate::rules::ini_parser::IniFile;

#[test]
fn unset_keys_read_none_and_set_keys_read_their_value() {
    let ini = IniFile::from_str(
        "[A]\nPassengers.SyncOwner=yes\nPassengers.SyncOwner.RevertOnExit=no\n\n\
         [B]\nStrength=100\n",
    );
    let a = PhobosPassengersSync::from_section(ini.section("A").unwrap());
    assert_eq!(a.sync_owner, Some(true));
    assert_eq!(a.sync_owner_revert_on_exit, Some(false));
    assert_eq!(
        PhobosPassengersSync::from_section(ini.section("B").unwrap()),
        PhobosPassengersSync::default(),
        "a section without the keys stays unset"
    );
}

#[test]
fn effective_values_fall_back_to_the_globals() {
    let keys = PhobosPassengersSync {
        sync_owner: Some(false),
        ..PhobosPassengersSync::default()
    };
    assert!(!keys.sync_owner(true), "the type key overrides the global");
    assert!(
        !keys.sync_owner_revert_on_exit(false),
        "the unset revert key takes a no global"
    );
    assert!(
        keys.sync_owner_revert_on_exit(true),
        "the unset revert key takes a yes global"
    );
}
