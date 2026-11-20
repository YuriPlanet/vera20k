//! Phobos `Deploy.*` and `IsSimpleDeployer.*` UnitType keys
//! (`Ext/UnitType/Body.cpp:55-56,84-86`).
//!
//! These fields feed four mechanisms:
//! - `Deploy.SkipPassengerUnload=` / `Deploy.NoPassenger=` skip the passenger
//!   unload of `UnitClass::Mission_Unload` (`Ext/Unit/Hooks.Unload.cpp:53-58`,
//!   hook `0x73D63B`) and of the self cursor (`:77-89`, hook `0x740015`).
//! - `Deploy.NoTiberium=` skips an empty harvester's tiberium dump
//!   (`Ext/Unit/Hooks.Unload.cpp:61-75`, hook `0x73DEEB`).
//! - `IsSimpleDeployer.ConsiderPathfinding=` / `.DisallowedLandTypes=` gate a
//!   simple deployer's own cell by land type and pathfinding
//!   (`UnitExt::SimpleDeployerAllowedToDeploy`, `Ext/Unit/Body.cpp:258-316`,
//!   consumed by the `CanShowDeployCursor` hook `0x7010C1`,
//!   `Ext/Techno/Hooks.cpp:1452-1474`).
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::terrain_rules::LandType;

/// The disallowed-land-type stock set for a `DeployToLand` Hover/Jumpjet lander
/// (`Ext/Unit/Body.cpp:280`): `LandTypeFlags::Water | LandTypeFlags::Beach`.
const STOCK_LANDER_DISALLOWED: u16 =
    (1u16 << LandType::Water.as_index()) | (1u16 << LandType::Beach.as_index());

/// The five `UnitTypeExt` deploy-gate fields. The four bools are
/// `Valueable<bool>` constructing false (`Ext/UnitType/Body.h`); the land-type
/// filter is `Nullable<LandTypeFlags>` (`Read<false, true>`,
/// `Ext/UnitType/Body.cpp:56`), unset until a read sets it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhobosDeploy {
    /// `Deploy.SkipPassengerUnload=`, default false.
    pub skip_passenger_unload: bool,
    /// `Deploy.NoPassenger=`, default false.
    pub no_passenger: bool,
    /// `Deploy.NoTiberium=`, default false.
    pub no_tiberium: bool,
    /// `IsSimpleDeployer.ConsiderPathfinding=`, default false.
    pub consider_pathfinding: bool,
    /// `IsSimpleDeployer.DisallowedLandTypes=`: `None` while unset or cleared
    /// by `<default>`; `Some(flags)` once a read sets the `LandTypeFlags` bit
    /// field (`none` sets the empty set, `Some(0)`).
    pub disallowed_land_types: Option<u16>,
}

impl Default for PhobosDeploy {
    fn default() -> Self {
        Self {
            skip_passenger_unload: false,
            no_passenger: false,
            no_tiberium: false,
            consider_pathfinding: false,
            disallowed_land_types: None,
        }
    }
}

impl PhobosDeploy {
    /// The reads of `UnitTypeExt::LoadFromINIFile`, in their native order:
    /// `IsSimpleDeployer.*` (`Ext/UnitType/Body.cpp:55-56`) then `Deploy.*`
    /// (`:84-86`).
    pub fn from_section(section: &IniSection) -> Self {
        Self {
            consider_pathfinding: section
                .read_ares_bool("IsSimpleDeployer.ConsiderPathfinding", false),
            disallowed_land_types: section.read_phobos_nullable_allow_none_with(
                "IsSimpleDeployer.DisallowedLandTypes",
                None,
                parse_phobos_land_type_flags,
            ),
            skip_passenger_unload: section.read_ares_bool("Deploy.SkipPassengerUnload", false),
            no_passenger: section.read_ares_bool("Deploy.NoPassenger", false),
            no_tiberium: section.read_ares_bool("Deploy.NoTiberium", false),
        }
    }

    /// The self-cursor passenger skip (`Ext/Unit/Hooks.Unload.cpp:86-88`, hook
    /// `0x740015`): `SkipPassengerUnload`, or `NoPassenger` once the transport
    /// holds no passengers. Unlike the mission gate this carries NO
    /// `MissionStatus` test.
    pub fn cursor_skips_passengers(&self, live_passengers: usize) -> bool {
        self.skip_passenger_unload || (self.no_passenger && live_passengers == 0)
    }

    /// The mission passenger skip (`Ext/Unit/Hooks.Unload.cpp:53-56`, hook
    /// `0x73D63B`): `SkipPassengerUnload`, or `NoPassenger` with no passengers
    /// AND `MissionStatus == 0` (the unit has run one status step).
    pub fn mission_skips_passengers(&self, live_passengers: usize, mission_status: u32) -> bool {
        self.skip_passenger_unload
            || (self.no_passenger && live_passengers == 0 && mission_status == 0)
    }

    /// The disallowed flags of `SimpleDeployerAllowedToDeploy`
    /// (`Ext/Unit/Body.cpp:271-281`): the configured filter when set, else the
    /// stock `Water | Beach` for a `DeployToLand` Hover/Jumpjet lander and
    /// `LandTypeFlags::None` for every other type.
    pub fn disallowed_flags(&self, deploy_to_land: bool, locomotor: LocomotorKind) -> u16 {
        if let Some(flags) = self.disallowed_land_types {
            return flags;
        }
        let lander =
            deploy_to_land && matches!(locomotor, LocomotorKind::Hover | LocomotorKind::Jumpjet);
        if lander { STOCK_LANDER_DISALLOWED } else { 0 }
    }
}

/// `IsLandTypeInFlags` (`Utilities/Enum.h:91-94`): the cell's land-type bit
/// against the disallowed field.
pub fn land_type_disallowed(flags: u16, land_type: LandType) -> bool {
    flags & (1u16 << land_type.as_index()) != 0
}

/// Phobos `read<LandTypeFlags>` (`Utilities/TemplateDef.h:681-712`): `strtok`
/// on `","` (`Phobos::readDelims`), skipping a `"none"` token, but
/// `GetLandTypeFromName` is called on `parser.value()` — the buffer head, i.e.
/// the FIRST token — on every pass (`:694`). A comma list therefore resolves
/// to its first token's bit; an unknown first token, or a `"none"` head followed
/// by any named token, fails the read so the `Nullable` retains its previous
/// value. `None` here is that failed read. `none` alone skips every token and
/// yields the empty set.
fn parse_phobos_land_type_flags(raw: &str) -> Option<u16> {
    let head = raw.split(',').next()?;
    let head_land = LandType::from_name(head);
    let mut flags = 0u16;
    for token in raw.split(',') {
        if token.eq_ignore_ascii_case("none") {
            continue;
        }
        match head_land {
            Some(land) => flags |= 1u16 << land.as_index(),
            None => return None,
        }
    }
    Some(flags)
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "phobos_deploy_tests.rs"]
mod tests;
