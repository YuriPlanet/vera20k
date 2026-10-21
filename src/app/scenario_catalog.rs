//! Scenario catalog (F11): one owner for the skirmish scenario records and
//! the shell map entries projected from them.
//!
//! Before this owner, `skirmish_scenario_records` and `skirmish_shell_maps`
//! were two parallel `AppState` vectors reconciled by hand: the random-map
//! flow mutated the records and then patched the projected list separately by
//! name-position, and nothing else re-projected — any future record mutation
//! silently broke the projection. Here the projection is derived state: reads
//! are borrows, and the only mutable access re-projects when it drops, so the
//! two lists cannot drift.

use crate::app::loading::init::MapMenuEntry;
use crate::app::match_runtime::session::NetworkMapMetadata;
use crate::assets::asset_manager::AssetManager;
use crate::map::skirmish_scenarios::SkirmishScenarioRecord;
use crate::map::source::{LoadedMapSource, load_map_ini_by_name_or_path_with_assets};
use crate::rules::ini_parser::IniFile;
use std::path::Path;

/// Qualified local lookup result, borrowed from the one scenario catalog.
/// No record is fabricated for an unavailable or transferred file.
pub(crate) struct VerifiedNetworkMap<'a> {
    pub record: &'a SkirmishScenarioRecord,
    /// A WOL guest previews only a map read from a mixfile.
    pub source: LoadedMapSource,
}

fn network_map_digest(ini: &IniFile) -> String {
    // ReadString69536B/5E5339: original literals81BFE0/831898/83189C.
    ini.section_or_empty("Digest")
        .read_string("1", "No digest here mate. Nope.", 0x20)
}

fn physical_map_length(source: &LoadedMapSource) -> i32 {
    match source {
        LoadedMapSource::Loose { payload_len, .. } | LoadedMapSource::Mix { payload_len, .. } => {
            *payload_len as u32 as i32
        }
        _ => unreachable!("metadata reader returns only physical source bytes"),
    }
}

pub(crate) struct ScenarioCatalog {
    records: Vec<SkirmishScenarioRecord>,
    /// Always exactly `records.iter().map(to_map_menu_entry)`.
    shell_maps: Vec<MapMenuEntry>,
}

impl ScenarioCatalog {
    pub(crate) fn from_records(records: Vec<SkirmishScenarioRecord>) -> Self {
        let mut catalog = Self {
            records,
            shell_maps: Vec::new(),
        };
        catalog.reproject();
        catalog
    }

    pub(crate) fn records(&self) -> &[SkirmishScenarioRecord] {
        &self.records
    }

    pub(crate) fn shell_maps(&self) -> &[MapMenuEntry] {
        &self.shell_maps
    }

    /// Host options describe the selected local record and its physical file.
    /// This never adds a chooser record or treats a filename as proof of bytes.
    pub(crate) fn network_map_metadata(
        &self,
        file: &str,
        root: &Path,
        assets: &AssetManager,
    ) -> anyhow::Result<NetworkMapMetadata> {
        let record = self
            .records
            .iter()
            .find(|record| record.file_name.eq_ignore_ascii_case(file))
            .ok_or_else(|| {
                anyhow::anyhow!("selected network map has no local catalog record: {file}")
            })?;
        let loaded = load_map_ini_by_name_or_path_with_assets(root, &record.file_name, assets)?;
        let digest = if record
            .file_name
            .eq_ignore_ascii_case(crate::map::skirmish_scenarios::RANDMAP_SED)
        {
            // The random-map record carries 0x005E84D0's option digest
            // (0x005E8590), not a [Digest]; the .Sed holds the same block.
            let mut options = crate::map::rmg::RmgOptions::default();
            options.apply_sed(&loaded.ini);
            options.native_digest()
        } else {
            network_map_digest(&loaded.ini)
        };
        Ok(NetworkMapMetadata {
            display_name: record.display_name.clone(),
            player_capacity: record.player_capacity,
            official: record.official,
            byte_length: physical_map_length(&loaded.source),
            digest,
        })
    }

    ///6951F0 and ParsePacket5E51F8: registered name, existence, exact physical
    /// length, then case-sensitive [Digest]1 text. No computed checksum is used.
    /// CD prompting is a service boundary; unavailable physical sources returnNone.
    pub(crate) fn verify_network_map(
        &self,
        file: &str,
        metadata: &NetworkMapMetadata,
        root: &Path,
        assets: &AssetManager,
    ) -> Option<VerifiedNetworkMap<'_>> {
        if file.eq_ignore_ascii_case("RandMap.Sed") {
            return None;
        }
        for record in self
            .records
            .iter()
            .filter(|record| record.file_name.eq_ignore_ascii_case(file))
        {
            let Ok(loaded) =
                load_map_ini_by_name_or_path_with_assets(root, &record.file_name, assets)
            else {
                continue;
            };
            if physical_map_length(&loaded.source) != metadata.byte_length {
                continue;
            }
            if !metadata.digest.is_empty() && metadata.digest != network_map_digest(&loaded.ini) {
                continue;
            }
            return Some(VerifiedNetworkMap {
                record,
                source: loaded.source,
            });
        }
        None
    }

    /// Mutable access to the records. The guard re-projects the shell map
    /// entries when it drops, so every mutation path — including the modal's
    /// random-map sentinel upsert — keeps the projection exact.
    pub(crate) fn records_mut(&mut self) -> RecordsMut<'_> {
        RecordsMut { catalog: self }
    }

    fn reproject(&mut self) {
        self.shell_maps = self
            .records
            .iter()
            .map(SkirmishScenarioRecord::to_map_menu_entry)
            .collect();
    }
}

pub(crate) struct RecordsMut<'a> {
    catalog: &'a mut ScenarioCatalog,
}

impl std::ops::Deref for RecordsMut<'_> {
    type Target = Vec<SkirmishScenarioRecord>;
    fn deref(&self) -> &Self::Target {
        &self.catalog.records
    }
}

impl std::ops::DerefMut for RecordsMut<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.catalog.records
    }
}

impl Drop for RecordsMut<'_> {
    fn drop(&mut self) {
        self.catalog.reproject();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::skirmish_scenarios::SkirmishScenarioSource;
    use crate::map::source::test_support::TestDirectory;

    #[test]
    fn the_random_map_record_publishes_the_generator_option_digest() {
        let root = TestDirectory::new("network-random-map-metadata");
        let options = crate::map::rmg::RmgOptions {
            seed: 4321,
            ..crate::map::rmg::RmgOptions::default()
        };
        let bytes = options.to_sed_bytes();
        root.write("RandMap.Sed", &bytes);
        let assets = AssetManager::from_loose_root_for_test(root.path());
        let catalog =
            ScenarioCatalog::from_records(vec![SkirmishScenarioRecord::random_map_sentinel(
                0,
                "Random Map",
                4,
            )]);
        let metadata = catalog
            .network_map_metadata("RandMap.Sed", root.path(), &assets)
            .unwrap();
        assert_eq!(metadata.digest, options.native_digest());
        assert_eq!(metadata.byte_length, bytes.len() as i32);
        assert!(metadata.official);
    }

    #[test]
    fn network_availability_matches_registered_name_size_and_native_digest_reader() {
        let root = TestDirectory::new("network-map-metadata");
        let bytes = b"[Basic]\nName=Local title\n[Digest]\n1=CaseSensitive\n";
        root.write("CONTROLLED.MAP", bytes);
        let assets = AssetManager::from_loose_root_for_test(root.path());
        let ini = IniFile::from_bytes(bytes).unwrap();
        let record = SkirmishScenarioRecord::concrete_from_ini(
            0,
            SkirmishScenarioSource::Synthetic,
            "CONTROLLED.MAP",
            &ini,
        );
        let catalog = ScenarioCatalog::from_records(vec![record]);
        let metadata = catalog
            .network_map_metadata("controlled.map", root.path(), &assets)
            .unwrap();
        assert_eq!(metadata.display_name, "Local title");
        assert_eq!(metadata.byte_length, bytes.len() as i32);
        let found = catalog
            .verify_network_map("controlled.map", &metadata, root.path(), &assets)
            .unwrap();
        assert_eq!(found.record.file_name, "CONTROLLED.MAP");
        assert!(matches!(found.source, LoadedMapSource::Loose { .. }));
        let mut wrong = metadata.clone();
        wrong.byte_length += 1;
        assert!(
            catalog
                .verify_network_map("CONTROLLED.MAP", &wrong, root.path(), &assets)
                .is_none()
        );
        wrong = metadata.clone();
        wrong.digest.make_ascii_lowercase();
        assert!(
            catalog
                .verify_network_map("CONTROLLED.MAP", &wrong, root.path(), &assets)
                .is_none()
        );
        wrong.digest.clear();
        assert!(
            catalog
                .verify_network_map("CONTROLLED.MAP", &wrong, root.path(), &assets)
                .is_some()
        );
        let empty = ScenarioCatalog::from_records(Vec::new());
        assert!(
            empty
                .verify_network_map("CONTROLLED.MAP", &metadata, root.path(), &assets)
                .is_none(),
            "a loose file alone is not6951F0 catalog admission"
        );
        assert_eq!(
            network_map_digest(&IniFile::empty()),
            "No digest here mate. Nope."
        );
        assert_eq!(
            network_map_digest(&IniFile::from_str(
                "[Digest]\n1=12345678901234567890123456789012345\n"
            ))
            .len(),
            31
        );
    }
}
