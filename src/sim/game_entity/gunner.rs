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

    /// Ares TechnoClass_Update (Ares.dll 1004B320, hook 0x6F9E50, `#1208`):
    /// a `PassengerTurret=` type's current turret (`+0x124`) is its passenger
    /// count, at most 17 and `TurretCount - 1`.
    #[cfg(feature = "ares")]
    pub(crate) fn update_passenger_turret(&mut self, object: &ObjectType) {
        if object.techno_combat().movement().passenger_turret() {
            let passengers =
                i32::try_from(self.passenger_role.passenger_count()).unwrap_or(i32::MAX);
            self.turret_selection.current_turret_index =
                passengers.min(17).min(object.turret_count.wrapping_sub(1));
        }
    }

    /// TechnoClass::SetGunnerWeapon70DC70 -> TechnoType7178B0 (Ares
    /// TechnoClass_SwitchGunner 10042480 reads indices from 18 on).
    /// Charge types retain both words; out-of-range signed modes select mode0.
    /// This changes no timer, attachment or RNG state.
    /// `ares` is [`RuleSet::ares`](crate::rules::ruleset::RuleSet::ares).
    pub(crate) fn set_gunner_weapon(&mut self, mode: i32, object: &ObjectType, ares: bool) {
        if object.gunner_turrets.is_charge_turret() {
            return;
        }
        // SwitchGunner hook10042480 validates against WeaponCount when Ares
        // plays; stock retains the original eighteen-record boundary.
        let limit = if ares {
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

    /// Whether [`Self::update_charge_turret`] runs for `object`'s type: a
    /// charge turret with turrets, not a Gattling. Others keep their index.
    pub(crate) fn charges_turret(object: &ObjectType) -> bool {
        object.gunner_turrets.is_charge_turret() && object.turret_count > 0 && !object.is_gattling
    }

    /// TechnoAI6FA4FB..6FA5BE, after target validity and before missions.
    /// The signed wrapping product precedes IDIV, then the result is clamped.
    /// Only drawing reads this index; no weapon, timer or RNG is changed.
    pub(crate) fn update_charge_turret(&mut self, frame: i32, object: &ObjectType) {
        if !Self::charges_turret(object) {
            return;
        }
        let rearm_remaining = self.rearm_timer.remaining(frame);
        let selection = &self.turret_selection;
        let index = if selection.charge_turret_delay <= 0 {
            0
        } else {
            (object.turret_count.wrapping_mul(rearm_remaining) / selection.charge_turret_delay)
                .clamp(0, object.turret_count - 1)
        };
        // The index sits in the record's seldom part: an unchanged index
        // writes nothing, so a presentation frame keeps sharing that part.
        if selection.current_turret_index != index {
            self.turret_selection.current_turret_index = index;
        }
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
