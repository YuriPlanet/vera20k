//! The GPU voxel raster: the encoded-span raster of 7DF9C0/7DFAE0 and the
//! shadow stencil of 756860, executed into the pose pages
//! ([`crate::render::unit_pose_cache`]).
//!
//! The CPU keeps each draw's x87 setup ([`NativeRaster`], [`StencilRaster`]:
//! corners, 8.8 steps, crop and lighting pages) and each section's encoded
//! columns, read once ([`section_voxels`], [`stencil_columns`]). The GPU puts
//! each store where the native cursor puts it. The last native store to a
//! pixel wins, stores of zero included: a store's depth is its place in
//! native order (sections in 754510's order, columns in the raster loop's,
//! voxels along the walk), and only a later store passes the test.
//!
//! A draw stays on the CPU where native rasterizes it depth-tested
//! (UseBuffer, 757980/757BF0), where a section's forward and backward walks
//! read different voxels, or where the CPU raster would fail.

use std::collections::HashMap;
use std::sync::Arc;

use wgpu::util::DeviceExt;

use crate::assets::vpl_file::VplFile;
use crate::assets::vxl_file::VxlLimb;
use crate::render::shroud_buffer::{layout_entry, texture_entry};
use crate::render::vxl_raster::shadow::{StencilRaster, stencil_address, stencil_columns};
use crate::render::vxl_raster::{NativeRaster, RasterParams, VoxelImage, section_voxels};

/// The padding after each page image, which its clear covers too.
pub(crate) fn shader_source() -> String {
    format!(
        "const PADDING: i32 = {};\n{}",
        crate::render::atlas_growth::SPRITE_PADDING,
        include_str!("voxel_gpu.wgsl")
    )
}

/// The paint shader's flags.
const MIRROR_X: u32 = 1;
const MIRROR_Y: u32 = 2;
const REVERSE: u32 = 4;
const LIT: u32 = 8;

/// A store's place in native order must stay exact as a depth: below 2^24.
const SEQUENCE_LIMIT: u32 = 1 << 24;

/// The shader's `Draw` block.
const DRAW_SIZE: usize = 80 + 256;

/// The widest skip texture row; the WebGL2 texture limit.
const SKIP_ROW: usize = 2048;

/// A section of a loaded VXL: its body allocation and limb index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct LimbKey(usize, usize);

impl LimbKey {
    fn of(limb: &VxlLimb, index: usize) -> Option<(Self, &Arc<[u8]>)> {
        let body = &limb.native_spans.as_ref()?.body;
        Some((Self(Arc::as_ptr(body).cast::<u8>() as usize, index), body))
    }
}

/// A voxel image the GPU raster draws: an ordinary draw's sections, or a
/// shadow stencil.
#[derive(Debug, Clone)]
pub(crate) enum GpuImage {
    Paint {
        sections: Vec<(LimbKey, RasterParams)>,
        pages: [u8; 256],
        /// The crop: x, y, width, height in the 256-square buffer.
        crop: [i32; 4],
    },
    Stencil {
        limb: LimbKey,
        raster: StencilRaster,
    },
}

/// An image's place on a page: its corner, and the image.
pub(crate) type PlacedImage<'a> = ([u32; 2], &'a GpuImage);

/// One page's pass: the page, and the GPU images on it. The pass keeps the
/// rest of the page.
pub(crate) struct PagePass<'a> {
    pub target: &'a wgpu::TextureView,
    pub images: Vec<PlacedImage<'a>>,
}

struct Limb {
    /// Holds the allocation, so no other VXL takes the key.
    _body: Arc<[u8]>,
    /// Built on the limb's first paint, None when it cannot paint here.
    voxels: Option<Option<LimbVoxels>>,
    /// Built on the limb's first stencil, None when it cannot mark here.
    columns: Option<Option<LimbColumns>>,
}

struct LimbVoxels {
    count: u32,
    /// The voxels and skips, until uploaded.
    pending: Option<(Vec<[u32; 3]>, Vec<u8>)>,
    gpu: Option<(wgpu::Buffer, wgpu::BindGroup)>,
}

struct LimbColumns {
    columns: Vec<u32>,
    gpu: Option<wgpu::Buffer>,
}

/// The GPU raster's limb data and, once a draw needs them, its pipelines.
#[derive(Default)]
pub(crate) struct VoxelGpu {
    limbs: HashMap<LimbKey, Limb>,
    raster: Option<Raster>,
}

impl VoxelGpu {
    /// `image` for the GPU raster, or None where it stays on the CPU.
    pub(crate) fn admit(
        &mut self,
        image: &VoxelImage<'_>,
        vpl: Option<&VplFile>,
    ) -> Option<GpuImage> {
        match image {
            VoxelImage::Cpu(_) => None,
            VoxelImage::Paint { vxl, raster, .. } => {
                // The VPL becomes a texture, at most 2048 rows on WebGL2.
                if vpl.is_some_and(|vpl| vpl.pages_slice().len() > 2048) {
                    return None;
                }
                let NativeRaster {
                    sections,
                    pages,
                    rect,
                } = raster;
                let mut sequence = 0u32;
                let mut admitted = Vec::with_capacity(sections.len());
                for (index, params) in sections {
                    let key = self.voxels(vxl.limbs.get(*index)?, *index)?;
                    let [nx, ny, _] = params.sizes.map(u32::from);
                    // The raster loop's start column is the far corner of
                    // each mirrored axis (`ORIENTATIONS` in vxl_native).
                    let [start, step_x, step_y] = params.column_steps;
                    let expected = if step_x < 0 { nx as i32 - 1 } else { 0 }
                        + if step_y < 0 {
                            (ny as i32 - 1) * nx as i32
                        } else {
                            0
                        };
                    if start != expected || step_x.abs() != 1 || step_y.unsigned_abs() != nx {
                        return None;
                    }
                    sequence = sequence
                        .checked_add((nx * ny).checked_mul(256)?)
                        .filter(|&end| end < SEQUENCE_LIMIT)?;
                    admitted.push((key, params.clone()));
                }
                Some(GpuImage::Paint {
                    sections: admitted,
                    pages: *pages,
                    crop: [rect[2], rect[3], rect[4], rect[5]],
                })
            }
            VoxelImage::Stencil { vxl, raster } => {
                let limb = vxl.limbs.get(raster.limb)?;
                let key = self.columns(limb, raster.limb)?;
                let columns = &self.limbs[&key].columns.as_ref()?.as_ref()?.columns;
                // A second byte past the buffer fails 756860's raster here.
                if columns
                    .iter()
                    .any(|&column| stencil_address(raster, column) == u16::MAX)
                {
                    return None;
                }
                Some(GpuImage::Stencil {
                    limb: key,
                    raster: raster.clone(),
                })
            }
        }
    }

    fn limb(&mut self, limb: &VxlLimb, index: usize) -> Option<(LimbKey, &mut Limb)> {
        let (key, body) = LimbKey::of(limb, index)?;
        let entry = self.limbs.entry(key).or_insert_with(|| Limb {
            _body: body.clone(),
            voxels: None,
            columns: None,
        });
        Some((key, entry))
    }

    fn voxels(&mut self, limb: &VxlLimb, index: usize) -> Option<LimbKey> {
        let (key, entry) = self.limb(limb, index)?;
        entry
            .voxels
            .get_or_insert_with(|| {
                let voxels = section_voxels(limb)?;
                // Skips reach the shader as texture rows.
                (voxels.skips.len() <= SKIP_ROW * SKIP_ROW).then(|| LimbVoxels {
                    count: voxels.voxels.len() as u32,
                    pending: Some((voxels.voxels, voxels.skips)),
                    gpu: None,
                })
            })
            .as_ref()?;
        Some(key)
    }

    fn columns(&mut self, limb: &VxlLimb, index: usize) -> Option<LimbKey> {
        let (key, entry) = self.limb(limb, index)?;
        entry
            .columns
            .get_or_insert_with(|| {
                stencil_columns(limb).map(|columns| LimbColumns { columns, gpu: None })
            })
            .as_ref()?;
        Some(key)
    }

    /// Draw each `page_size` page's GPU images, each over its cleared
    /// rectangle and padding. `vpl` is the one every admitted paint was lit
    /// with. Once per submission: every draw reads its block from one buffer.
    pub(crate) fn encode(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        page_size: [u32; 2],
        pages: &[PagePass<'_>],
        vpl: Option<&VplFile>,
    ) {
        if pages.iter().all(|page| page.images.is_empty()) {
            return;
        }
        let raster = self
            .raster
            .get_or_insert_with(|| Raster::new(device, queue, vpl, page_size));
        raster.fit_depth(device, page_size);
        let blocks = draw_blocks(pages, page_size, raster.vpl_rows);
        let mut offsets = raster.write_draws(device, queue, &blocks).into_iter();
        self.upload(device, queue);
        let raster = self.raster.as_ref().expect("created above");
        for page in pages.iter().filter(|page| !page.images.is_empty()) {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Voxel pose page"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: page.target,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &raster.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            for &(_, image) in &page.images {
                let offset = offsets.next().expect("one block per clear");
                pass.set_pipeline(&raster.clear);
                pass.set_bind_group(0, &raster.draws, &[offset]);
                pass.draw(0..4, 0..1);
                match image {
                    GpuImage::Paint { sections, .. } => {
                        pass.set_pipeline(&raster.paint);
                        for (key, _) in sections {
                            let offset = offsets.next().expect("one block per section");
                            let Some(Some(LimbVoxels {
                                count,
                                gpu: Some((buffer, skips)),
                                ..
                            })) = self.limbs.get(key).and_then(|limb| limb.voxels.as_ref())
                            else {
                                continue;
                            };
                            pass.set_bind_group(0, &raster.draws, &[offset]);
                            pass.set_bind_group(1, skips, &[]);
                            pass.set_vertex_buffer(0, buffer.slice(..));
                            pass.draw(0..*count, 0..1);
                        }
                    }
                    GpuImage::Stencil { limb, .. } => {
                        let offset = offsets.next().expect("one block per stencil");
                        let Some(Some(LimbColumns {
                            columns,
                            gpu: Some(buffer),
                        })) = self.limbs.get(limb).and_then(|limb| limb.columns.as_ref())
                        else {
                            continue;
                        };
                        pass.set_pipeline(&raster.stencil);
                        pass.set_bind_group(0, &raster.draws, &[offset]);
                        pass.set_vertex_buffer(0, buffer.slice(..));
                        pass.draw(0..2, 0..columns.len() as u32);
                    }
                }
            }
        }
    }

    /// Upload the limbs admitted since the last encode.
    fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let Some(raster) = self.raster.as_ref() else {
            return;
        };
        for limb in self.limbs.values_mut() {
            if let Some(Some(voxels)) = limb.voxels.as_mut()
                && let Some((data, skips)) = voxels.pending.take()
                && !data.is_empty()
            {
                let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Voxel section voxels"),
                    contents: bytemuck::cast_slice(&data),
                    usage: wgpu::BufferUsages::VERTEX,
                });
                voxels.gpu = Some((buffer, raster.skip_group(device, queue, &skips)));
            }
            if let Some(Some(columns)) = limb.columns.as_mut()
                && columns.gpu.is_none()
                && !columns.columns.is_empty()
            {
                columns.gpu = Some(
                    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("Voxel stencil columns"),
                        contents: bytemuck::cast_slice(&columns.columns),
                        usage: wgpu::BufferUsages::VERTEX,
                    }),
                );
            }
        }
    }
}

fn pack(words: [u16; 2]) -> u32 {
    u32::from(words[0]) | u32::from(words[1]) << 16
}

fn pack_steps([x, y]: [i16; 2]) -> u32 {
    pack([x as u16, y as u16])
}

/// Each draw's block, in the order [`VoxelGpu::encode`] draws them.
fn draw_blocks(
    pages: &[PagePass<'_>],
    [width, height]: [u32; 2],
    vpl_rows: u32,
) -> Vec<[u8; DRAW_SIZE]> {
    let mut blocks = Vec::new();
    for page in pages {
        for &([x, y], image) in &page.images {
            let target = [x, y, width, height];
            blocks.push(clear_draw(image, target));
            match image {
                GpuImage::Paint {
                    sections,
                    pages: lighting,
                    crop,
                } => {
                    let mut sequence = 0;
                    for (_, params) in sections {
                        blocks.push(paint_draw(
                            params, lighting, vpl_rows, sequence, *crop, target,
                        ));
                        let [nx, ny, _] = params.sizes.map(u32::from);
                        sequence += nx * ny * 256;
                    }
                }
                GpuImage::Stencil { raster, .. } => blocks.push(stencil_draw(raster, target)),
            }
        }
    }
    blocks
}

/// A `Draw` block: twenty words, then the lighting pages.
fn draw_block(words: [u32; 20], pages: &[u8; 256]) -> [u8; DRAW_SIZE] {
    let mut block = [0; DRAW_SIZE];
    for (bytes, word) in block.chunks_exact_mut(4).zip(words) {
        bytes.copy_from_slice(&word.to_le_bytes());
    }
    block[80..].copy_from_slice(pages);
    block
}

fn paint_draw(
    params: &RasterParams,
    pages: &[u8; 256],
    vpl_rows: u32,
    sequence: u32,
    [x, y, width, height]: [i32; 4],
    target: [u32; 4],
) -> [u8; DRAW_SIZE] {
    let [nx, ny, _] = params.sizes.map(u32::from);
    let [_, step_x, step_y] = params.column_steps;
    let mut flags = 0;
    if step_x < 0 {
        flags |= MIRROR_X;
    }
    if step_y < 0 {
        flags |= MIRROR_Y;
    }
    if params.reverse {
        flags |= REVERSE;
    }
    if vpl_rows > 0 {
        flags |= LIT;
    }
    let [dx, dy] = params.axis_xy[2].map(|step| i32::from(step) as u32);
    draw_block(
        [
            pack(params.origin_xy),
            pack_steps(params.axis_xy[0]),
            pack_steps(params.axis_xy[1]),
            pack_steps(params.axis_xy[2]),
            dx,
            dy,
            nx,
            ny,
            flags,
            sequence,
            vpl_rows,
            0,
            x as u32,
            y as u32,
            width as u32,
            height as u32,
            target[0],
            target[1],
            target[2],
            target[3],
        ],
        pages,
    )
}

/// The image's clear: its size as the crop's.
fn clear_draw(image: &GpuImage, target: [u32; 4]) -> [u8; DRAW_SIZE] {
    let [width, height] = match image {
        GpuImage::Paint { crop, .. } => [crop[2], crop[3]],
        GpuImage::Stencil { raster, .. } => [raster.rect[4], raster.rect[5]],
    };
    let mut words = [0; 20];
    words[14] = width as u32;
    words[15] = height as u32;
    words[16..].copy_from_slice(&target);
    draw_block(words, &[0; 256])
}

fn stencil_draw(raster: &StencilRaster, target: [u32; 4]) -> [u8; DRAW_SIZE] {
    let [_, _, x, y, width, height] = raster.rect;
    draw_block(
        [
            pack(raster.origin),
            pack(raster.step_x),
            pack(raster.step_y),
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            x as u32,
            y as u32,
            width as u32,
            height as u32,
            target[0],
            target[1],
            target[2],
            target[3],
        ],
        &[0; 256],
    )
}

struct Raster {
    clear: wgpu::RenderPipeline,
    paint: wgpu::RenderPipeline,
    stencil: wgpu::RenderPipeline,
    draw_layout: wgpu::BindGroupLayout,
    skip_layout: wgpu::BindGroupLayout,
    /// The lighting table: one row of 256 indices per VPL page.
    vpl: wgpu::TextureView,
    vpl_rows: u32,
    uniforms: wgpu::Buffer,
    stride: u64,
    draws: wgpu::BindGroup,
    depth: wgpu::TextureView,
    depth_size: [u32; 2],
}

impl Raster {
    fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        vpl: Option<&VplFile>,
        size: [u32; 2],
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Voxel raster"),
            source: wgpu::ShaderSource::Wgsl(shader_source().into()),
        });
        let uint = wgpu::TextureSampleType::Uint;
        let draw_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Voxel raster draw"),
            entries: &[
                layout_entry(
                    0,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(DRAW_SIZE as u64),
                    },
                ),
                texture_entry(1, uint),
            ],
        });
        let skip_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Voxel raster skips"),
            entries: &[texture_entry(0, uint)],
        });
        let voxel_attributes = wgpu::vertex_attr_array![0 => Uint32x3];
        let column_attributes = wgpu::vertex_attr_array![0 => Uint32];
        let pipeline = |label,
                        layouts: &[Option<&wgpu::BindGroupLayout>],
                        vertex: &'static str,
                        buffer: Option<wgpu::VertexBufferLayout<'_>>,
                        topology: wgpu::PrimitiveTopology,
                        depth_write: bool,
                        depth_compare: wgpu::CompareFunction| {
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
                    buffers: &buffer.into_iter().map(Some).collect::<Vec<_>>(),
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_store"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::R8Uint,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(depth_write),
                    depth_compare: Some(depth_compare),
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let clear = pipeline(
            "Voxel image clear",
            &[Some(&draw_layout)],
            "vs_clear",
            None,
            wgpu::PrimitiveTopology::TriangleStrip,
            false,
            wgpu::CompareFunction::Always,
        );
        let paint = pipeline(
            "Voxel paint (7DF9C0/7DFAE0)",
            &[Some(&draw_layout), Some(&skip_layout)],
            "vs_paint",
            Some(wgpu::VertexBufferLayout {
                array_stride: 12,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &voxel_attributes,
            }),
            wgpu::PrimitiveTopology::PointList,
            true,
            wgpu::CompareFunction::Greater,
        );
        let stencil = pipeline(
            "Voxel shadow stencil (756860)",
            &[Some(&draw_layout)],
            "vs_stencil",
            Some(wgpu::VertexBufferLayout {
                array_stride: 4,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &column_attributes,
            }),
            wgpu::PrimitiveTopology::PointList,
            false,
            wgpu::CompareFunction::Always,
        );
        let pages = vpl.map_or(&[][..], VplFile::pages_slice);
        let vpl_rows = pages.len() as u32;
        let vpl = uint_texture(
            device,
            queue,
            "Voxel lighting table",
            [256, vpl_rows.max(1)],
            &if pages.is_empty() {
                vec![0; 256]
            } else {
                pages.concat()
            },
        );
        let stride = u64::from(device.limits().min_uniform_buffer_offset_alignment)
            .max(DRAW_SIZE as u64)
            .next_multiple_of(u64::from(
                device.limits().min_uniform_buffer_offset_alignment,
            ));
        let uniforms = uniform_buffer(device, stride * 64);
        let draws = draw_group(device, &draw_layout, &uniforms, &vpl);
        let (depth, depth_size) = (depth_view(device, size), size);
        Self {
            clear,
            paint,
            stencil,
            draw_layout,
            skip_layout,
            vpl,
            vpl_rows,
            uniforms,
            stride,
            draws,
            depth,
            depth_size,
        }
    }

    /// The depth attachment matches the pages.
    fn fit_depth(&mut self, device: &wgpu::Device, size: [u32; 2]) {
        if self.depth_size != size {
            self.depth = depth_view(device, size);
            self.depth_size = size;
        }
    }

    /// Write every draw's block and return their dynamic offsets.
    fn write_draws(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        draws: &[[u8; DRAW_SIZE]],
    ) -> Vec<u32> {
        let needed = self.stride * draws.len() as u64;
        if needed > self.uniforms.size() {
            self.uniforms = uniform_buffer(device, needed.next_power_of_two());
            self.draws = draw_group(device, &self.draw_layout, &self.uniforms, &self.vpl);
        }
        let mut bytes = vec![0; needed as usize];
        for (index, draw) in draws.iter().enumerate() {
            let at = index * self.stride as usize;
            bytes[at..at + draw.len()].copy_from_slice(draw);
        }
        if !bytes.is_empty() {
            queue.write_buffer(&self.uniforms, 0, &bytes);
        }
        (0..draws.len())
            .map(|index| (index as u64 * self.stride) as u32)
            .collect()
    }

    /// A section's skips as texture rows, at most [`SKIP_ROW`] wide.
    fn skip_group(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        skips: &[u8],
    ) -> wgpu::BindGroup {
        let width = skips.len().clamp(1, SKIP_ROW);
        let height = skips.len().div_ceil(width).max(1);
        let mut texels = skips.to_vec();
        texels.resize(width * height, 0);
        let view = uint_texture(
            device,
            queue,
            "Voxel section skips",
            [width as u32, height as u32],
            &texels,
        );
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Voxel section skips"),
            layout: &self.skip_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            }],
        })
    }
}

fn uint_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &'static str,
    [width, height]: [u32; 2],
    texels: &[u8],
) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
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
    crate::render::atlas_growth::write_texels(queue, &texture, [0, 0], [width, height], 1, texels);
    texture.create_view(&Default::default())
}

fn uniform_buffer(device: &wgpu::Device, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Voxel raster draws"),
        size,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn draw_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniforms: &wgpu::Buffer,
    vpl: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Voxel raster draw"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: uniforms,
                    offset: 0,
                    size: wgpu::BufferSize::new(DRAW_SIZE as u64),
                }),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(vpl),
            },
        ],
    })
}

fn depth_view(device: &wgpu::Device, [width, height]: [u32; 2]) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("Voxel raster store order"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

#[cfg(test)]
#[path = "voxel_gpu_tests.rs"]
mod tests;
