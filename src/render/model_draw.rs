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
//! Node animations stand in for HVA frames: each part is posed at the key
//! of its clip ([`Model::part_clip`]) its HVA frame number selects
//! ([`crate::assets::model_file::ModelAnimation::frame_time`]), and the frame
//! numbers are part of the pose's key. A normal map (`NAME.normals.png`, else
//! the material's normal texture) bends the lit normal in a tangent frame
//! taken from the screen derivatives of position and UV.
//!
//! A model's shadow is a mask on the same page ([`ModelFrame::request_shadow`]):
//! the `shadow` node's meshes, else the body's (a debris piece: the nodes
//! it draws), flattened onto their bottom face and projected as
//! `0x00753F90` projects a VXL section's bottom-face quad (+3 pixels along
//! x, the viewer halved for `ConsideredAircraft=`), with the index plane
//! nonzero where shadowed and the colour plane unused. The piece hands it
//! to the SHP shadow edit, so darkening follows the native rule.
//!
//! Light ([`ModelLight`]) is the voxel world light, as a unit's VXL takes it
//! (Blinn-Phong, `0x00753D00`) or as a projectile's and a debris piece's
//! take it (diffuse only, `0x00753C80`).
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
//! [`MODEL_UNITS_PER_LEPTON_BITS`]: crate::util::lepton::MODEL_UNITS_PER_LEPTON_BITS

use std::collections::HashMap;
use std::sync::Arc;

use glam::{Mat3, Mat4, Vec3, Vec4};
use wgpu::util::DeviceExt;

use crate::assets::asset_manager::AssetManager;
use crate::assets::model_file::{AlphaMode, Model, ModelBounds, ModelPart, TextureRef};
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

/// The shader's `Draw` block (`model_draw.wgsl`); `model_shadow.wgsl` reads
/// its first matrix.
const DRAW_SIZE: u64 = 640;

/// The light translation `0x00753F90` adds to a shadow quad's x after the
/// viewer (`0x00754C00` stores `0x403FFFF1` at `0x00887420`, just under 3),
/// as `vxl_shadow::projected_quad` takes it.
const SHADOW_LIGHT_X_BITS: u32 = 0x403f_fff1;

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
    /// The body's and the turret's HVA frame numbers
    /// (`instances::units::animation_frames_with`); the barrel takes the
    /// body's, as its HVA does. A building's guns take the turret's and the
    /// barrel's ([`ModelNodes::BuildingGuns`]). Unused by a model without
    /// animations.
    pub frames: [u32; 2],
    pub light: ModelLight,
}

/// VERA-only, experimental (`docs/picture-model-art.md`): how a pose is
/// lit, as the VXL it stands in for is lit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ModelLight {
    /// A Techno's voxel draw (`TechnoClass::Render 0x00706ED0` ->
    /// `0x00753D00`, [`crate::render::vxl_normals::blinn_phong_pages`]):
    /// diffuse plus the Schlick term of the halfway vector, strength 3.
    Unit,
    /// A projectile's (Bullet draw `0x0046B0C0`) and a debris piece's
    /// (VoxelAnim DrawIt `0x00749D97`) voxel draw through `0x00753C80`
    /// ([`crate::render::vxl_normals::diffuse_pages`]): the diffuse term
    /// alone, nothing where it is negative.
    Diffuse,
}

/// One object's model shadow, drawn at HVA frame zero as `0x00707280`
/// samples it.
pub(crate) struct ModelShadow {
    pub model: Arc<DeclaredModel>,
    /// The shadow's draw matrix in the voxel camera's space: its facing and
    /// slope, no locomotor arm (`vxl_raster::body_draw_matrix` of the shadow
    /// key's facing and slope, as `vxl_shadow::draw_matrix` takes it).
    pub matrix: Mat4,
    /// `ConsideredAircraft=`: the viewer is halved (`0x00707319`).
    pub considered_aircraft: bool,
    /// The nodes the shadow is cast from ([`Layout::shadow_of`]): a unit's
    /// `shadow`, else `body`; a debris piece's drawn nodes.
    pub nodes: ModelNodes,
}

/// VERA-only, experimental (`docs/picture-model-art.md`): which nodes a
/// pose draws, and at which of its three matrices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ModelNodes {
    /// A voxel unit: `body` at matrix 0 and, with the pose's gun, its
    /// `turretN` at 1 and `barrelN` at 2. A projectile draws the body alone.
    Unit,
    /// A building's voxel guns (Building draw `43DA80`): `body` as the B8
    /// turret at matrix 1 when the building has one, and `barrel` as the C0
    /// barrel at matrix 2.
    BuildingGuns { turret: bool },
    /// The mesh node at this position among the model's nodes with a mesh,
    /// in file order, at matrix 0: a debris piece's `VoxelIndex=`, as
    /// VoxelAnim draw `749DCE` draws one VXL section. Its own meshes only.
    MeshNode(usize),
    /// One part at matrix 0 in its rest pose: a `ShareSource=` debris
    /// piece's shared body, turret or barrel (VoxelAnim `74B4D0`), drawn
    /// without an HVA as `749DCE` draws the shared VXL.
    Part(ModelPart),
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
enum PoseKey {
    Body {
        model: usize,
        matrices: [[u32; 16]; 3],
        gun: Option<u32>,
        ramp: Option<[[u8; 3]; 16]>,
        frames: [u32; 2],
        nodes: ModelNodes,
        light: ModelLight,
    },
    Shadow {
        model: usize,
        matrix: [u32; 16],
        considered_aircraft: bool,
        nodes: ModelNodes,
    },
}

/// One mesh node of a part.
struct PartNode {
    node: usize,
    /// The rest-pose model matrix.
    rest: Mat4,
    /// The mesh's bounds in the node's own space.
    local: Option<ModelBounds>,
}

/// One part's nodes, rest bounds and clip, or one mesh node's
/// ([`ModelNodes::MeshNode`]).
struct PartLayout {
    nodes: Vec<PartNode>,
    /// The rest bounds in model space ([`Model::part_bounds`]).
    bounds: Option<ModelBounds>,
    /// The animation the part's HVA frames select ([`Model::part_clip`]).
    clip: Option<usize>,
}

impl PartLayout {
    /// Each mesh node with its model matrix at HVA frame `frame`: at the key
    /// the frame selects in the part's clip, else at rest. None draws the
    /// rest pose whatever the clip, as a VXL drawn without its HVA.
    fn posed<'a>(
        &'a self,
        model: &'a Model,
        frame: Option<u32>,
    ) -> impl Iterator<Item = (&'a PartNode, Mat4)> {
        let pose = self.clip.zip(frame).and_then(|(clip, frame)| {
            let animation = model.animations.get(clip)?;
            Some((clip, animation.frame_time(frame)))
        });
        self.nodes.iter().map(move |node| {
            let world = match pose {
                Some(pose) => Mat4::from_cols_array(&model.world_matrix_at(node.node, Some(pose))),
                None => node.rest,
            };
            (node, world)
        })
    }
}

/// The CPU half of a model: its parts' nodes and bounds.
struct Layout {
    /// glTF model space to voxel space.
    to_voxels: Mat4,
    /// Each part's mesh nodes with their rest-pose model matrices, and the
    /// part's rest bounds.
    parts: Vec<(ModelPart, Vec<(usize, Mat4)>, Option<ModelBounds>)>,
    /// Every node with a mesh, in file order, alone with its rest-pose model
    /// matrix, and the rest bounds of its own meshes ([`ModelNodes::MeshNode`]).
    mesh_nodes: Vec<(Vec<(usize, Mat4)>, Option<ModelBounds>)>,
}

impl Layout {
    fn of(declared: &DeclaredModel) -> Self {
        let model = &declared.model;
        let length = model
            .body_bounds()
            .map_or(1.0, |bounds| bounds.max[2] - bounds.min[2]);
        let units_per_lepton =
            f64::from_bits(crate::util::lepton::MODEL_UNITS_PER_LEPTON_BITS) as f32;
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
                (model.part(part) == Some(index)).then(|| PartLayout {
                    part,
                    nodes: part_nodes(model, index),
                    bounds: model.part_bounds(index),
                    clip: model.part_clip(part),
                })
            })
            .collect();
        let mesh_nodes = model
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| node.mesh.is_some())
            .map(|(index, _)| {
                let world = Mat4::from_cols_array(&model.rest_world_matrix(index));
                (vec![(index, world)], node_mesh_bounds(model, index, world))
            })
            .collect();
        Self {
            to_voxels,
            parts,
            mesh_nodes,
        }
    }

    fn part(&self, part: ModelPart) -> Option<&PartLayout> {
        self.parts.iter().find(|found| found.part == part)
    }

    /// The parts `nodes` and `gun` draw with, with the draw matrix index and
    /// the frame index ([`ModelPose::frames`]) of each; None draws it at rest.
    fn drawn(
        &self,
        nodes: ModelNodes,
        gun: Option<u32>,
    ) -> impl Iterator<Item = (usize, &PartLayout, Option<usize>)> {
        let parts = match nodes {
            ModelNodes::Unit => [
                Some((0, ModelPart::Body, Some(0))),
                gun.map(|gun| (1, ModelPart::Turret(gun), Some(1))),
                gun.map(|gun| (2, ModelPart::Barrel(gun), Some(0))),
            ],
            // Building draw 43DA80's B8 at the +0x148 counter's frame and C0
            // at its own (frame zero beside a B8, 43E2FA).
            ModelNodes::BuildingGuns { turret } => [
                turret.then_some((1, ModelPart::Body, Some(0))),
                Some((2, ModelPart::Barrel(0), Some(1))),
                None,
            ],
            ModelNodes::Part(part) => [Some((0, part, None)), None, None],
            ModelNodes::MeshNode(_) => [None; 3],
        };
        let mesh_node = match nodes {
            ModelNodes::MeshNode(index) => self.mesh_nodes.get(index).map(|node| (0, node, None)),
            _ => None,
        };
        parts
            .into_iter()
            .flatten()
            .filter_map(|(matrix, part)| {
                let (_, nodes, bounds) = self.part(part)?;
                Some((matrix, nodes, *bounds))
            })
            .chain(mesh_node)
    }

    /// The nodes a shadow of `nodes` is cast from, and the frame it is cast
    /// at: a unit's `shadow`, else `body`, at frame 0 (`0x00707280` samples
    /// HVA frame 0); a debris piece's drawn node or part at rest, as
    /// `vxl_shadow::render_matrix` takes its section's bottom face with no
    /// HVA (VoxelAnim DrawIt `0x00749CBE`). A building's guns cast none.
    fn shadow_of(&self, nodes: ModelNodes) -> Option<(&PartLayout, Option<u32>)> {
        match nodes {
            ModelNodes::Unit => self
                .part(ModelPart::Shadow)
                .or_else(|| self.part(ModelPart::Body))
                .map(|part| (part, Some(0))),
            ModelNodes::MeshNode(index) => self.mesh_nodes.get(index).map(|node| (node, None)),
            ModelNodes::Part(part) => self.part(part).map(|part| (part, None)),
            ModelNodes::BuildingGuns { .. } => None,
        }
    }
}

/// The screen extents of boxes: x right, y down (`native_vxl_draw_bounds`).
struct Extents {
    min: [f32; 2],
    max: [f32; 2],
}

impl Extents {
    fn new() -> Self {
        Self {
            min: [f32::INFINITY; 2],
            max: [f32::NEG_INFINITY; 2],
        }
    }

    /// Take the corners of `bounds` through `to_camera`, voxel camera space
    /// whose -y is screen y.
    fn add(&mut self, to_camera: Mat4, bounds: ModelBounds) {
        for corner in box_corners(bounds) {
            let projected = to_camera.transform_point3(corner);
            for (axis, value) in [projected.x, -projected.y].into_iter().enumerate() {
                self.min[axis] = self.min[axis].min(value);
                self.max[axis] = self.max[axis].max(value);
            }
        }
    }

    /// The rectangle a voxel draw of these extents takes, relative to the
    /// anchor (`native_draw_bounds_from_extents`, `0x00754510`'s rule, which
    /// `vxl_shadow::prepare_matrix` shares for a shadow's crop).
    fn rect(&self) -> Option<[i32; 4]> {
        crate::render::vxl_raster::native_draw_bounds_from_extents(self.min, self.max)
    }
}

fn box_corners(bounds: ModelBounds) -> [Vec3; 8] {
    std::array::from_fn(|corner| {
        let pick = |axis: usize| {
            if corner & (1 << axis) != 0 {
                bounds.max[axis]
            } else {
                bounds.min[axis]
            }
        };
        Vec3::new(pick(0), pick(1), pick(2))
    })
}

/// Voxel model space to the shadow's anchor-relative camera space, as
/// `0x00753F90` projects a section's bottom-face quad: every point is moved
/// down onto the bottom face (voxel z `bottom`), taken through the shadow's
/// draw `matrix`, its camera z dropped, its x and y halved for
/// `ConsideredAircraft=` (`0x00707319`) and then its x moved by the light
/// (`SHADOW_LIGHT_X_BITS`), which is not halved. Screen y is camera -y.
fn shadow_camera(matrix: Mat4, bottom: f32, considered_aircraft: bool) -> Mat4 {
    let flatten = Mat4::from_cols(
        Vec4::X,
        Vec4::Y,
        Vec4::ZERO,
        Vec4::new(0.0, 0.0, bottom, 1.0),
    );
    let viewer = if considered_aircraft { 0.5 } else { 1.0 };
    let light = f32::from_bits(SHADOW_LIGHT_X_BITS);
    let screen = Mat4::from_cols(
        Vec4::new(viewer, 0.0, 0.0, 0.0),
        Vec4::new(0.0, viewer, 0.0, 0.0),
        Vec4::ZERO,
        Vec4::new(light, 0.0, 0.0, 1.0),
    );
    screen * matrix * flatten
}

/// The nodes with meshes under `node`, itself included, leaving out other
/// parts' subtrees as [`Model::part_bounds`] does, with their rest-pose
/// model matrices and their meshes' own bounds.
fn part_nodes(model: &Model, node: usize) -> Vec<PartNode> {
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
        if let Some(mesh) = found.mesh {
            nodes.push(PartNode {
                node: index,
                rest: Mat4::from_cols_array(&model.rest_world_matrix(index)),
                local: model.mesh_bounds(mesh),
            });
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

/// The rest-pose bounds, in model space, of `node`'s own meshes, whose
/// rest-pose model matrix is `world`; its children are left out.
fn node_mesh_bounds(model: &Model, node: usize, world: Mat4) -> Option<ModelBounds> {
    let mesh = model.meshes.get(model.nodes.get(node)?.mesh?)?;
    let mut bounds: Option<ModelBounds> = None;
    for position in mesh
        .primitives
        .iter()
        .flat_map(|primitive| &primitive.positions)
    {
        let point = world
            .transform_point3(Vec3::from_array(*position))
            .to_array();
        let next = bounds.get_or_insert(ModelBounds {
            min: point,
            max: point,
        });
        for axis in 0..3 {
            next.min[axis] = next.min[axis].min(point[axis]);
            next.max[axis] = next.max[axis].max(point[axis]);
        }
    }
    bounds
}

/// A request placed on this frame's page.
struct Placed {
    pose: ModelPose,
    nodes: ModelNodes,
    corner: [u32; 2],
    rect: [i32; 4],
    /// Each mesh node drawn, with its model-to-camera matrix (anchor-relative
    /// voxel camera space; a shadow's flattened, [`shadow_camera`]) and the
    /// voxel light's viewer there: its part's voxel draw +Z.
    draws: Vec<(usize, Mat4, Vec3)>,
    /// The owner's ramp, for a body.
    ramp: Option<[[u8; 3]; 16]>,
    /// A shadow mask, drawn by the shadow pipeline.
    shadow: bool,
    light: ModelLight,
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
    /// The VPL's brightness per light page ([`vpl_page_brightness`]), read
    /// on the first frame that draws a model.
    vpl_curve: Option<[f32; 32]>,
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
        self.request_nodes(pose, ModelNodes::Unit)
    }

    /// [`Self::request`] for a pose that draws `nodes`: a building's guns,
    /// a projectile or a debris piece (`docs/picture-model-art.md`).
    pub(crate) fn request_nodes(
        &mut self,
        pose: ModelPose,
        nodes: ModelNodes,
    ) -> Option<ModelImage> {
        let page = self.page.as_ref()?;
        let model = Arc::as_ptr(&pose.model) as usize;
        let animated = !pose.model.model.animations.is_empty();
        let key = PoseKey::Body {
            model,
            matrices: pose
                .matrices
                .map(|matrix| matrix.to_cols_array().map(f32::to_bits)),
            gun: pose.gun,
            ramp: pose.ramp,
            nodes,
            light: pose.light,
        };
        if let Some(image) = self.keys.get(&key) {
            return Some(*image);
        }
        let layout = self
            .layouts
            .entry(model)
            .or_insert_with(|| Layout::of(&pose.model));
        let mut extents = Extents::new();
        let mut draws = Vec::new();
        for (matrix, part, frame) in layout.drawn(nodes, pose.gun) {
            let to_camera = pose.matrices[matrix] * layout.to_voxels;
            // The voxel light lights in the draw's model space with the
            // viewer at its +Z (`vxl_normals::blinn_phong_pages`); in the
            // camera's space that viewer is the draw matrix's +Z.
            let viewer = pose.matrices[matrix]
                .transform_vector3(Vec3::Z)
                .normalize_or(Vec3::Z);
            let frame = frame.map(|frame| pose.frames[frame]);
            let posed = part.clip.is_some() && frame.is_some();
            // A part at rest keeps its tight rest bounds; a posed part takes
            // its nodes' boxes, as a voxel draw takes its sections' boxes
            // at their HVA frame.
            if !posed && let Some(bounds) = part.bounds {
                extents.add(to_camera, bounds);
            }
            for (node, world) in part.posed(&pose.model.model, frame) {
                if posed && let Some(local) = node.local {
                    extents.add(to_camera * world, local);
                }
                draws.push((node.node, to_camera * world, viewer));
            }
        }
        let rect = extents.rect()?;
        self.place(
            key,
            Placed {
                model: pose.model,
                corner: [0; 2],
                rect,
                draws,
                ramp: pose.ramp,
                shadow: false,
                light: pose.light,
            },
        )
    }

    /// This frame's shadow mask of `shadow`, placed as [`Self::request`]
    /// places an image: the nodes [`Layout::shadow_of`] picks, flattened
    /// onto their bottom face ([`shadow_camera`]).
    pub(crate) fn request_shadow(&mut self, shadow: ModelShadow) -> Option<ModelImage> {
        self.page.as_ref()?;
        let model = Arc::as_ptr(&shadow.model) as usize;
        let key = PoseKey::Shadow {
            model,
            matrix: shadow.matrix.to_cols_array().map(f32::to_bits),
            considered_aircraft: shadow.considered_aircraft,
            nodes: shadow.nodes,
        };
        if let Some(image) = self.keys.get(&key) {
            return Some(*image);
        }
        let layout = self
            .layouts
            .entry(model)
            .or_insert_with(|| Layout::of(&shadow.model));
        let (part, frame) = layout.shadow_of(shadow.nodes)?;
        let nodes: Vec<(usize, Mat4, Option<ModelBounds>)> = part
            .posed(&shadow.model.model, frame)
            .map(|(node, world)| (node.node, layout.to_voxels * world, node.local))
            .collect();
        // The bottom face: the lowest voxel z of the nodes' boxes.
        let bottom = nodes
            .iter()
            .filter_map(|(_, to_voxels, local)| Some((to_voxels, (*local)?)))
            .flat_map(|(to_voxels, local)| {
                box_corners(local).map(|corner| to_voxels.transform_point3(corner).z)
            })
            .fold(f32::INFINITY, f32::min);
        if !bottom.is_finite() {
            return None;
        }
        let camera = shadow_camera(shadow.matrix, bottom, shadow.considered_aircraft);
        let mut extents = Extents::new();
        let mut draws = Vec::with_capacity(nodes.len());
        for (node, to_voxels, local) in nodes {
            if let Some(local) = local {
                extents.add(camera * to_voxels, local);
            }
            draws.push((node, camera * to_voxels, Vec3::Z));
        }
        let rect = extents.rect()?;
        self.place(
            key,
            Placed {
                model: shadow.model,
                corner: [0; 2],
                rect,
                draws,
                ramp: None,
                shadow: true,
                light: ModelLight::Unit,
            },
        )
    }

    /// Place `placed`, whose rectangle is set, on this frame's page under
    /// `key`.
    fn place(&mut self, key: PoseKey, mut placed: Placed) -> Option<ModelImage> {
        let page = self.page.as_ref()?;
        let rect = placed.rect;
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
        self.placed.push(Placed {
            pose,
            nodes,
            corner,
            rect,
        });
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
            let model = Arc::as_ptr(&placed.model) as usize;
            self.models
                .entry(model)
                .or_insert_with(|| GpuModel::upload(device, queue, gpu, &placed.model, assets));
        }
        // One uniform block per primitive drawn.
        let mut blocks: Vec<u8> = Vec::new();
        let mut draws: Vec<(usize, usize, usize, [u32; 4], bool)> = Vec::new();
        let light = crate::render::vxl_normals::world_light();
        let vpl_curve = *self
            .vpl_curve
            .get_or_insert_with(|| vpl_page_brightness(assets));
        for placed in &self.placed {
            let key = Arc::as_ptr(&placed.model) as usize;
            let Some(model) = self.models.get(&key) else {
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
                .ramp
                .map(|ramp| ramp.map(|color| color.map(srgb_to_linear)));
            for &(node, to_camera, viewer) in &placed.draws {
                let Some(primitives) = placed
                    .model
                    .model
                    .nodes
                    .get(node)
                    .and_then(|node| node.mesh)
                    .and_then(|mesh| model.meshes.get(mesh))
                else {
                    continue;
                };
                let normal = Mat3::from_mat4(to_camera);
                let normal = if normal.determinant().abs() > f32::EPSILON {
                    normal.inverse().transpose()
                } else {
                    Mat3::IDENTITY
                };
                for &primitive in primitives {
                    let offset = blocks.len();
                    if placed.shadow {
                        write_shadow_block(&mut blocks, clip * to_camera);
                    } else {
                        let material = &model.materials[model.primitives[primitive].material];
                        write_block(
                            &mut blocks,
                            clip * to_camera,
                            to_camera,
                            normal,
                            material,
                            (light, viewer, placed.light),
                            ramp.as_ref(),
                            &vpl_curve,
                        );
                    }
                    blocks.resize(offset + gpu.stride as usize, 0);
                    draws.push((offset, key, primitive, scissor, placed.shadow));
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
        let mut bound: Option<bool> = None;
        for (offset, key, primitive, [x, y, width, height], shadow) in draws {
            let Some(model) = self.models.get(&key) else {
                continue;
            };
            if bound != Some(shadow) {
                pass.set_pipeline(if shadow {
                    &gpu.shadow_pipeline
                } else {
                    &gpu.pipeline
                });
                bound = Some(shadow);
            }
            let primitive = &model.primitives[primitive];
            pass.set_scissor_rect(x, y, width, height);
            pass.set_bind_group(0, &gpu.draws, &[offset as u32]);
            if !shadow {
                pass.set_bind_group(1, &model.materials[primitive.material].bind_group, &[]);
            }
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

/// The brightness `VOXELS.VPL` gives each light page 0..31 of the voxel
/// light: the mean lightness of the unit palette's ordinary colours (32 to
/// 239) through that page, over their own. A VPL page relights palette bytes,
/// so a model's colour takes the same factor on its encoded bytes
/// (`model_draw.wgsl`). Retail's own curve when the files are not at hand.
fn vpl_page_brightness(assets: Option<&AssetManager>) -> [f32; 32] {
    let read = || -> Option<[f32; 32]> {
        let assets = assets?;
        let vpl =
            crate::assets::vpl_file::VplFile::from_bytes(assets.get_ref("VOXELS.VPL")?).ok()?;
        let palette =
            crate::assets::pal_file::Palette::from_bytes(assets.get_ref("unittem.pal")?).ok()?;
        let lightness = |index: u8| {
            let c = palette.color(index);
            0.299 * f32::from(c.r) + 0.587 * f32::from(c.g) + 0.114 * f32::from(c.b)
        };
        let own: f32 = (32..240u8).map(lightness).sum();
        let pages = vpl.pages_slice().len();
        if own <= 0.0 || pages == 0 {
            return None;
        }
        Some(std::array::from_fn(|page| {
            let page = page.min(pages - 1) as u8;
            (32..240u8)
                .map(|index| lightness(vpl.get_palette_index(page, index)))
                .sum::<f32>()
                / own
        }))
    };
    read().unwrap_or(RETAIL_VPL_PAGE_BRIGHTNESS)
}

/// [`vpl_page_brightness`] of retail `VOXELS.VPL` and `UNITTEM.PAL`.
const RETAIL_VPL_PAGE_BRIGHTNESS: [f32; 32] = [
    0.529, 0.574, 0.618, 0.658, 0.701, 0.872, 0.912, 0.966, 1.000, 1.031, 1.073, 1.114, 1.150,
    1.180, 1.215, 1.243, 1.269, 1.335, 1.388, 1.438, 1.486, 1.524, 1.561, 1.590, 1.624, 1.650,
    1.672, 1.692, 1.707, 1.724, 1.742, 1.761,
];

/// One `Draw` block (`model_draw.wgsl`).
#[allow(clippy::too_many_arguments)]
fn write_block(
    out: &mut Vec<u8>,
    clip: Mat4,
    to_camera: Mat4,
    normal: Mat3,
    material: &GpuMaterial,
    (light, viewer, mode): (Vec3, Vec3, ModelLight),
    ramp: Option<&[[f32; 3]; 16]>,
    vpl_curve: &[f32; 32],
) {
    let mut floats = |values: &[f32]| {
        for value in values {
            out.extend_from_slice(&value.to_le_bytes());
        }
    };
    floats(&clip.to_cols_array());
    floats(&to_camera.to_cols_array());
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
    for flag in [
        u32::from(material.normal_map),
        u32::from(mode == ModelLight::Diffuse),
        alpha_mode,
        u32::from(ramp.is_some()),
    ] {
        out.extend_from_slice(&flag.to_le_bytes());
    }
    for step in 0..16 {
        let [r, g, b] = ramp.map_or([0.0; 3], |ramp| ramp[step]);
        for value in [r, g, b, 1.0_f32] {
            out.extend_from_slice(&value.to_le_bytes());
        }
    }
    for value in [viewer.x, viewer.y, viewer.z, 0.0] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    for value in vpl_curve {
        out.extend_from_slice(&value.to_le_bytes());
    }
}

/// A shadow's block: the clip matrix `model_shadow.wgsl` reads, the rest of
/// the block left zero.
fn write_shadow_block(out: &mut Vec<u8>, clip: Mat4) {
    for value in clip.to_cols_array() {
        out.extend_from_slice(&value.to_le_bytes());
    }
}

/// The pipeline and the buffers every model draw shares.
struct Gpu {
    pipeline: wgpu::RenderPipeline,
    /// Draws shadow masks: index 1 wherever a flattened triangle covers.
    shadow_pipeline: wgpu::RenderPipeline,
    draw_layout: wgpu::BindGroupLayout,
    material_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniforms: wgpu::Buffer,
    draws: wgpu::BindGroup,
    stride: u64,
    /// 1x1 textures standing in for a missing one: white base colour and
    /// emissive (times the material's factor), black mask, flat normal.
    white: wgpu::TextureView,
    black: wgpu::TextureView,
    flat: wgpu::TextureView,
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
                texture_entry(4, float),
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
        let shadow_pipeline = shadow_pipeline(device, &draw_layout);
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
            shadow_pipeline,
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
            flat: pixel(
                "Model flat normal",
                [128, 128, 255, 255],
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

/// The shadow mask pipeline (`model_shadow.wgsl`): the model page's targets
/// and depth format, no depth test or write, no culling (a flattened mesh
/// shows both faces), only the vertex position read.
fn shadow_pipeline(
    device: &wgpu::Device,
    draw_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Model shadow"),
        source: wgpu::ShaderSource::Wgsl(include_str!("model_shadow.wgsl").into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Model shadow"),
        bind_group_layouts: &[Some(draw_layout)],
        immediate_size: 0,
    });
    let attributes = wgpu::vertex_attr_array![0 => Float32x3];
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Model shadow"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_shadow"),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: (VERTEX_FLOATS * 4) as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &attributes,
            })],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_shadow"),
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
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::Always),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    })
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
    /// A normal map is bound (the shader bends the normal by it).
    normal_map: bool,
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
        // NAME.texture.png the base colour, NAME.normals.png the normal map,
        // NAME.mask.png the house-colour mask (docs/picture-model-art.md).
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
        // Normal maps hold vectors, not colours: read linear.
        let normals_sidecar = sidecar("normals.png", wgpu::TextureFormat::Rgba8Unorm);
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
        let image = |texture: Option<TextureRef>| {
            texture
                .and_then(|texture| images.get(texture.image))
                .cloned()
        };
        // The images materials name as normal textures, again in a linear
        // format, each once.
        let mut linear: HashMap<usize, wgpu::TextureView> = HashMap::new();
        if normals_sidecar.is_none() {
            for texture in model.materials.iter().filter_map(|m| m.normal_texture) {
                let Some(source) = model.images.get(texture.image) else {
                    continue;
                };
                linear.entry(texture.image).or_insert_with(|| {
                    rgba_texture(
                        device,
                        queue,
                        &format!("{} normals {}", declared.decl.file, texture.image),
                        source.width,
                        source.height,
                        &source.rgba,
                        wgpu::TextureFormat::Rgba8Unorm,
                    )
                });
            }
        }
        let normals = |texture: Option<TextureRef>| {
            normals_sidecar.clone().or_else(|| {
                texture
                    .and_then(|texture| linear.get(&texture.image))
                    .cloned()
            })
        };
        let material = |base: wgpu::TextureView,
                        emissive: wgpu::TextureView,
                        normal: Option<wgpu::TextureView>,
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
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::TextureView(
                            normal.as_ref().unwrap_or(&gpu.flat),
                        ),
                    },
                ],
            });
            GpuMaterial {
                base_color,
                emissive: emissive_factor,
                normal_map: normal.is_some(),
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
                    normals(source.normal_texture),
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
            normals(None),
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

    /// A model lights at the voxel light's page. For a level ground unit at
    /// each facing, the page `model_draw.wgsl` computes from camera-space
    /// vectors (16 times the diffuse term plus the Schlick term to the draw's
    /// +Z, [`ModelFrame::request_nodes`]'s viewer) for each table normal
    /// carried into the camera's space is the page `blinn_phong_pages`
    /// truncates for it in model space.
    #[test]
    fn a_model_lights_at_the_voxel_light_page() {
        use crate::render::vxl_raster::{GroundPose, ground_unit_pose_matrices};
        let light = crate::render::vxl_normals::world_light();
        for facing in (0..=255u8).step_by(8) {
            let [body, ..] = ground_unit_pose_matrices(
                0,
                GroundPose {
                    angles: [0.0; 2],
                    tunnel_tilt: None,
                    body_facing: facing,
                    slope_blend: None,
                    turret_offset: 0,
                },
                [10.0, 10.0],
                facing,
                0,
            );
            let rotation = Mat3::from_mat4(body);
            let pages = crate::render::vxl_normals::blinn_phong_pages(4, rotation);
            let viewer = body.transform_vector3(Vec3::Z).normalize();
            let half = (light + viewer).normalize();
            for index in 0..=240u8 {
                let n = (rotation * crate::render::vxl_normals::get_normal(4, index)).normalize();
                let h = half.dot(n);
                let page = 16.0 * (n.dot(light).max(0.0) + (h / (3.0 - 2.0 * h)).max(0.0));
                let native = f32::from(pages[usize::from(index)]);
                assert!(
                    page >= native - 0.01 && page < native + 1.01,
                    "facing {facing}, normal {index}: shader page {page}, voxel page {native}"
                );
            }
        }
    }

    /// The brightness curve is read from the VPL: a VPL whose pages leave
    /// every colour as it is gives 1 everywhere, and without files retail's
    /// curve keeps page 8 at about the colour itself.
    #[test]
    fn the_vpl_curve_keeps_page_eight_at_the_colour() {
        assert!((RETAIL_VPL_PAGE_BRIGHTNESS[8] - 1.0).abs() < 0.01);
        assert!(RETAIL_VPL_PAGE_BRIGHTNESS[0] > 0.5);
        assert!(
            RETAIL_VPL_PAGE_BRIGHTNESS
                .windows(2)
                .all(|pair| pair[0] <= pair[1])
        );
        assert_eq!(vpl_page_brightness(None), RETAIL_VPL_PAGE_BRIGHTNESS);
    }

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
        let units = f64::from_bits(crate::util::lepton::MODEL_UNITS_PER_LEPTON_BITS) as f32;
        let front = layout.to_voxels.transform_point3(Vec3::new(0.0, 0.0, 2.0));
        assert!((front.x - 128.0 * units).abs() < 1e-3, "{front}");
        let top = layout.to_voxels.transform_point3(Vec3::new(0.0, 0.5, 0.0));
        assert!(top.z > 0.0 && top.x == 0.0 && top.y == 0.0);
        assert_eq!(
            layout.drawn(ModelNodes::Unit, None).count(),
            1,
            "the body alone"
        );
        assert_eq!(
            layout.drawn(ModelNodes::Unit, Some(0)).count(),
            2,
            "and turret0; no barrel"
        );
    }

    /// A building's guns draw `body` at the turret matrix and `barrel` at
    /// the barrel matrix (43DA80's B8 and C0); a C0-only building draws the
    /// barrel alone. A debris piece draws the `VoxelIndex`th mesh node, its
    /// own mesh only, and nothing past the last.
    #[test]
    fn building_guns_and_debris_draw_their_nodes() {
        let glb = crate::assets::model_file::test_glb::box_model(2.0, 1.0, 4.0, &["barrel"]);
        let model = Arc::new(Model::from_glb(&glb).unwrap());
        let layout = Layout::of(&DeclaredModel::for_test(model, 256));
        let matrices = |nodes| {
            layout
                .drawn(nodes, None)
                .map(|(matrix, _, _)| matrix)
                .collect::<Vec<_>>()
        };
        assert_eq!(matrices(ModelNodes::BuildingGuns { turret: true }), [1, 2]);
        assert_eq!(matrices(ModelNodes::BuildingGuns { turret: false }), [2]);
        assert_eq!(matrices(ModelNodes::MeshNode(1)), [0]);
        let (_, node, frame) = layout.drawn(ModelNodes::MeshNode(1), None).next().unwrap();
        assert_eq!(node.nodes.len(), 1, "the barrel node alone");
        assert_eq!(node.bounds.unwrap().max, [1.0, 0.5, 2.0]);
        assert!(node.clip.is_none() && frame.is_none(), "at rest");
        assert_eq!(matrices(ModelNodes::MeshNode(2)), [0usize; 0]);
        // The guns animate: B8 at the turret's frame, C0 at the barrel's.
        let frames = |nodes| {
            layout
                .drawn(nodes, None)
                .map(|(_, _, frame)| frame)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            frames(ModelNodes::BuildingGuns { turret: true }),
            [Some(0), Some(1)]
        );
        // A shared part draws alone at matrix 0 and at rest (749DCE, no
        // HVA); a debris piece's shadow is cast from what it draws, at rest.
        let barrel = ModelNodes::Part(ModelPart::Barrel(0));
        assert_eq!(matrices(barrel), [0]);
        assert_eq!(frames(barrel), [None::<usize>]);
        let (shadow, frame) = layout.shadow_of(ModelNodes::MeshNode(1)).unwrap();
        assert!(std::ptr::eq(shadow, node) && frame.is_none());
        assert!(matches!(
            layout.shadow_of(ModelNodes::Unit),
            Some((_, Some(0)))
        ));
        assert!(
            layout
                .shadow_of(ModelNodes::BuildingGuns { turret: true })
                .is_none()
        );
    }

    /// A shadow lies on the bottom face as `0x00753F90`'s quad does: height
    /// does not move a point, camera z is dropped, x moves by the light, and
    /// `ConsideredAircraft=` halves the viewer before the light
    /// (`vxl_shadow::projected_quad`).
    #[test]
    fn a_shadow_projects_onto_its_bottom_face() {
        let light = f32::from_bits(SHADOW_LIGHT_X_BITS);
        // A draw matrix that tips voxel z into camera y, as the camera does.
        let draw = Mat4::from_cols(Vec4::X, Vec4::Y, Vec4::new(0.0, 0.5, 1.0, 0.0), Vec4::W);
        let near = |a: Vec3, b: Vec3| (a - b).abs().max_element() < 1e-5;
        let ground = shadow_camera(draw, 2.0, false);
        let high = ground.transform_point3(Vec3::new(10.0, 4.0, 50.0));
        assert!(near(
            high,
            ground.transform_point3(Vec3::new(10.0, 4.0, 2.0))
        ));
        assert!(near(high, Vec3::new(10.0 + light, 5.0, 0.0)), "{high}");
        let aircraft = shadow_camera(draw, 2.0, true).transform_point3(Vec3::new(10.0, 4.0, 50.0));
        assert!(
            near(aircraft, Vec3::new(5.0 + light, 2.5, 0.0)),
            "{aircraft}"
        );
    }
}
