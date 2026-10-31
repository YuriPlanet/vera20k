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
//! ## Effects
//! - `ColorGrade` runs in linear RGB on the decoded texel.
//! - `FilmicPass`, `Colourfulness` and `Sepia` are ports of the ReShade
//!   community shaders of those names. They are authored in gamma space, so
//!   the shader encodes to sRGB before running them and decodes back for the
//!   sRGB target.
//!
//! ## Colour space
//! The frame target carries the surface's sRGB suffix, so `textureLoad` decodes
//! to linear and the target re-encodes on write.
//!
//! Depends on: `render/gpu.rs` (through the caller) and wgpu only.

const SHADER_SRC: &str = include_str!("post_process.wgsl");

/// Uniform payload for [`PostChain`]; six vec4s keep std140 alignment trivial.
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

    filmic_strength: f32,
    filmic_fade: f32,
    filmic_contrast: f32,
    filmic_linearization: f32,

    filmic_bleach: f32,
    filmic_saturation: f32,
    /// 1 runs the FilmicPass arm; 0 skips it.
    filmic_enabled: f32,
    _pad0: f32,

    colour_amount: f32,
    colour_lim_luma: f32,
    /// 1 runs the Colourfulness arm; 0 skips it.
    colour_enabled: f32,
    /// 1 runs the Sepia arm; 0 skips it.
    sepia_enabled: f32,

    sepia_strength: f32,
    sepia_tint_r: f32,
    sepia_tint_g: f32,
    sepia_tint_b: f32,
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
            filmic_strength: 0.0,
            filmic_fade: 0.0,
            filmic_contrast: 1.0,
            filmic_linearization: 1.0,
            filmic_bleach: 0.0,
            filmic_saturation: 0.0,
            filmic_enabled: 0.0,
            _pad0: 0.0,
            colour_amount: 0.0,
            colour_lim_luma: 1.0,
            colour_enabled: 0.0,
            sepia_enabled: 0.0,
            sepia_strength: 0.0,
            sepia_tint_r: 1.0,
            sepia_tint_g: 1.0,
            sepia_tint_b: 1.0,
        }
    }

    /// Build the uniform from the configured effects; `None` means the chain
    /// has nothing to do and the caller should keep the texture copy.
    ///
    /// - `filmic`: strength, fade, contrast, linearization, bleach, saturation.
    /// - `colourfulness`: amount, lim_luma.
    /// - `sepia`: strength, tint red, tint green, tint blue.
    pub(crate) fn from_effects(
        color_grade: Option<[f32; 3]>,
        filmic: Option<[f32; 6]>,
        colourfulness: Option<[f32; 2]>,
        sepia: Option<[f32; 4]>,
        viewport: [f32; 4],
    ) -> Option<Self> {
        if color_grade.is_none()
            && filmic.is_none()
            && colourfulness.is_none()
            && sepia.is_none()
        {
            return None;
        }
        let (brightness, contrast, saturation, color_grade) = match color_grade {
            Some([brightness, contrast, saturation]) => (brightness, contrast, saturation, 1.0),
            None => (0.0, 1.0, 1.0, 0.0),
        };
        let [
            filmic_strength,
            filmic_fade,
            filmic_contrast,
            filmic_linearization,
            filmic_bleach,
            filmic_saturation,
        ] = filmic.unwrap_or([0.0, 0.0, 1.0, 1.0, 0.0, 0.0]);
        let [colour_amount, colour_lim_luma] = colourfulness.unwrap_or([0.0, 1.0]);
        let [sepia_strength, sepia_tint_r, sepia_tint_g, sepia_tint_b] =
            sepia.unwrap_or([0.0, 1.0, 1.0, 1.0]);
        Some(Self {
            brightness,
            contrast,
            saturation,
            color_grade,
            viewport,
            filmic_strength,
            filmic_fade,
            filmic_contrast,
            filmic_linearization,
            filmic_bleach,
            filmic_saturation,
            filmic_enabled: filmic.is_some() as u8 as f32,
            _pad0: 0.0,
            colour_amount,
            colour_lim_luma,
            colour_enabled: colourfulness.is_some() as u8 as f32,
            sepia_enabled: sepia.is_some() as u8 as f32,
            sepia_strength,
            sepia_tint_r,
            sepia_tint_g,
            sepia_tint_b,
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
        Self::from_effects(Some([brightness, contrast, saturation]), None, None, None, viewport)
            .expect("colour grade enables the chain")
    }

    /// FilmicPass at its authored defaults; the tests build chains this way.
    #[cfg(test)]
    pub(crate) fn filmic(viewport: [f32; 4]) -> Self {
        Self::from_effects(
            None,
            Some([0.85, 0.4, 1.0, 0.5, 0.0, -0.15]),
            None,
            None,
            viewport,
        )
        .expect("filmic pass enables the chain")
    }

    /// Sepia at its authored defaults; the tests build chains this way.
    #[cfg(test)]
    pub(crate) fn sepia(viewport: [f32; 4]) -> Self {
        Self::from_effects(None, None, None, Some([0.58, 0.55, 0.43, 0.42]), viewport)
            .expect("sepia enables the chain")
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
            bytemuck::cast::<_, [f32; 24]>(params),
            [
                0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0,
                0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0
            ]
        );
    }

    #[test]
    fn from_effects_enables_only_the_configured_arms() {
        // Every part disabled: the caller keeps the texture copy.
        assert!(
            super::PostParams::from_effects(None, None, None, None, [0.0; 4]).is_none()
        );

        let grade =
            super::PostParams::from_effects(Some([0.1, 1.2, 0.5]), None, None, None, [0.0; 4])
                .expect("colour grade configures the chain");
        assert_eq!(
            bytemuck::cast::<_, [f32; 24]>(grade),
            [
                0.1, 1.2, 0.5, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0,
                0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0
            ],
            "grade-only leaves the ReShade arms disabled"
        );

        let filmic = super::PostParams::from_effects(
            None,
            Some([0.85, 0.4, 1.0, 0.5, 0.0, -0.15]),
            Some([0.4, 0.7]),
            Some([0.58, 0.55, 0.43, 0.42]),
            [0.0, 0.0, 800.0, 600.0],
        )
        .expect("ReShade arms configure the chain");
        assert_eq!(
            bytemuck::cast::<_, [f32; 24]>(filmic),
            [
                0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 800.0, 600.0, 0.85, 0.4, 1.0, 0.5, 0.0, -0.15,
                1.0, 0.0, 0.4, 0.7, 1.0, 1.0, 0.58, 0.55, 0.43, 0.42
            ]
        );
    }
}

#[cfg(test)]
#[path = "post_process_tests.rs"]
mod gpu_tests;
