//! Construction and frame API around Simulation and its bound match resources.
//!
//! App, headless and replay execution use SimRuntime. SimResources binds the
//! rules/art registry, map heights, overlay registry, trigger definitions and
//! base terrain template; advance_frame reads these without caller-substituted
//! per-frame resources. Simulation owns live mutable terrain and gameplay state.
//! SimView provides immutable access for presentation and diagnostics.
//!
//! Dependency rules: part of sim/; never depends on render/, ui/, sidebar/,
//! audio/ or net/.

use crate::map::resolved_terrain::TerrainTileAnimation;
use crate::rules::ruleset::RuleSet;
use crate::sim::anim_class::{AnimDrawRuntime, AnimWorldCoord};
use crate::sim::components::AnimClassSpawnDescriptor;
use crate::sim::world::Simulation;

/// Per-match resource inputs bound at construction and read during frame advancement.

pub struct SimResources {
    /// Fixed per-cell terrain heights parsed from the loaded map.
    pub height_map: std::collections::BTreeMap<(u16, u16), u8>,
    /// Bridge-deck heights layered above the terrain heights.
    pub bridge_height_map: std::collections::BTreeMap<(u16, u16), u8>,
    /// Rules-semantic overlay registry for the loaded match.
    pub overlay_registry: crate::rules::overlay_types::OverlayTypeRegistry,
    /// The immutable base resolved-terrain template: source-derived, used for
    /// static rendering and snapshot restore. Never the live runtime grid -
    /// the simulation owns and rebuilds its own live resolved terrain (F08
    /// naming, bound as an F07 cone).
    pub terrain_template: Option<crate::map::resolved_terrain::ResolvedTerrainGrid>,
    /// The complete immutable match rules (including the sole ArtRegistry).
    pub rules: crate::rules::ruleset::RuleSet,
    /// Immutable trigger definitions parsed from the map; the runtime state
    /// machine lives in the simulation, these are bound once (F07: the app
    /// no longer passes definitions each frame).
    pub trigger_graph: crate::map::trigger_graph::TriggerGraph,
    pub triggers: crate::map::triggers::TriggerMap,
    pub events: crate::map::events::EventMap,
    pub actions: crate::map::actions::ActionMap,
    /// Complete immutable scenario waypoint table used by trigger actions.
    pub waypoints: std::collections::HashMap<u32, crate::map::waypoints::Waypoint>,
}

impl SimResources {
    /// Empty pre-bind resources for fixture and fallback construction.
    pub fn empty() -> Self {
        Self {
            height_map: std::collections::BTreeMap::new(),
            bridge_height_map: std::collections::BTreeMap::new(),
            overlay_registry: crate::rules::overlay_types::OverlayTypeRegistry::empty(),
            rules: crate::rules::ruleset::RuleSet::from_ini(
                &crate::rules::ini_parser::IniFile::from_str(""),
            )
            .expect("empty rules parse"),
            terrain_template: None,
            trigger_graph: Default::default(),
            triggers: Default::default(),
            events: Default::default(),
            actions: Default::default(),
            waypoints: Default::default(),
        }
    }
}

/// The runtime owner: one deterministic simulation plus its bound resources.
pub struct SimRuntime {
    pub simulation: Simulation,
    pub resources: SimResources,
}

impl SimRuntime {
    /// Test adapter for an existing simulation with empty resource inputs.
    #[cfg(test)]
    pub(crate) fn from_simulation(simulation: Simulation) -> Self {
        Self {
            simulation,
            resources: SimResources::empty(),
        }
    }

    /// Immutable read facade for presentation and diagnostics.
    pub fn view(&self) -> SimView<'_> {
        SimView {
            simulation: &self.simulation,
        }
    }
}

/// Immutable borrow facade over simulation state for presentation and diagnostics.
pub struct SimView<'a> {
    simulation: &'a Simulation,
}

impl<'a> SimView<'a> {
    /// Full immutable state access for consumers without a narrower view method.
    pub fn simulation(&self) -> &'a Simulation {
        self.simulation
    }

    pub fn interner(&self) -> &'a crate::sim::intern::StringInterner {
        &self.simulation.interner
    }

    pub fn entities(&self) -> &'a crate::sim::entity_store::EntityStore {
        self.simulation.entities()
    }

    pub fn session(&self) -> &'a crate::sim::scenario_session::ScenarioSession {
        &self.simulation.session
    }

    pub fn fog(&self) -> &'a crate::sim::vision::FogState {
        &self.simulation.fog
    }

    pub fn houses(
        &self,
    ) -> &'a std::collections::BTreeMap<
        crate::sim::intern::InternedId,
        crate::sim::house_state::HouseState,
    > {
        &self.simulation.houses
    }

    pub fn path_grid(&self) -> Option<&'a crate::sim::pathfinding::PathGrid> {
        self.simulation.path_grid()
    }

    pub(crate) fn resolved_terrain(
        &self,
    ) -> Option<&'a crate::map::resolved_terrain::ResolvedTerrainGrid> {
        self.simulation.resolved_terrain.as_ref()
    }

    pub fn bridge_state(&self) -> Option<&'a crate::sim::bridge_state::BridgeRuntimeState> {
        self.simulation.bridge_state.as_ref()
    }

    pub(crate) fn overlay_grid(&self) -> Option<&'a crate::sim::overlay_grid::OverlayGrid> {
        self.simulation.overlay_grid.as_ref()
    }

    /// LogicClass active-object order — presentation draws in this order.
    pub(crate) fn tactical_registration_order(&self) -> &'a [u64] {
        self.simulation.tactical_registration_order()
    }

    /// Pending radar-terrain batch for the minimap dirty gate. Presentation
    /// acknowledges this exact generation only after a completed update.
    pub(crate) fn radar_terrain_dirty(&self) -> (&'a [(u16, u16)], u64) {
        (
            &self.simulation.radar_terrain_dirty_cells,
            self.simulation.radar_terrain_dirty_generation,
        )
    }

    /// Current normalized MapClass LocalSize authority plus the generation of
    /// successful trigger-action-40 writers that rebuilt radar/scroll state.
    pub(crate) fn playfield_authority(
        &self,
    ) -> (Option<crate::map::playfield::PlayfieldBounds>, u64) {
        (
            self.simulation.playfield_bounds,
            self.simulation.playfield_revision,
        )
    }

    /// Full MapClass Size dimensions retained beside normalized LocalSize.
    /// Radar input `0x00653D92..0x00653DE3` reads both values before resolving
    /// its signed click cell through `MapClass::Get_CellClass`.
    pub(crate) fn playfield_map_size(&self) -> Option<(i32, i32)> {
        Some((
            self.simulation.playfield_bounds?.base,
            self.simulation.playfield_size_height?,
        ))
    }
}

impl SimRuntime {
    /// Clear the exact radar-terrain batch a completed presentation update read.
    pub(crate) fn acknowledge_radar_terrain_dirty(&mut self, generation: u64) -> bool {
        self.simulation.acknowledge_radar_terrain_dirty(generation)
    }

    /// One command-free Ordinary-lane frame for side binaries (parity-digest):
    /// the same bound-resource transaction as `advance_frame`, with the
    /// crate-private frame output discarded so no internal type goes public.
    pub fn advance_idle_frame_for_tooling(
        &mut self,
        tick_ms: u32,
    ) -> Result<(), crate::sim::world::FrameAdvanceError> {
        self.advance_frame(&[], tick_ms, crate::sim::world::TickLane::Ordinary)
            .map(|_| ())
    }

    /// The production frame transaction: advance one lane-tagged frame using
    /// the bound immutable resources. Callers cannot substitute rules, maps,
    /// registries, definitions, or navigation (the simulation pins its own
    /// canonical path snapshot internally).
    pub(crate) fn advance_frame(
        &mut self,
        commands: &[crate::sim::command::CommandEnvelope],
        tick_ms: u32,
        lane: crate::sim::world::TickLane,
    ) -> Result<crate::sim::world::SimFrameOutput, crate::sim::world::FrameAdvanceError> {
        self.simulation.advance_app_frame(
            commands,
            Some(&self.resources.rules),
            &self.resources.height_map,
            Some(&self.resources.overlay_registry),
            tick_ms,
            lane,
            Some(crate::sim::world::TriggerInputs {
                graph: &self.resources.trigger_graph,
                triggers: &self.resources.triggers,
                events: &self.resources.events,
                actions: &self.resources.actions,
                waypoints: &self.resources.waypoints,
                rules: Some(&self.resources.rules),
            }),
        )
    }
}

impl SimRuntime {
    /// Replace a same-content restored simulation in place. Resource ownership
    /// never changes and callers must already have a running match.
    pub(crate) fn replace_simulation(&mut self, simulation: Simulation) {
        self.simulation = simulation;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staged_campaign_selects_current_house_after_roster_construction() {
        let map = crate::map::map_file::MapFile::from_bytes(
            b"[Map]\nTheater=TEMPERATE\nSize=0,0,40,40\nLocalSize=2,2,36,32\n[Basic]\nPlayer=Alpha\n[Houses]\n7=Zulu\n2=Alpha\n[Zulu]\n[Alpha]\n[IsoMapPack5]\n1=CAAEABUAAAAAEQAA\n",
        ).expect("minimal authored map");
        let roster = crate::map::houses::parse_house_roster(&map.ini, &[], None);
        let descriptor = crate::sim::scenario_session::ScenarioDescriptor::default();
        let mut sim = Simulation::new();
        sim.interner.intern("alpha");
        let terrain =
            crate::map::resolved_terrain::ResolvedTerrainGrid::from_cells(0, 0, Vec::new());
        populate_staged_scenario_with_generated_inits(
            &mut sim,
            &map,
            &terrain,
            "TEMPERATE",
            None,
            &std::collections::BTreeMap::new(),
            None,
            None,
            crate::map::basic::BridgeDestroyabilityMode::CampaignOrEditor,
            &descriptor,
            None,
            |sim| crate::sim::scenario_bootstrap::initialize_map_roster_houses(sim, &roster, None),
        )
        .expect("staged map-roster construction");
        let selected = sim.interner.get("Alpha").unwrap();
        assert_eq!(sim.session.current_house, Some(selected));
        assert!(sim.houses[&selected].is_human && sim.houses[&selected].player_control);
        let first = sim.interner.get("Zulu").unwrap();
        assert_eq!(sim.session.house_order, [first, selected]);
        assert!(!sim.houses[&first].is_human && !sim.houses[&first].player_control);
    }

    /// F07 matrix: the runtime always uses bound navigation and resources —
    /// both production install paths keep the match resources. The map-load
    /// install binds them from the load result; the in-scenario restore path
    /// (quickload / save-load panel) must CARRY the surviving resources, not
    /// rebind empty ones.
    #[test]
    fn runtime_always_uses_bound_navigation_and_resources() {
        let mut resources = SimResources::empty();
        resources.height_map.insert((3, 4), 7);
        resources.waypoints.insert(
            0,
            crate::map::waypoints::Waypoint {
                index: 0,
                rx: 93,
                ry: 106,
            },
        );
        resources.waypoints.insert(
            701,
            crate::map::waypoints::Waypoint {
                index: 701,
                rx: 122,
                ry: 135,
            },
        );
        let expected_waypoints = resources.waypoints.clone();
        let mut original = SimRuntime {
            simulation: Simulation::new(),
            resources,
        };

        original.replace_simulation(Simulation::new());
        let rebound = original;
        assert_eq!(
            rebound.resources.height_map.get(&(3, 4)),
            Some(&7),
            "restore must carry the match resources, never rebind empty"
        );
        assert_eq!(rebound.resources.waypoints, expected_waypoints);
    }

    /// The crate regeneration rung is guarded in the master frame by
    /// `if let Some(overlay_registry)`, a Rust availability gate with no native
    /// counterpart. Prove the SOLE production frame entry point always binds
    /// one, so that gate can never silently disable
    /// `MapClass__UpdateCrateRegenTimers @ 0x0056BBE0` in a real match.
    #[test]
    fn crate_regen_rung_runs_in_every_production_frame() {
        use crate::sim::crates::CrateSlot;
        use crate::sim::crates::tests::{crate_registry, crate_ruleset, sim_with_grid};

        let mut simulation = sim_with_grid(0x62);
        simulation.session.game_mode_nonzero = true;
        simulation.session.game_options.crates = true;
        let due = CrateSlot {
            start_frame: -1,
            aux: 0,
            duration: 0,
            cell_x: 15,
            cell_y: 15,
        };
        *simulation.crate_authority.slot_mut(0) = due;

        let mut resources = SimResources::empty();
        resources.rules = crate_ruleset("");
        resources.overlay_registry = crate_registry();
        let mut runtime = SimRuntime {
            simulation,
            resources,
        };

        let _ = runtime
            .advance_frame(&[], 33, crate::sim::world::TickLane::Ordinary)
            .expect("fixture frame must complete");

        assert_ne!(
            runtime.simulation.crate_authority.slots()[0],
            due,
            "SimRuntime::advance_frame must reach the crate regeneration rung"
        );
    }

    #[test]
    fn gsi_04_12_generated_funnel_projects_preconsumed_words_without_scenario_draw() {
        use crate::map::entities::{EntityCategory, MapEntity};
        use crate::rules::ini_parser::IniFile;
        use crate::sim::game_entity::GeneratedTechnoInit;
        use crate::sim::world::GeneratedTechnoInitTable;

        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=MTNK\n\n[MTNK]\nStrength=300\nSpeed=6\n",
        ))
        .expect("generated projection rules");
        let entity = MapEntity {
            owner: "Americans".to_string(),
            type_id: "MTNK".to_string(),
            health: 256,
            cell_x: 7,
            cell_y: 9,
            facing: 0,
            category: EntityCategory::Unit,
            sub_cell: 0,
            veterancy: 0,
            high: false,
            mission: None,
            recruitable_a: true,
            recruitable_b: true,
            structure_upgrades: [None, None, None],
        };
        let inits = GeneratedTechnoInitTable::try_new([GeneratedTechnoInit {
            entity_index: 0,
            techno_type: "MTNK".to_string(),
            cell: (7, 9),
            techno_ctor_random_word: 0xA55A,
        }])
        .expect("one exact generated binding");
        let mut sim = Simulation::with_seed(0xC701_0412);
        let owner = sim.interner.intern("Americans");
        sim.houses.insert(
            owner,
            crate::sim::house_state::HouseState::new(owner, 0, None, true, 0, 10),
        );
        let before = sim.scenario_rng.logical_state();

        assert_eq!(
            project_map_entities(
                &mut sim,
                &[entity],
                Some(&rules),
                &std::collections::BTreeMap::new(),
                None,
                None,
                Some(&inits),
            )
            .expect("generated funnel projection"),
            1
        );
        assert_eq!(sim.scenario_rng.logical_state(), before);
        assert_eq!(
            sim.entities()
                .values()
                .next()
                .expect("projected MTNK")
                .techno_ctor_random_word,
            0xA55A
        );
    }
}

fn project_map_entities(
    sim: &mut Simulation,
    entities: &[crate::map::entities::MapEntity],
    rules: Option<&crate::rules::ruleset::RuleSet>,
    height_map: &std::collections::BTreeMap<(u16, u16), u8>,
    resolved_terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    generated_inits: Option<&crate::sim::world::GeneratedTechnoInitTable>,
) -> Result<u32, crate::sim::world::GeneratedTechnoInitError> {
    if let Some(generated_inits) = generated_inits {
        sim.spawn_generated_from_map_with_resolved(
            entities,
            rules.expect("generated-map projection requires loaded rules"),
            height_map,
            resolved_terrain,
            generated_inits,
        )
    } else {
        Ok(sim.spawn_from_map_with_resolved_and_overlay_registry(
            entities,
            rules,
            height_map,
            resolved_terrain,
            overlay_registry,
        ))
    }
}

/// Construct the post-map-section terrain-attached AnimClass set.
///
/// gamemd-derived: the final `MapClass::InitCellAttributes @ 0x00568BB0`
/// anti-diagonal pass calls `CellClass::RecalcAttributes @ 0x0047D2B0`; the
/// constructor row at 0x0047DA3B..0x0047DA5F supplies delay 0, signed loop -1,
/// flags 0x1600 and constructor ZAdjust 0. The delay-zero constructor calls
/// `Middle`, then the producer writes the tile ZAdjust; the descriptor carries
/// the producer's explicit +0x196/+0x197 state into the Rust object.
pub(crate) fn spawn_terrain_tile_animations(
    sim: &mut Simulation,
    rules: &RuleSet,
    tile_animations: &[TerrainTileAnimation],
) -> Vec<u64> {
    const TILE_ANIM_DRAW_FLAGS: u32 = 0x1600;
    let mut spawned = Vec::with_capacity(tile_animations.len());
    for tile in tile_animations {
        let type_name = sim.interner.intern(&tile.anim_name);
        let descriptor = AnimClassSpawnDescriptor {
            type_name,
            rx: tile.rx,
            ry: tile.ry,
            sub_x: crate::util::fixed_math::SimFixed::from_num(
                tile.world_x
                    .wrapping_sub(i32::from(tile.rx).wrapping_mul(256)),
            ),
            sub_y: crate::util::fixed_math::SimFixed::from_num(
                tile.world_y
                    .wrapping_sub(i32::from(tile.ry).wrapping_mul(256)),
            ),
            z: u8::try_from(
                tile.world_z
                    .div_euclid(crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS),
            )
            .unwrap_or(0),
            delay: 0,
            loop_count: -1,
            draw_flags: TILE_ANIM_DRAW_FLAGS,
            z_adjust: 0,
            reverse: false,
            use_cell_drawer: true,
            terrain_attached: true,
            draw_runtime: AnimDrawRuntime::default(),
        };
        let id = sim
            .spawn_anim_at_world(
                rules,
                descriptor,
                AnimWorldCoord {
                    x: tile.world_x,
                    y: tile.world_y,
                    z: tile.world_z,
                },
            )
            .unwrap_or_else(|error| {
                panic!(
                    "resolved terrain animation [{}] must be bound before map spawn: {error}",
                    tile.anim_name
                )
            });
        assert!(
            sim.set_terrain_anim_z_adjust_after_construction(id, tile.z_adjust),
            "terrain animation {id} disappeared before its producer ZAdjust write"
        );
        spawned.push(id);
    }
    spawned
}

/// GPU-free scenario construction shared by app and headless (F09): native
/// order preserved — bootstrap RNG into the session, houses before every
/// object section (ScenarioClass__Create_Houses @ 0x00687F10 precedes
/// TerrainClass__Read_Map_Section @ 0x0071CA70), terrain objects before map
/// entities, then terrain-attached animations. Presentation atlases consume
/// the constructed simulation afterward; nothing here touches the GPU.
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn construct_scenario<F>(
    map_data: &crate::map::map_file::MapFile,
    resolved_terrain: &crate::map::resolved_terrain::ResolvedTerrainGrid,
    theater_name: &str,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    height_map: &std::collections::BTreeMap<(u16, u16), u8>,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    overlay_grid: Option<&crate::sim::overlay_grid::OverlayGrid>,
    bridge_destroyability_mode: crate::map::basic::BridgeDestroyabilityMode,
    descriptor: &crate::sim::scenario_session::ScenarioDescriptor,
    bootstrap_rng: crate::sim::scenario_bootstrap::ScenarioBootstrapRng,
    initialize_houses_before_objects: F,
) -> Simulation
where
    F: FnOnce(&mut Simulation),
{
    construct_scenario_with_generated_inits(
        map_data,
        resolved_terrain,
        theater_name,
        rules,
        height_map,
        overlay_registry,
        overlay_grid,
        bridge_destroyability_mode,
        descriptor,
        bootstrap_rng,
        None,
        initialize_houses_before_objects,
    )
    .expect("fixed-map Techno constructor projection cannot fail")
}

/// Generated-map variant of the shared construction funnel. The binding table
/// proves that launch-time RMG already consumed every Techno constructor word;
/// projection validates the complete table before creating its first entity.
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn construct_scenario_with_generated_inits<F>(
    map_data: &crate::map::map_file::MapFile,
    resolved_terrain: &crate::map::resolved_terrain::ResolvedTerrainGrid,
    theater_name: &str,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    height_map: &std::collections::BTreeMap<(u16, u16), u8>,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    overlay_grid: Option<&crate::sim::overlay_grid::OverlayGrid>,
    bridge_destroyability_mode: crate::map::basic::BridgeDestroyabilityMode,
    descriptor: &crate::sim::scenario_session::ScenarioDescriptor,
    bootstrap_rng: crate::sim::scenario_bootstrap::ScenarioBootstrapRng,
    generated_inits: Option<&crate::sim::world::GeneratedTechnoInitTable>,
    initialize_houses_before_objects: F,
) -> Result<Simulation, crate::sim::world::GeneratedTechnoInitError>
where
    F: FnOnce(&mut Simulation),
{
    let mut sim: Simulation = bootstrap_rng.into_simulation(descriptor);
    populate_staged_scenario_with_generated_inits(
        &mut sim,
        map_data,
        resolved_terrain,
        theater_name,
        rules,
        height_map,
        overlay_registry,
        overlay_grid,
        bridge_destroyability_mode,
        descriptor,
        generated_inits,
        initialize_houses_before_objects,
    )?;
    Ok(sim)
}

/// Populate the one Simulation that already owns the post-prefix load cursors.
///
/// Fresh-load orchestration stages this owner before terrain Fill.  Keeping the
/// object-section funnel separate from construction prevents a later shadow
/// Simulation from replacing the registries and identities that OverlayPack
/// finalization has already touched.
#[derive(Debug, thiserror::Error)]
enum ScenarioPopulationError {
    #[error(transparent)]
    GeneratedTechno(#[from] crate::sim::world::GeneratedTechnoInitError),
    #[error(transparent)]
    NativeIdentity(#[from] crate::sim::native_identity::NativeMapTubeConstructionError),
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn populate_staged_scenario_with_generated_inits<F>(
    sim: &mut Simulation,
    map_data: &crate::map::map_file::MapFile,
    resolved_terrain: &crate::map::resolved_terrain::ResolvedTerrainGrid,
    theater_name: &str,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    height_map: &std::collections::BTreeMap<(u16, u16), u8>,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    overlay_grid: Option<&crate::sim::overlay_grid::OverlayGrid>,
    bridge_destroyability_mode: crate::map::basic::BridgeDestroyabilityMode,
    descriptor: &crate::sim::scenario_session::ScenarioDescriptor,
    generated_inits: Option<&crate::sim::world::GeneratedTechnoInitTable>,
    initialize_houses_before_objects: F,
) -> Result<(), crate::sim::world::GeneratedTechnoInitError>
where
    F: FnOnce(&mut Simulation),
{
    match populate_staged_scenario_inner(
        sim,
        map_data,
        resolved_terrain,
        theater_name,
        rules,
        height_map,
        overlay_registry,
        overlay_grid,
        None,
        bridge_destroyability_mode,
        descriptor,
        generated_inits,
        initialize_houses_before_objects,
    ) {
        Ok(()) => Ok(()),
        Err(ScenarioPopulationError::GeneratedTechno(error)) => Err(error),
        Err(ScenarioPopulationError::NativeIdentity(_)) => {
            unreachable!("generated/compatibility population cannot consume native map IDs")
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn populate_staged_scenario_inner<F>(
    sim: &mut Simulation,
    map_data: &crate::map::map_file::MapFile,
    resolved_terrain: &crate::map::resolved_terrain::ResolvedTerrainGrid,
    theater_name: &str,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    height_map: &std::collections::BTreeMap<(u16, u16), u8>,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    overlay_grid: Option<&crate::sim::overlay_grid::OverlayGrid>,
    authored_overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    bridge_destroyability_mode: crate::map::basic::BridgeDestroyabilityMode,
    descriptor: &crate::sim::scenario_session::ScenarioDescriptor,
    generated_inits: Option<&crate::sim::world::GeneratedTechnoInitTable>,
    initialize_houses_before_objects: F,
) -> Result<(), ScenarioPopulationError>
where
    F: FnOnce(&mut Simulation),
{
    // Active YR `ScenarioClass__Full_Init @ 0x00686B20` calls
    // `ScenarioClass__Create_Houses @ 0x00687F10` before
    // `TerrainClass__Read_Map_Section @ 0x0071CA70` and every Techno section.
    // Keep the app-specific roster construction outside sim while making that
    // order an explicit prerequisite of the shared object-construction funnel.
    initialize_houses_before_objects(sim);
    if !descriptor.game_mode_nonzero {
        let roster = crate::map::houses::parse_house_roster(
            &map_data.ini,
            rules.map_or(&[], |rules| rules.color_schemes.as_slice()),
            rules,
        );
        crate::sim::scenario_bootstrap::initialize_campaign_current_house(
            sim,
            &roster,
            &map_data.ini,
        );
    }
    // Frame tripwire: every MP start waypoint must sit inside the session
    // bounds (= the fog window, cell-array frame). A start outside means the
    // descriptor was fed wrong-frame bounds (e.g. raw [Map] Size=) and the
    // player's own base would be permanently shrouded.
    for (idx, (rx, ry)) in &descriptor.mp_start_waypoints {
        if *rx >= descriptor.map_width || *ry >= descriptor.map_height {
            log::error!(
                "MP start waypoint {idx} at ({rx},{ry}) lies outside session bounds {}x{} — wrong coordinate frame?",
                descriptor.map_width,
                descriptor.map_height
            );
            debug_assert!(
                false,
                "start waypoint outside session bounds (coordinate-frame mismatch)"
            );
        }
    }
    sim.install_resolved_terrain_for_new_map(resolved_terrain.clone());
    // Active `CellClass` overlay identity exists before the Techno map
    // sections are read. Install the already-resolved grid now so every
    // UnitClass virtual Unlimbo sees ore, walls, and structural bridges;
    // finalization later replaces this clone after wall-owner reconstruction.
    sim.overlay_grid = overlay_grid.cloned();
    // Wire the cliff/slope coefficients from [General] into the live World config;
    // it otherwise holds compiled vanilla defaults and never sees a modded INI.
    if let Some(rules) = rules {
        sim.terrain_speed_config =
            crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::from_general(
                rules.general.tracked_uphill,
                rules.general.tracked_downhill,
                rules.general.wheeled_uphill,
                rules.general.wheeled_downhill,
            );
    }
    // Normal loading first clips/normalizes LocalSize through
    // `MapClass::Set_Clipped_LocalSize @ 0x00567230`; every playfield consumer
    // then shares those stored fields and the same isometric-diamond test.
    sim.install_playfield_from_map_header(&map_data.header);
    let bridge_destroyable = map_data
        .special_flags
        .effective_destroyable_bridges(bridge_destroyability_mode);
    let bridge_strength = rules
        .map(|rules| rules.bridge_rules.strength)
        .unwrap_or(1500);
    sim.bridge_state = Some(
        crate::sim::bridge_state::BridgeRuntimeState::from_resolved_terrain_with_map_size(
            resolved_terrain,
            bridge_destroyable,
            bridge_strength,
            (map_data.header.width as i32, map_data.header.height as i32),
        ),
    );
    sim.bridge_explosions = rules
        .map(|r| {
            r.bridge_rules
                .explosions
                .iter()
                .map(|s| sim.interner.intern(s))
                .collect()
        })
        .unwrap_or_default();
    sim.metallic_debris = rules
        .map(|r| {
            r.general
                .metallic_debris
                .iter()
                .map(|s| sim.interner.intern(s))
                .collect()
        })
        .unwrap_or_default();
    // gamemd `TerrainClass::Read_Map_Section` runs while the map sections are
    // walked, ahead of `[Units]`/`[Aircraft]`/`[Infantry]`/`[Structures]`: every
    // tree owns its cell before the first map object is placed on it. The
    // ore-spawner animation index is attached later, once the terrain SHP frame
    // counts are known.
    if let Some(rules) = rules {
        let constructed = if let Some(overlay_registry) = authored_overlay_registry {
            crate::sim::terrain_spawn::construct_authored_terrain_objects(
                sim,
                &map_data.terrain_objects,
                rules,
                theater_name.eq_ignore_ascii_case("SNOW"),
                overlay_registry,
            )?
        } else {
            crate::sim::terrain_spawn::construct_terrain_objects(
                sim,
                &map_data.terrain_objects,
                rules,
                theater_name.eq_ignore_ascii_case("SNOW"),
            )
        };
        if constructed > 0 {
            log::info!("Constructed {constructed} map terrain objects before map entities");
        }
    } else {
        log::warn!("No rules loaded — skipping terrain object construction");
    }
    if let (Some(rules), Some(overlay_registry)) = (rules, authored_overlay_registry) {
        // The live grid is read through a temporary view and handed back.
        let live_grid = sim.overlay_grid.take();
        let stats = initialize_native_tiberium_queues(
            sim,
            &map_data.basic,
            &map_data.special_flags,
            rules,
            overlay_registry,
            live_grid.as_ref(),
            (map_data.header.width as u16, map_data.header.height as u16),
        );
        sim.overlay_grid = live_grid;
        if let Some(stats) = stats {
            log::info!(
                "Initialized authored native tiberium queues before Technos: {} growth, {} spread",
                stats.growth_entries,
                stats.spread_entries,
            );
        }
    }
    let authored_entity_terrain =
        authored_overlay_registry.and_then(|_| sim.resolved_terrain.as_ref().cloned());
    let entity_terrain = authored_entity_terrain.as_ref().unwrap_or(resolved_terrain);
    if !map_data.entities.is_empty() || generated_inits.is_some() {
        let _count: u32 = project_map_entities(
            sim,
            &map_data.entities,
            rules,
            height_map,
            Some(entity_terrain),
            overlay_registry,
            generated_inits,
        )?;
        let miner_count: usize = sim
            .entities()
            .values()
            .filter(|e| e.miner.is_some())
            .count();
        log::info!("Miner components attached: {}", miner_count);
    }
    if authored_overlay_registry.is_none() && !resolved_terrain.tile_animations().is_empty() {
        let rules = rules.expect("resolved terrain animations require bound art/rules data");
        let spawned = spawn_terrain_tile_animations(sim, rules, resolved_terrain.tile_animations());
        log::info!(
            "Spawned {} terrain-attached animations after map objects",
            spawned.len()
        );
    }
    Ok(())
}

/// Native growth-then-spread queue initialization from a read-only view of
/// the then-current cells. Authored `Full_Init` runs it between the Terrain
/// and Techno sections; a generated launch runs it in the
/// `RandomMapGenerator::Generate @ 0x00598960` tail after the generator
/// constructors and before `InitCellAttributes(1)` germinates the densities.
///
/// gamemd-derived: `TiberiumClass::InitGrowthQueues_All @ 0x00722D00` then
/// `TiberiumClass::InitSpreadQueues_All @ 0x00722240`, each freeing and
/// rebuilding every TiberiumClass's queue from the current cell state.
/// Returns `None` when no overlay grid exists (queues reset empty).
pub(crate) fn initialize_native_tiberium_queues(
    sim: &mut Simulation,
    basic: &crate::map::basic::BasicSection,
    special_flags: &crate::map::basic::SpecialFlagsSection,
    rules: &RuleSet,
    overlay_registry: &crate::map::overlay_types::OverlayTypeRegistry,
    overlay_grid: Option<&crate::sim::overlay_grid::OverlayGrid>,
    native_rect: (u16, u16),
) -> Option<crate::sim::ore_growth::NativeTiberiumRebuildStats> {
    sim.production.ore_growth_config =
        crate::sim::ore_growth::OreGrowthConfig::resolve(basic, special_flags, &sim.session);
    let (width, height) = overlay_grid
        .map(|grid| (grid.width(), grid.height()))
        .or_else(|| {
            sim.resolved_terrain
                .as_ref()
                .map(|terrain| (terrain.width(), terrain.height()))
        })
        .unwrap_or((0, 0));
    sim.production.ore_growth_state = crate::sim::ore_growth::OreGrowthState::new(width, height);
    // `CellClass+0xE4 FirstObject != 0` at this point: terrain objects plus
    // every ground-list Techno already constructed (none on an authored load,
    // the generator's own Technos on a generated one).
    let mut source_object_cells = sim
        .production
        .terrain_object_cells
        .keys()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    source_object_cells.extend(
        sim.substrate
            .occupancy
            .occupied_cells_on_layer(crate::sim::movement::locomotor::MovementLayer::Ground),
    );
    let Some(overlay_grid) = overlay_grid else {
        // No overlay grid: no stores to seed, but keep the `[Map] Size` rect
        // so a later snapshot restore rebuilds from it, not the storage dims.
        sim.production
            .ore_growth_state
            .reset_native_tiberium_classes_for_rect(native_rect, 0, sim.session.binary_frame);
        return None;
    };
    Some(
        sim.production
            .ore_growth_state
            .rebuild_native_tiberium_queues_from_overlays(
                overlay_grid,
                overlay_registry,
                &rules.tiberium_types,
                sim.resolved_terrain.as_ref(),
                &source_object_cells,
                sim.production.ore_growth_config.grows,
                sim.production.ore_growth_config.spreads,
                sim.session.binary_frame,
                native_rect,
            ),
    )
}

#[derive(Debug)]
pub(crate) struct AuthoredScenarioLoadOutput {
    pub(crate) resolved_terrain: crate::map::resolved_terrain::ResolvedTerrainGrid,
    pub(crate) overlay_grid: crate::sim::overlay_grid::OverlayGrid,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum AuthoredScenarioLoadError {
    #[error(transparent)]
    NativeIdentity(#[from] crate::sim::native_identity::NativeMapTubeConstructionError),
    #[error(transparent)]
    TubeRegistry(#[from] crate::map::resolved_terrain::NativeTubeRegistryBindError),
    #[error(transparent)]
    OverlayPacks(#[from] crate::map::map_file::OverlayPacksAlreadyConsumed),
    #[error(transparent)]
    OverlayFinalize(
        #[from]
        crate::map::authored_overlay::AuthoredOverlayFinalizeError<
            crate::sim::world::authored_load_host::SimulationAuthoredLoadError,
        >,
    ),
    #[error(transparent)]
    GeneratedTechno(#[from] crate::sim::world::GeneratedTechnoInitError),
}

/// Consume the fresh authored map-load corridor on the one staged Simulation.
/// One `LoadCellRecalcState` and one native-ID cursor span raw Tubes,
/// OverlayPack + first Recalc, Terrain/queues/Technos/Smudges, scalar deletion,
/// and the final anti-diagonal Recalc sweep.
#[allow(clippy::too_many_arguments)]
pub(crate) fn finalize_and_populate_staged_authored_scenario<F>(
    sim: &mut Simulation,
    map_data: &mut crate::map::map_file::MapFile,
    terrain_fill: crate::map::resolved_terrain::AuthoredTerrainFill,
    theater_data: &crate::map::theater::TheaterData,
    assets: &crate::assets::asset_manager::AssetManager,
    rules: &RuleSet,
    art: &mut crate::rules::art_data::ArtRegistry,
    overlay_registry: &crate::map::overlay_types::OverlayTypeRegistry,
    overlay_shp_ids: &std::collections::BTreeSet<u8>,
    signed_new_ini_format: i32,
    lat_enabled: bool,
    cliff_back_impassability: u8,
    theater_ext: &str,
    theater_name: &str,
    bridge_destroyability_mode: crate::map::basic::BridgeDestroyabilityMode,
    descriptor: &crate::sim::scenario_session::ScenarioDescriptor,
    initialize_houses_before_objects: F,
) -> Result<AuthoredScenarioLoadOutput, AuthoredScenarioLoadError>
where
    F: FnOnce(&mut Simulation),
{
    let mut terrain = terrain_fill.into_pending_grid();
    let native_tubes = sim.take_native_map_tubes_receipt()?;
    terrain.bind_native_map_tubes(native_tubes)?;
    let mut recalc = crate::map::resolved_terrain::LoadCellRecalcState::for_authored_load(
        map_data,
        theater_data,
        assets,
        &rules.terrain_rules,
        overlay_registry,
        lat_enabled,
        cliff_back_impassability,
        terrain.cells().len(),
        sim.session.pixel_conversion_bounds,
    );
    let shape = crate::map::authored_overlay::NativeOverlayMapShape::new(
        i32::try_from(map_data.header.width).unwrap_or(i32::MAX),
        i32::try_from(map_data.header.height).unwrap_or(i32::MAX),
    );
    let packs = map_data.take_authored_overlay_packs()?;
    let payload = {
        let mut host = crate::sim::world::authored_load_host::SimulationAuthoredLoadHost::new(
            sim,
            art,
            assets,
            theater_ext,
            theater_name,
        );
        crate::map::authored_overlay::AuthoredOverlayFinalizer::with_recalc(
            &mut terrain,
            &mut recalc,
            shape,
            overlay_registry,
            &rules.tiberium_types,
            overlay_shp_ids,
            signed_new_ini_format,
            descriptor.game_mode_nonzero,
            &mut host,
        )
        .run(packs)?
    };
    let overlay_grid = crate::sim::overlay_grid::OverlayGrid::from_finalized_map_payload(payload);
    let construction_height_map = terrain.build_height_map();

    match populate_staged_scenario_inner(
        sim,
        map_data,
        &terrain,
        theater_name,
        Some(rules),
        &construction_height_map,
        Some(overlay_registry),
        Some(&overlay_grid),
        Some(overlay_registry),
        bridge_destroyability_mode,
        descriptor,
        None,
        initialize_houses_before_objects,
    ) {
        Ok(()) => {}
        Err(ScenarioPopulationError::GeneratedTechno(error)) => return Err(error.into()),
        Err(ScenarioPopulationError::NativeIdentity(error)) => return Err(error.into()),
    }

    let mut terrain = sim
        .resolved_terrain
        .take()
        .expect("authored object population keeps the live terrain installed");
    let mut overlay_grid = sim
        .overlay_grid
        .take()
        .expect("authored object population keeps the live overlay installed");

    // SmudgeClass constructors consume identity before Unlimbo/placement can
    // reject a row. Install the resulting first-sweep grid now; the shared
    // post-map finalizer preserves it instead of rebuilding against later
    // final-sweep cells.
    for entry in &map_data.smudges {
        if rules.smudge_types.find_by_name(&entry.type_name).is_some() {
            let _native_unique_id = sim.next_native_load_id()?;
        }
    }
    sim.smudge_grid = Some(crate::sim::smudge_grid::SmudgeGrid::from_map_entries(
        &map_data.smudges,
        &rules.smudge_types,
        &terrain,
        &overlay_grid,
        terrain.width(),
        terrain.height(),
    ));
    sim.flush_smudge_dirty();

    let removed = sim.scalar_delete_load_terrain_anims();
    if removed > 0 {
        log::info!("Scalar-deleted {removed} first-sweep terrain animations");
    }
    // `MapClass::InitCellAttributes @ 0x00568BB0` with argument 0 calls
    // `CellClass::Get_Tiberium_Value @ 0x00485020` for every real cell before
    // that cell's Recalc and sums the returns with wrapping machine arithmetic;
    // `ScenarioClass::Full_Init @ 0x00686B20` stores the return at
    // `MapClass+0x134` (`0x0087F91C`).
    let mut tiberium_value_total: i32 = 0;
    {
        let mut host = crate::sim::world::authored_load_host::SimulationAuthoredLoadHost::new(
            sim,
            art,
            assets,
            theater_ext,
            theater_name,
        );
        for (x, y) in shape.recalc_cells() {
            let Some(index) = terrain.native_fixed_cell_index(x, y) else {
                debug_assert!(false, "final authored iterator reached an unallocated cell");
                continue;
            };
            recalc.clear_terrain_anim_latch(index);
            let (rx, ry) = (x as u16, y as u16);
            let current = overlay_grid
                .finalized_map_cell(rx, ry)
                .expect("final authored iterator coordinate remains in the live overlay grid");
            tiberium_value_total =
                tiberium_value_total.wrapping_add(crate::sim::ore_twinkle::tiberium_value(
                    current.overlay_id(),
                    current.state(),
                    overlay_registry,
                    &rules.tiberium_types,
                ));
            let finalized = crate::map::authored_overlay::recalc_final_authored_overlay_cell(
                &mut terrain,
                &mut recalc,
                index,
                current,
                &mut host,
            )?;
            let written = overlay_grid.write_finalized_map_cell(rx, ry, finalized);
            debug_assert!(written);
        }
    }
    sim.authored_tiberium_value_total = Some(tiberium_value_total);
    terrain.rebuild_bridgehead_anchor_classes_from_final_tiles(theater_data);
    let bridge_destroyable = map_data
        .special_flags
        .effective_destroyable_bridges(bridge_destroyability_mode);
    let bridge_state =
        crate::sim::bridge_state::BridgeRuntimeState::from_resolved_terrain_with_map_size(
            &terrain,
            bridge_destroyable,
            rules.bridge_rules.strength,
            (map_data.header.width as i32, map_data.header.height as i32),
        );

    let final_terrain = terrain.clone();
    let final_overlay = overlay_grid.clone();
    sim.install_resolved_terrain_for_new_map(terrain);
    sim.overlay_grid = Some(overlay_grid);
    sim.bridge_state = Some(bridge_state);
    Ok(AuthoredScenarioLoadOutput {
        resolved_terrain: final_terrain,
        overlay_grid: final_overlay,
    })
}

/// BuildingClass::GetCoords projects the stored north-west anchor to the
/// foundation centre before distance consumers receive it.
fn project_building_get_coords_xy(
    northwest_x: i32,
    northwest_y: i32,
    foundation_width: u16,
    foundation_height: u16,
) -> (i32, i32) {
    let x_offset = i32::from(foundation_width)
        .wrapping_sub(1)
        .wrapping_mul(128);
    let y_offset = i32::from(foundation_height)
        .wrapping_sub(1)
        .wrapping_mul(128);
    (
        northwest_x.wrapping_add(x_offset),
        northwest_y.wrapping_add(y_offset),
    )
}

pub(crate) fn map_wall_owner_candidate_from_building(
    entity: &crate::sim::game_entity::GameEntity,
    resolved_terrain: &crate::map::resolved_terrain::ResolvedTerrainGrid,
    house_wall_owner: bool,
) -> crate::sim::overlay_grid::MapWallOwnerCandidate {
    let northwest_x = i32::from(entity.position.rx)
        .wrapping_mul(256)
        .wrapping_add(entity.position.sub_x.to_num::<i32>());
    let northwest_y = i32::from(entity.position.ry)
        .wrapping_mul(256)
        .wrapping_add(entity.position.sub_y.to_num::<i32>());
    let world_z = resolved_terrain
        .cell(entity.position.rx, entity.position.ry)
        .and_then(|cell| {
            crate::util::lepton::ground_height_leptons(
                cell.level,
                cell.slope_type,
                northwest_x,
                northwest_y,
            )
            .ok()
        })
        .unwrap_or(i32::from(entity.position.z) * crate::util::lepton::LEPTONS_PER_LEVEL as i32);
    let (foundation_width, foundation_height) =
        crate::sim::production::foundation_dimensions(&entity.foundation);
    let (world_x, world_y) = project_building_get_coords_xy(
        northwest_x,
        northwest_y,
        foundation_width,
        foundation_height,
    );

    crate::sim::overlay_grid::MapWallOwnerCandidate {
        owner: entity.owner(),
        world_x,
        world_y,
        world_z,
        foundation_width,
        foundation_height,
        object_alive: entity.lifecycle.object_alive,
        cell_marked: entity.lifecycle.cell_marked,
        house_wall_owner,
    }
}

/// Post-funnel scenario finalization shared by the app loader and the
/// headless loader (F09). The ordering is part of the construction contract:
/// ore-spawner terrain seeding, map-wall owner reconstruction from the
/// spawned structures, overlay-grid installation, smudge-grid seeding, then
/// the authoritative post-map tail (`ScenarioClass::Post_Map_Init` cone).
pub(crate) fn finalize_constructed_scenario(
    sim: &mut Simulation,
    map_data: &crate::map::map_file::MapFile,
    rules: &RuleSet,
    overlay_registry: &crate::map::overlay_types::OverlayTypeRegistry,
    mut overlay_grid: crate::sim::overlay_grid::OverlayGrid,
    house_roster: &crate::map::houses::HouseRoster,
    skirmish_session: Option<&crate::sim::scenario_bootstrap::MatchLaunchDescriptor>,
    tiberium_queues_preinitialized: bool,
) -> crate::sim::scenario_post_map::ScenarioPostMapOutput {
    // Attach the TIBTRE ore-spawner animation index to the terrain objects
    // constructed ahead of the map entities. Its authoritative raw SHP count
    // is rules-owned; presentation atlases retain only body-frame ranges.
    let seeded_terrain = crate::sim::terrain_spawn::seed_terrain_spawner_animation(sim, rules);
    if seeded_terrain > 0 {
        log::info!(
            "Seeded {} ore-spawning terrain objects (TIBTRE)",
            seeded_terrain,
        );
    }
    // Move the already-resolved CellClass overlay state into Simulation,
    // then reconstruct map-wall ownership now that buildings exist.
    if sim.resolved_terrain.is_some() {
        let (grid_width, grid_height) = (overlay_grid.width(), overlay_grid.height());
        let rt = sim
            .resolved_terrain
            .as_ref()
            .expect("terrain checked above");
        let buildings: Vec<crate::sim::overlay_grid::MapWallOwnerCandidate> = sim
            .substrate
            .entities
            .values()
            .filter(|entity| entity.category == crate::map::entities::EntityCategory::Structure)
            .map(|entity| {
                let country = sim
                    .houses
                    .get(&entity.owner())
                    .and_then(|house| house.country)
                    .map(|country| sim.interner.resolve(country));
                map_wall_owner_candidate_from_building(
                    entity,
                    rt,
                    crate::sim::house_state::resolve_wall_owner(Some(rules), country),
                )
            })
            .collect();
        overlay_grid.reconstruct_map_wall_owners(rt, overlay_registry, &buildings);
        sim.overlay_grid = Some(overlay_grid);
        log::info!(
            "Overlay grid initialized: {}x{}, {} entries",
            grid_width,
            grid_height,
            map_data.overlays.len(),
        );
    }
    // Seed smudge grid from map [Smudge] entries. Requires terrain +
    // overlay grids built above so placement gates (slope, overlay,
    // accepts_smudge) can reject invalid map entries at load.
    if sim.smudge_grid.is_none()
        && let (Some(rt), Some(overlay)) =
            (sim.resolved_terrain.as_ref(), sim.overlay_grid.as_ref())
    {
        let grid_width = rt.width();
        let grid_height = rt.height();
        sim.smudge_grid = Some(crate::sim::smudge_grid::SmudgeGrid::from_map_entries(
            &map_data.smudges,
            &rules.smudge_types,
            rt,
            overlay,
            grid_width,
            grid_height,
        ));
        sim.flush_smudge_dirty();
        log::info!(
            "Smudge grid initialized: {}x{}, {} entries",
            grid_width,
            grid_height,
            map_data.smudges.len(),
        );
    }
    // The caller submits one immutable initialization command; Simulation owns
    // every match-affecting write and Scenario RNG draw in the post-map tail.
    let output =
        sim.finalize_scenario_post_map(crate::sim::scenario_post_map::ScenarioPostMapInput {
            map_width: map_data.header.width as u16,
            map_height: map_data.header.height as u16,
            basic: &map_data.basic,
            special_flags: &map_data.special_flags,
            normal_lighting: crate::map::lighting::parse_lighting_profiles(&map_data.ini).normal,
            rules,
            overlay_registry,
            house_roster,
            skirmish_session,
            tiberium_queues_preinitialized,
        });
    sim.discard_lighting_events();
    output
}
