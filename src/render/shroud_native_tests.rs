//! Native SHROUD leaf, selector and ordinary full-rebuild prerequisites.
use super::*;
use serde_json::Value;

fn fixture() -> Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/procedural_drawing_oracle/shroud.json",
    ))
    .unwrap()
}
fn bytes(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}
fn dimensions(row: &Value) -> [u32; 2] {
    std::array::from_fn(|i| row[i].as_u64().unwrap() as u32)
}
fn point(row: &Value) -> [i32; 2] {
    std::array::from_fn(|i| row[i].as_i64().unwrap() as i32)
}
fn stock_frames(native: &Value) -> Option<(Vec<Vec<u8>>, [u32; 2])> {
    let (_, assets) = crate::rules::retail_ini_fixture::retail_assets()?;
    let source = assets.get("shroud.shp").expect("retail SHROUD.SHP");
    assert_eq!(
        crate::util::sha256::sha256_hex(&source),
        native["stock"]["sha256"].as_str().unwrap()
    );
    let shp = crate::assets::shp_file::ShpFile::from_bytes(&source).unwrap();
    let (frames, width, height) = extract_shp_brightness(&shp);
    assert_eq!([width, height], dimensions(&native["stock"]["canvas"]));
    assert_eq!(
        frames.len() as u64,
        native["stock"]["frame_count"].as_u64().unwrap()
    );
    Some((frames, [width, height]))
}

#[test]
fn retail_shroud_pixels_and_clipping_match_original_47efe0() {
    let native = fixture();
    let Some((frames, canvas)) = stock_frames(&native) else {
        return;
    };
    let frames = frames
        .into_iter()
        .map(|pixels| BrightnessFrame::new(pixels, canvas))
        .collect::<Vec<_>>();
    let size = dimensions(&native["size"]);
    for row in native["leaf_cases"].as_array().unwrap() {
        let mut pixels =
            vec![native["initial_value"].as_u64().unwrap() as u8; (size[0] * size[1]) as usize];
        for draw in row["draws"].as_array().unwrap() {
            let [cx, cy, width, height]: [i32; 4] =
                std::array::from_fn(|i| draw["clip"][i].as_i64().unwrap() as i32);
            let x = cx.max(0);
            let y = cy.max(0);
            let right = (cx + width).min(size[0] as i32);
            let bottom = (cy + height).min(size[1] as i32);
            if x >= right || y >= bottom {
                continue;
            }
            let location = point(&draw["point"]);
            let start = (y as u32 * size[0] + x as u32) as usize;
            blit_frame_pixels(
                &mut pixels[start..],
                size[0] as usize,
                [(right - x) as u32, (bottom - y) as u32],
                &frames[draw["frame"].as_u64().unwrap() as usize],
                [location[0] - x, location[1] - y],
            );
        }
        assert_eq!(
            pixels,
            bytes(row["alpha_hex"].as_str().unwrap()),
            "{}",
            row["input"]
        );
    }
}

#[test]
fn native_shroud_selector_matches_every_retained_neighbor_mask() {
    let native = fixture();
    for row in native["selector_cases"].as_array().unwrap() {
        let mask = row["mask"].as_u64().unwrap() as usize;
        let selected = match SHROUD_EDGE_LUT[mask] {
            0xff => 0,
            0xfe => 15,
            frame => frame,
        };
        assert_eq!(
            u64::from(selected),
            row["frame"].as_u64().unwrap(),
            "native mask{mask}"
        );
    }
    // The native unrevealed/gap path chooses frame15 independently of mask.
    let mut fog = FogState {
        width: 32,
        height: 32,
        ..Default::default()
    };
    let mut interner = crate::sim::intern::StringInterner::new();
    let owner = interner.intern("Local");
    fog.mark_visible_for_owner(owner, 1, 1);
    assert_eq!(
        cell_fill(&fog, owner, 10, 20, &SHROUD_EDGE_LUT),
        CellFill::Frame(15)
    );
}

#[test]
#[ignore = "requires retail archives and GPU source allocation; executes the full production CPU A rebuild"]
fn production_shroud_rebuild_matches_original_tactical_scenes() {
    let native = fixture();
    let (frames, canvas) = stock_frames(&native).expect("RA2_DIR supplies stock SHROUD");
    let size = dimensions(&native["size"]);
    let gpu = crate::render::terrain_draw_gpu_tests::Gpu::new();
    let mut source = ShroudBuffer::new_on_device(
        &gpu.device,
        &gpu.queue,
        size,
        [64, 64],
        frames,
        canvas,
        SHROUD_EDGE_LUT,
    );
    let mut interner = crate::sim::intern::StringInterner::new();
    let owner = interner.intern("Local");
    for row in native["scenes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["input"]["full_redraw"] != false)
    {
        let input = &row["input"];
        let mut fog = FogState {
            width: 64,
            height: 64,
            ..Default::default()
        };
        let default_clear = input["default_flags"].as_u64() == Some(24);
        let rectangle = input["revealed_rectangle"].as_array();
        for ry in 0..64 {
            for rx in 0..64 {
                let inside = rectangle.is_some_and(|r| {
                    i64::from(rx) >= r[0].as_i64().unwrap()
                        && i64::from(ry) >= r[1].as_i64().unwrap()
                        && i64::from(rx) <= r[2].as_i64().unwrap()
                        && i64::from(ry) <= r[3].as_i64().unwrap()
                });
                if default_clear || inside {
                    fog.mark_visible_for_owner(owner, rx, ry);
                }
            }
        }
        let camera = input.get("camera").map(point).unwrap_or([-320, 440]);
        source.last_cam_x = camera[0] as f32;
        source.last_cam_y = (camera[1] + native["world_y_bias"].as_i64().unwrap() as i32) as f32;
        source.rasterize(&fog, owner);
        let pixels = source
            .pixels
            .chunks_exact(source.row_stride as usize)
            .flat_map(|row| row[..size[0] as usize].iter().copied())
            .collect::<Vec<_>>();
        assert_eq!(
            pixels,
            bytes(row["alpha_hex"].as_str().unwrap()),
            "{}",
            input["name"]
        );
        // The stock frontier feeds native rally admission with nonzero A2,
        // not the former synthesized zero. This applies throughout the plane.
        assert!(!pixels.contains(&0), "{}", input["name"]);
    }
}

#[test]
#[ignore = "requires retail archives and GPU; run optimized to measure the actual A raster/upload owners"]
fn production_shroud_rebuild_workload_timing() {
    use std::time::{Duration, Instant};

    let native = fixture();
    let (frames, canvas) = stock_frames(&native).expect("RA2_DIR supplies stock SHROUD");
    let gpu = crate::render::terrain_draw_gpu_tests::Gpu::new();
    let screen = [1280, 720];
    let map = [256u16; 2];
    let limit = gpu.device.limits().max_texture_dimension_2d;
    let minimum_zoom = (screen[0] as f32 / limit as f32).max(0.25);
    let mut interner = crate::sim::intern::StringInterner::new();
    let owner = interner.intern("Local");
    let mut results = Vec::new();
    for scene in ["unrevealed", "frontier"] {
        let mut fog = FogState {
            width: map[0],
            height: map[1],
            ..Default::default()
        };
        // Establish the viewer outside the measured view. The frontier's
        // sight setup is also excluded from rendering measurements.
        fog.mark_visible_for_owner(owner, 1, 1);
        if scene == "frontier" {
            for ry in 0..map[1] / 2 {
                for rx in 0..map[0] {
                    fog.mark_visible_for_owner(owner, rx, ry);
                }
            }
        }
        for zoom in [1.0, minimum_zoom] {
            let size = virtual_dimensions(screen, zoom);
            assert!(size[0] <= limit && size[1] <= limit);
            let mut source = ShroudBuffer::new_on_device(
                &gpu.device,
                &gpu.queue,
                size,
                map,
                frames.clone(),
                canvas,
                SHROUD_EDGE_LUT,
            );
            let mut samples = Vec::new();
            for frame in 0..22 {
                // Pan by a native source pixel each frame. The production
                // dirty gate would rebuild; reuse allocation as steady play
                // does. Rare resize and unchanged-gate costs are not measured.
                source.last_cam_x = -(size[0] as f32) / 2.0 + frame as f32;
                source.last_cam_y = 3840.0 - size[1] as f32 / 2.0;
                let started = Instant::now();
                source.rasterize(&fog, owner);
                let raster_ms = started.elapsed().as_secs_f64() * 1000.0;
                let writing = Instant::now();
                source.upload(&gpu.queue);
                let queue_write_ms = writing.elapsed().as_secs_f64() * 1000.0;
                let submitted = Instant::now();
                let submission = gpu.queue.submit([]);
                gpu.device
                    .poll(wgpu::PollType::Wait {
                        submission_index: Some(submission),
                        timeout: Some(Duration::from_secs(60)),
                    })
                    .unwrap();
                let completion_ms = submitted.elapsed().as_secs_f64() * 1000.0;
                let total_ms = started.elapsed().as_secs_f64() * 1000.0;
                if frame >= 2 {
                    samples.push(serde_json::json!({
                        "frame": frame - 2,
                        "raster_ms": raster_ms,
                        "queue_write_ms": queue_write_ms,
                        "submit_to_completion_ms": completion_ms,
                        "total_ms": total_ms,
                    }));
                }
            }
            let mean = |field: &str| {
                samples
                    .iter()
                    .map(|sample| sample[field].as_f64().unwrap())
                    .sum::<f64>()
                    / samples.len() as f64
            };
            eprintln!(
                "A_REBUILD_TIMING scene={scene} screen={screen:?} virtual={size:?} zoom={zoom} map={map:?} warmups=2 samples=20 raster_mean_ms={:.4} queue_write_mean_ms={:.4} completion_mean_ms={:.4} total_mean_ms={:.4}",
                mean("raster_ms"),
                mean("queue_write_ms"),
                mean("submit_to_completion_ms"),
                mean("total_ms")
            );
            results.push(serde_json::json!({
                "scene": scene,
                "virtual_size": size,
                "zoom": zoom,
                "upload_bytes": source.row_stride * source.height,
                "samples": samples,
            }));
        }
    }
    if let Some(path) = std::env::var_os("VERA20K_SHROUD_PERF_OUTPUT") {
        let report = serde_json::json!({
            "scope": "production ShroudBuffer rasterize and upload, sequential queue fence; excludes allocation, resize and unchanged dirty gate",
            "screen": screen,
            "map": map,
            "device_max_texture_dimension_2d": limit,
            "debug_assertions": cfg!(debug_assertions),
            "warmup_frames": 2,
            "measured_frames": 20,
            "stock_shroud_sha256": native["stock"]["sha256"],
            "results": results,
        });
        std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    }
}
