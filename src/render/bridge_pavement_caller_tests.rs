//! The tactical terrain draw reads the live pavement bit.
use crate::map::bridge_pavement::DAMAGED_PAVEMENT;

#[test]
#[ignore = "requires retail assets, VERA20K_XMP34U4_MAP and a GPU"]
fn retail_pavement_live_damage_changes_actual_terrain_pixels() {
    use crate::assets::asset_manager::{AssetManager, MediaArchiveMode};
    use crate::map::terrain::{TerrainCell, TerrainGrid, TilePlacement};
    use crate::map::theater::{TileKey, load_theater, load_tile_images};
    use std::collections::HashSet;
    use std::path::PathBuf;

    let root = PathBuf::from(std::env::var_os("RA2_DIR").expect("retail root"));
    let map = std::env::var("VERA20K_XMP34U4_MAP").expect("retail xmp34u4 map");
    let mut scenario = crate::headless_scenario::load(&root, &map, 0x0B21_D6E5)
        .expect("normal scenario construction with stock rules/art/terrain");
    let (rx, ry) = (66, 102);
    let terrain = scenario.sim().resolved_terrain.as_ref().unwrap();
    let cell = terrain.cell(rx, ry).unwrap();
    let (tile_id, sub_tile) = terrain.presentation_tile(cell);
    assert_eq!((tile_id, sub_tile), (278, 7));
    assert!(!terrain.pavement_damaged_at(rx, ry));
    assert!(!cell.has_bridge_deck && !cell.bridge_facts.has_structural_bridge());
    let pristine = TileKey {
        tile_id,
        sub_tile,
        variant: 0,
    };
    let damaged = TileKey {
        variant: 1,
        ..pristine
    };
    let mut assets = AssetManager::new(&root, MediaArchiveMode::STOCK_DIGITAL).unwrap();
    let theater = load_theater(&mut assets, &scenario.map.header.theater).unwrap();
    let tiles = load_tile_images(
        &assets,
        &theater.lookup,
        &theater.iso_palette,
        &HashSet::from([pristine, damaged]),
    );
    let tile = tiles.get(&pristine).expect("retail pristine TMP");
    let broken = tiles.get(&damaged).expect("retail damaged TMP");
    assert_eq!(
        (tile.width, tile.height, tile.offset_x, tile.offset_y),
        (
            broken.width,
            broken.height,
            broken.offset_x,
            broken.offset_y
        )
    );
    let grid = TerrainGrid {
        cells: vec![TerrainCell {
            screen_x: -(tile.offset_x as f32),
            screen_y: -(tile.offset_y as f32),
            tile_id,
            sub_tile,
            z: cell.level,
            rx,
            ry,
            variant: 0,
            tint: [1.0; 3],
            radar_left: [0; 3],
            radar_right: [0; 3],
        }],
        world_width: tile.width as f32,
        world_height: tile.height as f32,
        origin_x: 0.0,
        origin_y: 0.0,
        bridge_middle_tiles: None,
    };
    let draw = |sim: &crate::sim::world::Simulation, expected: TileKey| {
        let selected = std::cell::RefCell::new(Vec::new());
        let uv = |tile_id, sub_tile, variant| {
            let key = TileKey {
                tile_id,
                sub_tile,
                variant,
            };
            selected.borrow_mut().push(key);
            tiles.get(&key).map(|image| TilePlacement {
                uv_origin: [0.0; 2],
                uv_size: [1.0; 2],
                pixel_size: [image.width as f32, image.height as f32],
                draw_offset: [image.offset_x as f32, image.offset_y as f32],
            })
        };
        let instances = super::build_visible_instances(
            &grid,
            None,
            0.0,
            0.0,
            tile.width as f32,
            tile.height as f32,
            Some(&uv),
            sim.resolved_terrain.as_ref(),
        );
        assert_eq!(*selected.borrow(), [expected]);
        assert_eq!(instances.normal.len(), 1);
        crate::render::depth_gpu_tests::render_terrain_lighting_probe(
            tiles.get(&expected).unwrap(),
            instances.normal[0],
        )
    };
    let before = draw(scenario.sim(), pristine);
    let sim = &mut scenario.runtime.simulation;
    // The live pavement bit (56E990's +140 bit13) on this plain pavement
    // cell, which has no structural runtime entry.
    let set_pavement = |sim: &mut crate::sim::world::Simulation, damaged: bool| {
        let terrain = sim.resolved_terrain.as_mut().unwrap();
        let native = terrain.native_cell_identity((rx as i16, ry as i16));
        let flags = terrain.native_cell_flags(native) & !DAMAGED_PAVEMENT;
        terrain.write_pavement_flags(native, flags | if damaged { DAMAGED_PAVEMENT } else { 0 });
    };
    set_pavement(sim, true);
    let after = draw(sim, damaged);
    assert_eq!(before.len(), after.len());
    let changed = before.iter().zip(&after).filter(|(a, b)| a != b).count();
    assert!(changed > 0, "live damage must reach production GPU pixels");
    for pixels in [&before, &after] {
        assert!(
            pixels.iter().any(|pixel| pixel[..3] != [0, 0, 0]),
            "nonblank output"
        );
    }
    set_pavement(sim, false);
    let restored = draw(sim, pristine);
    assert_eq!(restored, before);
    eprintln!(
        "retail pavement {tile_id}/{sub_tile}: GPU changed {changed}/{} pixels; clearing restores original pixels",
        before.len()
    );
    if let Some(out) = std::env::var_os("VERA20K_PAVEMENT_PROBE_OUTPUT") {
        let out = PathBuf::from(out);
        std::fs::create_dir_all(&out).unwrap();
        for (name, pixels) in [
            ("pristine.png", &before),
            ("damaged.png", &after),
            ("cleared.png", &restored),
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
