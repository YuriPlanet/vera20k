//! Authoritative current weapon/turret pair and charge-model index.
//! Native executable controls: tools/spatial_oracle/ifv_turret_switching.{py,json,md}.

use super::GameEntity;
use crate::rules::object_type::{ObjectType, WEAPON_SLOT_COUNT};

/// The current weapon (`TechnoClass+0x138`) and turret (`+0x124`), and the
/// charge turret's saved rearm duration (`+0x2F8`). The Techno constructor
/// stores 0, -1 and 0 (`0x006F2B99..0x006F2BCA`, `0x006F2E98`).
///
/// Only this module's operations write it: `SetGunnerWeapon` the pair
/// ([`GameEntity::set_gunner_weapon`]), FireAt the duration
/// ([`GameEntity::rearm_after_fire`]) and the charge AI the turret
/// ([`GameEntity::update_charge_turret`]). Combat selection and turret facing
/// read the weapon; indexed voxel drawing reads the turret.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct TurretSelection {
    current_weapon_number: i32,
    current_turret_index: i32,
    charge_turret_delay: i32,
}

impl TurretSelection {
    /// The Techno constructor's values.
    pub(crate) const fn constructed() -> Self {
        Self {
            current_weapon_number: 0,
            current_turret_index: -1,
            charge_turret_delay: 0,
        }
    }
}

impl GameEntity {
    pub fn current_weapon_number(&self) -> i32 {
        self.turret_selection.current_weapon_number
    }

    pub fn current_turret_index(&self) -> i32 {
        self.turret_selection.current_turret_index
    }

    /// TechnoClass::SetGunnerWeapon70DC70 -> TechnoType7178B0.
    /// Charge types retain both words; out-of-range signed modes select mode0.
    /// This changes no timer, attachment or RNG state.
    pub(crate) fn set_gunner_weapon(&mut self, mode: i32, object: &ObjectType) {
        if object.gunner_turrets.is_charge_turret() {
            return;
        }
        // Installed SwitchGunner hook10042480 validates against WeaponCount;
        // the default build retains the original eighteen-record boundary.
        let limit = if cfg!(feature = "ares") {
            object.weapon_count
        } else {
            WEAPON_SLOT_COUNT as i32
        };
        let mode = if (0..limit).contains(&mode) { mode } else { 0 };
        self.turret_selection.current_weapon_number = mode;
        self.turret_selection.current_turret_index = object.gunner_turrets.turret_for_weapon(mode);
    }

    /// FireAt6FE4A4/6FF29E retains GetROF's final duration in +2F8 as well
    /// as starting +2EC. Other rearm writers do not change this saved word.
    pub(crate) fn rearm_after_fire(&mut self, frame: i32, duration: i32) {
        self.turret_selection.charge_turret_delay = duration;
        self.rearm_timer.start(frame, duration);
    }

    /// TechnoAI6FA4FB..6FA5BE, after target validity and before missions.
    /// The signed wrapping product precedes IDIV, then the result is clamped.
    /// Only drawing reads this index; no weapon, timer or RNG is changed.
    pub(crate) fn update_charge_turret(&mut self, frame: i32, object: &ObjectType) {
        if !object.gunner_turrets.is_charge_turret()
            || object.turret_count <= 0
            || object.is_gattling
        {
            return;
        }
        let selection = &mut self.turret_selection;
        selection.current_turret_index = if selection.charge_turret_delay <= 0 {
            0
        } else {
            (object
                .turret_count
                .wrapping_mul(self.rearm_timer.remaining(frame))
                / selection.charge_turret_delay)
                .clamp(0, object.turret_count - 1)
        };
    }

    /// Raw native state for component oracle inputs; production uses the
    /// constructor, SetGunnerWeapon and charge AI above.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_gunner_selection_for_test(&mut self, weapon: i32, turret: i32) {
        self.turret_selection.current_weapon_number = weapon;
        self.turret_selection.current_turret_index = turret;
    }
}

#[cfg(test)]
#[path = "gunner_tests.rs"]
mod tests;
