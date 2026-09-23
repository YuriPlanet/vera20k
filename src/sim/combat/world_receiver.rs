//! World-owned synchronous receiver execution.
//!
//! Gameplay authorities stay in Simulation across nested receiver/lifecycle
//! calls. Only recursion guards and ordered publication receipts are local.

use super::*;
use crate::sim::world::Simulation;

#[path = "aircraft_release.rs"]
mod aircraft_release;

fn respond_to_base_attack(
    world: &mut Simulation,
    rules: &RuleSet,
    _site: BaseDefenseResponseCallSite,
    victim_id: u64,
    attacker_id: u64,
) {
    #[cfg(test)]
    if let Some(fixture) = world.receiver_fixture.as_mut() {
        let victim = world
            .substrate
            .entities
            .get(victim_id)
            .expect("response victim represented");
        let last_attacker_house_index = world.houses.get(&victim.owner()).map_or(-1, |house| {
            house.strategy_emergency.last_attacker_house_index()
        });
        fixture
            .trace
            .entries
            .push(super::receiver_fixture::BaseDefenseResponseTraceEntry {
                site: _site,
                victim_id,
                health: victim.health.current,
                last_attacker_house_index,
            });
        return;
    }
    let mut context = base_defense_response::BaseDefenseResponseContext {
        entities: &mut world.substrate.entities,
        rules,
        interner: &world.interner,
        houses: &mut world.houses,
        alliances: &world.house_alliances,
        scenario_rng: &mut world.scenario_rng,
        teams: &mut world.team_script_vm,
        zone_grid: world.zone_grid.as_ref(),
        terrain: world.resolved_terrain.as_ref(),
        playfield_bounds: world.playfield_bounds,
        map_size_width: i32::from(world.session.map_width),
        map_size_height: i32::from(world.session.map_height),
        current_frame: world.session.binary_frame as i32,
        game_mode_nonzero: world.session.game_mode_nonzero,
    };
    base_defense_response::respond_to_base_attack(victim_id, attacker_id, &mut context);
}

#[allow(clippy::too_many_arguments)]
fn collect_area(
    world: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    cell: (u16, u16),
    damage: i32,
    warhead: &WarheadType,
    origin: (u64, Option<InternedId>, InternedId),
    air_impact: Option<combat_aoe::AoEAirImpact>,
    impact_z: i32,
) -> combat_aoe::AoEDamageResult {
    #[cfg(not(test))]
    let include_terrain_objects = true;
    #[cfg(test)]
    let include_terrain_objects = world
        .receiver_fixture
        .as_ref()
        .is_none_or(|fixture| fixture.terrain_collection);
    let mut prelude = crate::sim::world::simulation_area_damage_cell_prelude(
        rules,
        warhead,
        damage,
        true,
        world.session.no_damage,
        &mut world.production.ore_growth_state,
        &world.production.tiberium_spawning_terrain_cells,
        &world.production.terrain_object_cells,
        world.session.binary_frame,
        world.production.ore_growth_config.spreads,
        &mut world.radar_terrain_dirty_cells,
        &mut world.radar_terrain_dirty_generation,
        &mut world.tactical_dirty_cells,
        &mut world.terrain_costs,
        &mut world.zone_grid,
        &mut world.path_grid,
        world.bridge_state.as_ref(),
        world.playfield_bounds,
    );
    #[cfg(test)]
    let mut deferred_prelude = world.receiver_fixture.as_mut().map(|fixture| {
        super::receiver_fixture::DeferredCellPrelude {
            amount: (!world.session.no_damage)
                .then(|| tiberium_reduction_amount(damage, true, warhead))
                .flatten(),
            deferred: &mut fixture.deferred_tiberium,
        }
    });
    #[cfg(test)]
    let prelude: &mut dyn combat_aoe::AoECellPrelude = match deferred_prelude.as_mut() {
        Some(deferred) => deferred,
        None => &mut prelude,
    };
    #[cfg(not(test))]
    let prelude: &mut dyn combat_aoe::AoECellPrelude = &mut prelude;
    combat_aoe::apply_aoe_damage_with_terrain_and_scenario(
        &mut world.substrate.entities,
        cell.0,
        cell.1,
        damage,
        warhead,
        rules,
        &world.interner,
        world.rule_handles,
        origin,
        combat_aoe::AoELayerContext {
            occupancy: Some(&world.substrate.occupancy),
            terrain: world.resolved_terrain.as_mut(),
            overlay_grid: world.overlay_grid.as_mut(),
            overlay_registry,
            scenario_rng: Some(&mut world.scenario_rng),
            air_impact,
            impact_z,
        },
        include_terrain_objects.then_some(combat_aoe::TerrainCollectionView {
            objects: &world.production.terrain_objects,
            cells: &world.production.terrain_object_cells,
        }),
        world.session.no_damage,
        Some(prelude),
    )
}

fn commit_smudges(
    world: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    requests: Vec<SmudgeSpawnRequest>,
    _deferred: &mut Vec<SmudgeSpawnRequest>,
) {
    #[cfg(test)]
    if world.receiver_fixture.is_some() {
        _deferred.extend(requests);
        return;
    }
    for request in requests {
        world.commit_smudge_request_inline(rules, overlay_registry, request);
    }
}

#[derive(Default)]
pub(crate) struct ReceiverRun {
    pub(crate) handled_deaths: Vec<u64>,
    pub(super) finalizing_terrain: BTreeSet<u64>,
    pub(crate) navigation_changed_cells: Vec<(u16, u16)>,
}

impl ReceiverRun {
    pub(crate) fn finish(self, world: &mut Simulation) -> Vec<(u16, u16)> {
        debug_assert!(self.finalizing_terrain.is_empty());
        let inactive: Vec<_> = world
            .production
            .terrain_objects
            .values()
            .filter(|object| !object.is_live() && object.in_logic_vector)
            .map(|object| object.stable_id)
            .collect();
        for stable_id in inactive {
            let retired = world.retire_non_entity_object(stable_id);
            debug_assert!(retired);
        }
        self.navigation_changed_cells
    }
}

pub(crate) fn commit_area(
    world: &mut Simulation,
    run: &mut ReceiverRun,
    receivers: &[combat_aoe::AreaDamageReceiver],
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
) -> (DeathEffects, Vec<UnderAttackEvent>) {
    let current_tick = receiver_tick(world);

    let isolation_armed =
        area_near_center_ic_isolation_armed(receivers, &mut world.substrate.entities, current_tick);
    let mut effects = DeathEffects::default();
    let mut under_attack_events = Vec::new();

    for receiver in receivers {
        match *receiver {
            combat_aoe::AreaDamageReceiver::Entity(event) => {
                let (nested, mut pings) = commit_entities(
                    world,
                    run,
                    std::slice::from_ref(&event),
                    Some(isolation_armed),
                    rules,
                    overlay_registry,
                );
                effects.append(nested);
                under_attack_events.append(&mut pings);
            }
            combat_aoe::AreaDamageReceiver::Terrain(event) => {
                if isolation_armed && event.near_center_ic_isolation_eligible {
                    continue;
                }
                let (nested, mut pings) =
                    commit_terrain(world, run, event, false, rules, overlay_registry);
                effects.append(nested);
                under_attack_events.append(&mut pings);
            }
        }
    }

    (effects, under_attack_events)
}

/// Complete one TerrainClass receiver, including its nested C4 and removal.
/// Direct bridge calls use ignore_defenses=true; ordinary area calls retain
/// their damage kernel and perform the area-wide IC gate before entering here.
pub(crate) fn commit_terrain(
    world: &mut Simulation,
    run: &mut ReceiverRun,
    event: TerrainDamageEvent,
    ignore_defenses: bool,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
) -> (DeathEffects, Vec<UnderAttackEvent>) {
    let mut effects = DeathEffects::default();
    let mut under_attack_events = Vec::new();
    let Some(warhead) = rules
        .warhead(world.interner.resolve(event.warhead_ref))
        .cloned()
    else {
        return (effects, under_attack_events);
    };
    let receive = crate::sim::terrain_object::receive_terrain_damage_with_scenario(
        &mut world.production.terrain_objects,
        &world.production.terrain_object_cells,
        &mut run.finalizing_terrain,
        event.stable_id,
        (event.rx, event.ry),
        event.damage,
        event.distance_leptons,
        &warhead,
        rules,
        &mut world.interner,
        world.session.no_damage,
        ignore_defenses,
    );
    let TerrainAreaReceiveResult::Lethal(lethal) = receive else {
        return (effects, under_attack_events);
    };

    if lethal.spawns_tiberium
        && let Some(c4_warhead) = rules.warhead(&rules.bridge_warheads.c4_name).cloned()
    {
        let c4_id = world.interner.intern(&c4_warhead.id);
        let impact_z = world
            .resolved_terrain
            .as_ref()
            .and_then(|grid| grid.cell(lethal.cell.0, lethal.cell.1))
            .map_or(0, |cell| i32::from(cell.level));
        let aoe = {
            let collected = collect_area(
                world,
                rules,
                overlay_registry,
                lethal.cell,
                100,
                &c4_warhead,
                (RAD_NO_ATTACKER, None, c4_id),
                None,
                impact_z,
            );
            append_fixture_tiberium(world, &mut effects.tiberium_reduction_requests);
            collected
        };
        #[cfg(test)]
        effects.wall_mutations.extend(aoe.wall_mutations);

        #[cfg(test)]
        effects
            .cell_target_detaches
            .extend(aoe.cell_target_detaches);
        let (nested, mut pings) = commit_area(world, run, &aoe.receivers, rules, overlay_registry);
        effects.append(nested);
        under_attack_events.append(&mut pings);
    }

    let _ = crate::sim::terrain_object::finalize_terrain_lethal(
        crate::sim::terrain_object::production_authority_parts(
            &mut world.production,
            &mut world.substrate.raw_cell_occupation,
        ),
        &mut run.finalizing_terrain,
        &mut run.navigation_changed_cells,
        lethal,
        world.resolved_terrain.as_mut(),
    );
    (effects, under_attack_events)
}

pub(crate) fn commit_entities(
    world: &mut Simulation,
    run: &mut ReceiverRun,
    damage_events: &[EntityDamageEvent],
    near_center_ic_isolation_override: Option<bool>,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
) -> (DeathEffects, Vec<UnderAttackEvent>) {
    let sound_enabled = sound_enabled(world);

    let current_tick = receiver_tick(world);
    let scenario_no_damage = world.session.no_damage;

    let mut death = DeathEffects::default();
    let mut under_attack_events = Vec::new();
    // Apply_area_damage finishes collecting its fixed target/distance records
    // before dispatch. A qualifying near-center Iron Curtain therefore
    // isolates the entire eligible transaction, including records collected
    // before the arming record. The per-record check below remains live so an
    // earlier receiver or nested death effect can change later protection.
    let near_center_ic_isolation = near_center_ic_isolation_override.unwrap_or_else(|| {
        near_center_ic_isolation_armed(damage_events, &mut world.substrate.entities, current_tick)
    });

    for event in damage_events {
        if near_center_ic_isolation
            && event.near_center_ic_isolation_eligible
            && !world
                .substrate
                .entities
                .get(event.target_id)
                .is_some_and(|target| has_active_area_invulnerability(target, current_tick))
        {
            continue;
        }
        let target_id = event.target_id;
        let attacker_id = event.attacker_id;
        match apply_building_receive_prelude(
            event,
            &mut world.substrate.entities,
            rules,
            &mut world.interner,
            &mut world.houses,
            current_tick,
        ) {
            BuildingReceivePrelude::ReturnZero => continue,
            BuildingReceivePrelude::Respond => {
                if let Some(victim_owner) = world
                    .substrate
                    .entities
                    .get(event.target_id)
                    .map(|victim| victim.owner())
                {
                    let attacker_house_index = world
                        .substrate
                        .entities
                        .get(event.attacker_id)
                        .and_then(|attacker| {
                            world
                                .session
                                .house_order
                                .iter()
                                .position(|owner| *owner == attacker.owner())
                        })
                        .map_or(-1, |index| index as i32);
                    if let Some(owner) = world.houses.get_mut(&victim_owner) {
                        owner
                            .strategy_emergency
                            .note_building_attacker(attacker_house_index);
                    }
                }
                {
                    respond_to_base_attack(
                        world,
                        rules,
                        BaseDefenseResponseCallSite::BuildingPrelude,
                        event.target_id,
                        event.attacker_id,
                    );
                }
            }
            BuildingReceivePrelude::Continue => {}
        }
        // FootClass::ReceiveDamage 0x004D7330..0x004D7413 runs its parasite
        // prefix on the raw damage before TechnoClass::ReceiveDamage.
        if event.distance_leptons.is_some() {
            world.foot_receive_damage_parasite_prefix(
                target_id,
                (attacker_id != RAD_NO_ATTACKER).then_some(attacker_id),
                event.damage,
                event.warhead_ref,
                rules,
            );
        }
        // ReceiveDamage carries sourceHouse separately from the source object.
        // Area records snapshot it at detonation; legacy precomputed records
        // retain the former live-source lookup. Periodic radiation supplies
        // both null source object and null source house explicitly.
        let attacker_owner: Option<InternedId> = event.source_house.or_else(|| {
            (attacker_id != RAD_NO_ATTACKER)
                .then(|| {
                    world
                        .substrate
                        .entities
                        .get(attacker_id)
                        .map(|attacker| attacker.owner())
                })
                .flatten()
        });
        // UpdateAngerNodes reads source->Owner directly; the separately
        // captured source-house ABI argument is not used by this callback.
        let live_source_owner = (attacker_id != RAD_NO_ATTACKER)
            .then(|| {
                world
                    .substrate
                    .entities
                    .get(attacker_id)
                    .map(|source| source.owner())
            })
            .flatten();
        let receiver_outcome = event.distance_leptons.map(|_| {
            resolve_receive_damage(
                event,
                &mut world.substrate.entities,
                rules,
                &mut world.interner,
                &mut world.houses,
                &world.house_alliances,
                scenario_no_damage,
                current_tick,
                world.resolved_terrain.as_ref(),
            )
        });
        if let Some(effect) = receiver_outcome
            .flatten()
            .and_then(|resolved| resolved.invulnerability_impact)
        {
            death.invulnerability_impact_effects.push(effect);
        }
        let Some(receiver_health::ReceiverHealthCommit {
            building_entry_frame,
            became_fatal,
            state: receive_state,
            entered_techno_death,
            reached_exact_zero,
            postmortem_candidate,
            fatal_category,
            positive_postlude,
            synchronous_retaliation,
            smoke_maintenance,
            healing_only,
            latch_hostile_hit,
            uncloak_after_damage,
            building_damage_cue,
            voice_feedback_cue,
            threat_feedback,
        }) = receiver_health::commit_receiver_health(
            event,
            &mut world.substrate.entities,
            rules,
            &mut world.interner,
            &world.house_alliances,
            attacker_owner,
            live_source_owner,
            receiver_outcome,
            current_tick,
        )
        else {
            continue;
        };

        // gamemd-derived: `TechnoClass__ReceiveDamage @
        // 0x007027AE..0x007027EE` invokes the protected-Techno response after
        // ObjectClass has committed health/visual state and before the dead
        // branch. `ShouldProtect+0x3CF` has no active YR writer; `ToProtect=`
        // is the exact live gate retained here.
        let protected_techno_response = event.distance_leptons.is_some()
            && attacker_id != RAD_NO_ATTACKER
            && world
                .substrate
                .entities
                .get(target_id)
                .is_some_and(|target| {
                    rules
                        .object(world.interner.resolve(target.type_ref()))
                        .is_some_and(|object| object.to_protect)
                        && world
                            .houses
                            .get(&target.owner())
                            .is_some_and(|house| !house.is_human)
                });
        if protected_techno_response {
            respond_to_base_attack(
                world,
                rules,
                BaseDefenseResponseCallSite::ProtectedTechno,
                target_id,
                attacker_id,
            );
        }

        // `0x0070281D`, in its native slot: after the `ToProtect` response
        // (`0x007027E9`) and before the death branch's kill callbacks. The
        // `StartUncloaking(0)` argument is zero, so it owns one positional
        // `[AudioVisual] CloakSound`.
        if uncloak_after_damage {
            // Production combat callers derive `current_tick` from
            // `ScenarioSession::binary_frame`, which is the clock the cloak step
            // timer is compared against in `CloakingTick`.
            let now = current_tick as i32;
            let cloaking_speed = world
                .substrate
                .entities
                .get(target_id)
                .and_then(|target| rules.object(world.interner.resolve(target.type_ref())))
                .map_or(1, |object| object.cloaking_speed);
            let surfaced = world
                .substrate
                .entities
                .get_mut(target_id)
                .and_then(|target| target.cloak.as_mut())
                .map(|cloak| cloak.start_uncloaking_from_damage(now, cloaking_speed));
            if surfaced.is_some_and(|result| result.play_sound)
                && let Some(sound_name) = rules.general.cloak_sound.as_deref()
                && let Some(sink) = sound_enabled.then_some(&mut world.sound_events)
                && let Some(target) = world.substrate.entities.get(target_id)
            {
                sink.push(SimSoundEvent::cloak_sound(
                    sound_name.to_owned(),
                    &target.position,
                ));
            }
        }

        if reached_exact_zero && let Some(target) = world.substrate.entities.get_mut(target_id) {
            // ObjectClass routes its kill callback while Health is exactly zero,
            // before Destroy's reference notification and before TechnoClass's
            // victim-house anger callback.
            capture_kill_credit(target, attacker_owner, rules, &mut world.interner);
        }
        // `Record_The_Kill` awards the killer's experience in the same call, so
        // it is a same-tick write the victim's own death effects can already
        // observe. Deferring it to the lifecycle release point would change that
        // visibility.
        if reached_exact_zero {
            award_kill_experience(
                &mut world.substrate.entities,
                rules,
                &mut world.interner,
                &world.house_alliances,
                attacker_id,
                target_id,
            );
        }
        // ObjectClass::ReceiveDamage 0x005F5765..0x005F57AF: after the kill
        // callback the exact-zero arm runs Destroy = Detach_All(1), so every
        // listener drops the dying object at the killing hit, before the
        // anger callback and TechnoClass's death arm. The CausesDelayKill
        // PostMortem stage below runs the same callback after its bookkeeping.
        if reached_exact_zero && postmortem_candidate.is_none() && callbacks_enabled(world) {
            world.object_destroy_callback(
                target_id,
                crate::sim::world::UninitContext::with_rules(rules),
            );
        }
        if postmortem_candidate.is_some() {
            if callbacks_enabled(world) {
                world.apply_fatal_lifecycle_stage(
                    rules,
                    FatalLifecycleStage::PostMortemExactZero {
                        killer_owner: attacker_owner,
                    },
                    target_id,
                    fatal_category,
                    crate::sim::world::UninitContext::with_rules(rules),
                );
            };
        }
        if let Some((victim_owner, source_owner, final_damage, strength, cost)) = threat_feedback {
            let delta = receiver_anger_delta(final_damage, strength, cost);
            update_anger_nodes(
                &mut world.houses,
                &world.session.house_order,
                &world.house_alliances,
                &mut world.interner,
                victim_owner,
                source_owner,
                delta,
            );
            #[cfg(test)]
            death
                .receiver_stage_trace
                .push(ReceiverStageTrace::HouseThreat { target_id, delta });
        }
        if let Some(duration_frames) = postmortem_candidate {
            let current_frame = current_tick as u32 as i32;
            let target = world
                .substrate
                .entities
                .get_mut(target_id)
                .expect("PostMortem exact-zero callbacks retain the represented target");
            // RecordKill already consumed this exact-zero attribution in the
            // synchronous PostMortem hook. Native retains no killer on the
            // restored object: a fresh null-source timer expiry must stay
            // uncredited, while a later sourced lethal hit captures anew.
            target.killed_by = None;
            target.kill_award_points = 0;
            let replace = target
                .pending_c4_detonation
                .is_none_or(|pending| duration_frames < pending.remaining_at(current_frame));
            if replace {
                let retained_source = target
                    .pending_c4_detonation
                    .and_then(|pending| pending.source_entity_id);
                target.pending_c4_detonation = Some(crate::sim::components::PendingC4Detonation {
                    start_frame: current_frame,
                    duration_frames,
                    source_entity_id: retained_source,
                });
            }
            target.lifecycle.object_alive = true;
            target.health.current = 1;
            target.dying = false;
            #[cfg(test)]
            death
                .receiver_stage_trace
                .push(ReceiverStageTrace::PostMortem { target_id });
            continue;
        }
        if latch_hostile_hit && let Some(target) = world.substrate.entities.get_mut(target_id) {
            target.was_attacked_by_enemy = true;
        }

        if let Some((category, state)) = smoke_maintenance {
            if callbacks_enabled(world) {
                world.apply_fatal_lifecycle_stage(
                    rules,
                    FatalLifecycleStage::MaintainDamageSmoke { state },
                    target_id,
                    category,
                    crate::sim::world::UninitContext::with_rules(rules),
                );
            };
        }
        if healing_only {
            finish_building_art_receiver(
                world,
                target_id,
                receive_state,
                building_entry_frame,
                rules,
            );
            continue;
        }

        if let Some((damage, _reached_survivor_postlude, _hostile_source)) = positive_postlude {
            // InfantryClass's concrete receiver dispatches Scatter only for a
            // surviving result state (1..=3), after HP has changed and before
            // fear or the shared Techno postlude. The attacker coordinate is
            // read while the source is still represented, including nested
            // DeathWeapon receiver recursion.
            let surviving_infantry_result = matches!(
                receive_state,
                damage::DamageState::Damaged
                    | damage::DamageState::Yellow
                    | damage::DamageState::Red
            );
            let attacker_coord = (attacker_id != RAD_NO_ATTACKER)
                .then(|| world.substrate.entities.get(attacker_id))
                .flatten()
                .map(|attacker| {
                    (
                        i32::from(attacker.position.rx)
                            .wrapping_mul(256)
                            .wrapping_add(attacker.position.sub_x.to_num::<i32>()),
                        i32::from(attacker.position.ry)
                            .wrapping_mul(256)
                            .wrapping_add(attacker.position.sub_y.to_num::<i32>()),
                    )
                });
            let scatter = if surviving_infantry_result {
                attacker_coord.and_then(|attacker_coord| {
                    world.select_infantry_damage_scatter(
                        target_id, attacker_coord, rules, overlay_registry,
                    ).unwrap_or_else(|cause| {
                        panic!("damage Scatter requires valid live infantry entry state for {target_id}: {cause}")
                    })
                })
            } else {
                None
            };
            if let Some(scatter) = scatter {
                if let Some(target) = world.substrate.entities.get_mut(target_id) {
                    queue_entity_mission_deferred(target, MissionId::from_known(MissionType::Move));
                }
                let walk = world
                    .assign_damage_scatter_walk_destination(
                        target_id,
                        scatter,
                        rules,
                        overlay_registry,
                    )
                    .unwrap_or_else(|cause| {
                        panic!("damage Scatter destination for {target_id}: {cause}")
                    });
                if !walk {
                    // Residual: non-Walk and JumpJet's class-switching setter
                    // still use the prior compatibility handoff. This is not
                    // a second implementation of the migrated Walk branch.
                    let target = world.substrate.entities.get_mut(target_id).unwrap();
                    crate::sim::mission::concrete_effects::represented_assign_destination_mode_one(
                        target,
                        Some(crate::sim::components::NavTargetRef::cell(
                            scatter.destination.0,
                            scatter.destination.1,
                        )),
                    );
                    crate::sim::movement::issue_direct_move(
                        &mut world.substrate.entities,
                        target_id,
                        scatter.destination,
                        scatter.speed,
                        crate::sim::movement::DestinationTiming::from_rules(
                            world.session.binary_frame,
                            Some(rules),
                        ),
                    );
                }
            }

            let Some(target) = world.substrate.entities.get_mut(target_id) else {
                continue;
            };
            if let Some(obj) = rules.object(world.interner.resolve(target.type_ref())) {
                infantry::apply_fear_from_damage(
                    obj,
                    target,
                    damage,
                    true,
                    rules.general.condition_red,
                    rules.general.condition_yellow,
                );
            }
            // Neither native ping survives the killing blow.
            // `UnitClass::ReceiveDamage @ 0x00737C90`: `0x00737D69 CMP EAX,4`
            // sends result 4 to the death branch (`+0x3B8`
            // `Death_Announcement`, `Death_Explosion`); the `Harvester=` ping
            // (`0x007384B9..0x00738530`) sits only in the `result != 4` arm.
            // `BuildingClass::ReceiveDamage @ 0x00442230`: case 4 calls
            // `DestructionEffects` (slot `+0x4EC` = `0x004415F0`, which never
            // writes `+0x90`) and then, for the stock timer (`+0x530` = 8),
            // `ObjectClass::UnInit` (`0x005F65F0`, clears `IsAlive +0x90` at
            // `0x005F6625`), so the `0x00442905` re-test returns 4 before
            // `NotifyUnderAttack`. RESIDUAL: `DestructionEffects` arms a zero
            // timer for `BuildingType+0xD15` types and a building whose
            // current mission is Selling (0x13), leaving `IsAlive` set and the
            // ping live on their killing blow; neither is modelled here (walls
            // are overlay mutations, a sold building dying is rare).
            if damage > 0
                && !became_fatal
                && let Some(obj) = rules.object(world.interner.resolve(target.type_ref()))
            {
                // `BuildingClass::ReceiveDamage @ 0x00442230`: with a non-null
                // source, a non-zero damage result and `Insignificant=` clear,
                // `HouseClass::NotifyUnderAttack` runs — there is no
                // attacker-house test on that path, so a force-fired own
                // building announces. (Both native sites first test the
                // victim's `vtbl+0x80` predicate; its identity is not pinned
                // here and is not modelled — VERA-internal, gamemd equivalent
                // UNCHECKED.) `NotifyUnderAttack
                // 0x004F9491..0x004F94A3` routes a building whose type has
                // `UndeploysInto=` and `ResourceGatherer=yes` (the deployed
                // slave miner) to the ore-miner line.
                //
                // `UnitClass::ReceiveDamage 0x007384B9..0x00738530`: a
                // `Harvester=` unit pings on any non-zero non-fatal result,
                // with or without a source.
                let ping = if target.category == EntityCategory::Structure {
                    (attacker_owner.is_some() && !obj.insignificant)
                        .then_some((obj.undeploys_into.is_some() && obj.resource_gatherer, true))
                } else {
                    obj.harvester.then_some((true, false))
                };
                if let Some((miner, structure)) = ping {
                    under_attack_events.push(UnderAttackEvent {
                        rx: target.position.rx,
                        ry: target.position.ry,
                        owner: target.owner(),
                        miner,
                        structure,
                    });
                }
            }
            // Legacy precomputed callers have not yet entered the authoritative
            // receiver ABI, so retain their Phase-6 handoff. Ordered area/direct
            // receiver hits make their retaliation decision synchronously below.
            if attacker_id != RAD_NO_ATTACKER && event.distance_leptons.is_none() {
                target.last_attacker_id = Some(attacker_id);
            }
        }

        // Native order: the TechnoClass arm runs inside
        // `TechnoClass::ReceiveDamage @ 0x00701900`, which `BuildingClass::
        // ReceiveDamage` only resumes after at `0x00442425`, so the voice
        // precedes the building cue.
        if let Some((owner, type_ref, rx, ry)) = voice_feedback_cue
            && let Some(sink) = sound_enabled.then_some(&mut world.sound_events)
        {
            sink.push(SimSoundEvent::VoiceFeedback {
                owner,
                type_ref,
                rx,
                ry,
            });
        }

        if let Some((rx, ry)) = building_damage_cue
            && let Some(sink) = sound_enabled.then_some(&mut world.sound_events)
        {
            sink.push(SimSoundEvent::BuildingDamagedSfx { rx, ry });
        }

        if synchronous_retaliation && attacker_id != RAD_NO_ATTACKER {
            #[cfg(test)]
            death
                .receiver_stage_trace
                .push(ReceiverStageTrace::ShouldRetaliate { target_id });
            if combat_targeting::should_retaliate_from_damage(
                &mut world.substrate.entities,
                target_id,
                attacker_id,
                rules,
                &mut world.interner,
                &mut world.houses,
                &world.house_alliances,
                world.resolved_terrain.as_ref(),
            ) {
                override_mission_on_damage_response(
                    &mut world.substrate.entities,
                    target_id,
                    attacker_id,
                );
            }
        }

        if entered_techno_death {
            {
                if callbacks_enabled(world) {
                    world.apply_fatal_lifecycle_stage(
                        rules,
                        FatalLifecycleStage::BeforeDeathEffects,
                        target_id,
                        fatal_category,
                        crate::sim::world::UninitContext::with_rules(rules),
                    );
                };
            }
            let mut nested = handle_death(
                world,
                run,
                &[target_id],
                std::slice::from_ref(event),
                rules,
                overlay_registry,
            );
            under_attack_events.append(&mut nested.under_attack_events);
            if matches!(
                fatal_category,
                EntityCategory::Unit | EntityCategory::Structure
            ) {
                {
                    if callbacks_enabled(world) {
                        world.apply_fatal_lifecycle_stage(
                            rules,
                            FatalLifecycleStage::AfterDeathEffects,
                            target_id,
                            fatal_category,
                            crate::sim::world::UninitContext::with_rules(rules),
                        );
                    };
                }
                if callbacks_enabled(world) {
                    nested
                        .immediate_uninit_ids
                        .retain(|&dead_id| dead_id != target_id);
                }
            }
            death.append(nested);
        }
        finish_building_art_receiver(world, target_id, receive_state, building_entry_frame, rules);
    }

    (death, under_attack_events)
}

/// Building442A95 refreshes for nonzero returned result;442B37 independently
/// refreshes if current body frame differs from the entry value. Result5 and
/// cleared ObjectAlive leave before either arm. In particular healing result0
/// must not unconditionally synchronize the retained animation flag.
fn finish_building_art_receiver(
    world: &mut Simulation,
    target_id: u64,
    state: damage::DamageState,
    entry_frame: Option<i32>,
    rules: &RuleSet,
) {
    let Some(entry_frame) = entry_frame else {
        return;
    };
    if state == damage::DamageState::AlreadyDead {
        return;
    }
    let Some(target) = world.substrate.entities.get(target_id) else {
        return;
    };
    if !target.lifecycle.object_alive {
        return;
    }
    let Some(object) = rules.object(world.interner.resolve(target.type_ref())) else {
        return;
    };
    if state != damage::DamageState::Unaffected
        || crate::sim::building_art::receiver_body_frame(target, object, rules) != entry_frame
    {
        world.refresh_building_damage_state(target_id, rules);
    }
}

pub(crate) fn handle_death(
    world: &mut Simulation,
    run: &mut ReceiverRun,
    dead_entities: &[u64],
    damage_events: &[EntityDamageEvent],
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
) -> DeathEffects {
    let handles = world.rule_handles;
    let scenario_no_damage = world.session.no_damage;

    debug_assert!(
        dead_entities.len() <= 1,
        "ReceiveDamage enters one concrete fatal postlude at a time"
    );
    let mut death_sounds: Vec<(InternedId, u16, u16)> = Vec::new();
    #[cfg(test)]
    let mut receiver_stage_trace = Vec::new();
    let mut tiberium_reduction_requests: Vec<TiberiumReductionRequest> = Vec::new();
    // Death-weapon detonations use the destroyed object's game-space position.
    // The cell and z still drive damage/smudge dispatch; sub-cell leptons keep
    // AnimList placement aligned with the detonation CoordStruct shape.
    let mut death_aoe: Vec<(
        u16,
        u16,
        SimFixed,
        SimFixed,
        u8,
        i32,
        Option<combat_aoe::AoEAirImpact>,
        i32,
        InternedId,
        InternedId,
        u64,
        InternedId,
    )> = Vec::new();
    let mut despawned_ids: Vec<u64> = Vec::new();
    let mut immediate_uninit_ids: Vec<u64> = Vec::new();
    let mut destroyed_crewed_buildings: Vec<DestroyedCrewedBuilding> = Vec::new();
    let mut explosion_effects: Vec<ExplosionEffect> = Vec::new();
    let mut voxel_debris: Vec<crate::sim::voxel_anim::VoxelDebrisSpawn> = Vec::new();
    let mut invulnerability_impact_effects: Vec<InvulnerabilityImpactEffect> = Vec::new();
    let mut bridge_damage_events: Vec<BridgeDamageEvent> = Vec::new();
    #[cfg(test)]
    let mut wall_mutations: Vec<WallMutation> = Vec::new();
    #[cfg(test)]
    let mut cell_target_detaches: Vec<combat_aoe::CellTargetDetach> = Vec::new();
    let mut smudge_spawn_requests: Vec<SmudgeSpawnRequest> = Vec::new();
    let mut rad_detonations: Vec<crate::sim::radiation::RadDetonation> = Vec::new();
    let mut under_attack_events: Vec<UnderAttackEvent> = Vec::new();
    let mut unit_lost_events: Vec<UnitLostEvent> = Vec::new();
    let mut structure_destroyed: bool = false;
    for &dead_id in dead_entities {
        // Native702035 re-enters this branch for an already-zero receiver.
        // Keep unique diagnostic IDs, without suppressing receiver effects.
        if !run.handled_deaths.contains(&dead_id) {
            run.handled_deaths.push(dead_id);
        }
        let dead_info = world.substrate.entities.get(dead_id).map(|e| {
            if e.category == EntityCategory::Structure {
                structure_destroyed = true;
            }
            let air_impact = combat_aoe::air_impact_from_entity(e, world.resolved_terrain.as_ref());
            let world_z_leptons = object_world_z_leptons(e, world.resolved_terrain.as_ref());
            (
                e.type_ref(),
                e.position.rx,
                e.position.ry,
                e.position.sub_x,
                e.position.sub_y,
                e.position.z,
                world_z_leptons,
                air_impact,
                e.owner(),
                e.category,
                e.veterancy,
                e.current_weapon_index,
                e.current_weapon_ref,
            )
        });

        if let Some((
            type_id,
            rx,
            ry,
            sub_x,
            sub_y,
            z,
            world_z_leptons,
            air_impact,
            owner,
            category,
            veterancy,
            current_weapon_index,
            current_weapon_ref,
        )) = dead_info
        {
            let type_id_str = world.interner.resolve(type_id);
            if let Some(obj) = rules.object(type_id_str) {
                append_selected_death_sounds(
                    obj,
                    category,
                    rules.general.building_die_sound.as_deref(),
                    world.houses.get(&owner).is_some_and(|house| house.is_human),
                    &mut world.main_rng,
                    &mut world.interner,
                    rx,
                    ry,
                    &mut death_sounds,
                );
                // 0x00702206..0x00702210: RADIO OVER_OUT to every contact and
                // the Stun, between the death sounds and the debris.
                if callbacks_enabled(world) {
                    world.techno_death_stun(
                        dead_id,
                        crate::sim::world::UninitContext::with_rules(rules),
                    );
                }
                // gamemd-derived: the debris block of
                // `TechnoClass::ReceiveDamage @ 0x00701900`
                // (`0x00702281`..`0x0070256C`). It sits BELOW the two death
                // sound draws in the same destruction arm and ABOVE
                // `UnitClass::Death_Explosion`, which `UnitClass::ReceiveDamage
                // @ 0x00737C90` only calls after this function has returned 4 —
                // so it lands here, between the sounds and the `Explosion=` /
                // `DestroyAnim=` draws below.
                throw_debris_for_death(
                    obj,
                    rules,
                    &mut world.interner,
                    owner,
                    rx,
                    ry,
                    sub_x,
                    sub_y,
                    z,
                    world_z_leptons,
                    &mut world.scenario_rng,
                    &mut voxel_debris,
                    &mut explosion_effects,
                );
                if let Some((dmg, wh_id, weapon_id)) = death_weapon_aoe(
                    rules,
                    obj,
                    veterancy,
                    current_weapon_index,
                    current_weapon_ref,
                    &mut world.interner,
                ) {
                    death_aoe.push((
                        rx,
                        ry,
                        sub_x,
                        sub_y,
                        z,
                        world_z_leptons,
                        air_impact,
                        dmg,
                        wh_id,
                        weapon_id,
                        dead_id,
                        owner,
                    ));
                }
            }

            // The world fatal prelude already owns garrison ejection before
            // the nested death weapon. Generic cargo remains attached only in
            // callback-disabled receiver fixtures, for their UnInit assertions.
        }
    }

    // Apply death explosion AoE damage.
    for (
        rx,
        ry,
        sub_x,
        sub_y,
        z,
        world_z_leptons,
        air_impact,
        dmg,
        wh_id,
        weapon_id,
        source_id,
        owner_id,
    ) in &death_aoe
    {
        if let Some(warhead) = rules.warhead(world.interner.resolve(*wh_id)) {
            let routed_wall =
                wall_overlay_flags_at(world.overlay_grid.as_ref(), overlay_registry, *rx, *ry)
                    .is_some_and(|flags| warhead_damages_wall(warhead, flags));
            let aoe = {
                let collected = collect_area(
                    world,
                    rules,
                    overlay_registry,
                    (*rx, *ry),
                    *dmg,
                    warhead,
                    (*source_id, Some(*owner_id), *wh_id),
                    *air_impact,
                    i32::from(*z),
                );
                append_fixture_tiberium(world, &mut tiberium_reduction_requests);
                collected
            };
            #[cfg(test)]
            wall_mutations.extend(aoe.wall_mutations);

            #[cfg(test)]
            cell_target_detaches.extend(aoe.cell_target_detaches);
            if !scenario_no_damage && !routed_wall && warhead.wall && *dmg > 0 {
                let wh_iid = *wh_id;
                bridge_damage_events.push(BridgeDamageEvent {
                    rx: *rx,
                    ry: *ry,
                    damage: (*dmg).min(i32::from(u16::MAX)) as u16,
                    warhead_ref: wh_iid,
                    is_ion_cannon: wh_iid
                        == handles
                            .expect("Simulation::resolve_type_handles must run before combat")
                            .ion_cannon,
                    impact_z: *z as i32,
                });
            }
            if let Some(weapon) = rules.weapon(world.interner.resolve(*weapon_id))
                && weapon.rad_level > 0
            {
                rad_detonations.push(crate::sim::radiation::RadDetonation {
                    rx: *rx,
                    ry: *ry,
                    rad_level: weapon.rad_level,
                    spread: warhead.cell_spread.to_num::<i32>(),
                });
            }
            // One native Apply_area_damage owns the whole fixed record vector.
            // The commit loop still enters ReceiveDamage/death effects inline
            // per record, while retaining transaction-wide IC isolation.
            let (mut nested, mut pings) =
                commit_area(world, run, &aoe.receivers, rules, overlay_registry);
            despawned_ids.append(&mut nested.despawned_ids);
            immediate_uninit_ids.append(&mut nested.immediate_uninit_ids);
            structure_destroyed |= nested.structure_destroyed;
            destroyed_crewed_buildings.append(&mut nested.destroyed_crewed_buildings);
            explosion_effects.append(&mut nested.explosion_effects);
            voxel_debris.append(&mut nested.voxel_debris);
            invulnerability_impact_effects.append(&mut nested.invulnerability_impact_effects);
            bridge_damage_events.append(&mut nested.bridge_damage_events);
            #[cfg(test)]
            wall_mutations.append(&mut nested.wall_mutations);

            #[cfg(test)]
            cell_target_detaches.append(&mut nested.cell_target_detaches);
            tiberium_reduction_requests.append(&mut nested.tiberium_reduction_requests);
            death_sounds.append(&mut nested.death_sounds);
            smudge_spawn_requests.append(&mut nested.smudge_spawn_requests);
            rad_detonations.append(&mut nested.rad_detonations);
            unit_lost_events.append(&mut nested.unit_lost_events);
            #[cfg(test)]
            receiver_stage_trace.append(&mut nested.receiver_stage_trace);
            under_attack_events.append(&mut pings);
            let outer_anim_start = smudge_spawn_requests.len();
            emit_warhead_detonation_effects(
                warhead,
                *dmg,
                *rx,
                *ry,
                *sub_x,
                *sub_y,
                *z,
                *world_z_leptons,
                &mut world.interner,
                &mut explosion_effects,
                &mut smudge_spawn_requests,
            );
            let outer_anim_requests = smudge_spawn_requests.split_off(outer_anim_start);
            commit_smudges(
                world,
                rules,
                overlay_registry,
                outer_anim_requests,
                &mut smudge_spawn_requests,
            );
        }
    }
    let mut effects = DeathEffects {
        despawned_ids,
        immediate_uninit_ids,
        structure_destroyed,
        destroyed_crewed_buildings,
        explosion_effects,
        voxel_debris,
        invulnerability_impact_effects,
        bridge_damage_events,
        #[cfg(test)]
        wall_mutations,
        #[cfg(test)]
        cell_target_detaches,
        tiberium_reduction_requests,
        death_sounds,
        smudge_spawn_requests,
        rad_detonations,
        under_attack_events,
        unit_lost_events,
        #[cfg(test)]
        receiver_stage_trace,
    };

    // Concrete receivers resume after Techno's nested DeathWeapon. Read the
    // retained entity's current state at that boundary.
    for &dead_id in dead_entities {
        finish_concrete_death(
            world,
            dead_id,
            damage_events,
            rules,
            overlay_registry,
            &mut effects,
        );
    }

    effects
}

/// Concrete receiver work after shared Techno death effects return.
/// Evidence: Infantry517FA0, Unit737C90 and Building442230 base-call order.
fn finish_concrete_death(
    world: &mut Simulation,
    dead_id: u64,
    damage_events: &[EntityDamageEvent],
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    effects: &mut DeathEffects,
) {
    let Some(entity) = world.substrate.entities.get(dead_id) else {
        return;
    };
    let category = entity.category;
    // Building442230 returns before its result dispatch when Alive is clear.
    if category == EntityCategory::Structure && !entity.lifecycle.object_alive {
        return;
    }
    let (type_id, owner, rx, ry, sub_x, sub_y, z, has_animation) = (
        entity.type_ref(),
        entity.owner(),
        entity.position.rx,
        entity.position.ry,
        entity.position.sub_x,
        entity.position.sub_y,
        entity.position.z,
        entity.animation.is_some(),
    );
    let mut concrete_smudge_plans = Vec::new();
    if let Some(obj) = rules.object(world.interner.resolve(type_id)) {
        // `TechnoClass::Death_Announcement @ 0x004D98C0` runs at the
        // Aircraft/Infantry/Unit `ReceiveDamage` kill sites (vtable
        // `+0x3B8`; `BuildingClass` has none) and skips `Spawned=`
        // types at `0x004D98DD`. Its owner gate (`0x0050B6F0`) and
        // the `CreateRadarEvent(7)` dedupe (`0x004D98FE`) need the
        // house table and radar queue, which the world owns.
        if let Some(event) = death_announcement_event(obj, category, rx, ry, owner) {
            effects.unit_lost_events.push(event);
        }
        // Crewed structures eject infantry survivors on destruction.
        if obj.crewed && category == EntityCategory::Structure {
            effects
                .destroyed_crewed_buildings
                .push(DestroyedCrewedBuilding {
                    type_id: type_id,
                    owner: owner,
                    rx,
                    ry,
                    z,
                });
        }
    }
    // Look up the warhead that dealt the killing blow for InfDeath
    // selection below. The AnimList anim + smudge are emitted at
    // the per-shot fire site (and at the death-AoE loop), not here.
    let killing_warhead = damage_events
        .iter()
        .rfind(|event| event.target_id == dead_id)
        .and_then(|event| {
            rules
                .warhead(world.interner.resolve(event.warhead_ref))
                .map(|wh| (wh, event.damage))
        });

    // BuildingClass runs DestructionEffects/SpawnSurvivors only after
    // TechnoClass's synchronous death weapon has returned. Capture the
    // immutable plan now; placement and all RNG stay at that postlude.
    if category == EntityCategory::Structure {
        let foundation = rules
            .object(world.interner.resolve(type_id))
            .map(|obj| obj.foundation.as_str())
            .unwrap_or("1x1");
        concrete_smudge_plans.push(ConcreteDeathSmudgePlan::Building {
            rx,
            ry,
            z: i32::from(z),
            foundation: foundation.to_owned(),
        });
    }

    // `UnitClass::Death_Explosion @ 0x00738680` for vehicles and
    // `AircraftClass::ReceiveDamage @ 0x0041661F` for aircraft: after
    // the killing warhead's own `AnimList=` anim, the dying object plays
    // ONE anim drawn from its type's `Explosion=` list and then one from
    // `DestroyAnim=`, both at its own coordinate, one `Random__Next()`
    // draw each. Without this a Grizzly and an Apocalypse died with the
    // same warhead-derived puff.
    //
    // INFANTRY are excluded deliberately: `get_xrefs_to 0x00738680`
    // returns four callers, all `UnitClass`, and an operand scan for the
    // `Explosion=` count (`type+0x73C`) finds readers only in
    // `AircraftClass::ReceiveDamage` and
    // `BuildingClass::DestructionEffects @ 0x0044194D`. Infantry death
    // spawns from the `InfDeath`/`DeathAnims` table alone. No stock
    // `[InfantryTypes]` section authors `Explosion=` either, so this is
    // a contract correction rather than a visible one.
    //
    // RESIDUAL (GSI-08.11) — the BUILDING arm is not modelled. Native
    // runs `BuildingClass::DestructionEffects @ 0x004415F0`, which plays
    // the `Explosion=` list once PER FOUNDATION CELL at cell centre with
    // a scatter helper and a `RandomRanged(0, 3)` anim delay, then one
    // `DestroyAnim=` at the building coordinate; the sub-order of the
    // scatter, delay and index draws is UNCHECKED, and getting it wrong
    // would misroute the stream for every structure death. `Explodes=`
    // (forcing the last `Explosion=` entry for a loaded miner) is
    // likewise unread. Trigger: every building death, and every loaded
    // miner death. Frequency: continuous.
    //
    // Landing that arm also un-latents a second gap: four of the
    // buildings it would newly reach — the base power plant of every
    // faction (`GAPOWR`, `NAPOWR`, `YAPOWR`) plus `YAROCK` — author an
    // `Explosion=` list whose sixth entry (`gtpowexp`/`tstlexp`) has no
    // art section, so one draw in six per foundation cell resolves to
    // nothing VERA can construct. That is correct against gamemd, which
    // also draws nothing there, but VERA skips the object entirely
    // where native still constructs and discards one. Read the residual
    // on `ArtRegistry::bind_anim_class_assets` before
    // treating a missing power-plant explosion as a bug.
    if matches!(category, EntityCategory::Unit | EntityCategory::Aircraft)
        && let Some(obj) = rules.object(world.interner.resolve(type_id))
    {
        for list in [&obj.explosion_anims, &obj.destroy_anims] {
            if list.is_empty() {
                continue;
            }
            // `UnitClass::Death_Explosion @ 0x00738680` takes both picks on
            // the Scenario stream (`[0x00A8B230]+0x218`: Explosion= at
            // `0x007386A7`, DestroyAnim= at `0x0073881D`); the Aircraft arm
            // loads the same instance at `0x0041663C`.
            let index = (world.scenario_rng.next_u32() % list.len() as u32) as usize;
            let shp_name = world.interner.intern(&list[index]);
            effects.explosion_effects.push(ExplosionEffect {
                shp_name,
                rx,
                ry,
                sub_x,
                sub_y,
                z,
            });
        }
    }

    let inf_death = killing_warhead.as_ref().map_or(1, |(wh, _)| wh.inf_death);
    if category == EntityCategory::Infantry {
        let postlude = world.begin_infantry_receiver_death(
            dead_id,
            inf_death,
            &mut effects.immediate_uninit_ids,
        );
        concrete_smudge_plans.push(ConcreteDeathSmudgePlan::Infantry(postlude));
        effects.despawned_ids.push(dead_id);
    } else if has_animation {
        // Non-Infantry SHP lifetime remains on its existing path.
        if let Some(entity) = world.substrate.entities.get_mut(dead_id) {
            entity.dying = true;
            if let (Some(sequence), Some(anim)) = (
                crate::sim::animation::death_sequence_for_inf_death(inf_death),
                entity.animation.as_mut(),
            ) {
                anim.switch_to(sequence);
            }
        }
        // The corpse stays in the store for its death animation; the death
        // arm's OVER_OUT to every contact (`techno_death_stun`) already
        // released its dock slots.
        effects.despawned_ids.push(dead_id);
    } else {
        effects.immediate_uninit_ids.push(dead_id);
        effects.despawned_ids.push(dead_id);
    }
    // The receiver owns the recursion boundary; each concrete owner consumes
    // its captured postlude here without moving constructor/smudge RNG earlier.
    for plan in concrete_smudge_plans {
        match plan {
            ConcreteDeathSmudgePlan::Infantry(postlude) => {
                postlude.commit(world, rules, overlay_registry, effects);
            }
            ConcreteDeathSmudgePlan::Building {
                rx,
                ry,
                z,
                foundation,
            } => {
                let mut requests = Vec::new();
                append_building_smudge_requests(&mut requests, rx, ry, z, &foundation);
                commit_smudges(
                    world,
                    rules,
                    overlay_registry,
                    requests,
                    &mut effects.smudge_spawn_requests,
                );
            }
        }
    }
}

fn emit_one_projectile_detonation(
    world: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    detonation: &ProjectileDetonation,
    out: &mut CombatEmit,
) {
    let handles = world.rule_handles;
    let scenario_no_damage = world.session.no_damage;

    let Some(warhead) = rules.warhead(world.interner.resolve(detonation.payload.warhead)) else {
        log::warn!(
            "Projectile {} dropped: missing serialized warhead {}",
            detonation.projectile_id,
            detonation.payload.warhead
        );
        return;
    };
    let (impact_rx, impact_ry, impact_sub_x, impact_sub_y, world_z_leptons) =
        projectile_impact_cell(detonation.impact);
    let impact_z = world_z_leptons.div_euclid(LEPTONS_PER_LEVEL as i32);
    let air_impact = Some(combat_aoe::AoEAirImpact {
        sub_x: impact_sub_x,
        sub_y: impact_sub_y,
        z_leptons: world_z_leptons,
    });

    // Named location: `BulletClass::Detonate @ 0x004690b0`. Radiation is
    // outside and before the exclusive special-effect chain.
    if let Some(weapon) = rules.weapon(world.interner.resolve(detonation.payload.weapon))
        && weapon.rad_level > 0
    {
        out.effects
            .rad_detonations
            .push(crate::sim::radiation::RadDetonation {
                rx: impact_rx,
                ry: impact_ry,
                rad_level: weapon.rad_level,
                spread: warhead.cell_spread.to_num::<i32>(),
            });
    }

    // `BulletClass::DetonateAtCoord @ 0x0046978e` is the chain's only
    // conditional arm and the only one that consults the target: it reads the
    // bullet's raw `+0x10c` and calls `What_Am_I` through vtable `+0x2c`,
    // claiming the impact only for RTTI 1 (`UnitClass::What_Am_I
    // @ 0x00746e20`). A cell, dummy-cell or null target is never a UnitClass.
    let special_target = SpecialDetonationTarget {
        is_unit: match detonation.target {
            ProjectileTarget::Entity(id) => world
                .substrate
                .entities
                .get(id)
                .is_some_and(|entity| entity.category == EntityCategory::Unit),
            ProjectileTarget::Cell { .. }
            | ProjectileTarget::None
            | ProjectileTarget::DummyCell => false,
        },
    };
    let special_action = projectile_special_detonation_action(
        SpecialDetonationFlags {
            mind_control: warhead.mind_control,
            ivan_bomb: warhead.ivan_bomb,
            electric_assault: warhead.electric_assault,
            parasite: warhead.parasite,
            temporal: warhead.temporal,
            is_locomotor: warhead.is_locomotor,
            airstrike: warhead.airstrike,
            direct_rocker: warhead.direct_rocker,
            bomb_disarm: warhead.bomb_disarm,
            makes_disguise: warhead.makes_disguise,
            nuke_maker: warhead.nuke_maker,
        },
        special_target,
    );

    // Native ownership: only the final else at `0x00469a3f` runs
    // `BulletClass::SpawnShrapnel @ 0x0046a310` and
    // `Apply_area_damage @ 0x00489280`, so an arm that claims the impact
    // shadows both — including an arm whose effect body VERA has not ported,
    // because native's own bail-outs (e.g. `0x00469235`) shadow them too.
    // What an arm never shadows is the shared tail below `LAB_00469AA4`: every
    // arm reaches the explosion anim and its smudge. The RESIDUAL list for the
    // unported effect bodies lives on `SpecialDetonationAction`.
    match special_action {
        SpecialDetonationAction::OrdinaryDamage => {
            emit_projectile_shrapnel(
                detonation,
                &mut world.substrate.entities,
                &mut world.substrate.occupancy,
                rules,
                &mut world.interner,
                world.resolved_terrain.as_ref(),
                &world.house_alliances,
                &mut world.scenario_rng,
                out,
            );

            let routed_wall = wall_overlay_flags_at(
                world.overlay_grid.as_ref(),
                overlay_registry,
                impact_rx,
                impact_ry,
            )
            .is_some_and(|flags| warhead_damages_wall(warhead, flags));
            let aoe = {
                let collected = collect_area(
                    world,
                    rules,
                    overlay_registry,
                    (impact_rx, impact_ry),
                    detonation.payload.base_damage,
                    warhead,
                    (
                        detonation.source_id,
                        Some(detonation.payload.owner),
                        detonation.payload.warhead,
                    ),
                    air_impact,
                    impact_z,
                );
                append_fixture_tiberium(world, &mut out.effects.tiberium_reduction_requests);
                collected
            };
            #[cfg(test)]
            out.effects.wall_mutations.extend(aoe.wall_mutations);

            #[cfg(test)]
            out.effects
                .cell_target_detaches
                .extend(aoe.cell_target_detaches);
            out.damage_events.extend(aoe.receivers);

            if !scenario_no_damage && detonation.payload.base_damage > 0 {
                let damage = detonation.payload.base_damage.min(i32::from(u16::MAX)) as u16;
                if !routed_wall && warhead.wall {
                    out.effects.bridge_damage_events.push(BridgeDamageEvent {
                        rx: impact_rx,
                        ry: impact_ry,
                        damage,
                        warhead_ref: detonation.payload.warhead,
                        is_ion_cannon: detonation.payload.warhead
                            == handles
                                .expect("Simulation::resolve_type_handles must run before combat")
                                .ion_cannon,
                        impact_z,
                    });
                }
            }
        }
        SpecialDetonationAction::Parasite => {
            // 0x004693DD..0x0046941E: no bullet Owner (+0xB0) -> nothing;
            // otherwise Owner->ParasiteImUsing->AttachTo(Target if FootClass).
            // The impact point is not consulted.
            if world.substrate.entities.contains(detonation.source_id) {
                let victim = match detonation.target {
                    ProjectileTarget::Entity(id)
                        if world.substrate.entities.get(id).is_some_and(|target| {
                            matches!(
                                target.category,
                                EntityCategory::Unit
                                    | EntityCategory::Infantry
                                    | EntityCategory::Aircraft
                            )
                        }) =>
                    {
                        Some(id)
                    }
                    _ => None,
                };
                world.parasite_attach(detonation.source_id, victim, rules);
            }
        }
        claimed => {
            log::debug!(
                "Projectile {} claimed by unimplemented special detonation {:?}; \
                 shrapnel and area damage suppressed, shared tail still runs",
                detonation.projectile_id,
                claimed
            );
        }
    }

    // `LAB_00469AA4` — reached by the ordinary arm and by every special arm.
    emit_warhead_detonation_effects(
        warhead,
        detonation.payload.base_damage,
        impact_rx,
        impact_ry,
        impact_sub_x,
        impact_sub_y,
        impact_z_byte(impact_z),
        world_z_leptons,
        &mut world.interner,
        &mut out.effects.explosion_effects,
        &mut out.effects.smudge_spawn_requests,
    );
}

pub(super) fn emit_projectile_detonations(
    world: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    detonations: &[ProjectileDetonation],
    out: &mut CombatEmit,
) {
    for detonation in detonations {
        let projectile_type = rules
            .weapon(world.interner.resolve(detonation.payload.weapon))
            .and_then(|weapon| weapon.projectile.as_deref())
            .and_then(|projectile| rules.projectile(projectile));
        let Some(projectile_type) = projectile_type else {
            emit_one_projectile_detonation(world, rules, overlay_registry, detonation, out);
            continue;
        };

        let airburst = projectile_type.airburst;
        let cluster = projectile_type.cluster;
        if airburst {
            emit_one_projectile_detonation(world, rules, overlay_registry, detonation, out);
            continue;
        }

        let mut coordinate = detonation.impact;
        for _ in 0..cluster.max(0) {
            let mut clustered = *detonation;
            clustered.impact = coordinate;
            emit_one_projectile_detonation(world, rules, overlay_registry, &clustered, out);
            coordinate = projectile_next_cluster_coord(coordinate, &mut world.scenario_rng);
        }
    }
}

pub(crate) fn commit_projectile_detonations_inline(
    world: &mut Simulation,
    run: &mut ReceiverRun,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    projectile_detonations: &[ProjectileDetonation],
    emit: &mut CombatEmit,
    under_attack_events: &mut Vec<UnderAttackEvent>,
) {
    for detonation in projectile_detonations {
        let damage_start = emit.damage_events.len();
        let explosion_start = emit.effects.explosion_effects.len();
        let smudge_start = emit.effects.smudge_spawn_requests.len();
        emit_projectile_detonations(
            world,
            rules,
            overlay_registry,
            std::slice::from_ref(detonation),
            emit,
        );
        let outer_explosion_effects = emit.effects.explosion_effects.split_off(explosion_start);
        let outer_anim_requests = emit.effects.smudge_spawn_requests.split_off(smudge_start);
        let (inline_death, mut pings) = commit_area(
            world,
            run,
            &emit.damage_events[damage_start..],
            rules,
            overlay_registry,
        );
        emit.effects.append(inline_death);
        emit.effects
            .explosion_effects
            .extend(outer_explosion_effects);
        commit_smudges(
            world,
            rules,
            overlay_registry,
            outer_anim_requests,
            &mut emit.effects.smudge_spawn_requests,
        );
        under_attack_events.append(&mut pings);
    }
}

pub(crate) fn commit_projectiles(
    world: &mut Simulation,
    run: &mut ReceiverRun,
    detonations: &[ProjectileDetonation],
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
) -> LogicProjectileCommit {
    let mut emit = CombatEmit::default();
    let mut under_attack_events = Vec::new();
    commit_projectile_detonations_inline(
        world,
        run,
        rules,
        overlay_registry,
        detonations,
        &mut emit,
        &mut under_attack_events,
    );

    debug_assert!(emit.remove_attack.is_empty());
    debug_assert!(emit.retarget_events.is_empty());
    debug_assert!(emit.fire_events.is_empty());
    debug_assert!(emit.reveal_events.is_empty());
    debug_assert!(emit.burst_updates.is_empty());
    debug_assert!(emit.ammo_deduct.is_empty());
    debug_assert!(emit.garrison_advance.is_empty());
    debug_assert!(emit.pending_infantry_updates.is_empty());
    debug_assert!(emit.animation_switches.is_empty());
    debug_assert!(emit.current_weapon_updates.is_empty());
    debug_assert!(emit.unit_facing.is_empty());
    debug_assert!(emit.spawn_target_updates.is_empty());

    LogicProjectileCommit {
        projectile_spawns: emit.projectile_spawns,
        effects: emit.effects,
        under_attack_events,
    }
}

fn emit_missile_detonations(
    world: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    detonations: &[crate::sim::spawn_manager::MissileDetonation],
    out: &mut CombatEmit,
) {
    for det in detonations {
        let warhead_name = world.interner.resolve(det.warhead).to_string();
        let Some(warhead) = rules.warhead(&warhead_name) else {
            continue;
        };
        let wh_iid = world.interner.intern(&warhead.id);
        let impact_z =
            combat_aoe::bridge_adjusted_impact_z(world.resolved_terrain.as_ref(), det.rx, det.ry);
        let air_impact = combat_aoe::air_impact_from_layer_z(
            world.resolved_terrain.as_ref(),
            det.rx,
            det.ry,
            crate::util::lepton::CELL_CENTER_LEPTON,
            crate::util::lepton::CELL_CENTER_LEPTON,
            impact_z,
        );
        let world_z_leptons = air_impact
            .map(|impact| impact.z_leptons)
            .unwrap_or_else(|| impact_z.wrapping_mul(LEPTONS_PER_LEVEL as i32));
        let mut outer_explosions = Vec::new();
        let mut outer_smudges = Vec::new();
        emit_warhead_detonation_effects(
            warhead,
            det.damage,
            det.rx,
            det.ry,
            crate::util::lepton::CELL_CENTER_LEPTON,
            crate::util::lepton::CELL_CENTER_LEPTON,
            impact_z_byte(impact_z),
            world_z_leptons,
            &mut world.interner,
            &mut outer_explosions,
            &mut outer_smudges,
        );
        out.effects.explosion_effects.extend(outer_explosions);
        commit_smudges(
            world,
            rules,
            overlay_registry,
            outer_smudges,
            &mut out.effects.smudge_spawn_requests,
        );
        let aoe = {
            let collected = collect_area(
                world,
                rules,
                overlay_registry,
                (det.rx, det.ry),
                det.damage,
                warhead,
                (det.firer_id, Some(det.owner), wh_iid),
                air_impact,
                impact_z,
            );
            append_fixture_tiberium(world, &mut out.effects.tiberium_reduction_requests);
            collected
        };
        #[cfg(test)]
        out.effects.wall_mutations.extend(aoe.wall_mutations);

        #[cfg(test)]
        out.effects
            .cell_target_detaches
            .extend(aoe.cell_target_detaches);
        out.damage_events.extend(aoe.receivers);
    }
}

pub(super) fn resolve_attacker_fire(
    world: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    snap: &AttackerSnapshot,
    fog: Option<&FogState>,
    require_playfield_membership: bool,
    binary_frame: u32,
    _tick_ms: u32,
    has_active_wave: bool,
    out: &mut CombatEmit,
) {
    if let Some(shot) = admit_attacker_fire(
        world,
        rules,
        overlay_registry,
        snap,
        fog,
        require_playfield_membership,
        binary_frame,
        _tick_ms,
        has_active_wave,
        out,
    ) {
        emit_admitted_fire(world, rules, overlay_registry, shot, binary_frame, out);
    }
}

fn admit_attacker_fire<'r>(
    world: &mut Simulation,
    rules: &'r RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    snap: &AttackerSnapshot,
    fog: Option<&FogState>,
    require_playfield_membership: bool,
    binary_frame: u32,
    _tick_ms: u32,
    has_active_wave: bool,
    out: &mut CombatEmit,
) -> Option<AdmittedFire<'r>> {
    let sound_enabled = sound_enabled(world);
    let delayed_building_slot = snap
        .pending_building_fire
        .map(|pending| pending.weapon_slot);
    if delayed_building_slot.is_some() {
        // ProcessDelayedFire clears mode/timer regardless of whether its live
        // target and saved weapon still pass GetFireError.
        if let Some(entity) = world.substrate.entities.get_mut(snap.stable_id) {
            entity.pending_building_fire = None;
        }
    }
    // Pre-compute garrison scan range for retargeting (includes +1 buffer).
    let garrison_retarget_range: Option<SimFixed> = snap.garrison.as_ref().map(|gs| {
        let cells = gs.half_foundation as i32 + 1 + rules.garrison_rules.occupy_weapon_range;
        SimFixed::from_num(cells.max(1))
    });
    let obj = match rules.object(world.interner.resolve(snap.type_id)) {
        Some(o) => o,
        None => {
            if delayed_building_slot.is_none() {
                out.remove_attack.push(snap.stable_id);
            }
            return None;
        }
    };

    // Stock Sonic weapons occupy index 0, and native FireAt tests that
    // WeaponType before resolving the target. Preserve that whole-call gate
    // so a stale/missing target cannot retarget or clear the order while the
    // owner's exact Wave link remains live. The selected-weapon check below
    // retains the same protection for non-stock overrides/secondary layouts.
    if has_active_wave
        && combat_weapon::primary_for_tier(obj, snap.veterancy)
            .and_then(|weapon_id| rules.weapon(weapon_id))
            .is_some_and(|weapon| weapon.is_sonic)
    {
        return None;
    }

    // Check if target is alive and get its data.
    // For structures, target_coords returns the foundation center instead
    // of the NW corner.
    // For Cell targets (force-fire on terrain), synthesize a target_data
    // tuple: cell-center coords, "always alive" (cells don't despawn), no
    // category/type/owner — the unit fires its primary weapon and splash
    // delivers the damage.
    let target_data: Option<(
        u16,
        u16,
        SimFixed,
        SimFixed,
        i32,
        EntityCategory,
        InternedId,
        InternedId,
        bool,
    )> = match snap.target {
        TargetKind::Entity(target_id) => world.substrate.entities.get(target_id).map(|t| {
            let (trx, try_, tsx, tsy) = target_coords(t, Some(rules), &world.interner);
            (
                trx,
                try_,
                tsx,
                tsy,
                t.health.current,
                combat_target_category(t, rules, &world.interner),
                t.type_ref(),
                t.owner(),
                t.category == EntityCategory::Infantry && infantry::is_prone_for_damage(t),
            )
        }),
        TargetKind::Cell(rx, ry) => {
            // Synthetic target_data for force-fire-on-cell.
            // - hp = 1 so the "target dead" retarget branch never fires for cells.
            // - category = Structure so weapon-vs-armor selection picks an
            //   anti-structure weapon when one exists; otherwise falls
            //   through to primary (matches "fire your default weapon at
            //   the ground" intent).
            // - type_ref/owner = attacker's own — friendly-fire check
            //   sees self-vs-self and is short-circuited downstream.
            let (trx, try_, tsx, tsy) = cell_center_coords(rx, ry);
            Some((
                trx,
                try_,
                tsx,
                tsy,
                1i32,
                EntityCategory::Structure,
                snap.type_id,
                snap.owner,
                false,
            ))
        }
    };

    let (
        target_rx,
        target_ry,
        target_sub_x,
        target_sub_y,
        _target_hp,
        target_cat,
        target_type_ref,
        target_owner,
        _target_prone_infantry,
    ) = match target_data {
        Some((rx, ry, sx, sy, hp, cat, tr, own, prone)) if hp > 0 => {
            (rx, ry, sx, sy, hp, cat, tr, own, prone)
        }
        _ => {
            if delayed_building_slot.is_some() {
                return None;
            }
            if let Some(new_target) = acquire_best_target(
                &mut world.substrate.entities,
                &world.substrate.occupancy,
                rules,
                &mut world.interner,
                snap,
                obj,
                fog,
                garrison_retarget_range,
                world.resolved_terrain.as_ref(),
                require_playfield_membership,
                // No zone grid on the retarget path, and none is needed: this
                // snapshot's mask comes from `build_attacker_snapshot`, i.e.
                // `scan_mission_for`, which only ever returns `1` or `2`. The
                // scanner-zone slot native fills at `0x006F8EC4` is read only by
                // the mask-0 flat walk, which this callsite cannot select.
                None,
                line_of_fire::LineOfFireInputs {
                    overlay_grid: world.overlay_grid.as_ref(),
                    overlay_registry,
                    alliances: fog.map(|fog_state| &fog_state.alliances),
                },
            ) {
                out.retarget_events.push((snap.stable_id, new_target));
            } else {
                out.remove_attack.push(snap.stable_id);
            }
            return None;
        }
    };

    // `TechnoClass::GetFireError @ 0x006FC133`: a target that is already this
    // object's `DrainTarget` (`+0x1CC`) is refused — a linked Floating Disc
    // fires nothing further at the building it drains (GSI-09.01).
    if let TargetKind::Entity(target_id) = snap.target
        && world
            .substrate
            .entities
            .get(snap.stable_id)
            .is_some_and(|attacker| attacker.drain_target == Some(target_id))
    {
        return None;
    }

    let target_armor: String = rules
        .object(world.interner.resolve(target_type_ref))
        .map(|o| o.armor.clone())
        .unwrap_or_else(|| "none".to_string());

    // Target facts for `What_Weapon_Should_I_Use` and the GetFireError
    // targeting subset. Cell targets read the terrain cell; entity targets
    // read the target's occupied cell, altitude, bridge and cloak state.
    let target_facts = match snap.target {
        TargetKind::Entity(target_id) => {
            let Some((target_entity, target_obj)) =
                world.substrate.entities.get(target_id).and_then(|t| {
                    rules
                        .object(world.interner.resolve(t.type_ref()))
                        .map(|target_obj| (t, target_obj))
                })
            else {
                if delayed_building_slot.is_none() {
                    out.remove_attack.push(snap.stable_id);
                }
                return None;
            };
            let is_ally = combat_weapon::is_ally_by_object(
                fog.map(|fog_state| &fog_state.alliances),
                &mut world.interner,
                snap.owner,
                target_entity.owner(),
            );
            combat_weapon::techno_target_facts(
                target_entity,
                target_obj,
                world.resolved_terrain.as_ref(),
                is_ally,
            )
        }
        TargetKind::Cell(rx, ry) => {
            combat_weapon::cell_target_facts(rx, ry, world.resolved_terrain.as_ref())
        }
    };

    // Weapon selection: garrison uses occupant's OccupyWeapon, everything
    // else runs the native selection ladder.
    let (selected, is_garrison) = if let Some(saved_slot) = delayed_building_slot {
        match select_weapon_slot(rules, obj, snap.veterancy, saved_slot, &target_facts) {
            Some(selected) => (selected, false),
            None => return None,
        }
    } else if let Some(ref gs) = snap.garrison {
        match combat_weapon::select_garrison_weapon(
            rules,
            world.interner.resolve(gs.occupant_type_id),
            gs.occupant_veterancy,
            target_cat,
            &target_armor,
        ) {
            Some(s) => (s, true),
            None => {
                out.remove_attack.push(snap.stable_id);
                return None;
            }
        }
    } else {
        let attacker_facts = world
            .substrate
            .entities
            .get(snap.stable_id)
            .map(|entity| combat_weapon::attacker_facts(entity, obj))
            .unwrap_or_else(|| combat_weapon::attacker_facts_from_snapshot(snap, obj));
        match combat_weapon::select_weapon_for_target(rules, obj, &attacker_facts, &target_facts) {
            Some(s) => (s, false),
            None => {
                out.remove_attack.push(snap.stable_id);
                return None;
            }
        }
    };
    let weapon = selected.weapon;
    // gamemd-derived: `TechnoClass::FireAt @ 0x006FDE4A..0x006FDE5C`.
    // Only an IsSonic selected weapon observes the firer's active-Wave link,
    // and that return precedes every shot/cooldown/report/current-weapon side
    // effect owned below. Type-3's local effect check is not this gate.
    if weapon.is_sonic && has_active_wave {
        return None;
    }
    // GetFireError 0x006FCAC5..0x006FCB21: a Parasite shot at a Foot another
    // jump has launch-locked (Foot+698) or at an Iron-Curtained one is
    // FIRE_ILLEGAL (5), like the CanInfect gate in the targeting subset, and
    // VERA drops the target for all three alike. RESIDUAL: native keeps an
    // ILLEGAL target until TechnoClass::AI's 16-frame check
    // (0x006FA472..0x006FA4CB) drops it, so VERA gives up up to 15 frames
    // sooner; no RNG or state beyond the target is involved.
    if selected.warhead.parasite
        && let TargetKind::Entity(target_id) = snap.target
        && world
            .substrate
            .entities
            .get(target_id)
            .is_some_and(|target| {
                binary_frame < target.parasite_launch_lock
                    || crate::sim::superweapon::invulnerability::is_invulnerable(
                        target.invulnerability.as_ref(),
                        binary_frame,
                    )
            })
    {
        if delayed_building_slot.is_none() {
            out.remove_attack.push(snap.stable_id);
        }
        return None;
    }
    // GetFireError, FootClass::IsParalyzed `0x004DE770`: a paralyzed firer
    // cannot launch a Spawner weapon (`0x006FC61F..0x006FC62B`, CANT) and an
    // Organic one cannot fire at all (`0x006FCCBD..0x006FCCDD`, ILLEGAL).
    if (weapon.spawner || obj.organic)
        && world
            .substrate
            .entities
            .get(snap.stable_id)
            .is_some_and(|firer| firer.is_paralyzed(binary_frame))
    {
        return None;
    }
    if delayed_building_slot.is_none() {
        out.current_weapon_updates.push((
            snap.stable_id,
            match selected.slot {
                WeaponSlot::Primary => 0,
                WeaponSlot::Secondary => 1,
            },
            world.interner.intern(selected.weapon_id),
        ));
    }

    // Friendly-fire and visibility-driven retarget logic only applies to
    // Entity targets. Cell targets are an explicit player force-fire — the
    // player intentionally chose this cell (allies, ground, anything), so
    // never auto-retarget away from a Cell.
    let is_cell_target = matches!(snap.target, TargetKind::Cell(_, _));
    if let Some(fog_state) = fog {
        let snap_owner_str = world.interner.resolve(snap.owner);
        let target_owner_str = world.interner.resolve(target_owner);
        if !is_cell_target && fog_state.is_friendly(snap_owner_str, target_owner_str) {
            if delayed_building_slot.is_some() {
                return None;
            }
            if let Some(new_target) = acquire_best_target(
                &mut world.substrate.entities,
                &world.substrate.occupancy,
                rules,
                &mut world.interner,
                snap,
                obj,
                fog,
                garrison_retarget_range,
                world.resolved_terrain.as_ref(),
                require_playfield_membership,
                // No zone grid on the retarget path, and none is needed: this
                // snapshot's mask comes from `build_attacker_snapshot`, i.e.
                // `scan_mission_for`, which only ever returns `1` or `2`. The
                // scanner-zone slot native fills at `0x006F8EC4` is read only by
                // the mask-0 flat walk, which this callsite cannot select.
                None,
                line_of_fire::LineOfFireInputs {
                    overlay_grid: world.overlay_grid.as_ref(),
                    overlay_registry,
                    alliances: fog.map(|fog_state| &fog_state.alliances),
                },
            ) {
                out.retarget_events.push((snap.stable_id, new_target));
            } else {
                out.remove_attack.push(snap.stable_id);
            }
            return None;
        }
        if !is_cell_target && !fog_state.is_cell_visible(snap.owner, target_rx, target_ry) {
            if delayed_building_slot.is_some() {
                return None;
            }
            if let Some(new_target) = acquire_best_target(
                &mut world.substrate.entities,
                &world.substrate.occupancy,
                rules,
                &mut world.interner,
                snap,
                obj,
                fog,
                garrison_retarget_range,
                world.resolved_terrain.as_ref(),
                require_playfield_membership,
                // No zone grid on the retarget path, and none is needed: this
                // snapshot's mask comes from `build_attacker_snapshot`, i.e.
                // `scan_mission_for`, which only ever returns `1` or `2`. The
                // scanner-zone slot native fills at `0x006F8EC4` is read only by
                // the mask-0 flat walk, which this callsite cannot select.
                None,
                line_of_fire::LineOfFireInputs {
                    overlay_grid: world.overlay_grid.as_ref(),
                    overlay_registry,
                    alliances: fog.map(|fog_state| &fog_state.alliances),
                },
            ) {
                out.retarget_events.push((snap.stable_id, new_target));
            } else {
                out.remove_attack.push(snap.stable_id);
            }
            return None;
        }
    }

    // `TechnoClass::GetFireError @ 0x006FC24D..0x006FC29D` — the target-side
    // cloak exit, ahead of ammo and the `DecloakToFire` self-check:
    //
    //   vs = target->GetVisualState(1, this->pOwner);          // vt+0x68
    //   if (vs == 5 && !SensorCountForHouse(targetCell, myHouse->ArrayIndex)) {
    //       if (GetWeaponRange(this, -1) > 0)            return 6;   // 0x006FCD29
    //       if (!IsAlliedWith(target->pOwner, myOwner))  return 6;
    //   }
    //
    // `GetVisualState(1, house) @ 0x00703860` returns 5 for a `CloakState == 2`
    // object exactly when that house has no sensor on its cell, so for cloak
    // this is "fully cloaked and unsensed by me". Error 6 suppresses the shot
    // only — no retarget and no clear, like the bridge-mismatch arm above.
    //
    // Acquisition already refuses such a candidate; this is the case
    // acquisition cannot cover — a target held from before the dive (the
    // attacker's house was sensing the cell, so `PointerExpired` let it keep
    // the pointer) that later falls out of sensor coverage.
    //
    // The range-zero/allied fall-through is modelled: a range-0 weapon against
    // an ALLIED invisible target is allowed through, everything else is not.
    //
    // RESIDUAL (SUBSTITUTION) — the range term is the SELECTED weapon's range;
    // native's `GetWeaponRange(this, -1)` at `0x006FC27C` asks the object for
    // its range across weapons, so the two differ only when the selected weapon
    // has range 0 while another slot does not.
    // - Trigger: a zero-range weapon aimed at an allied, fully cloaked,
    //   unsensed target.
    // - Player effect: VERA lets the shot through where native returns 6.
    // - Frequency: none observed in stock — no stock weapon pairs range 0 with
    //   a second armed slot on a unit that can hold an allied cloaked target.
    // - Downstream risk: low; it is one range lookup.
    if let TargetKind::Entity(target_sid) = snap.target
        && world
            .substrate
            .entities
            .get(target_sid)
            .and_then(|target| target.cloak.as_ref())
            .is_some_and(|cloak| cloak.is_fully_cloaked())
        && fog.is_some_and(|fog_state| {
            !fog_state.has_sensor_for_house(snap.owner, target_rx, target_ry)
        })
    {
        let attacker_owner_str = world.interner.resolve(snap.owner);
        let target_owner_str = world.interner.resolve(target_owner);
        let allied = fog
            .is_some_and(|fog_state| fog_state.is_friendly(attacker_owner_str, target_owner_str));
        if weapon.range > SimFixed::ZERO || !allied {
            return None;
        }
    }

    let infantry_fire_sync =
        snap.category == EntityCategory::Infantry && !is_garrison && snap.animation_frame.is_some();
    let mut pending_at_fire_frame = false;
    if infantry_fire_sync {
        if let Some(pending) = snap.pending_infantry_fire {
            if snap.has_movement || snap.animation_sequence != Some(pending.sequence) {
                out.pending_infantry_updates.push((snap.stable_id, None));
                out.animation_switches.push((
                    snap.stable_id,
                    infantry_idle_sequence(snap.is_prone, snap.is_fully_deployed),
                ));
                return None;
            }
            if snap.animation_frame != Some(pending.fire_frame) {
                return None;
            }
            pending_at_fire_frame = true;
        }
    }

    // Range check (lepton-precise, sub-cell aware).
    // Garrison range: (half_foundation + OccupyWeaponRange) cells (no +1 buffer for fire).
    let effective_range = if let Some(ref gs) = snap.garrison {
        let cells = gs.half_foundation as i32 + rules.garrison_rules.occupy_weapon_range;
        SimFixed::from_num(cells.max(1))
    } else {
        weapon.range
    };
    // Range failure: range alone does not clear or retarget — the pursuit
    // pre-combat stage walks the unit into range. Combat tick just skips
    // this tick's fire attempt and lets the unit close the gap.
    let in_range_for_fire = if !is_garrison && effective_range == weapon.range {
        // Standard fire: 3D check via compute_in_range when terrain available.
        match (
            world.resolved_terrain.as_ref(),
            world.substrate.entities.get(snap.stable_id),
        ) {
            (Some(t), Some(attacker_entity)) => {
                let Some(src) = in_range::fire_source_coords(
                    attacker_entity,
                    &snap.target,
                    weapon,
                    &world.substrate.entities,
                    t,
                ) else {
                    return None;
                };
                in_range::compute_in_range(
                    attacker_entity,
                    src,
                    &snap.target,
                    weapon,
                    rules,
                    &world.interner,
                    &world.substrate.entities,
                    t,
                    &line_of_fire::LineOfFireInputs {
                        overlay_grid: world.overlay_grid.as_ref(),
                        overlay_registry,
                        alliances: fog.map(|fog_state| &fog_state.alliances),
                    },
                )
            }
            _ => {
                let dist_sq = lepton_distance_sq_raw(
                    snap.pos_rx,
                    snap.pos_ry,
                    snap.sub_x,
                    snap.sub_y,
                    target_rx,
                    target_ry,
                    target_sub_x,
                    target_sub_y,
                );
                is_within_range_leptons(dist_sq, effective_range)
            }
        }
    } else {
        // Garrison override path — preserve 2D until a later stage threads
        // override-aware 3D.
        //
        // RESIDUAL (line of fire) — because this arm never calls
        // `compute_in_range`, it never runs the wall/cliff walk that
        // `TechnoClass::InRange` 0x006F7220 ends in at 0x006F7642.
        // - Trigger: any garrisoned occupant firing across a wall or a
        //   ≥4-Level step; stock `[E1]`'s `M60` is `Projectile=InvisibleLow`
        //   (`SubjectToWalls=yes`, `SubjectToCliffs=yes`) and its `SA` warhead
        //   has no `Wall=`, so gamemd refuses that shot.
        // - Player effect: a garrisoned GI shoots straight through a GAWALL the
        //   identical GI standing in the open is refused.
        // - Frequency: routine on urban maps — garrisoning a building next to
        //   a wall line is an ordinary opening, not an edge case.
        // - Downstream risk: none to deterministic state. The cure is the
        //   override-aware range VALUE chain M8 records (Garrison / Bunker /
        //   OpenTopped), which lets this branch use `compute_in_range` instead
        //   of a second walk bolted onto the 2-D twin.
        let dist_sq = lepton_distance_sq_raw(
            snap.pos_rx,
            snap.pos_ry,
            snap.sub_x,
            snap.sub_y,
            target_rx,
            target_ry,
            target_sub_x,
            target_sub_y,
        );
        is_within_range_leptons(dist_sq, effective_range)
    };
    if !in_range_for_fire {
        if pending_at_fire_frame {
            out.pending_infantry_updates.push((snap.stable_id, None));
            out.animation_switches.push((
                snap.stable_id,
                infantry_idle_sequence(snap.is_prone, snap.is_fully_deployed),
            ));
        }
        return None;
    }

    // `TechnoClass::GetFireError` 0x006FC0B0 refuses the shot with error 5 when
    // attacker and target disagree on OnBridge, both stand in bridge cells, and
    // the attacker is not high-flying — a unit on the deck and a unit sheltering
    // directly beneath it cannot shoot each other. `InRange` 0x006F7220 already
    // blocks the under-to-over half through
    // `attacker_under_bridge_targeting_above`; this is the over-to-under half
    // and the cases where the height test does not fire.
    //
    // Evaluated here rather than inside `compute_in_range` because the native
    // InRange has no such clause. Like the native it only suppresses the shot:
    // no retarget, no clear, no effect on the pursuit stage.
    if let (Some(t), Some(attacker_entity)) = (
        world.resolved_terrain.as_ref(),
        world.substrate.entities.get(snap.stable_id),
    ) && in_range::fire_error_on_bridge_mismatch(
        attacker_entity,
        &snap.target,
        &world.substrate.entities,
        t,
    ) {
        if pending_at_fire_frame {
            out.pending_infantry_updates.push((snap.stable_id, None));
            out.animation_switches.push((
                snap.stable_id,
                infantry_idle_sequence(snap.is_prone, snap.is_fully_deployed),
            ));
        }
        return None;
    }

    // Burst / cooldown state machine.
    if snap.cooldown_ticks > 0 || snap.burst_delay_ticks > 0 {
        if pending_at_fire_frame {
            out.pending_infantry_updates.push((snap.stable_id, None));
            out.animation_switches.push((
                snap.stable_id,
                infantry_idle_sequence(snap.is_prone, snap.is_fully_deployed),
            ));
        }
        return None;
    }

    // InfantryClass::GetFireError 0051C9B8..0051C9C9: after common legality,
    // Foot+578 > binary64 0.1 returns error 7, before starting or emitting fire.
    // Read the existing Foot owner: an attack order retains its paid Walk step
    // even after clearing NavCom. A movement-target test is not this predicate.
    // 0.1 lies between SimFixed raw 6553 and 6554; integer division selects the
    // largest admissible representable fraction. Original boundary witnesses:
    // tools/spatial_oracle/infantry_fire_speed.json.
    if snap.category == EntityCategory::Infantry
        && world
            .substrate
            .entities
            .get(snap.stable_id)
            .is_some_and(|entity| {
                entity.foot_speed.applied_fraction > SimFixed::ONE / SimFixed::from_num(10)
            })
    {
        if pending_at_fire_frame {
            out.pending_infantry_updates.push((snap.stable_id, None));
            out.animation_switches.push((
                snap.stable_id,
                infantry_idle_sequence(snap.is_prone, snap.is_fully_deployed),
            ));
        }
        return None;
    }

    // TechnoClass::GetFireError @ 0x006FC0B0 returns 9 after the ordinary
    // busy/rearm/ammo gates when DecloakToFire= is set and the current cloak
    // state requires surfacing. UnitClass::Fire_At_Target @ 0x00736DF0 then
    // rechecks CanFireAt, calls StartUncloaking(0), and emits no shot, damage,
    // rearm, report, or fire event on this visit. The retained attack target is
    // the Rust retry latch; native's separate firing-sequence byte has no Rust
    // producer or reader to clear.
    if snap.category == EntityCategory::Unit {
        let cloak_state = world
            .substrate
            .entities
            .get(snap.stable_id)
            .and_then(|entity| entity.cloak.as_ref())
            .map_or(0, |cloak| cloak.state);
        if crate::sim::cloak_disguise::fire_requires_uncloaking(
            weapon.decloak_to_fire,
            cloak_state,
            1, // UnitClass::WhatAmI, not locomotor kind.
        ) {
            let start = world
                .substrate
                .entities
                .get_mut(snap.stable_id)
                .and_then(|entity| entity.cloak.as_mut())
                .map(|cloak| {
                    cloak.start_uncloaking_to_fire(binary_frame as i32, obj.cloaking_speed)
                });
            if start.is_some_and(|result| result.play_sound)
                && let Some(sound_name) = rules.general.cloak_sound.as_deref()
                && let Some(sink) = sound_enabled.then_some(&mut world.sound_events)
                && let Some(entity) = world.substrate.entities.get(snap.stable_id)
            {
                sink.push(SimSoundEvent::cloak_sound(
                    sound_name.to_owned(),
                    &entity.position,
                ));
            }
            return None;
        }
    }

    // The projectile's `ROT=` (`BulletTypeClass+0x2DC`) is read twice in this
    // region of `UnitClass::GetFireError`: once by the rotation refusal below
    // (`0x00741243`) and once to widen the angle tolerance (`0x007412B6`).
    let projectile_homes = weapon
        .projectile
        .as_deref()
        .and_then(|id| rules.projectile(id))
        .is_some_and(|projectile| projectile.rot != 0);

    // ---- FIRE_ROTATING (4) ----------------------------------------------
    //
    // gamemd-derived: `UnitClass::GetFireError @ 0x00740FD0` step 14,
    // `0x00741229`..`0x00741259` (disassembled this session):
    //
    //   MOV AL,[ESI+0x68D] / TEST AL,AL / JNZ 0x0074125C  ; firing sequence set
    //   MOV AL,[ESI+0x6AF] / TEST AL,AL / JZ  0x0074125C  ; latch clear
    //   MOV EDX,[EBX+0xA0] / MOV EAX,[EDX+0x2DC] / TEST / JNZ 0x0074125C ; homing
    //   MOV EAX,0x4 / RET 0xC                             ; FIRE_ROTATING
    //
    // A vehicle whose turret is mid-arc is refused HERE, before the OmniFire
    // skip at `0x0074125C` and before the step-17 angle test — so it may not
    // fire until the arc it committed to has actually finished, not merely
    // once the animated turret wanders inside the tolerance. Without this the
    // latch would delay the aim but not the shot, and every re-aim wider than
    // one tolerance step would open fire two to three frames early.
    //
    // Unit-and-turret only. This is the only FIRE gate that reads `+0x6AF`
    // (`search_instructions` over `+ 0x6af]` — the other readers on this layout
    // are Receive_Radio, Mission_Unload, Scatter, PassiveAcquireGate, the
    // not-moving-and-not-rotating sound test in `FUN_00740E80 @ 0x00740EA7`,
    // and the checksum walk; `BuildingClass`/`InfantryClass::GetFireError` have
    // no such term). The only writer that ever SETS it is
    // `UnitClass::Facing_Update @ 0x00736B16`, exclusively inside the
    // `Turret=yes` arm; the other two writes are the `FootClass::Constructor`
    // zero-fill at `0x004D3420` and the unconditional clear at `0x00736AD5`.
    //
    // The firing-sequence byte needs no VERA analogue: of the 21 references to
    // `+0x68D`, the only store of 1 is `InfantryClass::Fire_At_Target @
    // 0x00520912`. On a UnitClass receiver the byte is written 0 by
    // `FootClass::Constructor @ 0x004D33C6` and by three sites in
    // `UnitClass::Fire_At_Target` (`0x00736EF9`, `0x0073702D`, `0x0073704B`)
    // and never set, so for a vehicle the first test always falls through.
    if snap.category == EntityCategory::Unit && snap.turret_rotation_latch && !projectile_homes {
        // Native code 4, which is also one of the {0, 2, 3, 4} codes that drive
        // gattling spin-up. `FireDecision` has no variant for it and no
        // producer at all — that gap is the residual recorded on the enum in
        // `fire_decision.rs`, not something this return introduces.
        return None;
    }

    // ---- Facing arm of the fire gate ------------------------------------
    //
    // gamemd-derived: `UnitClass::GetFireError @ 0x00740FD0` step 17
    // (`0x00741288`..`0x007412EF`, read this session by `disassemble_bytes`;
    // Ghidra has no function boundary there) and its structure twin
    // `BuildingClass::GetFireError @ 0x00447F10` (`0x00447FE1`..`0x00448045`).
    //
    // Native picks the facing to test by `Turret=` (`TechnoType+0xCA1`), NOT by
    // whether a turret interpolator exists: `Turret=yes` compares the turret
    // `+0x3A0`, `Turret=no` compares the HULL `+0x388`. Both use the same
    // tolerance, both take the 16-bit signed difference against
    // `DirectionToTarget`, and the `JGE` at `0x007412EF` skips the error return
    // — so `|delta| == tol` passes and `tol + 1` fails. There is no
    // not-rotating term.
    //
    // Infantry are NOT angle-gated: `InfantryClass::Fire_At_Target @
    // 0x005206B0` snaps `+0x388` with `UpdateFacing` at `0x00520925` the moment
    // firing becomes possible and applies no test. A turretless STRUCTURE gets
    // no gate either (`0x00447FE3` requires `HasTurret`, vtable `+0x3FC`).
    //
    // Aircraft41A9E0 compares SecondaryFacing even without Turret. Fighter
    // bypasses only that class-specific arc; OmniFire/homing do not widen it.
    let facing_gate_applies = match snap.category {
        EntityCategory::Unit => true,
        // `0x00447FE3` requires `HasTurret` (vtable `+0x3FC`) before the
        // building facing test — a turretless structure is never angle-gated.
        EntityCategory::Structure => snap.barrel_facing.is_some(),
        EntityCategory::Aircraft => !obj.fighter,
        // Infantry are gated by their FIRE sequence, never by angle.
        EntityCategory::Infantry => false,
    };
    if facing_gate_applies && (snap.category == EntityCategory::Aircraft || !weapon.omni_fire) {
        let desired: u16 = crate::sim::movement::turret::facing_toward_lepton(
            snap.pos_rx,
            snap.pos_ry,
            snap.sub_x,
            snap.sub_y,
            target_rx,
            target_ry,
            target_sub_x,
            target_sub_y,
        );
        // `Turret=yes` reads the turret, `Turret=no` reads the hull. VERA gives
        // a turret interpolator to exactly the `Turret=yes` types, so its
        // presence is the same predicate.
        let current: u16 = match snap.barrel_facing {
            Some(ref barrel) => barrel.current(binary_frame),
            None => match snap.hull_facing {
                Some(ref hull) => hull.current(binary_frame),
                None => crate::sim::movement::turret::body_facing_to_turret(snap.facing),
            },
        };
        // A BUILDING narrows to an exact match when its turret art is a voxel
        // (`BuildingTypeClass+0x16C5`); everything else widens to 1/16 of a turn
        // when the PROJECTILE homes (`BulletTypeClass+0x2DC != 0`).
        let is_voxel_turret_building =
            snap.category == EntityCategory::Structure && obj.turret_anim_is_voxel;
        let tolerance: i32 = if snap.category == EntityCategory::Aircraft {
            0x800
        } else if is_voxel_turret_building {
            NATIVE_FIRE_FACING_TOLERANCE_VOXEL_TURRET
        } else if projectile_homes {
            NATIVE_FIRE_FACING_TOLERANCE_HOMING
        } else {
            NATIVE_FIRE_FACING_TOLERANCE
        };
        let delta = i32::from(current.wrapping_sub(desired) as i16);
        let mut aligned = delta.abs() <= tolerance;

        // `BuildingClass::Mission_Attack @ 0x0044ACF0` gives a voxel turret a
        // second chance in the SAME visit: when the miss is within one `ROT=`
        // step (`0x0044B068`..`0x0044B0A4`, or unconditionally when
        // `BuildingType+0x71C` is zero) it snaps `+0x388` with `UpdateFacing`
        // at `0x0044B0AC` and re-runs `GetFireError`. That is why a Grand
        // Cannon at `ROT=1` fires on the tick it comes within one step instead
        // of waiting a further frame.
        if !aligned && is_voxel_turret_building {
            // 44B068..44B0A4 uses abs(low-byte ROT << 8 interpreted as
            // signed16), WITHOUT FacingClass SetROT's upper clamp. Keep this
            // native difference: ROT128 allows a half-turn retry; ROT256 only
            // an exact match. Original decisions: building_fire_turn.json.
            let rot_step = i32::from(((obj.turret_rot as u8 as u16) << 8) as i16).abs();
            if obj.turret_rot == 0 || delta.abs() <= rot_step {
                if let Some(barrel) = world
                    .substrate
                    .entities
                    .get_mut(snap.stable_id)
                    .and_then(|entity| entity.barrel_facing.as_mut())
                {
                    barrel.snap(desired, binary_frame);
                }
                aligned = true;
            }
        }

        if !aligned {
            if pending_at_fire_frame {
                out.pending_infantry_updates.push((snap.stable_id, None));
                out.animation_switches.push((
                    snap.stable_id,
                    infantry_idle_sequence(snap.is_prone, snap.is_fully_deployed),
                ));
            }
            // `UnitClass::Fire_At_Target @ 0x00736DF0` case 2
            // (`0x00736FB6`..`0x0073701C`): a TURRETLESS vehicle that is
            // stationary with no destination turns its HULL toward the target at
            // its own `ROT=` — `FacingClass::Set(+0x388)` at `0x00737004`, with
            // no locomotor involvement — and copies the hull's raw destination
            // into the turret slot at `0x0073701C`. A moving one does not turn
            // at all. This is the emitter the gate above depends on: without it
            // an artillery piece would be refused every frame and never line up.
            if snap.category == EntityCategory::Unit
                && snap.barrel_facing.is_none()
                && !snap.has_movement
                && let Some(update) = out
                    .unit_facing
                    .iter_mut()
                    .find(|u| u.entity_id == snap.stable_id)
            {
                update.hull_destination = Some(desired);
            }
            // FireDecision::Facing — drives gattling spin-up via
            // drives_gattling_spinup() == true.
            return None;
        }
    }

    if delayed_building_slot.is_none()
        && snap.category == EntityCategory::Structure
        && !rules
            .general
            .prism_type
            .as_deref()
            .is_some_and(|prism_type| obj.id.eq_ignore_ascii_case(prism_type))
    {
        let delayed_fire_delay = rules
            .art_registry
            .resolve_metadata_entry(&obj.id, &obj.image)
            .filter(|art| art.is_anim_delayed_fire)
            .map(|art| art.delayed_fire_delay);
        if let Some(delay) = delayed_fire_delay {
            // gamemd-derived: the non-Prism generic arm in
            // BuildingClass::Mission_Attack @ 0x0044B630 saves the selected
            // weapon slot and signed delay without firing/rearming. This same
            // BuildingClass::Update visit then enters ProcessDelayedFire @
            // 0x004503F0, so account for its pre-decrement immediately.
            let pending = PendingBuildingFire {
                remaining_ticks: delay.saturating_sub(1).max(0),
                weapon_slot: selected.slot,
            };
            if pending.remaining_ticks != 0 {
                if let Some(entity) = world.substrate.entities.get_mut(snap.stable_id) {
                    entity.pending_building_fire = Some(pending);
                }
                // SpecialAnim presentation and its Report cue are app-layer
                // residuals; they do not authorize early weapon emission.
                return None;
            }
        }
    }

    // InfantryClass::Fire_At_Target 00520904..00520925: after admission and
    // starting the fire action, snap body +388 through DirectionToTarget.
    // Pending actions skip this writer, even if the target moves before FireUp.
    // Publish both the retained owner and the emission snapshot: FireUp=0 must
    // use the new heading for this very shot's FLH and presentation event.
    let mut firing_snapshot;
    let snap = if snap.category == EntityCategory::Infantry && !pending_at_fire_frame {
        let desired = crate::sim::movement::turret::facing_toward_lepton(
            snap.pos_rx,
            snap.pos_ry,
            snap.sub_x,
            snap.sub_y,
            target_rx,
            target_ry,
            target_sub_x,
            target_sub_y,
        );
        let Some(entity) = world.substrate.entities.get_mut(snap.stable_id) else {
            return None;
        };
        let body = entity.body_facing.get_or_insert_with(|| {
            // Infantry ctor517BBD..517BC5 seeds PrimaryFacing with127,
            // independently of Type ROT (Unit/Aircraft use the type value).
            crate::sim::movement::FacingClass::new(u16::from(entity.facing) << 8, 127)
        });
        body.snap(desired, binary_frame);
        entity.facing = (body.current(binary_frame) >> 8) as u8;
        firing_snapshot = snap.clone();
        firing_snapshot.facing = entity.facing;
        firing_snapshot.hull_facing = entity.body_facing;
        &firing_snapshot
    } else {
        snap
    };

    if infantry_fire_sync && !pending_at_fire_frame {
        let sequence =
            infantry_fire_sequence(obj, selected.slot, snap.is_prone, snap.is_fully_deployed);
        let fire_frame =
            infantry_fire_frame(obj, selected.slot, snap.is_prone, snap.is_fully_deployed);
        out.animation_switches.push((snap.stable_id, sequence));
        if fire_frame != 0 {
            out.pending_infantry_updates.push((
                snap.stable_id,
                Some(PendingInfantryFire {
                    sequence,
                    fire_frame,
                }),
            ));
            return None;
        }
    }
    if pending_at_fire_frame {
        out.pending_infantry_updates.push((snap.stable_id, None));
    }

    Some(AdmittedFire {
        snap: snap.clone(),
        obj,
        selected,
        target_coords: (target_rx, target_ry, target_sub_x, target_sub_y),
        target_type_ref,
        is_garrison,
    })
}

/// Call-local result of admission and fire-action work.
/// This is call-local data, never a saved permission to fire on a later frame.
/// Native Mission_Attack418403 checks legality once before its burst loop;
/// separating emission lets the aircraft caller reselect from live state
/// without repeating admission for every FireAt call.
struct AdmittedFire<'a> {
    snap: AttackerSnapshot,
    obj: &'a ObjectType,
    selected: combat_weapon::SelectedWeapon<'a>,
    target_coords: (u16, u16, SimFixed, SimFixed),
    target_type_ref: InternedId,
    is_garrison: bool,
}

/// Existing FireAt delivery and bookkeeping, shared by the world receiver.
/// The caller still owns legality, fire-action timing and inline damage commit.
fn emit_admitted_fire(
    world: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    shot: AdmittedFire<'_>,
    binary_frame: u32,
    out: &mut CombatEmit,
) {
    let AdmittedFire {
        snap,
        obj,
        selected,
        target_coords: (target_rx, target_ry, target_sub_x, target_sub_y),
        target_type_ref,
        is_garrison,
    } = shot;
    let snap = &snap;
    let weapon = selected.weapon;
    let handles = world.rule_handles;
    let scenario_no_damage = world.session.no_damage;
    if weapon.is_sonic && world.active_wave_links.contains_key(&snap.stable_id) {
        return;
    }

    // Spawner weapon: gamemd's Fire_At short-circuits here. It calls
    // `SpawnManagerClass::SetTarget` and returns NULL — no bullet, no damage,
    // no detonation effects, and no rearm timer write (the rearm write lives
    // further down Fire_At, past this branch). Because the branch returns above
    // the first random draw as well as above bullet allocation, a spawner fire
    // consumes zero scenario-RNG draws natively.
    //
    // `TechnoClass::GetFireError` step 18 (`disassemble_bytes 0x006FC606`) adds
    // three gates ahead of the shot, all read off the FIRER, not the target:
    //   1. `TechnoClass__IsOnBridge_ForFiring` (`0x00703B10`) → error 6.
    //      **NOT MODELLED.** It is not a plain "am I on a bridge deck" test —
    //      it is gated on the firer's own OnBridge byte being *clear*, then
    //      samples the firer's cell plus four direction-offset neighbours and
    //      tests cell flags 0x100/0x800 in four different combinations. It
    //      refuses a launch made from under or alongside a bridge span. Trigger:
    //      a V3/Dreadnought/Boomer ordered to fire while standing in a
    //      bridge-adjacent cell. Player effect: VERA launches where retail
    //      refuses; the missile flies where retail would have made the unit hold
    //      fire. Frequency: uncommon — needs the launcher parked under a span,
    //      which players avoid because it blocks line of sight anyway.
    //      Downstream risk: none — the gate only suppresses a launch, it feeds
    //      nothing.
    //
    //      Blocker updated 2026-08-19: the flags and offsets are no longer
    //      UNCHECKED. 0x100 is "cell belongs to a bridge", 0x800 is the span's
    //      axis bit, and the four offsets are `g_DirectionOffsets` 0x0089F688
    //      indices 4/0/2/6 — S, N, E, W — with N and S requiring 0x800 SET and
    //      E and W requiring it CLEAR (table filled at runtime from 0x0049F2F0).
    //      The predicate itself is now ported, as
    //      `app::presentation::instances::helpers::is_on_bridge_for_firing`.
    //      What blocks this gate is placement, not evidence: that function sits
    //      above the sim boundary and `sim/` must never depend on `app/`, so
    //      wiring it here means moving the predicate down to `map/bridge_facts`
    //      first and having both callers read it there.
    //   2. `this->vtable+0x380` (FootClass::IsParalyzed `0x004DE770`) → error
    //      6. MODELLED in `admit_attacker_fire`.
    //   3. `SpawnManagerClass::CountAliveSpawns == 0` → error 3. MODELLED below.
    if weapon.spawner {
        let alive = world
            .substrate
            .entities
            .get(snap.stable_id)
            .and_then(|e| e.spawn_manager.as_ref())
            .map(|m| m.count_alive_spawns())
            .unwrap_or(0);
        if alive > 0 {
            out.spawn_target_updates.push((snap.stable_id, snap.target));
        }
        return;
    }

    // `TechnoClass::Fire_At @ 0x006FDF5D..0x006FDF9D`: a `DrainWeapon=yes`
    // weapon (`WeaponType+0x142`) against a Techno whose type is
    // `Drainable=yes` (`+0x5EF`, read at `0x006FDF7B`) calls the link
    // installer `0x0070FD70`, then `Assign_Target(NULL)` (`[vtable+0x3C8]`
    // at `0x006FDF97`, applied where the link is installed) and returns NULL
    // — no bullet, no rearm, no report — so the shot below never runs. A
    // DrainWeapon aimed at anything else takes the `0x006FDE03` exit
    // (its identity is UNCHECKED; unreachable in stock, where the selection
    // ladder's arm K only picks the DrainWeapon against a Drainable target).
    if weapon.drain_weapon {
        if let TargetKind::Entity(target_id) = snap.target
            && rules
                .object(world.interner.resolve(target_type_ref))
                .is_some_and(|target_obj| target_obj.drainable)
        {
            out.drain_links.push((snap.stable_id, target_id));
        }
        return;
    }

    // Fire one shot!
    //
    // The burst index is needed twice — the fire coordinate mirrors its lateral
    // offset on odd shots, and the burst state machine advances on it — so it is
    // resolved once here.
    let burst = world
        .substrate
        .entities
        .get(snap.stable_id)
        .map(|entity| entity.weapon_burst)
        .unwrap_or_default();
    // FLH uses only odd/even parity; retain the signed dword in its owner.
    let burst_index = (burst.index() & 1) as u8;
    // One fire coordinate per shot: the bullet origin, the muzzle animation and
    // the report sound all take it (`combat::fire_coord`).
    let fire = super::fire_coord::fire_coordinate(
        world,
        rules,
        &super::fire_coord::FireSource::from(snap),
        obj,
        selected.slot,
        burst_index,
    );
    let warhead = selected.warhead;
    // `TechnoClass::Fire_At @ 0x006FDD50`, damage chain: the firepower fold
    // (`0x006FE33D..0x006FE34D`, country x per-unit x `Damage=`, not
    // modelled here — VERA reads the bare `Damage=`), THEN the FIREPOWER
    // ability stage `ftol(damage * Rules.VeteranCombat)` at
    // `0x006FE3C8..0x006FE3D8`, THEN the occupy multiplier. The firer's own
    // rank and type decide the stage — for a garrison shot that is the
    // building, exactly as native's `this` is.
    let firer_rank = self::veterancy::rank_from_u16(snap.veterancy);
    let veteran_damage = self::veterancy::scale_if_ability(
        weapon.damage,
        firer_rank,
        obj,
        crate::rules::object_type::Ability::Firepower,
        rules.general.veteran_combat,
    );
    // Garrison damage: apply OccupyDamageMultiplier to base damage before AoE or
    // single-target paths. Matches gamemd Fire_At which modifies damage before bullet
    // creation, so AoE splash uses the modified value.
    let base_damage = if is_garrison {
        sim_to_i32(
            SimFixed::from_num(veteran_damage) * rules.garrison_rules.occupy_damage_multiplier,
        )
    } else {
        veteran_damage
    };
    let persistent_delivery = classify_projectile_delivery(weapon, rules);
    if let ProjectileDelivery::Persistent {
        arm_frames,
        tracks_target,
        collision,
        ballistic,
        vertical,
        acceleration,
        launch_scatter_is_flak,
        mut guidance,
    } = persistent_delivery
    {
        let impact_world_z_leptons = attack_world_z_leptons(
            snap.target,
            target_rx,
            target_ry,
            target_sub_x,
            target_sub_y,
            &mut world.substrate.entities,
            world.resolved_terrain.as_ref(),
        );
        let origin_world_z_leptons = fire.source_z;
        let impact = ProjectileCoord::new(
            i32::from(target_rx) * 256 + target_sub_x.to_num::<i32>(),
            i32::from(target_ry) * 256 + target_sub_y.to_num::<i32>(),
            impact_world_z_leptons,
        );
        let target = match snap.target {
            TargetKind::Entity(id) => ProjectileTarget::Entity(id),
            TargetKind::Cell(rx, ry) => ProjectileTarget::Cell { rx, ry },
        };
        // `TechnoClass::Fire_At` launches the bullet FROM the fire coordinate
        // and derives the launch velocity as `target - FLH`, so the barrel
        // offset sets both where the shot starts and which way it leaves.
        let mut origin = fire.coord;
        let aim_facing16 = fire.aim_facing16;
        let body_facing16 = crate::sim::movement::turret::body_facing_to_turret(snap.facing);
        let projectile_type = weapon
            .projectile
            .as_deref()
            .and_then(|projectile_id| rules.projectile(projectile_id));
        // `TechnoClass::FireAt` step order, `0x006FE663`..`0x006FEA52`:
        // resolve the target delta, scatter it, recompute the facing from the
        // scattered delta, clamp the launch speed to half the straight-line
        // distance, and only then force a homing or vertical shot to one
        // lepton per frame.
        let frozen_target_position = impact;
        let delta = (
            impact.x - origin.x,
            impact.y - origin.y,
            impact.z - origin.z,
        );
        let delta = match launch_scatter_is_flak {
            Some(flak) => crate::sim::projectile::projectile_launch_scatter(
                delta,
                rules.combat_damage.ballistic_scatter,
                (weapon.range * SimFixed::from_num(crate::util::lepton::LEPTONS_PER_CELL_I32))
                    .to_num::<i32>(),
                flak,
                &mut world.scenario_rng,
            ),
            None => delta,
        };
        let impact =
            ProjectileCoord::new(origin.x + delta.0, origin.y + delta.1, origin.z + delta.2);
        use crate::sim::projectile::launch::{
            FireAtLaunch, FireAtLaunchResult, fireat_launch, high_arc_root,
        };
        let raw_source = ProjectileCoord::new(
            i32::from(snap.pos_rx) * 256 + snap.sub_x.to_num::<i32>(),
            i32::from(snap.pos_ry) * 256 + snap.sub_y.to_num::<i32>(),
            origin_world_z_leptons,
        );
        // 70D590 reads the source's live current target, independently of the
        // FireAt parameter and the scattered launch delta.
        let current_target = world
            .substrate
            .entities
            .get(snap.stable_id)
            .and_then(|source| source.attack_target.as_ref())
            .map(|attack| attack.target);
        let target_location = |target: TargetKind| -> Option<ProjectileCoord> {
            match target {
                TargetKind::Entity(id) => world.substrate.entities.get(id).map(|entity| {
                    let (rx, ry, sx, sy) = target_coords(entity, Some(rules), &world.interner);
                    ProjectileCoord::new(
                        i32::from(rx) * 256 + sx.to_num::<i32>(),
                        i32::from(ry) * 256 + sy.to_num::<i32>(),
                        object_world_z_leptons(entity, world.resolved_terrain.as_ref()),
                    )
                }),
                TargetKind::Cell(rx, ry) => {
                    use crate::sim::cell_rect::{CellRef, get_cellclass_fallback};
                    let cell = get_cellclass_fallback(
                        world.resolved_terrain.as_ref(),
                        i32::from(rx),
                        i32::from(ry),
                    );
                    let (x, y, level, slope) = match cell {
                        CellRef::Real(cell) => (
                            i32::from(cell.rx as i16),
                            i32::from(cell.ry as i16),
                            cell.level,
                            cell.slope_type,
                        ),
                        CellRef::Dummy { cell } => {
                            let cell = cell.snapshot();
                            (
                                cell.coord.0,
                                cell.coord.1,
                                cell.level as u8,
                                cell.slope_type,
                            )
                        }
                    };
                    let x = x * 256 + 128;
                    let y = y * 256 + 128;
                    Some(ProjectileCoord::new(
                        x,
                        y,
                        crate::util::lepton::ground_height_leptons(level, slope, x, y)
                            .expect("native target cell slope"),
                    ))
                }
            }
        };
        let flh_origin_z = origin.z;
        // 6FE947..6FE98A: directed launches read virtual+308, then
        // Dropping replaces only the launch origin via source+48. The delta
        // was already computed from FLH and must not be recomputed here.
        let directed_heading = projectile_type
            .filter(|projectile| projectile.dropping || projectile.rot != 0)
            .map(|_| {
                let source = world.substrate.entities.get(snap.stable_id);
                let hull = source.map_or(body_facing16, |source| {
                    crate::sim::movement::turret::hull_facing_16(source, binary_frame)
                });
                match snap.category {
                    EntityCategory::Unit if obj.has_turret => source
                        .and_then(|source| source.barrel_facing.as_ref())
                        .map_or(0, |facing| facing.current(binary_frame)),
                    EntityCategory::Unit | EntityCategory::Infantry => hull,
                    EntityCategory::Aircraft => source
                        .and_then(|source| source.barrel_facing.as_ref())
                        .map_or(0, |facing| facing.current(binary_frame)),
                    // RESIDUAL: Building +308 (44D7D0/43ED40) has current-target,
                    // pixel-offset and quantization producers beyond this owner.
                    // Ordinary stock reachability of a directed Building projectile
                    // is not established; its input producer remains open.
                    EntityCategory::Structure => aim_facing16,
                }
            });
        if projectile_type.is_some_and(|projectile| projectile.dropping) {
            origin = target_location(TargetKind::Entity(snap.stable_id))
                .expect("live native dropping source");
        }
        let current_target_coord = (ballistic && !weapon.lobber)
            .then(|| current_target.and_then(target_location))
            .flatten();
        // The scalar launch consumer receives the source +300 Z. Stock
        // Building +300 preserves raw Z; Infantry delegates its FLH getter.
        // Unit/Aircraft use the transformed zero-vector pivot. The existing
        // flat FLH/pivot producer still lacks locomotor slope translation.
        let pivot_z = if snap.category == EntityCategory::Infantry {
            flh_origin_z
        } else {
            raw_source.z
        };
        let voxel = projectile_type.is_some_and(|projectile| projectile.voxel);
        let building_pitch_height = (!ballistic && !voxel && delta.2.wrapping_abs() > 200)
            .then(|| current_target)
            .flatten()
            .and_then(|target| match target {
                TargetKind::Entity(id) => world.substrate.entities.get(id),
                TargetKind::Cell(_, _) => None,
            })
            .filter(|target| target.category == EntityCategory::Structure)
            .and_then(|target| rules.object(world.interner.resolve(target.type_ref())))
            .map(|target_type| {
                rules
                    .building_launch_height(target_type)
                    .wrapping_mul(200)
                    .wrapping_sub(pivot_z)
            });
        let launch = if let Some(guidance) = guidance.as_mut() {
            // Homing's launch/steering producer remains open. Its integer XY
            // output is widened into the same authoritative binary64 state.
            let heading_bam = aim_facing16.wrapping_sub(0x4000);
            guidance.heading_bam = heading_bam;
            guidance.fuse_reference = impact;
            Some(FireAtLaunchResult {
                velocity: ProjectileVelocity::new(
                    crate::sim::movement::homing_movement::cos_bam(heading_bam).to_num::<i32>(),
                    crate::sim::movement::homing_movement::sin_bam(heading_bam).to_num::<i32>(),
                    0,
                ),
                speed: 1,
            })
        } else {
            fireat_launch(FireAtLaunch {
                delta: ProjectileCoord::new(delta.0, delta.1, delta.2),
                speed: weapon.speed,
                vertical: vertical.is_some(),
                heading: directed_heading,
                arcing: ballistic,
                gravity: crate::sim::projectile::projectile_gravity(
                    rules.general.gravity,
                    collision.floater,
                ),
                high_root: high_arc_root(weapon.lobber, raw_source, current_target_coord),
                voxel_downward: (!ballistic && voxel).then(|| {
                    target_location(snap.target)
                        .expect("live native FireAt target")
                        .z
                        < raw_source.z
                }),
                building_pitch_height,
            })
        };
        // 6FF000/6FF93C destroys a failed launch, then rejoins the remaining
        // FireAt effects at6FF749. Do not manufacture a stationary projectile
        // or return before the common fire-event/burst bookkeeping below.
        if let Some(FireAtLaunchResult {
            velocity,
            speed: launch_speed,
        }) = launch
        {
            let visual = projectile_type
                .map(|projectile| {
                    ProjectileVisualState::new(
                        projectile.anim_low as u8,
                        projectile.anim_high as u8,
                        projectile.anim_rate as u8,
                    )
                })
                .unwrap_or_else(|| ProjectileVisualState::new(0, 0, 0));
            out.projectile_spawns.push(ProjectileSpawn {
                flat: projectile_type.is_some_and(|projectile| projectile.flat),
                source_id: snap.stable_id,
                origin,
                target,
                initial_target_position: frozen_target_position,
                payload: ProjectilePayload {
                    base_damage,
                    warhead: world.interner.intern(&warhead.id),
                    weapon: world.interner.intern(selected.weapon_id),
                    owner: snap.owner,
                },
                speed_leptons_per_frame: launch_speed.clamp(0, i32::from(u16::MAX)) as u16,
                velocity,
                trajectory: match vertical {
                    Some(detonation_altitude) => ProjectileTrajectory::Vertical {
                        detonation_altitude,
                        acceleration,
                        max_speed: weapon.speed,
                    },
                    None if ballistic || guidance.is_none() => ProjectileTrajectory::Ballistic,
                    None => ProjectileTrajectory::Straight,
                },
                guidance,
                visual,
                arm_frames: projectile_arm_delay(arm_frames, target, &mut world.substrate.entities),
                fuse_frames: None,
                // AI 467C0C calls Check for ROT>0 or Ranged even when
                // Dropping later suppresses detector-only admission.
                ranged_fuse: tracks_target
                    || projectile_type.is_some_and(|projectile| projectile.ranged),
                tracks_target,
                target_expiry: TargetExpiryPolicy::DetonateAtLastKnown,
                collision,
            });
        }
    } else {
        let impact_z = attack_impact_z(
            snap.target,
            &mut world.substrate.entities,
            world.resolved_terrain.as_ref(),
        );
        let air_impact = attack_air_impact(
            snap.target,
            target_rx,
            target_ry,
            target_sub_x,
            target_sub_y,
            &mut world.substrate.entities,
            world.resolved_terrain.as_ref(),
        );
        let world_z_leptons = attack_world_z_leptons(
            snap.target,
            target_rx,
            target_ry,
            target_sub_x,
            target_sub_y,
            &mut world.substrate.entities,
            world.resolved_terrain.as_ref(),
        );
        let routed_wall = wall_overlay_flags_at(
            world.overlay_grid.as_ref(),
            overlay_registry,
            target_rx,
            target_ry,
        )
        .is_some_and(|flags| warhead_damages_wall(warhead, flags));
        let wh_iid = world.interner.intern(&warhead.id);
        let aoe = {
            let collected = collect_area(
                world,
                rules,
                overlay_registry,
                (target_rx, target_ry),
                base_damage,
                warhead,
                (snap.stable_id, Some(snap.owner), wh_iid),
                air_impact,
                impact_z,
            );
            append_fixture_tiberium(world, &mut out.effects.tiberium_reduction_requests);
            collected
        };
        #[cfg(test)]
        out.effects.wall_mutations.extend(aoe.wall_mutations);

        #[cfg(test)]
        out.effects
            .cell_target_detaches
            .extend(aoe.cell_target_detaches);

        out.damage_events.extend(aoe.receivers);
        if !scenario_no_damage && base_damage > 0 && !routed_wall && warhead.wall {
            let wh_iid = world.interner.intern(&warhead.id);
            out.effects.bridge_damage_events.push(BridgeDamageEvent {
                rx: target_rx,
                ry: target_ry,
                damage: base_damage.min(i32::from(u16::MAX)) as u16,
                warhead_ref: wh_iid,
                is_ion_cannon: wh_iid
                    == handles
                        .expect("Simulation::resolve_type_handles must run before combat")
                        .ion_cannon,
                impact_z,
            });
        }
        // Radiation-emitting detonation: one site request per shot at the impact
        // cell. Spread is the warhead's CellSpread truncated to whole cells.
        if weapon.rad_level > 0 {
            out.effects
                .rad_detonations
                .push(crate::sim::radiation::RadDetonation {
                    rx: target_rx,
                    ry: target_ry,
                    rad_level: weapon.rad_level,
                    spread: warhead.cell_spread.to_num::<i32>(),
                });
        }

        // One impact coordinate: the original engine hands the SAME resolved
        // coord to the area-damage call and to the AnimList placement, so the
        // animation height is the impact height that fed the damage above,
        // never a separately derived value. (Only the smudge dispatcher
        // re-derives a ground reference of its own; the animation is drawn at
        // this z.)
        let effect_z: u8 = impact_z_byte(impact_z);
        // BulletClass::Detonate randomizes only the visible CoordStruct for an
        // Inviso projectile. The draw happens before AnimList selection, so this
        // must run even when the warhead has no animation to emit.
        let (effect_rx, effect_ry, effect_sub_x, effect_sub_y) = if weapon
            .projectile
            .as_deref()
            .and_then(|projectile_id| rules.projectile(projectile_id))
            .is_some_and(|projectile| projectile.inviso)
        {
            inviso_scatter::scatter_inviso_effect_coord(
                &mut world.scenario_rng,
                target_rx,
                target_ry,
                target_sub_x,
                target_sub_y,
            )
        } else {
            (target_rx, target_ry, target_sub_x, target_sub_y)
        };
        emit_warhead_detonation_effects(
            warhead,
            base_damage,
            effect_rx,
            effect_ry,
            effect_sub_x,
            effect_sub_y,
            effect_z,
            world_z_leptons,
            &mut world.interner,
            &mut out.effects.explosion_effects,
            &mut out.effects.smudge_spawn_requests,
        );
    }

    let report_sound_id = weapon
        .report
        .as_ref()
        .map(|report_id| world.interner.intern(report_id));
    out.fire_events.push(SimFireEvent {
        attacker_id: snap.stable_id,
        attacker_type_ref: snap.type_id,
        weapon_slot: selected.slot,
        weapon_id: world.interner.intern(selected.weapon_id),
        facing: snap.facing,
        veterancy: snap.veterancy,
        origin_snapshot: FireOriginSnapshot {
            rx: snap.pos_rx,
            ry: snap.pos_ry,
            z: snap.pos_z,
            sub_x: snap.sub_x,
            sub_y: snap.sub_y,
            facing: snap.facing,
        },
        target: snap.target,
        report_sound_id,
        fire_coord: fire.coord,
        fire_offset_y: fire.offset_y,
        muzzle_anim: super::fire_coord::muzzle_anim_name(
            weapon,
            fire.aim_facing16,
            snap.garrison.is_some(),
        )
        .map(|name| world.interner.intern(name)),
        occupied_building: snap.garrison.is_some(),
        firer_category: snap.category,
    });
    if weapon.reveal_on_fire {
        out.reveal_events.push(RevealEvent {
            owner: snap.owner,
            rx: snap.pos_rx,
            ry: snap.pos_ry,
            radius: REVEAL_ON_FIRE_RADIUS,
        });
    }

    let next_index = burst.next_index();
    let mid_burst = next_index < weapon.burst;
    if mid_burst {
        // gamemd-derived: `TechnoClass::GetROF @ 0x006FCFA0`, mid-burst branch.
        // The gap between shots inside a burst is drawn, not fixed.
        //
        // RESIDUAL (GSI-08.05) — the Unit override ahead of the draw is not
        // modelled. Native checks UnitType's per-burst delays
        // (`+0xE44 + idx*4`, sentinel `-1`) first for a Unit firer and
        // returns the authored value without drawing. No stock section authors
        // any `BurstDelay%d=`, so every stock burst reaches the draw and the
        // draw count is unchanged; a mod that authors one would diverge, and
        // would also consume an RNG draw native does not.
        let burst_delay = world.scenario_rng.next_range_u32_inclusive(
            BURST_INTER_SHOT_DELAY_MIN as u32,
            BURST_INTER_SHOT_DELAY_MAX as u32,
        ) as u8;
        out.burst_updates.push((snap.stable_id, burst_delay, 0));
    } else {
        let mut rof_ticks = rof_to_cooldown_frames(weapon.rof, &mut world.scenario_rng);
        // `GetROF @ 0x006FCFA0`, `0x006FD0E2..0x006FD14C`: a ROF-ability
        // holder then stores `ftol(rof * Rules.VeteranROF)` — applied ONCE,
        // after the jitter and before the garrison divides. The firer's rank
        // and type decide it (a garrison shot reads the building's).
        rof_ticks = veteran_rof_frames(
            rof_ticks,
            self::veterancy::rank_from_u16(snap.veterancy),
            obj,
            rules.general.veteran_rof,
        );
        // Garrison ROF: divide by occupant count, then by multiplier.
        // More occupants = proportionally faster fire (gamemd GetROF 0x006FCFA0).
        if let Some(ref gs) = snap.garrison {
            let count = (gs.occupant_count as u16).max(1);
            rof_ticks /= count;
            if rules.garrison_rules.occupy_rof_multiplier > SIM_ZERO {
                rof_ticks = sim_to_i32(
                    SimFixed::from_num(rof_ticks) / rules.garrison_rules.occupy_rof_multiplier,
                ) as u16;
            }
            rof_ticks = rof_ticks.max(1);
        }
        out.burst_updates.push((snap.stable_id, 0, rof_ticks));
    }

    // Aircraft ammo deduction: one ammo per burst completion (not per shot).
    if let Some(entity) = world.substrate.entities.get_mut(snap.stable_id) {
        entity.weapon_burst.complete_shot(weapon.burst.max(1));
    }
    if !mid_burst
        && !world
            .substrate
            .entities
            .get(snap.stable_id)
            .and_then(|entity| entity.aircraft_mission.as_ref())
            .is_some_and(|mission| mission.is_attacking())
    {
        out.ammo_deduct.push(snap.stable_id);
    }

    // Track garrison buildings that fired for round-robin advancement.
    if is_garrison {
        out.garrison_advance.push(snap.stable_id);
    }

    // `TechnoClass::Fire @ 0x006FF749..0x006FF872` runs after the bullet
    // launch, rearm, Report and weapon Anim.
    if weapon.limbo_launch {
        world.parasite_limbo_launch(snap.stable_id, snap.target, weapon, rules);
    }
}

fn commit_fire_bookkeeping(world: &mut Simulation, emit: &mut CombatEmit) {
    let binary_frame = world.session.binary_frame;
    let spawn_target_updates = std::mem::take(&mut emit.spawn_target_updates);
    let drain_links = std::mem::take(&mut emit.drain_links);
    let burst_updates = std::mem::take(&mut emit.burst_updates);
    // Spawner weapons: hand the fire target to the parent's spawn manager.
    // `SpawnManagerClass::SetTarget` only queues a target that differs from the
    // live one; the manager's own AI pass promotes it.
    for &(parent_id, target) in &spawn_target_updates {
        if let Some(manager) = world
            .substrate
            .entities
            .get_mut(parent_id)
            .and_then(|e| e.spawn_manager.as_mut())
        {
            manager.set_target(Some(target));
        }
    }
    // Drain weapons: `0x0070FD70` installs the reciprocal
    // `DrainTarget`/`DrainingMe` pair when the drainer sits over the victim.
    // `Fire_At @ 0x006FDF93..0x006FDF97` then calls `[vtable+0x3C8]` =
    // `TechnoClass::Assign_Target @ 0x006FCDB0` with NULL unconditionally
    // (the install's own cell gate does not feed back), which clears the
    // Target (`+0x2B4`), the passive-acquire byte (`+0x50C`), the burst index
    // (`+0x3B8`) and, when a SpawnManager (`+0x2D0`) exists, its target. The
    // disc therefore leaves `Fire_At` with no target and its Attack mission
    // takes the no-target exit into idle mode on its next dispatch. The
    // `+0x304` link the setter also releases is not modelled (identity
    // UNCHECKED; no stock drainer carries a SpawnManager or that link).
    for &(drainer_id, victim_id) in &drain_links {
        crate::sim::credit_income::install_drain_link(
            &mut world.substrate.entities,
            drainer_id,
            victim_id,
        );
        if let Some(drainer) = world.substrate.entities.get_mut(drainer_id) {
            represented_assign_target(drainer, None);
            if let Some(manager) = drainer.spawn_manager.as_mut() {
                manager.set_target(None);
            }
        }
    }

    for &(attacker_id, burst_delay, rof_cd) in &burst_updates {
        if let Some(entity) = world.substrate.entities.get_mut(attacker_id) {
            // gamemd-derived: `TechnoClass::Fire_At @ 0x006FF743` stores
            // `g_CurrentFrameCounter` into `+0x120` once the shot is committed.
            // With the constructor at `0x006F2B9C` that is the ONLY writer of
            // that field on a TechnoClass in the image, which is what makes
            // `UnitClass::Facing_Update`'s idle dwell a since-my-last-shot
            // timer rather than a since-target-loss one. `burst_updates` carries
            // exactly one entry per committed shot, so this is that store.
            entity.last_fire_frame = i64::from(binary_frame);
            if let Some(ref mut attack) = entity.attack_target {
                attack.burst_delay_ticks = burst_delay;
                attack.cooldown_ticks = rof_cd;
            }
            // `TechnoClass::Fire_At @ 0x006FDD50` writes the SAME rearm
            // countdown into `TechnoClass+0x2EC/+0x2F4` (stores at 0x006FE4B0
            // and 0x006FF2AA), and `CanAutoCloak @ 0x006FBDC0` reads that timer
            // as its first gate after the `CloakState == 2` early-out
            // (`param_1[0xbb]`/`[0xbd]`). VERA keeps the rearm counter on the
            // attack record instead of the object, so the cloak runtime carries
            // its own copy of the same value; without it a Typhoon that
            // surfaced to fire could re-dive on the very next tick.
            //
            // The duration native stores is `CALL [EDX+0x318]` at 0x006FE49E
            // — `TechnoClass::GetROF @ 0x006FCFA0` (vtable slot read at
            // 0x007F4C78) — whose MID-BURST branch returns the inter-shot gap,
            // not zero. Native keeps one timer; VERA splits it into
            // `cooldown_ticks` (armed on the burst's last shot) and
            // `burst_delay_ticks` (armed between burst shots). The two
            // decrement together and the fire gate is their union, so the
            // native `+0x2F4` value is whichever of the two this shot armed.
            // With `[BoomerTorpedo] Burst=2` on a `Cloakable=yes` BSUB, taking
            // the union is what stops a re-dive between the two torpedoes.
            let rearm_gate_frames = i32::from(rof_cd).max(i32::from(burst_delay));
            if let Some(cloak) = entity.cloak.as_mut() {
                cloak.arm_rearm_gate(binary_frame as i32, rearm_gate_frames);
            }
        }
    }
}

/// Boundaries of one synchronous FireAt transaction in the event accumulator.
struct FireCommitBoundary {
    damage_start: usize,
    explosion_start: usize,
    smudge_start: usize,
    current_weapon_start: usize,
    fire_event_start: usize,
}

impl FireCommitBoundary {
    fn capture(emit: &CombatEmit) -> Self {
        Self {
            damage_start: emit.damage_events.len(),
            explosion_start: emit.effects.explosion_effects.len(),
            smudge_start: emit.effects.smudge_spawn_requests.len(),
            current_weapon_start: emit.current_weapon_updates.len(),
            fire_event_start: emit.fire_events.len(),
        }
    }

    fn commit(
        self,
        world: &mut Simulation,
        run: &mut ReceiverRun,
        rules: &RuleSet,
        overlay_registry: Option<&OverlayTypeRegistry>,
        emit: &mut CombatEmit,
        under_attack_events: &mut Vec<UnderAttackEvent>,
    ) {
        let Self {
            damage_start,
            explosion_start,
            smudge_start,
            current_weapon_start,
            fire_event_start,
        } = self;
        let outer_explosion_effects = emit.effects.explosion_effects.split_off(explosion_start);
        let outer_anim_requests = emit.effects.smudge_spawn_requests.split_off(smudge_start);
        for &(entity_id, weapon_index, weapon_ref) in
            &emit.current_weapon_updates[current_weapon_start..]
        {
            if let Some(entity) = world.substrate.entities.get_mut(entity_id) {
                entity.current_weapon_index = weapon_index;
                entity.current_weapon_ref = Some(weapon_ref);
            }
        }
        let (inline_death, mut pings) = commit_area(
            world,
            run,
            &emit.damage_events[damage_start..],
            rules,
            overlay_registry,
        );
        emit.effects.append(inline_death);
        emit.effects
            .explosion_effects
            .extend(outer_explosion_effects);
        commit_smudges(
            world,
            rules,
            overlay_registry,
            outer_anim_requests,
            &mut emit.effects.smudge_spawn_requests,
        );
        under_attack_events.append(&mut pings);
        commit_fire_bookkeeping(world, emit);
        let wave_fire_events = emit.fire_events[fire_event_start..].to_vec();
        for event in &wave_fire_events {
            {
                if callbacks_enabled(world) {
                    world.commit_fired_wave(rules, event);
                }
            }
        }
    }
}

pub(crate) fn tick_combat(
    world: &mut Simulation,
    run: &mut ReceiverRun,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    tick_ms: u32,
    live_order: &[u64],
    fire_suppressed: &BTreeSet<u64>,
    aircraft_fire_requests: &BTreeSet<u64>,
    projectile_detonations: &[ProjectileDetonation],
    wave_damage_events: &[WaveDamageEvent],
) -> CombatTickResult {
    let radiation_enabled = radiation_enabled(world);

    let sound_enabled = sound_enabled(world);

    let binary_frame = world.session.binary_frame;
    let require_playfield_membership = world.playfield_bounds.is_some();
    #[cfg(test)]
    let require_playfield_membership = world
        .receiver_fixture
        .as_ref()
        .map_or(require_playfield_membership, |fixture| {
            fixture.require_playfield_membership
        });
    let fog_snapshot = world.fog.clone();
    let fog = Some(&fog_snapshot);
    #[cfg(test)]
    let fog = fog.filter(|_| {
        world
            .receiver_fixture
            .as_ref()
            .is_none_or(|fixture| fixture.fog_enabled)
    });
    let power_snapshot = world.power_states.clone();
    let power_states = &power_snapshot;
    let active_wave_owners: BTreeSet<_> = world.active_wave_links.keys().copied().collect();
    let missile_detonations = std::mem::take(&mut world.pending_missile_detonations);

    if tick_ms == 0 {
        return CombatTickResult {
            projectile_spawns: Vec::new(),
            unit_facing: Vec::new(),
            consequences: crate::sim::world::damage_consequences::DamageConsequences::ordinary(
                DeathEffects::default(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ),
        };
    }

    // Completed prior-frame bullets physically advanced before this frame's
    // object AI/fire walk. Each detonation commits ReceiveDamage and any
    // recursive death weapon before the next detonation or attacker reads
    // wall, target, health, or RNG state.
    let mut emit = CombatEmit::default();
    let mut under_attack_events = Vec::new();
    commit_projectile_detonations_inline(
        world,
        run,
        rules,
        overlay_registry,
        projectile_detonations,
        &mut emit,
        &mut under_attack_events,
    );
    for detonation in &missile_detonations {
        let damage_start = emit.damage_events.len();
        emit_missile_detonations(
            world,
            rules,
            overlay_registry,
            std::slice::from_ref(detonation),
            &mut emit,
        );
        let (inline_death, mut pings) = commit_area(
            world,
            run,
            &emit.damage_events[damage_start..],
            rules,
            overlay_registry,
        );
        emit.effects.append(inline_death);
        under_attack_events.append(&mut pings);
    }

    // Pre-scan: collect entities blocked from firing by locomotor or power state.
    let fire_blocked = combat_fire_gate::collect_fire_blocked_entities(
        &mut world.substrate.entities,
        power_states,
        Some(rules),
        &mut world.interner,
    );

    let keys: Vec<u64> = world.substrate.entities.keys_sorted();

    // Deployed self-irradiator re-fire (Desolator): a deployed DeployFire unit
    // whose deploy weapon emits radiation — and whose own type is radiation-
    // immune — maintains the radiation field under its feet. When the site at
    // its cell is missing or its effective level has decayed below a third of
    // the weapon's RadLevel, the unit force-fires its deploy weapon at its own
    // cell (the detonation re-arms the site, which closes the gate again).
    // The synthesized self-target is cleared once the gate closes; targets the
    // player set explicitly are never touched.
    if let Some(rad) = radiation_enabled.then_some(&world.radiation) {
        let mut set_self_target: Vec<u64> = Vec::new();
        let mut clear_self_target: Vec<u64> = Vec::new();
        for &id in &keys {
            if fire_suppressed.contains(&id) {
                continue;
            }
            let Some(entity) = world.substrate.entities.get(id) else {
                continue;
            };
            if !entity.is_fully_deployed() || entity.dying || !entity.is_alive() {
                continue;
            }
            let Some(obj) = rules.object(world.interner.resolve(entity.type_ref())) else {
                continue;
            };
            if !obj.deploy_fire || !obj.immune_to_radiation {
                continue;
            }
            let Some(weapon) = combat_weapon::deploy_fire_weapon_id(obj, entity.veterancy)
                .and_then(|weapon_id| rules.weapon(weapon_id))
            else {
                continue;
            };
            if weapon.rad_level <= 0 {
                continue;
            }
            let own_cell = (entity.position.rx, entity.position.ry);
            let gate_open = rad.site_at(own_cell).is_none_or(|site| {
                crate::sim::radiation::RadiationState::current_site_level(site)
                    < weapon.rad_level / 3
            });
            let has_self_target = matches!(
                entity.attack_target.as_ref().map(|attack| attack.target),
                Some(TargetKind::Cell(rx, ry)) if (rx, ry) == own_cell
            );
            if gate_open && entity.attack_target.is_none() {
                set_self_target.push(id);
            } else if !gate_open && has_self_target {
                clear_self_target.push(id);
            }
        }
        for id in set_self_target {
            if let Some(entity) = world.substrate.entities.get_mut(id) {
                let (rx, ry) = (entity.position.rx, entity.position.ry);
                entity.attack_target = Some(AttackTarget::for_cell(rx, ry));
            }
        }
        for id in clear_self_target {
            if let Some(entity) = world.substrate.entities.get_mut(id) {
                entity.attack_target = None;
                entity.passively_acquired_target = false;
            }
        }
    }

    // Garrison auto-acquire: idle garrisoned buildings scan for hostile targets.
    // Runs before Phase 1 so newly-targeted buildings are included in snapshots.
    for &id in &keys {
        let (is_candidate, owner, pos_rx, pos_ry, sub_x, sub_y, type_id, _barrel_facing) = {
            let entity = match world.substrate.entities.get(id) {
                Some(e) => e,
                None => continue,
            };
            if entity.category != EntityCategory::Structure
                || entity.attack_target.is_some()
                || entity.dying
                || !entity.is_alive()
                || fire_blocked.contains(&id)
            {
                continue;
            }
            (
                true,
                entity.owner(),
                entity.position.rx,
                entity.position.ry,
                entity.position.sub_x,
                entity.position.sub_y,
                entity.type_ref(),
                entity.barrel_facing,
            )
        };
        if !is_candidate {
            continue;
        }

        let obj = match rules.object(world.interner.resolve(type_id)) {
            Some(o) => o,
            None => continue,
        };
        if !obj.can_be_occupied || !obj.can_occupy_fire {
            continue;
        }

        // Read cargo info (immutable borrow).
        let (occ_id, half_foundation) = {
            let entity = match world.substrate.entities.get(id) {
                Some(e) => e,
                None => continue,
            };
            let cargo = match entity.passenger_role.cargo() {
                Some(c) if !c.is_empty() => c,
                _ => continue,
            };
            let fi = cargo.garrison_fire_index as usize % cargo.count() as usize;
            let occ_id = cargo.passengers[fi];
            let (fw, fh) = foundation_dimensions(&obj.foundation);
            (occ_id, fw.min(fh) / 2)
        };

        // Resolve occupant type + veterancy for garrison weapon validation.
        let (occ_type, occ_vet) = match world.substrate.entities.get(occ_id) {
            Some(occ) => (occ.type_ref(), occ.veterancy),
            None => continue,
        };

        // Scan range = half_foundation + 1 + OccupyWeaponRange (gamemd Greatest_Threat).
        let scan_cells = half_foundation as i32 + 1 + rules.garrison_rules.occupy_weapon_range;
        let scan_range = SimFixed::from_num(scan_cells.max(1));

        // Scan for best hostile target using garrison weapon for Verses/projectile checks.
        // gamemd's Greatest_Threat calls GetWeapon on the building, which returns
        // the occupant's OccupyWeapon — not the occupant's primary weapon.
        let mut best_target: Option<(i64, u8, u64)> = None;
        let owner_str = world.interner.resolve(owner);
        for candidate in world.substrate.entities.values() {
            if candidate.stable_id() == id
                || candidate.health.current == 0
                || candidate.dying
                || candidate.lifecycle.in_limbo
                || candidate.passenger_role.is_inside_transport()
            {
                continue;
            }
            if candidate.owner() == owner {
                continue;
            }
            if let Some(fog_state) = fog {
                let candidate_owner_str = world.interner.resolve(candidate.owner());
                if fog_state.is_friendly(owner_str, candidate_owner_str) {
                    continue;
                }
                if !fog_state.is_cell_visible(owner, candidate.position.rx, candidate.position.ry) {
                    continue;
                }
            }
            let target_cat = combat_target_category(candidate, rules, &world.interner);
            let target_armor = rules
                .object(world.interner.resolve(candidate.type_ref()))
                .map(|o| o.armor.as_str())
                .unwrap_or("none");
            // Use garrison weapon (OccupyWeapon) for target compatibility check.
            let occ_type_str = world.interner.resolve(occ_type);
            let selected = match combat_weapon::select_garrison_weapon(
                rules,
                occ_type_str,
                occ_vet,
                target_cat,
                target_armor,
            ) {
                Some(s) => s,
                None => continue,
            };
            if combat_weapon::verses_gate(selected.verses_pct)
                == combat_weapon::VersesGate::Suppressed
            {
                continue;
            }
            // Garrison passive scan_range = half_foundation + 1 + OccupyWeaponRange,
            // which never matches selected.weapon.range — same override-fallback
            // case as the scan_range_override branch in acquire_best_target. Keep
            // the 2D check until a future stage threads override-aware 3D.
            let dist_sq = lepton_distance_sq_raw(
                pos_rx,
                pos_ry,
                sub_x,
                sub_y,
                candidate.position.rx,
                candidate.position.ry,
                candidate.position.sub_x,
                candidate.position.sub_y,
            );
            if !is_within_range_leptons(dist_sq, scan_range) {
                continue;
            }
            // Same VERA-internal two-bucket ordering as `threat_class`, on the
            // native `Is_Armed` model rather than `Primary=` so a `[SREF]` or
            // `[YAGGUN]` candidate is not ranked as an unarmed bystander.
            let class = match rules.object(world.interner.resolve(candidate.type_ref())) {
                Some(o) if combat_weapon::is_armed(candidate, o) => 0u8,
                _ => 1,
            };
            let rank = (dist_sq, class, candidate.stable_id());
            match best_target {
                Some(current) if rank >= current => {}
                _ => best_target = Some(rank),
            }
        }

        if let Some((_, _, target_id)) = best_target {
            if let Some(building) = world.substrate.entities.get_mut(id) {
                building.attack_target = Some(AttackTarget::new(target_id));
            }
        }
    }

    // Phase 1: snapshot all attackers and advance cooldowns / burst delays.
    let mut snapshots: Vec<AttackerSnapshot> = Vec::new();
    for &id in &keys {
        // TubeMovement owns this object's complete AI turn.  The active state
        // may already have cleared on finalization, so the world host carries
        // the entry-time suppression set into this phased combat adapter.
        if fire_suppressed.contains(&id) {
            continue;
        }
        // Mutable borrow: tick cooldowns and capture the per-attacker scalars +
        // garrison cargo info. Entity field-reads move into `build_attacker_snapshot`
        // (pure) below, after this borrow releases.
        let (
            attack_target,
            cooldown_ticks,
            burst_delay_ticks,
            pending_infantry_fire,
            pending_building_fire,
            garrison_cargo,
        ) = {
            let entity = match world.substrate.entities.get_mut(id) {
                Some(e) => e,
                None => continue,
            };
            // Skip entities inside a transport — they can't fire (unless OpenTopped, deferred).
            if entity.passenger_role.is_inside_transport() {
                continue;
            }
            if entity.dying || !entity.is_alive() {
                // BuildingClass::Update no longer reaches ProcessDelayedFire
                // once the object is dead.
                continue;
            }
            let attack_state = entity.attack_target.as_mut().map(|attack| {
                attack.cooldown_ticks = attack.cooldown_ticks.saturating_sub(1);
                attack.burst_delay_ticks = attack.burst_delay_ticks.saturating_sub(1);
                (
                    attack.target,
                    attack.cooldown_ticks,
                    attack.burst_delay_ticks,
                    attack.pending_infantry_fire,
                )
            });

            // gamemd-derived: BuildingClass::Update @ 0x0043FB20 invokes
            // ProcessDelayedFire @ 0x004503F0 after mission dispatch. The
            // signed counter is pre-decremented and values <= 0 clamp to zero
            // and expire on this visit.
            let pending_building_fire = entity.pending_building_fire.as_mut().map(|pending| {
                pending.remaining_ticks = pending.remaining_ticks.saturating_sub(1).max(0);
                *pending
            });
            if pending_building_fire.is_some_and(|pending| pending.remaining_ticks != 0) {
                // GetFireError @ 0x00447F10 blocks ordinary fire while armed.
                continue;
            }
            let Some((attack_target, cooldown_ticks, burst_delay_ticks, pending_infantry_fire)) =
                attack_state
            else {
                // Expiry reads only the live target. A missing target clears
                // the latch and does not acquire or drop another target.
                if pending_building_fire.is_some() {
                    entity.pending_building_fire = None;
                }
                continue;
            };
            // Skip snapshot for entities blocked by locomotor state (cooldowns still tick).
            if fire_blocked.contains(&id)
                || (entity
                    .aircraft_mission
                    .as_ref()
                    .is_some_and(|mission| mission.is_attacking())
                    && !aircraft_fire_requests.contains(&id))
            {
                // Delayed expiry rechecks fire admissibility and clears on any
                // failure rather than postponing until the building is usable.
                if pending_building_fire.is_some() {
                    entity.pending_building_fire = None;
                }
                continue;
            }

            // Extract garrison cargo info while we have the entity.
            let garrison_cargo: Option<(u8, u8, u64)> =
                if entity.category == EntityCategory::Structure {
                    entity.passenger_role.cargo().and_then(|c| {
                        if c.is_empty() {
                            return None;
                        }
                        let fi = c.garrison_fire_index;
                        let count = c.count() as u8;
                        let oi = fi as usize % count as usize;
                        Some((fi, count, c.passengers[oi]))
                    })
                } else {
                    None
                };

            (
                attack_target,
                cooldown_ticks,
                burst_delay_ticks,
                pending_infantry_fire,
                pending_building_fire,
                garrison_cargo,
            )
        }; // mutable borrow released

        // Re-fetch the attacker immutably (nothing mutated `entities` since the
        // borrow above released) and resolve any garrison occupant, then build the
        // snapshot through the shared `build_attacker_snapshot` so the field-reads
        // stay byte-identical to the per-object Fire→Facing host.
        let entity = match world.substrate.entities.get(id) {
            Some(e) => e,
            None => continue,
        };
        let garrison = garrison_cargo.and_then(|(fire_idx, count, occ_id)| {
            let obj = rules.object(world.interner.resolve(entity.type_ref()))?;
            if !obj.can_be_occupied || !obj.can_occupy_fire {
                return None;
            }
            let occ = world.substrate.entities.get(occ_id)?;
            let (fw, fh) = foundation_dimensions(&obj.foundation);
            Some(GarrisonSnapshot {
                occupant_type_id: occ.type_ref(),
                occupant_veterancy: occ.veterancy,
                fire_index: fire_idx,
                occupant_count: count,
                half_foundation: fw.min(fh) / 2,
            })
        });

        snapshots.push(build_attacker_snapshot(
            entity,
            attack_target,
            cooldown_ticks,
            burst_delay_ticks,
            pending_infantry_fire,
            pending_building_fire,
            garrison,
        ));
    }
    // Native combat resolves each object inline during the single live-object
    // (reveal/insertion-order) AI walk, so firing/damage/kill-credit order is
    // the live-object order, not stable-id. Sort the collected attacker
    // snapshots by their position in the live order. stable_id is the
    // deterministic tiebreaker for any attacker absent from the live order
    // (limbo objects do not fire) and makes an empty live_order reproduce the
    // previous stable-id order exactly.
    let live_index: std::collections::HashMap<u64, usize> = live_order
        .iter()
        .enumerate()
        .map(|(i, &id)| (id, i))
        .collect();
    snapshots.sort_by_key(|s| {
        (
            live_index.get(&s.stable_id).copied().unwrap_or(usize::MAX),
            s.stable_id,
        )
    });

    // UnitClass Facing_Update runs immediately after this object's Fire_At_Target,
    // before any bullet created by the fire reaches its later LogicVector slot.
    // Capture that read window for every Unit up front: VERA's unsupported-
    // projectile immediate path may enter fatal lifecycle synchronously below,
    // but that approximation must not make this frame's barrel destination
    // observe a target loss native Facing_Update cannot yet see.
    for snap in &snapshots {
        let Some(entity) = world
            .substrate
            .entities
            .get(snap.stable_id)
            .filter(|entity| entity.category == EntityCategory::Unit)
        else {
            continue;
        };
        emit.unit_facing.push(UnitFacingUpdate::from_facing_update(
            snap.stable_id,
            crate::sim::movement::turret::facing_update(
                entity,
                &world.substrate.entities,
                Some(rules),
                &world.interner,
                binary_frame,
            ),
        ));
    }

    // Phase 2: per-attacker fire decision + emission, in live-LOGIC snapshot
    // order. Each attacker is resolved through `resolve_attacker_fire` (the
    // reusable per-object fire body); emission order is identical to the prior
    // inline loop, preserving both event order and inline Scenario-RNG draws.
    // Fire is category-agnostic (Units fire through the same body here); Unit
    // FACING destinations use the preseeded native read window above, with
    // own-retarget/remove replacement below, then are applied post-batch by
    // `unit_post::apply_unit_facing`.
    for snap in &snapshots {
        let Some(live_attack) = world
            .substrate
            .entities
            .get(snap.stable_id)
            .filter(|entity| entity.is_alive() && !entity.dying)
            .and_then(|entity| {
                entity.attack_target.as_ref().map(|attack| {
                    (
                        attack.target,
                        attack.cooldown_ticks,
                        attack.burst_delay_ticks,
                        attack.pending_infantry_fire,
                        entity.pending_building_fire,
                    )
                })
            })
        else {
            continue;
        };
        let mut live_snap = snap.clone();
        live_snap.target = live_attack.0;
        live_snap.cooldown_ticks = live_attack.1;
        live_snap.burst_delay_ticks = live_attack.2;
        live_snap.pending_infantry_fire = live_attack.3;
        live_snap.pending_building_fire = live_attack.4;

        let n_retarget = emit.retarget_events.len();
        let n_remove = emit.remove_attack.len();
        let boundary = FireCommitBoundary::capture(&emit);
        if aircraft_fire_requests.contains(&live_snap.stable_id) {
            aircraft_release::fire(
                world,
                run,
                rules,
                overlay_registry,
                &live_snap,
                fog,
                require_playfield_membership,
                binary_frame,
                tick_ms,
                &mut emit,
                &mut under_attack_events,
            );
        } else {
            resolve_attacker_fire(
                world,
                rules,
                overlay_registry,
                &live_snap,
                fog,
                require_playfield_membership,
                binary_frame,
                tick_ms,
                active_wave_owners.contains(&live_snap.stable_id),
                &mut emit,
            );
            boundary.commit(
                world,
                run,
                rules,
                overlay_registry,
                &mut emit,
                &mut under_attack_events,
            );
        }
        // S3: only this Unit's explicit retarget/remove may replace its seeded
        // destination. Synchronous target expiry from VERA's immediate-delivery
        // approximation is deliberately not visible to native Facing_Update.
        let Some(e) = world
            .substrate
            .entities
            .get(snap.stable_id)
            .filter(|e| e.category == EntityCategory::Unit && e.barrel_facing.is_some())
        else {
            continue;
        };
        let own_retarget = emit.retarget_events[n_retarget..]
            .iter()
            .find(|&&(aid, _)| aid == snap.stable_id)
            .map(|&(_, tid)| tid);
        let own_removed = emit.remove_attack[n_remove..].contains(&snap.stable_id);
        // Which side of the `+0x6AF` store at `0x00736B16` the replacement
        // belongs on. A retarget still leaves `Target != 0`, so native reaches
        // it through arm A's `Set` at `0x00736A89` and the arc arms the latch
        // on its first frame. A removal leaves `Target == 0`, so native's arm A
        // does not run at all and the only `Set` left is arm B's idle return at
        // `0x00736BDD`, which the store precedes — that arc starts with a clear
        // latch. (VERA swings back on the removal tick rather than after the
        // dwell; that difference is the pre-existing S3 kill-tick behaviour,
        // not this flag.)
        let replacement_is_idle_return = own_retarget.is_none();
        let replacement: Option<u16> = if let Some(tid) = own_retarget {
            Some(
                crate::sim::movement::turret::facing_toward_target(
                    e,
                    &TargetKind::Entity(tid),
                    &world.substrate.entities,
                    Some(rules),
                    &world.interner,
                )
                .unwrap_or_else(|| crate::sim::movement::turret::body_facing_to_turret(e.facing)),
            )
        } else if own_removed {
            Some(crate::sim::movement::turret::body_facing_to_turret(
                e.facing,
            ))
        } else {
            None
        };
        if let Some(replacement) = replacement {
            let update = emit
                .unit_facing
                .iter_mut()
                .find(|u| u.entity_id == snap.stable_id)
                .expect("Unit attacker was seeded before fire");
            update.turret_destination = Some(replacement);
            update.turret_destination_is_idle_return = replacement_is_idle_return;
        }
    }
    // S3 residual: every Unit not in the attacker snapshot set (target-less,
    // or in-transport holders excluded at the snapshot build). This runs after
    // all attack ReceiveDamage/death-helper calls, so its target read observes
    // the same live state the next native object window would expose.
    {
        let mut computed: Vec<u64> = emit.unit_facing.iter().map(|u| u.entity_id).collect();
        computed.sort_unstable();
        for &id in &keys {
            if fire_suppressed.contains(&id) {
                continue;
            }
            if computed.binary_search(&id).is_ok() {
                continue;
            }
            let Some(e) = world.substrate.entities.get(id) else {
                continue;
            };
            if e.category != EntityCategory::Unit {
                continue;
            }
            emit.unit_facing.push(UnitFacingUpdate::from_facing_update(
                id,
                crate::sim::movement::turret::facing_update(
                    e,
                    &world.substrate.entities,
                    Some(rules),
                    &world.interner,
                    binary_frame,
                ),
            ));
        }
    }
    // Every projectile, missile, and live-order attack damage event emitted so
    // far is already committed. WaveClass::DamageArea is consumed below in its
    // native wave -> recorded-cell -> selected Cell-list order, followed by
    // periodic radiation in live-victim order.
    let committed_damage_event_count = emit.damage_events.len();
    for event in wave_damage_events {
        emit.damage_events
            .push(combat_aoe::AreaDamageReceiver::Entity(
                EntityDamageEvent::from_wave(*event, &mut world.substrate.entities),
            ));
    }
    // Destructure back into the named locals for post-fire state updates.
    let CombatEmit {
        mut effects,
        projectile_spawns,
        mut damage_events,
        mut remove_attack,
        retarget_events,
        fire_events,
        reveal_events,
        burst_updates: _,
        ammo_deduct,
        garrison_advance,
        pending_infantry_updates,
        animation_switches,
        current_weapon_updates: _,
        unit_facing,
        spawn_target_updates: _,
        drain_links: _,
    } = emit;

    // Phase 3: apply retargets. Burst and rearm writes already happened in
    // each attacker's `commit_fire_bookkeeping` boundary. Auto-retargets only ever produce Entity targets (acquire_best_target
    // scans hostile entities), so this wraps the u64 in TargetKind::Entity.
    for &(attacker_id, new_target_sid) in &retarget_events {
        if let Some(entity) = world.substrate.entities.get_mut(attacker_id) {
            retarget_preserving_rearm(entity, new_target_sid);
        }
    }
    for &(attacker_id, sequence) in &animation_switches {
        if let Some(entity) = world.substrate.entities.get_mut(attacker_id) {
            if entity.infantry_terminal.is_some() {
                continue;
            }
            if let Some(ref mut anim) = entity.animation {
                anim.switch_to(sequence);
            }
        }
    }
    for &(attacker_id, pending) in &pending_infantry_updates {
        if let Some(entity) = world.substrate.entities.get_mut(attacker_id) {
            if let Some(ref mut attack) = entity.attack_target {
                attack.pending_infantry_fire = pending;
            }
        }
    }
    // Phase 3b: deduct ammo from aircraft that completed a burst this tick.
    for &attacker_id in &ammo_deduct {
        if let Some(entity) = world.substrate.entities.get_mut(attacker_id) {
            if let Some(ref mut ammo) = entity.aircraft_ammo {
                if ammo.current > 0 {
                    ammo.current -= 1;
                }
            }
        }
    }

    // Phase 3c: advance garrison fire index for buildings that fired this tick.
    // Round-robin: (idx + 1) % count — matches gamemd Fire_At 0x006FDD50.
    for &building_id in &garrison_advance {
        if let Some(entity) = world.substrate.entities.get_mut(building_id) {
            if let Some(cargo) = entity.passenger_role.cargo_mut() {
                let count = cargo.count() as u8;
                if count > 0 {
                    cargo.garrison_fire_index = (cargo.garrison_fire_index + 1) % count;
                }
            }
        }
    }

    // Phase 3.5: fold radiation-emitting detonations into the field, then
    // collect the periodic radiation damage. The original applies this damage
    // inside each foot unit's own AI step, gated on the global frame counter;
    // the phased engine collects it here so deaths route through the same
    // death pipeline as weapon damage (death anim selection via the
    // RadSiteWarhead, owned-count bookkeeping, survivor ejection).
    if let Some(rad) = radiation_enabled.then_some(&mut world.radiation) {
        for det in effects.rad_detonations.drain(..) {
            rad.apply_detonation(
                det,
                binary_frame,
                &rules.radiation,
                world.resolved_terrain.as_ref(),
            );
        }
        if !rad.is_empty() && binary_frame.is_multiple_of(rules.radiation.application_delay as u32)
        {
            if let Some(rad_warhead) = rules.warhead(&rules.radiation.site_warhead) {
                let wh_iid = world.interner.intern(&rad_warhead.id);
                // Victims are walked in live-LOGIC order (the same order the
                // per-object AI would have applied this damage), stable-id
                // fallback for entities absent from the live order.
                let mut victim_ids: Vec<u64> = keys.clone();
                victim_ids
                    .sort_by_key(|&id| (live_index.get(&id).copied().unwrap_or(usize::MAX), id));
                for &id in &victim_ids {
                    let Some(entity) = world.substrate.entities.get(id) else {
                        continue;
                    };
                    // Buildings never take radiation damage; corpses, limbo
                    // (transported) and airborne units are exempt.
                    if entity.category == EntityCategory::Structure
                        || entity.dying
                        || !entity.is_alive()
                        || entity.immune_to_radiation
                        || entity.passenger_role.is_inside_transport()
                    {
                        continue;
                    }
                    let airborne = entity
                        .locomotor
                        .as_ref()
                        .is_some_and(|loco| loco.altitude > SIM_ZERO);
                    if airborne {
                        continue;
                    }
                    let level = rad.damaging_level(
                        (entity.position.rx, entity.position.ry),
                        rules.radiation.level_max,
                    );
                    if level <= 0 {
                        continue;
                    }
                    // FootClass::AI @ 0x004DA530 passes the signed two-stage
                    // ftol result directly to concrete ReceiveDamage at
                    // distance zero. Verses and live defender modifiers belong
                    // to that receiver, not this producer.
                    let base = (level as f64 * rules.radiation.level_factor) as i32;
                    damage_events.push(combat_aoe::AreaDamageReceiver::Entity(
                        EntityDamageEvent::direct_receiver(
                            id,
                            base,
                            0,
                            RAD_NO_ATTACKER,
                            None,
                            wh_iid,
                            ReceiverCallFlags {
                                ignore_defenses: false,
                                arg6: true,
                            },
                        ),
                    ));
                }
            }
        }
    }

    // Periodic radiation is the only damage appended after the native-order
    // projectile/missile/object windows above. Commit that late slice in its
    // existing live-victim order and enter any fatal death helper immediately.
    let (mut late_death, mut late_pings) = commit_area(
        world,
        run,
        &damage_events[committed_damage_event_count..],
        rules,
        overlay_registry,
    );
    if let Some(rad) = radiation_enabled.then_some(&mut world.radiation) {
        for det in late_death.rad_detonations.drain(..) {
            rad.apply_detonation(
                det,
                binary_frame,
                &rules.radiation,
                world.resolved_terrain.as_ref(),
            );
        }
    }
    // Earlier emission and recursive death already share this accumulator;
    // append the late radiation slice without rebuilding parallel vectors.
    effects.append(late_death);
    under_attack_events.append(&mut late_pings);

    // Phase 5: remove AttackTarget from finished attackers.
    remove_attack.sort_unstable();
    remove_attack.dedup();
    for &attacker_id in &remove_attack {
        if let Some(entity) = world.substrate.entities.get_mut(attacker_id) {
            entity.attack_target = None;
            // The provenance flag cannot outlive the target it describes.
            entity.passively_acquired_target = false;
        }
    }

    // Push the synchronously selected death sounds to the presentation sink;
    // entity UnInit itself remains the world-owned deferred handoff.
    if sound_enabled {
        let sink = &mut world.sound_events;
        for (die_id, rx, ry) in effects.death_sounds.drain(..) {
            sink.push(SimSoundEvent::EntityDied {
                die_sound_id: die_id,
                rx,
                ry,
            });
        }
    }

    if !damage_events.is_empty() {
        log::trace!(
            "Combat tick: {} shots fired, {} entities destroyed",
            damage_events.len(),
            run.handled_deaths.len(),
        );
    }

    CombatTickResult {
        projectile_spawns,
        unit_facing,
        consequences: crate::sim::world::damage_consequences::DamageConsequences::ordinary(
            effects,
            under_attack_events,
            run.navigation_changed_cells.clone(),
            reveal_events,
            fire_events,
        ),
    }
}

#[inline]
fn callbacks_enabled(_world: &Simulation) -> bool {
    #[cfg(test)]
    if _world.receiver_fixture.is_some() {
        return false;
    }
    true
}

#[inline]
fn receiver_tick(world: &Simulation) -> u64 {
    #[cfg(test)]
    if let Some(fixture) = world.receiver_fixture.as_ref() {
        return fixture.current_tick;
    }
    u64::from(world.session.binary_frame)
}

#[inline]
fn sound_enabled(_world: &Simulation) -> bool {
    #[cfg(test)]
    if let Some(fixture) = _world.receiver_fixture.as_ref() {
        return fixture.sound_enabled;
    }
    true
}

#[inline]
fn radiation_enabled(_world: &Simulation) -> bool {
    #[cfg(test)]
    if let Some(fixture) = _world.receiver_fixture.as_ref() {
        return fixture.radiation_enabled;
    }
    true
}

#[inline]
fn append_fixture_tiberium(_world: &mut Simulation, _out: &mut Vec<TiberiumReductionRequest>) {
    #[cfg(test)]
    if let Some(fixture) = _world.receiver_fixture.as_mut() {
        _out.append(&mut fixture.deferred_tiberium);
    }
}
