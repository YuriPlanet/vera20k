//! Saved match inputs whose live authorities are outside `Simulation`.
//!
//! LoadContent67E730 restores Rules675210, Mouse5BDF70 and the class hierarchy
//! before PostLoad685120. Re-running fresh Full_Init would instead consume
//! constructors/RNG and replace retained reader fields. This neutral wire
//! record captures the existing processed Rules and bound resource owners;
//! it is not another live rules registry or a native SAV decoder.

use serde::{Deserialize, Serialize};

use crate::map::resolved_terrain::{SavedTerrainError, SavedTerrainTemplate, SharedCellDummy};
use crate::rules::native_processing::ProcessedRulesLayers;
use crate::rules::ruleset::RuleSet;
use crate::sim::runtime::SimResources;

/// Frozen inputs for rebuilding a saved world's resident resources. Caches,
/// process identities, RNG streams and GPU objects are deliberately absent,
/// and so are the map's trigger records: the saved Simulation's trigger
/// runtime is their only owner.
#[derive(Serialize, Deserialize)]
pub struct SavedScenarioResources {
    processed_rules: ProcessedRulesLayers,
    fixed_art_hash: u64,
    scheduler_anim_roots: Vec<String>,
    terrain: SavedTerrainTemplate,
    map: SavedMapMetadata,
}

/// Serialization borrows the sole retained Rules authority. In particular it
/// cannot clone or replay the move-only constructor chronology.
#[derive(Serialize)]
pub struct SavedScenarioResourcesRef<'a> {
    processed_rules: &'a ProcessedRulesLayers,
    fixed_art_hash: u64,
    scheduler_anim_roots: Vec<String>,
    terrain: SavedTerrainTemplate,
    map: SavedMapMetadata,
}

impl<'a> SavedScenarioResourcesRef<'a> {
    pub fn capture(
        resources: &'a SimResources,
        processed_rules: &'a ProcessedRulesLayers,
        fixed_art_hash: u64,
        terrain: SavedTerrainTemplate,
        map: SavedMapMetadata,
    ) -> Self {
        Self {
            processed_rules,
            fixed_art_hash,
            scheduler_anim_roots: resources
                .rules
                .art()
                .scheduler_anim_types()
                .iter()
                .cloned()
                .collect(),
            terrain,
            map,
        }
    }
}

impl SavedScenarioResources {
    pub fn processed_rules(&self) -> &ProcessedRulesLayers {
        &self.processed_rules
    }

    pub fn fixed_art_hash(&self) -> u64 {
        self.fixed_art_hash
    }

    pub fn scheduler_anim_roots(&self) -> &[String] {
        &self.scheduler_anim_roots
    }

    pub fn terrain(&self) -> &SavedTerrainTemplate {
        &self.terrain
    }

    pub fn map(&self) -> &SavedMapMetadata {
        &self.map
    }

    /// Consume the payload into the existing runtime and process owners.
    /// Rules have been projected; the map owner admits its saved values here.
    /// Resident assets and live-cell selector admission follow dynamic restore.
    pub fn into_restored_parts(
        self,
        rules: RuleSet,
        overlay_registry: crate::rules::overlay_types::OverlayTypeRegistry,
        dummy: SharedCellDummy,
    ) -> Result<(SimResources, ProcessedRulesLayers, SavedMapMetadata), SavedTerrainError> {
        let terrain = self.terrain.restore_values(dummy)?;
        Ok((
            SimResources {
                rules,
                terrain_template: Some(terrain),
                overlay_registry,
            },
            self.processed_rules,
            self.map,
        ))
    }
}

/// Map display/source inputs and local match presentation owned by the app.
/// Coordinates, live terrain/object state, theater and lighting timers remain
/// in Simulation.
#[derive(Clone, Serialize, Deserialize)]
pub struct SavedMapMetadata {
    map_hash: u64,
    #[serde(with = "source_identity")]
    source: crate::map::source::LoadedMapSource,
    basic: crate::map::basic::BasicSection,
    ranking: crate::map::ranking::RankingSection,
    terrain_objects: Vec<crate::map::overlay::TerrainObject>,
    house_roster: crate::map::houses::HouseRoster,
    house_colors: crate::map::houses::HouseColorMap,
    lighting: crate::map::lighting::LightingConfig,
    /// ScenarioSave689378 writes the stopped +61C score accumulator.
    elapsed_buckets: u32,
    /// HouseSave504080/HouseLoad503040 retain the already-assigned local name.
    /// Native writer688092 controls: spatial_oracle/naval_house_stats::name_cases.
    local_player_name: Option<String>,
    /// ThemeSave721010's retained/pending pair. The live owner remains in audio.
    theme_slots: [i32; 2],
    /// Rust media reconstruction policy: release this credit because the
    /// physical Radar video queue is not restored. Other pause holds persist.
    radar_pause_credit: bool,
}

impl SavedMapMetadata {
    #[expect(
        clippy::too_many_arguments,
        reason = "capture existing map, presentation, clock and Theme owners at one save boundary"
    )]
    pub fn capture(
        map_hash: u64,
        source: &crate::map::source::LoadedMapSource,
        basic: &crate::map::basic::BasicSection,
        ranking: &crate::map::ranking::RankingSection,
        terrain_objects: &[crate::map::overlay::TerrainObject],
        house_roster: &crate::map::houses::HouseRoster,
        house_colors: &crate::map::houses::HouseColorMap,
        lighting: &crate::map::lighting::LightingConfig,
        elapsed_buckets: u32,
        local_player_name: Option<&str>,
        theme_slots: [i32; 2],
    ) -> Self {
        Self {
            map_hash,
            source: source.clone(),
            basic: basic.clone(),
            ranking: ranking.clone(),
            terrain_objects: terrain_objects.to_vec(),
            house_roster: house_roster.clone(),
            house_colors: house_colors.clone(),
            lighting: lighting.clone(),
            elapsed_buckets,
            local_player_name: local_player_name.map(str::to_owned),
            theme_slots,
            radar_pause_credit: false,
        }
    }

    pub fn map_hash(&self) -> u64 {
        self.map_hash
    }
    pub fn source(&self) -> &crate::map::source::LoadedMapSource {
        &self.source
    }
    pub fn basic(&self) -> &crate::map::basic::BasicSection {
        &self.basic
    }
    pub fn ranking(&self) -> &crate::map::ranking::RankingSection {
        &self.ranking
    }
    pub fn terrain_objects(&self) -> &[crate::map::overlay::TerrainObject] {
        &self.terrain_objects
    }
    pub fn house_roster(&self) -> &crate::map::houses::HouseRoster {
        &self.house_roster
    }
    pub fn house_colors(&self) -> &crate::map::houses::HouseColorMap {
        &self.house_colors
    }
    pub fn lighting(&self) -> &crate::map::lighting::LightingConfig {
        &self.lighting
    }

    pub fn elapsed_buckets(&self) -> u32 {
        self.elapsed_buckets
    }
    pub fn local_player_name(&self) -> Option<&str> {
        self.local_player_name.as_deref()
    }
    pub fn radar_pause_credit(&self) -> bool {
        self.radar_pause_credit
    }
    pub fn set_radar_pause_credit(&mut self, held: bool) {
        self.radar_pause_credit = held;
    }
    pub fn theme_slots(&self) -> [i32; 2] {
        self.theme_slots
    }
}

/// Source identity already has a tagged diagnostic JSON representation. Keep
/// that one schema inside the binary envelope: bincode cannot deserialize its
/// internally tagged enum through `deserialize_any`. No source file is read.
mod source_identity {
    use super::*;

    pub(super) fn serialize<S: serde::Serializer>(
        value: &crate::map::source::LoadedMapSource,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serde_json::to_string(value)
            .map_err(serde::ser::Error::custom)?
            .serialize(serializer)
    }

    pub(super) fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<crate::map::source::LoadedMapSource, D::Error> {
        let json = String::deserialize(deserializer)?;
        serde_json::from_str(&json).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::process_owner::NativeRulesProcessOwner;

    #[test]
    fn saved_resource_wire_restores_without_source_map_or_rule_replay() {
        let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
            IniFile::from_str("[BuildingTypes]\n0=ROOT\n[ROOT]\nStrength=200\n"),
            None,
            IniFile::from_str(""),
            std::sync::Arc::default(),
        )
        .unwrap();
        let loaded = owner
            .load_scenario(
                crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(None),
                &IniFile::from_str("[BuildingTypes]\n1=MAPONLY\n[MAPONLY]\nStrength=37\n"),
            )
            .unwrap();
        let (rules, _, _, _) = loaded.into_parts();
        let expected_hash = rules.simulation_config_hash();
        let mut resources = SimResources::empty();
        resources.rules = rules;
        resources.terrain_template = Some(crate::map::resolved_terrain::test_flat_ground_grid(3));
        let source = crate::map::source::LoadedMapSource::Loose {
            path: "/nonexistent/saved-only-map.map".into(),
            payload_len: 27,
            source_sha256: "saved-source-identity".into(),
        };
        let saved = SavedScenarioResourcesRef::capture(
            &resources,
            owner.saved_scenario_rules().unwrap(),
            owner.fixed_art().content_hash(),
            resources
                .terrain_template
                .as_ref()
                .unwrap()
                .capture_saved_template()
                .unwrap(),
            SavedMapMetadata::capture(
                123,
                &source,
                &Default::default(),
                &Default::default(),
                &[],
                &Default::default(),
                &Default::default(),
                &Default::default(),
                0,
                None,
                [-1, -1],
            ),
        );
        // Exercise the production borrowed writer against the owning reader.
        // JSON round trips would miss tagged-enum/bincode incompatibilities.
        let bytes = crate::util::save_codec::serialize(&saved).unwrap();
        let restored: SavedScenarioResources =
            crate::util::save_codec::deserialize(&bytes).unwrap();
        assert_eq!(restored.map().source(), &source);
        assert_eq!(restored.map().map_hash(), 123);
        assert_eq!(restored.terrain().dimensions(), (3, 3));
        let rules = owner
            .project_saved_scenario_rules(restored.processed_rules())
            .unwrap();
        assert_eq!(rules.simulation_config_hash(), expected_hash);
        let (restored, processed, _) = restored
            .into_restored_parts(
                rules,
                crate::rules::overlay_types::OverlayTypeRegistry::empty(),
                SharedCellDummy::fresh(),
            )
            .unwrap();
        assert_eq!(
            restored.terrain_template.unwrap().cells(),
            resources.terrain_template.as_ref().unwrap().cells()
        );
        assert_eq!(processed.native_type_construction_trace().event_count(), 0);
        assert_eq!(
            owner
                .saved_scenario_rules()
                .unwrap()
                .native_type_construction_trace()
                .event_count(),
            0
        );
    }
}
