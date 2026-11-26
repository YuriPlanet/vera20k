use super::{AirstrikeKind, AirstrikeTargets};

#[test]
fn the_default_designates_buildings_only() {
    let targets = AirstrikeTargets::DEFAULT;
    assert!(targets.allows(AirstrikeKind::Building));
    assert!(!targets.allows(AirstrikeKind::Infantry));
    assert!(!targets.allows(AirstrikeKind::Unit));
    assert!(!targets.allows(AirstrikeKind::Aircraft));
}

#[test]
fn aircraft_read_the_units_bit_as_phobos_reads_it() {
    let targets = AirstrikeTargets::parse("units").unwrap();
    assert!(targets.allows(AirstrikeKind::Unit));
    assert!(targets.allows(AirstrikeKind::Aircraft));
    assert!(!targets.allows(AirstrikeKind::Infantry));
}

#[test]
fn names_accumulate_across_one_comma_list() {
    let targets = AirstrikeTargets::parse(" Infantry , BUILDINGS ").unwrap();
    assert!(targets.allows(AirstrikeKind::Infantry));
    assert!(targets.allows(AirstrikeKind::Building));
    assert!(!targets.allows(AirstrikeKind::Unit));
}

#[test]
fn none_designates_nothing_and_all_designates_every_kind() {
    let none = AirstrikeTargets::parse("none").unwrap();
    for kind in [
        AirstrikeKind::Infantry,
        AirstrikeKind::Unit,
        AirstrikeKind::Aircraft,
        AirstrikeKind::Building,
    ] {
        assert!(!none.allows(kind), "none allows {kind:?}");
    }
    let all = AirstrikeTargets::parse("all").unwrap();
    for kind in [
        AirstrikeKind::Infantry,
        AirstrikeKind::Unit,
        AirstrikeKind::Aircraft,
        AirstrikeKind::Building,
    ] {
        assert!(all.allows(kind), "all refuses {kind:?}");
    }
}

#[test]
fn an_unreadable_list_leaves_the_default_in_place() {
    assert_eq!(AirstrikeTargets::parse("units, tanks"), None);
    assert_eq!(AirstrikeTargets::parse(""), None);
}
