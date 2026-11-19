//! Packed SHP / whole Unit final composite. Destination and Z remain owned by
//! TerrainDrawRenderer. Original 73B140, 495590 and 4986D0; executed evidence:
//! tools/procedural_drawing_oracle/translucent_blitter_a.{json,md}.
//!
//! A warp leaf (selector bit 8) blends with the destination word 0..3 to its
//! right, which its span has not stored yet, so every resolve reads only the
//! transaction's snapshot ([`warp_param`]).

use super::batching::{TerrainBatches, TerrainCommand};
use super::{LiveTail, PackedSurface, Tail, TerrainBatchStats, TerrainDrawRenderer, piece_scissor};
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
    selector: u32,
    /// [`warp_param`].
    warp: u32,
    reads_depth: u32,
    /// WGSL rounds the uniform struct up to 32 bytes.
    _pad: u32,
}

pub(super) struct Stencil {
    words: wgpu::TextureView,
    candidates: wgpu::TextureView,
}

/// Native signed candidate and projection have one source shared with the
/// opaque path, rather than another geometry/Z port.
pub(super) fn raster_source(body: &str, fragment: &str) -> String {
    let prefix = &body[..body.find("fn apply_fx(").expect("projection boundary")];
    crate::render::tactical_shader::world_source(&format!("{prefix}\n{fragment}"))
}

/// Packed resolve shader source, the same on every backend.
pub(super) fn packed_source() -> String {
    crate::render::tactical_shader::source(include_str!("terrain_packed.wgsl"))
}

pub(super) struct PackedSpriteGpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    shp: wgpu::RenderPipeline,
    mask: wgpu::RenderPipeline,
    ui: wgpu::RenderPipeline,
    voxel: wgpu::RenderPipeline,
    clear: wgpu::RenderPipeline,
    resolve_read: wgpu::RenderPipeline,
    resolve_write: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    stencil: Option<Stencil>,
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
        let params = wgpu::BindGroupLayoutEntry {
            binding: 4,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: true,
                min_binding_size: wgpu::BufferSize::new(32),
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Packed destination transaction"),
            entries: &[
                texture(0, wgpu::TextureSampleType::Uint),
                texture(1, wgpu::TextureSampleType::Float { filterable: false }),
                texture(2, wgpu::TextureSampleType::Uint),
                texture(3, wgpu::TextureSampleType::Float { filterable: false }),
                params,
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Packed whole composite resolve"),
            source: wgpu::ShaderSource::Wgsl(packed_source().into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Packed resolve layout"),
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
            uniform_chunks: Vec::new(),
            uniform_slot: 0,
            uniform_stride: u64::from(device.limits().min_uniform_buffer_offset_alignment).max(32),
            batches: TerrainBatches::default(),
        }
    }

    pub(super) fn prepare(&mut self, size: wgpu::Extent3d) {
        self.stencil = Some(self.create_stencil(size));
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
    /// rect plus a warp's read halo. `None` when it draws nothing.
    pub(crate) fn packed_footprint(
        &self,
        composite: &PackedComposite<'_>,
        tactical: [u32; 4],
    ) -> Option<[u32; 4]> {
        let rect = composite_rect(&composite.pieces, self.camera, tactical)?;
        if !warps(composite.pieces.first()?.instance, composite.render_z) {
            return Some(rect);
        }
        let size = self
            .targets
            .as_ref()
            .expect("packed footprint after prepare")
            .source_color
            .size();
        Some(read_halo(rect, size.width))
    }

    /// Pixel-local composites commute only when their complete conservative
    /// rectangles are disjoint, whatever their selectors and Z policies: each
    /// resolves its own pixels with its own parameters. Reuse the
    /// destination-edit tile scheduler; a warp's neighbour reads remain strict
    /// sequential fences. The last transaction's resolve is left as the tail.
    pub(crate) fn draw_packed_span(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        color: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        batch: &BatchRenderer,
        buffer: &wgpu::Buffer,
        composites: &[PackedComposite<'_>],
        tactical: [u32; 4],
    ) -> (TerrainBatchStats, Option<LiveTail>) {
        let mut batches = std::mem::take(&mut self.packed.batches);
        let mut stats = TerrainBatchStats::default();
        let mut tail = None;
        let mut cursor = 0;
        let mut members = Vec::new();
        while cursor < composites.len() {
            let first = &composites[cursor];
            if first.pieces.is_empty() {
                cursor += 1;
                continue;
            }
            if !packed_joins_waves(first) {
                stats.accumulate(self.packed_transaction(
                    encoder,
                    color,
                    depth,
                    buffer,
                    &[Member {
                        pieces: &first.pieces,
                        render_z: first.render_z,
                    }],
                    Some(tactical),
                    self.camera,
                    batch.camera_bind_group(),
                    None,
                    &mut tail,
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
                stats.accumulate(self.packed_transaction(
                    encoder,
                    color,
                    depth,
                    buffer,
                    &members,
                    Some(tactical),
                    self.camera,
                    batch.camera_bind_group(),
                    None,
                    &mut tail,
                ));
            }
            stats.tile_dependencies += batches.stats().tile_dependencies;
            cursor = end;
        }
        self.packed.batches = batches;
        (stats, tail.map(|resolve| LiveTail(Tail::Packed(resolve))))
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

    /// [`Self::packed_transaction`] with its resolve in a pass of its own.
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
        let mut tail = None;
        let mut stats = self.packed_transaction(
            encoder,
            color,
            depth,
            buffer,
            members,
            tactical,
            camera,
            camera_group,
            surface,
            &mut tail,
        );
        if let Some(resolve) = tail {
            stats.passes += self.resolve_packed(encoder, color, depth, &resolve);
        }
        stats
    }

    /// Record `members`, pairwise-disjoint composites (a lone one when it
    /// warps), as one transaction: its scratch passes
    /// here, its resolve left in `tail`. A previous resolve in `tail` lands
    /// first: this snapshot reads its pixels and reuses its scratch. Every
    /// piece is scissored inside its own composite's rectangle, so outside
    /// them nothing is admitted and the union's other pixels would only
    /// discard; the passes cover just them, and each member's pixels resolve
    /// with only its own snapshot, stencil, selector and Z policy.
    fn packed_transaction(
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
        tail: &mut Option<PackedResolve>,
    ) -> TerrainBatchStats {
        let tactical = tactical.unwrap_or([
            0,
            0,
            camera.screen_size[0] as u32,
            camera.screen_size[1] as u32,
        ]);
        // A surface's pieces clip in camera pixels, then scale into its own
        // pixels. A whole scale covers whole blocks; the sidebar's Large size
        // (1.5) lays a camera pixel across a fractional span, so its
        // rectangles round outward to keep every covered pixel inside.
        let scale = surface.map_or(1.0, |surface| surface.scale);
        let size = surface
            .map(|surface| &surface.targets)
            .unwrap_or_else(|| self.targets.as_ref().expect("packed draw after prepare"))
            .source_color
            .size();
        let scaled = |rect: [u32; 4]| {
            if scale == 1.0 {
                return rect;
            }
            let left = (rect[0] as f32 * scale).floor() as u32;
            let top = (rect[1] as f32 * scale).floor() as u32;
            let right = (((rect[0] + rect[2]) as f32 * scale).ceil() as u32).min(size.width);
            let bottom = (((rect[1] + rect[3]) as f32 * scale).ceil() as u32).min(size.height);
            [
                left.min(right),
                top.min(bottom),
                right.saturating_sub(left),
                bottom.saturating_sub(top),
            ]
        };
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
            // A warp's parities are in native pixels. Only world draws warp,
            // at scale 1: the sidebar's packed layers (mask and gclock
            // selectors, `draw_packed_surface`) never do.
            debug_assert!(
                scale == 1.0 || !warps(first.instance, member.render_z),
                "a packed warp blit needs native pixels"
            );
            let params = Params {
                rect,
                selector: first.instance.draw_state.native_selector_bits(),
                warp: warp_param(first.instance, member.render_z, rect, tactical),
                reads_depth: u32::from(member.render_z != RenderZPolicy::None),
                _pad: 0,
            };
            // Selector0 is the original opaque copy (494B60), also used when
            // a signed candidate cannot be compared as a wrapped hardware depth.
            let writes = matches!(
                member.render_z,
                RenderZPolicy::ReadWrite | RenderZPolicy::AlphaReadWrite
            );
            transactions.push((member.pieces, params, writes));
        }
        if transactions.is_empty() {
            return TerrainBatchStats::default();
        }
        let mut passes = 0;
        if let Some(previous) = tail.take() {
            passes += self.resolve_packed(encoder, color, depth, &previous);
        }
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
                binding: 4,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &packed.uniform_chunks[chunk],
                    offset: 0,
                    size: wgpu::BufferSize::new(32),
                }),
            };
            let transaction = packed.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Packed transaction"),
                layout: &packed.layout,
                entries: &[
                    view(0, &targets.words),
                    view(1, &targets.depth),
                    view(2, &stencil.words),
                    view(3, &stencil.candidates),
                    params,
                ],
            });
            bindings.push((chunk, transaction));
        }
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
                let [x, y, width, height] = if params.warp == 0 {
                    params.rect
                } else {
                    read_halo(params.rect, size.width)
                };
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
        *tail = Some(PackedResolve {
            members: transactions
                .iter()
                .zip(&slots)
                .map(|(&(_, params, writes), &(chunk, offset))| {
                    (params.rect, chunk, offset, writes)
                })
                .collect(),
            bindings,
        });
        let scratch = 1 + usize::from(self.packed_snapshot_pipeline.is_none());
        self.submission.note_closed_passes(encoder, scratch);
        TerrainBatchStats {
            pieces,
            waves: 1,
            passes: passes + scratch,
            tile_dependencies: 0,
        }
    }

    /// Draw `resolve` in a pass of its own. Returns the passes closed (one).
    pub(super) fn resolve_packed(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        color: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        resolve: &PackedResolve,
    ) -> usize {
        let label = "Packed whole composite resolve";
        self.live_pass(encoder, color, depth, label, |renderer, pass| {
            renderer.record_packed_resolve(pass, resolve);
        })
    }

    /// Record `resolve` into a pass over the live colour and depth.
    pub(super) fn record_packed_resolve(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        resolve: &PackedResolve,
    ) {
        let packed = &self.packed;
        let mut bound = None;
        for &(rect, chunk, uniform_offset, writes) in &resolve.members {
            if bound != Some(writes) {
                pass.set_pipeline(if writes {
                    &packed.resolve_write
                } else {
                    &packed.resolve_read
                });
                bound = Some(writes);
            }
            let (_, binding) = resolve
                .bindings
                .iter()
                .find(|&&(bound, _)| bound == chunk)
                .expect("every slot's chunk is bound");
            pass.set_bind_group(0, binding, &[uniform_offset]);
            let [x, y, width, height] = rect;
            pass.set_scissor_rect(x, y, width, height);
            pass.draw(0..3, 0..1);
        }
    }
}

/// A packed transaction's resolve into the live colour and depth, after its
/// scratch passes: in a pass of its own or first in the next pass over them
/// (`LiveTail`). Nothing else may draw the scratch before it.
pub(super) struct PackedResolve {
    /// Per member, in order: its rectangle, uniform chunk and dynamic
    /// offset, and whether it writes Z.
    members: Vec<([u32; 4], usize, u32, bool)>,
    /// One transaction binding per uniform chunk the members' slots lie in.
    bindings: Vec<(usize, wgpu::BindGroup)>,
}

/// One composite of a packed transaction: its pieces and Z policy.
#[derive(Clone, Copy)]
struct Member<'p, 'a> {
    pieces: &'p [PackedCompositePiece<'a>],
    render_z: RenderZPolicy,
}

/// Whether a non-empty composite may share a wave with others in
/// [`TerrainDrawRenderer::draw_packed_span`], whatever its native selector
/// and Z policy: a warp reads its neighbours' words, so it replays alone.
pub(crate) fn packed_joins_waves(composite: &PackedComposite<'_>) -> bool {
    composite
        .pieces
        .first()
        .is_some_and(|piece| !warps(piece.instance, composite.render_z))
}

/// Whether this draw's leaf reads the word `warp` to its right. Selector bit 8
/// picks a Z-read warp blitter (`BlitTransLucent75AlphaZReadWarp` 0x004953F0
/// and its 50/25 siblings); the Z-writing selector ignores it (0x00495D60,
/// 0x00499860). A colour word sends Bit_Blit 0x004373B0 to the tinted slot
/// (TEST CX,CX at 0x00437894), whose warp thunk (0x00495530) calls the plain
/// leaf with warp 0. Unit 0x0073B140, the voxel body these draws come from,
/// passes DrawVoxelBody's colour word there.
fn warps(instance: &SpriteInstance, render_z: RenderZPolicy) -> bool {
    instance.draw_state.native_selector_bits() & 8 != 0
        && !matches!(
            render_z,
            RenderZPolicy::ReadWrite | RenderZPolicy::AlphaReadWrite
        )
        && instance.palette_light.colour_word() == 0
}

/// The resolve's `warp` uniform: 0 without a warp, else 4 plus the parities
/// the shader's `warp_words` seeds from. Bit_Blit 0x004373B0 measures its
/// first row from the A-buffer's top, whose rect is the tactical view
/// (Set_View_Dimensions 0x004A8960, 0x004A8AB6..0x004A8AD7), and takes the
/// clipped left x on the screen; Build_Tactical_View_Rect 0x0072AD20 puts
/// the stock tactical view at x 0, so VERA measures it from its tactical
/// left. `rect` is the composite's clipped rect, in native pixels.
///
/// RESIDUAL: RLE_Blit 0x00437A10, which VXL_CacheBlit 0x0070750E uses for
/// cached voxels, also XORs the warp by 3 for every source row its clip
/// skips (0x00437D5B), and its tinted slot keeps the warp (0x00498C90).
/// Unit bodies take Bit_Blit, so only a cached voxel clipped an odd number
/// of rows at the tactical top, or a tinted one, ripples in the other phase.
fn warp_param(
    instance: &SpriteInstance,
    render_z: RenderZPolicy,
    rect: [u32; 4],
    tactical: [u32; 4],
) -> u32 {
    if !warps(instance, render_z) {
        return 0;
    }
    4 | ((tactical[1] & 1) << 1) | ((rect[0] ^ tactical[0]) & 1)
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

/// A warp's stores plus the up to three words right of them it reads, within
/// a surface `width` wide.
fn read_halo(rect: [u32; 4], width: u32) -> [u32; 4] {
    [
        rect[0],
        rect[1],
        (rect[2] + 3).min(width - rect[0]),
        rect[3],
    ]
}
