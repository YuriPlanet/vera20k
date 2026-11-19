//! Phobos `KeepTargetOnMove.*` TechnoType keys
//! (`Ext/TechnoType/Body.cpp:1103-1106`; fields `Ext/TechnoType/Body.h:320-323`;
//! constructor defaults `Body.h:754-757`).
//!
//! Consumers:
//! - `EventClass_Execute_MegaMission_MoveCommand @ 0x4C7462`
//!   (`Ext/Techno/Hooks.Misc.cpp:1036-1075`): a Move order to a unit with a
//!   target already close enough to attack keeps that target and stores the
//!   order destination instead of clearing the target.
//! - `UnitExt::UpdateKeepTargetOnMove` (`Ext/Unit/Body.cpp:94-144`), called
//!   from `UnitClass_AI @ 0x736480` (`Ext/Techno/Hooks.cpp:39-82`): once the
//!   latched unit stops being able to reach the target (range + ExtraDistance,
//!   or a Guard unit with NoMorePursuit queueing Attack), the target is
//!   dropped.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependency on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;

/// The four `KeepTargetOnMove.*` keys. The main switch is `Valueable<bool>`;
/// `.ExtraDistance` is a `Valueable<Leptons>` (INI cells × 256).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct PhobosKeepTargetOnMove {
    /// `KeepTargetOnMove=`, default `no`.
    pub enabled: bool,
    /// `KeepTargetOnMove.Weapon=`, default -1: use `SelectWeapon(target)`.
    pub weapon: i32,
    /// `KeepTargetOnMove.NoMorePursuit=`, default `yes`: a Guard unit keeps
    /// the target instead of queueing Attack when its Move order is issued.
    pub no_more_pursuit: bool,
    /// `KeepTargetOnMove.ExtraDistance=`, `Valueable<Leptons>`, default 0:
    /// the temporary range extension while checking whether the target is
    /// still reachable.
    pub extra_distance: i32,
}

impl Default for PhobosKeepTargetOnMove {
    fn default() -> Self {
        Self {
            enabled: false,
            weapon: -1,
            no_more_pursuit: true,
            extra_distance: 0,
        }
    }
}

impl PhobosKeepTargetOnMove {
    pub fn from_section(section: &IniSection) -> Self {
        Self {
            enabled: section.read_bool("KeepTargetOnMove", false),
            weapon: section.read_ares_int("KeepTargetOnMove.Weapon", -1),
            no_more_pursuit: section.read_bool("KeepTargetOnMove.NoMorePursuit", true),
            extra_distance: section.read_phobos_leptons("KeepTargetOnMove.ExtraDistance", 0),
        }
    }
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "phobos_keep_target_tests.rs"]
mod tests;
