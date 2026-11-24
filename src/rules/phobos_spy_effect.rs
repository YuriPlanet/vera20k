//! Phobos `SpyEffect.*` building-type keys (`Ext/BuildingType/Body.cpp:337-345`;
//! fields `Body.h:59-61`; constructor defaults `Body.h:192-194`).
//!
//! `SpyEffect.Custom` is the Ares tag (`Valueable<bool>`, default `no`); the
//! two `ValueableIdx<SuperWeaponTypeClass>` keys are read only when the
//! `SuperWeaponTypeClass::Array` holds a type, and resolve a name through
//! `FindIndex` at read time. They are consumed by
//! `BuildingExt::HandleInfiltrate` (`Ext/Building/Body.cpp:316-356`), the
//! second half of Phobos's `BuildingClass_InfiltratedBy_Wrapper` hook at
//! `0x51A00B` (`:662-671`): after the base infiltration, each configured
//! index launches its house's copy of that Super at the building.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependency on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;

/// The three `SpyEffect.*` keys on one TechnoType (a building type in
/// practice).
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct PhobosSpyEffect {
    /// `SpyEffect.Custom=`, `Valueable<bool>`, default `no`: the Ares tag
    /// that admits the two launch arms.
    pub custom: bool,
    /// `SpyEffect.VictimSuperWeapon=`,
    /// `ValueableIdx<SuperWeaponTypeClass>`: the victim house launches it.
    pub victim_super_weapon: Option<String>,
    /// `SpyEffect.InfiltratorSuperWeapon=`,
    /// `ValueableIdx<SuperWeaponTypeClass>`: the spy's house launches it.
    pub infiltrator_super_weapon: Option<String>,
}

impl Default for PhobosSpyEffect {
    fn default() -> Self {
        Self {
            custom: false,
            victim_super_weapon: None,
            infiltrator_super_weapon: None,
        }
    }
}

impl PhobosSpyEffect {
    pub fn from_section(section: &IniSection) -> Self {
        Self {
            custom: section.read_bool("SpyEffect.Custom", false),
            victim_super_weapon: read_super_weapon_ref(section, "SpyEffect.VictimSuperWeapon"),
            infiltrator_super_weapon: read_super_weapon_ref(
                section,
                "SpyEffect.InfiltratorSuperWeapon",
            ),
        }
    }
}

/// `ValueableIdx<SuperWeaponTypeClass>::Read` (`Utilities/TemplateDef.h:1812-1826`):
/// `ReadString` (2048 bytes) then `FindIndex`; `none`/`<none>` stores -1
/// (no Super) and an absent or empty key keeps the field.
///
/// The native `FindIndex` runs against the `[SuperWeaponTypes]` array at read
/// time and an unknown non-blank name keeps the previous index. VERA stores
/// the name (the merged-section reader has no per-layer previous value) and
/// resolves it against the RuleSet at use, where an unknown name launches
/// nothing — the same answer an unset index gives. The
/// `SuperWeaponTypeClass::Array.Count > 0` read guard is likewise deferred to
/// that lookup.
fn read_super_weapon_ref(section: &IniSection, key: &str) -> Option<String> {
    let name = section.read_name(key, 0x800)?;
    if crate::rules::ini_parser::is_native_none_type_name(name) {
        None
    } else {
        Some(name.to_owned())
    }
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "phobos_spy_effect_tests.rs"]
mod tests;
