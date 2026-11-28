//! Phobos `Overload.*` (`Ext/TechnoType/Body.cpp:1133-1138`).
//!
//! A Mastermind's capture manager replaces the stock overload tables with its
//! own type's lists when they are non-empty; the per-frame overload helper
//! (`CaptureManagerClass_Overload_AI`, `Ext/CaptureManager/Hooks.cpp:111`) then
//! uses the shared `Overload.ParticleSysCount` count.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependency on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PhobosOverloadSettings {
    /// `Overload.Count=` (`NullableVector<int>`): `None` keeps the Rules
    /// list; a present list replaces it, and a present empty list disables
    /// the overload checks entirely (`GetElements` returns it as set).
    pub count: Option<Vec<i32>>,
    /// `Overload.Damage=`, same nullable-vector semantics.
    pub damage: Option<Vec<i32>>,
    /// `Overload.Frames=`, same nullable-vector semantics.
    pub frames: Option<Vec<i32>>,
    /// `Overload.DeathSound=` (`ValueableIdx<VocClass>`): bound by
    /// `ObjectType::bind_scalar_sound_references`, which keeps `None` for an
    /// unknown name so the stock voice stays.
    pub death_sound: Option<String>,
    /// `Overload.ParticleSys=`; `None` keeps `[CombatDamage]
    /// DefaultSparkSystem=`.
    pub particle_sys: Option<String>,
    /// `Overload.ParticleSysCount=`; `None` keeps the `[CombatDamage]`
    /// fallback.
    pub particle_sys_count: Option<i32>,
}

impl PhobosOverloadSettings {
    pub fn from_section(section: &IniSection) -> Self {
        // `NullableVector<int>::Read`: absent, empty and `<default>` keep the
        // Rules list; any other value replaces it, including a present empty
        // list.
        use crate::rules::native_processing::read_nullable_type_vector;
        Self {
            count: read_nullable_type_vector(
                section,
                "Overload.Count",
                None,
                crate::rules::ini_value::parse_ares_int,
            ),
            damage: read_nullable_type_vector(
                section,
                "Overload.Damage",
                None,
                crate::rules::ini_value::parse_ares_int,
            ),
            frames: read_nullable_type_vector(
                section,
                "Overload.Frames",
                None,
                crate::rules::ini_value::parse_ares_int,
            ),
            death_sound: None,
            particle_sys: section
                .read_name("Overload.ParticleSys", 0x80)
                .map(str::to_owned),
            particle_sys_count: section.read_phobos_nullable_int("Overload.ParticleSysCount", None),
        }
    }
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "phobos_overload_tests.rs"]
mod tests;
