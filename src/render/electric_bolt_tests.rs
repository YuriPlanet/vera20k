//! Executed original EBolt geometry/Main continuation and packed-surface pixels.
//! Each geometry comparison starts at a recorded native draw boundary; app
//! registry tests separately own lifetime, reverse traversal and scene clearing.

use super::*;
use crate::render::batch::BatchRenderer;
use crate::render::electric_bolt::{ElectricBoltDraw, ElectricBoltPalette};
use crate::render::laser::LaserDraw;
use crate::render::terrain_draw_gpu_tests::{Gpu, camera, clear, encoded, seed_depth_grid};
use crate::sim::projectile::ProjectileCoord;
use crate::sim::rng::{MainRng, SimRng};
use serde_json::Value;

fn native() -> &'static Value {
    static CORPUS: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    CORPUS.get_or_init(|| {
        serde_json::from_str(crate::test_fixture::text(
            "tools/procedural_drawing_oracle/electric_bolt.json",
        ))
        .unwrap()
    })
}

fn drawing_rows() -> impl Iterator<Item = &'static Value> {
    native()["draw_cases"]
        .as_array()
        .unwrap()
        .iter()
        .chain(native()["mixed_cases"].as_array().unwrap())
}

fn integer(value: &Value) -> i32 {
    value.as_i64().unwrap() as i32
}

fn point<const N: usize>(value: &Value) -> [i32; N] {
    std::array::from_fn(|i| integer(&value[i]))
}

fn coord(value: &Value) -> ProjectileCoord {
    let [x, y, z] = point(value);
    ProjectileCoord::new(x, y, z)
}

fn palette() -> ElectricBoltPalette {
    let colors = &native()["palette"]["selected"];
    ElectricBoltPalette {
        ordinary: integer(&colors["10"]) as u16,
        alternate: integer(&colors["5"]) as u16,
        center: integer(&colors["15"]) as u16,
    }
}

fn viewport(input: &Value) -> SurfaceLineViewport {
    let mut camera: [i32; 2] = point(&input["camera"]);
    camera[1] = camera[1].wrapping_add(15); // Shared VERA world-row convention.
    SurfaceLineViewport {
        camera,
        clip: input.get("clip").map_or([0, 0, 160, 120], point),
        z_origin_y: 0,
        zoom: 1.,
    }
}

fn laser(row: &Value) -> Option<LaserDraw> {
    row.get("laser")
        .filter(|laser| !laser.is_null())
        .map(|laser| LaserDraw {
            from: coord(&laser["source"]),
            to: coord(&laser["target"]),
            z_adjust: integer(&laser["z_adjust"]),
            width: integer(&laser["width"]),
            supported: laser["supported"] == 1,
            rgb: point::<3>(&laser["inner"]).map(|value| value as u8),
            age: integer(&laser["age"]),
            duration: integer(&laser["duration"]),
        })
}

fn trails(row: &Value, visit: &Value) -> Vec<LineTrailSegment> {
    let Some(trail) = row.get("trail").filter(|trail| !trail.is_null()) else {
        return Vec::new();
    };
    let head = integer(&trail["head"]) as usize;
    visit["segments"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|line| line["family"] == "trail")
        .enumerate()
        .map(|(offset, line)| {
            // Positions come from original Update556B70; the per-visit
            // intensity is original Draw556C00's argument. App tests own aging.
            let segment = LineTrailSegment {
                from: coord(&trail["ring"][(head + offset) & 31]["xyz"]),
                to: coord(&trail["ring"][(head + offset + 1) & 31]["xyz"]),
                color: point::<3>(&trail["rgb"]).map(|value| value as u8),
                strength: integer(&line["intensity"]),
            };
            let projected = segment.project(viewport(&row["input"]).camera);
            assert_eq!(line["entry"], "0x4beac0");
            assert_eq!(projected.from, point(&line["from_point"]));
            assert_eq!(projected.to, point(&line["to_point"]));
            assert_eq!(
                projected.z_adjust,
                [integer(&line["z_start"]), integer(&line["z_end"])]
            );
            assert_eq!(segment.color, point::<3>(&line["rgb"]).map(|v| v as u8));
            segment
        })
        .collect()
}

/// Feed the recorded live entries immediately before this visit, rather than
/// implementing another lifetime owner in the render test.
fn visit_lines(row: &Value, index: usize) -> (Vec<SurfaceLine>, Vec<bool>, Vec<usize>, String) {
    let visit = &row["visits"][index];
    let main: MainRng =
        SimRng::from_native_state_hex_for_test(visit["rng_before"]["main"].as_str().unwrap())
            .into();
    let mut lines = Vec::new();
    let mut accepted = Vec::new();
    let mut drawn = Vec::new();
    if visit["clear"] != true {
        let before: Vec<&Value> = if index == 0 {
            row["births"]
                .as_array()
                .unwrap()
                .iter()
                .map(|birth| &birth["bolt"])
                .collect()
        } else {
            row["visits"][index - 1]["bolts"]
                .as_array()
                .unwrap()
                .iter()
                .collect()
        };
        let registered: Vec<usize> = if index == 0 {
            (0..before.len()).collect()
        } else {
            row["visits"][index - 1]["registered"]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| value.as_u64().unwrap() as usize)
                .collect()
        };
        for &id in registered.iter().rev() {
            let bolt = before[id];
            if integer(&bolt["decay"]) == 0 {
                continue;
            }
            let draw = ElectricBoltDraw {
                from: coord(&bolt["source"]),
                to: coord(&bolt["target"]),
                z_adjust: integer(&bolt["z_adjust"]),
                phase: integer(&bolt["phase"]),
                alternate_color: bolt["alternate"].as_bool().unwrap(),
            };
            let admitted = draw.lines(
                viewport(&row["input"]),
                palette(),
                &mut main.draws(),
                |line| lines.push(line),
            );
            accepted.push(admitted);
            if admitted {
                drawn.push(id);
            }
        }
    }
    (lines, accepted, drawn, main.native_state_hex())
}

#[test]
fn palette_words_match_original_filesystem_converter() {
    // The pre-existing action-line corpus preserves these same physical
    // PALETTE.PAL bytes; the EBolt producer independently executes Convert.
    let action: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/procedural_drawing_oracle/action_lines.json",
    ))
    .unwrap();
    let source = &action["physical_inputs"]["palettes"]["PALETTE.PAL"];
    let original = native()["palette"]["loads"]
        .as_array()
        .unwrap()
        .iter()
        .find(|load| load["name"] == "PALETTE.PAL")
        .unwrap();
    assert_eq!(source["sha256"], original["sha256"]);
    let bytes: Vec<u8> = source["hex"]
        .as_str()
        .unwrap()
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect();
    let actual = ElectricBoltPalette::from_palette(
        &crate::assets::pal_file::Palette::from_bytes(&bytes).unwrap(),
    );
    assert_eq!(actual, palette());
}

#[test]
fn subdivision_clipping_and_main_continuation_match_original_visits() {
    for row in drawing_rows() {
        let name = row["input"]["name"].as_str().unwrap();
        for (index, visit) in row["visits"].as_array().unwrap().iter().enumerate() {
            let (actual, trace) = if name == "ordinary" && index == 0 {
                crate::sim::rng::trace_draws(|| visit_lines(row, index))
            } else {
                (visit_lines(row, index), Vec::new())
            };
            let (lines, accepted, drawn, main) = actual;
            let expected: Vec<SurfaceLine> = visit["segments"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|line| line["family"].is_null() || line["family"] == "bolt")
                .map(|line| {
                    assert_eq!(line["entry"], "0x4bfd30");
                    assert_eq!(line["write_z"], 0);
                    SurfaceLine {
                        from: point(&line["from_point"]),
                        to: point(&line["to_point"]),
                        z_adjust: [integer(&line["z_start"]), integer(&line["z_end"])],
                        blend: SurfaceLineBlend::Replace(integer(&line["color"]) as u16),
                    }
                })
                .collect();
            assert_eq!(lines, expected, "{name} visit{index} line arguments");
            assert_eq!(
                accepted,
                visit["clips"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|clip| clip["accepted"].as_bool().unwrap())
                    .collect::<Vec<_>>(),
                "{name} visit{index} manager clipping"
            );
            assert_eq!(
                drawn,
                visit["draw_order"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|id| id.as_u64().unwrap() as usize)
                    .collect::<Vec<_>>(),
                "{name} visit{index} draw order"
            );
            assert_eq!(
                main,
                visit["rng_after"]["main"].as_str().unwrap(),
                "{name} visit{index} complete Main state"
            );
            if row["input"]["mixed_families"] == true {
                let stages = visit["stages"].as_array().unwrap();
                assert_eq!(
                    stages
                        .iter()
                        .map(|stage| stage["family"].as_str().unwrap())
                        .collect::<Vec<_>>(),
                    ["laser", "bolt", "trail"]
                );
                assert!(stages.windows(2).all(|pair| {
                    pair[0]["pixel_sha256"] != pair[1]["pixel_sha256"]
                        && integer(&pair[0]["segment_end"]) < integer(&pair[1]["segment_end"])
                }));
                assert!(!trails(row, visit).is_empty());
            }
            if name == "ordinary" && index == 0 {
                let expected: Vec<u64> = visit["rng_requests"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .flat_map(|request| {
                        assert_eq!(request["stream"], "main");
                        request["raw_draws"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|raw| raw.as_u64().unwrap())
                    })
                    .collect();
                assert_eq!(
                    trace
                        .iter()
                        .map(|draw| draw["value"].as_u64().unwrap())
                        .collect::<Vec<_>>(),
                    expected,
                    "{name} ordered raw Main draws"
                );
            }
        }
    }
}

#[test]
#[ignore = "requires GPU; native Bolt and mixed Laser/Bolt/Trail pixels, chunks and immutable Z"]
fn production_electric_bolt_pixels_match_native_with_bounded_edge_residuals() {
    let gpu = Gpu::new();
    let mut failures = Vec::new();
    for format in [
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ] {
        let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
        let mut terrain = TerrainDrawRenderer::new(&gpu.device, &gpu.queue, format, &batch);
        for row in drawing_rows() {
            let input = &row["input"];
            let name = input["name"].as_str().unwrap();
            let size: [u32; 2] = input
                .get("size")
                .map_or([160, 120], |v| point(v).map(|v| v as u32));
            batch.write_camera(&gpu.queue, camera(size));
            let color = gpu.target(size, format);
            let cv = color.create_view(&Default::default());
            let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
            let dv = depth.create_view(&Default::default());
            terrain.prepare(&gpu.device, &color, &dv, batch.camera_uniform());
            let initial = encoded(
                input["background"].as_u64().unwrap_or(0x39e7) as u16,
                format,
            )
            .repeat((size[0] * size[1]) as usize);
            let z: Vec<u16> = (0..size[1])
                .flat_map(|y| {
                    (0..size[0]).map(move |x| {
                        if let Some(values) = input["z"].as_array() {
                            integer(&values[((x / 11 + y / 7) % values.len() as u32) as usize])
                                as u16
                        } else {
                            input["z"].as_u64().unwrap_or(65535) as u16
                        }
                    })
                })
                .collect();
            for chunk in [None, Some(97)] {
                if chunk.is_some()
                    && !matches!(
                        name,
                        "ordinary"
                            | "mixed_alpha_depth"
                            | "reverse_order"
                            | "length_16384"
                            | "laser_bolt_trail_overlap"
                    )
                {
                    continue;
                }
                terrain.surface_lines.test_operation_limit = chunk;
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
                for (index, visit) in row["visits"].as_array().unwrap().iter().enumerate() {
                    let (lines, _, _, main) = visit_lines(row, index);
                    assert_eq!(main, visit["rng_after"]["main"].as_str().unwrap());
                    let trails = trails(row, visit);
                    terrain.begin_surface_lines();
                    let prepared_lines = terrain.prepare_surface_lines(
                        &gpu.device,
                        &gpu.queue,
                        laser(row),
                        &lines,
                        &trails,
                        viewport(input),
                        || true,
                        |[x, y]| match input["alpha"].as_str() {
                            Some("mixed") => [0, 1, 63, 127, 255][((x / 9 + y / 7) % 5) as usize],
                            Some("black") => 0,
                            _ => 127,
                        },
                    );
                    let mut encoder = gpu.device.create_command_encoder(&Default::default());
                    if input["z"].is_array() {
                        seed_depth_grid(&gpu, &mut encoder, &dv, &z, size[0]);
                    } else {
                        clear(&mut encoder, &cv, &dv, z[0], wgpu::LoadOp::Load);
                    }
                    terrain.draw_surface_lines(&mut encoder, &cv, prepared_lines);
                    let reads = [
                        gpu.read(&mut encoder, &color),
                        gpu.read(&mut encoder, &depth),
                    ];
                    let output = gpu.finish(encoder, &reads, size);
                    let mut expected = initial.clone();
                    for pixel in visit["pixels"].as_array().unwrap() {
                        let at = (integer(&pixel[1]) as usize * size[0] as usize
                            + integer(&pixel[0]) as usize)
                            * 4;
                        expected[at..at + 4]
                            .copy_from_slice(&encoded(integer(&pixel[2]) as u16, format));
                    }
                    assert_eq!(output[0].len(), expected.len());
                    let different: Vec<_> = output[0]
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .zip(expected.as_chunks::<4>().0)
                        .enumerate()
                        .filter(|(_, (a, b))| a != b)
                        .collect();
                    // One-pixel edge residuals: reverse_order visit1 (158,116),
                    // accepted_clip visit0 (0,57); every other pixel stays exact.
                    let residual_pixel = match (name, index) {
                        ("reverse_order", 1) => Some(116 * size[0] as usize + 158),
                        ("accepted_clip", 0) => Some(57 * size[0] as usize),
                        _ => None,
                    };
                    let within_residual = different.len() <= 1
                        && different
                            .iter()
                            .all(|(pixel, _)| Some(*pixel) == residual_pixel);
                    if !within_residual {
                        failures.push(format!("{name} visit{index} {format:?} chunk{chunk:?}: {} pixels differ, first{:?}",
                            different.len(), &different[..different.len().min(8)]));
                    }
                    let expected_z: Vec<u8> = z
                        .iter()
                        .flat_map(|&z| crate::render::native_z::stored_depth(z).to_le_bytes())
                        .collect();
                    if output[1] != expected_z {
                        failures.push(format!(
                            "depth changed: {name} visit{index} {format:?} chunk{chunk:?}"
                        ));
                    }
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
