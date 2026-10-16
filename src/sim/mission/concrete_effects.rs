//! Concrete Target/NavCom effects required by Mission wrapper transactions.
//!
//! The verified Techno/Foot wrappers archive intent around virtual setters, but
//! the complete category-specific setters are not implemented in Rust yet.
//! This sealed two-phase interface lets Mission authority prove availability
//! before its first write and then commit an infallible, ordered transaction.

use crate::sim::combat::TargetKind;
use crate::sim::components::NavTargetRef;
use crate::sim::world::FrameEffects;
use crate::sim::world::Simulation;

#[cfg(test)]
#[path = "infantry_target_tests.rs"]
mod infantry_target_tests;

#[cfg(test)]
#[path = "building_destination_tests.rs"]
mod building_destination_tests;

mod private {
    pub trait Sealed {}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConcreteSetterRequest {
    Target {
        requested: Option<TargetKind>,
    },
    Destination {
        requested: Option<NavTargetRef>,
    },
    TargetAndDestination {
        requested_target: Option<TargetKind>,
        requested_destination: Option<NavTargetRef>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AuthorityUnavailable {
    #[error("exact concrete Target setter is unavailable for Mission receiver {0}")]
    TargetSetter(u64),
    #[error("exact destination setter is unavailable for Mission receiver {0}")]
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

    /// Returns whether the receiver's class setter ran. False leaves only the
    /// represented NavCom write, with the class setter still owed.
    fn apply_destination(
        &mut self,
        sim: &mut Simulation,
        prepared: &Self::Prepared,
        requested: Option<NavTargetRef>,
    ) -> bool;
}

/// Honest production boundary until full concrete Target and destination
/// setters are implemented.
#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Default)]
pub(crate) struct UnavailableConcreteMissionEffects;

#[cfg(any(test, feature = "test-support"))]
impl private::Sealed for UnavailableConcreteMissionEffects {}

#[cfg(any(test, feature = "test-support"))]
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
            ConcreteSetterRequest::Destination { .. } => {
                Err(AuthorityUnavailable::DestinationSetter(receiver))
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

    fn apply_destination(
        &mut self,
        _sim: &mut Simulation,
        _prepared: &Self::Prepared,
        _requested: Option<NavTargetRef>,
    ) -> bool {
        unreachable!("unavailable provider cannot produce a destination token")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RepresentedPrepared {
    receiver: u64,
}

#[derive(Default)]
pub(crate) struct RepresentedConcreteMissionEffects<'r> {
    /// Class setter type/contact/timer inputs; overlay data belongs to the
    /// moving Infantry receiver's current-cell admission query.
    rules: Option<&'r crate::rules::ruleset::RuleSet>,
    overlay_registry: Option<&'r crate::rules::overlay_types::OverlayTypeRegistry>,
    frame_effects: FrameEffects<'r>,
    /// vt+0x480's mode 0 (keep NavQueue) instead of 1. Only Unit741970
    /// reads it; Infantry51AA40 and Aircraft41AA80 pass it to Foot4D94B0,
    /// which never reads it.
    keep_queue: bool,
}

impl<'r> RepresentedConcreteMissionEffects<'r> {
    pub(crate) fn new(
        rules: Option<&'r crate::rules::ruleset::RuleSet>,
        overlay_registry: Option<&'r crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: FrameEffects<'r>,
    ) -> Self {
        Self {
            rules,
            overlay_registry,
            frame_effects,
            keep_queue: false,
        }
    }

    /// The same provider calling vt+0x480 with mode 0.
    pub(crate) fn keeping_queue(self) -> Self {
        Self {
            keep_queue: true,
            ..self
        }
    }

    /// Whether the receiver's destination setter takes `destination`
    /// without failing: the read-only half of
    /// [`ConcreteMissionEffects::preflight`].
    fn destination_available(
        &self,
        sim: &Simulation,
        receiver: u64,
        destination: Option<NavTargetRef>,
    ) -> bool {
        if self.rules.is_some()
            && let Some(
                NavTargetRef::Entity { id }
                | NavTargetRef::Object { id }
                | NavTargetRef::Building { id },
            ) = destination
            && !sim.substrate.entities.contains(id)
            // Building455D50 only archives the pointer; it never asks the
            // destination for a coordinate or otherwise dereferences it.
            && sim.substrate.entities.get(receiver).is_some_and(|actor| {
                actor.category() != crate::map::entities::EntityCategory::Structure
            })
        {
            return false;
        }
        let (Some(rules), Some(destination)) = (self.rules, destination) else {
            return true;
        };
        let actor = sim
            .substrate
            .entities
            .get(receiver)
            .expect("present receiver");
        if matches!(
            actor.category(),
            crate::map::entities::EntityCategory::Unit
                | crate::map::entities::EntityCategory::Infantry
        ) && sim.object_type(actor.type_ref(), rules).is_none()
        {
            return false;
        }
        // The Aircraft setter reads the same +4C (`0x0041AA80` ends in Foot
        // `0x004D94B0`), and its port expects the coordinate.
        if (actor.category() == crate::map::entities::EntityCategory::Unit
            && sim.unit_setter_receiver(receiver, Some(rules))
            || actor.category() == crate::map::entities::EntityCategory::Aircraft)
            && !matches!(destination, NavTargetRef::Cell { .. })
            && crate::sim::movement::nav_target_coordinate(
                destination,
                Some(receiver),
                &sim.substrate.entities,
                sim.resolved_terrain.as_ref(),
                Some((rules, &sim.interner)),
            )
            .is_err()
        {
            // An object +4C projection is read-only: unlike Cell lookup,
            // this does not stamp the shared dummy during preflight.
            return false;
        }
        !sim.infantry_setter_receiver(receiver, rules)
            || sim.infantry_destination_inputs_available(
                receiver,
                destination,
                rules,
                self.overlay_registry,
            )
    }

    /// Whether a class setter, not the NavCom-only fallback, takes a
    /// non-null destination for `receiver`: the arms of
    /// [`ConcreteMissionEffects::apply_destination`].
    fn class_setter_represents(&self, sim: &Simulation, receiver: u64) -> bool {
        use crate::map::entities::EntityCategory;
        let Some(actor) = sim.substrate.entities.get(receiver) else {
            return false;
        };
        match actor.category() {
            EntityCategory::Structure => true,
            EntityCategory::Unit => self
                .rules
                .is_some_and(|rules| sim.unit_setter_receiver(receiver, Some(rules))),
            EntityCategory::Infantry => self
                .rules
                .is_some_and(|rules| sim.infantry_setter_receiver(receiver, rules)),
            EntityCategory::Aircraft => self.rules.is_some(),
        }
    }
}

impl private::Sealed for RepresentedConcreteMissionEffects<'_> {}

impl ConcreteMissionEffects for RepresentedConcreteMissionEffects<'_> {
    type Prepared = RepresentedPrepared;

    fn preflight(
        &mut self,
        sim: &Simulation,
        receiver: u64,
        request: ConcreteSetterRequest,
    ) -> Result<Self::Prepared, AuthorityUnavailable> {
        if !sim.substrate.entities.contains(receiver) {
            return Err(match request {
                ConcreteSetterRequest::Destination { .. } => {
                    AuthorityUnavailable::DestinationSetter(receiver)
                }
                ConcreteSetterRequest::Target { .. }
                | ConcreteSetterRequest::TargetAndDestination { .. } => {
                    AuthorityUnavailable::TargetSetter(receiver)
                }
            });
        }
        let (target, destination) = match request {
            ConcreteSetterRequest::Target { requested } => (requested, None),
            ConcreteSetterRequest::Destination { requested } => (None, requested),
            ConcreteSetterRequest::TargetAndDestination {
                requested_target,
                requested_destination,
            } => (requested_target, requested_destination),
        };
        if !self.destination_available(sim, receiver, destination) {
            return Err(AuthorityUnavailable::DestinationSetter(receiver));
        }
        // The Infantry Target setter's infiltration arm sets a destination of
        // its own ([`Self::apply_target`]). NavCom, which decides that arm,
        // has no writer before it, so this answers as the arm will.
        if self
            .rules
            .and_then(|rules| sim.infiltrator_target_building(receiver, target, rules))
            .is_some_and(|building| {
                !self.destination_available(sim, receiver, Some(NavTargetRef::object(building)))
            })
        {
            return Err(AuthorityUnavailable::TargetSetter(receiver));
        }
        Ok(RepresentedPrepared { receiver })
    }

    fn apply_target(
        &mut self,
        sim: &mut Simulation,
        prepared: &Self::Prepared,
        requested: Option<TargetKind>,
    ) {
        // A building's setter is BuildingClass::SetTarget (vt+0x3C8,
        // `0x00443B90`), which hands a target it refuses to this base setter
        // as NULL (`0x00443BA9`, `0x00443BFE`): the same-target test then
        // compares NULL, so a refused current target is cleared.
        let requested = requested.filter(|_| {
            self.rules
                .is_none_or(|rules| sim.building_admits_target(prepared.receiver, requested, rules))
        });
        let entity = sim
            .substrate
            .entities
            .get(prepared.receiver)
            .expect("preflight guaranteed receiver");
        let changes = entity.attack_target.target() != requested;
        // Infantry51B20E..51B24F clears its firing latch and requests an
        // unforced idle action BEFORE Techno6FCDB0 reads/writes TarCom. The
        // current Doing must survive until Do_Action tests interruptibility.
        if changes && let Some(rules) = self.rules {
            sim.infantry_target_change_action(prepared.receiver, rules);
        }
        let entity = sim
            .substrate
            .entities
            .get(prepared.receiver)
            .expect("target-change action retains its receiver");
        let infantry = entity.category() == crate::map::entities::EntityCategory::Infantry;
        //51B273..51B299: even an unchanged target or a Health<=0 receiver
        // takes this gate, using Doing AFTER the requested idle action.
        if infantry
            && entity
                .mission_leaf
                .as_infantry()
                .is_some_and(|leaf| matches!(leaf.doing(), 27..=30))
            && self.rules.is_some_and(|rules| {
                sim.object_type(entity.type_ref(), rules)
                    .is_some_and(|object| !object.deploy_fire)
            })
        {
            return;
        }
        //6FCDD2..6FCDE1: only a receiver owning its manager clears the
        //designation here; aircraft and marked buildings borrow +294.
        if changes && entity.airstrike_link() == Some(prepared.receiver) {
            sim.airstrike_clear_target(prepared.receiver, self.rules, self.frame_effects);
        }
        let commits = assign_target_commits(&sim.substrate.entities, requested);
        let entity = sim
            .substrate
            .entities
            .get_mut(prepared.receiver)
            .expect("preflight guaranteed receiver");
        if infantry {
            //51B2A2: a same-target assignment still invalidates Foot+5E0.
            entity.clear_live_path_head();
        }
        represented_assign_target_admitted(entity, requested, commits);
        //51B2B1..51B33F: an Infiltrate= infantryman with no NavCom heads for
        // a building target it may enter (vt+0x480(target, 1)).
        if infantry
            && let Some(building) = self.rules.and_then(|rules| {
                sim.infiltrator_target_building(prepared.receiver, requested, rules)
            })
        {
            self.apply_destination(sim, prepared, Some(NavTargetRef::object(building)));
        }
        if infantry {
            // Infantry51B310/51B33F: even a same-target assignment clears
            // automatic charging intent, after the infiltration tail. The
            // Doing27..30 refusal above never reaches this write.
            sim.substrate
                .entities
                .get_mut(prepared.receiver)
                .unwrap()
                .tesla_charger
                .clear();
        }
    }

    fn apply_destination(
        &mut self,
        sim: &mut Simulation,
        prepared: &Self::Prepared,
        requested: Option<NavTargetRef>,
    ) -> bool {
        if sim
            .substrate
            .entities
            .get(prepared.receiver)
            .is_some_and(|actor| {
                actor.category() == crate::map::entities::EntityCategory::Structure
            })
        {
            sim.set_building_destination(prepared.receiver, requested, self.rules);
            return true;
        }
        if requested.is_none() {
            sim.assign_null_destination(
                prepared.receiver,
                self.rules,
                self.overlay_registry,
                self.frame_effects,
            );
            return true;
        }
        let requested = requested.expect("nonnull destination arm");
        if self.class_setter_represents(sim, prepared.receiver) {
            let rules = self.rules.expect("a represented class setter has rules");
            match sim
                .substrate
                .entities
                .get(prepared.receiver)
                .expect("preflight guaranteed receiver")
                .category()
            {
                // `UnitClass::Assign_Destination @ 0x00741970`, the one
                // setter that reads the mode.
                crate::map::entities::EntityCategory::Unit => {
                    let _ = sim.set_unit_destination(
                        prepared.receiver,
                        requested,
                        rules,
                        !self.keep_queue,
                        self.frame_effects,
                    );
                }
                // `InfantryClass::Assign_Destination @ 0x0051AA40`.
                crate::map::entities::EntityCategory::Infantry => {
                    let _ = sim
                        .assign_infantry_destination(
                            prepared.receiver,
                            requested,
                            rules,
                            self.overlay_registry,
                            self.frame_effects,
                        )
                        .expect("represented Infantry destination dependencies must be available");
                }
                // `AircraftClass::Assign_Destination @ 0x0041AA80`.
                crate::map::entities::EntityCategory::Aircraft => {
                    sim.assign_aircraft_destination(
                        prepared.receiver,
                        Some(requested),
                        rules,
                        self.frame_effects,
                    );
                }
                crate::map::entities::EntityCategory::Structure => {
                    unreachable!("the Building setter took the destination above")
                }
            }
            return true;
        }
        let entity = sim
            .substrate
            .entities
            .get_mut(prepared.receiver)
            .expect("preflight guaranteed receiver");
        // RESIDUAL: a receiver no class setter above represents keeps the
        // NavCom write set alone: a rules-less fixture, a Unit or Infantry
        // without a constructed locomotor (the zero-speed gate in
        // `should_construct_locomotor`), a Fly or Rocket Unit, or an Infantry
        // on Drive, Ship, Hover, Fly or Rocket. Trigger: an order to such an
        // object; in retail rules only the four types without rules data
        // (DeathDummy, YDUM, VISC_LRG, VISC_SML) lack a locomotor, and no
        // other VehicleType or InfantryType falls here
        // (`every_retail_unit_and_infantry_type_reaches_its_class_setter`).
        // Effect: the order skips the setter's admission, timers and Move_To.
        // Frequency: none in retail play observed. Risk: such an object keeps
        // a NavCom no locomotor drives toward.
        represented_assign_destination(entity, Some(requested));
        false
    }
}

impl Simulation {
    /// Whether vt+0x480 with `requested` would run the receiver's class
    /// setter, not the dispatcher's NavCom-only fallback, with every input
    /// its preflight checks available. The deferred setter's completion
    /// (`complete_pending_order`) asks it before it mutates.
    pub(crate) fn destination_setter_available(
        &self,
        receiver: u64,
        requested: NavTargetRef,
        rules: &crate::rules::ruleset::RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> bool {
        let effects = RepresentedConcreteMissionEffects::new(
            Some(rules),
            overlay_registry,
            FrameEffects::empty(),
        );
        effects.class_setter_represents(self, receiver)
            && effects.destination_available(self, receiver, Some(requested))
    }

    /// `BuildingClass::Assign_Destination @ 0x00455D50` (vt+0x480).
    /// Selling returns untouched. HasRallyPoint (`0x00455DA0`) or
    /// ConstructionYard archives the requested pointer through the existing
    /// `Set_ArchiveTarget @ 0x0070C610` owner; the Techno709A30 tail is a
    /// no-op. A Building therefore never writes Foot's NavCom or its timers.
    ///
    /// EMP belongs to Stop input admission44F5C0, not this setter. The mode
    /// argument is unused. Native executable controls (null/non-null,
    /// Selling/Construction, mode0/1, EMP) are in
    /// `tools/procedural_drawing_oracle/factory_destination.{py,json}`.
    pub(crate) fn set_building_destination(
        &mut self,
        receiver: u64,
        requested: Option<NavTargetRef>,
        rules: Option<&crate::rules::ruleset::RuleSet>,
    ) {
        let Some(actor) = self.substrate.entities.get(receiver) else {
            return;
        };
        debug_assert_eq!(
            actor.category(),
            crate::map::entities::EntityCategory::Structure
        );
        if actor.mission.current().known() == Some(super::MissionType::Selling) {
            return;
        }
        // Production callers carry rules. A bare-storage fixture without the
        // type cannot establish rally admission and leaves its archive alone.
        let archives = rules
            .and_then(|rules| self.object_type(actor.type_ref(), rules))
            .is_some_and(|kind| kind.has_rally_line() || kind.construction_yard);
        if archives {
            self.substrate
                .entities
                .get_mut(receiver)
                .expect("same Building destination receiver")
                .set_archive_target(requested.map(TargetKind::from));
        }
    }
}

/// Whether `TechnoClass::Assign_Target @ 0x006FCDB0` commits the requested
/// target (`0x006FCE4B..0x006FCF36`). A cell or NULL passes. An object commits
/// only while it is alive (`+0x90`) with nonzero Health (`+0x6C`), and an
/// Infantry only outside a death action (`IsInDeathSequence @ 0x00522CB0`,
/// `0x006FCF2D`), which keeps a shot-down Rocketeer falling at Health 1 off
/// every target list; otherwise the setter writes NULL, so a Restore or a
/// retaliation that names a dying object leaves its receiver without a
/// target. Techno+3CD also refuses the candidate (`0x006FCF10`); its stock
/// surface-ship producer is Unit ReceiveDamage737E51, owned by world::sinking.
pub(crate) fn assign_target_commits(
    entities: &crate::sim::entity_store::EntityStore,
    requested: Option<TargetKind>,
) -> bool {
    match requested {
        Some(TargetKind::Entity(id)) => entities.get(id).is_some_and(|target| {
            target.lifecycle.object_alive()
                && target.health.current() != 0
                && !target.sinking.is_active()
                && !(target.category() == crate::map::entities::EntityCategory::Infantry
                    && target.mission_leaf.as_infantry().is_some_and(|leaf| {
                        crate::sim::movement::infantry_action::in_death_sequence(leaf.doing())
                    }))
        }),
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
/// This is the represented Techno6FCDB0 base write set, shared by Mission
/// transactions and remaining bare-storage consumers. Infantry class action,
/// DeployFire admission and path invalidation run before it in apply_target;
/// they must not be approximated by clearing Doing here. Native Techno6FCDB0
/// also redirects targets (a tank-bunkered object, self) and tears down linked
/// effects. Remaining bare-storage callers do not yet dispatch the complete
/// class setter; neither this base write set nor apply_target claims those
/// larger linked-effect/C4/Ivan branches.
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
    entity.passive_scan.clear_acquired();
    // The same-target early-out (`0x006FCDCC`, Infantry `0x0051B201`) compares
    // the requested pointer, before the liveness refusal.
    if entity.attack_target.target() == requested {
        return;
    }
    // `0x006FCDEB..0x006FCE46`, the aircraft arm: a spawned aircraft with
    // ammo, mid-run in Mission_Attack states 5..9, handed another target
    // drops it and empties its ammo, so it leaves the run. This reads the
    //TYPE's Spawned/MissileSpawn flags, also for Boris planes without a
    //SpawnOwner. A NULL request reaches this gate too (6FCDEB..6FCE46).
    let requested = if entity.type_facts.spawned_non_missile_aircraft()
        && matches!(crate::sim::aircraft::attack_state(entity), Some(5..=9))
        && let Some(ammo) = entity.aircraft_ammo.as_mut()
        && ammo.current() != 0
    {
        ammo.empty();
        None
    } else if commits {
        requested
    } else {
        None
    };

    if requested.is_none() {
        // `0x006FCF38..0x006FCF4E`: a NULL target goes on to the
        // SpawnManager (`SetTarget 0x006B7B90`), which drops a queued
        // retarget while it keeps a current target.
        if let Some(manager) = entity.spawn_manager.as_mut() {
            manager.set_target(None);
        }
        entity.weapon_burst.reset();
    }
    // The store (`0x006FCF3E`).
    entity.attack_target.set(requested);
}

/// The represented `Assign_Destination` write set, entity-local, for either
/// mode: it never touches NavQueue.
pub(crate) fn represented_assign_destination(
    entity: &mut crate::sim::game_entity::GameEntity,
    requested: Option<NavTargetRef>,
) {
    entity.navigation.publish_nav_com(requested);
}

#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RecordingPrepared {
    receiver: u64,
    request: ConcreteSetterRequest,
}

#[cfg(any(test, feature = "test-support"))]
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

#[cfg(any(test, feature = "test-support"))]
#[derive(Debug)]
pub(crate) struct RecordingConcreteMissionEffects {
    pub allow_target: bool,
    pub allow_destination: bool,
    pub events: Vec<ConcreteEffectEvent>,
}

#[cfg(any(test, feature = "test-support"))]
impl RecordingConcreteMissionEffects {
    pub(crate) fn available() -> Self {
        Self {
            allow_target: true,
            allow_destination: true,
            events: Vec::new(),
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
impl private::Sealed for RecordingConcreteMissionEffects {}

#[cfg(any(test, feature = "test-support"))]
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
            ConcreteSetterRequest::Destination { .. } => {
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
            ConcreteSetterRequest::Destination { .. } => false,
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
            archived_target: entity.suspended_attack_target.target(),
            archived_destination: entity.navigation.suspended_nav_com(),
        });
        if entity.attack_target.target() != requested {
            entity.attack_target.set_for_test(requested);
        }
    }

    fn apply_destination(
        &mut self,
        sim: &mut Simulation,
        prepared: &Self::Prepared,
        requested: Option<NavTargetRef>,
    ) -> bool {
        debug_assert!(match prepared.request {
            ConcreteSetterRequest::Destination {
                requested: destination,
            }
            | ConcreteSetterRequest::TargetAndDestination {
                requested_destination: destination,
                ..
            } => destination == requested,
            ConcreteSetterRequest::Target { .. } => false,
        });
        let entity = sim
            .substrate
            .entities
            .get_mut(prepared.receiver)
            .expect("preflight guaranteed receiver");
        self.events.push(ConcreteEffectEvent::Destination {
            receiver: prepared.receiver,
            requested,
            mission_current: entity.mission.current(),
            installed_target: entity.attack_target.target(),
        });
        entity.navigation.set_nav_com_aux_for_test(None);
        entity.navigation.set_nav_com_for_test(requested);
        entity
            .navigation
            .set_pending_destination_setter_for_test(false);
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn retail_rules() -> Option<crate::rules::ruleset::RuleSet> {
        let (rules_ini, art_ini) = crate::rules::retail_ini_fixture::retail_rules_and_art()?;
        let mut rules =
            crate::rules::ruleset::RuleSet::from_ini_with_fixed_art_for_test(&rules_ini, &art_ini)
                .expect("retail rules parse");
        rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art_ini));
        Some(rules)
    }

    /// vt+0x480's mode reaches only the Unit setter (`0x00741970` reads its
    /// `clear_queue`): mode 1 clears a retail tank's NavQueue, mode 0 keeps
    /// it.
    #[test]
    fn only_a_mode_one_destination_clears_a_unit_navqueue() {
        let Some(rules) = retail_rules() else {
            return;
        };
        for keep_queue in [true, false] {
            let mut sim = Simulation::new();
            crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
            let id = sim
                .spawn_object("MTNK", "Americans", 10, 10, 0, &rules)
                .expect("retail MTNK");
            let queued = vec![NavTargetRef::cell(14, 10), NavTargetRef::cell(16, 10)];
            sim.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .navigation
                .set_nav_queue_for_test(queued.clone());
            let requested = Some(NavTargetRef::cell(12, 10));
            let effects = FrameEffects::default();
            let assigned = if keep_queue {
                sim.assign_destination_keeping_queue(id, requested, Some(&rules), None, effects)
            } else {
                sim.assign_destination_represented(id, requested, Some(&rules), None, effects)
            };
            assigned.expect("the Unit setter takes the cell");
            let unit = sim.substrate.entities.get(id).unwrap();
            assert_eq!(unit.navigation.nav_com(), requested);
            let kept = if keep_queue { queued } else { Vec::new() };
            assert_eq!(unit.navigation.nav_queue(), kept, "keep_queue {keep_queue}");
        }
    }

    /// FootClass::Mission_Enter (`0x004D9290`) is the Infantry's too
    /// (vt+0x240), and no class test precedes its arms: a tethered
    /// infantryman with no NavCom takes NavQueue[0] through his class setter
    /// with mode 0 (`0x004D939A`), and the queue shifts.
    #[test]
    fn a_tethered_infantryman_on_enter_takes_his_first_waypoint() {
        let Some(rules) = retail_rules() else {
            return;
        };
        let mut sim = Simulation::new();
        crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
        let building = sim
            .spawn_object("GAPOWR", "Americans", 20, 20, 0, &rules)
            .expect("retail GAPOWR");
        let id = sim
            .spawn_object("E1", "Americans", 10, 10, 0, &rules)
            .expect("retail E1");
        let queued = [NavTargetRef::cell(14, 10), NavTargetRef::cell(16, 10)];
        let infantry = sim.substrate.entities.get_mut(id).unwrap();
        infantry.mark_live_contact_with(building);
        infantry.tether.set_for_test(Some(building));
        infantry.navigation.set_nav_queue_for_test(queued.to_vec());
        sim.substrate
            .entities
            .get_mut(building)
            .unwrap()
            .mark_live_contact_with(id);
        sim.mission_assign_exact(
            id,
            crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Enter),
            0,
        )
        .expect("retail E1 exists");

        crate::sim::mission::enter::mission_enter(
            &mut sim,
            &rules,
            None,
            id,
            FrameEffects::default(),
        );

        let infantry = sim.substrate.entities.get(id).unwrap();
        assert_eq!(infantry.navigation.nav_com(), Some(queued[0]));
        assert_eq!(infantry.navigation.nav_queue(), [queued[1]]);
    }

    /// A Restore the AoE pointer expiry deferred (`pending_destination_setter`)
    /// completes through the Aircraft setter (`0x0041AA80`) like Restore's
    /// own call (`0x004D8F99`): NavCom keeps the saved order and the Fly
    /// heads for it.
    #[test]
    fn a_deferred_aircraft_restore_completes_through_the_aircraft_setter() {
        let Some(rules) = retail_rules() else {
            return;
        };
        let mut sim = Simulation::new();
        crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
        let id = sim
            .spawn_object("ORCA", "Americans", 10, 10, 0, &rules)
            .expect("retail ORCA");
        let saved = NavTargetRef::cell(20, 10);
        let aircraft = sim.substrate.entities.get_mut(id).unwrap();
        aircraft.navigation.set_nav_com_for_test(Some(saved));
        aircraft
            .navigation
            .set_pending_destination_setter_for_test(true);

        sim.complete_pending_order(id, Some(&rules), None, FrameEffects::default());

        let aircraft = sim.substrate.entities.get(id).unwrap();
        assert!(!aircraft.navigation.pending_destination_setter());
        assert_eq!(aircraft.navigation.nav_com(), Some(saved));
        let fly = aircraft
            .locomotor
            .as_ref()
            .and_then(|locomotor| locomotor.fly_runtime())
            .expect("Fly runtime");
        assert!(fly.moving());
        let destination = fly.destination();
        assert_eq!((destination.x / 256, destination.y / 256), (20, 10));
    }

    /// FootClass::AI's pending entry calls vt+0x480 on the waiting Foot
    /// (`0x0070D849`), so an aircraft that waited for a `UnitRepair=` depot
    /// takes the Aircraft setter (`0x0041AA80`) toward it once the depot
    /// answers CAN_LOAD.
    #[test]
    fn an_aircraft_pending_depot_entry_takes_the_aircraft_setter() {
        let Some(rules) = retail_rules() else {
            return;
        };
        let mut sim = Simulation::new();
        crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
        let depot = sim
            .spawn_object("GADEPT", "Americans", 20, 20, 0, &rules)
            .expect("retail GADEPT");
        let id = sim
            .spawn_object("ORCA", "Americans", 10, 10, 0, &rules)
            .expect("retail ORCA");
        crate::sim::docking::building_dock::set_pending_entry(&mut sim, id, Some(depot));

        crate::sim::docking::building_dock::try_pending_entry(
            &mut sim,
            &rules,
            id,
            FrameEffects::default(),
        );

        let aircraft = sim.substrate.entities.get(id).unwrap();
        assert_eq!(aircraft.pending_entry(), None);
        assert_eq!(
            aircraft.navigation.nav_com(),
            Some(NavTargetRef::Building { id: depot })
        );
    }

    /// Every retail `[VehicleTypes]` and `[InfantryTypes]` entry, spawned
    /// through production, reaches its class setter (Unit `0x741970`,
    /// Infantry `0x51AA40`) rather than the NavCom-only fallback, except the
    /// four without rules data, which construct no locomotor
    /// (`should_construct_locomotor`'s zero-speed gate): `DeathDummy` reads
    /// only `Primary=`, and `YDUM`, `VISC_LRG` and `VISC_SML` have no section.
    /// No map is installed: the receiver tests read the object alone, and
    /// without one a naval type is not refused a land cell.
    #[test]
    fn every_retail_unit_and_infantry_type_reaches_its_class_setter() {
        let Some(rules) = retail_rules() else {
            return;
        };
        let mut unspawned = Vec::new();
        let mut without_locomotor = Vec::new();
        let mut unrepresented = Vec::new();
        for type_id in rules.vehicle_ids.iter().chain(&rules.infantry_ids) {
            let mut sim = Simulation::new();
            let Some(id) = sim.spawn_object(type_id, "Americans", 10, 10, 64, &rules) else {
                unspawned.push(type_id.as_str());
                continue;
            };
            let actor = sim.substrate.entities.get(id).expect("spawned");
            let Some(locomotor) = actor.locomotor.as_ref() else {
                without_locomotor.push(type_id.as_str());
                continue;
            };
            let represented = match actor.category() {
                crate::map::entities::EntityCategory::Unit => {
                    sim.unit_setter_receiver(id, Some(&rules))
                }
                crate::map::entities::EntityCategory::Infantry => {
                    sim.infantry_setter_receiver(id, &rules)
                }
                _ => false,
            };
            if !represented {
                unrepresented.push(format!(
                    "{type_id} {:?} {:?}",
                    actor.category(),
                    locomotor.active_kind()
                ));
            }
        }
        assert!(
            rules.vehicle_ids.len() + rules.infantry_ids.len() > 100,
            "the retail rosters were read"
        );
        assert_eq!(unspawned, Vec::<&str>::new(), "types that did not spawn");
        assert_eq!(
            without_locomotor,
            ["DeathDummy", "YDUM", "VISC_LRG", "VISC_SML"]
        );
        assert_eq!(
            unrepresented,
            Vec::<String>::new(),
            "types without a class setter"
        );
    }

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
