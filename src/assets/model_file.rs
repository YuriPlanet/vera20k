//! VERA-only, experimental model art (`Model=`, `docs/picture-model-art.md`):
//! a reader for binary glTF 2.0 (`.glb`) files.
//!
//! It reads the JSON and BIN chunks, triangle meshes, the node tree of the
//! default scene, materials, embedded PNG images and node animations, and
//! refuses what the engine does not draw (compressed meshes, KTX2 textures,
//! skins, morph targets, unknown required extensions). Every read of file data
//! is bounds-checked; a bad file is an error, never a panic.
//!
//! Units are the model's own; glTF is +Y up and +Z forward. Scaling by
//! `ModelSize=` is the caller's.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::assets::picture_file::{PictureError, PictureFrames, PictureHeader};

/// The most bytes a `.glb` file may hold.
pub const MODEL_BYTES_MAX: usize = 64 << 20;
/// The most vertices all primitives of a model may hold together.
pub const MODEL_VERTICES_MAX: usize = 1 << 20;
/// The most indices all primitives of a model may hold together.
pub const MODEL_INDICES_MAX: usize = 3 << 20;
/// The most nodes a model may hold.
pub const MODEL_NODES_MAX: usize = 1024;
/// The widest or tallest an embedded image may be, in pixels.
pub const MODEL_IMAGE_SIDE_MAX: u32 = 4096;
/// The most decoded pixels all embedded images may hold together.
pub const MODEL_IMAGE_PIXELS_MAX: u64 = 32 << 20;
/// The most keyframes all animation channels may hold together.
pub const MODEL_ANIMATION_KEYS_MAX: usize = 1 << 20;

const GLB_MAGIC: u32 = 0x4654_6C67; // "glTF"
const CHUNK_JSON: u32 = 0x4E4F_534A; // "JSON"
const CHUNK_BIN: u32 = 0x004E_4942; // "BIN\0"

const BYTE: u32 = 5120;
const UNSIGNED_BYTE: u32 = 5121;
const SHORT: u32 = 5122;
const UNSIGNED_SHORT: u32 = 5123;
const UNSIGNED_INT: u32 = 5125;
const FLOAT: u32 = 5126;

const TRIANGLES: u32 = 4;

/// Required extensions this reader handles: quantized attributes are read
/// through the same component conversion as any accessor.
const KNOWN_REQUIRED: &[&str] = &["KHR_mesh_quantization"];

#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    #[error("not a binary glTF (.glb) file")]
    NotGlb,
    #[error("glTF version {0} is not 2")]
    Version(String),
    #[error("glTF JSON is not readable: {0}")]
    Json(#[from] serde_json::Error),
    #[error("malformed glTF: {0}")]
    Malformed(String),
    #[error("Draco mesh compression (KHR_draco_mesh_compression) is not supported")]
    Draco,
    #[error("meshopt compression (EXT_meshopt_compression) is not supported")]
    Meshopt,
    #[error("KTX2/Basis textures (KHR_texture_basisu) are not supported")]
    Basisu,
    #[error("skins are not supported")]
    Skin,
    #[error("morph targets are not supported")]
    MorphTargets,
    #[error("required extension {0} is not supported")]
    UnknownExtension(String),
    #[error("not supported: {0}")]
    Unsupported(String),
    #[error("{what}: {actual} is more than the {max} a model may hold")]
    TooLarge {
        what: &'static str,
        actual: u64,
        max: u64,
    },
    #[error("embedded image: {0}")]
    Image(#[from] PictureError),
}

fn malformed(detail: impl Into<String>) -> ModelError {
    ModelError::Malformed(detail.into())
}

fn too_large(what: &'static str, actual: usize, max: usize) -> ModelError {
    ModelError::TooLarge {
        what,
        actual: actual as u64,
        max: max as u64,
    }
}

/// A decoded model: everything the engine reads from a `.glb`.
#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    pub nodes: Vec<ModelNode>,
    /// The parent of each node, `None` for a top-level node.
    pub parents: Vec<Option<usize>>,
    /// The top-level nodes of the default scene.
    pub roots: Vec<usize>,
    pub meshes: Vec<ModelMesh>,
    pub materials: Vec<ModelMaterial>,
    pub images: Vec<ModelImage>,
    pub animations: Vec<ModelAnimation>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelNode {
    pub name: Option<String>,
    pub transform: NodeTransform,
    pub children: Vec<usize>,
    pub mesh: Option<usize>,
}

/// A node's local transform as the file states it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NodeTransform {
    Trs(Trs),
    /// Column-major 4x4 matrix. glTF does not animate such nodes.
    Matrix([f32; 16]),
}

impl NodeTransform {
    /// The local transform as a column-major 4x4 matrix.
    pub fn matrix(&self) -> [f32; 16] {
        match self {
            NodeTransform::Trs(trs) => trs.matrix(),
            NodeTransform::Matrix(matrix) => *matrix,
        }
    }
}

/// Translation, rotation (unit quaternion x, y, z, w) and scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Trs {
    pub translation: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
}

impl Default for Trs {
    fn default() -> Self {
        Self {
            translation: [0.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0; 3],
        }
    }
}

impl Trs {
    /// `T * R * S` as a column-major 4x4 matrix.
    pub fn matrix(&self) -> [f32; 16] {
        let [x, y, z, w] = self.rotation;
        let rotation = [
            [
                1.0 - 2.0 * (y * y + z * z),
                2.0 * (x * y - z * w),
                2.0 * (x * z + y * w),
            ],
            [
                2.0 * (x * y + z * w),
                1.0 - 2.0 * (x * x + z * z),
                2.0 * (y * z - x * w),
            ],
            [
                2.0 * (x * z - y * w),
                2.0 * (y * z + x * w),
                1.0 - 2.0 * (x * x + y * y),
            ],
        ];
        let mut matrix = [0.0; 16];
        for column in 0..3 {
            for row in 0..3 {
                matrix[column * 4 + row] = rotation[row][column] * self.scale[column];
            }
        }
        matrix[12] = self.translation[0];
        matrix[13] = self.translation[1];
        matrix[14] = self.translation[2];
        matrix[15] = 1.0;
        matrix
    }
}

/// `a * b` for column-major 4x4 matrices.
pub fn mul_matrix(a: &[f32; 16], b: &[f32; 16]) -> [f32; 16] {
    let mut out = [0.0; 16];
    for column in 0..4 {
        for row in 0..4 {
            out[column * 4 + row] = (0..4).map(|k| a[k * 4 + row] * b[column * 4 + k]).sum();
        }
    }
    out
}

/// The point `p` moved by the column-major 4x4 `matrix`.
pub fn transform_point(matrix: &[f32; 16], p: [f32; 3]) -> [f32; 3] {
    let mut out = [0.0; 3];
    for (row, value) in out.iter_mut().enumerate() {
        *value =
            matrix[row] * p[0] + matrix[4 + row] * p[1] + matrix[8 + row] * p[2] + matrix[12 + row];
    }
    out
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelMesh {
    pub name: Option<String>,
    pub primitives: Vec<ModelPrimitive>,
}

/// One triangle list. Optional attributes hold one entry per position.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelPrimitive {
    pub positions: Vec<[f32; 3]>,
    pub normals: Option<Vec<[f32; 3]>>,
    pub uvs: Option<Vec<[f32; 2]>>,
    /// `COLOR_0` as RGBA; a VEC3 colour has alpha 1.
    pub colors: Option<Vec<[f32; 4]>>,
    /// Three per triangle, each below `positions.len()`; a non-indexed
    /// primitive gets `0..positions.len()`.
    pub indices: Vec<u32>,
    pub material: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlphaMode {
    Opaque,
    Mask,
    Blend,
}

/// A material's texture: an index into `Model::images`, sampled with
/// `TEXCOORD_0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextureRef {
    pub image: usize,
}

/// The material values the engine draws with. Metallic and roughness are not
/// read.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelMaterial {
    pub base_color_factor: [f32; 4],
    pub base_color_texture: Option<TextureRef>,
    pub normal_texture: Option<TextureRef>,
    pub emissive_factor: [f32; 3],
    pub emissive_texture: Option<TextureRef>,
    pub alpha_mode: AlphaMode,
    pub alpha_cutoff: f32,
}

/// An embedded PNG decoded to straight RGBA8, row by row. An APNG gives its
/// first frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimationPath {
    Translation,
    Rotation,
    Scale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interpolation {
    Linear,
    Step,
}

/// One animated property of one node.
#[derive(Debug, Clone, PartialEq)]
pub struct AnimationChannel {
    pub node: usize,
    pub path: AnimationPath,
    pub interpolation: Interpolation,
    /// Key times in seconds, non-decreasing, at least one.
    pub times: Vec<f32>,
    /// One value per key; translation and scale leave the fourth lane 0.
    pub values: Vec<[f32; 4]>,
}

impl AnimationChannel {
    /// The channel's value at `time` seconds, held at the first and last
    /// keys outside their range.
    pub fn sample(&self, time: f32) -> [f32; 4] {
        let first = self.values.first().copied().unwrap_or_default();
        let (Some(&start), Some(&end)) = (self.times.first(), self.times.last()) else {
            return first;
        };
        if time.is_nan() || time <= start {
            return first;
        }
        if time >= end {
            return self.values.last().copied().unwrap_or(first);
        }
        // The last key at or before `time`; `time < end` keeps a next key.
        let index = self
            .times
            .partition_point(|&key| key <= time)
            .saturating_sub(1);
        let (Some(&a), Some(&b), Some(&t0), Some(&t1)) = (
            self.values.get(index),
            self.values.get(index + 1),
            self.times.get(index),
            self.times.get(index + 1),
        ) else {
            return first;
        };
        if self.interpolation == Interpolation::Step {
            return a;
        }
        let span = t1 - t0;
        let u = if span > 0.0 { (time - t0) / span } else { 1.0 };
        match self.path {
            AnimationPath::Rotation => slerp(a, b, u),
            _ => std::array::from_fn(|lane| a[lane] + (b[lane] - a[lane]) * u),
        }
    }
}

/// Spherical interpolation along the shorter arc between unit quaternions.
fn slerp(a: [f32; 4], b: [f32; 4], u: f32) -> [f32; 4] {
    let mut dot: f32 = (0..4).map(|lane| a[lane] * b[lane]).sum();
    let mut b = b;
    if dot < 0.0 {
        b = b.map(|lane| -lane);
        dot = -dot;
    }
    let (wa, wb) = if dot > 0.9995 {
        (1.0 - u, u)
    } else {
        // `libm`: the same results on every platform, as any math whose
        // results may reach shared state (AGENTS.md).
        let theta = libm::acosf(dot.min(1.0));
        let sin = libm::sinf(theta);
        (
            libm::sinf((1.0 - u) * theta) / sin,
            libm::sinf(u * theta) / sin,
        )
    };
    let out: [f32; 4] = std::array::from_fn(|lane| a[lane] * wa + b[lane] * wb);
    let length = libm::sqrtf(out.iter().map(|lane| lane * lane).sum::<f32>());
    if length > 0.0 {
        out.map(|lane| lane / length)
    } else {
        a
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelAnimation {
    pub name: Option<String>,
    pub channels: Vec<AnimationChannel>,
    /// The last key time of any channel, in seconds.
    pub duration: f32,
}

impl ModelAnimation {
    /// The frames the clip holds in place of an HVA's: the key count of its
    /// channel with the most keys, at least 1. Frame `k` is that channel's
    /// key `k` ([`Self::frame_time`]), so a modder exports one key per HVA
    /// frame.
    pub fn frame_count(&self) -> u32 {
        let keys = self
            .channels
            .iter()
            .map(|channel| channel.times.len())
            .max()
            .unwrap_or(0);
        u32::try_from(keys).unwrap_or(u32::MAX).max(1)
    }

    /// The time in seconds of HVA frame `frame`: the key time of frame
    /// `frame % `[`Self::frame_count`] on the channel with the most keys (the
    /// first such channel); 0 for a clip without keys.
    pub fn frame_time(&self, frame: u32) -> f32 {
        let Some(channel) = self
            .channels
            .iter()
            .rev()
            .max_by_key(|channel| channel.times.len())
        else {
            return 0.0;
        };
        let count = channel.times.len();
        if count == 0 {
            return 0.0;
        }
        channel.times[frame as usize % count]
    }
}

/// A named part of a model (`docs/picture-model-art.md`, "Model: NAME.glb").
/// `turret` and `turret0` are both `Turret(0)`; `turretN` stands in for
/// `TURN.VXL`, `barrelN` for `BARLN.VXL`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModelPart {
    Body,
    Turret(u32),
    Barrel(u32),
    Shadow,
}

impl ModelPart {
    /// The part a node name names, ignoring ASCII case.
    pub fn from_name(name: &str) -> Option<Self> {
        let lower = name.to_ascii_lowercase();
        let numbered = |prefix: &str| -> Option<u32> {
            let digits = lower.strip_prefix(prefix)?;
            if digits.is_empty() {
                return Some(0);
            }
            if !digits.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            digits.parse().ok()
        };
        match lower.as_str() {
            "body" => Some(Self::Body),
            "shadow" => Some(Self::Shadow),
            _ => numbered("turret")
                .map(Self::Turret)
                .or_else(|| numbered("barrel").map(Self::Barrel)),
        }
    }
}

/// An axis-aligned box in the model's units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelBounds {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

impl Model {
    /// The first node, in file order, naming `part`.
    pub fn part(&self, part: ModelPart) -> Option<usize> {
        self.nodes.iter().position(|node| {
            node.name
                .as_deref()
                .and_then(ModelPart::from_name)
                .is_some_and(|found| found == part)
        })
    }

    /// The node's rest-pose transform from its space to the model's.
    pub fn rest_world_matrix(&self, node: usize) -> [f32; 16] {
        self.world_matrix_at(node, None)
    }

    /// The node's transform from its space to the model's with every node of
    /// its chain posed at `(animation, time)` ([`Self::sample_node`]), or at
    /// rest for `None`. A node given as a matrix keeps its matrix.
    pub fn world_matrix_at(&self, node: usize, pose: Option<(usize, f32)>) -> [f32; 16] {
        let mut chain = Vec::new();
        let mut current = Some(node);
        // The tree was checked acyclic at load; the bound guards it anyway.
        while let Some(index) = current {
            if chain.len() > self.nodes.len() {
                break;
            }
            let Some(found) = self.nodes.get(index) else {
                break;
            };
            let posed = pose.and_then(|(animation, time)| self.sample_node(animation, index, time));
            chain.push(posed.map_or_else(|| found.transform.matrix(), |trs| trs.matrix()));
            current = self.parents.get(index).copied().flatten();
        }
        let mut world = Trs::default().matrix();
        for local in chain.iter().rev() {
            world = mul_matrix(&world, local);
        }
        world
    }

    /// The rest-pose bounds of `node`'s meshes and its descendants' in model
    /// space, leaving out descendants that are other parts (a turret under the
    /// body) and their subtrees. `None` when they hold no vertex.
    pub fn part_bounds(&self, node: usize) -> Option<ModelBounds> {
        let mut bounds: Option<ModelBounds> = None;
        let mut stack = vec![node];
        let mut visited = 0;
        while let Some(index) = stack.pop() {
            visited += 1;
            if visited > self.nodes.len() {
                break;
            }
            let Some(found) = self.nodes.get(index) else {
                continue;
            };
            if let Some(mesh) = found.mesh.and_then(|mesh| self.meshes.get(mesh)) {
                let world = self.rest_world_matrix(index);
                for position in mesh.primitives.iter().flat_map(|p| &p.positions) {
                    let point = transform_point(&world, *position);
                    let next = bounds.get_or_insert(ModelBounds {
                        min: point,
                        max: point,
                    });
                    for axis in 0..3 {
                        next.min[axis] = next.min[axis].min(point[axis]);
                        next.max[axis] = next.max[axis].max(point[axis]);
                    }
                }
            }
            for &child in &found.children {
                let is_part = self
                    .nodes
                    .get(child)
                    .and_then(|child| child.name.as_deref())
                    .and_then(ModelPart::from_name)
                    .is_some();
                if !is_part {
                    stack.push(child);
                }
            }
        }
        bounds
    }

    /// The `body` node's rest-pose bounds in model space.
    pub fn body_bounds(&self) -> Option<ModelBounds> {
        self.part_bounds(self.part(ModelPart::Body)?)
    }

    /// The bounds of mesh `mesh`'s vertices in its node's own space; `None`
    /// for a mesh without vertices.
    pub fn mesh_bounds(&self, mesh: usize) -> Option<ModelBounds> {
        let mut bounds: Option<ModelBounds> = None;
        for &point in self
            .meshes
            .get(mesh)?
            .primitives
            .iter()
            .flat_map(|p| &p.positions)
        {
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

    /// The clip `part` animates by in place of its HVA: the animation named
    /// for the part (`body`, `turret`/`turretN`, `barrel`/`barrelN`,
    /// `shadow`, ignoring ASCII case), else the first animation not named
    /// for any part; `None` when there is neither (the part draws at rest).
    pub fn part_clip(&self, part: ModelPart) -> Option<usize> {
        let named =
            |animation: &ModelAnimation| animation.name.as_deref().and_then(ModelPart::from_name);
        self.animations
            .iter()
            .position(|animation| named(animation) == Some(part))
            .or_else(|| {
                self.animations
                    .iter()
                    .position(|animation| named(animation).is_none())
            })
    }

    /// `node`'s local TRS at `time` seconds of `animation`: its rest TRS with
    /// the animation's channels for it applied. `None` for an index out of
    /// range or a node given as a matrix.
    pub fn sample_node(&self, animation: usize, node: usize, time: f32) -> Option<Trs> {
        let NodeTransform::Trs(mut trs) = self.nodes.get(node)?.transform else {
            return None;
        };
        let animation = self.animations.get(animation)?;
        for channel in animation.channels.iter().filter(|c| c.node == node) {
            let value = channel.sample(time);
            match channel.path {
                AnimationPath::Translation => trs.translation = [value[0], value[1], value[2]],
                AnimationPath::Rotation => trs.rotation = value,
                AnimationPath::Scale => trs.scale = [value[0], value[1], value[2]],
            }
        }
        Some(trs)
    }

    /// Decode the `.glb` file `bytes`.
    pub fn from_glb(bytes: &[u8]) -> Result<Self, ModelError> {
        if bytes.len() > MODEL_BYTES_MAX {
            return Err(too_large("file bytes", bytes.len(), MODEL_BYTES_MAX));
        }
        let (json, bin) = split_chunks(bytes)?;
        let doc: Doc = serde_json::from_slice(json)?;
        if !doc.asset.version.starts_with("2.") {
            return Err(ModelError::Version(doc.asset.version.clone()));
        }
        check_extensions(&doc)?;
        if !doc.skins.is_empty() || doc.nodes.iter().any(|node| node.skin.is_some()) {
            return Err(ModelError::Skin);
        }
        if doc.nodes.len() > MODEL_NODES_MAX {
            return Err(too_large("nodes", doc.nodes.len(), MODEL_NODES_MAX));
        }
        let reader = Reader::new(&doc, bin)?;
        let images = read_images(&doc, &reader)?;
        let materials = read_materials(&doc, images.len())?;
        let meshes = read_meshes(&doc, &reader, materials.len())?;
        let (nodes, parents, roots) = read_nodes(&doc, meshes.len())?;
        let animations = read_animations(&doc, &reader, nodes.len())?;
        Ok(Self {
            nodes,
            parents,
            roots,
            meshes,
            materials,
            images,
            animations,
        })
    }
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    let word = bytes.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes(word.try_into().ok()?))
}

/// The JSON chunk and the BIN chunk (empty when absent) of a GLB container.
fn split_chunks(bytes: &[u8]) -> Result<(&[u8], &[u8]), ModelError> {
    if u32_at(bytes, 0) != Some(GLB_MAGIC) {
        return Err(ModelError::NotGlb);
    }
    let version = u32_at(bytes, 4).ok_or_else(|| malformed("truncated header"))?;
    if version != 2 {
        return Err(ModelError::Version(version.to_string()));
    }
    let length = u32_at(bytes, 8).ok_or_else(|| malformed("truncated header"))? as usize;
    let file = bytes
        .get(..length)
        .ok_or_else(|| malformed("header length past the end of the file"))?;
    let mut at = 12;
    let mut json = None;
    let mut bin: &[u8] = &[];
    while at < file.len() {
        let chunk_length =
            u32_at(file, at).ok_or_else(|| malformed("truncated chunk header"))? as usize;
        let kind = u32_at(file, at + 4).ok_or_else(|| malformed("truncated chunk header"))?;
        let start = at + 8;
        let end = start
            .checked_add(chunk_length)
            .ok_or_else(|| malformed("chunk past the end of the file"))?;
        let data = file
            .get(start..end)
            .ok_or_else(|| malformed("chunk past the end of the file"))?;
        match (kind, json.is_some()) {
            (CHUNK_JSON, false) => json = Some(data),
            (CHUNK_BIN, true) if bin.is_empty() => bin = data,
            (_, false) => return Err(malformed("first chunk is not JSON")),
            _ => {}
        }
        at = end;
    }
    Ok((json.ok_or_else(|| malformed("no JSON chunk"))?, bin))
}

fn check_extensions(doc: &Doc) -> Result<(), ModelError> {
    for name in &doc.extensions_required {
        match name.as_str() {
            "KHR_draco_mesh_compression" => return Err(ModelError::Draco),
            "EXT_meshopt_compression" | "KHR_meshopt_compression" => {
                return Err(ModelError::Meshopt);
            }
            "KHR_texture_basisu" => return Err(ModelError::Basisu),
            known if KNOWN_REQUIRED.contains(&known) => {}
            other => return Err(ModelError::UnknownExtension(other.to_string())),
        }
    }
    Ok(())
}

/// Reads accessor data out of the BIN chunk.
struct Reader<'a> {
    doc: &'a Doc,
    /// Each buffer's bytes; only the BIN chunk's buffer is readable.
    buffers: Vec<&'a [u8]>,
}

impl<'a> Reader<'a> {
    fn new(doc: &'a Doc, bin: &'a [u8]) -> Result<Self, ModelError> {
        let mut buffers = Vec::with_capacity(doc.buffers.len());
        for (index, buffer) in doc.buffers.iter().enumerate() {
            if buffer.uri.is_some() {
                return Err(ModelError::Unsupported(
                    "buffers outside the .glb file".into(),
                ));
            }
            if index != 0 {
                return Err(malformed("only buffer 0 may use the BIN chunk"));
            }
            let bytes = bin
                .get(..buffer.byte_length)
                .ok_or_else(|| malformed("buffer longer than the BIN chunk"))?;
            buffers.push(bytes);
        }
        Ok(Self { doc, buffers })
    }

    /// A buffer view's bytes and its stride, if it states one.
    fn view(&self, index: usize) -> Result<(&'a [u8], Option<usize>), ModelError> {
        let view = self
            .doc
            .buffer_views
            .get(index)
            .ok_or_else(|| malformed("bufferView index out of range"))?;
        let buffer: &'a [u8] = self
            .buffers
            .get(view.buffer)
            .copied()
            .ok_or_else(|| malformed("buffer index out of range"))?;
        let bytes = view
            .byte_offset
            .checked_add(view.byte_length)
            .and_then(|end| buffer.get(view.byte_offset..end))
            .ok_or_else(|| malformed("bufferView past the end of its buffer"))?;
        Ok((bytes, view.byte_stride))
    }

    /// An accessor's elements as `f32`, `components` per element, with
    /// normalized integers mapped to 0..1 or -1..1.
    fn floats(&self, index: usize, components: usize) -> Result<Vec<f32>, ModelError> {
        let accessor = self.accessor(index, components)?;
        let size = component_size(accessor.component_type)?;
        let read = |bytes: &[u8]| -> Option<f32> {
            Some(match (accessor.component_type, accessor.normalized) {
                (FLOAT, _) => f32::from_le_bytes(bytes.try_into().ok()?),
                (UNSIGNED_BYTE, true) => f32::from(*bytes.first()?) / 255.0,
                (UNSIGNED_BYTE, false) => f32::from(*bytes.first()?),
                (BYTE, true) => (f32::from(*bytes.first()? as i8) / 127.0).max(-1.0),
                (BYTE, false) => f32::from(*bytes.first()? as i8),
                (UNSIGNED_SHORT, true) => {
                    f32::from(u16::from_le_bytes(bytes.try_into().ok()?)) / 65535.0
                }
                (UNSIGNED_SHORT, false) => f32::from(u16::from_le_bytes(bytes.try_into().ok()?)),
                (SHORT, true) => {
                    (f32::from(i16::from_le_bytes(bytes.try_into().ok()?)) / 32767.0).max(-1.0)
                }
                (SHORT, false) => f32::from(i16::from_le_bytes(bytes.try_into().ok()?)),
                _ => return None,
            })
        };
        if accessor.component_type == UNSIGNED_INT {
            return Err(malformed("unsigned int components in a float attribute"));
        }
        self.elements(accessor, components, size, read)
    }

    /// An index accessor's values.
    fn indices(&self, index: usize) -> Result<Vec<u32>, ModelError> {
        let accessor = self.accessor(index, 1)?;
        if accessor.normalized {
            return Err(malformed("normalized indices"));
        }
        let size = component_size(accessor.component_type)?;
        let read = |bytes: &[u8]| -> Option<u32> {
            Some(match accessor.component_type {
                UNSIGNED_BYTE => u32::from(*bytes.first()?),
                UNSIGNED_SHORT => u32::from(u16::from_le_bytes(bytes.try_into().ok()?)),
                UNSIGNED_INT => u32::from_le_bytes(bytes.try_into().ok()?),
                _ => return None,
            })
        };
        self.elements(accessor, 1, size, read)
    }

    /// The accessor at `index`, checked to hold `components` per element and
    /// no more elements than any model may.
    fn accessor(&self, index: usize, components: usize) -> Result<&'a AccessorJson, ModelError> {
        let accessor = self
            .doc
            .accessors
            .get(index)
            .ok_or_else(|| malformed("accessor index out of range"))?;
        if accessor.sparse.is_some() {
            return Err(ModelError::Unsupported("sparse accessors".into()));
        }
        let found = match accessor.kind.as_str() {
            "SCALAR" => 1,
            "VEC2" => 2,
            "VEC3" => 3,
            "VEC4" => 4,
            other => return Err(malformed(format!("accessor type {other}"))),
        };
        if found != components {
            return Err(malformed(format!(
                "accessor {index} is {} where {components} components are needed",
                accessor.kind
            )));
        }
        if accessor.count > MODEL_INDICES_MAX {
            return Err(too_large(
                "accessor elements",
                accessor.count,
                MODEL_INDICES_MAX,
            ));
        }
        Ok(accessor)
    }

    /// Reads `accessor.count` elements of `components` values each, every
    /// value through `read` on its `size` bytes. An accessor with no buffer
    /// view is all zeros.
    fn elements<T: Default + Copy>(
        &self,
        accessor: &AccessorJson,
        components: usize,
        size: usize,
        read: impl Fn(&[u8]) -> Option<T>,
    ) -> Result<Vec<T>, ModelError> {
        let total = accessor.count * components;
        let Some(view) = accessor.buffer_view else {
            return Ok(vec![T::default(); total]);
        };
        let (bytes, stride) = self.view(view)?;
        let element = size * components;
        let stride = stride.unwrap_or(element);
        if stride < element {
            return Err(malformed("byteStride smaller than an element"));
        }
        // The last element must end inside the view.
        if accessor.count > 0 {
            let end = (accessor.count - 1)
                .checked_mul(stride)
                .and_then(|at| at.checked_add(accessor.byte_offset))
                .and_then(|at| at.checked_add(element));
            if end.is_none_or(|end| end > bytes.len()) {
                return Err(malformed("accessor past the end of its bufferView"));
            }
        }
        let mut out = Vec::with_capacity(total);
        for item in 0..accessor.count {
            for component in 0..components {
                let at = accessor.byte_offset + item * stride + component * size;
                let value = bytes
                    .get(at..at + size)
                    .and_then(&read)
                    .ok_or_else(|| malformed("accessor component unreadable"))?;
                out.push(value);
            }
        }
        Ok(out)
    }
}

fn component_size(component_type: u32) -> Result<usize, ModelError> {
    match component_type {
        BYTE | UNSIGNED_BYTE => Ok(1),
        SHORT | UNSIGNED_SHORT => Ok(2),
        UNSIGNED_INT | FLOAT => Ok(4),
        other => Err(malformed(format!("componentType {other}"))),
    }
}

fn read_images(doc: &Doc, reader: &Reader<'_>) -> Result<Vec<ModelImage>, ModelError> {
    let mut pixels_total: u64 = 0;
    let mut images = Vec::with_capacity(doc.images.len());
    for image in &doc.images {
        if image.mime_type.as_deref() == Some("image/ktx2") {
            return Err(ModelError::Basisu);
        }
        if image.uri.is_some() {
            return Err(ModelError::Unsupported(
                "images outside the .glb file".into(),
            ));
        }
        let view = image
            .buffer_view
            .ok_or_else(|| malformed("image with neither uri nor bufferView"))?;
        let (bytes, _) = reader.view(view)?;
        if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            return Err(ModelError::Unsupported("images that are not PNG".into()));
        }
        let header = PictureHeader::read(bytes)?;
        let side = header.width.max(header.height);
        if side > MODEL_IMAGE_SIDE_MAX {
            return Err(ModelError::TooLarge {
                what: "image side",
                actual: u64::from(side),
                max: u64::from(MODEL_IMAGE_SIDE_MAX),
            });
        }
        pixels_total +=
            u64::from(header.width) * u64::from(header.height) * u64::from(header.frames);
        if pixels_total > MODEL_IMAGE_PIXELS_MAX {
            return Err(ModelError::TooLarge {
                what: "image pixels",
                actual: pixels_total,
                max: MODEL_IMAGE_PIXELS_MAX,
            });
        }
        let decoded = PictureFrames::decode(bytes)?;
        let rgba = decoded
            .frames
            .into_iter()
            .next()
            .ok_or(PictureError::NoFrames)?;
        images.push(ModelImage {
            width: decoded.width,
            height: decoded.height,
            rgba,
        });
    }
    Ok(images)
}

fn read_materials(doc: &Doc, image_count: usize) -> Result<Vec<ModelMaterial>, ModelError> {
    let texture = |info: &Option<TextureInfoJson>| -> Result<Option<TextureRef>, ModelError> {
        let Some(info) = info else {
            return Ok(None);
        };
        if info.tex_coord != 0 {
            return Err(ModelError::Unsupported(
                "textures sampled with a TEXCOORD other than 0".into(),
            ));
        }
        let texture = doc
            .textures
            .get(info.index)
            .ok_or_else(|| malformed("texture index out of range"))?;
        let Some(image) = texture.source else {
            if texture
                .extensions
                .as_ref()
                .is_some_and(|map| map.contains_key("KHR_texture_basisu"))
            {
                return Err(ModelError::Basisu);
            }
            return Err(malformed("texture with no source image"));
        };
        if image >= image_count {
            return Err(malformed("image index out of range"));
        }
        Ok(Some(TextureRef { image }))
    };
    doc.materials
        .iter()
        .map(|material| {
            let pbr = material.pbr_metallic_roughness.as_ref();
            let alpha_mode = match material.alpha_mode.as_deref() {
                None | Some("OPAQUE") => AlphaMode::Opaque,
                Some("MASK") => AlphaMode::Mask,
                Some("BLEND") => AlphaMode::Blend,
                Some(other) => return Err(malformed(format!("alphaMode {other}"))),
            };
            Ok(ModelMaterial {
                base_color_factor: pbr
                    .and_then(|pbr| pbr.base_color_factor)
                    .unwrap_or([1.0; 4]),
                base_color_texture: texture(&pbr.and_then(|pbr| pbr.base_color_texture.clone()))?,
                normal_texture: texture(&material.normal_texture)?,
                emissive_factor: material.emissive_factor.unwrap_or([0.0; 3]),
                emissive_texture: texture(&material.emissive_texture)?,
                alpha_mode,
                alpha_cutoff: material.alpha_cutoff.unwrap_or(0.5),
            })
        })
        .collect()
}

fn read_meshes(
    doc: &Doc,
    reader: &Reader<'_>,
    material_count: usize,
) -> Result<Vec<ModelMesh>, ModelError> {
    let mut vertices_total = 0usize;
    let mut indices_total = 0usize;
    let mut meshes = Vec::with_capacity(doc.meshes.len());
    for mesh in &doc.meshes {
        let mut primitives = Vec::with_capacity(mesh.primitives.len());
        for primitive in &mesh.primitives {
            if !primitive.targets.is_empty() {
                return Err(ModelError::MorphTargets);
            }
            let mode = primitive.mode.unwrap_or(TRIANGLES);
            if mode != TRIANGLES {
                return Err(ModelError::Unsupported(format!(
                    "primitive mode {mode} (only triangle lists)"
                )));
            }
            let attribute = |name: &str| primitive.attributes.get(name).copied();
            let position =
                attribute("POSITION").ok_or_else(|| malformed("primitive without POSITION"))?;
            let positions: Vec<[f32; 3]> = triples(reader.floats(position, 3)?);
            if positions.iter().flatten().any(|value| !value.is_finite()) {
                return Err(malformed("non-finite POSITION"));
            }
            let count = positions.len();
            vertices_total += count;
            if vertices_total > MODEL_VERTICES_MAX {
                return Err(too_large("vertices", vertices_total, MODEL_VERTICES_MAX));
            }
            let same_count = |len: usize, name: &str| {
                if len == count {
                    Ok(())
                } else {
                    Err(malformed(format!("{name} count differs from POSITION")))
                }
            };
            let normals = match attribute("NORMAL") {
                Some(index) => {
                    let normals = triples(reader.floats(index, 3)?);
                    same_count(normals.len(), "NORMAL")?;
                    Some(normals)
                }
                None => None,
            };
            let uvs = match attribute("TEXCOORD_0") {
                Some(index) => {
                    let uvs: Vec<[f32; 2]> = reader
                        .floats(index, 2)?
                        .chunks_exact(2)
                        .map(|pair| [pair[0], pair[1]])
                        .collect();
                    same_count(uvs.len(), "TEXCOORD_0")?;
                    Some(uvs)
                }
                None => None,
            };
            let colors = match attribute("COLOR_0") {
                Some(index) => {
                    let components = match doc.accessors.get(index).map(|a| a.kind.as_str()) {
                        Some("VEC3") => 3,
                        _ => 4,
                    };
                    let colors: Vec<[f32; 4]> = reader
                        .floats(index, components)?
                        .chunks_exact(components)
                        .map(|c| [c[0], c[1], c[2], c.get(3).copied().unwrap_or(1.0)])
                        .collect();
                    same_count(colors.len(), "COLOR_0")?;
                    Some(colors)
                }
                None => None,
            };
            let indices = match primitive.indices {
                Some(index) => reader.indices(index)?,
                None => (0..count as u32).collect(),
            };
            if indices.len() % 3 != 0 {
                return Err(malformed("index count not a multiple of 3"));
            }
            if indices.iter().any(|&index| index as usize >= count) {
                return Err(malformed("vertex index out of range"));
            }
            indices_total += indices.len();
            if indices_total > MODEL_INDICES_MAX {
                return Err(too_large("indices", indices_total, MODEL_INDICES_MAX));
            }
            if primitive.material.is_some_and(|m| m >= material_count) {
                return Err(malformed("material index out of range"));
            }
            primitives.push(ModelPrimitive {
                positions,
                normals,
                uvs,
                colors,
                indices,
                material: primitive.material,
            });
        }
        meshes.push(ModelMesh {
            name: mesh.name.clone(),
            primitives,
        });
    }
    Ok(meshes)
}

fn triples(values: Vec<f32>) -> Vec<[f32; 3]> {
    values.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect()
}

type NodeTree = (Vec<ModelNode>, Vec<Option<usize>>, Vec<usize>);

/// The nodes, each node's parent and the default scene's roots, with the
/// tree checked: every index in range, one parent at most, no cycles.
fn read_nodes(doc: &Doc, mesh_count: usize) -> Result<NodeTree, ModelError> {
    let count = doc.nodes.len();
    let mut parents = vec![None; count];
    let mut nodes = Vec::with_capacity(count);
    for (index, node) in doc.nodes.iter().enumerate() {
        for &child in &node.children {
            let slot = parents
                .get_mut(child)
                .ok_or_else(|| malformed("child node index out of range"))?;
            if slot.is_some() || child == index {
                return Err(malformed("node that is its own child or has two parents"));
            }
            *slot = Some(index);
        }
        if node.mesh.is_some_and(|mesh| mesh >= mesh_count) {
            return Err(malformed("mesh index out of range"));
        }
        let transform = match node.matrix {
            Some(matrix) => NodeTransform::Matrix(matrix),
            None => NodeTransform::Trs(Trs {
                translation: node.translation.unwrap_or([0.0; 3]),
                rotation: node.rotation.unwrap_or([0.0, 0.0, 0.0, 1.0]),
                scale: node.scale.unwrap_or([1.0; 3]),
            }),
        };
        nodes.push(ModelNode {
            name: node.name.clone(),
            transform,
            children: node.children.clone(),
            mesh: node.mesh,
        });
    }
    // With one parent each, a cycle is a parent chain longer than the tree.
    for start in 0..count {
        let mut current = parents[start];
        let mut steps = 0;
        while let Some(parent) = current {
            steps += 1;
            if steps > count {
                return Err(malformed("node cycle"));
            }
            current = parents.get(parent).copied().flatten();
        }
    }
    let roots = if doc.scenes.is_empty() {
        (0..count)
            .filter(|&index| parents[index].is_none())
            .collect()
    } else {
        let scene = doc
            .scenes
            .get(doc.scene.unwrap_or(0))
            .ok_or_else(|| malformed("scene index out of range"))?;
        if scene
            .nodes
            .iter()
            .any(|&node| node >= count || parents[node].is_some())
        {
            return Err(malformed("scene root out of range or not top-level"));
        }
        scene.nodes.clone()
    };
    Ok((nodes, parents, roots))
}

fn read_animations(
    doc: &Doc,
    reader: &Reader<'_>,
    node_count: usize,
) -> Result<Vec<ModelAnimation>, ModelError> {
    let mut keys_total = 0usize;
    let mut animations = Vec::with_capacity(doc.animations.len());
    for animation in &doc.animations {
        let mut channels = Vec::with_capacity(animation.channels.len());
        let mut duration: f32 = 0.0;
        for channel in &animation.channels {
            let path = match channel.target.path.as_str() {
                "translation" => AnimationPath::Translation,
                "rotation" => AnimationPath::Rotation,
                "scale" => AnimationPath::Scale,
                "weights" => return Err(ModelError::MorphTargets),
                other => return Err(malformed(format!("animation path {other}"))),
            };
            // A channel without a node targets an extension's object: skipped.
            let Some(node) = channel.target.node else {
                continue;
            };
            if node >= node_count {
                return Err(malformed("animated node index out of range"));
            }
            let sampler = animation
                .samplers
                .get(channel.sampler)
                .ok_or_else(|| malformed("animation sampler index out of range"))?;
            let interpolation = match sampler.interpolation.as_deref() {
                None | Some("LINEAR") => Interpolation::Linear,
                Some("STEP") => Interpolation::Step,
                Some(other) => {
                    return Err(ModelError::Unsupported(format!("{other} interpolation")));
                }
            };
            let times = reader.floats(sampler.input, 1)?;
            if times.is_empty()
                || times.iter().any(|time| !time.is_finite())
                || times.windows(2).any(|pair| pair[1] < pair[0])
            {
                return Err(malformed("animation times empty, non-finite or decreasing"));
            }
            let components = if path == AnimationPath::Rotation {
                4
            } else {
                3
            };
            let values: Vec<[f32; 4]> = reader
                .floats(sampler.output, components)?
                .chunks_exact(components)
                .map(|c| [c[0], c[1], c[2], c.get(3).copied().unwrap_or(0.0)])
                .collect();
            if values.len() != times.len() {
                return Err(malformed("animation output count differs from input"));
            }
            keys_total += times.len();
            if keys_total > MODEL_ANIMATION_KEYS_MAX {
                return Err(too_large(
                    "animation keys",
                    keys_total,
                    MODEL_ANIMATION_KEYS_MAX,
                ));
            }
            duration = duration.max(times.last().copied().unwrap_or(0.0));
            channels.push(AnimationChannel {
                node,
                path,
                interpolation,
                times,
                values,
            });
        }
        animations.push(ModelAnimation {
            name: animation.name.clone(),
            channels,
            duration,
        });
    }
    Ok(animations)
}

// The glTF JSON fields this reader takes; others are ignored.

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct Doc {
    asset: AssetJson,
    extensions_required: Vec<String>,
    scene: Option<usize>,
    scenes: Vec<SceneJson>,
    nodes: Vec<NodeJson>,
    meshes: Vec<MeshJson>,
    accessors: Vec<AccessorJson>,
    buffer_views: Vec<BufferViewJson>,
    buffers: Vec<BufferJson>,
    materials: Vec<MaterialJson>,
    textures: Vec<TextureJson>,
    images: Vec<ImageJson>,
    animations: Vec<AnimationJson>,
    skins: Vec<serde_json::Value>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct AssetJson {
    version: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct SceneJson {
    nodes: Vec<usize>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct NodeJson {
    name: Option<String>,
    children: Vec<usize>,
    mesh: Option<usize>,
    skin: Option<usize>,
    matrix: Option<[f32; 16]>,
    translation: Option<[f32; 3]>,
    rotation: Option<[f32; 4]>,
    scale: Option<[f32; 3]>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct MeshJson {
    name: Option<String>,
    primitives: Vec<PrimitiveJson>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct PrimitiveJson {
    attributes: BTreeMap<String, usize>,
    indices: Option<usize>,
    material: Option<usize>,
    mode: Option<u32>,
    targets: Vec<serde_json::Value>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct AccessorJson {
    buffer_view: Option<usize>,
    byte_offset: usize,
    component_type: u32,
    normalized: bool,
    count: usize,
    #[serde(rename = "type")]
    kind: String,
    sparse: Option<serde_json::Value>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct BufferViewJson {
    buffer: usize,
    byte_offset: usize,
    byte_length: usize,
    byte_stride: Option<usize>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct BufferJson {
    byte_length: usize,
    uri: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct MaterialJson {
    pbr_metallic_roughness: Option<PbrJson>,
    normal_texture: Option<TextureInfoJson>,
    emissive_texture: Option<TextureInfoJson>,
    emissive_factor: Option<[f32; 3]>,
    alpha_mode: Option<String>,
    alpha_cutoff: Option<f32>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct PbrJson {
    base_color_factor: Option<[f32; 4]>,
    base_color_texture: Option<TextureInfoJson>,
}

#[derive(Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase", default)]
struct TextureInfoJson {
    index: usize,
    tex_coord: u32,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct TextureJson {
    source: Option<usize>,
    extensions: Option<serde_json::Map<String, serde_json::Value>>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct ImageJson {
    buffer_view: Option<usize>,
    mime_type: Option<String>,
    uri: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct AnimationJson {
    name: Option<String>,
    channels: Vec<ChannelJson>,
    samplers: Vec<SamplerJson>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ChannelJson {
    sampler: usize,
    target: TargetJson,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct TargetJson {
    node: Option<usize>,
    path: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct SamplerJson {
    input: usize,
    output: usize,
    interpolation: Option<String>,
}

/// A tiny `.glb` writer for tests of model readers, here and in the
/// client's tests.
#[cfg(any(test, feature = "test-support"))]
pub mod test_glb {
    use super::{CHUNK_BIN, CHUNK_JSON, FLOAT, GLB_MAGIC};
    use serde_json::{Value, json};

    /// Collects float accessors into one BIN buffer and wraps a glTF JSON
    /// document around them.
    #[derive(Default)]
    pub struct GlbBuilder {
        bin: Vec<u8>,
        views: Vec<Value>,
        accessors: Vec<Value>,
    }

    impl GlbBuilder {
        /// Adds `data` as a float accessor of `kind`; returns its index.
        pub fn floats(&mut self, data: &[f32], kind: &str, components: usize) -> usize {
            let offset = self.bin.len();
            for value in data {
                self.bin.extend_from_slice(&value.to_le_bytes());
            }
            self.views.push(json!({
                "buffer": 0, "byteOffset": offset, "byteLength": data.len() * 4
            }));
            self.accessors.push(json!({
                "bufferView": self.views.len() - 1, "componentType": FLOAT,
                "count": data.len() / components, "type": kind
            }));
            self.accessors.len() - 1
        }

        pub fn build(self, mut doc: Value) -> Vec<u8> {
            doc["asset"] = json!({ "version": "2.0" });
            doc["buffers"] = json!([{ "byteLength": self.bin.len() }]);
            doc["bufferViews"] = Value::Array(self.views);
            doc["accessors"] = Value::Array(self.accessors);
            let mut json = serde_json::to_vec(&doc).unwrap();
            while json.len() % 4 != 0 {
                json.push(b' ');
            }
            let mut bin = self.bin;
            while bin.len() % 4 != 0 {
                bin.push(0);
            }
            let length = 12 + 8 + json.len() + 8 + bin.len();
            let mut out = Vec::new();
            out.extend_from_slice(&GLB_MAGIC.to_le_bytes());
            out.extend_from_slice(&2u32.to_le_bytes());
            out.extend_from_slice(&(length as u32).to_le_bytes());
            out.extend_from_slice(&(json.len() as u32).to_le_bytes());
            out.extend_from_slice(&CHUNK_JSON.to_le_bytes());
            out.extend_from_slice(&json);
            out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
            out.extend_from_slice(&CHUNK_BIN.to_le_bytes());
            out.extend_from_slice(&bin);
            out
        }
    }

    /// A model of one triangle-pair box mesh under a `body` node: the box
    /// spans `width` along X, `height` along Y and `length` along Z (the
    /// forward axis), centred on the origin; each name in `parts` adds a
    /// node of that name carrying the same mesh.
    pub fn box_model(width: f32, height: f32, length: f32, parts: &[&str]) -> Vec<u8> {
        let (x, y, z) = (width / 2.0, height / 2.0, length / 2.0);
        let mut builder = GlbBuilder::default();
        let positions = builder.floats(&[-x, -y, -z, x, y, z, -x, y, z], "VEC3", 3);
        let mut nodes = vec![json!({ "name": "body", "mesh": 0 })];
        nodes.extend(parts.iter().map(|name| json!({ "name": name, "mesh": 0 })));
        let roots: Vec<usize> = (0..nodes.len()).collect();
        builder.build(json!({
            "scene": 0,
            "scenes": [{ "nodes": roots }],
            "nodes": nodes,
            "meshes": [{ "primitives": [{ "attributes": { "POSITION": positions } }] }]
        }))
    }
}

#[cfg(all(test, tests_assets))]
mod tests {
    use super::test_glb::GlbBuilder;
    use super::*;
    use serde_json::json;

    #[test]
    fn body_bounds_take_a_translated_child_but_not_the_turret() {
        let mut builder = GlbBuilder::default();
        let positions = builder.floats(&[-1.0, 0.0, -2.0, 1.0, 0.0, 2.0, 0.0, 1.0, 0.0], "VEC3", 3);
        let glb = builder.build(json!({
            "scene": 0,
            "scenes": [{ "nodes": [0] }],
            "nodes": [
                { "name": "body", "translation": [1.0, 0.0, 0.0], "children": [1, 2] },
                { "mesh": 0, "translation": [0.0, 2.0, 0.0] },
                { "name": "Turret", "mesh": 0, "translation": [0.0, -5.0, 0.0] }
            ],
            "meshes": [{ "primitives": [{ "attributes": { "POSITION": positions } }] }]
        }));
        let model = Model::from_glb(&glb).unwrap();
        assert_eq!(model.roots, vec![0]);
        assert_eq!(model.part(ModelPart::Turret(0)), Some(2));
        assert_eq!(model.part(ModelPart::Barrel(1)), None);
        assert_eq!(model.meshes[0].primitives[0].indices, vec![0, 1, 2]);
        // The child's vertices moved by (1, 2, 0); the turret at y -3 is left out.
        assert_eq!(
            model.body_bounds(),
            Some(ModelBounds {
                min: [0.0, 2.0, -2.0],
                max: [2.0, 3.0, 2.0],
            })
        );
    }

    #[test]
    fn linear_translation_samples_between_keys() {
        let mut builder = GlbBuilder::default();
        let times = builder.floats(&[0.0, 2.0], "SCALAR", 1);
        let values = builder.floats(&[0.0, 0.0, 0.0, 4.0, -2.0, 0.0], "VEC3", 3);
        let glb = builder.build(json!({
            "nodes": [{ "name": "barrel", "scale": [2.0, 2.0, 2.0] }],
            "animations": [{
                "channels": [{ "sampler": 0, "target": { "node": 0, "path": "translation" } }],
                "samplers": [{ "input": times, "output": values, "interpolation": "LINEAR" }]
            }]
        }));
        let model = Model::from_glb(&glb).unwrap();
        assert_eq!(model.animations[0].duration, 2.0);
        let trs = model.sample_node(0, 0, 0.5).unwrap();
        assert_eq!(trs.translation, [1.0, -0.5, 0.0]);
        assert_eq!(trs.scale, [2.0; 3]);
        // Past the last key the value holds.
        assert_eq!(
            model.sample_node(0, 0, 9.0).unwrap().translation,
            [4.0, -2.0, 0.0]
        );
    }

    /// A clip's frames are its keys: frame `k` samples key `k % count` of the
    /// channel with the most keys, and a part takes the clip named for it,
    /// else the first clip named for no part.
    #[test]
    fn animation_frames_map_to_their_key_times() {
        let mut builder = GlbBuilder::default();
        let times = builder.floats(&[0.0, 0.5, 2.0], "SCALAR", 1);
        let values = builder.floats(&[0.0; 9], "VEC3", 3);
        let short = builder.floats(&[0.0, 1.0], "SCALAR", 1);
        let short_values = builder.floats(&[0.0; 6], "VEC3", 3);
        let sampler = |input, output| json!({ "input": input, "output": output });
        let glb = builder.build(json!({
            "nodes": [{ "name": "body" }, { "name": "turret" }],
            "animations": [
                {
                    "name": "Turret",
                    "channels": [{ "sampler": 0, "target": { "node": 1, "path": "translation" } }],
                    "samplers": [sampler(short, short_values)]
                },
                {
                    "name": "drive",
                    "channels": [
                        { "sampler": 0, "target": { "node": 0, "path": "translation" } },
                        { "sampler": 1, "target": { "node": 0, "path": "scale" } }
                    ],
                    "samplers": [sampler(short, short_values), sampler(times, values)]
                }
            ]
        }));
        let model = Model::from_glb(&glb).unwrap();
        assert_eq!(model.part_clip(ModelPart::Turret(0)), Some(0));
        assert_eq!(model.part_clip(ModelPart::Body), Some(1));
        let drive = &model.animations[1];
        assert_eq!(drive.frame_count(), 3);
        assert_eq!(drive.frame_time(1), 0.5);
        // Frame 4 of 3 is frame 1.
        assert_eq!(drive.frame_time(4), 0.5);
        assert_eq!(model.animations[0].frame_count(), 2);
    }

    #[test]
    fn required_draco_and_non_glb_are_refused() {
        let glb = GlbBuilder::default().build(json!({
            "extensionsUsed": ["KHR_draco_mesh_compression"],
            "extensionsRequired": ["KHR_draco_mesh_compression"]
        }));
        assert!(matches!(Model::from_glb(&glb), Err(ModelError::Draco)));
        assert!(matches!(
            Model::from_glb(b"{\"asset\":{\"version\":\"2.0\"}}"),
            Err(ModelError::NotGlb)
        ));
    }
}
