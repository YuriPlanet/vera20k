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
use crate::render::post_process::PostChain;

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
        chain.draw(
            &gpu.device,
            &gpu.queue,
            &mut encoder,
            &source_view,
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
    chain.draw(
        &gpu.device,
        &gpu.queue,
        &mut encoder,
        &source_view,
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
    chain.draw(
        &gpu.device,
        &gpu.queue,
        &mut encoder,
        &source_view,
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
    chain.draw(
        &gpu.device,
        &gpu.queue,
        &mut encoder,
        &source_view,
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
    chain.draw(
        &gpu.device,
        &gpu.queue,
        &mut encoder,
        &source_view,
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
    chain.draw(
        &gpu.device,
        &gpu.queue,
        &mut encoder,
        &source_view,
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
    chain.draw(
        &gpu.device,
        &gpu.queue,
        &mut encoder,
        &source_view,
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
