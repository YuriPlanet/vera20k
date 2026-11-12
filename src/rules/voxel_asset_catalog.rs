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
}
impl LoadedVoxelModel {
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
    Some([
        f32::from(section.size_y) * 0.5 + 0.5,
        f32::from(section.size_x) * 0.5,
    ])
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
    }
}

#[derive(Debug, Default, Clone)]
pub(crate) struct VoxelAssetCatalog {
    bound: bool,
    loaded: BTreeMap<String, Option<[u32; 2]>>,
    by_handle: Vec<bool>,
    models: BTreeMap<(ObjectCategory, String), LoadedVoxelModel>,
    voxel_anims: Vec<Option<VoxelPart>>,
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
                (format!("{category:?}:{name}"), parts)
            })
            .collect();
        result.extend(
            self.voxel_anims
                .iter()
                .enumerate()
                .map(|(i, p)| (format!("VoxelAnim:{i}"), vec![part(p.as_ref())])),
        );
        result
    }
    pub(crate) fn bind(rules: &RuleSet, assets: &AssetManager) -> Self {
        let mut libraries = BTreeMap::new();
        let requests: BTreeSet<_> = rules.voxel_model_plan.requests().cloned().collect();
        for request in requests {
            let (vxl, hva) = crate::rules::art_data::voxel_asset_names(&request.image);
            let name = match request.kind {
                LibraryKind::Vxl => vxl,
                LibraryKind::Hva => hva,
            };
            let bytes = assets.get_ref(&name);
            let library = bytes.and_then(|bytes| match request.kind {
                LibraryKind::Vxl => VxlFile::from_bytes(bytes)
                    .ok()
                    .map(|v| Library::Vxl(Arc::new(v))),
                LibraryKind::Hva => HvaFile::from_bytes(bytes)
                    .ok()
                    .map(|v| Library::Hva(Arc::new(v))),
            });
            libraries.insert(
                request,
                LibraryRead {
                    present: bytes.is_some(),
                    library,
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
                (
                    (object.category, object.id.to_ascii_uppercase()),
                    model(&object.voxel_model_slots, &plan, &libraries),
                )
            })
            .collect();
        let by_handle = rules
            .all_objects()
            .map(|object| {
                models[&(object.category, object.id.to_ascii_uppercase())]
                    .main()
                    .is_some()
            })
            .collect();
        let loaded = rules
            .all_objects()
            .filter_map(|object| {
                let body = models[&(object.category, object.id.to_ascii_uppercase())].main()?;
                Some((
                    object.id.to_ascii_uppercase(),
                    voxel_tilt_half_sizes(body.vxl()).map(|v| v.map(f32::to_bits)),
                ))
            })
            .collect();
        let voxel_anims = rules
            .voxel_anim_types_iter()
            .map(|kind| part(kind.model_slots.main, &plan, &libraries))
            .collect();
        Self {
            bound: true,
            loaded,
            by_handle,
            models,
            voxel_anims,
        }
    }
    pub(crate) fn is_bound(&self) -> bool {
        self.bound
    }
    pub(crate) fn model(&self, category: ObjectCategory, name: &str) -> Option<&LoadedVoxelModel> {
        self.models.get(&(category, name.to_ascii_uppercase()))
    }
    pub(crate) fn voxel_anim(&self, index: usize) -> Option<&VoxelPart> {
        self.voxel_anims.get(index)?.as_ref()
    }
    pub(crate) fn contains(&self, object: TypeHandle) -> bool {
        self.by_handle
            .get(object.0 as usize)
            .copied()
            .unwrap_or(false)
    }
    pub(crate) fn half_sizes(&self, type_id: &str) -> Option<[f32; 2]> {
        self.loaded
            .get(&type_id.to_ascii_uppercase())?
            .map(|bits| bits.map(f32::from_bits))
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

#[cfg(test)]
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
    #[cfg(feature = "ares")]
    #[test]
    fn ares_no_spawn_alt_unit_needs_its_wo_pair() {
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
            // 0x5F8277): SOURCE's NoSpawnAlt=yes needs its <Image>WO pair.
            #[cfg(feature = "ares")]
            for image in ["MTNK", "ALT"] {
                std::fs::write(
                    directory.path().join(format!("{image}WO.{extension}")),
                    retail.get_ref(&format!("MTNK.{extension}")).unwrap(),
                )
                .unwrap();
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
        assert!(changed.object("SOURCE").unwrap().no_spawn_alt);
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
