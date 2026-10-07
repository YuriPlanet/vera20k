//! Physical AEGIS type -> production model/raster -> parent waterline input.
use super::*;
use crate::render::sinking::SinkingWaterlines;
use crate::util::sha256::sha256_hex;

fn admission_corpus() -> serde_json::Value {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/naval_sinking_clip.json",
    ))
    .unwrap();
    corpus["admission"].clone()
}

fn native_draw_entity(input: &serde_json::Value) -> crate::sim::game_entity::GameEntity {
    let xyz: [i32; 3] = std::array::from_fn(|i| input["xyz"][i].as_i64().unwrap() as i32);
    let mut entity = crate::sim::game_entity::GameEntity::test_default(
        1,
        "AEGIS",
        "Americans",
        (xyz[0] / 256) as u16,
        (xyz[1] / 256) as u16,
    );
    entity.category = EntityCategory::Unit;
    entity.position.sub_x = crate::util::fixed_math::SimFixed::from_num(xyz[0] % 256);
    entity.position.sub_y = crate::util::fixed_math::SimFixed::from_num(xyz[1] % 256);
    entity.position.exact_z_leptons = Some(xyz[2]);
    entity
}

fn native_camera(input: &serde_json::Value) -> [f32; 2] {
    // VERA's common world-pixel frame adds15 to both object and camera Y.
    [
        input["camera"][0].as_i64().unwrap() as f32,
        input["camera"][1].as_i64().unwrap() as f32 + TILE_HEIGHT / 2.0,
    ]
}

#[test]
fn sinking_unit_draw_admission_matches_original_projection_boundaries() {
    let corpus = admission_corpus();
    let rows = corpus["cases"].as_array().unwrap();
    assert_eq!(rows.len(), 15);
    for row in rows {
        let input = &row["input"];
        let entity = native_draw_entity(input);
        let viewport = std::array::from_fn(|i| input["viewport"][i].as_i64().unwrap() as f32);
        let camera = native_camera(input);
        let anchor = unit_draw_anchor(&entity, camera, viewport);
        assert_eq!(
            anchor.is_some(),
            row["output"]["admitted"].as_bool().unwrap(),
            "{row}"
        );
        if let Some(anchor) = anchor {
            assert_eq!(
                [anchor[0] - camera[0], anchor[1] - camera[1]],
                std::array::from_fn(|i| row["output"]["body_anchor"][i].as_i64().unwrap() as f32)
            );
        }
    }
}

#[test]
fn offscreen_first_sinking_draw_survives_descent_camera_pan_and_save_load() {
    use crate::sim::snapshot::GameSnapshot;
    let corpus = admission_corpus();
    let raster: [i32; 4] =
        std::array::from_fn(|i| corpus["aegis_cached_dirty"][i].as_i64().unwrap() as i32);
    let entry = UnitSpriteEntry {
        uv_origin: [0.0; 2],
        uv_size: [1.0; 2],
        pixel_size: [raster[2] as f32, raster[3] as f32],
        offset_x: (raster[0] - 128) as f32,
        offset_y: (raster[1] - 128) as f32,
        page: 0,
        native_draw_bounds: Some([raster[0] - 128, raster[1] - 128, raster[2], raster[3]]),
    };
    let mut cache = SinkingWaterlines::default();
    let mut sim = crate::sim::world::Simulation::new();
    sim.session.map_name = "DRAW-ADMISSION.MAP".into();
    sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let hash = sim.state_hash();
    for step in corpus["sequence"].as_array().unwrap() {
        let input = &step["input"];
        let entity = native_draw_entity(input);
        let viewport = std::array::from_fn(|i| input["viewport"][i].as_i64().unwrap() as f32);
        let anchor = unit_draw_anchor(&entity, native_camera(input), viewport)
            .expect("original Unit DrawIt admits this offscreen or camera-panned anchor");
        let bounds = composite_draw_bounds([(entry, anchor)]);
        let clip = bounds.unit_waterline(&mut cache, entity.stable_id(), true);
        let expected = step["output"]["waterline_after"].as_i64().unwrap() as i16 + 15;
        assert_eq!(cache.saved(), [(entity.stable_id(), expected)]);
        let native_clips = !step["output"]["calls"].as_array().unwrap().is_empty();
        assert_eq!(clip, native_clips.then_some(expected));
        let mut draw = DrawState::default();
        crate::render::sinking::apply_waterline_clip(&mut draw, clip);
        assert_eq!(
            draw.fx_flags & crate::render::draw_state::FX_SINKING_CLIP != 0,
            native_clips
        );
        if native_clips {
            assert_eq!(
                draw.effect_tint[3] - native_camera(input)[1],
                step["output"]["clip"][3].as_i64().unwrap() as f32
            );
        }
        // Use the app's actual neutral snapshot supplement between draws.
        let bytes = GameSnapshot::save_validated_with_sinking_waterlines(
            &sim,
            1,
            2,
            "offscreen sinking",
            3,
            &cache.saved(),
        );
        let loaded = GameSnapshot::load_validated(&bytes, 1, 2, "DRAW-ADMISSION.MAP").unwrap();
        assert_eq!(loaded.sim.state_hash(), hash);
        cache.clear();
        cache.restore(loaded.sinking_waterlines);
    }
}

#[test]
#[ignore = "requires physical Shrapnel map, retail AEGIS/HVA/VPL and original draw observations"]
fn retail_aegis_raster_and_parent_waterline_match_original_ship_draw() {
    let retail = std::path::PathBuf::from(std::env::var_os("RA2_DIR").unwrap());
    let map = std::env::var("VERA20K_SHRAPNEL_MAP").unwrap_or_else(|_| "XShrapnel.MAP".into());
    let scene = crate::headless_scenario::load(&retail, &map, 0x0B21_D6E5).unwrap();
    let rules = &scene.runtime.resources.rules;
    let assets = crate::assets::asset_manager::AssetManager::new(
        &retail,
        crate::assets::asset_manager::MediaArchiveMode::STOCK_DIGITAL,
    )
    .unwrap();
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/naval_draw_bounds.json",
    ))
    .unwrap();
    for (name, provenance) in corpus["files"].as_object().unwrap() {
        let bytes = assets.get_ref(name).unwrap();
        assert_eq!(
            bytes.len() as u64,
            provenance["bytes"].as_u64().unwrap(),
            "{name}"
        );
        assert_eq!(
            sha256_hex(bytes),
            provenance["sha256"].as_str().unwrap(),
            "{name}"
        );
    }
    let object = rules.object("AEGIS").unwrap();
    assert_eq!(
        object.image,
        corpus["reader"]["layers"][2]["after"].as_str().unwrap()
    );
    assert!(rules.art().get(&object.image).unwrap().voxel);
    assert!(!object.has_turret);
    let model = crate::render::unit_atlas::UnitModel::load(&assets, "AEGIS", Some(rules)).unwrap();
    let vpl = crate::assets::vpl_file::VplFile::from_bytes(assets.get_ref("VOXELS.VPL").unwrap())
        .unwrap();
    for row in corpus["cases"].as_array().unwrap() {
        let facing = row["input"]["facing_step32"].as_u64().unwrap() as u8 * 8;
        let key = UnitSpriteKey {
            type_id: "AEGIS".into(),
            turret_index: 0,
            facing,
            layer: VxlLayer::Composite,
            frame: 0,
            slope_type: 0,
            barrel_pitch: 0,
        };
        let (sprite, native_draw_bounds) = model.render(&key, Some(&vpl), None, &mut None).unwrap();
        let rect: [i32; 6] =
            std::array::from_fn(|i| row["native_rect"][i].as_i64().unwrap() as i32);
        assert_eq!(
            native_draw_bounds,
            Some([rect[0], rect[1], rect[4], rect[5]]),
            "facing{facing}"
        );
        let mut canonical = vec![0; 65536];
        let left = sprite.offset_x as i32 + rect[2] - rect[0];
        let top = sprite.offset_y as i32 + rect[3] - rect[1];
        for y in 0..sprite.height as i32 {
            for x in 0..sprite.width as i32 {
                let byte = sprite.palette_indices[(y as u32 * sprite.width + x as u32) as usize];
                if byte != 0 {
                    canonical[((top + y) * 256 + left + x) as usize] = byte;
                }
            }
        }
        assert_eq!(
            sha256_hex(&canonical),
            row["pixels_sha256"].as_str().unwrap(),
            "facing{facing} physical raster"
        );
        let entry = UnitSpriteEntry {
            uv_origin: [0.0; 2],
            uv_size: [1.0; 2],
            pixel_size: [sprite.width as f32, sprite.height as f32],
            offset_x: sprite.offset_x,
            offset_y: sprite.offset_y,
            page: 0,
            native_draw_bounds,
        };
        // Original packet supplies camera1000 and draw anchor200. The app
        // composes their world row first; no source-crop Y participates.
        let bounds = composite_draw_bounds([(entry, [0.0, 1200.0])]);
        let [_, y, _, height] = bounds.native.unwrap();
        let mut cache = SinkingWaterlines::default();
        assert_eq!(cache.unit_draw(1, true, y + height), None);
        let expected = row["first_clip"]["waterline_after"].as_i64().unwrap() as i16;
        assert_eq!(cache.saved(), [(1, expected)]);
        assert_eq!(cache.unit_draw(1, true, y + height + 50), Some(expected));
        eprintln!(
            "AEGIS facing{facing} native rectangle {rect:?}, raster bytes match, retained waterline{expected}"
        );
    }
}
