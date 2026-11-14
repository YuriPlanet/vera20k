//! Native SHROUD leaf, selector and ordinary full-rebuild prerequisites.
use super::*;
use serde_json::Value;

fn fixture() -> &'static Value {
    crate::test_fixture::json("tools/procedural_drawing_oracle/shroud.json")
}
fn bytes(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
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

/// 47EFE0 leaf stores through the production compositor: each draw copies
/// one frame at `point` into `plane` (`size` wide) within its `clip`, the
/// way the old CPU leaf wrote a clipped sub-slice. The clip's bytes go up
/// to a plane of the clip's size, the blit lands at `point - clip`, and the
/// composed bytes come back.
fn compose_leaf(
    gpu: &crate::render::terrain_draw_gpu_tests::Gpu,
    compositor: &mut Compositor,
    frames: &mut FrameAtlas,
    plane: &mut [u8],
    size: [u32; 2],
    clip: [i32; 4],
    frame: u16,
    point: [i32; 2],
) {
    let x = clip[0].max(0);
    let y = clip[1].max(0);
    let right = (clip[0] + clip[2]).min(size[0] as i32);
    let bottom = (clip[1] + clip[3]).min(size[1] as i32);
    if x >= right || y >= bottom {
        return;
    }
    let extent = [(right - x) as u32, (bottom - y) as u32];
    let rows = |row: u32| {
        let start = ((y as u32 + row) * size[0] + x as u32) as usize;
        start..start + extent[0] as usize
    };
    let texture = create_a_plane(&gpu.device, extent);
    let initial: Vec<u8> = (0..extent[1])
        .flat_map(|row| plane[rows(row)].to_vec())
        .collect();
    crate::render::atlas_growth::write_texels(&gpu.queue, &texture, [0, 0], extent, 1, &initial);
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    compositor.encode_cells(
        &gpu.device,
        &gpu.queue,
        &mut encoder,
        &texture.create_view(&Default::default()),
        extent,
        frames,
        &[CellBlits {
            origin: [point[0] - x, point[1] - y],
            frames: [frame, NO_FRAME],
        }],
        false,
    );
    let pending = crate::render::frame_readback::PendingTextureReadback::encode_r8(
        &gpu.device,
        &mut encoder,
        &texture,
        [0, 0],
        extent,
    )
    .unwrap();
    let submission = gpu.queue.submit([encoder.finish()]);
    let composed = pending
        .finish(&gpu.device, submission, std::time::Duration::from_secs(30))
        .unwrap();
    for (row, bytes) in composed.chunks_exact(extent[0] as usize).enumerate() {
        plane[rows(row as u32)].copy_from_slice(bytes);
    }
}

#[test]
#[ignore = "requires GPU and retail assets; FogOfWar A-buffer source/blend/defeat"]
fn retail_fog_source_and_defeat_gate_reach_the_shared_a_texture() {
    use crate::map::resolved_terrain::{NativeCellQuery, ResolvedTerrainCell, ResolvedTerrainGrid};
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let decode = |name| {
        let bytes = assets.get(name).unwrap();
        let shp = crate::assets::shp_file::ShpFile::from_bytes(&bytes).unwrap();
        extract_shp_brightness(&shp)
    };
    let (shroud, width, height) = decode("shroud.shp");
    let (fog_frames, fog_width, fog_height) = decode("fog.shp");
    let center = (fog_height / 2 * fog_width + fog_width / 2) as usize;
    let bright = fog_frames[0][center];
    let dim = fog_frames[15][center];
    assert_eq!((bright, dim), (0x7f, 0x42)); // Physical FOG.SHP bytes.
    let dark = shroud[15][(height / 2 * width + width / 2) as usize];
    let terrain = ResolvedTerrainGrid::from_cells(
        16,
        16,
        (0..16)
            .flat_map(|y| (0..16).map(move |x| ResolvedTerrainCell::clear_for_test(x, y)))
            .collect(),
    );
    let before = terrain.shared_cell_dummy().snapshot();
    let cells = NativeCellQuery::isolated(&terrain);
    let gpu = crate::render::terrain_draw_gpu_tests::Gpu::new();
    let size = [640, 360];
    let mut source = ShroudBuffer::new(
        &gpu.device,
        &gpu.queue,
        size,
        [16, 16],
        shroud,
        [width, height],
        SHROUD_EDGE_LUT,
    );
    source.install_fog_frames(&gpu.device, &gpu.queue, fog_frames, [fog_width, fog_height]);
    let mut names = crate::sim::intern::StringInterner::new();
    let owner = names.intern("Local");
    let mut fog = FogState::new(16, 16);
    // Ground remains explored after regrowth clears the separate Cell140
    // fog channel. The center's selectors are -1 (ground), -2 (fog).
    for y in 7..=9 {
        for x in 7..=9 {
            fog.set_ground_bits_for_test(owner, x, y, 0x18);
        }
    }
    assert_eq!(
        fog.map_cell_edge_frame(owner, (8, 8), false, Some(&cells)),
        -1
    );
    assert_eq!(
        fog.map_cell_edge_frame(owner, (8, 8), true, Some(&cells)),
        -2
    );
    let pixel = |cell: (u16, u16)| {
        let (x, y) = iso_to_screen(cell.0, cell.1, 0);
        ((y as u32 + 15) * size[0] + (x as i32 + 320 + 30) as u32) as usize
    };
    //47EFE0's source changes with the flag. 47F250 blends only before
    //defeat; expected intensities here are retail source bytes, not a model.
    for (options, explored, unexplored) in [
        (FogDrawOptions::default(), bright, Some(dark)),
        (
            FogDrawOptions {
                fog_of_war: true,
                current_house_defeated: false,
            },
            dim,
            None,
        ),
        (
            FogDrawOptions {
                fog_of_war: true,
                current_house_defeated: true,
            },
            bright,
            Some(dim),
        ),
    ] {
        source.rebuild_submitted(
            &gpu.device,
            &gpu.queue,
            &fog,
            owner,
            -320.0,
            0.0,
            size[0],
            size[1],
            1.0,
            Some(&cells),
            options,
        );
        let pixels = gpu.read_float_texels(source.gpu_source().0, [0, 0], size);
        assert_eq!((pixels[pixel((8, 8))][0] * 255.0).round() as u8, explored);
        if let Some(expected) = unexplored {
            assert_eq!((pixels[pixel((4, 4))][0] * 255.0).round() as u8, expected);
        }
    }
    assert_eq!(terrain.shared_cell_dummy().snapshot(), before);
}

#[test]
fn retail_shroud_pixels_and_clipping_match_original_47efe0() {
    let native = fixture();
    let Some((frames, canvas)) = stock_frames(&native) else {
        return;
    };
    let gpu = crate::render::terrain_draw_gpu_tests::Gpu::new();
    let mut frames = FrameAtlas::new(&gpu.device, &gpu.queue, frames, canvas);
    let mut compositor = Compositor::new(&gpu.device);
    let size = dimensions(&native["size"]);
    for row in native["leaf_cases"].as_array().unwrap() {
        let mut pixels =
            vec![native["initial_value"].as_u64().unwrap() as u8; (size[0] * size[1]) as usize];
        for draw in row["draws"].as_array().unwrap() {
            compose_leaf(
                &gpu,
                &mut compositor,
                &mut frames,
                &mut pixels,
                size,
                std::array::from_fn(|i| draw["clip"][i].as_i64().unwrap() as i32),
                draw["frame"].as_u64().unwrap() as u16,
                point(&draw["point"]),
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
    let mut fog = FogState::new(32, 32);
    let mut interner = crate::sim::intern::StringInterner::new();
    let owner = interner.intern("Local");
    fog.mark_visible_for_owner(owner, 1, 1);
    assert_eq!(
        cell_fill(&fog, owner, 10, 20, &SHROUD_EDGE_LUT),
        CellFill::Frame(15)
    );
}

#[test]
#[ignore = "requires retail archives and GPU; full production A rebuild and actual source texture readback"]
fn production_shroud_rebuild_matches_original_tactical_scenes() {
    let native = fixture();
    let (frames, canvas) = stock_frames(&native).expect("RA2_DIR supplies stock SHROUD");
    let size = dimensions(&native["size"]);
    let gpu = crate::render::terrain_draw_gpu_tests::Gpu::new();
    let mut source = ShroudBuffer::new(
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
        let mut fog = FogState::new(64, 64);
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
        // Each row installs a different map with a restarted view generation.
        // Use the same invalidation as a load, then the ordinary dirty gate
        // and upload, rather than writing private camera/cache fields.
        source.mark_stale();
        source.rebuild_submitted(
            &gpu.device,
            &gpu.queue,
            &fog,
            owner,
            camera[0] as f32,
            (camera[1] + native["world_y_bias"].as_i64().unwrap() as i32) as f32,
            size[0],
            size[1],
            1.0,
            None,
            Default::default(),
        );
        let pixels = gpu
            .read_float_texels(source.gpu_source().0, [0, 0], size)
            .into_iter()
            .map(|texel| (texel[0] * 255.0).round() as u8)
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
fn clear_tile_shroud_edge_matches_original_a_producer() {
    let packet: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/tmp_clear_render.json",
    ))
    .unwrap();
    let native = fixture();
    let Some((frames, canvas)) = stock_frames(&native) else {
        return;
    };
    let edge = &packet["inputs"]["shroud_edge"];
    assert_eq!(edge["source"]["sha256"], native["stock"]["sha256"]);
    let size = [
        packet["surface"]["width"].as_u64().unwrap() as u32,
        packet["surface"]["height"].as_u64().unwrap() as u32,
    ];
    assert_eq!(edge["clip"], serde_json::json!([0, 0, size[0], size[1]]));
    let gpu = crate::render::terrain_draw_gpu_tests::Gpu::new();
    let mut pixels = vec![edge["initial"].as_u64().unwrap() as u8; (size[0] * size[1]) as usize];
    compose_leaf(
        &gpu,
        &mut Compositor::new(&gpu.device),
        &mut FrameAtlas::new(&gpu.device, &gpu.queue, frames, canvas),
        &mut pixels,
        size,
        [0, 0, size[0] as i32, size[1] as i32],
        edge["frame"].as_u64().unwrap() as u16,
        point(&edge["point"]),
    );
    let row = packet["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["input"]["name"] == "physical_shroud_edge")
        .unwrap();
    assert_eq!(
        pixels,
        bytes(row["input"]["a_hex"].as_str().unwrap()),
        "same physical A bytes consumed by Cell/TMP GPU comparison"
    );
}
