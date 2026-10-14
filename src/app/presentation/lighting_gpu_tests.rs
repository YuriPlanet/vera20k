//! Explicit stock-asset GPU witness for live lamp-to-ground lighting.
//! The authored map is a diagnostic copy; this is not a retail pixel-parity oracle.
//!
//! Reproduction (retail files remain local): extract Fight.MAP from multimd.mix
//! with asset-browser, entry 0x9306F050, 91254 bytes, SHA-256
//! d751dce7cd3611077e9228c33235f39c71681fff6ac08ca1f716d963ad6ce070.
//! Make a diagnostic copy with exactly these edits:
//! - In [Lighting], change Ambient=1.000000 to Ambient=0.300000.
//! - Immediately after [Structures], insert this stock-type structure row:
//!   9000000=Neutral,GALITE,256,53,113,0,None,0,0,1,0,0,None,None,None,0,0
//!
//! No rules/type overrides are needed. Set RA2_DIR to the retail installation,
//! VERA20K_LAMP_PROBE_MAP to the absolute diagnostic map path, and optionally
//! VERA20K_LAMP_PROBE_OUTPUT to a PNG output directory. Run:
//! cargo test -p vera20k --lib stock_lamp_loaded_by_scenario_changes_actual_terrain_pixels -- --ignored --nocapture
//! This isolates one terrain tile; full-window draw routing, shroud composition
//! and equivalence to gamemd pixels require separate evidence.
use std::collections::HashSet;
use std::path::PathBuf;

#[test]
#[ignore = "requires stock retail assets, a diagnostic authored lamp map and a GPU"]
fn stock_lamp_loaded_by_scenario_changes_actual_terrain_pixels() {
    use super::lighting::MatchLighting;
    use crate::assets::asset_manager::{AssetManager, MediaArchiveMode};
    use crate::map::lighting::{CellLightGrid, parse_lighting};
    use crate::map::terrain::{TerrainCell, TerrainGrid, TilePlacement};
    use crate::map::theater::{TileKey, load_theater, load_tile_images};

    let root = PathBuf::from(std::env::var_os("RA2_DIR").expect("retail root"));
    let map = std::env::var("VERA20K_LAMP_PROBE_MAP").expect("authored diagnostic map");
    let mut scenario = crate::headless_scenario::load(&root, &map, 0x420)
        .expect("normal scenario construction with stock rules/art/terrain");
    let lamp = scenario
        .sim()
        .entities()
        .values()
        .find(|entity| {
            scenario
                .sim()
                .interner
                .resolve(entity.type_ref())
                .eq_ignore_ascii_case("GALITE")
                && (entity.position.rx, entity.position.ry) == (53, 113)
        })
        .expect("authored stock GALITE must survive normal scenario construction");
    let (id, rx, ry) = (lamp.stable_id(), lamp.position.rx, lamp.position.ry);
    assert!(
        lamp.lifecycle.object_alive() && !lamp.lifecycle.in_limbo() && lamp.lifecycle.cell_marked
    );
    let rules = &scenario.runtime.resources.rules;
    let stock = rules.object("GALITE").expect("stock GALITE rules");
    assert_eq!(stock.light_visibility, 5000);
    assert!((stock.light_intensity - 0.2).abs() < 0.00001);
    // Headless construction retains the actual resolved world but omits the
    // app-only immutable template. Snapshot that same terrain for this view.
    let terrain_snapshot = scenario.sim().resolved_terrain.as_ref().unwrap().clone();
    let terrain = &terrain_snapshot;
    let cell = terrain.cell(rx, ry).expect("lamp has real terrain");
    let (tile_id, sub_tile) = terrain.presentation_tile(cell);
    let key = TileKey {
        tile_id,
        sub_tile,
        variant: 0,
    };
    let mut assets = AssetManager::new(&root, MediaArchiveMode::STOCK_DIGITAL).unwrap();
    let theater = load_theater(&mut assets, &scenario.map.header.theater).unwrap();
    let tile = load_tile_images(
        &assets,
        &theater.lookup,
        &theater.iso_palette,
        &HashSet::from([key]),
    )
    .remove(&key)
    .expect("actual map TMP decodes");
    let placement = TilePlacement {
        uv_origin: [0.0; 2],
        uv_size: [1.0; 2],
        pixel_size: [tile.width as f32, tile.height as f32],
        draw_offset: [tile.offset_x as f32, tile.offset_y as f32],
    };
    let grid = TerrainGrid {
        cells: vec![TerrainCell {
            screen_x: -(tile.offset_x as f32),
            screen_y: -(tile.offset_y as f32),
            tile_id,
            sub_tile,
            z: cell.level,
            rx,
            ry,
            is_water: false,
            variant: 0,
            tint: [1.0; 3],
            radar_left: [0; 3],
            radar_right: [0; 3],
            has_damaged_data: false,
        }],
        world_width: tile.width as f32,
        world_height: tile.height as f32,
        origin_x: 0.0,
        origin_y: 0.0,
        local_bounds: None,
        bridge_middle_tiles: None,
    };
    let mut lighting = MatchLighting::default();
    lighting.install(
        CellLightGrid::new(),
        parse_lighting(&scenario.map.ini),
        2,
        Some((terrain, &scenario.runtime.simulation, rules)),
    );
    let lit_scalar = lighting
        .grid()
        .cell_light_at((rx, ry))
        .unwrap()
        .common_scalar;
    let lit_additive = lighting
        .grid()
        .cell_light_at((rx, ry))
        .unwrap()
        .raw_additive_intensity;
    assert!(
        lit_additive > 0,
        "stock lamp must reach the live lighting owner"
    );
    let lookup = |_, _, _| Some(placement);
    let instance = crate::render::terrain_instances::build_visible_instances(
        &grid,
        Some(lighting.grid()),
        0.0,
        0.0,
        tile.width as f32,
        tile.height as f32,
        Some(&lookup),
        None,
    )
    .normal
    .remove(0);
    let lit_pixels = crate::render::depth_gpu_tests::render_terrain_lighting_probe(&tile, instance);

    scenario.runtime.simulation.discard_lighting_events();
    scenario.runtime.simulation.apply_fatal_lifecycle_stage(
        rules,
        crate::sim::combat::FatalLifecycleStage::BeforeDeathEffects,
        id,
        crate::map::entities::EntityCategory::Structure,
        crate::sim::world::UninitContext::with_rules(rules),
    );
    let events = std::mem::take(&mut scenario.runtime.simulation.lighting_sources.pending);
    lighting.apply_events(terrain, &events);
    lighting.refresh(terrain, &scenario.runtime.simulation, rules, 2);
    let dark_scalar = lighting
        .grid()
        .cell_light_at((rx, ry))
        .unwrap()
        .common_scalar;
    assert_eq!(
        lighting
            .grid()
            .cell_light_at((rx, ry))
            .unwrap()
            .raw_additive_intensity,
        0
    );
    let instance = crate::render::terrain_instances::build_visible_instances(
        &grid,
        Some(lighting.grid()),
        0.0,
        0.0,
        tile.width as f32,
        tile.height as f32,
        Some(&lookup),
        None,
    )
    .normal
    .remove(0);
    let dark_pixels =
        crate::render::depth_gpu_tests::render_terrain_lighting_probe(&tile, instance);
    let changed = lit_pixels
        .iter()
        .zip(&dark_pixels)
        .filter(|(a, b)| a != b)
        .count();
    let brightness = |pixels: &[[u8; 4]]| {
        pixels
            .iter()
            .map(|p| u64::from(p[0]) + u64::from(p[1]) + u64::from(p[2]))
            .sum::<u64>()
    };
    assert!(
        changed > 0,
        "lamp changed state but never reached GPU pixels"
    );
    assert!(
        brightness(&lit_pixels) > brightness(&dark_pixels),
        "positive stock lamp must brighten actual terrain"
    );
    eprintln!(
        "GALITE({rx},{ry}), stock tile {tile_id}/{sub_tile}, scalar {lit_scalar}->{dark_scalar}, additive {lit_additive}->0, GPU changed {changed}/{} pixels",
        lit_pixels.len()
    );
    if let Some(out) = std::env::var_os("VERA20K_LAMP_PROBE_OUTPUT") {
        let out = PathBuf::from(out);
        std::fs::create_dir_all(&out).unwrap();
        for (name, pixels) in [
            ("lamp-present.png", &lit_pixels),
            ("lamp-removed.png", &dark_pixels),
        ] {
            let bytes: Vec<u8> = pixels
                .iter()
                .flat_map(|pixel| pixel.iter().copied())
                .collect();
            image::save_buffer(
                out.join(name),
                &bytes,
                tile.width,
                tile.height,
                image::ColorType::Rgba8,
            )
            .unwrap();
        }
    }
}
