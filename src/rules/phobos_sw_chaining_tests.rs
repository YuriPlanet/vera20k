//! `SWTypeExt::LoadFromINIFile`'s `SW.Next*`/`SW.Link*` reads
//! (`Ext/SWType/Body.cpp:170-265`).

use super::PhobosSuperWeaponChaining;
use crate::rules::ini_parser::IniFile;

const TYPES: &[&str] = &["Alpha", "Beta", "Gamma"];

fn resolve(token: &str) -> Option<String> {
    TYPES
        .iter()
        .find(|name| name.eq_ignore_ascii_case(token))
        .map(|name| (*name).to_owned())
}

fn read(ini: &str, section: &str) -> PhobosSuperWeaponChaining {
    read_pass(PhobosSuperWeaponChaining::default(), ini, section)
}

/// One later rules pass into the same retained extension.
fn read_pass(
    previous: PhobosSuperWeaponChaining,
    ini: &str,
    section: &str,
) -> PhobosSuperWeaponChaining {
    let file = IniFile::from_str(ini);
    PhobosSuperWeaponChaining::from_section(previous, file.section_or_empty(section), resolve)
}

#[test]
fn defaults_match_the_constructor() {
    let set = PhobosSuperWeaponChaining::default();
    assert!(set.next().is_empty());
    assert!(set.next_real_launch());
    assert!(!set.next_ignore_inhibitors());
    assert!(set.next_ignore_designators());
    assert!(set.next_roll_chances().is_empty());
    assert!(set.next_random_weights().is_empty());
    assert!(set.link().is_empty());
    assert!(!set.link_grant());
    assert!(!set.link_ready());
    assert!(!set.link_reset());
    assert!(set.link_roll_chances().is_empty());
    assert!(set.link_random_weights().is_empty());
}

#[test]
fn reads_next_and_link_lists_with_unknown_names_dropped() {
    let set = read("[S]\nSW.Next=Alpha,Missing,beta\nSW.Link=Gamma,Nope\n", "S");
    assert_eq!(set.next(), ["Alpha".to_owned(), "Beta".to_owned()]);
    assert_eq!(set.link(), ["Gamma".to_owned()]);
}

/// `SW.Next`/`SW.Link` are `ValueableIdxVector`: a reached list replaces the
/// field and an absent or empty one leaves it empty.
#[test]
fn absent_or_empty_lists_are_empty() {
    assert!(read("[S]\nSW.TechLevel=1\n", "S").next().is_empty());
    assert!(read("[S]\nSW.Next=\nSW.Link=,,\n", "S").next().is_empty());
    assert!(read("[S]\nSW.Link=\n", "S").link().is_empty());
    let unknown_only = read("[S]\nSW.Next=Missing\nSW.Link=Missing\n", "S");
    assert!(unknown_only.next().is_empty());
    assert!(unknown_only.link().is_empty());
}

#[test]
fn reads_the_flags_with_their_constructor_defaults() {
    let set = read(
        "[S]\nSW.Next.RealLaunch=no\nSW.Next.IgnoreInhibitors=yes\n\
         SW.Next.IgnoreDesignators=no\nSW.Link.Grant=yes\nSW.Link.Ready=yes\nSW.Link.Reset=yes\n",
        "S",
    );
    assert!(!set.next_real_launch());
    assert!(set.next_ignore_inhibitors());
    assert!(!set.next_ignore_designators());
    assert!(set.link_grant());
    assert!(set.link_ready());
    assert!(set.link_reset());

    let defaults = read("[S]\n", "S");
    assert!(defaults.next_real_launch());
    assert!(!defaults.next_ignore_inhibitors());
    assert!(defaults.next_ignore_designators());
    assert!(!defaults.link_grant());
    assert!(!defaults.link_ready());
    assert!(!defaults.link_reset());
}

#[test]
fn reads_roll_chances_as_binary32() {
    let set = read(
        "[S]\nSW.Next.RollChances=0.25,1,0\nSW.Link.RollChances=0.5\n",
        "S",
    );
    assert_eq!(set.next_roll_chances(), vec![0.25, 1.0, 0.0]);
    assert_eq!(set.link_roll_chances(), vec![0.5]);
    assert!(
        read("[S]\nSW.Next.RollChances=\n", "S")
            .next_roll_chances()
            .is_empty()
    );
}

/// The numbered weight lists stop at the first empty/missing key, and the
/// unnumbered alias replaces or creates slot 0.
#[test]
fn numbered_weights_stop_at_the_first_empty_and_the_alias_rewrites_slot_zero() {
    let set = read(
        "[S]\nSW.Next.RandomWeights0=1,2\nSW.Next.RandomWeights1=3,4\n\
         SW.Next.RandomWeights2=\nSW.Next.RandomWeights3=9\nSW.Next.RandomWeights=7,8\n",
        "S",
    );
    assert_eq!(
        set.next_random_weights(),
        [vec![7, 8], vec![3, 4]],
        "slot 0 takes the alias, list 1 survives, the empty list 2 stops the loop"
    );

    let numbered_only = read("[S]\nSW.Next.RandomWeights0=5\n", "S");
    assert_eq!(numbered_only.next_random_weights(), [vec![5]]);

    let alias_only = read("[S]\nSW.Link.RandomWeights=2,0,1\n", "S");
    assert_eq!(alias_only.link_random_weights(), [vec![2, 0, 1]]);
    assert!(
        read("[S]\nSW.Link.RandomWeights0=1\n", "S")
            .link_random_weights()
            .len()
            == 1
    );
}

/// `SWTypeExt` is one retained object across rules passes (`Body.cpp:170-243`
/// reads into `this`), so a key the later pass does not reach keeps the
/// earlier pass's value instead of falling back to the constructor default.
#[test]
fn a_later_pass_retains_keys_it_does_not_reach() {
    let first = read(
        "[S]\nSW.Next=Alpha\nSW.Next.RealLaunch=no\nSW.Next.RollChances=0.5\n\
         SW.Link=Gamma\nSW.Link.Grant=yes\n",
        "S",
    );
    let second = read_pass(first, "[S]\nSW.Next.IgnoreInhibitors=yes\n", "S");
    assert_eq!(second.next(), ["Alpha".to_owned()]);
    assert!(!second.next_real_launch());
    assert!(second.next_ignore_inhibitors());
    assert_eq!(second.next_roll_chances(), vec![0.5]);
    assert_eq!(second.link(), ["Gamma".to_owned()]);
    assert!(second.link_grant());
}

/// `Body.cpp:184-235`: a numbered list below the retained length replaces its
/// slot, a longer run appends, and the loop stops without touching the tail.
#[test]
fn numbered_weights_update_the_retained_list_across_passes() {
    let first = read(
        "[S]\nSW.Next.RandomWeights0=1,2\nSW.Next.RandomWeights1=3,4\n\
         SW.Next.RandomWeights2=5\n",
        "S",
    );
    let shorter = read_pass(first.clone(), "[S]\nSW.Next.RandomWeights0=9\n", "S");
    assert_eq!(
        shorter.next_random_weights(),
        [vec![9], vec![3, 4], vec![5]],
        "the later pass replaces slot 0 and keeps the retained tail"
    );
    let longer = read_pass(
        first,
        "[S]\nSW.Next.RandomWeights0=9\nSW.Next.RandomWeights1=3,4\n\
         SW.Next.RandomWeights2=5\nSW.Next.RandomWeights3=7\n",
        "S",
    );
    assert_eq!(
        longer.next_random_weights(),
        [vec![9], vec![3, 4], vec![5], vec![7]],
        "the reached slots are replaced and slot 3 appends"
    );
}

/// The alias is read after the numbered loop in every pass (`:201-209`), so a
/// later pass that only writes `SW.Next.RandomWeights` does not re-apply the
/// earlier pass's numbered slot 0, and a later numbered list wins over an
/// earlier alias.
#[test]
fn the_alias_only_patches_slot_zero_within_its_own_pass() {
    let numbered = read_pass(
        read("[S]\nSW.Next.RandomWeights=7,8\n", "S"),
        "[S]\nSW.Next.RandomWeights0=1,2\n",
        "S",
    );
    assert_eq!(numbered.next_random_weights(), [vec![1, 2]]);
}

/// `Body.cpp:253-263` has no replace arm: `SW.Link.RandomWeightsN` appends to
/// the retained vector on every pass (Phobos' own behavior, kept as written),
/// and the alias still patches slot 0 of the grown vector.
#[test]
fn link_numbered_weights_accumulate_across_passes() {
    let first = read(
        "[S]\nSW.Link.RandomWeights0=1\nSW.Link.RandomWeights1=2\n",
        "S",
    );
    let second = read_pass(first.clone(), "[S]\nSW.Link.RandomWeights0=3\n", "S");
    assert_eq!(
        second.link_random_weights(),
        [vec![1], vec![2], vec![3]],
        "the reread appends instead of replacing slot 0"
    );
    let with_alias = read_pass(
        first,
        "[S]\nSW.Link.RandomWeights0=3\nSW.Link.RandomWeights=9\n",
        "S",
    );
    assert_eq!(
        with_alias.link_random_weights(),
        [vec![9], vec![2], vec![3]],
        "the alias patches slot 0 of the accumulated vector"
    );
}
