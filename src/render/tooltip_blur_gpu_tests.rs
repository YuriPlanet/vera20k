//! Explicit GPU regression: the tooltip blur pass converges a step-edge rect
//! to its mean under a large `ToolTip.Background.BlurSize`, the way native's
//! three-box gaussian (`Ext/Surface/Body.cpp:9`) flattens the background
//! fill over what it covers. Requires a wgpu adapter.

use super::gpu::block_on;
use super::tooltip_blur::TooltipBlurRenderer;
use wgpu::util::DeviceExt;

/// The composition is a hard black/white step; a blur far larger than the
/// rect flattens every pixel to the rect's mean (`Ext/Surface/Body.cpp:9`'s
/// three-box gaussian over the whole region).
#[test]
#[ignore = "requires a wgpu adapter; run explicitly"]
fn tooltip_blur_flattens_the_rect_to_its_mean() {
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
        label: Some("Tooltip blur regression device"),
        ..Default::default()
    }))
    .expect("request offscreen test device");

    let size = [32u32, 16u32];
    // Left half black, right half white: the mean is 0.5 grey.
    let mut pixels = vec![0u8; (size[0] * size[1] * 4) as usize];
    for pixel in pixels.chunks_mut(4) {
        pixel.copy_from_slice(&[0, 0, 0, 255]);
    }
    for row in 0..size[1] {
        for column in size[0] / 2..size[0] {
            let index = ((row * size[0] + column) * 4) as usize;
            pixels[index..index + 4].copy_from_slice(&[255, 255, 255, 255]);
        }
    }
    let composition = device.create_texture_with_data(
        &queue,
        &wgpu::TextureDescriptor {
            label: Some("Tooltip blur test composition"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &pixels,
    );
    let composition_view = composition.create_view(&wgpu::TextureViewDescriptor::default());

    let mut renderer = TooltipBlurRenderer::new(&device, &composition);
    let mut encoder = device.create_command_encoder(&Default::default());
    // A rect covering the whole composition; the blur size dwarfs the extent.
    renderer.draw(
        &device,
        &queue,
        &mut encoder,
        &composition_view,
        [size[0] as f32, size[1] as f32],
        [0.0, 0.0, size[0] as f32, size[1] as f32],
        64.0,
    );

    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Tooltip blur test readback"),
        size: (size[0] * size[1] * 4) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &composition,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size[0] * 4),
                rows_per_image: None,
            },
        },
        wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);
    let slice = readback.slice(..);
    let (sender, receiver) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        sender.send(result).expect("readback receiver");
    });
    device.poll(wgpu::PollType::wait_indefinitely());
    receiver
        .recv()
        .expect("readback completion")
        .expect("readback map");
    let mapping = slice
        .get_mapped_range()
        .expect("readback range");
    for (index, pixel) in mapping.chunks_exact(4).enumerate() {
        let grey = (f32::from(pixel[0]) + f32::from(pixel[1]) + f32::from(pixel[2])) / 3.0;
        assert!(
            (grey - 127.5).abs() <= 6.0,
            "pixel {index} blurred to {grey}, expected the ~127.5 mean"
        );
    }
}
