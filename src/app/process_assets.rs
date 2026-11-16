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

/// Return the outgoing scene's live Rules writes before any native Type reset,
/// whether the next consumer is Full_Init or an RMG preview.
pub(crate) fn transfer_outgoing_simulation_rules(
    owner: &mut crate::rules::process_owner::NativeRulesProcessOwner,
    simulation: &mut crate::sim::world::Simulation,
) -> Result<(), crate::rules::error::RulesError> {
    owner.consume_outgoing_scenario_rules(
        simulation.retire_object_type_tech_levels(),
        simulation.retire_ambient_change_inputs(),
        simulation.retire_building_upgrade_art_names(),
        simulation.retire_voxel_type_damage(),
    )
}

pub(crate) struct ProcessAssets {
    /// Selected at launch, retained even if initial archive loading fails.
    media_archive_mode: MediaArchiveMode,
    /// The launch's mod folder (`app::mods`): a manager rebuilt after a lost
    /// lease reads the same files.
    mod_dir: Option<std::path::PathBuf>,
    /// The launch's Ares setting (`util::ares_launch::AresSetting`), kept as
    /// the mod folder is.
    ares_setting: crate::util::ares_launch::AresSetting,
    /// That folder's files as one digest (`app::mod_files::files_digest`), taken
    /// once: the pre-match check compares it.
    mod_files: std::sync::Arc<std::sync::OnceLock<Result<[u8; 32], String>>>,
    /// [`ProcessAssets::read_mod_files_ahead`] has started its thread.
    mod_files_ahead: std::sync::atomic::AtomicBool,
    /// That folder's map additions (`app::mod_additions`), read once: the
    /// matches play what the pre-match check compares.
    map_additions: std::sync::OnceLock<
        Result<Option<std::sync::Arc<crate::app::mod_additions::MapAdditions>>, String>,
    >,
    /// Launch request and its first resolved names survive a lost asset lease.
    ini_config: crate::util::ini_roots::IniRootConfig,
    selected_ini_roots: Option<crate::util::ini_roots::IniRoots>,
    /// The player's own rule switches, read at startup with VERA.INI and
    /// changed by the skirmish Game Options ([`Self::set_terrain_deformation`],
    /// [`Self::set_superweapon_limit`]). A network room plays its host's
    /// instead ([`Self::use_personal_rules`]).
    personal_rules: crate::rules::personal_rules::PersonalRules,
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
    /// Game-folder files that change those sources or the theater tiles from
    /// the retail copies, found with the hashes.
    changed_game_files: Vec<String>,
    /// VERA: the theater tiles the game folder changes, found once from the
    /// process sources by whichever needs them first: the startup hashes, or
    /// the rules digest of a process that never starts (the launch probe).
    changed_tiles: std::sync::OnceLock<crate::map::theater::ChangedTiles>,
}

fn mod_files_digest(dir: &std::path::Path) -> Result<[u8; 32], String> {
    crate::app::mod_files::files_digest(dir).map_err(|error| {
        log::warn!("Could not read the mod in {}: {error}", dir.display());
        format!("the mod's files could not be read: {error}")
    })
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
            mod_dir: None,
            ares_setting: Default::default(),
            mod_files: Default::default(),
            mod_files_ahead: Default::default(),
            map_additions: Default::default(),
            ini_config: Default::default(),
            selected_ini_roots: None,
            personal_rules: Default::default(),
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
            changed_game_files: Vec::new(),
            changed_tiles: Default::default(),
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

    pub(crate) fn with_personal_rules(
        mut self,
        rules: crate::rules::personal_rules::PersonalRules,
    ) -> Self {
        self.personal_rules = rules;
        self
    }

    pub(crate) fn with_mod_dir(mut self, mod_dir: Option<std::path::PathBuf>) -> Self {
        self.mod_dir = mod_dir;
        self
    }

    pub(crate) fn with_ares_setting(
        mut self,
        setting: crate::util::ares_launch::AresSetting,
    ) -> Self {
        self.ares_setting = setting;
        self
    }

    /// The launch's Ares setting, which other players' games must share.
    pub(crate) fn ares_setting(&self) -> crate::util::ares_launch::AresSetting {
        self.ares_setting
    }

    /// The player opened a multiplayer menu: start reading the mod's files
    /// now, off the thread that draws, so the pre-match check that follows
    /// rarely waits. A mod's archives take a moment to read, and a run that
    /// never comes here (single player, a map picture) never reads them.
    /// Where no thread starts, that check reads them itself.
    pub(crate) fn read_mod_files_ahead(&self) {
        use std::sync::atomic::Ordering;
        let Some(dir) = self.mod_dir.clone() else {
            return;
        };
        if self.mod_files_ahead.swap(true, Ordering::Relaxed) {
            return;
        }
        let files = std::sync::Arc::clone(&self.mod_files);
        let _ = std::thread::Builder::new()
            .name("mod-files".into())
            .spawn(move || {
                files.get_or_init(|| mod_files_digest(&dir));
            });
    }

    /// The launch's mod folder as one digest, or why it could not be read;
    /// `None` without a mod.
    fn mod_files(&self) -> Option<&Result<[u8; 32], String>> {
        let dir = self.mod_dir.as_deref()?;
        Some(self.mod_files.get_or_init(|| mod_files_digest(dir)))
    }

    /// The launch mod's map additions, which the matches they reach
    /// (`mod_additions::reach`) add to their maps, or why they could not be
    /// read; `None` without a mod or when its folder holds other files the
    /// game reads. `assets` name the Rules, Art, AI, sound and theater INIs
    /// this game reads.
    pub(crate) fn map_additions(
        &self,
        assets: &AssetManager,
    ) -> Result<Option<std::sync::Arc<crate::app::mod_additions::MapAdditions>>, String> {
        let Some(dir) = self.mod_dir.as_deref() else {
            return Ok(None);
        };
        self.map_additions
            .get_or_init(|| {
                let roots = assets.ini_roots();
                let mut names: Vec<String> = [
                    roots.rules(),
                    roots.art(),
                    roots.ai(),
                    roots.sound(),
                    roots.eva(),
                ]
                .map(str::to_owned)
                .to_vec();
                names.extend(crate::map::theater::theater_ini_names(assets));
                crate::app::mod_additions::MapAdditions::read(dir, &names)
                    .map(|additions| additions.map(std::sync::Arc::new))
            })
            .clone()
    }

    /// The skirmish Game Options' Terrain Deformation box: the next offline
    /// scenario's rules read it.
    pub(crate) fn set_terrain_deformation(&mut self, on: bool) {
        self.personal_rules.set_terrain_deformation(on);
        if let Some(rules) = self.native_rules.as_mut() {
            rules.set_terrain_deformation(on);
        }
    }

    /// The skirmish Game Options' Superweapon Limit: the next offline
    /// scenario's rules read it.
    pub(crate) fn set_superweapon_limit(&mut self, limit: i32) {
        self.personal_rules.set_superweapon_limit(limit);
        if let Some(rules) = self.native_rules.as_mut() {
            rules.set_superweapon_limit(limit);
        }
    }

    /// The player's own rule switches, which offline games play and a
    /// network room's host shares.
    pub(crate) fn personal_rules(&self) -> &crate::rules::personal_rules::PersonalRules {
        &self.personal_rules
    }

    /// Play `rules` from the next scenario on, and agree on them in a network
    /// room: the room's host's, or none in Quick Match
    /// (`NativeRulesProcessOwner::use_personal_rules`). Before the rules load
    /// the player's own apply when the sources select.
    pub(crate) fn use_personal_rules(
        &mut self,
        rules: crate::rules::personal_rules::PersonalRules,
    ) {
        if let Some(owner) = self.native_rules.as_mut() {
            owner.use_personal_rules(rules);
        }
    }

    /// Offline games play the player's own switches.
    pub(crate) fn use_own_personal_rules(&mut self) {
        self.use_personal_rules(self.personal_rules.clone());
    }

    /// The Rules, Art and AI roots and the theater INI suffix are the stock YR
    /// ones; config.toml's `[ini]` profiles, names and `theater_suffix` can
    /// pick others. Any other suffix counts, even one that picks a shipped
    /// theater INI under another theater's name.
    pub(crate) fn stock_ini_roots(&self) -> bool {
        let stock = crate::util::ini_roots::IniRoots::default();
        self.selected_ini_roots.as_ref().is_none_or(|roots| {
            [
                (roots.rules(), stock.rules()),
                (roots.art(), stock.art()),
                (roots.ai(), stock.ai()),
                (roots.theater_suffix(), stock.theater_suffix()),
            ]
            .iter()
            .all(|(used, stock)| used.eq_ignore_ascii_case(stock))
        })
    }

    /// Loose files or archives in the game folder that change the rules the
    /// network compares or a theater's tiles from the retail copies, e.g.
    /// `rulesmd.ini`, `clear01.sno` or `expandmd99.mix`; found at startup.
    pub(crate) fn changed_game_files(&self) -> &[String] {
        &self.changed_game_files
    }

    fn changed_tiles(&self, assets: &AssetManager) -> &crate::map::theater::ChangedTiles {
        self.changed_tiles
            .get_or_init(|| crate::map::theater::changed_tiles(assets))
    }

    /// Recovery uses the process's already resolved choice, never a new Auto
    /// decision against an archive set that may have changed since startup.
    pub(crate) fn reconstruct_manager(
        &self,
        root: &std::path::Path,
    ) -> anyhow::Result<AssetManager> {
        let assets = AssetManager::with_mod_and_ares(
            root,
            self.mod_dir.as_deref(),
            self.media_archive_mode,
            self.ares_setting,
        )?;
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
            let definitions = crate::rules::audio_sources::AudioDefinitions::select_admitting(
                assets,
                index.as_ref().map(|loaded| &loaded.index),
            );
            let source_receipt = serde_json::json!({
                "soundmd": definitions.sound_source(),
                "evamd": definitions.eva_source(),
                "index_enabled": self.load_audio_index,
                "index": index.as_ref().map(|loaded| &loaded.sources),
                "sound_count": definitions.sounds().len(),
                "eva_count": definitions.eva().len(),
            });
            let source_receipt = if assets.ships_ares() {
                let mut receipt = source_receipt;
                receipt["additional_indexes"] =
                    serde_json::json!(index.as_ref().map(|loaded| &loaded.additional_sources));
                receipt
            } else {
                source_receipt
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
            .with_personal_rules(self.personal_rules.clone())
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
        self.refresh_campaigns_if_available();
    }

    /// ClearScene685609 re-reads the process-retained Campaign registry on
    /// saved loads too. It never infers campaign identity from a map name.
    pub(crate) fn refresh_campaigns_if_available(&mut self) {
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

    /// The media choice startup resolved: New Mod opens the game folder again
    /// with it (`App::make_new_mod`).
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

    /// Whether this game is read the Ares way, as Ares.dll loaded reads it
    /// (`NativeRulesProcessOwner::reads_ares_way`): the shell's answer for its
    /// Ares hooks, the random map generator's and the random countries'.
    pub(crate) fn reads_ares_way(&self) -> bool {
        self.native_rules
            .as_ref()
            .is_some_and(|rules| rules.reads_ares_way())
    }

    pub(crate) fn native_ini_hashes(
        &self,
    ) -> Option<crate::rules::native_ini_hash::NativeIniHashes> {
        // VERA: without the Options switches, which a
        // network room takes from its host and Quick Match leaves out; the
        // rules digest still compares the ones each game plays.
        self.native_ini_hashes
    }

    /// The VERA protocol compares the exact process-selected Rules/LANGRULE/ART
    /// plus the selected mode and AIMD snapshots used by scenario loading,
    /// each theater's INI as `map::theater::load_theater` reads it, and the
    /// theater tiles the game folder changes.
    /// The legacy advertisement CRC remains a separate native UI value.
    pub(crate) fn multiplayer_rules_digest(&self, mode: &str) -> Result<String, String> {
        use crate::rules::ini_parser::IniFile;
        use crate::rules::retail_sources::{select_ini, select_mode_ini};
        let rules = self
            .native_rules
            .as_ref()
            .ok_or("retail rules not loaded")?;
        let assets = self.manager.as_ref().ok_or("retail sources are loading")?;
        let ai = select_ini(assets, assets.ini_roots().ai())?;
        let selected_mode = select_mode_ini(assets, Some(mode))?;
        let mut digests = rules.multiplayer_source_digests().to_vec();
        digests.push(Some(ai.ini.multiplayer_digest()));
        digests.push(selected_mode.as_ref().map(IniFile::multiplayer_digest));
        // VERA: a theater INI's tilesets decide every cell's land and height.
        // One that is absent or does not parse fails that theater's load.
        digests.extend(
            crate::map::theater::theater_ini_names(assets)
                .iter()
                .map(|name| {
                    let (bytes, _) = assets.get_with_source_ref(name)?;
                    IniFile::from_bytes(bytes)
                        .ok()
                        .map(|ini| ini.multiplayer_digest())
                }),
        );
        // VERA: and the tiles the game folder changes; none add an absent
        // digest.
        digests.push(self.changed_tiles(assets).digest.clone());
        let mut bytes = b"VERA20k rules agreement v1\0".to_vec();
        // VERA: the Options switches played, which the digested sources
        // leave out; none adds nothing.
        if let Some(crc) = rules.personal_rules_compatibility_crc() {
            bytes.extend_from_slice(b"personal rules\0");
            bytes.extend_from_slice(&crc.to_le_bytes());
        }
        for digest in digests {
            bytes.push(u8::from(digest.is_some()));
            if let Some(digest) = digest {
                bytes.extend_from_slice(digest.as_bytes());
            }
        }
        // Whether Ares plays (`rules::ares_switch`) follows these sources and
        // the map, which the map agreement covers, and whether the game is an
        // Ares game: the selected mod mounted `ares.mix`, or the player's Ares
        // setting is On. Such a game adds the byte, so the same files read
        // with and without Ares never agree; with no mod selected and the
        // setting on Auto the installed archive is ignored and adds nothing.
        if assets.ships_ares() {
            bytes.push(1);
        }
        // VERA: the player's Ares setting. Off keeps Ares keys from turning
        // Ares on, so such a game never agrees with one set to Auto; Auto
        // adds nothing, as before the setting.
        match assets.ares_setting() {
            crate::util::ares_launch::AresSetting::Auto => {}
            // On without a mod can play with or without the installation's
            // `ares.mix`; which one is part of the game.
            crate::util::ares_launch::AresSetting::On => {
                bytes.extend_from_slice(b"ares on\0");
                bytes.push(u8::from(assets.ares_mix_mounted()));
            }
            crate::util::ares_launch::AresSetting::Off => bytes.extend_from_slice(b"ares off\0"),
        }
        // VERA: with a mod in use, its other files too (art, maps, archives),
        // so players whose copies of a mod differ never start a match. Without
        // a mod this adds nothing.
        if let Some(files) = self.mod_files() {
            bytes.extend_from_slice(b"mod files\0");
            bytes.extend_from_slice(files.as_ref().map_err(String::clone)?);
        }
        // VERA: and the map additions the matches play, as this process read
        // them. A game that does not add them, such as an older release with
        // the same files, never agrees with one that does.
        if let Some(additions) = self.map_additions(assets)? {
            bytes.extend_from_slice(b"map additions\0");
            bytes.extend_from_slice(additions.digest().as_bytes());
        }
        Ok(crate::util::sha256::sha256_hex(&bytes))
    }

    /// A seed regenerates its map after Start, so its rules agreement also
    /// covers the inputs selected by the ordinary generation preparation.
    pub(crate) fn multiplayer_generated_rules_digest(
        &mut self,
        mode: &str,
        options: &crate::map::rmg::RmgOptions,
    ) -> Result<String, String> {
        let rules = self.multiplayer_rules_digest(mode)?;
        let assets = self.manager.as_mut().ok_or("retail sources are loading")?;
        let mode_rules = crate::rules::retail_sources::select_mode_ini(assets, Some(mode))?;
        let prepared = crate::app::shell_random_map::prepare_random_map_generation(
            assets,
            options,
            crate::app::shell_random_map::RandomMapRulesPreparation::ReadOnly(
                self.native_rules.as_ref(),
            ),
            mode_rules.as_ref(),
        )
        .ok_or("generated map theater is unavailable")?;
        let mut bytes = b"VERA20k generated rules agreement v1\0".to_vec();
        bytes.extend_from_slice(rules.as_bytes());
        bytes.extend_from_slice(prepared.multiplayer_digest().as_bytes());
        Ok(crate::util::sha256::sha256_hex(&bytes))
    }

    /// Hash the process sources once (`crate::rules::native_ini_hash`):
    /// the selected rules root and ART, AIMD, each mode's INI by id
    /// `1..=count` and CoopCampMD.
    /// VERA: also finds the game-folder files that change those sources,
    /// a selected mod's LANGRULE.INI, a theater INI or a theater's tiles
    /// (VERA's own agreement compares these) from the retail copies
    /// ([`Self::changed_game_files`]).
    pub(crate) fn install_native_ini_hashes(
        &mut self,
        assets: &AssetManager,
        modes: &[crate::skirmish_modes::SkirmishGameMode],
    ) {
        use crate::rules::ini_parser::IniFile;
        use crate::rules::retail_sources::{select_ini, select_mode_ini};
        let roots = assets.ini_roots();
        let theater_inis = crate::map::theater::theater_ini_names(assets);
        // MPModesMD.ini is the mode roster: which mode INIs the hash adds.
        let mut compared = vec![roots.rules(), roots.art(), roots.ai(), "MPModesMD.ini"];
        // Match RetailRulesSources' selection: Original mode never consumes
        // the installation's optional LANGRULE, so it changes no game files.
        if assets.mod_selected() {
            compared.push("langrule.ini");
        }
        compared.extend(modes.iter().map(|mode| mode.override_file.as_str()));
        compared
            .push(crate::app::frontend::skirmish_session::cooperative::COOPERATIVE_CAMPAIGN_INI);
        compared.extend(theater_inis.iter().map(String::as_str));
        self.changed_game_files =
            crate::rules::retail_sources::files_changing_shipped_inis(assets, &compared);
        // A mounted `ares.mix` (a selected mod's, or the installation's with
        // the player's Ares setting On) makes the game an Ares game
        // (`rules::ares_switch`): its INIs and Rules are read the Ares way even
        // where every compared file is retail. With no mod selected and the
        // setting on Auto it is ignored.
        // The player's Ares setting is not a file: the room names it itself.
        if assets.ares_mix_mounted() {
            self.changed_game_files.push("ares.mix".to_owned());
        }
        for file in self.changed_tiles(assets).files.clone() {
            if !self.changed_game_files.contains(&file) {
                self.changed_game_files.push(file);
            }
        }
        log::info!(
            "Game files that change the original game: {}",
            if self.changed_game_files.is_empty() {
                "none".to_owned()
            } else {
                self.changed_game_files.join(", ")
            }
        );
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

    pub(crate) fn country_random_rows(&self) -> Vec<(&str, Option<&str>, bool, i32, i32)> {
        self.native_rules
            .as_ref()
            .map(|rules| rules.country_random_rows())
            .unwrap_or_default()
    }

    /// The side's `DialogBackground.Image/.Palette` pair for the shell dialog
    /// chrome, or `None` to keep the stock per-side backgrounds.
    pub(crate) fn side_dialog_background(&self, side: i32) -> Option<(String, String)> {
        self.native_rules
            .as_ref()
            .and_then(|rules| rules.side_dialog_background(side))
            .map(|(image, palette)| (image.to_owned(), palette.to_owned()))
    }

    /// The score dialog's theme name for a side, or `None` for gamemd's
    /// literal `SCORE`.
    pub(crate) fn side_score_theme(
        &self,
        side: i32,
        theme: crate::rules::ruleset::ScoreTheme,
    ) -> Option<&str> {
        self.native_rules
            .as_ref()
            .and_then(|rules| rules.side_score_theme(side, theme))
    }

    /// The shell side's `Game_LoadCampaignScoreAssets` /
    /// `Game_LoadMultiplayerScoreAssets` file names. Every field `None` keeps
    /// that name's fixed stock table entry.
    pub(crate) fn side_score_art(&self, side: i32) -> crate::rules::ruleset::SideScoreArt<'_> {
        self.native_rules
            .as_ref()
            .map(|rules| rules.side_score_art(side))
            .unwrap_or_default()
    }

    /// `Game_GetFlagSurface @ 0x1001DCE0` country insignia by HouseType index,
    /// for lobby/diplomacy rows that resolve before a match Rules projection
    /// exists. `None` unless Ares.dll is loaded (the game is read the Ares
    /// way).
    pub(crate) fn country_flag_surface(&self, index: u16) -> Option<String> {
        let rules = self.native_rules.as_ref()?;
        if rules.reads_ares_way() {
            rules.country_flag_surface(index).map(str::to_owned)
        } else {
            None
        }
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

    /// Field-level split borrow for a scenario rescan: the scan registers loose
    /// YRO archives, and its PKT titles resolve through the process CSF. Both
    /// fields live on this owner, so the pair is borrowed together.
    #[cfg(feature = "verax")]
    pub(crate) fn scenario_rescan_sources(
        &mut self,
    ) -> (
        Option<&mut AssetManager>,
        Option<&crate::assets::csf_file::CsfFile>,
    ) {
        (self.manager.as_mut(), self.csf.as_ref())
    }

    /// Field-level split borrow: the resident manager and the terrain-variant
    /// cache live on the same owner, and the RMG preview path needs both at
    /// once.
    pub(crate) fn manager_and_rules_with_tile_cache(
        &mut self,
    ) -> (
        Option<&mut AssetManager>,
        Option<&mut crate::rules::process_owner::NativeRulesProcessOwner>,
        &mut crate::map::tile_variant_selector::TileVariantSelectorCache,
    ) {
        (
            self.manager.as_mut(),
            self.native_rules.as_mut(),
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

#[cfg(all(test, tests_app))]
mod tests {
    use super::{MediaArchiveMode, ProcessAssets};
    use crate::assets::asset_manager::AssetManager;

    #[test]
    fn original_game_ignores_langrule_while_mods_keep_its_rules_and_digest() {
        use crate::assets::asset_manager::test_support::{TestDirectory, audio_test_install};

        let install = audio_test_install("original-langrule");
        let mod_dir = TestDirectory::new("selected-langrule");
        for (name, text) in [
            (
                "rulesmd.ini",
                "[General]\nBuildSpeed=.7\n[ColorAdd]\nBase=1,2,3\n",
            ),
            ("artmd.ini", "[UNIT]\nVoxel=yes\n"),
            ("aimd.ini", "[AI]\nValue=1\n"),
        ] {
            std::fs::write(install.path().join(name), text).unwrap();
        }
        let start = |selected_mod: bool| -> Result<_, String> {
            let assets = AssetManager::with_mod(
                install.path(),
                selected_mod.then_some(mod_dir.path()),
                MediaArchiveMode::STOCK_DIGITAL,
            )
            .unwrap();
            let mut process = ProcessAssets::new(MediaArchiveMode::STOCK_DIGITAL, false);
            let (rules, _) = process.initialize_sources_if_needed(&assets)?;
            let color = rules.unwrap().color_add.entries()[0].rgb;
            process.install_native_ini_hashes(&assets, &[]);
            process.return_from_loading(assets);
            Ok((
                process.multiplayer_rules_digest("")?,
                color,
                process.changed_game_files().to_vec(),
            ))
        };
        let original = start(false).unwrap();
        std::fs::write(
            install.path().join("LANGRULE.INI"),
            crate::rules::retail_ini_fixture::DIGESTED_LANGRULE_CONTROL,
        )
        .unwrap();
        assert_eq!(start(false).unwrap(), original);
        let modded = start(true).unwrap();
        assert_ne!(modded.0, original.0);
        assert_eq!(modded.1, [7, 8, 9]);
        assert!(
            modded
                .2
                .iter()
                .any(|name| name.eq_ignore_ascii_case("langrule.ini"))
        );
        std::fs::write(
            install.path().join("LANGRULE.INI"),
            "[General]\nBuildSpeed=9\n",
        )
        .unwrap();
        assert_eq!(start(false).unwrap(), original);
        assert!(start(true).is_err(), "mods retain LANGRULE's Digest check");
    }

    #[test]
    fn original_game_agrees_with_an_ares_installation_without_loading_ares() {
        use crate::assets::asset_manager::test_support::{TestDirectory, audio_test_install};
        use crate::rules::ares_switch::read_ares_way;

        let install = audio_test_install("original-ares-agreement");
        for (name, text) in [
            ("rulesmd.ini", "[General]\nBuildSpeed=.7\nBuildSpeed=.8\n"),
            ("artmd.ini", "[UNIT]\nVoxel=yes\n"),
            ("aimd.ini", "[AI]\nValue=1\n"),
        ] {
            std::fs::write(install.path().join(name), text).unwrap();
        }
        let mod_dir = TestDirectory::new("selected-ares-agreement");
        let digest = |selected_mod: bool| {
            let assets = AssetManager::with_mod(
                install.path(),
                selected_mod.then_some(mod_dir.path()),
                MediaArchiveMode::STOCK_DIGITAL,
            )
            .unwrap();
            let _reading = read_ares_way(assets.ships_ares());
            let mut process = ProcessAssets::new(MediaArchiveMode::STOCK_DIGITAL, false);
            process.initialize_sources_if_needed(&assets).unwrap();
            process.return_from_loading(assets);
            process.multiplayer_rules_digest("").unwrap()
        };
        let original = digest(false);
        install.write_mix("ares.mix", "ares.txt", b"installed Ares");
        assert_eq!(digest(false), original);
        assert_ne!(digest(true), original);
        std::fs::write(
            install.path().join("rulesmd.ini"),
            "[General]\nBuildSpeed=.9\n",
        )
        .unwrap();
        assert_ne!(digest(false), original, "actual rule changes still differ");
    }

    #[test]
    fn generated_rules_agreement_covers_rmg_settings_and_theater_inputs() {
        let dir = crate::assets::asset_manager::test_support::audio_test_install("rmg-agreement");
        for (name, bytes) in [
            ("rulesmd.ini", "[General]\nBuildSpeed=.7\n"),
            ("artmd.ini", "[UNIT]\nVoxel=yes\n"),
            ("aimd.ini", "[AI]\nValue=1\n"),
            ("rmgmd.ini", "[General]\nMaxTrees=500\n"),
            (
                "temperatmd.ini",
                "[General]\n[TileSet0000]\nFileName=clear\nTilesInSet=1\nMorphable=yes\n",
            ),
        ] {
            std::fs::write(dir.path().join(name), bytes).unwrap();
        }
        for name in ["unittem.pal", "isotem.pal", "temperat.pal"] {
            std::fs::write(dir.path().join(name), [0_u8; 768]).unwrap();
        }
        let digest = || {
            let mut process = ProcessAssets::new(MediaArchiveMode::STOCK_DIGITAL, false);
            let assets = AssetManager::new(dir.path(), MediaArchiveMode::STOCK_DIGITAL).unwrap();
            process.initialize_sources_if_needed(&assets).unwrap();
            process.return_from_loading(assets);
            process
                .multiplayer_generated_rules_digest("", &crate::map::rmg::RmgOptions::default())
                .unwrap()
        };
        let original = digest();
        std::fs::write(dir.path().join("rmgmd.ini"), "[General]\nMaxTrees=501\n").unwrap();
        assert_ne!(original, digest(), "generator tuning must agree");
        std::fs::write(dir.path().join("rmgmd.ini"), "[General]\nMaxTrees=500\n").unwrap();
        std::fs::write(
            dir.path().join("temperatmd.ini"),
            "[General]\n[TileSet0000]\nFileName=clear\nTilesInSet=1\nMorphable=no\n",
        )
        .unwrap();
        assert_ne!(original, digest(), "resolved theater inputs must agree");
    }

    #[test]
    fn multiplayer_agreement_covers_frozen_langrule_and_the_selected_mode() {
        let dir =
            crate::assets::asset_manager::test_support::audio_test_install("multiplayer-agreement");
        for (name, bytes) in [
            ("rulesmd.ini", "[General]\nBuildSpeed=.7\n"),
            ("artmd.ini", "[UNIT]\nVoxel=yes\n"),
            ("aimd.ini", "[AI]\nValue=1\n"),
            (
                "langrule.ini",
                crate::rules::retail_ini_fixture::DIGESTED_LANGRULE_CONTROL,
            ),
            ("mode.ini", "[General]\nBuildSpeed=.6\n"),
        ] {
            std::fs::write(dir.path().join(name), bytes).unwrap();
        }
        let mod_dir = crate::assets::asset_manager::test_support::TestDirectory::new(
            "langrule-agreement-mod",
        );
        let mut process = ProcessAssets::new(MediaArchiveMode::STOCK_DIGITAL, false);
        let assets = AssetManager::with_mod(
            dir.path(),
            Some(mod_dir.path()),
            MediaArchiveMode::STOCK_DIGITAL,
        )
        .unwrap();
        process.initialize_sources_if_needed(&assets).unwrap();
        process.return_from_loading(assets);
        let original = process.multiplayer_rules_digest("mode.ini").unwrap();
        assert_ne!(original, process.multiplayer_rules_digest("").unwrap());
        std::fs::remove_file(dir.path().join("langrule.ini")).unwrap();
        assert_eq!(
            original,
            process.multiplayer_rules_digest("mode.ini").unwrap(),
            "the live process retains its source snapshot"
        );
        let mut fresh = ProcessAssets::new(MediaArchiveMode::STOCK_DIGITAL, false);
        let assets = AssetManager::with_mod(
            dir.path(),
            Some(mod_dir.path()),
            MediaArchiveMode::STOCK_DIGITAL,
        )
        .unwrap();
        fresh.initialize_sources_if_needed(&assets).unwrap();
        fresh.return_from_loading(assets);
        assert_ne!(
            original,
            fresh.multiplayer_rules_digest("mode.ini").unwrap()
        );
    }

    /// VERA: players whose copies of a mod differ in a file the rules do not
    /// read never agree, nor a player with a mod and one without.
    #[test]
    fn a_mods_other_files_take_part_in_the_agreement() {
        let dir = crate::assets::asset_manager::test_support::audio_test_install("mod-files");
        for (name, bytes) in [
            ("rulesmd.ini", "[General]\nBuildSpeed=.7\n"),
            ("artmd.ini", "[UNIT]\nVoxel=yes\n"),
            ("aimd.ini", "[AI]\nValue=1\n"),
        ] {
            std::fs::write(dir.path().join(name), bytes).unwrap();
        }
        let mods = dir.path().join("mods-under-test");
        for (folder, art) in [("one", "a"), ("same", "a"), ("other", "b")] {
            std::fs::create_dir_all(mods.join(folder)).unwrap();
            std::fs::write(mods.join(folder).join("tank.vxl"), art).unwrap();
        }
        let digest = |folder: Option<&str>| {
            let mod_dir = folder.map(|folder| mods.join(folder));
            let mut process = ProcessAssets::new(MediaArchiveMode::STOCK_DIGITAL, false)
                .with_mod_dir(mod_dir.clone());
            let assets = AssetManager::with_mod(
                dir.path(),
                mod_dir.as_deref(),
                MediaArchiveMode::STOCK_DIGITAL,
            )
            .unwrap();
            process.initialize_sources_if_needed(&assets).unwrap();
            process.return_from_loading(assets);
            process.multiplayer_rules_digest("").unwrap()
        };
        assert_eq!(digest(Some("one")), digest(Some("same")));
        assert_ne!(digest(Some("one")), digest(Some("other")));
        assert_ne!(digest(Some("one")), digest(None));
    }

    /// VERA: the matches play a mod's map additions as the game read them, so
    /// a player whose file changed after their game read it never agrees with
    /// one whose game reads the changed file.
    #[test]
    fn map_additions_take_part_in_the_agreement_as_they_were_read() {
        let dir = crate::assets::asset_manager::test_support::audio_test_install("additions");
        for (name, bytes) in [
            ("rulesmd.ini", "[General]\nBuildSpeed=.7\n"),
            ("artmd.ini", "[UNIT]\nVoxel=yes\n"),
            ("aimd.ini", "[AI]\nValue=1\n"),
        ] {
            std::fs::write(dir.path().join(name), bytes).unwrap();
        }
        let hard = dir.path().join("mods-under-test").join("Stronger AI");
        std::fs::create_dir_all(&hard).unwrap();
        std::fs::write(hard.join("AI teams.ini"), "[AI]\nValue=2\n").unwrap();
        let start = || {
            let mut process = ProcessAssets::new(MediaArchiveMode::STOCK_DIGITAL, false)
                .with_mod_dir(Some(hard.clone()));
            let assets =
                AssetManager::with_mod(dir.path(), Some(&hard), MediaArchiveMode::STOCK_DIGITAL)
                    .unwrap();
            process.initialize_sources_if_needed(&assets).unwrap();
            process.return_from_loading(assets);
            process
        };
        let early = start();
        let manager = early.manager().unwrap();
        assert!(early.map_additions(manager).unwrap().is_some());
        std::fs::write(hard.join("AI teams.ini"), "[AI]\nValue=3\n").unwrap();
        assert_ne!(
            early.multiplayer_rules_digest("").unwrap(),
            start().multiplayer_rules_digest("").unwrap()
        );
    }

    /// VERA: a theater INI's tilesets decide each cell's land and height, so
    /// players whose theater INIs differ never agree, and Quick Match, which
    /// pairs without the agreement, names the changed file. The agreement reads
    /// the file a theater suffix selects, else the stock one, as the loader does.
    #[test]
    fn theater_inis_take_part_in_the_agreement() {
        use crate::util::ini_roots::IniRootConfig;
        let dir =
            crate::assets::asset_manager::test_support::audio_test_install("theater-agreement");
        for (name, bytes) in [
            ("rulesmd.ini", "[General]\nBuildSpeed=.7\n"),
            ("artmd.ini", "[UNIT]\nVoxel=yes\n"),
            ("aimd.ini", "[AI]\nValue=1\n"),
        ] {
            std::fs::write(dir.path().join(name), bytes).unwrap();
        }
        let started = |theater_suffix: Option<&str>| {
            let roots = IniRootConfig {
                theater_suffix: theater_suffix.map(str::to_owned),
                ..IniRootConfig::default()
            };
            let mut process = ProcessAssets::new(MediaArchiveMode::STOCK_DIGITAL, false);
            let assets = AssetManager::new(dir.path(), MediaArchiveMode::STOCK_DIGITAL)
                .unwrap()
                .with_ini_roots(&roots)
                .unwrap();
            process.initialize_sources_if_needed(&assets).unwrap();
            process.install_native_ini_hashes(&assets, &[]);
            process.return_from_loading(assets);
            process
        };
        let digest = |theater_suffix| started(theater_suffix).multiplayer_rules_digest("");
        let original = digest(None).unwrap();
        std::fs::write(
            dir.path().join("snowmd.ini"),
            "[TileSet0000]\nFileName=clear\nTilesInSet=1\n",
        )
        .unwrap();
        let changed = started(None);
        let snow = changed.multiplayer_rules_digest("").unwrap();
        assert_ne!(original, snow);
        assert!(
            changed
                .changed_game_files()
                .iter()
                .any(|file| file == "snowmd.ini")
        );
        assert_eq!(
            digest(Some("mo")).unwrap(),
            snow,
            "no snowmo.ini: snowmd.ini"
        );
        std::fs::write(dir.path().join("snowmo.ini"), "[General]\n").unwrap();
        assert_ne!(digest(Some("mo")).unwrap(), snow);
        // Quick Match refuses any other suffix, even one that picks a shipped
        // file, as `nmd` picks New Urban's `urbannmd.ini` for Urban.
        assert!(changed.stock_ini_roots());
        assert!(!started(Some("nmd")).stock_ini_roots());
    }

    /// VERA: a theater's tiles decide its cells' land and height, and map
    /// loading picks each cell's variant by how many files its tile has. A
    /// game-folder tile that adds a variant, or a theater MIX that lacks one or
    /// does not open, changes the agreement and is named for Quick Match; the
    /// retail bytes change nothing. The launch probe, which never runs the
    /// startup hashes, reports the same digest.
    #[test]
    fn changed_theater_tiles_take_part_in_the_agreement() {
        use crate::map::source::test_support::make_new_format_mix_bytes;
        let dir = crate::assets::asset_manager::test_support::audio_test_install("theater-tiles");
        let local = make_new_format_mix_bytes(&[
            ("rulesmd.ini", b"[General]\nBuildSpeed=.7\n"),
            ("artmd.ini", b"[UNIT]\nVoxel=yes\n"),
            ("aimd.ini", b"[AI]\nValue=1\n"),
            (
                "snowmd.ini",
                b"[TileSet0000]\nFileName=clear\nTilesInSet=1\n",
            ),
        ]);
        let tiles =
            make_new_format_mix_bytes(&[("clear01.sno", b"tile"), ("clear01a.sno", b"next")]);
        std::fs::remove_file(dir.path().join("localmd.mix")).unwrap();
        std::fs::write(
            dir.path().join("ra2md.mix"),
            make_new_format_mix_bytes(&[("localmd.mix", &local), ("isosnomd.mix", &tiles)]),
        )
        .unwrap();
        let started_with = |hashes: bool| {
            let mut process = ProcessAssets::new(MediaArchiveMode::STOCK_DIGITAL, false);
            let assets = AssetManager::new(dir.path(), MediaArchiveMode::STOCK_DIGITAL).unwrap();
            process.initialize_sources_if_needed(&assets).unwrap();
            if hashes {
                process.install_native_ini_hashes(&assets, &[]);
            }
            process.return_from_loading(assets);
            process
        };
        let started = || started_with(true);
        let retail = started();
        assert!(retail.changed_game_files().is_empty());
        let digest = retail.multiplayer_rules_digest("").unwrap();
        std::fs::write(dir.path().join("clear01.sno"), b"tile").unwrap();
        assert_eq!(started().multiplayer_rules_digest("").unwrap(), digest);
        std::fs::write(dir.path().join("clear01b.sno"), b"next").unwrap();
        let added = started();
        assert_eq!(added.changed_game_files(), ["clear01b.sno"]);
        let changed = added.multiplayer_rules_digest("").unwrap();
        assert_ne!(changed, digest);
        // `spawn_probe::probe` digests without the startup hashes.
        assert_eq!(
            started_with(false).multiplayer_rules_digest("").unwrap(),
            changed
        );
        std::fs::remove_file(dir.path().join("clear01b.sno")).unwrap();
        std::fs::write(
            dir.path().join("isosnomd.mix"),
            make_new_format_mix_bytes(&[("clear01.sno", b"tile")]),
        )
        .unwrap();
        assert_eq!(started().changed_game_files(), ["isosnomd.mix"]);
        std::fs::write(dir.path().join("isosnomd.mix"), b"not a MIX").unwrap();
        assert_eq!(started().changed_game_files(), ["isosnomd.mix"]);
    }

    #[test]
    fn rooms_compare_the_switches_they_play_not_the_players_own() {
        use crate::map::source::test_support::TestDirectory;
        use crate::rules::personal_rules::PersonalRules;

        let dir = TestDirectory::new("personal-rules-network");
        dir.write("rulesmd.ini", b"[General]\nBuildSpeed=.7\n");
        dir.write("artmd.ini", b"[Test]\nVoxel=no\n");
        dir.write("aimd.ini", b"[Test]\nValue=1\n");
        let start = |rules: PersonalRules| {
            let assets = AssetManager::from_loose_root_for_test(dir.path());
            let mut process = ProcessAssets::new(MediaArchiveMode::STOCK_DIGITAL, false)
                .with_personal_rules(rules);
            process.initialize_sources_if_needed(&assets).unwrap();
            process.install_native_ini_hashes(&assets, &[]);
            process.return_from_loading(assets);
            process
        };
        let digest = |process: &ProcessAssets| process.multiplayer_rules_digest("").unwrap();
        let mut deformation = PersonalRules::default();
        deformation.set_terrain_deformation(true);
        let mut stock = start(PersonalRules::default());
        let mut own = start(deformation);
        // The room's door leaves the switches to the room.
        assert_eq!(stock.native_ini_hashes(), own.native_ini_hashes());
        let original = digest(&stock);
        assert_ne!(original, digest(&own));
        // Quick Match plays none; a room its host's.
        own.use_personal_rules(PersonalRules::default());
        assert_eq!(original, digest(&own));
        let mut host = PersonalRules::default();
        host.set_superweapon_limit(5);
        stock.use_personal_rules(host.clone());
        own.use_personal_rules(host);
        assert_eq!(digest(&stock), digest(&own));
        assert_ne!(original, digest(&stock));
    }

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
