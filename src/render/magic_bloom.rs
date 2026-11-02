//! MagicBloom (luluco250, MIT): eight single-pass 9x9 Gaussian levels, a
//! threshold applied on the first one, and an adapted Uncharted 2 tone-map
//! before the screen blend. The adaptation chain is the same shape ArcaneBloom
//! uses: a luminance pyramid built each frame and a 1x1 ping-pong value that
//! carries the smoothing across draws.
//!
//! Lens dirt is not ported (it needs a texture the game does not ship), and
//! the debug combo is not exposed as an INI key.
//!
//! Depends on: `render/gpu.rs` (through the caller) and wgpu only.

use crate::rules::post_process::MagicBloomRules;

const SHADER: &str = include_str!("magic_bloom.wgsl");

/// Eight levels; level `i` is the frame divided by `2^(i+1)`.
const LEVELS: usize = 8;
/// Blur step per level, in frame pixels.
const BLUR_STEPS: [f32; LEVELS] = [2.0, 4.0, 8.0, 8.0, 16.0, 32.0, 64.0, 128.0];
/// Luminance chain: 64x64 down to 1x1.
const LUMA_BASE: u32 = 64;
const LUMA_MIPS: usize = 7;

// One uniform buffer per pass, since every queue write lands before submission.
const LUMA_SLOT: usize = 0;
const LUMA_DOWN_SLOT: usize = LUMA_SLOT + 1;
const ADAPT_SLOT: usize = LUMA_DOWN_SLOT + LUMA_MIPS - 1;
const BLUR_SLOT: usize = ADAPT_SLOT + 1;
const BLEND_SLOT: usize = BLUR_SLOT + LEVELS;
const SLOTS: usize = BLEND_SLOT + 1;

/// The pass constants, mirrored by `Params` in the shader.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct PassParams {
    dst: [f32; 2],
    step: [f32; 2],
    frame: [f32; 2],
    pad: [f32; 2],
    scalar: [f32; 4],
    adapt: [f32; 4],
    viewport: [f32; 4],
}

impl PassParams {
    fn new(dst: [f32; 2], step: [f32; 2], frame: [f32; 2]) -> Self {
        Self {
            dst,
            step,
            frame,
            pad: [0.0; 2],
            scalar: [0.0; 4],
            adapt: [0.0; 4],
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
    luma: wgpu::RenderPipeline,
    luma_down: wgpu::RenderPipeline,
    adapt: wgpu::RenderPipeline,
    blur: wgpu::RenderPipeline,
    blend: wgpu::RenderPipeline,
}

pub(crate) struct MagicBloom {
    format: wgpu::TextureFormat,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    params: Vec<wgpu::Buffer>,
    pipelines: Pipelines,
    size: (u32, u32),
    levels: Vec<Target>,
    luma: Vec<Target>,
    adapt: [Target; 2],
    /// 1x1 stand-in for bindings a pass never reads.
    spare: Target,
    /// Which adaptation texture this frame writes; the other is last frame's.
    adapt_index: usize,
    /// The rules' `Adapt.Precision`, resolved into a luminance-chain level.
    precision: usize,
    /// Whether a frame has been drawn since the targets were (re)built; the
    /// first adaptation jumps straight to the measured value.
    started: bool,
}

impl MagicBloom {
    pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let layout = create_layout(device);
        // Every intermediate target is RGBA16F; only the blend writes the
        // chain's own (often sRGB) format, so the pipelines differ by target.
        let internal = wgpu::TextureFormat::Rgba16Float;
        let pipelines = Pipelines {
            luma: create_pipeline(device, &layout, internal, "fs_luma", "MagicBloom luma"),
            luma_down: create_pipeline(
                device,
                &layout,
                internal,
                "fs_luma_down",
                "MagicBloom luma down",
            ),
            adapt: create_pipeline(device, &layout, internal, "fs_adapt", "MagicBloom adapt"),
            blur: create_pipeline(device, &layout, internal, "fs_blur", "MagicBloom blur"),
            blend: create_pipeline(device, &layout, format, "fs_blend", "MagicBloom blend"),
        };
        let params = (0..SLOTS)
            .map(|slot| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(&format!("MagicBloom params {slot}")),
                    size: std::mem::size_of::<PassParams>() as u64,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                })
            })
            .collect();
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("MagicBloom sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let spare = create_target(device, "MagicBloom spare", (1, 1));
        let mut bloom = Self {
            format,
            layout,
            sampler,
            params,
            pipelines,
            size: (0, 0),
            levels: Vec::new(),
            luma: Vec::new(),
            adapt: [
                create_target(device, "MagicBloom adapt A", (1, 1)),
                create_target(device, "MagicBloom adapt B", (1, 1)),
            ],
            spare,
            adapt_index: 0,
            precision: 2,
            started: false,
        };
        bloom.ensure_targets(device, (1, 1));
        bloom
    }

    fn ensure_targets(&mut self, device: &wgpu::Device, size: (u32, u32)) {
        if size == self.size {
            return;
        }
        self.size = size;
        self.levels = (0..LEVELS)
            .map(|level| {
                let divisor = 1u32 << (level + 1);
                let level_size = ((size.0 / divisor).max(1), (size.1 / divisor).max(1));
                create_target(device, "MagicBloom level", level_size)
            })
            .collect();
        self.luma = (0..LUMA_MIPS)
            .map(|mip| {
                let width = (LUMA_BASE >> mip).max(1);
                create_target(device, "MagicBloom luma", (width, width))
            })
            .collect();
        self.adapt_index = 0;
        self.started = false;
    }

    /// Write every pass's uniforms for this frame.
    pub(crate) fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        size: (u32, u32),
        rules: &MagicBloomRules,
        viewport: [f32; 4],
    ) {
        self.ensure_targets(device, size);
        self.precision = (rules.adapt_precision as usize).min(LUMA_MIPS - 1);
        let frame = [size.0 as f32, size.1 as f32];
        let write = |slot: usize, params: PassParams| {
            queue.write_buffer(&self.params[slot], 0, bytemuck::bytes_of(&params));
        };
        let luma = &self.luma[0];
        let mut params = PassParams::new([luma.size.0 as f32, luma.size.1 as f32], [0.0; 2], frame);
        write(LUMA_SLOT, params);
        for mip in 1..LUMA_MIPS {
            let target = &self.luma[mip];
            params = PassParams::new(
                [target.size.0 as f32, target.size.1 as f32],
                [0.0; 2],
                frame,
            );
            write(LUMA_DOWN_SLOT + mip - 1, params);
        }
        params = PassParams::new([1.0, 1.0], [0.0; 2], frame);
        params.adapt = [
            rules.adapt_sensitivity,
            if self.started {
                rules.adapt_speed.clamp(0.0, 1.0)
            } else {
                1.0
            },
            rules.adapt_limit[0],
            rules.adapt_limit[1],
        ];
        write(ADAPT_SLOT, params);
        for level in 0..LEVELS {
            let target = &self.levels[level];
            params = PassParams::new(
                [target.size.0 as f32, target.size.1 as f32],
                [
                    BLUR_STEPS[level] / frame[0].max(1.0),
                    BLUR_STEPS[level] / frame[1].max(1.0),
                ],
                frame,
            );
            params.scalar = [
                rules.threshold,
                rules.intensity,
                if level == 0 { 1.0 } else { 0.0 },
                0.0,
            ];
            write(BLUR_SLOT + level, params);
        }
        params = PassParams::new(frame, [0.0; 2], frame);
        params.pad = [rules.exposure, 0.0];
        params.viewport = viewport;
        write(BLEND_SLOT, params);
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
        let level_views: Vec<&wgpu::TextureView> =
            self.levels.iter().map(|level| &level.view).collect();
        // Luminance chain: frame -> 64x64, then 2x2 boxes down to 1x1.
        let group = self.bind_group(device, source, &self.spare.view, &[], LUMA_SLOT);
        self.run(encoder, &self.pipelines.luma, &group, &self.luma[0].view);
        for mip in 1..LUMA_MIPS {
            let slot = LUMA_DOWN_SLOT + mip - 1;
            let group =
                self.bind_group(device, &self.luma[mip - 1].view, &self.spare.view, &[], slot);
            self.run(
                encoder,
                &self.pipelines.luma_down,
                &group,
                &self.luma[mip].view,
            );
        }
        // Adaptation: the level the precision names, smoothed against last frame.
        let source_luma = &self.luma[self.precision_level()].view;
        let last = 1 - self.adapt_index;
        let group = self.bind_group(device, source_luma, &self.adapt[last].view, &[], ADAPT_SLOT);
        self.run(
            encoder,
            &self.pipelines.adapt,
            &group,
            &self.adapt[self.adapt_index].view,
        );
        // Eight Gaussian levels; the first reads the graded frame and thresholds.
        for level in 0..LEVELS {
            let input = if level == 0 {
                source
            } else {
                &self.levels[level - 1].view
            };
            let slot = BLUR_SLOT + level;
            let group = self.bind_group(device, input, &self.spare.view, &[], slot);
            self.run(
                encoder,
                &self.pipelines.blur,
                &group,
                &self.levels[level].view,
            );
        }
        let group = self.bind_group(
            device,
            source,
            &self.adapt[self.adapt_index].view,
            &level_views,
            BLEND_SLOT,
        );
        self.run(encoder, &self.pipelines.blend, &group, destination);
        self.adapt_index = 1 - self.adapt_index;
        self.started = true;
    }

    /// The `Adapt.Precision` knob, translated into a luminance-chain level:
    /// 0 is the fully averaged 1x1, higher picks a finer level.
    fn precision_level(&self) -> usize {
        (LUMA_MIPS - 1).saturating_sub(self.precision)
    }

    fn bind_group(
        &self,
        device: &wgpu::Device,
        source: &wgpu::TextureView,
        aux: &wgpu::TextureView,
        levels: &[&wgpu::TextureView],
        slot: usize,
    ) -> wgpu::BindGroup {
        let mut entries = Vec::with_capacity(12);
        entries.push(wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(source),
        });
        entries.push(wgpu::BindGroupEntry {
            binding: 1,
            resource: wgpu::BindingResource::Sampler(&self.sampler),
        });
        entries.push(wgpu::BindGroupEntry {
            binding: 2,
            resource: self.params[slot].as_entire_binding(),
        });
        entries.push(wgpu::BindGroupEntry {
            binding: 3,
            resource: wgpu::BindingResource::TextureView(aux),
        });
        for level in 0..LEVELS {
            let view = levels.get(level).copied().unwrap_or(&self.spare.view);
            entries.push(wgpu::BindGroupEntry {
                binding: 4 + level as u32,
                resource: wgpu::BindingResource::TextureView(view),
            });
        }
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("MagicBloom bind group"),
            layout: &self.layout,
            entries: &entries,
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
            label: Some("MagicBloom pass"),
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
    let mut entries = vec![
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
    ];
    for level in 0..LEVELS {
        entries.push(texture(4 + level as u32));
    }
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("MagicBloom layout"),
        entries: &entries,
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
    fn a_bright_patch_spreads_a_halo() {
        let gpu = Gpu::new();
        let size = (64u32, 64u32);
        let format = wgpu::TextureFormat::Rgba16Float;
        let scene = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("MagicBloom scene"),
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
        let mut bloom = MagicBloom::new(&gpu.device, format);
        let rules = MagicBloomRules {
            // No threshold so the patch's energy reaches the levels cleanly, and
            // an instant adaptation so one frame is the steady state.
            threshold: 1.0,
            adapt_speed: 1.0,
            ..MagicBloomRules::default()
        };
        bloom.prepare(
            &gpu.device,
            &gpu.queue,
            size,
            &rules,
            [0.0, 0.0, size.0 as f32, size.1 as f32],
        );
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        bloom.draw(&gpu.device, &mut encoder, &scene_view, &destination_view, size);
        let submission = gpu.queue.submit([encoder.finish()]);
        gpu.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(Duration::from_secs(60)),
            })
            .unwrap();
        let texels = gpu.read_float_texels(&destination_view, [0, 0], [size.0, size.1]);
        let centre = texels[(28 * size.0 + 28) as usize][0];
        let adjacent = texels[(28 * size.0 + 40) as usize][0];
        let corner = texels[(2 * size.0 + 2) as usize][0];
        assert!(
            centre > 0.4 && adjacent > corner + 0.01,
            "the halo reaches past the patch: centre {centre}, next {adjacent}, corner {corner}"
        );
    }
}
