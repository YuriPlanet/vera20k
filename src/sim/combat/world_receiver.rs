//! World-owned synchronous receiver execution.
//!
//! Gameplay authorities stay in Simulation across nested receiver/lifecycle
//! calls. Only recursion guards and ordered publication receipts are local.

use super::world_fire::{object_get_coords, retaliation_reaches};
use super::*;
use crate::sim::movement::{DamageRetaliation, ScatterFlags};
use crate::sim::world::techno_ai_cloak::emit_configured_cloak_sound;
use crate::sim::world::{FrameEffects, Simulation};

fn respond_to_base_attack(
    world: &mut Simulation,
    rules: &RuleSet,
    _site: BaseDefenseResponseCallSite,
    victim_id: u64,
    attacker_id: u64,
    effects: FrameEffects<'_>,
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
                health: victim.health.current(),
                last_attacker_house_index,
            });
        return;
    }
    base_defense_response::respond_to_base_attack(world, rules, victim_id, attacker_id, effects);
}

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
pub(crate) fn apply_area_damage(
    world: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    impact: ProjectileCoord,
    damage: i32,
    warhead: &WarheadType,
    origin: (u64, Option<InternedId>, InternedId),
    effects: FrameEffects<'_>,
) -> bool {
    #[cfg(test)]
    super::receiver_fixture::observe_area_visit(world, impact, damage, warhead, origin);
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
        world.commit_noncombat_aoe_receivers(rules, overlay_registry, &area.aoe.receivers, effects);
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
        effects,
    );
    receipt.bridge_state_changed || bridge_continued
}

/// [`apply_area_damage`] inside a damage transaction (a TerrainClass death's
/// C4 at `0x0071BABF`, a dying carrier's bomb at `0x0070267F`): the receivers
/// commit in `run`, then the bridge continuation. Their effects and pings go
/// back to the receiver that set the area off.
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
        run.effects(),
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

pub(super) fn commit_smudges(
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
pub(crate) struct ReceiverRun<'a> {
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
    effects: FrameEffects<'a>,
}

impl<'a> ReceiverRun<'a> {
    pub(crate) fn with_effects(mut self, effects: FrameEffects<'a>) -> Self {
        self.effects = effects;
        self
    }

    pub(crate) const fn effects(&self) -> FrameEffects<'a> {
        self.effects
    }

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
        area_near_center_ic_isolation_armed(receivers, &world.substrate.entities, current_tick);
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
            target.lifecycle.object_alive()
                && !(target.category() == EntityCategory::Structure
                    && rules
                        .object(world.interner.resolve(target.type_ref()))
                        .is_some_and(|object| object.invisible_in_game))
                && target.health.current() > 0
                && target.lifecycle.cell_marked
                && !target.lifecycle.in_limbo()
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
        &world.interner,
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
        near_center_ic_isolation_armed(damage_events, &world.substrate.entities, current_tick)
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
                target.category() == EntityCategory::Unit
                    && target.selected
                    && world.session.current_house == Some(target.owner())
            })
        {
            run.selected_units.push(target_id);
        }
        match apply_building_receive_prelude(
            event,
            &world.substrate.entities,
            rules,
            &world.interner,
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
                        run.effects(),
                    );
                }
            }
            BuildingReceivePrelude::Continue => {}
        }
        // `UnitClass::ReceiveDamage 0x00737CB6..0x00737D2E`: a unit still
        // inside its war factory returns 0 before FootClass::ReceiveDamage.
        if !event.receiver_flags.ignore_defenses && world.unit_inside_war_factory(target_id, rules)
        {
            continue;
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
                .filter(|target| target.category() == EntityCategory::Structure)
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
            run.effects(),
        );
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
        // `0x00701D71..0x00701DB4`: a Psychedelic hit that turns its target
        // berserk takes it out of its team, with its idle order
        // (`0x006EA870`), clears its target (`vt+0x3C8(0)`) and queues Hunt
        // (`vt+0x1E8(0xF, 0)`). The removal runs here, before the receiver's
        // timer and flag writes, which it does not read; the other two follow
        // them, as natively.
        let starts_berserk = receiver_outcome
            .and_then(|resolved| resolved.outcome.psychedelic_value)
            .is_some()
            && world
                .substrate
                .entities
                .get(target_id)
                .is_some_and(|target| !target.berserk.active);
        if starts_berserk {
            world.leave_team(target_id, false, Some(rules), run.effects());
        }
        let receiver_callbacks = callbacks_enabled(world);
        let Some(receiver_health::ReceiverHealthCommit {
            building_entry_frame,
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
            threat_feedback,
            psychedelic,
        }) = receiver_health::commit_receiver_health(
            event,
            world,
            rules,
            overlay_registry,
            receiver_outcome,
            run.effects(),
            receiver_callbacks,
        )
        else {
            continue;
        };
        // Generic Object damage Tags have already returned. This later
        //kill continuation reads the source's current Owner; the separately
        //supplied source-House ABI pointer stays fixed for the receive call.
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
        if starts_berserk {
            // The class setter: Infantry's (`0x0051B1F0`) also resets its
            // action and path head.
            let _ = world.assign_target_represented(
                target_id,
                None,
                Some(rules),
                overlay_registry,
                run.effects(),
            );
            let _ = world.mission_queue_exact(
                target_id,
                MissionId::from_known(MissionType::Hunt),
                0,
                world.session.binary_frame,
                &crate::sim::mission::authority::EntityReadyInputProvider,
            );
        }

        // Techno701900 result1 (switch table702D24 entry 1) selects
        // 702713..7027A9: two distinct Voc750E20 retained-coordinate requests
        // at direct Location+9C. The arm's own tail7027AE (only reached from
        // 702775) holds the protected-Techno call7027E9 before common7027F7.
        // Each request reaches the existing synchronous sound boundary before
        // later receiver consequences can spend Main.
        // The exact-zero override70202E takes the death arm instead, and the
        // Psychedelic arm returns its result 1 before this tail.
        let result_one_arm =
            receive_state == damage::DamageState::Damaged && !entered_techno_death && !psychedelic;
        if result_one_arm && sound_enabled {
            for _ in 0..2 {
                if let Some(target) = world.substrate.entities.get(target_id)
                    && let Some(sound_id) = rules
                        .object(world.interner.resolve(target.type_ref()))
                        .and_then(|object| object.damage_sound())
                {
                    let coordinate =
                        crate::sim::movement::ground_pose::position_world_coord(&target.position);
                    world.sound_events.push(SimSoundEvent::VocAtCoord {
                        sound_id: sound_id.to_owned(),
                        coordinate,
                        //702734/70277D PUSH0 before each750E20;
                        //executed coordinate_attempts retain args[0].
                        flags: 0,
                    });
                    run.effects().flush_sound(world, Some(rules));
                }
            }
        }

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
        // 0x007027AE..0x007027EE`, the tail of result 1's own arm, invokes the
        // protected-Techno response after ObjectClass has committed
        // health/visual state. Unaffected hits, threshold crossings and kills
        // never reach it. `ShouldProtect+0x3CF` has no active YR writer;
        // `ToProtect=` is the exact live gate retained here. An owner a human
        // controls (`0x0050B730` at `0x007027D2`) refuses.
        let protected_techno_response = result_one_arm
            && attacker_id != RAD_NO_ATTACKER
            && world
                .substrate
                .entities
                .get(target_id)
                .is_some_and(|target| {
                    rules
                        .object(world.interner.resolve(target.type_ref()))
                        .is_some_and(|object| object.to_protect)
                        && world.houses.contains_key(&target.owner())
                        && !world.owner_is_human(target.owner())
                });
        if protected_techno_response {
            respond_to_base_attack(
                world,
                rules,
                BaseDefenseResponseCallSite::ProtectedTechno,
                target_id,
                attacker_id,
                run.effects(),
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
            //Object5F5772..579A chooses E4 only for a supplied sourceHouse
            //that differs from the source Techno's CURRENT Owner (or NULL
            //Techno). Do not turn that House pointer into an E0 attacker.
            let live_source_owner = (attacker_id != RAD_NO_ATTACKER)
                .then(|| {
                    world
                        .substrate
                        .entities
                        .get(attacker_id)
                        .map(GameEntity::owner)
                })
                .flatten();
            let source = if let Some(house) = event.source_house
                && (attacker_id == RAD_NO_ATTACKER || live_source_owner != Some(house))
            {
                super::KillSource::House(Some(house))
            } else {
                super::KillSource::Techno((attacker_id != RAD_NO_ATTACKER).then_some(attacker_id))
            };
            world.record_the_kill(
                target_id,
                source,
                super::KillCallback::Terminal,
                crate::sim::world::UninitContext::new(Some(rules), overlay_registry)
                    .with_effects(run.effects()),
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
                crate::sim::world::UninitContext::new(Some(rules), overlay_registry)
                    .with_effects(run.effects()),
            );
        }
        if postmortem_candidate.is_some() && callbacks_enabled(world) {
            world.apply_fatal_lifecycle_stage(
                rules,
                FatalLifecycleStage::PostMortemExactZero {
                    killer_owner: attacker_owner,
                },
                target_id,
                fatal_category,
                crate::sim::world::UninitContext::with_rules(rules)
                    .with_registry(overlay_registry)
                    .with_effects(run.effects()),
            );
        };
        if let Some((victim_owner, source_owner, final_damage, strength, cost)) = threat_feedback {
            let delta = receiver_anger_delta(final_damage, strength, cost);
            update_anger_nodes(
                &mut world.houses,
                &world.session.house_order,
                &world.house_alliances,
                &world.interner,
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
            // PostMortem's restore (`0x00701F60`, `0x00701F67`).
            target.lifecycle.restore_alive();
            target.health.set(1);
            target.death.restore();
            #[cfg(test)]
            death
                .receiver_stage_trace
                .push(ReceiverStageTrace::PostMortem { target_id });
            continue;
        }
        if latch_hostile_hit && let Some(target) = world.substrate.entities.get_mut(target_id) {
            target.ai_sell_repair.hit_by_enemy();
        }

        if let Some((category, state)) = smoke_maintenance
            && callbacks_enabled(world)
        {
            world.apply_fatal_lifecycle_stage(
                rules,
                FatalLifecycleStage::MaintainDamageSmoke { state },
                target_id,
                category,
                crate::sim::world::UninitContext::with_rules(rules)
                    .with_registry(overlay_registry)
                    .with_effects(run.effects()),
            );
        };
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

        // TechnoClass::ReceiveDamage's retaliation tail. `ShouldRetaliate`
        // (`0x007087C0`, called at `0x00702A43`) answers 0 for a null source.
        // An accepted source within reach takes `Override_Mission`
        // (`vt+0x1F4`, `0x00702B41`); either answer then reaches the scatter
        // tail (`0x00702B47..0x00702D0B`), whose Scatter can run its locomotor
        // Process at once.
        if synchronous_retaliation {
            let retaliates = attacker_id != RAD_NO_ATTACKER && {
                #[cfg(test)]
                death
                    .receiver_stage_trace
                    .push(ReceiverStageTrace::ShouldRetaliate { target_id });
                combat_targeting::should_retaliate(world, rules, target_id, attacker_id)
            };
            if retaliates && retaliation_reaches(world, rules, target_id, attacker_id) {
                world.override_mission_on_damage_response(
                    target_id,
                    attacker_id,
                    rules,
                    run.effects(),
                );
            }
            let retaliation = if retaliates {
                DamageRetaliation::Accepted
            } else {
                DamageRetaliation::Refused
            };
            match world.receive_damage_scatter(
                target_id,
                retaliation,
                rules,
                overlay_registry,
                run.effects(),
            ) {
                Ok(changed) => death.bridge_state_changed |= changed,
                Err(cause) => log::debug!("damaged {target_id} did not scatter: {cause}"),
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
                        crate::sim::world::UninitContext::with_rules(rules)
                            .with_registry(overlay_registry)
                            .with_effects(run.effects()),
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
                                .with_registry(overlay_registry)
                                .with_effects(run.effects()),
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

        // Building442425 has returned from the complete Techno receiver,
        // including its retaliation tail, before this live dispatch:
        // alive44242C, results2/3 at442476 and resolved type+538==-1 at4426D2.
        // The global Rules+714 request442706 uses direct Location+9C, not
        // virtual+48/GetCoords. Read here instead of latching admission and
        // coordinates before callbacks. Flush before Building's own response.
        // Residual outside this no-emitter route: result2's non-null gap
        // generator +30C/+E8 multiplication at4426AC precedes this gate.
        if matches!(
            receive_state,
            damage::DamageState::Yellow | damage::DamageState::Red
        ) && sound_enabled
            && let Some(target) = world.substrate.entities.get(target_id)
            && target.category() == EntityCategory::Structure
            && target.lifecycle.object_alive()
            && rules
                .object(world.interner.resolve(target.type_ref()))
                .is_some_and(|object| object.damage_sound().is_none())
            && let Some(sound_id) = rules.general.building_damage_sound()
        {
            let coordinate =
                crate::sim::movement::ground_pose::position_world_coord(&target.position);
            world.sound_events.push(SimSoundEvent::VocAtCoord {
                sound_id: sound_id.to_owned(),
                coordinate,
                //4426E1 PUSH0 before750E20 at442706 (same executed owner).
                flags: 0,
            });
            run.effects().flush_sound(world, Some(rules));
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
            .is_some_and(|target| target.category() != EntityCategory::Structure)
        {
            world.team_took_damage(
                target_id,
                (attacker_id != RAD_NO_ATTACKER).then_some(attacker_id),
                rules,
            );
        }
        // FootClass::ReceiveDamage's own tail, then the concrete Foot
        // receivers once it returns (`0x00518042`, `0x00737D52`), all after
        // the retaliation and scatter tail and the team report above. Both
        // concrete receivers return on 5 and take 4 to their death arms;
        // Infantry returns on 0 (`0x0051805B`) and Unit skips its ping on 0
        // (`0x007384BB`), so what follows runs for 1..=3.
        if let Some((damage, survivor_tail, _hostile_source)) = positive_postlude
            && survivor_tail
            && matches!(
                receive_state,
                damage::DamageState::Damaged
                    | damage::DamageState::Yellow
                    | damage::DamageState::Red
            )
        {
            // The source's Location, read after the Techno tail; a source no
            // longer represented is a null source.
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
            // FootClass (`0x004D746D..0x004D74C7`): a teamless Foot hit by a
            // source, whose current mission's MissionControl sets `NoThreat=`
            // and clears `Zombie=` (`0x005B3A00` +4, +5), takes
            // `Enter_Idle_Mode(0, 1)` (vt+0x484). Retail `[Harmless]` is such
            // a mission.
            if attacker_coord.is_some()
                && world
                    .substrate
                    .entities
                    .get(target_id)
                    .is_some_and(|target| {
                        matches!(
                            target.category(),
                            EntityCategory::Infantry
                                | EntityCategory::Unit
                                | EntityCategory::Aircraft
                        ) && crate::sim::mission::control::current_control(rules, &target.mission)
                            .is_some_and(|entry| entry.no_threat && !entry.zombie)
                    })
                && world.team_script_vm.team_for_member(target_id).is_none()
            {
                crate::sim::world::enter_idle_mode(
                    world,
                    target_id,
                    rules,
                    overlay_registry,
                    run.effects(),
                );
            }
            let (is_infantry, has_locomotor) =
                world
                    .substrate
                    .entities
                    .get(target_id)
                    .map_or((false, false), |target| {
                        (
                            target.category() == EntityCategory::Infantry,
                            target.locomotor.is_some(),
                        )
                    });
            // InfantryClass (`0x00518BF2..0x00518C3F`): a computer's
            // `Engineer=` man whose Get_Mission (vt+0x184) is Guard or
            // Area_Guard queues Hunt, with or without a source.
            let engineer_hunts = is_infantry
                && world
                    .substrate
                    .entities
                    .get(target_id)
                    .is_some_and(|target| {
                        !world.owner_is_human(target.owner())
                            && rules
                                .object(world.interner.resolve(target.type_ref()))
                                .is_some_and(|obj| obj.engineer)
                            && matches!(
                                target.mission.effective().known(),
                                Some(MissionType::Guard | MissionType::AreaGuard)
                            )
                    });
            if engineer_hunts && let Some(target) = world.substrate.entities.get_mut(target_id) {
                queue_entity_mission_deferred(target, MissionId::from_known(MissionType::Hunt));
            }
            // Then, only with a source, `Scatter(source Location, 0, 0)`
            // (`0x00518C56`) before the fear latch; a null source jumps to the
            // fear ladder (`0x00518C47`). A locomotor-less man is refused, as
            // in `Simulation::receive_damage_scatter`.
            if is_infantry
                && has_locomotor
                && let Some(source) = attacker_coord
                && let Err(cause) = world.infantry_scatter_from(
                    target_id,
                    source,
                    ScatterFlags::new(false, false),
                    rules,
                    overlay_registry,
                    run.effects(),
                )
            {
                log::debug!(
                    "damaged infantry {target_id} did not scatter from its source: {cause}"
                );
            }
            if let Some(target) = world.substrate.entities.get_mut(target_id) {
                if let Some(obj) = rules.object(world.interner.resolve(target.type_ref())) {
                    infantry::apply_fear_from_damage(
                        obj,
                        target,
                        damage,
                        attacker_coord.is_some(),
                        rules.general.condition_red,
                        rules.general.condition_yellow,
                    );
                }
                // UnitClass (`0x007384B9..0x00738530`): the `Harvester=` ping,
                // with or without a source. A building's ping is its
                // retaliation block's, below.
                if target.category() != EntityCategory::Structure
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
        }
        // UnitClass's computer-owner response after its ping
        // (`0x00738535..0x0073866E`), for results 0 to 3: result 0 jumps
        // there past the ping (`0x007384BB`).
        if !entered_techno_death
            && matches!(
                receive_state,
                damage::DamageState::Unaffected
                    | damage::DamageState::Damaged
                    | damage::DamageState::Yellow
                    | damage::DamageState::Red
            )
        {
            world.unit_damage_response(
                target_id,
                (attacker_id != RAD_NO_ATTACKER).then_some(attacker_id),
                rules,
                run.effects(),
            );
        }
        if receive_state != damage::DamageState::AlreadyDead
            && world
                .substrate
                .entities
                .get(target_id)
                .is_some_and(|target| {
                    target.category() == EntityCategory::Structure
                        && target.lifecycle.object_alive()
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
    if !target.lifecycle.object_alive() {
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
            if e.category() == EntityCategory::Structure {
                structure_destroyed = true;
            }
            let world_z_leptons = object_world_z_leptons(e, world.resolved_terrain.as_ref());
            (
                e.type_ref(),
                e.position.rx,
                e.position.ry,
                world_z_leptons,
                e.owner(),
                e.category(),
                e.veterancy.rank(),
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
                world.free_slaves(
                    dead_id,
                    killer,
                    None,
                    rules,
                    overlay_registry,
                    run.effects(),
                );
            }
            // `0x00702112`: the death arm frees a controller's captives before
            // its death sounds (their fate draws precede the debris draws).
            if callbacks_enabled(world) {
                world.free_all_captures(dead_id, rules, overlay_registry, run.effects());
            }
            let type_id_str = world.interner.resolve(type_id);
            if let Some(obj) = rules.object(type_id_str) {
                append_selected_death_sounds(
                    obj,
                    category,
                    rules.general.building_die_sound.as_deref(),
                    world.houses.get(&owner).is_some_and(|house| house.is_human),
                    &world.main_rng,
                    rx,
                    ry,
                    &mut death_sounds,
                );
                // 0x00702206..0x00702210: RADIO OVER_OUT to every contact and
                // the Stun, between the death sounds and the debris.
                if callbacks_enabled(world) {
                    world.techno_death_stun(
                        dead_id,
                        crate::sim::world::UninitContext::new(Some(rules), overlay_registry)
                            .with_effects(run.effects()),
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
                // Techno702232..70227B skips this block only for a low
                // dropped bomb over water; ordinary water deaths still throw.
                if world.dropped_bomb_water_death_location(dead_id).is_none() {
                    throw_debris_for_death(
                        world,
                        obj,
                        rules,
                        owner,
                        glam::IVec3::new(center.0, center.1, world_z_leptons),
                        &mut voxel_debris,
                        &mut explosion_effects,
                    );
                }
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
                    world.kill_passengers(
                        dead_id,
                        attacker,
                        rules,
                        overlay_registry,
                        run.effects(),
                    );
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
                    building.category() == EntityCategory::Structure
                        && building.lifecycle.object_alive()
                })
        {
            world.building_now_dead_contacts(dead_id, contacts, Some(rules), run.effects());
        }
        finish_concrete_death(
            world,
            dead_id,
            run.selected_units.contains(&dead_id),
            damage_events,
            rules,
            overlay_registry,
            &mut effects,
            run.effects(),
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
    frame_effects: FrameEffects<'_>,
) {
    let Some(entity) = world.substrate.entities.get(dead_id) else {
        return;
    };
    let category = entity.category();
    // Building442230 returns before its result dispatch when Alive is clear.
    if category == EntityCategory::Structure && !entity.lifecycle.object_alive() {
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
                world.begin_ship_sinking(dead_id, rules, frame_effects);
            } else if !world.unit_dropped_bomb_water_splash(
                rules,
                dead_id,
                &mut effects.explosion_effects,
            ) {
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
        world.foot_mark_remove(dead_id, Some(rules), overlay_registry, frame_effects);
        let dying = crate::sim::crew_survival::DyingTransport {
            attacker: killing_attacker(world, damage_events, dead_id),
            ignore_defenses,
            selected_by_player,
        };
        world.release_dying_unit_passengers(rules, overlay_registry, dead_id, dying, frame_effects);
        world.spawn_vehicle_crew(
            rules,
            overlay_registry,
            dead_id,
            prevent_crew_escape,
            selected_by_player,
            frame_effects,
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
            frame_effects,
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
            entity.death.begin();
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
            frame_effects,
        )
    {
        // `AircraftClass::ReceiveDamage` (`0x00416694..0x004166A3`): an
        // airborne aircraft crashes instead of its UnInit. It stays alive and
        // represented, with Health 0, until its fall's impact.
        effects.despawned_ids.push(dead_id);
    } else if crashable
        && callbacks_enabled(world)
        && world.foot_crash(dead_id, None, rules, overlay_registry, frame_effects)
    {
        // `UnitClass::ReceiveDamage` (`0x00738457..0x00738475`): an airborne
        // `Crashable=` unit crashes (`Crash(0)`, no attacker) instead of its
        // UnInit, and falls to its impact (a Jumpjet: `jumpjet_crash_impact`).
        effects.despawned_ids.push(dead_id);
    } else if category == EntityCategory::Unit
        && !crashable
        && world
            .substrate
            .entities
            .get(dead_id)
            .is_some_and(|entity| entity.sinking.is_active())
    {
        // Unit738493 checks +3CD after Mark(UP), passenger/crew handling and
        // Crashable; a Crashable type its Crash refused is UnInit even when
        // sinking (`0x0073847F`). A sinking hull retains Alive and its
        // Logic/display slot.
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
                let uninit = callbacks_enabled(world).then(|| {
                    crate::sim::world::UninitContext::new(Some(rules), overlay_registry)
                        .with_effects(frame_effects)
                });
                postlude.commit(world, rules, effects, uninit);
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
                        frame_effects,
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
/// RESIDUAL: the ElectricAssault (`0x0046937A`), Airstrike (`0x00469705`)
/// and DirectRocker (`0x0046978E`) bodies are not ported; those arms claim
/// the impact and do nothing else (see `SpecialDetonationAction`).
fn run_special_detonation_arm(
    world: &mut Simulation,
    rules: &RuleSet,
    action: SpecialDetonationAction,
    detonation: &ProjectileDetonation,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
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
                            target.category(),
                            EntityCategory::Unit
                                | EntityCategory::Infantry
                                | EntityCategory::Aircraft
                        )
                    })
                });
                world.parasite_attach(owner, victim, rules, effects);
            }
        }
        SpecialDetonationAction::MindControl => {
            world.mind_control_detonation(owner, object, rules, overlay_registry, effects);
        }
        SpecialDetonationAction::Temporal => {
            let target = match target {
                SpecialArmTarget::Object(id) => {
                    crate::sim::temporal::TemporalShotTarget::Object(id)
                }
                SpecialArmTarget::Cell => crate::sim::temporal::TemporalShotTarget::Cell,
                SpecialArmTarget::None => crate::sim::temporal::TemporalShotTarget::None,
            };
            world.temporal_detonation(owner, target, rules, overlay_registry, effects);
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
        SpecialDetonationAction::MakesDisguise => {
            // 0x00469A03..0x00469A2A: no bullet Owner (+0xB0) -> nothing;
            // otherwise Owner->DisguiseAs(Target) through vt+0x46C.
            if world.substrate.entities.contains(owner) {
                world.disguise_as(owner, object, !matches!(target, SpecialArmTarget::None));
            }
        }
        SpecialDetonationAction::Locomotor => {
            // 0x004694CB..0x00469700: the bullet's strength (+0x6C) and the
            // warhead's Locomotor= class (+0x15C).
            let Some(warhead) = rules.warhead(world.interner.resolve(detonation.payload.warhead))
            else {
                return;
            };
            let target = match target {
                SpecialArmTarget::Object(id) => {
                    crate::sim::locomotor_lift::LocomotorShotTarget::Object(id)
                }
                SpecialArmTarget::Cell => crate::sim::locomotor_lift::LocomotorShotTarget::Cell,
                SpecialArmTarget::None => crate::sim::locomotor_lift::LocomotorShotTarget::None,
            };
            world.locomotor_detonation(
                owner,
                target,
                detonation.payload.base_damage,
                warhead.locomotor,
                rules,
                overlay_registry,
                effects,
            );
        }
        SpecialDetonationAction::ElectricAssault
        | SpecialDetonationAction::Airstrike
        | SpecialDetonationAction::DirectRocker => {
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
    effects: FrameEffects<'_>,
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
        effects,
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
    effects: FrameEffects<'_>,
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
                .is_some_and(|entity| entity.category() == EntityCategory::Unit),
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
            run_special_detonation_arm(
                world,
                rules,
                claimed,
                detonation,
                overlay_registry,
                effects,
            );
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
        out.effects
            .combat_light_requests
            .push(CombatLightRequest::Impact {
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
                run.effects(),
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
                    run.effects(),
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
pub(super) fn sound_enabled(_world: &Simulation) -> bool {
    #[cfg(test)]
    if let Some(fixture) = _world.receiver_fixture.as_ref() {
        return fixture.sound_enabled;
    }
    true
}

#[inline]
pub(super) fn fog_enabled(_world: &Simulation) -> bool {
    #[cfg(test)]
    if let Some(fixture) = _world.receiver_fixture.as_ref() {
        return fixture.fog_enabled;
    }
    true
}

#[inline]
pub(super) fn radiation_enabled(_world: &Simulation) -> bool {
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
#[path = "ifv_area_receipt_tests.rs"]
mod ifv_area_receipt_tests;

#[cfg(test)]
#[path = "nuke_maker_tests.rs"]
mod nuke_maker_tests;

/// Return the active wall-overlay flags at a cell, if available.
fn wall_overlay_flags_at<'a>(
    overlay_grid: Option<&OverlayGrid>,
    overlay_registry: Option<&'a OverlayTypeRegistry>,
    rx: u16,
    ry: u16,
) -> Option<&'a crate::rules::overlay_types::OverlayTypeFlags> {
    let (Some(grid), Some(registry)) = (overlay_grid, overlay_registry) else {
        return None;
    };
    grid.cell(rx, ry)
        .overlay_id
        .and_then(|id| registry.flags(id))
        .filter(|flags| flags.wall)
}

fn warhead_damages_wall(
    warhead: &WarheadType,
    wall_flags: &crate::rules::overlay_types::OverlayTypeFlags,
) -> bool {
    warhead.wall || warhead.wall_absolute_destroyer || (warhead.wood && wall_flags.armor_is_wood)
}
