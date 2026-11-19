//! Phobos `Ammo.Shared=` / `Ammo.Shared.Group=`
//! (`Ext/TechnoType/Body.cpp:955-956`) and `TechnoExt::UpdateSharedAmmo`
//! (`Ext/Techno/Body.Update.cpp:825-853`).
//!
//! Phobos hooks the entry of `TechnoClass::StartReloading @ 0x006FB080`
//! (`0x006FB086`): when an `OpenTopped` transport with a finite `Ammo=` starts
//! its reload clock, it hands one round per call to each passenger whose type
//! sets `Ammo.Shared=yes`, whose `Ammo.Shared.Group=` matches the transport's
//! (a negative transport group matches every passenger) and whose own count is
//! below its type's `Ammo=`. The pass runs before the stock clock body, so a
//! hand-out that empties a full transport still starts its reload clock.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/.
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

use crate::rules::ruleset::RuleSet;
use crate::sim::entity_store::EntityStore;
use crate::sim::intern::StringInterner;

/// `TechnoClass::StartReloading @ 0x006FB080` in the world: the Phobos
/// `0x006FB086` hook's shared-ammo pass first, then the stock clock body with
/// the reduced count.
pub(crate) fn start_reloading(
    entities: &mut EntityStore,
    interner: &StringInterner,
    rules: &RuleSet,
    id: u64,
    now: i32,
) {
    #[cfg(feature = "phobos")]
    update_shared_ammo(entities, interner, rules, id);
    #[cfg(not(feature = "phobos"))]
    let _ = (interner, rules);
    let Some(object) = entities
        .get(id)
        .and_then(|entity| interner.object_type(entity.type_ref(), rules))
    else {
        return;
    };
    if let Some(entity) = entities.get_mut(id) {
        entity.start_reloading_ammo(object, now);
    }
}

/// `TechnoExt::UpdateSharedAmmo` (`Ext/Techno/Body.Update.cpp:825`): one round
/// per eligible passenger, in CargoClass list-head-first order.
#[cfg(feature = "phobos")]
fn update_shared_ammo(
    entities: &mut EntityStore,
    interner: &StringInterner,
    rules: &RuleSet,
    transport_id: u64,
) {
    let Some(transport) = entities.get(transport_id) else {
        return;
    };
    let Some(transport_type) = interner.object_type(transport.type_ref(), rules) else {
        return;
    };
    let (shared, open_topped, type_ammo, group) = (
        transport_type.ammo_shared,
        transport_type.open_topped,
        transport_type.ammo,
        transport_type.ammo_shared_group,
    );
    if !shared || !open_topped || type_ammo <= 0 {
        return;
    }
    let Some(passengers) = transport
        .passenger_role
        .cargo()
        .map(|cargo| cargo.passengers().to_vec())
    else {
        return;
    };
    for passenger_id in passengers {
        // Native re-reads `pThis->Ammo` before every passenger.
        let Some(current) = entities
            .get(transport_id)
            .and_then(|transport| transport.ammo())
            .map(|ammo| ammo.current())
        else {
            return;
        };
        if current <= 0 {
            break;
        }
        let Some(passenger) = entities.get(passenger_id) else {
            continue;
        };
        let Some(passenger_type) = interner.object_type(passenger.type_ref(), rules) else {
            continue;
        };
        let (passenger_shared, passenger_ammo, passenger_group) = (
            passenger_type.ammo_shared,
            passenger_type.ammo,
            passenger_type.ammo_shared_group,
        );
        if !passenger_shared || !(group < 0 || group == passenger_group) {
            continue;
        }
        if !passenger
            .ammo()
            .is_some_and(|ammo| ammo.current() < passenger_ammo)
        {
            continue;
        }
        if let Some(ammo) = entities
            .get_mut(transport_id)
            .and_then(|transport| transport.ammo_mut())
        {
            ammo.add(-1);
        }
        if let Some(ammo) = entities
            .get_mut(passenger_id)
            .and_then(|passenger| passenger.ammo_mut())
        {
            ammo.add(1);
        }
    }
}

#[cfg(all(test, feature = "phobos"))]
#[path = "shared_ammo_tests.rs"]
mod tests;
