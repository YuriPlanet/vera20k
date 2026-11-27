//! `asset export-model <NAME>`: a voxel unit as model art
//! (`docs/picture-model-art.md`, "Model: NAME.glb"): `<out>/model/NAME.glb`
//! from `NAME.VXL`, `NAMETUR.VXL` and `NAMEBARL.VXL` and their HVAs (for the
//! building gun GTGCANTUR, its barrel GTGCANBARL), and
//! `<out>/model/NAME.mask.png` beside it when the unit has house colour.
//!
//! Each VXL becomes one node: `body`, `turret`, `barrel`. Every section of a
//! file is one primitive of its node's mesh, made of the voxels' exposed
//! faces, placed as the native draw places the voxels at HVA frame 0
//! (`vxl_native::prepare_geometry`): the section's bounds map its grid, then
//! the HVA matrix, its translation scaled by section 0's scale (`5F823E`).
//! One glTF unit is one voxel-space unit, so the three parts keep their
//! relative places; the axes are those `render::model_draw` assumes (glTF +Z
//! the hull's X, forward; X its Y; +Y its Z, up).
//!
//! Colour: the draw multiplies `COLOR_0` into the base colour, so each face
//! carries its voxel's palette colour as a linear `COLOR_0`; its `NORMAL` is
//! the voxel's VXL table normal (`vxl_normals::get_normal`) turned by the
//! HVA. House colour comes from the `NAME.mask.png` sidecar, sampled at
//! `TEXCOORD_0`: the mask is two texels, black and red, and a house-colour
//! voxel's faces (indices 16-31) take the red texel's centre. Their colour is
//! grey by step as `export-picture` writes it, 17 darker per step from index
//! 16, less half a step so the shader's float brightness lands inside the
//! step it picks the owner's ramp by. Files made from retail art stay on the
//! player's machine.
//!
//! ## Dependency rules
//! - Part of `asset_tools/`: depends on `assets/`, `rules/`, `util/` and the
//!   CPU-only `render::vxl_normals` and `render::vxl_raster`.

use glam::{Mat3, Mat4, Vec3};
use serde_json::{Value, json};

use crate::asset_tools::report::ErrorReport;
use crate::asset_tools::verb_render::RenderOptions;
use crate::assets::asset_manager::AssetManager;
use crate::assets::hva_file::HvaFile;
use crate::assets::model_file::{Model, ModelBounds};
use crate::assets::pal_file::Palette;
use crate::assets::vxl_file::VxlFile;
use crate::render::vxl_normals::get_normal;
use crate::render::vxl_raster::hva_to_mat4;
use crate::rules::art_data::MODEL_SIZE_MAX;
use crate::rules::voxel_asset_catalog::{model_tilt_half_sizes, voxel_tilt_half_sizes};

/// Subdirectory under the output root that this verb owns.
const MODEL_SUBDIR: &str = "model";
/// The palette voxels take their colours from unless `--palette` names one.
const DEFAULT_PALETTE: &str = "unittem.pal";
/// `TEXCOORD_0` of a face: the centre of the mask's black texel, or of its
/// red one for a house-colour voxel.
const UV_PLAIN: [f32; 2] = [0.25, 0.5];
const UV_HOUSE: [f32; 2] = [0.75, 0.5];

const FLOAT: u32 = 5126;
const UNSIGNED_INT: u32 = 5125;

#[derive(Debug, serde::Serialize)]
pub struct ExportModelReport {
    pub model: String,
    pub mask: Option<String>,
    pub palette: String,
    pub parts: Vec<ExportModelPart>,
    pub house_voxels: usize,
    /// The `body` node's rest length along glTF +Z, in voxels.
    pub body_length: f32,
    /// Its width along glTF X, in voxels.
    pub body_width: f32,
    /// `NAME.VXL` section 0's `size_x` and `size_y`, which the tilt half
    /// sizes come from.
    pub vxl_size: [u8; 2],
    pub vxl_tilt_half_sizes: [f32; 2],
    /// The suggested `ModelSize=`, in leptons ([`suggested_model_size`]).
    pub model_size: i32,
    /// The tilt half sizes the game takes from the model at `model_size`.
    pub model_tilt_half_sizes: Option<[f32; 2]>,
    /// Whether they equal the VXL's.
    pub tilt_matches: bool,
    /// The scale the draw gives the model at `model_size`
    /// (`model_size * u / body_length`); 1 draws it voxel for voxel.
    pub draw_scale: f32,
}

#[derive(Debug, serde::Serialize)]
pub struct ExportModelPart {
    pub node: &'static str,
    pub vxl: String,
    pub hva: Option<String>,
    pub sections: usize,
    pub voxels: usize,
    pub vertices: usize,
    pub triangles: usize,
}

fn fail(error: String) -> ErrorReport {
    ErrorReport { error, hint: None }
}

/// Write `<out>/model/<NAME>.glb` (and `.mask.png`) from `<NAME>.VXL`.
pub fn run(
    asset_manager: &AssetManager,
    name: &str,
    opts: &RenderOptions,
) -> Result<ExportModelReport, ErrorReport> {
    let stem = name
        .rsplit_once('.')
        .filter(|(_, ext)| ext.eq_ignore_ascii_case("vxl"))
        .map_or(name, |(stem, _)| stem)
        .to_ascii_uppercase();
    let body = read_part(asset_manager, &stem)?.ok_or_else(|| ErrorReport {
        error: format!("asset not found: {stem}.VXL"),
        hint: Some(format!("run `asset find {stem}.VXL`")),
    })?;
    let turret = read_part(asset_manager, &format!("{stem}TUR"))?;
    // A building's voxel gun (`TurretAnim=NAMETUR`) keeps its barrel in the
    // file with TUR turned to BARL (`voxel_frame_catalog`'s pair names); the
    // model then draws `body` as the turret and `barrel` as the barrel.
    let barrel = match read_part(asset_manager, &format!("{stem}BARL"))? {
        Some(barrel) => Some(barrel),
        None => match stem.strip_suffix("TUR") {
            Some(base) => read_part(asset_manager, &format!("{base}BARL"))?,
            None => None,
        },
    };
    let palette_name = opts.palette.as_deref().unwrap_or(DEFAULT_PALETTE);
    let palette = asset_manager
        .get_ref(palette_name)
        .and_then(|bytes| Palette::from_bytes(bytes).ok())
        .ok_or_else(|| fail(format!("palette not found: {palette_name}")))?;

    let mut inputs = vec![("body", &body)];
    inputs.extend(turret.as_ref().map(|part| ("turret", part)));
    inputs.extend(barrel.as_ref().map(|part| ("barrel", part)));
    let built = build_glb(
        &inputs
            .iter()
            .map(|(node, part)| (*node, &part.vxl, part.hva.as_ref().map(|(_, hva)| hva)))
            .collect::<Vec<_>>(),
        &palette,
    );
    let model = Model::from_glb(&built.glb)
        .map_err(|error| fail(format!("the written model does not read back: {error}")))?;
    let bounds = model
        .body_bounds()
        .ok_or_else(|| fail(format!("{stem}.VXL has no voxels to export")))?;
    let section = body
        .vxl
        .limbs
        .first()
        .ok_or_else(|| fail(format!("{stem}.VXL has no sections")))?;
    let vxl_half = voxel_tilt_half_sizes(&body.vxl).unwrap_or([0.0; 2]);
    let (model_size, tilt_matches) = suggested_model_size(bounds, section.size_x, vxl_half);
    let body_length = bounds.max[2] - bounds.min[2];
    let units_per_lepton = f64::from_bits(crate::util::lepton::MODEL_UNITS_PER_LEPTON_BITS);
    let draw_scale = (f64::from(model_size) * units_per_lepton / f64::from(body_length)) as f32;

    let dir = opts.out.join(MODEL_SUBDIR);
    std::fs::create_dir_all(&dir)
        .map_err(|error| fail(format!("cannot create {}: {error}", dir.display())))?;
    let write = |file: String, bytes: &[u8]| {
        let path = dir.join(&file);
        std::fs::write(&path, bytes)
            .map_err(|error| fail(format!("cannot write {}: {error}", path.display())))?;
        Ok::<_, ErrorReport>(path.display().to_string())
    };
    let model_path = write(format!("{stem}.glb"), &built.glb)?;
    let mask = if built.house_voxels > 0 {
        let texels = vec![0, 0, 0, 255, 255, 0, 0, 255];
        let png = crate::assets::picture_file::encode_frames(2, 1, &[texels])
            .map_err(|error| fail(format!("encoding {stem}.mask.png: {error}")))?;
        Some(write(format!("{stem}.mask.png"), &png)?)
    } else {
        None
    };
    let parts = inputs
        .iter()
        .zip(&built.parts)
        .map(|((node, input), counts)| ExportModelPart {
            node: *node,
            vxl: input.vxl_name.clone(),
            hva: input.hva.as_ref().map(|(name, _)| name.clone()),
            sections: input.vxl.limbs.len(),
            voxels: counts.voxels,
            vertices: counts.vertices,
            triangles: counts.triangles,
        })
        .collect();
    Ok(ExportModelReport {
        model: model_path,
        mask,
        palette: palette_name.to_owned(),
        parts,
        house_voxels: built.house_voxels,
        body_length,
        body_width: bounds.max[0] - bounds.min[0],
        vxl_size: [section.size_x, section.size_y],
        vxl_tilt_half_sizes: vxl_half,
        model_size,
        model_tilt_half_sizes: model_tilt_half_sizes(bounds, model_size),
        tilt_matches,
        draw_scale,
    })
}

/// One VXL and its HVA, as read.
struct PartInput {
    vxl_name: String,
    vxl: VxlFile,
    hva: Option<(String, HvaFile)>,
}

/// `<STEM>.VXL` and `<STEM>.HVA`; None when the VXL is not there.
fn read_part(asset_manager: &AssetManager, stem: &str) -> Result<Option<PartInput>, ErrorReport> {
    let vxl_name = format!("{stem}.VXL");
    let Some(bytes) = asset_manager.get_ref(&vxl_name) else {
        return Ok(None);
    };
    let vxl = VxlFile::from_bytes(bytes)
        .map_err(|error| fail(format!("{vxl_name} does not parse: {error}")))?;
    let hva_name = format!("{stem}.HVA");
    let hva = match asset_manager.get_ref(&hva_name) {
        Some(bytes) => Some((
            hva_name.clone(),
            HvaFile::from_bytes(bytes)
                .map_err(|error| fail(format!("{hva_name} does not parse: {error}")))?,
        )),
        None => None,
    };
    Ok(Some(PartInput { vxl_name, vxl, hva }))
}

/// The `ModelSize=` whose tilt half sizes equal the VXL's. The game gives a
/// model `size_x = round(ModelSize * u)` and
/// `size_y = round(width / length * ModelSize * u)`
/// ([`model_tilt_half_sizes`]), so the inverse is `ModelSize ≈ size_x / u`:
/// of 1..=`MODEL_SIZE_MAX` the value nearest it whose half sizes are the
/// VXL's, through the game's own conversion; else the nearest, unmatched.
fn suggested_model_size(bounds: ModelBounds, size_x: u8, vxl_half: [f32; 2]) -> (i32, bool) {
    let units_per_lepton = f64::from_bits(crate::util::lepton::MODEL_UNITS_PER_LEPTON_BITS);
    let ideal = f64::from(size_x) / units_per_lepton;
    let distance = |size: i32| (f64::from(size) - ideal).abs();
    (1..=MODEL_SIZE_MAX)
        .filter(|&size| model_tilt_half_sizes(bounds, size) == Some(vxl_half))
        .min_by(|a, b| distance(*a).total_cmp(&distance(*b)))
        .map_or(
            ((ideal.round() as i32).clamp(1, MODEL_SIZE_MAX), false),
            |size| (size, true),
        )
}

/// What one part's mesh holds.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PartCounts {
    pub voxels: usize,
    pub vertices: usize,
    pub triangles: usize,
}

/// A written model.
pub struct BuiltModel {
    pub glb: Vec<u8>,
    /// One per input part, in order.
    pub parts: Vec<PartCounts>,
    /// Voxels with a house-colour index (16-31) across all parts.
    pub house_voxels: usize,
}

/// One section's exposed faces.
#[derive(Default)]
struct Primitive {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

/// Voxel space (X forward, Y, Z up) to glTF: the inverse of `model_draw`'s
/// `to_voxels` at scale 1 (glTF +Z to voxel X, X to Y, +Y to Z).
fn to_gltf(v: Vec3) -> [f32; 3] {
    [v.y, v.z, v.x]
}

/// The `.glb` of `parts`, each a node name (`body`, `turret`, `barrel`), its
/// VXL and its HVA, coloured from `palette`.
pub fn build_glb(parts: &[(&str, &VxlFile, Option<&HvaFile>)], palette: &Palette) -> BuiltModel {
    let colors = vertex_colors(palette);
    let mut writer = GlbWriter::default();
    let mut meshes = Vec::new();
    let mut nodes = Vec::new();
    let mut counts = Vec::new();
    let mut house_voxels = 0;
    for &(node, vxl, hva) in parts {
        let mut count = PartCounts::default();
        let mut primitives = Vec::new();
        for index in 0..vxl.limbs.len() {
            let (primitive, house) = section_faces(vxl, hva, index, &colors);
            count.voxels += vxl.limbs[index].voxels.len();
            house_voxels += house;
            if primitive.indices.is_empty() {
                continue;
            }
            count.vertices += primitive.positions.len();
            count.triangles += primitive.indices.len() / 3;
            primitives.push(json!({
                "attributes": {
                    "POSITION": writer.floats(&primitive.positions, "VEC3", true),
                    "NORMAL": writer.floats(&primitive.normals, "VEC3", false),
                    "TEXCOORD_0": writer.floats(&primitive.uvs, "VEC2", false),
                    "COLOR_0": writer.floats(&primitive.colors, "VEC4", false),
                },
                "indices": writer.indices(&primitive.indices),
                "material": 0,
            }));
        }
        let mut entry = json!({ "name": node });
        if !primitives.is_empty() {
            entry["mesh"] = json!(meshes.len());
            meshes.push(json!({ "name": node, "primitives": primitives }));
        }
        nodes.push(entry);
        counts.push(count);
    }
    let roots: Vec<usize> = (0..nodes.len()).collect();
    let glb = writer.finish(json!({
        "asset": { "version": "2.0", "generator": "vera20k asset export-model" },
        "scene": 0,
        "scenes": [{ "nodes": roots }],
        "nodes": nodes,
        "meshes": meshes,
        "materials": [{
            "name": "voxel",
            "pbrMetallicRoughness": {
                "baseColorFactor": [1.0, 1.0, 1.0, 1.0],
                "metallicFactor": 0.0,
                "roughnessFactor": 1.0
            }
        }],
    }));
    BuiltModel {
        glb,
        parts: counts,
        house_voxels,
    }
}

/// Section `index`'s exposed voxel faces in glTF space, and how many of its
/// voxels are house colour.
fn section_faces(
    vxl: &VxlFile,
    hva: Option<&HvaFile>,
    index: usize,
    colors: &[[f32; 4]; 256],
) -> (Primitive, usize) {
    let limb = &vxl.limbs[index];
    // The native draw (`vxl_native::prepare_geometry`) maps the section's
    // bounds box through the HVA matrix, whose translation `5F823E` scales by
    // section 0's scale; without an HVA it takes no transform (`706F64`).
    let lo = Vec3::new(limb.bounds[0], limb.bounds[1], limb.bounds[2]);
    let hi = Vec3::new(limb.bounds[3], limb.bounds[4], limb.bounds[5]);
    let size = [limb.size_x, limb.size_y, limb.size_z].map(usize::from);
    let sizes = Vec3::from(size.map(|s| s.max(1) as f32));
    let grid = Mat4::from_translation(lo) * Mat4::from_scale((hi - lo) / sizes);
    let scale = vxl.limbs.first().map_or(1.0, |first| first.scale);
    let bone = hva
        .and_then(|hva| hva.get_transform(0, index as u32))
        .map_or(Mat4::IDENTITY, |raw| hva_to_mat4(raw, scale));
    let matrix = bone * grid;
    let turn = Mat3::from_mat4(bone);
    // A mirroring matrix turns the faces inside out; wind them the other way.
    let mirrored = Mat3::from_mat4(matrix).determinant() < 0.0;

    let at = |p: [i32; 3]| -> Option<usize> {
        (0..3)
            .all(|axis| p[axis] >= 0 && (p[axis] as usize) < size[axis])
            .then(|| p[0] as usize + size[0] * (p[1] as usize + size[1] * p[2] as usize))
    };
    let mut filled = vec![false; size[0] * size[1] * size[2]];
    for voxel in &limb.voxels {
        if let Some(cell) = at([voxel.x, voxel.y, voxel.z].map(i32::from)) {
            filled[cell] = true;
        }
    }

    let mut out = Primitive::default();
    let mut house = 0;
    for voxel in &limb.voxels {
        let p = [voxel.x, voxel.y, voxel.z].map(i32::from);
        if at(p).is_none() {
            continue;
        }
        let is_house = (16..=31).contains(&voxel.color_index);
        house += usize::from(is_house);
        let color = colors[usize::from(voxel.color_index)];
        let normal =
            to_gltf((turn * get_normal(limb.normals_mode, voxel.normal_index)).normalize_or_zero());
        let uv = if is_house { UV_HOUSE } else { UV_PLAIN };
        for axis in 0..3 {
            for positive in [false, true] {
                let mut neighbour = p;
                neighbour[axis] += if positive { 1 } else { -1 };
                if at(neighbour).is_some_and(|cell| filled[cell]) {
                    continue;
                }
                // The face's corners run base, +u, +u+v, +v, with u x v along
                // +axis: counter-clockwise seen from +axis.
                let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
                let mut base = p.map(|c| c as f32);
                if positive {
                    base[axis] += 1.0;
                }
                let first = out.positions.len() as u32;
                for (du, dv) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)] {
                    let mut corner = base;
                    corner[u] += du;
                    corner[v] += dv;
                    out.positions
                        .push(to_gltf(matrix.transform_point3(Vec3::from(corner))));
                    out.normals.push(normal);
                    out.uvs.push(uv);
                    out.colors.push(color);
                }
                let quad = if positive != mirrored {
                    [0, 1, 2, 0, 2, 3]
                } else {
                    [0, 2, 1, 0, 3, 2]
                };
                out.indices.extend(quad.map(|k| first + k));
            }
        }
    }
    (out, house)
}

/// Each palette index's linear `COLOR_0`: its palette colour, or for the
/// house-colour indices 16-31 a grey by step (see the module docs).
fn vertex_colors(palette: &Palette) -> [[f32; 4]; 256] {
    std::array::from_fn(|index| {
        let index = index as u8;
        let rgb = match index {
            16..=31 => [(255 - (index - 16) * 17).saturating_sub(8); 3],
            _ => {
                let color = palette.color(index);
                [color.r, color.g, color.b]
            }
        };
        let [r, g, b] = rgb.map(srgb_to_linear);
        [r, g, b, 1.0]
    })
}

fn srgb_to_linear(byte: u8) -> f32 {
    let c = f32::from(byte) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Collects accessors into one BIN buffer and wraps the glTF JSON around it.
#[derive(Default)]
struct GlbWriter {
    bin: Vec<u8>,
    views: Vec<Value>,
    accessors: Vec<Value>,
}

impl GlbWriter {
    /// Adds `data` as a float accessor of `kind`, with its `min`/`max` when
    /// `bounds` (glTF requires them on POSITION); returns its index.
    fn floats<const N: usize>(&mut self, data: &[[f32; N]], kind: &str, bounds: bool) -> usize {
        let view = self.view(data.iter().flatten().flat_map(|value| value.to_le_bytes()));
        let mut accessor = json!({
            "bufferView": view, "componentType": FLOAT, "count": data.len(), "type": kind
        });
        if bounds && !data.is_empty() {
            let mut min = [f32::INFINITY; N];
            let mut max = [f32::NEG_INFINITY; N];
            for element in data {
                for axis in 0..N {
                    min[axis] = min[axis].min(element[axis]);
                    max[axis] = max[axis].max(element[axis]);
                }
            }
            accessor["min"] = json!(min.to_vec());
            accessor["max"] = json!(max.to_vec());
        }
        self.accessors.push(accessor);
        self.accessors.len() - 1
    }

    fn indices(&mut self, data: &[u32]) -> usize {
        let view = self.view(data.iter().flat_map(|value| value.to_le_bytes()));
        self.accessors.push(json!({
            "bufferView": view, "componentType": UNSIGNED_INT, "count": data.len(), "type": "SCALAR"
        }));
        self.accessors.len() - 1
    }

    /// Appends `bytes` as a buffer view, 4-byte aligned; returns its index.
    fn view(&mut self, bytes: impl Iterator<Item = u8>) -> usize {
        let offset = self.bin.len();
        self.bin.extend(bytes);
        self.views.push(json!({
            "buffer": 0, "byteOffset": offset, "byteLength": self.bin.len() - offset
        }));
        while self.bin.len() % 4 != 0 {
            self.bin.push(0);
        }
        self.views.len() - 1
    }

    fn finish(self, mut doc: Value) -> Vec<u8> {
        doc["buffers"] = json!([{ "byteLength": self.bin.len() }]);
        doc["bufferViews"] = Value::Array(self.views);
        doc["accessors"] = Value::Array(self.accessors);
        let mut json = serde_json::to_vec(&doc).unwrap_or_default();
        while json.len() % 4 != 0 {
            json.push(b' ');
        }
        let bin = self.bin;
        let length = 12 + 8 + json.len() + 8 + bin.len();
        let mut out = Vec::with_capacity(length);
        out.extend_from_slice(b"glTF");
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&(length as u32).to_le_bytes());
        out.extend_from_slice(&(json.len() as u32).to_le_bytes());
        out.extend_from_slice(b"JSON");
        out.extend_from_slice(&json);
        out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(&bin);
        out
    }
}

#[cfg(all(test, tests_asset_tools))]
mod tests {
    use super::*;
    use crate::assets::model_file::ModelPart;

    /// The test VXL (2x2x2 grid; voxels at (0,0,0), (0,0,1), (1,1,1)) with
    /// bounds stretching its X two to one: it reads back as a `body` whose
    /// bounds are the voxels' cells in glTF axes, voxel X on +Z, Y on X and
    /// Z on +Y, and each section a primitive of exposed faces only.
    #[test]
    fn a_vxl_exports_as_a_body_the_reader_accepts() {
        let mut vxl = VxlFile::from_bytes(&crate::assets::vxl_file::minimal_test_vxl()).unwrap();
        vxl.limbs[0].bounds = [-1.0, 0.0, 0.0, 3.0, 2.0, 2.0];
        let palette = Palette::from_rgb(|index| [index; 3]);
        let built = build_glb(&[("body", &vxl, None)], &palette);
        let model = Model::from_glb(&built.glb).unwrap();
        assert_eq!(model.part(ModelPart::Body), Some(0));
        assert_eq!(
            model.body_bounds(),
            Some(ModelBounds {
                min: [0.0, 0.0, -1.0],
                max: [2.0, 2.0, 3.0],
            })
        );
        // Two stacked voxels share a face; the third touches neither:
        // 10 + 6 faces of two triangles.
        assert_eq!(built.parts[0].triangles, 32);
        let primitive = &model.meshes[0].primitives[0];
        assert!(primitive.colors.is_some() && primitive.normals.is_some());
        assert_eq!(built.house_voxels, 0);
    }
}
