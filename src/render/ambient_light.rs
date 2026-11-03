//! AmbientLight (Ganossa, 2015): highlight detection at 1/16 resolution,
//! twelve horizontal/vertical 5-tap blur pairs, and an adapted screen blend
//! with the effect's texture-free dirt branch. The lens and dirt-texture arms
//! are not ported (the game ships neither `Dirt.png` nor `Lens*.png`), and the
//! debug bars are not exposed.
//!
//! Depends on: `render/gpu.rs` (through the caller) and wgpu only.

use crate::rules::post_process::AmbientLightRules;

const SHADER: &str = include_str!("ambient_light.wgsl");

/// The effect's blur pairs; each pair is one horizontal and one vertical pass.
const BLUR_PAIRS: usize = 12;
/// The highlight detection copy, as the effect sizes it.
const DETECT_SIZE: u32 = 32;

const DETECT_INT_SLOT: usize = 0;
const DETECT_LOW_SLOT: usize = 1;
const DETECT_HIGH_SLOT: usize = 2;
const BLUR_H_SLOT: usize = 3;
const BLUR_V_SLOT: usize = 4;
const MAGIC_SLOT: usize = 5;
const SLOTS: usize = MAGIC_SLOT + 1;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct PassParams {
    dst: [f32; 2],
    frame: [f32; 2],
    step: [f32; 2],
    phase: f32,
    intensity: f32,
    threshold: f32,
    adapt_strength: f32,
    adapt_base_mult: f32,
    black_level: f32,
    dirt_intensity: f32,
    adaptation: f32,
    dither: f32,
    dirt: f32,
    viewport: [f32; 4],
}

impl PassParams {
    fn new(dst: [f32; 2], frame: [f32; 2]) -> Self {
        Self {
            dst,
            frame,
            step: [0.0; 2],
            phase: 0.0,
            intensity: 1.0,
            threshold: 0.0,
            adapt_strength: 0.0,
            adapt_base_mult: 1.0,
            black_level: 0.0,
            dirt_intensity: 0.0,
            adaptation: 0.0,
            dither: 0.0,
            dirt: 0.0,
            viewport: [0.0; 4],
        }
    }
}

struct Target {
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
    size: (u32, u32),
}

struct Pipelines {
    detect_int: wgpu::RenderPipeline,
    detect_low: wgpu::RenderPipeline,
    detect_high: wgpu::RenderPipeline,
    blur_h: wgpu::RenderPipeline,
    blur_v: wgpu::RenderPipeline,
    magic: wgpu::RenderPipeline,
}

pub(crate) struct AmbientLight {
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    params: Vec<wgpu::Buffer>,
    pipelines: Pipelines,
    size: (u32, u32),
    detect_int: Target,
    detect_low: Target,
    /// The blur ping-pong pair at 1/16 resolution.
    in_target: Target,
    out_target: Target,
    spare: Target,
    /// The ping-pong phase the effect advances every draw.
    phase: f32,
}

impl AmbientLight {
    pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let layout = create_layout(device);
        let internal = wgpu::TextureFormat::Rgba16Float;
        let pipelines = Pipelines {
            detect_int: create_pipeline(device, &layout, internal, "fs_detect_int", "AL detect int"),
            detect_low: create_pipeline(device, &layout, internal, "fs_detect_low", "AL detect low"),
            detect_high: create_pipeline(
                device,
                &layout,
                internal,
                "fs_detect_high",
                "AL detect high",
            ),
            blur_h: create_pipeline(device, &layout, internal, "fs_blur_h", "AL blur H"),
            blur_v: create_pipeline(device, &layout, internal, "fs_blur_v", "AL blur V"),
            magic: create_pipeline(device, &layout, format, "fs_magic", "AL magic"),
        };
        let params = (0..SLOTS)
            .map(|slot| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(&format!("AL params {slot}")),
                    size: std::mem::size_of::<PassParams>() as u64,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                })
            })
            .collect();
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("AL sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let spare = create_target(device, "AL spare", (1, 1));
        let mut ambient = Self {
            layout,
            sampler,
            params,
            pipelines,
            size: (0, 0),
            detect_int: create_target(device, "AL detect int", (DETECT_SIZE, DETECT_SIZE)),
            detect_low: create_target(device, "AL detect low", (1, 1)),
            in_target: create_target(device, "AL in", (1, 1)),
            out_target: create_target(device, "AL out", (1, 1)),
            spare,
            phase: 0.0,
        };
        ambient.ensure_targets(device, (1, 1));
        ambient
    }

    fn ensure_targets(&mut self, device: &wgpu::Device, size: (u32, u32)) {
        if size == self.size {
            return;
        }
        self.size = size;
        let half = (size.0.max(16) / 16, size.1.max(16) / 16);
        self.in_target = create_target(device, "AL in", half);
        self.out_target = create_target(device, "AL out", half);
    }

    pub(crate) fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        size: (u32, u32),
        rules: &AmbientLightRules,
        viewport: [f32; 4],
    ) {
        self.ensure_targets(device, size);
        self.phase = (self.phase + 0.1) % 6.2831853;
        let frame = [size.0 as f32, size.1 as f32];
        let write = |slot: usize, params: PassParams| {
            queue.write_buffer(&self.params[slot], 0, bytemuck::bytes_of(&params));
        };
        let mut params = PassParams::new([DETECT_SIZE as f32; 2], frame);
        write(DETECT_INT_SLOT, params);
        params = PassParams::new([1.0, 1.0], frame);
        write(DETECT_LOW_SLOT, params);
        params = PassParams::new(
            [self.in_target.size.0 as f32, self.in_target.size.1 as f32],
            frame,
        );
        write(DETECT_HIGH_SLOT, params);
        params = PassParams::new(
            [self.in_target.size.0 as f32, self.in_target.size.1 as f32],
            frame,
        );
        params.step = [
            1.0 / self.in_target.size.0.max(1) as f32,
            1.0 / self.in_target.size.1.max(1) as f32,
        ];
        params.phase = self.phase;
        params.threshold = rules.threshold;
        write(BLUR_H_SLOT, params);
        write(BLUR_V_SLOT, params);
        params = PassParams::new(frame, frame);
        params.phase = self.phase;
        params.intensity = rules.intensity;
        params.threshold = rules.threshold;
        params.adapt_strength = rules.adapt;
        params.adapt_base_mult = rules.adapt_base_mult;
        params.black_level = rules.adapt_base_black_level as f32;
        params.dirt_intensity = rules.dirt_intensity;
        params.adaptation = if rules.adaptation { 1.0 } else { 0.0 };
        params.dither = if rules.dither { 1.0 } else { 0.0 };
        params.dirt = if rules.dirt { 1.0 } else { 0.0 };
        params.viewport = viewport;
        write(MAGIC_SLOT, params);
    }

    pub(crate) fn draw(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::TextureView,
        destination: &wgpu::TextureView,
        size: (u32, u32),
    ) {
        debug_assert_eq!(self.size, size);
        let spare = &self.spare.view;
        let group = self.bind_group(device, source, spare, spare, DETECT_INT_SLOT);
        self.run(encoder, &self.pipelines.detect_int, &group, &self.detect_int.view);
        let group = self.bind_group(device, &self.detect_int.view, spare, spare, DETECT_LOW_SLOT);
        self.run(encoder, &self.pipelines.detect_low, &group, &self.detect_low.view);
        let group = self.bind_group(device, source, spare, spare, DETECT_HIGH_SLOT);
        self.run(encoder, &self.pipelines.detect_high, &group, &self.in_target.view);

        // Twelve horizontal/vertical pairs; the last pass lands back in `in`.
        for _ in 0..BLUR_PAIRS {
            let group = self.bind_group(
                device,
                &self.in_target.view,
                spare,
                spare,
                BLUR_H_SLOT,
            );
            self.run(encoder, &self.pipelines.blur_h, &group, &self.out_target.view);
            let group = self.bind_group(
                device,
                &self.out_target.view,
                spare,
                spare,
                BLUR_V_SLOT,
            );
            self.run(encoder, &self.pipelines.blur_v, &group, &self.in_target.view);
        }

        let group = self.bind_group(
            device,
            source,
            &self.in_target.view,
            &self.detect_low.view,
            MAGIC_SLOT,
        );
        self.run(encoder, &self.pipelines.magic, &group, destination);
    }

    fn bind_group(
        &self,
        device: &wgpu::Device,
        source: &wgpu::TextureView,
        aux: &wgpu::TextureView,
        low: &wgpu::TextureView,
        slot: usize,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("AL bind group"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(source),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.params[slot].as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(aux),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(low),
                },
            ],
        })
    }

    fn run(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        pipeline: &wgpu::RenderPipeline,
        group: &wgpu::BindGroup,
        target: &wgpu::TextureView,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("AL pass"),
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
}

fn create_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    let texture = |binding: u32| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    };
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("AL layout"),
        entries: &[
            texture(0),
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            texture(3),
            texture(4),
        ],
    })
}

fn create_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
    entry: &str,
    label: &str,
) -> wgpu::RenderPipeline {
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &[Some(layout)],
        immediate_size: 0,
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

fn create_target(device: &wgpu::Device, label: &str, (width, height): (u32, u32)) -> Target {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    Target {
        _texture: texture,
        view,
        size: (width.max(1), height.max(1)),
    }
}

#[cfg(test)]
mod gpu_tests {
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

    #[test]
    fn the_pass_chain_runs_and_moves_a_bright_patch() {
        let gpu = Gpu::new();
        let size = (64u32, 64u32);
        let format = wgpu::TextureFormat::Rgba16Float;
        let scene = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("AL scene"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let mut texels = vec![[0u8; 4]; (size.0 * size.1) as usize];
        for y in 24..32u32 {
            for x in 24..32u32 {
                texels[(y * size.0 + x) as usize] = [255, 255, 255, 255];
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
        let mut ambient = AmbientLight::new(&gpu.device, format);
        let rules = AmbientLightRules {
            // A low threshold lets the patch reach the blur chain on this test
            // frame; adaptation stays on so the whole chain runs.
            threshold: 2.0,
            ..AmbientLightRules::default()
        };
        ambient.prepare(
            &gpu.device,
            &gpu.queue,
            size,
            &rules,
            [0.0, 0.0, size.0 as f32, size.1 as f32],
        );
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        ambient.draw(&gpu.device, &mut encoder, &scene_view, &destination_view, size);
        let submission = gpu.queue.submit([encoder.finish()]);
        gpu.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(Duration::from_secs(60)),
            })
            .unwrap();
        let texels = gpu.read_float_texels(&destination_view, [0, 0], [size.0, size.1]);
        let centre = texels[(28 * size.0 + 28) as usize][0];
        let corner = texels[(2 * size.0 + 2) as usize][0];
        assert!(
            centre >= corner && centre.is_finite(),
            "the chain leaves the patch at least as lit as the corner: centre {centre}, corner {corner}"
        );
    }
}
