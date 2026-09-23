//! Narrow CellClass geometry and content-query kernels shared by simulation callers.
//!
//! These helpers intentionally keep floor, deck, coordinate lookup, and object-list
//! selection separate: YR composes them at their individual call sites.

#[cfg(test)]
use crate::sim::map::bridge_topology::BRIDGE_DECK_HEIGHT_LEPTONS;
use crate::util::fixed_math::isqrt_i64;
use crate::util::lepton::{
    GROUND_LEVEL_HEIGHT_LEPTONS, UnsupportedGroundSlope, ground_height_leptons,
};

pub const LEPTONS_PER_CELL: i32 = crate::util::lepton::LEPTONS_PER_CELL_I32;
pub const CELL_CENTER_LEPTONS: i32 = crate::util::lepton::CELL_CENTER_LEPTON_I32;
pub const INFANTRY_OCCUPATION_VEHICLE_BIT: u8 = 0x20;
pub const INFANTRY_OCCUPATION_OBJECT_BIT: u8 = 0x40;
/// Native-ordered (x, y, z) sub-cell lepton offsets, slots 0..4. Same values
/// as `util::lepton::SUBCELL_*` (the SimFixed pairs); kept as a separate
/// integer table because this is the native ordering + Z shape the kernel
/// indexes by slot. Change both or neither.
pub const INFANTRY_SUBCELL_OFFSETS: [(i32, i32, i32); 5] = [
    (128, 128, 0),
    (64, 64, 0),
    (192, 64, 0),
    (64, 192, 0),
    (192, 192, 0),
];

const INFANTRY_SEQUENCE: [[u8; 4]; 5] = [
    [1, 2, 3, 4],
    [0, 2, 3, 4],
    [0, 1, 4, 3],
    [0, 1, 4, 2],
    [0, 2, 3, 1],
];
const INFANTRY_ALTERNATE_SEQUENCE: [[u8; 4]; 4] =
    [[1, 2, 3, 4], [2, 3, 4, 1], [3, 4, 1, 2], [4, 1, 2, 3]];

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellQueryPoint {
    pub x: i32,
    pub y: i32,
}

/// YR `CellClass::GetCellCoords`: center the cell and keep its terrain floor.
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

/// `Coord2Cell` uses signed division truncating toward zero, not mathematical floor.
pub fn world_to_cell_trunc(world: i32) -> i32 {
    world / LEPTONS_PER_CELL
}

/// Native invalid-cell coordinates are process-global sentinels. At the Rust map
/// boundary, represent that unproven raw value as `None` rather than manufacture it.
#[cfg(test)]
pub fn checked_cell_from_world(
    world_x: i32,
    world_y: i32,
    width: u16,
    height: u16,
) -> Option<(u16, u16)> {
    let x = world_to_cell_trunc(world_x);
    let y = world_to_cell_trunc(world_y);
    if x < 0 || y < 0 || x >= i32::from(width) || y >= i32::from(height) {
        return None;
    }
    Some((x as u16, y as u16))
}

/// YR `CellClass::ComputeGroundHeightAtCoord @ 0x0047B3A0`; bridge height is
/// deliberately not included.
pub fn cell_floor_height(
    level: u8,
    slope_index: u8,
    world_x: i32,
    world_y: i32,
) -> Result<i32, UnsupportedGroundSlope> {
    ground_height_leptons(level, slope_index, world_x, world_y)
}

/// Apply the high-bridge deck only when a caller has already selected that layer.
#[cfg(test)]
pub fn selected_cell_surface_height(floor_z: i32, deck_selected: bool) -> i32 {
    if deck_selected {
        floor_z.wrapping_add(BRIDGE_DECK_HEIGHT_LEPTONS)
    } else {
        floor_z
    }
}

/// Alternate-list threshold: signed cell level plus four bridge levels,
/// independent of the floor kernel. There is no
/// `MapClass::PickInfantrySublocation` in this program — the name this
/// comment used to carry was invented.
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
    native_distance([dx, dy])
}

/// `FootClass::Find_Path` head 0x4D39F3..0x4D3A2B: the actor's `+0x48`
/// coordinate minus the target cell centre (x, y and z), each `FILD`ed, then
/// `(dz*dz + dy*dy) + dx*dx`, `Sqrt_Approx` and the truncating `Math_ftol`.
pub(crate) fn native_xyz_distance(dx: i32, dy: i32, dz: i32) -> i32 {
    native_distance([dz, dy, dx])
}

/// `CoordStruct::Distance3D` 0x0041C380 over a signed delta: `(x*x + y*y)
/// + z*z` in x87, `Sqrt_Approx` 0x4CAC40 and the truncating `Math_ftol`.
pub(crate) fn native_coord_distance(dx: i32, dy: i32, dz: i32) -> i32 {
    native_distance([dx, dy, dz])
}

/// Shared x87 shape: squares are accumulated in the given order, each square
/// added to the running sum before the next component.
fn native_distance(components: impl IntoIterator<Item = i32>) -> i32 {
    use crate::util::native_x87::{X87Chop53, sqrt_approx_f32};
    let mut components = components.into_iter().map(X87Chop53::load_i32);
    let first = components.next().expect("at least one distance component");
    let squared = components.fold(X87Chop53::mul(first, first), |sum, component| {
        X87Chop53::add(sum, X87Chop53::mul(component, component))
    });
    let Ok(root_bits) = sqrt_approx_f32(squared) else {
        return i32::MAX;
    };
    let Ok(root) = X87Chop53::load_f32(root_bits) else {
        return i32::MAX;
    };
    X87Chop53::ftol_i64(root).map_or(i32::MAX, |distance| distance as i32)
}

/// `FindTechnoNearestTo` scoring after the caller has applied linked-list order,
/// active bit, and excluded-object gates. Equal distances retain the first entry.
pub fn nearest_eligible_in_order<T>(
    input: CellQueryPoint,
    candidates: impl IntoIterator<Item = (T, bool, CellQueryPoint)>,
) -> Option<T> {
    let mut winner: Option<(i32, T)> = None;
    for (candidate, active, coordinate) in candidates {
        if !active {
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
/// intentional. (There is no `CellClass::FindInfantrySubposition`.)
pub fn infantry_preferred_spot(point: CellQueryPoint) -> u8 {
    let x = point.x & 0xff;
    let y = point.y & 0xff;
    let dx = i64::from(x - CELL_CENTER_LEPTONS);
    let dy = i64::from(y - CELL_CENTER_LEPTONS);
    if isqrt_i64(dx * dx + dy * dy) < 60 {
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

pub fn infantry_subcell_offset(spot: u8) -> (i32, i32, i32) {
    INFANTRY_SUBCELL_OFFSETS
        .get(usize::from(spot))
        .copied()
        .unwrap_or(INFANTRY_SUBCELL_OFFSETS[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yr_cell_floor_and_coordinate_vectors() {
        assert_eq!(cell_floor_height(0xff, 0, 0, 0), Ok(-103));
        assert_eq!(cell_floor_height(0, 1, 255, 0), Ok(103));
        assert_eq!(cell_floor_height(0, 13, 255, 255), Ok(207));
        assert_eq!(cell_floor_height(0, 20, 255, 255), Ok(52));
        assert_eq!(world_to_cell_trunc(-1), 0);
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
        assert_eq!(infantry_subcell_offset(3), (64, 192, 0));
    }
}
