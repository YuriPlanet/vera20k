//! Original Wave0/3 spans/packed pixels through the retained Air Display body,
//! shared SurfaceLines snapshots and real GPU output. Component inputs are
//! supplied native state; producer/lift admission is tested by its sim owner.

use crate::app::presentation::render::draw_plan_lowering::{
    NativeDisplayOrder, ObjectBodyDraw, PlannedObjectInstance, lower_object_instances,
};
use crate::app::presentation::render::merge_passes::draw_native_object_pass;
use crate::render::batch::BatchRenderer;
use crate::render::native_surface_format::{DirectDrawPixelFormat, RGB555, RGB565};
use crate::render::shroud_buffer::ShroudBuffer;
use crate::render::surface_line::{SurfaceDraw, SurfaceLineViewport};
use crate::render::tactical_draw_plan::SpriteEncoding;
use crate::render::terrain_draw::TerrainDrawRenderer;
use crate::render::terrain_draw_gpu_tests::{Gpu, camera, encoded, seed_depth_grid};
use crate::render::wave_geometry::{
    WaveClipPath, WaveDraw, WavePixel, WavePolygon, WaveRasterError, polygon_spans, wave_tables,
};
use crate::sim::world::display_layers::{DisplayLayer, DisplayLayers};
use crate::util::sha256::sha256_hex;
use serde_json::Value;

fn corpus() -> &'static Value {
    crate::test_fixture::json("tools/procedural_drawing_oracle/magnetic_wave.json")
}

fn sonic_corpus() -> &'static Value {
    crate::test_fixture::json("tools/procedural_drawing_oracle/sonic_wave.json")
}

fn integer(value: &Value) -> i32 {
    value.as_i64().unwrap().try_into().unwrap()
}

fn array<const N: usize>(value: &Value) -> [i32; N] {
    std::array::from_fn(|i| integer(&value[i]))
}

fn words(value: &Value) -> Vec<u16> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|v| integer(v).try_into().unwrap())
        .collect()
}

fn hex(value: &Value) -> Vec<u8> {
    let text = value.as_str().unwrap();
    assert_eq!(text.len() % 2, 0);
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect()
}

fn word_hash(words: &[u16]) -> String {
    sha256_hex(
        &words
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>(),
    )
}

fn draw(value: &Value) -> WaveDraw {
    WaveDraw {
        polygon: if value["wave_type"] == 0 {
            WavePolygon::Sonic(std::array::from_fn(|i| array(&value["polygon"][i])))
        } else {
            WavePolygon::Magnetic(std::array::from_fn(|i| array(&value["polygon"][i])))
        },
        source_point: array(&value["source_point"]),
        source_z: integer(&value["source_z"]),
        phase: integer(&value["phase"]),
        direction: integer(&value["direction"]).try_into().unwrap(),
        clip_path: match value["clip_path"].as_str().unwrap() {
            "fast" => WaveClipPath::Fast,
            "fallback" => WaveClipPath::Fallback,
            other => panic!("unknown native clip branch {other}"),
        },
    }
}

fn format(input: &Value) -> DirectDrawPixelFormat {
    match input["format"].as_str().unwrap() {
        "rgb565" => RGB565,
        "rgb555" => RGB555,
        other => panic!("unknown native surface format {other}"),
    }
}

fn expected_words(row: &Value) -> Vec<u16> {
    let input = &row["input"];
    let size = array::<2>(&input["size"]);
    let mut expected = words(&input["background_words"]);
    assert_eq!(expected.len(), (size[0] * size[1]) as usize);
    for changed in row["after"]["pixels"].as_array().unwrap() {
        let [x, y, word] = array::<3>(changed);
        expected[(y * size[0] + x) as usize] = word.try_into().unwrap();
    }
    assert_eq!(
        word_hash(&expected),
        row["after"]["pixel_sha256"].as_str().unwrap(),
        "{} lossless native whole-plane expansion",
        input["name"]
    );
    expected
}

fn rows() -> impl Iterator<Item = &'static Value> {
    corpus()["raster_rows"].as_array().unwrap().iter().chain(
        sonic_corpus()["raster_rows"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["source_domain"] != "out_of_plane"),
    )
}

#[test]
fn original_wave_tables_and_integer_polygon_spans_are_consumed() {
    let tables = wave_tables();
    for native in [&corpus()["tables"], &sonic_corpus()["tables"]] {
        assert_eq!(
            tables
                .distance
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
            hex(&native["distance"]["hex"]),
            "whole original75F020 distance table"
        );
        assert_eq!(
            tables
                .magnetic
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
            hex(&native["magnetic"]["hex"]),
            "whole original75F020 magnetic table"
        );
        assert_eq!(
            (0..1002)
                .flat_map(|i| tables.magnetic_sine(i).unwrap().to_le_bytes())
                .collect::<Vec<_>>(),
            hex(&native["magnetic_window"]["hex"]),
            "original magnetic +75E8F0 zero coordinate +Sonic initialized read window"
        );
        assert_eq!(
            tables.brightness.to_vec(),
            native["brightness"]["values"]
                .as_array()
                .unwrap()
                .iter()
                .map(integer)
                .collect::<Vec<_>>()
        );
    }
    let mut directions = [false; 8];
    let mut kinds = [false; 2];
    let mut paths = [false; 2];
    for row in rows() {
        for value in row["draws"].as_array().unwrap() {
            let wave = draw(value);
            directions[wave.direction as usize] = true;
            kinds[usize::from(matches!(wave.polygon, WavePolygon::Sonic(_)))] = true;
            paths[usize::from(wave.clip_path == WaveClipPath::Fallback)] = true;
            let spans = polygon_spans(wave.polygon.points());
            let expected = &value["spans"];
            let expected_rows = expected["rows"]
                .as_array()
                .unwrap()
                .iter()
                .map(array::<2>)
                .collect::<Vec<_>>();
            assert_eq!(
                spans.rows, expected_rows,
                "{} original63F080/63F2E0",
                row["input"]["name"]
            );
            if !spans.rows.is_empty() {
                assert_eq!(spans.top, integer(&expected["top"]));
            }
        }
    }
    assert!(
        directions.into_iter().all(|covered| covered),
        "all8 native constructor directions"
    );
    assert!(
        paths.into_iter().all(|covered| covered),
        "native fast3 and fallback2 clips"
    );
    assert!(
        kinds.into_iter().all(|covered| covered),
        "both native Wave bodies"
    );
}

#[test]
fn original_pixel_linear_aliases_and_initialized_adjacent_window_are_consumed() {
    for row in corpus()["lookup_rows"].as_array().unwrap() {
        let input = &row["input"];
        let point = array::<2>(&input["destination"]);
        // Pixel760190 takes relative lookup coordinates independently of its
        // supplied destination pointer. Express those declared relative inputs
        // at that destination for the shared raster leaf; no Z admission here.
        let wave = WaveDraw {
            polygon: WavePolygon::Magnetic([[0; 2]; 4]),
            source_point: [
                point[0] - integer(&input["delta_x"]),
                point[1] - integer(&input["abs_y"]),
            ],
            source_z: 0,
            phase: integer(&input["phase"]),
            direction: integer(&input["direction"]).try_into().unwrap(),
            clip_path: WaveClipPath::Fast,
        };
        let pixel = wave
            .pixel_operation(
                point,
                array(&input["size"]),
                integer(&input["pitch_words"]),
                0,
            )
            .unwrap();
        let native = &row["operation"];
        assert_eq!(
            pixel.distance_index,
            integer(&native["distance_index"]),
            "{input}"
        );
        assert_eq!(pixel.sine_index, integer(&native["sine_index"]), "{input}");
        assert_eq!(pixel.brightness, integer(&native["brightness"]), "{input}");
        assert_eq!(pixel.source, array::<2>(&native["source"]), "{input}");
    }
}

#[test]
fn original_wave_traversal_displaced_reads_and_depth_admission_match() {
    let mut formats = [false; 2];
    for row in rows() {
        let input = &row["input"];
        let name = &input["name"];
        let size = array::<2>(&input["size"]);
        let pixel_format = format(input);
        formats[usize::from(pixel_format == RGB555)] = true;
        let z = words(&input["z_words"]);
        let alpha = words(&input["alpha_words"]);
        let mut output = words(&input["background_words"]);
        assert_eq!(z.len(), output.len());
        assert_eq!(alpha.len(), output.len());
        let operations = row["operations"].as_array().unwrap();
        let mut admitted = Vec::new();
        for value in row["draws"].as_array().unwrap() {
            draw(value)
                .rasterize(
                    array(&input["clip"]),
                    size,
                    integer(&input["pitch_words"]),
                    integer(&input["z_origin_y"]),
                    |pixel| {
                        let index = (pixel.point[1] * size[0] + pixel.point[0]) as usize;
                        if pixel.candidate_z < z[index] {
                            let source =
                                output[(pixel.source[1] * size[0] + pixel.source[0]) as usize];
                            // Replay the native store only to expose the next Wave's
                            // actual source word. The production WGSL channel gain
                            // is compared with native full planes by the GPU test.
                            let native = &operations[admitted.len()];
                            let after = integer(&native["destination_word"]) as u16;
                            output[index] = after;
                            admitted.push((pixel, source, after));
                        }
                    },
                )
                .unwrap_or_else(|error| {
                    panic!("{name} actual native lookup/source domain: {error:?}")
                });
        }
        assert_eq!(
            output,
            expected_words(row),
            "{name} original Sonic/magnetic full plane"
        );
        assert_eq!(
            admitted.len(),
            operations.len(),
            "{name} native admitted count"
        );
        for ((pixel, source, after), native) in admitted.iter().zip(operations) {
            let expected = WavePixel {
                point: array(&native["point"]),
                source: array(&native["source"]),
                candidate_z: integer(&native["candidate_z"]).try_into().unwrap(),
                brightness: integer(&native["brightness"]),
                distance_index: integer(&native["distance_index"]),
                sine_index: integer(&native["sine_index"]),
            };
            assert_eq!(
                *pixel, expected,
                "{name} native traversal/lookup/read address"
            );
            assert_eq!(*source, integer(&native["source_word"]) as u16);
            assert_eq!(*after, integer(&native["destination_word"]) as u16);
        }
        for key in ["z", "alpha"] {
            assert_eq!(
                row["after"][format!("{key}_before_sha256")],
                row["after"][format!("{key}_after_sha256")],
                "{name} native unchanged {key}"
            );
        }
        // These full native states establish non-spending by the executed
        //component, not a claim that Rust's whole-app streams were compared.
        for stream in ["main", "scenario", "mapgen"] {
            let before = row["rng_before"][stream].as_str().unwrap();
            assert_eq!(before.len(), 0x3f4 * 2);
            assert_eq!(
                before,
                row["rng_after"][stream].as_str().unwrap(),
                "{name} {stream}"
            );
        }
    }
    assert!(
        formats.into_iter().all(|covered| covered),
        "both native packed formats"
    );
}

#[test]
fn original_sonic_out_of_plane_sample_is_an_explicit_domain_error() {
    let mut rejected = 0;
    for row in sonic_corpus()["raster_rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["source_domain"] == "out_of_plane")
    {
        let input = &row["input"];
        let size = array::<2>(&input["size"]);
        let pitch = integer(&input["pitch_words"]);
        let native = row["operations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|op| {
                let [x, y] = array::<2>(&op["source"]);
                x < 0 || x >= size[0] || y < 0 || y >= size[1]
            })
            .unwrap();
        let [x, y] = array::<2>(&native["source"]);
        let error = draw(&row["draws"][0])
            .rasterize(
                array(&input["clip"]),
                size,
                pitch,
                integer(&input["z_origin_y"]),
                |_| {},
            )
            .unwrap_err();
        assert_eq!(error, WaveRasterError::SourceWord(y * pitch + x));
        rejected += 1;
    }
    assert_eq!(rejected, 1, "the declared original small-surface refusal");
}

#[test]
#[ignore = "requires GPU; original Wave0/3 words, Air Display order, A/Z and bounded source snapshots"]
fn wave_pixels_reach_retained_display_and_shared_gpu_with_immutable_z_and_a() {
    let gpu = Gpu::new();
    for row in rows().filter(|row| row["input"]["format"] == "rgb565") {
        let input = &row["input"];
        let name = &input["name"];
        let size = array::<2>(&input["size"]).map(|v| v.try_into().unwrap());
        let background = words(&input["background_words"]);
        let z = words(&input["z_words"]);
        let alpha = words(&input["alpha_words"]);
        let expected = expected_words(row);
        let waves = row["draws"]
            .as_array()
            .unwrap()
            .iter()
            .map(draw)
            .collect::<Vec<_>>();
        for format in [
            wgpu::TextureFormat::Bgra8UnormSrgb,
            wgpu::TextureFormat::Rgba8UnormSrgb,
        ] {
            let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
            batch.write_camera(&gpu.queue, camera(size));
            let color = gpu.target(size, format);
            let cv = color.create_view(&Default::default());
            let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
            let dv = depth.create_view(&Default::default());
            let mut terrain = TerrainDrawRenderer::new(&gpu.device, &gpu.queue, format, &batch);
            terrain.prepare(&gpu.device, &color, &dv, batch.camera_uniform());
            let a = ShroudBuffer::fixture(
                &gpu.device,
                &gpu.queue,
                size,
                [0.; 2],
                &alpha
                    .iter()
                    .map(|v| (*v).try_into().unwrap())
                    .collect::<Vec<_>>(),
            );
            let a_before = gpu.read_float_texels(a.gpu_source().0, [0, 0], size);
            for limit in [None, Some(7)] {
                terrain.set_surface_line_operation_limit(limit);
                terrain.begin_surface_lines();
                let mut display = DisplayLayers::default();
                // Opposite storage/ID order cannot replace native Air append order.
                let ids = (1..=waves.len() as u64).rev().collect::<Vec<_>>();
                for &id in &ids {
                    assert!(display.submit(id, Some(DisplayLayer::AIR), &|_| 0));
                }
                let order = NativeDisplayOrder::from_display(&display);
                let mut parents = waves
                    .iter()
                    .zip(&ids)
                    .map(|(&wave, &id)| {
                        let mut parent = PlannedObjectInstance::object(
                            order.object_draw(id, SpriteEncoding::Plain).unwrap(),
                            Vec::new(),
                        );
                        parent.body_draw = Some(ObjectBodyDraw::Surface(SurfaceDraw::Wave(wave)));
                        parent
                    })
                    .collect::<Vec<_>>();
                parents.reverse();
                let mut layers = lower_object_instances(parents);
                let air = &mut layers[3];
                for draws in &air.surface_draws {
                    air.line_batches.push(terrain.prepare_surface_lines(
                        &gpu.device,
                        &gpu.queue,
                        std::iter::empty(),
                        draws.iter().copied(),
                        &[],
                        &[],
                        &[],
                        SurfaceLineViewport {
                            camera: [0; 2],
                            clip: array(&input["clip"]),
                            z_origin_y: integer(&input["z_origin_y"]),
                            zoom: 1.,
                        },
                        |_| false,
                        |_| panic!("Wave must never query A"),
                    ));
                }
                let initial = background
                    .iter()
                    .flat_map(|&word| encoded(word, format))
                    .collect::<Vec<_>>();
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
                let mut encoder = gpu.device.create_command_encoder(&Default::default());
                seed_depth_grid(&gpu, &mut encoder, &dv, &z, size[0]);
                draw_native_object_pass(
                    &mut encoder,
                    &cv,
                    &dv,
                    &mut terrain,
                    [0, 0, size[0], size[1]],
                    &batch,
                    None,
                    air,
                    None,
                    None,
                    None,
                    None,
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
                let expected_color = expected
                    .iter()
                    .flat_map(|&word| encoded(word, format))
                    .collect::<Vec<_>>();
                assert_eq!(
                    output[0], expected_color,
                    "{name} {format:?} limit{limit:?}: native whole color"
                );
                let expected_depth = z
                    .iter()
                    .flat_map(|&word| crate::render::native_z::stored_depth(word).to_le_bytes())
                    .collect::<Vec<_>>();
                assert_eq!(
                    output[1], expected_depth,
                    "{name} {format:?}: whole Z unchanged"
                );
                assert_eq!(
                    gpu.read_float_texels(a.gpu_source().0, [0, 0], size),
                    a_before,
                    "{name} {format:?}: whole A source unchanged"
                );
            }
        }
    }
}
