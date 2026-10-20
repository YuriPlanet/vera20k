//! Physical GI atlas/frame/material/replay against original518F90 body pixels.
//! The viewport controls join Object5F4B10 and its original camera projection;
//! Rust derives their positions through the production screen-position owner.
//! Pose, Cell light and final shape Z remain declared native priors. CAOS
//! gameplay and the full builder have separate production controls.

use super::super::draw_plan_lowering::{
    NativeDisplayOrder, ObjectPieceInstance, PlannedObjectInstance, lower_object_instances,
};
use super::*;
use crate::app::presentation::lighting::{
    body_extra_light, body_palette_light, shp_colour_word, techno_effect_light,
};
use crate::assets::{asset_manager::AssetManager, pal_file::Palette, shp_file::ShpFile};
use crate::map::entities::EntityCategory;
use crate::map::houses::HouseColorMap;
use crate::map::lighting::CellLightGrid;
use crate::render::batch::{InstanceBufferPool, SpriteInstance};
use crate::render::native_z::{ZGradient, pack_z_gradient, stored_z};
use crate::render::shroud_buffer::ShroudBuffer;
use crate::render::sprite_atlas::{ShpPaletteContext, ShpSpriteKey, build_sprite_atlas};
use crate::render::tactical_draw_plan::SpriteEncoding;
use crate::render::terrain_draw::TerrainDrawRenderer;
use crate::render::terrain_draw_gpu_tests::{Gpu, camera, clear, encoded};
use crate::rules::{art_data::ArtRegistry, house_colors::HouseColorIndex, ini_parser::IniFile};
use crate::sim::animation;
use crate::sim::combat::Health;
use crate::sim::components::DriveCoord;
use crate::sim::game_entity::GameEntity;
use crate::sim::movement::FacingClass;
use crate::sim::world::{Simulation, display_layers::DisplayLayer};
use serde_json::Value;
use std::collections::HashSet;

fn int(value: &Value) -> i32 {
    i32::try_from(value.as_i64().unwrap()).unwrap()
}

fn decode(value: &Value) -> Vec<u8> {
    let hex = value.as_str().unwrap();
    assert_eq!(hex.len() % 2, 0);
    (0..hex.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&hex[at..at + 2], 16).unwrap())
        .collect()
}

fn physical_rules(assets: &AssetManager, shared: &Value) -> crate::rules::ruleset::RuleSet {
    let original = IniFile::from_bytes(assets.get_ref("RULESMD.INI").unwrap()).unwrap();
    let art = IniFile::from_bytes(assets.get_ref("ARTMD.INI").unwrap()).unwrap();
    for (source, sections) in [
        (&original, &shared["rule_sections"]),
        (&art, &shared["art_sections"]),
    ] {
        for (name, keys) in sections.as_object().unwrap() {
            let section = source.section(name).unwrap();
            for (key, value) in keys.as_object().unwrap() {
                assert_eq!(section.get_for_test(key), value.as_str(), "{name}.{key}");
            }
        }
    }
    // Match the native selected E1/AudioVisual input subset, retaining physical
    // source order for ColorAdd. Colors keeps actual named scheme identity;
    // the fixture's selected-name local index1 is not a world/GPU scheme row.
    let mut text = String::from("[InfantryTypes]\n0=E1\n");
    for name in ["Colors", "ColorAdd", "AudioVisual", "E1"] {
        text.push_str(&format!("[{name}]\n"));
        let section = original.section(name).unwrap();
        for key in section.keys() {
            if name == "Colors" || shared["rule_sections"][name].get(key).is_some() {
                text.push_str(&format!("{key}={}\n", section.get_for_test(key).unwrap()));
            }
        }
    }
    let mut rules = crate::rules::ruleset::RuleSet::from_ini_with_fixed_art_for_test(
        &IniFile::from_str(&text),
        &art,
    )
    .unwrap();
    rules.install_art_data(ArtRegistry::from_ini(&art));
    // Match map loading's data-only fixed-ART sequence binding before the
    // atlas resolves real Infantry action records and all facing frames.
    rules.bind_animation_sequences(
        &crate::rules::infantry_sequence::parse_infantry_sequence_registry(&art),
    );
    let kind = rules.object("E1").unwrap();
    assert_eq!(kind.image, shared["native_type"]["image"].as_str().unwrap());
    assert_eq!(kind.strength, int(&shared["native_type"]["strength"]));
    assert_eq!(
        rules.general.extra_infantry_light,
        int(&shared["native_rules"]["extra_infantry_light"])
    );
    assert_eq!(
        rules.general.berserk_color,
        int(&shared["native_rules"]["berserk_color"])
    );
    rules
}

#[test]
#[ignore = "requires GPU and retail assets; physical GI atlas and retained material versus eight original whole-body draws"]
fn retail_gi_berserk_atlas_and_replay_match_original_body_pixels() {
    compare_original_body_pixels(false);
}

#[test]
#[ignore = "requires GPU and retail assets; physical GI camera projection and clipping versus original Object DrawIfVisible and body pixels"]
fn retail_gi_viewport_camera_and_replay_match_original_admission_pixels() {
    compare_original_body_pixels(true);
}

fn compare_original_body_pixels(join_viewport: bool) {
    let Some((_, mut assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    // Use the existing theater owner to admit the physical CLEAR01 input.
    // Final shape Z remains an observed native prior for this flat crop.
    crate::map::theater::load_theater(&mut assets, "TEMPERATE").unwrap();
    let native =
        crate::test_fixture::json("tools/procedural_drawing_oracle/infantry_berserk_body.json");
    let shared = &native["shared"];
    for asset in shared["assets"].as_array().unwrap() {
        let name = asset["name"].as_str().unwrap();
        let raw = assets
            .get_ref(name)
            .unwrap_or_else(|| panic!("physical {name}"));
        assert_eq!(raw.len() as u64, asset["bytes"], "{name}");
        assert_eq!(
            crate::util::sha256::sha256_hex(raw),
            asset["sha256"],
            "{name}"
        );
    }
    let rules = physical_rules(&assets, shared);
    let source = ShpFile::from_bytes(assets.get_ref("GI.SHP").unwrap()).unwrap();
    let palette = Palette::from_bytes(assets.get_ref("UNITTEM.PAL").unwrap()).unwrap();
    let scheme = &shared["colour_scheme"];
    let scheme_index = rules
        .color_schemes
        .iter()
        .position(|entry| Some(entry.name.as_str()) == scheme["name"].as_str())
        .unwrap();
    assert_eq!(
        serde_json::json!(rules.color_schemes[scheme_index].hsv),
        scheme["hsv"]
    );
    let house_color = HouseColorIndex(u8::try_from(scheme_index).unwrap());
    let house_colors = HouseColorMap::from([("PreparedHouse".to_string(), house_color)]);
    for frame in shared["frames"].as_array().unwrap() {
        let actual = &source.frames[int(&frame["frame"]) as usize];
        assert_eq!(
            serde_json::json!([
                actual.frame_x,
                actual.frame_y,
                actual.frame_width,
                actual.frame_height
            ]),
            frame["native_rect"]
        );
        assert_eq!(
            actual.format & 2 != 0,
            frame["compressed"].as_bool().unwrap()
        );
    }

    let mut sim = Simulation::with_seed(31);
    let owner = sim.intern("PreparedHouse");
    let type_id = sim.intern("E1");
    let actor = &shared["actor"];
    let mut gi = GameEntity::new_at_frame_zero_for_test(
        1,
        10,
        20,
        0,
        0,
        owner,
        Health::new(int(&actor["health"])),
        type_id,
        EntityCategory::Infantry,
        0,
        5,
        false,
    );
    gi.position.put_location(DriveCoord {
        x: int(&actor["xyz"][0]),
        y: int(&actor["xyz"][1]),
        z: int(&actor["xyz"][2]),
    });
    gi.lifecycle
        .set_in_limbo_for_test(actor["limbo"].as_bool().unwrap());
    gi.lifecycle
        .set_object_alive_for_test(actor["alive"].as_bool().unwrap());
    sim.entities_mut_for_test().insert(gi);
    sim.submit_object_display_for_test(1, DisplayLayer::GROUND, Some(&rules));
    let order = NativeDisplayOrder::from_display(sim.display_layers());
    let mut light_grid = CellLightGrid::new();
    light_grid.insert_profiled_light(
        (10, 20),
        [1.; 3],
        int(&shared["cells"]["top_light"]) as f32 / 1000.,
    );
    let surface = &shared["surface"];
    let size = ["width", "height"].map(|key| int(&surface[key]) as u32);
    let pixels = (size[0] * size[1]) as usize;
    let a_value = int(&surface["a_value"]) as u8;
    let background = int(&surface["background"]) as u16;
    let rows = native[if join_viewport {
        "viewport_rows"
    } else {
        "rows"
    }]
    .as_array()
    .unwrap();
    assert_eq!(rows.len(), if join_viewport { 28 } else { 8 });
    let gpu = Gpu::new();
    let mut compared = 0;
    for format in [
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ] {
        let mut batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
        let mut view = camera(size);
        view.native_z_origin_y = int(&surface["depth_baseline"]) as f32 - 32768.;
        batch.write_camera(&gpu.queue, view);
        let atlas = build_sprite_atlas(
            &gpu.device,
            &gpu.queue,
            &batch,
            sim.entities(),
            &assets,
            &palette,
            "tem",
            "TEMPERATE",
            Some(&rules),
            Some(rules.art()),
            &house_colors,
            &[],
            &HashSet::new(),
            &HashSet::new(),
            None,
            None,
            Some(sim.interner()),
        )
        .expect("physical E1/GI sprite atlas");
        let fog = ShroudBuffer::fixture(
            &gpu.device,
            &gpu.queue,
            size,
            [0.; 2],
            &vec![a_value; pixels],
        );
        batch.bind_shroud(&gpu.device, Some(&fog));
        for row in rows {
            let name = row["name"].as_str().unwrap();
            let input = &row["input"];
            // DrawIfVisible may return before the body. Its return value never
            // gates the Rust draw: a conservatively submitted outside sprite
            // must still produce the same final pixels. Use the original
            // ordinary body of this physical frame for its flat material/Z.
            let body = if row["body"].is_null() {
                native["rows"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|control| {
                        control["input"]["frame"] == input["frame"]
                            && control["name"].as_str().unwrap().ends_with("_ordinary")
                    })
                    .map(|control| &control["body"])
                    .unwrap()
            } else {
                &row["body"]
            };
            for key in [
                "source_unchanged",
                "a_unchanged",
                "z_unchanged",
                "original_code_unchanged",
            ] {
                assert_eq!(row["invariants"][key], true, "{name}: native {key}");
            }
            for stream in ["main", "scenario", "mapgen"] {
                assert_eq!(row["invariants"]["rng_unchanged"][stream], true);
            }
            assert!(
                row["invariants"]["rng_requests"]
                    .as_array()
                    .unwrap()
                    .is_empty()
            );
            if join_viewport {
                assert_eq!(row["invariants"]["vtable_unchanged"], true);
                assert_eq!(row["invariants"]["facing_table_unchanged"], true);
                // Native Tactical and VERA retain different world-row origins.
                // Use the same origin query as the production camera owner;
                // the observed native actor point never positions this draw.
                let origin = crate::util::lepton::absolute_leptons_to_screen(0, 0, 0);
                view.camera_pos = [
                    int(&input["camera"][0]) as f32 + origin.0,
                    int(&input["camera"][1]) as f32 + origin.1,
                ];
                batch.write_camera(&gpu.queue, view);
            }
            let gi = sim.entities_mut_for_test().get_mut(1).unwrap();
            gi.berserk
                .set_active_for_test(input["berserk"].as_bool().unwrap());
            gi.body_facing = FacingClass::new(int(&input["facing"]) as u16, 0);
            let gi = sim.entities().get(1).unwrap();
            let before = sim.state_hash();
            let record = rules
                .animation_sequence("E1")
                .unwrap()
                .infantry_action(int(&actor["doing"]))
                .unwrap();
            let frame = u16::try_from(animation::resolve_shp_frame(
                record,
                gi.body_facing_byte(sim.session().binary_frame()),
                int(&actor["stage"]),
            ))
            .unwrap();
            assert_eq!(
                i32::from(frame),
                int(&input["frame"]),
                "{name}: original518D80"
            );
            let entry = atlas
                .get(&ShpSpriteKey {
                    palette_context: ShpPaletteContext::Legacy,
                    type_id: "E1".to_string(),
                    facing: 0,
                    frame,
                    house_color,
                })
                .expect("actual sequence frame in production atlas");
            let parent = order.object_draw(1, SpriteEncoding::Plain).unwrap();
            assert_eq!(parent.policy.render_z, RenderZPolicy::ReadOnly);
            let extra = body_extra_light(gi, &rules, &sim, None);
            let light = body_palette_light(&light_grid, (10, 20), extra);
            let (tint, light) = techno_effect_light(gi, [1.; 3], light, &sim);
            let word = shp_colour_word(gi, &sim, &rules, || input["shrouded"].as_bool().unwrap());
            let light = light.with_colour_word(if parent.policy.ors_colour_word(entry.extended) {
                word
            } else {
                0
            });
            assert_eq!(
                light.brightness(),
                int(&body["intensity"]),
                "{name}: class/shared intensity"
            );
            assert_eq!(
                i32::from(light.colour_word()),
                int(&body["colour_word"]),
                "{name}: original caller word"
            );
            let anchor = if join_viewport {
                let (x, y) = crate::render::locomotor_visual::screen_position(gi);
                if let Some(point) = row["admission"]["point"].as_array() {
                    assert_eq!(
                        [x - view.camera_pos[0], y - view.camera_pos[1]],
                        [int(&point[0]) as f32, int(&point[1]) as f32],
                        "{name}: production projection versus original41BE00/6D2140"
                    );
                }
                [x, y]
            } else {
                [int(&body["point"][0]) as f32, int(&body["point"][1]) as f32]
            };
            let position = [anchor[0] + entry.offset_x, anchor[1] + entry.offset_y];
            let gradient = pack_z_gradient(ZGradient::Vertical, false);
            assert_eq!(gradient, int(&body["gradient"]) as u32);
            let layers = lower_object_instances(vec![PlannedObjectInstance::object(
                parent,
                vec![ObjectPieceInstance {
                    target: ObjectTexture::ShpPage(entry.page as usize),
                    render_z: parent.policy.render_z,
                    instance: SpriteInstance {
                        position,
                        size: entry.pixel_size,
                        uv_origin: entry.uv_origin,
                        uv_size: entry.uv_size,
                        source_palette: entry.source_palette,
                        tint,
                        alpha: 1.,
                        palette_light: light,
                        // This is the observed final shape input, not a new
                        // derivation of Foot GetZAdjust or map/TMP state.
                        z_adjust: int(&body["z_adjust"]) as f32,
                        z_gradient: gradient,
                        ..Default::default()
                    },
                }],
            )]);
            let layer = &layers[2];
            assert_eq!(layer.owners, [1]);
            let mut pool = InstanceBufferPool::new();
            pool.upload_on_device(&gpu.device, &gpu.queue, "gi_native_body", &layer.instances);
            let color = gpu.target(size, format);
            gpu.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &color,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &encoded(background, format).repeat(pixels),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(size[0] * 4),
                    rows_per_image: Some(size[1]),
                },
                color.size(),
            );
            let cv = color.create_view(&Default::default());
            let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
            let dv = depth.create_view(&Default::default());
            let mut terrain = TerrainDrawRenderer::new(&gpu.device, &gpu.queue, format, &batch);
            terrain.prepare(&gpu.device, &color, &dv, batch.camera_uniform());
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            let old_z = int(&input["old_z"]) as u16;
            clear(&mut encoder, &cv, &dv, old_z, wgpu::LoadOp::Load);
            let clip = if join_viewport {
                let caller_clip = [0, 0, size[0], size[1]];
                assert_eq!(input["caller_clip"], serde_json::json!(caller_clip));
                // The existing pass owns target/scissor clipping. Native's
                // mutated clip is an observation, never a Rust draw input.
                caller_clip
            } else {
                std::array::from_fn(|i| int(&body["clip"][i]) as u32)
            };
            draw_native_object_pass(
                &mut encoder,
                &cv,
                &dv,
                &mut terrain,
                clip,
                &batch,
                pool.get("gi_native_body"),
                layer,
                None,
                None,
                None,
                Some(&atlas),
                None,
                batch.default_zshape_bind_group(),
                None,
                None,
            );
            let reads = [
                gpu.read(&mut encoder, &color),
                gpu.read(&mut encoder, &depth),
            ];
            let output = gpu.finish(encoder, &reads, size);
            // Actual native shadow calls372/373 see empty physical rectangles.
            // Comparing both observations establishes no positive shadow stores.
            let shadow_frame = row["shadow"]["frame"]
                .as_u64()
                .unwrap_or(u64::from(frame) + 372);
            let shadow = &source.frames[shadow_frame as usize];
            assert_eq!((shadow.frame_width, shadow.frame_height), (0, 0));
            let final_pixels = &row["final_rgb565_le_hex"];
            let stages = if join_viewport {
                vec![("final", final_pixels)]
            } else {
                vec![("body", &body["rgb565_le_hex"]), ("final", final_pixels)]
            };
            for (stage, expected) in stages {
                let expected = decode(expected);
                assert_eq!(expected.len(), pixels * 2);
                for (at, pixel) in output[0].as_chunks::<4>().0.iter().enumerate() {
                    let word = u16::from_le_bytes(expected[at * 2..at * 2 + 2].try_into().unwrap());
                    assert_eq!(
                        *pixel,
                        encoded(word, format),
                        "{name} {stage} {format:?}: native pixel{at}"
                    );
                }
            }
            let frame = &source.frames[usize::from(frame)];
            let mut holes = 0;
            for (at, &index) in frame.pixels.iter().enumerate() {
                if index == 0 {
                    let x = (position[0] - view.camera_pos[0]) as i32
                        + (at % usize::from(frame.frame_width)) as i32;
                    let y = (position[1] - view.camera_pos[1]) as i32
                        + (at / usize::from(frame.frame_width)) as i32;
                    if x < 0 || y < 0 || x >= size[0] as i32 || y >= size[1] as i32 {
                        continue;
                    }
                    let (x, y) = (x as usize, y as usize);
                    assert_eq!(
                        &output[0][(y * size[0] as usize + x) * 4..][..4],
                        &encoded(background, format),
                        "{name}: physical source hole{x},{y}"
                    );
                    holes += 1;
                }
            }
            if !join_viewport {
                assert!(holes > 0);
            }
            for pixel in output[1].as_chunks::<4>().0 {
                assert_eq!(
                    stored_z(f32::from_le_bytes(*pixel)),
                    old_z,
                    "{name}: Z unchanged"
                );
            }
            for actual in gpu.read_float_texels(fog.gpu_source().0, [0, 0], size) {
                assert_eq!(
                    (actual[0] * 255.).round() as u8,
                    a_value,
                    "{name}: A unchanged"
                );
            }
            assert_eq!(
                sim.state_hash(),
                before,
                "{name}: presentation leaves simulation unchanged"
            );
            compared += 1;
        }
    }
    assert_eq!(compared, rows.len() * 2);
}
