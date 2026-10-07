//! Exact Mission authority and category-aware wrapper transactions.
//!
//! LIVE since the authority flip: player commands queue through
//! [`Simulation::mission_queue_exact`] (the event-execute shape), and the
//! per-object AI host promotes queued missions through
//! [`Simulation::mission_host_promote`] (the per-category AI Ready→Commence
//! shape). The methods preserve native base transitions, Aircraft leaf policy,
//! synchronous Queue/Ready/Commence order, and the verified Target/NavCom
//! wrapper order.  Concrete setters use a two-phase provider so unavailable
//! effects cannot leave a partially-written transaction.
//!
//! Readiness inputs: the Unit world lookups (Radio contact slot 0, the
//! building-under stored-order lookup, `WeaponsFactory=`) are live through
//! [`LiveReadyInputProvider`]. The locomotor moving state is live for all six
//! families that can reach this gate, derived per gate evaluation by
//! [`crate::sim::movement::ready_producer`] — native makes a fresh locomotor
//! call at each of its own gate sites rather than caching one per frame. The
//! signed object height still has no live producer; the host promotion maps that
//! unavailability to a permissive moving-defer gate (recorded residual) rather
//! than stalling queued missions forever.

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::components::NavTargetRef;
use crate::sim::world::Simulation;

#[cfg(test)]
use super::concrete_effects::UnavailableConcreteMissionEffects;
use super::concrete_effects::{
    AuthorityUnavailable, ConcreteMissionEffects, ConcreteSetterRequest,
    RepresentedConcreteMissionEffects, represented_assign_destination_mode_one,
    represented_assign_target_admitted,
};
use super::readiness::{
    AircraftReadyView, BuildingReadyView, InfantryReadyView, ReadyLeptonPoint, ReadyResult,
    ReadyUnavailable, UnitReadyBuilding, UnitReadyContact, UnitReadyView, UnitReadyWorld,
    aircraft_ready_to_commence, building_ready_to_commence, infantry_ready_to_commence,
    unit_ready_to_commence,
};
use super::verb::{self, QueueContinuation};
use super::{MissionCom, MissionId};

mod ready_private {
    pub trait Sealed {}
}

/// Entity-local Restore transaction used after a represented pointer-expiry
/// callback has already cleared the expired target through Assign_Target.
///
/// The write set matches native Restore: selector first, then archived Target,
/// then mode-one destination. Callers must preserve the distinct expiry order
/// (clear first, then conditionally Restore); live-object detach uses a separate
/// wrapper and order. `saved_target_commits` is `assign_target_commits` for the
/// archived target, read before the receiver was borrowed.
pub(crate) fn restore_entity_after_target_expiry(
    entity: &mut crate::sim::game_entity::GameEntity,
    saved_target_commits: bool,
) -> bool {
    if entity.mission.suspended() == MissionId::NONE {
        return false;
    }

    let saved_target = entity.suspended_attack_target;
    let saved_destination = entity.navigation.suspended_nav_com;
    let category = entity.category;
    let restored = verb::restore_base(&mut entity.mission);
    debug_assert!(restored);
    represented_assign_target_admitted(entity, saved_target, saved_target_commits);
    if category != EntityCategory::Structure {
        represented_assign_destination_mode_one(entity, saved_destination);
        if saved_destination.is_some() {
            entity.navigation.pending_arrival_clear = true;
        }
    }
    true
}

/// Entity-local category wrapper for `Queue_Mission(requested, false)`.
///
/// Receiver special-damage branches already hold the target's mutable entity
/// and must make the queued selector visible before the next ordered receiver.
/// `commence=false` performs no readiness query or synchronous promotion.
pub(crate) fn queue_entity_mission_deferred(
    entity: &mut crate::sim::game_entity::GameEntity,
    requested: MissionId,
) -> bool {
    if !aircraft_allows(entity, requested) {
        return false;
    }
    verb::queue_base(&mut entity.mission, requested) == QueueContinuation::Continue
}

const AIRCRAFT_ACTION_EXCEPTION: MissionId = MissionId::from_raw(0x1e);
const MISSION_GUARD: MissionId = MissionId::from_raw(5);
/// Mission id 1, the `[Attack]` control entry. The selector every ground
/// locomotor overrides onto when an object it is not allied with stands in the
/// way of its next step.
const MISSION_ATTACK: MissionId = MissionId::from_raw(1);
const AIRCRAFT_PROTECTED: [MissionId; 5] = [
    MissionId::from_raw(4),
    MissionId::from_raw(0x1a),
    MissionId::from_raw(0x1b),
    MissionId::from_raw(0x1e),
    MissionId::from_raw(0x1f),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OverridePacket {
    mission: MissionId,
    combat_target: Option<TargetKind>,
    destination: Option<NavTargetRef>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum MissionAuthorityError {
    #[error("Mission receiver {0} does not exist")]
    MissingReceiver(u64),
    #[error(transparent)]
    Readiness(#[from] ReadyUnavailable),
    #[error(transparent)]
    AuthorityUnavailable(#[from] AuthorityUnavailable),
}

/// Supplies exact non-Mission inputs for Queue's synchronous readiness call.
///
/// Validation runs against a copied post-Queue Mission preview before the real
/// queue write.  The final readiness value is deliberately read again after
/// the real write and is never cached from validation. The trait is sealed:
/// successful validation guarantees that this fresh read is available, so an
/// integration error cannot be returned after Queue has mutated real state.
pub(crate) trait ReadyInputProvider: ready_private::Sealed {
    fn validate_ready_inputs(
        &self,
        sim: &Simulation,
        receiver: u64,
        preview: &MissionCom,
    ) -> Result<(), ReadyUnavailable>;

    fn ready_to_commence(&self, sim: &Simulation, receiver: u64, mission: &MissionCom) -> bool;
}

/// Production view over currently represented exact inputs, without rules.
///
/// Unit factory/contact lookups need parsed rules data, so this rules-free
/// provider reports them unavailable. Use [`LiveReadyInputProvider`] wherever
/// a `RuleSet` is in scope. Early native false branches still short-circuit
/// normally.
#[derive(Debug, Default)]
pub(crate) struct EntityReadyInputProvider;

/// Production view with the Unit world lookups live (rules in scope).
///
/// Retained as the rules-aware exact readiness provider while the current host
/// evaluates the same inputs directly.
#[derive(Debug)]
pub(crate) struct LiveReadyInputProvider<'r> {
    pub(crate) rules: &'r RuleSet,
}

struct UnavailableUnitWorld;

/// Live Unit-readiness world: Radio contact slot 0 identity and the
/// building-under lookup in the existing per-cell stored list order.
struct LiveUnitWorld<'a> {
    sim: &'a Simulation,
    rules: &'a RuleSet,
    entity: &'a crate::sim::game_entity::GameEntity,
}

impl LiveUnitWorld<'_> {
    fn weapons_factory(&self, entity: &crate::sim::game_entity::GameEntity) -> bool {
        self.rules
            .object(self.sim.interner.resolve(entity.type_ref()))
            .is_some_and(|obj| obj.weapons_factory)
    }
}

impl UnitReadyWorld for LiveUnitWorld<'_> {
    fn contact_slot_zero(&self) -> Result<Option<UnitReadyContact>, ReadyUnavailable> {
        let Some(contact_id) = self.entity.radio_contacts.slot(0) else {
            return Ok(None);
        };
        let Some(contact) = self.sim.substrate.entities.get(contact_id) else {
            // A live native slot always resolves; a stale id cannot be a
            // weapons-factory Building, so it classifies as a non-Building
            // contact rather than erroring the whole predicate.
            return Ok(Some(UnitReadyContact::Other));
        };
        if contact.category != EntityCategory::Structure {
            return Ok(Some(UnitReadyContact::Other));
        }
        Ok(Some(UnitReadyContact::Building {
            weapons_factory: self.weapons_factory(contact),
        }))
    }

    fn building_under_in_stored_order(
        &self,
        _unit_position: ReadyLeptonPoint,
    ) -> Result<Option<UnitReadyBuilding>, ReadyUnavailable> {
        let (rx, ry) = (self.entity.position.rx, self.entity.position.ry);
        let Some(cell) = self.sim.substrate.occupancy.get(rx, ry) else {
            return Ok(None);
        };
        // The per-cell occupant list already keeps the gamemd insertion order
        // (non-buildings prepend, buildings append); take the first Building
        // in that stored order — no sort, no rebuilt candidate list.
        for occupant in cell.iter_layer(crate::sim::movement::locomotor::MovementLayer::Ground) {
            let Some(entity) = self.sim.substrate.entities.get(occupant.entity_id) else {
                continue;
            };
            if entity.category != EntityCategory::Structure {
                continue;
            }
            // Anchor: the Building's footprint-NW cell as a lepton point whose
            // native lepton→cell conversion lands on that cell. UNCHECKED
            // whether the native building coordinate is NW-cell-anchored for
            // multi-cell foundations; the affected branch is the narrow
            // no-contact factory-cell hold.
            let anchor = ReadyLeptonPoint::new(
                i32::from(entity.position.rx)
                    .wrapping_mul(256)
                    .wrapping_add(128),
                i32::from(entity.position.ry)
                    .wrapping_mul(256)
                    .wrapping_add(128),
            );
            return Ok(Some(UnitReadyBuilding::new(
                self.weapons_factory(entity),
                anchor,
            )));
        }
        Ok(None)
    }
}

impl ready_private::Sealed for EntityReadyInputProvider {}
impl ready_private::Sealed for LiveReadyInputProvider<'_> {}

impl UnitReadyWorld for UnavailableUnitWorld {
    fn contact_slot_zero(&self) -> Result<Option<UnitReadyContact>, ReadyUnavailable> {
        Err(ReadyUnavailable::WorldLookup)
    }

    fn building_under_in_stored_order(
        &self,
        _unit_position: ReadyLeptonPoint,
    ) -> Result<Option<UnitReadyBuilding>, ReadyUnavailable> {
        Err(ReadyUnavailable::WorldLookup)
    }
}

/// Fallback moving-gate input for entities whose locomotor has no producer.
///
/// All six families that can actually reach this gate — Drive, Ship, Walk, Hover,
/// Teleport and Jumpjet — are produced live each tick by
/// `sim::movement::ready_producer`, so none of them land here any more. What
/// still lands here is Fly and Rocket, which that producer returns `None` for,
/// and an entity without a locomotor. (The dormant Tiberian Sun kinds resolve
/// to the constructor's Teleport.)
///
/// Those do not need a producer. `is_moving_now` has exactly two consumers here,
/// the Unit and Infantry branches in `sim::mission::readiness`; aircraft
/// readiness decides from its mission plus two flags and never reads the
/// locomotor, and no retail Unit or Infantry type runs Fly or Rocket. So this
/// is a floor for state the gate cannot reach, not a stand-in for missing work —
/// and answering "not moving" is also the safe direction if that ever changes.
/// It is not their native answer: Fly and Rocket have real `Is_Moving_Now`
/// bodies, which `motion_query::is_moving_now` answers.
///
/// This constant and the `degraded_moving_gate` parameter can retire together
/// once `evaluate_ready` no longer needs a `None` fallback at all.
const DEGRADED_NOT_MOVING: crate::sim::movement::locomotor_ready::LocomotorReadyState =
    crate::sim::movement::locomotor_ready::LocomotorReadyState::Drive {
        turning_active: false,
        slot_moving: false,
        head_to_nonnull: false,
        owner_speed: 0,
    };

fn evaluate_ready(
    sim: &Simulation,
    receiver: u64,
    mission: &MissionCom,
    rules: Option<&RuleSet>,
    degraded_moving_gate: bool,
) -> ReadyResult {
    {
        let entity = sim
            .substrate
            .entities
            .get(receiver)
            .ok_or(ReadyUnavailable::WorldLookup)?;
        // Derived here, at the gate, rather than read from a per-tick cache:
        // native's readiness virtual performs a fresh locomotor call every time
        // it runs, and it runs twice per object per tick — once either side of
        // that object's movement step — so a single stored value would answer
        // the second call with the first call's state.
        let locomotor = crate::sim::movement::ready_producer::ready_state_for(
            entity,
            rules.map(|rules| {
                crate::sim::movement::SpeedRules::new(
                    rules,
                    &sim.interner,
                    &sim.type_handles,
                    &sim.houses,
                )
            }),
            sim.session.binary_frame,
        )
        .or(if degraded_moving_gate {
            Some(DEGRADED_NOT_MOVING)
        } else {
            None
        });
        let attack_target_present = entity.attack_target.is_some();

        match entity.category {
            EntityCategory::Unit => {
                let leaf = entity
                    .mission_leaf
                    .as_unit()
                    .ok_or(ReadyUnavailable::WorldLookup)?;
                let position = ReadyLeptonPoint::new(
                    i32::from(entity.position.rx)
                        .wrapping_mul(256)
                        .wrapping_add(entity.position.sub_x.to_num::<i32>()),
                    i32::from(entity.position.ry)
                        .wrapping_mul(256)
                        .wrapping_add(entity.position.sub_y.to_num::<i32>()),
                );
                let unload_active = entity
                    .miner
                    .as_ref()
                    .is_some_and(|miner| miner.unload_active);
                match rules {
                    Some(rules) => unit_ready_to_commence(UnitReadyView {
                        mission,
                        leaf,
                        unload_active,
                        locomotor,
                        // Deliberately absent, and NOT for want of a producer.
                        //
                        // Native's input is `Get_Height`: the object's Z minus
                        // the ground height at its cell, minus the bridge deck
                        // when on a bridge — so ground and bridge both read 0,
                        // airborne reads positive, only below-ground reads
                        // negative. `LocomotorState::altitude` is exactly that
                        // quantity, so the producer is a one-liner.
                        //
                        // It is not wired up because turning it on stalls
                        // vehicles. Supplying it makes this branch's
                        // `signed_height >= 0` true, which arms the moving-defer
                        // — and measured over the global harness that took gate
                        // evaluations from 28 (all ready) to 8045, of which 8021
                        // were deferrals. The count explodes precisely because a
                        // deferral leaves the mission queued, so the same unit
                        // re-defers every tick and the queue never drains.
                        //
                        // Root cause is upstream of here: the dominant deferral
                        // reads `effective=Move, current=NONE, queued=Move,
                        // bypass_latch=0, moving=true` — a unit already moving
                        // with no commenced mission at all. Our command path
                        // sets `movement_target` directly AND queues the
                        // mission, so movement precedes commencement and the
                        // gate then refuses to commence because the unit is
                        // moving. Native runs it the other way: the order
                        // queues, the unit is not yet moving, the gate
                        // commences, and the mission handler starts the move.
                        // `set_movement_bypass_after_verified_queue` — native's
                        // escape hatch for exactly this — is called only by the
                        // refinery and jumpjet completion paths here, never by
                        // the general queue an ordinary Move uses.
                        //
                        // So this stays `None` until the command path stops
                        // starting movement ahead of commencement. Until then
                        // the host maps the resulting error to permissive-ready
                        // and the Unit moving-defer branch is inert — which is
                        // why none of the Drive/Ship readiness mapping can
                        // affect vehicle behaviour yet.
                        // MCV Event9 stops before queuing Unload, so this route
                        // can supply native GetHeight without the existing Move
                        // adapter's start-before-Commence dependency cycle.
                        signed_height: (mission.queued()
                            == MissionId::from_known(super::MissionType::Unload)
                            && crate::sim::mcv_deploy::is_mcv(sim, entity, rules))
                        .then(|| {
                            crate::sim::movement::air_movement::current_fly_height(
                                entity,
                                sim.resolved_terrain.as_ref(),
                            )
                        }),
                        attack_target_present,
                        position,
                        world: &LiveUnitWorld { sim, rules, entity },
                    }),
                    None => unit_ready_to_commence(UnitReadyView {
                        mission,
                        leaf,
                        unload_active,
                        locomotor,
                        signed_height: None,
                        attack_target_present,
                        position,
                        world: &UnavailableUnitWorld,
                    }),
                }
            }
            EntityCategory::Infantry => {
                let leaf = entity
                    .mission_leaf
                    .as_infantry()
                    .ok_or(ReadyUnavailable::WorldLookup)?;
                infantry_ready_to_commence(InfantryReadyView {
                    mission,
                    leaf,
                    object_is_falling_down: u8::from(entity.is_falling_down()),
                    locomotor,
                    attack_target_present,
                })
            }
            EntityCategory::Aircraft => {
                let leaf = entity
                    .mission_leaf
                    .as_aircraft()
                    .ok_or(ReadyUnavailable::WorldLookup)?;
                aircraft_ready_to_commence(AircraftReadyView { mission, leaf })
            }
            EntityCategory::Structure => {
                let leaf = entity
                    .mission_leaf
                    .as_building()
                    .ok_or(ReadyUnavailable::WorldLookup)?;
                building_ready_to_commence(BuildingReadyView { leaf })
            }
        }
    }
}

impl ReadyInputProvider for EntityReadyInputProvider {
    fn validate_ready_inputs(
        &self,
        sim: &Simulation,
        receiver: u64,
        preview: &MissionCom,
    ) -> Result<(), ReadyUnavailable> {
        evaluate_ready(sim, receiver, preview, None, false).map(|_| ())
    }

    fn ready_to_commence(&self, sim: &Simulation, receiver: u64, mission: &MissionCom) -> bool {
        evaluate_ready(sim, receiver, mission, None, false)
            .expect("successful readiness preflight must make the fresh read available")
    }
}

impl ReadyInputProvider for LiveReadyInputProvider<'_> {
    fn validate_ready_inputs(
        &self,
        sim: &Simulation,
        receiver: u64,
        preview: &MissionCom,
    ) -> Result<(), ReadyUnavailable> {
        evaluate_ready(sim, receiver, preview, Some(self.rules), false).map(|_| ())
    }

    fn ready_to_commence(&self, sim: &Simulation, receiver: u64, mission: &MissionCom) -> bool {
        evaluate_ready(sim, receiver, mission, Some(self.rules), false)
            .expect("successful readiness preflight must make the fresh read available")
    }
}

fn aircraft_allows(entity: &crate::sim::game_entity::GameEntity, requested: MissionId) -> bool {
    let Some(leaf) = entity.mission_leaf.as_aircraft() else {
        return true;
    };
    leaf.airstrike_manager_present()
        || !AIRCRAFT_PROTECTED.contains(&entity.mission.current())
        || AIRCRAFT_PROTECTED.contains(&requested)
}

pub(crate) fn commence_entity_mission(
    entity: &mut crate::sim::game_entity::GameEntity,
    now: u32,
) -> bool {
    if entity.mission_leaf.as_aircraft().is_some() {
        let old_current = entity.mission.current();
        if old_current != AIRCRAFT_ACTION_EXCEPTION {
            entity.mission_leaf.clear_aircraft_action_for_commence();
        }
    }
    verb::commence_base(&mut entity.mission, now)
}

/// The `RandomRanged(0, 2)` ceiling of the Rate epilogue.
pub(crate) const RATE_EPILOGUE_JITTER_MAX_FRAMES: u32 = 2;

impl Simulation {
    /// The handler return every `ftol(Rate × 900) + RandomRanged(0, 2)` exit
    /// shares — Mission_Harvest (`0x0073EF77`), Mission_Enter (`0x004D946C`),
    /// Mission_Unload (`0x0073E289`) and the Foot handlers: the base is the
    /// MissionControl slot of `mission` (`0x005B3A00` indexes the object's
    /// CURRENT mission, so a handler that commenced another mission passes
    /// that one), computed first with no RNG, then one draw on the Scenario
    /// stream (`0x0065C7E0` on `*(0x00A8B230)+0x218`). Native evidence:
    /// tools/spatial_oracle/refinery_dock.json (`delay` of the Enter, Harvest
    /// and Unload rows, one `[0, 2]` draw each).
    pub(crate) fn mission_rate_epilogue(
        &mut self,
        rules: &RuleSet,
        mission: super::MissionType,
    ) -> i32 {
        let base = rules.mission_control.rate_frames(mission);
        base.wrapping_add(
            self.scenario_rng
                .next_range_u32_inclusive(0, RATE_EPILOGUE_JITTER_MAX_FRAMES) as i32,
        )
    }

    /// [`Self::mission_rate_epilogue`] on `receiver`'s current mission as the
    /// handler leaves it (`dispatched` when the object is gone or holds none).
    pub(crate) fn mission_rate_epilogue_for(
        &mut self,
        rules: &RuleSet,
        receiver: u64,
        dispatched: super::MissionType,
    ) -> i32 {
        let current = self
            .substrate
            .entities
            .get(receiver)
            .and_then(|entity| entity.mission.current().known())
            .unwrap_or(dispatched);
        self.mission_rate_epilogue(rules, current)
    }

    pub(crate) fn mission_assign_exact(
        &mut self,
        receiver: u64,
        requested: MissionId,
        now: u32,
    ) -> Result<(), MissionAuthorityError> {
        let entity = self
            .substrate
            .entities
            .get_mut(receiver)
            .ok_or(MissionAuthorityError::MissingReceiver(receiver))?;
        if !aircraft_allows(entity, requested) {
            return Ok(());
        }
        verb::assign_base(&mut entity.mission, requested, now);
        Ok(())
    }

    pub(crate) fn mission_commence_exact(
        &mut self,
        receiver: u64,
        now: u32,
    ) -> Result<bool, MissionAuthorityError> {
        let entity = self
            .substrate
            .entities
            .get_mut(receiver)
            .ok_or(MissionAuthorityError::MissingReceiver(receiver))?;
        Ok(commence_entity_mission(entity, now))
    }

    /// Host-time queued-mission promotion at the per-object AI position.
    ///
    /// Native shape: each per-category AI update calls ReadyToCommence and, on
    /// true, Commence — Unit AI (`0x00736473`/`0x007366FD`), Infantry AI
    /// (`0x0051BC51`/`0x0051BF03`), Aircraft AI (`0x00415058`), Building
    /// Update (`0x0043FE43`/`0x0043FFA3`); verified via
    /// `decompile_function 0x007360c0` / `0x0051bab0` and the Queue/Commence
    /// active-caller census. Commence is a fieldwise no-op on an empty queue,
    /// so the Ready read is skipped there (pure, result unused).
    ///
    /// Readiness degradation (recorded residual): the exact locomotor family
    /// states and the signed object height have no live producers, so the
    /// native moving-defer branch cannot be evaluated; that unavailability is
    /// mapped to "promote" (the branch's pass outcome). Every other gate —
    /// excluded missions, deploy/unload latches, tracker bytes, the Radio
    /// slot-0 weapons-factory hold, the no-contact factory-cell hold, the
    /// Infantry firing/falling/Doing gates, the Aircraft and Building latches
    /// — evaluates exactly. Any other unavailable input blocks promotion.
    pub(crate) fn mission_host_promote(&mut self, receiver: u64, now: u32, rules: &RuleSet) {
        let queued = self
            .substrate
            .entities
            .get(receiver)
            .is_some_and(|entity| entity.mission.queued() != MissionId::NONE);
        if queued
            && self.mission_ready_to_commence(receiver, rules)
            && let Some(entity) = self.substrate.entities.get_mut(receiver)
        {
            commence_entity_mission(entity, now);
        }
    }

    /// `Ready_To_Commence` (vt+0x200) as a query, also for handlers that act
    /// between the answer and the Commence (Mission_Unload state 4 sends
    /// OVER_OUT first, `0x0073E264..0x0073E279`). Degraded moving gate:
    /// absent locomotor producers read as "not moving now" so every later
    /// exact gate still evaluates; a residual Locomotor/SignedHeight error (a
    /// live producer without a height owner) degrades to the branch's pass
    /// outcome.
    pub(crate) fn mission_ready_to_commence(&self, receiver: u64, rules: &RuleSet) -> bool {
        let Some(entity) = self.substrate.entities.get(receiver) else {
            return false;
        };
        match evaluate_ready(self, receiver, &entity.mission, Some(rules), true) {
            Ok(ready) => ready,
            Err(ReadyUnavailable::Locomotor | ReadyUnavailable::SignedHeight) => true,
            Err(_) => false,
        }
    }

    pub(crate) fn mission_queue_exact(
        &mut self,
        receiver: u64,
        requested: MissionId,
        commence_now: i32,
        now: u32,
        readiness: &impl ReadyInputProvider,
    ) -> Result<(), MissionAuthorityError> {
        let entity = self
            .substrate
            .entities
            .get(receiver)
            .ok_or(MissionAuthorityError::MissingReceiver(receiver))?;
        if !aircraft_allows(entity, requested) {
            return Ok(());
        }
        // Derived wrappers forward the full dword, but MissionClass::Queue
        // reads only its low byte to decide whether promotion is immediate.
        let commence_immediately = commence_now as u8 != 0;

        let mut preview = entity.mission;
        if verb::queue_base(&mut preview, requested) == QueueContinuation::OuterGuardBlocked {
            return Ok(());
        }
        if commence_immediately {
            readiness.validate_ready_inputs(self, receiver, &preview)?;
        }

        let entity = self
            .substrate
            .entities
            .get_mut(receiver)
            .expect("receiver was resolved before Queue mutation");
        let continuation = verb::queue_base(&mut entity.mission, requested);
        debug_assert_eq!(continuation, QueueContinuation::Continue);
        if !commence_immediately {
            return Ok(());
        }

        let ready = {
            let entity = self
                .substrate
                .entities
                .get(receiver)
                .expect("receiver remains present during synchronous Queue");
            readiness.ready_to_commence(self, receiver, &entity.mission)
        };
        if ready {
            let entity = self
                .substrate
                .entities
                .get_mut(receiver)
                .expect("receiver remains present during synchronous Commence");
            commence_entity_mission(entity, now);
        }
        Ok(())
    }

    #[cfg(test)]
    fn mission_override_exact(
        &mut self,
        receiver: u64,
        packet: OverridePacket,
    ) -> Result<bool, MissionAuthorityError> {
        let mut effects = UnavailableConcreteMissionEffects;
        self.mission_override_exact_with_effects(receiver, packet, &mut effects)
    }

    #[cfg(test)]
    pub(crate) fn mission_restore_exact(
        &mut self,
        receiver: u64,
    ) -> Result<bool, MissionAuthorityError> {
        let mut effects = UnavailableConcreteMissionEffects;
        self.mission_restore_exact_with_effects(receiver, &mut effects)
    }

    pub(crate) fn assign_target_represented(
        &mut self,
        receiver: u64,
        requested: Option<TargetKind>,
        rules: Option<&RuleSet>,
    ) -> Result<(), MissionAuthorityError> {
        if !self.substrate.entities.contains(receiver) {
            return Err(MissionAuthorityError::MissingReceiver(receiver));
        }
        let mut effects = RepresentedConcreteMissionEffects::new(rules, None);
        let prepared =
            effects.preflight(self, receiver, ConcreteSetterRequest::Target { requested })?;
        effects.apply_target(self, &prepared, requested);
        Ok(())
    }

    /// Dispatch the same concrete destination owner used by Override/Restore,
    /// without changing a mission or its Target. AreaGuard and Rescue require
    /// its synchronous Move_To/path/timer effects before their cadence tail.
    pub(crate) fn assign_destination_represented(
        &mut self,
        receiver: u64,
        requested: Option<NavTargetRef>,
        rules: Option<&RuleSet>,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    ) -> Result<(), MissionAuthorityError> {
        if !self.substrate.entities.contains(receiver) {
            return Err(MissionAuthorityError::MissingReceiver(receiver));
        }
        let mut effects = RepresentedConcreteMissionEffects::new(rules, overlay_registry);
        let prepared = effects.preflight(
            self,
            receiver,
            ConcreteSetterRequest::Destination { requested },
        )?;
        effects.apply_destination_mode_one(self, &prepared, requested);
        Ok(())
    }

    pub(crate) fn mission_restore_after_target_expiry(
        &mut self,
        receiver: u64,
        rules: Option<&RuleSet>,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    ) -> Result<bool, MissionAuthorityError> {
        let mut effects = RepresentedConcreteMissionEffects::new(rules, overlay_registry);
        self.mission_restore_exact_with_effects(receiver, &mut effects)
    }

    /// The Restore half of the detach sweep that releases every object shooting
    /// at an object which is leaving play *while still alive*.
    ///
    /// Same represented setters as the pointer-expiry Restore, and a separate
    /// name because the two native sites are not the same shape: the expiry
    /// site asks whether a mission is suspended and clears the target *before*
    /// restoring, while the detach sweep restores first, unconditionally, and
    /// clears the target afterwards only if the Restore did not install a
    /// different archived one. Restore is a total field-wise no-op on an object
    /// with no suspended selector, so "unconditional" and "guarded" agree on
    /// the write set; the difference that matters is the ordering the caller
    /// wraps around it.
    pub(crate) fn mission_restore_on_target_detach(
        &mut self,
        receiver: u64,
        rules: Option<&RuleSet>,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    ) -> Result<bool, MissionAuthorityError> {
        let mut effects = RepresentedConcreteMissionEffects::new(rules, overlay_registry);
        self.mission_restore_exact_with_effects(receiver, &mut effects)
    }

    /// `Override(Attack, attacker, NULL)` from the synchronous ReceiveDamage
    /// retaliation.
    ///
    /// `TechnoClass::ReceiveDamage @ 0x00701900` dispatches the concrete
    /// Mission wrapper before returning to its caller, so an ordered receiver
    /// makes the new mission visible to a later live-order combat slot in the
    /// same frame. Buildings archive/set only TarCom; Foot-derived objects
    /// archive NavCom first and then take the class setter's NULL destination
    /// (Foot::Override_Mission `0x004D8F6D`), which for a Drive/Ship Unit is
    /// Unit `0x00741970` ([`Self::assign_null_destination`]).
    pub(crate) fn override_mission_on_damage_response(
        &mut self,
        receiver: u64,
        attacker: u64,
        rules: &RuleSet,
    ) -> bool {
        if !self.substrate.entities.contains(attacker)
            || !self.substrate.entities.contains(receiver)
        {
            return false;
        }
        let mut effects = RepresentedConcreteMissionEffects::new(Some(rules), None);
        //7013CB dispatches the class target setter AFTER the mission writes.
        // A dying source or a refused Building target still takes the override;
        // target admission belongs to that live setter, not this caller.
        self.mission_override_exact_with_effects(
            receiver,
            OverridePacket {
                mission: MISSION_ATTACK,
                combat_target: Some(TargetKind::Entity(attacker)),
                destination: None,
            },
            &mut effects,
        )
        .expect("present represented retaliation receiver has concrete setters")
    }

    /// `Foot::Override_Mission(Attack, target, NULL)` (`0x004D8F40`) from the
    /// ground locomotor code-4/5 arm (Drive `0x004B3BE9`, Ship `0x006A3238`,
    /// Walk `0x0075BAEB`/`0x0075BB49`), after
    /// that arm asked the mover's House about an object target. NavCom and
    /// TarCom are archived, the mission overrides onto Attack and the target
    /// is assigned, then the class setter receives NULL. Ordinary Walk
    /// Infantry uses51AA40; the existing Unit path uses741970.
    pub(crate) fn mission_override_movement_blocker(
        &mut self,
        mover: u64,
        target: TargetKind,
        rules: &RuleSet,
    ) -> bool {
        if !self.substrate.entities.contains(mover) {
            return false;
        }
        let mut effects = RepresentedConcreteMissionEffects::new(Some(rules), None);
        // The locomotor caller owns its later head/Stop suffix. A refused
        // class destination must retain navigation through this transaction.
        self.mission_override_exact_with_effects(
            mover,
            OverridePacket {
                mission: MISSION_ATTACK,
                combat_target: Some(target),
                destination: None,
            },
            &mut effects,
        )
        .expect("present represented blocker receiver has concrete setters")
    }

    #[cfg(test)]
    pub(crate) fn mission_refinery_completion_exact(
        &mut self,
        receiver: u64,
        now: u32,
    ) -> Result<(), MissionAuthorityError> {
        self.mission_queue_exact(receiver, MISSION_GUARD, 0, now, &EntityReadyInputProvider)?;
        let entity = self
            .substrate
            .entities
            .get_mut(receiver)
            .expect("Queue resolved the refinery-completion receiver");
        entity.mission.set_movement_bypass_after_verified_queue();
        Ok(())
    }

    /// `JumpjetLocomotionClass::Move_To`'s lift tail for an unloading owner
    /// (`0x0054B479..0x0054B4B1`): `Queue_Mission(Guard, 1)`, the queued
    /// bypass latch `+0xB8`, then a second readiness gate and Commence.
    pub(crate) fn mission_jumpjet_move_to_completion_exact(
        &mut self,
        receiver: u64,
        now: u32,
        readiness: &impl ReadyInputProvider,
    ) -> Result<(), MissionAuthorityError> {
        self.validate_jumpjet_second_gate_previews(receiver, now, readiness)?;
        self.mission_queue_exact(receiver, MISSION_GUARD, 1, now, readiness)?;
        {
            let entity = self
                .substrate
                .entities
                .get_mut(receiver)
                .expect("Queue resolved the jumpjet-completion receiver");
            entity.mission.set_movement_bypass_after_verified_queue();
        }
        let ready = {
            let entity = self
                .substrate
                .entities
                .get(receiver)
                .expect("receiver remains present for the second Jumpjet gate");
            readiness.ready_to_commence(self, receiver, &entity.mission)
        };
        if ready {
            let entity = self
                .substrate
                .entities
                .get_mut(receiver)
                .expect("receiver remains present for the second Jumpjet Commence");
            commence_entity_mission(entity, now);
        }
        Ok(())
    }

    fn validate_jumpjet_second_gate_previews(
        &self,
        receiver: u64,
        now: u32,
        readiness: &impl ReadyInputProvider,
    ) -> Result<(), MissionAuthorityError> {
        let entity = self
            .substrate
            .entities
            .get(receiver)
            .ok_or(MissionAuthorityError::MissingReceiver(receiver))?;

        let mut post_queue = entity.mission;
        let queue_continues = aircraft_allows(entity, MISSION_GUARD)
            && verb::queue_base(&mut post_queue, MISSION_GUARD) == QueueContinuation::Continue;

        if !queue_continues {
            post_queue.set_movement_bypass_after_verified_queue();
            readiness.validate_ready_inputs(self, receiver, &post_queue)?;
            return Ok(());
        }

        let mut no_commence = post_queue;
        no_commence.set_movement_bypass_after_verified_queue();
        readiness.validate_ready_inputs(self, receiver, &no_commence)?;

        let mut successful_commence = post_queue;
        verb::commence_base(&mut successful_commence, now);
        successful_commence.set_movement_bypass_after_verified_queue();
        readiness.validate_ready_inputs(self, receiver, &successful_commence)?;
        Ok(())
    }

    /// A building's ready check in its Update (`0x0043FE27..0x0043FE54`
    /// before the Techno AI, which also needs BState out of 0, and
    /// `0x0043FF91..0x0043FFB4` after it): with `+0x6DD` set
    /// (Ready_To_Commence, building vt+0x200 = `0x00454250`), a successful
    /// Commence clears the byte. Commence fails with nothing queued and then
    /// leaves it set.
    pub(crate) fn mission_building_ready_commence(
        &mut self,
        receiver: u64,
        now: u32,
    ) -> Result<bool, MissionAuthorityError> {
        let ready = {
            let entity = self
                .substrate
                .entities
                .get(receiver)
                .ok_or(MissionAuthorityError::MissingReceiver(receiver))?;
            let leaf = entity
                .mission_leaf
                .as_building()
                .ok_or(ReadyUnavailable::WorldLookup)?;
            building_ready_to_commence(BuildingReadyView { leaf })?
        };
        if !ready {
            return Ok(false);
        }

        let entity = self
            .substrate
            .entities
            .get_mut(receiver)
            .expect("building-ready receiver remains present");
        if !commence_entity_mission(entity, now) {
            return Ok(false);
        }
        entity.mission_leaf.set_building_ready_latch(0);
        Ok(true)
    }

    /// Original Foot4D8F40 -> Techno7013A0 -> Mission5B3650, then the
    /// concrete Target7013CB and Destination4D8F6D. Refused setters retain
    /// their native partial effects; they do not undo the prior override.
    fn mission_override_exact_with_effects<E: ConcreteMissionEffects>(
        &mut self,
        receiver: u64,
        packet: OverridePacket,
        effects: &mut E,
    ) -> Result<bool, MissionAuthorityError> {
        let entity = self
            .substrate
            .entities
            .get(receiver)
            .ok_or(MissionAuthorityError::MissingReceiver(receiver))?;
        if !aircraft_allows(entity, packet.mission) {
            return Ok(false);
        }
        let request = match entity.category {
            EntityCategory::Structure => ConcreteSetterRequest::Target {
                requested: packet.combat_target,
            },
            EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft => {
                ConcreteSetterRequest::TargetAndDestination {
                    requested_target: packet.combat_target,
                    requested_destination: packet.destination,
                }
            }
        };
        let prepared = effects.preflight(self, receiver, request)?;

        {
            let entity = self
                .substrate
                .entities
                .get_mut(receiver)
                .expect("preflight cannot remove receiver");
            if entity.category != EntityCategory::Structure {
                entity.navigation.suspended_nav_com = entity.navigation.nav_com;
            }
            entity.suspended_attack_target =
                entity.attack_target.as_ref().map(|target| target.target);
            verb::override_base(&mut entity.mission, packet.mission);
        }
        effects.apply_target(self, &prepared, packet.combat_target);
        if !matches!(
            self.substrate
                .entities
                .get(receiver)
                .map(|entity| entity.category),
            Some(EntityCategory::Structure)
        ) {
            effects.apply_destination_mode_one(self, &prepared, packet.destination);
        }
        Ok(true)
    }

    fn mission_restore_exact_with_effects<E: ConcreteMissionEffects>(
        &mut self,
        receiver: u64,
        effects: &mut E,
    ) -> Result<bool, MissionAuthorityError> {
        let entity = self
            .substrate
            .entities
            .get(receiver)
            .ok_or(MissionAuthorityError::MissingReceiver(receiver))?;
        if entity.mission.suspended() == MissionId::NONE {
            return Ok(false);
        }
        let saved_target = entity.suspended_attack_target;
        let saved_destination = entity.navigation.suspended_nav_com;
        let category = entity.category;
        let request = match category {
            EntityCategory::Structure => ConcreteSetterRequest::Target {
                requested: saved_target,
            },
            EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft => {
                ConcreteSetterRequest::TargetAndDestination {
                    requested_target: saved_target,
                    requested_destination: saved_destination,
                }
            }
        };
        let prepared = effects.preflight(self, receiver, request)?;

        let restored = {
            let entity = self
                .substrate
                .entities
                .get_mut(receiver)
                .expect("preflight cannot remove receiver");
            verb::restore_base(&mut entity.mission)
        };
        debug_assert!(restored);
        effects.apply_target(self, &prepared, saved_target);
        if category != EntityCategory::Structure {
            let setter_ran = effects.apply_destination_mode_one(self, &prepared, saved_destination);
            if saved_destination.is_some()
                && !setter_ran
                && let Some(entity) = self.substrate.entities.get_mut(receiver)
            {
                // The original calls the class setter with the saved NavCom
                // (`FootClass::Restore_Mission` `0x004D8F99`: vt+0x480
                // (Foot+5A8, 1)); the stored path array is never archived,
                // only the destination is. A receiver without a represented
                // class setter owes that call; `complete_pending_order`
                // makes it at the object's next Process entry. Without it a
                // restored object holds its order and never moves toward it,
                // which is exactly the state a blocked-step Override leaves it
                // in.
                entity.navigation.pending_arrival_clear = true;
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;

    use super::super::MissionDispatchTimer;
    use super::super::concrete_effects::{ConcreteEffectEvent, RecordingConcreteMissionEffects};
    use super::super::leaf::MissionLeafState;
    use super::super::state::MissionTestFixture;
    use super::*;
    use crate::sim::combat::AttackTarget;
    use crate::sim::game_entity::GameEntity;

    const GUARD: MissionId = MissionId::from_raw(5);
    const MOVE: MissionId = MissionId::from_raw(2);
    const ATTACK: MissionId = MissionId::from_raw(1);

    struct TestReadyProvider {
        validation: Result<(), ReadyUnavailable>,
        ready: bool,
        validations: Cell<u32>,
        reads: Cell<u32>,
        missions: RefCell<Vec<MissionCom>>,
    }

    struct SequencedReadyProvider {
        values: RefCell<VecDeque<bool>>,
        validations: Cell<u32>,
        reads: Cell<u32>,
        missions: RefCell<Vec<MissionCom>>,
    }

    struct RejectSuccessfulJumpjetPreview {
        validations: Cell<u32>,
        reads: Cell<u32>,
        missions: RefCell<Vec<MissionCom>>,
    }

    impl SequencedReadyProvider {
        fn new(values: impl IntoIterator<Item = bool>) -> Self {
            Self {
                values: RefCell::new(values.into_iter().collect()),
                validations: Cell::new(0),
                reads: Cell::new(0),
                missions: RefCell::new(Vec::new()),
            }
        }
    }

    impl ready_private::Sealed for SequencedReadyProvider {}

    impl ReadyInputProvider for SequencedReadyProvider {
        fn validate_ready_inputs(
            &self,
            _sim: &Simulation,
            _receiver: u64,
            preview: &MissionCom,
        ) -> Result<(), ReadyUnavailable> {
            self.validations.set(self.validations.get() + 1);
            self.missions.borrow_mut().push(*preview);
            Ok(())
        }

        fn ready_to_commence(
            &self,
            _sim: &Simulation,
            _receiver: u64,
            _mission: &MissionCom,
        ) -> bool {
            self.reads.set(self.reads.get() + 1);
            self.values
                .borrow_mut()
                .pop_front()
                .expect("sequenced readiness value")
        }
    }

    impl ready_private::Sealed for RejectSuccessfulJumpjetPreview {}

    impl ReadyInputProvider for RejectSuccessfulJumpjetPreview {
        fn validate_ready_inputs(
            &self,
            _sim: &Simulation,
            _receiver: u64,
            preview: &MissionCom,
        ) -> Result<(), ReadyUnavailable> {
            self.validations.set(self.validations.get() + 1);
            self.missions.borrow_mut().push(*preview);
            if preview.movement_bypass_latch() != 0
                && preview.current() == GUARD
                && preview.queued() == MissionId::NONE
            {
                Err(ReadyUnavailable::WorldLookup)
            } else {
                Ok(())
            }
        }

        fn ready_to_commence(
            &self,
            _sim: &Simulation,
            _receiver: u64,
            _mission: &MissionCom,
        ) -> bool {
            self.reads.set(self.reads.get() + 1);
            false
        }
    }

    impl TestReadyProvider {
        fn ready(value: bool) -> Self {
            Self {
                validation: Ok(()),
                ready: value,
                validations: Cell::new(0),
                reads: Cell::new(0),
                missions: RefCell::new(Vec::new()),
            }
        }
    }

    impl ready_private::Sealed for TestReadyProvider {}

    impl ReadyInputProvider for TestReadyProvider {
        fn validate_ready_inputs(
            &self,
            _sim: &Simulation,
            _receiver: u64,
            preview: &MissionCom,
        ) -> Result<(), ReadyUnavailable> {
            self.validations.set(self.validations.get() + 1);
            self.missions.borrow_mut().push(*preview);
            self.validation
        }

        fn ready_to_commence(
            &self,
            _sim: &Simulation,
            _receiver: u64,
            mission: &MissionCom,
        ) -> bool {
            self.reads.set(self.reads.get() + 1);
            self.missions.borrow_mut().push(*mission);
            self.ready
        }
    }

    fn entity(category: EntityCategory, current: MissionId) -> GameEntity {
        let mut entity = GameEntity::test_default(1, "MTNK", "Americans", 5, 5);
        entity.category = category;
        entity.mission_leaf = MissionLeafState::for_entity_category(category);
        entity.mission.apply_test_fixture(MissionTestFixture {
            current,
            suspended: MissionId::NONE,
            queued: MissionId::NONE,
            movement_bypass_latch: 0xa5,
            handler_state: 0x1122_3344,
            mission_start_frame: 0x5566_7788,
            ai_counter: 0x99aa_bbcc,
            dispatch_timer: MissionDispatchTimer::from_raw(-17, -29),
        });
        entity
    }

    fn sim_with(entity: GameEntity) -> Simulation {
        let mut sim = Simulation::new();
        sim.substrate.entities.insert(entity);
        sim
    }

    #[test]
    fn mission_authority_assign_applies_aircraft_gate_before_writes() {
        let mut aircraft = entity(EntityCategory::Aircraft, MissionId::from_raw(4));
        aircraft.mission_leaf = MissionLeafState::aircraft_raw_for_test(9, 1, false);
        let before = aircraft.mission;
        let mut sim = sim_with(aircraft);

        sim.mission_assign_exact(1, MissionId::NONE, 10).unwrap();
        assert_eq!(sim.substrate.entities.get(1).unwrap().mission, before);

        sim.mission_assign_exact(1, MissionId::from_raw(0x1a), 10)
            .unwrap();
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().mission.current(),
            MissionId::from_raw(0x1a)
        );
    }

    #[test]
    fn mission_authority_assign_preserves_base_guard_and_raw_ids() {
        let mut sim = sim_with(entity(EntityCategory::Unit, MissionId::from_raw(28)));
        let before = sim.substrate.entities.get(1).unwrap().mission;
        sim.mission_assign_exact(1, GUARD, 10).unwrap();
        assert_eq!(sim.substrate.entities.get(1).unwrap().mission, before);

        let unknown = MissionId::from_raw(0x1234_5678);
        sim.mission_assign_exact(1, unknown, 11).unwrap();
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().mission.current(),
            unknown
        );
    }

    #[test]
    fn mission_authority_queue_unavailable_validation_is_atomic() {
        let mut sim = sim_with(entity(EntityCategory::Unit, GUARD));
        let before = sim.substrate.entities.get(1).unwrap().clone();
        let readiness = TestReadyProvider {
            validation: Err(ReadyUnavailable::SignedHeight),
            ready: true,
            validations: Cell::new(0),
            reads: Cell::new(0),
            missions: RefCell::new(Vec::new()),
        };

        assert!(matches!(
            sim.mission_queue_exact(1, MOVE, 1, 10, &readiness),
            Err(MissionAuthorityError::Readiness(
                ReadyUnavailable::SignedHeight
            ))
        ));
        let after = sim.substrate.entities.get(1).unwrap();
        assert_eq!(after.mission, before.mission);
        assert_eq!(readiness.validations.get(), 1);
        assert_eq!(readiness.reads.get(), 0);
    }

    #[test]
    fn mission_authority_queue_zero_never_reads_or_commences() {
        let mut sim = sim_with(entity(EntityCategory::Unit, GUARD));
        let readiness = TestReadyProvider::ready(true);

        sim.mission_queue_exact(1, MOVE, 0, 10, &readiness).unwrap();
        let entity = sim.substrate.entities.get(1).unwrap();
        assert_eq!(entity.mission.current(), GUARD);
        assert_eq!(entity.mission.queued(), MOVE);
        assert_eq!(readiness.validations.get(), 0);
        assert_eq!(readiness.reads.get(), 0);
    }

    #[test]
    fn mission_authority_queue_tests_only_low_byte_of_commence_now() {
        for commence_now in [0x100, 0x1_0000, i32::MIN] {
            let mut sim = sim_with(entity(EntityCategory::Unit, GUARD));
            let readiness = TestReadyProvider::ready(true);

            sim.mission_queue_exact(1, MOVE, commence_now, 10, &readiness)
                .unwrap();

            let after = sim.substrate.entities.get(1).unwrap();
            assert_eq!(after.mission.current(), GUARD);
            assert_eq!(after.mission.queued(), MOVE);
            assert_eq!(readiness.validations.get(), 0);
            assert_eq!(readiness.reads.get(), 0);
        }

        let mut sim = sim_with(entity(EntityCategory::Unit, GUARD));
        let readiness = TestReadyProvider::ready(true);
        sim.mission_queue_exact(1, MOVE, 0x101, 10, &readiness)
            .unwrap();

        let after = sim.substrate.entities.get(1).unwrap();
        assert_eq!(after.mission.current(), MOVE);
        assert_eq!(after.mission.queued(), MissionId::NONE);
        assert_eq!(readiness.validations.get(), 1);
        assert_eq!(readiness.reads.get(), 1);
    }

    #[test]
    fn mission_authority_queue_ready_false_keeps_queue_ready_true_commences_inline() {
        let mut false_sim = sim_with(entity(EntityCategory::Unit, GUARD));
        let false_ready = TestReadyProvider::ready(false);
        false_sim
            .mission_queue_exact(1, MOVE, 1, 10, &false_ready)
            .unwrap();
        let false_entity = false_sim.substrate.entities.get(1).unwrap();
        assert_eq!(false_entity.mission.current(), GUARD);
        assert_eq!(false_entity.mission.queued(), MOVE);
        assert_eq!(false_ready.validations.get(), 1);
        assert_eq!(false_ready.reads.get(), 1);

        let mut true_sim = sim_with(entity(EntityCategory::Unit, GUARD));
        let true_ready = TestReadyProvider::ready(true);
        true_sim
            .mission_queue_exact(1, MOVE, 1, 10, &true_ready)
            .unwrap();
        let entity = true_sim.substrate.entities.get(1).unwrap();
        assert_eq!(entity.mission.current(), MOVE);
        assert_eq!(entity.mission.queued(), MissionId::NONE);
        assert_eq!(true_ready.validations.get(), 1);
        assert_eq!(true_ready.reads.get(), 1);
    }

    #[test]
    fn aircraft_mission_authority_queue_gate_covers_manager_unknown_and_none() {
        for requested in [MissionId::NONE, MissionId::from_raw(0x1234_5678)] {
            let mut aircraft = entity(EntityCategory::Aircraft, MissionId::from_raw(4));
            aircraft.mission_leaf = MissionLeafState::aircraft_raw_for_test(9, 1, false);
            let before_mission = aircraft.mission;
            let before_leaf = aircraft.mission_leaf;
            let mut sim = sim_with(aircraft);
            let readiness = TestReadyProvider::ready(true);

            sim.mission_queue_exact(1, requested, 1, 10, &readiness)
                .unwrap();

            let after = sim.substrate.entities.get(1).unwrap();
            assert_eq!(after.mission, before_mission);
            assert_eq!(after.mission_leaf, before_leaf);
            assert_eq!(readiness.validations.get(), 0);
            assert_eq!(readiness.reads.get(), 0);
        }

        let mut protected_request = entity(EntityCategory::Aircraft, MissionId::from_raw(4));
        protected_request.mission_leaf = MissionLeafState::aircraft_raw_for_test(9, 1, false);
        let mut sim = sim_with(protected_request);
        sim.mission_queue_exact(
            1,
            MissionId::from_raw(0x1a),
            0,
            10,
            &TestReadyProvider::ready(false),
        )
        .unwrap();
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().mission.queued(),
            MissionId::from_raw(0x1a)
        );

        let mut managed = entity(EntityCategory::Aircraft, MissionId::from_raw(4));
        managed.mission_leaf = MissionLeafState::aircraft_raw_for_test(9, 1, true);
        let unknown = MissionId::from_raw(0x1234_5678);
        let mut sim = sim_with(managed);
        sim.mission_queue_exact(1, unknown, 0, 10, &TestReadyProvider::ready(false))
            .unwrap();
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().mission.queued(),
            unknown
        );
    }

    #[test]
    fn mission_authority_queue_outer_guards_do_not_read_readiness() {
        for (current, requested) in [
            (MissionId::from_raw(19), MOVE),
            (MissionId::from_raw(28), GUARD),
        ] {
            let mut sim = sim_with(entity(EntityCategory::Unit, current));
            let before = sim.substrate.entities.get(1).unwrap().mission;
            let readiness = TestReadyProvider::ready(true);

            sim.mission_queue_exact(1, requested, 1, 10, &readiness)
                .unwrap();

            assert_eq!(sim.substrate.entities.get(1).unwrap().mission, before);
            assert_eq!(readiness.validations.get(), 0);
            assert_eq!(readiness.reads.get(), 0);
        }
    }

    #[test]
    fn mission_authority_queue_none_and_redundant_requests_still_read_once() {
        for requested in [MissionId::NONE, GUARD] {
            let mut sim = sim_with(entity(EntityCategory::Unit, GUARD));
            let before = sim.substrate.entities.get(1).unwrap().mission;
            let readiness = TestReadyProvider::ready(false);

            sim.mission_queue_exact(1, requested, 1, 10, &readiness)
                .unwrap();

            assert_eq!(sim.substrate.entities.get(1).unwrap().mission, before);
            assert_eq!(readiness.validations.get(), 1);
            assert_eq!(readiness.reads.get(), 1);
        }
    }

    #[test]
    fn aircraft_queue_owned_commence_clears_action_except_for_old_1e() {
        for (old_current, requested, expected_action) in [
            (ATTACK, MOVE, 0),
            (AIRCRAFT_ACTION_EXCEPTION, MissionId::from_raw(0x1a), 9),
        ] {
            let mut aircraft = entity(EntityCategory::Aircraft, old_current);
            aircraft.mission_leaf = MissionLeafState::aircraft_raw_for_test(9, 1, false);
            let mut sim = sim_with(aircraft);
            let readiness = TestReadyProvider::ready(true);

            sim.mission_queue_exact(1, requested, 1, 10, &readiness)
                .unwrap();

            let after = sim.substrate.entities.get(1).unwrap();
            assert_eq!(after.mission.current(), requested);
            assert_eq!(after.mission.queued(), MissionId::NONE);
            assert_eq!(
                after.mission_leaf.as_aircraft().unwrap().action_latch(),
                expected_action
            );
            assert_eq!(readiness.validations.get(), 1);
            assert_eq!(readiness.reads.get(), 1);
        }
    }

    #[test]
    fn aircraft_mission_authority_commence_hook_runs_even_with_empty_queue() {
        let mut aircraft = entity(EntityCategory::Aircraft, ATTACK);
        aircraft.mission_leaf = MissionLeafState::aircraft_raw_for_test(9, 1, false);
        let mut sim = sim_with(aircraft);

        assert!(!sim.mission_commence_exact(1, 10).unwrap());
        assert_eq!(
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .mission_leaf
                .as_aircraft()
                .unwrap()
                .action_latch(),
            0
        );
    }

    #[test]
    fn native_e1_override_callers_keep_target_action_and_destination_refusal_order() {
        use crate::rules::ini_parser::IniFile;
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        use crate::sim::components::{Health, MovementTarget};
        use crate::sim::house_state::HouseState;
        use crate::sim::movement::locomotor::LocomotorState;
        use crate::sim::rng::SimRng;
        use serde_json::Value;

        let Some(ini) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
            return;
        };
        let Some(art) = crate::rules::retail_ini_fixture::retail_ini("artmd.ini") else {
            return;
        };
        let registry = crate::rules::infantry_sequence::parse_infantry_sequence_registry(&art);
        let rules: std::collections::BTreeMap<_, _> = ["yes", "no"]
            .into_iter()
            .map(|raw| {
                let mut layers = RulesLayerStack::new(ini.clone());
                layers.push(
                    RulesLayerKind::Scenario,
                    IniFile::from_str(&format!("[E1]\nDeployFire={raw}\n")),
                );
                let mut rules =
                    RuleSet::from_processed_rules(&layers.process_with_fixed_art(&art).unwrap())
                        .unwrap();
                rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
                rules.bind_animation_sequences(&registry);
                (raw, rules)
            })
            .collect();
        let native: Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/base_defense_response.json",
        ))
        .unwrap();
        let completion: Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/walk_completion.json",
        ))
        .unwrap();

        // Original51B1F0/51D6F0/6FCDB0 establish each target/action result.
        // Original51AA40's human Doing gate in walk_completion establishes
        // NULL destination admission after the resulting action. Foot4D8F40
        // and Techno7013A0 call those setters in this order, even on refusal.
        for name in [
            "doing_4_target_assign",
            "doing_0_target_same",
            "deployfire_false_27_assign",
            "deployfire_false_29_assign",
        ] {
            let row = native["infantry_assignment"]["rows"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["input"]["name"] == name)
                .unwrap();
            let before = &row["before"];
            let after = &row["after"];
            let destination = completion
                .as_array()
                .unwrap()
                .iter()
                .find(|row| {
                    row["input"]["human"] == 1
                        && row["input"]["doing"] == after["doing"]
                        && row["input"]["mission"] == 1
                        && row["input"]["queued_mission"] == -1
                        && row["input"]["contact"] == 0
                        && row["input"]["alive"] == 1
                        && row["input"]["limbo"] == 0
                        && row["input"]["falling"] == 0
                })
                .unwrap();
            for damage_response in [true, false] {
                let rules = &rules[row["input"]["deploy_fire_raw"].as_str().unwrap()];
                let mut sim = Simulation::new();
                let owner = sim.interner.intern("Receiver");
                sim.houses
                    .insert(owner, HouseState::new(owner, 0, None, true, 0, 0));
                let mut actor = GameEntity::new_at_frame_zero_for_test(
                    1,
                    10,
                    10,
                    0,
                    0,
                    owner,
                    Health { current: 100 },
                    sim.interner.intern("E1"),
                    EntityCategory::Infantry,
                    0,
                    0,
                    false,
                );
                actor.lifecycle.in_limbo = false;
                actor.mission.apply_test_fixture(MissionTestFixture {
                    current: MOVE,
                    suspended: MissionId::NONE,
                    queued: GUARD,
                    movement_bypass_latch: 0,
                    handler_state: 0,
                    mission_start_frame: 0,
                    ai_counter: 0,
                    dispatch_timer: MissionDispatchTimer::at_frame(0),
                });
                let doing = before["doing"].as_i64().unwrap() as i32;
                actor
                    .mission_leaf
                    .set_infantry_doing_verified(doing)
                    .unwrap();
                actor
                    .mission_leaf
                    .set_foot_firing_sequence(before["firing_latch"].as_u64().unwrap() as u8);
                actor.passively_acquired_target = before["passive"].as_u64().unwrap() != 0;
                let prior_target =
                    (before["target"] == "new_target").then_some(TargetKind::Entity(2));
                actor.attack_target = prior_target.map(|_| AttackTarget::new(2));
                actor.navigation.nav_com = Some(NavTargetRef::cell(12, 10));
                actor.navigation.path_replay.directions =
                    vec![before["path_field"].as_u64().unwrap() as u8, 2, 3];
                actor.movement_target = Some(MovementTarget::default());
                actor.locomotor = Some(LocomotorState::for_test_kind(
                    crate::rules::locomotor_type::LocomotorKind::Walk,
                ));
                let signed = |value: &Value| value.as_i64().unwrap() as i32;
                let words = &before["action_timer"];
                // Supplied native raw entity backing in the existing oracle;
                // FC/110 do not enter this setter comparison or its expected outputs.
                actor.install_native_stage_fixture(
                    crate::sim::stage::StageClass::from_native_fixture(
                        signed(&before["frame"]),
                        0,
                        crate::sim::timer::CdTimer::from_raw(signed(&words[0]), signed(&words[2])),
                        signed(&before["action_repeat"]),
                        0,
                    ),
                );
                sim.substrate.entities.insert(actor);
                sim.substrate
                    .entities
                    .insert(GameEntity::new_at_frame_zero_for_test(
                        2,
                        11,
                        10,
                        0,
                        0,
                        owner,
                        Health { current: 100 },
                        sim.interner.intern("MTNK"),
                        EntityCategory::Unit,
                        0,
                        0,
                        true,
                    ));
                sim.session.binary_frame = row["input"]["frame"].as_u64().unwrap() as u32;
                sim.scenario_rng = SimRng::new(row["input"]["seed"].as_u64().unwrap());
                assert_eq!(sim.scenario_rng.native_state_hex(), row["rng_before"]);

                let ran = if damage_response {
                    sim.override_mission_on_damage_response(1, 2, rules)
                } else {
                    sim.mission_override_movement_blocker(1, TargetKind::Entity(2), rules)
                };
                assert!(ran, "{name}, damage={damage_response}");
                let actor = sim.substrate.entities.get(1).unwrap();
                assert_eq!(actor.mission.current(), ATTACK, "{name}");
                // Original5B3675..5B367B archives the non-null queued
                // selector; current Move is used only without a queue.
                assert_eq!(actor.mission.suspended(), GUARD, "{name}");
                assert_eq!(actor.suspended_attack_target, prior_target, "{name}");
                assert_eq!(
                    actor.navigation.suspended_nav_com,
                    Some(NavTargetRef::cell(12, 10)),
                    "{name}",
                );
                let leaf = actor.mission_leaf.as_infantry().unwrap();
                assert_eq!(
                    leaf.doing(),
                    after["doing"].as_i64().unwrap() as i32,
                    "{name}"
                );
                assert_eq!(
                    leaf.firing_sequence_latch(),
                    after["firing_latch"].as_u64().unwrap() as u8,
                    "{name}",
                );
                assert_eq!(
                    actor.attack_target.as_ref().map(|target| target.target),
                    (after["target"] == "new_target").then_some(TargetKind::Entity(2)),
                    "{name}",
                );
                assert_eq!(
                    actor.passively_acquired_target,
                    after["passive"].as_u64().unwrap() != 0,
                    "{name}",
                );
                assert_eq!(
                    actor.native_stage().value(),
                    after["frame"].as_i64().unwrap() as i32,
                    "{name}",
                );
                let words = &after["action_timer"];
                assert_eq!(
                    (
                        actor.native_stage().timer().start_frame(),
                        actor.native_stage().timer().duration(),
                        actor.native_stage().rate(),
                    ),
                    (
                        signed(&words[0]),
                        signed(&words[2]),
                        signed(&after["action_repeat"]),
                    ),
                    "{name}: native retained action clock",
                );
                assert_eq!(
                    actor.navigation.nav_com.is_some(),
                    destination["nav"].as_bool().unwrap(),
                    "{name}",
                );
                assert_eq!(
                    actor.movement_target.is_some(),
                    destination["nav"].as_bool().unwrap() && after["path_field"] != -1,
                    "only a retained path head can keep its derived movement adapter: {name}",
                );
                let expected_head = (after["path_field"] != -1)
                    .then(|| after["path_field"].as_u64().unwrap() as u8);
                assert_eq!(
                    actor
                        .navigation
                        .path_replay
                        .remaining_directions()
                        .first()
                        .copied(),
                    expected_head,
                    "{name}",
                );
                assert_eq!(
                    sim.scenario_rng.native_state_hex(),
                    row["rng_after"],
                    "{name}"
                );
            }
        }
    }

    #[test]
    fn override_target_unavailable_is_fieldwise_noop() {
        let mut building = entity(EntityCategory::Structure, GUARD);
        building.attack_target = Some(AttackTarget::new(7));
        let before = building.clone();
        let mut sim = sim_with(building);

        assert!(matches!(
            sim.mission_override_exact(
                1,
                OverridePacket {
                    mission: ATTACK,
                    combat_target: Some(TargetKind::Entity(8)),
                    destination: None,
                }
            ),
            Err(MissionAuthorityError::AuthorityUnavailable(
                AuthorityUnavailable::TargetSetter(1)
            ))
        ));
        let after = sim.substrate.entities.get(1).unwrap();
        assert_eq!(after.mission, before.mission);
        assert_eq!(
            after.attack_target.as_ref().map(|target| target.target),
            before.attack_target.as_ref().map(|target| target.target)
        );
        assert_eq!(
            after.suspended_attack_target,
            before.suspended_attack_target
        );
    }

    #[test]
    fn override_destination_unavailable_is_fieldwise_noop() {
        let mut sim = sim_with(entity(EntityCategory::Unit, GUARD));
        let before = sim.substrate.entities.get(1).unwrap().clone();
        let mut effects = RecordingConcreteMissionEffects::available();
        effects.allow_destination = false;

        assert!(matches!(
            sim.mission_override_exact_with_effects(
                1,
                OverridePacket {
                    mission: ATTACK,
                    combat_target: None,
                    destination: Some(NavTargetRef::cell(8, 9)),
                },
                &mut effects,
            ),
            Err(MissionAuthorityError::AuthorityUnavailable(
                AuthorityUnavailable::DestinationSetter(1)
            ))
        ));
        let after = sim.substrate.entities.get(1).unwrap();
        assert_eq!(after.mission, before.mission);
        assert_eq!(
            after.navigation.suspended_nav_com,
            before.navigation.suspended_nav_com
        );
    }

    #[test]
    fn foot_override_provider_order_includes_same_identity_target_dispatch() {
        let mut unit = entity(EntityCategory::Unit, GUARD);
        let active_target = AttackTarget::new(7);
        unit.rearm_timer = crate::sim::timer::CdTimer::started(0, 17);
        unit.weapon_burst.complete_shot(4);
        unit.attack_target = Some(active_target);
        unit.navigation.nav_com = Some(NavTargetRef::cell(1, 2));
        unit.navigation.pending_arrival_clear = true;
        let mut sim = sim_with(unit);
        let mut effects = RecordingConcreteMissionEffects::available();

        sim.mission_override_exact_with_effects(
            1,
            OverridePacket {
                mission: ATTACK,
                combat_target: Some(TargetKind::Entity(7)),
                destination: Some(NavTargetRef::cell(8, 9)),
            },
            &mut effects,
        )
        .unwrap();

        assert_eq!(effects.events.len(), 3);
        assert!(matches!(
            effects.events[0],
            ConcreteEffectEvent::Preflight {
                request: ConcreteSetterRequest::TargetAndDestination { .. },
                ..
            }
        ));
        assert!(matches!(
            effects.events[1],
            ConcreteEffectEvent::Target {
                requested: Some(TargetKind::Entity(7)),
                mission_current: ATTACK,
                suspended_mission: GUARD,
                archived_target: Some(TargetKind::Entity(7)),
                archived_destination: Some(NavTargetRef::Cell { rx: 1, ry: 2 }),
                ..
            }
        ));
        assert!(matches!(
            effects.events[2],
            ConcreteEffectEvent::Destination {
                requested: Some(NavTargetRef::Cell { rx: 8, ry: 9 }),
                mission_current: ATTACK,
                installed_target: Some(TargetKind::Entity(7)),
                ..
            }
        ));
        let installed = sim
            .substrate
            .entities
            .get(1)
            .unwrap()
            .attack_target
            .as_ref()
            .unwrap();
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().rearm_timer,
            crate::sim::timer::CdTimer::started(0, 17)
        );
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().weapon_burst.index(),
            1
        );
        assert_eq!(installed.target, TargetKind::Entity(7));
        assert!(
            !sim.substrate
                .entities
                .get(1)
                .unwrap()
                .navigation
                .pending_arrival_clear
        );
    }

    #[test]
    fn override_transaction_traces_each_concrete_category_and_building_never_sets_nav() {
        for category in [
            EntityCategory::Structure,
            EntityCategory::Unit,
            EntityCategory::Infantry,
            EntityCategory::Aircraft,
        ] {
            let mut receiver = entity(category, GUARD);
            receiver.attack_target = Some(AttackTarget::new(7));
            receiver.navigation.nav_com = Some(NavTargetRef::cell(1, 2));
            let mut sim = sim_with(receiver);
            let mut effects = RecordingConcreteMissionEffects::available();

            sim.mission_override_exact_with_effects(
                1,
                OverridePacket {
                    mission: ATTACK,
                    combat_target: Some(TargetKind::Entity(8)),
                    destination: Some(NavTargetRef::cell(8, 9)),
                },
                &mut effects,
            )
            .unwrap();

            let after = sim.substrate.entities.get(1).unwrap();
            assert_eq!(after.mission.current(), ATTACK);
            assert_eq!(after.mission.suspended(), GUARD);
            assert_eq!(after.suspended_attack_target, Some(TargetKind::Entity(7)));
            assert!(matches!(
                effects.events[0],
                ConcreteEffectEvent::Preflight { .. }
            ));
            assert!(matches!(
                effects.events[1],
                ConcreteEffectEvent::Target {
                    mission_current: ATTACK,
                    suspended_mission: GUARD,
                    archived_target: Some(TargetKind::Entity(7)),
                    ..
                }
            ));

            if category == EntityCategory::Structure {
                assert_eq!(effects.events.len(), 2);
                assert_eq!(after.navigation.nav_com, Some(NavTargetRef::cell(1, 2)));
                assert_eq!(after.navigation.suspended_nav_com, None);
            } else {
                assert_eq!(effects.events.len(), 3);
                assert_eq!(
                    after.navigation.suspended_nav_com,
                    Some(NavTargetRef::cell(1, 2))
                );
                assert_eq!(after.navigation.nav_com, Some(NavTargetRef::cell(8, 9)));
                assert!(matches!(
                    effects.events[2],
                    ConcreteEffectEvent::Destination {
                        mission_current: ATTACK,
                        installed_target: Some(TargetKind::Entity(8)),
                        ..
                    }
                ));
            }
        }
    }

    #[test]
    fn guarded_override_base_still_archives_and_runs_concrete_setters() {
        let deliberate = MissionId::from_raw(28);
        let mut unit = entity(EntityCategory::Unit, deliberate);
        unit.attack_target = Some(AttackTarget::new(7));
        unit.navigation.nav_com = Some(NavTargetRef::cell(1, 2));
        let before_mission = unit.mission;
        let mut sim = sim_with(unit);
        let mut effects = RecordingConcreteMissionEffects::available();

        sim.mission_override_exact_with_effects(
            1,
            OverridePacket {
                mission: GUARD,
                combat_target: Some(TargetKind::Entity(8)),
                destination: Some(NavTargetRef::cell(8, 9)),
            },
            &mut effects,
        )
        .unwrap();

        let after = sim.substrate.entities.get(1).unwrap();
        assert_eq!(after.mission, before_mission);
        assert_eq!(after.suspended_attack_target, Some(TargetKind::Entity(7)));
        assert_eq!(
            after.navigation.suspended_nav_com,
            Some(NavTargetRef::cell(1, 2))
        );
        assert_eq!(
            after.attack_target.as_ref().map(|target| target.target),
            Some(TargetKind::Entity(8))
        );
        assert_eq!(after.navigation.nav_com, Some(NavTargetRef::cell(8, 9)));
        assert!(matches!(
            effects.events.as_slice(),
            [
                ConcreteEffectEvent::Preflight { .. },
                ConcreteEffectEvent::Target {
                    mission_current,
                    suspended_mission,
                    ..
                },
                ConcreteEffectEvent::Destination { .. }
            ] if *mission_current == deliberate && *suspended_mission == MissionId::NONE
        ));
    }

    #[test]
    fn blocked_aircraft_override_has_empty_trace_and_byte_identical_state() {
        let mut aircraft = entity(EntityCategory::Aircraft, MissionId::from_raw(4));
        aircraft.mission_leaf = MissionLeafState::aircraft_raw_for_test(9, 1, false);
        aircraft.attack_target = Some(AttackTarget::new(7));
        aircraft.navigation.nav_com = Some(NavTargetRef::cell(1, 2));
        let before = bincode::serialize(&aircraft).expect("serialize blocked Aircraft");
        let mut sim = sim_with(aircraft);
        let mut effects = RecordingConcreteMissionEffects::available();

        sim.mission_override_exact_with_effects(
            1,
            OverridePacket {
                mission: MissionId::NONE,
                combat_target: Some(TargetKind::Entity(8)),
                destination: Some(NavTargetRef::cell(8, 9)),
            },
            &mut effects,
        )
        .unwrap();

        let after =
            bincode::serialize(sim.substrate.entities.get(1).unwrap()).expect("serialize result");
        assert_eq!(after, before);
        assert!(effects.events.is_empty());
    }

    #[test]
    fn restore_empty_does_not_require_concrete_provider() {
        let mut sim = sim_with(entity(EntityCategory::Unit, GUARD));
        assert!(!sim.mission_restore_exact(1).unwrap());
    }

    #[test]
    fn restore_success_unavailable_does_not_pop_mission() {
        let mut unit = entity(EntityCategory::Unit, ATTACK);
        unit.mission.apply_test_fixture(MissionTestFixture {
            current: ATTACK,
            suspended: GUARD,
            queued: MOVE,
            movement_bypass_latch: 9,
            handler_state: 10,
            mission_start_frame: 11,
            ai_counter: 12,
            dispatch_timer: MissionDispatchTimer::from_raw(13, 14),
        });
        let before = unit.mission;
        let mut sim = sim_with(unit);

        assert!(matches!(
            sim.mission_restore_exact(1),
            Err(MissionAuthorityError::AuthorityUnavailable(_))
        ));
        assert_eq!(sim.substrate.entities.get(1).unwrap().mission, before);
    }

    #[test]
    fn restore_provider_order_is_target_then_destination_and_retains_archives() {
        let mut unit = entity(EntityCategory::Unit, ATTACK);
        unit.mission.apply_test_fixture(MissionTestFixture {
            current: ATTACK,
            suspended: GUARD,
            queued: MOVE,
            movement_bypass_latch: 9,
            handler_state: 10,
            mission_start_frame: 11,
            ai_counter: 12,
            dispatch_timer: MissionDispatchTimer::from_raw(13, 14),
        });
        unit.suspended_attack_target = Some(TargetKind::Entity(7));
        unit.navigation.suspended_nav_com = Some(NavTargetRef::cell(8, 9));
        let mut sim = sim_with(unit);
        let mut effects = RecordingConcreteMissionEffects::available();

        assert!(
            sim.mission_restore_exact_with_effects(1, &mut effects)
                .unwrap()
        );
        let entity = sim.substrate.entities.get(1).unwrap();
        assert_eq!(entity.mission.current(), GUARD);
        assert_eq!(entity.mission.queued(), MOVE);
        assert_eq!(entity.suspended_attack_target, Some(TargetKind::Entity(7)));
        assert_eq!(
            entity.navigation.suspended_nav_com,
            Some(NavTargetRef::cell(8, 9))
        );
        assert!(matches!(
            effects.events.as_slice(),
            [
                ConcreteEffectEvent::Preflight { .. },
                ConcreteEffectEvent::Target { .. },
                ConcreteEffectEvent::Destination { .. }
            ]
        ));
    }

    #[test]
    fn missing_receiver_is_atomic_for_every_exact_authority_entry() {
        let mut sim = Simulation::new();
        let readiness = TestReadyProvider::ready(true);

        assert!(matches!(
            sim.mission_assign_exact(99, MOVE, 0),
            Err(MissionAuthorityError::MissingReceiver(99))
        ));
        assert!(matches!(
            sim.mission_queue_exact(99, MOVE, 1, 0, &readiness),
            Err(MissionAuthorityError::MissingReceiver(99))
        ));
        assert!(matches!(
            sim.mission_commence_exact(99, 0),
            Err(MissionAuthorityError::MissingReceiver(99))
        ));
        assert!(matches!(
            sim.mission_override_exact(
                99,
                OverridePacket {
                    mission: MOVE,
                    combat_target: None,
                    destination: None,
                }
            ),
            Err(MissionAuthorityError::MissingReceiver(99))
        ));
        assert!(matches!(
            sim.mission_restore_exact(99),
            Err(MissionAuthorityError::MissingReceiver(99))
        ));
    }

    #[test]
    fn mission_authority_operations_preserve_all_rng_streams() {
        let mut sim = sim_with(entity(EntityCategory::Unit, GUARD));
        let before = (
            sim.scenario_rng.state(),
            sim.main_rng.state(),
            sim.mapgen_rng.state(),
        );
        let readiness = TestReadyProvider::ready(false);

        sim.mission_assign_exact(1, ATTACK, 1).unwrap();
        sim.mission_queue_exact(1, MOVE, 1, 2, &readiness).unwrap();
        let after = (
            sim.scenario_rng.state(),
            sim.main_rng.state(),
            sim.mapgen_rng.state(),
        );
        assert_eq!(after, before);
    }

    #[test]
    fn mission_b8_owner_sequence_refinery_queues_then_sets_latch() {
        let mut sim = sim_with(entity(EntityCategory::Unit, ATTACK));

        sim.mission_refinery_completion_exact(1, 10).unwrap();

        let mission = &sim.substrate.entities.get(1).unwrap().mission;
        assert_eq!(mission.current(), ATTACK);
        assert_eq!(mission.queued(), GUARD);
        assert_eq!(mission.movement_bypass_latch(), 1);
    }

    #[test]
    fn mission_b8_owner_sequence_jumpjet_second_gate_can_commence_and_clear_latch() {
        let mut sim = sim_with(entity(EntityCategory::Unit, ATTACK));
        let readiness = SequencedReadyProvider::new([false, true]);

        sim.mission_jumpjet_move_to_completion_exact(1, 10, &readiness)
            .unwrap();

        let mission = &sim.substrate.entities.get(1).unwrap().mission;
        assert_eq!(readiness.validations.get(), 3);
        assert_eq!(readiness.reads.get(), 2);
        let previews = readiness.missions.borrow();
        assert_eq!(previews[0].current(), ATTACK);
        assert_eq!(previews[0].queued(), GUARD);
        assert_eq!(previews[0].movement_bypass_latch(), 1);
        assert_eq!(previews[1].current(), GUARD);
        assert_eq!(previews[1].queued(), MissionId::NONE);
        assert_eq!(previews[1].movement_bypass_latch(), 1);
        assert_eq!(previews[2].current(), ATTACK);
        assert_eq!(previews[2].queued(), GUARD);
        assert_eq!(previews[2].movement_bypass_latch(), 0);
        assert_eq!(mission.current(), GUARD);
        assert_eq!(mission.queued(), MissionId::NONE);
        assert_eq!(mission.movement_bypass_latch(), 0);
    }

    #[test]
    fn mission_b8_owner_sequence_jumpjet_later_false_leaves_latch_set() {
        let mut sim = sim_with(entity(EntityCategory::Unit, ATTACK));
        let readiness = SequencedReadyProvider::new([false, false]);

        sim.mission_jumpjet_move_to_completion_exact(1, 10, &readiness)
            .unwrap();

        let mission = &sim.substrate.entities.get(1).unwrap().mission;
        assert_eq!(readiness.validations.get(), 3);
        assert_eq!(readiness.reads.get(), 2);
        assert_eq!(mission.current(), ATTACK);
        assert_eq!(mission.queued(), GUARD);
        assert_eq!(mission.movement_bypass_latch(), 1);
    }

    #[test]
    fn mission_b8_owner_sequence_jumpjet_second_preflight_error_is_atomic() {
        let mut sim = sim_with(entity(EntityCategory::Unit, ATTACK));
        let before_mission = sim.substrate.entities.get(1).unwrap().mission;
        let before_leaf = sim.substrate.entities.get(1).unwrap().mission_leaf;
        let readiness = RejectSuccessfulJumpjetPreview {
            validations: Cell::new(0),
            reads: Cell::new(0),
            missions: RefCell::new(Vec::new()),
        };

        assert!(matches!(
            sim.mission_jumpjet_move_to_completion_exact(1, 10, &readiness),
            Err(MissionAuthorityError::Readiness(
                ReadyUnavailable::WorldLookup
            ))
        ));

        let after = sim.substrate.entities.get(1).unwrap();
        assert_eq!(after.mission, before_mission);
        assert_eq!(after.mission_leaf, before_leaf);
        assert_eq!(readiness.validations.get(), 2);
        assert_eq!(readiness.reads.get(), 0);
        let previews = readiness.missions.borrow();
        assert_eq!(previews[0].current(), ATTACK);
        assert_eq!(previews[0].queued(), GUARD);
        assert_eq!(previews[0].movement_bypass_latch(), 1);
        assert_eq!(previews[1].current(), GUARD);
        assert_eq!(previews[1].queued(), MissionId::NONE);
        assert_eq!(previews[1].movement_bypass_latch(), 1);
    }

    #[test]
    fn building_ready_consume_empty_queue_preserves_latch() {
        let mut building = entity(EntityCategory::Structure, GUARD);
        building.mission_leaf = MissionLeafState::building_raw_for_test(1);
        let mut sim = sim_with(building);

        assert!(!sim.mission_building_ready_commence(1, 10).unwrap());
        assert_eq!(
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .mission_leaf
                .as_building()
                .unwrap()
                .ready_latch(),
            1
        );
    }

    #[test]
    fn building_ready_consume_success_clears_latch() {
        let mut building = entity(EntityCategory::Structure, GUARD);
        building.mission_leaf = MissionLeafState::building_raw_for_test(1);
        building.mission.apply_test_fixture(MissionTestFixture {
            current: GUARD,
            suspended: MissionId::NONE,
            queued: MOVE,
            movement_bypass_latch: 9,
            handler_state: 10,
            mission_start_frame: 11,
            ai_counter: 12,
            dispatch_timer: MissionDispatchTimer::from_raw(13, 14),
        });
        let mut sim = sim_with(building);

        assert!(sim.mission_building_ready_commence(1, 10).unwrap());
        let entity = sim.substrate.entities.get(1).unwrap();
        assert_eq!(entity.mission.current(), MOVE);
        assert_eq!(entity.mission_leaf.as_building().unwrap().ready_latch(), 0);
    }
}
