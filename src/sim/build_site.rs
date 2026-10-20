//! Whether a building may stand on a cell, and on its whole foundation.
//!
//! The one owner of `CellClass::Is_Clear_To_Build @ 0x0047C620` and the
//! foundation walk over it, `BuildingTypeClass::CanPlaceAt @ 0x00464AC0`
//! (vtable `+0xA8`) → `TechnoTypeClass::CanPlaceAt @ 0x00716150`, and of the
//! computer's clearing of a site before it places there
//! (`BuildingTypeClass::Flush_For_Placement @ 0x0045EE70`). Its callers
//! here: player placement (the cursor's per-cell test `0x0047EE93` and the
//! wall fill scan `0x0058886E`), MCV Deploy (`0x007394D2`), TryToDeploy
//! (`0x00738E08`, `0x00739333`) and the building/terrain answers of the live
//! cell-entry query (`BuildingTypeClass::CanPlaceAt`, TerrainType
//! `0x0071C554`).
//!
//! The shared scenario-init / forced-placement bracket `0xA8E7AC` admits
//! immediately when nonzero (`0x0047C632`); ordinary gameplay keeps it clear.
//! The byte
//! at `0xA8E9A0` is set once a game runs, so the object tests are live; the
//! editor byte `0xA8ED6B` is clear, so the cell must lie in the playfield
//! (`Is_Cell_In_Playfield(cell, 1)`, `0x0047C878`) and an overlay the type
//! cannot share rejects it (`0x0047C9A7`).

use crate::map::cell_index::NativeCellIdentity;
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::{NativeCellQuery, ResolvedTerrainGrid};
use crate::rules::locomotor_type::SpeedType;
use crate::rules::object_type::ObjectType;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::{CellObjectMember, RawCellKey};
use crate::sim::world::{FrameEffects, Simulation};

/// CellClass+0x140 bit tested beside `0x400` by both the overlay and the
/// terrain arms: a high bridge over the cell.
const FLAG_HIGH_BRIDGE: u32 = 0x100;
/// CellClass+0x140 bit `0x400`, the bridge body/ramp marker.
const FLAG_BRIDGE_BODY: u32 = 0x400;
/// CellClass+0x124 bits `0x3F`: infantry sub-cells and a vehicle.
const OCCUPATION_INFANTRY_OR_VEHICLE: u8 = 0x3F;
/// Overlay indices the wall arms compare (`0x0047C8B1`, `0x0047C903`):
/// GASAND 0, GAWALL 2 and NAWALL 0x1A in the stock `[OverlayTypes]` list.
const OVERLAY_GASAND: u8 = 0;
const OVERLAY_GAWALL: u8 = 2;
const OVERLAY_NAWALL: u8 = 0x1A;
/// The laser fence overlay a `LaserFence=` type may stand on (`0x0047C961`).
const OVERLAY_LASER_FENCE: u8 = 0x7E;
/// The overlay data a wall reaches once damaged (`0x0047C8D4`, `0x0047C926`):
/// the high nibble is the damage stage.
const WALL_DAMAGED_DATA: u8 = 0x10;

/// The SpeedType a building type carries into Is_Clear_To_Build.
/// `BuildingTypeClass::ReadINI` overwrites TechnoType+0x67C with
/// `WaterBound ? Float : -1` after reading (`0x0045FF85..0x0045FFA9`), so a
/// building's own `SpeedType=` line never reaches placement.
pub(crate) fn building_speed_type(ty: &ObjectType) -> Option<SpeedType> {
    ty.water_bound.then_some(SpeedType::Float)
}

/// `BuildingTypeClass::CanPlaceAt @ 0x00464AC0`: a `PlaceAnywhere=` type
/// stands anywhere (`0x00464AC0..0x00464ACC`); otherwise
/// `TechnoTypeClass::CanPlaceAt @ 0x00716150` walks the foundation list from
/// `origin`. `origin == (0, 0)` (`Cell::Empty`, `0x0071615F`) is refused. Every
/// foundation cell is looked up through `MapClass::GetCellAt` (which stamps the
/// shared dummy off the map) and tested — the walk does not stop at the first
/// refusal. The foundation must be clear everywhere, except for a `ToTile=`
/// type, which needs one clear cell (`0x00716260..0x00716280`).
/// Simulation callers use the canonical lookup (`None`); presentation carries
/// its isolated query through this same walk and every nested Cell decision.
pub(crate) fn can_place_building_at(
    sim: &Simulation,
    rules: &RuleSet,
    registry: Option<&OverlayTypeRegistry>,
    ty: &ObjectType,
    origin: (i16, i16),
    house: Option<InternedId>,
    native_cells: Option<&NativeCellQuery<'_>>,
) -> bool {
    if ty.place_anywhere {
        return true;
    }
    if origin == (0, 0) {
        return false;
    }
    let Some(terrain) = sim.resolved_terrain.as_ref() else {
        return false;
    };
    let canonical = NativeCellQuery::canonical(terrain);
    let cells = native_cells.unwrap_or(&canonical);
    let speed = building_speed_type(ty);
    let mut refused = false;
    let mut admitted = false;
    for (dx, dy) in crate::rules::foundation::foundation_cell_offsets(&ty.foundation) {
        let cell = cells.lookup((origin.0.wrapping_add(dx), origin.1.wrapping_add(dy)));
        if is_clear_to_build(
            sim,
            rules,
            registry,
            cell,
            speed,
            Some(ty),
            house,
            Some(cells),
        ) {
            admitted = true;
        } else {
            refused = true;
        }
    }
    if resolved_to_tile(terrain, ty) {
        admitted
    } else {
        !refused
    }
}

/// `CellClass::Is_Clear_To_Build @ 0x0047C620` for `cell`, `speed` (`None` is
/// -1), the building type (`None` for a TerrainType's test) and the placing
/// house (`None` is null; it matches an unowned wall).
/// A supplied query must be the context that resolved `cell`, so retained
/// Dummy coordinates/ground fields come from that identity without relookup.
pub(crate) fn is_clear_to_build(
    sim: &Simulation,
    rules: &RuleSet,
    registry: Option<&OverlayTypeRegistry>,
    cell: NativeCellIdentity,
    speed: Option<SpeedType>,
    ty: Option<&ObjectType>,
    house: Option<InternedId>,
    native_cells: Option<&NativeCellQuery<'_>>,
) -> bool {
    // Original47C632 returns true while the shared A8E7AC bracket is raised.
    // Building440580 still enters449440; the bypass belongs to this cell
    // decision, not to a caller-specific skipping of Building's admission.
    if sim.object_placement_scope_active() {
        return true;
    }
    let Some(terrain) = sim.resolved_terrain.as_ref() else {
        return false;
    };
    let canonical = NativeCellQuery::canonical(terrain);
    let cells = native_cells.unwrap_or(&canonical);
    if let Some(ty) = ty
        && !objects_admit(sim, rules, terrain, cell, ty, house)
    {
        return false;
    }
    if !crate::sim::cell_rect::retained_cell_is_in_playfield(cell, sim.playfield_bounds(), cells) {
        return false;
    }
    // Deliberate developer override; object occupancy and bounds above still apply.
    #[cfg(feature = "debug-tools")]
    if ty.is_some() && house.is_some_and(|house| sim.debug_flags_for(house).build_anywhere) {
        return true;
    }
    let flags = cells.flags(cell);
    let slope = cells.ground_fields(cell).1;
    if let NativeCellIdentity::Real(index) = cell {
        let (rx, ry) = (terrain.cells()[index].rx, terrain.cells()[index].ry);
        if let Some(overlay) = sim.overlay_grid.as_ref().map(|grid| *grid.cell(rx, ry))
            && let Some(overlay_id) = overlay.overlay_id
        {
            return overlay_admits(
                rules,
                registry,
                overlay_id,
                overlay.overlay_data,
                overlay.wall_owner,
                ty,
                house,
                flags,
                slope,
            );
        }
    }
    // 0x0047C9CD: the ground itself. A LandType section missing from every
    // rules layer leaves its row as the process started it, 0.0 speeds and
    // not Buildable (`RulesClass::ReadSpeedTypeLandTypeTable @ 0x00674000`
    // skips it); a missing key reads 1.0 or false.
    let land = cells.land_type(cell) as u8;
    let semantics = rules.terrain_rules.semantics_for_land_type(land);
    match speed {
        None => {
            if flags & (FLAG_HIGH_BRIDGE | FLAG_BRIDGE_BODY) != 0 || slope != 0 {
                return false;
            }
            if ty.is_some_and(|ty| ty.naval) {
                // `0x0047CA0F..0x0047CA25`: a Naval type needs a WaterSet tile.
                terrain.native_cell_is_water_set_tile(cell)
            } else {
                semantics.is_some_and(|land| land.buildable)
            }
        }
        // The LandType's float speed for this SpeedType is not 0.0
        // (`0x0047CA4D..0x0047CA6A`).
        Some(speed) => semantics
            .is_some_and(|land| land.cost_for_speed_type(speed).is_none_or(|cost| cost != 0)),
    }
}

/// What `BuildingTypeClass::Flush_For_Placement @ 0x0045EE70` answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Flush {
    /// 0: nothing stands in the way, or the origin is (0, 0).
    Clear,
    /// 1: allied units stand in the way and were told to leave, or are
    /// leaving already.
    Scattered,
    /// 2: something that will not leave stands in the way.
    Blocked,
}

/// `BuildingTypeClass::Flush_For_Placement @ 0x0045EE70` for `ty` at `origin`
/// and the placing `house`: over the foundation cells on the map
/// (`MapClass::In_Bounds`), an overlay blocks (a `WallTower=` type may stand
/// on GAWALL, `0x0045EF11..0x0045EF26`), and so does the cell's first object
/// (`+0xE4`) when it is a Terrain object, a building (whose
/// `PowersUpBuilding=` arm `0x00452670` is dormant) or a unit whose house is
/// not allied to `house`. Any other unit counts as told to leave; unless it
/// is heading elsewhere already (a NavCom other than its own cell,
/// `0x0045EF9D..0x0045EFDF`), its cell is scattered
/// (`CellClass::Scatter_Objects @ 0x00481670` with a null source, forced, on
/// the ground list). The scan stops at the first blocker. A receiver error
/// (malformed live state) is logged and leaves that cell's occupants in place.
pub(crate) fn flush_for_placement(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: Option<&OverlayTypeRegistry>,
    ty: &ObjectType,
    origin: (i16, i16),
    house: InternedId,
    effects: FrameEffects<'_>,
) -> Flush {
    if origin == (0, 0) {
        return Flush::Clear;
    }
    let wall_tower = rules
        .general
        .building_types
        .wall_tower
        .as_deref()
        .is_some_and(|name| name.eq_ignore_ascii_case(&ty.id));
    let mut scattered = false;
    for (dx, dy) in crate::rules::foundation::foundation_cell_offsets(&ty.foundation) {
        let cell = (origin.0.wrapping_add(dx), origin.1.wrapping_add(dy));
        if !sim.map_cell_in_bounds(cell) {
            continue;
        }
        let (Ok(rx), Ok(ry)) = (u16::try_from(cell.0), u16::try_from(cell.1)) else {
            continue;
        };
        let overlay = sim
            .overlay_grid
            .as_ref()
            .and_then(|grid| grid.cell(rx, ry).overlay_id);
        if overlay.is_some_and(|overlay| !(wall_tower && overlay == OVERLAY_GAWALL)) {
            return Flush::Blocked;
        }
        let Some(first) = sim.cell_objects((rx, ry), MovementLayer::Ground).next() else {
            continue;
        };
        let CellObjectMember::Entity(id) = first else {
            return Flush::Blocked;
        };
        let Some(object) = sim.substrate.entities.get(id) else {
            continue;
        };
        let allied = crate::map::houses::is_allied_with(
            sim.house_alliances.as_map(),
            sim.interner.resolve(object.owner()),
            sim.interner.resolve(house),
        );
        if object.category() == EntityCategory::Structure || !allied {
            return Flush::Blocked;
        }
        scattered = true;
        let own_cell =
            crate::sim::components::NavTargetRef::cell(object.position.rx(), object.position.ry());
        if object
            .navigation
            .nav_com()
            .is_some_and(|nav_com| nav_com != own_cell)
        {
            continue;
        }
        if let Err(cause) = sim.scatter_cell_contacts(cell, false, true, rules, registry, effects) {
            log::debug!("site cell {cell:?} did not scatter: {cause}");
        }
    }
    if scattered {
        Flush::Scattered
    } else {
        Flush::Clear
    }
}

/// The type-specific object tests before the playfield test
/// (`0x0047C638..0x0047C866`).
fn objects_admit(
    sim: &Simulation,
    rules: &RuleSet,
    terrain: &ResolvedTerrainGrid,
    cell: NativeCellIdentity,
    ty: &ObjectType,
    house: Option<InternedId>,
) -> bool {
    let members: Vec<CellObjectMember> = match cell {
        NativeCellIdentity::Real(index) => {
            let cell = &terrain.cells()[index];
            sim.cell_objects((cell.rx, cell.ry), MovementLayer::Ground)
                .collect()
        }
        // The shared dummy carries no object list.
        NativeCellIdentity::Dummy => Vec::new(),
    };
    let category = |member: &CellObjectMember| match *member {
        CellObjectMember::Entity(id) => sim.substrate.entities.get(id).map(|e| e.category()),
        CellObjectMember::Terrain(_) => None,
    };
    let is_terrain = |member: &CellObjectMember| matches!(member, CellObjectMember::Terrain(_));
    let occupation_clear = || {
        let key = RawCellKey::from_native(terrain, cell);
        sim.substrate
            .raw_cell_occupation
            .bits_at(key, MovementLayer::Ground)
            & OCCUPATION_INFANTRY_OR_VEHICLE
            == 0
    };
    if ty.laser_fence {
        // 0x0047C640..0x0047C69E: no Building, then no Terrain, in the list.
        !members
            .iter()
            .any(|member| category(member) == Some(EntityCategory::Structure) || is_terrain(member))
    } else if ty.laser_fence_post || ty.gate {
        // 0x0047C7C8..0x0047C851: the first Aircraft in the list, else the
        // nearest Techno, else the first Terrain. Only a LaserFence building
        // of the placing house may share the cell.
        let aircraft = members
            .iter()
            .find(|member| category(member) == Some(EntityCategory::Aircraft))
            .copied();
        let blocker = aircraft
            .or_else(|| {
                let NativeCellIdentity::Real(index) = cell else {
                    return None;
                };
                let cell = &terrain.cells()[index];
                sim.nearest_cell_object((cell.rx, cell.ry), MovementLayer::Ground, None)
                    .map(CellObjectMember::Entity)
            })
            .or_else(|| members.iter().find(|member| is_terrain(member)).copied());
        match blocker {
            None => occupation_clear(),
            Some(CellObjectMember::Entity(id)) => {
                sim.substrate.entities.get(id).is_some_and(|e| {
                    e.category() == EntityCategory::Structure
                        && sim
                            .object_type(e.type_ref(), rules)
                            .is_some_and(|blocker| blocker.laser_fence)
                        && Some(e.owner()) == house
                }) && occupation_clear()
            }
            Some(CellObjectMember::Terrain(_)) => false,
        }
    } else if resolved_to_tile(terrain, ty) {
        // 0x0047C76B..0x0047C7C3: a valid current tile must be Morphable,
        // and no Building may stand in the list.
        // The ToTile name resolved through the resident tile registry this
        // query reads, so it answers.
        let tile = terrain.native_cell_tile_index(cell);
        terrain.tile_allows_morph_placement(tile) == Ok(true)
            && !members
                .iter()
                .any(|member| category(member) == Some(EntityCategory::Structure))
    } else {
        // 0x0047C701..0x0047C866: no Aircraft, no Techno at all (Find_Nearest
        // answers one whenever the list holds any), no Terrain, and neither
        // infantry nor a vehicle holds the cell's occupation bits.
        members.is_empty() && occupation_clear()
    }
}

/// The overlay arm (`0x0047C88D..0x0047C9C7`). `wall_owner` is the House that
/// Cell+0x50 names, if any.
fn overlay_admits(
    rules: &RuleSet,
    registry: Option<&OverlayTypeRegistry>,
    overlay_id: u8,
    overlay_data: u8,
    wall_owner: Option<InternedId>,
    ty: Option<&ObjectType>,
    house: Option<InternedId>,
    flags: u32,
    slope: u8,
) -> bool {
    let Some(ty) = ty else {
        return false;
    };
    // BuildingType+0xE54 ToOverlay, compared by OverlayType index.
    let own_damaged_wall = || {
        overlay_data >= WALL_DAMAGED_DATA
            && ty
                .to_overlay
                .as_deref()
                .and_then(|name| registry?.id_for_name(name))
                == Some(overlay_id)
    };
    let gates = &rules.general.building_types;
    if matches!(overlay_id, OVERLAY_GASAND | OVERLAY_GAWALL)
        && (own_damaged_wall() || gates.stands_on_gdi_wall(&ty.id))
        && wall_owner == house
    {
        return true;
    }
    if overlay_id == OVERLAY_NAWALL
        && (own_damaged_wall() || gates.stands_on_nod_wall(&ty.id))
        && wall_owner == house
    {
        return true;
    }
    // A LaserFence type on the fence overlay or on ore
    // (`OverlayToTiberiumIndex @ 0x005FDD20` != -1).
    if ty.laser_fence
        && (overlay_id == OVERLAY_LASER_FENCE
            || registry.is_some_and(|registry| {
                registry
                    .tiberium_type_for_overlay(&rules.tiberium_types, overlay_id)
                    .is_some()
            }))
    {
        return flags & (FLAG_HIGH_BRIDGE | FLAG_BRIDGE_BODY) == 0 && slope == 0;
    }
    false
}

/// BuildingType+0xE58: `ToTile=` resolves (`0x00465CC0` → `0x00544CE0`) only
/// when the theater registers that tile name. A terrain without a resident
/// tile registry registers none.
pub(crate) fn resolved_to_tile(terrain: &ResolvedTerrainGrid, ty: &ObjectType) -> bool {
    ty.to_tile.as_deref().is_some_and(|name| {
        terrain
            .resolve_registered_tile_name(name)
            .ok()
            .flatten()
            .is_some()
    })
}

#[cfg(test)]
#[path = "build_site_tests.rs"]
mod tests;
