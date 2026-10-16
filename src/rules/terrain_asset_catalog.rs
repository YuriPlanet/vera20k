//! GPU-independent terrain SHP metadata used by authoritative match systems.
//!
//! Terrain rendering addresses only the body half of a terrain SHP. Gameplay,
//! however, follows `TerrainClass::AI @ 0x0071C730` and reads the signed SHP
//! header frame count before comparing the current frame with `count / 2`.
//! Keeping the raw count here prevents presentation's body-frame projection
//! from being halved a second time by the simulation.

use std::collections::{BTreeMap, BTreeSet};

use crate::assets::asset_manager::AssetManager;
use crate::assets::shp_file::ShpFile;
use crate::rules::art_data;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;

/// Deterministic raw SHP frame counts for animated terrain ore spawners.
///
/// Keys are trimmed uppercase terrain-type IDs. Missing or malformed optional
/// assets are omitted so the existing zero-frame simulation fallback remains
/// explicit at the consumer. That fallback is VERA-internal; gamemd behavior
/// for missing or malformed modded terrain art remains UNCHECKED.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct TerrainSpawnerAssetCatalog {
    frame_counts: BTreeMap<String, u16>,
}

impl TerrainSpawnerAssetCatalog {
    /// Bind every registered animated `SpawnsTiberium=yes` terrain type.
    pub fn bind(
        rules: &RuleSet,
        rules_ini: &IniFile,
        asset_manager: &AssetManager,
        theater_ext: &str,
        theater_name: &str,
    ) -> Self {
        let roots: BTreeSet<String> = rules
            .terrain_object_types
            .values()
            .filter(|terrain| terrain.spawns_tiberium && terrain.is_animated)
            .filter_map(|terrain| canonical_type_id(&terrain.name))
            .collect();
        let mut catalog = Self::default();

        for name in roots {
            let image_id = rules.art().resolve_overlay_image_id(&name, rules_ini);
            let candidates = art_data::overlay_shp_candidates(
                Some(rules.art()),
                &name,
                &image_id,
                theater_ext,
                theater_name,
            );
            let mut found_asset = false;
            let mut invalid_candidates = Vec::new();
            let mut frame_count = None;
            for candidate in &candidates {
                let Some(data) = asset_manager.get_ref(candidate) else {
                    continue;
                };
                found_asset = true;
                match ShpFile::frame_count_from_bytes(data) {
                    Ok(count) => {
                        frame_count = Some(count);
                        break;
                    }
                    Err(error) => invalid_candidates.push(format!("{candidate}: {error}")),
                }
            }

            if let Some(frame_count) = frame_count {
                catalog.frame_counts.insert(name, frame_count);
            } else if found_asset {
                log::warn!(
                    "Authoritative terrain-spawner asset [{name}] has no valid SHP candidate ({invalid_candidates:?}); no validated spawning midpoint is available"
                );
            } else {
                log::warn!(
                    "Authoritative terrain-spawner asset [{name}] is missing; no validated spawning midpoint is available"
                );
            }
        }

        catalog
    }

    /// Literal unsigned SHP header count used by the persisted terrain-spawner
    /// state. Stock TIBTRE assets return 22 and therefore target midpoint 11.
    pub fn frame_count(&self, name: &str) -> Option<u16> {
        canonical_type_id(name).and_then(|name| self.frame_counts.get(&name).copied())
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_for_test(&mut self, name: &str, frame_count: u16) {
        if let Some(name) = canonical_type_id(name) {
            self.frame_counts.insert(name, frame_count);
        }
    }
}

fn canonical_type_id(name: &str) -> Option<String> {
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    use crate::rules::art_data::ArtRegistry;

    #[test]
    fn loose_binding_keeps_raw_tibtre_count_without_renderer_body_halving() {
        let root = TestRoot::new();
        std::fs::write(root.path().join("TIBALT.TEM"), truncated_shp_header(4))
            .expect("write malformed first candidate");
        std::fs::write(root.path().join("TIBALT.SHP"), shp_header(22))
            .expect("write valid fallback TIBTRE fixture");
        let assets = AssetManager::from_loose_root_for_test(root.path());
        let rules_ini = IniFile::from_str(
            "[TerrainTypes]\n0=TIBTRE01\n1=TREE01\n\
             [TIBTRE01]\nImage=TIBALT\nSpawnsTiberium=yes\nIsAnimated=yes\n\
             [TREE01]\nSpawnsTiberium=no\nIsAnimated=yes\n",
        );
        let mut rules = RuleSet::from_ini(&rules_ini).expect("terrain asset rules");
        let art = ArtRegistry::from_ini(&IniFile::from_str("[TIBTRE01]\nTheater=yes\n"));
        // Synthetic fixture supplies ART directly, without native read-admission replay.
        rules.install_art_fixture(art);

        let catalog =
            TerrainSpawnerAssetCatalog::bind(&rules, &rules_ini, &assets, "TEM", "TEMPERATE");

        assert_eq!(catalog.frame_count(" tibtre01 "), Some(22));
        assert_eq!(catalog.frame_count("TREE01"), None);
    }

    fn shp_header(frame_count: u16) -> Vec<u8> {
        let mut data = vec![0_u8; 8 + usize::from(frame_count) * 24];
        data[6..8].copy_from_slice(&frame_count.to_le_bytes());
        data
    }

    fn truncated_shp_header(frame_count: u16) -> Vec<u8> {
        let mut data = vec![0_u8; 8];
        data[6..8].copy_from_slice(&frame_count.to_le_bytes());
        data
    }

    static NEXT_TEST_ROOT: AtomicU64 = AtomicU64::new(0);

    struct TestRoot(PathBuf);

    impl TestRoot {
        fn new() -> Self {
            let serial = NEXT_TEST_ROOT.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "vera20k-terrain-asset-catalog-{}-{serial}",
                std::process::id()
            ));
            std::fs::create_dir(&path).expect("create terrain asset test root");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
