//! Bridge cell-sweep submission against original 00497390 pixel goldens.
//! The complete retail Cell/shape corpus separately establishes caller inputs;
//! this gate keeps the actual bridge dispatch on the shared color/Z owner.

use super::draw_passes::draw_bridge_shadows;
use crate::render::batch::{BatchRenderer, InstanceBufferPool};
use crate::render::native_z::{DEFAULT_Z, stored_z};
use crate::render::terrain_draw::TerrainDrawRenderer;
use crate::render::terrain_draw_gpu_tests::{Gpu, camera, clear, encoded, sprite};

#[test]
#[ignore = "requires GPU; actual bridge dispatch, native signed depth stores and repeated overlap"]
fn bridge_shadow_submission_matches_native_color_depth_and_repeat() {
    // 0047F510 flags 0x4601 select Convert +0x114 -> 00497390, the
    // already-executed Terrain shadow leaf. Reuse its original-instruction
    // corpus; do not calculate expected darkening or depth stores in Rust.
    let fixture: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/terrain_draw_oracle/fixtures/leaf.json",
    ))
    .unwrap();
    assert_eq!(fixture["leaves"][1], "00497390");
    let cases = fixture["depth_cases"].as_array().unwrap();
    let gpu = Gpu::new();
    let size = [128, 2];
    for format in [
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ] {
        let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
        batch.write_camera(&gpu.queue, camera(size));
        let mut terrain = TerrainDrawRenderer::new(&gpu.device, &gpu.queue, format, &batch);
        let color = gpu.target(size, format);
        let cv = color.create_view(&Default::default());
        let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
        let dv = depth.create_view(&Default::default());
        terrain.prepare(&gpu.device, &color, &dv, batch.camera_uniform());
        // Preserve the previous bridge RGBA payload to reproduce its defect.
        // The native path consumes the integer stencil, never alpha128.
        let texture = batch.create_texture_on_device(
            &gpu.device,
            &gpu.queue,
            &[0, 0, 0, 128, 0, 0, 0, 0],
            2,
            1,
            Some(&[1, 0]),
        );
        let mut pool = InstanceBufferPool::new();
        for old in [0u16, 1, 32767, 32768, 65535] {
            let selected: Vec<_> = cases
                .iter()
                .filter(|case| {
                    case[0].as_u64() == Some(1) && case[3].as_u64() == Some(u64::from(old))
                })
                .collect();
            assert!(selected.len() * 2 <= size[0] as usize);
            let instances: Vec<_> = selected
                .iter()
                .enumerate()
                .map(|(index, case)| {
                    // Feed the original leaf's signed candidate directly to
                    // a flat one-row sprite. This is leaf admission coverage,
                    // not a second implementation of SHP row/Z-shape walking.
                    let candidate = case[1].as_i64().unwrap() - case[2].as_i64().unwrap();
                    sprite(
                        [(index * 2) as f32, 0.0],
                        [2.0, 1.0],
                        (candidate - i64::from(DEFAULT_Z)) as f32,
                    )
                })
                .collect();
            pool.upload_on_device(&gpu.device, &gpu.queue, "bridge-shadow-test", &instances);
            for repeat in [false, true] {
                let mut encoder = gpu.device.create_command_encoder(&Default::default());
                if !repeat {
                    clear(
                        &mut encoder,
                        &cv,
                        &dv,
                        old,
                        wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                    );
                }
                draw_bridge_shadows(
                    &mut encoder,
                    &cv,
                    &dv,
                    &mut terrain,
                    [0, 0, size[0], size[1]],
                    &batch,
                    pool.get("bridge-shadow-test"),
                    &instances,
                    Some(&texture),
                );
                let reads = [
                    gpu.read(&mut encoder, &color),
                    gpu.read(&mut encoder, &depth),
                ];
                let output = gpu.finish(encoder, &reads, size);
                for (index, case) in selected.iter().enumerate() {
                    let offset = index * 8;
                    let expected = case[if repeat { 6 } else { 4 }].as_u64().unwrap() as u16;
                    assert_eq!(
                        output[0][offset..offset + 4],
                        encoded(expected, format),
                        "{format:?}, old={old}, repeat={repeat}, native leaf={case}"
                    );
                    let actual =
                        f32::from_le_bytes(output[1][offset..offset + 4].try_into().unwrap());
                    assert_eq!(
                        stored_z(actual),
                        case[5].as_u64().unwrap() as u16,
                        "live bridge depth; old={old}, repeat={repeat}, native leaf={case}"
                    );
                    assert_eq!(
                        &output[0][offset + 4..offset + 8],
                        &[255; 4],
                        "zero source index must preserve destination color"
                    );
                    let transparent =
                        f32::from_le_bytes(output[1][offset + 4..offset + 8].try_into().unwrap());
                    assert_eq!(stored_z(transparent), old, "zero index preserves live Z");
                }
            }
        }
    }
}

use crate::app::presentation::instances::bridges::build_bridge_shadow_instances_inner;
use crate::assets::asset_manager::{AssetManager, MediaArchiveMode};
use crate::map::overlay::OverlayEntry;
use crate::map::resolved_terrain::{test_flat_cell, test_grid};
use crate::render::batch::{CameraUniform, SpriteInstance};
use crate::render::bridge_atlas::{BridgeAtlas, build_bridge_atlas};
use crate::render::terrain_draw_gpu_tests::extent;
use crate::rules::ini_parser::IniFile;
use crate::rules::overlay_types::OverlayTypeRegistry;
use serde_json::Value;
use std::collections::BTreeMap;

struct RetailBridgeFixture {
    assets: AssetManager,
    theater: crate::map::theater::TheaterData,
    rules: crate::rules::ruleset::RuleSet,
    rules_ini: IniFile,
    registry: OverlayTypeRegistry,
    names: BTreeMap<u8, String>,
    overlays: Vec<OverlayEntry>,
}

impl RetailBridgeFixture {
    fn load(packet: &Value) -> Self {
        let root =
            std::path::PathBuf::from(std::env::var_os("RA2_DIR").expect("physical retail root"));
        let mut assets = AssetManager::new(&root, MediaArchiveMode::STOCK_DIGITAL).unwrap();
        let mode = IniFile::from_bytes(assets.get_ref("MPBattleMD.ini").unwrap()).unwrap();
        let (mut rules, rules_ini, art, _) =
            crate::app::loading::init_helpers::load_rules_with_merged_ini(
                &assets,
                Some(&mode),
                None,
            )
            .unwrap();
        rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
        let registry = OverlayTypeRegistry::from_ini(&rules_ini, Some(&art));
        let theater = crate::map::theater::load_theater(&mut assets, "TEMPERATE").unwrap();
        for source in packet["inputs"]["sources"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|source| source["name"].as_str().unwrap().ends_with(".tem"))
        {
            assert_eq!(
                crate::util::sha256::sha256_hex(
                    assets.get_ref(source["name"].as_str().unwrap()).unwrap()
                ),
                source["sha256"].as_str().unwrap(),
                "GPU fixture and original caller must use the same physical SHP"
            );
        }
        let names: BTreeMap<_, _> = ["BRIDGE1", "BRIDGE2", "BRIDGEB1", "BRIDGEB2"]
            .into_iter()
            .map(|name| (registry.id_for_name(name).unwrap(), name.to_owned()))
            .collect();
        let overlays = names
            .keys()
            .map(|&overlay_id| OverlayEntry {
                rx: 10,
                ry: 20,
                overlay_id,
                frame: 0,
            })
            .collect();
        Self {
            assets,
            theater,
            rules,
            rules_ini,
            registry,
            names,
            overlays,
        }
    }

    fn atlas(&self, gpu: &Gpu, batch: &BatchRenderer) -> BridgeAtlas {
        build_bridge_atlas(
            &gpu.device,
            &gpu.queue,
            batch,
            &self.overlays,
            &self.names,
            &self.assets,
            &self.theater.iso_palette,
            &self.theater.unit_palette,
            self.theater.extension,
            "TEMPERATE",
            &self.registry,
            &self.rules.crate_rules,
            &self.rules_ini,
            self.rules.art(),
        )
        .unwrap()
    }

    fn instances(
        &self,
        input: &Value,
        atlas: &BridgeAtlas,
        size: [u32; 2],
    ) -> (CameraUniform, Vec<SpriteInstance>) {
        let rx = input["coords"][0].as_u64().unwrap() as u16;
        let ry = input["coords"][1].as_u64().unwrap() as u16;
        let mut terrain = test_grid(rx + 1, ry + 1, test_flat_cell);
        let cell = terrain.cell_mut(rx, ry).unwrap();
        cell.level = input["level"].as_i64().unwrap() as u8;
        cell.bridge_facts.raw_flags = input["flags"].as_u64().unwrap() as u32;
        cell.bridge_facts.state_byte = input["state"].as_u64().unwrap() as u8;
        cell.bridge_facts.overlay_id = Some(
            self.registry
                .id_for_name(input["type"].as_str().unwrap())
                .unwrap(),
        );
        // These original caller cases supply the unraised cell screen point.
        // Align the existing isometric projection with that seam. The actual
        // bridge builder must apply the live signed height, flags and frame.
        let point = crate::map::terrain::iso_to_screen(rx, ry, 0);
        let mut camera = camera(size);
        camera.camera_pos = [
            point.0 - input["cell_point"][0].as_i64().unwrap() as f32,
            point.1
                - input["cell_point"][1].as_i64().unwrap() as f32
                - input["viewport_y"].as_i64().unwrap() as f32,
        ];
        // Native fixture viewport +Y moves the shape; its Z-buffer origin is
        // independently zero. Do not use viewport Y as a second depth bias.
        let mut instances = Vec::new();
        build_bridge_shadow_instances_inner(
            &terrain,
            atlas,
            &self.names,
            camera.world_origin_y,
            camera.world_height,
            camera.camera_pos[0],
            camera.camera_pos[1],
            size[0] as f32,
            size[1] as f32,
            &mut instances,
        );
        assert_eq!(
            instances.len(),
            1,
            "original caller admits {}",
            input["name"]
        );
        let instances = instances.repeat(input["repeat"].as_u64().unwrap() as usize);
        (camera, instances)
    }
}

fn native_bridge_packet() -> Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/bridge_shadow_render.json",
    ))
    .unwrap()
}

fn assert_native_cell_sweep_order(
    fixture: &RetailBridgeFixture,
    atlas: &BridgeAtlas,
    packet: &Value,
) {
    let calls: Vec<_> = packet["traversal"]["calls"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|call| call["piece"] == "shadow")
        .collect();
    let mut terrain = test_grid(32, 32, test_flat_cell);
    let id = fixture.registry.id_for_name("BRIDGE1").unwrap();
    let mut identity_by_position = BTreeMap::new();
    let mut expected = Vec::new();
    for call in calls {
        let coords: [u16; 2] = std::array::from_fn(|i| call["coords"][i].as_u64().unwrap() as u16);
        expected.push(coords);
        let cell = terrain.cell_mut(coords[0], coords[1]).unwrap();
        cell.bridge_facts.raw_flags = 0x180;
        cell.bridge_facts.overlay_id = Some(id);
        let point = crate::map::terrain::iso_to_screen(coords[0], coords[1], 0);
        // VERA's documented world frame adds fifteen rows to every layer;
        // the camera absorbs that translation. Pin it to original traversal
        // points instead of deriving a native expectation from Rust alone.
        assert_eq!(point.0, call["point"][0].as_i64().unwrap() as f32);
        assert_eq!(point.1 - 15.0, call["point"][1].as_i64().unwrap() as f32);
        let input = serde_json::json!({"name":"traversal identity", "coords":coords,
            "level":0,"flags":384,"state":0,"type":"BRIDGE1", "repeat":1,
            "cell_point":[(point.0+600.0) as i32,(point.1-200.0) as i32], "viewport_y":0});
        // A single emitted instance identifies its source cell. The ordering
        // expectation itself is the recorded original Display traversal.
        let (_, one) = fixture.instances(&input, atlas, [1024, 1024]);
        assert!(
            identity_by_position
                .insert(one[0].position.map(f32::to_bits), coords)
                .is_none()
        );
    }
    let mut instances = Vec::new();
    build_bridge_shadow_instances_inner(
        &terrain,
        atlas,
        &fixture.names,
        -100.0,
        1000.0,
        -600.0,
        200.0,
        1024.0,
        1024.0,
        &mut instances,
    );
    let actual: Vec<_> = instances
        .iter()
        .map(|instance| {
            *identity_by_position
                .get(&instance.position.map(f32::to_bits))
                .unwrap()
        })
        .collect();
    assert_eq!(
        actual, expected,
        "bridge instance order must preserve native cell sweep"
    );
}

#[test]
#[ignore = "requires GPU and retail assets; full original bridge caller through production atlas, builder and submission"]
fn retail_bridge_caller_matches_native_color_and_depth_attachments() {
    let packet = native_bridge_packet();
    assert_eq!(
        packet["surface"]["baseline"].as_i64(),
        Some(i64::from(DEFAULT_Z))
    );
    let size = [
        packet["surface"]["width"].as_u64().unwrap() as u32,
        packet["surface"]["height"].as_u64().unwrap() as u32,
    ];
    let pixels = (size[0] * size[1]) as usize;
    let fixture = RetailBridgeFixture::load(&packet);
    let gpu = Gpu::new();
    for format in [
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ] {
        let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
        let atlas = fixture.atlas(&gpu, &batch);
        assert_native_cell_sweep_order(&fixture, &atlas, &packet);
        let mut terrain = TerrainDrawRenderer::new(&gpu.device, &gpu.queue, format, &batch);
        let color = gpu.target(size, format);
        let cv = color.create_view(&Default::default());
        let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
        let dv = depth.create_view(&Default::default());
        let mut pool = InstanceBufferPool::new();
        for row in packet["rows"].as_array().unwrap() {
            let input = &row["input"];
            let (camera, instances) = fixture.instances(input, &atlas, size);
            batch.write_camera(&gpu.queue, camera);
            terrain.prepare(&gpu.device, &color, &dv, camera);
            pool.upload_on_device(&gpu.device, &gpu.queue, "bridge-native-row", &instances);
            let background = input["background"].as_u64().unwrap() as u16;
            let old_z = input["old_z"].as_u64().unwrap() as u16;
            let initial = encoded(background, format).repeat(pixels);
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
                extent(size),
            );
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            clear(&mut encoder, &cv, &dv, old_z, wgpu::LoadOp::Load);
            let clip = std::array::from_fn(|i| input["clip"][i].as_u64().unwrap() as u32);
            draw_bridge_shadows(
                &mut encoder,
                &cv,
                &dv,
                &mut terrain,
                clip,
                &batch,
                pool.get("bridge-native-row"),
                &instances,
                Some(&atlas.texture),
            );
            let reads = [
                gpu.read(&mut encoder, &color),
                gpu.read(&mut encoder, &depth),
            ];
            let output = gpu.finish(encoder, &reads, size);
            // Lossless expansion of captured native buffers. This contains no
            // projection, depth arithmetic, stencil or color transformation.
            let mut expected = vec![[background, old_z]; pixels];
            for run in row["output"]["runs"].as_array().unwrap() {
                let values: [usize; 5] = std::array::from_fn(|i| run[i].as_u64().unwrap() as usize);
                let [x, y, count, word, z] = values;
                let start = y * size[0] as usize + x;
                expected[start..start + count].fill([word as u16, z as u16]);
            }
            for (index, [word, z]) in expected.into_iter().enumerate() {
                let offset = index * 4;
                let actual_z = stored_z(f32::from_le_bytes(
                    output[1][offset..offset + 4].try_into().unwrap(),
                ));
                assert_eq!(
                    (&output[0][offset..offset + 4], actual_z),
                    (&encoded(word, format)[..], z),
                    "{} {format:?} pixel ({},{})",
                    row["name"],
                    index % size[0] as usize,
                    index / size[0] as usize,
                );
            }
        }
    }
    eprintln!(
        "{} full original bridge caller/shape rows matched production atlas, live-cell builder, GPU color and depth in both sRGB formats",
        packet["rows"].as_array().unwrap().len()
    );
}

#[test]
#[ignore = "bounded bridge GPU/CPU timing; requires retail assets and an uncontended GPU"]
fn retail_bridge_shadow_workload_timing() {
    use std::time::{Duration, Instant};
    let packet = native_bridge_packet();
    let fixture = RetailBridgeFixture::load(&packet);
    let gpu = Gpu::with_features(
        wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS,
    );
    let size = [800, 600];
    let format = wgpu::TextureFormat::Bgra8UnormSrgb;
    let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
    let atlas = fixture.atlas(&gpu, &batch);
    let mut input = packet["rows"][0]["input"].clone();
    input["cell_point"] = serde_json::json!([180, 200]);
    let (camera, one) = fixture.instances(&input, &atlas, size);
    batch.write_camera(&gpu.queue, camera);
    let mut terrain = TerrainDrawRenderer::new(&gpu.device, &gpu.queue, format, &batch);
    let color = gpu.target(size, format);
    let cv = color.create_view(&Default::default());
    let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
    let dv = depth.create_view(&Default::default());
    let shape = packet["shapes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|shape| shape["image_name"] == "BRIDGE" && shape["frame"] == 18)
        .unwrap();
    let rect: [usize; 4] =
        std::array::from_fn(|i| shape["native_rect"][i].as_u64().unwrap() as usize);
    let [left, top, width, height] = rect;
    assert_eq!(one[0].size, [width as f32, height as f32]);
    // Recreate only the former alpha128 payload from the captured physical
    // stencil for a dispatch-cost reference. It is never an expected image.
    let mut legacy_rgba = vec![0; width * height * 4];
    for run in shape["mask_runs"].as_array().unwrap() {
        let [x, y, count]: [usize; 3] = std::array::from_fn(|i| run[i].as_u64().unwrap() as usize);
        for x in x..x + count {
            legacy_rgba[((y - top) * width + x - left) * 4 + 3] = 128;
        }
    }
    let legacy = batch.create_texture_on_device(
        &gpu.device,
        &gpu.queue,
        &legacy_rgba,
        width as u32,
        height as u32,
        None,
    );
    let queries = gpu.device.create_query_set(&wgpu::QuerySetDescriptor {
        label: Some("Bridge shadow dispatch interval"),
        ty: wgpu::QueryType::Timestamp,
        count: 2,
    });
    let resolved = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 256,
        usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let mapped = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 16,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut pool = InstanceBufferPool::new();
    let timestamp_period_ns = gpu.queue.get_timestamp_period() as f64;
    let mut results = Vec::new();
    for (count, layout) in [(1usize, "single"), (16, "adjacent"), (128, "repeated")] {
        let instances: Vec<_> = (0..count)
            .map(|i| {
                let mut instance = one[0];
                if layout == "adjacent" {
                    instance.position[0] += 30.0 * i as f32;
                    instance.position[1] += 15.0 * i as f32;
                }
                instance
            })
            .collect();
        for native in [false, true] {
            let upload: Vec<_> = instances
                .iter()
                .map(|instance| {
                    let mut instance = *instance;
                    if !native {
                        instance.uv_origin = [0.0; 2];
                        instance.uv_size = [1.0; 2];
                    }
                    instance
                })
                .collect();
            pool.upload_on_device(&gpu.device, &gpu.queue, "bridge-perf", &upload);
            // Complete setup uploads outside the measured interval. Later
            // samples begin with the preceding sample's completion fence done.
            let uploaded = gpu.queue.submit([]);
            gpu.device
                .poll(wgpu::PollType::Wait {
                    submission_index: Some(uploaded),
                    timeout: Some(Duration::from_secs(60)),
                })
                .unwrap();
            for sample in 0..4 {
                let begin = Instant::now();
                terrain.prepare(&gpu.device, &color, &dv, camera);
                let mut encoder = gpu.device.create_command_encoder(&Default::default());
                encoder.write_timestamp(&queries, 0);
                clear(
                    &mut encoder,
                    &cv,
                    &dv,
                    65535,
                    wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                );
                let stats = if native {
                    draw_bridge_shadows(
                        &mut encoder,
                        &cv,
                        &dv,
                        &mut terrain,
                        [0, 0, size[0], size[1]],
                        &batch,
                        pool.get("bridge-perf"),
                        &upload,
                        Some(&atlas.texture),
                    )
                } else {
                    let mut pass = crate::app::presentation::sidebar_render::begin_main_load_pass(
                        &mut encoder,
                        &cv,
                        &dv,
                    );
                    let (buffer, count) = pool.get("bridge-perf").unwrap();
                    batch.draw_with_buffer_passthrough(&mut pass, &legacy, buffer, count);
                    drop(pass);
                    crate::render::terrain_draw::TerrainBatchStats {
                        pieces: count as usize,
                        passes: 1,
                        ..Default::default()
                    }
                };
                encoder.write_timestamp(&queries, 1);
                let encode_ms = begin.elapsed().as_secs_f64() * 1000.0;
                encoder.resolve_query_set(&queries, 0..2, &resolved, 0);
                encoder.copy_buffer_to_buffer(&resolved, 0, &mapped, 0, 16);
                let final_submit_begin = Instant::now();
                let submission = gpu.queue.submit([encoder.finish()]);
                let (tx, rx) = std::sync::mpsc::channel();
                mapped
                    .slice(..)
                    .map_async(wgpu::MapMode::Read, move |result| tx.send(result).unwrap());
                gpu.device
                    .poll(wgpu::PollType::Wait {
                        submission_index: Some(submission),
                        timeout: Some(Duration::from_secs(60)),
                    })
                    .unwrap();
                rx.recv().unwrap().unwrap();
                let final_submit_to_completion_ms =
                    final_submit_begin.elapsed().as_secs_f64() * 1000.0;
                let wall_prepare_to_completion_ms = begin.elapsed().as_secs_f64() * 1000.0;
                let data = mapped.slice(..).get_mapped_range();
                let first = u64::from_le_bytes(data[0..8].try_into().unwrap());
                let last = u64::from_le_bytes(data[8..16].try_into().unwrap());
                // wgpu27 encoder timestamps can be reordered. The Metal HAL
                // defers them to later encoders and documents empty-blit
                // sampling limits; keep raw values, never invent a duration.
                // https://docs.rs/wgpu/27.0.1/wgpu/struct.CommandEncoder.html#method.write_timestamp
                // https://docs.rs/wgpu-hal/27.0.4/src/wgpu_hal/metal/command.rs.html
                let (timestamp_status, encoder_timestamp_ms) =
                    if first == 0 || last == 0 || first == u64::MAX || last == u64::MAX {
                        ("unavailable_zero_or_max_sample", None)
                    } else if last <= first {
                        ("unavailable_nonincreasing_samples", None)
                    } else if !timestamp_period_ns.is_finite() || timestamp_period_ns <= 0.0 {
                        ("unavailable_timestamp_period", None)
                    } else {
                        (
                            "limited_encoder_boundary",
                            Some((last - first) as f64 * timestamp_period_ns / 1_000_000.0),
                        )
                    };
                eprintln!(
                    "BRIDGE timing: count={count}, layout={layout}, native={native}, sample={sample}, timestamp_status={timestamp_status}, first={first}, last={last}, period_ns={timestamp_period_ns}, encoder_timestamp_ms={encoder_timestamp_ms:?}, cpu_prepare_encode_ms={encode_ms}, wall_prepare_to_completion_ms={wall_prepare_to_completion_ms}, final_submit_to_completion_ms={final_submit_to_completion_ms}"
                );
                drop(data);
                mapped.unmap();
                if native && layout == "repeated" {
                    assert!(
                        terrain.submissions() > 0,
                        "large overlapping bridge sweep uses bounded submission"
                    );
                }
                if sample > 0 {
                    results.push(serde_json::json!({"count":count,"layout":layout,"native":native,"sample":sample,
                        "pieces":stats.pieces,"waves":stats.waves,"passes":stats.passes,
                        "tile_dependencies":stats.tile_dependencies,"bounded_submissions":terrain.submissions(),
                        "timestamp_status":timestamp_status,"timestamp_first":first,"timestamp_last":last,
                        "timestamp_period_ns":timestamp_period_ns,"encoder_timestamp_ms":encoder_timestamp_ms,
                        "cpu_prepare_encode_ms":encode_ms,
                        "wall_prepare_to_completion_ms":wall_prepare_to_completion_ms,
                        "final_submit_to_completion_ms":final_submit_to_completion_ms}));
                }
            }
        }
    }
    let report = serde_json::json!({"target":size,"format":"Bgra8UnormSrgb",
        "source":"Physical BRIDGE.TEM frame18 through production atlas/builder/bridge dispatch; supplied adjacent and overlap workloads",
        "reference":"Former alpha128 passthrough dispatch with the same captured physical stencil; reference pixels are not parity expectations",
        "interval":"CPU preparation/encoding includes terrain preparation, clear, draw scheduling and any bounded intermediate submissions; excludes setup uploads and the final resolve/finish/submit/wait. Wall preparation-to-completion includes that work plus query resolve/readback and the final submission completion fence. Final-submit-to-completion covers only the final finish/submit/map/wait tail. Warmup omitted. No whole-frame or 20000-unit performance claim.",
        "gpu_timestamp_limit":"Encoder samples are diagnostic: wgpu27 permits backend/driver reordering, so a positive delta is a limited encoder interval, not an exact sum of GPU pass execution. Zero/max or nonincreasing values are reported with null duration; fence-completion wall measurements remain available.",
        "samples":results});
    eprintln!("{}", serde_json::to_string_pretty(&report).unwrap());
    if let Some(path) = std::env::var_os("VERA20K_BRIDGE_SHADOW_PERF_OUTPUT") {
        std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    }
}

#[test]
#[ignore = "requires physical high bridges, ordinary Engineer repair and GPU readbacks"]
fn retail_high_bridge_damage_and_repair_reach_shadow_pixels() {
    use crate::sim::world::bridge_test_evidence::{
        RetailHighBridgeScene, visit_retail_high_bridge_stages,
    };
    use crate::util::sha256::sha256_hex;
    let packet = native_bridge_packet();
    let fixture = RetailBridgeFixture::load(&packet);
    let gpu = Gpu::new();
    let size = [640, 360];
    let format = wgpu::TextureFormat::Bgra8UnormSrgb;
    let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
    let atlas = fixture.atlas(&gpu, &batch);
    let mut renderer = TerrainDrawRenderer::new(&gpu.device, &gpu.queue, format, &batch);
    let color = gpu.target(size, format);
    let cv = color.create_view(&Default::default());
    let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
    let dv = depth.create_view(&Default::default());
    let mut pool = InstanceBufferPool::new();
    for (kind, label, center, level) in [
        (
            RetailHighBridgeScene::HillsWood,
            "hills-wood",
            (65u16, 69u16),
            6,
        ),
        (
            RetailHighBridgeScene::PacificConcrete,
            "pacific-concrete",
            (81, 95),
            2,
        ),
    ] {
        let point = crate::map::terrain::iso_to_screen(center.0, center.1, level);
        let mut view = camera(size);
        view.camera_pos = [
            point.0 - size[0] as f32 / 2.0,
            point.1 - size[1] as f32 / 2.0,
        ];
        batch.write_camera(&gpu.queue, view);
        let output_dir = std::env::var_os("VERA20K_BRIDGE_SHADOW_LIFECYCLE_OUTPUT")
            .map(|path| std::path::PathBuf::from(path).join(label));
        if let Some(path) = &output_dir {
            std::fs::create_dir_all(path).unwrap();
        }
        let mut receipts = Vec::new();
        let mut images = Vec::new();
        visit_retail_high_bridge_stages(kind, |phase, scene| {
            let sim = scene.sim();
            let before = sim.state_hash();
            let terrain = sim.resolved_terrain.as_ref().unwrap();
            let mut instances = Vec::new();
            build_bridge_shadow_instances_inner(
                terrain,
                &atlas,
                &fixture.names,
                view.world_origin_y,
                view.world_height,
                view.camera_pos[0],
                view.camera_pos[1],
                size[0] as f32,
                size[1] as f32,
                &mut instances,
            );
            assert!(
                !instances.is_empty(),
                "{phase}: live retained bridge art emits shadows"
            );
            renderer.prepare(&gpu.device, &color, &dv, view);
            pool.upload_on_device(&gpu.device, &gpu.queue, "bridge-live", &instances);
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            clear(
                &mut encoder,
                &cv,
                &dv,
                65535,
                wgpu::LoadOp::Clear(wgpu::Color::WHITE),
            );
            draw_bridge_shadows(
                &mut encoder,
                &cv,
                &dv,
                &mut renderer,
                [0, 0, size[0], size[1]],
                &batch,
                pool.get("bridge-live"),
                &instances,
                Some(&atlas.texture),
            );
            let reads = [
                gpu.read(&mut encoder, &color),
                gpu.read(&mut encoder, &depth),
            ];
            let output = gpu.finish(encoder, &reads, size);
            let mut changed = 0usize;
            for (rgba, raw_z) in output[0].chunks_exact(4).zip(output[1].chunks_exact(4)) {
                let edited = rgba != [255; 4];
                let stored = stored_z(f32::from_le_bytes(raw_z.try_into().unwrap()));
                assert_eq!(
                    edited,
                    stored != 65535,
                    "{phase}: accepted stencil owns both stores"
                );
                changed += usize::from(edited);
            }
            assert!(
                changed > 0,
                "{phase}: actual shadow pixels reached the attachments"
            );
            assert_eq!(sim.state_hash(), before, "drawing cannot mutate simulation");
            let cells: Vec<_> = terrain
                .iter()
                .filter(|c| c.rx.abs_diff(center.0) <= 8 && c.ry.abs_diff(center.1) <= 8)
                .filter(|c| {
                    c.bridge_facts
                        .overlay_id
                        .is_some_and(|id| fixture.names.contains_key(&id))
                })
                .map(|c| {
                    serde_json::json!({"coords":[c.rx,c.ry],"level":c.level,
                "overlay":c.bridge_facts.overlay_id,"state":c.bridge_facts.state_byte,
                "flags":c.bridge_facts.raw_flags})
                })
                .collect();
            let receipt = serde_json::json!({"scene":label,"phase":phase,"frame":sim.session.binary_frame,
            "state_hash":format!("{before:016x}"),"cells":cells,"instances":instances.len(),
            "instance_sha256":sha256_hex(bytemuck::cast_slice(&instances)),
            "changed_pixels":changed,"color_sha256":sha256_hex(&output[0]),
            "depth_sha256":sha256_hex(&output[1])});
            eprintln!("BRIDGE_SHADOW_LIFECYCLE {receipt}");
            receipts.push(receipt);
            if let Some(path) = &output_dir {
                let mut rgba = output[0].clone();
                for pixel in rgba.chunks_exact_mut(4) {
                    pixel.swap(0, 2);
                }
                image::save_buffer(
                    path.join(format!("{phase}.png")),
                    &rgba,
                    size[0],
                    size[1],
                    image::ColorType::Rgba8,
                )
                .unwrap();
            }
            images.push(output[0].clone());
        });
        assert_eq!(
            receipts
                .iter()
                .map(|r| r["phase"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["loaded", "damaged", "collapsed", "repaired"]
        );
        // The published state must reach atlas selection even when adjoining
        // shadows hide a changed source mask. Native corpus tests separately own
        // per-frame pixel expectations (concrete18/24 and27/33 are identical;
        // the corresponding wood masks differ).
        for phases in receipts.windows(2) {
            assert_ne!(
                phases[0]["instance_sha256"], phases[1]["instance_sha256"],
                "{label}: live bridge publication must change emitted shadow instances"
            );
        }
        assert!(
            images[1] != images[2],
            "{label}: collapse changes retained shadow coverage"
        );
        assert!(
            images[2] != images[3],
            "{label}: ordinary repair restores shadow coverage"
        );
        if let Some(path) = &output_dir {
            let receipt = serde_json::json!({"scene":label,"size":size,"camera":view.camera_pos,"stages":receipts,
            "scope":"Physical high bridge, live HE receiver damage/collapse and ordinary Engineer repair, retained physical atlas, production shadow builder and GPU color/Z. Shadows isolated against white/clear depth. State hashes unchanged by rendering. Native caller parity is tested separately; no native whole-scene, projectile-launch or full lifecycle parity claim."});
            std::fs::write(
                path.join("receipt.json"),
                serde_json::to_vec_pretty(&receipt).unwrap(),
            )
            .unwrap();
        }
    }
}
