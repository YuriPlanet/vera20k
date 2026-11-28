//! Phobos `Ammo.*` deploy keys (`Ext/UnitType/Body.cpp:45-49`).
//!
//! Three mechanisms share these fields:
//! - `Ammo.AddOnDeploy=` rewrites a simple deployer's ammo when a deploy or
//!   undeploy completes (`UnitExt::HandleOnDeployAmmoChange`,
//!   `Ext/Unit/Body.cpp:239-256`; calls at `Ext/Unit/Hooks.SimpleDeployer.cpp:167,224`).
//! - `Ammo.DeployUnlockMinimumAmount=` / `.MaximumAmount=` gate whether the
//!   player may deploy (`UnitExt::HasAmmoToDeploy`, `:218-237`, consumed by the
//!   `CanShowDeployCursor` hook `0x7010C1`, `Ext/Techno/Hooks.cpp:1455-1470`).
//! - `Ammo.AutoDeployMinimumAmount=` / `.MaximumAmount=` queue or cancel the
//!   Unload mission from `UnitExt::DepletedAmmoActions` (`:149-195`), run after
//!   `FootClass::AI` in the `UnitClass::AI` hook `0x736480`
//!   (`Ext/Techno/Hooks.cpp:56-66`).
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;

/// The five `UnitTypeExt` deploy-ammo fields, all `Valueable<int>`
/// (`Ext/UnitType/Body.h:30-34`): `AddOnDeploy` constructs 0 (no change), both
/// ranges construct -1 (unbounded).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhobosAmmoDeploy {
    /// `Ammo.AddOnDeploy=`: added to the ammo on a completed deploy and again
    /// on a completed undeploy, clamped to `[0, type Ammo]`.
    pub add_on_deploy: i32,
    /// `Ammo.AutoDeployMinimumAmount=`, default -1.
    pub auto_deploy_minimum: i32,
    /// `Ammo.AutoDeployMaximumAmount=`, default -1.
    pub auto_deploy_maximum: i32,
    /// `Ammo.DeployUnlockMinimumAmount=`, default -1.
    pub deploy_unlock_minimum: i32,
    /// `Ammo.DeployUnlockMaximumAmount=`, default -1.
    pub deploy_unlock_maximum: i32,
}

impl Default for PhobosAmmoDeploy {
    fn default() -> Self {
        Self {
            add_on_deploy: 0,
            auto_deploy_minimum: -1,
            auto_deploy_maximum: -1,
            deploy_unlock_minimum: -1,
            deploy_unlock_maximum: -1,
        }
    }
}

impl PhobosAmmoDeploy {
    /// The reads of `UnitTypeExt::LoadFromINIFile` (`Ext/UnitType/Body.cpp:45-49`).
    pub fn from_section(section: &IniSection) -> Self {
        Self {
            add_on_deploy: section.read_ares_int("Ammo.AddOnDeploy", 0),
            auto_deploy_minimum: section.read_ares_int("Ammo.AutoDeployMinimumAmount", -1),
            auto_deploy_maximum: section.read_ares_int("Ammo.AutoDeployMaximumAmount", -1),
            deploy_unlock_minimum: section.read_ares_int("Ammo.DeployUnlockMinimumAmount", -1),
            deploy_unlock_maximum: section.read_ares_int("Ammo.DeployUnlockMaximumAmount", -1),
        }
    }

    /// `UnitExt::HasAmmoToDeploy` (`Ext/Unit/Body.cpp:218-237`): both bounds
    /// unset admits every count; otherwise the ammo must sit inside the band.
    pub fn has_ammo_to_deploy(&self, ammo: i32) -> bool {
        if self.deploy_unlock_minimum < 0 && self.deploy_unlock_maximum < 0 {
            return true;
        }
        (self.deploy_unlock_minimum < 0 || ammo >= self.deploy_unlock_minimum)
            && (self.deploy_unlock_maximum < 0 || ammo <= self.deploy_unlock_maximum)
    }

    /// `DepletedAmmoActions`' band test: `None` when both bounds are unset,
    /// which is the mechanism's own "not configured" gate.
    pub fn auto_deploy_band(&self) -> Option<(i32, i32)> {
        (self.auto_deploy_minimum >= 0 || self.auto_deploy_maximum >= 0)
            .then_some((self.auto_deploy_minimum, self.auto_deploy_maximum))
    }

    /// `HandleOnDeployAmmoChange`: `original + add`, clamped to
    /// `[0, max_ammo]`. A zero add keeps the count. `max_ammo` may be the
    /// type's unset `-1`; `std::clamp`'s ordered form then answers 0 for a
    /// negative sum and -1 above zero, as native does without a panic.
    pub fn ammo_after_deploy(&self, original: i32, max_ammo: i32) -> i32 {
        let value = original.wrapping_add(self.add_on_deploy);
        if value < 0 {
            0
        } else if max_ammo < value {
            max_ammo
        } else {
            value
        }
    }
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "phobos_ammo_deploy_tests.rs"]
mod tests;
