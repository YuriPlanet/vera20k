//! Unlimbo / Reveal: the placement transaction that puts a limbo object on
//! the map, its coordinate and barrel elevation, and the building spotlight,
//! AI-repair and owner-discovery work done at Unlimbo.

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::house_tracking::HouseTracking;
use crate::sim::houses::Houses;
use crate::sim::intern::StringInterner;
use crate::sim::scenario_session::ScenarioSession;
use crate::sim::world::{ObjectSubstrate, Simulation};
use crate::util::lepton::{BRIDGE_DECK_HEIGHT_LEPTONS, LEPTONS_PER_LEVEL};

#[cfg(any(test, feature = "test-support"))]
use super::LifecycleOutput;
#[cfg(any(test, feature = "test-support"))]
use super::LifecycleTestEvent;
use super::{
    PlacementEvidence, RevealFailure, RevealOutcome, RevealPosition, RevealRequest, UninitContext,
};

impl Simulation {
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn trace_lifecycle_for_test(&mut self, event: LifecycleTestEvent) {
        self.lifecycle_test_events.push(event);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn lifecycle_test_events_for_test(&self) -> &[LifecycleTestEvent] {
        &self.lifecycle_test_events
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn clear_lifecycle_test_events_for_test(&mut self) {
        self.lifecycle_test_events.clear();
    }

    /// Let client fixtures consume actual Begin/Abandon facts without running
    /// unrelated Logic. The ordinary frame collects this same channel.
    #[cfg(any(test, feature = "test-support"))]
    pub fn take_lifecycle_outputs_for_test(&mut self) -> Vec<LifecycleOutput> {
        self.lifecycle_outputs.take_for_test()
    }

    /// The Location Z Object Unlimbo 0x5F4EC0 commits (SetLocation at
    /// `0x005F4FA8`), adapted from the level-based placement API. It passes its
    /// input coordinate through the type's virtual +0x6C (`0x005F4F88`):
    /// - BuildingType 0x464A70 keeps the XY and takes the floor there
    ///   (`0x578080`) as the Z, with no deck, whatever the input Z.
    /// - UnitType 0x747EB0 / InfantryType 0x5247D0 clamp that input to the
    ///   exact ground surface, whatever the locomotor, and add no bridge
    ///   offset. Authored bridge placement supplies its deck coordinate before
    ///   that clamp; translate our coarse deck request here. This coarse API
    ///   cannot recover all raw authored inputs (notably Unit input zero on
    ///   negative terrain); the report records that caller residual. See
    ///   docs/research/RAMP_UNIT_HEIGHT_GHIDRA_REPORT.md.
    /// - AircraftType keeps the coordinate (`0x0041CF80`), whose Z
    ///   AircraftClass::Unlimbo 0x414310 set first: a `MissileSpawn=` type
    ///   keeps its input (`0x00414338`), with no floor read; one without
    ///   Techno+3D4 (`0x00414342`) inside the playfield (`0x005785F0`) takes
    ///   the floor (`0x00414361`); any other the floor plus its FlightLevel
    ///   (type virtual +0xBC, `0x00414383`). Native comparison:
    ///   tools/spatial_oracle/aircraft_unlimbo_height.json.
    ///
    /// A falling paradrop keeps its drop's Z, and a `MissileSpawn=` aircraft
    /// the Z its caller staged on its limbo Location (the spawn launch's
    /// coordinate, `spawn_manager::launch_coordinate`). Exact-coordinate input
    /// is retained independently of the legacy coarse level. `None` keeps no exact Z, for:
    /// - a tube owner, whose own state carries the height;
    /// - a `MissileSpawn=` aircraft whose caller staged no exact Z (no
    ///   production caller does);
    /// - an Aircraft whose type the rules lack (fixtures);
    /// - an object revealed without terrain (headless fixtures), except a
    ///   `MissileSpawn=` aircraft, whose Z reads no floor.
    fn unlimbo_z(
        &self,
        stable_id: u64,
        position: RevealPosition,
        mut context: UninitContext<'_>,
    ) -> Option<i32> {
        use crate::sim::movement::ground_pose::ground_surface_z_at;

        let xy = crate::sim::movement::ground_pose::position_world_xy(&position);
        let entity = self.substrate.entities.get(stable_id)?;
        if entity.is_falling_down() {
            // Paradrop's Unlimbo coordinate is the drop coordinate, which
            // SetLocation then commits whole (`0x005F5A50`): the falling
            // object keeps the Z its drop gave it.
            return position
                .exact_z_leptons()
                .or(entity.position.exact_z_leptons());
        }
        if entity.active_tube().is_some() {
            return None;
        }
        let aircraft_type = (entity.category() == EntityCategory::Aircraft)
            .then(|| {
                self.interner.object_type_in_category(
                    entity.type_ref(),
                    entity.category().into(),
                    context.rules(),
                )
            })
            .flatten();
        if aircraft_type.is_some_and(|object| object.missile_spawn) {
            return position
                .exact_z_leptons()
                .or(entity.position.exact_z_leptons());
        }
        let terrain = context.terrain().or(self.resolved_terrain.as_ref())?;
        let ground_z = ground_surface_z_at(xy, false, Some(terrain), None)?;
        match entity.category() {
            EntityCategory::Structure => return Some(ground_z),
            EntityCategory::Aircraft => {
                let object = aircraft_type?;
                return Some(
                    if !entity.is_mission_only()
                        && self.reveal_position_is_in_playfield(position, context.reborrow())
                    {
                        ground_z
                    } else {
                        ground_z
                            .wrapping_add(object.flight_level(context.rules().general.flight_level))
                    },
                );
            }
            EntityCategory::Unit | EntityCategory::Infantry => {}
        }
        let level = terrain
            .native_fixed_cell_index((xy[0] / 256) as i16, (xy[1] / 256) as i16)
            .map_or_else(
                || terrain.shared_cell_dummy().snapshot().level,
                |index| terrain.cells()[index].level as i8,
            );
        let input_z = if let Some(input_z) = position.exact_z_leptons() {
            input_z
        } else if entity.on_bridge()
            && let Some(deck) = terrain
                .corrected_high_bridge_deck_level(((xy[0] / 256) as u16, (xy[1] / 256) as u16))
        {
            i32::from(deck).wrapping_mul(LEPTONS_PER_LEVEL as i32)
        } else if entity.on_bridge() && i32::from(position.z() as i8) == i32::from(level) + 4 {
            // VERA input adapter: retain the ramp remainder that the existing
            // coarse bridge-level API cannot carry. Native clamp remains max.
            ground_z.wrapping_add(BRIDGE_DECK_HEIGHT_LEPTONS)
        } else {
            i32::from(position.z() as i8).wrapping_mul(LEPTONS_PER_LEVEL as i32)
        };
        Some(input_z.max(ground_z))
    }

    /// The object's own Location (`Object+0x9C`), the coordinate a Reveal at the
    /// current position passes on.
    fn current_reveal_position(&self, stable_id: u64) -> Option<RevealPosition> {
        self.substrate
            .entities
            .get(stable_id)
            .map(|entity| entity.position)
    }

    /// Compatibility convenience for already-admitted current-position callers.
    /// It still executes the complete result-bearing Reveal transaction.
    #[cfg(any(test, feature = "test-support"))]
    pub fn reveal(&mut self, stable_id: u64) -> RevealOutcome {
        if self.substrate.anims.contains_key(stable_id) {
            let registered = self.reveal_anim(
                stable_id,
                crate::rules::ruleset::RuleSet::empty_for_test(),
                true,
            );
            return RevealOutcome::Revealed {
                logic_registered: registered,
            };
        }
        if self.substrate.particle_systems.contains_key(stable_id) {
            let registered = self.reveal_particle_system(stable_id);
            return RevealOutcome::Revealed {
                logic_registered: registered,
            };
        }
        let Some(position) = self.current_reveal_position(stable_id) else {
            return RevealOutcome::Failed(RevealFailure::MissingObject);
        };
        self.try_reveal_entity(
            stable_id,
            RevealRequest {
                position,
                placement: PlacementEvidence::MarkSucceeded,
                logic_eligible: true,
            },
        )
    }

    /// [`Self::reveal`] for an entity, with the rules context the type-reading
    /// Unlimbo writers need (Aircraft Techno+3D4 retention).
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn reveal_entity_with_rules(
        &mut self,
        stable_id: u64,
        rules: &RuleSet,
    ) -> RevealOutcome {
        self.reveal_entity_with_context(stable_id, UninitContext::new(rules))
    }

    /// A nested Reveal retains the enclosing frame and receiver authorities.
    pub(crate) fn reveal_entity_with_context(
        &mut self,
        stable_id: u64,
        context: UninitContext<'_>,
    ) -> RevealOutcome {
        let Some(position) = self.current_reveal_position(stable_id) else {
            return RevealOutcome::Failed(RevealFailure::MissingObject);
        };
        self.try_reveal_entity_with_context(
            stable_id,
            RevealRequest {
                position,
                placement: PlacementEvidence::MarkSucceeded,
                logic_eligible: true,
            },
            context,
        )
    }

    /// `TechnoClass::Unlimbo`'s barrel elevation writes (`+0x370`,
    /// `0x006F6DC3`, `0x006F6DF5`), aimed by the object's `FireAngle=`, behind
    /// the alive gate that also guards its Added_To_Game (`0x006F6D04`).
    /// The parent retains that one pre-callback admission through its suffix;
    /// this writer only checks that the admitted object still exists.
    pub(crate) fn unlimbo_barrel_elevation(
        interner: &StringInterner,
        session: &ScenarioSession,
        substrate: &mut ObjectSubstrate,
        stable_id: u64,
        rules: &RuleSet,
    ) {
        let Some(fire_angle) = substrate
            .entities
            .get(stable_id)
            .and_then(|entity| {
                interner.object_type_in_category(entity.type_ref(), entity.category().into(), rules)
            })
            .map(|object| object.fire_angle)
        else {
            return;
        };
        let frame = session.binary_frame();
        if let Some(entity) = substrate.entities.get_mut(stable_id) {
            entity.unlimbo_barrel_elevation(fire_angle, frame);
        }
    }

    /// ObjectClass::Reveal: clear limbo for the attempt, commit coordinates,
    /// Mark(PUT), expose display, then append eligible LogicClass membership.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn try_reveal_entity(
        &mut self,
        stable_id: u64,
        request: RevealRequest,
    ) -> RevealOutcome {
        self.try_reveal_entity_with_context(stable_id, request, UninitContext::default())
    }

    /// A nested receiver Reveal uses the same map authority as its enclosing
    /// destruction transaction. SellBuilding @ 0x00458060 still reaches the
    /// global MapClass membership writer in Techno Unlimbo @ 0x006F6CC0..6CFE.
    /// Source: active gamemd.exe call and instruction bodies.
    pub(crate) fn try_reveal_entity_with_context(
        &mut self,
        stable_id: u64,
        request: RevealRequest,
        mut context: UninitContext<'_>,
    ) -> RevealOutcome {
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return RevealOutcome::Failed(RevealFailure::MissingObject);
        };
        if !entity.lifecycle.in_limbo {
            return RevealOutcome::AlreadyRevealed;
        }
        if entity.lifecycle.cell_marked || request.placement == PlacementEvidence::RejectedEarly {
            return RevealOutcome::Failed(RevealFailure::RejectedEarly);
        }

        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            entity.lifecycle.in_limbo = false;
        }
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::RevealLimboCleared);

        // Foot4D9C60 adjusts query-local height/list pointers. OnBridge and
        // the caller's requested XYZ remain independent through Unlimbo.
        let position = request.position;
        let exact_z = self.unlimbo_z(stable_id, position, context.reborrow());
        // RESIDUAL: `ObjectClass::Unlimbo` sets this Location through
        // SetLocation (vt+0x1B4 at 0x005F4FA8, before its Mark(1) at
        // 0x005F4FB4), which for a Foot also runs the OpenTopped rider tail.
        // VERA copies the parts, keeping no exact Z for a tube owner
        // (`unlimbo_z`), so it cannot go through `foot_set_location`
        // yet. Trigger: a loaded OpenTopped transport revealed away from its
        // riders. Frequency: none known in retail. Effect: the riders keep
        // their old Location.
        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            entity
                .position
                .copy_from(crate::sim::components::Position::new(
                    position.rx(),
                    position.ry(),
                    position.z(),
                    exact_z,
                    position.sub_x(),
                    position.sub_y(),
                ));
        }
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::RevealCoordinatesCommitted);

        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::MarkPut);
        if request.placement == PlacementEvidence::MarkFailed {
            if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
                entity.lifecycle.in_limbo = true;
            }
            return RevealOutcome::Failed(RevealFailure::MarkFailed);
        }

        if !self.mark_entity_put(stable_id, context.reborrow()) {
            if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
                entity.lifecycle.in_limbo = true;
            }
            return RevealOutcome::Failed(RevealFailure::MarkFailed);
        }
        // Techno6F6CB1 calls Object Unlimbo, and its failed return6F6CB8
        // precedes the +3D5 establishment at6F6CFE. Failed Mark retains the
        // prior byte. Headless fixtures have no MapClass authority and retain
        // the constructor default through this existing writer.
        self.establish_entity_playfield_membership_on_unlimbo(stable_id, context.reborrow());
        // Techno6F6D04/D0C tests +90 once before the synchronous sight
        // receiver. Original6F6D27..6EDE does not recheck it after callbacks;
        // carry this admission while each owner retains object-existence safety.
        let techno_alive_on_entry = self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| entity.lifecycle.object_alive);
        //6F6D27/6F6D61: source sight and Cell ground-frame border run
        //synchronously before Added_To_Game and the ordinary discovery tail.
        if techno_alive_on_entry {
            self.reveal_object_sight_on_unlimbo(stable_id, context.reborrow());
        }
        // `TechnoClass::Unlimbo` calls Added_To_Game once placement succeeded
        // (`0x006F6D8F`), behind its alive gate (`0x006F6D04`: a dead Techno
        // returns success before it).
        if techno_alive_on_entry {
            #[cfg(feature = "phobos")]
            self.phobos_tracking_before_unlimbo(stable_id);
            self.house_added_to_game(stable_id, context.rules());
        }
        // Techno6F6DAA: the admitted successful Techno snaps
        // the body to this call's direction, before barrel elevation and idle.
        // Its original6F6CA0 body is retained in the shared Unlimbo evidence.
        if techno_alive_on_entry
            && let Some(facing) = context.requested_facing
            && let Some(entity) = self.substrate.entities.get_mut(stable_id)
        {
            entity
                .body_facing
                .snap(u16::from(facing) << 8, self.session.binary_frame());
            #[cfg(any(test, feature = "test-support"))]
            self.trace_lifecycle_for_test(LifecycleTestEvent::UnlimboBodyFacingSnapped);
        }
        // The barrel elevation writes follow the body snap (`0x006F6DC3`).
        if techno_alive_on_entry {
            Self::unlimbo_barrel_elevation(
                &self.interner,
                &self.session,
                &mut self.substrate,
                stable_id,
                context.rules(),
            );
        }
        // TechnoClass::Unlimbo 0x006F6E2A..0x006F6E4F: Enter_Idle_Mode(1, 1),
        // Ready_To_Commence and Commence, ahead of its second mode-one query
        // and of the owner discovery below (Foot 0x004D722F, Building
        // 0x00440E5C), which observes the mission it left: a Foot's committed
        // idle mission, or a placed building's queued Construction. An
        // upgrade's Unlimbo attaches its type to the host and returns
        // (0x00440A1C..0x00440A2F) before TechnoClass::Unlimbo.
        if techno_alive_on_entry {
            #[cfg(any(test, feature = "test-support"))]
            if context.requested_facing.is_some() {
                self.trace_lifecycle_for_test(LifecycleTestEvent::UnlimboIdleMode);
            }
            super::techno_unlimbo_idle_mode(
                self,
                stable_id,
                context.rules(),
                context.registry(),
                context.effects(),
            );
        }
        // TechnoUnlimbo6F6E65..AD runs this second mode-one query only
        // after successful Object Mark and the +90 alive gate. A failed Mark
        // must retain history. This precedes Foot4D722F's owner observation.
        // The dead Techno arm still returns success to Foot4D7184, so only
        // this writer is gated: ForceSlope, owner discovery and Sight follow.
        if techno_alive_on_entry
            && self.entity_playfield_membership_mode_one(
                stable_id,
                context.terrain().or(self.resolved_terrain.as_ref()),
            ) == Some(false)
            && let Some(entity) = self.substrate.entities.get_mut(stable_id)
        {
            entity.seldom.discovery.clear_current_discovery();
        }
        // Techno6F6ED2/6F6EDE publishes its current threat before the Foot
        // tail. Failed Mark and the dead successful Reveal arm publish none.
        if techno_alive_on_entry {
            self.spatial_threat_after_unlimbo(stable_id, context.rules(), context.terrain());
        }
        // Ares 3.0 TechnoClass_Put_1/_2 (1004EDE0, hooks 0x6F6D0E and
        // 0x6F6F20): every successful TechnoClass::Unlimbo, the dead arm
        // included, gives a `HasSpotlight=` Techno its BuildingLight here;
        // BuildingClass_Put_DontSpawnSpotlight (1004EA10, hook 0x441163)
        // skips the building's own construction below.
        if context.rules.ares() {
            self.construct_spotlight(stable_id, context.rules());
        }
        // Ares 3.0 FootClass_Put_InitialPayload (100491B0, hook 0x4D718C):
        // FootClass::Unlimbo, once TechnoClass::Unlimbo has answered true
        // (the dead arm too), makes a Unit's or Aircraft's InitialPayload.
        if context.rules.ares()
            && self
                .substrate
                .entities
                .get(stable_id)
                .is_some_and(|entity| {
                    matches!(
                        entity.category(),
                        EntityCategory::Unit | EntityCategory::Aircraft
                    )
                })
        {
            self.create_ares_initial_payload(stable_id, context.rules(), context.effects());
        }
        // FootClass::Unlimbo @ 0x004D7170 dispatches active Drive/Ship
        // Force_Slope at 0x004D71A9 only after TechnoClass placement succeeds.
        // This precedes display/Logic exposure and must not run on either
        // failed placement path above.
        let reveal_slope = self.substrate.entities.get(stable_id).and_then(|entity| {
            context
                .terrain()
                .or(self.resolved_terrain.as_ref())?
                .cell(entity.position.rx(), entity.position.ry())
                .map(|cell| cell.slope_type)
        });
        if let Some(sampled_slope) = reveal_slope
            && let Some(entity) = self.substrate.entities.get_mut(stable_id)
        {
            crate::sim::movement::slope_transition::snap_after_successful_unlimbo(
                entity,
                sampled_slope,
                self.session.binary_frame(),
            );
        }
        // Ares3.0 FootPut hook4D7221 ->1004E780 follows successful
        // locomotor Put and precedes owner discovery4D722F. Declaration
        // membership alone requests the owner's existing House+1FC pass.
        let rules = context.rules();
        if context.rules.ares()
            && let Some(owner) = self.substrate.entities.get(stable_id).and_then(|entity| {
                (entity.category() != EntityCategory::Structure
                    && rules.is_ares_prerequisite_alternate(
                        entity.category().into(),
                        self.interner.resolve(entity.type_ref()),
                    ))
                .then_some(entity.owner())
            })
        {
            self.houses.request_house_tech_tree_recheck(owner);
        }
        // ObjectUnlimbo5F4FB4 Mark/CellPUT has already run. Foot4D722F calls
        // +198(owner) next; only afterward Infantry51E0EF clears +41B for
        // exactly Sight=0. Never move these producers before the Mark call.
        if let Some(entity) = self.substrate.entities.get(stable_id)
            && entity.category() != EntityCategory::Structure
        {
            self.discover_techno(stable_id, entity.owner(), context.reborrow());
        }
        let high_flight = self.foot_neighbors_after_unlimbo(stable_id, context.rules);
        // Foot4D72B2/+54 requires high flight, then Type ConsideredAircraft
        // (+D96) admits AirTrackerAdd4D72DB, whatever the locomotor. Mark
        // itself never adds anything. A type the rules lack (fixtures) reads
        // +D96 as its default, Aircraft only.
        if high_flight
            && self.substrate.entities.get(stable_id).is_some_and(|e| {
                self.interner
                    .object_type_in_category(e.type_ref(), e.category().into(), context.rules())
                    .map_or(e.category() == EntityCategory::Aircraft, |o| {
                        o.considered_aircraft
                    })
            })
        {
            Self::aircraft_tracker_add(&self.session, &mut self.substrate, stable_id);
        }
        // FootUnlimbo4D72EA..4D72F4 copies Type+2F0 into Foot+530 after
        // neighbors and optional AirTracker, including the dead-Techno success
        // arm. Both early and Mark refusals above must retain the prior value.
        if let Some(entity) = self.substrate.entities.get_mut(stable_id)
            && matches!(
                entity.category(),
                EntityCategory::Infantry | EntityCategory::Unit | EntityCategory::Aircraft
            )
            && let Some(object) = self.interner.object_type_in_category(
                entity.type_ref(),
                entity.category().into(),
                context.rules(),
            )
        {
            entity
                .navigation
                .retain_threat_avoidance_after_unlimbo(object.threat_avoidance_coefficient);
        }
        // FootUnlimbo4D72FA..4D7318 ends on the sensor circle: a type with
        // a sensor radius (Type+5F0, the one gate) dispatches vt+0x4E8
        // AddSensorsAt4DE7B0 with the null cell, so the circle centres on
        // current GetCoords. Every successful Foot Unlimbo reaches it, the
        // dead-Techno arm included; no caller adds it again.
        // RESIDUAL: a first Unlimbo runs this before its caller installs the
        // cloak runtime, so the unit's own vt+0x420 callback cannot start a
        // cloak here as native's can. Trigger: a new Cloakable sensor unit
        // put inside its house's CloakGenerator field (no stock building has
        // one). Effect: it cloaks on its first cloak tick instead.
        // RESIDUAL (pre-existing in every AddSensorsAt caller): its building
        // arm (`0x004DE8DE..0x004DE909`), which sets byte +0x80 on a
        // building not owned by the local player in each stamped cell, has
        // no port; the field is not identified.
        self.add_unit_sensors_at_current(stable_id, context.rules(), context.registry());
        // Unit737BBE..737BD2 / Aircraft414403..414417 snap Secondary after
        // Foot success, including dead-Techno success. The Techno body snap
        // above has its own alive gate and precedes idle. Diagnostic callers
        // without a direction retain the existing facing seam.
        if let Some(facing) = context.requested_facing
            && let Some(entity) = self.substrate.entities.get_mut(stable_id)
            && matches!(
                entity.category(),
                EntityCategory::Unit | EntityCategory::Aircraft
            )
            && let Some(barrel) = entity.barrel_facing.as_mut()
        {
            barrel.snap(u16::from(facing) << 8, self.session.binary_frame());
        }
        // Unit737BF5..737C75 resets the SAME +F8 StageClass after Foot
        // success. E18/E19 are Unit-section booleans, not General references.
        // The existing owner preserves FC/110; +104 is native stack padding.
        // Original execution/RNG: anytown_damage/unit_unlimbo stage controls.
        if self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| entity.category() == EntityCategory::Unit)
        {
            let object = self.substrate.entities.get(stable_id).and_then(|entity| {
                self.interner.object_type_in_category(
                    entity.type_ref(),
                    entity.category().into(),
                    context.rules(),
                )
            });
            let visceroid =
                object.is_some_and(|object| object.small_visceroid || object.large_visceroid);
            // Firestorm 2.0 `UnitClass::Unlimbo @ 0x0064F790` also starts a
            // Tiberian Sun Limpet Drone's Stage at 0, rate 1, after the same
            // visceroid test (`0x0064F808`, `0x0064F83D..0x0064F85E`); its
            // body draws that Stage (`sim::limpet::wrap_drone_stage`).
            let drone = object.is_some_and(|object| object.is_limpet_drone);
            let value = if visceroid {
                self.scenario_rng.next_range_i32_inclusive(0, 29)
            } else {
                0
            };
            if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
                entity.restart_native_stage(
                    value,
                    self.session.binary_frame() as i32,
                    i32::from(visceroid || drone),
                );
            }
        }
        // Aircraft4143A8 follows successful Foot Unlimbo, including the dead
        // Techno success arm. Failed placement above must not promote +3D4.
        // RESIDUAL: the class Unlimbo tails also write what VERA does not
        // write here.
        // - Concrete coordinate/height Unlimbo supplies a direction to this
        //   shared lifecycle owner. Low-level diagnostic Reveal has no
        //   direction argument and retains the existing facing seam.
        // - The Aircraft +0x6C9 latch (`0x004143F2..0x004143FC`), set when a
        //   first passenger rides at Unlimbo. Dormant: carriers take their
        //   passengers after Unlimbo, and `superweapon::paradrop` records why
        //   VERA keeps no latch for the paradrop's own writes (`0x0065E7B8`,
        //   `0x0065DCE9`).
        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            let type_id = self.interner.resolve(entity.type_ref());
            entity.retain_aircraft_unlimbo_control(context.rules(), type_id);
            if entity.category() == EntityCategory::Aircraft
                && let Some(object) = context.rules().object_in_category(
                    crate::rules::object_type::ObjectCategory::Aircraft,
                    type_id,
                )
            {
                let height = crate::sim::movement::air_movement::current_fly_height(
                    entity,
                    context.terrain().or(self.resolved_terrain.as_ref()),
                );
                entity.finish_aircraft_unlimbo(
                    height,
                    object.flight_level(context.rules().general.flight_level),
                    self.session.binary_frame() as i32,
                );
            }
        }
        if let Some(entity) = self.substrate.entities.get_mut(stable_id)
            && entity.category() == EntityCategory::Infantry
            && entity.seldom.sight.is_zero()
        {
            entity.seldom.discovery.clear_current_discovery();
        }
        // Then `InfantryClass::Unlimbo` marks the man's sub-cell through his
        // vt+0xF0 (`0x005217C0`) unless he was placed more than a deck above
        // the ground (`0x0051E0F6..0x0051E10E`). His Mark never writes it.
        if let Some((owner, coord)) = self
            .substrate
            .entities
            .get(stable_id)
            .filter(|entity| entity.category() == EntityCategory::Infantry)
            .map(|entity| {
                (
                    entity.owner(),
                    crate::sim::movement::ground_pose::position_world_coord(&entity.position),
                )
            })
        {
            let terrain = context.terrain().or(self.resolved_terrain.as_ref());
            let deck = crate::sim::movement::ground_pose::ground_surface_z_at(
                [coord.x, coord.y],
                true,
                terrain,
                None,
            )
            .unwrap_or_else(|| coord.z.wrapping_add(BRIDGE_DECK_HEIGHT_LEPTONS));
            if coord.z <= deck {
                crate::sim::movement::walk_head::raw_at(
                    &mut self.substrate.raw_cell_occupation,
                    owner,
                    coord,
                    true,
                    terrain,
                    self.path_grid.as_deref(),
                );
                #[cfg(any(test, feature = "test-support"))]
                self.trace_lifecycle_for_test(LifecycleTestEvent::RawOccupationMarked);
            }
        }
        if !self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| entity.lifecycle.object_alive)
        {
            return RevealOutcome::Revealed {
                logic_registered: false,
            };
        }
        Self::mark_ai_repairable_at_unlimbo(
            &self.session,
            &mut self.substrate,
            &self.houses,
            stable_id,
        );
        // BuildingClass::Unlimbo440B81..440C03 freezes an already-fogged
        // foundation before exposing the new building. Its false visibility
        // argument prevents newly constructed enemy art leaking into memory.
        if self.session.game_options().fog_of_war
            && self
                .substrate
                .entities
                .get(stable_id)
                .is_some_and(|entity| entity.category() == EntityCategory::Structure)
        {
            for &viewer in self.fog_viewers().iter() {
                self.freeze_building_in_fog(viewer, stable_id, false, context.reborrow());
            }
        }
        // Ares 3.0 BuildingClass_Put_FirestormWall (10014A70, hook
        // 0x440D01) connects a placed firestorm wall section to its
        // neighbours (`UpdateFirewallLinks`).
        if context.rules.ares()
            && self
                .substrate
                .entities
                .get(stable_id)
                .is_some_and(|entity| entity.category() == EntityCategory::Structure)
        {
            self.ares_update_firewall_links(
                stable_id,
                context.rules(),
                context.registry(),
                context.effects(),
            );
        }
        // Building440D07: recompute from the PRE-append House+68. Failed
        // Mark and the earlier dead arms never reach it.
        if let Some(owner) = self
            .substrate
            .entities
            .get(stable_id)
            .filter(|entity| entity.category() == EntityCategory::Structure)
            .map(|entity| entity.owner())
        {
            self.recalculate_house_base_geometry(owner, context.rules());
        }
        // Building440D13: IncrementFactoryCount right after it.
        HouseTracking::register_factory(&mut self.substrate, &mut self.houses, stable_id);
        // Building440D1E, after IncrementFactoryCount and before +544.
        if let Some(owner) = self
            .substrate
            .entities
            .get(stable_id)
            .filter(|entity| entity.category() == EntityCategory::Structure)
            .map(|entity| entity.owner())
        {
            self.houses.request_house_tech_tree_recheck(owner);
        }
        self.append_live_build_const(stable_id);
        Self::append_house_base_building(&self.substrate, &mut self.houses, stable_id);
        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            // Building440D2D, after native House building registration.
            entity.reset_building_health_sample_at_unlimbo();
        }
        // Residual (invisible): native marks at 0x00440C34, before
        // 0x00440D07..0x00440D2D above; only House AI reads the marks.
        self.mark_building_base_reservation_with_arg(stable_id, false, context.reborrow());
        self.light_building_at_unlimbo(stable_id, context.reborrow());
        // Residual: 0x00440E02..0x00440E57. A LaserFencePost= building, in
        // the world and not in limbo outside the placement scope
        // (0x00A8E7AC), extends fence sections toward the posts in its four
        // directions (0x00452DC0 with 0, 2, 4, 6) and reconnects
        // (0x004533A0(1)). VERA has no fence sections: a mod's posts stand
        // without a fence between them. No retail type is LaserFencePost=.
        self.discover_building_owner_after_unlimbo(stable_id, context.reborrow());
        // 0x00440E99..0x00440EB5: a type with NaturalParticleSystem=
        // (Type+0x764) and no system yet (+0x30C) builds one. No retail
        // BuildingType sets the key.
        if self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| {
                entity.category() == EntityCategory::Structure
                    && entity.building.natural_particle_system.system().is_none()
            })
        {
            self.construct_natural_particle_system(stable_id, context.rules());
        }
        // While Ares plays, BuildingClass_Put_DontSpawnSpotlight (1004EA10,
        // hook 0x441163) skips this: TechnoClass's Put hook built the light.
        if !context.rules.ares() {
            self.construct_spotlight(stable_id, context.rules());
        }
        // Unlimbo44119C clears this object's retained primary before
        //448070 asks the owning house for another live primary.
        self.production.clear_primary_factory(stable_id);
        crate::sim::production::initialize_factory_primary(self, context.rules(), stable_id);
        //440E76's virtual discovery precedes the successful BasePlan
        // tail at44159D..4415B3, including a reached first opening.
        self.fill_base_plan_from_successful_building_unlimbo(stable_id);
        // `ObjectClass::Unlimbo @ 0x005F4EC0` enters Display (`0x005F4FE2`),
        // Logic (`0x005F5040`) and its AlphaShape (`0x005F5045..0x005F5100`)
        // before the Building tail above, whose discovery calls
        // (`0x00440D61`, `0x00440E76`) can raise Tag event 4. A building an
        // action destroys there leaves all three through its own Limbo, so
        // entering them here would put a dead object back. Native rereads
        // alive `+0x90` and limbo `+0x81` only at `0x00440E1B..0x00440E2D`,
        // for the wall links; the rest of the tail, 4415D8 below included,
        // runs and Unlimbo returns 1 (`0x004415E0`). No Foot callback runs
        // between the alive gate above and this point.
        // Residual (invisible): the dead object's disabled AlphaShape, drawn
        // until the next purge, is not constructed.
        let still_placed = self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| entity.lifecycle.object_alive && !entity.lifecycle.in_limbo);
        let mut logic_registered = false;
        if still_placed {
            Self::submit_entity_display(
                &self.interner,
                &mut self.substrate,
                &self.resolved_terrain,
                stable_id,
                context.rules,
                context.terrain(),
            );
            #[cfg(any(test, feature = "test-support"))]
            self.trace_lifecycle_for_test(LifecycleTestEvent::RevealDisplayBoundary);

            if request.logic_eligible {
                logic_registered = self.register_logic_object(stable_id);
            }
            // Unlimbo's AlphaShapeClass block follows the Logic registration.
            self.unlimbo_alpha_shape(
                stable_id,
                crate::sim::world::substrate::ObjectKind::Entity,
                context.rules(),
            );
        }
        // Building4415D8: successful normal Unlimbo after opening/base-plan.
        // The early dead/failed paths do not reach this call.
        let rules = context.rules();
        if let Some((owner, category)) = self
            .substrate
            .entities
            .get(stable_id)
            .filter(|entity| entity.category() == EntityCategory::Structure)
            .and_then(|entity| {
                let object = self.entity_object_type(entity, rules)?;
                Some((
                    entity.owner(),
                    crate::sim::production::factory_category_for_building(object)?,
                ))
            })
        {
            crate::sim::production::update_factory_queue(
                self,
                rules,
                owner,
                category,
                context.effects(),
            );
        }
        RevealOutcome::Revealed { logic_registered }
    }

    /// `BuildingClass::Unlimbo 0x00440B4F..0x00440B7A`, after the Techno
    /// Unlimbo and its alive gate: outside a campaign, a building of a house
    /// no human controls whose house type is not `MultiplayPassive=` becomes
    /// AI-repairable (`+0x6CB`), whatever its map line said.
    fn mark_ai_repairable_at_unlimbo(
        session: &ScenarioSession,
        substrate: &mut ObjectSubstrate,
        houses: &Houses,
        stable_id: u64,
    ) {
        let Some(entity) = substrate.entities.get(stable_id) else {
            return;
        };
        let policy = session.rule_policy();
        if entity.category() == EntityCategory::Structure
            && policy.uses_managed_ai_base()
            && houses.get(&entity.owner()).is_some_and(|house| {
                !house.is_controlled_by_human(policy) && !house.multiplay_passive()
            })
            && let Some(entity) = substrate.entities.get_mut(stable_id)
        {
            entity.seldom.ai_sell_repair.make_repairable();
        }
    }

    /// `BuildingClass::Unlimbo 0x00440D25..0x00440D6C`: only a campaign
    /// building the player has discovered (`+0x41B`), or whose Unlimbo cell's
    /// ground is not open to the player (`Cell+0x12C & 0x10`), is lit here
    /// ([`Self::light_building`]). Any other takes DiscoveredBy(PlayerPtr)
    /// (`0x00440D61`, [`Self::discover_by_player`]) and is lit when it
    /// opens. That call mostly repeats CellPUT's, which tests the
    /// height-projected cell's mapped bit instead; where CellPUT missed the
    /// building, a human-owned one opens and lights here.
    fn light_building_at_unlimbo(&mut self, id: u64, context: UninitContext<'_>) {
        let Some(entity) = self
            .substrate
            .entities
            .get(id)
            .filter(|entity| entity.category() == EntityCategory::Structure)
        else {
            return;
        };
        let (rx, ry) = (entity.position.rx(), entity.position.ry());
        let lit = self.session.uses_local_player_shroud()
            && (entity.seldom.discovery.discovered_by_current_house()
                || !self
                    .session
                    .current_house()
                    .is_some_and(|player| self.fog.is_ground_open(player, rx, ry)));
        if lit {
            self.light_building(id, context.rules());
        } else {
            self.discover_by_player(id, None, context);
        }
    }
}
