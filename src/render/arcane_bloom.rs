//! ArcaneBloom, ported from luluco250's ReShade `ArcaneBloom.fx`.
//!
//! The effect lifts the graded frame into an HDR-ish range (inverse Reinhard to
//! `MaxBrightness`), builds five Gaussian levels (each box-downsampled to half
//! the previous size and blurred with the effect's 13-tap kernel), and mixes
//! their weighted sum back at the composite step. An optional adaptation
//! exposure scales the whole frame by the scene's average luminance, smoothed
//! over `Adapt.Time` so it follows rather than flashes.
//!
//! The average lives in an 8-level luminance chain (128x128 down to 1x1) built
//! each frame; the adaptation itself is a 1x1 texture ping-ponged between frames
//! so the smoothing survives across draws. UI texels outside the tactical
//! viewport keep the graded frame, so chrome neither glows nor re-exposes.

use crate::rules::post_process::ArcaneBloomRules;
use std::time::Instant;
use wgpu::util::DeviceExt;

const SHADER: &str = include_str!("arcane_bloom.wgsl");
const BLEND_SHADER: &str = include_str!("arcane_bloom_blend.wgsl");

/// The effect's five Gaussian levels; level `i` is the frame divided by
/// `2^(i+1)`.
const LEVELS: usize = 5;
/// Luminance chain base size and mip count (128x128 down to 1x1).
const LUMA_SIZE: u32 = 128;
const LUMA_MIPS: u32 = 8;
/// Blur step per level, in frame pixels (`DEF_BLUR_SHADER` divisors).
const BLUR_STEPS: [f32; LEVELS] = [1.0, 4.0, 8.0, 32.0, 96.0];
/// Box-downsample radius per level, in frame pixels (`DEF_DOWN_SHADER`).
const BOX_STEPS: [f32; LEVELS] = [4.0, 8.0, 16.0, 32.0, 64.0];

// Uniform slots: one buffer per pass, because the queue writes all land before
// the single submitted command buffer.
const HDR_SLOT: usize = 0;
const LUMA_SLOT: usize = 1;
const LUMA_DOWN_SLOT: usize = 2;
const ADAPT_SLOT: usize = LUMA_DOWN_SLOT + (LUMA_MIPS as usize - 1);
const BOX_SLOT: usize = ADAPT_SLOT + 1;
const BLUR_SLOT: usize = BOX_SLOT + LEVELS;
const BLEND_SLOT: usize = BLUR_SLOT + LEVELS * 4;
const SLOTS: usize = BLEND_SLOT + 1;

/// The pass constants, mirrored by `Params` in both shaders.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct PassParams {
    dst: [f32; 2],
    step: [f32; 2],
    frame: [f32; 2],
    _pad: [f32; 2],
    /// intensity, max brightness, exposure, adaptation intensity.
    scalar: [f32; 4],
    /// sensitivity, adaptation seconds, frame seconds, luminance mip.
    adapt: [f32; 4],
    /// limit min, limit max, use limits, unused.
    limits: [f32; 4],
    viewport: [f32; 4],
}

impl PassParams {
    fn new(dst: [f32; 2], step: [f32; 2], frame: [f32; 2]) -> Self {
        Self {
            dst,
            step,
            frame,
            _pad: [0.0; 2],
            scalar: [0.0; 4],
            adapt: [0.0; 4],
            limits: [0.0; 4],
            viewport: [0.0; 4],
        }
    }
}

/// One render target kept with its view for GPU resource ownership.
struct Target {
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
}

/// One Gaussian level: the box result (`main`) and the blur ping-pong `alt`.
struct Level {
    size: (u32, u32),
    main: Target,
    alt: Target,
}

struct Luma {
    _texture: wgpu::Texture,
    /// All mips, for the adaptation's level sample.
    view: wgpu::TextureView,
    /// One single-mip view per level, for rendering.
    mips: Vec<wgpu::TextureView>,
}

struct Pipelines {
    hdr: wgpu::RenderPipeline,
    luma: wgpu::RenderPipeline,
    luma_down: wgpu::RenderPipeline,
    adapt: wgpu::RenderPipeline,
    box_down: wgpu::RenderPipeline,
    blur: wgpu::RenderPipeline,
    blend: wgpu::RenderPipeline,
}

/// One frame's ArcaneBloom resources, sized for the graded frame.
pub(crate) struct ArcaneBloom {
    layout: wgpu::BindGroupLayout,
    blend_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    params: Vec<wgpu::Buffer>,
    pipelines: Pipelines,
    hdr: Target,
    hdr_size: (u32, u32),
    levels: Vec<Level>,
    luma: Luma,
    adapt: [Target; 2],
    /// A one-texel stand-in for passes that never read `aux`.
    spare: Target,
    /// Which adaptation texture this frame writes; the other is last frame's.
    parity: usize,
    size: (u32, u32),
    rules: ArcaneBloomRules,
    viewport: [f32; 4],
    last_draw: Option<Instant>,
}

impl ArcaneBloom {
    pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let layout = create_layout(device);
        let blend_layout = create_blend_layout(device);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("ArcaneBloom"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let params = (0..SLOTS)
            .map(|_| {
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("ArcaneBloom pass"),
                    contents: bytemuck::bytes_of(&PassParams::new([1.0; 2], [0.0; 2], [1.0; 2])),
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                })
            })
            .collect::<Vec<_>>();
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ArcaneBloom"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let blend_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ArcaneBloom blend"),
            source: wgpu::ShaderSource::Wgsl(BLEND_SHADER.into()),
        });
        let pass_layout = create_pipeline_layout(device, &layout, "ArcaneBloom");
        let blend_pipeline_layout = create_pipeline_layout(device, &blend_layout, "ArcaneBloom");
        let hdr_format = wgpu::TextureFormat::Rgba16Float;
        let luma_format = wgpu::TextureFormat::R16Float;
        let pipelines = Pipelines {
            hdr: create_pipeline(device, &pass_layout, &shader, "fs_hdr", hdr_format),
            luma: create_pipeline(device, &pass_layout, &shader, "fs_luma", luma_format),
            luma_down: create_pipeline(device, &pass_layout, &shader, "fs_luma_down", luma_format),
            adapt: create_pipeline(device, &pass_layout, &shader, "fs_adapt", luma_format),
            box_down: create_pipeline(device, &pass_layout, &shader, "fs_box", hdr_format),
            blur: create_pipeline(device, &pass_layout, &shader, "fs_blur", hdr_format),
            blend: create_pipeline(device, &blend_pipeline_layout, &blend_shader, "fs_blend", format),
        };
        // Placeholder targets; `prepare` replaces them at the first frame size.
        let (hdr, hdr_size, levels, luma, adapt, spare) = create_targets(device, (1, 1));
        Self {
            layout,
            blend_layout,
            sampler,
            params,
            pipelines,
            hdr,
            hdr_size,
            levels,
            luma,
            adapt,
            spare,
            parity: 0,
            size: (0, 0),
            rules: ArcaneBloomRules::default(),
            viewport: [0.0, 0.0, 0.0, 0.0],
            last_draw: None,
        }
    }

    /// Follow the frame size and the live rules before this frame is encoded.
    pub(crate) fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        size: (u32, u32),
        rules: ArcaneBloomRules,
        viewport: [f32; 4],
    ) {
        self.rules = rules;
        self.viewport = viewport;
        let size = (size.0.max(1), size.1.max(1));
        if size != self.size {
            let (hdr, hdr_size, levels, luma, adapt, spare) = create_targets(device, size);
            self.hdr = hdr;
            self.hdr_size = hdr_size;
            self.levels = levels;
            self.luma = luma;
            self.adapt = adapt;
            self.spare = spare;
            self.size = size;
            self.last_draw = None;
        }
        let now = Instant::now();
        // The first frame (or the first after a resize) snaps to the scene's
        // average instead of fading in from a stale value.
        let seconds = match self.last_draw {
            Some(last) => (now - last).as_secs_f32().clamp(0.001, 0.25),
            None => rules.adapt_time.max(0.001),
        };
        self.last_draw = Some(now);
        self.write_params(queue, seconds);
    }

    fn write_params(&self, queue: &wgpu::Queue, seconds: f32) {
        let frame = [self.size.0 as f32, self.size.1 as f32];
        let scalars = [
            self.rules.intensity,
            self.rules.max_brightness.max(1.0),
            self.rules.exposure,
            self.rules.adapt_intensity,
        ];
        let write = |slot: usize, mut params: PassParams| {
            params.scalar = scalars;
            params.viewport = self.viewport;
            queue.write_buffer(&self.params[slot], 0, bytemuck::bytes_of(&params));
        };
        write(
            HDR_SLOT,
            PassParams::new(
                [self.hdr_size.0 as f32, self.hdr_size.1 as f32],
                [0.0; 2],
                frame,
            ),
        );
        write(
            LUMA_SLOT,
            PassParams::new([LUMA_SIZE as f32; 2], [0.0; 2], frame),
        );
        for index in 0..LUMA_MIPS as usize - 1 {
            let (wide, tall) = luma_mip_size(index as u32 + 1);
            write(
                LUMA_DOWN_SLOT + index,
                PassParams::new([wide as f32, tall as f32], [0.0; 2], frame),
            );
        }
        let mut adapt = PassParams::new([1.0; 2], [0.0; 2], frame);
        adapt.adapt = [
            self.rules.sensitivity,
            self.rules.adapt_time.max(0.0001),
            seconds,
            (LUMA_MIPS - 1).saturating_sub(self.rules.precision) as f32,
        ];
        adapt.limits = [
            self.rules.limit_min.min(self.rules.limit_max),
            self.rules.limit_max.max(self.rules.limit_min),
            if self.rules.limits { 1.0 } else { 0.0 },
            0.0,
        ];
        write(ADAPT_SLOT, adapt);
        for (index, level) in self.levels.iter().enumerate() {
            let target = [level.size.0 as f32, level.size.1 as f32];
            write(
                BOX_SLOT + index,
                PassParams::new(
                    target,
                    [BOX_STEPS[index] / frame[0], BOX_STEPS[index] / frame[1]],
                    frame,
                ),
            );
            for pass in 0..4 {
                let axis = if pass % 2 == 0 {
                    [BLUR_STEPS[index] / frame[0], 0.0]
                } else {
                    [0.0, BLUR_STEPS[index] / frame[1]]
                };
                write(
                    BLUR_SLOT + index * 4 + pass,
                    PassParams::new(target, axis, frame),
                );
            }
        }
        write(BLEND_SLOT, PassParams::new(frame, [0.0; 2], frame));
    }

    /// Run the chain from the graded `scene` into `destination`.
    pub(crate) fn draw(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        scene: &wgpu::TextureView,
        destination: &wgpu::TextureView,
    ) {
        let write = self.parity;
        let read = 1 - write;
        let aux = &self.spare.view;
        let group = bind_group(
            device,
            &self.layout,
            scene,
            &self.params[HDR_SLOT],
            aux,
            &self.sampler,
        );
        run(
            encoder,
            &self.pipelines.hdr,
            &group,
            &self.hdr.view,
            "ArcaneBloom HDR",
        );
        // Luminance chain: the average the adaptation reads.
        let group = bind_group(
            device,
            &self.layout,
            &self.hdr.view,
            &self.params[LUMA_SLOT],
            aux,
            &self.sampler,
        );
        run(
            encoder,
            &self.pipelines.luma,
            &group,
            &self.luma.mips[0],
            "ArcaneBloom luma",
        );
        for index in 1..LUMA_MIPS as usize {
            let group = bind_group(
                device,
                &self.layout,
                &self.luma.mips[index - 1],
                &self.params[LUMA_DOWN_SLOT + index - 1],
                aux,
                &self.sampler,
            );
            run(
                encoder,
                &self.pipelines.luma_down,
                &group,
                &self.luma.mips[index],
                "ArcaneBloom luma down",
            );
        }
        let group = bind_group(
            device,
            &self.layout,
            &self.luma.view,
            &self.params[ADAPT_SLOT],
            &self.adapt[read].view,
            &self.sampler,
        );
        run(
            encoder,
            &self.pipelines.adapt,
            &group,
            &self.adapt[write].view,
            "ArcaneBloom adapt",
        );
        self.parity = read;
        // Five levels: box downsample, then the effect's four Gaussian passes
        // (X, Y, X, Y) so the level ends up in `main`.
        for index in 0..LEVELS {
            let source = if index == 0 {
                &self.hdr.view
            } else {
                &self.levels[index - 1].main.view
            };
            let group = bind_group(
                device,
                &self.layout,
                source,
                &self.params[BOX_SLOT + index],
                aux,
                &self.sampler,
            );
            run(
                encoder,
                &self.pipelines.box_down,
                &group,
                &self.levels[index].main.view,
                "ArcaneBloom box",
            );
            for pass in 0..4 {
                let group = bind_group(
                    device,
                    &self.layout,
                    if pass % 2 == 0 {
                        &self.levels[index].main.view
                    } else {
                        &self.levels[index].alt.view
                    },
                    &self.params[BLUR_SLOT + index * 4 + pass],
                    aux,
                    &self.sampler,
                );
                run(
                    encoder,
                    &self.pipelines.blur,
                    &group,
                    if pass % 2 == 0 {
                        &self.levels[index].alt.view
                    } else {
                        &self.levels[index].main.view
                    },
                    "ArcaneBloom blur",
                );
            }
        }
        let group = blend_group(
            device,
            &self.blend_layout,
            scene,
            &self.levels,
            &self.adapt[write].view,
            &self.params[BLEND_SLOT],
            &self.sampler,
        );
        run(
            encoder,
            &self.pipelines.blend,
            &group,
            destination,
            "ArcaneBloom blend",
        );
    }
}

fn luma_mip_size(level: u32) -> (u32, u32) {
    let size = (LUMA_SIZE >> level).max(1);
    (size, size)
}

fn create_targets(
    device: &wgpu::Device,
    size: (u32, u32),
) -> (Target, (u32, u32), Vec<Level>, Luma, [Target; 2], Target) {
    let half = ((size.0 >> 1).max(1), (size.1 >> 1).max(1));
    let hdr = render_target(device, "ArcaneBloom HDR", half, wgpu::TextureFormat::Rgba16Float);
    let levels = (0..LEVELS)
        .map(|index| {
            let divisor = 1 << (index + 1);
            let level = ((size.0 / divisor).max(1), (size.1 / divisor).max(1));
            Level {
                size: level,
                main: render_target(device, "ArcaneBloom level", level, wgpu::TextureFormat::Rgba16Float),
                alt: render_target(device, "ArcaneBloom alt", level, wgpu::TextureFormat::Rgba16Float),
            }
        })
        .collect();
    let luma_texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("ArcaneBloom luma"),
        size: wgpu::Extent3d {
            width: LUMA_SIZE,
            height: LUMA_SIZE,
            depth_or_array_layers: 1,
        },
        mip_level_count: LUMA_MIPS,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R16Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let luma_view = luma_texture.create_view(&wgpu::TextureViewDescriptor::default());
    let luma_mips = (0..LUMA_MIPS)
        .map(|level| {
            luma_texture.create_view(&wgpu::TextureViewDescriptor {
                label: Some("ArcaneBloom luma mip"),
                base_mip_level: level,
                mip_level_count: Some(1),
                ..Default::default()
            })
        })
        .collect();
    let luma = Luma {
        _texture: luma_texture,
        view: luma_view,
        mips: luma_mips,
    };
    let adapt = [
        render_target(device, "ArcaneBloom adapt", (1, 1), wgpu::TextureFormat::R16Float),
        render_target(device, "ArcaneBloom adapt", (1, 1), wgpu::TextureFormat::R16Float),
    ];
    let spare = render_target(
        device,
        "ArcaneBloom spare",
        (1, 1),
        wgpu::TextureFormat::R16Float,
    );
    (hdr, half, levels, luma, adapt, spare)
}

fn render_target(
    device: &wgpu::Device,
    label: &'static str,
    size: (u32, u32),
    format: wgpu::TextureFormat,
) -> Target {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size.0.max(1),
            height: size.1.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    Target {
        _texture: texture,
        view,
    }
}

fn create_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("ArcaneBloom"),
        entries: &[
            texture_entry(0),
            uniform_entry(1),
            sampler_entry(2),
            texture_entry(3),
        ],
    })
}

fn create_blend_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("ArcaneBloom blend"),
        entries: &[
            texture_entry(0),
            texture_entry(1),
            texture_entry(2),
            texture_entry(3),
            texture_entry(4),
            texture_entry(5),
            texture_entry(6),
            uniform_entry(7),
            sampler_entry(8),
        ],
    })
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

fn create_pipeline_layout(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    label: &str,
) -> wgpu::PipelineLayout {
    device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &[Some(layout)],
        immediate_size: 0,
    })
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
    aux: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("ArcaneBloom pass"),
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
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(aux),
            },
        ],
    })
}

#[allow(clippy::too_many_arguments)]
fn blend_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    scene: &wgpu::TextureView,
    levels: &[Level],
    adapt: &wgpu::TextureView,
    params: &wgpu::Buffer,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("ArcaneBloom blend"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(scene),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&levels[0].main.view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&levels[1].main.view),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(&levels[2].main.view),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::TextureView(&levels[3].main.view),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: wgpu::BindingResource::TextureView(&levels[4].main.view),
            },
            wgpu::BindGroupEntry {
                binding: 6,
                resource: wgpu::BindingResource::TextureView(adapt),
            },
            wgpu::BindGroupEntry {
                binding: 7,
                resource: params.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 8,
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
    fn the_shaders_compile() {
        for source in [SHADER, BLEND_SHADER] {
            let module = naga::front::wgsl::parse_str(source).expect("the shader parses");
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::empty(),
            )
            .validate(&module)
            .expect("the shader validates");
        }
    }

    #[test]
    fn a_bright_patch_spreads_a_halo() {
        let gpu = Gpu::new();
        let size = (32u32, 32u32);
        let format = wgpu::TextureFormat::Rgba16Float;
        // The scene is an sRGB frame like the graded target, so the patch is an
        // 8-bit white and the rest is black.
        let scene = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ArcaneBloom scene"),
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
        for y in 12..20u32 {
            for x in 12..20u32 {
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
        let mut bloom = ArcaneBloom::new(&gpu.device, format);
        let rules = ArcaneBloomRules {
            intensity: 3.0,
            // No adaptation, so only the Gaussian levels move the frame.
            adapt_intensity: 0.0,
            ..ArcaneBloomRules::default()
        };
        bloom.prepare(
            &gpu.device,
            &gpu.queue,
            (size.0, size.1),
            rules,
            [0.0, 0.0, size.0 as f32, size.1 as f32],
        );
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        bloom.draw(&gpu.device, &mut encoder, &scene_view, &destination_view);
        let submission = gpu.queue.submit([encoder.finish()]);
        gpu.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(Duration::from_secs(60)),
            })
            .unwrap();
        let texels = gpu.read_float_texels(&destination_view, [0, 0], [size.0, size.1]);
        let centre = texels[(16 * size.0 + 16) as usize][0];
        let adjacent = texels[(16 * size.0 + 22) as usize][0];
        let corner = texels[(2 * size.0 + 2) as usize][0];
        assert!(
            rival(centre, adjacent, corner),
            "the halo reaches next to the patch: centre {centre}, next {adjacent}, corner {corner}"
        );
    }

    /// The patch stays bright, its neighbourhood glows, and the corner does
    /// not.
    fn rival(centre: f32, adjacent: f32, corner: f32) -> bool {
        centre > 0.5 && adjacent > corner + 0.02
    }
}
