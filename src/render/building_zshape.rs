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
//! `R8Unorm` texture that `zsprite_shader.wgsl` reads with `textureLoad` at
//! `world_pos - zshape_origin`. CC_Draw_Shape clips the body to this canvas
//! before format dispatch; the shader also seeds from that intersection.

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
}

/// Frame 0 of a z-shape SHP as biased `R8` texels over the full header canvas.
///
/// Returns `(width, height, texels)`; texels are `zshape_remap(byte) + 128`.
pub fn decode_zshape_canvas(shp: &ShpFile) -> Option<(u32, u32, Vec<u8>)> {
    let frame = shp.frames.first()?;
    let width = u32::from(shp.width);
    let height = u32::from(shp.height);
    if width == 0 || height == 0 {
        return None;
    }
    let neutral = ZSHAPE_TEXEL_BIAS as u8;
    let mut texels = vec![neutral; (width * height) as usize];
    let fw = usize::from(frame.frame_width);
    let fh = usize::from(frame.frame_height);
    for y in 0..fh {
        let cy = y + usize::from(frame.frame_y);
        if cy >= height as usize {
            break;
        }
        for x in 0..fw {
            let cx = x + usize::from(frame.frame_x);
            if cx >= width as usize {
                break;
            }
            let byte = frame.pixels.get(y * fw + x).copied().unwrap_or(0);
            let signed = i32::from(zshape_remap(byte)) + ZSHAPE_TEXEL_BIAS;
            texels[cy * width as usize + cx] = signed.clamp(0, 255) as u8;
        }
    }
    Some((width, height, texels))
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
        let (width, height, texels) = decode_zshape_canvas(&shp)?;
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
        })
    }
}

#[cfg(test)]
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
    fn canvas_places_the_frame_at_its_offset_and_biases_the_remap() {
        let frame = ShpFrame {
            frame_x: 1,
            frame_y: 1,
            frame_width: 2,
            frame_height: 1,
            format: 1,
            radar_color: [0; 3],
            pixels: vec![0x41, 0x43],
        };
        let (w, h, texels) = decode_zshape_canvas(&shp(4, 3, frame)).unwrap();
        assert_eq!((w, h), (4, 3));
        // Untouched canvas is neutral (offset 0).
        assert_eq!(texels[0], 128);
        // 0x41 -> 0 -> 128; 0x43 -> +2 -> 130.
        assert_eq!(&texels[4 + 1..4 + 3], &[128, 130]);
    }
}
