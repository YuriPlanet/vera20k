//! Phobos `SurfaceExt::BlurRect` (`Ext/Surface/Body.cpp:9`) as a GPU pass:
//! the tooltip background's rect blurred in place over the composition, the
//! three-box gaussian run as six sequential single-direction passes and the
//! finished blur composited back over the rect. The gate and size come from
//! `[Phobos] ToolTipBlur` plus the side/rules `ToolTip.Background.BlurSize`
//! (`Misc/PhobosToolTip.cpp:439`, hook `0x478FDC`); a zero size keeps native's
//! `CLOSE_ENOUGH` early return.
//!
//! GPU f32 accumulation replaces native's int sliding window; that
//! presentation-only rounding difference is not an executed GPU parity claim.

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    /// The sampled texture's pixel size.
    source_size: [f32; 2],
    /// Where the blurred rect starts inside the sampled texture (pixels).
    rect_origin: [f32; 2],
    /// The blurred rect's pixel size.
    rect_size: [f32; 2],
    /// This pass's box radius in pixels and its direction (0 x, 1 y).
    radius_direction: [f32; 2],
    /// The quad's clip-space window: the full target for a blur pass, the
    /// rect's window for the composite.
    clip_min: [f32; 2],
    clip_max: [f32; 2],
    /// x: 0 blur, 1 composite copy; y: direction.
    mode_direction: [f32; 2],
    _pad: [f32; 2],
}

const SHADER: &str = include_str!("tooltip_blur.wgsl");

/// Three boxes, horizontal then vertical each, over the ping-pong pair. The
/// sixth pass composites into the composition.
const PASS_DIRECTIONS: [f32; 6] = [0.0, 1.0, 0.0, 1.0, 0.0, 1.0];

pub(crate) struct TooltipBlurRenderer {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    params: wgpu::Buffer,
    format: wgpu::TextureFormat,
    ping: Option<(wgpu::Texture, wgpu::TextureView)>,
    pong: Option<(wgpu::Texture, wgpu::TextureView)>,
    extent: [u32; 2],
}

impl TooltipBlurRenderer {
    /// The composition texture is the format and size authority: the blur
    /// targets and the pipeline target match it.
    pub(crate) fn new(device: &wgpu::Device, composition: &wgpu::Texture) -> Self {
        let target_format = composition.format();

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Tooltip blur"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Tooltip blur clamp"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Tooltip blur layout"),
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
            ],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Tooltip blur pipeline"),
            layout: Some(
                &device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("Tooltip blur pipeline layout"),
                    bind_group_layouts: &[Some(&layout)],
                    immediate_size: 0,
                }),
            ),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Tooltip blur parameters"),
            size: std::mem::size_of::<Params>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            pipeline,
            layout,
            sampler,
            params,
            format: target_format,
            ping: None,
            pong: None,
            extent: [0, 0],
        }
    }

    /// Recreate the ping-pong targets when the rect resizes.
    fn ensure_targets(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        let target_format = self.format;
        if self.ping.as_ref().is_some_and(|(texture, _)| {
            texture.size().width == width && texture.size().height == height
        }) {
            return;
        }
        let extent = wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        };
        for (label, slot) in [("Tooltip blur A", 0), ("Tooltip blur B", 1)] {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: extent,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: target_format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            if slot == 0 {
                self.ping = Some((texture, view));
            } else {
                self.pong = Some((texture, view));
            }
        }
        self.extent = [extent.width, extent.height];
    }

    /// Blur `rect` (composition pixels) of the composition in place: six box
    /// passes through the ping-pong targets, then the finished blur
    /// composited back over the rect. `blur_size` is the native `float`
    /// whose three box radii follow `Ext/Surface/Body.cpp:36-44`.
    pub(crate) fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        composition_view: &wgpu::TextureView,
        composition_size: [f32; 2],
        rect: [f32; 4],
        blur_size: f32,
    ) {
        let width = (rect[2].max(1.0)) as u32;
        let height = (rect[3].max(1.0)) as u32;
        self.ensure_targets(device, width, height);
        let Some((_, view_a)) = self.ping.as_ref() else {
            return;
        };
        let Some((_, view_b)) = self.pong.as_ref() else {
            return;
        };
        // `wl`/`wu`/`m` per the native three-box derivation
        // (`Ext/Surface/Body.cpp:36-44`).
        let mut wl = (4.0 * blur_size * blur_size + 1.0).sqrt().floor() as i32;
        if wl % 2 == 0 {
            wl -= 1;
        }
        let wl = wl.max(1);
        let wu = wl + 2;
        let m = ((12.0 * blur_size * blur_size - 3.0 * (wl * wl) as f32 - 12.0 * wl as f32 - 9.0)
            / (-4.0 * wl as f32 - 4.0))
            .round() as i32;
        let radius = |box_index: usize| {
            let width = if (box_index as i32) < m { wl } else { wu };
            (((width - 1) / 2).max(0)) as f32
        };

        let full_clip = [[-1.0, -1.0], [1.0, 1.0]];
        let rect_clip = [
            [rect[0] / composition_size[0], rect[1] / composition_size[1]],
            [
                (rect[0] + rect[2]) / composition_size[0],
                (rect[1] + rect[3]) / composition_size[1],
            ],
        ];
        let rect_size = [rect[2], rect[3]];
        let internal_size = [width as f32, height as f32];
        // (source view, target view, source size, rect origin, clip, mode,
        //  radius, direction) — five blur passes ping-ponging between the
        // pair, then the composite into the composition.
        let passes = [
            (
                composition_view,
                view_a,
                composition_size,
                [rect[0], rect[1]],
                full_clip,
                0.0,
                radius(0),
                0.0,
            ),
            (
                view_a,
                view_b,
                internal_size,
                [0.0, 0.0],
                full_clip,
                0.0,
                radius(0),
                1.0,
            ),
            (
                view_b,
                view_a,
                internal_size,
                [0.0, 0.0],
                full_clip,
                0.0,
                radius(1),
                0.0,
            ),
            (
                view_a,
                view_b,
                internal_size,
                [0.0, 0.0],
                full_clip,
                0.0,
                radius(1),
                1.0,
            ),
            (
                view_b,
                view_a,
                internal_size,
                [0.0, 0.0],
                full_clip,
                0.0,
                radius(2),
                0.0,
            ),
            (
                view_a,
                composition_view,
                internal_size,
                [0.0, 0.0],
                rect_clip,
                1.0,
                radius(2),
                1.0,
            ),
        ];
        for (
            pass_index,
            (source_view, target_view, source_size, rect_origin, clip, mode, radius, direction),
        ) in passes.into_iter().enumerate()
        {
            let params_value = Params {
                source_size,
                rect_origin,
                rect_size,
                radius_direction: [radius, direction],
                clip_min: clip[0],
                clip_max: clip[1],
                mode_direction: [mode, direction],
                _pad: [0.0; 2],
            };
            queue.write_buffer(&self.params, 0, bytemuck::bytes_of(&params_value));
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Tooltip blur group"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(source_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: self.params.as_entire_binding(),
                    },
                ],
            });
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Tooltip blur pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.draw(0..6, 0..1);
            let _ = pass_index;
        }
    }

    /// The blur targets' extent, for the rect-clamp debug assertion.
    pub(crate) fn extent(&self) -> [u32; 2] {
        self.extent
    }
}
