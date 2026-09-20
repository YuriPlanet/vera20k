//! Converts generator state into the engine's in-memory `MapFile`.
//!
//! Terrain, overlay and waypoint population lands with the phases; this
//! establishes the header geometry and the empty sections the rest of the
//! loader expects.

use std::collections::HashMap;

use crate::map::entities::{EntityCategory, MapEntity};
use crate::map::map_file::{MapCell, MapFile, MapHeader};
use crate::map::overlay::{OverlayDataPack, OverlayEntry, TerrainObject};
use crate::map::waypoints::Waypoint;

use crate::rules::ini_parser::IniFile;

use super::grid::RmgGrid;
use super::options::RmgOptions;
use super::tiles::TILE_UNASSIGNED;

/// Placed-at-full-health value for emitted neutral structures (256 = 100%).
const NEUTRAL_HEALTH: u16 = 256;
/// House the generator assigns to neutral tech buildings.
const NEUTRAL_OWNER: &str = "Neutral";

/// The full map is the generated interior plus a fixed margin, and the
/// playable area is inset from the top-left corner.
const SIZE_PAD_X: u32 = 4;
const SIZE_PAD_Y: u32 = 12;
const LOCAL_LEFT: u32 = 2;
const LOCAL_TOP: u32 = 5;

/// Map the theater option to the engine's theater name.
///
/// Out-of-range values fall back to temperate, which matches the original
/// leaving the option unclamped and indexing a fixed-stride name table.
pub fn theater_name(theater: i32) -> &'static str {
    match theater {
        1 => "SNOW",
        2 => "URBAN",
        3 => "DESERT",
        4 => "NEWURBAN",
        _ => "TEMPERATE",
    }
}

/// A `MapFile` with header and empty sections, ready for phases to fill.
///
/// `gen_w`/`gen_h` are the generated interior dimensions.
pub fn empty_map_file(options: &RmgOptions, gen_w: u32, gen_h: u32) -> MapFile {
    let header = MapHeader {
        theater: theater_name(options.theater).to_string(),
        fill: "Clear".to_string(),
        level: 0,
        width: gen_w + SIZE_PAD_X,
        height: gen_h + SIZE_PAD_Y,
        local_left: LOCAL_LEFT,
        local_top: LOCAL_TOP,
        local_width: gen_w,
        local_height: gen_h,
    };
    MapFile {
        header,
        basic: Default::default(),
        briefing: Default::default(),
        preview: Default::default(),
        cells: Vec::new(),
        iso_map_pack_lookups: Vec::new(),
        entities: Vec::new(),
        overlays: Vec::new(),
        authored_overlay_packs: crate::map::map_file::AuthoredOverlayPackSlot::empty(),
        smudges: Vec::new(),
        terrain_objects: Vec::new(),
        waypoints: HashMap::new(),
        cell_tags: Default::default(),
        tags: Default::default(),
        triggers: Default::default(),
        events: Default::default(),
        actions: Default::default(),
        local_variables: Default::default(),
        trigger_graph: Default::default(),
        special_flags: Default::default(),
        explicit_tubes: Vec::new(),
        // `IniFile` has no `Default`; an empty document is the equivalent.
        ini: IniFile::from_str(""),
    }
}

/// Project the generated grid and the phase outputs into a `MapFile`.
///
/// The grid `(x, y)` frame is the engine's cell-coordinate frame — the same one
/// the IsoMapPack loader reads into `MapCell { rx, ry }` — so the projection is
/// an identity coordinate mapping. Every in-band diamond cell becomes a
/// `MapCell`; the unassigned sentinel (`0xFFFF`) maps to the loader's no-tile
/// `-1`, and tile `0` stays `0` (the clear-set flat index). Cells carrying an
/// overlay also emit an `OverlayEntry` plus a density byte in the data pack.
///
/// `terrain` are the placed trees / TIBTRE `(name, x, y)`; `structures` are
/// generated neutral Buildings in constructor order (bridge-repair CABHUTs,
/// then neutral tech) as `(name, x, y)`; `waypoints` are the `(slot, x, y)`
/// start positions.
pub fn populate(
    map_file: &mut MapFile,
    grid: &RmgGrid,
    terrain: &[(String, i16, i16)],
    structures: &[(String, i16, i16)],
    waypoints: &[(u8, u16, u16)],
) {
    let mut overlay_cells: Vec<(u16, u16, u8)> = Vec::new();

    for (x, y) in grid.native_cells().collect::<Vec<_>>() {
        let cell = grid.get(x, y).expect("native cell is in-band");
        let (rx, ry) = (x as u16, y as u16);

        let tile_index = if cell.tile == TILE_UNASSIGNED {
            -1
        } else {
            cell.tile
        };
        map_file.cells.push(MapCell {
            rx,
            ry,
            tile_index,
            sub_tile: cell.sub_tile,
            z: cell.level,
        });

        if cell.overlay != -1 {
            map_file.overlays.push(OverlayEntry {
                rx,
                ry,
                overlay_id: cell.overlay as u8,
                frame: cell.density,
            });
            overlay_cells.push((rx, ry, cell.density));
        }
    }

    if !overlay_cells.is_empty() {
        map_file
            .replace_unconsumed_overlay_data(OverlayDataPack::from_cells(overlay_cells))
            .expect("new RMG map retains its raw overlay-pack slot");
    }

    for (name, x, y) in terrain {
        map_file.terrain_objects.push(TerrainObject {
            rx: *x as u16,
            ry: *y as u16,
            name: name.clone(),
        });
    }

    for (name, x, y) in structures {
        map_file.entities.push(MapEntity {
            owner: NEUTRAL_OWNER.to_string(),
            type_id: name.clone(),
            health: i32::from(NEUTRAL_HEALTH),
            cell_x: *x as u16,
            cell_y: *y as u16,
            facing: 0,
            category: EntityCategory::Structure,
            sub_cell: 0,
            veterancy: 0,
            high: false,
            mission: None,
            recruitable_a: true,
            recruitable_b: true,
            structure_upgrades: [None, None, None],
        });
    }

    for (slot, x, y) in waypoints {
        let index = u32::from(*slot);
        map_file.waypoints.insert(
            index,
            Waypoint {
                index,
                rx: *x,
                ry: *y,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::rmg::MapGeometry;

    #[test]
    fn theater_indices_map_to_engine_names() {
        assert_eq!(theater_name(0), "TEMPERATE");
        assert_eq!(theater_name(1), "SNOW");
        assert_eq!(theater_name(2), "URBAN");
        assert_eq!(theater_name(3), "DESERT");
        assert_eq!(theater_name(4), "NEWURBAN");
        assert_eq!(
            theater_name(99),
            "TEMPERATE",
            "the theater option is never clamped, so out-of-range must fall back"
        );
        assert_eq!(theater_name(-1), "TEMPERATE");
    }

    #[test]
    fn gsi_04_03a_header_is_level_zero_and_insets_the_playable_area() {
        let map = empty_map_file(&RmgOptions::default(), 60, 60);
        assert_eq!(map.header.level, 0);
        assert_eq!(map.header.width, 64, "full width is interior + 4");
        assert_eq!(map.header.height, 72, "full height is interior + 12");
        assert_eq!(map.header.local_left, 2);
        assert_eq!(map.header.local_top, 5, "playable area starts at row 5");
        assert_eq!((map.header.local_width, map.header.local_height), (60, 60));
    }

    #[test]
    fn generated_map_starts_with_no_content() {
        let map = empty_map_file(&RmgOptions::default(), 32, 32);
        assert!(map.cells.is_empty());
        assert!(map.overlays.is_empty());
        assert!(map.terrain_objects.is_empty());
        assert!(map.waypoints.is_empty());
    }

    fn grid() -> RmgGrid {
        let (map_w, map_h) = (20, 24);
        let stride = (map_w + map_h + 1) as usize;
        RmgGrid::new(stride, map_w, map_w + 2 * map_h)
    }

    #[test]
    fn gsi_04_03a_every_in_band_cell_emits_its_generated_level() {
        let g = grid();
        let expected = g.native_cells().count();
        let mut map = empty_map_file(&RmgOptions::default(), 16, 12);
        populate(&mut map, &g, &[], &[], &[]);
        assert_eq!(map.cells.len(), expected, "one MapCell per in-band cell");
        // An untouched grid is all unassigned → all no-tile (-1).
        assert!(map.cells.iter().all(|c| c.tile_index == -1));
        assert!(map.cells.iter().all(|c| c.z == 4));
        assert!(map.overlays.is_empty());
    }

    #[test]
    fn tile_sentinels_map_but_clear_zero_is_kept() {
        let mut g = grid();
        // (22,22) assigned the clear base tile 0; a neighbour left unassigned.
        assert!(g.is_valid(22, 22) && g.is_valid(23, 22));
        g.get_mut(22, 22).unwrap().tile = 0;
        g.get_mut(23, 22).unwrap().tile = 700;
        let mut map = empty_map_file(&RmgOptions::default(), 16, 12);
        populate(&mut map, &g, &[], &[], &[]);
        let at = |rx: u16, ry: u16| map.cells.iter().find(|c| c.rx == rx && c.ry == ry).unwrap();
        assert_eq!(
            at(22, 22).tile_index,
            0,
            "clear tile 0 is a real index, kept"
        );
        assert_eq!(at(23, 22).tile_index, 700, "assigned tile passes through");
        // Any untouched cell is still -1.
        assert!(map.cells.iter().any(|c| c.tile_index == -1));
    }

    #[test]
    fn overlays_project_with_density_and_data_pack() {
        let mut g = grid();
        g.get_mut(24, 24).unwrap().tile = 0;
        g.get_mut(24, 24).unwrap().overlay = 105; // an ore overlay index
        g.get_mut(24, 24).unwrap().density = 7;
        let mut map = empty_map_file(&RmgOptions::default(), 16, 12);
        populate(&mut map, &g, &[], &[], &[]);
        assert_eq!(map.overlays.len(), 1);
        let ov = &map.overlays[0];
        assert_eq!((ov.rx, ov.ry, ov.overlay_id, ov.frame), (24, 24, 105, 7));
        // The data pack carries the same density at that cell.
        let data = map
            .overlay_data_pack()
            .expect("new RMG map retains raw overlay data");
        assert!(data.is_present());
        assert_eq!(data.byte_at(24, 24), 7);
    }

    #[test]
    fn terrain_structures_and_waypoints_are_written() {
        let g = grid();
        let mut map = empty_map_file(&RmgOptions::default(), 16, 12);
        let terrain = vec![("TREE05".to_string(), 22i16, 22i16)];
        let structures = vec![("CAHOSP".to_string(), 25i16, 25i16)];
        let waypoints = vec![(0u8, 22u16, 22u16), (1u8, 30u16, 30u16)];
        populate(&mut map, &g, &terrain, &structures, &waypoints);

        assert_eq!(map.terrain_objects.len(), 1);
        assert_eq!(map.terrain_objects[0].name, "TREE05");
        assert_eq!(
            (map.terrain_objects[0].rx, map.terrain_objects[0].ry),
            (22, 22)
        );

        assert_eq!(map.entities.len(), 1);
        let building = &map.entities[0];
        assert_eq!(building.type_id, "CAHOSP");
        assert_eq!(building.owner, "Neutral");
        assert_eq!(building.health, 256);
        assert_eq!(building.category, EntityCategory::Structure);
        assert_eq!((building.cell_x, building.cell_y), (25, 25));

        assert_eq!(map.waypoints.len(), 2);
        assert_eq!(
            map.waypoints[&0],
            Waypoint {
                index: 0,
                rx: 22,
                ry: 22
            }
        );
        assert_eq!(
            map.waypoints[&1],
            Waypoint {
                index: 1,
                rx: 30,
                ry: 30
            }
        );
    }

    #[test]
    fn populated_geometry_matches_the_generator_grid_frame() {
        // Cells emit at their grid coordinates verbatim (identity frame): the
        // populated rx/ry are exactly the diamond cell coords.
        let options = RmgOptions::default();
        let geometry = MapGeometry::from_options(&options);
        let g = RmgGrid::new(geometry.stride, geometry.diamond_min, geometry.diamond_max);
        let mut map = empty_map_file(&options, geometry.gen_w as u32, geometry.gen_h as u32);
        populate(&mut map, &g, &[], &[], &[]);
        for (x, y) in g.native_cells() {
            assert!(
                map.cells
                    .iter()
                    .any(|c| c.rx == x as u16 && c.ry == y as u16),
                "grid cell ({x},{y}) projects to the same map coord"
            );
        }
    }

    /// Architecture invariant: the simulation must never depend on the
    /// generator. Generation is pre-play map construction.
    #[test]
    fn sim_does_not_reference_the_generator() {
        let mut offenders = Vec::new();
        for path in rust_files(std::path::Path::new("src/sim")) {
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            if text.contains("map::rmg") {
                offenders.push(path.display().to_string());
            }
        }
        assert!(
            offenders.is_empty(),
            "sim/ must not depend on the map generator: {offenders:?}"
        );
    }

    fn rust_files(root: &std::path::Path) -> Vec<std::path::PathBuf> {
        let mut out = Vec::new();
        let Ok(entries) = std::fs::read_dir(root) else {
            return out;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                out.extend(rust_files(&path));
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
        out
    }
}
