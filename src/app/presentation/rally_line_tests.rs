use super::*;
use serde_json::Value;

pub(super) fn rally_native() -> &'static Value {
    static NATIVE: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    NATIVE.get_or_init(|| {
        serde_json::from_str(crate::test_fixture::text(
            "tools/procedural_drawing_oracle/rally.json",
        ))
        .unwrap()
    })
}

fn producer_rows() -> impl Iterator<Item = &'static Value> {
    let native = rally_native();
    native["producer_cases"]
        .as_array()
        .unwrap()
        .iter()
        .chain(native["multi_producer_cases"].as_array().unwrap().iter())
}

fn selected_for_row(row: &Value) -> Vec<u64> {
    row["input"]["selection_order"].as_array().map_or_else(
        || vec![10],
        |order| order.iter().map(|id| 10 + id.as_u64().unwrap()).collect(),
    )
}

pub(super) fn alpha_fixture([x, y]: [i32; 2], mode: &str) -> u8 {
    match mode {
        "clear" => 127,
        "black" => 0,
        _ => [0, 1, 63, 127, 255][((x / 9 + y / 7) % 5) as usize],
    }
}

fn point<const N: usize>(value: &Value) -> [i32; N] {
    std::array::from_fn(|i| value[i].as_i64().unwrap() as i32)
}

fn packed(tint: [f32; 3]) -> u16 {
    let [r, g, b] = tint.map(|c| (c * 255.).round() as u16);
    (r >> 3) << 11 | (g >> 2) << 5 | (b >> 3)
}

fn composite(canvas: &mut [u16], instances: &[SpriteInstance], view: TacticalViewport) {
    for instance in instances {
        let x = instance.position[0] as i32 - view.camera[0];
        let y = instance.position[1] as i32 - view.camera[1];
        for dy in 0..instance.size[1] as i32 {
            for dx in 0..instance.size[0] as i32 {
                canvas[((y + dy) * 160 + x + dx) as usize] = packed(instance.tint);
            }
        }
    }
}

pub(super) fn assert_native_passes(
    actual: &[Vec<SpriteInstance>; 2],
    row: &Value,
    view: TacticalViewport,
) {
    let mut canvas = vec![0x39E7; 160 * 120];
    for (index, pass) in actual.iter().enumerate() {
        composite(&mut canvas, pass, view);
        let pixels: Vec<_> = canvas
            .iter()
            .enumerate()
            .filter(|(_, c)| **c != 0x39E7)
            .map(|(i, c)| [i as i32 % 160, i as i32 / 160, i32::from(*c)])
            .collect();
        let expected: Vec<[i32; 3]> = row["passes"][index]["pixels"]
            .as_array()
            .unwrap()
            .iter()
            .map(point)
            .collect();
        assert!(
            pixels == expected,
            "native producer {} pass {index}: actual/expected lengths {}/{}, first differing stores {:?}",
            row["input"],
            pixels.len(),
            expected.len(),
            pixels
                .iter()
                .zip(&expected)
                .find(|(actual, expected)| actual != expected)
        );
    }
}

#[test]
fn native_patterned_leaf_all_octants_ties_endpoints_and_phases() {
    for row in rally_native()["leaf_cases"].as_array().unwrap() {
        let input = &row["input"];
        let mut pixels = Vec::new();
        crate::render::surface_line::patterned_line(
            point(&input["from_point"]),
            point(&input["to_point"]),
            &RALLY_PATTERN,
            input["phase"].as_i64().unwrap() as i32,
            |p| {
                if usize::from(alpha_fixture(p, "mixed") == 0)
                    == input["pass_id"].as_u64().unwrap() as usize
                {
                    pixels.push([p[0], p[1], 0xF941]);
                }
            },
        );
        pixels.sort_by_key(|p| (p[1], p[0]));
        let expected: Vec<[i32; 3]> = row["pixels"]
            .as_array()
            .unwrap()
            .iter()
            .map(point)
            .collect();
        assert_eq!(pixels, expected, "native leaf {input}");
    }
}

pub(super) fn projected_fixture(
    row: &Value,
    zoom: f32,
    camera: [i32; 2],
) -> ([Vec<SpriteInstance>; 2], TacticalViewport) {
    let view = TacticalViewport {
        camera,
        clip: point(&row["clip"]),
        zoom,
    };
    let mut actual = [Vec::new(), Vec::new()];
    for rows in row["passes"][0]["clips"].as_array().unwrap().chunks(3) {
        let clip = &rows[0];
        let mut from = point(&clip["from_point"]);
        let mut to = point(&clip["to_point"]);
        from[1] -= 2;
        to[1] -= 2;
        let input = &row["input"];
        emit_rally_line(
            &mut actual,
            from,
            to,
            [248. / 255., 40. / 255., 8. / 255.],
            input["frame"].as_u64().unwrap_or(0) as u32,
            view,
            |p| alpha_fixture(p, input["alpha"].as_str().unwrap_or("clear")),
        );
    }
    (actual, view)
}

#[test]
fn native_rally_producer_clipped_rows_phase_and_abuffer_passes() {
    assert_eq!(
        rally_native()["producer_cases"].as_array().unwrap().len(),
        65
    );
    assert_eq!(
        rally_native()["multi_producer_cases"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    for row in producer_rows() {
        let (actual, view) = projected_fixture(row, 1., [0, 0]);
        assert_native_passes(&actual, row, view);
    }
}

#[test]
fn rally_crossing_stock_shroud_frontier_matches_the_original_combined_draw() {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/procedural_drawing_oracle/shroud.json",
    ))
    .unwrap();
    let scene = corpus["scenes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|scene| scene["input"]["name"] == "factory_frontier")
        .unwrap();
    // The production ShroudBuffer rebuild is independently checked against
    // these whole6D3660 bytes. Feed the same original A into the actual rally
    // emitter and compare6DA9D0's stores after each pass.
    let alpha = scene["alpha_hex"].as_str().unwrap();
    let alpha: Vec<_> = (0..alpha.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&alpha[i..i + 2], 16).unwrap())
        .collect();
    let row = serde_json::json!({
        "input": scene["input"], "passes": scene["rally_passes"]
    });
    let view = TacticalViewport {
        camera: [0; 2],
        clip: [0, 0, 160, 120],
        zoom: 1.,
    };
    let clip = &row["passes"][0]["clips"][0];
    let mut from: [i32; 2] = point(&clip["from_point"]);
    let mut to: [i32; 2] = point(&clip["to_point"]);
    from[1] -= 2;
    to[1] -= 2;
    let mut actual = [Vec::new(), Vec::new()];
    emit_rally_line(
        &mut actual,
        from,
        to,
        [248. / 255., 40. / 255., 8. / 255.],
        0,
        view,
        |[x, y]| alpha[(y * 160 + x) as usize],
    );
    assert_native_passes(&actual, &row, view);
}

fn simulation_fixture(row: &Value) -> (Simulation, RuleSet) {
    let input = &row["input"];
    let factory = match input["factory"].as_i64().unwrap_or(40) {
        40 => "UnitType",
        16 => "InfantryType",
        _ => "none",
    };
    let rules = RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(&format!(
        "[BuildingTypes]\n0=FACTORY\n[FACTORY]\nFactory={factory}\nUnitRepair={}\nCloning={}\n",
        input["unit_repair"].as_bool().unwrap_or(false),
        input["cloning"].as_bool().unwrap_or(false)
    )))
    .unwrap();
    let foundation = format!(
        "{}x{}",
        row["foundation_size"][0], row["foundation_size"][1]
    );
    (
        simulation_fixture_for_type(row, "FACTORY", &foundation, "Americans", 10),
        rules,
    )
}

fn simulation_fixture_for_type(
    row: &Value,
    type_name: &str,
    foundation: &str,
    owner: &str,
    first_id: u64,
) -> Simulation {
    use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
    use crate::sim::components::DriveCoord;
    let input = &row["input"];
    let mut sim = Simulation::new();
    let mut entity = GameEntity::test_default_of_category(
        first_id,
        type_name,
        owner,
        10,
        20,
        EntityCategory::Structure,
    );
    entity.type_ref = sim.interner.intern(type_name);
    entity.owner = sim.interner.intern(owner);
    entity.selected = input["selected"].as_bool().unwrap_or(true);
    entity.lifecycle.object_alive = input["alive"].as_bool().unwrap_or(true);
    entity.foundation = foundation.to_owned();
    let [tx, ty] = input.get("target_cell").map(point).unwrap_or([14, 20]);
    if !input["no_target"].as_bool().unwrap_or(false) {
        if let Some(target) = input.get("target_object") {
            let [x, y, z] = point(target);
            let target_id = first_id + 1;
            let mut target =
                GameEntity::test_default(target_id, "MTNK", owner, tx as u16, ty as u16);
            crate::sim::movement::ground_pose::put_location(
                &mut target.position,
                DriveCoord { x, y, z },
            );
            sim.entities_mut().insert(target);
            entity.set_archive_target(Some(TargetKind::Entity(target_id)));
        } else {
            entity.set_archive_target(Some(TargetKind::Cell(tx as u16, ty as u16)));
        }
    }
    let sources: Vec<_> = input["sources"].as_array().map_or_else(
        || vec![input.get("source").map(point).unwrap_or([2688, 5248, 0])],
        |sources| sources.iter().map(point).collect(),
    );
    for (index, [x, y, z]) in sources.into_iter().enumerate() {
        let mut factory = entity.clone();
        factory.stable_id = first_id + index as u64;
        crate::sim::movement::ground_pose::put_location(
            &mut factory.position,
            DriveCoord { x, y, z },
        );
        let coords = crate::sim::movement::ground_pose::object_get_coords(&factory, None);
        let expected = if input.get("sources").is_some() {
            &row["source_coords"][index]
        } else {
            &row["source_coords"]
        };
        assert_eq!([coords.x, coords.y, coords.z], point::<3>(expected));
        sim.entities_mut().insert(factory);
    }
    let width = u16::try_from(tx + 1).unwrap().max(32);
    let height = u16::try_from(ty + 1).unwrap().max(32);
    let mut cells: Vec<_> = (0..height)
        .flat_map(|y| (0..width).map(move |x| ResolvedTerrainCell::clear_for_test(x, y)))
        .collect();
    let target = cells
        .iter_mut()
        .find(|c| i32::from(c.rx) == tx && i32::from(c.ry) == ty)
        .unwrap();
    target.level = input["level"].as_u64().unwrap_or(0) as u8;
    target.slope_type = input["slope"].as_u64().unwrap_or(0) as u8;
    target.has_bridge_deck = input["flags"].as_u64().unwrap_or(0) & 256 != 0;
    sim.resolved_terrain =
        Some(ResolvedTerrainGrid::from_cells(width, height, cells).test_mark_decks_structural());
    sim.session.binary_frame = input["frame"].as_u64().unwrap_or(0) as u32;
    sim
}

#[test]
fn native_rally_gates_foundation_ground_bridge_and_object_target_through_production_builder() {
    for row in producer_rows() {
        let (sim, rules) = simulation_fixture(row);
        let mut camera = point(&row["camera"]);
        camera[1] += 15; // VERA's shared absolute projection bias, not a rally offset.
        let view = TacticalViewport {
            camera,
            clip: point(&row["clip"]),
            zoom: 1.,
        };
        let mut actual = build_factory_rally_line_instances(
            Some(&sim),
            Some(&rules),
            &selected_for_row(row),
            &HouseColorMap::new(),
            Some(if row["input"]["enemy"].as_bool().unwrap_or(false) {
                "Other"
            } else {
                "Americans"
            }),
            view,
            |p| alpha_fixture(p, row["input"]["alpha"].as_str().unwrap_or("clear")),
        );
        // House RGB is a supplied native boundary. Its RGB565 conversion is
        // checked by the separate native producer + GPU comparisons.
        for instance in actual.iter_mut().flatten() {
            if instance.tint != [0.; 3] {
                instance.tint = [248. / 255., 40. / 255., 8. / 255.];
            }
        }
        assert_native_passes(&actual, row, view);
    }
}

#[test]
fn retail_gapile_house_color_and_production_crop_match_original_rally_and_no_target() {
    use crate::rules::color_scheme::scheme_entry_for_priority;
    use crate::rules::foundation::{foundation_dimensions, foundation_id};
    use crate::rules::house_colors::HouseColorIndex;
    use crate::rules::object_type::FactoryType;

    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    // The fixture owner loads physical startup/Battle/map layers and installs
    // fixed ART. No synthetic FACTORY type or replacement tint enters this test.
    let rules = &retail.rules;
    let stock = &rally_native()["stock_type_inputs"];
    let type_name = stock["result"]["type_id"].as_str().unwrap();
    let object = rules.object(type_name).unwrap();
    assert_eq!(
        object.factory,
        FactoryType::from_ini(
            stock["physical_files"]["rules"]["keys"]["Factory"]
                .as_str()
                .unwrap()
        )
    );
    assert_eq!(
        serde_json::json!(foundation_id(&object.foundation)),
        stock["result"]["foundation"]
    );
    assert_eq!(
        serde_json::json!(u8::from(object.unit_repair)),
        stock["result"]["unit_repair"]
    );
    assert_eq!(
        serde_json::json!(u8::from(object.cloning)),
        stock["result"]["cloning"]
    );
    assert_eq!(
        serde_json::json!(object.has_rally_line()),
        stock["result"]["has_rally_point"]
    );

    let rows = rally_native()["production_crop_cases"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    let mut target_states = std::collections::BTreeSet::new();
    for row in rows {
        let input = &row["input"];
        let owner = input["production_owner"].as_str().unwrap();
        let id = input["production_actor_id"].as_u64().unwrap();
        let no_target = input["no_target"].as_bool().unwrap();
        target_states.insert(no_target);
        assert_eq!(input["factory"], stock["result"]["factory"]);
        assert_eq!(input["foundation"], stock["result"]["foundation"]);
        assert_eq!(input["unit_repair"], serde_json::json!(object.unit_repair));
        assert_eq!(input["cloning"], serde_json::json!(object.cloning));
        let (width, height) = foundation_dimensions(&object.foundation);
        assert_eq!(
            [i32::from(width), i32::from(height)],
            point(&row["foundation_size"])
        );

        let entry =
            scheme_entry_for_priority(input["production_color_priority"].as_i64().unwrap() as i32);
        assert_eq!(
            rules.color_schemes[entry].name,
            input["color_scheme_name"].as_str().unwrap()
        );
        let color = HouseColorIndex(u8::try_from(entry).unwrap());
        let colors = HouseColorMap::from([(owner.to_owned(), color)]);
        assert_eq!(
            crate::render::palette_light::house_color_rgb(&rules.house_color_ramps, color)
                .map(i32::from),
            point(&input["rgb"])
        );

        // Original6DA9D0 receives prepared Scenario fields recorded beside the
        // capture hashes: entity/target state, frame, camera and clear A. The
        // null Archive row is the observed post-Stop state, not a Stop execution
        // claim; Building455D50's separate destination corpus covers that write.
        let sim = simulation_fixture_for_type(row, type_name, &object.foundation, owner, id);
        let entity = sim.entities().get(id).unwrap();
        assert_eq!(
            [i32::from(entity.position.rx), i32::from(entity.position.ry)],
            point(&input["production_cell"])
        );
        assert_eq!(entity.archive_target().is_none(), no_target);
        let crop: [i32; 2] = point(&input["crop_origin"]);
        let production_camera: [i32; 2] = point(&input["production_camera"]);
        let camera = std::array::from_fn(|axis| production_camera[axis] + crop[axis]);
        let mut native_camera: [i32; 2] = point(&row["camera"]);
        native_camera[1] += input["world_y_bias"].as_i64().unwrap() as i32;
        assert_eq!(camera, native_camera);
        assert_eq!(input["production_zoom"], 1);
        assert_eq!(input["alpha"], "clear");
        let view = TacticalViewport {
            camera,
            clip: point(&row["clip"]),
            zoom: 1.,
        };
        let actual = build_factory_rally_line_instances(
            Some(&sim),
            Some(rules),
            &[id],
            &colors,
            Some(owner),
            view,
            |p| alpha_fixture(p, input["alpha"].as_str().unwrap()),
        );
        assert_native_passes(&actual, row, view);
        assert_eq!(actual.iter().all(Vec::is_empty), no_target);
    }
    assert_eq!(
        target_states,
        std::collections::BTreeSet::from([false, true])
    );
}

#[test]
fn presentation_target_query_does_not_stamp_simulation_dummy_cell() {
    let row = &rally_native()["producer_cases"][0];
    let (mut sim, rules) = simulation_fixture(row);
    sim.entities_mut()
        .get_mut(10)
        .unwrap()
        .set_archive_target(Some(TargetKind::Cell(u16::MAX, u16::MAX)));
    let terrain = sim.resolved_terrain.as_ref().unwrap();
    let canonical = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
    let before = canonical.dummy().snapshot().coord;
    let _ = build_factory_rally_line_instances(
        Some(&sim),
        Some(&rules),
        &[10],
        &HouseColorMap::new(),
        Some("Americans"),
        TacticalViewport {
            camera: [0, 0],
            clip: [0, 0, 160, 120],
            zoom: 1.,
        },
        |_| 127,
    );
    assert_eq!(canonical.dummy().snapshot().coord, before);
}

#[test]
#[ignore = "requires GPU; compares original complete rally pixels and unchanged depth"]
fn production_rally_gpu_matches_native_pixels_and_preserves_depth() {
    use crate::render::batch::BatchRenderer;
    use crate::render::terrain_draw_gpu_tests::{Gpu, camera, clear, encoded};
    use wgpu::util::DeviceExt;
    let gpu = Gpu::new();
    for format in [
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ] {
        for zoom in [0.75, 1., 1.5, 2.] {
            let size = [(160. * zoom) as u32, (120. * zoom) as u32];
            let camera_pos = [-17, 73];
            let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
            batch.write_camera(
                &gpu.queue,
                crate::render::batch::CameraUniform {
                    camera_pos: camera_pos.map(|v| v as f32),
                    zoom,
                    ..camera(size)
                },
            );
            let texture =
                batch.create_texture_on_device(&gpu.device, &gpu.queue, &[255; 4], 1, 1, None);
            let color = gpu.target(size, format);
            let cv = color.create_view(&Default::default());
            let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
            let dv = depth.create_view(&Default::default());
            for row in producer_rows() {
                let (mut actual, _) = projected_fixture(row, zoom, camera_pos);
                // Poison unused hardware depth: both reads and writes must be bypassed.
                for instance in actual.iter_mut().flatten() {
                    instance.depth = 0.9;
                }
                let background = encoded(0x39E7, format).repeat((size[0] * size[1]) as usize);
                gpu.queue.write_texture(
                    color.as_image_copy(),
                    &background,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(size[0] * 4),
                        rows_per_image: Some(size[1]),
                    },
                    color.size(),
                );
                for (pass_id, instances) in actual.iter().enumerate() {
                    let buffer = (!instances.is_empty()).then(|| {
                        gpu.device
                            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                                label: Some("Production rally spans"),
                                contents: bytemuck::cast_slice(instances),
                                usage: wgpu::BufferUsages::VERTEX,
                            })
                    });
                    let mut encoder = gpu.device.create_command_encoder(&Default::default());
                    clear(&mut encoder, &cv, &dv, 12345, wgpu::LoadOp::Load);
                    if let Some(buffer) = &buffer {
                        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("Native rally ABuffer pass"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: &cv,
                                depth_slice: None,
                                resolve_target: None,
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
                            timestamp_writes: None,
                            occlusion_query_set: None,
                        });
                        batch.draw_with_buffer_ui_passthrough(
                            &mut pass,
                            &texture,
                            buffer,
                            instances.len() as u32,
                        );
                    }
                    let reads = [
                        gpu.read(&mut encoder, &color),
                        gpu.read(&mut encoder, &depth),
                    ];
                    let output = gpu.finish(encoder, &reads, size);
                    let mut native_words = vec![0x39E7_u16; 160 * 120];
                    for pixel in row["passes"][pass_id]["pixels"].as_array().unwrap() {
                        let [x, y, word] = point(pixel);
                        native_words[y as usize * 160 + x as usize] = word as u16;
                    }
                    let expected: Vec<u8> = (0..size[1])
                        .flat_map(|y| {
                            let native_words = &native_words;
                            (0..size[0]).flat_map(move |x| {
                                let sx = ((x as f32 + 0.5) / zoom).floor() as usize;
                                let sy = ((y as f32 + 0.5) / zoom).floor() as usize;
                                encoded(native_words[sy * 160 + sx], format)
                            })
                        })
                        .collect();
                    assert_eq!(
                        output[0], expected,
                        "{format:?} zoom{zoom}, {} pass{pass_id}",
                        row["input"]
                    );
                    assert!(
                        output[1]
                            .chunks_exact(4)
                            .all(
                                |bytes| crate::render::native_z::stored_z(f32::from_le_bytes(
                                    bytes.try_into().unwrap()
                                )) == 12345
                            ),
                        "rally changed Z: {format:?}, {} pass{pass_id}",
                        row["input"]
                    );
                }
            }
        }
    }
}

#[test]
#[ignore = "bounded CPU/GPU timing for rally/Move/Attack builders, pooled upload and draw"]
fn production_rally_workload_timing() {
    use crate::render::batch::{BatchRenderer, InstanceBufferPool};
    use crate::render::terrain_draw_gpu_tests::{Gpu, camera};
    use crate::sim::components::DriveCoord;
    use crate::sim::movement::ground_pose;
    use std::time::{Duration, Instant};
    let gpu = Gpu::with_features(wgpu::Features::TIMESTAMP_QUERY);
    let size = [800, 600];
    let format = wgpu::TextureFormat::Bgra8UnormSrgb;
    let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
    let view = TacticalViewport {
        camera: [-400, 350],
        clip: [0, 0, 632, 568],
        zoom: 1.,
    };
    batch.write_camera(
        &gpu.queue,
        crate::render::batch::CameraUniform {
            camera_pos: view.camera.map(|v| v as f32),
            ..camera(size)
        },
    );
    let texture = batch.create_texture_on_device(&gpu.device, &gpu.queue, &[255; 4], 1, 1, None);
    let color = gpu.target(size, format);
    let cv = color.create_view(&Default::default());
    let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
    let dv = depth.create_view(&Default::default());
    let queries = gpu.device.create_query_set(&wgpu::QuerySetDescriptor {
        label: Some("Rally timing"),
        ty: wgpu::QueryType::Timestamp,
        count: 2,
    });
    let resolved = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Rally timestamp resolve"),
        size: 256,
        usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let mapped = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Rally timestamp read"),
        size: 16,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let colors = HouseColorMap::new();
    let mut pool = InstanceBufferPool::new();
    // Both production builders use this one upload/encode/query/readback owner.
    // A single action-line batch must not draw a retained second rally buffer.
    let mut measure_draw = |batches: &[&[SpriteInstance]], clip: [i32; 4]| {
        const KEYS: [&str; 2] = ["procedural_first", "procedural_second"];
        assert!(batches.len() <= KEYS.len());
        let upload_started = Instant::now();
        for (&key, instances) in KEYS.iter().zip(batches) {
            pool.upload_on_device(&gpu.device, &gpu.queue, key, instances);
        }
        let upload_ms = upload_started.elapsed().as_secs_f64() * 1000.;
        let encode_started = Instant::now();
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        let mut draw_calls = 0;
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Procedural line workload"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &cv,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &dv,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0.2),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                // wgpu-hal27.0.4 Metal attaches these to vertex-start and
                // fragment-end; empty encoder samples returned zero on M4.
                // Upstream af91efa9, metal/command.rs:630..651.
                timestamp_writes: Some(wgpu::RenderPassTimestampWrites {
                    query_set: &queries,
                    beginning_of_pass_write_index: Some(0),
                    end_of_pass_write_index: Some(1),
                }),
                occlusion_query_set: None,
            });
            pass.set_scissor_rect(
                clip[0] as u32,
                clip[1] as u32,
                clip[2] as u32,
                clip[3] as u32,
            );
            for &key in KEYS.iter().take(batches.len()) {
                if let Some((buffer, count)) = pool.get(key) {
                    batch.draw_with_buffer_ui_passthrough(&mut pass, &texture, buffer, count);
                    draw_calls += 1;
                }
            }
        }
        encoder.resolve_query_set(&queries, 0..2, &resolved, 0);
        encoder.copy_buffer_to_buffer(&resolved, 0, &mapped, 0, 16);
        let command = encoder.finish();
        let encode_ms = encode_started.elapsed().as_secs_f64() * 1000.;
        let submitted = Instant::now();
        let submission = gpu.queue.submit([command]);
        let submit_ms = submitted.elapsed().as_secs_f64() * 1000.;
        let (tx, rx) = std::sync::mpsc::channel();
        mapped
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
        // wgpu27 PollType::Wait(Some(submission)) waits for execution and
        // callbacks on native backends. This is completion wall time including
        // submission, queued uploads, pass work, query copy and mapping, not
        // a render-only GPU interval. API: wgpu-types27.0.1 PollType::Wait;
        // https://docs.rs/wgpu/27.0.1/wgpu/type.PollType.html
        gpu.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(Duration::from_secs(60)),
            })
            .unwrap();
        rx.recv().unwrap().unwrap();
        let completed_ms = submitted.elapsed().as_secs_f64() * 1000.;
        let data = mapped.slice(..).get_mapped_range();
        let first = u64::from_le_bytes(data[..8].try_into().unwrap());
        let last = u64::from_le_bytes(data[8..].try_into().unwrap());
        // Retain unsupported samples explicitly, never report zero as a
        // measured GPU cost. The completion clock above is independent.
        let gpu_ms = (first != 0 && last != u64::MAX && last > first).then(|| {
            (last - first) as f64 * f64::from(gpu.queue.get_timestamp_period()) / 1_000_000.
        });
        drop(data);
        mapped.unmap();
        let spans_per_batch: Vec<_> = batches.iter().map(|instances| instances.len()).collect();
        let spans = spans_per_batch.iter().sum::<usize>();
        serde_json::json!({
            "upload_ms":upload_ms, "encode_ms":encode_ms,
            "encode_upload_ms":upload_ms + encode_ms,
            "submit_ms":submit_ms, "completed_ms":completed_ms, "gpu_ms":gpu_ms,
            "timestamp_first":first, "timestamp_last":last,
            "timestamp_period_ns":gpu.queue.get_timestamp_period(),
            "spans_per_batch":spans_per_batch, "spans":spans,
            "upload_calls":batches.iter().filter(|instances| !instances.is_empty()).count(),
            "draw_calls":draw_calls,
            "upload_bytes":spans * std::mem::size_of::<SpriteInstance>(),
        })
    };
    let mut results = Vec::new();
    for (count, mobile, missing, pending) in [
        (1, false, false, false),
        (64, false, false, false),
        (1024, false, false, false),
        (20_000, true, false, false),
        (20_000, true, true, false),
        (20_000, true, false, true),
    ] {
        let (mut sim, rules) = simulation_fixture(&rally_native()["producer_cases"][0]);
        let rules = if mobile {
            RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
                "[VehicleTypes]\n0=MTNK\n[MTNK]\nPrimary=TankGun\n[TankGun]\nDamage=10\n",
            ))
            .unwrap()
        } else {
            rules
        };
        let mut template = sim.entities().get(10).unwrap().clone();
        if mobile {
            template.category = EntityCategory::Unit;
            template.type_ref = sim.interner.intern("MTNK");
        }
        template.selected = !pending;
        let ledger: Vec<_> = (10..10 + count as u64).collect();
        for &id in &ledger {
            let mut entity = template.clone();
            entity.stable_id = id;
            entity.position.rx += (id % 8) as u16;
            entity.position.ry += (id % 5) as u16;
            sim.entities_mut().insert(entity);
        }
        let mut samples = Vec::new();
        for sample in 0..22 {
            sim.session.binary_frame = sample;
            let begin = Instant::now();
            let selected = crate::app::input::dispatch::selected_stable_ids_in_order(
                Some(&sim),
                Some(&rules),
                if missing { &[] } else { &ledger },
                pending,
            );
            let selection_ms = begin.elapsed().as_secs_f64() * 1000.;
            let actual = build_factory_rally_line_instances(
                Some(&sim),
                Some(&rules),
                &selected,
                &colors,
                Some("Americans"),
                view,
                |p| alpha_fixture(p, "mixed"),
            );
            let build_ms = begin.elapsed().as_secs_f64() * 1000.;
            if mobile {
                assert!(actual.iter().all(Vec::is_empty));
                assert_eq!(selected.len(), count);
                if missing {
                    assert!(selected.iter().eq(ledger.iter().rev()));
                } else {
                    assert_eq!(selected, ledger);
                }
            }
            let mut timing = measure_draw(&[&actual[0], &actual[1]], view.clip);
            if sample >= 2 {
                timing["selection_ms"] = serde_json::json!(selection_ms);
                timing["build_ms"] = serde_json::json!(build_ms);
                timing["line_builder_ms"] = serde_json::json!(build_ms - selection_ms);
                samples.push(timing);
            }
        }
        results.push(
            serde_json::json!({ "family":"rally", "selected_count": count, "mobile":mobile,
            "missing_ledger":missing, "pending_selection":pending, "samples": samples }),
        );
    }
    for (kind, (count, selected, active)) in [SelectedLineKind::Move, SelectedLineKind::Attack]
        .into_iter()
        .flat_map(|kind| {
            [
                (1, true, true),
                (64, true, true),
                (1024, true, true),
                (20_000, true, true),
                (20_000, true, false),
                (20_000, false, true),
            ]
            .into_iter()
            .map(move |workload| (kind, workload))
        })
    {
        let (mut sim, rules, mut line_state, mut view, palette) =
            super::action_tests::workload_fixture(kind);
        view.clip = [0, 0, 632, 568];
        batch.write_camera(
            &gpu.queue,
            crate::render::batch::CameraUniform {
                camera_pos: view.camera.map(|v| v as f32),
                ..camera(size)
            },
        );
        let template = sim.entities().get(1).unwrap().clone();
        let source = ground_pose::position_world_coord(&template.position);
        // An overlapping synthetic stress field: disperse raw source XY by
        // 8-lepton increments within one cell, retain source Z and the shared
        // Cell NavCom or live Unit TarCom. All sources remain visible; line
        // lengths vary slightly. The Attack target retains its native fixture
        // motion/speed/facing inputs without advancing a gameplay tick.
        for id in 1..=count as u64 {
            let mut entity = template.clone();
            entity.stable_id = id;
            entity.selected = selected;
            ground_pose::put_location(
                &mut entity.position,
                DriveCoord {
                    x: source.x + 8 * ((id - 1) % 32) as i32,
                    y: source.y + 8 * (((id - 1) / 32) % 32) as i32,
                    z: source.z,
                },
            );
            sim.entities_mut().insert(entity);
        }
        let mut samples = Vec::new();
        for sample in 0..22 {
            let frame = 100 + sample;
            sim.session.binary_frame = frame;
            line_state.start_timer(if active {
                frame
            } else {
                frame.wrapping_sub(DURATION_TICKS as u32)
            });
            assert_eq!(line_state.is_selected_action_active(frame), active);
            let begin = Instant::now();
            let actual = build_target_line_instances(
                &line_state,
                Some(&sim),
                rules.as_ref(),
                Some(&palette),
                view,
            );
            let build_ms = begin.elapsed().as_secs_f64() * 1000.;
            assert_eq!(!actual.is_empty(), active && selected);
            let mut timing = measure_draw(&[&actual], view.clip);
            if sample >= 2 {
                // This production builder scans Techno entities itself; it
                // does not acquire the rally CurrentObjects selection ledger.
                timing["selection_ms"] = serde_json::Value::Null;
                timing["build_ms"] = serde_json::json!(build_ms);
                timing["line_builder_ms"] = serde_json::json!(build_ms);
                samples.push(timing);
            }
        }
        results.push(serde_json::json!({
            "family":match kind { SelectedLineKind::Move => "move", SelectedLineKind::Attack => "attack" },
            "actor_count":count + usize::from(kind == SelectedLineKind::Attack),
            "selected_count":if selected { count } else { 0 }, "timer_active":active,
            "camera":view.camera, "clip":view.clip,
            "source_xy_stride_leptons":8, "source_xy_grid_side":32,
            "samples":samples,
        }));
    }
    let report = serde_json::json!({ "schema":"vera20k.procedural-line-workload.v2",
        "target": size, "format": "Bgra8UnormSrgb", "zoom":1,
        "interval":"CPU build_ms includes the actual selection owner for rally (selection_ms subset) and the production builder. Move/Attack build_ms includes the full entity scan, live coordinate queries and palette conversion; selection_ms is null. upload_ms measures pooled CPU staging; encode_ms includes pass/query-copy encoding and finish, and encode_upload_ms is their sum. submit_ms is the included CPU queue-submit subset of completed_ms. gpu_ms is the backend-reported render-pass timestamp interval (including attachment clears), excludes uploads; unavailable timestamps stay null. completed_ms independently measures submit through upload, GPU, query-copy and map completion and is not render-only GPU time. Two warmups, twenty measured frames; timer preparation and source construction occur before timing. Synthetic overlapping stress; not whole-game FPS or native timing parity.",
        "workloads":results });
    eprintln!("{}", serde_json::to_string_pretty(&report).unwrap());
    if let Some(path) = std::env::var_os("VERA20K_PROCEDURAL_LINE_PERF_OUTPUT")
        .or_else(|| std::env::var_os("VERA20K_RALLY_PERF_OUTPUT"))
    {
        std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    }
}
