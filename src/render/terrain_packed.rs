//! Packed SHP / whole Unit final composite. Destination and Z remain owned by
//! TerrainDrawRenderer. Original 73B140, 495590 and 4986D0; executed evidence:
//! tools/procedural_drawing_oracle/translucent_blitter_a.{json,md}.
//!
//! Negative offsets read earlier stores in the same raster span. Each residue
//! modulo abs(offset_words) is independent; its compute invocation walks in
//! native framebuffer word order. No immutable-piece shortcut is used there.

use super::batching::{TerrainBatches, TerrainCommand};
use super::{TerrainBatchStats, TerrainDrawRenderer, piece_scissor};
use crate::render::batch::{BatchRenderer, SPRITE_INSTANCE_ATTRIBUTES, SpriteInstance};
use crate::render::tactical_draw_plan::RenderZPolicy;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PackedSpriteKind {
    Shp,
    Voxel,
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

struct Stencil {
    words: wgpu::TextureView,
    candidates: wgpu::TextureView,
}

pub(super) struct PackedSpriteGpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    shp: wgpu::RenderPipeline,
    voxel: wgpu::RenderPipeline,
    clear: wgpu::RenderPipeline,
    compute: wgpu::ComputePipeline,
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
        let raster = |body: &str,
                      fragment: &str,
                      source_layout: &wgpu::BindGroupLayout,
                      material_layout: &wgpu::BindGroupLayout| {
            // Native signed candidate and projection have one source shared
            // with the opaque path, rather than another geometry/Z port.
            let prefix = &body[..body.find("fn apply_fx(").expect("projection boundary")];
            let source =
                crate::render::tactical_shader::world_source(&format!("{prefix}\n{fragment}"));
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Packed source stencil"),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Packed source layout"),
                bind_group_layouts: &[
                    batch.camera_bind_group_layout(),
                    source_layout,
                    material_layout,
                ],
                push_constant_ranges: &[],
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Packed source raster"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<SpriteInstance>() as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &SPRITE_INSTANCE_ATTRIBUTES,
                    }],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_packed"),
                    targets: &[
                        target(wgpu::TextureFormat::R32Uint),
                        target(wgpu::TextureFormat::R32Float),
                    ],
                    compilation_options: Default::default(),
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview: None,
                cache: None,
            })
        };
        let texture = |binding, sample_type| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE | wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
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
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::COMPUTE | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::COMPUTE | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(32),
                    },
                    count: None,
                },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Native packed residue feedback"),
            source: wgpu::ShaderSource::Wgsl(
                crate::render::tactical_shader::source(include_str!("terrain_packed.wgsl")).into(),
            ),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Packed resolve layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let clear_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Packed stencil reset layout"),
            bind_group_layouts: &[],
            push_constant_ranges: &[],
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
                multiview: None,
                cache: None,
            })
        };
        let depth = |write| {
            Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: write,
                depth_compare: wgpu::CompareFunction::Always,
                stencil: Default::default(),
                bias: Default::default(),
            })
        };
        Self {
            device: device.clone(),
            queue: queue.clone(),
            shp: raster(
                include_str!("zsprite_shader.wgsl"),
                include_str!("terrain_packed_shp.wgsl"),
                batch.texture_bind_group_layout(),
                batch.zshape_bind_group_layout(),
            ),
            voxel: raster(
                include_str!("sprite_voxel_shader.wgsl"),
                include_str!("terrain_packed_voxel.wgsl"),
                &batch.unit_atlas_bind_group_layout,
                &batch.voxel_palette_bind_group_layout,
            ),
            clear: full_screen(
                "fs_clear",
                &clear_layout,
                &[
                    target(wgpu::TextureFormat::R32Uint),
                    target(wgpu::TextureFormat::R32Float),
                ],
                None,
            ),
            compute: device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("Packed native ordered words"),
                layout: Some(&pipeline_layout),
                module: &shader,
                entry_point: Some("cs_main"),
                compilation_options: Default::default(),
                cache: None,
            }),
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
        self.stencil = Some(Stencil {
            words: create(wgpu::TextureFormat::R32Uint),
            candidates: create(wgpu::TextureFormat::R32Float),
        });
        self.results = None;
    }

    pub(super) fn begin_frame(&mut self) {
        // The previous frame has been submitted before prepare. Queue writes
        // execute after that submission and before this frame's commands.
        // Within a frame every slot is unique, including 128-pass continuations.
        // wgpu 27 api/queue.rs write_buffer execution-order contract:
        // https://github.com/gfx-rs/wgpu/blob/v27.0.1/wgpu/src/api/queue.rs
        self.uniform_slot = 0;
    }

    fn transaction_buffers(
        &mut self,
        params: Params,
        result_bytes: u64,
    ) -> (wgpu::Buffer, wgpu::Buffer, u32) {
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
        let uniform = self.uniform_chunks[chunk].clone();
        self.queue
            .write_buffer(&uniform, offset, bytemuck::bytes_of(&params));
        self.uniform_slot += 1;
        if self
            .results
            .as_ref()
            .is_none_or(|buffer| buffer.size() < result_bytes)
        {
            self.results = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Packed reusable rectangle results"),
                size: result_bytes,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            }));
        }
        (
            uniform,
            self.results.as_ref().unwrap().clone(),
            offset as u32,
        )
    }
}

impl TerrainDrawRenderer {
    /// Pixel-local, equal-policy composites commute only when their complete
    /// conservative rectangles are disjoint. Reuse the destination-edit tile
    /// scheduler; displaced neighbor reads remain strict sequential fences.
    #[allow(clippy::too_many_arguments)]
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
        let mut pieces = Vec::new();
        while cursor < composites.len() {
            let first = &composites[cursor];
            let Some(piece) = first.pieces.first() else {
                cursor += 1;
                continue;
            };
            let selector = piece.instance.draw_state.native_selector_bits();
            if effective_offset(piece.instance, first.render_z) != 0 {
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
                if let Some(piece) = composite.pieces.first() {
                    if composite.render_z != first.render_z
                        || piece.instance.draw_state.native_selector_bits() != selector
                        || effective_offset(piece.instance, composite.render_z) != 0
                    {
                        break;
                    }
                    if let Some(rect) = composite_rect(&composite.pieces, self.camera, tactical) {
                        assert!(batches.push(TerrainCommand::new(end, rect)));
                    }
                }
                end += 1;
            }
            for wave in batches.waves() {
                pieces.clear();
                for command in wave {
                    pieces.extend_from_slice(&composites[command.draw].pieces);
                }
                stats.accumulate(self.draw_packed_composite(
                    encoder,
                    color,
                    depth,
                    batch,
                    buffer,
                    &pieces,
                    first.render_z,
                    Some(tactical),
                ));
            }
            stats.tile_dependencies += batches.stats().tile_dependencies;
            cursor = end;
        }
        self.packed.batches = batches;
        stats
    }

    #[allow(clippy::too_many_arguments)]
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
        let Some(first) = pieces.first() else {
            return TerrainBatchStats::default();
        };
        let tactical = tactical.unwrap_or([
            0,
            0,
            self.camera.screen_size[0] as u32,
            self.camera.screen_size[1] as u32,
        ]);
        let rect = composite_rect(pieces, self.camera, tactical);
        let Some(rect) = rect else {
            return TerrainBatchStats::default();
        };
        let selector = first.instance.draw_state.native_selector_bits();
        assert_ne!(selector & 6, 0, "packed selector is required");
        let writes = matches!(
            render_z,
            RenderZPolicy::ReadWrite | RenderZPolicy::AlphaReadWrite
        );
        // Original write-Z selector ignores bit8 (native 495D60/499860).
        let offset = effective_offset(first.instance, render_z);
        let size = self
            .targets
            .as_ref()
            .expect("packed draw after prepare")
            .source_color
            .size();
        let read_rect = read_halo(rect, offset, size.width, size.height);
        let params = Params {
            rect,
            surface_width: size.width,
            selector,
            offset_words: offset,
            reads_depth: u32::from(render_z != RenderZPolicy::None),
        };
        let bytes = u64::from(rect[2]) * u64::from(rect[3]) * 4;
        assert!(
            bytes <= u64::from(self.packed.device.limits().max_storage_buffer_binding_size),
            "packed source rectangle fits storage binding"
        );
        let (params, results, uniform_offset) = self.packed.transaction_buffers(params, bytes);
        let packed = &self.packed;
        let targets = self.targets.as_ref().expect("packed draw after prepare");
        let stencil = packed
            .stencil
            .as_ref()
            .expect("packed source after prepare");
        let view = |binding, view| wgpu::BindGroupEntry {
            binding,
            resource: wgpu::BindingResource::TextureView(view),
        };
        let transaction = packed.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Packed transaction"),
            layout: &packed.layout,
            entries: &[
                view(0, &targets.words),
                view(1, &targets.depth),
                view(2, &stencil.words),
                view(3, &stencil.candidates),
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: results.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &params,
                        offset: 0,
                        size: wgpu::BufferSize::new(32),
                    }),
                },
            ],
        });
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
        {
            // Snapshot read halo as well as stores. No other family or parent
            // can run between this snapshot and its packed resolve.
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Packed destination snapshot"),
                color_attachments: &[attachment(&targets.words), attachment(&targets.depth)],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.snapshot_pipeline);
            pass.set_bind_group(0, &targets.source, &[]);
            pass.set_scissor_rect(read_rect[0], read_rect[1], read_rect[2], read_rect[3]);
            pass.draw(0..3, 0..1);
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Whole composite admitted source"),
                color_attachments: &[attachment(&stencil.words), attachment(&stencil.candidates)],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&packed.clear);
            pass.set_scissor_rect(rect[0], rect[1], rect[2], rect[3]);
            pass.draw(0..3, 0..1);
            pass.set_vertex_buffer(0, buffer.slice(..));
            pass.set_bind_group(0, batch.camera_bind_group(), &[]);
            for piece in pieces {
                let Some(clip) = piece_scissor(piece.instance, self.camera, tactical) else {
                    continue;
                };
                assert_eq!(
                    piece.instance.draw_state.native_selector_bits(),
                    selector,
                    "one parent has one native selector"
                );
                pass.set_pipeline(match piece.kind {
                    PackedSpriteKind::Shp => &packed.shp,
                    PackedSpriteKind::Voxel => &packed.voxel,
                });
                pass.set_bind_group(1, piece.source, &[]);
                pass.set_bind_group(2, piece.material, &[]);
                pass.set_scissor_rect(clip[0], clip[1], clip[2], clip[3]);
                pass.draw(0..6, piece.index..piece.index + 1);
            }
        }
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Native packed ordered feedback"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&packed.compute);
            pass.set_bind_group(0, &transaction, &[uniform_offset]);
            let invocations = if offset < 0 {
                offset.unsigned_abs()
            } else {
                rect[2] * rect[3]
            };
            // A disjoint wave's union can span a supported 4096x4096 viewport.
            // wgpu-types27.0.1 Limits::default caps each dispatch dimension at
            //65535, not its product; keep work within the requested device limit.
            // https://github.com/gfx-rs/wgpu/blob/v27.0.1/wgpu-types/src/lib.rs
            let groups = invocations.div_ceil(64);
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
            });
            pass.set_pipeline(if writes {
                &packed.resolve_write
            } else {
                &packed.resolve_read
            });
            pass.set_bind_group(0, &transaction, &[uniform_offset]);
            pass.set_scissor_rect(rect[0], rect[1], rect[2], rect[3]);
            pass.draw(0..3, 0..1);
        }
        self.submission.note_closed_passes(encoder, 4);
        TerrainBatchStats {
            pieces: pieces.len(),
            waves: 1,
            passes: 4,
            tile_dependencies: 0,
        }
    }
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

fn union(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
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
