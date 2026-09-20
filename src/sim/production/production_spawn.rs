//! Spawn cell selection for newly produced units.
//!
//! Determines where to place a unit after production completes, based on
//! factory location, exit offsets, and walkability. Extracted from
//! production_tech.rs for file-size limits.

use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::locomotor_type::{MovementZone, SpeedType};
use crate::rules::object_type::ObjectCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::entity_store::EntityStore;
use crate::sim::world::Simulation;

#[cfg(test)]
use crate::sim::cell_rect::{
    CellRect, CellRectOccupancyContext, CellRectPassabilityContext, check_occupancy_rect,
    check_passability_rect,
};

use super::production_tech::{
    producer_candidates_for_owner_category, production_category_for_object,
};
use super::production_types::ProductionCategory;
use crate::sim::movement::bump_crush;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::OccupancyGrid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProductionSpawnSelection {
    pub producer_id: u64,
    pub cell: (u16, u16),
    pub(super) delivery: ProductionDeliveryKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ProductionDeliveryKind {
    Standard,
    /// `BuildingClass::ExitObject_Main @ 0x00443C60`'s produced-Unit
    /// `!Refinery && !Weeder && WeaponsFactory && Naval` branch.
    NavalUnit {
        producer_rally: Option<(u16, u16)>,
    },
}

pub fn find_spawn_cell_for_owner(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: &str,
    produced_category: ObjectCategory,
    path_grid: Option<&crate::sim::pathfinding::PathGrid>,
    require_water: bool,
) -> Option<(u16, u16)> {
    find_spawn_selection_for_owner(
        sim,
        rules,
        owner,
        produced_category,
        path_grid,
        require_water,
    )
    .map(|selection| selection.cell)
}

pub fn find_spawn_selection_for_owner(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: &str,
    produced_category: ObjectCategory,
    path_grid: Option<&crate::sim::pathfinding::PathGrid>,
    require_water: bool,
) -> Option<ProductionSpawnSelection> {
    find_spawn_selection_for_owner_with_type(
        sim,
        rules,
        owner,
        None,
        produced_category,
        path_grid,
        require_water,
    )
}

pub(super) fn find_spawn_selection_for_owner_with_type(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: &str,
    produced_type_id: Option<&str>,
    produced_category: ObjectCategory,
    path_grid: Option<&crate::sim::pathfinding::PathGrid>,
    require_water: bool,
) -> Option<ProductionSpawnSelection> {
    let Some(queue_category) = produced_type_id
        .and_then(|type_id| rules.object(type_id))
        .map(production_category_for_object)
        .or_else(|| producer_queue_category_for_object(produced_category, require_water))
    else {
        return None;
    };
    let preferred_factories = producer_candidates_for_owner_category(
        &sim.substrate.entities,
        rules,
        owner,
        queue_category,
        true,
        &sim.interner,
    );
    let fallback_structures = producer_candidates_for_owner_category(
        &sim.substrate.entities,
        rules,
        owner,
        queue_category,
        false,
        &sim.interner,
    );
    let mut ordered_bases = preferred_factories.clone();
    let owner_id = sim.interner.intern(owner);
    if let Some(active_sid) = sim
        .production
        .active_producer_by_owner
        .get(&owner_id)
        .and_then(|categories| categories.get(&queue_category))
        .copied()
    {
        if let Some(index) = ordered_bases
            .iter()
            .position(|candidate| candidate.0 == active_sid)
        {
            ordered_bases.rotate_left(index);
        }
    } else if let Some(first) = preferred_factories.first() {
        sim.production
            .active_producer_by_owner
            .entry(owner_id)
            .or_default()
            .insert(queue_category, first.0);
    }

    let bases: &[(u64, u16, u16, String)] = if !ordered_bases.is_empty() {
        &ordered_bases
    } else if queue_category == ProductionCategory::Ship {
        // HouseClass's Ship slot cannot borrow a land factory or arbitrary
        // structure when its selected producer is absent.
        return None;
    } else {
        &fallback_structures
    };
    let resolved_terrain = sim.resolved_terrain.as_ref();
    let overlay_grid = sim.overlay_grid.as_ref();
    let zone_grid = sim.zone_grid.as_ref();
    let map_size = sim
        .playfield_bounds
        .zip(sim.playfield_size_height)
        .map(|(bounds, height)| (bounds.base, height));
    let movement_profile =
        spawn_movement_profile(rules, produced_type_id, produced_category, require_water);

    if produced_category == ObjectCategory::Vehicle {
        // Native HouseClass::Place_Production chooses one producer, then calls
        // ExitObject once. A failed cell choice or Unlimbo does not retry the
        // next factory, so every Unit branch below is bound to `bases.first()`.
        let (producer_id, bx, by, structure_id) = bases.first()?;
        if exact_naval_vehicle_exit_factory(rules, structure_id) {
            let producer_rally = sim
                .substrate
                .entities
                .get(*producer_id)
                .and_then(|producer| producer.rally_target);
            let (cell, _) = find_naval_unit_delivery_cell(
                *producer_id,
                *bx,
                *by,
                structure_id,
                producer_rally,
                movement_profile,
                rules,
                path_grid,
                &sim.substrate.occupancy,
                &sim.substrate.entities,
                resolved_terrain,
                overlay_grid,
                zone_grid,
                sim.session.binary_frame,
                sim.playfield_bounds,
                map_size,
            );
            return Some(ProductionSpawnSelection {
                producer_id: *producer_id,
                cell,
                delivery: ProductionDeliveryKind::NavalUnit { producer_rally },
            });
        }
        if !require_water && exact_land_vehicle_exit_factory(rules, structure_id) {
            return find_exact_exitcoord_spawn_cell(
                *bx,
                *by,
                structure_id,
                produced_category,
                rules,
                path_grid,
                &sim.substrate.occupancy,
                resolved_terrain,
                require_water,
            )
            .map(|cell| ProductionSpawnSelection {
                producer_id: *producer_id,
                cell,
                delivery: ProductionDeliveryKind::Standard,
            });
        }

        // Unverified/modded produced-Unit branches retain the legacy adapter,
        // but still cannot fail over to a second producer after selection.
        return find_spawn_cell_near_structure(
            *bx,
            *by,
            structure_id,
            produced_category,
            movement_profile,
            rules,
            path_grid,
            &sim.substrate.occupancy,
            &sim.substrate.entities,
            resolved_terrain,
            overlay_grid,
            zone_grid,
            require_water,
            sim.session.binary_frame,
            sim.playfield_bounds,
            map_size,
        )
        .map(|cell| ProductionSpawnSelection {
            producer_id: *producer_id,
            cell,
            delivery: ProductionDeliveryKind::Standard,
        });
    }

    for (producer_id, bx, by, structure_id) in bases {
        let cell = match produced_category {
            ObjectCategory::Infantry => {
                find_infantry_spawn_cell_near_structure(rules, *bx, *by, structure_id)
            }
            _ => find_spawn_cell_near_structure(
                *bx,
                *by,
                structure_id,
                produced_category,
                movement_profile,
                rules,
                path_grid,
                &sim.substrate.occupancy,
                &sim.substrate.entities,
                resolved_terrain,
                overlay_grid,
                zone_grid,
                require_water,
                // Frame-counter input for the authoritative FNPC fallback. The
                // counter is committed late, so during this advance it holds
                // current frame N, which is the value the fallback must alias.
                sim.session.binary_frame,
                sim.playfield_bounds,
                map_size,
            ),
        };
        if let Some(cell) = cell {
            return Some(ProductionSpawnSelection {
                producer_id: *producer_id,
                cell,
                delivery: ProductionDeliveryKind::Standard,
            });
        }
    }
    None
}

/// Mark the produced unit as having the reciprocal RadioClass contact created
/// by successful stock land war-factory unlimbo.
///
/// The caller must invoke this immediately after `spawn_object` returns the
/// produced unit stable ID. `find_spawn_selection_for_owner` supplies the
/// `producer_id` without changing the older cell-only API.
pub fn mark_war_factory_spawn_contact(
    sim: &mut Simulation,
    rules: &RuleSet,
    producer_id: u64,
    produced_id: u64,
) -> bool {
    let Some((producer_type, produced_is_vehicle)) =
        sim.substrate.entities.get(producer_id).and_then(|p| {
            let producer_type = sim.interner.resolve(p.type_ref()).to_string();
            let produced = sim.substrate.entities.get(produced_id)?;
            Some((
                producer_type,
                produced.category == crate::map::entities::EntityCategory::Unit,
            ))
        })
    else {
        return false;
    };

    if !produced_is_vehicle || !exact_land_vehicle_exit_factory(rules, &producer_type) {
        return false;
    }

    let Some(produced) = sim.substrate.entities.get_mut(produced_id) else {
        return false;
    };
    produced.mark_live_contact_with(producer_id);
    // gamemd ExitObject_Main also sends 0x18 (sets +0x418) beside the HELLO contact;
    // the footprint-clear break (tick_war_factory_exit_contacts) gates on this flag.
    produced.dock_entered_with = Some(producer_id);
    true
}

pub(super) fn exact_land_vehicle_exit_factory(rules: &RuleSet, structure_id: &str) -> bool {
    rules.object(structure_id).is_some_and(|obj| {
        !obj.refinery
            && !obj.weeder
            && obj.weapons_factory
            && !obj.naval
            && obj.exit_coord.is_some()
    })
}

fn exact_naval_vehicle_exit_factory(rules: &RuleSet, structure_id: &str) -> bool {
    rules
        .object(structure_id)
        .is_some_and(|obj| !obj.refinery && !obj.weeder && obj.weapons_factory && obj.naval)
}

/// Active-retail produced-Unit naval delivery owner.
///
/// `BuildingClass::ExitObject_Main @ 0x00443C60` begins at the producer's
/// `GetCoords` foundation centre, optionally walks out of the producer toward
/// its own ArchiveTarget/rally cell, and otherwise invokes the one shared FNPC
/// call at `0x004443DC`. FNPC failure is the literal zero cell; the caller still
/// performs one normal Unlimbo attempt against that result.
#[allow(clippy::too_many_arguments)]
fn find_naval_unit_delivery_cell(
    producer_id: u64,
    base_rx: u16,
    base_ry: u16,
    structure_id: &str,
    producer_rally: Option<(u16, u16)>,
    movement_profile: SpawnMovementProfile,
    rules: &RuleSet,
    path_grid: Option<&crate::sim::pathfinding::PathGrid>,
    occupancy: &OccupancyGrid,
    entities: &EntityStore,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    overlay_grid: Option<&crate::sim::overlay_grid::OverlayGrid>,
    zone_grid: Option<&crate::sim::pathfinding::zone_map::ZoneGrid>,
    frame_counter: u32,
    playfield_bounds: Option<crate::sim::cell_rect::PlayfieldBounds>,
    map_size: Option<(i32, i32)>,
) -> ((u16, u16), bool) {
    let Some(origin) = building_get_coords_cell(rules, structure_id, base_rx, base_ry) else {
        return ((0, 0), false);
    };

    if let Some(rally) = producer_rally
        && let Some(candidate) = naval_rally_fast_path_cell(
            producer_id,
            (base_rx, base_ry),
            origin,
            rally,
            occupancy,
            entities,
            resolved_terrain,
            playfield_bounds,
        )
    {
        return (candidate, true);
    }

    let Some(grid) = path_grid else {
        return ((0, 0), false);
    };
    let Some(query) = nearby_query_for_naval_unit_delivery(
        movement_profile.speed_type,
        grid,
        resolved_terrain,
        overlay_grid,
        zone_grid,
        playfield_bounds,
        map_size,
    ) else {
        return ((0, 0), false);
    };
    (
        crate::sim::find_nearby_cell::find_nearby_passable_cell(
            (i32::from(origin.0), i32::from(origin.1)),
            &query,
            frame_counter,
        )
        .unwrap_or((0, 0)),
        false,
    )
}

/// BuildingClass::GetCoords @ 0x00447AC0 followed by
/// ObjectClass::Get_Cell_Packed @ 0x0041BEA0. Location starts at the NW cell
/// centre; `(foundation_dim - 1) * 128` is added before signed `/ 256`.
fn building_get_coords_cell(
    rules: &RuleSet,
    structure_id: &str,
    base_rx: u16,
    base_ry: u16,
) -> Option<(u16, u16)> {
    fn axis(base: u16, span: u16) -> Option<u16> {
        let location = i32::from(base) * crate::sim::cell_kernel::LEPTONS_PER_CELL
            + crate::sim::cell_kernel::CELL_CENTER_LEPTONS;
        let centre =
            location + (i32::from(span) - 1) * crate::sim::cell_kernel::CELL_CENTER_LEPTONS;
        u16::try_from(centre / crate::sim::cell_kernel::LEPTONS_PER_CELL).ok()
    }

    let object = rules.object(structure_id)?;
    let (width, height) = super::production_tech::foundation_dimensions(&object.foundation);
    axis(base_rx, width).zip(axis(base_ry, height))
}

#[allow(clippy::too_many_arguments)]
fn naval_rally_fast_path_cell(
    producer_id: u64,
    producer_nw: (u16, u16),
    foundation_center: (u16, u16),
    rally: (u16, u16),
    occupancy: &OccupancyGrid,
    entities: &EntityStore,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    playfield_bounds: Option<crate::sim::cell_rect::PlayfieldBounds>,
) -> Option<(u16, u16)> {
    let candidate = naval_rally_walk_candidate(
        producer_id,
        producer_nw,
        foundation_center,
        rally,
        occupancy,
    );

    let terrain = resolved_terrain?;
    if terrain.cell(candidate.0, candidate.1)?.yr_cell_land_type
        != crate::rules::terrain_rules::LandType::Water.as_index()
    {
        return None;
    }
    let has_eligible_active_object = occupancy
        .get(candidate.0, candidate.1)
        .into_iter()
        .flat_map(|cell| cell.iter_layer(MovementLayer::Ground))
        .any(|occupant| {
            entities
                .get(occupant.entity_id)
                .is_some_and(|entity| entity.is_active())
        });
    if has_eligible_active_object {
        return None;
    }
    crate::sim::cell_rect::cell_is_in_playfield_height_aware(
        (i32::from(candidate.0), i32::from(candidate.1)),
        playfield_bounds,
        Some(terrain),
    )
    .then_some(candidate)
}

fn naval_rally_walk_candidate(
    producer_id: u64,
    producer_nw: (u16, u16),
    foundation_center: (u16, u16),
    rally: (u16, u16),
    occupancy: &OccupancyGrid,
) -> (u16, u16) {
    let facing = crate::util::fixed_math::facing_from_delta_int(
        i32::from(rally.0) - i32::from(producer_nw.0),
        i32::from(rally.1) - i32::from(producer_nw.1),
    );
    let (step_x, step_y) = crate::util::fixed_math::dir_to_cell_delta(facing);
    let mut candidate = foundation_center;

    // Look_up_building_in_cell @ 0x0047C520 returns the first Building in
    // literal +0xE4 list order. A different first Building stops the walk even
    // if this producer occurs later in the same cell's list.
    while occupancy.first_building_on_layer(candidate.0, candidate.1, MovementLayer::Ground)
        == Some(producer_id)
    {
        candidate = (
            candidate.0.wrapping_add_signed(step_x as i16),
            candidate.1.wrapping_add_signed(step_y as i16),
        );
    }
    candidate
}

#[allow(clippy::too_many_arguments)]
fn nearby_query_for_naval_unit_delivery<'a>(
    speed_type: SpeedType,
    grid: &'a crate::sim::pathfinding::PathGrid,
    resolved_terrain: Option<&'a ResolvedTerrainGrid>,
    overlay_grid: Option<&'a crate::sim::overlay_grid::OverlayGrid>,
    zone_grid: Option<&'a crate::sim::pathfinding::zone_map::ZoneGrid>,
    playfield_bounds: Option<crate::sim::cell_rect::PlayfieldBounds>,
    map_size: Option<(i32, i32)>,
) -> Option<crate::sim::find_nearby_cell::NearbyQuery<'a>> {
    use crate::sim::find_nearby_cell::{
        NearbyAnchorGate, NearbyFootprint, NearbyQuery, PassabilityArgs, map_owned_radius_cap,
    };
    let (size_width, size_height) = map_size?;
    Some(NearbyQuery {
        native_cells: None,
        raw_occupation: None,
        passability: PassabilityArgs {
            // Unit vtable +0x84 -> +0x88 -> UnitType+0x67C.
            speed_type,
            required_zone_id: None,
            // Literal caller argument 5 at 0x0044434B..0x004443DC.
            movement_zone: MovementZone::Normal,
            bridge_aware_zone: false,
        },
        footprint: NearbyFootprint::SINGLE,
        anchor_gate: NearbyAnchorGate::NativeHeightAware,
        allow_bridge_cells: true,
        check_height: false,
        // Arguments 11 and 15 are both false. Immediate UnitClass::Unlimbo,
        // not FNPC, owns the one occupancy/CanEnter/Mark decision.
        check_occupancy: false,
        radius_cap: map_owned_radius_cap(size_width, size_height),
        // The caller passes a pointer to CellStruct(0,0); FNPC maps that value
        // to live-frame modulo selection with no RNG draw.
        target_cell: None,
        path_grid: Some(grid),
        resolved_terrain,
        overlay_grid,
        // Both caller occupancy flags are false, so this query must not let
        // CellRect's compatibility object-list projection recreate one.
        occupancy: None,
        entities: None,
        zone_grid,
        playfield_bounds,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::sim) enum ProductionUnitAdmission {
    /// Exact UnitClass::Can_Enter_Cell result zero. Crush victims are retained
    /// only as evidence for the selected occupation-plane tail; production
    /// Unlimbo does not execute the later movement-time crush here.
    ExactZero {
        layer: MovementLayer,
        crush_victims: Vec<u64>,
    },
    NonZero {
        code: u8,
        layer: MovementLayer,
    },
}

impl ProductionUnitAdmission {
    #[cfg(test)]
    fn exact_zero(&self) -> bool {
        matches!(self, Self::ExactZero { .. })
    }

    pub(in crate::sim) fn exact_zero_layer(&self) -> Option<MovementLayer> {
        match self {
            Self::ExactZero { layer, .. } => Some(*layer),
            Self::NonZero { .. } => None,
        }
    }

    fn nonzero(code: u8, layer: MovementLayer) -> Self {
        Self::NonZero { code, layer }
    }
}

/// Production specialization of `UnitClass::Can_Enter_Cell @ 0x0073F0A0` for
/// the exact native tuple `(cell,-1,-1,0,0)`. This result deliberately does not
/// consume PathGrid or the general movement classifier: live CellClass terrain,
/// overlay, selected object list, and selected raw occupation plane own it.
fn produced_unit_unlimbo_entry(
    sim: &Simulation,
    rules: &RuleSet,
    owner: &str,
    produced_type_id: &str,
    produced_id: u64,
    producer_id: u64,
    cell: (u16, u16),
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
) -> Option<(ProductionUnitAdmission, (u16, u16))> {
    let resolved_cell = resolve_produced_unit_cell_coords(sim, cell)?;
    Some((
        produced_unit_unlimbo_entry_at_resolved_cell(
            sim,
            rules,
            owner,
            produced_type_id,
            produced_id,
            producer_id,
            resolved_cell,
            overlay_registry,
        ),
        resolved_cell,
    ))
}

/// Resolve the caller's `CellStruct` through the never-null MapClass lookup and
/// `CellClass::GetCoords @ 0x00486840` before ObjectClass evaluates its normal
/// zero-cell sentinel. A miss therefore updates and observes the one retained
/// shared-dummy identity; it is not collapsed to a Rust-side `None`.
fn resolve_produced_unit_cell_coords(
    sim: &Simulation,
    requested: (u16, u16),
) -> Option<(u16, u16)> {
    let selected = crate::sim::cell_rect::get_cellclass_fallback(
        sim.resolved_terrain.as_ref(),
        i32::from(requested.0),
        i32::from(requested.1),
    );
    let (coord, level, slope) = match selected {
        crate::sim::cell_rect::CellRef::Real(cell) => (
            (i32::from(cell.rx), i32::from(cell.ry)),
            cell.level,
            cell.slope_type,
        ),
        crate::sim::cell_rect::CellRef::Dummy { cell } => {
            let snapshot = cell.snapshot();
            (snapshot.coord, snapshot.level as u8, snapshot.slope_type)
        }
    };
    let center_x = coord
        .0
        .wrapping_mul(crate::sim::cell_kernel::LEPTONS_PER_CELL)
        .wrapping_add(crate::sim::cell_kernel::CELL_CENTER_LEPTONS);
    let center_y = coord
        .1
        .wrapping_mul(crate::sim::cell_kernel::LEPTONS_PER_CELL)
        .wrapping_add(crate::sim::cell_kernel::CELL_CENTER_LEPTONS);
    let ground_z =
        crate::util::lepton::ground_height_leptons(level, slope, center_x, center_y).ok()?;
    let coords = crate::sim::cell_kernel::cell_center(
        crate::sim::cell_kernel::CellCoordinate {
            x: coord.0,
            y: coord.1,
        },
        ground_z,
    );
    Some((
        crate::sim::cell_kernel::world_to_cell_trunc(coords.x) as i16 as u16,
        crate::sim::cell_kernel::world_to_cell_trunc(coords.y) as i16 as u16,
    ))
}

pub(in crate::sim) fn produced_unit_unlimbo_entry_at_resolved_cell(
    sim: &Simulation,
    rules: &RuleSet,
    owner: &str,
    produced_type_id: &str,
    produced_id: u64,
    producer_id: u64,
    cell: (u16, u16),
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
) -> ProductionUnitAdmission {
    use crate::map::entities::EntityCategory;
    use crate::rules::terrain_rules::LandType;
    use crate::sim::pathfinding::cell_entry::{
        BuildingOccupantEntryDecision, LiveVehicleBuildingEntry, VehicleBuildingEntryBranch,
        decide_live_vehicle_building_entry,
    };

    let Some(object) = rules.object(produced_type_id) else {
        return ProductionUnitAdmission::nonzero(7, MovementLayer::Ground);
    };
    let Some(resolved_terrain) = sim.resolved_terrain.as_ref() else {
        return ProductionUnitAdmission::nonzero(7, MovementLayer::Ground);
    };

    // The FNPC zero sentinel is a CellStruct, not a CoordStruct. It has already
    // resolved through CellClass/GetCoords to (128,128,z), so ordinary Unit
    // admission reaches this mandatory mode-one playfield predicate.
    if !crate::sim::cell_rect::cell_is_in_playfield_height_aware(
        (i32::from(cell.0), i32::from(cell.1)),
        sim.playfield_bounds,
        Some(resolved_terrain),
    ) {
        return ProductionUnitAdmission::nonzero(7, MovementLayer::Ground);
    }

    let Some(terrain_cell) = resolved_terrain.cell(cell.0, cell.1) else {
        return ProductionUnitAdmission::nonzero(7, MovementLayer::Ground);
    };
    let structural_bridge = terrain_cell.bridge_facts.has_structural_bridge();
    let layer = if structural_bridge {
        MovementLayer::Bridge
    } else {
        MovementLayer::Ground
    };
    let overlay_cell = sim
        .overlay_grid
        .as_ref()
        .map(|grid| grid.cell(cell.0, cell.1));
    let overlay_id = overlay_cell
        .and_then(|overlay| overlay.overlay_id)
        .or(terrain_cell.bridge_facts.overlay_id);

    // UnitType+0xDFC. Tunnel is allowed to reach its later sub-tile/land-speed
    // result; for stock HYD/SQD that terminal Float row is exactly zero. The
    // 0xED/0xEE exception samples the caller's initial height (-1) before bridge
    // traversal seeds Level+4, so only the literal signed-level comparison is
    // needed for the production zero predicate.
    let land_type = LandType::from_index(terrain_cell.yr_cell_land_type);
    if let Some(required) = object.movement_restricted_to
        && land_type != Some(required)
    {
        let tunnel_continues = land_type == Some(LandType::Tunnel);
        let low_bridge_height_exception =
            matches!(overlay_id, Some(0xED | 0xEE)) && -1i8 != terrain_cell.level as i8;
        if !tunnel_continues && !low_bridge_height_exception {
            return ProductionUnitAdmission::nonzero(7, layer);
        }
    }

    let produced_veterancy = sim
        .substrate
        .entities
        .get(produced_id)
        .map_or(0, |entity| entity.veterancy);
    let regular_crusher = object.crusher
        || (produced_veterancy >= 100 && object.veteran_crusher)
        || (produced_veterancy >= 200 && object.elite_crusher);
    let crush_capability = bump_crush::CrushCapability::new(regular_crusher, object.omni_crusher);

    // Wall=yes is an ordered overlay branch. Enemy/unowned Crushable walls keep
    // the running result at zero for static/rank CRUSHER and bypass the later
    // zero Float/Hover Wall row; allied or non-crushable/noncrusher walls do not.
    let mut wall_crush_admitted = false;
    if let Some(overlay_id) = overlay_cell.and_then(|overlay| overlay.overlay_id) {
        let Some(flags) = overlay_registry.and_then(|registry| registry.flags(overlay_id)) else {
            return ProductionUnitAdmission::nonzero(7, layer);
        };
        if flags.wall {
            let allied_wall = overlay_cell
                .and_then(|overlay| overlay.wall_owner)
                .is_some_and(|wall_owner| {
                    crate::map::houses::are_houses_friendly(
                        &sim.house_alliances,
                        owner,
                        sim.interner.resolve(wall_owner),
                    )
                });
            if flags.crushable && regular_crusher && !allied_wall {
                wall_crush_admitted = true;
            } else {
                return ProductionUnitAdmission::nonzero(if allied_wall { 4 } else { 7 }, layer);
            }
        }
    }

    let first_building = sim
        .substrate
        .occupancy
        .first_building_on_layer(cell.0, cell.1, layer);
    let mut crush_victims = Vec::new();
    if let Some(occupancy) = sim.substrate.occupancy.get(cell.0, cell.1) {
        for occupant in occupancy.iter_layer(layer) {
            if occupant.entity_id == produced_id {
                continue;
            }
            let Some(blocker) = sim.substrate.entities.get(occupant.entity_id) else {
                return ProductionUnitAdmission::nonzero(7, layer);
            };
            let blocker_owner = sim.interner.resolve(blocker.owner());
            let allied =
                crate::map::houses::are_houses_friendly(&sim.house_alliances, owner, blocker_owner);

            if blocker.category == EntityCategory::Structure {
                let Some(blocker_type) = rules.object(sim.interner.resolve(blocker.type_ref()))
                else {
                    return ProductionUnitAdmission::nonzero(7, layer);
                };
                if blocker_type.invisible_in_game {
                    continue;
                }
                if matches!(
                    decide_live_vehicle_building_entry(LiveVehicleBuildingEntry {
                        mover_category: EntityCategory::Unit,
                        branch: VehicleBuildingEntryBranch::UnitRepairOrBunker,
                        checked_building_id: blocker.stable_id(),
                        candidate_building_id: first_building,
                        candidate_x: cell.0,
                        building_origin_x: blocker.position.rx,
                        number_impassable_rows: blocker_type.number_impassable_rows,
                        is_unit_repair: blocker_type.unit_repair,
                        is_bunker: blocker_type.bunker,
                        bunker_occupied: blocker.bunker_occupant.is_some(),
                    }),
                    BuildingOccupantEntryDecision::SkipBlocker
                ) {
                    continue;
                }
                if blocker_type.bib
                    && sim.substrate.occupancy.first_building_on_layer(
                        cell.0.wrapping_add(1),
                        cell.1,
                        layer,
                    ) != Some(blocker.stable_id())
                {
                    continue;
                }
                if blocker_type.gate {
                    if blocker
                        .building_gate
                        .is_some_and(|runtime| runtime.can_garrison_passable())
                    {
                        continue;
                    }
                    // RESIDUAL (DRIFT, deferred): native's enemy-gate arm in
                    // `UnitClass::Can_Enter_Cell @ 0x0073F0A0` is
                    // `if (!IsAllied) { if (!this->vt+0x2AC()) return 7; max(5) }`
                    // — `TechnoClass::Is_Armed`, which resolves the ONE slot
                    // `GetCurrentWeapon` picks (the gunner slot for a
                    // `TurretCount>0` type), where VERA tests slots 0 and 1.
                    // `[SREF]`/`[YAGGUN]` now read correctly either way, since
                    // `primary` is native weapon-array slot 0 (`+0x898`, filled
                    // by `Weapon1=` — see `ObjectType::read_weapon_arrays`), so
                    // what is left is the slot-selection difference. Trigger: a
                    // `TurretCount>0` unit whose current gunner slot is empty
                    // while slot 0 or 1 is not, leaving a war factory onto a
                    // cell held by an enemy gate. Player effect: 5 (EnemyBlock)
                    // where gamemd answers 7 (Impassable) — an A*
                    // edge-cost/admission difference on the exit cell only.
                    // Frequency: zero on retail data (no stock section is
                    // shaped that way) and it also needs an enemy gate inside
                    // the producer's exit footprint. Downstream: none. Left
                    // with the other armed-predicate readers outside this
                    // mechanism rather than changed from the weapon-selection
                    // branch.
                    return ProductionUnitAdmission::nonzero(
                        if allied {
                            3
                        } else if object.primary.is_some() || object.secondary.is_some() {
                            5
                        } else {
                            7
                        },
                        layer,
                    );
                }
                // Ordinary buildings, including CABHUT, are blockers. The
                // selected producer id is threaded explicitly; only the live
                // helper above can skip that same yard occupant.
                let _selected_producer = blocker.stable_id() == producer_id;
                return ProductionUnitAdmission::nonzero(if allied { 7 } else { 5 }, layer);
            }

            if allied {
                return ProductionUnitAdmission::nonzero(6, layer);
            }
            if matches!(
                blocker.category,
                EntityCategory::Unit
                    | EntityCategory::Aircraft
                    | EntityCategory::Structure
                    | EntityCategory::Infantry
            ) && blocker.cloak.as_ref().is_some_and(|cloak| cloak.state == 2)
            {
                return ProductionUnitAdmission::nonzero(1, layer);
            }
            if bump_crush::can_crush(
                crush_capability,
                bump_crush::CrushTarget::from_entity(blocker, sim.session.binary_frame),
            ) {
                crush_victims.push(blocker.stable_id());
                continue;
            }
            return ProductionUnitAdmission::nonzero(5, layer);
        }
    }

    // TerrainClass objects participate in the ground object list and are never
    // crushable in active retail data. Early rejection is result-equivalent for
    // this zero-only caller because no later arm can lower a nonzero result.
    if layer == MovementLayer::Ground && sim.production.terrain_object_cells.contains_key(&cell) {
        return ProductionUnitAdmission::nonzero(7, layer);
    }

    // Only the ground-list exhaustion path reads the dynamic SpeedType row.
    // A successful wall crush is the proven escape from the zero Wall row.
    if layer == MovementLayer::Ground
        && !wall_crush_admitted
        && !terrain_cell
            .speed_costs
            .cost_for_speed_type(object.speed_type)
            .is_some_and(|speed| speed != 0)
    {
        return ProductionUnitAdmission::nonzero(7, layer);
    }

    let raw_bits =
        match layer {
            MovementLayer::Ground => sim
                .substrate
                .raw_cell_occupation
                .ground_bits(cell.0, cell.1),
            MovementLayer::Bridge => sim.substrate.raw_cell_occupation.deck_bits(cell.0, cell.1),
            MovementLayer::Air | MovementLayer::Underground => 0,
        } | sim
            .substrate
            .cell_occupation
            .vehicle_bits_ignoring(cell.0, cell.1, layer, produced_id);
    let unit_bit = raw_bits & crate::sim::occupancy::VEHICLE_OCCUPATION_BIT != 0;
    if !crush_victims.is_empty() {
        if unit_bit {
            let first_unit = sim.substrate.occupancy.first_category_on_layer(
                cell.0,
                cell.1,
                layer,
                EntityCategory::Unit,
                &sim.substrate.entities,
            );
            if !first_unit.is_some_and(|unit| crush_victims.contains(&unit)) {
                return ProductionUnitAdmission::nonzero(2, layer);
            }
        }
    } else {
        if unit_bit {
            return ProductionUnitAdmission::nonzero(2, layer);
        }
        if raw_bits & 0x1F != 0 {
            if !regular_crusher {
                // RESIDUAL (DRIFT, deferred): native's matching arm in
                // `UnitClass::Can_Enter_Cell @ 0x0073F0A0` is richer than an
                // armed test — `Type+0xC94 == 0 && (GetWeapon()->WeaponType ==
                // NULL || GetWeapon(0)->Projectile->AG (+0x2A5) == 0)` returns
                // 7, else 5. VERA models only the "names a weapon" half. The
                // slots it reads are now the native weapon-array fields
                // (`primary` = `+0x898`, filled by `Weapon1=` for a
                // `TurretCount>0` type — see `ObjectType::read_weapon_arrays`),
                // so `[SREF]`/`[YAGGUN]` answer 5 here as gamemd does; the
                // omitted terms are `Type+0xC94` and the projectile `AG` flag.
                // Trigger: an armed unit whose slot-0 projectile has `AG=no`
                // (`AEGIS`, `NASAM`, `NAFLAK` shapes), or any type authoring
                // `+0xC94`, exiting onto an occupied non-crushable cell. Player
                // effect: 5 (EnemyBlock) where gamemd answers 7 (Impassable).
                // Frequency: rare — needs a blocked exit cell under a producer,
                // and the AA-only types above are buildings that never exit
                // one. Downstream: none.
                return ProductionUnitAdmission::nonzero(
                    if object.primary.is_some() || object.secondary.is_some() {
                        5
                    } else {
                        7
                    },
                    layer,
                );
            }
            if let Some(infantry_owner) = sim
                .substrate
                .raw_cell_occupation
                .infantry_owner(cell.0, cell.1, layer)
                && crate::map::houses::are_houses_friendly(
                    &sim.house_alliances,
                    owner,
                    sim.interner.resolve(infantry_owner),
                )
            {
                return ProductionUnitAdmission::nonzero(2, layer);
            }
            // Enemy/unowned Infantry masks and stale residual low-bit shapes
            // preserve zero for CRUSHER. No TemporaryOccupation is fabricated.
        }
    }

    ProductionUnitAdmission::ExactZero {
        layer,
        crush_victims,
    }
}

/// One `UnitClass::Unlimbo`-shaped transaction for the queue-held naval Unit:
/// exact-zero CanEnter proceeds to modeled Mark(PUT), while every nonzero code
/// rejects before Mark. Either refusal leaves the same stored identity in limbo.
#[allow(clippy::too_many_arguments)]
pub(super) fn unlimbo_held_naval_unit(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: &str,
    produced_type_id: &str,
    stable_id: u64,
    producer_id: u64,
    cell: (u16, u16),
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    height_map: &std::collections::BTreeMap<(u16, u16), u8>,
) -> Option<u64> {
    let (entry, resolved_cell) = produced_unit_unlimbo_entry(
        sim,
        rules,
        owner,
        produced_type_id,
        stable_id,
        producer_id,
        cell,
        overlay_registry,
    )?;
    let admitted_layer = entry.exact_zero_layer();
    let z = match admitted_layer {
        Some(layer) => {
            let terrain_cell = sim
                .resolved_terrain
                .as_ref()
                .and_then(|terrain| terrain.cell(resolved_cell.0, resolved_cell.1))
                .expect("exact-zero production admission retains its resolved CellClass");
            let z = match layer {
                MovementLayer::Bridge => terrain_cell.bridge_deck_level,
                MovementLayer::Ground => height_map.get(&resolved_cell).copied().unwrap_or(0),
                MovementLayer::Air | MovementLayer::Underground => {
                    unreachable!("production Unit admission selects only ground or bridge")
                }
            };
            if let Some(entity) = sim.substrate.entities.get_mut(stable_id) {
                // `CheckBridgeTraversal @ 0x004D9C60` establishes the selected
                // CellClass plane and its Level+4 deck height before mode-one
                // Mark. Mark then consumes OnBridge for E4/E8 list selection
                // and the committed Z for the +0x124/+0x128 raw plane.
                entity.on_bridge = layer == MovementLayer::Bridge;
            }
            z
        }
        None => height_map.get(&resolved_cell).copied().unwrap_or(0),
    };
    let placement = admitted_layer.map_or(
        crate::sim::world::PlacementEvidence::RejectedEarly,
        |layer| crate::sim::world::PlacementEvidence::UnitCanEnterExactZero { layer },
    );
    let result = sim.unlimbo_held_production_object(
        stable_id,
        resolved_cell.0,
        resolved_cell.1,
        0x40,
        z,
        placement,
        rules,
    );
    assert!(
        admitted_layer.is_none() || result.is_some(),
        "production Unit exact-zero admission established infallible mode-one Mark preconditions"
    );
    result
}

fn producer_queue_category_for_object(
    produced_category: ObjectCategory,
    require_water: bool,
) -> Option<ProductionCategory> {
    match produced_category {
        ObjectCategory::Infantry => Some(ProductionCategory::Infantry),
        ObjectCategory::Vehicle if require_water => Some(ProductionCategory::Ship),
        ObjectCategory::Vehicle => Some(ProductionCategory::Vehicle),
        ObjectCategory::Aircraft => Some(ProductionCategory::Aircraft),
        ObjectCategory::Building => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn find_spawn_cell_near_structure(
    base_rx: u16,
    base_ry: u16,
    structure_id: &str,
    produced_category: ObjectCategory,
    movement_profile: SpawnMovementProfile,
    rules: &RuleSet,
    path_grid: Option<&crate::sim::pathfinding::PathGrid>,
    occupancy: &OccupancyGrid,
    entities: &EntityStore,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    overlay_grid: Option<&crate::sim::overlay_grid::OverlayGrid>,
    zone_grid: Option<&crate::sim::pathfinding::zone_map::ZoneGrid>,
    require_water: bool,
    frame_counter: u32,
    playfield_bounds: Option<crate::sim::cell_rect::PlayfieldBounds>,
    map_size: Option<(i32, i32)>,
) -> Option<(u16, u16)> {
    let offsets: Vec<(i16, i16)> = preferred_exit_offsets(rules, structure_id);
    for (ox, oy) in offsets {
        let Some(cand) = add_cell_offset(base_rx, base_ry, ox, oy) else {
            continue;
        };
        match path_grid {
            Some(grid) => {
                if cand.0 < grid.width()
                    && cand.1 < grid.height()
                    && spawn_cell_passable(grid, cand, resolved_terrain, require_water)
                    && cell_available_for_spawn(
                        cand,
                        produced_category,
                        occupancy,
                        resolved_terrain,
                        require_water,
                    )
                {
                    return Some(cand);
                }
            }
            None => {
                if cell_available_for_spawn(
                    cand,
                    produced_category,
                    occupancy,
                    resolved_terrain,
                    require_water,
                ) {
                    return Some(cand);
                }
            }
        }
    }

    let Some(grid) = path_grid else {
        return Some((base_rx.saturating_add(2), base_ry.saturating_add(2)));
    };
    // AUTHORITATIVE nearby-passable-cell search: the engine's diamond-ring FNPC
    // (frame-counter selection), replacing the old ad-hoc box-ring first-match
    // (`nearest_walkable_around`). This changes the chosen exit/spawn cell (and the
    // hashed spawn position) by design — the FNPC pool order + per-ring early-out +
    // `frame_counter % pool.len()` selection are the verified engine behavior.
    let q = nearby_query_for_spawn(
        movement_profile,
        grid,
        occupancy,
        entities,
        resolved_terrain,
        overlay_grid,
        zone_grid,
        require_water,
        playfield_bounds,
        map_size,
    )?;
    let found = crate::sim::find_nearby_cell::find_nearby_passable_cell(
        (base_rx as i32, base_ry as i32),
        &q,
        frame_counter,
    )?;
    // FNPC's per-candidate passability/occupancy already mirrors the facade, but the
    // spawn layer adds the naval/land terrain-type and sub-cell-availability filter
    // (`cell_available_for_spawn`) that the engine FNPC does not encode; re-apply it so
    // the authoritative pick still honors the land-vs-water and infantry sub-cell rules.
    cell_available_for_spawn(
        found,
        produced_category,
        occupancy,
        resolved_terrain,
        require_water,
    )
    .then_some(found)
}

/// Build the authoritative FNPC query for the spawn/exit fallback from the spawn
/// layer's movement profile + grids. Mirrors the engine FNPC caller args: per-candidate
/// 1x1 passability + occupancy (reservations always SKIPPED), bridges allowed (the spawn
/// path does not forbid bridge cells), required-height `-1`, frame-counter selection
/// (no target). `require_water` routes the movement zone so naval units search water.
/// Live games thread the final normalized `playfield_bounds` fields into the exact
/// isometric corner query. Missing fields reject candidates; there is no terrain-
/// rectangle replacement for active `MapClass::IsRectInPlayfield @ 0x00578390`.
/// Search radius comes independently from the retained signed MapClass `Size`
/// pair; missing size authority rejects the fallback instead of reviving a
/// caller-owned radius.
#[allow(clippy::too_many_arguments)]
fn nearby_query_for_spawn<'a>(
    movement_profile: SpawnMovementProfile,
    grid: &'a crate::sim::pathfinding::PathGrid,
    occupancy: &'a OccupancyGrid,
    entities: &'a EntityStore,
    resolved_terrain: Option<&'a ResolvedTerrainGrid>,
    overlay_grid: Option<&'a crate::sim::overlay_grid::OverlayGrid>,
    zone_grid: Option<&'a crate::sim::pathfinding::zone_map::ZoneGrid>,
    require_water: bool,
    playfield_bounds: Option<crate::sim::cell_rect::PlayfieldBounds>,
    map_size: Option<(i32, i32)>,
) -> Option<crate::sim::find_nearby_cell::NearbyQuery<'a>> {
    use crate::sim::find_nearby_cell::{
        NearbyAnchorGate, NearbyFootprint, NearbyQuery, PassabilityArgs, map_owned_radius_cap,
    };
    let (size_width, size_height) = map_size?;
    let movement_zone = if require_water {
        MovementZone::Water
    } else {
        movement_profile.movement_zone
    };
    Some(NearbyQuery {
        native_cells: None,
        raw_occupation: None,
        passability: PassabilityArgs {
            speed_type: movement_profile.speed_type,
            required_zone_id: None,
            movement_zone,
            bridge_aware_zone: false,
        },
        footprint: NearbyFootprint::SINGLE,
        // gamemd-derived: every FNPC candidate path calls
        // Is_Cell_In_Playfield_CellClass(cell, 1) @ 0x00578540 immediately
        // before CellRect passability, including production spawn/exit callers.
        anchor_gate: NearbyAnchorGate::NativeHeightAware,
        allow_bridge_cells: true,
        check_height: false,
        check_occupancy: true,
        // gamemd-derived: FNPC @ 0x0056DC20 reads signed MapClass Size
        // +0xF4/+0xF8, sums them, and clamps only values above 32.
        radius_cap: map_owned_radius_cap(size_width, size_height),
        target_cell: None,
        path_grid: Some(grid),
        resolved_terrain,
        overlay_grid,
        occupancy: Some(occupancy),
        entities: Some(entities),
        zone_grid,
        playfield_bounds,
    })
}

fn find_exact_exitcoord_spawn_cell(
    base_rx: u16,
    base_ry: u16,
    structure_id: &str,
    produced_category: ObjectCategory,
    rules: &RuleSet,
    path_grid: Option<&crate::sim::pathfinding::PathGrid>,
    occupancy: &OccupancyGrid,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    require_water: bool,
) -> Option<(u16, u16)> {
    let (lx, ly, _lz) = rules.object(structure_id)?.exit_coord?;
    let cand = add_cell_offset(
        base_rx,
        base_ry,
        lepton_to_cell_round_nearest(lx),
        lepton_to_cell_round_nearest(ly),
    )?;
    if let Some(grid) = path_grid {
        if cand.0 >= grid.width()
            || cand.1 >= grid.height()
            || !spawn_cell_passable(grid, cand, resolved_terrain, require_water)
        {
            return None;
        }
    }
    cell_available_for_spawn(
        cand,
        produced_category,
        occupancy,
        resolved_terrain,
        require_water,
    )
    .then_some(cand)
}

/// Infantry-specific spawn cell: the foundation-center cell of the producing
/// barracks. Matches the original engine's alt-path Unlimbo at the building's
/// center lepton coord; `ExitCoord` is intentionally ignored, no passability
/// check is performed, and there is no fallback to a nearby cell.
///
/// The infantry then walks out of the foundation via the existing pathfinder
/// once the rally MoveTo is issued; the foundation cells are passable to
/// infantry (only vehicles are hard-blocked).
fn find_infantry_spawn_cell_near_structure(
    rules: &RuleSet,
    base_rx: u16,
    base_ry: u16,
    structure_id: &str,
) -> Option<(u16, u16)> {
    let obj = rules.object(structure_id)?;
    let (w, h) = super::production_tech::foundation_dimensions(&obj.foundation);
    Some((base_rx.saturating_add(w / 2), base_ry.saturating_add(h / 2)))
}

/// Retired ad-hoc box-ring nearest-cell search. The authoritative spawn/exit
/// fallback now routes through the engine's diamond-ring FNPC
/// (`find_nearby_cell::find_nearby_passable_cell`); this is kept ONLY as the legacy
/// oracle the shadow tests compare the FNPC pool against — it has no production caller.
#[cfg(test)]
fn nearest_walkable_around(
    grid: &crate::sim::pathfinding::PathGrid,
    center: (u16, u16),
    max_radius: u16,
    produced_category: ObjectCategory,
    movement_profile: SpawnMovementProfile,
    occupancy: &OccupancyGrid,
    entities: &EntityStore,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    overlay_grid: Option<&crate::sim::overlay_grid::OverlayGrid>,
    zone_grid: Option<&crate::sim::pathfinding::zone_map::ZoneGrid>,
    playfield_bounds: crate::sim::cell_rect::PlayfieldBounds,
    require_water: bool,
) -> Option<(u16, u16)> {
    let cx = center.0 as i32;
    let cy = center.1 as i32;
    let w = grid.width() as i32;
    let h = grid.height() as i32;
    for r in 1..=max_radius as i32 {
        let min_x = (cx - r).max(0);
        let max_x = (cx + r).min(w - 1);
        let min_y = (cy - r).max(0);
        let max_y = (cy + r).min(h - 1);
        for x in min_x..=max_x {
            let top = (x as u16, min_y as u16);
            if spawn_fallback_candidate_passable(
                grid,
                top,
                movement_profile,
                occupancy,
                entities,
                resolved_terrain,
                overlay_grid,
                zone_grid,
                playfield_bounds,
                require_water,
            ) && cell_available_for_spawn(
                top,
                produced_category,
                occupancy,
                resolved_terrain,
                require_water,
            ) {
                return Some(top);
            }
            let bot = (x as u16, max_y as u16);
            if spawn_fallback_candidate_passable(
                grid,
                bot,
                movement_profile,
                occupancy,
                entities,
                resolved_terrain,
                overlay_grid,
                zone_grid,
                playfield_bounds,
                require_water,
            ) && cell_available_for_spawn(
                bot,
                produced_category,
                occupancy,
                resolved_terrain,
                require_water,
            ) {
                return Some(bot);
            }
        }
        for y in (min_y + 1)..=(max_y - 1) {
            let left = (min_x as u16, y as u16);
            if spawn_fallback_candidate_passable(
                grid,
                left,
                movement_profile,
                occupancy,
                entities,
                resolved_terrain,
                overlay_grid,
                zone_grid,
                playfield_bounds,
                require_water,
            ) && cell_available_for_spawn(
                left,
                produced_category,
                occupancy,
                resolved_terrain,
                require_water,
            ) {
                return Some(left);
            }
            let right = (max_x as u16, y as u16);
            if spawn_fallback_candidate_passable(
                grid,
                right,
                movement_profile,
                occupancy,
                entities,
                resolved_terrain,
                overlay_grid,
                zone_grid,
                playfield_bounds,
                require_water,
            ) && cell_available_for_spawn(
                right,
                produced_category,
                occupancy,
                resolved_terrain,
                require_water,
            ) {
                return Some(right);
            }
        }
    }
    None
}

#[derive(Debug, Clone, Copy)]
struct SpawnMovementProfile {
    speed_type: SpeedType,
    movement_zone: MovementZone,
}

fn spawn_movement_profile(
    rules: &RuleSet,
    produced_type_id: Option<&str>,
    produced_category: ObjectCategory,
    require_water: bool,
) -> SpawnMovementProfile {
    if let Some(obj) = produced_type_id.and_then(|type_id| rules.object(type_id)) {
        return SpawnMovementProfile {
            speed_type: obj.speed_type,
            movement_zone: obj.movement_zone,
        };
    }
    if require_water {
        return SpawnMovementProfile {
            speed_type: SpeedType::Float,
            movement_zone: MovementZone::Water,
        };
    }
    match produced_category {
        ObjectCategory::Infantry => SpawnMovementProfile {
            speed_type: SpeedType::Foot,
            movement_zone: MovementZone::Infantry,
        },
        ObjectCategory::Aircraft => SpawnMovementProfile {
            speed_type: SpeedType::Winged,
            movement_zone: MovementZone::Fly,
        },
        ObjectCategory::Vehicle | ObjectCategory::Building => SpawnMovementProfile {
            speed_type: SpeedType::Track,
            movement_zone: MovementZone::Normal,
        },
    }
}

/// Legacy per-candidate passability+occupancy predicate of the retired box-ring.
/// Kept ONLY for the shadow tests (the authoritative FNPC builds the same per-candidate
/// check through `find_nearby_cell` via the facade); no production caller remains.
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
fn spawn_fallback_candidate_passable(
    grid: &crate::sim::pathfinding::PathGrid,
    cell: (u16, u16),
    movement_profile: SpawnMovementProfile,
    occupancy: &OccupancyGrid,
    entities: &EntityStore,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    overlay_grid: Option<&crate::sim::overlay_grid::OverlayGrid>,
    zone_grid: Option<&crate::sim::pathfinding::zone_map::ZoneGrid>,
    playfield_bounds: crate::sim::cell_rect::PlayfieldBounds,
    require_water: bool,
) -> bool {
    if !spawn_cell_passable(grid, cell, resolved_terrain, require_water) {
        return false;
    }
    if require_water {
        return true;
    }
    let rect = CellRect::single(cell.0, cell.1);
    check_passability_rect(CellRectPassabilityContext {
        native_cells: None,
        rect,
        speed_type: movement_profile.speed_type,
        required_zone_id: None,
        movement_zone: movement_profile.movement_zone,
        required_height_or_level: None,
        bridge_aware_zone: false,
        reject_any_overlay: false,
        path_grid: Some(grid),
        resolved_terrain,
        overlay_grid,
        occupancy: Some(occupancy),
        zone_grid,
    }) && check_occupancy_rect(CellRectOccupancyContext {
        native_cells: None,
        rect,
        reservation_arg: -1,
        reservations: None,
        occupancy: Some(occupancy),
        entities: Some(entities),
        terrain_object_cells: None,
        resolved_terrain,
        overlay_grid,
        playfield_bounds: Some(playfield_bounds),
    })
}

/// Check whether a cell can accept a newly spawned unit. Infantry require a free
/// sub-cell (max 3 per cell). Vehicles/aircraft require no existing blockers.
/// When `require_water` is true, only water cells are accepted (naval units).
/// When false, water cells are rejected (land units shouldn't spawn on water).
fn cell_available_for_spawn(
    cell: (u16, u16),
    produced_category: ObjectCategory,
    occupancy: &OccupancyGrid,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    require_water: bool,
) -> bool {
    // Terrain type filter: naval units need water, land units avoid water.
    if let Some(terrain) = resolved_terrain {
        let is_water = terrain.cell(cell.0, cell.1).map_or(false, |c| c.is_water);
        if require_water && !is_water {
            return false;
        }
        if !require_water && is_water {
            return false;
        }
    }
    let occ = occupancy.get(cell.0, cell.1);
    match produced_category {
        ObjectCategory::Infantry => {
            bump_crush::cell_passable_for_infantry(occ, MovementLayer::Ground)
        }
        _ => {
            // Vehicles/aircraft need no vehicle or structure already in the cell.
            match occ {
                Some(o) => !o.has_blockers_on(MovementLayer::Ground),
                None => true,
            }
        }
    }
}

fn spawn_cell_passable(
    grid: &crate::sim::pathfinding::PathGrid,
    cell: (u16, u16),
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    require_water: bool,
) -> bool {
    if require_water {
        crate::sim::pathfinding::is_cell_passable_for_mover(
            grid,
            cell.0,
            cell.1,
            Some(MovementZone::Water),
            resolved_terrain,
        )
    } else {
        grid.is_walkable(cell.0, cell.1)
    }
}

/// Determine exit cell offsets for a factory building, data-driven from rules.ini.
///
/// If the building has `ExitCoord=X,Y,Z` in rules.ini, converts leptons to a cell
/// offset (256 leptons = 1 cell) and generates candidates around it. Otherwise,
/// falls back to foundation-perimeter offsets derived from the building's Foundation=.
fn preferred_exit_offsets(rules: &RuleSet, structure_id: &str) -> Vec<(i16, i16)> {
    if let Some(obj) = rules.object(structure_id) {
        // Data-driven: use ExitCoord from rules.ini if available.
        if let Some((lx, ly, _lz)) = obj.exit_coord {
            let primary_x: i16 = lepton_to_cell_round_nearest(lx);
            let primary_y: i16 = lepton_to_cell_round_nearest(ly);
            return exit_candidates_around(primary_x, primary_y);
        }
        // No ExitCoord: generate offsets from foundation perimeter.
        let (w, h) = super::production_tech::foundation_dimensions(&obj.foundation);
        return foundation_perimeter_offsets(w as i16, h as i16);
    }
    // Unknown structure: simple default.
    foundation_perimeter_offsets(2, 2)
}

/// Convert a lepton value to the NEAREST cell offset (256 leptons = 1 cell).
///
/// Deliberately round-half-away, NOT the truncating
/// `util::direction_tables::lepton_to_cell` — e.g. 200 leptons is cell 1 here
/// and cell 0 there. Renamed so the two can never be conflated.
fn lepton_to_cell_round_nearest(leptons: i32) -> i16 {
    // Round toward the nearest cell center. +128 for positive, -128 for negative.
    let rounded: i32 = if leptons >= 0 {
        (leptons + 128) / 256
    } else {
        (leptons - 128) / 256
    };
    rounded as i16
}

/// Generate exit candidate offsets around a primary exit cell.
/// Returns the primary cell first, then its 8 neighbors, providing
/// fallback positions if the primary cell is blocked.
fn exit_candidates_around(cx: i16, cy: i16) -> Vec<(i16, i16)> {
    vec![
        (cx, cy),
        (cx + 1, cy),
        (cx - 1, cy),
        (cx, cy + 1),
        (cx, cy - 1),
        (cx + 1, cy + 1),
        (cx - 1, cy + 1),
        (cx + 1, cy - 1),
        (cx - 1, cy - 1),
    ]
}

/// Generate exit offsets around the perimeter of a foundation.
/// Tries bottom edge first, then right edge, then remaining sides.
fn foundation_perimeter_offsets(w: i16, h: i16) -> Vec<(i16, i16)> {
    let mut offsets: Vec<(i16, i16)> = Vec::with_capacity(((w + h) * 2 + 8) as usize);
    // Bottom edge (y = h).
    for x in 0..w {
        offsets.push((x, h));
    }
    // Right edge (x = w).
    for y in 0..h {
        offsets.push((w, y));
    }
    // Top edge (y = -1).
    for x in 0..w {
        offsets.push((x, -1));
    }
    // Left edge (x = -1).
    for y in 0..h {
        offsets.push((-1, y));
    }
    // Corners just outside the foundation.
    offsets.push((w, h));
    offsets.push((-1, -1));
    offsets.push((w, -1));
    offsets.push((-1, h));
    offsets
}

fn add_cell_offset(base_rx: u16, base_ry: u16, ox: i16, oy: i16) -> Option<(u16, u16)> {
    let rx = base_rx as i32 + ox as i32;
    let ry = base_ry as i32 + oy as i32;
    if rx < 0 || ry < 0 {
        return None;
    }
    Some((rx as u16, ry as u16))
}

/// Find an airfield with a free dock slot for a newly produced aircraft.
///
/// Returns `(airfield_stable_id, spawn_rx, spawn_ry)` — the airfield's
/// foundation center cell where the aircraft entity will be placed.
/// Returns `None` if no airfield has a free dock slot.
pub fn find_helipad_for_aircraft(
    sim: &Simulation,
    rules: &RuleSet,
    owner: &str,
) -> Option<(u64, u16, u16)> {
    let owner_id = sim.interner.get(owner)?;

    for entity in sim.substrate.entities.values() {
        if entity.category != crate::map::entities::EntityCategory::Structure {
            continue;
        }
        if entity.health.current == 0 || entity.dying || entity.lifecycle.in_limbo {
            continue;
        }
        if entity.owner() != owner_id {
            continue;
        }
        let type_str = sim.interner.resolve(entity.type_ref());
        let Some(obj) = rules.object(type_str) else {
            continue;
        };
        if !obj.helipad && !obj.unit_reload {
            continue;
        }
        let max_slots = obj.dock_contact_capacity();
        if !sim
            .production
            .airfield_docks
            .has_free_slot(entity.stable_id(), max_slots)
        {
            continue;
        }
        let (fw, fh) = crate::sim::production::foundation_dimensions(&obj.foundation);
        let cx = entity.position.rx + fw / 2;
        let cy = entity.position.ry + fh / 2;
        return Some((entity.stable_id(), cx, cy));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::bridge_facts::BridgeCellFacts;
    use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid, zone_class};
    use crate::rules::terrain_rules::{SpeedCostProfile, TerrainClass};
    use crate::sim::entity_store::EntityStore;
    use crate::sim::pathfinding::PathGrid;

    fn terrain_cell(rx: u16, ry: u16) -> ResolvedTerrainCell {
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
            zone_type: zone_class::GROUND,
            base_ground_walk_blocked: false,
            base_build_blocked: false,
            base_land_type: 0,
            base_yr_cell_land_type: 0,
            base_terrain_class: TerrainClass::Clear,
            base_speed_costs: SpeedCostProfile::default(),
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

    fn flat_terrain(width: u16, height: u16) -> ResolvedTerrainGrid {
        let cells = (0..height)
            .flat_map(|ry| (0..width).map(move |rx| terrain_cell(rx, ry)))
            .collect();
        ResolvedTerrainGrid::from_cells(width, height, cells)
    }

    fn test_playfield_bounds() -> crate::sim::cell_rect::PlayfieldBounds {
        crate::sim::cell_rect::PlayfieldBounds {
            base: 0,
            off_fc: -100,
            off_100: -100,
            off_104: 200,
            off_108: 200,
        }
    }

    fn naval_delivery_rules() -> RuleSet {
        RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[InfantryTypes]\n\
             [VehicleTypes]\n\
             0=DEST\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             0=GAYARD\n\
             1=BLOCKER\n\
             [DEST]\n\
             Strength=600\n\
             Speed=6\n\
             SpeedType=Float\n\
             MovementZone=Water\n\
             Locomotor={2BEA74E1-7CCA-11D3-BE14-00104B62A16C}\n\
             [GAYARD]\n\
             Factory=UnitType\n\
             WeaponsFactory=yes\n\
             Naval=yes\n\
             Foundation=4x4\n\
             [BLOCKER]\n\
             Foundation=1x1\n",
        ))
        .expect("naval delivery rules")
    }

    fn production_admission_rules() -> RuleSet {
        RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[InfantryTypes]\n\
             [VehicleTypes]\n\
             0=SAPC\n\
             1=DEST\n\
             2=RANKER\n\
             3=ARMED\n\
             4=OMNI\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             0=YARD\n\
             1=INVISIBLE\n\
             2=BIBBER\n\
             3=NORMAL\n\
             4=GATE\n\
             5=CABHUT\n\
             6=LASERDEFAULT\n\
             7=FIREDEFAULT\n\
             [SAPC]\n\
             SpeedType=Hover\n\
             Crusher=yes\n\
             [DEST]\n\
             SpeedType=Float\n\
             [RANKER]\n\
             SpeedType=Hover\n\
             VeteranAbilities=CRUSHER\n\
             [ARMED]\n\
             SpeedType=Hover\n\
             Primary=CANNON\n\
             [OMNI]\n\
             SpeedType=Hover\n\
             OmniCrusher=yes\n\
             [YARD]\n\
             Foundation=4x4\n\
             UnitRepair=yes\n\
             NumberImpassableRows=3\n\
             [INVISIBLE]\n\
             InvisibleInGame=yes\n\
             [BIBBER]\n\
             Bib=yes\n\
             [NORMAL]\n\
             Foundation=1x1\n\
             [GATE]\n\
             Gate=yes\n\
             DamagedDoor=yes\n\
             [CABHUT]\n\
             BridgeRepairHut=yes\n\
             [LASERDEFAULT]\n\
             LaserFence=yes\n\
             [FIREDEFAULT]\n\
             Foundation=1x1\n",
        ))
        .expect("production admission rules")
    }

    fn production_admission_sim() -> Simulation {
        let mut sim = Simulation::default();
        let mut terrain = flat_terrain(20, 20);
        for cell in &mut terrain.cells {
            // Retail ground rows used by the compact admission matrix.
            cell.speed_costs.float = Some(0);
            cell.base_speed_costs.float = Some(0);
            cell.speed_costs.hover = Some(50);
            cell.base_speed_costs.hover = Some(50);
        }
        sim.resolved_terrain = Some(terrain);
        sim.playfield_bounds = Some(test_playfield_bounds());
        sim
    }

    fn set_admission_water_cell(sim: &mut Simulation, cell: (u16, u16)) {
        let water = sim
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut(cell.0, cell.1)
            .unwrap();
        water.yr_cell_land_type = crate::rules::terrain_rules::LandType::Water.as_index();
        water.land_type = crate::rules::terrain_rules::LandType::Water.as_index();
        water.terrain_class = TerrainClass::Water;
        water.speed_costs.float = Some(100);
        water.base_speed_costs.float = Some(100);
    }

    fn add_admission_occupant(
        sim: &mut Simulation,
        stable_id: u64,
        type_id: &str,
        owner: &str,
        category: crate::map::entities::EntityCategory,
        origin: (u16, u16),
        occupied_cell: (u16, u16),
        layer: MovementLayer,
    ) {
        let mut entity = crate::sim::game_entity::GameEntity::test_default(
            stable_id, type_id, owner, origin.0, origin.1,
        );
        entity.type_ref = sim.interner.intern(type_id);
        entity.owner = sim.interner.intern(owner);
        entity.category = category;
        sim.substrate.entities.insert(entity);
        sim.substrate.occupancy.add(
            occupied_cell.0,
            occupied_cell.1,
            stable_id,
            layer,
            None,
            if category == crate::map::entities::EntityCategory::Structure {
                crate::sim::occupancy::CellListInsertion::AppendBuilding
            } else {
                crate::sim::occupancy::CellListInsertion::PrependNonBuilding
            },
        );
    }

    fn water_terrain(width: u16, height: u16) -> ResolvedTerrainGrid {
        let mut terrain = flat_terrain(width, height);
        for cell in &mut terrain.cells {
            cell.land_type = crate::rules::terrain_rules::LandType::Water.as_index();
            cell.yr_cell_land_type = crate::rules::terrain_rules::LandType::Water.as_index();
            cell.base_land_type = crate::rules::terrain_rules::LandType::Water.as_index();
            cell.base_yr_cell_land_type = crate::rules::terrain_rules::LandType::Water.as_index();
            cell.terrain_class = TerrainClass::Water;
            cell.base_terrain_class = TerrainClass::Water;
            cell.is_water = true;
            cell.zone_type = zone_class::WATER;
            cell.speed_costs.float = Some(100);
            cell.base_speed_costs.float = Some(100);
        }
        terrain
    }

    fn test_entity(
        stable_id: u64,
        category: crate::map::entities::EntityCategory,
        cell: (u16, u16),
    ) -> crate::sim::game_entity::GameEntity {
        let mut entity = crate::sim::game_entity::GameEntity::test_default(
            stable_id,
            if category == crate::map::entities::EntityCategory::Structure {
                "BLOCKER"
            } else {
                "DEST"
            },
            "Americans",
            cell.0,
            cell.1,
        );
        entity.category = category;
        entity
    }

    #[test]
    fn naval_dispatch_uses_direct_four_flag_predicate_and_getcoords_center() {
        let rules = naval_delivery_rules();
        let yard = rules.object("GAYARD").unwrap();
        assert!(!yard.refinery && !yard.weeder && yard.weapons_factory && yard.naval);
        assert!(exact_naval_vehicle_exit_factory(&rules, "GAYARD"));
        assert_eq!(
            building_get_coords_cell(&rules, "GAYARD", 10, 10),
            Some((12, 12)),
            "4x4 NW (10,10) GetCoords adds 384 leptons then truncates to (12,12)"
        );

        let blocked = RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
             0=YARD\n[YARD]\nFactory=UnitType\nWeaponsFactory=yes\nNaval=yes\nWeeder=yes\n",
        ))
        .unwrap();
        assert!(!exact_naval_vehicle_exit_factory(&blocked, "YARD"));
        assert!(blocked.object("YARD").unwrap().weeder);
    }

    #[test]
    fn production_zero_cell_reaches_height_aware_playfield_rejection() {
        let rules = production_admission_rules();
        let mut sim = production_admission_sim();
        // Flat-terrain native diamond: 12 < x+y <= 26, x-y < 14, y-x < 6.
        // Cell (0,0) remains a valid allocated terrain-grid coordinate, which
        // makes this distinguish the mandatory MapClass predicate from a mere
        // rectangular lookup or a special zero/passability bypass.
        sim.playfield_bounds = Some(crate::sim::cell_rect::PlayfieldBounds {
            base: 10,
            off_fc: 2,
            off_100: 1,
            off_104: 10,
            off_108: 6,
        });

        assert_eq!(
            produced_unit_unlimbo_entry_at_resolved_cell(
                &sim,
                &rules,
                "Americans",
                "SAPC",
                900,
                1,
                (0, 0),
                None,
            ),
            ProductionUnitAdmission::NonZero {
                code: 7,
                layer: MovementLayer::Ground,
            },
            "the zero CellStruct value must reach the ordinary height-aware playfield gate"
        );
        assert!(
            produced_unit_unlimbo_entry_at_resolved_cell(
                &sim,
                &rules,
                "Americans",
                "SAPC",
                900,
                1,
                (8, 8),
                None,
            )
            .exact_zero(),
            "an otherwise-identical in-diamond control remains admissible"
        );
    }

    #[test]
    fn production_cell_center_uses_104_for_real_and_dummy_without_changing_xy() {
        let mut sim = production_admission_sim();
        let requested = (8, 8);
        let flat = resolve_produced_unit_cell_coords(&sim, requested);
        assert_eq!(flat, Some(requested));

        let cell = sim
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut(requested.0, requested.1)
            .unwrap();
        cell.level = 2;
        cell.slope_type = 0;
        assert_eq!(
            crate::util::lepton::ground_height_leptons(
                cell.level,
                cell.slope_type,
                i32::from(requested.0) * 256 + 128,
                i32::from(requested.1) * 256 + 128,
            ),
            Ok(208),
        );
        assert_eq!(
            resolve_produced_unit_cell_coords(&sim, requested),
            flat,
            "GetCoords evaluates raised Z but the current production helper returns only X/Y",
        );

        sim.resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut(requested.0, requested.1)
            .unwrap()
            .slope_type = 21;
        assert_eq!(resolve_produced_unit_cell_coords(&sim, requested), None);

        let dummy_grid = ResolvedTerrainGrid::from_cells(0, 0, Vec::new());
        let mut dummy_sim = Simulation::default();
        dummy_sim.resolved_terrain = Some(dummy_grid);
        let dummy_request = (u16::MAX, 7);
        let flat_dummy = resolve_produced_unit_cell_coords(&dummy_sim, dummy_request);
        dummy_sim
            .resolved_terrain
            .as_ref()
            .unwrap()
            .test_set_dummy_cell_level_slope(2, 0);
        assert_eq!(
            resolve_produced_unit_cell_coords(&dummy_sim, dummy_request),
            flat_dummy,
            "shared-dummy GetCoords evaluates Level 2 as 208 but retains the same signed-center X/Y output",
        );
        dummy_sim
            .resolved_terrain
            .as_ref()
            .unwrap()
            .test_set_dummy_cell_level_slope(2, 21);
        assert_eq!(
            resolve_produced_unit_cell_coords(&dummy_sim, dummy_request),
            None,
            "the native-unsafe slope remains a Rust admission failure on the retained dummy too",
        );
    }

    #[test]
    fn production_admission_ignores_contradictory_path_grid() {
        let rules = production_admission_rules();
        let sim = production_admission_sim();
        let contradictory = PathGrid::test_all_blocked(20, 20);
        assert!(!contradictory.is_walkable(8, 8));

        assert!(
            produced_unit_unlimbo_entry_at_resolved_cell(
                &sim,
                &rules,
                "Americans",
                "SAPC",
                900,
                1,
                (8, 8),
                None,
            )
            .exact_zero(),
            "the exact production tuple has no PathGrid input, so a contradictory grid cannot veto it"
        );
    }

    #[test]
    fn production_ground_speed_requires_row_authority_and_uses_retail_clear_values() {
        let rules = production_admission_rules();
        let cell = (8, 8);
        let admission = |sim: &Simulation, type_id: &str| {
            produced_unit_unlimbo_entry_at_resolved_cell(
                sim,
                &rules,
                "Americans",
                type_id,
                900,
                1,
                cell,
                None,
            )
        };

        let retail_rows = production_admission_sim();
        assert!(
            admission(&retail_rows, "SAPC").exact_zero(),
            "retail Hover/Clear=50 admits an otherwise-empty Clear cell"
        );
        assert_eq!(
            admission(&retail_rows, "DEST"),
            ProductionUnitAdmission::NonZero {
                code: 7,
                layer: MovementLayer::Ground,
            },
            "retail Float/Clear=0 rejects the same Clear cell"
        );

        let mut missing = Simulation::default();
        missing.resolved_terrain = Some(flat_terrain(20, 20));
        missing.playfield_bounds = Some(test_playfield_bounds());
        assert_eq!(
            admission(&missing, "SAPC"),
            ProductionUnitAdmission::NonZero {
                code: 7,
                layer: MovementLayer::Ground,
            },
            "missing the required SpeedType/LandType row is rejected rather than treated as nonzero"
        );
    }

    #[test]
    fn production_structural_low_bridge_selects_deck_plane_and_restriction_exception() {
        let rules = RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[InfantryTypes]\n\
             [VehicleTypes]\n\
             0=HYD\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [HYD]\n\
             SpeedType=Float\n\
             MovementRestrictedTo=Water\n",
        ))
        .expect("restricted naval unit rules");
        let mut sim = production_admission_sim();
        let cell = (8, 8);
        let admission = |sim: &Simulation| {
            produced_unit_unlimbo_entry_at_resolved_cell(
                sim,
                &rules,
                "Americans",
                "HYD",
                900,
                1,
                cell,
                None,
            )
        };

        assert_eq!(
            admission(&sim),
            ProductionUnitAdmission::NonZero {
                code: 7,
                layer: MovementLayer::Ground,
            },
            "ordinary Clear ground fails MovementRestrictedTo=Water"
        );

        {
            let tunnel = sim
                .resolved_terrain
                .as_mut()
                .unwrap()
                .cell_mut(cell.0, cell.1)
                .unwrap();
            tunnel.yr_cell_land_type = crate::rules::terrain_rules::LandType::Tunnel.as_index();
            tunnel.speed_costs.float = Some(0);
        }
        assert_eq!(
            admission(&sim),
            ProductionUnitAdmission::NonZero {
                code: 7,
                layer: MovementLayer::Ground,
            },
            "Tunnel reaches but fails the stock zero Float speed row"
        );

        {
            let bridge = sim
                .resolved_terrain
                .as_mut()
                .unwrap()
                .cell_mut(cell.0, cell.1)
                .unwrap();
            bridge.yr_cell_land_type = crate::rules::terrain_rules::LandType::Clear.as_index();
            bridge.bridge_facts.raw_flags = crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
            bridge.bridge_facts.overlay_id = Some(0xED);
        }
        sim.substrate.raw_cell_occupation.mark_ground(
            cell.0,
            cell.1,
            crate::sim::occupancy::VEHICLE_OCCUPATION_BIT,
        );
        assert_eq!(
            admission(&sim),
            ProductionUnitAdmission::ExactZero {
                layer: MovementLayer::Bridge,
                crush_victims: Vec::new(),
            },
            "BRIDGEB1 bypasses the restriction and ignores the occupied ground plane"
        );

        sim.substrate.raw_cell_occupation.mark_deck(
            cell.0,
            cell.1,
            crate::sim::occupancy::VEHICLE_OCCUPATION_BIT,
        );
        assert_eq!(
            admission(&sim),
            ProductionUnitAdmission::NonZero {
                code: 2,
                layer: MovementLayer::Bridge,
            },
            "the same structural anchor reads the bridge/deck occupation byte"
        );
    }

    #[test]
    fn production_wall_admission_uses_registry_owner_and_rank_crusher() {
        let rules = production_admission_rules();
        let overlay_ini = crate::rules::ini_parser::IniFile::from_str(
            "[OverlayTypes]\n0=TESTWALL\n[TESTWALL]\nWall=yes\nCrushable=yes\nLand=Wall\n",
        );
        let registry = crate::map::overlay_types::OverlayTypeRegistry::from_ini(&overlay_ini, None);
        let cell = (8, 8);
        let admission = |sim: &Simulation, type_id: &str| {
            produced_unit_unlimbo_entry_at_resolved_cell(
                sim,
                &rules,
                "Americans",
                type_id,
                900,
                1,
                cell,
                Some(&registry),
            )
        };

        let mut unowned = production_admission_sim();
        let mut overlay = crate::sim::overlay_grid::OverlayGrid::new(20, 20);
        overlay.place_overlay(cell.0, cell.1, 0, 0);
        unowned.overlay_grid = Some(overlay);
        assert!(admission(&unowned, "SAPC").exact_zero());
        assert_eq!(
            admission(&unowned, "DEST"),
            ProductionUnitAdmission::NonZero {
                code: 7,
                layer: MovementLayer::Ground,
            },
            "a noncrusher naval type cannot pass a crushable wall"
        );

        let mut allied = production_admission_sim();
        let allied_owner = allied.interner.intern("Americans");
        let mut overlay = crate::sim::overlay_grid::OverlayGrid::new(20, 20);
        overlay.place_owned_wall(cell.0, cell.1, 0, 0, allied_owner);
        allied.overlay_grid = Some(overlay);
        assert_eq!(
            admission(&allied, "SAPC"),
            ProductionUnitAdmission::NonZero {
                code: 4,
                layer: MovementLayer::Ground,
            },
            "an allied crushable wall remains nonzero"
        );

        let mut enemy = production_admission_sim();
        let enemy_owner = enemy.interner.intern("Russians");
        let mut overlay = crate::sim::overlay_grid::OverlayGrid::new(20, 20);
        overlay.place_owned_wall(cell.0, cell.1, 0, 0, enemy_owner);
        enemy.overlay_grid = Some(overlay);
        assert!(admission(&enemy, "SAPC").exact_zero());

        let mut ranker = crate::sim::game_entity::GameEntity::test_default(
            900,
            "RANKER",
            "Americans",
            cell.0,
            cell.1,
        );
        ranker.type_ref = enemy.interner.intern("RANKER");
        ranker.owner = enemy.interner.intern("Americans");
        ranker.veterancy = 100;
        enemy.substrate.entities.insert(ranker);
        assert!(
            admission(&enemy, "RANKER").exact_zero(),
            "VeteranAbilities=CRUSHER is selected at veteran rank"
        );
    }

    #[test]
    fn production_building_list_uses_yard_x_columns_invisible_and_bib_east_identity() {
        use crate::map::entities::EntityCategory;

        let rules = production_admission_rules();
        let admission = |sim: &Simulation, cell| {
            produced_unit_unlimbo_entry_at_resolved_cell(
                sim,
                &rules,
                "Americans",
                "SAPC",
                900,
                1,
                cell,
                None,
            )
        };

        let mut west = production_admission_sim();
        add_admission_occupant(
            &mut west,
            10,
            "YARD",
            "Americans",
            EntityCategory::Structure,
            (10, 8),
            (12, 8),
            MovementLayer::Ground,
        );
        assert!(!admission(&west, (12, 8)).exact_zero());

        let mut east = production_admission_sim();
        add_admission_occupant(
            &mut east,
            10,
            "YARD",
            "Americans",
            EntityCategory::Structure,
            (10, 8),
            (13, 8),
            MovementLayer::Ground,
        );
        assert!(
            admission(&east, (13, 8)).exact_zero(),
            "the fourth/eastmost column skips the same yard"
        );
        add_admission_occupant(
            &mut east,
            11,
            "NORMAL",
            "Russians",
            EntityCategory::Structure,
            (13, 8),
            (13, 8),
            MovementLayer::Ground,
        );
        assert!(
            !admission(&east, (13, 8)).exact_zero(),
            "another building later in that eastmost cell still rejects"
        );

        let mut invisible = production_admission_sim();
        add_admission_occupant(
            &mut invisible,
            20,
            "INVISIBLE",
            "Russians",
            EntityCategory::Structure,
            (8, 8),
            (8, 8),
            MovementLayer::Ground,
        );
        assert!(admission(&invisible, (8, 8)).exact_zero());

        let mut bib_edge = production_admission_sim();
        add_admission_occupant(
            &mut bib_edge,
            30,
            "BIBBER",
            "Russians",
            EntityCategory::Structure,
            (8, 8),
            (8, 8),
            MovementLayer::Ground,
        );
        assert!(
            admission(&bib_edge, (8, 8)).exact_zero(),
            "Bib skips when the first building one cell east is not the same identity"
        );
        bib_edge.substrate.occupancy.add(
            9,
            8,
            30,
            MovementLayer::Ground,
            None,
            crate::sim::occupancy::CellListInsertion::AppendBuilding,
        );
        assert!(
            !admission(&bib_edge, (8, 8)).exact_zero(),
            "Bib does not skip while the same building continues east"
        );
    }

    #[test]
    fn production_gate_only_skips_mission_open_stable_and_maps_failure_codes() {
        use crate::map::entities::EntityCategory;
        use crate::sim::game_entity::{BuildingGatePhase, BuildingGateRuntime};

        let rules = production_admission_rules();
        let cell = (8, 8);
        let make_gate = |owner: &str, phase: BuildingGatePhase, mission_18_active: bool| {
            let mut sim = production_admission_sim();
            add_admission_occupant(
                &mut sim,
                40,
                "GATE",
                owner,
                EntityCategory::Structure,
                cell,
                cell,
                MovementLayer::Ground,
            );
            sim.substrate.entities.get_mut(40).unwrap().building_gate = Some(BuildingGateRuntime {
                phase,
                mission_18_active,
                ..BuildingGateRuntime::default()
            });
            sim
        };
        let admission = |sim: &Simulation, type_id: &str| {
            produced_unit_unlimbo_entry_at_resolved_cell(
                sim,
                &rules,
                "Americans",
                type_id,
                900,
                1,
                cell,
                None,
            )
        };

        let open = make_gate("Russians", BuildingGatePhase::OpenStable, true);
        assert!(
            admission(&open, "SAPC").exact_zero(),
            "mission 0x18 plus stable-open skips the gate; DamagedDoor is deliberately not read"
        );
        let wrong_mission = make_gate("Russians", BuildingGatePhase::OpenStable, false);
        assert_eq!(
            admission(&wrong_mission, "ARMED"),
            ProductionUnitAdmission::NonZero {
                code: 5,
                layer: MovementLayer::Ground,
            }
        );

        for phase in [
            BuildingGatePhase::ClosedStable,
            BuildingGatePhase::Opening,
            BuildingGatePhase::Closing,
        ] {
            let allied = make_gate("Americans", phase, true);
            assert_eq!(
                admission(&allied, "ARMED"),
                ProductionUnitAdmission::NonZero {
                    code: 3,
                    layer: MovementLayer::Ground,
                }
            );
            let hostile_armed = make_gate("Russians", phase, true);
            assert_eq!(
                admission(&hostile_armed, "ARMED"),
                ProductionUnitAdmission::NonZero {
                    code: 5,
                    layer: MovementLayer::Ground,
                }
            );
            let hostile_unarmed = make_gate("Russians", phase, true);
            assert_eq!(
                admission(&hostile_unarmed, "DEST"),
                ProductionUnitAdmission::NonZero {
                    code: 7,
                    layer: MovementLayer::Ground,
                }
            );
        }
    }

    #[test]
    fn production_inactive_fence_defaults_cabhut_and_terrain_are_terminal_blockers() {
        use crate::map::entities::EntityCategory;

        let rules = production_admission_rules();
        assert!(rules.object("LASERDEFAULT").unwrap().laser_fence);
        // Active retail has no FirestormWall=yes type. The fixture therefore
        // leaves FIREDEFAULT on the ordinary ObjectType defaults instead of
        // guessing the retained TS arm; DamagedDoor is likewise not a field
        // consumed by the production evaluator.
        assert!(!rules.object("FIREDEFAULT").unwrap().laser_fence);
        let cell = (8, 8);
        let admission = |sim: &Simulation| {
            produced_unit_unlimbo_entry_at_resolved_cell(
                sim,
                &rules,
                "Americans",
                "DEST",
                900,
                1,
                cell,
                None,
            )
        };

        for type_id in ["CABHUT", "LASERDEFAULT", "FIREDEFAULT"] {
            let mut sim = production_admission_sim();
            add_admission_occupant(
                &mut sim,
                50,
                type_id,
                "Russians",
                EntityCategory::Structure,
                cell,
                cell,
                MovementLayer::Ground,
            );
            assert!(
                !admission(&sim).exact_zero(),
                "{type_id} must remain an ordinary terminal blocker in the active stock slice"
            );
        }
        assert!(rules.object("CABHUT").unwrap().bridge_repair_hut);

        let mut terrain = production_admission_sim();
        terrain.production.terrain_object_cells.insert(cell, 60);
        assert!(
            !admission(&terrain).exact_zero(),
            "a live TerrainClass cell rejects even without a PathGrid bit or fabricated object"
        );
    }

    #[test]
    fn production_fully_cloaked_techno_rejects_before_successful_crush() {
        use crate::map::entities::EntityCategory;

        let rules = production_admission_rules();
        let cell = (8, 8);
        let admission = |sim: &Simulation, type_id: &str| {
            produced_unit_unlimbo_entry_at_resolved_cell(
                sim,
                &rules,
                "Americans",
                type_id,
                900,
                1,
                cell,
                None,
            )
        };

        let mut enemy = production_admission_sim();
        add_admission_occupant(
            &mut enemy,
            60,
            "E1",
            "Russians",
            EntityCategory::Infantry,
            cell,
            cell,
            MovementLayer::Ground,
        );
        enemy.substrate.entities.get_mut(60).unwrap().crushable = true;
        assert_eq!(
            admission(&enemy, "SAPC"),
            ProductionUnitAdmission::ExactZero {
                layer: MovementLayer::Ground,
                crush_victims: vec![60],
            },
            "non-cloaked enemy crushable infantry preserves exact zero"
        );
        assert_eq!(
            admission(&enemy, "DEST"),
            ProductionUnitAdmission::NonZero {
                code: 5,
                layer: MovementLayer::Ground,
            },
            "an unarmed noncrusher cannot use the crush escape"
        );

        let mut cloak = crate::sim::cloak_disguise::CloakRuntime::new(0, 1);
        cloak.state = 2;
        enemy.substrate.entities.get_mut(60).unwrap().cloak = Some(cloak);
        assert_eq!(
            admission(&enemy, "SAPC"),
            ProductionUnitAdmission::NonZero {
                code: 1,
                layer: MovementLayer::Ground,
            },
            "fully-cloaked Techno code 1 is evaluated before crushability"
        );

        let mut allied = production_admission_sim();
        add_admission_occupant(
            &mut allied,
            61,
            "E1",
            "Americans",
            EntityCategory::Infantry,
            cell,
            cell,
            MovementLayer::Ground,
        );
        allied.substrate.entities.get_mut(61).unwrap().crushable = true;
        assert_eq!(
            admission(&allied, "SAPC"),
            ProductionUnitAdmission::NonZero {
                code: 6,
                layer: MovementLayer::Ground,
            },
            "alliance rejection precedes enemy crush handling"
        );

        let mut mixed = production_admission_sim();
        add_admission_occupant(
            &mut mixed,
            62,
            "E1",
            "Russians",
            EntityCategory::Infantry,
            cell,
            cell,
            MovementLayer::Ground,
        );
        mixed.substrate.entities.get_mut(62).unwrap().crushable = true;
        add_admission_occupant(
            &mut mixed,
            63,
            "HEAVY",
            "Russians",
            EntityCategory::Infantry,
            cell,
            cell,
            MovementLayer::Ground,
        );
        assert!(
            !admission(&mixed, "SAPC").exact_zero(),
            "a later noncrushable object keeps the mixed cell nonzero"
        );
    }

    #[test]
    fn production_raw_selected_plane_tail_preserves_verified_zero_shapes() {
        use crate::map::entities::EntityCategory;

        let rules = production_admission_rules();
        let cell = (8, 8);
        let admission = |sim: &Simulation, type_id: &str| {
            produced_unit_unlimbo_entry_at_resolved_cell(
                sim,
                &rules,
                "Americans",
                type_id,
                900,
                1,
                cell,
                None,
            )
        };

        let mut unit_only = production_admission_sim();
        unit_only.substrate.raw_cell_occupation.mark_ground(
            cell.0,
            cell.1,
            crate::sim::occupancy::VEHICLE_OCCUPATION_BIT,
        );
        assert_eq!(
            admission(&unit_only, "SAPC"),
            ProductionUnitAdmission::NonZero {
                code: 2,
                layer: MovementLayer::Ground,
            }
        );

        let mut enemy_infantry = production_admission_sim();
        let mut infantry =
            crate::sim::game_entity::GameEntity::test_default(70, "E1", "Russians", cell.0, cell.1);
        infantry.category = EntityCategory::Infantry;
        infantry.owner = enemy_infantry.interner.intern("Russians");
        enemy_infantry.substrate.entities.insert(infantry);
        enemy_infantry
            .substrate
            .raw_cell_occupation
            .mark_ground_infantry(
                cell.0,
                cell.1,
                0x04,
                enemy_infantry.interner.get("Russians").unwrap(),
            );
        assert!(admission(&enemy_infantry, "SAPC").exact_zero());
        assert!(
            !admission(&enemy_infantry, "DEST").exact_zero(),
            "noncrusher low-bit occupation remains nonzero"
        );

        let mut residual = production_admission_sim();
        residual
            .substrate
            .raw_cell_occupation
            .mark_ground(cell.0, cell.1, 0x01);
        assert!(
            admission(&residual, "SAPC").exact_zero(),
            "verified list-empty residual low bit remains zero without TemporaryOccupation"
        );

        let mut ordered_unit = production_admission_sim();
        add_admission_occupant(
            &mut ordered_unit,
            71,
            "CRUSHABLEUNIT",
            "Russians",
            EntityCategory::Unit,
            cell,
            cell,
            MovementLayer::Ground,
        );
        ordered_unit
            .substrate
            .entities
            .get_mut(71)
            .unwrap()
            .crushable = true;
        ordered_unit.substrate.raw_cell_occupation.mark_ground(
            cell.0,
            cell.1,
            crate::sim::occupancy::VEHICLE_OCCUPATION_BIT,
        );
        assert_eq!(
            admission(&ordered_unit, "OMNI"),
            ProductionUnitAdmission::ExactZero {
                layer: MovementLayer::Ground,
                crush_victims: vec![71],
            },
            "Unit bit stays zero only when the first Unit-list identity is the crush victim"
        );

        let mut structural = production_admission_sim();
        let bridge = structural
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut(cell.0, cell.1)
            .unwrap();
        bridge.bridge_facts.raw_flags = crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
        structural.substrate.raw_cell_occupation.mark_ground(
            cell.0,
            cell.1,
            crate::sim::occupancy::VEHICLE_OCCUPATION_BIT,
        );
        assert!(admission(&structural, "SAPC").exact_zero());
        structural.substrate.raw_cell_occupation.mark_deck(
            cell.0,
            cell.1,
            crate::sim::occupancy::VEHICLE_OCCUPATION_BIT,
        );
        assert_eq!(
            admission(&structural, "SAPC"),
            ProductionUnitAdmission::NonZero {
                code: 2,
                layer: MovementLayer::Bridge,
            }
        );
    }

    #[test]
    fn production_exact_zero_unlimbo_marks_held_identity_without_forced_outcome() {
        let rules = production_admission_rules();
        let mut sim = production_admission_sim();
        set_admission_water_cell(&mut sim, (8, 8));
        let stable_id = sim
            .create_production_object_limbo_at_height("DEST", "Americans", 8, 8, 0x40, 0, &rules)
            .expect("held production Unit");
        assert_eq!(
            unlimbo_held_naval_unit(
                &mut sim,
                &rules,
                "Americans",
                "DEST",
                stable_id,
                1,
                (8, 8),
                None,
                &std::collections::BTreeMap::new(),
            ),
            Some(stable_id),
            "the production API exposes no caller-forced Mark outcome"
        );
        let entity = sim.substrate.entities.get(stable_id).unwrap();
        assert!(!entity.lifecycle.in_limbo && entity.lifecycle.cell_marked);
    }

    #[test]
    fn production_structural_bridge_unlimbo_marks_e8_deck_at_native_height() {
        let rules = production_admission_rules();
        let cell = (8, 8);
        let make_structural_sim = || {
            let mut sim = production_admission_sim();
            {
                let bridge = sim
                    .resolved_terrain
                    .as_mut()
                    .unwrap()
                    .cell_mut(cell.0, cell.1)
                    .unwrap();
                bridge.level = 3;
                bridge.bridge_deck_level = 7;
                bridge.has_bridge_deck = true;
                bridge.bridge_walkable = true;
                bridge.bridge_facts.raw_flags = crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
            }
            sim.bridge_state = Some(
                crate::sim::bridge_state::BridgeRuntimeState::from_resolved_terrain(
                    sim.resolved_terrain.as_ref().unwrap(),
                    true,
                    300,
                ),
            );
            sim
        };

        let mut sim = make_structural_sim();
        let stable_id = sim
            .create_production_object_limbo_at_height(
                "DEST",
                "Americans",
                cell.0,
                cell.1,
                0x40,
                3,
                &rules,
            )
            .expect("held production Unit");
        let height_map = std::collections::BTreeMap::from([(cell, 3)]);
        assert_eq!(
            unlimbo_held_naval_unit(
                &mut sim,
                &rules,
                "Americans",
                "DEST",
                stable_id,
                1,
                cell,
                None,
                &height_map,
            ),
            Some(stable_id)
        );

        let entity = sim.substrate.entities.get(stable_id).unwrap();
        assert!(
            entity.on_bridge,
            "selected bridge admission establishes OnBridge"
        );
        assert_eq!(entity.position.z, 7, "native deck Z is CellClass Level+4");
        let occupancy = sim.substrate.occupancy.get(cell.0, cell.1).unwrap();
        assert_eq!(
            occupancy
                .iter_layer(MovementLayer::Bridge)
                .map(|entry| entry.entity_id)
                .collect::<Vec<_>>(),
            vec![stable_id],
            "mode-one Mark links the Unit into CellClass+0xE8"
        );
        assert!(
            occupancy.iter_layer(MovementLayer::Ground).next().is_none(),
            "mode-one Mark must not link the Unit into CellClass+0xE4"
        );
        assert_eq!(
            sim.substrate.raw_cell_occupation.deck_bits(cell.0, cell.1)
                & crate::sim::occupancy::VEHICLE_OCCUPATION_BIT,
            crate::sim::occupancy::VEHICLE_OCCUPATION_BIT
        );
        assert_eq!(
            sim.substrate
                .raw_cell_occupation
                .ground_bits(cell.0, cell.1)
                & crate::sim::occupancy::VEHICLE_OCCUPATION_BIT,
            0
        );
        assert_eq!(
            sim.substrate
                .cell_occupation
                .vehicle_bits(cell.0, cell.1, MovementLayer::Bridge,),
            crate::sim::occupancy::VEHICLE_OCCUPATION_BIT
        );
        assert_eq!(
            sim.substrate
                .cell_occupation
                .vehicle_bits(cell.0, cell.1, MovementLayer::Ground,),
            0
        );

        let mut selected_plane = make_structural_sim();
        selected_plane.substrate.raw_cell_occupation.mark_ground(
            cell.0,
            cell.1,
            crate::sim::occupancy::VEHICLE_OCCUPATION_BIT,
        );
        assert!(
            produced_unit_unlimbo_entry_at_resolved_cell(
                &selected_plane,
                &rules,
                "Americans",
                "DEST",
                900,
                1,
                cell,
                None,
            )
            .exact_zero(),
            "ground-plane occupation does not block structural bridge admission"
        );
        selected_plane.substrate.raw_cell_occupation.mark_deck(
            cell.0,
            cell.1,
            crate::sim::occupancy::VEHICLE_OCCUPATION_BIT,
        );
        assert_eq!(
            produced_unit_unlimbo_entry_at_resolved_cell(
                &selected_plane,
                &rules,
                "Americans",
                "DEST",
                900,
                1,
                cell,
                None,
            ),
            ProductionUnitAdmission::NonZero {
                code: 2,
                layer: MovementLayer::Bridge,
            },
            "deck-plane occupation rejects the same structural bridge admission"
        );
    }

    #[test]
    #[should_panic(
        expected = "production Unit exact-zero admission established infallible mode-one Mark preconditions"
    )]
    fn production_exact_zero_internal_mark_inconsistency_is_invariant_failure() {
        let rules = production_admission_rules();
        let mut sim = production_admission_sim();
        set_admission_water_cell(&mut sim, (8, 8));
        let stable_id = sim
            .create_production_object_limbo_at_height("DEST", "Americans", 8, 8, 0x40, 0, &rules)
            .expect("held production Unit");
        sim.substrate
            .entities
            .get_mut(stable_id)
            .unwrap()
            .lifecycle
            .cell_marked = true;
        let _ = unlimbo_held_naval_unit(
            &mut sim,
            &rules,
            "Americans",
            "DEST",
            stable_id,
            1,
            (8, 8),
            None,
            &std::collections::BTreeMap::new(),
        );
    }

    #[test]
    fn naval_fallback_query_uses_dynamic_speed_literal_zone_zero_and_live_frame_pool() {
        let mut terrain = water_terrain(7, 7);
        for cell in &mut terrain.cells {
            cell.speed_costs.float = Some(0);
            cell.base_speed_costs.float = Some(0);
        }
        for candidate in [(2, 1), (3, 1)] {
            let cell = terrain.cell_mut(candidate.0, candidate.1).unwrap();
            cell.speed_costs.float = Some(100);
            cell.base_speed_costs.float = Some(100);
        }
        let grid = PathGrid::from_resolved_terrain(&terrain);
        let query = nearby_query_for_naval_unit_delivery(
            SpeedType::Float,
            &grid,
            Some(&terrain),
            None,
            None,
            Some(test_playfield_bounds()),
            Some((7, 7)),
        )
        .unwrap();

        assert_eq!(query.passability.speed_type, SpeedType::Float);
        assert_eq!(query.passability.movement_zone, MovementZone::Normal);
        assert_eq!(query.passability.required_zone_id, None);
        assert!(!query.passability.bridge_aware_zone);
        assert_eq!(
            query.footprint,
            crate::sim::find_nearby_cell::NearbyFootprint::SINGLE
        );
        assert!(query.allow_bridge_cells);
        assert!(!query.check_height && !query.check_occupancy);
        assert_eq!(query.target_cell, None);
        assert_eq!(
            crate::sim::find_nearby_cell::find_nearby_passable_cell((3, 2), &query, 0),
            Some((2, 1))
        );
        assert_eq!(
            crate::sim::find_nearby_cell::find_nearby_passable_cell((3, 2), &query, 1),
            Some((3, 1)),
            "live frame modulo chooses the second engine-ordered survivor"
        );
    }

    #[test]
    fn naval_rally_walk_observes_first_building_list_order() {
        let mut occupancy = OccupancyGrid::new();
        let append = crate::sim::occupancy::CellListInsertion::AppendBuilding;
        occupancy.add(12, 12, 10, MovementLayer::Ground, None, append);
        occupancy.add(13, 12, 11, MovementLayer::Ground, None, append);
        occupancy.add(13, 12, 10, MovementLayer::Ground, None, append);
        assert_eq!(
            naval_rally_walk_candidate(10, (10, 10), (12, 12), (20, 10), &occupancy),
            (13, 12),
            "a different first Building stops even when the producer is later"
        );

        let mut producer_first = OccupancyGrid::new();
        producer_first.add(12, 12, 10, MovementLayer::Ground, None, append);
        producer_first.add(13, 12, 10, MovementLayer::Ground, None, append);
        producer_first.add(13, 12, 11, MovementLayer::Ground, None, append);
        assert_eq!(
            naval_rally_walk_candidate(10, (10, 10), (12, 12), (20, 10), &producer_first,),
            (14, 12),
            "producer-first order continues through the foundation run"
        );
    }

    #[test]
    fn naval_rally_fast_path_requires_water_empty_and_playfield_then_bypasses_fnpc() {
        let terrain = water_terrain(32, 32);
        let bounds = test_playfield_bounds();
        let mut occupancy = OccupancyGrid::new();
        let append = crate::sim::occupancy::CellListInsertion::AppendBuilding;
        occupancy.add(12, 12, 10, MovementLayer::Ground, None, append);
        occupancy.add(13, 12, 10, MovementLayer::Ground, None, append);
        let mut entities = EntityStore::new();
        entities.insert(test_entity(
            10,
            crate::map::entities::EntityCategory::Structure,
            (10, 10),
        ));

        assert_eq!(
            naval_rally_fast_path_cell(
                10,
                (10, 10),
                (12, 12),
                (20, 10),
                &occupancy,
                &entities,
                Some(&terrain),
                Some(bounds),
            ),
            Some((14, 12))
        );

        let mut land_failure = water_terrain(32, 32);
        land_failure.cell_mut(14, 12).unwrap().yr_cell_land_type =
            crate::rules::terrain_rules::LandType::Clear.as_index();
        assert_eq!(
            naval_rally_fast_path_cell(
                10,
                (10, 10),
                (12, 12),
                (20, 10),
                &occupancy,
                &entities,
                Some(&land_failure),
                Some(bounds),
            ),
            None,
            "LandType gate failure must fall back"
        );

        let mut object_occupancy = occupancy.clone();
        object_occupancy.add(
            14,
            12,
            20,
            MovementLayer::Ground,
            None,
            crate::sim::occupancy::CellListInsertion::PrependNonBuilding,
        );
        entities.insert(test_entity(
            20,
            crate::map::entities::EntityCategory::Unit,
            (14, 12),
        ));
        assert_eq!(
            naval_rally_fast_path_cell(
                10,
                (10, 10),
                (12, 12),
                (20, 10),
                &object_occupancy,
                &entities,
                Some(&terrain),
                Some(bounds),
            ),
            None,
            "selector-zero active object gate failure must fall back"
        );
        assert_eq!(
            naval_rally_fast_path_cell(
                10,
                (10, 10),
                (12, 12),
                (20, 10),
                &occupancy,
                &entities,
                Some(&terrain),
                None,
            ),
            None,
            "missing/failed mode-one playfield authority must fall back"
        );

        let rules = naval_delivery_rules();
        let grid = PathGrid::from_resolved_terrain(&land_failure);
        assert_eq!(
            find_naval_unit_delivery_cell(
                10,
                10,
                10,
                "GAYARD",
                Some((20, 10)),
                SpawnMovementProfile {
                    speed_type: SpeedType::Float,
                    movement_zone: MovementZone::Water,
                },
                &rules,
                Some(&grid),
                &occupancy,
                &entities,
                Some(&land_failure),
                None,
                None,
                0,
                Some(bounds),
                Some((32, 32)),
            ),
            ((12, 12), false),
            "failed direct gate restarts FNPC at the original GetCoords centre"
        );
    }

    #[test]
    fn nearby_fallback_uses_cellrect_occupancy_blockers() {
        let mut terrain = flat_terrain(3, 3);
        terrain.cells[0].slope_type = 1;
        let path_grid = PathGrid::from_resolved_terrain(&terrain);
        let occupancy = OccupancyGrid::new();
        let entities = EntityStore::new();
        let movement_profile = SpawnMovementProfile {
            speed_type: SpeedType::Track,
            movement_zone: MovementZone::Normal,
        };

        let cell = nearest_walkable_around(
            &path_grid,
            (1, 1),
            1,
            ObjectCategory::Vehicle,
            movement_profile,
            &occupancy,
            &entities,
            Some(&terrain),
            None,
            None,
            test_playfield_bounds(),
            false,
        );

        assert_eq!(cell, Some((0, 2)));
    }

    #[test]
    fn spawn_fnpc_radius_uses_installed_map_size_and_reaches_ring_twelve() {
        const GRID: u16 = 40;
        const SEED: (u16, u16) = (20, 20);
        const RING_TWELVE: (u16, u16) = (8, 8);

        let mut terrain = flat_terrain(GRID, GRID);
        for cell in &mut terrain.cells {
            cell.ground_walk_blocked = true;
            cell.base_ground_walk_blocked = true;
            cell.speed_costs.track = Some(0);
            cell.base_speed_costs.track = Some(0);
        }
        let survivor = terrain
            .cell_mut(RING_TWELVE.0, RING_TWELVE.1)
            .expect("ring-twelve survivor");
        survivor.ground_walk_blocked = false;
        survivor.base_ground_walk_blocked = false;
        survivor.speed_costs.track = Some(100);
        survivor.base_speed_costs.track = Some(100);

        let path_grid = PathGrid::from_resolved_terrain(&terrain);
        let occupancy = OccupancyGrid::new();
        let entities = EntityStore::new();
        let movement_profile = SpawnMovementProfile {
            speed_type: SpeedType::Track,
            movement_zone: MovementZone::Normal,
        };
        let rules = RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n",
        ))
        .expect("minimal spawn rules");

        let find = |map_size| {
            find_spawn_cell_near_structure(
                SEED.0,
                SEED.1,
                "UNKNOWN",
                ObjectCategory::Vehicle,
                movement_profile,
                &rules,
                Some(&path_grid),
                &occupancy,
                &entities,
                Some(&terrain),
                None,
                None,
                false,
                0,
                Some(test_playfield_bounds()),
                map_size,
            )
        };

        assert_eq!(
            find(Some((20, 20))),
            Some(RING_TWELVE),
            "MapClass Size sum 40 clamps to 32 and reaches ring 12"
        );
        assert_eq!(
            find(Some((6, 6))),
            None,
            "a native cap of 12 scans only rings 0 through 11"
        );
        assert_eq!(
            find(Some((-10, 10))),
            None,
            "a nonpositive signed Size sum performs no ring search"
        );
        assert_eq!(
            find(None),
            None,
            "missing installed MapClass Size authority must not invent a radius"
        );
    }

    #[test]
    fn spawn_fnpc_anchor_gate_excludes_earlier_rectangular_candidate() {
        const GRID: u16 = 40;
        const SEED: (u16, u16) = (9, 4);
        const OFF_DIAMOND: (u16, u16) = (7, 2);
        const IN_DIAMOND: (u16, u16) = (7, 6);

        let mut terrain = flat_terrain(GRID, GRID);
        for cell in &mut terrain.cells {
            cell.ground_walk_blocked = true;
            cell.base_ground_walk_blocked = true;
            cell.speed_costs.track = Some(0);
            cell.base_speed_costs.track = Some(0);
        }
        for candidate in [OFF_DIAMOND, IN_DIAMOND] {
            let cell = terrain
                .cell_mut(candidate.0, candidate.1)
                .expect("ring-two candidate");
            cell.ground_walk_blocked = false;
            cell.base_ground_walk_blocked = false;
            cell.speed_costs.track = Some(100);
            cell.base_speed_costs.track = Some(100);
        }

        let path_grid = PathGrid::from_resolved_terrain(&terrain);
        let occupancy = OccupancyGrid::new();
        let entities = EntityStore::new();
        let movement_profile = SpawnMovementProfile {
            speed_type: SpeedType::Track,
            movement_zone: MovementZone::Normal,
        };
        // Flat-terrain diamond: 12 < x+y <= 26, x-y < 14, y-x < 6.
        // Both candidates are valid cells inside the 40x40 terrain rectangle,
        // but ring order visits off-diamond (7,2) before in-diamond (7,6).
        let bounds = crate::sim::cell_rect::PlayfieldBounds {
            base: 10,
            off_fc: 2,
            off_100: 1,
            off_104: 10,
            off_108: 6,
        };
        let rules = RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n",
        ))
        .expect("minimal spawn rules");

        let query = nearby_query_for_spawn(
            movement_profile,
            &path_grid,
            &occupancy,
            &entities,
            Some(&terrain),
            None,
            None,
            false,
            Some(bounds),
            Some((bounds.base, 10)),
        )
        .expect("installed MapClass authority");
        assert!(matches!(
            query.anchor_gate,
            crate::sim::find_nearby_cell::NearbyAnchorGate::NativeHeightAware
        ));

        assert_eq!(
            find_spawn_cell_near_structure(
                SEED.0,
                SEED.1,
                "UNKNOWN",
                ObjectCategory::Vehicle,
                movement_profile,
                &rules,
                Some(&path_grid),
                &occupancy,
                &entities,
                Some(&terrain),
                None,
                None,
                false,
                0,
                Some(bounds),
                Some((bounds.base, 10)),
            ),
            Some(IN_DIAMOND),
            "the independent anchor gate excludes the earlier off-diamond survivor"
        );
    }

    // --- T5: shadow-assert the FNPC search against the legacy box-ring ---

    #[test]
    fn find_nearby_candidate_set_shadows_nearest_walkable_around() {
        // Shadow the new diamond-ring FNPC against the legacy box-ring on the same
        // grid. They CHOOSE differently by design (frame-counter vs first-match), so
        // we do NOT assert the chosen cell. Instead we assert the FNPC pick is itself
        // a cell the legacy predicate would accept — surfacing search-shape divergence
        // without flipping any authoritative output.
        use crate::sim::find_nearby_cell::{
            NearbyAnchorGate, NearbyFootprint, NearbyQuery, PassabilityArgs, RADIUS_HARD_CAP,
            find_nearby_passable_cell,
        };
        let terrain = flat_terrain(7, 7);
        let path_grid = PathGrid::from_resolved_terrain(&terrain);
        let occupancy = OccupancyGrid::new();
        let entities = EntityStore::new();
        let movement_profile = SpawnMovementProfile {
            speed_type: SpeedType::Track,
            movement_zone: MovementZone::Normal,
        };

        let q = NearbyQuery {
            native_cells: None,
            raw_occupation: None,
            passability: PassabilityArgs {
                speed_type: movement_profile.speed_type,
                required_zone_id: None,
                movement_zone: movement_profile.movement_zone,
                bridge_aware_zone: false,
            },
            footprint: NearbyFootprint::SINGLE,
            anchor_gate: NearbyAnchorGate::UnverifiedCompatibilityBypass,
            allow_bridge_cells: true,
            check_height: false,
            check_occupancy: true,
            radius_cap: RADIUS_HARD_CAP,
            target_cell: None,
            path_grid: Some(&path_grid),
            resolved_terrain: Some(&terrain),
            overlay_grid: None,
            occupancy: Some(&occupancy),
            entities: Some(&entities),
            zone_grid: None,
            playfield_bounds: Some(test_playfield_bounds()),
        };

        let fnpc = find_nearby_passable_cell((3, 3), &q, 0).expect("FNPC finds a cell");
        // The FNPC pick must pass the legacy predicate the box-ring used per candidate.
        assert!(
            spawn_fallback_candidate_passable(
                &path_grid,
                fnpc,
                movement_profile,
                &occupancy,
                &entities,
                Some(&terrain),
                None,
                None,
                test_playfield_bounds(),
                false,
            ) && cell_available_for_spawn(
                fnpc,
                ObjectCategory::Vehicle,
                &occupancy,
                Some(&terrain),
                false,
            ),
            "FNPC chose ({},{}) which the legacy box-ring predicate rejects — search-shape divergence",
            fnpc.0,
            fnpc.1
        );
    }

    // --- T6: the spawn fallback's accept/reject is the facade's verdict ---

    #[test]
    fn spawn_fallback_uses_validator_predicates() {
        // The spawn fallback's per-candidate verdict is single-sourced through the
        // facade predicates. On a free land cell the combined helper accepts; a
        // structure-blocked cell is rejected by the occupancy facade.
        let terrain = flat_terrain(3, 3);
        let path_grid = PathGrid::from_resolved_terrain(&terrain);
        let occupancy = OccupancyGrid::new();
        let entities = EntityStore::new();
        let movement_profile = SpawnMovementProfile {
            speed_type: SpeedType::Track,
            movement_zone: MovementZone::Normal,
        };

        // Free cell -> the facade-backed helper accepts.
        assert!(spawn_fallback_candidate_passable(
            &path_grid,
            (1, 1),
            movement_profile,
            &occupancy,
            &entities,
            Some(&terrain),
            None,
            None,
            test_playfield_bounds(),
            false,
        ));

        // The facade occupancy predicate matches the helper: both agree the cell is free.
        let facade_ok = check_occupancy_rect(CellRectOccupancyContext {
            native_cells: None,
            rect: CellRect::single(1, 1),
            reservation_arg: -1,
            reservations: None,
            occupancy: Some(&occupancy),
            entities: Some(&entities),
            terrain_object_cells: None,
            resolved_terrain: Some(&terrain),
            overlay_grid: None,
            playfield_bounds: Some(test_playfield_bounds()),
        });
        assert!(facade_ok);
    }

    #[test]
    fn spawn_fallback_no_hash_change_when_predicates_agree() {
        // Routing the per-candidate decision through the facade does not change the
        // chosen cell: the legacy box-ring over the facade predicates returns the same
        // first-match cell it did before the reconcile (proves the invert is hash-neutral).
        let mut terrain = flat_terrain(3, 3);
        terrain.cells[0].slope_type = 1; // (0,0) blocked, mirrors the existing fixture
        let path_grid = PathGrid::from_resolved_terrain(&terrain);
        let occupancy = OccupancyGrid::new();
        let entities = EntityStore::new();
        let movement_profile = SpawnMovementProfile {
            speed_type: SpeedType::Track,
            movement_zone: MovementZone::Normal,
        };
        let cell = nearest_walkable_around(
            &path_grid,
            (1, 1),
            1,
            ObjectCategory::Vehicle,
            movement_profile,
            &occupancy,
            &entities,
            Some(&terrain),
            None,
            None,
            test_playfield_bounds(),
            false,
        );
        // Same authoritative first-match the legacy ring produced (no behavior flip).
        assert_eq!(cell, Some((0, 2)));
    }
}
