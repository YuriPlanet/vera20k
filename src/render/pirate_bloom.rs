//! `Pirate_Bloom.fx` (Pirate / Daodan), ported from the ReShade effect.
//!
//! The frame is drawn into two quarter-resolution RGBA8 targets that ping-pong:
//! the first horizontal pass subtracts `Threshold` from every tap, and each
//! following pass box-blurs the previous result (horizontal then vertical) with
//! the effect's 12 taps. `BLOOM_PASSES = 2` means two horizontal/vertical pass
//! pairs, which is what `Pirate_Bloom.cfg` ships. The final pass mixes the
//! blurred copy back over the frame with one of the effect's five blend modes.
//!
//! Texels outside the tactical viewport keep the frame untouched, so the
//! sidebar and bottom strip never glow.

use crate::rules::post_process::PirateBloomRules;
use wgpu::util::DeviceExt;

const SHADER: &str = include_str!("pirate_bloom.wgsl");

/// `Pirate_Bloom.cfg`: two pass pairs, quarter-size targets, 12 taps.
const PASSES: usize = 2;
const TEXTURE_DIVISOR: u32 = 4;
const TAPS: f32 = 12.0;

/// One pass's uniform; both shader entries share this layout.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct PassParams {
    step: [f32; 2],
    dst_size: [f32; 2],
    filt: [f32; 4],
    blend: [f32; 4],
    viewport: [f32; 4],
}

/// One render target kept with its view for GPU resource ownership.
struct Target {
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
}

/// One frame's bloom resources, sized for the graded frame.
pub(crate) struct PirateBloom {
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// One uniform per pass: the queue writes all land before the submitted
    /// command buffer, so a shared buffer would show every pass the last write.
    params: Vec<wgpu::Buffer>,
    blur_pipeline: wgpu::RenderPipeline,
    combine_pipeline: wgpu::RenderPipeline,
    horizontal: Target,
    vertical: Target,
    /// A one-texel stand-in for passes that never read `t_bloom`.
    spare: Target,
    frame: (u32, u32),
    rules: PirateBloomRules,
    viewport: [f32; 4],
}

/// Slot order: the four blur passes, then the composite.
const SLOTS: usize = PASSES * 2 + 1;

impl PirateBloom {
    pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let layout = create_layout(device);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("PirateBloom"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let params = (0..SLOTS)
            .map(|_| {
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("PirateBloom pass"),
                    contents: bytemuck::bytes_of(&PassParams {
                        step: [0.0; 2],
                        dst_size: [1.0; 2],
                        filt: [0.0; 4],
                        blend: [0.0; 4],
                        viewport: [0.0; 4],
                    }),
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                })
            })
            .collect::<Vec<_>>();
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("PirateBloom"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("PirateBloom"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let blur_pipeline = create_pipeline(
            device,
            &pipeline_layout,
            &shader,
            "fs_blur",
            wgpu::TextureFormat::Rgba8Unorm,
        );
        let combine_pipeline =
            create_pipeline(device, &pipeline_layout, &shader, "fs_combine", format);
        // Placeholders; `prepare` replaces them at the first frame size.
        let horizontal = render_target(device, "PirateBloom horizontal", (1, 1));
        let vertical = render_target(device, "PirateBloom vertical", (1, 1));
        let spare = render_target(device, "PirateBloom spare", (1, 1));
        Self {
            layout,
            sampler,
            params,
            blur_pipeline,
            combine_pipeline,
            horizontal,
            vertical,
            spare,
            frame: (0, 0),
            rules: PirateBloomRules::default(),
            viewport: [0.0; 4],
        }
    }

    /// Follow the frame size and the live rules before this frame is encoded.
    pub(crate) fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        size: (u32, u32),
        rules: PirateBloomRules,
        viewport: [f32; 4],
    ) {
        self.rules = rules;
        self.viewport = viewport;
        let size = (size.0.max(1), size.1.max(1));
        if size != self.frame {
            let half = (
                (size.0 / TEXTURE_DIVISOR).max(1),
                (size.1 / TEXTURE_DIVISOR).max(1),
            );
            self.horizontal = render_target(device, "PirateBloom horizontal", half);
            self.vertical = render_target(device, "PirateBloom vertical", half);
            self.frame = size;
        }
        self.write_params(queue);
    }

    fn write_params(&self, queue: &wgpu::Queue) {
        let frame = [self.frame.0 as f32, self.frame.1 as f32];
        let size = [
            self.horizontal._texture.width() as f32,
            self.horizontal._texture.height() as f32,
        ];
        let threshold = self.rules.threshold.max(0.0);
        let taps = TAPS * 2.0 - 1.0;
        // The first pass divides by `(1 - threshold) * taps`, the later ones
        // only by `taps`.
        let first_norm = 1.0 / (1.0 - threshold).max(1e-4) / taps;
        let later_norm = 1.0 / taps;
        let step_x = self.rules.radius.max(0.0) / frame[0];
        let step_y = self.rules.radius.max(0.0) / frame[1];
        let blend = [
            self.rules.strength,
            self.rules.saturation,
            self.rules.blend as f32,
            0.0,
        ];
        let write = |slot: usize, step: [f32; 2], filt: [f32; 4], blend: [f32; 4]| {
            queue.write_buffer(
                &self.params[slot],
                0,
                bytemuck::bytes_of(&PassParams {
                    step,
                    dst_size: size,
                    filt,
                    blend,
                    viewport: self.viewport,
                }),
            );
        };
        write(0, [step_x, 0.0], [threshold, first_norm, 0.0, 0.0], blend);
        write(1, [0.0, step_y], [0.0, later_norm, 0.0, 0.0], blend);
        write(2, [step_x, 0.0], [0.0, later_norm, 0.0, 0.0], blend);
        write(3, [0.0, step_y], [0.0, later_norm, 0.0, 0.0], blend);
        // The composite reads the full-frame source; its target is the frame.
        write(4, [0.0, 0.0], [0.0; 4], blend);
    }

    /// Run the chain from `scene` into `destination`.
    pub(crate) fn draw(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        scene: &wgpu::TextureView,
        destination: &wgpu::TextureView,
    ) {
        let spare = &self.spare.view;
        let passes: [(usize, &wgpu::TextureView, &Target); 4] = [
            (0, scene, &self.horizontal),
            (1, &self.horizontal.view, &self.vertical),
            (2, &self.vertical.view, &self.horizontal),
            (3, &self.horizontal.view, &self.vertical),
        ];
        for (slot, source, target) in passes {
            let group = bind_group(
                device,
                &self.layout,
                source,
                &self.params[slot],
                spare,
                &self.sampler,
            );
            run(
                encoder,
                &self.blur_pipeline,
                &group,
                &target.view,
                "PirateBloom blur",
            );
        }
        let group = bind_group(
            device,
            &self.layout,
            scene,
            &self.params[SLOTS - 1],
            &self.vertical.view,
            &self.sampler,
        );
        run(
            encoder,
            &self.combine_pipeline,
            &group,
            destination,
            "PirateBloom combine",
        );
    }
}

fn render_target(device: &wgpu::Device, label: &'static str, size: (u32, u32)) -> Target {
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

fn create_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("PirateBloom"),
        entries: &[
            texture_entry(0),
            uniform_entry(1),
            sampler_entry(2),
            texture_entry(3),
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
    bloom: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("PirateBloom pass"),
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
                resource: wgpu::BindingResource::TextureView(bloom),
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

    #[test]
    fn a_bright_patch_glows_next_to_it() {
        let gpu = Gpu::new();
        let size = (128u32, 128u32);
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let scene = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("PirateBloom scene"),
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
        for y in 60..68u32 {
            for x in 60..68u32 {
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
        let mut bloom = PirateBloom::new(&gpu.device, format);
        bloom.prepare(
            &gpu.device,
            &gpu.queue,
            (size.0, size.1),
            PirateBloomRules {
                threshold: 0.5,
                strength: 1.0,
                radius: 0.8,
                saturation: 1.0,
                blend: 0,
            },
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
        let centre = texels[(64 * size.0 + 64) as usize][0];
        let adjacent = texels[(64 * size.0 + 72) as usize][0];
        let corner = texels[(4 * size.0 + 4) as usize][0];
        assert!(
            centre > 0.9 && adjacent > 0.05 && adjacent > corner + 0.02,
            "the halo reaches past the patch: centre {centre}, next {adjacent}, corner {corner}"
        );
    }
}
