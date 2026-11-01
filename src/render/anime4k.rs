//! Anime4K: the CNN upscaler from TC2's `A4K_*.hlsl` effect pack.
//!
//! The reduced battlefield render (`SuperResolution.RenderScale=0.5`) is a
//! quarter of the frame's pixels, and the wheel-zoom copy reconstructs it. This
//! module runs the network before that copy when `SuperResolution.Anime4K`
//! names a tier, so the reconstruction is learned rather than bicubic.
//!
//! The network is a chain of convolution passes at the reduced resolution plus
//! one depth-to-space pass that writes the 2x2 block over a bilinear residual.
//! Layers live in one of three RGBA16F texture arrays: a pass reads the array
//! its predecessor wrote and writes the next one, so no pass samples a texture
//! it renders into. `network` supplies the weights and wiring parsed from the
//! effect files.

mod network;

use crate::rules::post_process::Anime4kTier;
use wgpu::util::DeviceExt;

const CONV_SRC: &str = include_str!("anime4k.wgsl");
const STAGE_SRC: &str = include_str!("anime4k_stage.wgsl");
const LAYER_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// One tier's pipelines and textures, sized for one reduced render.
pub(crate) struct Anime4K {
    /// Texture views keep the layers alive; the arrays stay beside them for
    /// the same reason `scaled_copy` keeps its depth target.
    _arrays: [wgpu::Texture; 3],
    /// Per-layer 2D views, used as render attachments.
    layers: [Vec<wgpu::TextureView>; 3],
    output: wgpu::Texture,
    output_view: wgpu::TextureView,
    stage_layout: wgpu::BindGroupLayout,
    stage_pipeline: wgpu::RenderPipeline,
    /// Indexed by output count (1..=3).
    conv_pipelines: Vec<wgpu::RenderPipeline>,
    upsample_pipeline: wgpu::RenderPipeline,
    passes: Vec<PassGpu>,
}

struct PassGpu {
    bind_group: wgpu::BindGroup,
    outputs: Vec<usize>,
    upsample: bool,
}

/// The pass constants, mirrored by `Params` in `anime4k.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct PassParams {
    size: [u32; 2],
    in_count: u32,
    taps: u32,
    linear: u32,
    bias_base: u32,
    targets: u32,
    _pad: u32,
    /// `(array << 16) | layer` per input group.
    layers: [u32; 16],
}

impl Anime4K {
    /// Build the network for `size`, the reduced world size. `None` when the
    /// tier's effect file cannot be read, so the caller can keep the bicubic.
    pub(crate) fn new(device: &wgpu::Device, tier: Anime4kTier, size: [u32; 2]) -> Option<Self> {
        let network = match network::parse(tier) {
            Ok(network) => network,
            Err(error) => {
                log::warn!("[Anime4K] {}: {error}", tier.label());
                return None;
            }
        };
        let (width, height) = (size[0].max(1), size[1].max(1));
        let layer_count = network.layers as u32;
        log::info!(
            "[Anime4K] {} network, {} passes, {} layers at {}x{}",
            tier.label(),
            network.passes.len(),
            network.layers,
            width,
            height
        );
        let arrays = [
            create_layer_array(device, "Anime4K layers A", width, height, layer_count),
            create_layer_array(device, "Anime4K layers B", width, height, layer_count),
            create_layer_array(device, "Anime4K layers C", width, height, layer_count),
        ];
        let layers = [
            create_layer_views(&arrays[0], layer_count),
            create_layer_views(&arrays[1], layer_count),
            create_layer_views(&arrays[2], layer_count),
        ];
        // The network's last pass writes the 2x frame; the zoom copy then
        // blits it one to one into the frame rect.
        let output = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Anime4K output"),
            size: wgpu::Extent3d {
                width: width * 2,
                height: height * 2,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: LAYER_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let output_view = output.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Anime4K bilinear"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let layout = create_layout(device);
        let stage_layout = create_stage_layout(device);
        let stage_pipeline = create_pipeline(
            device,
            &create_pipeline_layout(device, &stage_layout, "Anime4K input"),
            &device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Anime4K input"),
                source: wgpu::ShaderSource::Wgsl(STAGE_SRC.into()),
            }),
            ("fs_main", "vs_main"),
            1,
            LAYER_FORMAT,
        );
        let conv_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Anime4K convolutions"),
            source: wgpu::ShaderSource::Wgsl(CONV_SRC.into()),
        });
        let pipeline_layout = create_pipeline_layout(device, &layout, "Anime4K");
        let conv_pipelines = ["fs_conv1", "fs_conv2", "fs_conv3"]
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                create_pipeline(
                    device,
                    &pipeline_layout,
                    &conv_module,
                    (entry, "vs_main"),
                    index + 1,
                    LAYER_FORMAT,
                )
            })
            .collect();
        let upsample_pipeline = create_pipeline(
            device,
            &pipeline_layout,
            &conv_module,
            ("fs_final", "vs_main"),
            1,
            LAYER_FORMAT,
        );

        // Which pass last wrote each layer. A pass reads its predecessor's
        // array, but the final pass reads skip-connection layers from both, so
        // each input's array follows the pass that wrote it.
        let mut writer = vec![usize::MAX; network.layers];
        for (index, pass) in network.passes.iter().enumerate() {
            for &layer in &pass.outputs {
                writer[layer] = index;
            }
        }
        let array_views = [
            create_array_view(&arrays[0]),
            create_array_view(&arrays[1]),
            create_array_view(&arrays[2]),
        ];
        // One texel standing in for bind-group slots a pass never samples.
        let spare_texture = create_layer_array(device, "Anime4K spare", 1, 1, 1);
        let spare = create_array_view(&spare_texture);
        let mut passes = Vec::with_capacity(network.passes.len());
        for (index, pass) in network.passes.iter().enumerate() {
            let mut slots = [0u32; 16];
            let mut reads = [false; 3];
            for (slot, &layer) in pass.inputs.iter().enumerate() {
                // Passes cycle through three arrays, so a pass never samples
                // the texture it renders into: it reads the array its
                // predecessor wrote and writes the next one.
                let array = if pass.upsample {
                    match writer[layer] {
                        usize::MAX => 0,
                        written => (written + 1) % 3,
                    }
                } else {
                    index % 3
                };
                slots[slot] = ((array as u32) << 16) | layer as u32;
                reads[array] = true;
            }
            let params = PassParams {
                size: [width, height],
                in_count: pass.inputs.len() as u32,
                taps: pass.taps,
                linear: if pass.linear { 1 } else { 0 },
                bias_base: pass.weights.len() as u32,
                targets: pass.targets,
                _pad: 0,
                layers: slots,
            };
            let params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Anime4K pass"),
                contents: bytemuck::bytes_of(&params),
                usage: wgpu::BufferUsages::STORAGE,
            });
            let mut weights = pass.weights.clone();
            weights.extend_from_slice(&pass.bias);
            let weights_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Anime4K weights"),
                contents: bytemuck::cast_slice(&weights),
                usage: wgpu::BufferUsages::STORAGE,
            });
            // A texture cannot be sampled and rendered into in one pass, so
            // the slot for the array the pass writes holds the spare layer
            // instead of that array. The shader only follows `layers`, which
            // never names it.
            let views = [
                if reads[0] { &array_views[0] } else { &spare },
                if reads[1] { &array_views[1] } else { &spare },
                if reads[2] { &array_views[2] } else { &spare },
            ];
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Anime4K pass"),
                layout: &layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(views[0]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(views[1]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(views[2]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: params_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: weights_buffer.as_entire_binding(),
                    },
                ],
            });
            passes.push(PassGpu {
                bind_group,
                outputs: pass.outputs.clone(),
                upsample: pass.upsample,
            });
        }
        Some(Self {
            _arrays: arrays,
            layers,
            output,
            output_view,
            stage_layout,
            stage_pipeline,
            conv_pipelines,
            upsample_pipeline,
            passes,
        })
    }

    /// The 2x reconstruction of the last [`Self::run`].
    pub(crate) fn output_texture(&self) -> &wgpu::Texture {
        &self.output
    }

    /// Run the network from `source` (the reduced world colour).
    pub(crate) fn run(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::Texture,
    ) {
        let input_view = source.create_view(&wgpu::TextureViewDescriptor::default());
        let input_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Anime4K input"),
            layout: &self.stage_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&input_view),
            }],
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Anime4K input"),
                color_attachments: &[Some(attachment(&self.layers[0][0]))],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.stage_pipeline);
            pass.set_bind_group(0, &input_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        for (index, step) in self.passes.iter().enumerate() {
            if step.upsample {
                continue;
            }
            let target = &self.layers[(index + 1) % 3];
            let outputs = step.outputs.len();
            let mut buffers = [None, None, None];
            for (slot, &layer) in step.outputs.iter().enumerate() {
                buffers[slot] = Some(attachment(&target[layer]));
            }
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Anime4K convolution"),
                color_attachments: &buffers[..outputs],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.conv_pipelines[outputs - 1]);
            pass.set_bind_group(0, &step.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        if let Some(step) = self.passes.last() {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Anime4K upsample"),
                color_attachments: &[Some(attachment(&self.output_view))],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.upsample_pipeline);
            pass.set_bind_group(0, &step.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}

fn attachment(view: &wgpu::TextureView) -> wgpu::RenderPassColorAttachment<'_> {
    wgpu::RenderPassColorAttachment {
        view,
        resolve_target: None,
        depth_slice: None,
        ops: wgpu::Operations {
            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            store: wgpu::StoreOp::Store,
        },
    }
}

fn create_layer_array(
    device: &wgpu::Device,
    label: &'static str,
    width: u32,
    height: u32,
    layers: u32,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: layers,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: LAYER_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}

fn create_layer_views(texture: &wgpu::Texture, layers: u32) -> Vec<wgpu::TextureView> {
    (0..layers)
        .map(|layer| {
            texture.create_view(&wgpu::TextureViewDescriptor {
                label: Some("Anime4K layer"),
                dimension: Some(wgpu::TextureViewDimension::D2),
                base_array_layer: layer,
                array_layer_count: Some(1),
                ..Default::default()
            })
        })
        .collect()
}

fn create_array_view(texture: &wgpu::Texture) -> wgpu::TextureView {
    texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some("Anime4K array"),
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    })
}

fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2Array,
            multisampled: false,
        },
        count: None,
    }
}

fn storage_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn create_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Anime4K"),
        entries: &[
            texture_entry(0),
            texture_entry(1),
            texture_entry(2),
            wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
            storage_entry(4),
            storage_entry(5),
        ],
    })
}

fn create_stage_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Anime4K input"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        }],
    })
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
    entries: (&str, &str),
    outputs: usize,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let targets: Vec<Option<wgpu::ColorTargetState>> = (0..outputs)
        .map(|_| {
            Some(wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })
        })
        .collect();
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(entries.0),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some(entries.1),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(entries.0),
            targets: &targets,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::terrain_draw_gpu_tests::Gpu;
    use std::time::Duration;

    #[test]
    fn the_convolution_shaders_compile() {
        for source in [CONV_SRC, STAGE_SRC] {
            let module = naga::front::wgsl::parse_str(source).expect("the shader parses");
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::empty(),
            )
            .validate(&module)
            .expect("the shader validates");
        }
    }

    /// Two flat halves with one vertical edge, written as sRGB bytes.
    fn split_source(gpu: &Gpu, size: [u32; 2]) -> wgpu::Texture {
        let texture = gpu.target(size, wgpu::TextureFormat::Rgba8UnormSrgb);
        let mut bytes = Vec::with_capacity((size[0] * size[1] * 4) as usize);
        for _ in 0..size[1] {
            for x in 0..size[0] {
                let value = if x < size[0] / 2 { 96u8 } else { 224u8 };
                bytes.extend_from_slice(&[value, value, value, 255]);
            }
        }
        gpu.queue.write_texture(
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

    fn run(gpu: &Gpu, network: &Anime4K, source: &wgpu::Texture) {
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        network.run(&gpu.device, &mut encoder, source);
        let submission = gpu.queue.submit([encoder.finish()]);
        gpu.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(Duration::from_secs(60)),
            })
            .unwrap();
    }

    #[test]
    fn every_tier_builds_and_upsamples_by_two() {
        let gpu = Gpu::new();
        let size = [8u32, 8u32];
        let source = split_source(&gpu, size);
        for tier in [Anime4kTier::S, Anime4kTier::L, Anime4kTier::Vl, Anime4kTier::Ul] {
            let network = Anime4K::new(&gpu.device, tier, size).expect("the tier builds");
            run(&gpu, &network, &source);
            assert_eq!(network.output_texture().width(), 16, "{} width", tier.label());
            assert_eq!(network.output_texture().height(), 16, "{} height", tier.label());
        }
    }

    #[test]
    fn the_zoom_pass_shows_the_reconstructed_frame() {
        let gpu = Gpu::new();
        let size = [16u32, 12u32];
        let source = split_source(&gpu, size);
        let zoom = crate::render::upscale_pass::WorldZoomPass::new(
            &gpu.device,
            &gpu.queue,
            wgpu::TextureFormat::Bgra8UnormSrgb,
            [32, 24],
            2.0,
            false,
            Anime4kTier::S,
        );
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        zoom.composite(
            &gpu.device,
            &mut encoder,
            &source,
            [0, 0, 32, 24],
            wgpu::Color::BLACK,
        );
        let submission = gpu.queue.submit([encoder.finish()]);
        gpu.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(Duration::from_secs(60)),
            })
            .unwrap();
        let texels = gpu.read_float_texels(zoom.color_view(), [0, 0], [32, 24]);
        let row = 12;
        let dark = texels[row * 32 + 3][0];
        let bright = texels[row * 32 + 28][0];
        assert!(dark < 0.45, "the zoomed dark side: {dark}");
        assert!(bright > 0.55, "the zoomed bright side: {bright}");
    }

    #[test]
    fn the_small_tier_keeps_the_flat_sides_of_an_edge() {
        let gpu = Gpu::new();
        let size = [16u32, 12u32];
        let source = split_source(&gpu, size);
        let network = Anime4K::new(&gpu.device, Anime4kTier::S, size).expect("the S tier builds");
        run(&gpu, &network, &source);
        let view = network
            .output_texture()
            .create_view(&wgpu::TextureViewDescriptor::default());
        let texels = gpu.read_float_texels(&view, [0, 0], [32, 24]);
        let row = 12;
        let dark = texels[row * 32 + 3][0];
        let bright = texels[row * 32 + 28][0];
        // A learned edge reconstruction keeps flat colours: the sRGB bytes
        // decode to about 0.117 and 0.745. Wider drift would mean a weighting
        // or tap mistake, not a reconstruction choice.
        assert!((dark - 0.117).abs() < 0.06, "the dark side drifts: {dark}");
        assert!(
            (bright - 0.745).abs() < 0.06,
            "the bright side drifts: {bright}"
        );
    }
}
