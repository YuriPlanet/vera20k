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
//! Must NOT depend on render/ui/sidebar/audio/net (sim invariant #1).
//! Dispatch is `match category` only — no trait object / dyn / vtable
//! (invariant #2).

mod mission_handlers;
mod target_scan;
pub(crate) use mission_handlers::harvester_enter_idle_mode_selector;
pub(crate) use mission_handlers::infantry_unlimbo_idle_mode;
pub(crate) use mission_handlers::queue_foot_enter_idle_mode;

use mission_handlers::*;
use target_scan::{can_acquire_target, passive_acquire_step};

use super::Simulation;
use crate::map::entities::EntityCategory;
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::rules::particle_system_type::ParticleSystemBehavesLike;
use crate::rules::ruleset::RuleSet;
use crate::sim::miner::MinerConfig;
use crate::sim::mission::MissionType;
use crate::sim::pathfinding::PathGrid;

/// Non-rules world context the mission handler bodies dispatched from the
/// host need (grids and per-tick config the spine already owns). Empty in
/// barebones fixtures — handlers that need an absent piece degrade the same
/// way the legacy global phases did with `None` arguments.
#[derive(Default, Clone, Copy)]
pub(crate) struct ObjectAiCtx<'a> {
    pub(crate) path_grid: Option<&'a PathGrid>,
    pub(crate) overlay_registry: Option<&'a OverlayTypeRegistry>,
    pub(crate) terrain_spawner_cells: Option<&'a std::collections::BTreeSet<(u16, u16)>>,
    pub(crate) miner_config: Option<&'a MinerConfig>,
}

// P3 oracle probe import — used only by the `#[cfg(test)]` factory_oracle_step_trace.
#[cfg(test)]
use crate::sim::production::StepOutcome;

impl Simulation {
    pub(crate) fn clear_active_wave_link(&mut self, wave_id: u64) {
        self.active_wave_links
            .retain(|_, linked_wave_id| *linked_wave_id != wave_id);
    }

    pub(crate) fn wave_update_context(&self, wave_id: u64) -> crate::sim::wave::WaveUpdateContext {
        use crate::sim::combat::TargetKind;
        use crate::sim::projectile::ProjectileCoord;

        let Some(wave) = self.waves.get(wave_id) else {
            return crate::sim::wave::WaveUpdateContext {
                owner_position: None,
                owner_current_target: None,
                target_position: None,
            };
        };
        // WaveClass keeps raw owner/target pointers through the represented
        // dying interval. Only PointerExpired makes either identity null.
        let owner = wave
            .owner_id
            .and_then(|owner_id| self.substrate.entities.get(owner_id));
        let owner_position = owner.map(|entity| {
            ProjectileCoord::new(
                i32::from(entity.position.rx) * 256 + entity.position.sub_x.to_num::<i32>(),
                i32::from(entity.position.ry) * 256 + entity.position.sub_y.to_num::<i32>(),
                crate::sim::combat::object_world_z_leptons(entity, self.resolved_terrain.as_ref()),
            )
        });
        let owner_current_target = owner
            .and_then(|entity| entity.attack_target.as_ref())
            .map(|attack| attack.target);
        let target_position = match wave.target_ref {
            Some(TargetKind::Entity(target_id)) => {
                self.substrate.entities.get(target_id).map(|entity| {
                    ProjectileCoord::new(
                        i32::from(entity.position.rx) * 256 + entity.position.sub_x.to_num::<i32>(),
                        i32::from(entity.position.ry) * 256 + entity.position.sub_y.to_num::<i32>(),
                        crate::sim::combat::object_world_z_leptons(
                            entity,
                            self.resolved_terrain.as_ref(),
                        ),
                    )
                })
            }
            Some(TargetKind::Cell(rx, ry)) => Some(self.wave_cell_target_position(rx, ry)),
            None => None,
        };
        crate::sim::wave::WaveUpdateContext {
            owner_position,
            owner_current_target,
            target_position,
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
    #[cfg(test)]
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
    #[cfg(test)]
    pub(crate) fn object_ai_post_movement_promote(&mut self, rules: Option<&RuleSet>) {
        let Some(rules) = rules else {
            return;
        };
        let now = self.session.binary_frame;
        self.for_each_live_object(|sim, id| {
            if sim.substrate.anims.contains_key(id) {
                return;
            }
            let Some(entity) = sim.substrate.entities.get(id) else {
                return;
            };
            if entity.dying {
                return;
            }
            if !matches!(
                entity.category,
                EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
            ) {
                return;
            }
            sim.mission_host_promote(id, now, rules);
            sim.aircraft_ammo_after_commence(id);
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
        if entity.dying
            || !matches!(
                entity.category,
                EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
            )
        {
            return;
        }
        self.mission_host_promote(id, self.session.binary_frame, rules);
        self.aircraft_ammo_after_commence(id);
    }

    /// AircraftAI41505E..415085 consumes a pending release after Ready/Commence,
    /// using the resulting native Mission+AC, including when an order interrupts
    /// Attack. It neither reads nor clears the separate readiness latch+6D2.
    fn aircraft_ammo_after_commence(&mut self, id: u64) {
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return;
        };
        if entity.category == EntityCategory::Aircraft && entity.mission.current().raw() != 1 {
            if let Some(ammo) = entity.aircraft_ammo.as_mut() {
                ammo.consume_release(false);
            }
        }
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
    pub(crate) fn visit_voxel_anim(&mut self, id: u64, rules: Option<&RuleSet>) {
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

    /// Dispatch one current LogicVector slot. A finishing death sequence calls
    /// UnInit synchronously here, so compacting removal is visible before the
    /// scheduler increments its cursor.
    pub(crate) fn object_ai_visit_one(
        &mut self,
        id: u64,
        rules: Option<&RuleSet>,
        ctx: ObjectAiCtx<'_>,
    ) -> bool {
        if self.substrate.anims.contains_key(id) {
            if let Some(rules) = rules {
                self.visit_anim(id, rules, ctx.overlay_registry);
            }
            return true;
        }
        if self.substrate.voxel_anims.contains_key(id) {
            self.visit_voxel_anim(id, rules);
            return true;
        }
        if self.substrate.particle_systems.contains_key(id) {
            if let Some(rules) = rules {
                crate::sim::particles::system_ai::tick_particle_system(self, rules, id);
            }
            return true;
        }
        if self.production.terrain_objects.contains_key(&id) {
            crate::sim::terrain_spawn::tick_terrain_object_ai(
                self,
                id,
                rules,
                ctx.overlay_registry,
                ctx.terrain_spawner_cells,
            );
            return true;
        }
        if let Some(projectile) = self.projectiles.get(id) {
            // BulletClass::AI @ 0x00467C3C reads the live firer (+0xB0),
            // then its +0x84 TechnoType receiver and JumpJet (+0xD94).
            // Do not cache this on launch: PointerExpired can null the firer.
            let source_is_jumpjet = rules.is_some_and(|rules| {
                self.substrate
                    .entities
                    .get(projectile.source_id)
                    .is_some_and(|source| {
                        self.object_type(source.type_ref(), rules)
                            .is_some_and(|kind| kind.jumpjet)
                    })
            });
            // `BulletClass::AI 0x0046699D` reads the GLOBAL frame counter's
            // parity for the half-rate course-locked acceleration ramp, so the
            // Logic slot has to hand the flight loop the current frame.
            let binary_frame = self.session.binary_frame;
            let shared_cell_dummy = self.effective_shared_cell_dummy();
            let terrain = self.resolved_terrain.as_ref();
            let overlay_grid = self.overlay_grid.as_ref();
            let occupancy = &self.substrate.occupancy;
            let entities = &self.substrate.entities;
            let interner = &self.interner;
            let house_alliances = &self.house_alliances;
            let collision_world = super::projectile_collision::ProjectileCollisionWorld {
                terrain,
                dummy: &shared_cell_dummy,
                occupancy,
                entities,
                interner,
                alliances: house_alliances,
                overlays: overlay_grid,
                overlay_registry: ctx.overlay_registry,
                rules,
                map_size: self
                    .playfield_bounds
                    .zip(self.playfield_size_height)
                    .map(|(bounds, height)| (bounds.base, height)),
            };
            let result = self
                .projectiles
                .advance_one(
                    id,
                    binary_frame,
                    |target_id| {
                        collision_world
                            .target_aim(crate::sim::projectile::ProjectileTarget::Entity(target_id))
                    },
                    terrain,
                    &shared_cell_dummy,
                    rules.map_or_else(
                        || crate::rules::ruleset::GeneralRules::default().gravity,
                        |rules| rules.general.gravity,
                    ),
                    source_is_jumpjet,
                    |projectile, candidate, phase| {
                        collision_world.collide(projectile, candidate, phase)
                    },
                )
                .expect("projectile remained present for its Logic slot");
            let terminal = !result.expired.is_empty() || !result.detonations.is_empty();
            if let Some(rules) = rules {
                self.commit_logic_projectile_detonations(
                    rules,
                    ctx.overlay_registry,
                    &result.detonations,
                );
            } else {
                // Rules-less fixture dispatch has no authoritative receiver
                // contract. Production always commits at the Bullet slot.
                self.pending_projectile_detonations
                    .extend(result.detonations);
            }
            if terminal {
                let retired = self.retire_non_entity_object(id);
                debug_assert!(retired);
            }
            return true;
        }
        if self.waves.get(id).is_some() {
            let context = self.wave_update_context(id);
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
            if let Some(request) = request {
                if let Some(rules) = rules {
                    self.commit_logic_wave_damage_request(rules, ctx.overlay_registry, &request);
                } else {
                    // Rules-less fixture dispatch retains the former buffer;
                    // production Wave AI commits before lifetime retirement.
                    self.pending_wave_damage_requests.push(request);
                }
            }
            if !result.alive {
                let retired = self.retire_non_entity_object(id);
                debug_assert!(retired);
            }
            return true;
        }

        // Building43FB20 samples its operational edge before delayed Health0
        // cleanup. A live dying-animation diversion must not skip that edge.
        if let Some(rules) = rules {
            self.visit_building_operational(id, rules);
        }
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        if entity.infantry_terminal.is_some() {
            return self.visit_infantry_terminal(id, rules, ctx);
        }
        if entity.dying {
            let Some(rules) = rules else {
                return true;
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
                &self.session.game_options,
                self.session.binary_frame,
            );
            if finished {
                self.release_move_sound(id);
                self.uninit_with_rules(id, rules);
            }
            return true;
        }

        // UnitClass::AI / InfantryClass::AI test the object-owned TubeMovement
        // index before entering the ordinary Foot/mission body.  The active
        // leaf runs later in this same LogicVector slot (the world host owns
        // the mutable movement substrates), so this visit must contribute no
        // mission-counter, queued-mission, cadence, or passive-target work.
        if matches!(
            entity.category,
            EntityCategory::Unit | EntityCategory::Infantry
        ) && entity.low_bridge_tube_state.is_some()
        {
            return true;
        }

        let category = entity.category;
        techno_ai_shell(self, id, category, rules, ctx);
        true
    }

    /// [`Simulation::object_ai_stage`] with the world context the dispatched
    /// mission handler bodies need (the production spine entry).
    #[cfg(test)]
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
    /// Dying objects retain their slot while their death sequence runs but do
    /// not enter the ordinary per-category shell. When `record`, return the
    /// visited ids in order (debug/test observation); otherwise the returned
    /// `Vec` is empty and unallocated.
    #[cfg(test)]
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

/// Per-category dispatch shell.
///
/// `match category` — NO trait / dyn / vtable (invariant #2). Every arm runs
/// the common per-object Mission work (`+0xC4` counter + owner-local queued
/// promotion); the Unit arm additionally runs the TechnoClass common bracket
/// (pre/post blocks). Absorbing the remaining per-leaf behavior (movement,
/// turret, combat, fear/sequence, aircraft dispatch) is later per-arm work.
/// The match is exhaustive over the four real variants (no `_` arm), so a
/// future `EntityCategory` addition is a compile error, intentionally.
fn techno_ai_shell(
    sim: &mut Simulation,
    id: u64,
    category: EntityCategory,
    rules: Option<&RuleSet>,
    ctx: ObjectAiCtx<'_>,
) {
    // Each leaf runs its chain head's TemporalClass::Update first (Unit
    // 0x00736204, Infantry 0x0051BB6E, Aircraft 0x00414BDB; a Building after
    // its damage fire at 0x0043FC39, 0x0043FCF9) and a warped object returns
    // before the common Techno body. VERA's object turn then skips its
    // locomotor for the same reason.
    if let Some(rules) = rules {
        if category == EntityCategory::Structure {
            sim.update_building_damage_fire(id, rules);
        }
        if sim.temporal_ai_prologue(id, rules) {
            return;
        }
    }
    // Techno6F9F6E..9F precedes promotion, missions and acquisition for every
    // Techno category. object_ai_visit_one excludes the entry-active Tube leaf
    // before this common owner. Actual health and this retained estimate differ.
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity
            .estimated_health
            .recover(i32::from(entity.health.current), sim.session.binary_frame);
    }
    match category {
        EntityCategory::Unit => {
            unit_techno_bracket(sim, id, rules, ctx);
        }
        // InfantryClass::AI promotes queued missions via Ready→Commence
        // (`0x0051BC51`/`0x0051BF03`); the fear/sequence absorption is later work.
        // Infantry reach the common Techno AI body through the same foot-leaf
        // call units do, so they run the same off-mission clear and passive
        // block in the same order. Idle infantry are the majority of on-map
        // objects; without this a squad holding a chokepoint does nothing until
        // something shoots it. A passenger inside a transport is gated out by
        // the mission read, and a garrisoned occupant fires through the garrison
        // path instead.
        // The supported Foot mission cadence branches run here as well.
        EntityCategory::Infantry => {
            if let Some(rules) = rules
                && !techno_common_steps(sim, id, rules, ctx.overlay_registry)
            {
                return;
            }
            clear_passive_target_off_mission(sim, id);
            mission_common_step(sim, id, rules);
            if let Some(rules) = rules {
                dispatch_supported_foot_mission_cadence(sim, id, rules, ctx);
            }
            passive_acquire_step(sim, id, rules, ctx);
            bomb_fuse_slot(sim, id, rules, ctx.overlay_registry);
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
                if !techno_common_steps(sim, id, rules, ctx.overlay_registry) {
                    return;
                }
            }
            // Buildings run the SAME common Techno AI body units do — it is the
            // only acquisition path a base defence has. Same order: off-mission
            // clear, then the counter/promotion, then the passive block. There
            // is deliberately no Guard→Attack mission flip at the dispatch point
            // between them — see the block comment above
            // `clear_passive_target_off_mission`'s neighbours for why.
            //
            // The clear is DEAD for structures as things stand, and is kept only
            // so the arm keeps the body's shape: a structure never carries a
            // destination, a navigation goal or a standing order, so its
            // committed mission always reads as finished, the derived Guard
            // reading always wins, and Guard is not one of the twelve missions
            // that strip a scanner target. It starts doing work the moment a
            // structure gains live mission machinery.
            //
            // RESIDUAL, same root cause: a structure being sold holds the
            // Selling mission with nothing running, so it reads Guard and keeps
            // scanning, acquiring and firing for the couple of seconds the sale
            // takes. Same shape as the `building_up` residual noted on
            // `passive_acquire_step`.
            clear_passive_target_off_mission(sim, id);
            // BuildingClass::Update consumes its ready latch via Ready→Commence
            // (`0x0043FE43`/`0x0043FFA3`); with no latch writers live the
            // promotion evaluates to not-ready (recorded residual).
            mission_common_step(sim, id, rules);
            passive_acquire_step(sim, id, rules, ctx);
            if !bomb_fuse_slot(sim, id, rules, ctx.overlay_registry) {
                return;
            }
            // BuildingClass::Update consumes the shared C4/PostMortem latch at
            // its late tail. Keep the forced receiver inline in this object's
            // LogicVector visit so nested death effects precede the next slot.
            if let Some(rules) = rules {
                sim.tick_pending_building_detonation(id, rules, ctx.overlay_registry);
            }
        }
        // AircraftClass::AI reaches the shared Foot/mission work before its
        // locomotor, but promotes via Ready→Commence only after Foot returns
        // (`0x0041504A`/`0x00415058`). Keep the counter here; the sole promotion
        // is `object_ai_post_movement_promote_one`.
        //
        // RESIDUAL — no passive block on this arm. Aircraft reach the common
        // Techno AI body in the original through the same foot-leaf call the
        // Unit and Infantry leaves use, so the block is shared with them there;
        // whether it does anything for a YR aircraft in practice is UNCHECKED.
        // It is omitted here because VERA's aircraft mission machine owns firing
        // and return-to-base, and the idle/parked/docked aircraft states all read
        // as Guard — so wiring this in would install targets on helipad-parked
        // aircraft outside the system that decides when they may shoot. Doing it
        // properly means choosing which aircraft states may acquire and routing
        // the pick through that machine, which is its own slice.
        EntityCategory::Aircraft => {
            if let Some(rules) = rules
                && !techno_common_steps(sim, id, rules, ctx.overlay_registry)
            {
                return;
            }
            drop_unsensed_cloaked_target_step(sim, id);
            mission_counter_step(sim, id);
            // The Aircraft Unload slot `0x004151E0` (vtable `+0x23C`), the one
            // aircraft mission handler absorbed so far; timer-gated inside.
            if let Some(rules) = rules {
                crate::sim::transport_unload::dispatch_aircraft_unload(
                    sim,
                    id,
                    rules,
                    ctx.path_grid,
                    ctx.overlay_registry,
                );
                // The remaining aircraft missions dispatch here too, inside
                // this slot and before Fly Process (FootClass::AI4DA530).
                if crate::sim::aircraft::dispatch_aircraft_mission(sim, rules, id, ctx.path_grid) {
                    sim.aircraft_fire_requests.insert(id);
                }
            }
            bomb_fuse_slot(sim, id, rules, ctx.overlay_registry);
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
/// `owner == g_PlayerPtr` in a skirmish). The flash countdown itself is ticked
/// here too; native decrements it through a timer virtual later in the same
/// update (`vtable+0x124`, UNCHECKED position) and only the presentation reads
/// it, so the one-frame placement difference is invisible.
fn veterancy_promotion_step(sim: &mut Simulation, id: u64, rules: &RuleSet) {
    let Some(entity) = sim.substrate.entities.get_mut(id) else {
        return;
    };
    entity.elite_flash_frames = entity.elite_flash_frames.saturating_sub(1);
    let Some(promotion) =
        crate::sim::combat::veterancy::sample_promotion(entity, rules.general.elite_flash_timer)
    else {
        return;
    };
    let owner = entity.owner();
    let (rx, ry) = (entity.position.rx, entity.position.ry);
    let (sound, elite) = match promotion {
        crate::sim::combat::veterancy::Promotion::Elite => {
            (rules.general.upgrade_elite_sound.as_deref(), true)
        }
        crate::sim::combat::veterancy::Promotion::Veteran => {
            (rules.general.upgrade_veteran_sound.as_deref(), false)
        }
    };
    let sound_id = sound.map(|name| sim.interner.intern(name));
    sim.sound_events.push(super::SimSoundEvent::UnitPromoted {
        owner,
        sound_id,
        elite,
        rx,
        ry,
    });
    refresh_mover_speed_after_promotion(sim, id, rules);
}

/// Re-derive a moving unit's `FASTER` speed the moment its rank crosses.
///
/// gamemd never caches a mover speed: the ground locomotors call
/// `FootClass::GetCurrentSpeed @ 0x004DB1A0` through vtable slot `+0x538` on
/// every frame they step (`DriveLocomotionClass::Process_Drive_Track
/// @ 0x004B1274`, `WalkLocomotionClass::ProcessMovement @ 0x0075BFC0`,
/// `ShipLocomotionClass::Process_Drive_Track @ 0x006A093C`,
/// `HoverLocomotionClass::Move @ 0x00514372`), so a unit promoted mid-path
/// speeds up on the very next frame. VERA stamps `MovementTarget::speed` once,
/// at path creation, and recomputes only `current_speed` per frame — so the
/// rank is the one input to the getter that can change while a path is live.
/// Refreshing it here reproduces the native observable without a per-frame
/// type lookup: the type speed, the house multiplier and the crate multiplier
/// are stable for the life of a path or are separate open rows.
///
/// RESIDUAL, and unreachable today: a unit promoted while its group is
/// speed-matched to the slowest member (the `movement_tick` group-min clamp,
/// VERA-internal — gamemd has no such clamp) would keep the formation speed
/// until the next order. Every production `Command::Move` currently passes
/// `group_id: None`, so `sync_formation_speeds_after_live_pass` never clamps
/// and this cannot fire. Trigger: promotion during a group move, once group
/// moves carry an id. Frequency: zero today. Downstream risk: none — the
/// clamp re-applies on the next path.
fn refresh_mover_speed_after_promotion(sim: &mut Simulation, id: u64, rules: &RuleSet) {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    if entity.movement_target.is_none() {
        return;
    }
    let obj = sim.object_type(entity.type_ref(), rules);
    let loco_multiplier = entity
        .locomotor
        .as_ref()
        .map(|loco| loco.speed_multiplier)
        .unwrap_or(crate::util::fixed_math::SIM_ONE);
    let base = crate::sim::combat::veterancy::entity_mover_speed_leptons_per_second(
        entity,
        obj,
        obj.map_or(4, |o| o.speed),
        rules.general.veteran_speed,
    );
    let speed = (base * loco_multiplier).max(crate::util::fixed_math::SimFixed::lit("25"));
    if let Some(target) = sim
        .substrate
        .entities
        .get_mut(id)
        .and_then(|e| e.movement_target.as_mut())
    {
        target.speed = speed;
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
        sim.session.binary_frame,
        interval,
    ) {
        return;
    }
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity.health.current = entity.health.current.wrapping_add(1);
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
fn drop_unsensed_cloaked_target_step(sim: &mut Simulation, id: u64) {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    let Some(crate::sim::combat::TargetKind::Entity(target_id)) =
        entity.attack_target.as_ref().map(|target| target.target)
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
    let target_cell = (target.position.rx, target.position.ry);
    if sim
        .fog
        .is_friendly(&owner_str, sim.interner.resolve(target.owner()))
    {
        return;
    }
    if sim
        .fog
        .has_sensor_for_house(owner, target_cell.0, target_cell.1)
    {
        return;
    }
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        crate::sim::mission::concrete_effects::represented_assign_target(entity, None);
    }
}

/// Tick the common `+0xC4` per-mission AI counter once per object visit.
fn mission_counter_step(sim: &mut Simulation, id: u64) {
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity.mission.increment_ai_counter();
    }
}

/// The pre-movement Mission step for categories whose leaf AI has a
/// Ready→Commence checkpoint at this position. Promotion needs parsed rules
/// for Unit world lookups; a rules-less call ticks the counter and leaves the
/// queue for a later rules-bearing pass.
fn mission_common_step(sim: &mut Simulation, id: u64, rules: Option<&RuleSet>) {
    mission_counter_step(sim, id);
    if let Some(rules) = rules {
        let now = sim.session.binary_frame;
        sim.mission_host_promote(id, now, rules);
    }
}

// ===== TechnoClass common-body bracket =====
//
// Per live Unit, gamemd's `TechnoClass::AI_Update` body is one contiguous
// bracket: pre-mission block -> +0xC4/Mission work -> post-mission block, with
// two IsAlive early-returns (after the pre-block, after dispatch). The mission
// work at the dispatch point is the flip's counter + owner-local promotion;
// the handler-body execution remains with legacy per-system phases except for
// the timer-only Move reschedule below and the absorbed Harvest handler.

/// S4a pre-mission common block (the `TechnoClass::AI_Update` head: one-shot
/// flag clear, turret-anim loop sound, cloak tick, health smoothing, target
/// validation, …). The stock cloak producer now executes at the verified head;
/// the remaining common-body items stay owned by their existing phases.
#[allow(unused_variables)]
/// `TechnoClass::AI_Update`'s leading common steps, in native order: the
/// promotion sample (`0x006FA054`), the drain blocks (`0x006FA14B..
/// 0x006FA224`, directly after the rank-cache write at `0x006FA145`), the
/// CaptureManager update (`0x006FA730`), then the IsAlive gate (`0x006FA735`)
/// before the self-heal pulse. Returns false when the object died in them (an
/// overloaded Mastermind), which ends its AI body. The bomb fuse, which
/// natively sits between passive acquisition and the CaptureManager
/// (`0x006FA6F5`), runs from each category's arm after its passive step
/// ([`bomb_fuse_slot`]).
///
/// RESIDUAL: natively the CaptureManager follows the mission step, passive
/// acquisition and the bomb fuse, which VERA runs after this block. Trigger: a
/// Yuri or Mastermind whose capture update draws or kills in the frame it would
/// also acquire, advance its mission or set off a bomb it carries. Effect: the
/// Scenario draws of those steps come in a different order, and a controller
/// the overload kills skips them. Frequency: rare; the bomb case needs a
/// bombed controller at its fuse's end. Downstream: later Scenario draws in
/// that frame shift.
fn techno_common_steps(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
) -> bool {
    veterancy_promotion_step(sim, id, rules);
    crate::sim::credit_income::drain_common_step(sim, id, rules);
    allied_target_drop_step(sim, id, rules);
    illegal_target_drop_step(sim, id, rules);
    sim.capture_manager_update(id, rules, overlay_registry);
    if !sim.substrate.entities.get(id).is_some_and(|e| e.is_alive()) {
        return false;
    }
    self_heal_step(sim, id, rules);
    true
}

/// `TechnoClass::AI_Update @ 0x006FA472..0x006FA4CB`, after the drain
/// blocks: on every sixteenth frame an object with a target asks GetFireError
/// without range (vt+0x3BC, `SelectWeapon(Target)`) and drops the target on
/// ILLEGAL (5) or CANT (6), unless its current mission is Capture (8) or
/// Sabotage (0x11). A Unit's and an Infantry's own fire routines keep an
/// illegal target; this is where they let go of it.
fn illegal_target_drop_step(sim: &mut Simulation, id: u64, rules: &RuleSet) {
    use crate::sim::combat::{TargetKind, fire_error::FireError};
    if !sim.session.binary_frame.is_multiple_of(16) {
        return;
    }
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    let Some(target) = entity.attack_target.as_ref().map(|attack| attack.target) else {
        return;
    };
    // A target that no longer resolves never reaches this check natively
    // (`PointerExpired` clears it first); the Attack handler owns VERA's
    // clear of such a stale target.
    if let TargetKind::Entity(target_id) = target
        && sim.substrate.entities.get(target_id).is_none()
    {
        return;
    }
    if matches!(entity.mission.current().raw(), 8 | 0x11) {
        return;
    }
    let weapon = target_scan::select_weapon(sim, rules, id, Some(target));
    let code = target_scan::fire_error_at(sim, rules, id, Some(target), weapon, false);
    if matches!(code, FireError::Illegal | FireError::Cant)
        && let Some(entity) = sim.substrate.entities.get_mut(id)
    {
        crate::sim::mission::concrete_effects::represented_assign_target(entity, None);
    }
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
    let controller_house = entity
        .passenger_role
        .inside_transport_id()
        .and_then(|transport_id| sim.substrate.entities.get(transport_id))
        .filter(|transport| {
            rules
                .object(sim.interner.resolve(transport.type_ref()))
                .is_some_and(|obj| obj.open_topped)
        })
        .and_then(|transport| transport.mind_control.controller())
        .and_then(|controller| sim.substrate.entities.get(controller))
        .map(|controller| controller.owner());
    let house = controller_house.unwrap_or(entity.owner());
    if sim
        .houses
        .get(&house)
        .is_some_and(|state| state.is_controlled_by_human(sim.session.game_mode_nonzero))
    {
        return;
    }
    // `0x004F9AF0` tests the target's Object flag first: a cell is never
    // allied.
    let Some(TargetKind::Entity(target_id)) =
        entity.attack_target.as_ref().map(|attack| attack.target)
    else {
        return;
    };
    let Some(target) = sim.substrate.entities.get(target_id) else {
        return;
    };
    if !combat_weapon::is_ally_by_object(
        Some(&sim.fog.alliances),
        &sim.interner,
        house,
        target.owner(),
    ) {
        return;
    }
    let (Some(obj), Some(target_obj)) = (
        rules.object(sim.interner.resolve(entity.type_ref())),
        rules.object(sim.interner.resolve(target.type_ref())),
    ) else {
        return;
    };
    let infantry = entity.category == EntityCategory::Infantry;
    let target_building = target.category == EntityCategory::Structure;
    let occupiable = infantry
        && target_building
        && target.passenger_role.cargo().is_some_and(|cargo| {
            crate::sim::passenger::can_dock_occupier_garrison(
                entity,
                target,
                obj,
                target_obj,
                cargo,
                rules,
                &sim.houses,
                None,
            )
        });
    let engineer = infantry && obj.engineer;
    let overpowers = target_building
        && target_obj.overpowerable
        && target_scan::weapon_at_index(sim, rules, id, 1)
            .and_then(|weapon| combat_weapon::warhead_of(rules, weapon))
            .is_some_and(|warhead| warhead.electric_assault);
    if engineer || overpowers || occupiable || entity.berserk.active {
        return;
    }
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        crate::sim::mission::concrete_effects::represented_assign_target(entity, None);
    }
}

/// The Techno AI a Die1/Die2 corpse still runs. `InfantryClass::AI` keeps
/// calling `FootClass::AI` (`0x0051BC9F`) until the sequence completes, and
/// `TechnoClass::AI_Update` has no health gate on these steps: the estimate
/// recovery (`0x006F9F6E`), the allied and illegal target drops
/// (`0x006FA30C`, `0x006FA472`) and the passive block (`0x006FA65A`), whose
/// scan draws and can acquire. Mission dispatch alone skips a Health-0
/// object (`0x005B30A7`). The corpse's own fire routine is refused CANT for
/// its death Doing (`0x0051C8B8`).
///
/// RESIDUAL: the rest of the subset (veterancy, drains, CaptureManager,
/// self-heal, cloak, bomb) does nothing for a GI corpse and is not run.
pub(super) fn dying_infantry_techno_ai(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    ctx: ObjectAiCtx<'_>,
) {
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity
            .estimated_health
            .recover(entity.health.current, sim.session.binary_frame);
    }
    allied_target_drop_step(sim, id, rules);
    illegal_target_drop_step(sim, id, rules);
    passive_acquire_step(sim, id, Some(rules), ctx);
}

/// The bomb fuse's slot in `TechnoClass::AI_Update` (`0x006FA6F5..
/// 0x006FA717`): after the mission step and passive acquisition, before the
/// SlaveManager and CaptureManager. A carrier its own blast kills runs no
/// further AI this frame (the IsAlive gate at `0x006FA735`).
fn bomb_fuse_slot(
    sim: &mut Simulation,
    id: u64,
    rules: Option<&RuleSet>,
    overlay_registry: Option<&OverlayTypeRegistry>,
) -> bool {
    if let Some(rules) = rules {
        sim.bomb_fuse_step(id, rules, overlay_registry);
    }
    sim.substrate.entities.get(id).is_some_and(|e| e.is_alive())
}

fn techno_common_pre(
    sim: &mut Simulation,
    id: u64,
    rules: Option<&RuleSet>,
    overlay_registry: Option<&OverlayTypeRegistry>,
) {
    let Some(rules) = rules else { return };
    // `AI_Update` order: the promotion sample and the self-heal pulse both
    // precede the cloak tick (`vtable+0x410` at `0x006FA946`), so a same-frame
    // heal is visible to the cloak's health branch.
    if !techno_common_steps(sim, id, rules, overlay_registry) {
        return;
    }
    super::techno_ai_cloak::tick_stock_cloak_producer(sim, id, rules);
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    let Some(object_type) = rules.object(sim.interner.resolve(entity.type_ref())) else {
        return;
    };
    if !object_type.disguise_when_still || entity.locomotor.is_none() {
        return;
    }
    let is_moving =
        crate::sim::movement::drive_locomotor_is_moving(entity) || entity.movement_target.is_some();
    if is_moving {
        if let Some(disguise) = sim
            .substrate
            .entities
            .get_mut(id)
            .and_then(|e| e.disguise.as_mut())
        {
            disguise.clear_unit();
        }
        return;
    }
    let blocked_by_contact = !entity.radio_contacts.is_empty();
    let reveal_blocking = entity
        .disguise
        .as_ref()
        .is_some_and(|state| state.raw_reveal_remaining(sim.session.binary_frame) != 0);
    if blocked_by_contact || reveal_blocking || rules.general.default_mirage_disguises.is_empty() {
        return;
    }

    // `UnitClass::UpdateDisguise @ 0x007468c0`: one RandomRanged draw on every
    // eligible unblocked update; selection is independent of the 7/8 scan cadence.
    let last = rules.general.default_mirage_disguises.len() as u32 - 1;
    let index = sim.scenario_rng.next_range_u32_inclusive(0, last) as usize;
    let disguise_type = sim
        .interner
        .intern(&rules.general.default_mirage_disguises[index]);
    let owner = sim.substrate.entities.get(id).map(|e| e.owner());
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        let state = entity.disguise.get_or_insert_with(Default::default);
        state.acquire(sim.session.binary_frame, Some(disguise_type), owner);
    }
}

/// `damage_particle_live_until` sentinel for a spawned spark system whose
/// `Lifetime <= 0`: gamemd's `ParticleSystemClass::AI` removal counter (set from
/// `Type+0x2b8`) only fires on `--counter == 0`, so a non-positive lifetime never
/// reaches 0 going down → the system (and thus `+0x308`) holds for the whole
/// match. Distinct from a real finite `spawn_tick + lifetime` (always `>= 1` for
/// `lifetime > 0`, and `tick` won't reach `u64::MAX` in any real match).
const DAMAGE_PARTICLE_LIVE_FOREVER: u64 = u64::MAX;

/// Largest `roll` value the gamemd damage-Spark prob-roll yields
/// (`RandomRanged(0, 0x7ffffffe)`).
const DAMAGE_SPARK_ROLL_MAX: u32 = 0x7fff_fffe;

/// S4a post-mission common block (the steps after `Mission_Dispatch`: passive
/// acquire (S4c), the damage-particle RNG (S4b), the timer accumulator, EMP
/// recovery).
///
/// S4b — the AI_Update damage-Spark `scenario_rng` consumption, modelled exactly
/// from the verified gamemd block. Per object, per tick:
///   - Outer gate: `emits_damage_spark` (TechnoTypeClass `+0xC8F` = `Cyborg`,
///     infantry-only) AND `HealthRatio < ConditionYellow` (STRICT) AND
///     not-in-special-damage-state (`vtable+0x1c8() > -10`, unmodelled → pass).
///   - Build the Spark sublist of `DamageParticleSystems` (`BehavesLike == Spark`).
///     No RNG.
///   - Inner gate: no live spark system (`+0x308`-equivalent empty) AND Spark
///     count > 0.
///   - Draw #1 (always, on inner-gate pass): the prob-roll on `scenario_rng`;
///     succeed iff `roll < threshold` (red band if `HealthRatio < ConditionRed`,
///     else yellow). On success → Draw #2: the list-pick `n(0, count-1)` (consumes
///     no draw when count == 1, matching gamemd `RandomRanged(min == max)`), and
///     arm the live-system hold to `tick + sparkType.Lifetime`.
///
/// Draw truth table (`scenario_rng`): 0 (outer gate fail / live system / no
/// Spark) — 1 (roll fails) — 2 (roll succeeds, count >= 2) — 1 (roll succeeds,
/// count == 1). The spawn/offset/ctor draw NOTHING. The visual is render-side;
/// this consumes the draws and tracks the gate only.
///
/// Dormant in stock YR: `emits_damage_spark` is false for every stock type (no
/// `Cyborg=yes` units, and `techno_common_post` runs only for the vehicle arm),
/// so the early-out fires before any allocation or draw — zero `scenario_rng`
/// movement. Modelled exactly so the stream stays aligned if a mod ever enables it.
fn techno_common_post(sim: &mut Simulation, id: u64, rules: Option<&RuleSet>) {
    let Some(rules) = rules else {
        return;
    };

    // Read the entity facts we need, then resolve the type. `obj` borrows `rules`
    // (external), not `sim`, so the later &mut sim draws/writes don't alias it.
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    let type_ref = entity.type_ref();
    let live_until_in = entity.damage_particle_live_until;
    let Some(obj) = rules.object(sim.interner.resolve(type_ref)) else {
        return;
    };

    use crate::util::native_x87::{MaskedX87Chop53 as X87, MaskedX87Ordering, NativeF64Bits};
    let health_ratio = entity.health.ratio(obj.strength);
    let below = |threshold: f64| {
        matches!(
            X87::compare(
                health_ratio,
                X87::load_f64(NativeF64Bits::from_bits(threshold.to_bits()))
            ),
            MaskedX87Ordering::Less | MaskedX87Ordering::Unordered
        )
    };

    // Outer gate. Check the cheap, near-always-false `emits_damage_spark` first so
    // the common path (every stock vehicle) exits before building the Spark list.
    // 6FACF3..FE tests x87 C0: strict Less or Unordered. The red selector
    // at6FADDA..E5 uses the same predicate on the signed live-Strength ratio.
    // The `vtable+0x1c8() > -10` special-state term is unmodelled here → pass.
    let below_yellow = below(rules.general.condition_yellow);
    if !(obj.emits_damage_spark() && below_yellow) {
        return;
    }

    // Spark sublist: `DamageParticleSystems` entries resolving to a
    // `BehavesLike == Spark` particle system, in list order. Collect each one's
    // Lifetime for the `+0x308` hold; the list-pick indexes into this sublist.
    let mut spark_lifetimes: Vec<i32> = Vec::new();
    for name in &obj.damage_particle_systems {
        if let Some(ps_id) = rules.ps_type_id_by_name(name) {
            let pst = rules.particle_system_type(ps_id);
            if pst.behaves_like == ParticleSystemBehavesLike::Spark {
                spark_lifetimes.push(pst.lifetime);
            }
        }
    }
    let spark_count = spark_lifetimes.len() as u32;
    // Band select needs ConditionRed; bind once (both gate and draw read it).
    let below_red = below(rules.general.condition_red);

    // `+0x308`-equivalent live-system gate. Resolve expiry lazily here (the only
    // observable effect of the hold is gating draws, which only happen under this
    // gate, so lazy expiry yields the same draw sequence as gamemd's eager null).
    let tick = sim.session.tick;
    let mut live_until = live_until_in;
    if live_until != 0 && live_until != DAMAGE_PARTICLE_LIVE_FOREVER && tick >= live_until {
        live_until = 0; // system expired → `+0x308` nulls; may roll again this tick
    }
    let system_live = live_until != 0;

    // Inner gate: no live system AND at least one Spark system.
    if !system_live && spark_count > 0 {
        // Draw #1 — prob-roll on Scen->Random (always, on inner-gate pass).
        let roll = sim
            .scenario_rng
            .next_range_u32_inclusive(0, DAMAGE_SPARK_ROLL_MAX);
        let threshold = if below_red {
            rules.general.condition_red_spark_threshold
        } else {
            rules.general.condition_yellow_spark_threshold
        };
        if roll < threshold {
            // Draw #2 — list-pick (no draw when spark_count == 1: n(0,0)).
            let idx = sim
                .scenario_rng
                .next_range_u32_inclusive(0, spark_count - 1) as usize;
            let lifetime = spark_lifetimes[idx];
            live_until = if lifetime > 0 {
                tick.saturating_add(lifetime as u64)
            } else {
                DAMAGE_PARTICLE_LIVE_FOREVER
            };
        }
    }

    // Commit the (possibly cleared or freshly-armed) live-system state.
    if live_until != live_until_in {
        if let Some(entity) = sim.substrate.entities.get_mut(id) {
            entity.damage_particle_live_until = live_until;
        }
    }
}

/// Outcome of the common bracket for one Unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BracketReach {
    /// Died after the pre-block (health 0); no mission work ran.
    DiedInPre,
    /// Live Unit: ran the `+0xC4` counter + owner-local promotion at the
    /// dispatch point.
    Dispatched,
}

/// Per-Unit TechnoClass common bracket. Runs the contiguous
/// `pre -> [IsAlive B] -> +0xC4/promotion -> [IsAlive E] -> post` structure at
/// the gamemd-faithful per-object AI point (pre-movement, LogicVector order).
/// The promotion is UnitClass::AI's Ready→Commence (`0x00736473`, the call
/// before FootClass::AI; the second in-update Ready→Commence at `0x007366FD`
/// is a recorded residual).
fn unit_techno_bracket(
    sim: &mut Simulation,
    id: u64,
    rules: Option<&RuleSet>,
    ctx: ObjectAiCtx<'_>,
) -> BracketReach {
    techno_common_pre(sim, id, rules, ctx.overlay_registry);
    // Guard B (post-pre IsAlive): a health-0 Unit runs no mission work. No
    // lethal pre-block step exists yet, so this fires only for an already-dead
    // Unit.
    if !sim.substrate.entities.get(id).is_some_and(|e| e.is_alive()) {
        return BracketReach::DiedInPre;
    }
    // The off-mission passive-target clear runs BEFORE the +0xC4 counter.
    clear_passive_target_off_mission(sim, id);
    mission_common_step(sim, id, rules);
    // Mission_Dispatch position: the absorbed handler bodies run here,
    // timer-gated, ending with the verified post-handler epilogue write
    // (start = current frame, delay = handler return). Harvest (the miner
    // FSM) is the first absorbed handler; Move/Guard are Track A2.
    if let (Some(rules), Some(config)) = (rules, ctx.miner_config) {
        crate::sim::miner::dispatch_harvest_for_object(
            sim,
            rules,
            config,
            ctx.path_grid,
            ctx.overlay_registry,
            id,
        );
    }
    if let Some(rules) = rules {
        dispatch_supported_foot_mission_cadence(sim, id, rules, ctx);
    }
    // A transport turning in place for its Unload reads its hull
    // `PrimaryFacing.Current()` every frame; the movement tick only turns
    // objects that hold a movement target, so the idle turn is advanced here.
    if let Some(rules) = rules {
        crate::sim::transport_unload::refresh_idle_hull_turn(sim, id, rules);
    }
    // `UnitClass::AI @ 0x007361A9..0x007361E9`: a draining Floating Disc
    // re-checks the building under it every 16th frame and drops the link
    // when it has drifted off (after FootClass::AI in the native order).
    crate::sim::credit_income::drain_unit_ai_step(sim, id);
    // Passive / opportunity target acquisition sits between mission dispatch
    // and the second IsAlive guard, before the object's own locomotion.
    passive_acquire_step(sim, id, rules, ctx);
    bomb_fuse_slot(sim, id, rules, ctx.overlay_registry);
    // Guard E (post-dispatch IsAlive): the dispatched handler, or the bomb it
    // carried, may have destroyed the Unit; a dead Unit runs no post-mission
    // block.
    if !sim.substrate.entities.get(id).is_some_and(|e| e.is_alive()) {
        return BracketReach::Dispatched;
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
fn clear_passive_target_off_mission(sim: &mut Simulation, id: u64) {
    let drop = sim.substrate.entities.get(id).is_some_and(|entity| {
        entity.attack_target.is_some()
            && entity.passively_acquired_target
            && PASSIVE_TARGET_CLEAR_MISSIONS.contains(&entity.passive_acquire_mission())
    });
    if !drop {
        return;
    }
    let _ = sim.set_archive_target_represented(id, None);
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity.passively_acquired_target = false;
    }
}

// ===== Why there is no building Guard->Attack mission flip here =====
//
// In the original, a Guard-mission building that holds a target commits
// Mission_Attack, and that mission is NOT a latch: it re-derives an action from
// the live target on every dispatch through an action jumptable, and when the
// target pointer goes null it clears the target, re-assigns Guard and commences
// — read out of the original's building Attack-mission handler this session,
// whose null-target arm is exactly assign-target-null, assign-mission-Guard,
// commence. The flip is safe there because the Attack mission owns the
// re-evaluation.
//
// VERA has no building Mission_Attack handler, and firing here does not read
// the mission at all: the fire gate and the attacker snapshot never look at it,
// so a structure holding a target fires whatever mission it is on. So writing
// Attack would buy exactly zero firing while costing the rescan — the passive
// gate only admits {Move, Harvest, Guard}, and nothing in VERA would ever move
// the building back off Attack, because combat deliberately does not clear a
// target that has merely gone out of range and buildings are excluded from
// pursuit. A Tesla Coil that acquired a scout at 6 cells would stay locked on it
// after it backed off to 8 and stayed in vision — silent for the rest of the
// match, for near-certain in the first minutes of any game.
//
// RESIDUAL: the flip is omitted, so a building stays on the bridged Guard
// reading and the passive scan owns its target: it keeps a scanner target until
// GetFireError answers ILLEGAL, CANT or RANGE, then picks again
// (`target_scan`). Restoring the flip requires a real building Mission_Attack
// handler first.

// ===== P2 (factory substrate) — Structure-arm read-only shadow trace (FIT a) =====
//
// FIT option (a): the per-(house, category) factory step is driven from the
// Structure arm of object_ai_stage() in LogicVector order; the FactoryRegistry is
// a LOOKUP, not a tick-loop owner. In P1+P2 there is no authoritative step, so the
// `EntityCategory::Structure` arm stays a no-op and this debug-only trace records
// each live Structure in LogicVector order — the same "proof lives beside, not
// inside, the no-op arm" shape as the S1 shadow. The order-follows-LogicVector
// property is proven by a test that injects a known non-sorted order
// (`factory_shadow_trace_order_matches_logic_vector`); the runtime debug_assert
// only checks the cheap intrinsic invariants (strictly-increasing visit ordinal;
// each traced id resolves to a live, non-dying Structure). Read-only, never hashed.

/// One Structure visited by the P2 factory shell trace, in LogicVector order.
#[cfg(any(test, debug_assertions))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FactoryShellTrace {
    structure_id: u64,
    visit_seq: u32,
}

impl Simulation {
    /// Build the P2 factory shell trace: each live, non-dying Structure in
    /// LogicVector order. Read-only; never hashed, never serialized. The order IS
    /// LogicVector order by construction (it walks `live_object_order_snapshot`) —
    /// the FIT-(a) ordering, exercised by the injected-order test.
    #[cfg(any(test, debug_assertions))]
    fn factory_shell_trace(&self) -> Vec<FactoryShellTrace> {
        let mut seq = 0u32;
        let mut traces: Vec<FactoryShellTrace> = Vec::new();
        for id in self.live_object_order_snapshot() {
            let is_live_structure = self
                .substrate
                .entities
                .get(id)
                .is_some_and(|e| !e.dying && e.category == EntityCategory::Structure);
            if !is_live_structure {
                continue;
            }
            traces.push(FactoryShellTrace {
                structure_id: id,
                visit_seq: seq,
            });
            seq += 1;
        }
        traces
    }

    /// Test-only accessor: the structure ids the P2 trace visits, in order. The
    /// test injects a non-sorted live order and asserts this equals it (so it
    /// would fail if the trace used BTreeMap/entity-id order instead).
    #[cfg(test)]
    pub(crate) fn factory_shell_trace_order(&self) -> Vec<u64> {
        self.factory_shell_trace()
            .iter()
            .map(|t| t.structure_id)
            .collect()
    }

    /// Debug-only P2 assert: the factory shell trace visits live, non-dying
    /// Structures with a strictly-increasing visit ordinal. INTRINSIC invariants
    /// only — not a self-comparison; the LogicVector-order property is proven by a
    /// dedicated injected-order test, never re-derived here.
    #[cfg(any(test, debug_assertions))]
    pub(crate) fn debug_assert_factory_shell_trace(&self) {
        let traces = self.factory_shell_trace();
        for w in traces.windows(2) {
            debug_assert!(
                w[0].visit_seq < w[1].visit_seq,
                "P2: tick {}: factory shell trace visit_seq must strictly increase",
                self.session.tick,
            );
        }
        for t in &traces {
            debug_assert!(
                self.substrate
                    .entities
                    .get(t.structure_id)
                    .is_some_and(|e| !e.dying && e.category == EntityCategory::Structure),
                "P2: tick {}: factory shell trace id {} must resolve to a live Structure",
                self.session.tick,
                t.structure_id,
            );
        }
    }

    /// Test-only P3 oracle probe: walk live Structures in LogicVector order and, for
    /// each, step a CLONE of its owner's factories against a CLONE of the owner's
    /// economy — exercising `set_rate` + `advance_one_step` on throwaways. READ-ONLY
    /// w.r.t. all hashed state: it writes only local clones, NEVER the registry, the
    /// wallet, or any entity. The `EntityCategory::Structure` arm stays a no-op; this
    /// is the "proof beside the no-op" shape (FIT option a) and the P5 precursor (the
    /// flip swaps the arm body, not the iteration source). The full per-building
    /// Primary_For* routing is a later slice — the probe uses a bounded per-owner
    /// scope (every factory the visited Structure's owner holds), hash-neutral
    /// regardless of routing precision.
    #[cfg(test)]
    pub(crate) fn factory_oracle_step_trace(&self) -> Vec<(u64, StepOutcome)> {
        let mut out: Vec<(u64, StepOutcome)> = Vec::new();
        for id in self.live_object_order_snapshot() {
            let Some(entity) = self.substrate.entities.get(id) else {
                continue;
            };
            if entity.dying || entity.category != EntityCategory::Structure {
                continue;
            }
            let owner = entity.owner;
            // Clone the owner's economy (the oracle wallet); default if no house.
            let mut oracle_econ = self
                .houses
                .get(&owner)
                .map(|h| h.economy.clone())
                .unwrap_or_default();
            // Bounded scope: step a CLONE of each of this owner's factories. The
            // registry is a LOOKUP (FIT a); we read it, never mutate it.
            for factory in self.production.factory_shadow.iter_insertion_ordered() {
                if factory.owner != owner || factory.object.is_none() {
                    continue;
                }
                let mut oracle_factory = factory.clone();
                // Exercise SetRate (build-step total is a placeholder until the
                // GetBuildStepTime pipeline lands; original_balance is a stand-in
                // input — the probe proves the step machine runs, not the rate value).
                oracle_factory.set_rate(oracle_factory.original_balance);
                let outcome = oracle_factory.advance_one_step(&mut oracle_econ);
                out.push((id, outcome));
                // local clones dropped here; nothing written back.
            }
        }
        out
    }

    /// Test-only dormant probe (P5a): prove the C7 delivery -> start_next_queued
    /// mechanics on a CLONE of the registry (NEVER the hashed shadow). Returns, per
    /// factory, (owner, category, popped-front-after-a-simulated-delivery). NO
    /// authoritative call site — a later slice binds start_next_queued to the real
    /// delivery commit; this only proves the post-delivery pop end-to-end.
    #[cfg(test)]
    pub(crate) fn factory_delivery_probe(
        &self,
    ) -> Vec<(
        crate::sim::intern::InternedId,
        crate::sim::production::ProductionCategory,
        Option<crate::sim::intern::InternedId>,
    )> {
        let mut out = Vec::new();
        for factory in self.production.factory_shadow.iter_insertion_ordered() {
            let mut d = factory.clone();
            d.object = None; // simulate the delivery commit
            d.suspended = false;
            let popped = d.start_next_queued(0, 0);
            out.push((factory.owner, factory.category, popped));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::tube_facts::TubeId;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::sim::aircraft::AircraftMission;
    use crate::sim::combat::{AttackTarget, TargetKind};
    use crate::sim::components::{
        DriveCoord, DriveLocomotionRuntime, MovementTarget, NavTargetRef,
    };
    use crate::sim::docking::building_dock::{DockPhase, DockState};
    use crate::sim::game_entity::{BunkerLink, GameEntity};
    use crate::sim::miner::{Miner, MinerConfig, MinerKind};
    use crate::sim::mission::leaf::MissionLeafState;
    use crate::sim::mission::state::MissionTestFixture;
    use crate::sim::mission::{
        MissionCom, MissionControl, MissionDispatchTimer, MissionId, MissionType,
    };
    use crate::sim::movement::locomotion::LocomotorSlot;
    use crate::sim::movement::locomotor::{LocomotorState, MovementLayer};
    use crate::sim::movement::tube_movement::LowBridgeTubeMovementState;
    use crate::sim::rng::SimRngLogicalState;
    use crate::sim::snapshot::GameSnapshot;
    use crate::sim::world::SimulationRngState;

    /// Build a test entity of a specific category (`test_default` makes a Unit).
    fn entity_of(id: u64, category: EntityCategory) -> GameEntity {
        let mut e = GameEntity::test_default(id, "TEST", "Americans", 5, 5);
        e.category = category;
        e.mission_leaf = crate::sim::mission::leaf::MissionLeafState::for_entity_category(category);
        e
    }

    fn register_entity(sim: &mut Simulation, mut entity: GameEntity) {
        entity.owner = sim.interner.intern("Americans");
        entity.type_ref = sim.interner.intern("TEST");
        sim.substrate.entities.insert(entity);
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

    #[test]
    fn object_ai_stage_ticks_every_live_object_counter() {
        // Post-flip: every live, non-dying object gets its `+0xC4` AI-counter
        // tick at the host; `current` is verb-owned and stays untouched (no
        // command queued anything here).
        let mut sim = Simulation::new();
        sim.substrate
            .entities
            .insert(entity_of(1, EntityCategory::Unit));
        sim.substrate
            .entities
            .insert(entity_of(2, EntityCategory::Infantry));
        sim.substrate
            .entities
            .insert(entity_of(3, EntityCategory::Structure));
        sim.substrate
            .entities
            .insert(entity_of(4, EntityCategory::Aircraft));
        sim.set_logic_order_for_test(vec![1, 2, 3, 4]);

        sim.object_ai_stage(None);

        for id in [1u64, 2, 3, 4] {
            let e = sim.substrate.entities.get(id).unwrap();
            assert_eq!(
                e.mission.ai_counter(),
                1,
                "live object {id} gets exactly one counter tick per stage pass"
            );
            assert_eq!(
                e.mission.current(),
                MissionId::NONE,
                "no verb ran, so object {id}'s current selector stays none"
            );
        }
    }

    #[test]
    fn techno_ai_shell_membership_matches_phase_snapshot() {
        let mut sim = Simulation::new();
        sim.substrate
            .entities
            .insert(entity_of(1, EntityCategory::Unit));
        sim.substrate
            .entities
            .insert(entity_of(2, EntityCategory::Structure));
        sim.substrate
            .entities
            .insert(entity_of(3, EntityCategory::Aircraft));
        // Deliberately NON-sorted order to prove the walk preserves live order
        // verbatim (no sort).
        sim.set_logic_order_for_test(vec![3, 1, 2]);

        let visited = sim.object_ai_walk(true, None, ObjectAiCtx::default());
        assert_eq!(
            visited,
            sim.live_object_order_snapshot(),
            "every live object visited exactly once, in live order"
        );
        assert_eq!(
            visited,
            vec![3, 1, 2],
            "live order preserved verbatim (no sort)"
        );
    }

    #[test]
    fn techno_ai_shell_preserves_advance_tick_phase_order() {
        // The stage is wired into advance_tick (called every tick, before
        // refresh_mission_shadow). Identical fixtures must produce identical
        // per-tick state_hash sequences — the stage introduces no nondeterminism
        // and no panic. Together with the commit proof
        // (object_ai_stage_commits_live_unit_mission, which exercises the entity
        // walk directly) this shows the stage perturbs no phase and no surrounding
        // ordering beyond its own mission commit. The fixture is intentionally entity-free:
        // raw test_default entities carry interned ids that advance_tick's
        // entity systems would resolve against an empty interner (a fixture
        // concern unrelated to the stage); the stage still runs each tick over
        // the empty live order.
        fn run() -> Vec<u64> {
            let mut sim = Simulation::new();
            let heights = std::collections::BTreeMap::new();
            (0..5)
                .map(|_| {
                    sim.advance_tick(&[], None, &heights, None, None, 67);
                    sim.state_hash()
                })
                .collect()
        }
        assert_eq!(
            run(),
            run(),
            "advance_tick with the object-AI stage stays deterministic"
        );
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
        sim.substrate.entities.get_mut(2).unwrap().dying = true;

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

    #[test]
    fn object_ai_stage_tolerates_absent_id_in_order() {
        let mut sim = Simulation::new();
        let live_id = 1u64;
        let absent_id = 999u64;
        sim.substrate
            .entities
            .insert(entity_of(live_id, EntityCategory::Unit));
        // Force the live order to include an id with no entity in the store
        // (set_logic_order_for_test only flips flags on existing ids, so set the
        // order directly to keep the absent id a non-member with no entity).
        sim.substrate
            .logic
            .set_order_for_test(vec![absent_id, live_id]);

        let visited = sim.object_ai_walk(true, None, ObjectAiCtx::default());
        assert_eq!(
            visited,
            vec![live_id],
            "absent id skipped without panic; live id still visited"
        );
        // Stage must not panic on the absent member either.
        sim.object_ai_stage(None);
    }

    // ===== TechnoClass common bracket =====

    #[test]
    fn bracket_ticks_live_unit_counter_without_touching_current() {
        let mut sim = Simulation::new();
        sim.substrate
            .entities
            .insert(entity_of(1, EntityCategory::Unit));
        // A live Unit reaches the dispatch point: +0xC4 counter tick; the
        // verb-owned current selector is untouched.
        assert_eq!(
            unit_techno_bracket(&mut sim, 1, None, ObjectAiCtx::default()),
            BracketReach::Dispatched
        );
        let u = sim.substrate.entities.get(1).unwrap();
        assert_eq!(u.mission.ai_counter(), 1);
        assert_eq!(u.mission.current(), MissionId::NONE);
    }

    #[test]
    fn bracket_pre_guard_short_circuits_dead_unit() {
        let mut sim = Simulation::new();
        let mut e = entity_of(1, EntityCategory::Unit);
        e.health.current = 0; // not alive
        sim.substrate.entities.insert(e);
        // Guard B fires after the (empty) pre-block: a health-0 Unit runs no
        // mission work (counter stays 0).
        assert_eq!(
            unit_techno_bracket(&mut sim, 1, None, ObjectAiCtx::default()),
            BracketReach::DiedInPre
        );
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().mission.ai_counter(),
            0
        );
    }

    #[test]
    fn bracket_ticks_miner_counter_like_any_unit() {
        let mut sim = Simulation::new();
        let mut miner = entity_of(1, EntityCategory::Unit);
        miner.miner = Some(Miner::new(MinerKind::War, &MinerConfig::default(), 0));
        sim.substrate.entities.insert(miner);
        // Post-flip there is no miner deferral: the bracket runs the same
        // common mission step for every live Unit (the miner FSM keeps driving
        // behavior; its mission commits arrive through the departure verbs).
        assert_eq!(
            unit_techno_bracket(&mut sim, 1, None, ObjectAiCtx::default()),
            BracketReach::Dispatched
        );
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().mission.ai_counter(),
            1
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
            veterancy: 0,
            high: false,
            mission: None,
            recruitable_a: true,
            recruitable_b: true,
            structure_upgrades: [None, None, None],
        }
    }

    /// Two hostile vehicles parked three cells apart, neither of them given any
    /// order. Runs `ticks` real ticks through `advance_tick` and returns the
    /// sim. Entity 1 is the Allied vehicle, entity 2 the Soviet one.
    ///
    /// Nothing in this fixture issues a command, assigns a target, or deals
    /// damage on purpose, so the ONLY way a target can appear on a unit whose
    /// enemy never shoots is the passive scanner.
    fn run_idle_pair(allied_type: &str, soviet_type: &str, ticks: u64) -> Simulation {
        let rules = passive_rules();
        let heights: std::collections::BTreeMap<(u16, u16), u8> = std::collections::BTreeMap::new();
        let grid = crate::sim::pathfinding::PathGrid::new(64, 64);
        let mut sim = Simulation::with_seed(0x5CA1_AB1E_0001);
        sim.spawn_from_map(
            &[
                passive_map_entity("Americans", allied_type, 20, 20, EntityCategory::Unit),
                passive_map_entity("Soviet", soviet_type, 23, 20, EntityCategory::Unit),
            ],
            Some(&rules),
            &heights,
        );
        for _ in 0..ticks {
            let _ = sim.advance_tick(&[], Some(&rules), &heights, Some(&grid), None, 67);
        }
        sim
    }

    /// `FootClass::Mission_Guard 0x004D52A9` in production: a guarding tank
    /// that has fired is next dispatched when its reload runs out, not on
    /// `[Guard] Rate` plus a draw. The 50-frame `105mm` reload spans several
    /// 14-frame Guard dispatches.
    #[test]
    fn a_guarding_tank_that_fired_waits_out_its_reload() {
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
        .expect("guard reload rules parse");
        let heights: std::collections::BTreeMap<(u16, u16), u8> = std::collections::BTreeMap::new();
        let grid = crate::sim::pathfinding::PathGrid::new(64, 64);
        let mut sim = Simulation::with_seed(0x5CA1_AB1E_0009);
        let tank = crate::map::entities::MapEntity {
            mission: Some(MissionType::Guard),
            ..passive_map_entity("Americans", "MTNK", 20, 20, EntityCategory::Unit)
        };
        sim.spawn_from_map(
            &[
                tank,
                passive_map_entity("Soviet", "UNARM", 23, 20, EntityCategory::Unit),
            ],
            Some(&rules),
            &heights,
        );
        let mut waits = 0;
        let mut last_dispatch = None;
        for _ in 0..400 {
            let _ = sim.advance_tick(&[], Some(&rules), &heights, Some(&grid), None, 67);
            let tank = sim.substrate.entities.get(1).expect("tank present");
            assert_eq!(tank.mission.current().known(), Some(MissionType::Guard));
            let timer = tank.mission.dispatch_timer();
            if last_dispatch == Some(timer) {
                continue;
            }
            last_dispatch = Some(timer);
            // A dispatch after the last shot's rearm was armed saw this timer.
            let rearm = tank.rearm_timer;
            if rearm.duration() > 0
                && rearm.start_frame() < timer.start_frame()
                && rearm.remaining(timer.start_frame()) != 0
            {
                assert_eq!(
                    timer.delay(),
                    rearm.remaining(timer.start_frame()),
                    "dispatch at {}",
                    timer.start_frame()
                );
                waits += 1;
            }
        }
        assert!(
            waits >= 3,
            "precondition: dispatches landed inside reloads ({waits})"
        );
    }

    #[test]
    fn idle_guard_unit_acquires_a_target_with_no_order_at_all() {
        // The headline behavior: a parked tank opens fire on an enemy that is
        // simply standing in range. No order, no damage taken, no attack-move.
        // The target is unarmed so receiver-synchronous retaliation cannot
        // replace the scanner-owned bookkeeping under test here.
        let sim = run_idle_pair("MTNK", "UNARM", 90);
        let allied = sim.substrate.entities.get(1).expect("allied tank present");
        assert!(
            allied.attack_target.is_some(),
            "an idle Guard-mission unit must passively acquire a hostile in range"
        );
        assert!(
            allied.passively_acquired_target,
            "the target must be flagged as scanner-acquired (it gates the drop/clear blocks)"
        );
    }

    #[test]
    fn techno_playfield_stored_membership_gates_same_frame_passive_targeting() {
        let rules = passive_rules();
        let heights = std::collections::BTreeMap::new();
        let grid = crate::sim::pathfinding::PathGrid::new(64, 64);
        let mut sim = Simulation::with_seed(0x3D5);
        sim.spawn_from_map(
            &[
                passive_map_entity("Americans", "MTNK", 20, 20, EntityCategory::Unit),
                passive_map_entity("Soviet", "UNARM", 23, 20, EntityCategory::Unit),
            ],
            Some(&rules),
            &heights,
        );
        sim.playfield_bounds = Some(
            crate::map::playfield::PlayfieldBounds::from_normalized_local_size(64, 2, 2, 56, 52),
        );
        sim.substrate.entities.get_mut(1).unwrap().in_playfield = true;
        sim.substrate.entities.get_mut(2).unwrap().in_playfield = false;

        for _ in 0..90 {
            let _ = sim.advance_tick(&[], Some(&rules), &heights, Some(&grid), None, 67);
        }
        assert!(
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .attack_target
                .is_none()
        );

        sim.substrate.entities.get_mut(2).unwrap().in_playfield = true;
        for _ in 0..90 {
            let _ = sim.advance_tick(&[], Some(&rules), &heights, Some(&grid), None, 67);
        }
        assert!(
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .attack_target
                .is_some(),
            "Evaluate_Candidate @ 0x006F7DB0 admits the stored true member"
        );
    }

    #[test]
    fn a_unit_that_passively_acquires_does_not_move_toward_the_target() {
        // Pursuit regression guard. The passive commit writes the target
        // pointer only; a Guard unit fires from where it stands. If pursuit
        // ever picks these units up again they walk off across the map with no
        // OrderIntent to bring them home.
        //
        // A target can only be acquired IN range, so the scenario has to open a
        // range gap afterwards: the unarmed Allied scout parks two cells away
        // until the Soviet tank picks it up, then drives off. That is exactly
        // the "an enemy scouted past my base" case. The scout is unarmed, so it
        // never shoots and no retaliation can install a target another way.
        let rules = passive_rules();
        let heights: std::collections::BTreeMap<(u16, u16), u8> = std::collections::BTreeMap::new();
        let grid = crate::sim::pathfinding::PathGrid::new(64, 64);
        let mut sim = Simulation::with_seed(0x5CA1_AB1E_0002);
        sim.spawn_from_map(
            &[
                passive_map_entity("Americans", "UNARM", 22, 20, EntityCategory::Unit),
                passive_map_entity("Soviet", "MTNK", 20, 20, EntityCategory::Unit),
            ],
            Some(&rules),
            &heights,
        );
        let allied = sim.interner.get("Americans").expect("Americans interned");
        let start = sim
            .substrate
            .entities
            .get(2)
            .map(|e| (e.position.rx, e.position.ry))
            .expect("soviet tank present");

        // Let the Soviet tank acquire the parked scout.
        for _ in 0..60 {
            let _ = sim.advance_tick(&[], Some(&rules), &heights, Some(&grid), None, 67);
        }
        let soviet = sim.substrate.entities.get(2).expect("soviet tank present");
        assert!(
            soviet.attack_target.is_some() && soviet.passively_acquired_target,
            "precondition: the idle tank must have picked the scout up on its own"
        );

        // Now the scout runs. The tank must not follow it.
        let scram = crate::sim::command::CommandEnvelope::new(
            allied,
            61,
            crate::sim::command::Command::Move {
                entity_id: 1,
                target_rx: 55,
                target_ry: 20,
                queue: false,
                group_id: None,
            },
        );
        for tick in 60..260u64 {
            let due: Vec<crate::sim::command::CommandEnvelope> = if tick + 1 == 61 {
                vec![scram.clone()]
            } else {
                Vec::new()
            };
            let _ = sim.advance_tick(&due, Some(&rules), &heights, Some(&grid), None, 67);
        }
        let scout = sim.substrate.entities.get(1).expect("scout present");
        assert!(
            scout.position.rx > 30,
            "precondition: the scout actually ran out of the tank's weapon range"
        );
        let soviet = sim.substrate.entities.get(2).expect("soviet tank present");
        assert_eq!(
            (soviet.position.rx, soviet.position.ry),
            start,
            "a passively-acquired target must never be pursued — the unit holds its ground"
        );
        assert!(
            soviet.movement_target.is_none(),
            "no movement may be issued for a scanner-acquired target"
        );
    }

    #[test]
    fn passive_scan_keeps_rescanning_on_cadence_instead_of_latching() {
        // The scanner must not go dormant after one acquisition. With a target
        // installed by the scanner itself the object stays on Guard, so every
        // cadence expiry draws again and re-picks. Latching would leave the
        // timer expired forever and re-acquire with zero delay after combat
        // cleared the target.
        let rules = passive_rules();
        let mut sim = Simulation::new();
        insert_scannable(&mut sim, 1, "Americans", "MTNK", EntityCategory::Unit);

        // First scan: no hostile, so nothing is installed but the draw happens.
        let before = sim.scenario_rng.state();
        passive_acquire_step(&mut sim, 1, Some(&rules), ObjectAiCtx::default());
        assert_ne!(sim.scenario_rng.state(), before);

        // Pretend the scan had found something.
        {
            let e = sim.substrate.entities.get_mut(1).unwrap();
            e.attack_target = Some(AttackTarget::new(9));
            e.passively_acquired_target = true;
            e.passive_scan_timer.clear(); // cadence expired again
        }
        assert_eq!(
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .passive_acquire_mission(),
            MissionType::Guard,
            "a scanner-installed target must not move the object off Guard"
        );

        let armed = sim.scenario_rng.state();
        passive_acquire_step(&mut sim, 1, Some(&rules), ObjectAiCtx::default());
        assert_ne!(
            sim.scenario_rng.state(),
            armed,
            "the object must still be scanning on cadence with a target installed"
        );
        assert!(
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .passive_scan_timer
                .is_armed(),
            "the cadence timer is re-armed by the rescan, not left expired"
        );
    }

    #[test]
    fn rescan_that_repicks_the_same_target_does_not_reset_the_weapon_cooldown() {
        // The reload is the object's own timer; a re-pick of the same target
        // must not touch it.
        let rules = passive_rules();
        let heights: std::collections::BTreeMap<(u16, u16), u8> = std::collections::BTreeMap::new();
        let grid = crate::sim::pathfinding::PathGrid::new(64, 64);
        let mut sim = Simulation::with_seed(0x5CA1_AB1E_0003);
        // Spawn through the real path so vision is established; the only
        // candidate is the unarmed Soviet vehicle two cells away.
        sim.spawn_from_map(
            &[
                passive_map_entity("Americans", "MTNK", 20, 20, EntityCategory::Unit),
                passive_map_entity("Soviet", "UNARM", 22, 20, EntityCategory::Unit),
            ],
            Some(&rules),
            &heights,
        );
        let _ = sim.advance_tick(&[], Some(&rules), &heights, Some(&grid), None, 67);
        let rearm = crate::sim::timer::CdTimer::started(sim.session.binary_frame as i32, 40);
        {
            let e = sim.substrate.entities.get_mut(1).unwrap();
            e.attack_target = Some(AttackTarget::new(2));
            e.rearm_timer = rearm;
            e.passively_acquired_target = true;
            e.passive_scan_timer.clear();
        }
        passive_acquire_step(&mut sim, 1, Some(&rules), ObjectAiCtx::default());
        let e = sim.substrate.entities.get(1).unwrap();
        let attack = e.attack_target.as_ref().expect("target retained");
        assert_eq!(attack.target, TargetKind::Entity(2));
        assert_eq!(
            e.rearm_timer, rearm,
            "re-picking the same target must leave the ROF cooldown untouched"
        );
    }

    #[test]
    fn a_scanner_target_is_kept_when_a_closer_enemy_arrives() {
        // `Retaliate_And_Scan` drops a scanner target only on ILLEGAL, CANT or
        // RANGE (`0x007098EC..0x0070990A`); a nearer enemy is no reason. The
        // tank keeps the farther victim, its reload untouched.
        let rules = passive_rules();
        let heights: std::collections::BTreeMap<(u16, u16), u8> = std::collections::BTreeMap::new();
        let grid = crate::sim::pathfinding::PathGrid::new(64, 64);
        let mut sim = Simulation::with_seed(0x5CA1_AB1E_0006);
        sim.spawn_from_map(
            &[
                passive_map_entity("Americans", "MTNK", 20, 20, EntityCategory::Unit),
                passive_map_entity("Soviet", "UNARM", 24, 20, EntityCategory::Unit),
                passive_map_entity("Soviet", "UNARM", 21, 20, EntityCategory::Unit),
            ],
            Some(&rules),
            &heights,
        );
        let _ = sim.advance_tick(&[], Some(&rules), &heights, Some(&grid), None, 67);
        let rearm = crate::sim::timer::CdTimer::started(sim.session.binary_frame as i32, 40);
        {
            let e = sim.substrate.entities.get_mut(1).unwrap();
            e.attack_target = Some(AttackTarget::new(2));
            e.rearm_timer = rearm;
            e.passively_acquired_target = true;
            e.passive_scan_timer.clear();
        }
        let before = sim.scenario_rng.state();
        passive_acquire_step(&mut sim, 1, Some(&rules), ObjectAiCtx::default());
        assert_ne!(sim.scenario_rng.state(), before, "the scan still draws");
        let e = sim.substrate.entities.get(1).unwrap();
        assert_eq!(
            e.attack_target.as_ref().map(|attack| attack.target),
            Some(TargetKind::Entity(2)),
            "the victim in range is kept"
        );
        assert!(e.passively_acquired_target);
        assert_eq!(e.rearm_timer, rearm);
    }

    #[test]
    fn after_a_kill_the_next_passive_scan_picks_again() {
        // Native has no retarget on a kill: pointer expiry clears the killer's
        // target (`0x007077C0`) and the next passive scan picks again, marking
        // the new target as its own (`0x006FA6EE`). The invariant is checked
        // every tick: a target the tank was never ordered onto is always
        // scanner-owned, or pursuit would chase it.
        let rules = passive_rules();
        let heights: std::collections::BTreeMap<(u16, u16), u8> = std::collections::BTreeMap::new();
        let grid = crate::sim::pathfinding::PathGrid::new(64, 64);
        let mut sim = Simulation::with_seed(0x5CA1_AB1E_0007);
        sim.spawn_from_map(
            &[
                passive_map_entity("Americans", "MTNK", 20, 20, EntityCategory::Unit),
                passive_map_entity("Soviet", "UNARM", 21, 20, EntityCategory::Unit),
                passive_map_entity("Soviet", "UNARM", 23, 20, EntityCategory::Unit),
            ],
            Some(&rules),
            &heights,
        );
        for _ in 0..60 {
            let _ = sim.advance_tick(&[], Some(&rules), &heights, Some(&grid), None, 67);
        }
        assert_eq!(
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .attack_target
                .as_ref()
                .map(|t| t.target),
            Some(TargetKind::Entity(2)),
            "precondition: the scanner picked the nearer candidate"
        );

        // Put the current victim one hit from death and let combat finish it.
        // The invariant is checked EVERY tick, not just at the end: the leak
        // opens on the exact tick the victim dies and can be papered over by the
        // next cadence rescan, so an end-state assertion would miss it.
        sim.substrate.entities.get_mut(2).unwrap().health.current = 1;
        for tick in 0..80 {
            let _ = sim.advance_tick(&[], Some(&rules), &heights, Some(&grid), None, 67);
            let tank = sim.substrate.entities.get(1).expect("tank present");
            assert!(
                tank.attack_target.is_none() || tank.passively_acquired_target,
                "tick {tick}: the tank was never ordered to attack anything, so a target it \
                 holds must always be scanner-owned — a live target with the flag false takes \
                 it out of the passive block, into pursuit, and it never releases"
            );
        }
        assert!(
            sim.substrate
                .entities
                .get(2)
                .is_none_or(|e| e.health.current == 0 || e.dying),
            "precondition: the first victim died"
        );
        let tank = sim.substrate.entities.get(1).expect("tank present");
        assert!(
            tank.attack_target.is_some() && tank.passively_acquired_target,
            "precondition: the second candidate is still in range and was taken up"
        );
    }

    #[test]
    fn a_defence_actually_damages_what_it_passively_acquires() {
        // Structures carry gates units do not — the deploy animation and the
        // power check — so acquisition alone does not prove a defence shoots.
        let rules = passive_rules();
        let heights: std::collections::BTreeMap<(u16, u16), u8> = std::collections::BTreeMap::new();
        let grid = crate::sim::pathfinding::PathGrid::new(64, 64);
        let mut sim = Simulation::with_seed(0x5CA1_AB1E_0008);
        sim.spawn_from_map(
            &[
                passive_map_entity("Soviet", "UNARM", 22, 20, EntityCategory::Unit),
                passive_map_entity("Americans", "NASAM", 20, 20, EntityCategory::Structure),
                passive_map_entity("Americans", "GAPOWR", 14, 26, EntityCategory::Structure),
            ],
            Some(&rules),
            &heights,
        );
        for _ in 0..160 {
            let _ = sim.advance_tick(&[], Some(&rules), &heights, Some(&grid), None, 67);
        }
        let scout = sim.substrate.entities.get(1).expect("scout present");
        assert!(
            scout.health.current
                < rules
                    .object(sim.interner.resolve(scout.type_ref()))
                    .unwrap()
                    .strength,
            "an unordered base defence must actually shoot what it picks up: {}/{}",
            scout.health.current,
            rules
                .object(sim.interner.resolve(scout.type_ref()))
                .unwrap()
                .strength,
        );
    }

    #[test]
    fn a_unit_that_finished_a_move_order_still_acquires_and_shoots() {
        // The dominant case in real play: a unit is ordered somewhere, arrives,
        // and sits. Five Grizzlies sent to a chokepoint must not watch an enemy
        // drive past. Since the Move handler's arrival hook landed, the unit
        // gets there the way retail does — the hook drops the target, queues
        // Guard, and the host promotes it — rather than by the derived-reading
        // bridge that used to carry it while the selector stayed stuck on Move.
        let rules = passive_rules();
        let heights: std::collections::BTreeMap<(u16, u16), u8> = std::collections::BTreeMap::new();
        let grid = crate::sim::pathfinding::PathGrid::new(64, 64);
        let mut sim = Simulation::with_seed(0x5CA1_AB1E_0009);
        sim.spawn_from_map(
            &[
                passive_map_entity("Americans", "MTNK", 10, 20, EntityCategory::Unit),
                passive_map_entity("Soviet", "UNARM", 22, 20, EntityCategory::Unit),
            ],
            Some(&rules),
            &heights,
        );
        let allied = sim.interner.get("Americans").expect("Americans interned");
        // Drive to a cell three away from the parked enemy, then never touch it
        // again. The enemy is unarmed, so nothing can provoke a retaliation.
        let order = crate::sim::command::CommandEnvelope::new(
            allied,
            2,
            crate::sim::command::Command::Move {
                entity_id: 1,
                target_rx: 19,
                target_ry: 20,
                queue: false,
                group_id: None,
            },
        );
        for tick in 0..220u64 {
            let due: Vec<crate::sim::command::CommandEnvelope> = if tick + 1 == 2 {
                vec![order.clone()]
            } else {
                Vec::new()
            };
            let _ = sim.advance_tick(&due, Some(&rules), &heights, Some(&grid), None, 67);
        }

        let tank = sim.substrate.entities.get(1).expect("tank present");
        assert_eq!(
            tank.mission.current().known(),
            Some(MissionType::Guard),
            "the arrival hook committed Guard — the unit must not be left on Move"
        );
        assert!(
            tank.movement_target.is_none(),
            "precondition: the move actually finished"
        );
        assert_eq!(
            tank.passive_acquire_mission(),
            MissionType::Guard,
            "a finished order must not leave the unit stuck outside the gate"
        );
        assert!(
            tank.attack_target.is_some() && tank.passively_acquired_target,
            "an ordered-then-idle unit must acquire like any other idle unit"
        );
        let enemy = sim.substrate.entities.get(2).expect("enemy present");
        assert!(
            enemy.health.current
                < rules
                    .object(sim.interner.resolve(enemy.type_ref()))
                    .unwrap()
                    .strength,
            "and it must actually open fire: {}/{}",
            enemy.health.current,
            rules
                .object(sim.interner.resolve(enemy.type_ref()))
                .unwrap()
                .strength,
        );
    }

    #[test]
    fn a_stopped_unit_is_not_deafened_by_the_stop_mission() {
        // Stop(13) is one of the twelve missions that strip a scanner target, so
        // a literal read of the committed selector would make pressing S both
        // silence the unit and take away what it had found.
        let mut e = GameEntity::test_default(1, "MTNK", "Americans", 5, 5);
        e.mission
            .apply_test_fixture(fixture_with_current(&e.mission, MissionType::Stop));
        assert_eq!(
            e.passive_acquire_mission(),
            MissionType::Guard,
            "a finished Stop reads as Guard, not as the target-stripping Stop mission"
        );
        // While the order is still live, the committed selector wins.
        e.movement_target = Some(MovementTarget::default());
        assert_eq!(e.passive_acquire_mission(), MissionType::Stop);
    }

    fn fixture_with_current(mission: &MissionCom, current: MissionType) -> MissionTestFixture {
        let mut fixture = mission_test_fixture(mission);
        fixture.current = MissionId::from_known(current);
        fixture
    }

    #[test]
    fn a_passively_acquired_target_actually_gets_shot() {
        let rules = passive_rules();
        // Acquire AND fire. Everything else here stops at "a target is
        // installed"; this is the one that proves the round trip reaches damage.
        // Both tanks are armed and neither is ordered to do anything.
        let sim = run_idle_pair("MTNK", "MTNK", 160);
        let allied = sim.substrate.entities.get(1).expect("allied tank present");
        let soviet = sim.substrate.entities.get(2).expect("soviet tank present");
        assert!(
            allied.health.current
                < rules
                    .object(sim.interner.resolve(allied.type_ref()))
                    .unwrap()
                    .strength
                || soviet.health.current
                    < rules
                        .object(sim.interner.resolve(soviet.type_ref()))
                        .unwrap()
                        .strength,
            "a passively acquired target must actually be fired on: \
             allied {}/{}, soviet {}/{}",
            allied.health.current,
            rules
                .object(sim.interner.resolve(allied.type_ref()))
                .unwrap()
                .strength,
            soviet.health.current,
            rules
                .object(sim.interner.resolve(soviet.type_ref()))
                .unwrap()
                .strength,
        );
    }

    #[test]
    fn idle_infantry_acquires_a_target_with_no_order_at_all() {
        // Infantry reach the same block through the same foot leaf. A rifleman
        // standing next to an enemy must open fire on his own.
        let rules = passive_rules();
        let heights: std::collections::BTreeMap<(u16, u16), u8> = std::collections::BTreeMap::new();
        let grid = crate::sim::pathfinding::PathGrid::new(64, 64);
        let mut sim = Simulation::with_seed(0x5CA1_AB1E_0004);
        sim.spawn_from_map(
            &[
                passive_map_entity("Americans", "UNARM", 22, 20, EntityCategory::Unit),
                passive_map_entity("Soviet", "GI", 20, 20, EntityCategory::Infantry),
            ],
            Some(&rules),
            &heights,
        );
        for _ in 0..90 {
            let _ = sim.advance_tick(&[], Some(&rules), &heights, Some(&grid), None, 67);
        }
        let rifleman = sim.substrate.entities.get(2).expect("infantry present");
        assert!(
            rifleman.attack_target.is_some() && rifleman.passively_acquired_target,
            "idle infantry must passively acquire a hostile in range"
        );
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
        let heights: std::collections::BTreeMap<(u16, u16), u8> = std::collections::BTreeMap::new();
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
            &heights,
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
            "`holds_until_retasked` must let the authored selector beat the derived Guard"
        );
        for _ in 0..90 {
            let _ = sim.advance_tick(&[], Some(&rules), &heights, Some(&grid), None, 67);
        }
        let sticky_unit = sim.substrate.entities.get(2).expect("infantry present");
        assert_eq!(
            sticky_unit.mission.current().known(),
            Some(MissionType::Sticky),
            "nothing retasked it, so it must still be on Sticky"
        );
        assert!(
            sticky_unit.attack_target.is_none(),
            "Sticky is outside the passive-acquire gate's three admitted missions"
        );
    }

    #[test]
    fn can_passive_aquire_no_type_never_acquires() {
        // The enemy is UNARMED in both runs, so it can neither acquire nor
        // shoot — retaliation cannot muddy the result and a target on the
        // vehicle under test can only have come from its own scanner.
        let control = run_idle_pair("UNARM", "MTNK", 90);
        assert!(
            control
                .substrate
                .entities
                .get(2)
                .expect("soviet vehicle present")
                .attack_target
                .is_some(),
            "control: an ordinary type acquires the unarmed enemy in this fixture"
        );

        let opted_out = run_idle_pair("UNARM", "NOACQ", 90);
        let soviet = opted_out
            .substrate
            .entities
            .get(2)
            .expect("soviet vehicle present");
        assert!(
            soviet.attack_target.is_none(),
            "CanPassiveAquire=no must keep the type out of the scanner entirely"
        );
        assert!(!soviet.passively_acquired_target);
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
        let owner_ref = sim.interner.intern(owner);
        let type_ref = sim.interner.intern(type_id);
        let mut e = GameEntity::new_at_frame_zero_for_test(
            id,
            5,
            5,
            0,
            0,
            owner_ref,
            crate::sim::components::Health { current: 100 },
            type_ref,
            category,
            0,
            5,
            true,
        );
        e.category = category;
        e.passive_scan_timer.clear(); // due now
        sim.substrate.entities.insert(e);
    }

    #[test]
    fn scan_rearms_the_timer_to_the_ini_delay_plus_the_draw() {
        // The cadence comes from [General], never from a constant in code: the
        // re-armed duration is NormalTargetingDelay plus the 0..=2 jitter, and
        // the anchor is the current frame.
        let rules = passive_rules();
        let mut sim = Simulation::new();
        insert_scannable(&mut sim, 1, "Americans", "MTNK", EntityCategory::Unit);

        let mut probe = sim.scenario_rng.clone();
        let expected_jitter = probe.next_range_u32_inclusive(0, 2);

        passive_acquire_step(&mut sim, 1, Some(&rules), ObjectAiCtx::default());

        let timer = sim.substrate.entities.get(1).unwrap().passive_scan_timer;
        assert_eq!(timer.start_frame, sim.session.binary_frame);
        assert_eq!(
            timer.duration,
            rules.general.normal_targeting_delay + expected_jitter,
            "re-armed duration must be the INI delay plus this scan's jitter draw"
        );
        assert_eq!(
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .last_target_scan_frame,
            sim.session.binary_frame
        );
    }

    #[test]
    fn scan_draws_exactly_one_scenario_value_and_moves_no_other_stream() {
        // Lockstep contract. Exactly one RandomRanged(0,2) per scan, on the
        // SCENARIO stream, and it is unconditional — this fixture has no
        // hostile at all, so no target is found and the draw still happens.
        let rules = passive_rules();
        let mut sim = Simulation::new();
        insert_scannable(&mut sim, 1, "Americans", "MTNK", EntityCategory::Unit);

        let mut expected_scenario = sim.scenario_rng.clone();
        expected_scenario.next_range_u32_inclusive(0, 2);
        let main_before = sim.main_rng.state();
        let mapgen_before = sim.mapgen_rng.state();

        passive_acquire_step(&mut sim, 1, Some(&rules), ObjectAiCtx::default());

        assert_eq!(
            sim.scenario_rng.state(),
            expected_scenario.state(),
            "the scan must consume EXACTLY one scenario draw"
        );
        assert_eq!(
            sim.main_rng.state(),
            main_before,
            "main stream must not move"
        );
        assert_eq!(
            sim.mapgen_rng.state(),
            mapgen_before,
            "mapgen stream must not move"
        );
        assert!(
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .attack_target
                .is_none(),
            "no hostile exists, so no target is installed — the draw still happened"
        );

        // A second call before the timer expires draws nothing at all.
        let after_first = sim.scenario_rng.state();
        passive_acquire_step(&mut sim, 1, Some(&rules), ObjectAiCtx::default());
        assert_eq!(
            sim.scenario_rng.state(),
            after_first,
            "the cadence timer gates the scan; a non-due object draws nothing"
        );
    }

    /// `TechnoClass::PassiveAcquireGate @ 0x00709290`, arm by arm, through
    /// the passive block. The scan's first act is its draw, so a draw means
    /// the gate passed.
    #[test]
    fn passive_gate_arms_follow_the_original() {
        let rules = passive_rules();
        let mut sim = Simulation::new();
        let mut scans = |sim: &mut Simulation, id: u64| {
            let before = sim.scenario_rng.state();
            passive_acquire_step(sim, id, Some(&rules), ObjectAiCtx::default());
            sim.scenario_rng.state() != before
        };
        let on_move = |sim: &mut Simulation, id: u64| {
            let entity = sim.substrate.entities.get_mut(id).unwrap();
            update_mission_test_fixture(&mut entity.mission, |fixture| {
                fixture.current = MissionId::from_known(MissionType::Move);
            });
            entity.navigation.nav_com =
                Some(crate::sim::components::NavTargetRef::Cell { rx: 30, ry: 30 });
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
        crate::sim::team_script_vm::join_one_member_team_for_test(&mut sim, 5, false, true);
        sim.substrate
            .entities
            .get_mut(5)
            .unwrap()
            .passive_scan_timer
            .clear();
        assert!(scans(&mut sim, 5));
        // The same arm is closed to a human house.
        insert_scannable(&mut sim, 6, "Americans", "NOACQ", EntityCategory::Unit);
        on_move(&mut sim, 6);
        let americans = sim.interner.intern("Americans");
        sim.houses.insert(
            americans,
            crate::sim::house_state::HouseState::new(americans, 0, None, true, 0, 10),
        );
        crate::sim::team_script_vm::join_one_member_team_for_test(&mut sim, 6, false, true);
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
        sim.houses.insert(
            soviet,
            crate::sim::house_state::HouseState::new(soviet, 0, None, false, 0, 10),
        );
        let aim = |sim: &mut Simulation| {
            sim.substrate.entities.get_mut(1).unwrap().attack_target = Some(AttackTarget::new(2));
        };
        let held = |sim: &Simulation| {
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .attack_target
                .is_some()
        };

        aim(&mut sim);
        sim.substrate.entities.get_mut(1).unwrap().berserk.active = true;
        allied_target_drop_step(&mut sim, 1, &rules);
        assert!(held(&sim), "a berserk object keeps it");

        sim.substrate.entities.get_mut(1).unwrap().berserk.active = false;
        allied_target_drop_step(&mut sim, 1, &rules);
        assert!(!held(&sim), "a computer house drops it");

        aim(&mut sim);
        sim.houses.get_mut(&soviet).unwrap().is_human = true;
        allied_target_drop_step(&mut sim, 1, &rules);
        assert!(held(&sim), "a human house keeps it");
    }

    /// `TechnoClass::Unlimbo @ 0x006F6E2A`: an infantryman leaving limbo with
    /// nowhere to go commits Guard at once, so the retaliation gate and the
    /// passive block read a truthful mission.
    #[test]
    fn a_produced_infantryman_enters_the_map_on_guard() {
        let rules = passive_rules();
        let heights: std::collections::BTreeMap<(u16, u16), u8> = std::collections::BTreeMap::new();
        let mut sim = Simulation::new();
        let id = sim
            .spawn_object("GI", "Americans", 10, 10, 0, &rules, &heights)
            .expect("GI spawns");
        let entity = sim.substrate.entities.get(id).unwrap();
        assert_eq!(
            entity.mission.current(),
            MissionId::from_known(MissionType::Guard)
        );
        assert_eq!(entity.mission.queued(), MissionId::NONE);
    }

    /// A Die1 corpse keeps running `TechnoClass::AI_Update` through
    /// `FootClass::AI` (`0x0051BC9F`): its passive block scans (one Scenario
    /// `RandomRanged(0, 2)`) on the visit that removes it.
    #[test]
    fn a_dying_infantryman_still_takes_its_passive_scan() {
        let rules = passive_rules();
        let mut sim = Simulation::new();
        insert_scannable(&mut sim, 1, "Americans", "GI", EntityCategory::Infantry);
        sim.substrate.entities.get_mut(1).unwrap().mission_leaf =
            crate::sim::mission::leaf::MissionLeafState::for_entity_category(
                EntityCategory::Infantry,
            );
        sim.begin_infantry_death_sequence(
            1,
            super::super::infantry_terminal::InfantryDeathSequence::Die1,
        );
        let mut expected = sim.clone_scenario_rng();
        let _ = expected.next_range_u32_inclusive(0, 2);
        sim.object_ai_visit_one(1, Some(&rules), ObjectAiCtx::default());
        assert_eq!(
            sim.scenario_rng.logical_state(),
            expected.logical_state(),
            "the corpse's scan drew once"
        );
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
        let owner = sim.interner.intern("Americans");
        sim.power_states.insert(
            owner,
            crate::sim::power_system::PowerState {
                total_drain: 100,
                is_low_power: true,
                ..Default::default()
            },
        );

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
    fn a_defence_releases_a_target_that_leaves_range_and_does_not_latch() {
        // The base-defence loop must keep re-evaluating. Nothing in VERA ever
        // moves a building off a mission, and combat deliberately keeps a target
        // that has only gone out of range, so if the defence ever left the
        // passive gate it would stay locked on one scout for the whole match and
        // stay silent through everything that came after.
        //
        // The gap must be range-only, not vision: combat already drops a target
        // whose cell stops being visible, so a scout that ran off the map edge
        // would prove nothing. [NASAM] has Range=6 and Sight=10, so the scout
        // backs off from 2 cells to 8 — outside the weapon, still plainly in
        // sight. Only the scanner's own re-evaluation can release it there.
        let rules = passive_rules();
        let heights: std::collections::BTreeMap<(u16, u16), u8> = std::collections::BTreeMap::new();
        let grid = crate::sim::pathfinding::PathGrid::new(64, 64);
        let mut sim = Simulation::with_seed(0x5CA1_AB1E_0005);
        sim.spawn_from_map(
            &[
                passive_map_entity("Soviet", "UNARM", 22, 20, EntityCategory::Unit),
                passive_map_entity("Americans", "NASAM", 20, 20, EntityCategory::Structure),
                // The defence drains power; without a plant the unpowered arm of
                // the can-acquire check would (correctly) keep it out entirely.
                passive_map_entity("Americans", "GAPOWR", 14, 26, EntityCategory::Structure),
            ],
            Some(&rules),
            &heights,
        );
        let soviet = sim.interner.get("Soviet").expect("Soviet interned");
        for _ in 0..60 {
            let _ = sim.advance_tick(&[], Some(&rules), &heights, Some(&grid), None, 67);
        }
        assert!(
            sim.substrate
                .entities
                .get(2)
                .expect("defence present")
                .attack_target
                .is_some(),
            "precondition: the defence acquires the scout parked in range"
        );

        let scram = crate::sim::command::CommandEnvelope::new(
            soviet,
            61,
            crate::sim::command::Command::Move {
                entity_id: 1,
                target_rx: 28,
                target_ry: 20,
                queue: false,
                group_id: None,
            },
        );
        for tick in 60..300u64 {
            let due: Vec<crate::sim::command::CommandEnvelope> = if tick + 1 == 61 {
                vec![scram.clone()]
            } else {
                Vec::new()
            };
            let _ = sim.advance_tick(&due, Some(&rules), &heights, Some(&grid), None, 67);
        }
        let scout = sim.substrate.entities.get(1).expect("scout present");
        assert!(
            scout.position.rx >= 27,
            "precondition: the scout actually left the defence's Range=6"
        );
        assert!(
            sim.fog.is_cell_visible(
                sim.interner.get("Americans").expect("Americans interned"),
                scout.position.rx,
                scout.position.ry,
            ),
            "precondition: the scout is still in sight, so only a rescan can release it"
        );
        let defence = sim.substrate.entities.get(2).expect("defence present");
        assert!(
            defence.attack_target.is_none(),
            "the defence must release a target that walked out of range, not latch onto it"
        );
        assert!(
            defence.passive_scan_timer.is_armed(),
            "and it must still be scanning on cadence, ready for the next contact"
        );
    }

    #[test]
    fn off_mission_clear_drops_a_scanner_target_but_not_an_ordered_one() {
        let mut sim = Simulation::new();
        insert_scannable(&mut sim, 1, "Americans", "MTNK", EntityCategory::Unit);
        let now = sim.session.binary_frame;
        // Unload is one of the twelve missions that drop a scanner target. The
        // job has to be LIVE for the committed selector to be read — a mission
        // whose machinery has gone quiet defers to the derived reading, which is
        // what stops a finished order from deafening a unit forever.
        sim.mission_assign_exact(1, MissionId::from_known(MissionType::Unload), now)
            .unwrap();
        {
            let e = sim.substrate.entities.get_mut(1).unwrap();
            e.movement_target = Some(MovementTarget::default());
            e.attack_target = Some(AttackTarget::new(9));
            e.passively_acquired_target = true;
        }
        clear_passive_target_off_mission(&mut sim, 1);
        let e = sim.substrate.entities.get(1).unwrap();
        assert!(e.attack_target.is_none(), "a scanner target is dropped");
        assert!(!e.passively_acquired_target);

        // An ordered target on the same mission is left alone.
        {
            let e = sim.substrate.entities.get_mut(1).unwrap();
            e.attack_target = Some(AttackTarget::new(9));
            e.passively_acquired_target = false;
        }
        clear_passive_target_off_mission(&mut sim, 1);
        assert!(
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .attack_target
                .is_some(),
            "only a scanner-acquired target is dropped"
        );
    }

    // ===== Checkpoint A — cloned ordinary-Drive host trace =====

    const ORDINARY_DRIVE_HOST_ID: u64 = 41;
    const STOCK_MOVE_RATE_FRAMES: u32 = 14;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum ActiveGate {
        GuardB,
        Dispatch,
        GuardE,
        FootPostTechno,
        FootPostProcess,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum UnitMoveByte {
        Byte6e1,
        Byte6e2,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum HostTraceEvent {
        TechnoPreThroughRocking,
        ActiveGate {
            gate: ActiveGate,
            pass: bool,
        },
        TechnoRemainingPre,
        MissionAiCounter {
            before: u32,
            after: u32,
        },
        MissionDispatchEnter,
        ObjectAiMarker,
        DispatchTimerGate {
            due: bool,
        },
        DispatchHealthGate {
            pass: bool,
        },
        UnitMoveRead6e0 {
            nonzero: bool,
        },
        UnitMoveClear6d2,
        UnitMoveCheckSaved6e0 {
            nonzero: bool,
        },
        UnitMoveCheck {
            byte: UnitMoveByte,
            nonzero: bool,
        },
        QueueMissionMarker {
            mission_id: MissionId,
            arg: u32,
        },
        UnitTrackerCheckMarker,
        UnitTrackerRestartMarker,
        FootMissionMove,
        NavComCheck {
            present: bool,
        },
        IsMovingCall {
            moving: bool,
        },
        NullLocomotorInvariant,
        OnArrivalMarker {
            arg0: u32,
            arg1: u32,
        },
        RateLookup {
            mission: MissionType,
            frames: u32,
        },
        ScenarioRandomRangedApi {
            low: u32,
            high: u32,
            value: u32,
            raw_advances: usize,
        },
        DispatchWriteStart {
            frame: i32,
        },
        DispatchWriteScratchMarker,
        DispatchWriteDelay {
            delay: i32,
        },
        PassiveAcquireMarker,
        BombMarker,
        SlaveManagerMarker,
        CaptureManagerMarker,
        TechnoLatePostMarker,
        FootPreProcessMarker,
        FootProcessGate {
            ordinal: u8,
            pass: bool,
        },
        DriveProcessMarker,
        FootLaterWorkMarker,
        FootReturnMarker,
    }

    #[derive(Debug, Clone, Copy)]
    struct HostTraceGates {
        guard_b_active: bool,
        dispatch_active: bool,
        guard_e_active: bool,
        foot_post_techno_active: bool,
        foot_post_process_active: bool,
        unit_move_bytes: [bool; 3],
        tracker_needs_restart: bool,
        is_moving: bool,
        foot_process_gates: [bool; 5],
        class_special_pre_foot_path: bool,
        lifecycle_countdown_exit: bool,
    }

    impl HostTraceGates {
        fn ordinary() -> Self {
            Self {
                guard_b_active: true,
                dispatch_active: true,
                guard_e_active: true,
                foot_post_techno_active: true,
                foot_post_process_active: true,
                unit_move_bytes: [false; 3],
                tracker_needs_restart: false,
                is_moving: true,
                foot_process_gates: [true; 5],
                class_special_pre_foot_path: false,
                lifecycle_countdown_exit: false,
            }
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct ClonedHostTrace {
        events: Vec<HostTraceEvent>,
        mission_after: MissionCom,
        scenario_rng_after: SimRngLogicalState,
        is_moving_calls: u8,
        move_random_ranged_calls: u8,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum HostTraceError {
        MissingEntity,
        NonUnit,
        NonMoveStoredMission,
        MinerPath,
        DockPath,
        AircraftPath,
        SpecialLocomotorPath,
        ActiveTube,
        ClassSpecialPath,
        LifecyclePath,
        MissingDriveRuntime,
        StockMoveRate { actual: u32 },
    }

    #[derive(Debug, PartialEq, Eq)]
    struct LiveHostWitness {
        state_hash: u64,
        rng_state: SimulationRngState,
        mission: Option<MissionCom>,
        snapshot: Vec<u8>,
        occupancy_debug: String,
        occupancy_generation: u64,
        occupied_cell_count: usize,
        event_lengths: [usize; 5],
    }

    fn stock_move_control() -> MissionControl {
        MissionControl::from_ini(&IniFile::from_str("[Move]\nRate=.016\n"))
    }

    fn ordinary_drive_host_sim(seed: u64) -> Simulation {
        let mut sim = Simulation::with_seed(seed);
        let mut entity = entity_of(ORDINARY_DRIVE_HOST_ID, EntityCategory::Unit);
        entity.owner = sim.interner.intern("Americans");
        entity.type_ref = sim.interner.intern("TEST");
        entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
        entity.drive_locomotion = Some(DriveLocomotionRuntime::default());
        entity.navigation.nav_com = Some(NavTargetRef::cell(8, 8));
        update_mission_test_fixture(&mut entity.mission, |fixture| {
            fixture.current = MissionId::from_known(MissionType::Move);
            fixture.dispatch_timer = MissionDispatchTimer::at_frame(0);
        });
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
        attacker.attack_target = Some(AttackTarget::new(2));
        update_mission_test_fixture(&mut attacker.mission, |fixture| {
            fixture.current = MissionId::from_known(MissionType::Attack);
            fixture.dispatch_timer = MissionDispatchTimer::at_frame(0);
        });
        // 512 leptons from the target at (5, 5) — inside the 282..=768 band.
        attacker.position.rx = 7;
        attacker.owner = sim.interner.intern("Americans");
        attacker.type_ref = sim.interner.intern(type_name);
        sim.substrate.entities.insert(attacker);
        // On the map: TechnoClass::AI's 16-frame check drops a target in limbo
        // (GetFireError T12).
        let mut target = entity_of(2, EntityCategory::Unit);
        target.lifecycle.in_limbo = false;
        register_entity(&mut sim, target);
        // An enemy: a computer house drops an allied target every frame
        // (`0x006FA30C`).
        let soviet = sim.interner.intern("Soviet");
        sim.substrate.entities.get_mut(2).unwrap().owner = soviet;
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
        let delay = sim
            .substrate
            .entities
            .get(1)
            .unwrap()
            .mission
            .dispatch_timer()
            .delay();
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

        assert_eq!(
            sim.substrate
                .entities
                .get(ORDINARY_DRIVE_HOST_ID)
                .expect("fixture entity")
                .mission
                .dispatch_timer(),
            pending
        );
        assert_eq!(sim.scenario_rng.logical_state(), before_rng);
    }

    #[test]
    fn move_handler_arrival_returns_one_frame_without_rng() {
        let mut sim = ordinary_drive_host_sim(0xCAFE);
        let rules = move_cadence_rules();
        sim.substrate
            .entities
            .get_mut(ORDINARY_DRIVE_HOST_ID)
            .expect("fixture entity")
            .navigation
            .nav_com = None;
        let before_rng = sim.scenario_rng.logical_state();

        sim.object_ai_visit_one(ORDINARY_DRIVE_HOST_ID, Some(&rules), ObjectAiCtx::default());

        assert_eq!(
            sim.substrate
                .entities
                .get(ORDINARY_DRIVE_HOST_ID)
                .expect("fixture entity")
                .mission
                .dispatch_timer(),
            MissionDispatchTimer::from_raw(0, 1)
        );
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
        // All three fixtures sit at 512 leptons, so the band is satisfied and
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
    }

    #[test]
    fn gsi_07_06_close_range_does_not_qualify_a_vehicle() {
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

    /// The Attack handler's only exit. With the shoot-at target gone and no
    /// destination left, the idle-mode selector commits Guard — and Guard is
    /// inside the passive-acquire gate, so the object starts scanning again.
    /// Without this arm an object parked on Attack never scans for a target
    /// for the rest of the match.
    #[test]
    fn attack_handler_with_no_target_enters_idle_mode_and_regains_passive_acquire() {
        let mut sim = Simulation::with_seed(0xA774);
        let rules = representative_foot_handler_rules();
        let mut infantry = entity_of(1, EntityCategory::Infantry);
        infantry.attack_target = None;
        update_mission_test_fixture(&mut infantry.mission, |fixture| {
            fixture.current = MissionId::from_known(MissionType::Attack);
            fixture.dispatch_timer = MissionDispatchTimer::at_frame(0);
        });
        register_entity(&mut sim, infantry);

        let mut expected_rng = sim.clone_scenario_rng();
        // Both arms of the target branch reach the cadence tail, so the idle
        // exit adds no draw; the half-cadence band needs a live target.
        let expected_delay = 14 + expected_rng.next_range_u32_inclusive(0, 2) as i32;
        sim.object_ai_visit_one(1, Some(&rules), ObjectAiCtx::default());

        let infantry = sim.substrate.entities.get(1).unwrap();
        assert_eq!(
            infantry.mission.queued().known(),
            Some(MissionType::Guard),
            "the idle selector queued a replacement instead of leaving it on Attack"
        );
        assert_eq!(
            infantry.mission.dispatch_timer(),
            MissionDispatchTimer::from_raw(0, expected_delay)
        );
        assert_eq!(
            sim.scenario_rng.logical_state(),
            expected_rng.logical_state(),
            "the idle exit consumes no extra RNG"
        );
    }

    /// Same exit, but the object still has somewhere to be — the destination a
    /// Restore just gave back. The idle selector picks Move, not Guard.
    #[test]
    fn attack_handler_with_no_target_but_a_destination_enters_move() {
        let mut sim = Simulation::with_seed(0xA775);
        let rules = representative_foot_handler_rules();
        let mut infantry = entity_of(1, EntityCategory::Infantry);
        infantry.attack_target = None;
        infantry.navigation.nav_com =
            Some(crate::sim::components::NavTargetRef::Cell { rx: 20, ry: 21 });
        update_mission_test_fixture(&mut infantry.mission, |fixture| {
            fixture.current = MissionId::from_known(MissionType::Attack);
            fixture.dispatch_timer = MissionDispatchTimer::at_frame(0);
        });
        register_entity(&mut sim, infantry);

        sim.object_ai_visit_one(1, Some(&rules), ObjectAiCtx::default());

        assert_eq!(
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .mission
                .queued()
                .known(),
            Some(MissionType::Move)
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
            harvester_miner: false,
            depot_dock_state: false,
            timer_due: true,
            moving_or_queued: false,
            bunker_delegate: false,
            has_attack_target: false,
            has_destination: false,
            effective_mission: Some(MissionType::Attack),
            unit_deploy_begin_active: false,
            unit_deploy_reverse_active: false,
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

    /// The `Zombie=`/`Paralyzed=` early return, read off the object's own
    /// control entry. Neither key is present in stock `[Attack]`, so this cannot
    /// fire from the Attack handler; the gate is modelled because the same
    /// virtual is entered from other missions.
    #[test]
    fn enter_idle_mode_selector_honours_frozen_control_entries() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[General]\n\n[Attack]\nRate=.016\n\n[Hunt]\nRate=.016\nParalyzed=yes\n\n[Sticky]\nRate=.016\nZombie=yes\n",
        ))
        .expect("frozen control rules parse");
        let base = MissionHandlerInput {
            category: EntityCategory::Infantry,
            mission: Some(MissionType::Attack),
            harvester_miner: false,
            depot_dock_state: false,
            timer_due: true,
            moving_or_queued: false,
            bunker_delegate: false,
            has_attack_target: false,
            has_destination: false,
            effective_mission: Some(MissionType::Attack),
            unit_deploy_begin_active: false,
            unit_deploy_reverse_active: false,
            infantry_deployed_do_type: false,
            infantry_deploy_fire_stance: false,
        };

        for frozen in [MissionType::Hunt, MissionType::Sticky] {
            assert_eq!(
                foot_enter_idle_mode_queue(
                    &rules,
                    MissionHandlerInput {
                        effective_mission: Some(frozen),
                        ..base
                    }
                ),
                None,
                "{frozen:?} carries a frozen control entry"
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
            sim.session.binary_frame = frame;
            for (id, type_name, owner, rx) in
                [(1, attacker, "Russians", 5), (2, "BRUTE", "YuriCountry", 6)]
            {
                let mut entity = GameEntity::test_default(id, type_name, owner, rx, 5);
                entity.category = EntityCategory::Infantry;
                entity.mission_leaf =
                    MissionLeafState::for_entity_category(EntityCategory::Infantry);
                entity.lifecycle.in_limbo = false;
                entity.owner = sim.interner.intern(owner);
                entity.type_ref = sim.interner.intern(type_name);
                sim.substrate.entities.insert(entity);
            }
            if cloaked {
                let mut cloak = crate::sim::cloak_disguise::CloakRuntime::new(0, 9);
                cloak.state = 2;
                sim.substrate.entities.get_mut(2).unwrap().cloak = Some(cloak);
            }
            let dog = sim.substrate.entities.get_mut(1).unwrap();
            dog.attack_target = Some(AttackTarget::new(2));
            update_mission_test_fixture(&mut dog.mission, |fixture| {
                fixture.current = MissionId::from_known(MissionType::Attack);
                fixture.dispatch_timer = MissionDispatchTimer::at_frame(frame);
            });
            sim.object_ai_visit_one(1, Some(&rules), ObjectAiCtx::default());
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .attack_target
                .is_some()
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

    #[test]
    fn attack_handler_clears_only_an_authoritatively_stale_entity_target() {
        let mut sim = Simulation::with_seed(0xA773);
        let rules = representative_foot_handler_rules();
        let mut unit = entity_of(1, EntityCategory::Unit);
        unit.attack_target = Some(AttackTarget::new(99));
        unit.weapon_burst.complete_shot(2);
        update_mission_test_fixture(&mut unit.mission, |fixture| {
            fixture.current = MissionId::from_known(MissionType::Attack);
            fixture.dispatch_timer = MissionDispatchTimer::at_frame(0);
        });
        register_entity(&mut sim, unit);

        sim.object_ai_visit_one(1, Some(&rules), ObjectAiCtx::default());

        let unit = sim.substrate.entities.get(1).unwrap();
        assert!(unit.attack_target.is_none());
        assert_eq!(unit.weapon_burst.index(), 1);
        assert_eq!(unit.mission.queued(), MissionId::NONE);
    }

    #[test]
    fn infantry_guard_handler_skips_jitter_for_a_bunker_delegate() {
        let mut sim = Simulation::with_seed(0x6A2D);
        let rules = representative_foot_handler_rules();
        let mut infantry = entity_of(1, EntityCategory::Infantry);
        infantry.bunker_link = BunkerLink::Installed(99);
        update_mission_test_fixture(&mut infantry.mission, |fixture| {
            fixture.current = MissionId::from_known(MissionType::Guard);
            fixture.dispatch_timer = MissionDispatchTimer::at_frame(0);
        });
        sim.substrate.entities.insert(infantry);
        let before_rng = sim.scenario_rng.logical_state();

        sim.object_ai_visit_one(1, Some(&rules), ObjectAiCtx::default());

        assert_eq!(
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .mission
                .dispatch_timer(),
            MissionDispatchTimer::from_raw(0, 14)
        );
        assert_eq!(sim.scenario_rng.logical_state(), before_rng);
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
                object.rearm_timer = crate::sim::timer::CdTimer::started(armed_at, 30);
                update_mission_test_fixture(&mut object.mission, |fixture| {
                    fixture.current = MissionId::from_known(mission);
                    fixture.dispatch_timer = MissionDispatchTimer::at_frame(0);
                });
                register_entity(&mut sim, object);
                let before_rng = sim.scenario_rng.logical_state();

                sim.object_ai_visit_one(1, Some(&rules), ObjectAiCtx::default());

                let timer = sim
                    .substrate
                    .entities
                    .get(1)
                    .unwrap()
                    .mission
                    .dispatch_timer();
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
    fn unit_guard_deploy_begin_queues_harvest_without_rng() {
        let mut sim = Simulation::with_seed(0x6A2E);
        let rules = representative_foot_handler_rules();
        let mut unit = entity_of(1, EntityCategory::Unit);
        unit.mission_leaf = MissionLeafState::unit_raw_for_test(1, 0, 0, 0);
        update_mission_test_fixture(&mut unit.mission, |fixture| {
            fixture.current = MissionId::from_known(MissionType::Guard);
            fixture.dispatch_timer = MissionDispatchTimer::at_frame(0);
        });
        register_entity(&mut sim, unit);
        let before_rng = sim.scenario_rng.logical_state();

        sim.object_ai_visit_one(1, Some(&rules), ObjectAiCtx::default());

        let unit = sim.substrate.entities.get(1).unwrap();
        assert_eq!(unit.mission.queued().known(), Some(MissionType::Harvest));
        assert_eq!(
            unit.mission.dispatch_timer(),
            MissionDispatchTimer::from_raw(0, 1)
        );
        assert_eq!(sim.scenario_rng.logical_state(), before_rng);
    }

    #[test]
    fn unit_guard_deploy_reverse_queues_unload_without_rng() {
        let mut sim = Simulation::with_seed(0x6A2F);
        let rules = representative_foot_handler_rules();
        let mut unit = entity_of(1, EntityCategory::Unit);
        unit.mission_leaf = MissionLeafState::unit_raw_for_test(0, 1, 0, 0);
        update_mission_test_fixture(&mut unit.mission, |fixture| {
            fixture.current = MissionId::from_known(MissionType::Guard);
            fixture.dispatch_timer = MissionDispatchTimer::at_frame(0);
        });
        register_entity(&mut sim, unit);
        let before_rng = sim.scenario_rng.logical_state();

        sim.object_ai_visit_one(1, Some(&rules), ObjectAiCtx::default());

        let unit = sim.substrate.entities.get(1).unwrap();
        assert_eq!(unit.mission.queued().known(), Some(MissionType::Unload));
        assert_eq!(
            unit.mission.dispatch_timer(),
            MissionDispatchTimer::from_raw(0, 1)
        );
        assert_eq!(sim.scenario_rng.logical_state(), before_rng);
    }

    #[test]
    fn infantry_hunt_handler_uses_foot_jittered_cadence() {
        let mut sim = Simulation::with_seed(0x487A);
        let rules = representative_foot_handler_rules();
        let mut infantry = entity_of(1, EntityCategory::Infantry);
        // The Hunt body resolves the object type (for `StupidHunt=`) and runs
        // the scanner, so the fixture has to be interned in THIS sim.
        infantry.owner = sim.interner.intern("Americans");
        infantry.type_ref = sim.interner.intern("TEST");
        update_mission_test_fixture(&mut infantry.mission, |fixture| {
            fixture.current = MissionId::from_known(MissionType::Hunt);
            fixture.dispatch_timer = MissionDispatchTimer::at_frame(0);
        });
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
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .mission
                .dispatch_timer(),
            MissionDispatchTimer::from_raw(0, expected_delay)
        );
        assert_eq!(
            sim.scenario_rng.logical_state(),
            expected_rng.logical_state()
        );
    }

    /// Both `UnitClass::Mission_Hunt @ 0x0073EFC0` exits and the
    /// `FootClass::Mission_Hunt @ 0x004D5350` body it falls through to draw
    /// `RandomRanged(0, 2)`. The earlier "no-jitter fallback" this test pinned
    /// does not exist in the binary.
    #[test]
    fn gsi_07_20_unit_hunt_handler_draws_the_foot_jitter() {
        let mut sim = Simulation::with_seed(0xF007);
        let rules = representative_foot_handler_rules();
        let mut unit = entity_of(1, EntityCategory::Unit);
        unit.owner = sim.interner.intern("Americans");
        unit.type_ref = sim.interner.intern("TEST");
        update_mission_test_fixture(&mut unit.mission, |fixture| {
            fixture.current = MissionId::from_known(MissionType::Hunt);
            fixture.dispatch_timer = MissionDispatchTimer::at_frame(0);
        });
        sim.substrate.entities.insert(unit);
        let before_rng = sim.scenario_rng.logical_state();

        sim.object_ai_visit_one(1, Some(&rules), ObjectAiCtx::default());

        let delay = sim
            .substrate
            .entities
            .get(1)
            .unwrap()
            .mission
            .dispatch_timer()
            .delay();
        assert!(
            (14..=16).contains(&delay),
            "Hunt re-arms at Rate + RandomRanged(0, 2), got {delay}"
        );
        assert_ne!(
            sim.scenario_rng.logical_state(),
            before_rng,
            "the jitter draw lands on the scenario stream"
        );
    }

    #[test]
    fn unit_hunt_does_not_infer_enter_without_an_approach_result() {
        let mut sim = Simulation::with_seed(0xF008);
        let rules = representative_foot_handler_rules();
        let mut unit = entity_of(1, EntityCategory::Unit);
        unit.attack_target = Some(AttackTarget::new(2));
        update_mission_test_fixture(&mut unit.mission, |fixture| {
            fixture.current = MissionId::from_known(MissionType::Hunt);
            fixture.dispatch_timer = MissionDispatchTimer::at_frame(0);
        });
        register_entity(&mut sim, unit);
        let target = entity_of(2, EntityCategory::Structure);
        register_entity(&mut sim, target);
        let before_rng = sim.scenario_rng.logical_state();

        sim.object_ai_visit_one(1, Some(&rules), ObjectAiCtx::default());

        let unit = sim.substrate.entities.get(1).unwrap();
        assert_eq!(unit.mission.queued(), MissionId::NONE);
        let delay = unit.mission.dispatch_timer().delay();
        assert!((14..=16).contains(&delay), "Rate + RandomRanged(0, 2)");
        assert_ne!(sim.scenario_rng.logical_state(), before_rng);
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
        let heights: std::collections::BTreeMap<(u16, u16), u8> = std::collections::BTreeMap::new();
        let mut sim = Simulation::with_seed(seed);
        sim.spawn_from_map(
            &[
                passive_map_entity("Americans", hunter_type, 20, 20, EntityCategory::Unit),
                passive_map_entity("Soviet", "HUNTVEH", 20 + gap, 20, EntityCategory::Unit),
            ],
            Some(&rules),
            &heights,
        );
        let hunter = sim
            .substrate
            .entities
            .get_mut(1)
            .expect("hunter spawned first");
        update_mission_test_fixture(&mut hunter.mission, |fixture| {
            fixture.current = MissionId::from_known(mission);
            fixture.dispatch_timer = MissionDispatchTimer::at_frame(0);
        });
        sim
    }

    /// The headline of GSI-07.20: a hunting object acquires past its own weapon
    /// reach, and the target it installs is NOT flagged passively-acquired — so
    /// the pursuit pass will close on it instead of skipping it. Native:
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
        let heights: std::collections::BTreeMap<(u16, u16), u8> = std::collections::BTreeMap::new();
        let grid = crate::sim::pathfinding::PathGrid::new(64, 64);
        // Record the state at the moment of acquisition: the hunter starts
        // closing as soon as it has a target, and the point of the test is how
        // far away it was when it found one.
        let mut acquired = None;
        for _ in 0..40 {
            let _ = sim.advance_tick(&[], Some(&rules), &heights, Some(&grid), None, 67);
            let hunter = sim.substrate.entities.get(1).expect("hunter present");
            if hunter.attack_target.is_some() {
                acquired = Some((
                    28u16.saturating_sub(hunter.position.rx),
                    hunter.passively_acquired_target,
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

    /// The control for the test above: the very same pair, at the very same
    /// range, acquires nothing on Guard. Without this the Hunt result could be
    /// explained by an over-wide scan rather than by the mask-0 arm.
    #[test]
    fn gsi_07_20_guard_at_the_same_range_acquires_nothing() {
        let rules = hunt_body_rules();
        let mut sim = hunt_pair(0x40_0721, "HUNTVEH", 8, MissionType::Guard);
        let heights: std::collections::BTreeMap<(u16, u16), u8> = std::collections::BTreeMap::new();
        let grid = crate::sim::pathfinding::PathGrid::new(64, 64);
        for _ in 0..40 {
            let _ = sim.advance_tick(&[], Some(&rules), &heights, Some(&grid), None, 67);
        }

        assert!(
            sim.substrate
                .entities
                .get(1)
                .expect("guard present")
                .attack_target
                .is_none(),
            "a Guard-mission scan defers to the weapon's own reach (4 cells), so 8 is out"
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
            hunter.attack_target.is_none(),
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
        attacker.attack_target = Some(AttackTarget::new(2));
        update_mission_test_fixture(&mut attacker.mission, |fixture| {
            fixture.current = MissionId::from_known(MissionType::Attack);
            fixture.dispatch_timer = MissionDispatchTimer::at_frame(0);
        });
        // Target sits at (5, 5); 3 cells east is exactly 768 leptons, and the
        // 30-lepton north/south offset pushes d² past 768² without pushing the
        // truncated root past 768.
        attacker.position.rx = 8;
        attacker.position.sub_y += crate::util::fixed_math::SimFixed::from_num(30);
        attacker.owner = sim.interner.intern("Americans");
        attacker.type_ref = sim.interner.intern("SHORTVEH");
        sim.substrate.entities.insert(attacker);
        let mut target = entity_of(2, EntityCategory::Unit);
        target.lifecycle.in_limbo = false;
        register_entity(&mut sim, target);
        let soviet = sim.interner.intern("Soviet");
        sim.substrate.entities.get_mut(2).unwrap().owner = soviet;

        let mut expected_rng = sim.clone_scenario_rng();
        let jitter = expected_rng.next_range_u32_inclusive(0, 2) as i32;
        sim.object_ai_visit_one(1, Some(&rules), ObjectAiCtx::default());

        assert_eq!(
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .mission
                .dispatch_timer()
                .delay(),
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
        man.owner = sim.interner.intern("Americans");
        man.type_ref = sim.interner.intern("DEPINF");
        man.deploy_state = Some(crate::sim::deploy::DeployPhase::Deployed);
        update_mission_test_fixture(&mut man.mission, |fixture| {
            fixture.current = MissionId::from_known(MissionType::AreaGuard);
            fixture.dispatch_timer = MissionDispatchTimer::at_frame(0);
        });
        sim.substrate.entities.insert(man);

        let mut expected_rng = sim.clone_scenario_rng();
        let expected_delay = 14 + expected_rng.next_range_u32_inclusive(0, 2) as i32;

        sim.object_ai_visit_one(1, Some(&rules), ObjectAiCtx::default());

        assert_eq!(
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .mission
                .dispatch_timer()
                .delay(),
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
            (EntityCategory::Unit, MissionType::Stop),
            (EntityCategory::Unit, MissionType::Selling),
            (EntityCategory::Unit, MissionType::Sleep),
            (EntityCategory::Unit, MissionType::Harmless),
            (EntityCategory::Infantry, MissionType::Stop),
            // Repair is a base stub for Infantry only.
            (EntityCategory::Infantry, MissionType::Repair),
        ] {
            let mut sim = Simulation::with_seed(0x5B2E);
            let rules = representative_foot_handler_rules();
            let mut unit = entity_of(1, category);
            update_mission_test_fixture(&mut unit.mission, |fixture| {
                fixture.current = MissionId::from_known(mission);
                fixture.dispatch_timer = MissionDispatchTimer::at_frame(0);
            });
            register_entity(&mut sim, unit);
            let before_rng = sim.scenario_rng.logical_state();

            sim.object_ai_visit_one(1, Some(&rules), ObjectAiCtx::default());

            assert_eq!(
                sim.substrate
                    .entities
                    .get(1)
                    .unwrap()
                    .mission
                    .dispatch_timer(),
                MissionDispatchTimer::from_raw(0, BASE_MISSION_HANDLER_FRAMES),
                "{category:?} on {mission:?} must re-arm the base handler's flat delay"
            );
            assert_eq!(
                sim.scenario_rng.logical_state(),
                before_rng,
                "{category:?} on {mission:?} must draw no RNG — the base stub touches nothing"
            );
        }
    }

    /// The dispatcher's bounds test on the mission id is unsigned, so the idle
    /// sentinel takes the switch default — the same base handler `Sleep(0)` uses
    /// — rather than being skipped.
    #[test]
    fn the_idle_sentinel_takes_the_default_arm() {
        let mut sim = Simulation::with_seed(0x5B34);
        let rules = representative_foot_handler_rules();
        let mut unit = entity_of(1, EntityCategory::Unit);
        update_mission_test_fixture(&mut unit.mission, |fixture| {
            fixture.current = MissionId::NONE;
            fixture.dispatch_timer = MissionDispatchTimer::at_frame(0);
        });
        register_entity(&mut sim, unit);
        let before_rng = sim.scenario_rng.logical_state();

        sim.object_ai_visit_one(1, Some(&rules), ObjectAiCtx::default());

        assert_eq!(
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .mission
                .dispatch_timer(),
            MissionDispatchTimer::from_raw(0, BASE_MISSION_HANDLER_FRAMES)
        );
        assert_eq!(sim.scenario_rng.logical_state(), before_rng);
    }

    /// A mission whose leaf class DOES override the slot with a real handler
    /// must not be given the base handler's value — that would install a 30
    /// second timer where the original installs its own, much shorter one.
    #[test]
    fn overridden_leaf_handlers_are_left_alone_by_the_default_arm() {
        // Area Guard used to sit in this list; it now has an absorbed handler
        // arm of its own, so the dispatcher reaches it and re-arms its timer.
        // `base_mission_handler_delay` still reports `None` for it — the two
        // facts are independent.
        assert_eq!(
            base_mission_handler_delay(EntityCategory::Unit, Some(MissionType::AreaGuard)),
            None
        );
        for (category, mission) in [
            (EntityCategory::Unit, MissionType::Enter),
            (EntityCategory::Unit, MissionType::Unload),
            // Repair overrides on Units even though it is a stub on Infantry.
            (EntityCategory::Unit, MissionType::Repair),
            (EntityCategory::Infantry, MissionType::Enter),
        ] {
            assert_eq!(
                base_mission_handler_delay(category, Some(mission)),
                None,
                "{category:?} on {mission:?} has a real leaf handler"
            );
            let mut sim = Simulation::with_seed(0x5B2F);
            let rules = representative_foot_handler_rules();
            let mut unit = entity_of(1, category);
            update_mission_test_fixture(&mut unit.mission, |fixture| {
                fixture.current = MissionId::from_known(mission);
                fixture.dispatch_timer = MissionDispatchTimer::at_frame(0);
            });
            register_entity(&mut sim, unit);

            sim.object_ai_visit_one(1, Some(&rules), ObjectAiCtx::default());

            assert_eq!(
                sim.substrate
                    .entities
                    .get(1)
                    .unwrap()
                    .mission
                    .dispatch_timer(),
                MissionDispatchTimer::at_frame(0),
                "{category:?} on {mission:?} must keep its untouched timer"
            );
        }
    }

    /// Sticky and Guard dispatch through the same slot, so Sticky runs the
    /// Guard handler — but the cadence comes from the object's OWN mission
    /// slot, so `[Sticky] Rate` (14) applies, not `[Guard] Rate` (26).
    #[test]
    fn sticky_runs_the_guard_handler_at_its_own_rate() {
        let mut sim = Simulation::with_seed(0x21C0);
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[General]\n\n[Guard]\nRate=.030\n\n[Sticky]\nRate=.016\n",
        ))
        .expect("sticky cadence rules parse");
        let mut unit = entity_of(1, EntityCategory::Unit);
        update_mission_test_fixture(&mut unit.mission, |fixture| {
            fixture.current = MissionId::from_known(MissionType::Sticky);
            fixture.dispatch_timer = MissionDispatchTimer::at_frame(0);
        });
        register_entity(&mut sim, unit);
        let mut expected_rng = sim.clone_scenario_rng();
        let jitter = expected_rng.next_range_u32_inclusive(0, 2) as i32;

        sim.object_ai_visit_one(1, Some(&rules), ObjectAiCtx::default());

        assert_eq!(
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .mission
                .dispatch_timer(),
            MissionDispatchTimer::from_raw(0, 14 + jitter),
            "[Sticky] Rate=.016 is 14 frames; [Guard] Rate=.030 would be 26"
        );
        assert_eq!(
            sim.scenario_rng.logical_state(),
            expected_rng.logical_state()
        );
    }

    /// A vehicle that finishes a move order drops its shoot-at target and
    /// queues Guard — the arrival hook's ordinary-vehicle arm. Without this the
    /// unit stays on Move for the rest of the match.
    #[test]
    fn move_arrival_drops_the_vehicle_target_and_queues_guard() {
        let mut sim = Simulation::with_seed(0x7389);
        let rules = move_cadence_rules();
        let mut unit = entity_of(1, EntityCategory::Unit);
        unit.attack_target = Some(AttackTarget::new(2));
        unit.passively_acquired_target = true;
        unit.weapon_burst.complete_shot(2);
        update_mission_test_fixture(&mut unit.mission, |fixture| {
            fixture.current = MissionId::from_known(MissionType::Move);
            fixture.dispatch_timer = MissionDispatchTimer::at_frame(0);
        });
        register_entity(&mut sim, unit);
        let before_rng = sim.scenario_rng.logical_state();

        sim.object_ai_visit_one(1, Some(&rules), ObjectAiCtx::default());

        let entity = sim.substrate.entities.get(1).expect("fixture entity");
        assert_eq!(
            entity.mission.queued(),
            MissionId::from_known(MissionType::Guard)
        );
        assert!(
            entity.attack_target.is_none(),
            "Assign_Target(NULL) on arrival"
        );
        assert!(!entity.passively_acquired_target);
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
        man.attack_target = Some(AttackTarget::new(2));
        update_mission_test_fixture(&mut man.mission, |fixture| {
            fixture.current = MissionId::from_known(MissionType::Move);
            fixture.dispatch_timer = MissionDispatchTimer::at_frame(0);
        });
        register_entity(&mut sim, man);

        sim.object_ai_visit_one(1, Some(&rules), ObjectAiCtx::default());

        let entity = sim.substrate.entities.get(1).expect("fixture entity");
        assert_eq!(
            entity.mission.queued(),
            MissionId::from_known(MissionType::Attack)
        );
        assert!(
            entity.attack_target.is_some(),
            "the infantry arm keeps the target it promotes"
        );
    }

    /// A targetless infantryman arriving falls to Guard, same as a vehicle.
    #[test]
    fn move_arrival_queues_guard_for_a_targetless_infantryman() {
        let mut sim = Simulation::with_seed(0x51CC);
        let rules = move_cadence_rules();
        let mut man = entity_of(1, EntityCategory::Infantry);
        update_mission_test_fixture(&mut man.mission, |fixture| {
            fixture.current = MissionId::from_known(MissionType::Move);
            fixture.dispatch_timer = MissionDispatchTimer::at_frame(0);
        });
        register_entity(&mut sim, man);

        sim.object_ai_visit_one(1, Some(&rules), ObjectAiCtx::default());

        assert_eq!(
            sim.substrate
                .entities
                .get(1)
                .expect("fixture entity")
                .mission
                .queued(),
            MissionId::from_known(MissionType::Guard)
        );
    }

    /// The infantry arrival selector is skipped entirely when the mission the
    /// object arrived ON carries `Zombie=` or `Paralyzed=`. `[Move]` carries
    /// neither in stock rules, so this uses a fixture that sets one.
    #[test]
    fn move_arrival_infantry_selector_is_skipped_by_a_paralyzed_move_entry() {
        let mut sim = Simulation::with_seed(0x51CD);
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[General]\n\n[Move]\nRate=.016\nParalyzed=yes\n",
        ))
        .expect("paralyzed move rules parse");
        let mut man = entity_of(1, EntityCategory::Infantry);
        update_mission_test_fixture(&mut man.mission, |fixture| {
            fixture.current = MissionId::from_known(MissionType::Move);
            fixture.dispatch_timer = MissionDispatchTimer::at_frame(0);
        });
        register_entity(&mut sim, man);

        sim.object_ai_visit_one(1, Some(&rules), ObjectAiCtx::default());

        let entity = sim.substrate.entities.get(1).expect("fixture entity");
        assert_eq!(entity.mission.queued(), MissionId::NONE);
        assert_eq!(
            entity.mission.dispatch_timer(),
            MissionDispatchTimer::from_raw(0, 1)
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
        update_mission_test_fixture(&mut unit.mission, |fixture| {
            fixture.current = MissionId::from_known(MissionType::AreaGuard);
            fixture.dispatch_timer = MissionDispatchTimer::at_frame(0);
        });
        register_entity(&mut sim, unit);
        let mut expected_rng = sim.clone_scenario_rng();
        let jitter = expected_rng.next_range_u32_inclusive(1, 5) as i32;
        assert!((1..=5).contains(&jitter));

        sim.object_ai_visit_one(1, Some(&rules), ObjectAiCtx::default());

        assert_eq!(
            sim.substrate
                .entities
                .get(1)
                .expect("fixture entity")
                .mission
                .dispatch_timer(),
            MissionDispatchTimer::from_raw(0, 35 + jitter),
            "[Area Guard] Rate=.040 is 35 frames; [Guard] Rate=.030 would be 26"
        );
        assert_eq!(
            sim.scenario_rng.logical_state(),
            expected_rng.logical_state(),
            "exactly one RandomRanged(1, 5) and nothing else"
        );
    }

    fn capture_live_host_witness(sim: &Simulation, id: u64) -> LiveHostWitness {
        LiveHostWitness {
            state_hash: sim.state_hash(),
            rng_state: sim.rng_state(),
            mission: sim.substrate.entities.get(id).map(|entity| entity.mission),
            snapshot: GameSnapshot::save(sim, 0, 0, "checkpoint_a_host_trace", 0),
            occupancy_debug: format!("{:?}", sim.occupancy()),
            occupancy_generation: sim.occupancy().generation(),
            occupied_cell_count: sim.occupancy().occupied_cell_count(),
            event_lengths: [
                sim.sound_events.len(),
                sim.fire_events.len(),
                sim.pending_smudge_requests.len(),
                sim.bale_events.len(),
                sim.bunker_wall_events.len(),
            ],
        }
    }

    fn validate_ordinary_drive_host_entity(
        entity: &GameEntity,
        gates: HostTraceGates,
    ) -> Result<(), HostTraceError> {
        if entity.category == EntityCategory::Aircraft || entity.aircraft_mission.is_some() {
            return Err(HostTraceError::AircraftPath);
        }
        if entity.category != EntityCategory::Unit {
            return Err(HostTraceError::NonUnit);
        }
        if entity.mission.current() != MissionId::from_known(MissionType::Move) {
            return Err(HostTraceError::NonMoveStoredMission);
        }
        if entity.miner.is_some() {
            return Err(HostTraceError::MinerPath);
        }
        if entity.dock_state.is_some() {
            return Err(HostTraceError::DockPath);
        }
        if entity.low_bridge_tube_state.is_some() {
            return Err(HostTraceError::ActiveTube);
        }
        // Ordinary and forced descriptors share the same Drive Process host.
        // This inert shell trace only records that boundary; track state does
        // not select another pre-Foot path or make the shell out of scope.
        if gates.class_special_pre_foot_path {
            return Err(HostTraceError::ClassSpecialPath);
        }
        if gates.lifecycle_countdown_exit {
            return Err(HostTraceError::LifecyclePath);
        }
        if entity.teleport_state.is_some()
            || entity.rocket_state.is_some()
            || entity.homing_state.is_some()
            || entity.parachute_state.is_some()
        {
            return Err(HostTraceError::SpecialLocomotorPath);
        }

        let locomotor = entity
            .locomotor
            .as_ref()
            .ok_or(HostTraceError::SpecialLocomotorPath)?;
        if locomotor.active_kind() != LocomotorKind::Drive
            || locomotor.effective_kind() != LocomotorKind::Drive
            || locomotor.piggyback.is_some()
            || locomotor.is_overridden()
        {
            return Err(HostTraceError::SpecialLocomotorPath);
        }
        if entity.drive_locomotion.is_none() && entity.navigation.nav_com.is_some() {
            return Err(HostTraceError::MissingDriveRuntime);
        }
        Ok(())
    }

    fn finish_cloned_host_trace(
        events: Vec<HostTraceEvent>,
        entity: &GameEntity,
        scenario_rng: &crate::sim::rng::SimRng,
        is_moving_calls: u8,
        move_random_ranged_calls: u8,
    ) -> ClonedHostTrace {
        ClonedHostTrace {
            events,
            mission_after: entity.mission,
            scenario_rng_after: scenario_rng.logical_state(),
            is_moving_calls,
            move_random_ranged_calls,
        }
    }

    fn draw_cloned_move_jitter(rng: &mut crate::sim::rng::SimRng) -> (u32, usize) {
        let mut probe = rng.clone();
        let value = rng.next_range_u32_inclusive(0, 2);
        let mut raw_advances = 0usize;
        let probe_value = loop {
            raw_advances += 1;
            let candidate = probe.next_u32() & 3;
            if candidate <= 2 {
                break candidate;
            }
        };
        assert_eq!(
            probe_value, value,
            "the raw probe must reproduce the ranged result"
        );
        assert_eq!(
            probe.logical_state(),
            rng.logical_state(),
            "the raw probe must reproduce the complete ranged-call RNG state"
        );
        (value, raw_advances)
    }

    fn trace_cloned_ordinary_drive_host(
        sim: &Simulation,
        id: u64,
        mission_control: &MissionControl,
        native_frame: u32,
        gates: HostTraceGates,
    ) -> Result<ClonedHostTrace, HostTraceError> {
        let source = sim
            .substrate
            .entities
            .get(id)
            .ok_or(HostTraceError::MissingEntity)?;
        validate_ordinary_drive_host_entity(source, gates)?;
        let move_rate = mission_control.rate_frames(MissionType::Move);
        if move_rate != STOCK_MOVE_RATE_FRAMES {
            return Err(HostTraceError::StockMoveRate { actual: move_rate });
        }

        let mut entity = source.clone();
        let mut scenario_rng = sim.clone_scenario_rng();
        let mut events = Vec::new();
        let mut is_moving_calls = 0u8;
        let mut move_random_ranged_calls = 0u8;

        events.push(HostTraceEvent::TechnoPreThroughRocking);
        events.push(HostTraceEvent::ActiveGate {
            gate: ActiveGate::GuardB,
            pass: gates.guard_b_active,
        });
        if !gates.guard_b_active {
            events.push(HostTraceEvent::ActiveGate {
                gate: ActiveGate::FootPostTechno,
                pass: false,
            });
            events.push(HostTraceEvent::FootReturnMarker);
            return Ok(finish_cloned_host_trace(
                events,
                &entity,
                &scenario_rng,
                is_moving_calls,
                move_random_ranged_calls,
            ));
        }

        events.push(HostTraceEvent::TechnoRemainingPre);
        let ai_counter_before = entity.mission.ai_counter();
        update_mission_test_fixture(&mut entity.mission, |fixture| {
            fixture.ai_counter = ai_counter_before.wrapping_add(1);
        });
        events.push(HostTraceEvent::MissionAiCounter {
            before: ai_counter_before,
            after: entity.mission.ai_counter(),
        });
        events.push(HostTraceEvent::MissionDispatchEnter);
        events.push(HostTraceEvent::ObjectAiMarker);
        events.push(HostTraceEvent::ActiveGate {
            gate: ActiveGate::Dispatch,
            pass: gates.dispatch_active,
        });

        let dispatch_inactive = !gates.dispatch_active;
        let mut handler_delay: Option<i32> = None;
        if gates.dispatch_active {
            let due = entity.mission.dispatch_timer().due(native_frame);
            events.push(HostTraceEvent::DispatchTimerGate { due });
            if due {
                let health_pass = entity.health.current > 0;
                events.push(HostTraceEvent::DispatchHealthGate { pass: health_pass });
                if health_pass {
                    let byte6e0 = gates.unit_move_bytes[0];
                    events.push(HostTraceEvent::UnitMoveRead6e0 { nonzero: byte6e0 });
                    events.push(HostTraceEvent::UnitMoveClear6d2);
                    events.push(HostTraceEvent::UnitMoveCheckSaved6e0 { nonzero: byte6e0 });

                    let queue_guard = if byte6e0 {
                        true
                    } else {
                        let byte6e1 = gates.unit_move_bytes[1];
                        events.push(HostTraceEvent::UnitMoveCheck {
                            byte: UnitMoveByte::Byte6e1,
                            nonzero: byte6e1,
                        });
                        if byte6e1 {
                            true
                        } else {
                            let byte6e2 = gates.unit_move_bytes[2];
                            events.push(HostTraceEvent::UnitMoveCheck {
                                byte: UnitMoveByte::Byte6e2,
                                nonzero: byte6e2,
                            });
                            byte6e2
                        }
                    };

                    if queue_guard {
                        events.push(HostTraceEvent::QueueMissionMarker {
                            mission_id: MissionId::from_known(MissionType::Guard),
                            arg: 0,
                        });
                        handler_delay = Some(1);
                    } else {
                        events.push(HostTraceEvent::UnitTrackerCheckMarker);
                        if gates.tracker_needs_restart {
                            events.push(HostTraceEvent::UnitTrackerRestartMarker);
                        }
                        events.push(HostTraceEvent::FootMissionMove);
                        let nav_com_present = entity.navigation.nav_com.is_some();
                        events.push(HostTraceEvent::NavComCheck {
                            present: nav_com_present,
                        });

                        if !nav_com_present && entity.drive_locomotion.is_none() {
                            events.push(HostTraceEvent::NullLocomotorInvariant);
                            return Ok(finish_cloned_host_trace(
                                events,
                                &entity,
                                &scenario_rng,
                                is_moving_calls,
                                move_random_ranged_calls,
                            ));
                        }

                        let stopped_arrival = if nav_com_present {
                            false
                        } else {
                            is_moving_calls = is_moving_calls.wrapping_add(1);
                            events.push(HostTraceEvent::IsMovingCall {
                                moving: gates.is_moving,
                            });
                            !gates.is_moving && entity.mission.queued() == MissionId::NONE
                        };

                        if stopped_arrival {
                            events.push(HostTraceEvent::OnArrivalMarker { arg0: 0, arg1: 1 });
                            handler_delay = Some(1);
                        } else {
                            events.push(HostTraceEvent::RateLookup {
                                mission: MissionType::Move,
                                frames: move_rate,
                            });
                            let (jitter, raw_advances) = draw_cloned_move_jitter(&mut scenario_rng);
                            move_random_ranged_calls = move_random_ranged_calls.wrapping_add(1);
                            events.push(HostTraceEvent::ScenarioRandomRangedApi {
                                low: 0,
                                high: 2,
                                value: jitter,
                                raw_advances,
                            });
                            handler_delay = Some((move_rate + jitter) as i32);
                        }
                    }
                }
            }
        }

        if let Some(delay) = handler_delay {
            events.push(HostTraceEvent::DispatchWriteStart {
                frame: native_frame as i32,
            });
            events.push(HostTraceEvent::DispatchWriteScratchMarker);
            events.push(HostTraceEvent::DispatchWriteDelay { delay });
            update_mission_test_fixture(&mut entity.mission, |fixture| {
                fixture.dispatch_timer = MissionDispatchTimer::from_raw(native_frame as i32, delay);
            });
        }

        events.push(HostTraceEvent::PassiveAcquireMarker);
        events.push(HostTraceEvent::BombMarker);
        events.push(HostTraceEvent::SlaveManagerMarker);
        events.push(HostTraceEvent::CaptureManagerMarker);
        let guard_e_active = !dispatch_inactive && gates.guard_e_active;
        events.push(HostTraceEvent::ActiveGate {
            gate: ActiveGate::GuardE,
            pass: guard_e_active,
        });
        if !guard_e_active {
            events.push(HostTraceEvent::ActiveGate {
                gate: ActiveGate::FootPostTechno,
                pass: false,
            });
            events.push(HostTraceEvent::FootReturnMarker);
            return Ok(finish_cloned_host_trace(
                events,
                &entity,
                &scenario_rng,
                is_moving_calls,
                move_random_ranged_calls,
            ));
        }

        events.push(HostTraceEvent::TechnoLatePostMarker);
        events.push(HostTraceEvent::ActiveGate {
            gate: ActiveGate::FootPostTechno,
            pass: gates.foot_post_techno_active,
        });
        if !gates.foot_post_techno_active {
            events.push(HostTraceEvent::FootReturnMarker);
            return Ok(finish_cloned_host_trace(
                events,
                &entity,
                &scenario_rng,
                is_moving_calls,
                move_random_ranged_calls,
            ));
        }

        events.push(HostTraceEvent::FootPreProcessMarker);
        for (index, pass) in gates.foot_process_gates.into_iter().enumerate() {
            events.push(HostTraceEvent::FootProcessGate {
                ordinal: index as u8 + 1,
                pass,
            });
            if !pass {
                events.push(HostTraceEvent::FootLaterWorkMarker);
                events.push(HostTraceEvent::FootReturnMarker);
                return Ok(finish_cloned_host_trace(
                    events,
                    &entity,
                    &scenario_rng,
                    is_moving_calls,
                    move_random_ranged_calls,
                ));
            }
        }

        // This trace records admission only; real Process execution belongs
        // to the live object-turn host and has separate integration coverage.
        if entity.drive_locomotion.is_some() {
            events.push(HostTraceEvent::DriveProcessMarker);
        } else {
            events.push(HostTraceEvent::NullLocomotorInvariant);
            return Ok(finish_cloned_host_trace(
                events,
                &entity,
                &scenario_rng,
                is_moving_calls,
                move_random_ranged_calls,
            ));
        }

        events.push(HostTraceEvent::ActiveGate {
            gate: ActiveGate::FootPostProcess,
            pass: gates.foot_post_process_active,
        });
        if gates.foot_post_process_active {
            events.push(HostTraceEvent::FootLaterWorkMarker);
        }
        events.push(HostTraceEvent::FootReturnMarker);
        Ok(finish_cloned_host_trace(
            events,
            &entity,
            &scenario_rng,
            is_moving_calls,
            move_random_ranged_calls,
        ))
    }

    fn run_inert_ordinary_drive_host_trace(
        sim: &Simulation,
        id: u64,
        mission_control: &MissionControl,
        native_frame: u32,
        gates: HostTraceGates,
    ) -> Result<ClonedHostTrace, HostTraceError> {
        let before = capture_live_host_witness(sim, id);
        let result =
            trace_cloned_ordinary_drive_host(sim, id, mission_control, native_frame, gates);
        let after = capture_live_host_witness(sim, id);
        assert_eq!(
            before, after,
            "the cloned host trace must leave live state inert"
        );
        result
    }

    #[track_caller]
    fn ordinary_drive_host_trace_ok(
        sim: &Simulation,
        native_frame: u32,
        gates: HostTraceGates,
    ) -> ClonedHostTrace {
        run_inert_ordinary_drive_host_trace(
            sim,
            ORDINARY_DRIVE_HOST_ID,
            &stock_move_control(),
            native_frame,
            gates,
        )
        .expect("ordinary Drive host fixture should trace")
    }

    #[track_caller]
    fn assert_ordinary_drive_host_error(
        sim: &Simulation,
        mission_control: &MissionControl,
        native_frame: u32,
        gates: HostTraceGates,
        expected: HostTraceError,
    ) {
        assert_eq!(
            run_inert_ordinary_drive_host_trace(
                sim,
                ORDINARY_DRIVE_HOST_ID,
                mission_control,
                native_frame,
                gates,
            )
            .expect_err("out-of-scope fixture must be rejected"),
            expected
        );
    }

    #[test]
    fn checkpoint_a_ordinary_drive_host_due_move_full_order_is_inert() {
        let sim = ordinary_drive_host_sim(1);
        let native_frame = 100;
        let trace = ordinary_drive_host_trace_ok(&sim, native_frame, HostTraceGates::ordinary());
        let (jitter, raw_advances) = trace
            .events
            .iter()
            .find_map(|event| match event {
                HostTraceEvent::ScenarioRandomRangedApi {
                    low: 0,
                    high: 2,
                    value,
                    raw_advances,
                } => Some((*value, *raw_advances)),
                _ => None,
            })
            .expect("the due Move path makes one ranged call");
        let delay = (STOCK_MOVE_RATE_FRAMES + jitter) as i32;
        assert_eq!(
            trace.events,
            vec![
                HostTraceEvent::TechnoPreThroughRocking,
                HostTraceEvent::ActiveGate {
                    gate: ActiveGate::GuardB,
                    pass: true,
                },
                HostTraceEvent::TechnoRemainingPre,
                HostTraceEvent::MissionAiCounter {
                    before: 0,
                    after: 1,
                },
                HostTraceEvent::MissionDispatchEnter,
                HostTraceEvent::ObjectAiMarker,
                HostTraceEvent::ActiveGate {
                    gate: ActiveGate::Dispatch,
                    pass: true,
                },
                HostTraceEvent::DispatchTimerGate { due: true },
                HostTraceEvent::DispatchHealthGate { pass: true },
                HostTraceEvent::UnitMoveRead6e0 { nonzero: false },
                HostTraceEvent::UnitMoveClear6d2,
                HostTraceEvent::UnitMoveCheckSaved6e0 { nonzero: false },
                HostTraceEvent::UnitMoveCheck {
                    byte: UnitMoveByte::Byte6e1,
                    nonzero: false,
                },
                HostTraceEvent::UnitMoveCheck {
                    byte: UnitMoveByte::Byte6e2,
                    nonzero: false,
                },
                HostTraceEvent::UnitTrackerCheckMarker,
                HostTraceEvent::FootMissionMove,
                HostTraceEvent::NavComCheck { present: true },
                HostTraceEvent::RateLookup {
                    mission: MissionType::Move,
                    frames: STOCK_MOVE_RATE_FRAMES,
                },
                HostTraceEvent::ScenarioRandomRangedApi {
                    low: 0,
                    high: 2,
                    value: jitter,
                    raw_advances,
                },
                HostTraceEvent::DispatchWriteStart {
                    frame: native_frame as i32,
                },
                HostTraceEvent::DispatchWriteScratchMarker,
                HostTraceEvent::DispatchWriteDelay { delay },
                HostTraceEvent::PassiveAcquireMarker,
                HostTraceEvent::BombMarker,
                HostTraceEvent::SlaveManagerMarker,
                HostTraceEvent::CaptureManagerMarker,
                HostTraceEvent::ActiveGate {
                    gate: ActiveGate::GuardE,
                    pass: true,
                },
                HostTraceEvent::TechnoLatePostMarker,
                HostTraceEvent::ActiveGate {
                    gate: ActiveGate::FootPostTechno,
                    pass: true,
                },
                HostTraceEvent::FootPreProcessMarker,
                HostTraceEvent::FootProcessGate {
                    ordinal: 1,
                    pass: true,
                },
                HostTraceEvent::FootProcessGate {
                    ordinal: 2,
                    pass: true,
                },
                HostTraceEvent::FootProcessGate {
                    ordinal: 3,
                    pass: true,
                },
                HostTraceEvent::FootProcessGate {
                    ordinal: 4,
                    pass: true,
                },
                HostTraceEvent::FootProcessGate {
                    ordinal: 5,
                    pass: true,
                },
                HostTraceEvent::DriveProcessMarker,
                HostTraceEvent::ActiveGate {
                    gate: ActiveGate::FootPostProcess,
                    pass: true,
                },
                HostTraceEvent::FootLaterWorkMarker,
                HostTraceEvent::FootReturnMarker,
            ]
        );
        assert_eq!(trace.is_moving_calls, 0);
        assert_eq!(trace.move_random_ranged_calls, 1);
        assert!((14..=16).contains(&delay));
        assert_eq!(trace.mission_after.ai_counter(), 1);
        assert_eq!(
            trace.mission_after.current().known(),
            Some(MissionType::Move)
        );
        assert_eq!(
            trace.mission_after.dispatch_timer(),
            MissionDispatchTimer::from_raw(native_frame as i32, delay)
        );
    }

    #[test]
    fn checkpoint_a_ordinary_drive_host_timer_not_due_still_marks_process() {
        let mut sim = ordinary_drive_host_sim(2);
        update_mission_test_fixture(
            &mut sim
                .substrate
                .entities
                .get_mut(ORDINARY_DRIVE_HOST_ID)
                .unwrap()
                .mission,
            |fixture| fixture.dispatch_timer = MissionDispatchTimer::from_raw(10, 5),
        );
        let trace = ordinary_drive_host_trace_ok(&sim, 14, HostTraceGates::ordinary());

        assert!(
            trace
                .events
                .contains(&HostTraceEvent::DispatchTimerGate { due: false })
        );
        assert!(!trace.events.iter().any(|event| matches!(
            event,
            HostTraceEvent::DispatchHealthGate { .. }
                | HostTraceEvent::UnitMoveRead6e0 { .. }
                | HostTraceEvent::FootMissionMove
                | HostTraceEvent::ScenarioRandomRangedApi { .. }
                | HostTraceEvent::DispatchWriteStart { .. }
                | HostTraceEvent::DispatchWriteScratchMarker
                | HostTraceEvent::DispatchWriteDelay { .. }
        )));
        assert!(trace.events.contains(&HostTraceEvent::DriveProcessMarker));
        assert_eq!(trace.events.last(), Some(&HostTraceEvent::FootReturnMarker));
        assert_eq!(
            trace.mission_after.dispatch_timer(),
            MissionDispatchTimer::from_raw(10, 5)
        );
    }

    #[test]
    fn checkpoint_a_ordinary_drive_host_due_health_failure_skips_handler_and_write() {
        let mut sim = ordinary_drive_host_sim(3);
        sim.substrate
            .entities
            .get_mut(ORDINARY_DRIVE_HOST_ID)
            .unwrap()
            .health
            .current = 0;
        let trace = ordinary_drive_host_trace_ok(&sim, 20, HostTraceGates::ordinary());

        assert!(
            trace
                .events
                .contains(&HostTraceEvent::DispatchHealthGate { pass: false })
        );
        assert!(!trace.events.iter().any(|event| matches!(
            event,
            HostTraceEvent::UnitMoveRead6e0 { .. }
                | HostTraceEvent::FootMissionMove
                | HostTraceEvent::ScenarioRandomRangedApi { .. }
                | HostTraceEvent::DispatchWriteStart { .. }
                | HostTraceEvent::DispatchWriteDelay { .. }
        )));
        assert!(trace.events.contains(&HostTraceEvent::PassiveAcquireMarker));
        assert!(trace.events.contains(&HostTraceEvent::DriveProcessMarker));
        assert_eq!(
            trace.mission_after.dispatch_timer(),
            MissionDispatchTimer::at_frame(0)
        );
    }

    #[test]
    fn checkpoint_a_ordinary_drive_host_dispatch_inactive_propagates_to_foot_return() {
        let sim = ordinary_drive_host_sim(4);
        let mut gates = HostTraceGates::ordinary();
        gates.dispatch_active = false;
        let trace = ordinary_drive_host_trace_ok(&sim, 30, gates);

        assert!(!trace.events.iter().any(|event| matches!(
            event,
            HostTraceEvent::DispatchTimerGate { .. }
                | HostTraceEvent::DispatchHealthGate { .. }
                | HostTraceEvent::UnitMoveRead6e0 { .. }
                | HostTraceEvent::DriveProcessMarker
        )));
        assert!(trace.events.ends_with(&[
            HostTraceEvent::PassiveAcquireMarker,
            HostTraceEvent::BombMarker,
            HostTraceEvent::SlaveManagerMarker,
            HostTraceEvent::CaptureManagerMarker,
            HostTraceEvent::ActiveGate {
                gate: ActiveGate::GuardE,
                pass: false,
            },
            HostTraceEvent::ActiveGate {
                gate: ActiveGate::FootPostTechno,
                pass: false,
            },
            HostTraceEvent::FootReturnMarker,
        ]));
        assert!(!trace.events.contains(&HostTraceEvent::TechnoLatePostMarker));
    }

    #[test]
    fn checkpoint_a_ordinary_drive_host_each_unit_wrapper_byte_queues_guard() {
        let sim = ordinary_drive_host_sim(5);
        for (bytes, expected_checks) in [
            (
                [true, false, false],
                vec![
                    HostTraceEvent::UnitMoveRead6e0 { nonzero: true },
                    HostTraceEvent::UnitMoveClear6d2,
                    HostTraceEvent::UnitMoveCheckSaved6e0 { nonzero: true },
                ],
            ),
            (
                [false, true, false],
                vec![
                    HostTraceEvent::UnitMoveRead6e0 { nonzero: false },
                    HostTraceEvent::UnitMoveClear6d2,
                    HostTraceEvent::UnitMoveCheckSaved6e0 { nonzero: false },
                    HostTraceEvent::UnitMoveCheck {
                        byte: UnitMoveByte::Byte6e1,
                        nonzero: true,
                    },
                ],
            ),
            (
                [false, false, true],
                vec![
                    HostTraceEvent::UnitMoveRead6e0 { nonzero: false },
                    HostTraceEvent::UnitMoveClear6d2,
                    HostTraceEvent::UnitMoveCheckSaved6e0 { nonzero: false },
                    HostTraceEvent::UnitMoveCheck {
                        byte: UnitMoveByte::Byte6e1,
                        nonzero: false,
                    },
                    HostTraceEvent::UnitMoveCheck {
                        byte: UnitMoveByte::Byte6e2,
                        nonzero: true,
                    },
                ],
            ),
        ] {
            let mut gates = HostTraceGates::ordinary();
            gates.unit_move_bytes = bytes;
            let trace = ordinary_drive_host_trace_ok(&sim, 40, gates);
            let observed_checks: Vec<HostTraceEvent> = trace
                .events
                .iter()
                .filter(|event| {
                    matches!(
                        event,
                        HostTraceEvent::UnitMoveRead6e0 { .. }
                            | HostTraceEvent::UnitMoveClear6d2
                            | HostTraceEvent::UnitMoveCheckSaved6e0 { .. }
                            | HostTraceEvent::UnitMoveCheck { .. }
                    )
                })
                .cloned()
                .collect();
            assert_eq!(observed_checks, expected_checks);
            assert!(trace.events.contains(&HostTraceEvent::QueueMissionMarker {
                mission_id: MissionId::from_known(MissionType::Guard),
                arg: 0,
            }));
            assert!(
                trace
                    .events
                    .contains(&HostTraceEvent::DispatchWriteDelay { delay: 1 })
            );
            assert!(!trace.events.contains(&HostTraceEvent::FootMissionMove));
            assert!(
                !trace
                    .events
                    .iter()
                    .any(|event| matches!(event, HostTraceEvent::ScenarioRandomRangedApi { .. }))
            );
            assert!(trace.events.contains(&HostTraceEvent::DriveProcessMarker));
        }
    }

    #[test]
    fn checkpoint_a_ordinary_drive_host_foot_move_branch_matrix_uses_is_moving() {
        let live_nav = ordinary_drive_host_sim(6);
        let live_trace = ordinary_drive_host_trace_ok(&live_nav, 50, HostTraceGates::ordinary());
        assert_eq!(live_trace.is_moving_calls, 0);
        assert_eq!(live_trace.move_random_ranged_calls, 1);

        let mut null_moving = ordinary_drive_host_sim(6);
        null_moving
            .substrate
            .entities
            .get_mut(ORDINARY_DRIVE_HOST_ID)
            .unwrap()
            .navigation
            .nav_com = None;
        let moving_trace =
            ordinary_drive_host_trace_ok(&null_moving, 50, HostTraceGates::ordinary());
        assert_eq!(moving_trace.is_moving_calls, 1);
        assert_eq!(moving_trace.move_random_ranged_calls, 1);
        assert!(
            !moving_trace
                .events
                .iter()
                .any(|event| matches!(event, HostTraceEvent::OnArrivalMarker { .. }))
        );

        let mut null_stopped_queued = ordinary_drive_host_sim(6);
        {
            let entity = null_stopped_queued
                .substrate
                .entities
                .get_mut(ORDINARY_DRIVE_HOST_ID)
                .unwrap();
            entity.navigation.nav_com = None;
            update_mission_test_fixture(&mut entity.mission, |fixture| {
                fixture.queued = MissionId::from_known(MissionType::Guard);
            });
        }
        let mut stopped = HostTraceGates::ordinary();
        stopped.is_moving = false;
        let queued_trace = ordinary_drive_host_trace_ok(&null_stopped_queued, 50, stopped);
        assert_eq!(queued_trace.is_moving_calls, 1);
        assert_eq!(queued_trace.move_random_ranged_calls, 1);
        assert!(
            !queued_trace
                .events
                .iter()
                .any(|event| matches!(event, HostTraceEvent::OnArrivalMarker { .. }))
        );

        let mut null_stopped = ordinary_drive_host_sim(6);
        null_stopped
            .substrate
            .entities
            .get_mut(ORDINARY_DRIVE_HOST_ID)
            .unwrap()
            .navigation
            .nav_com = None;
        let arrived_trace = ordinary_drive_host_trace_ok(&null_stopped, 50, stopped);
        assert_eq!(arrived_trace.is_moving_calls, 1);
        assert_eq!(arrived_trace.move_random_ranged_calls, 0);
        assert!(
            arrived_trace
                .events
                .contains(&HostTraceEvent::OnArrivalMarker { arg0: 0, arg1: 1 })
        );
        assert!(
            arrived_trace
                .events
                .contains(&HostTraceEvent::DispatchWriteDelay { delay: 1 })
        );
    }

    #[test]
    fn checkpoint_a_ordinary_drive_host_null_locomotor_is_invariant() {
        let mut sim = ordinary_drive_host_sim(7);
        {
            let entity = sim
                .substrate
                .entities
                .get_mut(ORDINARY_DRIVE_HOST_ID)
                .unwrap();
            entity.navigation.nav_com = None;
            entity.drive_locomotion = None;
        }
        let trace = ordinary_drive_host_trace_ok(&sim, 60, HostTraceGates::ordinary());
        assert_eq!(
            trace.events.last(),
            Some(&HostTraceEvent::NullLocomotorInvariant)
        );
        assert!(!trace.events.iter().any(|event| matches!(
            event,
            HostTraceEvent::OnArrivalMarker { .. }
                | HostTraceEvent::DispatchWriteStart { .. }
                | HostTraceEvent::PassiveAcquireMarker
                | HostTraceEvent::DriveProcessMarker
                | HostTraceEvent::FootReturnMarker
        )));
        assert_eq!(trace.move_random_ranged_calls, 0);
        assert_eq!(
            trace.mission_after.dispatch_timer(),
            MissionDispatchTimer::at_frame(0)
        );
    }

    #[test]
    fn checkpoint_a_ordinary_drive_host_guard_exits_truncate_exact_segments() {
        let sim = ordinary_drive_host_sim(8);

        let mut guard_b = HostTraceGates::ordinary();
        guard_b.guard_b_active = false;
        let guard_b_trace = ordinary_drive_host_trace_ok(&sim, 70, guard_b);
        assert_eq!(
            guard_b_trace.events,
            vec![
                HostTraceEvent::TechnoPreThroughRocking,
                HostTraceEvent::ActiveGate {
                    gate: ActiveGate::GuardB,
                    pass: false,
                },
                HostTraceEvent::ActiveGate {
                    gate: ActiveGate::FootPostTechno,
                    pass: false,
                },
                HostTraceEvent::FootReturnMarker,
            ]
        );

        let mut guard_e = HostTraceGates::ordinary();
        guard_e.guard_e_active = false;
        guard_e.foot_post_techno_active = true;
        let guard_e_trace = ordinary_drive_host_trace_ok(&sim, 70, guard_e);
        assert!(guard_e_trace.events.ends_with(&[
            HostTraceEvent::ActiveGate {
                gate: ActiveGate::GuardE,
                pass: false,
            },
            HostTraceEvent::ActiveGate {
                gate: ActiveGate::FootPostTechno,
                pass: false,
            },
            HostTraceEvent::FootReturnMarker,
        ]));
        assert!(
            !guard_e_trace
                .events
                .contains(&HostTraceEvent::TechnoLatePostMarker)
        );
        assert!(
            !guard_e_trace
                .events
                .contains(&HostTraceEvent::FootPreProcessMarker)
        );

        let mut foot_guard = HostTraceGates::ordinary();
        foot_guard.foot_post_techno_active = false;
        let foot_trace = ordinary_drive_host_trace_ok(&sim, 70, foot_guard);
        assert!(foot_trace.events.ends_with(&[
            HostTraceEvent::TechnoLatePostMarker,
            HostTraceEvent::ActiveGate {
                gate: ActiveGate::FootPostTechno,
                pass: false,
            },
            HostTraceEvent::FootReturnMarker,
        ]));
        assert!(
            !foot_trace
                .events
                .contains(&HostTraceEvent::FootPreProcessMarker)
        );
    }

    #[test]
    fn checkpoint_a_ordinary_drive_host_each_foot_process_gate_short_circuits() {
        let sim = ordinary_drive_host_sim(10);
        for failed_index in 0..5 {
            let mut gates = HostTraceGates::ordinary();
            gates.foot_process_gates[failed_index] = false;
            let trace = ordinary_drive_host_trace_ok(&sim, 80, gates);
            let observed: Vec<(u8, bool)> = trace
                .events
                .iter()
                .filter_map(|event| match event {
                    HostTraceEvent::FootProcessGate { ordinal, pass } => Some((*ordinal, *pass)),
                    _ => None,
                })
                .collect();
            let expected: Vec<(u8, bool)> = (0..=failed_index)
                .map(|index| (index as u8 + 1, index != failed_index))
                .collect();
            assert_eq!(observed, expected);
            assert!(!trace.events.contains(&HostTraceEvent::DriveProcessMarker));
            assert!(trace.events.ends_with(&[
                HostTraceEvent::FootLaterWorkMarker,
                HostTraceEvent::FootReturnMarker,
            ]));
        }
    }

    #[test]
    fn checkpoint_a_ordinary_drive_host_post_process_guard_uses_epilogue_exit() {
        let sim = ordinary_drive_host_sim(11);
        let mut gates = HostTraceGates::ordinary();
        gates.foot_post_process_active = false;
        let trace = ordinary_drive_host_trace_ok(&sim, 90, gates);
        assert!(trace.events.ends_with(&[
            HostTraceEvent::DriveProcessMarker,
            HostTraceEvent::ActiveGate {
                gate: ActiveGate::FootPostProcess,
                pass: false,
            },
            HostTraceEvent::FootReturnMarker,
        ]));
        assert!(!trace.events.contains(&HostTraceEvent::FootLaterWorkMarker));
    }

    #[test]
    fn checkpoint_a_ordinary_drive_host_rng_rejection_advances_clone_twice() {
        let sim = ordinary_drive_host_sim(9);
        let mut reference = sim.clone_scenario_rng();
        assert_eq!(reference.next_u32() & 3, 3);
        assert_eq!(reference.next_u32() & 3, 0);

        let trace = ordinary_drive_host_trace_ok(&sim, 100, HostTraceGates::ordinary());
        let ranged_events: Vec<(u32, usize)> = trace
            .events
            .iter()
            .filter_map(|event| match event {
                HostTraceEvent::ScenarioRandomRangedApi {
                    value,
                    raw_advances,
                    ..
                } => Some((*value, *raw_advances)),
                _ => None,
            })
            .collect();
        assert_eq!(ranged_events, vec![(0, 2)]);
        assert_eq!(trace.move_random_ranged_calls, 1);
        assert_eq!(trace.scenario_rng_after, reference.logical_state());
        assert_eq!(
            sim.rng_state().scenario,
            sim.clone_scenario_rng().logical_state()
        );
    }

    #[test]
    fn checkpoint_a_ordinary_drive_host_reads_stored_move_not_derived_projection() {
        let sim = ordinary_drive_host_sim(12);
        let entity = sim.substrate.entities.get(ORDINARY_DRIVE_HOST_ID).unwrap();
        assert_eq!(entity.mission.current().known(), Some(MissionType::Move));
        assert!(entity.movement_target.is_none());
        assert!(entity.attack_target.is_none());
        assert!(entity.dock_state.is_none());
        assert!(entity.miner.is_none());

        let trace = ordinary_drive_host_trace_ok(&sim, 110, HostTraceGates::ordinary());
        assert!(trace.events.contains(&HostTraceEvent::FootMissionMove));
        assert_eq!(
            trace.mission_after.current().known(),
            Some(MissionType::Move)
        );
    }

    #[test]
    fn checkpoint_a_shared_drive_host_accepts_ordinary_and_forced_descriptors_without_executing_them()
     {
        for forced in [false, true] {
            let mut sim = ordinary_drive_host_sim(13);
            // The trace-only fixture supplies Logic order but remains in
            // constructor Limbo. Real Force admission requires a revealed
            // owner; publish it through the lifecycle transaction first.
            assert!(matches!(
                sim.reveal(ORDINARY_DRIVE_HOST_ID),
                crate::sim::world::RevealOutcome::Revealed { .. }
            ));
            let head = DriveCoord::cell(8, 7, 731);
            if forced {
                assert!(sim.force_drive_track(ORDINARY_DRIVE_HOST_ID, 0x47, head));
            } else {
                let drive = sim
                    .substrate
                    .entities
                    .get_mut(ORDINARY_DRIVE_HOST_ID)
                    .unwrap()
                    .drive_locomotion
                    .as_mut()
                    .unwrap();
                drive.head_to = Some(head);
                drive.track.turn_index = 0;
                drive.track.cursor = 12;
                drive.track.residual = 5;
            }
            let before = sim
                .substrate
                .entities
                .get(ORDINARY_DRIVE_HOST_ID)
                .unwrap()
                .drive_locomotion
                .clone();
            let trace = ordinary_drive_host_trace_ok(&sim, 120, HostTraceGates::ordinary());
            assert_eq!(
                trace
                    .events
                    .iter()
                    .filter(|event| **event == HostTraceEvent::DriveProcessMarker)
                    .count(),
                1
            );
            assert_eq!(
                sim.substrate
                    .entities
                    .get(ORDINARY_DRIVE_HOST_ID)
                    .unwrap()
                    .drive_locomotion,
                before
            );
        }
    }

    #[test]
    fn checkpoint_a_ordinary_drive_host_rejects_out_of_scope_fixtures() {
        let control = stock_move_control();
        let ordinary = HostTraceGates::ordinary();

        let missing = Simulation::with_seed(13);
        assert_ordinary_drive_host_error(
            &missing,
            &control,
            120,
            ordinary,
            HostTraceError::MissingEntity,
        );

        let mut non_unit = ordinary_drive_host_sim(13);
        non_unit
            .substrate
            .entities
            .get_mut(ORDINARY_DRIVE_HOST_ID)
            .unwrap()
            .category = EntityCategory::Infantry;
        assert_ordinary_drive_host_error(
            &non_unit,
            &control,
            120,
            ordinary,
            HostTraceError::NonUnit,
        );

        let mut non_move = ordinary_drive_host_sim(13);
        update_mission_test_fixture(
            &mut non_move
                .substrate
                .entities
                .get_mut(ORDINARY_DRIVE_HOST_ID)
                .unwrap()
                .mission,
            |fixture| fixture.current = MissionId::from_known(MissionType::Guard),
        );
        assert_ordinary_drive_host_error(
            &non_move,
            &control,
            120,
            ordinary,
            HostTraceError::NonMoveStoredMission,
        );

        let mut low_bridge_tube = ordinary_drive_host_sim(13);
        low_bridge_tube
            .substrate
            .entities
            .get_mut(ORDINARY_DRIVE_HOST_ID)
            .unwrap()
            .low_bridge_tube_state = Some(LowBridgeTubeMovementState {
            tube_id: TubeId(0),
            cursor: 0,
            target: DriveCoord {
                x: 6 * 256 + 128,
                y: 5 * 256 + 128,
                z: 0,
            },
        });
        assert_ordinary_drive_host_error(
            &low_bridge_tube,
            &control,
            120,
            ordinary,
            HostTraceError::ActiveTube,
        );

        let mut miner = ordinary_drive_host_sim(13);
        miner
            .substrate
            .entities
            .get_mut(ORDINARY_DRIVE_HOST_ID)
            .unwrap()
            .miner = Some(Miner::new(MinerKind::War, &MinerConfig::default(), 0));
        assert_ordinary_drive_host_error(
            &miner,
            &control,
            120,
            ordinary,
            HostTraceError::MinerPath,
        );

        let mut dock = ordinary_drive_host_sim(13);
        dock.substrate
            .entities
            .get_mut(ORDINARY_DRIVE_HOST_ID)
            .unwrap()
            .dock_state = Some(DockState {
            dock_building_id: 99,
            phase: DockPhase::Approach,
            service_timer: 0,
            no_funds_ticks: 0,
            enter_retry: Default::default(),
        });
        assert_ordinary_drive_host_error(&dock, &control, 120, ordinary, HostTraceError::DockPath);

        let mut aircraft = ordinary_drive_host_sim(13);
        aircraft
            .substrate
            .entities
            .get_mut(ORDINARY_DRIVE_HOST_ID)
            .unwrap()
            .aircraft_mission = Some(AircraftMission::Guard);
        assert_ordinary_drive_host_error(
            &aircraft,
            &control,
            120,
            ordinary,
            HostTraceError::AircraftPath,
        );

        let mut primary_mismatch = ordinary_drive_host_sim(13);
        primary_mismatch
            .substrate
            .entities
            .get_mut(ORDINARY_DRIVE_HOST_ID)
            .unwrap()
            .locomotor
            .as_mut()
            .unwrap()
            .slot = LocomotorSlot::from_kind(LocomotorKind::Teleport);
        assert_ordinary_drive_host_error(
            &primary_mismatch,
            &control,
            120,
            ordinary,
            HostTraceError::SpecialLocomotorPath,
        );

        let mut piggyback = ordinary_drive_host_sim(13);
        piggyback
            .substrate
            .entities
            .get_mut(ORDINARY_DRIVE_HOST_ID)
            .unwrap()
            .locomotor
            .as_mut()
            .unwrap()
            .begin_piggyback(LocomotorKind::Teleport, MovementLayer::Ground, 0);
        assert_ordinary_drive_host_error(
            &piggyback,
            &control,
            120,
            ordinary,
            HostTraceError::SpecialLocomotorPath,
        );
        // There used to be a second scenario here for the separate "override"
        // mechanism. The two collapsed into one when the piggyback slot became
        // single, so the case it covered is the one directly above.

        let mut class_special = HostTraceGates::ordinary();
        class_special.class_special_pre_foot_path = true;
        assert_ordinary_drive_host_error(
            &ordinary_drive_host_sim(13),
            &control,
            120,
            class_special,
            HostTraceError::ClassSpecialPath,
        );

        let mut lifecycle = HostTraceGates::ordinary();
        lifecycle.lifecycle_countdown_exit = true;
        assert_ordinary_drive_host_error(
            &ordinary_drive_host_sim(13),
            &control,
            120,
            lifecycle,
            HostTraceError::LifecyclePath,
        );

        let mut missing_runtime = ordinary_drive_host_sim(13);
        missing_runtime
            .substrate
            .entities
            .get_mut(ORDINARY_DRIVE_HOST_ID)
            .unwrap()
            .drive_locomotion = None;
        assert_ordinary_drive_host_error(
            &missing_runtime,
            &control,
            120,
            ordinary,
            HostTraceError::MissingDriveRuntime,
        );

        let bad_rate = MissionControl::from_ini(&IniFile::from_str("[Move]\nRate=.017\n"));
        assert_ordinary_drive_host_error(
            &ordinary_drive_host_sim(13),
            &bad_rate,
            120,
            ordinary,
            HostTraceError::StockMoveRate { actual: 15 },
        );
    }

    #[test]
    fn checkpoint_a_ordinary_drive_host_uses_exact_signed_dispatch_timer_domain() {
        for (timer, native_frame, expected_due) in [
            // The unanchored start skips the elapsed subtraction (0x005B308C)
            // but still faces the shared `TEST EAX,EAX` at 0x005B309F with the
            // raw delay, so it is due only when the delay is also zero.
            (MissionDispatchTimer::from_raw(-1, 1), 120, false),
            (MissionDispatchTimer::from_raw(-1, 0), i32::MIN as u32, true),
            (MissionDispatchTimer::from_raw(10, 5), 9, false),
            (
                MissionDispatchTimer::from_raw(i32::MIN, 0),
                i32::MIN as u32,
                true,
            ),
            (MissionDispatchTimer::from_raw(0, 0), i32::MIN as u32, false),
            (MissionDispatchTimer::from_raw(0, i32::MIN), 120, true),
            (MissionDispatchTimer::from_raw(-3, 5), 2, true),
        ] {
            let mut sim = ordinary_drive_host_sim(13);
            update_mission_test_fixture(
                &mut sim
                    .substrate
                    .entities
                    .get_mut(ORDINARY_DRIVE_HOST_ID)
                    .unwrap()
                    .mission,
                |fixture| fixture.dispatch_timer = timer,
            );
            let trace =
                ordinary_drive_host_trace_ok(&sim, native_frame, HostTraceGates::ordinary());
            assert!(
                trace
                    .events
                    .contains(&HostTraceEvent::DispatchTimerGate { due: expected_due }),
                "signed timer {timer:?} at frame {native_frame:#010x}"
            );
            assert_eq!(trace.mission_after.ai_counter(), 1);
            if expected_due {
                assert!(trace.events.contains(&HostTraceEvent::FootMissionMove));
                assert!(trace.events.iter().any(|event| matches!(
                    event,
                    HostTraceEvent::DispatchWriteStart { frame }
                        if *frame == native_frame as i32
                )));
                assert_eq!(
                    trace.mission_after.dispatch_timer().start_frame(),
                    native_frame as i32
                );
                assert!((14..=16).contains(&trace.mission_after.dispatch_timer().delay()));
            } else {
                assert!(!trace.events.iter().any(|event| matches!(
                    event,
                    HostTraceEvent::DispatchHealthGate { .. }
                        | HostTraceEvent::FootMissionMove
                        | HostTraceEvent::DispatchWriteStart { .. }
                )));
                assert_eq!(trace.mission_after.dispatch_timer(), timer);
            }
        }
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

    /// A unit interned through the SIM's interner (survives verb + promotion
    /// paths that resolve the type name).
    fn insert_interned_unit(sim: &mut Simulation, id: u64, rx: u16, ry: u16) {
        let owner = sim.interner.intern("Americans");
        let type_ref = sim.interner.intern("TEST");
        let e = GameEntity::new_at_frame_zero_for_test(
            id,
            rx,
            ry,
            0,
            0,
            owner,
            crate::sim::components::Health { current: 100 },
            type_ref,
            EntityCategory::Unit,
            0,
            5,
            true,
        );
        sim.substrate.entities.insert(e);
    }

    fn insert_interned_aircraft(sim: &mut Simulation, id: u64, rx: u16, ry: u16) {
        let owner = sim.interner.intern("Americans");
        let type_ref = sim.interner.intern("TEST");
        let e = GameEntity::new_at_frame_zero_for_test(
            id,
            rx,
            ry,
            0,
            0,
            owner,
            crate::sim::components::Health { current: 100 },
            type_ref,
            EntityCategory::Aircraft,
            0,
            5,
            true,
        );
        sim.substrate.entities.insert(e);
    }

    /// An interned Infantry carrying a Walk locomotor, so the readiness producer
    /// has a real family to map and the moving gate is live.
    fn insert_interned_walker(sim: &mut Simulation, id: u64, rx: u16, ry: u16) {
        let owner = sim.interner.intern("Americans");
        let type_ref = sim.interner.intern("TEST");
        let mut e = GameEntity::new_at_frame_zero_for_test(
            id,
            rx,
            ry,
            0,
            0,
            owner,
            crate::sim::components::Health { current: 100 },
            type_ref,
            EntityCategory::Infantry,
            0,
            5,
            true,
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
            .in_logic_vector = true;
    }

    /// Supply the retained Walk byte/head and shared Foot fraction read by
    /// native75AB40; a compatibility path alone cannot establish readiness.
    fn set_walking(sim: &mut Simulation, id: u64, walking: bool) {
        let entity = sim.substrate.entities.get_mut(id).expect("fixture entity");
        let head = walking.then(|| crate::sim::components::DriveCoord::cell(6, 5, 0));
        let loco = entity.locomotor.as_mut().unwrap();
        loco.set_step_head(head);
        loco.set_walk_destination(head);
        entity.foot_speed.applied_fraction = if walking {
            crate::util::fixed_math::SIM_ONE
        } else {
            crate::util::fixed_math::SIM_ZERO
        };
        entity.movement_target = walking.then(|| crate::sim::components::MovementTarget {
            path: vec![(5, 5), (6, 5)],
            next_index: 1,
            current_speed: crate::util::fixed_math::SimFixed::from_num(4),
            ..Default::default()
        });
    }

    #[test]
    fn host_promotes_queued_mission() {
        // Queue(Move, 0) at command time, then the host's Ready→Commence
        // promotes it: current=Move, queue cleared, Commence reset applied.
        let rules = promotion_rules();
        let mut sim = Simulation::new();
        insert_interned_unit(&mut sim, 1, 5, 5);
        sim.mission_queue_exact(
            1,
            MissionId::from_known(MissionType::Move),
            0,
            0,
            &crate::sim::mission::authority::EntityReadyInputProvider,
        )
        .unwrap();
        let e = sim.substrate.entities.get(1).unwrap();
        assert_eq!(e.mission.current(), MissionId::NONE);
        assert_eq!(e.mission.queued().known(), Some(MissionType::Move));

        sim.mission_host_promote(1, 7, &rules);

        let e = sim.substrate.entities.get(1).unwrap();
        assert_eq!(e.mission.current().known(), Some(MissionType::Move));
        assert_eq!(e.mission.queued(), MissionId::NONE);
        assert_eq!(e.mission.mission_start_frame(), 7);
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

        sim.mission_queue_exact(
            1,
            MissionId::from_known(MissionType::Move),
            0,
            0,
            &crate::sim::mission::authority::EntityReadyInputProvider,
        )
        .unwrap();

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

        // Retiring the path adapter alone does not finish a paid Walk step.
        sim.substrate.entities.get_mut(1).unwrap().movement_target = None;
        sim.object_ai_post_movement_promote(Some(&rules));
        assert_eq!(
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .mission
                .queued()
                .known(),
            Some(MissionType::Move)
        );

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
        insert_interned_aircraft(&mut sim, 1, 5, 5);
        sim.mission_queue_exact(
            1,
            MissionId::from_known(MissionType::Move),
            0,
            0,
            &crate::sim::mission::authority::EntityReadyInputProvider,
        )
        .unwrap();

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

    /// Readiness can become true during the aircraft's locomotor work. The
    /// same live-object turn must observe that new latch at the post-Foot gate.
    #[test]
    fn gsi_05_06_aircraft_latch_change_promotes_at_post_movement_gate() {
        let rules = promotion_rules();
        let mut sim = Simulation::new();
        insert_interned_aircraft(&mut sim, 1, 5, 5);
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .mission_leaf
            .set_aircraft_transition_ready(0);
        sim.mission_queue_exact(
            1,
            MissionId::from_known(MissionType::Move),
            0,
            0,
            &crate::sim::mission::authority::EntityReadyInputProvider,
        )
        .unwrap();

        sim.object_ai_visit_one(1, Some(&rules), ObjectAiCtx::default());
        let aircraft = sim.substrate.entities.get(1).expect("aircraft present");
        assert_eq!(aircraft.mission.current(), MissionId::NONE);
        assert_eq!(aircraft.mission.queued().known(), Some(MissionType::Move));

        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .mission_leaf
            .set_aircraft_transition_ready(1);
        sim.object_ai_post_movement_promote_one(1, Some(&rules));

        let aircraft = sim.substrate.entities.get(1).expect("aircraft present");
        assert_eq!(aircraft.mission.current().known(), Some(MissionType::Move));
        assert_eq!(aircraft.mission.queued(), MissionId::NONE);
        assert_eq!(
            aircraft.mission.ai_counter(),
            0,
            "post-movement Commence reset must remain final; no second counter tick"
        );
    }

    /// Structures gate inside BuildingClass::Update, not this movement bracket.
    #[test]
    fn post_movement_checkpoint_skips_structures() {
        let rules = promotion_rules();
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        let type_ref = sim.interner.intern("FACT");
        let structure = GameEntity::new_at_frame_zero_for_test(
            2,
            5,
            5,
            0,
            0,
            owner,
            crate::sim::components::Health { current: 100 },
            type_ref,
            EntityCategory::Structure,
            0,
            5,
            true,
        );
        sim.substrate.entities.insert(structure);
        sim.mission_queue_exact(
            2,
            MissionId::from_known(MissionType::Move),
            0,
            0,
            &crate::sim::mission::authority::EntityReadyInputProvider,
        )
        .unwrap();
        let before = sim.substrate.entities.get(2).unwrap().mission;

        sim.object_ai_post_movement_promote_one(2, Some(&rules));

        assert_eq!(sim.substrate.entities.get(2).unwrap().mission, before);
    }

    #[test]
    fn host_promotion_empty_queue_is_noop() {
        let rules = promotion_rules();
        let mut sim = Simulation::new();
        insert_interned_unit(&mut sim, 1, 5, 5);
        let before = sim.substrate.entities.get(1).unwrap().mission;
        sim.mission_host_promote(1, 7, &rules);
        assert_eq!(sim.substrate.entities.get(1).unwrap().mission, before);
    }

    #[test]
    fn host_promotion_holds_on_weapons_factory_contact() {
        // A Unit whose Radio slot 0 is a weapons-factory Building is NOT ready
        // for a non-Move/Enter queued mission (the exact slot-0 hold).
        let rules = promotion_rules();
        let mut sim = Simulation::new();
        insert_interned_unit(&mut sim, 1, 5, 5);
        let owner = sim.interner.intern("Americans");
        let fact_type = sim.interner.intern("FACT");
        let fact = GameEntity::new_at_frame_zero_for_test(
            2,
            6,
            6,
            0,
            0,
            owner,
            crate::sim::components::Health { current: 100 },
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

        sim.mission_queue_exact(
            1,
            MissionId::from_known(MissionType::Guard),
            0,
            0,
            &crate::sim::mission::authority::EntityReadyInputProvider,
        )
        .unwrap();
        sim.mission_host_promote(1, 7, &rules);
        let e = sim.substrate.entities.get(1).unwrap();
        assert_eq!(
            e.mission.current(),
            MissionId::NONE,
            "weapons-factory slot-0 contact holds a queued Guard"
        );
        assert_eq!(e.mission.queued().known(), Some(MissionType::Guard));

        // A queued Move is exempt from the slot-0 hold and promotes.
        sim.mission_queue_exact(
            1,
            MissionId::from_known(MissionType::Move),
            0,
            0,
            &crate::sim::mission::authority::EntityReadyInputProvider,
        )
        .unwrap();
        sim.mission_host_promote(1, 9, &rules);
        let e = sim.substrate.entities.get(1).unwrap();
        assert_eq!(e.mission.current().known(), Some(MissionType::Move));
    }

    #[test]
    fn unit_dispatch_attackmove_unreachable_for_units() {
        // derived_mission never yields AttackMove for any machine combination.
        let mut e = GameEntity::test_default(1, "TEST", "Americans", 5, 5);
        e.movement_target = Some(MovementTarget::default());
        e.attack_target = Some(AttackTarget {
            target: TargetKind::Entity(99),
            pending_infantry_fire: None,
        });
        assert_ne!(e.derived_mission().0, MissionType::AttackMove);
    }

    #[test]
    fn unit_dispatch_preserves_advance_tick_phase_order() {
        fn run() -> Vec<u64> {
            let mut sim = Simulation::new();
            let heights = std::collections::BTreeMap::new();
            (0..5)
                .map(|_| {
                    sim.advance_tick(&[], None, &heights, None, None, 67);
                    sim.state_hash()
                })
                .collect()
        }
        assert_eq!(
            run(),
            run(),
            "advance_tick with the dispatch host stays deterministic"
        );
    }

    // ===== In-loop dispatch authority =====

    /// A scoped moving-unit fixture interned through the simulation's interner,
    /// so it survives a real `advance_tick` (test_intern ids don't exist in
    /// `sim.interner`, and tick-path resolves would panic).
    fn insert_s2_scoped_move_unit(sim: &mut Simulation, id: u64, rx: u16, ry: u16) {
        let owner = sim.interner.intern("Americans");
        let type_ref = sim.interner.intern("TEST");
        let mut e = GameEntity::new_at_frame_zero_for_test(
            id,
            rx,
            ry,
            0, // z = ground level
            0, // facing = north
            owner,
            crate::sim::components::Health { current: 100 },
            type_ref,
            EntityCategory::Unit,
            0, // veterancy = rookie
            5, // vision_range = 5 cells
            true,
        );
        e.movement_target = Some(MovementTarget::default());
        e.drive_locomotion = Some(DriveLocomotionRuntime::default());
        sim.substrate.entities.insert(e);
    }

    /// Post-flip: mission state advances only through the verbs — the tick
    /// never invents a mission from the legacy machines. An uncommanded mover
    /// keeps its selector at none across arrival.
    #[test]
    fn tick_never_projects_a_mission_from_the_machines() {
        let mut sim = Simulation::new();
        insert_s2_scoped_move_unit(&mut sim, 1, 5, 5); // default target: arrives tick 1
        sim.set_logic_order_for_test(vec![1]);
        let heights = std::collections::BTreeMap::new();

        let _ = sim.advance_tick(&[], None, &heights, None, None, 67);
        let e = sim.substrate.entities.get(1).unwrap();
        assert!(e.movement_target.is_none(), "fixture must arrive on tick 1");
        assert_eq!(
            e.mission.current(),
            MissionId::NONE,
            "no verb ran — arrival must not project a mission"
        );

        let _ = sim.advance_tick(&[], None, &heights, None, None, 67);
        let e = sim.substrate.entities.get(1).unwrap();
        assert_eq!(
            e.mission.current(),
            MissionId::NONE,
            "idle ticks must not project a mission either"
        );
        assert_eq!(e.mission.ai_counter(), 2, "the host counter still ticks");
    }

    /// Post-flip corpse freeze: a dying Unit still owns its live scheduler slot,
    /// but only its death action runs there. Its ordinary mission state (counter
    /// included) therefore freezes at death.
    #[test]
    fn dying_unit_mission_state_freezes() {
        let mut sim = Simulation::new();
        insert_s2_scoped_move_unit(&mut sim, 1, 5, 5);
        sim.set_logic_order_for_test(vec![1]);
        sim.object_ai_stage(None);
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().mission.ai_counter(),
            1
        );
        sim.substrate.entities.get_mut(1).unwrap().dying = true;
        sim.object_ai_stage(None);
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().mission.ai_counter(),
            1,
            "a dying Unit's mission state freezes while its death slot runs"
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
        sim.substrate.entities.get_mut(2).unwrap().movement_target = None;
        sim.set_logic_order_for_test(vec![1, 2]);
        let heights = std::collections::BTreeMap::new();

        let _ = sim.advance_tick(&[], None, &heights, None, None, 67);
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().mission.ai_counter(),
            1
        );
        assert_eq!(
            sim.substrate.entities.get(2).unwrap().mission.ai_counter(),
            1
        );
        let _ = sim.advance_tick(&[], None, &heights, None, None, 67);
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
    /// movement_target) must restore an IDENTICAL state hash and value.
    /// Guards the deleted post-load re-derive against reintroduction.
    #[test]
    fn save_load_round_trip_trusts_serialized_mission() {
        use crate::sim::snapshot::GameSnapshot;
        let mut sim = Simulation::new();
        insert_s2_scoped_move_unit(&mut sim, 1, 5, 5);
        sim.substrate.entities.get_mut(1).unwrap().movement_target = None;
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

    /// Two Spark systems, each `Lifetime=5`, so the list-pick (count==2) consumes
    /// a draw and the armed hold is `tick+5` regardless of the picked index.
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
        let owner = sim.interner.intern("Americans");
        let type_ref = sim.interner.intern("CYB");
        let e = GameEntity::new_at_frame_zero_for_test(
            id,
            5,
            5,
            0,
            0,
            owner,
            crate::sim::components::Health { current },
            type_ref,
            EntityCategory::Unit,
            0,
            5,
            true,
        );
        sim.substrate.entities.insert(e);
    }

    fn live_until(sim: &Simulation, id: u64) -> u64 {
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .damage_particle_live_until
    }

    #[test]
    fn s4b_no_draw_above_condition_yellow() {
        // 60/100 = above ConditionYellow (0.5): the outer gate fails → zero draws.
        let rules = cyborg_rules("1.0", "1.0", "SparkA,SparkB", TWO_SPARK_SYSTEMS);
        let mut sim = Simulation::new();
        insert_cyborg_unit(&mut sim, 1, 60);
        let scen = sim.scenario_rng.state();
        let main = sim.main_rng.state();
        techno_common_post(&mut sim, 1, Some(&rules));
        assert_eq!(
            sim.scenario_rng.state(),
            scen,
            "no scenario draw above ConditionYellow"
        );
        assert_eq!(sim.main_rng.state(), main);
        assert_eq!(live_until(&sim, 1), 0);
    }

    #[test]
    fn s4b_one_draw_when_roll_fails() {
        // Below ConditionRed, prob 0.0 → threshold 0 → roll always fails: exactly
        // one draw (the prob-roll), no list-pick, no live system armed.
        let rules = cyborg_rules("0.0", "0.0", "SparkA,SparkB", TWO_SPARK_SYSTEMS);
        let mut sim = Simulation::new();
        insert_cyborg_unit(&mut sim, 1, 20); // below red
        let mut expect = sim.scenario_rng.clone();
        let main = sim.main_rng.state();
        techno_common_post(&mut sim, 1, Some(&rules));
        expect.next_range_u32_inclusive(0, DAMAGE_SPARK_ROLL_MAX);
        assert_eq!(
            sim.scenario_rng.state(),
            expect.state(),
            "exactly one prob-roll draw"
        );
        assert_eq!(
            sim.main_rng.state(),
            main,
            "scenario stream only, never main"
        );
        assert_eq!(live_until(&sim, 1), 0, "roll failed → no live system");
    }

    #[test]
    fn s4b_two_draws_when_roll_succeeds() {
        // prob 1.0 → threshold MAX → roll always succeeds; 2 Spark systems → the
        // list-pick (n(0,1)) consumes a second draw, and the hold arms to tick+5.
        let rules = cyborg_rules("1.0", "1.0", "SparkA,SparkB", TWO_SPARK_SYSTEMS);
        let mut sim = Simulation::new();
        insert_cyborg_unit(&mut sim, 1, 20);
        let tick = sim.session.tick;
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
        assert_eq!(
            live_until(&sim, 1),
            tick + 5,
            "armed to spawn_tick + Lifetime"
        );
    }

    #[test]
    fn s4b_one_draw_when_single_spark_succeeds() {
        // Single Spark system: on success the list-pick is n(0,0) → consumes NO
        // draw (gamemd RandomRanged min==max), so a successful roll is ONE draw.
        let rules = cyborg_rules("1.0", "1.0", "SparkA,SmokeA", ONE_SPARK_ONE_SMOKE_SYSTEMS);
        let mut sim = Simulation::new();
        insert_cyborg_unit(&mut sim, 1, 20);
        let tick = sim.session.tick;
        let mut expect = sim.scenario_rng.clone();
        techno_common_post(&mut sim, 1, Some(&rules));
        expect.next_range_u32_inclusive(0, DAMAGE_SPARK_ROLL_MAX); // roll only
        assert_eq!(
            sim.scenario_rng.state(),
            expect.state(),
            "single-spark success = one draw"
        );
        assert_eq!(
            live_until(&sim, 1),
            tick + 5,
            "armed despite the no-draw list-pick"
        );
    }

    #[test]
    fn s4b_no_draw_while_system_live() {
        // After a successful spawn (live_until = 5 at tick 0), a same-tick re-entry
        // sees the live system (+0x308 != 0) and makes zero draws; advancing past
        // live_until expires it and rolling resumes.
        let rules = cyborg_rules("1.0", "1.0", "SparkA,SparkB", TWO_SPARK_SYSTEMS);
        let mut sim = Simulation::new();
        insert_cyborg_unit(&mut sim, 1, 20);
        techno_common_post(&mut sim, 1, Some(&rules)); // spawn → live_until = 5
        assert_eq!(live_until(&sim, 1), 5);

        let frozen = sim.scenario_rng.state();
        techno_common_post(&mut sim, 1, Some(&rules)); // still tick 0 < 5 → no draw
        assert_eq!(
            sim.scenario_rng.state(),
            frozen,
            "live system blocks the draw"
        );
        assert_eq!(live_until(&sim, 1), 5, "hold unchanged while live");

        // At tick 5 the system has expired: clears and re-rolls (2 draws, re-armed).
        sim.session.tick = 5;
        let mut expect = sim.scenario_rng.clone();
        techno_common_post(&mut sim, 1, Some(&rules));
        expect.next_range_u32_inclusive(0, DAMAGE_SPARK_ROLL_MAX);
        expect.next_range_u32_inclusive(0, 1);
        assert_eq!(
            sim.scenario_rng.state(),
            expect.state(),
            "expiry resumes rolling"
        );
        assert_eq!(live_until(&sim, 1), 10, "re-armed to 5 + Lifetime");
    }

    #[test]
    fn s4b_zero_draw_without_spark_systems() {
        // Below ConditionRed but DamageParticleSystems has no Spark entry: the
        // inner gate (Spark count > 0) fails → zero draws, even at prob 1.0.
        let rules = cyborg_rules("1.0", "1.0", "SmokeA", SMOKE_ONLY_SYSTEMS);
        let mut sim = Simulation::new();
        insert_cyborg_unit(&mut sim, 1, 20);
        let scen = sim.scenario_rng.state();
        techno_common_post(&mut sim, 1, Some(&rules));
        assert_eq!(sim.scenario_rng.state(), scen, "no Spark system → no draw");
        assert_eq!(live_until(&sim, 1), 0);
    }

    #[test]
    fn s4b_dormant_for_non_cyborg_type() {
        // The slice's faithfulness claim: a non-Cyborg type makes zero draws even
        // below ConditionRed with Spark systems, because emits_damage_spark
        // (Type+0xC8F) is false. Here a VEHICLE with `Cyborg=yes` (nonsensical, but
        // it proves the category gate) — gamemd only honours Cyborg on infantry, so
        // its +0xC8F stays 0 and it never sparks. Build rules inline registering the
        // type under [VehicleTypes].
        use crate::rules::ini_parser::IniFile;
        let text = format!(
            "[General]\n\
BuildSpeed=0.75\nMultipleFactory=0.7\nLowPowerPenaltyModifier=1.25\n\
MinLowPowerProductionSpeed=0.4\nMaxLowPowerProductionSpeed=0.85\n\
ConditionRedSparkingProbability=1.0\nConditionYellowSparkingProbability=1.0\n\n\
[InfantryTypes]\n[VehicleTypes]\n1=VEHCYB\n[AircraftTypes]\n[BuildingTypes]\n\n\
[VEHCYB]\nCyborg=yes\nDamageParticleSystems=SparkA,SparkB\n\n{TWO_SPARK_SYSTEMS}\n"
        );
        let rules = RuleSet::from_ini(&IniFile::from_str(&text)).expect("veh rules parse");
        // Sanity: the type parsed as a Cyborg vehicle that nonetheless does NOT emit.
        let obj = rules.object("VEHCYB").expect("VEHCYB present");
        assert!(obj.cyborg, "Cyborg= parsed");
        assert!(
            !obj.emits_damage_spark(),
            "a vehicle never emits AI_Update sparks"
        );

        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        let type_ref = sim.interner.intern("VEHCYB");
        let e = GameEntity::new_at_frame_zero_for_test(
            1,
            5,
            5,
            0,
            0,
            owner,
            crate::sim::components::Health { current: 20 },
            type_ref,
            EntityCategory::Unit,
            0,
            5,
            true,
        );
        sim.substrate.entities.insert(e);
        let scen = sim.scenario_rng.state();
        techno_common_post(&mut sim, 1, Some(&rules));
        assert_eq!(
            sim.scenario_rng.state(),
            scen,
            "non-Cyborg-infantry type makes zero draws"
        );
        assert_eq!(live_until(&sim, 1), 0);
    }

    #[test]
    fn s4b_permanent_hold_blocks_draw() {
        // A spawned spark whose Lifetime <= 0 holds +0x308 indefinitely: live_until
        // = u64::MAX never expires, so the object never rolls again.
        let rules = cyborg_rules(
            "1.0",
            "1.0",
            "SparkA",
            "[ParticleSystems]\n1=SparkA\n\n[SparkA]\nBehavesLike=Spark\nLifetime=-1\n",
        );
        let mut sim = Simulation::new();
        insert_cyborg_unit(&mut sim, 1, 20);
        techno_common_post(&mut sim, 1, Some(&rules)); // success → permanent hold
        assert_eq!(
            live_until(&sim, 1),
            u64::MAX,
            "Lifetime<=0 → indefinite hold"
        );
        sim.session.tick = 1_000_000;
        let frozen = sim.scenario_rng.state();
        techno_common_post(&mut sim, 1, Some(&rules));
        assert_eq!(
            sim.scenario_rng.state(),
            frozen,
            "permanent hold never re-rolls"
        );
    }
}

#[cfg(test)]
#[path = "techno_ai_veterancy_tests.rs"]
mod veterancy_tests;

#[path = "bounce_terrain.rs"]
mod bounce_terrain;

#[cfg(test)]
#[path = "techno_ai/signed_health_tests.rs"]
mod signed_health_tests;
