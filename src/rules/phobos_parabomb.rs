//! Phobos parabombs: a projectile that parachutes down to the ground instead
//! of flying a trajectory.
//!
//! ```ini
//! Parachuted=             ; Valueable<bool>, false              Ext/BulletType/Body.cpp:79
//! Parachuted.FallRate=    ; Valueable<int>, 1                   :80
//! Parachuted.MaxFallRate= ; Nullable<int>, unset                :81
//! BombParachute=          ; Nullable<AnimTypeClass*>, unset      :82
//! ```
//!
//! Declarations `Ext/BulletType/Body.h:83-86`, constructor `Body.h:145-148`.
//! `Parachuted.MaxFallRate=` falls back to `[General] ParachuteMaxFallRate`
//! and `BombParachute=` to `[General] BombParachute` (`Rules+0xBB8`), both read
//! by the `RulesClass` owner VERA already has.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependency on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;

/// The four parabomb keys of a `BulletType`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhobosParabomb {
    /// `Parachuted=`: `BulletClass::Unlimbo` becomes `SpawnParachuted`
    /// (`Ext/Bullet/Hooks.cpp:573-583`).
    pub parachuted: bool,
    /// `Parachuted.FallRate=`: leptons the parachute adds to the fall each
    /// frame (`:130`).
    pub fall_rate: i32,
    /// `Parachuted.MaxFallRate=`, `Nullable`: `None` keeps
    /// `[General] ParachuteMaxFallRate`.
    pub max_fall_rate: Option<i32>,
    /// `BombParachute=`: the canopy AnimType, else `[General] BombParachute`.
    pub bomb_parachute: Option<String>,
}

impl Default for PhobosParabomb {
    fn default() -> Self {
        Self {
            parachuted: false,
            fall_rate: 1,
            max_fall_rate: None,
            bomb_parachute: None,
        }
    }
}

impl PhobosParabomb {
    pub fn from_section(section: &IniSection) -> Self {
        let defaults = Self::default();
        Self {
            parachuted: section.read_bool("Parachuted", defaults.parachuted),
            fall_rate: section.read_int("Parachuted.FallRate", defaults.fall_rate),
            max_fall_rate: section.read_phobos_nullable_int("Parachuted.MaxFallRate", None),
            bomb_parachute: section
                .read_phobos_nullable_with("BombParachute", None, |raw| Some(raw.to_owned())),
        }
    }

    /// `Parachuted_MaxFallRate.Get(RulesClass::ParachuteMaxFallRate)`
    /// (`Ext/Bullet/Hooks.cpp:131`).
    pub fn max_fall_rate(&self, global: i32) -> i32 {
        self.max_fall_rate.unwrap_or(global)
    }
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "phobos_parabomb_tests.rs"]
mod tests;
