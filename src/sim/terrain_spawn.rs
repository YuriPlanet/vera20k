//! TIBTRE-style terrain object ore spawning.
//!
//! Per-object retained animation state for terrain objects with `IsAnimated=yes`.
//! Idle animations roll `AnimationProbability`; a hit starts the native timer.
//! `SpawnsTiberium=yes` resets the animation and emits ore at its midpoint.
//! Under the Phobos capability, `SpawnsTiberium.CellsPerAnim/Type/Range/
//! GrowthStage/Particle` replace that single emission; see
//! [docs/phobos-ore-spawners.md](../../docs/phobos-ore-spawners.md).
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

#[cfg(test)]
use std::collections::BTreeMap;

use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::{InternedId, StringInterner};
#[cfg(test)]
use crate::sim::overlay_grid::OverlayGrid;
use crate::sim::overlay_mark_host::RuntimeOverlayError;
use crate::sim::rng::SimRng;
use crate::sim::scenario_session::ScenarioSession;
use crate::sim::terrain_object::TerrainObjectState;
use crate::sim::terrain_object::TerrainObjectStore;
use crate::sim::tiberium::SpawnerSpread;
use crate::sim::tiberium::spread_tiberium;
use crate::sim::timer::CdTimer;
use crate::sim::world::FrameEffects;
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

    /// The seeded SHP frame count. The rules asset catalog supplies it in
    /// production; a fixture may seed the state directly.
    pub(crate) fn frame_count(&self) -> u16 {
        self.frame_count as u16
    }

    /// `TerrainClass::Start_To_Crumble @ 0x0071C8E0`: Stage resets to 0 at
    /// rate 2. A non-animated tree has no seeded state; this one only feeds
    /// the crumble sequence.
    pub(crate) fn new_crumbling(type_ref: InternedId, frame_count: u16, binary_frame: u32) -> Self {
        let mut state = Self::new(
            type_ref,
            NativeF32Bits::POSITIVE_ZERO,
            2,
            frame_count,
            binary_frame,
        );
        state.current_frame = 0;
        state.rate = 2;
        state.timer = CdTimer::started(binary_frame as i32, 2);
        state
    }

    pub(crate) fn start_crumble(&mut self, binary_frame: u32) {
        self.current_frame = 0;
        self.rate = 2;
        self.timer.start(binary_frame as i32, 2);
    }

    /// One retained Stage step for the crumble sequence.
    pub(crate) fn advance_crumble(&mut self, binary_frame: u32) -> bool {
        let now = binary_frame as i32;
        if self.rate == 0 || !self.timer.expired(now) {
            return false;
        }
        self.current_frame = self.current_frame.wrapping_add(1);
        self.timer.start(now, self.rate);
        true
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
        cycle: i32,
        reset_without_spawner: bool,
        rng: &mut SimRng,
    ) -> bool {
        let cycle = if cycle < 0 {
            i32::from(self.frame_count) / 2
        } else {
            cycle
        };
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
        if is_animated && self.current_frame == cycle && (spawns_tiberium || reset_without_spawner)
        {
            self.current_frame = 0;
            self.rate = 0;
            self.timer.start(now, 0);
            return spawns_tiberium;
        }
        false
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn advance_for_test(&mut self, binary_frame: u32, rng: &mut SimRng) {
        self.tick(binary_frame, true, true, -1, false, rng);
    }
}

/// Dispatch one TerrainClass AI slot: the animation and ore emission
/// ([`tick_terrain_animation`]), then under Ares the forest fire.
pub(crate) fn tick_terrain_object_ai(
    sim: &mut crate::sim::world::Simulation,
    stable_id: u64,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) -> Result<(), RuntimeOverlayError> {
    let live = sim
        .terrain_objects
        .objects()
        .get(&stable_id)
        .is_some_and(|terrain| terrain.is_live());
    // Phobos `TerrainClass_AI_Crumbling` (0x71C812) precedes the IsAnimated
    // block and returns from the AI on both of its reached arms.
    if tick_terrain_crumble(sim, stable_id, rules, overlay_registry, effects)? {
        return Ok(());
    }
    let live = rules.ares_form()
        && sim
            .terrain_objects
            .objects()
            .get(&stable_id)
            .is_some_and(|terrain| terrain.is_live());
    tick_terrain_animation(sim, stable_id, rules, overlay_registry, effects)?;
    // Ares 3.0 TerrainClass_Update_ForestFire (1004F530, hook 0x71C7C2)
    // follows the IsAnimated draw. VERA runs it after the whole stage step:
    // the order differs only for an animated flammable non-spawner tree
    // whose stage reaches its midpoint the same frame (none in retail).
    if live {
        sim.ares_spread_forest_fire(stable_id, rules);
    }
    Ok(())
}

/// Phobos `TerrainClass_AI_Crumbling` (`0x71C812`,
/// `Ext/TerrainType/Hooks.cpp:80-131`): only a crumbling object enters. A
/// `HasDamagedFrames=` type above zero health clears the damaged-redraw
/// latch (and leaves Logic when it is neither animated nor flammable); the
/// lethal arm plays the destroy effects and removes the tree once
/// `currentStage + 1 == Frames / 2`, otherwise advances the stage at rate 2.
/// Both reached arms return from the AI, skipping the IsAnimated block.
fn tick_terrain_crumble(
    sim: &mut crate::sim::world::Simulation,
    stable_id: u64,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) -> Result<bool, RuntimeOverlayError> {
    if !sim.terrain_objects.is_crumbling(stable_id) {
        return Ok(false);
    }
    let Some(type_ref) = sim
        .terrain_objects
        .objects()
        .get(&stable_id)
        .filter(|terrain| terrain.is_live())
        .map(TerrainObjectState::type_ref)
    else {
        return Ok(false);
    };
    let name = sim.interner.resolve(type_ref).to_string();
    let Some(terrain_type) = rules.terrain_object_type_case_insensitive(&name) else {
        return Ok(false);
    };
    let health = sim.terrain_objects.objects()[&stable_id].health();
    if terrain_type.has_damaged_frames && health > 0 {
        if !terrain_type.is_animated && !terrain_type.is_flammable {
            sim.terrain_remove_from_logic(stable_id);
        }
        sim.terrain_objects.set_crumbling(stable_id, false);
        return Ok(false);
    }
    // `0x71C7C2..0x71C807` runs the stock Stage step before `0x71C80A` tests
    // IsCrumbling, and the `0x71C812` hook compares the stepped value; a
    // visit whose timer did not step returns at `0x71C840` without reaching
    // the hook.
    if !sim
        .terrain_objects
        .advance_crumble(stable_id, sim.session.binary_frame())
    {
        return Ok(true);
    }
    let frames = i32::from(rules.terrain_display_frame_count(&name).unwrap_or(0));
    let damaged = i32::from(terrain_type.has_damaged_frames);
    let animation_length = terrain_type
        .animation_length
        .unwrap_or(frames / (2 * (damaged + 1)));
    let stage = sim
        .terrain_objects
        .animation_current_frame(stable_id)
        .unwrap_or(0);
    let current_stage = stage
        + if terrain_type.is_animated {
            animation_length * (damaged + 1)
        } else {
            damaged
        };
    if current_stage + 1 == frames / 2 {
        sim.terrain_play_destroy_effects(stable_id, rules);
        sim.terrain_crumble_finish(stable_id, rules, overlay_registry, effects);
    }
    Ok(true)
}

/// TerrainClass::AI71C730's IsAnimated draw, stage step and ore emission.
/// The retained Stage/rate/timer and any probability draw change before the
/// midpoint's forced spread. Release the animation borrow before entering
/// Overlay5FC380/Mark5FC570, whose CellAnim callbacks require the full live
/// Simulation and its existing owners.
fn tick_terrain_animation(
    sim: &mut crate::sim::world::Simulation,
    stable_id: u64,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) -> Result<(), RuntimeOverlayError> {
    let Some(cell) = sim
        .terrain_objects
        .objects()
        .get(&stable_id)
        .filter(|terrain| terrain.is_live())
        .map(TerrainObjectState::cell)
    else {
        return Ok(());
    };
    let Some(type_ref) = sim
        .terrain_objects
        .animations()
        .get(&stable_id)
        .map(TerrainAnimationState::type_ref)
    else {
        return Ok(());
    };
    let terrain_type = rules.terrain_object_type_case_insensitive(sim.interner.resolve(type_ref));
    let is_animated = terrain_type.is_none_or(|ty| ty.is_animated);
    let spawns_tiberium = terrain_type.map_or_else(
        || sim.terrain_objects.spawning_cells().contains(&cell),
        |ty| ty.spawns_tiberium,
    );
    // Phobos `TerrainClass_AI_Animated` (0x71C84D): the cycle is
    // `AnimationLength`, else `Frames / (2 * (HasDamagedFrames + 1))`, and
    // every animated type resets (only a spawner spreads ore).
    let (cycle, reset_without_spawner) = terrain_type.map_or((-1, false), |ty| {
        let frames = rules
            .terrain_display_frame_count(sim.interner.resolve(type_ref))
            .map(i32::from)
            .or_else(|| {
                sim.terrain_objects
                    .animations()
                    .get(&stable_id)
                    .map(|animation| i32::from(animation.frame_count()))
            })
            .unwrap_or(0);
        let damaged = i32::from(ty.has_damaged_frames);
        (
            ty.animation_length.unwrap_or(frames / (2 * (damaged + 1))),
            true,
        )
    });
    let fired = {
        let Some((spawner, _, _)) = sim.terrain_objects.animation_with_indexes_mut(stable_id)
        else {
            return Ok(());
        };
        spawner.tick(
            sim.session.binary_frame(),
            is_animated,
            spawns_tiberium,
            cycle,
            reset_without_spawner,
            &mut sim.scenario_rng,
        )
    };
    if !fired {
        return Ok(());
    }
    let Some(registry) = overlay_registry else {
        return Ok(());
    };
    if sim.overlay_grid.is_none() || sim.resolved_terrain.is_none() {
        return Ok(());
    }
    let ext = terrain_type.map(|ty| &ty.spawns_tiberium_ext);
    // TerrainTypeExt::GetCellsPerAnim draws once after the native midpoint
    // reset. Single-valued/reversed ranges draw nothing, including zero attempts.
    let cells = ext.map_or(1, |ext| {
        let range = ext.cells_per_anim;
        if range.is_single() {
            range.low
        } else {
            sim.scenario_rng
                .next_range_i32_inclusive(range.low, range.high)
        }
    });
    let spread_context = ext.map(|ext| SpawnerSpread {
        tiberium_type: ext.tiberium_type,
        range: ext.range,
        growth_stage: ext.growth_stage,
    });
    let mut placed_cells = Vec::new();
    let mut result = Ok(());
    for _ in 0..cells.max(0) {
        match spread_tiberium(sim, rules, registry, cell, true, spread_context, effects) {
            Ok(Some(emitted)) => placed_cells.push(emitted.cell()),
            Ok(None) => break,
            Err(error) => {
                result = Err(error);
                break;
            }
        }
    }
    // Keep the existing plain-ore Recalc batching, including successful cells
    // preceding an error. Structural and CellAnim callbacks already ran inside
    // the shared constructor; no deferred Overlay identity receipt remains.
    if !placed_cells.is_empty() {
        sim.publish_tiberium_cells(rules, registry, &placed_cells);
    }
    result?;
    spawn_spawner_particle(sim, rules, stable_id);
    Ok(())
}

/// Phobos `TerrainTypeExt`'s `SpawnsTiberium.Particle`, emitted after the
/// midpoint's spread loop: `ParticleSystemClass::SpawnParticle @ 0x0062E430`
/// for the named `ParticleTypeClass` on the Scenario's shared system. That
/// system is the global at `0xA8ED78`, which `ScenarioSetup @ 0x00685040` fills
/// with the literal GasCloudSys instance, so the ore spawner shares
/// [`Simulation::spawn_gas_particle`]'s owner rather than a second one.
fn spawn_spawner_particle(
    sim: &mut crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    stable_id: u64,
) {
    let Some((type_ref, location)) = sim
        .terrain_objects
        .objects()
        .get(&stable_id)
        .filter(|terrain| terrain.is_live())
        .map(|terrain| (terrain.type_ref(), terrain.world_coord()))
    else {
        return;
    };
    let Some(kind) = rules
        .terrain_object_type_case_insensitive(sim.interner().resolve(type_ref))
        .and_then(|terrain| terrain.spawns_tiberium_ext.particle.as_deref())
        .and_then(|name| rules.p_type_id_by_name(name))
    else {
        return;
    };
    sim.spawn_gas_particle(
        kind,
        glam::IVec3::new(location.x, location.y, location.z),
        rules,
    );
}

/// Compatibility construction without a native fresh-map prefix.
///
/// Admitted authored/generated scenarios use the same constructor below with
/// an explicit mode, retaining every native numeric identity.
pub fn construct_terrain_objects(
    sim: &mut crate::sim::world::Simulation,
    terrain_objects: &[crate::map::overlay::TerrainObject],
    rules: &crate::rules::ruleset::RuleSet,
    snow_theater: bool,
) -> usize {
    construct_terrain_objects_with_rules(
        sim,
        terrain_objects,
        &mut TerrainConstructionRules::Projected(rules),
        snow_theater,
        TerrainConstructionMode::Compatibility,
    )
    .expect("compatibility Terrain construction requires an admitted native footprint")
}

/// Native identity admission is independent of authored Unlimbo resource
/// clearing. RMG5A2C05 constructs a drill before painting its resource, so
/// generated materialization must retain that already-painted cell.
#[derive(Clone, Copy)]
pub enum TerrainConstructionMode<'a> {
    Compatibility,
    Generated,
    Authored(&'a OverlayTypeRegistry),
}

#[derive(Debug, thiserror::Error)]
pub enum TerrainConstructionError {
    #[error(transparent)]
    NativeIdentity(#[from] crate::sim::native_identity::NativeMapTubeConstructionError),
    #[error(transparent)]
    Rules(#[from] crate::rules::error::RulesError),
}

/// Borrowed construction policy, never another Rules registry. Ordinary map
/// reading can mutate its continuing family; generated/compatibility inputs
/// retain the already projected types their earlier construction selected.
pub enum TerrainConstructionRules<'a> {
    Projected(&'a crate::rules::ruleset::RuleSet),
    Authored {
        rules: &'a mut crate::rules::ruleset::RuleSet,
        owner: &'a mut crate::rules::process_owner::NativeRulesProcessOwner,
    },
}

impl TerrainConstructionRules<'_> {
    pub(crate) fn rules(&self) -> &crate::rules::ruleset::RuleSet {
        match self {
            Self::Projected(rules) => rules,
            Self::Authored { rules, .. } => rules,
        }
    }

    fn find_or_allocate(
        &mut self,
        incoming: &str,
    ) -> Result<
        Option<(
            &crate::rules::terrain_object_type::TerrainObjectType,
            Option<crate::rules::native_processing::NativeTypeConstructionEvent>,
        )>,
        crate::rules::error::RulesError,
    > {
        match self {
            Self::Projected(rules) => Ok(rules
                .terrain_object_type_case_insensitive(incoming)
                .map(|terrain| (terrain, None))),
            Self::Authored { rules, owner } => {
                let Some((name, constructor)) =
                    owner.find_or_allocate_map_terrain_type(incoming, rules)?
                else {
                    return Ok(None);
                };
                Ok(Some((
                    rules
                        .terrain_object_type_case_insensitive(&name)
                        .expect("retained map factory result has its typed projection"),
                    constructor,
                )))
            }
        }
    }
}

/// One TerrainCtor71BB90 port for every source. Create_ID at71BC19 precedes
/// zero-cell/Unlimbo admission, including a later failed placement. Authored
/// input completes Recalc then clears a source resource; RMG has already run
/// that suffix before painting. Both spend the same numeric identity.
pub fn construct_terrain_objects_with_rules(
    sim: &mut crate::sim::world::Simulation,
    terrain_objects: &[crate::map::overlay::TerrainObject],
    rules: &mut TerrainConstructionRules<'_>,
    snow_theater: bool,
    mode: TerrainConstructionMode<'_>,
) -> Result<usize, TerrainConstructionError> {
    let old_terrain_ids = sim
        .terrain_objects
        .objects()
        .keys()
        .copied()
        .collect::<Vec<_>>();
    for stable_id in old_terrain_ids {
        sim.unregister_non_entity_object(stable_id);
    }
    let cell_grid = sim.cell_grid();
    sim.terrain_objects.configure_cell_grid(cell_grid);
    sim.terrain_objects.clear();

    let mut constructed = 0usize;
    for obj in terrain_objects {
        let Some((t, constructor)) = rules.find_or_allocate(&obj.name)? else {
            continue;
        };
        // TerrainReadMap71CA70 at71CAD2 -> TypeCtor71DA80 at71DB11
        // constructs only on a miss, before this instance's71BC19 identity.
        if let Some(_constructor) = constructor {
            sim.next_native_load_id()?;
        }
        let type_ref = sim.interner.intern(&t.name);
        // TerrainCtor71BC19 -> AbstractCreate_ID410230 -> Scenario68BCB0,
        // original instructions: one numeric increment, no RNG draw.
        let stable_id = sim.substrate.allocate_stable_id();
        let native_unique_id = match mode {
            TerrainConstructionMode::Compatibility => None,
            TerrainConstructionMode::Generated | TerrainConstructionMode::Authored(_) => {
                Some(sim.next_native_load_id()?)
            }
        };
        let terrain_state = TerrainObjectState::new(
            stable_id,
            type_ref,
            (obj.rx, obj.ry),
            t,
            snow_theater,
            sim.resolved_terrain.as_ref(),
        )
        .with_native_unique_id(native_unique_id);
        // Query only a reached PlaceDown: the zero sentinel skips Unlimbo
        // entirely, even when its retained type has an unsupported list.
        let cells = if terrain_state.is_live() {
            terrain_state.allocated_footprint_cells(
                &t.occupied_cell_offsets()?,
                sim.resolved_terrain.as_ref(),
            )
        } else {
            Vec::new()
        };
        sim.terrain_objects
            .insert_constructed(terrain_state, t.spawns_tiberium);
        constructed += 1;
        // TerrainCtor71BC34..71BC86 (original instructions) retains the
        // zero-cell identity but skips Unlimbo, including resource clearing.
        if !sim.terrain_objects.get(stable_id).unwrap().is_live() {
            continue;
        }
        let terrain_snapshot = sim.terrain_objects.get(stable_id).unwrap().clone();
        // TypeCtor71DAF1 leaves Occupy_List null; MapPlaceDown568432
        // visits no Cell, independently of successful Unlimbo/Display.
        for cell in cells {
            sim.terrain_objects.add_to_cell(stable_id, cell);
            terrain_snapshot.mark_raw_occupation(
                &mut sim.substrate.raw_cell_occupation,
                sim.resolved_terrain.as_ref(),
            );
            if let Some(grid) = sim.resolved_terrain.as_mut() {
                sim.terrain_objects.project_cell_occupation(grid, cell);
            }
            // PlaceDown5684E1 Recalc belongs to this recipient. Display and
            // Terrain's once-only halo/resource suffix follow the whole list.
            sim.publish_recalculated_cells(rules.rules(), &[cell]);
        }
        let registered = sim.register_terrain_object(stable_id, rules.rules());
        debug_assert!(registered);
        if let TerrainConstructionMode::Authored(overlay_registry) = mode {
            // TerrainUnlimbo71D080..71D097 has eight ordered Cell lookups
            // even when PlaceDown admitted no Cell content. Real counters
            // stay derived; Dummy's wrapping byte is retained process state.
            terrain_snapshot.adjust_dummy_neighbors(sim.resolved_terrain.as_ref(), true);
            let source = terrain_snapshot.raw_occupation_cell(sim.resolved_terrain.as_ref());
            // 71D0E2..71D12B clears only the selected Cell's literal overlay
            // identity/data fields, without DestroyOverlay, Recalc or RNG.
            let before = match source {
                crate::sim::occupancy::RawCellKey::Real(x, y) => {
                    sim.overlay_grid.as_ref().map(|grid| {
                        let source = grid.cell(x, y);
                        (source.overlay_id.map_or(-1, i32::from), source.overlay_data)
                    })
                }
                crate::sim::occupancy::RawCellKey::Dummy => sim
                    .resolved_terrain
                    .as_ref()
                    .map(|terrain| terrain.shared_cell_dummy().overlay_identity_state()),
            };
            if let Some(before) = before {
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
                    match source {
                        crate::sim::occupancy::RawCellKey::Real(x, y) => {
                            sim.overlay_grid.as_mut().unwrap().clear_cell_silently(x, y)
                        }
                        crate::sim::occupancy::RawCellKey::Dummy => sim
                            .resolved_terrain
                            .as_ref()
                            .unwrap()
                            .shared_cell_dummy()
                            .write_overlay_identity_state(after.0, after.1),
                    }
                }
            }
        } else {
            sim.terrain_objects
                .get(stable_id)
                .unwrap()
                .adjust_dummy_neighbors(sim.resolved_terrain.as_ref(), true);
        }
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

/// Phobos `TerrainTypeExt::PlayDestroyEffects`
/// (`Ext/TerrainType/Body.cpp:17-21`): `DestroySound` at the terrain's
/// coordinate, then a `DestroyAnim` there.
impl crate::sim::world::Simulation {
    pub(crate) fn terrain_play_destroy_effects(&mut self, stable_id: u64, rules: &RuleSet) {
        let Some(terrain) = self.terrain_objects.get(stable_id) else {
            return;
        };
        let name = self.interner.resolve(terrain.type_ref()).to_string();
        let coords = terrain.world_coord();
        let Some(terrain_type) = rules.terrain_object_type_case_insensitive(&name) else {
            return;
        };
        let world = crate::sim::anim_class::AnimWorldCoord {
            x: coords.x,
            y: coords.y,
            z: coords.z,
        };
        if let Some(sound) = &terrain_type.destroy_sound {
            self.sound_events
                .emit(crate::sim::world::SimSoundEvent::voc_at_world_coord(
                    sound.clone(),
                    world,
                ));
        }
        if let Some(anim) = self.resolve_random_anim_name(rules, &terrain_type.destroy_anim) {
            let (rx, ry, sub_x, sub_y, z) = world.to_cell_sub_z();
            let descriptor = crate::sim::components::AnimClassSpawnDescriptor::new(
                self.interner.intern(anim),
                rx,
                ry,
                sub_x,
                sub_y,
                z,
            );
            let _ = self.spawn_anim_at_world(rules, descriptor, world);
        }
    }
}

#[cfg(all(test, tests_sim_t_z))]
#[cfg(all(test, tests_sim_t_z))]
mod tests {

    use super::*;

    use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
    use crate::rules::ini_parser::IniFile;
    use crate::rules::overlay_types::OverlayTypeRegistry;
    use crate::rules::ruleset::RuleSet;

    use crate::sim::intern::StringInterner;

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

    /// A complete live world: these synthetic animation controls enter the same
    /// TerrainClass AI and Overlay constructor as the runtime.
    struct SpawnWorld {
        binary_frame: u32,
        registry: OverlayTypeRegistry,
        rules: RuleSet,
        sim: crate::sim::world::Simulation,
    }

    impl SpawnWorld {
        fn new() -> Self {
            let text = format!(
                "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
                 [TerrainTypes]\n0=TIBTRE_HALF\n\
                 [TIBTRE_HALF]\nIsAnimated=yes\nSpawnsTiberium=yes\n\
                 {}",
                crate::sim::tiberium::test_support::tiberium_rules_text(),
            );
            let ini = IniFile::from_str(&text);
            let rules = RuleSet::from_ini(&ini).expect("spawn world rules");
            let registry = OverlayTypeRegistry::from_ini(&ini, None);
            let mut sim = crate::sim::world::Simulation::new();
            sim.overlay_grid = Some(OverlayGrid::new(32, 32));
            sim.install_resolved_terrain_for_new_map(resolved_grid(32, 32));
            sim.ore_growth_config =
                crate::sim::ore_growth::OreGrowthConfig::for_test(true, true, false);
            sim.ore_growth_state = crate::sim::ore_growth::OreGrowthState::new(32, 32);
            sim.ore_growth_state
                .reset_native_tiberium_classes(rules.tiberium_types.len(), 0);
            Self {
                binary_frame: 0,
                registry,
                rules,
                sim,
            }
        }

        fn tick(
            &mut self,
            spawners: &mut BTreeMap<(u16, u16), TerrainAnimationState>,
            rng: &mut SimRng,
        ) {
            if self.sim.terrain_objects.objects().is_empty() {
                let objects = spawners
                    .iter()
                    .map(|(&cell, state)| crate::map::overlay::TerrainObject {
                        rx: cell.0,
                        ry: cell.1,
                        name: self.sim.interner.resolve(state.type_ref()).to_string(),
                    })
                    .collect::<Vec<_>>();
                assert_eq!(
                    construct_terrain_objects(&mut self.sim, &objects, &self.rules, false),
                    objects.len()
                );
            }
            self.sim
                .session
                .set_binary_frame_for_test(self.binary_frame);
            std::mem::swap(&mut self.sim.scenario_rng, rng);
            for (&cell, state) in spawners {
                let id = self
                    .sim
                    .terrain_objects
                    .id_at(cell)
                    .expect("constructed spawner cell");
                self.sim.terrain_objects.seed_animation(id, state.clone());
                tick_terrain_object_ai(
                    &mut self.sim,
                    id,
                    &self.rules,
                    Some(&self.registry),
                    FrameEffects::default(),
                )
                .unwrap();
                *state = self.sim.terrain_objects.animations()[&id].clone();
            }
            std::mem::swap(&mut self.sim.scenario_rng, rng);
            self.binary_frame += 1;
        }

        fn placed(&self) -> Vec<((u16, u16), u8)> {
            self.sim
                .overlay_grid
                .as_ref()
                .unwrap()
                .iter_occupied()
                .map(|(rx, ry, cell)| ((rx, ry), cell.overlay_data))
                .collect()
        }
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
                        world.sim.substrate.raw_cell_occupation.mark_ground(
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
                spawner(&mut world.sim.interner, "TIBTRE_HALF", 500_000),
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
                spawner(&mut world.sim.interner, "TIBTRE_HALF", 500_000),
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
            &assets,
            theater.extension,
            crate::rules::terrain_asset_catalog::TerrainAssetLoadPhase::Fresh,
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
    fn destructible_retail_drill_constructs_its_c4_anim_and_restores_until_cleanup() {
        use crate::map::overlay::TerrainObject;
        use crate::sim::snapshot::GameSnapshot;
        use crate::sim::world::SimSoundEvent;

        let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch(
            "XMP03T4.MAP",
            &IniFile::from_str("[TIBTRE01]\nImmune=no\n"),
        ) else {
            return;
        };
        let (_, mut assets) = crate::rules::retail_ini_fixture::retail_scenario_assets().unwrap();
        let theater = crate::map::theater::load_theater(&mut assets, "TEMPERATE").unwrap();
        let mut rules = battle.rules;
        rules.bind_terrain_assets(
            &assets,
            theater.extension,
            crate::rules::terrain_asset_catalog::TerrainAssetLoadPhase::Fresh,
        );
        let roots = crate::rules::effect_asset_catalog::anim_class_roots(&rules);
        rules.bind_anim_class_assets(&roots, &assets, theater.extension, "TEMPERATE");
        let registry =
            OverlayTypeRegistry::from_ini(&battle.processed_rules, Some(&battle.fixed_art));
        let mut sim = crate::sim::world::Simulation::with_seed(0);
        sim.session.set_map_width_for_test(8);
        sim.session.set_map_height_for_test(8);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let template = resolved_grid(8, 8);
        sim.install_resolved_terrain_for_new_map(template.clone());
        sim.overlay_grid = Some(OverlayGrid::new(8, 8));
        let objects = [
            TerrainObject {
                rx: 2,
                ry: 2,
                name: "TIBTRE01".into(),
            },
            TerrainObject {
                rx: 4,
                ry: 2,
                name: "TIBTRE02".into(),
            },
        ];
        assert_eq!(
            construct_terrain_objects(&mut sim, &objects, &rules, false),
            2
        );
        let doomed = sim.terrain_objects.id_at((2, 2)).unwrap();
        let immune = sim.terrain_objects.id_at((4, 2)).unwrap();
        let impact = sim.terrain_objects.get(doomed).unwrap().world_coord();
        let he = rules.warhead("HE").unwrap();
        let he_ref = sim.interner.intern("HE");
        for id in [immune, doomed] {
            let at = sim.terrain_objects.get(id).unwrap().world_coord();
            crate::sim::combat::world_receiver::apply_area_damage(
                &mut sim,
                &rules,
                Some(&registry),
                at,
                10000,
                he,
                true,
                (crate::sim::combat::RAD_NO_ATTACKER, None, he_ref),
                crate::sim::world::FrameEffects::default(),
            );
            if id == immune {
                assert_eq!(sim.terrain_objects.get(id).unwrap().health(), 200);
                assert_eq!(
                    sim.anims().count(),
                    0,
                    "stock immunity must not construct an explosion"
                );
            }
        }
        let (anim_id, anim) = sim
            .anims()
            .next()
            .expect("Terrain71BA85 constructs before nested C4");
        let anim_id = *anim_id;
        // SelectAnim48A5EB..48A615: damage100 picks slot4 of the retail Super list.
        assert_eq!(sim.interner.resolve(anim.type_id()), "XGRYMED2");
        assert_eq!(
            [
                anim.world_coord().x,
                anim.world_coord().y,
                anim.world_coord().z
            ],
            [impact.x, impact.y, impact.z]
        );
        assert_eq!((anim.draw_flags(), anim.z_adjust()), (0x2600, -15));
        assert!(sim.sound_events.iter().any(|event| matches!(event,
            SimSoundEvent::AnimationStarted { anim_id: id, sound_id, .. }
                if *id == anim_id && sound_id == "EXPLOSION12"
        )));
        assert!(!sim.terrain_objects.get(doomed).unwrap().is_live());
        sim.scenario_rng = SimRng::new(0); // Native load resets this stream.
        let saved = GameSnapshot::save_validated(
            &sim,
            1,
            rules.simulation_config_hash(),
            "drill explosion",
            0,
        );
        let mut restored = GameSnapshot::load(&saved).unwrap().sim;
        restored.restore_after_snapshot_load().unwrap();
        restored.rebuild_caches_after_load(template, Default::default(), &rules);
        restored
            .restore_map_authority_after_snapshot_load(&rules, &registry, None)
            .unwrap();
        assert_eq!(sim.anim(anim_id), restored.anim(anim_id));
        for _ in 0..200 {
            sim.advance_tick(&[], Some(&rules), None, Some(&registry), 67);
            restored.advance_tick(&[], Some(&rules), None, Some(&registry), 67);
            assert_eq!(sim.anim(anim_id), restored.anim(anim_id));
            if sim.anim(anim_id).is_none() {
                break;
            }
        }
        assert!(
            sim.anim(anim_id).is_none(),
            "the normal Anim lifecycle must retire the explosion"
        );
        assert!(sim.terrain_objects.get(doomed).is_none());
        assert!(restored.terrain_objects.get(doomed).is_none());
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
            true,
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
            true,
            (crate::sim::combat::RAD_NO_ATTACKER, None, warhead_ref),
            crate::sim::world::FrameEffects::default(),
        );
        assert_eq!(sim.terrain_objects.id_at((2, 2)), None);
        assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(2, 2), 0);
        sim.advance_tick(&[], Some(&rules), None, Some(&registry), 67);
        assert!(sim.terrain_objects.objects().is_empty());
    }

    fn retail_terrain_foundation_scene(
        template: ResolvedTerrainGrid,
    ) -> Option<(
        crate::sim::world::Simulation,
        RuleSet,
        OverlayTypeRegistry,
        ResolvedTerrainGrid,
    )> {
        use crate::rules::process_owner::NativeScenarioRulesPrefix;
        use crate::rules::retail_sources::RetailRulesSources;

        let (_, startup_assets) = crate::rules::retail_ini_fixture::retail_assets()?;
        let mut sources = RetailRulesSources::select(&startup_assets).unwrap();
        sources
            .artmd
            .ini
            .merge(&IniFile::from_str("[TREE01]\nFoundation=2x2\n"));
        let audio = crate::rules::audio_sources::AudioDefinitions::select(&startup_assets);
        let mut owner = sources
            .into_owner(audio.sounds().clone(), audio.eva().clone())
            .unwrap();
        let (root, mut assets) = crate::rules::retail_ini_fixture::retail_scenario_assets()?;
        let map =
            crate::map::source::load_map_by_name_or_path_with_assets(&root, "XMP03T4.MAP", &assets)
                .unwrap();
        let theater = crate::map::theater::load_theater(&mut assets, "TEMPERATE").unwrap();
        let mode =
            crate::rules::retail_sources::select_mode_ini(&assets, Some("MPBattleMD.INI")).unwrap();
        let (mut rules, _, _, _) = owner
            .load_scenario(
                NativeScenarioRulesPrefix::NonCampaign(mode.as_ref()),
                &map.map.ini,
            )
            .unwrap()
            .into_parts();
        rules.bind_terrain_assets(
            &assets,
            theater.extension,
            crate::rules::terrain_asset_catalog::TerrainAssetLoadPhase::Fresh,
        );
        let registry = rules.overlay_registry().clone();
        let mut sim = crate::sim::world::Simulation::with_seed(31);
        sim.session_mut_for_test()
            .set_map_width_for_test(template.width());
        sim.session_mut_for_test()
            .set_map_height_for_test(template.height());
        // This fixture starts at the real Terrain constructor phase; the
        // normal app launch captures exercise the complete Scenario prefix.
        sim.native_unique_ids =
            Some(crate::sim::native_identity::NativeUniqueIdCursor::for_synthetic_simulation());
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        sim.overlay_grid = Some(OverlayGrid::new(template.width(), template.height()));
        sim.install_resolved_terrain_for_new_map(template.clone());
        Some((sim, rules, registry, template))
    }

    #[test]
    fn terrain_foundation_cells_receive_nonanchor_damage_and_leave_through_common_cleanup() {
        use crate::map::overlay::{OverlayEntry, TerrainObject};
        use crate::sim::combat::combat_aoe::AreaDamageReceiver;
        use crate::sim::combat::world_receiver::{apply_area_damage, collect_area};
        use crate::sim::world::FrameEffects;

        let Some((mut sim, rules, registry, _)) =
            retail_terrain_foundation_scene(resolved_grid(8, 8))
        else {
            return;
        };
        let ore = registry.id_for_name("TIB01").unwrap();
        sim.overlay_grid = Some(OverlayGrid::from_overlay_entries(
            &[
                OverlayEntry {
                    rx: 2,
                    ry: 2,
                    overlay_id: ore,
                    frame: 7,
                },
                OverlayEntry {
                    rx: 3,
                    ry: 2,
                    overlay_id: ore,
                    frame: 7,
                },
            ],
            8,
            8,
        ));
        let before_rng = sim.scenario_rng.logical_state();
        let before_cursor = sim.native_identity_cursor().unwrap();
        construct_terrain_objects_with_rules(
            &mut sim,
            &[TerrainObject {
                rx: 2,
                ry: 2,
                name: "TREE01".into(),
            }],
            &mut TerrainConstructionRules::Projected(&rules),
            false,
            TerrainConstructionMode::Authored(&registry),
        )
        .unwrap();
        let id = sim.terrain_objects.id_at((2, 2)).unwrap();
        let cells = [(2, 2), (3, 2), (2, 3), (3, 3)];
        // TerrainMark71BFF8 -> PlaceDown5683C0 visits selector3's four
        // initialized offsets71D69F..71D734, with one shared Object+30.
        assert_eq!(sim.terrain_objects.cell_index().len(), 4);
        assert_eq!(sim.scenario_rng.logical_state(), before_rng);
        assert_eq!(
            sim.native_identity_cursor(),
            Some(before_cursor.wrapping_add(1))
        );
        assert!(sim.rebuild_dynamic_navigation(&rules));
        for cell in cells {
            assert_eq!(sim.terrain_objects.id_at(cell), Some(id));
            assert!(
                !sim.path_grid_snapshot()
                    .unwrap()
                    .is_walkable(cell.0, cell.1)
            );
        }
        assert_ne!(
            sim.substrate.raw_cell_occupation.ground_bits(2, 2) & 0x1C,
            0
        );
        assert_eq!(
            sim.substrate.raw_cell_occupation.ground_bits(3, 2) & 0x1C,
            0
        );
        assert_eq!(
            sim.overlay_grid.as_ref().unwrap().cell(2, 2).overlay_id,
            None
        );
        assert_eq!(
            sim.overlay_grid.as_ref().unwrap().cell(3, 2).overlay_id,
            Some(ore)
        );

        let he = rules.warhead("HE").unwrap();
        let he_ref = sim.interner.intern("HE");
        let origin = (crate::sim::combat::RAD_NO_ATTACKER, None, he_ref);
        let collected = collect_area(
            &mut sim,
            &rules,
            Some(&registry),
            (2, 2),
            10,
            he,
            true,
            origin,
            None,
            0,
            crate::sim::world::FrameEffects::default(),
        );
        let records: Vec<_> = collected
            .receivers
            .iter()
            .filter_map(|receiver| match receiver {
                AreaDamageReceiver::Terrain(event) => Some(*event),
                _ => None,
            })
            .collect();
        assert_eq!(
            records.len(),
            4,
            "AreaDamage489280 retains every visited Cell record"
        );
        let secondary = *records
            .iter()
            .find(|event| (event.rx, event.ry) == (3, 2))
            .unwrap();
        sim.commit_noncombat_aoe_receivers(
            &rules,
            Some(&registry),
            &collected.receivers,
            FrameEffects::default(),
        );
        let health = sim.terrain_objects.get(id).unwrap().health();
        assert!(health > 0 && health < sim.terrain_objects.get(id).unwrap().max_health());
        // TakeDamage71B920 has no Cell argument: this actual collected
        // non-anchor record must reach the same retained receiver.
        sim.commit_direct_terrain_damage_receiver(
            &rules,
            Some(&registry),
            secondary,
            false,
            FrameEffects::default(),
        );
        assert!(sim.terrain_objects.get(id).unwrap().health() < health);
        let impact = sim.terrain_objects.get(id).unwrap().world_coord();
        apply_area_damage(
            &mut sim,
            &rules,
            Some(&registry),
            impact,
            10000,
            he,
            true,
            origin,
            FrameEffects::default(),
        );
        for cell in cells {
            assert_eq!(sim.terrain_objects.id_at(cell), None);
            assert!(
                sim.path_grid_snapshot()
                    .unwrap()
                    .is_walkable(cell.0, cell.1)
            );
        }
        assert_eq!(
            sim.substrate.raw_cell_occupation.ground_bits(2, 2) & 0x1C,
            0
        );
        assert!(sim.substrate.pending_delete.contains_object(&id));
        sim.process_pending_delete_with(&rules, Some(&registry), FrameEffects::default(), 1, true);
        assert!(sim.terrain_objects.get(id).is_none());
    }

    #[test]
    fn terrain_foundation_cells_restore_shared_live_heads_and_pick_up_in_native_order() {
        use crate::map::overlay::TerrainObject;
        use crate::sim::snapshot::GameSnapshot;
        use crate::sim::world::UninitContext;

        let Some((mut sim, rules, registry, template)) =
            retail_terrain_foundation_scene(resolved_grid(8, 8))
        else {
            return;
        };
        construct_terrain_objects_with_rules(
            &mut sim,
            &[
                TerrainObject {
                    rx: 2,
                    ry: 2,
                    name: "TREE01".into(),
                },
                TerrainObject {
                    rx: 1,
                    ry: 1,
                    name: "TREE01".into(),
                },
            ],
            &mut TerrainConstructionRules::Projected(&rules),
            false,
            TerrainConstructionMode::Authored(&registry),
        )
        .unwrap();
        let ids: Vec<_> = sim.terrain_objects.objects().keys().copied().collect();
        let (older, newer) = (ids[0], ids[1]);
        // The last new recipient2,2 overwrites the ONE Next with older.
        // Therefore1,1 exposes older despite lying outside its own list.
        assert_eq!(
            sim.terrain_objects.members_at((1, 1)).collect::<Vec<_>>(),
            [newer, older]
        );
        assert_eq!(
            sim.terrain_objects.members_at((3, 3)).collect::<Vec<_>>(),
            [older]
        );
        let saved_heads = sim.terrain_objects.cell_index().clone();
        let bytes =
            GameSnapshot::save_validated(&sim, 0, rules.simulation_config_hash(), "Terrain2x2", 0);
        let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
        restored.restore_after_snapshot_load().unwrap();
        restored.rebuild_caches_after_load(template, Default::default(), &rules);
        restored
            .restore_map_authority_after_snapshot_load(&rules, &registry, None)
            .unwrap();
        assert_eq!(restored.terrain_objects.cell_index(), &saved_heads);
        assert_eq!(
            restored
                .terrain_objects
                .members_at((1, 1))
                .collect::<Vec<_>>(),
            [newer, older]
        );
        assert!(
            restored
                .resolved_terrain
                .as_ref()
                .unwrap()
                .cell(2, 1)
                .unwrap()
                .terrain_object_occupation
                .is_some()
        );
        restored.uninit_with_context(newer, UninitContext::new(&rules, Some(&registry)));
        // RemoveContent47EAF0 clears shared Next only when found: the first
        // recipient keeps older, the later heads receive the now-null Next.
        assert_eq!(restored.terrain_objects.id_at((1, 1)), Some(older));
        for cell in [(2, 1), (1, 2), (2, 2)] {
            assert_eq!(restored.terrain_objects.id_at(cell), None);
        }
        assert_eq!(restored.terrain_objects.id_at((3, 3)), Some(older));
        assert!(restored.terrain_objects.get(older).unwrap().is_live());
        restored.process_pending_delete_with(&rules, Some(&registry), Default::default(), 1, true);
        assert!(restored.terrain_objects.get(newer).is_none());
        // Keep the exposed older object live; later native destruction of a
        // foreign direct head is a separate dangling-pointer boundary.
        let bytes = GameSnapshot::save_validated(
            &restored,
            0,
            rules.simulation_config_hash(),
            "Terrain2x2 survivor",
            0,
        );
        GameSnapshot::load(&bytes)
            .unwrap()
            .sim
            .restore_after_snapshot_load()
            .unwrap();
    }

    #[test]
    fn terrain_foundation_cells_mark_dummy_source_for_allocated_secondary_recipients() {
        use crate::map::overlay::TerrainObject;
        use crate::sim::movement::locomotor::MovementLayer;
        use crate::sim::occupancy::RawCellKey;
        use crate::sim::world::UninitContext;

        let mut template = resolved_grid(4, 4);
        template.test_set_native_allocated_cells(&[(2, 1), (1, 2), (2, 2)]);
        assert!(template.native_fixed_cell_index(1, 1).is_none());
        assert!(
            [(2, 1), (1, 2), (2, 2)]
                .into_iter()
                .all(|(x, y)| template.native_fixed_cell_index(x, y).is_some())
        );
        let Some((mut sim, rules, registry, _)) = retail_terrain_foundation_scene(template) else {
            return;
        };
        let infantry_owner = sim.interner.intern("Americans");
        sim.substrate.raw_cell_occupation.write_occupant(
            RawCellKey::Dummy,
            MovementLayer::Ground,
            0x03,
            Some(infantry_owner),
            true,
        );
        construct_terrain_objects_with_rules(
            &mut sim,
            &[TerrainObject {
                rx: 1,
                ry: 1,
                name: "TREE01".into(),
            }],
            &mut TerrainConstructionRules::Projected(&rules),
            false,
            TerrainConstructionMode::Authored(&registry),
        )
        .unwrap();
        let id = *sim.terrain_objects.objects().keys().next().unwrap();
        // PlaceDown568471 checks each allocation; CellAddContent47EA52
        // still passes retained Object.Location to TerrainMarkOccupation.
        assert_eq!(sim.terrain_objects.cell_index().len(), 3);
        assert_eq!(sim.terrain_objects.id_at((1, 1)), None);
        assert_ne!(
            sim.substrate
                .raw_cell_occupation
                .bits_at(RawCellKey::Dummy, MovementLayer::Ground)
                & 0x1C,
            0
        );
        assert_eq!(
            sim.substrate
                .raw_cell_occupation
                .occupant_at(RawCellKey::Dummy, MovementLayer::Ground)
                .1,
            Some(infantry_owner)
        );
        assert_eq!(
            sim.substrate.raw_cell_occupation.ground_bits(1, 1) & 0x1C,
            0
        );
        assert!(sim.rebuild_dynamic_navigation(&rules));
        sim.uninit_with_context(id, UninitContext::new(&rules, Some(&registry)));
        assert!(sim.terrain_objects.cell_index().is_empty());
        assert_eq!(
            sim.substrate
                .raw_cell_occupation
                .occupant_at(RawCellKey::Dummy, MovementLayer::Ground),
            (0x03, Some(infantry_owner))
        );
        assert_eq!(
            sim.substrate
                .raw_cell_occupation
                .bits_at(RawCellKey::Dummy, MovementLayer::Ground)
                & 0x1C,
            0
        );
        for cell in [(2, 1), (1, 2), (2, 2)] {
            assert!(
                sim.path_grid_snapshot()
                    .unwrap()
                    .is_walkable(cell.0, cell.1)
            );
        }
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
        let terrain_objects = parse_terrain_objects(&map_ini, 4);
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
        let objects = crate::map::overlay::parse_terrain_objects(
            &IniFile::from_str("[Terrain]\n2002=TREE01\n"),
            4,
        );
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
                construct_terrain_objects_with_rules(
                    &mut sim,
                    &objects,
                    &mut TerrainConstructionRules::Projected(&rules),
                    snow,
                    TerrainConstructionMode::Authored(&registry),
                )
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
                true,
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
    fn retail_zero_cell_terrain_retains_limbo_identity_through_restore_and_cleanup() {
        use crate::map::overlay::OverlayEntry;
        use crate::sim::snapshot::GameSnapshot;
        use crate::sim::world::{LifecycleTestEvent, Simulation, UninitContext};

        let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let rules = battle.rules;
        let registry =
            OverlayTypeRegistry::from_ini(&battle.processed_rules, Some(&battle.fixed_art));
        let ore = registry.id_for_name("TIB01").unwrap();
        let objects = crate::map::overlay::parse_terrain_objects(
            &IniFile::from_str("[Terrain]\n0=TIBTRE01\n2002=TREE01\n"),
            4,
        );
        let mut sim = Simulation::with_seed(31);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let mut grid = resolved_grid(4, 4);
        // Resize565C10 never allocates cell0 in a legal authored map.
        grid.test_set_native_allocated_cells(&[(2, 2)]);
        sim.install_resolved_terrain_for_new_map(grid.clone());
        sim.overlay_grid = Some(OverlayGrid::from_overlay_entries(
            &[OverlayEntry {
                rx: 0,
                ry: 0,
                overlay_id: ore,
                frame: 7,
            }],
            4,
            4,
        ));
        sim.substrate.raw_cell_occupation.mark_ground(0, 0, 0xE0);
        sim.native_unique_ids = Some(
            crate::sim::native_identity::build_noncampaign_fresh_id_prefix(0, 0, 0, 0, 0, 0, 1, 1)
                .into_cursor(),
        );
        let before_id = sim.native_unique_ids.as_ref().unwrap().current_raw();
        let rng = sim.scenario_rng.native_state_hex();
        assert_eq!(
            construct_terrain_objects_with_rules(
                &mut sim,
                &objects,
                &mut TerrainConstructionRules::Projected(&rules),
                false,
                TerrainConstructionMode::Authored(&registry),
            )
            .unwrap(),
            2
        );
        let zero = sim
            .terrain_objects
            .objects()
            .values()
            .find(|object| object.cell() == (0, 0))
            .unwrap();
        let id = zero.stable_id();
        assert_eq!(
            zero.world_coord(),
            crate::sim::projectile::ProjectileCoord::new(0, 0, 0)
        );
        assert_eq!(
            zero.native_unique_id(),
            Some(before_id.wrapping_add(1) as i32)
        );
        assert!(zero.object_alive() && !zero.is_live() && !zero.in_logic_vector());
        assert_eq!(
            zero.health(),
            rules
                .terrain_object_type_case_insensitive("TIBTRE01")
                .unwrap()
                .strength
        );
        assert_eq!(sim.terrain_objects.cell_index().len(), 1);
        assert!(sim.terrain_objects.id_at((0, 0)).is_none());
        assert!(sim.substrate.display.layer_of(id).is_none());
        seed_terrain_spawner_animation(
            &sim.interner,
            &mut sim.terrain_objects,
            &sim.session,
            &rules,
        );
        assert!(sim.terrain_objects.animations().is_empty());
        assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(0, 0), 0xE0);
        assert_eq!(
            sim.overlay_grid.as_ref().unwrap().cell(0, 0).overlay_id,
            Some(ore)
        );
        assert_eq!(sim.scenario_rng.native_state_hex(), rng);

        let bytes = GameSnapshot::save(&sim, 0, 0, "zero terrain", 0);
        let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
        restored.restore_after_snapshot_load().unwrap();
        restored.rebuild_caches_after_load(grid.clone(), Default::default(), &rules);
        assert!(restored.rebuild_dynamic_navigation(&rules));
        // Bare simulations have other load-time timer resets; compare the
        // retained Terrain owner rather than that unrelated whole-world hash.
        assert_eq!(
            restored.terrain_objects.objects(),
            sim.terrain_objects.objects()
        );
        assert!(restored.terrain_objects.get(id).unwrap().object_alive());
        let mut initializing = GameSnapshot::load(&bytes).unwrap().sim;
        initializing.restore_after_snapshot_load().unwrap();
        initializing.rebuild_caches_after_load(grid.clone(), Default::default(), &rules);
        assert!(initializing.rebuild_dynamic_navigation(&rules));
        let path = initializing.path_grid_snapshot().unwrap();
        initializing.with_object_placement_scope(|sim| {
            sim.uninit_with_context(id, UninitContext::with_rules(&rules));
        });
        assert!(initializing.dirty_cells.radar().is_empty());
        assert!(std::sync::Arc::ptr_eq(
            &path,
            &initializing.path_grid_snapshot().unwrap()
        ));
        restored.clear_lifecycle_test_events_for_test();
        restored.dirty_cells.clear_radar_for_test();
        restored
            .resolved_terrain
            .as_ref()
            .unwrap()
            .native_cell_identity((9, 9));
        let path = restored.path_grid_snapshot().unwrap();
        restored.uninit_with_context(id, UninitContext::with_rules(&rules));
        assert!(std::sync::Arc::ptr_eq(
            &path,
            &restored.path_grid_snapshot().unwrap()
        ));
        let dead = restored.terrain_objects.get(id).unwrap();
        assert!(!dead.object_alive());
        assert_eq!(
            dead.world_coord(),
            crate::sim::projectile::ProjectileCoord::new(0, 0, 0)
        );
        assert_eq!(
            restored.substrate.raw_cell_occupation.ground_bits(0, 0),
            0xE0
        );
        assert_eq!(restored.lifecycle_test_events_for_test().iter()
            .filter(|event| matches!(event, LifecycleTestEvent::TerrainExpiryBoundary { stable_id, .. } if *stable_id == id)).count(), 1);
        assert_eq!(restored.dirty_cells.radar(), vec![(0, 0)]);
        assert_eq!(
            restored
                .resolved_terrain
                .as_ref()
                .unwrap()
                .shared_cell_dummy()
                .snapshot()
                .coord,
            (0, 0)
        );
        let bytes = GameSnapshot::save(&restored, 0, 0, "dead zero terrain", 0);
        let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
        restored.restore_after_snapshot_load().unwrap();
        restored.rebuild_caches_after_load(grid, Default::default(), &rules);
        assert!(restored.rebuild_dynamic_navigation(&rules));
        assert_eq!(
            restored.terrain_objects.get(id).unwrap().world_coord(),
            crate::sim::projectile::ProjectileCoord::new(0, 0, 0)
        );
        restored.dirty_cells.clear_radar_for_test();
        let path = restored.path_grid_snapshot().unwrap();
        restored.process_pending_delete_with(&rules, Some(&registry), Default::default(), 1, true);
        assert!(std::sync::Arc::ptr_eq(
            &path,
            &restored.path_grid_snapshot().unwrap()
        ));
        assert!(restored.terrain_objects.get(id).is_none());
        assert_eq!(restored.terrain_objects.cell_index().len(), 1);
        assert_eq!(restored.dirty_cells.radar(), vec![(0, 0)]);
    }

    #[test]
    fn unknown_terrain_restores_type_binding_without_rebuilding_cell_membership_before_pickup() {
        use crate::rules::native_processing::ProcessedRulesLayers;
        use crate::rules::process_owner::NativeScenarioRulesPrefix;
        use crate::sim::native_identity::NativeUniqueIdCursor;
        use crate::sim::snapshot::GameSnapshot;
        use crate::sim::world::{Simulation, UninitContext};

        let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let mut owner = crate::rules::retail_ini_fixture::retail_rules_owner(&assets);
        let map = IniFile::from_str(
            "[Map]\nTheater=TEMPERATE\n[Terrain]\n2002=sMoKeY\n02002=TREE01\n\
             [sMoKeY]\nStrength=9000\n",
        );
        let mode =
            crate::rules::retail_sources::select_mode_ini(&assets, Some("MPBattleMD.INI")).unwrap();
        let (mut rules, _, _, _) = owner
            .load_scenario(NativeScenarioRulesPrefix::NonCampaign(mode.as_ref()), &map)
            .unwrap()
            .into_parts();
        let overlays = rules.overlay_registry().clone();
        let mut sim = Simulation::with_seed(31);
        sim.session_mut_for_test().set_map_width_for_test(4);
        sim.session_mut_for_test().set_map_height_for_test(4);
        // This phase fixture deliberately omits the full Scenario prefix;
        // the headless regression exercises it through actual retail loading.
        sim.native_unique_ids = Some(NativeUniqueIdCursor::for_synthetic_simulation());
        let grid = resolved_grid(4, 4);
        sim.install_resolved_terrain_for_new_map(grid.clone());
        sim.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(4, 4));
        let americans = sim.interner.intern("Americans");
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                americans, 0, None, true, 0, 10,
            ));
        construct_terrain_objects_with_rules(
            &mut sim,
            &crate::map::overlay::parse_terrain_objects(&map, 4),
            &mut TerrainConstructionRules::Authored {
                rules: &mut rules,
                owner: &mut owner,
            },
            false,
            TerrainConstructionMode::Authored(&overlays),
        )
        .unwrap();
        let unknown = sim
            .terrain_objects
            .objects()
            .values()
            .find(|object| sim.interner.resolve(object.type_ref()) == "sMoKeY")
            .unwrap();
        let unknown_id = unknown.stable_id();
        let native_id = unknown.native_unique_id();
        let known_id = sim.terrain_objects.id_at((2, 2)).unwrap();
        let known_raw_bits = sim.substrate.raw_cell_occupation.ground_bits(2, 2) & 0x1C;
        assert_ne!(
            known_raw_bits, 0,
            "retail known member must mark occupation"
        );
        let cursor = sim.native_identity_cursor();
        let bytes = GameSnapshot::save_validated(
            &sim,
            0,
            rules.simulation_config_hash(),
            "unknown Terrain",
            0,
        );

        // Older self-describing processed records lack the new cache field.
        // Their actual read history must preserve TREE01's bound footprint
        // while leaving the constructor-only member null.
        let mut legacy = serde_json::to_value(owner.saved_scenario_rules().unwrap()).unwrap();
        let members =
            legacy["native_type_construction_trace"]["registry_state"]["families"]["Terrain"]
                .as_array_mut()
                .unwrap();
        for name in ["TREE01", "sMoKeY"] {
            let member = members
                .iter_mut()
                .find(|member| member["native_stored_id"] == name)
                .unwrap();
            member["terrain_type_read_state"]
                .as_object_mut()
                .unwrap()
                .remove("occupy_list_bound");
        }
        let mut processed: ProcessedRulesLayers = serde_json::from_value(legacy).unwrap();
        let before_load = owner.project_saved_scenario_rules(&processed).unwrap();
        assert!(
            before_load
                .terrain_object_type_case_insensitive("TREE01")
                .unwrap()
                .has_occupy_list()
        );
        assert!(
            !before_load
                .terrain_object_type_case_insensitive("SMOKEY")
                .unwrap()
                .has_occupy_list()
        );

        // Fresh MapPickUp56884C sees the null list and cannot unmark the
        // older Cell member. TerrainLimbo's independent suffix still runs.
        sim.uninit_with_context(unknown_id, UninitContext::with_rules(&rules));
        assert_eq!(
            sim.substrate.raw_cell_occupation.ground_bits(2, 2) & 0x1C,
            known_raw_bits
        );
        assert_eq!(sim.terrain_objects.id_at((2, 2)), Some(known_id));

        // TypeLoad71E22D binds table0 (initialized by71D580), while CellLoad
        //483B9F keeps its saved FirstObject. RemoveContent47EAF7 then unmarks
        // even though this restored unknown is absent from that Cell chain.
        processed.restore_object_image_loads("TEMPERATE").unwrap();
        let loaded_rules = owner.project_saved_scenario_rules(&processed).unwrap();
        let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
        restored.restore_after_snapshot_load().unwrap();
        restored.rebuild_caches_after_load(grid, Default::default(), &loaded_rules);
        restored
            .restore_map_authority_after_snapshot_load(&loaded_rules, &overlays, None)
            .unwrap();
        restored.resolve_type_handles(&loaded_rules);
        assert_eq!(
            restored
                .terrain_objects
                .members_at((2, 2))
                .collect::<Vec<_>>(),
            [known_id]
        );
        assert_eq!(
            restored
                .terrain_objects
                .get(unknown_id)
                .unwrap()
                .native_unique_id(),
            native_id
        );
        restored.uninit_with_context(unknown_id, UninitContext::with_rules(&loaded_rules));
        assert_eq!(
            restored.substrate.raw_cell_occupation.ground_bits(2, 2) & 0x1C,
            0
        );
        assert_eq!(restored.terrain_objects.id_at((2, 2)), Some(known_id));
        restored.process_pending_delete_with(
            &loaded_rules,
            Some(&overlays),
            Default::default(),
            1,
            true,
        );
        assert!(restored.terrain_objects.get(unknown_id).is_none());
        assert!(restored.terrain_objects.get(known_id).unwrap().is_live());
        assert_eq!(
            restored.native_identity_cursor(),
            cursor,
            "retirement refunds no identity"
        );
        let unit = restored
            .spawn_object_with_overlay_registry(
                "MTNK",
                "Americans",
                1,
                2,
                0,
                &loaded_rules,
                &overlays,
            )
            .expect("later ordinary constructor");
        assert_eq!(
            restored.entities().get(unit).unwrap().native_unique_id(),
            cursor.unwrap().wrapping_add(1) as i32
        );
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

        let constructed = construct_terrain_objects_with_rules(
            &mut sim,
            &[TerrainObject {
                rx: 2,
                ry: 2,
                name: "TERR0".to_string(),
            }],
            &mut TerrainConstructionRules::Projected(&rules),
            false,
            TerrainConstructionMode::Authored(&overlays),
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
    /// Phobos `TerrainClass_TakeDamage_NowDead_Add` (`0x71BB2C`) and
    /// `TerrainClass_AI_Crumbling` (`0x71C812`): a `HasCrumblingFrames=`
    /// tree stays alive, plays `CrumblingSound` and its crumble Stage, then
    /// plays `PlayDestroyEffects` and is removed.
    #[test]
    fn phobos_crumbling_tree_keeps_alive_then_destroys_and_removes() {
        use crate::sim::world::SimSoundEvent;

        let ini = IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
             [TerrainTypes]\n0=TREE\n\
             [TREE]\nStrength=100\nIsAnimated=no\nSpawnsTiberium=no\n",
        );
        let mut rules = RuleSet::from_ini(&ini).expect("crumble rules");
        let mut terrain_type = rules
            .terrain_object_type_case_insensitive("TREE")
            .expect("tree type")
            .clone();
        terrain_type.has_crumbling_frames = true;
        terrain_type.crumbling_sound = Some("TreeCrumble".to_string());
        terrain_type.destroy_sound = Some("TreeDestroy".to_string());
        // 8 SHP frames with AnimationLength 4: the crumble completes at stage 3.
        terrain_type.animation_length = Some(4);
        rules.install_map_terrain_type(terrain_type);
        rules.set_terrain_spawner_frame_count_for_test("TREE", 8);

        let mut sim = crate::sim::world::Simulation::new();
        sim.install_resolved_terrain_for_new_map(resolved_grid(8, 8));
        assert_eq!(
            construct_terrain_objects(
                &mut sim,
                &[crate::map::overlay::TerrainObject {
                    rx: 4,
                    ry: 4,
                    name: "TREE".to_string(),
                }],
                &rules,
                false,
            ),
            1
        );
        let id = sim.terrain_objects.id_at((4, 4)).expect("tree");
        sim.terrain_start_crumble(id, &rules);
        assert!(sim.terrain_objects.is_crumbling(id));
        assert!(
            sim.sound_events.iter().any(|event| matches!(
                event,
                SimSoundEvent::VocAt { sound_id, .. } if sound_id == "TreeCrumble"
            )),
            "CrumblingSound plays once at the crumble start"
        );

        let mut removed_at = None;
        for frame in 0..12 {
            sim.session.set_binary_frame_for_test(frame);
            tick_terrain_object_ai(&mut sim, id, &rules, None, FrameEffects::default())
                .expect("terrain AI");
            if sim
                .terrain_objects
                .objects()
                .get(&id)
                .is_some_and(|terrain| !terrain.is_live())
            {
                removed_at = Some(frame);
                break;
            }
        }
        // The stock Stage step runs before the 0x71C812 fold, so the value
        // reaches 3 at frame 6 and removes on that same visit.
        assert_eq!(removed_at, Some(6), "the crumble sequence removes the tree");
        assert!(
            sim.sound_events.iter().any(|event| matches!(
                event,
                SimSoundEvent::VocAt { sound_id, .. } if sound_id == "TreeDestroy"
            )),
            "PlayDestroyEffects plays DestroySound once"
        );
    }

    /// The production area receiver (`commit_terrain`) routes a lethal hit:
    /// a `HasCrumblingFrames` tree stays live and crumbling (and leaves the
    /// transaction's finalizing set, which `ReceiverRun::finish` expects
    /// empty), while a plain tree finalizes in the same call. Under Ares an
    /// on-fire tree skips Start_To_Crumble (`0x71BADC`).
    #[test]
    fn a_lethal_area_hit_routes_crumbling_and_plain_trees_through_the_receiver() {
        let ini = IniFile::from_str(
            "[InfantryTypes]
[VehicleTypes]
[AircraftTypes]
[BuildingTypes]
             [TerrainTypes]
0=TREE
1=PLAIN
             [Warheads]
0=WH
             [TREE]
Strength=100
IsAnimated=no
SpawnsTiberium=no
             [PLAIN]
Strength=100
IsAnimated=no
SpawnsTiberium=no
             [WH]
Wood=yes
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
",
        );
        let mut rules = RuleSet::from_ini(&ini).expect("receiver rules");
        let mut crumbling = rules
            .terrain_object_type_case_insensitive("TREE")
            .expect("tree type")
            .clone();
        crumbling.has_crumbling_frames = true;
        crumbling.crumbling_sound = Some("TreeCrumble".to_string());
        crumbling.destroy_sound = Some("TreeDestroy".to_string());
        crumbling.animation_length = Some(4);
        rules.install_map_terrain_type(crumbling);
        rules.set_terrain_spawner_frame_count_for_test("TREE", 8);

        let mut sim = crate::sim::world::Simulation::new();
        sim.install_resolved_terrain_for_new_map(resolved_grid(8, 8));
        construct_terrain_objects(
            &mut sim,
            &[
                crate::map::overlay::TerrainObject {
                    rx: 4,
                    ry: 4,
                    name: "TREE".to_string(),
                },
                crate::map::overlay::TerrainObject {
                    rx: 6,
                    ry: 4,
                    name: "PLAIN".to_string(),
                },
            ],
            &rules,
            false,
        );
        let tree = sim.terrain_objects.id_at((4, 4)).expect("tree");
        let plain = sim.terrain_objects.id_at((6, 4)).expect("plain");
        sim.resolve_type_handles(&rules);
        let warhead = rules.warhead("WH").expect("warhead").clone();
        let warhead_ref = sim.interner.intern("WH");
        let live = |sim: &crate::sim::world::Simulation, id: u64| {
            sim.terrain_objects
                .objects()
                .get(&id)
                .is_some_and(TerrainObjectState::is_live)
        };
        for (id, rx) in [(tree, 4), (plain, 6)] {
            crate::sim::combat::world_receiver::apply_area_damage(
                &mut sim,
                &rules,
                None,
                crate::sim::projectile::ProjectileCoord::new(
                    i32::from(rx) * 256 + 128,
                    4 * 256 + 128,
                    0,
                ),
                1000,
                &warhead,
                false,
                (crate::sim::combat::RAD_NO_ATTACKER, None, warhead_ref),
                FrameEffects::default(),
            );
            if id == tree {
                assert!(live(&sim, tree), "the crumbling tree survives the hit");
                assert!(sim.terrain_objects.is_crumbling(tree));
            } else {
                assert!(!live(&sim, plain), "the plain tree finalizes in the call");
            }
        }

        {
            construct_terrain_objects(
                &mut sim,
                &[crate::map::overlay::TerrainObject {
                    rx: 2,
                    ry: 4,
                    name: "TREE".to_string(),
                }],
                &rules,
                false,
            );
            let burning = sim.terrain_objects.id_at((2, 4)).expect("burning tree");
            assert!(sim.terrain_objects.set_on_fire(burning, true));
            crate::sim::combat::world_receiver::apply_area_damage(
                &mut sim,
                &rules,
                None,
                crate::sim::projectile::ProjectileCoord::new(2 * 256 + 128, 4 * 256 + 128, 0),
                1000,
                &warhead,
                false,
                (crate::sim::combat::RAD_NO_ATTACKER, None, warhead_ref),
                FrameEffects::default(),
            );
            assert!(
                !live(&sim, burning) && !sim.terrain_objects.is_crumbling(burning),
                "0x71BADC skips Start_To_Crumble for an on-fire tree"
            );
        }
    }

    /// `HasDamagedFrames=` above zero health clears the damaged-redraw latch
    /// (`0x71C812`) without removing the tree.
    #[test]
    fn phobos_has_damaged_frames_clears_the_redraw_latch_while_alive() {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
             [TerrainTypes]\n0=TREE\n\
             [TREE]\nStrength=100\nIsAnimated=no\nSpawnsTiberium=no\n",
        );
        let mut rules = RuleSet::from_ini(&ini).expect("damaged rules");
        let mut terrain_type = rules
            .terrain_object_type_case_insensitive("TREE")
            .expect("tree type")
            .clone();
        terrain_type.has_damaged_frames = true;
        rules.install_map_terrain_type(terrain_type);

        let mut sim = crate::sim::world::Simulation::new();
        sim.install_resolved_terrain_for_new_map(resolved_grid(8, 8));
        construct_terrain_objects(
            &mut sim,
            &[crate::map::overlay::TerrainObject {
                rx: 4,
                ry: 4,
                name: "TREE".to_string(),
            }],
            &rules,
            false,
        );
        let id = sim.terrain_objects.id_at((4, 4)).expect("tree");
        sim.terrain_objects.set_crumbling(id, true);
        sim.session.set_binary_frame_for_test(0);
        tick_terrain_object_ai(&mut sim, id, &rules, None, FrameEffects::default())
            .expect("terrain AI");
        assert!(!sim.terrain_objects.is_crumbling(id));
        assert!(
            sim.terrain_objects
                .objects()
                .get(&id)
                .is_some_and(|terrain| terrain.is_live()),
            "the tree stays on the map"
        );
    }

    fn terrain_at(rx: u16, ry: u16, name: &str) -> crate::map::overlay::TerrainObject {
        crate::map::overlay::TerrainObject {
            rx,
            ry,
            name: name.to_string(),
        }
    }

    /// Phobos `TerrainClass_TakeDamage_SetContext` (`0x71B965`) and
    /// `TerrainClass_TakeDamage_RefreshDamageFrame` (`0x71B98B`,
    /// `Ext/TerrainType/Hooks.cpp:259-283`): a non-lethal hit that carries a
    /// non-animated `HasDamagedFrames=` type across `ConditionYellow`
    /// downward latches `IsCrumbling` (the forced redraw) and joins Logic; the
    /// next `TerrainClass_AI_Crumbling` (`0x71C812`) visit clears it while
    /// Health is above zero. An upward crossing, an already-damaged ratio, an
    /// animated type and a type without damaged frames all refuse.
    #[test]
    fn a_nonlethal_hit_across_condition_yellow_latches_crumbling_for_one_frame() {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
             [AudioVisual]\nConditionYellow=0.5\n\
             [TerrainTypes]\n0=TREE\n1=ANIM\n2=PLAIN\n\
             [Warheads]\n0=WH\n\
             [TREE]\nStrength=100\nIsAnimated=no\nSpawnsTiberium=no\nHasDamagedFrames=yes\n\
             [ANIM]\nStrength=100\nIsAnimated=yes\nSpawnsTiberium=no\nHasDamagedFrames=yes\n\
             [PLAIN]\nStrength=100\nIsAnimated=no\nSpawnsTiberium=no\n\
             [WH]\nWood=yes\n\
             Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        );
        let mut rules = RuleSet::from_ini(&ini).expect("latch rules");
        rules.set_terrain_spawner_frame_count_for_test("ANIM", 8);

        let mut sim = crate::sim::world::Simulation::new();
        sim.install_resolved_terrain_for_new_map(resolved_grid(8, 8));
        construct_terrain_objects(
            &mut sim,
            &[
                terrain_at(4, 4, "TREE"),
                terrain_at(5, 4, "TREE"),
                terrain_at(6, 4, "TREE"),
                terrain_at(4, 6, "ANIM"),
                terrain_at(6, 6, "PLAIN"),
            ],
            &rules,
            false,
        );
        let id_at = |sim: &crate::sim::world::Simulation, cell| {
            sim.terrain_objects.id_at(cell).expect("constructed tree")
        };
        let crossing = id_at(&sim, (4, 4));
        let already_damaged = id_at(&sim, (5, 4));
        let stays_above = id_at(&sim, (6, 4));
        let animated = id_at(&sim, (4, 6));
        let plain = id_at(&sim, (6, 6));
        // `0x71B965` stores the pre-hit ratio; 40 is already below the line.
        sim.terrain_objects
            .objects_mut_for_test()
            .get_mut(&already_damaged)
            .expect("damaged tree")
            .set_health_for_test(40);
        assert!(
            !sim.terrain_objects.get(crossing).unwrap().in_logic_vector(),
            "a plain damaged-frames tree does not join Logic at construction"
        );

        sim.resolve_type_handles(&rules);
        let warhead = rules.warhead("WH").expect("warhead").clone();
        let warhead_ref = sim.interner.intern("WH");
        for (cell, damage) in [
            ((4, 4), 60),
            ((5, 4), 10),
            ((6, 4), 45),
            ((4, 6), 60),
            ((6, 6), 60),
        ] {
            crate::sim::combat::world_receiver::apply_area_damage(
                &mut sim,
                &rules,
                None,
                crate::sim::projectile::ProjectileCoord::new(
                    i32::from(cell.0) * 256 + 128,
                    i32::from(cell.1) * 256 + 128,
                    0,
                ),
                damage,
                &warhead,
                false,
                (crate::sim::combat::RAD_NO_ATTACKER, None, warhead_ref),
                FrameEffects::default(),
            );
        }

        assert_eq!(
            sim.terrain_objects.get(crossing).unwrap().health(),
            40,
            "100 -> 40 crosses 0.5"
        );
        assert!(sim.terrain_objects.is_crumbling(crossing));
        assert!(
            sim.terrain_objects.get(crossing).unwrap().in_logic_vector(),
            "the latch joins Logic for the forced redraw"
        );
        for (id, reason) in [
            (already_damaged, "prior 40 was already below the line"),
            (stays_above, "100 -> 55 stays above the line"),
            (animated, "an animated type never latches"),
            (plain, "a type without HasDamagedFrames never latches"),
        ] {
            assert!(
                !sim.terrain_objects.is_crumbling(id),
                "{reason}: 100 -> {}",
                sim.terrain_objects.get(id).unwrap().health()
            );
        }

        // `0x71C812`: the next AI visit clears the latch while Health > 0, and
        // a type that is neither animated nor flammable leaves Logic with it.
        sim.session.set_binary_frame_for_test(0);
        tick_terrain_object_ai(&mut sim, crossing, &rules, None, FrameEffects::default())
            .expect("terrain AI");
        assert!(!sim.terrain_objects.is_crumbling(crossing));
        assert!(
            !sim.terrain_objects.get(crossing).unwrap().in_logic_vector(),
            "the cleared latch leaves Logic"
        );
        assert_eq!(
            sim.terrain_objects.get(crossing).unwrap().health(),
            40,
            "the latch never writes Health"
        );
    }

    /// `Ext/Rules/Body.cpp` `ConditionYellow_Terrain` (`[AudioVisual]
    /// ConditionYellow.Terrain=`): the override replaces the base
    /// `ConditionYellow` for `TerrainClass_Draw_PickFrame @ 0x71C1FE`
    /// (`Ext/TerrainType/Hooks.cpp:106`) and for the `0x71B98B` latch (`:274`).
    /// One `RuleSet` owner answers both, so a ratio that crosses the override
    /// but not the base line still latches.
    #[test]
    fn condition_yellow_terrain_overrides_the_base_threshold_in_sim_and_draw() {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
             [AudioVisual]\nConditionYellow=0.5\nConditionYellow.Terrain=0.7\n\
             [TerrainTypes]\n0=TREE\n\
             [Warheads]\n0=WH\n\
             [TREE]\nStrength=100\nIsAnimated=no\nSpawnsTiberium=no\nHasDamagedFrames=yes\n\
             [WH]\nWood=yes\n\
             Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("override rules");
        assert_eq!(rules.general.condition_yellow, 0.5);
        assert_eq!(
            rules.terrain_condition_yellow(),
            f64::from(0.7f32),
            "the Phobos Parser<double> widening"
        );

        let mut sim = crate::sim::world::Simulation::new();
        sim.install_resolved_terrain_for_new_map(resolved_grid(8, 8));
        construct_terrain_objects(&mut sim, &[terrain_at(3, 3, "TREE")], &rules, false);
        let tree = sim.terrain_objects.id_at((3, 3)).expect("tree");
        sim.resolve_type_handles(&rules);
        let warhead = rules.warhead("WH").expect("warhead").clone();
        let warhead_ref = sim.interner.intern("WH");
        crate::sim::combat::world_receiver::apply_area_damage(
            &mut sim,
            &rules,
            None,
            crate::sim::projectile::ProjectileCoord::new(3 * 256 + 128, 3 * 256 + 128, 0),
            35,
            &warhead,
            false,
            (crate::sim::combat::RAD_NO_ATTACKER, None, warhead_ref),
            FrameEffects::default(),
        );
        assert_eq!(
            sim.terrain_objects.get(tree).unwrap().health(),
            65,
            "100 -> 65"
        );
        assert!(
            sim.terrain_objects.is_crumbling(tree),
            "0.65 crosses the 0.7 override but not the 0.5 base line"
        );
    }
}
