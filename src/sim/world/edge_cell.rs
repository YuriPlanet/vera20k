//! Map-edge cells: the house's edges and `MapClass::PickCellOnEdge @
//! 0x004AA440` as its aircraft callers use it (the empty cell as both
//! references, criterion 4): randomized LocalSize scans select a cell just
//! outside the isometric playfield without consulting ground passability.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on map/ and sim/cell_rect.
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

use crate::map::playfield::{PlayfieldBounds, local_to_packed_cell};
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::sim::cell_rect::cell_is_in_playfield_height_aware;
use crate::sim::rng::SimRng;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    North,
    East,
    South,
    West,
}

impl Edge {
    /// `HouseClass @ 0x0050DA80`: the house's own edge, `+0x577C`
    /// (`HouseState::waypoint_edge`) when it is 0..3, else North.
    pub(crate) fn own_edge(waypoint_edge: u8) -> Self {
        Self::from_index(waypoint_edge).unwrap_or(Edge::North)
    }

    /// Shared authored-edge choice in Retreat415A50, SendParadrop65E6C5
    /// and SendSpyPlanes65EB17. Their 0..3 override falls back to the sole
    /// GetEdge50DA80 owner above; it does not overwrite the waypoint field.
    pub(crate) fn authored_or_waypoint(authored: i32, waypoint_edge: u8) -> Self {
        u8::try_from(authored)
            .ok()
            .and_then(Self::from_index)
            .unwrap_or_else(|| Self::own_edge(waypoint_edge))
    }

    /// `HouseClass @ 0x0050DAC0`: the edge across from `+0x577C`, through
    /// its jump table `0x0050DAE8` (North to South, East to West and back);
    /// a value past 3 is North (`0x0050DAE4`).
    pub(crate) fn opposite_edge(waypoint_edge: u8) -> Self {
        match Self::from_index(waypoint_edge) {
            Some(Edge::North) => Edge::South,
            Some(Edge::East) => Edge::West,
            Some(Edge::West) => Edge::East,
            Some(Edge::South) | None => Edge::North,
        }
    }

    pub fn from_index(i: u8) -> Option<Self> {
        match i {
            0 => Some(Edge::North),
            1 => Some(Edge::East),
            2 => Some(Edge::South),
            3 => Some(Edge::West),
            _ => None,
        }
    }
}

/// Find the paradrop carrier spawn/exit cell along a MapClass edge.
///
/// Active spawner and Approach/Overfly callers pass sentinel references and
/// criterion `4`, as does Aircraft Mission_Attack's state 10 (`0x00418C43`).
/// `FUN_004AA440 @ 0x004AA440` first rejects cells that are inside mode-one
/// playfield geometry, then criterion 4 accepts the first outside candidate
/// unconditionally. Every edge spends its initial Scenario RandomRanged call;
/// North scans local row 0 (the sentinel arm leaves `param_4` 0), East and
/// West the local columns `LocalWidth` and 0; South gathers one outside
/// candidate per local X and spends a second draw to choose among the full
/// vector. Original rows: `tools/spatial_oracle/aircraft_states.py`
/// (`state10`, `state10_seeds`: all four edges over a flat map).
pub fn find_paradrop_edge_cell(
    playfield_bounds: Option<PlayfieldBounds>,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    edge: Edge,
    scenario_rng: &mut SimRng,
) -> Option<(u16, u16)> {
    let bounds = playfield_bounds?;
    let width = bounds.off_104;
    let twice_height = bounds.off_108.wrapping_mul(2);

    let start = match edge {
        Edge::North | Edge::South => scenario_rng
            .next_range_i32_inclusive(1, width)
            .wrapping_sub(1),
        Edge::East => scenario_rng
            .next_range_i32_inclusive(1, twice_height)
            .wrapping_sub(1),
        Edge::West => scenario_rng
            .next_range_i32_inclusive(0, twice_height)
            .wrapping_sub(1),
    };
    let fallback = local_to_packed_cell(bounds, 1, width / 2);

    match edge {
        Edge::North => {
            for n in 0..width {
                let local_u = n.wrapping_add(start) % width;
                let candidate = local_to_packed_cell(bounds, local_u, 0);
                if candidate_is_outside(candidate, bounds, resolved_terrain) {
                    return Some(pack_cell(candidate));
                }
            }
            Some(pack_cell(fallback))
        }
        Edge::East | Edge::West => {
            let local_u = if edge == Edge::East { width } else { 0 };
            for n in 0..twice_height {
                let local_v = n.wrapping_add(start) % twice_height;
                let candidate = local_to_packed_cell(bounds, local_u, local_v);
                if candidate_is_outside(candidate, bounds, resolved_terrain) {
                    return Some(pack_cell(candidate));
                }
            }
            Some(pack_cell(fallback))
        }
        Edge::South => {
            let mut candidates = Vec::with_capacity(10);
            for local_u in 0..width {
                for offset in 0..15 {
                    let local_v = twice_height.wrapping_add(offset);
                    let candidate = local_to_packed_cell(bounds, local_u, local_v);
                    if candidate_is_outside(candidate, bounds, resolved_terrain) {
                        candidates.push(pack_cell(candidate));
                        break;
                    }
                }
            }
            if candidates.is_empty() {
                return Some((0, 0));
            }
            let index = scenario_rng.next_range_i32_inclusive(
                0,
                i32::try_from(candidates.len() - 1).expect("native candidate count fits i32"),
            );
            Some(candidates[index as usize])
        }
    }
}

fn candidate_is_outside(
    candidate: (i32, i32),
    bounds: PlayfieldBounds,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
) -> bool {
    !cell_is_in_playfield_height_aware(candidate, Some(bounds), resolved_terrain)
}

const fn pack_cell(cell: (i32, i32)) -> (u16, u16) {
    (cell.0 as i16 as u16, cell.1 as i16 as u16)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square_bounds() -> PlayfieldBounds {
        PlayfieldBounds {
            base: 100,
            off_fc: 0,
            off_100: 0,
            off_104: 100,
            off_108: 100,
        }
    }

    /// A regression over one square LocalSize: the cell is VERA's arithmetic.
    /// The native evidence for North's row 0 is the oracle replay
    /// (`aircraft::attack_mission::tests::original_state10_rows`).
    #[test]
    fn paradrop_north_spends_initial_draw_and_returns_first_outside_cell() {
        let bounds = square_bounds();
        let mut expected_rng = SimRng::new(0xAA44_0000);
        let start = expected_rng.next_range_u32_inclusive(1, 100) as i32 - 1;
        assert_eq!(start, 36);
        let expected = pack_cell(local_to_packed_cell(bounds, start, 0));
        assert_eq!(expected, (36, 64));
        let mut actual_rng = SimRng::new(0xAA44_0000);
        assert_eq!(
            find_paradrop_edge_cell(Some(bounds), None, Edge::North, &mut actual_rng),
            Some(expected)
        );
        assert_eq!(actual_rng.logical_state(), expected_rng.logical_state());
    }

    #[test]
    fn paradrop_vertical_modes_keep_native_random_start_and_west_negative_remainder() {
        let bounds = square_bounds();
        for (edge, seed) in [(Edge::East, 0xAA44_0001), (Edge::West, 0xAA44_0001)] {
            let mut expected_rng = SimRng::new(seed);
            let start = match edge {
                Edge::East => expected_rng.next_range_u32_inclusive(1, 200) as i32 - 1,
                Edge::West => expected_rng.next_range_u32_inclusive(0, 200) as i32 - 1,
                _ => unreachable!(),
            };
            assert_eq!(
                start,
                if edge == Edge::East { 0 } else { -1 },
                "fixed witness pins West's negative x86 remainder"
            );
            let local_u = if edge == Edge::East { 100 } else { 0 };
            let expected = (0..200)
                .map(|n| local_to_packed_cell(bounds, local_u, (n + start) % 200))
                .find(|&candidate| candidate_is_outside(candidate, bounds, None))
                .map(pack_cell)
                .expect("square edge scan has an outside candidate");
            let mut actual_rng = SimRng::new(seed);
            assert_eq!(
                find_paradrop_edge_cell(Some(bounds), None, edge, &mut actual_rng),
                Some(expected),
                "{edge:?}"
            );
            assert_eq!(
                actual_rng.logical_state(),
                expected_rng.logical_state(),
                "{edge:?}"
            );
        }
    }

    #[test]
    fn paradrop_south_grows_past_ten_and_randomly_selects_full_vector() {
        let bounds = square_bounds();
        let mut expected_rng = SimRng::new(0xAA44_0002);
        let _unused_start_draw = expected_rng.next_range_u32_inclusive(1, 100);
        assert_eq!(_unused_start_draw, 61);
        let mut candidates = Vec::new();
        for local_u in 0..100 {
            let candidate = (0..15)
                .map(|offset| local_to_packed_cell(bounds, local_u, 200 + offset))
                .find(|&candidate| candidate_is_outside(candidate, bounds, None))
                .expect("each square south column reaches outside within fifteen rows");
            candidates.push(pack_cell(candidate));
        }
        assert_eq!(candidates.len(), 100, "ten is capacity, not a cap");
        let selected = expected_rng.next_range_u32_inclusive(0, 99) as usize;
        assert_eq!(selected, 29, "fixed witness exercises grown storage");
        assert_eq!(candidates[selected], (131, 172));
        let mut actual_rng = SimRng::new(0xAA44_0002);
        assert_eq!(
            find_paradrop_edge_cell(Some(bounds), None, Edge::South, &mut actual_rng),
            Some(candidates[selected])
        );
        assert_eq!(actual_rng.logical_state(), expected_rng.logical_state());
    }

    #[test]
    fn paradrop_edge_requires_mapclass_authority_without_spending_rng() {
        let mut rng = SimRng::new(0xAA44_FFFF);
        let before = rng.logical_state();
        assert_eq!(
            find_paradrop_edge_cell(None, None, Edge::North, &mut rng),
            None
        );
        assert_eq!(rng.logical_state(), before);
    }

    #[test]
    fn test_edge_from_index() {
        assert_eq!(Edge::from_index(0), Some(Edge::North));
        assert_eq!(Edge::from_index(1), Some(Edge::East));
        assert_eq!(Edge::from_index(2), Some(Edge::South));
        assert_eq!(Edge::from_index(3), Some(Edge::West));
        assert_eq!(Edge::from_index(4), None);
    }
}
