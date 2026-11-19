//! Phobos `TabIndex` on a `SuperWeaponType`: which sidebar tab its cameo
//! occupies.
//!
//! ```ini
//! TabIndex=   ; Valueable<int>, 1, validated 0..3   Ext/SWType/Body.cpp:285-286
//! ```
//!
//! Declaration `Ext/SWType/Body.h:88`, constructor `Body.h:187` (`1`).
//!
//! `GeneralUtils::IntValidCheck(&TabIndex, pSection, "TabIndex", 1, 0, 3)`
//! (`Utilities/GeneralUtils.cpp:19-26`) is **not** a clamp: a value outside
//! `[0, 3]` is reset to `1` wholesale and logged as a developer warning.
//!
//! `SidebarClass::GetObjectTabIndex` (`0x6ABC9D`, `Ext/SWType/Hooks.cpp:245-258`)
//! answers it for a SuperWeapon type, and the four
//! `SidebarClass_*_TabIndex` hooks (`:204-262`) route every tab lookup through
//! that getter. VERA's `SidebarTab` is the same 0..3 scan order
//! (`src/ui/sidebar/mod.rs`), so `TabIndex` maps straight onto it.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependency on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;

/// The tab a super weapon's cameo occupies. `Valueable<int>` with
/// `IntValidCheck`'s reset-to-default, not a clamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PhobosSwTabIndex {
    /// `TabIndex=`, in `0..=3`; anything else resets to `1`.
    pub index: i32,
}

impl PhobosSwTabIndex {
    /// `Ext/SWType/Body.cpp:285-286`.
    pub fn from_section(section: &IniSection) -> Self {
        let mut index = section.read_int("TabIndex", 1);
        if !(0..=3).contains(&index) {
            index = 1;
        }
        Self { index }
    }

    /// The sidebar tab this cameo belongs to. The `SidebarTab` variant lives
    /// in the ui/ layer, so the rules owner keeps only the integer and the
    /// mapping is done at the one consumer.
    pub fn index(self) -> usize {
        self.index as usize
    }
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "phobos_sw_tab_index_tests.rs"]
mod tests;
