//! Phobos `ExitThroughRoof` on a `TechnoType`: a produced unit leaves its
//! factory through the roof animation instead of the gate.
//!
//! ```ini
//! ExitThroughRoof=   ; Nullable<bool>, unset   Ext/TechnoType/Body.cpp:1200
//! ```
//!
//! Declaration `Ext/TechnoType/Body.h:437`, constructor `Body.h:859`.
//!
//! Two consumers answer it with **different** fallbacks
//! (`Ext/Building/Hooks.cpp:1053`, `:1068`):
//!
//! ```cpp
//! static bool IsRoofExitTechno(TechnoTypeClass* pType)
//! {
//!     return TechnoTypeExt::Fetch(pType)->ExitThroughRoof
//!         .Get(pType->JumpJet || pType->BalloonHover);
//! }
//! // BuildingClass_DrawDoorAnim (0x43D0CF):
//! return ExitThroughRoof.Get(pType->JumpJet) ? UseRoof : UseGate;
//! ```
//!
//! So the "is this a roof-exit techno" predicate defaults to
//! `JumpJet || BalloonHover`, while the door-anim **draw** defaults to
//! `JumpJet` alone. Both are `Nullable<bool>::Get` overrides: an explicit
//! `yes`/`no` on the type outranks the locomotor hint.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependency on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;

/// `ExitThroughRoof=`, a `Nullable<bool>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PhobosExitThroughRoof {
    /// `ExitThroughRoof=`: `None` keeps the locomotor hint of the caller.
    pub value: Option<bool>,
}

impl PhobosExitThroughRoof {
    pub fn from_section(section: &IniSection) -> Self {
        Self {
            value: section.read_phobos_nullable_bool("ExitThroughRoof", None),
        }
    }

    /// `IsRoofExitTechno` (`Ext/Building/Hooks.cpp:1053`): the type's key,
    /// else `JumpJet || BalloonHover`.
    pub fn exits_through_roof(self, jump_jet: bool, balloon_hover: bool) -> bool {
        self.value.unwrap_or(jump_jet || balloon_hover)
    }

    /// `BuildingClass_DrawDoorAnim_ExitThroughRoof` (`:1068`): the type's
    /// key, else `JumpJet` alone — the narrower of the two fallbacks.
    pub fn draws_through_roof(self, jump_jet: bool) -> bool {
        self.value.unwrap_or(jump_jet)
    }
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "phobos_exit_through_roof_tests.rs"]
mod tests;
