//! Narrow CellClass geometry and content-query kernels shared by simulation callers.
//!
//! These helpers intentionally keep floor, deck, coordinate lookup, and object-list
//! selection separate: YR composes them at their individual call sites.

use crate::map::cell_index::NativeCellIdentity;
use crate::map::resolved_terrain::NativeCellQuery;

use crate::map::resolved_terrain::ResolvedTerrainGrid;
#[cfg(any(test, feature = "test-support"))]
use crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS;
use crate::util::lepton::{GROUND_LEVEL_HEIGHT_LEPTONS, ground_height_leptons};

pub const LEPTONS_PER_CELL: i32 = crate::util::lepton::LEPTONS_PER_CELL_I32;
pub const CELL_CENTER_LEPTONS: i32 = crate::util::lepton::CELL_CENTER_LEPTON_I32;
pub const INFANTRY_OCCUPATION_VEHICLE_BIT: u8 = 0x20;
pub const INFANTRY_OCCUPATION_OBJECT_BIT: u8 = 0x40;

const INFANTRY_SEQUENCE: [[u8; 4]; 5] = [
    [1, 2, 3, 4],
    [0, 2, 3, 4],
    [0, 1, 4, 3],
    [0, 1, 4, 2],
    [0, 2, 3, 1],
];
const INFANTRY_ALTERNATE_SEQUENCE: [[u8; 4]; 4] =
    [[1, 2, 3, 4], [2, 3, 4, 1], [3, 4, 1, 2], [4, 1, 2, 3]];

/// Native480510's overlay prefix returns here or continues through the
/// corresponding Building/Gate object-list arm. Direction is read only by
/// those continuations; the supported overlay-only queries need no direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WallConnectionStep {
    Return(bool),
    GdiBuildings,
    NodBuildings,
}

/// `CellClass::IsWallConnectableInDirection @ 0x00480510..0x00480622`.
/// Source: original caller/body reads in the authored-fence comparison
/// (`tools/spatial_oracle/anytown_damage/authored_fence.json`). Stored and
/// requested identities retain native signed width; -1 means no overlay/type.
///
/// The literals are OverlayTypes declaration indices, not INI key numbers:
/// stock2/26/243 are GAWALL/NAWALL/GAFWLL; CAKRMW is240. This is not the
/// Wall=yes set (sandbags and fences do not pass the negative-type query).
/// The physical ordered registry readers, rather than key-minus-one, own IDs.
///
/// All negative-type LOS/Bullet/bounce/Spark callers and CAFNCB203 cleanup
/// resolve in this prefix. The distinct Building/Gate continuations stay
/// explicit rather than pretending this is a whole-function port.
pub(crate) fn wall_connection_step(stored: i32, requested: i32) -> WallConnectionStep {
    if stored != -1 && stored == requested {
        WallConnectionStep::Return(true)
    } else if requested == -1 {
        WallConnectionStep::Return(matches!(stored, 2 | 26 | 243))
    } else {
        match requested {
            0 | 2 => WallConnectionStep::GdiBuildings,
            26 => WallConnectionStep::NodBuildings,
            _ => WallConnectionStep::Return(false),
        }
    }
}

/// Cell4848B0: direction of a cliff entered from `previous_world_xy`.
/// The receiver was selected separately by the caller. Original table81CD4C,
/// ordered canonical lookups and asymmetric corner returns are consumed by
/// tools/spatial_oracle/bounce_cliff.{json,md}.
///
/// Ordinary TIRE motion crosses neighboring cells. Native indexes beyond this
/// nine-dword table for larger raw displacements; those unrelated image bytes
/// are outside the represented domain and produce no reflection here.
pub(crate) fn cliff_face_direction(
    cells: &NativeCellQuery<'_>,
    receiver: NativeCellIdentity,
    previous_world_xy: (i32, i32),
) -> Option<u8> {
    let previous = cells.lookup_world(previous_world_xy.0, previous_world_xy.1);
    let (x, y) = cells.coord(previous);
    // Read the receiver after that lookup: both identities may be the Dummy.
    let (receiver_x, receiver_y) = cells.coord(receiver);
    let dx = i32::from(receiver_x) - i32::from(x);
    let dy = i32::from(receiver_y) - i32::from(y);
    const DIRECTIONS: [i8; 9] = [7, 0, 1, 6, -1, 2, 5, 4, 3];
    let table_index = dx + 3 * dy + 4;
    let Some(&direction) = usize::try_from(table_index)
        .ok()
        .and_then(|index| DIRECTIONS.get(index))
    else {
        log::warn!("Cell4848B0 displacement ({dx},{dy}) exceeds the native neighbor table");
        return None;
    };
    let direction = u8::try_from(direction).ok()?;
    if dx == 0 || dy == 0 {
        return Some(direction);
    }

    let (side_x, side_y) = if dx == dy { (dx, dy) } else { (dy, dx) };
    let side_a = ((i32::from(x) + side_x) as i16, y);
    let side_b = (x, (i32::from(y) + side_y) as i16);
    let raised_side = |coord| {
        // Each test independently reselects the previous world Cell BEFORE
        // selecting its side. A missing side can stamp the retained Dummy.
        let previous = cells.lookup_world(previous_world_xy.0, previous_world_xy.1);
        let threshold = i32::from(cells.ground_fields(previous).0 as i8) + 2;
        let side = cells.lookup(coord);
        i32::from(cells.ground_fields(side).0 as i8) >= threshold
    };
    match (raised_side(side_a), raised_side(side_b)) {
        (true, false) => Some(direction.wrapping_sub(1) & 7),
        // Native484A62 increments without &7; NW can return8.
        (false, true) => Some(direction + 1),
        _ => Some(direction),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorldCoordinate {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellCoordinate {
    pub x: i32,
    pub y: i32,
}

/// Packed `CellStruct` words, sign-extended as CellStruct-to-Coord642740
/// reads them.
impl From<(i16, i16)> for CellCoordinate {
    fn from((x, y): (i16, i16)) -> Self {
        Self {
            x: i32::from(x),
            y: i32::from(y),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellQueryPoint {
    pub x: i32,
    pub y: i32,
}

impl crate::sim::world::Simulation {
    /// `MapClass::operator[](CoordStruct)` (`0x00565730`) for a world XY,
    /// which truncates toward zero and wraps by row: the real cell, or `None`
    /// for the shared dummy cell off the map, which holds no object. Without
    /// resolved terrain, the truncated cell.
    pub(crate) fn map_coord_cell(
        resolved_terrain: &Option<ResolvedTerrainGrid>,
        x: i32,
        y: i32,
    ) -> Option<(u16, u16)> {
        let (rx, ry) = match resolved_terrain.as_ref() {
            Some(terrain) => {
                let cells = NativeCellQuery::canonical(terrain);
                let cell = cells.lookup_world(x, y);
                let NativeCellIdentity::Real(_) = cell else {
                    return None;
                };
                cells.coord(cell)
            }
            None => (
                crate::util::lepton::lepton_to_cell_packed(x),
                crate::util::lepton::lepton_to_cell_packed(y),
            ),
        };
        Some((u16::try_from(rx).ok()?, u16::try_from(ry).ok()?))
    }

    /// `CellClass::Find_Nearest_Object 0x47C3D0` with the (0,0) sub-point over
    /// `layer`'s list of `cell`: ranked by each object's vt+0x48 coordinate (a
    /// building's centre), the first in list order on a tie.
    pub(crate) fn nearest_cell_object(
        &self,
        cell: (u16, u16),
        layer: crate::sim::movement::locomotor::MovementLayer,
        exclude: Option<u64>,
    ) -> Option<u64> {
        self.nearest_cell_object_from(
            cell,
            crate::sim::cell_kernel::CellQueryPoint { x: 0, y: 0 },
            layer,
            exclude,
        )
    }

    /// [`Self::nearest_cell_object`] from the caller's sub-point `point`.
    pub(crate) fn nearest_cell_object_from(
        &self,
        cell: (u16, u16),
        point: crate::sim::cell_kernel::CellQueryPoint,
        layer: crate::sim::movement::locomotor::MovementLayer,
        exclude: Option<u64>,
    ) -> Option<u64> {
        crate::sim::cell_kernel::nearest_eligible_in_order(
            point,
            self.substrate
                .occupancy
                .get(cell.0, cell.1)
                .into_iter()
                .flat_map(|list| list.iter_layer(layer))
                .filter_map(|entry| self.substrate.entities.get(entry.entity_id))
                .filter(|entity| Some(entity.stable_id()) != exclude)
                .map(|entity| {
                    let [x, y] = crate::sim::movement::ground_pose::object_center_xy(entity);
                    (
                        entity.stable_id(),
                        true,
                        crate::sim::cell_kernel::CellQueryPoint { x, y },
                    )
                }),
        )
    }
}

/// Shared cell-center arithmetic, including CellStruct-to-Coord642740.
/// That caller sign-extends the packed cell words before entering here.
/// Other callers retain their supplied cell width and height.
pub fn cell_center(cell: CellCoordinate, floor_z: i32) -> WorldCoordinate {
    WorldCoordinate {
        x: cell
            .x
            .wrapping_mul(LEPTONS_PER_CELL)
            .wrapping_add(CELL_CENTER_LEPTONS),
        y: cell
            .y
            .wrapping_mul(LEPTONS_PER_CELL)
            .wrapping_add(CELL_CENTER_LEPTONS),
        z: floor_z,
    }
}

/// `CellClass::GetCoords` 0x00486840 — a cell's own world point is its centre
/// (`MapCoord * 0x100 + 0x80` on both axes) with Z from
/// `CellClass::ComputeGroundHeightAtCoord` 0x0047B3A0. **No bridge-deck term:**
/// the deck offset belongs to whoever is standing on the deck, and
/// `TechnoClass::CanFireAt` adds it separately from the object's own OnBridge
/// byte at 0x006F7887.
pub(crate) fn native_cell_own_coords(
    cell: NativeCellIdentity,
    cells: &NativeCellQuery<'_>,
) -> Option<(i64, i64, i64)> {
    let (x, y) = cells.coord(cell);
    let (level, slope) = cells.ground_fields(cell);
    let coord = cell_own_coords(i32::from(x), i32::from(y), level, slope)?;
    Some((i64::from(coord.x), i64::from(coord.y), i64::from(coord.z)))
}

impl crate::sim::cell_rect::CellRef<'_> {
    /// Cell486840/480A30: the receiver's signed packed center and current
    /// ground height, without a bridge-deck rise. A retained Dummy is read now;
    /// later map lookups can change its coordinate without changing identity.
    pub fn center_coordinate(&self) -> Option<crate::sim::components::DriveCoord> {
        let (x, y, level, slope) = match self {
            Self::Real(cell) => (
                i32::from(cell.rx as i16),
                i32::from(cell.ry as i16),
                cell.level,
                cell.slope_type,
            ),
            Self::Dummy { cell } => {
                let value = cell.snapshot();
                (
                    value.coord.0,
                    value.coord.1,
                    value.level as u8,
                    value.slope_type,
                )
            }
        };
        cell_own_coords(x, y, level, slope)
    }
}

fn cell_own_coords(
    x: i32,
    y: i32,
    level: u8,
    slope: u8,
) -> Option<crate::sim::components::DriveCoord> {
    let point = cell_center(CellCoordinate { x, y }, 0);
    Some(crate::sim::components::DriveCoord {
        x: point.x,
        y: point.y,
        z: ground_height_leptons(level, slope, point.x, point.y).ok()?,
    })
}

/// The XY of [`native_cell_own_coords`]: the centre of the cell's current
/// packed coordinate, which a Dummy holds from its last stamp. It needs no
/// supported slope.
pub(crate) fn native_cell_own_xy(
    cell: NativeCellIdentity,
    cells: &NativeCellQuery<'_>,
) -> [i32; 2] {
    let (x, y) = cells.coord(cell);
    let point = cell_center(
        CellCoordinate {
            x: i32::from(x),
            y: i32::from(y),
        },
        0,
    );
    [point.x, point.y]
}

/// Native invalid-cell coordinates are process-global sentinels. At the Rust map
/// boundary, represent that unproven raw value as `None` rather than manufacture it.
#[cfg(any(test, feature = "test-support"))]
pub fn checked_cell_from_world(
    world_x: i32,
    world_y: i32,
    width: u16,
    height: u16,
) -> Option<(u16, u16)> {
    let x = crate::util::lepton::lepton_to_cell(world_x);
    let y = crate::util::lepton::lepton_to_cell(world_y);
    if x < 0 || y < 0 || x >= i32::from(width) || y >= i32::from(height) {
        return None;
    }
    Some((x as u16, y as u16))
}

/// Apply the high-bridge deck only when a caller has already selected that layer.
#[cfg(any(test, feature = "test-support"))]
pub fn selected_cell_surface_height(floor_z: i32, deck_selected: bool) -> i32 {
    if deck_selected {
        floor_z.wrapping_add(BRIDGE_DECK_HEIGHT_LEPTONS)
    } else {
        floor_z
    }
}

/// Alternate-list threshold: signed cell level plus four bridge levels,
/// independent of the floor kernel. `0x004ACA10`'s deck test
/// ([`crate::sim::movement::bump_crush::pick_infantry_sublocation`]).
pub fn selects_infantry_bridge_layer(has_high_bridge: bool, level: u8, input_z: i32) -> bool {
    let deck_z = i32::from(level as i8)
        .wrapping_add(4)
        .wrapping_mul(GROUND_LEVEL_HEIGHT_LEPTONS);
    has_high_bridge && input_z >= deck_z
}

/// Original 0x47C46B..0x47C48C (inside 0x0047C3D0): FILD both signed deltas,
/// square and sum them in x87, then Sqrt_Approx 0x4CAC40 and the truncating
/// Math_ftol 0x7C5F00. Low-byte extraction and list admission belong to callers.
pub(crate) fn native_xy_distance(dx: i32, dy: i32) -> i32 {
    crate::util::native_x87::sqrt_approx_length([dx, dy])
}

/// `FootClass::Find_Path` head 0x4D39F3..0x4D3A2B: the actor's `+0x48`
/// coordinate minus the target cell centre (x, y and z), each `FILD`ed, then
/// `(dz*dz + dy*dy) + dx*dx`, `Sqrt_Approx` and the truncating `Math_ftol`.
pub(crate) fn native_xyz_distance(dx: i32, dy: i32, dz: i32) -> i32 {
    crate::util::native_x87::sqrt_approx_length([dz, dy, dx])
}

/// Cell47C3D0 scoring after the caller supplies linked-list order, Techno
/// identity (+14 bit0) and excluded-object gates. The native identity byte
/// does not test health or limbo. Equal distances retain the first entry.
pub fn nearest_eligible_in_order<T>(
    input: CellQueryPoint,
    candidates: impl IntoIterator<Item = (T, bool, CellQueryPoint)>,
) -> Option<T> {
    let mut winner: Option<(i32, T)> = None;
    for (candidate, eligible, coordinate) in candidates {
        if !eligible {
            continue;
        }
        let candidate_x = coordinate.x & 0xff;
        let candidate_y = coordinate.y & 0xff;
        let input_x = input.x.wrapping_mul(7);
        let input_y = input.y.wrapping_mul(7);
        let dx = candidate_x.wrapping_sub(input_x);
        let dy = candidate_y.wrapping_sub(input_y);
        let distance = native_xy_distance(dx, dy);
        if winner.as_ref().is_none_or(|(best, _)| distance < *best) {
            winner = Some((distance, candidate));
        }
    }
    winner.map(|(_, candidate)| candidate)
}

/// `CellClass::PlaceInfantryInCell` @ `0x00481180` preference. Low bytes are
/// intentional. (There is no `CellClass::FindInfantrySubposition`.) The
/// centre test is `dx² + dy²` through `Sqrt_Approx` and `Math::ftol`
/// (`0x004811C6..0x004811EC`).
pub fn infantry_preferred_spot(point: CellQueryPoint) -> u8 {
    let x = point.x & 0xff;
    let y = point.y & 0xff;
    if crate::util::native_x87::sqrt_approx_length([
        x - CELL_CENTER_LEPTONS,
        y - CELL_CENTER_LEPTONS,
    ]) < 60
    {
        return 0;
    }
    let bits = u8::from(x > CELL_CENTER_LEPTONS) | (u8::from(y > CELL_CENTER_LEPTONS) << 1);
    if bits == 0 { 0 } else { bits + 1 }
}

/// `PlaceInfantryInCell` @ `0x00481180` rejects vehicle occupation on either list and normal
/// object occupation unless the caller proves the normal-list building traversable.
pub fn infantry_occupation_allows(
    occupied_mask: u8,
    normal_layer: bool,
    normal_building_traversable: bool,
) -> bool {
    occupied_mask & INFANTRY_OCCUPATION_VEHICLE_BIT == 0
        && (!normal_layer
            || occupied_mask & INFANTRY_OCCUPATION_OBJECT_BIT == 0
            || normal_building_traversable)
}

/// Return the preferred spot directly when caller bypasses occupancy. Otherwise
/// callers supply the one synchronized center-row draw only for preference zero.
pub fn select_infantry_subcell(
    preferred: u8,
    occupied_mask: u8,
    bypass_occupation: bool,
    random_row: Option<u8>,
) -> Option<u8> {
    if bypass_occupation {
        return Some(preferred);
    }
    if !infantry_occupation_allows(occupied_mask, false, false) {
        return None;
    }
    if preferred >= 2 && occupied_mask & (1 << preferred) == 0 {
        return Some(preferred);
    }
    let sequence = if preferred == 0 {
        &INFANTRY_ALTERNATE_SEQUENCE[usize::from(random_row? & 3)]
    } else {
        INFANTRY_SEQUENCE.get(usize::from(preferred))?
    };
    sequence
        .iter()
        .copied()
        .find(|spot| *spot >= 2 && occupied_mask & (1 << spot) == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wall_overlay_prefix_matches_original_cell_queries() {
        let native =
            crate::test_fixture::json("tools/spatial_oracle/anytown_damage/authored_fence.json");
        for case in native["cases"].as_array().unwrap() {
            let controls = &case["query_controls"];
            assert_eq!(controls["rng_before"], controls["rng_after"]);
            for row in controls["rows"].as_array().unwrap() {
                let stored = i32::try_from(row["stored"].as_i64().unwrap()).unwrap();
                let requested = i32::try_from(row["requested"].as_i64().unwrap()).unwrap();
                assert_eq!(
                    wall_connection_step(stored, requested),
                    WallConnectionStep::Return(row["returned_low_byte"].as_u64().unwrap() != 0),
                    "original480510 query {row}"
                );
                assert_eq!(row["rng_unchanged"], true);
            }
        }
    }

    #[test]
    fn yr_cell_floor_and_coordinate_vectors() {
        assert_eq!(ground_height_leptons(0xff, 0, 0, 0), Ok(-103));
        assert_eq!(ground_height_leptons(0, 1, 255, 0), Ok(103));
        assert_eq!(ground_height_leptons(0, 13, 255, 255), Ok(207));
        assert_eq!(ground_height_leptons(0, 20, 255, 255), Ok(52));
        assert_eq!(checked_cell_from_world(-1, 255, 2, 2), Some((0, 0)));
        assert_eq!(checked_cell_from_world(-257, 0, 2, 2), None);
        assert_eq!(
            cell_center(CellCoordinate { x: -1, y: 2 }, -103),
            WorldCoordinate {
                x: -128,
                y: 640,
                z: -103
            }
        );
    }

    #[test]
    fn yr_floor_and_deck_are_separate() {
        assert_eq!(selected_cell_surface_height(104, false), 104);
        assert_eq!(selected_cell_surface_height(104, true), 520);
        assert!(selects_infantry_bridge_layer(true, 2, 624));
        assert!(!selects_infantry_bridge_layer(true, 2, 623));
        assert!(!selects_infantry_bridge_layer(false, 2, 624));
    }

    #[test]
    fn yr_nearest_keeps_first_equal_distance() {
        assert_eq!(
            nearest_eligible_in_order(
                CellQueryPoint { x: 0, y: 0 },
                [
                    ("first", true, CellQueryPoint { x: 65, y: 63 }),
                    (
                        "equal native integer distance keeps list order",
                        true,
                        CellQueryPoint { x: 64, y: 64 }
                    ),
                ]
            ),
            Some("first")
        );
        let picked = nearest_eligible_in_order(
            CellQueryPoint { x: 10, y: 10 },
            [
                ("first", true, CellQueryPoint { x: 80, y: 70 }),
                ("equal", true, CellQueryPoint { x: 60, y: 70 }),
                ("inactive", false, CellQueryPoint { x: 70, y: 70 }),
            ],
        );
        assert_eq!(picked, Some("first"));
        assert_eq!(
            nearest_eligible_in_order::<u8>(CellQueryPoint { x: 0, y: 0 }, []),
            None
        );
    }

    #[test]
    fn yr_infantry_subcell_vectors() {
        assert_eq!(
            infantry_preferred_spot(CellQueryPoint { x: 128, y: 128 }),
            0
        );
        assert_eq!(
            infantry_preferred_spot(CellQueryPoint { x: 188, y: 128 }),
            2
        );
        assert_eq!(
            infantry_preferred_spot(CellQueryPoint { x: 128, y: 188 }),
            3
        );
        assert_eq!(
            infantry_preferred_spot(CellQueryPoint { x: 188, y: 188 }),
            4
        );
        assert_eq!(infantry_preferred_spot(CellQueryPoint { x: 68, y: 68 }), 0);
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/sqrt_approx_distances.json",
        ))
        .unwrap();
        // PlaceInfantryInCell 0x00481180..0x00481212 executed on each row's
        // coordinate (tools/spatial_oracle/sqrt_approx_distances.py).
        let rows = corpus["infantry"].as_array().unwrap();
        for row in rows {
            let [x, y, _]: [i32; 3] = serde_json::from_value(row["coord"].clone()).unwrap();
            assert_eq!(
                i64::from(infantry_preferred_spot(CellQueryPoint { x, y })),
                row["expected"].as_i64().unwrap(),
                "{}",
                row["name"]
            );
        }
        assert_eq!(rows.len(), 8);
        assert_eq!(select_infantry_subcell(0, 0, false, Some(0)), Some(2));
        assert_eq!(select_infantry_subcell(2, 1 << 2, false, None), Some(4));
        assert_eq!(
            select_infantry_subcell(4, (1 << 2) | (1 << 3) | (1 << 4), false, None),
            None
        );
        assert_eq!(
            select_infantry_subcell(0, INFANTRY_OCCUPATION_VEHICLE_BIT, false, Some(0)),
            None
        );
        assert_eq!(
            select_infantry_subcell(0, INFANTRY_OCCUPATION_VEHICLE_BIT, true, None),
            Some(0)
        );
        assert!(!infantry_occupation_allows(
            INFANTRY_OCCUPATION_OBJECT_BIT,
            true,
            false,
        ));
        assert!(infantry_occupation_allows(
            INFANTRY_OCCUPATION_OBJECT_BIT,
            true,
            true,
        ));
        assert!(infantry_occupation_allows(
            INFANTRY_OCCUPATION_OBJECT_BIT,
            false,
            false,
        ));
    }
}
