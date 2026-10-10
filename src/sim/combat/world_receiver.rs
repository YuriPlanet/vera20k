//! World-owned synchronous receiver execution.
//!
//! Gameplay authorities stay in Simulation across nested receiver/lifecycle
//! calls. Only recursion guards and ordered publication receipts are local.

use super::*;
use crate::sim::world::Simulation;
use crate::sim::world::techno_ai_cloak::emit_configured_cloak_sound;

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
    base_defense_response::respond_to_base_attack(world, rules, victim_id, attacker_id);
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn collect_area(
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

/// `Apply_area_damage @ 0x00489280` outside a damage transaction (an
/// anim's landing at `0x00423EAB`, the Psychic Dominator at `0x0053B16B`, a
/// bomb's fuse at `0x006FA712`):
/// the area's receivers around `impact` committed in order, then its bridge
/// continuation (`0x00489E87`). `origin` is the source object, the source
/// house and the warhead's id. Returns whether a bridge changed.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_area_damage(
    world: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    impact: ProjectileCoord,
    damage: i32,
    warhead: &WarheadType,
    origin: (u64, Option<InternedId>, InternedId),
) -> bool {
    let area = collect_area_at(
        world,
        rules,
        overlay_registry,
        impact,
        damage,
        warhead,
        origin,
    );
    let receipt =
        world.commit_noncombat_aoe_receivers(rules, overlay_registry, &area.aoe.receivers);
    let bridge_continued = continue_area_bridge_damage(
        world,
        rules,
        overlay_registry,
        area.cell,
        damage,
        origin.2,
        area.z_leptons,
        area.routed_wall,
        receipt.area_result.expect("area receiver receipt"),
    );
    receipt.bridge_state_changed || bridge_continued
}

/// [`apply_area_damage`] inside a damage transaction (a TerrainClass death's
/// C4 at `0x0071BABF`, a dying carrier's bomb at `0x0070267F`): the receivers
/// commit in `run`, then the bridge continuation. Their effects and pings go
/// back to the receiver that set the area off.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_area_damage_in_run(
    world: &mut Simulation,
    run: &mut ReceiverRun,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    impact: ProjectileCoord,
    damage: i32,
    warhead: &WarheadType,
    origin: (u64, Option<InternedId>, InternedId),
) -> (DeathEffects, Vec<UnderAttackEvent>) {
    let area = collect_area_at(
        world,
        rules,
        overlay_registry,
        impact,
        damage,
        warhead,
        origin,
    );
    let mut effects = DeathEffects::default();
    append_fixture_tiberium(world, &mut effects.tiberium_reduction_requests);
    #[cfg(test)]
    effects.wall_mutations.extend(area.aoe.wall_mutations);
    #[cfg(test)]
    effects
        .cell_target_detaches
        .extend(area.aoe.cell_target_detaches);
    let (nested, pings, area_result) =
        commit_area_with_dispatch(world, run, &area.aoe.receivers, rules, overlay_registry);
    effects.append(nested);
    effects.bridge_state_changed |= continue_area_bridge_damage(
        world,
        rules,
        overlay_registry,
        area.cell,
        damage,
        origin.2,
        area.z_leptons,
        area.routed_wall,
        area_result,
    );
    (effects, pings)
}

/// The first half of `Apply_area_damage @ 0x00489280` at `impact`, shared by
/// the bullet's ordinary arm and the two entry points above: the impact
/// cell's wall route, then the receivers collected around that cell, the air
/// query at the impact's sub-cell and height.
struct AreaAtImpact {
    cell: (u16, u16),
    z_leptons: i32,
    routed_wall: bool,
    aoe: combat_aoe::AoEDamageResult,
}

fn collect_area_at(
    world: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    impact: ProjectileCoord,
    damage: i32,
    warhead: &WarheadType,
    origin: (u64, Option<InternedId>, InternedId),
) -> AreaAtImpact {
    let (rx, ry, sub_x, sub_y, z_leptons) = projectile_impact_cell(impact);
    let routed_wall = area_routes_to_wall(world, overlay_registry, (rx, ry), warhead);
    let aoe = collect_area(
        world,
        rules,
        overlay_registry,
        (rx, ry),
        damage,
        warhead,
        origin,
        Some(combat_aoe::AoEAirImpact {
            sub_x,
            sub_y,
            z_leptons,
        }),
        z_leptons.div_euclid(LEPTONS_PER_LEVEL as i32),
    );
    AreaAtImpact {
        cell: (rx, ry),
        z_leptons,
        routed_wall,
        aoe,
    }
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
    /// Units the local player had selected when their receiver was last
    /// entered: `UnitClass::ReceiveDamage` reads it first
    /// (`0x00737C98..0x00737CB6`: IsSelected `+0x83` and
    /// `HouseClass::IsHumanPlayer @ 0x0050B6F0`), before the kill's Destroy
    /// callback deselects the unit, and a dying one hands it to its
    /// passengers and crewman.
    pub(crate) selected_units: Vec<u64>,
}

impl ReceiverRun {
    pub(crate) fn finish(self) -> Vec<(u16, u16)> {
        debug_assert!(self.finalizing_terrain.is_empty());
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
    let (effects, pings, _) =
        commit_area_with_dispatch(world, run, receivers, rules, overlay_registry);
    (effects, pings)
}

/// Apply_area_damage48935C initializes its receiver flag to zero, sets it
/// after each dispatched virtual ReceiveDamage489ABC (including ReturnZero),
/// and returns its inverse at48A47E. Keep this receipt transaction-local;
/// nested areas must not overwrite their parent's dispatched status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AreaDamageResult {
    ReceiverDispatched = 0,
    NoReceiver = 1,
    IronCurtain = 2,
}

pub(crate) fn commit_area_with_dispatch(
    world: &mut Simulation,
    run: &mut ReceiverRun,
    receivers: &[combat_aoe::AreaDamageReceiver],
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
) -> (DeathEffects, Vec<UnderAttackEvent>, AreaDamageResult) {
    let current_tick = receiver_tick(world);

    let isolation_armed =
        area_near_center_ic_isolation_armed(receivers, &mut world.substrate.entities, current_tick);
    let mut effects = DeathEffects::default();
    let mut under_attack_events = Vec::new();
    let mut dispatched = false;

    for receiver in receivers {
        match *receiver {
            combat_aoe::AreaDamageReceiver::Entity(event) => {
                if !area_record_dispatches(world, rules, &event) {
                    continue;
                }
                if !area_isolation_dispatches(world, &event, isolation_armed, current_tick) {
                    continue;
                }
                dispatched = true;
                let (nested, mut pings) = commit_entities(
                    world,
                    run,
                    std::slice::from_ref(&event),
                    Some(false),
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
                if !world
                    .production
                    .terrain_objects
                    .get(&event.stable_id)
                    .is_some_and(|terrain| terrain.is_live() && terrain.health > 0)
                {
                    continue;
                }
                dispatched = true;
                let (nested, mut pings) =
                    commit_terrain(world, run, event, false, rules, overlay_registry);
                effects.append(nested);
                under_attack_events.append(&mut pings);
            }
        }
    }

    let result = if isolation_armed {
        AreaDamageResult::IronCurtain
    } else if dispatched {
        AreaDamageResult::ReceiverDispatched
    } else {
        AreaDamageResult::NoReceiver
    };
    (effects, under_attack_events, result)
}

fn area_isolation_dispatches(
    world: &Simulation,
    event: &EntityDamageEvent,
    isolation_armed: bool,
    current_tick: u64,
) -> bool {
    !isolation_armed
        || !event.near_center_ic_isolation_eligible
        || world
            .substrate
            .entities
            .get(event.target_id)
            .is_some_and(|target| has_active_area_invulnerability(target, current_tick))
}

/// `Apply_area_damage @ 0x00489280`'s per-record dispatch gates
/// (`0x004899F1..0x00489A95`), read live when the record's turn comes: an
/// earlier record's receiver (a death weapon, an Ivan bomb, a C4 cascade) can
/// kill, remove or limbo a later record's object before it is reached. The
/// object must be alive (`+0x90`), not an `InvisibleInGame=` building
/// (BuildingType `+0x1701`, `0x00489A1B`), have Health above zero (`+0x6C`,
/// `0x00489A79`), be marked on the map (`+0x74`, `0x00489A80`) and be out of
/// limbo (`+0x81`, `0x00489A87`). The distance bound (`0x00489A91`) and the
/// airborne-aircraft halving (`0x00489A59..0x00489A77`) are fixed at
/// collection; the near-centre Iron Curtain filter is `area_isolation_dispatches`.
fn area_record_dispatches(world: &Simulation, rules: &RuleSet, event: &EntityDamageEvent) -> bool {
    world
        .substrate
        .entities
        .get(event.target_id)
        .is_some_and(|target| {
            target.lifecycle.object_alive
                && !(target.category == EntityCategory::Structure
                    && rules
                        .object(world.interner.resolve(target.type_ref()))
                        .is_some_and(|object| object.invisible_in_game))
                && target.health.current > 0
                && target.lifecycle.cell_marked
                && !target.lifecycle.in_limbo
        })
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
        // Terrain71BABF passes its retained Object Location, not a fresh
        // sample of the ground/deck after nested callbacks.
        let impact = world.production.terrain_objects[&lethal.stable_id].world_coord();
        let (nested, mut pings) = apply_area_damage_in_run(
            world,
            run,
            rules,
            overlay_registry,
            impact,
            100,
            &c4_warhead,
            (RAD_NO_ATTACKER, None, c4_id),
        );
        effects.append(nested);
        under_attack_events.append(&mut pings);
    }

    let finalized = crate::sim::terrain_object::finalize_terrain_lethal(
        crate::sim::terrain_object::production_authority_parts(
            &mut world.production,
            &mut world.substrate.raw_cell_occupation,
        ),
        &mut run.finalizing_terrain,
        &mut run.navigation_changed_cells,
        lethal,
        world.resolved_terrain.as_mut(),
    );
    if finalized {
        // Terrain's common tail 0x71BB2C -> ObjectUnInit 0x5F65F0 retires Logic membership before
        // returning to the current receiver walk; physical deletion is deferred.
        // Native executable comparison: terrain_debris_receiver.json.
        let retired = world.retire_non_entity_object(lethal.stable_id);
        debug_assert!(retired);
    }
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
        if !area_isolation_dispatches(world, event, near_center_ic_isolation, current_tick) {
            continue;
        }
        let target_id = event.target_id;
        let attacker_id = event.attacker_id;
        run.selected_units.retain(|&id| id != target_id);
        if world
            .substrate
            .entities
            .get(target_id)
            .is_some_and(|target| {
                target.category == EntityCategory::Unit
                    && target.selected
                    && world.session.current_house == Some(target.owner())
            })
        {
            run.selected_units.push(target_id);
        }
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
        // Building4422F9..44234A copies its sparse contacts before calling
        // Techno442425. Destroy/Stun can remove the live links before the
        // concrete NowDead loop442511 resumes, so this per-receiver snapshot
        // must survive the synchronous common death effects.
        let building_contacts = if callbacks_enabled(world) {
            world
                .substrate
                .entities
                .get(target_id)
                .filter(|target| target.category == EntityCategory::Structure)
                .map(|target| target.radio_contacts.iter_live().collect::<Vec<_>>())
        } else {
            None
        };
        // FootClass::ReceiveDamage 0x004D7330..0x004D7413 runs its parasite
        // prefix on the raw damage before TechnoClass::ReceiveDamage.
        world.foot_receive_damage_parasite_prefix(
            target_id,
            (attacker_id != RAD_NO_ATTACKER).then_some(attacker_id),
            event.damage,
            event.warhead_ref,
            rules,
        );
        // ReceiveDamage carries sourceHouse separately from the source object.
        // Records snapshot it at detonation; a record without one falls back
        // to the live source's owner. Periodic radiation supplies both null
        // source object and null source house explicitly.
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
        let receiver_outcome = resolve_receive_damage(
            event,
            &world.substrate.entities,
            rules,
            &world.interner,
            &world.houses,
            &world.house_alliances,
            scenario_no_damage,
            current_tick,
            world.resolved_terrain.as_ref(),
        );
        if let Some(effect) = receiver_outcome.and_then(|resolved| resolved.invulnerability_impact)
        {
            death.combat_light_requests.push(effect);
        }
        // `0x00701D71..0x00701D9B`: a Psychedelic hit that turns its target
        // berserk takes it out of its team, with its idle order, before the
        // receiver clears its target and queues Hunt. Here the removal runs
        // before the receiver's berserk timer and flag writes, which it does
        // not read.
        let starts_berserk = receiver_outcome
            .and_then(|resolved| resolved.outcome.psychedelic_value)
            .is_some()
            && world
                .substrate
                .entities
                .get(target_id)
                .is_some_and(|target| !target.berserk.active);
        if starts_berserk {
            world.leave_team(target_id, false, Some(rules));
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
            world.session.binary_frame,
        )
        else {
            continue;
        };

        // Techno ReceiveDamage702049 selects result 2's arm702695. The
        // percent draw precedes House50B6F0; accepted lists spend one raw
        // Main draw even for a singleton. This belongs before common7027F7,
        // Uncloak70281D, smoke and retaliation, not at app sound publication.
        // Original executed controls: input_oracle/unit_voice_playback.
        if receive_state == damage::DamageState::Yellow
            && let Some(target) = world.substrate.entities.get(target_id)
            && let Some(object) = rules.object(world.interner.resolve(target.type_ref()))
            && !object.voice_feedback.is_empty()
            && world.main_rng.next_range_i32_inclusive(0, 99) < 0x1E
            && world.house_is_human_player(target.owner())
        {
            let index = (world.main_rng.next_u32() % object.voice_feedback.len() as u32) as usize;
            // Virtual+48 is Object/Building GetCoords, not a cell centre or
            // the separate navigation+4C getter. Reuse the coordinate owner;
            // pack only this presentation receipt, never move the actor.
            let coord = crate::sim::movement::ground_pose::object_get_coords(
                target,
                world.resolved_terrain.as_ref(),
            );
            let mut position = target.position;
            crate::sim::movement::ground_pose::put_location(&mut position, coord);
            if sound_enabled {
                world.sound_events.push(SimSoundEvent::voc_at(
                    object.voice_feedback[index].clone(),
                    &position,
                ));
            }
        }

        // gamemd-derived: `TechnoClass__ReceiveDamage @
        // 0x007027AE..0x007027EE` invokes the protected-Techno response after
        // ObjectClass has committed health/visual state and before the dead
        // branch. `ShouldProtect+0x3CF` has no active YR writer; `ToProtect=`
        // is the exact live gate retained here.
        let protected_techno_response = attacker_id != RAD_NO_ATTACKER
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
            // `TechnoClass::ReceiveDamage @ 0x0070281D` invokes virtual
            // `+0xFC` (`0x00703850`, a plain `StartUncloaking(0)` wrapper) for
            // every damage result that is not NowDead — the heal case
            // included, because the `if (damage < 0) return` early-out sits
            // AFTER this call. Argument zero, so the transition owns a
            // `CloakSound`.
            let surfaced = world
                .substrate
                .entities
                .get_mut(target_id)
                .and_then(|target| target.cloak.as_mut())
                .map(|cloak| {
                    cloak.start_uncloaking(
                        now,
                        cloaking_speed,
                        rules.general.cloaking_stages,
                        false,
                    )
                });
            if surfaced.is_some_and(|result| result.play_sound) && sound_enabled {
                emit_configured_cloak_sound(world, target_id, rules);
            }
        }

        if reached_exact_zero && postmortem_candidate.is_none() {
            world.begin_receiver_kill_record(target_id);
        }
        // ObjectClass routes its kill callback while Health is exactly zero,
        // before Destroy's reference notification and before TechnoClass's
        // victim-house anger callback. `Record_The_Kill` awards the killer's
        // experience in the same call, so it is a same-tick write the victim's
        // own death effects can already observe. Deferring it to the lifecycle
        // release point would change that visibility.
        if reached_exact_zero {
            world.record_the_kill(
                target_id,
                (attacker_id != RAD_NO_ATTACKER).then_some(attacker_id),
                attacker_owner,
                super::KillCallback::Terminal,
                rules,
            );
        }
        if reached_exact_zero && postmortem_candidate.is_none() {
            // Techno702FF0 awards experience before the score/loss writes
            // (703003..7031DC); Object5F57AF's Destroy observes all of them.
            // Keep the separately owned PostMortem bookkeeping below intact.
            world.record_destruction_once(target_id);
        }
        // ObjectClass::ReceiveDamage 0x005F5765..0x005F57AF: after the kill
        // callback the exact-zero arm runs Destroy = Detach_All(1), so every
        // listener drops the dying object at the killing hit, before the
        // anger callback and TechnoClass's death arm. The CausesDelayKill
        // PostMortem stage below runs the same callback after its bookkeeping.
        if reached_exact_zero && postmortem_candidate.is_none() && callbacks_enabled(world) {
            world.object_destroy_callback(
                target_id,
                crate::sim::world::UninitContext::new(Some(rules), overlay_registry),
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
                    crate::sim::world::UninitContext::with_rules(rules)
                        .with_registry(overlay_registry),
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
            // shared RecordTheKill receiver before the PostMortem Destroy hook. Native retains no killer on the
            // restored object: a fresh null-source timer expiry must stay
            // uncredited, while a later sourced lethal hit captures anew.
            target.killed_by = None;
            target.destruction_recorded = false;
            let replace = target
                .pending_c4_detonation
                .is_none_or(|pending| duration_frames < pending.timer.remaining(current_frame));
            if replace {
                let retained_source = target
                    .pending_c4_detonation
                    .and_then(|pending| pending.source_entity_id);
                target.pending_c4_detonation = Some(crate::sim::components::PendingC4Detonation {
                    timer: crate::sim::timer::CdTimer::started(current_frame, duration_frames),
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
                    crate::sim::world::UninitContext::with_rules(rules)
                        .with_registry(overlay_registry),
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
                    .assign_infantry_walk_destination(
                        target_id,
                        crate::sim::components::NavTargetRef::cell(
                            scatter.destination.0,
                            scatter.destination.1,
                        ),
                        scatter.speed,
                        rules,
                        overlay_registry,
                    )
                    .unwrap_or_else(|cause| {
                        panic!("damage Scatter destination for {target_id}: {cause}")
                    });
                // A Teleport man's setter is represented: a false answer is its
                // Move_To's refusal (`0x0071820F`), which stands.
                let teleport = world
                    .substrate
                    .entities
                    .get(target_id)
                    .and_then(|target| target.locomotor.as_ref())
                    .is_some_and(|locomotor| {
                        locomotor.active_kind()
                            == crate::rules::locomotor_type::LocomotorKind::Teleport
                    });
                if !walk && !teleport {
                    // A Jumpjet man: Infantry setter `0x0051AA40`, whose Foot
                    // tail (`0x004D94B0`: NavCom, Jumpjet Move_To, timer tail)
                    // is `issue_air_cell_destination`. RESIDUAL: its JumpJet
                    // arms before the tail (same-cell return, vt+0x500 stop
                    // while moving, Walk piggyback switch) are unported.
                    // Trigger: a damaged Jumpjet infantryman that scatters;
                    // retail JUMPJET is Fearless=yes, so none with retail data.
                    world.issue_air_cell_destination(
                        target_id,
                        scatter.destination,
                        scatter.speed,
                        Some(rules),
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
            // `UnitClass::ReceiveDamage @ 0x00737C90`: `0x00737D69 CMP EAX,4`
            // sends result 4 to the death branch (`+0x3B8`
            // `Death_Announcement`, `Death_Explosion`); the `Harvester=` ping
            // (`0x007384B9..0x00738530`) sits only in the `result != 4` arm,
            // on any non-zero result, with or without a source. A building's
            // ping is its retaliation block's, below.
            if damage > 0
                && !became_fatal
                && target.category != EntityCategory::Structure
                && rules
                    .object(world.interner.resolve(target.type_ref()))
                    .is_some_and(|obj| obj.harvester)
            {
                under_attack_events.push(UnderAttackEvent {
                    rx: target.position.rx,
                    ry: target.position.ry,
                    owner: target.owner(),
                    miner: true,
                    structure: false,
                });
            }
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
            if combat_targeting::should_retaliate(world, rules, target_id, attacker_id)
                && retaliation_reaches(world, rules, target_id, attacker_id)
            {
                world.override_mission_on_damage_response(target_id, attacker_id, rules);
            }
            // RESIDUAL — ReceiveDamage's scatter tail (`0x00702B47..0x00702D0B`)
            // is not ported. Past the Override, a Foot with no Target and no
            // NavCom scatters when `[CombatDamage] PlayerScatter=`
            // (`Rules+0x17ED`) is set or its rank holds the SCATTER ability; the
            // refused arm (`0x00702BFE`) has its own gate on the mission's
            // `Scatter=`. Scatter (vt+0x174, `InfantryClass::Scatter @
            // 0x0051D0D0`) draws `RandomRanged` on the Scenario RNG
            // (`0x0051D2BA`). Effect: native steps a hit infantryman aside and
            // makes one Scenario draw VERA does not make, so the RNG stream
            // diverges from the first such hit. The next combat increment.
        }

        if entered_techno_death {
            {
                if callbacks_enabled(world) {
                    world.apply_fatal_lifecycle_stage(
                        rules,
                        FatalLifecycleStage::BeforeDeathEffects,
                        target_id,
                        fatal_category,
                        crate::sim::world::UninitContext::with_rules(rules)
                            .with_registry(overlay_registry),
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
                building_contacts.as_deref(),
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
                            crate::sim::world::UninitContext::with_rules(rules)
                                .with_registry(overlay_registry),
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
        // `BuildingClass::ReceiveDamage`'s retaliation block
        // (`0x00442942..0x00442A90`, [`Simulation::building_hit_response`])
        // runs after the IsAlive re-test (`0x00442905`; `+0x90` is
        // `object_alive`); result 5 returned at `0x0044247D`. Case 4 calls
        // `DestructionEffects` (slot `+0x4EC` = `0x004415F0`, which never
        // writes `+0x90`) and then, for the stock timer (`+0x530` = 8),
        // `ObjectClass::UnInit` (`0x005F65F0`, clears `+0x90` at `0x005F6625`),
        // so a killing blow skips the block. RESIDUAL: `DestructionEffects`
        // arms a zero timer for `Explodes=` types (TechnoType `+0xD15`,
        // ReadINI `0x007122BE..0x007122D2`: stock GAYARD, NAYARD, YAYARD,
        // NANRCT, AMMOCRAT, CAOILD, CAMISC01/02, YAPPPT) and for a building
        // whose current mission is Selling (0x13), leaving `+0x90` set and the
        // block live on their killing blow; VERA UnInits them at once (see
        // `crew_survival` for the second SpawnSurvivors this skips). Every
        // retail `Explodes=` type is unarmed and Selling stops the block after
        // its ping, so the missing ping is the whole effect.
        // `FootClass::ReceiveDamage @ 0x004D7442..0x004D7453`: a team member
        // whose hit returned other than none (0) or gone (5) reports it to
        // its team; a killed member has already left it.
        if !matches!(
            receive_state,
            damage::DamageState::Unaffected | damage::DamageState::AlreadyDead
        ) && world
            .substrate
            .entities
            .get(target_id)
            .is_some_and(|target| target.category != EntityCategory::Structure)
        {
            world.team_took_damage(
                target_id,
                (attacker_id != RAD_NO_ATTACKER).then_some(attacker_id),
                rules,
            );
        }
        if receive_state != damage::DamageState::AlreadyDead
            && world
                .substrate
                .entities
                .get(target_id)
                .is_some_and(|target| {
                    target.category == EntityCategory::Structure && target.lifecycle.object_alive
                })
            && let Some(ping) = world.building_hit_response(
                target_id,
                (attacker_id != RAD_NO_ATTACKER).then_some(attacker_id),
                receive_state,
                rules,
                overlay_registry,
            )
        {
            under_attack_events.push(ping);
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
    building_contacts: Option<&[u64]>,
) -> DeathEffects {
    debug_assert!(
        dead_entities.len() <= 1,
        "ReceiveDamage enters one concrete fatal postlude at a time"
    );
    let mut death_sounds: Vec<(String, u16, u16)> = Vec::new();
    let mut explosion_effects: Vec<ExplosionEffect> = Vec::new();
    let mut voxel_debris: Vec<crate::sim::voxel_anim::VoxelDebrisSpawn> = Vec::new();
    let mut under_attack_events: Vec<UnderAttackEvent> = Vec::new();
    let mut structure_destroyed: bool = false;
    // What the death weapon and the bomb set off, after this arm's own effects.
    let mut blast_effects = DeathEffects::default();
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
            let world_z_leptons = object_world_z_leptons(e, world.resolved_terrain.as_ref());
            (
                e.type_ref(),
                e.position.rx,
                e.position.ry,
                world_z_leptons,
                e.owner(),
                e.category,
                e.veterancy(),
            )
        });

        if let Some((type_id, rx, ry, world_z_leptons, owner, category, veterancy)) = dead_info {
            // `0x00702050..0x00702065`: the death arm first frees a master's
            // slaves to the killing hit's source (FreeSlaves, no house).
            if callbacks_enabled(world) {
                let killer = damage_events
                    .iter()
                    .rfind(|event| event.target_id == dead_id)
                    .map(|event| event.attacker_id)
                    .filter(|&attacker| attacker != RAD_NO_ATTACKER);
                world.free_slaves(dead_id, killer, None, rules, overlay_registry);
            }
            // `0x00702112`: the death arm frees a controller's captives before
            // its death sounds (their fate draws precede the debris draws).
            if callbacks_enabled(world) {
                world.free_all_captures(dead_id, rules, overlay_registry);
            }
            let type_id_str = world.interner.resolve(type_id);
            if let Some(obj) = rules.object(type_id_str) {
                append_selected_death_sounds(
                    obj,
                    category,
                    rules.general.building_die_sound.as_deref(),
                    world.houses.get(&owner).is_some_and(|house| house.is_human),
                    &mut world.main_rng,
                    rx,
                    ry,
                    &mut death_sounds,
                );
                // 0x00702206..0x00702210: RADIO OVER_OUT to every contact and
                // the Stun, between the death sounds and the debris.
                if callbacks_enabled(world) {
                    world.techno_death_stun(
                        dead_id,
                        crate::sim::world::UninitContext::new(Some(rules), overlay_registry),
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
                // The pieces start from the dying object's GetCoords
                // (`vtable+0x48` at `0x007024FC`): a building's foundation
                // centre (`BuildingClass::GetCoords @ 0x00447AC0`).
                let center = world
                    .substrate
                    .entities
                    .get(dead_id)
                    .map_or((0, 0), |entity| {
                        let [x, y] = crate::sim::movement::ground_pose::object_center_xy(entity);
                        (x, y)
                    });
                throw_debris_for_death(
                    world,
                    obj,
                    rules,
                    owner,
                    glam::IVec3::new(center.0, center.1, world_z_leptons),
                    &mut voxel_debris,
                    &mut explosion_effects,
                );
                // An exploding object's passengers die with it
                // (`0x00702603..0x00702667`, FootClass::KillPassengers' loop
                // inlined, credited to the killing hit's source) before its
                // death weapon. A unit that does not explode lets them escape
                // after this arm (`finish_concrete_death`).
                let numbered_weapon = world.substrate.entities.get(dead_id).and_then(|entity| {
                    super::combat_weapon::get_weapon(
                        entity,
                        obj,
                        super::combat_weapon::attacker_facts(entity, obj).current_weapon_number,
                        &world.substrate.entities,
                        rules,
                        &world.interner,
                    )
                });
                let explodes = death_arm_explodes(obj, veterancy, numbered_weapon);
                if explodes && callbacks_enabled(world) {
                    let attacker = killing_attacker(world, damage_events, dead_id);
                    world.kill_passengers(dead_id, attacker, rules, overlay_registry);
                }
                // `Fire_Death_Weapon(0)` (`0x0070266D`): its bullet
                // detonates here, inside this ReceiveDamage, through the
                // DetonateAtCoord every bullet takes. An IvanBomb warhead (the
                // Crazy Ivan's own bomber) plants a bomb on the dying object
                // instead of damaging (`0x00469343`), which goes off below.
                if explodes
                    && let Some(detonation) = super::death_weapon_detonation(world, dead_id, rules)
                {
                    let commit =
                        commit_projectiles(world, run, &[detonation], rules, overlay_registry);
                    world.admit_projectile_spawns(commit.projectile_spawns, rules);
                    under_attack_events.extend(commit.under_attack_events);
                    blast_effects.append(commit.effects);
                }
            }
            // `0x0070267F`: after its death weapon, the bomb it carries goes
            // off (`BombClass::Detonate @ 0x00438720`).
            if let Some((effects, pings)) =
                world.bomb_detonate_in_run(run, dead_id, rules, overlay_registry)
            {
                under_attack_events.extend(pings);
                blast_effects.append(effects);
            }

            // The world fatal prelude already owns garrison ejection before
            // the nested death weapon. Callback-disabled receiver fixtures
            // leave the cargo attached for their UnInit assertions.
        }
    }

    let mut effects = DeathEffects {
        structure_destroyed,
        explosion_effects,
        voxel_debris,
        death_sounds,
        under_attack_events,
        ..DeathEffects::default()
    };
    effects.append(blast_effects);

    // Concrete receivers resume after Techno's nested DeathWeapon. Read the
    // retained entity's current state at that boundary.
    for &dead_id in dead_entities {
        // Building442425 returns from Techno701900 before dispatching
        // NowDead4 to4424A2/442511. Its saved-contact RUN_AWAY loop follows
        // Destroy's BREAK/expiry and Techno's sounds, Stun, debris and nested
        // death weapon; Building DestructionEffects442665 follows it.
        // Techno702035 also forces result4 on an admitted Health0 re-entry. A
        // nested effect that cleared ObjectAlive skips the concrete wrapper
        // at44242C, and PostMortem never reaches this result4 continuation.
        if callbacks_enabled(world)
            && let Some(contacts) = building_contacts
            && world
                .substrate
                .entities
                .get(dead_id)
                .is_some_and(|building| {
                    building.category == EntityCategory::Structure
                        && building.lifecycle.object_alive
                })
        {
            world.building_now_dead_contacts(dead_id, contacts, Some(rules));
        }
        finish_concrete_death(
            world,
            dead_id,
            run.selected_units.contains(&dead_id),
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
/// `selected_by_player` is the dying unit's local-player selection on
/// receiver entry ([`ReceiverRun::selected_units`]), which the kill's Destroy
/// callback has since cleared.
fn finish_concrete_death(
    world: &mut Simulation,
    dead_id: u64,
    selected_by_player: bool,
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
    let (type_id, owner, rx, ry, has_animation) = (
        entity.type_ref(),
        entity.owner(),
        entity.position.rx,
        entity.position.ry,
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
    // The killing call's concrete receiver booleans: IgnoreDefenses (arg5)
    // becomes the building's NoSurvivor and kills a unit's passengers, arg6
    // gates the vehicle crew.
    let (ignore_defenses, prevent_crew_escape) = damage_events
        .iter()
        .rfind(|event| event.target_id == dead_id)
        .map(|event| event.receiver_flags)
        .map_or((false, false), |flags| (flags.ignore_defenses, flags.arg6));

    // BuildingClass runs DestructionEffects/SpawnSurvivors only after
    // TechnoClass's synchronous death weapon has returned. Capture the
    // immutable plan now; placement and all RNG stay at that postlude.
    if category == EntityCategory::Structure {
        concrete_smudge_plans.push(ConcreteDeathSmudgePlan::Building);
    }

    // `UnitClass::Death_Explosion @ 0x00738680` unless the unit sinks
    // (`0x00737DE2`), and the Aircraft death arm (`0x0041661F`); the killing
    // detonation's own impact anim follows its receivers. Infantry play none:
    // every `Explosion=` reader is a Unit, Aircraft or Building body
    // (`get_xrefs_to 0x00738680`; the `type+0x73C` operand scan), and
    // infantry death anims come from `InfDeath`/`DeathAnims`.
    match category {
        EntityCategory::Unit => {
            if world.unit_sinks_on_death(rules, dead_id) {
                world.begin_ship_sinking(dead_id, rules);
            } else {
                world.unit_death_explosion(rules, dead_id, &mut effects.explosion_effects);
            }
        }
        EntityCategory::Aircraft => {
            world.aircraft_death_explosion(rules, dead_id, &mut effects.explosion_effects)
        }
        _ => {}
    }
    // `UnitClass::ReceiveDamage` then lifts the dying unit off its cell
    // (vt+0x124 Mark(UP) at `0x00737F7A`) before its passengers and crew
    // leave. The UnInit that follows finds it already unmarked. Mark leaves
    // the AircraftTracker alone: a falling Jumpjet wreck stays in it until its
    // impact (`0x0054D075`), any other dying unit until its UnInit.
    let crashable = category == EntityCategory::Unit
        && world
            .object_type(type_id, rules)
            .is_some_and(|object| object.crashable);
    if category == EntityCategory::Unit && callbacks_enabled(world) {
        world.foot_mark_remove(dead_id, Some(rules), overlay_registry);
        let dying = crate::sim::crew_survival::DyingTransport {
            attacker: killing_attacker(world, damage_events, dead_id),
            ignore_defenses,
            selected_by_player,
        };
        world.release_dying_unit_passengers(rules, overlay_registry, dead_id, dying);
        world.spawn_vehicle_crew(
            rules,
            overlay_registry,
            dead_id,
            prevent_crew_escape,
            selected_by_player,
        );
    }

    let inf_death = killing_warhead.as_ref().map_or(1, |(wh, _)| wh.inf_death);
    if category == EntityCategory::Infantry {
        // `InfantryClass::ReceiveDamage` on result 4 (`0x00518077..0x0051808E`):
        // a dying slave leaves its master's node (RemoveSlave).
        world.remove_slave(dead_id);
        // The house InfDeath 8 and 9 give their anim (`0x0051887B`,
        // `0x00518A96`): the source's owner (`+0x21C`), else the house
        // argument.
        let anim_house = killing_attacker(world, damage_events, dead_id)
            .and_then(|attacker| world.substrate.entities.get(attacker))
            .map(|attacker| attacker.owner())
            .or_else(|| {
                damage_events
                    .iter()
                    .rfind(|event| event.target_id == dead_id)
                    .and_then(|event| event.source_house)
            });
        let postlude = world.begin_infantry_receiver_death(
            dead_id,
            inf_death,
            anim_house,
            rules,
            overlay_registry,
            &mut effects.immediate_uninit_ids,
        );
        concrete_smudge_plans.push(ConcreteDeathSmudgePlan::Infantry(postlude));
        effects.despawned_ids.push(dead_id);
    } else if has_animation
        && !world
            .substrate
            .entities
            .get(dead_id)
            .is_some_and(|entity| entity.sinking.is_active())
    {
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
    } else if category == EntityCategory::Aircraft
        && callbacks_enabled(world)
        && world.foot_crash(
            dead_id,
            killing_attacker(world, damage_events, dead_id),
            rules,
            overlay_registry,
        )
    {
        // `AircraftClass::ReceiveDamage` (`0x00416694..0x004166A3`): an
        // airborne aircraft crashes instead of its UnInit. It stays alive and
        // represented, with Health 0, until its fall's impact.
        effects.despawned_ids.push(dead_id);
    } else if crashable
        && callbacks_enabled(world)
        && world.foot_crash(dead_id, None, rules, overlay_registry)
    {
        // `UnitClass::ReceiveDamage` (`0x00738457..0x00738475`): an airborne
        // `Crashable=` unit crashes (`Crash(0)`, no attacker) instead of its
        // UnInit, and falls to its impact (a Jumpjet: `jumpjet_crash_impact`).
        effects.despawned_ids.push(dead_id);
    } else if category == EntityCategory::Unit
        && world
            .substrate
            .entities
            .get(dead_id)
            .is_some_and(|entity| entity.sinking.is_active())
    {
        // Unit738493 checks +3CD after Mark(UP), passenger/crew handling and
        // Crashable. A sinking hull retains Alive and its Logic/display slot.
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
                postlude.commit(world, rules, effects);
            }
            ConcreteDeathSmudgePlan::Building => {
                // DestructionEffects (`0x004415F0`): the building's own anims
                // and centre mark, then SpawnSurvivors (`0x00441F1B`), which
                // interleaves each foundation cell's survivor roll with that
                // cell's own mark.
                let deferred = &mut effects.smudge_spawn_requests;
                world.building_destruction_anims(
                    rules,
                    dead_id,
                    &mut effects.explosion_effects,
                    |world, request| {
                        commit_smudges(world, rules, overlay_registry, vec![request], deferred);
                    },
                );
                if callbacks_enabled(world) {
                    let deferred = &mut effects.smudge_spawn_requests;
                    world.spawn_building_survivors(
                        rules,
                        overlay_registry,
                        dead_id,
                        ignore_defenses,
                        |world, (cell_rx, cell_ry)| {
                            commit_smudges(
                                world,
                                rules,
                                overlay_registry,
                                vec![SmudgeSpawnRequest::BuildingSurvivor { cell_rx, cell_ry }],
                                deferred,
                            );
                        },
                    );
                }
            }
        }
    }
}

/// The killing ReceiveDamage call's attacker (its fourth argument), when it
/// names an object.
fn killing_attacker(
    world: &Simulation,
    damage_events: &[EntityDamageEvent],
    dead_id: u64,
) -> Option<u64> {
    damage_events
        .iter()
        .rfind(|event| event.target_id == dead_id)
        .map(|event| event.attacker_id)
        .filter(|&attacker| {
            attacker != crate::sim::combat::RAD_NO_ATTACKER
                && world.substrate.entities.contains(attacker)
        })
}

/// What a special arm of `BulletClass::DetonateAtCoord` reads as its target:
/// the bullet's `+0x10C`, which is an object, a cell (a real one or the dummy
/// cell) or nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpecialArmTarget {
    Object(u64),
    Cell,
    None,
}

/// The special arm of `BulletClass::DetonateAtCoord @ 0x004690B0`
/// (`0x0046920B..0x00469A3D`) the warhead's flags selected, for both
/// deliveries: a visible bullet's detonation, and VERA's immediate `Inviso=`
/// shot, which natively is the same bullet detonating through the same chain.
///
/// Whatever the arm does, it claims the impact: every arm, its internal
/// refusals included, leaves by `JMP 0x00469AA4`, and `Apply_area_damage`
/// (`0x00469A83`) and `SpawnShrapnel` are reachable only from the final else
/// at `0x00469A3F`. So the caller runs neither, and runs the shared tail below
/// `LAB_00469AA4` after this returns. `owner` is the bullet's `+0xB0`.
///
/// RESIDUAL: the ElectricAssault (`0x0046937A`), IsLocomotor (`0x004694CB`),
/// Airstrike (`0x00469705`), DirectRocker (`0x0046978E`) and MakesDisguise
/// (`0x00469A03`) bodies are not ported; those arms claim the impact and do
/// nothing else (see `SpecialDetonationAction`).
fn run_special_detonation_arm(
    world: &mut Simulation,
    rules: &RuleSet,
    action: SpecialDetonationAction,
    detonation: &ProjectileDetonation,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
) {
    let owner = detonation.source_id;
    let target = match detonation.target {
        ProjectileTarget::Entity(id) => SpecialArmTarget::Object(id),
        ProjectileTarget::Cell { .. } | ProjectileTarget::DummyCell => SpecialArmTarget::Cell,
        ProjectileTarget::None => SpecialArmTarget::None,
    };
    let object = match target {
        SpecialArmTarget::Object(id) => Some(id),
        SpecialArmTarget::Cell | SpecialArmTarget::None => None,
    };
    match action {
        SpecialDetonationAction::OrdinaryDamage => {
            debug_assert!(false, "the ordinary arm is its caller's");
        }
        SpecialDetonationAction::Parasite => {
            // 0x004693DD..0x0046941E: no bullet Owner (+0xB0) -> nothing;
            // otherwise Owner->ParasiteImUsing->AttachTo(Target if FootClass).
            // The impact point is not consulted.
            if world.substrate.entities.contains(owner) {
                let victim = object.filter(|&id| {
                    world.substrate.entities.get(id).is_some_and(|target| {
                        matches!(
                            target.category,
                            EntityCategory::Unit
                                | EntityCategory::Infantry
                                | EntityCategory::Aircraft
                        )
                    })
                });
                world.parasite_attach(owner, victim, rules);
            }
        }
        SpecialDetonationAction::MindControl => {
            world.mind_control_detonation(owner, object, rules, overlay_registry);
        }
        SpecialDetonationAction::Temporal => {
            let target = match target {
                SpecialArmTarget::Object(id) => {
                    crate::sim::temporal::TemporalShotTarget::Object(id)
                }
                SpecialArmTarget::Cell => crate::sim::temporal::TemporalShotTarget::Cell,
                SpecialArmTarget::None => crate::sim::temporal::TemporalShotTarget::None,
            };
            world.temporal_detonation(owner, target, rules, overlay_registry);
        }
        SpecialDetonationAction::IvanBomb => {
            // 0x00469343..0x00469375: the bullet's owner plants on a Techno.
            world.bomb_attach(owner, object, rules);
        }
        SpecialDetonationAction::BombDisarm => {
            // 0x004699C4..0x004699FE: a bombed Object target is defused.
            if let Some(id) = object {
                world.bomb_defuse(id);
            }
        }
        SpecialDetonationAction::NukeMaker => nuke_maker(world, rules, detonation),
        SpecialDetonationAction::ElectricAssault
        | SpecialDetonationAction::Locomotor
        | SpecialDetonationAction::Airstrike
        | SpecialDetonationAction::DirectRocker
        | SpecialDetonationAction::MakesDisguise => {
            log::debug!(
                "special detonation {action:?} from {owner} claimed with its body unported; \
                 shrapnel and area damage suppressed, the shared tail still runs"
            );
        }
    }
}

/// `BulletClass::NukeMaker @ 0x0046B310` (Ghidra `SpawnDownwardNuke`), the
/// arm of a `NukeMaker=` warhead (`0x00469A2C`): the nuclear missile's
/// falling warhead. A bullet of the `NukePayload` weapon (the literal at
/// `0x0081AFA0`, `0x0046B371`) is built with that weapon's BulletType,
/// damage, warhead and speed (`BulletClass::Construct @ 0x004664C0`), the
/// detonating bullet's target and owner, bright, and given the weapon
/// (`+0x130`). It falls from the GetCoords of the cell under the target's
/// GetCoords (`0x0046B32E..0x0046B36C`), raised by the detonating weapon's
/// BulletType's `DetonationAltitude=` (`+0x2BC`, `0x0046B459`), straight
/// down ([`missile_launch_velocity`] at -1.0).
///
/// Native dereferences a missing target, NukePayload weapon or warhead; VERA
/// drops nothing then.
///
/// [`missile_launch_velocity`]: crate::sim::projectile::launch::missile_launch_velocity
fn nuke_maker(world: &mut Simulation, rules: &RuleSet, detonation: &ProjectileDetonation) {
    /// `[0x007E4900]` = -1.0.
    const FALL_SCALE: NativeF64Bits = NativeF64Bits::from_bits(0xbff0_0000_0000_0000);
    let terrain = world.resolved_terrain.as_ref();
    let target_coords = match detonation.target {
        ProjectileTarget::Entity(id) => object_get_coords(world, id),
        ProjectileTarget::Cell { rx, ry } => {
            Some(crate::sim::projectile::cell_ground_coord(terrain, rx, ry))
        }
        ProjectileTarget::DummyCell => Some(crate::sim::projectile::dummy_cell_ground_coord(
            &world.effective_shared_cell_dummy(),
        )),
        ProjectileTarget::None => None,
    };
    let altitude = rules
        .weapon(world.interner.resolve(detonation.payload.weapon))
        .and_then(|weapon| weapon.projectile.as_deref())
        .and_then(|projectile| rules.projectile(projectile))
        .map_or(0, |projectile| projectile.detonation_altitude);
    let weapon = rules.weapon(NUKE_PAYLOAD_WEAPON);
    let warhead = weapon
        .and_then(|weapon| weapon.warhead.as_deref())
        .and_then(|name| rules.warhead(name));
    let (Some(target_coords), Some(weapon), Some(warhead)) = (target_coords, weapon, warhead)
    else {
        return;
    };
    let (rx, ry) = (
        (target_coords.x / 256) as u16,
        (target_coords.y / 256) as u16,
    );
    let ground = crate::sim::projectile::cell_ground_coord(terrain, rx, ry);
    let origin = ProjectileCoord::new(ground.x, ground.y, ground.z.wrapping_add(altitude));
    let bullet_id = world.allocate_stable_id();
    let native_unique_id = world.next_native_runtime_id();
    let velocity = crate::sim::projectile::launch::missile_launch_velocity(FALL_SCALE);
    let payload = ProjectilePayload::new(
        weapon.damage,
        world.interner.intern(&warhead.id),
        world.interner.intern(&weapon.id),
    );
    if let Some(bullet) = super::FiredBullet::launched(
        world,
        native_unique_id,
        detonation.source_id,
        detonation.target,
        payload,
        weapon.speed,
        origin,
        velocity,
    ) {
        super::admit_fired_bullet(world, rules, weapon, bullet_id, bullet);
    }
}

/// The weapon NukeMaker drops, named by the literal `0x0081AFA0`.
const NUKE_PAYLOAD_WEAPON: &str = "NukePayload";

/// Snapshot the native primary-cell wall branch before its receiver collection
/// mutates the overlay. Its return skips the bridge tail even if the wall dies.
pub(crate) fn area_routes_to_wall(
    world: &Simulation,
    overlay_registry: Option<&OverlayTypeRegistry>,
    cell: (u16, u16),
    warhead: &WarheadType,
) -> bool {
    wall_overlay_flags_at(
        world.overlay_grid.as_ref(),
        overlay_registry,
        cell.0,
        cell.1,
    )
    .is_some_and(|flags| warhead_damages_wall(warhead, flags))
}

/// Apply_area_damage's bridge continuation489E87..48A2C4 runs after all
/// ordinary receivers and their recursive deaths, before returning to the
/// caller. In particular Bullet469033 completes it before cluster RNG469057.
/// A negative nonzero packet still reaches the native strength draw.
#[allow(clippy::too_many_arguments)]
pub(crate) fn continue_area_bridge_damage(
    world: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    cell: (u16, u16),
    damage: i32,
    warhead_ref: InternedId,
    impact_z_leptons: i32,
    routed_wall: bool,
    area_result: AreaDamageResult,
) -> bool {
    if area_result == AreaDamageResult::IronCurtain
        || world.session.no_damage
        || damage == 0
        || routed_wall
        || !rules
            .warhead(world.interner.resolve(warhead_ref))
            .is_some_and(|warhead| warhead.wall)
    {
        return false;
    }
    let event = BridgeDamageEvent {
        rx: cell.0,
        ry: cell.1,
        damage,
        warhead_ref,
        is_ion_cannon: warhead_ref
            == world
                .rule_handles
                .expect("Simulation::resolve_type_handles must run before combat")
                .ion_cannon,
        impact_z_leptons,
    };
    crate::sim::world::bridge_orchestrator::apply_bridge_damage_events_with_overlay_registry(
        world,
        rules,
        std::slice::from_ref(&event),
        overlay_registry,
    )
}

/// `BulletClass::DetonateAtCoord @ 0x004690B0` up to its receivers: the
/// radiation site, then the special-warhead chain or, in its final else, the
/// shrapnel (`0x00469A51`) and `Apply_area_damage` (`0x00489280`) records,
/// then bridge damage. Returns the ordinary area's wall-route decision, or
/// None for a special arm without Apply_area_damage. The receivers and bridge
/// continuation commit before the anim tail
/// ([`emit_detonation_anim`]), as native's area damage returns before
/// `LAB_00469AA4`.
///
/// The area damage's house (`0x00469A5C..0x00469A75`) is the live bullet
/// owner's: a bullet whose firer has expired (`BulletClass::PointerExpired
/// @ 0x004684E0` nulled `+0xB0`) damages with no house.
fn emit_detonation_receivers(
    world: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    detonation: &ProjectileDetonation,
    warhead: &WarheadType,
    out: &mut CombatEmit,
) -> Option<bool> {
    let (impact_rx, impact_ry, ..) = projectile_impact_cell(detonation.impact);

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
                spread: super::cell_spread::whole_cells(warhead.cell_spread_f64),
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
    let special_action =
        projectile_special_detonation_action(SpecialDetonationFlags::of(warhead), special_target);

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
                bullet_location(world, detonation).0,
                &world.substrate.entities,
                &world.substrate.occupancy,
                &world.production.terrain_object_cells,
                rules,
                &mut world.interner,
                world.resolved_terrain.as_ref(),
                &world.house_alliances,
                &mut world.scenario_rng,
                &mut world.native_unique_ids,
                out,
            );

            // `0x00469A69..0x00469A75`: the bullet's live Owner's house, or
            // none once the owner is gone (`BulletClass+0xB0` is detached).
            let source_house = world
                .substrate
                .entities
                .get(detonation.source_id)
                .map(|source| source.owner());
            let area = collect_area_at(
                world,
                rules,
                overlay_registry,
                detonation.impact,
                detonation.payload.area_damage(),
                warhead,
                (
                    detonation.source_id,
                    source_house,
                    detonation.payload.warhead,
                ),
            );
            append_fixture_tiberium(world, &mut out.effects.tiberium_reduction_requests);
            #[cfg(test)]
            out.effects.wall_mutations.extend(area.aoe.wall_mutations);

            #[cfg(test)]
            out.effects
                .cell_target_detaches
                .extend(area.aoe.cell_target_detaches);
            out.damage_events.extend(area.aoe.receivers);

            Some(area.routed_wall)
        }
        claimed => {
            run_special_detonation_arm(world, rules, claimed, detonation, overlay_registry);
            None
        }
    }
}

/// The detonating bullet's Location (`+0x9C`) and OnBridge, which
/// SpawnShrapnel and the anim tail read. The impact ladder
/// (`ResolveImpactCoordAndDetonate @ 0x00468D80`) hands DetonateAtCoord a
/// resolved coordinate but never moves the bullet. A death weapon's bullet is
/// never placed: Fire_Death_Weapon creates it, Limbos it and detonates it at
/// its firer's GetCoords (`0x0070D756..0x0070D782`), so its Location is still
/// the constructor's zero coordinate (`0x005F3993` copies `0x00AC1380`, which
/// `0x005F38A0` zeroes), off any bridge. A detonation with no live bullet (a
/// test fixture) stands at its own coordinate.
fn bullet_location(
    world: &Simulation,
    detonation: &ProjectileDetonation,
) -> (ProjectileCoord, bool) {
    match world.projectiles.get(detonation.projectile_id) {
        Some(bullet) => (bullet.position, bullet.on_bridge),
        None if detonation.reason
            == crate::sim::projectile::ProjectileDetonationReason::DeathWeapon =>
        {
            (ProjectileCoord::new(0, 0, 0), false)
        }
        None => (detonation.impact, false),
    }
}

/// `LAB_00469AA4`, reached by the ordinary arm and every special arm after
/// their receivers: an `Inviso=` bullet first scatters the anim coordinate
/// by one raw Scenario draw (`0x0049F420`, radius 0x20; the damage keeps the
/// impact), then the AnimList anim and its smudge.
#[allow(clippy::too_many_arguments)]
fn emit_detonation_anim(
    world: &mut Simulation,
    rules: &RuleSet,
    detonation: &ProjectileDetonation,
    warhead: &WarheadType,
    inviso: bool,
    bright: bool,
    area_result: Option<AreaDamageResult>,
    out: &mut CombatEmit,
) {
    let (impact_rx, impact_ry, impact_sub_x, impact_sub_y, world_z_leptons) =
        projectile_impact_cell(detonation.impact);
    let (rx, ry, sub_x, sub_y) = if inviso {
        inviso_scatter::scatter_inviso_effect_coord(
            &mut world.scenario_rng,
            impact_rx,
            impact_ry,
            impact_sub_x,
            impact_sub_y,
        )
    } else {
        (impact_rx, impact_ry, impact_sub_x, impact_sub_y)
    };
    // 469AF0..469BCF reads the still-live Bullet, not the damage/animation
    // coordinate copied at469AA4. The terrain here includes the synchronous
    // bridge continuation, so a collapsed deck can now select SplashList.
    let (selection_coordinate, on_bridge) = bullet_location(world, detonation);
    let land = detonation_anim::bullet_land(
        world,
        rules,
        selection_coordinate,
        on_bridge,
        area_result == Some(AreaDamageResult::ReceiverDispatched),
    );
    let placement = ProjectileCoord::new(
        i32::from(rx) * 256 + sub_x.to_num::<i32>(),
        i32::from(ry) * 256 + sub_y.to_num::<i32>(),
        world_z_leptons,
    );
    // Selection still runs for an isolated Iron Curtain impact, including
    // EMEffect RNG. Only afterward469BEA replaces its constructor type.
    let selected = detonation_anim::effect(
        world,
        rules,
        warhead,
        detonation.payload.base_damage,
        land,
        selection_coordinate,
        placement,
    );
    // `0x00469BD6..0x00469C41`: a Bright bullet (`+0xE0`, the weapon's
    // `Bright=`) lights the anim coordinate with its damage (`+0x6C`),
    // force 1, and the warhead's CLDisable channels. AreaDamage result2
    // skips the light and selected ordinary animation after selection RNG.
    if bright && area_result != Some(AreaDamageResult::IronCurtain) {
        let flags = (u32::from(warhead.cl_disable_red) << 1)
            | (u32::from(warhead.cl_disable_green) << 2)
            | (u32::from(warhead.cl_disable_blue) << 3);
        out.effects.combat_light_requests.push(CombatLightRequest {
            target_id: None,
            damage: detonation.payload.base_damage,
            warhead_ref: detonation.payload.warhead,
            coord: ProjectileCoord::new(
                i32::from(rx) * 256 + sub_x.to_num::<i32>(),
                i32::from(ry) * 256 + sub_y.to_num::<i32>(),
                world_z_leptons,
            ),
            force_create: true,
            flags,
        });
    }
    let effect = if area_result == Some(AreaDamageResult::IronCurtain) {
        (!rules.general.weapon_nullify_anim.is_empty()).then(|| {
            let type_id = world.interner.intern(&rules.general.weapon_nullify_anim);
            // 46A2A1..46A301 uses the same placement and constructor row as
            // the ordinary impact, then returns before the ordinary tail.
            detonation_anim::placed_effect(type_id, placement)
        })
    } else {
        selected
    };
    if let Some(effect) = effect {
        crate::sim::world::damage_consequences::admit_explosion_effect(world, rules, effect);
    }
}

/// One bullet's detonation, `BulletClass::DetonateAtCoord` then its cluster
/// loop (`0x00469020..0x00469091`): per cluster the receivers commit, the anim
/// tail runs, and every cluster (the last included) draws its successor
/// around the impact, `RandomRanged(0x100, 0x200)` plus one raw draw. An
/// `Airburst=` bullet detonates once and draws nothing, as does a death
/// weapon's bare `DetonateAtCoord` (`ProjectileDetonationReason::DeathWeapon`)
/// and (VERA-internal, never in retail) a weapon that names no BulletType.
///
/// The selected animation constructs synchronously inside the tail, so its
/// common identity and constructor RNG precede the next cluster's draws.
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
        let Some(warhead) = rules.warhead(world.interner.resolve(detonation.payload.warhead))
        else {
            log::warn!(
                "Projectile {} dropped: missing serialized warhead {}",
                detonation.projectile_id,
                detonation.payload.warhead
            );
            continue;
        };
        let projectile_type = rules
            .weapon(world.interner.resolve(detonation.payload.weapon))
            .and_then(|weapon| weapon.projectile.as_deref())
            .and_then(|projectile| rules.projectile(projectile));
        let inviso = projectile_type.is_some_and(|projectile| projectile.inviso);
        // `CreateBullet` hands the bullet the weapon's `Bright=` (`+0x12F`).
        let bright = rules
            .weapon(world.interner.resolve(detonation.payload.weapon))
            .is_some_and(|weapon| weapon.bright);
        let death_weapon =
            detonation.reason == crate::sim::projectile::ProjectileDetonationReason::DeathWeapon;
        let (clusters, cluster_draws) = match projectile_type {
            Some(projectile) if !projectile.airburst && !death_weapon => {
                (projectile.cluster.max(0), true)
            }
            _ => (1, false),
        };
        let mut coordinate = detonation.impact;
        for _ in 0..clusters {
            let clustered = ProjectileDetonation {
                impact: coordinate,
                ..*detonation
            };
            let damage_start = emit.damage_events.len();
            let explosion_start = emit.effects.explosion_effects.len();
            let smudge_start = emit.effects.smudge_spawn_requests.len();
            let area_wall_route = emit_detonation_receivers(
                world,
                rules,
                overlay_registry,
                &clustered,
                warhead,
                emit,
            );
            let outer_explosion_effects = emit.effects.explosion_effects.split_off(explosion_start);
            let outer_anim_requests = emit.effects.smudge_spawn_requests.split_off(smudge_start);
            let (inline_death, mut pings, area_result) = commit_area_with_dispatch(
                world,
                run,
                &emit.damage_events[damage_start..],
                rules,
                overlay_registry,
            );
            emit.effects.append(inline_death);
            if let Some(routed_wall) = area_wall_route
                && area_result != AreaDamageResult::IronCurtain
            {
                let (rx, ry, _, _, z) = projectile_impact_cell(clustered.impact);
                emit.effects.bridge_state_changed |= continue_area_bridge_damage(
                    world,
                    rules,
                    overlay_registry,
                    (rx, ry),
                    clustered.payload.area_damage(),
                    clustered.payload.warhead,
                    z,
                    routed_wall,
                    area_result,
                );
            }
            emit.effects
                .explosion_effects
                .extend(outer_explosion_effects);
            under_attack_events.append(&mut pings);
            let anim_start = emit.effects.smudge_spawn_requests.len();
            emit_detonation_anim(
                world,
                rules,
                &clustered,
                warhead,
                inviso,
                bright,
                area_wall_route.map(|_| area_result),
                emit,
            );
            let mut anim_requests = outer_anim_requests;
            anim_requests.extend(emit.effects.smudge_spawn_requests.split_off(anim_start));
            commit_smudges(
                world,
                rules,
                overlay_registry,
                anim_requests,
                &mut emit.effects.smudge_spawn_requests,
            );
            if cluster_draws {
                coordinate =
                    projectile_next_cluster_coord(detonation.impact, &mut world.scenario_rng);
            }
        }
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
    debug_assert!(emit.fire_events.is_empty());
    debug_assert!(emit.ammo_deduct.is_empty());
    debug_assert!(emit.unit_facing.is_empty());
    debug_assert!(emit.spawn_target_updates.is_empty());

    LogicProjectileCommit {
        projectile_spawns: emit.projectile_spawns,
        effects: emit.effects,
        under_attack_events,
    }
}

/// Returns the GetFireError code the class acted on, when the fire routine
/// reached GetFireError at all.
pub(super) fn resolve_attacker_fire(
    world: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    snap: &AttackerSnapshot,
    fog_enabled: bool,
    binary_frame: u32,
    has_active_wave: bool,
    out: &mut CombatEmit,
) -> Option<fire_error::FireError> {
    let mut fire_error = None;
    if let Some(shot) = admit_attacker_fire(
        world,
        rules,
        overlay_registry,
        snap,
        fog_enabled,
        binary_frame,
        has_active_wave,
        &mut fire_error,
        out,
    ) {
        emit_admitted_fire(world, rules, shot, binary_frame, out, overlay_registry);
    }
    fire_error
}

/// `TechnoClass::Fire`'s per-shot report (`0x006FF349..0x006FF38F`, every
/// class, buildings included): none for an empty `Report=` (a signed count
/// test) or an `IsGattling=` type, whose report is its stage loop
/// (`combat::gattling`); otherwise `Report[(u16)+0x3C8 % Count]`, the low
/// word of the constructor's Scenario draw picking the item (an unsigned
/// `div`). Native rows: `tools/spatial_oracle/building_gattling.json`
/// (`report_gate`).
pub(crate) fn per_shot_report(
    weapon: &crate::rules::weapon_type::WeaponType,
    is_gattling: bool,
    sequence: u16,
) -> Option<&str> {
    let count = weapon.report_count();
    if count <= 0 || is_gattling {
        return None;
    }
    weapon.report_item(usize::from(sequence) % count as usize)
}

fn admit_attacker_fire<'r>(
    world: &mut Simulation,
    rules: &'r RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    snap: &AttackerSnapshot,
    fog_enabled: bool,
    binary_frame: u32,
    has_active_wave: bool,
    fire_error_out: &mut Option<fire_error::FireError>,
    out: &mut CombatEmit,
) -> Option<AdmittedFire<'r>> {
    let sound_enabled = sound_enabled(world);
    let delayed_building_slot = match snap.building_shot {
        Some(super::BuildingShot::Delayed { slot, .. }) => Some(slot),
        _ => None,
    };
    let mission_building_weapon = match snap.building_shot {
        Some(super::BuildingShot::Mission { weapon, .. }) => Some(weapon),
        _ => None,
    };
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
    // owner's exact Wave link remains live. GetFireError's T37/T46 (the live
    // Wave on either slot) keep the same protection for other layouts.
    if snap.category == EntityCategory::Unit
        && has_active_wave
        && combat_weapon::primary_for_tier(obj, snap.veterancy)
            .and_then(|weapon_id| rules.weapon(weapon_id))
            .is_some_and(|weapon| weapon.is_sonic)
    {
        return None;
    }

    // Check if target is alive and get its data.
    // For structures, target_coords returns the foundation center instead
    // of the NW corner.
    // For Cell targets (force-fire on terrain), synthesize the data:
    // cell-center coords, "always alive" (hp 1: cells don't despawn), and the
    // attacker's own type as the target type.
    let target_data: Option<(u16, u16, SimFixed, SimFixed, i32, InternedId)> = match snap.target {
        TargetKind::Entity(target_id) => world.substrate.entities.get(target_id).map(|t| {
            let (trx, try_, tsx, tsy) = target_coords(t);
            (trx, try_, tsx, tsy, t.health.current, t.type_ref())
        }),
        TargetKind::Cell(rx, ry) => {
            let (trx, try_, tsx, tsy) = cell_center_coords(rx, ry);
            Some((trx, try_, tsx, tsy, 1i32, snap.type_id))
        }
    };

    let (target_rx, target_ry, target_sub_x, target_sub_y, target_type_ref) = match target_data {
        Some((rx, ry, sx, sy, hp, type_ref)) if hp > 0 => (rx, ry, sx, sy, type_ref),
        _ => {
            if delayed_building_slot.is_some() {
                return None;
            }
            // Pointer expiry clears a dead target natively before any fire
            // routine runs; only a kill that skips the broadcast reaches here.
            out.remove_attack.push(snap.stable_id);
            return None;
        }
    };

    // Weapon selection, none of which asks legality (GetFireError below, or
    // the building's own visit, does): a delayed building shot resolves the
    // slot it saved while arming, a building's Mission_Attack shot the weapon
    // its visit selected ([`super::BuildingShot::Mission`]), and everything
    // else runs the native selection ladder (`What_Weapon_Should_I_Use`
    // `0x006F3330`). Each index resolves through GetWeapon
    // ([`combat_weapon::resolve_weapon_index`]), so an occupied building fires
    // its firing occupant's weapon (`0x004526F0`).
    let firer = world.substrate.entities.get(snap.stable_id)?;
    let is_garrison = snap.garrison.is_some();
    let (mut weapon_index, mut selected) = if let Some(saved_slot) = delayed_building_slot {
        let index = match saved_slot {
            WeaponSlot::Primary => 0,
            WeaponSlot::Secondary => 1,
        };
        let selected = combat_weapon::resolve_weapon_index(
            rules,
            firer,
            obj,
            index,
            &world.substrate.entities,
            &world.interner,
        )?;
        (selected.index, Some(selected))
    } else if let Some(weapon) = mission_building_weapon {
        (
            weapon,
            combat_weapon::resolve_weapon_index(
                rules,
                firer,
                obj,
                weapon,
                &world.substrate.entities,
                &world.interner,
            ),
        )
    } else {
        let Some((index, selected)) = combat_weapon::select_weapon_against(
            rules,
            firer,
            obj,
            &combat_weapon::attacker_facts(firer, obj),
            Some(&snap.target),
            &world.substrate.entities,
            &world.interner,
            world.resolved_terrain.as_ref(),
            fog_enabled.then_some(&world.fog.alliances),
        ) else {
            out.remove_attack.push(snap.stable_id);
            return None;
        };
        (index, selected)
    };

    let infantry_fire = snap.category == EntityCategory::Infantry;
    let sequences = rules.animation_sequence(world.interner.resolve(snap.type_id));
    let firing_at_entry = infantry_fire
        && world
            .substrate
            .entities
            .get(snap.stable_id)
            .is_some_and(|entity| entity.mission_leaf.foot_firing_sequence_latch() != 0);
    if firing_at_entry {
        let entity = world.substrate.entities.get(snap.stable_id)?;
        let prone = entity
            .infantry
            .as_ref()
            .is_some_and(|infantry| infantry.is_prone);
        //5206DE skips the first GetFireError.5209AF asks the second only
        // at exact equality, including signed/out-of-range retained stages.
        if entity.native_stage().value() != infantry_fire_frame(obj, sequences, weapon_index, prone)
        {
            return None;
        }
    }

    // `GetFireError` (vt+0x3C0, `fire_error`) with `SelectWeapon(Target)` and
    // the range check, asked where each class's fire routine asks it:
    // `UnitClass::Fire_At_Target @ 0x00736E3A`, `InfantryClass::
    // Fire_At_Target @ 0x005206F3` (`0x005209DE` on the fire frame) and
    // `AircraftClass::Mission_Attack @ 0x0041832E`. Each routine then acts on
    // the code as below. A building shoots only a FireAt its own visit asked
    // for this frame (`techno_ai::building_missions`): Mission_Attack's FireAt
    // arm, whose GetFireError (`0x0044B00F`) answered OK, or
    // ProcessDelayedFire's expiry, whose GetFireError (`0x00450476`) did;
    // neither is asked again.
    let subject = |world: &Simulation,
                   answer: &mut dyn FnMut(&fire_error_world::FireSubject<'_>)| {
        if let Some(firer) = world.substrate.entities.get(snap.stable_id) {
            answer(&fire_error_world::FireSubject {
                world,
                rules,
                overlay_registry,
                fog: fog_enabled.then_some(&world.fog),
                firer,
                obj,
                target: Some(snap.target),
                weapon_index,
            });
        }
    };
    let code = if snap.category == EntityCategory::Structure {
        snap.building_shot?;
        fire_error::FireError::Ok
    } else {
        let mut code = None;
        subject(&*world, &mut |s| code = Some(s.fire_error(true)));
        code?
    };
    let direction = crate::sim::movement::turret::facing_toward_lepton(
        snap.pos_rx,
        snap.pos_ry,
        snap.sub_x,
        snap.sub_y,
        target_rx,
        target_ry,
        target_sub_x,
        target_sub_y,
    );
    match snap.category {
        // The table at `0x00737148`.
        EntityCategory::Unit => match code {
            // Case 2 (`0x00736F78..0x0073701C`): a turretless vehicle
            // (`UnitType+0xE11`, which ReadINI sets to !`Turret=` at
            // `0x00747759`) with no NavCom (`+0x5A4`, `0x00736FB6`) whose
            // locomotor's Is_Moving (ILocomotion+0x10, `0x00736FE1`) is false
            // turns its hull toward the target at its own `ROT=` (`0x00737004`)
            // and copies the hull's destination into the turret slot
            // (`0x0073701C`). A pending order is neither. A turret follows its
            // target through the turret sweep.
            fire_error::FireError::Facing => {
                if snap.barrel_facing.is_none()
                    && world
                        .substrate
                        .entities
                        .get(snap.stable_id)
                        .is_some_and(|firer| {
                            firer.navigation.nav_com.is_none()
                                && crate::sim::movement::motion_query::is_moving(firer)
                                    != Some(true)
                        })
                    && let Some(update) = out
                        .unit_facing
                        .iter_mut()
                        .find(|u| u.entity_id == snap.stable_id)
                {
                    update.hull_destination = Some(direction);
                }
            }
            // Case 5 (`0x00736E7E`).
            fire_error::FireError::Illegal => {
                if heal_weapon_drops_target(
                    world,
                    rules,
                    selected.as_ref().map(|selected| selected.weapon),
                    snap.target,
                    EntityCategory::Unit,
                ) {
                    out.remove_attack.push(snap.stable_id);
                }
            }
            // Case 9 (`0x00737023`): surface only in range (`CanFireAt
            // 0x006F77B0`).
            fire_error::FireError::Cloaked => {
                let mut in_range = false;
                subject(&*world, &mut |s| in_range = s.in_range());
                if in_range {
                    uncloak_to_fire(world, rules, obj, snap.stable_id, sound_enabled);
                }
            }
            // Case 6 (`0x00737054`): a CANT shot clears a spawner's targets
            // (`SpawnManagerClass::ClearAllTargets 0x006B7BB0`).
            fire_error::FireError::Cant => {
                crate::sim::spawn_manager::clear_all_spawn_targets(
                    world,
                    snap.stable_id,
                    Some(rules),
                    overlay_registry,
                );
            }
            _ => {}
        },
        EntityCategory::Infantry => {
            if firing_at_entry {
                if code != fire_error::FireError::Ok {
                    world
                        .substrate
                        .entities
                        .get_mut(snap.stable_id)?
                        .mission_leaf
                        .set_foot_firing_sequence(0);
                    world.infantry_fire_refused_action(snap.stable_id, rules);
                }
            } else {
                match code {
                    fire_error::FireError::Illegal => {
                        if heal_weapon_drops_target(
                            world,
                            rules,
                            selected.as_ref().map(|selected| selected.weapon),
                            snap.target,
                            EntityCategory::Infantry,
                        ) {
                            out.remove_attack.push(snap.stable_id);
                        }
                    }
                    fire_error::FireError::Cloaked => {
                        uncloak_to_fire(world, rules, obj, snap.stable_id, sound_enabled);
                    }
                    _ => {}
                }
            }
        }
        // A delayed shot is dropped on any refusal (`0x004504D7`); an ordinary
        // one's codes were acted on by Mission_Attack (`0x0044B728`).
        EntityCategory::Structure => {}
        // Mission_Attack's strike states act on their codes in
        // `aircraft_release`; an aircraft reaches this only outside such a
        // visit, where code 9 surfaces as state 4's arm (`0x0041834C`) does.
        EntityCategory::Aircraft => {
            if code == fire_error::FireError::Cloaked {
                uncloak_to_fire(world, rules, obj, snap.stable_id, sound_enabled);
            }
        }
    }
    *fire_error_out = Some(code);
    if code != fire_error::FireError::Ok {
        return None;
    }
    if infantry_fire && !firing_at_entry {
        let actor = world.substrate.entities.get(snap.stable_id)?;
        let fire_fly =
            obj.jumpjet && crate::sim::movement::infantry_action::uses_jumpjet_locomotor(actor);
        let requested = if fire_fly {
            crate::sim::movement::infantry_action::DO_FIRE_FLY
        } else {
            infantry_fire_action(
                sequences,
                weapon_index,
                actor
                    .infantry
                    .as_ref()
                    .is_some_and(|infantry| infantry.is_prone),
                actor.infantry_deploy_doing(),
            )
        };
        //5208FE may refuse unchanged/noninterruptible/absent actions.520912
        // raises+68D regardless of AL; the retained Doing/Stage still decides
        // when this attempt can discharge.
        if let Err(cause) = world.infantry_do_action(snap.stable_id, requested, false, rules) {
            log::debug!("infantry {} firing Do_Action: {cause}", snap.stable_id);
        }
        let actor = world.substrate.entities.get_mut(snap.stable_id)?;
        actor.mission_leaf.set_foot_firing_sequence(1);
        actor.body_facing.snap(direction, binary_frame);
        let target_is_navcom = crate::sim::movement::nav_targets_same_receiver(
            actor.navigation.nav_com,
            match snap.target {
                TargetKind::Entity(id) => crate::sim::components::NavTargetRef::object(id),
                TargetKind::Cell(rx, ry) => crate::sim::components::NavTargetRef::cell(rx, ry),
            },
        );
        if target_is_navcom {
            //52093C/520946: shared FootStop4DF0D0 then the concrete+500
            // receiver. That receiver owns any DoAction/Walk Stop callback.
            crate::sim::movement::foot_stop_moving(actor);
            if let Err(cause) =
                world.run_find_path_failed_receiver(snap.stable_id, rules, overlay_registry)
            {
                log::debug!("infantry {} firing Stop_Driver: {cause}", snap.stable_id);
            }
        }
        let actor = world.substrate.entities.get(snap.stable_id)?;
        let prone = actor
            .infantry
            .as_ref()
            .is_some_and(|infantry| infantry.is_prone);
        if actor.native_stage().value() != infantry_fire_frame(obj, sequences, weapon_index, prone)
        {
            return None;
        }
        //5209C7/5209DE reselect and query live state after action/facing/Stop,
        // even when a frame-zero action can shoot in this same call.
        // Both actor and target are live reads after the synchronous callbacks.
        // Native5209BB/5209D2 reloads Target for SelectWeapon/GetFireError.
        let target = actor.attack_target.as_ref().map(|attack| attack.target);
        (weapon_index, selected) = combat_weapon::select_weapon_against(
            rules,
            actor,
            obj,
            &combat_weapon::attacker_facts(actor, obj),
            target.as_ref(),
            &world.substrate.entities,
            &world.interner,
            world.resolved_terrain.as_ref(),
            fog_enabled.then_some(&world.fog.alliances),
        )?;
        let code = fire_error_world::FireSubject {
            world,
            rules,
            overlay_registry,
            fog: fog_enabled.then_some(&world.fog),
            firer: actor,
            obj,
            target,
            weapon_index,
        }
        .fire_error(true);
        *fire_error_out = Some(code);
        if code != fire_error::FireError::Ok {
            world
                .substrate
                .entities
                .get_mut(snap.stable_id)?
                .mission_leaf
                .set_foot_firing_sequence(0);
            world.infantry_fire_refused_action(snap.stable_id, rules);
            return None;
        }
    }
    // Keep emission's FLH/heading/prone snapshot in the native post-action
    // window. Other classes retain their existing caller-owned snapshot.
    let firing_snapshot;
    let snap = if infantry_fire {
        let actor = world.substrate.entities.get(snap.stable_id)?;
        firing_snapshot = build_attacker_snapshot(
            actor,
            actor.attack_target.as_ref()?.target,
            snap.garrison.clone(),
        );
        &firing_snapshot
    } else {
        snap
    };
    let selected = selected?;
    let (target_coords, target_type_ref) = if infantry_fire {
        match snap.target {
            TargetKind::Entity(id) => {
                let target = world.substrate.entities.get(id)?;
                (target_coords(target), target.type_ref())
            }
            TargetKind::Cell(rx, ry) => (cell_center_coords(rx, ry), snap.type_id),
        }
    } else {
        (
            (target_rx, target_ry, target_sub_x, target_sub_y),
            target_type_ref,
        )
    };

    Some(AdmittedFire {
        snap: snap.clone(),
        obj,
        selected,
        target_coords,
        target_type_ref,
        is_garrison,
    })
}

/// A unit whose turn still reaches the firing update: alive, and not warped
/// out. `UnitClass::AI` returns first on vt+0x1D4, BeingWarpedOut `+0x270`
/// (`0x007362FB..0x0073635A`), which a Temporal chain and the teleport's
/// warp-out both set, and gates the update on IsAlive (`+0x90`,
/// `0x007365BB`), so a crashing Unit's wreck still reaches it at Health 0.
fn unit_reaches_fire_update(world: &Simulation, id: u64) -> bool {
    world.substrate.entities.get(id).is_some_and(|entity| {
        entity.category == EntityCategory::Unit && entity.is_ai_alive() && !entity.is_warped_out()
    })
}

/// Non-Unit fire hosts require Health: a Building's delayed fire,
/// an Infantry's fire and an Aircraft's attack mission do not run for a
/// Health-0 wreck. Units use their own live-slot IsAlive guard above.
fn attacker_reaches_fire(entity: &crate::sim::game_entity::GameEntity) -> bool {
    entity.is_alive() && !entity.dying
}

/// `Fire_At_Target`'s ILLEGAL arm (Unit `0x00736E7E`, Infantry `0x00520721`):
/// a healing weapon (`0x006F3970` of the selected slot, Damage + AmbientDamage
/// below zero) keeps only a damaged target of the firer's own class (health
/// ratio below `Rules+0x16F8`, 1.0; a NaN ratio keeps); any other target is
/// dropped. Every other weapon keeps it for `TechnoClass::AI`'s 16-frame
/// check.
fn heal_weapon_drops_target(
    world: &Simulation,
    rules: &RuleSet,
    weapon: Option<&WeaponType>,
    target: TargetKind,
    class: EntityCategory,
) -> bool {
    if weapon.is_none_or(|weapon| weapon.damage.wrapping_add(weapon.ambient_damage) >= 0) {
        return false;
    }
    let TargetKind::Entity(id) = target else {
        return true;
    };
    let Some(target) = world
        .substrate
        .entities
        .get(id)
        .filter(|target| target.category == class)
    else {
        return true;
    };
    let strength = rules
        .object(world.interner.resolve(target.type_ref()))
        .map_or(0, |object| object.strength);
    target.health.is_full(strength)
}

/// StartUncloaking (vt+0x45C) from a building's Mission_Attack CLOAKED arm
/// (`0x0044B284`).
pub(crate) fn start_uncloaking_to_fire(world: &mut Simulation, rules: &RuleSet, id: u64) {
    let sound_enabled = sound_enabled(world);
    let Some(obj) = world
        .substrate
        .entities
        .get(id)
        .and_then(|entity| rules.object(world.interner.resolve(entity.type_ref())))
    else {
        return;
    };
    uncloak_to_fire(world, rules, obj, id, sound_enabled);
}

/// `TechnoClass::Uncloak` (vt+0x45C, `0x007036C0`) with its sound, each
/// class's CLOAKED (9) arm.
fn uncloak_to_fire(
    world: &mut Simulation,
    rules: &RuleSet,
    obj: &ObjectType,
    id: u64,
    sound_enabled: bool,
) {
    let binary_frame = world.session.binary_frame;
    // `UnitClass::Fire_At_Target @ 0x00736DF0` case 9 invokes virtual
    // `StartUncloaking +0x45C @ 0x007036C0` after rechecking CanFireAt.
    let start = world
        .substrate
        .entities
        .get_mut(id)
        .and_then(|entity| entity.cloak.as_mut())
        .map(|cloak| {
            cloak.start_uncloaking(
                binary_frame as i32,
                obj.cloaking_speed,
                rules.general.cloaking_stages,
                false,
            )
        });
    if start.is_some_and(|result| result.play_sound) && sound_enabled {
        emit_configured_cloak_sound(world, id, rules);
    }
}

/// Call-local result of admission and fire-action work.
/// This is call-local data, never a saved permission to fire on a later frame.
/// Native Mission_Attack418403 checks legality once before its burst loop;
/// separating emission lets the aircraft caller reselect from live state
/// without repeating admission for every FireAt call.
pub(super) struct AdmittedFire<'a> {
    pub(super) snap: AttackerSnapshot,
    pub(super) obj: &'a ObjectType,
    pub(super) selected: combat_weapon::SelectedWeapon<'a>,
    pub(super) target_coords: (u16, u16, SimFixed, SimFixed),
    pub(super) target_type_ref: InternedId,
    pub(super) is_garrison: bool,
}

/// Object `id`'s GetCoords (`vt+0x48`,
/// [`crate::sim::movement::ground_pose::object_get_coords`]) as a projectile
/// coordinate.
fn object_get_coords(world: &Simulation, id: u64) -> Option<ProjectileCoord> {
    let entity = world.substrate.entities.get(id)?;
    let coord = crate::sim::movement::ground_pose::object_get_coords(
        entity,
        world.resolved_terrain.as_ref(),
    );
    Some(ProjectileCoord::new(coord.x, coord.y, coord.z))
}

/// `TechnoClass::ReceiveDamage @ 0x00702A58..0x00702B2F`, after
/// `ShouldRetaliate` agreed: the retaliation is issued only when the source is
/// in range of the weapon selected against it (vt+0x3A8, `0x00702A69`), or
/// the owner is a computer (`0x00702A7D`), or the source stands within the
/// victim's sight: `ftol(Sqrt_Approx(dx² + dy² + dz²))` between the two
/// GetCoords at most `(Sight + 0.5) * 256` (`0x00702A8A..0x00702B2C`). A
/// player's unit therefore does not charge an out-of-sight V3 or Grand Cannon.
fn retaliation_reaches(
    world: &Simulation,
    rules: &RuleSet,
    victim_id: u64,
    source_id: u64,
) -> bool {
    let entities = &world.substrate.entities;
    let (Some(victim), Some(source)) = (entities.get(victim_id), entities.get(source_id)) else {
        return false;
    };
    let (Some(victim_type), Some(source_type)) = (
        rules.object(world.interner.resolve(victim.type_ref())),
        rules.object(world.interner.resolve(source.type_ref())),
    ) else {
        return false;
    };
    let in_range = super::fire_error_world::FireSubject {
        world,
        rules,
        overlay_registry: None,
        fog: Some(&world.fog),
        firer: victim,
        obj: victim_type,
        target: Some(TargetKind::Entity(source_id)),
        weapon_index: combat_targeting::retaliation_weapon_index(
            world,
            rules,
            victim,
            victim_type,
            source,
            source_type,
        ),
    }
    .in_range();
    let human = world.owner_is_human(victim.owner());
    if in_range || !human {
        return true;
    }
    let (Some(from), Some(to)) = (
        object_get_coords(world, victim_id),
        object_get_coords(world, source_id),
    ) else {
        return false;
    };
    let distance =
        crate::util::native_x87::distance_3d_leptons([to.x, to.y, to.z], [from.x, from.y, from.z]);
    // (Sight + 0.5) * 256 >= distance, exactly: 2 * distance <= (2 * Sight + 1) * 256.
    2 * i64::from(distance) <= (2 * i64::from(victim_type.sight) + 1) * 256
}

/// Where `TechnoClass::FireAt @ 0x006FDD50` launches from: `[ESP+0x44]`, which
/// the bullet launch (`BulletClass::Fire`, vt+0x1F0 at `0x006FF014`), the
/// report (`0x006FF38F`) and the muzzle anim (`0x006FF3C2`) all read.
struct FireAtLaunchSource {
    /// The fire coordinate (`GetFLH`, vt+0xB0, `0x006FE268`), replaced for a
    /// `Dropping=` projectile (BulletType `+0x29C`) by the firer's GetCoords
    /// (vt+0x48, `0x006FE2D8..0x006FE2FE`; `0x006FE960` re-reads the same
    /// value). Such a shot starts, flashes and sounds at the firer's centre.
    /// `Arcing=` is `+0x29B` and does not move the source: a cannon shell
    /// leaves its barrel. The only retail `Dropping=` projectile,
    /// `[V3AirburstP]`, belongs to no weapon in use.
    coord: ProjectileCoord,
    /// `coord.y` minus the Y of the object coordinate (vt+0xAC) the fire
    /// coordinate's offset was added to: a building's muzzle-anim `ZAdjust`
    /// input (`0x006FF3E2..0x006FF40B`), taken from the launch source.
    offset_y: i32,
}

fn fireat_launch_source(
    world: &Simulation,
    rules: &RuleSet,
    snap: &AttackerSnapshot,
    fire: &super::fire_coord::FireCoordinate,
    weapon: &crate::rules::weapon_type::WeaponType,
) -> FireAtLaunchSource {
    let dropping = weapon
        .projectile
        .as_deref()
        .and_then(|id| rules.projectile(id))
        .is_some_and(|projectile| projectile.dropping);
    match object_get_coords(world, snap.stable_id).filter(|_| dropping) {
        Some(coord) => FireAtLaunchSource {
            coord,
            offset_y: fire.offset_y + (coord.y - fire.coord.y),
        },
        None => FireAtLaunchSource {
            coord: fire.coord,
            offset_y: fire.offset_y,
        },
    }
}

/// What a shot's delta aims at and how fast it launches.
struct FireAtLaunchAim {
    /// The delta's endpoint (`0x006FE62F` -> `0x0070BCB0`): the current
    /// target's vt+0x58 coordinate, led along its facing when it is a moving
    /// UnitClass (`launch::lead_aim`).
    aim: ProjectileCoord,
    /// `WeaponTypeClass::GetSpeed` (`0x006FE53A`) at FireAt's distance from
    /// the launch source to the target coordinate (`0x006FE4F6..0x006FE537`).
    speed: i32,
}

/// `TechnoClass::FireAt`'s aim and launch speed for the shot `snap` fires
/// with `weapon` from `source` at `target_coord` (the target's unled
/// vt+0x58/vt+0xA4 coordinate). Native execution of the numeric leaves:
/// `tools/projectile_oracle/fireat_speed.py`. The aim (`0x0070BCB0`) reads
/// the call-local TarCom authority independently of FireAt's argument. The
/// argument's coordinate supplies only the launch-speed distance here
/// (`0x006FE1DA..0x006FE218`, `0x006FE53A`).
///
/// RESIDUAL: a building target's vt+0xA4 adds its type's
/// `TargetCoordOffset=` (`0x004500A0`, BuildingType `+0xEBC`), unparsed; the
/// launch distance uses its centre. Trigger: shots at the three shipyards,
/// the only stock types with an offset. Effect: a launch speed a few leptons
/// per frame off.
fn fireat_launch_aim(
    world: &Simulation,
    rules: &RuleSet,
    snap: &AttackerSnapshot,
    weapon: &crate::rules::weapon_type::WeaponType,
    source: ProjectileCoord,
    target_coord: ProjectileCoord,
    tarcom: Option<TargetKind>,
) -> FireAtLaunchAim {
    use crate::sim::projectile::launch::{
        LaunchSpeedProjectile, fireat_launch_distance, weapon_launch_speed,
    };
    let speed_projectile = |weapon: &crate::rules::weapon_type::WeaponType| {
        weapon
            .projectile
            .as_deref()
            .and_then(|id| rules.projectile(id))
            .map(|projectile| LaunchSpeedProjectile {
                rot: projectile.rot,
                floater: projectile.floater,
            })
    };
    let speed = weapon_launch_speed(
        weapon.speed,
        speed_projectile(weapon),
        rules.general.gravity,
        fireat_launch_distance(source, target_coord),
    );
    FireAtLaunchAim {
        aim: super::aim_coord::led_target_coordinate(world, rules, snap.stable_id, tarcom),
        speed,
    }
}

/// `TechnoClass::FireAt`'s damage build (`damage::attacker::fire_damage`)
/// for the shot `snap` fires with `weapon`. The firer's own rank and type
/// decide the FIREPOWER stage; for a garrison shot that is the building,
/// exactly as native's `this` is.
///
/// RESIDUAL: the firepower fold's `House+0x188` is 1.0. The house value is
/// `[Easy]/[Normal]/[Difficult] FirePower=` times the country's `Firepower=`,
/// both 1.0 by default (`0x0066D28E`, `0x00511980`) and set by no retail
/// layer. `Techno+0x160` is the firer's live `firepower_multiplier`, raised
/// by the Firepower crate (`crates::effects`).
///
/// A passenger of an `OpenTopped=` transport (retail: `[BFRT]`) fires with
/// `+0x82` set (`PerCellProcess 0x0051A45E`/`0x0073A75D` ->
/// `SetInOpenTransport 0x00710470`), which takes the open-topped stage.
fn fireat_damage(
    world: &Simulation,
    rules: &RuleSet,
    snap: &AttackerSnapshot,
    obj: &ObjectType,
    weapon: &crate::rules::weapon_type::WeaponType,
    is_garrison: bool,
) -> i32 {
    use crate::util::native_x87::{NativeF32Bits, NativeF64Bits};
    let firer = world.substrate.entities.get(snap.stable_id);
    let f32_bits = |value: f32| NativeF32Bits::from_bits(value.to_bits());
    let multipliers = &rules.garrison_rules;
    let stages = damage::attacker::FireDamageStages {
        house_firepower: NativeF64Bits::ONE,
        unit_firepower: firer.map_or(NativeF64Bits::ONE, |firer| firer.firepower_multiplier),
        rank_firepower: self::veterancy::has_weapon_ability(
            self::veterancy::rank_from_u16(snap.veterancy),
            obj,
            crate::rules::object_type::Ability::Firepower,
        )
        .then(|| NativeF64Bits::from_bits(rules.general.veteran_combat.to_bits())),
        // `vt+0x400`: BuildingClass `0x00458DD0`, CanBeOccupied &&
        // CanOccupyFire && an occupant; false for every other class.
        occupied: is_garrison.then(|| f32_bits(multipliers.occupy_damage_multiplier)),
        // `+0x2E4` on a non-building: a unit installed in a Tank Bunker.
        bunkered: firer
            .filter(|firer| firer.category != EntityCategory::Structure)
            .and_then(|firer| firer.bunker_link.installed_in())
            .map(|_| f32_bits(multipliers.bunker_damage_multiplier)),
        open_topped: firer
            .filter(|firer| firer.passenger_role.in_open_transport())
            .map(|_| f32_bits(multipliers.open_topped_damage_multiplier)),
    };
    damage::attacker::fire_damage(
        weapon.damage,
        weapon.is_sonic || weapon.use_fire_particles,
        &stages,
    )
}

/// FireAt's RevealOnFire (`0x006FF66C..0x006FF743`, reached only by a
/// launched shot). The target's owner, when it is the local player
/// (`0x0050B6F0`), has radius 3 around the firer revealed on its own map
/// (`MapClass::RevealShroud @ 0x005673A0`). When that player owns the firer
/// (`+0x41A`) or has discovered it (`+0x41B`, set for every placed object in a
/// multiplayer game), the firer's own coordinate must still be shrouded for it
/// (`MapClass::IsShrouded @ 0x00586360`), and an aircraft it owns never
/// reveals. A cell target reveals nothing.
///
/// VERA keeps a map per house, so every human house takes the reveal its own
/// client would take; computer houses keep no shroud.
fn reveal_on_fire(world: &mut Simulation, rules: &RuleSet, firer_id: u64, target: TargetKind) {
    let TargetKind::Entity(target_id) = target else {
        return;
    };
    let Some(house) = world
        .substrate
        .entities
        .get(target_id)
        .map(|target| target.owner())
    else {
        return;
    };
    if !world.owner_is_human(house) {
        return;
    }
    let Some(firer) = world.substrate.entities.get(firer_id) else {
        return;
    };
    let coord = crate::sim::movement::ground_pose::position_world_coord(&firer.position);
    let owned = firer.owner() == house;
    let discovered = world.session.game_mode_nonzero
        || (world.session.current_house == Some(house)
            && firer.discovery.discovered_by_current_house);
    if owned || discovered {
        if owned && firer.category == EntityCategory::Aircraft {
            return;
        }
        // `0x006FF692..0x006FF6B4`: IsShrouded, else `0x005865E0`, which is
        // `XOR AL,AL; RET 4` and never admits, so IsShrouded alone decides.
        let shrouded = match world.resolved_terrain.as_ref() {
            Some(terrain) => {
                let cells = crate::map::resolved_terrain::NativeCellQuery::isolated(terrain);
                crate::sim::vision::coordinate_is_shrouded(&cells, coord, &|cell| {
                    Ok(match cell {
                        crate::map::cell_index::NativeCellIdentity::Real(index) => {
                            let cell = &terrain.cells()[index];
                            world.fog.is_cell_revealed(house, cell.rx, cell.ry)
                        }
                        crate::map::cell_index::NativeCellIdentity::Dummy => false,
                    })
                })
                .unwrap_or(false)
            }
            None => !world
                .fog
                .is_cell_revealed(house, firer.position.rx, firer.position.ry),
        };
        if !shrouded {
            return;
        }
    }
    let reveal_by_height = rules.general.reveal_by_height;
    let height_grid = reveal_by_height
        .then(|| {
            world
                .path_grid
                .as_ref()
                .map(|grid| grid.ground_height_grid())
        })
        .flatten();
    let map_size = world.map_size_diamond();
    crate::sim::vision::reveal_shroud_on_fire(
        &mut world.fog,
        house,
        coord,
        reveal_by_height,
        height_grid.as_deref(),
        map_size,
    );
}

/// `TechnoClass::GetROF` (vt+0x318) for a shot: of the weapon GetWeapon
/// answers (a garrison's next occupant's), at the stepped burst index. The rank
/// and class are the firer's (a garrison shot reads the building's). FireAt
/// created the fired weapon's particle systems just before
/// (`0x006FF15B..0x006FF26E`), so its flags are the live systems GetROF tests;
/// a system left from an earlier shot matters only when GetWeapon answers a
/// different weapon. RESIDUAL: retained live systems from an earlier weapon
/// are not queried here.
fn fireat_get_rof(
    world: &mut Simulation,
    rules: &RuleSet,
    snap: &AttackerSnapshot,
    obj: &ObjectType,
    weapon: &WeaponType,
    rof_weapon: Option<&WeaponType>,
    next_index: i32,
) -> i32 {
    let firer = world.substrate.entities.get(snap.stable_id);
    super::rof::get_rof(
        &super::rof::RofQuery {
            // RESIDUAL: a building's own Ammo (`+0x2FC`) is not kept;
            // no retail building sets `Ammo=`.
            building_ammo: None,
            weapon: rof_weapon,
            live: super::rof::LiveParticles {
                spark: weapon.use_spark_particles,
                fire: weapon.use_fire_particles,
                railgun: weapon.is_railgun,
            },
            burst_index: next_index,
            unit_burst_delays: (snap.category == EntityCategory::Unit).then_some(obj.burst_delays),
            house_rof: world
                .houses
                .get(&snap.owner)
                .map_or(crate::util::native_x87::NativeF64Bits::ONE, |house| {
                    house.rof_bias()
                }),
            rof_ability: self::veterancy::has_weapon_ability(
                self::veterancy::rank_from_u16(snap.veterancy),
                obj,
                crate::rules::object_type::Ability::Rof,
            ),
            veteran_rof: rules.general.veteran_rof,
            occupants: snap.garrison.as_ref().map(|gs| gs.occupant_count as i32),
            bunkered: snap.category != EntityCategory::Structure
                && firer.is_some_and(|firer| {
                    matches!(
                        firer.bunker_link,
                        crate::sim::game_entity::BunkerLink::Installed(_)
                    )
                }),
            occupy_rof_multiplier: rules.garrison_rules.occupy_rof_multiplier,
            bunker_rof_multiplier: rules.garrison_rules.bunker_rof_multiplier,
        },
        &mut world.scenario_rng,
    )
}

/// Existing FireAt delivery and bookkeeping, shared by the world receiver.
/// The caller still owns legality, fire-action timing and inline damage commit.
/// Answers the bullet `TechnoClass::FireAt @ 0x006FDD50` returns: the one
/// launched, or `None` where it returns NULL (a refused sonic shot, the
/// spawner, drain and DiskLaser arms, and a launch with no solution).
pub(super) fn emit_admitted_fire(
    world: &mut Simulation,
    rules: &RuleSet,
    shot: AdmittedFire<'_>,
    binary_frame: u32,
    out: &mut CombatEmit,
    overlay_registry: Option<&OverlayTypeRegistry>,
) -> Option<u64> {
    // Infantry51DF70 clears68D for every direct FireAt caller, including
    // Guard521432 and a launch subsequently refused by Techno6FDD50.
    if shot.snap.category == EntityCategory::Infantry {
        if let Some(actor) = world.substrate.entities.get_mut(shot.snap.stable_id) {
            actor.mission_leaf.set_foot_firing_sequence(0);
        }
    }
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
    if weapon.is_sonic && world.active_wave_links.contains_key(&snap.stable_id) {
        return None;
    }

    // Spawner weapon: gamemd's Fire_At short-circuits here. It calls
    // `SpawnManagerClass::SetTarget` and returns NULL — no bullet, no damage,
    // no detonation effects, and no rearm timer write (the rearm write lives
    // further down Fire_At, past this branch). Because the branch returns above
    // the first random draw as well as above bullet allocation, a spawner fire
    // consumes zero scenario-RNG draws natively.
    //
    // GetFireError T35 (`0x006FC606`) has already refused a launch from a
    // bridge-side cell (`IsOnBridge_ForFiring 0x00703B10`), from a paralysed
    // firer, and with no spawn out of regeneration (REARM).
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
        return None;
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
        return None;
    }

    // `TechnoClass::FireAt`'s DiskLaser arm (`0x006FE460..0x006FE4EF`): a
    // DiskLaserClass takes the shot, the burst steps around GetROF, the rearm
    // stores GetROF's value unhalved, and FireAt returns with no bullet,
    // report or muzzle anim. `DiskLaserClass::AI @ 0x004A7340` then deletes
    // the laser before it draws or deals anything while its owner is crashing
    // (`+0x425`, `0x004A7462`), so a falling Floating Disc's shots are spent
    // here. A live Disc's delivery is VERA's unported DiskLaser path below.
    if weapon.disk_laser
        && world
            .substrate
            .entities
            .get(snap.stable_id)
            .is_some_and(|firer| firer.crashing)
    {
        let burst = world
            .substrate
            .entities
            .get(snap.stable_id)
            .map(|entity| entity.weapon_burst)
            .unwrap_or_default();
        let rof = fireat_get_rof(
            world,
            rules,
            snap,
            obj,
            weapon,
            Some(weapon),
            burst.next_index(),
        );
        if let Some(entity) = world.substrate.entities.get_mut(snap.stable_id) {
            entity.rearm_after_fire(binary_frame as i32, rof);
            entity.weapon_burst.complete_shot(weapon.burst.max(1));
        }
        return None;
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
    let tarcom = fireat_tarcom(world, snap);
    // One fire coordinate per shot: the bullet origin, the muzzle animation and
    // the report sound all take it (`combat::fire_coord`).
    let fire = super::fire_coord::fire_coordinate(
        world,
        rules,
        &super::fire_coord::FireSource {
            tar_com: tarcom,
            ..super::fire_coord::FireSource::from(snap)
        },
        obj,
        selected.index,
        burst_index,
        crate::rules::flh::Flh::default(),
    );
    let launch_source = fireat_launch_source(world, rules, snap, &fire, weapon);
    // Laser width reads +664 inside FireAt, before its caller applies/clears
    // the support multiplier. Preserve that read across the existing deferred
    // bullet payload construction below.
    let prism_support_count = world
        .substrate
        .entities
        .get(snap.stable_id)
        .map_or(0, |entity| entity.prism_support_count);
    let warhead = selected.warhead;
    let base_damage = fireat_damage(world, rules, snap, obj, weapon, is_garrison);
    let ProjectileDelivery {
        collision,
        ballistic,
        vertical,
        launch_scatter_is_flak,
        guidance,
        ..
    } = classify_projectile_delivery(weapon, rules);
    // `CreateBullet @ 0x0046B050` (called at `0x006FE55D`) takes the bullet's
    // unique id (`0x00410230`) before the launch math, so a launch that then
    // fails has still spent one.
    let bullet_id = world.allocate_stable_id();
    let native_unique_id = world.next_native_runtime_id();
    fireat_estimate_debit(world, rules, snap.stable_id, tarcom, obj, weapon);
    let launched = {
        let impact_world_z_leptons = attack_world_z_leptons(
            snap.target,
            &world.substrate.entities,
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
        // The bullet leaves from the launch source (the fire coordinate, or for
        // a `Dropping=` projectile the firer's GetCoords, `0x006FE2E2`); the
        // delta aims at the target, led when it is a moving vehicle
        // (`0x0070BCB0`). See `fireat_launch_source`/`fireat_launch_aim`.
        let launch_geometry = fireat_launch_aim(
            world,
            rules,
            snap,
            weapon,
            launch_source.coord,
            impact,
            tarcom,
        );
        let origin = launch_source.coord;
        let projectile_type = weapon
            .projectile
            .as_deref()
            .and_then(|projectile_id| rules.projectile(projectile_id));
        // `TechnoClass::FireAt` step order, `0x006FE663`..`0x006FEA52`:
        // resolve the target delta, scatter it, recompute the facing from the
        // scattered delta, clamp the launch speed to half the straight-line
        // distance, and only then force a homing or vertical shot to one
        // lepton per frame. `BulletClass::Fire` keeps the target's own
        // unled coordinate as the bullet's (`0x00468700..0x0046872B`).
        let frozen_target_position = impact;
        let aim = launch_geometry.aim;
        let delta = ProjectileCoord::new(aim.x - origin.x, aim.y - origin.y, aim.z - origin.z);
        let delta = match launch_scatter_is_flak {
            Some(flak) => {
                // vt+0x168 (`TechnoClass::GetWeaponRange @ 0x007012C0`) for
                // the fired weapon: an open-topped firer's passengers cap it,
                // and an occupied building answers its firing occupant's
                // weapon, the one this shot fires.
                let range = world.substrate.entities.get(snap.stable_id).map_or(
                    weapon.range_leptons,
                    |firer| {
                        combat_weapon::weapon_range(
                            firer,
                            obj,
                            selected.index,
                            &world.substrate.entities,
                            rules,
                            &world.interner,
                        )
                    },
                );
                crate::sim::projectile::launch::fireat_launch_scatter(
                    delta,
                    rules.combat_damage.ballistic_scatter,
                    range,
                    flak,
                    &mut world.scenario_rng,
                )
            }
            None => delta,
        };
        use crate::sim::projectile::launch::{
            FireAtLaunch, FireAtLaunchResult, fireat_launch, high_arc_root,
        };
        let raw_source = ProjectileCoord::new(
            i32::from(snap.pos_rx) * 256 + snap.sub_x.to_num::<i32>(),
            i32::from(snap.pos_ry) * 256 + snap.sub_y.to_num::<i32>(),
            origin_world_z_leptons,
        );
        // 70D590 reads the source's current target (TarCom), independently of
        // the FireAt parameter and the scattered launch delta.
        let current_target = tarcom;
        let target_location = |target: TargetKind| -> Option<ProjectileCoord> {
            match target {
                TargetKind::Entity(id) => object_get_coords(world, id),
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
        let flh_origin_z = fire.coord.z;
        // 6FE947..6FE98A: directed launches read virtual+308. Dropping's
        // second GetCoords read (0x006FE960) stores the value the launch
        // source already holds (`fireat_launch_source`).
        // RESIDUAL: `RadialFireSegments=` (`TechnoTypeClass+0x6A4`) is not
        // parsed. One stock author, `[AEGIS]`: native replaces the launch
        // direction with `body facing + (PI * counter / segments - PI / 2)`,
        // cycling a counter at `TechnoClass+0x43C`, and forces the ROT>0
        // launch speed to 1 only when it is zero (`0x006FEA18..0x006FEA3A`).
        // Player effect: the Aegis Cruiser fires straight at one target
        // instead of sweeping its flak arc, in every Allied naval engagement.
        let directed_heading = projectile_type
            .filter(|projectile| projectile.dropping || projectile.rot != 0)
            .map(|_| {
                let source = world.substrate.entities.get(snap.stable_id);
                let hull = source.map_or(snap.hull_facing.current(binary_frame), |source| {
                    source.body_facing_current(binary_frame)
                });
                match snap.category {
                    EntityCategory::Unit if obj.has_turret => source
                        .and_then(|source| source.barrel_facing.as_ref())
                        .map_or(0, |facing| facing.current(binary_frame)),
                    EntityCategory::Unit | EntityCategory::Infantry => hull,
                    EntityCategory::Aircraft => source
                        .and_then(|source| source.barrel_facing.as_ref())
                        .map_or(0, |facing| facing.current(binary_frame)),
                    // A building's is its fire facing, vt+0x308 (`0x0044D7D0`,
                    // read at `0x006FE2D2` and `0x006FE950`).
                    EntityCategory::Structure => fire.aim_facing16,
                }
            });
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
        let building_pitch_height = (!ballistic && !voxel && delta.z.wrapping_abs() > 200)
            .then_some(current_target)
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
        let launch = fireat_launch(FireAtLaunch {
            delta,
            speed: launch_geometry.speed,
            vertical: vertical.is_some(),
            homing: guidance.is_some(),
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
        });
        // A launch with no ballistic solution deletes its bullet
        // (`0x006FF000` -> `0x006FF93C`, or `0x006FF01E` when
        // `BulletClass::Fire` refuses) and resumes at `0x006FF749`.
        let launched = launch.is_some();
        if let Some(FireAtLaunchResult {
            velocity,
            speed: launch_speed,
        }) = launch
        {
            // ProcessDelayedFire writes its support bonus on the bullet this
            // FireAt returns (`0x00450496..0x004504CD`). The rest of FireAt
            // reads neither the bullet's multiplier nor the count, save the
            // laser width (`0x006FF52B`), sampled before this consumption.
            let damage_multiplier = match snap.building_shot {
                Some(super::BuildingShot::Delayed { .. }) => {
                    world.take_support_bonus(snap.stable_id, rules)
                }
                _ => ProjectilePayload::UNSCALED,
            };
            let payload = ProjectilePayload::new(
                base_damage,
                world.interner.intern(&warhead.id),
                world.interner.intern(selected.weapon_id),
            )
            .with_damage_multiplier(damage_multiplier);
            // `BulletClass::Fire` (`0x006FF014`) at the launch source.
            super::admit_fired_bullet(
                world,
                rules,
                weapon,
                bullet_id,
                super::FiredBullet {
                    native_unique_id,
                    source_id: snap.stable_id,
                    target,
                    target_position: frozen_target_position,
                    payload,
                    max_speed: weapon.speed,
                    origin,
                    velocity,
                    speed: launch_speed.clamp(0, i32::from(u16::MAX)) as u16,
                },
            );
        }
        launched
    };

    // A failed launch resumes at `0x006FF749`, past the rest of the shot
    // (`0x006FF031..0x006FF743`): the occupant advance, recoil, particle
    // systems, the burst step, GetROF and its draws, the rearm, the muzzle
    // anim, Report, the laser/bolt/wave, DecreaseAmmo (`0x006FF656`), the
    // `+0x3BC` 15-frame timer (`0x006FF4B0`), RevealOnFire and the `+0x120`
    // store, so the firer may try again next frame. The tail from
    // `0x006FF749` still runs.
    if !launched {
        fireat_tail(world, rules, snap, weapon, None, overlay_registry);
        return None;
    }

    let sequence = world
        .substrate
        .entities
        .get(snap.stable_id)
        .map_or(0, |firer| firer.techno_ctor_random_word);
    let report_sound_id = per_shot_report(weapon, obj.is_gattling, sequence)
        .map(|report| world.interner.intern(report));
    let in_open_transport = world
        .substrate
        .entities
        .get(snap.stable_id)
        .is_some_and(|firer| firer.passenger_role.in_open_transport());
    let facing = (snap.hull_facing.current(binary_frame) >> 8) as u8;
    out.fire_events.push(SimFireEvent {
        attacker_id: snap.stable_id,
        attacker_type_ref: snap.type_id,
        weapon_slot: selected.slot,
        weapon_id: world.interner.intern(selected.weapon_id),
        facing,
        veterancy: snap.veterancy,
        origin_snapshot: FireOriginSnapshot {
            rx: snap.pos_rx,
            ry: snap.pos_ry,
            z: snap.pos_z,
            sub_x: snap.sub_x,
            sub_y: snap.sub_y,
            facing,
        },
        target: snap.target,
        report_sound_id,
        // The report and the muzzle anim read the launch source (`[ESP+0x44]`,
        // `0x006FF38F`, `0x006FF3C2`), not the fire coordinate itself.
        fire_coord: launch_source.coord,
        fire_offset_y: launch_source.offset_y,
        muzzle_anim: super::fire_coord::muzzle_anim_name(
            weapon,
            fire.aim_facing16,
            snap.garrison.is_some(),
            in_open_transport,
        )
        .map(|name| world.interner.intern(name)),
        occupied_building: snap.garrison.is_some(),
        firer_category: snap.category,
    });

    // `TechnoClass::FireAt @ 0x006FF031..0x006FF085`, right after
    // `BulletClass::Fire`: an occupied building advances its firing occupant,
    // `(+0x69C + 1) % occupants`. Everything after reads the new index: the
    // rearm below (`GetROF` at `0x006FF289` takes the building's weapon
    // through `BuildingClass::GetWeapon @ 0x004526F0`, i.e. the NEXT
    // occupant's ROF and Burst) and the shot's own kill credit, since its
    // Inviso bullet detonates later, in its own AI in this frame's Logic tail.
    if is_garrison
        && let Some(cargo) = world
            .substrate
            .entities
            .get_mut(snap.stable_id)
            .and_then(|building| building.passenger_role.cargo_mut())
    {
        let count = cargo.count() as u8;
        if count > 0 {
            cargo.garrison_fire_index = (cargo.garrison_fire_index + 1) % count;
        }
    }
    // Techno6FF0B7..6FF15B: only a successfully launched shot arms recoil.
    // Stock recoil buildings have their own Turret=yes. Building4527D0's
    // upgrade-provided turret remains part of the separate upgrade mechanism.
    if let Some(entity) = world.substrate.entities.get_mut(snap.stable_id) {
        entity.fire_voxel_recoil(obj.has_turret);
    }
    // GetROF (`0x006FCFA0`) reads its weapon through GetWeapon (vt+0x3F8,
    // `0x006FCFCA`), after the advance above. Preserve a missing weapon:
    // `0x006FCFD2..0x006FCFDD` returns 1 without an RNG draw, rather than
    // reusing the weapon just fired (native `techno_rearm.json` null arm).
    let rof_weapon = world
        .substrate
        .entities
        .get(snap.stable_id)
        .and_then(|firer| {
            combat_weapon::get_weapon(
                firer,
                obj,
                selected.index,
                &world.substrate.entities,
                rules,
                &world.interner,
            )
        });

    let next_index = burst.next_index();
    let mid_burst = rof_weapon.is_some_and(|weapon| next_index < weapon.burst);
    // `CALL [EDX+0x318]` at `0x006FF289`.
    let rof = fireat_get_rof(world, rules, snap, obj, weapon, rof_weapon, next_index);

    // `0x006FF274..0x006FF2CB`, all on the firer: the burst step around GetROF,
    // then the rearm (`+0x2EC`) with GetROF's value, which a berserk firer
    // (`+0x298`) halves, signed and toward zero (`0x006FF28F..0x006FF29C`).
    // `0x006FF743` stores the frame in `+0x120`, the since-my-last-shot mark
    // `UnitClass::Facing_Update`'s idle dwell and a Gattling building's idle
    // decay (`0x0043FEF5`) read (the constructor at `0x006F2B9C` is its only
    // other writer).
    // The remainder (`0x006FF2C5`) divides by the Burst of the weapon fired,
    // where GetROF's mid-burst test read the (next) weapon GetWeapon answers.
    // A DiskLaser weapon's own path stores GetROF's value unhalved
    // (`0x006FE4A4..0x006FE4C5`); the rest of that path (the disk laser fires
    // and FireAt returns) is the unported DiskLaser delivery.
    if let Some(entity) = world.substrate.entities.get_mut(snap.stable_id) {
        let rearm = if weapon.disk_laser {
            rof
        } else {
            fireat_rearm_frames(rof, entity.berserk.active)
        };
        entity.rearm_after_fire(binary_frame as i32, rearm);
        entity.weapon_burst.complete_shot(weapon.burst.max(1));
        entity.last_fire_frame = i64::from(binary_frame);
    }
    // `0x006FF394..0x006FF43F`, after GetROF: the muzzle anim, appended to the
    // Logic vector behind this shot's bullet.
    if let Some(event) = out.fire_events.last().cloned() {
        crate::sim::world::damage_consequences::admit_muzzle_anim(world, rules, &event);
    }
    // 6FF4CC..6FF54B: successful FireAt only, after burst advance and before
    // ammo/RevealOnFire. SpawnLaser recomputes its own current GetFLH.
    super::laser::fired(
        world,
        rules,
        snap.stable_id,
        snap.target,
        selected.index,
        weapon,
        prism_support_count,
    );
    // Aircraft ammo deduction: one ammo per burst completion (not per shot).
    if !mid_burst
        && !world
            .substrate
            .entities
            .get(snap.stable_id)
            .and_then(crate::sim::aircraft::attack_state)
            .is_some()
    {
        out.ammo_deduct.push(snap.stable_id);
    }
    // `0x006FF66C`, after DecreaseAmmo (`0x006FF656`): RevealOnFire.
    if weapon.reveal_on_fire {
        reveal_on_fire(world, rules, snap.stable_id, snap.target);
    }

    fireat_tail(
        world,
        rules,
        snap,
        weapon,
        Some(bullet_id),
        overlay_registry,
    );
    Some(bullet_id)
}

/// `TechnoClass::FireAt 0x006FF749..0x006FF939`, which runs after a launched
/// and a failed shot alike: a `LimboLaunch=` weapon takes the firer off the
/// map (`0x006FF7F3`). Its `Parasite=` arm re-fires the launched `bullet`
/// with the firer as its Owner (`0x006FF825`, the tail's one bullet
/// dereference), so a parasite shot is never a failed launch.
///
/// RESIDUAL, pre-existing and not ported:
/// - FireOnce (`WeaponType+0x135`, `0x006FF8F1..0x006FF929`): a team member
///   steps its team (`0x006E9050`), then the firer drops its target
///   (Assign_Target(NULL)). Triggers: every mind-control, Psi wave, Ivan bomb,
///   disguise kit, disc drain and defuse kit shot. Effect: VERA's firer keeps
///   the target where native's lets go at once.
/// - DistributedFire (TechnoType `+0x6B0`, `0x006FF872..0x006FF8EB`): the
///   target is remembered at `+0x470`, then dropped. Trigger: the Aegis
///   Cruiser. Effect: VERA's Aegis keeps its target.
fn fireat_tail(
    world: &mut Simulation,
    rules: &RuleSet,
    snap: &AttackerSnapshot,
    weapon: &WeaponType,
    bullet: Option<u64>,
    overlay_registry: Option<&OverlayTypeRegistry>,
) {
    if weapon.limbo_launch {
        world.parasite_limbo_launch(
            snap.stable_id,
            snap.target,
            weapon,
            bullet,
            rules,
            overlay_registry,
        );
    }
}

/// What a firer shoots this frame, and its infantry fire latch: a building's
/// request carries the target its visit fired at ([`super::BuildingShot`]);
/// any other firer fires at its live target.
fn shot_target(
    entity: &crate::sim::game_entity::GameEntity,
    building_shot: Option<super::BuildingShot>,
) -> Option<TargetKind> {
    match building_shot {
        Some(shot) => Some(shot.target()),
        None => entity.attack_target.as_ref().map(|attack| attack.target),
    }
}

/// The firer's TarCom (`+0x2B4`) as FireAt reads it: a building's FireAt runs
/// inside the visit whose TarCom its request carries; any other firer's is
/// its live target.
fn fireat_tarcom(world: &Simulation, snap: &AttackerSnapshot) -> Option<TargetKind> {
    match snap.building_shot {
        Some(shot) => Some(shot.target()),
        None => world
            .substrate
            .entities
            .get(snap.stable_id)
            .and_then(|firer| firer.attack_target.as_ref())
            .map(|attack| attack.target),
    }
}

/// `TechnoClass::FireAt 0x006FE582..0x006FE622`, right after the bullet is
/// built and before the launch math, so a launch that then fails has debited
/// too. A Foot firer whose locomotor `Is_Moving` (vt `+0x10`) and whose type
/// is not `JumpJet=` (`+0xD94`) marks the bullet (`+0xB4`, read nowhere in the
/// bullet's AI, Fire or Detonate), and a marked bullet skips the debit.
/// Otherwise, unless the BulletType is `Inaccurate=` (`+0x2A2`), a TarCom
/// (`+0x2B4`) that is a Techno has `EstimateDamage(TarCom, weapon)` taken off
/// its retained estimate (`+0x70`). The weapon is the one FireAt just
/// installed on the bullet (`SetWeaponType 0x0046B260`); a shot with no
/// BulletType fires the default Inviso type, which is not Inaccurate.
fn fireat_estimate_debit(
    world: &mut Simulation,
    rules: &RuleSet,
    firer_id: u64,
    tarcom: Option<TargetKind>,
    obj: &ObjectType,
    weapon: &WeaponType,
) {
    let Some(firer) = world.substrate.entities.get(firer_id) else {
        return;
    };
    let foot = matches!(
        firer.category,
        EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
    );
    let marked = foot
        && crate::sim::movement::motion_query::is_moving(firer).unwrap_or(false)
        && !obj.jumpjet;
    let inaccurate = weapon
        .projectile
        .as_deref()
        .and_then(|id| rules.projectile(id))
        .is_some_and(|projectile| projectile.inaccurate);
    let Some(TargetKind::Entity(target_id)) = tarcom else {
        return;
    };
    if marked || inaccurate {
        return;
    }
    let estimate = super::estimated_damage_on(world, rules, firer_id, target_id, weapon);
    if let Some(target) = world.substrate.entities.get_mut(target_id) {
        target.estimated_health.debit(estimate);
    }
}

/// The rearm duration FireAt stores from GetROF's value: a berserk firer
/// (`TechnoClass+0x298`) halves it, signed and toward zero
/// (`0x006FF28F..0x006FF29C`). Native execution:
/// `tools/spatial_oracle/rearm_timer.py` (`fire` rows).
pub(crate) fn fireat_rearm_frames(get_rof: i32, berserk: bool) -> i32 {
    if berserk { get_rof / 2 } else { get_rof }
}

fn commit_fire_bookkeeping(
    world: &mut Simulation,
    rules: &RuleSet,
    emit: &mut CombatEmit,
    overlay_registry: Option<&OverlayTypeRegistry>,
) {
    let spawn_target_updates = std::mem::take(&mut emit.spawn_target_updates);
    let drain_links = std::mem::take(&mut emit.drain_links);
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
        // `0x0070FDBD`: a drained Psychic Tower frees its captives; then the
        // drainer leaves its team without idling (`0x0070FE19..0x0070FE32`).
        if crate::sim::credit_income::install_drain_link(world, drainer_id, victim_id) {
            world.free_all_captures(victim_id, rules, overlay_registry);
            world.leave_team(drainer_id, true, Some(rules));
        }
        if let Some(drainer) = world.substrate.entities.get_mut(drainer_id) {
            represented_assign_target(drainer, None);
        }
    }
}

/// Boundaries of one synchronous FireAt transaction in the event accumulator.
struct FireCommitBoundary {
    damage_start: usize,
    explosion_start: usize,
    smudge_start: usize,
    fire_event_start: usize,
}

impl FireCommitBoundary {
    fn capture(emit: &CombatEmit) -> Self {
        Self {
            damage_start: emit.damage_events.len(),
            explosion_start: emit.effects.explosion_effects.len(),
            smudge_start: emit.effects.smudge_spawn_requests.len(),
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
            fire_event_start,
        } = self;
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
        commit_fire_bookkeeping(world, rules, emit, overlay_registry);
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

fn deduct_fire_ammo(world: &mut Simulation, firers: &[u64]) {
    for &id in firers {
        if let Some(ammo) = world
            .substrate
            .entities
            .get_mut(id)
            .and_then(|entity| entity.aircraft_ammo.as_mut())
            && ammo.current > 0
        {
            ammo.current -= 1;
        }
    }
}

/// Call-local native trigger; neither variant stores a future shot.
pub(crate) enum FireVisit {
    /// Unit7365E1 Fire_At_Target then7365E8 Facing_Update.
    UnitTarget(u64),
    /// Infantry51BF59 dispatches Fire_At_Target5206B0, including Stage admission.
    InfantryTarget(u64),
    /// A mission already checked GetFireError and calls FireAt directly.
    Direct {
        id: u64,
        target: TargetKind,
        weapon_index: i32,
    },
}

/// One inline receiver transaction for both class and mission FireAt callers.
/// All emission remains in the existing emit_admitted_fire owner.
pub(crate) fn visit_fire(
    world: &mut Simulation,
    run: &mut ReceiverRun,
    visit: FireVisit,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    emit: &mut CombatEmit,
    pings: &mut Vec<UnderAttackEvent>,
) {
    let boundary = FireCommitBoundary::capture(emit);
    let remove_start = emit.remove_attack.len();
    let ammo_start = emit.ammo_deduct.len();
    let mut unit_tail = None;
    let facing_start = emit.unit_facing.len();
    #[cfg(not(test))]
    let fog_enabled = true;
    #[cfg(test)]
    let fog_enabled = world
        .receiver_fixture
        .as_ref()
        .is_none_or(|f| f.fog_enabled);
    match visit {
        FireVisit::UnitTarget(id) => {
            if unit_reaches_fire_update(world, id)
                && let Some(actor) = world.substrate.entities.get(id)
                && !(actor.passenger_role.is_inside_transport()
                    && !actor.passenger_role.in_open_transport())
            {
                let snap = (!combat_fire_gate::fire_blocked(actor))
                    .then(|| shot_target(actor, None))
                    .flatten()
                    .map(|target| build_attacker_snapshot(actor, target, None));
                let mut fire_error = None;
                if let Some(snap) = snap {
                    emit.unit_facing.push(UnitFacingUpdate {
                        entity_id: id,
                        turret_destination: None,
                        hull_destination: None,
                        turret_destination_is_idle_return: false,
                    });
                    fire_error = resolve_attacker_fire(
                        world,
                        rules,
                        overlay_registry,
                        &snap,
                        fog_enabled,
                        world.session.binary_frame,
                        world.active_wave_links.contains_key(&id),
                        emit,
                    );
                }
                unit_tail = Some((id, fire_error));
            }
        }
        FireVisit::InfantryTarget(id) => {
            let actor = world.substrate.entities.get(id);
            let target = actor.and_then(|actor| actor.attack_target.as_ref().map(|a| a.target));
            if let (Some(actor), Some(target)) = (actor, target) {
                let snap = build_attacker_snapshot(actor, target, None);
                resolve_attacker_fire(
                    world,
                    rules,
                    overlay_registry,
                    &snap,
                    fog_enabled,
                    world.session.binary_frame,
                    world.active_wave_links.contains_key(&id),
                    emit,
                );
            } else if let Some(actor) = world.substrate.entities.get_mut(id) {
                //520AD2: absence of TarCom clears the raw firing latch.
                actor.mission_leaf.set_foot_firing_sequence(0);
            }
        }
        FireVisit::Direct {
            id,
            target,
            weapon_index,
        } => {
            // These are the actual FireAt arguments, independently of TarCom.
            // Legality and dispatch cadence belong to the calling mission.
            let shot = world.substrate.entities.get(id).and_then(|actor| {
                let obj = rules.object(world.interner.resolve(actor.type_ref()))?;
                let selected = combat_weapon::resolve_weapon_index(
                    rules,
                    actor,
                    obj,
                    weapon_index,
                    &world.substrate.entities,
                    &world.interner,
                )?;
                let coords = resolve_target_coords(&target, &world.substrate.entities)?;
                let type_ref = match target {
                    TargetKind::Entity(target) => world.substrate.entities.get(target)?.type_ref(),
                    TargetKind::Cell(..) => actor.type_ref(),
                };
                Some(AdmittedFire {
                    snap: build_attacker_snapshot(actor, target, None),
                    obj,
                    selected,
                    target_coords: coords,
                    target_type_ref: type_ref,
                    is_garrison: false,
                })
            });
            if let Some(shot) = shot {
                emit_admitted_fire(
                    world,
                    rules,
                    shot,
                    world.session.binary_frame,
                    emit,
                    overlay_registry,
                );
            }
        }
    }
    boundary.commit(world, run, rules, overlay_registry, emit, pings);
    for removed in emit.remove_attack.drain(remove_start..) {
        world
            .assign_target_represented(removed, None, Some(rules))
            .expect("FireAt target receiver remains retained");
    }
    deduct_fire_ammo(world, &emit.ammo_deduct[ammo_start..]);
    emit.ammo_deduct.truncate(ammo_start);
    if let Some((id, fire_error)) = unit_tail {
        let fire_hull = if emit.unit_facing.len() > facing_start {
            emit.unit_facing.pop().and_then(|f| f.hull_destination)
        } else {
            None
        };
        if unit_reaches_fire_update(world, id) {
            world.unit_fire_update_tail(
                id,
                fire_error.map_or(
                    gattling::UnitFireOutcome::NoTarget,
                    gattling::UnitFireOutcome::Code,
                ),
                rules,
            );
        }
        if let Some(entity) = world.substrate.entities.get(id) {
            let binary_frame = world.session.binary_frame;
            let mut facing = UnitFacingUpdate::from_facing_update(
                id,
                crate::sim::movement::turret::facing_update(
                    entity,
                    &world.substrate.entities,
                    Some(rules),
                    &world.interner,
                    binary_frame,
                ),
            );
            if fire_hull.is_some() {
                facing.hull_destination = fire_hull;
            }
            crate::sim::world::unit_post::apply_unit_facing(
                &mut world.substrate.entities,
                std::slice::from_ref(&facing),
                rules,
                &world.interner,
                binary_frame,
            );
            #[cfg(test)]
            if world.receiver_fixture.is_some() {
                emit.unit_facing.push(facing);
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
    fire_requests: &super::FireRequests,
    projectile_detonations: &[ProjectileDetonation],
) -> CombatTickResult {
    let radiation_enabled = radiation_enabled(world);

    let sound_enabled = sound_enabled(world);

    let binary_frame = world.session.binary_frame;
    let fog_snapshot = world.fog.clone();
    let fog = Some(&fog_snapshot);
    #[cfg(test)]
    let fog = fog.filter(|_| {
        world
            .receiver_fixture
            .as_ref()
            .is_none_or(|fixture| fixture.fog_enabled)
    });
    let active_wave_owners: BTreeSet<_> = world.active_wave_links.keys().copied().collect();

    if tick_ms == 0 {
        return CombatTickResult {
            #[cfg(test)]
            fixture_anims: Vec::new(),
            projectile_spawns: Vec::new(),
            unit_facing: Vec::new(),
            consequences: crate::sim::world::damage_consequences::DamageConsequences::ordinary(
                DeathEffects::default(),
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

    // Pre-scan: collect entities whose attack routine does not run.
    let fire_blocked = combat_fire_gate::collect_fire_blocked_entities(&world.substrate.entities);

    let keys: Vec<u64> = world.substrate.entities.keys_sorted();

    // Infantry Guard521320 owns radiation self-fire inside its class visit.
    // There is no late global target synthesizer after all Infantry firing.

    // Phase 1: snapshot all attackers.
    let mut snapshots: Vec<AttackerSnapshot> = Vec::new();
    for &id in &keys {
        // TubeMovement owns this object's complete AI turn.  The active state
        // may already have cleared on finalization, so the world host carries
        // the entry-time suppression set into this phased combat adapter.
        if fire_suppressed.contains(&id) {
            continue;
        }
        // Entity field-reads move into `build_attacker_snapshot` (pure) below.
        let entity = match world.substrate.entities.get(id) {
            Some(e) => e,
            None => continue,
        };
        // Units and Infantry already fired in their own live Logic slots.
        // Component fixtures below invoke that same slot without a world pass.
        if matches!(
            entity.category,
            EntityCategory::Unit | EntityCategory::Infantry
        ) {
            continue;
        }
        // A closed transport's passengers left the logic walk; an
        // open-topped transport's riders stay in it and fire from inside
        // (`SetInOpenTransport @ 0x00710470`).
        if entity.passenger_role.is_inside_transport() && !entity.passenger_role.in_open_transport()
        {
            continue;
        }
        // Every Infantry already fired at its own51BF59 slot. This also
        // excludes newly born actors that never received an AI visit; the
        // legacy batch cannot grant them an extra end-of-frame firing turn.
        if entity.category == EntityCategory::Infantry || !attacker_reaches_fire(entity) {
            continue;
        }
        // An aircraft fires only in the Mission_Attack strike visit its
        // dispatch asked for (FireAt in states 4..9, `0x0041832E`), which
        // opens with its own prefix: AircraftClass::AI has no fire slot of
        // its own, and its other missions fire nothing.
        let requested = fire_requests.aircraft.contains(&id);
        if entity.category == EntityCategory::Aircraft && !requested {
            continue;
        }
        // Skip snapshot for entities blocked by locomotor state.
        let blocked = !requested && fire_blocked.contains(&id);
        // A building shoots only the FireAt its own visit asked for this
        // frame: Mission_Attack's FireAt arm or ProcessDelayedFire's expiry
        // (`techno_ai::building_missions`).
        let building_shot = fire_requests.buildings.get(&id).copied();
        if entity.category == EntityCategory::Structure && building_shot.is_none() {
            continue;
        }
        // A missing target does not acquire or drop another target.
        let Some(attack_target) = shot_target(entity, building_shot) else {
            continue;
        };
        if blocked {
            continue;
        }

        // An occupied building's fire state ([`combat_weapon::is_occupied`]),
        // then the snapshot through the shared `build_attacker_snapshot` so
        // the field-reads stay byte-identical to the per-object Fire→Facing
        // host.
        let garrison = rules
            .object(world.interner.resolve(entity.type_ref()))
            .filter(|obj| combat_weapon::is_occupied(entity, obj))
            .and_then(|_| entity.passenger_role.cargo())
            .map(|cargo| GarrisonSnapshot {
                fire_index: cargo.garrison_fire_index,
                occupant_count: cargo.count() as u8,
            });

        snapshots.push(AttackerSnapshot {
            building_shot,
            ..build_attacker_snapshot(entity, attack_target, garrison)
        });
    }
    // The remaining class hosts retain their existing phase order. Foot firers
    // have no production entry here: their complete slot ran in the live pass.
    let live_index: std::collections::HashMap<u64, usize> = live_order
        .iter()
        .enumerate()
        .map(|(i, &id)| (id, i))
        .collect();
    enum Visit {
        Attacker(AttackerSnapshot),
        #[cfg(test)]
        Foot(u64),
    }
    let mut visits: Vec<_> = snapshots.into_iter().map(Visit::Attacker).collect();
    #[cfg(test)]
    if world.receiver_fixture.is_some() {
        visits.extend(keys.iter().copied().filter_map(|id| {
            (!fire_suppressed.contains(&id)
                && world.substrate.entities.get(id).is_some_and(|actor| {
                    matches!(
                        actor.category,
                        EntityCategory::Unit | EntityCategory::Infantry
                    ) && actor.is_ai_alive()
                        && !actor.lifecycle.in_limbo
                }))
            .then_some(Visit::Foot(id))
        }));
    }
    visits.sort_by_key(|visit| {
        let id = match visit {
            Visit::Attacker(snap) => snap.stable_id,
            #[cfg(test)]
            Visit::Foot(id) => *id,
        };
        (live_index.get(&id).copied().unwrap_or(usize::MAX), id)
    });
    for visit in visits {
        let snap = match visit {
            Visit::Attacker(snap) => snap,
            #[cfg(test)]
            Visit::Foot(id) => {
                let actor = world.substrate.entities.get_mut(id).unwrap();
                let visit = if actor.category == EntityCategory::Unit {
                    FireVisit::UnitTarget(id)
                } else {
                    if actor.mission_leaf.as_infantry().is_none() {
                        actor.mission_leaf =
                            crate::sim::mission::MissionLeafState::for_entity_category(
                                EntityCategory::Infantry,
                            );
                    }
                    FireVisit::InfantryTarget(id)
                };
                visit_fire(
                    world,
                    run,
                    visit,
                    rules,
                    overlay_registry,
                    &mut emit,
                    &mut under_attack_events,
                );
                continue;
            }
        };
        let Some(live_attack) = world
            .substrate
            .entities
            .get(snap.stable_id)
            .filter(|entity| attacker_reaches_fire(entity))
            .and_then(|entity| shot_target(entity, snap.building_shot))
        else {
            continue;
        };
        let mut live_snap = snap;
        live_snap.target = live_attack;

        if fire_requests.aircraft.contains(&live_snap.stable_id) {
            aircraft_release::visit(
                world,
                run,
                rules,
                overlay_registry,
                &live_snap,
                fog,
                binary_frame,
                &mut emit,
                &mut under_attack_events,
            );
        } else {
            let boundary = FireCommitBoundary::capture(&emit);
            resolve_attacker_fire(
                world,
                rules,
                overlay_registry,
                &live_snap,
                fog.is_some(),
                binary_frame,
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
    }
    // Every projectile, missile, and live-order attack damage event emitted so
    // far is already committed; periodic radiation follows in live-victim order.
    let committed_damage_event_count = emit.damage_events.len();
    // Destructure back into the named locals for post-fire state updates.
    let CombatEmit {
        mut effects,
        projectile_spawns,
        mut damage_events,
        mut remove_attack,
        fire_events,
        ammo_deduct,
        unit_facing,
        spawn_target_updates: _,
        drain_links: _,
    } = emit;

    // Phase 3: the burst step and rearm were written in each shot's FireAt
    // emission.
    // Phase 3b: deduct ammo from aircraft that completed a burst this tick.
    deduct_fire_ammo(world, &ammo_deduct);

    // Phase 3.5: fold radiation-emitting detonations into the field, then
    // collect the periodic radiation damage. The original applies this damage
    // inside each foot unit's own AI step, gated on the global frame counter;
    // the phased engine collects it here so deaths route through the same
    // death pipeline as weapon damage (death anim selection via the
    // RadSiteWarhead, destruction bookkeeping, survivor ejection).
    if let Some(rad) = radiation_enabled.then_some(&mut world.radiation) {
        for det in effects.rad_detonations.drain(..) {
            rad.apply_detonation(
                det,
                binary_frame,
                &rules.radiation,
                world.resolved_terrain.as_ref(),
                &mut Vec::new(),
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
                    // (transported) and objects in the air are exempt.
                    // FootClass::AI asks vt+0x54, IsInAir (`0x004DA588`).
                    if entity.category == EntityCategory::Structure
                        || entity.dying
                        || !entity.is_alive()
                        || entity.immune_to_radiation
                        || entity.passenger_role.is_inside_transport()
                        || crate::sim::movement::air_movement::is_high_flying(
                            entity,
                            world.resolved_terrain.as_ref(),
                            Some((rules, &world.interner)),
                        )
                    {
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
                &mut Vec::new(),
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
            crate::sim::mission::concrete_effects::represented_assign_target(entity, None);
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
        #[cfg(test)]
        fixture_anims: Vec::new(),
        projectile_spawns,
        unit_facing,
        consequences: crate::sim::world::damage_consequences::DamageConsequences::ordinary(
            effects,
            under_attack_events,
            run.navigation_changed_cells.clone(),
            fire_events,
        ),
    }
}

#[inline]
pub(super) fn callbacks_enabled(_world: &Simulation) -> bool {
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

#[cfg(test)]
mod reveal_on_fire_tests {
    use super::*;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::house_state::HouseState;

    /// A computer Soviet tank at (10, 10) shooting a human American tank at
    /// (12, 10), in a multiplayer game.
    fn world() -> (Simulation, RuleSet) {
        let rules = RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[VehicleTypes]\n0=MTNK\n[MTNK]\nStrength=300\n",
        ))
        .unwrap();
        let mut sim = Simulation::new();
        sim.fog.width = 32;
        sim.fog.height = 32;
        sim.session.game_mode_nonzero = true;
        for (name, human) in [("Americans", true), ("Soviet", false)] {
            let id = sim.interner.intern(name);
            sim.houses
                .insert(id, HouseState::new(id, 0, None, human, 0, 10));
        }
        for (id, owner, rx) in [(1, "Soviet", 10), (2, "Americans", 12)] {
            let mut entity = GameEntity::test_default(id, "MTNK", owner, rx, 10);
            entity.owner = sim.interner.intern(owner);
            entity.type_ref = sim.interner.intern("MTNK");
            sim.substrate.entities.insert(entity);
        }
        (sim, rules)
    }

    fn revealed(sim: &Simulation, house: &str, rx: u16, ry: u16) -> bool {
        sim.fog
            .is_cell_revealed(sim.interner.get(house).unwrap(), rx, ry)
    }

    #[test]
    fn a_shot_from_shroud_reveals_the_firer_to_its_human_victim() {
        let (mut sim, rules) = world();
        reveal_on_fire(&mut sim, &rules, 1, TargetKind::Entity(2));
        assert!(revealed(&sim, "Americans", 10, 10));
        assert!(revealed(&sim, "Americans", 10, 13), "radius 3");
        assert!(!revealed(&sim, "Americans", 10, 14));
        assert!(
            !revealed(&sim, "Soviet", 10, 10),
            "the firer's map is untouched"
        );
    }

    #[test]
    fn no_reveal_for_a_computer_victim_or_a_mapped_firer_cell() {
        let (mut sim, rules) = world();
        reveal_on_fire(&mut sim, &rules, 2, TargetKind::Entity(1));
        assert!(!revealed(&sim, "Soviet", 12, 10), "a computer victim");

        let (mut sim, rules) = world();
        let americans = sim.interner.get("Americans").unwrap();
        crate::sim::vision::reveal_radius(&mut sim.fog, americans, 10, 10, 1);
        assert!(!revealed(&sim, "Americans", 10, 12));
        reveal_on_fire(&mut sim, &rules, 1, TargetKind::Entity(2));
        assert!(
            !revealed(&sim, "Americans", 10, 12),
            "the firer's own cell is already mapped: IsShrouded is false"
        );

        let (mut sim, rules) = world();
        reveal_on_fire(&mut sim, &rules, 1, TargetKind::Cell(12, 10));
        assert!(!revealed(&sim, "Americans", 10, 10), "a cell target");
    }
}

#[cfg(test)]
#[path = "ifv_area_receipt_tests.rs"]
mod ifv_area_receipt_tests;

#[cfg(test)]
#[path = "nuke_maker_tests.rs"]
mod nuke_maker_tests;
