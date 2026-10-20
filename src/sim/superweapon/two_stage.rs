//! Saved, owner-scoped one-use selections for money-bearing two-click powers.
//! Free retail SPECIAL_PLACE retains its native, unticketed path. Ares's
//! ChronoSphere NoMoney/source-settings substitution defines settlement; the
//! sequence guard is VERA's deterministic command/replay safety, not a native ABI.

use crate::rules::ruleset::RuleSet;
use crate::rules::superweapon_type::SuperWeaponType;
use crate::sim::intern::InternedId;
use crate::sim::world::{SimSoundEvent, Simulation};
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};

impl super::SuperWeaponInstance {
    pub(crate) fn hash_paid_selection(&self, hasher: &mut impl Hasher) {
        if self.paid_sequence != 0 || self.paid_selection.is_some() {
            b"paid-super-selection-v1".hash(hasher);
            self.paid_sequence.hash(hasher);
            self.paid_selection.hash(hasher);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PaidSuperWeaponSelection {
    pub source: InternedId,
    pub destination: InternedId,
    pub sequence: u64,
}

pub(super) fn source_for_destination<'a>(
    sim: &Simulation,
    rules: &'a RuleSet,
    sw: &SuperWeaponType,
) -> Option<(InternedId, &'a SuperWeaponType)> {
    if !sw.post_click {
        return None;
    }
    let name = rules
        .super_weapon_order
        .get(usize::try_from(sw.pre_dependent).ok()?)?;
    Some((sim.interner.get(name)?, rules.super_weapon(name)?))
}

pub(super) fn destination_for_source(
    sim: &Simulation,
    rules: &RuleSet,
    source: InternedId,
    sw: &SuperWeaponType,
) -> Option<InternedId> {
    if !sw.pre_click || sw.post_click || sw.use_charge_drain {
        return None;
    }
    rules.super_weapon_order.iter().find_map(|name| {
        let destination = rules.super_weapon(name)?;
        let (pre, _) = source_for_destination(sim, rules, destination)?;
        (pre == source
            && !destination.pre_click
            && !destination.use_charge_drain
            && (sw.money_amount != 0 || destination.money_amount != 0))
            .then(|| sim.interner.get(name))
            .flatten()
    })
}

impl Simulation {
    pub(crate) fn validate_paid_super_weapon_selections(&self) -> Result<(), &'static str> {
        for (owner, weapons) in &self.super_weapons {
            let mut pending = false;
            for (source, instance) in weapons {
                let Some(ticket) = instance.paid_selection else {
                    continue;
                };
                if pending {
                    return Err("more than one pending selection for one House");
                }
                pending = true;
                if !self.houses.contains_key(owner)
                    || instance.owner != *owner
                    || instance.type_id != *source
                    || ticket.source != *source
                {
                    return Err("selection is not stored on its source and owner");
                }
                if ticket.sequence == 0 || ticket.sequence > instance.paid_sequence {
                    return Err("selection sequence is outside its saved source history");
                }
                if ticket.destination == *source || !weapons.contains_key(&ticket.destination) {
                    return Err("selection destination is not a distinct retained Super");
                }
            }
        }
        Ok(())
    }

    /// Read projection for command producers. No money or local cursor state is
    /// owned here. A completed/cancelled sequence is never reused after reload.
    pub fn paid_super_weapon_selection(
        &self,
        owner: InternedId,
    ) -> Option<PaidSuperWeaponSelection> {
        self.super_weapons
            .get(&owner)?
            .values()
            .find_map(|instance| instance.paid_selection)
    }

    pub fn next_paid_super_weapon_selection(
        &self,
        rules: &RuleSet,
        owner: InternedId,
        source: InternedId,
    ) -> Option<PaidSuperWeaponSelection> {
        let sw = rules.super_weapon(self.interner.resolve(source))?;
        Some(PaidSuperWeaponSelection {
            source,
            destination: destination_for_source(self, rules, source, sw)?,
            sequence: self
                .super_weapons
                .get(&owner)?
                .get(&source)?
                .paid_sequence
                .checked_add(1)?,
        })
    }

    /// Cancellation travels through Command like final activation. A stale
    /// cancellation cannot tear down a newer selection, even for the same SW.
    pub(crate) fn cancel_paid_super_weapon_selection(
        &mut self,
        _rules: &RuleSet,
        owner: InternedId,
        selection: PaidSuperWeaponSelection,
    ) -> bool {
        let Some(instance) = self
            .super_weapons
            .get_mut(&owner)
            .and_then(|weapons| weapons.get_mut(&selection.source))
        else {
            return false;
        };
        if instance.paid_selection != Some(selection) {
            return false;
        }
        instance.paid_selection = None;
        if instance.is_active && !instance.is_suspended {
            instance.is_ready = true;
        }
        self.release_super_anim(owner, selection.source);
        self.sound_events
            .push(SimSoundEvent::SuperWeaponStatusChanged {
                owner,
                sw_type: selection.destination,
            });
        // The recharge was not consumed by source selection. A later final
        // activation, rather than cancellation, owns restart/one-time loss.
        true
    }

    pub(super) fn retire_paid_selections(&mut self, rules: &RuleSet, owner: InternedId) {
        if let Some(selection) = self.paid_super_weapon_selection(owner) {
            self.cancel_paid_super_weapon_selection(rules, owner, selection);
        }
    }
}

pub(super) fn source_available(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    source: InternedId,
    sw: &SuperWeaponType,
) -> bool {
    let Some(house) = sim.houses.get(&owner) else {
        return false;
    };
    let Some(instance) = sim
        .super_weapons
        .get(&owner)
        .and_then(|supers| supers.get(&source))
    else {
        return false;
    };
    if house.is_defeated
        || !instance.is_active
        || instance.is_suspended
        || !super::building_requirements_met(sim, rules, owner, sw)
    {
        return false;
    }
    if instance.one_time || !instance.can_lose {
        return true;
    }
    let (provided, online) = super::provider_supers(sim, rules, owner);
    (sim.session.game_options.super_weapons || !sw.disableable_from_shell)
        && provided.contains(&source)
        && (!sw.is_powered
            || (online.contains(&source)
                && !sim
                    .power_states
                    .get(&owner)
                    .is_some_and(|power| power.low_power())))
}
