//! Phobos `Explodes.KillPassengers` (TechnoTypeExt) and
//! `Explodes.DuringBuildup` (BuildingTypeExt) with their `[General]`
//! fallbacks (`Ext/TechnoType/Body.cpp:968`, `Ext/BuildingType/Body.cpp:222`,
//! globals `Ext/Rules/Body.cpp:470-471`, constructors `Ext/Rules/Body.h:885-886`).
//!
//! Both are `Nullable<bool>`: unset falls back to the global. The consumer is
//! `TechnoClass::ReceiveDamage`'s death arm, where the stock arm kills the
//! passengers and fires the death weapon; a building dying in its
//! Construction or Selling mission with `Explodes.DuringBuildup` false
//! behaves as if `Explodes=` were false, and `Explodes.KillPassengers=false`
//! skips only the passenger kill (`Ext/Techno/Hooks.ReceiveDamage.cpp:242-258`,
//! hook `0x702603`, skip targets `0x702669` and `0x702672`).
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub struct PhobosExplodesSettings {
    /// `Explodes.KillPassengers=`: unset takes the `[General]` global.
    pub kill_passengers: Option<bool>,
    /// `Explodes.DuringBuildup=`: unset takes the `[General]` global. Read
    /// only for a BuildingType; other families answer `None`.
    pub during_buildup: Option<bool>,
}

impl PhobosExplodesSettings {
    /// `building` selects the BuildingTypeExt reader, which alone holds
    /// `Explodes.DuringBuildup`.
    pub fn from_section(section: &IniSection, building: bool) -> Self {
        Self {
            kill_passengers: section.read_phobos_nullable_bool("Explodes.KillPassengers", None),
            during_buildup: building
                .then(|| section.read_phobos_nullable_bool("Explodes.DuringBuildup", None))
                .flatten(),
        }
    }
}
