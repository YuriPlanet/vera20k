//! Ares 3.0 `Type=Battery` (`src/Misc/SWTypes/Battery.cpp`): a charge-drain
//! power whose activation lends its owner `Battery.Power` and keeps the
//! `Battery.KeepOnline` buildings online and the `Battery.Overpower` ones
//! overpowered until it deactivates.
//!
//! Installed Ares: Activate `0x1006EBD0`, Deactivate `0x1006EC40`,
//! Initialize `0x1006ECA0` (AITargeting LowPower), LoadFromINI `0x1006ECC0`
//! (Action=None and UseChargeDrain forced every pass). The charge-drain
//! ClickFire (`0x10035130`), drain expiry (`0x10035730`), loss
//! (`0x10035610`) and hold (`0x10035750`) owners in `fire`/`mod` call these.
//! The House power owner keeps the lent state ([`AresHousePower`]); its
//! consumers are the power assessment, operational and overpower tests and
//! the radar's low-power arm.
//!
//! Ledger: no RNG draws, no timer writes, no detach calls; both edges only
//! dirty the House's power assessment (`House+0x5778`).
//!
//! [`AresHousePower`]: crate::sim::power_system::AresHousePower

use crate::rules::ruleset::RuleSet;
use crate::rules::superweapon_type::SuperWeaponType;
use crate::sim::intern::InternedId;
use crate::sim::world::Simulation;

/// SW_Battery::Activate `0x1006EBD0`: count the battery, add its power and
/// append its two lists, then dirty the House's power. Always activates.
pub(super) fn activate(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw: &SuperWeaponType,
) {
    let Some(launch) = rules
        .super_weapon_metadata(&sw.id)
        .map(|metadata| metadata.launch())
    else {
        return;
    };
    sim.power_states.house_mut(owner).activate_battery(
        launch.battery_power(),
        launch.battery_keep_online(),
        launch.battery_overpower(),
    );
}

/// SW_Battery::Deactivate `0x1006EC40`: the reverse of [`activate`], each
/// list item removing its first occurrence (`0x1006EB70`).
pub(super) fn deactivate(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw: &SuperWeaponType,
) {
    let Some(launch) = rules
        .super_weapon_metadata(&sw.id)
        .map(|metadata| metadata.launch())
    else {
        return;
    };
    sim.power_states.house_mut(owner).deactivate_battery(
        launch.battery_power(),
        launch.battery_keep_online(),
        launch.battery_overpower(),
    );
}
