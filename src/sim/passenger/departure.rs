//! Cargo-head departure owns admission-time size and the complete retry transition.
//!
//! Geometry and mission cadence remain with each production caller. No removed
//! entry escapes this operation: an unsuccessful attempt restores the same head
//! and recorded size before returning. Serialized CargoClass state is unchanged.
//! Native removal: CargoClass::RemoveFirstPassenger @ 0x00473430; retry:
//! AddPassenger @ 0x004733A0. Route-specific compatibility policies below preserve
//! the existing Rust branches; they do not claim uniform native retry semantics.

use super::PassengerRole;
use crate::rules::ruleset::RuleSet;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::world::{
    PlacementEvidence, RevealOutcome, RevealPosition, RevealRequest, Simulation,
};
use crate::util::lepton;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DepartureRoute {
    Garrison,
    Vehicle,
    LandedAircraft,
    Paradrop,
}

/// The phase which refused departure, rather than a caller-owned rollback plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DepartureFailure {
    NoCargo,
    MissingPassenger,
    NotReady,
    Placement,
    GroundReveal(RevealOutcome),
    ParachuteAttach(MovementLayer),
    ParachuteReveal(RevealOutcome, MovementLayer),
}

/// Run one complete cargo-head attempt. Only the passenger ID is exposed to the
/// placement operation; admission-time size and head restoration stay here.
pub(crate) fn depart_cargo_head(
    sim: &mut Simulation,
    rules: &RuleSet,
    transport_id: u64,
    route: DepartureRoute,
    attempt: impl FnOnce(&mut Simulation, u64) -> Result<(), DepartureFailure>,
) -> Result<(), DepartureFailure> {
    let transport = sim
        .substrate
        .entities
        .get_mut(transport_id)
        .ok_or(DepartureFailure::NoCargo)?;
    let cargo = transport
        .passenger_role
        .cargo_mut()
        .ok_or(DepartureFailure::NoCargo)?;
    let (passenger_id, passenger_size) = cargo.unload_first().ok_or(DepartureFailure::NoCargo)?;
    // FUN_004DE710's empty-hold weapon reset occurs before placement for these
    // two callers. Garrison resets only after successful scatter; paradrop does
    // not reset the carrier override at all.
    let emptied = matches!(
        route,
        DepartureRoute::Vehicle | DepartureRoute::LandedAircraft
    ) && cargo.is_empty();
    if emptied {
        transport.weapon_override = None;
        // The same pop's `+0x4D8` for a `Gunner=` type (`0x004DE72E..
        // 0x004DE742` -> `0x007464E0`): an IFV's TemporalClass returns to the
        // gunner, which lets its target go.
        let gunner = sim
            .substrate
            .entities
            .get(transport_id)
            .and_then(|transport| sim.object_type(transport.type_ref(), rules))
            .is_some_and(|object| object.gunner);
        if gunner {
            sim.temporal_remove_gunner(transport_id, passenger_id);
        }
    }
    let result = attempt(sim, passenger_id);
    if let Err(failure) = result.as_ref() {
        restore_departure(
            sim,
            rules,
            transport_id,
            passenger_id,
            passenger_size,
            route,
            *failure,
        );
    }
    result
}

fn restore_departure(
    sim: &mut Simulation,
    rules: &RuleSet,
    transport_id: u64,
    passenger_id: u64,
    passenger_size: u32,
    route: DepartureRoute,
    failure: DepartureFailure,
) {
    let reveal_outcome = match failure {
        DepartureFailure::GroundReveal(outcome) | DepartureFailure::ParachuteReveal(outcome, _) => {
            Some(outcome)
        }
        _ => None,
    };
    if reveal_outcome == Some(RevealOutcome::AlreadyRevealed) {
        // Defensive legacy repair of an already-broken cargo/limbo invariant.
        let _ = sim.techno_limbo_with_rules(passenger_id, rules);
    }
    match route {
        DepartureRoute::Paradrop => match failure {
            DepartureFailure::MissingPassenger => sim.clear_radio_contacts_for(passenger_id),
            DepartureFailure::NotReady | DepartureFailure::Placement => {}
            DepartureFailure::ParachuteAttach(prior_layer)
            | DepartureFailure::ParachuteReveal(_, prior_layer) => {
                sim.clear_radio_contacts_for(passenger_id);
                if let Some(passenger) = sim.substrate.entities.get_mut(passenger_id) {
                    if matches!(failure, DepartureFailure::ParachuteReveal(..)) {
                        passenger.parachute_state = None;
                    }
                    passenger.passenger_role = PassengerRole::Inside { transport_id };
                    if let Some(locomotor) = passenger.locomotor.as_mut() {
                        locomotor.layer = prior_layer;
                    }
                }
            }
            DepartureFailure::NoCargo | DepartureFailure::GroundReveal(_) => {
                unreachable!("invalid paradrop departure phase")
            }
        },
        DepartureRoute::Garrison | DepartureRoute::Vehicle | DepartureRoute::LandedAircraft => {
            assert!(
                matches!(
                    failure,
                    DepartureFailure::MissingPassenger
                        | DepartureFailure::Placement
                        | DepartureFailure::GroundReveal(_)
                ),
                "invalid ground departure phase"
            );
            if let Some(passenger) = sim.substrate.entities.get_mut(passenger_id) {
                passenger.passenger_role = PassengerRole::Inside { transport_id };
            }
        }
    }
    if let Some(cargo) = sim
        .substrate
        .entities
        .get_mut(transport_id)
        .and_then(|transport| transport.passenger_role.cargo_mut())
    {
        cargo.restore_front(passenger_id, passenger_size);
    }
    // Vehicle failures always re-adopt the current passenger's IFVMode. The
    // landed-aircraft compatibility path did so only before Reveal; retain that
    // distinction instead of restoring a saved override or unifying failure tails.
    if route == DepartureRoute::Vehicle
        || (route == DepartureRoute::LandedAircraft
            && !matches!(failure, DepartureFailure::GroundReveal(_)))
    {
        reapply_gunner_weapon(sim, rules, transport_id, passenger_id);
    }
}

/// Reveal a cargo passenger at an exit cell. The passenger's `sub_cell` and
/// `facing` must already be written by the caller; its role is cleared here.
pub(crate) fn reveal_unloaded_passenger(
    sim: &mut Simulation,
    transport_id: u64,
    passenger_id: u64,
    rx: u16,
    ry: u16,
    z: u8,
) -> Result<(), DepartureFailure> {
    let sub_cell = sim
        .substrate
        .entities
        .get(passenger_id)
        .and_then(|passenger| passenger.sub_cell);
    let (sub_x, sub_y) = lepton::subcell_lepton_offset(sub_cell);
    if let Some(passenger) = sim.substrate.entities.get_mut(passenger_id) {
        debug_assert!(matches!(
            passenger.passenger_role,
            PassengerRole::Inside {
                transport_id: current_transport
            } if current_transport == transport_id
        ));
        passenger.passenger_role = PassengerRole::None;
    }
    let outcome = sim.try_reveal_entity(
        passenger_id,
        RevealRequest {
            position: RevealPosition {
                rx,
                ry,
                z,
                sub_x,
                sub_y,
            },
            // The caller already selected an unoccupied exit cell. The
            // still-blocked native admission oracle is not fabricated here.
            placement: PlacementEvidence::MarkSucceeded,
            logic_eligible: true,
        },
    );
    match outcome {
        RevealOutcome::Revealed { .. } => Ok(()),
        other => Err(DepartureFailure::GroundReveal(other)),
    }
}

/// Vehicle failure tail `0x0073DC71`..`0x0073DCA6`: AddPassenger precedes
/// UnitClass `+0x4D4` (`0x00746420`), which re-adopts the passenger's attachment
/// and applies InfantryType+0x688 (IFVMode) through `FUN_0070DC70`. It reverses
/// the empty-pop `+0x4D8` (`0x007464E0`). Aircraft/Techno bind those slots to
/// stubs `0x004DE750`/`0x004DE760`; preserve the represented Rust Gunner gate.
/// UnitClass `+0x4D4` (`0x00746420`) for a `Gunner=yes` transport: the
/// re-added head passenger's `IFVMode=` becomes the transport's weapon slot
/// again. VERA's representation of that swap is `weapon_override`.
fn reapply_gunner_weapon(sim: &mut Simulation, rules: &RuleSet, transport_id: u64, pax_id: u64) {
    let Some(transport) = sim.substrate.entities.get(transport_id) else {
        return;
    };
    if !sim
        .object_type(transport.type_ref(), rules)
        .is_some_and(|obj| obj.gunner)
    {
        return;
    }
    let Some(passenger) = sim.substrate.entities.get(pax_id) else {
        return;
    };
    let ifv_mode = sim
        .object_type(passenger.type_ref(), rules)
        .map_or(0, |obj| obj.ifv_mode);
    if let Some(transport) = sim.substrate.entities.get_mut(transport_id) {
        transport.weapon_override = Some(
            crate::sim::combat::combat_weapon::WeaponOverride::IfvSlot(ifv_mode),
        );
    }
    sim.temporal_receive_gunner(transport_id, pax_id);
}
