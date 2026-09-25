//! Stock-hull executable comparisons and a retained constructed-preview test.
//!
//! The explicit GTNK acceptance probe compares all 32 flat hull facings,
//! original visibility bytes and all six crop outputs through production
//! preparation/raster APIs. Synthetic forward/backward, section, empty-run and
//! zero-erasure fixtures live in vxl_native.rs; production bakes replay the
//! prepared writes on the CPU. Native VPL result zero is a real store that can erase a
//! prior voxel. The small no-VPL test here only checks source colors in a
//! manually constructed legacy preview; it does not define native zero rules.
//!
//! Hull evidence does not establish relative turret/barrel transforms, part
//! composition, final RGB, or complete live scene equivalence.

#[cfg(test)]
mod tests {
    use crate::assets::vxl_file::{VxlFile, VxlLimb, VxlVoxel};
    use crate::render::vxl_raster::{self, VxlRenderParams, VxlSprite};

    /// Explicit retail acceptance probe. Inputs and native observations come
    /// from the original-executable loader/raster sweep, never Rust output.
    /// Set VERA20K_VOXEL_PROBE_DIR to the local grizzly-raster-proof directory.
    /// The canonical comparison retains native world offsets and transparent
    /// pixels; no translation search, resizing or best-match alignment occurs.
    #[test]
    #[ignore = "requires extracted retail GTNK assets and original-executable all-facing observations"]
    fn native_grizzly_hull_matches_retail_all_facings() {
        use crate::assets::{hva_file::HvaFile, vpl_file::VplFile};
        use std::{fs, path::PathBuf};

        let root = PathBuf::from(
            std::env::var_os("VERA20K_VOXEL_PROBE_DIR")
                .expect("set VERA20K_VOXEL_PROBE_DIR to the native hull probe directory"),
        );
        let assets = root.join("extract");
        let vxl = VxlFile::from_bytes(&fs::read(assets.join("gtnk.vxl")).unwrap()).unwrap();
        let hva = HvaFile::from_bytes(&fs::read(assets.join("gtnk.hva")).unwrap()).unwrap();
        let vpl = VplFile::from_bytes(&fs::read(assets.join("voxels.vpl")).unwrap()).unwrap();
        assert_eq!(vxl.limbs.len(), 1);
        assert_eq!((hva.frame_count, hva.section_count), (1, 1));
        let output = root.join("rust-all-facings");
        fs::create_dir_all(&output).unwrap();
        let mut reports = Vec::new();
        let mut total_differences = 0usize;
        for step in 0..32u8 {
            let native_dir = root.join("all-facings").join(format!("{step:02}"));
            let native = fs::read(native_dir.join("gtnk-facing0-indexed.bin")).unwrap();
            assert_eq!(native.len(), 256 * 256);
            let metadata: serde_json::Value =
                serde_json::from_slice(&fs::read(native_dir.join("probe-result.json")).unwrap())
                    .unwrap();
            let raw = metadata["rect_raw"].as_str().unwrap();
            let mut rect_bytes = [0u8; 24];
            for (i, byte) in rect_bytes.iter_mut().enumerate() {
                *byte = u8::from_str_radix(&raw[i * 2..i * 2 + 2], 16).unwrap();
            }
            let rect: [i32; 6] = std::array::from_fn(|i| {
                i32::from_le_bytes(rect_bytes[i * 4..i * 4 + 4].try_into().unwrap())
            });
            let params = VxlRenderParams {
                facing: step * 8,
                ..Default::default()
            };
            let prepared = vxl_raster::prepare_native_draw(&vxl, Some(&hva), &params, Some(&vpl))
                .expect("stock GTNK must use the encoded native path");
            assert_eq!(prepared.rect, rect, "all six native GTNK crop outputs");
            let sprite = vxl_raster::render_vxl(&vxl, Some(&hva), &params, Some(&vpl));
            assert_eq!(
                sprite.palette_indices,
                prepared.render_cpu().unwrap().palette_indices
            );
            let mut canonical = vec![0u8; 256 * 256];
            let start_x = sprite.offset_x as i32 + rect[2] - rect[0];
            let start_y = sprite.offset_y as i32 + rect[3] - rect[1];
            for y in 0..sprite.height as i32 {
                for x in 0..sprite.width as i32 {
                    let color =
                        sprite.palette_indices[(y as u32 * sprite.width + x as u32) as usize];
                    if color == 0 {
                        continue;
                    }
                    let (nx, ny) = (x + start_x, y + start_y);
                    assert!((0..256).contains(&nx) && (0..256).contains(&ny));
                    canonical[(ny * 256 + nx) as usize] = color;
                }
            }
            let differences = canonical
                .iter()
                .zip(&native)
                .filter(|(a, b)| a != b)
                .count();
            let mask_differences = canonical
                .iter()
                .zip(&native)
                .filter(|(a, b)| (**a == 0) != (**b == 0))
                .count();
            total_differences += differences;
            let report = serde_json::json!({
                "step": step, "differences": differences, "mask_differences": mask_differences,
                "native_nonzero": native.iter().filter(|v| **v != 0).count(),
                "rust_nonzero": canonical.iter().filter(|v| **v != 0).count(),
                "rust_offset": [sprite.offset_x, sprite.offset_y],
                "rust_dimensions": [sprite.width, sprite.height], "native_rect": rect,
            });
            eprintln!("{report}");
            fs::write(output.join(format!("{step:02}.bin")), canonical).unwrap();
            reports.push(report);
        }
        fs::write(
            output.join("report.json"),
            serde_json::to_vec_pretty(&reports).unwrap(),
        )
        .unwrap();
        assert_eq!(
            total_differences, 0,
            "native hull raster differences across 32 facings"
        );
    }

    /// Build a tiny 2×2×2 VXL with two opaque voxels (color indices 10 and 20)
    /// and otherwise empty cells. Verifies that the rasterizer writes those
    /// non-zero bytes for the opaque pixels and leaves byte 0 for everything
    /// else. No VPL is loaded, so the post-VPL byte equals the source
    /// color_index directly (per the no-VPL fallback in render_vxl).
    fn make_two_voxel_vxl() -> VxlFile {
        let identity: [f32; 12] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0];
        VxlFile {
            limb_count: 1,
            body_size: 0,
            palette: vec![[0; 3]; 256],
            limbs: vec![VxlLimb {
                native_spans: None,
                name: "body".to_string(),
                scale: 1.0,
                bounds: [-1.0, -1.0, -1.0, 1.0, 1.0, 1.0],
                transform: identity,
                size_x: 2,
                size_y: 2,
                size_z: 2,
                normals_mode: 4,
                voxels: vec![
                    VxlVoxel {
                        x: 1,
                        y: 1,
                        z: 1,
                        color_index: 10,
                        normal_index: 0,
                    },
                    VxlVoxel {
                        x: 0,
                        y: 0,
                        z: 0,
                        color_index: 20,
                        normal_index: 1,
                    },
                ],
            }],
        }
    }

    #[test]
    fn constructed_preview_without_vpl_contains_only_source_color_indices() {
        let vxl: VxlFile = make_two_voxel_vxl();
        let params: VxlRenderParams = VxlRenderParams::default();
        let sprite: VxlSprite = vxl_raster::render_vxl(&vxl, None, &params, None);

        // Output must contain only bytes 0 (transparent) or one of the source
        // color indices we wrote (10 or 20). Anything else means the
        // rasterizer corrupted a pixel value.
        for &byte in &sprite.palette_indices {
            assert!(
                byte == 0 || byte == 10 || byte == 20,
                "Unexpected palette byte {} in rasterizer output (allowed: 0, 10, 20)",
                byte,
            );
        }

        // At least one pixel of each source color must appear (the rasterizer
        // wouldn't be doing its job otherwise).
        let count_10: usize = sprite.palette_indices.iter().filter(|&&b| b == 10).count();
        let count_20: usize = sprite.palette_indices.iter().filter(|&&b| b == 20).count();
        assert!(
            count_10 > 0,
            "voxel with color_index=10 produced no output bytes"
        );
        assert!(
            count_20 > 0,
            "voxel with color_index=20 produced no output bytes"
        );
    }
}
