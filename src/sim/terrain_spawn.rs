//! TIBTRE-style terrain object ore spawning.
//!
//! Per-cell retained animation state for terrain objects with `IsAnimated=yes`.
//! Idle animations roll `AnimationProbability`; a hit starts the native timer.
//! `SpawnsTiberium=yes` resets the animation and emits ore at its midpoint.
//!
//! ## Animation model
//! Two-phase: roll succeeds -> start at frame 0 -> advance one frame every
//! `AnimationRate` ticks -> reset to idle at midpoint -> forced tiberium spread.
//!
//! ## Dependency rules
//! - Part of sim/ - depends on rules data, sim/overlay_grid, sim/tiberium and
//!   sim/rng.
//! - Per-object animation config is baked into TerrainAnimationState at seed time
//!   (mirrors OreGrowthConfig pattern); live placement gates still read entity
//!   and rules state for building exceptions.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use std::collections::{BTreeMap, BTreeSet};

use crate::map::overlay_types::OverlayTypeRegistry;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::ruleset::RuleSet;
use crate::sim::entity_store::EntityStore;
use crate::sim::intern::{InternedId, StringInterner};
use crate::sim::occupancy::OccupancyGrid;
use crate::sim::ore_growth::OreGrowthState;
use crate::sim::overlay_grid::OverlayGrid;
use crate::sim::rng::SimRng;
use crate::sim::terrain_object::{TerrainObjectState, mark_terrain_raw_occupation};
use crate::sim::tiberium::{
    NewTiberiumAdmission, PlaceTiberiumContext, TiberiumPlacement, TiberiumPlacementObjectContext,
    spread_tiberium,
};
use crate::sim::timer::CdTimer;
use crate::util::native_x87::{MaskedX87Chop53, MaskedX87Ordering, NativeF32Bits, NativeF64Bits};

/// Native Terrain AI71C745..71C796: signed abs/remainder, PC53/chop
/// multiply by the original1e-6 double, then FCOM/SAHF/JNC. This preserves
/// stock .003's admitted sample3000 and INT_MIN's negative remainder.
/// Original execution: tools/spatial_oracle/tibtre.json.
fn probability_accepts_raw(probability: NativeF32Bits, raw: u32) -> bool {
    let remainder = (raw as i32).wrapping_abs() % 1_000_000;
    let sample = MaskedX87Chop53::mul(
        MaskedX87Chop53::load_i32(remainder),
        MaskedX87Chop53::load_f64(NativeF64Bits::from_bits(0x3eb0_c6f7_a0b5_ed8d)),
    );
    matches!(
        MaskedX87Chop53::compare(sample, MaskedX87Chop53::load_f32(probability)),
        MaskedX87Ordering::Less | MaskedX87Ordering::Unordered
    )
}

/// One live TerrainClass animation, retained by the terrain animation owner.
/// Type data is immutable; frame/rate/timer are mutated only by this module.
/// Terrain removal/limbo removes the cell index with the live terrain object.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct TerrainAnimationState {
    type_ref: InternedId,
    animation_probability: NativeF32Bits,
    animation_rate_ticks: i32,
    frame_count: i16,
    current_frame: i32,
    rate: i32,
    timer: CdTimer,
}

impl TerrainAnimationState {
    pub fn new(
        type_ref: InternedId,
        animation_probability: NativeF32Bits,
        animation_rate_ticks: i32,
        frame_count: u16,
        binary_frame: u32,
    ) -> Self {
        Self {
            type_ref,
            animation_probability,
            animation_rate_ticks,
            frame_count: frame_count as i16,
            current_frame: 0,
            rate: 0,
            // Terrain constructor/reset71BB9E..71BBD1,71BC86..71BCA5.
            timer: CdTimer::started(binary_frame as i32, 0),
        }
    }

    pub fn type_ref(&self) -> InternedId {
        self.type_ref
    }

    /// Terrain DrawIt71C208 reads this retained Stage value directly.
    pub fn current_frame(&self) -> i32 {
        self.current_frame
    }

    pub fn is_active(&self) -> bool {
        self.rate != 0
    }

    /// Terrain AI71C730, including the same-visit timer test after a hit.
    /// Stock resets Stage/rate/timer before its forced spread at midpoint11.
    fn tick(
        &mut self,
        binary_frame: u32,
        is_animated: bool,
        spawns_tiberium: bool,
        rng: &mut SimRng,
    ) -> bool {
        let now = binary_frame as i32;
        if is_animated
            && self.rate == 0
            && probability_accepts_raw(self.animation_probability, rng.next_u32())
        {
            self.current_frame = 0;
            self.rate = self.animation_rate_ticks;
            self.timer.start(now, self.rate);
        }
        if !self.timer.expired(now) || self.rate == 0 {
            return false;
        }
        self.current_frame = self.current_frame.wrapping_add(1);
        self.timer.start(now, self.rate);
        if is_animated && spawns_tiberium && self.current_frame == i32::from(self.frame_count) / 2 {
            self.current_frame = 0;
            self.rate = 0;
            self.timer.start(now, 0);
            return true;
        }
        false
    }

    #[cfg(test)]
    pub(crate) fn advance_for_test(&mut self, binary_frame: u32, rng: &mut SimRng) {
        self.tick(binary_frame, true, true, rng);
    }
}

/// Short-lived mutation context for the stateful terrain spawner tick.
pub struct TerrainSpawnContext<'a> {
    pub overlay_grid: Option<&'a mut OverlayGrid>,
    pub resolved_terrain: Option<&'a ResolvedTerrainGrid>,
    pub overlay_registry: Option<&'a OverlayTypeRegistry>,
    pub ore_growth_state: Option<&'a mut OreGrowthState>,
    growth_enabled: bool,
    spread_enabled: bool,
    pub radar_dirty_cells: Option<&'a mut Vec<(u16, u16)>>,
    pub radar_dirty_generation: Option<&'a mut u64>,
    pub tactical_dirty_cells: Option<&'a mut Vec<(u16, u16)>>,
    pub binary_frame: u32,
    pub spawning_terrain_cells: Option<&'a BTreeSet<(u16, u16)>>,
    pub entities: Option<&'a EntityStore>,
    pub occupancy: Option<&'a OccupancyGrid>,
    pub rules: Option<&'a RuleSet>,
    pub interner: Option<&'a StringInterner>,
    /// Terrain objects (trees, rocks) indexed by cell — the non-Techno half of the
    /// native `Cell+0xE4` FirstObject list that `CanSpreadTiberium` reads.
    pub terrain_object_cells: Option<&'a BTreeMap<(u16, u16), u64>>,
    pub rng: &'a mut SimRng,
}

impl<'a> TerrainSpawnContext<'a> {
    pub fn new(overlay_grid: Option<&'a mut OverlayGrid>, rng: &'a mut SimRng) -> Self {
        Self {
            overlay_grid,
            resolved_terrain: None,
            overlay_registry: None,
            ore_growth_state: None,
            growth_enabled: true,
            spread_enabled: true,
            radar_dirty_cells: None,
            radar_dirty_generation: None,
            tactical_dirty_cells: None,
            binary_frame: 0,
            spawning_terrain_cells: None,
            entities: None,
            occupancy: None,
            rules: None,
            interner: None,
            terrain_object_cells: None,
            rng,
        }
    }

    pub fn with_validation_context(
        mut self,
        resolved_terrain: Option<&'a ResolvedTerrainGrid>,
        overlay_registry: Option<&'a OverlayTypeRegistry>,
    ) -> Self {
        self.resolved_terrain = resolved_terrain;
        self.overlay_registry = overlay_registry;
        self
    }

    pub fn with_growth_queue(
        mut self,
        ore_growth_state: &'a mut OreGrowthState,
        binary_frame: u32,
        growth_enabled: bool,
        spread_enabled: bool,
    ) -> Self {
        self.ore_growth_state = Some(ore_growth_state);
        self.binary_frame = binary_frame;
        self.growth_enabled = growth_enabled;
        self.spread_enabled = spread_enabled;
        self
    }

    pub fn with_dirty_tracking(
        mut self,
        radar_dirty_cells: &'a mut Vec<(u16, u16)>,
        radar_dirty_generation: &'a mut u64,
        tactical_dirty_cells: &'a mut Vec<(u16, u16)>,
    ) -> Self {
        self.radar_dirty_cells = Some(radar_dirty_cells);
        self.radar_dirty_generation = Some(radar_dirty_generation);
        self.tactical_dirty_cells = Some(tactical_dirty_cells);
        self
    }

    pub fn with_spawning_terrain_cells(mut self, cells: &'a BTreeSet<(u16, u16)>) -> Self {
        self.spawning_terrain_cells = Some(cells);
        self
    }

    pub fn with_live_object_context(
        mut self,
        entities: &'a EntityStore,
        occupancy: &'a OccupancyGrid,
        rules: &'a RuleSet,
        interner: &'a StringInterner,
        terrain_object_cells: &'a BTreeMap<(u16, u16), u64>,
    ) -> Self {
        self.entities = Some(entities);
        self.occupancy = Some(occupancy);
        self.rules = Some(rules);
        self.interner = Some(interner);
        self.terrain_object_cells = Some(terrain_object_cells);
        self
    }
}

/// Test adapter over the same single-object animation and spread owners.
/// Production dispatch follows retained Logic order, not this cell-key order.
#[cfg(test)]
pub fn tick_terrain_animations_stateful(
    spawners: &mut BTreeMap<(u16, u16), TerrainAnimationState>,
    mut ctx: TerrainSpawnContext<'_>,
) {
    if spawners.is_empty() {
        return;
    }

    let spawner_cells: BTreeSet<(u16, u16)> = spawners.keys().copied().collect();
    for &cell in &spawner_cells {
        tick_terrain_spawner_one_inner(spawners, cell, &spawner_cells, &mut ctx);
    }
}

/// Dispatch one TerrainClass AI slot through the retained animation owner.
pub(crate) fn tick_terrain_spawner_stateful_one(
    spawners: &mut BTreeMap<(u16, u16), TerrainAnimationState>,
    cell: (u16, u16),
    spawner_cells: &BTreeSet<(u16, u16)>,
    mut ctx: TerrainSpawnContext<'_>,
) -> Option<TiberiumPlacement> {
    tick_terrain_spawner_one_inner(spawners, cell, spawner_cells, &mut ctx)
}

fn tick_terrain_spawner_one_inner(
    spawners: &mut BTreeMap<(u16, u16), TerrainAnimationState>,
    cell: (u16, u16),
    spawner_cells: &BTreeSet<(u16, u16)>,
    ctx: &mut TerrainSpawnContext<'_>,
) -> Option<TiberiumPlacement> {
    let spawner = spawners.get_mut(&cell)?;
    let source_cells = ctx.spawning_terrain_cells.unwrap_or(spawner_cells);
    let is_animated = ctx
        .rules
        .zip(ctx.interner)
        .and_then(|(rules, interner)| {
            rules.terrain_object_type_case_insensitive(interner.resolve(spawner.type_ref))
        })
        .is_none_or(|ty| ty.is_animated);
    if !spawner.tick(
        ctx.binary_frame,
        is_animated,
        source_cells.contains(&cell),
        ctx.rng,
    ) {
        return None;
    }
    let (Some(grid), Some(registry), Some(state), Some(rules), Some(terrain), Some(objects)) = (
        ctx.overlay_grid.as_deref_mut(),
        ctx.overlay_registry,
        ctx.ore_growth_state.as_deref_mut(),
        ctx.rules,
        ctx.resolved_terrain,
        live_object_context(
            ctx.entities,
            ctx.occupancy,
            ctx.rules,
            ctx.interner,
            ctx.terrain_object_cells,
        ),
    ) else {
        return None;
    };
    let mut placement = PlaceTiberiumContext {
        overlay_grid: grid,
        ore_growth_state: state,
        overlay_registry: registry,
        tiberium_types: &rules.tiberium_types,
        resolved_terrain: Some(terrain),
        source_object_cells: source_cells,
        new_cell_admission: Some(NewTiberiumAdmission::runtime(terrain, objects)),
        live_objects: Some(objects.object_view()),
        rng: ctx.rng,
        binary_frame: ctx.binary_frame,
        growth_enabled: ctx.growth_enabled,
        spread_enabled: ctx.spread_enabled,
        radar_dirty_cells: ctx.radar_dirty_cells.as_deref_mut(),
        radar_dirty_generation: ctx.radar_dirty_generation.as_deref_mut(),
        tactical_dirty_cells: ctx.tactical_dirty_cells.as_deref_mut(),
    };
    spread_tiberium(&mut placement, cell, true)
}

/// Dispatch the TerrainClass AI leaf for one current LogicClass slot.
pub(crate) fn tick_terrain_object_ai(
    sim: &mut crate::sim::world::Simulation,
    stable_id: u64,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    overlay_registry: Option<&OverlayTypeRegistry>,
    spawner_cells: Option<&BTreeSet<(u16, u16)>>,
) {
    let Some(cell) = sim
        .production
        .terrain_objects
        .get(&stable_id)
        .filter(|terrain| terrain.is_live())
        .map(TerrainObjectState::cell)
    else {
        return;
    };
    if !sim.production.terrain_animations.contains_key(&cell) {
        return;
    }
    let Some(rules) = rules else {
        return;
    };

    let fallback_spawner_cells;
    let spawner_cells = if let Some(spawner_cells) = spawner_cells {
        spawner_cells
    } else {
        fallback_spawner_cells = sim
            .production
            .terrain_animations
            .keys()
            .copied()
            .collect::<BTreeSet<_>>();
        &fallback_spawner_cells
    };
    let production = &mut sim.production;
    let placed = tick_terrain_spawner_stateful_one(
        &mut production.terrain_animations,
        cell,
        spawner_cells,
        TerrainSpawnContext::new(sim.overlay_grid.as_mut(), &mut sim.scenario_rng)
            .with_growth_queue(
                &mut production.ore_growth_state,
                sim.session.binary_frame,
                production.ore_growth_config.grows,
                production.ore_growth_config.spreads,
            )
            .with_dirty_tracking(
                &mut sim.radar_terrain_dirty_cells,
                &mut sim.radar_terrain_dirty_generation,
                &mut sim.tactical_dirty_cells,
            )
            .with_spawning_terrain_cells(&production.tiberium_spawning_terrain_cells)
            .with_live_object_context(
                &sim.substrate.entities,
                &sim.substrate.occupancy,
                rules,
                &sim.interner,
                &production.terrain_object_cells,
            )
            .with_validation_context(sim.resolved_terrain.as_ref(), overlay_registry),
    );
    // Overlay Mark5FC570 ->Cell Recalc47D2B0 is synchronous: a later
    // Logic object (including a miner) sees the new LandType this frame.
    let placed_cell = placed.as_ref().map(TiberiumPlacement::cell);
    if let (Some(cell), Some(registry)) = (placed_cell, overlay_registry) {
        sim.publish_tiberium_cells(rules, registry, &[cell]);
    }
    sim.publish_overlay_constructions(
        placed.and_then(TiberiumPlacement::into_overlay_construction),
    );
}

fn live_object_context<'a>(
    entities: Option<&'a EntityStore>,
    occupancy: Option<&'a OccupancyGrid>,
    rules: Option<&'a RuleSet>,
    interner: Option<&'a StringInterner>,
    terrain_object_cells: Option<&'a BTreeMap<(u16, u16), u64>>,
) -> Option<TiberiumPlacementObjectContext<'a>> {
    Some(TiberiumPlacementObjectContext::new(
        entities?,
        occupancy?,
        rules?,
        interner?,
        terrain_object_cells?,
    ))
}

/// Apply `TerrainClass::Unlimbo @ 0x0071D000` source-cell tiberium clearing
/// before map-derived navigation and simulation state are published.
///
/// This is a map-load projection, not a runtime overlay mutation, so cleared
/// cells do not enter the dirty-cell output queue. Both graphical and headless
/// loading must run the same projection before deriving height/path state.
pub fn clear_tiberium_source_cells_for_terrain(
    overlay_grid: &mut OverlayGrid,
    resolved_terrain: &mut ResolvedTerrainGrid,
    terrain_objects: &[crate::map::overlay::TerrainObject],
    rules: &RuleSet,
    overlay_registry: &OverlayTypeRegistry,
) -> BTreeSet<(u16, u16)> {
    let mut cleared_cells = BTreeSet::new();
    for terrain_object in terrain_objects {
        if rules
            .terrain_object_type_case_insensitive(&terrain_object.name)
            .is_none()
        {
            continue;
        }
        let Some(overlay_id) = overlay_grid
            .cell(terrain_object.rx, terrain_object.ry)
            .overlay_id
        else {
            continue;
        };
        if !overlay_registry
            .flags(overlay_id)
            .is_some_and(|flags| flags.tiberium)
        {
            continue;
        }

        *overlay_grid.cell_mut(terrain_object.rx, terrain_object.ry) = Default::default();
        crate::sim::overlay_grid::recalc_overlay_passability(
            overlay_grid,
            resolved_terrain,
            overlay_registry,
            terrain_object.rx,
            terrain_object.ry,
        );
        cleared_cells.insert((terrain_object.rx, terrain_object.ry));
    }

    cleared_cells
}

/// `TerrainClass::Read_Map_Section` — construct one live terrain object per
/// map `[Terrain]` entry.
///
/// gamemd reads `[Terrain]` while the map sections are being walked, *before*
/// `[Units]`, `[Aircraft]`, `[Infantry]` and `[Structures]`, so every tree
/// already owns its cell (occupation bits committed) by the time the first map
/// object is placed. Callers must run this before the map-entity spawn pass.
///
/// Returns the number of terrain objects constructed.
pub fn construct_terrain_objects(
    sim: &mut crate::sim::world::Simulation,
    terrain_objects: &[crate::map::overlay::TerrainObject],
    rules: &crate::rules::ruleset::RuleSet,
    snow_theater: bool,
) -> usize {
    construct_terrain_objects_inner(sim, terrain_objects, rules, snow_theater, None)
        .expect("compatibility Terrain construction does not require a native-ID cursor")
}

/// Fresh-authored variant. Each successful Terrain constructor spends and
/// retains its native ID, projects its exact occupation into the same live
/// CellClass grid, completes that immediate Recalc result, and only then clears
/// a same-cell resource overlay without a second Recalc. The final authored
/// Init sweep repairs the post-clear attributes.
pub(crate) fn construct_authored_terrain_objects(
    sim: &mut crate::sim::world::Simulation,
    terrain_objects: &[crate::map::overlay::TerrainObject],
    rules: &crate::rules::ruleset::RuleSet,
    snow_theater: bool,
    overlay_registry: &OverlayTypeRegistry,
) -> Result<usize, crate::sim::native_identity::NativeMapTubeConstructionError> {
    construct_terrain_objects_inner(
        sim,
        terrain_objects,
        rules,
        snow_theater,
        Some(overlay_registry),
    )
}

fn construct_terrain_objects_inner(
    sim: &mut crate::sim::world::Simulation,
    terrain_objects: &[crate::map::overlay::TerrainObject],
    rules: &crate::rules::ruleset::RuleSet,
    snow_theater: bool,
    authored_overlay_registry: Option<&OverlayTypeRegistry>,
) -> Result<usize, crate::sim::native_identity::NativeMapTubeConstructionError> {
    let old_terrain_ids = sim
        .production
        .terrain_objects
        .keys()
        .copied()
        .collect::<Vec<_>>();
    for stable_id in old_terrain_ids {
        sim.unregister_non_entity_object(stable_id);
    }
    sim.production.terrain_animations.clear();
    sim.production.terrain_objects.clear();
    sim.production.terrain_object_cells.clear();
    sim.production.terrain_occupation_bits.clear();
    sim.production.tiberium_spawning_terrain_cells.clear();

    let mut constructed = 0usize;
    for obj in terrain_objects {
        let Some(t) = rules.terrain_object_type_case_insensitive(&obj.name) else {
            continue;
        };
        let type_ref = sim.interner.intern(&obj.name);
        // TerrainClass construction reaches AbstractClass::Create_ID
        // @ 0x00410230, which draws from ScenarioClass::NextUniqueID
        // @ 0x0068BCB0 just like every other modeled runtime object.
        let stable_id = sim.allocate_stable_id();
        let native_unique_id = authored_overlay_registry
            .map(|_| sim.next_native_load_id())
            .transpose()?;
        let mut terrain_state = TerrainObjectState::new(
            stable_id,
            type_ref,
            (obj.rx, obj.ry),
            t,
            snow_theater,
            sim.resolved_terrain.as_ref(),
        );
        terrain_state.native_unique_id = native_unique_id;
        let occupation_bits = terrain_state.occupation_bits;
        if occupation_bits != 0 {
            sim.production
                .terrain_occupation_bits
                .insert((obj.rx, obj.ry), occupation_bits);
        }
        sim.production
            .terrain_object_cells
            .insert((obj.rx, obj.ry), stable_id);
        sim.production
            .terrain_objects
            .insert(stable_id, terrain_state);
        let registered = sim.register_terrain_object(stable_id, Some(rules));
        debug_assert!(registered);
        mark_terrain_raw_occupation(
            &mut sim.substrate.raw_cell_occupation,
            (obj.rx, obj.ry),
            occupation_bits,
        );
        if let Some(overlay_registry) = authored_overlay_registry {
            let terrain_snapshot = sim
                .production
                .terrain_objects
                .get(&stable_id)
                .expect("new Terrain remains registered")
                .clone();
            crate::sim::terrain_object::mark_terrain_occupation(
                &mut sim.production,
                &terrain_snapshot,
                sim.resolved_terrain.as_mut(),
            );
            let clears_resource = sim
                .overlay_grid
                .as_ref()
                .and_then(|grid| grid.cell(obj.rx, obj.ry).overlay_id)
                .and_then(|overlay_id| overlay_registry.flags(overlay_id))
                .is_some_and(|flags| flags.tiberium);
            if clears_resource {
                *sim.overlay_grid
                    .as_mut()
                    .expect("authored overlay grid was checked above")
                    .cell_mut(obj.rx, obj.ry) = Default::default();
            }
        }
        if t.spawns_tiberium {
            sim.production
                .tiberium_spawning_terrain_cells
                .insert((obj.rx, obj.ry));
        }
        constructed += 1;
    }
    Ok(constructed)
}

/// Attach the terrain animation index to already-constructed terrain objects.
///
/// Split out of construction because the overlay registry that selects the
/// default ore identity is installed later in the current map-load pipeline.
/// The authoritative raw SHP count is already bound on `RuleSet`; the renderer
/// neither supplies nor mutates this state.
///
/// Returns the number of animated live terrain objects indexed.
pub fn seed_terrain_spawner_animation(
    sim: &mut crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
) -> usize {
    sim.production.terrain_animations.clear();

    let candidates: Vec<(u64, (u16, u16), InternedId)> = sim
        .production
        .terrain_objects
        .values()
        .filter(|terrain| terrain.is_live())
        .map(|terrain| (terrain.stable_id, terrain.cell(), terrain.type_ref))
        .collect();

    let mut seeded = 0usize;
    for (stable_id, cell, type_ref) in candidates {
        // Two entries can name the same cell; only the object the cell index
        // points at owns that cell's spawner.
        if sim.production.terrain_object_cells.get(&cell) != Some(&stable_id) {
            continue;
        }
        let name = sim.interner.resolve(type_ref).to_string();
        let Some(t) = rules.terrain_object_type_case_insensitive(&name) else {
            continue;
        };
        if !t.is_animated {
            continue;
        }
        let frame_count = rules.terrain_spawner_frame_count(&name).unwrap_or(0);
        sim.production.terrain_animations.insert(
            cell,
            TerrainAnimationState::new(
                type_ref,
                t.animation_probability,
                t.animation_rate,
                frame_count,
                sim.session.binary_frame,
            ),
        );
        seeded += 1;
    }
    seeded
}

/// Construct terrain objects and seed their spawner index in one call.
///
/// Test convenience. The production load path calls the two halves separately
/// so construction keeps its native position ahead of `[Units]`.
#[cfg(test)]
pub fn seed_terrain_animations(
    sim: &mut crate::sim::world::Simulation,
    terrain_objects: &[crate::map::overlay::TerrainObject],
    rules: &crate::rules::ruleset::RuleSet,
    snow_theater: bool,
) -> usize {
    construct_terrain_objects(sim, terrain_objects, rules, snow_theater);
    seed_terrain_spawner_animation(sim, rules)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    use crate::assets::asset_manager::AssetManager;
    use crate::map::bridge_facts::{
        BRIDGE_FLAG_DESTROYED_OR_RAMP, BRIDGE_FLAG_STRUCTURAL, BRIDGE_FLAG_TRANSITION,
    };
    use crate::map::entities::EntityCategory;
    use crate::map::overlay_types::OverlayTypeRegistry;
    use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::rules::terrain_rules::TerrainClass;
    use crate::sim::entity_store::EntityStore;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::intern::StringInterner;
    use crate::sim::movement::locomotor::MovementLayer;
    use crate::sim::occupancy::{CellListInsertion, OccupancyGrid};
    use crate::sim::ore_growth::OreGrowthState;
    use crate::sim::tiberium::{
        ADJACENT_OFFSETS, admit_new_tiberium_target, resolved_cell_accepts_tiberium,
    };

    #[test]
    fn probability_boundaries_match_original_executable_controls() {
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/tibtre.json",
        ))
        .unwrap();
        for row in native["probability_controls"].as_array().unwrap() {
            let input = &row["input"];
            assert_eq!(
                probability_accepts_raw(
                    NativeF32Bits::from_bits(
                        input["type"]["probability_bits"].as_u64().unwrap() as u32
                    ),
                    input["next_raw"].as_i64().unwrap() as u32
                ),
                row["started"].as_bool().unwrap(),
                "{input}"
            );
        }
    }

    #[test]
    fn animation_constructor_matches_original_frame_anchored_timer() {
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/tibtre.json",
        ))
        .unwrap();
        for row in native["instance_constructor_controls"].as_array().unwrap() {
            let state = TerrainAnimationState::new(
                StringInterner::default().intern("TIBTRE01"),
                NativeF32Bits::POSITIVE_ZERO,
                0,
                22,
                row["frame"].as_i64().unwrap() as u32,
            );
            assert_eq!(
                i64::from(state.current_frame),
                row["stage"].as_i64().unwrap()
            );
            assert_eq!(i64::from(state.rate), row["rate"].as_i64().unwrap());
            assert_eq!(
                i64::from(state.timer.start_frame()),
                row["timer"][0].as_i64().unwrap()
            );
            assert_eq!(
                i64::from(state.timer.duration()),
                row["timer"][2].as_i64().unwrap()
            );
        }
    }

    const STOCK_FRAME_COUNT: u16 = 22;
    const STOCK_RATE: i32 = 3;
    const SPAWN_DENSITY_LEVELS: u8 = 3;

    fn resolved_cell() -> ResolvedTerrainCell {
        ResolvedTerrainCell {
            allows_tiberium: true,
            base_terrain_class: TerrainClass::Clear,
            ..crate::map::resolved_terrain::test_flat_cell(0, 0)
        }
    }

    fn resolved_grid(width: u16, height: u16) -> ResolvedTerrainGrid {
        let template = resolved_cell();
        let mut cells = Vec::with_capacity(width as usize * height as usize);
        for ry in 0..height {
            for rx in 0..width {
                let mut cell = template.clone();
                cell.rx = rx;
                cell.ry = ry;
                cells.push(cell);
            }
        }
        ResolvedTerrainGrid::from_cells(width, height, cells)
    }

    fn spawner(
        interner: &mut StringInterner,
        name: &str,
        prob_micros: u32,
    ) -> TerrainAnimationState {
        TerrainAnimationState::new(
            interner.intern(name),
            NativeF32Bits::from_bits((prob_micros as f32 / 1_000_000.0).to_bits()),
            STOCK_RATE,
            STOCK_FRAME_COUNT,
            0,
        )
    }

    /// The full `CellClass::PlaceTiberium` context, so spawner tests observe the
    /// overlay cells a live spawner writes.
    struct SpawnWorld {
        binary_frame: u32,
        registry: OverlayTypeRegistry,
        rules: RuleSet,
        overlay_grid: OverlayGrid,
        growth_state: OreGrowthState,
        terrain: ResolvedTerrainGrid,
        interner: StringInterner,
        entities: EntityStore,
        occupancy: OccupancyGrid,
        terrain_object_cells: BTreeMap<(u16, u16), u64>,
    }

    impl SpawnWorld {
        fn new() -> Self {
            let rules = RuleSet::from_ini(&IniFile::from_str(
                "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
                 [Tiberiums]\n0=Riparius\n\
                 [Riparius]\nImage=1\nGrowth=2200\nGrowthPercentage=.06\n\
                 Spread=2200\nSpreadPercentage=.06\n",
            ))
            .expect("spawn world rules");
            let mut growth_state = OreGrowthState::new(32, 32);
            growth_state.reset_native_tiberium_classes(rules.tiberium_types.len(), 0);
            Self {
                binary_frame: 0,
                registry: registry_with_tib_variants(),
                rules,
                overlay_grid: OverlayGrid::new(32, 32),
                growth_state,
                terrain: resolved_grid(32, 32),
                interner: StringInterner::default(),
                entities: EntityStore::new(),
                occupancy: OccupancyGrid::new(),
                terrain_object_cells: BTreeMap::new(),
            }
        }

        fn tick(
            &mut self,
            spawners: &mut BTreeMap<(u16, u16), TerrainAnimationState>,
            rng: &mut SimRng,
        ) {
            tick_terrain_animations_stateful(
                spawners,
                TerrainSpawnContext::new(Some(&mut self.overlay_grid), rng)
                    .with_growth_queue(&mut self.growth_state, self.binary_frame, true, true)
                    .with_live_object_context(
                        &self.entities,
                        &self.occupancy,
                        &self.rules,
                        &self.interner,
                        &self.terrain_object_cells,
                    )
                    .with_validation_context(Some(&self.terrain), Some(&self.registry)),
            );
            self.binary_frame += 1;
        }

        /// Every occupied overlay cell as `(cell, overlay_data)`.
        fn placed(&self) -> Vec<((u16, u16), u8)> {
            self.overlay_grid
                .iter_occupied()
                .map(|(rx, ry, cell)| ((rx, ry), cell.overlay_data))
                .collect()
        }
    }

    fn registry_with_tib_variants() -> OverlayTypeRegistry {
        let mut ini_text = String::from("[OverlayTypes]\n");
        for i in 1..=12 {
            ini_text.push_str(&format!("{}=TIB{:02}\n", i - 1, i));
        }
        for i in 1..=12 {
            ini_text.push_str(&format!("[TIB{:02}]\nTiberium=yes\n", i));
        }
        let ini = IniFile::from_str(&ini_text);
        OverlayTypeRegistry::from_ini(&ini, None)
    }

    #[test]
    fn resolved_cell_gate_requires_flat_buildable_allow_tiberium_non_bridge() {
        let cell = resolved_cell();
        assert!(resolved_cell_accepts_tiberium(&cell));

        let mut no_allow = cell.clone();
        no_allow.allows_tiberium = false;
        assert!(!resolved_cell_accepts_tiberium(&no_allow));

        let mut sloped = cell.clone();
        sloped.slope_type = 1;
        assert!(!resolved_cell_accepts_tiberium(&sloped));

        let mut blocked = cell.clone();
        blocked.base_build_blocked = true;
        assert!(!resolved_cell_accepts_tiberium(&blocked));

        for (raw_flags, accepts) in [
            (0, true),
            (BRIDGE_FLAG_STRUCTURAL, false),
            (BRIDGE_FLAG_DESTROYED_OR_RAMP, false),
            (
                BRIDGE_FLAG_STRUCTURAL | BRIDGE_FLAG_DESTROYED_OR_RAMP,
                false,
            ),
            (BRIDGE_FLAG_TRANSITION, true),
            (0x0004_0000, true),
        ] {
            let mut flagged = cell.clone();
            flagged.bridge_facts.raw_flags = raw_flags;
            assert_eq!(
                resolved_cell_accepts_tiberium(&flagged),
                accepts,
                "CellClass+0x140={raw_flags:#x}"
            );
        }

        let mut not_walkable = cell;
        not_walkable.ground_walk_blocked = true;
        assert!(
            resolved_cell_accepts_tiberium(&not_walkable),
            "the native 0x500 gate does not substitute generalized walkability"
        );
    }

    #[test]
    fn probability_hit_does_not_spawn_same_tick() {
        let mut world = SpawnWorld::new();
        let mut spawners = BTreeMap::new();
        spawners.insert(
            (10, 10),
            spawner(&mut world.interner, "TIBTRE01", 1_000_000),
        );
        let mut rng = SimRng::new(7);

        world.tick(&mut spawners, &mut rng);

        assert!(world.placed().is_empty());
        let animation = &spawners[&(10, 10)];
        assert!(animation.is_active());
        assert_eq!(animation.current_frame(), 0);
        assert_eq!(animation.timer, CdTimer::started(0, 3));
    }

    #[test]
    fn stock_rate3_spawns_33_ticks_after_probability_hit() {
        let mut world = SpawnWorld::new();
        let mut spawners = BTreeMap::new();
        spawners.insert(
            (10, 10),
            spawner(&mut world.interner, "TIBTRE01", 1_000_000),
        );
        let mut rng = SimRng::new(7);

        world.tick(&mut spawners, &mut rng);
        for _ in 0..32 {
            world.tick(&mut spawners, &mut rng);
            assert!(world.placed().is_empty());
        }

        world.tick(&mut spawners, &mut rng);
        assert_eq!(world.placed().len(), 1);
        assert!(!spawners[&(10, 10)].is_active());
        assert_eq!(spawners[&(10, 10)].current_frame(), 0);
    }

    #[test]
    fn rules_bound_raw_asset_seeds_live_midpoint_and_spawns_after_33_ticks() {
        use crate::map::overlay::TerrainObject;
        use crate::rules::art_data::ArtRegistry;
        use crate::sim::overlay_grid::OverlayGrid;
        use crate::sim::pathfinding::PathGrid;
        use crate::sim::world::Simulation;

        let root = TestAssetRoot::new();
        std::fs::write(root.path().join("TIBTRE01.TEM"), shp_header(22))
            .expect("write raw 22-frame terrain SHP");
        let assets = AssetManager::from_loose_root_for_test(root.path());

        let mut rules_text = String::from(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
             [TerrainTypes]\n0=TIBTRE01\n\
             [TIBTRE01]\nSpawnsTiberium=yes\nIsAnimated=yes\n\
             AnimationRate=3\nAnimationProbability=1\n\
             [OverlayTypes]\n",
        );
        for index in 0..12 {
            rules_text.push_str(&format!("{index}=TIB{:02}\n", index + 1));
        }
        rules_text.push_str("12=TIBFALLBACK\n");
        for index in 1..=12 {
            rules_text.push_str(&format!("[TIB{index:02}]\nTiberium=yes\n"));
        }
        rules_text.push_str(
            "[TIBFALLBACK]\nTiberium=yes\n\
             [Tiberiums]\n0=Riparius\n\
             [Riparius]\nImage=1\n",
        );
        let rules_ini = IniFile::from_str(&rules_text);
        let mut rules = RuleSet::from_ini(&rules_ini).expect("terrain-spawner rules");
        let art = ArtRegistry::from_ini(&IniFile::from_str("[TIBTRE01]\nTheater=yes\n"));
        // Synthetic fixture supplies ART directly, without native read-admission replay.
        rules.install_art_fixture(art);
        rules.bind_terrain_spawner_assets(&rules_ini, &assets, "TEM", "TEMPERATE");

        let registry = OverlayTypeRegistry::from_ini(&rules_ini, None);
        let mut sim = Simulation::with_seed(7);
        sim.resolved_terrain = Some(resolved_grid(32, 32));
        sim.overlay_grid = Some(OverlayGrid::new(32, 32));
        construct_terrain_objects(
            &mut sim,
            &[TerrainObject {
                rx: 10,
                ry: 10,
                name: "TIBTRE01".to_string(),
            }],
            &rules,
            false,
        );
        assert_eq!(seed_terrain_spawner_animation(&mut sim, &rules), 1);
        let state = &sim.production.terrain_animations[&(10, 10)];
        assert_eq!(state.frame_count, 22);
        assert_eq!(state.frame_count / 2, 11);

        let path_grid = PathGrid::test_all_passable(32, 32);
        let advance = |sim: &mut Simulation| {
            sim.advance_tick(&[], Some(&rules), Some(&path_grid), Some(&registry), 67)
        };

        assert!(advance(&mut sim).frame_committed);
        assert_eq!(
            sim.overlay_grid
                .as_ref()
                .expect("overlay grid")
                .iter_occupied()
                .count(),
            0
        );
        for _ in 0..32 {
            assert!(advance(&mut sim).frame_committed);
            assert_eq!(
                sim.overlay_grid
                    .as_ref()
                    .expect("overlay grid")
                    .iter_occupied()
                    .count(),
                0
            );
        }
        assert!(advance(&mut sim).frame_committed);
        assert!(!sim.production.terrain_animations[&(10, 10)].is_active());
        let placed_cells: Vec<(u8, u8)> = sim
            .overlay_grid
            .as_ref()
            .expect("overlay grid")
            .iter_occupied()
            .map(|(_, _, cell)| {
                (
                    cell.overlay_id.expect("occupied identity"),
                    cell.overlay_data,
                )
            })
            .collect();
        assert_eq!(placed_cells.len(), 1);
        assert_eq!(placed_cells[0].1, SPAWN_DENSITY_LEVELS as u8);
        assert!(placed_cells[0].0 < 12, "a registry TIB01..TIB12 variant");
    }

    #[test]
    fn active_animation_suppresses_probability_rolls() {
        let mut world = SpawnWorld::new();
        let mut spawners = BTreeMap::new();
        let mut state = spawner(&mut world.interner, "TIBTRE01", 1_000_000);
        state.rate = STOCK_RATE;
        state.timer = CdTimer::started(0, STOCK_RATE);
        spawners.insert((10, 10), state);
        let mut rng = SimRng::new(123);
        let before = rng.state();

        world.tick(&mut spawners, &mut rng);

        assert_eq!(
            rng.state(),
            before,
            "active non-midpoint tick consumes no RNG"
        );
        assert!(world.placed().is_empty());
    }

    #[test]
    fn zero_probability_still_consumes_idle_draws() {
        let mut world = SpawnWorld::new();
        let mut spawners = BTreeMap::new();
        spawners.insert((10, 10), spawner(&mut world.interner, "TIBTRE_NEVER", 0));
        let mut rng = SimRng::new(7);
        let mut native_rng = rng.clone();

        for _ in 0..1000 {
            world.tick(&mut spawners, &mut rng);
            native_rng.next_u32();
        }
        assert_eq!(
            rng.state(),
            native_rng.state(),
            "Terrain AI71C745 rolls even at zero probability"
        );
        assert!(world.placed().is_empty());
        assert!(!spawners[&(10, 10)].is_active());
        assert_eq!(spawners[&(10, 10)].current_frame(), 0);
    }

    #[test]
    fn spawn_on_empty_cell_creates_density_3_ore() {
        let mut world = SpawnWorld::new();
        let mut spawners = BTreeMap::new();
        spawners.insert(
            (10, 10),
            spawner(&mut world.interner, "TIBTRE01", 1_000_000),
        );
        let mut rng = SimRng::new(7);

        for _ in 0..34 {
            world.tick(&mut spawners, &mut rng);
        }

        let placed = world.placed();
        assert_eq!(placed.len(), 1);
        assert_eq!(placed[0].1, SPAWN_DENSITY_LEVELS as u8);
    }

    #[test]
    fn spawn_skips_existing_ore_neighbors_instead_of_growing_them() {
        let mut world = SpawnWorld::new();
        let mut spawners = BTreeMap::new();
        spawners.insert(
            (10, 10),
            spawner(&mut world.interner, "TIBTRE01", 1_000_000),
        );
        for &(dx, dy) in &ADJACENT_OFFSETS {
            if (dx, dy) == (1, 1) {
                continue;
            }
            world
                .overlay_grid
                .place_overlay((10 + dx) as u16, (10 + dy) as u16, 0, 1);
        }
        let mut rng = SimRng::new(7);

        for _ in 0..34 {
            world.tick(&mut spawners, &mut rng);
        }

        assert_eq!(
            world.overlay_grid.cell(11, 11).overlay_data,
            SPAWN_DENSITY_LEVELS as u8
        );
        let grown_existing = world
            .placed()
            .iter()
            .filter(|(cell, data)| *cell != (11, 11) && *data != 1)
            .count();
        assert_eq!(grown_existing, 0, "existing ore must not be additive-grown");
    }

    #[test]
    fn spawn_places_nothing_when_all_neighbors_have_overlays() {
        let mut world = SpawnWorld::new();
        let mut spawners = BTreeMap::new();
        spawners.insert(
            (10, 10),
            spawner(&mut world.interner, "TIBTRE01", 1_000_000),
        );
        for &(dx, dy) in &ADJACENT_OFFSETS {
            world
                .overlay_grid
                .place_overlay((10 + dx) as u16, (10 + dy) as u16, 5, 0);
        }
        let mut rng = SimRng::new(7);

        for _ in 0..34 {
            world.tick(&mut spawners, &mut rng);
        }

        assert_eq!(world.placed().len(), 8);
        assert!(world.placed().iter().all(|(_, data)| *data == 0));
    }

    #[test]
    fn live_building_gate_rejects_visible_and_allows_invisible_exceptions() {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
             [BuildingTypes]\n0=GAPOWR\n1=BRIDGEA\n2=BRIDGEB\n\
             [GAPOWR]\nStrength=100\n\
             [BRIDGEA]\nStrength=100\nInvisible=yes\n\
             [BRIDGEB]\nStrength=100\nInvisibleInGame=yes\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("rules");
        let overlay_grid = OverlayGrid::new(32, 32);
        let terrain = resolved_grid(32, 32);
        let spawner_cells = BTreeSet::new();

        fn context_for<'a>(
            type_name: &str,
            rules: &'a RuleSet,
            interner: &'a mut StringInterner,
            entities: &'a mut EntityStore,
            occupancy: &'a mut OccupancyGrid,
            terrain_object_cells: &'a BTreeMap<(u16, u16), u64>,
        ) -> TiberiumPlacementObjectContext<'a> {
            let mut entity = GameEntity::test_default_of_category(
                1,
                type_name,
                "Neutral",
                11,
                10,
                EntityCategory::Structure,
            );
            entity.type_ref = interner.intern(type_name);
            entities.insert(entity);
            occupancy.add(
                11,
                10,
                1,
                MovementLayer::Ground,
                None,
                CellListInsertion::AppendBuilding,
            );
            TiberiumPlacementObjectContext::new(
                entities,
                occupancy,
                rules,
                interner,
                terrain_object_cells,
            )
        }

        for (type_name, expected) in [("GAPOWR", false), ("BRIDGEA", true), ("BRIDGEB", true)] {
            let mut interner = StringInterner::default();
            let mut entities = EntityStore::new();
            let mut occupancy = OccupancyGrid::new();
            let terrain_object_cells = BTreeMap::new();
            let context = context_for(
                type_name,
                &rules,
                &mut interner,
                &mut entities,
                &mut occupancy,
                &terrain_object_cells,
            );
            let admission = NewTiberiumAdmission::runtime(&terrain, context);

            assert_eq!(
                admit_new_tiberium_target(&overlay_grid, &spawner_cells, admission, (11, 10))
                    .is_some(),
                expected,
                "{type_name}"
            );
        }
    }

    #[test]
    fn spawning_terrain_cells_reject_tiberium_even_when_not_animated() {
        let overlay_grid = OverlayGrid::new(32, 32);
        let mut spawning_terrain_cells = BTreeSet::new();
        spawning_terrain_cells.insert((12, 10));
        let terrain = resolved_grid(32, 32);
        let no_objects = crate::sim::tiberium::test_support::NoLiveObjects::new();
        let admission = NewTiberiumAdmission::runtime(&terrain, no_objects.context());

        assert!(
            admit_new_tiberium_target(&overlay_grid, &spawning_terrain_cells, admission, (13, 10))
                .is_some()
        );
        assert!(
            admit_new_tiberium_target(&overlay_grid, &spawning_terrain_cells, admission, (12, 10))
                .is_none()
        );
    }

    #[test]
    fn deterministic_same_seed_same_pattern() {
        fn run(seed: u64) -> Vec<((u16, u16), u8)> {
            let mut world = SpawnWorld::new();
            let mut spawners = BTreeMap::new();
            spawners.insert(
                (10, 10),
                spawner(&mut world.interner, "TIBTRE_HALF", 500_000),
            );
            let mut rng = SimRng::new(seed);
            for _ in 0..200 {
                world.tick(&mut spawners, &mut rng);
            }
            world.placed()
        }

        let a = run(42);
        let b = run(42);
        assert_eq!(a, b, "same seed must produce identical state");
    }

    #[test]
    fn seed_all_animated_types_and_keep_native_probability_and_rate() {
        use crate::map::overlay::TerrainObject;
        use crate::rules::ini_parser::IniFile;
        use crate::rules::ruleset::RuleSet;
        use crate::sim::world::Simulation;

        let ini = IniFile::from_str(
            "[InfantryTypes]\n\
             [VehicleTypes]\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [TerrainTypes]\n1=TIBTRE01\n2=TREE01\n3=TREE02\n\
             [TIBTRE01]\nSpawnsTiberium=yes\nIsAnimated=yes\n\
             AnimationRate=3\nAnimationProbability=.003\n\
             [TREE01]\nSpawnsTiberium=no\nIsAnimated=yes\n\
             [TREE02]\nSpawnsTiberium=yes\nIsAnimated=no\n",
        );
        let mut rules = RuleSet::from_ini(&ini).expect("rules");
        rules.set_terrain_spawner_frame_count_for_test("TIBTRE01", STOCK_FRAME_COUNT);
        let mut sim = Simulation::new();
        let objs = vec![
            TerrainObject {
                rx: 5,
                ry: 6,
                name: "TIBTRE01".to_string(),
            },
            TerrainObject {
                rx: 8,
                ry: 9,
                name: "TREE01".to_string(),
            },
            TerrainObject {
                rx: 1,
                ry: 2,
                name: "TREE02".to_string(),
            },
            TerrainObject {
                rx: 3,
                ry: 4,
                name: "UNKNOWN".to_string(),
            },
        ];
        let seeded = seed_terrain_animations(&mut sim, &objs, &rules, false);
        assert_eq!(seeded, 2);
        let placed = sim
            .production
            .terrain_animations
            .get(&(5, 6))
            .expect("TIBTRE01 seeded at (5,6)");
        assert_eq!(placed.animation_probability.bits(), 0x3b44_9ba6);
        assert_eq!(placed.animation_rate_ticks, 3);
        // Rendering addresses 11 body frames in the 22-frame SHP. Native
        // TerrainClass::AI reads raw 22 and performs the one midpoint divide
        // itself, so the authoritative target must remain 11 rather than 5.
        assert_eq!(placed.frame_count, STOCK_FRAME_COUNT as i16);
        assert_eq!(placed.frame_count / 2, STOCK_FRAME_COUNT as i16 / 2);
        assert_eq!(
            sim.production.tiberium_spawning_terrain_cells,
            BTreeSet::from([(5, 6), (1, 2)])
        );
    }

    /// A `[Terrain]` entry read straight out of a map INI must become a live
    /// object that a player can force-fire, damage and destroy — the whole
    /// chain from map section to `TerrainClass::Take_Damage`.
    #[test]
    fn gsi_17_01_map_terrain_entry_becomes_a_force_fireable_object() {
        use crate::map::overlay::parse_terrain_objects;
        use crate::sim::command::{Command, CommandEnvelope};
        use crate::sim::components::Health;
        use crate::sim::game_entity::GameEntity;
        use crate::sim::pathfinding::PathGrid;
        use crate::sim::terrain_object::TerrainObjectLifecycle;
        use crate::sim::world::Simulation;

        // Stock-shaped rules: TREE01 declares no Strength, so it resolves
        // through `[General] TreeStrength`, and no Immune/LegalTarget keys —
        // exactly as `ini/rulesmd.ini` writes it. `Wood=yes` on the warhead is
        // what lets a shot reach a terrain object at all.
        let rules_ini = IniFile::from_str(
            "[General]\nTreeStrength=200\n\
             [InfantryTypes]\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [VehicleTypes]\n0=MTNK\n\
             [TerrainTypes]\n0=TREE01\n\
             [TREE01]\nName=Tree\nTemperateOccupationBits=4\nSnowOccupationBits=6\n\
             [MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=105mm\n\
             [105mm]\nDamage=65\nROF=50\nRange=6\nWarhead=AP\n\
             [AP]\nWood=yes\n\
             Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n",
        );
        let rules = RuleSet::from_ini(&rules_ini).expect("rules");

        // Real map syntax: the `[Terrain]` key is `ry * 1000 + rx`.
        let map_ini = IniFile::from_str("[Terrain]\n5010=TREE01\n");
        let terrain_objects = parse_terrain_objects(&map_ini);
        assert_eq!(terrain_objects.len(), 1);
        assert_eq!((terrain_objects[0].rx, terrain_objects[0].ry), (10, 5));

        let mut sim = Simulation::new();
        let attacker_id = sim.allocate_stable_id();
        let mut attacker = GameEntity::test_default(attacker_id, "MTNK", "Americans", 5, 5);
        attacker.health = Health { current: 300 };
        let owner_id = attacker.owner;
        // `test_default` interns both owner and type through the shared test
        // interner. Snapshot it only after constructing the entity so those
        // handles resolve through the Simulation that will execute the order.
        sim.interner = crate::sim::intern::test_interner();

        sim.input_delay_ticks = 0;
        sim.resolved_terrain = Some(resolved_grid(64, 64));
        sim.substrate.entities.insert(attacker);
        assert!(matches!(
            sim.reveal(attacker_id),
            crate::sim::world::RevealOutcome::Revealed { .. }
        ));

        let constructed = construct_terrain_objects(&mut sim, &terrain_objects, &rules, false);
        assert_eq!(
            constructed, 1,
            "the [Terrain] entry constructs a live object"
        );
        let stable_id = sim.production.terrain_object_cells[&(10, 5)];
        assert_eq!(sim.production.terrain_objects[&stable_id].health, 200);
        assert_eq!(sim.production.terrain_occupation_bits[&(10, 5)], 4);

        let grid = PathGrid::test_all_passable(64, 64);
        sim.queue_command(CommandEnvelope::new(
            owner_id,
            sim.session.tick + 1,
            Command::ForceAttackCell {
                attacker_id,
                target_rx: 10,
                target_ry: 5,
            },
        ));

        let mut damaged = false;
        let mut destroyed = false;
        let mut shots = 0usize;
        let mut targeted = false;
        let mut last_health = 200;
        for _ in 0..600 {
            let pending = sim.take_due_commands();
            sim.advance_tick(&pending, Some(&rules), Some(&grid), None, 100);
            shots += sim.fire_events.len();
            targeted |= sim
                .substrate
                .entities
                .get(attacker_id)
                .is_some_and(|e| e.attack_target.is_some());
            match sim.production.terrain_objects.get(&stable_id) {
                Some(terrain) => {
                    last_health = terrain.health;
                    damaged |= terrain.health < 200;
                    if terrain.lifecycle == TerrainObjectLifecycle::Destroyed {
                        destroyed = true;
                        break;
                    }
                }
                None => {
                    // Terminal Terrain follows ObjectClass UnInit: it remains
                    // resolvable until the common post-frame delete drain, then
                    // its physical record is finalized in this same advance.
                    destroyed = true;
                    break;
                }
            }
        }

        assert!(
            damaged,
            "force-fire on the tree cell must damage the tree \
             (targeted={targeted}, shots={shots}, health={})",
            last_health
        );
        assert!(destroyed, "sustained force-fire must destroy the tree");
        assert!(
            !sim.production.terrain_object_cells.contains_key(&(10, 5)),
            "a destroyed tree releases its cell"
        );
        assert!(
            !sim.production
                .terrain_occupation_bits
                .contains_key(&(10, 5)),
            "a destroyed tree releases its occupation bits"
        );
    }

    /// The animation index is a decoration over already-constructed objects, so
    /// running it alone can never resurrect terrain.
    #[test]
    fn gsi_17_01_spawner_animation_pass_only_decorates_constructed_objects() {
        use crate::map::overlay::TerrainObject;
        use crate::sim::world::Simulation;

        let ini = IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
             [TerrainTypes]\n0=TIBTRE01\n\
             [TIBTRE01]\nSpawnsTiberium=yes\nIsAnimated=yes\n\
             AnimationRate=3\nAnimationProbability=.003\n",
        );
        let mut rules = RuleSet::from_ini(&ini).expect("rules");
        rules.set_terrain_spawner_frame_count_for_test("TIBTRE01", STOCK_FRAME_COUNT);

        let mut sim = Simulation::new();
        assert_eq!(
            seed_terrain_spawner_animation(&mut sim, &rules),
            0,
            "no constructed objects means no spawners"
        );
        assert!(sim.production.terrain_objects.is_empty());

        construct_terrain_objects(
            &mut sim,
            &[TerrainObject {
                rx: 5,
                ry: 6,
                name: "TIBTRE01".to_string(),
            }],
            &rules,
            false,
        );
        assert!(
            sim.production.terrain_animations.is_empty(),
            "construction alone leaves the animation index empty"
        );
        assert_eq!(seed_terrain_spawner_animation(&mut sim, &rules), 1);
        assert_eq!(
            sim.production.terrain_animations[&(5, 6)].frame_count,
            STOCK_FRAME_COUNT as i16
        );
    }

    #[test]
    fn gsi_04_12_terrain_raw_occupation_seed_maps_theater_masks_at_source_cell() {
        use crate::map::overlay::TerrainObject;
        use crate::sim::terrain_object::TerrainObjectLifecycle;
        use crate::sim::world::Simulation;

        let ini = IniFile::from_str(
            "[InfantryTypes]\n\
             [VehicleTypes]\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [TerrainTypes]\n0=TERR0\n1=TERR1\n2=TERR2\n3=TERR4\n4=TERR7\n\
             [TERR0]\nTemperateOccupationBits=0\nSnowOccupationBits=7\n\
             [TERR1]\nTemperateOccupationBits=1\nSnowOccupationBits=4\n\
             [TERR2]\nTemperateOccupationBits=2\nSnowOccupationBits=2\n\
             [TERR4]\nTemperateOccupationBits=4\nSnowOccupationBits=1\n\
             [TERR7]\nTemperateOccupationBits=7\nSnowOccupationBits=0\n",
        );
        let mut rules = RuleSet::from_ini(&ini).expect("terrain raw rules");
        rules
            .terrain_object_types
            .get_mut("TERR7")
            .expect("TERR7")
            .merge_art_foundation("2x2");
        let objects = [
            TerrainObject {
                rx: 2,
                ry: 2,
                name: "TERR0".to_string(),
            },
            TerrainObject {
                rx: 6,
                ry: 2,
                name: "TERR1".to_string(),
            },
            TerrainObject {
                rx: 10,
                ry: 2,
                name: "TERR2".to_string(),
            },
            TerrainObject {
                rx: 14,
                ry: 2,
                name: "TERR4".to_string(),
            },
            TerrainObject {
                rx: 18,
                ry: 2,
                name: "TERR7".to_string(),
            },
        ];
        let source_masks = [[0u8, 1, 2, 4, 7], [7u8, 4, 2, 1, 0]];

        for (snow_theater, selected_masks) in [(false, source_masks[0]), (true, source_masks[1])] {
            let mut sim = Simulation::new();
            for object in &objects {
                sim.substrate
                    .raw_cell_occupation
                    .mark_deck(object.rx, object.ry, 0x5A);
            }

            let seeded = seed_terrain_animations(&mut sim, &objects, &rules, snow_theater);

            assert_eq!(seeded, 0, "all fixtures are recognized non-spawners");
            assert!(sim.production.terrain_animations.is_empty());
            assert_eq!(sim.production.terrain_objects.len(), objects.len());
            for (object, source_mask) in objects.iter().zip(selected_masks) {
                let expected_raw = match source_mask {
                    0 => 0x00,
                    1 => 0x04,
                    2 => 0x08,
                    4 => 0x10,
                    7 => 0x1C,
                    other => panic!("unexpected fixture source mask {other}"),
                };
                assert_eq!(
                    sim.substrate
                        .raw_cell_occupation
                        .ground_bits(object.rx, object.ry),
                    expected_raw,
                    "snow={snow_theater} type={} source={source_mask}",
                    object.name
                );
                assert_eq!(
                    sim.substrate
                        .raw_cell_occupation
                        .deck_bits(object.rx, object.ry),
                    0x5A,
                    "terrain raw producer is ground-only"
                );
                let stable_id = sim.production.terrain_object_cells[&(object.rx, object.ry)];
                let terrain = &sim.production.terrain_objects[&stable_id];
                assert_eq!(terrain.occupation_bits, source_mask);
                assert_eq!(terrain.lifecycle, TerrainObjectLifecycle::Live);
                if source_mask != 0 {
                    assert_eq!(
                        sim.production.terrain_occupation_bits[&(object.rx, object.ry)],
                        source_mask,
                        "zone/passability authority retains the unshifted source mask"
                    );
                }
            }

            for foundation_only_cell in [(19, 2), (18, 3), (19, 3)] {
                assert_eq!(
                    sim.substrate
                        .raw_cell_occupation
                        .ground_bits(foundation_only_cell.0, foundation_only_cell.1),
                    0,
                    "2x2 TERR7 foundation must repeatedly target only its source cell"
                );
                assert_eq!(
                    sim.substrate
                        .raw_cell_occupation
                        .deck_bits(foundation_only_cell.0, foundation_only_cell.1),
                    0
                );
            }
        }
    }

    #[test]
    fn authored_terrain_retains_native_id_projects_zero_occupation_then_clears_resource() {
        use crate::map::overlay::{OverlayEntry, TerrainObject};
        use crate::sim::world::Simulation;

        let ini = IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
             [TerrainTypes]\n0=TERR0\n\
             [TERR0]\nTemperateOccupationBits=0\nSnowOccupationBits=7\n\
             [OverlayTypes]\n0=ORE\n[ORE]\nTiberium=yes\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("authored Terrain rules");
        let overlays = OverlayTypeRegistry::from_ini(&ini, None);
        let mut sim = Simulation::new();
        sim.resolved_terrain = Some(resolved_grid(4, 4));
        sim.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::from_overlay_entries(
            &[OverlayEntry {
                rx: 2,
                ry: 2,
                overlay_id: 0,
                frame: 7,
            }],
            4,
            4,
        ));
        sim.native_unique_ids = Some(
            crate::sim::native_identity::build_noncampaign_fresh_id_prefix(0, 0, 0, 0, 0, 0, 1, 1)
                .into_cursor(),
        );
        let before = sim
            .native_unique_ids
            .as_ref()
            .expect("native cursor")
            .current_raw();

        let constructed = construct_authored_terrain_objects(
            &mut sim,
            &[TerrainObject {
                rx: 2,
                ry: 2,
                name: "TERR0".to_string(),
            }],
            &rules,
            false,
            &overlays,
        )
        .expect("authored Terrain construction");

        assert_eq!(constructed, 1);
        let stable_id = sim.production.terrain_object_cells[&(2, 2)];
        assert_eq!(
            sim.production.terrain_objects[&stable_id].native_unique_id,
            Some(before.wrapping_add(1) as i32)
        );
        assert_eq!(
            sim.resolved_terrain
                .as_ref()
                .unwrap()
                .cell(2, 2)
                .unwrap()
                .terrain_object_occupation,
            Some(0),
            "zero remains a present Terrain receiver for native zone classification"
        );
        assert_eq!(
            sim.overlay_grid.as_ref().unwrap().cell(2, 2).overlay_id,
            None,
            "same-cell resource clearing occurs after the immediate Terrain Recalc"
        );
    }

    fn shp_header(frame_count: u16) -> Vec<u8> {
        let mut data = vec![0_u8; 8 + usize::from(frame_count) * 24];
        data[6..8].copy_from_slice(&frame_count.to_le_bytes());
        data
    }

    static NEXT_ASSET_ROOT: AtomicU64 = AtomicU64::new(0);

    struct TestAssetRoot(PathBuf);

    impl TestAssetRoot {
        fn new() -> Self {
            let serial = NEXT_ASSET_ROOT.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "vera20k-terrain-spawner-live-{}-{serial}",
                std::process::id()
            ));
            std::fs::create_dir(&path).expect("create terrain spawner test root");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestAssetRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
