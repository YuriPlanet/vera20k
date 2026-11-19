//! Phobos `Passengers.SyncOwner` / `Passengers.SyncOwner.RevertOnExit`
//! (`Ext/TechnoType/Body.cpp:958-959`).
//!
//! Two `Nullable<bool>` techno-type keys on the transport: `yes` re-houses
//! every cargo passenger when the transport's house changes
//! (`TechnoClass_ChangeHouse_Passenger_SyncOwner @ 0x701881`), and the
//! revert key puts each passenger back on the house it held when it boarded
//! (`FootClass_LeaveTransport @ 0x4DE722`, `UnitClass_ReceiveDamage_Cargo_
//! SyncOwner @ 0x737F80`). An unset key falls back to the `[General]` global
//! (`Ext/Rules/Body.cpp:467-468`, constructor `Ext/Rules/Body.h:882-883`),
//! through `Nullable<bool>::Get`.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;

/// The two `TechnoTypeExt` fields, each `Nullable<bool>` constructed unset
/// (`Ext/TechnoType/Body.h:202-203`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PhobosPassengersSync {
    /// `Passengers.SyncOwner=`.
    pub sync_owner: Option<bool>,
    /// `Passengers.SyncOwner.RevertOnExit=`.
    pub sync_owner_revert_on_exit: Option<bool>,
}

impl PhobosPassengersSync {
    pub fn from_section(section: &IniSection) -> Self {
        Self {
            sync_owner: section.read_phobos_nullable_bool("Passengers.SyncOwner", None),
            sync_owner_revert_on_exit: section
                .read_phobos_nullable_bool("Passengers.SyncOwner.RevertOnExit", None),
        }
    }

    /// `Nullable<bool>::Get(global)`: the type value when set, else the
    /// `[General]` global's `false` default.
    pub fn sync_owner(&self, global: bool) -> bool {
        self.sync_owner.unwrap_or(global)
    }

    /// The `Passengers.SyncOwner.RevertOnExit=` effective value; the global
    /// constructs `true` (`Ext/Rules/Body.h:883`).
    pub fn sync_owner_revert_on_exit(&self, global: bool) -> bool {
        self.sync_owner_revert_on_exit.unwrap_or(global)
    }
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "phobos_passengers_sync_tests.rs"]
mod tests;
