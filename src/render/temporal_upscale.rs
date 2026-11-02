//! Temporal accumulation of the reduced world before its scaled copy.
//!
//! This is the approximate half of FSR2 the engine can feed: the world already
//! renders at `SuperResolution.RenderScale`, so the pass reprojects that frame
//! with the camera's integer-pixel delta, clamps the history to the current
//! neighbourhood, and lets the existing EASU/bicubic copy reconstruct it. It
//! trades FSR2's per-object motion vectors and depth disocclusion for a camera
//! delta, which is exact for the scroll/zoom the isometric view actually has.
//!
//! The result is a steadier low-resolution frame: edge shimmer settles while
//! the camera holds still, and a moving camera blends toward the current frame
//! instead of leaving a trail.
//!
//! Depends on: `render/gpu.rs` (through the caller) and wgpu only.

const SHADER_SRC: &str = include_str!("temporal_upscale.wgsl");

/// Source-pixel steps beyond this are a jump, not motion: the history restarts.
const RESET_STEP: f32 = 24.0;

pub(crate) struct TemporalUpscale {
    format: wgpu::TextureFormat,
    size: (u32, u32),
    /// Resolved history this frame samples.
    history: wgpu::Texture,
    history_view: wgpu::TextureView,
    /// Target the next resolve writes, swapped into `history` afterwards.
    next: wgpu::Texture,
    next_view: wgpu::TextureView,
    params: wgpu::Buffer,
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    last_camera: Option<[f32; 2]>,
}

impl TemporalUpscale {
    pub(crate) fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        size: (u32, u32),
    ) -> Self {
        let (history, history_view) = create_target(device, "Temporal history", format, size);
        let (next, next_view) = create_target(device, "Temporal next", format, size);
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Temporal params"),
            size: 32,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Temporal layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
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
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Temporal layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Temporal shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER_SRC.into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Temporal resolve"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_resolve"),
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
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Temporal sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            format,
            size,
            history,
            history_view,
            next,
            next_view,
            params,
            layout,
            pipeline,
            sampler,
            last_camera: None,
        }
    }

    pub(crate) fn size(&self) -> (u32, u32) {
        self.size
    }

    /// Follow a new source size; the history restarts at that size.
    pub(crate) fn resize(&mut self, device: &wgpu::Device, (width, height): (u32, u32)) {
        if (width, height) == self.size {
            return;
        }
        (self.history, self.history_view) =
            create_target(device, "Temporal history", self.format, (width, height));
        (self.next, self.next_view) =
            create_target(device, "Temporal next", self.format, (width, height));
        self.size = (width, height);
        self.last_camera = None;
    }

    /// Blend `source` into the history and return the texture holding the result.
    pub(crate) fn resolve<'a>(
        &'a mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        source_view: &wgpu::TextureView,
        camera: [f32; 2],
        render_scale: f32,
        alpha: f32,
    ) -> &'a wgpu::Texture {
        let (width, height) = self.size;
        let mut delta = [0.0f32; 2];
        let mut reset = 1.0f32;
        if let Some(previous) = self.last_camera {
            let step = [
                (camera[0] - previous[0]) * render_scale / width.max(1) as f32,
                (camera[1] - previous[1]) * render_scale / height.max(1) as f32,
            ];
            // A teleport is not motion: blending across it would smear the whole
            // field, so the history restarts as it does on the first frame.
            let jump = (step[0] * width as f32).abs() + (step[1] * height as f32).abs();
            if jump <= RESET_STEP {
                delta = step;
                reset = 0.0;
            }
        }
        queue.write_buffer(
            &self.params,
            0,
            bytemuck::cast_slice(&[
                width as f32,
                height as f32,
                alpha.clamp(0.01, 0.5),
                reset,
                delta[0],
                delta[1],
                0.0,
                0.0,
            ]),
        );
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Temporal bind group"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(source_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&self.history_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.params.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Temporal resolve"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.next_view,
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
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        self.last_camera = Some(camera);
        std::mem::swap(&mut self.history, &mut self.next);
        std::mem::swap(&mut self.history_view, &mut self.next_view);
        &self.history
    }
}

fn create_target(
    device: &wgpu::Device,
    label: &str,
    format: wgpu::TextureFormat,
    (width, height): (u32, u32),
) -> (wgpu::Texture, wgpu::TextureView) {
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
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    (texture, view)
}
