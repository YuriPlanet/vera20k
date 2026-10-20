//! Per-object AI dispatch host (TechnoClass/FootClass spine).
//!
//! Walks the substrate's live-object order and dispatches each live object
//! through a per-`EntityCategory` shell. Since the Mission authority flip the
//! shell owns the native per-object Mission work: the `+0xC4` AI counter and
//! the owner-local queued-mission promotion (Ready→Commence) that every
//! per-category AI update performs. The mission handler *bodies* remain the
//! legacy per-system state machines (movement, combat, miner, aircraft, …)
//! running in their existing phases — absorbing them into this walk is the
//! remaining per-arm work.
//!
//! Depends on: `world::Simulation` (substrate live order + entity store).
//! Must NOT depend on render/ui/audio/net (sim invariant #1).
//! Dispatch is `match category` only — no trait object / dyn / vtable
//! (invariant #2).

mod aircraft_guard;
mod building_missile;
mod building_missions;
mod building_retaliation;
#[cfg(any(test, feature = "test-support"))]
pub(crate) use building_missions::PendingBuildingFire;
pub(crate) use building_missions::PrismSupport;
pub(crate) use building_missions::queue_and_commence;
pub use building_missions::{BuildingDelayedFire, DelayedFire};
mod mission_handlers;
mod scan_batch;
mod target_scan;
pub(crate) use mission_handlers::dispatch_foot_mission;
pub(crate) use mission_handlers::enter_idle_mode;
pub(crate) use mission_handlers::foot_enter_idle_mode_selection;
pub(crate) use mission_handlers::techno_unlimbo_idle_mode;
#[cfg(any(test, feature = "test-support"))]
use mission_handlers::*;
pub(crate) use scan_batch::ScanBatch;
pub(crate) use target_scan::PassiveScan;
pub(crate) use target_scan::direct_greatest_threat;
pub(crate) use target_scan::passive_target_acquire;
pub(crate) use target_scan::scan as retaliate_and_scan;
pub(crate) use target_scan::{can_fire_at, weapon_at_index};

use target_scan::{can_acquire_target, passive_acquire_step};

use super::{FrameEffects, ObjectSubstrate, Simulation};
use crate::map::entities::EntityCategory;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::StringInterner;
use crate::sim::miner::MinerConfig;
use crate::sim::mission::MissionType;
use crate::sim::scenario_session::ScenarioSession;

/// Non-rules world context the mission handler bodies dispatched from the
/// host need (overlay types and per-tick config the spine already owns;
/// handlers read navigation from the Simulation). Empty in
/// barebones fixtures — handlers that need an absent piece degrade the same
/// way the legacy global phases did with `None` arguments.
#[derive(Default, Clone, Copy)]
pub(crate) struct ObjectAiCtx<'a> {
    pub(crate) effects: super::FrameEffects<'a>,
    pub(crate) overlay_registry: Option<&'a OverlayTypeRegistry>,
    pub(crate) terrain_spawner_cells: Option<&'a std::collections::BTreeSet<(u16, u16)>>,
    pub(crate) miner_config: Option<&'a MinerConfig>,
}

/// Ephemeral outputs of a single live AI visit, forwarded to the frame result.
#[derive(Default)]
pub(super) struct ObjectAiOutcome {
    #[cfg(any(test, feature = "test-support"))]
    pub(super) visited: bool,
    pub(super) bridge_state_changed: bool,
    pub(super) destroyed_structure: bool,
    ownership_changed: bool,
    pub(super) landed: bool,
    pub(super) fall_error: Option<super::FrameAdvanceError>,
}

impl ObjectAiOutcome {
    pub(super) fn visited() -> Self {
        Self {
            #[cfg(any(test, feature = "test-support"))]
            visited: true,
            ..Default::default()
        }
    }

    pub(super) fn ownership_changed(&self) -> bool {
        self.ownership_changed
    }
}

impl Simulation {
    /// The existing Techno+324 link, including its native dying/deferred
    /// interval. Readers must not reconstruct it from the Wave registry.
    pub fn active_wave_id(&self, owner_id: u64) -> Option<u64> {
        self.active_wave_links.get(&owner_id).copied()
    }

    pub(crate) fn clear_active_wave_link(&mut self, wave_id: u64) {
        self.active_wave_links
            .retain(|_, linked_wave_id| *linked_wave_id != wave_id);
    }

    pub(crate) fn wave_update_context(
        &self,
        wave: &crate::sim::wave::Wave,
        rules: Option<&RuleSet>,
    ) -> crate::sim::wave::WaveUpdateContext {
        use crate::sim::combat::TargetKind;
        use crate::sim::projectile::ProjectileCoord;

        // WaveClass keeps raw owner/target pointers through the represented
        // dying interval. Only PointerExpired makes either identity null.
        let owner = wave
            .owner_id()
            .and_then(|owner_id| self.substrate.entities.get(owner_id));
        // The tracking distance reads Location (`0x00762BB4`), independently
        // of the GetFLH(0, null) geometry call (`0x00762AF0`). The ordinary
        // production host supplies its current rules; rules-less test worlds
        // retain their Location-only compatibility boundary.
        let owner_position = owner.map(|entity| {
            let location = crate::sim::movement::ground_pose::object_location(
                entity,
                self.resolved_terrain.as_ref(),
            );
            ProjectileCoord::new(location.x, location.y, location.z)
        });
        let owner_current_target = owner.and_then(|entity| entity.attack_target.target());
        let owner_fire_coordinate = owner.and_then(|entity| {
            let Some(rules) = rules else {
                return owner_position;
            };
            let object = self.interner.object_type(entity.type_ref(), rules)?;
            // GetFLH(0, null) still mirrors by the owner's current burst
            // parity (+0x3B8, `0x006F3C82`), as FireAt's own call does.
            let burst_index = (entity.weapon_burst.index() & 1) as u8;
            Some(
                crate::sim::combat::fire_coord::fire_coordinate(
                    self,
                    rules,
                    &crate::sim::combat::fire_coord::FireSource::of_entity(entity),
                    object,
                    0,
                    burst_index,
                    crate::rules::flh::Flh::default(),
                )
                .coord,
            )
        });
        // The target's vt+0x58 (`0x00762BD1`, `0x00762D2C`), which an object
        // forwards to its GetCoords (`0x00410540`).
        let target_position = match wave.target_ref() {
            Some(target @ TargetKind::Entity(_)) => {
                crate::sim::movement::ground_pose::target_get_target_coords(
                    target,
                    self,
                    self.resolved_terrain.as_ref(),
                )
                .map(|coords| ProjectileCoord::new(coords.x, coords.y, coords.z))
            }
            Some(TargetKind::Cell(rx, ry)) => Some(self.wave_cell_target_position(rx, ry)),
            None => None,
        };
        let target_is_unit = match wave.target_ref() {
            Some(TargetKind::Entity(id)) => self
                .substrate
                .entities
                .get(id)
                .is_some_and(|entity| entity.category() == EntityCategory::Unit),
            _ => false,
        };
        crate::sim::wave::WaveUpdateContext {
            owner_position,
            owner_fire_coordinate,
            owner_current_target,
            target_position,
            target_is_unit,
        }
    }
}

impl Simulation {
    /// Object-AI stage: the authoritative per-object Mission host.
    ///
    /// Walks the live LogicVector order via `for_each_live_object` — the same
    /// re-read contract the native scheduler uses. Every present slot receives
    /// its owner-local visit: Terrain, Bullet, Wave, Anim, and ParticleSystem
    /// leaves dispatch in that slot, while ordinary Techno objects enter
    /// `techno_ai_shell` for the `+0xC4`
    /// AI-counter increment and queued-mission promotion at the verified
    /// per-category AI position (see `Simulation::mission_host_promote`).
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn object_ai_stage(&mut self, rules: Option<&RuleSet>) {
        self.object_ai_stage_with(rules, ObjectAiCtx::default());
    }

    /// The post-movement Ready→Commence checkpoint.
    ///
    /// `InfantryClass::AI` and `UnitClass::AI` each gate twice per tick, once on
    /// either side of the object's own locomotion, and both checkpoints are the
    /// same pair of virtuals — the readiness predicate at self-vtable `+0x200`
    /// followed by `Commence` at `+0x1ec` when it returns true. Our object-AI
    /// stage is the first checkpoint (it runs immediately before Phase-1 ground
    /// movement); this is the second.
    ///
    /// Without it a unit that came to rest during its own movement step could
    /// not commence until the next tick, so every mission commencement that
    /// depends on having stopped was one tick late. That compounds through
    /// chained handoffs — harvest dock/unload/exit, guard→attack — at one extra
    /// tick per stage.
    ///
    /// Unit and Infantry reach this as their second checkpoint. Aircraft reach
    /// it as their sole checkpoint: `AircraftClass::AI @ 0x00414BB0` calls
    /// `FootClass::AI` (which processes the locomotor) at `0x00414DA3`, then
    /// calls ReadyToCommence/Commence at `0x0041504A`/`0x00415058`. Buildings
    /// gate inside their own update, which is not this movement bracket.
    ///
    /// The AI counter is NOT ticked here. Native increments it once per AI pass,
    /// and the pre-movement mission step already did so for every category.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn object_ai_post_movement_promote(&mut self, rules: Option<&RuleSet>) {
        let Some(rules) = rules else {
            return;
        };
        let now = self.session.binary_frame();
        self.for_each_live_object(|sim, id| {
            if sim.substrate.anims.contains_key(id) {
                return;
            }
            let Some(entity) = sim.substrate.entities.get(id) else {
                return;
            };
            if entity.death.is_dying() {
                return;
            }
            if !matches!(
                entity.category(),
                EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
            ) {
                return;
            }
            sim.mission_host_promote(id, now, rules);
            sim.substrate.aircraft_ammo_after_commence(id);
        });
    }

    /// The post-movement Ready-to-Commence checkpoint for one object, called
    /// immediately after that object's own locomotion. This is the second
    /// checkpoint for Unit/Infantry and the sole checkpoint for Aircraft.
    pub(crate) fn object_ai_post_movement_promote_one(&mut self, id: u64, rules: Option<&RuleSet>) {
        let Some(rules) = rules else {
            return;
        };
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        if entity.death.is_dying()
            || !matches!(
                entity.category(),
                EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
            )
        {
            return;
        }
        self.mission_host_promote(id, self.session.binary_frame(), rules);
        self.substrate.aircraft_ammo_after_commence(id);
    }

    /// One `VoxelAnimClass::AI @ 0x00749F30` LogicVector slot.
    ///
    /// The piece is taken out of the store for the visit so the integrator can
    /// read the live terrain grid through `&self` while it mutates the body,
    /// then reinserted at the same key — `BTreeMap` order is unchanged.
    ///
    /// A `Delete` verdict is native's `vtable+0xF8`. Native queues the free on
    /// the deferred finalization list; here the object leaves the LogicVector
    /// and the store in the same step, because nothing resolves debris by id
    /// between the two points.
    pub fn visit_voxel_anim(&mut self, id: u64, rules: Option<&RuleSet>) {
        let Some(mut debris) = self.substrate.voxel_anims.remove(id) else {
            return;
        };
        let outcome = {
            let terrain = bounce_terrain::ResolvedBounceTerrain { sim: self, rules };
            crate::sim::voxel_anim::voxel_anim_ai(&mut debris, &terrain)
        };
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(error) => {
                log::warn!("voxel debris {id} left the verified x87 domain: {error}");
                crate::sim::voxel_anim::VoxelAnimAiOutcome::Delete
            }
        };
        // Reinsert before the verdict is acted on: the LogicVector dispatch
        // classifies by store membership, and native's object is likewise still
        // registered when `vtable+0xF8` runs.
        self.substrate.voxel_anims.insert(debris);
        if outcome == crate::sim::voxel_anim::VoxelAnimAiOutcome::Delete {
            self.unregister_non_entity_object(id);
            self.substrate.voxel_anims.remove(id);
        }
    }

    /// `BounceClass::Update @ 0x00439B00` for an `AnimClass` chunk's body over
    /// the live terrain. A body that leaves the verified x87 domain lands.
    pub(crate) fn anim_bounce_update(
        &self,
        body: &mut crate::sim::bounce::BounceState,
        rules: &RuleSet,
    ) -> crate::sim::bounce::BounceOutcome {
        let terrain = bounce_terrain::ResolvedBounceTerrain {
            sim: self,
            rules: Some(rules),
        };
        body.update(&terrain).unwrap_or_else(|error| {
            log::warn!("bouncing anim left the verified x87 domain: {error}");
            crate::sim::bounce::BounceOutcome::Stopped
        })
    }

    /// The water test a landing chunk makes: the cell's LandType is Water
    /// (`CellClass+0xEC == 2`, `AnimClass::AI 0x00423C75`).
    pub(crate) fn bounce_cell_is_water(&self, coord: glam::IVec3, rules: &RuleSet) -> bool {
        use crate::sim::bounce::BounceTerrain;
        bounce_terrain::ResolvedBounceTerrain {
            sim: self,
            rules: Some(rules),
        }
        .is_water(coord)
    }

    /// Dispatch one current LogicVector slot. A finishing death sequence calls
    /// UnInit synchronously here, so compacting removal is visible before the
    /// scheduler increments its cursor.
    pub(super) fn object_ai_visit_one_with_effects(
        &mut self,
        id: u64,
        rules: Option<&RuleSet>,
        ctx: ObjectAiCtx<'_>,
    ) -> ObjectAiOutcome {
        let mut outcome = ObjectAiOutcome::visited();
        if self.substrate.anims.contains_key(id) {
            if let Some(rules) = rules {
                outcome.bridge_state_changed =
                    self.visit_anim(id, rules, ctx.overlay_registry, ctx.effects);
            }
            return outcome;
        }
        if self.substrate.spotlights.contains_key(id) {
            if let Some(rules) = rules {
                self.visit_spotlight(
                    id,
                    rules,
                    super::UninitContext::new(Some(rules), ctx.overlay_registry)
                        .with_effects(ctx.effects),
                );
            }
            return outcome;
        }
        if self.substrate.voxel_anims.contains_key(id) {
            self.visit_voxel_anim(id, rules);
            return outcome;
        }
        if self.substrate.particle_systems.contains_key(id) {
            if let Some(rules) = rules {
                crate::sim::particles::system_ai::tick_particle_system(
                    self,
                    rules,
                    id,
                    super::UninitContext::new(Some(rules), ctx.overlay_registry)
                        .with_effects(ctx.effects),
                );
            }
            return outcome;
        }
        // One stable id allocator numbers every store, so an entity is no
        // terrain object: its hashed lookup spares the commonest turn the
        // terrain map's.
        if !self.substrate.entities.contains(id) && self.terrain_objects.objects().contains_key(&id)
        {
            // TerrainClass::AI @ 0x0071C730 calls ObjectClass::AI first
            // (`0x0071C738`).
            self.object_ai_sounds(id, rules, ctx.effects);
            crate::sim::terrain_spawn::tick_terrain_object_ai(
                self,
                id,
                rules,
                ctx.overlay_registry,
                ctx.terrain_spawner_cells,
            );
            return outcome;
        }
        if self.projectiles.get(id).is_some() {
            let head = self
                .projectiles
                .begin_ai_visit(id)
                .expect("live Bullet has an AI prefix");
            // Bullet466826..4668B8 constructs at the old Location after its
            // visual update, before target/floor queries or flight. Registering
            // an Anim here also preserves the live Logic cursor on impact.
            if let Some(rules) = rules {
                let trailer = self
                    .projectiles
                    .get(id)
                    .and_then(|projectile| projectile.payload().projectile_type())
                    .and_then(|type_id| rules.projectile(self.interner.resolve(type_id)))
                    .and_then(|kind| {
                        self.projectiles
                            .trailer(&head, kind, self.session.binary_frame())
                    });
                if let Some((name, position)) = trailer {
                    self.spawn_named_anim(
                        rules,
                        name,
                        crate::sim::anim_class::AnimWorldCoord {
                            x: position.x,
                            y: position.y,
                            z: position.z,
                        },
                        1,
                        0x600,
                        0,
                    );
                }
            }
            let projectile = self
                .projectiles
                .get(id)
                .expect("Bullet remains registered across its Trailer constructor");
            // BulletClass::AI @ 0x00467C3C reads the live firer (+0xB0),
            // then its +0x84 TechnoType receiver and JumpJet (+0xD94).
            // Do not cache this on launch: PointerExpired can null the firer.
            let source_is_jumpjet = rules.is_some_and(|rules| {
                self.substrate
                    .entities
                    .get(projectile.source_id())
                    .is_some_and(|source| {
                        self.object_type(source.type_ref(), rules)
                            .is_some_and(|kind| kind.jumpjet)
                    })
            });
            // `BulletClass::AI 0x0046699D` reads the GLOBAL frame counter's
            // parity for the half-rate course-locked acceleration ramp, so the
            // Logic slot has to hand the flight loop the current frame.
            let target_is_aircraft = match projectile.target() {
                crate::sim::projectile::ProjectileTarget::Entity(target) => {
                    self.substrate.entities.get(target).is_some_and(|entity| {
                        entity.category() == crate::map::entities::EntityCategory::Aircraft
                    })
                }
                _ => false,
            };
            let binary_frame = self.session.binary_frame();
            let shared_cell_dummy = self.effective_shared_cell_dummy();
            let terrain = self.resolved_terrain.as_ref();
            let overlay_grid = self.overlay_grid.as_ref();
            let occupancy = &self.substrate.occupancy;
            let entities = &self.substrate.entities;
            let interner = &self.interner;
            let house_alliances = self.house_alliances.as_map();
            let collision_world = super::projectile_collision::ProjectileCollisionWorld {
                terrain,
                dummy: &shared_cell_dummy,
                occupancy,
                entities,
                terrain_objects: self.terrain_objects.objects(),
                interner,
                alliances: house_alliances,
                overlays: overlay_grid,
                overlay_registry: ctx.overlay_registry,
                rules,
                map_size: self.map_size_diamond(),
            };
            let nuke_ball_type =
                || rules.is_some_and(crate::sim::superweapon::nuke::nuke_ball_type);
            let nuke = rules
                .map(|_| crate::sim::projectile::NukeImpactContext::new(interner, &nuke_ball_type));
            let result = self
                .projectiles
                .advance_after_head(
                    head,
                    binary_frame,
                    |target, query| collision_world.target_coordinate(target, query),
                    terrain,
                    &shared_cell_dummy,
                    rules.map_or_else(
                        || crate::rules::ruleset::GeneralRules::default().gravity,
                        |rules| rules.general.gravity,
                    ),
                    source_is_jumpjet,
                    target_is_aircraft,
                    rules.map_or_else(
                        || crate::rules::ruleset::GeneralRules::default().safety_altitude,
                        |rules| rules.general.safety_altitude,
                    ),
                    |projectile, candidate, phase| {
                        collision_world.collide(projectile, candidate, phase)
                    },
                    nuke.as_ref(),
                )
                .expect("projectile remained present for its Logic slot");
            let terminal = !result.expired.is_empty() || !result.detonations.is_empty();
            // A rules-less fixture dispatch has no receiver, so its
            // detonations are not committed.
            if let Some(rules) = rules {
                // The rest of a `NUKE` impact precedes the slot's detonations.
                for &impact in result.nuke_impacts() {
                    crate::sim::superweapon::nuke::impact(self, rules, impact);
                }
                outcome.bridge_state_changed |= self.commit_logic_projectile_detonations(
                    rules,
                    ctx.overlay_registry,
                    &result.detonations,
                    ctx.effects,
                );
            }
            if terminal {
                let retired = self.retire_non_entity_object(
                    id,
                    super::UninitContext::new(rules, ctx.overlay_registry)
                        .with_effects(ctx.effects),
                );
                debug_assert!(retired);
            }
            return outcome;
        }
        if self.waves.get(id).is_some() {
            let context =
                self.wave_update_context(self.waves.get(id).expect("represented Wave"), rules);
            let terrain = self.resolved_terrain.as_ref();
            let (request, result) = self
                .waves
                .advance_one(id, context, terrain)
                .expect("wave remained present for its Logic slot");
            // Fade-terminal UnInit dispatches exact pointer expiry before the
            // stale AI-20 cell vector is damaged on AI-21.
            if result.uninitialized {
                self.clear_active_wave_link(id);
            }
            // As for a Bullet, a rules-less fixture dispatch commits nothing.
            if let Some(request) = request
                && let Some(rules) = rules
            {
                self.commit_logic_wave_damage_request(
                    rules,
                    ctx.overlay_registry,
                    &request,
                    ctx.effects,
                );
            }
            if !result.alive {
                let retired = self.retire_non_entity_object(
                    id,
                    super::UninitContext::new(rules, ctx.overlay_registry)
                        .with_effects(ctx.effects),
                );
                debug_assert!(retired);
            }
            return outcome;
        }

        // Building43FB20 samples its operational edge before delayed Health0
        // cleanup. A live dying-animation diversion must not skip that edge.
        if let Some(rules) = rules {
            self.visit_building_operational(id, rules, ctx.overlay_registry, ctx.effects);
            // `UnitClass::AI` opens with an offline Unit's sparks
            // (`0x007360C9..0x007361A4`), ahead of its tube and Temporal tests.
            if self
                .substrate
                .entities
                .get(id)
                .is_some_and(|entity| entity.category() == EntityCategory::Unit)
            {
                self.deactivated_unit_sparks(id, rules);
            }
        }
        let Some(entity) = self.substrate.entities.get(id) else {
            return ObjectAiOutcome::default();
        };
        // An infantry corpse is no dying object: it keeps IsAlive at Health 0
        // and takes the ordinary visit below (`world::infantry_death`).
        if entity.death.is_dying() {
            let Some(rules) = rules else {
                return outcome;
            };
            let type_ref = entity.type_ref();
            let type_name = self.interner.resolve(type_ref);
            let sequence_set = rules.animation_sequence(type_name);
            let finished = crate::sim::animation::tick_dying_animation(
                self.substrate
                    .entities
                    .get_mut(id)
                    .expect("dying object remained present"),
                sequence_set,
                self.session.game_options(),
                self.session.binary_frame(),
            );
            if finished {
                self.uninit_with_context(
                    id,
                    super::UninitContext::new(Some(rules), ctx.overlay_registry)
                        .with_effects(ctx.effects),
                );
            }
            return outcome;
        }

        // UnitClass::AI / InfantryClass::AI test the object-owned TubeMovement
        // index before entering the ordinary Foot/mission body.  The active
        // leaf runs later in this same LogicVector slot (the world host owns
        // the mutable movement substrates), so this visit must contribute no
        // mission-counter, queued-mission, cadence, or passive-target work.
        if matches!(
            entity.category(),
            EntityCategory::Unit | EntityCategory::Infantry
        ) && entity.low_bridge_tube_state.is_some()
        {
            return outcome;
        }

        let category = entity.category();
        techno_ai_shell(self, id, category, rules, ctx, &mut outcome);
        outcome
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn object_ai_visit_one(
        &mut self,
        id: u64,
        rules: Option<&RuleSet>,
        ctx: ObjectAiCtx<'_>,
    ) -> bool {
        self.object_ai_visit_one_with_effects(id, rules, ctx)
            .visited
    }

    /// [`Simulation::object_ai_stage`] with the world context the dispatched
    /// mission handler bodies need (the production spine entry).
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn object_ai_stage_with(&mut self, rules: Option<&RuleSet>, ctx: ObjectAiCtx<'_>) {
        let visited = self.object_ai_walk(cfg!(debug_assertions), rules, ctx);

        #[cfg(debug_assertions)]
        debug_assert_eq!(
            visited
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            visited.len(),
            "object_ai_stage visited one live object more than once",
        );

        #[cfg(not(debug_assertions))]
        let _ = visited;
    }

    /// The walk: visit every live, present object slot once in live order.
    /// Objects on the SHP death sequence (the dying mark) retain their slot
    /// while it runs but do not enter the ordinary per-category shell; an
    /// infantry corpse does. When `record`, return the
    /// visited ids in order (debug/test observation); otherwise the returned
    /// `Vec` is empty and unallocated.
    #[cfg(any(test, feature = "test-support"))]
    fn object_ai_walk(
        &mut self,
        record: bool,
        rules: Option<&RuleSet>,
        ctx: ObjectAiCtx<'_>,
    ) -> Vec<u64> {
        let mut visited: Vec<u64> = Vec::new();
        self.for_each_live_object(|sim, id| {
            if sim.object_ai_visit_one(id, rules, ctx) && record {
                visited.push(id);
            }
        });
        visited
    }
}

impl crate::sim::world::ObjectSubstrate {
    /// AircraftAI41505E..415085 consumes a pending release after Ready/Commence,
    /// using the resulting native Mission+AC, including when an order interrupts
    /// Attack. It neither reads nor clears the separate readiness latch+6D2.
    fn aircraft_ammo_after_commence(&mut self, id: u64) {
        let Some(entity) = self.entities.get_mut(id) else {
            return;
        };
        if entity.category() == EntityCategory::Aircraft
            && entity.mission.current().raw() != 1
            && let Some(ammo) = entity.ammo_mut()
        {
            ammo.consume_release(false);
        }
    }
}

/// Per-category dispatch shell.
///
/// `match category` — NO trait / dyn / vtable (invariant #2). Every arm runs
/// the common per-object Mission work (`+0xC4` counter + owner-local queued
/// promotion). Unit and Infantry run the common Techno steps; movement,
/// Infantry fear/fire/sequence and aircraft dispatch retain their ordered
/// owners in the surrounding object-turn transaction.
/// The match is exhaustive over the four real variants (no `_` arm), so a
/// future `EntityCategory` addition is a compile error, intentionally.
fn techno_ai_shell(
    sim: &mut Simulation,
    id: u64,
    category: EntityCategory,
    rules: Option<&RuleSet>,
    ctx: ObjectAiCtx<'_>,
    outcome: &mut ObjectAiOutcome,
) {
    // Each leaf runs its chain head's TemporalClass::Update first (Unit
    // 0x00736204, Infantry 0x0051BB6E, Aircraft 0x00414BDB; a Building after
    // its damage fire at 0x0043FC39, 0x0043FCF9) and a warped object returns
    // before the common Techno body. VERA's object turn then skips its
    // locomotor for the same reason.
    // `BuildingClass::Update`'s turret-rotation sample
    // (reference/Steam `0x0043FCC7..0x0043FCF9`) is taken after damage fire,
    // before `TemporalClass::Update` and the `TechnoClass::AI_Update` call at
    // `0x0043FE56`, whose early block consumes it. Only a structure writes it.
    let mut turret_rotate_sample = false;
    if let Some(rules) = rules {
        if category == EntityCategory::Structure {
            sim.update_building_damage_fire(id, rules);
            turret_rotate_sample = sim.sample_building_turret_rotation(id, rules);
        }
        if sim.temporal_ai_prologue(id, rules, ctx, &mut outcome.bridge_state_changed) {
            return;
        }
    }
    // Unit7363C9..7363DE precedes Hunt/Ready/Foot AI, after the existing
    // Tube, death and Temporal entry gates. Limboed Units have no Logic visit.
    if category == EntityCategory::Unit
        && let Some(entity) = sim.substrate.entities.get_mut(id)
    {
        crate::sim::animation::tick_unit_firing_counter(entity);
    }
    // Unit736465/736473 and Infantry51BC1C/51BC51 run Ready/Commence
    // before Foot -> Techno AI. Commence clears+C4; Techno6FA646..655 then
    // increments it and dispatches. Native Anytown mission_order first visit
    // ends at counter1. Promoting at the dispatch point erased that increment
    // and let Techno's preceding readers observe the previous mission.
    if matches!(category, EntityCategory::Unit | EntityCategory::Infantry)
        && let Some(rules) = rules
    {
        // `UnitClass::AI`'s Hunt queue (`0x007363DE..0x0073645B`) comes right
        // before its Ready/Commence.
        if category == EntityCategory::Unit {
            crate::sim::mcv_deploy::ai_queue_hunt(sim, id, rules);
        }
        sim.mission_host_promote(id, sim.session.binary_frame(), rules);
    }
    // `InfantryClass::AI`'s Health reset (`0x0051BC57`) precedes
    // `FootClass::AI`, and with it the Techno body below.
    if category == EntityCategory::Infantry {
        sim.substrate.infantry_health_reset(id);
    }
    // Techno6F9F6E..9F precedes veterancy promotion, missions and acquisition for every
    // Techno category. object_ai_visit_one excludes the entry-active Tube leaf
    // before this common owner. Actual health and this retained estimate differ.
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity
            .estimated_health
            .recover(entity.health.current(), sim.session.binary_frame());
    }
    match category {
        EntityCategory::Unit => {
            unit_techno_bracket(sim, id, rules, ctx, outcome);
        }
        // InfantryClass::AI promotes queued missions via Ready→Commence
        // (`0x0051BC51`/`0x0051BF03`); fear, fire and sequence run in
        // the later Infantry object-turn slots, through their shared owners.
        // Infantry reach the common Techno AI body through the same foot-leaf
        // call units do, so they run the same off-mission clear and passive
        // block in the same order. Idle infantry are the majority of on-map
        // objects; without this a squad holding a chokepoint does nothing until
        // something shoots it. A passenger inside a closed transport is gated
        // out by the mission read, an open-topped transport's rider scans from
        // the Guard its boarding gave it (`SetInOpenTransport 0x00710470`), and
        // a garrisoned occupant's weapon is fired by its building
        // (`BuildingClass::GetWeapon @ 0x004526F0`).
        // The supported Foot mission cadence branches run here as well.
        EntityCategory::Infantry => {
            if let Some(rules) = rules
                && !techno_common_steps(sim, id, rules, ctx.overlay_registry, ctx.effects, None)
            {
                return;
            }
            clear_passive_target_off_mission(sim, id, rules);
            if !mission_counter_step(sim, id, rules, ctx, outcome) {
                return;
            }
            if let Some(rules) = rules {
                outcome.bridge_state_changed |= dispatch_foot_mission(sim, id, rules, ctx);
            }
            passive_acquire_step(sim, id, rules, ctx);
            if !techno_after_acquisition(sim, id, rules, ctx, outcome) {
                return;
            }
            if let Some(rules) = rules {
                target_reach_step(sim, id, rules, ctx.overlay_registry, ctx.effects);
            }
            techno_common_post(sim, id, rules);
        }
        EntityCategory::Structure => {
            if let Some(rules) = rules {
                // Building43FC39 damage-fire (run above, before the warp
                // update) and43FD2C ProduceCash precede the shared Techno call
                // at43FE56. A yellow-crossing selfheal must not retire damage
                // fire one native frame early.
                crate::sim::credit_income::produce_cash_step(sim, id, rules);
                sim.update_building_absorb_anim(id, rules);
                sim.update_building_storage_anims(id, rules);
            }
            let airstrike_active = sim.airstrike_tint_active(id);
            if let Some(entity) = sim.substrate.entities.get_mut(id) {
                entity.building_art.capture_tint_sample(
                    sim.session.binary_frame(),
                    entity.flashing.flashed(),
                    entity.invulnerability.as_ref(),
                    &entity.airstrike_tint,
                    airstrike_active,
                );
            }
            building_missions::update_animation(sim, id, rules);
            // The ready check after UpdateAnimation (`0x0043FE27`).
            building_missions::ready_commence(&sim.session, &mut sim.substrate, id, true);
            if let Some(rules) = rules
                && !techno_common_steps(
                    sim,
                    id,
                    rules,
                    ctx.overlay_registry,
                    ctx.effects,
                    Some(turret_rotate_sample),
                )
            {
                return;
            }
            // Buildings run the SAME common Techno AI body units do: the
            // off-mission clear, the `+0xC4` count, the mission dispatch and
            // then the passive block, which is how a defence on Guard picks
            // its targets. A building commences a queued mission only at
            // Update's ready checks, never here.
            clear_passive_target_off_mission(sim, id, rules);
            if !mission_counter_step(sim, id, rules, ctx, outcome) {
                return;
            }
            building_missions::dispatch(sim, id, rules, ctx, &mut outcome.bridge_state_changed);
            if sim
                .substrate
                .entities
                .get(id)
                .is_none_or(|entity| entity.death.is_dying())
            {
                return;
            }
            passive_acquire_step(sim, id, rules, ctx);
            if !techno_after_acquisition(sim, id, rules, ctx, outcome) {
                return;
            }
            // The Gattling block after the Techno AI (`0x0043FE5B..0x0043FF8B`).
            flash_step(&mut sim.substrate, id);
            if let Some(rules) = rules {
                building_missions::gattling_idle(sim, id, rules);
            }
            // The ready check after the Techno AI (`0x0043FF91`).
            building_missions::ready_commence(&sim.session, &mut sim.substrate, id, false);
            building_missions::apply_queued_body(&sim.session, &mut sim.substrate, id);
            // Building440042..440074 samples Health AFTER its queued body
            // and BEFORE delayed fire/paid repair. Engineer701410 itself
            // deliberately does not invalidate the House.
            Simulation::sample_building_health_for_house_update(
                &mut sim.substrate,
                &sim.houses,
                &mut sim.power_states,
                id,
            );
            // Building440072..4400F1 retires a building at Health 0 once its
            // shared C4/death fuse has run out: one that outlived its killing
            // blow (an `Explodes=` type or one dying while Selling, whose
            // DestructionEffects armed a zero fuse, `0x00441C43`) and a zero-HP
            // map building that never entered ReceiveDamage (SOV01's collapsed
            // CASANF10; its constructor's fuse has duration0). An armed fuse
            // with time left holds it. Then Limbo -> SpawnSurvivors (a second
            // round for the former, under the NoSurvivor its killing blow
            // left) -> UnInit -> AfterDestruction, without inventing a fatal
            // hit. Its Logic visit reaches here: Update leaves earlier only
            // for a warp (`0x0043FD14`, `0x0043FD26`) or a cleared IsAlive
            // (`0x0043FE63`), and Techno AI's self-heal skips Health 0
            // (`0x0070BE80`).
            // Steam gamemd3e81a617..08600 control: object_health.py
            // --building-zero-health and building_zero_health.{json,meta.json}.
            // This caller draws no RNG and writes no timer. Anim retirement,
            // Limbo/detach, survivor draws/smudges and rubble stay with their
            // existing owners.
            if sim
                .substrate
                .entities
                .get(id)
                .is_some_and(|entity| entity.health.current() == 0)
            {
                sim.clear_building_damage_fire_slots(id, rules);
                let ready = sim.substrate.entities.get(id).is_some_and(|entity| {
                    entity
                        .pending_c4_detonation
                        .pending()
                        .is_none_or(|pending| {
                            pending.timer().remaining(sim.session.binary_frame() as i32) == 0
                        })
                });
                if ready && let Some(rules) = rules {
                    let context = super::UninitContext::new(Some(rules), ctx.overlay_registry)
                        .with_effects(ctx.effects);
                    sim.techno_limbo_with_context(id, context);
                    // `0x004400CB..0x004400D4`: SpawnSurvivors(C4AppliedBy).
                    sim.spawn_building_survivors(
                        rules,
                        ctx.overlay_registry,
                        id,
                        |sim, (cell_rx, cell_ry)| {
                            sim.commit_smudge_request_inline(
                                rules,
                                ctx.overlay_registry,
                                crate::sim::combat::SmudgeSpawnRequest::BuildingSurvivor {
                                    cell_rx,
                                    cell_ry,
                                },
                            )
                        },
                        ctx.effects,
                    );
                    sim.apply_fatal_lifecycle_stage(
                        rules,
                        crate::sim::combat::FatalLifecycleStage::AfterDeathEffects,
                        id,
                        EntityCategory::Structure,
                        context,
                    );
                }
                return;
            }
            // ProcessDelayedFire (`0x004400F4`), then
            // BuildingClass::UpdateRepairAndPower (`0x004401B6`): the
            // computer's low-credit sale or auto-repair start, then the
            // repair step.
            if let Some(rules) = rules {
                building_missions::process_delayed_fire(
                    sim,
                    id,
                    rules,
                    ctx,
                    &mut outcome.bridge_state_changed,
                );
                sim.update_tesla_charge(id, rules);
                // Building4401A3..4401AF: allegiance belongs to this object's
                // AI tail, before repair/power and factory work. Earlier
                // native returns admit only a live, nonzero-health building.
                if sim.substrate.entities.get(id).is_some_and(|entity| {
                    entity.lifecycle.object_alive()
                        && entity.health.current() != 0
                        && !entity.is_warped_out()
                        && !entity.is_warping_in()
                }) {
                    outcome.ownership_changed |=
                        crate::sim::passenger::reconcile_civilian_garrison_owner_for_building(
                            sim,
                            rules,
                            ctx.overlay_registry,
                            id,
                            ctx.effects,
                        );
                }
                crate::sim::production::update_repair_and_power(
                    sim,
                    rules,
                    id,
                    ctx.overlay_registry,
                    ctx.effects,
                );
                // A `Factory=` type's own production (`0x004401BB..0x004401CD`).
                let has_factory = sim
                    .substrate
                    .entities
                    .get(id)
                    .and_then(|entity| sim.object_type(entity.type_ref(), rules))
                    .is_some_and(|ty| ty.has_factory());
                if has_factory {
                    crate::sim::production::factory_ai(
                        sim,
                        rules,
                        id,
                        ctx.overlay_registry,
                        ctx.effects,
                    );
                }
            }
            // BuildingClass::Update consumes the shared C4/PostMortem latch at
            // its late tail. Keep the forced receiver inline in this object's
            // LogicVector visit so nested death effects precede the next slot.
            if let Some(rules) = rules {
                outcome.bridge_state_changed |= sim.tick_pending_building_detonation(
                    id,
                    rules,
                    ctx.overlay_registry,
                    ctx.effects,
                );
                // The range drop that ends the Update (`0x00440378`).
                building_missions::range_drop(sim, id, rules, ctx);
            }
        }
        // AircraftClass::AI reaches the shared Foot/mission work before its
        // locomotor, but promotes via Ready→Commence only after Foot returns
        // (`0x0041504A`/`0x00415058`). Keep the counter here; the sole promotion
        // is `object_ai_post_movement_promote_one`.
        //
        // AircraftClass::AI drops an unsensed cloaked Target, then reaches the
        // common Techno AI body through FootClass::AI (`0x00414DA3`), so the
        // off-mission clear, the counter, MissionClass::AI and the passive
        // block run in AI_Update's order (`0x006FA5E8..0x006FA6EE`). The
        // retail fixed-wing types are `CanPassiveAquire=no`, so the block's
        // gate refuses them except on Move in a computer house's
        // `Aggressive=yes`, `Suicide=no` team (Korea's Black Eagles). A scan
        // takes nothing for an aircraft: with no `+0x3C4` override its mask 1
        // gives a zero flags word, which the class gate (`0x006F821A`) refuses
        // for every candidate; the scan's Scenario draw and timer remain.
        EntityCategory::Aircraft => {
            drop_unsensed_cloaked_target_step(sim, id, rules, ctx.effects);
            if let Some(rules) = rules
                && !techno_common_steps(sim, id, rules, ctx.overlay_registry, ctx.effects, None)
            {
                return;
            }
            clear_passive_target_off_mission(sim, id, rules);
            if !mission_counter_step(sim, id, rules, ctx, outcome) {
                return;
            }
            // MissionClass::AI, inside this slot and before Fly Process
            // (FootClass::AI4DA530). A Mission_Attack strike fires here.
            if let Some(rules) = rules
                && let Some(receipt) = crate::sim::aircraft::dispatch_mission(sim, id, rules, ctx)
            {
                outcome.bridge_state_changed |= receipt.bridge_state_changed;
                outcome.destroyed_structure |= receipt.structure_destroyed;
            }
            passive_acquire_step(sim, id, rules, ctx);
            if !techno_after_acquisition(sim, id, rules, ctx, outcome) {
                return;
            }
            techno_common_post(sim, id, rules);
        }
    }
}

/// The promotion detector of `TechnoClass::AI_Update @ 0x006FA054..0x006FA145`,
/// run once per live-object visit for every category (the common body is
/// reached from each leaf `AI`).
///
/// gamemd-derived: the rank cache (`+0x13C`) is compared with
/// `GetVeterancyLevel`; a crossing to elite seeds the flash timer (`+0xF0`,
/// `EliteFlashTimer`) for any owner and — for the local human's object — plays
/// `UpgradeEliteSound` positionally plus `EVA_UnitPromoted`; a crossing to
/// veteran plays `UpgradeVeteranSound` plus the EVA. The local-player gate is
/// the app layer's (`HouseClass::IsHumanPlayer @ 0x0050B6F0` is
/// `owner == g_PlayerPtr` in a skirmish). The flash's existing signed dword
/// counter is stepped later by4CC770 at6FAC4D, through flash_step; a
/// newly promoted object's timer therefore loses one on this same visit.
fn veterancy_promotion_step(sim: &mut Simulation, id: u64, rules: &RuleSet) {
    let Some(entity) = sim.substrate.entities.get_mut(id) else {
        return;
    };
    let Some(promotion) = entity
        .veterancy
        .sample_promotion(&mut entity.flashing, rules.general.elite_flash_timer)
    else {
        return;
    };
    let owner = entity.owner();
    let (rx, ry) = (entity.position.rx(), entity.position.ry());
    let sound = match promotion {
        crate::sim::combat::veterancy::Promotion::Elite => {
            rules.general.upgrade_elite_sound.as_deref()
        }
        crate::sim::combat::veterancy::Promotion::Veteran => {
            rules.general.upgrade_veteran_sound.as_deref()
        }
    };
    let sound_id = sound.map(|name| sim.interner.intern(name));
    sim.sound_events.emit(super::SimSoundEvent::UnitPromoted {
        owner,
        sound_id,
        rx,
        ry,
    });
}

/// The house self-heal pulse of `TechnoClass::AI_Update`, read from the
/// disassembly. It is a second, independent heal beside [`self_heal_step`]'s
/// per-object `+1`, and it has **two arms** in one `AI_Update` body:
///
/// ```text
/// // units: 0x6FA7D2..0x6FA857
/// if (Type->Organic(+0xD97) != 0) goto infantry arm;
/// if (bl != 0) return;
/// if (Frame % Rules->SelfHealUnitFrames(+0x38)) return;
/// if (!House->HasUnitSelfHeal()) return;          // 0x0050D9D0: +0x168 > 0
/// step = House->GetUnitSelfHealStep();            // 0x0050D9F0: (+0x3C) * +0x168
/// Health += min(step, Strength - Health);         // then the smoke tail
///
/// // infantry: 0x6FA8C8..0x6FA93E
/// if (Type->Organic(+0xD97) == 0) return;
/// if (bl != 0) return;
/// if (Frame % Rules->SelfHealInfantryFrames(+0x30)) return;
/// if (!House->HasInfantrySelfHeal()) return;      // 0x0050D9C0: +0x164 > 0
/// step = House->GetInfSelfHealStep();             // 0x0050D9E0: (+0x34) * +0x164
/// Health += min(step, Strength - Health);
/// ```
///
/// `+0xD97` is `Organic=`, which the InfantryType constructor stores
/// (`0x00523911`) and the other classes leave clear, so the first arm serves
/// non-organic Units off the house unit count `UnitsGainSelfHeal` feeds,
/// and the second serves Infantry and organic Units off the infantry count
/// `InfantryGainSelfHeal` feeds. Both run after the per-object pulse and before
/// the cloak tick (`vt+0x410` at `0x006FA946`), so a same-frame heal is visible
/// to the cloak's health branch.
///
/// The unit arm's tail after the add (`0x6FA85A..0x6FA89D`) retires the damage
/// smoke when the object holds one; [`Simulation::retire_damage_smoke_after_self_heal`]
/// is that owner, which [`self_heal_step`] already drives.
fn house_self_heal_step(sim: &mut Simulation, id: u64, rules: &RuleSet) {
    house_heal_arm(sim, id, rules, false);
    house_heal_arm(sim, id, rules, true);
}

/// One arm of [`house_self_heal_step`]. `infantry` selects the hospital arm:
/// its own class admission, frame key, house counter, amount key and predicate.
///
/// Admission, straight from `TechnoClass::AI_Update` (`0x006F9E50`):
///
/// ```text
/// 6FA7B9  cmp eax,1 ; jne 6FA8A2     ; the units arm is Unit-only
/// 6FA7D2  test cl,cl ; jne 6FA8A2    ; and skips an organic type (+0xD97)
/// 6FA7DA  test bl,bl ; jne 6FA8A2    ; and an at/over-strength or zero-Health object
/// 6FA8A9  cmp eax,0Fh ; je 6FA8D2    ; Infantry takes the hospital arm directly ...
/// 6FA8B5  cmp eax,1 ; jne 6FA941     ; ... which otherwise needs a Unit ...
/// 6FA8CE  test cl,cl ; je 6FA941     ; ... whose type is organic
/// 6FA8D2  test bl,bl ; jne 6FA941    ; the same guard as the units arm
/// ```
///
/// Only Infantry and Unit reach either arm — a Structure or an Aircraft
/// house-heals nothing, whatever its type's `Organic` says — and an
/// `Organic=no` Infantry still takes the hospital arm, because the Infantry
/// test at `0x6FA8A9` precedes the `Organic` read at `0x6FA8CE`.
///
/// `bl` is built at `0x006FA79A..0x006FA7B0` (`ebp` is zeroed at `0x006F9E58`):
/// it is set when `Health >= Type+0xA0` (`0x006FA7A2`, `jl` past the set) or when
/// `Health == 0` (`0x006FA7AC`), and either arm leaves before its heal block.
fn house_heal_arm(sim: &mut Simulation, id: u64, rules: &RuleSet, infantry: bool) {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    let Some(object) = sim.object_type(entity.type_ref(), rules) else {
        return;
    };
    let unit = entity.category() == EntityCategory::Unit;
    let admitted = if infantry {
        entity.category() == EntityCategory::Infantry || (unit && object.organic)
    } else {
        unit && !object.organic
    };
    if !admitted {
        return;
    }
    let strength = object.strength;
    let health = entity.health.current();
    if health >= strength || health == 0 {
        return;
    }
    let interval = if infantry {
        rules.general.self_heal_infantry_frames
    } else {
        rules.general.self_heal_unit_frames
    };
    // Original6FA7EE/6FA8E2 divides before testing the House count. Missing
    // keys retain constructor1000; an explicitly invalid IDIV is a fault,
    // including period0 with no grant and signed MIN/-1 overflow.
    let frame = sim.session.binary_frame() as i32;
    let remainder = frame.checked_rem(interval).unwrap_or_else(|| {
        panic!("native House self-heal IDIV fault at 006FA7EE/006FA8E2: frame={frame} period={interval}")
    });
    if remainder != 0 {
        return;
    }
    let owner = entity.owner();
    let count = sim.houses.get(&owner).map_or(0, |house| {
        if infantry {
            house.self_heal_infantry()
        } else {
            house.self_heal_units()
        }
    });
    if count <= 0 {
        return;
    }
    let amount = if infantry {
        rules.general.self_heal_infantry_amount
    } else {
        rules.general.self_heal_unit_amount
    };
    let step = amount.wrapping_mul(count);
    let Some(entity) = sim.substrate.entities.get_mut(id) else {
        return;
    };
    let missing = strength.wrapping_sub(entity.health.current());
    let heal = if step < missing { step } else { missing };
    entity.health.heal(heal);
    // Only the unit arm carries the damage-smoke tail before the cloak tick;
    // the infantry arm rejoins the common flow at 0x006FA941.
    if !infantry {
        sim.retire_damage_smoke_after_self_heal(id, rules);
    }
}

/// The self-heal pulse of `TechnoClass::AI_Update @ 0x006FA743..0x006FA757`:
/// when the eligibility virtual (`vtable+0x294` → `FUN_0070BE80`) holds,
/// `Health += 1` — a raw `INC` on `+0x6C`, no amount key.
///
/// The reached tail marks attached smoke done above ConditionYellow, retaining
/// the pointer until expiry. Building+6E6 and animation slots remain unchanged.
/// Original corpus: building_art_transition.json. After non-Greater health,
/// virtual1C8 is Object GetHeight5F5F40, which also retires smoke below -10
/// leptons relative to the live ground surface and explicit OnBridge offset.
fn self_heal_step(sim: &mut Simulation, id: u64, rules: &RuleSet) {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    let Some(object) = sim
        .interner
        .try_resolve(entity.type_ref())
        .and_then(|type_id| rules.object(type_id))
    else {
        return;
    };
    let interval =
        crate::sim::combat::veterancy::self_heal_interval_frames(rules.general.repair_rate_minutes);
    if !crate::sim::combat::veterancy::self_heal_eligible(
        entity,
        object,
        sim.session.binary_frame(),
        interval,
    ) {
        return;
    }
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity.health.heal(1);
    }
    sim.retire_damage_smoke_after_self_heal(id, rules);
}

/// `AircraftClass::AI @ 0x00414D4D..0x00414DA1`, read from the disassembly:
///
/// ```text
/// T = Target(+0x2B4);
/// if (T && (T->flags & 1) && !IsAlliedWith(myOwner, T)
///       && T->CloakState(+0x220) == 2
///       && !T->vt+0x32C(myOwner))                 // IsSensedByHouse
///     Assign_Target(NULL);
/// ```
///
/// Aircraft alone re-test this every tick, so a Harrier loses its lock the
/// moment its house's sensor coverage of a submerged target lapses — the ground
/// units around it keep theirs until their own scan cadence comes round.
/// (`vt+0x32C` at `0x0070D460` has no function boundary in the database; the
/// body reads the sensor count for the passed house at the object's own cell,
/// and returns false for a null house.)
fn drop_unsensed_cloaked_target_step(
    sim: &mut Simulation,
    id: u64,
    rules: Option<&RuleSet>,
    effects: FrameEffects<'_>,
) {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    let Some(crate::sim::combat::TargetKind::Entity(target_id)) = entity.attack_target.target()
    else {
        return;
    };
    let owner = entity.owner();
    let owner_str = sim.interner.resolve(owner).to_owned();
    let Some(target) = sim.substrate.entities.get(target_id) else {
        return;
    };
    if !target
        .cloak
        .as_ref()
        .is_some_and(|cloak| cloak.is_fully_cloaked())
    {
        return;
    }
    let target_cell = (target.position.rx(), target.position.ry());
    if crate::map::houses::are_houses_friendly(
        sim.house_alliances.as_map(),
        &owner_str,
        sim.interner.resolve(target.owner()),
    ) {
        return;
    }
    if sim
        .fog
        .has_sensor_for_house(owner, target_cell.0, target_cell.1)
    {
        return;
    }
    let _ = sim.assign_target_represented(id, None, rules, None, effects);
}

/// Tick the common `+0xC4` per-mission AI counter once per object visit
/// (`0x006FA646..0x006FA64F`), then enter `MissionClass::AI @ 0x005B3060`
/// (`0x006FA655`), whose first call is `ObjectClass::AI`'s sound block.
fn mission_counter_step(
    sim: &mut Simulation,
    id: u64,
    rules: Option<&RuleSet>,
    ctx: ObjectAiCtx<'_>,
    outcome: &mut ObjectAiOutcome,
) -> bool {
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity.mission.increment_ai_counter();
    }
    if let Some(rules) = rules {
        match sim.object_ai_fall(id, rules, ctx.overlay_registry, ctx.effects) {
            Ok((landed, changed)) => {
                outcome.landed |= landed;
                outcome.bridge_state_changed |= changed;
            }
            Err(error) => {
                outcome.fall_error = Some(error);
                return false;
            }
        }
    }
    sim.object_ai_sounds(id, rules, ctx.effects);
    ai_alive(&sim.substrate, id)
}

// ===== TechnoClass common-body bracket =====
//
// Per live Unit, gamemd's `TechnoClass::AI_Update` body is one contiguous
// bracket: pre-mission block -> +0xC4/Mission work -> post-mission block, with
// two IsAlive early-returns (after the pre-block, after dispatch). The mission
// work at the dispatch point increments the counter and runs absorbed mission
// handlers. The Unit/Infantry entry Ready/Commence precedes this bracket;
// post-movement promotion stays with the corresponding leaf owner.

/// `ObjectClass+0x90` IsAlive ([`GameEntity::is_object_alive`]), which
/// `TechnoClass::AI_Update` tests after the rocking update when `vt+0x298`
/// answers true (`0x006FA23C`) and again after the bomb and the managers
/// (`0x006FA735`), and `UnitClass::AI` before its fire update (`0x007365BB`).
/// A crashing wreck, an infantry corpse on its death sequence and an authored
/// zero-HP building keep it at Health 0: none may skip its terminal AI by
/// deriving this flag from Health.
/// `MissionClass::AI`'s gate ([`GameEntity::mission_handler_due`]) still
/// requires positive Health.
///
/// [`GameEntity::is_object_alive`]: crate::sim::game_entity::GameEntity::is_object_alive
/// [`GameEntity::mission_handler_due`]: crate::sim::game_entity::GameEntity::mission_handler_due
pub(crate) fn ai_alive(substrate: &ObjectSubstrate, id: u64) -> bool {
    substrate
        .entities
        .get(id)
        .is_some_and(|entity| entity.is_object_alive())
}

/// `TechnoClass::AI_Update`'s leading common steps, in native order: the
/// Iron Curtain tint stage (`0x006F9EAF`, [`iron_tint_step`]), the berserk
/// countdown (`0x006F9F0D`, [`berserk_step`]), the
/// promotion sample (`0x006FA054`) and drain blocks (`0x006FA14B..0x006FA224`),
/// followed by rocking, target validity, charge turret and the Door clock.
/// Returns false when rocking destroyed the object. Managers, self-healing
/// and cloak belong to [`techno_after_acquisition`], after mission dispatch
/// and passive targeting, rather than this pre-mission block.
fn techno_common_steps(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
    turret_rotate_sample: Option<bool>,
) -> bool {
    iron_tint_step(sim, id);
    // Techno6F9EBB follows the tint head and precedes veterancy/drain work.
    // A queued voice probes its handle only when this actor really reaches
    // this slot; recording final Logic membership would lose early exits.
    sim.record_unit_voice_visit(id);
    berserk_step(sim, id, rules, overlay_registry, effects);
    // Techno6F9FA9 follows the voice head and precedes promotion6FA054.
    // Building Update froze this sample before Temporal and its common AI.
    if let Some(sample) = turret_rotate_sample {
        sim.visit_turret_rotate_sound(id, sample, rules, effects);
    }
    veterancy_promotion_step(sim, id, rules);
    crate::sim::credit_income::drain_common_step(sim, id, rules);
    // Techno6FA224..6FA244: only a valid loaded main VXL reaches
    // RockingUpdate70B570 and its immediate Alive guard. This precedes
    // locomotor Process and the Foot parasite tail; no later global update
    // may erase a parasite's direct sideways-angle write.
    let rocking_type = sim.substrate.entities.get(id).and_then(|entity| {
        let object = sim.interner.type_handle(entity.type_ref(), rules)?;
        rules
            .object_has_loaded_voxel(object)
            .then(|| rules.object_by_handle(object))
    });
    if let Some(object) = rocking_type {
        let needs_self_damage = sim.substrate.entities.get_mut(id).is_some_and(|entity| {
            crate::sim::rocking::update(
                entity,
                object,
                rules.general.fallback_coefficient,
                sim.session.binary_frame(),
            )
        });
        if needs_self_damage {
            // RockingUpdate70BC23..70BCA3 calls the actual receiver with
            // type Strength, Rules C4Warhead, ignore-defenses and arg6=false.
            // Commit after releasing the entity borrow, before the Alive guard.
            let c4 = sim.interner.intern(&rules.bridge_warheads.c4_name);
            let event = crate::sim::combat::EntityDamageEvent::direct_receiver(
                id,
                object.strength,
                0,
                crate::sim::combat::RAD_NO_ATTACKER,
                None,
                c4,
                crate::sim::combat::ReceiverCallFlags {
                    ignore_defenses: true,
                    arg6: false,
                },
            );
            sim.commit_direct_damage_receiver(rules, overlay_registry, event, effects);
        }
        if !ai_alive(&sim.substrate, id) {
            return false;
        }
    }
    behind_indicator_step(sim, id, rules);
    allied_target_drop_step(sim, id, rules);
    illegal_target_drop_step(sim, id, rules);
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity.update_voxel_recoil(); // Techno6FA4D1, before ChargeTurret.
    }
    charge_turret_step(&sim.interner, &sim.session, &mut sim.substrate, id, rules);
    //Techno6FA5BE..6FA5D6, before Mission AI: every Techno finishes its own
    //due Door transition here. Gate/Factory mission work observes this change
    //inside the same LogicVector visit; no global phase advances Door clocks.
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity.advance_door(sim.session.binary_frame());
    }
    true
}

/// `TechnoClass::AI_Update`'s berserk block (`0x006F9F0D..0x006F9F6E`),
/// after the voice probe: a berserk object counts its time down
/// ([`BerserkState::count_down`](crate::sim::combat::BerserkState::count_down)).
/// Once it calms down its target goes (`vt+0x3C8(0)`), and it queues Guard if
/// its house is a human player's (`IsHuman`, House `+0x1EC`), else Hunt
/// (`vt+0x1E8`). No draws. The Psychedelic arm (`combat::receiver_health`)
/// sets the state.
fn berserk_step(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) {
    let Some(entity) = sim.substrate.entities.get_mut(id) else {
        return;
    };
    if !entity.berserk.count_down() {
        return;
    }
    let owner = entity.owner();
    let _ = sim.assign_target_represented(id, None, Some(rules), overlay_registry, effects);
    let mission = if sim.houses.get(&owner).is_some_and(|house| house.is_human()) {
        MissionType::Guard
    } else {
        MissionType::Hunt
    };
    let _ = sim.mission_queue_exact(
        id,
        crate::sim::mission::MissionId::from_known(mission),
        0,
        sim.session.binary_frame(),
        &crate::sim::mission::authority::EntityReadyInputProvider,
    );
}

/// `TechnoClass::UpdateIronTint` (`0x006F9EAF`), the first step of
/// `TechnoClass::AI_Update` after the hover flag and the Gattling sound: a
/// curtained object's tint stage, whose stage 2 draws from the Scenario
/// stream ([`InvulnerabilityState::update_tint`]).
///
/// Next, UpdateAirstrikeTint70E920 (call6F9EB6) steps a marked building,
/// consuming its jitter draw after the Iron draw on the same Scenario stream.
///
/// [`InvulnerabilityState::update_tint`]: crate::sim::superweapon::invulnerability::InvulnerabilityState::update_tint
fn iron_tint_step(sim: &mut Simulation, id: u64) {
    let frame = sim.session.binary_frame() as i32;
    let airstrike_active = sim.airstrike_tint_active(id);
    if let Some(curtain) = sim
        .substrate
        .entities
        .get_mut(id)
        .and_then(|entity| entity.invulnerability.as_mut())
    {
        curtain.update_tint(frame, &mut sim.scenario_rng);
    }
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity
            .airstrike_tint
            .update_tint(frame, airstrike_active, &mut sim.scenario_rng);
    }
}

/// Techno6FA4FB follows target validity and precedes mission dispatch. The
/// same object's later shot arms its charge duration for the next AI visit.
fn charge_turret_step(
    interner: &StringInterner,
    session: &ScenarioSession,
    substrate: &mut ObjectSubstrate,
    id: u64,
    rules: &RuleSet,
) {
    let Some(entity) = substrate.entities.get_mut(id) else {
        return;
    };
    let Some(object) = interner.object_type(entity.type_ref(), rules) else {
        return;
    };
    entity.update_charge_turret(session.binary_frame() as i32, object);
}

/// `TechnoClass::AI_Update @ 0x006FA472..0x006FA4CB`, after the drain
/// blocks: on every sixteenth frame an object with a target asks GetFireError
/// without range (vt+0x3BC, `SelectWeapon(Target)`) and drops the target on
/// ILLEGAL (5) or CANT (6), unless its current mission is Capture (8) or
/// Sabotage (0x11). A Unit's and an Infantry's own fire routines keep an
/// illegal target; this is where they let go of it.
fn illegal_target_drop_step(sim: &mut Simulation, id: u64, rules: &RuleSet) {
    use crate::sim::combat::{TargetKind, fire_error::FireError};
    if !sim.session.binary_frame().is_multiple_of(16) {
        return;
    }
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    let Some(target) = entity.attack_target.target() else {
        return;
    };
    // A target that no longer resolves never reaches this check natively
    // (`PointerExpired` clears it first); the Attack handler owns VERA's
    // clear of such a stale target.
    if matches!(target, TargetKind::Entity(_))
        && crate::sim::combat::resolve_target_coords(&target, sim).is_none()
    {
        return;
    }
    if matches!(entity.mission.current().raw(), 8 | 0x11) {
        return;
    }
    let weapon = target_scan::select_weapon(sim, rules, id, Some(target));
    let code = target_scan::fire_error_at(sim, rules, id, Some(target), weapon, false, None);
    if matches!(code, FireError::Illegal | FireError::Cant) {
        //6FA4CB dispatches class+3C8 before this object's mission handler:
        // Infantry51B1F0 clears68D/requests its action and invalidates5E0.
        sim.assign_target_represented(id, None, Some(rules), None, FrameEffects::default())
            .expect("the queried target-drop receiver remains present");
    }
}

/// `TechnoClass::AI_Update @ 0x006FAAEF..0x006FABB8`, after the spawn
/// manager: a Foot outside a team that holds a target it cannot reach, or an
/// open-topped transport's rider (`+0x82`), approaches the target, then lets
/// go of one it still cannot hit unless that left it a destination.
///
/// - The block skips an aircraft (vt+0x2C == 2), an object without a target,
///   a team member (vt+0x290 = `0x0041C050`, Foot `+0x5D4`) and a Foot with a
///   NavCom (`+0x5A4`). A building answers reachable (vt+0x2C4 =
///   `0x00459D90`) and is no rider, so it never acts here.
/// - A Foot asks IsInSameZoneAs (vt+0x2C4,
///   [`Simulation::foot_is_in_same_zone_as`]). A reachable target held by an
///   object outside a transport ends the block (`0x006FAB52`).
/// - Otherwise the Foot runs Approach_Target(0) (vt+0x53C at `0x006FAB74`,
///   [`Simulation::approach_target`]). With a NavCom then, the block ends;
///   without, the target it then holds is measured with CanFireAt (vt+0x3A8)
///   and the weapon SelectWeapon picks for it (vt+0x2E4), and a miss drops it
///   (vt+0x3C8(NULL), `0x006FABB2`). CanFireAt answers yes for no target
///   (`0x006F77BB`).
///
/// A rider never chases (`0x004D5782`) and is refused any destination
/// (`0x004D94D7`): out of range on its Guard (every rider:
/// `SetInOpenTransport` puts it there), Approach drops the target and the
/// destination (`0x004D571F..0x004D5744`); on Attack, a human's Area Guard or
/// a computer's Hunt, Approach goes on to its search, and the block's
/// CanFireAt drops the target. In range, Approach returns at `0x004D5A34`.
///
/// The block draws nothing itself. Approach draws through PlaceInfantryInCell
/// (`0x00481180`): a `CloseRange=` infantryman's spot requests, and the
/// destination setter of a Teleport or Jumpjet infantryman, which picks his
/// spot in the cell it is given. The block has no Health gate: a Die1/Die2
/// corpse, still IsAlive, runs it too.
///
/// RESIDUAL: a unit rider runs the head of `UnitClass::Approach_Target @
/// 0x007414E0` ([`Simulation::unit_approach_head`]), whose crush and
/// BalloonHover arms take the target as the destination through
/// `UnitClass::Set_Destination @ 0x00741970`, unreviewed for a rider ahead of
/// the Foot refusal. Trigger: a modded unit rider with `Crusher=`,
/// `OmniCrusher=` or `BalloonHover=`; retail's only unit riders, the Terror
/// and Chaos Drones, have none. Effect: none in retail.
fn target_reach_step(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) {
    use crate::sim::combat::TargetKind;
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    let Some(target) = entity.attack_target.target() else {
        return;
    };
    if !matches!(
        entity.category(),
        EntityCategory::Unit | EntityCategory::Infantry
    ) || sim.team_script_vm.team_for_member(id).is_some()
        || entity.navigation.nav_com().is_some()
    {
        return;
    }
    // A target that no longer resolves is cleared natively (`PointerExpired`)
    // before this block; the Attack handler owns VERA's clear of it.
    if matches!(target, TargetKind::Entity(_))
        && crate::sim::combat::resolve_target_coords(&target, sim).is_none()
    {
        return;
    }
    let rider = entity.passenger_role.in_open_transport();
    // A rider asks too (`0x006FAB48`), though only `+0x82` decides for it.
    // The query needs the map's cells, zones, bounds and size and a Foot's
    // locomotor, which a loaded scenario always has; a fixture without them
    // cannot ask and answers reachable.
    let reachable = match sim.foot_is_in_same_zone_as(id, target, rules) {
        Ok(reachable) => reachable,
        Err(cause) => {
            log::debug!("object {id} cannot ask IsInSameZoneAs: {cause}");
            true
        }
    };
    if reachable && !rider {
        return;
    }
    sim.approach_target(id, rules, overlay_registry, effects);
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    if entity.navigation.nav_com().is_some() {
        return;
    }
    let Some(target) = entity.attack_target.target() else {
        return;
    };
    let weapon = target_scan::select_weapon(sim, rules, id, Some(target));
    if target_scan::can_fire_at(sim, rules, id, target, weapon, overlay_registry) {
        return;
    }
    let _ = sim.assign_target_represented(id, None, Some(rules), overlay_registry, effects);
}

/// `TechnoClass::AI_Update @ 0x006FA30C..0x006FA46C`, every frame after the
/// drain blocks: a computer house's object lets go of a target its house is
/// allied with (`HouseClass::Is_Ally_ByObject @ 0x004F9AF0`). The deciding
/// house is the controller's when the object rides a mind-controlled
/// `OpenTopped=` transport (`+0x11C`, `+0x5E4`, `+0x2C0`), else its own; a
/// human house never drops (`0x0050B730`). It keeps the target when
/// - it is an infantryman and the target a building that would admit it as an
///   occupant (`0x00457CE0`);
/// - it is an `Engineer=` infantryman (`+0xEC3`);
/// - its weapon 1's warhead is `ElectricAssault=` (`+0x158`) and the target a
///   building whose type is `Overpowerable=` (`+0x1575`);
/// - it is berserk (`+0x298`).
///
/// Otherwise Assign_Target(NULL). No draw, no retarget.
fn allied_target_drop_step(sim: &mut Simulation, id: u64, rules: &RuleSet) {
    use crate::sim::combat::{TargetKind, combat_weapon};
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    let house = combat_weapon::controlled_transport_house(
        &sim.substrate.entities,
        rules,
        &sim.interner,
        entity,
    )
    .unwrap_or(entity.owner());
    if sim.owner_is_human(house) {
        return;
    }
    // `0x004F9AF0` tests the target's Object flag first: a cell is never
    // allied.
    let Some(TargetKind::Entity(target_id)) = entity.attack_target.target() else {
        return;
    };
    let Some(target) = sim.substrate.entities.get(target_id) else {
        return;
    };
    if !combat_weapon::is_ally_by_object(
        Some(sim.house_alliances.as_map()),
        &sim.interner,
        house,
        target.owner(),
    ) {
        return;
    }
    let (Some(obj), Some(target_obj)) = (
        sim.interner.object_type(entity.type_ref(), rules),
        sim.interner.object_type(target.type_ref(), rules),
    ) else {
        return;
    };
    let infantry = entity.category() == EntityCategory::Infantry;
    let target_building = target.category() == EntityCategory::Structure;
    let occupiable = infantry
        && target_building
        && target.passenger_role.cargo().is_some_and(|cargo| {
            crate::sim::passenger::can_dock_occupier_garrison(
                entity, target, obj, target_obj, cargo, rules, sim,
            )
        });
    let engineer = infantry && obj.engineer;
    let overpowers = target_building
        && target_obj.overpowerable
        && target_scan::weapon_at_index(sim, rules, id, 1)
            .and_then(|weapon| combat_weapon::warhead_of(rules, weapon))
            .is_some_and(|warhead| warhead.electric_assault);
    if engineer || overpowers || occupiable || entity.berserk.is_active() {
        return;
    }
    //6FA46C precedes the subsequent live target re-read and illegal test.
    // Preserve class refusal and synchronous action/path effects.
    sim.assign_target_represented(id, None, Some(rules), None, FrameEffects::default())
        .expect("the queried allied-target receiver remains present");
}

/// The shared post-acquisition sequence of `TechnoClass::AI_Update`:
/// Bomb6FA6F5, Slave6FA717, Capture6FA726, IsAlive6FA735, self-heal6FA743,
/// House healing6FA79A, Cloak6FA941 and Spawn6FA94C. Original instructions
/// and executable controls: `tools/spatial_oracle/capture_overload_update.py
/// --techno-order` and its documented callback/stock-MIND boundaries.
///
/// Re-read every owner after the previous callback: a fatal bomb frees the
/// captives before CaptureManager is queried, so the stock manager emits no
/// overload pulse. An overload still constructs its sparks, but a dead
/// controller never heals, cloaks or spawns. Mission and scan draws precede
/// overload draws on the same Scenario stream. Unit,
/// Infantry, Building and Aircraft use this one sequence in their Logic visit.
fn techno_after_acquisition(
    sim: &mut Simulation,
    id: u64,
    rules: Option<&RuleSet>,
    ctx: ObjectAiCtx<'_>,
    outcome: &mut ObjectAiOutcome,
) -> bool {
    if let Some(rules) = rules {
        outcome.bridge_state_changed |=
            sim.bomb_fuse_step(id, rules, ctx.overlay_registry, ctx.effects);
        sim.slave_manager_ai(id, rules, ctx.overlay_registry, ctx.effects);
        sim.capture_manager_update(id, rules, ctx.overlay_registry, ctx.effects);
    }
    if !ai_alive(&sim.substrate, id) {
        return false;
    }
    if let Some(rules) = rules {
        self_heal_step(sim, id, rules);
        house_self_heal_step(sim, id, rules);
        super::techno_ai_cloak::tick_stock_cloak_producer(
            sim,
            id,
            rules,
            ctx.overlay_registry,
            ctx.effects,
        );
        // A target the Unit's later Fire_At_Target hands its manager waits
        // until the next visit (Unit73647B/7365E1).
        crate::sim::spawn_manager::spawn_manager_ai(
            sim,
            rules,
            id,
            ctx.overlay_registry,
            ctx.effects,
        );
        healer_target_drop_step(sim, id, rules);
    }
    true
}

/// `TechnoClass::AI_Update @ 0x006FA24A..0x006FA30C`, before the allied
/// target drop: an object whose type has `CanBeHidden=` (TechnoType `+0x724`)
/// and that is not an Aircraft (vt+0x2C 2), standing on a Cell that hides it
/// (`cell_hides_objects`, `0x00487E00`, the Cell of its GetCoords vt+0x48),
/// takes the Behind indicator (`UpdateBehindIndicator @ 0x0070F1D0`); any
/// other object UnInits a retained one (vt+0xF8) and clears `+0x12C`.
///
/// UpdateBehindIndicator acts only when GetVisualState(0, NULL) (vt+0x68,
/// [`screen_visual_character`]) answers 0 and `+0x12C` is empty: it
/// constructs `[General] Behind=` (`Rules+0xB8`) at the object's GetCoords
/// with delay 0, loop 1, flags 0x600, ZAdjust 0 (`0x0070F20D..0x0070F24C`),
/// stores it at `+0x12C` and attaches it (SetOwnerObject `0x00424B50`). The
/// Anim's hidden byte (`+0x19D`) follows the local Show Hidden option
/// (`Options+0x1F`, `0x0070F26B`, AnimClass::AI `0x00423B62..0x00423B88`); the
/// presentation reads the option in its place.
///
/// RESIDUAL: with no `Behind=` type (retail sets BEHIND), native draws a
/// blinking bracket at the caller's screen point instead
/// (`0x0070F290..0x0070F659`); VERA draws nothing.
///
/// RESIDUAL: GetVisualState(0, NULL) depends on the current house only for
/// an `Invisible=`/`InvisibleInGame=` object (about ten retail buildings) on
/// a hiding Cell: it is 0 for its owner and 5 for anyone else, so whether
/// the anim exists, and the IDs after it, differ between peers, as natively.
///
/// RESIDUAL: when BEHIND's art never bound, the spawn takes a native unique
/// ID and fails each frame; native instead constructs a default-type Anim
/// that expires and is rebuilt. Neither draws. No RNG draws or timer writes.
fn behind_indicator_step(sim: &mut Simulation, id: u64, rules: &RuleSet) {
    use crate::sim::components::AnimClassSpawnDescriptor;
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    let Some(object) = sim.interner.object_type(entity.type_ref(), rules) else {
        return;
    };
    // With no hidden-occupation entry no Cell hides anything; skip the
    // per-object coordinate and Cell lookups (VERA-only, result unchanged).
    let coord = (object.can_be_hidden
        && entity.category() != EntityCategory::Aircraft
        && sim.substrate.hidden_occupation.entry_count() != 0)
        .then(|| sim.anim_owner_coords(id))
        .flatten();
    let hidden = coord.is_some_and(|coord| {
        u16::try_from(coord.x / 256)
            .ok()
            .zip(u16::try_from(coord.y / 256).ok())
            .is_some_and(|(rx, ry)| sim.substrate.cell_hides_objects(rx, ry))
    });
    if !hidden {
        if let Some(anim) = sim
            .substrate
            .entities
            .get_mut(id)
            .and_then(|entity| entity.take_behind_anim())
        {
            sim.destroy_anim(anim, rules);
        }
        return;
    }
    if screen_visual_character(sim, entity, rules) != 0 || entity.behind_anim().is_some() {
        return;
    }
    let (Some(name), Some(coord)) = (rules.general.behind.as_deref(), coord) else {
        return;
    };
    let type_name = sim.interner.intern(name);
    let (rx, ry, sub_x, sub_y, z) = coord.to_cell_sub_z();
    let descriptor = AnimClassSpawnDescriptor {
        draw_flags: 0x600,
        owner: Some(id),
        ..AnimClassSpawnDescriptor::new(type_name, rx, ry, sub_x, sub_y, z)
    };
    if let Ok(anim) = sim.spawn_anim_at_world(rules, descriptor, coord)
        && let Some(entity) = sim.substrate.entities.get_mut(id)
    {
        entity.retain_behind_anim(anim);
    }
}

/// GetVisualState(0, NULL) (`0x00703860`) as the AI asks it: the screen
/// query for the current house (`0x00A83D4C`, the persisted
/// `session.current_house`), its sensor on the object's Cell and both
/// alliance directions.
fn screen_visual_character(
    sim: &Simulation,
    entity: &crate::sim::game_entity::GameEntity,
    rules: &RuleSet,
) -> u8 {
    use crate::sim::combat::combat_weapon::is_ally_by_object;
    let viewer = sim.session.current_house();
    let owner = entity.owner();
    let allied = |a, b| is_ally_by_object(Some(sim.house_alliances.as_map()), &sim.interner, a, b);
    let invisible = sim
        .interner
        .object_type(entity.type_ref(), rules)
        .is_some_and(|object| {
            object.invisible
                || entity.category() == EntityCategory::Structure && object.invisible_in_game
        });
    entity.visual_character(
        rules.general.cloaking_stages,
        invisible,
        crate::sim::cloak_disguise::VisualCharacterQuery::screen(
            true,
            viewer.is_some(),
            viewer.is_some_and(|viewer| {
                sim.fog
                    .has_sensor_for_house(viewer, entity.position.rx(), entity.position.ry())
            }),
            viewer.is_some_and(|viewer| allied(viewer, owner) && allied(owner, viewer)),
            !sim.session.game_mode_nonzero(),
            false,
        ),
    )
}

/// `TechnoClass::AI_Update @ 0x006FA9D8..0x006FAAEF`, after the spawn manager
/// (`0x006FA958`) and the cloak-field arms: a healer (CombatDamage
/// `0x006F3970(-1) < 0`) lets go of its target (vt+0x3C8(NULL)) in three
/// arms, each re-reading the target:
/// - GameMode 0 (`0x00A8B238`): a house that is not the player's
///   (`0x0050B6F0`) drops a target it is not allied with
///   (`0x004F9AF0`, `0x006FA9D8..0x006FAA1B`);
/// - any other GameMode: a human house (`+0x1EC`, which IsControlledByHuman
///   `0x0050B730` reads there) drops a target it is not allied with
///   (`0x006FAA21..0x006FAA69`);
/// - a Unit drops an Aircraft target that is airborne (GetHeight vt+0x1C8
///   above 0) or stands on a building's Cell (`0x0047C520` at its Location,
///   `0x006FAA6F..0x006FAAE9`).
///
/// IsAlliedWith on an object asks its owner; a Cell or an ownerless object is
/// never allied. Both alliance arms ask the object's own house (+0x21C).
///
/// RESIDUAL: the cloak-field arms before this block (`0x006FA95B..0x006FA9D2`)
/// are not run. Trigger: an object in a house's cloak field, which only a
/// `CloakGenerator=` building raises; retail rules name it only in a comment
/// and `fog.cloaked_by_houses` has no writer, so they are dormant, not
/// unreachable. Effect if a map sets it: the field's cloak and uncloak calls
/// are missed. No RNG draws.
fn healer_target_drop_step(sim: &mut Simulation, id: u64, rules: &RuleSet) {
    use crate::sim::combat::{TargetKind, combat_weapon};
    let healer = |sim: &Simulation| {
        sim.substrate.entities.get(id).is_some_and(|entity| {
            sim.interner
                .object_type(entity.type_ref(), rules)
                .is_some_and(|kind| {
                    combat_weapon::weapon_damage_value(
                        entity,
                        kind,
                        &sim.substrate.entities,
                        rules,
                        &sim.interner,
                    ) < 0
                })
        })
    };
    let target = |sim: &Simulation| {
        sim.substrate
            .entities
            .get(id)
            .and_then(|entity| entity.attack_target.target())
    };
    let allied = |sim: &Simulation, target: TargetKind| {
        let (Some(entity), TargetKind::Entity(target_id)) =
            (sim.substrate.entities.get(id), target)
        else {
            return false;
        };
        sim.substrate.entities.get(target_id).is_some_and(|object| {
            combat_weapon::is_ally_by_object(
                Some(sim.house_alliances.as_map()),
                &sim.interner,
                entity.owner(),
                object.owner(),
            )
        })
    };
    let drop = |sim: &mut Simulation| {
        sim.assign_target_represented(id, None, Some(rules), None, FrameEffects::default())
            .expect("the queried healer receiver remains present");
    };
    let Some(owner) = sim.substrate.entities.get(id).map(|entity| entity.owner()) else {
        return;
    };
    if !sim.session.game_mode_nonzero()
        && !sim.house_is_human_player(owner)
        && let Some(current) = target(sim)
        && !allied(sim, current)
        && healer(sim)
    {
        drop(sim);
    }
    if let Some(current) = target(sim)
        && sim.session.game_mode_nonzero()
        && sim.owner_is_human(owner)
        && healer(sim)
        && !allied(sim, current)
    {
        drop(sim);
    }
    let Some(TargetKind::Entity(target_id)) = target(sim) else {
        return;
    };
    let Some(aircraft) = sim
        .substrate
        .entities
        .get(target_id)
        .filter(|object| object.category() == EntityCategory::Aircraft)
    else {
        return;
    };
    let unit = sim
        .substrate
        .entities
        .get(id)
        .is_some_and(|entity| entity.category() == EntityCategory::Unit);
    if !unit || !healer(sim) {
        return;
    }
    let airborne = crate::sim::movement::air_movement::current_fly_height(
        aircraft,
        sim.resolved_terrain.as_ref(),
    ) > 0;
    let padded = sim
        .substrate
        .occupancy
        .first_building_on_layer(
            aircraft.position.rx(),
            aircraft.position.ry(),
            crate::sim::movement::locomotor::MovementLayer::Ground,
        )
        .is_some();
    if airborne || padded {
        drop(sim);
    }
}

/// The common Techno post-mission stage and the stock-dormant Cyborg Spark
/// producer. The retained system and both native producers share particles::spawn.
fn techno_common_post(sim: &mut Simulation, id: u64, rules: Option<&RuleSet>) {
    // Techno6FABC4 follows Mission, passive acquisition and the reach slot.
    // Native Buildings step at4509DE in UpdateAnimation before Techno and
    // never enter this receiver. Their existing construction owner still
    // steps in the late region; Sell steps at mission dispatch. Moving those
    // clocks and completion effects to the native early slot remains the
    // separate Building Update scheduling mechanism.
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity.tick_native_stage(sim.session.binary_frame() as i32);
    }
    flash_step(&mut sim.substrate, id);
    let Some(rules) = rules else {
        return;
    };

    sim.update_damage_spark_after_ai(id, rules);
}

/// Shared late Techno6FAC31..6FAC79 slot for every represented category.
/// The countdown owns no RNG or CdTimer. Native redraw virtual+124 is a
/// presentation dependency; the existing +F0 supplies its phase bits.
fn flash_step(substrate: &mut ObjectSubstrate, id: u64) {
    if ai_alive(substrate, id)
        && let Some(entity) = substrate.entities.get_mut(id)
    {
        entity.flashing.tick();
    }
}

/// Outcome of the common bracket for one Unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BracketReach {
    /// Died after the pre-block (health 0); no mission work ran.
    DiedInPre,
    /// Live Unit: ran the `+0xC4` counter and admitted mission handlers at the
    /// dispatch point.
    Dispatched,
}

/// Per-Unit TechnoClass common bracket. Runs the contiguous
/// `pre -> [IsAlive B] -> +0xC4/dispatch -> [IsAlive E] -> post` structure at
/// the gamemd-faithful per-object AI point (pre-movement, LogicVector order).
/// UnitClass::AI's first Ready→Commence at736473 precedes this bracket in
/// `techno_ai_shell`; the later checkpoint has its post-movement owner.
fn unit_techno_bracket(
    sim: &mut Simulation,
    id: u64,
    rules: Option<&RuleSet>,
    ctx: ObjectAiCtx<'_>,
    outcome: &mut ObjectAiOutcome,
) -> BracketReach {
    if let Some(rules) = rules
        && !techno_common_steps(sim, id, rules, ctx.overlay_registry, ctx.effects, None)
    {
        return BracketReach::DiedInPre;
    }
    // Guard B (IsAlive, `0x006FA23C`). A crashing wreck is alive at Health 0,
    // so it runs on: the counter, the passive block (on Move,
    // Guard or Harvest the gate and the scan with its Scenario draws, which
    // can acquire a target its fire update then shoots), the bomb and the
    // managers, and the post block; only the handlers are skipped.
    if !ai_alive(&sim.substrate, id) {
        return BracketReach::DiedInPre;
    }
    // The off-mission passive-target clear runs BEFORE the +0xC4 counter.
    clear_passive_target_off_mission(sim, id, rules);
    if !mission_counter_step(sim, id, rules, ctx, outcome) {
        return BracketReach::Dispatched;
    }
    // Mission_Dispatch position: the absorbed handler bodies run here,
    // timer-gated, ending with the verified post-handler epilogue write
    // (start = current frame, delay = handler return). One committed-selector
    // match routes Harvest, Attack and the other represented Foot handlers.
    if let Some(rules) = rules {
        outcome.bridge_state_changed |= dispatch_foot_mission(sim, id, rules, ctx);
    }
    // `UnitClass::AI @ 0x007361A9..0x00736204`: a draining Floating Disc
    // re-checks the building under it every 16th frame and drops the link
    // when it has drifted off, then removes a ray left with no link (after
    // FootClass::AI in the native order).
    crate::sim::credit_income::drain_unit_ai_step(sim, id, rules);
    // Passive / opportunity target acquisition sits between mission dispatch
    // and the second IsAlive guard, before the object's own locomotion.
    passive_acquire_step(sim, id, rules, ctx);
    // Guard E (IsAlive, `0x006FA735`): the dispatched handler, or the bomb it
    // carried, may have destroyed the Unit; a dead Unit runs no post-mission
    // block.
    if !techno_after_acquisition(sim, id, rules, ctx, outcome) {
        return BracketReach::Dispatched;
    }
    if let Some(rules) = rules {
        target_reach_step(sim, id, rules, ctx.overlay_registry, ctx.effects);
    }
    techno_common_post(sim, id, rules);
    BracketReach::Dispatched
}

// ===== Passive / opportunity target acquisition =====
//
// What makes an idle Grizzly shoot a tank that drives past: the passive block
// of TechnoClass::AI_Update (`0x006FA65A`) asks the gate and runs
// Retaliate_And_Scan. Both live in `target_scan`; this section keeps the
// mission-change drop of a scanner-installed target.

/// Missions on which a passively-acquired target is dropped, before the AI
/// counter runs. Meaning: the moment an object takes a job that should not be
/// shooting, a target it picked up on its own goes away. The mission *numbers*
/// are verified ({0, 7, 13, 14, 16, 18, 19, 20, 22, 23, 24, 28}); the names
/// below are this project's mission table for those indices.
const PASSIVE_TARGET_CLEAR_MISSIONS: [MissionType; 12] = [
    MissionType::Sleep,
    MissionType::Enter,
    MissionType::Stop,
    MissionType::Ambush,
    MissionType::Unload,
    MissionType::Construction,
    MissionType::Selling,
    MissionType::Repair,
    MissionType::Missile,
    MissionType::Harmless,
    MissionType::Open,
    MissionType::Deliberate,
];

/// The off-mission clear, which runs before the AI counter: a passively
/// acquired target is dropped the moment the object takes a job that should not
/// be shooting (see [`PASSIVE_TARGET_CLEAR_MISSIONS`]).
/// `TechnoClass::AI_Update @ 0x006F9E50` at `0x006FA5E8..0x006FA63B`: TarCom `+0x2B4`,
/// the mark `+0x50C`, then the mission `+0xAC`.
fn clear_passive_target_off_mission(sim: &mut Simulation, id: u64, rules: Option<&RuleSet>) {
    let drop = sim.substrate.entities.get(id).is_some_and(|entity| {
        entity.attack_target.target().is_some()
            && entity.passive_scan.acquired()
            && PASSIVE_TARGET_CLEAR_MISSIONS.contains(&entity.passive_acquire_mission())
    });
    if !drop {
        return;
    }
    let _ = sim.assign_target_represented(id, None, rules, None, FrameEffects::default());
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity.passive_scan.clear_acquired();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::tube_facts::TubeId;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::sim::combat::TargetKind;
    use crate::sim::components::{DriveCoord, NavTargetRef};
    use crate::sim::docking::bunker_link::BunkerLink;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::mission::leaf::MissionLeafState;
    use crate::sim::mission::state::MissionTestFixture;
    use crate::sim::mission::{
        MISSION_STUB_FRAMES, MissionCom, MissionDispatchTimer, MissionId, MissionType,
    };
    use crate::sim::movement::locomotor::LocomotorState;
    use crate::sim::movement::tube_movement::LowBridgeTubeMovementState;
    use crate::sim::snapshot::GameSnapshot;

    /// TechnoClass::AI's Behind indicator (`0x006FA24A..0x006FA30C`) on
    /// retail rules: a tank on a hiding Cell takes one attached BEHIND anim
    /// and keeps it, and UnInits it once the Cell stops hiding it; an
    /// Aircraft never takes one; a Cell holding a building hides only from a
    /// second contribution on.
    #[test]
    fn a_tank_behind_a_building_takes_the_behind_anim_until_it_leaves() {
        use crate::rules::object_type::BuildingHiddenOccupancyProfile;
        let Some(fixture) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let mut rules = fixture.rules;
        assert_eq!(rules.general.behind.as_deref(), Some("BEHIND"));
        let Some((_, mut assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        crate::map::theater::load_theater(&mut assets, "TEMPERATE").unwrap();
        // Use the loader's actual root list and physical SHP. A stand-in count
        // concealed the missing Behind root and let this test pass without art.
        let roots = crate::rules::effect_asset_catalog::tolerant_anim_class_roots(&rules);
        rules.bind_anim_class_assets(&roots, &assets, "tem", "TEMPERATE");
        assert_eq!(
            rules
                .art()
                .anim_runtime_config("BEHIND")
                .unwrap()
                .raw_shp_frame_count,
            Some(18),
            "the retail marker must bind before Techno AI can construct it"
        );
        assert!(
            rules
                .object("MTNK")
                .is_some_and(|object| object.can_be_hidden)
        );
        let mut sim = Simulation::new();
        sim.session.set_map_width_for_test(32);
        sim.session.set_map_height_for_test(32);
        sim.resolve_type_handles(&rules);
        let tank = sim
            .spawn_object("MTNK", "Americans", 10, 10, 0, &rules)
            .expect("retail MTNK spawns");
        let profile = BuildingHiddenOccupancyProfile {
            can_hide_things: true,
            occupy_height: 1,
            ..Default::default()
        };
        let hide = |sim: &mut Simulation, enter: bool, cell: (u16, u16)| {
            let grid = &mut sim.substrate.hidden_occupation;
            if enter {
                grid.enter_building(cell, "1x1", profile, Some((32, 32)));
            } else {
                grid.exit_building(cell, "1x1", profile, Some((32, 32)));
            }
        };
        let behind = |sim: &Simulation| sim.substrate.entities.get(tank).unwrap().behind_anim();

        behind_indicator_step(&mut sim, tank, &rules);
        assert_eq!(behind(&sim), None, "an open Cell");
        hide(&mut sim, true, (10, 10));
        behind_indicator_step(&mut sim, tank, &rules);
        let anim = behind(&sim).expect("the Behind anim");
        let object = sim.anim(anim).unwrap();
        assert_eq!(object.owner_entity(), Some(tank));
        assert!(
            sim.interner
                .resolve(object.type_id())
                .eq_ignore_ascii_case("BEHIND")
        );
        behind_indicator_step(&mut sim, tank, &rules);
        assert_eq!(behind(&sim), Some(anim), "one anim");
        hide(&mut sim, false, (10, 10));
        behind_indicator_step(&mut sim, tank, &rules);
        assert_eq!(behind(&sim), None);
        assert!(sim.substrate.pending_delete.contains_object(&anim));

        // An Aircraft never takes it.
        let orca = sim
            .spawn_object("ORCA", "Americans", 14, 10, 0, &rules)
            .expect("retail ORCA spawns");
        hide(&mut sim, true, (14, 10));
        behind_indicator_step(&mut sim, orca, &rules);
        assert_eq!(
            sim.substrate.entities.get(orca).unwrap().behind_anim(),
            None
        );

        let power = sim
            .spawn_object("GAPOWR", "Americans", 20, 10, 0, &rules)
            .expect("retail GAPOWR spawns");
        assert!(sim.substrate.occupancy.contains_entity(20, 10, power));
        while sim.substrate.hidden_occupation.count(20, 10) != 0 {
            hide(&mut sim, false, (20, 10));
        }
        hide(&mut sim, true, (20, 10));
        assert!(!sim.substrate.cell_hides_objects(20, 10), "its own count");
        hide(&mut sim, true, (20, 10));
        assert!(sim.substrate.cell_hides_objects(20, 10));
    }

    /// TechnoClass::AI's healer drops (`0x006FA9D8..0x006FAAEF`) in
    /// skirmish: a human house's medic lets go of an enemy (a target Yuri
    /// took over) and keeps an ally; a healing Unit lets go of an airborne
    /// aircraft and keeps a grounded one; an ordinary gun keeps any target.
    #[test]
    fn a_human_healer_drops_an_enemy_and_a_unit_healer_an_airborne_aircraft() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n0=MEDIC\n[VehicleTypes]\n0=IFV\n1=TANK\n\
             [AircraftTypes]\n0=ORCA\n[BuildingTypes]\n0=PAD\n\
             [PAD]\nStrength=500\nFoundation=1x1\n\
             [MEDIC]\nStrength=100\nPrimary=Heal\n\
             [IFV]\nStrength=200\nPrimary=Fix\n\
             [TANK]\nStrength=300\nPrimary=Gun\n\
             [ORCA]\nStrength=150\n\
             [WeaponTypes]\n0=Heal\n1=Fix\n2=Gun\n\
             [Heal]\nDamage=-50\nRange=2\n[Fix]\nDamage=-30\nRange=2\n\
             [Gun]\nDamage=60\nRange=5\n",
        ))
        .unwrap();
        let mut sim = Simulation::new();
        let ours = sim.interner.intern("Americans");
        let theirs = sim.interner.intern("Russians");
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                ours, 0, None, true, 0, 10,
            ));
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                theirs, 1, None, false, 0, 10,
            ));
        sim.session.set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );
        sim.session.set_current_house_for_test(Some(ours));
        sim.resolve_type_handles(&rules);
        let spawn = |sim: &mut Simulation, name: &str, owner: &str, x: u16| {
            sim.spawn_object(name, owner, x, 10, 0, &rules)
                .unwrap_or_else(|| panic!("{name} spawns"))
        };
        let medic = spawn(&mut sim, "MEDIC", "Americans", 10);
        let ifv = spawn(&mut sim, "IFV", "Americans", 12);
        let gun = spawn(&mut sim, "TANK", "Americans", 14);
        let enemy = spawn(&mut sim, "TANK", "Russians", 16);
        let friend = spawn(&mut sim, "TANK", "Americans", 18);
        let orca = spawn(&mut sim, "ORCA", "Americans", 20);
        let pad = spawn(&mut sim, "PAD", "Americans", 24);
        let their_medic = spawn(&mut sim, "MEDIC", "Russians", 26);
        let run = |sim: &mut Simulation, id: u64, target: u64| {
            sim.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .attack_target
                .set_for_test(Some(TargetKind::Entity(target)));
            healer_target_drop_step(sim, id, &rules);
            sim.substrate
                .entities
                .get(id)
                .unwrap()
                .attack_target
                .target()
        };
        assert_eq!(run(&mut sim, medic, enemy), None);
        assert_eq!(
            run(&mut sim, medic, friend),
            Some(TargetKind::Entity(friend))
        );
        assert_eq!(run(&mut sim, gun, enemy), Some(TargetKind::Entity(enemy)));
        sim.substrate
            .entities
            .get_mut(orca)
            .unwrap()
            .position
            .set_exact_z_leptons_for_test(Some(0));
        assert_eq!(run(&mut sim, ifv, orca), Some(TargetKind::Entity(orca)));
        sim.substrate
            .entities
            .get_mut(orca)
            .unwrap()
            .position
            .set_exact_z_leptons_for_test(Some(600));
        assert_eq!(run(&mut sim, ifv, orca), None);
        // Only a Unit drops an aircraft.
        assert_eq!(run(&mut sim, medic, orca), Some(TargetKind::Entity(orca)));
        // A grounded aircraft on a building's Cell (an airpad).
        let at_pad = |sim: &mut Simulation| {
            let position = &mut sim.substrate.entities.get_mut(orca).unwrap().position;
            position.set_rx_for_test(24);
            position.set_exact_z_leptons_for_test(Some(0));
        };
        at_pad(&mut sim);
        assert_eq!(
            sim.substrate.occupancy.first_building_on_layer(
                24,
                10,
                crate::sim::movement::locomotor::MovementLayer::Ground
            ),
            Some(pad)
        );
        assert_eq!(run(&mut sim, ifv, orca), None);
        // A Cell is never allied.
        sim.substrate
            .entities
            .get_mut(medic)
            .unwrap()
            .attack_target
            .set_for_test(Some(TargetKind::Cell(30, 10)));
        healer_target_drop_step(&mut sim, medic, &rules);
        assert_eq!(
            sim.substrate
                .entities
                .get(medic)
                .unwrap()
                .attack_target
                .target(),
            None
        );
        // GameMode 0: a computer house drops an enemy, a human house keeps it.
        sim.session.set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(false),
        );
        assert_eq!(run(&mut sim, their_medic, friend), None);
        assert_eq!(run(&mut sim, medic, enemy), Some(TargetKind::Entity(enemy)));
    }

    /// A Die1 corpse takes the healer drops (`0x006FA9D8..0x006FAAEF`) on its
    /// one ordinary visit: IsAlive gates them (`0x006FA735`), Health does
    /// not. A human house's dead medic lets go of the enemy it held; a dead
    /// rifleman beside it, no healer, keeps his on a frame the sixteenth-frame
    /// CANT drop (`0x006FA472`) skips.
    ///
    /// The test sets the held target directly to isolate that gate. In play
    /// the killing hit's Stun (`0x006FCD40`) clears it; a corpse holds an
    /// enemy again only after a passive re-acquire (`0x007091D0`, no Health
    /// gate) followed by an owner change.
    #[test]
    fn a_dead_medic_lets_go_of_an_enemy_on_its_corpse_visit() {
        let art = IniFile::from_str(crate::rules::retail_ini_fixture::GI_ART_EXCERPT);
        let mut rules = RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n0=MEDIC\n1=E1\n[VehicleTypes]\n0=TANK\n\
             [AircraftTypes]\n[BuildingTypes]\n\
             [MEDIC]\nImage=GI\nStrength=100\nPrimary=Heal\n\
             [E1]\nImage=GI\nStrength=100\nPrimary=Gun\n\
             [TANK]\nStrength=300\nPrimary=Gun\n\
             [WeaponTypes]\n0=Heal\n1=Gun\n\
             [Heal]\nDamage=-50\nRange=2\n[Gun]\nDamage=60\nRange=5\n",
        ))
        .unwrap();
        rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
        rules.bind_animation_sequences(
            &crate::rules::infantry_sequence::parse_infantry_sequence_registry(&art),
        );
        let mut sim = Simulation::new();
        let ours = sim.interner.intern("Americans");
        let theirs = sim.interner.intern("Russians");
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                ours, 0, None, true, 0, 10,
            ));
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                theirs, 1, None, false, 0, 10,
            ));
        sim.session.set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );
        sim.session.set_current_house_for_test(Some(ours));
        sim.session.set_binary_frame_for_test(1);
        sim.resolve_type_handles(&rules);
        let enemy = sim
            .spawn_object("TANK", "Russians", 16, 10, 0, &rules)
            .unwrap();
        let mut corpse_target = |name: &str, x: u16| {
            let id = sim
                .spawn_object(name, "Americans", x, 10, 0, &rules)
                .unwrap();
            let entity = sim.substrate.entities.get_mut(id).unwrap();
            entity
                .attack_target
                .set_for_test(Some(TargetKind::Entity(enemy)));
            entity.health.set_for_test(0);
            sim.begin_infantry_death_sequence(
                id,
                crate::sim::world::InfantryDeathSequence::Die1,
                &rules,
                crate::sim::world::FrameEffects::default(),
            );
            sim.object_ai_visit_one(id, Some(&rules), ObjectAiCtx::default());
            let corpse = sim.substrate.entities.get(id).unwrap();
            assert_eq!(corpse.mission_leaf.as_infantry().unwrap().doing(), 11);
            assert!(corpse.is_object_alive() && corpse.health.current() == 0);
            corpse.attack_target.target()
        };
        assert_eq!(corpse_target("MEDIC", 10), None);
        assert_eq!(corpse_target("E1", 12), Some(TargetKind::Entity(enemy)));
    }

    #[test]
    fn wave_context_keeps_terrain_object_coordinates_until_pointer_expiry() {
        use crate::sim::projectile::ProjectileCoord;
        use crate::sim::terrain_object::{TerrainObjectLifecycle, TerrainObjectState};
        use crate::sim::wave::Wave;

        let mut sim = Simulation::new();
        let tree_type = sim.interner.intern("TREE01");
        let tree = TerrainObjectState::for_test(2, tree_type, 7, 5);
        let retained = tree.world_coord();
        sim.terrain_objects.objects_mut_for_test().insert(2, tree);
        sim.waves.spawn(
            3,
            Wave::new_owned(
                0,
                1,
                TargetKind::Entity(2),
                ProjectileCoord::new(0, 0, 0),
                retained,
            ),
        );
        assert_eq!(
            sim.wave_update_context(sim.waves.get(3).unwrap(), None)
                .target_position,
            Some(retained)
        );
        let tree = sim
            .terrain_objects
            .objects_mut_for_test()
            .get_mut(&2)
            .unwrap();
        tree.set_health_for_test(0);
        tree.set_lifecycle_for_test(TerrainObjectLifecycle::Limbo);
        assert_eq!(
            sim.wave_update_context(sim.waves.get(3).unwrap(), None)
                .target_position,
            Some(retained)
        );
        sim.waves.pointer_expired(3, 2);
        assert_eq!(
            sim.wave_update_context(sim.waves.get(3).unwrap(), None)
                .target_position,
            None
        );
    }

    /// Build a test entity of a specific category (`test_default` makes a Unit).
    fn entity_of(id: u64, category: EntityCategory) -> GameEntity {
        let mut e = GameEntity::test_default(id, "TEST", "Americans", 5, 5);
        e.set_category_for_test(category);
        e.mission_leaf = crate::sim::mission::leaf::MissionLeafState::for_entity_category(category);
        e
    }

    fn register_entity(sim: &mut Simulation, mut entity: GameEntity) {
        entity.set_owner_for_test(sim.interner.intern("Americans"));
        entity.set_type_ref_for_test(sim.interner.intern("TEST"));
        sim.substrate.entities.insert(entity);
    }

    /// A ground-level, north-facing rookie voxel object (vision 5) at frame
    /// zero, with `owner` and `type_id` interned through `sim`.
    fn interned_entity(
        sim: &mut Simulation,
        id: u64,
        owner: &str,
        type_id: &str,
        category: EntityCategory,
        (rx, ry): (u16, u16),
        health: i32,
    ) -> GameEntity {
        let owner = sim.interner.intern(owner);
        let type_ref = sim.interner.intern(type_id);
        GameEntity::new_at_frame_zero_for_test(
            id,
            rx,
            ry,
            0,
            0,
            owner,
            crate::sim::combat::Health::new(health),
            type_ref,
            category,
            0,
            5,
            true,
        )
    }

    fn rocking_phase_fixture() -> (Simulation, RuleSet, u64, u64) {
        let mut rules = RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=TEST\n[InfantryTypes]\n0=WARPER\n\
             [TEST]\nStrength=300\nSpeed=4\nCost=100\n\
             Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
             [WARPER]\nStrength=100\nSpeed=4\nPrimary=Beam\n\
             Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
             [Beam]\nDamage=1\nRange=8\nWarhead=ChronoBeam\n\
             [ChronoBeam]\nTemporal=yes\n",
        ))
        .unwrap();
        // Explicit native loaded-model prior for this headless AI-slot
        // component fixture; asset/catalog tests prove physical admission.
        rules.supply_loaded_voxel_for_test("TEST");
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        sim.resolve_rule_animation_lists(&rules);
        let first = sim
            .spawn_object("TEST", "Americans", 5, 5, 0, &rules)
            .unwrap();
        let second = sim
            .spawn_object("TEST", "Americans", 7, 5, 0, &rules)
            .unwrap();
        (sim, rules, first, second)
    }

    #[test]
    fn rocking_tipover_uses_real_c4_receiver_before_the_alive_guard() {
        let Some(mut rules) =
            crate::rules::retail_ini_fixture::retail_battle_rules().map(|r| r.rules)
        else {
            return;
        };
        let (_, assets) = crate::rules::retail_ini_fixture::retail_assets().unwrap();
        rules.bind_effect_assets(&assets, "TEM", "TEMPERATE");
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let victim = sim
            .spawn_object("MTNK", "Americans", 5, 5, 0, &rules)
            .unwrap();
        // Explicit retained-state component input from original70B570;
        // this does not claim a stock Drone impulse reaches the tipover gate.
        let controls: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/unit_rocking_update.json",
        ))
        .unwrap();
        let row = controls
            .as_array()
            .unwrap()
            .iter()
            .find(|row| {
                row["output"]["damage_called"] == true
                    && row["input"]["direct_rocker_link"] == false
                    && row["input"]["crash"] == 0
                    && row["input"]["sinking"] == 0
            })
            .unwrap();
        let word =
            |key: &str| u32::from_str_radix(row["input"][key].as_str().unwrap(), 16).unwrap();
        let prior = serde_json::from_value(serde_json::json!({
            "angle_sideways": word("side_angle_bits"),
            "angle_forwards": word("forward_angle_bits"),
            "vel_sideways": word("side_velocity_bits"),
            "vel_forwards": word("forward_velocity_bits"),
        }))
        .unwrap();
        sim.substrate.entities.get_mut(victim).unwrap().rocking = Some(prior);
        sim.object_ai_visit_one(victim, Some(&rules), ObjectAiCtx::default());
        assert!(
            sim.substrate.entities.get(victim).is_none_or(|entity| {
                !entity.lifecycle.object_alive() || entity.health.current() <= 0
            }),
            "Rocking's C4 request must reach the real fatal receiver, not a no-op hook"
        );
    }

    /// A supplied earlier/later callback invokes the real Crash owner on a
    /// represented airborne Unit. Production live-cursor and Techno visits
    /// decide whether its new rates are consumed in this same pass.
    #[test]
    fn rocking_crash_birth_respects_the_victims_live_logic_slot() {
        use crate::util::fixed_math::SimFixed;
        for earlier in [true, false] {
            let (mut sim, rules, first, second) = rocking_phase_fixture();
            let (caller, victim) = if earlier {
                (first, second)
            } else {
                (second, first)
            };
            sim.substrate
                .entities
                .get_mut(victim)
                .unwrap()
                .position
                .set_exact_z_leptons_for_test(Some(512));
            let mut rates = None;
            sim.for_each_live_object(|sim, id| {
                if id == caller {
                    assert!(sim.foot_crash(victim, None, &rules, None, FrameEffects::default()));
                    let rocking = sim.substrate.entities.get(victim).unwrap().rocking.unwrap();
                    rates = Some((rocking.vel_sideways(), rocking.vel_forwards()));
                }
                sim.object_ai_visit_one(id, Some(&rules), ObjectAiCtx::default());
            });
            let rates = rates.unwrap();
            assert!(rates.0 != SimFixed::ZERO || rates.1 != SimFixed::ZERO);
            let rocking = sim.substrate.entities.get(victim).unwrap().rocking.unwrap();
            let expected = if earlier {
                rates
            } else {
                (SimFixed::ZERO, SimFixed::ZERO)
            };
            assert_eq!(
                (rocking.angle_sideways(), rocking.angle_forwards()),
                expected,
                "earlier={earlier}: never retroactively visit the new crash"
            );
            sim.session
                .set_binary_frame_for_test(sim.session.binary_frame() + 1);
            sim.object_ai_stage_with(Some(&rules), ObjectAiCtx::default());
            let rocking = sim.substrate.entities.get(victim).unwrap().rocking.unwrap();
            assert_eq!(
                (rocking.angle_sideways(), rocking.angle_forwards()),
                (expected.0 + rates.0, expected.1 + rates.1),
                "one integration on the next visit"
            );
        }
    }

    #[test]
    fn rocking_skips_unloaded_temporal_tube_and_limbo_entries() {
        use crate::sim::rocking::RockingState;
        use crate::util::fixed_math::SimFixed;
        for gate in ["unloaded", "temporal", "tube", "limbo"] {
            let (mut sim, mut rules, _, victim) = rocking_phase_fixture();
            let mut rocking = RockingState::default();
            rocking.set_angle_sideways(SimFixed::lit("0.1"));
            sim.substrate.entities.get_mut(victim).unwrap().rocking = Some(rocking);
            match gate {
                "unloaded" => {
                    // Replacing ART invalidates the previously supplied model.
                    rules.install_art_fixture(crate::rules::art_data::ArtRegistry::empty());
                    let object = rules.type_handle("TEST").unwrap();
                    assert!(!rules.object_has_loaded_voxel(object));
                }
                "temporal" => {
                    let warper = sim
                        .spawn_object("WARPER", "Russians", 9, 5, 0, &rules)
                        .unwrap();
                    sim.temporal_initiate_warp(
                        warper,
                        Some(victim),
                        &rules,
                        None,
                        FrameEffects::default(),
                    );
                    assert!(sim.substrate.entities.get(victim).unwrap().ai_frozen());
                }
                "tube" => {
                    sim.substrate
                        .entities
                        .get_mut(victim)
                        .unwrap()
                        .low_bridge_tube_state = Some(LowBridgeTubeMovementState::for_test(
                        TubeId(0),
                        0,
                        DriveCoord {
                            x: 9 * 256 + 128,
                            y: 5 * 256 + 128,
                            z: 0,
                        },
                    ));
                }
                "limbo" => {
                    sim.techno_limbo_with_context(
                        victim,
                        super::super::UninitContext::with_rules(&rules),
                    );
                    assert!(
                        sim.substrate
                            .entities
                            .get(victim)
                            .unwrap()
                            .lifecycle
                            .in_limbo()
                    );
                }
                _ => unreachable!(),
            }
            // Tube owns its separate Process; this component test stops at
            // the AI shell, without fabricating a Tube map or calling it twice.
            if gate == "tube" {
                sim.object_ai_stage_with(Some(&rules), ObjectAiCtx::default());
            } else {
                assert!(
                    sim.advance_tick(&[], Some(&rules), None, None, 33)
                        .frame_committed
                );
            }
            assert_eq!(
                sim.substrate
                    .entities
                    .get(victim)
                    .unwrap()
                    .rocking
                    .unwrap()
                    .angle_sideways(),
                SimFixed::lit("0.1"),
                "{gate}: Rocking slot was not reached"
            );
        }
    }

    fn unit_voice_fixture() -> (Simulation, RuleSet) {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n0=TEST\n[TEST]\nStrength=100\n",
        ))
        .expect("supplied unarmed Infantry type");
        let mut sim = Simulation::new();
        for id in [3, 5, 7, 9] {
            let mut entity = GameEntity::test_default_of_category(
                id,
                "TEST",
                "Americans",
                5,
                5,
                EntityCategory::Infantry,
            );
            // Supply placed, ordinary AI boundaries; id7 remains off Logic.
            entity.lifecycle.set_in_limbo_for_test(false);
            register_entity(&mut sim, entity);
        }
        sim.set_logic_order_for_test(vec![9, 5, 3]);
        (sim, rules)
    }

    fn unit_voice_facts(sim: &Simulation) -> Vec<(u64, bool)> {
        sim.sound_events
            .iter()
            .filter_map(|event| match event {
                super::super::SimSoundEvent::UnitVoiceVisit { owner } => Some((*owner, false)),
                super::super::SimSoundEvent::UnitVoiceDestroyed { owner } => Some((*owner, true)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn unit_voice_interest_follows_reached_logic_heads_without_sorted_or_unvisited_owners() {
        let (mut sim, rules) = unit_voice_fixture();
        sim.prepare_unit_voice_visits([3, 7, 9, 404].into());
        // Taking the interest projection without an AI pass (including a
        // paused app pass) cannot create a visit or consume any voice latch.
        assert!(unit_voice_facts(&sim).is_empty());
        sim.object_ai_stage_with(Some(&rules), ObjectAiCtx::default());
        assert_eq!(unit_voice_facts(&sim), vec![(9, false), (3, false)]);

        // No pending owners means no per-actor output for the whole live walk.
        sim.sound_events.clear_for_test();
        sim.prepare_unit_voice_visits(Default::default());
        sim.object_ai_stage_with(Some(&rules), ObjectAiCtx::default());
        assert!(unit_voice_facts(&sim).is_empty());
    }

    #[test]
    fn unit_voice_disposal_stays_ordered_after_visits_and_before_a_removed_owners_slot() {
        let (mut sim, rules) = unit_voice_fixture();
        sim.prepare_unit_voice_visits([3, 9].into());
        // Ordinary UnInit removes id3 before its slot; the common physical
        // destructor must still clear a pending, never-visited voice owner.
        sim.uninit_with_rules(3, &rules);
        sim.process_pending_delete_with(
            Some(&rules),
            None,
            crate::sim::world::FrameEffects::default(),
            1,
            false,
        );
        sim.object_ai_stage_with(Some(&rules), ObjectAiCtx::default());
        sim.uninit_with_rules(9, &rules);
        sim.process_pending_delete_with(
            Some(&rules),
            None,
            crate::sim::world::FrameEffects::default(),
            1,
            false,
        );
        assert_eq!(
            unit_voice_facts(&sim),
            vec![(3, true), (9, false), (9, true)]
        );

        // Repeating the drain cannot publish a second destructor marker.
        sim.process_pending_delete_with(
            Some(&rules),
            None,
            crate::sim::world::FrameEffects::default(),
            1,
            false,
        );
        assert_eq!(
            unit_voice_facts(&sim),
            vec![(3, true), (9, false), (9, true)]
        );
    }

    #[test]
    fn unit_voice_interest_is_not_snapshot_or_hash_state_and_expires_after_a_frame() {
        let mut sim = Simulation::new();
        let baseline = GameSnapshot::save(&sim, 0, 0, "voice_interest", 0);
        let hash = sim.state_hash();
        sim.prepare_unit_voice_visits([3, 9].into());
        assert_eq!(sim.state_hash(), hash);
        assert_eq!(
            GameSnapshot::save(&sim, 0, 0, "voice_interest", 0),
            baseline
        );
        let mut restored = GameSnapshot::load(&baseline).expect("snapshot").sim;
        restored.record_unit_voice_visit(3);
        assert!(unit_voice_facts(&restored).is_empty());

        sim.advance_app_frame(
            &[],
            None,
            None,
            67,
            super::super::TickLane::Ordinary,
            crate::sim::world::FrameEffects::default(),
        )
        .expect("empty authoritative frame");
        sim.record_unit_voice_visit(3);
        assert!(
            unit_voice_facts(&sim).is_empty(),
            "no stale interest after a frame"
        );
    }

    fn mission_test_fixture(mission: &MissionCom) -> MissionTestFixture {
        MissionTestFixture {
            current: mission.current(),
            suspended: mission.suspended(),
            queued: mission.queued(),
            movement_bypass_latch: mission.movement_bypass_latch(),
            handler_state: mission.handler_state(),
            mission_start_frame: mission.mission_start_frame(),
            ai_counter: mission.ai_counter(),
            dispatch_timer: mission.dispatch_timer(),
        }
    }

    fn update_mission_test_fixture(
        mission: &mut MissionCom,
        update: impl FnOnce(&mut MissionTestFixture),
    ) {
        let mut fixture = mission_test_fixture(mission);
        update(&mut fixture);
        mission.apply_test_fixture(fixture);
    }

    fn target_of(sim: &Simulation, id: u64) -> Option<TargetKind> {
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .attack_target
            .target()
    }

    fn dispatch_timer_of(sim: &Simulation, id: u64) -> MissionDispatchTimer {
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .mission
            .dispatch_timer()
    }

    /// Commit `current` with its dispatch timer due from `frame`.
    fn commit_due_mission(mission: &mut MissionCom, current: MissionType, frame: u32) {
        update_mission_test_fixture(mission, |fixture| {
            fixture.current = MissionId::from_known(current);
            fixture.dispatch_timer = MissionDispatchTimer::at_frame(frame);
        });
    }

    #[test]
    fn object_ai_stage_visits_dying_object_in_its_live_slot() {
        let mut sim = Simulation::new();
        sim.substrate
            .entities
            .insert(entity_of(1, EntityCategory::Unit));
        sim.substrate
            .entities
            .insert(entity_of(2, EntityCategory::Unit));
        sim.set_logic_order_for_test(vec![1, 2]);
        // Mark id 2 dying AFTER set_logic_order_for_test — that helper resets
        // presence / in_logic_vector but does NOT touch `dying`, and id 2 stays
        // in the live order.
        sim.substrate
            .entities
            .get_mut(2)
            .unwrap()
            .death
            .set_dying_for_test(true);

        let visited = sim.object_ai_walk(true, None, ObjectAiCtx::default());
        assert_eq!(
            visited,
            vec![1, 2],
            "a dying object keeps receiving its live scheduler slot until UnInit"
        );
        // With no sequence table this fixture cannot finish the death action,
        // so the second visit must leave the object registered for a later pass.
        sim.object_ai_stage(None);
        assert_eq!(sim.live_object_order_snapshot(), vec![1, 2]);
    }

    // ===== TechnoClass common bracket =====

    #[test]
    fn bracket_pre_guard_short_circuits_dead_unit() {
        let mut sim = Simulation::new();
        let mut e = entity_of(1, EntityCategory::Unit);
        // Killed, its UnInit deferred: native has already cleared IsAlive.
        e.health.set_for_test(0);
        e.lifecycle.set_object_alive_for_test(false);
        sim.substrate.entities.insert(e);
        // Guard B (IsAlive) fires after the (empty) pre-block: a dead Unit runs
        // no mission work (counter stays 0).
        assert_eq!(
            unit_techno_bracket(
                &mut sim,
                1,
                None,
                ObjectAiCtx::default(),
                &mut ObjectAiOutcome::default()
            ),
            BracketReach::DiedInPre
        );
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().mission.ai_counter(),
            0
        );
    }

    // ===== Passive acquisition — production line =====

    /// Rules for the passive-acquire tests. `MTNK` is an ordinary armed tank
    /// (no `OpportunityFire` — it must still acquire through the Guard arm),
    /// `NOACQ` is the same tank with the type-level opt-out, and `NASAM` is an
    /// armed `Powered=yes` defence that drains power.
    fn passive_rules() -> RuleSet {
        RuleSet::from_ini(&IniFile::from_str(
            "[General]\nNormalTargetingDelay=27\nGuardAreaTargetingDelay=36\n\n\
             [InfantryTypes]\n0=GI\n[AircraftTypes]\n\
             [VehicleTypes]\n0=MTNK\n1=NOACQ\n2=UNARM\n3=OPPTNK\n4=SPRAY\n\
             [BuildingTypes]\n0=NASAM\n1=GAPOWR\n2=GARR\n\n\
             [GARR]\nStrength=750\nArmor=wood\nFoundation=2x2\nSight=5\nCanBeOccupied=yes\n\
             MaxNumberOccupants=5\nPrimary=105mm\n\n\
             [GAPOWR]\nStrength=750\nArmor=wood\nFoundation=2x2\nSight=5\nPower=100\n\n\
             [GI]\nLocomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
             Strength=125\nArmor=none\nSpeed=4\nSight=10\nPrimary=105mm\n\n\
             [MTNK]\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
             Strength=300\nArmor=heavy\nSpeed=6\nSight=10\nPrimary=105mm\n\n\
             [NOACQ]\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
             Strength=300\nArmor=heavy\nSpeed=6\nSight=10\nPrimary=105mm\nCanPassiveAquire=no\n\n\
             [OPPTNK]\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
             Strength=300\nArmor=heavy\nSpeed=6\nSight=10\nPrimary=105mm\nOpportunityFire=yes\n\n\
             [SPRAY]\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
             Strength=300\nArmor=heavy\nSpeed=6\nSight=10\nPrimary=AFIRE\nSprayAttack=yes\n\n\
             [AFIRE]\nDamage=10\nROF=50\nRange=2\nWarhead=AP\nAreaFire=yes\n\n\
             [UNARM]\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
             Strength=300\nArmor=heavy\nSpeed=6\nSight=10\n\n\
             [NASAM]\nStrength=1000\nArmor=wood\nFoundation=1x1\nSight=10\n\
             Primary=105mm\nPowered=yes\nPower=-50\n\n\
             [105mm]\nDamage=65\nROF=50\nRange=6\nWarhead=AP\n\n\
             [AP]\nVerses=100%,100%,90%,75%,75%,75%,60%,30%,20%,0%,0%\n",
        ))
        .expect("passive-acquire test rules parse")
    }

    fn passive_map_entity(
        owner: &str,
        type_id: &str,
        cx: u16,
        cy: u16,
        category: EntityCategory,
    ) -> crate::map::entities::MapEntity {
        crate::map::entities::MapEntity {
            owner: owner.to_string(),
            type_id: type_id.to_string(),
            health: 256,
            cell_x: cx,
            cell_y: cy,
            facing: 64,
            category,
            sub_cell: 0,
            veterancy: Some(0),
            high: false,
            mission: None,
            trigger_tag: None,
            recruitable_a: true,
            recruitable_b: true,
            structure_upgrades: [None, None, None],
            structure_ai_sellable: false,
            structure_ai_repairable: false,
            structure_online: true,
        }
    }

    /// A `guard_type` on an authored Guard at (20, 20) and an unarmed `UNARM`
    /// three cells east, spawned through the map path, with the path grid to
    /// tick them on. Entity 1 is the guard, entity 2 the enemy.
    fn guard_beside_unarmed(
        rules: &RuleSet,
        guard_type: &str,
    ) -> (Simulation, crate::sim::pathfinding::PathGrid) {
        let mut sim = Simulation::with_seed(0x5CA1_AB1E_0009);
        let guard = crate::map::entities::MapEntity {
            mission: Some(MissionType::Guard),
            ..passive_map_entity("Americans", guard_type, 20, 20, EntityCategory::Unit)
        };
        let enemy = passive_map_entity("Soviet", "UNARM", 23, 20, EntityCategory::Unit);
        sim.spawn_from_map(&[guard, enemy], Some(rules));
        (sim, crate::sim::pathfinding::PathGrid::new(64, 64))
    }

    /// A crashing wreck (Health 0, alive until its impact) keeps its Techno
    /// and Unit AI while it falls: on Guard the passive block's scan picks up
    /// an enemy in range and its fire update shoots it (`TechnoClass::
    /// AI_Update`'s IsAlive gates `0x006FA23C`/`0x006FA735`, `UnitClass::AI
    /// 0x007365BB`), but `MissionClass::AI` runs no Guard handler for it
    /// (`0x005B30A7`), so its dispatch timer stands.
    #[test]
    fn a_falling_wreck_on_guard_acquires_and_fires_but_runs_no_handler() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[General]\nNormalTargetingDelay=27\nGuardAreaTargetingDelay=36\n\n\
             [Guard]\nRate=.016\n\n\
             [InfantryTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
             [VehicleTypes]\n0=MTNK\n1=UNARM\n\
             [MTNK]\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
             Strength=300\nArmor=heavy\nSpeed=6\nSight=10\nPrimary=105mm\n\n\
             [UNARM]\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
             Strength=3000\nArmor=heavy\nSpeed=6\nSight=10\n\n\
             [105mm]\nDamage=65\nROF=50\nRange=6\nWarhead=AP\n\n\
             [AP]\nVerses=100%,100%,90%,75%,75%,75%,60%,30%,20%,0%,0%\n",
        ))
        .expect("wreck rules parse");
        let (mut sim, grid) = guard_beside_unarmed(&rules, "MTNK");
        let timer = {
            let wreck = sim.substrate.entities.get_mut(1).expect("tank present");
            wreck.health.set_for_test(0);
            wreck.crash.set_crashing_for_test(true);
            wreck.mission.dispatch_timer()
        };
        let mut acquired = false;
        for _ in 0..200 {
            let _ = sim.advance_tick(&[], Some(&rules), Some(&grid), None, 67);
            let wreck = sim.substrate.entities.get(1).expect("the wreck stays");
            assert_eq!(wreck.health.current(), 0, "a wreck does not heal");
            assert_eq!(
                wreck.mission.dispatch_timer(),
                timer,
                "no Guard handler ran, so its epilogue never wrote the timer"
            );
            acquired |= wreck.attack_target.target().is_some() && wreck.passive_scan.acquired();
            if sim.substrate.entities.get(2).unwrap().health.current() < 3000 {
                break;
            }
        }
        assert!(acquired, "the passive scan picked the enemy up");
        assert!(
            sim.substrate.entities.get(2).unwrap().health.current() < 3000,
            "the wreck's fire update shot it"
        );
    }

    #[test]
    fn techno_playfield_stored_membership_gates_same_frame_passive_targeting() {
        let rules = passive_rules();
        let grid = crate::sim::pathfinding::PathGrid::new(64, 64);
        let mut sim = Simulation::with_seed(0x3D5);
        sim.spawn_from_map(
            &[
                passive_map_entity("Americans", "MTNK", 20, 20, EntityCategory::Unit),
                passive_map_entity("Soviet", "UNARM", 23, 20, EntityCategory::Unit),
            ],
            Some(&rules),
        );
        sim.playfield_bounds = Some(
            crate::map::playfield::PlayfieldBounds::from_normalized_local_size(64, 2, 2, 56, 52),
        );
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .set_in_playfield(true);
        sim.substrate
            .entities
            .get_mut(2)
            .unwrap()
            .set_in_playfield(false);

        for _ in 0..90 {
            let _ = sim.advance_tick(&[], Some(&rules), Some(&grid), None, 67);
        }
        assert!(target_of(&sim, 1).is_none());

        sim.substrate
            .entities
            .get_mut(2)
            .unwrap()
            .set_in_playfield(true);
        for _ in 0..90 {
            let _ = sim.advance_tick(&[], Some(&rules), Some(&grid), None, 67);
        }
        assert!(
            target_of(&sim, 1).is_some(),
            "Evaluate_Candidate @ 0x006F7DB0 admits the stored true member"
        );
    }

    #[test]
    fn a_unit_that_passively_acquires_does_not_move_toward_the_target() {
        // The passive commit writes the target pointer only; a Guard unit fires
        // from where it stands, and the reach block approaches only a target in
        // another zone. A unit that chased these targets would walk off across
        // the map with no order to bring it home.
        //
        // A target can only be acquired IN range, so the scenario has to open a
        // range gap afterwards: the unarmed Allied scout parks two cells away
        // until the Soviet tank picks it up, then drives off. That is exactly
        // the "an enemy scouted past my base" case. The scout is unarmed, so it
        // never shoots and no retaliation can install a target another way.
        let rules = passive_rules();
        let mut sim = Simulation::with_seed(0x5CA1_AB1E_0002);
        let grid = crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
        sim.spawn_from_map(
            &[
                passive_map_entity("Americans", "UNARM", 22, 20, EntityCategory::Unit),
                passive_map_entity("Soviet", "MTNK", 20, 20, EntityCategory::Unit),
            ],
            Some(&rules),
        );
        let allied = sim.interner.get("Americans").expect("Americans interned");
        let start = sim
            .substrate
            .entities
            .get(2)
            .map(|e| (e.position.rx(), e.position.ry()))
            .expect("soviet tank present");

        // Let the Soviet tank acquire the parked scout.
        for _ in 0..60 {
            let _ = sim.advance_tick(&[], Some(&rules), Some(&grid), None, 67);
        }
        let soviet = sim.substrate.entities.get(2).expect("soviet tank present");
        assert!(
            soviet.attack_target.target().is_some() && soviet.passive_scan.acquired(),
            "precondition: the idle tank must have picked the scout up on its own"
        );

        // Now the scout runs. The tank must not follow it.
        let scram = crate::sim::command::CommandEnvelope::new(
            allied,
            61,
            crate::sim::command::Command::Move {
                entity_id: 1,
                target_rx: 30,
                target_ry: 20,
            },
        );
        for tick in 60..260u64 {
            let due: Vec<crate::sim::command::CommandEnvelope> = if tick + 1 == 61 {
                vec![scram.clone()]
            } else {
                Vec::new()
            };
            let _ = sim.advance_tick(&due, Some(&rules), Some(&grid), None, 67);
        }
        let scout = sim.substrate.entities.get(1).expect("scout present");
        assert!(
            scout.position.rx() >= 27, // beyond Range=6 of the tank at 20
            "precondition: the scout actually ran out of the tank's weapon range"
        );
        let soviet = sim.substrate.entities.get(2).expect("soviet tank present");
        assert_eq!(
            (soviet.position.rx(), soviet.position.ry()),
            start,
            "a passively-acquired target must never be pursued — the unit holds its ground"
        );
        assert!(
            crate::sim::movement::motion_query::is_moving(&soviet) != Some(true),
            "no movement may be issued for a scanner-acquired target"
        );
    }

    #[test]
    fn after_a_kill_the_next_passive_scan_picks_again() {
        // Native has no retarget on a kill: pointer expiry clears the killer's
        // target (`0x007077C0`) and the next passive scan picks again, marking
        // the new target as its own (`0x006FA6EE`). The invariant is checked
        // every tick: a target the tank was never ordered onto is always
        // scanner-owned.
        let rules = passive_rules();
        let grid = crate::sim::pathfinding::PathGrid::new(64, 64);
        let mut sim = Simulation::with_seed(0x5CA1_AB1E_0007);
        sim.spawn_from_map(
            &[
                passive_map_entity("Americans", "MTNK", 20, 20, EntityCategory::Unit),
                passive_map_entity("Soviet", "UNARM", 21, 20, EntityCategory::Unit),
                passive_map_entity("Soviet", "UNARM", 23, 20, EntityCategory::Unit),
            ],
            Some(&rules),
        );
        for _ in 0..60 {
            let _ = sim.advance_tick(&[], Some(&rules), Some(&grid), None, 67);
        }
        assert_eq!(
            target_of(&sim, 1),
            Some(TargetKind::Entity(2)),
            "precondition: the scanner picked the nearer candidate"
        );

        // Put the current victim one hit from death and let combat finish it.
        // The invariant is checked EVERY tick, not just at the end: the leak
        // opens on the exact tick the victim dies and can be papered over by the
        // next cadence rescan, so an end-state assertion would miss it.
        sim.substrate
            .entities
            .get_mut(2)
            .unwrap()
            .health
            .set_for_test(1);
        for tick in 0..80 {
            let _ = sim.advance_tick(&[], Some(&rules), Some(&grid), None, 67);
            let tank = sim.substrate.entities.get(1).expect("tank present");
            assert!(
                tank.attack_target.target().is_none() || tank.passive_scan.acquired(),
                "tick {tick}: the tank was never ordered to attack anything, so a target it \
                 holds must always be scanner-owned — a live target with the flag false takes \
                 it out of the passive block, and it never releases"
            );
        }
        assert!(
            sim.substrate
                .entities
                .get(2)
                .is_none_or(|e| e.health.current() == 0 || e.death.is_dying()),
            "precondition: the first victim died"
        );
        let tank = sim.substrate.entities.get(1).expect("tank present");
        assert!(
            tank.attack_target.target().is_some() && tank.passive_scan.acquired(),
            "precondition: the second candidate is still in range and was taken up"
        );
    }

    fn run_captured_garrison(
        rules: &RuleSet,
        building_type: &str,
        occupant_type: &str,
        enemy_type: &str,
        enemy: (u16, u16),
    ) -> (Option<TargetKind>, i32) {
        let mut sim = Simulation::with_seed(0x5CA1_AB1E_000B);
        let grid = crate::sim::arena_fixture::flat_ground(&mut sim, rules);
        sim.spawn_from_map(
            &[
                passive_map_entity(
                    "Americans",
                    building_type,
                    10,
                    10,
                    EntityCategory::Structure,
                ),
                passive_map_entity("Americans", occupant_type, 9, 10, EntityCategory::Infantry),
                passive_map_entity("Soviet", enemy_type, enemy.0, enemy.1, EntityCategory::Unit),
            ],
            Some(rules),
        );
        // A human's probe: ReceiveDamage's scatter tail moves a computer's
        // unarmed unit off the reach edge (`0x00702C8A`); this measures reach.
        let soviet = sim.interner.intern("Soviet");
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                soviet, 1, None, true, 0, 10,
            ));
        // Capture walks onto the foundation before PerCellProcess admits
        // the occupant (InfantryClass::PerCellProcess @ 0x00519675).
        assert!(sim.apply_command(
            "Americans",
            &crate::sim::command::Command::CaptureBuilding {
                engineer_id: 2,
                target_building_id: 1,
            },
            Some(rules),
        ));
        for _ in 0..160 {
            let _ = sim.advance_tick(&[], Some(rules), Some(&grid), None, 67);
        }
        assert!(
            sim.substrate
                .entities
                .get(1)
                .and_then(|house| house.passenger_role.cargo())
                .is_some_and(|cargo| cargo.passengers() == [2]),
            "precondition: Capture admitted the occupant"
        );
        (
            target_of(&sim, 1),
            sim.substrate.entities.get(3).unwrap().health.current(),
        )
    }

    #[test]
    fn a_retail_garrison_acquires_and_fires_with_its_art_foundation() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules() else {
            return;
        };
        assert_eq!(rules.object("CABUNK01").unwrap().foundation, "2x2");
        let initial_health = rules.object("AMCV").unwrap().strength;
        let (target, health) = run_captured_garrison(&rules, "CABUNK01", "E1", "AMCV", (10, 16));
        assert_eq!(target, Some(TargetKind::Entity(3)));
        assert!(
            health < initial_health,
            "the retail GI fired from the bunker"
        );
        let (target, health) = run_captured_garrison(&rules, "CABUNK01", "E1", "AMCV", (10, 17));
        assert_eq!(target, None);
        assert_eq!(
            health, initial_health,
            "the MCV outside occupied reach was not shot"
        );
    }

    #[test]
    fn queued_guard_does_not_open_passive_gate_before_commence() {
        // The original FV collapse continuation reaches TechnoAI6FA697 with
        // Attack still committed and Guard only queued. Its native scan is
        // absent until UnitAI's later Commence has promoted that selector.
        let rules = passive_rules();
        let mut sim = Simulation::new();
        insert_scannable(&mut sim, 1, "Americans", "MTNK", EntityCategory::Unit);
        sim.session.set_binary_frame_for_test(45);
        sim.substrate
            .mission_assign_exact(1, MissionId::from_known(MissionType::Attack), 45)
            .unwrap();
        sim.mission_queue_exact(
            1,
            MissionId::from_known(MissionType::Guard),
            0,
            45,
            &crate::sim::mission::authority::EntityReadyInputProvider,
        )
        .unwrap();
        let before = sim.scenario_rng.logical_state();
        let timer = sim.substrate.entities.get(1).unwrap().passive_scan.timer();
        passive_acquire_step(&mut sim, 1, Some(&rules), ObjectAiCtx::default());
        assert_eq!(sim.scenario_rng.logical_state(), before);
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().passive_scan.timer(),
            timer
        );

        sim.substrate.mission_commence_exact(1, 45).unwrap();
        sim.session.set_binary_frame_for_test(46);
        passive_acquire_step(&mut sim, 1, Some(&rules), ObjectAiCtx::default());
        assert_ne!(sim.scenario_rng.logical_state(), before);
        assert_eq!(
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .passive_scan
                .last_scan_frame(),
            46
        );
    }

    fn fixture_with_current(mission: &MissionCom, current: MissionType) -> MissionTestFixture {
        let mut fixture = mission_test_fixture(mission);
        fixture.current = MissionId::from_known(current);
        fixture
    }

    #[test]
    fn an_authored_map_mission_survives_the_spawn_and_suppresses_acquisition() {
        // The middle stage of the MISSION= column's route: parse (map/entities)
        // -> `commit_map_placement_mission` -> `passive_acquire_mission`. Stock
        // maps park 46 civilian objects on Sticky and 627 on Sleep; if the
        // authored value is dropped at spawn they all read as Guard and shoot.
        // The GI is armed and its hostile neighbour is in range, so on the
        // derived Guard it would acquire; the neighbour is `UNARM` so nothing
        // can shoot back and drag the GI onto Attack by retaliation instead.
        let rules = passive_rules();
        let grid = crate::sim::pathfinding::PathGrid::new(64, 64);
        let mut sim = Simulation::with_seed(0x5CA1_AB1E_0011);
        let mut sticky = passive_map_entity("Soviet", "GI", 20, 20, EntityCategory::Infantry);
        sticky.mission = Some(MissionType::Sticky);
        sim.spawn_from_map(
            &[
                passive_map_entity("Americans", "UNARM", 22, 20, EntityCategory::Unit),
                sticky,
            ],
            Some(&rules),
        );
        let spawned = sim.substrate.entities.get(2).expect("infantry present");
        assert_eq!(
            spawned.mission.current().known(),
            Some(MissionType::Sticky),
            "the authored MISSION= column must reach the committed selector at spawn"
        );
        assert_eq!(
            spawned.passive_acquire_mission(),
            MissionType::Sticky,
            "the authored Sticky selector must beat the derived Guard"
        );
        for _ in 0..90 {
            let _ = sim.advance_tick(&[], Some(&rules), Some(&grid), None, 67);
        }
        let sticky_unit = sim.substrate.entities.get(2).expect("infantry present");
        assert_eq!(
            sticky_unit.mission.current().known(),
            Some(MissionType::Sticky),
            "nothing retasked it, so it must still be on Sticky"
        );
        assert!(
            sticky_unit.attack_target.target().is_none(),
            "Sticky is outside the passive-acquire gate's three admitted missions"
        );
    }

    /// Insert one object of `type_id` owned by `owner` with the passive-scan
    /// timer already due, so the next `passive_acquire_step` reaches the gate.
    fn insert_scannable(
        sim: &mut Simulation,
        id: u64,
        owner: &str,
        type_id: &str,
        category: EntityCategory,
    ) {
        let mut e = interned_entity(sim, id, owner, type_id, category, (5, 5), 100);
        e.set_category_for_test(category);
        e.mission
            .apply_test_fixture(fixture_with_current(&e.mission, MissionType::Guard));
        e.passive_scan.timer_mut_for_test().clear(); // due now
        sim.substrate.entities.insert(e);
    }

    /// `TechnoClass::PassiveAcquireGate @ 0x00709290`, arm by arm, through
    /// the passive block. The scan's first act is its draw, so a draw means
    /// the gate passed.
    #[test]
    fn passive_gate_arms_follow_the_original() {
        let rules = passive_rules();
        let mut sim = Simulation::new();
        let scans = |sim: &mut Simulation, id: u64| {
            let before = sim.scenario_rng.state();
            passive_acquire_step(sim, id, Some(&rules), ObjectAiCtx::default());
            sim.scenario_rng.state() != before
        };
        let on_move = |sim: &mut Simulation, id: u64| {
            let entity = sim.substrate.entities.get_mut(id).unwrap();
            update_mission_test_fixture(&mut entity.mission, |fixture| {
                fixture.current = MissionId::from_known(MissionType::Move);
            });
            entity.navigation.set_nav_com_for_test(Some(
                crate::sim::components::NavTargetRef::Cell { rx: 30, ry: 30 },
            ));
        };

        // Guard passes without OpportunityFire.
        insert_scannable(&mut sim, 1, "Americans", "MTNK", EntityCategory::Unit);
        assert!(scans(&mut sim, 1));
        // Move needs OpportunityFire (`0x007093DF`).
        insert_scannable(&mut sim, 2, "Americans", "MTNK", EntityCategory::Unit);
        on_move(&mut sim, 2);
        assert!(!scans(&mut sim, 2));
        insert_scannable(&mut sim, 3, "Americans", "OPPTNK", EntityCategory::Unit);
        on_move(&mut sim, 3);
        assert!(scans(&mut sim, 3));
        // On Guard, a SprayAttack type's slot-0 AreaFire weapon that
        // SelectWeapon picks refuses (`0x007093F8..0x00709449`).
        insert_scannable(&mut sim, 4, "Americans", "SPRAY", EntityCategory::Unit);
        assert!(!scans(&mut sim, 4));
        // A computer Foot with no target, on Move in an Aggressive, non-Suicide
        // team, passes without CanAcquireTarget (`0x0070929A..0x007092EC`):
        // here a `CanPassiveAquire=no` type.
        insert_scannable(&mut sim, 5, "Soviet", "NOACQ", EntityCategory::Unit);
        on_move(&mut sim, 5);
        assert!(
            !scans(&mut sim, 5),
            "outside a team CanAcquireTarget refuses"
        );
        crate::sim::team_script_vm::join_team_for_test(&mut sim, &[5], false, true);
        sim.substrate
            .entities
            .get_mut(5)
            .unwrap()
            .passive_scan
            .timer_mut_for_test()
            .clear();
        assert!(scans(&mut sim, 5));
        // The same arm is closed to a human house.
        insert_scannable(&mut sim, 6, "Americans", "NOACQ", EntityCategory::Unit);
        on_move(&mut sim, 6);
        let americans = sim.interner.intern("Americans");
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                americans, 0, None, true, 0, 10,
            ));
        crate::sim::team_script_vm::join_team_for_test(&mut sim, &[6], false, true);
        assert!(!scans(&mut sim, 6));
    }

    /// `TechnoClass::AI_Update @ 0x006FA30C..0x006FA46C`: a computer house's
    /// object drops a target its house is allied with, unless it is berserk;
    /// a human house's keeps it.
    #[test]
    fn a_computer_object_drops_an_allied_target() {
        let rules = passive_rules();
        let mut sim = Simulation::new();
        insert_scannable(&mut sim, 1, "Soviet", "MTNK", EntityCategory::Unit);
        insert_scannable(&mut sim, 2, "Soviet", "MTNK", EntityCategory::Unit);
        let soviet = sim.interner.intern("Soviet");
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                soviet, 0, None, false, 0, 10,
            ));
        let aim = |sim: &mut Simulation| {
            sim.substrate
                .entities
                .get_mut(1)
                .unwrap()
                .attack_target
                .set_for_test(Some(TargetKind::Entity(2)));
        };
        let held = |sim: &Simulation| target_of(sim, 1).is_some();

        aim(&mut sim);
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .berserk
            .set_active_for_test(true);
        allied_target_drop_step(&mut sim, 1, &rules);
        assert!(held(&sim), "a berserk object keeps it");

        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .berserk
            .set_active_for_test(false);
        allied_target_drop_step(&mut sim, 1, &rules);
        assert!(!held(&sim), "a computer house drops it");

        aim(&mut sim);
        sim.houses
            .get_mut(&soviet)
            .unwrap()
            .set_is_human_for_test(true);
        allied_target_drop_step(&mut sim, 1, &rules);
        assert!(held(&sim), "a human house keeps it");
    }

    /// `TechnoClass::AI_Update @ 0x006F9F0D..0x006F9F6E`: a berserk object
    /// counts its time down each frame and, once it reaches zero, calms down:
    /// flag and time clear, its target goes, and it queues Guard for a human
    /// house, else Hunt.
    #[test]
    fn a_berserk_object_calms_down_when_its_time_runs_out() {
        let rules = passive_rules();
        for human in [false, true] {
            let mut sim = Simulation::new();
            insert_scannable(&mut sim, 1, "Soviet", "MTNK", EntityCategory::Unit);
            insert_scannable(&mut sim, 2, "Soviet", "MTNK", EntityCategory::Unit);
            let soviet = sim.interner.intern("Soviet");
            sim.houses
                .register(crate::sim::house_state::HouseState::new(
                    soviet, 0, None, human, 0, 10,
                ));
            let actor = sim.substrate.entities.get_mut(1).unwrap();
            actor
                .mission
                .apply_test_fixture(fixture_with_current(&actor.mission, MissionType::Move));
            actor
                .attack_target
                .set_for_test(Some(TargetKind::Entity(2)));
            actor.berserk.set_active_for_test(true);
            actor.berserk.set_timer_for_test(2);
            let step = |sim: &mut Simulation| {
                berserk_step(
                    sim,
                    1,
                    &rules,
                    None,
                    crate::sim::world::FrameEffects::default(),
                );
                let actor = sim.substrate.entities.get(1).unwrap();
                (
                    actor.berserk.is_active(),
                    actor.berserk.timer(),
                    actor.attack_target.target().is_some(),
                    actor.mission.queued().known(),
                )
            };
            assert_eq!(step(&mut sim), (true, 1, true, None));
            let calm = if human {
                MissionType::Guard
            } else {
                MissionType::Hunt
            };
            assert_eq!(step(&mut sim), (false, 0, false, Some(calm)), "{human}");
        }
    }

    #[test]
    fn common_ai_target_drops_run_the_infantry_class_effects() {
        // Native6FA46C/6FA4CB call+3C8 before Rescue/AreaGuard dispatch.
        // The ordinary51B1F0 NULL controls independently pin its68D clear
        // and path-head invalidation in mission::infantry_target_tests.
        let rules = passive_rules();
        for allied in [true, false] {
            let mut sim = Simulation::new();
            sim.session.set_binary_frame_for_test(16);
            insert_scannable(&mut sim, 1, "Soviet", "GI", EntityCategory::Infantry);
            insert_scannable(
                &mut sim,
                2,
                if allied { "Soviet" } else { "Americans" },
                "MTNK",
                EntityCategory::Unit,
            );
            let owner = sim.intern("Soviet");
            sim.houses
                .register(crate::sim::house_state::HouseState::new(
                    owner, 0, None, false, 0, 10,
                ));
            let actor = sim.substrate.entities.get_mut(1).unwrap();
            actor
                .attack_target
                .set_for_test(Some(TargetKind::Entity(2)));
            actor.mission_leaf.set_infantry_doing_verified(0).unwrap();
            actor.mission_leaf.set_foot_firing_sequence(1);
            actor
                .navigation
                .path_replay_mut()
                .set_directions_for_test(vec![0, 1, 2]);
            actor.navigation.path_replay_mut().set_cursor_for_test(0);
            if allied {
                allied_target_drop_step(&mut sim, 1, &rules);
            } else {
                // The supplied target is still in limbo, hence ILLEGAL on
                // the no-range query, as the native pointer gate requires.
                let target = crate::sim::combat::TargetKind::Entity(2);
                let weapon = target_scan::select_weapon(&sim, &rules, 1, Some(target));
                assert!(matches!(
                    target_scan::fire_error_at(&sim, &rules, 1, Some(target), weapon, false, None),
                    crate::sim::combat::fire_error::FireError::Illegal
                        | crate::sim::combat::fire_error::FireError::Cant
                ));
                illegal_target_drop_step(&mut sim, 1, &rules);
            }
            let actor = sim.substrate.entities.get(1).unwrap();
            assert!(actor.attack_target.target().is_none());
            assert_eq!(actor.mission_leaf.foot_firing_sequence_latch(), 0);
            assert!(
                actor
                    .navigation
                    .path_replay()
                    .remaining_directions()
                    .is_empty()
            );
            assert_eq!(&actor.navigation.path_replay().directions()[1..], &[1, 2]);
        }
    }

    #[test]
    fn power_does_not_gate_the_scan_but_an_empty_garrison_does() {
        // `CanAcquireTarget @ 0x007091D0` has no power term: an unpowered
        // defence still scans (its own fire routine refuses the shot). Its
        // building arm refuses a `CanBeOccupied=` building with no occupants
        // (`0x0070920D..0x0070922E`). The scan's first act is its draw.
        let rules = passive_rules();
        let mut sim = Simulation::new();
        insert_scannable(&mut sim, 1, "Americans", "NASAM", EntityCategory::Structure);
        insert_scannable(&mut sim, 2, "Americans", "GARR", EntityCategory::Structure);
        // Both guard, as a placed building does after its first Update.
        for id in [1, 2] {
            update_mission_test_fixture(
                &mut sim.substrate.entities.get_mut(id).unwrap().mission,
                |fixture| fixture.current = MissionId::from_known(MissionType::Guard),
            );
        }
        let owner = sim.interner.intern("Americans");
        let mut power = crate::sim::power_system::PowerState::default();
        power.set_total_drain_for_test(100);
        power.set_is_low_power_for_test(true);
        sim.power_states.map_mut_for_test().insert(owner, power);

        let before = sim.scenario_rng.state();
        passive_acquire_step(&mut sim, 1, Some(&rules), ObjectAiCtx::default());
        assert_ne!(
            sim.scenario_rng.state(),
            before,
            "an unpowered defence still reaches the scanner"
        );

        let before = sim.scenario_rng.state();
        passive_acquire_step(&mut sim, 2, Some(&rules), ObjectAiCtx::default());
        assert_eq!(
            sim.scenario_rng.state(),
            before,
            "an empty garrisonable building does not"
        );
    }

    #[test]
    fn off_mission_clear_drops_a_scanner_target_but_not_an_ordered_one() {
        let mut sim = Simulation::new();
        insert_scannable(&mut sim, 1, "Americans", "MTNK", EntityCategory::Unit);
        let now = sim.session.binary_frame();
        // Unload is one of the twelve missions that drop a scanner target. The
        // job has to be LIVE for the committed selector to be read — a mission
        // whose machinery has gone quiet defers to the derived reading, which is
        // what stops a finished order from deafening a unit forever.
        sim.substrate
            .mission_assign_exact(1, MissionId::from_known(MissionType::Unload), now)
            .unwrap();
        {
            let e = sim.substrate.entities.get_mut(1).unwrap();
            e.attack_target.set_for_test(Some(TargetKind::Entity(9)));
            e.passive_scan.set_passively_acquired_target_for_test(true);
        }
        clear_passive_target_off_mission(&mut sim, 1, None);
        let e = sim.substrate.entities.get(1).unwrap();
        assert!(
            e.attack_target.target().is_none(),
            "a scanner target is dropped"
        );
        assert!(!e.passive_scan.acquired());

        // An ordered target on the same mission is left alone.
        {
            let e = sim.substrate.entities.get_mut(1).unwrap();
            e.attack_target.set_for_test(Some(TargetKind::Entity(9)));
            e.passive_scan.set_passively_acquired_target_for_test(false);
        }
        clear_passive_target_off_mission(&mut sim, 1, None);
        assert!(
            target_of(&sim, 1).is_some(),
            "only a scanner-acquired target is dropped"
        );
    }

    // ===== Foot mission handlers through the object-AI host =====

    const ORDINARY_DRIVE_HOST_ID: u64 = 41;

    fn ordinary_drive_host_sim(seed: u64) -> Simulation {
        let mut sim = Simulation::with_seed(seed);
        let mut entity = entity_of(ORDINARY_DRIVE_HOST_ID, EntityCategory::Unit);
        entity.set_owner_for_test(sim.interner.intern("Americans"));
        entity.set_type_ref_for_test(sim.interner.intern("TEST"));
        entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
        entity
            .locomotor
            .as_mut()
            .unwrap()
            .ensure_installed_track_state();
        entity
            .navigation
            .set_nav_com_for_test(Some(NavTargetRef::cell(8, 8)));
        commit_due_mission(&mut entity.mission, MissionType::Move, 0);
        sim.substrate.entities.insert(entity);
        sim.set_logic_order_for_test(vec![ORDINARY_DRIVE_HOST_ID]);
        sim
    }

    fn move_cadence_rules() -> RuleSet {
        RuleSet::from_ini(&IniFile::from_str("[General]\n\n[Move]\nRate=.016\n"))
            .expect("move cadence rules parse")
    }

    fn representative_foot_handler_rules() -> RuleSet {
        // `TEST` carries a long primary on purpose: `FootClass::Mission_Attack`
        // halves its cadence only for an infantry type with `CloseRange=` or a
        // primary reaching under 513 leptons, so the DEFAULT fixture type must
        // be one that does NOT qualify. `CLOSEINF` and `SHORTVEH` below are the
        // two qualifying shapes.
        RuleSet::from_ini(&IniFile::from_str(
            "[General]\n\n[Move]\nRate=.016\n\n[Attack]\nRate=.016\n\n             [Guard]\nRate=.016\n\n[Hunt]\nRate=.016\n\n             [VehicleTypes]\n0=TEST\n1=SHORTVEH\n\n             [InfantryTypes]\n0=CLOSEINF\n\n             [TEST]\nStrength=300\nPrimary=LONGGUN\n\n             [SHORTVEH]\nStrength=300\nPrimary=SHORTGUN\n\n             [CLOSEINF]\nStrength=100\nCloseRange=yes\nPrimary=LONGGUN\n\n             [LONGGUN]\nDamage=10\nROF=20\nRange=5\nWarhead=WH\n\n             [SHORTGUN]\nDamage=10\nROF=20\nRange=1\nWarhead=WH\n\n             [WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        ))
        .expect("representative Foot handler rules parse")
    }

    /// Build an Attack-committed attacker of `type_name` two cells from a
    /// target, i.e. squarely inside the close band, and return the sim.
    fn attack_band_fixture(seed: u64, type_name: &str, category: EntityCategory) -> Simulation {
        let mut sim = Simulation::with_seed(seed);
        let mut attacker = entity_of(1, category);
        attacker
            .attack_target
            .set_for_test(Some(TargetKind::Entity(2)));
        commit_due_mission(&mut attacker.mission, MissionType::Attack, 0);
        // 512 leptons from the target at (5, 5) — inside the 282..=768 band.
        attacker.position.set_rx_for_test(7);
        attacker.set_owner_for_test(sim.interner.intern("Americans"));
        attacker.set_type_ref_for_test(sim.interner.intern(type_name));
        sim.substrate.entities.insert(attacker);
        // On the map: TechnoClass::AI's 16-frame check drops a target in limbo
        // (GetFireError T12).
        let mut target = entity_of(2, EntityCategory::Unit);
        target.lifecycle.set_in_limbo_for_test(false);
        register_entity(&mut sim, target);
        // An enemy: a computer house drops an allied target every frame
        // (`0x006FA30C`).
        let soviet = sim.interner.intern("Soviet");
        sim.substrate.entities.change_owner(2, soviet);
        sim
    }

    /// The dispatch delay one Attack visit installs, and the jitter draw it
    /// took, for an attacker already inside the close band.
    fn attack_band_delay(seed: u64, type_name: &str, category: EntityCategory) -> (i32, i32) {
        let rules = representative_foot_handler_rules();
        let mut sim = attack_band_fixture(seed, type_name, category);
        let mut expected_rng = sim.clone_scenario_rng();
        let jitter = expected_rng.next_range_u32_inclusive(0, 2) as i32;
        sim.object_ai_visit_one(1, Some(&rules), ObjectAiCtx::default());
        assert_eq!(
            sim.scenario_rng.logical_state(),
            expected_rng.logical_state(),
            "both cadence arms draw the same jitter; the gate must not add or skip a draw"
        );
        let delay = dispatch_timer_of(&sim, 1).delay();
        (delay, 14 + jitter)
    }

    #[test]
    fn move_handler_rearms_from_the_authoritative_object_ai_host() {
        let mut sim = ordinary_drive_host_sim(0xCAFE);
        let rules = move_cadence_rules();
        let mut expected_rng = sim.clone_scenario_rng();
        let jitter = expected_rng.next_range_u32_inclusive(0, 2) as i32;

        assert!(sim.object_ai_visit_one(
            ORDINARY_DRIVE_HOST_ID,
            Some(&rules),
            ObjectAiCtx::default(),
        ));

        let entity = sim
            .substrate
            .entities
            .get(ORDINARY_DRIVE_HOST_ID)
            .expect("fixture entity");
        assert_eq!(entity.mission.dispatch_timer().start_frame(), 0);
        assert_eq!(entity.mission.dispatch_timer().delay(), 14 + jitter);
        assert_eq!(
            sim.scenario_rng.logical_state(),
            expected_rng.logical_state()
        );

        // Native in-scenario load resets Scenario RNG; isolate mission-timer
        // persistence by comparing against that same post-load baseline.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let hash = sim.state_hash();
        let bytes = GameSnapshot::save(&sim, 0, 0, "move_cadence", 0);
        let restored = GameSnapshot::load(&bytes).expect("snapshot").sim;
        assert_eq!(restored.state_hash(), hash);
        assert_eq!(
            restored
                .substrate
                .entities
                .get(ORDINARY_DRIVE_HOST_ID)
                .expect("restored fixture entity")
                .mission
                .dispatch_timer(),
            entity.mission.dispatch_timer()
        );
    }

    #[test]
    fn move_handler_preserves_pending_timer_without_consuming_rng() {
        let mut sim = ordinary_drive_host_sim(0xCAFE);
        let rules = move_cadence_rules();
        let pending = MissionDispatchTimer::from_raw(0, 5);
        let fixture = mission_test_fixture(
            &sim.substrate
                .entities
                .get(ORDINARY_DRIVE_HOST_ID)
                .expect("fixture entity")
                .mission,
        );
        sim.substrate
            .entities
            .get_mut(ORDINARY_DRIVE_HOST_ID)
            .expect("fixture entity")
            .mission
            .apply_test_fixture(MissionTestFixture {
                dispatch_timer: pending,
                ..fixture
            });
        let before_rng = sim.scenario_rng.logical_state();

        sim.object_ai_visit_one(ORDINARY_DRIVE_HOST_ID, Some(&rules), ObjectAiCtx::default());

        assert_eq!(dispatch_timer_of(&sim, ORDINARY_DRIVE_HOST_ID), pending);
        assert_eq!(sim.scenario_rng.logical_state(), before_rng);
    }

    #[test]
    fn gsi_07_06_attack_cadence_halves_only_for_qualifying_types() {
        // `FootClass::Mission_Attack @ 0x004D4DC0` halves its return only when a
        // target is installed AND the type qualifies AND the distance is in the
        // close band. The type half is
        // `(What_Am_I() == 0xF && InfantryType->CloseRange) || primary.Range <
        // 0x201`, and it was missing entirely — every tank and rifleman ran the
        // halved cadence at 1.1-3 cells, doubling the Attack dispatch rate and
        // the scenario jitter it consumes through every close engagement.
        //
        // Every fixture sits at 512 leptons, so the band is satisfied and
        // the ONLY thing under test is the type gate.

        // A long-primary vehicle: the 71-of-93 stock majority. Full cadence.
        let (delay, full) = attack_band_delay(0xA772, "TEST", EntityCategory::Unit);
        assert_eq!(
            delay, full,
            "a 5-cell primary must not qualify; this is the case the old test pinned backwards"
        );

        // A long-primary infantryman is equally unqualified — being infantry is
        // not sufficient without `CloseRange=`.
        let (delay, full) = attack_band_delay(0xA771, "TEST", EntityCategory::Infantry);
        assert_eq!(delay, full, "infantry alone does not qualify");

        // A short primary qualifies whatever the category — `Range=1` is 256
        // leptons, under the 0x201 threshold.
        let (delay, full) = attack_band_delay(0xA773, "SHORTVEH", EntityCategory::Unit);
        assert_eq!(delay, full / 2, "a sub-513-lepton primary qualifies");

        // `CloseRange=` qualifies an INFANTRY type even with a long primary.
        let (delay, full) = attack_band_delay(0xA774, "CLOSEINF", EntityCategory::Infantry);
        assert_eq!(delay, full / 2, "CloseRange= qualifies an infantry type");

        // `What_Am_I() == 0xF` is InfantryClass, so a VEHICLE carrying
        // `CloseRange=` does not take the short path in native. The key is
        // authored on three stock infantry types only, so this pins the gate's
        // shape rather than a stock case.
        let (delay, full) = attack_band_delay(0xA775, "CLOSEINF", EntityCategory::Unit);
        assert_eq!(
            delay, full,
            "CloseRange= on a vehicle must not halve the cadence — the native test is on the class"
        );
    }

    /// The idle selector's whole decision table, driven directly so the arms the
    /// Attack handler cannot reach are still pinned.
    #[test]
    fn enter_idle_mode_selector_matches_the_leaf_decision_table() {
        let rules = representative_foot_handler_rules();
        let base = MissionHandlerInput {
            category: EntityCategory::Infantry,
            mission: Some(MissionType::Attack),
            depot_dock_state: false,
            refinery_dock_miner: false,
            entry_adapter: false,
            moving_or_queued: false,
            has_attack_target: false,
            has_destination: false,
            effective_mission: Some(MissionType::Attack),
            infantry_deployed_do_type: false,
            infantry_deploy_fire_stance: false,
        };

        assert_eq!(
            foot_enter_idle_mode_queue(&rules, base),
            Some(MissionType::Guard),
            "no destination: Guard"
        );
        assert_eq!(
            foot_enter_idle_mode_queue(
                &rules,
                MissionHandlerInput {
                    has_destination: true,
                    ..base
                }
            ),
            Some(MissionType::Move),
            "a destination is installed: Move"
        );
        for already_idle in [MissionType::Guard, MissionType::AreaGuard] {
            assert_eq!(
                foot_enter_idle_mode_queue(
                    &rules,
                    MissionHandlerInput {
                        mission: Some(MissionType::Hunt),
                        effective_mission: Some(already_idle),
                        ..base
                    }
                ),
                None,
                "{already_idle:?} is already idle, so nothing is assigned"
            );
        }
        assert_eq!(
            foot_enter_idle_mode_queue(
                &rules,
                MissionHandlerInput {
                    mission: Some(MissionType::Patrol),
                    effective_mission: Some(MissionType::Patrol),
                    has_destination: true,
                    ..base
                }
            ),
            None,
            "the committed-selector tail gate blocks the assign"
        );
        for blocked in [MissionType::Unload, MissionType::Eaten] {
            assert_eq!(
                foot_enter_idle_mode_queue(
                    &rules,
                    MissionHandlerInput {
                        category: EntityCategory::Unit,
                        mission: Some(blocked),
                        effective_mission: Some(blocked),
                        has_destination: true,
                        ..base
                    }
                ),
                None,
                "the Unit tail gate also excludes {blocked:?}"
            );
            assert_eq!(
                foot_enter_idle_mode_queue(
                    &rules,
                    MissionHandlerInput {
                        mission: Some(blocked),
                        effective_mission: Some(blocked),
                        has_destination: true,
                        ..base
                    }
                ),
                Some(MissionType::Move),
                "but the Infantry tail gate does not"
            );
        }
    }

    /// Infantry51CC97/51CCA9 calls GetMissionControl5B3A00, which indexes
    /// committed Mission+AC; effective mission only admits that lookup.
    /// Neither frozen key is present in stock `[Attack]`.
    #[test]
    fn enter_idle_mode_selector_honours_frozen_control_entries() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[General]\n\n[Attack]\nRate=.016\n\n[Hunt]\nRate=.016\nParalyzed=yes\n\n[Sticky]\nRate=.016\nZombie=yes\n",
        ))
        .expect("frozen control rules parse");
        let base = MissionHandlerInput {
            category: EntityCategory::Infantry,
            mission: Some(MissionType::Attack),
            depot_dock_state: false,
            refinery_dock_miner: false,
            entry_adapter: false,
            moving_or_queued: false,
            has_attack_target: false,
            has_destination: false,
            effective_mission: Some(MissionType::Attack),
            infantry_deployed_do_type: false,
            infantry_deploy_fire_stance: false,
        };

        for frozen in [MissionType::Hunt, MissionType::Sticky] {
            assert_eq!(
                foot_enter_idle_mode_queue(
                    &rules,
                    MissionHandlerInput {
                        mission: Some(frozen),
                        effective_mission: Some(frozen),
                        ..base
                    }
                ),
                None,
                "committed {frozen:?} carries a frozen control entry"
            );
            assert_eq!(
                foot_enter_idle_mode_queue(
                    &rules,
                    MissionHandlerInput {
                        effective_mission: Some(frozen),
                        ..base
                    }
                ),
                Some(MissionType::Guard),
                "effective {frozen:?} does not replace committed Attack's control entry"
            );
        }
        assert_eq!(
            foot_enter_idle_mode_queue(&rules, base),
            Some(MissionType::Guard),
            "stock [Attack] carries neither key"
        );
    }

    /// `TechnoClass::AI_Update @ 0x006FA472..0x006FA4CB`: an attack dog
    /// (`Natural=yes`) holding a Brute (`Unnatural=yes`) is refused (T14,
    /// ILLEGAL). Its own fire routine keeps the target; the sixteenth-frame
    /// check lets go of it. A dog without `Natural=` keeps it, unless the
    /// Brute is cloaked where its house has no sensor (T17, CANT).
    #[test]
    fn a_natural_attacker_lets_go_of_an_unnatural_target_on_the_sixteenth_frame() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[General]\n\n[Attack]\nRate=.016\n\n[Guard]\nRate=.016\n\n\
             [VehicleTypes]\n\n[InfantryTypes]\n0=DOG\n1=PUP\n2=BRUTE\n\n\
             [DOG]\nStrength=100\nNatural=yes\nPrimary=Jaw\n\n\
             [PUP]\nStrength=100\nPrimary=Jaw\n\n\
             [BRUTE]\nStrength=300\nUnnatural=yes\n\n\
             [Jaw]\nDamage=100\nROF=20\nRange=2\nWarhead=WH\n\n\
             [WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        ))
        .expect("dog rules");
        let held = |attacker: &str, frame: u32, cloaked: bool| {
            let mut sim = Simulation::with_seed(0x6FA4);
            sim.session.set_binary_frame_for_test(frame);
            for (id, type_name, owner, rx) in
                [(1, attacker, "Russians", 5), (2, "BRUTE", "YuriCountry", 6)]
            {
                let mut entity = GameEntity::test_default(id, type_name, owner, rx, 5);
                entity.set_category_for_test(EntityCategory::Infantry);
                entity.mission_leaf =
                    MissionLeafState::for_entity_category(EntityCategory::Infantry);
                entity.lifecycle.set_in_limbo_for_test(false);
                entity.set_owner_for_test(sim.interner.intern(owner));
                entity.set_type_ref_for_test(sim.interner.intern(type_name));
                sim.substrate.entities.insert(entity);
            }
            if cloaked {
                let mut cloak = crate::sim::cloak_disguise::CloakRuntime::new(0);
                cloak.set_state_for_test(2);
                sim.substrate.entities.get_mut(2).unwrap().cloak = Some(cloak);
            }
            let dog = sim.substrate.entities.get_mut(1).unwrap();
            dog.attack_target.set_for_test(Some(TargetKind::Entity(2)));
            commit_due_mission(&mut dog.mission, MissionType::Attack, frame);
            sim.object_ai_visit_one(1, Some(&rules), ObjectAiCtx::default());
            target_of(&sim, 1).is_some()
        };
        assert!(
            held("DOG", 15, false),
            "the fire routine keeps an illegal target"
        );
        assert!(!held("DOG", 16, false), "the sixteenth frame drops it");
        assert!(held("PUP", 16, false), "a dog that is not Natural keeps it");
        assert!(held("PUP", 15, true), "a cloaked target is kept until then");
        assert!(!held("PUP", 16, true), "and dropped on CANT");
    }

    /// The heads of `FootClass::Mission_Guard` (`0x004D5076..0x004D50CD`)
    /// and `Mission_AreaGuard` (`0x004D6ACC`): with `+0x68F` set the Bio
    /// Reactor seek runs and the handler returns the flat `ftol(Rate × 900)`
    /// (both at the constructed 0.016 here) with no draw, whatever the seek
    /// found; a house with no absorber clears the latch, so the next visit
    /// takes the body.
    #[test]
    fn guard_heads_run_the_bio_reactor_seek_while_its_latch_holds() {
        for mission in [MissionType::Guard, MissionType::AreaGuard] {
            let mut sim = Simulation::with_seed(0x6A2D);
            let rules = representative_foot_handler_rules();
            let mut infantry = entity_of(1, EntityCategory::Infantry);
            infantry.set_owner_for_test(sim.interner.intern("Americans"));
            infantry.set_type_ref_for_test(sim.interner.intern("TEST"));
            crate::sim::building_entry::set_should_enter_absorber_for_test(&mut infantry, true);
            commit_due_mission(&mut infantry.mission, mission, 0);
            sim.substrate.entities.insert(infantry);
            let before_rng = sim.scenario_rng.logical_state();

            sim.object_ai_visit_one(1, Some(&rules), ObjectAiCtx::default());

            let infantry = sim.substrate.entities.get(1).unwrap();
            assert_eq!(
                infantry.mission.dispatch_timer(),
                MissionDispatchTimer::from_raw(0, 14),
                "{mission:?}"
            );
            assert_eq!(sim.scenario_rng.logical_state(), before_rng, "{mission:?}");
            assert!(
                !infantry.building_entry.should_enter_absorber(),
                "{mission:?}"
            );
            assert_eq!(infantry.mission.current(), MissionId::from_known(mission));
        }
    }

    /// `FootClass::Mission_AreaGuard`'s head (`0x004D6AAB..0x004D6ACB`): an
    /// object in a tank bunker (`+0x2E4`) queues Guard and returns 1 with no
    /// draw. Mission_Guard has no such head: it takes its body and draws.
    #[test]
    fn area_guard_head_returns_a_bunkered_object_to_guard() {
        for mission in [MissionType::AreaGuard, MissionType::Guard] {
            let mut sim = Simulation::with_seed(0x6A2E);
            let rules = representative_foot_handler_rules();
            let mut unit = entity_of(1, EntityCategory::Unit);
            unit.bunker
                .set_bunker_link_for_test(BunkerLink::Installed(99));
            // Queue_Mission's immediate Commence reads the locomotor.
            unit.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
            commit_due_mission(&mut unit.mission, mission, 0);
            register_entity(&mut sim, unit);
            let before_rng = sim.scenario_rng.logical_state();

            sim.object_ai_visit_one(1, Some(&rules), ObjectAiCtx::default());

            let unit = sim.substrate.entities.get(1).unwrap();
            let guard = MissionId::from_known(MissionType::Guard);
            if mission == MissionType::AreaGuard {
                assert_eq!(
                    unit.mission.dispatch_timer(),
                    MissionDispatchTimer::from_raw(0, 1)
                );
                assert_eq!(sim.scenario_rng.logical_state(), before_rng);
                assert_eq!(unit.mission.current(), guard);
            } else {
                assert_ne!(sim.scenario_rng.logical_state(), before_rng);
            }
        }
    }

    /// `FootClass::Mission_Guard 0x004D52A9..0x004D52F4`: while the object's
    /// rearm timer runs, Guard and Sticky return its remaining frames and draw
    /// nothing; once it has run out, the handler draws its jitter.
    #[test]
    fn guard_handler_waits_out_the_rearm_without_a_draw() {
        for (category, mission) in [
            (EntityCategory::Unit, MissionType::Guard),
            (EntityCategory::Infantry, MissionType::Guard),
            (EntityCategory::Unit, MissionType::Sticky),
        ] {
            // Armed 10 frames ago for 30 (20 left), or 30 frames ago (spent).
            for (armed_at, delay) in [(-10, Some(20)), (-30, None)] {
                let mut sim = Simulation::with_seed(0x6A30);
                let rules = representative_foot_handler_rules();
                let mut object = entity_of(1, category);
                object
                    .rearm_timer
                    .set_for_test(crate::sim::timer::CdTimer::started(armed_at, 30));
                commit_due_mission(&mut object.mission, mission, 0);
                register_entity(&mut sim, object);
                let before_rng = sim.scenario_rng.logical_state();

                sim.object_ai_visit_one(1, Some(&rules), ObjectAiCtx::default());

                let timer = dispatch_timer_of(&sim, 1);
                let case = format!("{category:?} {mission:?} armed at {armed_at}");
                match delay {
                    Some(delay) => {
                        assert_eq!(timer, MissionDispatchTimer::from_raw(0, delay), "{case}");
                        assert_eq!(sim.scenario_rng.logical_state(), before_rng, "{case}");
                    }
                    None => assert_ne!(sim.scenario_rng.logical_state(), before_rng, "{case}"),
                }
            }
        }
    }

    #[test]
    fn infantry_hunt_handler_uses_foot_jittered_cadence() {
        let mut sim = Simulation::with_seed(0x487A);
        let rules = representative_foot_handler_rules();
        let mut infantry = entity_of(1, EntityCategory::Infantry);
        // The Hunt body resolves the object type (for `StupidHunt=`) and runs
        // the scanner, so the fixture has to be interned in THIS sim.
        infantry.set_owner_for_test(sim.interner.intern("Americans"));
        infantry.set_type_ref_for_test(sim.interner.intern("TEST"));
        commit_due_mission(&mut infantry.mission, MissionType::Hunt, 0);
        sim.substrate.entities.insert(infantry);
        let mut expected_rng = sim.clone_scenario_rng();
        // Two draws, in this order. `TechnoClass::Retaliate_And_Scan @
        // 0x00709820` takes the first one unconditionally, before any branch
        // (`PUSH 0x2` / `PUSH 0x0` at `0x0070982C`/`0x0070983D`), for the scan
        // timer's re-arm; the handler tail at `0x004D55A9` takes the second for
        // the cadence jitter.
        let _scan_jitter = expected_rng.next_range_u32_inclusive(0, 2);
        let expected_delay = 14 + expected_rng.next_range_u32_inclusive(0, 2) as i32;

        sim.object_ai_visit_one(1, Some(&rules), ObjectAiCtx::default());

        assert_eq!(
            dispatch_timer_of(&sim, 1),
            MissionDispatchTimer::from_raw(0, expected_delay)
        );
        assert_eq!(
            sim.scenario_rng.logical_state(),
            expected_rng.logical_state()
        );
    }

    /// Rules for the Hunt body and the infantry deploy shim.
    ///
    /// `HUNTVEH` is an ordinary armed vehicle; `DUMBVEH` carries
    /// `StupidHunt=yes`; `DEPINF` carries `DeployFire=yes` with neither
    /// `ImmuneToRadiation=` nor `UndeployDelay=`, which is the GI / Guardian GI
    /// shape the shim's live arm selects.
    fn hunt_body_rules() -> RuleSet {
        RuleSet::from_ini(&IniFile::from_str(
            "[General]\nNormalTargetingDelay=27\nGuardAreaTargetingDelay=36\n\n\
             [Attack]\nRate=.016\n\n[Guard]\nRate=.016\n\n[AreaGuard]\nRate=.016\n\n\
             [Hunt]\nRate=.016\n\n[Move]\nRate=.016\n\n\
             [VehicleTypes]\n0=HUNTVEH\n1=DUMBVEH\n\n[InfantryTypes]\n0=DEPINF\n\n\
             [HUNTVEH]\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
             Strength=300\nArmor=heavy\nSpeed=6\nSight=10\nPrimary=SHORTGUN\n\n\
             [DUMBVEH]\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
             Strength=300\nArmor=heavy\nSpeed=6\nSight=10\nPrimary=SHORTGUN\nStupidHunt=yes\n\n\
             [DEPINF]\nLocomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
             Strength=125\nArmor=none\nSpeed=4\nSight=10\nPrimary=SHORTGUN\nDeployFire=yes\n\n\
             [SHORTGUN]\nDamage=10\nROF=20\nRange=4\nWarhead=WH\n\n\
             [WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        ))
        .expect("Hunt body rules parse")
    }

    /// One armed hunter of `hunter_type` at (20, 20) and one hostile at
    /// (20 + gap, 20), both spawned through the map path so ownership, fog and
    /// playfield membership are the production ones.
    fn hunt_pair(seed: u64, hunter_type: &str, gap: u16, mission: MissionType) -> Simulation {
        let rules = hunt_body_rules();
        let mut sim = Simulation::with_seed(seed);
        sim.spawn_from_map(
            &[
                passive_map_entity("Americans", hunter_type, 20, 20, EntityCategory::Unit),
                passive_map_entity("Soviet", "HUNTVEH", 20 + gap, 20, EntityCategory::Unit),
            ],
            Some(&rules),
        );
        let hunter = sim
            .substrate
            .entities
            .get_mut(1)
            .expect("hunter spawned first");
        commit_due_mission(&mut hunter.mission, mission, 0);
        sim
    }

    /// The headline of GSI-07.20: a hunting object acquires past its own weapon
    /// reach, and the target it installs is NOT flagged passively-acquired, so
    /// nothing treats it as a scan pick-up (the crush arms, the off-mission
    /// clear). Native:
    /// `FootClass::Mission_Hunt @ 0x004D5373` scans with threat mask `0`, and
    /// `TechnoClass::Greatest_Threat @ 0x006F8FE0` (`TEST AL,0x3 ; JZ`) jumps
    /// past the whole radius block for that mask; `Retaliate_And_Scan @
    /// 0x00709820` installs the result through `Assign_Target` at `0x00709960`
    /// and never writes the passively-acquired byte `+0x50C`.
    #[test]
    fn gsi_07_20_hunt_acquires_past_weapon_range_and_leaves_the_target_pursuable() {
        let rules = hunt_body_rules();
        // 8 cells apart against `SHORTGUN` Range=4 — comfortably outside the
        // weapon reach that bounds a Guard-mission scan, comfortably inside
        // Sight=10 so the candidate is not shrouded.
        let mut sim = hunt_pair(0x40_0720, "HUNTVEH", 8, MissionType::Hunt);
        let grid = crate::sim::pathfinding::PathGrid::new(64, 64);
        // Record the state at the moment of acquisition: the hunter starts
        // closing as soon as it has a target, and the point of the test is how
        // far away it was when it found one.
        let mut acquired = None;
        for _ in 0..40 {
            let _ = sim.advance_tick(&[], Some(&rules), Some(&grid), None, 67);
            let hunter = sim.substrate.entities.get(1).expect("hunter present");
            if hunter.attack_target.target().is_some() {
                acquired = Some((
                    28u16.saturating_sub(hunter.position.rx()),
                    hunter.passive_scan.acquired(),
                ));
                break;
            }
        }
        let (gap_cells, passive) = acquired.expect(
            "a hunting object scans with no radius cutoff and must find the hostile 8 cells away",
        );
        assert!(
            gap_cells > 4,
            "acquisition must happen outside SHORTGUN's 4-cell reach: {gap_cells}"
        );
        assert!(
            !passive,
            "Retaliate_And_Scan never writes +0x50C, so the Hunt target must stay pursuable"
        );
    }

    /// `StupidHunt=` (`TechnoTypeClass+0x6D4`, store `0x00714C80`, key
    /// `0x008438A4`) is the FIRST test in `FootClass::Mission_Hunt`
    /// (`0x004D535F`): such a type never reaches the scanner at all. It
    /// therefore acquires nothing AND takes only the tail's single draw.
    #[test]
    fn gsi_07_20_stupid_hunt_type_never_scans_and_draws_once() {
        let rules = hunt_body_rules();
        let mut sim = hunt_pair(0x40_0722, "DUMBVEH", 8, MissionType::Hunt);
        let mut expected_rng = sim.clone_scenario_rng();
        let expected_delay = 14 + expected_rng.next_range_u32_inclusive(0, 2) as i32;

        sim.object_ai_visit_one(1, Some(&rules), ObjectAiCtx::default());

        let hunter = sim.substrate.entities.get(1).expect("hunter present");
        assert!(
            hunter.attack_target.target().is_none(),
            "StupidHunt skips the scan entirely"
        );
        assert_eq!(hunter.mission.dispatch_timer().delay(), expected_delay);
        assert_eq!(
            sim.scenario_rng.logical_state(),
            expected_rng.logical_state(),
            "the StupidHunt path draws only the tail jitter"
        );
    }

    /// The cadence band is `[282, 768]` on the **approximated** distance
    /// `ftol(Sqrt_Approx(dx*dx + dy*dy))`, not on the squared one:
    /// `disassemble_bytes 0x004D4EF0..0x004D4FB1` — `CMP EAX,0x300 ; JG skip`
    /// after the `Sqrt_Approx`/`ftol` pair.
    ///
    /// This fixture sits at dx = 768, dy = 30, i.e. `d² = 590724`. The old
    /// squared predicate `d² <= 768² = 589824` rejected it; native truncates
    /// the root to 768 and admits it, halving the cadence.
    #[test]
    fn gsi_07_06_cadence_band_admits_a_distance_above_the_squared_bound() {
        let rules = representative_foot_handler_rules();
        let mut sim = Simulation::with_seed(0x40_0706);
        let mut attacker = entity_of(1, EntityCategory::Unit);
        attacker
            .attack_target
            .set_for_test(Some(TargetKind::Entity(2)));
        commit_due_mission(&mut attacker.mission, MissionType::Attack, 0);
        // Target sits at (5, 5); 3 cells east is exactly 768 leptons, and the
        // 30-lepton north/south offset pushes d² past 768² without pushing the
        // truncated root past 768.
        attacker.position.set_rx_for_test(8);
        attacker.position.set_sub_y_for_test(
            attacker.position.sub_y() + crate::util::fixed_math::SimFixed::from_num(30),
        );
        attacker.set_owner_for_test(sim.interner.intern("Americans"));
        attacker.set_type_ref_for_test(sim.interner.intern("SHORTVEH"));
        sim.substrate.entities.insert(attacker);
        let mut target = entity_of(2, EntityCategory::Unit);
        target.lifecycle.set_in_limbo_for_test(false);
        register_entity(&mut sim, target);
        let soviet = sim.interner.intern("Soviet");
        sim.substrate.entities.change_owner(2, soviet);

        let mut expected_rng = sim.clone_scenario_rng();
        let jitter = expected_rng.next_range_u32_inclusive(0, 2) as i32;
        sim.object_ai_visit_one(1, Some(&rules), ObjectAiCtx::default());

        assert_eq!(
            dispatch_timer_of(&sim, 1).delay(),
            (14 + jitter) / 2,
            "trunc(Sqrt_Approx(590724)) is 768, which the band admits"
        );
    }

    /// GSI-07.16 — a deployed `DeployFire=` infantryman on Area Guard runs the
    /// leaf shim `FUN_00521320`, not `FootClass::Mission_AreaGuard`. The shim
    /// re-acquires in place and returns `Rate + RandomRanged(0, 2)`; the Foot
    /// body would run `Retaliate_And_Scan` (one draw) and then return
    /// `Rate + RandomRanged(1, 5)` (a second). One draw versus two is the
    /// discriminator.
    #[test]
    fn gsi_07_16_deployed_deploy_fire_infantry_takes_the_shim_on_area_guard() {
        let rules = hunt_body_rules();
        let mut sim = Simulation::with_seed(0x40_0716);
        let mut man = entity_of(1, EntityCategory::Infantry);
        man.set_owner_for_test(sim.interner.intern("Americans"));
        man.set_type_ref_for_test(sim.interner.intern("DEPINF"));
        man.mission_leaf.set_infantry_doing_verified(28).unwrap();
        commit_due_mission(&mut man.mission, MissionType::AreaGuard, 0);
        sim.substrate.entities.insert(man);

        let mut expected_rng = sim.clone_scenario_rng();
        let expected_delay = 14 + expected_rng.next_range_u32_inclusive(0, 2) as i32;

        sim.object_ai_visit_one(1, Some(&rules), ObjectAiCtx::default());

        assert_eq!(
            dispatch_timer_of(&sim, 1).delay(),
            expected_delay,
            "the shim returns [AreaGuard] Rate + RandomRanged(0, 2)"
        );
        assert_eq!(
            sim.scenario_rng.logical_state(),
            expected_rng.logical_state(),
            "the shim never reaches the Foot body's scan or its (1, 5) jitter"
        );
    }

    // ===== The base (un-overridden) mission handler =====

    /// A mission whose slot no leaf class overrides re-arms with a flat 450
    /// frames and draws nothing. Without this arm the dispatch timer stayed at
    /// whatever Commence wrote — `{now, 0}`, i.e. permanently due — so the
    /// mission timer gated nothing for those objects.
    #[test]
    fn base_stub_missions_rearm_450_frames_without_touching_rng() {
        for (category, mission) in [
            (EntityCategory::Unit, Some(MissionType::Stop)),
            (EntityCategory::Unit, Some(MissionType::Selling)),
            (EntityCategory::Unit, Some(MissionType::Sleep)),
            (EntityCategory::Unit, Some(MissionType::Harmless)),
            (EntityCategory::Infantry, Some(MissionType::Stop)),
            // Repair is a base stub for Infantry only.
            (EntityCategory::Infantry, Some(MissionType::Repair)),
            // The dispatcher's bounds test on the mission id is unsigned, so
            // the idle sentinel takes the switch default — the same base
            // handler `Sleep(0)` uses — rather than being skipped.
            (EntityCategory::Unit, None),
        ] {
            let mut sim = Simulation::with_seed(0x5B2E);
            let rules = representative_foot_handler_rules();
            let mut unit = entity_of(1, category);
            update_mission_test_fixture(&mut unit.mission, |fixture| {
                fixture.current = mission.map_or(MissionId::NONE, MissionId::from_known);
                fixture.dispatch_timer = MissionDispatchTimer::at_frame(0);
            });
            register_entity(&mut sim, unit);
            let before_rng = sim.scenario_rng.logical_state();

            sim.object_ai_visit_one(1, Some(&rules), ObjectAiCtx::default());

            assert_eq!(
                dispatch_timer_of(&sim, 1),
                MissionDispatchTimer::from_raw(0, MISSION_STUB_FRAMES),
                "{category:?} on {mission:?} must re-arm the base handler's flat delay"
            );
            assert_eq!(
                sim.scenario_rng.logical_state(),
                before_rng,
                "{category:?} on {mission:?} must draw no RNG — the base stub touches nothing"
            );
        }
    }

    /// A vehicle that finishes a move order drops its shoot-at target and
    /// queues Guard — the arrival hook's ordinary-vehicle arm. Without this the
    /// unit stays on Move for the rest of the match.
    #[test]
    fn move_arrival_drops_the_vehicle_target_and_queues_guard() {
        let mut sim = Simulation::with_seed(0x7389);
        let rules = move_cadence_rules();
        let mut unit = entity_of(1, EntityCategory::Unit);
        unit.attack_target.set_for_test(Some(TargetKind::Entity(2)));
        unit.passive_scan
            .set_passively_acquired_target_for_test(true);
        unit.weapon_burst.complete_shot(2);
        commit_due_mission(&mut unit.mission, MissionType::Move, 0);
        register_entity(&mut sim, unit);
        let before_rng = sim.scenario_rng.logical_state();

        sim.object_ai_visit_one(1, Some(&rules), ObjectAiCtx::default());

        let entity = sim.substrate.entities.get(1).expect("fixture entity");
        assert_eq!(
            entity.mission.queued(),
            MissionId::from_known(MissionType::Guard)
        );
        assert!(
            entity.attack_target.target().is_none(),
            "Assign_Target(NULL) on arrival"
        );
        assert!(!entity.passive_scan.acquired());
        assert_eq!(entity.weapon_burst.index(), 0);
        assert_eq!(
            entity.mission.dispatch_timer(),
            MissionDispatchTimer::from_raw(0, 1),
            "the arrival branch still returns one frame"
        );
        // The arrival branch draws nothing: the jitter belongs to the still-
        // moving branch only.
        assert_eq!(sim.scenario_rng.logical_state(), before_rng);
    }

    /// Infantry's arrival selector is NOT the vehicle one: a live target queues
    /// Attack and the target is kept, where a vehicle would drop it and fall to
    /// Guard.
    #[test]
    fn move_arrival_promotes_an_infantry_target_to_attack() {
        let mut sim = Simulation::with_seed(0x51CB);
        let rules = move_cadence_rules();
        let mut man = entity_of(1, EntityCategory::Infantry);
        man.attack_target.set_for_test(Some(TargetKind::Entity(2)));
        commit_due_mission(&mut man.mission, MissionType::Move, 0);
        register_entity(&mut sim, man);

        sim.object_ai_visit_one(1, Some(&rules), ObjectAiCtx::default());

        let entity = sim.substrate.entities.get(1).expect("fixture entity");
        assert_eq!(
            entity.mission.queued(),
            MissionId::from_known(MissionType::Attack)
        );
        assert!(
            entity.attack_target.target().is_some(),
            "the infantry arm keeps the target it promotes"
        );
    }

    /// Area Guard has its own handler and its own cadence: `[Area Guard]
    /// Rate=.040` is 35 frames, and the jitter draw is `RandomRanged(1, 5)` —
    /// NOT the `(0, 2)` every other absorbed handler takes. The fixture type
    /// carries no weapon, so the can-acquire predicate fails and the scan (with
    /// its own separate draw) never runs.
    #[test]
    fn area_guard_rearms_at_its_own_rate_with_a_one_to_five_jitter() {
        let mut sim = Simulation::with_seed(0x4D6A);
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[General]\n\n[Guard]\nRate=.030\n\n[Area Guard]\nRate=.040\nAARate=.032\n",
        ))
        .expect("area guard cadence rules parse");
        let mut unit = entity_of(1, EntityCategory::Unit);
        commit_due_mission(&mut unit.mission, MissionType::AreaGuard, 0);
        register_entity(&mut sim, unit);
        let mut expected_rng = sim.clone_scenario_rng();
        let jitter = expected_rng.next_range_u32_inclusive(1, 5) as i32;
        assert!((1..=5).contains(&jitter));

        sim.object_ai_visit_one(1, Some(&rules), ObjectAiCtx::default());

        assert_eq!(
            dispatch_timer_of(&sim, 1),
            MissionDispatchTimer::from_raw(0, 35 + jitter),
            "[Area Guard] Rate=.040 is 35 frames; [Guard] Rate=.030 would be 26"
        );
        assert_eq!(
            sim.scenario_rng.logical_state(),
            expected_rng.logical_state(),
            "exactly one RandomRanged(1, 5) and nothing else"
        );
    }

    // ===== Host promotion (Ready→Commence at the per-object AI position) =====

    /// Minimal rules for host-promotion tests: one vehicle type "TEST" plus a
    /// weapons-factory building type "FACT".
    fn promotion_rules() -> RuleSet {
        let text = "[General]\n\
BuildSpeed=0.75\nMultipleFactory=0.7\nLowPowerPenaltyModifier=1.25\n\
MinLowPowerProductionSpeed=0.4\nMaxLowPowerProductionSpeed=0.85\n\n\
[InfantryTypes]\n[VehicleTypes]\n1=TEST\n[AircraftTypes]\n[BuildingTypes]\n1=FACT\n\n\
[TEST]\n\n[FACT]\nWeaponsFactory=yes\n";
        RuleSet::from_ini(&IniFile::from_str(text)).expect("promotion test rules parse")
    }

    /// Queue `mission` on `id` through the mission owner at frame 0.
    fn queue_mission(sim: &mut Simulation, id: u64, mission: MissionType) {
        sim.mission_queue_exact(
            id,
            MissionId::from_known(mission),
            0,
            0,
            &crate::sim::mission::authority::EntityReadyInputProvider,
        )
        .unwrap();
    }

    /// A `TEST` object of `category` at (5, 5), interned through the SIM's
    /// interner (survives verb + promotion paths that resolve the type name).
    fn insert_interned(sim: &mut Simulation, id: u64, category: EntityCategory) {
        let e = interned_entity(sim, id, "Americans", "TEST", category, (5, 5), 100);
        sim.substrate.entities.insert(e);
    }

    /// An interned Infantry carrying a Walk locomotor, so the readiness producer
    /// has a real family to map and the moving gate is live.
    fn insert_interned_walker(sim: &mut Simulation, id: u64, rx: u16, ry: u16) {
        let mut e = interned_entity(
            sim,
            id,
            "Americans",
            "TEST",
            EntityCategory::Infantry,
            (rx, ry),
            100,
        );
        e.locomotor = Some(
            crate::sim::movement::locomotor::LocomotorState::for_test_kind(
                crate::rules::locomotor_type::LocomotorKind::Walk,
            ),
        );
        e.mission_leaf = crate::sim::mission::leaf::MissionLeafState::infantry_raw_for_test(0, -1);
        sim.substrate.entities.insert(e);
        register_in_logic(sim, id);
    }

    /// The post-movement checkpoint walks the logic scheduler, not the entity
    /// store, so a fixture entity has to be a scheduler member or the walk skips
    /// it and every assertion passes vacuously.
    fn register_in_logic(sim: &mut Simulation, id: u64) {
        sim.substrate.logic.try_push(id).expect("logic slot");
        sim.substrate
            .entities
            .get_mut(id)
            .expect("fixture entity")
            .in_logic_vector
            .set_for_test(true);
    }

    /// Supply the retained Walk byte/head and shared Foot fraction read by
    /// native75AB40; a compatibility path alone cannot establish readiness.
    fn set_walking(sim: &mut Simulation, id: u64, walking: bool) {
        let entity = sim.substrate.entities.get_mut(id).expect("fixture entity");
        let head = walking.then(|| crate::sim::components::DriveCoord::cell(6, 5, 0));
        let loco = entity.locomotor.as_mut().unwrap();
        loco.set_step_head(head);
        loco.set_walk_destination(head);
        entity.foot_speed.set_speed_fraction(if walking {
            crate::util::fixed_math::SIM_ONE
        } else {
            crate::util::fixed_math::SIM_ZERO
        });
    }

    #[test]
    fn host_promotes_before_the_shared_native_mission_counter() {
        // Original Unit736473 Commence -> Foot/Techno ->6FA64E increments C4.
        // The executed MTNK first visit is pinned in this native projection;
        // Infantry51BC51 has the same instruction-established call ordering.
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/anytown_damage/mission_test_vectors.json",
        ))
        .unwrap();
        for category in [EntityCategory::Unit, EntityCategory::Infantry] {
            let rules = promotion_rules();
            let mut sim = Simulation::new();
            if category == EntityCategory::Unit {
                insert_interned(&mut sim, 1, EntityCategory::Unit);
            } else {
                insert_interned_walker(&mut sim, 1, 5, 5);
            }
            queue_mission(&mut sim, 1, MissionType::Move);
            let e = sim.substrate.entities.get(1).unwrap();
            assert_eq!(e.mission.current(), MissionId::NONE);
            assert_eq!(e.mission.queued().known(), Some(MissionType::Move));

            sim.session.set_binary_frame_for_test(7);
            sim.object_ai_visit_one(1, Some(&rules), ObjectAiCtx::default());

            let e = sim.substrate.entities.get(1).unwrap();
            assert_eq!(e.mission.current().known(), Some(MissionType::Move));
            // The Move handler may queue Guard after the prefix when no
            // destination exists; promotion and the counter are already final.
            assert_eq!(e.mission.mission_start_frame(), 7);
            assert_eq!(
                i64::from(e.mission.ai_counter()),
                native["seed0"]["first_tick"]["mission_visit_count"]
                    .as_i64()
                    .unwrap(),
                "{category:?} Commence must precede the Techno counter"
            );
        }
    }

    /// An infantry that is still walking when the pre-movement checkpoint runs,
    /// and has come to rest by the post-movement one, must commence in the SAME
    /// tick — not the next.
    ///
    /// This is the whole point of the second checkpoint. gamemd's
    /// `InfantryClass::AI` gates either side of the object's own locomotion, so
    /// stopping mid-tick makes the unit eligible before the tick ends. With only
    /// the pre-movement gate the promotion slipped a tick, and that lag compounds
    /// through every chained handoff.
    ///
    /// Infantry rather than Unit deliberately: the Unit moving-defer branch is
    /// inert in production today because `signed_height` has no producer and the
    /// resulting error maps to permissive-ready, so a Unit fixture would pass for
    /// the wrong reason. Infantry readiness reads no height.
    #[test]
    fn post_movement_checkpoint_commences_a_walker_that_stopped_this_tick() {
        let rules = promotion_rules();
        let mut sim = Simulation::new();
        insert_interned_walker(&mut sim, 1, 5, 5);

        queue_mission(&mut sim, 1, MissionType::Move);

        // Still walking: the pre-movement checkpoint must NOT commence it.
        set_walking(&mut sim, 1, true);
        sim.object_ai_post_movement_promote(Some(&rules));
        let e = sim.substrate.entities.get(1).unwrap();
        assert_eq!(
            e.mission.queued().known(),
            Some(MissionType::Move),
            "a walking infantry must still be deferred: the moving gate is what \
             this checkpoint exists to re-evaluate, so if it commences here the \
             test proves nothing"
        );
        assert_eq!(e.mission.current(), MissionId::NONE);

        // Movement ended during the tick's movement phases.
        set_walking(&mut sim, 1, false);
        sim.object_ai_post_movement_promote(Some(&rules));

        let e = sim.substrate.entities.get(1).unwrap();
        assert_eq!(
            e.mission.current().known(),
            Some(MissionType::Move),
            "coming to rest during the movement phases must let the second \
             checkpoint commence the queued mission in the same tick"
        );
        assert_eq!(e.mission.queued(), MissionId::NONE);
    }

    /// A ready Aircraft stays queued through the shared Foot/mission visit and
    /// promotes only at AircraftClass::AI's post-Foot checkpoint.
    #[test]
    fn gsi_05_06_aircraft_ready_queue_promotes_only_post_movement() {
        let rules = promotion_rules();
        let mut sim = Simulation::new();
        insert_interned(&mut sim, 1, EntityCategory::Aircraft);
        queue_mission(&mut sim, 1, MissionType::Move);

        sim.object_ai_visit_one(1, Some(&rules), ObjectAiCtx::default());

        let aircraft = sim.substrate.entities.get(1).expect("aircraft present");
        assert_eq!(aircraft.mission.current(), MissionId::NONE);
        assert_eq!(aircraft.mission.queued().known(), Some(MissionType::Move));
        let counter_before_post = aircraft.mission.ai_counter();
        assert_eq!(counter_before_post, 1, "Foot mission work still ran");

        sim.object_ai_post_movement_promote_one(1, Some(&rules));

        let aircraft = sim.substrate.entities.get(1).expect("aircraft present");
        assert_eq!(aircraft.mission.current().known(), Some(MissionType::Move));
        assert_eq!(aircraft.mission.queued(), MissionId::NONE);
        assert_eq!(
            aircraft.mission.ai_counter(),
            0,
            "Commence resets the counter and the post gate must not increment it again"
        );
    }

    #[test]
    fn host_promotion_holds_on_weapons_factory_contact() {
        // A Unit whose Radio slot 0 is a weapons-factory Building is NOT ready
        // for a non-Move/Enter queued mission (the exact slot-0 hold).
        let rules = promotion_rules();
        let mut sim = Simulation::new();
        insert_interned(&mut sim, 1, EntityCategory::Unit);
        let owner = sim.interner.intern("Americans");
        let fact_type = sim.interner.intern("FACT");
        let fact = GameEntity::new_at_frame_zero_for_test(
            2,
            6,
            6,
            0,
            0,
            owner,
            crate::sim::combat::Health::new(100),
            fact_type,
            EntityCategory::Structure,
            0,
            5,
            false,
        );
        sim.substrate.entities.insert(fact);
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .radio_contacts
            .insert(2);

        queue_mission(&mut sim, 1, MissionType::Guard);
        sim.mission_host_promote(1, 7, &rules);
        let e = sim.substrate.entities.get(1).unwrap();
        assert_eq!(
            e.mission.current(),
            MissionId::NONE,
            "weapons-factory slot-0 contact holds a queued Guard"
        );
        assert_eq!(e.mission.queued().known(), Some(MissionType::Guard));

        // A queued Move is exempt from the slot-0 hold and promotes.
        queue_mission(&mut sim, 1, MissionType::Move);
        sim.mission_host_promote(1, 9, &rules);
        let e = sim.substrate.entities.get(1).unwrap();
        assert_eq!(e.mission.current().known(), Some(MissionType::Move));
    }

    // ===== In-loop dispatch authority =====

    /// A scoped moving-unit fixture interned through the simulation's interner,
    /// so it survives a real `advance_tick` (test_intern ids don't exist in
    /// `sim.interner`, and tick-path resolves would panic).
    fn insert_s2_scoped_move_unit(sim: &mut Simulation, id: u64, rx: u16, ry: u16) {
        let mut e = interned_entity(
            sim,
            id,
            "Americans",
            "TEST",
            EntityCategory::Unit,
            (rx, ry),
            100,
        );
        // Out of limbo, where `FootClass::AI` admits a Process (`0x004DA86E`).
        // The ground corridor's Walk Process takes its idle tail (Walk75BCE3)
        // on the first Process.
        e.lifecycle.set_in_limbo_for_test(false);
        e.locomotor = Some(
            crate::sim::movement::locomotor::LocomotorState::for_test_kind(
                crate::rules::locomotor_type::LocomotorKind::Walk,
            ),
        );
        sim.substrate.entities.insert(e);
    }

    /// `FootClass::AI` calls no locomotor Process while the object falls
    /// (ObjectClass `+0x8D`, tested at `0x004DA826`): the idle Walk's tail
    /// (Walk75BCE3), which stops a positive Foot speed, waits until the fall
    /// ends.
    #[test]
    fn a_falling_mover_gets_no_process() {
        use crate::util::fixed_math::{SIM_ONE, SIM_ZERO};
        let mut sim = Simulation::new();
        insert_s2_scoped_move_unit(&mut sim, 1, 5, 5);
        let mover = sim.substrate.entities.get_mut(1).unwrap();
        mover.set_falling_down_for_test(true);
        mover.foot_speed.set_speed_fraction(SIM_ONE);
        sim.set_logic_order_for_test(vec![1]);

        let _ = sim.advance_tick(&[], None, None, None, 67);
        let mover = sim.substrate.entities.get_mut(1).unwrap();
        assert_eq!(
            mover.foot_speed.applied_fraction(),
            SIM_ONE,
            "no Process while falling"
        );

        mover.set_falling_down_for_test(false);
        let _ = sim.advance_tick(&[], None, None, None, 67);
        assert_eq!(
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .foot_speed
                .applied_fraction(),
            SIM_ZERO,
            "the landed mover's Process runs"
        );
    }

    /// S2: exactly one ai_counter increment per unit-tick — in-loop for a
    /// dispatched mover, tail for an idle (never-collected) unit. Double or
    /// zero count is permanent lockstep drift.
    #[test]
    fn s2_ai_counter_increments_exactly_once() {
        let mut sim = Simulation::new();
        insert_s2_scoped_move_unit(&mut sim, 1, 5, 5); // dispatched on tick 1
        insert_s2_scoped_move_unit(&mut sim, 2, 8, 8);
        // never collected; never scoped
        sim.set_logic_order_for_test(vec![1, 2]);

        let _ = sim.advance_tick(&[], None, None, None, 67);
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().mission.ai_counter(),
            1
        );
        assert_eq!(
            sim.substrate.entities.get(2).unwrap().mission.ai_counter(),
            1
        );
        let _ = sim.advance_tick(&[], None, None, None, 67);
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().mission.ai_counter(),
            2
        );
        assert_eq!(
            sim.substrate.entities.get(2).unwrap().mission.ai_counter(),
            2
        );
    }

    /// Load trusts serialized MissionCom: a save carrying a verb-committed
    /// mission that no legacy machine could re-derive (Move current, no
    /// destination) must restore an IDENTICAL state hash and value.
    /// Guards the deleted post-load re-derive against reintroduction.
    #[test]
    fn save_load_round_trip_trusts_serialized_mission() {
        use crate::sim::snapshot::GameSnapshot;
        let mut sim = Simulation::new();
        insert_s2_scoped_move_unit(&mut sim, 1, 5, 5);
        update_mission_test_fixture(
            &mut sim.substrate.entities.get_mut(1).unwrap().mission,
            |fixture| {
                fixture.current = MissionId::from_known(MissionType::Move);
                fixture.ai_counter = 9;
            },
        );
        sim.set_logic_order_for_test(vec![1]);
        // Native in-scenario load resets Scenario RNG; isolate MissionCom
        // persistence by comparing against that same post-load baseline.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let hash_before = sim.state_hash();

        let bytes = GameSnapshot::save(&sim, 0, 0, "test_map", 0);
        let mut restored = GameSnapshot::load(&bytes).expect("load").sim;
        restored.rebuild_logic_membership(); // the real post-deserialize step
        assert_eq!(
            restored.state_hash(),
            hash_before,
            "load must trust serialized MissionCom"
        );
        assert_eq!(
            restored
                .substrate
                .entities
                .get(1)
                .unwrap()
                .mission
                .current()
                .known(),
            Some(MissionType::Move),
        );
    }

    // ===== Slice S4b — AI_Update damage-Spark scenario_rng consumption =====

    /// Two Spark systems, each `Lifetime=5`; the list-pick consumes a draw
    /// before the selected real system is constructed.
    const TWO_SPARK_SYSTEMS: &str = "[ParticleSystems]\n\
1=SparkA\n2=SparkB\n\n[SparkA]\nBehavesLike=Spark\nLifetime=5\n\n[SparkB]\nBehavesLike=Spark\nLifetime=5\n";

    /// One Spark (`Lifetime=5`) plus one Smoke — exercises the Spark filter and the
    /// single-Spark list-pick (count==1 → no draw, matching `n(0,0)`).
    const ONE_SPARK_ONE_SMOKE_SYSTEMS: &str = "[ParticleSystems]\n\
1=SparkA\n2=SmokeA\n\n[SparkA]\nBehavesLike=Spark\nLifetime=5\n\n[SmokeA]\nBehavesLike=Smoke\n";

    /// A Smoke-only damage particle system — no Spark, so the inner gate never
    /// passes (zero draws even below ConditionRed).
    const SMOKE_ONLY_SYSTEMS: &str = "[ParticleSystems]\n1=SmokeA\n\n[SmokeA]\nBehavesLike=Smoke\n";

    /// Minimal RuleSet with one `Cyborg=yes` infantry "CYB" (so `emits_damage_spark`
    /// is true), `DamageParticleSystems=dps`, the named particle `systems`, and the
    /// two damage-Spark probabilities. prob "1.0" → always-succeed threshold; "0.0"
    /// → always-fail — so the draw outcome is deterministic regardless of the seed's
    /// actual roll value.
    fn cyborg_rules(red_prob: &str, yellow_prob: &str, dps: &str, systems: &str) -> RuleSet {
        use crate::rules::ini_parser::IniFile;
        let text = format!(
            "[General]\n\
BuildSpeed=0.75\nMultipleFactory=0.7\nLowPowerPenaltyModifier=1.25\n\
MinLowPowerProductionSpeed=0.4\nMaxLowPowerProductionSpeed=0.85\n\
ConditionRedSparkingProbability={red_prob}\nConditionYellowSparkingProbability={yellow_prob}\n\n\
[InfantryTypes]\n1=CYB\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\n\
[CYB]\nStrength=100\nCyborg=yes\nDamageParticleSystems={dps}\n\n{systems}\n"
        );
        RuleSet::from_ini(&IniFile::from_str(&text)).expect("cyborg test rules parse")
    }

    /// Insert a unit whose type resolves to the Cyborg infantry "CYB". The entity
    /// category is `Unit` (the only arm hosting `techno_common_post` today); the
    /// gate keys off the TYPE's `emits_damage_spark`, so this exercises the draw
    /// path. Live CYB Strength100 and the supplied actual HP set the health band.
    fn insert_cyborg_unit(sim: &mut Simulation, id: u64, current: i32) {
        assert_eq!(sim.allocate_stable_id(), id);
        let e = interned_entity(
            sim,
            id,
            "Americans",
            "CYB",
            EntityCategory::Unit,
            (5, 5),
            current,
        );
        sim.substrate.entities.insert(e);
    }

    const DAMAGE_SPARK_ROLL_MAX: u32 = 0x7fff_fffe;

    fn spark_system(sim: &Simulation, id: u64) -> Option<u64> {
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .spark_particle_system
            .system()
    }

    /// Scenario draws per damage-Spark gate, never on the main stream: above
    /// ConditionYellow (60/100 > 0.5) the outer gate fails; prob 0.0 makes the
    /// roll always fail (one draw, no list-pick, no system); a single Spark
    /// system's list-pick is `n(0,0)`, which draws nothing (gamemd RandomRanged
    /// min==max); a list without a Spark fails the inner gate (Spark count > 0).
    /// A VEHICLE authoring `Cyborg=yes` keeps `emits_damage_spark` (Type+0xC8F)
    /// false, since gamemd honours Cyborg only on infantry, so it never draws.
    #[test]
    fn s4b_damage_spark_draw_counts() {
        use crate::rules::ini_parser::IniFile;
        let vehicle_cyborg = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[General]\n\
BuildSpeed=0.75\nMultipleFactory=0.7\nLowPowerPenaltyModifier=1.25\n\
MinLowPowerProductionSpeed=0.4\nMaxLowPowerProductionSpeed=0.85\n\
ConditionRedSparkingProbability=1.0\nConditionYellowSparkingProbability=1.0\n\n\
[InfantryTypes]\n[VehicleTypes]\n1=VEHCYB\n[AircraftTypes]\n[BuildingTypes]\n\n\
[VEHCYB]\nCyborg=yes\nDamageParticleSystems=SparkA,SparkB\n\n{TWO_SPARK_SYSTEMS}\n"
        )))
        .expect("veh rules parse");
        let vehicle = vehicle_cyborg.object("VEHCYB").expect("VEHCYB present");
        assert!(vehicle.cyborg, "Cyborg= parsed");
        assert!(!vehicle.emits_damage_spark());
        let cases = [
            (
                "above ConditionYellow",
                cyborg_rules("1.0", "1.0", "SparkA,SparkB", TWO_SPARK_SYSTEMS),
                "CYB",
                60,
                &[][..],
                false,
            ),
            (
                "roll fails",
                cyborg_rules("0.0", "0.0", "SparkA,SparkB", TWO_SPARK_SYSTEMS),
                "CYB",
                20,
                &[DAMAGE_SPARK_ROLL_MAX][..],
                false,
            ),
            (
                "single Spark succeeds",
                cyborg_rules("1.0", "1.0", "SparkA,SmokeA", ONE_SPARK_ONE_SMOKE_SYSTEMS),
                "CYB",
                20,
                &[DAMAGE_SPARK_ROLL_MAX][..],
                true,
            ),
            (
                "no Spark system",
                cyborg_rules("1.0", "1.0", "SmokeA", SMOKE_ONLY_SYSTEMS),
                "CYB",
                20,
                &[][..],
                false,
            ),
            (
                "Cyborg vehicle",
                vehicle_cyborg,
                "VEHCYB",
                20,
                &[][..],
                false,
            ),
        ];
        for (case, rules, type_id, health, draws, sparks) in cases {
            let mut sim = Simulation::new();
            if type_id == "CYB" {
                insert_cyborg_unit(&mut sim, 1, health);
            } else {
                let e = interned_entity(
                    &mut sim,
                    1,
                    "Americans",
                    type_id,
                    EntityCategory::Unit,
                    (5, 5),
                    health,
                );
                sim.substrate.entities.insert(e);
            }
            let mut expect = sim.scenario_rng.clone();
            let main = sim.main_rng.state();
            techno_common_post(&mut sim, 1, Some(&rules));
            for &max in draws {
                expect.next_range_u32_inclusive(0, max);
            }
            assert_eq!(sim.scenario_rng.state(), expect.state(), "{case}");
            assert_eq!(sim.main_rng.state(), main, "{case}");
            assert_eq!(spark_system(&sim, 1).is_some(), sparks, "{case}");
            assert_eq!(sim.particle_systems().len(), usize::from(sparks), "{case}");
        }
    }

    #[test]
    fn s4b_two_draws_when_roll_succeeds() {
        // prob 1.0 → threshold MAX → roll always succeeds; 2 Spark systems → the
        // list-pick (n(0,1)) consumes a second draw before construction.
        let rules = cyborg_rules("1.0", "1.0", "SparkA,SparkB", TWO_SPARK_SYSTEMS);
        let mut sim = Simulation::new();
        insert_cyborg_unit(&mut sim, 1, 20);
        let mut expect = sim.scenario_rng.clone();
        let main = sim.main_rng.state();
        techno_common_post(&mut sim, 1, Some(&rules));
        expect.next_range_u32_inclusive(0, DAMAGE_SPARK_ROLL_MAX); // roll
        expect.next_range_u32_inclusive(0, 1); // list-pick over 2 sparks
        assert_eq!(
            sim.scenario_rng.state(),
            expect.state(),
            "roll + list-pick = two draws"
        );
        assert_eq!(sim.main_rng.state(), main);
        let system = sim
            .particle_systems()
            .get(spark_system(&sim, 1).unwrap())
            .unwrap();
        assert_eq!(system.owner_entity(), Some(1));
        assert_eq!(system.lifetime(), 5);
        assert!(system.in_logic_vector());
        assert!(
            sim.display_layers()
                .ordered_ids()
                .any(|&id| id == system.stable_id())
        );
    }

    #[test]
    fn s4b_no_draw_while_actual_system_live_then_expiry_resumes_roll() {
        let rules = cyborg_rules("1.0", "1.0", "SparkA,SparkB", TWO_SPARK_SYSTEMS);
        let mut sim = Simulation::new();
        insert_cyborg_unit(&mut sim, 1, 20);
        techno_common_post(&mut sim, 1, Some(&rules));
        let first = spark_system(&sim, 1).unwrap();
        let frozen = sim.scenario_rng.state();
        // Advancing the session clock alone must never expire a live link.
        sim.session.set_tick_for_test(1_000_000);
        techno_common_post(&mut sim, 1, Some(&rules));
        assert_eq!(sim.scenario_rng.state(), frozen);
        assert_eq!(spark_system(&sim, 1), Some(first));

        sim.substrate
            .particle_systems
            .get_mut(first)
            .unwrap()
            .set_done_spawning_for_test(true);
        sim.retire_particle_system(first, crate::sim::world::UninitContext::with_rules(&rules));
        assert_eq!(spark_system(&sim, 1), None);
        let mut expect = sim.scenario_rng.clone();
        expect.next_range_u32_inclusive(0, DAMAGE_SPARK_ROLL_MAX);
        expect.next_range_u32_inclusive(0, 1);
        techno_common_post(&mut sim, 1, Some(&rules));
        assert_eq!(sim.scenario_rng.state(), expect.state());
        assert_ne!(spark_system(&sim, 1), Some(first));
        assert!(spark_system(&sim, 1).is_some());
    }

    #[test]
    fn s4b_nonpositive_lifetime_retires_through_done_empty_system_ai() {
        let rules = cyborg_rules(
            "1.0",
            "1.0",
            "SparkA",
            "[ParticleSystems]\n1=SparkA\n[SparkA]\nBehavesLike=Spark\nLifetime=-1\n",
        );
        let mut sim = Simulation::new();
        insert_cyborg_unit(&mut sim, 1, 20);
        techno_common_post(&mut sim, 1, Some(&rules));
        let system_id = spark_system(&sim, 1).unwrap();
        assert_eq!(
            sim.particle_systems().get(system_id).unwrap().lifetime(),
            -1
        );
        // This is an explicit done-empty prior. Its real common AI, rather
        // than a guessed Lifetime<=0 timer, broadcasts the matching expiry.
        sim.substrate
            .particle_systems
            .get_mut(system_id)
            .unwrap()
            .set_done_spawning_for_test(true);
        crate::sim::particles::system_ai::tick_particle_system(
            &mut sim,
            &rules,
            system_id,
            crate::sim::world::UninitContext::with_rules(&rules),
        );
        assert_eq!(spark_system(&sim, 1), None);
        assert!(sim.particle_systems().get(system_id).unwrap().in_limbo());
        sim.process_pending_delete();
        assert!(sim.particle_systems().get(system_id).is_none());
    }
}

#[cfg(test)]
#[path = "techno_ai_selfheal_tests.rs"]
mod selfheal_tests;

#[cfg(test)]
#[path = "techno_ai_selfheal_oracle_tests.rs"]
mod selfheal_oracle_tests;

#[path = "bounce_terrain.rs"]
mod bounce_terrain;

#[cfg(test)]
#[path = "techno_ai/signed_health_tests.rs"]
mod signed_health_tests;

#[cfg(test)]
#[path = "techno_ai/bridge_target_layer_tests.rs"]
mod bridge_target_layer_tests;

#[cfg(test)]
#[path = "techno_ai/bridge_engineer_entry_tests.rs"]
mod bridge_engineer_entry_tests;

#[cfg(test)]
#[path = "techno_ai/bridge_low_repair_tests.rs"]
mod bridge_low_repair_tests;

#[cfg(test)]
#[path = "techno_ai/mirage_disguise_tests.rs"]
mod mirage_disguise_tests;

#[cfg(any(test, feature = "test-support"))]
use super::bridge_test_evidence;
