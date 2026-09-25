//! Headless checks of the production Batch upload, TerrainDrawRenderer snapshot,
//! piece edit, and shared camera/instance ABI. Synthetic native leaf goldens are
//! produced by tools/terrain_draw_oracle/leaf.py, not by the Rust implementation.

use super::{
    batch::{BatchRenderer, CameraUniform, SpriteInstance},
    terrain_draw::{TerrainDrawRenderer, TerrainPiece},
};
use std::time::Duration;
use wgpu::util::DeviceExt;

pub(crate) struct Gpu {
    pub(crate) device: wgpu::Device,
    pub(crate) queue: wgpu::Queue,
}
impl Gpu {
    pub(crate) fn new() -> Self {
        Self::with_features(wgpu::Features::empty())
    }
    fn with_features(features: wgpu::Features) -> Self {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..Default::default()
        });
        let adapter = pollster::block_on(instance.request_adapter(&Default::default()))
            .expect("explicit GPU check needs an adapter");
        eprintln!("Terrain production owner GPU: {:?}", adapter.get_info());
        assert!(
            adapter.features().contains(features),
            "required explicit GPU probe features unavailable"
        );
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: features,
            ..Default::default()
        }))
        .unwrap();
        Self { device, queue }
    }
    pub(crate) fn target(&self, size: [u32; 2], format: wgpu::TextureFormat) -> wgpu::Texture {
        self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Terrain production owner test attachment"),
            size: extent(size),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: if format == wgpu::TextureFormat::Bgra8UnormSrgb {
                &[wgpu::TextureFormat::Bgra8Unorm]
            } else if format == wgpu::TextureFormat::Rgba8UnormSrgb {
                &[wgpu::TextureFormat::Rgba8Unorm]
            } else {
                &[]
            },
        })
    }
    /// Read back one rectangle of an unsigned-integer texture (e.g. palette
    /// indices) through `textureLoad`; atlas textures have no COPY_SRC.
    pub(crate) fn read_uint_texels(
        &self,
        view: &wgpu::TextureView,
        origin: [u32; 2],
        size: [u32; 2],
    ) -> Vec<u8> {
        let ([x, y], [width, height]) = (origin, size);
        let source = format!(
            "@group(0) @binding(0) var t:texture_2d<u32>; @group(0) @binding(1) var<storage,read_write> output:array<u32>; @compute @workgroup_size(8,8) fn main(@builtin(global_invocation_id) p:vec3<u32>) {{ if p.x<{width}u && p.y<{height}u {{ output[p.y*{width}u+p.x]=textureLoad(t,vec2<i32>(i32(p.x+{x}u),i32(p.y+{y}u)),0).r; }} }}"
        );
        let shader = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("uint texel readback"),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
        let pipeline = self
            .device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: None,
                layout: None,
                module: &shader,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                cache: None,
            });
        let out = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(width * height * 4),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let read = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: out.size(),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: out.as_entire_binding(),
                },
            ],
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(width.div_ceil(8), height.div_ceil(8), 1);
        }
        encoder.copy_buffer_to_buffer(&out, 0, &read, 0, out.size());
        self.queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        read.slice(..)
            .map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        rx.recv().unwrap().unwrap();
        let data = read.slice(..).get_mapped_range();
        data.chunks_exact(4)
            .map(|texel| u32::from_le_bytes(texel.try_into().unwrap()) as u8)
            .collect()
    }

    pub(crate) fn read(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        texture: &wgpu::Texture,
    ) -> (wgpu::Buffer, u32) {
        let pitch = (texture.width() * 4).div_ceil(256) * 256;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Terrain exact attachment readback"),
            size: u64::from(pitch * texture.height()),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: if texture.format().is_depth_stencil_format() {
                    wgpu::TextureAspect::DepthOnly
                } else {
                    wgpu::TextureAspect::All
                },
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(pitch),
                    rows_per_image: Some(texture.height()),
                },
            },
            texture.size(),
        );
        (buffer, pitch)
    }
    pub(crate) fn finish(
        &self,
        encoder: wgpu::CommandEncoder,
        reads: &[(wgpu::Buffer, u32)],
        size: [u32; 2],
    ) -> Vec<Vec<u8>> {
        let submission = self.queue.submit([encoder.finish()]);
        let receivers = reads
            .iter()
            .map(|(buffer, _)| {
                let (tx, rx) = std::sync::mpsc::channel();
                buffer
                    .slice(..)
                    .map_async(wgpu::MapMode::Read, move |result| {
                        tx.send(result).unwrap();
                    });
                rx
            })
            .collect::<Vec<_>>();
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(Duration::from_secs(60)),
            })
            .unwrap();
        receivers
            .into_iter()
            .for_each(|rx| rx.recv().unwrap().unwrap());
        reads
            .iter()
            .map(|(buffer, pitch)| {
                let data = buffer.slice(..).get_mapped_range();
                let bytes = data
                    .chunks_exact(*pitch as usize)
                    .flat_map(|row| row[..size[0] as usize * 4].iter().copied())
                    .collect();
                drop(data);
                buffer.unmap();
                bytes
            })
            .collect()
    }
}
pub(crate) fn extent(size: [u32; 2]) -> wgpu::Extent3d {
    wgpu::Extent3d {
        width: size[0],
        height: size[1],
        depth_or_array_layers: 1,
    }
}
pub(crate) fn camera(size: [u32; 2]) -> CameraUniform {
    CameraUniform {
        screen_size: size.map(|v| v as f32),
        camera_pos: [0.0; 2],
        zoom: 1.0,
        world_origin_y: -100.0,
        world_height: 1000.0,
        _pad: 0.0,
        native_z_origin_y: 0.0,
        _native_z_pad: 0.0,
    }
}
pub(crate) fn encoded(word: u16, format: wgpu::TextureFormat) -> [u8; 4] {
    let p = super::native_surface_format::ACTIVE_RETAIL_RGB565_PRESENTATION;
    let mut rgba = [
        p.five_bit[(word >> 11) as usize],
        p.six_bit[((word >> 5) & 63) as usize],
        p.five_bit[(word & 31) as usize],
        255,
    ];
    if matches!(
        format,
        wgpu::TextureFormat::Bgra8UnormSrgb | wgpu::TextureFormat::Bgra8Unorm
    ) {
        rgba.swap(0, 2);
    }
    rgba
}
pub(crate) fn clear(
    encoder: &mut wgpu::CommandEncoder,
    color: &wgpu::TextureView,
    depth: &wgpu::TextureView,
    old: u16,
    color_load: wgpu::LoadOp<wgpu::Color>,
) {
    let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Prepare original leaf destinations"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: color,
            resolve_target: None,
            depth_slice: None,
            ops: wgpu::Operations {
                load: color_load,
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: depth,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(super::native_z::stored_depth(old)),
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
    });
}
pub(crate) fn sprite(position: [f32; 2], size: [f32; 2], z_adjust: f32) -> SpriteInstance {
    SpriteInstance {
        position,
        size,
        uv_size: [1.0; 2],
        tint: [1.0; 3],
        alpha: 1.0,
        z_adjust,
        palette_light: super::palette_light::PaletteLight::plain(1, 1000),
        ..Default::default()
    }
}

#[test]
#[ignore = "requires a GPU; executes actual production snapshot/edit/upload"]
fn production_shadow_exhausts_all_65536_destination_words() {
    let gpu = Gpu::new();
    let goldens = include_bytes!("../../tools/terrain_draw_oracle/fixtures/half-rgb565.bin");
    for format in [
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ] {
        let size = [256; 2];
        let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
        let mut terrain = TerrainDrawRenderer::new(&gpu.device, format, &batch);
        batch.write_camera(&gpu.queue, camera(size));
        let color = gpu.target(size, format);
        let cv = color.create_view(&Default::default());
        let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
        let dv = depth.create_view(&Default::default());
        let bytes = (0..=u16::MAX)
            .flat_map(|v| encoded(v, format))
            .collect::<Vec<_>>();
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &color,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(1024),
                rows_per_image: Some(256),
            },
            extent(size),
        );
        terrain.prepare(&gpu.device, &color, &dv, batch.camera_uniform());
        let atlas =
            batch.create_texture_on_device(&gpu.device, &gpu.queue, &[255; 4], 1, 1, Some(&[1]));
        let instance = sprite([0.0; 2], [256.0; 2], -1.0);
        let buffer = gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Actual tree ABI"),
                contents: bytemuck::bytes_of(&instance),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        clear(&mut encoder, &cv, &dv, 32768, wgpu::LoadOp::Load);
        terrain.draw_piece(
            &mut encoder,
            &cv,
            &dv,
            &batch,
            &atlas,
            &buffer,
            0,
            &instance,
            TerrainPiece::Shadow,
            [0, 0, 256, 256],
        );
        let reads = [
            gpu.read(&mut encoder, &color),
            gpu.read(&mut encoder, &depth),
        ];
        let output = gpu.finish(encoder, &reads, size);
        for i in 0..65536 {
            let native = u16::from_le_bytes([goldens[i * 2], goldens[i * 2 + 1]]);
            assert_eq!(
                output[0][i * 4..i * 4 + 4],
                encoded(native, format),
                "{format:?} word {i:04x}"
            );
            let z = f32::from_le_bytes(output[1][i * 4..i * 4 + 4].try_into().unwrap());
            assert_eq!(super::native_z::stored_z(z), 32767 - (i / 256) as u16);
        }
    }
}

#[test]
#[ignore = "requires a GPU; original signed/store/repeat leaf cases"]
fn production_tree_signed_depth_and_repeated_overlap_match_original_leaves() {
    let gpu = Gpu::new();
    let format = wgpu::TextureFormat::Bgra8UnormSrgb;
    let size = [32, 4];
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../tools/terrain_draw_oracle/fixtures/leaf.json"
    ))
    .unwrap();
    let cases = fixture["depth_cases"].as_array().unwrap();
    let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
    let mut terrain = TerrainDrawRenderer::new(&gpu.device, format, &batch);
    batch.write_camera(&gpu.queue, camera(size));
    let color = gpu.target(size, format);
    let cv = color.create_view(&Default::default());
    let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
    let dv = depth.create_view(&Default::default());
    terrain.prepare(&gpu.device, &color, &dv, batch.camera_uniform());
    // The prepared original Convert fixture maps source index 1 to 0x55aa.
    let atlas = batch.create_texture_on_device(
        &gpu.device,
        &gpu.queue,
        &[80, 180, 80, 255],
        1,
        1,
        Some(&[1]),
    );
    for old in [0u16, 1, 32767, 32768, 65535] {
        let selected = cases
            .iter()
            .filter(|c| c[3].as_u64() == Some(u64::from(old)))
            .collect::<Vec<_>>();
        let instances = selected
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let y = (i / 32) as i32;
                let candidate = c[1].as_i64().unwrap() as i32 - c[2].as_i64().unwrap() as i32;
                sprite(
                    [(i % 32) as f32, y as f32],
                    [1.0; 2],
                    (candidate - (32768 - y)) as f32,
                )
            })
            .collect::<Vec<_>>();
        let buffer = gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Native signed candidates"),
                contents: bytemuck::cast_slice(&instances),
                usage: wgpu::BufferUsages::VERTEX,
            });
        for repeated in [false, true] {
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            if !repeated {
                clear(
                    &mut encoder,
                    &cv,
                    &dv,
                    old,
                    wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                );
            }
            terrain.draw_span(
                &mut encoder,
                &cv,
                &dv,
                &batch,
                &atlas,
                &buffer,
                &instances,
                selected.iter().enumerate().map(|(i, c)| {
                    (
                        i as u32,
                        if c[0].as_i64() == Some(1) {
                            TerrainPiece::Shadow
                        } else {
                            TerrainPiece::Body
                        },
                    )
                }),
                [0, 0, size[0], size[1]],
            );
            let reads = [
                gpu.read(&mut encoder, &color),
                gpu.read(&mut encoder, &depth),
            ];
            let output = gpu.finish(encoder, &reads, size);
            for (i, c) in selected.iter().enumerate() {
                let word = c[if repeated { 6 } else { 4 }].as_u64().unwrap() as u16;
                assert_eq!(
                    output[0][i * 4..i * 4 + 4],
                    encoded(word, format),
                    "old={old}, repeat={repeated}, native={c}"
                );
                let z = f32::from_le_bytes(output[1][i * 4..i * 4 + 4].try_into().unwrap());
                assert_eq!(
                    super::native_z::stored_z(z),
                    c[5].as_u64().unwrap() as u16,
                    "native={c}"
                );
            }
        }
    }
    eprintln!(
        "550 original signed-before-store and repeated-overlap cases matched production GPU owner"
    );
}

#[test]
#[ignore = "requires a GPU; original clipped row fixture through the production owner"]
fn production_tree_clipping_and_viewport_origin_match_original_rows() {
    let gpu = Gpu::new();
    let format = wgpu::TextureFormat::Bgra8UnormSrgb;
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../tools/terrain_draw_oracle/fixtures/rows.json"
    ))
    .unwrap();
    let cases = fixture["cases"].as_array().unwrap();
    let size = [cases.len() as u32, 550];
    let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
    let mut terrain = TerrainDrawRenderer::new(&gpu.device, format, &batch);
    let atlas = batch.create_texture_on_device(
        &gpu.device,
        &gpu.queue,
        &[80, 180, 80, 255],
        1,
        1,
        Some(&[1]),
    );
    let color = gpu.target(size, format);
    let cv = color.create_view(&Default::default());
    let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
    let dv = depth.create_view(&Default::default());
    for origin in [0i32, 37] {
        let cam = CameraUniform {
            native_z_origin_y: origin as f32,
            ..camera(size)
        };
        batch.write_camera(&gpu.queue, cam);
        terrain.prepare(&gpu.device, &color, &dv, batch.camera_uniform());
        let instances = cases
            .iter()
            .enumerate()
            .map(|(i, c)| SpriteInstance {
                z_gradient: c[0].as_u64().unwrap() as u32,
                ..sprite(
                    [i as f32, (c[1].as_i64().unwrap() as i32 + origin) as f32],
                    [1.0, c[2].as_u64().unwrap() as f32],
                    c[3].as_i64().unwrap() as f32,
                )
            })
            .collect::<Vec<_>>();
        let buffer = gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Original clipped row instances"),
                contents: bytemuck::cast_slice(&instances),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        clear(
            &mut encoder,
            &cv,
            &dv,
            super::native_z::EMPTY_Z,
            wgpu::LoadOp::Clear(wgpu::Color::WHITE),
        );
        for (i, c) in cases.iter().enumerate() {
            let first = (c[1].as_i64().unwrap() + c[4].as_i64().unwrap() + i64::from(origin))
                .max(i64::from(origin)) as u32;
            terrain.draw_piece(
                &mut encoder,
                &cv,
                &dv,
                &batch,
                &atlas,
                &buffer,
                i as u32,
                &instances[i],
                TerrainPiece::Body,
                [i as u32, first, 1, size[1] - first],
            );
        }
        let reads = [
            gpu.read(&mut encoder, &color),
            gpu.read(&mut encoder, &depth),
        ];
        let output = gpu.finish(encoder, &reads, size);
        for (i, c) in cases.iter().enumerate() {
            let top = c[1].as_i64().unwrap() + i64::from(origin);
            let skip = c[4].as_i64().unwrap();
            let rows = c[6].as_array().unwrap();
            for y in 0..size[1] as i64 {
                let row = y - top - skip;
                let expected = if y >= i64::from(origin) && row >= 0 && (row as usize) < rows.len()
                {
                    rows[row as usize].as_u64().unwrap() as u16
                } else {
                    super::native_z::EMPTY_Z
                };
                let pixel = y as usize * size[0] as usize + i;
                let z = f32::from_le_bytes(output[1][pixel * 4..pixel * 4 + 4].try_into().unwrap());
                assert_eq!(
                    super::native_z::stored_z(z),
                    expected,
                    "origin={origin}, y={y}, native={c}"
                );
                let expected_color =
                    if y >= i64::from(origin) && row >= 0 && (row as usize) < rows.len() {
                        0x55aa
                    } else {
                        0xffff
                    };
                assert_eq!(
                    output[0][pixel * 4..pixel * 4 + 4],
                    encoded(expected_color, format)
                );
            }
        }
    }
}

#[test]
#[ignore = "requires a GPU; native empty clear, zero stencil, resize and depth-view replacement"]
fn production_tree_empty_depth_holes_and_target_rebinding_are_preserved() {
    let gpu = Gpu::new();
    let format = wgpu::TextureFormat::Bgra8UnormSrgb;
    let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
    let mut terrain = TerrainDrawRenderer::new(&gpu.device, format, &batch);
    // Nonzero black source1 must write; source0 must not write either target.
    let atlas = batch.create_texture_on_device(
        &gpu.device,
        &gpu.queue,
        &[0, 0, 0, 255, 0, 0, 0, 255],
        2,
        1,
        Some(&[1, 0]),
    );
    for size in [[16, 4], [24, 8], [24, 8], [16, 4]] {
        let cam = camera(size);
        batch.write_camera(&gpu.queue, cam);
        let color = gpu.target(size, format);
        let cv = color.create_view(&Default::default());
        let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
        let dv = depth.create_view(&Default::default());
        terrain.prepare(&gpu.device, &color, &dv, batch.camera_uniform());
        let candidates = [32768i32, 32769, 65533, 65534, 65535, 65536];
        let instances = candidates
            .iter()
            .enumerate()
            .map(|(i, &z)| sprite([i as f32 * 2.0, 0.0], [2.0, 1.0], (z - 32768) as f32))
            .collect::<Vec<_>>();
        let buffer = gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Native clear boundary ABI"),
                contents: bytemuck::cast_slice(&instances),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        assert_eq!(
            super::native_z::stored_z(super::native_z::STORED_DEPTH_CLEAR),
            65535
        );
        clear(
            &mut encoder,
            &cv,
            &dv,
            super::native_z::EMPTY_Z,
            wgpu::LoadOp::Clear(wgpu::Color::WHITE),
        );
        for (i, instance) in instances.iter().enumerate() {
            terrain.draw_piece(
                &mut encoder,
                &cv,
                &dv,
                &batch,
                &atlas,
                &buffer,
                i as u32,
                instance,
                TerrainPiece::Body,
                [0, 0, size[0], size[1]],
            );
        }
        let reads = [
            gpu.read(&mut encoder, &color),
            gpu.read(&mut encoder, &depth),
        ];
        let output = gpu.finish(encoder, &reads, size);
        for (i, &z) in candidates.iter().enumerate() {
            for hole in [false, true] {
                let pixel = i * 2 + usize::from(hole);
                let admitted = !hole && z < 65535;
                assert_eq!(
                    output[0][pixel * 4..pixel * 4 + 4],
                    encoded(if admitted { 0 } else { 0xffff }, format)
                );
                let depth =
                    f32::from_le_bytes(output[1][pixel * 4..pixel * 4 + 4].try_into().unwrap());
                assert_eq!(
                    super::native_z::stored_z(depth),
                    if admitted { z as u16 } else { 65535 }
                );
            }
        }
    }
}

#[test]
#[ignore = "requires GPU; actual camera writes, zoom raster and per-piece scissor"]
fn production_tree_camera_zoom_matches_unscissored_shp_coverage() {
    let gpu = Gpu::new();
    let format = wgpu::TextureFormat::Bgra8UnormSrgb;
    let size = [48, 48];
    let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
    let mut terrain = TerrainDrawRenderer::new(&gpu.device, format, &batch);
    let atlas = batch.create_texture_on_device(
        &gpu.device,
        &gpu.queue,
        &[80, 180, 80, 255],
        1,
        1,
        Some(&[1]),
    );
    let color = gpu.target(size, format);
    let cv = color.create_view(&Default::default());
    let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
    let dv = depth.create_view(&Default::default());
    for zoom in [0.75, 1.0, 1.25, 1.5, 2.0] {
        for raw_camera in [[-0.6f32, -0.4], [0.4, 0.6], [42.4, 100.6]] {
            batch.update_camera_on_queue(
                &gpu.queue,
                48.0,
                48.0,
                raw_camera[0],
                raw_camera[1],
                zoom,
                super::batch::DepthAxis {
                    origin_y: -100.0,
                    world_height: 1000.0,
                },
            );
            assert_eq!(
                batch.camera_uniform().native_z_origin_y,
                0.0,
                "frame camera must reset previous origin"
            );
            batch.update_native_z_origin(&gpu.queue, 7.0 / zoom);
            let cam = batch.camera_uniform();
            assert_eq!(cam.camera_pos, raw_camera.map(f32::round));
            assert_eq!(cam.native_z_origin_y, 7.0 / zoom);
            terrain.prepare(&gpu.device, &color, &dv, cam);
            let instance = SpriteInstance {
                z_gradient: 2,
                ..sprite(
                    [cam.camera_pos[0] + 2.25, cam.camera_pos[1] + 7.25 / zoom],
                    [11.5, 17.5],
                    -12.0,
                )
            };
            let buffer = gpu
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Fractional camera actual ABI"),
                    contents: bytemuck::bytes_of(&instance),
                    usage: wgpu::BufferUsages::VERTEX,
                });
            let mut outputs = Vec::new();
            for tree in [false, true] {
                let mut encoder = gpu.device.create_command_encoder(&Default::default());
                clear(
                    &mut encoder,
                    &cv,
                    &dv,
                    65535,
                    wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                );
                if tree {
                    terrain.draw_piece(
                        &mut encoder,
                        &cv,
                        &dv,
                        &batch,
                        &atlas,
                        &buffer,
                        0,
                        &instance,
                        TerrainPiece::Body,
                        [0, 7, 48, 41],
                    );
                } else {
                    // Full tactical scissor is deliberately larger than the tree
                    // bounds, exposing any crop introduced by piece_scissor.
                    let mut pass = crate::app::presentation::sidebar_render::begin_main_load_pass(
                        &mut encoder,
                        &cv,
                        &dv,
                    );
                    pass.set_scissor_rect(0, 7, 48, 41);
                    batch.draw_zsprite_range(
                        &mut pass,
                        &atlas,
                        batch.default_zshape_bind_group(),
                        &buffer,
                        0,
                        1,
                        true,
                    );
                }
                let reads = [
                    gpu.read(&mut encoder, &color),
                    gpu.read(&mut encoder, &depth),
                ];
                outputs.push(gpu.finish(encoder, &reads, size));
            }
            assert_eq!(outputs[0], outputs[1], "zoom{zoom}, camera{raw_camera:?}");
            assert!(
                outputs[1][0]
                    .chunks_exact(4)
                    .any(|p| p == encoded(0x55aa, format)),
                "empty comparison"
            );
        }
    }
}

#[test]
#[ignore = "requires GPU; actual Batch stamp/bracket/UI pipelines and production compatibility depth"]
fn production_batch_stamp_bracket_and_ui_keep_near_equal_and_map_edge_policies() {
    let gpu = Gpu::new();
    let format = wgpu::TextureFormat::Bgra8UnormSrgb;
    let size = [8, 8];
    let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
    batch.write_camera(&gpu.queue, camera(size));
    let atlas = batch.create_texture_on_device(
        &gpu.device,
        &gpu.queue,
        &[0, 0, 0, 255, 0, 0, 0, 0],
        2,
        1,
        Some(&[1, 0]),
    );
    let bracket = batch.create_texture_on_device(
        &gpu.device,
        &gpu.queue,
        &[248, 0, 0, 255],
        1,
        1,
        Some(&[1]),
    );
    let color = gpu.target(size, format);
    let cv = color.create_view(&Default::default());
    let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
    let dv = depth.create_view(&Default::default());
    // Actual legacy producer clamps outside the world extent. These tests
    // preserve ordering of the resulting scalar; they do not recover lost rows.
    for row in [-200.0, -99.0, 0.0, 899.0, 1200.0] {
        let base = crate::app::presentation::instances::compute_sprite_depth_params(
            -100.0, 1000.0, row, 0,
        );
        for delta in [0.0, -0.0001, 0.0001] {
            let stamp = SpriteInstance {
                depth: base,
                ..sprite([0.0; 2], [8.0; 2], 0.0)
            };
            let test = SpriteInstance {
                depth: base + delta,
                ..stamp
            };
            let stampbuf = gpu
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Stamp actual ABI"),
                    contents: bytemuck::bytes_of(&stamp),
                    usage: wgpu::BufferUsages::VERTEX,
                });
            let testbuf = gpu
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Bracket actual ABI"),
                    contents: bytemuck::bytes_of(&test),
                    usage: wgpu::BufferUsages::VERTEX,
                });
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            clear(
                &mut encoder,
                &cv,
                &dv,
                65535,
                wgpu::LoadOp::Clear(wgpu::Color::WHITE),
            );
            {
                let mut pass = crate::app::presentation::sidebar_render::begin_main_load_pass(
                    &mut encoder,
                    &cv,
                    &dv,
                );
                batch.draw_with_buffer_depth_stamp(&mut pass, &atlas, &stampbuf, 1);
            }
            let stamped = gpu.read(&mut encoder, &depth);
            {
                let mut pass = crate::app::presentation::sidebar_render::begin_main_load_pass(
                    &mut encoder,
                    &cv,
                    &dv,
                );
                batch.draw_with_buffer_depth_test(&mut pass, &bracket, &testbuf, 1);
            }
            let reads = [
                gpu.read(&mut encoder, &color),
                gpu.read(&mut encoder, &depth),
                stamped,
            ];
            let output = gpu.finish(encoder, &reads, size);
            assert_eq!(output[1], output[2], "bracket must not store Z");
            for y in 0..8usize {
                for x in 0..8usize {
                    let passes = x >= 4 || delta < 0.0;
                    assert_eq!(
                        output[0][(y * 8 + x) * 4..(y * 8 + x + 1) * 4],
                        encoded(if passes { 0xf800 } else { 0xffff }, format),
                        "row{row},delta{delta},x{x}"
                    );
                }
            }
            // The real screen-fixed UI overlay uses LessEqual+write, so an
            // equal candidate must overwrite. Its UI camera retains zoom1.
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            {
                let mut pass = crate::app::presentation::sidebar_render::begin_main_load_pass(
                    &mut encoder,
                    &cv,
                    &dv,
                );
                pass.set_pipeline(batch.overlay_pipeline());
                pass.set_bind_group(0, batch.ui_camera_bind_group(), &[]);
                pass.set_bind_group(1, &bracket.bind_group, &[]);
                pass.set_vertex_buffer(0, stampbuf.slice(..));
                pass.draw(0..6, 0..1);
            }
            let reads = [gpu.read(&mut encoder, &color)];
            let output = gpu.finish(encoder, &reads, size);
            assert!(
                output[0]
                    .chunks_exact(4)
                    .all(|p| p == encoded(0xf800, format)),
                "UI equality row{row}"
            );
        }
    }
}

#[test]
#[ignore = "requires GPU; multi-piece wave equivalence, clips, camera and target reuse"]
fn production_tree_dependency_waves_match_sequential_pixels_and_depth() {
    let gpu = Gpu::new();
    let mut cases = 0;
    for format in [
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ] {
        let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
        let mut terrain = TerrainDrawRenderer::new(&gpu.device, format, &batch);
        // Actual source-zero stencil and alternating nonzero palette entries.
        // Native leaf goldens cover each operation; these generated mixtures
        // compare the optimized schedule to that sequential production owner.
        let indices = [1, 0, 1, 2, 2, 1, 0, 1, 0, 2, 1, 1, 1, 1, 2, 0];
        let rgba: Vec<u8> = indices
            .iter()
            .flat_map(|&index| {
                if index == 2 {
                    [240, 96, 16, 255]
                } else {
                    [80, 180, 80, 255]
                }
            })
            .collect();
        let atlas =
            batch.create_texture_on_device(&gpu.device, &gpu.queue, &rgba, 4, 4, Some(&indices));
        let specs = [
            ([2., 2.], [24., 28.], TerrainPiece::Body, -33000.),
            ([66., 2.], [24., 28.], TerrainPiece::Body, -33000.),
            ([2., 2.], [24., 28.], TerrainPiece::Shadow, -33000.),
            ([66., 2.], [24., 28.], TerrainPiece::Shadow, -33000.),
            ([2., 2.], [24., 28.], TerrainPiece::Shadow, -33000.),
            ([22., 12.], [60., 18.], TerrainPiece::Shadow, -33001.),
            ([10., 66.], [32., 28.], TerrainPiece::Body, -32800.),
            ([10., 66.], [32., 28.], TerrainPiece::Shadow, -32800.),
            ([31.5, 32.], [32., 1.], TerrainPiece::Body, -1.),
            ([64., 32.], [32., 1.], TerrainPiece::Shadow, -2.),
            ([-40., -40.], [5., 5.], TerrainPiece::Shadow, -33000.),
            ([140., 88.], [30., 30.], TerrainPiece::Body, -32768.),
            ([0., 0.], [0., 0.], TerrainPiece::Shadow, -33000.),
        ];
        let instances = specs
            .iter()
            .map(|&(position, size, _, z)| sprite(position, size, z))
            .collect::<Vec<_>>();
        let buffer = gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Multi-wave exact production inputs"),
                contents: bytemuck::cast_slice(&instances),
                usage: wgpu::BufferUsages::VERTEX,
            });
        // Reuse the same scheduler through growth, shrink and source-handle
        // changes. Every case contains disjoint members and overlapping chains.
        for size in [[160, 96], [224, 144], [160, 96]] {
            for (camera_pos, zoom) in [
                ([0., 0.], 1.),
                ([0.25, 0.75], 1.),
                ([2.25, -1.25], 0.75),
                ([-3.5, 2.5], 1.25),
                ([0., 0.], 2.),
            ] {
                let mut camera = camera(size);
                camera.camera_pos = camera_pos;
                camera.zoom = zoom;
                camera.native_z_origin_y = 37.;
                batch.write_camera(&gpu.queue, camera);
                let mut outputs = Vec::new();
                for batched in [false, true] {
                    let color = gpu.target(size, format);
                    let cv = color.create_view(&Default::default());
                    let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
                    let dv = depth.create_view(&Default::default());
                    terrain.prepare(&gpu.device, &color, &dv, batch.camera_uniform());
                    let mut encoder = gpu.device.create_command_encoder(&Default::default());
                    clear(
                        &mut encoder,
                        &cv,
                        &dv,
                        65535,
                        wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                    );
                    let tactical = [1, 2, size[0] - 3, size[1] - 4];
                    if batched {
                        let stats = terrain.draw_span(
                            &mut encoder,
                            &cv,
                            &dv,
                            &batch,
                            &atlas,
                            &buffer,
                            &instances,
                            specs.iter().enumerate().map(|(i, spec)| (i as u32, spec.2)),
                            tactical,
                        );
                        assert!(
                            stats.waves > 1 && stats.waves < stats.pieces,
                            "actual multi-member waves: {stats:?}"
                        );
                    } else {
                        for (i, instance) in instances.iter().enumerate() {
                            terrain.draw_piece(
                                &mut encoder,
                                &cv,
                                &dv,
                                &batch,
                                &atlas,
                                &buffer,
                                i as u32,
                                instance,
                                specs[i].2,
                                tactical,
                            );
                        }
                    }
                    let reads = [
                        gpu.read(&mut encoder, &color),
                        gpu.read(&mut encoder, &depth),
                    ];
                    outputs.push(gpu.finish(encoder, &reads, size));
                }
                for attachment in 0..2 {
                    assert_eq!(outputs[0][attachment].len(), outputs[1][attachment].len());
                    let mismatches = outputs[0][attachment]
                        .chunks_exact(4)
                        .zip(outputs[1][attachment].chunks_exact(4))
                        .enumerate()
                        .filter(|(_, (a, b))| a != b)
                        .take(8)
                        .collect::<Vec<_>>();
                    assert!(
                        mismatches.is_empty(),
                        "{format:?}, {size:?}, {camera_pos:?}, zoom{zoom}, attachment{attachment}: {mismatches:?}"
                    );
                }
                cases += 1;
            }
        }
        let size = [64, 64];
        batch.write_camera(&gpu.queue, camera(size));
        let color = gpu.target(size, format);
        let cv = color.create_view(&Default::default());
        let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
        let dv = depth.create_view(&Default::default());
        terrain.prepare(&gpu.device, &color, &dv, batch.camera_uniform());
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        clear(
            &mut encoder,
            &cv,
            &dv,
            65535,
            wgpu::LoadOp::Clear(wgpu::Color::WHITE),
        );
        let stats = terrain.draw_span(
            &mut encoder,
            &cv,
            &dv,
            &batch,
            &atlas,
            &buffer,
            &instances,
            [(0, TerrainPiece::Body), (1, TerrainPiece::Shadow)],
            [10, 10, 0, 0],
        );
        assert_eq!(stats, Default::default(), "empty clip submits no passes");
        let reads = [
            gpu.read(&mut encoder, &color),
            gpu.read(&mut encoder, &depth),
        ];
        let output = gpu.finish(encoder, &reads, size);
        assert!(
            output[0]
                .chunks_exact(4)
                .all(|p| p == encoded(0xffff, format))
        );
        assert!(
            output[1]
                .chunks_exact(4)
                .all(
                    |p| super::native_z::stored_z(f32::from_le_bytes(p.try_into().unwrap()))
                        == 65535
                )
        );
    }
    eprintln!(
        "TREE batching: {cases} multi-wave source/target/camera cases matched sequential color+depth in both target formats; empty clips preserved"
    );
}

#[test]
#[ignore = "bounded GPU/CPU timing; close other renderers before uncontended measurements"]
fn production_tree_piece_workload_timing() {
    use std::time::Instant;
    let gpu = Gpu::with_features(
        wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS,
    );
    let format = wgpu::TextureFormat::Bgra8UnormSrgb;
    let size = [800, 600];
    let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
    batch.write_camera(&gpu.queue, camera(size));
    let mut terrain = TerrainDrawRenderer::new(&gpu.device, format, &batch);
    let color = gpu.target(size, format);
    let cv = color.create_view(&Default::default());
    let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
    let dv = depth.create_view(&Default::default());
    terrain.prepare(&gpu.device, &color, &dv, batch.camera_uniform());
    let atlas = batch.create_texture_on_device(
        &gpu.device,
        &gpu.queue,
        &[80, 180, 80, 255],
        1,
        1,
        Some(&[1]),
    );
    let queries = gpu.device.create_query_set(&wgpu::QuerySetDescriptor {
        label: Some("Tree workload interval"),
        ty: wgpu::QueryType::Timestamp,
        count: 2,
    });
    let resolved = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Tree timestamp resolve"),
        size: 256,
        usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let mapped = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Tree timestamp map"),
        size: 16,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut results = Vec::new();
    // Original frozen800x600 workloads remain literal. The added32-pair
    // disjoint grids fit this target;512 full-size disjoint pairs cannot.
    // Pixel-disjoint rectangles can still share conservative dependency tiles.
    for count in [1usize, 32, 128, 512] {
        for layout in ["dispersed", "repeated", "pixel_disjoint", "tile_disjoint"] {
            if layout.ends_with("disjoint") && count != 32 {
                continue;
            }
            let instances = (0..count)
                .flat_map(|i| {
                    let [x, y] = match layout {
                        "repeated" => [350.0, 250.0],
                        "pixel_disjoint" => {
                            [10.0 + (i % 8) as f32 * 96.0, 10.0 + (i / 8) as f32 * 80.0]
                        }
                        "tile_disjoint" => {
                            [4.0 + (i % 6) as f32 * 128.0, 4.0 + (i / 6) as f32 * 96.0]
                        }
                        _ => [
                            10.0 + (i % 10) as f32 * 70.0,
                            10.0 + ((i / 10) % 6) as f32 * 80.0,
                        ],
                    };
                    [
                        SpriteInstance {
                            z_gradient: 2,
                            ..sprite([x, y], [33.0, 78.0], -12.0 - i as f32)
                        },
                        sprite([x + 18.0, y + 42.0], [74.0, 36.0], -3.0 - i as f32),
                    ]
                })
                .collect::<Vec<_>>();
            let buffer = gpu
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Tree workload instances"),
                    contents: bytemuck::cast_slice(&instances),
                    usage: wgpu::BufferUsages::VERTEX,
                });
            for batched in [false, true] {
                eprintln!("TREE timing begins: pairs={count}, layout={layout}, batched={batched}");
                for sample in 0..4 {
                    // Exactly one application frame per submission preserves
                    // the established command-buffer-memory measurement bound.
                    let begin = Instant::now();
                    let mut encoder = gpu.device.create_command_encoder(&Default::default());
                    encoder.write_timestamp(&queries, 0);
                    clear(
                        &mut encoder,
                        &cv,
                        &dv,
                        65535,
                        wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                    );
                    let kind = |i| {
                        if i % 2 == 0 {
                            TerrainPiece::Body
                        } else {
                            TerrainPiece::Shadow
                        }
                    };
                    let stats = if batched {
                        terrain.draw_span(
                            &mut encoder,
                            &cv,
                            &dv,
                            &batch,
                            &atlas,
                            &buffer,
                            &instances,
                            (0..instances.len()).map(|i| (i as u32, kind(i))),
                            [0, 0, 800, 600],
                        )
                    } else {
                        for (i, instance) in instances.iter().enumerate() {
                            terrain.draw_piece(
                                &mut encoder,
                                &cv,
                                &dv,
                                &batch,
                                &atlas,
                                &buffer,
                                i as u32,
                                instance,
                                kind(i),
                                [0, 0, 800, 600],
                            );
                        }
                        super::terrain_draw::TerrainBatchStats {
                            pieces: instances.len(),
                            waves: instances.len(),
                            tile_dependencies: 0,
                        }
                    };
                    encoder.write_timestamp(&queries, 1);
                    let encode_ms = begin.elapsed().as_secs_f64() * 1000.0;
                    if batched {
                        let expected_waves = match layout {
                            "repeated" => 2 * count,
                            "pixel_disjoint" => 22,
                            "tile_disjoint" => 2,
                            _ => match count {
                                1 => 2,
                                32 => 24,
                                128 => 34,
                                512 => 58,
                                _ => unreachable!(),
                            },
                        };
                        assert_eq!(
                            stats.waves, expected_waves,
                            "independent scheduling prototype"
                        );
                    }
                    assert_eq!(stats.pieces, count * 2);
                    encoder.resolve_query_set(&queries, 0..2, &resolved, 0);
                    encoder.copy_buffer_to_buffer(&resolved, 0, &mapped, 0, 16);
                    let submission = gpu.queue.submit([encoder.finish()]);
                    let (tx, rx) = std::sync::mpsc::channel();
                    mapped
                        .slice(..)
                        .map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
                    gpu.device
                        .poll(wgpu::PollType::Wait {
                            submission_index: Some(submission),
                            timeout: Some(Duration::from_secs(60)),
                        })
                        .unwrap();
                    rx.recv().unwrap().unwrap();
                    let data = mapped.slice(..).get_mapped_range();
                    let first = u64::from_le_bytes(data[0..8].try_into().unwrap());
                    let last = u64::from_le_bytes(data[8..16].try_into().unwrap());
                    assert!(last > first);
                    let gpu_ms = (last - first) as f64 * gpu.queue.get_timestamp_period() as f64
                        / 1_000_000.0;
                    drop(data);
                    mapped.unmap();
                    if sample > 0 {
                        eprintln!(
                            "TREE timing sample: pairs={count}, layout={layout}, batched={batched}, sample={sample}, waves={}, passes={}, tile_dependencies={}, gpu_ms={gpu_ms}, cpu_prepare_encode_ms={encode_ms}",
                            stats.waves,
                            stats.waves * 2,
                            stats.tile_dependencies
                        );
                        results.push(serde_json::json!({"tree_pairs":count,"layout":layout,"overlap":layout=="repeated","batched":batched,"sample":sample,
                            "frames":1,"pieces":stats.pieces,"waves":stats.waves,"passes":stats.waves*2,"tile_dependencies":stats.tile_dependencies,
                            "gpu_ms_per_frame":gpu_ms,"cpu_prepare_encode_ms_per_frame":encode_ms}));
                    }
                }
            }
        }
    }
    let report = serde_json::json!({"target":[800,600],"color_format":"Bgra8UnormSrgb",
        "source":"frozen original opaque TREE01-sized workload positions plus32 pixel-disjoint and tile-disjoint pairs; actual sequential and batched snapshot/edit owner",
        "interval":"GPU includes frame clears and every snapshot/edit. CPU includes scheduler reset/preparation, encoder construction and commands, excludes finish/submit/wait. Setup excluded. tile_dependencies counts covered tiles per command, each queried then updated.",
        "samples":results});
    eprintln!("{}", serde_json::to_string_pretty(&report).unwrap());
    if let Some(path) = std::env::var_os("VERA20K_TERRAIN_PERF_OUTPUT") {
        std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    }
}
