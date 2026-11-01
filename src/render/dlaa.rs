//! Spatial half of Barbatos' `BaBa_DLAA-T.fx` (MIT), the Directionally
//! Localized AA from Andreev's GDC 2011 talk.
//!
//! The effect runs two passes at frame resolution: the prefilter writes the
//! frame's gamma-space colour plus a crushed high-pass alpha, and the spatial
//! pass reads that map and blends the dominant short/long edge axis into the
//! centre texel. The optional temporal accumulation needs motion vectors and a
//! history buffer, neither of which the presentation chain has, so the port
//! stops at the spatial stage.
//!
//! Texels outside the tactical viewport keep the frame untouched, matching the
//! other presentation effects: the sidebar and bottom strip never soften.

use crate::rules::post_process::DlaaRules;
use wgpu::util::DeviceExt;

const SHADER: &str = include_str!("dlaa.wgsl");

/// Both entries share this uniform; the values are the same for each pass.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    /// strength, lambda, epsilon, edge contrast
    scalars: [f32; 4],
    /// long-edge threshold, unused, unused, unused
    gate: [f32; 4],
    /// (x, y, width, height) of the tactical viewport in frame pixels.
    viewport: [f32; 4],
}

/// One render target kept with its view for GPU resource ownership.
struct Target {
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
}

/// One frame's DLAA resources, sized for the frame.
pub(crate) struct Dlaa {
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    params: wgpu::Buffer,
    prefilter_pipeline: wgpu::RenderPipeline,
    spatial_pipeline: wgpu::RenderPipeline,
    prefilter: Target,
    size: (u32, u32),
}

impl Dlaa {
    pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("DLAA"),
            entries: &[
                texture_entry(0),
                uniform_entry(1),
                sampler_entry(2),
            ],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("DLAA"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("DLAA params"),
            contents: bytemuck::bytes_of(&Params {
                scalars: [0.0; 4],
                gate: [0.0; 4],
                viewport: [0.0; 4],
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("DLAA"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("DLAA"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        // The prefilter keeps the gamma-space colour in an RGBA8 map, the same
        // format the shader authors against.
        let prefilter_pipeline = create_pipeline(
            device,
            &pipeline_layout,
            &shader,
            "fs_prefilter",
            wgpu::TextureFormat::Rgba8Unorm,
        );
        let spatial_pipeline = create_pipeline(
            device,
            &pipeline_layout,
            &shader,
            "fs_spatial",
            format,
        );
        let prefilter = render_target(device, (1, 1));
        Self {
            layout,
            sampler,
            params,
            prefilter_pipeline,
            spatial_pipeline,
            prefilter,
            size: (0, 0),
        }
    }

    /// Follow the frame size and the live rules before this frame is encoded.
    pub(crate) fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        size: (u32, u32),
        rules: DlaaRules,
        viewport: [f32; 4],
    ) {
        let size = (size.0.max(1), size.1.max(1));
        if size != self.size {
            self.prefilter = render_target(device, size);
            self.size = size;
        }
        queue.write_buffer(
            &self.params,
            0,
            bytemuck::bytes_of(&Params {
                scalars: [
                    rules.strength,
                    rules.lambda,
                    rules.epsilon,
                    rules.edge_contrast,
                ],
                gate: [rules.long_edge_threshold, 0.0, 0.0, 0.0],
                viewport,
            }),
        );
    }

    /// Run the prefilter and spatial passes from `scene` into `destination`.
    pub(crate) fn draw(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        scene: &wgpu::TextureView,
        destination: &wgpu::TextureView,
    ) {
        let group = bind_group(
            device,
            &self.layout,
            scene,
            &self.params,
            &self.sampler,
        );
        run(
            encoder,
            &self.prefilter_pipeline,
            &group,
            &self.prefilter.view,
            "DLAA prefilter",
        );
        let group = bind_group(
            device,
            &self.layout,
            &self.prefilter.view,
            &self.params,
            &self.sampler,
        );
        run(
            encoder,
            &self.spatial_pipeline,
            &group,
            destination,
            "DLAA spatial",
        );
    }
}

fn render_target(device: &wgpu::Device, size: (u32, u32)) -> Target {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("DLAA prefilter"),
        size: wgpu::Extent3d {
            width: size.0.max(1),
            height: size.1.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    Target {
        _texture: texture,
        view,
    }
}

fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn uniform_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn sampler_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}

fn create_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    module: &wgpu::ShaderModule,
    entry: &str,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(entry),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(entry),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

fn bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    source: &wgpu::TextureView,
    params: &wgpu::Buffer,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("DLAA pass"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(source),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: params.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

fn run(
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::RenderPipeline,
    group: &wgpu::BindGroup,
    target: &wgpu::TextureView,
    label: &'static str,
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target,
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
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, group, &[]);
    pass.draw(0..3, 0..1);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::terrain_draw_gpu_tests::Gpu;
    use std::time::Duration;

    #[test]
    fn the_shader_compiles() {
        let module = naga::front::wgsl::parse_str(SHADER).expect("the shader parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("the shader validates");
    }

    /// DLAA leaves a perfectly straight edge alone but smooths its steps, so a
    /// staircase boundary must gain intermediate texels.
    #[test]
    fn a_staircase_edge_gains_intermediate_texels() {
        let gpu = Gpu::new();
        let size = (32u32, 32u32);
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let scene = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("DLAA scene"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let mut texels = vec![[0u8; 4]; (size.0 * size.1) as usize];
        for y in 0..size.1 {
            // A staircase boundary: four rows dark, then one texel to the right.
            let edge = size.0 / 2 + (y / 4);
            for x in 0..size.0 {
                let value = if x < edge { 40 } else { 220 };
                texels[(y * size.0 + x) as usize] = [value, value, value, 255];
            }
        }
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &scene,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(&texels),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size.0 * 4),
                rows_per_image: Some(size.1),
            },
            wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
        );
        let destination = gpu.target([size.0, size.1], format);
        let scene_view = scene.create_view(&wgpu::TextureViewDescriptor::default());
        let destination_view = destination.create_view(&wgpu::TextureViewDescriptor::default());
        let mut dlaa = Dlaa::new(&gpu.device, format);
        dlaa.prepare(
            &gpu.device,
            &gpu.queue,
            (size.0, size.1),
            DlaaRules::default(),
            [0.0, 0.0, size.0 as f32, size.1 as f32],
        );
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        dlaa.draw(&gpu.device, &mut encoder, &scene_view, &destination_view);
        let submission = gpu.queue.submit([encoder.finish()]);
        gpu.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(Duration::from_secs(60)),
            })
            .unwrap();
        let out = gpu.read_float_texels(&destination_view, [0, 0], [size.0, size.1]);
        let dark = out[(2 * size.0) as usize][0];
        let bright = out[(2 * size.0 + size.0 - 1) as usize][0];
        // Straight runs keep their values; only the steps should mix.
        let intermediate = out
            .iter()
            .filter(|texel| texel[0] > dark + 0.05 && texel[0] < bright - 0.1)
            .count();
        assert!(
            intermediate > 0,
            "the staircase steps gain intermediate texels: dark {dark}, bright {bright}"
        );
    }
}
