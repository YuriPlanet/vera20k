//! Headless checks of the production Batch upload, TerrainDrawRenderer snapshot,
//! piece edit, and shared camera/instance ABI. Synthetic native leaf goldens are
//! produced by tools/terrain_draw_oracle/leaf.py, not by the Rust implementation.

use super::{
    batch::{BatchRenderer, CameraUniform, SpriteInstance},
    terrain_draw::{DestinationEditCommand, TerrainDrawRenderer, TerrainPiece},
};
use crate::render::gpu::block_on;
use crate::render::tactical_draw_plan::RenderZPolicy;
use std::time::Duration;
use wgpu::util::DeviceExt;

fn terrain_command(index: u32, piece: TerrainPiece) -> DestinationEditCommand {
    DestinationEditCommand {
        index,
        piece,
        render_z: RenderZPolicy::ReadWrite,
        atlas_slot: 0,
    }
}

pub(crate) struct Gpu {
    pub(crate) device: wgpu::Device,
    pub(crate) queue: wgpu::Queue,
}

/// Shared production-draw timing interval. Query resources are reused; asset,
/// upload and attachment setup stay outside the measured encoder callback.
pub(crate) struct GpuFrameTimer {
    queries: [wgpu::QuerySet; 2],
    resolved: wgpu::Buffer,
    mapped: wgpu::Buffer,
    marker: wgpu::ComputePipeline,
    marker_bind_group: wgpu::BindGroup,
}

impl GpuFrameTimer {
    pub(crate) fn new(gpu: &Gpu) -> Self {
        // Real dispatches make both pass-boundary samples observable on Metal.
        // The prior encoder-only interval failed its monotonicity check on M4.
        // Upstream27.0.4 documents empty-encoder sampling pitfalls:
        // https://github.com/gfx-rs/wgpu/blob/v27.0.4/wgpu-hal/src/metal/command.rs#L46-L59
        let shader = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Timestamp boundary dispatch"),
            source: wgpu::ShaderSource::Wgsl("@group(0) @binding(0) var<storage, read_write> counter: u32; @compute @workgroup_size(1) fn main() { counter += 1u; }".into()),
        });
        let marker = gpu
            .device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("Timestamp boundary dispatch"),
                layout: None,
                module: &shader,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                cache: None,
            });
        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Timestamp marker storage"),
            size: 4,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let marker_bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Timestamp marker storage"),
            layout: &marker.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });
        Self {
            queries: std::array::from_fn(|_| {
                gpu.device.create_query_set(&wgpu::QuerySetDescriptor {
                    label: Some("Production draw interval"),
                    ty: wgpu::QueryType::Timestamp,
                    count: 1,
                })
            }),
            resolved: gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Production timestamp resolve"),
                size: 512,
                usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            }),
            mapped: gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Production timestamp map"),
                size: 16,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            }),
            marker,
            marker_bind_group,
        }
    }

    fn mark(&self, encoder: &mut wgpu::CommandEncoder, index: u32) {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("Production timing boundary"),
            timestamp_writes: Some(wgpu::ComputePassTimestampWrites {
                query_set: &self.queries[index as usize],
                // Use one end sample per set, resolved separately. The prior
                // two-slot interval returned a zero second sample on M4.
                // The interval includes the final one-workgroup marker.
                beginning_of_pass_write_index: None,
                end_of_pass_write_index: Some(0),
            }),
        });
        pass.set_pipeline(&self.marker);
        pass.set_bind_group(0, &self.marker_bind_group, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }

    /// CPU includes encoder construction/commands, excluding final submit/wait.
    /// GPU includes callback commands/intermediate submits and the final marker.
    pub(crate) fn measure<T>(
        &self,
        gpu: &Gpu,
        encode: impl FnOnce(&mut wgpu::CommandEncoder) -> T,
    ) -> (T, f64, f64) {
        let begin = std::time::Instant::now();
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        self.mark(&mut encoder, 0);
        let result = encode(&mut encoder);
        self.mark(&mut encoder, 1);
        let cpu_ms = begin.elapsed().as_secs_f64() * 1000.0;
        let submission = gpu.queue.submit([encoder.finish()]);
        // Resolve only after the producer has completed. Immediate resolve in
        // the producer command buffer yielded a zero final sample on this M4.
        gpu.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(Duration::from_secs(60)),
            })
            .unwrap();
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        for (index, queries) in self.queries.iter().enumerate() {
            let offset = index as u64 * wgpu::QUERY_RESOLVE_BUFFER_ALIGNMENT;
            encoder.resolve_query_set(queries, 0..1, &self.resolved, offset);
            encoder.copy_buffer_to_buffer(
                &self.resolved,
                offset,
                &self.mapped,
                index as u64 * 8,
                8,
            );
        }
        let submission = gpu.queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        self.mapped
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
        gpu.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(Duration::from_secs(60)),
            })
            .unwrap();
        rx.recv().unwrap().unwrap();
        let data = self.mapped.slice(..).get_mapped_range();
        let first = u64::from_le_bytes(data[0..8].try_into().unwrap());
        let last = u64::from_le_bytes(data[8..16].try_into().unwrap());
        assert!(
            last > first,
            "GPU interval timestamps: first={first}, last={last}"
        );
        let gpu_ms = (last - first) as f64 * gpu.queue.get_timestamp_period() as f64 / 1_000_000.0;
        drop(data);
        self.mapped.unmap();
        (result, cpu_ms, gpu_ms)
    }
}

impl Gpu {
    pub(crate) fn new() -> Self {
        Self::with_features(wgpu::Features::empty())
    }
    pub(crate) fn with_features(features: wgpu::Features) -> Self {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            // Use the production owner's standard WGPU_BACKEND override so
            // the same native readbacks can exercise Windows DX12 explicitly.
            backends: wgpu::Backends::PRIMARY.with_env(),
            ..Default::default()
        });
        let adapter = block_on(instance.request_adapter(&Default::default()))
            .expect("explicit GPU check needs an adapter");
        eprintln!("Terrain production owner GPU: {:?}", adapter.get_info());
        assert!(
            adapter.features().contains(features),
            "required explicit GPU probe features unavailable"
        );
        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
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
        self.load_texels(view, origin, size, "texture_2d<u32>", "")
            .as_chunks::<4>()
            .0
            .iter()
            .map(|texel| texel[0] as u8)
            .collect()
    }
    /// Read back one rectangle of a float texture as the values `textureLoad`
    /// returns (linear for sRGB formats).
    pub(crate) fn read_float_texels(
        &self,
        view: &wgpu::TextureView,
        origin: [u32; 2],
        size: [u32; 2],
    ) -> Vec<[f32; 4]> {
        self.load_texels(view, origin, size, "texture_2d<f32>", "bitcast<vec4<u32>>")
            .as_chunks::<4>()
            .0
            .iter()
            .map(|texel| std::array::from_fn(|channel| f32::from_bits(texel[channel])))
            .collect()
    }
    /// Each texel's four `textureLoad` channels, as `words` converts them.
    fn load_texels(
        &self,
        view: &wgpu::TextureView,
        origin: [u32; 2],
        size: [u32; 2],
        texture: &str,
        words: &str,
    ) -> Vec<u32> {
        let ([x, y], [width, height]) = (origin, size);
        let source = format!(
            "@group(0) @binding(0) var t:{texture}; @group(0) @binding(1) var<storage,read_write> output:array<vec4<u32>>; @compute @workgroup_size(8,8) fn main(@builtin(global_invocation_id) p:vec3<u32>) {{ if p.x<{width}u && p.y<{height}u {{ output[p.y*{width}u+p.x]={words}(textureLoad(t,vec2<i32>(i32(p.x+{x}u),i32(p.y+{y}u)),0)); }} }}"
        );
        let shader = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("texel readback"),
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
            size: u64::from(width * height * 16),
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
        data.as_chunks::<4>()
            .0
            .iter()
            .map(|word| u32::from_le_bytes(*word))
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
    let goldens = crate::test_fixture::bytes("tools/terrain_draw_oracle/fixtures/half-rgb565.bin");
    for format in [
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ] {
        let size = [256; 2];
        let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
        let mut terrain = TerrainDrawRenderer::new(&gpu.device, &gpu.queue, format, &batch);
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
            &instance,
            terrain_command(0, TerrainPiece::Shadow),
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

/// StripDraw401 uses the same native packed-half kernel with no Z reads or
/// writes. An output-local camera/scratch must not inherit the tactical view.
#[test]
#[ignore = "requires a GPU; executes the production no-Z packed surface"]
fn production_no_z_mask_exhausts_words_and_preserves_depth() {
    use super::terrain_draw::{PackedCompositePiece, PackedSpriteKind};
    let gpu = Gpu::new();
    let goldens = crate::test_fixture::bytes("tools/terrain_draw_oracle/fixtures/half-rgb565.bin");
    let half = |word: u16| {
        let i = usize::from(word) * 2;
        u16::from_le_bytes([goldens[i], goldens[i + 1]])
    };
    for format in [
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ] {
        let size = [256; 2];
        let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
        let mut renderer = TerrainDrawRenderer::new(&gpu.device, &gpu.queue, format, &batch);
        let primary = gpu.target([7, 9], format);
        let primary_depth = gpu.target([7, 9], wgpu::TextureFormat::Depth32Float);
        let primary_dv = primary_depth.create_view(&Default::default());
        renderer.prepare(&gpu.device, &primary, &primary_dv, camera([7, 9]));
        let color = gpu.target(size, format);
        let cv = color.create_view(&Default::default());
        let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
        let dv = depth.create_view(&Default::default());
        let surface = renderer.create_packed_surface(&gpu.device, &color, &dv);
        let offset = [137.25, -31.5];
        let ui = CameraUniform {
            camera_pos: offset,
            ..camera(size)
        };
        let (ui_buffer, ui_group) = batch.create_ui_camera(&gpu.device);
        gpu.queue
            .write_buffer(&ui_buffer, 0, bytemuck::bytes_of(&ui));
        let bytes: Vec<_> = (0..=u16::MAX)
            .flat_map(|word| encoded(word, format))
            .collect();
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
        // Source colour, lighting and native candidate are deliberately
        // irrelevant. Candidate0 versus storedZ0 would fail any Z test.
        let mask = batch.create_texture_on_device(
            &gpu.device,
            &gpu.queue,
            &[17, 231, 49, 0],
            1,
            1,
            Some(&[255]),
        );
        let instance = SpriteInstance {
            draw_state: super::draw_state::DrawState::from_native_selector(0, 1),
            ..sprite(offset, [256.0; 2], 65535.0)
        };
        let buffer = gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Native no-Z mask ABI"),
                contents: bytemuck::bytes_of(&instance),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let piece = PackedCompositePiece {
            index: 0,
            instance: &instance,
            source: &mask.bind_group,
            material: batch.default_zshape_bind_group(),
            kind: PackedSpriteKind::IndexMask,
        };
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        clear(&mut encoder, &cv, &dv, 0, wgpu::LoadOp::Load);
        let mut reads = Vec::new();
        for _ in 0..2 {
            renderer.draw_packed_surface(
                &mut encoder,
                &cv,
                &dv,
                &surface,
                ui,
                &ui_group,
                &buffer,
                &[piece],
            );
            reads.push(gpu.read(&mut encoder, &color));
        }
        reads.push(gpu.read(&mut encoder, &depth));
        let output = gpu.finish(encoder, &reads, size);
        for word in 0..=u16::MAX {
            let at = usize::from(word) * 4;
            assert_eq!(
                output[0][at..at + 4],
                encoded(half(word), format),
                "first {format:?} word{word:04x}"
            );
            assert_eq!(
                output[1][at..at + 4],
                encoded(half(half(word)), format),
                "repeat {format:?} word{word:04x}"
            );
            assert_eq!(
                f32::from_le_bytes(output[2][at..at + 4].try_into().unwrap()),
                super::native_z::stored_depth(0)
            );
        }
    }
}

#[test]
#[ignore = "requires a GPU; executes native N=1 words through the no-Z UI compositor"]
fn production_no_z_ui_half_blend_matches_native_controls() {
    use super::terrain_draw::{PackedCompositePiece, PackedSpriteKind};
    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/procedural_drawing_oracle/translucent_blitter_a.json",
    ))
    .unwrap();
    let rows: Vec<_> = native["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| {
            row["route"] == "shp"
                && row["profile"] == "plain1"
                && row["selector_bits"] == 4
                && row["brightness"] == 1000
                && row["displacement"] == 0
                && row["writes_depth"] == false
        })
        .collect();
    assert!(!rows.is_empty());
    let gpu = Gpu::new();
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let size = [rows.len() as u32, 1];
    let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
    batch.write_camera(&gpu.queue, camera(size));
    let mut renderer = TerrainDrawRenderer::new(&gpu.device, &gpu.queue, format, &batch);
    let color = gpu.target(size, format);
    let cv = color.create_view(&Default::default());
    let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
    let dv = depth.create_view(&Default::default());
    renderer.prepare(&gpu.device, &color, &dv, camera(size));
    let surface = renderer.create_packed_surface(&gpu.device, &color, &dv);
    let bytes: Vec<_> = rows
        .iter()
        .flat_map(|row| encoded(row["raw"].as_u64().unwrap() as u16, format))
        .collect();
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
            bytes_per_row: Some(size[0] * 4),
            rows_per_image: Some(1),
        },
        extent(size),
    );
    let indices: Vec<_> = rows
        .iter()
        .map(|row| row["index"].as_u64().unwrap() as u8)
        .collect();
    // The production clock loader already performs ordinary N=1 conversion.
    let profile = super::native_surface_format::ACTIVE_RETAIL_RGB565_PRESENTATION;
    let rgba: Vec<_> = rows
        .iter()
        .flat_map(|row| {
            profile.quantize_rgba8([
                row["rgb"][0].as_u64().unwrap() as u8,
                row["rgb"][1].as_u64().unwrap() as u8,
                row["rgb"][2].as_u64().unwrap() as u8,
                255,
            ])
        })
        .collect();
    let texture =
        batch.create_texture_on_device(&gpu.device, &gpu.queue, &rgba, size[0], 1, Some(&indices));
    let instance = SpriteInstance {
        draw_state: super::draw_state::DrawState::from_native_selector(0, 4),
        ..sprite([0.0; 2], [size[0] as f32, 1.0], 65535.0)
    };
    let buffer = gpu
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::bytes_of(&instance),
            usage: wgpu::BufferUsages::VERTEX,
        });
    let piece = PackedCompositePiece {
        index: 0,
        instance: &instance,
        source: &texture.bind_group,
        material: batch.default_zshape_bind_group(),
        kind: PackedSpriteKind::UiPalette,
    };
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    clear(&mut encoder, &cv, &dv, 0, wgpu::LoadOp::Load);
    renderer.draw_packed_surface(
        &mut encoder,
        &cv,
        &dv,
        &surface,
        camera(size),
        batch.camera_bind_group(),
        &buffer,
        &[piece],
    );
    let reads = [
        gpu.read(&mut encoder, &color),
        gpu.read(&mut encoder, &depth),
    ];
    let output = gpu.finish(encoder, &reads, size);
    for (i, row) in rows.iter().enumerate() {
        assert_eq!(
            output[0][i * 4..i * 4 + 4],
            encoded(row["colors"][0].as_u64().unwrap() as u16, format),
            "native control{i}: {row}"
        );
        assert_eq!(
            f32::from_le_bytes(output[1][i * 4..i * 4 + 4].try_into().unwrap()),
            super::native_z::stored_depth(0)
        );
    }
}

#[test]
#[ignore = "requires a GPU; original signed/store/repeat leaf cases"]
fn production_tree_signed_depth_and_repeated_overlap_match_original_leaves() {
    let gpu = Gpu::new();
    let format = wgpu::TextureFormat::Bgra8UnormSrgb;
    let size = [32, 4];
    let fixture: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/terrain_draw_oracle/fixtures/leaf.json",
    ))
    .unwrap();
    let cases = fixture["depth_cases"].as_array().unwrap();
    let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
    let mut terrain = TerrainDrawRenderer::new(&gpu.device, &gpu.queue, format, &batch);
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
                |_| Some(&atlas),
                &buffer,
                &instances,
                selected.iter().enumerate().map(|(i, c)| {
                    terrain_command(
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
    let fixture: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/terrain_draw_oracle/fixtures/rows.json",
    ))
    .unwrap();
    let cases = fixture["cases"].as_array().unwrap();
    let size = [cases.len() as u32, 550];
    let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
    let mut terrain = TerrainDrawRenderer::new(&gpu.device, &gpu.queue, format, &batch);
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
                &instances[i],
                terrain_command(i as u32, TerrainPiece::Body),
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
    let mut terrain = TerrainDrawRenderer::new(&gpu.device, &gpu.queue, format, &batch);
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
                instance,
                terrain_command(i as u32, TerrainPiece::Body),
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
    let mut terrain = TerrainDrawRenderer::new(&gpu.device, &gpu.queue, format, &batch);
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
                        &instance,
                        terrain_command(0, TerrainPiece::Body),
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
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|p| *p == encoded(0x55aa, format)),
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
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .all(|p| *p == encoded(0xf800, format)),
                "UI equality row{row}"
            );
        }
    }
}

#[test]
#[ignore = "requires GPU; multi-piece wave equivalence, clips, camera and target reuse"]
fn production_mixed_destination_waves_preserve_pixels_depth_and_atlas_order() {
    let gpu = Gpu::new();
    let mut cases = 0;
    for format in [
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ] {
        let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
        let mut terrain = TerrainDrawRenderer::new(&gpu.device, &gpu.queue, format, &batch);
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
        let second_rgba: Vec<u8> = indices.iter().flat_map(|_| [16, 64, 240, 255]).collect();
        let second_atlas = batch.create_texture_on_device(
            &gpu.device,
            &gpu.queue,
            &second_rgba,
            4,
            4,
            Some(&indices),
        );
        let atlases = [&atlas, &second_atlas];
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
            // Read-only Bullet-like pieces cross both atlas pages and overlap
            // stored Terrain depth. Native leaf tests independently establish
            // admission/color semantics; this tests scheduling and page order.
            ([2., 2.], [24., 28.], TerrainPiece::Body, -33000.),
            ([66., 2.], [24., 28.], TerrainPiece::Body, -33000.),
            ([2., 2.], [24., 28.], TerrainPiece::Shadow, -33000.),
            ([66., 2.], [24., 28.], TerrainPiece::Shadow, -33000.),
            ([2., 2.], [24., 28.], TerrainPiece::Shadow, -33000.),
        ];
        let command = |index: usize| DestinationEditCommand {
            index: index as u32,
            piece: specs[index].2,
            render_z: if index < 13 {
                RenderZPolicy::ReadWrite
            } else {
                RenderZPolicy::ReadOnly
            },
            atlas_slot: if index < 13 { 0 } else { index % 2 },
        };
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
                            |slot| atlases.get(slot).copied(),
                            &buffer,
                            &instances,
                            (0..specs.len()).map(command),
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
                                atlases[command(i).atlas_slot],
                                &buffer,
                                instance,
                                command(i),
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
                for (attachment, (first, second)) in outputs[0].iter().zip(&outputs[1]).enumerate()
                {
                    assert_eq!(first.len(), second.len());
                    let mismatches = first
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .zip(second.as_chunks::<4>().0)
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
            |slot| atlases.get(slot).copied(),
            &buffer,
            &instances,
            [
                terrain_command(0, TerrainPiece::Body),
                terrain_command(1, TerrainPiece::Shadow),
            ],
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
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| *p == encoded(0xffff, format))
        );
        assert!(
            output[1]
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| super::native_z::stored_z(f32::from_le_bytes(*p)) == 65535)
        );
    }
    eprintln!(
        "Mixed Terrain/Bullet batching: {cases} multi-wave/page/source/target/camera cases matched sequential color+depth in both target formats; empty clips preserved"
    );
}

#[test]
#[ignore = "bounded GPU/CPU timing; close other renderers before uncontended measurements"]
fn production_tree_piece_workload_timing() {
    let gpu = Gpu::with_features(wgpu::Features::TIMESTAMP_QUERY);
    let format = wgpu::TextureFormat::Bgra8UnormSrgb;
    let size = [800, 600];
    let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
    batch.write_camera(&gpu.queue, camera(size));
    let mut terrain = TerrainDrawRenderer::new(&gpu.device, &gpu.queue, format, &batch);
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
    let timer = GpuFrameTimer::new(&gpu);
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
                    let (stats, encode_ms, gpu_ms) = timer.measure(&gpu, |encoder| {
                        clear(
                            encoder,
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
                                encoder,
                                &cv,
                                &dv,
                                &batch,
                                |_| Some(&atlas),
                                &buffer,
                                &instances,
                                (0..instances.len()).map(|i| terrain_command(i as u32, kind(i))),
                                [0, 0, 800, 600],
                            )
                        } else {
                            for (i, instance) in instances.iter().enumerate() {
                                terrain.draw_piece(
                                    encoder,
                                    &cv,
                                    &dv,
                                    &batch,
                                    &atlas,
                                    &buffer,
                                    instance,
                                    terrain_command(i as u32, kind(i)),
                                    [0, 0, 800, 600],
                                );
                            }
                            super::terrain_draw::TerrainBatchStats {
                                pieces: instances.len(),
                                waves: instances.len(),
                                passes: instances.len() * 2,
                                tile_dependencies: 0,
                            }
                        };
                        stats
                    });
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
                    if sample > 0 {
                        eprintln!(
                            "TREE timing sample: pairs={count}, layout={layout}, batched={batched}, sample={sample}, waves={}, passes={}, tile_dependencies={}, gpu_ms={gpu_ms}, cpu_prepare_encode_ms={encode_ms}",
                            stats.waves, stats.passes, stats.tile_dependencies
                        );
                        results.push(serde_json::json!({"tree_pairs":count,"layout":layout,"overlap":layout=="repeated","batched":batched,"sample":sample,
                            "frames":1,"pieces":stats.pieces,"waves":stats.waves,"passes":stats.passes,"tile_dependencies":stats.tile_dependencies,
                            "gpu_ms_per_frame":gpu_ms,"cpu_prepare_encode_ms_per_frame":encode_ms}));
                    }
                }
            }
        }
    }
    let report = serde_json::json!({"target":[800,600],"color_format":"Bgra8UnormSrgb",
        "source":"frozen original opaque TREE01-sized workload positions plus32 pixel-disjoint and tile-disjoint pairs; actual sequential and batched snapshot/edit owner",
        "interval":"GPU includes frame clears, every snapshot/edit and the final one-workgroup timestamp marker. CPU includes scheduler reset/preparation, encoder construction and commands, excludes finish/submit/wait. Setup excluded. tile_dependencies counts covered tiles per command, each queried then updated.",
        "samples":results});
    eprintln!("{}", serde_json::to_string_pretty(&report).unwrap());
    if let Some(path) = std::env::var_os("VERA20K_TERRAIN_PERF_OUTPUT") {
        std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    }
}

pub(crate) fn seed_depth(
    gpu: &Gpu,
    encoder: &mut wgpu::CommandEncoder,
    depth: &wgpu::TextureView,
    words: &[u16],
) {
    seed_depth_grid(gpu, encoder, depth, words, words.len() as u32);
}

/// Prepare arbitrary native destination Z words, without passing through any
/// renderer admission logic under test. This shader only writes fixture data.
pub(crate) fn seed_depth_grid(
    gpu: &Gpu,
    encoder: &mut wgpu::CommandEncoder,
    depth: &wgpu::TextureView,
    words: &[u16],
    width: u32,
) {
    let values = words
        .iter()
        .map(|&word| crate::render::native_z::stored_depth(word))
        .collect::<Vec<_>>();
    let buffer = gpu
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Original destination Z words"),
            contents: bytemuck::cast_slice(&values),
            usage: wgpu::BufferUsages::STORAGE,
        });
    let shader = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Original destination depth fixture upload"),
        source: wgpu::ShaderSource::Wgsl(format!(
            "@group(0) @binding(0) var<storage,read> words:array<f32>; @vertex fn vs(@builtin(vertex_index) i:u32)->@builtin(position) vec4f {{ let p=array<vec2f,3>(vec2f(-1.,-1.),vec2f(3.,-1.),vec2f(-1.,3.)); return vec4f(p[i],0.,1.); }} @fragment fn fs(@builtin(position) p:vec4f)->@builtin(frag_depth) f32 {{ return words[(u32(p.y)*{width}u+u32(p.x))%arrayLength(&words)]; }}"
        ).into()),
    });
    let pipeline = gpu
        .device
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Original destination depth fixture upload"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                targets: &[],
                compilation_options: Default::default(),
            }),
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Always,
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
    let binding = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: buffer.as_entire_binding(),
        }],
    });
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Original destination depth fixture upload"),
        color_attachments: &[],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: depth,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(1.0),
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
    });
    pass.set_pipeline(&pipeline);
    pass.set_bind_group(0, &binding, &[]);
    pass.draw(0..3, 0..1);
}

/// Physical PIPS2/PALETTE bytes and native centered calls, not native screen
/// goldens: LoadPipAssets5F76B0 ->709AEF/70A0B5..1D9 ->CC_Draw_Shape4AED70.
/// The readback protects the shared resource upload/UV/origin/transparency and
/// production status pipeline; gameplay count/flash admission is tested by app.
#[test]
#[ignore = "requires GPU and physical retail assets; actual PIPS2 centered resource/status draw"]
fn retail_pips2_centered_strip_preserves_native_frames_palette_and_scissor() {
    use crate::assets::{
        asset_manager::{AssetManager, MediaArchiveMode},
        pal_file::Palette,
        shp_file::ShpFile,
    };
    use crate::render::{batch::InstanceBufferPool, selection_overlay::CenteredShapeStrip};

    let root = std::env::var("RA2_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            crate::util::config::GameConfig::load()
                .unwrap()
                .paths
                .ra2_dir
        });
    let assets = AssetManager::new(&root, MediaArchiveMode::STOCK_DIGITAL).unwrap();
    let shp = ShpFile::from_bytes(&assets.get("pips2.shp").unwrap()).unwrap();
    let palette = Palette::from_bytes(&assets.get("palette.pal").unwrap()).unwrap();
    assert_eq!((shp.width, shp.height, shp.frames.len()), (8, 15, 19));
    for frame in &shp.frames[..6] {
        assert_eq!(
            (
                frame.frame_x,
                frame.frame_y,
                frame.frame_width,
                frame.frame_height
            ),
            (1, 4, 4, 4),
        );
    }
    // Physical raw indices distinguish transparent empty slots from black
    // outlines, and native flash3 from idle-overload4. No house tint applies.
    assert_eq!(&shp.frames[0].pixels[5..7], &[0, 0]);
    assert_eq!(&shp.frames[3].pixels[5..7], &[0x81, 0x84]);
    assert_eq!(&shp.frames[4].pixels[5..7], &[0xe5, 0xe8]);
    let gpu = Gpu::new();
    let size = [40, 24];
    for format in [
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ] {
        let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
        batch.write_camera(&gpu.queue, camera(size));
        let strip = CenteredShapeStrip::from_assets_on_device(
            &gpu.device,
            &gpu.queue,
            &batch,
            &assets,
            "pips2.shp",
            "palette.pal",
        )
        .expect("physical PIPS2 uses the production centered-strip owner");
        assert_eq!((strip.texture().width, strip.texture().height), (91, 14));
        // Base/extra pip frames and all three former cargo variants share one
        // texture. Native nonbuilding points advance(+4,0); origin(1,4) minus
        // integer canvas half(4,7) makes the first stored top-left(5,5).
        let frames = [0, 1, 3, 4, 0, 2, 5];
        let mut instances: Vec<_> = frames
            .iter()
            .enumerate()
            .map(|(slot, &frame)| {
                let instance = strip
                    .instance(frame, (8.0 + slot as f32 * 4.0, 8.0), 0.0004)
                    .unwrap();
                assert_eq!(instance.position, [5.0 + slot as f32 * 4.0, 5.0]);
                assert_eq!(instance.size, [4.0, 4.0]);
                instance
            })
            .collect();
        let tail = strip.instance(18, (8.0, 20.0), 0.0004).unwrap();
        assert_eq!(tail.position, [6.0, 15.0]);
        assert_eq!(tail.size, [4.0, 9.0]);
        assert!(strip.instance(19, (8.0, 20.0), 0.0004).is_none());
        instances.push(tail);
        let mut pool = InstanceBufferPool::new();
        pool.upload_on_device(&gpu.device, &gpu.queue, "scale_pips", &instances);
        let (buffer, count) = pool.get("scale_pips").unwrap();
        assert_eq!(count, 8);
        let color = gpu.target(size, format);
        let cv = color.create_view(&Default::default());
        let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
        let dv = depth.create_view(&Default::default());
        for clip in [[0, 0, 40, 24], [6, 6, 24, 2]] {
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            clear(
                &mut encoder,
                &cv,
                &dv,
                65535,
                wgpu::LoadOp::Clear(wgpu::Color::GREEN),
            );
            {
                // The production status path is named no_depth but uses its
                // real LessEqual/write attachment. Keep its camera and shader.
                let mut pass = crate::app::presentation::sidebar_render::begin_main_load_pass(
                    &mut encoder,
                    &cv,
                    &dv,
                );
                pass.set_scissor_rect(clip[0], clip[1], clip[2], clip[3]);
                batch.draw_with_buffer_no_depth(&mut pass, strip.texture(), buffer, count);
            }
            let read = gpu.read(&mut encoder, &color);
            let output = gpu.finish(encoder, &[read], size).remove(0);
            let mut expected = vec![[0, 255, 0, 255]; (size[0] * size[1]) as usize];
            // The oracle is physical source indices at original call positions,
            // independent of the atlas packing and returned instance UVs.
            for (frame, left, top) in frames
                .iter()
                .enumerate()
                .map(|(slot, &frame)| (frame, 5 + slot as u32 * 4, 5))
                .chain(std::iter::once((18, 6, 15)))
            {
                let source = &shp.frames[frame];
                for y in 0..u32::from(source.frame_height) {
                    for x in 0..u32::from(source.frame_width) {
                        let (dx, dy) = (left + x, top + y);
                        let index = source.pixels[(y * u32::from(source.frame_width) + x) as usize];
                        if index == 0
                            || dx < clip[0]
                            || dy < clip[1]
                            || dx >= clip[0] + clip[2]
                            || dy >= clip[1] + clip[3]
                        {
                            continue;
                        }
                        let c = palette.color(index);
                        expected[(dy * size[0] + dx) as usize] = [c.r, c.g, c.b, 255];
                    }
                }
            }
            for (pixel, actual) in output.as_chunks::<4>().0.iter().enumerate() {
                let actual = if format == wgpu::TextureFormat::Bgra8UnormSrgb {
                    [actual[2], actual[1], actual[0], actual[3]]
                } else {
                    *actual
                };
                assert_eq!(
                    actual,
                    expected[pixel],
                    "{format:?} scissor{clip:?},pixel({},{})",
                    pixel % size[0] as usize,
                    pixel / size[0] as usize,
                );
            }
        }
    }
}

/// Compare losslessly captured native RGB565/Z runs with both GPU attachments.
/// Shared by terrain, bridge and ordinary Cell caller witnesses.
pub(crate) fn assert_native_color_and_depth(
    output: &[Vec<u8>],
    row: &serde_json::Value,
    format: wgpu::TextureFormat,
    size: [u32; 2],
) {
    let input = &row["input"];
    let background = input["background"].as_u64().unwrap() as u16;
    let old_z = input["old_z"].as_u64().unwrap() as u16;
    let pixels = (size[0] * size[1]) as usize;
    // Lossless expansion of captured native buffers. This contains no
    // projection, depth arithmetic, stencil or color transformation.
    let mut expected = vec![[background, old_z]; pixels];
    for run in row["output"]["runs"].as_array().unwrap() {
        let values: [usize; 5] = std::array::from_fn(|i| run[i].as_u64().unwrap() as usize);
        let [x, y, count, word, z] = values;
        let start = y * size[0] as usize + x;
        expected[start..start + count].fill([word as u16, z as u16]);
    }
    for (index, [word, z]) in expected.into_iter().enumerate() {
        let offset = index * 4;
        let actual_z = super::native_z::stored_z(f32::from_le_bytes(
            output[1][offset..offset + 4].try_into().unwrap(),
        ));
        assert_eq!(
            (&output[0][offset..offset + 4], actual_z),
            (&encoded(word, format)[..], z),
            "{} {format:?} pixel ({},{})",
            row["name"],
            index % size[0] as usize,
            index / size[0] as usize,
        );
    }
}
