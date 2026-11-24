//! Phobos `WeaponTypeExt` strafing keys (`Strafing.Shots`,
//! `Strafing.SimulateBurst`, `Strafing.UseAmmoPerShot`, `Strafing.TargetCell`
//! and `Strafing.EndDelay`).
//!
//! `WeaponTypeExt::LoadFromINIFile` reads the five in its section's own order
//! (`Ext/WeaponType/Body.cpp:108-112`), after the `Strafing=` classifier
//! (`:107`, out of this chain). The declarations are `Nullable<int>` /
//! `Nullable<bool>` (`Ext/WeaponType/Body.h:40-44`) and serialize at
//! `Body.cpp:240-244`, so `<default>`, `none` and `<none>` reset a key to
//! unset; an unset boolean falls back to its `[General]` `Valueable<bool>`
//! global (`Ext/Rules/Body.cpp:264-266`, constructor `Body.h:706-708`, all
//! false), and an unset `Strafing.Shots` reads 5 through `.Get(5)`.
//!
//! Consumers:
//! - `AircraftExt::FireWeapon` (`Ext/Aircraft/Body.cpp:22`, `:36`):
//!   `SimulateBurst` and `UseAmmoPerShot`.
//! - `CheckSpyPlaneCameraCount` (`Ext/Aircraft/Hooks.cpp:1164-1179`): the
//!   `Strafing.Shots` camera cap, only when the key is explicitly set.
//! - `AircraftClass_Mission_Attack_StrafeShots` (`:37`), `GetDelay`
//!   (`:217-222`) and the `TargetCell` store (`:234`).
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;
use crate::rules::phobos_globals::PhobosGlobals;

/// The five `WeaponTypeExt` strafing fields. Each `None` is the native
/// `Nullable`'s unset state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PhobosStrafingRules {
    /// `Strafing.Shots=`, `Nullable<int>`: the bomb count one strafing round
    /// fires; unset reads 5 through `.Get(5)`.
    pub shots: Option<i32>,
    /// `Strafing.SimulateBurst=`, `Nullable<bool>`: alternate
    /// `CurrentBurstIndex` across single-shot strafing passes.
    pub simulate_burst: Option<bool>,
    /// `Strafing.UseAmmoPerShot=`, `Nullable<bool>`: debit Ammo after every
    /// shot of the round instead of once at its end.
    pub use_ammo_per_shot: Option<bool>,
    /// `Strafing.TargetCell=`, `Nullable<bool>`: after the release, fire the
    /// round at the Target's cell instead of the Target.
    pub target_cell: Option<bool>,
    /// `Strafing.EndDelay=`, `Nullable<int>`: the frames after the last shot;
    /// unset derives `(Range + 4 * LeptonsPerCell) / Speed`.
    pub end_delay: Option<i32>,
    /// `Strafing=`, `Nullable<bool>` (`Ext/WeaponType/Body.cpp:107`, declared
    /// `Body.h:39`, constructed empty `Body.h:134`): an explicit answer to
    /// "does this weapon strafe". Unset infers from the projectile
    /// (`Ext/Aircraft/Hooks.cpp:102`: `ROT<=1 && !Inviso && no Trajectory=`;
    /// `Ext/Foot/Body.cpp:840` repeats it as `ROT<2`).
    pub strafing: Option<bool>,
}

impl PhobosStrafingRules {
    /// `WeaponTypeExt::LoadFromINIFile`'s six reads (`Body.cpp:107-112`),
    /// classifier first.
    pub fn from_section(section: &IniSection) -> Self {
        Self {
            strafing: section.read_phobos_nullable_bool("Strafing", None),
            shots: section.read_phobos_nullable_int("Strafing.Shots", None),
            simulate_burst: section.read_phobos_nullable_bool("Strafing.SimulateBurst", None),
            use_ammo_per_shot: section.read_phobos_nullable_bool("Strafing.UseAmmoPerShot", None),
            target_cell: section.read_phobos_nullable_bool("Strafing.TargetCell", None),
            end_delay: section.read_phobos_nullable_int("Strafing.EndDelay", None),
        }
    }

    /// Whether every key is unset: stock rules keep the previous hash stream.
    pub const fn is_default(&self) -> bool {
        self.strafing.is_none()
            && self.shots.is_none()
            && self.simulate_burst.is_none()
            && self.use_ammo_per_shot.is_none()
            && self.target_cell.is_none()
            && self.end_delay.is_none()
    }

    /// `Strafing_Shots.Get(5)`.
    pub fn shots(&self) -> i32 {
        self.shots.unwrap_or(5)
    }

    /// `Strafing_SimulateBurst.Get(RulesExt::Global()->Strafing_SimulateBurst)`.
    pub fn simulate_burst(&self, globals: &PhobosGlobals) -> bool {
        self.simulate_burst
            .unwrap_or(globals.strafing_simulate_burst)
    }

    /// `Strafing_UseAmmoPerShot.Get(RulesExt::Global()->Strafing_UseAmmoPerShot)`.
    pub fn use_ammo_per_shot(&self, globals: &PhobosGlobals) -> bool {
        self.use_ammo_per_shot
            .unwrap_or(globals.strafing_use_ammo_per_shot)
    }

    /// `Strafing_TargetCell.Get(RulesExt::Global()->Strafing_TargetCell)`.
    pub fn target_cell(&self, globals: &PhobosGlobals) -> bool {
        self.target_cell.unwrap_or(globals.strafing_target_cell)
    }
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "phobos_strafing_tests.rs"]
mod tests;
