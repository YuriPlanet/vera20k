//! Grounded ObjectClass coordinate writes shared by movement and placement.
//!
//! `ObjectClass::SetHeight @ 0x005F5FA0` ([`set_height`]) samples the committed
//! world XY through GetGroundHeight @ 0x00578080, then adds the explicit
//! OnBridge offset. Callers own cadence: Drive/Ship residual movement does
//! not call this setter and retains the last raw coordinate Z.

use crate::map::cell_index::NativeCellIdentity;
use crate::map::resolved_terrain::{NativeCellQuery, ResolvedTerrainGrid};
use crate::sim::components::{DriveCoord, Position};
use crate::sim::pathfinding::PathGrid;
use crate::sim::world::Simulation;
use crate::util::lepton::{
    BRIDGE_DECK_HEIGHT_LEPTONS, GROUND_LEVEL_HEIGHT_LEPTONS, ground_height_leptons,
};

impl Simulation {
    /// Object+1C4, Object5F6A10: query virtual+4C with a null requester,
    /// truncate its XY to signed cell words, then perform one Map5657A0
    /// lookup. A Foot therefore uses its tube exit or paid head through
    /// Foot4DBDF0; this is distinct from physical Object+1BC below.
    /// Executed G730D60 callers: tools/input_oracle/area_guard.json.
    pub(crate) fn object_navigation_cell(
        &self,
        id: u64,
        rules: &crate::rules::ruleset::RuleSet,
    ) -> Result<(i16, i16), String> {
        let coordinate = super::navcom::nav_target_coordinate(
            crate::sim::components::NavTargetRef::Entity { id },
            None,
            &self.substrate.entities,
            self.resolved_terrain.as_ref(),
            Some((rules, &self.interner)),
        )?;
        let terrain = self
            .resolved_terrain
            .as_ref()
            .ok_or("Object navigation-cell query requires the native map")?;
        let cells = NativeCellQuery::canonical(terrain);
        let cell = cells.lookup(((coordinate.x / 256) as i16, (coordinate.y / 256) as i16));
        Ok(cells.coord(cell))
    }

    /// ObjectClass::SetZ5F6060 writes only Object+A4. A marked owner runs
    /// virtual Mark(REMOVE), stores Z at5F607A, then Mark(PUT). The unmarked
    /// leaf5F6092 stores Z directly; it does not move OpenTopped riders.
    /// Executed native controls: spatial_oracle/jumpjet_states.py.
    pub(crate) fn set_object_z(
        &mut self,
        id: u64,
        z: i32,
        rules: Option<&crate::rules::ruleset::RuleSet>,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) {
        let marked = self
            .substrate
            .entities
            .get(id)
            .expect("SetZ owner exists")
            .lifecycle
            .cell_marked;
        if marked {
            self.foot_mark_remove(id, rules, registry);
        }
        self.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .position
            .exact_z_leptons = Some(z);
        if marked {
            self.foot_mark_put(id, rules, registry);
        }
    }
}

/// Map578080 through the caller's query identity. Input queries isolate Dummy;
/// simulation callbacks use the canonical retained Dummy and its lookup order.
pub(crate) fn query_ground_height(
    cells: &NativeCellQuery<'_>,
    point: DriveCoord,
) -> Result<i32, String> {
    let cell = cells.lookup_world(point.x, point.y);
    let (level, slope) = cells.ground_fields(cell);
    ground_height_leptons(level, slope, point.x, point.y)
        .map_err(|error| format!("native ground query: {error:?}"))
}

/// Object+1BC receiver5F6960 performs two Map565730 lookups from physical
/// Object+9C. The first lookup is observable when it stamps the shared Dummy;
/// the second returns the identity retained by the caller.
pub(crate) fn query_object_cell(
    cells: &NativeCellQuery<'_>,
    physical: DriveCoord,
) -> NativeCellIdentity {
    let _ = cells.lookup_world(physical.x, physical.y);
    cells.lookup_world(physical.x, physical.y)
}

/// Object5F5F00: signed current-cell level plus four for OnBridge, through
/// the same Object+1BC query used by firing and movement.
pub(crate) fn query_object_cell_height(
    cells: &NativeCellQuery<'_>,
    physical: DriveCoord,
    on_bridge: bool,
) -> i32 {
    let cell = query_object_cell(cells, physical);
    i32::from(cells.ground_fields(cell).0 as i8) + if on_bridge { 4 } else { 0 }
}

/// Techno70C620, virtual+2B0: compare Map6D6410's terrain projection to
/// Object41BEA0's current packed cell. Both consume raw Location XY; Z is
/// ignored. Callers retain the projection's ordered canonical Dummy lookups.
/// Native body/caller: tools/spatial_oracle/unit_simple_deploy.md.
pub(crate) fn terrain_projection_differs(cells: &NativeCellQuery<'_>, [x, y]: [i32; 2]) -> bool {
    let cell = (
        i32::from(crate::util::lepton::lepton_to_cell_packed(x)),
        i32::from(crate::util::lepton::lepton_to_cell_packed(y)),
    );
    crate::sim::find_nearby_cell::project_world_coordinate_with_lookup(x, y, |cx, cy| {
        cells.projection_view(cx, cy)
    }) != cell
}

/// Foot+BC4DDC40(false) -> Object5F6A70. The navigation coordinate can be a
/// paid head; source bridge selection is independent of the cached path layer.
/// Both ground samples precede the conditional structural-cell lookup.
/// Original-byte layer and consumer-seam receipts, including312/313 and
/// Tube/Dummy ordering: tools/spatial_oracle/foot_bridge_layer.{json,md}.
pub(crate) fn navigation_should_be_on_bridge(
    cells: &NativeCellQuery<'_>,
    navigation: DriveCoord,
    current: DriveCoord,
    on_bridge: bool,
    in_tube: bool,
) -> Result<bool, String> {
    if in_tube {
        return Ok(false);
    }
    let head_ground = query_ground_height(cells, navigation)?;
    let current_ground = query_ground_height(cells, current)?;
    if !on_bridge && current_ground.wrapping_sub(head_ground) > 3 * GROUND_LEVEL_HEIGHT_LEPTONS {
        return Ok(cells.flags(cells.lookup_world(navigation.x, navigation.y)) & 0x100 != 0);
    }
    if on_bridge && head_ground.wrapping_sub(current_ground) > 3 * GROUND_LEVEL_HEIGHT_LEPTONS {
        return Ok(false);
    }
    Ok(on_bridge)
}

/// The world XY of an object's Location (`ObjectClass+0x9C`, `+0xA0`) from
/// the cell and sub-cell [`set_position_world_xy`] stores.
pub(crate) fn position_world_xy(position: &Position) -> [i32; 2] {
    [
        i32::from(position.rx)
            .wrapping_mul(256)
            .wrapping_add(position.sub_x.to_num::<i32>()),
        i32::from(position.ry)
            .wrapping_mul(256)
            .wrapping_add(position.sub_y.to_num::<i32>()),
    ]
}

/// Store a Location's world XY. On the map the cell is the Location's cell
/// (`0x0041BEA0`: each axis divided by 256 toward zero) and the sub-cell the
/// remainder. Off it the unsigned cell clamps to 0 or 65535 and the sub-cell
/// keeps the rest, so a negative or 16-bit-aliased coordinate from -32768 to
/// 65535 * 256 + 32767 still reads back unchanged through
/// [`position_world_xy`]; its native cell word comes from that full XY.
/// Beyond that range the sub-cell overflows (a debug panic, a wrap in
/// release), where native keeps any `i32`; no mover on a map reaches it.
pub(crate) fn set_position_world_xy(position: &mut Position, xy: [i32; 2]) {
    let [x, y] = xy;
    let cell_x = crate::util::lepton::lepton_to_cell(x).clamp(0, i32::from(u16::MAX));
    let cell_y = crate::util::lepton::lepton_to_cell(y).clamp(0, i32::from(u16::MAX));
    position.rx = cell_x as u16;
    position.ry = cell_y as u16;
    position.sub_x = crate::util::fixed_math::SimFixed::from_num(x.wrapping_sub(cell_x * 256));
    position.sub_y = crate::util::fixed_math::SimFixed::from_num(y.wrapping_sub(cell_y * 256));
}

/// `ObjectClass::SetLocation @ 0x005F6940`: the Location becomes `coord`.
pub(crate) fn put_location(position: &mut Position, coord: DriveCoord) {
    set_position_world_xy(position, [coord.x, coord.y]);
    position.exact_z_leptons = Some(coord.z);
}

/// `FootClass::SetLocation` (vt+0x1B4 = `0x004DB810`): the Location becomes
/// `coord` ([`put_location`]) whether or not it changed, then, only when it
/// changed (`0x004DB819..0x004DB83D`), an `OpenTopped=` transport's riders
/// take it (`0x004DB870..0x004DB88A` -> `0x007104F0`,
/// [`open_topped_riders_follow`]). Drive, Ship, Walk, Hover, Jumpjet, the
/// tube, Fly crash fall and Unit sinking move a Foot through it.
///
/// This is its unmarked arm; [`Simulation::foot_set_location_marked`] adds
/// the marked one (`0x004DB83F..0x004DB866`, Mark(UP),
/// `ObjectClass::SetLocation`, Mark(DOWN) while `+0x74` is set), which the
/// Chronosphere's warp-in, a hull its PostWarpValidation sank and every
/// Rocket Process move (`0x006622C0`) reach. Every
/// other native call a Rust caller ports runs it unmarked: Drive,
/// Hover and Walk Mark(UP) first on a cell change (`0x004B2071`,
/// `0x005148E3`, `0x0075BD7D`, `0x0075C11E`) and Drive and Hover clear
/// `+0x74` around it otherwise (`0x004B209F`, `0x005149F7`); Jumpjet clears
/// `+0x74` around it (`0x0054C189..0x0054C1A3`) or Mark(UP)s first
/// (`0x0054C820`, `0x0054CBE1`); a tube mover is out of the lists until its
/// exit's Mark(DOWN); the Fly crash fall Mark(UP)s first (`0x004CD766`);
/// and a sinking Unit was unmarked by its fatal receiver (`0x00737F7A`)
/// before Unit AI's call (`0x007364E3`).
/// The Jumpjet replay also sets the Location it Mark(UP)s from while marked,
/// which moves nothing. Natively `ObjectClass::Paradrop` calls it on the
/// object Unlimbo just marked, with the same coordinate (`0x005F5A3D`,
/// `0x005F5A50`); VERA's Reveal commits the drop coordinate instead, which
/// skips a Pick_Up/Place_Down pair that leaves the list order as it was, and
/// its two Recalcs. Trigger: every paradrop. Effect: none on the lists. Risk:
/// a new caller that sets a marked object's Location.
///
/// RESIDUAL: the change test reads [`position_world_coord`], whose Z is the
/// stored level when no exact Z is kept (a missile, a tube owner, or an
/// object a bridge DropIn set down). Only the rider copy depends on it; an
/// extra copy changes nothing, and a Z-only change it misses leaves riders
/// at the old Z.
///
/// [`open_topped_riders_follow`]: crate::sim::passenger::open_topped_riders_follow
pub(crate) fn foot_set_location(
    entities: &mut crate::sim::entity_store::EntityStore,
    id: u64,
    coord: DriveCoord,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    interner: &crate::sim::intern::StringInterner,
) {
    let Some(entity) = entities.get_mut(id) else {
        return;
    };
    let changed = position_world_coord(&entity.position) != coord;
    put_location(&mut entity.position, coord);
    if let (true, Some(rules)) = (changed, rules) {
        crate::sim::passenger::open_topped_riders_follow(entities, id, rules, interner);
    }
}

/// Read retained ObjectClass coordinates without resampling changed terrain.
/// Legacy positions without an exact Z use their stored signed level until a
/// real coordinate writer supplies raw leptons. That level omits the slope, a
/// Hover or Air height and a parachute's height, so a reader of an object's
/// Z asks [`object_location`] or [`object_get_coords`] instead.
pub(crate) fn position_world_coord(position: &Position) -> DriveCoord {
    let [x, y] = position_world_xy(position);
    DriveCoord {
        x,
        y,
        z: position.exact_z_leptons.unwrap_or_else(|| {
            i32::from(position.z as i8) * crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS
        }),
    }
}

/// ObjectClass GetCoords Z (virtual +0x48, Object+0xA4) of any object: the
/// one answer to "how high is this object". An exact coordinate a native
/// writer retained is already total world Z. Otherwise VERA still keeps the
/// object's height in parts: the live sloped ground at its XY (Map578080
/// through `ground_height_leptons`), the OnBridge deck, and its one active
/// altitude source ([`object_altitude_leptons`]). Without terrain the stored
/// signed level stands in for the ground. InRange's low-flying snap is its
/// caller's rule, not part of this read.
pub(crate) fn object_world_z_leptons(
    entity: &crate::sim::game_entity::GameEntity,
    terrain: Option<&ResolvedTerrainGrid>,
) -> i32 {
    entity.position.exact_z_leptons.unwrap_or_else(|| {
        object_ground_z_leptons(entity, terrain).wrapping_add(object_altitude_leptons(entity))
    })
}

/// The ground an object without an exact coordinate stands on: the live
/// sloped ground at its XY plus the OnBridge deck, or without terrain its
/// stored signed level (which already includes a deck).
pub(crate) fn object_ground_z_leptons(
    entity: &crate::sim::game_entity::GameEntity,
    terrain: Option<&ResolvedTerrainGrid>,
) -> i32 {
    let [x, y] = position_world_xy(&entity.position);
    terrain
        .and_then(|terrain| terrain.cell(entity.position.rx, entity.position.ry))
        .and_then(|cell| ground_height_leptons(cell.level, cell.slope_type, x, y).ok())
        .map(|ground| {
            ground.wrapping_add(if entity.on_bridge {
                BRIDGE_DECK_HEIGHT_LEPTONS
            } else {
                0
            })
        })
        .unwrap_or_else(|| i32::from(entity.position.z as i8) * GROUND_LEVEL_HEIGHT_LEPTONS)
}

/// Height above the ground of an object without an exact coordinate: the
/// altitude of an Air-layer locomotor or of an active Hover (which floats on
/// the Ground layer). Any other Ground-layer locomotor never lifts, which
/// keeps a landed or docked aircraft on the floor whatever its stale
/// altitude. A falling object and a launched missile always have an exact
/// coordinate (the missile from its Unlimbo, `spawn_manager::launch_coordinate`,
/// which its flight moves), so a Rocket locomotor's piggyback altitude is
/// never read here.
pub(crate) fn object_altitude_leptons(entity: &crate::sim::game_entity::GameEntity) -> i32 {
    entity
        .locomotor
        .as_ref()
        .filter(|locomotor| {
            use crate::rules::locomotor_type::LocomotorKind;
            (locomotor.layer == crate::sim::movement::locomotor::MovementLayer::Air
                && locomotor.kind != LocomotorKind::Rocket)
                || locomotor.active_kind() == LocomotorKind::Hover
        })
        .map_or(0, |locomotor| locomotor.altitude.to_num::<i32>())
}

/// Building render-coordinate459EF0 and GetYSort449410's type adjustment.
/// One459EF0 port for ART, damage fires, destruction, muzzle coordinates,
/// Temporal sparkles, display registration and presentation. YSort stays
/// separate from the coordinate; neither uses foundation-center coords.
pub(crate) fn building_render_order_parts(
    mut location: DriveCoord,
    turret_anim_is_voxel: bool,
    gate: bool,
) -> (DriveCoord, i32) {
    location.x = location.x.wrapping_sub(128);
    location.y = location.y.wrapping_sub(128);
    (
        location,
        i32::from(turret_anim_is_voxel) * 32 - i32::from(gate) * 16,
    )
}

/// `BuildingClass::GetCoords @ 0x00447AC0`'s XY: a building's Location plus
/// `(dimension - 1) * 128` leptons along each axis of its foundation. The
/// Location is its north-west cell's centre, so the result is the
/// foundation's geometric centre.
fn foundation_center_xy(location: [i32; 2], foundation: &str) -> [i32; 2] {
    let (width, height) = crate::rules::foundation::foundation_dimensions(foundation);
    [
        location[0].wrapping_add(i32::from(width).wrapping_mul(128).wrapping_sub(128)),
        location[1].wrapping_add(i32::from(height).wrapping_mul(128).wrapping_sub(128)),
    ]
}

/// The XY of an object's GetCoords (virtual +0x48). A Unit, Infantry or
/// Aircraft returns its Location (`ObjectClass::GetCoords @ 0x005F65A0`). A
/// building returns its foundation centre ([`foundation_center_xy`]), read
/// from the foundation its type stamped on it at construction.
pub(crate) fn object_center_xy(entity: &crate::sim::game_entity::GameEntity) -> [i32; 2] {
    let location = position_world_xy(&entity.position);
    if entity.category == crate::map::entities::EntityCategory::Structure {
        foundation_center_xy(location, &entity.foundation)
    } else {
        location
    }
}

/// An object's Location (`ObjectClass+0x9C..+0xA4`) at its world Z
/// ([`object_world_z_leptons`]).
pub(crate) fn object_location(
    entity: &crate::sim::game_entity::GameEntity,
    terrain: Option<&ResolvedTerrainGrid>,
) -> DriveCoord {
    let [x, y] = position_world_xy(&entity.position);
    DriveCoord {
        x,
        y,
        z: object_world_z_leptons(entity, terrain),
    }
}

/// An object's GetCoords (virtual +0x48) in full: [`object_center_xy`] at the
/// object's world Z ([`object_world_z_leptons`]). A building keeps its
/// Location's Z. That Z is the floor at its Location: its Unlimbo coordinate
/// passes through `BuildingTypeClass` virtual +0x6C (`0x00464A70`), which
/// replaces the Z with `0x00578080`'s ground height at that XY.
///
/// This is not a building's +0x4C approach coordinate for docks and bunkers.
pub(crate) fn object_get_coords(
    entity: &crate::sim::game_entity::GameEntity,
    terrain: Option<&ResolvedTerrainGrid>,
) -> DriveCoord {
    let [x, y] = object_center_xy(entity);
    DriveCoord {
        x,
        y,
        z: object_world_z_leptons(entity, terrain),
    }
}

/// Dispatch a retained Target's virtual+48 without changing its identity.
/// Object5F65A0/Building447AC0 and Cell486840 remain the coordinate owners;
/// navigation+4C and Cell+58 (deck target coordinates) are different queries.
/// Rescue4DE04E/4DE073 and AreaGuard4D6F00 pass this XYZ to their scanner.
pub(crate) fn target_get_coords(
    target: crate::sim::combat::TargetKind,
    entities: &crate::sim::entity_store::EntityStore,
    cells: Option<&NativeCellQuery<'_>>,
) -> Option<DriveCoord> {
    match target {
        crate::sim::combat::TargetKind::Cell(rx, ry) => {
            Some(super::navcom::target_cell_coord(rx, ry, cells))
        }
        crate::sim::combat::TargetKind::Entity(id) => {
            let entity = entities.get(id)?;
            Some(object_get_coords(
                entity,
                cells.map(NativeCellQuery::terrain),
            ))
        }
    }
}

/// The level and slope `CellClass::GetGroundHeight @ 0x00578080` reads at
/// full world XY. With resolved terrain that is Map[coord] (`0x00565730`),
/// which forms the wrapping fixed-stride index before narrowing fallback
/// coordinates; keep the common lookup owner. A PathGrid supplies the same
/// fields only for callers without resolved terrain.
fn ground_fields_at(
    world_xy: [i32; 2],
    terrain: Option<&ResolvedTerrainGrid>,
    path_grid: Option<&PathGrid>,
) -> Option<(u8, u8)> {
    if let Some(terrain) = terrain {
        let cells = NativeCellQuery::canonical(terrain);
        return Some(cells.ground_fields(cells.lookup_world(world_xy[0], world_xy[1])));
    }
    let rx = (world_xy[0] / 256) as i16;
    let ry = (world_xy[1] / 256) as i16;
    let cell = path_grid?.cell(rx as u16, ry as u16)?;
    Some((cell.ground_level, cell.slope_type))
}

fn supported_ground(level: u8, slope: u8, world_xy: [i32; 2]) -> Option<i32> {
    match ground_height_leptons(level, slope, world_xy[0], world_xy[1]) {
        Ok(ground) => Some(ground),
        Err(_) => {
            log::warn!(
                "ground pose at {world_xy:?} has unsupported slope {slope}; retaining raw Z"
            );
            None
        }
    }
}

/// Sample the live surface at full world XY: the ground plus the deck when
/// `on_bridge`, which is the Z SetHeight(0) writes. Missing headless terrain
/// leaves the caller's existing coordinate authoritative.
pub(crate) fn ground_surface_z_at(
    world_xy: [i32; 2],
    on_bridge: bool,
    terrain: Option<&ResolvedTerrainGrid>,
    path_grid: Option<&PathGrid>,
) -> Option<i32> {
    let (level, slope) = ground_fields_at(world_xy, terrain, path_grid)?;
    Some(z_at_height(
        supported_ground(level, slope, world_xy)?,
        0,
        on_bridge,
    ))
}

/// `ObjectClass::SetHeight @ 0x005F5FA0`'s Z for a sampled ground: the
/// requested height gains the deck (`[0x00AC13BC]`) when OnBridge
/// (`0x005F5FAB..0x005F5FB5`), then the ground under the Location
/// (`0x00578080`) is added (`0x005F5FFB`, `0x005F6044`).
pub(crate) fn z_at_height(ground: i32, height: i32, on_bridge: bool) -> i32 {
    ground.wrapping_add(height).wrapping_add(if on_bridge {
        BRIDGE_DECK_HEIGHT_LEPTONS
    } else {
        0
    })
}

/// `ObjectClass::GetHeight @ 0x005F5F40` for a sampled ground: the Z less the
/// ground and, OnBridge, the deck. The inverse of [`z_at_height`].
pub(crate) fn height_at_z(z: i32, ground: i32, on_bridge: bool) -> i32 {
    z.wrapping_sub(ground).wrapping_sub(if on_bridge {
        BRIDGE_DECK_HEIGHT_LEPTONS
    } else {
        0
    })
}

/// `ObjectClass::SetHeight @ 0x005F5FA0` (every object vtable's `+0x1CC`)
/// writing the Location's Z: [`z_at_height`] over the ground under its XY.
///
/// Callers that hold the object through a borrow call this where native runs
/// SetHeight unmarked: between Mark(REMOVE) and Mark(PUT), or with `+0x74`
/// cleared around it, as Drive (`0x004B1A88`, `0x004B209F`) and Walk
/// (`0x0075C1FB`) do. [`Simulation::set_object_height`] is the entry point for
/// an object id.
///
/// Without resolved terrain, the headless movement hosts that pass their
/// PathGrid take the cell's level and slope from it. Otherwise (mapless
/// fixtures) every lookup would be the Dummy cell, whose constructor ground is
/// flat level 0. An unsupported slope leaves Z unchanged.
pub(crate) fn set_height(
    position: &mut Position,
    on_bridge: bool,
    height: i32,
    terrain: Option<&ResolvedTerrainGrid>,
    path_grid: Option<&PathGrid>,
) {
    let world_xy = position_world_xy(position);
    let (level, slope) = ground_fields_at(world_xy, terrain, path_grid).unwrap_or((0, 0));
    if let Some(ground) = supported_ground(level, slope, world_xy) {
        position.exact_z_leptons = Some(z_at_height(ground, height, on_bridge));
    }
}

impl Simulation {
    /// `FootClass::SetLocation` in full: a marked object (`+0x74`) leaves its
    /// cell through Mark(UP) and rejoins it through Mark(DOWN) around the
    /// write (`0x004DB83F..0x004DB866`); either way [`foot_set_location`]
    /// does the write and the OpenTopped riders.
    pub(crate) fn foot_set_location_marked(
        &mut self,
        id: u64,
        coord: DriveCoord,
        rules: Option<&crate::rules::ruleset::RuleSet>,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) {
        let marked = self
            .substrate
            .entities
            .get(id)
            .is_some_and(|entity| entity.lifecycle.cell_marked);
        if marked {
            self.foot_mark_remove(id, rules, registry);
        }
        foot_set_location(
            &mut self.substrate.entities,
            id,
            coord,
            rules,
            &self.interner,
        );
        if marked {
            self.foot_mark_put(id, rules, registry);
        }
    }

    /// `ObjectClass::SetHeight @ 0x005F5FA0` for an object id. A marked object
    /// (`+0x74`) leaves its cell through its own Mark (vt+0x124, `0x005F5FC8`)
    /// before the Z write and marks again after it (`0x005F6009`), so a Foot
    /// is prepended to its cell's list again and the cell recalculates. That
    /// happens at a crash impact (Fly `0x004CD7BF`, Jumpjet notice
    /// `0x007461B9`, Infantry notice `0x00522B7D`/`0x00522B8B`) and a fall's
    /// landing (`0x005F3F7A`). An unmarked object takes the Z write alone
    /// ([`Self::set_object_height_unmarked`]).
    pub(crate) fn set_object_height(
        &mut self,
        id: u64,
        height: i32,
        rules: Option<&crate::rules::ruleset::RuleSet>,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) {
        let context = crate::sim::world::UninitContext::new(rules, registry);
        let marked = self
            .substrate
            .entities
            .get(id)
            .is_some_and(|entity| entity.lifecycle.cell_marked);
        if marked {
            self.unmark_entity_remove(id, context);
        }
        self.set_object_height_unmarked(id, height);
        if marked {
            self.mark_entity_put(id, context);
        }
    }

    /// SetHeight's unmarked arm (`0x005F6017..`): [`set_height`] for an
    /// object id. Hover's altitude step (`0x00513E74..0x00513E8C`) clears
    /// `+0x74` around its SetHeight, so it always takes this arm.
    ///
    /// It samples terrain only, as GetHeight ([`current_fly_height`]) does, so
    /// `SetHeight(GetHeight() + n)` round-trips without a map too. It also
    /// mirrors the requested height ([`mirror_height`]).
    ///
    /// [`current_fly_height`]: super::air_movement::current_fly_height
    pub(crate) fn set_object_height_unmarked(&mut self, id: u64, height: i32) {
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return;
        };
        set_height(
            &mut entity.position,
            entity.on_bridge,
            height,
            self.resolved_terrain.as_ref(),
            None,
        );
        mirror_height(entity, height);
    }
}

/// Keep `LocomotorState.altitude` equal to the object's GetHeight, for the
/// readers that still take it as the object's height (#692).
pub(crate) fn mirror_height(entity: &mut crate::sim::game_entity::GameEntity, height: i32) {
    if let Some(locomotor) = entity.locomotor.as_mut() {
        locomotor.altitude = crate::util::fixed_math::SimFixed::saturating_from_num(height);
    }
}
