//! Shared destination-dependent Terrain, Bullet and bridge SHP drawing.
//!
//! Terrain DrawIt 0071C304/0071C34E reaches 004990E0/00497390: signed Z
//! comparison precedes the u16 store, and shadow pixels halve the packed
//! destination. See TERRAIN_STATIC_BODY_SHADOW_NATIVE_2026_09_09.md.
//! Live tactical Depth32Float remains the only Z authority. wgpu 27 requires
//! full-size depth copies, so scissored MRT passes snapshot conservative rects
//! into integer color and float depth scratch while live Z is not attached.
//! Terrain edits use dependency waves, in which an edit replays the earlier
//! edits it overlaps per pixel where it can do so exactly. Read-only Bullet
//! edits, and read-write intervals that would need more waves, draw in command
//! order against the live depth test, and opaque ordinary bodies can join such
//! an interval (`draw_interval_tail`). WebGL2, without that path, reduces
//! read-only edits to the last admitted body and subsequent shadows in four
//! passes. Other ordinary draw families are fences.

use super::batch::{
    BatchRenderer, BatchTexture, CameraUniform, SHADOW_COUNT_FORMAT, SPRITE_INSTANCE_ATTRIBUTES,
    SpriteInstance,
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
use batching::{TerrainBatches, TerrainCommand, WINDOW};
pub(crate) use ion_blasts::IonBlastDraw;
pub(crate) use packed::{
    PackedComposite, PackedCompositePiece, PackedSpriteKind, packed_joins_waves,
};
use submission::PassSubmission;
pub(crate) use surface_lines::SurfaceLineBatch;

// WebGL2's reduction: high16 bits select the final admitted body; low16 retain
// its native word.
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

/// One read-write edit of a wave window, as terrain_edit.wgsl's `EditRecord`
/// holds it.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct EditRecord {
    quad: [f32; 4],
    uv: [f32; 4],
    rect: [u32; 4],
    light: [u32; 4],
    params: [u32; 4],
    earlier: [u32; 4],
}

impl EditRecord {
    fn new(instance: &SpriteInstance, command: &TerrainCommand) -> Self {
        let [x, y] = instance.position;
        let [width, height] = instance.size;
        let [u, v] = instance.uv_origin;
        let [uv_width, uv_height] = instance.uv_size;
        Self {
            quad: [x, y, width, height],
            uv: [u, v, uv_width, uv_height],
            rect: command.rect,
            light: instance.palette_light.0,
            params: [
                instance.z_adjust.to_bits(),
                instance.z_gradient,
                instance.source_palette,
                u32::from(command.draw.piece == TerrainPiece::Shadow),
            ],
            earlier: [command.earlier as u32, (command.earlier >> 32) as u32, 0, 0],
        }
    }
}

/// Whether read-write edit `later` can replay `earlier`, which it overlaps,
/// from their wave's shared snapshot (terrain_edit.wgsl `replay_earlier`). It
/// recomputes `earlier`'s pixels from its quad, so that quad must sit on
/// whole pixels at zoom 1 with one texel per pixel: there every pixel centre
/// lies half a texel and half a row inside the interpolated value, and the
/// recomputed pixel equals the one `earlier`'s own fragment draws. Both share
/// one atlas page, the one bound for `later`. A shadow replays only native
/// palette bodies, whose stored word it can recompute exactly.
fn replays<'a>(
    camera: &CameraUniform,
    instances: &[SpriteInstance],
    atlas: &impl Fn(usize) -> Option<&'a BatchTexture>,
    earlier: &DestinationEditCommand,
    later: &DestinationEditCommand,
) -> bool {
    let instance = &instances[earlier.index as usize];
    let whole = |value: f32| value.fract() == 0.0;
    let texel_per_pixel = |page: &BatchTexture| {
        let size = [page.width as f32, page.height as f32];
        (0..2).all(|axis| {
            let origin = instance.uv_origin[axis] * size[axis];
            (instance.uv_size[axis] * size[axis] - instance.size[axis]).abs() < 1.0 / 64.0
                && (origin - origin.round()).abs() < 1.0 / 64.0
        })
    };
    earlier.atlas_slot == later.atlas_slot
        && !(later.piece == TerrainPiece::Shadow
            && earlier.piece == TerrainPiece::Body
            && instance.palette_light.0[0] >> 24 == 0)
        && camera.zoom == 1.0
        && camera.camera_pos.into_iter().all(whole)
        && instance.position.into_iter().all(whole)
        && instance.size.into_iter().all(|v| whole(v) && v >= 1.0)
        && atlas(earlier.atlas_slot).is_some_and(texel_per_pixel)
}

struct Targets {
    source_color: wgpu::Texture,
    source_depth: wgpu::TextureView,
    _encoded_source: wgpu::TextureView,
    words: wgpu::TextureView,
    depth: wgpu::TextureView,
    source: wgpu::BindGroup,
    snapshot: wgpu::BindGroup,
    /// WebGL2's read-only reduction scratch.
    read_only: Option<ReadOnlyTargets>,
    /// Depth-tested edits' shadow counts (`R8Unorm`) and their resolve
    /// binding. Derived scratch: each interval clears it before counting.
    shadow_counts: Option<(wgpu::TextureView, wgpu::BindGroup)>,
}

/// Derived scratch, never display authority: the snapshot clears every covered
/// pixel before each read-only interval. `prepare` replaces it with the targets.
struct ReadOnlyTargets {
    /// Last-body and shadow-count attachments.
    attachments: [wgpu::TextureView; 2],
    /// The last bodies, as the shadows pass reads them.
    bodies: wgpu::BindGroup,
    /// The last bodies and shadow counts, as the resolve reads them.
    resolve: wgpu::BindGroup,
}

/// Edits against the live depth test (`terrain_depth.wgsl`). Absent on WebGL2,
/// which has no independent blend: it keeps the snapshot-and-edit waves and
/// reduces read-only intervals (`ReadOnlyReduction`).
struct DepthEditGpu {
    read_write: DepthEditPipelines,
    /// The same admission without a Z store.
    read_only: DepthEditPipelines,
    resolve: wgpu::RenderPipeline,
    counts_layout: wgpu::BindGroupLayout,
}

struct DepthEditPipelines {
    body: wgpu::RenderPipeline,
    shadow: wgpu::RenderPipeline,
    /// The same pieces' negative-candidate pixels, depth compare Always.
    body_wrapped: wgpu::RenderPipeline,
    shadow_wrapped: wgpu::RenderPipeline,
}

/// WebGL2's read-only intervals (`TerrainDrawRenderer::reduce_read_only`).
struct ReadOnlyReduction {
    snapshot: wgpu::RenderPipeline,
    body: wgpu::RenderPipeline,
    shadow: wgpu::RenderPipeline,
    resolve: wgpu::RenderPipeline,
    bodies_layout: wgpu::BindGroupLayout,
    resolve_layout: wgpu::BindGroupLayout,
}

/// Derived destination scratch for another native surface. The surface owner
/// retains this beside its attachments; switching outputs never replaces the
/// tactical scratch or creates a second destination-edit renderer.
pub(crate) struct PackedSurface {
    targets: Targets,
    stencil: packed::Stencil,
    /// Surface pixels per camera pixel, whole: each camera pixel is a block.
    scale: u32,
}

pub(crate) struct TerrainDrawRenderer {
    snapshot_pipeline: wgpu::RenderPipeline,
    /// The same snapshot into attachments 2 and 3 of the packed source pass,
    /// beside its stencil (`packed::PackedSpriteGpu`). `None` on WebGL2,
    /// which snapshots in a pass of its own.
    packed_snapshot_pipeline: Option<wgpu::RenderPipeline>,
    body_pipeline: wgpu::RenderPipeline,
    shadow_pipeline: wgpu::RenderPipeline,
    /// The browser WebGL2 fallback's read-only reduction.
    read_only: Option<ReadOnlyReduction>,
    #[cfg(test)]
    reference_body_pipeline: wgpu::RenderPipeline,
    #[cfg(test)]
    reference_shadow_pipeline: wgpu::RenderPipeline,
    source_layout: wgpu::BindGroupLayout,
    snapshot_layout: wgpu::BindGroupLayout,
    targets: Option<Targets>,
    camera: CameraUniform,
    batches: TerrainBatches,
    /// One pending read-only interval, flagged as `depth_commands` are.
    read_only_commands: Vec<(TerrainCommand, bool)>,
    /// This frame's read-write wave windows (terrain_edit.wgsl `edits`).
    edit_records: super::uniform_windows::UniformWindows,
    depth: Option<DepthEditGpu>,
    /// One pending read-write interval; true where a piece may have
    /// negative candidates and also draws its wrapped variant.
    depth_commands: Vec<(TerrainCommand, bool)>,
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
        let kept = |format| {
            Some(wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::empty(),
            })
        };
        // The packed stencil words and candidates stay as its draws left them.
        let packed_snapshot_targets = [
            kept(wgpu::TextureFormat::R32Uint),
            kept(wgpu::TextureFormat::R32Float),
            target(wgpu::TextureFormat::R32Uint),
            target(wgpu::TextureFormat::R32Float),
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
        let edit_records = super::uniform_windows::UniformWindows::new(
            device,
            queue,
            "Terrain read-write edit windows",
            1,
            wgpu::ShaderStages::FRAGMENT,
            (WINDOW * std::mem::size_of::<EditRecord>()) as u64,
            16,
        );
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Terrain edit layout"),
            bind_group_layouts: &[
                Some(batch.camera_bind_group_layout()),
                Some(batch.texture_bind_group_layout()),
                Some(&snapshot_layout),
                Some(edit_records.layout()),
            ],
            immediate_size: 0,
        });
        let pipeline = |fragment, depth_write_enabled| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(fragment),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_ordinal"),
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
        let read_only = webgl.then(|| {
            let bodies_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("WebGL2 read-only last bodies"),
                entries: &[entry(0, wgpu::TextureSampleType::Uint)],
            });
            let resolve_layout =
                device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("WebGL2 read-only last bodies and shadow counts"),
                    entries: &[
                        entry(0, wgpu::TextureSampleType::Uint),
                        entry(1, wgpu::TextureSampleType::Float { filterable: false }),
                    ],
                });
            // The bodies pass writes the R32Uint target its shadows pass
            // reads, so it binds no last-bodies group.
            let body_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("WebGL2 read-only last body layout"),
                bind_group_layouts: &[
                    Some(batch.camera_bind_group_layout()),
                    Some(batch.texture_bind_group_layout()),
                    Some(&snapshot_layout),
                ],
                immediate_size: 0,
            });
            let shadow_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("WebGL2 read-only admitted shadow layout"),
                bind_group_layouts: &[
                    Some(batch.camera_bind_group_layout()),
                    Some(batch.texture_bind_group_layout()),
                    Some(&snapshot_layout),
                    Some(&bodies_layout),
                ],
                immediate_size: 0,
            });
            let reduction = |fragment, layout, target: wgpu::ColorTargetState| {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(fragment),
                    layout: Some(layout),
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: Some("vs_ordinal"),
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
            let count = wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            };
            let resolve_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("WebGL2 read-only ordered word resolution"),
                source: wgpu::ShaderSource::Wgsl(resolve_source().into()),
            });
            let resolve_pipeline_layout =
                device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("WebGL2 read-only resolve layout"),
                    bind_group_layouts: &[Some(&snapshot_layout), Some(&resolve_layout)],
                    immediate_size: 0,
                });
            ReadOnlyReduction {
                snapshot: snapshot_pipeline(
                    "fs_read_only",
                    &snapshot_pipeline_layout,
                    &snapshot_targets[..],
                ),
                body: reduction(
                    "fs_read_only_body",
                    &body_layout,
                    wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::R32Uint,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    },
                ),
                shadow: reduction(
                    "fs_read_only_shadow",
                    &shadow_layout,
                    wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::R8Unorm,
                        blend: Some(wgpu::BlendState {
                            color: count,
                            alpha: count,
                        }),
                        write_mask: wgpu::ColorWrites::ALL,
                    },
                ),
                resolve: device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("WebGL2 read-only final destination words"),
                    layout: Some(&resolve_pipeline_layout),
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
                    // Words only: the live depth is attached (`LiveTail`), never
                    // tested or written.
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: wgpu::TextureFormat::Depth32Float,
                        depth_write_enabled: Some(false),
                        depth_compare: Some(wgpu::CompareFunction::Always),
                        stencil: Default::default(),
                        bias: Default::default(),
                    }),
                    multisample: Default::default(),
                    multiview_mask: None,
                    cache: None,
                }),
                bodies_layout,
                resolve_layout,
            }
        });
        let depth = (!webgl).then(|| {
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Terrain depth-tested edit layout"),
                bind_group_layouts: &[
                    Some(batch.camera_bind_group_layout()),
                    Some(batch.texture_bind_group_layout()),
                ],
                immediate_size: 0,
            });
            let add = wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            };
            // A body writes colour and resets its count; a shadow only adds
            // 1/255 to the count. Both store Z through the live depth test,
            // except in a read-only interval.
            let edit = |fragment, shadow: bool, depth_compare, stores_z: bool| {
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
                        targets: &[
                            Some(wgpu::ColorTargetState {
                                format,
                                blend: None,
                                write_mask: if shadow {
                                    wgpu::ColorWrites::empty()
                                } else {
                                    wgpu::ColorWrites::ALL
                                },
                            }),
                            Some(wgpu::ColorTargetState {
                                format: SHADOW_COUNT_FORMAT,
                                blend: shadow.then_some(wgpu::BlendState {
                                    color: add,
                                    alpha: add,
                                }),
                                write_mask: wgpu::ColorWrites::ALL,
                            }),
                        ],
                        compilation_options: Default::default(),
                    }),
                    primitive: Default::default(),
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: wgpu::TextureFormat::Depth32Float,
                        depth_write_enabled: Some(stores_z),
                        depth_compare: Some(depth_compare),
                        stencil: Default::default(),
                        bias: Default::default(),
                    }),
                    multisample: Default::default(),
                    multiview_mask: None,
                    cache: None,
                })
            };
            let counts_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Terrain depth-tested shadow counts"),
                entries: &[entry(
                    0,
                    wgpu::TextureSampleType::Float { filterable: false },
                )],
            });
            let resolve_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Terrain depth-tested shadow halves"),
                source: wgpu::ShaderSource::Wgsl(
                    super::tactical_shader::source(include_str!("terrain_depth_resolve.wgsl"))
                        .into(),
                ),
            });
            let resolve_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Terrain depth-tested resolve layout"),
                bind_group_layouts: &[Some(&snapshot_layout), Some(&counts_layout)],
                immediate_size: 0,
            });
            let resolve = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Terrain depth-tested shadow halves"),
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
                // Words only: the live depth is attached (`LiveTail`), never
                // tested or written.
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(wgpu::CompareFunction::Always),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            });
            let pipelines = |body, body_wrapped, stores_z| {
                use wgpu::CompareFunction::{Always, Less};
                DepthEditPipelines {
                    body: edit(body, false, Less, stores_z),
                    shadow: edit("fs_depth_shadow", true, Less, stores_z),
                    body_wrapped: edit(body_wrapped, false, Always, stores_z),
                    shadow_wrapped: edit("fs_depth_shadow_wrapped", true, Always, stores_z),
                }
            };
            DepthEditGpu {
                read_write: pipelines("fs_depth_body", "fs_depth_body_wrapped", true),
                read_only: pipelines(
                    "fs_depth_read_only_body",
                    "fs_depth_read_only_body_wrapped",
                    false,
                ),
                resolve,
                counts_layout,
            }
        });
        Self {
            snapshot_pipeline: snapshot_pipeline(
                "fs_main",
                &snapshot_pipeline_layout,
                &snapshot_targets[..2],
            ),
            packed_snapshot_pipeline: (!webgl).then(|| {
                snapshot_pipeline(
                    "fs_packed_snapshot",
                    &snapshot_pipeline_layout,
                    &packed_snapshot_targets,
                )
            }),
            body_pipeline: pipeline("fs_body", true),
            shadow_pipeline: pipeline("fs_shadow", true),
            read_only,
            // Original per-piece transactions remain only as test references.
            #[cfg(test)]
            reference_body_pipeline: pipeline("fs_body", false),
            #[cfg(test)]
            reference_shadow_pipeline: pipeline("fs_shadow", false),
            surface_lines: surface_lines::SurfaceLineGpu::new(
                device,
                queue,
                format,
                &snapshot_layout,
            ),
            ion_blasts: ion_blasts::IonBlastGpu::new(device, format, &snapshot_layout),
            packed: packed::PackedSpriteGpu::new(device, queue, format, batch),
            source_layout,
            snapshot_layout,
            targets: None,
            batches: TerrainBatches::default(),
            read_only_commands: Vec::new(),
            edit_records,
            depth,
            depth_commands: Vec::new(),
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

    /// Wall time of the bounded submissions made since the last `prepare`.
    pub(crate) fn frame_submit_time(&self) -> crate::util::time::Duration {
        self.submission.submit_time()
    }

    /// Mark the GPU's spans in the command buffers the bounded submissions
    /// split off, so a split frame's GPU time adds up all of them.
    pub(crate) fn set_gpu_timer(&mut self, timer: crate::render::gpu_timer::GpuTimer) {
        self.submission.timer = timer;
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
        self.edit_records.begin_frame();
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
    /// `scale` is the surface's pixels per camera pixel (the sidebar's at the
    /// window's pixels, `SidebarFrame::in_window`).
    pub(crate) fn create_packed_surface(
        &self,
        device: &wgpu::Device,
        color: &wgpu::Texture,
        depth: &wgpu::TextureView,
        scale: u32,
    ) -> PackedSurface {
        PackedSurface {
            targets: self.create_targets(device, color, depth),
            stencil: self.packed.create_stencil(color.size()),
            scale: scale.max(1),
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
        let shadow_counts = self.depth.as_ref().map(|gpu| {
            let counts = scratch(SHADOW_COUNT_FORMAT);
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Terrain depth-tested shadow counts"),
                layout: &gpu.counts_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&counts),
                }],
            });
            (counts, group)
        });
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
        let read_only = if let Some(gpu) = &self.read_only {
            let bodies = scratch(wgpu::TextureFormat::R32Uint);
            let shadows = scratch(wgpu::TextureFormat::R8Unorm);
            Some(ReadOnlyTargets {
                bodies: device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("WebGL2 read-only last bodies"),
                    layout: &gpu.bodies_layout,
                    entries: &[binding(0, &bodies)],
                }),
                resolve: device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("WebGL2 read-only last bodies and shadow counts"),
                    layout: &gpu.resolve_layout,
                    entries: &[binding(0, &bodies), binding(1, &shadows)],
                }),
                attachments: [bodies, shadows],
            })
        } else {
            None
        };
        Targets {
            source_color: color.clone(),
            source_depth: depth.clone(),
            _encoded_source: encoded_source,
            words,
            depth: scratch_depth,
            source,
            snapshot,
            read_only,
            shadow_counts,
        }
    }

    /// Whether edits can draw against the live depth test: read-write
    /// intervals needing two or more waves, and [`Self::draw_interval_tail`].
    pub(crate) fn depth_tested_edits(&self) -> bool {
        self.depth.is_some()
    }

    /// Test hook: read-write replays and strict waves only, as on WebGL2.
    /// Read-only edits have no other path on a device without its reduction.
    #[cfg(test)]
    pub(crate) fn disable_depth_tested_edits(&mut self) {
        self.depth = None;
    }

    /// Replay one contiguous destination-edit span across atlas pages. The
    /// caller fences every draw handled by another pipeline family.
    /// ReadWrite overlaps retain dependency waves, replaying each other inside
    /// one where they can (`replays`); an interval that still needs two or
    /// more waves draws against the live depth test where the adapter has it
    /// (`draw_read_write`). A maximal ReadOnly interval draws against the
    /// live depth test too, which it leaves as it found it (`draw_read_only`);
    /// command sequence, never instance/atlas index, selects its order.
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
        let (mut stats, tail) = self.draw_span_tail(
            encoder, color, depth, batch, &atlas, buffer, instances, commands, tactical,
        );
        if let Some(tail) = tail {
            stats.passes += self.flush_tail(encoder, color, depth, tail, batch, &atlas, buffer);
        }
        stats
    }

    /// [`Self::draw_span`], leaving the draws of its last pass, which only
    /// write the live attachments, for the caller's next pass over them.
    pub(crate) fn draw_span_tail<'a>(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        color: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        batch: &BatchRenderer,
        atlas: &impl Fn(usize) -> Option<&'a BatchTexture>,
        buffer: &wgpu::Buffer,
        instances: &[SpriteInstance],
        commands: impl IntoIterator<Item = DestinationEditCommand>,
        tactical: [u32; 4],
    ) -> (TerrainBatchStats, Option<LiveTail>) {
        let size = self.targets.as_ref().map_or([0, 0], |t| {
            [t.source_color.width(), t.source_color.height()]
        });
        self.batches.begin_span(size);
        self.read_only_commands.clear();
        let mut stats = TerrainBatchStats::default();
        let mut tail = None;
        for draw in commands {
            let Some((command, wraps)) = self.admit_edit(draw, atlas, instances, tactical) else {
                continue;
            };
            if draw.render_z == RenderZPolicy::ReadOnly {
                if self.batches.stats().pieces != 0 {
                    stats.accumulate(self.draw_read_write(
                        encoder, color, depth, batch, atlas, buffer, instances, &mut tail,
                    ));
                    self.batches.begin_span(size);
                }
                self.read_only_commands.push((command, wraps));
                if self.read_only.is_some()
                    && self.read_only_commands.len() == READ_ONLY_COMMAND_LIMIT
                {
                    stats.accumulate(
                        self.draw_read_only(encoder, color, depth, batch, atlas, buffer, &mut tail),
                    );
                }
                continue;
            }
            stats.accumulate(
                self.draw_read_only(encoder, color, depth, batch, atlas, buffer, &mut tail),
            );
            let camera = self.camera;
            if self.batches.push_replaying(command, |earlier, later| {
                replays(&camera, instances, atlas, earlier, later)
            }) {
                if self.depth.is_some() {
                    self.depth_commands.push((command, wraps));
                }
            } else {
                // Preserve the prior draw/validation behavior if the original
                // scissor exceeds the prepared attachment; do not index a grid
                // or let a malformed input weaken any overlap dependency.
                stats.accumulate(self.draw_read_write(
                    encoder, color, depth, batch, atlas, buffer, instances, &mut tail,
                ));
                stats.passes += self.draw_wave(
                    encoder,
                    color,
                    depth,
                    batch,
                    atlas,
                    buffer,
                    vec![command],
                    |command| EditRecord::new(&instances[command.draw.index as usize], command),
                    &mut tail,
                );
                stats.pieces += 1;
                stats.waves += 1;
                self.batches.begin_span(size);
            }
        }
        stats.accumulate(self.draw_read_write(
            encoder, color, depth, batch, atlas, buffer, instances, &mut tail,
        ));
        stats.accumulate(
            self.draw_read_only(encoder, color, depth, batch, atlas, buffer, &mut tail),
        );
        (stats, tail)
    }

    /// One interval of destination edits and opaque bodies in parent order,
    /// in a single pass against the live depth test, leaving its shadow
    /// resolve as the tail. A body restarts its pixels' shadow counts as a
    /// body edit does (`fs_counted`), so the words snapshot and resolve leave
    /// what the original sequence of stores and halvings leaves. VERA
    /// scheduling only: `draw_body` records the caller's body `u32`.
    pub(crate) fn draw_interval_tail<'a>(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        color: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        batch: &BatchRenderer,
        atlas: &impl Fn(usize) -> Option<&'a BatchTexture>,
        buffer: &wgpu::Buffer,
        instances: &[SpriteInstance],
        draws: &[IntervalDraw],
        tactical: [u32; 4],
        draw_body: impl FnMut(&mut wgpu::RenderPass<'_>, u32),
    ) -> (TerrainBatchStats, Option<LiveTail>) {
        let steps: Vec<_> = draws
            .iter()
            .filter_map(|&draw| match draw {
                IntervalDraw::Body(body) => Some(DepthStep::Body(body)),
                IntervalDraw::Edit(edit) => self
                    .admit_edit(edit, atlas, instances, tactical)
                    .map(|(command, wraps)| DepthStep::Edit(command, wraps)),
            })
            .collect();
        let mut tail = None;
        let stats = self.draw_depth_run(
            encoder,
            color,
            depth,
            batch,
            atlas,
            buffer,
            steps.iter().copied(),
            draw_body,
            &mut tail,
        );
        (stats, tail)
    }

    /// The scheduled form of an edit, and whether it also draws its wrapped
    /// variant; `None` when it can draw nothing.
    fn admit_edit<'a>(
        &self,
        draw: DestinationEditCommand,
        atlas: &impl Fn(usize) -> Option<&'a BatchTexture>,
        instances: &[SpriteInstance],
        tactical: [u32; 4],
    ) -> Option<(TerrainCommand, bool)> {
        assert!(
            matches!(
                draw.render_z,
                RenderZPolicy::ReadOnly | RenderZPolicy::ReadWrite
            ),
            "destination edits require an explicit native depth policy"
        );
        // Missing images stay absent and cannot manufacture dependencies.
        // The resolver is a stable frame-local view through every wave.
        atlas(draw.atlas_slot)?;
        let instance = &instances[draw.index as usize];
        let rect = piece_scissor(instance, self.camera, tactical)?;
        let mut wraps = false;
        if self.depth.is_some() {
            let low = candidate_range(instance, self.camera)[0];
            // Above 65535 nothing is admitted; only negative rows wrap.
            if low > 65535 {
                return None;
            }
            wraps = low < 0;
        }
        Some((TerrainCommand::new(draw, rect), wraps))
    }

    /// The pending read-write interval. One wave, its overlaps replayed
    /// inside it, keeps its snapshot and edit (one pass and a tail); an
    /// interval needing more waves draws in one pass against the live depth
    /// test, plus a words pass when shadows halve.
    fn draw_read_write<'texture>(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        color: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        batch: &BatchRenderer,
        atlas: &impl Fn(usize) -> Option<&'texture BatchTexture>,
        buffer: &wgpu::Buffer,
        instances: &[SpriteInstance],
        tail: &mut Option<LiveTail>,
    ) -> TerrainBatchStats {
        let stats = if self.batches.stats().waves > 1 && !self.depth_commands.is_empty() {
            let commands = std::mem::take(&mut self.depth_commands);
            let stats = self.draw_depth_run(
                encoder,
                color,
                depth,
                batch,
                atlas,
                buffer,
                edit_steps(&commands),
                no_bodies,
                tail,
            );
            self.depth_commands = commands;
            stats
        } else {
            self.draw_scheduled(encoder, color, depth, batch, atlas, buffer, instances, tail)
        };
        self.depth_commands.clear();
        stats
    }

    /// One maximal interval in a single pass against the live depth test, in
    /// command order, instead of a snapshot/edit wave per overlap layer.
    /// Admission and any stored Z are the original leaves'
    /// (`terrain_depth.wgsl`). Shadows count per pixel instead of halving;
    /// a words snapshot of the result and the resolve tail then halve each
    /// pixel once per shadow admitted after its last admitted body, which is
    /// what the original sequence of body stores and halvings leaves.
    fn draw_depth_run<'texture>(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        color: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        batch: &BatchRenderer,
        atlas: &impl Fn(usize) -> Option<&'texture BatchTexture>,
        buffer: &wgpu::Buffer,
        steps: impl Iterator<Item = DepthStep> + Clone,
        mut draw_body: impl FnMut(&mut wgpu::RenderPass<'_>, u32),
        tail: &mut Option<LiveTail>,
    ) -> TerrainBatchStats {
        let mut passes = 0;
        // The previous tail lands first; it also still reads the scratch.
        if let Some(previous) = tail.take() {
            passes += self.flush_tail(encoder, color, depth, previous, batch, atlas, buffer);
        }
        let gpu = self
            .depth
            .as_ref()
            .expect("depth-tested edits have pipelines");
        let targets = self
            .targets
            .as_ref()
            .expect("prepare precedes destination drawing");
        let (counts, _) = targets
            .shadow_counts
            .as_ref()
            .expect("depth-tested edits have shadow counts");
        let attachment = |view, load| {
            Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load,
                    store: wgpu::StoreOp::Store,
                },
            })
        };
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Terrain depth-tested edits"),
                color_attachments: &[
                    attachment(color, wgpu::LoadOp::Load),
                    attachment(counts, wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)),
                ],
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
            pass.set_vertex_buffer(0, buffer.slice(..));
            let mut bound_atlas = None;
            for step in steps.clone() {
                let (command, wraps) = match step {
                    DepthStep::Edit(command, wraps) => (command, wraps),
                    DepthStep::Body(body) => {
                        draw_body(&mut pass, body);
                        bound_atlas = None;
                        continue;
                    }
                };
                let draw = command.draw;
                if bound_atlas != Some(draw.atlas_slot) {
                    let texture =
                        atlas(draw.atlas_slot).expect("scheduled atlas remains available");
                    pass.set_bind_group(1, &texture.bind_group, &[]);
                    bound_atlas = Some(draw.atlas_slot);
                }
                let [x, y, width, height] = command.rect;
                pass.set_scissor_rect(x, y, width, height);
                let pipelines = if draw.render_z == RenderZPolicy::ReadOnly {
                    &gpu.read_only
                } else {
                    &gpu.read_write
                };
                let (pipeline, wrapped) = match draw.piece {
                    TerrainPiece::Body => (&pipelines.body, &pipelines.body_wrapped),
                    TerrainPiece::Shadow => (&pipelines.shadow, &pipelines.shadow_wrapped),
                };
                pass.set_pipeline(pipeline);
                pass.draw(0..6, draw.index..draw.index + 1);
                // The two variants admit disjoint rows of the same piece.
                if wraps {
                    pass.set_pipeline(wrapped);
                    pass.draw(0..6, draw.index..draw.index + 1);
                }
            }
        }
        self.submission.note_closed_passes(encoder, 1);
        passes += 1;
        let shadows: Vec<_> = steps
            .clone()
            .filter_map(|step| match step {
                DepthStep::Edit(command, _) if command.draw.piece == TerrainPiece::Shadow => {
                    Some(command.rect)
                }
                _ => None,
            })
            .collect();
        if !shadows.is_empty() {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Terrain depth-tested shadow words"),
                color_attachments: &[
                    attachment(&targets.words, wgpu::LoadOp::Load),
                    attachment(&targets.depth, wgpu::LoadOp::Load),
                ],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.snapshot_pipeline);
            pass.set_bind_group(0, &targets.source, &[]);
            for &[x, y, width, height] in &shadows {
                pass.set_scissor_rect(x, y, width, height);
                pass.draw(0..3, 0..1);
            }
            drop(pass);
            self.submission.note_closed_passes(encoder, 1);
            passes += 1;
            // Repeated clipped rectangles write the same immutable result.
            *tail = Some(LiveTail(Tail::Shadows(shadows)));
        }
        TerrainBatchStats {
            pieces: steps
                .filter(|step| matches!(step, DepthStep::Edit(..)))
                .count(),
            waves: 1,
            passes,
            tile_dependencies: 0,
        }
    }

    fn draw_scheduled<'a>(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        color: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        batch: &BatchRenderer,
        atlas: &impl Fn(usize) -> Option<&'a BatchTexture>,
        buffer: &wgpu::Buffer,
        instances: &[SpriteInstance],
        tail: &mut Option<LiveTail>,
    ) -> TerrainBatchStats {
        let batches = std::mem::take(&mut self.batches);
        let mut stats = batches.stats();
        stats.passes = 0;
        for wave in batches.waves() {
            stats.passes += self.draw_wave(
                encoder,
                color,
                depth,
                batch,
                atlas,
                buffer,
                wave.copied().collect(),
                |command| EditRecord::new(&instances[command.draw.index as usize], command),
                tail,
            );
        }
        self.batches = batches;
        stats
    }

    /// One maximal read-only interval, which leaves the pending commands
    /// empty. Its pieces never change Z, so the live depth test admits each
    /// as a snapshot of the Z it began with would.
    fn draw_read_only<'texture>(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        color: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        batch: &BatchRenderer,
        atlas: &impl Fn(usize) -> Option<&'texture BatchTexture>,
        buffer: &wgpu::Buffer,
        tail: &mut Option<LiveTail>,
    ) -> TerrainBatchStats {
        if self.read_only_commands.is_empty() {
            return TerrainBatchStats::default();
        }
        let mut commands = std::mem::take(&mut self.read_only_commands);
        let stats = if self.read_only.is_some() {
            self.reduce_read_only(encoder, color, depth, batch, atlas, buffer, &commands, tail)
        } else {
            self.draw_depth_run(
                encoder,
                color,
                depth,
                batch,
                atlas,
                buffer,
                edit_steps(&commands),
                no_bodies,
                tail,
            )
        };
        commands.clear();
        self.read_only_commands = commands;
        stats
    }

    /// WebGL2's read-only interval, in four passes. Original read-only leaves
    /// do not alter admission for any later piece. Select the last admitted
    /// body, count admitted shadows after that body, and apply the native
    /// packed half operation to that body's word (or the snapshot when there
    /// was no body). No depth/ordering authority is added. The final words
    /// are the tail.
    fn reduce_read_only<'texture>(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        color: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        batch: &BatchRenderer,
        atlas: &impl Fn(usize) -> Option<&'texture BatchTexture>,
        buffer: &wgpu::Buffer,
        commands: &[(TerrainCommand, bool)],
        tail: &mut Option<LiveTail>,
    ) -> TerrainBatchStats {
        let mut passes = 0;
        if let Some(previous) = tail.take() {
            passes += self.flush_tail(encoder, color, depth, previous, batch, atlas, buffer);
        }
        let gpu = self.read_only.as_ref().expect("the WebGL2 reduction");
        let targets = self
            .targets
            .as_ref()
            .expect("prepare precedes destination drawing");
        let scratch = targets.read_only.as_ref().expect("the WebGL2 scratch");
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
        let [bodies, shadows] = &scratch.attachments;
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Read-only snapshot and covered scratch reset"),
                color_attachments: &[
                    attachment(&targets.words),
                    attachment(&targets.depth),
                    attachment(bodies),
                    attachment(shadows),
                ],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&gpu.snapshot);
            pass.set_bind_group(0, &targets.source, &[]);
            for (command, _) in commands {
                let [x, y, width, height] = command.rect;
                pass.set_scissor_rect(x, y, width, height);
                pass.draw(0..3, 0..1);
            }
        }
        for (piece, pipeline, output) in [
            (TerrainPiece::Body, &gpu.body, bodies),
            (TerrainPiece::Shadow, &gpu.shadow, shadows),
        ] {
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
            if piece == TerrainPiece::Shadow {
                pass.set_bind_group(3, &scratch.bodies, &[]);
            }
            pass.set_vertex_buffer(0, buffer.slice(..));
            let mut bound_atlas = None;
            for (ordinal, (command, _)) in commands.iter().enumerate() {
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
                let [x, y, width, height] = command.rect;
                pass.set_scissor_rect(x, y, width, height);
                // Sequence is independent of instance storage, page and piece.
                let vertex = ordinal as u32 * 6;
                pass.draw(vertex..vertex + 6, draw.index..draw.index + 1);
            }
        }
        self.submission.note_closed_passes(encoder, 3);
        // Repeated clipped rectangles write the same immutable result.
        let rects = commands.iter().map(|(command, _)| command.rect).collect();
        *tail = Some(LiveTail(Tail::ReadOnly(rects)));
        TerrainBatchStats {
            pieces: commands.len(),
            waves: 1,
            passes: passes + 3,
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
        let atlas = |_| Some(atlas);
        let mut tail = None;
        let commands = vec![TerrainCommand::new(draw, rect)];
        self.draw_wave(
            encoder,
            color,
            depth,
            batch,
            &atlas,
            buffer,
            commands,
            |command| EditRecord::new(instance, command),
            &mut tail,
        );
        if let Some(tail) = tail {
            self.flush_tail(encoder, color, depth, tail, batch, &atlas, buffer);
        }
    }

    /// Snapshot one wave of `commands` and leave their edits as the tail:
    /// every rectangle is freshly snapped before any member edits the live
    /// attachments, and an earlier tail lands before the snapshot. Members
    /// that overlap replay their earlier overlaps from `record`'s window
    /// records ([`TerrainCommand::earlier`]); the rest are disjoint.
    /// Snapshot and edit use the identical cached absolute scissor; clipping
    /// never resets the SHP row/gradient origin. Returns the passes closed.
    fn draw_wave<'texture>(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        color: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        batch: &BatchRenderer,
        atlas: &impl Fn(usize) -> Option<&'texture BatchTexture>,
        buffer: &wgpu::Buffer,
        commands: Vec<TerrainCommand>,
        record: impl Fn(&TerrainCommand) -> EditRecord,
        tail: &mut Option<LiveTail>,
    ) -> usize {
        let mut passes = 0;
        if let Some(previous) = tail.take() {
            passes += self.flush_tail(encoder, color, depth, previous, batch, atlas, buffer);
        }
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
                label: Some("Terrain snapshot wave"),
                color_attachments: &[attachment(&targets.words), attachment(&targets.depth)],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.snapshot_pipeline);
            pass.set_bind_group(0, &targets.source, &[]);
            for command in &commands {
                let [x, y, width, height] = command.rect;
                pass.set_scissor_rect(x, y, width, height);
                pass.draw(0..3, 0..1);
            }
        }
        self.submission.note_closed_passes(encoder, 1);
        let windows = commands
            .chunks(WINDOW)
            .map(|window| {
                // Where nothing replays, the shader reads only empty `earlier`s.
                if window.iter().all(|command| command.earlier == 0) {
                    return self.edit_records.zeros();
                }
                let records: Vec<_> = window.iter().map(&record).collect();
                self.edit_records.write(bytemuck::cast_slice(&records))
            })
            .collect();
        *tail = Some(LiveTail(Tail::Edits(commands, windows)));
        passes + 1
    }

    /// Record `tail` into `pass`, which has the live colour and depth
    /// attached: the tail's draws come first, as in a pass of their own.
    pub(crate) fn record_tail<'texture>(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        tail: &LiveTail,
        batch: &BatchRenderer,
        atlas: &impl Fn(usize) -> Option<&'texture BatchTexture>,
        buffer: &wgpu::Buffer,
    ) {
        let targets = self
            .targets
            .as_ref()
            .expect("terrain prepare precedes drawing");
        match &tail.0 {
            Tail::Packed(resolve) => self.record_packed_resolve(pass, resolve),
            Tail::Edits(commands, windows) => {
                pass.set_bind_group(0, batch.camera_bind_group(), &[]);
                pass.set_bind_group(2, &targets.snapshot, &[]);
                pass.set_vertex_buffer(0, buffer.slice(..));
                let mut bound_atlas = None;
                for (window, (records, offset)) in commands.chunks(WINDOW).zip(windows) {
                    pass.set_bind_group(3, records, &[*offset]);
                    for (slot, command) in window.iter().enumerate() {
                        let draw = command.draw;
                        if bound_atlas != Some(draw.atlas_slot) {
                            let texture =
                                atlas(draw.atlas_slot).expect("scheduled atlas remains available");
                            pass.set_bind_group(1, &texture.bind_group, &[]);
                            bound_atlas = Some(draw.atlas_slot);
                        }
                        let [x, y, width, height] = command.rect;
                        pass.set_scissor_rect(x, y, width, height);
                        pass.set_pipeline(match (draw.piece, draw.render_z) {
                            (TerrainPiece::Body, RenderZPolicy::ReadWrite) => &self.body_pipeline,
                            (TerrainPiece::Shadow, RenderZPolicy::ReadWrite) => {
                                &self.shadow_pipeline
                            }
                            #[cfg(test)]
                            (TerrainPiece::Body, RenderZPolicy::ReadOnly) => {
                                &self.reference_body_pipeline
                            }
                            #[cfg(test)]
                            (TerrainPiece::Shadow, RenderZPolicy::ReadOnly) => {
                                &self.reference_shadow_pipeline
                            }
                            _ => {
                                unreachable!("destination depth policy validated before scheduling")
                            }
                        });
                        // The vertex range carries the edit's window slot.
                        let vertex = slot as u32 * 6;
                        pass.draw(vertex..vertex + 6, draw.index..draw.index + 1);
                    }
                }
            }
            Tail::Shadows(rects) => {
                let gpu = self
                    .depth
                    .as_ref()
                    .expect("depth-tested edits have pipelines");
                let (_, counts) = targets
                    .shadow_counts
                    .as_ref()
                    .expect("depth-tested edits have shadow counts");
                pass.set_pipeline(&gpu.resolve);
                pass.set_bind_group(0, &targets.snapshot, &[]);
                pass.set_bind_group(1, counts, &[]);
                // Pixels without a counted shadow discard, keeping live bytes.
                for &[x, y, width, height] in rects {
                    pass.set_scissor_rect(x, y, width, height);
                    pass.draw(0..3, 0..1);
                }
            }
            Tail::ReadOnly(rects) => {
                let gpu = self.read_only.as_ref().expect("the WebGL2 reduction");
                let scratch = targets.read_only.as_ref().expect("the WebGL2 scratch");
                pass.set_pipeline(&gpu.resolve);
                pass.set_bind_group(0, &targets.snapshot, &[]);
                pass.set_bind_group(1, &scratch.resolve, &[]);
                // Pixels with no admitted operation discard, preserving live bytes.
                for &[x, y, width, height] in rects {
                    pass.set_scissor_rect(x, y, width, height);
                    pass.draw(0..3, 0..1);
                }
            }
        }
    }

    /// Draw `tail` in a pass of its own. Returns the passes closed (one).
    pub(crate) fn flush_tail<'texture>(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        color: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        tail: LiveTail,
        batch: &BatchRenderer,
        atlas: &impl Fn(usize) -> Option<&'texture BatchTexture>,
        buffer: &wgpu::Buffer,
    ) -> usize {
        let label = match &tail.0 {
            Tail::Packed(resolve) => return self.resolve_packed(encoder, color, depth, resolve),
            Tail::Edits(..) => "Terrain native wave edits",
            Tail::Shadows(_) => "Terrain depth-tested shadow halves",
            Tail::ReadOnly(_) => "Read-only final ordered destination words",
        };
        self.live_pass(encoder, color, depth, label, |renderer, pass| {
            renderer.record_tail(pass, &tail, batch, atlas, buffer);
        })
    }

    /// Record `draws` in a pass of their own over the live colour and depth,
    /// both loaded and stored. Returns the passes closed (one).
    fn live_pass(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        color: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        label: &str,
        draws: impl FnOnce(&Self, &mut wgpu::RenderPass<'_>),
    ) -> usize {
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(label),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: color,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
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
            draws(self, &mut pass);
        }
        self.submission.note_closed_passes(encoder, 1);
        1
    }
}

/// One draw of [`TerrainDrawRenderer::draw_interval_tail`], in parent order.
#[derive(Clone, Copy)]
pub(crate) enum IntervalDraw {
    Edit(DestinationEditCommand),
    /// An opaque body the caller records, by its own number.
    Body(u32),
}

/// One draw of a depth-tested interval; an edit is flagged where it may have
/// negative candidates and also draws its wrapped variant.
#[derive(Clone, Copy)]
enum DepthStep {
    Edit(TerrainCommand, bool),
    Body(u32),
}

fn edit_steps(
    commands: &[(TerrainCommand, bool)],
) -> impl Iterator<Item = DepthStep> + Clone + use<'_> {
    commands
        .iter()
        .map(|&(command, wraps)| DepthStep::Edit(command, wraps))
}

fn no_bodies(_: &mut wgpu::RenderPass<'_>, _: u32) {
    unreachable!("an edit interval holds only edits");
}

/// The draws a destination edit leaves for the live colour and depth after
/// its last read of them. The caller's next pass over those attachments
/// records them first ([`TerrainDrawRenderer::record_tail`]); anything else
/// that reads them comes after a pass of their own
/// ([`TerrainDrawRenderer::flush_tail`]). Draws keep their order inside a
/// pass, so either way the pixels are the same. VERA scheduling only.
#[must_use = "a tail draws only when recorded or flushed"]
pub(crate) struct LiveTail(Tail);

enum Tail {
    /// A packed transaction's resolve.
    Packed(packed::PackedResolve),
    /// One read-write wave's edits, after their snapshot, with the binding
    /// and offset of each window's records.
    Edits(Vec<TerrainCommand>, Vec<(wgpu::BindGroup, u32)>),
    /// One depth-tested interval's shadow halves, per shadow rectangle.
    Shadows(Vec<[u32; 4]>),
    /// WebGL2's read-only final words, per rectangle.
    ReadOnly(Vec<[u32; 4]>),
}

/// Snapshot module source. The WebGL2 fallback loads the scene through its
/// sRGB view and has a snapshot that also resets its read-only scratch.
fn snapshot_source(webgl: bool) -> String {
    let snapshot = format!(
        "{}\n{}",
        include_str!("native_packed.wgsl"),
        include_str!("terrain_snapshot.wgsl")
    );
    if !webgl {
        return snapshot;
    }
    crate::render::webgl::encoded_shader(
        &format!("{snapshot}\n{WEBGL_READ_ONLY_SNAPSHOT}"),
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

/// Destination edit module source. Edits draw against the live depth test
/// (`terrain_depth.wgsl`), except on the WebGL2 fallback, which reduces its
/// read-only ones instead (`terrain_read_only.wgsl`).
fn edit_source(webgl: bool) -> String {
    // Reuse the production SHP projection/ABI verbatim, without its shape
    // binding or fragment policy. Both consumers share native_row_z.
    let shp = include_str!("zsprite_shader.wgsl");
    let vertex = shp[..shp
        .find("// One signed candidate owner")
        .expect("SHP projection boundary")]
        .replace("@group(2) @binding(0) var t_zshape: texture_2d<f32>;", "");
    let entry_points = if webgl {
        include_str!("terrain_read_only.wgsl")
    } else {
        include_str!("terrain_depth.wgsl")
    };
    super::tactical_shader::sprite_source(&format!(
        "{vertex}\n{}\n{entry_points}",
        include_str!("terrain_edit.wgsl"),
    ))
}

/// The WebGL2 fallback's read-only resolve module source.
fn resolve_source() -> String {
    super::tactical_shader::source(include_str!("terrain_read_only_resolve.wgsl"))
}

/// Signed candidate envelope of a destination edit, as `terrain_source` in
/// terrain_edit.wgsl walks its rows (WGSL `round` ties to even). Rows are
/// monotone, so the two end rows bound it. A gradient word other than a
/// plain entry is not mirrored here and claims the whole range.
fn candidate_range(instance: &SpriteInstance, camera: CameraUniform) -> [i32; 2] {
    use super::native_z::{ZGradient, sprite_row_z};
    if instance.z_gradient > ZGradient::Vertical as u32 {
        return [i32::MIN, i32::MAX];
    }
    let round = |value: f32| value.round_ties_even() as i32;
    let height = round(instance.size[1]).max(1);
    let screen_top =
        round(instance.position[1]) - round(camera.camera_pos[1] + camera.native_z_origin_y);
    let gradient = ZGradient::from_index(instance.z_gradient);
    let z_adjust = round(instance.z_adjust);
    let [first, last] =
        [0, height - 1].map(|row| sprite_row_z(gradient, screen_top, height, z_adjust, row));
    [first.min(last), first.max(last)]
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
            ("read-only resolve", super::resolve_source()),
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
            (
                "scaled copy",
                crate::render::scaled_copy::shader_source(true),
            ),
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
            (
                "shroud compose",
                crate::render::shroud_buffer::compose_shader_source().to_string(),
            ),
            (
                "radar compose",
                crate::render::radar::compose::shader_source(),
            ),
            ("voxel raster", crate::render::voxel_gpu::shader_source()),
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
