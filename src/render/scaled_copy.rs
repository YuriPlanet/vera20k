//! A surface drawn at its own size and shown scaled: resolution and wheel
//! zoom (`upscale_pass`) and the sidebar's size (`sidebar_surface`).
//!
//! The surface has its own colour and depth targets, so the batch pipelines
//! draw into it as they draw into the frame. Its copy (`scaled_copy.wgsl`)
//! puts source pixel (0, 0) at an origin in the target, each source pixel
//! covering `scale` target pixels: a whole scale copies blocks and stays
//! sharp, and a step between blends the two source pixels a target pixel
//! straddles by how much of it each covers, so only the seams soften.
//! Shrinking integrates all covered source texels after scene composition,
//! retaining thin details without filtering palette indices or depth values.
//! [`MaskedCopy`] draws the same copy into the window over a zoomed frame.

const SHADER_SRC: &str = include_str!("scaled_copy.wgsl");

/// The copy's source; `webgl` for the WebGL2 fallback. GLSL has no texelFetch
/// for depth samplers, so the fallback loads the mask as an unfilterable float
/// texture.
pub(crate) fn shader_source(webgl: bool) -> String {
    if !webgl {
        return SHADER_SRC.to_owned();
    }
    let source = super::webgl::patch(
        SHADER_SRC,
        "var t_mask: texture_depth_2d;",
        "var t_mask: texture_2d<f32>;",
    );
    super::webgl::patch(
        &source,
        "textureLoad(t_mask, clamp(at, vec2i(0), last), 0) < 1.0",
        "textureLoad(t_mask, clamp(at, vec2i(0), last), 0).r < 1.0",
    )
}

/// A copy's placement: source pixel (0, 0) at target pixel `origin`, each
/// source pixel covering `scale` target pixels; `sharp` for a whole scale.
fn create_placement_buffer(device: &wgpu::Device, label: &str) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: 16, // vec2f origin + f32 scale + f32 sharp flag
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn write_placement(
    queue: &wgpu::Queue,
    buffer: &wgpu::Buffer,
    origin: (f32, f32),
    scale: f32,
    sharp: bool,
) {
    queue.write_buffer(
        buffer,
        0,
        bytemuck::cast_slice(&[origin.0, origin.1, scale, if sharp { 1.0f32 } else { 0.0 }]),
    );
}

/// A source surface and the draw that shows it scaled.
pub(crate) struct ScaledSurface {
    label: &'static str,
    format: wgpu::TextureFormat,
    size: (u32, u32),
    color_texture: wgpu::Texture,
    color_view: wgpu::TextureView,
    // Kept with its view for GPU resource ownership.
    _depth_texture: wgpu::Texture,
    depth_view: wgpu::TextureView,
    params_buffer: wgpu::Buffer,
    bgl: wgpu::BindGroupLayout,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
}

impl ScaledSurface {
    /// A `size` surface in `format`. The copy is drawn into a `format` target
    /// in a pass whose depth attachment, if any, has `target_depth`; the copy
    /// neither tests nor writes it.
    pub(crate) fn new(
        device: &wgpu::Device,
        label: &'static str,
        format: wgpu::TextureFormat,
        size: (u32, u32),
        target_depth: Option<wgpu::TextureFormat>,
    ) -> Self {
        let (color_texture, color_view) = create_color_target(device, label, format, size);
        let (depth_texture, depth_view) = create_depth_target(device, label, size);
        let params_buffer = create_placement_buffer(device, label);
        let bgl = create_bgl(device, label, false);
        let bind_group = create_bind_group(device, label, &bgl, &color_texture, &params_buffer);
        let pipeline = create_pipeline(device, label, format, &bgl, target_depth, "fs_main");
        Self {
            label,
            format,
            size,
            color_texture,
            color_view,
            _depth_texture: depth_texture,
            depth_view,
            params_buffer,
            bgl,
            bind_group,
            pipeline,
        }
    }

    /// Follow a new size. Only the size-dependent targets are rebuilt, so a
    /// live resize never recompiles the shader.
    pub(crate) fn resize(&mut self, device: &wgpu::Device, size: (u32, u32)) {
        if size == self.size {
            return;
        }
        (self.color_texture, self.color_view) =
            create_color_target(device, self.label, self.format, size);
        (self._depth_texture, self.depth_view) = create_depth_target(device, self.label, size);
        self.bind_group = create_bind_group(
            device,
            self.label,
            &self.bgl,
            &self.color_texture,
            &self.params_buffer,
        );
        self.size = size;
    }

    /// Put source pixel (0, 0) at target pixel `origin`, each source pixel
    /// covering `scale` target pixels; `sharp` for a whole scale.
    pub(crate) fn place(&self, queue: &wgpu::Queue, origin: (f32, f32), scale: f32, sharp: bool) {
        write_placement(queue, &self.params_buffer, origin, scale, sharp);
    }

    pub(crate) fn color_view(&self) -> &wgpu::TextureView {
        &self.color_view
    }

    pub(crate) fn color_texture(&self) -> &wgpu::Texture {
        &self.color_texture
    }

    pub(crate) fn depth_view(&self) -> &wgpu::TextureView {
        &self.depth_view
    }

    /// The copy's bind group for another `source` of the same size, at the
    /// same placement.
    pub(crate) fn bind_group_for(
        &self,
        device: &wgpu::Device,
        source: &wgpu::Texture,
    ) -> wgpu::BindGroup {
        create_bind_group(device, self.label, &self.bgl, source, &self.params_buffer)
    }

    /// Draw the copy into `pass` over `scissor` (x, y, width, height) of its
    /// target, from the surface or from `source` (`bind_group_for`). The
    /// scissor stays set.
    pub(crate) fn draw(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        source: Option<&wgpu::BindGroup>,
        scissor: [u32; 4],
    ) {
        let [x, y, width, height] = scissor;
        if width == 0 || height == 0 {
            return;
        }
        pass.set_scissor_rect(x, y, width, height);
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, source.unwrap_or(&self.bind_group), &[]);
        pass.draw(0..3, 0..1);
    }
}

/// [`ScaledSurface`]s' copy into the window over a zoomed frame
/// (`app::presentation::sidebar_surface`), at the window's own pixels. It
/// keeps off every window pixel over a frame pixel that UI drawn after the
/// surfaces' frame copy covers: that UI (tooltips, messages, the cursor)
/// writes the frame's depth, so the zoomed frame shows it there as before.
pub(crate) struct MaskedCopy {
    label: &'static str,
    bgl: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    /// The frame's placement in the window.
    mask_buffer: wgpu::Buffer,
}

impl MaskedCopy {
    /// The copy into a `format` target in a pass without a depth attachment.
    pub(crate) fn new(
        device: &wgpu::Device,
        label: &'static str,
        format: wgpu::TextureFormat,
    ) -> Self {
        let bgl = create_bgl(device, label, true);
        let pipeline = create_pipeline(device, label, format, &bgl, None, "fs_masked");
        Self {
            label,
            bgl,
            pipeline,
            mask_buffer: create_placement_buffer(device, label),
        }
    }

    /// A buffer for one surface's placement in the window.
    pub(crate) fn placement_buffer(&self, device: &wgpu::Device) -> wgpu::Buffer {
        create_placement_buffer(device, self.label)
    }

    /// Put a surface's pixel (0, 0) at window pixel `origin` in its
    /// `placement`, each covering `scale` window pixels; `sharp` for a whole
    /// scale.
    pub(crate) fn place(
        &self,
        queue: &wgpu::Queue,
        placement: &wgpu::Buffer,
        origin: (f32, f32),
        scale: f32,
        sharp: bool,
    ) {
        write_placement(queue, placement, origin, scale, sharp);
    }

    /// Put frame pixel (0, 0) at window pixel `origin`, each frame pixel
    /// covering `scale` window pixels, at least one.
    pub(crate) fn place_frame(&self, queue: &wgpu::Queue, origin: (f32, f32), scale: f32) {
        write_placement(queue, &self.mask_buffer, origin, scale, false);
    }

    /// Draw `surface` at `placement` into `pass` over `scissor` (x, y,
    /// width, height) of the window, off the pixels `frame_depth` marks.
    pub(crate) fn draw(
        &self,
        device: &wgpu::Device,
        pass: &mut wgpu::RenderPass<'_>,
        surface: &ScaledSurface,
        placement: &wgpu::Buffer,
        frame_depth: &wgpu::TextureView,
        scissor: [u32; 4],
    ) {
        let [x, y, width, height] = scissor;
        if width == 0 || height == 0 {
            return;
        }
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(self.label),
            layout: &self.bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(surface.color_view()),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: placement.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(frame_depth),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: self.mask_buffer.as_entire_binding(),
                },
            ],
        });
        pass.set_scissor_rect(x, y, width, height);
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

fn create_color_target(
    device: &wgpu::Device,
    label: &str,
    format: wgpu::TextureFormat,
    (width, height): (u32, u32),
) -> (wgpu::Texture, wgpu::TextureView) {
    let encoded_view_formats = [format.remove_srgb_suffix()];
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(&format!("{label} Color RT")),
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
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST,
        // Destination-dependent native drawing snapshots encoded channel
        // bytes through the compatible linear view, before RGB565 packing.
        view_formats: if format.is_srgb() {
            super::webgl::view_formats(device, &encoded_view_formats)
        } else {
            &[]
        },
    });
    let view = texture.create_view(&Default::default());
    (texture, view)
}

pub(crate) fn create_depth_target(
    device: &wgpu::Device,
    label: &str,
    (width, height): (u32, u32),
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(&format!("{label} Depth RT")),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    (texture, view)
}

/// The copy's bindings; `masked` adds the frame's depth and placement.
fn create_bgl(device: &wgpu::Device, label: &str, masked: bool) -> wgpu::BindGroupLayout {
    let texture = |binding, sample_type| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type,
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    };
    let uniform = |binding| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    };
    // Whole texels are loaded, never filtered.
    let mut entries = vec![
        texture(0, wgpu::TextureSampleType::Float { filterable: false }),
        uniform(1),
    ];
    if masked {
        entries.extend([
            texture(
                2,
                if super::gpu::webgl_fallback(device) {
                    wgpu::TextureSampleType::Float { filterable: false }
                } else {
                    wgpu::TextureSampleType::Depth
                },
            ),
            uniform(3),
        ]);
    }
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(label),
        entries: &entries,
    })
}

fn create_bind_group(
    device: &wgpu::Device,
    label: &str,
    bgl: &wgpu::BindGroupLayout,
    source: &wgpu::Texture,
    params_buffer: &wgpu::Buffer,
) -> wgpu::BindGroup {
    let view = source.create_view(&Default::default());
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout: bgl,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: params_buffer.as_entire_binding(),
            },
        ],
    })
}

fn create_pipeline(
    device: &wgpu::Device,
    label: &str,
    format: wgpu::TextureFormat,
    bgl: &wgpu::BindGroupLayout,
    target_depth: Option<wgpu::TextureFormat>,
    fragment: &str,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(shader_source(super::gpu::webgl_fallback(device)).into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &[Some(bgl)],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some(fragment),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: None, // opaque overwrite
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            ..Default::default()
        },
        depth_stencil: target_depth.map(|format| wgpu::DepthStencilState {
            format,
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::Always),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaled_copy_shader_compiles() {
        use wgpu::naga;
        for source in [shader_source(false), shader_source(true)] {
            let module =
                naga::front::wgsl::parse_str(&source).expect("the scaled-copy shader parses");
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::empty(),
            )
            .validate(&module)
            .expect("the scaled-copy shader validates");
        }
    }
}
