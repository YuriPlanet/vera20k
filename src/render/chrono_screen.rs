//! ChronoScreenEffect53BBA0/53C0A0: retained scene, table-driven radial
//! rotation, truncated/clamped nearest samples and integer RGB565 whitening.
//! Native scalar controls: tools/chrono_screen_effect.json/draw_controls.
//! GPU f32 intermediates replace native x87 expression temporaries; this
//! presentation-only rounding difference is not an executed GPU parity claim.

use crate::render::gpu::GpuContext;
use wgpu::util::DeviceExt;

#[derive(Debug, Clone, Copy)]
pub(crate) enum ChronoScreenDraw {
    Warp { step: i32, duration: i32 },
    White,
    Restore,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    width: u32,
    height: u32,
    mode: u32,
    weight: u32,
    amplitude: f32,
    _pad: f32,
    white_width: u32,
    white_height: u32,
    origin_x: u32,
    origin_y: u32,
    _pad2: [u32; 2],
}

pub(crate) struct ChronoScreenRenderer {
    snapshot: wgpu::Texture,
    group: wgpu::BindGroup,
    params: wgpu::Buffer,
    pipeline: wgpu::RenderPipeline,
    extent: wgpu::Extent3d,
    encoded_format: wgpu::TextureFormat,
    origin: [u32; 2],
}

impl ChronoScreenRenderer {
    pub(crate) fn new(gpu: &GpuContext, source: &wgpu::Texture, tactical: [u32; 4]) -> Self {
        //VideoMode560FCB ->533FD0 ->53417B constructs Composite88731C:
        //tactical width, FULL visible height. Sidebar goes directly to Primary.
        let extent = wgpu::Extent3d {
            width: tactical[2].max(1),
            height: source.height(),
            depth_or_array_layers: 1,
        };
        let format = source.format();
        let encoded_format = format.remove_srgb_suffix();
        let snapshot = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Chrono native retained scene"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[encoded_format],
        });
        let view = snapshot.create_view(&wgpu::TextureViewDescriptor {
            format: Some(encoded_format),
            ..Default::default()
        });
        let params = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Chrono native draw parameters"),
            size: std::mem::size_of::<Params>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // One immutable retail table owner; do not regenerate sin/cos.
        let trig = gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Chrono shared retail trig table"),
                contents: crate::util::native_trig::RETAIL_SINE_TABLE,
                usage: wgpu::BufferUsages::STORAGE,
            });
        let layout = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Chrono screen layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: false },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });
        let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Chrono screen group"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: params.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: trig.as_entire_binding(),
                },
            ],
        });
        let shader = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Chrono native screen shader"),
                source: wgpu::ShaderSource::Wgsl(include_str!("chrono_screen.wgsl").into()),
            });
        let pipeline_layout = gpu
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Chrono screen pipeline layout"),
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            });
        let pipeline = gpu
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Chrono screen pipeline"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vertex"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fragment"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: encoded_format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            });
        Self {
            snapshot,
            group,
            params,
            pipeline,
            extent,
            encoded_format,
            origin: [tactical[0], 0],
        }
    }

    pub(crate) fn matches(&self, source: &wgpu::Texture, tactical: [u32; 4]) -> bool {
        self.extent.width == tactical[2]
            && self.extent.height == source.height()
            && self.origin[0] == tactical[0]
            && self.encoded_format == source.format().remove_srgb_suffix()
    }

    pub(crate) fn capture(&self, encoder: &mut wgpu::CommandEncoder, source: &wgpu::Texture) {
        let mut input = source.as_image_copy();
        input.origin.x = self.origin[0];
        encoder.copy_texture_to_texture(input, self.snapshot.as_image_copy(), self.extent);
    }

    pub(crate) fn draw(
        &self,
        gpu: &GpuContext,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::Texture,
        draw: ChronoScreenDraw,
        tactical: [u32; 4],
    ) {
        let (mode, weight, amplitude) = match draw {
            ChronoScreenDraw::Warp { step, duration } => {
                let amplitude = draw_amplitude(step);
                (
                    0,
                    step.wrapping_mul(255).wrapping_div(duration) as u32,
                    amplitude,
                )
            }
            ChronoScreenDraw::White => (1, 0, 0.0),
            ChronoScreenDraw::Restore => (2, 0, 0.0),
        };
        gpu.queue.write_buffer(
            &self.params,
            0,
            bytemuck::bytes_of(&Params {
                width: self.extent.width,
                height: self.extent.height,
                mode,
                weight,
                amplitude,
                _pad: 0.0,
                white_width: tactical[2],
                white_height: tactical[3],
                origin_x: self.origin[0],
                origin_y: self.origin[1],
                _pad2: [0; 2],
            }),
        );
        let view = target.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.encoded_format),
            ..Default::default()
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Chrono screen original surface stores"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.group, &[]);
        pass.draw(0..3, 0..1);
    }
}

fn draw_amplitude(step: i32) -> f32 {
    let mut velocity = 0.0_f64;
    let mut amplitude = 0.0_f64;
    //53BC85..53BC9A retains BOTH accumulators until the loop finishes.
    //7EC008 is a double .002, not an f32 step. Host f64 add substitutes
    //native PC53/chop intermediates; final FSTP uses the existing chop owner.
    //Any remaining subpixel index difference is presentation-only.
    for _ in 0..step.max(0) {
        velocity += f64::from_bits(0x3F60_624D_D2F1_A9FC);
        amplitude += velocity;
    }
    use crate::util::native_x87::{NativeF64Bits, X87Chop53};
    let value = X87Chop53::load_f64(NativeF64Bits::from_bits(amplitude.to_bits()))
        .expect("finite chrono amplitude");
    f32::from_bits(X87Chop53::store_f32_masked_chop(value).bits())
}
