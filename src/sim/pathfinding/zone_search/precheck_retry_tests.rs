//! Tests for the retries of `AStar_pathfind_search` (`0x0042C900`).

use super::super::super::zone_hierarchy::ZoneEdgeRecord;
use super::super::super::zone_map_tests::terrain_from_zone_classes;
use super::super::super::{
    BlockerNeighborCounts, LayeredPathStep, MoverSearchFacts, PathGrid, find_layered_path_marker,
};
use super::super::{CorridorPrecheck, find_layered_path_zoned_marker_detailed};
use super::*;
use crate::map::resolved_terrain::zone_class;
use crate::sim::movement::locomotor::MovementLayer;
use crate::util::native_x87::NativeF64Bits;

/// Ground where `open` holds and rock elsewhere, with the zones and the
/// hierarchy built from it.
struct Map {
    grid: PathGrid,
    terrain: ResolvedTerrainGrid,
    zones: ZoneGrid,
}

impl Map {
    fn new(width: u16, height: u16, open: impl Fn(u16, u16) -> bool) -> Self {
        let mut grid = PathGrid::new(width, height);
        let mut classes = Vec::new();
        for y in 0..height {
            for x in 0..width {
                grid.set_blocked(x, y, !open(x, y));
                classes.push(if open(x, y) {
                    zone_class::GROUND
                } else {
                    zone_class::IMPASSABLE
                });
            }
        }
        let terrain = terrain_from_zone_classes(width, height, &classes, &vec![0; classes.len()]);
        let size = Some((i32::from(width), i32::from(height)));
        let zones =
            ZoneGrid::build_with_native_bridge_geometry(&grid, &terrain, &[], width, height, size);
        Self {
            grid,
            terrain,
            zones,
        }
    }

    fn zone(&self, level: usize, cell: (u16, u16)) -> ZoneId {
        self.zones
            .hierarchy_zone_at_native(level, cell)
            .expect("a hierarchy cell")
    }

    fn hierarchy(&self) -> &ZoneHierarchy {
        self.zones
            .hierarchy_for(MovementZone::Normal)
            .expect("the Normal row")
    }

    fn retry_inputs<'a>(&'a self, foot: &'a Foot<'a>) -> RetryInputs<'a> {
        RetryInputs {
            zone_grid: &self.zones,
            hierarchy: self.hierarchy(),
            terrain: &self.terrain,
            bounds: None,
            movement_zone: MovementZone::Normal,
            foot_entry: foot,
        }
    }

    /// A Normal mover's whole path search, hierarchy admitted.
    fn search(
        &self,
        start: (u16, u16),
        goal: (u16, u16),
        refused: &[(u16, u16)],
    ) -> Result<Vec<LayeredPathStep>, PathSearchFailure> {
        let foot = Foot {
            grid: &self.grid,
            refused,
        };
        find_layered_path_zoned_marker_detailed(
            &self.grid,
            start,
            MovementLayer::Ground,
            goal,
            Some(&self.zones),
            MovementZone::Normal,
            Some(MovementZone::Normal),
            Some(&self.terrain),
            None,
            Some(&BlockerNeighborCounts::new(
                self.grid.width(),
                self.grid.height(),
            )),
            MoverSearchFacts {
                urgency: 0,
                foot_entry: &foot,
            },
            true,
            None,
        )
    }
}

/// A Foot that refuses (code 7) rock and the `refused` cells, which the zones
/// still cross, and sees no threat.
struct Foot<'a> {
    grid: &'a PathGrid,
    refused: &'a [(u16, u16)],
}

impl SearchFootEntry for Foot<'_> {
    fn classify(&self, query: SearchEntryQuery) -> Result<u8, String> {
        let SearchEntryCandidate::CopiedCoord(cell) = query.candidate else {
            return Err("this Foot answers copied coordinates only".into());
        };
        Ok(
            if self.grid.is_walkable(cell.0, cell.1) && !self.refused.contains(&cell) {
                0
            } else {
                7
            },
        )
    }
    fn finishing_threat_coefficient(&self) -> Result<NativeF64Bits, String> {
        Ok(NativeF64Bits::POSITIVE_ZERO)
    }
    fn hierarchy_house_threat(&self, _padded_index: i32) -> Result<i32, String> {
        Ok(0)
    }
}

/// Rooms at x 0..4 and 12..16 joined by a pass over rows 0..2 and another
/// over rows 10..12.
fn two_passes() -> Map {
    Map::new(16, 12, |x, y| !(4..12).contains(&x) || y < 2 || y >= 10)
}

/// The middle of the top pass.
const TOP_PASS_MIDDLE: [(u16, u16); 4] = [(8, 0), (9, 0), (8, 1), (9, 1)];

#[test]
fn a_failed_corridor_search_retries_through_the_other_pass() {
    let map = two_passes();
    let route = map
        .search((1, 1), (14, 1), &TOP_PASS_MIDDLE)
        .expect("the second try routes");
    let cells: Vec<_> = route.iter().map(|step| (step.rx, step.ry)).collect();
    assert_eq!(cells.last(), Some(&(14, 1)));
    assert!(
        cells.iter().any(|&(x, y)| (4..12).contains(&x) && y >= 10),
        "the route takes the bottom pass: {cells:?}"
    );
}

#[test]
fn the_retry_excludes_the_edges_the_flood_could_not_cross() {
    let map = two_passes();
    let foot = Foot {
        grid: &map.grid,
        refused: &TOP_PASS_MIDDLE,
    };
    let counts = BlockerNeighborCounts::new(16, 12);
    let hierarchy = map.hierarchy();
    let precheck = CorridorPrecheck {
        zone_grid: &map.zones,
        hierarchy,
        level0_zones: hierarchy.level(0).expect("level 0"),
        blocker_neighbor_counts: &counts,
        start_zone: map.zone(0, (1, 1)),
        goal_zone: map.zone(0, (14, 1)),
        movement_zone: MovementZone::Normal,
        threat_coefficient: NativeF64Bits::POSITIVE_ZERO,
    };
    let corridor = precheck
        .run(&ZonePrecheckExclusions::default(), &foot, (1, 1))
        .expect("a precheck")
        .expect("the top pass is the precheck's route");
    let first = find_layered_path_marker(
        &map.grid,
        (1, 1),
        MovementLayer::Ground,
        (14, 1),
        Some(corridor.gate()),
        Some(&map.terrain),
        None,
        MoverSearchFacts {
            urgency: 0,
            foot_entry: &foot,
        },
    );
    assert_eq!(first, Err(PathSearchFailure::CellSearchExhausted));

    // The furthest route zone the search reached is the last before the
    // refused cells, and it floods from the first cell it gave a node there.
    let progress = corridor.progress.get();
    assert_eq!(corridor.routes[0][progress.index], map.zone(0, (6, 0)));
    assert_eq!(map.zone(0, progress.cell), map.zone(0, (6, 0)));

    // At each level that flood stops at the refused cells and finds the zone
    // unsplit, so only its edge across the pass is excluded.
    let mut exclusions = ZonePrecheckExclusions::default();
    let usable = update_hierarchical_edges(
        &map.retry_inputs(&foot),
        &corridor.routes,
        progress.cell,
        &mut exclusions,
    );
    assert_eq!(usable, Ok(true));
    let mut expected = ZonePrecheckExclusions::default();
    for level in 0..ZONE_PRECHECK_LEVELS {
        expected.insert(level, map.zone(level, (6, 0)), map.zone(level, (8, 0)));
    }
    assert_eq!(exclusions, expected);

    let retried = precheck
        .run(&exclusions, &foot, (1, 1))
        .expect("a precheck")
        .expect("the bottom pass is the next route");
    assert!(retried.routes[0].contains(&map.zone(0, (6, 10))));
}

#[test]
fn a_split_zone_on_a_one_zone_route_runs_the_plain_search() {
    // The map is one level-2 zone, so the level-2 flood finds it split by the
    // refused wall and its one-zone route clears the hierarchy.
    let map = Map::new(8, 8, |_, _| true);
    let wall: Vec<_> = (0..6).flat_map(|y| [(3, y), (4, y)]).collect();
    let foot = Foot {
        grid: &map.grid,
        refused: &wall,
    };
    let counts = BlockerNeighborCounts::new(8, 8);
    let hierarchy = map.hierarchy();
    let precheck = CorridorPrecheck {
        zone_grid: &map.zones,
        hierarchy,
        level0_zones: hierarchy.level(0).expect("level 0"),
        blocker_neighbor_counts: &counts,
        start_zone: map.zone(0, (1, 1)),
        goal_zone: map.zone(0, (6, 1)),
        movement_zone: MovementZone::Normal,
        threat_coefficient: NativeF64Bits::POSITIVE_ZERO,
    };
    let corridor = precheck
        .run(&ZonePrecheckExclusions::default(), &foot, (1, 1))
        .expect("a precheck")
        .expect("a corridor");
    let first = find_layered_path_marker(
        &map.grid,
        (1, 1),
        MovementLayer::Ground,
        (6, 1),
        Some(corridor.gate()),
        Some(&map.terrain),
        None,
        MoverSearchFacts {
            urgency: 0,
            foot_entry: &foot,
        },
    );
    assert_eq!(first, Err(PathSearchFailure::CellSearchExhausted));
    let usable = update_hierarchical_edges(
        &map.retry_inputs(&foot),
        &corridor.routes,
        corridor.progress.get().cell,
        &mut ZonePrecheckExclusions::default(),
    );
    assert_eq!(usable, Ok(false), "the split clears +0x38");
    let route = map
        .search((1, 1), (6, 1), &wall)
        .expect("the plain search routes");
    assert!(
        route.iter().any(|step| step.ry >= 6),
        "the route goes round the wall"
    );
}

#[test]
fn the_split_scan_tests_one_cell() {
    // 0x00584395..0x0058441C adds the start's place in its block to the
    // start for every place it scans, so of two floods of one zone that
    // leave the same places unmarked, only the one whose probe lands in the
    // zone answers split.
    let map = Map::new(8, 8, |_, _| true);
    let foot = Foot {
        grid: &map.grid,
        refused: &[(3, 0), (3, 1)],
    };
    let inputs = map.retry_inputs(&foot);
    let zone = map.zone(0, (2, 0));
    assert_eq!(map.zone(0, (2, 1)), zone);
    assert_eq!(
        flood_fill_reachable_zones(&inputs, (2, 0), 0, zone),
        Ok(ZoneFlood::Split)
    );
    // From (2, 1) the probe is (2, 2), in the zone below.
    let Ok(ZoneFlood::Unreached(unreached)) = flood_fill_reachable_zones(&inputs, (2, 1), 0, zone)
    else {
        panic!("the probe lies outside the zone");
    };
    assert!(unreached.contains(&map.zone(0, (4, 0))));
    assert!(!unreached.contains(&map.zone(0, (0, 0))));
    assert!(!unreached.contains(&map.zone(0, (2, 2))));
}

#[test]
fn invalidating_a_route_edge_excludes_the_zones_beside_both_ends() {
    // Zones 1-2-3 in a row, 4 beside 2 and 3, 5 beside 3 alone.
    let mut graph = ZoneLevelGraph::new(5);
    for (a, b) in [(1, 2), (2, 3), (2, 4), (3, 4), (3, 5)] {
        graph.push_edge(a, ZoneEdgeRecord::new(b, 0));
        graph.push_edge(b, ZoneEdgeRecord::new(a, 0));
    }
    let route = [1, 2, 3];
    let mut expected = ZonePrecheckExclusions::default();
    expected.insert(1, 2, 3);
    expected.insert(1, 2, 4);
    for zone in [2, 3] {
        // The last zone takes the edge from the zone before it.
        let mut excluded = ZonePrecheckExclusions::default();
        assert!(invalidate_zone_edge(&graph, &route, zone, 1, &mut excluded));
        assert_eq!(excluded, expected);
    }
    // A zone off the route, or a route of one zone, clears +0x38 instead.
    let mut excluded = ZonePrecheckExclusions::default();
    assert!(!invalidate_zone_edge(&graph, &route, 5, 1, &mut excluded));
    assert!(!invalidate_zone_edge(
        &graph,
        &route[2..],
        3,
        1,
        &mut excluded
    ));
    assert_eq!(excluded, ZonePrecheckExclusions::default());
}

/// Rooms at x 0..2 and 14..16 joined in each band of eight rows by a pass
/// over the band's first two rows.
fn banded_passes(bands: u16) -> Map {
    Map::new(16, 8 * bands, |x, y| x < 2 || x >= 14 || y % 8 < 2)
}

/// The middles of the passes of `bands`.
fn pass_middles(bands: std::ops::Range<u16>) -> Vec<(u16, u16)> {
    bands
        .flat_map(|band| {
            let y = 8 * band;
            [(8, y), (9, y), (8, y + 1), (9, y + 1)]
        })
        .collect()
}

#[test]
fn one_path_search_runs_five_corridor_searches() {
    // Each failed try excludes its band's pass, so the next precheck takes
    // the next band's: the fifth try takes the fifth pass.
    let map = banded_passes(6);
    let route = map
        .search((1, 1), (14, 1), &pass_middles(0..4))
        .expect("the fifth try routes");
    assert!(
        route
            .iter()
            .any(|step| (2..14).contains(&step.rx) && step.ry / 8 == 4)
    );

    // With five passes refused the budget ends the search, though the plain
    // search takes the sixth pass.
    let refused = pass_middles(0..5);
    assert_eq!(
        map.search((1, 1), (14, 1), &refused),
        Err(PathSearchFailure::CellSearchExhausted)
    );
    let foot = Foot {
        grid: &map.grid,
        refused: &refused,
    };
    let plain = find_layered_path_marker(
        &map.grid,
        (1, 1),
        MovementLayer::Ground,
        (14, 1),
        None,
        Some(&map.terrain),
        None,
        MoverSearchFacts {
            urgency: 0,
            foot_entry: &foot,
        },
    );
    assert!(plain.is_ok());
}
