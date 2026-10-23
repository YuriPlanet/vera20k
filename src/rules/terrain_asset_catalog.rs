//! GPU-independent native TerrainType/terrain-spawner SHP metadata.
//!
//! TerrainTypeClass::ReadINI @ 0x0071DEA0 also reads frame 0's raw SHP-header
//! RGB before its per-pass RadarColor override. The same owner binds that
//! derived type value and the raw frame counts used by TerrainClass::AI.

use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};

use crate::assets::asset_manager::AssetManager;
use crate::assets::shp_file::ShpFile;
use crate::rules::ini_parser::IniSection;
use crate::rules::terrain_object_type::TerrainObjectType;

/// TerrainReadINI71DF9C loads non-theater Image.SHP only on a fresh read.
/// TypeLoad71E218 -> ObjectLoadSHP5F923E leaves it null; InitTheater71DCA0
/// reconstructs only theater shapes. This resource phase is not simulation state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TerrainAssetLoadPhase {
    #[default]
    Fresh,
    Saved,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct TerrainShapeBinding {
    filename: Option<String>,
    /// Immutable physical timing metadata for the already saved per-ID timer.
    /// Only IsAnimated/SpawnsTiberium types have this gameplay projection.
    frame_count: Option<u16>,
}

/// One asset result per Terrain type. Filenames represent native shape residency;
/// the timing projection remains independent of saved presentation reconstruction.
/// Rebinding from the same typed rules/assets replaces every derived entry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TerrainAssetCatalog {
    bindings: BTreeMap<String, TerrainShapeBinding>,
}

impl Hash for TerrainAssetCatalog {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Preserve the former BTreeMap<String,u16> timing fold. A false-theater
        // static image disappearing on native load must not reject its save as
        // different gameplay rules merely because presentation is rebuilt.
        self.bindings
            .values()
            .filter(|binding| binding.frame_count.is_some())
            .count()
            .hash(state);
        for (name, binding) in &self.bindings {
            if let Some(count) = binding.frame_count {
                name.hash(state);
                count.hash(state);
            }
        }
    }
}

impl TerrainAssetCatalog {
    pub fn bind(
        terrain_types: &mut HashMap<String, TerrainObjectType>,
        asset_manager: &AssetManager,
        theater_ext: &str,
        phase: TerrainAssetLoadPhase,
    ) -> Self {
        let mut image_colors = BTreeMap::<String, Option<[u8; 3]>>::new();
        for terrain in terrain_types.values_mut() {
            let mut current = [0; 3];
            for read in &terrain.radar_color_read_passes {
                if let Some(image) = &read.image {
                    let image_color = *image_colors
                        .entry(image.to_ascii_uppercase())
                        .or_insert_with(|| {
                            // TerrainTypeReadINI71DF9E..71DFC0 uses ImageFile.SHP
                            // exactly; theater shapes have not been reloaded yet.
                            asset_manager
                                .get_ref(&format!("{image}.SHP"))
                                .and_then(|bytes| {
                                    ShpFile::frame_radar_color_from_bytes(bytes, 0).ok()
                                })
                        });
                    if let Some(image_color) = image_color {
                        current = image_color;
                    }
                }
                if let Some(raw) = read.radar_color.as_deref() {
                    current = IniSection::read_color_rgb_value(raw, current);
                }
            }
            // Saved Type fields retain these historical read results. Loading
            // reconstructs presentation without performing another Rules read.
            terrain.radar_color = current;
        }

        let mut catalog = Self::default();
        for terrain in terrain_types.values() {
            let Some(name) = canonical_type_id(&terrain.name) else {
                continue;
            };
            let filename = terrain.shape_filename(theater_ext);
            let count = asset_manager
                .get_ref(&filename)
                .and_then(|bytes| ShpFile::frame_count_from_bytes(bytes).ok());
            let spawner = terrain.spawns_tiberium && terrain.is_animated;
            if spawner && count.is_none() {
                log::warn!(
                    "Terrain spawner [{name}] has no valid SHP {filename}; no validated midpoint is available"
                );
            }
            catalog.bindings.insert(
                name,
                TerrainShapeBinding {
                    filename: (count.is_some()
                        && (phase == TerrainAssetLoadPhase::Fresh || terrain.uses_theater_shape()))
                    .then_some(filename),
                    frame_count: spawner.then_some(count).flatten(),
                },
            );
        }
        catalog
    }

    /// Literal unsigned SHP count used by the retained per-ID Terrain timer.
    pub fn frame_count(&self, name: &str) -> Option<u16> {
        canonical_type_id(name).and_then(|name| self.bindings.get(&name)?.frame_count)
    }

    /// Resident shape after fresh or saved resource binding. The atlas consumes
    /// this result; it cannot select another image or invent a fallback.
    pub fn shape_filename(&self, name: &str) -> Option<&str> {
        canonical_type_id(name).and_then(|name| self.bindings.get(&name)?.filename.as_deref())
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_for_test(&mut self, name: &str, frame_count: u16) {
        if let Some(name) = canonical_type_id(name) {
            self.bindings.entry(name).or_default().frame_count = Some(frame_count);
        }
    }
}

fn canonical_type_id(name: &str) -> Option<String> {
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
    use crate::rules::ruleset::RuleSet;

    #[test]
    fn terrain_image_reads_retain_flags_foundation_and_color_before_saved_shape_rebuild() {
        let directory = crate::map::source::test_support::TestDirectory::new("terrain-image-reads");
        for (name, color) in [
            ("ROOT.SHP", [10, 20, 30]),
            ("NOART.SHP", [90, 91, 92]),
            ("PLAIN.SHP", [40, 50, 60]),
            ("TARGET.TEM", [70, 80, 90]),
        ] {
            directory.write(name, &test_shp_with_radar_color(color));
        }
        let assets = AssetManager::from_loose_root_for_test(directory.path());
        let art = IniFile::from_str(
            "[ROOT]\nTheater=yes\nNewTheater=yes\nFoundation=2x2\n[PLAIN]\nTheater=no\nFoundation=3x2\n[TARGET]\nTheater=yes\nFoundation=1x1\n",
        );
        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[TerrainTypes]\n0=TARGET\n[TARGET]\nImage=ROOT\n",
        ));
        layers.push(
            RulesLayerKind::GameMode,
            IniFile::from_str("[TARGET]\nImage=NOART\n"),
        );
        let processed = layers.process_with_fixed_art(&art).unwrap();
        let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
        rules.bind_terrain_assets(&assets, "TEM", TerrainAssetLoadPhase::Fresh);
        let target = rules
            .terrain_object_type_case_insensitive("TARGET")
            .unwrap();
        assert_eq!(
            target.foundation, "2x2",
            "absent Image ART keeps the prior Foundation"
        );
        assert_eq!(
            target.radar_color, [0; 3],
            "Theater=true skips both available Image.SHP header colors"
        );
        assert_eq!(
            rules.terrain_shape_filename("TARGET"),
            Some("TARGET.TEM"),
            "missing Image ART preserves the prior Theater byte"
        );

        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[TARGET]\nImage=PLAIN\n"),
        );
        let processed = layers.process_with_fixed_art(&art).unwrap();
        let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
        rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
        rules.bind_terrain_assets(&assets, "TEM", TerrainAssetLoadPhase::Fresh);
        let target = rules
            .terrain_object_type_case_insensitive("TARGET")
            .unwrap();
        assert_eq!(
            target.foundation, "3x2",
            "type-ID art installation cannot overwrite the Image-selected read"
        );
        assert_eq!(target.radar_color, [40, 50, 60]);
        assert_eq!(
            rules.terrain_shape_filename("TARGET"),
            Some("PLAIN.SHP"),
            "explicit false clears Theater despite type-ID ART"
        );
        let fresh_hash = rules.simulation_config_hash();
        rules.bind_terrain_assets(&assets, "TEM", TerrainAssetLoadPhase::Saved);
        assert_eq!(
            rules.terrain_shape_filename("TARGET"),
            None,
            "native TypeLoad clears non-theater art without replaying ReadINI"
        );
        assert_eq!(
            rules
                .terrain_object_type_case_insensitive("TARGET")
                .unwrap()
                .radar_color,
            [40, 50, 60]
        );
        assert_eq!(
            rules.simulation_config_hash(),
            fresh_hash,
            "presentation residency does not change saved gameplay rules"
        );

        let mut saved = serde_json::to_value(&processed).unwrap();
        saved["native_type_construction_trace"]["registry_state"]["families"]["Terrain"][0]["terrain_type_read_state"]
            ["foundation"] = serde_json::json!(crate::rules::foundation::FOUNDATION_TABLE.len());
        let restored = serde_json::from_value(saved).unwrap();
        assert!(matches!(
            RuleSet::from_processed_rules(&restored),
            Err(crate::rules::error::RulesError::InvalidSavedState(_))
        ));
    }

    #[test]
    fn retail_theater_spawner_keeps_own_shape_when_rules_image_changes() {
        // TerrainType InitTheater71DCD8..71DD3B replaces shape+A4 with
        // <stored type ID>.<theater>, independently of retained ImageFile.
        let Some(mut battle) =
            crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch(
                "XMP03T4.MAP",
                &IniFile::from_str("[TIBTRE01]\nImage=TREE01\n"),
            )
        else {
            return;
        };
        let (_, mut assets) = crate::rules::retail_ini_fixture::retail_scenario_assets().unwrap();
        let theater = crate::map::theater::load_theater(&mut assets, "TEMPERATE").unwrap();
        assert_eq!(
            ShpFile::frame_count_from_bytes(assets.get_ref("TREE01.TEM").unwrap()).unwrap(),
            2
        );
        let own_count =
            ShpFile::frame_count_from_bytes(assets.get_ref("TIBTRE01.TEM").unwrap()).unwrap();
        assert_eq!(own_count, 22);
        battle.rules.bind_terrain_assets(
            &assets,
            theater.extension,
            crate::rules::terrain_asset_catalog::TerrainAssetLoadPhase::Fresh,
        );
        assert_eq!(
            battle.rules.terrain_spawner_frame_count("TIBTRE01"),
            Some(own_count)
        );
    }

    #[test]
    fn terrain_radar_color_matches_stock_tree_after_asset_bind() {
        let Some(mut battle) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let Some((_, mut assets)) = crate::rules::retail_ini_fixture::retail_scenario_assets()
        else {
            return;
        };
        let theater = crate::map::theater::load_theater(&mut assets, "TEMPERATE")
            .expect("stock Battle theater");

        battle.rules.bind_terrain_assets(
            &assets,
            theater.extension,
            crate::rules::terrain_asset_catalog::TerrainAssetLoadPhase::Fresh,
        );
        let tree = battle
            .rules
            .terrain_object_type_case_insensitive("TREE01")
            .expect("stock Battle registers TREE01");
        assert_eq!(tree.radar_color, [0, 192, 0]);
    }

    #[test]
    fn terrain_radar_color_reloads_current_image_each_rules_pass() {
        let directory = crate::map::source::test_support::TestDirectory::new("terrain-radar-color");
        directory.write("ROOT.SHP", &test_shp_with_radar_color([10, 20, 30]));
        directory.write("MODE.SHP", &test_shp_with_radar_color([40, 50, 60]));
        let assets = AssetManager::from_loose_root_for_test(directory.path());

        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[TerrainTypes]\n0=RADAR_TEST\n[RADAR_TEST]\nImage=ROOT\n",
        ));
        layers.push(
            RulesLayerKind::GameMode,
            IniFile::from_str("[RADAR_TEST]\nImage=MODE\nRadarColor=90,91,92\n"),
        );
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[RADAR_TEST]\nRadarColor=invalid\n"),
        );
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[RADAR_TEST]\nStrength=1\n"),
        );
        let processed = layers.process().expect("production rules passes");
        let mut rules = RuleSet::from_processed_rules(&processed).expect("typed Rules projection");
        rules.bind_terrain_assets(&assets, "tem", TerrainAssetLoadPhase::Fresh);
        assert_eq!(
            rules
                .terrain_object_type_case_insensitive("RADAR_TEST")
                .expect("registered terrain type")
                .radar_color,
            [40, 50, 60],
            "the later absent/invalid RadarColor reads use the successfully loaded current Image frame"
        );

        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[RADAR_TEST]\nRadarColor=70,80,90\n"),
        );
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[RADAR_TEST]\nImage=MISSING\n"),
        );
        let processed = layers.process().expect("extended production rules passes");
        let mut rules = RuleSet::from_processed_rules(&processed).expect("typed Rules projection");
        rules.bind_terrain_assets(&assets, "tem", TerrainAssetLoadPhase::Fresh);
        assert_eq!(
            rules
                .terrain_object_type_case_insensitive("RADAR_TEST")
                .expect("registered terrain type")
                .radar_color,
            [70, 80, 90],
            "a missing Image SHP skips GetColor and preserves the current RadarColor"
        );
    }

    fn test_shp_with_radar_color(color: [u8; 3]) -> Vec<u8> {
        let mut data = vec![0; 8 + 24 + 1];
        data[2..4].copy_from_slice(&1u16.to_le_bytes());
        data[4..6].copy_from_slice(&1u16.to_le_bytes());
        data[6..8].copy_from_slice(&1u16.to_le_bytes());
        data[8 + 4..8 + 6].copy_from_slice(&1u16.to_le_bytes());
        data[8 + 6..8 + 8].copy_from_slice(&1u16.to_le_bytes());
        data[8 + 12..8 + 15].copy_from_slice(&color);
        data[8 + 20..8 + 24].copy_from_slice(&32u32.to_le_bytes());
        data[32] = 1;
        data
    }
}
