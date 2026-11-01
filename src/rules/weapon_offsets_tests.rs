//! Phobos `TechnoTypeExt::ParseBurstFLHs`: the ART burst-offset rows and the
//! `(Elite)(PrimaryFire|SecondaryFire|WeaponN)FLH.BurstN` key layout.

use super::{InfantryFlhState, WeaponOffsets};
use crate::rules::flh::Flh;
use crate::rules::ini_parser::{IniFile, IniSection};

fn artwork(text: &str) -> IniSection {
    IniFile::from_str(text).section("ART").unwrap().clone()
}

fn read(text: &str, turret_count: i32, weapon_count: i32) -> WeaponOffsets {
    let mut offsets = WeaponOffsets::default();
    offsets.read_pass(&artwork(text), turret_count, weapon_count);
    offsets
}

fn triple(value: [i32; 3]) -> Flh {
    Flh::from(value)
}

#[test]
fn primary_and_secondary_offsets_parse_in_burst_order() {
    let offsets = read(
        "[ART]\nPrimaryFireFLH.Burst0=1,2,3\nPrimaryFireFLH.Burst1=4,5,6\n\
         SecondaryFireFLH.Burst0=7,8,9\n",
        0,
        0,
    );
    assert_eq!(
        offsets.burst(0, false, InfantryFlhState::Ordinary, 0),
        Some(triple([1, 2, 3]))
    );
    assert_eq!(
        offsets.burst(0, false, InfantryFlhState::Ordinary, 1),
        Some(triple([4, 5, 6]))
    );
    assert_eq!(
        offsets.burst(0, false, InfantryFlhState::Ordinary, 2),
        None,
        "the run ends at the first absent index"
    );
    assert_eq!(
        offsets.burst(1, false, InfantryFlhState::Ordinary, 0),
        Some(triple([7, 8, 9]))
    );
    assert_eq!(
        offsets.burst(2, false, InfantryFlhState::Ordinary, 0),
        None,
        "two slots without weapon X"
    );
}

#[test]
fn a_multi_weapon_turret_uses_the_numbered_slots() {
    let offsets = read(
        "[ART]\nWeapon1FLH.Burst0=1,1,1\nWeapon3FLH.Burst0=3,3,3\n\
         PrimaryFireFLH.Burst0=9,9,9\n",
        1,
        3,
    );
    assert_eq!(
        offsets.burst(0, false, InfantryFlhState::Ordinary, 0),
        Some(triple([1, 1, 1]))
    );
    assert_eq!(
        offsets.burst(1, false, InfantryFlhState::Ordinary, 0),
        None,
        "the plain PrimaryFire key is not read for a multi-weapon turret"
    );
    assert_eq!(
        offsets.burst(2, false, InfantryFlhState::Ordinary, 0),
        Some(triple([3, 3, 3]))
    );

    let weaponless = read("[ART]\nPrimaryFireFLH.Burst0=5,5,5\n", 1, 0);
    assert_eq!(
        weaponless.burst(0, false, InfantryFlhState::Ordinary, 0),
        Some(triple([5, 5, 5])),
        "no WeaponCount means the primary/secondary layout"
    );
}

#[test]
fn elite_defaults_to_the_normal_row_and_a_missing_normal_reads_zero() {
    let offsets = read(
        "[ART]\nPrimaryFireFLH.Burst0=1,2,3\nPrimaryFireFLH.Burst1=4,5,6\n\
         ElitePrimaryFireFLH.Burst0=9,8,7\n",
        0,
        0,
    );
    assert_eq!(
        offsets.burst(0, true, InfantryFlhState::Ordinary, 0),
        Some(triple([9, 8, 7]))
    );
    assert_eq!(
        offsets.burst(0, true, InfantryFlhState::Ordinary, 1),
        Some(triple([4, 5, 6])),
        "an absent elite key copies the normal offset"
    );

    let elite_only = read(
        "[ART]\nPrimaryFireFLH.Burst0=1,2,3\nElitePrimaryFireFLH.Burst1=4,5,6\n",
        0,
        0,
    );
    assert_eq!(
        elite_only.burst(0, false, InfantryFlhState::Ordinary, 1),
        Some(triple([0, 0, 0])),
        "the null nullable value reads as a zero offset"
    );
    assert_eq!(
        elite_only.burst(0, true, InfantryFlhState::Ordinary, 1),
        Some(triple([4, 5, 6]))
    );
}

#[test]
fn the_run_stops_at_a_gap_or_a_rejected_coordinate() {
    let gapped = read(
        "[ART]\nPrimaryFireFLH.Burst0=1,2,3\nPrimaryFireFLH.Burst2=4,5,6\n",
        0,
        0,
    );
    assert_eq!(gapped.burst(0, false, InfantryFlhState::Ordinary, 1), None);
    assert_eq!(gapped.burst(0, false, InfantryFlhState::Ordinary, 2), None);

    let partial = read("[ART]\nPrimaryFireFLH.Burst0=1,2\n", 0, 0);
    assert_eq!(
        partial.burst(0, false, InfantryFlhState::Ordinary, 0),
        None,
        "a partial triple is absent"
    );

    let default_token = read("[ART]\nPrimaryFireFLH.Burst0=<default>\n", 0, 0);
    assert_eq!(
        default_token.burst(0, false, InfantryFlhState::Ordinary, 0),
        None
    );
}

#[test]
fn infantry_states_select_their_own_rows_and_simple_overrides() {
    let mut offsets = WeaponOffsets::default();
    let art = artwork(
        "[ART]\nPrimaryFireFLH=1,1,1\nPrimaryFireFLH.Burst0=2,2,2\n\
         DeployedPrimaryFireFLH.Burst0=3,3,3\nPronePrimaryFireFLH.Burst0=4,4,4\n\
         DeployedSecondaryFireFLH.Burst0=5,5,5\n\
         DeployedPrimaryFireFLH=6,6,6\nPronePrimaryFireFLH=7,7,7\n\
         ProneSecondaryFireFLH=8,8,8\n",
    );
    offsets.read_pass(&art, 0, 0);
    offsets.read_infantry_state_pass(&art, 0, 0);

    assert_eq!(
        offsets.burst(0, false, InfantryFlhState::Ordinary, 0),
        Some(triple([2, 2, 2]))
    );
    assert_eq!(
        offsets.burst(0, false, InfantryFlhState::Deployed, 0),
        Some(triple([3, 3, 3])),
        "the Deployed prefix reads DeployedPrimaryFireFLH.BurstN"
    );
    assert_eq!(
        offsets.burst(0, false, InfantryFlhState::Crouched, 0),
        Some(triple([4, 4, 4])),
        "the Prone prefix reads PronePrimaryFireFLH.BurstN"
    );
    assert_eq!(
        offsets.burst(1, false, InfantryFlhState::Deployed, 0),
        Some(triple([5, 5, 5])),
        "DeployedSecondaryFireFLH keys are slot 1"
    );
    assert_eq!(
        offsets.burst(0, false, InfantryFlhState::Deployed, 1),
        None,
        "each slot has its own run, not the type's"
    );

    assert_eq!(
        offsets.state_flh(0, true, false),
        Some(triple([6, 6, 6])),
        "GetSimpleFLH's deployed primary offset"
    );
    assert_eq!(
        offsets.state_flh(0, false, true),
        Some(triple([7, 7, 7])),
        "GetSimpleFLH's prone primary offset"
    );
    assert_eq!(
        offsets.state_flh(0, true, true),
        Some(triple([6, 6, 6])),
        "deployed wins over crawling"
    );
    assert_eq!(offsets.state_flh(0, false, false), None);
    assert_eq!(
        offsets.state_flh(1, false, true),
        Some(triple([8, 8, 8])),
        "the secondary slot uses its own key"
    );
    assert_eq!(
        offsets.state_flh(2, true, false),
        None,
        "GetSimpleFLH only knows the primary and secondary slots"
    );
}

#[test]
fn a_sized_state_vector_skips_the_type_rows() {
    let mut offsets = WeaponOffsets::default();
    let art = artwork("[ART]\nPrimaryFireFLH.Burst0=2,2,2\n");
    offsets.read_pass(&art, 0, 0);
    offsets.read_infantry_state_pass(&art, 0, 0);
    assert_eq!(
        offsets.burst(0, false, InfantryFlhState::Ordinary, 0),
        Some(triple([2, 2, 2]))
    );
    // `resize(weaponCount)` sizes the state vectors during the parse, so an
    // infantryman with no authored state key finds no burst row at all: the
    // hook then falls to GetSimpleFLH and the weapon record.
    assert_eq!(offsets.burst(0, false, InfantryFlhState::Deployed, 0), None);
    assert_eq!(offsets.burst(0, true, InfantryFlhState::Crouched, 0), None);
}

#[test]
fn a_repeated_read_keeps_the_first_run_at_its_indices() {
    let art = artwork("[ART]\nPrimaryFireFLH.Burst0=1,2,3\n");
    let mut offsets = WeaponOffsets::default();
    offsets.read_pass(&art, 0, 0);
    offsets.read_pass(&art, 0, 0);
    assert_eq!(
        offsets.burst(0, false, InfantryFlhState::Ordinary, 0),
        Some(triple([1, 2, 3])),
        "the native owner appends rows and never clears them"
    );
}
