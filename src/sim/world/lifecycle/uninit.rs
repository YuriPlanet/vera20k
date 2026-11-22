//! UnInit and pending delete: `uninit_with_context`, particle-system
//! retirement, building cargo deletion, `finalize_and_remove_common` and the
//! pending-delete drain.

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::house_tracking::HouseTracking;
use crate::sim::world::substrate::{DeferredDeleteTicket, ObjectKind};
use crate::sim::world::{FrameEffects, Simulation};

#[cfg(any(test, feature = "test-support"))]
use super::LifecycleTestEvent;
use super::{LifecycleOutput, UninitContext};

impl Simulation {
    /// ObjectClass::UnInit represented ordering on the rules an empty INI
    /// reads to.  Physical removal is deferred.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn uninit(&mut self, stable_id: u64) {
        self.uninit_with_context(stable_id, UninitContext::default());
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn uninit_with_rules(&mut self, stable_id: u64, rules: &RuleSet) {
        self.uninit_with_context(stable_id, UninitContext::new(rules));
    }

    pub fn uninit_with_context(&mut self, stable_id: u64, mut context: UninitContext<'_>) {
        if self.classify_object(stable_id) == Some(ObjectKind::Terrain) {
            self.retire_non_entity_object(stable_id, context);
            return;
        }
        let kind = match self.classify_object(stable_id) {
            Some(
                kind @ (ObjectKind::Entity
                | ObjectKind::Spotlight
                | ObjectKind::Smudge
                | ObjectKind::VoxelAnim),
            ) => kind,
            Some(ObjectKind::Particle)
                if self
                    .substrate
                    .particle_systems
                    .detached_particle(stable_id)
                    .is_some() =>
            {
                ObjectKind::Particle
            }
            // A particle still in its system retires with it; the other kinds
            // retire through their own owners, never this UnInit.
            Some(
                ObjectKind::Particle
                | ObjectKind::Anim
                | ObjectKind::ParticleSystem
                | ObjectKind::Terrain
                | ObjectKind::Projectile
                | ObjectKind::Wave,
            )
            | None => return,
        };
        // Ordinary Particle and Smudge instances have no Techno/Foot hooks
        // or passengers. Every represented family joins the one base tail.
        if let Some(entity) = self
            .substrate
            .entities
            .get(stable_id)
            .filter(|_| kind == ObjectKind::Entity)
        {
            // `FootClass::UnInit @ 0x004DE5DD` frees a controller's captives
            // first (a crushed or otherwise removed Yuri or Mastermind).
            let foot = matches!(
                entity.category(),
                EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
            );
            if foot && entity.capture_manager.is_some() {
                self.free_all_captures(
                    stable_id,
                    context.rules(),
                    context.registry(),
                    context.effects(),
                );
            }
            // `0x004DE5F0`: a Magnetron lets go of what it holds.
            if foot
                && self
                    .substrate
                    .entities
                    .get(stable_id)
                    .is_some_and(|entity| entity.lift.target().is_some())
            {
                self.release_locomotor(
                    stable_id,
                    true,
                    context.rules(),
                    context.registry(),
                    context.effects(),
                );
            }
            // `0x004DE604`: then the object leaves its team.
            if foot {
                self.leave_team(stable_id, false, context.rules(), context.effects());
            }

            self.run_represented_uninit_pre_hook(stable_id);
            #[cfg(feature = "phobos")]
            self.phobos_tracking_begin_uninit(stable_id);
            // `ObjectClass::UnInit` kills a Techno's passengers with no attacker
            // (`0x005F660D`).
            self.kill_passengers(stable_id, None, context.reborrow());

            #[cfg(any(test, feature = "test-support"))]
            {
                let (object_alive, cell_marked) = self
                    .substrate
                    .entities
                    .get(stable_id)
                    .map(|entity| (entity.lifecycle.object_alive, entity.lifecycle.cell_marked))
                    .unwrap_or((false, false));
                self.trace_lifecycle_for_test(LifecycleTestEvent::UninitRemovalNotifyBoundary {
                    stable_id,
                    object_alive,
                    cell_marked,
                    resolvable: self.substrate.entities.contains(stable_id),
                });
            }
        }
        // ObjectClass::UnInit's first statement (`0x005F65F3`) defuses a
        // carried bomb silently: sold, crushed, erased.
        Self::bomb_defuse(
            &mut self.substrate,
            &mut self.bombs,
            &mut self.sound_events,
            stable_id,
        );
        self.notify_pointer_expired(stable_id, context.reborrow());

        match kind {
            ObjectKind::Spotlight => {
                self.notify_pointer_expired(stable_id, context);
                self.finish_spotlight_retirement(stable_id);
            }
            ObjectKind::Smudge => {
                // Whole Smudge6B4A50 delegates to Object5F65F0. Its Mark(0)
                // writes no cell data; preserve its existing fused Conceal tail.
                self.notify_pointer_expired(stable_id, context);
                self.smudge_grid.as_mut().unwrap().finish_uninit(stable_id);
            }
            ObjectKind::Particle => {
                let _ = self.object_conceal_with_context(stable_id, context);
                self.substrate
                    .particle_systems
                    .finish_particle_retirement(stable_id);
            }
            ObjectKind::VoxelAnim => {
                // Voxel74A700 inherits ObjectUnInit5F65F0: Limbo while
                // Alive, clear +90, then queue the same retained body.
                let _ = self.object_conceal_with_context(stable_id, context);
                if let Some(piece) = self.substrate.voxel_anims.get_mut(stable_id) {
                    piece.lifecycle_mut().object_alive = false;
                }
            }
            ObjectKind::Entity => {
                let _ = self.techno_limbo_with_context(stable_id, context);
                if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
                    entity.lifecycle.object_alive = false;
                    entity.death.begin();
                }
            }
            ObjectKind::Anim
            | ObjectKind::ParticleSystem
            | ObjectKind::Terrain
            | ObjectKind::Projectile
            | ObjectKind::Wave => unreachable!("{kind:?} {stable_id} never reaches UnInit"),
        }
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::UninitAliveCleared { stable_id });

        // Native append has no duplicate suppression.  The drain collapses all
        // occurrences when this dead object becomes the selected ready entry.
        self.substrate.pending_delete.queue_object(stable_id);
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::PendingDeleteQueued { stable_id });
        #[cfg(feature = "phobos")]
        if kind == ObjectKind::Entity {
            self.phobos_tracking_end_uninit();
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn despawn_entity(&mut self, stable_id: u64) {
        self.uninit(stable_id);
    }

    /// Particle systems stay resolvable until the ordinary common late drain.
    /// Their owned particles have already emptied before this transition.
    /// SystemAI62FDC7..62FDE7 calls ObjectLimbo5F4D30 while Alive is still
    /// set, then clears Alive and queues deletion. Limbo's literal control1
    /// dispatch clears Techno+308/+310 synchronously, before the late destructor.
    pub(crate) fn retire_particle_system(&mut self, stable_id: u64, context: UninitContext<'_>) {
        let ready = self
            .substrate
            .particle_systems
            .get(stable_id)
            .is_some_and(|system| {
                system.alive() && system.done_spawning() && system.particles().is_empty()
            });
        if !ready {
            return;
        }

        if !self
            .substrate
            .particle_systems
            .get(stable_id)
            .unwrap()
            .in_limbo()
        {
            self.notify_pointer_expired(stable_id, context);
            self.conceal_particle_system(stable_id);
        }
        self.substrate.particle_systems.enter_limbo(stable_id);
        self.substrate.pending_delete.queue_object(stable_id);
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::PendingDeleteQueued { stable_id });
    }

    /// A particle system deleted at once rather than retired: its vt+0x20
    /// with 1 (`0x00630230`) runs `~ParticleSystemClass @ 0x0062E070` and
    /// frees the body. The destructor announces the expiry (`0x0062E092`),
    /// Limbos the system (`0x0062E099`), deletes each particle
    /// (`0x0062E0AE..0x0062E15D`) and leaves the system lists, all of which
    /// the common destructor below does, so every listener's link clears and
    /// the system and its particles are gone this frame. The destructor reads
    /// no GameState. A queued ticket would name a freed body, so it goes too.
    pub(crate) fn delete_particle_system_now(
        &mut self,
        stable_id: u64,
        context: UninitContext<'_>,
    ) {
        if !self.substrate.particle_systems.contains_key(stable_id) {
            return;
        }
        self.substrate.pending_delete.remove_object(stable_id);
        // Each particle's own destructor announces its expiry
        // (`0x0062D9C8`); the common destructor below gives it to the glows,
        // the only listener that holds a particle id.
        self.finalize_and_remove_common(stable_id, context, 1);
    }

    /// A queued object's IsReadyToDelete (vt+0x44), by kind. An ID no store
    /// holds is ready.
    fn pending_object_is_ready(&self, stable_id: u64) -> bool {
        let Some(kind) = self.classify_object(stable_id) else {
            return true;
        };
        match kind {
            ObjectKind::Spotlight => self.spotlight_alive(stable_id) == Some(false),
            ObjectKind::Smudge => self
                .smudge_grid
                .as_ref()
                .and_then(|grid| grid.object(stable_id))
                .is_some_and(|smudge| !smudge.object_alive()),
            ObjectKind::Entity => self
                .substrate
                .entities
                .get(stable_id)
                .is_some_and(|entity| !entity.lifecycle.object_alive),
            // Every ordinary Anim retirement Conceals before queueing. Owner
            // expiry425196 only sets19B and leaves Logic live until its own AI;
            // that byte is not ObjectUnInit5F6625's readiness state.
            ObjectKind::Anim => self
                .substrate
                .anims
                .get(stable_id)
                .is_some_and(|anim| !anim.in_logic_vector()),
            // SystemVT7EFB9C+44 -> IsReadyToDelete62FE60: done+F8 and
            // count+CC==0, independently of the ObjectAlive byte.
            ObjectKind::ParticleSystem => self
                .substrate
                .particle_systems
                .get(stable_id)
                .is_some_and(|system| system.done_spawning() && system.particles().is_empty()),
            // Particle+44 inherits Object5F6690: !ObjectAlive (+0x90). Only a
            // detached particle is queued; one still in its system is alive.
            ObjectKind::Particle => self
                .substrate
                .particle_systems
                .detached_particle(stable_id)
                .is_some_and(|(_, alive)| !alive),
            ObjectKind::Terrain => self
                .terrain_objects
                .objects()
                .get(&stable_id)
                .is_some_and(|terrain| !terrain.object_alive() && !terrain.in_logic_vector()),
            ObjectKind::Projectile => self
                .projectiles
                .get(stable_id)
                .is_some_and(|projectile| !projectile.in_logic_vector()),
            ObjectKind::Wave => self
                .waves
                .get(stable_id)
                .is_some_and(|wave| !wave.in_logic_vector()),
            // Voxel inherits Object IsReadyToDelete5F6690: !ObjectAlive.
            ObjectKind::VoxelAnim => self
                .substrate
                .voxel_anims
                .get(stable_id)
                .is_some_and(|piece| !piece.lifecycle().object_alive()),
        }
    }

    /// TemporalClass::Update (0x0071A9AC..0x0071A9DC): true Building Cargo gets
    /// scalar destruction, not RecordKill/UnInit, while ScenarioInit is zero.
    pub(crate) fn scalar_delete_building_cargo(
        &mut self,
        building_id: u64,
        mut context: UninitContext<'_>,
        game_state: i32,
    ) {
        if self.object_placement_scope_active() {
            return;
        }
        loop {
            // CargoClass::RemoveFirstPassenger (0x00473430) pops one head.
            // Its destructor's callbacks finish before71A9D6 rereads the head.
            let passenger = self
                .substrate
                .entities
                .get_mut(building_id)
                .and_then(|entity| entity.passenger_role_mut().cargo_mut())
                .and_then(|cargo| cargo.unload_first())
                .map(|(id, _)| id);
            let Some(passenger) = passenger else {
                break;
            };
            if self.substrate.entities.contains(passenger) {
                self.finalize_and_remove_common(passenger, context.reborrow(), game_state);
            }
        }
    }

    /// Shared physical destructor owner for deferred retirement, direct Cargo
    /// scalar destruction and constructor-complete limbo disposal. This owner's
    /// loss is not booked here. Rules reach synchronous expiry receivers.
    pub(in crate::sim::world) fn finalize_and_remove_common(
        &mut self,
        stable_id: u64,
        mut context: UninitContext<'_>,
        game_state: i32,
    ) {
        let kind = self.classify_object(stable_id);
        if kind == Some(ObjectKind::Spotlight) {
            self.destroy_spotlight(stable_id, context);
            return;
        }
        self.destroy_spotlight_for_parent(stable_id, context.reborrow());
        let foot = self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| entity.category() != EntityCategory::Structure);
        let infantry = self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| entity.category() == EntityCategory::Infantry);
        // Unit7357D6..7357EF, Infantry517DDB..517DEB and Aircraft
        // 4140C3..4140DC ask the existing House CanBuild4F7870 before
        // Team/Slave/Tracking removal. AtLimit(-1) requests the child's
        // current owner's House+1FC tech-tree refresh. The direct scalar
        // paths can arrive without an earlier UnInit.
        let rules = context.rules();
        if (game_state as u8) != 0
            && let Some(owner) = self.substrate.entities.get(stable_id).and_then(|entity| {
                matches!(
                    entity.category(),
                    EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
                )
                .then(|| self.entity_object_type(entity, rules))
                .flatten()
                .filter(|object| {
                    crate::sim::production::can_build(
                        self,
                        rules,
                        entity.owner(),
                        object,
                        false,
                        false,
                    ) == crate::sim::production::CanBuild::AtLimit
                })
                .map(|_| entity.owner())
            })
        {
            self.houses.request_house_tech_tree_recheck(owner);
        }
        // Constructor-complete Unit7357F6..735809, Infantry517DF2..517E05
        // and Aircraft4140EB..4140FE leave their Team before Remove_Tracking.
        // Their gate is the low byte of GameState[A8E9A0], independently of
        // ScenarioStarted[A8ED5C]. Foot4D3597..4D35C8 repeats the same shared
        // RemoveMember6EA870 when a class destructor retained the reference.
        // Source: original gamemd;
        // executed composed action7 cleanup: spatial_oracle/team_creation.
        // `AircraftClass::~AircraftClass @ 0x00414080` kills its passengers
        // with no attacker (`KillPassengers(NULL)`, `0x004140E3..0x004140E6`)
        // after the CanBuild test and before its Team leave. Ordinary UnInit
        // already emptied the cargo; a carrier destroyed without one has not.
        if (game_state as u8) != 0
            && self
                .substrate
                .entities
                .get(stable_id)
                .is_some_and(|entity| entity.category() == EntityCategory::Aircraft)
        {
            self.kill_passengers(stable_id, None, context.reborrow());
        }
        if foot && (game_state as u8) != 0 {
            // Ares3.0's Unit7357F6/Infantry517DF2/Aircraft4140EB hooks
            // run after the existing GameState/type/build-limit gates and
            // before LeaveTeam/RemoveTracking. Every declared alternate
            // requests a pass, even while another copy remains tracked.
            let rules = context.rules();
            if context.rules.ares()
                && let Some(owner) = self.substrate.entities.get(stable_id).and_then(|entity| {
                    rules
                        .is_ares_prerequisite_alternate(
                            entity.category().into(),
                            self.interner.resolve(entity.type_ref()),
                        )
                        .then_some(entity.owner())
                })
            {
                self.houses.request_house_tech_tree_recheck(owner);
            }
            self.leave_team(stable_id, false, context.rules(), context.effects());
        }
        // Infantry destructor517E22 removes its slave node after Team leave
        // and before RemoveTracking517E2E, only under GameState's low byte.
        if infantry && (game_state as u8) != 0 {
            Self::remove_slave(&self.session, &mut self.substrate, stable_id);
        }
        if kind == Some(ObjectKind::ParticleSystem) {
            // SystemDtor62E075/62E092 broadcasts control1 before its
            // delegated Limbo62E099 and before leaving object storage.
            // The ordinary done-empty AI already announced its earlier
            // live Limbo; both callbacks use the same retained-reference
            // owner instead of clearing only its producer after removal.
            self.notify_pointer_expired(stable_id, context.reborrow());
            self.conceal_particle_system(stable_id);
        }
        if kind == Some(ObjectKind::Particle) {
            // ParticleDtor62D9A0: its already-limbo check does no work,
            // then62D9C8 announces again before ObjectDtor5F3B80 unregisters
            // and frees the body. No Particle-specific RNG/timer writes.
            self.notify_pointer_expired(stable_id, context.reborrow());
        }
        if kind == Some(ObjectKind::VoxelAnim) {
            // Voxel destructor749A11 announces expiry before its GameActive
            // Limbo749A86 and before ObjectDtor749B0C releases the body.
            self.notify_pointer_expired(stable_id, context.reborrow());
            if (game_state as u8) != 0 {
                self.object_conceal_with_context(stable_id, context.reborrow());
            }
        }
        if kind == Some(ObjectKind::Smudge) {
            // Scalar Smudge6B4FA0 sends its third removed=true expiry at
            //6B4FC6 before leaving Smudge/Object registries. Place's cells
            // survive this destructor and Building Mark(0).
            self.notify_pointer_expired(stable_id, context.reborrow());
        }
        if kind == Some(ObjectKind::Terrain) {
            // Terrain destructor71B7D6 repeats expiry before leaving its
            // registry. This is later than the live lethal/UnInit callbacks.
            self.notify_pointer_expired(stable_id, context.reborrow());
            // TerrainDtor71B7B0 at71B824..71B840 repeats Limbo while GameActive,
            // even for a never-placed or previously UnInit'd object. Its
            // suffix still queries/recalculates the retained cell; the
            // already-InLimbo prefix performs no unmark or extra expiry.
            if (game_state as u8) != 0 {
                self.limbo_terrain_with_context(stable_id, context.reborrow());
            }
        }
        if kind == Some(ObjectKind::Projectile) {
            // BulletDtor466581 announces expiry before clearing construction
            // fields or leaving its registry, including never-Fired bullets.
            self.notify_pointer_expired(stable_id, context.reborrow());
        }
        if self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| entity.category() == EntityCategory::Structure)
        {
            // Building destructor43BD27 releases the construction SoundClass
            //(+6A0) before the delegated Techno destructor removes tracking.
            self.sound_events
                .emit(super::SimSoundEvent::ObjectSoundReleased { owner: stable_id });
            // The delegated Techno destructor also tears down the turret
            // rotation loop's `+0x488` handle; this object's own UnInit Limbo
            // released it already, and a structure that never limbos never
            // started a loop, so no second event is published here.
            // Building43BD67 broadcasts removed=true after sound release,
            // even when a limbo object never reached UnInit. The original
            // constructed-cancel controls execute this under Factory's
            // A8E7AC bracket; the ordinary late drain repeats it after UnInit.
            self.notify_pointer_expired(stable_id, context.reborrow());
            // Building43BD6C..43BD6E then Limbos the building, whatever
            // GameState holds. The repeat under it at 43BF3B takes the
            // in-limbo branch and only reruns the factory-queue update
            // (`0x00445E14` -> `0x00509140`), which the first call already
            // settled; it is not ported. After UnInit this call is itself
            // that repeat; a building destroyed while placed leaves its
            // cells here.
            self.techno_limbo_with_context(stable_id, context.reborrow());
            // Building43BDC5 -> ClearAnimSlot451E40(-2) synchronously deletes
            // ordinary21slots, then43BDE0 destroys the distinct eight fire
            // Anims, before Techno tracking removal43BF34. Whole original
            // UnInit/drain control: building_death_anims joined evidence,
            // gamemd.
            self.clear_all_building_anim_slots(stable_id);
            self.clear_building_damage_fire_slots(stable_id, context.rules());
            // Building43BE47..43BEF0 rechecks existing grants when one of its
            // actual457630/457690 providers departs. This is a destructor
            // hold/revoke callback, not an event-time grant pass.
            let rules = context.rules();
            if (game_state as u8) != 0
                && let Some(owner) = self
                    .substrate
                    .entities
                    .get(stable_id)
                    .filter(|entity| {
                        crate::sim::superweapon::provided_supers(self, rules, entity, false)
                            .next()
                            .is_some()
                    })
                    .map(|entity| entity.owner())
            {
                crate::sim::superweapon::update_owned_supers(self, rules, owner, context.effects());
            }
            // Active-frame Building43BEF5..43BF11 compares Health+6C with
            // the retained AI sample+544 after its slot/fire destructors.
            // A mismatch dirties power only, without publishing a sample or
            // touching radar, before Remove_Tracking43BF34. Original joined
            // building_death_anims: normal fatal0/38 versus direct0/0.
            let unsampled_owner = self.substrate.entities.get(stable_id).and_then(|entity| {
                entity
                    .building_power_health_sample()
                    .filter(|sample| *sample != entity.health.current())
                    .map(|_| entity.owner())
            });
            if (game_state as u8) != 0
                && let Some(owner) = unsampled_owner
            {
                Self::invalidate_house_power(&self.houses, &mut self.power_states, owner, false);
            }
        }
        // The Techno destructors (`0x0041410B`, `0x0043BF34`, `0x00517E2E`,
        // `0x00735816`) call Remove_Tracking under GameState's low-byte gate.
        // The admitted entity store contains constructor-complete concrete
        // types; the native null-Type branch is outside that representation.
        if (game_state as u8) != 0 {
            HouseTracking::unregister_object(&mut self.substrate, &mut self.houses, stable_id);
        }
        // Infantry517E33..517E6C resets movement animation/water/prone/Doing,
        // then calls FootLimbo4DB260 even when already hidden in Cargo. The
        // canonical class+Foot owner preserves that repeated-Limbo suffix.
        // Aircraft414110..414112 calls FootLimbo4DB260 at the same point.
        let aircraft = self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| entity.category() == EntityCategory::Aircraft);
        if (infantry || aircraft) && (game_state as u8) != 0 {
            self.techno_limbo_with_context(stable_id, context.reborrow());
        }
        // Building43BF40..43BF51 scalar-deletes its Factory (`+0x524`) after
        // Remove_Tracking, outside the GameState gate. Limbo's Detach_All
        // abandoned it for a placed building; one already in limbo kept it.
        if self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| entity.category() == EntityCategory::Structure)
        {
            crate::sim::production::delete_building_factory(
                self,
                context.rules(),
                stable_id,
                context.effects(),
                (game_state as u8) != 0,
            );
        }
        // Unit73581B..73584D follows its own RemoveTracking735816 with
        // CargoRemoveFirst473430 -> child's scalar+20, before Unit expiry
        //735892 and the delegated Foot destructor. A8E7AC!=0 skips this
        //loop: PlaceMembers65E5A4's final refusal still holds that scope,
        //whereas its initial NullCell65E12B disposal does not. Ordinary
        //UnInit already drained cargo; constructor-complete Full carriers
        //can reach this scalar owner directly. Do not book a kill/UnInit or
        //use FootRemoveFirst4DE710's gunner reset on this raw Cargo pop.
        // Source: Steam gamemd, Unit destructor735780 body.
        if (game_state as u8) != 0
            && !self.object_placement_scope_active()
            && self
                .substrate
                .entities
                .get(stable_id)
                .is_some_and(|entity| entity.category() == EntityCategory::Unit)
        {
            loop {
                let passenger = self
                    .substrate
                    .entities
                    .get_mut(stable_id)
                    .and_then(|entity| entity.passenger_role_mut().cargo_mut())
                    .and_then(|cargo| cargo.unload_first())
                    .map(|(id, _)| id);
                let Some(passenger) = passenger else {
                    break;
                };
                if self.substrate.entities.contains(passenger) {
                    self.finalize_and_remove_common(passenger, context.reborrow(), game_state);
                }
            }
        }
        // Unit73584D..73584F then calls FootLimbo4DB260, inside the GameState
        // gate and whatever A8E7AC holds. Residual: the flag it carries
        // (`+0x6CC`, 735858..735884) is not modeled, so no drop follows.
        if (game_state as u8) != 0
            && self
                .substrate
                .entities
                .get(stable_id)
                .is_some_and(|entity| entity.category() == EntityCategory::Unit)
        {
            self.techno_limbo_with_context(stable_id, context.reborrow());
        }
        if foot {
            // Class scalar destructors dispatch removed=true independently of
            // the active-state gate, after class tracking/Limbo and before the
            // delegated Foot cached-cell/sound tail: Unit735892,
            // Infantry517E75 and Aircraft414125 -> original7258D0. This also
            // repairs observers of a rejected constructor that never UnInits.
            self.notify_pointer_expired(stable_id, context.reborrow());
        }
        // Infantry517E75 announces expiry while still Alive;517F83 clears it
        // only afterward, before delegated Foot517F89. HP/kill state survive.
        if infantry && let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            entity.lifecycle.object_alive = false;
        }
        // The ObjectClass destructor's defensive Defuse (`0x005F3BA6`).
        Self::bomb_defuse(
            &mut self.substrate,
            &mut self.bombs,
            &mut self.sound_events,
            stable_id,
        );
        self.team_script_vm.object_deleted(stable_id);
        Self::release_house_base_tracking(&self.substrate, &mut self.houses, stable_id);
        // Selected original Foot destructor4D3632..4D366E clears only its
        // cached+564 Cell if+E0 still holds this Foot. UnInit/Limbo retain it;
        // the block sends no notification and leaves the obsolete+564 intact.
        self.substrate.clear_foot_air_slot_at_destruction(stable_id);
        // Foot destructor4D3677 Release406060 follows class Remove_Tracking
        // and Foot's cached-cell cleanup. All Foot classes share it, including
        // constructor-complete limbo disposal. No caller-local hard stop.
        if self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| entity.category() != EntityCategory::Structure)
        {
            self.sound_events
                .emit(super::SimSoundEvent::ObjectSoundReleased { owner: stable_id });
        }
        // Foot's Parasite destructor6293E0 destroys its held Anim and clears
        // progress; it neither invents a release nor rewrites the host's HP.
        self.reset_parasite_grapple(stable_id, context.rules());
        // The Building destructor's Release406060 on its +0x6B4 working-sound
        // handle (43BD1A), the same most-derived destructor slot.
        Self::release_building_working_sound(
            &self.interner,
            &self.substrate,
            &mut self.sound_events,
            stable_id,
            context.rules(),
        );
        if context.rules.ares() {
            self.destroy_ares_attach_effects(stable_id, context.rules());
        }
        // Techno destructor6F452C destroys its planning token after the Foot
        // tail, then6F4571 frees any slave manager without a GameState gate.
        // Direct scalar entry has no earlier UnInit to supply either phase.
        if kind == Some(ObjectKind::Entity) {
            self.destroy_planning_token(stable_id);
            self.free_slaves(
                stable_id,
                None,
                None,
                context.rules(),
                context.registry(),
                context.effects(),
            );
        }
        // Foot4D3701 delegates to Techno6F4500 only after its own sound
        // tail. Techno6F4607 destroys the distinct voice handle via405C00,
        // before its attached/deploy Anims6F468B. Keep the disposal marker in
        // the same stream as AI visits and those Anim sound callbacks.
        if kind == Some(ObjectKind::Entity) {
            self.sound_events
                .emit(super::SimSoundEvent::UnitVoiceDestroyed { owner: stable_id });
        }
        // Techno destructor6F4667..6F4679 UnInits its Behind Anim, then
        //6F467F..6F4691 its retained deploy Anim. Pointer-expiry may already
        // have cleared either through the Anim owner.
        if let Some(anim) = self
            .substrate
            .entities
            .get_mut(stable_id)
            .and_then(|entity| entity.take_behind_anim())
        {
            self.destroy_anim_with_context(anim, context.rules());
        }
        if let Some(anim) = self
            .substrate
            .entities
            .get_mut(stable_id)
            .and_then(|entity| entity.take_deploy_anim())
        {
            self.destroy_anim_with_context(anim, context.rules());
        }
        self.lighting_sources.destroy_building_light(stable_id);
        if kind == Some(ObjectKind::Anim) {
            self.notify_anim_pointer_expired(stable_id);
            self.substrate.clear_damage_fire_anim_reference(stable_id);
            self.substrate.release_last_anim_owner_flag(stable_id);
            self.substrate.release_anim_owner_reference(stable_id);
            self.substrate.clear_building_anim_reference(stable_id);
            self.conceal_anim(stable_id);
        }
        // The native Object field+34 disappears with storage. This is not
        // SetTag(nullptr): the shared Tag owner preserves its native counter.
        self.trigger_runtime
            .finalize_object_trigger_tag_attachment(stable_id);
        // Display registration never outlives the object. Fly's phase tail
        // (4CD4DE) resubmits even an owner concealed earlier in the frame.
        self.substrate.display.remove(stable_id);
        self.fog.forget_sight_source(stable_id);
        // FoggedObject4D0EF0 has no source-object pointer: its image survives
        // this destruction until CellClass::CleanFog486BF0 reveals it.
        self.fog.forget_fogged_live_object(stable_id);
        self.drop_object_custom_sound(stable_id);
        // Ares TunnelData::InvalidatePointer (100091E0): a Foot leaving play
        // inside a tunnel drops out of its network.
        if context.rules.ares() && kind == Some(ObjectKind::Entity) {
            self.ares_tunnels.forget(stable_id);
        }
        // `ObjectClass::~ObjectClass @ 0x005F3B80` ends by testing the Logic
        // membership byte (`+0x98`, `0x005F3D65`) and calling
        // `LogicClass::UnregisterObject` (`0x005F3D75`). Limbo has usually
        // cleared it; a rider killed inside an open-topped transport was
        // already in limbo, so Limbo returned at its entry guard
        // (`0x005F4D45`) and this is the only removal.
        self.unregister_logic_object(stable_id);
        let removed = match kind {
            Some(ObjectKind::Entity) => {
                let entity = self.substrate.entities.remove(stable_id);
                if entity
                    .as_ref()
                    .is_some_and(|entity| entity.category() == EntityCategory::Structure)
                {
                    // Techno+3D3 (ctor6F2F4F) dies with the Building's storage. Limbo retains it;
                    // physical disposal removes this identity without electing a
                    // replacement or altering the House's Factory queue slots.
                    self.production.clear_primary_factory(stable_id);
                }
                entity.is_some()
            }
            Some(ObjectKind::Anim) => self.substrate.anims.remove(stable_id).is_some(),
            Some(ObjectKind::ParticleSystem) => {
                let system = self.substrate.particle_systems.finalize_remove(stable_id);
                if let Some(system) = system.as_ref() {
                    // SystemDtor62E0AE..62E15D disposes retained children before its
                    // vector is freed. Ordinary AI retirement has no children left;
                    // direct disposal must still remove each individual AIR entry.
                    for particle in system.particles() {
                        let id = particle.stable_id();
                        self.substrate.conceal_particle(id);
                        // ParticleDtor62D9C8 announces the expiry: a glow
                        // waits for PurgeDisabled, or under Ares goes now.
                        self.alpha_shapes.pointer_expired(id);
                        self.delete_alpha_shape(id, context.rules());
                    }
                }
                system.is_some()
            }
            Some(ObjectKind::Particle) => self
                .substrate
                .particle_systems
                .finalize_particle(stable_id)
                .is_some(),
            Some(ObjectKind::Terrain) => self.terrain_objects.remove(stable_id).is_some(),
            Some(ObjectKind::Projectile) => {
                let removed = self.projectiles.remove(stable_id).is_some();
                if removed {
                    self.lifecycle_outputs
                        .emit(LifecycleOutput::LineTrailDetached { stable_id });
                }
                removed
            }
            Some(ObjectKind::Wave) => {
                let wave = self.waves.remove(stable_id);
                if let Some(wave) = wave.as_ref()
                    && let Some(owner_id) = wave.owner_id()
                    && self.active_wave_links.get(&owner_id) == Some(&stable_id)
                {
                    self.active_wave_links.remove(&owner_id);
                }
                wave.is_some()
            }
            Some(ObjectKind::Smudge) => self
                .smudge_grid
                .as_mut()
                .and_then(|grid| grid.finalize_remove(stable_id))
                .is_some(),
            Some(ObjectKind::VoxelAnim) => self.substrate.voxel_anims.remove(stable_id).is_some(),
            // A spotlight returned above.
            Some(ObjectKind::Spotlight) => false,
            // A stale deferred-delete ticket: a direct destructor already
            // removed its object (`discard_constructed_limbo` keeps a queued
            // constructor-owned child's ticket).
            None => false,
        };
        // Ares `ObjectClass_DTOR` (hook `0x005F3D65`, `Ares.dll 100476E0`).
        if removed {
            self.delete_alpha_shape(stable_id, context.rules());
        }
        debug_assert!(
            removed || kind.is_none(),
            "{kind:?} {stable_id} left no store at its destructor"
        );
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::FinalizedCommon { stable_id });
    }

    /// Read-only membership in the single deferred object-finalization queue.
    pub fn object_is_pending_delete(&self, stable_id: u64) -> bool {
        self.substrate.pending_delete.contains_object(&stable_id)
    }

    /// The pending-delete drain of test fixtures, on the rules an empty INI
    /// reads to (see [`Self::process_pending_delete_with`]).
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn process_pending_delete(&mut self) {
        self.process_pending_delete_with(
            crate::rules::ruleset::RuleSet::empty_for_test(),
            &mut FrameEffects::default(),
            1,
            self.session.started(),
        );
    }

    /// Native-shaped pending-delete drain: preserve alive entries, collapse all
    /// duplicate ready IDs, and finalize each selected object exactly once.
    /// The frame's rules reach the destructors' slave release.
    ///
    /// gamemd-derived: active YR `DrainDeferredFinalizationQueue @ 0x00725C70`
    /// is reached from `Main_Tick` at `0x0055DE9F` after the frame commit; it
    /// preserves non-ready entries, collapses selected duplicates, and finalizes
    /// each selected ready object once.
    pub fn process_pending_delete_with(
        &mut self,
        rules: &RuleSet,
        effects: &mut FrameEffects<'_>,
        game_state: i32,
        started: bool,
    ) {
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::PendingDeleteDrainStarted);
        let mut index = 0;
        while let Some(ticket) = self.substrate.pending_delete.ticket(index) {
            let ready = match ticket {
                DeferredDeleteTicket::Object(id) => self.pending_object_is_ready(id),
                DeferredDeleteTicket::Overlay(id) => self.load_objects.queued_overlay_is_ready(id),
                // Abstract410440's +44 returns true for both live families.
                DeferredDeleteTicket::Tag(_)
                | DeferredDeleteTicket::Trigger(_)
                | DeferredDeleteTicket::DiskLaser(_) => true,
            };
            if !ready {
                index += 1;
                continue;
            }
            //725C9A..725CE1 removes every selected duplicate BEFORE virtual
            // destruction, then reexamines the same live logical index.
            self.substrate.pending_delete.remove(ticket);
            #[cfg(any(test, feature = "test-support"))]
            self.trace_lifecycle_for_test(LifecycleTestEvent::DeferredDeleteSelected { ticket });
            match ticket {
                DeferredDeleteTicket::Object(id) => {
                    self.finalize_and_remove_common(
                        id,
                        UninitContext::new(rules).with_effects(effects),
                        game_state,
                    );
                }
                DeferredDeleteTicket::Overlay(id) => self.load_objects.destroy_queued_overlay(id),
                DeferredDeleteTicket::Tag(id) => {
                    self.delete_live_tag(id, rules, game_state, started)
                }
                DeferredDeleteTicket::Trigger(id) => self.trigger_runtime.delete_live_trigger(id),
                DeferredDeleteTicket::DiskLaser(id) => self.destroy_disk_laser(id),
            }
        }
        self.substrate.pending_delete.finish_drain();
        for anim in self.substrate.anims.drain_local_deletes() {
            self.delete_anim_alpha_shape(anim);
        }
    }

    /// Test compatibility only.  Production has one ordinary tail drain.
    #[cfg(any(test, feature = "test-support"))]
    pub fn flush_pending_delete(&mut self) {
        self.process_pending_delete();
    }
}
