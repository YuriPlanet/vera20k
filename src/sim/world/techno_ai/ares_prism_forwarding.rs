//! Ares Prism forwarding (BuildingExt `cPrismForwarding`, `+0x10`), which
//! replaces the stock Prism support of Mission_Attack and ProcessDelayedFire
//! in Ares builds. A firing tower (`PrismForwarding.*` of
//! `rules::ares_prism_forwarding`, a `CanAttack` type) recruits whole chains
//! of supporters in one Mission_Attack visit; each supporter beams its charge
//! at the tower it feeds once its own charge ends, adding its support
//! modifier and flat damage to that tower's reserves, and the firing tower's
//! delayed shot takes the sum.
//!
//! The network lives on each building ([`AresPrismForwarding`]): the towers
//! feeding it (`Senders`, in recruitment order), the tower it feeds
//! (`SupportTarget`), the frames before it starts charging
//! (`PrismChargeDelay`) and its reserves. The gamemd fields keep their stock
//! owners: `+0x704` stage, `+0x708` weapon or beam point and `+0x714`
//! countdown are [`BuildingDelayedFire`] (Idle is no pending fire; a Master
//! fires a weapon, a Slave a support beam), `+0x664` SupportingPrisms is
//! [`PrismSupport`], which Ares sets to the longest chain behind a tower.
//!
//! Hooks: Mission_Attack `0x44B2FE` (`100167E0`, [`mission_attack_prism`]),
//! ProcessDelayedFire `0x4503F0` (`100169A0`, [`update_prism`]) with the
//! support beam `0x44ABD0` (`10016490`, [`fire_laser`]), GetFireError
//! `0x447FAE` (`10016360`, in `combat::fire_error`), the delayed shot's
//! bonus (`combat::world_fire`), and the cleanup hooks that take a tower out
//! of its network ([`ObjectSubstrate::ares_prism_remove_from_network`]):
//! ReceiveDamage `0x4424EF`, Sell `0x447113`, ChangeOwner `0x448277`
//! ([`Simulation::ares_prism_change_owner`]), Detach_All `0x44EBF0`,
//! UpdatePowered `0x454B3D`, Install_Drain_Link `0x70FD9A` and
//! TemporalClass::InitiateWarp `0x71AF76`.
//!
//! Ledger: one Scenario draw per support beam whose `SupportWeapon` has a
//! `Report=`; every support beam restarts the tower's ReloadTimer
//! (Techno+1FC, the ammo reload clock), not its weapon rearm timer, and the
//! network test reads that clock; no DetachAll.
//!
//! RESIDUALS:
//! - FireLaser's `IsRadBeam=` and `IsElectricBolt=` support weapons
//!   (`10016490`) draw no beam or bolt (presentation; the bolt would also
//!   take its Main draws and spark system). The `IsLaser=` beam, `Report=`
//!   and `ROF=` of a support weapon are ported.
//! - The BuildingExt destructor's RemoveFromNetwork: a building removed
//!   without a Detach_All leaves its id in its neighbours' links, which every
//!   walk below skips.

use super::{
    ACTIVE_ANIM_SLOT, DelayedFire, ObjectAiCtx, SPECIAL_ANIM_SLOT, fire_error_at, is_elite,
};
use crate::map::entities::EntityCategory;
use crate::rules::ares_prism_forwarding::PrismForwardingRules;
use crate::rules::object_type::ObjectType;
use crate::rules::ruleset::RuleSet;
use crate::sim::building_art::requested_damage_state;
use crate::sim::combat::BuildingShot;
use crate::sim::combat::combat_weapon::{self, WeaponSlot};
use crate::sim::combat::fire_coord;
use crate::sim::combat::fire_error::FireError;
use crate::sim::entity_store::EntityStore;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::projectile::ProjectileCoord;
use crate::sim::world::{ObjectSubstrate, Simulation};

/// One building's `cPrismForwarding` (BuildingExt `+0x10`; save order
/// `10018C70`: Senders, SupportTarget, PrismChargeDelay, DamageReserve,
/// ModifierReserve). Every link is a building's stable id.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct AresPrismForwarding {
    /// `Senders` (`+0x4`): the towers feeding this one, in the order they
    /// were attached.
    senders: Vec<u64>,
    /// `SupportTarget` (`+0x1C`): the tower this one feeds.
    support_target: Option<u64>,
    /// `PrismChargeDelay` (`+0x20`): frames before the countdown starts.
    charge_delay: i32,
    /// `DamageReserve` (`+0x24`): flat damage the feeding towers added.
    damage_reserve: i32,
    /// `ModifierReserve` (`+0x28`, a double): the support percent the
    /// feeding towers added. Every addend is an int, so the double sum is
    /// exact and an i64 holds it bit for bit within +-2^53.
    modifier_reserve: i64,
}

impl AresPrismForwarding {
    /// Folded only while the building is part of a network or holds a
    /// reserve.
    pub(crate) fn hash_state(&self, hasher: &mut impl std::hash::Hasher) {
        if *self != Self::default() {
            std::hash::Hash::hash(b"ares-prism-forwarding-v1", hasher);
            std::hash::Hash::hash(self, hasher);
        }
    }

    /// The towers feeding this one.
    #[cfg(test)]
    pub(crate) fn senders(&self) -> &[u64] {
        &self.senders
    }

    /// The tower this one feeds.
    #[cfg(test)]
    pub(crate) fn support_target(&self) -> Option<u64> {
        self.support_target
    }

    /// `PrismChargeDelay`.
    #[cfg(test)]
    pub(crate) fn charge_delay(&self) -> i32 {
        self.charge_delay
    }

    /// Install a fixture's modifier reserve.
    #[cfg(test)]
    pub(crate) fn set_modifier_reserve_for_test(&mut self, reserve: i32) {
        self.modifier_reserve = i64::from(reserve);
    }
}

/// The Building's type and its `PrismForwarding.*`.
fn tower_type<'r>(
    sim: &Simulation,
    rules: &'r RuleSet,
    id: u64,
) -> Option<(&'r ObjectType, &'r PrismForwardingRules)> {
    let entity = sim.substrate.entities.get(id)?;
    if entity.category() != EntityCategory::Structure {
        return None;
    }
    let object = sim.interner.object_type(entity.type_ref(), rules)?;
    Some((object, object.building_type_ext().prism_forwarding()))
}

/// The type's `DelayedFireDelay=` (BuildingType `+0x16EC`, from its art).
fn delayed_fire_delay(rules: &RuleSet, object: &ObjectType) -> i32 {
    rules
        .art()
        .resolve_metadata_entry(&object.id, &object.image)
        .map_or(0, |art| art.delayed_fire_delay)
}

/// `GetPosition_2` (vt+0xAC, `0x00459EF0`) is Location less 128 leptons on
/// X and Y; only differences of two buildings' coordinates are taken.
fn location(sim: &Simulation, id: u64) -> Option<[i32; 3]> {
    let entity = sim.substrate.entities.get(id)?;
    let coord = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
    Some([coord.x, coord.y, coord.z])
}

/// `(int)CoordStruct::DistanceFrom` (`10018A10`): the int differences, their
/// double squares summed in X, Y, Z order, `sqrt` and `_ftol2`. Map
/// coordinates keep every product and sum exact, so the x87 evaluation and
/// IEEE binary64 agree.
fn distance(from: [i32; 3], to: [i32; 3]) -> i32 {
    let squared = (0..3)
        .map(|axis| {
            let delta = f64::from(to[axis].wrapping_sub(from[axis]));
            delta * delta
        })
        .fold(0.0, |sum, square| sum + square);
    squared.sqrt() as i32
}

/// The tower's weapon-0 FLH (`GetFLH(0, {0, 0, 0})`, vt+0xB0).
fn weapon0_flh(sim: &Simulation, rules: &RuleSet, id: u64) -> Option<ProjectileCoord> {
    let entity = sim.substrate.entities.get(id)?;
    let object = sim.interner.object_type(entity.type_ref(), rules)?;
    Some(
        fire_coord::fire_coordinate(
            sim,
            rules,
            &fire_coord::FireSource::of_entity(entity),
            object,
            0,
            (entity.weapon_burst.index() & 1) as u8,
            crate::rules::flh::Flh::default(),
        )
        .coord,
    )
}

fn senders(entities: &EntityStore, id: u64) -> Vec<u64> {
    entities
        .get(id)
        .map_or_else(Vec::new, |entity| entity.ares_prism.senders.clone())
}

fn sender_at(entities: &EntityStore, id: u64, index: usize) -> Option<u64> {
    entities
        .get(id)
        .and_then(|entity| entity.ares_prism.senders.get(index).copied())
}

/// Drop `sender` from `tower`'s Senders (the first match, as
/// `DynamicVectorClass::Remove`).
fn remove_sender(entities: &mut EntityStore, tower: u64, sender: u64) {
    if let Some(entity) = entities.get_mut(tower)
        && let Some(index) = entity
            .ares_prism
            .senders
            .iter()
            .position(|&id| id == sender)
    {
        entity.ares_prism.senders.remove(index);
    }
}

/// `cPrismForwarding::SetSupportTarget @ 10018E50`: leave the old target's
/// Senders, then join the new one's unless already listed.
fn set_support_target(entities: &mut EntityStore, id: u64, target: Option<u64>) {
    let Some(current) = entities
        .get(id)
        .map(|entity| entity.ares_prism.support_target)
    else {
        return;
    };
    if current == target {
        return;
    }
    if let Some(old) = current {
        remove_sender(entities, old, id);
    }
    if let Some(entity) = entities.get_mut(id) {
        entity.ares_prism.support_target = target;
    }
    if let Some(new) = target
        && let Some(entity) = entities.get_mut(new)
        && !entity.ares_prism.senders.contains(&id)
    {
        entity.ares_prism.senders.push(id);
    }
}

/// `cPrismForwarding::RemoveAllSenders @ 10018AE0`: from the last sender
/// back, each leaves its own target's list and forgets it; the list is then
/// emptied.
fn remove_all_senders(entities: &mut EntityStore, id: u64) {
    let count = entities
        .get(id)
        .map_or(0, |entity| entity.ares_prism.senders.len());
    for index in (0..count).rev() {
        let Some(sender) = sender_at(entities, id, index) else {
            continue;
        };
        let Some(target) = entities
            .get(sender)
            .and_then(|entity| entity.ares_prism.support_target)
        else {
            continue;
        };
        remove_sender(entities, target, sender);
        if let Some(entity) = entities.get_mut(sender) {
            entity.ares_prism.support_target = None;
        }
    }
    if let Some(entity) = entities.get_mut(id) {
        entity.ares_prism.senders.clear();
    }
}

/// `cPrismForwarding::RemoveFromNetwork @ 10018BB0`. A tower still waiting
/// to charge, or any tower with `cease`, goes idle at once (stage Idle,
/// `+0x714 = 0`, reserves cleared); one already charging keeps charging.
/// Either way it leaves its target, and each tower feeding it, from the last
/// back, leaves the network without `cease`.
fn remove_from_network(entities: &mut EntityStore, id: u64, cease: bool) {
    let Some(entity) = entities.get_mut(id) else {
        return;
    };
    if entity.category() != EntityCategory::Structure {
        return;
    }
    if entity.ares_prism.charge_delay != 0 || cease {
        entity.ares_prism.charge_delay = 0;
        entity.ares_prism.modifier_reserve = 0;
        entity.ares_prism.damage_reserve = 0;
        entity.building.pending_building_fire.pending = None;
    }
    set_support_target(entities, id, None);
    let count = entities
        .get(id)
        .map_or(0, |entity| entity.ares_prism.senders.len());
    for index in (0..count).rev() {
        if let Some(sender) = sender_at(entities, id, index) {
            remove_from_network(entities, sender, false);
        }
    }
}

impl ObjectSubstrate {
    /// [`remove_from_network`] for a building that becomes unavailable:
    /// destroyed (`cease`, hook `0x4424EF`), sold (`0x447113`), detached
    /// (`0x44EBF0`), gone offline (`0x454B3D`), drained (`0x70FD9A`) or
    /// warped (`0x71AF76`); a capture passes no `cease` (`0x448277`).
    pub(crate) fn ares_prism_remove_from_network(&mut self, id: u64, cease: bool) {
        remove_from_network(&mut self.entities, id, cease);
    }

    /// The delayed shot's `DamageMultiplier` (`100169A0`):
    /// `(int)((ModifierReserve + 100) * 256) / 100`, `_ftol2` answering
    /// `0x80000000` out of range and the division signed.
    pub(super) fn ares_prism_shot_multiplier(&self, id: u64) -> i32 {
        let reserve = self
            .entities
            .get(id)
            .map_or(0, |entity| entity.ares_prism.modifier_reserve);
        let product = (reserve as f64 + 100.0) * 256.0;
        let scaled = if (-2_147_483_648.0..2_147_483_648.0).contains(&product) {
            product as i32
        } else {
            i32::MIN
        };
        scaled / 100
    }
}

impl Simulation {
    /// The flat damage `100169A0` adds to the delayed shot's bullet:
    /// `PrismForwarding.DamageAdd` plus the tower's damage reserve.
    pub(crate) fn ares_prism_shot_damage_add(&self, id: u64, rules: &RuleSet) -> i32 {
        let Some((_, prism)) = tower_type(self, rules, id) else {
            return 0;
        };
        let reserve = self
            .substrate
            .entities
            .get(id)
            .map_or(0, |entity| entity.ares_prism.damage_reserve);
        prism.damage_add().wrapping_add(reserve)
    }

    /// Whether GetFireError's busy test spares the building (`10016360`,
    /// hook `0x447FAE`): a charging supporter of a `CanAttack` type with
    /// `PrismForwarding.BreakSupport=yes` may still turn master.
    pub(crate) fn ares_prism_breaks_support(&self, id: u64, rules: &RuleSet) -> bool {
        let Some((_, prism)) = tower_type(self, rules, id) else {
            return false;
        };
        prism.can_attack()
            && prism.break_support()
            && self.substrate.entities.get(id).is_some_and(|entity| {
                matches!(
                    entity
                        .building
                        .pending_building_fire
                        .pending()
                        .map(|pending| pending.fire()),
                    Some(DelayedFire::SupportBeam { .. })
                )
            })
    }

    /// `BuildingClass_ChangeOwner_PrismForwardAndLeaveBomb @ 100163B0` (hook
    /// `0x448277`), its Prism part: a `PrismForwarding.ToAllies` tower that
    /// feeds nobody, or whose new owner is allied with both the tower it
    /// feeds and the firing tower at the end of that chain, stays in its
    /// network. Any other building leaves it without `cease`. Answers whether
    /// the hook went on to clear the building's registered jammers (#305).
    pub(crate) fn ares_prism_change_owner(
        &mut self,
        id: u64,
        new_owner: crate::sim::intern::InternedId,
        rules: &RuleSet,
    ) -> bool {
        if let Some((_, prism)) = tower_type(self, rules, id)
            && prism.to_allies()
        {
            let entities = &self.substrate.entities;
            let Some(first) = entities
                .get(id)
                .and_then(|entity| entity.ares_prism.support_target)
            else {
                return false;
            };
            let allied = |tower: u64| {
                entities.get(tower).is_some_and(|tower| {
                    combat_weapon::is_ally_by_object(
                        Some(&self.house_alliances),
                        &self.interner,
                        new_owner,
                        tower.owner(),
                    )
                })
            };
            if allied(first) {
                // The firing tower ends the chain. A cycle has no end; stop
                // after visiting every building once.
                let mut last = first;
                for _ in 0..entities.building_registry_len() {
                    match entities
                        .get(last)
                        .and_then(|entity| entity.ares_prism.support_target)
                    {
                        Some(next) => last = next,
                        None => break,
                    }
                }
                if allied(last) {
                    return false;
                }
            }
        }
        remove_from_network(&mut self.substrate.entities, id, false);
        true
    }
}

/// `BuildingClass_Mi_Attack_IsPrism @ 100167E0` (hook `0x44B2FE`), in
/// place of the stock PrismType arm: a `CanAttack` tower never reaches
/// FireAt or the IsAnimDelayedFire arm, and resumes at the OK tail
/// (`0x44B6D6`). An idle tower turns master: its delayed shot (`+0x714` =
/// DelayedFireDelay) with weapon 1 when its type has one and it is
/// overpowered (`+0x661`), else weapon 0, empty reserves, then the whole
/// network stage by stage until a stage recruits nobody, and the charge
/// delays. A supporter turns master in place: the weapon, empty reserves,
/// no target, the countdown kept. A master is left alone. Answers whether
/// the hook took the arm (any other building goes on to `0x44B630`).
pub(super) fn mission_attack_prism(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    object: &ObjectType,
) -> bool {
    if !object.building_type_ext().prism_forwarding().can_attack() {
        return false;
    }
    let Some(entity) = sim.substrate.entities.get(id) else {
        return true;
    };
    let slot = if object.weapon_at(1).is_some() && entity.building.tesla_charge.is_overpowered() {
        WeaponSlot::Secondary
    } else {
        WeaponSlot::Primary
    };
    match entity
        .building
        .pending_building_fire
        .pending()
        .map(|pending| pending.fire())
    {
        None => {
            let delay = delayed_fire_delay(rules, object);
            if let Some(entity) = sim.substrate.entities.get_mut(id) {
                entity
                    .building
                    .pending_building_fire
                    .arm(delay, DelayedFire::Weapon(slot));
                entity.ares_prism.modifier_reserve = 0;
                entity.ares_prism.damage_reserve = 0;
            }
            let mut longest_chain = 0;
            let mut network_size = 0;
            let mut stage = 0;
            while acquire_slaves_multi_stage(
                sim,
                rules,
                id,
                id,
                stage,
                0,
                &mut network_size,
                &mut longest_chain,
            ) != 0
            {
                stage += 1;
            }
            set_charge_delay(sim, rules, id, longest_chain);
        }
        Some(DelayedFire::SupportBeam { .. }) => {
            if let Some(entity) = sim.substrate.entities.get_mut(id) {
                if let Some(pending) = entity.building.pending_building_fire.pending.as_mut() {
                    pending.fire = DelayedFire::Weapon(slot);
                }
                entity.ares_prism.modifier_reserve = 0;
                entity.ares_prism.damage_reserve = 0;
            }
            set_support_target(&mut sim.substrate.entities, id, None);
        }
        Some(DelayedFire::Weapon(_)) => {}
    }
    true
}

/// `cPrismForwarding::AcquireSlaves_MultiStage @ 100185B0`: stage 0 recruits
/// for `target` itself one chain link further; a later stage recurses into
/// each tower already feeding it, rereading the list as it grows.
#[allow(clippy::too_many_arguments)]
fn acquire_slaves_multi_stage(
    sim: &mut Simulation,
    rules: &RuleSet,
    master: u64,
    target: u64,
    stage: i32,
    chain: i32,
    network_size: &mut i32,
    longest_chain: &mut i32,
) -> i32 {
    if stage == 0 {
        return acquire_slaves_single_stage(
            sim,
            rules,
            master,
            target,
            chain + 1,
            network_size,
            longest_chain,
        );
    }
    let mut count = 0i32;
    let mut index = 0;
    while let Some(sender) = sender_at(&sim.substrate.entities, target, index) {
        count = count.wrapping_add(acquire_slaves_multi_stage(
            sim,
            rules,
            master,
            sender,
            stage - 1,
            chain + 1,
            network_size,
            longest_chain,
        ));
        index += 1;
    }
    count
}

/// `cPrismForwarding::AcquireSlaves_SingleStage @ 10018630`: nothing when
/// `target`'s `MaxFeeds` is 0, `chain` passes the master type's
/// `MaxChainLength` or the network is full (`MaxNetworkSize`; negative
/// limits are none). Otherwise every building of BuildingClass::Array that
/// may feed `target` ([`validate_support_tower`]), nearest first (a stable
/// sort, so a tie keeps array order), is recruited while the feed and
/// network limits allow: `+0x714` = its type's DelayedFireDelay, stage
/// Slave aimed at `target`'s weapon-0 FLH, and `target` becomes its support
/// target. Recruiting any lengthens the longest chain to `chain`.
fn acquire_slaves_single_stage(
    sim: &mut Simulation,
    rules: &RuleSet,
    master: u64,
    target: u64,
    chain: i32,
    network_size: &mut i32,
    longest_chain: &mut i32,
) -> i32 {
    let support_max = rules.general.prism_support.max;
    let (Some((_, master_prism)), Some((_, target_prism))) = (
        tower_type(sim, rules, master),
        tower_type(sim, rules, target),
    ) else {
        return 0;
    };
    let max_feeds = target_prism.max_feeds(support_max);
    let max_network_size = master_prism.max_network_size(support_max);
    let max_chain_length = master_prism.max_chain_length();
    if max_feeds == 0
        || (max_chain_length >= 0 && max_chain_length < chain)
        || (max_network_size >= 0 && max_network_size <= *network_size)
    {
        return 0;
    }
    let Some(origin) = location(sim, target) else {
        return 0;
    };
    let entities = &sim.substrate.entities;
    let mut eligible: Vec<(u64, i32)> = (0..entities.building_registry_len())
        .filter_map(|index| entities.building_registry_at(index))
        .filter(|&slave| validate_support_tower(sim, rules, master, target, slave))
        .filter_map(|slave| Some((slave, distance(origin, location(sim, slave)?))))
        .collect();
    eligible.sort_by_key(|&(_, distance)| distance);
    let mut feeds = 0i32;
    for (slave, _) in eligible {
        if max_feeds >= 0 && feeds >= max_feeds {
            break;
        }
        if max_network_size >= 0 && *network_size >= max_network_size {
            break;
        }
        feeds += 1;
        *network_size = network_size.wrapping_add(1);
        let Some(to) = weapon0_flh(sim, rules, target) else {
            continue;
        };
        let delay = tower_type(sim, rules, slave)
            .map_or(0, |(object, _)| delayed_fire_delay(rules, object));
        if let Some(entity) = sim.substrate.entities.get_mut(slave) {
            entity
                .building
                .pending_building_fire
                .arm(delay, DelayedFire::SupportBeam { to });
        }
        set_support_target(&mut sim.substrate.entities, slave, Some(target));
    }
    if feeds != 0 && chain > *longest_chain {
        *longest_chain += 1;
    }
    feeds
}

/// `cPrismForwarding::ValidateSupportTower @ 10018F10`: whether `slave` may
/// feed `target` in `master`'s network. It must be alive, of a `CanForward`
/// type, its ReloadTimer (Techno+1FC) run out, not `target`, not counting
/// down (`+0x714 < 1`), not drained
/// or warped out, not on an Attack, Construction or Selling mission
/// (vt+0x184), powered (`TechnoExt::IsPowered 100454F0`), operated
/// (`IsOperated 10045440`) and online (vt+0x350), list `target`'s type in its
/// `PrismForwarding.Targets`, and share one House with both towers, or with
/// `ToAllies` be allied with both. Then `target` must lie in range: below the
/// support weapon's `MinimumRange` fails; within its `Range`, or when it
/// has none (or no support weapon) the primary's `Range` plus one cell,
/// inclusive; a negative range reaches everywhere. Both weapons are read
/// from the base array, elite or not.
fn validate_support_tower(
    sim: &Simulation,
    rules: &RuleSet,
    master: u64,
    target: u64,
    slave: u64,
) -> bool {
    let entities = &sim.substrate.entities;
    let Some(entity) = entities.get(slave) else {
        return false;
    };
    if !entity.lifecycle.object_alive() {
        return false;
    }
    let Some((object, prism)) = tower_type(sim, rules, slave) else {
        return false;
    };
    if !prism.can_forward() {
        return false;
    }
    let now = sim.session.binary_frame() as i32;
    let mission = entity.mission.effective();
    let busy = [
        MissionType::Attack,
        MissionType::Construction,
        MissionType::Selling,
    ]
    .into_iter()
    .any(|busy| mission == MissionId::from_known(busy));
    // `ReloadTimer.Expired()` is the Techno+1FC clock (a paused one passes),
    // not the weapon rearm timer the stock walk asks.
    let reload = entity.ammo_reload_timer();
    let reloading =
        reload.start_frame() != crate::sim::timer::PAUSED_START_FRAME && reload.remaining(now) != 0;
    if reloading
        || slave == target
        || entity.building.pending_building_fire.remaining_ticks() >= 1
        || entity.drain.draining_me().is_some()
        || entity.is_warped_out()
        || busy
        || !sim.ares_is_powered(slave, rules)
        || !sim.ares_is_operated(slave, rules)
        || sim.building_operational_state(slave, rules) != Some(true)
    {
        return false;
    }
    let (Some(target_entity), Some(master_entity)) = (entities.get(target), entities.get(master))
    else {
        return false;
    };
    let Some(target_object) = sim.interner.object_type(target_entity.type_ref(), rules) else {
        return false;
    };
    if !prism.targets(&target_object.id) {
        return false;
    }
    let (slave_house, target_house, master_house) =
        (entity.owner(), target_entity.owner(), master_entity.owner());
    let allied = |other| {
        combat_weapon::is_ally_by_object(
            Some(&sim.house_alliances),
            &sim.interner,
            slave_house,
            other,
        )
    };
    if !((slave_house == target_house && slave_house == master_house)
        || (prism.to_allies() && allied(target_house) && allied(master_house)))
    {
        return false;
    }
    let (Some(from), Some(to)) = (location(sim, target), location(sim, slave)) else {
        return false;
    };
    let distance = distance(from, to);
    let weapon = |index: i32| {
        usize::try_from(index)
            .ok()
            .and_then(|index| object.weapon_at(index))
            .and_then(|name| rules.weapon(name))
    };
    let mut support_range = 0;
    if let Some(support) = weapon(prism.support_weapon_index(is_elite(entity))) {
        if distance < support.minimum_range_leptons {
            return false;
        }
        support_range = support.range_leptons;
    }
    if support_range == 0
        && let Some(primary) = weapon(0)
    {
        support_range = primary.range_leptons.wrapping_add(256);
    }
    support_range < 0 || distance <= support_range
}

/// `cPrismForwarding::SetChargeDelay @ 10018CD0`: for each chain length
/// from the longest down, the longest charge delay behind every chain link
/// and the longest countdown on it ([`charge_delay_get`]), then each
/// tower's charge delay ([`charge_delay_set`]).
fn set_charge_delay(sim: &mut Simulation, rules: &RuleSet, master: u64, longest_chain: i32) {
    let len = usize::try_from(longest_chain.wrapping_add(1)).unwrap_or(0);
    let mut charge = vec![0i32; len];
    let mut fire = vec![0i32; len];
    for end in (0..=longest_chain).rev() {
        charge_delay_get(
            sim,
            rules,
            master,
            0,
            end,
            longest_chain,
            &mut charge,
            &mut fire,
        );
    }
    charge_delay_set(sim, rules, master, 0, &charge, &fire, longest_chain);
}

/// `cPrismForwarding::SetChargeDelay_Get @ 100178D0`: at link `end`, a tower
/// raises that link's charge delay to its type's `ChargeDelay` plus the next
/// link's (unless `end` is the last link) and its countdown to its own
/// `+0x714`; signed compares against the zeroed arrays.
#[allow(clippy::too_many_arguments)]
fn charge_delay_get(
    sim: &Simulation,
    rules: &RuleSet,
    tower: u64,
    chain: i32,
    end: i32,
    longest_chain: i32,
    charge: &mut [i32],
    fire: &mut [i32],
) {
    if chain != end {
        for sender in senders(&sim.substrate.entities, tower) {
            charge_delay_get(
                sim,
                rules,
                sender,
                chain + 1,
                end,
                longest_chain,
                charge,
                fire,
            );
        }
        return;
    }
    let link = chain as usize;
    if chain != longest_chain {
        let delay = tower_type(sim, rules, tower)
            .map_or(1, |(_, prism)| prism.charge_delay())
            .wrapping_add(charge[link + 1]);
        if charge[link] < delay {
            charge[link] = delay;
        }
    }
    let countdown = sim.substrate.entities.get(tower).map_or(0, |entity| {
        entity.building.pending_building_fire.remaining_ticks()
    });
    if fire[link] < countdown {
        fire[link] = countdown;
    }
}

/// `cPrismForwarding::SetChargeDelay_Set @ 10017970`: a tower at `chain`
/// waits `charge + fire - +0x714` of its link before charging, and its
/// SupportingPrisms (`+0x664`) becomes the chain behind it. One with no wait
/// starts its charge animation at once. A link past the arrays (native would
/// read beyond them) ends the walk.
fn charge_delay_set(
    sim: &mut Simulation,
    rules: &RuleSet,
    tower: u64,
    chain: i32,
    charge: &[i32],
    fire: &[i32],
    longest_chain: i32,
) {
    let link = usize::try_from(chain).ok();
    let (Some(&link_charge), Some(&link_fire)) = (
        link.and_then(|link| charge.get(link)),
        link.and_then(|link| fire.get(link)),
    ) else {
        return;
    };
    let Some(entity) = sim.substrate.entities.get_mut(tower) else {
        return;
    };
    let delay = link_charge
        .wrapping_add(link_fire)
        .wrapping_sub(entity.building.pending_building_fire.remaining_ticks());
    entity.ares_prism.charge_delay = delay;
    entity.building.prism_support_count.count = longest_chain.wrapping_sub(chain);
    if delay == 0 {
        start_charge_anim(sim, tower, rules);
    }
    for sender in senders(&sim.substrate.entities, tower) {
        charge_delay_set(sim, rules, sender, chain + 1, charge, fire, longest_chain);
    }
}

/// The charge animation of `100169A0` and `10017970`, only for a type with a
/// `SpecialAnim=` (BuildingType `+0x11F4`): the Active slot is emptied and
/// the SpecialAnim plays, its Damaged variant unless the building is green
/// (`IsGreenHP`), its garrisoned one with occupants (vt+0x408).
fn start_charge_anim(sim: &mut Simulation, id: u64, rules: &RuleSet) {
    if !sim.building_has_anim(id, SPECIAL_ANIM_SLOT, rules) {
        return;
    }
    sim.clear_building_anim_slot(id, ACTIVE_ANIM_SLOT);
    let Some((entity, object)) = sim
        .substrate
        .entities
        .get(id)
        .and_then(|entity| Some((entity, sim.interner.object_type(entity.type_ref(), rules)?)))
    else {
        return;
    };
    let damaged = requested_damage_state(
        entity.health,
        object.strength,
        rules.general.condition_yellow,
    );
    let garrisoned = !entity.passenger_role.occupants().is_empty();
    let _ = sim.set_building_anim_slot(id, SPECIAL_ANIM_SLOT, damaged, garrisoned, 0, rules);
}

/// `BuildingClass_Update_Prism @ 100169A0` (hook `0x4503F0`), in place of
/// the whole ProcessDelayedFire. A tower that is not idle first waits out
/// its charge delay, starting its charge animation when it ends; then its
/// countdown runs (`+0x714`, a signed pre-decrement) and below 1 it acts:
/// - a supporter with a target adds its type's support modifier and its own
///   reserve to the target's modifier reserve, and its `DamageAdd` and its
///   damage reserve to the target's damage reserve, then beams
///   ([`fire_laser`]);
/// - a master with a target fires its weapon when GetFireError(target,
///   weapon, range) answers OK; its bullet takes the reserves
///   ([`ObjectSubstrate::ares_prism_shot_multiplier`],
///   [`Simulation::ares_prism_shot_damage_add`]).
///
/// Either way the tower then goes idle: reserves cleared, every feeding
/// tower released, SupportingPrisms 0, no target.
pub(super) fn update_prism(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    ctx: ObjectAiCtx<'_>,
    bridge_state_changed: &mut bool,
) {
    let Some(entity) = sim.substrate.entities.get_mut(id) else {
        return;
    };
    if entity.building.pending_building_fire.pending().is_none() {
        return;
    }
    if entity.ares_prism.charge_delay >= 1 {
        entity.ares_prism.charge_delay -= 1;
        if entity.ares_prism.charge_delay < 1 {
            start_charge_anim(sim, id, rules);
        }
        return;
    }
    let Some(fire) = entity.building.pending_building_fire.count_down() else {
        return;
    };
    let support_target = entity.ares_prism.support_target;
    match fire {
        DelayedFire::SupportBeam { to } => {
            if let Some(target) = support_target
                && sim.substrate.entities.contains(target)
            {
                let (modifier, damage_add) =
                    tower_type(sim, rules, id).map_or((0, 0), |(_, prism)| {
                        (
                            prism.support_modifier(rules.general.prism_support.modifier),
                            prism.damage_add(),
                        )
                    });
                let entities = &mut sim.substrate.entities;
                let (own_modifier, own_damage) = entities.get(id).map_or((0, 0), |entity| {
                    (
                        entity.ares_prism.modifier_reserve,
                        entity.ares_prism.damage_reserve,
                    )
                });
                if let Some(target) = entities.get_mut(target) {
                    let reserve = &mut target.ares_prism;
                    reserve.modifier_reserve = i64::from(modifier)
                        .wrapping_add(own_modifier)
                        .wrapping_add(reserve.modifier_reserve);
                    reserve.damage_reserve = reserve
                        .damage_reserve
                        .wrapping_add(damage_add.wrapping_add(own_damage));
                }
                fire_laser(sim, rules, id, to);
            }
        }
        DelayedFire::Weapon(slot) => {
            let weapon = match slot {
                WeaponSlot::Primary => 0,
                WeaponSlot::Secondary => 1,
            };
            if let Some(target) = sim
                .substrate
                .entities
                .get(id)
                .and_then(|entity| entity.attack_target.target())
                && fire_error_at(
                    sim,
                    rules,
                    id,
                    Some(target),
                    weapon,
                    true,
                    ctx.overlay_registry,
                ) == FireError::Ok
            {
                *bridge_state_changed |= sim
                    .commit_fire_visit(
                        crate::sim::combat::world_fire::FireVisit::Building {
                            id,
                            shot: BuildingShot::Delayed { slot, target },
                        },
                        rules,
                        ctx.overlay_registry,
                        ctx.effects,
                    )
                    .bridge_state_changed;
            }
        }
    }
    let entities = &mut sim.substrate.entities;
    if let Some(entity) = entities.get_mut(id) {
        entity.ares_prism.modifier_reserve = 0;
        entity.ares_prism.damage_reserve = 0;
    }
    remove_all_senders(entities, id);
    if let Some(entity) = entities.get_mut(id) {
        entity.building.prism_support_count.count = 0;
    }
    set_support_target(entities, id, None);
}

/// `BuildingClass_FireLaser @ 10016490` (hook `0x44ABD0`): the support beam
/// from the tower's weapon-0 FLH to `to`. With a support weapon in the base
/// slot of its (elite) `SupportWeapon` index, an `IsLaser=` weapon draws its
/// own beam (its colours, or the House's for `IsHouseColor=`, and its
/// `LaserDuration=`), a `Report=` plays one item picked by a Scenario
/// `RandomRanged(0, count - 1)`, and ReloadTimer (Techno+1FC, not the weapon
/// rearm timer stock restarts) takes the weapon's `ROF=`; otherwise the stock
/// House-colour beam and `PrismSupportDelay=`.
/// A tower with SupportingPrisms thickens its beam by `-Intensity` (a
/// negative one) or `Intensity` per link (a positive one), supported above
/// 5; SupportingPrisms is then 0.
fn fire_laser(sim: &mut Simulation, rules: &RuleSet, id: u64, to: ProjectileCoord) {
    let Some((object, prism)) = tower_type(sim, rules, id) else {
        return;
    };
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    let owner = entity.owner();
    let supporting = entity.building.prism_support_count.count();
    let weapon = usize::try_from(prism.support_weapon_index(is_elite(entity)))
        .ok()
        .and_then(|index| object.weapon_at(index))
        .and_then(|name| rules.weapon(name));
    let Some(from) = crate::sim::combat::laser::support_source(sim, rules, id) else {
        return;
    };
    if weapon.is_none_or(|weapon| weapon.is_laser) {
        crate::sim::combat::laser::support_beam(
            sim,
            rules,
            owner,
            from,
            to,
            weapon,
            supporting,
            prism.intensity(),
        );
    }
    let rearm = match weapon {
        Some(weapon) => {
            let count = weapon.report_count();
            if count > 0 {
                let pick = sim.scenario_rng.next_range_i32_inclusive(0, count - 1);
                if let Some(report) = usize::try_from(pick)
                    .ok()
                    .and_then(|pick| weapon.report_item(pick))
                {
                    sim.sound_events
                        .emit(crate::sim::world::SimSoundEvent::voc_at_world_coord(
                            report.to_owned(),
                            crate::sim::anim_class::AnimWorldCoord {
                                x: from.x,
                                y: from.y,
                                z: from.z,
                            },
                        ));
                }
            }
            weapon.rof
        }
        None => rules.general.prism_support.delay,
    };
    let now = sim.session.binary_frame() as i32;
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity.start_prism_support_downtime(now, rearm);
        entity.building.prism_support_count.count = 0;
    }
}
