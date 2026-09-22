//! Area-of-effect (AoE) damage logic for warheads with CellSpread > 0.
//!
//! When a warhead detonates with CellSpread > 0, it damages all entities
//! within the blast radius. Damage falls off linearly from 100% at the
//! epicenter to `PercentAtMax` at the edge of the radius.
//!
//! ## Damage formula
//! ```text
//! damage_at_distance(d) = base_damage * verses[armor] * lerp(1.0, percent_at_max, d / cell_spread)
//! ```
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/, map terrain, and sim occupancy/components.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use std::collections::BTreeMap;

use super::{EntityDamageEvent, TerrainDamageEvent, cell_spread};
use crate::map::entities::EntityCategory;
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::ruleset::RuleSet;
use crate::rules::warhead_type::WarheadType;
use crate::sim::entity_store::EntityStore;
use crate::sim::intern::StringInterner;
use crate::sim::map::bridge_topology::{BRIDGE_DECK_HEIGHT_LEVELS, CellBridgeView, ListLayer};
use crate::sim::mission::authority::restore_entity_after_target_expiry;
use crate::sim::mission::concrete_effects::represented_assign_target;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::{
    OccupancyGrid, air_spatial_query_bucket_order, air_spatial_tracks_entity,
};
#[cfg(test)]
use crate::sim::overlay_grid::WallMutation;
use crate::sim::overlay_grid::{
    OverlayGrid, WallDamageTransactionHost, WallDirtyStep, WallPointerTarget, WallZoneRepairKind,
    damage_wall_overlay_with_runtime_host,
};
use crate::sim::rng::SimRng;
use crate::sim::terrain_object::{TerrainObjectLifecycle, TerrainObjectState};
#[cfg(test)]
use crate::util::fixed_math::SIM_ZERO;
use crate::util::fixed_math::SimFixed;
use crate::util::lepton::{CELL_CENTER_LEPTON, LEPTONS_PER_LEVEL, ground_height_leptons};
use crate::util::native_x87::{X87Chop53, sqrt_approx_f32};

const BUILDING_CENTER_HEIGHT_ALLOWANCE_LEPTONS: i32 = 2 * LEPTONS_PER_LEVEL as i32;

// GATE A1/A2 CUTOVER (authoritative): the bridge-AoE deck height is now the single
// gamemd-verified value sourced from `bridge_topology` — the full deck offset is
// `4 × per_level` (416 leptons = 4 Level units), so the half-deck term used by the
// object-layer selector is `4 / 2 = 2`. The layer
// boundary math lives in `CellBridgeView::aoe_object_layer` (strict `>` against
// `ground_z + half_deck`); this module routes through it so there is ONE source of
// truth for the deck height. This replaces the old `BRIDGE_AOE_SELECTOR_HEIGHT_LEVELS
// = 4` const and its per-cell `(deck_level - level).max(4)` floor.
// (Source: GATE_BRIDGE_DECK_HEIGHT_RESOLUTION_GHIDRA_REPORT.md §3/§5;
//  GATE_BRIDGE_ONBRIDGE_OCCUPANCY_RESOLUTION_GHIDRA_REPORT.md §a.)

/// Optional map context for gamemd bridge object-list selection.
#[derive(Default)]
pub(crate) struct AoELayerContext<'a> {
    pub occupancy: Option<&'a OccupancyGrid>,
    pub terrain: Option<&'a mut ResolvedTerrainGrid>,
    pub overlay_grid: Option<&'a mut OverlayGrid>,
    pub overlay_registry: Option<&'a OverlayTypeRegistry>,
    pub scenario_rng: Option<&'a mut SimRng>,
    /// Exact detonation coordinate used by Apply_area_damage receiver-distance
    /// collection. `impact_z` remains in the established whole-Level domain
    /// for ground-vs-bridge object-list selection.
    pub air_impact: Option<AoEAirImpact>,
    pub impact_z: i32,
}

/// Native per-spread-cell side effects that run before wall routing and before
/// the cell's object-list records are captured. The callback receives short
/// reborrows of the same map/RNG authorities used by the AoE transaction, so a
/// producer can preserve exact inter-cell RNG order without moving those
/// authorities into combat_aoe.
pub(crate) trait AoECellPrelude {
    #[allow(clippy::too_many_arguments)]
    fn before_cell(
        &mut self,
        rx: u16,
        ry: u16,
        overlay_grid: Option<&mut OverlayGrid>,
        overlay_registry: Option<&OverlayTypeRegistry>,
        terrain: Option<&mut ResolvedTerrainGrid>,
        scenario_rng: Option<&mut SimRng>,
        occupancy: Option<&OccupancyGrid>,
    );

    fn wall_navigation_step(
        &mut self,
        _terrain: &ResolvedTerrainGrid,
        _cell: (u16, u16),
        _navigation_changed: bool,
        _repair: WallZoneRepairKind,
    ) {
    }

    fn wall_dirty_step(&mut self, _step: WallDirtyStep, _packed_coord: (u16, u16)) {}
}

struct AoEWallDamageHost<'borrow, 'prelude> {
    entities: &'borrow mut EntityStore,
    #[cfg(test)]
    trace: &'borrow mut Vec<CellTargetDetach>,
    prelude: &'borrow mut Option<&'prelude mut dyn AoECellPrelude>,
}

impl WallDamageTransactionHost for AoEWallDamageHost<'_, '_> {
    fn dirty_step(&mut self, step: WallDirtyStep, packed_coord: (u16, u16)) {
        if let Some(prelude) = self.prelude.as_deref_mut() {
            prelude.wall_dirty_step(step, packed_coord);
        }
    }

    fn navigation_step(
        &mut self,
        terrain: &ResolvedTerrainGrid,
        cell: (u16, u16),
        navigation_changed: bool,
        repair: WallZoneRepairKind,
    ) {
        if let Some(prelude) = self.prelude.as_deref_mut() {
            prelude.wall_navigation_step(terrain, cell, navigation_changed, repair);
        }
    }

    fn pointer_expired(&mut self, target: WallPointerTarget) {
        if let WallPointerTarget::Real(rx, ry) = target {
            expire_cell_target_references(
                self.entities,
                rx,
                ry,
                #[cfg(test)]
                self.trace,
            );
        }
    }
}

pub(crate) fn tiberium_reduction_cell_admitted(
    overlay_grid: Option<&OverlayGrid>,
    overlay_registry: Option<&OverlayTypeRegistry>,
    rx: u16,
    ry: u16,
) -> bool {
    let (Some(grid), Some(registry)) = (overlay_grid, overlay_registry) else {
        return false;
    };
    grid.cell(rx, ry)
        .overlay_id
        .and_then(|overlay_id| registry.flags(overlay_id))
        .is_some_and(|flags| flags.tiberium && flags.chain_reaction)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AoEAirImpact {
    pub sub_x: SimFixed,
    pub sub_y: SimFixed,
    pub z_leptons: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AoEDamageOrigin {
    pub source_id: u64,
    pub source_house: Option<crate::sim::intern::InternedId>,
    pub warhead_ref: crate::sim::intern::InternedId,
}

impl From<(u64, crate::sim::intern::InternedId)> for AoEDamageOrigin {
    fn from((source_id, warhead_ref): (u64, crate::sim::intern::InternedId)) -> Self {
        Self {
            source_id,
            source_house: None,
            warhead_ref,
        }
    }
}

impl
    From<(
        u64,
        Option<crate::sim::intern::InternedId>,
        crate::sim::intern::InternedId,
    )> for AoEDamageOrigin
{
    fn from(
        (source_id, source_house, warhead_ref): (
            u64,
            Option<crate::sim::intern::InternedId>,
            crate::sim::intern::InternedId,
        ),
    ) -> Self {
        Self {
            source_id,
            source_house,
            warhead_ref,
        }
    }
}

#[cfg(test)]
impl From<&str> for AoEDamageOrigin {
    fn from(_legacy_owner_fixture: &str) -> Self {
        Self {
            source_id: super::RAD_NO_ATTACKER,
            source_house: None,
            warhead_ref: crate::sim::intern::InternedId::default(),
        }
    }
}

/// Lift the established whole-Level impact coordinate into native absolute
/// lepton Z while preserving the exact XY subcell sample on slopes.
pub(crate) fn air_impact_from_layer_z(
    terrain: Option<&ResolvedTerrainGrid>,
    impact_rx: u16,
    impact_ry: u16,
    sub_x: SimFixed,
    sub_y: SimFixed,
    impact_z: i32,
) -> Option<AoEAirImpact> {
    let terrain = terrain?;
    let cell = terrain.cell(impact_rx, impact_ry)?;
    let world_x = i32::from(impact_rx)
        .wrapping_mul(256)
        .wrapping_add(sub_x.to_num::<i32>());
    let world_y = i32::from(impact_ry)
        .wrapping_mul(256)
        .wrapping_add(sub_y.to_num::<i32>());
    let ground_z = ground_height_leptons(cell.level, cell.slope_type, world_x, world_y).ok()?;
    let level_delta = impact_z.wrapping_sub(i32::from(cell.level));
    Some(AoEAirImpact {
        sub_x,
        sub_y,
        z_leptons: ground_z.wrapping_add(level_delta.wrapping_mul(LEPTONS_PER_LEVEL as i32)),
    })
}

pub(crate) fn air_impact_from_entity(
    entity: &crate::sim::game_entity::GameEntity,
    terrain: Option<&ResolvedTerrainGrid>,
) -> Option<AoEAirImpact> {
    Some(AoEAirImpact {
        sub_x: entity.position.sub_x,
        sub_y: entity.position.sub_y,
        z_leptons: i32::try_from(super::in_range::effective_z_leptons(entity, terrain?)?).ok()?,
    })
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CellTargetDetach {
    pub listener_id: u64,
    pub restored: bool,
    pub cleared: bool,
}

#[derive(Debug, Default)]
pub(crate) struct AoEDamageResult {
    pub receivers: Vec<AreaDamageReceiver>,
    /// Compatibility-only projection for existing unit tests. Production
    /// callers consume `receivers`, which is the sole ordered authority.
    #[cfg(test)]
    pub hits: Vec<EntityDamageEvent>,
    #[cfg(test)]
    pub wall_mutations: Vec<WallMutation>,
    #[cfg(test)]
    pub cell_target_detaches: Vec<CellTargetDetach>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AreaDamageReceiver {
    Entity(EntityDamageEvent),
    Terrain(TerrainDamageEvent),
}

#[derive(Clone, Copy)]
pub(crate) struct TerrainCollectionView<'a> {
    pub objects: &'a BTreeMap<u64, TerrainObjectState>,
    pub cells: &'a BTreeMap<(u16, u16), u64>,
}

impl AoEDamageResult {
    fn push_entity(&mut self, event: EntityDamageEvent) {
        #[cfg(test)]
        self.hits.push(event);
        self.receivers.push(AreaDamageReceiver::Entity(event));
    }

    fn push_terrain(&mut self, event: TerrainDamageEvent) {
        self.receivers.push(AreaDamageReceiver::Terrain(event));
    }
}

fn finalize_receiver_isolation_metadata(
    mut result: AoEDamageResult,
    warhead: &WarheadType,
) -> AoEDamageResult {
    // Apply_area_damage's near-center IC isolation exists only for the native
    // binary32 CellSpread <= 0.5 transaction. Preserve every collected record
    // and wall mutation here; the ordered receiver commit owns the live-state
    // pre-scan and dispatch skip after collection has fully completed.
    if warhead.cell_spread_f64 <= 0.5 {
        for receiver in &mut result.receivers {
            match receiver {
                AreaDamageReceiver::Entity(event) => {
                    event.near_center_ic_isolation_eligible = true;
                }
                AreaDamageReceiver::Terrain(event) => {
                    event.near_center_ic_isolation_eligible = true;
                }
            }
        }
        #[cfg(test)]
        for event in &mut result.hits {
            event.near_center_ic_isolation_eligible = true;
        }
    }
    result
}

/// Build the caller-owned impact Z used by bridge-aware AoE call sites.
///
/// Generic cell-center helpers stay ground-only; verified superweapon callers
/// add the structural-bridge deck height before entering Apply_area_damage.
pub(crate) fn bridge_adjusted_impact_z(
    terrain: Option<&ResolvedTerrainGrid>,
    impact_rx: u16,
    impact_ry: u16,
) -> i32 {
    let Some(cell) = terrain.and_then(|terrain| terrain.cell(impact_rx, impact_ry)) else {
        return 0;
    };

    let mut impact_z = cell.level as i32;
    if cell.bridge_facts.has_structural_bridge() {
        // Authoritative deck offset = full deck height (4 levels), not a per-cell
        // span. Same const the layer selector below compares against, so the
        // synthesized impact Z and the layer threshold stay consistent.
        impact_z += BRIDGE_DECK_HEIGHT_LEVELS;
    }
    impact_z
}

/// Apply area-of-effect damage from a warhead detonation at a specific cell.
///
/// Returns a list of (stable_id, damage) pairs for all entities within the blast
/// radius. Friendly fire IS applied — CellSpread does not discriminate by owner,
/// matching RA2 behavior (e.g., V3 rockets can damage your own units).
///
/// `base_damage` is the weapon's raw damage value (before Verses scaling).
#[cfg(test)]
pub(crate) fn apply_aoe_damage<O: Into<AoEDamageOrigin>>(
    entities: &mut EntityStore,
    impact_rx: u16,
    impact_ry: u16,
    base_damage: i32,
    warhead: &WarheadType,
    rules: &RuleSet,
    interner: &mut StringInterner,
    origin: O,
    layer_context: AoELayerContext<'_>,
) -> AoEDamageResult {
    apply_aoe_damage_with_terrain(
        entities,
        impact_rx,
        impact_ry,
        base_damage,
        warhead,
        rules,
        interner,
        origin,
        layer_context,
        None,
    )
}

/// Test-only wrapper: resolves the rule handles itself (mirroring sim init)
/// so fixture tests need no explicit `ResolvedRuleHandles` plumbing.
#[cfg(test)]
pub(crate) fn apply_aoe_damage_with_terrain<O: Into<AoEDamageOrigin>>(
    entities: &mut EntityStore,
    impact_rx: u16,
    impact_ry: u16,
    base_damage: i32,
    warhead: &WarheadType,
    rules: &RuleSet,
    interner: &mut StringInterner,
    origin: O,
    layer_context: AoELayerContext<'_>,
    terrain_objects: Option<TerrainCollectionView<'_>>,
) -> AoEDamageResult {
    let handles = Some(crate::sim::type_handle_table::ResolvedRuleHandles::resolve(
        rules, interner,
    ));
    apply_aoe_damage_with_terrain_and_scenario(
        entities,
        impact_rx,
        impact_ry,
        base_damage,
        warhead,
        rules,
        interner,
        handles,
        origin,
        layer_context,
        terrain_objects,
        false,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_aoe_damage_with_terrain_and_scenario<O: Into<AoEDamageOrigin>>(
    entities: &mut EntityStore,
    impact_rx: u16,
    impact_ry: u16,
    base_damage: i32,
    warhead: &WarheadType,
    rules: &RuleSet,
    interner: &StringInterner,
    handles: Option<crate::sim::type_handle_table::ResolvedRuleHandles>,
    origin: O,
    mut layer_context: AoELayerContext<'_>,
    terrain_objects: Option<TerrainCollectionView<'_>>,
    scenario_no_damage: bool,
    mut cell_prelude: Option<&mut dyn AoECellPrelude>,
) -> AoEDamageResult {
    // Apply_area_damage returns at entry for signed zero. Nothing downstream
    // observes CellSpread, wall routing, object lists, target detach, or RNG.
    // Native ScenarioFlags bit 0x20 shares that transaction-wide early-out.
    if base_damage == 0 || scenario_no_damage {
        return AoEDamageResult::default();
    }
    let mut origin = origin.into();
    if origin.source_house.is_none() && origin.source_id != super::RAD_NO_ATTACKER {
        origin.source_house = entities.get(origin.source_id).map(|source| source.owner());
    }
    let ground_source_admitted = handles
        .is_some_and(|handles| handles.is_crush(origin.warhead_ref))
        || entities
            .get(origin.source_id)
            .and_then(|source| rules.object(interner.resolve(source.type_ref())))
            .is_some_and(|source_type| source_type.damage_self);

    let cell_spread: SimFixed = warhead.cell_spread;

    // gamemd receiver-list radius = ftol(CellSpread * 256). Each candidate's
    // native Sqrt_Approx/ftol distance is compared to this signed threshold;
    // there is no exact-squared prefilter.
    let spread_leptons = (warhead.cell_spread_f64 * 256.0) as i64;
    let mut result = AoEDamageResult::default();
    let exact_impact = layer_context
        .air_impact
        .or_else(|| {
            air_impact_from_layer_z(
                layer_context.terrain.as_deref(),
                impact_rx,
                impact_ry,
                CELL_CENTER_LEPTON,
                CELL_CENTER_LEPTON,
                layer_context.impact_z,
            )
        })
        .unwrap_or(AoEAirImpact {
            sub_x: CELL_CENTER_LEPTON,
            sub_y: CELL_CENTER_LEPTON,
            z_leptons: layer_context
                .impact_z
                .wrapping_mul(LEPTONS_PER_LEVEL as i32),
        });

    if layer_context.occupancy.is_some() && layer_context.terrain.is_some() {
        let occupancy = layer_context
            .occupancy
            .expect("checked occupied Apply_area_damage context");
        let selected_layer = {
            let terrain = layer_context
                .terrain
                .as_deref()
                .expect("checked terrain Apply_area_damage context");
            select_object_damage_layer(terrain, impact_rx, impact_ry, layer_context.impact_z)
        };

        // Native enters the entire airborne query only when the exact impact
        // coordinate is strictly above GetGroundHeight at that same XY. Keep
        // this gate outside bucket materialization: equality observes no air
        // vector at all.
        if let Some(air_impact) = layer_context.air_impact
            && layer_context.terrain.as_deref().is_some_and(|terrain| {
                impact_is_above_ground(terrain, impact_rx, impact_ry, air_impact)
            })
        {
            // Native appends airborne receiver records before entering the
            // spread cell loop. Damage dispatch happens only after collection,
            // so walls still mutate during the later cell walk before any
            // receiver runs.
            let (terrain_width, terrain_height) = layer_context
                .terrain
                .as_deref()
                .map(|terrain| (terrain.width(), terrain.height()))
                .expect("checked terrain Apply_area_damage context");
            for entity_id in airborne_ids_in_spatial_order(
                entities,
                impact_rx,
                impact_ry,
                cell_spread.to_num::<i32>(),
                terrain_width,
                terrain_height,
            ) {
                let terrain = layer_context
                    .terrain
                    .as_deref()
                    .expect("checked terrain Apply_area_damage context");
                push_airborne_aoe_damage(
                    &mut result,
                    entities,
                    entity_id,
                    impact_rx,
                    impact_ry,
                    air_impact,
                    terrain,
                    spread_leptons,
                    base_damage,
                    origin.source_id,
                    origin.source_house,
                    origin.warhead_ref,
                );
            }
        }

        // gamemd cell sweep: count_table[ftol(CellSpread + 0.99)] entries, exact order.
        for &(dx, dy) in cell_spread::splash_cells(cell_spread) {
            let Some(rx) = offset_cell_coord(impact_rx, dx) else {
                continue;
            };
            let Some(ry) = offset_cell_coord(impact_ry, dy) else {
                continue;
            };
            if let Some(prelude) = cell_prelude.as_deref_mut() {
                prelude.before_cell(
                    rx,
                    ry,
                    layer_context.overlay_grid.as_deref_mut(),
                    layer_context.overlay_registry,
                    layer_context.terrain.as_deref_mut(),
                    layer_context.scenario_rng.as_deref_mut(),
                    layer_context.occupancy,
                );
            }
            route_wall_before_cell_objects(
                entities,
                rx,
                ry,
                base_damage,
                warhead,
                &mut layer_context,
                &mut cell_prelude,
                &mut result,
            );
            let mut terrain_inserted = selected_layer != MovementLayer::Ground;
            if let Some(cell_occ) = occupancy.get(rx, ry) {
                for occupant in cell_occ.iter_layer(selected_layer) {
                    // TerrainClass was revealed before map technos. Native
                    // AddContent prepends later nonbuildings and appends
                    // Buildings, so the captured Terrain record sits at this
                    // exact represented boundary.
                    if !terrain_inserted && occupant.is_building {
                        push_terrain_aoe_damage(
                            &mut result,
                            terrain_objects,
                            rx,
                            ry,
                            impact_rx,
                            impact_ry,
                            exact_impact,
                            layer_context.terrain.as_deref(),
                            spread_leptons,
                            base_damage,
                            origin.warhead_ref,
                        );
                        terrain_inserted = true;
                    }
                    push_entity_aoe_damage(
                        &mut result,
                        entities,
                        occupant.entity_id,
                        impact_rx,
                        impact_ry,
                        rx,
                        ry,
                        dx == 0 && dy == 0,
                        exact_impact,
                        layer_context.terrain.as_deref(),
                        spread_leptons,
                        base_damage,
                        origin.source_id,
                        origin.source_house,
                        ground_source_admitted,
                        origin.warhead_ref,
                    );
                }
            }
            if !terrain_inserted {
                // A tree-only cell has no OccupancyGrid entry. Terrain lookup
                // must remain outside that optional Techno cache.
                push_terrain_aoe_damage(
                    &mut result,
                    terrain_objects,
                    rx,
                    ry,
                    impact_rx,
                    impact_ry,
                    exact_impact,
                    layer_context.terrain.as_deref(),
                    spread_leptons,
                    base_damage,
                    origin.warhead_ref,
                );
            }
        }

        return finalize_receiver_isolation_metadata(result, warhead);
    }

    // Even headless/no-occupancy callers still route the center-first wall
    // sweep when mutable overlay authority was supplied.
    for &(dx, dy) in cell_spread::splash_cells(cell_spread) {
        let Some(rx) = offset_cell_coord(impact_rx, dx) else {
            continue;
        };
        let Some(ry) = offset_cell_coord(impact_ry, dy) else {
            continue;
        };
        if let Some(prelude) = cell_prelude.as_deref_mut() {
            prelude.before_cell(
                rx,
                ry,
                layer_context.overlay_grid.as_deref_mut(),
                layer_context.overlay_registry,
                layer_context.terrain.as_deref_mut(),
                layer_context.scenario_rng.as_deref_mut(),
                layer_context.occupancy,
            );
        }
        route_wall_before_cell_objects(
            entities,
            rx,
            ry,
            base_damage,
            warhead,
            &mut layer_context,
            &mut cell_prelude,
            &mut result,
        );
    }

    for entity in entities.values() {
        if entity.lifecycle.in_limbo || entity.occupancy_list_layer().is_none() {
            continue;
        }
        // Dying corpses (uninit'd this tick, awaiting the end-of-tick drain)
        // are off the live object list — exclude them. A sold/captured corpse
        // keeps health>0, so gate on `dying`, not just health.
        if entity.health.current == 0 || entity.dying {
            continue;
        }

        push_entity_aoe_damage(
            &mut result,
            entities,
            entity.stable_id(),
            impact_rx,
            impact_ry,
            entity.position.rx,
            entity.position.ry,
            entity.position.rx == impact_rx && entity.position.ry == impact_ry,
            exact_impact,
            layer_context.terrain.as_deref(),
            spread_leptons,
            base_damage,
            origin.source_id,
            origin.source_house,
            ground_source_admitted,
            origin.warhead_ref,
        );
    }

    finalize_receiver_isolation_metadata(result, warhead)
}

fn route_wall_before_cell_objects(
    entities: &mut EntityStore,
    rx: u16,
    ry: u16,
    raw_damage: i32,
    warhead: &WarheadType,
    context: &mut AoELayerContext<'_>,
    cell_prelude: &mut Option<&mut dyn AoECellPrelude>,
    _result: &mut AoEDamageResult,
) {
    let (Some(grid), Some(registry), Some(rng)) = (
        context.overlay_grid.as_deref_mut(),
        context.overlay_registry,
        context.scenario_rng.as_deref_mut(),
    ) else {
        return;
    };
    let Some(overlay_id) = grid.cell(rx, ry).overlay_id else {
        return;
    };
    let Some(flags) = registry.flags(overlay_id) else {
        return;
    };
    if !flags.wall {
        return;
    }

    // Active-YR routing precedence: WAD forces literal -1; ordinary Wall and
    // Wood against Armor=wood pass the unchanged signed weapon damage.
    let routed_damage = if warhead.wall_absolute_destroyer {
        -1
    } else if warhead.wall || (warhead.wood && flags.armor_is_wood) {
        raw_damage
    } else {
        return;
    };

    let _wall_result = {
        let mut host = AoEWallDamageHost {
            entities,
            #[cfg(test)]
            trace: &mut _result.cell_target_detaches,
            prelude: cell_prelude,
        };
        damage_wall_overlay_with_runtime_host(
            grid,
            registry,
            context.terrain.as_deref_mut(),
            rx,
            ry,
            routed_damage,
            rng,
            Some(&mut host),
        )
    };
    #[cfg(test)]
    _result.wall_mutations.extend(_wall_result.mutations);
}

/// Broadcast one CellClass pointer-expiry notification to represented Techno
/// listeners. Native walks the listener roster forward, clears the live Target
/// first, and only then runs Restore when a mission was suspended.
pub(crate) fn expire_cell_target_references(
    entities: &mut EntityStore,
    rx: u16,
    ry: u16,
    #[cfg(test)] trace: &mut Vec<CellTargetDetach>,
) {
    let listener_ids = entities.keys_sorted();
    for listener_id in listener_ids {
        let Some(entity) = entities.get_mut(listener_id) else {
            continue;
        };
        if !matches!(
            entity.attack_target.as_ref().map(|target| target.target),
            Some(super::TargetKind::Cell(tx, ty)) if (tx, ty) == (rx, ry)
        ) {
            continue;
        }

        let mission_was_suspended =
            entity.mission.suspended() != crate::sim::mission::MissionId::NONE;
        represented_assign_target(entity, None);
        let _restored = mission_was_suspended && restore_entity_after_target_expiry(entity);
        #[cfg(test)]
        trace.push(CellTargetDetach {
            listener_id,
            restored: _restored,
            cleared: true,
        });
    }
}

fn select_object_damage_layer(
    terrain: &ResolvedTerrainGrid,
    impact_rx: u16,
    impact_ry: u16,
    impact_z: i32,
) -> MovementLayer {
    let Some(cell) = terrain.cell(impact_rx, impact_ry) else {
        return MovementLayer::Ground;
    };

    // Authoritative: delegate the ground-vs-deck choice to the single verified
    // selector in bridge_topology. It applies the strict-`>` half-deck boundary
    // (`impact_z > ground_z + DECK/2`, half = 2 levels) on the structural-bridge gate.
    // `ground_z` is `cell.level` in the same Level domain as `impact_z` (P0b: the
    // generic cell-center callers add the fixed deck offset, never a routed
    // GetGroundHeight, so both operands are cell-Level units here).
    let view = CellBridgeView::from_resolved(cell);
    match view.aoe_object_layer(impact_z, cell.level as i32) {
        ListLayer::Bridge => MovementLayer::Bridge,
        ListLayer::Ground => MovementLayer::Ground,
    }
}

fn offset_cell_coord(origin: u16, delta: i16) -> Option<u16> {
    let value = origin as i32 + delta as i32;
    if (0..=u16::MAX as i32).contains(&value) {
        Some(value as u16)
    } else {
        None
    }
}

/// Materialize gamemd's global airborne-vector query without replacing its
/// producer order with entity IDs or geometric distance. Repeated bucket IDs,
/// if emitted by the native-shaped query walk, deliberately copy that vector
/// again; this collector has no deduplication stage.
fn airborne_ids_in_spatial_order(
    entities: &EntityStore,
    impact_rx: u16,
    impact_ry: u16,
    radius_cells: i32,
    map_width: u16,
    map_height: u16,
) -> Vec<u64> {
    let mut buckets: BTreeMap<u16, Vec<(u64, u64)>> = BTreeMap::new();
    for entity in entities
        .values()
        .filter(|entity| entity.air_spatial_bucket.is_some() && air_spatial_tracks_entity(entity))
    {
        buckets
            .entry(entity.air_spatial_bucket.expect("filtered above"))
            .or_default()
            .push((entity.air_spatial_enter_order, entity.stable_id()));
    }
    for entries in buckets.values_mut() {
        entries.sort_unstable();
    }

    let mut ordered = Vec::new();
    for bucket in
        air_spatial_query_bucket_order(impact_rx, impact_ry, radius_cells, map_width, map_height)
    {
        if let Some(entries) = buckets.get(&bucket) {
            ordered.extend(entries.iter().map(|&(_, stable_id)| stable_id));
        }
    }
    ordered
}

fn impact_is_above_ground(
    terrain: &ResolvedTerrainGrid,
    impact_rx: u16,
    impact_ry: u16,
    impact: AoEAirImpact,
) -> bool {
    let Some(cell) = terrain.cell(impact_rx, impact_ry) else {
        return false;
    };
    let world_x = i32::from(impact_rx)
        .wrapping_mul(256)
        .wrapping_add(impact.sub_x.to_num::<i32>());
    let world_y = i32::from(impact_ry)
        .wrapping_mul(256)
        .wrapping_add(impact.sub_y.to_num::<i32>());
    ground_height_leptons(cell.level, cell.slope_type, world_x, world_y)
        .is_ok_and(|ground_z| ground_z < impact.z_leptons)
}

/// CoordStruct::Distance3D (`0x41C380`): x87 `x*x + y*y + z*z`, the retail
/// Sqrt_Approx LUT, then Math__ftol. The signed deltas intentionally wrap in
/// the same i32 coordinate domain before entering x87.
fn native_distance_leptons(impact_xyz: (i32, i32, i32), target_xyz: (i32, i32, i32)) -> i32 {
    let dx = X87Chop53::load_i32(target_xyz.0.wrapping_sub(impact_xyz.0));
    let dy = X87Chop53::load_i32(target_xyz.1.wrapping_sub(impact_xyz.1));
    let dz = X87Chop53::load_i32(target_xyz.2.wrapping_sub(impact_xyz.2));
    let squared = X87Chop53::add(
        X87Chop53::add(X87Chop53::mul(dx, dx), X87Chop53::mul(dy, dy)),
        X87Chop53::mul(dz, dz),
    );
    let root_bits =
        sqrt_approx_f32(squared).expect("map-space squared distance stays in finite f32 range");
    let root =
        X87Chop53::load_f32(root_bits).expect("Sqrt_Approx always returns a finite normal or zero");
    X87Chop53::ftol_i64(root).expect("map-space distance fits a signed integer") as i32
}

#[allow(clippy::too_many_arguments)]
fn push_airborne_aoe_damage(
    result: &mut AoEDamageResult,
    entities: &EntityStore,
    entity_id: u64,
    impact_rx: u16,
    impact_ry: u16,
    impact: AoEAirImpact,
    terrain: &ResolvedTerrainGrid,
    spread_leptons: i64,
    base_damage: i32,
    source_id: u64,
    source_house: Option<crate::sim::intern::InternedId>,
    warhead_ref: crate::sim::intern::InternedId,
) {
    let Some(entity) = entities.get(entity_id) else {
        return;
    };
    if entity.health.current == 0 || entity.dying {
        return;
    }
    let Some(target_z) =
        super::in_range::effective_z_leptons(entity, terrain).and_then(|z| i32::try_from(z).ok())
    else {
        return;
    };
    let impact_x = i32::from(impact_rx)
        .wrapping_mul(256)
        .wrapping_add(impact.sub_x.to_num::<i32>());
    let impact_y = i32::from(impact_ry)
        .wrapping_mul(256)
        .wrapping_add(impact.sub_y.to_num::<i32>());
    let target_x = i32::from(entity.position.rx)
        .wrapping_mul(256)
        .wrapping_add(entity.position.sub_x.to_num::<i32>());
    let target_y = i32::from(entity.position.ry)
        .wrapping_mul(256)
        .wrapping_add(entity.position.sub_y.to_num::<i32>());
    let raw_distance_leptons = native_distance_leptons(
        (impact_x, impact_y, impact.z_leptons),
        (target_x, target_y, target_z),
    );
    if i64::from(raw_distance_leptons) > spread_leptons {
        return;
    }

    // Apply_area_damage stores the raw 3D distance in its airborne receiver
    // record, then halves that captured signed integer only for a true
    // AircraftClass whose IsHighFlying vslot is true. Rust i32 division has
    // the same toward-zero behavior as the native signed IDIV.
    //
    // Both halves of that conjunction are explicit here on purpose:
    // `Apply_area_damage` 0x00489280 calls `What_Am_I` at 0x00489A5D and
    // compares against 2 (`AircraftClass::What_Am_I` 0x0041C180 returns 2),
    // and only then calls `IsHighFlying` (`vtable+0x54`) at 0x00489A69. The
    // category test belongs to THIS site, not to `is_high_flying` — the native
    // predicate itself is universal over `ObjectClass`, so a Jumpjet
    // `UnitClass` like the stock Kirov is high-flying but is not halved here.
    let distance_leptons =
        if entity.category == EntityCategory::Aircraft && super::in_range::is_high_flying(entity) {
            raw_distance_leptons / 2
        } else {
            raw_distance_leptons
        };
    if i64::from(distance_leptons) > spread_leptons {
        return;
    }

    result.push_entity(EntityDamageEvent::area(
        entity.stable_id(),
        base_damage,
        distance_leptons,
        source_id,
        source_house,
        warhead_ref,
    ));
}

#[allow(clippy::too_many_arguments)]
fn push_entity_aoe_damage(
    result: &mut AoEDamageResult,
    entities: &EntityStore,
    entity_id: u64,
    impact_rx: u16,
    impact_ry: u16,
    scan_rx: u16,
    scan_ry: u16,
    center_cell: bool,
    impact: AoEAirImpact,
    terrain: Option<&ResolvedTerrainGrid>,
    spread_leptons: i64,
    base_damage: i32,
    source_id: u64,
    source_house: Option<crate::sim::intern::InternedId>,
    source_self_admitted: bool,
    warhead_ref: crate::sim::intern::InternedId,
) {
    // Apply_area_damage's selected ground/deck list excludes the source object
    // unless its type opts into DamageSelf or this is Rules.CrushWarhead.
    // Airborne collection is a separate native phase and deliberately does
    // not use this admission gate.
    if entity_id == source_id && !source_self_admitted {
        return;
    }
    let Some(entity) = entities.get(entity_id) else {
        return;
    };
    if entity.health.current == 0 || entity.dying {
        return;
    }

    let impact_x = i32::from(impact_rx)
        .wrapping_mul(256)
        .wrapping_add(impact.sub_x.to_num::<i32>());
    let impact_y = i32::from(impact_ry)
        .wrapping_mul(256)
        .wrapping_add(impact.sub_y.to_num::<i32>());

    // Building receiver records are keyed to the selected CellClass entry.
    // Both center and adjacent foundation cells use that cell's center
    // coordinate. The center record alone gets a two-Level vertical allowance
    // before storing the remaining exact 3D distance. Ordinary objects use
    // their virtual +A4 coordinate, including exact subcell XY and absolute Z.
    let target_xyz = if entity.category == EntityCategory::Structure {
        let x = i32::from(scan_rx)
            .wrapping_mul(256)
            .wrapping_add(CELL_CENTER_LEPTON.to_num::<i32>());
        let y = i32::from(scan_ry)
            .wrapping_mul(256)
            .wrapping_add(CELL_CENTER_LEPTON.to_num::<i32>());
        let z = terrain
            .and_then(|terrain| terrain.cell(scan_rx, scan_ry))
            .and_then(|cell| ground_height_leptons(cell.level, cell.slope_type, x, y).ok())
            .unwrap_or_else(|| i32::from(entity.position.z).wrapping_mul(LEPTONS_PER_LEVEL as i32));
        (x, y, z)
    } else {
        let x = i32::from(entity.position.rx)
            .wrapping_mul(256)
            .wrapping_add(entity.position.sub_x.to_num::<i32>());
        let y = i32::from(entity.position.ry)
            .wrapping_mul(256)
            .wrapping_add(entity.position.sub_y.to_num::<i32>());
        let z = terrain
            .and_then(|terrain| super::in_range::effective_z_leptons(entity, terrain))
            .and_then(|z| i32::try_from(z).ok())
            .unwrap_or_else(|| {
                i32::from(entity.position.z)
                    .wrapping_mul(LEPTONS_PER_LEVEL as i32)
                    .wrapping_add(
                        entity
                            .locomotor
                            .as_ref()
                            .map(|locomotor| locomotor.altitude.to_num::<i32>())
                            .unwrap_or(0),
                    )
            });
        (x, y, z)
    };
    let distance_leptons = if entity.category == EntityCategory::Structure && center_cell {
        let height_above_cell = impact.z_leptons.wrapping_sub(target_xyz.2);
        if height_above_cell <= BUILDING_CENTER_HEIGHT_ALLOWANCE_LEPTONS {
            0
        } else {
            native_distance_leptons((impact_x, impact_y, impact.z_leptons), target_xyz)
                .wrapping_sub(BUILDING_CENTER_HEIGHT_ALLOWANCE_LEPTONS)
        }
    } else {
        native_distance_leptons((impact_x, impact_y, impact.z_leptons), target_xyz)
    };
    if i64::from(distance_leptons) > spread_leptons {
        return;
    }

    result.push_entity(EntityDamageEvent::area(
        entity.stable_id(),
        base_damage,
        distance_leptons,
        source_id,
        source_house,
        warhead_ref,
    ));
}

#[allow(clippy::too_many_arguments)]
fn push_terrain_aoe_damage(
    result: &mut AoEDamageResult,
    terrain_objects: Option<TerrainCollectionView<'_>>,
    scan_rx: u16,
    scan_ry: u16,
    impact_rx: u16,
    impact_ry: u16,
    impact: AoEAirImpact,
    terrain: Option<&ResolvedTerrainGrid>,
    spread_leptons: i64,
    base_damage: i32,
    warhead_ref: crate::sim::intern::InternedId,
) {
    let Some(view) = terrain_objects else {
        return;
    };
    let Some(&stable_id) = view.cells.get(&(scan_rx, scan_ry)) else {
        return;
    };
    let Some(object) = view.objects.get(&stable_id) else {
        return;
    };
    if object.lifecycle != TerrainObjectLifecycle::Live
        || object.rx != scan_rx
        || object.ry != scan_ry
    {
        return;
    }

    let impact_x = i32::from(impact_rx)
        .wrapping_mul(256)
        .wrapping_add(impact.sub_x.to_num::<i32>());
    let impact_y = i32::from(impact_ry)
        .wrapping_mul(256)
        .wrapping_add(impact.sub_y.to_num::<i32>());
    let target_x = i32::from(scan_rx)
        .wrapping_mul(256)
        .wrapping_add(CELL_CENTER_LEPTON.to_num::<i32>());
    let target_y = i32::from(scan_ry)
        .wrapping_mul(256)
        .wrapping_add(CELL_CENTER_LEPTON.to_num::<i32>());
    let target_z = terrain
        .and_then(|grid| grid.cell(scan_rx, scan_ry))
        .and_then(|cell| {
            ground_height_leptons(cell.level, cell.slope_type, target_x, target_y).ok()
        })
        .unwrap_or(0);
    let distance_leptons = native_distance_leptons(
        (impact_x, impact_y, impact.z_leptons),
        (target_x, target_y, target_z),
    );
    if i64::from(distance_leptons) > spread_leptons {
        return;
    }

    result.push_terrain(TerrainDamageEvent {
        stable_id,
        rx: scan_rx,
        ry: scan_ry,
        damage: base_damage,
        distance_leptons,
        warhead_ref,
        near_center_ic_isolation_eligible: false,
    });
}

/// Compute distance-scaled AoE damage using integer/fixed-point math.
///
/// At distance 0 (epicenter): full `base_damage * verses_pct / 100`.
/// At distance == cell_spread (edge): `base_damage * verses_pct * percent_at_max_pct / 10000`.
/// Linear interpolation between those extremes.
#[cfg(test)]
fn aoe_damage_at_distance(
    base_damage: i32,
    distance: SimFixed,
    cell_spread: SimFixed,
    percent_at_max_pct: u8,
    verses_pct: u8,
) -> u16 {
    // t = distance / cell_spread, clamped [0, 1] — how far from center (SimFixed).
    let t: SimFixed = if cell_spread > SIM_ZERO {
        (distance / cell_spread).clamp(SIM_ZERO, SimFixed::from_num(1))
    } else {
        SIM_ZERO
    };
    // falloff_pct = lerp(100, percent_at_max_pct, t) in integer.
    // = 100 + (percent_at_max_pct - 100) * t
    let pam: i32 = percent_at_max_pct as i32;
    let falloff_fixed: SimFixed = SimFixed::from_num(100) + SimFixed::from_num(pam - 100) * t;
    let falloff_pct: i32 = falloff_fixed.to_num::<i32>();

    // raw = base_damage * verses_pct * falloff_pct / 10000
    // Compute in i64 and clamp to i32 range to prevent silent narrowing overflow.
    let wide = base_damage as i64 * verses_pct as i64 * falloff_pct as i64 / 10000;
    wide.clamp(0, u16::MAX as i64) as u16
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::map::bridge_facts::{BRIDGE_FLAG_STRUCTURAL, BridgeCellFacts};
    use crate::map::houses::HouseAllianceMap;
    use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::rules::terrain_rules::{SpeedCostProfile, TerrainClass};
    use crate::sim::combat::{AttackTarget, TargetKind};
    use crate::sim::entity_store::EntityStore;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::house_state::{HouseDifficulty, HouseState};
    use crate::sim::intern::{StringInterner, test_intern, test_interner};
    use crate::sim::mission::state::MissionTestFixture;
    use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionType};
    use crate::sim::movement::locomotor::MovementLayer;
    use crate::sim::occupancy::{CellListInsertion, OccupancyGrid};
    use crate::util::fixed_math::sim_from_f32;

    fn hit_ids(hits: &[EntityDamageEvent]) -> Vec<u64> {
        hits.iter().map(|event| event.target_id).collect()
    }

    fn hit_id_distances(hits: &[EntityDamageEvent]) -> Vec<(u64, i32)> {
        hits.iter()
            .map(|event| {
                (
                    event.target_id,
                    event
                        .distance_leptons
                        .expect("Apply_area_damage record carries distance"),
                )
            })
            .collect()
    }

    fn wall_aoe_fixture(
        cell_spread: &str,
        warhead_flags: &str,
    ) -> (RuleSet, WarheadType, OverlayTypeRegistry) {
        let ini = IniFile::from_str(&format!(
            "[InfantryTypes]\n\
             [VehicleTypes]\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [Warheads]\n0=WH\n\
             [OverlayTypes]\n0=GASAND\n1=CYCL\n2=GAWALL\n\
             [WH]\nCellSpread={cell_spread}\n{warhead_flags}\n\
             [GASAND]\nWall=yes\nArmor=wood\nStrength=400\n\
             [CYCL]\nWall=yes\nArmor=wood\nStrength=400\n\
             [GAWALL]\nWall=yes\nArmor=concrete\nStrength=400\n"
        ));
        let art = IniFile::from_str(
            "[GASAND]\nDamageLevels=2\n\
             [CYCL]\nDamageLevels=2\n\
             [GAWALL]\nDamageLevels=3\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("wall AoE rules parse");
        let warhead = WarheadType::from_ini_section("WH", ini.section("WH").expect("WH section"));
        let registry = OverlayTypeRegistry::from_ini(&ini, Some(&art));
        (rules, warhead, registry)
    }

    #[test]
    fn test_aoe_damage_at_center() {
        // At distance 0, full damage: 100 * 100 * 100 / 10000 = 100.
        let dmg = aoe_damage_at_distance(100, SIM_ZERO, sim_from_f32(3.0), 25, 100);
        assert_eq!(dmg, 100);
    }

    #[test]
    fn test_aoe_damage_at_edge() {
        // At distance == cell_spread, damage = base * percent_at_max / 100 = 100 * 25 / 100 = 25.
        let dmg = aoe_damage_at_distance(100, sim_from_f32(3.0), sim_from_f32(3.0), 25, 100);
        assert_eq!(dmg, 25);
    }

    #[test]
    fn test_aoe_damage_at_midpoint() {
        // At half distance, falloff_pct = lerp(100, 25, 0.5) = 62.
        // damage = 100 * 100 * 62 / 10000 = 62.
        let dmg = aoe_damage_at_distance(100, sim_from_f32(1.5), sim_from_f32(3.0), 25, 100);
        assert_eq!(dmg, 62);
    }

    #[test]
    fn test_aoe_damage_with_verses() {
        // 50% verses at center: 100 * 50 * 100 / 10000 = 50.
        let dmg = aoe_damage_at_distance(100, SIM_ZERO, sim_from_f32(3.0), 25, 50);
        assert_eq!(dmg, 50);
    }

    #[test]
    fn test_aoe_damage_zero_verses() {
        let dmg = aoe_damage_at_distance(100, SIM_ZERO, sim_from_f32(3.0), 25, 0);
        assert_eq!(dmg, 0);
    }

    #[test]
    fn gsi_04_07_damage_signed_zero_returns_before_wall_rng_objects_and_detach() {
        let mut entities = EntityStore::new();
        let mut listener = GameEntity::test_default(20, "MTNK", "Americans", 8, 8);
        listener.attack_target = Some(AttackTarget::for_cell(8, 8));
        entities.insert(listener);
        let mut interner = test_interner();
        let warhead_ref = interner.intern("BlastWH");

        let (rules, wad, registry) = wall_aoe_fixture(".5", "WallAbsoluteDestroyer=yes\nWall=yes");
        let mut overlays = OverlayGrid::new(16, 16);
        overlays.place_overlay(8, 8, 2, 0);
        let mut scenario_rng = SimRng::new(17);
        let before = scenario_rng.state();
        let result = apply_aoe_damage(
            &mut entities,
            8,
            8,
            0,
            &wad,
            &rules,
            &mut interner,
            (crate::sim::combat::RAD_NO_ATTACKER, warhead_ref),
            AoELayerContext {
                occupancy: None,
                terrain: None,
                overlay_grid: Some(&mut overlays),
                overlay_registry: Some(&registry),
                scenario_rng: Some(&mut scenario_rng),
                air_impact: None,
                impact_z: 0,
            },
        );
        assert_eq!(overlays.cell(8, 8).overlay_id, Some(2));
        assert_eq!(scenario_rng.state(), before);
        assert!(result.hits.is_empty());
        assert!(result.wall_mutations.is_empty());
        assert!(result.cell_target_detaches.is_empty());
        assert!(matches!(
            entities
                .get(20)
                .unwrap()
                .attack_target
                .as_ref()
                .map(|target| target.target),
            Some(TargetKind::Cell(8, 8))
        ));

        let (rules, wall, registry) = wall_aoe_fixture(".5", "Wall=yes");
        let mut overlays = OverlayGrid::new(16, 16);
        overlays.place_overlay(8, 8, 0, 0);
        let mut scenario_rng = SimRng::new(29);
        let before = scenario_rng.state();
        let result = apply_aoe_damage(
            &mut entities,
            8,
            8,
            0,
            &wall,
            &rules,
            &mut interner,
            (crate::sim::combat::RAD_NO_ATTACKER, warhead_ref),
            AoELayerContext {
                occupancy: None,
                terrain: None,
                overlay_grid: Some(&mut overlays),
                overlay_registry: Some(&registry),
                scenario_rng: Some(&mut scenario_rng),
                air_impact: None,
                impact_z: 0,
            },
        );
        assert_eq!(overlays.cell(8, 8).overlay_id, Some(0));
        assert_eq!(scenario_rng.state(), before);
        assert!(result.hits.is_empty());
        assert!(result.wall_mutations.is_empty());
        assert!(result.cell_target_detaches.is_empty());
    }

    #[test]
    fn gsi_04_10_scenario_no_damage_returns_before_wall_rng_objects_and_detach() {
        let mut entities = EntityStore::new();
        let mut listener = GameEntity::test_default(20, "MTNK", "Americans", 8, 8);
        listener.attack_target = Some(AttackTarget::for_cell(8, 8));
        entities.insert(listener);
        let mut interner = test_interner();
        let warhead_ref = interner.intern("BlastWH");
        let (rules, warhead, registry) =
            wall_aoe_fixture(".5", "WallAbsoluteDestroyer=yes\nWall=yes");
        let mut overlays = OverlayGrid::new(16, 16);
        overlays.place_overlay(8, 8, 2, 0);
        let mut scenario_rng = SimRng::new(17);
        let before = scenario_rng.state();

        let handles =
            crate::sim::type_handle_table::ResolvedRuleHandles::resolve(&rules, &mut interner);
        let result = apply_aoe_damage_with_terrain_and_scenario(
            &mut entities,
            8,
            8,
            100,
            &warhead,
            &rules,
            &interner,
            Some(handles),
            (crate::sim::combat::RAD_NO_ATTACKER, None, warhead_ref),
            AoELayerContext {
                occupancy: None,
                terrain: None,
                overlay_grid: Some(&mut overlays),
                overlay_registry: Some(&registry),
                scenario_rng: Some(&mut scenario_rng),
                air_impact: None,
                impact_z: 0,
            },
            None,
            true,
            None,
        );

        assert_eq!(overlays.cell(8, 8).overlay_id, Some(2));
        assert_eq!(scenario_rng.state(), before);
        assert!(result.receivers.is_empty());
        assert!(result.wall_mutations.is_empty());
        assert!(result.cell_target_detaches.is_empty());
        assert!(matches!(
            entities
                .get(20)
                .unwrap()
                .attack_target
                .as_ref()
                .map(|target| target.target),
            Some(TargetKind::Cell(8, 8))
        ));
    }

    #[test]
    fn gsi_04_11_per_cell_ore_rng_precedes_later_wall_rng() {
        struct OreReseedDraw {
            cell: (u16, u16),
        }

        impl AoECellPrelude for OreReseedDraw {
            fn before_cell(
                &mut self,
                rx: u16,
                ry: u16,
                _overlay_grid: Option<&mut OverlayGrid>,
                _overlay_registry: Option<&OverlayTypeRegistry>,
                _terrain: Option<&mut ResolvedTerrainGrid>,
                scenario_rng: Option<&mut SimRng>,
                _occupancy: Option<&OccupancyGrid>,
            ) {
                if (rx, ry) == self.cell {
                    let _ = scenario_rng
                        .expect("AoE fixture supplies scenario RNG")
                        .next_range_u32_inclusive(0, 7);
                }
            }
        }

        let (rules, warhead, registry) = wall_aoe_fixture("1", "Wall=yes");
        let mut interner = test_interner();
        let warhead_ref = interner.intern("WH");
        let offsets = cell_spread::splash_cells(warhead.cell_spread);
        assert_eq!(offsets[0], (0, 0));
        let wall_offset = offsets[1];
        let wall_cell = (
            (8_i32 + i32::from(wall_offset.0)) as u16,
            (8_i32 + i32::from(wall_offset.1)) as u16,
        );
        let mut overlays = OverlayGrid::new(16, 16);
        overlays.place_overlay(wall_cell.0, wall_cell.1, 2, 0);
        let mut scenario_rng = SimRng::new(91);
        let mut prelude = OreReseedDraw { cell: (8, 8) };

        let handles =
            crate::sim::type_handle_table::ResolvedRuleHandles::resolve(&rules, &mut interner);
        let _ = apply_aoe_damage_with_terrain_and_scenario(
            &mut EntityStore::new(),
            8,
            8,
            1,
            &warhead,
            &rules,
            &interner,
            Some(handles),
            (crate::sim::combat::RAD_NO_ATTACKER, None, warhead_ref),
            AoELayerContext {
                occupancy: None,
                terrain: None,
                overlay_grid: Some(&mut overlays),
                overlay_registry: Some(&registry),
                scenario_rng: Some(&mut scenario_rng),
                air_impact: None,
                impact_z: 0,
            },
            None,
            false,
            Some(&mut prelude),
        );

        let mut expected_rng = SimRng::new(91);
        let _ = expected_rng.next_range_u32_inclusive(0, 7);
        let _ = expected_rng.next_range_u32_inclusive(0, 400);
        assert_eq!(scenario_rng.state(), expected_rng.state());
    }

    #[test]
    fn gsi_04_07_damage_air_height_gate_and_exact_3d_falloff_boundaries() {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n0=JUMP\n\
             [VehicleTypes]\n\
             [AircraftTypes]\n0=AIR\n\
             [BuildingTypes]\n\
             [Warheads]\n0=BlastWH\n\
             [AIR]\nStrength=1000\nArmor=none\n\
             [JUMP]\nStrength=1000\nArmor=none\n\
             [BlastWH]\nCellSpread=1\nPercentAtMax=.25\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("3D air rules");
        let warhead = rules.warhead("BlastWH").unwrap().clone();
        let mut entities = EntityStore::new();
        for (stable_id, type_id, category, altitude) in [
            (1, "AIR", EntityCategory::Aircraft, 1),
            (2, "AIR", EntityCategory::Aircraft, 129),
            (3, "AIR", EntityCategory::Aircraft, 257),
            (4, "AIR", EntityCategory::Aircraft, 300),
            (5, "JUMP", EntityCategory::Infantry, 257),
        ] {
            let mut air = GameEntity::test_default(stable_id, type_id, "Soviet", 5, 5);
            air.category = category;
            air.health.current = 1000;
            let mut locomotor = crate::sim::movement::locomotor::LocomotorState::from_object_type(
                rules.object(type_id).unwrap(),
                0,
            );
            locomotor.layer = MovementLayer::Air;
            locomotor.altitude = SimFixed::from_num(altitude);
            air.locomotor = Some(locomotor);
            air.air_spatial_bucket = Some(5 + 5 * 20);
            air.air_spatial_enter_order = stable_id;
            entities.insert(air);
        }
        let mut interner = test_interner();
        let warhead_ref = interner.intern("BlastWH");
        let occupancy = OccupancyGrid::new();
        let cells = (0..20)
            .flat_map(|ry| (0..20).map(move |rx| test_terrain_cell(rx, ry)))
            .collect();
        let mut terrain = ResolvedTerrainGrid::from_cells(20, 20, cells);

        let at_ground = apply_aoe_damage(
            &mut entities,
            5,
            5,
            100,
            &warhead,
            &rules,
            &mut interner,
            (crate::sim::combat::RAD_NO_ATTACKER, warhead_ref),
            AoELayerContext {
                occupancy: Some(&occupancy),
                terrain: Some(&mut terrain),
                air_impact: Some(AoEAirImpact {
                    sub_x: CELL_CENTER_LEPTON,
                    sub_y: CELL_CENTER_LEPTON,
                    z_leptons: 0,
                }),
                impact_z: 0,
                ..AoELayerContext::default()
            },
        );
        assert!(
            at_ground.hits.is_empty(),
            "impact Z equal to exact ground height skips the entire air query"
        );

        let one_above = apply_aoe_damage(
            &mut entities,
            5,
            5,
            100,
            &warhead,
            &rules,
            &mut interner,
            (crate::sim::combat::RAD_NO_ATTACKER, warhead_ref),
            AoELayerContext {
                occupancy: Some(&occupancy),
                terrain: Some(&mut terrain),
                air_impact: Some(AoEAirImpact {
                    sub_x: CELL_CENTER_LEPTON,
                    sub_y: CELL_CENTER_LEPTON,
                    z_leptons: 1,
                }),
                impact_z: 0,
                ..AoELayerContext::default()
            },
        );
        assert_eq!(
            hit_id_distances(&one_above.hits),
            vec![(1, 0), (2, 128), (3, 128), (5, 256)],
            "low Aircraft 129 and high Aircraft 257/2 both capture 128; airborne Infantry stays unhalved and raw distance 299 stays outside"
        );
    }

    #[test]
    fn gsi_04_07_damage_ground_native_distance_and_receiver_kernel() {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n\
             [VehicleTypes]\n0=MTNK\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [Warheads]\n0=IvanWH\n\
             [CombatDamage]\nMaxDamage=10000\n\
             [MTNK]\nStrength=300\nArmor=heavy\n\
             [IvanWH]\nCellSpread=1.5\nPercentAtMax=.25\n\
             Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("Ivan falloff fixture");
        let warhead = rules.warhead("IvanWH").unwrap().clone();

        let mut entities = EntityStore::new();
        for (stable_id, rx, sub_x) in [(320, 11, 192), (384, 12, 0), (385, 12, 1), (386, 12, 2)] {
            let mut target = GameEntity::test_default(stable_id, "MTNK", "Soviet", rx, 10);
            target.position.sub_x = SimFixed::from_num(sub_x);
            target.position.sub_y = CELL_CENTER_LEPTON;
            target.health.current = 300;
            entities.insert(target);
        }
        let mut interner = test_interner();
        let warhead_ref = interner.intern("IvanWH");
        let mut occupancy = OccupancyGrid::new();
        occupancy.add(
            11,
            10,
            320,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        // Prepend in reverse so the cell's native selected-list order is
        // 384,385,386. The last candidate is excluded only after Sqrt_Approx.
        for stable_id in [386, 385, 384] {
            occupancy.add(
                12,
                10,
                stable_id,
                MovementLayer::Ground,
                None,
                CellListInsertion::PrependNonBuilding,
            );
        }
        let cells = (0..24)
            .flat_map(|ry| (0..24).map(move |rx| test_terrain_cell(rx, ry)))
            .collect();
        let mut terrain = ResolvedTerrainGrid::from_cells(24, 24, cells);

        let result = apply_aoe_damage(
            &mut entities,
            10,
            10,
            450,
            &warhead,
            &rules,
            &mut interner,
            (77, warhead_ref),
            AoELayerContext {
                occupancy: Some(&occupancy),
                terrain: Some(&mut terrain),
                air_impact: Some(AoEAirImpact {
                    sub_x: CELL_CENTER_LEPTON,
                    sub_y: CELL_CENTER_LEPTON,
                    z_leptons: 0,
                }),
                impact_z: 0,
                ..AoELayerContext::default()
            },
        );
        assert_eq!(
            result
                .hits
                .iter()
                .map(|event| {
                    (
                        event.target_id,
                        event.damage,
                        event.distance_leptons,
                        event.attacker_id,
                        event.warhead_ref,
                    )
                })
                .collect::<Vec<_>>(),
            vec![
                (320, 450, Some(320), 77, warhead_ref),
                (384, 450, Some(384), 77, warhead_ref),
                (385, 450, Some(384), 77, warhead_ref),
            ],
            "Sqrt_Approx keeps 385 at the inclusive boundary and rejects 386"
        );

        let mut main_rng = SimRng::new(1);
        let mut scenario_rng = SimRng::new(2);
        let mut handled_deaths = Vec::new();
        let mut houses = BTreeMap::new();
        let mut fatal_lifecycle = None;
        let mut sound_sink = None;
        let (death, pings) = crate::sim::combat::commit_damage_events(
            &result.hits,
            &mut entities,
            &mut occupancy,
            &rules,
            &mut interner,
            &mut houses,
            &[],
            &HouseAllianceMap::new(),
            &mut main_rng,
            &mut scenario_rng,
            &mut handled_deaths,
            None,
            None,
            Some(&mut terrain),
            0,
            &mut fatal_lifecycle,
            &mut sound_sink,
        );
        assert!(death.despawned_ids.is_empty());
        assert!(pings.is_empty());
        assert_eq!(entities.get(320).unwrap().health.current, 132);
        assert_eq!(entities.get(384).unwrap().health.current, 188);
        assert_eq!(entities.get(385).unwrap().health.current, 188);
        assert_eq!(entities.get(386).unwrap().health.current, 300);
    }

    #[test]
    fn gsi_04_07_damage_center_building_high_z_uses_two_level_allowance() {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n\
             [VehicleTypes]\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n0=BUILD\n\
             [Warheads]\n0=BlastWH\n\
             [BUILD]\nStrength=300\nArmor=concrete\n\
             [BlastWH]\nCellSpread=1\nPercentAtMax=.25\n\
             Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("center-building height fixture");
        let warhead = rules.warhead("BlastWH").unwrap().clone();
        let mut interner = test_interner();
        let warhead_ref = interner.intern("BlastWH");
        let mut entities = EntityStore::new();
        let mut building = GameEntity::test_default(1, "BUILD", "Neutral", 5, 5);
        building.category = EntityCategory::Structure;
        building.health.current = 300;
        entities.insert(building);
        let mut occupancy = OccupancyGrid::new();
        occupancy.add(
            5,
            5,
            1,
            MovementLayer::Ground,
            None,
            CellListInsertion::AppendBuilding,
        );
        let cells = (0..12)
            .flat_map(|ry| (0..12).map(move |rx| test_terrain_cell(rx, ry)))
            .collect();
        let mut terrain = ResolvedTerrainGrid::from_cells(12, 12, cells);

        for (impact_z, expected_distance) in [(208, 0), (256, 48)] {
            let result = apply_aoe_damage(
                &mut entities,
                5,
                5,
                100,
                &warhead,
                &rules,
                &mut interner,
                (crate::sim::combat::RAD_NO_ATTACKER, warhead_ref),
                AoELayerContext {
                    occupancy: Some(&occupancy),
                    terrain: Some(&mut terrain),
                    air_impact: Some(AoEAirImpact {
                        sub_x: CELL_CENTER_LEPTON,
                        sub_y: CELL_CENTER_LEPTON,
                        z_leptons: impact_z,
                    }),
                    impact_z: 0,
                    ..AoELayerContext::default()
                },
            );
            assert_eq!(
                result
                    .hits
                    .iter()
                    .map(|event| (event.target_id, event.damage, event.distance_leptons))
                    .collect::<Vec<_>>(),
                vec![(1, 100, Some(expected_distance))],
                "center Building at impact Z {impact_z} keeps native two-Level allowance"
            );
        }
    }

    #[test]
    fn gsi_04_07_damage_near_center_iron_curtain_isolates_non_invulnerable_records() {
        use crate::sim::superweapon::invulnerability::{InvulnKind, InvulnerabilityState};

        fn run(
            protected_distance: i32,
            kind: InvulnKind,
        ) -> (Vec<(u64, i32, bool)>, i32, Vec<u64>) {
            let ini = IniFile::from_str(
                "[InfantryTypes]\n\
                 [VehicleTypes]\n0=TARGET\n\
                 [AircraftTypes]\n\
                 [BuildingTypes]\n\
                 [Warheads]\n0=BlastWH\n\
                 [CombatDamage]\nMaxDamage=10000\n\
                 [TARGET]\nStrength=100\nArmor=heavy\n\
                 [BlastWH]\nCellSpread=.5\nPercentAtMax=1\n\
                 Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
            );
            let rules = RuleSet::from_ini(&ini).expect("near-center IC fixture");
            let warhead = rules.warhead("BlastWH").unwrap().clone();
            let mut interner = test_interner();
            let target_ref = interner.intern("TARGET");
            let victim_owner = interner.intern("Victim");
            let warhead_ref = interner.intern("BlastWH");
            let mut entities = EntityStore::new();

            let mut vulnerable = GameEntity::test_default(1, "TARGET", "Victim", 5, 5);
            vulnerable.type_ref = target_ref;
            vulnerable.owner = victim_owner;
            vulnerable.position.sub_x = CELL_CENTER_LEPTON;
            vulnerable.position.sub_y = CELL_CENTER_LEPTON;
            vulnerable.lifecycle.in_limbo = false;
            vulnerable.lifecycle.cell_marked = true;
            entities.insert(vulnerable);

            let mut protected = GameEntity::test_default(2, "TARGET", "Victim", 5, 5);
            protected.type_ref = target_ref;
            protected.owner = victim_owner;
            protected.position.sub_x = SimFixed::from_num(
                CELL_CENTER_LEPTON
                    .to_num::<i32>()
                    .wrapping_add(protected_distance),
            );
            protected.position.sub_y = CELL_CENTER_LEPTON;
            protected.lifecycle.in_limbo = false;
            protected.lifecycle.cell_marked = true;
            protected.invulnerability = Some(InvulnerabilityState {
                start_frame: 0,
                duration_frames: 100,
                kind,
            });
            entities.insert(protected);

            let mut occupancy = OccupancyGrid::new();
            // Native non-building insertion is at the list head. Insert the
            // protected target first so the vulnerable record is collected
            // first, proving that a later IC record isolates globally.
            for stable_id in [2, 1] {
                occupancy.add(
                    5,
                    5,
                    stable_id,
                    MovementLayer::Ground,
                    None,
                    CellListInsertion::PrependNonBuilding,
                );
            }
            let cells = (0..12)
                .flat_map(|ry| (0..12).map(move |rx| test_terrain_cell(rx, ry)))
                .collect();
            let mut terrain = ResolvedTerrainGrid::from_cells(12, 12, cells);
            let aoe = apply_aoe_damage(
                &mut entities,
                5,
                5,
                10,
                &warhead,
                &rules,
                &mut interner,
                (crate::sim::combat::RAD_NO_ATTACKER, warhead_ref),
                AoELayerContext {
                    occupancy: Some(&occupancy),
                    terrain: Some(&mut terrain),
                    air_impact: Some(AoEAirImpact {
                        sub_x: CELL_CENTER_LEPTON,
                        sub_y: CELL_CENTER_LEPTON,
                        z_leptons: 0,
                    }),
                    impact_z: 0,
                    ..AoELayerContext::default()
                },
            );
            let records = aoe
                .hits
                .iter()
                .map(|event| {
                    (
                        event.target_id,
                        event.distance_leptons.unwrap(),
                        event.near_center_ic_isolation_eligible,
                    )
                })
                .collect::<Vec<_>>();

            let mut main_rng = SimRng::new(1);
            let mut scenario_rng = SimRng::new(2);
            let mut handled_deaths = Vec::new();
            let mut houses = BTreeMap::new();
            let mut fatal_lifecycle = None;
            let mut sound_sink = None;
            let (effects, pings) = crate::sim::combat::commit_damage_events(
                &aoe.hits,
                &mut entities,
                &mut occupancy,
                &rules,
                &mut interner,
                &mut houses,
                &[],
                &HouseAllianceMap::new(),
                &mut main_rng,
                &mut scenario_rng,
                &mut handled_deaths,
                None,
                None,
                Some(&mut terrain),
                0,
                &mut fatal_lifecycle,
                &mut sound_sink,
            );
            assert!(pings.is_empty());
            (
                records,
                entities.get(1).unwrap().health.current,
                effects
                    .invulnerability_impact_effects
                    .iter()
                    .map(|effect| effect.target_id)
                    .collect(),
            )
        }

        for distance in [0, 84] {
            let (records, vulnerable_hp, effects) = run(distance, InvulnKind::IronCurtain);
            assert_eq!(records, vec![(1, 0, true), (2, distance, true)]);
            assert_eq!(
                vulnerable_hp, 100,
                "IC at signed distance {distance} isolates an earlier vulnerable record"
            );
            assert_eq!(
                effects,
                vec![2],
                "the protected receiver still runs in order"
            );
        }

        let (records, vulnerable_hp, effects) = run(85, InvulnKind::IronCurtain);
        assert_eq!(records, vec![(1, 0, true), (2, 85, true)]);
        assert_eq!(vulnerable_hp, 90, "distance 85 does not arm isolation");
        assert_eq!(effects, vec![2]);

        let (records, vulnerable_hp, effects) = run(0, InvulnKind::ForceShield);
        assert_eq!(records, vec![(1, 0, true), (2, 0, true)]);
        assert_eq!(vulnerable_hp, 90, "Force Shield does not arm isolation");
        assert_eq!(effects, vec![2]);
    }

    #[test]
    fn gsi_04_07_damage_air_receivers_precede_center_first_ground_death_effects() {
        fn run(
            hp: i32,
        ) -> (
            Vec<u64>,
            crate::sim::combat::DeathEffects,
            OverlayGrid,
            EntityStore,
            u64,
        ) {
            let ini = IniFile::from_str(
                "[InfantryTypes]\n\
                 [VehicleTypes]\n0=GROUNDBOMB\n\
                 [AircraftTypes]\n0=AIRBOMB\n\
                 [BuildingTypes]\n\
                 [Warheads]\n0=BlastWH\n1=WallWH\n\
                 [OverlayTypes]\n0=TESTWALL\n\
                 [AIRBOMB]\nStrength=10\nArmor=heavy\nExplodes=yes\nDeathWeapon=DeathBoom\n\
                 [GROUNDBOMB]\nStrength=10\nArmor=heavy\nExplodes=yes\nDeathWeapon=DeathBoom\n\
                 [DeathBoom]\nDamage=399\nWarhead=WallWH\n\
                 [BlastWH]\nCellSpread=2\nPercentAtMax=1\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
                 [WallWH]\nCellSpread=0\nWall=yes\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
                 [TESTWALL]\nWall=yes\nArmor=concrete\nStrength=400\n",
            );
            let art = IniFile::from_str("[TESTWALL]\nDamageLevels=2\n");
            let mut rules = RuleSet::from_ini(&ini).expect("air-first death rules");
            let blast = rules.warhead("BlastWH").unwrap().clone();
            let registry = OverlayTypeRegistry::from_ini(&ini, Some(&art));

            let make_air = |stable_id, rx, enter_order| {
                let mut air = GameEntity::test_default(stable_id, "AIRBOMB", "Soviet", rx, 5);
                air.category = EntityCategory::Aircraft;
                air.health.current = hp;
                air.position.z = 0;
                let mut air_locomotor =
                    crate::sim::movement::locomotor::LocomotorState::from_object_type(
                        rules.object("AIRBOMB").unwrap(),
                        0,
                    );
                air_locomotor.layer = MovementLayer::Air;
                air_locomotor.altitude = SimFixed::from_num(1);
                air.locomotor = Some(air_locomotor);
                air.air_spatial_bucket = Some(rx + 5 * 20);
                air.air_spatial_enter_order = enter_order;
                air
            };
            // Same-bucket vector order deliberately reverses stable IDs. The
            // lower-ID perimeter aircraft must still wait for the center bucket.
            let air_center_first = make_air(40, 5, 1);
            let air_center_second = make_air(30, 5, 2);
            let air_perimeter = make_air(5, 3, 3);

            let mut center = GameEntity::test_default(20, "GROUNDBOMB", "Soviet", 5, 5);
            center.health.current = hp;
            let mut east = GameEntity::test_default(10, "GROUNDBOMB", "Soviet", 7, 5);
            east.health.current = hp;

            let mut entities = EntityStore::new();
            entities.insert(east);
            entities.insert(center);
            entities.insert(air_perimeter);
            entities.insert(air_center_second);
            entities.insert(air_center_first);
            let mut occupancy = OccupancyGrid::new();
            occupancy.add(
                5,
                5,
                20,
                MovementLayer::Ground,
                None,
                CellListInsertion::PrependNonBuilding,
            );
            occupancy.add(
                7,
                5,
                10,
                MovementLayer::Ground,
                None,
                CellListInsertion::PrependNonBuilding,
            );
            let cells = (0..10)
                .flat_map(|ry| (0..10).map(move |rx| test_terrain_cell(rx, ry)))
                .collect();
            let mut terrain = ResolvedTerrainGrid::from_cells(10, 10, cells);
            let mut overlays = OverlayGrid::new(10, 10);
            for &(rx, ry) in &[(3, 5), (5, 5), (7, 5)] {
                overlays.place_overlay(rx, ry, 0, 0);
            }

            let mut interner = test_interner();
            let _handles =
                crate::sim::type_handle_table::ResolvedRuleHandles::resolve(&rules, &mut interner);
            let blast_ref = interner.intern("BlastWH");
            let mut scenario_rng = SimRng::new(1);
            let air_impact = Some(AoEAirImpact {
                sub_x: CELL_CENTER_LEPTON,
                sub_y: CELL_CENTER_LEPTON,
                z_leptons: 1,
            });
            let aoe = apply_aoe_damage(
                &mut entities,
                5,
                5,
                10,
                &blast,
                &rules,
                &mut interner,
                (crate::sim::combat::RAD_NO_ATTACKER, blast_ref),
                AoELayerContext {
                    occupancy: Some(&occupancy),
                    terrain: Some(&mut terrain),
                    overlay_grid: Some(&mut overlays),
                    overlay_registry: Some(&registry),
                    scenario_rng: Some(&mut scenario_rng),
                    air_impact,
                    impact_z: 0,
                },
            );
            let hit_order = hit_ids(&aoe.hits);
            let damage_events = aoe.hits;
            let mut main_rng = SimRng::new(9);
            let mut handled_deaths = Vec::new();
            let mut houses = BTreeMap::new();
            let mut fatal_lifecycle = None;
            let mut sound_sink = None;
            let (effects, pings) = crate::sim::combat::commit_damage_events(
                &damage_events,
                &mut entities,
                &mut occupancy,
                &rules,
                &mut interner,
                &mut houses,
                &[],
                &HouseAllianceMap::new(),
                &mut main_rng,
                &mut scenario_rng,
                &mut handled_deaths,
                Some(&mut overlays),
                Some(&registry),
                Some(&mut terrain),
                0,
                &mut fatal_lifecycle,
                &mut sound_sink,
            );
            assert!(pings.is_empty());
            (hit_order, effects, overlays, entities, scenario_rng.state())
        }

        let (hit_order, fatal, fatal_walls, fatal_entities, fatal_rng) = run(10);
        assert_eq!(
            hit_order,
            vec![40, 30, 5, 20, 10],
            "center air bucket/vector order, perimeter air, then center-first ground"
        );
        assert_eq!(
            fatal.immediate_uninit_ids,
            vec![30, 40, 30, 5, 20, 10],
            // Aircraft4165C0 calls Foot at4165EC before its concrete cleanup.
            // Nested30 therefore finishes before40, then the captured outer30
            // record repeats its zero-HP receiver (no Aircraft Alive guard).
            // Native producer-entry proof: bridge_zero_health_deathweapon.json.
            "nested aircraft cleanup precedes its parent; the captured outer record still repeats"
        );
        let direct_cells: Vec<_> = fatal
            .wall_mutations
            .iter()
            .filter(|mutation| {
                matches!(
                    mutation.kind,
                    crate::sim::overlay_grid::WallMutationKind::DirectUpdated
                        | crate::sim::overlay_grid::WallMutationKind::DirectRemoved
                )
            })
            .map(|mutation| (mutation.rx, mutation.ry))
            .collect();
        let mut reference_rng = SimRng::new(1);
        let rolls = [
            reference_rng.next_range_u32_inclusive(0, 400),
            reference_rng.next_range_u32_inclusive(0, 400),
            reference_rng.next_range_u32_inclusive(0, 400),
        ];
        assert_eq!(rolls[0], 213);
        let cells = [(5, 5), (3, 5), (7, 5)];
        let expected_direct: Vec<_> = cells
            .into_iter()
            .zip(rolls)
            .filter_map(|(cell, roll)| (roll < 399).then_some(cell))
            .collect();
        assert_eq!(direct_cells, expected_direct);
        assert_eq!(fatal_rng, reference_rng.state());
        for (cell, roll) in cells.into_iter().zip(rolls) {
            assert_eq!(
                fatal_walls.cell(cell.0, cell.1).overlay_id.is_none(),
                roll < 399
            );
        }
        for id in [40, 30, 5, 20, 10] {
            assert_eq!(fatal_entities.get(id).unwrap().health.current, 0);
        }

        let (hit_order, survives, surviving_walls, surviving_entities, surviving_rng) = run(11);
        assert_eq!(hit_order, vec![40, 30, 5, 20, 10]);
        assert!(survives.immediate_uninit_ids.is_empty());
        assert!(survives.wall_mutations.is_empty());
        assert_eq!(surviving_rng, SimRng::new(1).state());
        for &(rx, ry) in &cells {
            assert_eq!(surviving_walls.cell(rx, ry).overlay_id, Some(0));
        }
        for id in [40, 30, 5, 20, 10] {
            assert_eq!(surviving_entities.get(id).unwrap().health.current, 1);
        }
    }

    #[test]
    fn gsi_04_07_damage_band11_repeated_cell_dispatches_receiver_twice() {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n\
             [VehicleTypes]\n0=TESTUNIT\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [Warheads]\n0=WIDEWH\n\
             [TESTUNIT]\nStrength=100\nArmor=heavy\n\
             [WIDEWH]\nCellSpread=11\nPercentAtMax=1\n\
             Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        );
        let mut rules = RuleSet::from_ini(&ini).expect("band-11 rules");
        let wide = rules.warhead("WIDEWH").unwrap().clone();
        let mut stock = wide.clone();
        stock.cell_spread = SimFixed::from_num(10);
        stock.cell_spread_f64 = 10.0;

        let make_ground = |stable_id, rx, ry, sub_x, sub_y, hp| {
            let mut entity = GameEntity::test_default(stable_id, "TESTUNIT", "Soviet", rx, ry);
            entity.position.sub_x = SimFixed::from_num(sub_x);
            entity.position.sub_y = SimFixed::from_num(sub_y);
            entity.health.current = hp;
            entity
        };
        let mut entities = EntityStore::new();
        entities.insert(make_ground(100, 20, 20, 128, 128, 100));
        // Table indices 319 and 322 both visit (-3,+11). This exact
        // sub-cell coordinate is 2,763 leptons from the impact, within 2,816.
        entities.insert(make_ground(200, 17, 31, 255, 0, 60));
        entities.insert(make_ground(300, 23, 31, 0, 0, 100));
        entities.insert(make_ground(400, 17, 9, 255, 255, 100));

        let mut occupancy = OccupancyGrid::new();
        for &(id, rx, ry) in &[(100, 20, 20), (200, 17, 31), (300, 23, 31), (400, 17, 9)] {
            occupancy.add(
                rx,
                ry,
                id,
                MovementLayer::Ground,
                None,
                CellListInsertion::PrependNonBuilding,
            );
        }
        let cells = (0..48)
            .flat_map(|ry| (0..48).map(move |rx| test_terrain_cell(rx, ry)))
            .collect();
        let mut terrain = ResolvedTerrainGrid::from_cells(48, 48, cells);
        let mut interner = test_interner();
        let _handles =
            crate::sim::type_handle_table::ResolvedRuleHandles::resolve(&rules, &mut interner);
        let warhead_ref = interner.intern("WIDEWH");

        assert_eq!(
            cell_spread::splash_cells(wide.cell_spread)
                .iter()
                .enumerate()
                .filter_map(|(index, &offset)| (offset == (-3, 11)).then_some(index))
                .collect::<Vec<_>>(),
            vec![319, 322]
        );
        let stock_result = apply_aoe_damage(
            &mut entities,
            20,
            20,
            30,
            &stock,
            &rules,
            &mut interner,
            (crate::sim::combat::RAD_NO_ATTACKER, warhead_ref),
            AoELayerContext {
                occupancy: Some(&occupancy),
                terrain: Some(&mut terrain),
                impact_z: 0,
                ..AoELayerContext::default()
            },
        );
        assert_eq!(hit_ids(&stock_result.hits), vec![100]);

        let result = apply_aoe_damage(
            &mut entities,
            20,
            20,
            30,
            &wide,
            &rules,
            &mut interner,
            (crate::sim::combat::RAD_NO_ATTACKER, warhead_ref),
            AoELayerContext {
                occupancy: Some(&occupancy),
                terrain: Some(&mut terrain),
                impact_z: 0,
                ..AoELayerContext::default()
            },
        );
        assert_eq!(
            hit_ids(&result.hits),
            vec![100, 200, 300, 400, 200],
            "indices 319/320/321/322 retain the repeated receiver in place"
        );

        let events = result.hits;
        let mut main_rng = SimRng::new(7);
        let mut scenario_rng = SimRng::new(9);
        let mut handled_deaths = Vec::new();
        let mut houses = BTreeMap::new();
        let mut fatal_lifecycle = None;
        let mut sound_sink = None;
        let (death, pings) = crate::sim::combat::commit_damage_events(
            &events,
            &mut entities,
            &mut occupancy,
            &rules,
            &mut interner,
            &mut houses,
            &[],
            &HouseAllianceMap::new(),
            &mut main_rng,
            &mut scenario_rng,
            &mut handled_deaths,
            None,
            None,
            Some(&mut terrain),
            0,
            &mut fatal_lifecycle,
            &mut sound_sink,
        );
        assert!(pings.is_empty());
        assert_eq!(handled_deaths, vec![200]);
        assert_eq!(death.immediate_uninit_ids, vec![200]);
        assert_eq!(entities.get(200).unwrap().health.current, 0);
        assert_eq!(entities.get(100).unwrap().health.current, 70);
        assert_eq!(entities.get(300).unwrap().health.current, 70);
        assert_eq!(entities.get(400).unwrap().health.current, 70);
    }

    #[test]
    fn gsi_04_07_damage_center_first_seeded_strength_sweep() {
        let (rules, warhead, registry) = wall_aoe_fixture(".5", "Wall=yes");
        let mut entities = EntityStore::new();
        let mut interner = test_interner();
        let mut overlays = OverlayGrid::new(16, 16);
        overlays.place_overlay(8, 8, 0, 0);
        overlays.place_overlay(9, 7, 0, 0);
        let mut scenario_rng = SimRng::new(1);

        let mut reference_rng = SimRng::new(1);
        let center_roll = reference_rng.next_range_u32_inclusive(0, 400);
        let diagonal_roll = reference_rng.next_range_u32_inclusive(0, 400);
        assert_eq!(center_roll, 213, "seed fixes the first scanned-cell roll");

        let result = apply_aoe_damage(
            &mut entities,
            8,
            8,
            214,
            &warhead,
            &rules,
            &mut interner,
            "Americans",
            AoELayerContext {
                occupancy: None,
                terrain: None,
                overlay_grid: Some(&mut overlays),
                overlay_registry: Some(&registry),
                scenario_rng: Some(&mut scenario_rng),
                air_impact: None,
                impact_z: 0,
            },
        );

        assert_eq!(scenario_rng.state(), reference_rng.state());
        assert_eq!(overlays.cell(8, 8).overlay_id, None);
        assert_eq!(
            overlays.cell(9, 7).overlay_id.is_none(),
            diagonal_roll < 214,
            "the second RNG result belongs to the second splash-table cell"
        );
        let direct_cells: Vec<(u16, u16)> = result
            .wall_mutations
            .iter()
            .filter(|mutation| {
                matches!(
                    mutation.kind,
                    crate::sim::overlay_grid::WallMutationKind::DirectUpdated
                        | crate::sim::overlay_grid::WallMutationKind::DirectRemoved
                )
            })
            .map(|mutation| (mutation.rx, mutation.ry))
            .collect();
        let mut expected = vec![(8, 8)];
        if diagonal_roll < 214 {
            expected.push((9, 7));
        }
        assert_eq!(direct_cells, expected);
    }

    #[test]
    fn gsi_04_07_damage_fractional_spread_keeps_raw_i32_and_nuke_forces_all() {
        let (rules, wall, registry) = wall_aoe_fixture(".5", "Wall=yes");
        let mut entities = EntityStore::new();
        let mut interner = test_interner();
        let mut overlays = OverlayGrid::new(32, 32);
        overlays.place_overlay(16, 16, 0, 0);
        overlays.place_overlay(17, 15, 0, 0);
        let mut scenario_rng = SimRng::new(19);
        let before = scenario_rng.state();
        let result = apply_aoe_damage(
            &mut entities,
            16,
            16,
            65_537,
            &wall,
            &rules,
            &mut interner,
            "Americans",
            AoELayerContext {
                occupancy: None,
                terrain: None,
                overlay_grid: Some(&mut overlays),
                overlay_registry: Some(&registry),
                scenario_rng: Some(&mut scenario_rng),
                air_impact: None,
                impact_z: 0,
            },
        );
        assert_eq!(
            scenario_rng.state(),
            before,
            "raw 65537 must not wrap below Strength"
        );
        let direct_cells: Vec<(u16, u16)> = result
            .wall_mutations
            .iter()
            .filter(|mutation| {
                mutation.kind == crate::sim::overlay_grid::WallMutationKind::DirectRemoved
            })
            .map(|mutation| (mutation.rx, mutation.ry))
            .collect();
        assert_eq!(
            direct_cells,
            vec![(16, 16), (17, 15)],
            "CellSpread .5 reaches the adjacent wall and preserves center-first order"
        );

        let (rules, nuke, registry) = wall_aoe_fixture("10", "WallAbsoluteDestroyer=yes\nWall=yes");
        let mut overlays = OverlayGrid::new(40, 40);
        let stock_cells = [(20, 20), (20, 10), (30, 20), (17, 29)];
        for &(rx, ry) in &stock_cells {
            overlays.place_overlay(rx, ry, 2, 0);
        }
        let mut scenario_rng = SimRng::new(23);
        let before = scenario_rng.state();
        let result = apply_aoe_damage(
            &mut entities,
            20,
            20,
            -2,
            &nuke,
            &rules,
            &mut interner,
            "Americans",
            AoELayerContext {
                occupancy: None,
                terrain: None,
                overlay_grid: Some(&mut overlays),
                overlay_registry: Some(&registry),
                scenario_rng: Some(&mut scenario_rng),
                air_impact: None,
                impact_z: 0,
            },
        );
        assert_eq!(
            scenario_rng.state(),
            before,
            "WAD routes literal -1 on every spread cell"
        );
        for &(rx, ry) in &stock_cells {
            assert_eq!(overlays.cell(rx, ry).overlay_id, None);
            assert!(result.wall_mutations.iter().any(|mutation| {
                (mutation.rx, mutation.ry) == (rx, ry)
                    && mutation.kind == crate::sim::overlay_grid::WallMutationKind::DirectRemoved
            }));
        }
    }

    #[test]
    fn gsi_04_07_damage_pointer_expiry_is_cleanup_then_direct_forward_clear_first() {
        let (rules, warhead, registry) =
            wall_aoe_fixture("0", "WallAbsoluteDestroyer=yes\nWall=yes");
        let mut entities = EntityStore::new();
        let mut restored = GameEntity::test_default(20, "MTNK", "Americans", 2, 2);
        restored.attack_target = Some(AttackTarget::for_cell(8, 8));
        restored.mission.apply_test_fixture(MissionTestFixture {
            current: MissionId::from_known(MissionType::Attack),
            suspended: MissionId::from_known(MissionType::Guard),
            queued: MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: 0,
            ai_counter: 0,
            dispatch_timer: MissionDispatchTimer::at_frame(0),
        });
        restored.suspended_attack_target = Some(TargetKind::Cell(8, 8));
        entities.insert(restored);

        let mut cleared = GameEntity::test_default(30, "MTNK", "Americans", 3, 2);
        cleared.attack_target = Some(AttackTarget::for_cell(8, 8));
        cleared.passively_acquired_target = true;
        entities.insert(cleared);

        let mut chain_only = GameEntity::test_default(40, "MTNK", "Americans", 4, 2);
        chain_only.attack_target = Some(AttackTarget::for_cell(8, 7));
        entities.insert(chain_only);

        let mut interner = test_interner();
        let owner = crate::sim::intern::InternedId::from_index(7);
        let mut overlays = OverlayGrid::new(16, 16);
        overlays.place_overlay(8, 8, 2, 0);
        overlays.place_owned_wall(8, 7, 2, 0x24, owner);
        let mut scenario_rng = SimRng::new(5);
        let result = apply_aoe_damage(
            &mut entities,
            8,
            8,
            1,
            &warhead,
            &rules,
            &mut interner,
            "Americans",
            AoELayerContext {
                occupancy: None,
                terrain: None,
                overlay_grid: Some(&mut overlays),
                overlay_registry: Some(&registry),
                scenario_rng: Some(&mut scenario_rng),
                air_impact: None,
                impact_z: 0,
            },
        );

        assert_eq!(overlays.cell(8, 8).overlay_id, None);
        assert_eq!(
            overlays.cell(8, 7).overlay_id,
            None,
            "cleanup created this removal"
        );
        assert_eq!(
            result.cell_target_detaches,
            vec![
                CellTargetDetach {
                    listener_id: 40,
                    restored: false,
                    cleared: true,
                },
                CellTargetDetach {
                    listener_id: 20,
                    restored: true,
                    cleared: true,
                },
                CellTargetDetach {
                    listener_id: 30,
                    restored: false,
                    cleared: true,
                },
            ],
            "cleanup expiry precedes the direct callback; each roster walk is forward and clears before Restore"
        );
        assert!(entities.get(30).unwrap().attack_target.is_none());
        assert!(!entities.get(30).unwrap().passively_acquired_target);
        assert!(matches!(
            entities
                .get(20)
                .unwrap()
                .attack_target
                .as_ref()
                .map(|target| target.target),
            Some(TargetKind::Cell(8, 8))
        ));
        assert!(entities.get(40).unwrap().attack_target.is_none());
    }

    #[test]
    fn test_aoe_beyond_radius() {
        // Beyond radius clamped to t=1 → percent_at_max.
        let dmg = aoe_damage_at_distance(100, sim_from_f32(5.0), sim_from_f32(3.0), 25, 100);
        assert_eq!(dmg, 25);
    }

    #[test]
    fn bridge_impact_above_threshold_damages_only_bridge_layer() {
        let (mut entities, occupancy, mut terrain, rules, warhead, mut interner) =
            bridge_layer_test_fixture();
        let hits = apply_aoe_damage(
            &mut entities,
            5,
            5,
            100,
            &warhead,
            &rules,
            &mut interner,
            "Americans",
            AoELayerContext {
                occupancy: Some(&occupancy),
                terrain: Some(&mut terrain),
                overlay_grid: None,
                overlay_registry: None,
                scenario_rng: None,
                air_impact: None,
                impact_z: 4,
            },
        )
        .hits;

        assert_eq!(hit_ids(&hits), vec![2]);
    }

    #[test]
    fn bridge_impact_at_threshold_stays_on_ground_layer() {
        let (mut entities, occupancy, mut terrain, rules, warhead, mut interner) =
            bridge_layer_test_fixture();
        let hits = apply_aoe_damage(
            &mut entities,
            5,
            5,
            100,
            &warhead,
            &rules,
            &mut interner,
            "Americans",
            AoELayerContext {
                occupancy: Some(&occupancy),
                terrain: Some(&mut terrain),
                overlay_grid: None,
                overlay_registry: None,
                scenario_rng: None,
                air_impact: None,
                // Exactly at the half-deck mid-height (ground_z 0 + DECK/2 = 2).
                // Verified deck = 4 levels → half-deck = 2; strict `>` keeps the
                // boundary on the ground list.
                impact_z: 2,
            },
        )
        .hits;

        assert_eq!(hit_ids(&hits), vec![1]);
    }

    #[test]
    fn bridge_adjusted_impact_z_adds_height_only_at_call_site() {
        let (_, _, terrain, _, _, _) = bridge_layer_test_fixture();
        assert_eq!(bridge_adjusted_impact_z(Some(&terrain), 4, 4), 0);
        // Structural cell (5,5): ground level 0 + verified full deck height
        // (BRIDGE_DECK_HEIGHT_LEVELS = 4).
        assert_eq!(bridge_adjusted_impact_z(Some(&terrain), 5, 5), 4);
        assert_eq!(bridge_adjusted_impact_z(None, 5, 5), 0);
    }

    #[test]
    fn gsi_04_07_damage_receiver_pipeline_source_admission_and_house_snapshot() {
        fn run(
            damage_self: bool,
            warhead_name: &str,
        ) -> (
            Vec<u64>,
            Vec<Option<crate::sim::intern::InternedId>>,
            [i32; 3],
        ) {
            let ini = IniFile::from_str(&format!(
                "[InfantryTypes]\n\
                 [VehicleTypes]\n0=SOURCE\n1=TARGET\n\
                 [AircraftTypes]\n\
                 [BuildingTypes]\n\
                 [Warheads]\n0=OrdinaryWH\n1=CrushWH\n\
                 [CombatDamage]\nCrushWarhead=CrushWH\nMaxDamage=10000\n\
                 [SOURCE]\nStrength=100\nArmor=heavy\nDamageSelf={}\n\
                 [TARGET]\nStrength=100\nArmor=heavy\n\
                 [OrdinaryWH]\nCellSpread=0\nAffectsAllies=no\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
                 [CrushWH]\nCellSpread=0\nAffectsAllies=no\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
                if damage_self { "yes" } else { "no" }
            ));
            let mut rules = RuleSet::from_ini(&ini).expect("receiver admission fixture");
            let warhead = rules.warhead(warhead_name).unwrap().clone();
            let mut interner = test_interner();
            let _handles =
                crate::sim::type_handle_table::ResolvedRuleHandles::resolve(&rules, &mut interner);
            let warhead_ref = interner.intern(warhead_name);
            let source_house = interner.intern("SourceHouse");
            let ally_house = interner.intern("AllyHouse");
            let enemy_house = interner.intern("EnemyHouse");
            let source_type = interner.intern("SOURCE");
            let target_type = interner.intern("TARGET");

            let mut entities = EntityStore::new();
            let mut source = GameEntity::test_default(1, "SOURCE", "SourceHouse", 1, 1);
            source.type_ref = source_type;
            source.owner = source_house;
            entities.insert(source);
            let mut ally = GameEntity::test_default(2, "TARGET", "AllyHouse", 1, 1);
            ally.type_ref = target_type;
            ally.owner = ally_house;
            entities.insert(ally);
            let mut enemy = GameEntity::test_default(3, "TARGET", "EnemyHouse", 1, 1);
            enemy.type_ref = target_type;
            enemy.owner = enemy_house;
            entities.insert(enemy);
            let mut occupancy = OccupancyGrid::new();
            for stable_id in [3, 2, 1] {
                occupancy.add(
                    1,
                    1,
                    stable_id,
                    MovementLayer::Ground,
                    None,
                    CellListInsertion::PrependNonBuilding,
                );
            }
            let cells = (0..3)
                .flat_map(|ry| (0..3).map(move |rx| test_terrain_cell(rx, ry)))
                .collect();
            let mut terrain = ResolvedTerrainGrid::from_cells(3, 3, cells);
            let aoe = apply_aoe_damage(
                &mut entities,
                1,
                1,
                40,
                &warhead,
                &rules,
                &mut interner,
                (1, Some(source_house), warhead_ref),
                AoELayerContext {
                    occupancy: Some(&occupancy),
                    terrain: Some(&mut terrain),
                    impact_z: 0,
                    ..AoELayerContext::default()
                },
            );
            let ids = hit_ids(&aoe.hits);
            let captured_houses = aoe.hits.iter().map(|event| event.source_house).collect();

            // Prove alliance resolution reads the detonation ABI snapshot, not
            // a later live-source owner value.
            entities.get_mut(1).unwrap().owner = enemy_house;
            let mut alliances = HouseAllianceMap::new();
            alliances
                .entry("SOURCEHOUSE".to_string())
                .or_default()
                .insert("ALLYHOUSE".to_string());
            let mut main_rng = SimRng::new(1);
            let mut scenario_rng = SimRng::new(2);
            let mut handled_deaths = Vec::new();
            let mut houses = BTreeMap::new();
            let mut fatal_lifecycle = None;
            let mut sound_sink = None;
            let (death, pings) = crate::sim::combat::commit_damage_events(
                &aoe.hits,
                &mut entities,
                &mut occupancy,
                &rules,
                &mut interner,
                &mut houses,
                &[],
                &alliances,
                &mut main_rng,
                &mut scenario_rng,
                &mut handled_deaths,
                None,
                None,
                Some(&mut terrain),
                0,
                &mut fatal_lifecycle,
                &mut sound_sink,
            );
            assert!(death.despawned_ids.is_empty());
            assert!(pings.is_empty());
            (
                ids,
                captured_houses,
                [
                    entities.get(1).unwrap().health.current,
                    entities.get(2).unwrap().health.current,
                    entities.get(3).unwrap().health.current,
                ],
            )
        }

        let source_house = test_intern("SourceHouse");
        let (ordinary_ids, ordinary_houses, ordinary_hp) = run(false, "OrdinaryWH");
        assert_eq!(ordinary_ids, vec![2, 3]);
        assert_eq!(ordinary_houses, vec![Some(source_house); 2]);
        assert_eq!(ordinary_hp, [100, 100, 60]);

        let (damage_self_ids, _, damage_self_hp) = run(true, "OrdinaryWH");
        assert_eq!(damage_self_ids, vec![1, 2, 3]);
        assert_eq!(damage_self_hp, [60, 100, 60]);

        let (crush_ids, _, crush_hp) = run(false, "CrushWH");
        assert_eq!(crush_ids, vec![1, 2, 3]);
        assert_eq!(crush_hp, [60, 100, 60]);
    }

    #[test]
    fn gsi_04_07_damage_object_immune_short_circuits_before_hp() {
        fn run(immune: bool) -> (i32, usize, crate::sim::combat::DeathEffects) {
            let ini = IniFile::from_str(&format!(
                "[InfantryTypes]\n\
                 [VehicleTypes]\n0=MTNK\n\
                 [AircraftTypes]\n\
                 [BuildingTypes]\n0=CABHUT\n\
                 [Warheads]\n0=AP\n\
                 [MTNK]\nStrength=300\nArmor=heavy\nPrimary=105mm\n\
                 [CABHUT]\nStrength=2000\nArmor=none\nCanC4=yes\nImmune={}\n\
                 [105mm]\nDamage=65\nWarhead=AP\n\
                 [AP]\nCellSpread=0\nPercentAtMax=1\n\
                 Verses=25%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
                if immune { "yes" } else { "no" }
            ));
            let rules = RuleSet::from_ini(&ini).expect("CABHUT Immune fixture");
            let warhead = rules.warhead("AP").unwrap().clone();
            assert_eq!(rules.object("CABHUT").unwrap().immune, immune);
            assert_eq!(rules.weapon("105mm").unwrap().damage, 65);

            let mut interner = test_interner();
            let soviet = interner.intern("SovietHouse");
            let civilian = interner.intern("CivilianHouse");
            let mtnk = interner.intern("MTNK");
            let cabhut = interner.intern("CABHUT");
            let ap = interner.intern("AP");
            let mut entities = EntityStore::new();
            let mut source = GameEntity::test_default(1, "MTNK", "SovietHouse", 1, 1);
            source.owner = soviet;
            source.type_ref = mtnk;
            entities.insert(source);
            let mut hut = GameEntity::test_default(2, "CABHUT", "CivilianHouse", 5, 5);
            hut.owner = civilian;
            hut.type_ref = cabhut;
            hut.category = EntityCategory::Structure;
            hut.health.current = 2000;
            entities.insert(hut);

            let mut occupancy = OccupancyGrid::new();
            occupancy.add(
                5,
                5,
                2,
                MovementLayer::Ground,
                None,
                CellListInsertion::AppendBuilding,
            );
            let cells = (0..8)
                .flat_map(|ry| (0..8).map(move |rx| test_terrain_cell(rx, ry)))
                .collect();
            let mut terrain = ResolvedTerrainGrid::from_cells(8, 8, cells);
            let aoe = apply_aoe_damage(
                &mut entities,
                5,
                5,
                65,
                &warhead,
                &rules,
                &mut interner,
                (1, Some(soviet), ap),
                AoELayerContext {
                    occupancy: Some(&occupancy),
                    terrain: Some(&mut terrain),
                    impact_z: 0,
                    ..AoELayerContext::default()
                },
            );
            assert_eq!(
                aoe.hits
                    .iter()
                    .map(|event| (event.target_id, event.damage, event.distance_leptons))
                    .collect::<Vec<_>>(),
                vec![(2, 65, Some(0))],
                "Immune remains an ordered receiver record, not a collector filter"
            );

            let mut main_rng = SimRng::new(1);
            let mut scenario_rng = SimRng::new(2);
            let mut handled_deaths = Vec::new();
            let mut houses = BTreeMap::new();
            let mut fatal_lifecycle = None;
            let mut sound_sink = None;
            let (death, pings) = crate::sim::combat::commit_damage_events(
                &aoe.hits,
                &mut entities,
                &mut occupancy,
                &rules,
                &mut interner,
                &mut houses,
                &[],
                &HouseAllianceMap::new(),
                &mut main_rng,
                &mut scenario_rng,
                &mut handled_deaths,
                None,
                None,
                Some(&mut terrain),
                0,
                &mut fatal_lifecycle,
                &mut sound_sink,
            );
            (entities.get(2).unwrap().health.current, pings.len(), death)
        }

        let (immune_hp, immune_pings, immune_death) = run(true);
        assert_eq!(immune_hp, 2000);
        assert_eq!(immune_pings, 0);
        assert!(immune_death.despawned_ids.is_empty());
        assert!(immune_death.immediate_uninit_ids.is_empty());
        assert!(immune_death.explosion_effects.is_empty());

        let (ordinary_hp, ordinary_pings, ordinary_death) = run(false);
        assert_eq!(ordinary_hp, 1984, "ftol(65 x AP none 25%) = 16");
        assert_eq!(ordinary_pings, 1, "ordinary enemy structure hit pings");
        assert!(ordinary_death.despawned_ids.is_empty());
    }

    #[test]
    fn gsi_04_07_damage_live_defender_stronger_uses_veteran_armor() {
        let ini = IniFile::from_str(
            "[General]\nVeteranArmor=1.5\n\
             [Normal]\nArmor=1.0\n\
             [Countries]\n0=Americans\n1=Russians\n\
             [Americans]\nArmor=1.0\nArmorUnitsMult=1.0\n\
             [Russians]\n\
             [InfantryTypes]\n\
             [VehicleTypes]\n0=HTNK\n1=MTNK\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [HTNK]\nStrength=400\nArmor=heavy\nPrimary=RhinoGun\n\
             [MTNK]\nStrength=300\nArmor=heavy\nVeteranAbilities=STRONGER\nEliteAbilities=STRONGER\n\
             [RhinoGun]\nDamage=90\nWarhead=AP\n\
             [AP]\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("Rhino/Grizzly receiver fixture");
        let warhead = rules.warhead("AP").expect("AP").clone();
        assert_eq!(rules.weapon("RhinoGun").unwrap().damage, 90);
        assert!(rules.object("MTNK").unwrap().veteran_stronger);
        assert_eq!(rules.general.veteran_armor, 1.5);

        let mut interner = test_interner();
        let russian = interner.intern("Russians");
        let american = interner.intern("Americans");
        let htnk = interner.intern("HTNK");
        let mtnk = interner.intern("MTNK");
        let ap = interner.intern("AP");
        let mut entities = EntityStore::new();
        let mut source = GameEntity::test_default(1, "HTNK", "Russians", 1, 1);
        source.owner = russian;
        source.type_ref = htnk;
        entities.insert(source);
        let mut rookie = GameEntity::test_default(2, "MTNK", "Americans", 5, 5);
        rookie.owner = american;
        rookie.type_ref = mtnk;
        rookie.health.current = 300;
        entities.insert(rookie);
        let mut veteran = GameEntity::test_default(3, "MTNK", "Americans", 5, 5);
        veteran.owner = american;
        veteran.type_ref = mtnk;
        veteran.health.current = 300;
        veteran.veterancy = 100;
        entities.insert(veteran);

        let mut occupancy = OccupancyGrid::new();
        for stable_id in [3, 2] {
            occupancy.add(
                5,
                5,
                stable_id,
                MovementLayer::Ground,
                None,
                CellListInsertion::PrependNonBuilding,
            );
        }
        let cells = (0..8)
            .flat_map(|ry| (0..8).map(move |rx| test_terrain_cell(rx, ry)))
            .collect();
        let mut terrain = ResolvedTerrainGrid::from_cells(8, 8, cells);
        let aoe = apply_aoe_damage(
            &mut entities,
            5,
            5,
            90,
            &warhead,
            &rules,
            &mut interner,
            (1, Some(russian), ap),
            AoELayerContext {
                occupancy: Some(&occupancy),
                terrain: Some(&mut terrain),
                impact_z: 0,
                ..AoELayerContext::default()
            },
        );
        assert_eq!(
            aoe.hits
                .iter()
                .map(|event| (event.target_id, event.damage, event.distance_leptons))
                .collect::<Vec<_>>(),
            vec![(2, 90, Some(0)), (3, 90, Some(0))],
            "Apply_area_damage records keep raw Rhino damage for the ordered receiver"
        );

        let mut houses = BTreeMap::new();
        let mut target_house = HouseState::new(american, 0, Some(american), true, 0, 10);
        target_house.difficulty = HouseDifficulty::Normal;
        houses.insert(american, target_house);
        let mut main_rng = SimRng::new(1);
        let mut scenario_rng = SimRng::new(2);
        let mut handled_deaths = Vec::new();
        let mut fatal_lifecycle = None;
        let mut sound_sink = None;
        let (death, pings) = crate::sim::combat::commit_damage_events(
            &aoe.hits,
            &mut entities,
            &mut occupancy,
            &rules,
            &mut interner,
            &mut houses,
            &[],
            &HouseAllianceMap::new(),
            &mut main_rng,
            &mut scenario_rng,
            &mut handled_deaths,
            None,
            None,
            Some(&mut terrain),
            0,
            &mut fatal_lifecycle,
            &mut sound_sink,
        );

        assert_eq!(entities.get(2).unwrap().health.current, 210);
        assert_eq!(
            entities.get(3).unwrap().health.current,
            240,
            "STRONGER divides 90 by stock VeteranArmor 1.5 at the live receiver"
        );
        assert!(death.despawned_ids.is_empty());
        assert!(pings.is_empty());
    }

    #[test]
    fn gsi_04_07_damage_psychgas_commits_berserk_transaction() {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n\
             [VehicleTypes]\n0=CAOS\n1=TARGET\n2=IMMUNE\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n0=BUILD\n\
             [Warheads]\n0=PsychGasCreate\n1=PsychGas\n\
             [CAOS]\nStrength=100\nArmor=heavy\nPrimary=ChaosAttack\n\
             [TARGET]\nStrength=1000\nArmor=heavy\nImmuneToPsionics=no\n\
             [IMMUNE]\nStrength=1000\nArmor=heavy\nImmuneToPsionics=yes\n\
             [BUILD]\nStrength=1000\nArmor=heavy\nImmuneToPsionics=no\n\
             [ChaosAttack]\nDamage=600\nWarhead=PsychGasCreate\n\
             [PsychGasCreate]\nCellSpread=3\nPercentAtMax=1\nPsychedelic=yes\nAffectsAllies=yes\n\
             Verses=100%,100%,100%,100%,100%,50%,100%,100%,100%,100%,100%\n\
             [PsychGas]\nCellSpread=3\nPercentAtMax=1\nPsychedelic=yes\nAffectsAllies=yes\n\
             Verses=100%,100%,100%,100%,100%,50%,100%,100%,100%,100%,100%\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("stock-like Chaos gas fixture");
        let warhead = rules.warhead("PsychGasCreate").unwrap().clone();
        assert_eq!(rules.weapon("ChaosAttack").unwrap().damage, 600);
        assert_eq!(warhead.verses_f64[5], 0.5);
        assert_eq!(rules.warhead("PsychGas").unwrap().verses_f64[5], 0.5);

        let mut interner = test_interner();
        let yuri = interner.intern("YuriHouse");
        let soviet = interner.intern("SovietHouse");
        let caos = interner.intern("CAOS");
        let target_type = interner.intern("TARGET");
        let immune_type = interner.intern("IMMUNE");
        let building_type = interner.intern("BUILD");
        let psych_gas_create = interner.intern("PsychGasCreate");

        let mut entities = EntityStore::new();
        let mut source = GameEntity::test_default(1, "CAOS", "YuriHouse", 1, 1);
        source.owner = yuri;
        source.type_ref = caos;
        entities.insert(source);

        let mut target = GameEntity::test_default(2, "TARGET", "SovietHouse", 5, 5);
        target.owner = soviet;
        target.type_ref = target_type;
        target.health.current = 1000;
        target.attack_target = Some(AttackTarget::new(1));
        entities.insert(target);

        let mut allied = GameEntity::test_default(3, "TARGET", "YuriHouse", 5, 5);
        allied.owner = yuri;
        allied.type_ref = target_type;
        allied.health.current = 1000;
        allied.attack_target = Some(AttackTarget::new(1));
        entities.insert(allied);

        let mut immune = GameEntity::test_default(4, "IMMUNE", "SovietHouse", 5, 5);
        immune.owner = soviet;
        immune.type_ref = immune_type;
        immune.health.current = 1000;
        immune.attack_target = Some(AttackTarget::new(1));
        entities.insert(immune);

        let mut building = GameEntity::test_default(5, "BUILD", "SovietHouse", 5, 5);
        building.owner = soviet;
        building.type_ref = building_type;
        building.category = EntityCategory::Structure;
        building.health.current = 1000;
        building.attack_target = Some(AttackTarget::new(1));
        entities.insert(building);

        let mut occupancy = OccupancyGrid::new();
        for stable_id in [4, 3, 2] {
            occupancy.add(
                5,
                5,
                stable_id,
                MovementLayer::Ground,
                None,
                CellListInsertion::PrependNonBuilding,
            );
        }
        occupancy.add(
            5,
            5,
            5,
            MovementLayer::Ground,
            None,
            CellListInsertion::AppendBuilding,
        );
        let cells = (0..12)
            .flat_map(|ry| (0..12).map(move |rx| test_terrain_cell(rx, ry)))
            .collect();
        let mut terrain = ResolvedTerrainGrid::from_cells(12, 12, cells);
        let aoe = apply_aoe_damage(
            &mut entities,
            5,
            5,
            rules.weapon("ChaosAttack").unwrap().damage,
            &warhead,
            &rules,
            &mut interner,
            (1, Some(yuri), psych_gas_create),
            AoELayerContext {
                occupancy: Some(&occupancy),
                terrain: Some(&mut terrain),
                impact_z: 0,
                ..AoELayerContext::default()
            },
        );
        assert_eq!(
            aoe.hits
                .iter()
                .map(|event| (event.target_id, event.damage, event.distance_leptons))
                .collect::<Vec<_>>(),
            vec![
                (2, 600, Some(0)),
                (3, 600, Some(0)),
                (4, 600, Some(0)),
                (5, 600, Some(0)),
            ],
            "Apply_area_damage preserves raw ordered records; Psychedelic gates live in the receiver"
        );

        let mut main_rng = SimRng::new(1);
        let mut scenario_rng = SimRng::new(2);
        let mut handled_deaths = Vec::new();
        let mut houses = BTreeMap::new();
        let mut fatal_lifecycle = None;
        let mut sound_sink = None;
        let (death, pings) = crate::sim::combat::commit_damage_events(
            &aoe.hits,
            &mut entities,
            &mut occupancy,
            &rules,
            &mut interner,
            &mut houses,
            &[],
            &HouseAllianceMap::new(),
            &mut main_rng,
            &mut scenario_rng,
            &mut handled_deaths,
            None,
            None,
            Some(&mut terrain),
            0,
            &mut fatal_lifecycle,
            &mut sound_sink,
        );
        assert!(death.despawned_ids.is_empty());
        assert!(pings.is_empty());

        let target = entities.get(2).unwrap();
        assert_eq!(target.health.current, 1000, "Psychedelic skips Object HP");
        assert!(target.berserk.active);
        assert_eq!(target.berserk.timer, 300, "600 x heavy Verses 50%");
        assert!(target.attack_target.is_none());
        assert_eq!(target.mission.current(), MissionId::NONE);
        assert_eq!(
            target.mission.queued(),
            MissionId::from_known(MissionType::Hunt)
        );

        for stable_id in [3, 4, 5] {
            let control = entities.get(stable_id).unwrap();
            assert_eq!(control.health.current, 1000);
            assert_eq!(control.berserk, Default::default());
            assert_eq!(
                control.attack_target.as_ref().map(|target| target.target),
                Some(TargetKind::Entity(1))
            );
            assert_eq!(control.mission.queued(), MissionId::NONE);
        }

        // A later gas record refreshes only +0x29C: it does not repeat target
        // clearing or mission queue callbacks while +0x298 remains active.
        {
            let target = entities.get_mut(2).unwrap();
            target.attack_target = Some(AttackTarget::new(1));
            target.berserk.timer = 17;
            target.mission.apply_test_fixture(MissionTestFixture {
                current: MissionId::from_known(MissionType::Guard),
                suspended: MissionId::NONE,
                queued: MissionId::NONE,
                movement_bypass_latch: 0,
                handler_state: 0,
                mission_start_frame: 0,
                ai_counter: 0,
                dispatch_timer: MissionDispatchTimer::at_frame(0),
            });
        }
        let (refresh_death, refresh_pings) = crate::sim::combat::commit_damage_events(
            &aoe.hits[..1],
            &mut entities,
            &mut occupancy,
            &rules,
            &mut interner,
            &mut houses,
            &[],
            &HouseAllianceMap::new(),
            &mut main_rng,
            &mut scenario_rng,
            &mut handled_deaths,
            None,
            None,
            Some(&mut terrain),
            0,
            &mut fatal_lifecycle,
            &mut sound_sink,
        );
        let target = entities.get(2).unwrap();
        assert!(refresh_death.despawned_ids.is_empty());
        assert!(refresh_pings.is_empty());
        assert_eq!(target.health.current, 1000);
        assert_eq!(target.berserk.timer, 300);
        assert_eq!(
            target.attack_target.as_ref().map(|target| target.target),
            Some(TargetKind::Entity(1))
        );
        assert_eq!(target.mission.queued(), MissionId::NONE);
    }

    #[test]
    fn gsi_04_07_damage_prone_raw_scaling_precedes_area_receiver_pipeline() {
        let ini = IniFile::from_str(
            "[General]\nFixtureOnly=1\n\
             [Normal]\nArmor=1.0\n\
             [Easy]\nArmor=1.2\n\
             [InfantryTypes]\n0=E1\n\
             [VehicleTypes]\n0=MTNK\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [MTNK]\nStrength=300\nArmor=heavy\nPrimary=105mmE\n\
             [E1]\nStrength=100\nArmor=none\n\
             [105mmE]\nDamage=65\nWarhead=GRIZAPE\n\
             [GRIZAPE]\nCellSpread=.3\nPercentAtMax=.5\nProneDamage=50%\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("105mmE/GRIZAPE prone receiver fixture");
        let warhead = rules.warhead("GRIZAPE").expect("GRIZAPE").clone();
        assert_eq!(rules.weapon("105mmE").unwrap().damage, 65);
        assert_eq!(warhead.prone_damage_f64, 0.5);

        let mut interner = test_interner();
        let source_house = interner.intern("SourceHouse");
        let normal_house = interner.intern("NormalHouse");
        let easy_house = interner.intern("EasyHouse");
        let mtnk = interner.intern("MTNK");
        let e1 = interner.intern("E1");
        let grizape = interner.intern("GRIZAPE");
        let mut entities = EntityStore::new();
        let mut source = GameEntity::test_default(1, "MTNK", "SourceHouse", 1, 1);
        source.owner = source_house;
        source.type_ref = mtnk;
        entities.insert(source);

        let mut target = GameEntity::test_default(2, "E1", "NormalHouse", 5, 5);
        target.owner = normal_house;
        target.type_ref = e1;
        target.category = EntityCategory::Infantry;
        target.health.current = 20;
        target.infantry = Some(crate::sim::game_entity::InfantryRuntime::new());
        target.infantry.as_mut().unwrap().is_prone = true;
        entities.insert(target);

        let mut easy_prone = GameEntity::test_default(3, "E1", "EasyHouse", 5, 5);
        easy_prone.owner = easy_house;
        easy_prone.type_ref = e1;
        easy_prone.category = EntityCategory::Infantry;
        easy_prone.health.current = 100;
        easy_prone.infantry = Some(crate::sim::game_entity::InfantryRuntime::new());
        easy_prone.infantry.as_mut().unwrap().is_prone = true;
        entities.insert(easy_prone);

        let mut easy_standing = GameEntity::test_default(4, "E1", "EasyHouse", 5, 5);
        easy_standing.owner = easy_house;
        easy_standing.type_ref = e1;
        easy_standing.category = EntityCategory::Infantry;
        easy_standing.health.current = 100;
        easy_standing.infantry = Some(crate::sim::game_entity::InfantryRuntime::new());
        entities.insert(easy_standing);

        let mut occupancy = OccupancyGrid::new();
        for stable_id in [4, 3, 2] {
            occupancy.add(
                5,
                5,
                stable_id,
                MovementLayer::Ground,
                None,
                CellListInsertion::PrependNonBuilding,
            );
        }
        let cells = (0..8)
            .flat_map(|ry| (0..8).map(move |rx| test_terrain_cell(rx, ry)))
            .collect();
        let mut terrain = ResolvedTerrainGrid::from_cells(8, 8, cells);
        let aoe = apply_aoe_damage(
            &mut entities,
            5,
            5,
            65,
            &warhead,
            &rules,
            &mut interner,
            (1, Some(source_house), grizape),
            AoELayerContext {
                occupancy: Some(&occupancy),
                terrain: Some(&mut terrain),
                impact_z: 0,
                ..AoELayerContext::default()
            },
        );
        assert_eq!(
            aoe.hits
                .iter()
                .map(|event| (event.target_id, event.damage, event.distance_leptons))
                .collect::<Vec<_>>(),
            vec![(2, 65, Some(0)), (3, 65, Some(0)), (4, 65, Some(0))],
            "collection preserves stock 105mmE raw records"
        );

        let mut houses = BTreeMap::new();
        let mut normal = HouseState::new(normal_house, 0, None, false, 0, 10);
        normal.difficulty = HouseDifficulty::Normal;
        houses.insert(normal_house, normal);
        let mut easy = HouseState::new(easy_house, 0, None, false, 0, 10);
        easy.difficulty = HouseDifficulty::Easy;
        houses.insert(easy_house, easy);
        let mut main_rng = SimRng::new(1);
        let mut scenario_rng = SimRng::new(2);
        let mut handled_deaths = Vec::new();
        let mut fatal_lifecycle = None;
        let mut sound_sink = None;
        let (death, _) = crate::sim::combat::commit_damage_events(
            &aoe.hits,
            &mut entities,
            &mut occupancy,
            &rules,
            &mut interner,
            &mut houses,
            &[],
            &HouseAllianceMap::new(),
            &mut main_rng,
            &mut scenario_rng,
            &mut handled_deaths,
            None,
            None,
            Some(&mut terrain),
            0,
            &mut fatal_lifecycle,
            &mut sound_sink,
        );

        assert_eq!(entities.get(2).unwrap().health.current, 0);
        assert!(death.despawned_ids.contains(&2));
        assert_eq!(
            entities.get(3).unwrap().health.current,
            74,
            "ftol(65 * .5)=32, then Easy armor 1.2 yields 26 (not 27)"
        );
        assert_eq!(
            entities.get(4).unwrap().health.current,
            46,
            "standing Infantry bypasses the concrete prone pre-scale"
        );
    }

    #[test]
    fn gsi_04_07_damage_nonfatal_infantry_scatter_precedes_fear() {
        #[derive(Debug)]
        struct Outcome {
            health: i32,
            fear: u16,
            destination: Option<(u16, u16)>,
            queued_mission: MissionId,
            rng_changed: bool,
            rng_indices: serde_json::Value,
        }

        #[derive(Default)]
        struct ScatterFixture {
            doing: i32,
            animation: Option<crate::sim::animation::SequenceKind>,
            first_bridge: bool,
            all_bridges: bool,
            head: Option<crate::sim::components::DriveCoord>,
            outside: bool,
            entry_case: Option<serde_json::Value>,
        }

        fn run(
            mission: MissionType,
            attacker_present: bool,
            health: i32,
            fraidycat: bool,
            human_control: Option<bool>,
            team: bool,
            nav: bool,
        ) -> Outcome {
            run_fixture(
                mission,
                attacker_present,
                health,
                fraidycat,
                human_control,
                team,
                nav,
                ScatterFixture {
                    doing: -1,
                    ..Default::default()
                },
            )
        }

        fn run_fixture(
            mission: MissionType,
            attacker_present: bool,
            health: i32,
            fraidycat: bool,
            human_control: Option<bool>,
            team: bool,
            nav: bool,
            fixture: ScatterFixture,
        ) -> Outcome {
            let ini = IniFile::from_str(&format!(
                "[General]\nFixture=1\n[InfantryTypes]\n0=E1\n\
                 [VehicleTypes]\n0=ATTACKER\n\
                 [AircraftTypes]\n\
                 [BuildingTypes]\n\
                 [Warheads]\n0=WH\n\
                 [IQ]\nScatter=99\n\
                 [CombatDamage]\nPlayerScatter=no\nMaxDamage=10000\n\
                 [Guard]\nScatter=yes\n\
                 [Attack]\nScatter=no\n\
                 [E1]\nStrength=125\nArmor=none\nSpeed=4\nFraidycat={fraidycat}\n\
                 Locomotor={{4A582744-9839-11d1-B709-00A024DDAFD1}}\n\
                 MovementZone=Infantry\nSpeedType=Foot\n\
                 [ATTACKER]\nStrength=100\nArmor=none\n\
                 [WH]\nCellSpread=0\nPercentAtMax=1\n\
                 Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
            ));
            let rules = RuleSet::from_ini(&ini).expect("damage-scatter fixture");
            let mut interner = test_interner();
            let attacker_house = interner.intern("AttackerHouse");
            let victim_house = interner.intern("VictimHouse");
            let attacker_type = interner.intern("ATTACKER");
            let victim_type = interner.intern("E1");
            let warhead_ref = interner.intern("WH");

            let mut entities = EntityStore::new();
            let mut attacker = GameEntity::test_default(1, "ATTACKER", "AttackerHouse", 3, 5);
            attacker.owner = attacker_house;
            attacker.type_ref = attacker_type;
            entities.insert(attacker);

            let mut victim = GameEntity::test_default(2, "E1", "VictimHouse", 5, 5);
            victim.owner = victim_house;
            victim.type_ref = victim_type;
            victim.category = EntityCategory::Infantry;
            victim.mission_leaf = crate::sim::mission::leaf::MissionLeafState::for_entity_category(
                EntityCategory::Infantry,
            );
            victim
                .mission_leaf
                .set_infantry_doing_verified(fixture.doing)
                .unwrap();
            victim.animation = fixture.animation.map(crate::sim::animation::Animation::new);
            victim.health.current = health;
            victim.sub_cell = Some(2);
            victim.on_bridge = fixture
                .entry_case
                .as_ref()
                .and_then(|case| case["on_bridge"].as_bool())
                .unwrap_or(false);
            victim.infantry = Some(crate::sim::game_entity::InfantryRuntime::new());
            if nav {
                victim.navigation.nav_com = Some(crate::sim::components::NavTargetRef::cell(8, 8));
            }
            victim.locomotor = Some(
                crate::sim::movement::locomotor::LocomotorState::from_object_type(
                    rules.object("E1").expect("E1 type"),
                    0,
                ),
            );
            if let Some(head) = fixture.head {
                victim.locomotor.as_mut().unwrap().set_step_head(Some(head));
            }
            victim.mission.apply_test_fixture(MissionTestFixture {
                current: MissionId::from_known(mission),
                suspended: MissionId::NONE,
                queued: MissionId::NONE,
                movement_bypass_latch: 0,
                handler_state: 0,
                mission_start_frame: 0,
                ai_counter: 0,
                dispatch_timer: MissionDispatchTimer::at_frame(0),
            });
            entities.insert(victim);

            let mut occupancy = OccupancyGrid::new();
            occupancy.add(
                5,
                5,
                2,
                MovementLayer::Ground,
                Some(2),
                CellListInsertion::PrependNonBuilding,
            );
            let cells = (0..10)
                .flat_map(|ry| {
                    (0..10).map(move |rx| {
                        let mut cell = test_terrain_cell(rx, ry);
                        cell.speed_costs.foot = Some(100);
                        cell
                    })
                })
                .collect();
            let mut terrain = ResolvedTerrainGrid::from_cells(10, 10, cells);
            if fixture.first_bridge {
                terrain.cell_mut(6, 4).unwrap().bridge_facts.raw_flags = 0x100;
            }
            if fixture.all_bridges {
                for (dx, dy) in crate::util::direction::DIRECTION_DELTAS {
                    terrain
                        .cell_mut((5 + dx) as u16, (5 + dy) as u16)
                        .unwrap()
                        .bridge_facts
                        .raw_flags = 0x100;
                }
            }
            let mut raw = crate::sim::occupancy::RawCellOccupationGrid::new();
            if let Some(case) = fixture.entry_case.as_ref() {
                // Translate the native witness from (10,10) to this damage
                // receiver fixture's (5,5); source heading and neighbours agree.
                let xy = |v: &serde_json::Value| {
                    (
                        (v[0].as_u64().unwrap() - 5) as u16,
                        (v[1].as_u64().unwrap() - 5) as u16,
                    )
                };
                for cell in case["cells"].as_array().into_iter().flatten() {
                    let (x, y) = xy(cell);
                    let out = terrain.cell_mut(x, y).unwrap();
                    out.level = cell[2].as_i64().unwrap() as u8;
                    out.bridge_facts.raw_flags = cell[3].as_u64().unwrap() as u32;
                }
                for cell in case["slopes"].as_array().into_iter().flatten() {
                    let (x, y) = xy(cell);
                    terrain.cell_mut(x, y).unwrap().slope_type = cell[2].as_u64().unwrap() as u8;
                }
                for cell in case["blocked_terrain"].as_array().into_iter().flatten() {
                    let (x, y) = xy(cell);
                    terrain.cell_mut(x, y).unwrap().speed_costs.foot = Some(0);
                }
                for cell in case["raw"].as_array().into_iter().flatten() {
                    let (x, y) = xy(cell);
                    raw.mark_ground(x, y, cell[2].as_u64().unwrap() as u8);
                    raw.mark_deck(x, y, cell[3].as_u64().unwrap() as u8);
                }
            }
            let event = EntityDamageEvent::area(
                2,
                10,
                0,
                if attacker_present {
                    1
                } else {
                    crate::sim::combat::RAD_NO_ATTACKER
                },
                attacker_present.then_some(attacker_house),
                warhead_ref,
            );
            let mut world = crate::sim::world::Simulation::new();
            world.substrate.entities = entities;
            world.substrate.occupancy = occupancy;
            world.substrate.raw_cell_occupation = raw;
            world.interner = interner;
            world.resolved_terrain = Some(terrain);
            world.playfield_bounds = Some(
                crate::sim::cell_rect::PlayfieldBounds::from_normalized_local_size(
                    16, -16, -16, 64, 64,
                ),
            );
            if fixture.outside {
                world.playfield_bounds = Some(
                    crate::sim::cell_rect::PlayfieldBounds::from_normalized_local_size(
                        16, 0, 0, 1, 1,
                    ),
                );
            }
            world.main_rng = SimRng::new(7);
            world.scenario_rng = SimRng::new(1);
            // Some(false) models campaign PlayerControl without IsHuman.
            world.session.game_mode_nonzero = false;
            let mut house =
                HouseState::new(victim_house, 0, None, human_control == Some(true), 0, 10);
            house.player_control = human_control.is_some();
            world.houses.insert(victim_house, house);
            if team {
                let script = world.interner.intern("ScatterFixture");
                world
                    .team_script_vm
                    .create_team(victim_house, script, vec![2], None, 0);
            }
            let before_rng = world.scenario_rng.state();
            let mut receiver = crate::sim::combat::world_receiver::ReceiverRun::default();
            crate::sim::combat::world_receiver::commit_entities(
                &mut world,
                &mut receiver,
                std::slice::from_ref(&event),
                None,
                &rules,
                None,
            );
            let victim = world
                .substrate
                .entities
                .get(2)
                .expect("victim remains represented");
            Outcome {
                health: victim.health.current,
                fear: victim
                    .infantry
                    .as_ref()
                    .expect("Infantry runtime")
                    .fear_level,
                destination: victim
                    .movement_target
                    .as_ref()
                    .map(|movement| *movement.path.last().expect("direct move has destination")),
                queued_mission: victim.mission.queued(),
                rng_changed: world.scenario_rng.state() != before_rng,
                rng_indices: {
                    let state = world.scenario_rng.logical_state();
                    serde_json::json!([state.index_a, state.index_b])
                },
            }
        }

        let run_scene = |fixture| {
            run_fixture(
                MissionType::Guard,
                true,
                125,
                true,
                None,
                false,
                false,
                fixture,
            )
        };
        let fixture = || ScatterFixture {
            doing: -1,
            ..Default::default()
        };
        // These witnesses execute original Scatter51D0D0 and Infantry51BF90
        // together. Exercise the production damage receiver, including HP,
        // queue/destination writes and fear, with the same entry prestates.
        let corpus: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tools/spatial_oracle/infantry_scatter_entry.json"
        ))
        .unwrap();
        assert_eq!(corpus.as_array().unwrap().len(), 14);
        for row in corpus.as_array().unwrap() {
            let output = run_scene(ScatterFixture {
                entry_case: Some(row["input"].clone()),
                ..fixture()
            });
            let expected = row["destination"].as_array().map(|xy| {
                (
                    (xy[0].as_u64().unwrap() - 5) as u16,
                    (xy[1].as_u64().unwrap() - 5) as u16,
                )
            });
            assert_eq!(output.destination, expected, "{}", row["input"]);
            assert_eq!(
                output.rng_indices, row["random_indices"],
                "{}",
                row["input"]
            );
            assert_eq!(output.health, 115);
            assert_eq!(output.fear, 300);
            assert_eq!(
                output.queued_mission,
                expected.map_or(MissionId::NONE, |_| {
                    MissionId::from_known(MissionType::Move)
                })
            );
        }
        let displayed_death = run_scene(ScatterFixture {
            animation: Some(crate::sim::animation::SequenceKind::Die1),
            ..fixture()
        });
        assert_eq!(
            displayed_death.destination,
            Some((6, 4)),
            "display does not own Doing"
        );
        let retained_death = run_scene(ScatterFixture {
            doing: 7,
            animation: Some(crate::sim::animation::SequenceKind::Stand),
            ..fixture()
        });
        assert_eq!(retained_death.destination, None);
        assert!(!retained_death.rng_changed);
        let later_preferred = run_scene(ScatterFixture {
            first_bridge: true,
            ..fixture()
        });
        assert_eq!(later_preferred.destination, Some((6, 5)));
        let fallback = run_scene(ScatterFixture {
            all_bridges: true,
            ..fixture()
        });
        assert_eq!(fallback.destination, Some((6, 4)));
        let from_head = run_scene(ScatterFixture {
            head: Some(crate::sim::components::DriveCoord {
                x: 7 * 256 + 128,
                y: 7 * 256 + 128,
                z: 0,
            }),
            ..fixture()
        });
        assert_eq!(
            from_head.destination,
            Some((8, 6)),
            "scan follows Foot head; heading follows physical source"
        );
        let outside = run_scene(ScatterFixture {
            outside: true,
            ..fixture()
        });
        assert_eq!(outside.destination, None);
        assert!(
            outside.rng_changed,
            "selection failure keeps the preceding native draw"
        );

        // Seed 1 yields RandomRanged(0,4)==1. With the attacker due west,
        // native base direction is E (2), so start=NE (1) and the first open
        // cell is (6,4). The Move/NavCom write is already visible when fear is
        // subsequently latched to 300 for Fraidycat (native518C7B).
        let guard = run(MissionType::Guard, true, 125, true, None, false, false);
        assert_eq!(guard.health, 115);
        assert_eq!(guard.destination, Some((6, 4)));
        assert_eq!(
            guard.queued_mission,
            MissionId::from_known(MissionType::Move)
        );
        assert_eq!(guard.fear, 300);
        assert!(guard.rng_changed);

        let null_attacker = run(MissionType::Guard, false, 125, true, None, false, false);
        assert_eq!(null_attacker.destination, None);
        assert!(!null_attacker.rng_changed);

        let fatal = run(MissionType::Guard, true, 10, true, None, false, false);
        assert_eq!(fatal.health, 0);
        assert_eq!(fatal.destination, None);

        let attack_mission = run(MissionType::Attack, true, 125, true, None, false, false);
        assert_eq!(attack_mission.destination, None);
        assert!(!attack_mission.rng_changed);

        // Unforced damage must not make an ordinary combat infantryman flee,
        // even if an AI owns it or its human owner has a Team and NavCom.
        for control in [None, Some(true), Some(false)] {
            let soldier = run(MissionType::Guard, true, 125, false, control, true, true);
            assert_eq!(soldier.health, 115);
            assert_eq!(soldier.destination, None);
            assert!(!soldier.rng_changed, "{soldier:?}");
        }
        for control in [Some(true), Some(false)] {
            let moving_civilian = run(MissionType::Guard, true, 125, true, control, false, true);
            assert_eq!(moving_civilian.destination, None);
            assert!(
                !moving_civilian.rng_changed,
                "NavCom cannot substitute for Team"
            );
            let team_civilian = run(MissionType::Guard, true, 125, true, control, true, false);
            assert_eq!(team_civilian.destination, Some((6, 4)));
            assert!(team_civilian.rng_changed);
        }
    }

    fn bridge_layer_test_fixture() -> (
        EntityStore,
        OccupancyGrid,
        ResolvedTerrainGrid,
        RuleSet,
        WarheadType,
        StringInterner,
    ) {
        let ini = IniFile::from_str(
            "\
[VehicleTypes]\n0=MTNK\n\n\
[InfantryTypes]\n\n\
[AircraftTypes]\n\n\
[BuildingTypes]\n\n\
[Warheads]\n0=BridgeSplash\n\n\
[MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\n\n\
[BridgeSplash]\nCellSpread=1\nPercentAtMax=1\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("rules parse");
        let warhead =
            WarheadType::from_ini_section("BridgeSplash", ini.section("BridgeSplash").unwrap());

        let ground = GameEntity::test_default(1, "MTNK", "Soviet", 5, 5);
        let mut bridge = GameEntity::test_default(2, "MTNK", "Soviet", 5, 5);
        bridge.on_bridge = true;
        bridge.position.z = 4;

        let mut entities = EntityStore::new();
        entities.insert(ground);
        entities.insert(bridge);

        let mut occupancy = OccupancyGrid::new();
        occupancy.add(
            5,
            5,
            1,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        occupancy.add(
            5,
            5,
            2,
            MovementLayer::Bridge,
            None,
            CellListInsertion::PrependNonBuilding,
        );

        let mut cells = Vec::new();
        for ry in 0..10 {
            for rx in 0..10 {
                cells.push(test_terrain_cell(rx, ry));
            }
        }
        let idx = 5 * 10 + 5;
        cells[idx].bridge_facts = BridgeCellFacts {
            raw_flags: BRIDGE_FLAG_STRUCTURAL,
            ..BridgeCellFacts::default()
        };
        cells[idx].has_bridge_deck = true;
        cells[idx].bridge_walkable = true;
        cells[idx].bridge_deck_level = 4;
        let terrain = ResolvedTerrainGrid::from_cells(10, 10, cells);

        test_intern("Americans");
        let interner = test_interner();

        (entities, occupancy, terrain, rules, warhead, interner)
    }

    fn test_terrain_cell(rx: u16, ry: u16) -> ResolvedTerrainCell {
        ResolvedTerrainCell {
            rx,
            ry,
            source_tile_index: 0,
            source_sub_tile: 0,
            final_tile_index: 0,
            final_sub_tile: 0,
            is_wood_bridge_repair_tile: false,
            level: 0,
            filled_clear: false,
            tileset_index: Some(0),
            land_type: 0,
            yr_cell_land_type: 0,
            slope_type: 0,
            template_height: 0,
            render_offset_x: 0,
            render_offset_y: 0,
            terrain_class: TerrainClass::Clear,
            speed_costs: SpeedCostProfile::default(),
            is_water: false,
            is_cliff_like: false,
            is_rough: false,
            is_road: false,
            accepts_smudge: false,
            allows_tiberium: false,
            height_in_pixels: 0,
            variant: 0,
            has_ramp: false,
            canonical_ramp: None,
            ground_walk_blocked: false,
            terrain_object_blocks: false,
            terrain_object_occupation: None,
            overlay_blocks: false,
            overlay_zone_type: None,
            outside_playfield: false,
            zone_type: 0,
            base_ground_walk_blocked: false,
            base_build_blocked: false,
            base_land_type: 0,
            base_yr_cell_land_type: 0,
            base_terrain_class: Default::default(),
            base_speed_costs: Default::default(),
            build_blocked: false,
            has_bridge_deck: false,
            bridge_walkable: false,
            bridge_transition: false,
            bridge_deck_level: 0,
            bridge_layer: None,
            bridge_facts: BridgeCellFacts::default(),
            tube_index: None,
            radar_left: [0, 0, 0],
            radar_right: [0, 0, 0],
            has_damaged_data: false,
            bridgehead_anchor_class_at_load: None,
        }
    }

    #[test]
    fn gsi_04_10_wood_splash_collects_terrain_in_native_receiver_order() {
        let ini = IniFile::from_str(
            "[General]\nTreeStrength=100\n\
             [InfantryTypes]\n\
             [VehicleTypes]\n0=UNIT\n\
             [AircraftTypes]\n0=AIR\n\
             [BuildingTypes]\n0=BLDG\n\
             [TerrainTypes]\n0=TREE01\n\
             [Warheads]\n0=WOODWH\n\
             [UNIT]\nStrength=100\nArmor=none\n\
             [AIR]\nStrength=100\nArmor=none\n\
             [BLDG]\nStrength=100\nArmor=wood\n\
             [TREE01]\nStrength=100\nArmor=wood\nImmune=no\n\
             [WOODWH]\nWood=yes\nCellSpread=2\nPercentAtMax=.5\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("Terrain receiver-order rules");
        let warhead = rules.warhead("WOODWH").expect("wood warhead").clone();
        let mut interner = StringInterner::new();
        let warhead_ref = interner.intern("WOODWH");
        let tree_ref = interner.intern("TREE01");

        let mut entities = EntityStore::new();
        let mut unit = GameEntity::test_default(20, "UNIT", "Americans", 5, 5);
        unit.position.sub_x = CELL_CENTER_LEPTON;
        unit.position.sub_y = CELL_CENTER_LEPTON;
        entities.insert(unit);

        let mut building = GameEntity::test_default(30, "BLDG", "Americans", 5, 5);
        building.category = EntityCategory::Structure;
        entities.insert(building);

        let mut air = GameEntity::test_default(10, "AIR", "Americans", 5, 5);
        air.category = EntityCategory::Aircraft;
        air.position.sub_x = CELL_CENTER_LEPTON;
        air.position.sub_y = CELL_CENTER_LEPTON;
        let mut air_locomotor = crate::sim::movement::locomotor::LocomotorState::from_object_type(
            rules.object("AIR").expect("air type"),
            0,
        );
        air_locomotor.layer = MovementLayer::Air;
        air_locomotor.altitude = SimFixed::from_num(1);
        air.locomotor = Some(air_locomotor);
        air.air_spatial_bucket = Some(5 + 5 * 20);
        air.air_spatial_enter_order = 1;
        entities.insert(air);

        let mut occupancy = OccupancyGrid::new();
        occupancy.add(
            5,
            5,
            20,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        occupancy.add(
            5,
            5,
            30,
            MovementLayer::Ground,
            None,
            CellListInsertion::AppendBuilding,
        );

        let mut terrain_objects = BTreeMap::new();
        let mut terrain_cells = BTreeMap::new();
        for (stable_id, rx) in [(100, 5), (101, 6), (102, 8)] {
            terrain_objects.insert(
                stable_id,
                TerrainObjectState {
                    stable_id,
                    native_unique_id: None,
                    in_logic_vector: false,
                    type_ref: tree_ref,
                    rx,
                    ry: 5,
                    health: 100,
                    max_health: 100,
                    occupation_bits: 4,
                    lifecycle: TerrainObjectLifecycle::Live,
                },
            );
            terrain_cells.insert((rx, 5), stable_id);
        }

        let cells = (0..12)
            .flat_map(|ry| (0..12).map(move |rx| test_terrain_cell(rx, ry)))
            .collect();
        let mut terrain = ResolvedTerrainGrid::from_cells(12, 12, cells);
        let result = apply_aoe_damage_with_terrain(
            &mut entities,
            5,
            5,
            100,
            &warhead,
            &rules,
            &mut interner,
            (crate::sim::combat::RAD_NO_ATTACKER, None, warhead_ref),
            AoELayerContext {
                occupancy: Some(&occupancy),
                terrain: Some(&mut terrain),
                air_impact: Some(AoEAirImpact {
                    sub_x: CELL_CENTER_LEPTON,
                    sub_y: CELL_CENTER_LEPTON,
                    z_leptons: 1,
                }),
                impact_z: 0,
                ..AoELayerContext::default()
            },
            Some(TerrainCollectionView {
                objects: &terrain_objects,
                cells: &terrain_cells,
            }),
        );

        let receiver_ids = result
            .receivers
            .iter()
            .map(|receiver| match receiver {
                AreaDamageReceiver::Entity(event) => ("entity", event.target_id),
                AreaDamageReceiver::Terrain(event) => ("terrain", event.stable_id),
            })
            .collect::<Vec<_>>();
        assert_eq!(
            &receiver_ids[..4],
            &[
                ("entity", 10),
                ("entity", 20),
                ("terrain", 100),
                ("entity", 30),
            ],
            "air precedes the center list; Terrain stays after later nonbuildings and before Buildings"
        );
        let adjacent = result
            .receivers
            .iter()
            .find_map(|receiver| match receiver {
                AreaDamageReceiver::Terrain(event) if event.stable_id == 101 => Some(*event),
                _ => None,
            })
            .expect("an adjacent tree-only cell is collected without a Techno occupancy bucket");
        assert_eq!(adjacent.distance_leptons, 256);
        assert_eq!(
            adjacent.damage, 100,
            "the receiver record retains raw damage until ordered commit"
        );
        assert_eq!(
            super::super::damage::kernel::apply_warhead_damage(
                adjacent.damage,
                warhead.cell_spread_f64,
                warhead.percent_at_max_f64,
                &warhead.verses_f64,
                super::super::damage::ArmorClass(super::super::armor_index("wood") as u8),
                adjacent.distance_leptons,
                false,
                rules.combat_damage.max_damage,
            ),
            75,
            "the captured per-object distance produces the exact shared-kernel falloff at commit"
        );
        assert_eq!(
            result
                .receivers
                .iter()
                .filter(|receiver| {
                    matches!(
                        receiver,
                        AreaDamageReceiver::Terrain(event) if event.stable_id == 100
                    )
                })
                .count(),
            1,
            "the former exact-center Terrain sidecar must not duplicate the unified receiver"
        );
        assert!(
            !result.receivers.iter().any(|receiver| {
                matches!(
                    receiver,
                    AreaDamageReceiver::Terrain(event) if event.stable_id == 102
                )
            }),
            "Terrain outside the signed CellSpread radius is not captured"
        );
    }

    #[test]
    fn gsi_04_10_spawning_terrain_c4_precedes_uninit_and_guards_reentry() {
        let ini = IniFile::from_str(
            "[General]\nTreeStrength=10\n\
             [CombatDamage]\nC4Warhead=C4WH\n\
             [InfantryTypes]\n\
             [VehicleTypes]\n0=VICTIM\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [TerrainTypes]\n0=TIBTREE\n\
             [Warheads]\n0=OUTERWH\n1=C4WH\n\
             [VICTIM]\nStrength=1000\nArmor=wood\n\
             [TIBTREE]\nStrength=10\nArmor=wood\nImmune=no\nSpawnsTiberium=yes\n\
             [OUTERWH]\nWood=yes\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n\
             [C4WH]\nWood=yes\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n",
        );
        let mut rules = RuleSet::from_ini(&ini).expect("spawning Terrain rules");
        let mut sim = crate::sim::world::Simulation::new();
        sim.resolve_type_handles(&rules);
        let victim_id = sim
            .spawn_object("VICTIM", "VictimHouse", 5, 5, 0, &rules, &BTreeMap::new())
            .expect("nested C4 victim spawns");

        let cells = (0..10)
            .flat_map(|ry| (0..10).map(move |rx| test_terrain_cell(rx, ry)))
            .collect();
        sim.resolved_terrain = Some(ResolvedTerrainGrid::from_cells(10, 10, cells));

        let terrain_id = 900;
        let terrain_ref = sim.interner.intern("TIBTREE");
        let terrain_state = TerrainObjectState {
            stable_id: terrain_id,
            native_unique_id: None,
            in_logic_vector: false,
            type_ref: terrain_ref,
            rx: 5,
            ry: 5,
            health: 10,
            max_health: 10,
            occupation_bits: 4,
            lifecycle: TerrainObjectLifecycle::Live,
        };
        sim.production
            .terrain_objects
            .insert(terrain_id, terrain_state.clone());
        sim.production
            .terrain_object_cells
            .insert((5, 5), terrain_id);
        sim.production
            .tiberium_spawning_terrain_cells
            .insert((5, 5));
        crate::sim::terrain_object::mark_terrain_raw_occupation(
            &mut sim.substrate.raw_cell_occupation,
            (5, 5),
            terrain_state.occupation_bits,
        );
        sim.substrate.raw_cell_occupation.mark_ground(5, 5, 0x80);
        sim.substrate.raw_cell_occupation.mark_deck(5, 5, 0xA5);
        crate::sim::terrain_object::mark_terrain_occupation(
            &mut sim.production,
            &terrain_state,
            sim.resolved_terrain.as_mut(),
        );
        sim.terrain_costs =
            crate::sim::pathfinding::terrain_cost::build_canonical_terrain_cost_grids(
                sim.resolved_terrain
                    .as_ref()
                    .expect("pre-damage resolved terrain"),
            );
        let initial_path = crate::sim::pathfinding::PathGrid::from_resolved_terrain(
            sim.resolved_terrain
                .as_ref()
                .expect("pre-damage resolved terrain"),
        );
        sim.rebuild_zone_grid(&initial_path);

        let outer_ref = sim.interner.intern("OUTERWH");
        let later_parent_receiver = EntityDamageEvent::area(
            victim_id,
            10,
            0,
            crate::sim::combat::RAD_NO_ATTACKER,
            None,
            outer_ref,
        );
        sim.commit_noncombat_aoe_receivers(
            &rules,
            None,
            &[
                AreaDamageReceiver::Terrain(TerrainDamageEvent {
                    stable_id: terrain_id,
                    rx: 5,
                    ry: 5,
                    damage: 10,
                    distance_leptons: 0,
                    warhead_ref: outer_ref,
                    near_center_ic_isolation_eligible: true,
                }),
                AreaDamageReceiver::Entity(later_parent_receiver),
            ],
        );

        assert_eq!(
            sim.substrate
                .entities
                .get(victim_id)
                .unwrap()
                .health
                .current,
            890,
            "nested raw-100 null-source C4 completes before the later parent receiver"
        );
        assert_eq!(
            sim.substrate
                .entities
                .get(victim_id)
                .unwrap()
                .last_attacker_id,
            None,
            "nested and parent null-source events do not fabricate an attacker"
        );
        let destroyed = &sim.production.terrain_objects[&terrain_id];
        assert_eq!(destroyed.health, 0);
        assert_eq!(destroyed.lifecycle, TerrainObjectLifecycle::Destroyed);
        assert!(!sim.production.terrain_object_cells.contains_key(&(5, 5)));
        assert!(
            !sim.production
                .tiberium_spawning_terrain_cells
                .contains(&(5, 5)),
            "the outer Terrain finalizes once after the Wood-enabled C4 re-entry is guarded"
        );
        assert_eq!(
            sim.substrate.raw_cell_occupation.ground_bits(5, 5)
                & crate::sim::terrain_object::terrain_raw_occupation_mask(
                    terrain_state.occupation_bits
                ),
            0,
            "Terrain-specific raw occupation clears even though the live Techno remains"
        );
        assert_eq!(
            sim.substrate.raw_cell_occupation.ground_bits(5, 5) & 0x80,
            0x80
        );
        assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(5, 5), 0xA5);
        let resolved = sim
            .resolved_terrain
            .as_ref()
            .expect("same-frame terrain refresh");
        let cell = resolved.cell(5, 5).expect("Terrain source cell");
        assert_eq!(cell.terrain_object_occupation, None);
        assert!(!cell.terrain_object_blocks);
        assert!(
            crate::sim::pathfinding::PathGrid::from_resolved_terrain(resolved).is_walkable(5, 5)
        );
        assert!(
            sim.zone_grid.is_some(),
            "same-frame Terrain finalization rebuilds zones"
        );
    }

    #[test]
    fn gsi_04_10_scenario_no_damage_blocks_entity_terrain_and_nested_c4() {
        let ini = IniFile::from_str(
            "[General]\nTreeStrength=10\n\
             [CombatDamage]\nC4Warhead=C4WH\n\
             [InfantryTypes]\n\
             [VehicleTypes]\n0=VICTIM\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [TerrainTypes]\n0=TIBTREE\n\
             [Warheads]\n0=OUTERWH\n1=C4WH\n\
             [VICTIM]\nStrength=1000\nArmor=wood\n\
             [TIBTREE]\nStrength=10\nArmor=wood\nImmune=no\nSpawnsTiberium=yes\n\
             [OUTERWH]\nWood=yes\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n\
             [C4WH]\nWood=yes\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n",
        );
        let mut rules = RuleSet::from_ini(&ini).expect("inert Terrain rules");
        let mut sim = crate::sim::world::Simulation::new();
        sim.resolve_type_handles(&rules);
        let victim_id = sim
            .spawn_object("VICTIM", "VictimHouse", 5, 5, 0, &rules, &BTreeMap::new())
            .expect("inert receiver victim spawns");
        let cells = (0..10)
            .flat_map(|ry| (0..10).map(move |rx| test_terrain_cell(rx, ry)))
            .collect();
        sim.resolved_terrain = Some(ResolvedTerrainGrid::from_cells(10, 10, cells));

        let terrain_id = 901;
        let terrain_ref = sim.interner.intern("TIBTREE");
        sim.production.terrain_objects.insert(
            terrain_id,
            TerrainObjectState {
                stable_id: terrain_id,
                native_unique_id: None,
                in_logic_vector: false,
                type_ref: terrain_ref,
                rx: 5,
                ry: 5,
                health: 10,
                max_health: 10,
                occupation_bits: 4,
                lifecycle: TerrainObjectLifecycle::Live,
            },
        );
        sim.production
            .terrain_object_cells
            .insert((5, 5), terrain_id);
        sim.production
            .tiberium_spawning_terrain_cells
            .insert((5, 5));
        sim.session.no_damage = true;
        let outer_ref = sim.interner.intern("OUTERWH");
        let before_hash = sim.state_hash();
        let before_rng = sim.scenario_rng.state();

        sim.commit_noncombat_aoe_receivers(
            &rules,
            None,
            &[
                AreaDamageReceiver::Entity(EntityDamageEvent::direct_receiver(
                    victim_id,
                    100,
                    0,
                    crate::sim::combat::RAD_NO_ATTACKER,
                    None,
                    outer_ref,
                    crate::sim::combat::ReceiverCallFlags {
                        ignore_defenses: false,
                        arg6: true,
                    },
                )),
                AreaDamageReceiver::Terrain(TerrainDamageEvent {
                    stable_id: terrain_id,
                    rx: 5,
                    ry: 5,
                    damage: 10,
                    distance_leptons: 0,
                    warhead_ref: outer_ref,
                    near_center_ic_isolation_eligible: true,
                }),
            ],
        );

        assert_eq!(
            sim.substrate
                .entities
                .get(victim_id)
                .unwrap()
                .health
                .current,
            1000
        );
        let terrain = &sim.production.terrain_objects[&terrain_id];
        assert_eq!(terrain.health, 10);
        assert_eq!(terrain.lifecycle, TerrainObjectLifecycle::Live);
        assert_eq!(sim.production.terrain_object_cells[&(5, 5)], terrain_id);
        assert_eq!(sim.scenario_rng.state(), before_rng);
        assert_eq!(sim.state_hash(), before_hash);
    }
}
