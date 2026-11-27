//! `Simulation` object admission: projectile, bullet-scheme and wave
//! construction, logic-membership and lifecycle consistency checks, the live
//! object walk, and debris voxel admission.

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
use crate::sim::intern::StringInterner;
use crate::sim::projectile::ProjectileStore;

use super::LifecycleOutput;
#[cfg(any(test, feature = "test-support"))]
use super::LifecycleTestEvent;
use super::ObjectKind;
use super::ObjectSubstrate;
use super::Simulation;

/// How many Logic slots ahead of the turn it runs the live object walk asks
/// the processor for an object's record (`util::prefetch`; sim-scale, results
/// unchanged). In sim-bench's 20,000-unit battle on an Apple M4 a turn takes
/// one to two microseconds and main memory answers in about a tenth of one,
/// so one slot would do for turns that all run; four leaves room for the
/// sleepers and effects between them, and holds 9 KB of a 128 KB first-level
/// cache. Two builds started together took 2.7% less simulation-thread time
/// with it than without; distances of 2, 8 and 16 measured within noise of 4.
const WARM_AHEAD: usize = 4;

impl Simulation {
    /// CreateBullet46B050 without Fire: consumes one native constructor ID,
    /// retains the independent construction fields and joins Object expiry
    /// listeners through the existing store, but not Logic or Display.
    pub(crate) fn construct_projectile(
        &mut self,
        source: u64,
        target: crate::sim::projectile::ProjectileTarget,
        payload: crate::sim::projectile::ProjectilePayload,
        rules: &RuleSet,
    ) -> u64 {
        let native_id = crate::sim::native_identity::NativeUniqueIdCursor::assign_runtime(
            &mut self.native_unique_ids,
        );
        let id = self.substrate.allocate_stable_id();
        self.projectiles.construct(
            id,
            native_id,
            self.session.binary_frame(),
            source,
            target,
            payload,
        );
        Self::construct_bullet_scheme(
            &self.interner,
            &self.substrate,
            &self.houses,
            &mut self.projectiles,
            id,
            rules,
        );
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::UnfiredProjectileConstructed {
            stable_id: id,
            native_id,
            source_id: source,
            target,
            payload,
            position: self
                .projectiles
                .get(id)
                .expect("constructed Bullet")
                .position(),
            in_logic: self
                .projectiles
                .get(id)
                .expect("constructed Bullet")
                .in_logic_vector(),
            in_display: self.substrate.display.layer_of(id).is_some(),
        });
        id
    }

    pub fn admit_projectile(
        &mut self,
        stable_id: u64,
        spawn: crate::sim::projectile::ProjectileSpawn,
    ) -> u64 {
        let location = crate::sim::projectile::bullet_unlimbo_coord(
            self.resolved_terrain.as_ref(),
            &self.effective_shared_cell_dummy(),
            spawn.origin,
        );
        self.projectiles
            .spawn_at(stable_id, self.session.binary_frame(), spawn, location);
        let registered = self.register_projectile(stable_id, spawn.flat);
        debug_assert!(registered);
        if let Some(style) = spawn.line_trail {
            self.lifecycle_outputs
                .emit(LifecycleOutput::LineTrailConstructed { stable_id, style });
        }
        stable_id
    }

    /// `BulletClass::Construct @ 0x004664C0`, `0x00466519..0x0046653B`: a
    /// `FirersPalette=` BulletType (`+0x2A9`) whose Owner (`+0xB0`) exists
    /// keeps the Owner's House colour scheme at `+0x114`; every other bullet
    /// keeps -1. Runs after each admission and re-Construct.
    pub(crate) fn construct_bullet_scheme(
        interner: &StringInterner,
        substrate: &ObjectSubstrate,
        houses: &crate::sim::houses::Houses,
        projectiles: &mut ProjectileStore,
        bullet: u64,
        rules: &RuleSet,
    ) {
        let Some(projectile) = projectiles.get(bullet) else {
            return;
        };
        #[cfg(not(feature = "phobos"))]
        let _ = houses;
        let source_id = projectile.source_id();
        let firers_palette = projectile
            .payload()
            .projectile_type()
            .and_then(|id| interner.projectile_type(id, rules))
            .is_some_and(|kind| kind.firers_palette);
        let house = firers_palette
            .then(|| substrate.entities.get(source_id))
            .flatten()
            .map(|owner| owner.owner());
        projectiles.set_firer_house(bullet, house);
        #[cfg(feature = "phobos")]
        {
            // `BulletExt::InitializeState` retains the Owner's House for
            // every bullet, not only a `FirersPalette=` one. A re-Construct
            // with no live owner keeps the saved value (native only writes
            // inside `if (pThis->Owner)`).
            let retained = substrate.entities.get(source_id).map(|owner| owner.owner());
            if retained.is_some() {
                projectiles.set_phobos_firer_house(bullet, retained);
            }
            if let Some(multiplier) =
                crate::sim::combat::world_fire::bullet_init_firepower_multiplier(
                    interner, substrate, houses, rules, source_id,
                )
            {
                projectiles.set_phobos_firepower_multiplier_if_default(bullet, multiplier);
            }
        }
    }

    /// Admit the bullets a detonation spawned (`BulletClass::SpawnShrapnel
    /// @ 0x0046A310`), each [`Self::construct_bullet_scheme`]d as every
    /// constructed bullet is. SpawnShrapnel then Fires each one (vt+0x1F0,
    /// `0x0046A875`, `0x0046AD29`), whose Inviso arm (`0x004688B7..0x00468986`)
    /// stands an `Inviso=` child on its target's vt+0x58, the coordinate the
    /// spawn captured ([`ProjectileStore::fire_inviso`]); its OnBridge stays
    /// clear, as only FireAt copies one.
    ///
    /// RESIDUAL (without Ares; when Ares plays it gives the child its
    /// shrapnel weapon, [`Self::ares_shrapnel_flak_placement`]): a child has
    /// no WeaponType.
    /// The constructor zeroes `+0x130`
    /// (`0x00466401`) and only SetWeaponType (`0x0046B260`, called from
    /// `0x0044CAFF`, `0x006CDB7E`, `0x006E34E6`, `0x006FE573` and
    /// `0x0070D764`) writes it, so a `FlakScatter=` child would fault reading
    /// its `Range=` in Fire's flak placement (`0x004687E5`). VERA stands it on
    /// its target. Trigger: a shrapnel weapon whose projectile is `Inviso=`
    /// and `FlakScatter=`; the five retail shrapnel weapons' projectiles are
    /// `Inviso=` without it. Effect: native crashes.
    ///
    /// [`ProjectileStore::fire_inviso`]: crate::sim::projectile::ProjectileStore::fire_inviso
    pub(crate) fn admit_projectile_spawns(
        &mut self,
        spawns: impl IntoIterator<Item = crate::sim::projectile::ProjectileSpawn>,
        rules: &RuleSet,
    ) {
        for projectile in spawns {
            let stable_id = self.substrate.allocate_stable_id();
            let inviso = projectile
                .payload
                .projectile_type()
                .and_then(|id| self.interner.projectile_type(id, rules))
                .is_some_and(|kind| kind.inviso);
            let placement = projectile.initial_target_position;
            self.admit_projectile(stable_id, projectile);
            self.unlimbo_alpha_shape(stable_id, ObjectKind::Projectile, rules);
            Self::construct_bullet_scheme(
                &self.interner,
                &self.substrate,
                &self.houses,
                &mut self.projectiles,
                stable_id,
                rules,
            );
            #[cfg(test)]
            if let Some(fixture) = self.receiver_fixture.as_mut() {
                fixture
                    .admitted_spawns
                    .push((stable_id, inviso, projectile));
            }
            if inviso {
                let placement = if rules.ares() {
                    let placement = self.ares_shrapnel_flak_placement(stable_id, placement, rules);
                    // Ares 1001A940 (hook 0x4688BD): a firestorm wall on the
                    // way burns the child before the obstacle walk.
                    if self.ares_inviso_firestorm(stable_id, placement, rules) {
                        continue;
                    }
                    let placement = self.ares_inviso_obstacle(stable_id, placement, rules);
                    placement
                } else {
                    placement
                };
                self.projectiles.fire_inviso(stable_id, placement, false);
            }
        }
    }

    pub fn admit_wave(&mut self, stable_id: u64, wave: crate::sim::wave::Wave) -> u64 {
        self.waves.spawn(stable_id, wave);
        let registered = self.register_wave(stable_id);
        debug_assert!(registered);
        stable_id
    }

    // Native Reveal's append: +0x98 guard → tail-append → set flag. Idempotent.
    // Reveal/Conceal and raw LogicVector transactions live in lifecycle.rs.

    /// Debug-only invariant: the active order and each stored object's
    /// membership flag are two views of one set and must never disagree. The
    /// order must be duplicate-free, and its length must equal the number of
    /// objects whose `in_logic_vector` is set. O(n); compiled out of release.
    #[cfg(debug_assertions)]
    pub(crate) fn debug_assert_logic_membership_consistent(&self) {
        let order = self.substrate.logic.as_slice();
        let mut seen = std::collections::BTreeSet::new();
        for &id in order {
            let first_occurrence = seen.insert(id);
            debug_assert!(first_occurrence, "logic order has duplicate id {id}");
            debug_assert!(
                self.classify_object(id)
                    .is_some_and(|kind| self.logic_membership_flag(id, kind)),
                "logic order id {id} is missing or not membership-flagged",
            );
            debug_assert!(
                self.substrate
                    .logic
                    .kind_of(id)
                    .is_none_or(|kind| self.object_store_holds(kind, id)),
                "logic order id {id} notes a kind whose store does not hold it",
            );
        }
        let mut flagged = 0;
        for kind in ObjectKind::ALL {
            flagged += match kind {
                ObjectKind::Entity => self
                    .substrate
                    .entities
                    .values()
                    .filter(|entity| entity.seldom.in_logic_vector.is_member())
                    .count(),
                ObjectKind::Anim => self
                    .substrate
                    .anims
                    .iter()
                    .filter(|(_, anim)| anim.in_logic_vector())
                    .count(),
                ObjectKind::VoxelAnim => self
                    .substrate
                    .voxel_anims
                    .iter()
                    .filter(|(_, debris)| debris.in_logic_vector())
                    .count(),
                ObjectKind::Spotlight => self
                    .substrate
                    .spotlights
                    .iter()
                    .filter(|(_, light)| light.in_logic_vector())
                    .count(),
                ObjectKind::ParticleSystem => self
                    .substrate
                    .particle_systems
                    .iter()
                    .filter(|(_, system)| system.in_logic_vector())
                    .count(),
                ObjectKind::Terrain => self
                    .terrain_objects
                    .objects()
                    .values()
                    .filter(|terrain| terrain.in_logic_vector())
                    .count(),
                ObjectKind::Projectile => self
                    .projectiles
                    .iter()
                    .filter(|(_, projectile)| projectile.in_logic_vector())
                    .count(),
                ObjectKind::Wave => self
                    .waves
                    .iter()
                    .filter(|(_, wave)| wave.in_logic_vector())
                    .count(),
                ObjectKind::Smudge | ObjectKind::Particle => 0,
            };
        }
        debug_assert_eq!(
            order.len(),
            flagged,
            "logic order length ({}) != objects flagged in_logic_vector ({})",
            order.len(),
            flagged
        );
    }

    /// Release test builds have no debug assertions, so the check is a no-op
    /// there, like the `debug_assert!`s it holds. Tests call it without a cfg.
    #[cfg(all(test, not(debug_assertions)))]
    pub(crate) fn debug_assert_logic_membership_consistent(&self) {}

    /// Debug-only checks for relationships that remain true while the native
    /// lifecycle axes themselves are deliberately independent.
    #[cfg(debug_assertions)]
    pub(crate) fn debug_assert_lifecycle_consistent(&self) {
        for entity in self.substrate.entities.values() {
            if !entity.lifecycle.object_alive() {
                debug_assert!(
                    entity.lifecycle.in_limbo(),
                    "dead entity {} must have completed Conceal before alive clear",
                    entity.stable_id()
                );
                debug_assert!(
                    !entity.lifecycle.cell_marked(),
                    "dead entity {} must be unmarked before alive clear",
                    entity.stable_id()
                );
                debug_assert!(
                    !entity.seldom.in_logic_vector.is_member(),
                    "dead entity {} must leave LogicVector before alive clear",
                    entity.stable_id()
                );
                debug_assert!(
                    entity.seldom.kill_record.is_recorded(),
                    "dead entity {} must close its kill record before alive clear",
                    entity.stable_id()
                );
                debug_assert!(
                    self.substrate
                        .pending_delete
                        .contains_object(&entity.stable_id()),
                    "dead entity {} must remain pending until finalization",
                    entity.stable_id()
                );
            }
            if !entity.lifecycle.cell_marked()
                || entity.passenger_role().is_inside_transport()
                // A Jumpjet's hold and cruise (states 2 and 3) move it with
                // no Mark pair (0x0054D0FF..0x0054D134), so one flown onto a
                // cliff answers Ground with no list entry, in gamemd too.
                || entity
                    .locomotor
                    .as_ref()
                    .and_then(|locomotor| locomotor.jumpjet_runtime())
                    .is_some_and(|jumpjet| matches!(jumpjet.phase(), 2 | 3))
            {
                continue;
            }
            // A marked Foot sits where its Mark put it: the Mark owner's own
            // list decision, which the Ground answer makes rules-independent.
            let listed = if entity.category() == EntityCategory::Structure {
                if crate::sim::occupancy::cell_list_layer_for_entity(
                    entity,
                    self.resolved_terrain.as_ref(),
                )
                .is_some()
                {
                    crate::sim::occupancy::entity_occupancy_cells(entity)
                } else {
                    Vec::new()
                }
            } else {
                self.foot_mark_cell(entity.stable_id())
                    .map(|(cell, _)| vec![cell])
                    .unwrap_or_default()
            };
            for (rx, ry) in listed {
                debug_assert!(
                    self.substrate
                        .occupancy
                        .contains_entity(rx, ry, entity.stable_id()),
                    "cell-marked entity {} missing occupancy at ({rx}, {ry})",
                    entity.stable_id()
                );
            }
        }
    }

    /// The active order, verbatim. No sorted-ID fallback (was DRIFT).
    ///
    /// This is a point-in-time copy: a consumer iterating it CANNOT observe an
    /// object registered or unregistered during its own pass. For native
    /// same-pass membership semantics use [`Self::for_each_live_object`].
    pub fn live_object_order_snapshot(&self) -> Vec<u64> {
        self.substrate.logic.snapshot()
    }

    /// Forward pass over the active-object order that RE-READS the live length
    /// after every body call.
    ///
    /// Consequences (the native scheduler contract):
    /// - An object the body tail-appends via `register_live_object` runs later
    ///   in the SAME pass (the length grows before the cursor reaches it).
    /// - A compacting `unregister_live_object` shifts successors left while the
    ///   cursor still advances by one, so the object pulled into the just-
    ///   processed slot is skipped this pass. There is no index repair.
    /// - Each member is visited at most once per pass; the index only advances.
    ///
    /// The body must tolerate an id whose entity is absent — there is no item
    /// guard here. `uninit` always conceals before freeing the store
    /// slot, so the order never references a removed entity in practice.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn for_each_live_object<F: FnMut(&mut Simulation, u64)>(&mut self, mut body: F) {
        let result: Result<(), std::convert::Infallible> =
            self.try_for_each_live_object(|sim, id| {
                body(sim, id);
                Ok(())
            });
        match result {
            Ok(()) => {}
            Err(never) => match never {},
        }
    }

    /// The same live cursor, with failure stopping before the next object.
    /// Prior callback writes remain in the world; this is not a rollback.
    pub(crate) fn try_for_each_live_object<E>(
        &mut self,
        mut body: impl FnMut(&mut Simulation, u64) -> Result<(), E>,
    ) -> Result<(), E> {
        let mut i = 0;
        while i < self.substrate.logic.len() {
            let logic = self.substrate.logic.as_slice();
            let id = logic[i];
            // VERA-only (sim-scale, results unchanged): the records of a
            // large battle do not fit the cache, so ask for the one a few
            // slots on now and its turn finds it loaded (`util::prefetch`).
            if let Some(&ahead) = logic.get(i + WARM_AHEAD) {
                self.substrate.entities.warm(ahead);
            }
            #[cfg(any(test, feature = "test-support"))]
            let _draw_context = crate::sim::rng::observe_logic_object(id);
            body(self, id)?;
            i += 1;
        }
        Ok(())
    }

    /// Debug-only P5b invariants on the now-authoritative registry (repurposed from the
    /// P5a inversion-readiness assert — the legacy upfront charge it compared against is
    /// retired, so the comparison is gone). Read-only; SURFACES divergence with
    /// tick+owner+category; NEVER writes back.
    #[cfg(debug_assertions)]
    pub(crate) fn debug_assert_factory_invariants(&self) {
        use crate::sim::production::PRODUCTION_STEPS;

        // (A) ORDER: the registry sweep order must be a TOTAL order — `iter_insertion_ordered`
        // yields strictly-increasing `insertion_seq` with NO ties (the strict-monotonic
        // `enqueue_order` property the hash fold + `step_all` charge order both depend on; a
        // tie would make the sweep order ambiguous and desync lockstep). Within each factory
        // the tail stamps strictly increase AND exceed the factory's `insertion_seq`, its
        // construction stamp (FIFO `push_back` of a monotonic mint: every queued build was
        // stamped after the factory was made, and a promotion keeps the factory's stamp).
        let ordered = self.production.factories().iter_insertion_ordered();
        let mut prev_seq: Option<u64> = None;
        for f in &ordered {
            if let Some(p) = prev_seq {
                debug_assert!(
                    f.insertion_seq() > p,
                    "P5d (A): tick {}: insertion_seq must be strictly increasing across the sweep ({} after {})",
                    self.session.tick(),
                    f.insertion_seq(),
                    p,
                );
            }
            prev_seq = Some(f.insertion_seq());
            let mut tail_prev = f.insertion_seq();
            for e in f.queue() {
                debug_assert!(
                    e.enqueue_order > tail_prev,
                    "P5d (A): tick {} {:?}/{:?}: tail enqueue_order must strictly exceed the active build + prior tail ({} after {})",
                    self.session.tick(),
                    f.owner(),
                    f.category(),
                    e.enqueue_order,
                    tail_prev,
                );
                tail_prev = e.enqueue_order;
            }
        }

        // (B) STATE: progress in 0..=54; balance >= 0 (the seed is non-negative, the
        // per-step ladder only decrements it, and cancel resets it to 0).
        for f in self.production.factories().iter_insertion_ordered() {
            debug_assert!(
                f.progress() <= PRODUCTION_STEPS,
                "P5b (B): tick {} {:?}/{:?}: progress {} exceeds {}",
                self.session.tick(),
                f.owner(),
                f.category(),
                f.progress(),
                PRODUCTION_STEPS,
            );
            debug_assert!(
                f.balance() >= 0,
                "P5b (B): tick {} {:?}/{:?}: balance {} is negative",
                self.session.tick(),
                f.owner(),
                f.category(),
                f.balance(),
            );
        }
    }

    /// Test-only: force the active order and sync membership flags to it.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_logic_order_for_test(&mut self, order: Vec<u64>) {
        self.substrate.logic.set_order_for_test(order);
        self.rebuild_logic_membership();
    }

    /// Common VoxelAnim7493B0 constructor/admission for typed Techno and
    /// Bullet debris, trigger action 43 and meteor children. Create_ID74944E
    /// precedes the launch and Bounce4397E0 RNG; admission finishes before
    /// the next typed selector.
    /// `admit_inline=false` is the caller's existing no-callback numeric-fixture
    /// boundary: its packet retains both identities for later registration.
    pub fn emit_debris_voxel(
        &mut self,
        type_id: crate::rules::voxel_anim_type::VoxelAnimTypeId,
        voxel_type: &crate::rules::voxel_anim_type::VoxelAnimType,
        owner_house: Option<InternedId>,
        origin: glam::IVec3,
        admit_inline: bool,
        packets: &mut Vec<crate::sim::voxel_anim::VoxelDebrisSpawn>,
        rules: &RuleSet,
    ) -> Result<(), crate::util::native_x87::NativeX87Error> {
        let stable_id = self.substrate.allocate_stable_id();
        let native_unique_id = self.next_native_runtime_id();
        let object = crate::sim::voxel_anim::spawn_debris_piece(
            stable_id,
            native_unique_id,
            type_id,
            voxel_type,
            owner_house,
            origin,
            &mut self.scenario_rng,
        )?;
        let attached_system = voxel_type
            .attached_system
            .as_deref()
            .and_then(|name| rules.ps_type_id_by_name(name))
            .map(|system_type| (system_type, origin));
        let spawn = crate::sim::voxel_anim::VoxelDebrisSpawn {
            object,
            attached_system,
        };
        if admit_inline {
            self.admit_death_debris(std::iter::once(spawn), rules);
        } else {
            packets.push(spawn);
        }
        Ok(())
    }

    /// Register already-constructed debris. This also delivers numeric-fixture
    /// packets; neither stable nor native identity is allocated a second time.
    pub(crate) fn admit_death_debris(
        &mut self,
        spawns: impl IntoIterator<Item = crate::sim::voxel_anim::VoxelDebrisSpawn>,
        rules: &RuleSet,
    ) {
        for spawn in spawns {
            let stable_id = spawn.object.stable_id();
            self.substrate.voxel_anims.insert(spawn.object);
            if self.reveal_voxel_anim(stable_id) {
                self.unlimbo_alpha_shape(stable_id, ObjectKind::VoxelAnim, rules);
            }
            // The constructor's tail (`0x00749864..0x007498BD`) builds the
            // type's AttachedSystem at the input location, aimed at the cell
            // there (Map565730), owned by the anim, with zero target coords
            // (`0x00B1D188`) and no House, and keeps the result at +0x108.
            // RESIDUAL: off the map, Map565730 gives the shared dummy cell to
            // aim at; VERA passes no target, so the target coords stay zero.
            if let Some((system_type, origin)) = spawn.attached_system {
                let target = Self::map_coord_cell(&self.resolved_terrain, origin.x, origin.y)
                    .map(|(x, y)| crate::sim::combat::TargetKind::Cell(x, y));
                let system = self.spawn_particle_system(
                    system_type,
                    origin,
                    target,
                    Some(stable_id),
                    glam::IVec3::ZERO,
                    None,
                    rules,
                );
                if let Some(piece) = self.substrate.voxel_anims.get_mut(stable_id) {
                    piece.bind_attached_system(system);
                }
            }
        }
    }
}
