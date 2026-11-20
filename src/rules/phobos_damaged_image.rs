//! Phobos damaged-image keys, one group per type family that names a swap.
//!
//! A type may name replacement `Image=` sections for the yellow and red
//! condition bands. `TechnoTypeExt` reads the pair every techno type shares
//! (`Ext/TechnoType/Body.cpp:1112-1114`); `UnitTypeExt` adds the water pair a
//! deployed or swimming Unit draws instead (`Ext/UnitType/Body.cpp:78-80`).
//!
//! ```ini
//! Image.ConditionYellow=      ; Nullable<TechnoTypeClass*>, unset :1112
//! Image.ConditionRed=         ; Nullable<TechnoTypeClass*>, unset :1113
//! WaterImage.ConditionYellow= ; Nullable<UnitTypeClass*>, unset   :78
//! WaterImage.ConditionRed=    ; Nullable<UnitTypeClass*>, unset   :79
//! ```
//!
//! `NeedDamagedImage` (`Body.cpp:1114`, `UnitType/Body.cpp:80`) is the
//! derived `|=` of the four `isset()` tests, so a type with any of them set is
//! asked for a damaged image at all.
//!
//! Declarations `Ext/TechnoType/Body.h:329-331` and `Ext/UnitType/Body.h:63-64`,
//! constructors `TechnoType/Body.h:763-765` and `UnitType/Body.h:125-126`.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependency on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;

/// The four named swaps and the flag they imply.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PhobosDamagedImage {
    /// `Image.ConditionYellow=` (`:1112`): the type drawn in the yellow band.
    pub condition_yellow: Option<String>,
    /// `Image.ConditionRed=` (`:1113`): the type drawn in the red band.
    pub condition_red: Option<String>,
    /// `WaterImage.ConditionYellow=` (`UnitType/Body.cpp:78`): the water type
    /// drawn in the yellow band.
    pub water_condition_yellow: Option<String>,
    /// `WaterImage.ConditionRed=` (`UnitType/Body.cpp:79`).
    pub water_condition_red: Option<String>,
}

impl PhobosDamagedImage {
    /// Reads all four keys, in `LoadFromINI` order.
    pub fn from_section(section: &IniSection) -> Self {
        let type_name =
            |key: &str| section.read_phobos_nullable_with(key, None, |raw| Some(raw.to_owned()));
        Self {
            condition_yellow: type_name("Image.ConditionYellow"),
            condition_red: type_name("Image.ConditionRed"),
            water_condition_yellow: type_name("WaterImage.ConditionYellow"),
            water_condition_red: type_name("WaterImage.ConditionRed"),
        }
    }

    /// `NeedDamagedImage`: `Image_ConditionYellow.isset() ||
    /// Image_ConditionRed.isset()` (`TechnoType/Body.cpp:1114`) extended by the
    /// same `|=` for the water pair (`UnitType/Body.cpp:80`).
    pub fn need_damaged_image(&self) -> bool {
        self.condition_yellow.is_some()
            || self.condition_red.is_some()
            || self.water_condition_yellow.is_some()
            || self.water_condition_red.is_some()
    }

    /// The swap `Image.Condition*=` asks for at `condition_red`, if any.
    /// `None` leaves the type's own art in place.
    pub fn band_image(&self, red: bool) -> Option<&str> {
        if red {
            self.condition_red.as_deref()
        } else {
            self.condition_yellow.as_deref()
        }
    }

    /// The water swap, for a Unit that is deployed or swimming.
    pub fn water_band_image(&self, red: bool) -> Option<&str> {
        if red {
            self.water_condition_red.as_deref()
        } else {
            self.water_condition_yellow.as_deref()
        }
    }
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "phobos_damaged_image_tests.rs"]
mod tests;
