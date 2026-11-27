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
use crate::map::source::LoadedMapSource;
use crate::rules::ini_parser::IniFile;
use std::path::Path;

/// Qualified local lookup result, borrowed from the one scenario catalog.
/// No record is fabricated for an unavailable or transferred file.
pub(crate) struct VerifiedNetworkMap<'a> {
    pub record: &'a SkirmishScenarioRecord,
    /// A WOL guest previews only a map read from a mixfile.
    pub source: LoadedMapSource,
}

impl VerifiedNetworkMap<'_> {
    /// The record's title as network text, as a host publishes it
    /// ([`ScenarioCatalog::network_map_metadata`]).
    pub(crate) fn title(&self) -> String {
        network_map_title(&self.record.display_name, &self.record.file_name)
    }
}

// Legacy map headers widen each byte. In network display text, the Windows-1252
// punctuation must be printable Unicode: the lobby rejects C0 and C1 controls.
fn network_map_title(title: &str, file: &str) -> String {
    const C1: [char; 32] = [
        '€', '\0', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\0', 'Ž', '\0', '\0',
        '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\0', 'ž', 'Ÿ',
    ];
    let mut text: String = title
        .chars()
        .map(|ch| match ch {
            '\u{80}'..='\u{9f}' => C1[ch as usize - 0x80],
            _ => ch,
        })
        .filter(|ch| !ch.is_control())
        .collect();
    if text.is_empty() {
        text.push_str(file);
    }
    text.truncate(text.floor_char_boundary(256));
    text
}

fn network_map_digest(ini: &IniFile) -> String {
    // ReadString69536B/5E5339: original literals81BFE0/831898/83189C.
    ini.section_or_empty("Digest")
        .read_string("1", "No digest here mate. Nope.", 0x20)
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

    /// The first row with a file here. Online rows name no local file.
    pub(crate) fn first_local_index(&self) -> Option<usize> {
        self.records.iter().position(|record| !record.is_online())
    }

    pub(crate) fn shell_maps(&self) -> &[MapMenuEntry] {
        &self.shell_maps
    }

    /// Host options describe the selected local record and its physical file.
    /// This never adds a chooser record or treats a filename as proof of bytes.
    /// `ares`: Ares.dll is loaded (the game is read the Ares way), which a
    /// seed's effective digest reads.
    pub(crate) fn network_map_metadata(
        &self,
        file: &str,
        root: &Path,
        assets: &AssetManager,
        ares: bool,
    ) -> anyhow::Result<NetworkMapMetadata> {
        let record = self
            .records
            .iter()
            .find(|record| record.file_name.eq_ignore_ascii_case(file))
            .ok_or_else(|| {
                anyhow::anyhow!("selected network map has no local catalog record: {file}")
            })?;
        let loaded = crate::map::source::load_network_map_ini(root, &record.file_name, assets)?;
        let room_limit = crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT as i32;
        let digest = if crate::map::rmg::is_seed_selection(file) {
            let mut options = crate::map::rmg::RmgOptions::default();
            options.apply_sed(&loaded.ini, ares);
            anyhow::ensure!(
                options.num_players <= room_limit,
                "This random map has {} starts. Choose a seed with at most {room_limit} starts for a network room.",
                options.num_players
            );
            // Seeds carry the generator's option digest (0x005E84D0); the
            // guest reads the same options when it verifies the transferred file.
            options.native_digest()
        } else {
            network_map_digest(&loaded.ini)
        };
        let (length, source_sha256) = loaded
            .source
            .physical_identity()
            .ok_or_else(|| anyhow::anyhow!("selected map has no file identity: {file}"))?;
        Ok(NetworkMapMetadata {
            display_name: network_map_title(&record.display_name, file),
            player_capacity: record.player_capacity.min(room_limit),
            official: record.official,
            byte_length: length as i32,
            digest,
            effective_digest: crate::map::source::effective_network_map_digest(
                &loaded.ini,
                crate::map::rmg::is_seed_selection(file),
                ares,
            ),
            source_sha256: source_sha256.to_owned(),
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
        if file.eq_ignore_ascii_case(crate::map::skirmish_scenarios::RANDMAP_SED) {
            return None;
        }
        for record in self
            .records
            .iter()
            .filter(|record| record.file_name.eq_ignore_ascii_case(file))
        {
            let Ok(loaded) =
                crate::map::source::load_network_map_ini(root, &record.file_name, assets)
            else {
                continue;
            };
            let Some((length, source_sha256)) = loaded.source.physical_identity() else {
                continue;
            };
            if length as i32 != metadata.byte_length {
                continue;
            }
            if !metadata.digest.is_empty() && metadata.digest != network_map_digest(&loaded.ini) {
                continue;
            }
            if !metadata.source_sha256.is_empty() && source_sha256 != metadata.source_sha256 {
                continue;
            }
            if !metadata.effective_digest.is_empty()
                && metadata.effective_digest
                    != crate::map::source::effective_network_map_digest(&loaded.ini, false, false)
            {
                continue;
            }
            return Some(VerifiedNetworkMap {
                record,
                source: loaded.source,
            });
        }
        None
    }

    /// A transferred map has no synthetic chooser record. Its consumed bytes,
    /// native metadata and explicit start capacity must match the accepted map.
    /// `ares`: Ares.dll is loaded (the game is read the Ares way), which a
    /// seed's effective digest reads.
    pub(crate) fn verify_transferred_network_map(
        loaded: &crate::map::source::LoadedMapIni,
        metadata: &NetworkMapMetadata,
        seed: bool,
        ares: bool,
    ) -> bool {
        if crate::map::source::validate_network_map_content(&loaded.ini, seed).is_err() {
            return false;
        }
        let (digest, capacity) = if seed {
            let mut options = crate::map::rmg::RmgOptions::default();
            options.apply_sed(&loaded.ini, ares);
            // RmgOptions::native_digest owns the option-block fold5E84D0;
            // compare consumed values before generation normalization. The
            // separate effective-map digest already normalizes its input.
            let digest = options.native_digest();
            options.normalize();
            if options.num_players > crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT as i32 {
                return false;
            }
            (digest, options.num_players)
        } else {
            (
                network_map_digest(&loaded.ini),
                crate::map::waypoints::skirmish_player_capacity(&loaded.ini)
                    .min(crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT as i32),
            )
        };
        loaded
            .source
            .physical_identity()
            .is_some_and(|(length, digest)| {
                length == usize::try_from(metadata.byte_length).unwrap_or(0)
                    && crate::map::source::valid_source_sha256(digest)
                    && digest == metadata.source_sha256
            })
            && (metadata.digest.is_empty() || metadata.digest == digest)
            && capacity == metadata.player_capacity
            && metadata.effective_digest
                == crate::map::source::effective_network_map_digest(&loaded.ini, seed, ares)
    }

    /// `ares`: Ares.dll is loaded (the game is read the Ares way); see
    /// [`Self::verify_transferred_network_map`].
    pub(crate) fn verify_network_map_input(
        &self,
        file: &str,
        metadata: &NetworkMapMetadata,
        root: &Path,
        assets: &AssetManager,
        ares: bool,
    ) -> Option<crate::map::source::LoadedMapIni> {
        crate::map::source::validate_network_map_name(file).ok()?;
        let seed = crate::map::rmg::is_seed_selection(file);
        let verified = |name: &str| {
            let loaded = crate::map::source::load_network_map_ini(root, name, assets).ok()?;
            Self::verify_transferred_network_map(&loaded, metadata, seed, ares).then_some(loaded)
        };
        verified(file).or_else(|| {
            // VERA: the same map under another name or case, as a map received
            // or downloaded earlier is kept (`keep_received_map`): a loose map
            // in the game folder or VERA's Maps folder whose bytes verify.
            // Only files of the host's length are read.
            if seed {
                return None;
            }
            let length = u64::try_from(metadata.byte_length).ok()?;
            crate::map::source::mod_dir()
                .into_iter()
                .chain(std::iter::once(root))
                .chain(crate::map::source::downloaded_maps_dir())
                .filter_map(|dir| crate::util::file_system::read_dir(dir).ok())
                .flatten()
                .filter_map(|entry| Some(entry.ok()?.path()))
                .filter(|path| {
                    crate::map::source::is_loose_map_name(path)
                        && crate::util::file_system::metadata(path)
                            .is_ok_and(|meta| meta.is_file() && meta.len() == length)
                })
                .find_map(|path| verified(path.to_str()?))
        })
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

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;
    use crate::map::skirmish_scenarios::SkirmishScenarioSource;
    use crate::map::source::test_support::TestDirectory;

    #[test]
    fn first_local_index_skips_online_rows() {
        let record = |online: bool| {
            let mut record = SkirmishScenarioRecord::random_map_sentinel(0, "Map", 4);
            if online {
                record.source = SkirmishScenarioSource::Online("a".repeat(64));
            }
            record
        };
        let index = |rows| ScenarioCatalog::from_records(rows).first_local_index();
        assert_eq!(index(vec![record(true), record(false)]), Some(1));
        assert_eq!(index(vec![record(true)]), None);
    }

    #[test]
    fn the_random_map_record_publishes_the_generator_option_digest() {
        let root = TestDirectory::new("network-random-map-metadata");
        let mut options = crate::map::rmg::RmgOptions {
            seed: 4321,
            num_players: 4,
            ..crate::map::rmg::RmgOptions::default()
        };
        options.normalize();
        let bytes = options.to_sed_bytes(false);
        root.write("RandMap.Sed", &bytes);
        let assets = AssetManager::from_loose_root_for_test(root.path());
        let catalog =
            ScenarioCatalog::from_records(vec![SkirmishScenarioRecord::random_map_sentinel(
                0,
                "Random Map",
                4,
            )]);
        let metadata = catalog
            .network_map_metadata("RandMap.Sed", root.path(), &assets, false)
            .unwrap();
        assert_eq!(metadata.digest, options.native_digest());
        assert_eq!(metadata.byte_length, bytes.len() as i32);
        assert!(metadata.official);
        assert!(
            catalog
                .verify_network_map_input("RandMap.Sed", &metadata, root.path(), &assets, false)
                .is_some()
        );
        assert!(
            catalog
                .verify_network_map_input(
                    root.path().join("RandMap.Sed").to_str().unwrap(),
                    &metadata,
                    root.path(),
                    &assets,
                    false,
                )
                .is_none(),
            "a remote logical name must not select a host absolute path"
        );
        // External SED bytes may precede the generator's normalization.
        // Preserve the exact option-block digest while agreeing separately on
        // the normalized effective input, as normal UI generation also does.
        let mut raw = options.clone();
        raw.tiberium = 0;
        let bytes = raw.to_sed_bytes(false);
        root.write("RandMap.Sed", &bytes);
        let metadata = catalog
            .network_map_metadata("RandMap.Sed", root.path(), &assets, false)
            .unwrap();
        assert_ne!(metadata.digest, options.native_digest());
        assert_eq!(metadata.digest, raw.native_digest());
        assert_eq!(
            metadata.effective_digest,
            crate::map::source::effective_network_map_digest(
                &IniFile::from_bytes(&bytes).unwrap(),
                true,
                false
            )
        );
        assert!(
            catalog
                .verify_network_map_input("RandMap.Sed", &metadata, root.path(), &assets, false)
                .is_some()
        );
        let mut wrong = metadata.clone();
        wrong.digest = options.native_digest();
        assert!(
            catalog
                .verify_network_map_input("RandMap.Sed", &wrong, root.path(), &assets, false)
                .is_none()
        );
        wrong = metadata;
        wrong.effective_digest = "0".repeat(64);
        assert!(
            catalog
                .verify_network_map_input("RandMap.Sed", &wrong, root.path(), &assets, false)
                .is_none()
        );
        options.num_players = 120;
        root.write("RandMap.Sed", &options.to_sed_bytes(false));
        assert!(
            catalog
                .network_map_metadata("RandMap.Sed", root.path(), &assets, false)
                .is_err()
        );
    }

    #[test]
    fn extended_authored_map_advertises_and_verifies_the_network_capacity() {
        use std::fmt::Write;
        let root = TestDirectory::new("extended-network-map");
        let mut map = String::from(
            "[Map]\nTheater=TEMPERATE\nSize=0,0,2,1\nLocalSize=0,0,2,1\n[IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n[Basic]\nMaxPlayers=120\n[Waypoints]\n",
        );
        for slot in 0..120 {
            writeln!(map, "{slot}={}", 1001 + slot).unwrap();
        }
        root.write("Wide.map", map.as_bytes());
        let assets = AssetManager::from_loose_root_for_test(root.path());
        let record = SkirmishScenarioRecord::concrete_from_ini(
            0,
            SkirmishScenarioSource::Synthetic,
            "Wide.map",
            &IniFile::from_str(&map),
        );
        assert_eq!(record.player_capacity, 120);
        let catalog = ScenarioCatalog::from_records(vec![record]);
        let metadata = catalog
            .network_map_metadata("Wide.map", root.path(), &assets, false)
            .unwrap();
        assert_eq!(metadata.player_capacity, 30);
        assert!(
            catalog
                .verify_network_map_input("Wide.map", &metadata, root.path(), &assets, false)
                .is_some()
        );
    }

    #[test]
    fn a_guest_finds_its_copy_of_the_hosts_map_under_another_name() {
        let map: &[u8] = b"[Map]\nTheater=TEMPERATE\nSize=0,0,2,1\nLocalSize=0,0,2,1\n[IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n";
        let host = TestDirectory::new("kept-name-host");
        host.write("Lakes.map", map);
        let host_assets = AssetManager::from_loose_root_for_test(host.path());
        let record = SkirmishScenarioRecord::concrete_from_ini(
            0,
            SkirmishScenarioSource::Synthetic,
            "Lakes.map",
            &IniFile::from_bytes(map).unwrap(),
        );
        let metadata = ScenarioCatalog::from_records(vec![record])
            .network_map_metadata("Lakes.map", host.path(), &host_assets, false)
            .unwrap();

        let guest = TestDirectory::new("kept-name-guest");
        let guest_assets = AssetManager::from_loose_root_for_test(guest.path());
        let catalog = ScenarioCatalog::from_records(Vec::new());
        guest.write("Lakes.yrm", b"[Map]\nTheater=SNOW\n");
        assert!(
            catalog
                .verify_network_map_input(
                    "Lakes.map",
                    &metadata,
                    guest.path(),
                    &guest_assets,
                    false
                )
                .is_none(),
            "another map under the kept name is not the host's"
        );
        // Kept under another name and case, as a copy beside another map of
        // its name is.
        guest.write("lakes-1A2B.YRM", map);
        assert!(
            catalog
                .verify_network_map_input(
                    "Lakes.map",
                    &metadata,
                    guest.path(),
                    &guest_assets,
                    false
                )
                .is_some()
        );
    }

    #[test]
    fn network_availability_matches_registered_name_size_and_native_digest_reader() {
        let root = TestDirectory::new("network-map-metadata");
        let bytes = b"[Basic]\nName=Commander\x92s map\n[Digest]\n1=CaseSensitive\n";
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
            .network_map_metadata("controlled.map", root.path(), &assets, false)
            .unwrap();
        assert_eq!(metadata.display_name, "Commander’s map");
        assert_eq!(metadata.byte_length, bytes.len() as i32);
        let found = catalog
            .verify_network_map("controlled.map", &metadata, root.path(), &assets)
            .unwrap();
        assert_eq!(found.record.file_name, "CONTROLLED.MAP");
        assert_eq!(found.title(), metadata.display_name);
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
