//! Phobos `HoverDrownable`: whether a hover Unit that shut down over water
//! dies when it lands.
//!
//! ```ini
//! HoverDrownable=            ; Nullable<bool>, unset    Ext/UnitType/Body.cpp:87
//! ```
//!
//! The type's value falls back to `[General] HoverDrownable=` (default `yes`,
//! `Ext/Rules/Body.h:890`, read at `Ext/Rules/Body.cpp:475`). The landing arm
//! `ObjectClass_Update_FallingDown_ToDead` (`Ext/Techno/Hooks.cpp:1026-1043`)
//! asks `HoverDrownable.Get(RulesExt::Global()->HoverDrownable)` at `:1031`.
//!
//! Declaration `Ext/UnitType/Body.h:72`, constructor `Body.h:133`.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependency on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;

/// `HoverDrownable=` alone; the barrel and turret keys are
/// [`crate::rules::phobos_turret_barrel::PhobosTurretBarrel`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PhobosHoverDrown {
    /// `HoverDrownable=` (`:87`), `Nullable`: `None` keeps
    /// `[General] HoverDrownable`.
    pub hover_drownable: Option<bool>,
}

impl PhobosHoverDrown {
    pub fn from_section(section: &IniSection) -> Self {
        Self {
            hover_drownable: section.read_phobos_nullable_bool("HoverDrownable", None),
        }
    }

    /// `HoverDrownable.Get(RulesExt::Global()->HoverDrownable)`.
    pub fn drowns(&self, global: bool) -> bool {
        self.hover_drownable.unwrap_or(global)
    }
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "phobos_hover_drown_tests.rs"]
mod tests;
