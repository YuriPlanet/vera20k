//! Shared destination-dependent Terrain, Bullet and bridge SHP drawing.
//!
//! Terrain DrawIt 0071C304/0071C34E reaches 004990E0/00497390: signed Z
//! comparison precedes the u16 store, and shadow pixels halve the packed
//! destination. See TERRAIN_STATIC_BODY_SHADOW_NATIVE_2026_09_09.md.
//! Live tactical Depth32Float remains the only Z authority. wgpu 27 requires
//! full-size depth copies, so scissored MRT passes snapshot conservative rects
//! into integer color and float depth scratch while live Z is not attached.
//! Terrain edits use dependency waves. Read-only Bullet edits instead reduce
//! the last admitted body and subsequent shadows in four passes, preserving
//! exact original sequence with immutable Z. Ordinary draw families are fences.

use super::batch::{
    BatchRenderer, BatchTexture, CameraUniform, SPRITE_INSTANCE_ATTRIBUTES, SpriteInstance,
};
use super::tactical_draw_plan::RenderZPolicy;

#[path = "terrain_batch.rs"]
mod batching;
#[path = "terrain_ion_blast.rs"]
mod ion_blasts;
#[path = "terrain_packed.rs"]
mod packed;
#[path = "terrain_submission.rs"]
mod submission;
#[path = "terrain_surface_lines.rs"]
mod surface_lines;
pub(crate) use batching::TerrainBatchStats;
use batching::{TerrainBatches, TerrainCommand};
pub(crate) use ion_blasts::IonBlastDraw;
pub(crate) use packed::{PackedComposite, PackedCompositePiece, PackedSpriteKind, packed_wave_key};
use submission::PassSubmission;
pub(crate) use surface_lines::SurfaceLineBatch;

// High16 bits select the final admitted body; low16 retain its native word.
const READ_ONLY_COMMAND_LIMIT: usize = u16::MAX as usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TerrainPiece {
    Body,
    Shadow,
}

/// Frame-local draw data; atlas identity never participates in ordering.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DestinationEditCommand {
    pub index: u32,
    pub piece: TerrainPiece,
    pub render_z: RenderZPolicy,
    pub atlas_slot: usize,
}

struct Targets {
    source_color: wgpu::Texture,
    source_depth: wgpu::TextureView,
    _encoded_source: wgpu::TextureView,
    words: wgpu::TextureView,
    depth: wgpu::TextureView,
    source: wgpu::BindGroup,
    snapshot: wgpu::BindGroup,
    // Derived scratch, never display authority: snapshot clears every covered
    // pixel before each read-only chunk. prepare replaces it with target handles.
    read_only: wgpu::BindGroup,
    /// WebGL2 resolve inputs: last bodies and shadow counts.
    read_only_resolve: Option<wgpu::BindGroup>,
    /// WebGL2 last-body and shadow-count attachments replacing the atomics.
    read_only_targets: Option<[wgpu::TextureView; 2]>,
    read_only_rows: u32,
}

/// Derived destination scratch for another native surface. The surface owner
/// retains this beside its attachments; switching outputs never replaces the
/// tactical scratch or creates a second destination-edit renderer.
pub(crate) struct PackedSurface {
    targets: Targets,
    stencil: packed::Stencil,
}

pub(crate) struct TerrainDrawRenderer {
    snapshot_pipeline: wgpu::RenderPipeline,
    body_pipeline: wgpu::RenderPipeline,
    shadow_pipeline: wgpu::RenderPipeline,
    read_only_snapshot_pipeline: wgpu::RenderPipeline,
    read_only_body_pipeline: wgpu::RenderPipeline,
    read_only_shadow_pipeline: wgpu::RenderPipeline,
    read_only_resolve_pipeline: wgpu::RenderPipeline,
    #[cfg(test)]
    reference_body_pipeline: wgpu::RenderPipeline,
    #[cfg(test)]
    reference_shadow_pipeline: wgpu::RenderPipeline,
    source_layout: wgpu::BindGroupLayout,
    snapshot_layout: wgpu::BindGroupLayout,
    read_only_layout: wgpu::BindGroupLayout,
    /// The read-only layout itself, or on WebGL2 its bodies plus shadow counts.
    read_only_resolve_layout: wgpu::BindGroupLayout,
    /// Browser WebGL2 fallback: read-only reduction uses render targets.
    webgl: bool,
    targets: Option<Targets>,
    camera: CameraUniform,
    batches: TerrainBatches,
    read_only_commands: Vec<TerrainCommand>,
    submission: PassSubmission,
    surface_lines: surface_lines::SurfaceLineGpu,
    ion_blasts: ion_blasts::IonBlastGpu,
    packed: packed::PackedSpriteGpu,
}

impl TerrainDrawRenderer {
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        batch: &BatchRenderer,
    ) -> Self {
        assert!(
            matches!(
                format,
                wgpu::TextureFormat::Bgra8UnormSrgb | wgpu::TextureFormat::Rgba8UnormSrgb
            ),
            "native terrain output requires the declared sRGB tactical attachment"
        );
        let entry = |binding, sample_type| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let source_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Terrain live attachments"),
            entries: &[
                entry(0, wgpu::TextureSampleType::Float { filterable: false }),
                // GLSL has no texelFetch for depth samplers; the WebGL2
                // fallback loads depth as an unfilterable float texture.
                entry(
                    1,
                    if super::gpu::webgl_fallback(device) {
                        wgpu::TextureSampleType::Float { filterable: false }
                    } else {
                        wgpu::TextureSampleType::Depth
                    },
                ),
            ],
        });
        let snapshot_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Terrain immutable piece snapshot"),
            entries: &[
                entry(0, wgpu::TextureSampleType::Uint),
                entry(1, wgpu::TextureSampleType::Float { filterable: false }),
            ],
        });
        let webgl = super::gpu::webgl_fallback(device);
        let read_only_layout = if webgl {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("WebGL2 read-only last bodies"),
                entries: &[entry(0, wgpu::TextureSampleType::Uint)],
            })
        } else {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Native read-only destination reduction"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            })
        };
        let read_only_resolve_layout = if webgl {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("WebGL2 read-only last bodies and shadow counts"),
                entries: &[
                    entry(0, wgpu::TextureSampleType::Uint),
                    entry(1, wgpu::TextureSampleType::Float { filterable: false }),
                ],
            })
        } else {
            read_only_layout.clone()
        };
        let snapshot_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Terrain encoded RGB565 and native Z snapshot"),
            source: wgpu::ShaderSource::Wgsl(snapshot_source(webgl).into()),
        });
        let snapshot_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Terrain snapshot layout"),
                bind_group_layouts: &[Some(&source_layout)],
                immediate_size: 0,
            });
        // WebGL2 resets its render-target scratch through attachments instead.
        let read_only_snapshot_groups: &[_] = if webgl {
            &[Some(&source_layout)]
        } else {
            &[Some(&source_layout), Some(&read_only_layout)]
        };
        let read_only_snapshot_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Native read-only snapshot/reset layout"),
                bind_group_layouts: read_only_snapshot_groups,
                immediate_size: 0,
            });
        let target = |format| {
            Some(wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })
        };
        let snapshot_targets = [
            target(wgpu::TextureFormat::R32Uint),
            target(wgpu::TextureFormat::R32Float),
            target(wgpu::TextureFormat::R32Uint),
            target(wgpu::TextureFormat::R8Unorm),
        ];
        let snapshot_pipeline = |fragment, layout: &wgpu::PipelineLayout, targets| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Terrain piece snapshot"),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: &snapshot_shader,
                    entry_point: Some("vs_main"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &snapshot_shader,
                    entry_point: Some(fragment),
                    targets,
                    compilation_options: Default::default(),
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Terrain signed Z and packed destination edit"),
            source: wgpu::ShaderSource::Wgsl(
                super::webgl::camera_source(device, edit_source(webgl)).into(),
            ),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Terrain edit layout"),
            bind_group_layouts: &[
                Some(batch.camera_bind_group_layout()),
                Some(batch.texture_bind_group_layout()),
                Some(&snapshot_layout),
            ],
            immediate_size: 0,
        });
        let pipeline = |fragment, depth_write_enabled| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(fragment),
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
                    entry_point: Some(fragment),
                    targets: &[target(format)],
                    compilation_options: Default::default(),
                }),
                primitive: Default::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(depth_write_enabled),
                    // Manual comparison must precede wrapped storage. Hardware Less
                    // on low16(candidate) is not equivalent for signed candidates.
                    depth_compare: Some(wgpu::CompareFunction::Always),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let read_only_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Native read-only admitted body/shadow layout"),
                bind_group_layouts: &[
                    Some(batch.camera_bind_group_layout()),
                    Some(batch.texture_bind_group_layout()),
                    Some(&snapshot_layout),
                    Some(&read_only_layout),
                ],
                immediate_size: 0,
            });
        // The WebGL2 bodies pass writes the R32Uint target its shadows pass
        // reads, so it binds no read-only group.
        let read_only_body_layout = if webgl {
            &device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("WebGL2 read-only last body layout"),
                bind_group_layouts: &[
                    Some(batch.camera_bind_group_layout()),
                    Some(batch.texture_bind_group_layout()),
                    Some(&snapshot_layout),
                ],
                immediate_size: 0,
            })
        } else {
            &read_only_pipeline_layout
        };
        let read_only_pipeline = |fragment, layout, target: wgpu::ColorTargetState| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(fragment),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_read_only"),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<SpriteInstance>() as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &SPRITE_INSTANCE_ATTRIBUTES,
                    })],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fragment),
                    targets: &[Some(target)],
                    compilation_options: Default::default(),
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let resolve_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Native read-only ordered word resolution"),
            source: wgpu::ShaderSource::Wgsl(resolve_source(webgl).into()),
        });
        let resolve_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Native read-only resolve layout"),
            bind_group_layouts: &[Some(&snapshot_layout), Some(&read_only_resolve_layout)],
            immediate_size: 0,
        });
        // Color writes are disabled on ordinary devices; only the scratch atomics change.
        let discarded = wgpu::ColorTargetState {
            format,
            blend: None,
            write_mask: wgpu::ColorWrites::empty(),
        };
        let (body_target, shadow_target) = if webgl {
            let count = wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            };
            (
                wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::R32Uint,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                },
                wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::R8Unorm,
                    blend: Some(wgpu::BlendState {
                        color: count,
                        alpha: count,
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                },
            )
        } else {
            (discarded.clone(), discarded)
        };
        let read_only_resolve_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Native read-only final destination words"),
                layout: Some(&resolve_layout),
                vertex: wgpu::VertexState {
                    module: &resolve_shader,
                    entry_point: Some("vs_main"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &resolve_shader,
                    entry_point: Some("fs_main"),
                    targets: &[target(format)],
                    compilation_options: Default::default(),
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            });
        Self {
            snapshot_pipeline: snapshot_pipeline(
                "fs_main",
                &snapshot_pipeline_layout,
                &snapshot_targets[..2],
            ),
            read_only_snapshot_pipeline: snapshot_pipeline(
                "fs_read_only",
                &read_only_snapshot_layout,
                &snapshot_targets[..if webgl { 4 } else { 2 }],
            ),
            body_pipeline: pipeline("fs_body", true),
            shadow_pipeline: pipeline("fs_shadow", true),
            read_only_body_pipeline: read_only_pipeline(
                "fs_read_only_body",
                read_only_body_layout,
                body_target,
            ),
            read_only_shadow_pipeline: read_only_pipeline(
                "fs_read_only_shadow",
                &read_only_pipeline_layout,
                shadow_target,
            ),
            read_only_resolve_pipeline,
            // Original per-piece transactions remain only as test references.
            #[cfg(test)]
            reference_body_pipeline: pipeline("fs_body", false),
            #[cfg(test)]
            reference_shadow_pipeline: pipeline("fs_shadow", false),
            surface_lines: surface_lines::SurfaceLineGpu::new(device, format, &snapshot_layout),
            ion_blasts: ion_blasts::IonBlastGpu::new(device, format, &snapshot_layout),
            packed: packed::PackedSpriteGpu::new(device, queue, format, batch),
            source_layout,
            snapshot_layout,
            read_only_layout,
            read_only_resolve_layout,
            webgl,
            targets: None,
            batches: TerrainBatches::default(),
            read_only_commands: Vec::new(),
            submission: PassSubmission::new(device, queue),
            camera: CameraUniform {
                screen_size: [1.0, 1.0],
                camera_pos: [0.0, 0.0],
                zoom: 1.0,
                world_origin_y: 0.0,
                world_height: 1.0,
                _pad: 0.0,

                native_z_origin_y: 0.0,
                _native_z_pad: 0.0,
            },
        }
    }

    /// Every pixel one ordinary sprite or destination edit reads or writes:
    /// its quad as the sprite vertex shaders place it, clipped to `tactical`.
    /// `None` when it draws nothing.
    pub(crate) fn piece_footprint(
        &self,
        instance: &SpriteInstance,
        tactical: [u32; 4],
    ) -> Option<[u32; 4]> {
        piece_scissor(instance, self.camera, tactical)
    }

    /// App-owned ordinary passes share the same bounded submission budget.
    /// The caller must have ended its render pass; attachments resume with Load.
    pub(crate) fn note_external_passes(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        passes: usize,
    ) {
        self.submission.note_closed_passes(encoder, passes);
    }

    #[cfg(test)]
    pub(crate) fn submissions(&self) -> usize {
        self.submission.submissions()
    }

    /// Every pass closed since the last `prepare`, ordinary passes included.
    #[cfg(test)]
    pub(crate) fn closed_passes(&self) -> usize {
        self.submission.closed_passes()
    }

    /// Called after composition resize and after the optional upscale depth
    /// swap. Bindings track both concrete source handles, not only dimensions.
    pub(crate) fn prepare(
        &mut self,
        device: &wgpu::Device,
        color: &wgpu::Texture,
        depth: &wgpu::TextureView,
        camera: CameraUniform,
    ) {
        self.submission.reset_frame();
        self.packed.begin_frame();
        self.camera = camera;
        if self
            .targets
            .as_ref()
            .is_some_and(|t| t.source_color == *color && t.source_depth == *depth)
        {
            return;
        }
        self.packed.prepare(color.size());
        self.targets = Some(self.create_targets(device, color, depth));
    }

    /// Attach output-local derived scratch without changing the tactical
    /// camera, frame counters or primary target. Recreate only on resize.
    pub(crate) fn create_packed_surface(
        &self,
        device: &wgpu::Device,
        color: &wgpu::Texture,
        depth: &wgpu::TextureView,
    ) -> PackedSurface {
        PackedSurface {
            targets: self.create_targets(device, color, depth),
            stencil: self.packed.create_stencil(color.size()),
        }
    }

    fn create_targets(
        &self,
        device: &wgpu::Device,
        color: &wgpu::Texture,
        depth: &wgpu::TextureView,
    ) -> Targets {
        assert!(color.usage().contains(wgpu::TextureUsages::TEXTURE_BINDING));
        assert!(
            depth
                .texture()
                .usage()
                .contains(wgpu::TextureUsages::TEXTURE_BINDING)
        );
        assert_eq!(color.size(), depth.texture().size());
        let encoded_source = color.create_view(&wgpu::TextureViewDescriptor {
            label: Some("Terrain encoded source bytes"),
            format: Some(super::webgl::encoded_format(device, color.format())),
            usage: Some(wgpu::TextureUsages::TEXTURE_BINDING),
            ..Default::default()
        });
        let scratch = |format| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("Terrain piece scratch"),
                    size: color.size(),
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
        let words = scratch(wgpu::TextureFormat::R32Uint);
        let scratch_depth = scratch(wgpu::TextureFormat::R32Float);
        let binding = |binding, view| wgpu::BindGroupEntry {
            binding,
            resource: wgpu::BindingResource::TextureView(view),
        };
        let source = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Terrain live snapshot sources"),
            layout: &self.source_layout,
            entries: &[binding(0, &encoded_source), binding(1, depth)],
        });
        let snapshot = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Terrain piece snapshot results"),
            layout: &self.snapshot_layout,
            entries: &[binding(0, &words), binding(1, &scratch_depth)],
        });
        if self.webgl {
            // Whole-surface render targets: one band covers every row.
            let bodies = scratch(wgpu::TextureFormat::R32Uint);
            let shadows = scratch(wgpu::TextureFormat::R8Unorm);
            let read_only = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("WebGL2 read-only last bodies"),
                layout: &self.read_only_layout,
                entries: &[binding(0, &bodies)],
            });
            let read_only_resolve = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("WebGL2 read-only last bodies and shadow counts"),
                layout: &self.read_only_resolve_layout,
                entries: &[binding(0, &bodies), binding(1, &shadows)],
            });
            return Targets {
                source_color: color.clone(),
                source_depth: depth.clone(),
                _encoded_source: encoded_source,
                words,
                depth: scratch_depth,
                source,
                snapshot,
                read_only,
                read_only_resolve: Some(read_only_resolve),
                read_only_targets: Some([bodies, shadows]),
                read_only_rows: color.height(),
            };
        }
        let read_only_rows = color.height().min(
            (device.limits().max_storage_buffer_binding_size / 8 / u64::from(color.width()))
                as u32,
        );
        assert!(
            read_only_rows > 0,
            "one destination row fits a storage binding"
        );
        let state = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Native read-only body ordinal/word and shadow count"),
            size: u64::from(color.width()) * u64::from(read_only_rows) * 8,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let read_only = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Native read-only destination scratch"),
            layout: &self.read_only_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: state.as_entire_binding(),
            }],
        });
        Targets {
            source_color: color.clone(),
            source_depth: depth.clone(),
            _encoded_source: encoded_source,
            words,
            depth: scratch_depth,
            source,
            snapshot,
            read_only,
            read_only_resolve: None,
            read_only_targets: None,
            read_only_rows,
        }
    }

    /// Replay one contiguous destination-edit span across atlas pages. The
    /// caller fences every draw handled by another pipeline family.
    /// ReadWrite overlaps retain dependency waves. A maximal ReadOnly interval
    /// reduces admitted bodies/shadows against one immutable Z snapshot in four
    /// passes; command sequence, never instance/atlas index, selects its order.
    pub(crate) fn draw_span<'a>(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        color: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        batch: &BatchRenderer,
        atlas: impl Fn(usize) -> Option<&'a BatchTexture>,
        buffer: &wgpu::Buffer,
        instances: &[SpriteInstance],
        commands: impl IntoIterator<Item = DestinationEditCommand>,
        tactical: [u32; 4],
    ) -> TerrainBatchStats {
        let size = self.targets.as_ref().map_or([0, 0], |t| {
            [t.source_color.width(), t.source_color.height()]
        });
        self.batches.begin_span(size);
        self.read_only_commands.clear();
        let mut stats = TerrainBatchStats::default();
        for draw in commands {
            assert!(
                matches!(
                    draw.render_z,
                    RenderZPolicy::ReadOnly | RenderZPolicy::ReadWrite
                ),
                "destination edits require an explicit native depth policy"
            );
            // Missing images stay absent and cannot manufacture dependencies.
            // The resolver is a stable frame-local view through every wave.
            if atlas(draw.atlas_slot).is_none() {
                continue;
            }
            let instance = &instances[draw.index as usize];
            let Some(rect) = piece_scissor(instance, self.camera, tactical) else {
                continue;
            };
            let command = TerrainCommand::new(draw, rect);
            if draw.render_z == RenderZPolicy::ReadOnly {
                if self.batches.stats().pieces != 0 {
                    stats.accumulate(
                        self.draw_scheduled(encoder, color, depth, batch, &atlas, buffer),
                    );
                    self.batches.begin_span(size);
                }
                self.read_only_commands.push(command);
                if self.read_only_commands.len() == READ_ONLY_COMMAND_LIMIT {
                    stats.accumulate(self.draw_read_only(encoder, color, batch, &atlas, buffer));
                    self.read_only_commands.clear();
                }
                continue;
            }
            stats.accumulate(self.draw_read_only(encoder, color, batch, &atlas, buffer));
            self.read_only_commands.clear();
            if !self.batches.push(command) {
                // Preserve the prior draw/validation behavior if the original
                // scissor exceeds the prepared attachment; do not index a grid
                // or let a malformed input weaken any overlap dependency.
                stats.accumulate(self.draw_scheduled(encoder, color, depth, batch, &atlas, buffer));
                self.draw_wave(
                    encoder,
                    color,
                    depth,
                    batch,
                    &atlas,
                    buffer,
                    std::iter::once(&command),
                );
                self.submission.note_closed_passes(encoder, 2);
                stats.pieces += 1;
                stats.waves += 1;
                stats.passes += 2;
                self.batches.begin_span(size);
            }
        }
        stats.accumulate(self.draw_scheduled(encoder, color, depth, batch, &atlas, buffer));
        stats.accumulate(self.draw_read_only(encoder, color, batch, &atlas, buffer));
        self.read_only_commands.clear();
        stats
    }

    fn draw_scheduled<'a>(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        color: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        batch: &BatchRenderer,
        atlas: &impl Fn(usize) -> Option<&'a BatchTexture>,
        buffer: &wgpu::Buffer,
    ) -> TerrainBatchStats {
        for wave in self.batches.waves() {
            self.draw_wave(encoder, color, depth, batch, atlas, buffer, wave);
            self.submission.note_closed_passes(encoder, 2);
        }
        self.batches.stats()
    }

    /// Original read-only leaves do not alter admission for any later piece.
    /// Select the last admitted body, count admitted shadows after that body,
    /// and apply the native packed half operation to that body's word (or the
    /// snapshot when there was no body). No depth/ordering authority is added.
    /// Capacity bands reuse bounded scratch for large attachments; separate
    /// pixels are independent, so bands cannot reorder a pixel's operations.
    fn draw_read_only<'texture>(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        color: &wgpu::TextureView,
        batch: &BatchRenderer,
        atlas: &impl Fn(usize) -> Option<&'texture BatchTexture>,
        buffer: &wgpu::Buffer,
    ) -> TerrainBatchStats {
        let commands = &self.read_only_commands;
        if commands.is_empty() {
            return TerrainBatchStats::default();
        }
        let targets = self
            .targets
            .as_ref()
            .expect("prepare precedes destination drawing");
        let attachment = |view| {
            Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })
        };
        let mut passes = 0;
        for top in (0..targets.source_color.height()).step_by(targets.read_only_rows as usize) {
            let bottom = (top + targets.read_only_rows).min(targets.source_color.height());
            let band_rect = |rect: [u32; 4]| {
                let y = rect[1].max(top);
                let end = (rect[1] + rect[3]).min(bottom);
                (end > y).then_some([rect[0], y, rect[2], end.saturating_sub(y)])
            };
            if !commands
                .iter()
                .any(|command| band_rect(command.rect).is_some())
            {
                continue;
            }
            {
                let snapshot = [attachment(&targets.words), attachment(&targets.depth)];
                let webgl_snapshot = targets.read_only_targets.as_ref().map(|[bodies, shadows]| {
                    [
                        attachment(&targets.words),
                        attachment(&targets.depth),
                        attachment(bodies),
                        attachment(shadows),
                    ]
                });
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("Read-only snapshot and covered scratch reset"),
                    color_attachments: webgl_snapshot
                        .as_ref()
                        .map_or(&snapshot[..], |all| &all[..]),
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(&self.read_only_snapshot_pipeline);
                pass.set_bind_group(0, &targets.source, &[]);
                if targets.read_only_targets.is_none() {
                    pass.set_bind_group(1, &targets.read_only, &[]);
                }
                for command in commands {
                    let Some([x, y, width, height]) = band_rect(command.rect) else {
                        continue;
                    };
                    pass.set_scissor_rect(x, y, width, height);
                    pass.draw(0..3, 0..1);
                }
            }
            for (piece, pipeline) in [
                (TerrainPiece::Body, &self.read_only_body_pipeline),
                (TerrainPiece::Shadow, &self.read_only_shadow_pipeline),
            ] {
                // WebGL2 reduces into its own attachments; ordinary devices
                // attach the colour target with writes disabled.
                let output = match (&targets.read_only_targets, piece) {
                    (Some([bodies, _]), TerrainPiece::Body) => bodies,
                    (Some([_, shadows]), TerrainPiece::Shadow) => shadows,
                    (None, _) => color,
                };
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("Read-only admitted source reduction"),
                    color_attachments: &[attachment(output)],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, batch.camera_bind_group(), &[]);
                pass.set_bind_group(2, &targets.snapshot, &[]);
                if targets.read_only_targets.is_none() || piece == TerrainPiece::Shadow {
                    pass.set_bind_group(3, &targets.read_only, &[]);
                }
                pass.set_vertex_buffer(0, buffer.slice(..));
                let mut bound_atlas = None;
                for (ordinal, command) in commands.iter().enumerate() {
                    let draw = command.draw;
                    if draw.piece != piece {
                        continue;
                    }
                    if bound_atlas != Some(draw.atlas_slot) {
                        let texture =
                            atlas(draw.atlas_slot).expect("scheduled atlas remains available");
                        pass.set_bind_group(1, &texture.bind_group, &[]);
                        bound_atlas = Some(draw.atlas_slot);
                    }
                    let Some([x, y, width, height]) = band_rect(command.rect) else {
                        continue;
                    };
                    pass.set_scissor_rect(x, y, width, height);
                    // Sequence is independent of instance storage, page and piece.
                    let vertex = ordinal as u32 * 6;
                    pass.draw(vertex..vertex + 6, draw.index..draw.index + 1);
                }
            }
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("Read-only final ordered destination words"),
                    color_attachments: &[attachment(color)],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(&self.read_only_resolve_pipeline);
                pass.set_bind_group(0, &targets.snapshot, &[]);
                pass.set_bind_group(
                    1,
                    targets
                        .read_only_resolve
                        .as_ref()
                        .unwrap_or(&targets.read_only),
                    &[],
                );
                // Repeated clipped rectangles write the same immutable result.
                // Pixels with no admitted operation discard, preserving live bytes.
                for command in commands {
                    let Some([x, y, width, height]) = band_rect(command.rect) else {
                        continue;
                    };
                    pass.set_scissor_rect(x, y, width, height);
                    pass.draw(0..3, 0..1);
                }
            }
            self.submission.note_closed_passes(encoder, 4);
            passes += 4;
        }
        TerrainBatchStats {
            pieces: commands.len(),
            waves: 1,
            passes,
            tile_dependencies: 0,
        }
    }

    /// Sequential validation reference shares the exact clip and pipelines
    /// with batching; native leaf goldens exercise these shaders.
    #[cfg(test)]
    pub(crate) fn draw_piece(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        color: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        batch: &BatchRenderer,
        atlas: &BatchTexture,
        buffer: &wgpu::Buffer,
        instance: &SpriteInstance,
        draw: DestinationEditCommand,
        tactical: [u32; 4],
    ) {
        let Some(rect) = piece_scissor(instance, self.camera, tactical) else {
            return;
        };
        let command = TerrainCommand::new(draw, rect);
        self.draw_wave(
            encoder,
            color,
            depth,
            batch,
            &|_| Some(atlas),
            buffer,
            std::iter::once(&command),
        );
        self.submission.note_closed_passes(encoder, 2);
    }

    /// Every rectangle is freshly snapped before any member edits the live
    /// attachments. Snapshot/edit use the identical cached absolute scissor;
    /// clipping never resets the SHP row/gradient origin.
    fn draw_wave<'a, 'texture>(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        color: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        batch: &BatchRenderer,
        atlas: &impl Fn(usize) -> Option<&'texture BatchTexture>,
        buffer: &wgpu::Buffer,
        commands: impl Iterator<Item = &'a TerrainCommand> + Clone,
    ) {
        let targets = self
            .targets
            .as_ref()
            .expect("terrain prepare precedes drawing");
        let attachment = |view| {
            Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })
        };
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Terrain snapshot disjoint wave"),
                color_attachments: &[attachment(&targets.words), attachment(&targets.depth)],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.snapshot_pipeline);
            pass.set_bind_group(0, &targets.source, &[]);
            for command in commands.clone() {
                let [x, y, width, height] = command.rect;
                pass.set_scissor_rect(x, y, width, height);
                pass.draw(0..3, 0..1);
            }
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Terrain native disjoint edits"),
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
        pass.set_bind_group(0, batch.camera_bind_group(), &[]);
        pass.set_bind_group(2, &targets.snapshot, &[]);
        pass.set_vertex_buffer(0, buffer.slice(..));
        let mut bound_atlas = None;
        for command in commands {
            let draw = command.draw;
            if bound_atlas != Some(draw.atlas_slot) {
                let texture = atlas(draw.atlas_slot).expect("scheduled atlas remains available");
                pass.set_bind_group(1, &texture.bind_group, &[]);
                bound_atlas = Some(draw.atlas_slot);
            }
            let [x, y, width, height] = command.rect;
            pass.set_scissor_rect(x, y, width, height);
            pass.set_pipeline(match (draw.piece, draw.render_z) {
                (TerrainPiece::Body, RenderZPolicy::ReadWrite) => &self.body_pipeline,
                (TerrainPiece::Shadow, RenderZPolicy::ReadWrite) => &self.shadow_pipeline,
                #[cfg(test)]
                (TerrainPiece::Body, RenderZPolicy::ReadOnly) => &self.reference_body_pipeline,
                #[cfg(test)]
                (TerrainPiece::Shadow, RenderZPolicy::ReadOnly) => &self.reference_shadow_pipeline,
                _ => unreachable!("destination depth policy validated before scheduling"),
            });
            pass.draw(0..6, draw.index..draw.index + 1);
        }
    }
}

/// Snapshot module source. The WebGL2 fallback loads the scene through its
/// sRGB view and resets render-target scratch instead of storage atomics.
fn snapshot_source(webgl: bool) -> String {
    let snapshot = include_str!("terrain_snapshot.wgsl");
    if !webgl {
        return format!("{}\n{}", include_str!("native_packed.wgsl"), snapshot);
    }
    let ordinary = &snapshot[..snapshot
        .find("// Read-only span scratch")
        .expect("read-only snapshot section")];
    crate::render::webgl::encoded_shader(
        &format!(
            "{}\n{ordinary}\n{}",
            include_str!("native_packed.wgsl"),
            WEBGL_READ_ONLY_SNAPSHOT
        ),
        &[
            (
                "textureLoad(scene, p, 0).rgb",
                "fallback_load_encoded(textureLoad(scene, p, 0)).rgb",
            ),
            (
                "var live_depth: texture_depth_2d;",
                "var live_depth: texture_2d<f32>;",
            ),
            (
                "output.depth = textureLoad(live_depth, p, 0);",
                "output.depth = textureLoad(live_depth, p, 0).r;",
            ),
        ],
    )
}

/// WebGL2 read-only reset: the covered pixels' last body and shadow count.
const WEBGL_READ_ONLY_SNAPSHOT: &str = "struct FallbackReadOnlySnapshot {
    @location(0) word: u32,
    @location(1) depth: f32,
    @location(2) body: u32,
    @location(3) shadows: vec4f,
};
@fragment
fn fs_read_only(@builtin(position) position: vec4f) -> FallbackReadOnlySnapshot {
    let snapshot = snapshot_at(position);
    return FallbackReadOnlySnapshot(snapshot.word, snapshot.depth, 0u, vec4f(0.0));
}";

/// Destination edit module source. Without fragment atomics, the WebGL2
/// fallback draws one span's bodies in command order into an R32Uint target,
/// so the last admitted body is the one stored, as `atomicMax` selects; each
/// later admitted shadow adds 1/255 to an R8Unorm count by additive blending.
fn edit_source(webgl: bool) -> String {
    // Reuse the production SHP projection/ABI verbatim, without its shape
    // binding or fragment policy. Both consumers share native_row_z.
    let shp = include_str!("zsprite_shader.wgsl");
    let vertex = shp[..shp
        .find("// One signed candidate owner")
        .expect("SHP projection boundary")]
        .replace("@group(2) @binding(0) var t_zshape: texture_2d<f32>;", "");
    let read_only = include_str!("terrain_read_only.wgsl");
    let read_only = if webgl {
        use crate::render::webgl::patch;
        let read_only = patch(
            read_only,
            "struct ReadOnlyPixel { body: atomic<u32>, shadows: atomic<u32> };
@group(3) @binding(0) var<storage, read_write> read_only_pixels: array<ReadOnlyPixel>;",
            "@group(3) @binding(0) var read_only_bodies: texture_2d<u32>;",
        );
        let read_only = patch(
            &read_only,
            "fn read_only_address(input: VertexOutput) -> u32 {
    let p = vec2u(input.position.xy);
    let width = textureDimensions(old_depth).x;
    let rows = arrayLength(&read_only_pixels) / width;
    return (p.y % rows) * width + p.x;
}",
            "",
        );
        let read_only = patch(
            &read_only,
            "fn fs_read_only_body(input: VertexOutput) -> @location(0) vec4f {",
            "fn fs_read_only_body(input: VertexOutput) -> @location(0) u32 {",
        );
        let read_only = patch(
            &read_only,
            "    atomicMax(&read_only_pixels[read_only_address(input)].body,
        (input.command_ordinal << 16u) | word);
    return vec4f(0.0); // Color writes are disabled; only the scratch atomic changes.",
            "    return (input.command_ordinal << 16u) | word;",
        );
        patch(
            &read_only,
            "    let address = read_only_address(input);
    let last_body = atomicLoad(&read_only_pixels[address].body) >> 16u;
    if input.command_ordinal > last_body {
        atomicAdd(&read_only_pixels[address].shadows, 1u);
    }
    return vec4f(0.0);",
            "    let last_body = textureLoad(read_only_bodies, vec2i(input.position.xy), 0).r >> 16u;
    if input.command_ordinal <= last_body { discard; }
    return vec4f(1.0 / 255.0);",
        )
    } else {
        read_only.to_owned()
    };
    super::tactical_shader::sprite_source(&format!(
        "{vertex}\n{}\n{read_only}",
        include_str!("terrain_edit.wgsl"),
    ))
}

/// Read-only resolve module source; the WebGL2 fallback reads the last body
/// and shadow count from the render-target scratch.
fn resolve_source(webgl: bool) -> String {
    let resolve = include_str!("terrain_read_only_resolve.wgsl");
    if !webgl {
        return super::tactical_shader::source(resolve);
    }
    use crate::render::webgl::patch;
    let resolve = patch(
        resolve,
        "struct ReadOnlyPixel { body: atomic<u32>, shadows: atomic<u32> };
@group(1) @binding(0) var<storage, read_write> read_only_pixels: array<ReadOnlyPixel>;",
        "@group(1) @binding(0) var read_only_bodies: texture_2d<u32>;
@group(1) @binding(1) var read_only_shadows: texture_2d<f32>;",
    );
    let resolve = patch(
        &resolve,
        "    let rows = arrayLength(&read_only_pixels) / width;
    let address = (p.y % rows) * width + p.x;
    let body = atomicLoad(&read_only_pixels[address].body);
    let shadows = atomicLoad(&read_only_pixels[address].shadows);",
        "    let body = textureLoad(read_only_bodies, vec2i(p), 0).r;
    let shadows = u32(round(textureLoad(read_only_shadows, vec2i(p), 0).r * 255.0));",
    );
    super::tactical_shader::source(&resolve)
}

fn piece_scissor(
    instance: &SpriteInstance,
    camera: CameraUniform,
    tactical: [u32; 4],
) -> Option<[u32; 4]> {
    // Include the same half-screen-pixel zoom padding as the SHP vertex path.
    let pad = if (camera.zoom - 1.0).abs() >= 0.001 {
        0.5
    } else {
        0.0
    };
    let left = ((instance.position[0] - camera.camera_pos[0]) * camera.zoom - pad).floor() as i32;
    let top = ((instance.position[1] - camera.camera_pos[1]) * camera.zoom - pad).floor() as i32;
    let right = ((instance.position[0] + instance.size[0] - camera.camera_pos[0]) * camera.zoom
        + pad)
        .ceil() as i32;
    let bottom = ((instance.position[1] + instance.size[1] - camera.camera_pos[1]) * camera.zoom
        + pad)
        .ceil() as i32;
    let x = left.max(tactical[0] as i32).max(0);
    let y = top.max(tactical[1] as i32).max(0);
    let r = right
        .min((tactical[0] + tactical[2]) as i32)
        .min(camera.screen_size[0] as i32);
    let b = bottom
        .min((tactical[1] + tactical[3]) as i32)
        .min(camera.screen_size[1] as i32);
    (r > x && b > y).then_some([x as u32, y as u32, (r - x) as u32, (b - y) as u32])
}

#[cfg(test)]
#[path = "projectile_draw_gpu_tests.rs"]
mod projectile_gpu_tests;

#[cfg(test)]
mod webgl_shader_tests {
    use crate::render::{batch, tactical_shader, webgl::pad_camera_structs};

    /// Every module the browser WebGL2 fallback compiles, as it compiles it.
    fn fallback_sources() -> Vec<(&'static str, String)> {
        let sprite = format!(
            "{}\n{}",
            tactical_shader::SPRITE_SOURCE,
            include_str!("zsprite_shader.wgsl")
        );
        vec![
            (
                "batch",
                pad_camera_structs(&tactical_shader::sprite_source(batch::BATCH_SHADER)),
            ),
            (
                "z depth",
                pad_camera_structs(&tactical_shader::world_source(batch::ZDEPTH_SHADER)),
            ),
            (
                "z sprite",
                pad_camera_structs(&tactical_shader::sprite_source(batch::ZSPRITE_SHADER)),
            ),
            (
                "voxel sprite",
                pad_camera_structs(&tactical_shader::world_source(batch::VOXEL_SPRITE_SHADER)),
            ),
            ("terrain snapshot", super::snapshot_source(true)),
            (
                "terrain edit",
                pad_camera_structs(&super::edit_source(true)),
            ),
            ("read-only resolve", super::resolve_source(true)),
            ("packed composite", super::packed::packed_source(true)),
            (
                "packed SHP",
                pad_camera_structs(&super::packed::raster_source(
                    &sprite,
                    include_str!("terrain_packed_shp.wgsl"),
                )),
            ),
            (
                "packed voxel",
                pad_camera_structs(&super::packed::raster_source(
                    include_str!("sprite_voxel_shader.wgsl"),
                    include_str!("terrain_packed_voxel.wgsl"),
                )),
            ),
            ("surface lines", super::surface_lines::shader_source(true)),
            (
                "ion blast",
                tactical_shader::source(include_str!("terrain_ion_blast.wgsl")),
            ),
            ("scaled copy", include_str!("scaled_copy.wgsl").to_owned()),
            (
                "chrono screen",
                crate::render::chrono_screen::shader_source(true),
            ),
            (
                "shell presentation",
                crate::render::shell_surface_present::shader_source(true),
            ),
            (
                "combat light",
                pad_camera_structs(&crate::render::combat_light::shader_source(true)),
            ),
        ]
    }

    /// The fallback edits the ordinary shader sources; every edit must still
    /// apply, and every entry point must translate to WebGL2's GLSL ES 3.00
    /// as wgpu's GL backend translates it.
    #[test]
    fn webgl_fallback_shaders_translate_to_glsl_es() {
        let mut integer_mix = Vec::new();
        for (name, source) in fallback_sources() {
            let module = naga::front::wgsl::parse_str(&source)
                .unwrap_or_else(|error| panic!("{name}: {}", error.emit_to_string(&source)));
            let info = naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::empty(),
            )
            .validate(&module)
            .unwrap_or_else(|error| panic!("{name}: {error:?}"));
            assert!(
                !source.contains("var<storage") && !source.contains("@compute"),
                "{name} keeps a WebGL2-unsupported resource"
            );
            let functions = module
                .functions
                .iter()
                .map(|(handle, function)| (function, &info[handle]))
                .chain(
                    module
                        .entry_points
                        .iter()
                        .enumerate()
                        .map(|(index, entry)| (&entry.function, info.get_entry_point(index))),
                );
            for (function, function_info) in functions {
                for (handle, expression) in function.expressions.iter() {
                    let naga::Expression::Select { condition, .. } = expression else {
                        continue;
                    };
                    let vector = matches!(
                        function_info[*condition].ty.inner_with(&module.types),
                        naga::TypeInner::Vector { .. }
                    );
                    let float = matches!(
                        function_info[handle].ty.inner_with(&module.types),
                        naga::TypeInner::Vector {
                            scalar: naga::Scalar {
                                kind: naga::ScalarKind::Float,
                                ..
                            },
                            ..
                        }
                    );
                    // naga writes a vector select as mix(), which GLSL ES 3.00
                    // defines for float vectors only.
                    if vector && !float {
                        let line = function
                            .expressions
                            .get_span(handle)
                            .location(&source)
                            .line_number;
                        integer_mix.push(format!(
                            "{name}: {}",
                            source
                                .lines()
                                .nth(line as usize - 1)
                                .unwrap_or_default()
                                .trim()
                        ));
                    }
                }
            }
            for entry in &module.entry_points {
                let options = naga::back::glsl::Options {
                    version: naga::back::glsl::Version::Embedded {
                        version: 300,
                        is_webgl: true,
                    },
                    ..Default::default()
                };
                let pipeline = naga::back::glsl::PipelineOptions {
                    shader_stage: entry.stage,
                    entry_point: entry.name.clone(),
                    multiview: None,
                };
                let mut glsl = String::new();
                naga::back::glsl::Writer::new(
                    &mut glsl,
                    &module,
                    &info,
                    &options,
                    &pipeline,
                    naga::proc::BoundsCheckPolicies::default(),
                )
                .and_then(|mut writer| writer.write().map(drop))
                .unwrap_or_else(|error| panic!("{name} {}: {error}", entry.name));
                assert!(
                    !glsl.contains("webgl_"),
                    "{name} {}: WebGL reserves webgl_ names",
                    entry.name
                );
            }
        }
        integer_mix.dedup();
        assert!(
            integer_mix.is_empty(),
            "GLSL ES 3.00 has no integer mix():\n{}",
            integer_mix.join("\n")
        );
    }
}
