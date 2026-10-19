//! Original executed packed-material words through retained production replay.
use super::super::draw_plan_lowering::{
    ObjectPieceInstance, PlannedObjectInstance, lower_object_instances,
};
use super::*;
use crate::render::batch::{InstanceBufferPool, SpriteInstance};
use crate::render::draw_state::{DrawState, FX_CLOAK};
use crate::render::palette_light::PaletteLight;
use crate::render::shroud_buffer::ShroudBuffer;
use crate::render::tactical_draw_plan::{BlitPolicy, ObjectDraw, SpriteEncoding, TacticalLayer};
use crate::render::terrain_draw_gpu_tests::{Gpu, camera, clear, encoded};
use serde_json::Value;

fn native_light(row: &Value) -> PaletteLight {
    let brightness = row["brightness"].as_i64().unwrap() as i32;
    match row["profile"].as_str().unwrap() {
        "plain1" => PaletteLight::plain(1, brightness),
        "plain53" => PaletteLight::plain(53, brightness),
        "light27" => PaletteLight::new([1000; 3], 27, brightness, false),
        "scheme53" => PaletteLight::color_scheme([1000; 3], brightness),
        other => panic!("unknown native palette input {other}"),
    }
}

#[test]
#[ignore = "requires GPU; original packed translucency through actual SHP/VXL replay"]
fn retained_mobile_translucency_matches_original_packed_words() {
    let native: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/procedural_drawing_oracle/translucent_blitter_a.json",
    ))
    .unwrap();
    let gpu = Gpu::new();
    for collection in ["cases", "replay_controls"] {
        // SHP runs once through an RGBA page and once through a palette-indexed
        // atlas page, which must resolve to the same native words.
        for (route, indexed) in [("shp", false), ("shp", true), ("voxel", false)] {
            let mut groups = std::collections::BTreeMap::<[u8; 3], Vec<&Value>>::new();
            for row in native[collection]
                .as_array()
                .unwrap()
                .iter()
                .filter(|row| row["route"] == route)
            {
                let rgb = std::array::from_fn(|i| row["rgb"][i].as_u64().unwrap() as u8);
                groups.entry(rgb).or_default().push(row);
            }
            assert!(!groups.is_empty());
            for (rgb, all_cases) in groups {
                for cases in all_cases.chunks(512) {
                    let size = [8, cases.len() as u32 * 3];
                    // Native inputs contain a guarded nineteen-word destination window.
                    // An eight-word pitch and a three-row block preserve the +/-8 controls
                    // without treating a signed surface-word offset as a horizontal UV.
                    let indices: Vec<u8> = cases
                        .iter()
                        .flat_map(|row| {
                            let index = row["index"].as_u64().unwrap() as u8;
                            let middle = row
                                .get("overlap_sources")
                                .map_or(0, |source| source[1].as_u64().unwrap() as u8);
                            (0..24).map(move |word| match word {
                                10 | 12 => index,
                                11 => middle,
                                _ => 0,
                            })
                        })
                        .collect();
                    let a: Vec<u8> = cases
                        .iter()
                        .flat_map(|row| [row["a"].as_u64().unwrap() as u8; 24])
                        .collect();
                    for format in [
                        wgpu::TextureFormat::Bgra8UnormSrgb,
                        wgpu::TextureFormat::Rgba8UnormSrgb,
                    ] {
                        let mut batch =
                            BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
                        batch.write_camera(&gpu.queue, camera(size));
                        let fog =
                            ShroudBuffer::fixture(&gpu.device, &gpu.queue, size, [0.0; 2], &a);
                        batch.bind_shroud(&gpu.device, Some(&fog));
                        let palette = crate::assets::pal_file::Palette::from_rgb(|_| {
                            [rgb[0], rgb[1], rgb[2]]
                        });
                        let (shp, source_palette) = if indexed {
                            SpriteAtlas::from_test_indexed_page(
                                &gpu.device,
                                &gpu.queue,
                                &batch,
                                size,
                                &indices,
                                &palette,
                            )
                        } else {
                            let rgba: Vec<u8> = indices
                                .iter()
                                .flat_map(|&index| {
                                    [rgb[0], rgb[1], rgb[2], if index == 0 { 0 } else { 255 }]
                                })
                                .collect();
                            let page = crate::render::sprite_atlas::SpriteAtlasPage {
                                texture: batch.create_texture_on_device(
                                    &gpu.device,
                                    &gpu.queue,
                                    &rgba,
                                    size[0],
                                    size[1],
                                    Some(&indices),
                                ),
                            };
                            (SpriteAtlas::from_test_pages(vec![page]), 0)
                        };
                        let voxel =
                            crate::render::unit_pose_cache::VxlPoseFrameCache::from_test_textures(
                                vec![batch.create_unit_atlas_texture_on_device(
                                    &gpu.device,
                                    &gpu.queue,
                                    size[0],
                                    size[1],
                                    &indices,
                                )],
                            );
                        let palettes = PaletteSet::new_on_device(
                            &gpu.device,
                            &gpu.queue,
                            &palette,
                            &palette,
                            &crate::rules::house_colors::HouseColorRamps::from_schemes(&[]),
                            &[],
                        );
                        let parents = cases
                            .iter()
                            .enumerate()
                            .flat_map(|(y, row)| {
                                let bits = row["selector_bits"].as_u64().unwrap();
                                let opacity = match bits & 6 {
                                    2 => 0.75,
                                    4 => 0.5,
                                    6 => 0.25,
                                    _ => unreachable!(),
                                };
                                let mut parent = PlannedObjectInstance::object(
                                    ObjectDraw {
                                        id: y as u64,
                                        layer: TacticalLayer(3),
                                        display_order: y as u64,
                                        policy: BlitPolicy::z_read(SpriteEncoding::Plain),
                                    },
                                    vec![ObjectPieceInstance {
                                        target: if route == "shp" {
                                            ObjectTexture::ShpPage(0)
                                        } else {
                                            ObjectTexture::UnitPosePage(0)
                                        },
                                        render_z: if row["writes_depth"] == true {
                                            RenderZPolicy::ReadWrite
                                        } else {
                                            RenderZPolicy::ReadOnly
                                        },
                                        instance: SpriteInstance {
                                            position: [2.0, (y * 3 + 1) as f32],
                                            size: [3.0, 1.0],
                                            uv_origin: [0.25, (y * 3 + 1) as f32 / size[1] as f32],
                                            uv_size: [3.0 / 8.0, 1.0 / size[1] as f32],
                                            tint: [1.0; 3],
                                            alpha: 1.0,
                                            z_adjust: (4096 - (32768 - (y * 3 + 1) as i32)) as f32,
                                            palette_light: native_light(row),
                                            source_palette,
                                            draw_state: DrawState {
                                                fx_flags: FX_CLOAK,
                                                native_offset_words: row["displacement"]
                                                    .as_i64()
                                                    .unwrap()
                                                    as i32,
                                                fx_params: [opacity, bits as f32, 1.0, 0.0],
                                                ..Default::default()
                                            },
                                            ..Default::default()
                                        },
                                    }],
                                );
                                if route == "voxel" && row.get("overlap_sources").is_some() {
                                    let piece = &parent.pieces[0];
                                    parent.pieces.push(ObjectPieceInstance {
                                        target: piece.target,
                                        render_z: piece.render_z,
                                        instance: piece.instance,
                                    });
                                }
                                let replays = row["replay_count"].as_u64().unwrap_or(1) as usize;
                                (0..replays)
                                    .map(|repeat| {
                                        PlannedObjectInstance::object(
                                            ObjectDraw {
                                                id: (y * 4 + repeat) as u64,
                                                layer: parent.parent.layer,
                                                display_order: (y * 4 + repeat) as u64,
                                                policy: parent.parent.policy,
                                            },
                                            parent
                                                .pieces
                                                .iter()
                                                .map(|piece| ObjectPieceInstance {
                                                    target: piece.target,
                                                    render_z: piece.render_z,
                                                    instance: piece.instance,
                                                })
                                                .collect(),
                                        )
                                    })
                                    .collect::<Vec<_>>()
                            })
                            .collect();
                        let layers = lower_object_instances(parents);
                        let layer = &layers[3];
                        let mut pool = InstanceBufferPool::new();
                        pool.upload_on_device(
                            &gpu.device,
                            &gpu.queue,
                            "packed_native",
                            &layer.instances,
                        );
                        let white = batch.create_texture_on_device(
                            &gpu.device,
                            &gpu.queue,
                            &[255; 4],
                            1,
                            1,
                            Some(&[1]),
                        );
                        let seeds: Vec<_> = cases
                            .iter()
                            .enumerate()
                            .flat_map(|(y, row)| {
                                row["prior_depths"]
                                    .as_array()
                                    .unwrap()
                                    .iter()
                                    .enumerate()
                                    .filter(|(_, depth)| depth.as_u64().unwrap() < 65535)
                                    .map(move |(x, depth)| SpriteInstance {
                                        position: [(2 + x) as f32, (y * 3 + 1) as f32],
                                        size: [1.0; 2],
                                        uv_size: [1.0; 2],
                                        tint: [1.0; 3],
                                        alpha: 1.0,
                                        z_adjust: (depth.as_u64().unwrap() as i32
                                            - (32768 - (y * 3 + 1) as i32))
                                            as f32,
                                        palette_light: PaletteLight::plain(1, 1000),
                                        ..Default::default()
                                    })
                            })
                            .collect();
                        let mut raw = Vec::new();
                        for (y, row) in cases.iter().enumerate() {
                            let word = row["raw"].as_u64().unwrap() as u16;
                            let seed = |position, size, word| {
                                let rgb = encoded(word, wgpu::TextureFormat::Rgba8UnormSrgb);
                                SpriteInstance {
                                    position,
                                    size,
                                    uv_size: [1.0; 2],
                                    tint: std::array::from_fn(|i| f32::from(rgb[i]) / 255.0),
                                    alpha: 1.0,
                                    ..Default::default()
                                }
                            };
                            raw.push(seed([0.0, (y * 3) as f32], [8.0, 3.0], word));
                            let displacement = row["displacement"].as_i64().unwrap() as i32;
                            if displacement != 0 {
                                let guard = 10 + displacement;
                                raw.push(seed(
                                    [(guard % 8) as f32, (y * 3) as f32 + (guard / 8) as f32],
                                    [1.0; 2],
                                    word ^ 0x07e0,
                                ));
                            }
                        }
                        pool.upload_on_device(&gpu.device, &gpu.queue, "depth_seeds", &seeds);
                        pool.upload_on_device(&gpu.device, &gpu.queue, "raw_seeds", &raw);
                        let color = gpu.target(size, format);
                        let cv = color.create_view(&Default::default());
                        let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
                        let dv = depth.create_view(&Default::default());
                        let mut terrain = crate::render::terrain_draw::TerrainDrawRenderer::new(
                            &gpu.device,
                            &gpu.queue,
                            format,
                            &batch,
                        );
                        terrain.prepare(&gpu.device, &color, &dv, batch.camera_uniform());
                        let mut encoder = gpu.device.create_command_encoder(&Default::default());
                        clear(
                            &mut encoder,
                            &cv,
                            &dv,
                            65535,
                            wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        );
                        {
                            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                    view: &cv,
                                    resolve_target: None,
                                    depth_slice: None,
                                    ops: wgpu::Operations {
                                        load: wgpu::LoadOp::Load,
                                        store: wgpu::StoreOp::Store,
                                    },
                                })],
                                depth_stencil_attachment: Some(
                                    wgpu::RenderPassDepthStencilAttachment {
                                        view: &dv,
                                        depth_ops: Some(wgpu::Operations {
                                            load: wgpu::LoadOp::Load,
                                            store: wgpu::StoreOp::Store,
                                        }),
                                        stencil_ops: None,
                                    },
                                ),
                                ..Default::default()
                            });
                            let (seed, count) = pool.get("depth_seeds").unwrap();
                            batch.draw_zsprite_range(
                                &mut pass,
                                &white,
                                batch.default_zshape_bind_group(),
                                seed,
                                0,
                                count,
                                true,
                            );
                            let (raw, count) = pool.get("raw_seeds").unwrap();
                            batch.draw_with_buffer_passthrough(&mut pass, &white, raw, count);
                        }
                        draw_native_object_pass(
                            &mut encoder,
                            &cv,
                            &dv,
                            &mut terrain,
                            [0, 0, size[0], size[1]],
                            &batch,
                            pool.get("packed_native"),
                            layer,
                            None,
                            None,
                            Some(&voxel),
                            Some(&shp),
                            Some(&palettes),
                            batch.default_zshape_bind_group(),
                            None,
                            None,
                        );
                        let reads = [
                            gpu.read(&mut encoder, &color),
                            gpu.read(&mut encoder, &depth),
                        ];
                        let output = gpu.finish(encoder, &reads, size);
                        for (y, row) in cases.iter().enumerate() {
                            for x in 0..3 {
                                let offset = ((y * 3 + 1) * 8 + 2 + x) * 4;
                                assert_eq!(
                                    &output[0][offset..offset + 4],
                                    &encoded(row["colors"][x].as_u64().unwrap() as u16, format),
                                    "{collection} {route} indexed {indexed} {format:?} row{y} pixel{x} {row}"
                                );
                                assert_eq!(
                                    crate::render::native_z::stored_z(f32::from_le_bytes(
                                        output[1][offset..offset + 4].try_into().unwrap()
                                    )),
                                    row["depths"][x].as_u64().unwrap() as u16,
                                    "{route} indexed {indexed} row{y} pixel{x} depth"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Original Unit73BF7B material and Unit73B140 final blit with the physical
/// Image-resolved GTNK hull and Computer1's stock DarkRed scheme. The native
/// indexed cache is a supplied input here: this checks the production material,
/// both atlas bindings and replay, not hull/turret/barrel assembly. Actual
/// Ground-pose raster/cache/upload has separate native MTNK part coverage in
/// naval_sinking_gpu_tests::drone_rocked_tank_pose_upload_and_replay_match_native_part_pixels.
#[test]
#[ignore = "requires GPU and retail assets; physical Grizzly cache through native berserk material and both voxel replay bindings"]
fn retained_stock_grizzly_berserk_material_matches_original_pixels() {
    use crate::app::presentation::lighting::{techno_effect_light, unit_voxel_effect_light};
    use crate::render::native_z::{ZGradient, pack_voxel_z_gradient};
    use crate::render::unit_atlas::{UnitAtlas, UnitAtlasPage};
    use crate::render::unit_pose_cache::VxlPoseFrameCache;
    use crate::rules::house_colors::HouseColorIndex;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::superweapon::invulnerability::{InvulnKind, InvulnerabilityState};
    use crate::sim::timer::CdTimer;
    use crate::sim::world::Simulation;

    let native = &crate::test_fixture::json("tools/berserk_color.json")["mtnk"];
    let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    for asset in native["assets"].as_array().unwrap() {
        let bytes = assets.get_ref(asset["file"].as_str().unwrap()).unwrap();
        assert_eq!(bytes.len() as u64, asset["bytes"]);
        assert_eq!(crate::util::sha256::sha256_hex(bytes), asset["sha256"]);
    }
    assert_eq!(
        crate::rules::voxel_frame_catalog::voxel_image_id("MTNK", Some(&battle.rules)),
        native["image"]["resolved"].as_str().unwrap()
    );
    let scheme = &native["colour_scheme"];
    let scheme_entry = battle
        .rules
        .color_schemes
        .iter()
        .position(|entry| entry.name == scheme["name"].as_str().unwrap())
        .unwrap();
    assert_eq!(scheme_entry, 5, "Computer1 priority1 resolves to DarkRed");
    assert_eq!(
        serde_json::json!(battle.rules.color_schemes[scheme_entry].hsv),
        scheme["hsv"]
    );
    let house = HouseColorIndex(u8::try_from(scheme_entry).unwrap());
    // The native fixture constructs only the selected scheme's 1/53 pair.
    // Its local index1 is not the full startup array's index or GPU row.
    let remap_row = u32::from(house.0) + 1;
    assert_eq!(remap_row, 6);
    let palette = crate::assets::pal_file::Palette::from_bytes(
        assets
            .get_ref(scheme["palette"]["name"].as_str().unwrap())
            .unwrap(),
    )
    .unwrap();
    let decode = |value: &Value| {
        let hex = value.as_str().unwrap();
        assert_eq!(hex.len() % 2, 0);
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect::<Vec<_>>()
    };
    let remapped = palette.with_house_colors(battle.rules.house_color_ramps.ramp(house));
    let palette_rgb: Vec<_> = remapped
        .colors()
        .iter()
        .flat_map(|color| [color.r, color.g, color.b])
        .collect();
    assert_eq!(palette_rgb, decode(&native["scheme_palette_rgb8_hex"]));

    let rows = native["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 9);
    let int = |value: &Value| i32::try_from(value.as_i64().unwrap()).unwrap();
    let mut rules_by_selector = std::collections::BTreeMap::new();
    for row in rows {
        let key = (
            int(&row["arm"]["iron_color"]),
            int(&row["arm"]["berserk_color"]),
        );
        if let std::collections::btree_map::Entry::Vacant(entry) = rules_by_selector.entry(key) {
            let patch = IniFile::from_str(&format!(
                "[AudioVisual]\nIronCurtainColor={}\nBerserkColor={}\n",
                key.0, key.1
            ));
            let Some(battle) =
                crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch(
                    "XMP03T4.MAP",
                    &patch,
                )
            else {
                return;
            };
            entry.insert(battle.rules);
        }
    }
    let source = &native["source"];
    let source_size: [u32; 2] = ["width", "height"].map(|key| source[key].as_u64().unwrap() as u32);
    let mut indices = vec![0; (source_size[0] * source_size[1]) as usize];
    for pixel in source["pixels"].as_array().unwrap() {
        indices[pixel[0].as_u64().unwrap() as usize] = pixel[1].as_u64().unwrap() as u8;
    }
    let crop: [u32; 4] = std::array::from_fn(|i| source["source_rect"][i].as_u64().unwrap() as u32);
    let size: [u32; 2] =
        ["width", "height"].map(|key| native["surface"][key].as_u64().unwrap() as u32);
    let pixels = (size[0] * size[1]) as usize;
    let gpu = Gpu::new();
    let mut sim = Simulation::new();
    let mut compared = 0;
    for format in [
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ] {
        let mut batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
        let mut uniform = camera(size);
        uniform.native_z_origin_y = int(&native["surface"]["depth_baseline"]) as f32 - 32768.;
        batch.write_camera(&gpu.queue, uniform);
        let palettes = PaletteSet::new_on_device(
            &gpu.device,
            &gpu.queue,
            &palette,
            &palette,
            &battle.rules.house_color_ramps,
            &[house],
        );
        let texture = || {
            batch.create_unit_atlas_texture_on_device(
                &gpu.device,
                &gpu.queue,
                source_size[0],
                source_size[1],
                &indices,
            )
        };
        let units = UnitAtlas::from_test_pages(vec![UnitAtlasPage { texture: texture() }]);
        let poses = VxlPoseFrameCache::from_test_textures(vec![texture()]);
        for row in rows {
            let name = row["name"].as_str().unwrap();
            let arm = &row["arm"];
            assert_eq!(arm["pixel_format"], 2);
            assert_eq!(row["source_unchanged"], true);
            assert_eq!(row["z_unchanged"], true);
            assert_eq!(row["a_unchanged"], true);
            let mut unit = GameEntity::test_default(1, "MTNK", "Computer1", 50, 48);
            unit.berserk
                .set_active_for_test(arm["berzerk"].as_bool().unwrap());
            unit.deactivated
                .set_for_test(arm["deactivated"].as_bool().unwrap());
            unit.flashing.start(int(&arm["flash"]));
            unit.invulnerability = Some(InvulnerabilityState::with_tint(
                CdTimer::from_raw(int(&arm["curtain"][0]), int(&arm["curtain"][1])),
                InvulnKind::IronCurtain,
                arm["stage"].as_u64().unwrap() as u8,
                CdTimer::from_raw(int(&arm["tint"][0]), int(&arm["tint"][1])),
            ));
            sim.session
                .set_binary_frame_for_test(arm["frame"].as_u64().unwrap() as u32);
            let light = PaletteLight::color_scheme([1000; 3], int(&arm["intensity"]))
                .with_colour_word(int(&arm["word"]) as u16);
            let (tint, light) = techno_effect_light(&unit, [1.; 3], light, &sim);
            let (tint, light) = unit_voxel_effect_light(
                &unit,
                tint,
                light,
                Some(&rules_by_selector[&(int(&arm["iron_color"]), int(&arm["berserk_color"]))]),
                sim.session.binary_frame(),
            );
            assert_eq!(light.brightness(), int(&arm["out_intensity"]), "{name}");
            assert_eq!(
                u64::from(light.colour_word()),
                arm["out_word"].as_u64().unwrap() & 0xffff,
                "{name}"
            );
            let a_value = row["surface"]["a_value"].as_u64().unwrap() as u8;
            let fog = ShroudBuffer::fixture(
                &gpu.device,
                &gpu.queue,
                size,
                [0.; 2],
                &vec![a_value; pixels],
            );
            batch.bind_shroud(&gpu.device, Some(&fog));
            let expected = decode(&row["rgb565_le_hex"]);
            assert_eq!(expected.len(), pixels * 2);
            let old_z = row["surface"]["old_z"].as_u64().unwrap() as u16;
            let background = row["surface"]["background"].as_u64().unwrap() as u16;
            let initial: Vec<_> = encoded(background, format).repeat(pixels);
            let position: [f32; 2] =
                std::array::from_fn(|i| (int(&row["point"][i]) - crop[i + 2] as i32 / 2) as f32);
            let selector = (row["picks"][0]["flags"].as_u64().unwrap() & 0xe) as u8;
            let policy = if selector & 6 == 0 {
                BlitPolicy::z_read(SpriteEncoding::Voxel)
            } else {
                BlitPolicy::translucent(SpriteEncoding::Voxel, RenderZPolicy::ReadOnly)
            };
            // The recorded original leaf coordinates establish the supplied
            // final-blit rectangle. Projection and native raster assembly are
            // separate owners; no image alignment/search is performed here.
            assert_eq!(position[0], int(&row["leaves"][0]["x"]) as f32);
            assert_eq!(position[1], int(&row["leaves"][0]["y"]) as f32);
            for pose in [false, true] {
                let layer = lower_object_instances(vec![PlannedObjectInstance::object(
                    ObjectDraw {
                        id: 1,
                        layer: TacticalLayer(3),
                        display_order: 0,
                        policy,
                    },
                    vec![ObjectPieceInstance {
                        target: if pose {
                            ObjectTexture::UnitPosePage(0)
                        } else {
                            ObjectTexture::UnitAtlasPage(0)
                        },
                        render_z: RenderZPolicy::ReadOnly,
                        instance: SpriteInstance {
                            position,
                            size: [crop[2] as f32, crop[3] as f32],
                            uv_origin: [
                                crop[0] as f32 / source_size[0] as f32,
                                crop[1] as f32 / source_size[1] as f32,
                            ],
                            uv_size: [
                                crop[2] as f32 / source_size[0] as f32,
                                crop[3] as f32 / source_size[1] as f32,
                            ],
                            tint,
                            palette_light: light,
                            alpha: 1.,
                            draw_state: DrawState::from_native_selector(remap_row, selector),
                            z_adjust: int(&row["prepared_z_adjust"][0]) as f32,
                            z_gradient: pack_voxel_z_gradient(ZGradient::Vertical, false),
                            zshape_origin: [position[1], crop[3] as f32],
                            ..Default::default()
                        },
                    }],
                )]);
                let layer = &layer[3];
                let mut pool = InstanceBufferPool::new();
                pool.upload_on_device(
                    &gpu.device,
                    &gpu.queue,
                    "berserk-material",
                    &layer.instances,
                );
                let color = gpu.target(size, format);
                gpu.queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &color,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    &initial,
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
                let mut terrain = crate::render::terrain_draw::TerrainDrawRenderer::new(
                    &gpu.device,
                    &gpu.queue,
                    format,
                    &batch,
                );
                terrain.prepare(&gpu.device, &color, &dv, batch.camera_uniform());
                let mut encoder = gpu.device.create_command_encoder(&Default::default());
                clear(&mut encoder, &cv, &dv, old_z, wgpu::LoadOp::Load);
                let clip = std::array::from_fn(|i| row["clip"][i].as_u64().unwrap() as u32);
                let stats = draw_native_object_pass(
                    &mut encoder,
                    &cv,
                    &dv,
                    &mut terrain,
                    clip,
                    &batch,
                    pool.get("berserk-material"),
                    layer,
                    None,
                    Some(&units),
                    Some(&poses),
                    None,
                    Some(&palettes),
                    batch.default_zshape_bind_group(),
                    None,
                    None,
                );
                assert_eq!(
                    stats.passes > 0,
                    selector & 6 != 0,
                    "{name}: native packed admission"
                );
                let reads = [
                    gpu.read(&mut encoder, &color),
                    gpu.read(&mut encoder, &depth),
                ];
                let output = gpu.finish(encoder, &reads, size);
                for (at, pixel) in output[0].as_chunks::<4>().0.iter().enumerate() {
                    let word = u16::from_le_bytes(expected[at * 2..at * 2 + 2].try_into().unwrap());
                    assert_eq!(
                        *pixel,
                        encoded(word, format),
                        "{name} pose={pose} {format:?}: native pixel{at}"
                    );
                    assert_eq!(
                        crate::render::native_z::stored_z(f32::from_le_bytes(
                            output[1][at * 4..at * 4 + 4].try_into().unwrap()
                        )),
                        old_z,
                        "{name} pose={pose} {format:?}: native Z{at}"
                    );
                }
                for actual in gpu.read_float_texels(fog.gpu_source().0, [0, 0], size) {
                    assert_eq!(
                        (actual[0] * 255.).round() as u8,
                        a_value,
                        "{name}: native A unchanged"
                    );
                }
                compared += 1;
            }
        }
    }
    assert_eq!(compared, rows.len() * 4);
}

#[test]
#[ignore = "requires GPU; distant packed parents at a supported 4K viewport"]
fn retained_packed_high_resolution_dispatch_preserves_native_pixels() {
    let native: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/procedural_drawing_oracle/translucent_blitter_a.json",
    ))
    .unwrap();
    let row = native["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| {
            row["route"] == "shp"
                && row["profile"] == "plain1"
                && row["selector_bits"] == 4
                && row["a"] == 0
                && row["raw"] == 0
                && row["brightness"] == 1000
                && row["writes_depth"] == false
        })
        .unwrap();
    let rgb: [u8; 3] = std::array::from_fn(|i| row["rgb"][i].as_u64().unwrap() as u8);
    let gpu = Gpu::new();
    let size = [3840, 2160];
    // Two distant objects, rather than a large sprite, exercise the actual
    // production batching union that exceeded Limits::default() on one axis.
    let positions = [[0.0, 0.0], [3839.0, 2159.0]];
    for format in [
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ] {
        let mut batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
        batch.write_camera(&gpu.queue, camera(size));
        let fog = ShroudBuffer::fixture(
            &gpu.device,
            &gpu.queue,
            size,
            [0.0; 2],
            &vec![0; (size[0] * size[1]) as usize],
        );
        batch.bind_shroud(&gpu.device, Some(&fog));
        let shp =
            SpriteAtlas::from_test_pages(vec![crate::render::sprite_atlas::SpriteAtlasPage {
                texture: batch.create_texture_on_device(
                    &gpu.device,
                    &gpu.queue,
                    &[rgb[0], rgb[1], rgb[2], 255],
                    1,
                    1,
                    Some(&[row["index"].as_u64().unwrap() as u8]),
                ),
            }]);
        let layers = lower_object_instances(
            positions
                .iter()
                .enumerate()
                .map(|(i, &position)| {
                    PlannedObjectInstance::object(
                        ObjectDraw {
                            id: i as u64,
                            layer: TacticalLayer(3),
                            display_order: i as u64,
                            policy: BlitPolicy::z_read(SpriteEncoding::Plain),
                        },
                        vec![ObjectPieceInstance {
                            target: ObjectTexture::ShpPage(0),
                            render_z: RenderZPolicy::ReadOnly,
                            instance: SpriteInstance {
                                position,
                                size: [1.0; 2],
                                uv_size: [1.0; 2],
                                tint: [1.0; 3],
                                alpha: 1.0,
                                palette_light: native_light(row),
                                draw_state: DrawState {
                                    fx_flags: FX_CLOAK,
                                    fx_params: [0.5, 4.0, 1.0, 0.0],
                                    ..Default::default()
                                },
                                ..Default::default()
                            },
                        }],
                    )
                })
                .collect(),
        );
        let mut pool = InstanceBufferPool::new();
        pool.upload_on_device(&gpu.device, &gpu.queue, "packed_4k", &layers[3].instances);
        let color = gpu.target(size, format);
        let cv = color.create_view(&Default::default());
        let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
        let dv = depth.create_view(&Default::default());
        let mut terrain = crate::render::terrain_draw::TerrainDrawRenderer::new(
            &gpu.device,
            &gpu.queue,
            format,
            &batch,
        );
        terrain.prepare(&gpu.device, &color, &dv, batch.camera_uniform());
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        clear(
            &mut encoder,
            &cv,
            &dv,
            65535,
            wgpu::LoadOp::Clear(wgpu::Color::BLACK),
        );
        let stats = draw_native_object_pass(
            &mut encoder,
            &cv,
            &dv,
            &mut terrain,
            [0, 0, size[0], size[1]],
            &batch,
            pool.get("packed_4k"),
            &layers[3],
            None,
            None,
            None,
            Some(&shp),
            None,
            batch.default_zshape_bind_group(),
            None,
            None,
        );
        let read = gpu.read(&mut encoder, &color);
        let output = gpu.finish(encoder, &[read], size);
        assert_eq!(stats.pieces, 2);
        assert_eq!(stats.waves, 1);
        let expected = encoded(row["colors"][0].as_u64().unwrap() as u16, format);
        for [x, y] in positions {
            let index = ((y as usize * size[0] as usize) + x as usize) * 4;
            assert_eq!(&output[0][index..index + 4], &expected);
        }
        let middle = ((size[1] / 2 * size[0] + size[0] / 2) * 4) as usize;
        assert_eq!(&output[0][middle..middle + 4], &[0, 0, 0, 255]);
    }
}

/// Production replay cost, including continuation submissions. This reports
/// encode and completed wall time, not ordinary gameplay FPS or GPU timestamps.
#[test]
#[ignore = "requires GPU; packed replay workload at 1,000 and 20,000 parents"]
fn retained_packed_workload_timing() {
    let gpu = Gpu::new();
    let size = [1280, 720];
    let format = wgpu::TextureFormat::Bgra8UnormSrgb;
    let mut batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
    batch.write_camera(&gpu.queue, camera(size));
    let fog = ShroudBuffer::fixture(
        &gpu.device,
        &gpu.queue,
        size,
        [0.0; 2],
        &vec![0; (size[0] * size[1]) as usize],
    );
    batch.bind_shroud(&gpu.device, Some(&fog));
    let shp = SpriteAtlas::from_test_pages(vec![crate::render::sprite_atlas::SpriteAtlasPage {
        texture: batch.create_texture_on_device(
            &gpu.device,
            &gpu.queue,
            &[200, 100, 50, 255],
            1,
            1,
            Some(&[1]),
        ),
    }]);
    let color = gpu.target(size, format);
    let cv = color.create_view(&Default::default());
    let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
    let dv = depth.create_view(&Default::default());
    let mut terrain = crate::render::terrain_draw::TerrainDrawRenderer::new(
        &gpu.device,
        &gpu.queue,
        format,
        &batch,
    );
    for count in [1_000u32, 20_000] {
        let parents = (0..count)
            .map(|i| {
                PlannedObjectInstance::object(
                    ObjectDraw {
                        id: u64::from(i),
                        layer: TacticalLayer(3),
                        display_order: u64::from(i),
                        policy: BlitPolicy::z_read(SpriteEncoding::Plain),
                    },
                    vec![ObjectPieceInstance {
                        target: ObjectTexture::ShpPage(0),
                        render_z: RenderZPolicy::ReadOnly,
                        instance: SpriteInstance {
                            position: [(i % 80 * 16) as f32, ((i / 80) % 45 * 16) as f32],
                            size: [16.0; 2],
                            uv_size: [1.0; 2],
                            tint: [1.0; 3],
                            alpha: 1.0,
                            palette_light: PaletteLight::plain(1, 1000),
                            draw_state: DrawState {
                                fx_flags: FX_CLOAK,
                                fx_params: [0.5, 4.0, 1.0, 0.0],
                                ..Default::default()
                            },
                            ..Default::default()
                        },
                    }],
                )
            })
            .collect();
        let layers = lower_object_instances(parents);
        let layer = &layers[3];
        let mut pool = InstanceBufferPool::new();
        pool.upload_on_device(&gpu.device, &gpu.queue, "packed_workload", &layer.instances);
        terrain.prepare(&gpu.device, &color, &dv, batch.camera_uniform());
        let start = std::time::Instant::now();
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        clear(
            &mut encoder,
            &cv,
            &dv,
            65535,
            wgpu::LoadOp::Clear(wgpu::Color::BLACK),
        );
        let stats = draw_native_object_pass(
            &mut encoder,
            &cv,
            &dv,
            &mut terrain,
            [0, 0, size[0], size[1]],
            &batch,
            pool.get("packed_workload"),
            layer,
            None,
            None,
            None,
            Some(&shp),
            None,
            batch.default_zshape_bind_group(),
            None,
            None,
        );
        let command = encoder.finish();
        let encode_ms = start.elapsed().as_secs_f64() * 1000.0;
        let submission = gpu.queue.submit([command]);
        gpu.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(std::time::Duration::from_secs(60)),
            })
            .unwrap();
        eprintln!(
            "PACKED_WORKLOAD count={count} encode_ms={encode_ms:.3} completed_wall_ms={:.3} waves={} passes={} debug_assertions={}",
            start.elapsed().as_secs_f64() * 1000.0,
            stats.waves,
            stats.passes,
            cfg!(debug_assertions),
        );
        assert_eq!(stats.pieces, count as usize);
    }
}
