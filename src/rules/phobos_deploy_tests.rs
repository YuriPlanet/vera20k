//! The `Deploy.*` / `IsSimpleDeployer.*` reads (`Ext/UnitType/Body.cpp:55-56,84-86`,
//! constructors `Ext/UnitType/Body.h`) and the gates they feed.
use super::{PhobosDeploy, land_type_disallowed};
use crate::rules::ini_parser::IniFile;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::terrain_rules::LandType;

fn section(body: &str) -> crate::rules::ini_parser::IniSection {
    IniFile::from_str(&format!("[D]\n{body}"))
        .section_or_empty("D")
        .clone()
}

fn bit(land: LandType) -> u16 {
    1u16 << land.as_index()
}

#[test]
fn defaults_match_the_constructor() {
    let set = PhobosDeploy::default();
    assert!(!set.skip_passenger_unload && !set.no_passenger && !set.no_tiberium);
    assert!(!set.consider_pathfinding);
    assert_eq!(set.disallowed_land_types, None);
    // No key skips a passenger unload, on either the cursor or the mission.
    assert!(!set.cursor_skips_passengers(0) && !set.cursor_skips_passengers(3));
    assert!(!set.mission_skips_passengers(0, 0));
    // An unset filter keeps the stock lander set only for a DeployToLand
    // Hover/Jumpjet, and LandTypeFlags::None for every other type.
    let lander = bit(LandType::Water) | bit(LandType::Beach);
    assert_eq!(set.disallowed_flags(true, LocomotorKind::Hover), lander);
    assert_eq!(set.disallowed_flags(true, LocomotorKind::Jumpjet), lander);
    assert_eq!(set.disallowed_flags(false, LocomotorKind::Hover), 0);
    assert_eq!(set.disallowed_flags(true, LocomotorKind::Drive), 0);
}

#[test]
fn reads_the_five_keys_and_nullable_clearing() {
    let set = PhobosDeploy::from_section(&section(
        "Deploy.SkipPassengerUnload=yes\nDeploy.NoPassenger=yes\nDeploy.NoTiberium=yes\n\
         IsSimpleDeployer.ConsiderPathfinding=yes\nIsSimpleDeployer.DisallowedLandTypes=Water\n",
    ));
    assert!(set.skip_passenger_unload && set.no_passenger && set.no_tiberium);
    assert!(set.consider_pathfinding);
    assert_eq!(set.disallowed_land_types, Some(bit(LandType::Water)));
    // `Nullable::Read<false,true>` (`allowNone=true`) clears only `<default>`;
    // `none` parses as the empty set, while a failed parse or an empty value
    // keeps the current (seeded None).
    let none = PhobosDeploy::from_section(&section("IsSimpleDeployer.DisallowedLandTypes=none\n"));
    assert_eq!(none.disallowed_land_types, Some(0));
    // `Some(0)` overrides the stock lander set with no disallowed land types.
    assert_eq!(none.disallowed_flags(true, LocomotorKind::Hover), 0);
    for retained in ["<none>", "<default>", "Bogus", ""] {
        let cleared = PhobosDeploy::from_section(&section(&format!(
            "IsSimpleDeployer.DisallowedLandTypes={retained}\n"
        )));
        assert_eq!(cleared.disallowed_land_types, None, "{retained:?}");
    }
}

#[test]
fn land_type_flags_resolve_the_first_token() {
    // Phobos `read<LandTypeFlags>` calls GetLandTypeFromName on the buffer head
    // every strtok pass (`Utilities/TemplateDef.h:694`), so a comma list resolves
    // to its FIRST token's bit, not the union.
    let single =
        PhobosDeploy::from_section(&section("IsSimpleDeployer.DisallowedLandTypes=Clear\n"));
    assert_eq!(single.disallowed_land_types, Some(bit(LandType::Clear)));
    let list = PhobosDeploy::from_section(&section(
        "IsSimpleDeployer.DisallowedLandTypes=Water,Beach\n",
    ));
    assert_eq!(list.disallowed_land_types, Some(bit(LandType::Water)));
    // A "none" head followed by a named token fails the read and retains None.
    let none_head = PhobosDeploy::from_section(&section(
        "IsSimpleDeployer.DisallowedLandTypes=none,Water\n",
    ));
    assert_eq!(none_head.disallowed_land_types, None);
    assert!(land_type_disallowed(bit(LandType::Water), LandType::Water));
    assert!(!land_type_disallowed(bit(LandType::Water), LandType::Clear));
}

#[test]
fn cursor_and_mission_passenger_skip_differ_by_status() {
    let skip = PhobosDeploy::from_section(&section("Deploy.SkipPassengerUnload=yes\n"));
    assert!(skip.cursor_skips_passengers(4) && skip.mission_skips_passengers(4, 9));
    let no_passenger = PhobosDeploy::from_section(&section("Deploy.NoPassenger=yes\n"));
    // The cursor (hook 0x740015) carries no MissionStatus test.
    assert!(no_passenger.cursor_skips_passengers(0));
    assert!(!no_passenger.cursor_skips_passengers(1));
    // The mission (hook 0x73D63B) also waits for MissionStatus == 0.
    assert!(no_passenger.mission_skips_passengers(0, 0));
    assert!(!no_passenger.mission_skips_passengers(0, 1));
    assert!(!no_passenger.mission_skips_passengers(1, 0));
    // A configured filter overrides the stock lander set.
    let configured =
        PhobosDeploy::from_section(&section("IsSimpleDeployer.DisallowedLandTypes=Clear\n"));
    assert_eq!(
        configured.disallowed_flags(true, LocomotorKind::Hover),
        bit(LandType::Clear)
    );
}
