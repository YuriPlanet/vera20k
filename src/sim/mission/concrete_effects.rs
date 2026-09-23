//! Concrete Target/NavCom effects required by Mission wrapper transactions.
//!
//! The verified Techno/Foot wrappers archive intent around virtual setters, but
//! the complete category-specific setters are not implemented in Rust yet.
//! This sealed two-phase interface lets Mission authority prove availability
//! before its first write and then commit an infallible, ordered transaction.

use crate::sim::combat::TargetKind;
use crate::sim::components::NavTargetRef;
use crate::sim::world::Simulation;

mod private {
    pub trait Sealed {}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConcreteSetterRequest {
    Target {
        requested: Option<TargetKind>,
    },
    TargetAndDestination {
        requested_target: Option<TargetKind>,
        requested_destination: Option<NavTargetRef>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum AuthorityUnavailable {
    #[error("exact concrete Target setter is unavailable for Mission receiver {0}")]
    TargetSetter(u64),
    #[cfg(test)]
    #[error("exact mode-one destination setter is unavailable for Mission receiver {0}")]
    DestinationSetter(u64),
}

/// A complete concrete-effect provider.
///
/// `preflight` is read-only with respect to the simulation and validates the
/// entire requested setter chain.  Successful preflight guarantees the two
/// apply operations used by that request cannot fail.
pub(crate) trait ConcreteMissionEffects: private::Sealed {
    type Prepared;

    fn preflight(
        &mut self,
        sim: &Simulation,
        receiver: u64,
        request: ConcreteSetterRequest,
    ) -> Result<Self::Prepared, AuthorityUnavailable>;

    fn apply_target(
        &mut self,
        sim: &mut Simulation,
        prepared: &Self::Prepared,
        requested: Option<TargetKind>,
    );

    fn apply_destination_mode_one(
        &mut self,
        sim: &mut Simulation,
        prepared: &Self::Prepared,
        requested: Option<NavTargetRef>,
    );
}

/// Honest production boundary until full concrete Target and destination
/// setters are implemented.
#[cfg(test)]
#[derive(Debug, Default)]
pub(crate) struct UnavailableConcreteMissionEffects;

#[cfg(test)]
impl private::Sealed for UnavailableConcreteMissionEffects {}

#[cfg(test)]
impl ConcreteMissionEffects for UnavailableConcreteMissionEffects {
    type Prepared = ();

    fn preflight(
        &mut self,
        _sim: &Simulation,
        receiver: u64,
        request: ConcreteSetterRequest,
    ) -> Result<Self::Prepared, AuthorityUnavailable> {
        match request {
            ConcreteSetterRequest::Target { .. }
            | ConcreteSetterRequest::TargetAndDestination { .. } => {
                Err(AuthorityUnavailable::TargetSetter(receiver))
            }
        }
    }

    fn apply_target(
        &mut self,
        _sim: &mut Simulation,
        _prepared: &Self::Prepared,
        _requested: Option<TargetKind>,
    ) {
        unreachable!("unavailable provider cannot produce a concrete Target token")
    }

    fn apply_destination_mode_one(
        &mut self,
        _sim: &mut Simulation,
        _prepared: &Self::Prepared,
        _requested: Option<NavTargetRef>,
    ) {
        unreachable!("unavailable provider cannot produce a destination token")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RepresentedPrepared {
    receiver: u64,
}

#[derive(Debug, Default)]
pub(crate) struct RepresentedConcreteMissionEffects;

impl private::Sealed for RepresentedConcreteMissionEffects {}

impl ConcreteMissionEffects for RepresentedConcreteMissionEffects {
    type Prepared = RepresentedPrepared;

    fn preflight(
        &mut self,
        sim: &Simulation,
        receiver: u64,
        request: ConcreteSetterRequest,
    ) -> Result<Self::Prepared, AuthorityUnavailable> {
        if !sim.substrate.entities.contains(receiver) {
            return Err(AuthorityUnavailable::TargetSetter(receiver));
        }
        let _ = request;
        Ok(RepresentedPrepared { receiver })
    }

    fn apply_target(
        &mut self,
        sim: &mut Simulation,
        prepared: &Self::Prepared,
        requested: Option<TargetKind>,
    ) {
        let commits = assign_target_commits(&sim.substrate.entities, requested);
        let entity = sim
            .substrate
            .entities
            .get_mut(prepared.receiver)
            .expect("preflight guaranteed receiver");
        represented_assign_target_admitted(entity, requested, commits);
    }

    fn apply_destination_mode_one(
        &mut self,
        sim: &mut Simulation,
        prepared: &Self::Prepared,
        requested: Option<NavTargetRef>,
    ) {
        let entity = sim
            .substrate
            .entities
            .get_mut(prepared.receiver)
            .expect("preflight guaranteed receiver");
        represented_assign_destination_mode_one(entity, requested);
    }
}

/// Whether `TechnoClass::Assign_Target @ 0x006FCDB0` commits the requested
/// target (`0x006FCE4B..0x006FCF36`). A cell or NULL passes. An object commits
/// only while it is alive (`+0x90`) with nonzero Health (`+0x6C`); otherwise the
/// setter writes NULL, so a Restore or a retaliation that names a dying object
/// leaves its receiver without a target. Two further NULL arms have no VERA
/// producer: a Foot with `+0x3CD` set (sinking or crashing: `UnitClass::
/// ReceiveDamage 0x00737E51`, the Jumpjet crash `0x0054CEB7`, the squid grapple
/// `0x00629C69`, the Teleport water check `0x0071896B`/`0x00718AC2`), and an
/// Infantry in a death DoType (`0x00522CB0`), which VERA only enters at Health 0.
pub(crate) fn assign_target_commits(
    entities: &crate::sim::entity_store::EntityStore,
    requested: Option<TargetKind>,
) -> bool {
    match requested {
        Some(TargetKind::Entity(id)) => entities
            .get(id)
            .is_some_and(|target| target.lifecycle.object_alive && target.health.current != 0),
        Some(TargetKind::Cell(..)) | None => true,
    }
}

/// The represented `Assign_Target` write set for a NULL or cell target.
///
/// An object target needs [`assign_target_commits`], which reads the target;
/// use [`represented_assign_target_admitted`] for those.
pub(crate) fn represented_assign_target(
    entity: &mut crate::sim::game_entity::GameEntity,
    requested: Option<TargetKind>,
) {
    debug_assert!(
        !matches!(requested, Some(TargetKind::Entity(_))),
        "an object target needs assign_target_commits"
    );
    represented_assign_target_admitted(entity, requested, true);
}

/// The represented `Assign_Target` write set, entity-local; `commits` is
/// [`assign_target_commits`] for `requested`, read before the receiver was
/// borrowed.
///
/// The currently represented target/burst/Infantry-action writes and the
/// object-liveness refusal share this owner. Native Techno6FCDB0 also redirects
/// targets (a tank-bunkered object, self) and tears down linked effects;
/// Infantry51B1F0 has class-specific branches not all represented here. This is
/// not the whole native setter. The free function lets a bare `EntityStore`,
/// including movement, reach the same implementation as Mission transactions
/// without an open-coded copy.
pub(crate) fn represented_assign_target_admitted(
    entity: &mut crate::sim::game_entity::GameEntity,
    requested: Option<TargetKind>,
    commits: bool,
) {
    // The original's target assignment clears the passive-acquire flag as
    // its first statement, ahead of any same-target short-circuit, so a
    // target assigned by an order or retaliation cannot inherit the provenance
    // of one the scanner picked. The scanner
    // re-sets the flag itself after calling this.
    entity.passively_acquired_target = false;
    // The same-target early-out (`0x006FCDCC`, Infantry `0x0051B201`) compares
    // the requested pointer, before the liveness refusal.
    if entity.attack_target.as_ref().map(|target| target.target) == requested {
        return;
    }
    let requested = if commits { requested } else { None };

    // `InfantryClass::Assign_Target @ 0x0051B1F0` returns its receiver to an
    // idle sequence only while the receiver itself is alive (`0x0051B203`).
    if entity.category == crate::map::entities::EntityCategory::Infantry
        && entity.health.current > 0
    {
        entity.mission_leaf.set_infantry_firing_sequence(0);
        entity
            .mission_leaf
            .set_infantry_doing_verified(-1)
            .expect("idle Infantry action is always valid");
        if let Some(animation) = entity.animation.as_mut() {
            use crate::sim::animation::SequenceKind;

            let idle = match animation.sequence {
                SequenceKind::FireProne | SequenceKind::SecondaryProne => SequenceKind::Prone,
                SequenceKind::DeployedFire => SequenceKind::Deployed,
                SequenceKind::FireFly => SequenceKind::Fly,
                SequenceKind::WetAttack => SequenceKind::Tread,
                _ => SequenceKind::Stand,
            };
            if crate::sim::animation::sequence_is_fire_action(animation.sequence) {
                animation.switch_to(idle);
            }
        }
    }

    if requested.is_none() {
        entity.weapon_burst.clear_target();
    }
    entity.attack_target = requested.map(|target| match target {
        TargetKind::Entity(id) => crate::sim::combat::AttackTarget::new(id),
        TargetKind::Cell(rx, ry) => crate::sim::combat::AttackTarget::for_cell(rx, ry),
    });
}

/// The represented mode-one `Assign_Destination` write set, entity-local.
pub(crate) fn represented_assign_destination_mode_one(
    entity: &mut crate::sim::game_entity::GameEntity,
    requested: Option<NavTargetRef>,
) {
    entity.navigation.nav_com_aux = None;
    entity.navigation.nav_com = requested;
    entity.navigation.pending_arrival_clear = false;
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RecordingPrepared {
    receiver: u64,
    request: ConcreteSetterRequest,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConcreteEffectEvent {
    Preflight {
        receiver: u64,
        request: ConcreteSetterRequest,
    },
    Target {
        receiver: u64,
        requested: Option<TargetKind>,
        mission_current: super::MissionId,
        suspended_mission: super::MissionId,
        archived_target: Option<TargetKind>,
        archived_destination: Option<NavTargetRef>,
    },
    Destination {
        receiver: u64,
        requested: Option<NavTargetRef>,
        mission_current: super::MissionId,
        installed_target: Option<TargetKind>,
    },
}

#[cfg(test)]
#[derive(Debug)]
pub(crate) struct RecordingConcreteMissionEffects {
    pub allow_target: bool,
    pub allow_destination: bool,
    pub events: Vec<ConcreteEffectEvent>,
}

#[cfg(test)]
impl RecordingConcreteMissionEffects {
    pub(crate) fn available() -> Self {
        Self {
            allow_target: true,
            allow_destination: true,
            events: Vec::new(),
        }
    }
}

#[cfg(test)]
impl private::Sealed for RecordingConcreteMissionEffects {}

#[cfg(test)]
impl ConcreteMissionEffects for RecordingConcreteMissionEffects {
    type Prepared = RecordingPrepared;

    fn preflight(
        &mut self,
        _sim: &Simulation,
        receiver: u64,
        request: ConcreteSetterRequest,
    ) -> Result<Self::Prepared, AuthorityUnavailable> {
        self.events
            .push(ConcreteEffectEvent::Preflight { receiver, request });
        match request {
            ConcreteSetterRequest::Target { .. } if !self.allow_target => {
                return Err(AuthorityUnavailable::TargetSetter(receiver));
            }
            ConcreteSetterRequest::TargetAndDestination { .. } => {
                if !self.allow_target {
                    return Err(AuthorityUnavailable::TargetSetter(receiver));
                }
                if !self.allow_destination {
                    return Err(AuthorityUnavailable::DestinationSetter(receiver));
                }
            }
            ConcreteSetterRequest::Target { .. } => {}
        }
        Ok(RecordingPrepared { receiver, request })
    }

    fn apply_target(
        &mut self,
        sim: &mut Simulation,
        prepared: &Self::Prepared,
        requested: Option<TargetKind>,
    ) {
        debug_assert!(match prepared.request {
            ConcreteSetterRequest::Target {
                requested: prepared_target,
            } => prepared_target == requested,
            ConcreteSetterRequest::TargetAndDestination {
                requested_target, ..
            } => requested_target == requested,
        });
        let entity = sim
            .substrate
            .entities
            .get_mut(prepared.receiver)
            .expect("preflight guaranteed receiver");
        self.events.push(ConcreteEffectEvent::Target {
            receiver: prepared.receiver,
            requested,
            mission_current: entity.mission.current(),
            suspended_mission: entity.mission.suspended(),
            archived_target: entity.suspended_attack_target,
            archived_destination: entity.navigation.suspended_nav_com,
        });
        if entity.attack_target.as_ref().map(|target| target.target) != requested {
            entity.attack_target = requested.map(|target| match target {
                TargetKind::Entity(id) => crate::sim::combat::AttackTarget::new(id),
                TargetKind::Cell(rx, ry) => crate::sim::combat::AttackTarget::for_cell(rx, ry),
            });
        }
    }

    fn apply_destination_mode_one(
        &mut self,
        sim: &mut Simulation,
        prepared: &Self::Prepared,
        requested: Option<NavTargetRef>,
    ) {
        debug_assert!(matches!(
            prepared.request,
            ConcreteSetterRequest::TargetAndDestination {
                requested_destination,
                ..
            } if requested_destination == requested
        ));
        let entity = sim
            .substrate
            .entities
            .get_mut(prepared.receiver)
            .expect("preflight guaranteed receiver");
        self.events.push(ConcreteEffectEvent::Destination {
            receiver: prepared.receiver,
            requested,
            mission_current: entity.mission.current(),
            installed_target: entity.attack_target.as_ref().map(|target| target.target),
        });
        entity.navigation.nav_com_aux = None;
        entity.navigation.nav_com = requested;
        entity.navigation.pending_arrival_clear = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_concrete_effects_never_claim_partial_setter_coverage() {
        let sim = Simulation::new();
        let mut effects = UnavailableConcreteMissionEffects;

        assert_eq!(
            effects.preflight(&sim, 7, ConcreteSetterRequest::Target { requested: None }),
            Err(AuthorityUnavailable::TargetSetter(7))
        );
        assert_eq!(
            effects.preflight(
                &sim,
                7,
                ConcreteSetterRequest::TargetAndDestination {
                    requested_target: None,
                    requested_destination: None,
                }
            ),
            Err(AuthorityUnavailable::TargetSetter(7))
        );
    }
}
