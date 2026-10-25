//! Native screen shake as a client-pixel presentation copy.
//!
//! GScreenClass::vt_entry_44 @ 0x004F45B0 calls
//! Screen__UpdatePrimarySurface @ 0x004F4780 after the composite frame is drawn.
//! It shifts/crops that surface into the primary client rectangle, filling
//! exposed strips with color zero. This pass applies the same rectangle
//! operation to an already composed output region. It does not move the camera
//! or change what was drawn.

/// GPU-only resources derived from the destination extent and format. The
/// offset itself remains borrowed input from the presentation-state owner.
pub(crate) struct ScreenPresentationPass {
    scratch: Option<wgpu::Texture>,
    scratch_extent: Option<wgpu::Extent3d>,
    scratch_format: Option<wgpu::TextureFormat>,
}

impl ScreenPresentationPass {
    /// Start without a snapshot; the first nonzero offset allocates one.
    pub(crate) fn new() -> Self {
        Self {
            scratch: None,
            scratch_extent: None,
            scratch_format: None,
        }
    }

    /// Shift central_rect by signed client/output pixels, leaving everything
    /// outside that rectangle untouched. The rectangle is [x, y, width,
    /// height] in destination pixels; positive X/Y move right/down.
    ///
    /// The target is first snapshotted because texture copies cannot read and
    /// write the same texture. A black clear followed by four complement
    /// restores and one overlapping source copy leaves only the translated
    /// central region changed. i64 keeps negating i32::MIN well-defined.
    pub(crate) fn apply_offset_px(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::Texture,
        central_rect_px: [u32; 4],
        offset_px: [i32; 2],
    ) {
        if offset_px == [0, 0] {
            return;
        }

        let target_width = target.width();
        let target_height = target.height();
        let [x, y, width, height] = central_rect_px;
        let left = x.min(target_width);
        let top = y.min(target_height);
        let right = x.saturating_add(width).min(target_width);
        let bottom = y.saturating_add(height).min(target_height);
        if left >= right || top >= bottom || target_width == 0 || target_height == 0 {
            return;
        }

        let width = right - left;
        let height = bottom - top;
        self.ensure_scratch(device, target, target_width, target_height);
        let scratch = self
            .scratch
            .as_ref()
            .expect("scratch matches the output texture");

        encoder.copy_texture_to_texture(
            target.as_image_copy(),
            scratch.as_image_copy(),
            extent(target_width, target_height),
        );

        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        {
            let _clear = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Shake Native Output Clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }

        // Restore the complement of the central rectangle. The command bar is
        // part of the composite's full-height region; the right sidebar and
        // any letterbox remain fixed.
        copy_rect(
            encoder,
            scratch,
            target,
            [0, 0],
            [0, 0],
            [target_width, top],
        );
        copy_rect(
            encoder,
            scratch,
            target,
            [0, bottom],
            [0, bottom],
            [target_width, target_height - bottom],
        );
        copy_rect(encoder, scratch, target, [0, top], [0, top], [left, height]);
        copy_rect(
            encoder,
            scratch,
            target,
            [right, top],
            [right, top],
            [target_width - right, height],
        );

        let dx = i64::from(offset_px[0]);
        let dy = i64::from(offset_px[1]);
        let src_left = 0_i64.max(-dx);
        let src_top = 0_i64.max(-dy);
        let src_right = i64::from(width).min(i64::from(width) - dx);
        let src_bottom = i64::from(height).min(i64::from(height) - dy);
        if src_left >= src_right || src_top >= src_bottom {
            return;
        }

        let src_x = i64::from(left) + src_left;
        let src_y = i64::from(top) + src_top;
        let dst_x = src_x + dx;
        let dst_y = src_y + dy;
        let copy_width = (src_right - src_left) as u32;
        let copy_height = (src_bottom - src_top) as u32;
        copy_rect(
            encoder,
            scratch,
            target,
            [src_x as u32, src_y as u32],
            [dst_x as u32, dst_y as u32],
            [copy_width, copy_height],
        );
    }

    fn ensure_scratch(
        &mut self,
        device: &wgpu::Device,
        target: &wgpu::Texture,
        width: u32,
        height: u32,
    ) {
        let wanted_extent = extent(width, height);
        let wanted_format = target.format();
        if self.scratch_extent == Some(wanted_extent) && self.scratch_format == Some(wanted_format)
        {
            return;
        }

        self.scratch = Some(device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Shake Native Output Snapshot"),
            size: wanted_extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wanted_format,
            usage: wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        }));
        self.scratch_extent = Some(wanted_extent);
        self.scratch_format = Some(wanted_format);
    }
}

fn extent(width: u32, height: u32) -> wgpu::Extent3d {
    wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    }
}

fn copy_rect(
    encoder: &mut wgpu::CommandEncoder,
    source: &wgpu::Texture,
    destination: &wgpu::Texture,
    source_origin: [u32; 2],
    destination_origin: [u32; 2],
    size: [u32; 2],
) {
    let [width, height] = size;
    if width == 0 || height == 0 {
        return;
    }
    encoder.copy_texture_to_texture(
        wgpu::TexelCopyTextureInfo {
            texture: source,
            mip_level: 0,
            origin: wgpu::Origin3d {
                x: source_origin[0],
                y: source_origin[1],
                z: 0,
            },
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyTextureInfo {
            texture: destination,
            mip_level: 0,
            origin: wgpu::Origin3d {
                x: destination_origin[0],
                y: destination_origin[1],
                z: 0,
            },
            aspect: wgpu::TextureAspect::All,
        },
        extent(width, height),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::frame_readback::PendingBgra8Readback;
    use crate::render::gpu::block_on;
    use std::time::Duration;
    use wgpu::util::DeviceExt;

    const WIDTH: u32 = 12;
    const HEIGHT: u32 = 8;
    const CENTRAL_WIDTH: u32 = 8;
    const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;

    struct TestGpu {
        device: wgpu::Device,
        queue: wgpu::Queue,
    }

    impl TestGpu {
        fn new() -> Self {
            let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
                backends: wgpu::Backends::PRIMARY,
                ..wgpu::InstanceDescriptor::new_without_display_handle()
            });
            let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: None,
                ..Default::default()
            }))
            .expect("screen presentation GPU regression requires a wgpu adapter");
            let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("Screen presentation regression device"),
                ..Default::default()
            }))
            .expect("request screen presentation test device");
            Self { device, queue }
        }

        fn patterned_target(&self) -> (wgpu::Texture, Vec<[u8; 4]>) {
            let pixels = (0..HEIGHT)
                .flat_map(|y| (0..WIDTH).map(move |x| pixel(x, y)))
                .collect::<Vec<_>>();
            let bytes = pixels.iter().flatten().copied().collect::<Vec<_>>();
            let texture = self.device.create_texture_with_data(
                &self.queue,
                &wgpu::TextureDescriptor {
                    label: Some("Screen presentation patterned output"),
                    size: extent(WIDTH, HEIGHT),
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: FORMAT,
                    usage: wgpu::TextureUsages::COPY_SRC
                        | wgpu::TextureUsages::COPY_DST
                        | wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                },
                wgpu::util::TextureDataOrder::LayerMajor,
                &bytes,
            );
            (texture, pixels)
        }

        fn readback(
            &self,
            encoder: &mut wgpu::CommandEncoder,
            texture: &wgpu::Texture,
        ) -> PendingBgra8Readback {
            PendingBgra8Readback::encode(&self.device, encoder, texture, FORMAT, WIDTH, HEIGHT)
                .expect("encode output readback")
        }
    }

    fn pixel(x: u32, y: u32) -> [u8; 4] {
        [x as u8 + 11, y as u8 + 31, (x + y * WIDTH) as u8 + 51, 255]
    }

    fn read_pixel(bytes: &[u8], x: u32, y: u32) -> [u8; 4] {
        let index = ((y * WIDTH + x) * 4) as usize;
        bytes[index..index + 4].try_into().expect("four-byte pixel")
    }

    #[test]
    #[ignore = "requires a wgpu adapter for production GPU readback"]
    fn screen_presentation_shifts_signed_axes_and_preserves_the_sidebar() {
        let gpu = TestGpu::new();
        let (forward_target, forward_original) = gpu.patterned_target();
        let (reverse_target, reverse_original) = gpu.patterned_target();
        let (overflow_target, overflow_original) = gpu.patterned_target();
        let mut pass = ScreenPresentationPass::new();
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Screen presentation readback"),
            });

        pass.apply_offset_px(
            &gpu.device,
            &mut encoder,
            &forward_target,
            [0, 0, CENTRAL_WIDTH, HEIGHT],
            [1, -1],
        );
        let forward_readback = gpu.readback(&mut encoder, &forward_target);

        pass.apply_offset_px(
            &gpu.device,
            &mut encoder,
            &reverse_target,
            [0, 0, CENTRAL_WIDTH, HEIGHT],
            [-1, 1],
        );
        let reverse_readback = gpu.readback(&mut encoder, &reverse_target);

        pass.apply_offset_px(
            &gpu.device,
            &mut encoder,
            &overflow_target,
            [0, 0, CENTRAL_WIDTH, HEIGHT],
            [i32::MIN, 0],
        );
        let overflow_readback = gpu.readback(&mut encoder, &overflow_target);

        let submission = gpu.queue.submit([encoder.finish()]);
        let forward = forward_readback
            .finish(&gpu.device, submission.clone(), Duration::from_secs(5))
            .expect("read translated output");
        let reverse = reverse_readback
            .finish(&gpu.device, submission.clone(), Duration::from_secs(5))
            .expect("read reverse translated output");
        let overflowed = overflow_readback
            .finish(&gpu.device, submission, Duration::from_secs(5))
            .expect("read clear-only output");

        let black = [0, 0, 0, 255];
        assert_eq!(read_pixel(&forward, 0, 0), black);
        assert_eq!(
            read_pixel(&forward, 1, 4),
            forward_original[(5 * WIDTH) as usize]
        );
        assert_eq!(
            read_pixel(&forward, 7, 6),
            forward_original[(7 * WIDTH + 6) as usize]
        );
        assert_eq!(read_pixel(&forward, 3, 7), black);
        assert_eq!(
            read_pixel(&forward, 8, 4),
            forward_original[(4 * WIDTH + 8) as usize]
        );

        assert_eq!(read_pixel(&reverse, 7, 4), black);
        assert_eq!(read_pixel(&reverse, 4, 0), black);
        assert_eq!(read_pixel(&reverse, 0, 1), reverse_original[1]);
        assert_eq!(
            read_pixel(&reverse, 6, 6),
            reverse_original[(5 * WIDTH + 7) as usize]
        );
        assert_eq!(
            read_pixel(&reverse, 8, 4),
            reverse_original[(4 * WIDTH + 8) as usize]
        );

        assert_eq!(read_pixel(&overflowed, 0, 0), black);
        assert_eq!(read_pixel(&overflowed, 7, 7), black);
        assert_eq!(
            read_pixel(&overflowed, 8, 4),
            overflow_original[(4 * WIDTH + 8) as usize]
        );

        // The retail screenshot path reads the prior presented cursor-free
        // frame, then applies that frame's committed shake snapshot before
        // readback. This shares the production output pass and keeps the
        // screenshot one presentation behind the currently drawn frame.
        let (capture_source, capture_original) = gpu.patterned_target();
        let cache = crate::render::screenshot::PresentedFrameCache::default();
        let mut stage = gpu.device.create_command_encoder(&Default::default());
        cache.stage_pre_cursor_composition(
            &gpu.device,
            &mut stage,
            &capture_source,
            FORMAT,
            WIDTH,
            HEIGHT,
            WIDTH,
            HEIGHT,
        );
        gpu.queue.submit([stage.finish()]);
        cache.commit_presented([1, -1]);
        let mut capture_encoder = gpu.device.create_command_encoder(&Default::default());
        let capture = cache
            .capture_previous_if_requested(
                true,
                &gpu.device,
                &mut capture_encoder,
                FORMAT,
                WIDTH,
                HEIGHT,
                None,
                &mut pass,
                [0, 0, CENTRAL_WIDTH, HEIGHT],
            )
            .expect("capture shaken prior frame")
            .expect("capture requested");
        let submission = gpu.queue.submit([capture_encoder.finish()]);
        let captured = capture
            .finish(&gpu.device, submission, Duration::from_secs(5))
            .expect("read shaken screenshot");
        assert_eq!(read_pixel(&captured, 0, 0), black);
        assert_eq!(
            read_pixel(&captured, 1, 4),
            capture_original[(5 * WIDTH) as usize]
        );
        assert_eq!(
            read_pixel(&captured, 8, 4),
            capture_original[(4 * WIDTH + 8) as usize]
        );
    }
}
