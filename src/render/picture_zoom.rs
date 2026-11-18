//! VERA-only, experimental (`docs/picture-model-art.md`, chain 6): picture
//! art drawn again at its own resolution over the magnified world.
//!
//! The world draws every picture at `PictureSize=` game pixels, in native
//! order with everything else, and the wheel zoom or the Options zoom then
//! magnifies it. Art above 1x keeps an HD twin of each of its atlas pages
//! (`picture_atlas::HdPage`) at the same normalised coordinates, so a world
//! instance's UV addresses its HD rectangle unchanged. After the
//! magnification this pass draws each opaque world instance from such a page
//! again, from the twin, at the target's resolution. Its shader
//! (`picture_zoom.wgsl`) maps each target pixel back to the world pixel it
//! shows, computes that pixel's native Z as the zsprite shader does and
//! compares it with the world's own depth there (no copied depth), so the
//! sharp pixels replace the magnified ones exactly where the 1x draw passed.
//!
//! Known residual: anything drawn over such an object later in the world
//! without writing Z (smoke, explosions, units in front) is covered by the
//! sharp copy. Translucent, FX and packed (destination-reading) draws keep
//! their magnified 1x pixels. The WebGL2 fallback cannot load a depth texture
//! in a shader, so it builds no HD pages and the pass never runs there.
//!
//! Nothing here exists, allocates or walks anything unless the sprite atlas
//! has HD picture pages (`SpriteAtlas::has_picture_hd_pages`).

use crate::render::batch::{BatchRenderer, SPRITE_INSTANCE_ATTRIBUTES, SpriteInstance};
use crate::render::picture_atlas::HdPage;
use crate::render::sprite_atlas::SpriteAtlas;

/// One HD page on the GPU: premultiplied encoded colour with mipmaps down to
/// one texel per game pixel, and its level-0 index plane.
pub(crate) struct PictureHdPage {
    color: wgpu::TextureView,
    indices: wgpu::TextureView,
}

impl PictureHdPage {
    pub(crate) fn new(device: &wgpu::Device, queue: &wgpu::Queue, page: &HdPage) -> Self {
        use wgpu::util::DeviceExt;
        let size = wgpu::Extent3d {
            width: page.size[0],
            height: page.size[1],
            depth_or_array_layers: 1,
        };
        let (chain, levels) = page.mip_chain();
        let texture = |label, format, mip_level_count, data: &[u8]| {
            device
                .create_texture_with_data(
                    queue,
                    &wgpu::TextureDescriptor {
                        label: Some(label),
                        size,
                        mip_level_count,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format,
                        usage: wgpu::TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    },
                    wgpu::util::TextureDataOrder::LayerMajor,
                    data,
                )
                .create_view(&Default::default())
        };
        Self {
            // Encoded bytes, as the palette rows: the shader divides by alpha
            // and hands the result to the palette light unchanged.
            color: texture(
                "Picture HD page",
                wgpu::TextureFormat::Rgba8Unorm,
                levels,
                &chain,
            ),
            indices: texture(
                "Picture HD page indices",
                wgpu::TextureFormat::R8Uint,
                1,
                &page.indices,
            ),
        }
    }
}

/// How an HD run tests Z: the world run's `RenderZPolicy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HdDepth {
    /// Passthrough: no test.
    None = 0,
    /// Read-only leaves: native Less.
    Less = 1,
    /// Read-write leaves, which stored their own Z: Less or equal.
    LessEqual = 2,
}

/// A run of HD instances drawn from one page with one Z test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HdRun {
    pub page: usize,
    pub depth: HdDepth,
    pub start: u32,
    pub count: u32,
}

/// Where this frame's HD draw goes: over the magnified frame
/// (`WorldZoomPass::composite`), or over the window when the Options zoom
/// scales the frame and the world goes there itself
/// (`WorldZoomPass::draw_window_copy`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HdTarget {
    Frame,
    Window,
}

/// Where the world lands in a target, in target pixels.
pub(crate) struct HdPlacement<'a> {
    /// The target pixel of world-target pixel (0, 0).
    pub origin: [f32; 2],
    /// Target pixels per world pixel; the pass draws only above 1.
    pub scale: f32,
    /// For the window only: what keeps the frame's own pixels.
    pub mask: Option<HdMask<'a>>,
}

/// A window pixel whose frame pixel changed after the world went in (UI
/// drawn over the battlefield) keeps the window, as the window copy's
/// `WindowMask::Changed` keeps it.
pub(crate) struct HdMask<'a> {
    /// The window pixel of frame pixel (0, 0), and window pixels per frame
    /// pixel.
    pub frame_origin: [f32; 2],
    pub frame_scale: f32,
    /// The finished frame, and the frame as the world left it.
    pub now: &'a wgpu::TextureView,
    pub before: &'a wgpu::TextureView,
}

/// `Params` of picture_zoom.wgsl.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    origin: [f32; 2],
    scale: f32,
    policy: u32,
    target_size: [f32; 2],
    frame_scale: f32,
    masked: u32,
    frame_origin: [f32; 2],
    pad: [f32; 2],
}

const PARAMS_SIZE: u64 = std::mem::size_of::<Params>() as u64;

/// The pass: its pipeline, and this frame's HD instances and runs.
pub(crate) struct PictureZoomPass {
    pipeline: wgpu::RenderPipeline,
    page_layout: wgpu::BindGroupLayout,
    view_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// One `Params` per [`HdDepth`], `params_stride` apart (dynamic offset).
    params: wgpu::Buffer,
    params_stride: u64,
    /// Bound as both mask textures where there is no mask.
    no_mask: wgpu::TextureView,
    instances: Option<wgpu::Buffer>,
    runs: Vec<HdRun>,
    target: HdTarget,
}

impl PictureZoomPass {
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        batch: &BatchRenderer,
    ) -> Self {
        use wgpu::util::DeviceExt;
        let texture_entry = |binding, sample_type| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let page_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Picture HD page BGL"),
            entries: &[
                texture_entry(0, wgpu::TextureSampleType::Float { filterable: true }),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                texture_entry(2, wgpu::TextureSampleType::Uint),
            ],
        });
        let frame = wgpu::TextureSampleType::Float { filterable: false };
        let view_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Picture HD view BGL"),
            entries: &[
                texture_entry(0, wgpu::TextureSampleType::Depth),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(PARAMS_SIZE),
                    },
                    count: None,
                },
                texture_entry(2, frame),
                texture_entry(3, frame),
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Picture HD shader"),
            source: wgpu::ShaderSource::Wgsl(
                crate::render::tactical_shader::world_source(include_str!("picture_zoom.wgsl"))
                    .into(),
            ),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Picture HD pipeline layout"),
            bind_group_layouts: &[
                Some(batch.camera_bind_group_layout()),
                Some(&page_layout),
                Some(&view_layout),
                Some(batch.zshape_bind_group_layout()),
            ],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Picture HD pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<SpriteInstance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &SPRITE_INSTANCE_ATTRIBUTES,
                })],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                // Opaque stores only (`app::presentation::render::picture_zoom`).
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
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Picture HD sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let params_stride = PARAMS_SIZE.next_multiple_of(u64::from(
            device.limits().min_uniform_buffer_offset_alignment,
        ));
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Picture HD params"),
            size: params_stride * 3,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let no_mask = device
            .create_texture_with_data(
                queue,
                &wgpu::TextureDescriptor {
                    label: Some("Picture HD no mask"),
                    size: wgpu::Extent3d {
                        width: 1,
                        height: 1,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
                wgpu::util::TextureDataOrder::LayerMajor,
                &[0; 4],
            )
            .create_view(&Default::default());
        Self {
            pipeline,
            page_layout,
            view_layout,
            sampler,
            params,
            params_stride,
            no_mask,
            instances: None,
            runs: Vec::new(),
            target: HdTarget::Frame,
        }
    }

    /// This frame's HD `instances`, drawn in `runs`, for `target`.
    pub(crate) fn set_frame(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        instances: &[SpriteInstance],
        runs: Vec<HdRun>,
        target: HdTarget,
    ) {
        let bytes: &[u8] = bytemuck::cast_slice(instances);
        let needed = (bytes.len() as u64).max(1);
        if self
            .instances
            .as_ref()
            .is_none_or(|buffer| buffer.size() < needed)
        {
            self.instances = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Picture HD instances"),
                size: needed.next_power_of_two().max(4096),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        if let Some(buffer) = &self.instances
            && !bytes.is_empty()
        {
            queue.write_buffer(buffer, 0, bytes);
        }
        self.runs = runs;
        self.target = target;
    }

    /// No HD draw this frame.
    pub(crate) fn clear(&mut self) {
        self.runs.clear();
    }

    /// Where this frame draws, if it draws.
    pub(crate) fn target(&self) -> Option<HdTarget> {
        (!self.runs.is_empty()).then_some(self.target)
    }

    /// Draw this frame's runs into `view` at `placement`, inside `scissor`
    /// (target pixels), testing Z against `world_depth`, the world's own
    /// depth at one texel per world pixel. `zshape` is the world runs' shape
    /// binding (`draw_native_object_pass`).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn draw(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        placement: &HdPlacement<'_>,
        world_depth: &wgpu::TextureView,
        atlas: &SpriteAtlas,
        batch: &BatchRenderer,
        zshape: &wgpu::BindGroup,
        scissor: [u32; 4],
    ) {
        let Some(buffer) = &self.instances else {
            return;
        };
        if self.runs.is_empty() || placement.scale <= 1.0 {
            return;
        }
        let target = view.texture().size();
        let [x, y, width, height] = scissor;
        let (x, y) = (x.min(target.width), y.min(target.height));
        let width = width.min(target.width - x);
        let height = height.min(target.height - y);
        if width == 0 || height == 0 {
            return;
        }
        let (now, before, frame_origin, frame_scale, masked) = match &placement.mask {
            Some(mask) => (
                mask.now,
                mask.before,
                mask.frame_origin,
                mask.frame_scale,
                1,
            ),
            None => (&self.no_mask, &self.no_mask, [0.0; 2], 1.0, 0),
        };
        for depth in [HdDepth::None, HdDepth::Less, HdDepth::LessEqual] {
            let params = Params {
                origin: placement.origin,
                scale: placement.scale,
                policy: depth as u32,
                target_size: [target.width as f32, target.height as f32],
                frame_scale,
                masked,
                frame_origin,
                pad: [0.0; 2],
            };
            queue.write_buffer(
                &self.params,
                depth as u64 * self.params_stride,
                bytemuck::bytes_of(&params),
            );
        }
        let view_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Picture HD view"),
            layout: &self.view_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(world_depth),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &self.params,
                        offset: 0,
                        size: wgpu::BufferSize::new(PARAMS_SIZE),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(now),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(before),
                },
            ],
        });
        // One bind group per HD page this frame draws from.
        let mut page_groups: Vec<(usize, wgpu::BindGroup)> = Vec::new();
        for run in &self.runs {
            if page_groups.iter().any(|(page, _)| *page == run.page) {
                continue;
            }
            let Some(page) = atlas.picture_hd_page(run.page) else {
                continue;
            };
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Picture HD page"),
                layout: &self.page_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&page.color),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&page.indices),
                    },
                ],
            });
            page_groups.push((run.page, group));
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Picture HD over the magnified world"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
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
        pass.set_bind_group(0, batch.camera_bind_group(), &[]);
        pass.set_bind_group(3, zshape, &[]);
        pass.set_vertex_buffer(0, buffer.slice(..));
        pass.set_scissor_rect(x, y, width, height);
        for run in &self.runs {
            let Some((_, group)) = page_groups.iter().find(|(page, _)| *page == run.page) else {
                continue;
            };
            let offset = run.depth as u64 * self.params_stride;
            pass.set_bind_group(1, group, &[]);
            pass.set_bind_group(2, &view_group, &[offset as u32]);
            pass.draw(0..6, run.start..run.start + run.count);
        }
    }
}
