//! How far a ground unit drives between cells, for VERA's Team Starts
//! (`ui::skirmish_shell`), which has no gamemd counterpart. The skirmish page
//! measures its map without a simulation: it resolves the map's terrain from
//! the startup Rules root, then steps over the cells a `MovementZone::Normal`
//! unit may enter.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use super::PathGrid;
use super::passability::is_passable_for_zone;
use crate::assets::asset_manager::AssetManager;
use crate::map::map_file::MapFile;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::map::theater::TheaterData;
use crate::rules::ini_parser::IniFile;
use crate::rules::locomotor_type::MovementZone;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::terrain_rules::TerrainRules;
use crate::sim::movement::locomotor::MovementLayer;

/// A straight step costs this much, a diagonal one [`DIAGONAL_STEP`]: the
/// distance in cells is the cost over `STEP`.
const STEP: u32 = 10;
const DIAGONAL_STEP: u32 = 14;

/// Drive distances in cells between `cells` on `map`, by index: `None` where
/// no ground route joins two cells. `rules_root` and `art` are the startup
/// Rules root and ART the shell holds. A scenario's mode and map Rules layers
/// are not applied, and terrain objects are not placed: trees and the map's
/// own land or overlay changes do not count.
pub fn map_drive_distances(
    map: &MapFile,
    theater: &TheaterData,
    assets: &AssetManager,
    rules_root: &IniFile,
    art: &IniFile,
    cells: &[(u16, u16)],
) -> Vec<Vec<Option<u32>>> {
    let terrain_rules = TerrainRules::from_ini(rules_root);
    let overlays = OverlayTypeRegistry::from_ini(rules_root, Some(art));
    let cliff_back_impassability =
        crate::rules::ruleset::read_cliff_back_impassability(rules_root.section("General"));
    let terrain = ResolvedTerrainGrid::build(
        map,
        Some(theater),
        Some(assets),
        Some(&terrain_rules),
        Some(&overlays),
        true,
        cliff_back_impassability,
    );
    drive_distances(&terrain, cells)
}

/// Drive distances in cells between `cells` on `terrain`, by index: `None`
/// where no ground route joins two cells.
///
/// A unit steps to any of the eight neighbouring cells that it may enter: a
/// cell whose RecalcZoneType class (`0x00483C80`) passes the Normal movement
/// row, a bridge deck, or one of `cells` (a start stands on its own cell).
/// Between two ground cells a step climbs at most one level, as the native
/// zone flood joins them (`flood_fill_node_index`); a diagonal step also
/// needs one of the two cells beside it, so a unit never squeezes between two
/// blocked corners.
pub fn drive_distances(
    terrain: &ResolvedTerrainGrid,
    cells: &[(u16, u16)],
) -> Vec<Vec<Option<u32>>> {
    let paths = PathGrid::from_resolved_terrain(terrain);
    let (width, height) = (i32::from(terrain.width()), i32::from(terrain.height()));
    // A cell a unit may enter: its level, and whether it is a bridge deck.
    let enterable = |x: i32, y: i32| -> Option<(u8, bool)> {
        let (x, y) = (u16::try_from(x).ok()?, u16::try_from(y).ok()?);
        let cell = terrain.cell(x, y)?;
        if paths.is_walkable_on_layer(x, y, MovementLayer::Bridge) {
            Some((cell.level, true))
        } else {
            (is_passable_for_zone(cell.zone_type, MovementZone::Normal) || cells.contains(&(x, y)))
                .then_some((cell.level, false))
        }
    };
    let step_allowed =
        |from: (u8, bool), to: (u8, bool)| from.1 || to.1 || from.0.abs_diff(to.0) <= 1;
    let index = |x: i32, y: i32| (y * width + x) as usize;
    let inside = |&(x, y): &(u16, u16)| i32::from(x) < width && i32::from(y) < height;

    cells
        .iter()
        .map(|&from| {
            let mut cost = vec![u32::MAX; (width * height).max(0) as usize];
            let mut unsettled = cells.iter().filter(|cell| inside(cell)).count();
            let mut queue = BinaryHeap::new();
            if inside(&from) {
                let (x, y) = (i32::from(from.0), i32::from(from.1));
                cost[index(x, y)] = 0;
                queue.push(Reverse((0u32, x, y)));
            }
            while let Some(Reverse((spent, x, y))) = queue.pop() {
                if spent > cost[index(x, y)] {
                    continue;
                }
                if cells.contains(&(x as u16, y as u16)) {
                    unsettled -= 1;
                    if unsettled == 0 {
                        break;
                    }
                }
                let Some(here) = enterable(x, y) else {
                    continue;
                };
                for (dx, dy) in [
                    (1, 0),
                    (-1, 0),
                    (0, 1),
                    (0, -1),
                    (1, 1),
                    (1, -1),
                    (-1, 1),
                    (-1, -1),
                ] {
                    let (nx, ny) = (x + dx, y + dy);
                    let Some(there) = enterable(nx, ny) else {
                        continue;
                    };
                    if !step_allowed(here, there) {
                        continue;
                    }
                    let diagonal = dx != 0 && dy != 0;
                    if diagonal && enterable(x + dx, y).is_none() && enterable(x, y + dy).is_none()
                    {
                        continue;
                    }
                    let next = spent + if diagonal { DIAGONAL_STEP } else { STEP };
                    if next < cost[index(nx, ny)] {
                        cost[index(nx, ny)] = next;
                        queue.push(Reverse((next, nx, ny)));
                    }
                }
            }
            cells
                .iter()
                .map(|to| {
                    inside(to)
                        .then(|| cost[index(i32::from(to.0), i32::from(to.1))])
                        .filter(|&spent| spent != u32::MAX)
                        .map(|spent| spent / STEP)
                })
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::resolved_terrain::{test_clear_cell, test_grid, zone_class};

    /// A lake between two cells makes the drive around it longer than the
    /// straight line, and a walled-in cell has no route at all.
    #[test]
    fn water_sends_the_drive_around_and_an_enclosed_cell_has_no_route() {
        // A 20x20 field; a lake fills column 10 except its last two rows, and
        // water rings the cell (17, 2).
        let terrain = test_grid(20, 20, |rx, ry| {
            let mut cell = test_clear_cell(rx, ry);
            let lake = rx == 10 && ry < 18;
            let ring = (16..=18).contains(&rx) && (1..=3).contains(&ry) && (rx, ry) != (17, 2);
            if lake || ring {
                cell.zone_type = zone_class::WATER;
            }
            cell
        });
        let cells = [(5, 2), (15, 2), (5, 8), (17, 2)];
        let drive = drive_distances(&terrain, &cells);
        // Same shore: the drive is the straight line.
        assert_eq!(drive[0][2], Some(6));
        // Across the lake: 10 cells apart, but the drive goes round its end.
        let across = drive[0][1].expect("a route round the lake");
        assert!(across >= 25, "drive across the lake was {across}");
        assert_eq!(drive[1][0], drive[0][1]);
        // The ringed cell reaches nothing, and nothing reaches it.
        assert_eq!(drive[3][..3], [None, None, None]);
        assert_eq!(drive[0][3], None);
        assert_eq!(drive[3][3], Some(0));
    }

    /// Sedona Pass, read as the skirmish page reads it (retail map, theater
    /// and startup Rules root): starts 2 and 3 stand 61 cells apart in a
    /// straight line but the drive between them goes round the cliffs, while
    /// starts 1 and 2 (58 cells) share open ground.
    #[test]
    fn sedona_pass_cliffs_lengthen_the_drive_between_neighbouring_starts() {
        let Some((root, mut assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let map = crate::map::source::load_map_by_name_or_path_with_assets(
            &root,
            "SedonaPass.MAP",
            &assets,
        )
        .expect("retail Sedona Pass")
        .map;
        let theater = crate::map::theater::load_theater(&mut assets, &map.header.theater)
            .expect("Sedona Pass theater");
        let select = |name| {
            crate::rules::retail_sources::select_ini(&assets, name)
                .unwrap()
                .ini
        };
        let (rules, art) = (select("RULESMD.INI"), select("ARTMD.INI"));
        // Waypoints 1, 2 and 3.
        let cells = [(90, 178), (143, 154), (178, 104)];
        let drive = map_drive_distances(&map, &theater, &assets, &rules, &art, &cells);
        assert!(
            drive[0][1].is_some_and(|cells| cells < 75),
            "1 to 2: {:?}",
            drive[0][1]
        );
        assert!(
            drive[1][2].is_some_and(|cells| cells > 100),
            "2 to 3: {:?}",
            drive[1][2]
        );
    }
}
