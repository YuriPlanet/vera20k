//! Adapted from the author's ra2-mono TryDeductSuperweaponCost and target
//! preflight/insufficient-funds tests, contributed under VERA's GPLv3.
//! VERA's House wallet owns all balances/statistics. Positive grants, checked
//! i32::MIN costs and the shared UI/AI affordability read complete this port;
//! this is not an Ares executable-order or malformed-INI equivalence claim.

use crate::rules::superweapon_type::SuperWeaponType;
use crate::sim::intern::InternedId;
use crate::sim::world::{SimSoundEvent, Simulation};

#[cfg(test)]
#[path = "money_tests.rs"]
mod tests;

pub(super) fn can_afford(sim: &Simulation, owner: InternedId, sw: &SuperWeaponType) -> bool {
    if sw.money_amount == 0 {
        return true;
    }
    sim.houses.get(&owner).is_some_and(|house| {
        sw.money_amount > 0
            || i64::from(house.economy.available_money()) >= -i64::from(sw.money_amount)
    })
}

pub(super) fn valid_target(sim: &Simulation, sw: &SuperWeaponType, cell: (u16, u16)) -> bool {
    // Action=None (or the constructor's missing action) is the ordinary
    // direct-launch cameo path, whose event intentionally carries (0, 0).
    if sw
        .action
        .as_deref()
        .is_none_or(|action| action.eq_ignore_ascii_case("none"))
    {
        return true;
    }
    sim.resolved_terrain
        .as_ref()
        .and_then(|terrain| terrain.cell(cell.0, cell.1))
        .is_some()
        && crate::sim::cell_rect::cell_is_in_playfield_height_aware_in_query(
            (i32::from(cell.0), i32::from(cell.1)),
            sim.playfield_bounds,
            sim.resolved_terrain.as_ref(),
            None,
        )
}

/// Read-only wallet decision, with an owner-scoped presentation fact on refusal.
/// Called before paired-Super mutation, and checked again at settlement.
pub(super) fn admit(
    sim: &mut Simulation,
    owner: InternedId,
    sw_type: InternedId,
    sw: &SuperWeaponType,
) -> bool {
    if can_afford(sim, owner, sw) {
        return true;
    }
    if sim.houses.contains_key(&owner) {
        sim.sound_events
            .push(SimSoundEvent::SuperWeaponInsufficientFunds { owner, sw_type });
    }
    false
}

pub(super) fn transact(
    sim: &mut Simulation,
    owner: InternedId,
    sw_type: InternedId,
    sw: &SuperWeaponType,
) -> bool {
    if !admit(sim, owner, sw_type, sw) {
        return false;
    }
    if sw.money_amount < 0 {
        // Affordability proves the positive cost fits a signed wallet, so the
        // checked conversion cannot overflow even for an authored i32::MIN.
        let cost = sw.money_amount.checked_neg().expect("admitted signed cost");
        crate::sim::credit_income::spend_money(sim, owner, cost);
    } else if sw.money_amount > 0 {
        crate::sim::credit_income::add_credits(sim, owner, sw.money_amount);
    }
    true
}
