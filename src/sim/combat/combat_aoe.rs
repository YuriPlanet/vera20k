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
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

use super::{EntityDamageEvent, TerrainDamageEvent, cell_spread};
use crate::map::cell_index::NativeCellIdentity;
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::{NativeCellQuery, ResolvedTerrainGrid};
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::rules::warhead_type::WarheadType;
use crate::sim::entity_store::EntityStore;
use crate::sim::intern::StringInterner;
use crate::sim::map::bridge_topology::{CellBridgeView, ListLayer};
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::OccupancyGrid;
#[cfg(any(test, feature = "test-support"))]
use crate::sim::overlay_grid::WallMutation;
use crate::sim::overlay_grid::{
    OverlayGrid, WallDamageTransactionHost, WallDirtyStep, WallPointerTarget,
    damage_wall_overlay_with_runtime_host,
};
use crate::sim::pathfinding::zone_incremental::ZoneRepairKind;
use crate::sim::rng::SimRng;
use crate::util::fixed_math::SimFixed;
use crate::util::lepton::{CELL_CENTER_LEPTON, LEPTONS_PER_LEVEL, ground_height_leptons};
use crate::util::native_x87::distance_3d_leptons;

const BUILDING_CENTER_HEIGHT_ALLOWANCE_LEPTONS: i32 = 2 * LEPTONS_PER_LEVEL as i32;

// AreaDamage48955E..48958D freezes its ground/deck list before any
// per-cell callback. Bridge topology owns the strict exact-lepton selector;
// Map578080's existing ground query owns floor sampling and Dummy identity.

/// Optional map context for gamemd bridge object-list selection.
#[derive(Default)]
pub(crate) struct AoELayerContext<'a> {
    pub occupancy: Option<&'a OccupancyGrid>,
    pub terrain: Option<&'a mut ResolvedTerrainGrid>,
    pub overlay_grid: Option<&'a mut OverlayGrid>,
    pub overlay_registry: Option<&'a OverlayTypeRegistry>,
    pub scenario_rng: Option<&'a mut SimRng>,
    /// Exact detonation coordinate for receiver distance and object-list
    /// selection. Only callers without this coordinate use the whole-Level
    /// `impact_z` adapter below.
    pub air_impact: Option<AoEAirImpact>,
    pub impact_z: i32,
}

/// Native per-spread-cell side effects that run before wall routing and before
/// the cell's object-list records are captured. The callback receives short
/// reborrows of the same map/RNG authorities used by the AoE transaction, so a
/// producer can preserve exact inter-cell RNG order without moving those
/// authorities into combat_aoe.
pub(crate) trait AoECellPrelude {
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
        _overlays: &OverlayGrid,
        _cell: (u16, u16),
        _navigation_changed: bool,
        _repair: ZoneRepairKind,
    ) {
    }

    fn wall_dirty_step(&mut self, _step: WallDirtyStep, _packed_coord: (u16, u16)) {}
}

struct AoEWallDamageHost<'borrow, 'prelude> {
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
        overlays: &OverlayGrid,
        cell: (u16, u16),
        navigation_changed: bool,
        repair: ZoneRepairKind,
    ) {
        if let Some(prelude) = self.prelude.as_deref_mut() {
            prelude.wall_navigation_step(terrain, overlays, cell, navigation_changed, repair);
        }
    }

    // Cell ctor47BBF0 leaves Abstract+14 bit1 clear; 7258D0(Cell) does not
    // broadcast to Techno listeners. AreaDamage's explicit70D4A0 is separate.
    fn pointer_expired(&mut self, _target: WallPointerTarget) {}
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

/// `TechnoClass::TriggersCellInset @ 0x006F7660`, retained as a dormant
/// production query until its native caller is established. It asks the
/// shared deploy-weapon owner for the selected WeaponType, applies native
/// `IsAreaFire @ 0x0070DD50`, resolves that weapon's Warhead, then compares
/// `ftol(Sqrt_Approx(dx² + dy²))` in signed-cell coordinates against
/// `CellSpread - CellInset`. Native `TriggersCellInset @ 0x006F7660` calls
/// `Sqrt_Approx @ 0x004CAC40` then `Math__ftol @ 0x007C5F00`. The coordinate
/// owner dispatches `GetCoords` at vtable +0x48 (Object `0x005F65A0`, Building
/// `0x00447AC0`, Cell `0x00486840`). Native's `IsAreaFire` and subsequent
/// weapon read both use `GetDeployWeapon @ 0x0070E120`; this pure selection is
/// resolved once here and consumes no RNG or mutable state.
pub fn techno_triggers_cell_inset(
    world: &crate::sim::world::Simulation,
    techno_id: u64,
    target: super::TargetKind,
    rules: &RuleSet,
) -> bool {
    let Some(techno) = world.substrate.entities.get(techno_id) else {
        return false;
    };
    let Some(object_type) = world.interner.object_type(techno.type_ref(), rules) else {
        return false;
    };
    let (_, deployed) = super::combat_weapon::deploy_weapon(object_type, techno.veterancy.rank());
    let Some((weapon_id, _)) = deployed else {
        return false;
    };
    let Some(weapon) = rules.weapon(weapon_id) else {
        return false;
    };
    if !weapon.area_fire {
        return false;
    }
    let Some(warhead) = rules.weapon_warhead(weapon) else {
        return false;
    };

    // Keep the native GetCoords (+0x48) dispatch in ground_pose/target_get_coords
    // so Buildings, Cells and Terrain targets do not fall back to raw Location
    // fields or a fabricated point.
    let cells = world
        .resolved_terrain
        .as_ref()
        .map(NativeCellQuery::canonical);
    let Some(techno_coords) = crate::sim::movement::ground_pose::target_get_coords(
        super::TargetKind::Entity(techno_id),
        world,
        cells.as_ref(),
    ) else {
        return false;
    };
    let Some(target_coords) =
        crate::sim::movement::ground_pose::target_get_coords(target, world, cells.as_ref())
    else {
        return false;
    };
    let techno_x = crate::util::lepton::lepton_to_cell_packed(techno_coords.x);
    let techno_y = crate::util::lepton::lepton_to_cell_packed(techno_coords.y);
    let target_x = crate::util::lepton::lepton_to_cell_packed(target_coords.x);
    let target_y = crate::util::lepton::lepton_to_cell_packed(target_coords.y);
    let dx = target_x.wrapping_sub(techno_x);
    let dy = target_y.wrapping_sub(techno_y);
    let distance =
        crate::util::native_x87::sqrt_approx_length([i32::from(dx), i32::from(dy)]) as i16;

    // Native FLD CellSpread / FSUB CellInset / FSTP float produces one
    // binary32 threshold. The distance is then narrowed to signed i16, so
    // comparing that exact integer to a downward-projected I16F16 threshold
    // preserves the inclusive integer boundary while keeping sim math fixed.
    let threshold = warhead.cell_spread_f64 as f32 - warhead.cell_inset_f32;
    cell_inset_distance_within_threshold(distance, threshold)
}

fn cell_inset_distance_within_threshold(distance: i16, threshold: f32) -> bool {
    if threshold.is_nan() {
        // Native FCOMP sets the unordered C0/C3 flags; TEST AH, 0x41 is then
        // nonzero, so the following JZ (false path) is not taken.
        return true;
    }
    if threshold >= 32767.0 {
        return true;
    }
    if threshold < -32768.0 {
        return false;
    }

    let nearest = SimFixed::from_num(threshold);
    let threshold_fixed = if nearest.to_num::<f32>() > threshold {
        SimFixed::from_bits(nearest.to_bits() - 1)
    } else {
        nearest
    };
    SimFixed::from_num(i32::from(distance)) <= threshold_fixed
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AoEAirImpact {
    pub sub_x: SimFixed,
    pub sub_y: SimFixed,
    pub z_leptons: i32,
}

impl AoEAirImpact {
    /// Reconstruct the exact absolute world point from its packed cell and
    /// subcell fields. Packed negative cell coordinates remain signed.
    fn world_point(self, impact_rx: u16, impact_ry: u16) -> crate::sim::components::DriveCoord {
        crate::sim::components::DriveCoord {
            x: crate::util::lepton::absolute_lepton_axis(impact_rx, self.sub_x),
            y: crate::util::lepton::absolute_lepton_axis(impact_ry, self.sub_y),
            z: self.z_leptons,
        }
    }
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

#[cfg(any(test, feature = "test-support"))]
impl From<&str> for AoEDamageOrigin {
    fn from(_legacy_owner_fixture: &str) -> Self {
        Self {
            source_id: super::RAD_NO_ATTACKER,
            source_house: None,
            warhead_ref: crate::sim::intern::InternedId::default(),
        }
    }
}

/// Apply_area_damage's entry (`0x004892BE`): an inert scenario or a zero
/// damage returns at once. Ares DamageArea_NullDamage (10053940) lets a
/// warhead with `AllowZeroDamage=yes` take a zero damage through the area.
pub(crate) fn area_damage_enters(
    damage: i32,
    warhead: &WarheadType,
    scenario_no_damage: bool,
    ares: bool,
) -> bool {
    let zero_admitted = ares
        && warhead
            .ares_combat()
            .is_some_and(|combat| combat.allow_zero_damage());
    !scenario_no_damage && (damage != 0 || zero_admitted)
}

/// Lift the established whole-Level impact coordinate into native absolute
/// lepton Z while preserving the exact XY subcell sample on slopes.
fn air_impact_from_layer_z(
    terrain: Option<&ResolvedTerrainGrid>,
    impact_rx: u16,
    impact_ry: u16,
    sub_x: SimFixed,
    sub_y: SimFixed,
    impact_z: i32,
) -> Option<AoEAirImpact> {
    let cells = NativeCellQuery::canonical(terrain?);
    let cell = cells.lookup((impact_rx as i16, impact_ry as i16));
    let level = cells.ground_fields(cell).0;
    let mut impact = AoEAirImpact {
        sub_x,
        sub_y,
        z_leptons: 0,
    };
    let ground_z = crate::sim::movement::ground_pose::query_ground_height(
        &cells,
        impact.world_point(impact_rx, impact_ry),
    )
    .ok()?;
    let level_delta = impact_z.wrapping_sub(i32::from(level as i8));
    impact.z_leptons = ground_z.wrapping_add(level_delta.wrapping_mul(LEPTONS_PER_LEVEL as i32));
    Some(impact)
}

#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CellTargetDetach {
    pub listener_id: u64,
    pub restored: bool,
    pub cleared: bool,
}

#[derive(Debug, Default)]
pub(crate) struct AoEDamageResult {
    /// Original489357/48955E..48958D local, frozen before cell callbacks and
    /// reused after receiver dispatch by the area-rocker continuation.
    pub object_list_layer: ListLayer,
    pub receivers: Vec<AreaDamageReceiver>,
    /// Compatibility-only projection for existing unit tests. Production
    /// callers consume `receivers`, which is the sole ordered authority.
    #[cfg(any(test, feature = "test-support"))]
    pub hits: Vec<EntityDamageEvent>,
    #[cfg(any(test, feature = "test-support"))]
    pub wall_mutations: Vec<WallMutation>,
    #[cfg(any(test, feature = "test-support"))]
    pub cell_target_detaches: Vec<CellTargetDetach>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AreaDamageReceiver {
    Entity(EntityDamageEvent),
    Terrain(TerrainDamageEvent),
}

#[derive(Clone, Copy)]
pub(crate) struct TerrainCollectionView<'a> {
    pub store: &'a crate::sim::terrain_object::TerrainObjectStore,
}

impl AoEDamageResult {
    fn push_entity(&mut self, event: EntityDamageEvent) {
        #[cfg(any(test, feature = "test-support"))]
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
        #[cfg(any(test, feature = "test-support"))]
        for event in &mut result.hits {
            event.near_center_ic_isolation_eligible = true;
        }
    }
    result
}

/// Apply area-of-effect damage from a warhead detonation at a specific cell.
///
/// Returns a list of (stable_id, damage) pairs for all entities within the blast
/// radius. Friendly fire IS applied — CellSpread does not discriminate by owner,
/// matching RA2 behavior (e.g., V3 rockets can damage your own units).
///
/// `base_damage` is the weapon's raw damage value (before Verses scaling).
#[cfg(any(test, feature = "test-support"))]
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
#[cfg(any(test, feature = "test-support"))]
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

/// Call-local AreaDamage collection cursor. World authorities stay installed
/// between cells, so synchronous Restore/setter calls can run before capture.
pub(crate) struct AoECollector {
    impact_rx: u16,
    impact_ry: u16,
    base_damage: i32,
    origin: AoEDamageOrigin,
    ground_source_admitted: bool,
    spread_leptons: i64,
    exact_impact: AoEAirImpact,
    native_occupancy: bool,
    /// Whether Ares plays in this game, read once at the entry: the cells
    /// walked and the record limit must not change under the collector.
    ares: bool,
    /// [`crate::sim::scenario_session::ScenarioSession::truce_harvester_types`]:
    /// the `HarvesterUnit=` types while ScenarioFlags bit 0x800 is set.
    truce_harvesters: Vec<crate::sim::intern::InternedId>,
    pub result: AoEDamageResult,
}

pub(crate) struct AoESpreadCell {
    identity: Option<NativeCellIdentity>,
    real_coord: Option<(u16, u16)>,
    requested: (u16, u16),
    target: WallPointerTarget,
    initial_overlay_present: bool,
    center: bool,
}

impl AoESpreadCell {
    /// 4896B2..4896BA tests the LIVE overlay on the retained allocation after
    /// all recursive wall callbacks, but only when initially nonempty.
    pub fn detached_cell(&self, context: &AoELayerContext<'_>) -> Option<(u16, u16)> {
        (self.initial_overlay_present && retained_overlay(self.target, context).is_none())
            // A frozen request identifies the shared Dummy without reading or
            // restamping its mutable coordinate. The sweep compares allocation
            // identities; a real alias uses the retained canonical coordinate.
            .then_some(self.real_coord.unwrap_or(self.requested))
    }
}

fn retained_overlay(target: WallPointerTarget, context: &AoELayerContext<'_>) -> Option<u8> {
    match target {
        WallPointerTarget::Real(rx, ry) => context.overlay_grid.as_deref()?.cell(rx, ry).overlay_id,
        WallPointerTarget::SharedDummy => u8::try_from(
            context
                .terrain
                .as_deref()?
                .shared_cell_dummy()
                .overlay_identity_state()
                .0,
        )
        .ok(),
    }
}

impl AoECollector {
    pub fn begin(
        entities: &EntityStore,
        impact_rx: u16,
        impact_ry: u16,
        base_damage: i32,
        warhead: &WarheadType,
        rules: &RuleSet,
        interner: &StringInterner,
        handles: Option<crate::sim::type_handle_table::ResolvedRuleHandles>,
        origin: AoEDamageOrigin,
        layer_context: &AoELayerContext<'_>,
        scenario_no_damage: bool,
        truce_harvesters: Vec<crate::sim::intern::InternedId>,
    ) -> Option<Self> {
        // Apply_area_damage returns at entry for signed zero. Nothing downstream
        // observes CellSpread, wall routing, object lists, target detach, or RNG.
        // Native ScenarioFlags bit 0x20 shares that transaction-wide early-out.
        if !area_damage_enters(base_damage, warhead, scenario_no_damage, rules.ares()) {
            return None;
        }
        // DamageArea forwards its caller's SourceHouse unchanged to every
        // receiver (`0x004899DE` loads it, `0x00489AA7` pushes it): no fill.
        // `MapClass::DamageArea @ 0x00489280`'s ground/deck list excludes the
        // source object unless its type opts into `DamageSelf`, the warhead is
        // Rules.CrushWarhead, or Phobos `AllowDamageOnSelf` admits the firer
        // (`Explosion_Damage_DamageSelf @ 0x004896EC`).
        let source_damage_self = entities
            .get(origin.source_id)
            .and_then(|source| interner.object_type(source.type_ref(), rules))
            .is_some_and(|source_type| source_type.damage_self);
        let ground_source_admitted = handles
            .is_some_and(|handles| handles.is_crush(origin.warhead_ref))
            || source_damage_self
            || rules.allow_damage_on_self_for(warhead);
        // Retail's airborne phase admits the source unconditionally. Phobos
        // `MapClass_DamageArea_AirDamageSelfFix @ 0x00489416` applies the same
        // DamageSelf/AllowDamageOnSelf rule there (CrushWarhead does not).
        #[cfg(feature = "phobos")]
        let air_source_admitted = source_damage_self || rules.allow_damage_on_self_for(warhead);
        #[cfg(not(feature = "phobos"))]
        let air_source_admitted = true;

        let cell_spread = warhead.cell_spread_f64;

        // Each candidate's native Sqrt_Approx/ftol distance is compared to this
        // signed threshold; there is no exact-squared prefilter.
        let spread_leptons = cell_spread::splash_threshold_leptons(cell_spread);
        // Each detonation's receivers are its own list: the commit walks it
        // while a receiver's death detonates again and collects another, and
        // the bullet path hands it on to its damage events. A run lasts one
        // fire visit, so a buffer kept there would serve only those nested
        // detonations.
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

        result.object_list_layer = layer_context
            .terrain
            .as_deref()
            .map_or(ListLayer::Ground, |terrain| {
                select_object_damage_layer(terrain, impact_rx, impact_ry, exact_impact)
            });

        let native_occupancy = layer_context
            .occupancy
            .filter(|_| layer_context.terrain.is_some());
        if native_occupancy.is_some() {
            // Native enters the entire airborne query only when the exact impact
            // coordinate is strictly above GetGroundHeight at that same XY. Keep
            // this gate outside bucket materialization: equality observes no air
            // vector at all.
            if let Some(air_impact) = layer_context.air_impact
                && layer_context.terrain.as_deref().is_some_and(|terrain| {
                    impact_is_above_ground(
                        terrain, impact_rx, impact_ry, air_impact, warhead, rules,
                    )
                })
            {
                // Native appends airborne receiver records before entering the
                // spread cell loop. Damage dispatch happens only after collection,
                // so walls still mutate during the later cell walk before any
                // receiver runs.
                let center = crate::sim::cell_rect::get_cellclass_fallback(
                    layer_context.terrain.as_deref(),
                    i32::from(impact_rx as i16),
                    i32::from(impact_ry as i16),
                );
                for entity_id in entities.airborne_in_query_order(
                    &center,
                    cell_spread::whole_cells(cell_spread),
                    layer_context
                        .terrain
                        .as_deref()
                        .map(|terrain| (terrain.width(), terrain.height()))
                        .expect("checked terrain Apply_area_damage context"),
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
                        air_source_admitted,
                        rules,
                        interner,
                    );
                }
            }
        }

        Some(Self {
            impact_rx,
            impact_ry,
            base_damage,
            origin,
            ground_source_admitted,
            spread_leptons,
            exact_impact,
            native_occupancy: native_occupancy.is_some(),
            ares: rules.ares(),
            truce_harvesters,
            result,
        })
    }

    pub fn prepare_cell(
        &mut self,
        dx: i16,
        dy: i16,
        warhead: &WarheadType,
        layer_context: &mut AoELayerContext<'_>,
        cell_prelude: &mut Option<&mut dyn AoECellPrelude>,
    ) -> AoESpreadCell {
        let requested = (
            (self.impact_rx as i16).wrapping_add(dx),
            (self.impact_ry as i16).wrapping_add(dy),
        );
        let cell = layer_context
            .terrain
            .as_deref()
            .map(|terrain| NativeCellQuery::canonical(terrain).lookup(requested));
        let real_coord = match cell {
            Some(real @ NativeCellIdentity::Real(_)) => {
                let cells = NativeCellQuery::canonical(layer_context.terrain.as_deref().unwrap());
                let coord = cells.coord(real);
                Some((coord.0 as u16, coord.1 as u16))
            }
            Some(NativeCellIdentity::Dummy) => None,
            // No CellClass authority exists in legacy headless fixtures.
            None => Some((requested.0 as u16, requested.1 as u16)),
        };
        let target = real_coord.map_or(WallPointerTarget::SharedDummy, |(rx, ry)| {
            WallPointerTarget::Real(rx, ry)
        });
        // AreaDamage48960E..489623 retains initial admission and type BEFORE
        // ReduceTiberium. The final empty-cell sweep is independent of Wall.
        let initial_overlay = retained_overlay(target, layer_context);
        let initial_wall = initial_overlay.and_then(|id| {
            layer_context
                .overlay_registry?
                .flags(id)
                .map(|flags| (flags.wall, flags.armor_is_wood))
        });
        if let Some((rx, ry)) = real_coord
            && let Some(prelude) = cell_prelude.as_deref_mut()
        {
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
            target,
            initial_wall,
            self.base_damage,
            warhead,
            layer_context,
            cell_prelude,
            &mut self.result,
        );
        AoESpreadCell {
            identity: cell,
            real_coord,
            requested: (requested.0 as u16, requested.1 as u16),
            target,
            initial_overlay_present: initial_overlay.is_some(),
            center: dx == 0 && dy == 0,
        }
    }

    fn selected_layer(&self) -> MovementLayer {
        match self.result.object_list_layer {
            ListLayer::Ground => MovementLayer::Ground,
            ListLayer::Bridge => MovementLayer::Bridge,
        }
    }

    pub fn capture_cell(
        &mut self,
        cell: &AoESpreadCell,
        entities: &EntityStore,
        context: &AoELayerContext<'_>,
        terrain_objects: Option<TerrainCollectionView<'_>>,
    ) {
        let Some(occupancy) = context.occupancy.filter(|_| self.native_occupancy) else {
            return;
        };
        // Dummy E4/E8 heads are null (47BC82/88). Its mutable coordinates must
        // never turn unrelated rectangular-grid entries into native residents.
        if cell.identity == Some(NativeCellIdentity::Dummy) {
            return;
        }
        let (rx, ry) = cell
            .real_coord
            .expect("native real cell has its canonical coordinate");
        let mut terrain_inserted = self.selected_layer() != MovementLayer::Ground;
        if let Some(cell_occ) = occupancy.get(rx, ry) {
            for occupant in cell_occ.iter_layer(self.selected_layer()) {
                // TerrainClass was revealed before map technos. Native
                // AddContent prepends later nonbuildings and appends
                // Buildings, so the captured Terrain record sits at this
                // exact represented boundary.
                if !terrain_inserted && occupant.is_building {
                    push_terrain_aoe_damage(
                        &mut self.result,
                        terrain_objects,
                        rx,
                        ry,
                        self.impact_rx,
                        self.impact_ry,
                        self.exact_impact,
                        self.spread_leptons,
                        self.base_damage,
                        self.origin.warhead_ref,
                    );
                    terrain_inserted = true;
                }
                push_entity_aoe_damage(
                    &mut self.result,
                    entities,
                    occupant.entity_id,
                    self.impact_rx,
                    self.impact_ry,
                    rx,
                    ry,
                    cell.center,
                    self.exact_impact,
                    context.terrain.as_deref(),
                    self.spread_leptons,
                    self.base_damage,
                    self.origin.source_id,
                    self.origin.source_house,
                    self.ground_source_admitted,
                    self.origin.warhead_ref,
                    &self.truce_harvesters,
                );
            }
        }
        if !terrain_inserted {
            // A tree-only cell has no OccupancyGrid entry. Terrain lookup
            // must remain outside that optional Techno cache.
            push_terrain_aoe_damage(
                &mut self.result,
                terrain_objects,
                rx,
                ry,
                self.impact_rx,
                self.impact_ry,
                self.exact_impact,
                self.spread_leptons,
                self.base_damage,
                self.origin.warhead_ref,
            );
        }
    }

    pub fn finish(
        mut self,
        entities: &EntityStore,
        context: &AoELayerContext<'_>,
        warhead: &WarheadType,
    ) -> AoEDamageResult {
        if self.native_occupancy {
            limit_records_per_object(&mut self.result, warhead, self.ares);
            return finalize_receiver_isolation_metadata(self.result, warhead);
        }

        for entity in entities.values() {
            if entity.lifecycle.in_limbo()
                || crate::sim::occupancy::cell_list_layer_for_entity(
                    entity,
                    context.terrain.as_deref(),
                )
                .is_none()
            {
                continue;
            }
            // Dying corpses (uninit'd this tick, awaiting the end-of-tick drain)
            // are off the live object list — exclude them. A sold/captured corpse
            // keeps health>0, so gate on `dying`, not just health.
            if entity.health.current() == 0 || entity.death.is_dying() {
                continue;
            }

            push_entity_aoe_damage(
                &mut self.result,
                entities,
                entity.stable_id(),
                self.impact_rx,
                self.impact_ry,
                entity.position.rx(),
                entity.position.ry(),
                entity.position.rx() == self.impact_rx && entity.position.ry() == self.impact_ry,
                self.exact_impact,
                context.terrain.as_deref(),
                self.spread_leptons,
                self.base_damage,
                self.origin.source_id,
                self.origin.source_house,
                self.ground_source_admitted,
                self.origin.warhead_ref,
                &self.truce_harvesters,
            );
        }

        limit_records_per_object(&mut self.result, warhead, self.ares);
        finalize_receiver_isolation_metadata(self.result, warhead)
    }
}

/// Ares DamageArea_Damage_MaxAffect (10053400, hook 0x4899DA), after the
/// spread loop: with `CellSpread.MaxAffect >= 0`, each object keeps only
/// that many of its records (a building has one per foundation cell). In
/// record order, the first unhandled record's object collects its records
/// from there on; beyond the limit, a partial selection sort
/// (`std::min_element`, first minimum) keeps the nearest and the rest are
/// dropped, the survivors keeping their order.
///
/// RESIDUAL: VERA's near-center Iron Curtain pre-scan runs over the kept
/// records, while native arms it during collection. Only a dropped record
/// nearer than 85 leptons that arms while every kept one cannot differs,
/// which a building's equal-distance records never produce.
pub(super) fn limit_records_per_object(
    result: &mut AoEDamageResult,
    warhead: &WarheadType,
    ares: bool,
) {
    if ares {
        let Some(max_affect) = warhead
            .ares_combat()
            .map(|combat| combat.cell_spread_max_affect())
            .filter(|&max_affect| max_affect >= 0)
        else {
            return;
        };
        let max_affect = max_affect as usize;
        let key = |receiver: &AreaDamageReceiver| match receiver {
            AreaDamageReceiver::Entity(event) => (false, event.target_id),
            AreaDamageReceiver::Terrain(event) => (true, event.stable_id),
        };
        let distance = |receiver: &AreaDamageReceiver| match receiver {
            AreaDamageReceiver::Entity(event) => event.distance_leptons,
            AreaDamageReceiver::Terrain(event) => event.distance_leptons,
        };
        let records = &result.receivers;
        let mut keep = vec![true; records.len()];
        let mut handled = Vec::new();
        for first in 0..records.len() {
            let object = key(&records[first]);
            if !keep[first] || handled.contains(&object) {
                continue;
            }
            handled.push(object);
            let mut members: Vec<usize> = (first..records.len())
                .filter(|&index| key(&records[index]) == object)
                .collect();
            if members.len() <= max_affect {
                continue;
            }
            for slot in 0..max_affect {
                let nearest = (slot..members.len())
                    .min_by_key(|&at| (distance(&records[members[at]]), at))
                    .expect("a nonempty tail");
                members.swap(slot, nearest);
            }
            for &dropped in &members[max_affect..] {
                keep[dropped] = false;
            }
        }
        let mut kept = keep.into_iter();
        result
            .receivers
            .retain(|_| kept.next().expect("one flag per record"));
        #[cfg(any(test, feature = "test-support"))]
        {
            result.hits = result
                .receivers
                .iter()
                .filter_map(|receiver| match receiver {
                    AreaDamageReceiver::Entity(event) => Some(*event),
                    AreaDamageReceiver::Terrain(_) => None,
                })
                .collect();
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
/// Geometry-only fixture driver sharing the production collection cursor.
/// Full-world Restore callbacks are owned by world_receiver::collect_area.
/// Without occupancy, unlimboed living objects stand in for the cell lists.
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
    let Some(mut collector) = AoECollector::begin(
        entities,
        impact_rx,
        impact_ry,
        base_damage,
        warhead,
        rules,
        interner,
        handles,
        origin.into(),
        &layer_context,
        scenario_no_damage,
        Vec::new(),
    ) else {
        return AoEDamageResult::default();
    };
    for &(dx, dy) in cell_spread::area_damage_cells(warhead.cell_spread_f64, rules.ares()).iter() {
        let cell = collector.prepare_cell(dx, dy, warhead, &mut layer_context, &mut cell_prelude);
        collector.capture_cell(&cell, entities, &layer_context, terrain_objects);
    }
    collector.finish(entities, &layer_context, warhead)
}

fn route_wall_before_cell_objects(
    target: WallPointerTarget,
    initial_wall: Option<(bool, bool)>,
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
    let Some((true, armor_is_wood)) = initial_wall else {
        return;
    };

    // Active-YR routing precedence: WAD forces literal -1; ordinary Wall and
    // Wood against Armor=wood pass the unchanged signed weapon damage.
    let routed_damage = if warhead.wall_absolute_destroyer {
        -1
    } else if warhead.wall || (warhead.wood && armor_is_wood) {
        raw_damage
    } else {
        return;
    };

    let _wall_result = {
        let mut host = AoEWallDamageHost {
            prelude: cell_prelude,
        };
        damage_wall_overlay_with_runtime_host(
            grid,
            registry,
            context.terrain.as_deref_mut(),
            target,
            routed_damage,
            rng,
            Some(&mut host),
        )
    };
    #[cfg(any(test, feature = "test-support"))]
    _result.wall_mutations.extend(_wall_result.mutations);
}

fn select_object_damage_layer(
    terrain: &ResolvedTerrainGrid,
    impact_rx: u16,
    impact_ry: u16,
    impact: AoEAirImpact,
) -> ListLayer {
    let cells = NativeCellQuery::canonical(terrain);
    let cell = cells.lookup((impact_rx as i16, impact_ry as i16));
    let view = CellBridgeView::from_query(&cells, cell);
    // Original48955E gates the Map578080 floor call on the retained cell's
    // structural flag. Keep its lookup/Dummy side effects conditional too.
    let floor = if view.is_bridge_cell() {
        crate::sim::movement::ground_pose::query_ground_height(
            &cells,
            impact.world_point(impact_rx, impact_ry),
        )
        .expect("AreaDamage floor requires a supported native Cell slope")
    } else {
        0 // Unread by the topology selector on the non-bridge arm.
    };
    if let Some(deck) = crate::sim::movement::ground_pose::corrected_deck_z(terrain, cell) {
        // Retain 0x0048955E's strict halfway split, using the actual clearance.
        let midpoint = floor + (deck - floor) / 2;
        return if impact.z_leptons > midpoint {
            ListLayer::Bridge
        } else {
            ListLayer::Ground
        };
    }
    view.aoe_object_layer(impact.z_leptons, floor)
}

fn impact_is_above_ground(
    terrain: &ResolvedTerrainGrid,
    impact_rx: u16,
    impact_ry: u16,
    impact: AoEAirImpact,
    warhead: &WarheadType,
    rules: &RuleSet,
) -> bool {
    let cells = NativeCellQuery::canonical(terrain);
    let Ok(floor) = crate::sim::movement::ground_pose::query_ground_height(
        &cells,
        impact.world_point(impact_rx, impact_ry),
    ) else {
        return false;
    };
    // Ares DamageArea_DamageAir (100533A0, hook 0x4893BA): the height above
    // the floor must exceed DamageAirThreshold (default 0, the native
    // `floor < Z`); with `[General] DamageAirConsiderBridges=yes` a bridge
    // cell (flag 0x100) measures from its live deck once reached.
    if rules.ares()
        && let Some(combat) = warhead.ares_combat()
    {
        use crate::map::cell_index::NativeCellIdentity;
        use crate::sim::movement::ground_pose::corrected_deck_z;
        use crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS;

        let mut height = impact.z_leptons.wrapping_sub(floor);
        if rules.ares_warhead_globals().damage_air_consider_bridges() {
            let coord = (impact_rx as i16, impact_ry as i16);
            // Below the native threshold only a fixed real deck can change
            // the result. Preserve the original conditional Dummy lookup.
            let deck = if height >= BRIDGE_DECK_HEIGHT_LEPTONS {
                let cell = cells.lookup(coord);
                (cells.flags(cell) & crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL != 0)
                    .then(|| crate::sim::movement::ground_pose::deck_surface_z(&cells, cell, floor))
            } else {
                terrain
                    .native_fixed_cell_index(coord.0, coord.1)
                    .and_then(|index| corrected_deck_z(terrain, NativeCellIdentity::Real(index)))
            };
            if let Some(deck) = deck {
                let above_deck = impact.z_leptons.wrapping_sub(deck);
                if above_deck >= 0 {
                    height = above_deck;
                }
            }
        }
        return height > combat.damage_air_threshold();
    }
    floor < impact.z_leptons
}

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
    source_self_admitted: bool,
    rules: &RuleSet,
    interner: &StringInterner,
) {
    // Phobos `MapClass_DamageArea_AirDamageSelfFix @ 0x00489416`: the airborne
    // phase skips its own source unless DamageSelf or AllowDamageOnSelf admits
    // it. A non-Phobos build keeps retail's unconditional admission.
    if entity_id == source_id && !source_self_admitted {
        return;
    }
    let Some(entity) = entities.get(entity_id) else {
        return;
    };
    // `0x004893F2..0x00489410`: the member must be alive (`+0x90`), marked
    // (`+0x74`) and have Health left; its layer and height are not read.
    // VERA-only: the dying mark stands in for the IsAlive that
    // `ObjectClass::UnInit` (`0x005F65F0`) clears at once in gamemd.
    if !entity.lifecycle.object_alive()
        || !entity.lifecycle.cell_marked()
        || entity.health.current() == 0
        || entity.death.is_dying()
    {
        return;
    }
    let target = crate::sim::movement::ground_pose::object_location(entity, Some(terrain));
    let impact_point = impact.world_point(impact_rx, impact_ry);
    let (impact_x, impact_y) = (impact_point.x, impact_point.y);
    let raw_distance_leptons = distance_3d_leptons(
        [impact_x, impact_y, impact.z_leptons],
        [target.x, target.y, target.z],
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
    let distance_leptons = if entity.category() == EntityCategory::Aircraft
        && crate::sim::movement::air_movement::is_high_flying(
            entity,
            Some(terrain),
            Some((rules, interner)),
        ) {
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

/// Collects one ground/deck-list receiver record for `Apply_area_damage`
/// (`0x00489280`).
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
    truce_harvesters: &[crate::sim::intern::InternedId],
) {
    // Apply_area_damage's selected ground/deck list excludes the source object
    // unless its type opts into DamageSelf, the warhead is Rules.CrushWarhead,
    // or Phobos AllowDamageOnSelf admits the firer. Airborne collection is a
    // separate native phase and deliberately does not use this admission gate.
    if entity_id == source_id && !source_self_admitted {
        return;
    }
    let Some(entity) = entities.get(entity_id) else {
        return;
    };
    if entity.health.current() == 0 || entity.death.is_dying() {
        return;
    }
    // `0x00489710..0x00489759`: while ScenarioFlags bit 0x800 (HarvesterImmune,
    // the Harvester Truce) is set, a Unit whose type is in Rules'
    // `HarvesterUnit=` list gets no record, so no weapon or splash damage. The
    // airborne phase has no such skip.
    if entity.category() == EntityCategory::Unit && truce_harvesters.contains(&entity.type_ref()) {
        return;
    }

    let impact_point = impact.world_point(impact_rx, impact_ry);
    let (impact_x, impact_y) = (impact_point.x, impact_point.y);

    // Building receiver records are keyed to the selected CellClass entry.
    // Both center and adjacent foundation cells use that cell's center
    // coordinate. The center record alone gets a two-Level vertical allowance
    // before storing the remaining exact 3D distance. Ordinary objects use
    // their virtual +A4 coordinate, including exact subcell XY and absolute Z.
    let target_xyz = if entity.category() == EntityCategory::Structure {
        let x = i32::from(scan_rx)
            .wrapping_mul(256)
            .wrapping_add(CELL_CENTER_LEPTON.to_num::<i32>());
        let y = i32::from(scan_ry)
            .wrapping_mul(256)
            .wrapping_add(CELL_CENTER_LEPTON.to_num::<i32>());
        let z = terrain
            .and_then(|terrain| terrain.cell(scan_rx, scan_ry))
            .and_then(|cell| ground_height_leptons(cell.level, cell.slope_type, x, y).ok())
            .unwrap_or_else(|| {
                i32::from(entity.position.z()).wrapping_mul(LEPTONS_PER_LEVEL as i32)
            });
        [x, y, z]
    } else {
        let location = crate::sim::movement::ground_pose::object_location(entity, terrain);
        [location.x, location.y, location.z]
    };
    let distance_leptons = if entity.category() == EntityCategory::Structure && center_cell {
        let height_above_cell = impact.z_leptons.wrapping_sub(target_xyz[2]);
        if height_above_cell <= BUILDING_CENTER_HEIGHT_ALLOWANCE_LEPTONS {
            0
        } else {
            distance_3d_leptons([impact_x, impact_y, impact.z_leptons], target_xyz)
                .wrapping_sub(BUILDING_CENTER_HEIGHT_ALLOWANCE_LEPTONS)
        }
    } else {
        distance_3d_leptons([impact_x, impact_y, impact.z_leptons], target_xyz)
    };
    if i64::from(distance_leptons) > spread_leptons {
        return;
    }

    let mut event = EntityDamageEvent::area(
        entity.stable_id(),
        base_damage,
        distance_leptons,
        source_id,
        source_house,
        warhead_ref,
    );
    // 489932 tests the spread-loop index, independently of the later
    // transaction-wide filter. The unique zero offset is its first cell.
    event.adjacent_ground_cell_record = !center_cell;
    result.push_entity(event);
}

fn push_terrain_aoe_damage(
    result: &mut AoEDamageResult,
    terrain_objects: Option<TerrainCollectionView<'_>>,
    scan_rx: u16,
    scan_ry: u16,
    impact_rx: u16,
    impact_ry: u16,
    impact: AoEAirImpact,
    spread_leptons: i64,
    base_damage: i32,
    warhead_ref: crate::sim::intern::InternedId,
) {
    let Some(view) = terrain_objects else {
        return;
    };
    // Apply_area_damage489280 walks every Cell member; GetTerrain's first
    // matching object is insufficient when generated constructors overlap.
    for stable_id in view.store.members_at((scan_rx, scan_ry)) {
        let object = &view.store.objects()[&stable_id];
        if !object.is_live() {
            continue;
        }
        let impact_point = impact.world_point(impact_rx, impact_ry);
        let target_coord = object.world_coord();
        let distance_leptons = distance_3d_leptons(
            [impact_point.x, impact_point.y, impact.z_leptons],
            [target_coord.x, target_coord.y, target_coord.z],
        );
        if i64::from(distance_leptons) > spread_leptons {
            continue;
        }
        result.push_terrain(TerrainDamageEvent {
            damage_writeback: None,
            stable_id,
            rx: scan_rx,
            ry: scan_ry,
            damage: base_damage,
            distance_leptons,
            warhead_ref,
            near_center_ic_isolation_eligible: false,
        });
    }
}

#[cfg(all(test, tests_sim_combat_a_c))]
mod tests;
