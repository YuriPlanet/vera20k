//! Cargo-head departure owns admission-time size and the complete retry transition.
//!
//! Geometry and mission cadence remain with each production caller. No removed
//! entry escapes this operation: an unsuccessful attempt restores the same head
//! and recorded size before returning. Serialized CargoClass state is unchanged.
//! Native removal: CargoClass::RemoveFirstPassenger @ 0x00473430; retry:
//! AddPassenger @ 0x004733A0. Route-specific compatibility policies below preserve
//! the existing Rust branches; they do not claim uniform native retry semantics.

use crate::rules::ruleset::RuleSet;
use crate::sim::world::{
    FrameEffects, PlacementEvidence, RevealOutcome, RevealPosition, RevealRequest, Simulation,
    UninitContext,
};
use crate::util::lepton;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DepartureRoute {
    Vehicle,
    LandedAircraft,
    Paradrop,
    /// A dying Unit's passenger (`UnitClass::ReceiveDamage` `0x00737FD4`,
    /// `FootClass::RemoveFirstPassenger @ 0x004DE710`). Its attempt never
    /// fails: a refused passenger dies instead of re-boarding.
    DeathEscape,
}

/// The phase which refused departure, rather than a caller-owned rollback plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DepartureFailure {
    NoCargo,
    MissingPassenger,
    NotReady,
    Placement,
    GroundReveal(RevealOutcome),
    ParachuteAttach,
    ParachuteReveal(RevealOutcome),
}

/// Run one complete cargo-head attempt. Only the passenger ID is exposed to the
/// placement operation; admission-time size and head restoration stay here.
pub(crate) fn depart_cargo_head(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    transport_id: u64,
    route: DepartureRoute,
    attempt: impl FnOnce(&mut Simulation, u64) -> Result<(), DepartureFailure>,
    effects: FrameEffects<'_>,
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
    let removed = cargo.unload_first();
    // FUN_004DE710's empty-hold weapon reset occurs before placement for these
    // callers. Paradrop does not call RemoveGunner.
    let emptied = matches!(
        route,
        DepartureRoute::Vehicle | DepartureRoute::LandedAircraft | DepartureRoute::DeathEscape
    ) && cargo.is_empty();
    if emptied {
        // Foot4DE710 also calls the empty-hold reset when RemoveFirst returns
        // null. Keep that lifecycle effect before reporting NoCargo.
        super::remove_gunner(sim, rules, transport_id, removed.map(|(id, _)| id));
    }
    let (passenger_id, passenger_size) = removed.ok_or(DepartureFailure::NoCargo)?;
    // A failed Unlimbo re-attaches the passenger with `+0x82` untouched; only
    // a successful unload clears it (`0x0073DB98`).
    let open_topped = sim
        .substrate
        .entities
        .get(passenger_id)
        .is_some_and(|passenger| passenger.passenger_role.in_open_transport());
    let result = attempt(sim, passenger_id);
    if let Err(failure) = result.as_ref() {
        restore_departure(
            sim,
            rules,
            registry,
            transport_id,
            open_topped,
            passenger_id,
            passenger_size,
            route,
            *failure,
            effects,
        );
    }
    result
}

fn restore_departure(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    transport_id: u64,
    open_topped: bool,
    passenger_id: u64,
    passenger_size: u32,
    route: DepartureRoute,
    failure: DepartureFailure,
    effects: FrameEffects<'_>,
) {
    let reveal_outcome = match failure {
        DepartureFailure::GroundReveal(outcome) | DepartureFailure::ParachuteReveal(outcome) => {
            Some(outcome)
        }
        _ => None,
    };
    if reveal_outcome == Some(RevealOutcome::AlreadyRevealed) {
        // Defensive legacy repair of an already-broken cargo/limbo invariant.
        let _ = sim.techno_limbo_with_context(
            passenger_id,
            UninitContext::new(Some(rules), registry).with_effects(effects),
        );
    }
    match route {
        DepartureRoute::Paradrop => match failure {
            DepartureFailure::MissingPassenger => sim.clear_radio_contacts_for(passenger_id),
            DepartureFailure::NotReady | DepartureFailure::Placement => {}
            DepartureFailure::ParachuteAttach | DepartureFailure::ParachuteReveal(_) => {
                sim.clear_radio_contacts_for(passenger_id);
                if let Some(passenger) = sim.substrate.entities.get_mut(passenger_id) {
                    if matches!(failure, DepartureFailure::ParachuteReveal(_)) {
                        passenger.parachute_state = None;
                    }
                    passenger
                        .passenger_role
                        .set_inside_transport(transport_id, open_topped);
                }
            }
            DepartureFailure::NoCargo | DepartureFailure::GroundReveal(_) => {
                unreachable!("invalid paradrop departure phase")
            }
        },
        DepartureRoute::DeathEscape => {
            unreachable!("a dying transport's passenger never re-boards")
        }
        DepartureRoute::Vehicle | DepartureRoute::LandedAircraft => {
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
                passenger
                    .passenger_role
                    .set_inside_transport(transport_id, open_topped);
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
        super::receive_gunner(sim, rules, transport_id, passenger_id);
    }
}

/// Reveal a cargo passenger at an exit cell. The passenger's `sub_cell` and
/// `facing` must already be written by the caller; its role is cleared here.
/// The shared admitted Unlimbo owns barrel elevation, Techno6F6ED2 spatial
/// threat publication and Foot4D72F4's type coefficient copy before departure
/// queues its Move. Original reader/kernel and coefficient-copy execution:
/// tools/spatial_oracle/astar_threat_inputs.json and astar_mtnk_inputs.json.
pub(crate) fn reveal_unloaded_passenger(
    sim: &mut Simulation,
    rules: &RuleSet,
    transport_id: u64,
    passenger_id: u64,
    rx: u16,
    ry: u16,
    z: u8,
    effects: FrameEffects<'_>,
) -> Result<(), DepartureFailure> {
    let sub_cell = sim
        .substrate
        .entities
        .get(passenger_id)
        .and_then(|passenger| passenger.sub_cell);
    let (sub_x, sub_y) = lepton::subcell_lepton_offset(sub_cell);
    if let Some(passenger) = sim.substrate.entities.get_mut(passenger_id) {
        debug_assert_eq!(
            passenger.passenger_role.inside_transport_id(),
            Some(transport_id)
        );
        passenger.passenger_role.clear_carrier_relation();
    }
    let outcome = sim.try_reveal_entity_with_context(
        passenger_id,
        RevealRequest {
            position: RevealPosition {
                exact_z_leptons: None,
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
        UninitContext::with_rules(rules).with_effects(effects),
    );
    match outcome {
        RevealOutcome::Revealed { .. } => Ok(()),
        other => Err(DepartureFailure::GroundReveal(other)),
    }
}
