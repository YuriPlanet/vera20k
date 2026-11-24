//! Compatibility path for rules-owned immutable mission-control data.
//!
//! Canonical definitions and INI parsing live in `rules::mission_data`.
//! Runtime mission scheduling remains in the parent `sim::mission` module.

pub use crate::rules::mission_data::{MissionControl, MissionControlEntry};

use super::{MissionCom, MissionId};
use crate::rules::ruleset::RuleSet;

/// `MissionClass::Get_MissionControl @ 0x005B3A00`: the MissionControl slot
/// of the object's current mission (`+0xAC`) in the table at `0x00A8E3A8`.
///
/// `None` for no mission (-1). Native then reads the 0x20 bytes before the
/// table: zero through `+7` (`0x00A8E38C..0x00A8E38F` has no reference), then
/// the vtable pointer `0x007E9E64` stored at `0x00A8E390` (`0x004E6BFD`). So
/// `NoThreat=`, `Zombie=`, `Recruitable=` and `Paralyzed=` read clear, and
/// `Retaliate=` and `Scatter=` read set.
pub(crate) fn current_control<'a>(
    rules: &'a RuleSet,
    mission: &MissionCom,
) -> Option<&'a MissionControlEntry> {
    mission
        .current()
        .known()
        .and_then(|mission| rules.mission_control.entry(mission))
}

/// `0x005B36E0`: whether MissionControl lets an object on `mission` be
/// recruited: always with none (-1), else the mission's `Recruitable=`
/// (`MissionControl[m]+6`, constructor 1). Read by `FootClass::IsRecruitable
/// @ 0x004DA230` and `TeamClass::Can_Add @ 0x006EA72C`.
pub(crate) fn mission_recruitable(rules: &RuleSet, mission: MissionId) -> bool {
    if mission == MissionId::NONE {
        return true;
    }
    mission
        .known()
        .and_then(|mission| rules.mission_control.entry(mission))
        .is_some_and(|entry| entry.recruitable)
}
