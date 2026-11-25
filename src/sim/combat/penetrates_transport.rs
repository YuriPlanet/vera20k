//! Phobos `WarheadTypeExt::ApplyPenetratesTransport`
//! (`Ext/WarheadType/Detonate.cpp:780-930`), the passenger-damage arm of
//! `DetonateOnOneUnit` (`:233`).
//!
//! Called for every Techno target of the bullet detonation's per-target pass,
//! before the attach-effect and crit blocks. A warhead level above the
//! transport's level lets the shot reach its passengers.

use super::effects::DeathEffects;
use super::receiver_call::{EntityDamageEvent, ReceiverCallFlags};
use super::world_receiver::{ReceiverRun, commit_entities};
use crate::rules::ruleset::RuleSet;
use crate::rules::warhead_type::WarheadType;
use crate::sim::intern::InternedId;
use crate::sim::world::{SimSoundEvent, Simulation};

/// The `warhead_ref`/`source_id`/`source_house` arguments identify the
/// enclosing detonation; `damage` is the firing bullet's Health, which is the
/// zero Phobos passes from a pure `MapClass::DamageArea` (no bullet).
#[allow(clippy::too_many_arguments)]
pub(super) fn apply(
    world: &mut Simulation,
    run: &mut ReceiverRun<'_>,
    rules: &RuleSet,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    warhead: &WarheadType,
    warhead_ref: InternedId,
    source_id: u64,
    source_house: Option<InternedId>,
    target_id: u64,
    damage: i32,
    impact: crate::sim::projectile::ProjectileCoord,
    effects: &mut DeathEffects,
) {
    let settings = &warhead.phobos_penetrates_transport;
    if settings.level <= 0 || damage == 0 {
        return;
    }
    let Some(target) = world.substrate.entities.get(target_id) else {
        return;
    };
    let Some(target_type) = world.interner.object_type(target.type_ref(), rules) else {
        return;
    };
    let global_level = rules.phobos_globals.penetrates_transport_level;
    let target_rules = target_type.phobos_penetrates_transport;
    if settings.level <= target_rules.level(global_level) {
        return;
    }
    let Some(cargo) = target.passenger_role.cargo() else {
        return;
    };
    // `passengers.GetFirstPassenger()` walks the cargo list head first.
    let passengers: Vec<u64> = cargo.passengers().to_vec();
    if passengers.is_empty() {
        return;
    }
    // Each roll is drawn only when its rate can decide: a pass-through of 1
    // skips, and so does a fatal rate of 0 (`Detonate.cpp:794-800`).
    let pass_through = settings.pass_through * target_rules.pass_through_multiplier;
    if pass_through < 1.0 && world.scenario_rng.random_double() > pass_through {
        return;
    }
    let fatal_rate = settings.fatal_rate * target_rules.fatal_rate_multiplier;
    let fatal = fatal_rate > 0.0 && world.scenario_rng.random_double() <= fatal_rate;
    // `DamageAll=no` picks one passenger: `Random(0, NumPassengers - 1)`.
    let picked: Vec<u64> = if settings.damage_all {
        passengers
    } else {
        let last = passengers.len() as i32 - 1;
        let index = world.scenario_rng.next_range_i32_inclusive(0, last) as usize;
        vec![passengers[index]]
    };
    let adjusted = (f64::from(damage) * settings.damage_multiplier * target_rules.damage_multiplier)
        .ceil() as i32;
    // `Detonate.cpp:807`: `coords.DistanceFrom(transporterCoords)` when the
    // caller's distance is negative; the receiver's CellSpread falloff and
    // delay-kill interpolation read it.
    let transport_coord = crate::sim::movement::ground_pose::object_get_coords(
        target,
        world.resolved_terrain.as_ref(),
    );
    let distance = crate::util::native_x87::distance_3d_leptons(
        [impact.x, impact.y, impact.z],
        [transport_coord.x, transport_coord.y, transport_coord.z],
    );
    // `Gunner=yes` on a Unit target is the whole native gate besides `isFirst`.
    let gunner_transport =
        target_type.gunner && target.category() == crate::map::entities::EntityCategory::Unit;
    let mut gunner_removed = false;
    let mut pings = Vec::new();
    for (index, passenger_id) in picked.into_iter().enumerate() {
        let Some(passenger) = world.substrate.entities.get(passenger_id) else {
            continue;
        };
        let Some(passenger_type) = world.interner.object_type(passenger.type_ref(), rules) else {
            continue;
        };
        let passenger_rules = passenger_type.phobos_penetrates_transport;
        if settings.level <= passenger_rules.level(global_level) {
            continue;
        }
        let amount = if fatal {
            passenger_type.strength
        } else {
            adjusted
        };
        // `Detonate.cpp:813,845,865,916`: `passenger->SetLocation(
        // transporterCoords)` before ReceiveDamage, so a dying passenger's
        // death effects leave from the transport. FootClass::SetLocation
        // (`0x004DB780`) also carries the passenger's own riders.
        crate::sim::movement::ground_pose::foot_set_location(
            &mut world.substrate.entities,
            passenger_id,
            transport_coord,
            rules,
            &world.interner,
        );
        // `ReceiveDamage(..., ignoreDefenses = fatal, true, sourceHouse)`.
        let event = EntityDamageEvent::direct_receiver(
            passenger_id,
            amount,
            distance,
            source_id,
            source_house,
            warhead_ref,
            ReceiverCallFlags {
                ignore_defenses: fatal,
                arg6: true,
            },
        );
        let nested = commit_entities(
            world,
            run,
            std::slice::from_ref(&event),
            Some(false),
            rules,
            overlay_registry,
            &mut pings,
        );
        effects.append(nested);
        // `Detonate.cpp:887-903`: the first iterated passenger's `NowDead`
        // result calls the transport's `RemoveGunner` when its type has
        // `Gunner=yes`. The shared death path already ran the same arm for a
        // rider whose Transporter is set; this call is the native one and is
        // idempotent (mode 0).
        if index == 0
            && gunner_transport
            && world
                .substrate
                .entities
                .get(passenger_id)
                .is_some_and(|passenger| passenger.death.is_dying())
        {
            world.remove_gunner(rules, target_id, Some(passenger_id));
            gunner_removed = true;
        }
    }
    // `Detonate.cpp:907-913`: `passenger = passengers.GetFirstPassenger()`
    // after the loop; a surviving head takes the gunner slot back.
    if gunner_removed {
        let new_head = world
            .substrate
            .entities
            .get(target_id)
            .and_then(|target| target.passenger_role.cargo())
            .and_then(|cargo| cargo.passengers().first().copied());
        if let Some(pax_id) = new_head {
            crate::sim::passenger::receive_gunner(
                &world.interner,
                &world.session,
                &mut world.scenario_rng,
                &mut world.substrate,
                &world.houses,
                rules,
                target_id,
                pax_id,
            );
        }
    }
    effects.under_attack_events.append(&mut pings);
    // A clean pass that leaves no passenger plays `CleanSound` at the
    // transport's Location (`Detonate.cpp:908-920`).
    let remaining = world
        .substrate
        .entities
        .get(target_id)
        .and_then(|target| target.passenger_role.cargo())
        .map_or(0, |cargo| cargo.passengers().len());
    if remaining == 0
        && let Some(sound) = &settings.clean_sound
        && let Some(target) = world.substrate.entities.get(target_id)
    {
        let event =
            SimSoundEvent::voc_at_location(
                rules.sound_name(sound),
                target,
                world.resolved_terrain.as_ref(),
            );
        world.sound_events.emit(event);
    }
}
