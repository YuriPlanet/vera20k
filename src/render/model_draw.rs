//! VERA-only, experimental model art (`Model=`, `docs/picture-model-art.md`):
//! declared glTF models drawn by the GPU every frame.
//!
//! While the frame's pieces are built, each object that draws a declared
//! model asks for its pose ([`ModelFrame::request`]); equal poses share one
//! image. Before the world draws, one render pass draws every asked pose's
//! body, turret and barrel meshes into the sprite atlas's model page
//! ([`SpriteAtlas::install_model_page`]), each image in the rectangle a voxel
//! draw of the same pose takes ([`native_draw_bounds_from_extents`]). The
//! page is RGBA, lit with the voxel world light, with a palette-index plane
//! holding [`LIT_INDEX`] for lit pixels and [`UNLIT_INDEX`] for the mask's
//! unlit and the emissive ones, as picture pages hold. The pieces draw it
//! through the SHP blitters' RGBA source mode, so native Z from the drawn
//! rectangle, Display order, darkening and cell light follow unchanged.
//!
//! Nothing of a pose outlives its frame: the page is cleared and drawn again
//! each frame that asks for one, and a frame that asks for none draws
//! nothing. Only the meshes and textures of each model stay on the GPU.
//!
//! Model space: glTF +Y is up, +Z forward and +X left. A model draws with
//! +Z along the voxel hull's X (forward), X along its Y and +Y along its Z,
//! scaled so the `body` node's rest length along +Z is `ModelSize=` leptons
//! in voxel units ([`MODEL_UNITS_PER_LEPTON_BITS`]), which are screen pixels
//! (`VxlRenderParams::scale`). The origin is the point the object stands on,
//! and a turret turns about the vertical through it moved by `TurretOffset=`,
//! as a `TUR.VXL` does.
//!
//! [`native_draw_bounds_from_extents`]: crate::render::vxl_raster::native_draw_bounds_from_extents
//! [`MODEL_UNITS_PER_LEPTON_BITS`]: crate::map::voxel_pose::MODEL_UNITS_PER_LEPTON_BITS

use std::collections::HashMap;
use std::sync::Arc;

use glam::{Mat3, Mat4, Vec3, Vec4};
use wgpu::util::DeviceExt;

use crate::assets::asset_manager::AssetManager;
use crate::assets::model_file::{AlphaMode, Model, ModelBounds, ModelPart};
use crate::render::atlas_growth::ShelfCursor;
use crate::render::batch::BatchRenderer;
use crate::render::picture_atlas::{LIT_INDEX, UNLIT_INDEX};
use crate::render::shroud_buffer::{layout_entry, texture_entry};
use crate::render::sprite_atlas::SpriteAtlas;
use crate::rules::voxel_asset_catalog::DeclaredModel;

/// The first model page's side; a frame whose images do not fit doubles it
/// for the next frame, up to the device's texture limit.
const PAGE_START: u32 = 1024;

/// Voxel units of depth the page's depth range spans, centred on the anchor.
const DEPTH_SPAN: f32 = 4096.0;

/// The shader's `Draw` block (`model_draw.wgsl`).
const DRAW_SIZE: u64 = 432;

/// The vertex: position, normal, UV and colour.
const VERTEX_FLOATS: usize = 12;

const _: () = assert!(
    LIT_INDEX == 1 && UNLIT_INDEX == 240,
    "model_draw.wgsl writes these"
);

/// One object's model in one pose.
pub(crate) struct ModelPose {
    pub model: Arc<DeclaredModel>,
    /// The camera-space draw matrices of the body, the turret and the barrel,
    /// in voxel units (`vxl_raster::ground_unit_pose_matrices`).
    pub matrices: [Mat4; 3],
    /// The gun whose `turretN` and `barrelN` nodes draw, when the model has
    /// them; None draws the body alone.
    pub gun: Option<u32>,
    /// The owner's house ramp (palette indices 16..31) for the mask's red
    /// pixels; None leaves them their own colour.
    pub ramp: Option<[[u8; 3]; 16]>,
}

/// Where a pose's image is this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ModelImage {
    /// The sprite atlas page ([`SpriteAtlas::model_page`]).
    pub page: usize,
    pub uv_origin: [f32; 2],
    pub uv_size: [f32; 2],
    /// x, y, width and height relative to the draw anchor, as a voxel draw's
    /// destination rectangle (`native_vxl_draw_bounds`).
    pub rect: [i32; 4],
}

#[derive(PartialEq, Eq, Hash)]
struct PoseKey {
    model: usize,
    matrices: [[u32; 16]; 3],
    gun: Option<u32>,
    ramp: Option<[[u8; 3]; 16]>,
}

/// The CPU half of a model: its parts' nodes and bounds.
struct Layout {
    /// glTF model space to voxel space.
    to_voxels: Mat4,
    /// Each part's mesh nodes with their rest-pose model matrices, and the
    /// part's rest bounds.
    parts: Vec<(ModelPart, Vec<(usize, Mat4)>, Option<ModelBounds>)>,
}

impl Layout {
    fn of(declared: &DeclaredModel) -> Self {
        let model = &declared.model;
        let length = model
            .body_bounds()
            .map_or(1.0, |bounds| bounds.max[2] - bounds.min[2]);
        let units_per_lepton =
            f64::from_bits(crate::map::voxel_pose::MODEL_UNITS_PER_LEPTON_BITS) as f32;
        let scale = if length > 0.0 {
            declared.decl.size as f32 * units_per_lepton / length
        } else {
            0.0
        };
        // glTF +Z (forward) to voxel X, X to Y, +Y (up) to Z.
        let to_voxels = Mat4::from_cols(
            Vec4::new(0.0, scale, 0.0, 0.0),
            Vec4::new(0.0, 0.0, scale, 0.0),
            Vec4::new(scale, 0.0, 0.0, 0.0),
            Vec4::W,
        );
        let parts = model
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(index, node)| {
                let part = node.name.as_deref().and_then(ModelPart::from_name)?;
                // The first node naming a part is the part (`Model::part`).
                (part != ModelPart::Shadow && model.part(part) == Some(index))
                    .then(|| (part, part_nodes(model, index), model.part_bounds(index)))
            })
            .collect();
        Self { to_voxels, parts }
    }

    fn part(
        &self,
        part: ModelPart,
    ) -> Option<&(ModelPart, Vec<(usize, Mat4)>, Option<ModelBounds>)> {
        self.parts.iter().find(|(found, _, _)| *found == part)
    }

    /// The parts `gun` draws with, and the draw matrix index of each.
    fn drawn(
        &self,
        gun: Option<u32>,
    ) -> impl Iterator<Item = (usize, &Vec<(usize, Mat4)>, Option<ModelBounds>)> {
        [
            Some((0, ModelPart::Body)),
            gun.map(|gun| (1, ModelPart::Turret(gun))),
            gun.map(|gun| (2, ModelPart::Barrel(gun))),
        ]
        .into_iter()
        .flatten()
        .filter_map(|(matrix, part)| {
            let (_, nodes, bounds) = self.part(part)?;
            Some((matrix, nodes, *bounds))
        })
    }
}

/// The nodes with meshes under `node`, itself included, leaving out other
/// parts' subtrees as [`Model::part_bounds`] does, with their rest-pose
/// model matrices.
fn part_nodes(model: &Model, node: usize) -> Vec<(usize, Mat4)> {
    let mut nodes = Vec::new();
    let mut stack = vec![node];
    let mut visited = 0;
    while let Some(index) = stack.pop() {
        visited += 1;
        if visited > model.nodes.len() {
            break;
        }
        let Some(found) = model.nodes.get(index) else {
            continue;
        };
        if found.mesh.is_some() {
            nodes.push((
                index,
                Mat4::from_cols_array(&model.rest_world_matrix(index)),
            ));
        }
        stack.extend(found.children.iter().copied().filter(|&child| {
            model
                .nodes
                .get(child)
                .and_then(|child| child.name.as_deref())
                .and_then(ModelPart::from_name)
                .is_none()
        }));
    }
    nodes
}

/// A request placed on this frame's page.
struct Placed {
    pose: ModelPose,
    corner: [u32; 2],
    rect: [i32; 4],
}

/// The frame's page: the atlas page index and its render targets.
struct Page {
    index: usize,
    size: [u32; 2],
    color: wgpu::TextureView,
    indices: wgpu::TextureView,
    depth: wgpu::TextureView,
}

/// The models drawn this frame and the GPU state that draws them. Empty, and
/// never touched, while no type declares a model.
#[derive(Default)]
pub struct ModelFrame {
    page: Option<Page>,
    /// The side the next page takes; doubled after a frame overflowed.
    wanted: u32,
    shelf: Option<ShelfCursor>,
    placed: Vec<Placed>,
    keys: HashMap<PoseKey, ModelImage>,
    layouts: HashMap<usize, Layout>,
    gpu: Option<Gpu>,
    models: HashMap<usize, GpuModel>,
    overflowed: bool,
}

impl ModelFrame {
    /// Start a frame that may draw declared models: forget the last frame's
    /// images, and hold a page in `atlas` big enough for the frame before.
    pub(crate) fn begin_frame(
        &mut self,
        device: &wgpu::Device,
        batch: &BatchRenderer,
        atlas: &mut SpriteAtlas,
    ) {
        self.placed.clear();
        self.keys.clear();
        let limit = device.limits().max_texture_dimension_2d;
        let mut side = self.wanted.max(PAGE_START).min(limit);
        if std::mem::take(&mut self.overflowed) {
            side = side.saturating_mul(2).min(limit);
        }
        self.wanted = side;
        let current = self
            .page
            .as_ref()
            .filter(|page| atlas.model_page() == Some(page.index) && page.size == [side; 2]);
        if current.is_none() {
            let (texture, color, indices) = batch.create_render_target_page(device, side, side);
            let depth = device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("Model page depth"),
                    size: wgpu::Extent3d {
                        width: side,
                        height: side,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Depth32Float,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                })
                .create_view(&Default::default());
            self.page = atlas.install_model_page(texture).map(|index| Page {
                index,
                size: [side; 2],
                color: color.create_view(&Default::default()),
                indices: indices.create_view(&Default::default()),
                depth,
            });
        }
        self.shelf = self
            .page
            .as_ref()
            .map(|page| ShelfCursor::new(page.size[0], page.size[1]));
    }

    /// This frame's image of `pose`, placing it on the page the first time
    /// the frame asks for it. None when the frame has no page, the pose draws
    /// nothing, or the page is full (the next frame's page is bigger).
    pub(crate) fn request(&mut self, pose: ModelPose) -> Option<ModelImage> {
        let page = self.page.as_ref()?;
        let model = Arc::as_ptr(&pose.model) as usize;
        let key = PoseKey {
            model,
            matrices: pose
                .matrices
                .map(|matrix| matrix.to_cols_array().map(f32::to_bits)),
            gun: pose.gun,
            ramp: pose.ramp,
        };
        if let Some(image) = self.keys.get(&key) {
            return Some(*image);
        }
        let layout = self
            .layouts
            .entry(model)
            .or_insert_with(|| Layout::of(&pose.model));
        let mut minimum = [f32::INFINITY; 2];
        let mut maximum = [f32::NEG_INFINITY; 2];
        for (matrix, _, bounds) in layout.drawn(pose.gun) {
            let Some(bounds) = bounds else {
                continue;
            };
            let to_screen = pose.matrices[matrix] * layout.to_voxels;
            for corner in 0..8 {
                let point = Vec3::new(
                    if corner & 1 != 0 {
                        bounds.max[0]
                    } else {
                        bounds.min[0]
                    },
                    if corner & 2 != 0 {
                        bounds.max[1]
                    } else {
                        bounds.min[1]
                    },
                    if corner & 4 != 0 {
                        bounds.max[2]
                    } else {
                        bounds.min[2]
                    },
                );
                let projected = to_screen.transform_point3(point);
                // Screen y grows down (`native_vxl_draw_bounds`).
                for (axis, value) in [projected.x, -projected.y].into_iter().enumerate() {
                    minimum[axis] = minimum[axis].min(value);
                    maximum[axis] = maximum[axis].max(value);
                }
            }
        }
        let rect = crate::render::vxl_raster::native_draw_bounds_from_extents(minimum, maximum)?;
        let size = [rect[2], rect[3]].map(|side| u32::try_from(side).unwrap_or(0));
        if size.contains(&0) {
            return None;
        }
        let Some(corner) = self.shelf.as_mut()?.place(size[0], size[1]) else {
            if !self.overflowed {
                log::warn!(
                    "Model page {}x{} is full this frame; the next frame's is bigger",
                    page.size[0],
                    page.size[1]
                );
            }
            self.overflowed = true;
            return None;
        };
        let [width, height] = page.size.map(|side| side as f32);
        let image = ModelImage {
            page: page.index,
            uv_origin: [corner[0] as f32 / width, corner[1] as f32 / height],
            uv_size: [size[0] as f32 / width, size[1] as f32 / height],
            rect,
        };
        self.keys.insert(key, image);
        self.placed.push(Placed { pose, corner, rect });
        Some(image)
    }

    /// Draw this frame's images onto the page, before the world's draws
    /// sample it. A frame that asked for none draws nothing.
    pub(crate) fn encode(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        assets: Option<&AssetManager>,
        encoder: &mut wgpu::CommandEncoder,
    ) {
        if self.placed.is_empty() {
            return;
        }
        let Some(page) = self.page.as_ref() else {
            return;
        };
        let gpu = self.gpu.get_or_insert_with(|| Gpu::new(device, queue));
        for placed in &self.placed {
            let model = Arc::as_ptr(&placed.pose.model) as usize;
            self.models.entry(model).or_insert_with(|| {
                GpuModel::upload(device, queue, gpu, &placed.pose.model, assets)
            });
        }
        // One uniform block per primitive drawn.
        let mut blocks: Vec<u8> = Vec::new();
        let mut draws: Vec<(usize, usize, usize, [u32; 4])> = Vec::new();
        let light = crate::render::vxl_normals::world_light();
        for placed in &self.placed {
            let key = Arc::as_ptr(&placed.pose.model) as usize;
            let (Some(layout), Some(model)) = (self.layouts.get(&key), self.models.get(&key))
            else {
                continue;
            };
            let clip = page_clip(page.size, placed.corner, placed.rect);
            let scissor = [
                placed.corner[0],
                placed.corner[1],
                placed.rect[2] as u32,
                placed.rect[3] as u32,
            ];
            let ramp = placed
                .pose
                .ramp
                .map(|ramp| ramp.map(|color| color.map(srgb_to_linear)));
            for (matrix, nodes, _) in layout.drawn(placed.pose.gun) {
                for &(node, world) in nodes {
                    let to_camera = placed.pose.matrices[matrix] * layout.to_voxels * world;
                    let normal = Mat3::from_mat4(to_camera);
                    let normal = if normal.determinant().abs() > f32::EPSILON {
                        normal.inverse().transpose()
                    } else {
                        Mat3::IDENTITY
                    };
                    let Some(primitives) = placed
                        .pose
                        .model
                        .model
                        .nodes
                        .get(node)
                        .and_then(|node| node.mesh)
                        .and_then(|mesh| model.meshes.get(mesh))
                    else {
                        continue;
                    };
                    for &primitive in primitives {
                        let material = &model.materials[model.primitives[primitive].material];
                        let offset = blocks.len();
                        write_block(
                            &mut blocks,
                            clip * to_camera,
                            normal,
                            material,
                            light,
                            ramp.as_ref(),
                        );
                        blocks.resize(offset + gpu.stride as usize, 0);
                        draws.push((offset, key, primitive, scissor));
                    }
                }
            }
        }
        if draws.is_empty() {
            return;
        }
        gpu.reserve(device, blocks.len() as u64);
        queue.write_buffer(&gpu.uniforms, 0, &blocks);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Model page"),
            color_attachments: &[
                Some(wgpu::RenderPassColorAttachment {
                    view: &page.color,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                }),
                Some(wgpu::RenderPassColorAttachment {
                    view: &page.indices,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                }),
            ],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &page.depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&gpu.pipeline);
        for (offset, key, primitive, [x, y, width, height]) in draws {
            let Some(model) = self.models.get(&key) else {
                continue;
            };
            let primitive = &model.primitives[primitive];
            pass.set_scissor_rect(x, y, width, height);
            pass.set_bind_group(0, &gpu.draws, &[offset as u32]);
            pass.set_bind_group(1, &model.materials[primitive.material].bind_group, &[]);
            pass.set_vertex_buffer(0, primitive.vertices.slice(..));
            pass.set_index_buffer(primitive.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..primitive.count, 0, 0..1);
        }
    }
}

/// Voxel camera space to the page's clip space for an image whose rectangle
/// relative to its anchor is `rect`, placed at `corner` of a page of `size`:
/// screen x is the camera's x, screen y its -y (`native_vxl_draw_bounds`),
/// and depth falls as the camera's z, toward the viewer, rises.
fn page_clip(size: [u32; 2], corner: [u32; 2], rect: [i32; 4]) -> Mat4 {
    let [width, height] = size.map(|side| side as f32);
    let x = corner[0] as f32 - rect[0] as f32;
    let y = corner[1] as f32 - rect[1] as f32;
    Mat4::from_cols(
        Vec4::new(2.0 / width, 0.0, 0.0, 0.0),
        Vec4::new(0.0, 2.0 / height, 0.0, 0.0),
        Vec4::new(0.0, 0.0, -1.0 / DEPTH_SPAN, 0.0),
        Vec4::new(2.0 * x / width - 1.0, 1.0 - 2.0 * y / height, 0.5, 1.0),
    )
}

fn srgb_to_linear(byte: u8) -> f32 {
    let c = f32::from(byte) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// One `Draw` block (`model_draw.wgsl`).
fn write_block(
    out: &mut Vec<u8>,
    clip: Mat4,
    normal: Mat3,
    material: &GpuMaterial,
    light: Vec3,
    ramp: Option<&[[f32; 3]; 16]>,
) {
    let mut floats = |values: &[f32]| {
        for value in values {
            out.extend_from_slice(&value.to_le_bytes());
        }
    };
    floats(&clip.to_cols_array());
    for column in [normal.x_axis, normal.y_axis, normal.z_axis] {
        floats(&[column.x, column.y, column.z, 0.0]);
    }
    floats(&material.base_color);
    floats(&[
        material.emissive[0],
        material.emissive[1],
        material.emissive[2],
        0.0,
    ]);
    floats(&[light.x, light.y, light.z, material.alpha_cutoff]);
    let alpha_mode: u32 = match material.alpha_mode {
        AlphaMode::Opaque => 0,
        AlphaMode::Mask => 1,
        AlphaMode::Blend => 2,
    };
    for flag in [0, 0, alpha_mode, u32::from(ramp.is_some())] {
        out.extend_from_slice(&flag.to_le_bytes());
    }
    for step in 0..16 {
        let [r, g, b] = ramp.map_or([0.0; 3], |ramp| ramp[step]);
        for value in [r, g, b, 1.0_f32] {
            out.extend_from_slice(&value.to_le_bytes());
        }
    }
}

/// The pipeline and the buffers every model draw shares.
struct Gpu {
    pipeline: wgpu::RenderPipeline,
    draw_layout: wgpu::BindGroupLayout,
    material_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniforms: wgpu::Buffer,
    draws: wgpu::BindGroup,
    stride: u64,
    /// 1x1 textures standing in for a missing one: white base colour and
    /// emissive (times the material's factor), black mask.
    white: wgpu::TextureView,
    black: wgpu::TextureView,
}

impl Gpu {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Model draw"),
            source: wgpu::ShaderSource::Wgsl(include_str!("model_draw.wgsl").into()),
        });
        let draw_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Model draw block"),
            entries: &[layout_entry(
                0,
                wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(DRAW_SIZE),
                },
            )],
        });
        let float = wgpu::TextureSampleType::Float { filterable: true };
        let material_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Model material"),
            entries: &[
                texture_entry(0, float),
                texture_entry(1, float),
                texture_entry(2, float),
                layout_entry(
                    3,
                    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                ),
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Model draw"),
            bind_group_layouts: &[Some(&draw_layout), Some(&material_layout)],
            immediate_size: 0,
        });
        let attributes = wgpu::vertex_attr_array![
            0 => Float32x3,
            1 => Float32x3,
            2 => Float32x2,
            3 => Float32x4
        ];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Model draw"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: (VERTEX_FLOATS * 4) as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &attributes,
                })],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[
                    Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rgba8UnormSrgb,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                    Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::R8Uint,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                ],
                compilation_options: Default::default(),
            }),
            // The model scale and the page's flipped depth may turn a
            // triangle's winding, and glTF meshes may be open: no culling.
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Model textures"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let stride = DRAW_SIZE.next_multiple_of(u64::from(
            device.limits().min_uniform_buffer_offset_alignment,
        ));
        let (uniforms, draws) = draw_buffer(device, &draw_layout, stride);
        let pixel =
            |label, rgba: [u8; 4], format| rgba_texture(device, queue, label, 1, 1, &rgba, format);
        Self {
            pipeline,
            sampler,
            uniforms,
            draws,
            stride,
            white: pixel("Model white", [255; 4], wgpu::TextureFormat::Rgba8UnormSrgb),
            black: pixel(
                "Model black",
                [0, 0, 0, 255],
                wgpu::TextureFormat::Rgba8Unorm,
            ),
            draw_layout,
            material_layout,
        }
    }

    /// Grow the uniform buffer to hold `bytes`.
    fn reserve(&mut self, device: &wgpu::Device, bytes: u64) {
        if self.uniforms.size() < bytes {
            let blocks = bytes.next_power_of_two() / self.stride;
            let (uniforms, draws) =
                draw_buffer(device, &self.draw_layout, self.stride * blocks.max(1));
            self.uniforms = uniforms;
            self.draws = draws;
        }
    }
}

/// A uniform buffer of `bytes` and its bind group, one block bound at a time.
fn draw_buffer(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    bytes: u64,
) -> (wgpu::Buffer, wgpu::BindGroup) {
    let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Model draw blocks"),
        size: bytes.max(DRAW_SIZE),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let draws = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Model draw blocks"),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer: &uniforms,
                offset: 0,
                size: wgpu::BufferSize::new(DRAW_SIZE),
            }),
        }],
    });
    (uniforms, draws)
}

fn rgba_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    width: u32,
    height: u32,
    rgba: &[u8],
    format: wgpu::TextureFormat,
) -> wgpu::TextureView {
    device
        .create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            rgba,
        )
        .create_view(&Default::default())
}

/// One material's values and textures.
struct GpuMaterial {
    base_color: [f32; 4],
    emissive: [f32; 3],
    alpha_mode: AlphaMode,
    alpha_cutoff: f32,
    bind_group: wgpu::BindGroup,
}

struct GpuPrimitive {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    count: u32,
    /// An index into [`GpuModel::materials`]; the last is the default.
    material: usize,
}

/// A model's meshes and textures on the GPU, uploaded on its first draw.
struct GpuModel {
    primitives: Vec<GpuPrimitive>,
    /// Each mesh's primitives, as indices into `primitives`.
    meshes: Vec<Vec<usize>>,
    materials: Vec<GpuMaterial>,
}

impl GpuModel {
    fn upload(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        gpu: &Gpu,
        declared: &DeclaredModel,
        assets: Option<&AssetManager>,
    ) -> Self {
        let model = &declared.model;
        // Sidecar files beside the model replace its embedded images:
        // NAME.texture.png the base colour, NAME.mask.png the house-colour
        // mask (docs/picture-model-art.md). NAME.normals.png is not read yet.
        let stem = declared
            .decl
            .file
            .rsplit_once('.')
            .map_or(declared.decl.file.as_str(), |(stem, _)| stem);
        let sidecar = |suffix: &str, format| {
            let name = format!("{stem}.{suffix}");
            let bytes = assets?.get_ref(&name)?;
            match crate::assets::picture_file::PictureFrames::decode(bytes) {
                Ok(picture) => picture.frames.first().map(|frame| {
                    rgba_texture(
                        device,
                        queue,
                        &name,
                        picture.width,
                        picture.height,
                        frame,
                        format,
                    )
                }),
                Err(error) => {
                    log::warn!("[{}] {name} cannot be read ({error})", declared.section);
                    None
                }
            }
        };
        let base_sidecar = sidecar("texture.png", wgpu::TextureFormat::Rgba8UnormSrgb);
        let mask = sidecar("mask.png", wgpu::TextureFormat::Rgba8Unorm)
            .unwrap_or_else(|| gpu.black.clone());
        let images: Vec<wgpu::TextureView> = model
            .images
            .iter()
            .enumerate()
            .map(|(index, image)| {
                rgba_texture(
                    device,
                    queue,
                    &format!("{} image {index}", declared.decl.file),
                    image.width,
                    image.height,
                    &image.rgba,
                    wgpu::TextureFormat::Rgba8UnormSrgb,
                )
            })
            .collect();
        let image = |texture: Option<crate::assets::model_file::TextureRef>| {
            texture
                .and_then(|texture| images.get(texture.image))
                .cloned()
        };
        let material = |base: wgpu::TextureView,
                        emissive: wgpu::TextureView,
                        base_color: [f32; 4],
                        emissive_factor: [f32; 3],
                        alpha_mode: AlphaMode,
                        alpha_cutoff: f32| {
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Model material"),
                layout: &gpu.material_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&base),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&emissive),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&mask),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::Sampler(&gpu.sampler),
                    },
                ],
            });
            GpuMaterial {
                base_color,
                emissive: emissive_factor,
                alpha_mode,
                alpha_cutoff,
                bind_group,
            }
        };
        let mut materials: Vec<GpuMaterial> = model
            .materials
            .iter()
            .map(|source| {
                let base = base_sidecar
                    .clone()
                    .or_else(|| image(source.base_color_texture))
                    .unwrap_or_else(|| gpu.white.clone());
                let emissive = image(source.emissive_texture).unwrap_or_else(|| gpu.white.clone());
                material(
                    base,
                    emissive,
                    source.base_color_factor,
                    source.emissive_factor,
                    source.alpha_mode,
                    source.alpha_cutoff,
                )
            })
            .collect();
        // glTF's default material: white, opaque, not emissive.
        let default_material = materials.len();
        materials.push(material(
            base_sidecar.clone().unwrap_or_else(|| gpu.white.clone()),
            gpu.white.clone(),
            [1.0; 4],
            [0.0; 3],
            AlphaMode::Opaque,
            0.5,
        ));
        let mut primitives = Vec::new();
        let meshes = model
            .meshes
            .iter()
            .map(|mesh| {
                mesh.primitives
                    .iter()
                    .filter(|primitive| !primitive.indices.is_empty())
                    .map(|primitive| {
                        let vertices = vertex_data(primitive);
                        primitives.push(GpuPrimitive {
                            vertices: device.create_buffer_init(
                                &wgpu::util::BufferInitDescriptor {
                                    label: Some("Model vertices"),
                                    contents: &vertices,
                                    usage: wgpu::BufferUsages::VERTEX,
                                },
                            ),
                            indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                                label: Some("Model indices"),
                                contents: &primitive
                                    .indices
                                    .iter()
                                    .flat_map(|index| index.to_le_bytes())
                                    .collect::<Vec<u8>>(),
                                usage: wgpu::BufferUsages::INDEX,
                            }),
                            count: primitive.indices.len() as u32,
                            material: primitive
                                .material
                                .filter(|&material| material < default_material)
                                .unwrap_or(default_material),
                        });
                        primitives.len() - 1
                    })
                    .collect()
            })
            .collect();
        Self {
            primitives,
            meshes,
            materials,
        }
    }
}

/// A primitive's vertices: position, normal, UV and colour. Missing normals
/// are the area-weighted sum of the faces' at each vertex.
fn vertex_data(primitive: &crate::assets::model_file::ModelPrimitive) -> Vec<u8> {
    let positions = &primitive.positions;
    let normals = primitive.normals.clone().unwrap_or_else(|| {
        let mut normals = vec![[0.0_f32; 3]; positions.len()];
        for triangle in primitive.indices.chunks_exact(3) {
            let corner = |k: usize| Vec3::from(positions[triangle[k] as usize]);
            let face = (corner(1) - corner(0)).cross(corner(2) - corner(0));
            for &index in triangle {
                let normal = &mut normals[index as usize];
                *normal = (Vec3::from(*normal) + face).to_array();
            }
        }
        normals
    });
    let mut out = Vec::with_capacity(positions.len() * VERTEX_FLOATS * 4);
    for (index, position) in positions.iter().enumerate() {
        let uv = primitive
            .uvs
            .as_ref()
            .and_then(|uvs| uvs.get(index))
            .copied()
            .unwrap_or_default();
        let color = primitive
            .colors
            .as_ref()
            .and_then(|colors| colors.get(index))
            .copied()
            .unwrap_or([1.0; 4]);
        let normal = normals.get(index).copied().unwrap_or([0.0, 0.0, 1.0]);
        for value in position.iter().chain(&normal).chain(&uv).chain(&color) {
            out.extend_from_slice(&value.to_le_bytes());
        }
    }
    out
}

#[cfg(all(test, tests_render_a_r))]
mod tests {
    use super::*;

    /// The page clip puts an image's rectangle exactly over its slot: the
    /// rectangle's top-left anchor-relative corner lands on the slot's corner
    /// and its far corner on the slot's, in clip space.
    #[test]
    fn page_clip_maps_the_rectangle_onto_its_slot() {
        let clip = page_clip([100, 50], [10, 20], [-5, -8, 30, 16]);
        // Screen (-5, -8) is camera (-5, 8).
        let near = clip * Vec4::new(-5.0, 8.0, 0.0, 1.0);
        assert!((near.x - -0.8).abs() < 1e-6 && (near.y - 0.2).abs() < 1e-6);
        assert_eq!(near.z, 0.5);
        // Screen (25, 8) is camera (25, -8): pixel (40, 36).
        let far = clip * Vec4::new(25.0, -8.0, 0.0, 1.0);
        assert!((far.x - -0.2).abs() < 1e-6 && (far.y - -0.44).abs() < 1e-6);
        // Nearer the viewer is lower depth.
        assert!((clip * Vec4::new(0.0, 0.0, 10.0, 1.0)).z < 0.5);
    }

    /// The model's body length along glTF +Z is `ModelSize=` leptons in voxel
    /// units along the hull's X, and glTF +Y is up.
    #[test]
    fn a_model_is_scaled_and_turned_into_voxel_space() {
        let glb = crate::assets::model_file::test_glb::box_model(2.0, 1.0, 4.0, &["turret"]);
        let model = Arc::new(Model::from_glb(&glb).unwrap());
        let declared = DeclaredModel::for_test(model, 256);
        let layout = Layout::of(&declared);
        let units = f64::from_bits(crate::map::voxel_pose::MODEL_UNITS_PER_LEPTON_BITS) as f32;
        let front = layout.to_voxels.transform_point3(Vec3::new(0.0, 0.0, 2.0));
        assert!((front.x - 128.0 * units).abs() < 1e-3, "{front}");
        let top = layout.to_voxels.transform_point3(Vec3::new(0.0, 0.5, 0.0));
        assert!(top.z > 0.0 && top.x == 0.0 && top.y == 0.0);
        assert_eq!(layout.drawn(None).count(), 1, "the body alone");
        assert_eq!(layout.drawn(Some(0)).count(), 2, "and turret0; no barrel");
    }
}
