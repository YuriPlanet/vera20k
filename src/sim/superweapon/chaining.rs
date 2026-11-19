//! Phobos `SW.Next` chained launches and `SW.Link` charge groups
//! (`Ext/SWType/FireSuperWeapon.cpp:305-486`).
//!
//! `SWTypeExt::FireSuperWeaponExt` (`:13-40`) runs from the common
//! `SuperClass::Launch` tail (`0x6CDE40`): after a Super's own effect it
//! applies, in order, `ApplySWNext`, the type conversion and `ApplyLinkedSW`.
//! VERA calls [`apply_phobos_launch_tail`] from both dispatch tails
//! (stock kinds and Ares handlers).
//!
//! `ApplySWNext` launches each selected next Super at the same cell: with
//! `SW.Next.RealLaunch` (default yes) only a present, charged, unsuspended and
//! affordable one, consuming its charge; otherwise forcibly, with the charge
//! timer restored. Each next type's `SW.Inhibitors`/`SW.Designators` lists
//! are checked unless the two ignore flags bypass them (inhibitors default
//! checked, designators default ignored). `ApplyLinkedSW` grants a missing
//! linked Super one-time, or resets/readies a present one, under
//! `SW.Link.Grant`/`.Ready`/`.Reset`.
//!
//! Both selections share `WeightedRollsHandler` (`Ext/SWType/SWHelpers.cpp:6`):
//! no `RollChances` means one sure roll; otherwise one Scenario
//! `RandomDouble` per chance, skip when `dice > chance`, then
//! `ChooseOneWeighted` over the roll's weight list (the last list repeats),
//! ignoring an index past the list. `ChooseOneWeighted`
//! (`Utilities/GeneralUtils.cpp:108`) sums the integer weights in binary32 and
//! picks the first prefix whose ratio exceeds the same dice draw.
//!
//! Ledger: one Scenario `RandomDouble` per roll (the two draws per swap above
//! included); no timers beyond each chained launch's and each link's charge
//! writes; a chained `Launch` draws what its own effect draws.
//!
//! RESIDUALS:
//! - A cyclic `SW.Next`/`SW.Link` chain recurses without bound natively;
//!   VERA stops after [`MAX_CHAIN_DEPTH`] levels and logs.
//! - `SW.Link.Grant` also adds the linked Super's cameo to the current
//!   player's sidebar (`MouseClass::AddCameo`, repaint) natively; VERA has no
//!   sidebar owner here and leaves the grant to the ordinary availability
//!   pass.
//! - An Ares-handler launch that refuses its own activation returns before the
//!   shared tail in VERA, so its Convert/Next/Link arms do not run; native's
//!   refused handler falls through the stock case and still reaches
//!   `0x6CDE40`.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/, sim/rng, sim/world.

use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::rules::superweapon_type::SuperWeaponType;
use crate::sim::intern::InternedId;
use crate::sim::rng::SimRng;
use crate::sim::world::{FrameEffects, SimSoundEvent, Simulation};

/// Deepest nested `SW.Next`/`SW.Link` launch VERA follows before stopping a
/// cyclic chain (native recurses to stack exhaustion).
pub(super) const MAX_CHAIN_DEPTH: u32 = 16;

/// `Simulation`'s `SW.Next`/`SW.Link` nesting counter. Transient owner: it
/// only stops a cyclic chain (native would recurse to stack exhaustion) and is
/// zero outside a launch.
#[derive(Debug, Default, Clone)]
pub(crate) struct ChainDepth {
    depth: u32,
}

impl ChainDepth {
    pub(crate) fn at_limit(&self) -> bool {
        self.depth >= MAX_CHAIN_DEPTH
    }

    fn enter(&mut self) {
        self.depth += 1;
    }

    fn leave(&mut self) {
        self.depth -= 1;
    }
}

/// The `FireSuperWeaponExt` tail, in its native order: `SW.Next`, the type
/// conversion, then `SW.Link`.
#[allow(clippy::too_many_arguments)]
pub(super) fn apply_phobos_launch_tail(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw: &SuperWeaponType,
    cell: (u16, u16),
    overlay_registry: Option<&OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) {
    if sim.super_weapon_chain_depth.at_limit() {
        log::warn!(
            "Phobos superweapon chain from \"{}\" stopped at depth {MAX_CHAIN_DEPTH}",
            sw.id
        );
        return;
    }
    sim.super_weapon_chain_depth.enter();
    // Native order: LimboDelivery, LimboKill, Detonate, SW.Next, Convert,
    // SW.Link (`Ext/SWType/FireSuperWeapon.cpp:13-40`).
    super::limbo::apply_limbo_delivery(sim, rules, owner, sw, overlay_registry, effects);
    super::limbo::apply_limbo_kill(sim, rules, owner, sw, overlay_registry, effects);
    super::detonate::apply(sim, rules, owner, sw, cell, overlay_registry, effects);
    let chaining = rules
        .phobos_super_weapon_chaining(&sw.id)
        .cloned()
        .unwrap_or_default();
    apply_sw_next(
        sim,
        rules,
        owner,
        &chaining,
        cell,
        overlay_registry,
        effects,
    );
    super::fire::apply_sw_type_conversion(sim, rules, owner, sw, effects);
    apply_linked_sw(sim, rules, owner, sw, &chaining);
    // Phobos `SWTypeExt::ApplyActivatedMessage`/`ApplyActivatedEva`
    // (`Ext/SWType/FireSuperWeapon.cpp:40-42`): the launch tail's last step,
    // after `SW.Link`. The app owns the Owner/Allies/Enemies column against
    // the client's current player.
    if let Some(sw_type) = sim.interner.get(&sw.id) {
        sim.sound_events
            .emit(SimSoundEvent::SuperWeaponActivatedMessage { owner, sw_type });
    }
    sim.super_weapon_chain_depth.leave();
}

/// `SWTypeExt::ApplySWNext`: one launch per selected index, in roll order.
#[allow(clippy::too_many_arguments)]
fn apply_sw_next(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    chaining: &crate::rules::phobos_sw_chaining::PhobosSuperWeaponChaining,
    cell: (u16, u16),
    overlay_registry: Option<&OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) {
    if chaining.next().is_empty() {
        return;
    }
    let selected = if chaining.next_random_weights().is_empty() {
        (0..chaining.next().len()).collect()
    } else {
        weighted_rolls(
            &chaining.next_roll_chances(),
            chaining.next_random_weights(),
            chaining.next().len(),
            &mut sim.scenario_rng,
        )
    };
    for index in selected {
        let name = chaining.next()[index].clone();
        launch_next(
            sim,
            rules,
            owner,
            chaining,
            &name,
            cell,
            overlay_registry,
            effects,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn launch_next(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    chaining: &crate::rules::phobos_sw_chaining::PhobosSuperWeaponChaining,
    name: &str,
    cell: (u16, u16),
    overlay_registry: Option<&OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) {
    let Some(next_sw) = rules.super_weapon(name) else {
        return;
    };
    let Some(type_id) = sim.interner.get(name) else {
        return;
    };
    let Some(instance) = sim
        .super_weapons
        .get(&owner)
        .and_then(|supers| supers.get(&type_id))
    else {
        return;
    };
    let real_launch = chaining.next_real_launch();
    let allowed = !real_launch
        || (instance.is_active()
            && instance.is_ready()
            && !instance.is_suspended()
            && affordable(sim, owner, next_sw));
    if !allowed || !next_gates_pass(sim, rules, owner, next_sw, chaining, cell) {
        return;
    }
    super::fire::launch_chained_super(
        sim,
        rules,
        owner,
        type_id,
        next_sw,
        cell,
        overlay_registry,
        effects,
        real_launch,
    );
}

/// `pHouse->CanTransactMoney(pNextTypeExt->Money_Amount)`: Ares' wallet
/// inline. Without the Ares capability the key is unread (0), which always
/// passes.
pub(super) fn affordable(sim: &Simulation, owner: InternedId, next_sw: &SuperWeaponType) -> bool {
    {
        super::money::can_transact(&sim.houses, owner, next_sw.money_amount)
    }
}

/// The type's `HasInhibitor`/`HasDesignator` around the launch cell. The two
/// lists are the Ares-owned `SW.Inhibitors`/`SW.Designators`; without the
/// Ares capability they are unread, so the inhibitor gate is open and the
/// designator gate needs none.
fn next_gates_pass(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    next_sw: &SuperWeaponType,
    chaining: &crate::rules::phobos_sw_chaining::PhobosSuperWeaponChaining,
    cell: (u16, u16),
) -> bool {
    influence_gates_pass(
        sim,
        rules,
        owner,
        next_sw,
        cell,
        chaining.next_ignore_inhibitors(),
        chaining.next_ignore_designators(),
    )
}

/// The shared inhibitor/designator gate for a launch at `cell`.
pub(super) fn influence_gates_pass(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw: &SuperWeaponType,
    cell: (u16, u16),
    ignore_inhibitors: bool,
    ignore_designators: bool,
) -> bool {
    {
        let influence = rules
            .super_weapon_metadata(&sw.id)
            .map(|metadata| metadata.influence())
            .cloned()
            .unwrap_or_default();
        (ignore_inhibitors || !super::targeting::has_inhibitor(sim, rules, owner, &influence, cell))
            && (ignore_designators
                || super::targeting::has_designator(sim, rules, owner, &influence, cell))
    }
}

/// `SWTypeExt::ApplyLinkedSW`: one action per selected linked type.
fn apply_linked_sw(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw: &SuperWeaponType,
    chaining: &crate::rules::phobos_sw_chaining::PhobosSuperWeaponChaining,
) {
    if chaining.link().is_empty() {
        return;
    }
    if sim
        .houses
        .get(&owner)
        .is_none_or(|house| house.is_defeated())
    {
        return;
    }
    let selected = if chaining.link_random_weights().is_empty() {
        (0..chaining.link().len()).collect()
    } else {
        weighted_rolls(
            &chaining.link_roll_chances(),
            chaining.link_random_weights(),
            chaining.link().len(),
            &mut sim.scenario_rng,
        )
    };
    let mut is_active = false;
    for index in selected {
        let name = chaining.link()[index].clone();
        is_active |= link_one(sim, rules, owner, chaining, &name);
    }
    // `if (isActive && notObserver && pHouse->IsCurrentPlayer())` (`:482`).
    // The observer half is a sim fact (`[0xAC1198]`); the current-player half
    // is not, so the event carries the granting House and the app gates on it.
    if !is_active || sim.session.is_observer_house(owner) {
        return;
    }
    let Some(sw_type) = sim.interner.get(&sw.id) else {
        return;
    };
    sim.sound_events
        .emit(SimSoundEvent::SuperWeaponLinkedAcquired { owner, sw_type });
}

/// One `linkedSW` lambda call (`:417-456`). Answers native's `isActive`,
/// which is `granted` unless an arm below readied or reset the Super.
fn link_one(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    chaining: &crate::rules::phobos_sw_chaining::PhobosSuperWeaponChaining,
    name: &str,
) -> bool {
    let Some(next_sw) = rules.super_weapon(name) else {
        return false;
    };
    let Some(type_id) = sim.interner.get(name) else {
        return false;
    };
    let present = sim
        .super_weapons
        .get(&owner)
        .and_then(|supers| supers.get(&type_id))
        .is_some_and(|instance| instance.is_active());
    let mut granted = false;
    if chaining.link_grant() && !present {
        // `pSuper->Grant(true, false, false)`: a one-time (instantly charged)
        // grant with no announcement and no on-hold state.
        granted = super::grant_super(
            &sim.interner,
            &sim.session,
            &mut sim.substrate,
            &sim.houses,
            &mut sim.super_weapons,
            rules,
            owner,
            type_id,
            true,
            false,
        );
    }
    let active = granted || present;
    if !active {
        return false;
    }
    let frame = sim.session.binary_frame();
    if chaining.link_reset() {
        reset(sim, rules, owner, name);
        return true;
    }
    if chaining.link_ready() || (granted && initial_ready(rules, next_sw)) {
        if let Some(instance) = sim.super_weapons.instance_mut(&owner, &type_id) {
            // `pSuper->RechargeTimer.TimeLeft = 0; pSuper->SetReadiness(true)`.
            instance.store_charge_timer(crate::sim::timer::CdTimer::started(frame as i32, 0));
            instance.set_readiness(true);
        }
        return true;
    }
    if granted {
        // Reset a granted link that is neither readied nor reset above.
        reset(sim, rules, owner, name);
    }
    // The lambda's `isActive` keeps `granted`: a present, un-granted link that
    // nothing readied or reset did not activate.
    granted
}

fn reset(sim: &mut Simulation, rules: &RuleSet, owner: InternedId, name: &str) {
    let name = name.to_owned();
    super::reset_super(
        &sim.interner,
        &sim.session,
        &mut sim.substrate,
        &mut sim.sound_events,
        &mut sim.super_weapons,
        rules,
        owner,
        &name,
    );
}

/// `AresSuperWeaponCharge::initial_ready`, the `SW.InitialReady` arm of
/// `SW.Link.Ready` for a granted Super. Without Ares the key is unread.
fn initial_ready(rules: &RuleSet, next_sw: &SuperWeaponType) -> bool {
    {
        rules
            .super_weapon_metadata(&next_sw.id)
            .is_some_and(|metadata| metadata.charge().initial_ready)
    }
}

/// `SWTypeExt::WeightedRollsHandler` (`Ext/SWType/SWHelpers.cpp:6-44`).
pub(super) fn weighted_rolls(
    chances: &[f64],
    weights: &[Vec<i32>],
    size: usize,
    rng: &mut SimRng,
) -> Vec<usize> {
    let roll_once = chances.is_empty();
    let rolls = if roll_once { 1 } else { chances.len() };
    let mut indices = Vec::new();
    for roll in 0..rolls {
        let dice = rng.random_double();
        if !roll_once && dice > chances[roll] {
            continue;
        }
        // More rolls than weight lists reuse the last list.
        let list = (weights.len().saturating_sub(1)).min(roll);
        let Some(list) = weights.get(list) else {
            continue;
        };
        let index = choose_one_weighted(dice, list);
        // A weight past the object list is ignored; -1 answers none.
        if index >= 0 && (index as usize) < size {
            indices.push(index as usize);
        }
    }
    indices
}

/// `GeneralUtils::ChooseOneWeighted` (`Utilities/GeneralUtils.cpp:108-124`):
/// both sums are binary32 and each step adds one integer weight.
fn choose_one_weighted(dice: f64, weights: &[i32]) -> i32 {
    let mut sum: f32 = 0.0;
    for &weight in weights {
        sum += weight as f32;
    }
    let mut sum2: f32 = 0.0;
    for (index, &weight) in weights.iter().enumerate() {
        sum2 += weight as f32;
        if dice < f64::from(sum2 / sum) {
            return index as i32;
        }
    }
    -1
}

#[cfg(test)]
#[path = "chaining_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "phobos_sw_messages_tests.rs"]
mod phobos_sw_messages_tests;
