//! Kratos PP's frozen status, whose first source is `Wreck`.
//!
//! Kratos keeps one `Freezing` byte on each techno and refreshes it every
//! update from three sources: an alive `Freeze` script, a `VectorFreezeActive`
//! latch, and the type's `Wreck` key (`src/Ext/TechnoType/Status/Freeze.cpp`).
//! While it holds, the unit clears its target and stops moving, and the
//! engine's limb, turret and locomotion hooks refuse to advance it
//! (`0x736A95`, `0x6622E0`, `TechnoExtHook.cpp`'s locomotor arm).
//!
//! VERA derives the status from the type, which is exact for `Wreck`: a wreck
//! is frozen for as long as it exists, so no per-unit state or save field is
//! needed. The per-unit sources arrive with their own owners (the Freeze
//! AttachEffect, the vector latch); they will add a runtime input here rather
//! than a second status.

use super::ini_parser::IniSection;

/// The type-level inputs of the frozen status.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub(crate) struct FreezeRules {
    /// `Wreck=yes` on a `[TechnoType]`: a wreck that is never usable.
    wreck: bool,
}

impl FreezeRules {
    /// Read the type's keys; `Wreck` is a Kratos key, so it can turn the
    /// per-game switch on.
    pub(crate) fn read(section: &IniSection) -> Self {
        Self {
            wreck: crate::rules::kratos_switch::kratos_read(|| section.read_bool("Wreck", false)),
        }
    }

    /// Whether this type is frozen for its whole life.
    pub(crate) fn frozen(&self) -> bool {
        self.wreck
    }
}
