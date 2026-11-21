//! Logic-vector membership and non-entity objects: object classification,
//! Logic register/unregister, anim, particle, terrain, projectile and wave
//! registration and retirement.

use crate::rules::ruleset::RuleSet;
use crate::sim::projectile::ProjectileStore;
use crate::sim::terrain_object::TerrainObjectStore;
use crate::sim::wave::WaveStore;
use crate::sim::world::display_layers::DisplayLayer;
use crate::sim::world::substrate::{DeferredDeleteTicket, ObjectKind};
use crate::sim::world::{FrameEffects, ObjectSubstrate, Simulation};

#[cfg(any(test, feature = "test-support"))]
use super::LifecycleTestEvent;
use super::UninitContext;

impl Simulation {
    /// The kind of the object `stable_id` names, or `None` when no store
    /// holds it. Object IDs are unique across stores, so the probe order is
    /// only a cost: entities and anims first, as most Logic slots are; then
    /// the store the Logic list noted at the id's registration (a bullet's
    /// every visit, and its retirement until it leaves Logic, ask here);
    /// then the other stores in [`ObjectKind::ALL`]'s order, with particles
    /// last, as finding one walks the systems. VERA-only: gamemd's objects
    /// carry their class in their vtable.
    pub(crate) fn classify_object(&self, stable_id: u64) -> Option<ObjectKind> {
        const COMMON: [ObjectKind; 2] = [ObjectKind::Entity, ObjectKind::Anim];
        let holds = |kind| self.object_store_holds(kind, stable_id);
        if let Some(kind) = COMMON.into_iter().find(|&kind| holds(kind)) {
            return Some(kind);
        }
        if let Some(kind) = self.substrate.logic.kind_of(stable_id)
            && holds(kind)
        {
            return Some(kind);
        }
        ObjectKind::ALL
            .into_iter()
            .filter(|kind| !COMMON.contains(kind))
            .find(|&kind| holds(kind))
    }

    /// Whether `kind`'s store holds `stable_id`.
    pub(in crate::sim::world) fn object_store_holds(
        &self,
        kind: ObjectKind,
        stable_id: u64,
    ) -> bool {
        match kind {
            ObjectKind::Entity => self.substrate.entities.contains(stable_id),
            ObjectKind::Anim => self.substrate.anims.contains_key(stable_id),
            ObjectKind::VoxelAnim => self.substrate.voxel_anims.contains_key(stable_id),
            ObjectKind::Spotlight => self.substrate.spotlights.contains_key(stable_id),
            ObjectKind::ParticleSystem => self.substrate.particle_systems.contains_key(stable_id),
            ObjectKind::Terrain => self.terrain_objects.objects().contains_key(&stable_id),
            ObjectKind::Projectile => self.projectiles.get(stable_id).is_some(),
            ObjectKind::Wave => self.waves.get(stable_id).is_some(),
            ObjectKind::Smudge => self
                .smudge_grid
                .as_ref()
                .and_then(|grid| grid.object(stable_id))
                .is_some(),
            ObjectKind::Particle => self
                .substrate
                .particle_systems
                .particle(stable_id)
                .is_some(),
        }
    }

    /// Read the per-object `in_logic_vector` membership flag for a classified
    /// object. The flag lives on the object in its own store; this is the
    /// single dispatch the registration/removal contract reads through.
    /// For a Techno, `Object+0x98`: RegisterObject sets it (`0x0055BAC6`).
    pub(in crate::sim::world) fn logic_membership_flag(
        &self,
        stable_id: u64,
        kind: ObjectKind,
    ) -> bool {
        match kind {
            ObjectKind::Spotlight => self
                .substrate
                .spotlights
                .get(stable_id)
                .is_some_and(|l| l.in_logic_vector()),
            ObjectKind::Smudge | ObjectKind::Particle => false,
            ObjectKind::Anim => self
                .substrate
                .anims
                .get(stable_id)
                .is_some_and(|anim| anim.in_logic_vector()),
            ObjectKind::VoxelAnim => self
                .substrate
                .voxel_anims
                .get(stable_id)
                .is_some_and(|debris| debris.in_logic_vector()),
            ObjectKind::ParticleSystem => self
                .substrate
                .particle_systems
                .get(stable_id)
                .is_some_and(|system| system.in_logic_vector()),
            ObjectKind::Terrain => self
                .terrain_objects
                .objects()
                .get(&stable_id)
                .is_some_and(|terrain| terrain.in_logic_vector()),
            ObjectKind::Projectile => self
                .projectiles
                .get(stable_id)
                .is_some_and(|projectile| projectile.in_logic_vector()),
            ObjectKind::Wave => self
                .waves
                .get(stable_id)
                .is_some_and(|wave| wave.in_logic_vector()),
            ObjectKind::Entity => self
                .substrate
                .entities
                .get(stable_id)
                .is_some_and(|entity| entity.seldom.in_logic_vector.is_member()),
        }
    }

    /// Write the per-object `in_logic_vector` membership flag for a classified
    /// object. The single dispatch the registration/removal contract repairs
    /// the flag through. For a Techno it is RegisterObject's set (`0x0055BAC6`)
    /// or UnregisterObject's clear (`0x0055BB27`).
    pub(in crate::sim::world) fn set_logic_membership_flag(
        terrain_objects: &mut TerrainObjectStore,
        substrate: &mut ObjectSubstrate,
        projectiles: &mut ProjectileStore,
        waves: &mut WaveStore,
        stable_id: u64,
        kind: ObjectKind,
        member: bool,
    ) {
        match kind {
            ObjectKind::Spotlight => {
                if let Some(l) = substrate.spotlights.get_mut(stable_id) {
                    l.mark_logic_membership(member);
                }
            }
            ObjectKind::Smudge | ObjectKind::Particle => {
                assert!(!member, "{kind:?} {stable_id} never joins Logic");
            }
            ObjectKind::Anim => {
                if let Some(anim) = substrate.anims.get_mut(stable_id) {
                    anim.mark_logic_membership(member);
                }
            }
            ObjectKind::VoxelAnim => {
                if let Some(debris) = substrate.voxel_anims.get_mut(stable_id) {
                    debris.mark_logic_membership(member);
                }
            }
            ObjectKind::ParticleSystem => {
                if let Some(system) = substrate.particle_systems.get_mut(stable_id) {
                    system.mark_logic_membership(member);
                }
            }
            ObjectKind::Terrain => {
                terrain_objects.set_logic_membership(stable_id, member);
            }
            ObjectKind::Projectile => {
                if let Some(projectile) = projectiles.get_mut(stable_id) {
                    projectile.mark_logic_membership(member);
                }
            }
            ObjectKind::Wave => {
                if let Some(wave) = waves.get_mut(stable_id) {
                    wave.mark_logic_membership(member);
                }
            }
            ObjectKind::Entity => {
                if let Some(entity) = substrate.entities.get_mut(stable_id) {
                    entity.seldom.in_logic_vector.mark(member);
                }
            }
        }
    }

    /// gamemd-derived: active YR `LogicClass__RegisterObject @ 0x0055BAA0`
    /// gates on the object's membership flag, appends at the live tail, then
    /// sets the flag only after insertion succeeds.
    pub(super) fn register_logic_object(&mut self, stable_id: u64) -> bool {
        let kind = self.classify_object(stable_id);
        if let Some(kind) = kind
            && self.logic_membership_flag(stable_id, kind)
        {
            return true;
        }
        let Some(kind) = kind else {
            return false;
        };
        if !kind.joins_logic() {
            return false;
        }
        if self.substrate.logic.try_push(stable_id, kind).is_err() {
            return false;
        }
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::LogicAppended);
        Self::set_logic_membership_flag(
            &mut self.terrain_objects,
            &mut self.substrate,
            &mut self.projectiles,
            &mut self.waves,
            stable_id,
            kind,
            true,
        );
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::LogicMembershipSet);
        true
    }

    /// gamemd-derived: active YR `LogicClass__UnregisterObject @ 0x0055BAE0`
    /// gates removal on the object's membership flag, performs the first-match
    /// stable erase, then repairs that flag.
    pub(super) fn unregister_logic_object(&mut self, stable_id: u64) -> bool {
        let Some(kind) = self.classify_object(stable_id) else {
            return false;
        };
        if !self.logic_membership_flag(stable_id, kind) {
            return false;
        }
        let _ = self.substrate.logic.remove_first(stable_id);
        Self::set_logic_membership_flag(
            &mut self.terrain_objects,
            &mut self.substrate,
            &mut self.projectiles,
            &mut self.waves,
            stable_id,
            kind,
            false,
        );
        true
    }

    /// Test-only access to the exact LogicVector helper ordering. Returns the
    /// dispatch outcome so order/gate tests can assert it directly.
    #[cfg(any(test, feature = "test-support"))]
    pub fn register_live_object(&mut self, stable_id: u64) -> bool {
        self.register_logic_object(stable_id)
    }

    /// Test-only access to the exact LogicVector helper ordering. Returns the
    /// dispatch outcome so order/gate tests can assert it directly.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn unregister_live_object(&mut self, stable_id: u64) -> bool {
        self.unregister_logic_object(stable_id)
    }

    /// Reveal, leaving out the Display submit when `submit` is false: the
    /// constructor's attach submits the Anim before anything reads Display
    /// (`spawn_anim_at_world_with_constructor`).
    pub(crate) fn reveal_anim(&mut self, stable_id: u64, rules: &RuleSet, submit: bool) -> bool {
        // A physically retained Destroy/UnInit object has already left Logic
        // and Display. Its independent19B byte can still be zero, so it cannot
        // be resurrected through Reveal before the common deferred drain.
        if self.substrate.pending_delete.contains_object(&stable_id)
            || self.substrate.anims.local_delete_queued(stable_id)
            || !self
                .substrate
                .anims
                .get(stable_id)
                .is_some_and(|anim| !anim.runtime().inactive)
        {
            return false;
        }
        self.substrate.mark_anim_display(stable_id, true);
        if submit {
            self.submit_anim_display(stable_id, rules);
        }
        // `ObjectClass::Unlimbo @ 0x005F4EC0` leaves an object with id -2 out
        // of Logic in LAN and online games: the local list runs it instead
        // (`FrameStep::LocalAnims`).
        let registered = !crate::sim::anim_class::is_local_anim_id(stable_id)
            && self.register_logic_object(stable_id);
        // The glow block (`0x005F5045`) follows either way.
        self.unlimbo_alpha_shape(stable_id, ObjectKind::Anim, rules);
        registered
    }

    pub(crate) fn conceal_anim(&mut self, stable_id: u64) -> bool {
        self.substrate.display.remove(stable_id);
        self.substrate.anims.conceal_presentation(stable_id);
        self.substrate.mark_anim_display(stable_id, false);
        self.unregister_logic_object(stable_id)
    }

    /// The BuildingLight constructor435820 explicitly joins the shared Logic owner.
    pub(in crate::sim::world) fn register_spotlight_logic(&mut self, id: u64) {
        self.register_logic_object(id);
    }
    pub(in crate::sim::world) fn conceal_spotlight_logic(&mut self, id: u64) {
        self.substrate.display.remove(id);
        self.unregister_logic_object(id);
    }

    /// `ObjectClass::Unlimbo` inside `VoxelAnimClass::Constructor @ 0x007493B0`
    /// reveals the piece, which is what puts it in the LogicClass vector.
    pub(crate) fn reveal_voxel_anim(&mut self, stable_id: u64) -> bool {
        let Some(piece) = self.substrate.voxel_anims.get_mut(stable_id) else {
            return false;
        };
        if !piece.lifecycle().object_alive() {
            return false;
        }
        // ObjectUnlimbo5F4F4E / Mark5F58F7 update the base bytes even
        // though Voxel OccupyList74AB40 returns null: no Cell insertion.
        piece.lifecycle_mut().in_limbo = false;
        piece.lifecycle_mut().admit_mark_down();
        // VoxelAnim VT7F6318+78 ->74A960: always Air.
        Self::submit_object_display(&mut self.substrate, stable_id, DisplayLayer::AIR);
        self.register_logic_object(stable_id)
    }

    pub(crate) fn reveal_particle_system(&mut self, stable_id: u64) -> bool {
        if !self.substrate.particle_systems.contains_key(stable_id) {
            return false;
        }
        // ParticleSystem VT7EFB9C+78 ->62FE80: always Ground.
        // RESIDUAL: ObjectUnlimbo refuses the empty coordinate
        // (`0x005F4ECD..0x005F4EEF`), so gamemd leaves a system built at
        // 0,0,0 in Limbo; VERA reveals it. No producer builds one there.
        Self::submit_object_display(&mut self.substrate, stable_id, DisplayLayer::GROUND);
        self.register_logic_object(stable_id)
    }

    pub(crate) fn conceal_particle_system(&mut self, stable_id: u64) -> bool {
        self.substrate.display.remove(stable_id);
        self.unregister_logic_object(stable_id)
    }

    /// Particle62BB0E calls ObjectUnlimbo5F4EC0; GetLayer62D770 returns
    /// AIR (3), so Display4A9720 reaches the existing append owner5519B0.
    /// A particle is owned by its system and never joins global Logic.
    pub(crate) fn reveal_particle(substrate: &mut ObjectSubstrate, stable_id: u64) {
        Self::submit_object_display(substrate, stable_id, DisplayLayer::AIR);
    }

    /// Spark62ED2C (and the other System AI cleanup callers) invokes the
    /// Particle's inherited ObjectUnInit5F65F0. Transfer the unique body
    /// first, then use that common lifecycle entry; it owns expiry, Limbo,
    /// Alive and the one B0F698 queue. Physical destruction waits for725C70.
    /// Source and executed queue controls: procedural_drawing_oracle/welding.
    pub(crate) fn retire_particle(
        &mut self,
        particle: crate::sim::particles::Particle,
        rules: &RuleSet,
    ) {
        let id = self
            .substrate
            .particle_systems
            .retain_detached_particle(particle);
        self.uninit_with_context(id, UninitContext::new(rules));
    }

    pub(crate) fn register_terrain_object(&mut self, stable_id: u64, rules: &RuleSet) -> bool {
        let Some(terrain) = self
            .terrain_objects
            .get(stable_id)
            .filter(|terrain| terrain.is_live())
        else {
            return false;
        };
        let joins_logic = rules
            .terrain_object_type_case_insensitive(self.interner.resolve(terrain.type_ref()))
            .is_some_and(|terrain_type| {
                terrain.joins_logic_on_unlimbo(
                    terrain_type,
                    self.session.rule_policy(),
                    rules.ares_form(),
                )
            });
        // Terrain ctor71BC76 reveals at the retained type-adjusted coordinate;
        // Object5F4260 selects Ground even on elevated cells.
        Self::submit_object_display(&mut self.substrate, stable_id, DisplayLayer::GROUND);
        // ObjectUnlimbo5F4FE2 submits Display before the independent Logic
        // admission5F4FEF..5F5040; ordinary trees still complete Unlimbo.
        if joins_logic {
            self.register_logic_object(stable_id);
        }
        self.unlimbo_alpha_shape(stable_id, ObjectKind::Terrain, rules);
        true
    }

    /// Pickup reads the current Type cache, not construction-time instance
    /// state: TerrainTypeLoad71E22D can bind a formerly null Occupy_List.
    /// Resource-free synthetic callers retain their explicit legacy policy.
    fn terrain_occupied_offsets(&self, stable_id: u64, rules: &RuleSet) -> Vec<(i16, i16)> {
        self.terrain_objects
            .get(stable_id)
            .and_then(|terrain| {
                rules
                    .terrain_object_type_case_insensitive(self.interner.resolve(terrain.type_ref()))
            })
            .map_or_else(
                || vec![(0, 0)],
                |terrain_type| {
                    terrain_type
                        .occupied_cell_offsets()
                        .expect("Terrain footprint admitted at construction or saved load")
                },
            )
    }

    pub fn register_projectile(&mut self, stable_id: u64, flat: bool) -> bool {
        if self.projectiles.get(stable_id).is_none() {
            return false;
        }
        // Bullet Fire468B6D ->Submit; GetLayer468B90 reads type+2F7.
        let layer = if flat {
            DisplayLayer::SURFACE
        } else {
            DisplayLayer::AIR
        };
        Self::submit_object_display(&mut self.substrate, stable_id, layer);
        self.register_logic_object(stable_id)
    }

    pub(crate) fn register_wave(&mut self, stable_id: u64) -> bool {
        if self.waves.get(stable_id).is_none() {
            return false;
        }
        // Wave VT7F6BF4+78 ->75F890: always Air; Submit at75F952.
        Self::submit_object_display(&mut self.substrate, stable_id, DisplayLayer::AIR);
        self.register_logic_object(stable_id)
    }

    pub(crate) fn unregister_non_entity_object(&mut self, stable_id: u64) -> bool {
        self.substrate.display.remove(stable_id);
        self.unregister_logic_object(stable_id)
    }

    /// TerrainLimbo71C930 -> ObjectLimbo5F4D30: Mark(UP), Display removal,
    /// ambient hard-stop5F4D81, custom detach5F4D89, Logic removal5F4DD3,
    /// then Limbo flags. The Terrain-specific navigation suffix follows.
    pub(super) fn limbo_terrain_with_context(
        &mut self,
        stable_id: u64,
        mut context: UninitContext<'_>,
    ) -> bool {
        let Some(terrain) = self.terrain_objects.get(stable_id) else {
            return false;
        };
        let cell = terrain.cell();
        let was_live = terrain.is_live();
        if was_live {
            terrain.limbo_prefix(
                &mut self.substrate.raw_cell_occupation,
                self.resolved_terrain.as_ref(),
            );
            self.notify_pointer_expired(stable_id, context.reborrow());
            let occupied_offsets = self.terrain_occupied_offsets(stable_id, context.rules());
            let recalculated = self
                .terrain_objects
                .begin_limbo(
                    stable_id,
                    &mut self.substrate.raw_cell_occupation,
                    self.resolved_terrain.as_mut(),
                    &occupied_offsets,
                )
                .expect("live Terrain enters PickUp");
            // MapPickUp5688EB/568911 Recalc each visited recipient. The
            // TerrainLimbo71CA23..65 source-only zone/radar suffix is below,
            // once after ObjectLimbo, never replayed for footprint cells.
            self.publish_recalculated_cells(context.rules(), &recalculated);
            self.substrate.display.remove(stable_id);
            self.object_sound_conceal(stable_id, context.rules());
            let rules = context.rules();
            context.effects().flush_sound(self, rules);
            self.unregister_logic_object(stable_id);
            self.terrain_objects.finish_limbo(stable_id);
        }
        self.finish_terrain_navigation_changes(context.rules(), &[cell]);
        was_live
    }

    /// Terminal Terrain/Bullet/Wave objects leave Logic immediately but retain
    /// their physical store identity until the common late delete drain.
    ///
    /// gamemd-derived: active YR `DrainDeferredFinalizationQueue @ 0x00725C70`
    /// performs scalar destruction/freeing only after the frame commit.
    pub fn retire_non_entity_object(
        &mut self,
        stable_id: u64,
        mut context: UninitContext<'_>,
    ) -> bool {
        let represented = matches!(
            self.classify_object(stable_id),
            Some(ObjectKind::Terrain | ObjectKind::Projectile | ObjectKind::Wave)
        );
        if !represented {
            return false;
        }
        if self
            .terrain_objects
            .get(stable_id)
            .is_some_and(|terrain| terrain.object_alive())
        {
            // ObjectUnInit5F6616 and ObjectLimbo5F4D61 announce while this
            // instance is retained. TerrainLimbo71C930 unlinks only its ID;
            // a successor keeps its own Logic/Display, Stage and cell query.
            self.notify_pointer_expired(stable_id, context.reborrow());
            self.limbo_terrain_with_context(stable_id, context.reborrow());
            self.terrain_objects.finish_uninit(stable_id);
        }
        if self.classify_object(stable_id) == Some(ObjectKind::Projectile) {
            // `ObjectClass::UnInit @ 0x005F65F0` broadcasts the bullet's
            // expiry before it leaves Logic; an anim riding it ends
            // (`AnimClass::PointerExpired @ 0x00425150`, `+0x17C`).
            self.notify_pointer_expired(stable_id, context.reborrow());
            // ObjectLimbo5F4D61 repeats DetachAll before removing the fired
            // Bullet's membership. The later scalar destructor repeats once.
            self.notify_pointer_expired(stable_id, context);
        }
        if self.classify_object(stable_id) != Some(ObjectKind::Terrain) {
            self.detach_custom_sound(stable_id);
        }
        let _ = self.unregister_non_entity_object(stable_id);
        self.substrate.pending_delete.queue_object(stable_id);
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::PendingDeleteQueued { stable_id });
        true
    }

    /// Ordinary Terrain71B920 lethal tail: DetachAll, ObjectUnInit and
    /// ObjectLimbo each announce expiry while the retained object is still
    /// readable and marked. Only then may TerrainLimbo unlink its cell.
    pub(crate) fn terrain_lethal_uninit(
        &mut self,
        lethal: crate::sim::terrain_object::TerrainLethalDamage,
        finalizing: &mut std::collections::BTreeSet<u64>,
        mut context: UninitContext<'_>,
    ) -> bool {
        if !finalizing.contains(&lethal.stable_id) {
            return false;
        }
        // 71BB69->5F5311 precedes ordinary UnInit's5F6616/5F4D61.
        // Repeated announcements do not rematch a repaired Bullet.
        self.notify_pointer_expired(lethal.stable_id, context.reborrow());
        let finalized = self
            .terrain_objects
            .get(lethal.stable_id)
            .is_some_and(|terrain| terrain.object_alive());
        if finalized {
            let retired = self.retire_non_entity_object(lethal.stable_id, context);
            debug_assert!(retired);
        }
        finalizing.remove(&lethal.stable_id);
        finalized
    }

    /// COM Release46AFF0 -> scalar Bullet destructor46B5C0 for a temporary
    /// never-Fired Bullet. Its Limbo early-return performs no registration
    /// changes; destruction still uses the common observer/cleanup owner.
    pub(crate) fn destroy_unfired_projectile(
        &mut self,
        id: u64,
        rules: &RuleSet,
        effects: &mut FrameEffects<'_>,
    ) {
        let Some(bullet) = self.projectiles.get(id) else {
            return;
        };
        assert!(
            !bullet.in_logic_vector() && self.substrate.display.layer_of(id).is_none(),
            "temporary Bullet {id} must not be Fired before direct destruction"
        );
        self.substrate
            .pending_delete
            .remove(DeferredDeleteTicket::Object(id));
        self.finalize_and_remove_common(id, UninitContext::new(rules).with_effects(effects), 1);
    }

    /// Open-topped cargo entry hides the passenger but then directly restores
    /// its active membership. This is deliberately not Reveal: the passenger
    /// remains limbo/unmarked while its AI stays in the live object order.
    pub(crate) fn register_open_topped_passenger(&mut self, stable_id: u64) -> bool {
        if !self.substrate.entities.contains(stable_id) {
            return false;
        }
        self.register_logic_object(stable_id)
    }
}
