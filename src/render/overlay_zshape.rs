//! Optional raw depth surfaces for Cell47F6A0's sloped ore/legacy7E arm.
//!
//! Init54530E..545458 loads SLOP01Z..04Z verbatim. CC_Draw_Shape4AEF71
//! attaches69E740's encoded frame-data pointer to a cropped-width surface;
//! it does not decode the RLE bytes or apply BUILDNGZ's remapping. The four
//! entire files share one RGBA texture, preserving linear reads into their
//! headers/following frames. Native reads beyond the allocation depend on
//! unrelated heap bytes; VERA defines those reads as signed zero.

use crate::assets::asset_manager::AssetManager;
use crate::assets::shp_file::ShpFile;
use crate::render::batch::{BatchRenderer, SpriteInstance, create_unorm_texture_view_on_device};
use crate::render::native_z::{ZGradient, pack_overlay_z_gradient};
use crate::util::rect::{clip_blit_rects, clip_rect};

#[derive(Clone, Copy, Debug)]
pub(crate) struct OverlayShapeRequest {
    pub slope: u8,
    pub offset: [i32; 2],
}

#[derive(Clone, Copy)]
struct ShapeFrame {
    rect: [i32; 4],
    data_offset: i32,
}

pub(crate) struct OverlayZShapes {
    pub bind_group: wgpu::BindGroup,
    frames: [Option<ShapeFrame>; 4],
}

impl OverlayZShapes {
    pub fn load(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batch: &BatchRenderer,
        assets: &AssetManager,
        theater_extension: &str,
    ) -> Option<Self> {
        let files: [Option<(ShapeFrame, Vec<u8>)>; 4] = std::array::from_fn(|index| {
            let name = format!("SLOP0{}Z.{theater_extension}", index + 1);
            // Native uses CCFile directly, rather than the sticky SHP cache.
            let bytes = assets.get_ref(&name)?;
            let shp = ShpFile::from_bytes(bytes).ok()?;
            let frame = shp.frames.first()?;
            if frame.frame_width == 0 || frame.frame_height == 0 {
                return None;
            }
            let data = ShpFile::encoded_frame_data(bytes, 0).ok()?;
            Some((
                ShapeFrame {
                    rect: [
                        i32::from(frame.frame_x),
                        i32::from(frame.frame_y),
                        i32::from(frame.frame_width),
                        i32::from(frame.frame_height),
                    ],
                    data_offset: i32::try_from(bytes.len() - data.len()).ok()?,
                },
                bytes.to_vec(),
            ))
        });
        let max_bytes = files.iter().flatten().map(|(_, bytes)| bytes.len()).max()?;
        let width = 256;
        let height = (max_bytes as u32).div_ceil(width);
        // The files are uploaded whole as 256 px rows. A mod's slope file
        // past the device's textures: overlays take row depth, as `apply`
        // reads a missing shape, rather than failing validation.
        if !crate::render::gpu::texture_fits(device, [width, height]) {
            log::warn!(
                "SLOP0?Z.{theater_extension}: {max_bytes} bytes make a {width}x{height} px texture past this GPU's {} px; overlays take row depth",
                device.limits().max_texture_dimension_2d
            );
            return None;
        }
        let mut texels = vec![128; (width * height * 4) as usize];
        let mut frames = [None; 4];
        for (channel, file) in files.into_iter().enumerate() {
            let Some((frame, bytes)) = file else { continue };
            frames[channel] = Some(frame);
            for (index, byte) in bytes.into_iter().enumerate() {
                texels[index * 4 + channel] = byte.wrapping_add(128);
            }
        }
        let view = create_unorm_texture_view_on_device(
            device,
            queue,
            "Raw overlay depth files",
            &texels,
            width,
            height,
            wgpu::TextureFormat::Rgba8Unorm,
        );
        Some(Self {
            bind_group: batch.create_zshape_bind_group_on_device(device, &view),
            frames,
        })
    }

    /// Original4AF038..4AF0FB then437AAB/7BBE20. The latter clips the
    /// destination and source together; advancing only UVs or discarding
    /// outside fragments does not reproduce its placement or seeded Z.
    pub fn apply(
        &self,
        request: OverlayShapeRequest,
        draw_point_offset: [i32; 2],
        extended: bool,
        camera: [f32; 2],
        viewport: [f32; 2],
        instance: &mut SpriteInstance,
    ) -> bool {
        let Some(frame) = request
            .slope
            .checked_sub(1)
            .and_then(|index| self.frames.get(usize::from(index)))
            .copied()
            .flatten()
        else {
            // A missing native optional-shape pointer takes ordinary row Z.
            return true;
        };
        let position = instance.position.map(|value| value.round() as i32);
        let size = instance.size.map(|value| value as i32);
        let origin = [0, 1].map(|axis| {
            position[axis] + draw_point_offset[axis] - request.offset[axis] - frame.rect[axis]
        });
        let intersection = clip_rect(
            [0, 0, size[0], size[1]],
            [
                origin[0] - position[0],
                origin[1] - position[1],
                frame.rect[2],
                frame.rect[3],
            ],
        );
        if intersection[2] <= 0 || intersection[3] <= 0 {
            return false;
        }
        let camera = camera.map(|value| value.round() as i32);
        let Some((destination, source)) = clip_blit_rects(
            [
                position[0] - camera[0],
                position[1] - camera[1],
                intersection[2],
                intersection[3],
            ],
            [0, 0, viewport[0].ceil() as i32, viewport[1].ceil() as i32],
            intersection,
            intersection,
        ) else {
            return false;
        };
        let uv_step = [
            instance.uv_size[0] / instance.size[0],
            instance.uv_size[1] / instance.size[1],
        ];
        for axis in 0..2 {
            instance.position[axis] = (destination[axis] + camera[axis]) as f32;
            instance.size[axis] = destination[axis + 2] as f32;
            // 437AC4..437AE7 advances encoded body data by the change to
            // the supplied source rectangle, not its absolute coordinate.
            // The first shape intersection already has a nonzero origin.
            let source_delta = source[axis].wrapping_sub(intersection[axis]);
            instance.uv_origin[axis] += source_delta as f32 * uv_step[axis];
            instance.uv_size[axis] = source[axis + 2] as f32 * uv_step[axis];
        }
        if extended {
            let stride = frame.rect[2];
            instance.zshape_origin = [
                (origin[0] - frame.data_offset % stride) as f32,
                (origin[1] - frame.data_offset / stride) as f32,
            ];
            instance.z_gradient = pack_overlay_z_gradient(
                ZGradient::from_index(instance.z_gradient),
                request.slope,
                stride as u16,
            );
        }
        true
    }
}

#[cfg(all(test, tests_render_a_r))]
mod tests {
    use super::OverlayZShapes;
    use crate::assets::asset_manager::{AssetManager, test_support::TestDirectory};
    use crate::render::batch::BatchRenderer;
    use crate::render::terrain_draw_gpu_tests::Gpu;

    /// A slope file is uploaded whole as 256 px rows, so one past the
    /// device's limit refuses the shapes instead of failing validation.
    #[test]
    #[ignore = "requires GPU; a slope file one row past the device's texture limit"]
    fn a_slope_file_past_the_device_limit_leaves_the_shapes_out() {
        let gpu = Gpu::new();
        let limit = gpu.device.limits().max_texture_dimension_2d as usize;
        // A valid one-frame SHP (a 1x1 canvas, a 1x1 raw frame at offset
        // 32), padded to one row more than the limit.
        let mut bytes = vec![0u8; 256 * (limit + 1)];
        bytes[2] = 1;
        bytes[4] = 1;
        bytes[6] = 1;
        bytes[8 + 4..8 + 8].copy_from_slice(&[1, 0, 1, 0]);
        bytes[8 + 20..8 + 24].copy_from_slice(&32u32.to_le_bytes());
        bytes[32] = 1;
        let root = TestDirectory::new("overlay-zshape-past-limit");
        std::fs::write(root.path().join("SLOP01Z.TEM"), &bytes).expect("write slope SHP");
        let assets = AssetManager::from_loose_root_for_test(root.path());
        let batch = BatchRenderer::new_with_device(
            &gpu.device,
            &gpu.queue,
            wgpu::TextureFormat::Bgra8UnormSrgb,
        );

        assert!(OverlayZShapes::load(&gpu.device, &gpu.queue, &batch, &assets, "TEM").is_none());
    }
}
