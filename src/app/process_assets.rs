//! Process-wide asset ownership (F11): one `AssetManager` for the process,
//! leased to the loading pipeline and always returned.
//!
//! Retail keeps one process-global MIX list plus the sticky
//! `LoadFileFromMIX` CRC cache and the active theater archive group. All of
//! that state lives on the manager, so dropping or reconstructing it
//! mid-process silently discards process-sticky semantics. The slot makes the
//! lifecycle explicit:
//!
//! - `Available` — the manager is home; shell, audio, render, and capture
//!   borrow it in place.
//! - `Loading` — leased into the loading job; every loading outcome
//!   (success, failure, cancellation) must route the manager back through
//!   `return_from_loading`.
//! - Absent — startup could not open the retail archives; lookups no-op.
//!
//! A double return keeps the resident manager and logs instead of silently
//! replacing process-sticky state.

use crate::assets::asset_manager::{AssetManager, MediaArchiveMode};

pub(crate) struct ProcessAssets {
    /// Selected at launch, retained even if initial archive loading fails.
    media_archive_mode: MediaArchiveMode,
    /// Launch request and its first resolved names survive a lost asset lease.
    ini_config: crate::util::ini_roots::IniRootConfig,
    selected_ini_roots: Option<crate::util::ini_roots::IniRoots>,
    manager: Option<AssetManager>,
    leased: bool,
    /// Frozen launch policy, including deferred recovery after assetless startup.
    load_audio_index: bool,
    /// None means sources have not been selected. A selected empty catalog is
    /// still final for this process; later maps cannot replace its definitions.
    audio_catalog: Option<ProcessAudioCatalog>,
    /// One process-resident native Rules registry authority. Unlike the MIX
    /// manager it never leaves this owner: scenario transitions mutate it
    /// synchronously in place, so later failures cannot drop or roll it back.
    native_rules: Option<crate::rules::process_owner::NativeRulesProcessOwner>,
    /// CampaignClass's retained array. Init_Game52C605 creates it before
    /// Load_Game_Rules selects Movies; ClearScene685609 rereads its sources.
    campaigns: Option<crate::rules::campaigns::CampaignRegistry>,
    /// MapClass's one process-global fallback CellClass (`0x00ABDC50`). Map
    /// reloads bind their resolved grid to this same live identity.
    pub(crate) shared_cell_dummy: crate::map::resolved_terrain::SharedCellDummy,
    /// CSF string table — localized display names for units, buildings, UI
    /// text. Loaded once at startup from the retail archives; process-wide.
    pub(crate) csf: Option<crate::assets::csf_file::CsfFile>,
    /// Process-persistent terrain-load cache. Scenario teardown, failed
    /// loads, reseeds, and save transitions must not clear it.
    pub(crate) tile_variant_selector_cache:
        crate::map::tile_variant_selector::TileVariantSelectorCache,
    /// The network games' Rules/AI/Art INI hashes, computed once from the
    /// process sources; `None` without them.
    native_ini_hashes: Option<crate::rules::native_ini_hash::NativeIniHashes>,
}

/// Immutable process sources; output players and match queues have separate
/// lifetimes. Rules binding shares this exact SOUNDMD allocation.
pub(crate) struct ProcessAudioCatalog {
    definitions: crate::rules::audio_sources::AudioDefinitions,
    index: Option<crate::assets::asset_manager::LoadedAudioIndex>,
}

impl ProcessAudioCatalog {
    pub(crate) fn sounds(&self) -> &crate::rules::sound_ini::SoundRegistry {
        self.definitions.sounds()
    }

    pub(crate) fn eva(&self) -> &crate::rules::sound_ini::EvaRegistry {
        self.definitions.eva()
    }

    pub(crate) fn index(&self) -> Option<&crate::assets::audio_bag::AudioIndex> {
        self.index.as_ref().map(|loaded| &loaded.index)
    }
}

impl ProcessAssets {
    pub(crate) fn new(media_archive_mode: MediaArchiveMode, load_audio_index: bool) -> Self {
        Self {
            media_archive_mode,
            ini_config: Default::default(),
            selected_ini_roots: None,
            manager: None,
            leased: false,
            load_audio_index,
            audio_catalog: None,
            native_rules: None,
            campaigns: None,
            shared_cell_dummy: Default::default(),
            csf: None,
            tile_variant_selector_cache: Default::default(),
            native_ini_hashes: None,
        }
    }

    pub(crate) fn new_with_ini_config(
        media: MediaArchiveMode,
        audio: bool,
        config: crate::util::ini_roots::IniRootConfig,
    ) -> Self {
        let mut process = Self::new(media, audio);
        process.ini_config = config;
        process
    }

    /// Recovery uses the process's already resolved choice, never a new Auto
    /// decision against an archive set that may have changed since startup.
    pub(crate) fn reconstruct_manager(
        &self,
        root: &std::path::Path,
    ) -> anyhow::Result<AssetManager> {
        let assets = AssetManager::new(root, self.media_archive_mode)?;
        Ok(if let Some(roots) = &self.selected_ini_roots {
            assets.with_selected_ini_roots(roots.clone())
        } else {
            assets.with_ini_roots(&self.ini_config)?
        })
    }

    /// Init_Game52C763/52C796 and52C843/52C8A0 select fixed SOUNDMD/EVAMD
    /// before the ordinary shell/scenario loop. See tools/audio_catalog_owner.md.
    /// Recovery uses this same boundary before request preparation. A failed
    /// Rules load retains the audio selection; a live Rules registry is never
    /// replaced or rolled back. VERA retains its tolerant missing-audio policy.
    /// Returns shell compatibility projections only when Rules is first created.
    pub(crate) fn initialize_sources_if_needed(
        &mut self,
        assets: &AssetManager,
    ) -> Result<
        (
            Option<crate::rules::ruleset::RuleSet>,
            Option<crate::rules::ini_parser::IniFile>,
        ),
        String,
    > {
        if self
            .selected_ini_roots
            .as_ref()
            .is_some_and(|roots| roots != assets.ini_roots())
        {
            return Err("process INI roots changed after source selection".into());
        }
        self.selected_ini_roots
            .get_or_insert_with(|| assets.ini_roots().clone());
        if self.campaigns.is_none() {
            self.reload_campaigns(assets);
        }
        if self.audio_catalog.is_none() {
            let index = if self.load_audio_index {
                assets
                    .load_audio_index()
                    .map_err(|error| log::warn!("Process audio index unavailable: {error}"))
                    .ok()
                    .flatten()
            } else {
                None
            };
            let definitions = crate::rules::audio_sources::AudioDefinitions::select(assets);
            let source_receipt = serde_json::json!({
                "soundmd": definitions.sound_source(),
                "evamd": definitions.eva_source(),
                "index_enabled": self.load_audio_index,
                "index": index.as_ref().map(|loaded| &loaded.sources),
                "sound_count": definitions.sounds().len(),
                "eva_count": definitions.eva().len(),
            });
            #[cfg(feature = "ares")]
            let source_receipt = {
                let mut receipt = source_receipt;
                receipt["additional_indexes"] =
                    serde_json::json!(index.as_ref().map(|loaded| &loaded.additional_sources));
                receipt
            };
            log::info!("Process audio catalog selected: {source_receipt}");
            self.audio_catalog = Some(ProcessAudioCatalog { definitions, index });
        }
        if self.native_rules.is_some() {
            return Ok((None, None));
        }
        let sounds =
            std::sync::Arc::clone(self.audio_catalog.as_ref().unwrap().definitions.sounds());
        let eva = std::sync::Arc::clone(self.audio_catalog.as_ref().unwrap().definitions.eva());
        let startup = crate::rules::retail_sources::RetailRulesSources::select(assets)?
            .into_startup(sounds, eva)?;
        let (rules, projection, owner) = startup.into_parts();
        self.native_rules = Some(owner);
        Ok((rules, projection))
    }

    pub(crate) fn audio_catalog(&self) -> Option<&ProcessAudioCatalog> {
        self.audio_catalog.as_ref()
    }

    /// Init_Game52C605/ClearScene685609 use fresh local INI objects while keeping the
    /// campaign array. This selected retail route reads BATTLEMD.INI through
    /// the same archive/loose-file resolver as the other process sources.
    pub(crate) fn reload_campaigns(&mut self, assets: &AssetManager) {
        self.apply_campaign_source(crate::rules::retail_sources::select_ini(
            assets,
            "BATTLEMD.INI",
        ));
    }

    /// PrepareSession52DF25 retries Load_Campaigns when the retained array is empty.
    pub(crate) fn refresh_empty_campaigns_if_available(&mut self) {
        if self
            .campaigns
            .as_ref()
            .is_some_and(|campaigns| !campaigns.is_empty())
        {
            return;
        }
        let selected = self
            .manager
            .as_ref()
            .map(|assets| crate::rules::retail_sources::select_ini(assets, "BATTLEMD.INI"));
        if let Some(selected) = selected {
            self.apply_campaign_source(selected);
        }
    }

    fn apply_campaign_source(
        &mut self,
        selected: Result<crate::rules::retail_sources::SelectedIni, String>,
    ) {
        let registry = self.campaigns.get_or_insert_with(Default::default);
        let selected = match selected {
            Ok(selected) => selected,
            Err(error) => {
                log::warn!("Campaign catalog unavailable: {error}");
                return;
            }
        };
        let empty_movies = crate::rules::movies::MovieRegistry::default();
        let movies = self
            .native_rules
            .as_ref()
            .map_or(&empty_movies, |owner| owner.movies());
        registry.apply_ini(&selected.ini, movies);
        log::info!(
            "Campaign catalog read: {}",
            serde_json::json!(selected.source)
        );
    }

    pub(crate) fn campaigns(&self) -> Option<&crate::rules::campaigns::CampaignRegistry> {
        self.campaigns.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn media_archive_mode(&self) -> MediaArchiveMode {
        self.media_archive_mode
    }

    pub(crate) fn has_native_rules(&self) -> bool {
        self.native_rules.is_some()
    }

    pub(crate) fn native_rules(
        &self,
    ) -> Option<&crate::rules::process_owner::NativeRulesProcessOwner> {
        self.native_rules.as_ref()
    }

    pub(crate) fn native_ini_hashes(
        &self,
    ) -> Option<crate::rules::native_ini_hash::NativeIniHashes> {
        self.native_ini_hashes
    }

    /// Hash the process sources once (`crate::rules::native_ini_hash`):
    /// the selected rules root and ART, AIMD, each mode's INI by id
    /// `1..=count` and CoopCampMD.
    pub(crate) fn install_native_ini_hashes(
        &mut self,
        assets: &AssetManager,
        modes: &[crate::skirmish_modes::SkirmishGameMode],
    ) {
        use crate::rules::ini_parser::IniFile;
        use crate::rules::retail_sources::{select_ini, select_mode_ini};
        let Some(rules) = self.native_rules.as_ref() else {
            return;
        };
        let ai = match select_ini(assets, assets.ini_roots().ai()) {
            Ok(selected) => selected.ini,
            Err(error) => {
                log::warn!("No INI hashes without the AI root: {error}");
                return;
            }
        };
        // 0x00679D90 walks the ids 1..=count (0x005D5ED0) and finds each
        // mode by id (0x005D5F30); 0x005D67B0 gives an empty INI for a
        // missing file. Retail ids are 1..9 without a gap.
        let mode_inis: Vec<IniFile> = (1..=modes.len() as i32)
            .filter_map(|id| crate::skirmish_modes::mode_by_id(modes, id))
            .map(|mode| {
                select_mode_ini(assets, Some(&mode.override_file))
                    .ok()
                    .flatten()
                    .unwrap_or_else(IniFile::empty)
            })
            .collect();
        let cooperative = select_ini(
            assets,
            crate::app::frontend::skirmish_session::cooperative::COOPERATIVE_CAMPAIGN_INI,
        )
        .ok()
        .map(|selected| selected.ini);
        self.native_ini_hashes = Some(crate::rules::native_ini_hash::NativeIniHashes::compute(
            rules.selected_rules_root(),
            &mode_inis,
            cooperative.as_ref(),
            &ai,
            rules.fixed_art(),
        ));
    }

    pub(crate) fn read_multiplayer_country(
        &mut self,
        settings: &crate::rules::ini_parser::IniFile,
        current: i32,
    ) -> i32 {
        self.native_rules.as_mut().map_or(current, |rules| {
            rules.read_multiplayer_country(settings, current)
        })
    }

    pub(crate) fn refresh_multiplayer_country_bodies(&mut self) {
        if let Some(rules) = self.native_rules.as_mut() {
            rules.refresh_multiplayer_country_bodies();
        }
    }

    pub(crate) fn multiplayer_countries(&self) -> impl Iterator<Item = (&str, Option<&str>)> {
        self.native_rules
            .iter()
            .flat_map(|rules| rules.multiplayer_countries())
    }

    pub(crate) fn project_multiplayer_countries(
        &self,
        projection: &mut crate::rules::ruleset::RuleSet,
    ) {
        if let Some(rules) = self.native_rules.as_ref() {
            rules.project_multiplayer_countries(projection);
        }
    }

    /// Split the process-resident authorities used by a synchronous
    /// scenario load. The leased MIX manager lives in `LoadingJob`; the native
    /// Rules registry remains here so every `?` after its destructive reset
    /// preserves the actual live state.
    pub(crate) fn native_rules_mut_with_tile_cache(
        &mut self,
    ) -> (
        Option<&mut crate::rules::process_owner::NativeRulesProcessOwner>,
        &mut crate::map::tile_variant_selector::TileVariantSelectorCache,
        Option<&ProcessAudioCatalog>,
    ) {
        (
            self.native_rules.as_mut(),
            &mut self.tile_variant_selector_cache,
            self.audio_catalog.as_ref(),
        )
    }

    /// Shared borrow while the manager is home (`Available`).
    pub(crate) fn manager(&self) -> Option<&AssetManager> {
        self.manager.as_ref()
    }

    /// Field-level split borrow: the resident manager and the terrain-variant
    /// cache live on the same owner, and the RMG preview path needs both at
    /// once.
    pub(crate) fn manager_and_rules_with_tile_cache(
        &mut self,
    ) -> (
        Option<&mut AssetManager>,
        Option<&crate::rules::process_owner::NativeRulesProcessOwner>,
        &mut crate::map::tile_variant_selector::TileVariantSelectorCache,
    ) {
        (
            self.manager.as_mut(),
            self.native_rules.as_ref(),
            &mut self.tile_variant_selector_cache,
        )
    }

    pub(crate) fn is_available(&self) -> bool {
        self.manager.is_some()
    }

    /// True while a lease is outstanding — distinguishes `Loading` (a manager
    /// existed and went out) from Absent (startup never constructed one), so
    /// the reconstruction path only warns about a real loss.
    pub(crate) fn is_leased(&self) -> bool {
        self.leased
    }

    /// `Available -> Loading`. Returns `None` when the slot is absent or the
    /// manager is already leased out.
    pub(crate) fn lease_for_loading(&mut self) -> Option<AssetManager> {
        let leased = self.manager.take();
        if leased.is_some() {
            self.leased = true;
        }
        leased
    }

    /// `Loading -> Available`. Every loading outcome must come back through
    /// here so the sticky CRC cache and active theater identity survive the
    /// process. A double return keeps the resident manager and logs.
    pub(crate) fn return_from_loading(&mut self, manager: AssetManager) {
        self.leased = false;
        if self.manager.is_some() {
            log::error!(
                "ProcessAssets: manager returned while one is already resident; \
                 keeping the resident manager (process-sticky state preserved)"
            );
            return;
        }
        self.manager = Some(manager);
    }

    /// A lease ended without a manager to return (the loading job lost it or
    /// reconstructed elsewhere). Clears the lease so the slot can lease again;
    /// the loss itself is the caller's anomaly to log.
    pub(crate) fn note_lease_ended_without_return(&mut self) {
        self.leased = false;
    }
}

#[cfg(test)]
mod tests {
    use super::{MediaArchiveMode, ProcessAssets};
    use crate::assets::asset_manager::AssetManager;

    #[test]
    fn ini_roots_process_recovery_keeps_auto_choice_and_refuses_a_foreign_triplet() {
        use crate::util::ini_roots::{IniProfile, IniRootConfig};
        let dir =
            crate::assets::asset_manager::test_support::audio_test_install("ini-roots-process");
        std::fs::write(
            dir.path().join("rulesmo.ini"),
            b"[General]\nBuildSpeed=.7\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("artmo.ini"), b"[TEST]\nVoxel=yes\n").unwrap();
        std::fs::write(dir.path().join("aimo.ini"), b"[Marker]\nValue=MO\n").unwrap();
        let config = IniRootConfig {
            profile: IniProfile::Auto,
            ..Default::default()
        };
        let manager = AssetManager::new(dir.path(), MediaArchiveMode::STOCK_DIGITAL)
            .unwrap()
            .with_ini_roots(&config)
            .unwrap();
        let mut process = ProcessAssets::new_with_ini_config(
            MediaArchiveMode::STOCK_DIGITAL,
            false,
            config.clone(),
        );
        process.initialize_sources_if_needed(&manager).unwrap();
        process.return_from_loading(manager);
        drop(process.lease_for_loading().unwrap());
        process.note_lease_ended_without_return();
        for name in ["rulesmd.ini", "artmd.ini", "aimd.ini"] {
            std::fs::write(dir.path().join(name), b"[Late]\nValue=YR\n").unwrap();
        }
        let fresh = AssetManager::new(dir.path(), MediaArchiveMode::STOCK_DIGITAL).unwrap();
        assert!(process.initialize_sources_if_needed(&fresh).is_err());
        assert!(
            fresh.with_ini_roots(&config).is_err(),
            "new Auto choice would now be ambiguous"
        );
        let recovered = process.reconstruct_manager(dir.path()).unwrap();
        assert_eq!(recovered.ini_roots().profile(), IniProfile::Mo);
        process.initialize_sources_if_needed(&recovered).unwrap();
        assert_eq!(
            process
                .native_rules()
                .unwrap()
                .fixed_art()
                .section_or_empty("TEST")
                .read_bool("Voxel", false),
            true
        );
    }

    /// The production install over the retail sources against the native
    /// run of `0x00679D90`/`0x00679ED0`/`0x00679EC0`
    /// (tools/storage_oracle/ini_crc).
    #[test]
    fn retail_ini_hashes_match_the_native_composite() {
        let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let mut process = ProcessAssets::new_with_ini_config(
            MediaArchiveMode::STOCK_DIGITAL,
            false,
            Default::default(),
        );
        process.initialize_sources_if_needed(&assets).unwrap();
        let modes = crate::skirmish_modes::skirmish_modes_from_assets(&assets).unwrap();
        process.install_native_ini_hashes(&assets, &modes);
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/ini_crc.json",
        ))
        .unwrap();
        let composite = &corpus["composite"];
        let value = |key: &str| composite[key]["unsigned"].as_u64().unwrap() as u32;
        let hashes = process.native_ini_hashes().expect("retail INI hashes");
        assert_eq!(
            (hashes.rules, hashes.ai, hashes.art),
            (value("rules"), value("ai"), value("art"))
        );
    }

    fn test_manager(label: &str) -> AssetManager {
        let dir = std::env::temp_dir().join(format!(
            "vera20k-process-assets-{}-{label}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("test asset dir");
        AssetManager::from_loose_root_for_test(&dir)
    }

    /// F11: the slot's lease/return transitions cover every loading outcome —
    /// success and failure/cancel both return the same manager, a double
    /// return keeps the resident one, and an absent slot leases nothing.
    #[test]
    fn asset_manager_lease_returns_on_success_failure_and_cancel() {
        // Absent slot: nothing to lease, nothing available.
        let mut absent = ProcessAssets::new(MediaArchiveMode::STOCK_DIGITAL, false);
        assert!(!absent.is_available());
        assert!(absent.lease_for_loading().is_none());

        // Success-shaped cycle: lease out, manager comes home.
        let mut assets = ProcessAssets::new(MediaArchiveMode::STOCK_DIGITAL, false);
        assets.return_from_loading(test_manager("cycle"));
        assert!(assets.is_available());
        let leased = assets
            .lease_for_loading()
            .expect("available manager leases");
        assert!(
            !assets.is_available(),
            "leased slot has no resident manager"
        );
        assert!(
            assets.lease_for_loading().is_none(),
            "a leased slot cannot lease again"
        );
        assets.return_from_loading(leased);
        assert!(assets.is_available(), "success returns the manager");

        // Failure/cancel-shaped cycle: identical return path.
        let leased = assets.lease_for_loading().expect("re-lease after return");
        assets.return_from_loading(leased);
        assert!(assets.is_available(), "failure/cancel returns the manager");

        // Double return: the resident manager is kept, not replaced.
        assets.return_from_loading(test_manager("stray"));
        assert!(assets.is_available());

        // A lost lease clears the lease flag so the slot can lease again.
        let _ = assets.lease_for_loading().expect("lease");
        assets.note_lease_ended_without_return();
        assert!(!assets.is_available());
        assert!(
            assets.lease_for_loading().is_none(),
            "manager is truly gone"
        );
    }

    #[test]
    fn failed_rules_recovery_retains_selected_empty_audio_and_same_sound_owner() {
        use crate::map::source::test_support::TestDirectory;
        use crate::rules::ini_parser::IniFile;
        let dir = TestDirectory::new("audio-failed-rules-recovery");
        // Missing Rules is fallible, but missing optional audio is selected-empty,
        // not an invitation to reselect it when the next manager is acquired.
        let mut owner = ProcessAssets::new(MediaArchiveMode::STOCK_DIGITAL, false);
        let manager = AssetManager::from_loose_root_for_test(dir.path());
        assert!(owner.initialize_sources_if_needed(&manager).is_err());
        assert!(owner.native_rules().is_none());
        let sounds = std::sync::Arc::clone(owner.audio_catalog().unwrap().definitions.sounds());
        assert!(sounds.is_empty());
        dir.write("rulesmd.ini", b"[General]\nBuildSpeed=.7\n");
        dir.write("artmd.ini", b"[Test]\n");
        dir.write("soundmd.ini", b"[SoundList]\n0=Late\n[Late]\nSounds=late\n");
        let manager = AssetManager::from_loose_root_for_test(dir.path());
        owner.initialize_sources_if_needed(&manager).unwrap();
        assert!(std::sync::Arc::ptr_eq(
            &sounds,
            owner.native_rules().unwrap().fixed_sounds()
        ));
        assert!(
            owner
                .audio_catalog()
                .unwrap()
                .sounds()
                .get("Late")
                .is_none()
        );
        for _ in 0..2 {
            owner
                .native_rules
                .as_mut()
                .unwrap()
                .load_scenario(
                    crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(None),
                    &IniFile::empty(),
                )
                .unwrap();
            owner.initialize_sources_if_needed(&manager).unwrap();
            assert!(std::sync::Arc::ptr_eq(
                &sounds,
                owner.native_rules().unwrap().fixed_sounds()
            ));
        }
    }

    #[test]
    fn gsi_04_01_process_owner_binds_grid_clones_and_map_reloads() {
        let assets = ProcessAssets::new(MediaArchiveMode::STOCK_DIGITAL, false);
        let process_dummy = assets.shared_cell_dummy.clone();
        let mut first =
            crate::map::resolved_terrain::ResolvedTerrainGrid::from_cells(0, 0, Vec::new());
        first.bind_shared_cell_dummy(process_dummy.clone());
        let first_clone = first.clone();
        first.stamp_dummy_cell_requested_coord(7, 9);
        assert!(
            first
                .shared_cell_dummy()
                .same_identity(&first_clone.shared_cell_dummy())
        );
        assert_eq!(first_clone.dummy_cell_requested_coord(), (7, 9));

        process_dummy.set_level_slope(-7, 11);
        process_dummy.reconstruct_for_map_resize();
        let mut reloaded =
            crate::map::resolved_terrain::ResolvedTerrainGrid::from_cells(0, 0, Vec::new());
        reloaded.bind_shared_cell_dummy(assets.shared_cell_dummy.clone());
        assert!(
            reloaded
                .shared_cell_dummy()
                .same_identity(&first.shared_cell_dummy())
        );
        assert_eq!(reloaded.dummy_cell_requested_coord(), (0, 0));
        assert_eq!(reloaded.dummy_cell_level_slope(), (0, 0));

        let headless_process = crate::map::resolved_terrain::SharedCellDummy::fresh();
        assert!(!headless_process.same_identity(&assets.shared_cell_dummy));
        assert_eq!(headless_process.snapshot().coord, (0, 0));
    }
}
