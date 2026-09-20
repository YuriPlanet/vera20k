//! Compatibility inputs for phase-level tests of the production receiver.
//! This module and its temporary world transfers do not exist in production.

use super::*;
use crate::sim::world::Simulation;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct BaseDefenseResponseTraceEntry {
    pub(crate) site: BaseDefenseResponseCallSite,
    pub(crate) victim_id: u64,
    pub(crate) health: i32,
    pub(crate) last_attacker_house_index: i32,
}

#[derive(Default)]
pub(crate) struct FixtureTrace {
    pub(crate) entries: Vec<BaseDefenseResponseTraceEntry>,
}

/// Fixture omissions, never serialized or installed on a returned world.
pub(crate) struct FixturePolicy {
    pub(crate) current_tick: u64,
    pub(crate) require_playfield_membership: bool,
    pub(crate) fog_enabled: bool,
    pub(crate) radiation_enabled: bool,
    pub(crate) sound_enabled: bool,
    pub(crate) terrain_collection: bool,
    pub(crate) trace: FixtureTrace,
    pub(crate) deferred_tiberium: Vec<TiberiumReductionRequest>,
}

/// Adapters preserve the old phase's supplied state, without bootstrapping a
/// scenario or admitting fixture objects into the production Logic vector.
#[allow(clippy::too_many_arguments)]
fn with_world<R>(
    entities: &mut EntityStore,
    occupancy: &mut OccupancyGrid,
    interner: &mut StringInterner,
    mut houses: Option<&mut BTreeMap<InternedId, HouseState>>,
    main_rng: Option<&mut SimRng>,
    scenario_rng: &mut SimRng,
    overlay_grid: Option<&mut OverlayGrid>,
    terrain: Option<&mut ResolvedTerrainGrid>,
    mut terrain_area: Option<&mut TerrainAreaState>,
    mut sound_sink: Option<&mut Vec<SimSoundEvent>>,
    mut handled_deaths: Option<&mut Vec<u64>>,
    trace: Option<&mut FixtureTrace>,
    configure_and_run: impl FnOnce(&mut Simulation, &mut world_receiver::ReceiverRun) -> R,
) -> R {
    let mut world = Simulation::new();
    world.substrate.entities = std::mem::take(entities);
    world.substrate.occupancy = std::mem::take(occupancy);
    world.interner = std::mem::take(interner);
    if let Some(houses) = houses.as_deref_mut() {
        world.houses = std::mem::take(houses);
    }
    if let Some(rng) = main_rng.as_deref() {
        world.main_rng = rng.clone();
    }
    world.scenario_rng = scenario_rng.clone();
    world.overlay_grid = overlay_grid.as_deref().cloned();
    world.resolved_terrain = terrain.as_deref().cloned();
    if let Some(events) = sound_sink.as_deref_mut() {
        world.sound_events = std::mem::take(events);
    }
    let mut run = world_receiver::ReceiverRun::default();
    if let Some(handled) = handled_deaths.as_deref_mut() {
        run.handled_deaths = std::mem::take(handled);
    }
    if let Some(area) = terrain_area.as_deref_mut() {
        area.swap_authority(
            &mut world.production,
            &mut world.substrate.raw_cell_occupation,
        );
        (run.navigation_changed_cells, run.finalizing_terrain) = area.take_fixture_progress();
    }
    world.receiver_fixture = Some(FixturePolicy {
        current_tick: 0,
        require_playfield_membership: false,
        fog_enabled: false,
        radiation_enabled: false,
        sound_enabled: sound_sink.is_some(),
        terrain_collection: terrain_area.is_some(),
        trace: FixtureTrace::default(),
        deferred_tiberium: Vec::new(),
    });

    let result = configure_and_run(&mut world, &mut run);

    let policy = world
        .receiver_fixture
        .take()
        .expect("private fixture world");
    assert!(
        policy.deferred_tiberium.is_empty(),
        "each collection owns its deferred output"
    );
    if let Some(trace) = trace {
        trace.entries.extend(policy.trace.entries);
    }
    if let Some(area) = terrain_area {
        area.swap_authority(
            &mut world.production,
            &mut world.substrate.raw_cell_occupation,
        );
        area.restore_fixture_progress(run.navigation_changed_cells, run.finalizing_terrain);
    }
    if let Some(handled) = handled_deaths {
        *handled = run.handled_deaths;
    }
    *entities = world.substrate.entities;
    *occupancy = world.substrate.occupancy;
    *interner = world.interner;
    if let Some(houses) = houses {
        *houses = world.houses;
    }
    if let Some(rng) = main_rng {
        *rng = world.main_rng;
    }
    *scenario_rng = world.scenario_rng;
    if let Some(grid) = overlay_grid {
        *grid = world.overlay_grid.expect("supplied overlay retained");
    }
    if let Some(grid) = terrain {
        *grid = world.resolved_terrain.expect("supplied terrain retained");
    }
    if let Some(events) = sound_sink {
        *events = world.sound_events;
    }
    result
}

pub(crate) struct DeferredCellPrelude<'a> {
    pub(crate) amount: Option<i32>,
    pub(crate) deferred: &'a mut Vec<TiberiumReductionRequest>,
}

impl combat_aoe::AoECellPrelude for DeferredCellPrelude<'_> {
    fn before_cell(
        &mut self,
        rx: u16,
        ry: u16,
        overlay: Option<&mut OverlayGrid>,
        registry: Option<&OverlayTypeRegistry>,
        _terrain: Option<&mut ResolvedTerrainGrid>,
        _rng: Option<&mut SimRng>,
        _occupancy: Option<&OccupancyGrid>,
    ) {
        if let Some(amount) = self.amount
            && combat_aoe::tiberium_reduction_cell_admitted(overlay.as_deref(), registry, rx, ry)
        {
            self.deferred
                .push(TiberiumReductionRequest { rx, ry, amount });
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn tick_combat(
    entities: &mut EntityStore,
    occupancy: &mut OccupancyGrid,
    rules: &RuleSet,
    interner: &mut StringInterner,
    current_tick: u64,
    tick_ms: u32,
    binary_frame: u32,
    scenario_rng: &mut SimRng,
) -> CombatTickResult {
    tick_combat_with_fog(
        entities,
        occupancy,
        rules,
        interner,
        None,
        &BTreeMap::new(),
        None,
        None,
        None,
        None,
        current_tick,
        tick_ms,
        binary_frame,
        // Convenience shim (tests only); empty live order falls back to the
        // stable-id resolution order, preserving prior behavior exactly.
        &[],
        None,
        scenario_rng,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn tick_combat_with_fog(
    entities: &mut EntityStore,
    occupancy: &mut OccupancyGrid,
    rules: &RuleSet,
    interner: &mut StringInterner,
    fog: Option<&FogState>,
    power_states: &BTreeMap<InternedId, PowerState>,
    sound_sink: Option<&mut Vec<SimSoundEvent>>,
    overlay_grid: Option<&mut OverlayGrid>,
    overlay_registry: Option<&OverlayTypeRegistry>,
    terrain: Option<&mut crate::map::resolved_terrain::ResolvedTerrainGrid>,
    current_tick: u64,
    tick_ms: u32,
    binary_frame: u32,
    live_order: &[u64],
    radiation: Option<&mut crate::sim::radiation::RadiationState>,
    scenario_rng: &mut SimRng,
) -> CombatTickResult {
    // Test-convenience entry: resolve rule handles the way sim init does.
    let handles = Some(crate::sim::type_handle_table::ResolvedRuleHandles::resolve(
        rules, interner,
    ));
    let mut unused_main_rng = SimRng::new(0);
    let mut empty_houses = BTreeMap::new();
    tick_combat_with_fog_and_main_rng(
        entities,
        occupancy,
        rules,
        interner,
        handles,
        fog,
        power_states,
        &mut empty_houses,
        &[],
        &HouseAllianceMap::new(),
        sound_sink,
        overlay_grid,
        overlay_registry,
        terrain,
        current_tick,
        tick_ms,
        binary_frame,
        live_order,
        &[],
        &[],
        radiation,
        &[],
        scenario_rng,
        &mut unused_main_rng,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn tick_combat_with_fog_and_main_rng(
    entities: &mut EntityStore,
    occupancy: &mut OccupancyGrid,
    rules: &RuleSet,
    interner: &mut StringInterner,
    handles: Option<crate::sim::type_handle_table::ResolvedRuleHandles>,
    fog: Option<&FogState>,
    power_states: &BTreeMap<InternedId, PowerState>,
    houses: &mut BTreeMap<InternedId, HouseState>,
    house_order: &[InternedId],
    alliances: &HouseAllianceMap,
    sound_sink: Option<&mut Vec<SimSoundEvent>>,
    overlay_grid: Option<&mut OverlayGrid>,
    overlay_registry: Option<&OverlayTypeRegistry>,
    terrain: Option<&mut crate::map::resolved_terrain::ResolvedTerrainGrid>,
    current_tick: u64,
    tick_ms: u32,
    binary_frame: u32,
    live_order: &[u64],
    projectile_detonations: &[ProjectileDetonation],
    wave_damage_events: &[WaveDamageEvent],
    radiation: Option<&mut crate::sim::radiation::RadiationState>,
    missile_detonations: &[crate::sim::spawn_manager::MissileDetonation],
    scenario_rng: &mut SimRng,
    main_rng: &mut SimRng,
    inline_hooks: Option<&mut FixtureTrace>,
) -> CombatTickResult {
    tick_combat_with_fog_and_main_rng_with_terrain_area(
        entities,
        occupancy,
        rules,
        interner,
        handles,
        fog,
        power_states,
        houses,
        house_order,
        alliances,
        sound_sink,
        overlay_grid,
        overlay_registry,
        terrain,
        None,
        false,
        false,
        current_tick,
        tick_ms,
        binary_frame,
        live_order,
        &BTreeSet::new(),
        &BTreeSet::new(),
        projectile_detonations,
        wave_damage_events,
        radiation,
        missile_detonations,
        scenario_rng,
        main_rng,
        inline_hooks,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn commit_damage_events(
    damage_events: &[EntityDamageEvent],
    entities: &mut EntityStore,
    occupancy: &mut OccupancyGrid,
    rules: &RuleSet,
    interner: &mut StringInterner,
    houses: &mut BTreeMap<InternedId, HouseState>,
    house_order: &[InternedId],
    alliances: &HouseAllianceMap,
    main_rng: &mut SimRng,
    scenario_rng: &mut SimRng,
    handled_deaths: &mut Vec<u64>,
    overlay_grid: Option<&mut OverlayGrid>,
    overlay_registry: Option<&OverlayTypeRegistry>,
    terrain: Option<&mut crate::map::resolved_terrain::ResolvedTerrainGrid>,
    current_tick: u64,
    inline_hooks: &mut Option<&mut FixtureTrace>,
    sound_sink: &mut Option<&mut Vec<SimSoundEvent>>,
) -> (DeathEffects, Vec<UnderAttackEvent>) {
    let handles = Some(crate::sim::type_handle_table::ResolvedRuleHandles::resolve(
        rules, interner,
    ));
    with_world(
        entities,
        occupancy,
        interner,
        Some(houses),
        Some(main_rng),
        scenario_rng,
        overlay_grid,
        terrain,
        None,
        sound_sink.as_deref_mut(),
        Some(handled_deaths),
        inline_hooks.as_deref_mut(),
        |world, run| {
            world.rule_handles = handles;
            world.session.house_order = house_order.to_vec();
            world.house_alliances = alliances.clone();
            world.receiver_fixture.as_mut().unwrap().current_tick = current_tick;
            world.session.binary_frame = current_tick as u32;
            world_receiver::commit_entities(
                world,
                run,
                damage_events,
                None,
                rules,
                overlay_registry,
            )
        },
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn commit_area_damage_receivers(
    receivers: &[combat_aoe::AreaDamageReceiver],
    entities: &mut EntityStore,
    occupancy: &mut OccupancyGrid,
    rules: &RuleSet,
    interner: &mut StringInterner,
    houses: &mut BTreeMap<InternedId, HouseState>,
    house_order: &[InternedId],
    alliances: &HouseAllianceMap,
    main_rng: &mut SimRng,
    scenario_rng: &mut SimRng,
    handled_deaths: &mut Vec<u64>,
    overlay_grid: Option<&mut OverlayGrid>,
    overlay_registry: Option<&OverlayTypeRegistry>,
    terrain: Option<&mut crate::map::resolved_terrain::ResolvedTerrainGrid>,
    terrain_area_state: Option<&mut TerrainAreaState>,
    current_tick: u64,
    inline_hooks: &mut Option<&mut FixtureTrace>,
    sound_sink: &mut Option<&mut Vec<SimSoundEvent>>,
) -> (DeathEffects, Vec<UnderAttackEvent>) {
    let handles = Some(crate::sim::type_handle_table::ResolvedRuleHandles::resolve(
        rules, interner,
    ));
    with_world(
        entities,
        occupancy,
        interner,
        Some(houses),
        Some(main_rng),
        scenario_rng,
        overlay_grid,
        terrain,
        terrain_area_state,
        sound_sink.as_deref_mut(),
        Some(handled_deaths),
        inline_hooks.as_deref_mut(),
        |world, run| {
            world.rule_handles = handles;
            world.session.house_order = house_order.to_vec();
            world.house_alliances = alliances.clone();
            world.receiver_fixture.as_mut().unwrap().current_tick = current_tick;
            world.session.binary_frame = current_tick as u32;
            world_receiver::commit_area(world, run, receivers, rules, overlay_registry)
        },
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_entity_deaths(
    entities: &mut EntityStore,
    occupancy: &mut OccupancyGrid,
    rules: &RuleSet,
    interner: &mut StringInterner,
    handles: Option<crate::sim::type_handle_table::ResolvedRuleHandles>,
    houses: &mut BTreeMap<InternedId, HouseState>,
    house_order: &[InternedId],
    alliances: &HouseAllianceMap,
    main_rng: &mut SimRng,
    scenario_rng: &mut SimRng,
    handled_deaths: &mut Vec<u64>,
    dead_entities: &[u64],
    damage_events: &[EntityDamageEvent],
    overlay_grid: Option<&mut OverlayGrid>,
    overlay_registry: Option<&OverlayTypeRegistry>,
    terrain: Option<&mut crate::map::resolved_terrain::ResolvedTerrainGrid>,
    terrain_area_state: &mut Option<&mut TerrainAreaState>,
    scenario_no_damage: bool,
    current_tick: u64,
    inline_hooks: &mut Option<&mut FixtureTrace>,
    sound_sink: &mut Option<&mut Vec<SimSoundEvent>>,
) -> DeathEffects {
    with_world(
        entities,
        occupancy,
        interner,
        Some(houses),
        Some(main_rng),
        scenario_rng,
        overlay_grid,
        terrain,
        terrain_area_state.as_deref_mut(),
        sound_sink.as_deref_mut(),
        Some(handled_deaths),
        inline_hooks.as_deref_mut(),
        |world, run| {
            world.rule_handles = handles;
            world.session.house_order = house_order.to_vec();
            world.house_alliances = alliances.clone();
            world.receiver_fixture.as_mut().unwrap().current_tick = current_tick;
            world.session.no_damage = scenario_no_damage;
            world.session.binary_frame = current_tick as u32;
            world_receiver::handle_death(
                world,
                run,
                dead_entities,
                damage_events,
                rules,
                overlay_registry,
            )
        },
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_projectile_detonations(
    detonations: &[ProjectileDetonation],
    entities: &mut EntityStore,
    occupancy: &OccupancyGrid,
    rules: &RuleSet,
    interner: &mut StringInterner,
    handles: Option<crate::sim::type_handle_table::ResolvedRuleHandles>,
    overlay_grid: Option<&mut OverlayGrid>,
    overlay_registry: Option<&OverlayTypeRegistry>,
    terrain: Option<&mut crate::map::resolved_terrain::ResolvedTerrainGrid>,
    bridge_state: Option<&BridgeRuntimeState>,
    terrain_objects: Option<combat_aoe::TerrainCollectionView<'_>>,
    terrain_area_state: Option<&TerrainAreaState>,
    scenario_no_damage: bool,
    house_alliances: &HouseAllianceMap,
    scenario_rng: &mut SimRng,
    inline_hooks: &mut Option<&mut FixtureTrace>,
    out: &mut CombatEmit,
) {
    let mut fixture_occupancy = occupancy.clone();
    let mut fixture_terrain = terrain_area_state.cloned();
    with_world(
        entities,
        &mut fixture_occupancy,
        interner,
        None,
        None,
        scenario_rng,
        overlay_grid,
        terrain,
        fixture_terrain.as_mut(),
        None,
        None,
        inline_hooks.as_deref_mut(),
        |world, _run| {
            world.rule_handles = handles;
            world.session.no_damage = scenario_no_damage;
            world.receiver_fixture.as_mut().unwrap().terrain_collection = terrain_objects.is_some();
            if let Some(view) = terrain_objects {
                world.production.terrain_objects = view.objects.clone();
                world.production.terrain_object_cells = view.cells.clone();
            }
            world.house_alliances = house_alliances.clone();
            world.bridge_state = bridge_state.cloned();
            world_receiver::emit_projectile_detonations(
                world,
                rules,
                overlay_registry,
                detonations,
                out,
            )
        },
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn resolve_attacker_fire(
    snap: &AttackerSnapshot,
    entities: &mut EntityStore,
    rules: &RuleSet,
    interner: &mut StringInterner,
    handles: Option<crate::sim::type_handle_table::ResolvedRuleHandles>,
    fog: Option<&FogState>,
    occupancy: &OccupancyGrid,
    overlay_grid: Option<&mut OverlayGrid>,
    overlay_registry: Option<&OverlayTypeRegistry>,
    terrain: Option<&mut crate::map::resolved_terrain::ResolvedTerrainGrid>,
    terrain_objects: Option<combat_aoe::TerrainCollectionView<'_>>,
    terrain_area_state: Option<&TerrainAreaState>,
    scenario_no_damage: bool,
    require_playfield_membership: bool,
    binary_frame: u32,
    _tick_ms: u32,
    has_active_wave: bool,
    scenario_rng: &mut SimRng,
    sound_sink: Option<&mut Vec<SimSoundEvent>>,
    inline_hooks: &mut Option<&mut FixtureTrace>,
    out: &mut CombatEmit,
) {
    let mut fixture_occupancy = occupancy.clone();
    let mut fixture_terrain = terrain_area_state.cloned();
    with_world(
        entities,
        &mut fixture_occupancy,
        interner,
        None,
        None,
        scenario_rng,
        overlay_grid,
        terrain,
        fixture_terrain.as_mut(),
        sound_sink,
        None,
        inline_hooks.as_deref_mut(),
        |world, _run| {
            world.rule_handles = handles;
            world.session.no_damage = scenario_no_damage;
            world.receiver_fixture.as_mut().unwrap().terrain_collection = terrain_objects.is_some();
            if let Some(view) = terrain_objects {
                world.production.terrain_objects = view.objects.clone();
                world.production.terrain_object_cells = view.cells.clone();
            }
            world.session.binary_frame = binary_frame;
            world.receiver_fixture.as_mut().unwrap().current_tick = u64::from(binary_frame);
            world_receiver::resolve_attacker_fire(
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
            )
        },
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn tick_combat_with_fog_and_main_rng_with_terrain_area(
    entities: &mut EntityStore,
    occupancy: &mut OccupancyGrid,
    rules: &RuleSet,
    interner: &mut StringInterner,
    handles: Option<crate::sim::type_handle_table::ResolvedRuleHandles>,
    fog: Option<&FogState>,
    power_states: &BTreeMap<InternedId, PowerState>,
    houses: &mut BTreeMap<InternedId, HouseState>,
    house_order: &[InternedId],
    alliances: &HouseAllianceMap,
    sound_sink: Option<&mut Vec<SimSoundEvent>>,
    overlay_grid: Option<&mut OverlayGrid>,
    overlay_registry: Option<&OverlayTypeRegistry>,
    terrain: Option<&mut crate::map::resolved_terrain::ResolvedTerrainGrid>,
    bridge_state: Option<&BridgeRuntimeState>,
    scenario_no_damage: bool,
    require_playfield_membership: bool,
    current_tick: u64,
    tick_ms: u32,
    binary_frame: u32,
    live_order: &[u64],
    fire_suppressed: &BTreeSet<u64>,
    active_wave_owners: &BTreeSet<u64>,
    projectile_detonations: &[ProjectileDetonation],
    wave_damage_events: &[WaveDamageEvent],
    radiation: Option<&mut crate::sim::radiation::RadiationState>,
    missile_detonations: &[crate::sim::spawn_manager::MissileDetonation],
    scenario_rng: &mut SimRng,
    main_rng: &mut SimRng,
    inline_hooks: Option<&mut FixtureTrace>,
    terrain_area_state: Option<&mut TerrainAreaState>,
) -> CombatTickResult {
    with_world(
        entities,
        occupancy,
        interner,
        Some(houses),
        Some(main_rng),
        scenario_rng,
        overlay_grid,
        terrain,
        terrain_area_state,
        sound_sink,
        None,
        inline_hooks,
        |world, run| {
            world.rule_handles = handles;
            world.session.house_order = house_order.to_vec();
            world.house_alliances = alliances.clone();
            world.receiver_fixture.as_mut().unwrap().current_tick = current_tick;
            world.session.no_damage = scenario_no_damage;
            world.session.binary_frame = binary_frame;
            world.power_states = power_states.clone();
            world.bridge_state = bridge_state.cloned();
            if let Some(fog) = fog {
                world.fog = fog.clone();
            }
            world.receiver_fixture.as_mut().unwrap().fog_enabled = fog.is_some();
            world.receiver_fixture.as_mut().unwrap().radiation_enabled = radiation.is_some();
            world
                .receiver_fixture
                .as_mut()
                .unwrap()
                .require_playfield_membership = require_playfield_membership;
            world.active_wave_links = active_wave_owners.iter().map(|&owner| (owner, 0)).collect();
            world.pending_missile_detonations = missile_detonations.to_vec();
            if let Some(radiation) = radiation.as_deref() {
                world.radiation = radiation.clone();
            }
            let result = world_receiver::tick_combat(
                world,
                run,
                rules,
                overlay_registry,
                tick_ms,
                live_order,
                fire_suppressed,
                projectile_detonations,
                wave_damage_events,
            );
            if let Some(radiation) = radiation {
                *radiation = std::mem::take(&mut world.radiation);
            }
            result
        },
    )
}
