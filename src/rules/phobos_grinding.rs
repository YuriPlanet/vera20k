//! Phobos `Grinding.*` (`Ext/BuildingType/Body.cpp:200-207`).
//!
//! A building type can gate who it grinds, replace the die voice and the
//! grinder sound, and fire a weapon at itself once enough refund accumulates.
//! Consumers: `Ext/Building/Body.cpp` (`CanGrindTechno`, `DoGrindingExtras`)
//! and `Ext/Building/Hooks.Grinding.cpp`.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependency on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;

#[derive(Debug, Clone, PartialEq)]
pub struct PhobosGrindingSettings {
    /// `Grinding.AllowAllies=`, `Valueable<bool>`, default false.
    pub allow_allies: bool,
    /// `Grinding.AllowOwner=`, `Valueable<bool>`, default true.
    pub allow_owner: bool,
    /// `Grinding.AllowTypes=`, `ValueableVector<TechnoTypeClass*>`: when the
    /// read list holds any type, only those types may be ground.
    pub allow_types: Vec<String>,
    /// `Grinding.DisallowTypes=`, same vector; listed types may not be ground.
    pub disallow_types: Vec<String>,
    /// `Grinding.Sound=` (`ValueableIdx<VocClass>`): replaces the stock
    /// `[AudioVisual] EnterGrinderSound=` for this building.
    pub sound: Option<String>,
    /// `Grinding.PlayDieSound=`, `Valueable<bool>`, default true.
    pub play_die_sound: bool,
    /// `Grinding.Weapon=` (`Valueable<WeaponTypeClass*>`): fired at the
    /// building once the accumulated refund reaches the required credits.
    pub weapon: Option<String>,
    /// `Grinding.Weapon.RequiredCredits=`, `Valueable<int>`, default 0.
    pub weapon_required_credits: i32,
}

impl Default for PhobosGrindingSettings {
    fn default() -> Self {
        Self {
            allow_allies: false,
            allow_owner: true,
            allow_types: Vec::new(),
            disallow_types: Vec::new(),
            sound: None,
            play_die_sound: true,
            weapon: None,
            weapon_required_credits: 0,
        }
    }
}

impl PhobosGrindingSettings {
    pub fn from_section(section: &IniSection) -> Self {
        use crate::rules::native_processing::read_nullable_type_vector;
        // `ValueableVector` has no `<default>` form and drops any token that
        // does not name a type; a name that does not resolve can never match
        // a techno, so keep the tokens and resolve them at use time.
        let resolve = |name: &str| Some(name.to_owned());
        Self {
            allow_allies: section.read_ares_bool("Grinding.AllowAllies", false),
            allow_owner: section.read_ares_bool("Grinding.AllowOwner", true),
            allow_types: read_nullable_type_vector(section, "Grinding.AllowTypes", None, resolve)
                .unwrap_or_default(),
            disallow_types: read_nullable_type_vector(
                section,
                "Grinding.DisallowTypes",
                None,
                resolve,
            )
            .unwrap_or_default(),
            sound: None,
            play_die_sound: section.read_ares_bool("Grinding.PlayDieSound", true),
            // `Read<true>` allocates the named weapon on the same pass
            // (`native_processing::allocate_grinding_references`); `none`
            // names no weapon, as for every native type read.
            weapon: section
                .read_type_name("Grinding.Weapon", 0x80)
                .map(str::to_owned),
            weapon_required_credits: section.read_ares_int("Grinding.Weapon.RequiredCredits", 0),
        }
    }
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "phobos_grinding_tests.rs"]
mod tests;
