//! GPU-independent main-voxel admission shared by gameplay and presentation.
//!
//! Source: fixed ART's Voxel flag, final rules Image/Unit gun configuration,
//! and the match's AssetManager bytes. RuleSet rebuilds this immutable catalog
//! alongside effect assets; replacing ART invalidates it. An unbound catalog
//! admits no type. No renderer cache or per-entity flag can change this input.

use std::collections::BTreeMap;

use crate::assets::asset_manager::AssetManager;
use crate::assets::hva_file::HvaFile;
use crate::assets::vxl_file::VxlFile;
use crate::rules::object_type::ObjectCategory;
use crate::rules::ruleset::{RuleSet, TypeHandle};
use crate::rules::voxel_frame_catalog::{
    VxlLayer, indexed_turret_count, voxel_gun_basename, voxel_image_id,
};

/// Parsed physical part. Rendering borrows its inputs without owning a second
/// file-admission policy. Building and raw-image callers explicitly permit an
/// absent HVA; native Techno main-body admission requires the complete pair.
pub(crate) struct VoxelPart {
    vxl: VxlFile,
    hva: Option<HvaFile>,
}

impl VoxelPart {
    pub(crate) fn load(assets: &AssetManager, image: &str, require_hva: bool) -> Option<Self> {
        let (vxl, hva) = crate::rules::art_data::voxel_asset_names(image);
        let vxl = VxlFile::from_bytes(assets.get_ref(&vxl)?).ok()?;
        let hva = assets
            .get_ref(&hva)
            .and_then(|bytes| HvaFile::from_bytes(bytes).ok());
        if require_hva && hva.is_none() {
            return None;
        }
        Some(Self { vxl, hva })
    }

    pub(crate) fn vxl(&self) -> &VxlFile {
        &self.vxl
    }

    pub(crate) fn hva(&self) -> Option<&HvaFile> {
        self.hva.as_ref()
    }
}

pub(crate) type MainVoxelParts = (VoxelPart, Vec<(Option<VoxelPart>, Option<VoxelPart>)>);

/// Native TechnoType7160C7..71611D's main-VXL postpass: first section
/// SizeY/2+0.5 and SizeX/2, in original voxel units. Every value is exact in
/// binary32; both simulation and rendering use this physical metadata.
pub(crate) fn voxel_tilt_half_sizes(vxl: &VxlFile) -> Option<[f32; 2]> {
    let section = vxl.limbs.first()?;
    Some([
        f32::from(section.size_y) * 0.5 + 0.5,
        f32::from(section.size_x) * 0.5,
    ])
}

/// ObjectType5F8110's body and Unit gun loading. TechnoType716090's Voxel
/// declaration gates the call. Main VXL ctor755CD0/755CFE retains an error byte;
/// IsVoxel6F9E10 requires the resulting B0 pointer and error byte zero.
/// Missing/invalid main HVA sets failure5F8245; cleanup5F8A60..5F8AA4 clears
/// B0/B4. Indexed TUR/HVA and present BARL/HVA failures share that cleanup.
///
/// A name without a rules type is an explicit raw-image component boundary
/// (Bullet, debris or auxiliary image), retaining its existing optional HVA.
/// Optional legacy nonindexed gun HVA behavior remains outside this main-body
/// and indexed-pair admission claim.
pub(crate) fn load_main_voxel(
    assets: &AssetManager,
    type_id: &str,
    rules: Option<&RuleSet>,
) -> Option<MainVoxelParts> {
    let object = rules.and_then(|rules| rules.object(type_id));
    if let Some(object) = object
        && (object.category == ObjectCategory::Building
            || !rules?.art().get(&object.image).is_some_and(|art| art.voxel))
    {
        return None;
    }
    let image = voxel_image_id(type_id, rules);
    let body = VoxelPart::load(assets, &image, object.is_some())?;
    let indexed_count = indexed_turret_count(type_id, rules);
    let has_turret = object
        .is_some_and(|object| object.category == ObjectCategory::Vehicle && object.has_turret);
    let mut guns = Vec::new();
    for index in 0..indexed_count.max(i32::from(has_turret)) {
        let turret_name = voxel_gun_basename(&image, VxlLayer::Turret, index);
        let turret = VoxelPart::load(assets, &turret_name, indexed_count > 0);
        if indexed_count > 0 && turret.is_none() {
            return None;
        }
        let barrel = if has_turret {
            let name = voxel_gun_basename(&image, VxlLayer::Barrel, index);
            if assets.get_ref(&format!("{name}.VXL")).is_some() {
                let barrel = VoxelPart::load(assets, &name, indexed_count > 0);
                if indexed_count > 0 && barrel.is_none() {
                    return None;
                }
                barrel
            } else {
                None
            }
        } else {
            None
        };
        guns.push((turret, barrel));
    }
    Some((body, guns))
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct VoxelAssetCatalog {
    /// Each loaded type's uppercase id and its main body's tilt half sizes
    /// (binary32 bits): what the catalog hashes.
    loaded: BTreeMap<String, Option<[u32; 2]>>,
    /// Derived: whether each object handle's type is in `loaded`, so the
    /// per-frame question reads no name.
    by_handle: Vec<bool>,
}

impl std::hash::Hash for VoxelAssetCatalog {
    /// `loaded` alone: `by_handle` derives from it, so the rules hash that
    /// saves carry stays the same.
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.loaded.hash(state);
    }
}

impl VoxelAssetCatalog {
    pub(crate) fn bind(rules: &RuleSet, assets: &AssetManager) -> Self {
        let bodies: Vec<Option<Option<[u32; 2]>>> = rules
            .all_objects()
            .map(|object| {
                let (body, _) = load_main_voxel(assets, &object.id, Some(rules))?;
                Some(voxel_tilt_half_sizes(body.vxl()).map(|sizes| sizes.map(f32::to_bits)))
            })
            .collect();
        Self {
            loaded: rules
                .all_objects()
                .zip(&bodies)
                .filter_map(|(object, &body)| Some((object.id.to_ascii_uppercase(), body?)))
                .collect(),
            by_handle: bodies.iter().map(Option::is_some).collect(),
        }
    }

    pub(crate) fn contains(&self, object: TypeHandle) -> bool {
        self.by_handle
            .get(object.0 as usize)
            .copied()
            .unwrap_or(false)
    }

    fn body(&self, type_id: &str) -> Option<&Option<[u32; 2]>> {
        if type_id.bytes().any(|byte| byte.is_ascii_lowercase()) {
            self.loaded.get(&type_id.to_ascii_uppercase())
        } else {
            self.loaded.get(type_id)
        }
    }

    pub(crate) fn half_sizes(&self, type_id: &str) -> Option<[f32; 2]> {
        self.body(type_id)?.map(|bits| bits.map(f32::from_bits))
    }

    #[cfg(test)]
    pub(crate) fn supply_loaded(&mut self, type_id: &str, object: TypeHandle) {
        self.loaded.insert(type_id.to_ascii_uppercase(), None);
        let index = object.0 as usize;
        if self.by_handle.len() <= index {
            self.by_handle.resize(index + 1, false);
        }
        self.by_handle[index] = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{art_data::ArtRegistry, ini_parser::IniFile};

    fn body_rules(voxel: bool) -> RuleSet {
        let mut rules = RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=TEST\n[TEST]\nImage=BODY\nStrength=300\n",
        ))
        .unwrap();
        rules.install_art_fixture(ArtRegistry::from_ini(&IniFile::from_str(&format!(
            "[BODY]\nVoxel={voxel}\n"
        ))));
        rules
    }

    fn loaded(rules: &RuleSet, type_id: &str) -> bool {
        rules
            .type_handle(type_id)
            .is_some_and(|object| rules.object_has_loaded_voxel(object))
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
            !loaded(&rules, "TEST"),
            "a physical VXL is not a declaration"
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
