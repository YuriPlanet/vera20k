//! Phobos `RoofProductionAnim*` BuildingType ART keys.
//!
//! Read from the type's ART section (`Ext/BuildingType/Body.cpp:325-335`) and
//! consumed by `PlayRoofProductionAnim` (`Ext/Building/Hooks.cpp:1091-1165`)
//! for a roof-exit unit leaving a factory.
//!
//! ```ini
//! [SOMEBUILDING]                       ; ART section
//! RoofProductionAnim=                  ; AnimationType
//! RoofProductionAnimDamaged=           ; AnimationType
//! RoofProductionAnimGarrisoned=        ; AnimationType
//! RoofProductionAnimX=                 ; integer, defaults to ProductionAnimX
//! RoofProductionAnimY=                 ; integer, defaults to ProductionAnimY
//! RoofProductionAnimZAdjust=           ; integer, defaults to ProductionAnimZAdjust
//! RoofProductionAnimYSort=             ; integer, defaults to ProductionAnimYSort
//! RoofProductionAnimPowered=           ; boolean, defaults to ProductionAnimPowered
//! RoofProductionAnimPoweredLight=      ; boolean, defaults to ProductionAnimPoweredLight
//! RoofProductionAnimPoweredEffect=     ; boolean, defaults to ProductionAnimPoweredEffect
//! RoofProductionAnimPoweredSpecial=    ; boolean, defaults to ProductionAnimPoweredSpecial
//! ```
//!
//! Pick order (`PickRoofProductionAnim`, `Hooks.cpp:1080-1089`):
//! `Garrisoned` → `Damaged` → base; a null pick falls back to the plain
//! `ProductionAnim` slot name, never to the plain damaged/garrisoned variants.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependency on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;

/// The eleven `RoofProductionAnim*` ART overrides.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PhobosRoofProductionAnim {
    /// `RoofProductionAnim=`, a `Valueable<AnimTypeClass*>`: the token is
    /// retained here; unknown names resolve to null at the consumer and fall
    /// back to the plain `ProductionAnim`.
    pub anim: Option<String>,
    /// `RoofProductionAnimDamaged=`.
    pub damaged: Option<String>,
    /// `RoofProductionAnimGarrisoned=`.
    pub garrisoned: Option<String>,
    /// `RoofProductionAnimX=`, `Nullable<int>`.
    pub x: Option<i32>,
    /// `RoofProductionAnimY=`, `Nullable<int>`.
    pub y: Option<i32>,
    /// `RoofProductionAnimZAdjust=`, `Nullable<int>`.
    pub z_adjust: Option<i32>,
    /// `RoofProductionAnimYSort=`, `Nullable<int>`.
    pub y_sort: Option<i32>,
    /// `RoofProductionAnimPowered=`, `Nullable<bool>`.
    pub powered: Option<bool>,
    /// `RoofProductionAnimPoweredLight=`, `Nullable<bool>`.
    pub powered_light: Option<bool>,
    /// `RoofProductionAnimPoweredEffect=`, `Nullable<bool>`.
    pub powered_effect: Option<bool>,
    /// `RoofProductionAnimPoweredSpecial=`, `Nullable<bool>`.
    pub powered_special: Option<bool>,
}

impl PhobosRoofProductionAnim {
    /// `BuildingTypeExt::LoadFromINIFile`'s ART reads.
    pub fn from_section(section: &IniSection) -> Self {
        Self {
            anim: read_anim_name(section, "RoofProductionAnim"),
            damaged: read_anim_name(section, "RoofProductionAnimDamaged"),
            garrisoned: read_anim_name(section, "RoofProductionAnimGarrisoned"),
            x: section.read_phobos_nullable_int("RoofProductionAnimX", None),
            y: section.read_phobos_nullable_int("RoofProductionAnimY", None),
            z_adjust: section.read_phobos_nullable_int("RoofProductionAnimZAdjust", None),
            y_sort: section.read_phobos_nullable_int("RoofProductionAnimYSort", None),
            powered: section.read_phobos_nullable_bool("RoofProductionAnimPowered", None),
            powered_light: section
                .read_phobos_nullable_bool("RoofProductionAnimPoweredLight", None),
            powered_effect: section
                .read_phobos_nullable_bool("RoofProductionAnimPoweredEffect", None),
            powered_special: section
                .read_phobos_nullable_bool("RoofProductionAnimPoweredSpecial", None),
        }
    }

    /// No key is set: the stock `ProductionAnim` line is untouched.
    pub fn is_empty(&self) -> bool {
        self.anim.is_none()
            && self.damaged.is_none()
            && self.garrisoned.is_none()
            && self.x.is_none()
            && self.y.is_none()
            && self.z_adjust.is_none()
            && self.y_sort.is_none()
            && self.powered.is_none()
            && self.powered_light.is_none()
            && self.powered_effect.is_none()
            && self.powered_special.is_none()
    }

    /// `PickRoofProductionAnim` (`Hooks.cpp:1080-1089`): Garrisoned, then
    /// Damaged, then the base key. `None` means the caller falls back to the
    /// plain `ProductionAnim` slot name.
    pub fn pick(&self, damaged: bool, garrisoned: bool) -> Option<&str> {
        if garrisoned && let Some(anim) = self.garrisoned.as_deref() {
            return Some(anim);
        }
        if damaged && let Some(anim) = self.damaged.as_deref() {
            return Some(anim);
        }
        self.anim.as_deref()
    }
}

/// `Valueable<AnimTypeClass*>` retains the token; `none` spellings resolve to
/// null through the shared type-name reader.
fn read_anim_name(section: &IniSection, key: &str) -> Option<String> {
    section.read_type_name(key, 2048).map(str::to_owned)
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "phobos_roof_production_anim_tests.rs"]
mod tests;
