//! Original contribution by baixiong443, adapting the contributor-identified
//! ra2-mono cost/preflight code (donor source unavailable for independent
//! comparison). Public Ares contract and VERA-specific limits:
//! `docs/ares-superweapon-money.md`. The existing House wallet owns balances,
//! spending and score; there is no competing financial state here.

use crate::rules::superweapon_type::{SuperWeaponKind, SuperWeaponType};
use crate::sim::intern::InternedId;
use crate::sim::world::{SimSoundEvent, Simulation};

#[cfg(test)]
#[path = "money_tests.rs"]
mod tests;

/// Ares separates source selection from final settlement (ChronoSphere
/// `NoMoney`, SWTypeExt::Launch). VERA has no saved one-use paid selection;
/// reject these optional settings instead of paying a repeatable source.
/// https://github.com/Ares-Developers/Ares/blob/4f1d929920aca31924c6cd4d3dfa849daa65252a/src/Ext/SWType/Body.cpp#L469
pub(super) fn configuration_supported(sw: &SuperWeaponType) -> bool {
    sw.money_amount == 0
        || (!sw.pre_click
            && !sw.post_click
            && !matches!(
                sw.kind,
                SuperWeaponKind::ChronoSphere | SuperWeaponKind::ChronoWarp
            ))
}

pub(super) fn can_afford(sim: &Simulation, owner: InternedId, sw: &SuperWeaponType) -> bool {
    if sw.money_amount == 0 {
        return true;
    }
    sim.houses.get(&owner).is_some_and(|house| {
        sw.money_amount > 0
            || i64::from(house.economy().available_money()) >= -i64::from(sw.money_amount)
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
    super::targeting::on_map(sim, cell)
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
