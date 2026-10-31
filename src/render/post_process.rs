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
//! format; the shader also has CPU-side parse tests so the checked-in WGSL is
//! validated without a GPU.
//!
//! ## Effects
//! - `ColorGrade` runs in linear RGB on the decoded texel.
//! - `FXAA` ports the ReShade FXAA 3.11 quality pass; it samples the source
//!   through a filtering sampler and blends edge texels before the other
//!   effects run.
//! - `FilmicPass`, `Colourfulness` and `Sepia` are ports of the ReShade
//!   community shaders of those names. They are authored in gamma space, so
//!   the shader encodes to sRGB before running them and decodes back for the
//!   sRGB target.
//! - `Bloom` is a four-level downsample/upsample chain derived from the
//!   ReShade bloom shaders: a display-luma bright pass, box downsamples and
//!   additive tent upsamples into RGBA16Float levels, then a composite over
//!   the graded frame.
//!
//! ## Colour space
//! The frame target carries the surface's sRGB suffix, so `textureLoad` decodes
//! to linear and the target re-encodes on write. Bloom levels stay linear.
//!
//! Depends on: `render/gpu.rs` (through the caller) and wgpu only.

const SHADER_SRC: &str = include_str!("post_process.wgsl");
const BLOOM_SHADER_SRC: &str = include_str!("post_process_bloom.wgsl");
const BLOOM_COMPOSITE_SRC: &str = include_str!("post_process_bloom_composite.wgsl");

/// Bloom levels below half resolution: 1/2, 1/4, 1/8 and 1/16 of the frame.
const BLOOM_LEVELS: usize = 4;

/// Uniform payload for [`PostChain`]; eight vec4s keep std140 alignment trivial.
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

    fxaa_subpix: f32,
    fxaa_edge_threshold: f32,
    fxaa_edge_threshold_min: f32,
    /// 1 runs the FXAA arm; 0 skips it.
    fxaa_enabled: f32,

    bloom_threshold: f32,
    bloom_intensity: f32,
    bloom_radius: f32,
    /// 1 runs the bloom chain; 0 composites nothing.
    bloom_enabled: f32,
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
            fxaa_subpix: 0.0,
            fxaa_edge_threshold: 0.125,
            fxaa_edge_threshold_min: 0.0,
            fxaa_enabled: 0.0,
            bloom_threshold: 0.0,
            bloom_intensity: 0.0,
            bloom_radius: 1.0,
            bloom_enabled: 0.0,
        }
    }

    /// Build the uniform from the configured effects; `None` means the chain
    /// has nothing to do and the caller should keep the texture copy.
    ///
    /// - `filmic`: strength, fade, contrast, linearization, bleach, saturation.
    /// - `colourfulness`: amount, lim_luma.
    /// - `sepia`: strength, tint red, tint green, tint blue.
    /// - `fxaa`: subpix, edge threshold, edge threshold minimum.
    /// - `bloom`: threshold, intensity, radius multiplier.
    pub(crate) fn from_effects(
        color_grade: Option<[f32; 3]>,
        filmic: Option<[f32; 6]>,
        colourfulness: Option<[f32; 2]>,
        sepia: Option<[f32; 4]>,
        fxaa: Option<[f32; 3]>,
        bloom: Option<[f32; 3]>,
        viewport: [f32; 4],
    ) -> Option<Self> {
        if color_grade.is_none()
            && filmic.is_none()
            && colourfulness.is_none()
            && sepia.is_none()
            && fxaa.is_none()
            && bloom.is_none()
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
        let [fxaa_subpix, fxaa_edge_threshold, fxaa_edge_threshold_min] =
            fxaa.unwrap_or([0.0, 0.125, 0.0]);
        let [bloom_threshold, bloom_intensity, bloom_radius] =
            bloom.unwrap_or([0.0, 0.0, 1.0]);
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
            fxaa_subpix,
            fxaa_edge_threshold,
            fxaa_edge_threshold_min,
            fxaa_enabled: fxaa.is_some() as u8 as f32,
            bloom_threshold,
            bloom_intensity,
            bloom_radius,
            bloom_enabled: bloom.is_some() as u8 as f32,
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
        Self::from_effects(
            Some([brightness, contrast, saturation]),
            None,
            None,
            None,
            None,
            None,
            viewport,
        )
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
            None,
            None,
            viewport,
        )
        .expect("filmic pass enables the chain")
    }

    /// Sepia at its authored defaults; the tests build chains this way.
    #[cfg(test)]
    pub(crate) fn sepia(viewport: [f32; 4]) -> Self {
        Self::from_effects(
            None,
            None,
            None,
            Some([0.58, 0.55, 0.43, 0.42]),
            None,
            None,
            viewport,
        )
        .expect("sepia enables the chain")
    }

    /// FXAA at its authored defaults; the tests build chains this way.
    #[cfg(test)]
    pub(crate) fn fxaa(viewport: [f32; 4]) -> Self {
        Self::from_effects(
            None,
            None,
            None,
            None,
            Some([0.25, 0.125, 0.0]),
            None,
            viewport,
        )
        .expect("fxaa enables the chain")
    }

    /// Bloom with an explicit threshold; the tests build chains this way.
    #[cfg(test)]
    pub(crate) fn bloom(threshold: f32, intensity: f32, viewport: [f32; 4]) -> Self {
        Self::from_effects(
            None,
            None,
            None,
            None,
            None,
            Some([threshold, intensity, 1.0]),
            viewport,
        )
        .expect("bloom enables the chain")
    }
}

/// One downsample level of the bloom chain.
struct BloomLevel {
    size: (u32, u32),
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
}

/// One full-frame shader chain drawn in place of the frame texture copy.
pub(crate) struct PostChain {
    format: wgpu::TextureFormat,
    bind_group_layout: wgpu::BindGroupLayout,
    composite_layout: wgpu::BindGroupLayout,
    params_buffer: wgpu::Buffer,
    /// One uniform per bloom level: each pass needs its own target size, and
    /// queue writes all land before the single submitted command buffer, so a
    /// shared buffer would show every pass the last written size.
    bloom_level_params: Vec<wgpu::Buffer>,
    bloom_threshold: f32,
    bloom_intensity: f32,
    bloom_radius: f32,
    viewport: [f32; 4],
    sampler: wgpu::Sampler,
    pipeline: wgpu::RenderPipeline,
    down_threshold_pipeline: wgpu::RenderPipeline,
    down_pipeline: wgpu::RenderPipeline,
    up_pipeline: wgpu::RenderPipeline,
    composite_pipeline: wgpu::RenderPipeline,
    intermediate: Option<wgpu::Texture>,
    intermediate_view: Option<wgpu::TextureView>,
    levels: Vec<BloomLevel>,
    size: (u32, u32),
    bloom_enabled: bool,
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
        let bloom_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Post process bloom"),
            source: wgpu::ShaderSource::Wgsl(BLOOM_SHADER_SRC.into()),
        });
        let bloom_composite_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Post process bloom composite"),
            source: wgpu::ShaderSource::Wgsl(BLOOM_COMPOSITE_SRC.into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Post process source"),
            entries: &[
                texture_entry(0, true),
                uniform_entry(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let composite_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Post process bloom composite"),
            entries: &[
                texture_entry(0, true),
                texture_entry(1, true),
                uniform_entry(2),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
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

        // Linear filtering, clamped: FXAA and the bloom upsample refetch texels.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Post process sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Post process"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let pipeline = post_pipeline(
            device,
            "Post process",
            &layout,
            &shader,
            "fs_main",
            format,
            None,
        );

        let bloom_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Post process bloom"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let bloom_format = wgpu::TextureFormat::Rgba16Float;
        let down_threshold_pipeline = post_pipeline(
            device,
            "Post process bloom threshold",
            &bloom_layout,
            &bloom_shader,
            "fs_down_threshold",
            bloom_format,
            None,
        );
        let down_pipeline = post_pipeline(
            device,
            "Post process bloom down",
            &bloom_layout,
            &bloom_shader,
            "fs_down",
            bloom_format,
            None,
        );
        let up_blend = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let up_pipeline = post_pipeline(
            device,
            "Post process bloom up",
            &bloom_layout,
            &bloom_shader,
            "fs_up",
            bloom_format,
            Some(up_blend),
        );

        let composite_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Post process bloom composite"),
                bind_group_layouts: &[Some(&composite_layout)],
                immediate_size: 0,
            });
        let composite_pipeline = post_pipeline(
            device,
            "Post process bloom composite",
            &composite_pipeline_layout,
            &bloom_composite_shader,
            "fs_composite",
            format,
            None,
        );

        Self {
            format,
            bind_group_layout,
            composite_layout,
            params_buffer,
            bloom_level_params: Vec::new(),
            bloom_threshold: params.bloom_threshold,
            bloom_intensity: params.bloom_intensity,
            bloom_radius: params.bloom_radius,
            viewport: params.viewport,
            sampler,
            pipeline,
            down_threshold_pipeline,
            down_pipeline,
            up_pipeline,
            composite_pipeline,
            intermediate: None,
            intermediate_view: None,
            levels: Vec::new(),
            size: (0, 0),
            bloom_enabled: params.bloom_enabled != 0.0,
        }
    }

    /// The colour format every target of this chain must use.
    pub(crate) fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    /// Update the uniform parameters without rebuilding the chain.
    pub(crate) fn set_params(&mut self, queue: &wgpu::Queue, params: PostParams) {
        queue.write_buffer(&self.params_buffer, 0, bytemuck::bytes_of(&params));
        self.bloom_enabled = params.bloom_enabled != 0.0;
        self.bloom_threshold = params.bloom_threshold;
        self.bloom_intensity = params.bloom_intensity;
        self.bloom_radius = params.bloom_radius;
        self.viewport = params.viewport;
        for buffer in &self.bloom_level_params {
            queue.write_buffer(
                buffer,
                0,
                bytemuck::cast_slice(&[
                    params.bloom_threshold,
                    params.bloom_intensity,
                    params.bloom_radius,
                    0.0,
                ]),
            );
            queue.write_buffer(buffer, 32, bytemuck::cast_slice(&params.viewport));
        }
    }

    /// Draw `source` into `destination`, one texel per texel.
    ///
    /// With bloom configured this renders the effect pass into an intermediate
    /// frame, builds the four down/up sample levels, and composites the result;
    /// otherwise the effect pass writes straight to the destination.
    pub(crate) fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::TextureView,
        destination: &wgpu::TextureView,
        size: (u32, u32),
    ) {
        if !self.bloom_enabled {
            let bind_group = self.source_bind_group(device, source);
            self.run_pass(
                encoder,
                &self.pipeline,
                &bind_group,
                destination,
                wgpu::LoadOp::Clear(wgpu::Color::BLACK),
            );
            return;
        }

        self.ensure_targets(device, queue, size);
        let intermediate = self
            .intermediate_view
            .as_ref()
            .expect("bloom targets exist after ensure_targets");
        let bind_group = self.source_bind_group(device, source);
        self.run_pass(
            encoder,
            &self.pipeline,
            &bind_group,
            intermediate,
            wgpu::LoadOp::Clear(wgpu::Color::BLACK),
        );

        let threshold_group =
            self.bloom_bind_group(device, intermediate, &self.bloom_level_params[0]);
        self.run_pass(
            encoder,
            &self.down_threshold_pipeline,
            &threshold_group,
            &self.levels[0].view,
            wgpu::LoadOp::Clear(wgpu::Color::BLACK),
        );
        for index in 0..BLOOM_LEVELS - 1 {
            let params = &self.bloom_level_params[index + 1];
            let bind_group = self.bloom_bind_group(device, &self.levels[index].view, params);
            self.run_pass(
                encoder,
                &self.down_pipeline,
                &bind_group,
                &self.levels[index + 1].view,
                wgpu::LoadOp::Clear(wgpu::Color::BLACK),
            );
        }
        for index in (1..BLOOM_LEVELS).rev() {
            let params = &self.bloom_level_params[index - 1];
            let bind_group = self.bloom_bind_group(device, &self.levels[index].view, params);
            // Additive: keep this level's downsample content and add the smaller
            // level's tent-filtered contribution on top.
            self.run_pass(
                encoder,
                &self.up_pipeline,
                &bind_group,
                &self.levels[index - 1].view,
                wgpu::LoadOp::Load,
            );
        }

        let composite_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Post process bloom composite"),
            layout: &self.composite_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(intermediate),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&self.levels[0].view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.bloom_level_params[0].as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        self.run_pass(
            encoder,
            &self.composite_pipeline,
            &composite_group,
            destination,
            wgpu::LoadOp::Clear(wgpu::Color::BLACK),
        );
    }

    fn source_bind_group(&self, device: &wgpu::Device, source: &wgpu::TextureView) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
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
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        })
    }

    fn bloom_bind_group(
        &self,
        device: &wgpu::Device,
        source: &wgpu::TextureView,
        params: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Post process bloom source"),
            layout: &self.bind_group_layout,
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
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        })
    }

    fn run_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        pipeline: &wgpu::RenderPipeline,
        bind_group: &wgpu::BindGroup,
        view: &wgpu::TextureView,
        load: wgpu::LoadOp<wgpu::Color>,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Post process"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.draw(0..3, 0..1);
    }

    /// Follow the frame size: the intermediate and the four bloom levels are
    /// rebuilt only when the size actually changes.
    fn ensure_targets(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, size: (u32, u32)) {
        let size = (size.0.max(1), size.1.max(1));
        if self.intermediate.is_some() && size == self.size {
            return;
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Post process intermediate"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        self.intermediate_view = Some(texture.create_view(&wgpu::TextureViewDescriptor::default()));
        self.intermediate = Some(texture);

        self.levels.clear();
        let mut level = size;
        for _ in 0..BLOOM_LEVELS {
            level = ((level.0 / 2).max(1), (level.1 / 2).max(1));
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Post process bloom level"),
                size: wgpu::Extent3d {
                    width: level.0,
                    height: level.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba16Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            self.levels.push(BloomLevel {
                size: level,
                view: texture.create_view(&wgpu::TextureViewDescriptor::default()),
                _texture: texture,
            });
        }

        self.bloom_level_params.clear();
        for level in &self.levels {
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Post process bloom params"),
                size: 48,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            queue.write_buffer(
                &buffer,
                0,
                bytemuck::cast_slice(&[
                    self.bloom_threshold,
                    self.bloom_intensity,
                    self.bloom_radius,
                    0.0,
                    level.size.0 as f32,
                    level.size.1 as f32,
                    0.0,
                    0.0,
                ]),
            );
            queue.write_buffer(&buffer, 32, bytemuck::cast_slice(&self.viewport));
            self.bloom_level_params.push(buffer);
        }
        self.size = size;
    }
}

fn texture_entry(binding: u32, filterable: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable },
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

fn post_pipeline(
    device: &wgpu::Device,
    label: &str,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    entry: &str,
    format: wgpu::TextureFormat,
    blend: Option<wgpu::BlendState>,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(entry),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend,
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

#[cfg(test)]
mod tests {
    use super::{BLOOM_COMPOSITE_SRC, BLOOM_SHADER_SRC, SHADER_SRC};

    fn shader_parses(source: &str, label: &str) {
        use wgpu::naga;
        let module = naga::front::wgsl::parse_str(source).expect(label);
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect(label);
    }

    #[test]
    fn post_process_shaders_compile() {
        shader_parses(SHADER_SRC, "the post-process shader");
        shader_parses(BLOOM_SHADER_SRC, "the bloom shader");
        shader_parses(BLOOM_COMPOSITE_SRC, "the bloom composite shader");
    }

    #[test]
    fn the_identity_params_disable_every_effect() {
        let params = super::PostParams::identity();
        assert_eq!(
            bytemuck::cast::<_, [f32; 32]>(params),
            [
                0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0,
                0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 0.0, 0.125, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0
            ]
        );
    }

    #[test]
    fn from_effects_enables_only_the_configured_arms() {
        // Every part disabled: the caller keeps the texture copy.
        assert!(
            super::PostParams::from_effects(None, None, None, None, None, None, [0.0; 4]).is_none()
        );

        let grade = super::PostParams::from_effects(
            Some([0.1, 1.2, 0.5]),
            None,
            None,
            None,
            None,
            None,
            [0.0; 4],
        )
        .expect("colour grade configures the chain");
        assert_eq!(
            bytemuck::cast::<_, [f32; 32]>(grade),
            [
                0.1, 1.2, 0.5, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0,
                0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 0.0, 0.125, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0
            ],
            "grade-only leaves the ReShade arms disabled"
        );

        let all = super::PostParams::from_effects(
            None,
            Some([0.85, 0.4, 1.0, 0.5, 0.0, -0.15]),
            Some([0.4, 0.7]),
            Some([0.58, 0.55, 0.43, 0.42]),
            Some([0.25, 0.125, 0.0]),
            Some([0.5, 0.4, 1.0]),
            [0.0, 0.0, 800.0, 600.0],
        )
        .expect("ReShade arms configure the chain");
        assert_eq!(
            bytemuck::cast::<_, [f32; 32]>(all),
            [
                0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 800.0, 600.0, 0.85, 0.4, 1.0, 0.5, 0.0, -0.15,
                1.0, 0.0, 0.4, 0.7, 1.0, 1.0, 0.58, 0.55, 0.43, 0.42, 0.25, 0.125, 0.0, 1.0, 0.5,
                0.4, 1.0, 1.0
            ]
        );
    }
}

#[cfg(test)]
#[path = "post_process_tests.rs"]
mod gpu_tests;
