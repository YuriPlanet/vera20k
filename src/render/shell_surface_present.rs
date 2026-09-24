//! Encoded-byte RGB565 presentation for stock shell and loading surfaces.
//!
//! The native shell composes into a 16-bit DirectDraw surface. Rust keeps its
//! existing sRGB shell/loading painters, then this boundary samples the
//! stored encoded bytes through a compatible unorm view, applies the guarded
//! presentation codebooks, and copies the resulting bytes into the swapchain.
//!
//! Retail provenance: DirectDraw surface-format derivation — `DSurface__Constructor` @ `0x004BA770`.

use std::num::NonZeroU64;

use anyhow::{Result, bail};
use wgpu::util::DeviceExt;

use super::{gpu::GpuContext, native_surface_format::ACTIVE_RETAIL_RGB565_PRESENTATION};

const SHADER_SOURCE: &str = include_str!("shell_surface_present.wgsl");
const PROFILE_WORD_COUNT: usize = 96;
const PROFILE_BUFFER_SIZE: u64 = (PROFILE_WORD_COUNT * std::mem::size_of::<u32>()) as u64;
/// `u32 fade_rows, height` padded to a 16-byte uniform.
const EFFECTS_WORD_COUNT: usize = 4;
const EFFECTS_BUFFER_SIZE: u64 = (EFFECTS_WORD_COUNT * std::mem::size_of::<u32>()) as u64;

/// Native 16-bit surface operations applied in the RGB565 unit domain, after
/// quantization and before the presentation codebook.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct SurfaceEffects {
    /// Rows at the top and bottom faded by Show_Credits `0x004C3E30`: row
    /// `r` from the edge keeps `floor(v * f / 256)` per channel with
    /// `f = 255 - min(256 - 8r, 255)`. Zero disables the fade.
    pub(crate) fade_rows: u32,
}

impl SurfaceEffects {
    fn words(self, height: u32) -> [u32; EFFECTS_WORD_COUNT] {
        [self.fade_rows, height, 0, 0]
    }
}

/// GPU resources for the active-retail shell presentation boundary.
pub(crate) struct ShellSurfacePresenter {
    _source_texture: wgpu::Texture,
    source_render_view: wgpu::TextureView,
    _source_encoded_view: wgpu::TextureView,
    presented_texture: wgpu::Texture,
    presented_view: wgpu::TextureView,
    _profile_buffer: wgpu::Buffer,
    /// Always-zero effects: ordinary presentation can never inherit effects.
    no_effects_buffer: wgpu::Buffer,
    /// Rewritten by each [`Self::encode_present_with_effects`] call.
    effects_buffer: wgpu::Buffer,
    bind_group_layout: wgpu::BindGroupLayout,
    bind_group: wgpu::BindGroup,
    effects_bind_group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
    surface_format: wgpu::TextureFormat,
    encoded_format: wgpu::TextureFormat,
    width: u32,
    height: u32,
}

impl ShellSurfacePresenter {
    /// Build the byte-domain presenter for the configured swapchain format.
    pub(crate) fn new(gpu: &GpuContext) -> Result<Self> {
        if gpu.config.width == 0 || gpu.config.height == 0 {
            bail!(
                "exact stock-shell presentation requires a non-zero surface, \
                 configured {}x{}",
                gpu.config.width,
                gpu.config.height
            );
        }
        let surface_format = gpu.surface_format;
        let encoded_format = encoded_surface_format(surface_format)?;
        let bind_group_layout = create_bind_group_layout(&gpu.device);
        let profile_words = ACTIVE_RETAIL_RGB565_PRESENTATION.shader_words();
        let profile_buffer = gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Active retail RGB565 presentation profile"),
                contents: bytemuck::cast_slice(&profile_words),
                usage: wgpu::BufferUsages::STORAGE,
            });
        let effects_usage = wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST;
        let no_effects_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Shell presentation: no surface effects"),
            size: EFFECTS_BUFFER_SIZE,
            usage: effects_usage,
            mapped_at_creation: false,
        });
        let effects_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Shell presentation: surface effects"),
            size: EFFECTS_BUFFER_SIZE,
            usage: effects_usage,
            mapped_at_creation: false,
        });
        let pipeline = create_pipeline(&gpu.device, &bind_group_layout, encoded_format);
        let targets = create_targets(
            &gpu.device,
            &bind_group_layout,
            &profile_buffer,
            [&no_effects_buffer, &effects_buffer],
            surface_format,
            encoded_format,
            gpu.config.width,
            gpu.config.height,
        );

        Ok(Self {
            _source_texture: targets.source_texture,
            source_render_view: targets.source_render_view,
            _source_encoded_view: targets.source_encoded_view,
            presented_texture: targets.presented_texture,
            presented_view: targets.presented_view,
            _profile_buffer: profile_buffer,
            no_effects_buffer,
            effects_buffer,
            bind_group_layout,
            bind_group: targets.bind_group,
            effects_bind_group: targets.effects_bind_group,
            pipeline,
            surface_format,
            encoded_format,
            width: gpu.config.width,
            height: gpu.config.height,
        })
    }

    /// Clone the sRGB offscreen view used by the existing shell painters.
    pub(crate) fn source_render_view(&self) -> wgpu::TextureView {
        self.source_render_view.clone()
    }

    /// Recreate only surface-sized resources after a swapchain resize.
    pub(crate) fn resize(&mut self, gpu: &GpuContext) {
        if self.width == gpu.config.width && self.height == gpu.config.height {
            return;
        }
        debug_assert_eq!(self.surface_format, gpu.surface_format);
        debug_assert_eq!(self.encoded_format, gpu.surface_format.remove_srgb_suffix());

        let targets = create_targets(
            &gpu.device,
            &self.bind_group_layout,
            &self._profile_buffer,
            [&self.no_effects_buffer, &self.effects_buffer],
            self.surface_format,
            self.encoded_format,
            gpu.config.width,
            gpu.config.height,
        );
        self._source_texture = targets.source_texture;
        self.source_render_view = targets.source_render_view;
        self._source_encoded_view = targets.source_encoded_view;
        self.presented_texture = targets.presented_texture;
        self.presented_view = targets.presented_view;
        self.bind_group = targets.bind_group;
        self.effects_bind_group = targets.effects_bind_group;
        self.width = gpu.config.width;
        self.height = gpu.config.height;
    }

    /// Quantize the completed shell and copy its encoded bytes to the surface.
    pub(crate) fn encode_present(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        destination: &wgpu::Texture,
    ) {
        self.encode_present_with_group(encoder, destination, &self.bind_group);
    }

    /// Present with native 16-bit surface effects. The effects buffer is
    /// written through the queue, so it applies to the next submission; only
    /// one effects presentation may be encoded per submitted frame.
    pub(crate) fn encode_present_with_effects(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        destination: &wgpu::Texture,
        effects: SurfaceEffects,
    ) {
        queue.write_buffer(
            &self.effects_buffer,
            0,
            bytemuck::cast_slice(&effects.words(self.height)),
        );
        self.encode_present_with_group(encoder, destination, &self.effects_bind_group);
    }

    fn encode_present_with_group(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        destination: &wgpu::Texture,
        bind_group: &wgpu::BindGroup,
    ) {
        debug_assert_eq!(destination.width(), self.width);
        debug_assert_eq!(destination.height(), self.height);
        debug_assert_eq!(destination.format(), self.surface_format);
        debug_assert!(
            destination.usage().contains(wgpu::TextureUsages::COPY_DST),
            "shell presentation destination lacks COPY_DST"
        );

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Active retail RGB565 shell presentation"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.presented_view,
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
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, bind_group, &[]);
            pass.draw(0..3, 0..1);
        }

        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.presented_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
                texture: destination,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );
    }
}

struct PresentationTargets {
    source_texture: wgpu::Texture,
    source_render_view: wgpu::TextureView,
    source_encoded_view: wgpu::TextureView,
    presented_texture: wgpu::Texture,
    presented_view: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
    effects_bind_group: wgpu::BindGroup,
}

fn encoded_surface_format(surface_format: wgpu::TextureFormat) -> Result<wgpu::TextureFormat> {
    match surface_format {
        wgpu::TextureFormat::Bgra8UnormSrgb | wgpu::TextureFormat::Rgba8UnormSrgb => {
            Ok(surface_format.remove_srgb_suffix())
        }
        _ => bail!(
            "exact stock-shell presentation requires an sRGB BGRA8/RGBA8 \
             swapchain, selected {surface_format:?}"
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn create_targets(
    device: &wgpu::Device,
    bind_group_layout: &wgpu::BindGroupLayout,
    profile_buffer: &wgpu::Buffer,
    [no_effects_buffer, effects_buffer]: [&wgpu::Buffer; 2],
    surface_format: wgpu::TextureFormat,
    encoded_format: wgpu::TextureFormat,
    width: u32,
    height: u32,
) -> PresentationTargets {
    let extent = wgpu::Extent3d {
        width: width.max(1),
        height: height.max(1),
        depth_or_array_layers: 1,
    };
    let source_texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Stock shell sRGB composition surface"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: surface_format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[encoded_format],
    });
    let source_render_view = source_texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some("Stock shell sRGB render view"),
        format: Some(surface_format),
        usage: Some(wgpu::TextureUsages::RENDER_ATTACHMENT),
        ..Default::default()
    });
    let source_encoded_view = source_texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some("Stock shell encoded-byte sampling view"),
        format: Some(encoded_format),
        usage: Some(wgpu::TextureUsages::TEXTURE_BINDING),
        ..Default::default()
    });
    let presented_texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Stock shell quantized presentation surface"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: encoded_format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let presented_view = presented_texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some("Stock shell quantized presentation view"),
        format: Some(encoded_format),
        usage: Some(wgpu::TextureUsages::RENDER_ATTACHMENT),
        ..Default::default()
    });
    let group = |effects: &wgpu::Buffer, label| {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout: bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&source_encoded_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: profile_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: effects.as_entire_binding(),
                },
            ],
        })
    };
    let bind_group = group(
        no_effects_buffer,
        "Stock shell RGB565 presentation bind group",
    );
    let effects_bind_group = group(effects_buffer, "Stock shell RGB565 effects bind group");

    PresentationTargets {
        source_texture,
        source_render_view,
        source_encoded_view,
        presented_texture,
        presented_view,
        bind_group,
        effects_bind_group,
    }
}

fn create_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Stock shell RGB565 presentation layout"),
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
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: NonZeroU64::new(PROFILE_BUFFER_SIZE),
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: NonZeroU64::new(EFFECTS_BUFFER_SIZE),
                },
                count: None,
            },
        ],
    })
}

fn create_pipeline(
    device: &wgpu::Device,
    bind_group_layout: &wgpu::BindGroupLayout,
    encoded_format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Stock shell RGB565 presentation shader"),
        source: wgpu::ShaderSource::Wgsl(SHADER_SOURCE.into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Stock shell RGB565 presentation pipeline layout"),
        bind_group_layouts: &[bind_group_layout],
        push_constant_ranges: &[],
    });

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Stock shell RGB565 presentation pipeline"),
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
                format: encoded_format,
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
        multiview: None,
        cache: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_copy_compatible_color_srgb_surface_formats_are_accepted() {
        assert_eq!(
            encoded_surface_format(wgpu::TextureFormat::Bgra8UnormSrgb).unwrap(),
            wgpu::TextureFormat::Bgra8Unorm
        );
        assert_eq!(
            encoded_surface_format(wgpu::TextureFormat::Rgba8UnormSrgb).unwrap(),
            wgpu::TextureFormat::Rgba8Unorm
        );
        assert!(encoded_surface_format(wgpu::TextureFormat::Bgra8Unorm).is_err());
        assert!(encoded_surface_format(wgpu::TextureFormat::Rgba16Float).is_err());
    }

    #[test]
    fn surface_effect_words_match_the_shader_uniform() {
        assert_eq!(EFFECTS_BUFFER_SIZE, 16);
        assert_eq!(SurfaceEffects { fade_rows: 32 }.words(600), [32, 600, 0, 0]);
        assert_eq!(SurfaceEffects::default().words(480), [0, 480, 0, 0]);
    }

    #[test]
    fn profile_buffer_size_matches_shader_contract() {
        assert_eq!(
            ACTIVE_RETAIL_RGB565_PRESENTATION.shader_words().len(),
            PROFILE_WORD_COUNT
        );
        assert_eq!(PROFILE_BUFFER_SIZE, 384);
    }
}
