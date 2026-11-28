//! Shared physical voxel slots, resolved from the retained native model plan.
//! Rules owns reads/generations; this immutable catalog owns parsed libraries.
use super::voxel_model_plan::{LibraryKind, LibraryRequest, ModelPair, ModelSlots, ResolvedPlan};
use crate::assets::{asset_manager::AssetManager, hva_file::HvaFile, vxl_file::VxlFile};
use crate::rules::{
    object_type::ObjectCategory,
    ruleset::{RuleSet, TypeHandle},
};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct VoxelPart {
    vxl: Arc<VxlFile>,
    hva: Option<Arc<HvaFile>>,
    identity: (String, Option<String>),
}
impl VoxelPart {
    /// Explicit image-only loader; rules-backed callers use the catalog below.
    pub fn load(assets: &AssetManager, image: &str, require_hva: bool) -> Option<Self> {
        let (vxl_name, hva_name) = crate::rules::art_data::voxel_asset_names(image);
        let vxl = Arc::new(VxlFile::from_bytes(assets.get_ref(&vxl_name)?).ok()?);
        let hva = assets
            .get_ref(&hva_name)
            .and_then(|bytes| HvaFile::from_bytes(bytes).ok())
            .map(Arc::new);
        if require_hva && hva.is_none() {
            return None;
        }
        let identity = (
            image.to_ascii_uppercase(),
            hva.as_ref().map(|_| image.to_ascii_uppercase()),
        );
        Some(Self { vxl, hva, identity })
    }
    pub fn vxl(&self) -> &VxlFile {
        &self.vxl
    }
    pub fn hva(&self) -> Option<&HvaFile> {
        self.hva.as_deref()
    }
    pub fn identity(&self) -> (&str, Option<&str>) {
        (&self.identity.0, self.identity.1.as_deref())
    }
}

#[derive(Debug, Clone, Default)]
pub struct LoadedVoxelModel {
    main: Option<VoxelPart>,
    auxiliary: Option<VoxelPart>,
    barrel: Option<VoxelPart>,
    indexed: Vec<(Option<VoxelPart>, Option<VoxelPart>)>,
    declared: Option<Arc<DeclaredModel>>,
}
impl LoadedVoxelModel {
    /// VERA-only, experimental (`docs/picture-model-art.md`): the model a
    /// type whose art declares `Model=` draws instead of its VXL parts, which
    /// are then all `None`. Present only when the model was admitted.
    pub fn declared_model(&self) -> Option<&Arc<DeclaredModel>> {
        self.declared.as_ref()
    }
    pub fn main(&self) -> Option<&VoxelPart> {
        self.main.as_ref()
    }
    /// Native B8: turret OR APC-W/NoSpawnAlt-WO, with actual fallback history.
    pub fn auxiliary(&self) -> Option<&VoxelPart> {
        self.auxiliary.as_ref()
    }
    pub fn barrel(&self) -> Option<&VoxelPart> {
        self.barrel.as_ref()
    }
    pub fn indexed_parts(&self, index: usize) -> (Option<&VoxelPart>, Option<&VoxelPart>) {
        self.indexed
            .get(index)
            .map_or((None, None), |(turret, barrel)| {
                (turret.as_ref(), barrel.as_ref())
            })
    }
    pub fn indexed_count(&self) -> usize {
        self.indexed.len()
    }
}

pub fn voxel_tilt_half_sizes(vxl: &VxlFile) -> Option<[f32; 2]> {
    let section = vxl.limbs.first()?;
    Some(tilt_half_sizes(section.size_x, section.size_y))
}

/// `TechnoTypeClass+0x360`/`+0x368` from the main section's sizes in voxels:
/// `size_x` along the hull's forward axis, `size_y` across it.
fn tilt_half_sizes(size_x: u8, size_y: u8) -> [f32; 2] {
    [f32::from(size_y) * 0.5 + 0.5, f32::from(size_x) * 0.5]
}

/// VERA-only, experimental (`docs/picture-model-art.md`): the tilt half sizes
/// of a declared model, as [`voxel_tilt_half_sizes`] takes them from a VXL.
///
/// No native constant relates a glTF model to voxels, so the model is first
/// given the VXL section sizes it stands in for: `ModelSize=` leptons per body
/// length along glTF +Z (forward), the same scale across glTF X, converted to
/// voxels with the native model units per lepton (`0x00B1D008`,
/// [`crate::util::lepton::MODEL_UNITS_PER_LEPTON_BITS`]; one voxel is one
/// model unit, as a section whose bounds span its size draws), rounded to
/// whole voxels and held to a size byte's 1..=255:
///
/// `size_x = round(ModelSize * u)`,
/// `size_y = round(width / length * ModelSize * u)`, `u = 0.16573…`.
///
/// The arithmetic is IEEE binary64 and rounds to integers, so every platform
/// gets the same sizes. `None` when the body has no forward length.
pub fn model_tilt_half_sizes(
    bounds: crate::assets::model_file::ModelBounds,
    model_size: i32,
) -> Option<[f32; 2]> {
    let length = f64::from(bounds.max[2]) - f64::from(bounds.min[2]);
    let width = f64::from(bounds.max[0]) - f64::from(bounds.min[0]);
    if !(length.is_finite() && width.is_finite() && length > 0.0) {
        return None;
    }
    let voxels_per_lepton = f64::from_bits(crate::util::lepton::MODEL_UNITS_PER_LEPTON_BITS);
    let voxels = |leptons: f64| (leptons * voxels_per_lepton).round().clamp(1.0, 255.0) as u8;
    let model_size = f64::from(model_size);
    Some(tilt_half_sizes(
        voxels(model_size),
        voxels(width / length * model_size),
    ))
}

/// VERA-only, experimental (`docs/picture-model-art.md`): an admitted
/// `Model=` of one art section, decoded once at asset binding.
#[derive(Debug)]
pub struct DeclaredModel {
    /// The art section that declares it.
    pub section: String,
    pub decl: crate::rules::art_data::ModelDecl,
    pub model: Arc<crate::assets::model_file::Model>,
    /// The tilt half sizes the simulation takes: `ModelVoxels=`'s as a VXL's
    /// ([`voxel_tilt_half_sizes`]), else [`model_tilt_half_sizes`].
    pub half_sizes: [f32; 2],
    /// FNV-1a of the file's bytes: the catalog hashes it with the file name,
    /// so a machine with another file is refused online.
    digest: u64,
    /// The model drawn where the VXL path draws `%sWO` (`NoSpawnAlt=`, its
    /// spawns away): `NAMEWO.glb` beside `Model=NAME.glb`, scaled so its own
    /// body is `ModelSize=` long. None when the type has no `NoSpawnAlt=` or
    /// the file is missing or unreadable; the main model then draws.
    /// Drawing only: the simulation takes nothing from it, so it is not
    /// hashed.
    pub no_spawn_alt: Option<Arc<DeclaredModel>>,
}

impl DeclaredModel {
    /// A declared model of `ModelSize=size` for tests of its drawing.
    #[cfg(any(test, feature = "test-support"))]
    pub fn for_test(model: Arc<crate::assets::model_file::Model>, size: i32) -> Self {
        let half_sizes = model
            .body_bounds()
            .and_then(|bounds| model_tilt_half_sizes(bounds, size))
            .unwrap_or_default();
        Self {
            section: "TEST".into(),
            decl: crate::rules::art_data::ModelDecl {
                file: "test.glb".into(),
                size,
                voxels: None,
            },
            model,
            half_sizes,
            digest: 0,
            no_spawn_alt: None,
        }
    }
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// VERA-only, experimental (`docs/picture-model-art.md`): read `decl` of art
/// section `section` for `object`, standing in for ObjectType LoadVoxel
/// `5F8110`. The model is admitted when the file decodes, has a `body` node
/// with a forward length and, for a unit that loads indexed guns
/// (`5F865F`), a `turretN` node for each of its first `TurretCount=` guns
/// (at most 18): a missing one fails the whole load, as a missing indexed
/// TUR VXL clears the main model at `5F8ACB`. Barrels and an ordinary
/// turret are optional, as their VXL files are. Refusals are logged.
/// `object` is None for art that is not a type's own voxel body (building
/// guns, voxel projectiles, debris), which needs no indexed nodes.
fn admit_declared_model(
    object: Option<&crate::rules::object_type::ObjectType>,
    section: &str,
    decl: &crate::rules::art_data::ModelDecl,
    decoded: &mut BTreeMap<String, Option<(Arc<crate::assets::model_file::Model>, u64)>>,
    assets: &AssetManager,
) -> Option<DeclaredModel> {
    use crate::assets::model_file::{Model, ModelPart};
    let (model, digest) = decoded
        .entry(decl.file.to_ascii_uppercase())
        .or_insert_with(|| {
            let Some(bytes) = assets.get_ref(&decl.file) else {
                log::warn!(
                    "[{section}] Model={} is not found; it draws nothing",
                    decl.file
                );
                crate::rules::art_problems::report(
                    section,
                    &format!("Model={}", decl.file),
                    "the file is not found",
                    "it draws nothing",
                );
                return None;
            };
            match Model::from_glb(bytes) {
                Ok(model) => Some((Arc::new(model), fnv1a64(bytes))),
                Err(error) => {
                    log::warn!(
                        "[{section}] Model={} cannot be read ({error}); it draws nothing",
                        decl.file
                    );
                    crate::rules::art_problems::report(
                        section,
                        &format!("Model={}", decl.file),
                        &format!("cannot be read ({error})"),
                        "it draws nothing",
                    );
                    None
                }
            }
        })
        .clone()?;
    // `ModelVoxels=` gives the VXL's own section sizes; else they follow
    // the model's body at `ModelSize=`.
    let Some(half_sizes) = model.body_bounds().and_then(|bounds| match decl.voxels {
        Some([size_x, size_y]) => Some(tilt_half_sizes(size_x, size_y)),
        None => model_tilt_half_sizes(bounds, decl.size),
    }) else {
        log::warn!(
            "[{section}] Model={} has no body node with a forward length; it draws nothing",
            decl.file
        );
        crate::rules::art_problems::report(
            section,
            &format!("Model={}", decl.file),
            "has no body node with a forward length",
            "it draws nothing",
        );
        return None;
    };
    let indexed = object.filter(|object| {
        object.category == ObjectCategory::Vehicle
            && object.has_turret
            && object.turret_count > 0
            && !object.is_gattling
    });
    if let Some(object) = indexed {
        let count = object.turret_count.clamp(0, 18) as u32;
        if let Some(missing) = (0..count).find(|&i| model.part(ModelPart::Turret(i)).is_none()) {
            log::warn!(
                "[{section}] Model={} has no turret{missing} node for TurretCount={}; \
                 it draws nothing",
                decl.file,
                object.turret_count
            );
            crate::rules::art_problems::report(
                section,
                &format!("Model={}", decl.file),
                &format!(
                    "has no turret{missing} node for TurretCount={}",
                    object.turret_count
                ),
                "it draws nothing",
            );
            return None;
        }
    }
    let no_spawn_alt = object
        .is_some_and(|object| object.category == ObjectCategory::Vehicle && object.no_spawn_alt())
        .then(|| no_spawn_alt_model(section, decl, half_sizes, assets))
        .flatten()
        .map(Arc::new);
    Some(DeclaredModel {
        section: section.to_owned(),
        decl: decl.clone(),
        model,
        half_sizes,
        digest,
        no_spawn_alt,
    })
}

/// VERA-only, experimental (`docs/picture-model-art.md`): the `NoSpawnAlt=`
/// model of `decl`, `NAMEWO.glb` beside `Model=NAME.glb`, standing in for
/// the `%sWO` VXL the stock loader reads (`0x005F8277`). Read only for a
/// unit with `NoSpawnAlt=yes` whose art declares `Model=`. A missing file is
/// no fault, as a missing WO pair is not to the stock loader; an unreadable
/// one, or one without a `body` node, is told and the main model draws.
fn no_spawn_alt_model(
    section: &str,
    decl: &crate::rules::art_data::ModelDecl,
    half_sizes: [f32; 2],
    assets: &AssetManager,
) -> Option<DeclaredModel> {
    let file = match decl.file.rsplit_once('.') {
        Some((stem, extension)) => format!("{stem}WO.{extension}"),
        None => format!("{}WO", decl.file),
    };
    let bytes = assets.get_ref(&file)?;
    let model = match crate::assets::model_file::Model::from_glb(bytes) {
        Ok(model) if model.body_bounds().is_some() => model,
        Ok(_) => {
            log::warn!("[{section}] {file} has no body node; the main model draws");
            crate::rules::art_problems::report(
                section,
                &file,
                "has no body node",
                "the main model draws",
            );
            return None;
        }
        Err(error) => {
            log::warn!("[{section}] {file} cannot be read ({error}); the main model draws");
            crate::rules::art_problems::report(
                section,
                &file,
                &format!("cannot be read ({error})"),
                "the main model draws",
            );
            return None;
        }
    };
    Some(DeclaredModel {
        section: section.to_owned(),
        decl: crate::rules::art_data::ModelDecl {
            file,
            size: decl.size,
            voxels: decl.voxels,
        },
        model: Arc::new(model),
        half_sizes,
        digest: fnv1a64(bytes),
        no_spawn_alt: None,
    })
}

/// VERA-only, experimental (`docs/picture-model-art.md`): `decl` of art
/// section `section` for art that is not a type's own voxel body, admitted
/// once per section ([`admit_declared_model`]) and shared by every building
/// gun, projectile and debris type that reads the section.
fn admit_section_model(
    section: &str,
    decl: &crate::rules::art_data::ModelDecl,
    by_section: &mut BTreeMap<String, Option<Arc<DeclaredModel>>>,
    decoded: &mut BTreeMap<String, Option<(Arc<crate::assets::model_file::Model>, u64)>>,
    assets: &AssetManager,
) -> Option<Arc<DeclaredModel>> {
    by_section
        .entry(section.to_ascii_uppercase())
        .or_insert_with(|| admit_declared_model(None, section, decl, decoded, assets).map(Arc::new))
        .clone()
}

/// VERA-only, experimental (`docs/picture-model-art.md`): the art section
/// whose `Model=` stands in for a building's voxel guns, the B8 turret and
/// C0 barrel BuildingType `45FA90` loads (names from `464438`/`46467E`,
/// [`super::voxel_frame_catalog::building_voxel_read_names`]): the
/// `TurretAnim=` section; else, for a `BarrelAnimIsVoxel=` building whose
/// `TurretAnim=` holds no `TUR` (its C0 is `VoxelBarrelFile=`), the
/// building's own. The model's `body` stands in for the turret and its
/// `barrel` for the barrel.
fn building_gun_model_decl<'a>(
    rules: &'a RuleSet,
    object: &crate::rules::object_type::ObjectType,
) -> Option<(&'a str, &'a crate::rules::art_data::ModelDecl)> {
    if object.category != ObjectCategory::Building
        || !(object.turret_anim_is_voxel || object.barrel_anim_is_voxel)
    {
        return None;
    }
    let turret_anim = object.turret_anim.as_deref().unwrap_or("").trim();
    if !turret_anim.is_empty()
        && let Some(found) = rules.art().resolve_declared_model(turret_anim, turret_anim)
    {
        return Some(found);
    }
    let (turret, _) = super::voxel_frame_catalog::building_voxel_pair_names(turret_anim, None);
    if turret.is_none() && object.barrel_anim_is_voxel {
        rules
            .art()
            .resolve_declared_model(&object.id, &object.image)
    } else {
        None
    }
}

/// VERA-only, experimental (`docs/picture-model-art.md`): the declared
/// model a debris type (`[VoxelAnims]`) draws in place of its VXL.
#[derive(Debug, Clone)]
pub struct DebrisModel {
    /// None when the model was refused: the piece draws nothing.
    pub model: Option<Arc<DeclaredModel>>,
    /// The part a `ShareSource=` type draws from its source's model (`body`,
    /// `turret` or `barrel`, as `ShareBodyData=`, `ShareTurretData=` or
    /// `ShareBarrelData=` selects, VoxelAnim `74B4D0`); None draws the
    /// `VoxelIndex=`th mesh node of the type's own model.
    pub shared: Option<crate::assets::model_file::ModelPart>,
}

/// What a debris type that shares another's data (`+0x296`, [`super::voxel_anim_type::VoxelAnimType::shares_data`])
/// draws: VoxelAnim ReadINI `74B4D0..74B55B` copies the `ShareSource=`
/// type's B0 (`ShareBodyData=`), else B8 (`ShareTurretData=`), else C0
/// pair into its own B0, found among Infantry, Vehicle, Aircraft and
/// Building types in that order; a restored game reloads the type's own
/// image instead (`74B8BB`). The type's main slot tells which: None when it
/// holds its own image's load, so the type draws as one that shares
/// nothing; `Some(None)` when it holds the source's pair and the source
/// declares no model for that part (the shared VXL draws), or holds nothing;
/// else the source's model and the part that stands in for the pair (a
/// building's guns: `body` for B8, `barrel` for C0, as
/// [`building_gun_model_decl`] maps them).
fn shared_debris_model(
    rules: &RuleSet,
    kind: &super::voxel_anim_type::VoxelAnimType,
    declared: &BTreeMap<(ObjectCategory, String), Option<Arc<DeclaredModel>>>,
    building_guns: &BTreeMap<String, Option<Arc<DeclaredModel>>>,
) -> Option<Option<DebrisModel>> {
    use crate::assets::model_file::ModelPart;
    if !kind.shares_data() {
        return None;
    }
    let main = kind.model_slots.main;
    // Rules processing copied the source's pair into the main slot
    // (`74B4D0..74B55B`) and keeps no name: the source is the first type, in
    // the order ShareSource= is looked up, whose shared slot holds that pair.
    let slot = if kind.share_body_data {
        0
    } else if kind.share_turret_data {
        1
    } else {
        2
    };
    let shared_pair = |object: &crate::rules::object_type::ObjectType| {
        let slots = &object.voxel_model_slots;
        [slots.main, slots.auxiliary, slots.barrel][slot]
    };
    if main == ModelPair::default() {
        // No source, or one without that part: the copied pair is null
        // (74B555) and nothing draws; a restored game holds its own image.
        return Some(None);
    }
    let source = [
        ObjectCategory::Infantry,
        ObjectCategory::Vehicle,
        ObjectCategory::Aircraft,
        ObjectCategory::Building,
    ]
    .into_iter()
    .find_map(|category| {
        rules
            .all_objects()
            .find(|object| object.category == category && shared_pair(object) == main)
    });
    let Some(source) = source else {
        // The pair is the type's own image's load: it draws as one that
        // shares nothing.
        return None;
    };
    let id = source.id.to_ascii_uppercase();
    let found = match (source.category, slot) {
        (ObjectCategory::Building, 1 | 2) => building_guns.get(&id).map(|model| {
            (
                model,
                if slot == 1 {
                    ModelPart::Body
                } else {
                    ModelPart::Barrel(0)
                },
            )
        }),
        _ => declared.get(&(source.category, id)).map(|model| {
            let part = match slot {
                0 => ModelPart::Body,
                1 => ModelPart::Turret(0),
                _ => ModelPart::Barrel(0),
            };
            (model, part)
        }),
    };
    Some(found.map(|(model, part)| DebrisModel {
        model: model.clone(),
        shared: Some(part),
    }))
}

/// The catalog-hash entry of a declared model: its file, `ModelSize=` and
/// byte digest, so a machine with another file hashes differently.
fn model_identity(declared: &DeclaredModel) -> Option<(String, Option<String>)> {
    Some((
        format!("MODEL:{}", declared.decl.file.to_ascii_uppercase()),
        Some(format!("{}:{:016x}", declared.decl.size, declared.digest)),
    ))
}

/// One native model resolver for gameplay, UnitModel and metadata consumers.
/// A rules-backed request never reconstructs a slot from the current filename.
pub fn load_voxel_model(
    assets: &AssetManager,
    type_id: &str,
    rules: Option<&RuleSet>,
) -> Option<LoadedVoxelModel> {
    if let Some(rules) = rules
        && let Some(object) = rules.object(type_id)
    {
        if rules.voxel_models_bound() {
            return rules.loaded_voxel_model(type_id).cloned();
        }
        return VoxelAssetCatalog::bind(rules, assets)
            .models
            .get(&(object.category, object.id.to_ascii_uppercase()))
            .cloned();
    }
    Some(LoadedVoxelModel {
        main: Some(VoxelPart::load(assets, type_id, false)?),
        ..Default::default()
    })
}

#[derive(Debug)]
enum Library {
    Vxl(Arc<VxlFile>),
    Hva(Arc<HvaFile>),
}
#[derive(Debug)]
struct LibraryRead {
    present: bool,
    library: Option<Library>,
    /// VERA: the game-folder file that supplied it, when no shipped archive
    /// does (`retail_sources::FolderFiles`), and how it stands to the copy a
    /// retail install reads: whether one of the two is there or reads and
    /// the other not, and whether a VXL's first limb has other half sizes.
    folder_file: Option<String>,
    validity_differs: bool,
    half_sizes_differ: bool,
}

fn part(
    pair: ModelPair,
    plan: &ResolvedPlan<'_>,
    libraries: &BTreeMap<LibraryRequest, LibraryRead>,
) -> Option<VoxelPart> {
    let vxl_request = plan.library(pair.vxl)?;
    let Library::Vxl(vxl) = libraries.get(vxl_request)?.library.as_ref()? else {
        return None;
    };
    let hva_request = plan.library(pair.hva);
    let hva = hva_request.and_then(|request| match libraries.get(request)?.library.as_ref()? {
        Library::Hva(hva) => Some(Arc::clone(hva)),
        _ => None,
    });
    Some(VoxelPart {
        vxl: Arc::clone(vxl),
        identity: (
            vxl_request.image.clone(),
            hva.as_ref().and(hva_request.map(|r| r.image.clone())),
        ),
        hva,
    })
}
fn model(
    slots: &ModelSlots,
    plan: &ResolvedPlan<'_>,
    libraries: &BTreeMap<LibraryRequest, LibraryRead>,
) -> LoadedVoxelModel {
    LoadedVoxelModel {
        main: part(slots.main, plan, libraries),
        auxiliary: part(slots.auxiliary, plan, libraries),
        barrel: part(slots.barrel, plan, libraries),
        indexed: slots
            .indexed
            .iter()
            .map(|gun| {
                (
                    part(gun.turret, plan, libraries),
                    part(gun.barrel, plan, libraries),
                )
            })
            .collect(),
        declared: None,
    }
}

#[derive(Debug, Default, Clone)]
pub(crate) struct VoxelAssetCatalog {
    bound: bool,
    loaded: BTreeMap<String, Option<[u32; 2]>>,
    by_handle: Vec<bool>,
    models: BTreeMap<(ObjectCategory, String), LoadedVoxelModel>,
    voxel_anims: Vec<Option<VoxelPart>>,
    /// Whether any type draws an admitted declared model, so the renderer
    /// skips its model work outright when none does. Derived from `models`
    /// and the three maps below.
    declared_models: bool,
    /// VERA-only, experimental (`docs/picture-model-art.md`): building
    /// types, by upper-case ID, whose voxel guns' art declares `Model=`
    /// ([`building_gun_model_decl`]); None when it was refused, so the guns
    /// draw nothing. Their VXL and HVA are not opened. Empty for retail art.
    building_guns: BTreeMap<String, Option<Arc<DeclaredModel>>>,
    /// The same for `Voxel=yes` projectile types whose `Image=` art declares
    /// `Model=`, by upper-case ID. Bullet draw `46B0C0` reads their VXL by
    /// name, outside the plan, so nothing here is hashed.
    projectiles: BTreeMap<String, Option<Arc<DeclaredModel>>>,
    /// The same for VoxelAnim types, by registry index, whose own `Image=`
    /// art declares `Model=`, or that share a part of a type whose art
    /// declares one ([`shared_debris_model`]).
    voxel_anim_models: BTreeMap<usize, DebrisModel>,
    /// VERA: the game-folder files that change which main models load or a
    /// main VXL's half sizes: a library that is there or reads where the
    /// retail copy does not (or the reverse), whatever part it is, as one
    /// bad part unloads the whole model; a main VXL of other half sizes;
    /// and a declared `Model=` whose load or half sizes are not the retail
    /// main VXL's (`FolderFiles::note_stand_in`). Not hashed or compared.
    folder_files: BTreeSet<String>,
}
impl PartialEq for VoxelAssetCatalog {
    fn eq(&self, other: &Self) -> bool {
        self.loaded == other.loaded && self.identities() == other.identities()
    }
}
impl Eq for VoxelAssetCatalog {}
impl std::hash::Hash for VoxelAssetCatalog {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.loaded.hash(state);
        self.identities().hash(state);
    }
}
impl VoxelAssetCatalog {
    fn identities(&self) -> Vec<(String, Vec<Option<(String, Option<String>)>>)> {
        let part = |p: Option<&VoxelPart>| p.map(|p| p.identity.clone());
        let mut result: Vec<_> = self
            .models
            .iter()
            .map(|((category, name), model)| {
                let mut parts = vec![
                    part(model.main()),
                    part(model.auxiliary()),
                    part(model.barrel()),
                ];
                for i in 0..model.indexed_count() {
                    let (t, b) = model.indexed_parts(i);
                    parts.extend([part(t), part(b)]);
                }
                // Only a declared model adds an entry, so a catalog without
                // one hashes as it did before (docs/picture-model-art.md).
                if let Some(declared) = model.declared_model() {
                    parts.push(model_identity(declared));
                }
                if *category == ObjectCategory::Building
                    && let Some(Some(declared)) = self.building_guns.get(name)
                {
                    parts.push(model_identity(declared));
                }
                (format!("{category:?}:{name}"), parts)
            })
            .collect();
        result.extend(self.voxel_anims.iter().enumerate().map(|(i, p)| {
            let mut parts = vec![part(p.as_ref())];
            if let Some(declared) = self
                .voxel_anim_models
                .get(&i)
                .and_then(|d| d.model.as_ref())
            {
                parts.push(model_identity(declared));
            }
            (format!("VoxelAnim:{i}"), parts)
        }));
        result
    }
    pub(crate) fn bind(rules: &RuleSet, assets: &AssetManager) -> Self {
        // VERA-only, experimental (docs/picture-model-art.md): a voxel type
        // whose image's art section declares `Model=` takes its parts from
        // the model, admitted or not, and the VXL and HVA files only such
        // types would read are never opened. Retail art declares none, so
        // this map stays empty and nothing below changes.
        let mut decoded = BTreeMap::new();
        let mut declared: BTreeMap<(ObjectCategory, String), Option<Arc<DeclaredModel>>> =
            BTreeMap::new();
        // Chain 5: building guns, voxel projectiles and debris whose art
        // declares `Model=`, each section admitted once.
        let mut by_section = BTreeMap::new();
        let mut building_guns: BTreeMap<String, Option<Arc<DeclaredModel>>> = BTreeMap::new();
        let folder = crate::rules::retail_sources::FolderFiles::new(assets);
        let half_sizes =
            |vxl: &VxlFile| voxel_tilt_half_sizes(vxl).map(|sizes| sizes.map(f32::to_bits));
        // VERA: the models whose load or tilt half sizes (what `loaded`
        // takes) are not the retail main VXL's.
        let mut model_files = BTreeSet::new();
        for object in rules.all_objects() {
            if let Some((section, decl)) = building_gun_model_decl(rules, object) {
                let admitted =
                    admit_section_model(section, decl, &mut by_section, &mut decoded, assets);
                building_guns.insert(object.id.to_ascii_uppercase(), admitted);
            }
            let Some(image) = object.voxel_image_load() else {
                continue;
            };
            let Some((section, decl)) = rules.art().resolve_declared_model(&object.id, image)
            else {
                continue;
            };
            let admitted = admit_declared_model(Some(object), section, decl, &mut decoded, assets);
            folder.note_stand_in(
                &decl.file,
                admitted
                    .as_ref()
                    .map(|model| model.half_sizes.map(f32::to_bits)),
                [crate::rules::art_data::voxel_asset_names(image).0],
                |bytes| half_sizes(&VxlFile::from_bytes(bytes).ok()?),
                &mut model_files,
            );
            declared.insert(
                (object.category, object.id.to_ascii_uppercase()),
                admitted.map(Arc::new),
            );
        }
        let mut voxel_anim_models: BTreeMap<usize, DebrisModel> = BTreeMap::new();
        for (index, kind) in rules.voxel_anim_types_iter().enumerate() {
            let own = rules.art().resolve_declared_model(&kind.name, &kind.image);
            if kind.shares_data() {
                // A type that shares a part draws the source's model of it,
                // or keeps the shared VXL (`Some(None)`). Only a declared
                // model anywhere makes it look for its source, so retail
                // art walks nothing here.
                let any = own.is_some() || !declared.is_empty() || !building_guns.is_empty();
                match any.then(|| shared_debris_model(rules, kind, &declared, &building_guns)) {
                    Some(None) => {}
                    Some(Some(Some(shared))) => {
                        voxel_anim_models.insert(index, shared);
                        continue;
                    }
                    None | Some(Some(None)) => continue,
                }
            }
            if let Some((section, decl)) = own {
                let admitted =
                    admit_section_model(section, decl, &mut by_section, &mut decoded, assets);
                voxel_anim_models.insert(
                    index,
                    DebrisModel {
                        model: admitted,
                        shared: None,
                    },
                );
            }
        }
        let mut projectiles: BTreeMap<String, Option<Arc<DeclaredModel>>> = BTreeMap::new();
        for kind in rules.projectiles_iter().filter(|kind| kind.voxel) {
            let Some((section, decl)) = kind
                .voxel_image_load()
                .and_then(|image| rules.art().resolve_declared_model(&kind.id, image))
            else {
                continue;
            };
            let admitted =
                admit_section_model(section, decl, &mut by_section, &mut decoded, assets);
            projectiles.insert(kind.id.to_ascii_uppercase(), admitted);
        }
        let unopened: BTreeSet<LibraryRequest> =
            if declared.is_empty() && building_guns.is_empty() && voxel_anim_models.is_empty() {
                BTreeSet::new()
            } else {
                let is_declared = |object: &&crate::rules::object_type::ObjectType| {
                    let id = object.id.to_ascii_uppercase();
                    (object.category == ObjectCategory::Building && building_guns.contains_key(&id))
                        || declared.contains_key(&(object.category, id))
                };
                let anim_models = &voxel_anim_models;
                let anims = |declared: bool| {
                    rules
                        .voxel_anim_types_iter()
                        .enumerate()
                        .filter(move |(index, _)| anim_models.contains_key(index) == declared)
                        .map(|(_, kind)| &kind.model_slots)
                };
                let mine = rules.voxel_model_plan.slot_requests(
                    rules
                        .all_objects()
                        .filter(|object| is_declared(object))
                        .map(|object| &object.voxel_model_slots)
                        .chain(anims(true)),
                );
                let others = rules.voxel_model_plan.slot_requests(
                    rules
                        .all_objects()
                        .filter(|object| !is_declared(object))
                        .map(|object| &object.voxel_model_slots)
                        .chain(anims(false)),
                );
                mine.difference(&others).cloned().collect()
            };
        let mut libraries = BTreeMap::new();
        let requests: BTreeSet<_> = rules.voxel_model_plan.requests().cloned().collect();
        for request in requests {
            if unopened.contains(&request) {
                continue;
            }
            let (vxl, hva) = crate::rules::art_data::voxel_asset_names(&request.image);
            let name = match request.kind {
                LibraryKind::Vxl => vxl,
                LibraryKind::Hva => hva,
            };
            let (bytes, folder_file) = folder.read(&name).unzip();
            let folder_file = folder_file.flatten();
            let library = bytes.and_then(|bytes| match request.kind {
                LibraryKind::Vxl => VxlFile::from_bytes(bytes)
                    .ok()
                    .map(|v| Library::Vxl(Arc::new(v))),
                LibraryKind::Hva => HvaFile::from_bytes(bytes)
                    .ok()
                    .map(|v| Library::Hva(Arc::new(v))),
            });
            // Only a game-folder file is held against the retail copy.
            let (validity_differs, half_sizes_differ) = if folder_file.is_some() {
                let shipped = folder.shipped(&name);
                let (shipped_valid, shipped_sizes) = match request.kind {
                    LibraryKind::Vxl => {
                        let vxl = shipped.and_then(|bytes| VxlFile::from_bytes(bytes).ok());
                        (vxl.is_some(), vxl.as_ref().and_then(half_sizes))
                    }
                    LibraryKind::Hva => (
                        shipped.is_some_and(|bytes| HvaFile::from_bytes(bytes).is_ok()),
                        None,
                    ),
                };
                let sizes = match &library {
                    Some(Library::Vxl(vxl)) => half_sizes(vxl),
                    _ => None,
                };
                (
                    (bytes.is_some(), library.is_some()) != (shipped.is_some(), shipped_valid),
                    sizes != shipped_sizes,
                )
            } else {
                (false, false)
            };
            libraries.insert(
                request,
                LibraryRead {
                    present: bytes.is_some(),
                    library,
                    folder_file,
                    validity_differs,
                    half_sizes_differ,
                },
            );
        }
        let plan = rules.voxel_model_plan.resolve(|request, valid| {
            libraries.get(request).is_some_and(|read| {
                if valid {
                    read.library.is_some()
                } else {
                    read.present
                }
            })
        });
        let models: BTreeMap<_, _> = rules
            .all_objects()
            .map(|object| {
                let key = (object.category, object.id.to_ascii_uppercase());
                let loaded = match declared.get(&key) {
                    Some(declared) => LoadedVoxelModel {
                        declared: declared.clone(),
                        ..Default::default()
                    },
                    // A building whose guns are a declared model loads no
                    // gun VXL; the model is not a main model, so IsVoxel
                    // (6F9E10) and the tilt sizes stay as without one.
                    None if object.category == ObjectCategory::Building
                        && building_guns.contains_key(&key.1) =>
                    {
                        LoadedVoxelModel::default()
                    }
                    None => model(&object.voxel_model_slots, &plan, &libraries),
                };
                (key, loaded)
            })
            .collect();
        let by_handle = rules
            .all_objects()
            .map(|object| {
                let model = &models[&(object.category, object.id.to_ascii_uppercase())];
                model.main().is_some() || model.declared_model().is_some()
            })
            .collect();
        let loaded = rules
            .all_objects()
            .filter_map(|object| {
                let model = &models[&(object.category, object.id.to_ascii_uppercase())];
                let half_sizes = match model.declared_model() {
                    Some(declared) => Some(declared.half_sizes),
                    None => voxel_tilt_half_sizes(model.main()?.vxl()),
                };
                Some((
                    object.id.to_ascii_uppercase(),
                    half_sizes.map(|v| v.map(f32::to_bits)),
                ))
            })
            .collect();
        let voxel_anims = rules
            .voxel_anim_types_iter()
            .enumerate()
            .map(|(index, kind)| {
                if voxel_anim_models.contains_key(&index) {
                    None
                } else {
                    part(kind.model_slots.main, &plan, &libraries)
                }
            })
            .collect();
        let declared_models = models
            .values()
            .any(|model| model.declared_model().is_some())
            || building_guns
                .values()
                .chain(projectiles.values())
                .chain(voxel_anim_models.values().map(|debris| &debris.model))
                .any(Option::is_some);
        // Only a type's main model reaches the simulation (`loaded`), and
        // of its files only the VXL's half sizes.
        let main_vxls: BTreeSet<&LibraryRequest> = rules
            .all_objects()
            .filter_map(|object| plan.library(object.voxel_model_slots.main.vxl))
            .collect();
        let mut folder_files: BTreeSet<String> = libraries
            .iter()
            .filter(|(request, read)| {
                read.validity_differs || (read.half_sizes_differ && main_vxls.contains(request))
            })
            .filter_map(|(_, read)| read.folder_file.clone())
            .collect();
        folder_files.extend(model_files);
        Self {
            bound: true,
            loaded,
            by_handle,
            models,
            voxel_anims,
            declared_models,
            building_guns,
            projectiles,
            voxel_anim_models,
            folder_files,
        }
    }
    /// VERA: the game-folder files that change the types' main models.
    pub(crate) fn folder_files(&self) -> impl Iterator<Item = &str> {
        self.folder_files.iter().map(String::as_str)
    }
    pub(crate) fn is_bound(&self) -> bool {
        self.bound
    }
    pub(crate) fn declares_models(&self) -> bool {
        self.declared_models
    }

    /// VERA: which types' main models loaded and their first limb's half
    /// sizes, without which image supplied each part: what
    /// `RuleSet::file_inputs_hash` folds of this catalog.
    pub(crate) fn fold_loaded(&self, state: &mut impl std::hash::Hasher) {
        std::hash::Hash::hash(&self.loaded, state);
    }
    pub(crate) fn model(&self, category: ObjectCategory, name: &str) -> Option<&LoadedVoxelModel> {
        self.models.get(&(category, name.to_ascii_uppercase()))
    }
    pub(crate) fn voxel_anim(&self, index: usize) -> Option<&VoxelPart> {
        self.voxel_anims.get(index)?.as_ref()
    }
    /// VERA-only, experimental (`docs/picture-model-art.md`): the declared
    /// model of building `type_id`'s voxel guns. None when its art declares
    /// none (the VXL path); `Some(None)` when it was refused (nothing draws).
    pub(crate) fn building_gun_model(&self, type_id: &str) -> Option<Option<&Arc<DeclaredModel>>> {
        if self.building_guns.is_empty() {
            return None;
        }
        crate::util::native_string::with_ascii_uppercase(type_id, |key| {
            self.building_guns.get(key).map(Option::as_ref)
        })
    }
    /// [`Self::building_gun_model`] for a voxel projectile type.
    pub(crate) fn projectile_model(&self, type_id: &str) -> Option<Option<&Arc<DeclaredModel>>> {
        if self.projectiles.is_empty() {
            return None;
        }
        crate::util::native_string::with_ascii_uppercase(type_id, |key| {
            self.projectiles.get(key).map(Option::as_ref)
        })
    }
    /// The declared model the VoxelAnim type at `index` draws in place of
    /// its VXL, and what of it ([`DebrisModel`]); None for the VXL path.
    pub(crate) fn voxel_anim_declared_model(&self, index: usize) -> Option<&DebrisModel> {
        self.voxel_anim_models.get(&index)
    }
    pub(crate) fn contains(&self, object: TypeHandle) -> bool {
        self.by_handle
            .get(object.0 as usize)
            .copied()
            .unwrap_or(false)
    }
    pub(crate) fn half_sizes(&self, type_id: &str) -> Option<[f32; 2]> {
        crate::util::native_string::with_ascii_uppercase(type_id, |key| {
            self.loaded.get(key)?.map(|bits| bits.map(f32::from_bits))
        })
    }
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn supply_loaded(&mut self, type_id: &str, object: TypeHandle) {
        self.loaded.insert(type_id.to_ascii_uppercase(), None);
        let index = object.0 as usize;
        if self.by_handle.len() <= index {
            self.by_handle.resize(index + 1, false);
        }
        self.by_handle[index] = true;
    }
}

#[cfg(all(test, tests_rules_o_z))]
mod tests {
    use super::*;
    use crate::rules::{art_data::ArtRegistry, ini_parser::IniFile};

    fn body_rules(voxel: bool) -> RuleSet {
        let art = IniFile::from_str(&format!("[BODY]\nVoxel={voxel}\n"));
        let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
            &IniFile::from_str("[VehicleTypes]\n0=TEST\n[TEST]\nImage=BODY\nStrength=300\n"),
            &art,
        )
        .unwrap();
        rules.install_art_fixture(ArtRegistry::from_ini(&art));
        rules
    }

    fn loaded(rules: &RuleSet, type_id: &str) -> bool {
        rules
            .type_handle(type_id)
            .is_some_and(|object| rules.object_has_loaded_voxel(object))
    }

    /// A type whose image's art declares `Model=` is loaded from its glb,
    /// with tilt half sizes from the body's bounds and `ModelSize=`
    /// (docs/picture-model-art.md); `Model=` without `ModelSize=` is refused
    /// and that section keeps its VXL path; a type without the key keeps its
    /// VXL path, which here finds no file.
    #[test]
    fn a_declared_model_is_loaded_with_tilt_sizes_from_its_body() {
        let directory = crate::map::source::test_support::TestDirectory::new("declared-model");
        let glb = crate::assets::model_file::test_glb::box_model(2.0, 1.0, 4.0, &[]);
        std::fs::write(directory.path().join("tank.glb"), &glb).unwrap();
        let assets = AssetManager::from_loose_root_for_test(directory.path());
        let art = IniFile::from_str(
            "[BODY]\nVoxel=yes\nModel=tank.glb\nModelSize=120\n\
             [NOSIZE]\nVoxel=yes\nModel=tank.glb\n[PLAIN]\nVoxel=yes\n",
        );
        let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
            &IniFile::from_str(
                "[VehicleTypes]\n0=TEST\n1=NOSIZE\n2=PLAIN\n[TEST]\nImage=BODY\nStrength=300\n\
                 [NOSIZE]\nStrength=300\n[PLAIN]\nStrength=300\n",
            ),
            &art,
        )
        .unwrap();
        rules.install_art_fixture(ArtRegistry::from_ini(&art));
        rules.bind_effect_assets(&assets, "TEM", "TEMPERATE");

        assert!(loaded(&rules, "TEST"));
        // 120 leptons are round(19.89) = 20 voxels long; the body is half as
        // wide, round(9.94) = 10 voxels: [10 / 2 + 0.5, 20 / 2].
        assert_eq!(rules.object_voxel_half_sizes("TEST"), Some([5.5, 10.0]));
        let model = rules.loaded_voxel_model("TEST").unwrap();
        assert!(model.main().is_none(), "no VXL part");
        assert_eq!(model.declared_model().unwrap().decl.size, 120);
        assert!(!rules.art().declares_model("NOSIZE"));
        assert!(!loaded(&rules, "NOSIZE"), "its VXL path finds no file");
        assert!(!loaded(&rules, "PLAIN"));
        assert!(rules.object_voxel_half_sizes("PLAIN").is_none());
        // The unit atlas skips a type drawn from a model, refused or not.
        assert!(rules.draws_declared_model("TEST"));
        assert!(!rules.draws_declared_model("PLAIN"));
    }

    /// With `ModelVoxels=X,Y` a model's tilt half sizes are those of a VXL
    /// whose main section is X by Y voxels (`voxel_tilt_half_sizes`),
    /// whatever `ModelSize=` and the body's bounds would give, and no VXL
    /// need be there; a refused value leaves them to `ModelSize=`.
    #[test]
    fn model_voxels_give_the_vxls_tilt_half_sizes() {
        let directory = crate::map::source::test_support::TestDirectory::new("model-voxels");
        let glb = crate::assets::model_file::test_glb::box_model(2.0, 1.0, 4.0, &[]);
        std::fs::write(directory.path().join("tank.glb"), &glb).unwrap();
        let assets = AssetManager::from_loose_root_for_test(directory.path());
        let art = IniFile::from_str(
            "[BODY]\nVoxel=yes\nModel=tank.glb\nModelSize=120\nModelVoxels=24,12\n\
             [BAD]\nVoxel=yes\nModel=tank.glb\nModelSize=120\nModelVoxels=300,12\n",
        );
        let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
            &IniFile::from_str(
                "[VehicleTypes]\n0=TEST\n1=BAD\n[TEST]\nImage=BODY\nStrength=300\n\
                 [BAD]\nStrength=300\n",
            ),
            &art,
        )
        .unwrap();
        rules.install_art_fixture(ArtRegistry::from_ini(&art));
        rules.bind_effect_assets(&assets, "TEM", "TEMPERATE");
        // A 24 by 12 section: [12 / 2 + 0.5, 24 / 2].
        assert_eq!(tilt_half_sizes(24, 12), [6.5, 12.0]);
        assert!(loaded(&rules, "TEST"));
        assert_eq!(rules.object_voxel_half_sizes("TEST"), Some([6.5, 12.0]));
        assert_eq!(
            rules.object_voxel_half_sizes("BAD"),
            Some([5.5, 10.0]),
            "ModelSize= decides, as without the key"
        );
    }

    /// A unit with indexed guns needs a `turretN` node for each, as a missing
    /// indexed TUR VXL clears the main model (5F865F, 5F8ACB); barrels stay
    /// optional.
    #[test]
    fn a_declared_model_without_an_indexed_turret_node_is_not_loaded() {
        let directory =
            crate::map::source::test_support::TestDirectory::new("declared-model-turrets");
        let body = crate::assets::model_file::test_glb::box_model;
        std::fs::write(
            directory.path().join("two.glb"),
            body(1.0, 1.0, 1.0, &["turret0", "turret1"]),
        )
        .unwrap();
        std::fs::write(
            directory.path().join("one.glb"),
            body(1.0, 1.0, 1.0, &["turret0"]),
        )
        .unwrap();
        let assets = AssetManager::from_loose_root_for_test(directory.path());
        let art = IniFile::from_str(
            "[FULL]\nVoxel=yes\nModel=two.glb\nModelSize=100\n\
             [SHORT]\nVoxel=yes\nModel=one.glb\nModelSize=100\n",
        );
        let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
            &IniFile::from_str(
                "[VehicleTypes]\n0=FULL\n1=SHORT\n\
                 [FULL]\nTurret=yes\nTurretCount=2\nStrength=300\n\
                 [SHORT]\nTurret=yes\nTurretCount=2\nStrength=300\n",
            ),
            &art,
        )
        .unwrap();
        rules.install_art_fixture(ArtRegistry::from_ini(&art));
        rules.bind_effect_assets(&assets, "TEM", "TEMPERATE");
        assert!(loaded(&rules, "FULL"));
        assert!(!loaded(&rules, "SHORT"), "turret1 is missing");
    }

    /// A building's voxel guns and a debris type whose art declares
    /// `Model=` take the model in place of their VXL (docs/picture-model-art.md,
    /// chain 5): the building's gun from its `TurretAnim=` section, without
    /// becoming a voxel type (6F9E10); a missing file is refused and draws
    /// nothing; art without the key keeps the VXL path. The model's bytes
    /// reach the config hash as its VXL's identity did.
    #[test]
    fn building_guns_and_debris_take_their_declared_models() {
        let directory = crate::map::source::test_support::TestDirectory::new("declared-art-models");
        let bind = |length: f32| {
            let glb = crate::assets::model_file::test_glb::box_model(1.0, 1.0, length, &["barrel"]);
            std::fs::write(directory.path().join("gun.glb"), &glb).unwrap();
            let assets = AssetManager::from_loose_root_for_test(directory.path());
            let art = IniFile::from_str(
                "[GUNTUR]\nModel=gun.glb\nModelSize=100\n\
                 [CHUNK]\nModel=gun.glb\nModelSize=60\n\
                 [LOST]\nModel=lost.glb\nModelSize=60\n",
            );
            let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
                &IniFile::from_str(
                    "[BuildingTypes]\n0=GUNBLD\n1=PLAINGUN\n\
                     [GUNBLD]\nStrength=300\nTurretAnim=GUNTUR\nTurretAnimIsVoxel=yes\n\
                     [PLAINGUN]\nStrength=300\nTurretAnim=NOTUR\nTurretAnimIsVoxel=yes\n\
                     [VoxelAnims]\n0=CHUNK\n1=LOST\n2=PLAIN\n",
                ),
                &art,
            )
            .unwrap();
            rules.install_art_fixture(ArtRegistry::from_ini(&art));
            rules.bind_effect_assets(&assets, "TEM", "TEMPERATE");
            rules
        };
        let rules = bind(2.0);
        let gun = rules.building_gun_model("GUNBLD").unwrap().unwrap();
        assert_eq!((gun.section.as_str(), gun.decl.size), ("GUNTUR", 100));
        assert!(rules.building_gun_model("PLAINGUN").is_none());
        assert!(rules.draws_declared_model("GUNBLD"));
        assert!(!rules.draws_declared_model("PLAINGUN"));
        assert!(!loaded(&rules, "GUNBLD"), "the guns are not a main model");
        let anim = |name| rules.voxel_anim_type_id_by_name(name).unwrap();
        let chunk = rules.voxel_anim_declared_model(anim("CHUNK")).unwrap();
        assert_eq!(chunk.model.as_ref().unwrap().decl.size, 60);
        assert!(chunk.shared.is_none());
        assert!(rules.voxel_anim_model(anim("CHUNK")).is_none());
        assert!(matches!(
            rules.voxel_anim_declared_model(anim("LOST")),
            Some(DebrisModel { model: None, .. })
        ));
        assert!(rules.voxel_anim_declared_model(anim("PLAIN")).is_none());
        assert!(rules.declares_models());
        assert_ne!(
            rules.simulation_config_hash(),
            bind(3.0).simulation_config_hash(),
            "another file is another identity"
        );
    }

    /// A debris type that shares a part (`ShareTurretData=`, VoxelAnim
    /// `74B4D0`) of a type whose art declares `Model=` draws that part of
    /// the model, and its copied VXL pair is not loaded; one sharing a type
    /// without a model keeps the shared VXL path (docs/picture-model-art.md).
    #[test]
    fn a_sharing_debris_type_draws_its_source_models_part() {
        let directory = crate::map::source::test_support::TestDirectory::new("shared-debris-model");
        let glb = crate::assets::model_file::test_glb::box_model(1.0, 1.0, 2.0, &["turret"]);
        std::fs::write(directory.path().join("src.glb"), &glb).unwrap();
        let assets = AssetManager::from_loose_root_for_test(directory.path());
        let art = IniFile::from_str(
            "[SRC]\nVoxel=yes\nModel=src.glb\nModelSize=100\n[PLAIN]\nVoxel=yes\n",
        );
        let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
            &IniFile::from_str(
                "[VehicleTypes]\n0=SRC\n1=PLAIN\n\
                 [SRC]\nTurret=yes\nStrength=300\n[PLAIN]\nTurret=yes\nStrength=300\n\
                 [VoxelAnims]\n0=SHARD\n1=OTHER\n\
                 [SHARD]\nShareTurretData=yes\nShareSource=SRC\n\
                 [OTHER]\nShareTurretData=yes\nShareSource=PLAIN\n",
            ),
            &art,
        )
        .unwrap();
        rules.install_art_fixture(ArtRegistry::from_ini(&art));
        rules.bind_effect_assets(&assets, "TEM", "TEMPERATE");
        let anim = |name| rules.voxel_anim_type_id_by_name(name).unwrap();
        let shard = rules.voxel_anim_declared_model(anim("SHARD")).unwrap();
        let source = rules
            .loaded_voxel_model("SRC")
            .and_then(LoadedVoxelModel::declared_model)
            .unwrap();
        assert!(Arc::ptr_eq(shard.model.as_ref().unwrap(), source));
        assert_eq!(
            shard.shared,
            Some(crate::assets::model_file::ModelPart::Turret(0))
        );
        assert!(rules.voxel_anim_model(anim("SHARD")).is_none());
        assert!(rules.voxel_anim_declared_model(anim("OTHER")).is_none());
    }

    #[test]
    fn shared_voxel_model_survives_serialization_then_user_loads_its_own_image() {
        use crate::rules::native_processing::{ProcessedRulesLayers, RulesLayerStack};
        let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let art = IniFile::from_str("[MTNK]\nVoxel=yes\n");
        let processed = RulesLayerStack::new(IniFile::from_str(
            "[VehicleTypes]\n0=SOURCE\n[SOURCE]\nImage=MTNK\nTurret=yes\n\
             [VoxelAnims]\n0=UNREAD\n1=SHARD\n[SHARD]\nImage=MTNK\nShareTurretData=yes\nShareSource=SOURCE\n",
        )).process_with_fixed_art(&art).unwrap();
        let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
        rules.bind_effect_assets(&assets, "TEM", "TEMPERATE");
        let shard = rules.voxel_anim_type_id_by_name("SHARD").unwrap();
        assert_eq!(shard.0, 1, "constructor-only types keep their ordinal");
        assert_eq!(
            rules
                .voxel_anim_type(crate::rules::voxel_anim_type::VoxelAnimTypeId(0))
                .duration,
            30
        );
        assert_eq!(
            rules.voxel_anim_model(shard).unwrap().identity().0,
            "MTNKTUR"
        );
        let before = rules.simulation_config_hash();
        let mut saved: ProcessedRulesLayers = crate::util::save_codec::deserialize(
            &crate::util::save_codec::serialize(&processed).unwrap(),
        )
        .unwrap();
        let mut rebound = RuleSet::from_processed_rules(&saved).unwrap();
        rebound.bind_effect_assets(&assets, "TEM", "TEMPERATE");
        assert_eq!(
            before,
            rebound.simulation_config_hash(),
            "interning caches are not saved identity"
        );
        // VoxelTypeLoad74B8BB loads own VXL+HVA, and never replays sharing.
        saved
            .restore_object_image_loads_for_test("TEMPERATE")
            .unwrap();
        let mut loaded = RuleSet::from_processed_rules(&saved).unwrap();
        loaded.bind_effect_assets(&assets, "TEM", "TEMPERATE");
        assert!(
            loaded.voxel_anim_type(shard).shares_data(),
            "+296 itself is saved"
        );
        assert_eq!(
            loaded.voxel_anim_model(shard).unwrap().identity(),
            ("MTNK", Some("MTNK"))
        );
        let hash = loaded.simulation_config_hash();
        // A user-file round trip rebuilds the registry's HashMap seed; its
        // allocation order must not change the restored resource identity.
        saved = crate::util::save_codec::deserialize(
            &crate::util::save_codec::serialize(&saved).unwrap(),
        )
        .unwrap();
        saved
            .restore_object_image_loads_for_test("TEMPERATE")
            .unwrap();
        let mut twice = RuleSet::from_processed_rules(&saved).unwrap();
        twice.bind_effect_assets(&assets, "TEM", "TEMPERATE");
        assert_eq!(twice.simulation_config_hash(), hash);
    }

    /// Ares ObjectTypeClass_Load3DArt_NoSpawnAlt1 (10041280, hook 0x5F8277):
    /// a NoSpawnAlt unit whose <Image>WO pair is missing fails its whole
    /// voxel load, where the stock loader treats the WO pair as optional.
    #[test]
    fn ares_no_spawn_alt_unit_needs_its_wo_pair() {
        let _ares = crate::rules::ares_switch::read_ares_way(true);
        use crate::rules::native_processing::RulesLayerStack;
        let Some((_, retail)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let has_main = |with_wo: bool| {
            let directory = crate::map::source::test_support::TestDirectory::new(if with_wo {
                "ares-no-spawn-alt-wo"
            } else {
                "ares-no-spawn-alt-missing-wo"
            });
            for extension in ["VXL", "HVA"] {
                let bytes = retail.get_ref(&format!("MTNK.{extension}")).unwrap();
                std::fs::write(directory.path().join(format!("MTNK.{extension}")), bytes).unwrap();
                if with_wo {
                    std::fs::write(directory.path().join(format!("MTNKWO.{extension}")), bytes)
                        .unwrap();
                }
            }
            let assets = AssetManager::from_loose_root_for_test(directory.path());
            let processed = RulesLayerStack::new(IniFile::from_str(
                "[VehicleTypes]\n0=SOURCE\n[SOURCE]\nImage=MTNK\nNoSpawnAlt=yes\n",
            ))
            .process_with_fixed_art(&IniFile::from_str("[MTNK]\nVoxel=yes\n"))
            .unwrap();
            let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
            rules.bind_effect_assets(&assets, "TEM", "TEMPERATE");
            rules
                .loaded_voxel_model("SOURCE")
                .is_some_and(|model| model.main().is_some())
        };
        assert!(has_main(true));
        assert!(!has_main(false));
    }

    #[test]
    fn shared_voxel_slots_keep_missing_guns_but_never_follow_retired_generations() {
        use crate::rules::native_processing::{NativeRulesRegistryState, RulesLayerStack};
        let Some((_, retail)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let directory = crate::map::source::test_support::TestDirectory::new("shared-voxel-slots");
        for name in [
            "MTNK.VXL",
            "MTNK.HVA",
            "MTNKTUR.VXL",
            "MTNKTUR.HVA",
            "MTNKBARL.VXL",
            "MTNKBARL.HVA",
        ] {
            std::fs::write(directory.path().join(name), retail.get_ref(name).unwrap()).unwrap();
        }
        for extension in ["VXL", "HVA"] {
            std::fs::write(
                directory.path().join(format!("ALT.{extension}")),
                retail.get_ref(&format!("MTNK.{extension}")).unwrap(),
            )
            .unwrap();
            // Ares ObjectTypeClass_Load3DArt_NoSpawnAlt1 (10041280, hook
            // 0x5F8277): SOURCE's NoSpawnAlt=yes needs its <Image>WO pair
            // when the Rules are read the Ares way.
            if crate::rules::ares_switch::ares_way() {
                for image in ["MTNK", "ALT"] {
                    std::fs::write(
                        directory.path().join(format!("{image}WO.{extension}")),
                        retail.get_ref(&format!("MTNK.{extension}")).unwrap(),
                    )
                    .unwrap();
                }
            }
        }
        let assets = AssetManager::from_loose_root_for_test(directory.path());
        let art = IniFile::from_str("[MTNK]\nVoxel=yes\n[ALT]\nVoxel=yes\n[MISSING]\nVoxel=yes\n");
        let mut state = NativeRulesRegistryState::default();
        let mut read = |text: &str| {
            let processed = RulesLayerStack::new(IniFile::from_str(text))
                .process_with_fixed_art_and_registry_state(&art, std::mem::take(&mut state))
                .unwrap();
            let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
            rules.bind_effect_assets(&assets, "TEM", "TEMPERATE");
            let (_, trace) = processed.into_ini_and_native_type_construction_trace();
            state = trace.into_registry_state_discarding_events();
            rules
        };
        let first = read(
            "[VehicleTypes]\n0=SOURCE\n[SOURCE]\nImage=MTNK\nTurret=yes\nNoSpawnAlt=yes\n\
            [VoxelAnims]\n0=SHARD\n[SHARD]\nImage=MTNK\nShareTurretData=yes\nShareSource=SOURCE\n\
            MinAngularVelocity=5\nMaxAngularVelocity=9\n",
        );
        let shard = first.voxel_anim_type_id_by_name("SHARD").unwrap();
        assert_eq!(
            first.voxel_anim_model(shard).unwrap().identity().0,
            "MTNKTUR"
        );
        let changed = read("[SOURCE]\nImage=ALT\nTurret=?\nNoSpawnAlt=?\n");
        assert!(changed.object("SOURCE").unwrap().has_turret);
        assert!(changed.object("SOURCE").unwrap().no_spawn_alt());
        assert_eq!(
            changed
                .loaded_voxel_model("SOURCE")
                .unwrap()
                .main()
                .unwrap()
                .identity()
                .0,
            "ALT"
        );
        assert_eq!(
            changed.voxel_anim_model(shard).unwrap().identity().0,
            "MTNKTUR",
            "absent ALTTUR preserves the earlier slot"
        );
        assert_eq!(
            super::super::voxel_frame_catalog::detect_hva_frame_count(
                &assets,
                "SOURCE",
                super::super::voxel_frame_catalog::VxlLayer::Turret,
                0,
                Some(&changed)
            ),
            1
        );
        let nodes = changed.voxel_model_plan.node_count();
        let mut node_counts = Vec::new();
        for _ in 0..4 {
            let repeated = read("[SOURCE]\nImage=ALT\n");
            node_counts.push(repeated.voxel_model_plan.node_count());
            assert_eq!(
                repeated.voxel_anim_model(shard).unwrap().identity().0,
                "MTNKTUR"
            );
        }
        assert!(
            node_counts.iter().all(|count| *count <= nodes),
            "identical read retains superseded model operations: initial={nodes}, repeats={node_counts:?}"
        );
        let replaced = read("[SOURCE]\nImage=MTNK\n");
        assert!(
            replaced
                .loaded_voxel_model("SOURCE")
                .unwrap()
                .auxiliary()
                .is_some()
        );
        assert!(
            replaced.voxel_anim_model(shard).is_none(),
            "retired generation policy never retargets a copied pointer to a new allocation of the same file"
        );
        let unshared = read("[SHARD]\nName=ReachedWithoutSharingFlags\nMinAngularVelocity=0\n");
        assert_eq!(
            unshared.voxel_anim_type(shard).min_angular_velocity,
            first.voxel_anim_type(shard).min_angular_velocity,
            "zero keeps the previous angular velocity"
        );
        assert_eq!(
            f64::from_bits(unshared.voxel_anim_type(shard).max_angular_velocity.bits()),
            -f64::from_bits(crate::rules::voxel_anim_type::DEGREES_TO_RADIANS.bits()),
            "an absent key on a reached section uses the literal negative sentinel"
        );
        assert!(
            !unshared.voxel_anim_type(shard).shares_data(),
            "per-read false defaults override historical sharing text"
        );
        assert_eq!(
            unshared.voxel_anim_model(shard).unwrap().identity().0,
            "MTNK"
        );
        let missing = read(
            "[SOURCE]\nImage=MISSING\n[SHARD]\nShareBodyData=yes\nShareBarrelData=yes\nShareSource=SOURCE\n",
        );
        assert!(
            missing
                .loaded_voxel_model("SOURCE")
                .unwrap()
                .barrel()
                .is_some()
        );
        assert!(
            missing.voxel_anim_model(shard).is_none(),
            "higher-priority null body cannot fall back to barrel"
        );
        let sentinel = read(
            "[VehicleTypes]\n1=<none>\n[<none>]\nImage=MTNK\nTurret=yes\n\
            [SHARD]\nShareBodyData=yes\n",
        );
        // ReadVehicleTypes6723B0 calls7480D0, which rejects both none
        // sentinels at7480D4..F6 before lookup or allocation.
        assert!(sentinel.object("<none>").is_none());
        assert!(
            sentinel.voxel_anim_model(shard).is_none(),
            "missing ShareSource remains null when the authored sentinel type is rejected"
        );
    }

    #[test]
    fn main_voxel_binding_requires_declared_valid_complete_pair_and_rebuilds() {
        let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let directory =
            crate::map::source::test_support::TestDirectory::new("voxel-main-admission");
        for (source, destination) in [("DEST.VXL", "BODY.VXL"), ("DEST.HVA", "BODY.HVA")] {
            std::fs::write(
                directory.path().join(destination),
                assets.get_ref(source).unwrap(),
            )
            .unwrap();
        }
        let load_assets = || AssetManager::from_loose_root_for_test(directory.path());
        let mut rules = body_rules(true);
        assert!(!loaded(&rules, "TEST"), "unbound is explicit");
        rules.bind_effect_assets(&load_assets(), "TEM", "TEMPERATE");
        assert!(loaded(&rules, "test"));
        // Original loaded DEST postpass in naval_draw_squid_tilt.json.
        assert_eq!(rules.object_voxel_half_sizes("test"), Some([23.5, 51.]));
        let bound_hash = rules.simulation_config_hash();
        rules.bind_effect_assets(&load_assets(), "TEM", "TEMPERATE");
        assert_eq!(rules.simulation_config_hash(), bound_hash);
        // Replacing the physical body changes both derived pose metadata and
        // deterministic configuration identity, even with unchanged admission.
        for (source, destination) in [("GTNK.VXL", "BODY.VXL"), ("GTNK.HVA", "BODY.HVA")] {
            std::fs::write(
                directory.path().join(destination),
                assets.get_ref(source).unwrap(),
            )
            .unwrap();
        }
        rules.bind_effect_assets(&load_assets(), "TEM", "TEMPERATE");
        assert!(loaded(&rules, "TEST"));
        assert_eq!(rules.object_voxel_half_sizes("TEST"), Some([15.5, 20.5]));
        assert_ne!(rules.simulation_config_hash(), bound_hash);
        rules.install_art_fixture(ArtRegistry::from_ini(&IniFile::from_str(
            "[BODY]\nVoxel=no\n",
        )));
        assert!(!loaded(&rules, "TEST"), "ART replacement invalidates");
        assert!(rules.object_voxel_half_sizes("TEST").is_none());
        rules.bind_effect_assets(&load_assets(), "TEM", "TEMPERATE");
        assert!(
            loaded(&rules, "TEST"),
            "ART metadata replacement cannot replay ReadINI or discard its retained VXL request"
        );
        rules = body_rules(false);
        rules.bind_effect_assets(&load_assets(), "TEM", "TEMPERATE");
        assert!(
            !loaded(&rules, "TEST"),
            "without a reached VXL request, a physical model alone is insufficient"
        );
        rules = body_rules(true);
        std::fs::remove_file(directory.path().join("BODY.HVA")).unwrap();
        rules.bind_effect_assets(&load_assets(), "TEM", "TEMPERATE");
        assert!(!loaded(&rules, "TEST"), "failed HVA unloads B0");
        std::fs::write(
            directory.path().join("BODY.HVA"),
            assets.get_ref("DEST.HVA").unwrap(),
        )
        .unwrap();
        std::fs::write(directory.path().join("BODY.VXL"), b"invalid VXL").unwrap();
        rules.bind_effect_assets(&load_assets(), "TEM", "TEMPERATE");
        assert!(!loaded(&rules, "TEST"), "malformed VXL is not loaded");
    }
}
