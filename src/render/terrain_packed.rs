//! Packed SHP / whole Unit final composite. Destination and Z remain owned by
//! TerrainDrawRenderer. Original 73B140, 495590 and 4986D0; executed evidence:
//! tools/procedural_drawing_oracle/translucent_blitter_a.{json,md}.
//!
//! Negative offsets read earlier stores in the same raster span. Each residue
//! modulo abs(offset_words) is independent; its compute invocation walks in
//! native framebuffer word order. No immutable-piece shortcut is used there.

use super::batching::{TerrainBatches, TerrainCommand};
use super::{PackedSurface, TerrainBatchStats, TerrainDrawRenderer, piece_scissor};
use crate::render::batch::{
    BatchRenderer, CameraUniform, SPRITE_INSTANCE_ATTRIBUTES, SpriteInstance,
};
use crate::render::tactical_draw_plan::RenderZPolicy;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PackedSpriteKind {
    Shp,
    Voxel,
    /// Original source-index stencil, with no palette, A or Z source reads.
    IndexMask,
    /// Ordinary N=1 UI source, already converted to native surface colours.
    UiPalette,
}

/// Same-parent VXL layers compose into one indexed source stencil, then one
/// scene destination operation. Missing sources are omitted by the caller.
#[derive(Clone, Copy)]
pub(crate) struct PackedCompositePiece<'a> {
    pub index: u32,
    pub instance: &'a SpriteInstance,
    pub source: &'a wgpu::BindGroup,
    pub material: &'a wgpu::BindGroup,
    pub kind: PackedSpriteKind,
}

pub(crate) struct PackedComposite<'a> {
    pub pieces: Vec<PackedCompositePiece<'a>>,
    pub render_z: RenderZPolicy,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    rect: [u32; 4],
    surface_width: u32,
    selector: u32,
    offset_words: i32,
    reads_depth: u32,
}

pub(super) struct Stencil {
    words: wgpu::TextureView,
    candidates: wgpu::TextureView,
    /// WebGL2 ordered words target and its resolve binding (`fs_feedback`).
    results: Option<(wgpu::TextureView, wgpu::BindGroup)>,
}

/// Native signed candidate and projection have one source shared with the
/// opaque path, rather than another geometry/Z port.
pub(super) fn raster_source(body: &str, fragment: &str) -> String {
    let prefix = &body[..body.find("fn apply_fx(").expect("projection boundary")];
    crate::render::tactical_shader::world_source(&format!("{prefix}\n{fragment}"))
}

/// Packed shader source. The WebGL2 fallback drops the compute entry point
/// and storage results for `terrain_packed_webgl.wgsl`.
pub(super) fn packed_source(webgl: bool) -> String {
    let source = include_str!("terrain_packed.wgsl");
    if !webgl {
        return crate::render::tactical_shader::source(source);
    }
    use crate::render::webgl::patch;
    let shared = &source[..source.find("fn resolve_pixel(").expect("packed resolve")];
    let shared = patch(
        shared,
        "@group(0) @binding(4) var<storage, read_write> results: array<u32>;\n",
        "",
    );
    let tail = patch(
        &source[source.find("@vertex").expect("packed full-screen passes")..],
        "word = results[local_index(p)];",
        "word = textureLoad(fallback_results, vec2i(p), 0).r;",
    );
    crate::render::tactical_shader::source(&format!(
        "{shared}\n{}\n{tail}",
        include_str!("terrain_packed_webgl.wgsl")
    ))
}

pub(super) struct PackedSpriteGpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    shp: wgpu::RenderPipeline,
    mask: wgpu::RenderPipeline,
    ui: wgpu::RenderPipeline,
    voxel: wgpu::RenderPipeline,
    clear: wgpu::RenderPipeline,
    /// Ordinary devices order feedback in compute; WebGL2 in `feedback`.
    compute: Option<wgpu::ComputePipeline>,
    feedback: Option<wgpu::RenderPipeline>,
    results_layout: Option<wgpu::BindGroupLayout>,
    resolve_read: wgpu::RenderPipeline,
    resolve_write: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    stencil: Option<Stencil>,
    results: Option<wgpu::Buffer>,
    uniform_chunks: Vec<wgpu::Buffer>,
    uniform_slot: usize,
    uniform_stride: u64,
    batches: TerrainBatches<usize>,
}

impl PackedSpriteGpu {
    pub(super) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        batch: &BatchRenderer,
    ) -> Self {
        let target = |format| {
            Some(wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })
        };
        let webgl = crate::render::gpu::webgl_fallback(device);
        // The source pass's stencil words and candidates. Attachments 2 and 3
        // take the destination snapshot (`packed_snapshot_pipeline`), which
        // the stencil draws leave as it is. WebGL2 has no per-attachment
        // write masks (wgpu's INDEPENDENT_BLEND), so there the snapshot keeps
        // a pass of its own.
        let kept = |format| {
            Some(wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::empty(),
            })
        };
        let source_targets = [
            target(wgpu::TextureFormat::R32Uint),
            target(wgpu::TextureFormat::R32Float),
            kept(wgpu::TextureFormat::R32Uint),
            kept(wgpu::TextureFormat::R32Float),
        ];
        let source_targets = &source_targets[..if webgl { 2 } else { 4 }];
        let raster = |body: &str,
                      fragment: &str,
                      source_layout: &wgpu::BindGroupLayout,
                      material_layout: &wgpu::BindGroupLayout| {
            let source = raster_source(body, fragment);
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Packed source stencil"),
                source: wgpu::ShaderSource::Wgsl(
                    crate::render::webgl::camera_source(device, source).into(),
                ),
            });
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Packed source layout"),
                bind_group_layouts: &[
                    Some(batch.camera_bind_group_layout()),
                    Some(source_layout),
                    Some(material_layout),
                ],
                immediate_size: 0,
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Packed source raster"),
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
                    entry_point: Some("fs_packed"),
                    targets: source_targets,
                    compilation_options: Default::default(),
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let visibility = if webgl {
            wgpu::ShaderStages::FRAGMENT
        } else {
            wgpu::ShaderStages::COMPUTE | wgpu::ShaderStages::FRAGMENT
        };
        let texture = |binding, sample_type| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty: wgpu::BindingType::Texture {
                sample_type,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let results = wgpu::BindGroupLayoutEntry {
            binding: 4,
            visibility,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: false },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let params = wgpu::BindGroupLayoutEntry {
            binding: 5,
            visibility,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: true,
                min_binding_size: wgpu::BufferSize::new(32),
            },
            count: None,
        };
        let sources = [
            texture(0, wgpu::TextureSampleType::Uint),
            texture(1, wgpu::TextureSampleType::Float { filterable: false }),
            texture(2, wgpu::TextureSampleType::Uint),
            texture(3, wgpu::TextureSampleType::Float { filterable: false }),
        ];
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Packed destination transaction"),
            entries: &if webgl {
                [sources.as_slice(), &[params]].concat()
            } else {
                [sources.as_slice(), &[results, params]].concat()
            },
        });
        // WebGL2 resolve reads the ordered words its feedback pass rendered.
        let results_layout = webgl.then(|| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("WebGL2 packed ordered words"),
                entries: &[texture(0, wgpu::TextureSampleType::Uint)],
            })
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Native packed residue feedback"),
            source: wgpu::ShaderSource::Wgsl(packed_source(webgl).into()),
        });
        let resolve_groups: Vec<_> = std::iter::once(&layout)
            .chain(results_layout.as_ref())
            .map(Some)
            .collect();
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Packed resolve layout"),
            bind_group_layouts: &resolve_groups,
            immediate_size: 0,
        });
        let feedback_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Packed feedback layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let clear_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Packed stencil reset layout"),
            bind_group_layouts: &[],
            immediate_size: 0,
        });
        let full_screen = |entry,
                           layout: &wgpu::PipelineLayout,
                           targets: &[Option<wgpu::ColorTargetState>],
                           depth_stencil| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Packed stencil reset / resolve"),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    targets,
                    compilation_options: Default::default(),
                }),
                primitive: Default::default(),
                depth_stencil,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let depth = |write| {
            Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(write),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            })
        };
        Self {
            device: device.clone(),
            queue: queue.clone(),
            shp: raster(
                &format!(
                    "{}\n{}",
                    crate::render::tactical_shader::SPRITE_SOURCE,
                    include_str!("zsprite_shader.wgsl")
                ),
                include_str!("terrain_packed_shp.wgsl"),
                batch.texture_bind_group_layout(),
                batch.zshape_bind_group_layout(),
            ),
            mask: raster(
                &format!(
                    "{}\n{}",
                    crate::render::tactical_shader::SPRITE_SOURCE,
                    include_str!("zsprite_shader.wgsl")
                ),
                include_str!("terrain_packed_mask.wgsl"),
                batch.texture_bind_group_layout(),
                batch.zshape_bind_group_layout(),
            ),
            ui: raster(
                &format!(
                    "{}\n{}",
                    crate::render::tactical_shader::SPRITE_SOURCE,
                    include_str!("zsprite_shader.wgsl")
                ),
                include_str!("terrain_packed_ui.wgsl"),
                batch.texture_bind_group_layout(),
                batch.zshape_bind_group_layout(),
            ),
            voxel: raster(
                include_str!("sprite_voxel_shader.wgsl"),
                include_str!("terrain_packed_voxel.wgsl"),
                &batch.unit_atlas_bind_group_layout,
                &batch.voxel_palette_bind_group_layout,
            ),
            clear: full_screen("fs_clear", &clear_layout, source_targets, None),
            compute: (!webgl).then(|| {
                device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some("Packed native ordered words"),
                    layout: Some(&feedback_layout),
                    module: &shader,
                    entry_point: Some("cs_main"),
                    compilation_options: Default::default(),
                    cache: None,
                })
            }),
            feedback: webgl.then(|| {
                full_screen(
                    "fs_feedback",
                    &feedback_layout,
                    &[target(wgpu::TextureFormat::R32Uint)],
                    None,
                )
            }),
            results_layout,
            resolve_read: full_screen(
                "fs_resolve",
                &pipeline_layout,
                &[target(format)],
                depth(false),
            ),
            resolve_write: full_screen(
                "fs_resolve",
                &pipeline_layout,
                &[target(format)],
                depth(true),
            ),
            layout,
            stencil: None,
            results: None,
            uniform_chunks: Vec::new(),
            uniform_slot: 0,
            uniform_stride: u64::from(device.limits().min_uniform_buffer_offset_alignment).max(32),
            batches: TerrainBatches::default(),
        }
    }

    pub(super) fn prepare(&mut self, size: wgpu::Extent3d) {
        self.stencil = Some(self.create_stencil(size));
        self.results = None;
    }

    pub(super) fn create_stencil(&self, size: wgpu::Extent3d) -> Stencil {
        let create = |format| {
            self.device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("Packed composite source stencil"),
                    size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        Stencil {
            words: create(wgpu::TextureFormat::R32Uint),
            candidates: create(wgpu::TextureFormat::R32Float),
            results: self.results_layout.as_ref().map(|layout| {
                let results = create(wgpu::TextureFormat::R32Uint);
                let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("WebGL2 packed ordered words"),
                    layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&results),
                    }],
                });
                (results, group)
            }),
        }
    }

    pub(super) fn begin_frame(&mut self) {
        // The previous frame has been submitted before prepare. Queue writes
        // execute after that submission and before this frame's commands.
        // Within a frame every slot is unique, including 128-pass continuations.
        // wgpu 27 api/queue.rs write_buffer execution-order contract:
        // https://github.com/gfx-rs/wgpu/blob/v27.0.1/wgpu/src/api/queue.rs
        self.uniform_slot = 0;
    }

    /// Write `params` to this frame's next uniform slot: its chunk of
    /// `uniform_chunks` and dynamic offset.
    fn uniform_slot(&mut self, params: Params) -> (usize, u32) {
        const SLOTS: usize = 128;
        let chunk = self.uniform_slot / SLOTS;
        let offset = (self.uniform_slot % SLOTS) as u64 * self.uniform_stride;
        if chunk == self.uniform_chunks.len() {
            self.uniform_chunks
                .push(self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("Packed frame uniform slots"),
                    size: self.uniform_stride * SLOTS as u64,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
        }
        self.queue.write_buffer(
            &self.uniform_chunks[chunk],
            offset,
            bytemuck::bytes_of(&params),
        );
        self.uniform_slot += 1;
        (chunk, offset as u32)
    }

    /// The reusable ordered-word results, at least `bytes` long, where
    /// compute orders them; WebGL2 renders them instead.
    fn results(&mut self, bytes: u64) -> Option<wgpu::Buffer> {
        self.compute.as_ref()?;
        if self
            .results
            .as_ref()
            .is_none_or(|buffer| buffer.size() < bytes)
        {
            self.results = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Packed reusable rectangle results"),
                size: bytes,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            }));
        }
        self.results.clone()
    }
}

impl TerrainDrawRenderer {
    /// Conservative clip of one sprite group, `[x, y, width, height]`, as
    /// ordinary sprite draws reach it; `None` when no instance is visible.
    pub(crate) fn sprites_footprint(
        &self,
        group: &[SpriteInstance],
        tactical: [u32; 4],
    ) -> Option<[u32; 4]> {
        group
            .iter()
            .filter_map(|instance| piece_scissor(instance, self.camera, tactical))
            .reduce(union)
    }

    /// Longest adjacent prefix whose complete sprite groups have disjoint
    /// conservative clips. The accumulated union deliberately over-fences;
    /// overlap retains source order, including stores outside a packed marker.
    pub(crate) fn disjoint_sprite_prefix<'a>(
        &self,
        groups: impl IntoIterator<Item = &'a [SpriteInstance]>,
        tactical: [u32; 4],
    ) -> usize {
        let mut bounds: Option<[u32; 4]> = None;
        let mut count = 0;
        for group in groups {
            if let Some(rect) = self.sprites_footprint(group, tactical) {
                if let Some(prior) = bounds {
                    if rect[0] < prior[0] + prior[2]
                        && prior[0] < rect[0] + rect[2]
                        && rect[1] < prior[1] + prior[3]
                        && prior[1] < rect[1] + rect[3]
                    {
                        break;
                    }
                    bounds = Some(union(prior, rect));
                } else {
                    bounds = Some(rect);
                }
            }
            count += 1;
        }
        count
    }

    /// Every pixel one composite reads or writes, colour and Z: its clipped
    /// rect plus the displaced read halo. `None` when it draws nothing.
    pub(crate) fn packed_footprint(
        &self,
        composite: &PackedComposite<'_>,
        tactical: [u32; 4],
    ) -> Option<[u32; 4]> {
        let rect = composite_rect(&composite.pieces, self.camera, tactical)?;
        let offset = effective_offset(composite.pieces.first()?.instance, composite.render_z);
        if offset == 0 {
            return Some(rect);
        }
        let size = self
            .targets
            .as_ref()
            .expect("packed footprint after prepare")
            .source_color
            .size();
        Some(read_halo(rect, offset, size.width, size.height))
    }

    /// Pixel-local composites commute only when their complete conservative
    /// rectangles are disjoint, whatever their selectors and Z policies: each
    /// resolves its own pixels with its own parameters. Reuse the
    /// destination-edit tile scheduler; displaced neighbor reads remain strict
    /// sequential fences.
    pub(crate) fn draw_packed_span(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        color: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        batch: &BatchRenderer,
        buffer: &wgpu::Buffer,
        composites: &[PackedComposite<'_>],
        tactical: [u32; 4],
    ) -> TerrainBatchStats {
        let mut batches = std::mem::take(&mut self.packed.batches);
        let mut stats = TerrainBatchStats::default();
        let mut cursor = 0;
        let mut members = Vec::new();
        while cursor < composites.len() {
            let first = &composites[cursor];
            if first.pieces.is_empty() {
                cursor += 1;
                continue;
            }
            if !packed_joins_waves(first) {
                stats.accumulate(self.draw_packed_composite(
                    encoder,
                    color,
                    depth,
                    batch,
                    buffer,
                    &first.pieces,
                    first.render_z,
                    Some(tactical),
                ));
                cursor += 1;
                continue;
            }
            batches.begin_span([
                self.camera.screen_size[0] as u32,
                self.camera.screen_size[1] as u32,
            ]);
            let mut end = cursor;
            while let Some(composite) = composites.get(end) {
                if !composite.pieces.is_empty() {
                    if !packed_joins_waves(composite) {
                        break;
                    }
                    if let Some(rect) = composite_rect(&composite.pieces, self.camera, tactical) {
                        assert!(batches.push(TerrainCommand::new(end, rect)));
                    }
                }
                end += 1;
            }
            for wave in batches.waves() {
                members.clear();
                members.extend(wave.map(|command| {
                    let composite = &composites[command.draw];
                    Member {
                        pieces: &composite.pieces,
                        render_z: composite.render_z,
                    }
                }));
                stats.accumulate(self.draw_packed_target(
                    encoder,
                    color,
                    depth,
                    buffer,
                    &members,
                    Some(tactical),
                    self.camera,
                    batch.camera_bind_group(),
                    None,
                ));
            }
            stats.tile_dependencies += batches.stats().tile_dependencies;
            cursor = end;
        }
        self.packed.batches = batches;
        stats
    }

    pub(crate) fn draw_packed_composite(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        color: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        batch: &BatchRenderer,
        buffer: &wgpu::Buffer,
        pieces: &[PackedCompositePiece<'_>],
        render_z: RenderZPolicy,
        tactical: Option<[u32; 4]>,
    ) -> TerrainBatchStats {
        self.draw_packed_target(
            encoder,
            color,
            depth,
            buffer,
            &[Member { pieces, render_z }],
            tactical,
            self.camera,
            batch.camera_bind_group(),
            None,
        )
    }

    /// The same packed transaction on an output-owned surface/camera. Sidebar
    /// StripDraw6A9B46 is a no-Z source-index stencil, rather than a tactical
    /// sprite or another brightness implementation.
    pub(crate) fn draw_packed_surface(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        color: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        surface: &PackedSurface,
        camera: CameraUniform,
        camera_group: &wgpu::BindGroup,
        buffer: &wgpu::Buffer,
        pieces: &[PackedCompositePiece<'_>],
    ) -> TerrainBatchStats {
        assert!(pieces.iter().all(|piece| {
            matches!(
                (piece.kind, piece.instance.draw_state.native_selector_bits()),
                (PackedSpriteKind::IndexMask, 1) | (PackedSpriteKind::UiPalette, 4)
            )
        }));
        self.draw_packed_target(
            encoder,
            color,
            depth,
            buffer,
            &[Member {
                pieces,
                render_z: RenderZPolicy::None,
            }],
            None,
            camera,
            camera_group,
            Some(surface),
        )
    }

    /// Replay `members`, pairwise-disjoint composites (a lone one when it
    /// reads displaced neighbours), as one transaction. Every piece is
    /// scissored inside its own composite's rectangle, so outside them
    /// nothing is admitted and the union's other pixels would only discard;
    /// the passes cover just them, and each member's pixels resolve with
    /// only its own snapshot, stencil, selector and Z policy.
    fn draw_packed_target(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        color: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        buffer: &wgpu::Buffer,
        members: &[Member<'_, '_>],
        tactical: Option<[u32; 4]>,
        camera: CameraUniform,
        camera_group: &wgpu::BindGroup,
        surface: Option<&PackedSurface>,
    ) -> TerrainBatchStats {
        let tactical = tactical.unwrap_or([
            0,
            0,
            camera.screen_size[0] as u32,
            camera.screen_size[1] as u32,
        ]);
        // A surface drawn at whole surface pixels per camera pixel clips in
        // camera pixels, then scales: each of its pieces covers the blocks.
        let scale = surface.map_or(1, |surface| surface.scale);
        let scaled = |rect: [u32; 4]| rect.map(|value| value * scale);
        let size = surface
            .map(|surface| &surface.targets)
            .unwrap_or_else(|| self.targets.as_ref().expect("packed draw after prepare"))
            .source_color
            .size();
        let mut transactions = Vec::with_capacity(members.len());
        let mut pieces = 0;
        for member in members {
            let Some(first) = member.pieces.first() else {
                continue;
            };
            let Some(rect) = composite_rect(member.pieces, camera, tactical).map(scaled) else {
                continue;
            };
            pieces += member.pieces.len();
            let params = Params {
                rect,
                surface_width: size.width,
                selector: first.instance.draw_state.native_selector_bits(),
                // Original write-Z selector ignores bit8 (native 495D60/499860).
                offset_words: effective_offset(first.instance, member.render_z),
                reads_depth: u32::from(member.render_z != RenderZPolicy::None),
            };
            // Selector0 is the original opaque copy (494B60), also used when
            // a signed candidate cannot be compared as a wrapped hardware depth.
            let writes = matches!(
                member.render_z,
                RenderZPolicy::ReadWrite | RenderZPolicy::AlphaReadWrite
            );
            transactions.push((member.pieces, params, writes));
        }
        let Some(&(_, first, _)) = transactions.first() else {
            return TerrainBatchStats::default();
        };
        // Only a negative offset reads earlier stores of this transaction, so
        // only it orders its words through `results` (compute or WebGL2
        // feedback). Any other resolve blends its snapshot word directly.
        let ordered = first.offset_words < 0;
        // An ordered resolve reads neighbours' admission anywhere in its
        // rectangle, the union of its pieces.
        assert!(
            !ordered || transactions.len() == 1,
            "an ordered composite replays alone"
        );
        let bytes = if ordered {
            u64::from(first.rect[2]) * u64::from(first.rect[3]) * 4
        } else {
            4
        };
        assert!(
            self.packed.compute.is_none()
                || bytes <= self.packed.device.limits().max_storage_buffer_binding_size,
            "packed source rectangle fits storage binding"
        );
        let results = self.packed.results(bytes);
        let slots: Vec<_> = transactions
            .iter()
            .map(|&(_, params, _)| self.packed.uniform_slot(params))
            .collect();
        let packed = &self.packed;
        let targets = surface
            .map(|surface| &surface.targets)
            .unwrap_or_else(|| self.targets.as_ref().expect("packed draw after prepare"));
        let stencil = surface.map(|surface| &surface.stencil).unwrap_or_else(|| {
            packed
                .stencil
                .as_ref()
                .expect("packed source after prepare")
        });
        let view = |binding, view| wgpu::BindGroupEntry {
            binding,
            resource: wgpu::BindingResource::TextureView(view),
        };
        // One binding per uniform chunk the members' slots lie in; slots are
        // handed out in order, so a chunk's members are adjacent.
        let mut bindings: Vec<(usize, wgpu::BindGroup)> = Vec::new();
        for &(chunk, _) in &slots {
            if bindings.last().is_some_and(|&(last, _)| last == chunk) {
                continue;
            }
            let params = wgpu::BindGroupEntry {
                binding: 5,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &packed.uniform_chunks[chunk],
                    offset: 0,
                    size: wgpu::BufferSize::new(32),
                }),
            };
            let sources = [
                view(0, &targets.words),
                view(1, &targets.depth),
                view(2, &stencil.words),
                view(3, &stencil.candidates),
            ];
            let entries = match &results {
                Some(results) => [
                    sources.as_slice(),
                    &[
                        wgpu::BindGroupEntry {
                            binding: 4,
                            resource: results.as_entire_binding(),
                        },
                        params,
                    ],
                ]
                .concat(),
                None => [sources.as_slice(), &[params]].concat(),
            };
            let transaction = packed.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Packed transaction"),
                layout: &packed.layout,
                entries: &entries,
            });
            bindings.push((chunk, transaction));
        }
        let binding = |chunk: usize| {
            &bindings
                .iter()
                .find(|&&(bound, _)| bound == chunk)
                .expect("every slot's chunk is bound")
                .1
        };
        let attachment = |view| {
            Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })
        };
        // Snapshot read halo as well as stores. No other family or parent can
        // run between this snapshot and its packed resolve.
        let snapshot = |pass: &mut wgpu::RenderPass<'_>, pipeline: &wgpu::RenderPipeline| {
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &targets.source, &[]);
            for &(_, params, _) in &transactions {
                let [x, y, width, height] =
                    read_halo(params.rect, params.offset_words, size.width, size.height);
                pass.set_scissor_rect(x, y, width, height);
                pass.draw(0..3, 0..1);
            }
        };
        let source = |pass: &mut wgpu::RenderPass<'_>| {
            pass.set_pipeline(&packed.clear);
            for &(_, params, _) in &transactions {
                let [x, y, width, height] = params.rect;
                pass.set_scissor_rect(x, y, width, height);
                pass.draw(0..3, 0..1);
            }
            pass.set_vertex_buffer(0, buffer.slice(..));
            pass.set_bind_group(0, camera_group, &[]);
            for &(pieces, params, _) in &transactions {
                for piece in pieces {
                    let Some(clip) = piece_scissor(piece.instance, camera, tactical).map(scaled)
                    else {
                        continue;
                    };
                    assert_eq!(
                        piece.instance.draw_state.native_selector_bits(),
                        params.selector,
                        "one parent has one native selector"
                    );
                    pass.set_pipeline(match piece.kind {
                        PackedSpriteKind::Shp => &packed.shp,
                        PackedSpriteKind::Voxel => &packed.voxel,
                        PackedSpriteKind::IndexMask => &packed.mask,
                        PackedSpriteKind::UiPalette => &packed.ui,
                    });
                    pass.set_bind_group(1, piece.source, &[]);
                    pass.set_bind_group(2, piece.material, &[]);
                    pass.set_scissor_rect(clip[0], clip[1], clip[2], clip[3]);
                    pass.draw(0..6, piece.index..piece.index + 1);
                }
            }
        };
        let scratch = [
            attachment(&stencil.words),
            attachment(&stencil.candidates),
            attachment(&targets.words),
            attachment(&targets.depth),
        ];
        match &self.packed_snapshot_pipeline {
            // The snapshot writes only the destination scratch (attachments 2
            // and 3) from the live attachments, the stencil draws only the
            // stencil (0 and 1), so they share one pass.
            Some(pipeline) => {
                let label = "Packed destination snapshot and admitted source";
                let mut pass = scratch_pass(encoder, label, &scratch);
                snapshot(&mut pass, pipeline);
                source(&mut pass);
            }
            None => {
                let label = "Packed destination snapshot";
                snapshot(
                    &mut scratch_pass(encoder, label, &scratch[2..]),
                    &self.snapshot_pipeline,
                );
                let label = "Whole composite admitted source";
                source(&mut scratch_pass(encoder, label, &scratch[..2]));
            }
        }
        let (chunk, uniform_offset) = slots[0];
        if let (true, Some(feedback), Some((results, _))) =
            (ordered, &packed.feedback, &stencil.results)
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("WebGL2 packed ordered feedback"),
                color_attachments: &[attachment(results)],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(feedback);
            pass.set_bind_group(0, binding(chunk), &[uniform_offset]);
            let [x, y, width, height] = first.rect;
            pass.set_scissor_rect(x, y, width, height);
            pass.draw(0..3, 0..1);
        }
        if let (true, Some(compute)) = (ordered, &packed.compute) {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Native packed ordered feedback"),
                timestamp_writes: None,
            });
            pass.set_pipeline(compute);
            pass.set_bind_group(0, binding(chunk), &[uniform_offset]);
            let groups = first.offset_words.unsigned_abs().div_ceil(64);
            let x = groups.min(packed.device.limits().max_compute_workgroups_per_dimension);
            pass.dispatch_workgroups(x, groups.div_ceil(x), 1);
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Packed whole composite resolve"),
                color_attachments: &[attachment(color)],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            let mut bound = None;
            for (&(_, params, writes), &(chunk, uniform_offset)) in transactions.iter().zip(&slots)
            {
                if bound != Some(writes) {
                    pass.set_pipeline(if writes {
                        &packed.resolve_write
                    } else {
                        &packed.resolve_read
                    });
                    if let (None, Some((_, results))) = (bound, &stencil.results) {
                        pass.set_bind_group(1, results, &[]);
                    }
                    bound = Some(writes);
                }
                pass.set_bind_group(0, binding(chunk), &[uniform_offset]);
                let [x, y, width, height] = params.rect;
                pass.set_scissor_rect(x, y, width, height);
                pass.draw(0..3, 0..1);
            }
        }
        let passes =
            2 + usize::from(ordered) + usize::from(self.packed_snapshot_pipeline.is_none());
        self.submission.note_closed_passes(encoder, passes);
        TerrainBatchStats {
            pieces,
            waves: 1,
            passes,
            tile_dependencies: 0,
        }
    }
}

/// One composite of a packed transaction: its pieces and Z policy.
#[derive(Clone, Copy)]
struct Member<'p, 'a> {
    pieces: &'p [PackedCompositePiece<'a>],
    render_z: RenderZPolicy,
}

/// Whether a non-empty composite may share a wave with others in
/// [`TerrainDrawRenderer::draw_packed_span`], whatever its native selector
/// and Z policy: displaced reads replay alone.
pub(crate) fn packed_joins_waves(composite: &PackedComposite<'_>) -> bool {
    composite
        .pieces
        .first()
        .is_some_and(|piece| effective_offset(piece.instance, composite.render_z) == 0)
}

fn effective_offset(instance: &SpriteInstance, render_z: RenderZPolicy) -> i32 {
    if instance.draw_state.native_selector_bits() & 8 != 0
        && !matches!(
            render_z,
            RenderZPolicy::ReadWrite | RenderZPolicy::AlphaReadWrite
        )
    {
        instance.draw_state.native_offset_words
    } else {
        0
    }
}

fn composite_rect(
    pieces: &[PackedCompositePiece<'_>],
    camera: crate::render::batch::CameraUniform,
    tactical: [u32; 4],
) -> Option<[u32; 4]> {
    pieces
        .iter()
        .filter_map(|piece| piece_scissor(piece.instance, camera, tactical))
        .reduce(union)
}

/// Bounding rectangle of two `[x, y, width, height]` rectangles.
pub(super) fn union(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    let x = a[0].min(b[0]);
    let y = a[1].min(b[1]);
    [
        x,
        y,
        (a[0] + a[2]).max(b[0] + b[2]) - x,
        (a[1] + a[3]).max(b[1] + b[3]) - y,
    ]
}

/// A signed word offset can wrap rows. Snapshot every in-surface endpoint and
/// retain the whole intervening rectangle; this is conservative read coverage.
/// A pass over the packed transaction's stencil and destination scratch.
fn scratch_pass<'e>(
    encoder: &'e mut wgpu::CommandEncoder,
    label: &str,
    color_attachments: &[Option<wgpu::RenderPassColorAttachment<'_>>],
) -> wgpu::RenderPass<'e> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments,
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}

fn read_halo(rect: [u32; 4], offset: i32, width: u32, height: u32) -> [u32; 4] {
    let mut result = rect;
    for y in rect[1]..rect[1] + rect[3] {
        let begin = i64::from(y) * i64::from(width) + i64::from(rect[0]) + i64::from(offset);
        let end = begin + i64::from(rect[2]) - 1;
        let begin = begin.max(0);
        let end = end.min(i64::from(width) * i64::from(height) - 1);
        if begin > end {
            continue;
        }
        let start_y = begin as u32 / width;
        let end_y = end as u32 / width;
        let read = if start_y == end_y {
            [begin as u32 % width, start_y, (end - begin + 1) as u32, 1]
        } else {
            [0, start_y, width, end_y - start_y + 1]
        };
        result = union(result, read);
    }
    result
}
