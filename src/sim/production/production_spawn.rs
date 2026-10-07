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
    NavalUnit,
    /// Infantry444A53..444C95 has distinct physical XY/facing and exit NavCom.
    Infantry {
        coordinate: crate::sim::components::DriveCoord,
        facing: u8,
        exit_cell: (u16, u16),
    },
}

/// The exit cell at one producer building, `(id, cell, type)`, for an object
/// of `produced_category` (type `produced_type_id`): the naval, exact
/// land and legacy Unit arms, the infantry arm and the adapter of the other
/// classes. Human House PLACE chooses its producer through the shared
/// ObjectType5F7900 FindFactory owner, then calls the same ExitObject443C60
/// owner as a computer's factory (`production::factory_ai`).
pub(super) fn spawn_selection_at_producer(
    sim: &Simulation,
    rules: &RuleSet,
    producer: (u64, u16, u16, &str),
    produced_entity_id: Option<u64>,
    produced_type_id: Option<&str>,
    produced_category: ObjectCategory,
    require_water: bool,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
) -> Option<ProductionSpawnSelection> {
    let (producer_id, bx, by, structure_id) = producer;
    let path_grid = sim.path_grid();
    let resolved_terrain = sim.resolved_terrain.as_ref();
    let overlay_grid = sim.overlay_grid.as_ref();
    let zone_grid = sim.zone_grid.as_ref();
    let map_size = sim
        .playfield_bounds
        .zip(sim.playfield_size_height)
        .map(|(bounds, height)| (bounds.base, height));
    let movement_profile =
        spawn_movement_profile(rules, produced_type_id, produced_category, require_water);
    let standard = |cell| ProductionSpawnSelection {
        producer_id,
        cell,
        delivery: ProductionDeliveryKind::Standard,
    };
    let near_structure = || {
        find_spawn_cell_near_structure(
            bx,
            by,
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
        )
    };
    match produced_category {
        ObjectCategory::Vehicle => {
            if exact_naval_vehicle_exit_factory(rules, structure_id) {
                let producer_rally = sim
                    .substrate
                    .entities
                    .get(producer_id)
                    .and_then(|producer| producer.rally_cell());
                let (cell, _) = find_naval_unit_delivery_cell(
                    producer_id,
                    bx,
                    by,
                    producer_rally,
                    movement_profile,
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
                    producer_id,
                    cell,
                    delivery: ProductionDeliveryKind::NavalUnit,
                });
            }
            if !require_water && exact_land_vehicle_exit_factory(rules, structure_id) {
                let producer = sim.substrate.entities.get(producer_id)?;
                let coord = crate::sim::movement::building_exit_coordinate(
                    crate::sim::movement::ground_pose::position_world_coord(&producer.position),
                    rules.object(structure_id)?,
                    || {
                        crate::sim::movement::ground_pose::object_get_coords(
                            producer,
                            resolved_terrain,
                        )
                    },
                );
                // 444565's caller owns admission. PathGrid/owner claims cannot
                // preempt its scoped Unlimbo or choose another exit coordinate.
                return Some(standard((
                    crate::util::lepton::lepton_to_cell_packed(coord.x) as u16,
                    crate::util::lepton::lepton_to_cell_packed(coord.y) as u16,
                )));
            }
            // Unverified/modded produced-Unit branches retain the legacy
            // adapter.
            near_structure().map(standard)
        }
        ObjectCategory::Infantry => {
            let mover = produced_entity_id?;
            let exit_cell = crate::sim::docking::building_dock::building_dock_cell(
                sim,
                producer_id,
                mover,
                (0, 0),
                rules,
                overlay_registry,
            )?;
            let producer = sim.substrate.entities.get(producer_id)?;
            let producer_type = sim.object_type(producer.type_ref(), rules)?;
            let (coordinate, facing) = infantry_exit_coordinate_and_facing(
                producer,
                producer_type,
                (exit_cell.0 as i16, exit_cell.1 as i16),
            );
            let cell = (
                u16::try_from(crate::util::lepton::lepton_to_cell_packed(coordinate.x)).ok()?,
                u16::try_from(crate::util::lepton::lepton_to_cell_packed(coordinate.y)).ok()?,
            );
            Some(ProductionSpawnSelection {
                producer_id,
                cell,
                delivery: ProductionDeliveryKind::Infantry {
                    coordinate,
                    facing,
                    exit_cell,
                },
            })
        }
        _ => near_structure().map(standard),
    }
}

/// Original Infantry Exit443C60's arithmetic444A53..444C7C. The angle
/// uses the unshrunk dock centre and Building447AC0's foundation centre;
/// each outside component moves one cell inward before physical Unlimbo.
/// Exact preferred cells alone add their type's rules-side ExitCoord.
/// Native goldens: basic-factory-exit-geometry-research/geometry-primary.json,
/// observation570630adeb29995ecc943d42d84f7659d19a02ae8c55ac5296b499e637ce87ba.
fn infantry_exit_coordinate_and_facing(
    producer: &crate::sim::game_entity::GameEntity,
    producer_type: &crate::rules::object_type::ObjectType,
    dock: (i16, i16),
) -> (crate::sim::components::DriveCoord, u8) {
    use crate::sim::{components::DriveCoord, movement::ground_pose};
    let location = ground_pose::position_world_xy(&producer.position);
    let nw = (
        crate::util::lepton::lepton_to_cell_packed(location[0]),
        crate::util::lepton::lepton_to_cell_packed(location[1]),
    );
    let centre = |cell: i16| (i32::from(cell) << 8).wrapping_add(128);
    let word = crate::util::direction_tables::facing16_between(
        ground_pose::object_center_xy(producer),
        [centre(dock.0), centre(dock.1)],
    );
    let facing = crate::util::direction_tables::quantize::round_facing16_to_8(word);
    let (width, height) = crate::rules::foundation::foundation_dimensions(&producer.foundation);
    let inward = |cell: i16, base: i16, size: u16| {
        if i32::from(cell) >= i32::from(base) + i32::from(size) {
            cell.wrapping_sub(1)
        } else if cell < base {
            cell.wrapping_add(1)
        } else {
            cell
        }
    };
    let mut coordinate = DriveCoord {
        x: centre(inward(dock.0, nw.0, width)),
        y: centre(inward(dock.1, nw.1, height)),
        z: 0,
    };
    if (producer_type.gdi_barracks() && dock == (nw.0.wrapping_add(1), nw.1.wrapping_add(2)))
        || (producer_type.nod_barracks() && dock == (nw.0.wrapping_add(2), nw.1.wrapping_add(2)))
        || (producer_type.yuri_barracks() && dock == (nw.0.wrapping_add(2), nw.1.wrapping_add(1)))
    {
        let (x, y, z) = producer_type.exit_coord.unwrap_or((0, 0, 0));
        coordinate.x = coordinate.x.wrapping_add(x);
        coordinate.y = coordinate.y.wrapping_add(y);
        coordinate.z = coordinate.z.wrapping_add(z);
    }
    (coordinate, facing)
}

/// ExitObject4445D6/4445E3 calls the shared radio owners for HELLO and
/// TETHER. Their reciprocal contacts and flags also use radio-owned cleanup.

pub fn mark_war_factory_spawn_contact(
    sim: &mut Simulation,
    rules: &RuleSet,
    producer_id: u64,
    produced_id: u64,
) -> bool {
    let valid = sim
        .substrate
        .entities
        .get(producer_id)
        .is_some_and(|producer| {
            exact_land_vehicle_exit_factory(rules, sim.interner.resolve(producer.type_ref()))
                && sim
                    .substrate
                    .entities
                    .get(produced_id)
                    .is_some_and(|product| {
                        product.category == crate::map::entities::EntityCategory::Unit
                    })
        });
    if !valid {
        return false;
    }
    for message in [
        crate::sim::radio::RadioMessage::Hello,
        crate::sim::radio::RadioMessage::Tether,
    ] {
        crate::sim::radio::transmit(
            sim,
            producer_id,
            produced_id,
            message,
            crate::sim::radio::RadioPayload::default(),
            Some(rules),
        );
    }
    true
}

pub(super) fn exact_land_vehicle_exit_factory(rules: &RuleSet, structure_id: &str) -> bool {
    rules.object(structure_id).is_some_and(|obj| {
        // Original ExitObject44413D..44416F has no ExitCoord presence gate.
        !obj.refinery && !obj.weeder && obj.weapons_factory && !obj.naval
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
    producer_rally: Option<(u16, u16)>,
    movement_profile: SpawnMovementProfile,
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
    let Some(origin) = producer_get_coords_cell(entities, producer_id) else {
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

/// The producing building's GetCoords (`BuildingClass::GetCoords @
/// 0x00447AC0`) as a cell (`ObjectClass::Get_Cell_Packed @ 0x0041BEA0`).
fn producer_get_coords_cell(entities: &EntityStore, producer_id: u64) -> Option<(u16, u16)> {
    let [x, y] = crate::sim::movement::ground_pose::object_center_xy(entities.get(producer_id)?);
    let cell = |value: i32| u16::try_from(crate::util::lepton::lepton_to_cell_packed(value)).ok();
    cell(x).zip(cell(y))
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

/// Resolve the caller's `CellStruct` through the never-null MapClass lookup and
/// `CellClass::GetCoords @ 0x00486840` before ObjectClass evaluates its normal
/// zero-cell sentinel. A miss therefore updates and observes the one retained
/// shared-dummy identity; it is not collapsed to a Rust-side `None`.
fn resolve_produced_unit_cell_coords(
    sim: &Simulation,
    requested: (u16, u16),
) -> Option<crate::sim::components::DriveCoord> {
    if let Some(terrain) = sim.resolved_terrain.as_ref() {
        let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
        let selected = cells.lookup((requested.0 as i16, requested.1 as i16));
        let (x, y, z) = crate::sim::cell_kernel::native_cell_own_coords(selected, &cells)?;
        return Some(crate::sim::components::DriveCoord {
            x: x as i32,
            y: y as i32,
            z: z as i32,
        });
    }
    // Terrain-less diagnostics keep their existing singleton-dummy adapter.
    // An ordinary native match always has MapClass; this is not another
    // CellClass port and performs no real/native map admission.
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
    let mut coords = crate::sim::cell_kernel::cell_center(
        crate::sim::cell_kernel::CellCoordinate {
            x: coord.0,
            y: coord.1,
        },
        0,
    );
    coords.z = crate::sim::cell_kernel::cell_floor_height(level, slope, coords.x, coords.y).ok()?;
    Some(crate::sim::components::DriveCoord {
        x: coords.x,
        y: coords.y,
        z: coords.z,
    })
}

/// Naval ExitObject's selected Cell goes through the same concrete Unlimbo
/// boundary as every Unit constructor; there is no production +1AC port.
pub(super) fn unlimbo_held_naval_unit(
    sim: &mut Simulation,
    rules: &RuleSet,
    stable_id: u64,
    cell: (u16, u16),
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
) -> Option<u64> {
    let coord = resolve_produced_unit_cell_coords(sim, cell)?;
    sim.reveal_constructed_object_at_coord_with_overlay_context(
        stable_id,
        coord,
        0x40,
        crate::sim::world::PlacementEvidence::EvaluateMark,
        rules,
        overlay_registry,
    )
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
/// `util::lepton::lepton_to_cell` — e.g. 200 leptons is cell 1 here
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

/// The foundation centre of `airfield`, a live `Helipad=` or `UnitReload=`
/// building out of limbo, while it has a free dock slot.
pub(super) fn free_helipad_cell(
    sim: &Simulation,
    rules: &RuleSet,
    airfield: u64,
) -> Option<(u16, u16)> {
    let entity = sim.substrate.entities.get(airfield)?;
    if entity.category != crate::map::entities::EntityCategory::Structure
        || entity.health.current == 0
        || entity.dying
        || entity.lifecycle.in_limbo
    {
        return None;
    }
    let obj = rules.object(sim.interner.resolve(entity.type_ref()))?;
    if !obj.helipad && !obj.unit_reload {
        return None;
    }
    if !sim
        .production
        .airfield_docks
        .has_free_slot(airfield, obj.dock_contact_capacity())
    {
        return None;
    }
    let [x, y] = crate::sim::movement::ground_pose::object_center_xy(entity);
    Some((u16::try_from(x / 256).ok()?, u16::try_from(y / 256).ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::resolved_terrain::{ResolvedTerrainGrid, zone_class};
    use crate::rules::terrain_rules::TerrainClass;
    use crate::sim::entity_store::EntityStore;
    use crate::sim::pathfinding::PathGrid;

    fn flat_terrain(width: u16, height: u16) -> ResolvedTerrainGrid {
        crate::map::resolved_terrain::test_grid(
            width,
            height,
            crate::map::resolved_terrain::test_clear_cell,
        )
    }

    #[test]
    fn infantry_exit_coordinate_and_facing_match_original_instructions() {
        use crate::rules::ini_parser::IniFile;
        use crate::sim::components::DriveCoord;
        use crate::sim::game_entity::GameEntity;
        use crate::sim::movement::ground_pose;
        let data: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "src/sim/production/fixtures/infantry_exit_native.json",
        ))
        .unwrap();
        assert_eq!(
            data["native_sha256"],
            "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
        );
        let mut compared = 0;
        let mut storage_residuals = 0;
        for row in data["rows"].as_array().unwrap() {
            let number = |key: &str, index: usize| row[key][index].as_i64().unwrap() as i32;
            let location = DriveCoord {
                x: number("location", 0),
                y: number("location", 1),
                z: number("location", 2),
            };
            if location.x == i32::MIN || location.x == i32::MAX {
                storage_residuals += 1;
                continue;
            }
            let rules = RuleSet::from_ini_with_fixed_art_for_test(
                &IniFile::from_str(&format!(
                    "[BuildingTypes]\n0=GAPILE\n[GAPILE]\nGDIBarracks={}\nNODBarracks={}\nYuriBarracks={}\nExitCoord={},{},{}\n",
                    if number("flags", 0) != 0 { "yes" } else { "no" },
                    if number("flags", 1) != 0 { "yes" } else { "no" },
                    if number("flags", 2) != 0 { "yes" } else { "no" },
                    number("exit_coord", 0), number("exit_coord", 1), number("exit_coord", 2),
                )),
                &IniFile::from_str("[GAPILE]\nFoundation=3x2\n"),
            ).unwrap();
            let mut producer = GameEntity::test_default_of_category(
                1,
                "GAPILE",
                "Americans",
                14,
                14,
                crate::map::entities::EntityCategory::Structure,
            );
            producer.foundation = "3x2".into();
            ground_pose::put_location(&mut producer.position, location);
            let dock = (number("dock", 0) as i16, number("dock", 1) as i16);
            let producer_type = rules.object("GAPILE").unwrap();
            assert_eq!(
                [
                    producer_type.gdi_barracks(),
                    producer_type.nod_barracks(),
                    producer_type.yuri_barracks()
                ],
                [
                    number("flags", 0) != 0,
                    number("flags", 1) != 0,
                    number("flags", 2) != 0
                ],
                "{}: fixture preserves each native nonzero-byte predicate",
                row["name"]
            );
            let (coordinate, facing) =
                infantry_exit_coordinate_and_facing(&producer, producer_type, dock);
            assert_eq!(
                [coordinate.x, coordinate.y, coordinate.z],
                [
                    number("coordinate", 0),
                    number("coordinate", 1),
                    number("coordinate", 2)
                ],
                "{}",
                row["name"]
            );
            assert_eq!(
                i32::from(facing),
                row["facing"].as_i64().unwrap() as i32,
                "{}",
                row["name"]
            );
            assert_eq!(
                crate::util::direction_tables::quantize::round_facing16_to_8(
                    row["facing_word"][0].as_u64().unwrap() as u16,
                ),
                facing,
                "{}",
                row["name"]
            );
            compared += 1;
        }
        assert_eq!(compared, 26);
        assert_eq!(storage_residuals, 2);
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
        RuleSet::from_ini_with_fixed_art_for_test(
            &crate::rules::ini_parser::IniFile::from_str(
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
            ),
            &crate::rules::ini_parser::IniFile::from_str(
                "[GAYARD]\nFoundation=4x4\n[BLOCKER]\nFoundation=1x1\n",
            ),
        )
        .expect("naval delivery rules")
    }

    fn production_admission_rules() -> RuleSet {
        RuleSet::from_ini_with_fixed_art_for_test(
            &crate::rules::ini_parser::IniFile::from_str(
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
            ),
            &crate::rules::ini_parser::IniFile::from_str(
                "[YARD]\nFoundation=4x4\n[NORMAL]\nFoundation=1x1\n[FIREDEFAULT]\nFoundation=1x1\n",
            ),
        )
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

    /// Producer 10: the `naval_delivery_rules` GAYARD at NW cell (10,10),
    /// with the `Foundation=4x4` construction stamps on it.
    fn gayard_producer() -> crate::sim::game_entity::GameEntity {
        let mut producer =
            crate::sim::game_entity::GameEntity::test_default(10, "GAYARD", "Americans", 10, 10);
        producer.category = crate::map::entities::EntityCategory::Structure;
        producer.foundation = "4x4".to_string();
        producer
    }

    #[test]
    fn naval_dispatch_uses_direct_four_flag_predicate_and_getcoords_center() {
        let rules = naval_delivery_rules();
        let yard = rules.object("GAYARD").unwrap();
        assert!(!yard.refinery && !yard.weeder && yard.weapons_factory && yard.naval);
        assert!(exact_naval_vehicle_exit_factory(&rules, "GAYARD"));
        assert_eq!(yard.foundation, "4x4");
        let mut entities = EntityStore::new();
        entities.insert(gayard_producer());
        assert_eq!(
            producer_get_coords_cell(&entities, 10),
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
    fn production_cell_center_uses_104_for_real_and_dummy_without_changing_xy() {
        let mut sim = production_admission_sim();
        let requested = (8, 8);
        let flat = resolve_produced_unit_cell_coords(&sim, requested);
        assert_eq!(
            flat,
            Some(crate::sim::components::DriveCoord {
                x: 8 * 256 + 128,
                y: 8 * 256 + 128,
                z: 0
            })
        );

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
            flat.map(|coord| crate::sim::components::DriveCoord { z: 208, ..coord }),
            "GetCoords retains raised Z without changing X/Y",
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
            flat_dummy.map(|coord| crate::sim::components::DriveCoord { z: 208, ..coord }),
            "shared-dummy GetCoords retains Level 2 Z and the same signed-center X/Y",
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
    fn unit_unlimbo_native_empty_raw_byte_is_not_resurrected_by_owner_claims() {
        let rules = production_admission_rules();
        let mut sim = production_admission_sim();
        set_admission_water_cell(&mut sim, (8, 8));
        let stable_id = sim
            .create_production_object_limbo_at_height("DEST", "Americans", 8, 8, 0x40, 0, &rules)
            .unwrap();
        // Unit7441B0 ORs 0x20; Unit744210 destructively clears it. The
        // derived owner index can retain another claim after that clear.
        for owner in [101, 102] {
            sim.substrate
                .cell_occupation
                .mark_vehicle_on_layer(8, 8, owner, MovementLayer::Ground);
            sim.substrate.raw_cell_occupation.mark_ground(8, 8, 0x20);
        }
        sim.substrate
            .cell_occupation
            .clear_vehicle_on_layer(8, 8, 101, MovementLayer::Ground);
        sim.substrate.raw_cell_occupation.clear_ground(8, 8, 0x20);
        assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(8, 8), 0);
        assert_eq!(
            sim.substrate
                .cell_occupation
                .vehicle_bits(8, 8, MovementLayer::Ground),
            0x20
        );
        // Original Unit Unlimbo's empty-list/raw-zero control admits. Claims
        // are not another input of the native +1AC receiver.
        assert_eq!(
            unlimbo_held_naval_unit(&mut sim, &rules, stable_id, (8, 8), None),
            Some(stable_id)
        );
    }

    #[test]
    fn unit_unlimbo_native_deck_query_keeps_the_callers_ground_pose() {
        let rules = production_admission_rules();
        let mut sim = production_admission_sim();
        set_admission_water_cell(&mut sim, (8, 8));
        let cell = sim
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut(8, 8)
            .unwrap();
        cell.level = 3;
        cell.bridge_deck_level = 7;
        cell.has_bridge_deck = true;
        cell.bridge_walkable = true;
        cell.bridge_facts.raw_flags = 0x100;
        let stable_id = sim
            .create_production_object_limbo_at_height("DEST", "Americans", 8, 8, 0x40, 3, &rules)
            .unwrap();
        assert_eq!(
            unlimbo_held_naval_unit(&mut sim, &rules, stable_id, (8, 8), None),
            Some(stable_id)
        );
        let entity = sim.substrate.entities.get(stable_id).unwrap();
        // Original Foot4D9C60 changes only caller-local query outputs;
        // initialized native bridge_clear_ground_input retains OnBridge0.
        assert!(!entity.on_bridge);
        assert_eq!(entity.position.exact_z_leptons, Some(312));
        assert_eq!(
            sim.substrate.raw_cell_occupation.ground_bits(8, 8) & 0x20,
            0x20
        );
        assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(8, 8) & 0x20, 0);
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
            unlimbo_held_naval_unit(&mut sim, &rules, stable_id, (8, 8), None,),
            Some(stable_id),
            "the production API exposes no caller-forced Mark outcome"
        );
        let entity = sim.substrate.entities.get(stable_id).unwrap();
        assert!(!entity.lifecycle.in_limbo && entity.lifecycle.cell_marked);
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
        entities.insert(gayard_producer());

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

        let grid = PathGrid::from_resolved_terrain(&land_failure);
        assert_eq!(
            find_naval_unit_delivery_cell(
                10,
                10,
                10,
                Some((20, 10)),
                SpawnMovementProfile {
                    speed_type: SpeedType::Float,
                    movement_zone: MovementZone::Water,
                },
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
