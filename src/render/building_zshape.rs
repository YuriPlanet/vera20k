//! BUILDNGZ.SHA: the shared per-pixel Z-shape every building body writes
//! its depth through.
//!
//! gamemd loads the file once (`FUN_0045E8F0`: `LoadFileFromMIX("BUILDNGZ.SHA")`
//! into `DAT_0089DDBC`, then `byte -= 0x41` on every non-zero byte of frame
//! 0), and `BuildingClass_DrawBody @ 0x0043D290` hands the pointer to
//! `TechnoClass_DrawSHP` as a12 with the canvas offset `(0xC6, 0x1BE) +
//! ZShapePointMove - CellToPixel(foundation far corner)` in a14/a15. The
//! extended blitter's leaf `0x004990E0` then tests and stores
//! `constant_seed_z - zshape[x]` per pixel. Retail YR ships one 396x477 frame in
//! `ra2md.mix -> conqmd.mix`.
//!
//! The remapped signed bytes are stored biased by [`ZSHAPE_TEXEL_BIAS`] in an
//! `R8Unorm` texture of the frame's size that `zsprite_shader.wgsl` reads
//! with `textureLoad` at `world_pos - zshape_origin`. CC_Draw_Shape clips the
//! body to this frame before format dispatch; the shader also seeds from that
//! intersection.
//!
//! RESIDUAL: when the frame starts inside the body, Blit_Clip's equal-size
//! branch (`0x007BBE20`, lead skip `0x00437AC4`) keeps the body's first
//! column and row at its origin, trims the far side and reads z bytes from
//! before the frame row, where VERA cuts the body to the frame. Retail
//! BUILDNGZ starts at least 38 px left of and 222 px above every retail body.

use crate::assets::asset_manager::AssetManager;
use crate::assets::shp_file::ShpFile;
use crate::render::batch::{BatchRenderer, create_r8_texture_view};
use crate::render::gpu::GpuContext;
use crate::render::native_z::{ZSHAPE_TEXEL_BIAS, zshape_remap};

/// Retail file name as the engine requests it.
pub const BUILDNGZ_FILE: &str = "buildngz.sha";

/// Loaded z-shape: the GPU bind group for group 2 of the zsprite pipelines.
pub struct BuildingZShape {
    pub bind_group: wgpu::BindGroup,
    pub width: u32,
    pub height: u32,
    /// The frame's signed offset in its file's canvas
    /// (`native_z::zshape_origin`).
    pub frame_offset: (i32, i32),
}

/// A z-shape's frame 0 as `CC_Draw_Shape` reads it.
pub struct ZShapeFrame {
    pub size: [u32; 2],
    /// The frame's signed x and y in its canvas.
    pub offset: (i32, i32),
    /// `zshape_remap(byte) + 128` per pixel.
    pub texels: Vec<u8>,
}

/// Frame 0 of a z-shape SHP as `CC_Draw_Shape` reads it: `GetFrameData`'s
/// bytes from the frame's offset (`0x004AEF71` -> `0x0069E740`), never RLE
/// decoded, wrapped as a surface of the frame's own size
/// (`0x004AEF80..0x004AEFB6`). Bytes past the file read as zero. The canvas
/// size is never read.
pub fn decode_zshape_frame(bytes: &[u8], shp: &ShpFile) -> Option<ZShapeFrame> {
    let frame = shp.frames.first()?;
    let size = [frame.frame_width, frame.frame_height].map(u32::from);
    if size.contains(&0) {
        return None;
    }
    let data = ShpFile::encoded_frame_data(bytes, 0).ok()?;
    let texels = (0..(size[0] * size[1]) as usize)
        .map(|index| {
            let byte = data.get(index).copied().unwrap_or(0);
            (i32::from(zshape_remap(byte)) + ZSHAPE_TEXEL_BIAS).clamp(0, 255) as u8
        })
        .collect();
    let signed = |value: u16| i32::from(value as i16);
    Some(ZShapeFrame {
        size,
        offset: (signed(frame.frame_x), signed(frame.frame_y)),
        texels,
    })
}

impl BuildingZShape {
    /// Load `BUILDNGZ.SHA` through the retail file boundary and upload it.
    /// `None` when the asset is missing or unparsable (callers fall back to
    /// the neutral z-shape, i.e. flat building depth).
    pub fn load(
        gpu: &GpuContext,
        batch: &BatchRenderer,
        asset_manager: &AssetManager,
    ) -> Option<Self> {
        let file = asset_manager.load_file_from_mix(BUILDNGZ_FILE)?;
        let shp = match ShpFile::from_bytes(&file.bytes) {
            Ok(shp) => shp,
            Err(err) => {
                log::warn!("{BUILDNGZ_FILE}: unparsable z-shape: {err}");
                return None;
            }
        };
        let ZShapeFrame {
            size: [width, height],
            offset,
            texels,
        } = decode_zshape_frame(&file.bytes, &shp)?;
        // A mod's z-shape past the device's textures: bodies write flat
        // depth, the caller's fallback, rather than failing validation.
        if !crate::render::gpu::texture_fits(&gpu.device, [width, height]) {
            log::warn!(
                "{BUILDNGZ_FILE}: {width}x{height} px z-shape does not fit this GPU's {} px textures",
                gpu.device.limits().max_texture_dimension_2d
            );
            return None;
        }
        let view = create_r8_texture_view(gpu, "BUILDNGZ ZShape", &texels, width, height);
        log::info!(
            "BUILDNGZ.SHA z-shape: {width}x{height} from {} ({} frames)",
            file.source_archive,
            shp.frames.len()
        );
        Some(Self {
            bind_group: batch.create_zshape_bind_group(gpu, &view),
            width,
            height,
            frame_offset: offset,
        })
    }
}

#[cfg(all(test, tests_render_a_r))]
mod tests {
    use super::*;
    use crate::assets::shp_file::ShpFrame;

    fn shp(width: u16, height: u16, frame: ShpFrame) -> ShpFile {
        ShpFile {
            width,
            height,
            frames: vec![frame],
        }
    }

    #[test]
    fn the_frame_reads_its_encoded_bytes_and_keeps_a_signed_offset() {
        let frame = ShpFrame {
            frame_x: 1,
            frame_y: (-2i16) as u16,
            frame_width: 2,
            frame_height: 2,
            format: 1,
            radar_color: [0; 3],
            pixels: vec![0x41, 0x43, 0x45, 0x47],
        };
        // Header, one frame entry, then three data bytes, not the decoded
        // pixels: the fourth pixel reads past the file.
        let mut bytes = vec![0u8; 8 + 24];
        bytes[6] = 1;
        bytes[8 + 4..8 + 8].copy_from_slice(&[2, 0, 2, 0]);
        bytes[8 + 20..8 + 24].copy_from_slice(&32u32.to_le_bytes());
        bytes.extend([0x41, 0x43, 0x00]);
        let decoded = decode_zshape_frame(&bytes, &shp(4, 3, frame)).unwrap();
        assert_eq!(decoded.size, [2, 2]);
        assert_eq!(decoded.offset, (1, -2));
        // 0x41 -> 0 -> 128; 0x43 -> +2 -> 130; zero and past-file stay 128.
        assert_eq!(decoded.texels, [128, 130, 128, 128]);
    }
}
