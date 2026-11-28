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
    /// The launch's Ares setting (`util::ares_launch::AresSetting`), kept as
    /// the mod folder is.
    ares_setting: crate::util::ares_launch::AresSetting,
    /// The launch's Kratos setting (`util::kratos_launch::KratosSetting`),
    /// kept so a rebuilt manager selects the same games.
    kratos_setting: crate::util::kratos_launch::KratosSetting,
    /// Whether that setting on Auto mounts `ares.mix` for this game
    /// (`AssetManager::auto_mounts_ares`), and whether set to Off it keeps
    /// the startup Rules' own Ares keys from turning Ares on
    /// (`RuleSet::ares_kept_off`): kept with the Rules for
    /// [`Self::agreed_ares_setting`] while the manager is away at a load.
    auto_mounts_ares: bool,
    /// Whether the resolved startup rules play Kratos, retained while assets are leased.
    startup_kratos: bool,
    rules_keep_ares_off: bool,
    /// That folder's files, read once: the pre-match check compares their
    /// digest.
    mod_files: std::sync::Arc<std::sync::OnceLock<Result<ModFiles, String>>>,
    /// [`ProcessAssets::read_mod_files_ahead`] has started its thread.
    mod_files_ahead: bool,
    /// That folder's map additions (`app::mod_additions`), read once: the
    /// matches play what the pre-match check compares.
    map_additions: std::sync::OnceLock<
        Result<Option<std::sync::Arc<crate::app::mod_additions::MapAdditions>>, String>,
    >,
    /// Frozen manifest and its first resolved names survive a lost asset lease.
    resources: crate::util::mod_definition::ResourceConfig,
    /// The selected mod's folder name in `Mods` (`mods::ActiveMod::folder`):
    /// the rules parts name the mod by it, whatever folder its files were
    /// assembled in (`util::mod_package::ModFolder`).
    mod_name: Option<String>,
    selected_ini_roots: Option<crate::util::ini_roots::IniRoots>,
    /// The player's own rule switches, read at startup with VERA.INI and
    /// changed by the skirmish Game Options ([`Self::set_terrain_deformation`],
    /// [`Self::set_superweapon_limit`]) and the Options page
    /// ([`Self::set_building_production_queue`]). A network match plays its
    /// room's instead ([`Self::use_personal_rules`]).
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
    /// VERA: game-folder files that change a theater INI or the theater
    /// tiles of the original game (no mod selected), found with the hashes.
    /// They are no part of `changed_game_files`: a network match reads them
    /// only when its map names a tile the original game lacks
    /// (`map::theater::TerrainFiles`).
    terrain_files: Vec<String>,
    /// VERA: the theater tiles the game folder changes, found once from the
    /// process sources by whichever needs them first: the startup hashes, or
    /// the rules digest of a process that never starts (the launch probe).
    changed_tiles: std::sync::OnceLock<crate::map::theater::ChangedTiles>,
}

/// A mod's files as the pre-match check reads them
/// (`app::mod_files::read_for_match`).
struct ModFiles {
    /// `FileList::digest`, which the rules agreement compares.
    digest: [u8; 32],
    /// What the mod's own list says of them, for the rules parts.
    against: crate::app::mod_files::Against,
}

fn read_mod_files(dir: &std::path::Path) -> Result<ModFiles, String> {
    crate::app::mod_files::read_for_match(dir)
        .map(|checked| ModFiles {
            digest: checked.found.digest(),
            against: checked.against,
        })
        .map_err(|error| {
            log::warn!("Could not read the mod in {}: {error}", dir.display());
            format!("the mod's files could not be read: {error}")
        })
}

/// One INI source the rules agreement digests
/// ([`ProcessAssets::agreed_inis`]).
#[derive(Clone)]
struct AgreedIni {
    /// Its `rules_parts` id (`app::rules_parts::RulesFile::id`).
    id: String,
    /// The name the game reads it by.
    name: String,
    digest: Option<String>,
    /// LANGRULE and the mode INI: absent, they add no part.
    optional: bool,
}

/// Immutable process sources; output players and match queues have separate
/// lifetimes. Rules binding shares this exact SOUNDMD allocation.
pub(crate) struct ProcessAudioCatalog {
    definitions: crate::rules::audio_sources::AudioDefinitions,
    index: Option<crate::assets::asset_manager::LoadedAudioIndex>,
}

/// Where a source (`AssetResolutionRef::source_archive`) the rules parts name
/// comes from: `retail`, `mod`, `loose` or `mix:<archive>`.
fn source_label(assets: &AssetManager, source: &str) -> String {
    use crate::rules::retail_sources::{game_folder_file, shipped_source};
    if shipped_source(source) {
        return "retail".to_owned();
    }
    let file = game_folder_file(source);
    if !source.starts_with("loose:") && !assets.from_mod_folder(&file) {
        return mix_label(&file);
    }
    file_label(assets, &file)
}

/// [`source_label`] of a file in the game folder by name.
fn file_label(assets: &AssetManager, file: &str) -> String {
    if assets.from_mod_folder(file) {
        "mod".to_owned()
    } else if file.to_ascii_lowercase().ends_with(".mix") {
        mix_label(file)
    } else {
        "loose".to_owned()
    }
}

/// `mix:<archive>`, lowercased: file systems differ in the case they give a
/// name, and two platforms reading the same archive must agree.
fn mix_label(file: &str) -> String {
    format!("mix:{}", file.to_ascii_lowercase())
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
            ares_setting: Default::default(),
            kratos_setting: Default::default(),
            auto_mounts_ares: false,
            startup_kratos: false,
            rules_keep_ares_off: false,
            mod_files: Default::default(),
            mod_files_ahead: false,
            map_additions: Default::default(),
            resources: Default::default(),
            mod_name: None,
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
            terrain_files: Vec::new(),
            changed_tiles: Default::default(),
        }
    }

    pub(crate) fn with_resources(
        mut self,
        resources: crate::util::mod_definition::ResourceConfig,
    ) -> Self {
        self.resources = resources;
        self
    }

    /// The selected mod's folder name in `Mods`, which the rules parts show.
    pub(crate) fn with_mod_name(mut self, name: Option<String>) -> Self {
        self.mod_name = name;
        self
    }

    pub(crate) fn uses_client_catalog(&self) -> bool {
        self.resources.client_install.is_some()
    }

    pub(crate) fn with_personal_rules(
        mut self,
        rules: crate::rules::personal_rules::PersonalRules,
    ) -> Self {
        self.personal_rules = rules;
        self
    }

    pub(crate) fn with_ares_setting(
        mut self,
        setting: crate::util::ares_launch::AresSetting,
    ) -> Self {
        self.ares_setting = setting;
        self
    }

    pub(crate) fn with_kratos_setting(
        mut self,
        setting: crate::util::kratos_launch::KratosSetting,
    ) -> Self {
        self.kratos_setting = setting;
        self
    }

    /// VERA: the launch's Ares setting where it decides which game this is,
    /// else Auto (`AresSetting::where_it_decides`), which other players'
    /// games must share: the rules digest counts it, the rules parts and the
    /// rules-differ texts name it and Quick Match refuses it.
    pub(crate) fn agreed_ares_setting(&self) -> crate::util::ares_launch::AresSetting {
        self.agreed_ares(self.manager.as_ref())
    }

    /// [`Self::agreed_ares_setting`] by `assets`, the open manager, which
    /// owns the setting; without one, by what startup kept of it.
    fn agreed_ares(&self, assets: Option<&AssetManager>) -> crate::util::ares_launch::AresSetting {
        let (setting, auto_mounts) = match assets {
            Some(assets) => (assets.ares_setting(), assets.auto_mounts_ares()),
            None => (self.ares_setting, self.auto_mounts_ares),
        };
        setting.where_it_decides(auto_mounts, self.rules_keep_ares_off)
    }

    pub(crate) fn startup_kratos(&self) -> bool {
        self.startup_kratos
    }

    /// The player opened a multiplayer menu: start reading the mod's files
    /// now, off the thread that draws, so the pre-match check that follows
    /// rarely waits. A mod's archives take a moment to read, and a run that
    /// never comes here (single player, a map picture) never reads them.
    /// Where no thread starts, that check reads them itself.
    pub(crate) fn read_mod_files_ahead(&mut self) {
        let Some(dir) = self.resources.mod_folder.clone() else {
            return;
        };
        if std::mem::replace(&mut self.mod_files_ahead, true) {
            return;
        }
        let files = std::sync::Arc::clone(&self.mod_files);
        let _ = std::thread::Builder::new()
            .name("mod-files".into())
            .spawn(move || {
                files.get_or_init(|| read_mod_files(&dir));
            });
    }

    /// The launch's mod folder's files, or why they could not be read;
    /// `None` without a mod.
    fn mod_files(&self) -> Option<&Result<ModFiles, String>> {
        let dir = self.resources.mod_folder.as_deref()?;
        Some(self.mod_files.get_or_init(|| read_mod_files(dir)))
    }

    /// The launch mod's map additions, which the matches they reach
    /// (`mod_additions::reach`) add to their maps, or why they could not be
    /// read; `None` without a mod or when its folder holds other files the
    /// game reads. `assets` names the selected roots and catalogs, including
    /// the theater INIs this game reads.
    pub(crate) fn map_additions(
        &self,
        assets: &AssetManager,
    ) -> Result<Option<std::sync::Arc<crate::app::mod_additions::MapAdditions>>, String> {
        let Some(dir) = self.resources.mod_folder.as_deref() else {
            return Ok(None);
        };
        self.map_additions
            .get_or_init(|| {
                let mut names = Vec::from(assets.ini_roots().files().map(str::to_owned));
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

    /// The Options page's Building Production Queue: the next offline
    /// scenario's rules read it.
    pub(crate) fn set_building_production_queue(&mut self, on: bool) {
        self.personal_rules.set_building_production_queue(on);
        if let Some(rules) = self.native_rules.as_mut() {
            rules.set_building_production_queue(on);
        }
    }

    /// The player's own rule switches, which offline games play and a
    /// network room's host shares.
    pub(crate) fn personal_rules(&self) -> &crate::rules::personal_rules::PersonalRules {
        &self.personal_rules
    }

    /// Play `rules` from the next scenario on, and digest them for a network
    /// start (`NativeRulesProcessOwner::use_personal_rules`); a network match
    /// plays the ones `MatchSession::network_personal_rules` picks. Before the
    /// rules load the player's own apply when the sources select.
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

    /// Whether every INI root the rules agreement compares
    /// (`IniRoots::agreed_files`) and the theater suffix use stock YR names
    /// (`IniRoots::is_stock`). Host configuration or a selected mod can
    /// choose others; even a suffix aliasing a shipped theater INI counts as
    /// a custom choice.
    pub(crate) fn stock_ini_roots(&self) -> bool {
        self.selected_ini_roots
            .as_ref()
            .is_none_or(crate::util::ini_roots::IniRoots::is_stock)
    }

    /// Loose files or archives in the game folder that change the rules the
    /// network compares from the retail copies, e.g. `rulesmd.ini` or
    /// `expandmd99.mix`, and with a mod selected a theater's INI or tiles;
    /// found at startup.
    pub(crate) fn changed_game_files(&self) -> &[String] {
        &self.changed_game_files
    }

    /// VERA: the original game's files that change a theater INI or tiles,
    /// e.g. `clear01.sno` or `expandmd06.mix`; found at startup.
    pub(crate) fn terrain_files(&self) -> &[String] {
        &self.terrain_files
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
        let mut assets = AssetManager::with_mod_and_ares(
            root,
            self.resources.mod_folder.as_deref(),
            self.media_archive_mode,
            self.ares_setting,
            &self.resources.archives,
        )?
        .with_mod_sources(&self.resources);
        assets.set_kratos_setting(self.kratos_setting);
        Ok(if let Some(roots) = &self.selected_ini_roots {
            assets.with_selected_ini_roots(roots.clone())
        } else {
            assets.with_ini_roots(&self.resources.ini)?
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
        self.auto_mounts_ares = assets.auto_mounts_ares();
        self.startup_kratos = rules.as_ref().is_some_and(|rules| rules.kratos());
        self.rules_keep_ares_off = rules.as_ref().is_some_and(|rules| rules.ares_kept_off());
        Ok((rules, projection))
    }

    /// A label in the language in use ([`crate::i18n::label`]): VERA's own
    /// (`VERA:`) come from its language files, the game's from the loaded
    /// table, and `fallback` stands in while none is loaded.
    pub(crate) fn csf_label(&self, key: &str, fallback: &str) -> String {
        crate::i18n::label(self.csf.as_ref(), key, fallback)
    }

    /// [`Self::csf_label`] with the key itself standing in while no table is
    /// loaded (a missing label resolves to the CSF's `MISSING:'key'` text).
    pub(crate) fn csf_text(&self, key: &str) -> String {
        self.csf_label(key, key)
    }

    pub(crate) fn audio_catalog(&self) -> Option<&ProcessAudioCatalog> {
        self.audio_catalog.as_ref()
    }

    /// Init_Game52C605/ClearScene685609 use fresh local INI objects while keeping the
    /// campaign array. This selected retail route reads the BATTLEMD file the
    /// INI roots name through the same archive/loose-file resolver as the
    /// other process sources.
    pub(crate) fn reload_campaigns(&mut self, assets: &AssetManager) {
        self.apply_campaign_source(crate::rules::retail_sources::select_ini(
            assets,
            assets.ini_roots().battle(),
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
        let selected = self.manager.as_ref().map(|assets| {
            crate::rules::retail_sources::select_ini(assets, assets.ini_roots().battle())
        });
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

    /// The rules digest network players compare for `mode`; its components
    /// are [`Self::multiplayer_rules_parts`] (docs/spawn.md "Rules parts").
    /// VERA: the original game compares the theater INIs a retail install
    /// ships and no tiles, and leaves its folder's terrain files to each
    /// match (`Self::agreed_inis`, `Self::match_terrain_digest`).
    /// The legacy advertisement CRC remains a separate native UI value.
    /// This is the digest of a game, with the AI INI its folder supplies: a
    /// launcher compares it before it seats anyone (docs/spawn.md). A match's
    /// is [`Self::multiplayer_match_rules_digest`].
    pub(crate) fn multiplayer_rules_digest(&self, mode: &str) -> Result<String, String> {
        self.rules_digest_reading(mode, crate::rules::retail_sources::AiFile::GameFolder)
    }

    /// VERA: [`Self::multiplayer_rules_digest`] of a network match, with the
    /// AI INI it reads (`retail_sources::match_ai_file`): the original one
    /// when it seats no computer (`seats_computers`), so players whose game
    /// folders hold other AI files agree until a computer is seated.
    pub(crate) fn multiplayer_match_rules_digest(
        &self,
        mode: &str,
        seats_computers: bool,
    ) -> Result<String, String> {
        let (_, assets) = self.rules_and_sources()?;
        self.rules_digest_reading(
            mode,
            crate::rules::retail_sources::match_ai_file(assets, true, seats_computers),
        )
    }

    fn rules_digest_reading(
        &self,
        mode: &str,
        ai_file: crate::rules::retail_sources::AiFile,
    ) -> Result<String, String> {
        let (rules, assets) = self.rules_and_sources()?;
        let mut digests: Vec<Option<String>> = self
            .agreed_inis(rules, assets, &[mode], true, ai_file)?
            .into_iter()
            .flatten()
            .map(|ini| ini.digest)
            .collect();
        // VERA: and, with a mod selected, the tiles the game folder changes;
        // none add an absent digest, as the original game does.
        digests.push(
            assets
                .mod_selected()
                .then(|| self.changed_tiles(assets).digest.clone())
                .flatten(),
        );
        // v2 includes the independent high-bridge deck calculation.
        let mut bytes = b"VERA20k rules agreement v2\0".to_vec();
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
        // setting is On (including a selected mod requiring Ares). Such a game adds the byte, so the same files read
        // with and without Ares never agree; with no mod selected and the
        // setting on Auto the installed archive is ignored and adds nothing.
        if assets.ships_ares() {
            bytes.push(1);
        }
        // VERA: the player's Ares setting where it decides which game this
        // is (`Self::agreed_ares_setting`): On, unless Auto mounts `ares.mix`
        // for it too, and Off only when Auto would or these Rules hold an
        // Ares key. Auto, and a setting that opens the game Auto opens, adds
        // nothing. Off also keeps a mode's or a map's Ares key from turning
        // Ares on, which only a match that reads one notices: its start
        // compares whether Ares plays
        // (`shell_network_setup::agreement::start_rules`).
        match self.agreed_ares(Some(assets)) {
            crate::util::ares_launch::AresSetting::Auto => {}
            // On without a mod can play with or without the installation's
            // `ares.mix`; which one is part of the game.
            crate::util::ares_launch::AresSetting::On => {
                bytes.extend_from_slice(b"ares on\0");
                bytes.push(u8::from(assets.ares_mix_mounted()));
            }
            crate::util::ares_launch::AresSetting::Off => bytes.extend_from_slice(b"ares off\0"),
        }
        if self.startup_kratos {
            bytes.extend_from_slice(b"kratos on\0");
        }
        // VERA: RA2 mode (docs/ra2-mode.md) also plays RA2's rules in the
        // simulation (`util::ra2_launch`), which its INI roots alone do not
        // show: config.toml can name the same roots without it. Off adds
        // nothing, so a game without it keeps its digest.
        if crate::util::ra2_launch::enabled() {
            bytes.extend_from_slice(b"ra2 mode\0");
        }
        // mod.ini's display metadata is deliberately absent from the mod-file
        // digest. Its effective loading declarations still select what plays;
        // compare the frozen choice, not the mutable manifest on disk.
        if let Some(selection) = self.resource_selection(assets) {
            bytes.extend_from_slice(b"resource selection\0");
            bytes.push(u8::from(selection.requires_ares));
            bytes.extend_from_slice(self.agreed_ares(Some(assets)).ini_value().as_bytes());
            bytes.push(0);
            for name in selection.roots.iter().chain(&selection.archives) {
                bytes.extend_from_slice(name.as_bytes());
                bytes.push(0);
            }
        }
        // VERA: with a mod in use, its other files too (art, maps, archives),
        // so players whose copies of a mod differ never start a match. Without
        // a mod this adds nothing.
        if let Some(files) = self.mod_files() {
            bytes.extend_from_slice(b"mod files\0");
            bytes.extend_from_slice(&files.as_ref().map_err(String::clone)?.digest);
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

    fn rules_and_sources(
        &self,
    ) -> Result<
        (
            &crate::rules::process_owner::NativeRulesProcessOwner,
            &AssetManager,
        ),
        String,
    > {
        let rules = self
            .native_rules
            .as_ref()
            .ok_or("retail rules not loaded")?;
        let assets = self.manager.as_ref().ok_or("retail sources are loading")?;
        Ok((rules, assets))
    }

    /// The selected mod's folder name in `Mods`: as the launch gave it
    /// ([`Self::with_mod_name`]), else the folder its files are read from
    /// unless that is one assembled from the mod's parts. Empty without one.
    fn mod_folder_name(&self) -> String {
        self.mod_name.clone().unwrap_or_else(|| {
            self.resources
                .mod_folder
                .as_ref()
                .filter(|dir| !dir.is_prepared())
                .and_then(|dir| dir.file_name())
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default()
        })
    }

    /// The INI roots and archive names players' games compare, lowercased,
    /// when a mod requires Ares or any of them is not YR's: the rules
    /// agreement compares them and the rules parts name them. `None` for a
    /// stock choice, which adds nothing.
    /// VERA: the roots are `IniRoots::agreed_files`, which hold what a match
    /// plays and the modes and maps a game offers. The sound, EVA, theme,
    /// campaign, mission and map-selection roots name what a game plays
    /// aloud or shows: two games that differ only in those names play
    /// together. Every archive name stays: an archive mounts into the search
    /// order whatever it is named for, so it can supply any file.
    fn resource_selection(
        &self,
        assets: &AssetManager,
    ) -> Option<crate::app::rules_parts::ResourceSelection> {
        let roots = assets.ini_roots();
        let archives = &self.resources.archives;
        let stock_archives = crate::util::archive_names::ArchiveNames::default();
        let archive_names = [
            (
                archives.expansion_prefix.as_str(),
                stock_archives.expansion_prefix.as_str(),
            ),
            (
                archives.map_prefix.as_str(),
                stock_archives.map_prefix.as_str(),
            ),
            (
                archives.multiplayer.as_str(),
                stock_archives.multiplayer.as_str(),
            ),
            (archives.theme.as_str(), stock_archives.theme.as_str()),
            (
                archives.movie_prefix.as_str(),
                stock_archives.movie_prefix.as_str(),
            ),
        ];
        let chosen = self.resources.requires_ares
            || !roots.agreed_files_are_stock()
            || archive_names
                .iter()
                .any(|(name, stock)| !name.eq_ignore_ascii_case(stock));
        chosen.then(|| crate::app::rules_parts::ResourceSelection {
            requires_ares: self.resources.requires_ares,
            roots: roots.agreed_files().map(str::to_ascii_lowercase).into(),
            archives: archive_names
                .iter()
                .map(|(name, _)| name.to_ascii_lowercase())
                .collect(),
        })
    }

    /// The INI sources the rules agreement digests, in its order: the
    /// process Rules root (as scenarios read it, with the Options switches,
    /// when `switches`; else as selected), LANGRULE, ART, AIMD, the mode's
    /// INI, then each theater's INI: as `map::theater::load_theater` reads it
    /// with a mod selected, else the copy a retail install ships.
    /// One list per mode in `modes`; what no mode changes is read once.
    /// AIMD is the copy `ai_file` names (`retail_sources::select_ai_ini`).
    fn agreed_inis(
        &self,
        rules: &crate::rules::process_owner::NativeRulesProcessOwner,
        assets: &AssetManager,
        modes: &[&str],
        switches: bool,
        ai_file: crate::rules::retail_sources::AiFile,
    ) -> Result<Vec<Vec<AgreedIni>>, String> {
        use crate::rules::ini_parser::IniFile;
        use crate::rules::retail_sources::{select_ai_ini, select_mode_ini};
        let roots = assets.ini_roots();
        let ai = select_ai_ini(assets, ai_file)?;
        let [root, langrule, art] = rules.multiplayer_source_digests();
        let root = if switches {
            root
        } else {
            Some(rules.selected_rules_root().multiplayer_digest())
        };
        let ini = |id: &str, name: &str, digest, optional| AgreedIni {
            id: id.to_owned(),
            name: name.to_owned(),
            digest,
            optional,
        };
        let before_mode = [
            ini("rules", roots.rules(), root, false),
            ini("langrule", "langrule.ini", langrule, true),
            ini("art", roots.art(), art, false),
            ini("ai", roots.ai(), Some(ai.multiplayer_digest()), false),
        ];
        // VERA: a theater INI's tilesets decide every cell's land and height.
        // One that is absent or does not parse fails that theater's load.
        // A mod's game agrees on the INIs it reads. The original game (no
        // mod selected) agrees on the copies a retail install ships, whatever
        // its folder supplies: a network match reads those and the original
        // tiles when its map names no other tile
        // (`loading::init::load_match_theater`), and one whose map does
        // agrees on the folder's terrain at its start
        // (`Self::match_terrain_digest`).
        let shipped = (!assets.mod_selected())
            .then(|| crate::rules::retail_sources::ShippedInis::new(assets));
        let theaters: Vec<AgreedIni> = crate::map::theater::theater_ini_names(assets)
            .into_iter()
            .map(|name| {
                let bytes = match &shipped {
                    Some(shipped) => shipped.used(&name).and_then(|used| used.shipped_bytes()),
                    None => assets.installed_file(&name).map(|(_, bytes)| bytes),
                };
                // VERA's tile-picture key does not count
                // (`retail_sources::vera_drawing_keys`).
                let digest = bytes.and_then(|bytes| {
                    IniFile::from_bytes(bytes).ok().map(|ini| {
                        ini.multiplayer_digest_without(crate::map::theater::VERA_THEATER_KEYS)
                    })
                });
                AgreedIni {
                    id: format!("theater:{}", name.to_ascii_lowercase()),
                    name,
                    digest,
                    optional: false,
                }
            })
            .collect();
        modes
            .iter()
            .map(|mode| {
                let selected_mode = select_mode_ini(assets, Some(*mode))?;
                let mut inis = before_mode.to_vec();
                inis.push(ini(
                    "mode",
                    *mode,
                    selected_mode.as_ref().map(IniFile::multiplayer_digest),
                    true,
                ));
                inis.extend(theaters.iter().cloned());
                Ok(inis)
            })
            .collect()
    }

    /// VERA: [`Self::multiplayer_rules_digest`] of each mode in `modes`, in
    /// parts (`app::rules_parts`): a part per component, in the digest's
    /// order, where each comes from and how it differs from the retail copy,
    /// and the settings the digest covers. The Rules part is the selected
    /// root without the Options switches, which `settings` names.
    pub(crate) fn multiplayer_rules_parts(
        &self,
        modes: &[&str],
    ) -> Result<Vec<crate::app::rules_parts::RulesParts>, String> {
        use crate::app::rules_parts::{
            ModCheck, ModIdentity, RulesFile, RulesParts, RulesSettings, short_digest,
        };
        use crate::rules::ini_parser::IniFile;
        use crate::rules::retail_sources::{ShippedCopy, ShippedInis};
        let (rules, assets) = self.rules_and_sources()?;
        let mod_files = self
            .mod_files()
            .map(|files| files.as_ref().map_err(String::clone))
            .transpose()?;
        let settings = RulesSettings {
            terrain_deformation: rules.personal_rules().terrain_deformation(),
            superweapon_limit: rules.personal_rules().superweapon_limit(),
            building_production_queue: rules.personal_rules().building_production_queue(),
            wasm_budget: rules.personal_rules().wasm_budget(),
            automation_allowed: rules.personal_rules().automation_allowed(),
            // As the digest counts it.
            ares: match self.agreed_ares(Some(assets)) {
                crate::util::ares_launch::AresSetting::Auto => "auto",
                crate::util::ares_launch::AresSetting::On => "on",
                crate::util::ares_launch::AresSetting::Off => "off",
            },
            // Mounted only where the digest counts it: with the setting On
            // (its byte) or with a mod that ships it (`ships_ares`); without
            // a mod only On mounts it, and Off never does.
            ares_mix: assets.ares_mix_mounted(),
            kratos: self.startup_kratos,
            ini_roots_stock: self.stock_ini_roots(),
            // As the digest counts it.
            ra2: crate::util::ra2_launch::enabled(),
            resources: self.resource_selection(assets),
            mod_in_use: self.resources.mod_folder.as_deref().map(|dir| {
                let (title, version) =
                    crate::app::mods::name_and_version(dir, &self.mod_folder_name());
                ModIdentity { title, version }
            }),
            ini: crate::app::rules_parts::agreed_ini(
                assets.ares_setting(),
                self.kratos_setting,
                crate::util::ra2_launch::enabled(),
                rules.personal_rules(),
            ),
        };
        let mod_check = mod_files.and_then(|files| match &files.against {
            crate::app::mod_files::Against::Same => Some(ModCheck {
                changed: Vec::new(),
                missing: Vec::new(),
                extra: Vec::new(),
            }),
            crate::app::mod_files::Against::Differs(differences) => Some(ModCheck {
                changed: differences.changed.clone(),
                missing: differences.missing.clone(),
                extra: differences.extra.clone(),
            }),
            crate::app::mod_files::Against::NoList | crate::app::mod_files::Against::BadList => {
                None
            }
        });
        // Where each component comes from, read once for every mode.
        let shipped = ShippedInis::new(assets);
        // The original game's theater parts are the retail copies and it has
        // no tiles part (`agreed_inis`, `multiplayer_rules_digest`).
        let original = !assets.mod_selected();
        let mut read: std::collections::HashMap<String, RulesFile> = Default::default();
        let mut ini_part = |ini: AgreedIni| {
            let mut part = read
                .entry(ini.name.to_ascii_lowercase())
                .or_insert_with(|| {
                    let mut part = RulesFile {
                        id: String::new(),
                        name: ini.name.clone(),
                        digest: String::new(),
                        source: "unknown".to_owned(),
                        changed: false,
                        sections: None,
                        sections_more: None,
                    };
                    if original && ini.id.starts_with("theater:") {
                        part.source = "retail".to_owned();
                    } else if let Some(used) = shipped.used(&ini.name) {
                        part.source = source_label(assets, used.source);
                        if let ShippedCopy::Changed(retail) = used.shipped {
                            part.changed = true;
                            if let Ok(used) = IniFile::from_bytes(used.bytes) {
                                let retail = retail
                                    .and_then(|bytes| IniFile::from_bytes(bytes).ok())
                                    .unwrap_or_else(IniFile::empty);
                                // VERA's drawing keys count neither here
                                // nor in `ShippedCopy` (an INI that differs
                                // only in them is `Same`), as the digests
                                // leave them out.
                                let dropped = crate::rules::retail_sources::vera_drawing_keys(
                                    assets, &ini.name,
                                );
                                part.set_sections(
                                    used.differing_sections_without(&retail, dropped),
                                );
                            }
                        }
                    }
                    part
                })
                .clone();
            part.id = ini.id;
            part.digest = short_digest(ini.digest.as_deref());
            part
        };
        let tiles = (!original).then(|| self.changed_tiles(assets));
        let tiles_digest = tiles.and_then(|tiles| tiles.digest.as_deref());
        let additions = self.map_additions(assets)?;
        // The game's parts, as a launcher reads them: the folder's AI INI.
        let ai_file = crate::rules::retail_sources::AiFile::GameFolder;
        self.agreed_inis(rules, assets, modes, false, ai_file)?
            .into_iter()
            .map(|inis| {
                let mut files: Vec<RulesFile> = inis
                    .into_iter()
                    .filter(|ini| !ini.optional || ini.digest.is_some())
                    .map(&mut ini_part)
                    .collect();
                files.push(RulesFile {
                    id: "tiles".to_owned(),
                    name: "theater tiles".to_owned(),
                    digest: short_digest(tiles_digest),
                    source: tiles
                        .and_then(|tiles| tiles.files.first())
                        .map_or_else(|| "retail".to_owned(), |file| file_label(assets, file)),
                    changed: tiles_digest.is_some(),
                    sections: None,
                    sections_more: None,
                });
                if let Some(files_read) = mod_files {
                    files.push(RulesFile {
                        id: "mod_files".to_owned(),
                        name: self.mod_folder_name(),
                        digest: short_digest(Some(&crate::util::sha256::digest_hex(
                            files_read.digest,
                        ))),
                        source: "mod".to_owned(),
                        changed: true,
                        sections: None,
                        sections_more: None,
                    });
                }
                if let Some(additions) = additions.as_ref() {
                    files.push(RulesFile {
                        id: "map_additions".to_owned(),
                        name: crate::app::shell_network_setup::agreement::changed_files_text(
                            additions.names(),
                        ),
                        digest: short_digest(Some(&additions.digest())),
                        source: "mod".to_owned(),
                        changed: true,
                        sections: None,
                        sections_more: None,
                    });
                }
                Ok(RulesParts::new(files, settings.clone(), mod_check.clone()))
            })
            .collect()
    }

    /// VERA: whether the loaded network match of the original game reads the
    /// game folder's terrain files: its map names a tile the original game
    /// lacks (`loading::init::load_match_theater`).
    pub(crate) fn match_reads_folder_terrain(&self) -> bool {
        self.manager
            .as_ref()
            .is_some_and(|assets| !assets.mod_selected() && !assets.names_pinned())
    }

    /// VERA: [`Self::terrain_files`] when the loaded match reads them, else
    /// none.
    pub(crate) fn match_terrain_files(&self) -> &[String] {
        if self.match_reads_folder_terrain() {
            &self.terrain_files
        } else {
            &[]
        }
    }

    /// VERA: the terrain files a loaded network match of the original game
    /// adds to its start agreement. `None` when the match reads the original
    /// terrain (`AssetManager::names_pinned`, every player the same files),
    /// when the game folder changes no theater INI or tile, and with a mod
    /// selected, whose rules digest covers them. Otherwise the match's map
    /// names a tile the original game lacks, and it reads the folder's
    /// theater INIs and tiles: these and the changed tiles, as the rules
    /// digest covered them before the original game left them out.
    pub(crate) fn match_terrain_digest(&self) -> Result<Option<String>, String> {
        use crate::rules::ini_parser::IniFile;
        use crate::rules::retail_sources::{ShippedCopy, ShippedInis};
        let (_, assets) = self.rules_and_sources()?;
        if assets.mod_selected() || assets.names_pinned() {
            return Ok(None);
        }
        let shipped = ShippedInis::new(assets);
        let tiles = self.changed_tiles(assets).digest.as_deref();
        let mut changed = tiles.is_some();
        let mut hash = crate::util::sha256::Sha256::new();
        hash.update(b"VERA20k game folder terrain v1\0");
        for name in crate::map::theater::theater_ini_names(assets) {
            let used = shipped.used(&name);
            changed |= used
                .as_ref()
                .is_some_and(|used| used.shipped != ShippedCopy::Same);
            let digest = used
                .and_then(|used| IniFile::from_bytes(used.bytes).ok())
                .map(|ini| ini.multiplayer_digest_without(crate::map::theater::VERA_THEATER_KEYS));
            hash.update(&[u8::from(digest.is_some())]);
            if let Some(digest) = digest {
                hash.update(digest.as_bytes());
            }
        }
        hash.update(&[u8::from(tiles.is_some())]);
        if let Some(tiles) = tiles {
            hash.update(tiles.as_bytes());
        }
        Ok(changed.then(|| crate::util::sha256::digest_hex(hash.finalize())))
    }

    /// A seed regenerates its map after Start, so its rules agreement also
    /// covers the inputs selected by the ordinary generation preparation.
    pub(crate) fn multiplayer_generated_rules_digest(
        &mut self,
        mode: &str,
        options: &crate::map::rmg::RmgOptions,
        seats_computers: bool,
    ) -> Result<String, String> {
        let rules = self.multiplayer_match_rules_digest(mode, seats_computers)?;
        let assets = self.manager.as_mut().ok_or("retail sources are loading")?;
        let mode_rules = crate::rules::retail_sources::select_mode_ini(assets, Some(mode))?;
        // VERA: the preparation loads a theater, which pins the terrain files
        // it reads (`map::theater::load_theater_with`). A match that is
        // loaded keeps the ones it read: its start agreement asks for them
        // after this (`Self::match_terrain_digest`).
        let pinned = assets.pin_names(None);
        let prepared = crate::app::shell_random_map::prepare_random_map_generation(
            assets,
            options,
            crate::app::shell_random_map::RandomMapRulesPreparation::NetworkAgreement(
                self.native_rules.as_ref(),
            ),
            mode_rules.as_ref(),
        );
        assets.pin_names(pinned);
        let prepared = prepared.ok_or("generated map theater is unavailable")?;
        let mut bytes = b"VERA20k generated rules agreement v1\0".to_vec();
        bytes.extend_from_slice(rules.as_bytes());
        bytes.extend_from_slice(prepared.multiplayer_digest().as_bytes());
        Ok(crate::util::sha256::sha256_hex(&bytes))
    }

    /// Hash the process sources once (`crate::rules::native_ini_hash`):
    /// the selected rules root and ART, AIMD, each mode's INI by id
    /// `1..=count` and CoopCampMD.
    /// VERA: also finds the game-folder files that change those sources or
    /// a selected mod's LANGRULE.INI from the retail copies
    /// ([`Self::changed_game_files`]), and the ones that change a theater INI
    /// or a theater's tiles: among those files with a mod selected, whose
    /// agreement compares them, else kept apart ([`Self::terrain_files`]).
    pub(crate) fn install_native_ini_hashes(
        &mut self,
        assets: &AssetManager,
        modes: &[crate::skirmish_modes::SkirmishGameMode],
    ) {
        use crate::rules::ini_parser::IniFile;
        use crate::rules::retail_sources::{select_ini, select_mode_ini};
        let roots = assets.ini_roots();
        let theater_inis = crate::map::theater::theater_ini_names(assets);
        // The selected roster decides which mode INIs the hash adds.
        let mut compared = vec![
            roots.rules(),
            roots.art(),
            roots.ai(),
            roots.modes(),
            roots.map_catalog(),
        ];
        // Match RetailRulesSources' selection: Original mode never consumes
        // the installation's optional LANGRULE, so it changes no game files.
        if assets.mod_selected() {
            compared.push("langrule.ini");
        }
        compared.extend(modes.iter().map(|mode| mode.override_file.as_str()));
        compared
            .push(crate::app::frontend::skirmish_session::cooperative::COOPERATIVE_CAMPAIGN_INI);
        // VERA: a mod's theater INIs and tiles are its game. The original
        // game's are kept apart (`terrain_files`): they change no match whose
        // map names only tiles the original game has.
        let theater_inis: Vec<&str> = theater_inis.iter().map(String::as_str).collect();
        let mut terrain_files =
            crate::rules::retail_sources::files_changing_shipped_inis(assets, &theater_inis);
        for file in &self.changed_tiles(assets).files {
            if !terrain_files.contains(file) {
                terrain_files.push(file.clone());
            }
        }
        if assets.mod_selected() {
            compared.extend(&theater_inis);
        }
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
        if assets.mod_selected() {
            for file in self.changed_tiles(assets).files.clone() {
                if !self.changed_game_files.contains(&file) {
                    self.changed_game_files.push(file);
                }
            }
        } else {
            if !terrain_files.is_empty() {
                log::info!(
                    "Game files that change terrain, read only by a map that needs them: {}",
                    terrain_files.join(", ")
                );
            }
            self.terrain_files = terrain_files;
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
        // VERA: a room is joined before it is known whether it seats a
        // computer, so the join compares the AI INI a match without one
        // reads: the original game's, whatever the folder supplies
        // (`retail_sources::match_ai_file`). A room that seats a computer
        // compares the folder's in its rules digest
        // (`Self::multiplayer_match_rules_digest`).
        let joined = crate::rules::retail_sources::match_ai_file(assets, true, false);
        let ai = match crate::rules::retail_sources::select_ai_ini(assets, joined) {
            Ok(ai) => ai,
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
    fn kratos_agreement_matches_the_game_and_names_it_in_the_rules_parts() {
        use crate::assets::asset_manager::test_support::audio_test_install;
        use crate::util::kratos_launch::KratosSetting::{Auto, Off, On};

        let install = audio_test_install("kratos-agreement");
        for (file, data) in [
            (
                "rulesmd.ini",
                "[InfantryTypes]\n0=GI\n[GI]\nPrimary=GUN\nFearless=no\n",
            ),
            ("aimd.ini", "[AI]\nValue=1\n"),
        ] {
            std::fs::write(install.path().join(file), data).unwrap();
        }
        let game = |setting| {
            let mut process = ProcessAssets::new(MediaArchiveMode::STOCK_DIGITAL, false)
                .with_kratos_setting(setting);
            let assets = process.reconstruct_manager(install.path()).unwrap();
            process.initialize_sources_if_needed(&assets).unwrap();
            process.return_from_loading(assets);
            let parts = process.multiplayer_rules_parts(&[""]).unwrap().remove(0);
            let parts: serde_json::Value = serde_json::from_str(&parts.json()).unwrap();
            (
                process.multiplayer_rules_digest("").unwrap(),
                parts["settings"].get("kratos").cloned(),
            )
        };
        let art = install.path().join("artmd.ini");
        std::fs::write(&art, "[GI]\nPrimaryFireFLH=12,0,60\n").unwrap();
        let stock = game(Auto);
        assert_eq!(stock.1, None);
        assert_eq!(game(Off), stock);
        let enabled = game(On);
        assert_ne!(enabled.0, stock.0);
        assert_eq!(enabled.1, Some(serde_json::Value::Bool(true)));

        std::fs::write(art, "[GI]\nPrimaryCrawlingFLH=17,0,20\n").unwrap();
        assert_eq!(game(Auto), game(On));
        assert_ne!(game(Auto).0, game(Off).0);
    }

    /// VERA: the Ares setting counts between players where it decides which
    /// game opens. Off opens the original game Auto opens, and On the game
    /// Auto opens for a mod whose `ares.mix` it mounts: their digests agree,
    /// their parts say `auto` and neither is a change from the original
    /// game. On without such a mod, Off with one, and Off over Rules that
    /// hold an Ares key open another game.
    #[test]
    fn the_ares_setting_counts_only_where_it_decides_the_game() {
        use crate::assets::asset_manager::test_support::{TestDirectory, audio_test_install};
        use crate::rules::ares_switch::read_ares_way;
        use crate::util::ares_launch::AresSetting::{self, Auto, Off, On};

        let install = audio_test_install("ares-setting-agreement");
        let write_rules = |text: &str| {
            std::fs::write(install.path().join("rulesmd.ini"), text).unwrap();
        };
        write_rules("[General]\nBuildSpeed=.7\n");
        for (name, text) in [
            ("artmd.ini", "[UNIT]\nVoxel=yes\n"),
            ("aimd.ini", "[AI]\nValue=1\n"),
        ] {
            std::fs::write(install.path().join(name), text).unwrap();
        }
        let mod_dir = TestDirectory::new("ares-setting-agreement-mod");
        mod_dir.write_mix("ares.mix", "ares.txt", b"mod Ares");
        let game = |selected_mod: bool, setting: AresSetting| {
            let assets = AssetManager::with_mod_and_ares(
                install.path(),
                selected_mod.then_some(mod_dir.path()),
                MediaArchiveMode::STOCK_DIGITAL,
                setting,
                &Default::default(),
            )
            .unwrap();
            let _reading = read_ares_way(assets.ships_ares());
            let mut process = ProcessAssets::new(MediaArchiveMode::STOCK_DIGITAL, false)
                .with_ares_setting(setting);
            process.initialize_sources_if_needed(&assets).unwrap();
            process.return_from_loading(assets);
            let parts = process.multiplayer_rules_parts(&[""]).unwrap().remove(0);
            let parts: serde_json::Value = serde_json::from_str(&parts.json()).unwrap();
            (
                process.multiplayer_rules_digest("").unwrap(),
                parts["settings"]["ares"].as_str().unwrap().to_owned(),
                process.agreed_ares_setting(),
            )
        };
        let original = game(false, Auto);
        assert_eq!(original.1, "auto");
        assert_eq!(game(false, Off), original);
        let on = game(false, On);
        assert_ne!(on.0, original.0);
        assert_eq!((on.1.as_str(), on.2), ("on", On));

        let with_mod = game(true, Auto);
        assert_ne!(with_mod.0, original.0);
        assert_eq!(game(true, On), with_mod);
        let off = game(true, Off);
        assert_ne!(off.0, with_mod.0);
        assert_eq!((off.1.as_str(), off.2), ("off", Off));

        // Rules whose own key turns Ares on for a player set to Auto: Off
        // plays them without Ares, in every match.
        write_rules(
            "[General]\nBuildSpeed=.7\n[VehicleTypes]\n1=HTNK\n[HTNK]\nImmuneToAbduction=yes\n",
        );
        let keyed = game(false, Auto);
        let kept_off = game(false, Off);
        assert_ne!(kept_off.0, keyed.0);
        assert_eq!((kept_off.1.as_str(), kept_off.2), ("off", Off));
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
                .multiplayer_generated_rules_digest(
                    "",
                    &crate::map::rmg::RmgOptions::default(),
                    true,
                )
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

    /// VERA: a seed room's agreement loads a theater for its inputs, which
    /// pins the terrain files it reads. A loaded match that read the game
    /// folder's keeps them, and with them its start's terrain digest.
    #[test]
    fn a_seed_rooms_agreement_keeps_the_loaded_matchs_terrain_files() {
        let dir = crate::map::theater::terrain_test_install("seed-agreement-terrain");
        for (name, bytes) in [
            ("rulesmd.ini", "[General]\nBuildSpeed=.7\n"),
            ("artmd.ini", "[UNIT]\nVoxel=yes\n"),
            ("aimd.ini", "[AI]\nValue=1\n"),
            ("rmgmd.ini", "[General]\nMaxTrees=500\n"),
        ] {
            std::fs::write(dir.path().join(name), bytes).unwrap();
        }
        std::fs::write(dir.path().join("snow.pal"), [0_u8; 768]).unwrap();
        let mut process = ProcessAssets::new(MediaArchiveMode::STOCK_DIGITAL, false);
        let mut assets = AssetManager::new(dir.path(), MediaArchiveMode::STOCK_DIGITAL).unwrap();
        process.initialize_sources_if_needed(&assets).unwrap();
        crate::map::theater::load_theater(&mut assets, "SNOW").unwrap();
        process.return_from_loading(assets);
        let terrain = process.match_terrain_digest().unwrap();
        assert!(terrain.is_some());
        let snow = crate::map::rmg::RmgOptions {
            theater: 1,
            ..Default::default()
        };
        process
            .multiplayer_generated_rules_digest("", &snow, true)
            .unwrap();
        assert_eq!(process.match_terrain_digest().unwrap(), terrain);
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
                .with_resources(crate::util::mod_definition::ResourceConfig {
                    mod_folder: mod_dir
                        .clone()
                        .map(crate::util::mod_package::ModFolder::loose),
                    ..Default::default()
                });
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
                .with_resources(crate::util::mod_definition::ResourceConfig {
                    mod_folder: Some(crate::util::mod_package::ModFolder::loose(hard.clone())),
                    ..Default::default()
                });
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

    /// VERA: a theater INI's tilesets decide each cell's land and height. The
    /// original game agrees on the copy a retail install ships: a game-folder
    /// theater INI changes neither the agreement nor Quick Match's files, is
    /// kept as a terrain file, and joins the start of a match that reads it.
    /// A mod's game agrees on the INI it reads. The agreement reads the file
    /// a theater suffix selects, else the stock one, as the loader does.
    /// Every INI root the agreement compares, the mode roster's too, counts
    /// against the original game, so Quick Match refuses it before it queues.
    /// A root that names what a game plays aloud or shows, such as EVA's,
    /// changes neither.
    #[test]
    fn ini_roots_and_shipped_theater_inis_take_part_in_the_agreement() {
        use crate::map::source::test_support::make_new_format_mix_bytes;
        use crate::util::ini_roots::IniRootConfig;
        let dir =
            crate::assets::asset_manager::test_support::audio_test_install("theater-agreement");
        let local = make_new_format_mix_bytes(&[
            ("rulesmd.ini", b"[General]\nBuildSpeed=.7\n"),
            ("artmd.ini", b"[UNIT]\nVoxel=yes\n"),
            ("aimd.ini", b"[AI]\nValue=1\n"),
            (
                "snowmd.ini",
                b"[TileSet0000]\nFileName=clear\nTilesInSet=1\n",
            ),
        ]);
        std::fs::remove_file(dir.path().join("localmd.mix")).unwrap();
        std::fs::write(
            dir.path().join("ra2md.mix"),
            make_new_format_mix_bytes(&[("localmd.mix", &local)]),
        )
        .unwrap();
        let mod_dir = dir.path().join("mods-under-test").join("Snow");
        std::fs::create_dir_all(&mod_dir).unwrap();
        std::fs::write(mod_dir.join("notes.txt"), "a mod").unwrap();
        let with_roots = |roots: IniRootConfig, mod_dir: Option<&std::path::Path>| {
            let mut process = ProcessAssets::new(MediaArchiveMode::STOCK_DIGITAL, false);
            if let Some(mod_dir) = mod_dir {
                process = process.with_resources(crate::util::mod_definition::ResourceConfig {
                    mod_folder: Some(crate::util::mod_package::ModFolder::loose(
                        mod_dir.to_owned(),
                    )),
                    ..Default::default()
                });
            }
            let assets =
                AssetManager::with_mod(dir.path(), mod_dir, MediaArchiveMode::STOCK_DIGITAL)
                    .unwrap()
                    .with_ini_roots(&roots)
                    .unwrap();
            process.initialize_sources_if_needed(&assets).unwrap();
            process.install_native_ini_hashes(&assets, &[]);
            process.return_from_loading(assets);
            process
        };
        let started = |theater_suffix: Option<&str>, mod_dir: Option<&std::path::Path>| {
            with_roots(
                IniRootConfig {
                    theater_suffix: theater_suffix.map(str::to_owned),
                    ..IniRootConfig::default()
                },
                mod_dir,
            )
        };
        let digest = |theater_suffix| started(theater_suffix, None).multiplayer_rules_digest("");
        let original = digest(None).unwrap();
        let eva = with_roots(
            IniRootConfig {
                eva: Some("my-eva.ini".to_owned()),
                ..IniRootConfig::default()
            },
            None,
        );
        assert_eq!(eva.multiplayer_rules_digest("").unwrap(), original);
        assert!(eva.stock_ini_roots());
        let modes = with_roots(
            IniRootConfig {
                modes: Some("my-modes.ini".to_owned()),
                ..IniRootConfig::default()
            },
            None,
        );
        assert_ne!(modes.multiplayer_rules_digest("").unwrap(), original);
        assert!(!modes.stock_ini_roots());
        assert_eq!(started(None, None).match_terrain_digest().unwrap(), None);
        let modded = started(None, Some(&mod_dir))
            .multiplayer_rules_digest("")
            .unwrap();
        std::fs::write(
            dir.path().join("snowmd.ini"),
            "[TileSet0000]\nFileName=clear\nTilesInSet=2\n",
        )
        .unwrap();
        let changed = started(None, None);
        assert_eq!(changed.multiplayer_rules_digest("").unwrap(), original);
        assert!(changed.changed_game_files().is_empty());
        assert_eq!(changed.terrain_files(), ["snowmd.ini"]);
        assert!(changed.match_terrain_digest().unwrap().is_some());
        let with_mod = started(None, Some(&mod_dir));
        assert_ne!(with_mod.multiplayer_rules_digest("").unwrap(), modded);
        assert!(
            with_mod
                .changed_game_files()
                .iter()
                .any(|file| file == "snowmd.ini")
        );
        assert_eq!(with_mod.match_terrain_digest().unwrap(), None);
        assert_eq!(
            digest(Some("mo")).unwrap(),
            original,
            "no snowmo.ini: snowmd.ini"
        );
        std::fs::write(dir.path().join("snowmo.ini"), "[General]\n").unwrap();
        assert_ne!(digest(Some("mo")).unwrap(), original);
        // Quick Match refuses any other suffix, even one that picks a shipped
        // file, as `nmd` picks New Urban's `urbannmd.ini` for Urban.
        assert!(changed.stock_ini_roots());
        assert!(!started(Some("nmd"), None).stock_ini_roots());
    }

    #[test]
    fn a_maps_pinned_snow_keeps_the_mods_lobby_agreement() {
        use crate::map::theater::{TerrainFiles, load_theater_with};
        let dir = crate::map::theater::terrain_test_install("pinned-mod-agreement");
        for (file, bytes) in [
            ("rulesmd.ini", "[General]"),
            ("artmd.ini", "[UNIT]"),
            ("aimd.ini", "[AI]"),
        ] {
            std::fs::write(dir.path().join(file), bytes).unwrap();
        }
        let folder = dir.path().join("mod");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(
            folder.join("snowmd.ini"),
            "[TileSet0000]\nFileName=modice\nTilesInSet=1\n",
        )
        .unwrap();
        std::fs::write(folder.join("modice01.sno"), b"mod tile").unwrap();
        let agreement = |pinned| {
            let mut assets =
                AssetManager::with_mod(dir.path(), Some(&folder), MediaArchiveMode::STOCK_DIGITAL)
                    .unwrap();
            let mut process = ProcessAssets::new(MediaArchiveMode::STOCK_DIGITAL, false);
            process.initialize_sources_if_needed(&assets).unwrap();
            if pinned {
                // Same pin boundary as a map using bundled ice, before the first
                // agreement, so its lazy comparison of the mod's tiles runs here.
                load_theater_with(&mut assets, "SNOW", TerrainFiles::Original).unwrap();
                assert!(assets.names_pinned());
            }
            process.return_from_loading(assets);
            (
                process.multiplayer_rules_digest("").unwrap(),
                process.multiplayer_rules_parts(&[""]).unwrap(),
            )
        };
        let expected = agreement(false);
        let parts: serde_json::Value = serde_json::from_str(&expected.1[0].json()).unwrap();
        let file = |id| {
            parts["files"]
                .as_array()
                .unwrap()
                .iter()
                .find(|f| f["id"] == id)
                .unwrap()
        };
        assert_eq!(file("theater:snowmd.ini")["source"], "mod");
        assert_eq!(file("theater:snowmd.ini")["changed"], true);
        assert_eq!(file("tiles")["changed"], true);
        assert_eq!(agreement(true), expected);
    }

    /// VERA: a theater's tiles decide its cells' land and height, and map
    /// loading picks each cell's variant by how many files its tile has. The
    /// original game leaves a game-folder tile that adds a variant, and a
    /// theater MIX that lacks one or does not open, out of the agreement and
    /// of Quick Match's files: it keeps them as terrain files, which join the
    /// start of a match that reads them. The retail bytes change nothing. The
    /// launch probe, which never runs the startup hashes, reports the same
    /// digest.
    #[test]
    fn the_original_game_keeps_changed_theater_tiles_for_the_match_that_reads_them() {
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
        assert!(retail.changed_game_files().is_empty() && retail.terrain_files().is_empty());
        assert_eq!(retail.match_terrain_digest().unwrap(), None);
        let digest = retail.multiplayer_rules_digest("").unwrap();
        std::fs::write(dir.path().join("clear01.sno"), b"tile").unwrap();
        assert!(started().terrain_files().is_empty(), "the retail bytes");
        std::fs::write(dir.path().join("clear01b.sno"), b"next").unwrap();
        let added = started();
        assert!(added.changed_game_files().is_empty());
        assert_eq!(added.terrain_files(), ["clear01b.sno"]);
        assert_eq!(added.multiplayer_rules_digest("").unwrap(), digest);
        let terrain = added.match_terrain_digest().unwrap();
        assert!(terrain.is_some());
        assert_eq!(started().match_terrain_digest().unwrap(), terrain);
        std::fs::write(dir.path().join("clear01b.sno"), b"other").unwrap();
        assert_ne!(started().match_terrain_digest().unwrap(), terrain);
        // `spawn_probe::probe` digests without the startup hashes.
        assert_eq!(
            started_with(false).multiplayer_rules_digest("").unwrap(),
            digest
        );
        std::fs::remove_file(dir.path().join("clear01b.sno")).unwrap();
        std::fs::write(
            dir.path().join("isosnomd.mix"),
            make_new_format_mix_bytes(&[("clear01.sno", b"tile")]),
        )
        .unwrap();
        assert_eq!(started().terrain_files(), ["isosnomd.mix"]);
        std::fs::write(dir.path().join("isosnomd.mix"), b"not a MIX").unwrap();
        assert_eq!(started().terrain_files(), ["isosnomd.mix"]);
    }

    /// VERA: the parts name the changed file, where it comes from and its
    /// changed sections; the Options switches change `settings`, never the
    /// Rules part.
    #[test]
    fn rules_parts_name_changed_sections_and_keep_switches_out_of_the_rules_part() {
        use crate::map::source::test_support::make_new_format_mix_bytes;
        use crate::rules::personal_rules::PersonalRules;
        let dir = crate::assets::asset_manager::test_support::audio_test_install("rules-parts");
        let local = make_new_format_mix_bytes(&[
            (
                "rulesmd.ini",
                b"[General]\nBuildSpeed=.7\n[NUKE]\nDamage=1\n",
            ),
            ("artmd.ini", b"[UNIT]\nVoxel=yes\n"),
            ("aimd.ini", b"[AI]\nValue=1\n"),
        ]);
        std::fs::remove_file(dir.path().join("localmd.mix")).unwrap();
        std::fs::write(
            dir.path().join("ra2md.mix"),
            make_new_format_mix_bytes(&[("localmd.mix", &local)]),
        )
        .unwrap();
        std::fs::write(
            dir.path().join("rulesmd.ini"),
            "[General]\nBuildSpeed=.8\n[NUKE]\nDamage=1\n",
        )
        .unwrap();
        let parts = |rules: PersonalRules| {
            let mut process = ProcessAssets::new(MediaArchiveMode::STOCK_DIGITAL, false)
                .with_personal_rules(rules);
            let assets = AssetManager::new(dir.path(), MediaArchiveMode::STOCK_DIGITAL).unwrap();
            process.initialize_sources_if_needed(&assets).unwrap();
            process.return_from_loading(assets);
            let parts = process.multiplayer_rules_parts(&[""]).unwrap().remove(0);
            serde_json::from_str::<serde_json::Value>(&parts.json()).unwrap()
        };
        let stock = parts(PersonalRules::default());
        let rules = &stock["files"][0];
        assert_eq!(rules["id"], "rules");
        assert_eq!(rules["source"], "loose");
        assert_eq!(rules["changed"], true);
        assert_eq!(rules["sections"], serde_json::json!(["General"]));
        let art = &stock["files"][1];
        assert_eq!(
            (&art["id"], &art["source"]),
            (&"art".into(), &"retail".into())
        );
        assert_eq!(art["changed"], false);
        assert!(art.get("sections").is_none());

        let mut deformation = PersonalRules::default();
        deformation.set_terrain_deformation(true);
        let deformed = parts(deformation);
        assert_eq!(deformed["files"], stock["files"]);
        assert_eq!(deformed["settings"]["terrain_deformation"], true);
        assert_ne!(deformed["digest"], stock["digest"]);
    }

    /// VERA: a room that seats no computer agrees on the original AI INI,
    /// and so does its join, whatever AI INI a game folder holds. A room
    /// that seats one compares the folder's, as a launcher's digest does.
    #[test]
    fn a_room_compares_the_folders_ai_ini_only_when_it_seats_a_computer() {
        use crate::map::source::test_support::make_new_format_mix_bytes;
        let dir =
            crate::assets::asset_manager::test_support::audio_test_install("ai-file-agreement");
        let local = make_new_format_mix_bytes(&[
            ("rulesmd.ini", b"[General]\nBuildSpeed=.7\n"),
            ("artmd.ini", b"[UNIT]\nVoxel=yes\n"),
            ("aimd.ini", b"[AI]\nValue=1\n"),
        ]);
        std::fs::remove_file(dir.path().join("localmd.mix")).unwrap();
        std::fs::write(
            dir.path().join("ra2md.mix"),
            make_new_format_mix_bytes(&[("localmd.mix", &local)]),
        )
        .unwrap();
        let start = || {
            let assets = AssetManager::new(dir.path(), MediaArchiveMode::STOCK_DIGITAL).unwrap();
            let mut process = ProcessAssets::new(MediaArchiveMode::STOCK_DIGITAL, false);
            process.initialize_sources_if_needed(&assets).unwrap();
            process.install_native_ini_hashes(&assets, &[]);
            process.return_from_loading(assets);
            process
        };
        let room = |process: &ProcessAssets, computers| {
            process
                .multiplayer_match_rules_digest("", computers)
                .unwrap()
        };
        let original = start();
        std::fs::write(dir.path().join("aimd.ini"), b"[AI]\nValue=2\n").unwrap();
        let changed = start();
        assert_eq!(changed.changed_game_files(), ["aimd.ini"]);

        assert_eq!(room(&original, false), room(&changed, false));
        assert_eq!(original.native_ini_hashes(), changed.native_ini_hashes());
        assert_ne!(room(&original, true), room(&changed, true));
        // The original game has one digest, whoever is seated.
        assert_eq!(room(&original, false), room(&original, true));
        assert_eq!(
            room(&changed, true),
            changed.multiplayer_rules_digest("").unwrap()
        );
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
    fn ini_roots_process_recovery_keeps_custom_choice_and_refuses_a_foreign_triplet() {
        use crate::util::ini_roots::{IniProfile, IniRootConfig};
        let dir =
            crate::assets::asset_manager::test_support::audio_test_install("ini-roots-process");
        std::fs::write(
            dir.path().join("storm-rules.ini"),
            b"[General]\nBuildSpeed=.7\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("storm-art.ini"), b"[TEST]\nVoxel=yes\n").unwrap();
        std::fs::write(dir.path().join("storm-ai.ini"), b"[Marker]\nValue=Storm\n").unwrap();
        let config = IniRootConfig {
            profile: IniProfile::Custom,
            rules: Some("storm-rules.ini".into()),
            art: Some("storm-art.ini".into()),
            ai: Some("storm-ai.ini".into()),
            ..Default::default()
        };
        let manager = AssetManager::new(dir.path(), MediaArchiveMode::STOCK_DIGITAL)
            .unwrap()
            .with_ini_roots(&config)
            .unwrap();
        let mut process = ProcessAssets::new(MediaArchiveMode::STOCK_DIGITAL, false)
            .with_resources(crate::util::mod_definition::ResourceConfig::from_ini(
                config,
            ));
        process.initialize_sources_if_needed(&manager).unwrap();
        process.return_from_loading(manager);
        drop(process.lease_for_loading().unwrap());
        process.note_lease_ended_without_return();
        for name in ["rulesmd.ini", "artmd.ini", "aimd.ini"] {
            std::fs::write(dir.path().join(name), b"[Late]\nValue=YR\n").unwrap();
        }
        let fresh = AssetManager::new(dir.path(), MediaArchiveMode::STOCK_DIGITAL).unwrap();
        assert!(process.initialize_sources_if_needed(&fresh).is_err());
        let recovered = process.reconstruct_manager(dir.path()).unwrap();
        assert_eq!(recovered.ini_roots().profile(), IniProfile::Custom);
        process.initialize_sources_if_needed(&recovered).unwrap();
        assert!(
            process
                .native_rules()
                .unwrap()
                .fixed_art()
                .section_or_empty("TEST")
                .read_bool("Voxel", false)
        );
    }

    #[test]
    fn custom_files_select_the_campaign_catalog() {
        use crate::util::ini_roots::{IniProfile, IniRootConfig};
        let dir = crate::assets::asset_manager::test_support::audio_test_install("custom-battle");
        for name in ["storm-rules.ini", "storm-art.ini", "storm-ai.ini"] {
            std::fs::write(dir.path().join(name), b"[General]\n").unwrap();
        }
        std::fs::write(
            dir.path().join("battlemd.ini"),
            b"[Battles]\n1=YR\n[YR]\nScenario=YR.MAP\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("storm-battle.ini"),
            b"[Battles]\n1=Storm\n[Storm]\nScenario=STORM.MAP\n",
        )
        .unwrap();
        let config = IniRootConfig {
            profile: IniProfile::Custom,
            rules: Some("storm-rules.ini".into()),
            art: Some("storm-art.ini".into()),
            ai: Some("storm-ai.ini".into()),
            battle: Some("storm-battle.ini".into()),
            ..Default::default()
        };
        let manager = AssetManager::new(dir.path(), MediaArchiveMode::STOCK_DIGITAL)
            .unwrap()
            .with_ini_roots(&config)
            .unwrap();
        let mut process = ProcessAssets::new(MediaArchiveMode::STOCK_DIGITAL, false)
            .with_resources(crate::util::mod_definition::ResourceConfig::from_ini(
                config,
            ));
        process.reload_campaigns(&manager);
        assert_eq!(
            process.campaigns().unwrap().get(0).unwrap().scenario(),
            "STORM.MAP"
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
        let mut process = ProcessAssets::new(MediaArchiveMode::STOCK_DIGITAL, false);
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
