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
mod common_body;
mod foot_patrol;
pub(crate) use building_missions::AresPrismForwarding;
#[cfg(any(test, feature = "test-support"))]
pub(crate) use building_missions::PendingBuildingFire;
pub(crate) use building_missions::PrismSupport;
pub(crate) use building_missions::queue_and_commence;
pub use building_missions::{BuildingDelayedFire, DelayedFire};
#[cfg(test)]
pub(crate) use common_body::BracketReach;
pub(crate) use common_body::ai_alive;
#[cfg(all(test, tests_sim_world_techno))]
use common_body::allied_target_drop_step;
#[cfg(all(test, tests_sim_world_techno))]
use common_body::behind_indicator_step;
#[cfg(all(test, tests_sim_world_techno))]
use common_body::berserk_step;
use common_body::clear_passive_target_off_mission;
use common_body::drop_airborne_target_step;
use common_body::drop_unsensed_cloaked_target_step;
#[cfg(all(test, tests_sim_world_techno))]
use common_body::healer_target_drop_step;
#[cfg(all(test, tests_sim_world_techno))]
use common_body::house_self_heal_step;
#[cfg(all(test, tests_sim_world_techno))]
use common_body::illegal_target_drop_step;
#[cfg(all(test, tests_sim_world_techno))]
use common_body::iron_tint_step;
use common_body::mission_counter_step;
#[cfg(all(test, tests_sim_world_techno))]
use common_body::self_heal_step;
use common_body::target_reach_step;
use common_body::techno_after_acquisition;
use common_body::techno_common_post;
use common_body::techno_common_steps;
use common_body::unit_techno_bracket;
pub(crate) use foot_patrol::PatrolPost;
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
pub(super) use target_scan::passive_block_acts;
pub(crate) use target_scan::passive_target_acquire;
pub(crate) use target_scan::scan as retaliate_and_scan;
pub(crate) use target_scan::{can_fire_at, weapon_at_index};

use target_scan::{can_acquire_target, passive_acquire_step};

use super::{FrameAdvanceError, techno_ai_cloak, turret_rotate_sound};
use super::{FrameEffects, ObjectKind, ObjectSubstrate, Simulation, bounce_terrain};
use crate::map::entities::EntityCategory;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::game_entity::GameEntity;
#[cfg(test)]
use crate::sim::mission::MissionType;
use crate::sim::world::SimSoundEvent;

/// Non-rules world context the mission handler bodies dispatched from the
/// host need (overlay types and frame effects; handlers read navigation
/// and Terrain state from the Simulation). Empty in
/// barebones fixtures — handlers that need an absent piece degrade the same
/// way the legacy global phases did with `None` arguments.
/// It holds the frame's effects handle by value; callers lend it as
/// `&mut ObjectAiCtx`.
#[derive(Default)]
pub(crate) struct ObjectAiCtx<'a> {
    pub(crate) effects: super::FrameEffects<'a>,
    pub(crate) overlay_registry: Option<&'a OverlayTypeRegistry>,
}

impl ObjectAiCtx<'_> {
    /// The same context for a holder that keeps it by value for one call.
    pub(crate) fn reborrow(&mut self) -> ObjectAiCtx<'_> {
        ObjectAiCtx {
            effects: self.effects.reborrow(),
            overlay_registry: self.overlay_registry,
        }
    }
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
    pub(super) frame_error: Option<super::FrameAdvanceError>,
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

    /// `overlay_registry` reaches the range query an Ares Magnetron asks.
    pub(crate) fn wave_update_context(
        &self,
        wave: &crate::sim::wave::Wave,
        rules: &RuleSet,
    ) -> crate::sim::wave::WaveUpdateContext {
        use crate::sim::combat::TargetKind;
        use crate::sim::projectile::ProjectileCoord;
        let ares_weapon = if rules.ares() {
            self.ares_wave_weapon(wave, rules)
        } else {
            None
        };
        // Ares WaveClass_Update_Beam (Ares.dll 10057290) asks GetFLH for the
        // owner's wave slot (TechnoExt+8), not index 0.
        let flh_index = |entity: &crate::sim::game_entity::GameEntity| {
            ares_weapon.map_or(0, |_| i32::from(entity.ares_wave_weapon_slot()))
        };

        // WaveClass keeps raw owner/target pointers through the represented
        // dying interval. Only PointerExpired makes either identity null.
        let owner = wave
            .owner_id()
            .and_then(|owner_id| self.substrate.entities.get(owner_id));
        // The tracking distance reads Location (`0x00762BB4`), independently
        // of the GetFLH(0, null) geometry call (`0x00762AF0`).
        let owner_position = owner.map(|entity| {
            let location = crate::sim::movement::ground_pose::object_location(
                entity,
                self.resolved_terrain.as_ref(),
            );
            ProjectileCoord::new(location.x, location.y, location.z)
        });
        let owner_current_target = owner.and_then(|entity| entity.attack_target.target());
        let owner_fire_coordinate = owner.and_then(|entity| {
            let object = self.interner.object_type(entity.type_ref(), rules)?;
            // GetFLH(0, null) still mirrors by the owner's current burst
            // parity (+0x3B8, `0x006F3C82`), as FireAt's own call does.
            let burst_index = (entity.seldom.weapon_burst.index() & 1) as u8;
            Some(
                crate::sim::combat::fire_coord::fire_coordinate(
                    self,
                    rules,
                    &crate::sim::combat::fire_coord::FireSource::of_entity(entity),
                    object,
                    flh_index(entity),
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
        let ares = ares_weapon.map(|weapon| {
            use crate::rules::ares_weapon::WaveReverseTarget;
            let target_class = match wave.target_ref() {
                Some(TargetKind::Entity(id)) => {
                    self.substrate
                        .entities
                        .get(id)
                        .map(|entity| match entity.category() {
                            EntityCategory::Unit => WaveReverseTarget::Vehicle,
                            EntityCategory::Aircraft => WaveReverseTarget::Aircraft,
                            EntityCategory::Structure => WaveReverseTarget::Building,
                            EntityCategory::Infantry => WaveReverseTarget::Infantry,
                        })
                }
                _ => None,
            };
            let in_range = match (owner, wave.target_ref(), target_position) {
                (Some(entity), Some(target), Some(target_position)) => {
                    if wave.wave_type() == 3 {
                        target_scan::can_fire_at(
                            self,
                            rules,
                            entity.stable_id(),
                            target,
                            i32::from(entity.ares_wave_weapon_slot()),
                        )
                    } else {
                        // Owner GetCoords (vt+0x48) to the target's vt+0x58:
                        // Range >= sqrt(dx²+dy²+dz²) / sqrt(2), compared
                        // exactly as 2*Range² >= d² (x87 ulp ties aside).
                        crate::sim::movement::ground_pose::target_get_coords(
                            TargetKind::Entity(entity.stable_id()),
                            self,
                            self.resolved_terrain
                                .as_ref()
                                .map(crate::map::resolved_terrain::NativeCellQuery::canonical)
                                .as_ref(),
                        )
                        .is_some_and(|owner| {
                            let squared = [
                                owner.x - target_position.x,
                                owner.y - target_position.y,
                                owner.z - target_position.z,
                            ]
                            .map(|axis| i64::from(axis).pow(2))
                            .iter()
                            .sum::<i64>();
                            let range = i64::from(weapon.range_leptons);
                            range >= 0 && 2 * range * range >= squared
                        })
                    }
                }
                _ => false,
            };
            crate::sim::wave::AresWaveTracking {
                in_range,
                reversed: weapon
                    .ares_visuals
                    .wave_reversed_against(target_class.unwrap_or(WaveReverseTarget::Other)),
            }
        });
        crate::sim::wave::WaveUpdateContext {
            owner_position,
            owner_fire_coordinate,
            owner_current_target,
            target_position,
            target_is_unit,
            ares,
        }
    }

    /// The weapon CreateWave (Ares.dll 10055160) registered in WaveExt for
    /// `wave`; every hooked wave reads it. `None` for a wave without one.
    pub fn ares_wave_weapon<'r>(
        &self,
        wave: &crate::sim::wave::Wave,
        rules: &'r RuleSet,
    ) -> Option<&'r crate::rules::weapon_type::WeaponType> {
        rules.weapon(self.interner.resolve(wave.ares_weapon()?))
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
        self.object_ai_stage_with(rules, &mut ObjectAiCtx::default());
    }

    /// The post-movement Ready→Commence checkpoint.
    ///
    /// `InfantryClass::AI` and `UnitClass::AI` each gate twice per tick, once on
    /// either side of the object's own locomotion, and both checkpoints are the
    /// same pair of virtuals — the readiness predicate at self-vtable `+0x200`
    /// followed by `Commence` at `+0x1ec` when it returns true. Our object-AI
    /// stage is the first checkpoint (it runs immediately before the object's
    /// own ground movement); this is the second.
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
    pub(crate) fn object_ai_post_movement_promote(&mut self, rules: &RuleSet) {
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
    pub(crate) fn object_ai_post_movement_promote_one(&mut self, id: u64, rules: &RuleSet) {
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

    /// `BounceClass::Update @ 0x00439B00` for an `AnimClass` chunk's body over
    /// the live terrain. A body that leaves the verified x87 domain lands.
    pub(crate) fn anim_bounce_update(
        &self,
        body: &mut crate::sim::bounce::BounceState,
        rules: &RuleSet,
    ) -> crate::sim::bounce::BounceOutcome {
        let terrain = bounce_terrain::ResolvedBounceTerrain { sim: self, rules };
        body.update(&terrain).unwrap_or_else(|error| {
            log::warn!("bouncing anim left the verified x87 domain: {error}");
            crate::sim::bounce::BounceOutcome::Stopped
        })
    }

    /// The water test a landing chunk makes: the cell's LandType is Water
    /// (`CellClass+0xEC == 2`, `AnimClass::AI 0x00423C75`).
    pub(crate) fn bounce_cell_is_water(&self, coord: glam::IVec3, rules: &RuleSet) -> bool {
        use crate::sim::bounce::BounceTerrain;
        bounce_terrain::ResolvedBounceTerrain { sim: self, rules }.is_water(coord)
    }

    /// One Bullet LogicVector slot.
    fn visit_logic_bullet(
        &mut self,
        id: u64,
        rules: &RuleSet,
        ctx: &mut ObjectAiCtx<'_>,
    ) -> ObjectAiOutcome {
        let mut outcome = ObjectAiOutcome::visited();
        // BulletClass::AI opens with ObjectClass::AI, which Ares'
        // `ObjectClass_Update_AlphaLight` (`0x005F3E70`, `10047730`) heads.
        if rules.ares() {
            self.update_alpha_shape(id, ObjectKind::Projectile, rules);
        }
        // Ares hook 46670F (1001AC10), at the wait test (0x0046670F).
        if rules.ares()
            && let Some(bullet) = self.projectiles.get(id)
            && let Some(anim) = bullet.awaited_anim().filter(|_| bullet.awaiting_anim())
            && self
                .interner
                .warhead_type(bullet.payload().warhead, rules)
                .and_then(|warhead| warhead.ares_rules())
                .is_some_and(|ares| ares.combat().pre_impact_anim_moves())
            && let Some(at) = self.anim_absolute_coord(anim)
        {
            let target = crate::sim::projectile::ProjectileTarget::looked_up_cell(
                self.resolved_terrain.as_ref(),
                (at.x / 256) as u16,
                (at.y / 256) as u16,
            );
            self.projectiles.follow_awaited_anim(
                id,
                crate::sim::projectile::ProjectileCoord::new(at.x, at.y, at.z),
                target,
            );
        }
        let head = self
            .projectiles
            .begin_ai_visit(id)
            .expect("live Bullet has an AI prefix");
        // Bullet466826..4668B8 constructs at the old Location after its
        // visual update, before target/floor queries or flight. Registering
        // an Anim here also preserves the live Logic cursor on impact.
        let projectile_type = self
            .projectiles
            .get(id)
            .and_then(|projectile| projectile.payload().projectile_type())
            .and_then(|type_id| self.interner.projectile_type(type_id, rules));
        let trailer = projectile_type.and_then(|kind| {
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
        let projectile = self
            .projectiles
            .get(id)
            .expect("Bullet remains registered across its Trailer constructor");
        // BulletClass::AI @ 0x00467C3C reads the live firer (+0xB0),
        // then its +0x84 TechnoType receiver and JumpJet (+0xD94).
        // Do not cache this on launch: PointerExpired can null the firer.
        let source_is_jumpjet = self
            .substrate
            .entities
            .get(projectile.source_id())
            .is_some_and(|source| {
                self.object_type(source.type_ref(), rules)
                    .is_some_and(|kind| kind.jumpjet)
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
        let house_alliances = &self.house_alliances;
        let collision_world = super::projectile_collision::ProjectileCollisionWorld {
            terrain,
            dummy: &shared_cell_dummy,
            occupancy,
            entities,
            terrain_objects: self.terrain_objects.objects(),
            interner,
            alliances: house_alliances,
            overlays: overlay_grid,
            rules,
            map_size: self.map_size_diamond(),
            ares_firestorm: &self.ares_firestorm,
        };
        let nuke_ball_type = || crate::sim::superweapon::nuke::nuke_ball_type(rules);
        let ares_ball = |warhead, nuke_sw| {
            crate::sim::superweapon::nuke::ares_nuke_ball(rules, interner, warhead, nuke_sw)
        };
        let nuke = Some(crate::sim::projectile::NukeImpactContext::new(
            interner,
            &nuke_ball_type,
        ));
        let nuke = if rules.ares() {
            nuke.map(|nuke| nuke.with_ares(&ares_ball))
        } else {
            nuke
        };
        let result = self
            .projectiles
            .advance_after_head(
                head,
                binary_frame,
                |target, query| collision_world.target_coordinate(target, query),
                terrain,
                &shared_cell_dummy,
                rules.general.gravity,
                source_is_jumpjet,
                target_is_aircraft,
                rules.general.safety_altitude,
                projectile_type.is_some_and(|kind| kind.degenerates),
                |projectile, candidate, phase| {
                    collision_world.collide(projectile, candidate, phase)
                },
                nuke.as_ref(),
            )
            .expect("projectile remained present for its Logic slot");
        let terminal = !result.expired.is_empty() || !result.detonations.is_empty();
        if rules.ares() && result.ares_firestormed {
            self.ares_burn_flying_bullet(id, rules);
        }
        // The rest of a `NUKE` impact precedes the slot's detonations.
        for &impact in result.nuke_impacts() {
            crate::sim::superweapon::nuke::impact(self, rules, impact);
        }
        outcome.bridge_state_changed |= self.commit_logic_projectile_detonations(
            rules,
            ctx.overlay_registry,
            &result.detonations,
            &mut ctx.effects,
        );
        if terminal {
            let retired = self.retire_non_entity_object(
                id,
                super::UninitContext::new(rules).with_effects(&mut ctx.effects),
            );
            debug_assert!(retired);
        }
        outcome
    }

    /// One Wave LogicVector slot.
    fn visit_logic_wave(
        &mut self,
        id: u64,
        rules: &RuleSet,
        ctx: &mut ObjectAiCtx<'_>,
    ) -> ObjectAiOutcome {
        // Ares WaveClass_Update (Ares.dll 100571A0, hook 0x760F50) replaces
        // WaveClass::AI: a wave whose weapon has AmbientDamage first damages
        // the cells of its previous update, whatever its type; the update and
        // lifecycle follow with the post-damage world.
        if rules.ares()
            && let Some(request) = self
                .ares_wave_weapon(self.waves.get(id).expect("represented Wave"), rules)
                .filter(|weapon| weapon.ambient_damage != 0)
                .and_then(|_| self.waves.ares_damage_request(id))
        {
            self.commit_logic_wave_damage_request(
                rules,
                ctx.overlay_registry,
                &request,
                &mut ctx.effects,
            );
        }
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
        if let Some(request) = request {
            self.commit_logic_wave_damage_request(
                rules,
                ctx.overlay_registry,
                &request,
                &mut ctx.effects,
            );
        }
        if !result.alive {
            let retired = self.retire_non_entity_object(
                id,
                super::UninitContext::new(rules).with_effects(&mut ctx.effects),
            );
            debug_assert!(retired);
        }
        ObjectAiOutcome::visited()
    }

    /// Dispatch one current LogicVector slot. A finishing death sequence calls
    /// UnInit synchronously here, so compacting removal is visible before the
    /// scheduler increments its cursor.
    pub(super) fn object_ai_visit_one_with_effects(
        &mut self,
        id: u64,
        rules: &RuleSet,
        ctx: &mut ObjectAiCtx<'_>,
    ) -> ObjectAiOutcome {
        let mut outcome = ObjectAiOutcome::visited();
        let kind = self.classify_object(id);
        // These kinds' whole turn is this dispatch.
        let _area = {
            use crate::sim::frame_profile::{area, phase};
            match kind {
                Some(ObjectKind::Projectile | ObjectKind::Wave) => Some(area(phase::BULLETS)),
                Some(
                    ObjectKind::Anim
                    | ObjectKind::VoxelAnim
                    | ObjectKind::Spotlight
                    | ObjectKind::ParticleSystem,
                ) => Some(area(phase::EFFECTS)),
                _ => None,
            }
        };
        match kind {
            Some(ObjectKind::Entity) => {}
            Some(ObjectKind::Anim) => {
                match self.visit_anim(id, rules, ctx.overlay_registry, &mut ctx.effects) {
                    Ok(changed) => outcome.bridge_state_changed = changed,
                    Err(error) => {
                        outcome.frame_error = Some(super::FrameAdvanceError::new(
                            self.session.tick(),
                            self.session.binary_frame(),
                            id,
                            error.to_string(),
                        ))
                    }
                }
                return outcome;
            }
            Some(ObjectKind::Spotlight) => {
                self.visit_spotlight(
                    id,
                    rules,
                    super::UninitContext::new(rules).with_effects(&mut ctx.effects),
                );
                return outcome;
            }
            Some(ObjectKind::VoxelAnim) => {
                match self.visit_voxel_anim(id, rules, &mut ctx.effects) {
                    Ok(changed) => outcome.bridge_state_changed = changed,
                    Err(error) => {
                        outcome.frame_error = Some(super::FrameAdvanceError::new(
                            self.session.tick(),
                            self.session.binary_frame(),
                            id,
                            error.to_string(),
                        ))
                    }
                }
                return outcome;
            }
            Some(ObjectKind::ParticleSystem) => {
                crate::sim::particles::system_ai::tick_particle_system(
                    self,
                    rules,
                    id,
                    super::UninitContext::new(rules).with_effects(&mut ctx.effects),
                );
                return outcome;
            }
            Some(ObjectKind::Terrain) => {
                // TerrainClass::AI @ 0x0071C730 calls ObjectClass::AI first
                // (`0x0071C738`), whose entry Ares hooks
                // (`ObjectClass_Update_AlphaLight`).
                if rules.ares() {
                    self.update_alpha_shape(id, ObjectKind::Terrain, rules);
                }
                self.object_ai_sounds(id, rules, &mut ctx.effects);
                if let Err(error) = crate::sim::terrain_spawn::tick_terrain_object_ai(
                    self,
                    id,
                    rules,
                    ctx.overlay_registry,
                    &mut ctx.effects,
                ) {
                    outcome.frame_error = Some(super::FrameAdvanceError::new(
                        self.session.tick(),
                        self.session.binary_frame(),
                        id,
                        error.to_string(),
                    ));
                }
                return outcome;
            }
            Some(ObjectKind::Projectile) => return self.visit_logic_bullet(id, rules, ctx),
            Some(ObjectKind::Wave) => return self.visit_logic_wave(id, rules, ctx),
            // Neither joins Logic (`ObjectKind::joins_logic`).
            Some(ObjectKind::Smudge | ObjectKind::Particle) | None => {
                return ObjectAiOutcome::default();
            }
        }

        // Building43FB20 samples its operational edge before delayed Health0
        // cleanup. A live dying-animation diversion must not skip that edge.
        self.visit_building_operational(id, rules, ctx.overlay_registry, &mut ctx.effects);
        // `BuildingClass_AI_Radiation @ 0x43FB23` follows that edge sample and
        // runs for every live building each frame. Phobos owns the branch;
        // retail leaves buildings untouched by radiation.
        #[cfg(feature = "phobos")]
        if self
            .substrate
            .entities
            .get(id)
            .is_some_and(|entity| entity.category() == EntityCategory::Structure)
            && let Some(receipt) = crate::sim::combat::world_receiver::apply_building_radiation(
                self,
                rules,
                id,
                ctx.overlay_registry,
                &mut ctx.effects,
            )
        {
            outcome.bridge_state_changed |= receipt.bridge_state_changed;
            outcome.destroyed_structure |= receipt.structure_destroyed;
        }
        // `UnitClass::AI` opens with an offline Unit's sparks
        // (`0x007360C9..0x007361A4`), ahead of its tube and Temporal tests.
        if self
            .substrate
            .entities
            .get(id)
            .is_some_and(|entity| entity.category() == EntityCategory::Unit)
        {
            self.deactivated_unit_sparks(id, rules);
            // `0x007361A9..0x00736204`, right after the sparks and ahead of
            // Temporal, the tube test and FootClass::AI: a draining Floating
            // Disc re-checks the building under it every 16th frame and drops
            // the link when it has drifted off, then removes a ray left with
            // no link. A quiet unit drains nothing (`world::quiet_units`).
            if !self.quiet(id) {
                crate::sim::credit_income::drain_unit_ai_step(self, id, rules);
            }
        }
        let Some(entity) = self.substrate.entities.get(id) else {
            return ObjectAiOutcome::default();
        };
        // A member past its UnInit (`+0x90` clear) that Logic still holds: a
        // rider killed inside an open-topped transport stays in the vector
        // until its destructor (`0x005F3D75`). `FootClass::AI @ 0x004DA530`
        // returns at its IsAlive test (`0x004DA53E`), as do the class bodies
        // after it (`0x0051BCA4`, `0x007365BB`, `0x00414DA8`).
        // Residual: `TechnoClass::AI @ 0x006F9E50` runs for it first, up to
        // its IsAlive test at `0x006FA735` (`0x006FA23C` for a voxel body):
        // the target-validity drops, the `+0xC4` counter, MissionClass::AI's
        // sound block, the Slave and Capture manager updates, and the passive
        // scan, which for a rider on Guard with its scan timer due writes
        // that timer and draws from the Scenario RNG (`0x00709820`). VERA
        // runs none of them. Trigger: a transport dies ahead of its rider's
        // Logic slot with that timer due; effect: the Scenario stream differs
        // from gamemd's by those draws, every other write lands on an object
        // freed at this frame's drain; rare; VERA peers stay in step.
        if !entity.lifecycle.object_alive() {
            return outcome;
        }
        // An infantry corpse is no dying object: it keeps IsAlive at Health 0
        // and takes the ordinary visit below (`world::infantry_death`).
        if entity.death.is_dying() {
            let sequence_set = self.interner.animation_sequence(entity.type_ref(), rules);
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
                    super::UninitContext::new(rules).with_effects(&mut ctx.effects),
                );
            }
            return outcome;
        }

        // UnitClass::AI / InfantryClass::AI test the object-owned TubeMovement
        // index before entering the ordinary Foot/mission body.  The active
        // leaf runs later in this same LogicVector slot (the world host owns
        // the mutable movement substrates), so this visit must contribute no
        // mission-counter, queued-mission, cadence, or passive-target work.
        // A quiet unit holds no tube (`world::quiet_units`).
        if matches!(
            entity.category(),
            EntityCategory::Unit | EntityCategory::Infantry
        ) && !self.quiet(id)
            && entity.active_tube().is_some()
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
        ctx: &mut ObjectAiCtx<'_>,
    ) -> bool {
        self.object_ai_visit_one_with_effects(
            id,
            rules.unwrap_or(crate::rules::ruleset::RuleSet::empty_for_test()),
            ctx,
        )
        .visited
    }

    /// [`Simulation::object_ai_stage`] with the world context the dispatched
    /// mission handler bodies need (the production spine entry).
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn object_ai_stage_with(
        &mut self,
        rules: Option<&RuleSet>,
        ctx: &mut ObjectAiCtx<'_>,
    ) {
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
        ctx: &mut ObjectAiCtx<'_>,
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
        if let Some(ammo) = self
            .entities
            .get_mut_if(id, |entity| {
                entity.category() == EntityCategory::Aircraft && entity.mission.current().raw() != 1
            })
            .and_then(GameEntity::ammo_mut)
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
    rules: &RuleSet,
    ctx: &mut ObjectAiCtx<'_>,
    outcome: &mut ObjectAiOutcome,
) {
    // AircraftClass::AI's first act, ahead of its TemporalClass::Update
    // (`0x00414BB9..0x00414BD4`): the `+0x6D2` latch clear.
    if category == EntityCategory::Aircraft
        && let Some(entity) = sim.substrate.entities.get_mut(id)
    {
        crate::sim::aircraft::ai_head_latch_clear(entity);
    }
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
    if category == EntityCategory::Structure {
        sim.update_building_damage_fire(id, rules);
        turret_rotate_sample = sim.sample_building_turret_rotation(id, rules);
    }
    // A quiet unit is neither warped nor warping (`world::quiet_units`).
    let temporal_frozen = !sim.quiet(id)
        && sim.temporal_ai_prologue(id, rules, ctx, &mut outcome.bridge_state_changed);
    // Phobos `ShieldClass::AI_Temporal` pauses recovery during the Temporal
    // prologue; frozen leaves never reach the normal `ShieldClass::AI` at
    // `TechnoClass_AI @ 0x6F9E50+0x5`. The shield owner handles that paused
    // visit or the normal recovery visit after this frame's warp update.
    #[cfg(feature = "phobos")]
    crate::sim::shield::tick(sim, id, rules);
    if temporal_frozen {
        return;
    }
    // Unit73635B..7363A3: the art `DeathFrames=` count, which may end it.
    if category == EntityCategory::Unit
        && sim.unit_death_count(id, rules, ctx.overlay_registry, &mut ctx.effects)
    {
        return;
    }
    // Unit7363C9..7363DE precedes Hunt/Ready/Foot AI, after the existing
    // Tube, death and Temporal entry gates. Limboed Units have no Logic visit.
    if category == EntityCategory::Unit
        && let Some(entity) = sim
            .substrate
            .entities
            .get_mut_if(id, |entity| entity.unit_firing_counter.counting())
    {
        crate::sim::animation::tick_unit_firing_counter(entity);
    }
    // Unit736465/736473 and Infantry51BC1C/51BC51 run Ready/Commence
    // before Foot -> Techno AI. Commence clears+C4; Techno6FA646..655 then
    // increments it and dispatches. Native Anytown mission_order first visit
    // ends at counter1. Promoting at the dispatch point erased that increment
    // and let Techno's preceding readers observe the previous mission.
    if matches!(category, EntityCategory::Unit | EntityCategory::Infantry) {
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
    // `TechnoClass_AI @ 0x6F9E50+0x5 -> TechnoExt::OnEarlyUpdate ->
    // ApplyMindControlRangeLimit`, after the leaf's own steps and before the
    // common Techno body's estimated-health recover.
    #[cfg(feature = "phobos")]
    sim.apply_mind_control_range_limit(id, rules, ctx.overlay_registry, &mut ctx.effects);
    // Techno6F9F6E..9F precedes veterancy promotion, missions and acquisition for every
    // Techno category. object_ai_visit_one excludes the entry-active Tube leaf
    // before this common owner. Actual health and this retained estimate differ.
    let frame = sim.session.binary_frame();
    if let Some(entity) = sim.substrate.entities.get_mut_if(id, |entity| {
        entity
            .estimated_health
            .recovers(entity.health.current(), frame)
    }) {
        entity
            .estimated_health
            .recover(entity.health.current(), frame);
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
            if !techno_common_steps(sim, id, rules, ctx.overlay_registry, &mut ctx.effects, None) {
                return;
            }
            clear_passive_target_off_mission(sim, id, rules);
            if !mission_counter_step(sim, id, rules, ctx, outcome) {
                return;
            }
            match dispatch_foot_mission(sim, id, rules, ctx) {
                Ok(changed) => outcome.bridge_state_changed |= changed,
                Err(error) => {
                    outcome.frame_error = Some(super::FrameAdvanceError::new(
                        sim.session.tick(),
                        sim.session.binary_frame(),
                        id,
                        error.to_string(),
                    ));
                    return;
                }
            }
            passive_acquire_step(sim, id, rules, ctx);
            if !techno_after_acquisition(sim, id, rules, ctx, outcome) {
                return;
            }
            target_reach_step(sim, id, rules, ctx.overlay_registry, &mut ctx.effects);
            techno_common_post(sim, id, rules, &mut ctx.effects);
        }
        EntityCategory::Structure => {
            // Building43FC39 damage-fire (run above, before the warp
            // update) and43FD2C ProduceCash precede the shared Techno call
            // at43FE56. A yellow-crossing selfheal must not retire damage
            // fire one native frame early.
            crate::sim::credit_income::produce_cash_step(sim, id, rules);
            // UpdateAnimation's depot raise (`0x00450A86..0x00450B34`) runs
            // just before its InfantryAbsorb block (`0x00450B34`).
            sim.update_depot_raise_anim(id, rules);
            sim.update_building_absorb_anim(id, rules);
            sim.update_building_storage_anims(id, rules);
            // UpdateAnimation's slot-anim light (`0x00450A47..0x00450A77`).
            sim.substrate
                .relight_building_anims(id, sim.session.binary_frame());
            building_missions::update_animation(sim, id, rules);
            // The ready check after UpdateAnimation (`0x0043FE27`).
            building_missions::ready_commence(&sim.session, &mut sim.substrate, id, true);
            if !techno_common_steps(
                sim,
                id,
                rules,
                ctx.overlay_registry,
                &mut ctx.effects,
                Some(turret_rotate_sample),
            ) {
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
            techno_common_post(sim, id, rules, &mut ctx.effects);
            // Ares BuildingClass_Update_SensorArray10016E30 (hook43FE69),
            // after Update's IsAlive test.
            if rules.ares() && ai_alive(&sim.substrate, id) {
                sim.update_ares_sensor_array(id, rules, ctx.overlay_registry);
            }
            building_missions::gattling_idle(
                &sim.interner,
                &sim.session,
                &mut sim.substrate,
                id,
                rules,
            );
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
            // Steam gamemd control: object_health.py
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
                        .building
                        .pending_c4_detonation
                        .pending()
                        .is_none_or(|pending| {
                            pending.timer().remaining(sim.session.binary_frame() as i32) == 0
                        })
                });
                if ready {
                    let mut context =
                        super::UninitContext::new(rules).with_effects(&mut ctx.effects);
                    sim.techno_limbo_with_context(id, context.reborrow());
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
                        context.effects(),
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
            building_missions::process_delayed_fire(
                sim,
                id,
                rules,
                ctx,
                &mut outcome.bridge_state_changed,
            );
            Simulation::update_tesla_charge(
                &sim.interner,
                &mut sim.substrate,
                &sim.power_states,
                id,
                rules,
            );
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
                        &mut ctx.effects,
                    );
            }
            crate::sim::production::update_repair_and_power(
                sim,
                rules,
                id,
                ctx.overlay_registry,
                &mut ctx.effects,
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
                    &mut ctx.effects,
                );
            }
            // BuildingClass::Update consumes the shared C4/PostMortem latch at
            // its late tail. Keep the forced receiver inline in this object's
            // LogicVector visit so nested death effects precede the next slot.
            outcome.bridge_state_changed |= sim.tick_pending_building_detonation(
                id,
                rules,
                ctx.overlay_registry,
                &mut ctx.effects,
            );
            // Ares BuildingClass_Update_FirestormWall (10014B40, hook
            // 0x440378) ahead of the range drop: a `Firestorm.Wall` section's
            // flicker and victims (`UpdateFirewall(false)`).
            if rules.ares()
                && sim.substrate.entities.get(id).is_some_and(|entity| {
                    sim.object_type(entity.type_ref(), rules)
                        .is_some_and(|object| object.building_type_ext().firestorm_wall)
                })
            {
                sim.ares_update_firewall(id, false, rules, ctx.overlay_registry, &mut ctx.effects);
            }
            // The range drop that ends the Update (`0x00440378`).
            building_missions::range_drop(sim, id, rules, ctx);
        }
        // AircraftClass::AI reaches the shared Foot/mission work before its
        // locomotor, but promotes via Ready→Commence only after Foot returns
        // (`0x0041504A`/`0x00415058`). Keep the counter here; the sole promotion
        // is `object_ai_post_movement_promote_one`.
        //
        // AircraftClass::AI drops an airborne or unsensed cloaked Target, then
        // reaches the common Techno AI body through FootClass::AI (`0x00414DA3`), so the
        // off-mission clear, the counter, MissionClass::AI and the passive
        // block run in AI_Update's order (`0x006FA5E8..0x006FA6EE`). The
        // retail fixed-wing types are `CanPassiveAquire=no`, so the block's
        // gate refuses them except on Move in a computer house's
        // `Aggressive=yes`, `Suicide=no` team (Korea's Black Eagles). A scan
        // takes nothing for an aircraft: with no `+0x3C4` override its mask 1
        // gives a zero flags word, which the class gate (`0x006F821A`) refuses
        // for every candidate; the scan's Scenario draw and timer remain.
        EntityCategory::Aircraft => {
            // Ares 3.0 AACombat (1005C190, hook 0x414D36) jumps past the
            // airborne drop to the cloaked-target test: an aircraft keeps an
            // airborne Target.
            let dropped =
                !rules.ares() && drop_airborne_target_step(sim, id, rules, &mut ctx.effects);
            if !dropped {
                drop_unsensed_cloaked_target_step(sim, id, rules, &mut ctx.effects);
            }
            if !techno_common_steps(sim, id, rules, ctx.overlay_registry, &mut ctx.effects, None) {
                return;
            }
            clear_passive_target_off_mission(sim, id, rules);
            if !mission_counter_step(sim, id, rules, ctx, outcome) {
                return;
            }
            // MissionClass::AI, inside this slot and before Fly Process
            // (FootClass::AI4DA530). A Mission_Attack strike fires here.
            if let Some(receipt) = crate::sim::aircraft::dispatch_mission(sim, id, rules, ctx) {
                outcome.bridge_state_changed |= receipt.bridge_state_changed;
                outcome.destroyed_structure |= receipt.structure_destroyed;
            }
            passive_acquire_step(sim, id, rules, ctx);
            if !techno_after_acquisition(sim, id, rules, ctx, outcome) {
                return;
            }
            techno_common_post(sim, id, rules, &mut ctx.effects);
        }
    }
}

#[cfg(all(test, tests_sim_world_techno))]
mod tests;

#[cfg(all(test, tests_sim_world_techno))]
#[path = "techno_ai_selfheal_tests.rs"]
mod selfheal_tests;

#[cfg(all(test, tests_sim_world_techno))]
#[path = "techno_ai_selfheal_oracle_tests.rs"]
mod selfheal_oracle_tests;

#[cfg(all(test, tests_sim_world_techno))]
#[path = "techno_ai/signed_health_tests.rs"]
mod signed_health_tests;

#[cfg(all(test, tests_sim_world_techno))]
#[path = "techno_ai/ares_self_heal_tests.rs"]
mod ares_self_heal_tests;

#[cfg(test)] // shared by test groups
#[path = "techno_ai/bridge_target_layer_tests.rs"]
mod bridge_target_layer_tests;

#[cfg(test)] // shared by test groups
#[path = "techno_ai/bridge_engineer_entry_tests.rs"]
mod bridge_engineer_entry_tests;

#[cfg(all(test, tests_sim_world_techno))]
#[path = "techno_ai/bridge_low_repair_tests.rs"]
mod bridge_low_repair_tests;

#[cfg(all(test, tests_sim_world_techno))]
#[path = "techno_ai/mirage_disguise_tests.rs"]
mod mirage_disguise_tests;

#[cfg(any(test, feature = "test-support"))]
use super::bridge_test_evidence;

#[cfg(all(test, tests_sim_world_techno))]
#[path = "techno_ai/promotion_sound_tests.rs"]
mod promotion_sound_tests;

#[cfg(all(test, tests_sim_world_techno))]
#[path = "techno_ai/surface_conversion_tests.rs"]
mod surface_conversion_tests;
