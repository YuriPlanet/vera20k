//! The presentation hook's identity requirement, on a real GPU.
//!
//! A chain with no effects must be indistinguishable from the texture copy it
//! replaces: same texels, same order, same bits. These tests build the
//! production `PostChain` and the production readback, copy one pattern both
//! ways and compare the bytes. They use the surface format the game presents
//! in, and odd dimensions so the row arithmetic cannot hide behind alignment.
//!
//! Run explicitly on a machine with a wgpu adapter:
//! `cargo test -p vera20k --lib render::post_process::gpu_tests:: -- --ignored`

use std::time::Duration;

use crate::render::frame_readback::PendingBgra8Readback;
use crate::render::gpu::block_on;
use crate::render::post_process::{PostChain, PostParams};
use crate::rules::post_process::DlaaRules;
use crate::render::water_distortion::{DistortionParams, WaterDistortion};
use crate::render::water_mask::WaterMask;

/// The format the window's surface reports once `add_srgb_suffix` is applied.
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
}

impl Gpu {
    fn new() -> Self {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: None,
            ..Default::default()
        }))
        .expect("explicit GPU regression test requires a wgpu adapter");
        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("Post process regression device"),
            ..Default::default()
        }))
        .expect("request offscreen test device");
        Self { device, queue }
    }

    fn texture(&self, size: [u32; 2], usage: wgpu::TextureUsages) -> wgpu::Texture {
        self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Post process test target"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage,
            view_formats: &[],
        })
    }

    fn upload(&self, texture: &wgpu::Texture, bytes: &[u8], size: [u32; 2]) {
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size[0] * 4),
                rows_per_image: Some(size[1]),
            },
            wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
        );
    }

    /// An `R8Unorm` mask with the left `water_width` columns set to 1.
    fn mask(&self, size: [u32; 2], water_width: u32) -> wgpu::Texture {
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Post process water mask"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let mut bytes = vec![0u8; (size[0] * size[1]) as usize];
        for y in 0..size[1] {
            for x in 0..water_width.min(size[0]) {
                bytes[(y * size[0] + x) as usize] = 255;
            }
        }
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size[0]),
                rows_per_image: Some(size[1]),
            },
            wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
        );
        texture
    }

    fn read(&self, texture: &wgpu::Texture, size: [u32; 2]) -> Vec<u8> {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Post process readback"),
            });
        let readback = PendingBgra8Readback::encode(
            &self.device,
            &mut encoder,
            texture,
            FORMAT,
            size[0],
            size[1],
        )
        .expect("encode offscreen readback");
        let submission = self.queue.submit([encoder.finish()]);
        readback
            .finish(&self.device, submission, Duration::from_secs(10))
            .expect("read GPU pixels")
    }

    /// Read a single-channel `R8Unorm` texture, dropping the row padding.
    fn read_r8(&self, texture: &wgpu::Texture, size: [u32; 2]) -> Vec<u8> {
        self.read_plane(texture, size, 1)
    }

    /// Read an `Rg16Float` texture as raw half-float pairs, dropping the row
    /// padding.
    fn read_rg16f(&self, texture: &wgpu::Texture, size: [u32; 2]) -> Vec<u8> {
        self.read_plane(texture, size, 4)
    }

    fn read_plane(&self, texture: &wgpu::Texture, size: [u32; 2], bpp: u32) -> Vec<u8> {
        let padded_row = (size[0] * bpp).div_ceil(256) * 256;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Plane readback"),
            size: u64::from(padded_row) * u64::from(size[1]),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Plane readback"),
            });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row),
                    rows_per_image: Some(size[1]),
                },
            },
            wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);
        buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("wait for plane readback");
        let mapped = buffer
            .slice(..)
            .get_mapped_range()
            .expect("map plane readback");
        let row_bytes = (size[0] * bpp) as usize;
        let mut out = Vec::with_capacity(row_bytes * size[1] as usize);
        for y in 0..size[1] as usize {
            let start = y * padded_row as usize;
            out.extend_from_slice(&mapped[start..start + row_bytes]);
        }
        drop(mapped);
        buffer.unmap();
        out
    }

    /// An `Rg16Float` displacement field: the left `water_width` columns push
    /// the frame +x by `amplitude` pixels (R = 1.0), the rest stays neutral.
    fn distortion(&self, size: [u32; 2], water_width: u32) -> wgpu::Texture {
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Post process water distortion"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rg16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        // Half-float 1.0 is 0x3C00, 0.5 is 0x3800, in little-endian pairs.
        let water = [0x00u8, 0x3C, 0x00, 0x38];
        let neutral = [0x00u8, 0x38, 0x00, 0x38];
        let mut bytes = Vec::with_capacity((size[0] * size[1] * 4) as usize);
        for _ in 0..size[1] {
            for x in 0..size[0] {
                bytes.extend_from_slice(if x < water_width { &water } else { &neutral });
            }
        }
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size[0] * 4),
                rows_per_image: Some(size[1]),
            },
            wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
        );
        texture
    }
}

/// A deterministic pattern with four distinct channels per texel, so a row or
/// channel swap cannot pass as equal.
fn pattern(size: [u32; 2]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity((size[0] * size[1] * 4) as usize);
    for y in 0..size[1] {
        for x in 0..size[0] {
            bytes.push((x * 37 + y * 11) as u8);
            bytes.push((x * 5 + y * 61) as u8);
            bytes.push((x * 3 + y * 7 + 91) as u8);
            bytes.push(255);
        }
    }
    bytes
}

#[test]
#[ignore = "needs a wgpu adapter; run explicitly"]
fn the_identity_chain_reproduces_the_frame_copy_byte_for_byte() {
    for size in [[23, 17], [1, 1], [64, 48]] {
        let gpu = Gpu::new();
        let source = gpu.texture(
            size,
            wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC,
        );
        gpu.upload(&source, &pattern(size), size);

        let copied = gpu.texture(
            size,
            wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC,
        );
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Post process copy"),
            });
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &source,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
                texture: &copied,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
        );
        gpu.queue.submit([encoder.finish()]);

        let chained = gpu.texture(
            size,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        );
        let mut chain = PostChain::new(
            &gpu.device,
            &gpu.queue,
            FORMAT,
            super::PostParams::identity(),
        );
        assert_eq!(chain.format(), FORMAT);
        let source_view = source.create_view(&wgpu::TextureViewDescriptor::default());
        let chained_view = chained.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Post process chain"),
            });
        let mask = gpu.mask(size, 0);
        let mask_view = mask.create_view(&wgpu::TextureViewDescriptor::default());
        chain.draw(
            &gpu.device,
            &gpu.queue,
            &mut encoder,
            &source_view,
            &mask_view,
            &chained_view,
            (size[0], size[1]),
        );
        gpu.queue.submit([encoder.finish()]);

        let copy_bytes = gpu.read(&copied, size);
        let chain_bytes = gpu.read(&chained, size);
        assert_eq!(
            copy_bytes.len(),
            chain_bytes.len(),
            "size {size:?}: readback lengths differ"
        );
        assert_eq!(
            chain_bytes, copy_bytes,
            "size {size:?}: the identity chain must equal the texture copy"
        );
    }
}

/// The colour-grade arm runs and reads the uniform: saturation 0 turns opaque
/// pure red into one grey value, so a lost or stale params buffer is visible.
#[test]
#[ignore = "needs a wgpu adapter; run explicitly"]
fn the_colour_grade_pass_reads_its_uniform() {
    let gpu = Gpu::new();
    let size = [1, 1];
    let source = gpu.texture(
        size,
        wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC,
    );
    // Opaque pure red in BGRA byte order.
    gpu.upload(&source, &[0, 0, 255, 255], size);

    let target = gpu.texture(
        size,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let mut chain = PostChain::new(
        &gpu.device,
        &gpu.queue,
        FORMAT,
        super::PostParams::color_grade(0.0, 1.0, 0.0, [0.0, 0.0, 1.0, 1.0]),
    );
    let source_view = source.create_view(&wgpu::TextureViewDescriptor::default());
    let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Post process colour grade"),
        });
    let mask = gpu.mask(size, 0);
    let mask_view = mask.create_view(&wgpu::TextureViewDescriptor::default());
    chain.draw(
        &gpu.device,
        &gpu.queue,
        &mut encoder,
        &source_view,
        &mask_view,
        &target_view,
        (size[0], size[1]),
    );
    gpu.queue.submit([encoder.finish()]);

    let bytes = gpu.read(&target, size);
    assert_eq!(bytes[3], 255, "alpha passes through: {bytes:?}");
    assert_eq!(bytes[0], bytes[1], "luma leaves equal B/G: {bytes:?}");
    assert_eq!(bytes[1], bytes[2], "luma leaves equal G/R: {bytes:?}");
    assert!(
        bytes[2] < 200,
        "saturated red must lose its red channel: {bytes:?}"
    );
}

/// The viewport mask keeps UI texels on the copy path: a 2×1 frame with a
/// one-pixel tactical viewport grades the left texel and leaves the right one
/// untouched, which is how the sidebar column survives grading.
#[test]
#[ignore = "needs a wgpu adapter; run explicitly"]
fn the_viewport_mask_excludes_the_ui_column() {
    let gpu = Gpu::new();
    let size = [2, 1];
    let source = gpu.texture(
        size,
        wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC,
    );
    // Two opaque pure-red texels in BGRA byte order.
    gpu.upload(&source, &[0, 0, 255, 255, 0, 0, 255, 255], size);

    let target = gpu.texture(
        size,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let mut chain = PostChain::new(
        &gpu.device,
        &gpu.queue,
        FORMAT,
        super::PostParams::color_grade(0.0, 1.0, 0.0, [0.0, 0.0, 1.0, 1.0]),
    );
    let source_view = source.create_view(&wgpu::TextureViewDescriptor::default());
    let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Post process viewport mask"),
        });
    let mask = gpu.mask(size, 0);
    let mask_view = mask.create_view(&wgpu::TextureViewDescriptor::default());
    chain.draw(
        &gpu.device,
        &gpu.queue,
        &mut encoder,
        &source_view,
        &mask_view,
        &target_view,
        (size[0], size[1]),
    );
    gpu.queue.submit([encoder.finish()]);

    let bytes = gpu.read(&target, size);
    let graded = &bytes[0..4];
    assert_eq!(graded[0], graded[1], "inside: equal B/G: {graded:?}");
    assert_eq!(graded[1], graded[2], "inside: equal G/R: {graded:?}");
    assert!(graded[2] < 200, "inside: red dropped: {graded:?}");
    assert_eq!(&bytes[4..8], &[0, 0, 255, 255], "outside: byte copy");
}

/// FilmicPass runs in gamma space: a neutral mid-grey moves off its input while
/// alpha and channel balance survive.
#[test]
#[ignore = "needs a wgpu adapter; run explicitly"]
fn the_filmic_pass_moves_mid_grey() {
    let gpu = Gpu::new();
    let size = [1, 1];
    let source = gpu.texture(
        size,
        wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC,
    );
    // Opaque mid-grey in BGRA byte order.
    gpu.upload(&source, &[128, 128, 128, 255], size);

    let target = gpu.texture(
        size,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let mut chain = PostChain::new(
        &gpu.device,
        &gpu.queue,
        FORMAT,
        super::PostParams::filmic([0.0, 0.0, 1.0, 1.0]),
    );
    let source_view = source.create_view(&wgpu::TextureViewDescriptor::default());
    let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Post process filmic"),
        });
    let mask = gpu.mask(size, 0);
    let mask_view = mask.create_view(&wgpu::TextureViewDescriptor::default());
    chain.draw(
        &gpu.device,
        &gpu.queue,
        &mut encoder,
        &source_view,
        &mask_view,
        &target_view,
        (size[0], size[1]),
    );
    gpu.queue.submit([encoder.finish()]);

    let bytes = gpu.read(&target, size);
    assert_eq!(bytes[3], 255, "alpha passes through: {bytes:?}");
    assert_ne!(bytes[0], 128, "filmic must move mid grey: {bytes:?}");
    assert_eq!(bytes[0], bytes[1], "neutral tint keeps B/G equal: {bytes:?}");
    assert_eq!(bytes[1], bytes[2], "neutral tint keeps G/R equal: {bytes:?}");
}

/// Sepia multiplies toward its warm tint: a mid-grey gains a red bias while
/// alpha stays opaque.
#[test]
#[ignore = "needs a wgpu adapter; run explicitly"]
fn the_sepia_pass_warms_mid_grey() {
    let gpu = Gpu::new();
    let size = [1, 1];
    let source = gpu.texture(
        size,
        wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC,
    );
    gpu.upload(&source, &[128, 128, 128, 255], size);

    let target = gpu.texture(
        size,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let mut chain = PostChain::new(
        &gpu.device,
        &gpu.queue,
        FORMAT,
        super::PostParams::sepia([0.0, 0.0, 1.0, 1.0]),
    );
    let source_view = source.create_view(&wgpu::TextureViewDescriptor::default());
    let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Post process sepia"),
        });
    let mask = gpu.mask(size, 0);
    let mask_view = mask.create_view(&wgpu::TextureViewDescriptor::default());
    chain.draw(
        &gpu.device,
        &gpu.queue,
        &mut encoder,
        &source_view,
        &mask_view,
        &target_view,
        (size[0], size[1]),
    );
    gpu.queue.submit([encoder.finish()]);

    let bytes = gpu.read(&target, size);
    assert_eq!(bytes[3], 255, "alpha passes through: {bytes:?}");
    assert!(bytes[2] > bytes[1], "red outruns green: {bytes:?}");
    assert!(bytes[1] > bytes[0], "green outruns blue: {bytes:?}");
}

/// FXAA blends texels across a hard edge while flat interiors stay byte copies.
#[test]
#[ignore = "needs a wgpu adapter; run explicitly"]
fn the_fxaa_pass_blends_only_the_edge() {
    let gpu = Gpu::new();
    let size = [8, 8];
    let mut source_bytes = Vec::new();
    for _ in 0..size[1] {
        for x in 0..size[0] {
            let value = if x < 4 { 0 } else { 255 };
            source_bytes.extend_from_slice(&[value, value, value, 255]);
        }
    }

    let source = gpu.texture(
        size,
        wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC,
    );
    gpu.upload(&source, &source_bytes, size);

    let target = gpu.texture(
        size,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let mut chain = PostChain::new(
        &gpu.device,
        &gpu.queue,
        FORMAT,
        super::PostParams::fxaa([0.0, 0.0, 8.0, 8.0]),
    );
    let source_view = source.create_view(&wgpu::TextureViewDescriptor::default());
    let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Post process fxaa"),
        });
    let mask = gpu.mask(size, 0);
    let mask_view = mask.create_view(&wgpu::TextureViewDescriptor::default());
    chain.draw(
        &gpu.device,
        &gpu.queue,
        &mut encoder,
        &source_view,
        &mask_view,
        &target_view,
        (size[0], size[1]),
    );
    gpu.queue.submit([encoder.finish()]);

    let out = gpu.read(&target, size);
    let stride = size[0] as usize * 4;
    for y in 0..size[1] as usize {
        let left = y * stride;
        let right = y * stride + 7 * 4;
        assert_eq!(
            &out[left..left + 4],
            &source_bytes[left..left + 4],
            "row {y}: the left interior must stay a byte copy"
        );
        assert_eq!(
            &out[right..right + 4],
            &source_bytes[right..right + 4],
            "row {y}: the right interior must stay a byte copy"
        );
    }
    let edge_changed = (0..size[1] as usize).any(|y| {
        (2..=5).any(|x| {
            let start = y * stride + x * 4;
            out[start..start + 4] != source_bytes[start..start + 4]
        })
    });
    assert!(edge_changed, "FXAA must reshape the hard edge");
}

/// Bloom keeps the bright block saturated, spills a halo beside it, and leaves
/// the far corners close to black.
#[test]
#[ignore = "needs a wgpu adapter; run explicitly"]
fn the_bloom_chain_spills_a_local_halo() {
    let gpu = Gpu::new();
    let size = [64, 64];
    let mut source_bytes = Vec::new();
    for y in 0..size[1] {
        for x in 0..size[0] {
            let value = if (24..40).contains(&x) && (24..40).contains(&y) {
                255
            } else {
                0
            };
            source_bytes.extend_from_slice(&[value, value, value, 255]);
        }
    }

    let source = gpu.texture(
        size,
        wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC,
    );
    gpu.upload(&source, &source_bytes, size);

    let target = gpu.texture(
        size,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let mut chain = PostChain::new(
        &gpu.device,
        &gpu.queue,
        FORMAT,
        super::PostParams::bloom(0.4, 0.6, [0.0, 0.0, 64.0, 64.0]),
    );
    let source_view = source.create_view(&wgpu::TextureViewDescriptor::default());
    let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Post process bloom"),
        });
    let mask = gpu.mask(size, 0);
    let mask_view = mask.create_view(&wgpu::TextureViewDescriptor::default());
    chain.draw(
        &gpu.device,
        &gpu.queue,
        &mut encoder,
        &source_view,
        &mask_view,
        &target_view,
        (size[0], size[1]),
    );
    gpu.queue.submit([encoder.finish()]);

    let out = gpu.read(&target, size);
    let stride = size[0] as usize * 4;
    let texel = |x: usize, y: usize| {
        let start = y * stride + x * 4;
        [out[start], out[start + 1], out[start + 2], out[start + 3]]
    };

    let centre = texel(31, 31);
    assert!(centre[0] >= 250, "the bright block stays saturated: {centre:?}");
    assert_eq!(centre[3], 255, "alpha passes through: {centre:?}");

    let halo = texel(20, 32);
    assert!(halo[2] > 0, "bloom must spill beside the block: {halo:?}");

    // Four upsample levels spread the glow wide, so the far corner is dim
    // rather than zero; the point is that it stays well below the block.
    let corner = texel(0, 0);
    assert!(
        corner[2] <= 48,
        "the far corner stays dim: {corner:?}"
    );
}

/// Sharpen deepens a soft edge while blur softens it: each arm reads its
/// uniform and only moves the texels beside the step.
#[test]
#[ignore = "needs a wgpu adapter; run explicitly"]
fn the_sharpen_and_blur_arms_move_the_edge_in_opposite_directions() {
    let gpu = Gpu::new();
    let size = [32, 8];
    let mut source_bytes = Vec::new();
    for _ in 0..size[1] {
        for x in 0..size[0] {
            let value = if x < 16 { 64 } else { 192 };
            source_bytes.extend_from_slice(&[value, value, value, 255]);
        }
    }
    let source = gpu.texture(
        size,
        wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC,
    );
    gpu.upload(&source, &source_bytes, size);

    let run = |params: PostParams| {
        let target = gpu.texture(
            size,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        );
        let source_view = source.create_view(&wgpu::TextureViewDescriptor::default());
        let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let mask = gpu.mask(size, 0);
        let mask_view = mask.create_view(&wgpu::TextureViewDescriptor::default());
        let mut chain = PostChain::new(&gpu.device, &gpu.queue, FORMAT, params);
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Post process sharpen/blur"),
            });
        chain.draw(
            &gpu.device,
            &gpu.queue,
            &mut encoder,
            &source_view,
            &mask_view,
            &target_view,
            (size[0], size[1]),
        );
        gpu.queue.submit([encoder.finish()]);
        gpu.read(&target, size)
    };

    // The test helpers live in `render::post_process::PostParams`, gated to the
    // crate's tests, so build the uniforms through the production entry point.
    let sharpen = PostParams::from_effects(
        None,
        None,
        None,
        None,
        None,
        None,
        Some([0.6, 0.5]),
        None,
        Default::default(),
        None,
        false,
        [0.0, 0.0, size[0] as f32, size[1] as f32],
    )
    .expect("sharpen configures the chain");
    let out = run(sharpen);
    let stride = size[0] as usize * 4;
    let at = |x: usize, y: usize| {
        let start = y * stride + x * 4;
        out[start]
    };
    assert!(at(15, 4) < 64, "the dark side darkens: {}", at(15, 4));
    assert!(at(16, 4) > 192, "the bright side brightens: {}", at(16, 4));

    let blur = PostParams::from_effects(
        None,
        None,
        None,
        None,
        None,
        Some([2.0, 1.0]),
        None,
        None,
        Default::default(),
        None,
        false,
        [0.0, 0.0, size[0] as f32, size[1] as f32],
    )
    .expect("blur configures the chain");
    let out = run(blur);
    let at = |x: usize, y: usize| {
        let start = y * stride + x * 4;
        out[start]
    };
    assert!(at(15, 4) > 64, "the dark edge lightens: {}", at(15, 4));
    assert!(at(16, 4) < 192, "the bright edge dims: {}", at(16, 4));
    assert_eq!(at(2, 4), 64, "the far interior is left alone");
    assert_eq!(at(29, 4), 192, "the far interior is left alone");
}

/// The LUT arm samples the `.cube` table it is given: a table of one flat
/// colour replaces the frame at full strength, which catches a table uploaded
/// with the wrong orientation or ignored by the shader.
#[test]
#[ignore = "needs a wgpu adapter; run explicitly"]
fn the_lut_arm_reads_the_cube_table() {
    let gpu = Gpu::new();
    let size = [2, 2];
    let source = gpu.texture(
        size,
        wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC,
    );
    let source_bytes = pattern(size);
    gpu.upload(&source, &source_bytes, size);

    // Every entry of a 2x2x2 table is the same blue, so trilinear interpolation
    // cannot blur the assertion.
    let mut cube = String::from("LUT_3D_SIZE 2\n");
    for _ in 0..8 {
        cube.push_str("0.0 0.0 1.0\n");
    }
    let lut = super::PostLut::from_cube(&gpu.device, &gpu.queue, &cube).expect("parse cube");

    let target = gpu.texture(
        size,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let params = PostParams::identity()
        .with_color(None, [1.0, 0.0, 0.0])
        .with_viewport([0.0, 0.0, size[0] as f32, size[1] as f32]);
    let mut chain = PostChain::new(&gpu.device, &gpu.queue, FORMAT, params);
    chain.set_luts([Some(lut.view().clone()), None, None]);
    let source_view = source.create_view(&wgpu::TextureViewDescriptor::default());
    let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let mask = gpu.mask(size, 0);
    let mask_view = mask.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Post process lut"),
        });
    chain.draw(
        &gpu.device,
        &gpu.queue,
        &mut encoder,
        &source_view,
        &mask_view,
        &target_view,
        (size[0], size[1]),
    );
    gpu.queue.submit([encoder.finish()]);

    let out = gpu.read(&target, size);
    for texel in out.chunks_exact(4) {
        // Readback bytes are BGRA: the table's blue lands in the first lane.
        assert!(texel[0] >= 250, "the table's blue replaces the frame: {texel:?}");
        assert!(texel[1] <= 5, "the table's green stays near zero: {texel:?}");
        assert_eq!(texel[3], 255, "alpha passes through: {texel:?}");
    }
}

/// MultiLUT applies its tables in order: the second table receives the colour
/// the first produced, so a flat red table followed by a flat green one ends
/// green. This also proves binding 7 is the second table.
#[test]
#[ignore = "needs a wgpu adapter; run explicitly"]
fn the_second_lut_refines_the_first() {
    let gpu = Gpu::new();
    let size = [2, 2];
    let source = gpu.texture(
        size,
        wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC,
    );
    let source_bytes = pattern(size);
    gpu.upload(&source, &source_bytes, size);

    let flat = |colour: &str| {
        let mut cube = String::from("LUT_3D_SIZE 2\n");
        for _ in 0..8 {
            cube.push_str(colour);
            cube.push('\n');
        }
        super::PostLut::from_cube(&gpu.device, &gpu.queue, &cube).expect("parse cube")
    };
    let red = flat("1.0 0.0 0.0");
    let green = flat("0.0 1.0 0.0");

    let target = gpu.texture(
        size,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let params = PostParams::identity()
        .with_color(None, [1.0, 1.0, 0.0])
        .with_viewport([0.0, 0.0, size[0] as f32, size[1] as f32]);
    let mut chain = PostChain::new(&gpu.device, &gpu.queue, FORMAT, params);
    chain.set_luts([
        Some(red.view().clone()),
        Some(green.view().clone()),
        None,
    ]);
    let source_view = source.create_view(&wgpu::TextureViewDescriptor::default());
    let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let mask = gpu.mask(size, 0);
    let mask_view = mask.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Post process multilut"),
        });
    chain.draw(
        &gpu.device,
        &gpu.queue,
        &mut encoder,
        &source_view,
        &mask_view,
        &target_view,
        (size[0], size[1]),
    );
    gpu.queue.submit([encoder.finish()]);

    let out = gpu.read(&target, size);
    for texel in out.chunks_exact(4) {
        // BGRA readback: green lands in the second lane.
        assert!(texel[1] >= 250, "the second table wins: {texel:?}");
        assert!(texel[0] <= 5 && texel[2] <= 5, "red is replaced: {texel:?}");
        assert_eq!(texel[3], 255, "alpha passes through: {texel:?}");
    }
}

/// The `MultiLUT` arm samples the atlas at binding 9. A sheet whose every texel
/// is blue forces the result to blue whichever selector/tile the coordinate
/// maths lands on, which catches the arm being skipped or the sheet unbound.
#[test]
#[ignore = "needs a wgpu adapter; run explicitly"]
fn the_atlas_arm_samples_the_multilut_sheet() {
    let gpu = Gpu::new();
    let size = [2, 2];
    let source = gpu.texture(
        size,
        wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC,
    );
    let source_bytes = pattern(size);
    gpu.upload(&source, &source_bytes, size);

    // `MultiLut_atlas1.png`'s shape: 32 tiles across, 17 LUT rows.
    let sheet_size = [32 * 32, 32 * 17];
    let atlas = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Post process MultiLUT test atlas"),
        size: wgpu::Extent3d {
            width: sheet_size[0],
            height: sheet_size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let mut sheet = vec![0u8; (sheet_size[0] * sheet_size[1] * 4) as usize];
    for texel in sheet.chunks_exact_mut(4) {
        texel[2] = 255;
        texel[3] = 255;
    }
    gpu.queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &atlas,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &sheet,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(sheet_size[0] * 4),
            rows_per_image: Some(sheet_size[1]),
        },
        wgpu::Extent3d {
            width: sheet_size[0],
            height: sheet_size[1],
            depth_or_array_layers: 1,
        },
    );
    let atlas_view = atlas.create_view(&wgpu::TextureViewDescriptor::default());

    let target = gpu.texture(
        size,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let params = PostParams::identity()
        .with_atlas_lut(Some([1.0, 1.0, 1.0]))
        .with_viewport([0.0, 0.0, size[0] as f32, size[1] as f32]);
    let mut chain = PostChain::new(&gpu.device, &gpu.queue, FORMAT, params);
    chain.set_atlas(Some(atlas_view));
    let source_view = source.create_view(&wgpu::TextureViewDescriptor::default());
    let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let mask = gpu.mask(size, 0);
    let mask_view = mask.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Post process atlas"),
        });
    chain.draw(
        &gpu.device,
        &gpu.queue,
        &mut encoder,
        &source_view,
        &mask_view,
        &target_view,
        (size[0], size[1]),
    );
    gpu.queue.submit([encoder.finish()]);

    let out = gpu.read(&target, size);
    for texel in out.chunks_exact(4) {
        // BGRA readback: the sheet's blue lands in the first lane.
        assert!(texel[0] >= 250, "the sheet's blue reaches the frame: {texel:?}");
        assert!(texel[1] <= 5, "the sheet has no green: {texel:?}");
        assert_eq!(texel[3], 255, "alpha passes through: {texel:?}");
    }
}

/// The chain's DLAA finish runs as its own presentation stage: with DLAA
/// configured, a staircase edge must come out with intermediate texels even
/// though the main pass is the identity.
#[test]
#[ignore = "needs a wgpu adapter; run explicitly"]
fn the_dlaa_finish_softens_a_staircase_edge() {
    let gpu = Gpu::new();
    let size = [32, 32];
    let source = gpu.texture(
        size,
        wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC,
    );
    let mut source_bytes = Vec::new();
    for y in 0..size[1] {
        let edge = size[0] / 2 + y / 4;
        for x in 0..size[0] {
            let value: u8 = if x < edge { 40 } else { 220 };
            // BGRA with grey, so the channel order does not matter.
            source_bytes.extend_from_slice(&[value, value, value, 255]);
        }
    }
    gpu.upload(&source, &source_bytes, size);

    let target = gpu.texture(
        size,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let params = PostParams::identity().with_viewport([0.0, 0.0, size[0] as f32, size[1] as f32]);
    let mut chain = PostChain::new(&gpu.device, &gpu.queue, FORMAT, params);
    chain.set_dlaa(Some(DlaaRules::default()));
    let source_view = source.create_view(&wgpu::TextureViewDescriptor::default());
    let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let mask = gpu.mask(size, 0);
    let mask_view = mask.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Post process dlaa"),
        });
    chain.draw(
        &gpu.device,
        &gpu.queue,
        &mut encoder,
        &source_view,
        &mask_view,
        &target_view,
        (size[0], size[1]),
    );
    gpu.queue.submit([encoder.finish()]);

    let out = gpu.read(&target, size);
    let intermediate = out
        .chunks_exact(4)
        .filter(|texel| (60..200).contains(&texel[0]))
        .count();
    assert!(
        intermediate > 0,
        "the DLAA finish smooths the staircase steps"
    );
}

/// RCAS sharpens a soft edge but never overshoots its neighbours: the dark
/// side cannot fall below the dark texel and the bright side cannot pass the
/// bright one, which is what separates RCAS from a plain unsharp mask.
#[test]
#[ignore = "needs a wgpu adapter; run explicitly"]
fn the_rcas_arm_sharpens_without_overshoot() {
    let gpu = Gpu::new();
    let size = [32, 8];
    let mut source_bytes = Vec::new();
    for _ in 0..size[1] {
        for x in 0..size[0] {
            let value = if x < 16 { 64 } else { 192 };
            source_bytes.extend_from_slice(&[value, value, value, 255]);
        }
    }
    let source = gpu.texture(
        size,
        wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC,
    );
    gpu.upload(&source, &source_bytes, size);

    let target = gpu.texture(
        size,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let params = PostParams::from_effects(
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some(1.0),
        Default::default(),
        None,
        false,
        [0.0, 0.0, size[0] as f32, size[1] as f32],
    )
    .expect("super resolution configures the chain")
    .with_rcas(true);
    let mut chain = PostChain::new(&gpu.device, &gpu.queue, FORMAT, params);
    let source_view = source.create_view(&wgpu::TextureViewDescriptor::default());
    let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let mask = gpu.mask(size, 0);
    let mask_view = mask.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Post process rcas"),
        });
    chain.draw(
        &gpu.device,
        &gpu.queue,
        &mut encoder,
        &source_view,
        &mask_view,
        &target_view,
        (size[0], size[1]),
    );
    gpu.queue.submit([encoder.finish()]);

    let out = gpu.read(&target, size);
    let stride = size[0] as usize * 4;
    let at = |x: usize, y: usize| {
        let start = y * stride + x * 4;
        out[start]
    };
    assert!(at(15, 4) <= 64, "the dark side cannot undershoot: {}", at(15, 4));
    assert!(at(16, 4) >= 192, "the bright side cannot overshoot: {}", at(16, 4));
    assert_eq!(at(2, 4), 64, "the far interior is left alone");
}

/// Deband smooths the 1-bit steps inside a gradient while a flat band middle
/// stays a byte copy (no noise where nothing varies).
#[test]
#[ignore = "needs a wgpu adapter; run explicitly"]
fn the_deband_arm_softens_steps_but_not_flat_colour() {
    let gpu = Gpu::new();
    let size = [64, 8];
    let mut source_bytes = Vec::new();
    for _ in 0..size[1] {
        for x in 0..size[0] {
            // Four 16-pixel bands, each two 8-bit steps brighter than the last.
            let value = 100 + (x / 16) as u8 * 2;
            source_bytes.extend_from_slice(&[value, value, value, 255]);
        }
    }
    let source = gpu.texture(
        size,
        wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC,
    );
    gpu.upload(&source, &source_bytes, size);

    let target = gpu.texture(
        size,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let params = PostParams::identity()
        .with_quality(Some([3.0, 0.02, 1.0]), None, None)
        .with_viewport([0.0, 0.0, size[0] as f32, size[1] as f32]);
    let mut chain = PostChain::new(&gpu.device, &gpu.queue, FORMAT, params);
    let source_view = source.create_view(&wgpu::TextureViewDescriptor::default());
    let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let mask = gpu.mask(size, 0);
    let mask_view = mask.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Post process deband"),
        });
    chain.draw(
        &gpu.device,
        &gpu.queue,
        &mut encoder,
        &source_view,
        &mask_view,
        &target_view,
        (size[0], size[1]),
    );
    gpu.queue.submit([encoder.finish()]);

    let out = gpu.read(&target, size);
    let stride = size[0] as usize * 4;
    let at = |x: usize, y: usize| {
        let start = y * stride + x * 4;
        out[start]
    };
    assert!(
        at(8, 4).abs_diff(100) <= 1,
        "a flat band middle stays put: {}",
        at(8, 4)
    );
    assert!(
        at(24, 4).abs_diff(102) <= 1,
        "the next flat band middle stays too: {}",
        at(24, 4)
    );
    let softened = (14..=17).any(|x| at(x, 4) != 100 && at(x, 4) != 102);
    assert!(softened, "the band step gains intermediate values");
}

/// Clarity lifts wide local contrast: the edge pair separates further while
/// the flat interiors keep their bytes.
#[test]
#[ignore = "needs a wgpu adapter; run explicitly"]
fn the_clarity_arm_lifts_wide_local_contrast() {
    let gpu = Gpu::new();
    let size = [32, 8];
    let mut source_bytes = Vec::new();
    for _ in 0..size[1] {
        for x in 0..size[0] {
            let value = if x < 16 { 96 } else { 160 };
            source_bytes.extend_from_slice(&[value, value, value, 255]);
        }
    }
    let source = gpu.texture(
        size,
        wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC,
    );
    gpu.upload(&source, &source_bytes, size);

    let target = gpu.texture(
        size,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let params = PostParams::identity()
        .with_quality(None, Some([4.0, 0.6]), None)
        .with_viewport([0.0, 0.0, size[0] as f32, size[1] as f32]);
    let mut chain = PostChain::new(&gpu.device, &gpu.queue, FORMAT, params);
    let source_view = source.create_view(&wgpu::TextureViewDescriptor::default());
    let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let mask = gpu.mask(size, 0);
    let mask_view = mask.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Post process clarity"),
        });
    chain.draw(
        &gpu.device,
        &gpu.queue,
        &mut encoder,
        &source_view,
        &mask_view,
        &target_view,
        (size[0], size[1]),
    );
    gpu.queue.submit([encoder.finish()]);

    let out = gpu.read(&target, size);
    let stride = size[0] as usize * 4;
    let at = |x: usize, y: usize| {
        let start = y * stride + x * 4;
        out[start]
    };
    assert!(at(14, 4) < 96, "the dark side darkens: {}", at(14, 4));
    assert!(at(18, 4) > 160, "the bright side brightens: {}", at(18, 4));
    assert_eq!(at(2, 4), 96, "the far interior is left alone");
}

/// Vibrance lifts a muted colour's chroma but leaves a saturated primary
/// alone, and keeps the luma in place.
#[test]
#[ignore = "needs a wgpu adapter; run explicitly"]
fn the_vibrance_arm_lifts_muted_colour_only() {
    let gpu = Gpu::new();
    let size = [2, 1];
    let source = gpu.texture(
        size,
        wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC,
    );
    // BGRA: one muted blue-grey and one saturated red.
    gpu.upload(&source, &[170, 180, 200, 255, 0, 0, 255, 255], size);

    let target = gpu.texture(
        size,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let params = PostParams::identity()
        .with_quality(None, None, Some(0.5))
        .with_viewport([0.0, 0.0, size[0] as f32, size[1] as f32]);
    let mut chain = PostChain::new(&gpu.device, &gpu.queue, FORMAT, params);
    let source_view = source.create_view(&wgpu::TextureViewDescriptor::default());
    let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let mask = gpu.mask(size, 0);
    let mask_view = mask.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Post process vibrance"),
        });
    chain.draw(
        &gpu.device,
        &gpu.queue,
        &mut encoder,
        &source_view,
        &mask_view,
        &target_view,
        (size[0], size[1]),
    );
    gpu.queue.submit([encoder.finish()]);

    let out = gpu.read(&target, size);
    let muted = [out[0], out[1], out[2]];
    let saturated = [out[4], out[5], out[6]];
    let spread = |c: [u8; 3]| {
        c.iter().max().unwrap().abs_diff(*c.iter().min().unwrap())
    };
    assert!(spread(muted) > 30, "the muted texel gains chroma: {muted:?}");
    assert!(spread(saturated) >= 250, "saturated red stays saturated: {saturated:?}");
    assert_eq!(out[3], 255, "alpha passes through");
}

/// SMAA softens a 45 degree staircase: the boundary texels leave their source
/// bytes while both flat interiors stay byte copies, which is what separates it
/// from a whole-frame blur.
#[test]
#[ignore = "needs a wgpu adapter; run explicitly"]
fn the_smaa_pass_softens_a_staircase_edge() {
    let gpu = Gpu::new();
    let size = [32, 32];
    let mut source_bytes = Vec::new();
    for y in 0..size[1] {
        for x in 0..size[0] {
            let value = if x + y < 32 { 255 } else { 0 };
            source_bytes.extend_from_slice(&[value, value, value, 255]);
        }
    }
    let source = gpu.texture(
        size,
        wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC,
    );
    gpu.upload(&source, &source_bytes, size);

    let target = gpu.texture(
        size,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let params = PostParams::identity()
        .with_smaa(Some([0.1, 8.0]))
        .with_viewport([0.0, 0.0, size[0] as f32, size[1] as f32]);
    let mut chain = PostChain::new(&gpu.device, &gpu.queue, FORMAT, params);
    let source_view = source.create_view(&wgpu::TextureViewDescriptor::default());
    let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let mask = gpu.mask(size, 0);
    let mask_view = mask.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Post process smaa"),
        });
    chain.draw(
        &gpu.device,
        &gpu.queue,
        &mut encoder,
        &source_view,
        &mask_view,
        &target_view,
        (size[0], size[1]),
    );
    gpu.queue.submit([encoder.finish()]);

    let out = gpu.read(&target, size);
    let stride = size[0] as usize * 4;
    let at = |x: usize, y: usize| {
        let start = y * stride + x * 4;
        [out[start], out[start + 1], out[start + 2], out[start + 3]]
    };
    assert_eq!(at(0, 0), [255, 255, 255, 255], "white interior is untouched");
    assert_eq!(at(31, 31), [0, 0, 0, 255], "black interior is untouched");
    let softened = (0..size[1] as usize).any(|y| {
        (0..size[0] as usize).any(|x| {
            let texel = at(x, y);
            texel[0] > 0 && texel[0] < 255
        })
    });
    assert!(softened, "the staircase boundary must gain intermediate texels");
}

/// The AO arm reads the bound battlefield depth: a texel behind a nearer
/// neighbour darkens, one that fronts the step does not, and the frame's
/// unrelated texels stay byte copies.
#[test]
#[ignore = "needs a wgpu adapter; run explicitly"]
fn the_depth_ao_arm_darkens_behind_a_nearer_step() {
    let gpu = Gpu::new();
    let size = [32, 8];
    let mut source_bytes = Vec::new();
    for _ in 0..size[1] {
        for _ in 0..size[0] {
            source_bytes.extend_from_slice(&[128, 128, 128, 255]);
        }
    }
    let source = gpu.texture(
        size,
        wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC,
    );
    gpu.upload(&source, &source_bytes, size);

    // Native Z: far on the left (0.6), nearer on the right (0.3). wgpu rejects
    // buffer copies into depth formats, so a tiny pass writes the step.
    let depth = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Post process test depth"),
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let depth_shader = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Post process test depth step"),
            source: wgpu::ShaderSource::Wgsl(
                r#"
@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> @builtin(position) vec4f {
    let x = f32((idx << 1u) & 2u);
    let y = f32(idx & 2u);
    return vec4f(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) position: vec4f) -> @builtin(frag_depth) f32 {
    return select(0.6, 0.3, position.x >= 16.0);
}
"#
                .into(),
            ),
        });
    let depth_pipeline = gpu
        .device
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Post process test depth step"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &depth_shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &depth_shader,
                entry_point: Some("fs_main"),
                targets: &[],
                compilation_options: Default::default(),
            }),
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
    let depth_view = depth.create_view(&wgpu::TextureViewDescriptor::default());
    {
        let mut fill = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Post process test depth fill"),
            });
        {
            let mut pass = fill.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Post process test depth fill"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&depth_pipeline);
            pass.draw(0..3, 0..1);
        }
        gpu.queue.submit([fill.finish()]);
    }

    let target = gpu.texture(
        size,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let params = PostParams::from_effects(
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        crate::render::post_process::DepthEffects {
            scale: [1.0, 1.0],
            depth_of_field: None,
            ssao: Some([6.0, 0.5, 0.02]),
        },
        None,
        false,
        [0.0, 0.0, size[0] as f32, size[1] as f32],
    )
    .expect("ao configures the chain");
    let mut chain = PostChain::new(&gpu.device, &gpu.queue, FORMAT, params);
    let source_view = source.create_view(&wgpu::TextureViewDescriptor::default());
    let depth_view = depth.create_view(&wgpu::TextureViewDescriptor::default());
    let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let mask = gpu.mask(size, 0);
    let mask_view = mask.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Post process depth ao"),
        });
    chain.draw_with_depth(
        &gpu.device,
        &gpu.queue,
        &mut encoder,
        &source_view,
        &mask_view,
        Some(&depth_view),
        &target_view,
        (size[0], size[1]),
    );
    gpu.queue.submit([encoder.finish()]);

    let out = gpu.read(&target, size);
    let stride = size[0] as usize * 4;
    let at = |x: usize, y: usize| {
        let start = y * stride + x * 4;
        out[start]
    };
    assert!(at(14, 4) < 128, "behind the step darkens: {}", at(14, 4));
    assert_eq!(at(18, 4), 128, "the near side stays unshaded");
    assert_eq!(at(28, 4), 128, "far from the step stays untouched");
}

/// The mask rasterizer covers exactly the diamond it is given, in frame
/// pixels: the shape the displacement later trusts for alignment.
#[test]
#[ignore = "needs a wgpu adapter; run explicitly"]
fn the_water_mask_covers_the_rasterized_diamond() {
    let gpu = Gpu::new();
    let size = [16, 16];
    let mut mask = WaterMask::new(&gpu.device, &gpu.queue, (size[0], size[1]));
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Water mask raster"),
        });
    // One iso diamond from x 0..8, y 0..8: centre (4,4), vertices on the axes.
    mask.draw(&gpu.queue, &mut encoder, &[[0.0, 0.0, 8.0, 8.0]]);
    gpu.queue.submit([encoder.finish()]);

    let bytes = gpu.read_r8(mask.test_texture(), size);
    let at = |x: usize, y: usize| bytes[y * size[0] as usize + x];
    assert_eq!(at(4, 4), 255, "the diamond centre is water");
    assert_eq!(at(4, 1), 255, "just below the top vertex is water");
    assert_eq!(at(0, 0), 0, "the diamond bounding-box corner is dry");
    assert_eq!(at(12, 12), 0, "outside the diamond stays dry");
}

/// The debug tint covers exactly the water mask, which is how the engine's
/// mask alignment is checked against the rendered water.
#[test]
#[ignore = "needs a wgpu adapter; run explicitly"]
fn the_mask_debug_tint_covers_water() {
    let gpu = Gpu::new();
    let size = [8, 4];
    let source = gpu.texture(
        size,
        wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC,
    );
    let source_bytes = pattern(size);
    gpu.upload(&source, &source_bytes, size);

    let target = gpu.texture(
        size,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let mut chain = PostChain::new(
        &gpu.device,
        &gpu.queue,
        FORMAT,
        super::PostParams::mask_debug([0.0, 0.0, 8.0, 4.0]),
    );
    let mask = gpu.mask(size, 4);
    let mask_view = mask.create_view(&wgpu::TextureViewDescriptor::default());
    let source_view = source.create_view(&wgpu::TextureViewDescriptor::default());
    let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Mask debug tint"),
        });
    chain.draw(
        &gpu.device,
        &gpu.queue,
        &mut encoder,
        &source_view,
        &mask_view,
        &target_view,
        (size[0], size[1]),
    );
    gpu.queue.submit([encoder.finish()]);

    let out = gpu.read(&target, size);
    let stride = size[0] as usize * 4;
    for y in 0..size[1] as usize {
        for x in 0..4 {
            let start = y * stride + x * 4;
            assert_eq!(
                &out[start..start + 4],
                &[255, 0, 255, 255],
                "water texel ({x}, {y}) must be magenta"
            );
        }
        for x in 4..size[0] as usize {
            let start = y * stride + x * 4;
            assert_eq!(
                &out[start..start + 4],
                &source_bytes[start..start + 4],
                "land texel ({x}, {y}) must stay a byte copy"
            );
        }
    }
}

/// The distortion generator writes the scrolling field inside water and the
/// neutral 0.5 outside it, so the post pass can sample it unconditionally.
#[test]
#[ignore = "needs a wgpu adapter; run explicitly"]
fn the_distortion_field_is_neutral_outside_water() {
    let gpu = Gpu::new();
    let size = [16, 8];
    let mask = gpu.mask(size, 8);
    let mask_view = mask.create_view(&wgpu::TextureViewDescriptor::default());
    let mut distortion = WaterDistortion::new(&gpu.device, &gpu.queue, (size[0], size[1]));
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Water distortion field"),
        });
    distortion.generate(
        &gpu.device,
        &gpu.queue,
        &mut encoder,
        &mask_view,
        DistortionParams::new(8.0, 0.5, 1.0),
    );
    gpu.queue.submit([encoder.finish()]);

    let bytes = gpu.read_rg16f(distortion.test_texture(), size);
    let neutral = [0x00u8, 0x38, 0x00, 0x38];
    for y in 0..size[1] as usize {
        for x in 8..size[0] as usize {
            let start = y * size[0] as usize * 4 + x * 4;
            assert_eq!(
                &bytes[start..start + 4],
                &neutral,
                "dry texel ({x}, {y}) must be neutral"
            );
        }
    }
    let moved = (0..size[1] as usize).any(|y| {
        (0..8).any(|x| {
            let start = y * size[0] as usize * 4 + x * 4;
            bytes[start..start + 4] != neutral
        })
    });
    assert!(moved, "water texels must carry a non-neutral offset field");
}

/// The field scrolls with time: two different times must produce different
/// water offsets, which is what makes the ripple move instead of freeze.
#[test]
#[ignore = "needs a wgpu adapter; run explicitly"]
fn the_distortion_field_scrolls_with_time() {
    let gpu = Gpu::new();
    let size = [32, 16];
    let mask = gpu.mask(size, 32);
    let mask_view = mask.create_view(&wgpu::TextureViewDescriptor::default());
    let mut distortion = WaterDistortion::new(&gpu.device, &gpu.queue, (size[0], size[1]));
    let mut readings = Vec::new();
    for time in [1.0_f32, 2.0] {
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Water distortion scroll"),
            });
        distortion.generate(
            &gpu.device,
            &gpu.queue,
            &mut encoder,
            &mask_view,
            DistortionParams::new(16.0, 0.8, time),
        );
        gpu.queue.submit([encoder.finish()]);
        readings.push(gpu.read_rg16f(distortion.test_texture(), size));
    }
    assert_ne!(
        readings[0], readings[1],
        "the water field must scroll with time"
    );
}
