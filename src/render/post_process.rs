//! Full-frame post-processing at the presentation boundary.
//!
//! The composed match frame already lives in an offscreen texture
//! (`RendererState::game_frame_texture`) and reaches the window through a 1:1
//! `copy_texture_to_texture`. This module replaces that copy with a shader
//! chain when effects are configured, and keeps the copy when none are.
//!
//! ## Identity
//! The chain must be invisible when it has nothing to do: with every effect
//! disabled the fragment shader returns the `textureLoad` texel before any
//! sampling, filtering or colour conversion, so it reproduces the copy byte for
//! byte. `post_process_tests` compares the two paths on the production surface
//! format; the shader also has a CPU-side parse test so the checked-in WGSL is
//! validated without a GPU.
//!
//! ## Colour space
//! The frame target carries the surface's sRGB suffix, so `textureLoad` decodes
//! to linear and the target re-encodes on write. Grading therefore works in
//! linear RGB; contrast pivots on linear 0.5, which is perceptually darker than
//! a display-referred 0.5.
//!
//! Depends on: `render/gpu.rs` (through the caller) and wgpu only.

const SHADER_SRC: &str = include_str!("post_process.wgsl");

/// Uniform payload for [`PostChain`]; two vec4s keep std140 alignment trivial.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct PostParams {
    brightness: f32,
    contrast: f32,
    saturation: f32,
    /// 1 runs the colour-grade arm; 0 leaves the texel untouched.
    color_grade: f32,
    /// `(x, y, width, height)` of the tactical viewport in frame pixels. Every
    /// texel outside it — the sidebar column and the bottom strip — returns
    /// unchanged, so UI keeps its retail colours.
    viewport: [f32; 4],
}

impl PostParams {
    /// The byte-identical pass: every effect disabled.
    pub(crate) fn identity() -> Self {
        Self {
            brightness: 0.0,
            contrast: 1.0,
            saturation: 1.0,
            color_grade: 0.0,
            viewport: [0.0; 4],
        }
    }

    /// Build the uniform from the configured effects; `None` means the chain
    /// has nothing to do and the caller should keep the texture copy.
    pub(crate) fn from_effects(
        color_grade: Option<[f32; 3]>,
        viewport: [f32; 4],
    ) -> Option<Self> {
        let (brightness, contrast, saturation, color_grade) = match color_grade {
            Some([brightness, contrast, saturation]) => (brightness, contrast, saturation, 1.0),
            None => return None,
        };
        Some(Self {
            brightness,
            contrast,
            saturation,
            color_grade,
            viewport,
        })
    }

    /// Colour grading; the tests build chains this way.
    #[cfg(test)]
    pub(crate) fn color_grade(
        brightness: f32,
        contrast: f32,
        saturation: f32,
        viewport: [f32; 4],
    ) -> Self {
        Self::from_effects(Some([brightness, contrast, saturation]), viewport)
            .expect("colour grade enables the chain")
    }
}

/// One full-frame shader chain drawn in place of the frame texture copy.
pub(crate) struct PostChain {
    format: wgpu::TextureFormat,
    bind_group_layout: wgpu::BindGroupLayout,
    params_buffer: wgpu::Buffer,
    pipeline: wgpu::RenderPipeline,
}

impl PostChain {
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        params: PostParams,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Post process"),
            source: wgpu::ShaderSource::Wgsl(SHADER_SRC.into()),
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Post process source"),
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
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let params_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Post process params"),
            size: std::mem::size_of::<PostParams>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&params_buffer, 0, bytemuck::bytes_of(&params));
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Post process"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Post process"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
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
        });
        Self {
            format,
            bind_group_layout,
            params_buffer,
            pipeline,
        }
    }

    /// The colour format every target of this chain must use.
    pub(crate) fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    /// Update the uniform parameters without rebuilding the chain.
    pub(crate) fn set_params(&self, queue: &wgpu::Queue, params: PostParams) {
        queue.write_buffer(&self.params_buffer, 0, bytemuck::bytes_of(&params));
    }

    /// Draw `source` into `destination`, one texel per texel.
    pub(crate) fn draw(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::TextureView,
        destination: &wgpu::TextureView,
    ) {
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Post process source"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(source),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.params_buffer.as_entire_binding(),
                },
            ],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Post process"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: destination,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    // The full-screen triangle replaces every texel; clearing
                    // first only keeps the target free of stale contents.
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
}

#[cfg(test)]
mod tests {
    use super::SHADER_SRC;

    #[test]
    fn post_process_shader_compiles() {
        use wgpu::naga;
        let module =
            naga::front::wgsl::parse_str(SHADER_SRC).expect("the post-process shader parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("the post-process shader validates");
    }

    #[test]
    fn the_identity_params_disable_every_effect() {
        let params = super::PostParams::identity();
        assert_eq!(
            bytemuck::cast::<_, [f32; 8]>(params),
            [0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0]
        );
        let grade = super::PostParams::from_effects(Some([0.1, 1.2, 0.5]), [0.0, 0.0, 800.0, 600.0])
            .expect("colour grade configures the chain");
        assert_eq!(
            bytemuck::cast::<_, [f32; 8]>(grade),
            [0.1, 1.2, 0.5, 1.0, 0.0, 0.0, 800.0, 600.0]
        );
        assert!(super::PostParams::from_effects(None, [0.0; 4]).is_none());
    }
}

#[cfg(test)]
#[path = "post_process_tests.rs"]
mod gpu_tests;
