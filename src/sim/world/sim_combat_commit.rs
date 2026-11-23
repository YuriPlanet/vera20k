//! `Simulation` combat commits: fatal lifecycle stages, fire visits, wave and
//! direct damage receivers, cliff collapse, and under-attack and unit-lost events.

#[cfg(test)]
use std::collections::BTreeSet;

use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::ruleset::RuleSet;
use crate::sim::house_diplomacy::HouseAlliances;
use crate::sim::houses::Houses;
use crate::sim::intern::InternedId;
use crate::sim::intern::StringInterner;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::passenger;
use crate::sim::projectile::ProjectileCoord;
use crate::sim::radar::RadarEventRequest;
use crate::sim::radar::RadarEventType;
use crate::sim::scenario_session::ScenarioSession;
use crate::util::fixed_math::SimFixed;

use super::AresUnitLostLine;
use super::FrameChannel;
use super::FrameEffects;
#[cfg(any(test, feature = "test-support"))]
use super::LifecycleTestEvent;
use super::SimFireEvent;
use super::SimSoundEvent;
use super::Simulation;
use super::SimulationWallRuntimeHost;
use super::UninitContext;
use super::damage_consequences;
#[cfg(any(test, feature = "test-support"))]
use super::techno_ai;

impl Simulation {
    /// The live CellClass level of a cell in the terrain the simulation owns,
    /// or `None` without terrain or off the map. Bridge-body and cliff writes
    /// update it, so readers never see a load-time copy.
    pub(crate) fn terrain_cell_level(&self, rx: u16, ry: u16) -> Option<u8> {
        Some(self.resolved_terrain.as_ref()?.cell(rx, ry)?.level)
    }

    /// Resolve the CellClass identity returned by MapClass::Get_CellClass and
    /// then dispatch its live GetTargetCoords virtual. Fixed-stride aliases
    /// therefore use the returned real CellClass's canonical coordinate, while
    /// misses retain and restamp the one process-global dummy identity.
    pub(super) fn wave_cell_target_position(&self, rx: u16, ry: u16) -> ProjectileCoord {
        self.wave_cell_target_position_in(self.resolved_terrain.as_ref(), rx, ry)
    }

    fn wave_cell_target_position_in(
        &self,
        terrain: Option<&ResolvedTerrainGrid>,
        rx: u16,
        ry: u16,
    ) -> ProjectileCoord {
        use crate::sim::cell_rect::{CellRef, get_cellclass_fallback};

        match get_cellclass_fallback(terrain, i32::from(rx), i32::from(ry)) {
            CellRef::Real(cell) => {
                crate::sim::projectile::cell_target_coord(terrain, cell.rx, cell.ry)
            }
            CellRef::Dummy { cell } => {
                let live_dummy = if terrain.is_some() {
                    cell
                } else {
                    // A mapless lookup still addresses Simulation's retained
                    // process dummy. The fallback supplies native i16 packing;
                    // restamp only its coordinate so live level/slope/bridge
                    // bytes remain authoritative.
                    let coord = cell.snapshot().coord;
                    let live_dummy = self.effective_shared_cell_dummy();
                    live_dummy.stamp_coord(coord.0, coord.1);
                    live_dummy
                };
                crate::sim::projectile::dummy_cell_target_coord(&live_dummy)
            }
        }
    }

    pub fn apply_fatal_lifecycle_stage(
        &mut self,
        rules: &RuleSet,
        stage: crate::sim::combat::FatalLifecycleStage,
        stable_id: u64,
        category: EntityCategory,
        mut uninit_context: UninitContext<'_>,
    ) {
        match stage {
            crate::sim::combat::FatalLifecycleStage::MaintainDamageSmoke { state } => {
                self.maintain_damage_smoke_after_receive(stable_id, state, rules);
            }
            crate::sim::combat::FatalLifecycleStage::BeforeBuildingDestruction => {
                // Building442625..44264C, after Techno701900 has completed
                // its debris and synchronous death weapon. Garrison Scatter
                // draws belong here, before DestructionEffects442665 and its
                // foundation marks and survivors. True Cargo is independent:
                // DestructionEffects441F27 kills it after SpawnSurvivors.
                // Native execution: tools/spatial_oracle/building_garrison_fatal.json.
                if category != EntityCategory::Structure {
                    return;
                }
                // 442625 gates SellBuilding(0,0) at 44263B on CanBeOccupied.
                let object = self
                    .substrate
                    .entities
                    .get(stable_id)
                    .and_then(|entity| self.interner.object_type(entity.type_ref(), rules));
                if object.is_some_and(|object| object.can_be_occupied) {
                    passenger::garrison_eject::sell_building_occupants(
                        self,
                        rules,
                        uninit_context.registry(),
                        stable_id,
                        passenger::garrison_eject::GarrisonEjectMode::Release,
                        uninit_context.effects(),
                    );
                }
                //44264C precedes the recursive destruction effects.
                self.lighting_sources
                    .set_building_light_active(stable_id, false);
            }
            crate::sim::combat::FatalLifecycleStage::AfterDeathEffects => {
                if !matches!(category, EntityCategory::Unit | EntityCategory::Structure) {
                    return;
                }
                // Unit's receiver retains a `Crashable=` type its Crash(0)
                // latched (`0x00738475`) and any other type's sinking hull
                // (`0x00738493`); their own AI terminal calls UnInit. A
                // Magnetron's drop raises the crash latch too, and keeps nothing.
                if self
                    .substrate
                    .entities
                    .get(stable_id)
                    .is_some_and(|entity| {
                        if self
                            .object_type(entity.type_ref(), rules)
                            .is_some_and(|object| object.crashable)
                        {
                            entity.crash.is_crashing()
                        } else {
                            entity.sinking.is_active()
                        }
                    })
                {
                    return;
                }
                // `BuildingClass::AfterDestruction @ 0x00441F60` follows the
                // dying building's UnInit and reads its type and Location.
                let destroyed_building = (category == EntityCategory::Structure)
                    .then(|| self.substrate.entities.get(stable_id))
                    .flatten()
                    .map(|building| {
                        (
                            building.type_ref(),
                            (building.position.rx(), building.position.ry()),
                        )
                    });
                self.uninit_with_context(stable_id, uninit_context.reborrow());
                if let Some((type_id, origin)) = destroyed_building {
                    self.building_after_destruction(
                        rules,
                        uninit_context.registry(),
                        type_id,
                        origin,
                        uninit_context.effects(),
                    );
                }
            }
        }
    }

    /// The receiver fixture's combat pass over this world
    /// ([`crate::sim::combat::world_fire::tick_combat`]).
    #[cfg(test)]
    pub(super) fn tick_combat_with_fatal_lifecycle(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        tick_ms: u32,
        logic_order: &[u64],
        fire_suppressed: &BTreeSet<u64>,
        projectile_detonations: &[crate::sim::projectile::ProjectileDetonation],
        frame_effects: &mut FrameEffects<'_>,
    ) -> crate::sim::combat::CombatTickResult {
        let mut run =
            crate::sim::combat::world_receiver::ReceiverRun::default().with_effects(frame_effects);
        let mut result = crate::sim::combat::world_fire::tick_combat(
            self,
            &mut run,
            rules,
            overlay_registry,
            tick_ms,
            logic_order,
            fire_suppressed,
            projectile_detonations,
        );
        result.consequences.finish_navigation(run.finish());
        result
    }

    /// Live class and mission FireAt deliveries finish before the next
    /// dynamic Logic object slot. The caller retains frame invalidations.
    pub fn commit_fire_visit(
        &mut self,
        visit: crate::sim::combat::world_fire::FireVisit,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: &mut FrameEffects<'_>,
    ) -> damage_consequences::DamageCommitReceipt {
        let _area = crate::sim::frame_profile::area(crate::sim::frame_profile::phase::FIRING);
        #[cfg(test)]
        if let crate::sim::combat::world_fire::FireVisit::Building { id, shot } = &visit
            && crate::sim::combat::receiver_fixture::substitute_building_fire(
                &self.session,
                &mut self.substrate,
                *id,
                *shot,
                rules,
            )
        {
            return damage_consequences::DamageCommitReceipt {
                area_result: None,
                structure_destroyed: false,
                bridge_state_changed: false,
            };
        }
        #[cfg(test)]
        let observed_direct = match &visit {
            crate::sim::combat::world_fire::FireVisit::Direct { id, .. } => Some(*id),
            crate::sim::combat::world_fire::FireVisit::Building { id, .. } => Some(*id),
            _ => None,
        };
        #[cfg(test)]
        if let Some(id) = observed_direct {
            crate::sim::combat::receiver_fixture::observe_fire_visit(self, id, "entry");
        }
        let mut run =
            crate::sim::combat::world_receiver::ReceiverRun::default().with_effects(frame_effects);
        let mut emit = crate::sim::combat::CombatEmit::default();
        let mut pings = Vec::new();
        crate::sim::combat::world_fire::visit_fire(
            self,
            &mut run,
            visit,
            rules,
            overlay_registry,
            &mut emit,
            &mut pings,
        );
        let crate::sim::combat::CombatEmit {
            effects,
            fire_events,
            // The receiver already committed these inline before returning.
            damage_events: _,
            remove_attack: _,
            unit_facing: _,
            spawn_target_updates: _,
            drain_links: _,
        } = emit;
        let receipt = damage_consequences::DamageConsequences::live_fire(
            effects,
            pings,
            run.finish(),
            fire_events,
        )
        .commit(self, rules, frame_effects);
        #[cfg(test)]
        if let Some(id) = observed_direct {
            crate::sim::combat::receiver_fixture::observe_fire_visit(self, id, "return");
        }
        receipt
    }

    /// The commit of a visit that fires nothing: no consequences, but it
    /// still delivers the overlay grid's pending navigation receipts and the
    /// smudge grid's dirty cells, as [`Self::commit_fire_visit`] does.
    pub(crate) fn commit_no_fire(
        &mut self,
        rules: &RuleSet,
        frame_effects: &mut FrameEffects<'_>,
    ) -> damage_consequences::DamageCommitReceipt {
        damage_consequences::DamageConsequences::live_fire(
            Default::default(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .commit(self, rules, frame_effects)
    }

    /// Commit a completed Bullet's detonation while its current Logic slot and
    /// physical object are still present. The caller retires the Bullet only
    /// after this transaction returns.
    pub(crate) fn commit_logic_projectile_detonations(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        detonations: &[crate::sim::projectile::ProjectileDetonation],
        frame_effects: &mut FrameEffects<'_>,
    ) -> bool {
        if detonations.is_empty() {
            return false;
        }
        let mut run =
            crate::sim::combat::world_receiver::ReceiverRun::default().with_effects(frame_effects);
        let commit = crate::sim::combat::world_receiver::commit_projectiles(
            self,
            &mut run,
            detonations,
            rules,
            overlay_registry,
        );
        let terrain_navigation_changed_cells = run.finish();

        #[cfg(test)]
        if let Some(fixture) = self.receiver_fixture.as_mut() {
            let changed = commit.effects.bridge_state_changed;
            fixture
                .tail_effects
                .push((commit.effects, commit.under_attack_events));
            return changed;
        }
        self.absorb_noncombat_damage_effects(
            rules,
            commit.effects,
            commit.under_attack_events,
            terrain_navigation_changed_cells,
            frame_effects,
        )
        .bridge_state_changed
    }

    /// `weapon_index` is FireAt's own argument; `overlay_registry` reaches
    /// the constructor's immediate update.
    pub(crate) fn commit_fired_wave(
        &mut self,
        rules: &RuleSet,
        event: &SimFireEvent,
        wave_type: u8,
        weapon_index: i32,
    ) {
        if let Some(wave) = self.prepare_fired_wave(rules, event, wave_type) {
            // Ares CreateWave (Ares.dll 10055160) stores the BYTE weapon index
            // in the owner's TechnoExt before the constructor runs.
            if rules.ares()
                && let Some(owner) = self.substrate.entities.get_mut(event.attacker_id)
            {
                owner.record_ares_wave_weapon_slot(weapon_index as u8);
            }
            self.admit_fired_wave(event.attacker_id, wave, rules);
        }
    }

    /// Read the firing transaction directly; construction never installs a second
    /// entity, map, house or RNG authority into the world.
    pub(super) fn prepare_fired_wave(
        &self,
        rules: &RuleSet,
        event: &SimFireEvent,
        wave_type: u8,
    ) -> Option<crate::sim::wave::Wave> {
        // Only Ares (TechnoClass_Fire_OtherWaves 10056E60) creates the TS
        // laser types 1 and 2.
        debug_assert!(matches!(wave_type, 0 | 3) || (rules.ares() && wave_type < 3));
        let target = match event.target {
            // FireAt aims at the target's vt+0xA4 (`0x006FE1EE`), including
            // a Building's TargetCoordOffset. Wave AI later reads +58 instead.
            crate::sim::combat::TargetKind::Entity(_) => {
                crate::sim::combat::fire_coord::effect_target_coordinate(
                    self,
                    rules,
                    event.target.projectile_target(self),
                )?
            }
            crate::sim::combat::TargetKind::Cell(rx, ry) => self.wave_cell_target_position(rx, ry),
        };
        let wave = if rules.ares() {
            // Ares TechnoClass_Fire_SonicWave/OtherWaves (Ares.dll 10056EF0/
            // 10056E60, hooks 0x6FF449/0x6FF5F5) build both through CreateWave
            // 10055160, which passes &[ESP+0x44] first and &[ESP+0x88] second,
            // and registers the firing weapon in WaveExt.
            crate::sim::wave::Wave::new_owned(
                wave_type,
                event.attacker_id,
                event.target,
                event.fire_coord,
                target,
            )
            .with_ares_weapon(event.weapon_id)
        } else {
            // Sonic 0x006FF465 and Magnetic 0x006FF63C pass the constructor
            // &[ESP+0x88], this target coordinate (0x006FE204), first and
            // &[ESP+0x44], the launch source (`event.fire_coord`: GetFLH or a
            // Dropping projectile's GetCoords), second.
            crate::sim::wave::Wave::new_owned(
                wave_type,
                event.attacker_id,
                event.target,
                target,
                event.fire_coord,
            )
        };
        Some(wave)
    }

    /// Commit identity, constructor cleanup and immediate Logic admission in their
    /// original order. The receiver keeps ownership of all combat/map state.
    pub(super) fn admit_fired_wave(
        &mut self,
        attacker_id: u64,
        mut wave: crate::sim::wave::Wave,
        rules: &RuleSet,
    ) {
        let stable_id = self.substrate.allocate_stable_id();
        if !wave.constructor_distance_is_live(self.session.map_exceeds_original_size()) {
            // Constructor UnInit precedes registration and unique identity,
            // but FireAt stores the returned dead pointer. The stable id only
            // represents that same-frame deferred cleanup window.
            self.waves.spawn(stable_id, wave);
            self.active_wave_links.insert(attacker_id, stable_id);
            let retired = self.retire_non_entity_object(stable_id, UninitContext::new(rules));
            debug_assert!(retired);
            return;
        }
        // 75EAA4 calls Create_ID410230 only after the short-distance abort.
        // The Scenario-owned cursor is separate from Rust's stable handle;
        // this chain has no consumer of the Wave's numeric ID itself.
        self.next_native_runtime_id();
        // Constructor75E950 runs an immediate Update_Wave762AF0, including
        // current target+58 and GetFLH(0), before the first Logic AI visit.
        let context = self.wave_update_context(&wave, rules);
        let terminal = wave.initialize(context, self.resolved_terrain.as_ref());
        self.admit_wave(stable_id, wave);
        self.active_wave_links.insert(attacker_id, stable_id);
        if terminal {
            let retired = self.retire_non_entity_object(stable_id, UninitContext::new(rules));
            debug_assert!(retired);
        }
    }

    /// Visit the non-Techno Logic objects from `first_tail_id` on, for
    /// fixtures that run without the live object pass (the receiver fixture's
    /// combat pass, a bullet's flight frames). FireAt appends each bullet,
    /// muzzle anim and wave to the Logic vector as it fires, and
    /// `LogicClass::PerTickUpdate` re-reads the vector's length after every AI
    /// call (`0x0055B613`), so those objects take their first AI later in the
    /// same pass, in append order; an Inviso bullet detonates on that visit.
    /// Anything they append in turn (explosion anims) follows. A detonated
    /// bullet leaves the vector (`LogicClass::Remove @ 0x0055BAE0`), which
    /// shifts the next entry into its slot, and the cursor moves past it: that
    /// entry waits a frame. Production firers append inside the live pass.
    #[cfg(any(test, feature = "test-support"))]
    pub fn visit_combat_tail(
        &mut self,
        first_tail_id: u64,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: &mut FrameEffects<'_>,
    ) -> bool {
        let mut bridge_state_changed = false;
        let mut index = 0;
        while index < self.substrate.logic.len() {
            let stable_id = self.substrate.logic.as_slice()[index];
            if stable_id >= first_tail_id && !self.substrate.entities.contains(stable_id) {
                bridge_state_changed |= self
                    .object_ai_visit_one_with_effects(
                        stable_id,
                        rules,
                        &mut techno_ai::ObjectAiCtx {
                            effects: frame_effects.reborrow(),
                            overlay_registry,
                            ..techno_ai::ObjectAiCtx::default()
                        },
                    )
                    .bridge_state_changed;
            }
            index += 1;
        }
        bridge_state_changed
    }

    /// Walk one Wave damage request in recorded-cell and current Cell-list
    /// order. Each receiver commits before the next occupant is selected, so
    /// fatal UnInit and nested effects are visible to the remaining walk.
    /// This is the receiver walk of `WaveClass::DamageArea` (`0x0075F330`).
    pub(super) fn commit_logic_wave_damage_request(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        request: &crate::sim::wave::WaveDamageRequest,
        frame_effects: &mut FrameEffects<'_>,
    ) {
        // DamageArea reads WaveClass+0x1D4 at the call boundary. Health and
        // Rust death-animation state do not null that pointer; only the exact
        // PointerExpired callback does. Revalidate the buffered fixture path
        // against the live Wave so an intervening owner UnInit still aborts.
        if self
            .waves
            .get(request.wave_id)
            .and_then(|wave| wave.owner_id())
            != Some(request.firer_id)
        {
            return;
        }

        use crate::sim::occupancy::CellObjectMember as CellObject;

        /// The cell's object list `WaveClass::DamageArea @ 0x0075F330` walks (the
        /// bridge layer's when its height test picks it), in its current order.
        fn current_order(
            occupancy: &crate::sim::occupancy::OccupancyGrid,
            terrain_objects: &crate::sim::terrain_object::TerrainObjectStore,
            rx: u16,
            ry: u16,
            layer: MovementLayer,
        ) -> Vec<CellObject> {
            occupancy
                .cell_objects(rx, ry, layer, terrain_objects.members_at((rx, ry)))
                .collect()
        }

        let mut run =
            crate::sim::combat::world_receiver::ReceiverRun::default().with_effects(frame_effects);
        // Wave's synchronous receiver (0x0075F42C) can detonate a DeathWeapon
        // at 0x0070D782. Zero-delay/default-Start impact Anim construction
        // (0x00469C93 -> 0x00422702/0x00424D5A) accesses the live smudge map
        // before returning. Keep that authority resident through recursion.
        // Evidence: active gamemd.exe bodies and caller instructions.
        let mut effects = crate::sim::combat::DeathEffects::default();
        let mut under_attack_events = Vec::new();

        {
            for recorded in &request.recorded_cells {
                let cell = recorded.current_cell(self.resolved_terrain.as_ref());
                // GetWeapon(0) is re-entered exactly once per cell, with the
                // owner's current rank. A null owner aborts DamageArea before
                // any receiver, wall, cliff, or RNG work.
                let Some(firer) = self
                    .waves
                    .get(request.wave_id)
                    .and_then(|wave| wave.owner_id())
                    .and_then(|id| self.substrate.entities.get(id))
                else {
                    break;
                };
                let Some(object_type) = self.interner.object_type(firer.type_ref(), rules) else {
                    break;
                };
                // Ares WaveClass_DamageCell (Ares.dll 10056FA0, hook
                // 0x75F38F) skips GetWeapon(0) for the wave's own weapon.
                let weapon_name = match request.ares_weapon {
                    Some(weapon) => Some(self.interner.resolve(weapon)),
                    None => crate::sim::combat::combat_weapon::primary_for_tier(
                        object_type,
                        firer.veterancy.rank(),
                    ),
                };
                let Some(weapon) = weapon_name.and_then(|name| rules.weapon(name)) else {
                    break;
                };
                let raw_ambient_damage = weapon.ambient_damage;
                let Some(warhead_name) = weapon.warhead.as_deref() else {
                    break;
                };
                let warhead_ref = self.interner.intern(warhead_name);
                let mut shared_damage = raw_ambient_damage;

                // The shared dummy is still a native CellClass entry, so the
                // GetWeapon(0) call above belongs to it too. Its final stamped
                // coordinate cannot own a real cell list, wall, or cliff.
                if !cell.real {
                    continue;
                }
                let (Ok(rx), Ok(ry)) = (u16::try_from(cell.rx), u16::try_from(cell.ry)) else {
                    continue;
                };
                let layer = if crate::sim::cell_kernel::selects_infantry_bridge_layer(
                    cell.structural_bridge,
                    cell.level as u8,
                    request.wave_z,
                ) {
                    MovementLayer::Bridge
                } else {
                    MovementLayer::Ground
                };

                let mut current = current_order(
                    &self.substrate.occupancy,
                    &self.terrain_objects,
                    rx,
                    ry,
                    layer,
                )
                .first()
                .copied();
                while let Some(receiver_object) = current {
                    // Wave75F3E9 reads its current owner again for the comparison
                    // and arg4. A prior callback can have cleared it; arg7 is always null.
                    let firer = self
                        .waves
                        .get(request.wave_id)
                        .and_then(|wave| wave.owner_id());
                    let receiver = match receiver_object {
                        CellObject::Entity(id)
                            if Some(id) != firer
                                && self
                                    .substrate
                                    .entities
                                    .get(id)
                                    .is_some_and(|actor| actor.is_live_on_map()) =>
                        {
                            let event = crate::sim::combat::EntityDamageEvent::direct_receiver(
                                id,
                                shared_damage,
                                0,
                                firer.unwrap_or(crate::sim::combat::RAD_NO_ATTACKER),
                                None,
                                warhead_ref,
                                crate::sim::combat::ReceiverCallFlags {
                                    ignore_defenses: false,
                                    arg6: false,
                                },
                            );
                            Some(crate::sim::combat::combat_aoe::AreaDamageReceiver::Entity(
                                event,
                            ))
                        }
                        CellObject::Terrain(id)
                            if self
                                .terrain_objects
                                .get(id)
                                .is_some_and(|tree| tree.is_live() && tree.health() > 0) =>
                        {
                            Some(crate::sim::combat::combat_aoe::AreaDamageReceiver::Terrain(
                                crate::sim::combat::TerrainDamageEvent {
                                    stable_id: id,
                                    rx,
                                    ry,
                                    damage: shared_damage,
                                    distance_leptons: 0,
                                    warhead_ref,
                                    near_center_ic_isolation_eligible: false,
                                    damage_writeback: None,
                                },
                            ))
                        }
                        _ => None,
                    };
                    if let Some(receiver) = receiver {
                        #[cfg(any(test, feature = "test-support"))]
                        self.trace_lifecycle_for_test(
                            LifecycleTestEvent::WaveDamageReceiverSelected {
                                wave_id: request.wave_id,
                                target_id: match receiver_object {
                                    CellObject::Entity(id) | CellObject::Terrain(id) => id,
                                },
                                scenario_rng_state: self.scenario_rng.state(),
                            },
                        );
                        let (nested, damage) = crate::sim::combat::world_receiver::commit_receiver(
                            self,
                            &mut run,
                            receiver,
                            rules,
                            overlay_registry,
                            &mut under_attack_events,
                        );
                        shared_damage = damage;
                        effects.append(nested);
                    }

                    // Native loads current->NextObject only after its callback.
                    // If UnInit removed the current object, its represented
                    // list link is gone and this walk terminates.
                    let order = current_order(
                        &self.substrate.occupancy,
                        &self.terrain_objects,
                        rx,
                        ry,
                        layer,
                    );
                    current = order
                        .iter()
                        .position(|&item| item == receiver_object)
                        .and_then(|index| order.get(index + 1).copied());
                }

                // ChainReaction's Wave-tail callee (75F44A..75F45F) is a bare
                // RET; Ares 3.0 fills it with CellChainReact (10050DD0). Wall
                // damage follows receivers and reloads raw AmbientDamage,
                // ignoring the shared mutable occupant value.
                if rules.ares() && self.overlay_chain_reacts((rx, ry), overlay_registry) {
                    self.ares_cell_chain_react((rx, ry), rules, overlay_registry, run.effects());
                }
                if let (Some(grid), Some(registry)) = (self.overlay_grid.as_mut(), overlay_registry)
                    && let Some(overlay_id) = grid.cell(rx, ry).overlay_id
                    && let Some(flags) = registry.flags(overlay_id)
                {
                    let _chain_reaction_no_op = flags.chain_reaction;
                    // Ares 3.0 WaveClass_DamageCell_Wall (1005D7A0, hook
                    // 0x75F46E): only a weapon whose Warhead is `Wall=yes`
                    // damages the wall.
                    if flags.wall
                        && (!rules.ares()
                            || rules
                                .warhead(warhead_name)
                                .is_some_and(|warhead| warhead.wall))
                    {
                        let _wall = {
                            let mut host = SimulationWallRuntimeHost {
                                dirty_cells: &mut self.dirty_cells,
                                zone_grid: &mut self.zone_grid,
                                path_grid: &mut self.path_grid,
                                bridge_state: self.bridge_state.as_ref(),
                                playfield_bounds: self.playfield_bounds,
                            };
                            crate::sim::overlay_grid::damage_wall_overlay_with_runtime_host(
                                grid,
                                registry,
                                self.resolved_terrain.as_mut(),
                                crate::sim::overlay_grid::WallPointerTarget::Real(rx, ry),
                                raw_ambient_damage,
                                &mut self.scenario_rng,
                                Some(&mut host),
                            )
                        };
                        #[cfg(any(test, feature = "test-support"))]
                        effects.wall_mutations.extend(_wall.mutations);
                    }
                }

                // The cliff tail is independent of damage magnitude and
                // Warhead.
                self.roll_cliff_collapse(
                    rx,
                    ry,
                    rules,
                    overlay_registry,
                    run.effects(),
                    &mut effects,
                );
            }
        }

        let terrain_navigation_changed_cells = run.finish();
        if let Some(terrain) = self.resolved_terrain.as_ref() {
            self.real_cell_bridge_flags_0x1180 = terrain.capture_real_cell_bridge_flags_0x1180();
        }
        self.absorb_noncombat_damage_effects(
            rules,
            effects,
            under_attack_events,
            terrain_navigation_changed_cells,
            frame_effects,
        );
    }

    /// The destroyable-cliff roll Wave `DamageArea` and the railgun line
    /// (`0x0070CAE9..0x0070CB2B` at the ground hit, `0x0070CC05..0x0070CC39`
    /// on a Cell target) share. A cell whose tile is a destroyable cliff
    /// (`CellClass::Tile_Is_DestroyableCliff @ 0x00486900`) spends one Scenario
    /// `RandomRanged(0, 99)` draw, including signed chances outside 0..=100,
    /// and collapses (`MapClass::CollapseCliff @ 0x00581140`) when the draw is
    /// below `[CombatDamage] CollapseChance`.
    pub(crate) fn roll_cliff_collapse(
        &mut self,
        rx: u16,
        ry: u16,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: &mut FrameEffects<'_>,
        #[cfg_attr(
            not(any(test, feature = "test-support")),
            allow(
                unused_variables,
                reason = "only the test-support receiver records into it"
            )
        )]
        effects: &mut crate::sim::combat::DeathEffects,
    ) {
        let destroyable_cliff = self
            .resolved_terrain
            .as_ref()
            .is_some_and(|terrain| terrain.is_destroyable_cliff(rx, ry));
        if destroyable_cliff {
            let chance_draw = self.scenario_rng.next_range_u32_inclusive(0, 99) as i32;
            if chance_draw < rules.combat_damage.collapse_chance
                && let Some(mutation) = self.resolved_terrain.as_mut().and_then(|terrain| {
                    terrain.collapse_destroyable_cliff_terrain(
                        rx,
                        ry,
                        overlay_registry.and_then(|registry| registry.cliff_back_rock()),
                        |cell_x, cell_y| {
                            if let Some(grid) = self.overlay_grid.as_mut() {
                                let _ = grid.clear_overlay(cell_x, cell_y);
                            }
                            if let Some(grid) = self.smudge_grid.as_mut() {
                                let _ = grid.clear_cell_slot(cell_x, cell_y);
                            }
                        },
                    )
                })
            {
                // Cliff stamps change Cell+11B without refreshing
                // retained lighting; publish geometry before any later
                // ordered source/owner callback samples these cells.
                self.publish_cell_lighting_levels(
                    mutation
                        .original_footprint
                        .iter()
                        .chain(&mutation.changed_cells)
                        .copied(),
                );
                // Native performs one complete zone rebuild after the
                // two sparse stamps and before its three footprint
                // passes. The Rust navigation owner rebuilds all
                // movement zones as the corresponding single unit.
                self.rebuild_dynamic_navigation(rules);

                // Pass three's externally represented effects remain
                // strictly interleaved: detach this CellClass target,
                // then dirty this radar cell, in old-TMP row order.
                for &coord in &mutation.original_footprint {
                    // Map5814D4/581BE6:70D4A0 then radar, per old footprint cell.
                    let _receipt =
                        self.stop_all_targeting_cell(coord.0, coord.1, rules, frame_effects);
                    #[cfg(any(test, feature = "test-support"))]
                    effects.cell_target_detaches.extend(_receipt.cell_detaches);
                    self.dirty_cells.mark_radar([coord]);
                }
                for &coord in &mutation.original_footprint {
                    self.dirty_cells.mark_tactical_once(coord);
                }

                // Resolve the three types in native order, then make
                // two row-major attempts at each of the 15 grid cells.
                // Every representable allocation is successful, so
                // each attempt consumes type, X, Y, and delay draws.
                let anim_types = crate::rules::effect_asset_catalog::CLIFF_COLLAPSE_ANIMS
                    .map(|name| self.interner.intern(name));
                for &(cell_x, cell_y) in &mutation.animation_cells {
                    for _ in 0..2 {
                        let type_index = self.scenario_rng.next_range_u32_inclusive(0, 2) as usize;
                        // 0x0058173A..0x005817C1, 0x00581E40..0x00581EC7: the
                        // first draw offsets Y, the second X.
                        let jitter_y = self.scenario_rng.next_range_u32_inclusive(0, 16) as i32 - 8;
                        let jitter_x =
                            self.scenario_rng.next_range_u32_inclusive(0, 24) as i32 - 12;
                        let level = self
                            .resolved_terrain
                            .as_ref()
                            .expect("collapse terrain retained")
                            .collapse_animation_level(cell_x, cell_y);
                        let delay = self.scenario_rng.next_range_u32_inclusive(0, 2) as u16;
                        let world_coord = crate::sim::anim_class::AnimWorldCoord {
                            x: i32::from(cell_x)
                                .wrapping_mul(256)
                                .wrapping_add(128)
                                .wrapping_add(jitter_x),
                            y: i32::from(cell_y)
                                .wrapping_mul(256)
                                .wrapping_add(128)
                                .wrapping_add(jitter_y),
                            z: i32::from(level).wrapping_mul(104),
                        };
                        let mut descriptor = crate::sim::components::AnimClassSpawnDescriptor::new(
                            anim_types[type_index],
                            cell_x as u16,
                            cell_y as u16,
                            SimFixed::from_num(128 + jitter_x),
                            SimFixed::from_num(128 + jitter_y),
                            level as u8,
                        );
                        descriptor.delay = delay;
                        descriptor.loop_count = 1;
                        descriptor.draw_flags = 0x600;
                        descriptor.z_adjust = 0;
                        descriptor.reverse = false;
                        // AnimClass construction completes before the
                        // next attempt selects its type. This matters
                        // when the selected AnimType consumes Scenario
                        // RNG for RandomRate during construction.
                        let _ = self.spawn_anim_at_world(rules, descriptor, world_coord);
                    }
                }

                let terrain = self
                    .resolved_terrain
                    .as_ref()
                    .expect("collapse terrain retained");
                for &(cell_x, cell_y) in &mutation.changed_cells {
                    if let Some(cell) = terrain.cell(cell_x, cell_y) {
                        self.dynamic_terrain_cells.record(cell);
                    }
                }
            }
        }
    }

    /// Complete one direct object ReceiveDamage call before its caller resumes.
    /// Unlike Apply_area_damage, a direct call has no collected area or area-wide
    /// Iron Curtain isolation decision. Nested deaths, world mutations and their
    /// publication finish before a live cell-list caller reads its successor.
    pub(crate) fn commit_direct_damage_receiver(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        event: crate::sim::combat::EntityDamageEvent,
        frame_effects: &mut FrameEffects<'_>,
    ) -> damage_consequences::DamageCommitReceipt {
        let mut run =
            crate::sim::combat::world_receiver::ReceiverRun::default().with_effects(frame_effects);
        let mut under_attack_events = Vec::new();
        let effects = crate::sim::combat::world_receiver::commit_entities(
            self,
            &mut run,
            std::slice::from_ref(&event),
            Some(false),
            rules,
            overlay_registry,
            &mut under_attack_events,
        );
        let terrain_navigation_changed_cells = run.finish();
        self.absorb_noncombat_damage_effects(
            rules,
            effects,
            under_attack_events,
            terrain_navigation_changed_cells,
            frame_effects,
        )
    }

    /// `ReceiveDamage(&Health, 0, C4Warhead=, NULL, 1, 1, NULL)` (vt+0x16C):
    /// the self-destruct that ends a flying locomotor's owner when it has
    /// nowhere to go, in Fly's Stop_Moving (`0x004CD0CD`) and landing retry
    /// (`0x004CEBC7`) and Jumpjet's Stop_Moving (`0x0054B6B6`). Native passes
    /// the Health field itself; this uses the shared bridge_ground stock
    /// C4Warhead=Super fatal-path quotient, not general aliased packet
    /// support: override-only early damage-pointer writes remain outside it.
    pub fn receive_own_health_c4(
        &mut self,
        id: u64,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: &mut FrameEffects<'_>,
    ) {
        let Some(health) = self.substrate.entities.get(id).map(|e| e.health.current()) else {
            return;
        };
        let warhead = self.interner.intern(&rules.bridge_warheads.c4_name);
        self.commit_direct_damage_receiver(
            rules,
            overlay_registry,
            crate::sim::combat::EntityDamageEvent::direct_receiver(
                id,
                health,
                0,
                crate::sim::combat::RAD_NO_ATTACKER,
                None,
                warhead,
                crate::sim::combat::ReceiverCallFlags {
                    ignore_defenses: true,
                    arg6: true,
                },
            ),
            frame_effects,
        );
    }

    /// Terrain counterpart of a direct object receiver. Native bridge fallout
    /// forces Object damage, while Terrain's Wood/Immune gate still applies.
    pub(crate) fn commit_direct_terrain_damage_receiver(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        event: crate::sim::combat::TerrainDamageEvent,
        ignore_defenses: bool,
        frame_effects: &mut FrameEffects<'_>,
    ) -> damage_consequences::DamageCommitReceipt {
        let mut run =
            crate::sim::combat::world_receiver::ReceiverRun::default().with_effects(frame_effects);
        let mut under_attack_events = Vec::new();
        let (effects, _) = crate::sim::combat::world_receiver::commit_terrain(
            self,
            &mut run,
            event,
            ignore_defenses,
            rules,
            overlay_registry,
            &mut under_attack_events,
        );
        let terrain_navigation_changed_cells = run.finish();
        self.absorb_noncombat_damage_effects(
            rules,
            effects,
            under_attack_events,
            terrain_navigation_changed_cells,
            frame_effects,
        )
    }

    /// Commit one non-combat Apply_area_damage hit list through the ordinary
    /// ReceiveDamage -> death transaction, then hand its deferred outputs back
    /// to the world owner. `hits` remains in CellClass/object-list order; the
    /// combat helper enters a nested DeathWeapon before advancing to the next
    /// hit.
    pub(crate) fn commit_noncombat_aoe_hits(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        hits: &[crate::sim::combat::EntityDamageEvent],
        frame_effects: &mut FrameEffects<'_>,
    ) {
        let receivers = hits
            .iter()
            .copied()
            .map(crate::sim::combat::combat_aoe::AreaDamageReceiver::Entity)
            .collect::<Vec<_>>();
        self.commit_noncombat_aoe_receivers(rules, overlay_registry, &receivers, frame_effects);
    }

    /// World-owned entry for a complete non-combat Apply_area_damage receiver
    /// vector, including TerrainClass records in captured object-list order.
    pub(crate) fn commit_noncombat_aoe_receivers(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        receivers: &[crate::sim::combat::combat_aoe::AreaDamageReceiver],
        frame_effects: &mut FrameEffects<'_>,
    ) -> damage_consequences::DamageCommitReceipt {
        let mut run =
            crate::sim::combat::world_receiver::ReceiverRun::default().with_effects(frame_effects);
        let mut under_attack_events = Vec::new();
        let (effects, area_result) = crate::sim::combat::world_receiver::commit_area_with_dispatch(
            self,
            &mut run,
            receivers,
            rules,
            overlay_registry,
            &mut under_attack_events,
        );
        let terrain_navigation_changed_cells = run.finish();
        let mut receipt = self.absorb_noncombat_damage_effects(
            rules,
            effects,
            under_attack_events,
            terrain_navigation_changed_cells,
            frame_effects,
        );
        receipt.area_result = Some(area_result);
        receipt
    }

    /// World-owned half of a non-combat damage transaction. Physical death
    /// consequences already happened recursively in combat; this consumes the
    /// same lifecycle/presentation/terrain outputs without leaving an alternate
    /// zero-HP object registered for the next LogicClass visit.
    pub(crate) fn absorb_noncombat_damage_effects(
        &mut self,
        rules: &RuleSet,
        effects: crate::sim::combat::DeathEffects,
        under_attack_events: Vec<crate::sim::combat::UnderAttackEvent>,
        terrain_navigation_changed_cells: Vec<(u16, u16)>,
        frame_effects: &mut FrameEffects<'_>,
    ) -> damage_consequences::DamageCommitReceipt {
        damage_consequences::DamageConsequences::immediate(
            effects,
            under_attack_events,
            terrain_navigation_changed_cells,
        )
        .commit(self, rules, frame_effects)
    }

    /// HouseClass::NotifyUnderAttack4F93E0 submits its radar/EVA/Voc before
    /// the dynamic House Tag walk4F95D4. Callbacks may change alliances or
    /// client admission state, so both selection and delivery precede them.
    pub(crate) fn notify_house_under_attack(
        &mut self,
        event: crate::sim::combat::UnderAttackEvent,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: &mut FrameEffects<'_>,
    ) {
        self.submit_under_attack_warning(event, rules, effects);
        self.raise_house_attacked_tags(event.owner, rules, registry, effects);
    }

    /// Unit73851D/738530 and Temporal71B04C/71B05F submit radar/EVA before
    /// returning to their caller. House4F93E0 reaches the same boundary before
    /// its Tags. A later AoE receiver must not overtake this warning.
    pub(crate) fn submit_under_attack_warning(
        &mut self,
        event: crate::sim::combat::UnderAttackEvent,
        rules: &RuleSet,
        effects: &mut FrameEffects<'_>,
    ) {
        Self::dispatch_under_attack_events(
            &self.interner,
            &self.session,
            &self.house_alliances,
            &mut self.sound_events,
            &self.houses,
            &[event],
            rules,
        );
        // Unit7384D8 and Temporal71B009 compare Owner with PlayerPtr before
        // entering their radar/EVA block. Keep all-owner producer facts for
        // replay, but only the reached local Unit call consumes this prefix.
        // HouseNotify can also warn a local ally of a nonlocal building.
        let building_warning = matches!(
            event.line,
            crate::sim::combat::UnderAttackLine::House { .. }
        );
        if building_warning || self.session.current_house() == Some(event.owner) {
            effects.flush_sound(self, rules);
        }
    }

    /// `HouseClass::NotifyUnderAttack @ 0x004F93E0` for a building warning,
    /// plus the ore-miner pings ([`crate::sim::combat::UnderAttackLine`]).
    ///
    /// The victim's own line: the radar diamond (type 4 miner / type 3 base,
    /// `0x004F94E4`/`0x004F9544`) is requested for the victim house; the app
    /// keeps the local-player filter and admits it there. The ally line (`0x004F955D..0x004F95B3`,
    /// a House line reached on every client): every *other* human house that the victim house
    /// lists in its own ally bitfield (`House+0x5788`, read at `0x004F9450`
    /// through `IsAlliedWith`'s one-way rule) hears
    /// `EVA_OurAllyIsUnderAttack` behind `CreateRadarEvent(0x10, cell)`,
    /// unless the victim's country is `MultiplayPassive=` in a non-campaign
    /// mode (`0x004F945D..0x004F9472`). Native has no other rate limit.
    pub(crate) fn dispatch_under_attack_events(
        interner: &StringInterner,
        session: &ScenarioSession,
        house_alliances: &HouseAlliances,
        sound_events: &mut FrameChannel<SimSoundEvent>,
        houses: &Houses,
        events: &[crate::sim::combat::UnderAttackEvent],
        rules: &RuleSet,
    ) {
        let game_mode_nonzero = session.game_mode_nonzero();
        for event in events {
            let Some(object) = interner.object_type(event.victim_type, rules) else {
                continue;
            };
            // House4F9491..4F94A3: a deployed miner needs both predicates.
            // OreMiner producers have already admitted their own victims.
            // Native corpus: tools/spatial_oracle/temporal_house_notice.json.
            let (miner, allies) = match event.line {
                crate::sim::combat::UnderAttackLine::OreMiner => (true, false),
                crate::sim::combat::UnderAttackLine::House { allies } => (
                    object.undeploys_into.is_some() && object.resource_gatherer,
                    allies,
                ),
            };
            let event_type = if miner {
                RadarEventType::HarvesterUnderAttack
            } else {
                RadarEventType::BaseUnderAttack
            };
            if event.own_alert {
                sound_events.emit(SimSoundEvent::UnderAttack {
                    rx: event.rx,
                    ry: event.ry,
                    owner: event.owner,
                    miner,
                    radar: RadarEventRequest::new(event_type, event.rx, event.ry),
                });
            }
            if !allies {
                continue;
            }
            let victim_passive = houses
                .get(&event.owner)
                .is_some_and(|house| house.multiplay_passive());
            if game_mode_nonzero && victim_passive {
                continue;
            }
            let listeners: Vec<InternedId> = houses
                .order()
                .iter()
                .copied()
                .filter(|&listener| {
                    listener != event.owner
                        && houses.get(&listener).is_some_and(|house| house.is_human())
                        && house_alliances.is_allied(interner, event.owner, listener)
                })
                .collect();
            for listener in listeners {
                sound_events.emit(SimSoundEvent::AllyUnderAttack {
                    owner: listener,
                    radar: RadarEventRequest::new(
                        RadarEventType::AllyUnderAttack,
                        event.rx,
                        event.ry,
                    ),
                });
            }
        }
    }

    /// `TechnoClass::Death_Announcement @ 0x004D98C0` for damage kills
    /// and the living-hull Squid cull: owner passes `HouseClass::IsHumanPlayer @ 0x0050B6F0`
    /// (`0x004D98CA`; the local player natively, every human house here with
    /// the app filtering), then `CreateRadarEvent(7, cell)` (`0x004D98FE`,
    /// 8-cell dedupe, admitted client-side) gates `EVA_UnitLost`
    /// (`0x004D9911`).
    pub(crate) fn dispatch_unit_lost_events(
        &mut self,
        events: &[crate::sim::combat::UnitLostEvent],
    ) {
        for event in events {
            if !self.owner_is_human(event.owner) {
                continue;
            }
            // Only the Ares event interns its line. A game without Ares must
            // not intern one here: a new ID changes later hashes.
            let ares = event.ares.as_ref().map(|ares| AresUnitLostLine {
                eva: self.interner.intern(&ares.eva),
                radar_event: ares.radar_event,
            });
            self.sound_events.emit(SimSoundEvent::UnitLost {
                owner: event.owner,
                radar: RadarEventRequest::new(RadarEventType::UnitLost, event.rx, event.ry),
                type_id: event.type_id,
                ares,
            });
        }
    }
}
