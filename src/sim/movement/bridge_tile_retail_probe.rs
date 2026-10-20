//! Production-loader input witness for native middle-ramp terrain replacement.
//! Loading is not emulated here; a native harness must establish damage parity.

#[test]
#[ignore = "requires active retail assets; exercises the resident bridge catalog after normal loading"]
fn retail_middle_bridge_resident_recalc_admits_both_families() {
    let retail = super::retail_dir().expect("configured active retail install");
    // This NEWURBAN mission has complete footprints for both orientations.
    // load() returns after dropping its temporary TheaterData/AssetManager.
    // The headless fixture loader accepts loose map paths; campaign maps may
    // need extraction from the install's MIX archive before this test.
    let map_path = std::env::var("VERA20K_C3Y03MD_MAP").unwrap_or_else(|_| "c3y03md.map".into());
    if !std::path::Path::new(&map_path).is_absolute()
        && !std::path::Path::new(&retail).join(&map_path).exists()
    {
        eprintln!("SKIPPED: campaign map {map_path} is not extracted beside the retail install");
        return;
    }
    let scenario = crate::headless_scenario::load(&retail, &map_path, super::SEED)
        .expect("load actual retail mission");
    let sim = scenario.sim();
    let initial = sim.resolved_terrain.as_ref().expect("resolved terrain");
    let overlays = sim.overlay_grid.as_ref().expect("live overlays");
    let registry = &scenario.runtime.resources.overlay_registry;
    let keys = initial
        .high_bridge_rim_tiles()
        .expect("retail theater keys");
    let mut grid = initial.clone();
    let mut coverage = std::collections::BTreeSet::new();
    let mut saw_empty_overlay = false;
    let mut saw_structural_overlay = false;
    for (axis, middle) in keys.middle.into_iter().enumerate() {
        let base = keys.base + middle - 1;
        let first = initial
            .iter()
            .find(|cell| {
                (base..=base + 4).contains(&cell.final_tile_index) && cell.final_sub_tile == 0
            })
            .unwrap_or_else(|| panic!("loaded mission lacks family {axis}"));
        let width = if axis == 0 { 2 } else { 5 };
        for sub in 0..10u8 {
            let coord = (
                first.rx + u16::from(sub) % width,
                first.ry + u16::from(sub) / width,
            );
            let source = initial.cell(coord.0, coord.1).expect("complete footprint");
            assert!((base..=base + 4).contains(&source.final_tile_index));
            assert_eq!(source.final_sub_tile, sub);
            let overlay = overlays.finalized_map_cell(coord.0, coord.1).unwrap();
            saw_empty_overlay |= overlay.overlay_id().is_none();
            saw_structural_overlay |=
                overlay.overlay_id().is_some() && source.bridge_facts.has_structural_bridge();
            let index = grid.index(coord.0, coord.1).unwrap();
            for variant in 0..5 {
                let target = base + variant;
                for level_override in [-1, 0].into_iter().take(if variant == 4 { 2 } else { 1 }) {
                    let cell = grid.cell_mut_for_test(coord.0, coord.1).unwrap();
                    *cell = source.clone();
                    cell.final_tile_index = target;
                    let outcome = grid
                        .recalc_resident_bridge_cell(
                            index,
                            overlay,
                            level_override,
                            registry,
                            sim.playfield_bounds(),
                        )
                        .unwrap_or_else(|error| {
                            panic!("retail tile {target}/{sub}, level {level_override}: {error}")
                        });
                    let cell = grid.cell(coord.0, coord.1).unwrap();
                    assert_eq!((cell.final_tile_index, cell.final_sub_tile), (target, sub));
                    assert_eq!(
                        cell.level,
                        if level_override == -1 {
                            source.level
                        } else {
                            0
                        }
                    );
                    assert_eq!(cell.bridge_facts.raw_flags, source.bridge_facts.raw_flags);
                    assert_eq!(
                        cell.terrain_object_occupation,
                        source.terrain_object_occupation
                    );
                    assert_eq!(outcome.finalized, overlay);
                    coverage.insert((target, sub));
                }
                *grid.cell_mut_for_test(coord.0, coord.1).unwrap() = source.clone();
            }
        }
    }
    assert_eq!(
        coverage.len(),
        100,
        "ten resident heads with ten valid subtiles each"
    );
    assert!(saw_empty_overlay && saw_structural_overlay);
    println!("c3y03md.map: 100 resident tile/subtile pairs and 20 explicit level overrides passed");
}

#[test]
#[ignore = "requires active retail assets; writes a local native-oracle input receipt"]
fn retail_high_bridge_middle_tile_inputs() {
    use serde_json::json;
    let retail = super::retail_dir().expect("configured active retail install");
    let map_name =
        std::env::var("VERA20K_BRIDGE_TILE_MAP").unwrap_or_else(|_| "xmp34u4.map".into());
    if !std::path::Path::new(&map_name).is_absolute()
        && !std::path::Path::new(&retail).join(&map_name).exists()
    {
        eprintln!("SKIPPED: campaign map {map_name} is not extracted beside the retail install");
        return;
    }
    let scenario = crate::headless_scenario::load(&retail, &map_name, super::SEED)
        .unwrap_or_else(|error| panic!("load {map_name}: {error}"));
    let sim = scenario.sim();
    let terrain = sim.resolved_terrain.as_ref().expect("resolved terrain");
    let bridges = sim.bridge_state.as_ref().expect("bridge state");
    let keys = terrain
        .high_bridge_rim_tiles()
        .expect("retail theater keys");
    let overlays = sim.overlay_grid.as_ref().expect("live overlays");
    let cells: Vec<_> = terrain
        .iter()
        .map(|cell| {
            let live = overlays.cell(cell.rx, cell.ry);
            json!([
                cell.rx,
                cell.ry,
                cell.final_tile_index,
                cell.final_sub_tile,
                cell.bridge_facts.raw_flags,
                live.overlay_id,
                live.overlay_data,
                cell.bridge_facts
                    .native_anchor
                    .map(|anchor| terrain.native_cell_coord(anchor)),
                cell.level,
                cell.land_type
            ])
        })
        .collect();
    let middle: Vec<_> = terrain
        .iter()
        .filter(|cell| {
            let relative = cell
                .final_tile_index
                .wrapping_sub(keys.base)
                .wrapping_add(1);
            keys.middle
                .iter()
                .any(|middle| (0..=4).any(|variant| relative == middle + variant))
        })
        .map(|cell| {
            json!({"coord": [cell.rx, cell.ry], "tile": cell.final_tile_index,
        "subtile": cell.final_sub_tile, "level": cell.level, "slope": cell.slope_type,
        "land": cell.land_type, "zone": cell.zone_type,
        "has_deck": cell.has_bridge_deck, "walkable": cell.bridge_walkable,
        "bridge_facts": cell.bridge_facts})
        })
        .collect();
    assert!(
        !middle.is_empty(),
        "selected map must establish a live middle-ramp witness"
    );
    let result = json!({"map": map_name, "theater": scenario.map.header.theater,
        "seed": super::SEED,
        "size": [scenario.map.header.width, scenario.map.header.height],
        "local_size": [scenario.map.header.local_left, scenario.map.header.local_top,
            scenario.map.header.local_width, scenario.map.header.local_height],
        "bridge_base": keys.base,
        "rim_keys": {"BridgeTopLeft1": keys.top_left[0], "BridgeTopLeft2": keys.top_left[1],
            "BridgeBottomRight1": keys.bottom_right[0], "BridgeBottomRight2": keys.bottom_right[1],
            "BridgeTopRight1": keys.top_right[0], "BridgeTopRight2": keys.top_right[1],
            "BridgeBottomLeft1": keys.bottom_left[0], "BridgeBottomLeft2": keys.bottom_left[1],
            "BridgeMiddle1": keys.middle[0], "BridgeMiddle2": keys.middle[1]},
        "records": bridges.endpoint_records(), "cells": cells, "middle_cells": middle,
        "cell_fields": ["x", "y", "tile", "subtile", "full_bridge_flags", "overlay_id",
            "overlay_state", "literal_anchor_current_coord", "level", "land"],
        "scope": "Current Rust retail-loader scalars and runtime middle-ramp projection; native loading and damage excluded"});
    let output = std::env::var("VERA20K_BRIDGE_TILE_OUTPUT")
        .unwrap_or_else(|_| ".local/bridge-middle-retail-inputs.json".into());
    std::fs::write(&output, serde_json::to_vec(&result).unwrap()).unwrap();
    println!(
        "{map_name}: {} cells, {} middle cells, saved {output}",
        cells.len(),
        middle.len()
    );
}
