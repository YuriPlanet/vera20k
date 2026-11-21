//! Conceal / Limbo: the common Object Conceal transaction, Techno Limbo,
//! factory-building limbo and kill-record closing.

use crate::map::entities::EntityCategory;
#[cfg(any(test, feature = "test-support"))]
use crate::rules::ruleset::RuleSet;
use crate::sim::house_tracking::HouseTracking;
use crate::sim::houses::Houses;
use crate::sim::scenario_session::ScenarioSession;
use crate::sim::world::{ObjectSubstrate, Simulation};

#[cfg(any(test, feature = "test-support"))]
use super::LifecycleTestEvent;
use super::{ConcealOutcome, LifecycleOutput, UninitContext};

impl Simulation {
    /// Compatibility dispatch which keeps AnimClass logic-only and routes every
    /// GameEntity through the complete common Object Conceal transaction.
    #[cfg(any(test, feature = "test-support"))]
    pub fn conceal(&mut self, stable_id: u64) -> ConcealOutcome {
        if self.substrate.anims.contains_key(stable_id) {
            return if self.conceal_anim(stable_id) {
                ConcealOutcome::Concealed
            } else {
                ConcealOutcome::AlreadyConcealed
            };
        }
        if self.substrate.particle_systems.contains_key(stable_id) {
            return if self.conceal_particle_system(stable_id) {
                ConcealOutcome::Concealed
            } else {
                ConcealOutcome::AlreadyConcealed
            };
        }
        self.object_conceal(stable_id)
    }

    /// ObjectClass::Conceal represented order. Conceal does not mutate Alive.
    #[cfg(any(test, feature = "test-support"))]
    pub fn object_conceal(&mut self, stable_id: u64) -> ConcealOutcome {
        self.object_conceal_with_context(stable_id, UninitContext::default())
    }

    /// BuildingDetach44EE7F/44EEC8 temporarily excludes only this building
    /// while the held type queries FindFactory and HouseAbandon runs. Restore
    /// the old byte after synchronous callbacks; no stored producer binding.
    pub(crate) fn with_factory_building_in_limbo<R>(
        &mut self,
        stable_id: u64,
        callback: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let prior = self
            .substrate
            .entities
            .get(stable_id)
            .map(|entity| entity.lifecycle.in_limbo);
        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            entity.lifecycle.in_limbo = true;
        }
        let result = callback(self);
        if let Some(prior) = prior
            && let Some(entity) = self.substrate.entities.get_mut(stable_id)
        {
            entity.lifecycle.in_limbo = prior;
        }
        result
    }

    pub(super) fn object_conceal_with_context(
        &mut self,
        stable_id: u64,
        mut context: UninitContext<'_>,
    ) -> ConcealOutcome {
        let Some((in_limbo, object_alive)) = self
            .substrate
            .entities
            .get(stable_id)
            .map(|entity| (entity.lifecycle.in_limbo, entity.lifecycle.object_alive))
            .or_else(|| {
                self.substrate.voxel_anims.get(stable_id).map(|piece| {
                    let state = piece.lifecycle();
                    (state.in_limbo, state.object_alive)
                })
            })
            .or_else(|| {
                self.substrate
                    .particle_systems
                    .detached_particle_lifecycle(stable_id)
                    .map(|(alive, in_limbo)| (in_limbo, alive))
            })
        else {
            return ConcealOutcome::MissingOrDead;
        };
        #[cfg(not(any(test, feature = "test-support")))]
        let _ = object_alive;
        // gamemd-derived: `ObjectClass::Conceal @ 0x005F4D30` tests InLimbo
        // at `0x005F4D45` and returns before Destroy on the already-limbo path.
        if in_limbo {
            #[cfg(any(test, feature = "test-support"))]
            self.trace_lifecycle_for_test(LifecycleTestEvent::ConcealAlreadyLimboReturn {
                stable_id,
                object_alive,
                resolvable: true,
            });
            return ConcealOutcome::AlreadyConcealed;
        }

        // Its Deselect (vt+0x150, `0x005F4D55`).
        self.object_deselect(stable_id);
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::ConcealDeselected);

        // gamemd-derived: active YR `ObjectClass::Conceal @ 0x005F4D30`
        // enters `ObjectClass::Detach_All(1) @ 0x005F5280` after deselection and
        // before `Mark(REMOVE)`, so the expiry broadcast observes the target
        // alive, resolvable, and still cell-marked.
        #[cfg(any(test, feature = "test-support"))]
        {
            let (object_alive, cell_marked) = self
                .substrate
                .entities
                .get(stable_id)
                .map(|entity| (entity.lifecycle.object_alive, entity.lifecycle.cell_marked))
                .or_else(|| {
                    self.substrate.voxel_anims.get(stable_id).map(|piece| {
                        let state = piece.lifecycle();
                        (state.object_alive, state.cell_marked)
                    })
                })
                .or_else(|| {
                    self.substrate
                        .particle_systems
                        .detached_particle_lifecycle(stable_id)
                        .map(|(alive, _)| (alive, false))
                })
                .unwrap_or((false, false));
            self.trace_lifecycle_for_test(LifecycleTestEvent::ConcealDestroyNotifyBoundary {
                stable_id,
                object_alive,
                cell_marked,
                resolvable: true,
            });
        }
        // BuildingDetach44EBF0 completes both production arms before the
        // common Object expiry broadcast (44EC01..44EF24).
        if self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| entity.category() == EntityCategory::Structure)
        {
            crate::sim::production::detach_building_production(
                self,
                context.rules(),
                stable_id,
                context.effects(),
            );
            //44EED0 repeats OVER_OUT after production, independently of
            //RadioLimbo65AA91's earlier contact pass. Repeated Conceal
            //returns before this BuildingDetach arm.
            crate::sim::radio::broadcast_break(self, stable_id, context.rules(), context.effects());
        }
        // The Foot prelude leaves the object's team before Limbo is set, so
        // the object still takes its idle mode. It skips that inside an
        // object-placement scope (`[0x00A8E7AC]`, `0x004D9736..0x004D9744`).
        if !self.object_placement_scope_active() {
            self.leave_team(stable_id, false, context.rules(), context.effects());
        }
        // This Detach_All(1) (`0x005F4D61`) also visits the concealed object
        // itself, so a live spawner's Limbo runs its SpawnManager's owner arm:
        // docked children UnInit with a zero regen timer, and airborne ones
        // turn kamikaze on the wing's target (`0x006B7100`).
        self.notify_pointer_expired(stable_id, context.reborrow());

        let unmarked = self.unmark_entity_remove(stable_id, context.reborrow())
            || self
                .substrate
                .voxel_anims
                .get_mut(stable_id)
                .is_some_and(|piece| {
                    // Voxel OccupyList74AB40 is null. Generic Mark(UP) still
                    // clears Object+74 before Display and sound concealment.
                    piece.lifecycle_mut().admit_mark_up()
                });
        if unmarked {
            #[cfg(any(test, feature = "test-support"))]
            self.trace_lifecycle_for_test(LifecycleTestEvent::ConcealUnmarked);
        }

        self.substrate.display.remove(stable_id);
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::ConcealDisplayBoundary);
        self.lifecycle_outputs
            .emit(LifecycleOutput::DetachAttachedAnims { stable_id });
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::ConcealAnimBoundary);
        // ObjectConceal5F4D81 hard-stops the AmbientSound handle (+3C) and
        //5F4D89 detaches the custom-sound handle (+50): `object_sound`.
        // They are separate from Foot+544/Anim+1A0/Building+6A0.
        self.object_sound_conceal(stable_id, context.rules());
        let rules = context.rules();
        context.effects().flush_sound(self, rules);
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::ConcealVocBoundary);

        if self.unregister_logic_object(stable_id) {
            #[cfg(any(test, feature = "test-support"))]
            self.trace_lifecycle_for_test(LifecycleTestEvent::ConcealLogicRemoved);
        }

        let dirty_rect_eligible = self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| entity.seldom.removal_facts.dirty_rect_eligible());
        if dirty_rect_eligible {
            self.lifecycle_outputs
                .emit(LifecycleOutput::DirtyTacticalRect { stable_id });
            #[cfg(any(test, feature = "test-support"))]
            self.trace_lifecycle_for_test(LifecycleTestEvent::ConcealDirtyTacticalRectBoundary);
        }
        self.lifecycle_outputs
            .emit(LifecycleOutput::ClearDrawnState { stable_id });
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::ConcealClearDrawnStateBoundary);

        // ObjectConceal5F4E98 invokes vt+0x11C here, before +81 Limbo is set
        // at5F4E9E (the same byte tested by the5F4D45 entry guard).
        // BuildingClass::Limbo @ 0x00445D81 UnInits the light; its storage
        // goes at the drain.
        self.uninit_spotlight_for_parent_limbo(stable_id, context);
        Self::techno_clear_discovery_on_conceal(
            &self.session,
            &mut self.substrate,
            &self.houses,
            stable_id,
        );
        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            entity.lifecycle.in_limbo = true;
        } else if let Some(piece) = self.substrate.voxel_anims.get_mut(stable_id) {
            piece.lifecycle_mut().in_limbo = true;
        } else {
            // Ordinary Particle base fields are null and Mark(REMOVE) has
            // no cell footprint; the same Object5F4D30 body still owns its
            // retained Display removal and independent +81 write.
            self.substrate
                .particle_systems
                .enter_particle_limbo(stable_id);
        }
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::ConcealLimboSet);
        self.lifecycle_outputs
            .emit(LifecycleOutput::ClearRedraw { stable_id });
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::ConcealClearRedrawBoundary);
        ConcealOutcome::Concealed
    }

    /// `TechnoClass::ClearDiscoveryOnConceal @ 0x006F4A40` (vt+0x11C): an
    /// object whose house a human does not control
    /// ([`HouseState::is_controlled_by_human`], `0x0050B730`) forgets that
    /// the current house discovered it (`+0x41B`). `+0x41A` and `+0x41C`
    /// stay. Object Conceal calls it (`0x005F4E98`), and so does a paradrop
    /// plane that took a refused passenger back (`0x00415EC1`).
    ///
    /// [`HouseState::is_controlled_by_human`]:
    /// crate::sim::house_state::HouseState::is_controlled_by_human
    pub(crate) fn techno_clear_discovery_on_conceal(
        session: &ScenarioSession,
        substrate: &mut ObjectSubstrate,
        houses: &Houses,
        stable_id: u64,
    ) {
        let human = substrate
            .entities
            .get(stable_id)
            .and_then(|entity| houses.get(&entity.owner()))
            .map(|house| house.is_controlled_by_human(session.rule_policy()));
        if human == Some(false)
            && let Some(entity) = substrate.entities.get_mut(stable_id)
        {
            entity.seldom.discovery.clear_current_discovery();
        }
    }

    /// TechnoClass Limbo sends synchronous BREAK to every contact before the
    /// common Object Conceal transaction.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn techno_limbo(&mut self, stable_id: u64) -> ConcealOutcome {
        self.techno_limbo_with_context(stable_id, UninitContext::default())
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn techno_limbo_with_rules(
        &mut self,
        stable_id: u64,
        rules: &RuleSet,
    ) -> ConcealOutcome {
        self.techno_limbo_with_context(stable_id, UninitContext::new(rules))
    }

    pub fn techno_limbo_with_context(
        &mut self,
        stable_id: u64,
        mut context: UninitContext<'_>,
    ) -> ConcealOutcome {
        if !self.substrate.entities.contains(stable_id) {
            return ConcealOutcome::MissingOrDead;
        }
        // Building445D8E: Recount on the first Limbo (Building4458CE skips a
        // repeated one), before the base recompute.
        if self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| !entity.lifecycle.in_limbo)
        {
            // BuildingClass::Limbo @ 0x00445880, 0x004458E4..0x00445903: the
            // first step behind the same gates (not in limbo 0x004458C6, has
            // a type 0x004458D4; every entity here has one). Each of the
            // eight damage-fire Anims (+0x5C8) gets the ordinary deferred
            // AnimClass::UnInit @ 0x004255B0 (vt+0xF8) and its slot is
            // nulled; +0x5E8 is not written.
            if self
                .substrate
                .entities
                .get(stable_id)
                .is_some_and(|entity| entity.category() == EntityCategory::Structure)
            {
                self.clear_building_damage_fire_slots(stable_id, context.rules());
            }
            HouseTracking::unregister_factory(&mut self.substrate, &mut self.houses, stable_id);
            // Building445D99 follows Recount and precedes Techno Limbo.
            // The non-factory prerequisite path stops here: its strip
            // cancellation is decided only after this frame's Factory sweep.
            if let Some(entity) = self.substrate.entities.get(stable_id)
                && entity.category() == EntityCategory::Structure
                && let Some(house) = self.houses.get_mut(&entity.owner())
            {
                house.request_tech_recheck();
            }
            // 0x004459AE..0x004459CA (infantry) and the unit arm after it:
            // the building's self-heal counts leave its house on the same
            // first Limbo, each clamped at zero.
            self.remove_house_self_heal(stable_id, context.rules());
        }
        // Building445DA6 precedes Techno Limbo445DDA, including its pointer
        // expiry and InLimbo write. Building4458CE skips it on repeated Limbo;
        // UnInit's earlier expiry may already have removed this list entry.
        if let Some(owner) = self
            .substrate
            .entities
            .get(stable_id)
            .filter(|entity| {
                entity.category() == EntityCategory::Structure && !entity.lifecycle.in_limbo
            })
            .map(|entity| entity.owner())
        {
            self.recalculate_house_base_geometry(owner, context.rules());
        }
        // `InfantryClass::Limbo @ 0x0051DF10`, before FootClass::Limbo and
        // whether or not the man is already in limbo: its locomotor's
        // Stop_Movement_Animation (ILocomotion `+0xAC`, `0x0051DF30`), the
        // water state's constructor sentinel (`+0x6E8 = 2`), not prone
        // (`+0x6DB`) and Doing Ready (`+0x6C4`, stored without Do_Action).
        // A boarded man leaves his transport with these.
        // Ares 3.0 InfantryClass_Remove_Teleport (1005CD50, hook 0x51DF27),
        // first: a `Teleporter=` man's Teleport locomotor forgets its Marked
        // coordinate (`+0x28`).
        if context.rules.ares() {
            let teleporter = self
                .substrate
                .entities
                .get(stable_id)
                .is_some_and(|entity| {
                    entity.category() == EntityCategory::Infantry
                        && self
                            .object_type(entity.type_ref(), context.rules())
                            .is_some_and(|object| object.teleporter)
                });
            if teleporter
                && let Some(runtime) = self
                    .substrate
                    .entities
                    .get_mut(stable_id)
                    .and_then(|entity| entity.locomotor.as_mut())
                    .and_then(|locomotor| locomotor.teleport_runtime_mut())
            {
                runtime.forget_resolved_destination();
            }
        }
        if let Some(entity) = self.substrate.entities.get_mut(stable_id)
            && entity.category() == EntityCategory::Infantry
        {
            if let Some(locomotor) = entity.locomotor.as_mut() {
                locomotor.stop_movement_animation();
            }
            entity.mission_leaf.reset_infantry_water_state();
            if let Some(infantry) = entity.infantry.as_mut() {
                infantry.stand_up();
            }
            entity
                .mission_leaf
                .set_infantry_doing_verified(crate::sim::movement::infantry_action::DO_READY)
                .expect("Ready is in the Doing table");
        }
        // Foot4DB266 reads InLimbo once before its callback-bearing prelude.
        // A repeated call jumps4DB3AF directly to TechnoLimbo; admission
        // belongs to this caller, not the locomotor's literal +9C body.
        let first_foot_limbo = self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| {
                entity.category() != EntityCategory::Structure && !entity.lifecycle.in_limbo
            });
        Self::foot_neighbors_before_limbo(
            &self.substrate,
            &self.resolved_terrain,
            &mut self.overlay_grid,
            stable_id,
        );
        // Foot4DB260 dispatches vt+0x500 on the first Limbo (0x004DB2FB,
        // while A8E7AC is zero) before +9C(0): the same class receiver as
        //FindPath/FireAt. Infantry51DAF0 clears Doing/Walk moving state;
        //Unit/Aircraft4D55C0 calls the active locomotor Stop_Moving. This
        //includes Hover/Jumpjet Script37 returns, before occupation release.
        let rules = context.rules();
        if !self.object_placement_scope_active()
            && self
                .substrate
                .entities
                .get(stable_id)
                .is_some_and(|entity| {
                    entity.category() != EntityCategory::Structure && !entity.lifecycle.in_limbo
                })
            && let Err(cause) =
                self.foot_stop_driver(stable_id, rules, context.registry(), context.effects())
        {
            log::debug!("Foot {stable_id} Limbo Stop_Driver: {cause}");
        }
        // 0x004DB324: only the admitted Foot prelude calls +9C(0). Keep
        // the entry admission across Stop_Driver's synchronous callbacks.
        // Native Hover5171C0 itself has no InLimbo gate and retains its head.
        if first_foot_limbo {
            self.locomotor_mark_all_occupation_bits(stable_id, false, rules.ares_form());
        }
        // FootClass::Limbo (0x004DB260) then Locks the locomotor (+0xB0) on
        // the first Limbo, so a boarded or stored man keeps no Walk
        // destination or head to resume.
        if let Some(entity) = self.substrate.entities.get_mut(stable_id)
            && !entity.lifecycle.in_limbo
            && let Some(locomotor) = entity.locomotor.as_mut()
        {
            locomotor.walk_lock();
        }
        // FootLimbo4DB353 follows the locomotor Lock and precedes the
        // air-tracker/Techno suffix. It detaches any shared Foot sound, with
        // no MoveSound latch gate or write. Unit7440B4, Infantry51DF53 and
        // Aircraft's direct vtable slot all delegate to this one Foot path.
        if self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| {
                entity.category() != EntityCategory::Structure && !entity.lifecycle.in_limbo
            })
        {
            self.sound_events
                .emit(super::SimSoundEvent::ObjectSoundDetached { owner: stable_id });
        }
        // FootLimbo4DB376 passes NullCell: remove around current GetCoords,
        // before tracker/Techno callbacks, only on the admitted first Limbo.
        // An airborne old-only fringe can survive this native removal.
        if first_foot_limbo {
            self.remove_unit_sensors_at_current(stable_id, rules, context.registry());
        }
        self.substrate
            .release_foot_air_tracker_before_limbo(stable_id);
        // Ares Remove1004B140 frees the jammer (UnjamAll) and the PoweredBy
        // source before the attached effects. Only a held source is cleared,
        // so an object without one keeps no Ares part.
        if context.rules.ares() {
            self.remove_radar_jammer(stable_id);
            if let Some(entity) = self.substrate.entities.get_mut(stable_id)
                && entity.seldom.ares.powered_by.is_some()
            {
                entity.seldom.ares.powered_by = None;
            }
            self.limbo_ares_attach_effects(stable_id, context.rules());
        }
        // Phobos `TechnoClass_Limbo_Wrapper` replaces the Limbo entry
        // (vtable + the Foot/Building call sites) and runs its
        // DiscardOn=Entry pass before calling the original body. Its work
        // therefore precedes the hooks inside that body; the Ares extension's
        // relative position is unobserved and kept as the AttachEffect branch
        // had it.
        #[cfg(feature = "phobos")]
        self.limbo_phobos_attach_effects(
            stable_id,
            context.rules(),
            context.registry(),
            context.effects(),
        );
        // Techno Remove6F6AC9 ->1004B140/1004B1A9 flushes even a repeated
        // Limbo, before the base's alive/InLimbo and sight-release branches.
        if context.rules.ares() {
            self.flush_bounty_display(stable_id, context.rules(), context.effects(), true);
        }
        if self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| !entity.lifecycle.in_limbo)
        {
            //6F6B16 ordinary sight release precedes TypeCD1 gap removal at
            //6F6B6A; both precede Object Limbo. Repeated Limbo emits neither.
            // A `RevealToAll=` type takes the branch at `0x006F6AEF..0x006F6B06`
            // instead: ReReveal(0, 0, 1, PlayerPtr) releases its sight for the
            // current house in place of its owner's. Without a current house
            // (headless) there is no PlayerPtr to release for, and the
            // ordinary release keeps its place before the gap removal.
            let rules = context.rules();
            let reveal_to_all = (self.session.execution_topology().is_lockstep()
                || self.session.current_house().is_some())
                && self
                    .substrate
                    .entities
                    .get(stable_id)
                    .and_then(|entity| self.object_type(entity.type_ref(), rules))
                    .is_some_and(|object| object.reveal_to_all);
            if reveal_to_all {
                self.rereveal_sight_to_current_house(stable_id, rules, false);
            } else {
                self.release_object_sight_with_context(stable_id, context.reborrow());
            }
            Self::remove_building_gap(
                &self.interner,
                &mut self.fog,
                &self.house_alliances,
                &mut self.substrate,
                &self.houses,
                &self.resolved_terrain,
                stable_id,
            );
            // Steam gamemd, TechnoLimbo6F6B98..6F6BA0 clears an
            // OpenTopped hull's own rider targets before HouseRemoved6F6BD1.
            // Cargo, each rider's +82 and Logic membership survive this call.
            self.open_topped_passengers_take_target(
                stable_id,
                None,
                context.rules(),
                context.effects(),
            );
            // 6F6BD1: Removed_From_Game, also only on the first Limbo.
            #[cfg(feature = "phobos")]
            self.phobos_tracking_before_limbo(stable_id);
            self.house_removed_from_game(stable_id, context.rules(), context.effects());
            // Ares BuildingClass_Remove_Academy (hook 0x00445905 ->
            // 10023690) precedes them: a marked academy leaves its owner's list.
            if context.rules.ares() {
                self.update_ares_academy_membership(stable_id, false, context.rules());
            }
            // BuildingClass::Limbo 0x00445946..0x00445988: an opened
            // Helipad's docks leave its house's AirportDocks.
            let rules = context.rules();
            if let Some(entity) = self.substrate.entities.get(stable_id)
                && entity.category() == EntityCategory::Structure
                && entity.building_art.actually_placed()
                && let Some(object) = self.object_type(entity.type_ref(), rules)
                && object.helipad
            {
                let owner = entity.owner();
                if let Some(house) = self.houses.get_mut(&owner) {
                    house.limbo_airport_docks(object.number_of_docks);
                }
            }
        }
        //6F6C2A/2F removes the retained contribution and clears+508 before
        //ObjectConceal; its result cannot roll these Techno writes back.
        self.spatial_threat_before_limbo(stable_id, context.rules(), context.terrain());
        // BuildingClass owns this pass before the common TechnoClass Limbo can
        // clear committed type/cell facts or broadcast another expiry callback.
        Self::invalidate_base_plan_from_building_limbo(
            self.session.rule_policy().ai_base(),
            &self.substrate,
            &mut self.houses,
            stable_id,
        );
        // BuildingClass::Limbo @ 0x00445880 keeps its separate array and
        // disguise removals; Foot's sensor receiver ran in its prelude.
        self.remove_building_sensor_before_limbo(stable_id, context.rules(), context.registry());
        // Dead and InLimbo are independent native state. TechnoClass::Limbo
        // still reaches ObjectClass::Conceal for a stored dead object; the
        // latter's InLimbo branch alone decides whether Conceal is a no-op.
        self.clear_building_base_reservation_and_repair(stable_id, context.reborrow());
        // The same TechnoClass::Limbo sound cleanup releases the turret
        // rotation loop's `+0x488` handle before the gattling report handle,
        // and re-arms its latch.
        // An object already in limbo skips both releases: TechnoClass::Limbo
        // jumps from its InLimbo test (`0x006F6AC9..0x006F6ACF`) past them to
        // `0x006F6C93`. The class destructors' own Limbo is such a repeat.
        let already_in_limbo = self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| entity.lifecycle.in_limbo);
        if !already_in_limbo {
            let rules = context.rules();
            self.turret_rotate_sound_limbo(stable_id, context.effects(), rules);
            // TechnoClass::Limbo releases the gattling loop and clears the
            // report latch (`0x006F6C6B`, `0x006F6C76`) ahead of its radio pass.
            self.gattling_limbo(stable_id, context.reborrow());
        }
        // RadioLimbo65AA80..65AA99 tests InLimbo before Broadcast(3).
        // CargoAdd4733B0 repeats virtual Limbo after Hospital51A35F's
        // post-conceal HELLO; breaking again loses the repair contact.
        if self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| !entity.lifecycle.in_limbo)
        {
            crate::sim::radio::broadcast_break(self, stable_id, context.rules(), context.effects());
        }
        let outcome = self.object_conceal_with_context(stable_id, context.reborrow());
        // Ares 3.0 UnitClass_Remove (1005D610, hook 0x7440BD), after a Foot
        // Limbo that removed the Unit: its Tank Bunker lets it go
        // (`BuildingClass::ClearBunker @ 0x00459470`).
        if context.rules.ares()
            && outcome == ConcealOutcome::Concealed
            && let Some(bunker) = self
                .substrate
                .entities
                .get(stable_id)
                .filter(|entity| entity.category() == EntityCategory::Unit)
                .and_then(|entity| entity.bunker.link().installed_in())
            && self
                .substrate
                .entities
                .get(bunker)
                .is_some_and(|building| building.category() == EntityCategory::Structure)
        {
            crate::sim::docking::bunker_link::clear_bunker(
                self,
                bunker,
                context.rules(),
                context.effects(),
            );
        }
        // Ares 3.0 BuildingClass_Remove_FirestormWall (10014A70, hook
        // 0x445DF4) after the base Limbo disconnects a firestorm wall
        // section from its neighbours (`UpdateFirewallLinks`).
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
        // Building445DF4..445E14 follows the base Limbo, including the
        // already-limbo branch4458CE. This is one House/category callback,
        // never a global eligibility sweep or a boolean-transition latch.
        let rules = context.rules();
        if let Some((owner, category)) = self
            .substrate
            .entities
            .get(stable_id)
            .filter(|entity| entity.category() == EntityCategory::Structure)
            .and_then(|entity| {
                let object = self.object_type(entity.type_ref(), rules)?;
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
        outcome
    }

    /// Close `stable_id`'s kill record ([`crate::sim::combat::KillRecord`])
    /// at UnInit and at ReceiveDamage's Destroy. It books nothing: the House
    /// loss counts (`+0x5434` units, `+0x5488` buildings) have no writer but
    /// RecordKill `0x00702D40` and RegisterKill `0x00703230`, so an object
    /// removed without either is no loss. Such removals: a zero-health map
    /// building at its first Update (`0x00440072..0x004400F1`), a parasite
    /// that dies leaving its victim (ExitUnit `0x0062A4A0`, PointerExpired
    /// `0x0062A260`) and a dropped Crashable unit (Notice `0x00746100`).
    pub(crate) fn close_kill_record(substrate: &mut ObjectSubstrate, stable_id: u64) {
        if let Some(entity) = substrate.entities.get_mut(stable_id) {
            entity.seldom.kill_record.close();
        }
    }
}
