//! Shared tactical ABuffer — native per-pixel palette-row input.
//!
//! The original engine writes SHROUD.SHP/FOG.SHP into a 16-bit circular buffer;
//! each tile/sprite blitter uses A to select a palette row. Ours is one R8 GPU
//! texture the GPU composes in place (`shroud_compose.wgsl`):
//!
//! 1. Clear to the neutral 0x7F, then copy/blend the selected frames' raw
//!    bytes from R8Uint atlases, in native cell order;
//! 2. Rescale it under the AlphaShapeClass glows through a scratch copy;
//! 3. Every world blitter, surface line and A-gated surface store reads this
//!    same texture. Play never reads it back to the CPU.
//!
//! Original493DF0/494B60/4990E0 select a palette row with A. Native surface
//! lines store their packed color directly, and shadow497390 halves the existing
//! destination without reading A. There is no later framebuffer curtain.
//!
//! The lattice is FLAT: the native per-cell blit
//! (`Tactical_layer_shroud_edges @ 0x006D3660`) zeroes the cell coord's Z
//! before projecting, so terrain height never shifts a shroud frame and the
//! diamonds tile the screen without gaps at height discontinuities.
//!
//! ## Dependency rules
//! - Part of render/ — reads FogState (no mutation), draws through Device/Queue.

use crate::map::resolved_terrain::NativeCellQuery;
use crate::map::terrain::{iso_to_screen, screen_to_iso};
use crate::rules::alpha_image_assets::AlphaImage;
use crate::sim::intern::InternedId;
use crate::sim::vision::FogState;
use std::ops::Range;
use std::sync::Arc;
use wgpu::util::DeviceExt;

/// Tactical clear value written by6D3F9F. A selects a palette row; it is not
/// itself a universal RGB brightness multiplier.
const NEUTRAL: u8 = 0x7F;

/// The default retains native A. The optional translucent style uses 500;
/// simulation visibility, shroud knowledge and stored A bytes stay unchanged.
/// The lift also halves the AlphaShapeClass glows, which share the plane with
/// the shroud.
const FOG_STRENGTH_MILLI: u32 = 1000;

/// Lift one A value toward [`NEUTRAL`] by `strength_milli` (0 = no shading,
/// 1000 = native). Every A reader's WGSL (`fog_lifted_a`) applies the same
/// integer expression; this one serves readbacks.
pub(crate) fn fog_lifted_a(a: u8, strength_milli: u32) -> u8 {
    let delta = i32::from(a) - i32::from(NEUTRAL);
    (i32::from(NEUTRAL) + delta * strength_milli as i32 / 1000).clamp(0, 255) as u8
}

/// SHP transparent pixel marker — skip (don't overwrite buffer).
const TRANSPARENT: u8 = 0xFE;
const SHROUD_CELL_PAD: i32 = 8;

/// Inputs retained by the scenario/House owners, sampled for this draw.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FogDrawOptions {
    pub fog_of_war: bool,
    pub current_house_defeated: bool,
}

/// One visited cell of a rebuild, in native order: where its frames land and
/// which frames it selects, [`NO_FRAME`] for none. The first is copied, the
/// second (FOG.SHP only) blended.
///
/// The A plane is a pure function of these, the lattice size and camera, the
/// frame set the options select and the frames' bytes. So a fog generation
/// whose list equals the drawn one, with all else unchanged, needs no redraw;
/// most ticks change no cell in view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CellBlits {
    origin: [i32; 2],
    frames: [u16; 2],
}

const NO_FRAME: u16 = u16::MAX;

/// One AlphaShapeClass glow (`sim::world::alpha_shape`): its origin in
/// absolute world pixels and its type's AlphaImage.
#[derive(Debug, Clone)]
pub struct AlphaShapeDraw {
    pub origin: [i32; 2],
    pub image: Arc<AlphaImage>,
}

/// Images are immutable rules resources, so the same allocation is the same
/// image and a frame's list compares without reading pixels.
impl PartialEq for AlphaShapeDraw {
    fn eq(&self, other: &Self) -> bool {
        self.origin == other.origin && Arc::ptr_eq(&self.image, &other.image)
    }
}

/// What the shroud brightness buffer writes for one cell.
///
/// Split out of the rebuild loop so the decision can be tested without a GPU.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CellFill {
    /// Leave the neutral value — nothing to darken here.
    None,
    /// Blit this SHROUD.SHP frame.
    Frame(usize),
}

/// Decide what one cell contributes to the shroud brightness buffer.
///
/// Native6FB170 changes Cell+12C shroud knowledge only without sustained sight;
/// native578100 consumes pending departure conceal at the120-frame Logic boundary.
/// FogState publishes that selected-cell result here. This CPU decision comparison
/// does not certify the complete native edge cache or GPU pixel composition.
pub(crate) fn cell_fill(
    fog: &FogState,
    owner: InternedId,
    rx: u16,
    ry: u16,
    lut: &[u8; 256],
) -> CellFill {
    // Original6D8700 ->4801F0 ->47EFE0 selects the actual SHROUD frame15.
    // Stock frame15 stores A2, not zero: this also decides native rally pass
    // admission. The archive bytes belong to the existing frame-data owner.
    if !fog.is_cell_revealed(owner, rx, ry) || fog.is_cell_gap_covered(owner, rx, ry) {
        return CellFill::Frame(15);
    }
    let bitmask = fog.shroud_edge_mask_8bit(owner, rx, ry);
    if bitmask == 0 {
        return CellFill::None; // Fully revealed — already bright.
    }
    match lut[bitmask as usize] {
        0xFF => CellFill::None,
        0xFE => CellFill::Frame(15),
        idx => CellFill::Frame(idx as usize),
    }
}

pub use crate::sim::vision::SHROUD_EDGE_LUT;

/// One SHROUD.SHP or FOG.SHP frame set: its raw bytes as an R8Uint atlas the
/// compositor reads, frames stacked down columns. Created once with the set;
/// viewport, camera and fog changes reuse it.
struct FrameAtlas {
    canvas: [u32; 2],
    count: usize,
    per_column: u32,
    view: wgpu::TextureView,
    /// No two cells' stores can meet (see [`cells_disjoint`]), so copies and
    /// blends of different cells commute.
    cells_disjoint: bool,
    /// The compositor's binding of this atlas, made with its first use.
    binding: Option<wgpu::BindGroup>,
}

impl FrameAtlas {
    fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frames: Vec<Vec<u8>>,
        canvas: [u32; 2],
    ) -> Self {
        let area = canvas[0] as usize * canvas[1] as usize;
        assert!(frames.iter().all(|frame| frame.len() == area));
        let limit = device.limits().max_texture_dimension_2d;
        let per_column = (limit / canvas[1].max(1)).max(1);
        let count = u32::try_from(frames.len()).expect("frame count");
        let size = [
            (count.div_ceil(per_column) * canvas[0]).max(1),
            (count.min(per_column) * canvas[1]).max(1),
        ];
        assert!(
            size[0] <= limit && size[1] <= limit,
            "frame atlas exceeds the GPU limit"
        );
        let mut texels = vec![TRANSPARENT; size[0] as usize * size[1] as usize];
        if area != 0 {
            for (index, frame) in frames.iter().enumerate() {
                let [x, y] = atlas_origin(index, per_column, canvas);
                for (row, source) in frame.chunks_exact(canvas[0] as usize).enumerate() {
                    let start = (y as usize + row) * size[0] as usize + x as usize;
                    texels[start..start + source.len()].copy_from_slice(source);
                }
            }
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Shroud frame atlas"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Uint,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        crate::render::atlas_growth::write_texels(queue, &texture, [0, 0], size, 1, &texels);
        Self {
            canvas,
            count: frames.len(),
            per_column,
            view: texture.create_view(&Default::default()),
            cells_disjoint: cells_disjoint(&frames, canvas),
            binding: None,
        }
    }

    fn has(&self, frame: u16) -> bool {
        frame != NO_FRAME && usize::from(frame) < self.count
    }
}

fn atlas_origin(index: usize, per_column: u32, canvas: [u32; 2]) -> [u32; 2] {
    let index = index as u32;
    [
        index / per_column * canvas[0],
        index % per_column * canvas[1],
    ]
}

/// Whether two different cells' stores can never meet: the union of every
/// frame's stored (non-FE) pixels misses itself at each lattice offset at
/// which two canvases overlap. Stock SHROUD and FOG diamonds tile exactly.
fn cells_disjoint(frames: &[Vec<u8>], canvas: [u32; 2]) -> bool {
    let [width, height] = canvas.map(|v| v as i32);
    if frames.is_empty() || width == 0 || height == 0 {
        return true;
    }
    let stored: Vec<bool> = (0..(width * height) as usize)
        .map(|i| frames.iter().any(|frame| frame[i] != TRANSPARENT))
        .collect();
    // The shared projection's lattice, one cell from a base well inside u16.
    let base = iso_to_screen(1024, 1024, 0);
    let offset = |rx: i32, ry: i32| {
        let (x, y) = iso_to_screen((1024 + rx) as u16, (1024 + ry) as u16, 0);
        [(x - base.0) as i32, (y - base.1) as i32]
    };
    let step = offset(1, 0).map(|v| v.abs().max(1));
    let reach = [(width - 1) / step[0], (height - 1) / step[1]];
    for u in -reach[0]..=reach[0] {
        for v in -reach[1]..=reach[1] {
            // u = rx - ry and v = rx + ry share parity.
            if (u, v) == (0, 0) || (u + v) % 2 != 0 {
                continue;
            }
            let [dx, dy] = offset((u + v) / 2, (v - u) / 2);
            for y in 0.max(-dy)..height.min(height - dy) {
                for x in 0.max(-dx)..width.min(width - dx) {
                    if stored[(y * width + x) as usize]
                        && stored[((y + dy) * width + x + dx) as usize]
                    {
                        return false;
                    }
                }
            }
        }
    }
    true
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct CellInstance {
    origin: [i32; 2],
    atlas: [u32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GlowInstance {
    scratch: [i32; 2],
    size: [i32; 2],
    plane: [i32; 2],
    image: [i32; 2],
}

/// A target size and canvas (cells) or scratch size (glows), 16 bytes.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ComposeParams {
    target_size: [f32; 2],
    extent: [f32; 2],
}

/// Instance indices of one cell run: its copies, then its blends.
struct CellRun {
    copies: Range<u32>,
    blends: Range<u32>,
}

/// A vertex buffer that grows to the largest upload.
#[derive(Default)]
pub(crate) struct InstanceUpload {
    buffer: Option<wgpu::Buffer>,
}

impl InstanceUpload {
    /// The buffer of the last write, if any.
    pub(crate) fn buffer(&self) -> Option<&wgpu::Buffer> {
        self.buffer.as_ref()
    }

    pub(crate) fn write(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        label: &'static str,
        bytes: &[u8],
    ) -> &wgpu::Buffer {
        let size = (bytes.len() as u64).max(16);
        if self
            .buffer
            .as_ref()
            .is_none_or(|buffer| buffer.size() < size)
        {
            self.buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: size.next_power_of_two(),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        let buffer = self.buffer.as_ref().expect("allocated above");
        queue.write_buffer(buffer, 0, bytes);
        buffer
    }
}

/// The GPU programs and transient state that compose the plane. Made with
/// the first composition, so A fixtures never compile them.
struct Compositor {
    cell_layout: wgpu::BindGroupLayout,
    copy: wgpu::RenderPipeline,
    blend: wgpu::RenderPipeline,
    glow_layout: wgpu::BindGroupLayout,
    image_layout: wgpu::BindGroupLayout,
    glow: wgpu::RenderPipeline,
    cell_params: wgpu::Buffer,
    glow_params: wgpu::Buffer,
    cell_upload: InstanceUpload,
    glow_upload: InstanceUpload,
    /// The glow binding of the current plane view.
    glow_binding: Option<(wgpu::TextureView, wgpu::BindGroup)>,
    /// Glow results before their copy back; grows to the largest group.
    scratch: Option<wgpu::Texture>,
    /// One binding per distinct glow image, holding its allocation.
    images: Vec<(Arc<AlphaImage>, wgpu::BindGroup)>,
    cells: Vec<CellInstance>,
    runs: Vec<CellRun>,
    glows: Vec<GlowInstance>,
    glow_images: Vec<usize>,
    glow_groups: Vec<Range<usize>>,
}

const CELL_STRIDE: u64 = std::mem::size_of::<CellInstance>() as u64;
const GLOW_STRIDE: u64 = std::mem::size_of::<GlowInstance>() as u64;

pub(crate) fn layout_entry(binding: u32, ty: wgpu::BindingType) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty,
        count: None,
    }
}

pub(crate) fn uniform_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    layout_entry(
        binding,
        wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
    )
}

pub(crate) fn texture_entry(
    binding: u32,
    sample_type: wgpu::TextureSampleType,
) -> wgpu::BindGroupLayoutEntry {
    layout_entry(
        binding,
        wgpu::BindingType::Texture {
            sample_type,
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
    )
}

fn params_buffer(device: &wgpu::Device, label: &'static str) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: std::mem::size_of::<ComposeParams>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

pub(crate) fn compose_shader_source() -> &'static str {
    include_str!("shroud_compose.wgsl")
}

impl Compositor {
    fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Tactical A composition"),
            source: wgpu::ShaderSource::Wgsl(compose_shader_source().into()),
        });
        let cell_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Shroud frame atlas"),
            entries: &[
                uniform_entry(0),
                texture_entry(1, wgpu::TextureSampleType::Uint),
            ],
        });
        let glow_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Glow A source"),
            entries: &[
                uniform_entry(2),
                texture_entry(3, wgpu::TextureSampleType::Float { filterable: false }),
            ],
        });
        let image_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Glow AlphaImage"),
            entries: &[texture_entry(0, wgpu::TextureSampleType::Uint)],
        });
        let pipeline = |label,
                        layouts: &[Option<&wgpu::BindGroupLayout>],
                        vertex: &'static str,
                        attributes: &[wgpu::VertexAttribute],
                        stride: u64,
                        fragment: &'static str,
                        blend: Option<wgpu::BlendState>| {
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: layouts,
                immediate_size: 0,
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vertex),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: stride,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes,
                    })],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fragment),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: PLANE_FORMAT,
                        blend,
                        write_mask: wgpu::ColorWrites::RED,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleStrip,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let cell_attributes = wgpu::vertex_attr_array![0 => Sint32x2, 1 => Uint32x2];
        let glow_attributes =
            wgpu::vertex_attr_array![0 => Sint32x2, 1 => Sint32x2, 2 => Sint32x2, 3 => Sint32x2];
        // A - (127 - byte), clamped at zero by the unorm target.
        let subtract = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::ReverseSubtract,
        };
        let copy = pipeline(
            "Shroud frame copy (47EFE0)",
            &[Some(&cell_layout)],
            "vs_cell",
            &cell_attributes,
            CELL_STRIDE,
            "fs_copy",
            None,
        );
        let blend = pipeline(
            "Fog frame blend (47F250)",
            &[Some(&cell_layout)],
            "vs_cell",
            &cell_attributes,
            CELL_STRIDE,
            "fs_blend",
            Some(wgpu::BlendState {
                color: subtract,
                alpha: subtract,
            }),
        );
        let glow = pipeline(
            "AlphaShape glow (421350)",
            &[Some(&glow_layout), Some(&image_layout)],
            "vs_glow",
            &glow_attributes,
            GLOW_STRIDE,
            "fs_glow",
            None,
        );
        Self {
            cell_layout,
            copy,
            blend,
            glow_layout,
            image_layout,
            glow,
            cell_params: params_buffer(device, "Shroud cell parameters"),
            glow_params: params_buffer(device, "Glow scratch parameters"),
            cell_upload: InstanceUpload::default(),
            glow_upload: InstanceUpload::default(),
            glow_binding: None,
            scratch: None,
            images: Vec::new(),
            cells: Vec::new(),
            runs: Vec::new(),
            glows: Vec::new(),
            glow_images: Vec::new(),
            glow_groups: Vec::new(),
        }
    }

    /// Copy each blit's first frame and blend its second onto `target`, in
    /// collection order, after clearing it to [`NEUTRAL`] when `clear`.
    /// Copies alone keep native order through primitive order. A run whose
    /// cells cannot meet draws all its copies, then all its blends; that is
    /// the whole list when the frames tile, else one diagonal's run of
    /// canvases that lie apart.
    #[allow(clippy::too_many_arguments)]
    fn encode_cells(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        size: [u32; 2],
        frames: &mut FrameAtlas,
        blits: &[CellBlits],
        clear: bool,
    ) {
        self.cells.clear();
        self.runs.clear();
        let any_blend = blits.iter().any(|blit| frames.has(blit.frames[1]));
        let mut start = 0;
        while start < blits.len() {
            let mut end = blits.len();
            if any_blend && !frames.cells_disjoint {
                // Native6D7312..6D7532: one diagonal shares its Y origin and
                // advances right, so each canvas only meets its neighbors.
                end = start + 1;
                while end < blits.len()
                    && blits[end].origin[1] == blits[start].origin[1]
                    && blits[end].origin[0] - blits[end - 1].origin[0] >= frames.canvas[0] as i32
                {
                    end += 1;
                }
            }
            let mut push = |slot: usize| {
                let first = self.cells.len() as u32;
                for blit in &blits[start..end] {
                    let frame = blit.frames[slot];
                    if frames.has(frame) {
                        self.cells.push(CellInstance {
                            origin: blit.origin,
                            atlas: atlas_origin(
                                usize::from(frame),
                                frames.per_column,
                                frames.canvas,
                            ),
                        });
                    }
                }
                first..self.cells.len() as u32
            };
            let copies = push(0);
            let blends = push(1);
            self.runs.push(CellRun { copies, blends });
            start = end;
        }
        queue.write_buffer(
            &self.cell_params,
            0,
            bytemuck::bytes_of(&ComposeParams {
                target_size: size.map(|v| v as f32),
                extent: frames.canvas.map(|v| v as f32),
            }),
        );
        let binding = frames.binding.get_or_insert_with(|| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Shroud frame atlas"),
                layout: &self.cell_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.cell_params.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&frames.view),
                    },
                ],
            })
        });
        let instances = self.cell_upload.write(
            device,
            queue,
            "Shroud cell blits",
            bytemuck::cast_slice(&self.cells),
        );
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Tactical A cells"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: if clear {
                        wgpu::LoadOp::Clear(wgpu::Color {
                            r: f64::from(NEUTRAL) / 255.0,
                            g: 0.0,
                            b: 0.0,
                            a: 1.0,
                        })
                    } else {
                        wgpu::LoadOp::Load
                    },
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_bind_group(0, &*binding, &[]);
        for run in &self.runs {
            for (pipeline, range) in [(&self.copy, &run.copies), (&self.blend, &run.blends)] {
                if range.is_empty() {
                    continue;
                }
                pass.set_pipeline(pipeline);
                // Slices rather than a first instance: WebGL2 has no base instance.
                pass.set_vertex_buffer(
                    0,
                    instances.slice(
                        u64::from(range.start) * CELL_STRIDE..u64::from(range.end) * CELL_STRIDE,
                    ),
                );
                pass.draw(0..4, 0..range.end - range.start);
            }
        }
    }

    /// `AlphaShapeClass::DrawAll_NoMask @ 0x00421350` over the whole plane,
    /// in vector order. Each glow rescales the A beneath it, so a glow reads
    /// the plane through its own scratch result and copies it back. Glows in
    /// one group never overlap, so they share a pass; an overlap starts the
    /// next group, which reads the earlier group's copies.
    #[allow(clippy::too_many_arguments)]
    fn encode_glows(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        plane: &wgpu::Texture,
        plane_view: &wgpu::TextureView,
        shapes: &[AlphaShapeDraw],
        camera: [i32; 2],
    ) {
        self.glows.clear();
        self.glow_images.clear();
        self.glow_groups.clear();
        let limit = device.limits().max_texture_dimension_2d as i32;
        let plane_size = [plane.width() as i32, plane.height() as i32];
        let mut group = 0;
        let mut x = 0;
        let mut extent = [1, 1];
        for shape in shapes {
            let origin = [shape.origin[0] - camera[0], shape.origin[1] - camera[1]];
            let Some(rect) = glow_rect(&shape.image, origin, plane_size) else {
                continue;
            };
            let size = [rect[2] - rect[0], rect[3] - rect[1]];
            let overlaps = self.glows[group..].iter().any(|glow| {
                glow.plane[0] < rect[2]
                    && rect[0] < glow.plane[0] + glow.size[0]
                    && glow.plane[1] < rect[3]
                    && rect[1] < glow.plane[1] + glow.size[1]
            });
            if overlaps || x + size[0] > limit {
                self.glow_groups.push(group..self.glows.len());
                group = self.glows.len();
                x = 0;
            }
            let [frame_x, frame_y, _, _] = shape.image.frame_bounds();
            self.glows.push(GlowInstance {
                scratch: [x, 0],
                size,
                plane: [rect[0], rect[1]],
                image: [origin[0] + frame_x, origin[1] + frame_y],
            });
            let image = self.image_binding(device, queue, &shape.image);
            self.glow_images.push(image);
            x += size[0];
            extent = [extent[0].max(x), extent[1].max(size[1])];
        }
        if group == self.glows.len() {
            return;
        }
        self.glow_groups.push(group..self.glows.len());
        if self.scratch.as_ref().is_none_or(|scratch| {
            (scratch.width() as i32) < extent[0] || (scratch.height() as i32) < extent[1]
        }) {
            let current = self.scratch.as_ref().map_or([0, 0], |scratch| {
                [scratch.width() as i32, scratch.height() as i32]
            });
            self.scratch = Some(create_plane_texture(
                device,
                "Glow scratch",
                [
                    current[0].max(extent[0]) as u32,
                    current[1].max(extent[1]) as u32,
                ],
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            ));
        }
        let scratch = self.scratch.as_ref().expect("allocated above");
        queue.write_buffer(
            &self.glow_params,
            0,
            bytemuck::bytes_of(&ComposeParams {
                target_size: [scratch.width() as f32, scratch.height() as f32],
                extent: [0.0; 2],
            }),
        );
        if self
            .glow_binding
            .as_ref()
            .is_none_or(|(view, _)| view != plane_view)
        {
            let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Glow A source"),
                layout: &self.glow_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: self.glow_params.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(plane_view),
                    },
                ],
            });
            self.glow_binding = Some((plane_view.clone(), binding));
        }
        let (_, binding) = self.glow_binding.as_ref().expect("bound above");
        let instances = self.glow_upload.write(
            device,
            queue,
            "Glow rectangles",
            bytemuck::cast_slice(&self.glows),
        );
        let scratch_view = scratch.create_view(&Default::default());
        for group in &self.glow_groups {
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("Tactical A glows"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &scratch_view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(&self.glow);
                pass.set_bind_group(0, binding, &[]);
                for index in group.clone() {
                    pass.set_bind_group(1, &self.images[self.glow_images[index]].1, &[]);
                    let offset = index as u64 * GLOW_STRIDE;
                    pass.set_vertex_buffer(0, instances.slice(offset..offset + GLOW_STRIDE));
                    pass.draw(0..4, 0..1);
                }
            }
            for glow in &self.glows[group.clone()] {
                encoder.copy_texture_to_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: scratch,
                        mip_level: 0,
                        origin: wgpu::Origin3d {
                            x: glow.scratch[0] as u32,
                            y: glow.scratch[1] as u32,
                            z: 0,
                        },
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::TexelCopyTextureInfo {
                        texture: plane,
                        mip_level: 0,
                        origin: wgpu::Origin3d {
                            x: glow.plane[0] as u32,
                            y: glow.plane[1] as u32,
                            z: 0,
                        },
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::Extent3d {
                        width: glow.size[0] as u32,
                        height: glow.size[1] as u32,
                        depth_or_array_layers: 1,
                    },
                );
            }
        }
    }

    /// The binding of one glow image, uploaded with its first use.
    fn image_binding(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        image: &Arc<AlphaImage>,
    ) -> usize {
        if let Some(index) = self
            .images
            .iter()
            .position(|(known, _)| Arc::ptr_eq(known, image))
        {
            return index;
        }
        let [_, _, width, height] = image.frame_bounds().map(|v| v.max(1) as u32);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Glow AlphaImage frame"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Uint,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        if image.pixels().len() == (width * height) as usize {
            crate::render::atlas_growth::write_texels(
                queue,
                &texture,
                [0, 0],
                [width, height],
                1,
                image.pixels(),
            );
        }
        let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Glow AlphaImage frame"),
            layout: &self.image_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(
                    &texture.create_view(&Default::default()),
                ),
            }],
        });
        self.images.push((image.clone(), binding));
        self.images.len() - 1
    }
}

/// The plane rectangle one glow rescales, `origin` in plane pixels. Nothing
/// is drawn unless the canvas meets the plane (`Rect_Intersect` at
/// `0x004213FA`, tested at `0x00421411..0x0042141E`). Frame 0 then starts at
/// its own offset: the left and top edges clip to the canvas and plane, the
/// right and bottom to the plane alone (`0x00421480..0x004214F4`). Every
/// frame byte, 0 included, rescales the A beneath it.
fn glow_rect(
    image: &AlphaImage,
    [x, y]: [i32; 2],
    [plane_w, plane_h]: [i32; 2],
) -> Option<[i32; 4]> {
    let [canvas_w, canvas_h] = image.canvas();
    let (left, top) = (x.max(0), y.max(0));
    if (x + canvas_w).min(plane_w) <= left || (y + canvas_h).min(plane_h) <= top {
        return None;
    }
    let [frame_x, frame_y, frame_w, frame_h] = image.frame_bounds();
    let left = left.max(x + frame_x);
    let top = top.max(y + frame_y);
    let right = (x + frame_x + frame_w).min(plane_w);
    let bottom = (y + frame_y + frame_h).min(plane_h);
    (right > left && bottom > top).then_some([left, top, right, bottom])
}

const PLANE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;

fn create_plane_texture(
    device: &wgpu::Device,
    label: &'static str,
    [width, height]: [u32; 2],
    usage: wgpu::TextureUsages,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: PLANE_FORMAT,
        usage,
        view_formats: &[],
    })
}

/// The live plane: sampled by every A reader, composed in place, glows copied
/// back into it, and read back by diagnostics.
fn create_a_plane(device: &wgpu::Device, size: [u32; 2]) -> wgpu::Texture {
    create_plane_texture(
        device,
        "Shroud ABuffer Texture",
        size,
        wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC,
    )
}

/// The authoritative A plane on the GPU and its source origin. Derived
/// bindings cache the retained view identity; a resize replaces it and
/// invalidates them.
pub struct ShroudBuffer {
    /// Plane width in virtual pixels.
    width: u32,
    /// Plane height in virtual pixels.
    height: u32,
    /// GPU R8 plane. Native clear is 0x7F; stock unrevealed SHROUD stores
    /// 0x02. These are palette-row inputs.
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    /// World-pixel origin of the GPU A lattice, updated with each rebuild.
    source_uniform: wgpu::Buffer,
    /// SHROUD.SHP bytes, recreated only with the source frame set.
    frames: FrameAtlas,
    /// FOG.SHP is a second source for the same A plane, not an RGB overlay.
    fog_frames: FrameAtlas,
    compositor: Option<Compositor>,
    /// Cached camera X for change detection.
    last_cam_x: f32,
    /// Cached camera Y for change detection.
    last_cam_y: f32,
    /// Cached fog generation for change detection.
    last_fog_gen: u64,
    /// The same fog generation can be sampled for a different local viewer.
    last_owner: Option<InternedId>,
    last_fog_options: FogDrawOptions,
    /// Cached screen width for resize detection.
    last_screen_w: u32,
    /// Cached screen height for resize detection.
    last_screen_h: u32,
    /// Cached zoom level for change detection.
    last_zoom: f32,
    /// Map dimensions in cells.
    map_width: u16,
    map_height: u16,
    /// 256-byte LUT mapping neighbor bitmask to SHROUD.SHP frame index.
    lut: [u8; 256],
    /// Translucent-fog policy, expressed as the fraction of native A darkening
    /// that survives; this source's own value keeps native fixtures intact.
    fog_strength_milli: u32,
    /// What the plane holds: the blits of the last composition.
    drawn_blits: Vec<CellBlits>,
    /// The next rebuild's blits, allocated once.
    next_blits: Vec<CellBlits>,
    /// The glows every composition applies after the cells.
    alpha_shapes: Vec<AlphaShapeDraw>,
    /// The next frame's glows, allocated once.
    next_alpha_shapes: Vec<AlphaShapeDraw>,
    /// A resize, new frames or a load leaves the plane unproven by
    /// `drawn_blits`; the next rebuild redraws whatever it collects.
    redraw_required: bool,
}

/// Same virtual-pixel coverage as the camera projection; no arbitrary4096
/// truncation. Device limits belong to the zoom/viewport admission owner.
fn virtual_dimensions(screen: [u32; 2], zoom: f32) -> [u32; 2] {
    assert!(zoom.is_finite() && zoom > 0.0);
    screen.map(|value| ((value as f32 / zoom).ceil() as u32).max(1))
}

fn clamp_cell_range(min_v: i32, max_v: i32, limit: u16) -> Option<(u16, u16)> {
    if limit == 0 {
        return None;
    }
    let upper = i32::from(limit) - 1;
    let start = min_v.clamp(0, upper);
    let end = max_v.clamp(0, upper);
    if start > end {
        return None;
    }
    Some((start as u16, end as u16))
}
impl ShroudBuffer {
    fn visible_cell_bounds(
        &self,
        cam_x: f32,
        cam_y: f32,
        vp_w: i32,
        vp_h: i32,
    ) -> Option<((u16, u16), (u16, u16))> {
        let pad_x = self.frames.canvas[0].max(self.fog_frames.canvas[0]) as f32;
        let pad_y = self.frames.canvas[1].max(self.fog_frames.canvas[1]) as f32;
        let left = cam_x - pad_x;
        let right = cam_x + vp_w as f32 + pad_x;
        let top = cam_y - pad_y;
        let bottom = cam_y + vp_h as f32 + pad_y;
        let corners = [
            screen_to_iso(left, top),
            screen_to_iso(right, top),
            screen_to_iso(left, bottom),
            screen_to_iso(right, bottom),
        ];

        let mut min_rx = f32::INFINITY;
        let mut max_rx = f32::NEG_INFINITY;
        let mut min_ry = f32::INFINITY;
        let mut max_ry = f32::NEG_INFINITY;
        for (rx, ry) in corners {
            min_rx = min_rx.min(rx);
            max_rx = max_rx.max(rx);
            min_ry = min_ry.min(ry);
            max_ry = max_ry.max(ry);
        }

        let rx_range = clamp_cell_range(
            min_rx.floor() as i32 - SHROUD_CELL_PAD,
            max_rx.ceil() as i32 + SHROUD_CELL_PAD,
            self.map_width,
        )?;
        let ry_range = clamp_cell_range(
            min_ry.floor() as i32 - SHROUD_CELL_PAD,
            max_ry.ceil() as i32 + SHROUD_CELL_PAD,
            self.map_height,
        )?;
        Some((rx_range, ry_range))
    }

    /// Create a new shroud buffer for the given screen and map dimensions.
    ///
    /// `frame_pixels` is the raw SHROUD.SHP brightness data per frame,
    /// extracted by the caller from the SHP file.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        screen_size: [u32; 2],
        map_size: [u16; 2],
        frame_pixels: Vec<Vec<u8>>,
        canvas_size: [u32; 2],
        lut: [u8; 256],
    ) -> Self {
        let [screen_w, screen_h] = screen_size;
        let [map_width, map_height] = map_size;
        let size = screen_size.map(|v| v.max(1));
        let texture = create_a_plane(device, size);
        // Readers may sample before the first rebuild composes the plane.
        crate::render::atlas_growth::write_texels(
            queue,
            &texture,
            [0, 0],
            size,
            1,
            &vec![NEUTRAL; size[0] as usize * size[1] as usize],
        );
        let view = texture.create_view(&Default::default());
        let source_uniform = source_uniform(device, [0.0; 2], true, FOG_STRENGTH_MILLI);
        Self {
            width: size[0],
            height: size[1],
            texture,
            view,
            source_uniform,
            frames: FrameAtlas::new(device, queue, frame_pixels, canvas_size),
            fog_frames: FrameAtlas::new(device, queue, Vec::new(), [0, 0]),
            compositor: None,
            last_cam_x: f32::NAN,
            last_cam_y: f32::NAN,
            last_fog_gen: u64::MAX,
            last_owner: None,
            last_fog_options: FogDrawOptions::default(),
            last_screen_w: screen_w,
            last_screen_h: screen_h,
            last_zoom: 1.0,
            map_width,
            map_height,
            lut,
            fog_strength_milli: FOG_STRENGTH_MILLI,
            drawn_blits: Vec::new(),
            next_blits: Vec::new(),
            alpha_shapes: Vec::new(),
            next_alpha_shapes: Vec::new(),
            redraw_required: true,
        }
    }

    /// Apply the local graphics preference through this A source's uniform.
    /// Every reader's WGSL lifts the stored bytes with the same strength.
    pub(crate) fn set_translucent_fog(&mut self, queue: &wgpu::Queue, enabled: bool) {
        let strength = if enabled { 500 } else { FOG_STRENGTH_MILLI };
        if self.fog_strength_milli != strength {
            self.fog_strength_milli = strength;
            self.write_source_uniform(queue);
        }
    }

    /// Cell47EFE0/47F250 load the literal FOG.SHP at81CE10. Keep its raw
    /// brightness bytes with the existing A-buffer source owner.
    pub(crate) fn install_fog_frames(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pixels: Vec<Vec<u8>>,
        canvas: [u32; 2],
    ) {
        self.fog_frames = FrameAtlas::new(device, queue, pixels, canvas);
        self.mark_stale();
    }

    /// Take this frame's AlphaShapeClass glows, in vector order. A list that
    /// differs from the drawn one makes the next rebuild redraw the plane.
    pub fn set_alpha_shapes(&mut self, shapes: impl IntoIterator<Item = AlphaShapeDraw>) {
        self.next_alpha_shapes.clear();
        self.next_alpha_shapes.extend(shapes);
        if self.next_alpha_shapes != self.alpha_shapes {
            std::mem::swap(&mut self.next_alpha_shapes, &mut self.alpha_shapes);
            self.mark_stale();
        }
    }

    /// Invalidate the fog dirty-gate so the next frame rebuilds regardless of
    /// generation (F10): the view-cache generation restarts from zero after a
    /// load, so an equal counter value no longer proves an unchanged view.
    pub fn mark_stale(&mut self) {
        self.last_fog_gen = u64::MAX;
        self.redraw_required = true;
    }

    /// Recompose the plane if the camera moved, fog changed, or the screen
    /// resized, recording the GPU work into `encoder` ahead of its readers.
    ///
    /// Native per-cell ordering is retained. A new fog generation that
    /// selects the same frames for every cell in view keeps the plane as
    /// drawn. Returns whether the plane was recomposed.
    #[allow(clippy::too_many_arguments)]
    pub fn rebuild_if_needed(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        fog: &FogState,
        owner: crate::sim::intern::InternedId,
        cam_x: f32,
        cam_y: f32,
        screen_w: u32,
        screen_h: u32,
        zoom: f32,
        native_cells: Option<&NativeCellQuery<'_>>,
        fog_options: FogDrawOptions,
    ) -> bool {
        // Compose at virtual resolution (screen / zoom) so diamond blits stay
        // at fixed world-pixel sizes. Blitters address it from the uploaded
        // camera/zoom and this source's origin, without normalized UV stretching.
        // The camera owner bounds zoom against the requested device limit.
        // Truncating this lattice would silently expose unsampled shroud as
        // neutral A; keep the full virtual extent and reject that broken input.
        let [virt_w, virt_h] = virtual_dimensions([screen_w, screen_h], zoom);
        let limit = device.limits().max_texture_dimension_2d;
        assert!(
            virt_w <= limit && virt_h <= limit,
            "tactical A extent exceeds requested GPU limit"
        );

        // Resize GPU texture if virtual dimensions changed.
        if virt_w != self.width
            || virt_h != self.height
            || screen_w != self.last_screen_w
            || screen_h != self.last_screen_h
        {
            self.width = virt_w;
            self.height = virt_h;
            self.texture = create_a_plane(device, [virt_w, virt_h]);
            self.view = self.texture.create_view(&Default::default());
            self.last_screen_w = screen_w;
            self.last_screen_h = screen_h;
            // Force rebuild after resize.
            self.mark_stale();
        }

        // Skip if nothing changed (camera rounded to pixel + fog generation + zoom).
        let cam_x_r = cam_x.floor();
        let cam_y_r = cam_y.floor();
        let same_view = self.last_owner == Some(owner)
            && self.last_fog_options == fog_options
            && cam_x_r == self.last_cam_x
            && cam_y_r == self.last_cam_y
            && (zoom - self.last_zoom).abs() < 1e-6;
        if same_view && fog.view_generation() == self.last_fog_gen {
            return false;
        }
        self.last_fog_gen = fog.view_generation();
        self.last_owner = Some(owner);
        self.last_fog_options = fog_options;
        self.last_cam_x = cam_x_r;
        self.last_cam_y = cam_y_r;
        self.last_zoom = zoom;

        let mut blits = std::mem::take(&mut self.next_blits);
        self.collect_blits(fog, owner, native_cells, fog_options, &mut blits);
        if same_view && !self.redraw_required && blits == self.drawn_blits {
            self.next_blits = blits;
            return false;
        }
        self.next_blits = std::mem::replace(&mut self.drawn_blits, blits);
        // Debug: compose on an own submission and save the plane as a
        // grayscale PNG when RA2_DUMP_SHROUD names a path. Diagnostic only —
        // no gamemd counterpart.
        if let Some(path) = std::env::var_os("RA2_DUMP_SHROUD") {
            let mut own = device.create_command_encoder(&Default::default());
            self.compose(device, queue, &mut own, fog_options);
            queue.submit([own.finish()]);
            match self.read_plane(device, queue, [0, 0], [self.width, self.height]) {
                Ok(plane) => {
                    if let Err(e) = image::save_buffer(
                        &path,
                        &plane,
                        self.width,
                        self.height,
                        image::ColorType::L8,
                    ) {
                        log::warn!("shroud dump failed: {e}");
                    }
                }
                Err(e) => log::warn!("shroud dump readback failed: {e}"),
            }
        } else {
            self.compose(device, queue, encoder, fog_options);
        }
        self.write_source_uniform(queue);
        true
    }

    fn write_source_uniform(&self, queue: &wgpu::Queue) {
        let origin =
            [self.last_cam_x, self.last_cam_y].map(|v| if v.is_finite() { v } else { 0.0 });
        queue.write_buffer(
            &self.source_uniform,
            0,
            bytemuck::bytes_of(&SourceUniform::new(origin, true, self.fog_strength_milli)),
        );
    }

    /// Collect and compose at the current camera, for direct native scene
    /// comparisons and timing. Empty bounds still retain the authoritative reset.
    #[cfg(test)]
    fn rasterize(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        fog: &FogState,
        owner: InternedId,
        native_cells: Option<&NativeCellQuery<'_>>,
        fog_options: FogDrawOptions,
    ) {
        let mut blits = std::mem::take(&mut self.next_blits);
        self.collect_blits(fog, owner, native_cells, fog_options, &mut blits);
        self.next_blits = std::mem::replace(&mut self.drawn_blits, blits);
        self.compose(device, queue, encoder, fog_options);
    }

    /// The cells a rebuild blits, in native order, with the frames each
    /// selects at the current camera. Reads the fog state only;
    /// [`Self::compose`] draws the plane from them. Cells that select no
    /// frame leave the neutral fill and are left out.
    fn collect_blits(
        &self,
        fog: &FogState,
        owner: InternedId,
        native_cells: Option<&NativeCellQuery<'_>>,
        fog_options: FogDrawOptions,
        blits: &mut Vec<CellBlits>,
    ) {
        blits.clear();
        let vp_w = self.width as i32;
        let vp_h = self.height as i32;
        let cam_x_r = self.last_cam_x;
        let cam_y_r = self.last_cam_y;
        let cam_xi = cam_x_r as i32;
        let cam_yi = cam_y_r as i32;

        let Some(((rx_start, rx_end), (ry_start, ry_end))) =
            self.visible_cell_bounds(cam_x_r, cam_y_r, vp_w, vp_h)
        else {
            return;
        };
        // A frame index past the set selects nothing, as a missing frame does.
        let frame = |index: usize| u16::try_from(index).unwrap_or(NO_FRAME);
        let canvas = [
            self.frames.canvas[0].max(self.fog_frames.canvas[0]) as i32,
            self.frames.canvas[1].max(self.fog_frames.canvas[1]) as i32,
        ];

        // Native6D7312..6D7532 visits increasing screen diagonals, then
        // increasing X/decreasing Y within each diagonal. Each cell copies
        // its ground source before blending fog (4801F0); preserve that order.
        for diagonal in
            u32::from(rx_start) + u32::from(ry_start)..=u32::from(rx_end) + u32::from(ry_end)
        {
            let start = u32::from(rx_start).max(diagonal.saturating_sub(u32::from(ry_end)));
            let end = u32::from(rx_end).min(diagonal - u32::from(ry_start));
            for x in start..=end {
                let rx = x as u16;
                let ry = (diagonal - x) as u16;
                // The shroud lattice is FLAT: gamemd's per-cell edge blit
                // (`Tactical_layer_shroud_edges @ 0x006D3660`) takes
                // `CellClass::Get_Center_Coords`, forces the coord's Z to 0,
                // projects, then centers the 60x30 frame there. Terrain height
                // never shifts a shroud blit, so the diamonds tile the screen
                // with no gaps at cliffs, ramps, or bridge ends. Blitting at
                // cell height instead left an undarkened tear at every height
                // discontinuity.
                let (sx, sy) = iso_to_screen(rx, ry, 0);
                let vx = sx as i32 - cam_xi;
                let vy = sy as i32 - cam_yi;

                if vx + canvas[0] <= 0 || vx >= vp_w || vy + canvas[1] <= 0 || vy >= vp_h {
                    continue;
                }

                let frames = if fog_options.fog_of_war {
                    //47F018..47F038 selects FOG.SHP for the ground copy too.
                    //480210/480266 map -2 to15 and -1 to0 for both channels.
                    let ground = fog.map_cell_edge_frame(owner, (rx, ry), false, native_cells);
                    //480249..480290 skips only this blend for a defeated
                    //current House (+1F5); the ground copy still used FOG.
                    let edge = if fog_options.current_house_defeated {
                        NO_FRAME
                    } else {
                        let edge = fog.map_cell_edge_frame(owner, (rx, ry), true, native_cells);
                        frame(native_frame_index(edge))
                    };
                    [frame(native_frame_index(ground)), edge]
                } else {
                    match cell_fill(fog, owner, rx, ry, &self.lut) {
                        CellFill::Frame(idx) => [frame(idx), NO_FRAME],
                        CellFill::None => [NO_FRAME; 2],
                    }
                };
                if frames != [NO_FRAME; 2] {
                    blits.push(CellBlits {
                        origin: [vx, vy],
                        frames,
                    });
                }
            }
        }
    }

    /// Recompose the plane from `drawn_blits`: fill neutral, copy each cell's
    /// first frame and blend its second in collection order, then apply the
    /// glows. Blitting in world-pixel coordinates at virtual resolution means
    /// diamond tiles match the world grid exactly at every zoom.
    fn compose(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        fog_options: FogDrawOptions,
    ) {
        self.compose_cells(device, queue, encoder, fog_options, true);
        // `AlphaShapeClass::DrawAll_NoMask @ 0x00421350` follows the cell
        // loop (`0x006D754C`), over the same pass rectangle.
        let camera = [self.last_cam_x as i32, self.last_cam_y as i32];
        let compositor = self
            .compositor
            .get_or_insert_with(|| Compositor::new(device));
        compositor.encode_glows(
            device,
            queue,
            encoder,
            &self.texture,
            &self.view,
            &self.alpha_shapes,
            camera,
        );
    }

    /// The cell half of [`Self::compose`]. Without `clear` it draws over the
    /// plane's current bytes, as the native leaf comparisons need.
    fn compose_cells(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        fog_options: FogDrawOptions,
        clear: bool,
    ) {
        self.redraw_required = false;
        let frames = if fog_options.fog_of_war {
            &mut self.fog_frames
        } else {
            &mut self.frames
        };
        self.compositor
            .get_or_insert_with(|| Compositor::new(device))
            .encode_cells(
                device,
                queue,
                encoder,
                &self.view,
                [self.width, self.height],
                frames,
                &self.drawn_blits,
                clear,
            );
    }

    /// Read-only GPU source. The view stays stable until a resize; the uniform
    /// holds the floored world origin from the last rebuild.
    pub(crate) fn gpu_source(&self) -> (&wgpu::TextureView, &wgpu::Buffer) {
        (&self.view, &self.source_uniform)
    }

    /// Synthetic native A input for GPU leaf tests, through the production
    /// source allocation and uniform owner. Pixels are row-major, without padding.
    #[cfg(test)]
    pub(crate) fn fixture(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        size: [u32; 2],
        origin: [f32; 2],
        pixels: &[u8],
    ) -> Self {
        let mut result = Self::new(
            device,
            queue,
            size,
            [0, 0],
            Vec::new(),
            [60, 30],
            SHROUD_EDGE_LUT,
        );
        result.last_cam_x = origin[0].floor();
        result.last_cam_y = origin[1].floor();
        result.write_plane(queue, pixels);
        result.write_source_uniform(queue);
        result
    }

    /// Replace the plane's bytes (row-major, unpadded) ahead of later work.
    #[cfg(test)]
    fn write_plane(&self, queue: &wgpu::Queue, pixels: &[u8]) {
        assert_eq!(pixels.len(), (self.width * self.height) as usize);
        crate::render::atlas_growth::write_texels(
            queue,
            &self.texture,
            [0, 0],
            [self.width, self.height],
            1,
            pixels,
        );
    }

    /// Rebuild through a submission of its own, as a frame would.
    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn rebuild_submitted(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        fog: &FogState,
        owner: InternedId,
        cam_x: f32,
        cam_y: f32,
        screen_w: u32,
        screen_h: u32,
        zoom: f32,
        native_cells: Option<&NativeCellQuery<'_>>,
        fog_options: FogDrawOptions,
    ) -> bool {
        let mut encoder = device.create_command_encoder(&Default::default());
        let rebuilt = self.rebuild_if_needed(
            device,
            queue,
            &mut encoder,
            fog,
            owner,
            cam_x,
            cam_y,
            screen_w,
            screen_h,
            zoom,
            native_cells,
            fog_options,
        );
        queue.submit([encoder.finish()]);
        rebuilt
    }

    /// The plane's raw bytes over a rectangle, after every submitted
    /// composition: a synchronous readback for diagnostics and tests.
    pub(crate) fn read_plane(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        origin: [u32; 2],
        size: [u32; 2],
    ) -> Result<Vec<u8>, crate::render::frame_readback::FrameReadbackError> {
        let mut encoder = device.create_command_encoder(&Default::default());
        let pending = crate::render::frame_readback::PendingTextureReadback::encode_r8(
            device,
            &mut encoder,
            &self.texture,
            origin,
            size,
        )?;
        let submission = queue.submit([encoder.finish()]);
        pending.finish(device, submission, std::time::Duration::from_secs(30))
    }

    /// The A a world pixel holds as the world blitters read it, after every
    /// submitted composition; `None` outside the plane. A synchronous
    /// readback for diagnostics and tests.
    pub(crate) fn read_world(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        world_x: f32,
        world_y: f32,
        cam_x: f32,
        cam_y: f32,
    ) -> Option<u8> {
        let vx = (world_x - cam_x.floor()).floor() as i32;
        let vy = (world_y - cam_y.floor()).floor() as i32;
        if vx < 0 || vy < 0 || vx >= self.width as i32 || vy >= self.height as i32 {
            return None;
        }
        match self.read_plane(device, queue, [vx as u32, vy as u32], [1, 1]) {
            Ok(bytes) => Some(fog_lifted_a(bytes[0], self.fog_strength_milli)),
            Err(error) => {
                log::warn!("tactical A readback failed: {error}");
                None
            }
        }
    }
}

///480210/480266 normalize the signed selector before the native blitters.
fn native_frame_index(frame: i8) -> usize {
    match frame {
        -2 => 15,
        -1 => 0,
        value => value as usize,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::intern;
    use crate::sim::vision::apply_gap_generators;

    #[test]
    fn virtual_source_covers_the_supported_zoomed_out_view() {
        assert_eq!(virtual_dimensions([1280, 720], 0.25), [5120, 2880]);
        assert_eq!(virtual_dimensions([3840, 2160], 0.25), [15360, 8640]);
        assert_eq!(virtual_dimensions([19, 11], 0.75), [26, 15]);
    }

    /// Translucent fog only lifts the A selector; the same integer expression
    /// runs in `tactical_a.wgsl`, and 1000 keeps native A exactly.
    #[test]
    fn translucent_fog_lifts_a_toward_the_neutral_selector() {
        assert_eq!(fog_lifted_a(127, 1000), 127);
        assert_eq!(fog_lifted_a(0, 1000), 0);
        assert_eq!(fog_lifted_a(255, 1000), 255);
        assert_eq!(fog_lifted_a(2, 0), 127);
        assert_eq!(fog_lifted_a(127, 0), 127);
        assert_eq!(fog_lifted_a(2, 500), 65);
        assert_eq!(fog_lifted_a(200, 500), 163);
        assert_eq!(fog_lifted_a(255, 500), 191);
    }

    #[test]
    fn gap_operational_native_order_reaches_shroud_fill_after_restore() {
        for (fog, viewer, shrouded) in
            crate::sim::world::gap_generator_tests::gap_operational_power_loss_views()
        {
            assert_eq!(
                matches!(
                    cell_fill(&fog, viewer, 12, 12, &SHROUD_EDGE_LUT),
                    CellFill::Frame(15)
                ),
                shrouded,
                "native gap/source update order must reach the visible shroud consumer"
            );
        }
    }

    /// A 5x5 explored block so the centre cell has all eight neighbours
    /// explored and therefore needs no edge frame of its own.
    fn explored_block(fog: &mut FogState, owner: InternedId) {
        for ry in 4..=8u16 {
            for rx in 4..=8u16 {
                fog.mark_visible_for_owner(owner, rx, ry);
            }
        }
    }

    fn fog_16() -> FogState {
        FogState::new(16, 16)
    }

    /// The generator's own ground stays at full brightness.
    ///
    /// VERA painted a flat half-bright field over it — a constant with no
    /// counterpart anywhere in the retail shroud path, which dimmed the
    /// owner's own base by roughly half over a radius-11 circle for the rest
    /// of the match.
    #[test]
    fn a_friendly_gap_generator_does_not_darken_its_owners_ground() {
        let owner = intern::test_intern("Americans");
        let interner = intern::test_interner();
        let mut fog = fog_16();
        explored_block(&mut fog, owner);
        apply_gap_generators(
            &mut fog,
            &Default::default(),
            &[(owner, 6, 6, 2)],
            &interner,
        );

        assert!(
            fog.is_cell_gap_fog(owner, 6, 6),
            "the sim still records friendly gap coverage for other consumers"
        );
        assert_eq!(
            cell_fill(&fog, owner, 6, 6, &SHROUD_EDGE_LUT),
            CellFill::None,
            "the shroud buffer must leave the generator's own ground alone"
        );
    }

    /// Past mapping without a sustained sight contribution is concealed.
    #[test]
    fn a_hostile_gap_generator_still_blacks_the_cell_out() {
        let victim = intern::test_intern("Soviet");
        let gapper = intern::test_intern("Americans");
        let interner = intern::test_interner();
        let mut fog = fog_16();
        explored_block(&mut fog, victim);
        apply_gap_generators(
            &mut fog,
            &Default::default(),
            &[(gapper, 6, 6, 2)],
            &interner,
        );

        assert_eq!(
            cell_fill(&fog, victim, 6, 6, &SHROUD_EDGE_LUT),
            CellFill::Frame(15)
        );
    }

    /// RevealEntireMap (a local defeat or Shroud=no) lifts the full shroud from
    /// unseen ground for the current house alone.
    #[test]
    fn a_whole_map_reveal_lifts_the_full_shroud_for_the_current_house() {
        let mut sim = crate::sim::world::Simulation::with_seed(0x577F30);
        sim.fog_mut_for_test().set_width_for_test(8);
        sim.fog_mut_for_test().set_height_for_test(6);
        let local = sim.intern("Player");
        let other = sim.intern("Computer");
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(local));
        assert_eq!(
            cell_fill(sim.fog(), local, 3, 2, &SHROUD_EDGE_LUT),
            CellFill::Frame(15)
        );

        sim.reveal_entire_map(
            Some(local),
            crate::sim::world::UninitContext::new(
                crate::rules::ruleset::RuleSet::empty_for_test(),
                None,
            ),
        );

        assert_eq!(
            cell_fill(sim.fog(), local, 3, 2, &SHROUD_EDGE_LUT),
            CellFill::None
        );
        assert_eq!(
            cell_fill(sim.fog(), other, 3, 2, &SHROUD_EDGE_LUT),
            CellFill::Frame(15)
        );
    }

    #[test]
    fn shroud_current_sight_keeps_tactical_cell_bright_until_pending_boundary() {
        let views = crate::sim::world::gap_generator_tests::sustained_source_gap_limbo_views();
        assert_eq!(views.len(), 5);
        for (fog, owner, bright) in views {
            assert_eq!(
                cell_fill(&fog, owner, 16, 16, &SHROUD_EDGE_LUT),
                if bright {
                    CellFill::None
                } else {
                    CellFill::Frame(15)
                },
                "the production source/gap/Limbo/120-frame sweep reaches the A selector"
            );
        }
    }

    #[test]
    fn shroud_current_sight_nontransitive_allied_view_stays_dark() {
        let views = crate::sim::world::gap_generator_tests::nontransitive_source_gap_views();
        assert_eq!(views.len(), 4);
        for (fog, direct_ally, indirect_ally) in views {
            assert_eq!(
                cell_fill(&fog, direct_ally, 16, 16, &SHROUD_EDGE_LUT),
                CellFill::None
            );
            assert_eq!(
                cell_fill(&fog, indirect_ally, 16, 16, &SHROUD_EDGE_LUT),
                CellFill::Frame(15)
            );
            assert!(!fog.is_cell_visible(indirect_ally, 16, 16));
        }
    }

    #[test]
    fn unexplored_ground_is_dark_and_a_shroud_boundary_picks_a_frame() {
        let owner = intern::test_intern("Americans");
        let mut fog = fog_16();
        explored_block(&mut fog, owner);

        assert_eq!(
            cell_fill(&fog, owner, 12, 12, &SHROUD_EDGE_LUT),
            CellFill::Frame(15),
            "never-explored ground is fully shrouded"
        );
        // A corner of the explored block borders shroud on five sides, so it
        // takes an edge frame rather than the full diamond.
        match cell_fill(&fog, owner, 4, 4, &SHROUD_EDGE_LUT) {
            CellFill::Frame(idx) => assert!(idx < 48, "frame {idx} is outside SHROUD.SHP"),
            other => panic!("a shroud boundary cell must take a frame, got {other:?}"),
        }
    }

    /// The sim bumps the view generation every committed tick. One that
    /// changes no cell in view keeps the drawn plane and records no work; a
    /// change in view redraws exactly what a fresh buffer draws.
    #[test]
    #[ignore = "requires GPU; the A plane's composition"]
    fn a_fog_generation_that_changes_no_cell_in_view_keeps_the_drawn_plane() {
        let gpu = crate::render::terrain_draw_gpu_tests::Gpu::new();
        let owner = intern::test_intern("Americans");
        // Distinct frames, so a wrong selection changes bytes.
        let frames: Vec<Vec<u8>> = (0..48u8).map(|frame| vec![frame; 60 * 30]).collect();
        let size = [320, 200];
        let new_source = || {
            ShroudBuffer::new(
                &gpu.device,
                &gpu.queue,
                size,
                [32, 32],
                frames.clone(),
                [60, 30],
                SHROUD_EDGE_LUT,
            )
        };
        // The view's centre is cell (16, 16).
        let rebuild = |source: &mut ShroudBuffer, fog: &FogState| {
            source.rebuild_submitted(
                &gpu.device,
                &gpu.queue,
                fog,
                owner,
                -190.0,
                410.0,
                size[0],
                size[1],
                1.0,
                None,
                FogDrawOptions::default(),
            )
        };
        let mut fog = FogState::new(32, 32);
        for ry in 14..=18 {
            for rx in 14..=18 {
                fog.mark_visible_for_owner(owner, rx, ry);
            }
        }
        let plane = |source: &ShroudBuffer| {
            source
                .read_plane(&gpu.device, &gpu.queue, [0, 0], size)
                .unwrap()
        };
        let mut source = new_source();
        assert!(rebuild(&mut source, &fog));
        let drawn = plane(&source);

        fog.mark_visible_for_owner(owner, 31, 0);
        fog.bump_view_generation();
        assert!(!rebuild(&mut source, &fog), "the change is out of view");
        assert_eq!(plane(&source), drawn);

        fog.mark_visible_for_owner(owner, 19, 16);
        fog.bump_view_generation();
        assert!(rebuild(&mut source, &fog));
        assert_ne!(plane(&source), drawn);
        let mut fresh = new_source();
        assert!(rebuild(&mut fresh, &fog));
        assert_eq!(plane(&source), plane(&fresh));
    }

    /// `AlphaShapeClass::DrawAll_NoMask @ 0x00421350` after the cells: frame
    /// 0 from its own offset and past its canvas's right edge (only the plane
    /// clips right and bottom), the plane's left and bottom edges clipping it,
    /// each byte rescaling A (254 doubles, 0 zeroes). A changed glow list
    /// redraws a plane whose fog is unchanged; an equal one does not.
    #[test]
    fn a_glow_rescales_a_under_frame_zero_and_redraws_when_it_changes() {
        let gpu = crate::render::terrain_draw_gpu_tests::Gpu::new();
        let owner = intern::test_intern("Americans");
        let size = [64, 32];
        let mut source = ShroudBuffer::new(
            &gpu.device,
            &gpu.queue,
            size,
            [32, 32],
            Vec::new(),
            [60, 30],
            SHROUD_EDGE_LUT,
        );
        let fog = FogState::new(32, 32);
        let rebuild = |source: &mut ShroudBuffer| {
            source.rebuild_submitted(
                &gpu.device,
                &gpu.queue,
                &fog,
                owner,
                100.0,
                50.0,
                size[0],
                size[1],
                1.0,
                None,
                FogDrawOptions::default(),
            )
        };
        assert!(rebuild(&mut source));
        let image = std::sync::Arc::new(crate::rules::alpha_image_assets::AlphaImage::for_test(
            [3, 4],
            [1, 1, 3, 2],
            vec![255, 254, 0, 9, 9, 9],
        ));
        // Plane origin (-2,30): the canvas ends at x 1, and frame 0 covers
        // x -1..2 and y 31..33.
        let glow = || AlphaShapeDraw {
            origin: [98, 80],
            image: image.clone(),
        };
        source.set_alpha_shapes([glow()]);
        assert!(rebuild(&mut source));
        let a = |x: f32, y: f32| {
            source.read_world(&gpu.device, &gpu.queue, 100.0 + x, 50.0 + y, 100.0, 50.0)
        };
        assert_eq!(
            [a(0.0, 31.0), a(1.0, 31.0), a(2.0, 31.0), a(0.0, 30.0)],
            [Some(254), Some(0), Some(NEUTRAL), Some(NEUTRAL)]
        );

        source.set_alpha_shapes([glow()]);
        assert!(!rebuild(&mut source));
        source.set_alpha_shapes([]);
        assert!(rebuild(&mut source));
        assert_eq!(
            source.read_world(&gpu.device, &gpu.queue, 100.0, 81.0, 100.0, 50.0),
            Some(NEUTRAL)
        );
    }
}

/// Extract raw brightness pixel data from a loaded SHROUD.SHP file.
///
/// Returns `(frame_pixels, canvas_w, canvas_h)` where each entry in
/// `frame_pixels` is a `canvas_w × canvas_h` buffer of raw SHP pixel values.
pub fn extract_shp_brightness(shp: &crate::assets::shp_file::ShpFile) -> (Vec<Vec<u8>>, u32, u32) {
    let canvas_w = shp.width as u32;
    let canvas_h = shp.height as u32;
    let canvas_size = (canvas_w * canvas_h) as usize;
    let mut all_frames: Vec<Vec<u8>> = Vec::with_capacity(shp.frames.len());

    for frame in &shp.frames {
        // Start with TRANSPARENT so pixels outside the actual SHP subframe
        // are skipped by the copy and blend. Only real SHP pixels affect A.
        let mut buf = vec![TRANSPARENT; canvas_size];
        let fw = frame.frame_width as u32;
        let fh = frame.frame_height as u32;
        let fx = frame.frame_x as u32;
        let fy = frame.frame_y as u32;

        for row in 0..fh {
            for col in 0..fw {
                let src = (row * fw + col) as usize;
                let pixel = frame.pixels[src];
                let dx = fx + col;
                let dy = fy + row;
                if dx < canvas_w && dy < canvas_h {
                    buf[(dy * canvas_w + dx) as usize] = pixel;
                }
            }
        }
        all_frames.push(buf);
    }

    (all_frames, canvas_w, canvas_h)
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct SourceUniform {
    origin: [f32; 2],
    enabled: u32,
    fog_strength_milli: u32,
}
impl SourceUniform {
    fn new(origin: [f32; 2], enabled: bool, fog_strength_milli: u32) -> Self {
        Self {
            origin,
            enabled: u32::from(enabled),
            fog_strength_milli,
        }
    }
}
fn source_uniform(
    device: &wgpu::Device,
    origin: [f32; 2],
    enabled: bool,
    fog_strength_milli: u32,
) -> wgpu::Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Tactical A source origin"),
        contents: bytemuck::bytes_of(&SourceUniform::new(origin, enabled, fog_strength_milli)),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    })
}

/// A disabled source for operation classes that do not read native A (surface
/// stores and screen UI), and for an absent/sandbox world. This constant is not
/// a second live A plane. Keeping it with the source owner also shares its ABI.
pub(crate) fn neutral_gpu_source(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> (wgpu::TextureView, wgpu::Buffer) {
    let texture = create_plane_texture(
        device,
        "Neutral tactical A",
        [1, 1],
        wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
    );
    crate::render::atlas_growth::write_texels(queue, &texture, [0, 0], [1, 1], 1, &[NEUTRAL]);
    (
        texture.create_view(&Default::default()),
        source_uniform(device, [0.0; 2], false, 1000),
    )
}

#[cfg(test)]
#[path = "shroud_native_tests.rs"]
mod native_tests;
