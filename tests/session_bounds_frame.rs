//! Regression guard for the session-bounds coordinate frame.
//!
//! Sim cell coordinates (IsoMapPack cells, entities, waypoints, vision) live
//! in the iso ARRAY frame, whose extent is ~(SizeW+SizeH) per axis — NOT the
//! raw `[Map] Size=` rect. The launch descriptor must carry the cell-array
//! dims (max rx/ry + 1), or start waypoints land outside the fog window and
//! the player's own base stays permanently shrouded.
//!
//! Skips silently when no retail map is present on this machine.

use std::path::Path;

use vera20k::map::map_file::MapFile;
use vera20k::map::waypoints::multiplayer_start_waypoints;

/// The retail install named by `RA2_DIR`; empty when it is unset, so the test skips.
fn retail_dir() -> String {
    std::env::var("RA2_DIR").unwrap_or_default()
}

#[test]
fn start_waypoints_fit_cell_grid_dims_not_header_size() {
    let dir = retail_dir();
    let dir = Path::new(&dir);
    if !dir.exists() {
        println!("SKIP: no retail dir");
        return;
    }
    let mut checked = 0usize;
    let mut frame_difference_seen = false;
    let entries: Vec<_> = std::fs::read_dir(dir)
        .map(|rd| rd.flatten().collect())
        .unwrap_or_default();
    for entry in entries {
        let path = entry.path();
        let is_map = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("map") || e.eq_ignore_ascii_case("mmx"));
        if !is_map {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let Ok(map) = MapFile::from_bytes(&bytes) else {
            continue; // .mmx containers that aren't raw INI parse elsewhere
        };
        // Cell-array dims — the same derivation ResolvedTerrainGrid uses and
        // the frame the launch descriptor must carry.
        let (mut gw, mut gh) = (0u16, 0u16);
        for cell in &map.cells {
            gw = gw.max(cell.rx.saturating_add(1));
            gh = gh.max(cell.ry.saturating_add(1));
        }
        if gw == 0 || gh == 0 {
            continue;
        }
        for wp in multiplayer_start_waypoints(&map.waypoints) {
            assert!(
                wp.rx < gw && wp.ry < gh,
                "{}: start waypoint {} at ({},{}) outside cell grid {}x{}",
                path.display(),
                wp.index,
                wp.rx,
                wp.ry,
                gw,
                gh
            );
            // Document the frame difference: on real maps the iso array
            // extends past the Size= rect, so Size= would be the WRONG bound.
            if u32::from(wp.rx) >= map.header.width || u32::from(wp.ry) >= map.header.height {
                frame_difference_seen = true;
            }
        }
        checked += 1;
        if checked >= 5 {
            break;
        }
    }
    if checked == 0 {
        println!("SKIP: no parseable retail maps found");
        return;
    }
    assert!(
        frame_difference_seen,
        "expected at least one start waypoint beyond [Map] Size= dims across {checked} maps — \
         if this stops holding, re-verify the bounds-frame analysis before changing the descriptor source"
    );
}
