//! TIBTRE-style terrain object ore spawning.
//!
//! Per-object retained animation state for terrain objects with `IsAnimated=yes`.
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
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

use std::collections::{BTreeMap, BTreeSet};

use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::entity_store::EntityStore;
use crate::sim::intern::{InternedId, StringInterner};
use crate::sim::occupancy::{OccupancyGrid, RawCellOccupationGrid};
use crate::sim::ore_growth::OreGrowthState;
use crate::sim::overlay_grid::OverlayGrid;
use crate::sim::rng::SimRng;
use crate::sim::scenario_session::ScenarioSession;
use crate::sim::terrain_object::TerrainObjectStore;
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
/// Terrain removal/limbo removes only that instance's animation.
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

    pub(crate) fn type_ref(&self) -> InternedId {
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

    #[cfg(any(test, feature = "test-support"))]
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
    pub dirty_cells: Option<&'a mut crate::sim::dirty_cells::DirtyCells>,
    pub binary_frame: u32,
    pub spawning_terrain_cells: Option<&'a BTreeSet<(u16, u16)>>,
    pub entities: Option<&'a EntityStore>,
    pub occupancy: Option<&'a OccupancyGrid>,
    /// Live raw occupation planes, which a new ore Overlay's Mark tests.
    pub(crate) raw_occupation: Option<&'a RawCellOccupationGrid>,
    pub rules: &'a RuleSet,
    pub interner: Option<&'a StringInterner>,
    /// Terrain objects (trees, rocks) indexed by cell — the non-Techno half of the
    /// native `Cell+0xE4` FirstObject list that `CanSpreadTiberium` reads.
    pub terrain_object_cells: Option<&'a BTreeMap<(u16, u16), u64>>,
    pub rng: &'a mut SimRng,
}

impl<'a> TerrainSpawnContext<'a> {
    pub fn new(
        overlay_grid: Option<&'a mut OverlayGrid>,
        rules: &'a RuleSet,
        rng: &'a mut SimRng,
    ) -> Self {
        Self {
            overlay_grid,
            resolved_terrain: None,
            overlay_registry: None,
            ore_growth_state: None,
            growth_enabled: true,
            spread_enabled: true,
            dirty_cells: None,
            binary_frame: 0,
            spawning_terrain_cells: None,
            entities: None,
            occupancy: None,
            raw_occupation: None,
            rules,
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
        dirty_cells: &'a mut crate::sim::dirty_cells::DirtyCells,
    ) -> Self {
        self.dirty_cells = Some(dirty_cells);
        self
    }

    pub fn with_spawning_terrain_cells(mut self, cells: &'a BTreeSet<(u16, u16)>) -> Self {
        self.spawning_terrain_cells = Some(cells);
        self
    }

    pub(crate) fn with_live_object_context(
        mut self,
        entities: &'a EntityStore,
        occupancy: &'a OccupancyGrid,
        raw_occupation: &'a RawCellOccupationGrid,
        interner: &'a StringInterner,
        terrain_object_cells: &'a BTreeMap<(u16, u16), u64>,
    ) -> Self {
        self.entities = Some(entities);
        self.occupancy = Some(occupancy);
        self.raw_occupation = Some(raw_occupation);
        self.interner = Some(interner);
        self.terrain_object_cells = Some(terrain_object_cells);
        self
    }
}

/// Test adapter over the same single-object animation and spread owners.
/// Production dispatch follows retained Logic order, not this cell-key order.
#[cfg(any(test, feature = "test-support"))]
pub fn tick_terrain_animations_stateful(
    spawners: &mut BTreeMap<(u16, u16), TerrainAnimationState>,
    mut ctx: TerrainSpawnContext<'_>,
) {
    if spawners.is_empty() {
        return;
    }

    let spawner_cells: BTreeSet<(u16, u16)> = spawners.keys().copied().collect();
    for &cell in &spawner_cells {
        if let Some(spawner) = spawners.get_mut(&cell) {
            tick_terrain_spawner_one_inner(spawner, cell, &spawner_cells, &mut ctx);
        }
    }
}

/// Dispatch one TerrainClass AI slot through the retained animation owner.
pub(crate) fn tick_terrain_spawner_stateful_one(
    spawner: &mut TerrainAnimationState,
    cell: (u16, u16),
    spawner_cells: &BTreeSet<(u16, u16)>,
    mut ctx: TerrainSpawnContext<'_>,
) -> Option<TiberiumPlacement> {
    tick_terrain_spawner_one_inner(spawner, cell, spawner_cells, &mut ctx)
}

fn tick_terrain_spawner_one_inner(
    spawner: &mut TerrainAnimationState,
    cell: (u16, u16),
    spawner_cells: &BTreeSet<(u16, u16)>,
    ctx: &mut TerrainSpawnContext<'_>,
) -> Option<TiberiumPlacement> {
    let source_cells = ctx.spawning_terrain_cells.unwrap_or(spawner_cells);
    let terrain_type = ctx.interner.and_then(|interner| {
        ctx.rules
            .terrain_object_type_case_insensitive(interner.resolve(spawner.type_ref))
    });
    let is_animated = terrain_type.is_none_or(|ty| ty.is_animated);
    let spawns_tiberium =
        terrain_type.map_or_else(|| source_cells.contains(&cell), |ty| ty.spawns_tiberium);
    if !spawner.tick(ctx.binary_frame, is_animated, spawns_tiberium, ctx.rng) {
        return None;
    }
    let rules = ctx.rules;
    let (Some(grid), Some(registry), Some(state), Some(terrain), Some(objects)) = (
        ctx.overlay_grid.as_deref_mut(),
        ctx.overlay_registry,
        ctx.ore_growth_state.as_deref_mut(),
        ctx.resolved_terrain,
        live_object_context(
            ctx.entities,
            ctx.occupancy,
            ctx.raw_occupation,
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
        dirty_cells: ctx.dirty_cells.as_deref_mut(),
    };
    spread_tiberium(&mut placement, cell, true)
}

/// Dispatch the TerrainClass AI leaf for one current LogicClass slot.
pub(crate) fn tick_terrain_object_ai(
    sim: &mut crate::sim::world::Simulation,
    stable_id: u64,
    rules: &crate::rules::ruleset::RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
) {
    let Some(cell) = sim
        .terrain_objects
        .objects()
        .get(&stable_id)
        .filter(|terrain| terrain.is_live())
        .map(TerrainObjectState::cell)
    else {
        return;
    };
    let Some((spawner, spawning_cells, cell_index)) =
        sim.terrain_objects.animation_with_indexes_mut(stable_id)
    else {
        return;
    };
    let placed = tick_terrain_spawner_stateful_one(
        spawner,
        cell,
        spawning_cells,
        TerrainSpawnContext::new(sim.overlay_grid.as_mut(), rules, &mut sim.scenario_rng)
            .with_growth_queue(
                &mut sim.ore_growth_state,
                sim.session.binary_frame(),
                sim.ore_growth_config.grows(),
                sim.ore_growth_config.spreads(),
            )
            .with_dirty_tracking(&mut sim.dirty_cells)
            .with_spawning_terrain_cells(spawning_cells)
            .with_live_object_context(
                &sim.substrate.entities,
                &sim.substrate.occupancy,
                &sim.substrate.raw_cell_occupation,
                &sim.interner,
                cell_index,
            )
            .with_validation_context(sim.resolved_terrain.as_ref(), overlay_registry),
    );
    // Overlay Mark5FC570 ->Cell Recalc47D2B0 is synchronous: a later
    // Logic object (including a miner) sees the new LandType this frame.
    let placed_cell = placed.as_ref().map(TiberiumPlacement::cell);
    if let (Some(cell), Some(registry)) = (placed_cell, overlay_registry) {
        sim.publish_tiberium_cells(rules, registry, &[cell]);
    }
    crate::sim::world::Simulation::publish_overlay_constructions(
        &mut sim.native_unique_ids,
        &mut sim.load_objects,
        &mut sim.substrate,
        placed.and_then(TiberiumPlacement::into_overlay_construction),
    );
}

fn live_object_context<'a>(
    entities: Option<&'a EntityStore>,
    occupancy: Option<&'a OccupancyGrid>,
    raw_occupation: Option<&'a RawCellOccupationGrid>,
    rules: &'a RuleSet,
    interner: Option<&'a StringInterner>,
    terrain_object_cells: Option<&'a BTreeMap<(u16, u16), u64>>,
) -> Option<TiberiumPlacementObjectContext<'a>> {
    Some(TiberiumPlacementObjectContext::new(
        entities?,
        occupancy?,
        raw_occupation?,
        rules,
        interner?,
        terrain_object_cells?,
    ))
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
        .terrain_objects
        .objects()
        .keys()
        .copied()
        .collect::<Vec<_>>();
    for stable_id in old_terrain_ids {
        sim.unregister_non_entity_object(stable_id);
    }
    sim.terrain_objects.clear();

    let mut constructed = 0usize;
    for obj in terrain_objects {
        let Some(t) = rules.terrain_object_type_case_insensitive(&obj.name) else {
            continue;
        };
        let type_ref = sim.interner.intern(&obj.name);
        // TerrainClass construction reaches AbstractClass::Create_ID
        // @ 0x00410230, which draws from ScenarioClass::NextUniqueID
        // @ 0x0068BCB0 just like every other modeled runtime object.
        let stable_id = sim.substrate.allocate_stable_id();
        let native_unique_id = authored_overlay_registry
            .map(|_| sim.next_native_load_id())
            .transpose()?;
        let terrain_state = TerrainObjectState::new(
            stable_id,
            type_ref,
            (obj.rx, obj.ry),
            t,
            snow_theater,
            sim.resolved_terrain.as_ref(),
        )
        .with_native_unique_id(native_unique_id);
        let occupation_bits = terrain_state.occupation_bits();
        sim.terrain_objects
            .insert_constructed(terrain_state, t.spawns_tiberium);
        let registered = sim.register_terrain_object(stable_id, rules);
        debug_assert!(registered);
        mark_terrain_raw_occupation(
            &mut sim.substrate.raw_cell_occupation,
            (obj.rx, obj.ry),
            occupation_bits,
        );
        if let Some(overlay_registry) = authored_overlay_registry {
            let terrain_snapshot = sim
                .terrain_objects
                .objects()
                .get(&stable_id)
                .expect("new Terrain remains registered")
                .clone();
            crate::sim::terrain_object::mark_terrain_occupation(
                &terrain_snapshot,
                sim.resolved_terrain.as_mut(),
            );
            if let Some(grid) = sim.overlay_grid.as_mut() {
                let source = grid.cell(obj.rx, obj.ry);
                let before = (source.overlay_id.map_or(-1, i32::from), source.overlay_data);
                let after = crate::map::tiberium_cell::terrain_unlimbo_overlay_fields(
                    before.0,
                    before.1,
                    |id| {
                        overlay_registry
                            .flags(id as u8)
                            .is_some_and(|flags| flags.tiberium)
                    },
                );
                if after != before {
                    grid.clear_cell_silently(obj.rx, obj.ry);
                }
            }
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
    interner: &StringInterner,
    terrain_objects: &mut TerrainObjectStore,
    session: &ScenarioSession,
    rules: &crate::rules::ruleset::RuleSet,
) -> usize {
    terrain_objects.clear_animations();

    let candidates: Vec<(u64, InternedId)> = terrain_objects
        .objects()
        .values()
        .filter(|terrain| terrain.is_live())
        .map(|terrain| (terrain.stable_id(), terrain.type_ref()))
        .collect();

    let mut seeded = 0usize;
    for (stable_id, type_ref) in candidates {
        let name = interner.resolve(type_ref).to_string();
        let Some(t) = rules.terrain_object_type_case_insensitive(&name) else {
            continue;
        };
        if !t.is_animated {
            continue;
        }
        let frame_count = rules.terrain_spawner_frame_count(&name).unwrap_or(0);
        terrain_objects.seed_animation(
            stable_id,
            TerrainAnimationState::new(
                type_ref,
                t.animation_probability,
                t.animation_rate,
                frame_count,
                session.binary_frame(),
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
#[cfg(any(test, feature = "test-support"))]
pub fn seed_terrain_animations(
    sim: &mut crate::sim::world::Simulation,
    terrain_objects: &[crate::map::overlay::TerrainObject],
    rules: &crate::rules::ruleset::RuleSet,
    snow_theater: bool,
) -> usize {
    construct_terrain_objects(sim, terrain_objects, rules, snow_theater);
    seed_terrain_spawner_animation(&sim.interner, &mut sim.terrain_objects, &sim.session, rules)
}

#[cfg(test)]
mod tests {

    use super::*;

    use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
    use crate::rules::ini_parser::IniFile;
    use crate::rules::overlay_types::OverlayTypeRegistry;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::entity_store::EntityStore;

    use crate::sim::intern::StringInterner;

    use crate::sim::occupancy::{OccupancyGrid, RawCellOccupationGrid};
    use crate::sim::ore_growth::OreGrowthState;

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

    /// A flat Clear cell that allows tiberium, with the retail `[Clear]` row
    /// whose Track entry the new ore's Mark admission reads (`0x004834A0`).
    fn resolved_cell() -> ResolvedTerrainCell {
        ResolvedTerrainCell {
            allows_tiberium: true,
            ..crate::map::resolved_terrain::test_retail_clear_cell(0, 0)
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
        raw_occupation: RawCellOccupationGrid,
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
                raw_occupation: RawCellOccupationGrid::new(),
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
                TerrainSpawnContext::new(Some(&mut self.overlay_grid), &self.rules, rng)
                    .with_growth_queue(&mut self.growth_state, self.binary_frame, true, true)
                    .with_live_object_context(
                        &self.entities,
                        &self.occupancy,
                        &self.raw_occupation,
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

    /// `CellClass::PlaceTiberium @ 0x0048728C` reveals a tree's new ore into
    /// `OverlayClass::Mark`, whose ordinary admission
    /// (`CellClass::CheckCellPassability @ 0x004834A0`, called at `0x005FD00C`
    /// or for TIB01 at `0x005FD04F`, Track with both ignores) still refuses a
    /// landed aircraft's bit 0x40.
    #[test]
    fn tree_grows_no_ore_under_a_landed_aircraft() {
        let run = |landed: bool| {
            let mut world = SpawnWorld::new();
            if landed {
                for ry in 0..32 {
                    for rx in 0..32 {
                        world.raw_occupation.mark_ground(
                            rx,
                            ry,
                            crate::sim::occupancy::OBJECT_OCCUPATION_BIT,
                        );
                    }
                }
            }
            let mut spawners = BTreeMap::new();
            spawners.insert(
                (10, 10),
                spawner(&mut world.interner, "TIBTRE_HALF", 500_000),
            );
            let mut rng = SimRng::new(42);
            for _ in 0..200 {
                world.tick(&mut spawners, &mut rng);
            }
            world.placed()
        };

        assert!(!run(false).is_empty(), "the open control grows ore");
        assert!(
            run(true).is_empty(),
            "no ore identity under a landed aircraft"
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

    fn retail_terrain_scene(
        names: &[&str],
    ) -> Option<(
        crate::sim::world::Simulation,
        RuleSet,
        OverlayTypeRegistry,
        ResolvedTerrainGrid,
    )> {
        use crate::map::overlay::TerrainObject;
        let battle = crate::rules::retail_ini_fixture::retail_battle_rules_with_map_patch(
            &IniFile::from_str(
                "[TIBTRE01]\nAnimationProbability=1\nAnimationRate=1\n\
                 [TIBTRE02]\nAnimationProbability=1\nAnimationRate=2\n",
            ),
        )?;
        let (_, mut assets) = crate::rules::retail_ini_fixture::retail_scenario_assets()?;
        let theater = crate::map::theater::load_theater(&mut assets, "TEMPERATE").unwrap();
        let mut rules = battle.rules;
        rules.bind_terrain_assets(
            &battle.processed_rules,
            &assets,
            theater.extension,
            "TEMPERATE",
        );
        assert!(rules.terrain_spawner_frame_count("TIBTRE01").is_some());
        let registry =
            OverlayTypeRegistry::from_ini(&battle.processed_rules, Some(&battle.fixed_art));
        let mut sim = crate::sim::world::Simulation::new();
        sim.session_mut_for_test().set_map_width_for_test(8);
        sim.session_mut_for_test().set_map_height_for_test(8);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let template = resolved_grid(8, 8);
        sim.install_resolved_terrain_for_new_map(template.clone());
        sim.overlay_grid = Some(OverlayGrid::new(8, 8));
        let objects: Vec<_> = names
            .iter()
            .map(|name| TerrainObject {
                rx: 2,
                ry: 2,
                name: (*name).into(),
            })
            .collect();
        assert_eq!(
            construct_terrain_objects(&mut sim, &objects, &rules, false),
            names.len()
        );
        seed_terrain_spawner_animation(
            &sim.interner,
            &mut sim.terrain_objects,
            &sim.session,
            &rules,
        );
        // The generated constructor funnel receives an already materialized
        // grid; the ordinary fixture supplies that same derived projection.
        for &id in sim.terrain_objects.cell_index().values() {
            crate::sim::terrain_object::mark_terrain_occupation(
                sim.terrain_objects.get(id).unwrap(),
                sim.resolved_terrain.as_mut(),
            );
        }
        Some((sim, rules, registry, template))
    }

    #[test]
    fn overlapping_retail_drills_keep_independent_ai_and_snapshot_continuations() {
        use crate::sim::snapshot::GameSnapshot;
        use crate::sim::world::Simulation;
        let Some((mut sim, rules, registry, template)) =
            retail_terrain_scene(&["TIBTRE01", "TIBTRE02"])
        else {
            return;
        };
        assert_eq!(
            sim.terrain_objects.animations().len(),
            2,
            "Terrain ctor71BB90 and AI71C730 retain one Stage/timer per object"
        );
        let mut expected_rng = sim.scenario_rng.clone();
        expected_rng.next_u32();
        expected_rng.next_u32();
        sim.advance_tick(&[], Some(&rules), None, Some(&registry), 67);
        assert_eq!(
            sim.scenario_rng.logical_state(),
            expected_rng.logical_state()
        );
        for _ in 0..2 {
            sim.advance_tick(&[], Some(&rules), None, Some(&registry), 67);
        }
        let stages = |world: &Simulation| {
            world
                .terrain_objects
                .objects()
                .values()
                .map(|object| {
                    (
                        world.interner.resolve(object.type_ref()).to_owned(),
                        world.terrain_objects.animations()[&object.stable_id()].current_frame(),
                    )
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(
            stages(&sim),
            [("TIBTRE01".into(), 2), ("TIBTRE02".into(), 1)]
        );
        sim.scenario_rng = SimRng::new(0);
        let bytes = GameSnapshot::save_validated(
            &sim,
            1,
            rules.simulation_config_hash(),
            "overlapping drills",
            0,
        );
        let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
        restored.restore_after_snapshot_load().unwrap();
        restored.rebuild_caches_after_load(template, Default::default(), &rules);
        restored
            .restore_map_authority_after_snapshot_load(&rules, &registry, None)
            .unwrap();
        for _ in 0..8 {
            sim.advance_tick(&[], Some(&rules), None, Some(&registry), 67);
            restored.advance_tick(&[], Some(&rules), None, Some(&registry), 67);
            assert_eq!(stages(&sim), stages(&restored));
            assert_eq!(
                sim.scenario_rng.logical_state(),
                restored.scenario_rng.logical_state()
            );
        }
    }

    #[test]
    fn overlapping_terrain_uninit_preserves_successors_and_native_cell_projections() {
        use crate::sim::occupancy::CellObjectMember::Terrain;
        use crate::sim::snapshot::GameSnapshot;
        use crate::sim::world::{Simulation, UninitContext};
        let Some((mut sim, rules, registry, template)) =
            retail_terrain_scene(&["TIBTRE01", "TIBTRE02", "TREE01"])
        else {
            return;
        };
        let ids: Vec<_> = sim.terrain_objects.objects().keys().copied().collect();
        let (older, middle, head) = (ids[0], ids[1], ids[2]);
        let count = |sim: &mut Simulation| {
            let grid = crate::sim::pathfinding::PathGrid::new(8, 8);
            sim.movement_pass_cache
                .blocker_plane(
                    &mut sim.substrate.entities,
                    &grid,
                    sim.resolved_terrain.as_ref(),
                    &sim.terrain_objects,
                    sim.overlay_grid.as_ref(),
                    &sim.interner,
                    &rules,
                )
                .count_at(1, 2)
        };
        assert_eq!(count(&mut sim), 3);
        assert_eq!(
            sim.cell_objects(
                (2, 2),
                crate::sim::movement::locomotor::MovementLayer::Ground
            )
            .collect::<Vec<_>>(),
            [Terrain(head), Terrain(middle), Terrain(older)]
        );
        assert!(
            !sim.terrain_objects.spawning_cells().contains(&(2, 2)),
            "CanPlaceTiberium4838E0 reads the first Terrain type"
        );
        // Controlled inherited UnInit, not a claim that stock immune drills
        // can be killed by a weapon. Expiry bypasses the non-head middle ID.
        sim.uninit_with_context(middle, UninitContext::new(&rules, Some(&registry)));
        assert_eq!(
            sim.terrain_objects.members_at((2, 2)).collect::<Vec<_>>(),
            [head, older]
        );
        assert_eq!(
            sim.terrain_objects.next_in_cell(middle),
            Some(older),
            "RemoveContent47EAF7 retains Next on the already-bypassed arm"
        );
        assert_eq!(sim.next_cell_object(Terrain(head)), Some(Terrain(older)));
        assert_eq!(sim.next_cell_object(Terrain(middle)), Some(Terrain(older)));
        assert!(!sim.terrain_objects.animations().contains_key(&middle));
        assert!(sim.substrate.pending_delete.contains_object(&middle));
        assert_eq!(
            sim.substrate.raw_cell_occupation.ground_bits(2, 2),
            0,
            "Unmark literally AND-clears bits, without overlap reference counts"
        );
        assert!(
            sim.resolved_terrain
                .as_ref()
                .unwrap()
                .cell(2, 2)
                .unwrap()
                .terrain_object_occupation
                .is_some()
        );
        assert_eq!(count(&mut sim), 2);
        let bytes = GameSnapshot::save_validated(
            &sim,
            1,
            rules.simulation_config_hash(),
            "Terrain successor",
            0,
        );
        let mut sim = GameSnapshot::load(&bytes).unwrap().sim;
        sim.restore_after_snapshot_load().unwrap();
        sim.rebuild_caches_after_load(template, Default::default(), &rules);
        sim.restore_map_authority_after_snapshot_load(&rules, &registry, None)
            .unwrap();
        assert_eq!(
            sim.terrain_objects.members_at((2, 2)).collect::<Vec<_>>(),
            [head, older]
        );
        assert_eq!(count(&mut sim), 2);
        sim.advance_tick(&[], Some(&rules), None, Some(&registry), 67);
        assert!(
            sim.terrain_objects.get(middle).is_none(),
            "common late drain removes only the retired instance"
        );
        let retained_stage = sim.terrain_objects.animations()[&older].clone();
        sim.uninit_with_context(head, UninitContext::new(&rules, Some(&registry)));
        assert_eq!(sim.terrain_objects.id_at((2, 2)), Some(older));
        assert!(sim.terrain_objects.spawning_cells().contains(&(2, 2)));
        assert_eq!(sim.terrain_objects.animations()[&older], retained_stage);
        assert!(sim.terrain_objects.get(older).unwrap().in_logic_vector());
        assert!(sim.substrate.display.layer_of(older).is_some());
        assert_eq!(count(&mut sim), 1);
        sim.uninit_with_context(older, UninitContext::new(&rules, Some(&registry)));
        assert_eq!(sim.terrain_objects.id_at((2, 2)), None);
        assert!(sim.terrain_objects.spawning_cells().is_empty());
        assert_eq!(count(&mut sim), 0);
        sim.advance_tick(&[], Some(&rules), None, Some(&registry), 67);
        assert!(sim.terrain_objects.objects().is_empty());
    }

    #[test]
    fn wood_splash_reaches_both_overlapping_retail_trees_before_lethal_cleanup() {
        let Some((mut sim, rules, registry, _)) = retail_terrain_scene(&["TREE01", "TREE01"])
        else {
            return;
        };
        let ids: Vec<_> = sim.terrain_objects.members_at((2, 2)).collect();
        let impact = sim.terrain_objects.get(ids[0]).unwrap().world_coord();
        let warhead = rules.warhead("HE").unwrap();
        let warhead_ref = sim.interner.intern("HE");
        crate::sim::combat::world_receiver::apply_area_damage(
            &mut sim,
            &rules,
            Some(&registry),
            impact,
            10,
            warhead,
            (crate::sim::combat::RAD_NO_ATTACKER, None, warhead_ref),
            crate::sim::world::FrameEffects::default(),
        );
        for &id in &ids {
            let tree = sim.terrain_objects.get(id).unwrap();
            assert!(
                tree.health() > 0 && tree.health() < tree.max_health(),
                "the non-head receiver must not be skipped"
            );
        }
        crate::sim::combat::world_receiver::apply_area_damage(
            &mut sim,
            &rules,
            Some(&registry),
            impact,
            10000,
            warhead,
            (crate::sim::combat::RAD_NO_ATTACKER, None, warhead_ref),
            crate::sim::world::FrameEffects::default(),
        );
        assert_eq!(sim.terrain_objects.id_at((2, 2)), None);
        assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(2, 2), 0);
        sim.advance_tick(&[], Some(&rules), None, Some(&registry), 67);
        assert!(sim.terrain_objects.objects().is_empty());
    }

    /// A map entry becomes a live force-fire target through the real weapon.
    #[test]
    fn gsi_17_01_map_terrain_entry_becomes_a_force_fireable_object() {
        use crate::map::overlay::parse_terrain_objects;
        use crate::sim::combat::Health;
        use crate::sim::command::{Command, CommandEnvelope};
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
        let attacker_id = sim.substrate.allocate_stable_id();
        let mut attacker = GameEntity::test_default(attacker_id, "MTNK", "Americans", 5, 5);
        attacker.health = Health::new(300);
        let owner_id = attacker.owner();
        // `test_default` interns both owner and type through the shared test
        // interner. Snapshot it only after constructing the entity so those
        // handles resolve through the Simulation that will execute the order.
        sim.interner = crate::sim::intern::test_interner();

        sim.set_input_delay_ticks(0);
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
        let stable_id = sim.terrain_objects.cell_index()[&(10, 5)];
        assert_eq!(sim.terrain_objects.objects()[&stable_id].health(), 200);
        assert_eq!(
            sim.terrain_objects.objects()[&stable_id].occupation_bits(),
            4
        );

        let grid = PathGrid::test_all_passable(64, 64);
        sim.queue_command(CommandEnvelope::new(
            owner_id,
            sim.session.tick() + 1,
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
                .is_some_and(|e| e.attack_target.target().is_some());
            match sim.terrain_objects.objects().get(&stable_id) {
                Some(terrain) => {
                    last_health = terrain.health();
                    damaged |= terrain.health() < 200;
                    if terrain.lifecycle() == TerrainObjectLifecycle::Destroyed {
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
            !sim.terrain_objects.cell_index().contains_key(&(10, 5)),
            "a destroyed tree releases its cell"
        );
        assert_eq!(
            sim.substrate.raw_cell_occupation.ground_bits(10, 5) & 0x1C,
            0,
            "a destroyed tree releases its raw occupation bits"
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
            assert!(sim.terrain_objects.animations().is_empty());
            assert_eq!(sim.terrain_objects.objects().len(), objects.len());
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
                let stable_id = sim.terrain_objects.cell_index()[&(object.rx, object.ry)];
                let terrain = &sim.terrain_objects.objects()[&stable_id];
                assert_eq!(terrain.occupation_bits(), i32::from(source_mask));
                assert_eq!(terrain.lifecycle(), TerrainObjectLifecycle::Live);
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
    fn retail_tree_signed_masks_survive_authored_mark_restore_and_limbo() {
        use crate::sim::world::Simulation;
        let assert_movement_zone = |sim: &Simulation| {
            let zone = crate::sim::pathfinding::zone_build::movement_class_for_cell(
                sim.resolved_terrain.as_ref().unwrap(),
                2,
                2,
            );
            // CellClass::RecalcZoneType483DFB/483E12 compares the full
            // selected dword with exactly7, independently of occupation.
            assert_eq!(zone, crate::map::resolved_terrain::zone_class::BUILDING);
            use crate::rules::locomotor_type::MovementZone;
            use crate::sim::pathfinding::passability::is_passable_for_zone;
            assert!(is_passable_for_zone(zone, MovementZone::Infantry));
            assert!(!is_passable_for_zone(zone, MovementZone::Destroyer));
        };
        let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules_with_map_patch(
            &IniFile::from_str("[TREE01]\nTemperateOccupationBits=8\nSnowOccupationBits=-1\n"),
        ) else {
            return;
        };
        let rules = battle.rules;
        let registry = OverlayTypeRegistry::empty();
        let objects = crate::map::overlay::parse_terrain_objects(&IniFile::from_str(
            "[Terrain]\n2002=TREE01\n",
        ));
        // Native Mark_Occupation71C17B..71C199 tests only bits1/2/4.
        // These two authored boundaries distinguish both old clamp errors.
        for (snow, mask, marked) in [(false, 8, 0xA3), (true, -1, 0xBF)] {
            let template = resolved_grid(4, 4);
            let mut sim = Simulation::new();
            sim.session_mut_for_test().set_map_width_for_test(4);
            sim.session_mut_for_test().set_map_height_for_test(4);
            sim.intern_rule_type_ids(&rules);
            sim.resolve_type_handles(&rules);
            sim.install_resolved_terrain_for_new_map(template.clone());
            sim.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(4, 4));
            sim.native_unique_ids = Some(
                crate::sim::native_identity::build_noncampaign_fresh_id_prefix(
                    0, 0, 0, 0, 0, 0, 1, 1,
                )
                .into_cursor(),
            );
            sim.substrate.raw_cell_occupation.mark_ground(2, 2, 0xA3);
            assert_eq!(
                construct_authored_terrain_objects(&mut sim, &objects, &rules, snow, &registry)
                    .unwrap(),
                1
            );
            let id = sim.terrain_objects.cell_index()[&(2, 2)];
            assert_eq!(sim.terrain_objects.objects()[&id].occupation_bits(), mask);
            assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(2, 2), marked);
            assert_eq!(
                sim.resolved_terrain
                    .as_ref()
                    .unwrap()
                    .cell(2, 2)
                    .unwrap()
                    .terrain_object_occupation,
                Some(mask)
            );
            assert_movement_zone(&sim);

            let bytes = crate::sim::snapshot::GameSnapshot::save_validated(
                &sim,
                1,
                rules.simulation_config_hash(),
                "terrain mask",
                0,
            );
            let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
                .unwrap()
                .sim;
            restored.restore_after_snapshot_load().unwrap();
            restored.rebuild_caches_after_load(template, Default::default(), &rules);
            restored
                .restore_map_authority_after_snapshot_load(&rules, &registry, None)
                .unwrap();
            restored.resolve_type_handles(&rules);
            assert_eq!(
                restored.substrate.raw_cell_occupation.ground_bits(2, 2),
                marked
            );
            assert_eq!(
                restored.terrain_objects.objects()[&id].occupation_bits(),
                mask
            );
            assert_eq!(
                restored
                    .resolved_terrain
                    .as_ref()
                    .unwrap()
                    .cell(2, 2)
                    .unwrap()
                    .terrain_object_occupation,
                Some(mask)
            );
            assert_movement_zone(&restored);
            // Stock HE takes the real area receiver through Terrain's lethal
            // owner, whose Limbo clears the selected occupation bits.
            let impact = restored.terrain_objects.objects()[&id].world_coord();
            let warhead = rules.warhead("HE").unwrap();
            let warhead_ref = restored.intern("HE");
            crate::sim::combat::world_receiver::apply_area_damage(
                &mut restored,
                &rules,
                Some(&registry),
                impact,
                10000,
                warhead,
                (crate::sim::combat::RAD_NO_ATTACKER, None, warhead_ref),
                crate::sim::world::FrameEffects::default(),
            );
            assert_eq!(
                restored.substrate.raw_cell_occupation.ground_bits(2, 2),
                0xA3
            );
            assert!(!restored.terrain_objects.cell_index().contains_key(&(2, 2)));
            assert_eq!(
                restored
                    .resolved_terrain
                    .as_ref()
                    .unwrap()
                    .cell(2, 2)
                    .unwrap()
                    .terrain_object_occupation,
                None
            );
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
        let stable_id = sim.terrain_objects.cell_index()[&(2, 2)];
        assert_eq!(
            sim.terrain_objects.objects()[&stable_id].native_unique_id(),
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
}
