//! Phobos `TechnoExt::EatPassengers` (`Ext/Techno/Body.Update.cpp:261-448`),
//! called from `TechnoExt::OnEarlyUpdate` after the AttachEffect update and
//! before `CheckDeathConditions`.
//!
//! A transport whose type configures `PassengerDeletion.Rate=` (or a
//! cost-derived rate) picks the last eligible passenger in CargoClass order,
//! runs a countdown, then deletes that passenger: the native body unlinks it,
//! plays the report sound and a random anim, optionally refunds Soylent money,
//! re-picks a gunner and finally kills the passenger's own cargo and uninitializes
//! it.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/ and other sim owners only.
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

use crate::rules::phobos_passenger_deletion::PassengerDeletionSettings;
use crate::rules::ruleset::RuleSet;
use crate::sim::flying_text::BountyText;
use crate::sim::intern::InternedId;
use crate::sim::world::{FrameEffects, Simulation};

/// `TechnoExt::EatPassengers` for `id` at the current frame.
pub(crate) fn eat_passengers(
    sim: &mut Simulation,
    rules: &RuleSet,
    id: u64,
    effects: FrameEffects<'_>,
) {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    let Some(object) = sim.interner.object_type(entity.type_ref(), rules) else {
        return;
    };
    let Some(settings) = object.phobos_passenger_deletion.as_ref() else {
        return;
    };
    let settings = settings.clone();
    let frame = sim.session.binary_frame();
    // `Body.Update.cpp:271-277`: limbo always pauses, and so do deactivation
    // and EMP unless UnderEMP says otherwise. A running clock's start moves
    // forward one frame per visit.
    let paused = entity.lifecycle.in_limbo()
        || (!settings.under_emp && (entity.deactivated.is_offline() || entity.emp_remaining() > 0));
    if paused {
        if let Some(entity) = sim.substrate.entities.get_mut(id) {
            let timer = entity.passenger_deletion_timer();
            if timer.remaining(frame as i32) > 0
                && timer.start_frame() != crate::sim::timer::PAUSED_START_FRAME
            {
                entity
                    .passenger_deletion_timer_mut()
                    .start(timer.start_frame().wrapping_add(1), timer.duration());
            }
        }
        return;
    }
    if !(settings.rate > 0 || settings.use_cost_as_rate) {
        if let Some(entity) = sim.substrate.entities.get_mut(id) {
            entity
                .passenger_deletion_timer_mut()
                .start(crate::sim::timer::PAUSED_START_FRAME, 0);
        }
        return;
    }
    let Some(cargo) = sim
        .substrate
        .entities
        .get(id)
        .and_then(|entity| entity.passenger_role.cargo())
    else {
        return;
    };
    let passengers = cargo.passengers().to_vec();
    if passengers.is_empty() {
        stop_timer(sim, id);
        return;
    }
    let owner = sim.substrate.entities.get(id).map(|entity| entity.owner());
    let Some(owner) = owner else {
        return;
    };
    // `Body.Update.cpp:284-301`: the loop keeps the LAST eligible passenger and
    // the one immediately before it in the list.
    let mut poor_guy = None;
    for &passenger_id in &passengers {
        let eligible = sim
            .substrate
            .entities
            .get(passenger_id)
            .is_some_and(|passenger| {
                crate::sim::combat::can_target_house(
                    settings.allowed_houses,
                    owner,
                    passenger.owner(),
                    &sim.houses,
                    sim.house_alliances(),
                    &sim.interner,
                )
            });
        if eligible {
            poor_guy = Some(passenger_id);
        }
    }
    let Some(poor_guy) = poor_guy else {
        // `Body.Update.cpp:303-307`: nothing eligible stops the clock.
        stop_timer(sim, id);
        return;
    };
    let timer = sim
        .substrate
        .entities
        .get(id)
        .map(|entity| entity.passenger_deletion_timer())
        .unwrap_or_default();
    let started = timer.start_frame() != crate::sim::timer::PAUSED_START_FRAME;
    if !started {
        // `Body.Update.cpp:309-334`: cost-as-rate or the explicit rate,
        // optionally scaled by the passenger's Size, capped when configured.
        let duration = deletion_duration(sim, rules, &settings, poor_guy);
        if let Some(entity) = sim.substrate.entities.get_mut(id) {
            entity
                .passenger_deletion_timer_mut()
                .start(frame as i32, duration);
        }
        return;
    }
    if timer.remaining(frame as i32) != 0 {
        return;
    }
    delete_passenger(sim, rules, id, owner, &settings, poor_guy, effects);
    stop_timer(sim, id);
}

fn stop_timer(sim: &mut Simulation, id: u64) {
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity
            .passenger_deletion_timer_mut()
            .start(crate::sim::timer::PAUSED_START_FRAME, 0);
    }
}

/// The countdown length for `poor_guy` (`Body.Update.cpp:311-333`).
fn deletion_duration(
    sim: &Simulation,
    rules: &RuleSet,
    settings: &PassengerDeletionSettings,
    poor_guy: u64,
) -> i32 {
    use crate::util::native_x87::{MaskedX87Chop53 as X, NativeF64Bits};
    let Some(passenger) = sim.substrate.entities.get(poor_guy) else {
        return 0;
    };
    let Some(object) = sim.interner.object_type(passenger.type_ref(), rules) else {
        return 0;
    };
    if settings.use_cost_as_rate {
        let product = X::mul(
            X::load_i32(object.cost),
            X::load_f64(NativeF64Bits::from_bits(settings.cost_multiplier.to_bits())),
        );
        let length = X::ftol_i32_low_masked(product);
        return settings.cost_rate_cap.map_or(length, |cap| length.min(cap));
    }
    let mut length = settings.rate;
    let size = object.size;
    if settings.rate_size_multiply && size > 1.0 {
        let scaled = X::ftol_i32_low_masked(X::add(
            X::load_f64(NativeF64Bits::from_bits(size.to_bits())),
            X::load_f64(NativeF64Bits::from_bits(0.5_f64.to_bits())),
        ));
        length = length.wrapping_mul(scaled);
    }
    length
}

/// One completed deletion (`Body.Update.cpp:335-441`).
fn delete_passenger(
    sim: &mut Simulation,
    rules: &RuleSet,
    transport: u64,
    owner: InternedId,
    settings: &PassengerDeletionSettings,
    poor_guy: u64,
    effects: FrameEffects<'_>,
) {
    // The transport's list splice and `--NumPassengers` happen before every
    // effect below.
    if let Some(cargo) = sim
        .substrate
        .entities
        .get_mut(transport)
        .and_then(|entity| entity.passenger_role.cargo_mut())
    {
        cargo.disembark(poor_guy);
    }
    // Native uses `GetCoords` for the sound and `Location` for the anim; a
    // building's GetCoords is its foundation centre while Location is not.
    let coords = sim.substrate.entities.get(transport).map(|entity| {
        crate::sim::movement::ground_pose::object_get_coords(entity, sim.resolved_terrain.as_ref())
    });
    let location = sim.substrate.entities.get(transport).map(|entity| {
        let location = crate::sim::movement::ground_pose::object_location(
            entity,
            sim.resolved_terrain.as_ref(),
        );
        crate::sim::anim_class::AnimWorldCoord {
            x: location.x,
            y: location.y,
            z: location.z,
        }
    });
    if let (Some(coords), Some(location)) = (coords, location) {
        if let Some(sound_id) = settings.report_sound.clone() {
            let (rx, ry, sub_x, sub_y, _) = location.to_cell_sub_z();
            sim.sound_events
                .emit(crate::sim::world::SimSoundEvent::VocAt {
                    sound_id,
                    audible_to: None,
                    rx,
                    ry,
                    sub_x,
                    sub_y,
                    world_z_leptons: coords.z,
                });
        }
        // `AnimExt::CreateRandomAnim`: one Scenario draw picks the type, then
        // the anim is created at the transport's Location with its Owner.
        if !settings.anim.is_empty() {
            let pick = sim
                .scenario_rng
                .next_range_i32_inclusive(0, settings.anim.len() as i32 - 1)
                as usize;
            if let Some(name) = settings.anim.get(pick) {
                let type_id = sim.interner.intern(name);
                sim.spawn_owned_anim(
                    rules,
                    type_id,
                    location,
                    owner,
                    Some(transport),
                    Some(transport),
                );
            }
        }
    }
    // Soylent refunds (`Body.Update.cpp:345-380`): the passenger's own
    // `GetRefund(owner, true)`, each of its passengers, a parasite eating it
    // and its hijacker, each scaled by SoylentMultiplier.
    let soylent_allowed = settings.soylent
        && sim
            .substrate
            .entities
            .get(poor_guy)
            .is_some_and(|passenger| {
                crate::sim::combat::can_target_house(
                    settings.soylent_allowed_houses,
                    owner,
                    passenger.owner(),
                    &sim.houses,
                    sim.house_alliances(),
                    &sim.interner,
                )
            });
    if soylent_allowed {
        let mut money_to_give = 0_i32;
        money_to_give = money_to_give.wrapping_add(refund_amount(
            sim,
            rules,
            poor_guy,
            settings.soylent_multiplier,
        ));
        let nested = sim
            .substrate
            .entities
            .get(poor_guy)
            .and_then(|passenger| passenger.passenger_role.cargo())
            .map(|cargo| cargo.passengers().to_vec())
            .unwrap_or_default();
        for passenger in nested {
            money_to_give = money_to_give.wrapping_add(refund_amount(
                sim,
                rules,
                passenger,
                settings.soylent_multiplier,
            ));
        }
        // `pPoorGuy->ParasiteEatingMe`: the eater's refund, then suppression
        // 50 and `ExitUnit`.
        let eater = sim
            .substrate
            .entities
            .get(poor_guy)
            .and_then(|passenger| passenger.parasite.eater());
        if let Some(eater) = eater {
            money_to_give = money_to_give.wrapping_add(refund_amount(
                sim,
                rules,
                eater,
                settings.soylent_multiplier,
            ));
            suppress_and_detach_parasite(sim, rules, eater, effects);
        }
        let hijacker = sim
            .substrate
            .entities
            .get(poor_guy)
            .map(|passenger| passenger.capture_infantry_type_index.get())
            .unwrap_or(-1);
        if hijacker >= 0
            && let Some(hijacker_type) = rules.type_array_at(
                crate::rules::object_type::ObjectCategory::Infantry,
                hijacker,
            )
        {
            let passenger_owner = sim.substrate.entities.get(poor_guy).map(|p| p.owner());
            if let Some(passenger_owner) = passenger_owner {
                money_to_give = money_to_give.wrapping_add(scaled_refund(
                    sim,
                    rules,
                    hijacker_type,
                    passenger_owner,
                    settings.soylent_multiplier,
                ));
            }
        }
        if money_to_give > 0 {
            crate::sim::credit_income::add_credits(&mut sim.houses, owner, money_to_give);
            if settings.display_soylent {
                emit_soylent_text(sim, rules, transport, money_to_give, settings, effects);
            }
        }
    } else if let Some(eater) = sim
        .substrate
        .entities
        .get(poor_guy)
        .and_then(|passenger| passenger.parasite.eater())
    {
        suppress_and_detach_parasite(sim, rules, eater, effects);
    }
    // Gunner change (`Body.Update.cpp:391-409`): the deleted passenger leaves
    // the turret and the list tail takes its place.
    let gunner_transport = sim
        .substrate
        .entities
        .get(transport)
        .and_then(|entity| sim.interner.object_type(entity.type_ref(), rules))
        .is_some_and(|object| object.gunner);
    if gunner_transport {
        sim.remove_gunner(rules, transport, Some(poor_guy));
        let last = sim
            .substrate
            .entities
            .get(transport)
            .and_then(|entity| entity.passenger_role.cargo())
            .and_then(|cargo| cargo.passengers().last().copied());
        if let Some(last) = last {
            crate::sim::passenger::receive_gunner(
                &sim.interner,
                &sim.session,
                &mut sim.scenario_rng,
                &mut sim.substrate,
                &sim.houses,
                rules,
                transport,
                last,
            );
        }
    }
    let attacker = (!settings.dont_score).then_some(transport);
    let context = crate::sim::world::UninitContext::new(rules).with_effects(effects);
    sim.kill_passengers(poor_guy, attacker, context);
    sim.record_kill_and_uninit(poor_guy, attacker, context);
    // `Body.Update.cpp:422-427`: a building transport with an ExtraPower
    // addend marks its House for the next power reassessment.
    let building_extra_power = sim
        .substrate
        .entities
        .get(transport)
        .filter(|entity| entity.category() == crate::map::entities::EntityCategory::Structure)
        .and_then(|entity| sim.interner.object_type(entity.type_ref(), rules))
        .is_some_and(|object| object.extra_power.output() != 0 || object.extra_power.drain() != 0);
    if building_extra_power && let Some(power) = sim.power_states.get_mut(&owner) {
        power.invalidate(false);
    }
}

/// `GetRefund(owner, true)` for `id`'s type and owner, scaled by `multiplier`.
fn refund_amount(sim: &Simulation, rules: &RuleSet, id: u64, multiplier: f64) -> i32 {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return 0;
    };
    let Some(object) = sim.interner.object_type(entity.type_ref(), rules) else {
        return 0;
    };
    scaled_refund(sim, rules, object, entity.owner(), multiplier)
}

fn scaled_refund(
    sim: &Simulation,
    rules: &RuleSet,
    object: &crate::rules::object_type::ObjectType,
    owner: InternedId,
    multiplier: f64,
) -> i32 {
    use crate::util::native_x87::{MaskedX87Chop53 as X, NativeF64Bits};
    let (Some(house), Some(factors)) = (
        sim.houses.get(&owner),
        Simulation::house_cost_factors_for(&sim.houses, owner, object),
    ) else {
        return 0;
    };
    let refund = crate::sim::production::type_refund(
        rules,
        object,
        house,
        &factors,
        sim.session.rule_policy(),
        true,
    );
    X::ftol_i32_low_masked(X::mul(
        X::load_i32(refund),
        X::load_f64(NativeF64Bits::from_bits(multiplier.to_bits())),
    ))
}

fn suppress_and_detach_parasite(
    sim: &mut Simulation,
    rules: &RuleSet,
    eater: u64,
    effects: FrameEffects<'_>,
) {
    let frame = sim.session.binary_frame();
    if let Some(entity) = sim.substrate.entities.get_mut(eater) {
        entity.parasite.suppress_parasite(frame, 50);
    }
    sim.parasite_exit_unit(eater, rules, effects);
}

fn emit_soylent_text(
    sim: &mut Simulation,
    rules: &RuleSet,
    transport: u64,
    amount: i32,
    settings: &PassengerDeletionSettings,
    effects: FrameEffects<'_>,
) {
    let Some(entity) = sim.substrate.entities.get(transport) else {
        return;
    };
    let coord =
        crate::sim::movement::ground_pose::object_get_coords(entity, sim.resolved_terrain.as_ref());
    // The client applies `DisplaySoylentToHouses` and the transport's
    // visibility; the event carries both the filter and the pixel offset.
    effects.emit_bounty_text(
        sim,
        rules,
        BountyText {
            entity_id: Some(transport),
            amount,
            coord: crate::sim::projectile::ProjectileCoord::new(coord.x, coord.y, coord.z),
            z_offset: 0,
            display_to: Some(settings.display_soylent_to_houses),
            display_owner: None,
            pixel_offset: settings.display_soylent_offset,
        },
    );
}

#[cfg(all(test, tests_sim_p_s))]
#[path = "passenger_deletion_tests.rs"]
mod tests;
