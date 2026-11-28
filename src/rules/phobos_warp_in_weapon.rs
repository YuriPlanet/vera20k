//! Phobos `WarpInWeapon.*` techno-type keys
//! (`Ext/TechnoType/Body.cpp:870-873`, constructor defaults
//! `Ext/TechnoType/Body.h:534-537`).
//!
//! A teleporting techno detonates a weapon at itself once its warp-in
//! animation step runs. Consumer:
//! `TeleportLocomotionClass_ILocomotion_Process_WarpInAnim @ 0x719742`
//! (`Ext/TechnoType/Hooks.Teleport.cpp:74-98`), ported at
//! `sim::movement::teleport_movement::Simulation::apply_warp_in_weapon`.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependency on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;

/// The three `WarpInWeapon.*` values.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WarpInWeaponRules {
    /// `WarpInWeapon=` (`Valueable<WeaponTypeClass*>`, `Read<true>`),
    /// default null.
    pub warp_in_weapon: Option<String>,
    /// `WarpInMinRangeWeapon=` (`Valueable<WeaponTypeClass*>`, `Read<true>`),
    /// default null: used instead of `WarpInWeapon` when the warp distance is
    /// below `[General] ChronoRangeMinimum`, falling back to `WarpInWeapon`
    /// while unset.
    pub warp_in_min_range_weapon: Option<String>,
    /// `WarpInWeapon.UseDistanceAsDamage=` (`Valueable<bool>`), default
    /// `false`: the copy deals `distance / 256` instead of the weapon's
    /// `Damage=`.
    pub use_distance_as_damage: bool,
    /// `ChronoRangeMinimum=` (`Nullable<int>`,
    /// `Ext/TechnoType/Body.cpp:865`, `Body.h:129`): the TechnoType's own
    /// short-warp threshold for the `WarpInMinRangeWeapon` split. Unset
    /// falls back to `[General] ChronoRangeMinimum` at the consumer.
    pub chrono_range_minimum: Option<i32>,
    /// `WarpOutWeapon=` (`Valueable<WeaponTypeClass*>`, `Read<true>`,
    /// `Ext/TechnoType/Body.cpp:872`), default null: the departure of an
    /// ordinary self-teleport detonates it at the departing object
    /// (`Ext/TechnoType/Hooks.Teleport.cpp:22-32`, hook `0x7193F6`); the
    /// Chronosphere's chronoshift departure fires nothing.
    pub warp_out_weapon: Option<String>,
}

impl WarpInWeaponRules {
    pub fn from_section(section: &IniSection) -> Self {
        Self {
            // `Read<true>` allocates the named weapon on the same pass
            // (`native_processing::allocate_warp_in_weapon_references`); a
            // blank value or `none` names no weapon, as for every native type
            // read, and an absent key keeps the constructor's null.
            warp_in_weapon: section
                .read_type_name("WarpInWeapon", 2048)
                .map(str::to_owned),
            warp_in_min_range_weapon: section
                .read_type_name("WarpInMinRangeWeapon", 2048)
                .map(str::to_owned),
            // Phobos `Valueable<bool>`: ReadString then the native first-byte
            // verdict; an absent or invalid value keeps the default.
            use_distance_as_damage: section
                .read_ares_bool("WarpInWeapon.UseDistanceAsDamage", false),
            // `Nullable<int>`: an absent, empty or rejected read keeps unset
            // (the global fallback); a parsed zero is a real override.
            chrono_range_minimum: section.read_phobos_nullable_int("ChronoRangeMinimum", None),
            warp_out_weapon: section
                .read_type_name("WarpOutWeapon", 2048)
                .map(str::to_owned),
        }
    }
}
