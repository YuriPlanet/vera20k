//! Process-resident ownership for native Rules Type registries.
//!
//! Active YR selects RULESMD/LANGRULE/ARTMD once during `Load_Game_Rules @
//! 0x0052CD70`. Cold startup constructs a partial Type registry, and later
//! preview/Full_Init rebuilds mutate or replace that same process authority.
//! The compatibility `RuleSet` is a projection from those sources; it is never
//! allowed to become a second registry owner.

#[cfg(test)]
#[path = "sinking_sound_tests.rs"]
mod sinking_sound_tests;

#[cfg(test)]
#[path = "selection_voice_tests.rs"]
mod selection_voice_tests;

#[cfg(test)]
#[path = "planning_rules_tests.rs"]
mod planning_rules_tests;

#[cfg(test)]
#[path = "object_image_tests.rs"]
mod object_image_tests;

use std::sync::Arc;

use crate::rules::error::RulesError;
use crate::rules::ini_parser::DigestVerification;
use crate::rules::ini_parser::IniFile;
use crate::rules::native_processing::{
    NativeRulesRegistryState, NativeTypeConstructionEvent, NativeTypeConstructionTrace,
    ProcessedRulesLayers, RulesLayerKind, RulesLayerStack, RulesProcessContext,
    process_native_campaign_rules_prepass, process_native_noncampaign_rules_prepass,
    process_native_rules_cold_start, read_native_multiplayer_country,
    refresh_native_multiplayer_country_bodies,
};
use crate::rules::object_type::ObjectCategory;
use crate::rules::personal_rules::PersonalRules;
use crate::rules::ruleset::RuleSet;
use crate::rules::sound_ini::{EvaRegistry, SoundRegistry};

/// Source snapshot for the optional LANGRULE input. `Selected` retains the
/// physical parse and payload length so startup and every Full_Init reset can
/// repeat ReadCCFile's Digest gate; `Prevalidated` keeps synthetic owner
/// callers' existing contract.
#[derive(Debug)]
pub(crate) enum NativeLangRuleSource {
    Prevalidated(IniFile),
    Selected(crate::rules::retail_sources::SelectedIni),
    SelectionFailed(String),
}

#[derive(Debug)]
enum NativeLangRuleRead {
    Absent,
    Accepted(IniFile),
    DigestRejected,
    /// Native short/malformed Digest comparison reads uninitialized stack
    /// tail bytes. VERA fails closed; this input remains a fidelity residual.
    DigestIndeterminate,
    /// Keep the existing optional-source parse-failure behavior distinct from
    /// physical absence. VERA skips this optional source and runs the ordinary
    /// root/mode tail; equivalence to native malformed-file handling is open.
    SelectionFailed,
}

impl NativeLangRuleSource {
    fn read_for_process(&self) -> NativeLangRuleRead {
        match self {
            Self::Prevalidated(ini) => NativeLangRuleRead::Accepted(ini.clone()),
            Self::Selected(selected) => {
                match selected
                    .physical_ini
                    .verify_digest(selected.source.payload_len)
                {
                    DigestVerification::Empty => NativeLangRuleRead::Accepted(selected.ini.clone()),
                    DigestVerification::Valid => {
                        let mut ini = selected.ini.clone();
                        ini.remove_digest_for_checked_load();
                        NativeLangRuleRead::Accepted(ini)
                    }
                    DigestVerification::Missing | DigestVerification::Invalid => {
                        NativeLangRuleRead::DigestRejected
                    }
                    DigestVerification::Indeterminate => NativeLangRuleRead::DigestIndeterminate,
                }
            }
            Self::SelectionFailed(_) => NativeLangRuleRead::SelectionFailed,
        }
    }

    fn source_ini(&self) -> Option<&IniFile> {
        match self {
            Self::Prevalidated(ini) => Some(ini),
            Self::Selected(selected) => Some(&selected.ini),
            Self::SelectionFailed(_) => None,
        }
    }
}

/// The startup-selected INI objects reused by every later native Process call.
///
/// This is intentionally move-only. Re-reading loose files during a scenario
/// would introduce a mutation window that native's already-selected INI
/// objects do not have.
#[derive(Debug)]
struct NativeRulesSourceSnapshot {
    /// Logical names frozen with the selected snapshots, including MOD roots.
    rules_art_names: [String; 2],
    selected_rules_root: IniFile,
    langrule: Option<NativeLangRuleSource>,
    fixed_art: IniFile,
    /// ARTMD `[Movies]`, selected once by Load_Game_Rules52D121. Movie
    /// references in campaigns and Basic share these indices across reloads.
    movies: crate::rules::movies::MovieRegistry,
    /// Immutable Voc catalog selected once by Init_Game52C763..52C796.
    /// It is not a Rules layer and survives scenario Rules reconstruction.
    fixed_sounds: Arc<SoundRegistry>,
    /// The same immutable EVAMD catalog used by process playback.
    fixed_eva: Arc<EvaRegistry>,
    /// VERA source customization frozen with this process's selected INIs.
    personal_rules: PersonalRules,
}

#[derive(Debug)]
enum NativeRulesRegistryOwner {
    /// Cold startup and shell constructors predate the next numeric-ID reset.
    /// Retain their chronology until that reset, while identities feed E_multi.
    BeforeReset(NativeTypeConstructionTrace),
    /// Later P/P_preview events have transferred to their numeric-ID consumer;
    /// only the continuing registry identity remains process-resident.
    Live(NativeRulesRegistryState),
    /// Completed scenario Rules inputs and their same live registry. Constructor
    /// events have transferred to Scenario; save reads this owner by reference.
    Scenario(Box<ProcessedRulesLayers>),
    /// Full_Init's E_multi has run, but its first Draw still reads this live
    /// registry before Type reset. The owner retains the constructor chronology
    /// until the matching loading attempt finishes or explicitly retires it.
    #[cfg(feature = "ares")]
    PreparedNonCampaign {
        trace: NativeTypeConstructionTrace,
        identity: Arc<()>,
    },
}

impl NativeRulesRegistryOwner {
    fn registry_state(&self) -> &NativeRulesRegistryState {
        match self {
            Self::BeforeReset(trace) => trace.registry_state(),
            Self::Live(state) => state,
            Self::Scenario(processed) => processed.native_registry_state(),
            #[cfg(feature = "ares")]
            Self::PreparedNonCampaign { trace, .. } => trace.registry_state(),
        }
    }

    fn into_shell_state(self) -> (Vec<NativeTypeConstructionEvent>, NativeRulesRegistryState) {
        match self {
            Self::BeforeReset(trace) => {
                let (events, _, state) = trace.into_parts();
                (events, state)
            }
            owner => (Vec::new(), owner.into_registry_state()),
        }
    }

    /// Retire the old scenario envelope while keeping its sole live Type
    /// authority for a pre-reset Rules prefix. Constructor events predate the
    /// next Scenario cursor and are deliberately discarded at this boundary.
    fn into_registry_state(self) -> NativeRulesRegistryState {
        match self {
            Self::BeforeReset(trace) => trace.into_registry_state_discarding_events(),
            Self::Live(state) => state,
            Self::Scenario(processed) => (*processed)
                .into_ini_and_native_type_construction_trace()
                .1
                .into_registry_state_discarding_events(),
            #[cfg(feature = "ares")]
            Self::PreparedNonCampaign { .. } => {
                panic!("prepared Rules prefix requires its loading attempt's ticket")
            }
        }
    }
}

/// The one move-only native Rules authority for a process.
#[derive(Debug)]
pub struct NativeRulesProcessOwner {
    sources: NativeRulesSourceSnapshot,
    /// `None` exists only while an in-place transition is executing. Every
    /// ordinary return, including a failed Process/RuleSet parse, restores it.
    registry: Option<NativeRulesRegistryOwner>,
}

/// Move-only permission to consume or retire one prepared noncampaign prefix.
///
/// This ticket contains no Rules state or constructor events. Pointer identity
/// binds it to the exact preparation, including across different process owners;
/// allocating this presentation capability consumes no simulation RNG or IDs.
#[cfg(feature = "ares")]
#[derive(Debug)]
#[must_use = "a prepared Rules prefix must be consumed or explicitly cancelled"]
pub struct NativeNonCampaignPrefixPreparation {
    identity: Arc<()>,
}

/// One constructor phase transferred to the independent native-ID cursor.
#[derive(Debug)]
pub(crate) struct NativeRulesPhaseReceipt {
    events: Vec<NativeTypeConstructionEvent>,
    allocated_super_weapon_type_count: usize,
}

impl NativeRulesPhaseReceipt {
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn event_count(&self) -> usize {
        self.events.len()
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn allocated_super_weapon_type_count(&self) -> usize {
        self.allocated_super_weapon_type_count
    }

    pub(crate) fn into_parts(self) -> (Vec<NativeTypeConstructionEvent>, usize) {
        (self.events, self.allocated_super_weapon_type_count)
    }
}

/// The exact constructor receipts on the two sides of Full_Init's destructive
/// Rules reset.
#[derive(Debug)]
pub struct NativeScenarioRulesReceipt {
    pre_reset: NativeRulesPhaseReceipt,
    post_reset: NativeRulesPhaseReceipt,
}

impl NativeScenarioRulesReceipt {
    /// Constructor count for the pre-reset native pass, exposed to app tests
    /// that exercise the production Fresh_Init startup/descriptor route.
    #[cfg(any(test, feature = "test-support"))]
    pub fn pre_reset_event_count(&self) -> usize {
        self.pre_reset.event_count()
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn pre_reset(&self) -> &NativeRulesPhaseReceipt {
        &self.pre_reset
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn post_reset(&self) -> &NativeRulesPhaseReceipt {
        &self.post_reset
    }

    pub(crate) fn into_parts(self) -> (NativeRulesPhaseReceipt, NativeRulesPhaseReceipt) {
        (self.pre_reset, self.post_reset)
    }
}

/// Native Process output plus the compatibility products used by current Rust
/// readers. Registry ownership has already returned to the process owner.
pub struct NativeScenarioRulesLoad {
    rules: RuleSet,
    processed_ini: IniFile,
    fixed_art_ini: IniFile,
    receipt: NativeScenarioRulesReceipt,
}

/// The mutually exclusive native pre-reset family and its optional source.
/// Campaigns use a basename INI and never a multiplayer mode override.
pub enum NativeScenarioRulesPrefix<'a> {
    NonCampaign(Option<&'a IniFile>),
    Campaign(Option<&'a IniFile>),
    /// Admitted generated .SED route: Full_Init's saved DL skips ClearClasses.
    RandomMap(Option<&'a IniFile>),
    /// Ordinary E_multi already ran before the first loading frame. Consume
    /// its exact chronology without clearing classes or reading root again.
    #[cfg(feature = "ares")]
    PreparedNonCampaign {
        preparation: NativeNonCampaignPrefixPreparation,
        mode_rules_override: Option<&'a IniFile>,
    },
}

impl NativeScenarioRulesLoad {
    pub fn into_parts(self) -> (RuleSet, IniFile, IniFile, NativeScenarioRulesReceipt) {
        (
            self.rules,
            self.processed_ini,
            self.fixed_art_ini,
            self.receipt,
        )
    }
}

impl NativeRulesProcessOwner {
    /// Enter native cold startup from the already-selected INI snapshots.
    ///
    /// gamemd provenance: `Load_Game_Rules @ 0x0052CD70` followed by
    /// `Init_Game @ 0x0052BA60`.
    pub fn from_cold_start_sources(
        selected_rules_root: IniFile,
        langrule: Option<IniFile>,
        fixed_art: IniFile,
        fixed_sounds: Arc<SoundRegistry>,
        fixed_eva: Arc<EvaRegistry>,
    ) -> Result<Self, RulesError> {
        Self::from_named_cold_start_sources(
            selected_rules_root,
            langrule.map(NativeLangRuleSource::Prevalidated),
            fixed_art,
            fixed_sounds,
            fixed_eva,
            ["RULESMD.INI".into(), "ARTMD.INI".into()],
            PersonalRules::default(),
        )
    }

    /// Preserve the selector's actual logical names beside the frozen inputs.
    /// These diagnostic labels do not alter Rules processing or saved state.
    pub(crate) fn from_named_cold_start_sources(
        selected_rules_root: IniFile,
        langrule: Option<NativeLangRuleSource>,
        fixed_art: IniFile,
        fixed_sounds: Arc<SoundRegistry>,
        fixed_eva: Arc<EvaRegistry>,
        rules_art_names: [String; 2],
        personal_rules: PersonalRules,
    ) -> Result<Self, RulesError> {
        let movies = crate::rules::movies::MovieRegistry::from_art(&fixed_art);
        if let Some(NativeLangRuleSource::SelectionFailed(error)) = langrule.as_ref() {
            log::warn!("Optional LANGRULE.INI selection failed: {error}");
        }
        // A rejected Digest is fatal at startup. Each reset re-reads this same
        // snapshot, so the reset tail's rejected branch cannot follow.
        let startup_langrule = match langrule
            .as_ref()
            .map(NativeLangRuleSource::read_for_process)
        {
            Some(NativeLangRuleRead::Accepted(ini)) => Some(ini),
            Some(NativeLangRuleRead::DigestRejected | NativeLangRuleRead::DigestIndeterminate) => {
                return Err(RulesError::LangRuleDigestRejected);
            }
            None | Some(NativeLangRuleRead::Absent | NativeLangRuleRead::SelectionFailed) => None,
        };
        let cold_trace = process_native_rules_cold_start(
            NativeRulesRegistryState::default(),
            &selected_rules_root,
            &fixed_art,
            startup_langrule.as_ref(),
            &fixed_eva,
        )?;
        Ok(Self {
            sources: NativeRulesSourceSnapshot {
                rules_art_names,
                selected_rules_root,
                langrule,
                fixed_art,
                movies,
                fixed_sounds,
                fixed_eva,
                personal_rules,
            },
            registry: Some(NativeRulesRegistryOwner::BeforeReset(cold_trace)),
        })
    }

    /// Session6980C0/475540 against the retained process INI. This mutates the
    /// one live registry before PrepareSession's separate Country body pass.
    pub fn read_multiplayer_country(&mut self, settings: &IniFile, current: i32) -> i32 {
        self.assert_no_prepared_prefix();
        let (events, state) = self
            .registry
            .take()
            .expect("Rules transition reentered")
            .into_shell_state();
        let (index, mut trace) = read_native_multiplayer_country(
            state,
            &self.sources.selected_rules_root,
            settings,
            current,
        );
        trace.prepend_events(events);
        self.registry = Some(NativeRulesRegistryOwner::BeforeReset(trace));
        index
    }

    /// PrepareSession52E123; intentionally separate from D7's6980C0 call.
    pub fn refresh_multiplayer_country_bodies(&mut self) {
        self.assert_no_prepared_prefix();
        let (events, state) = self
            .registry
            .take()
            .expect("Rules transition reentered")
            .into_shell_state();
        let mut trace =
            refresh_native_multiplayer_country_bodies(state, &self.sources.selected_rules_root);
        trace.prepend_events(events);
        self.registry = Some(NativeRulesRegistryOwner::BeforeReset(trace));
    }

    pub fn multiplayer_countries(&self) -> impl Iterator<Item = (&str, Option<&str>)> {
        self.registry
            .as_ref()
            .expect("Rules transition reentered")
            .registry_state()
            .country_projection()
            .map(|(id, body, ..)| (id, body.read_name("Name", 0x31)))
    }

    /// Every retained country's Ares `File.Flag=` (stock/unknown defaults
    /// included), deduplicated case-insensitively for shell atlas loading.
    #[cfg(feature = "ares")]
    pub fn country_flag_files(&self) -> Vec<String> {
        let state = self
            .registry
            .as_ref()
            .expect("Rules transition reentered")
            .registry_state();
        let mut files = Vec::new();
        for (_, _, _, _, _, country) in state.country_projection() {
            let Some(flag) = country.presentation().flag.as_deref() else {
                continue;
            };
            if !files
                .iter()
                .any(|existing: &String| existing.eq_ignore_ascii_case(flag))
            {
                files.push(flag.to_owned());
            }
        }
        files
    }

    /// Rows consumed by the Ares weighted country picker: registry ID, Name,
    /// Multiplay gate and both weights. Values come from the same retained
    /// Country extension as the shell flag/status projection.
    #[cfg(feature = "ares")]
    pub fn country_random_rows(&self) -> Vec<(&str, Option<&str>, bool, i32, i32)> {
        let state = self
            .registry
            .as_ref()
            .expect("Rules transition reentered")
            .registry_state();
        state
            .country_projection()
            .map(|(id, body, _, _, _, country)| {
                let presentation = country.presentation();
                (
                    id,
                    body.read_name("Name", 0x31),
                    body.read_bool("Multiplay", false),
                    presentation.random_weight,
                    presentation.ai_weight(),
                )
            })
            .collect()
    }

    /// The live registry's SideExt for a side index: `None` for a negative or
    /// unallocated side, where the native hooks skip (`Scenario+0x34B8` < 0).
    /// The score shells read it before any match Rules projection exists.
    #[cfg(feature = "ares")]
    fn side_extension(
        &self,
        side: i32,
    ) -> Option<&crate::rules::ares_side_extension::AresSideExtension> {
        self.registry
            .as_ref()
            .expect("Rules transition reentered")
            .registry_state()
            .side_projection()
            .nth(usize::try_from(side).ok()?)?
            .1
    }

    /// The side's score theme, or `None` where gamemd pushes its literal
    /// `SCORE` name (`0x0082FEBC`): a stock build or a side with no SideExt.
    pub fn side_score_theme(
        &self,
        side: i32,
        theme: crate::rules::ruleset::ScoreTheme,
    ) -> Option<&str> {
        #[cfg(feature = "ares")]
        {
            return self.side_extension(side)?.score_theme(theme);
        }
        #[cfg(not(feature = "ares"))]
        {
            let _ = (side, theme);
            None
        }
    }

    /// The side's seven score-screen file names (SideExt `+0x98..+0x158`). A
    /// stock build and a negative or unallocated side leave every field
    /// `None`, so the callers keep their fixed tables.
    pub fn side_score_art(&self, side: i32) -> crate::rules::ruleset::SideScoreArt<'_> {
        #[cfg(feature = "ares")]
        {
            return self
                .side_extension(side)
                .map(|extension| extension.score_art())
                .unwrap_or_default();
        }
        #[cfg(not(feature = "ares"))]
        {
            let _ = side;
            Default::default()
        }
    }

    /// `Game_GetFlagSurface @ 0x1001DCE0` for a shell HouseType country index:
    /// that country's `File.Flag`, or the `rani.pcx` null-surface branch when
    /// the file is cleared. `None` when no Country holds that index.
    #[cfg(feature = "ares")]
    pub fn country_flag_surface(&self, index: u16) -> Option<&str> {
        let id = self.country_name(i32::from(index))?;
        let state = self
            .registry
            .as_ref()
            .expect("Rules transition reentered")
            .registry_state();
        state
            .country_projection()
            .find(|(candidate, ..)| candidate.eq_ignore_ascii_case(id))
            .map(|(_, _, _, _, _, country)| {
                country
                    .presentation()
                    .flag
                    .as_deref()
                    .unwrap_or(crate::rules::ruleset::STOCK_UNKNOWN_INSIGNIA)
            })
    }

    pub fn country_name(&self, index: i32) -> Option<&str> {
        self.multiplayer_countries()
            .nth(usize::try_from(index).ok()?)
            .map(|(id, _)| id)
    }

    /// Ares loading hooks read the actual retained Country before Full_Init
    /// destroys the registry. Consumers clone this view once per loading attempt.
    #[cfg(feature = "ares")]
    pub fn country_loading(
        &self,
        index: crate::rules::ruleset::CountryIdx,
    ) -> Option<(&crate::rules::country_loading::AresCountryLoading, i32)> {
        self.registry
            .as_ref()
            .expect("Rules transition reentered")
            .registry_state()
            .country_projection()
            .nth(usize::from(index.0))
            .map(|(_, _, _, side, _, country)| (country.loading(), side))
    }

    /// Ares1002FB80 reads the retained Country's Side then Country directly
    /// from CCINI_Rules887048 before Read_Scenario684620. These names use
    /// ReadTheme4758F0's ReadString128; ThemeRuntime owns name/default binding.
    /// Mode/map Process projections and ParentCountry are not read here.
    #[cfg(feature = "ares")]
    pub fn loading_theme_names(
        &self,
        country: crate::rules::ruleset::CountryIdx,
    ) -> [Option<String>; 2] {
        let state = self
            .registry
            .as_ref()
            .expect("Rules transition reentered")
            .registry_state();
        let Some((country, _, _, side, ..)) =
            state.country_projection().nth(usize::from(country.0))
        else {
            return [None, None];
        };
        let read = |id: &str| {
            self.sources
                .selected_rules_root
                .section(id)
                .and_then(|section| section.read_name("LoadingTheme", 128))
                .map(str::to_owned)
        };
        let side = usize::try_from(side)
            .ok()
            .and_then(|index| state.side_projection().nth(index))
            .and_then(|(name, _)| read(name));
        [side, read(country)]
    }

    /// Early Ares6847B7 loading color, resolved against this same retained
    /// registry before E_multi or the later mode/map Rules reset can change it.
    /// `None` when the Country sets no color, leaving the stock side color.
    #[cfg(feature = "ares")]
    pub fn country_loading_text_hsv(
        &self,
        index: crate::rules::ruleset::CountryIdx,
    ) -> Option<[u8; 3]> {
        let (loading, _) = self.country_loading(index)?;
        self.registry
            .as_ref()
            .expect("Rules transition reentered")
            .registry_state()
            .loading_text_hsv(loading.color_index())
    }

    /// Refresh derived frontend country/side fields without replaying Process
    /// or allocating through the compatibility RuleSet.
    pub fn project_multiplayer_countries(&self, rules: &mut RuleSet) {
        let registry = self
            .registry
            .as_ref()
            .expect("Rules transition reentered")
            .registry_state();
        rules.refresh_country_side_projection(
            registry.country_projection(),
            registry.side_projection(),
        );
    }

    /// The same immutable catalog used by process playback; never replaced by
    /// Rules reconstruction or asset-manager recovery.
    #[cfg(any(test, feature = "test-support"))]
    pub fn fixed_sounds(&self) -> &Arc<SoundRegistry> {
        &self.sources.fixed_sounds
    }

    pub(crate) fn bind_type_audio_references(
        &self,
        rules: &mut RuleSet,
        processed: &ProcessedRulesLayers,
    ) {
        rules.bind_type_sound_references(processed.ini(), &self.sources.fixed_sounds);
        rules.bind_terrain_type_sound_references(processed, &self.sources.fixed_sounds);
        rules.bind_type_eva_references(processed.ini(), &self.sources.fixed_eva);
    }

    /// Immutable startup-selected root for consumers with a root-only native read.
    pub fn selected_rules_root(&self) -> &IniFile {
        &self.sources.selected_rules_root
    }

    /// Include VERA's extra source and precedence policy in network admission.
    pub fn personal_rules_compatibility_crc(&self) -> Option<u32> {
        self.sources.personal_rules.compatibility_crc()
    }

    /// Fixed ART snapshot, unaffected by mode/map Rules passes.
    pub fn fixed_art(&self) -> &IniFile {
        &self.sources.fixed_art
    }

    pub fn movies(&self) -> &crate::rules::movies::MovieRegistry {
        &self.sources.movies
    }

    /// Parsed-cache hashes of the actual frozen process sources. These are
    /// not byte SHA256 values and do not reopen files after source selection.
    pub fn selected_source_ini_hashes(&self) -> [(&str, Option<u64>); 3] {
        [
            (
                &self.sources.rules_art_names[0],
                Some(self.sources.selected_rules_root.content_hash()),
            ),
            (
                "LANGRULE.INI",
                self.sources
                    .langrule
                    .as_ref()
                    .and_then(|source| source.source_ini().map(IniFile::content_hash)),
            ),
            (
                &self.sources.rules_art_names[1],
                Some(self.sources.fixed_art.content_hash()),
            ),
        ]
    }

    /// Multiplayer agreement follows the frozen process sources, including
    /// LANGRULE. Reopening those files would hash different state from the
    /// sources Full_Init686B20 actually consumes.
    pub fn multiplayer_source_digests(&self) -> [Option<String>; 3] {
        [
            Some(self.sources.selected_rules_root.multiplayer_digest()),
            self.sources
                .langrule
                .as_ref()
                .and_then(|source| source.source_ini().map(IniFile::multiplayer_digest)),
            Some(self.sources.fixed_art.multiplayer_digest()),
        ]
    }

    /// RulesLoad675210/type hierarchy581F50 restore the reached native fields.
    /// The VERA envelope stores the existing processed state, including fields
    /// that cannot be recovered by parsing a merged INI or replaying Full_Init.
    pub fn saved_scenario_rules(&self) -> Option<&ProcessedRulesLayers> {
        match self.registry.as_ref()? {
            NativeRulesRegistryOwner::Scenario(processed) => Some(processed.as_ref()),
            NativeRulesRegistryOwner::BeforeReset(_) | NativeRulesRegistryOwner::Live(_) => None,
            #[cfg(feature = "ares")]
            NativeRulesRegistryOwner::PreparedNonCampaign { .. } => None,
        }
    }

    /// TerrainReadMap71CAD2 -> FindOrAllocate71E2A0 mutates the continuing
    /// family registry, then TerrainCtor71CB6E spends its own identity. The
    /// caller consumes the move-only new-Type receipt before that instance.
    pub(crate) fn find_or_allocate_map_terrain_type(
        &mut self,
        incoming: &str,
        rules: &mut RuleSet,
    ) -> Result<Option<(String, Option<NativeTypeConstructionEvent>)>, RulesError> {
        let Some(NativeRulesRegistryOwner::Scenario(processed)) = self.registry.as_mut() else {
            return Err(RulesError::MissingScenarioRegistry);
        };
        let Some((terrain, constructor)) = processed.find_or_allocate_map_terrain_type(incoming)
        else {
            return Ok(None);
        };
        let name = terrain.name.clone();
        if constructor.is_some() {
            rules.install_map_terrain_type(terrain);
        }
        Ok(Some((name, constructor)))
    }

    /// Prepare the compatibility projection without mutating process authority.
    /// The shared app loader subsequently binds current ART/theater assets.
    pub fn project_saved_scenario_rules(
        &self,
        processed: &ProcessedRulesLayers,
    ) -> Result<RuleSet, RulesError> {
        processed.validate_saved_state()?;
        let mut rules = RuleSet::from_processed_rules(processed)?;
        self.bind_type_audio_references(&mut rules, processed);
        Ok(rules)
    }

    /// Commit the validated saved registry, replacing the outgoing authority.
    /// Deserialization excludes constructor receipts, so load cannot replay IDs.
    pub fn install_saved_scenario_rules(&mut self, processed: ProcessedRulesLayers) {
        self.assert_no_prepared_prefix();
        self.registry = Some(NativeRulesRegistryOwner::Scenario(Box::new(processed)));
    }

    /// Consume the outgoing Simulation's runtime Rules writes once that
    /// scenario has retired. While a match is live Simulation owns changes;
    /// this process owner receives them only for the next pre-reset Process.
    /// Each update identifies one exact-category, exact-case native stored ID.
    /// All identities/slots are validated before any current field is changed.
    /// Ambient inputs survive Type reset; reached AudioVisual reads replace them.
    ///
    /// Native714570 reads the current field before Full_Init6876AC enters
    /// Reset6686C0. That reset deletes old TechnoTypes; new711082 constructors
    /// start at255, so these values are never transplanted by name across it.
    /// The retired envelope is no longer available to `saved_scenario_rules`.
    pub fn consume_outgoing_scenario_rules(
        &mut self,
        updates: impl IntoIterator<Item = (ObjectCategory, String, i32)>,
        ambient_change: [Option<crate::util::native_x87::NativeF64Bits>; 2],
        upgrade_art_names: impl IntoIterator<Item = (String, u8, String)>,
        voxel_damage: impl IntoIterator<Item = (String, i32)>,
    ) -> Result<(), RulesError> {
        self.assert_no_prepared_prefix();
        let mut registry = self
            .registry
            .take()
            .expect("native Rules registry transition may not be re-entered")
            .into_registry_state();
        let result =
            registry.consume_outgoing_type_runtime_fields(updates, upgrade_art_names, voxel_damage);
        if result.is_ok() {
            registry.consume_outgoing_ambient_change_inputs(ambient_change);
        }
        self.registry = Some(NativeRulesRegistryOwner::Live(registry));
        result
    }

    /// Build the shell-facing compatibility projection without changing the
    /// one native registry owner.
    pub fn startup_compatibility_projection(&self) -> Result<ProcessedRulesLayers, RulesError> {
        let langrule = match self.read_langrule_for_process() {
            NativeLangRuleRead::Accepted(ini) => Some(ini),
            NativeLangRuleRead::Absent
            | NativeLangRuleRead::DigestRejected
            | NativeLangRuleRead::DigestIndeterminate
            | NativeLangRuleRead::SelectionFailed => None,
        };
        self.startup_layers(langrule.as_ref())
            .process_with_fixed_art_and_registry_state_recovering(
                &self.sources.fixed_art,
                NativeRulesRegistryState::default(),
                RulesProcessContext::Ordinary,
                &self.sources.fixed_eva,
            )
            .map_err(|failure| failure.into_parts().0)
    }

    /// RMG InitMapFromSyntheticINI599650 calls ResetTypeRegistriesAndReloadRules
    /// 6686C0 at59A0F0: root, LANG, then committed Session mode668B8E..668BAA.
    /// Preview does not Process the synthetic map
    /// or retain outgoing scenario Type fields. This immutable projection
    /// supplies preview consumers without replacing the process registry.
    pub fn random_map_preview_projection(
        &self,
        mode_rules_override: Option<&IniFile>,
        theater_name: &str,
    ) -> Result<ProcessedRulesLayers, RulesError> {
        let langrule = self.read_langrule_for_process();
        let skip_root_color_add_and_mode = matches!(
            langrule,
            NativeLangRuleRead::DigestRejected | NativeLangRuleRead::DigestIndeterminate
        );
        let langrule = match langrule {
            NativeLangRuleRead::Accepted(ini) => Some(ini),
            NativeLangRuleRead::Absent
            | NativeLangRuleRead::DigestRejected
            | NativeLangRuleRead::DigestIndeterminate
            | NativeLangRuleRead::SelectionFailed => None,
        };
        let mut layers = self.startup_layers(langrule.as_ref());
        if !skip_root_color_add_and_mode && let Some(mode) = mode_rules_override {
            layers.push(RulesLayerKind::GameMode, self.personalized_layer(mode));
        }
        layers
            .process_with_fixed_art_and_registry_state_recovering(
                &self.sources.fixed_art,
                NativeRulesRegistryState::default(),
                RulesProcessContext::ScenarioReset {
                    theater: crate::rules::object_image::ImageTheater::from_name(theater_name),
                    skip_root_color_add_and_mode,
                },
                &self.sources.fixed_eva,
            )
            .map_err(|failure| failure.into_parts().0)
    }

    /// Run ordinary Full_Init's pre-reset E_multi exactly once, before Draw.
    ///
    /// Original68741D/68742E/687451 read root Countries, General and Country
    /// bodies; Draw687588 then observes those fields before TypeReset6876AC.
    /// Ares6847B7/1002F8F0 color and552D58/1001E0E0 PAL consumers run earlier.
    /// The app must transfer any retired scene's runtime Rules writes first.
    /// Generated RMG saved-DL admission deliberately keeps its existing path.
    #[cfg(feature = "ares")]
    pub fn prepare_noncampaign_scenario_prefix(&mut self) -> NativeNonCampaignPrefixPreparation {
        let state = self.take_pre_reset_state(true);
        let trace =
            process_native_noncampaign_rules_prepass(state, &self.sources.selected_rules_root);
        let identity = Arc::new(());
        self.registry = Some(NativeRulesRegistryOwner::PreparedNonCampaign {
            trace,
            identity: Arc::clone(&identity),
        });
        NativeNonCampaignPrefixPreparation { identity }
    }

    /// Retire an attempt which will not reach the destructive Type reset.
    ///
    /// Keep the actual post-prefix registry, including consumed Country parent
    /// flags. Its unconsumed constructor chronology belongs to this cancelled
    /// attempt and must not be replayed by the next Scenario's numeric cursor.
    #[cfg(feature = "ares")]
    pub fn cancel_prepared_noncampaign_scenario_prefix(
        &mut self,
        preparation: NativeNonCampaignPrefixPreparation,
    ) {
        let trace = self.take_prepared_noncampaign_trace(preparation);
        self.registry = Some(NativeRulesRegistryOwner::Live(
            trace.into_registry_state_discarding_events(),
        ));
    }

    fn assert_no_prepared_prefix(&self) {
        #[cfg(feature = "ares")]
        assert!(
            !matches!(
                self.registry.as_ref(),
                Some(NativeRulesRegistryOwner::PreparedNonCampaign { .. })
            ),
            "prepared Rules prefix must be consumed or cancelled before another Rules mutation"
        );
    }

    /// The same pre-reset ownership transfer serves both atomic loading and
    /// first-frame preparation. Check before take so a rejected transition
    /// cannot drop the pending trace or leave the owner empty during unwinding.
    fn take_pre_reset_state(&mut self, clear_classes: bool) -> NativeRulesRegistryState {
        self.assert_no_prepared_prefix();
        let state = self
            .registry
            .take()
            .expect("native Rules registry transition may not be re-entered")
            .into_registry_state();
        #[cfg(feature = "ares")]
        let state = {
            let mut state = state;
            // Full_Init686B5D/686B65 clears classes before the early map/rules
            // pass on ordinary loads. Generated RMG retains RulesExt, and the
            // later TypeReset6686C0 is a separate, retaining transaction.
            if clear_classes {
                state.clear_scenario_global_controls();
            }
            state
        };
        #[cfg(not(feature = "ares"))]
        let _ = clear_classes;
        state
    }

    #[cfg(feature = "ares")]
    fn take_prepared_noncampaign_trace(
        &mut self,
        preparation: NativeNonCampaignPrefixPreparation,
    ) -> NativeTypeConstructionTrace {
        match self.registry.as_ref() {
            Some(NativeRulesRegistryOwner::PreparedNonCampaign { identity, .. }) => assert!(
                Arc::ptr_eq(identity, &preparation.identity),
                "Rules prefix ticket does not belong to the current loading attempt"
            ),
            _ => panic!("Rules prefix ticket has no pending loading attempt"),
        }
        let Some(NativeRulesRegistryOwner::PreparedNonCampaign { trace, .. }) =
            self.registry.take()
        else {
            unreachable!("prepared Rules prefix validated before take")
        };
        trace
    }

    /// Execute the active Full_Init Rules chronology in place.
    ///
    /// Noncampaign runs `E_multi` against the process-retained registry;
    /// campaign runs optional scenario-basename INI P against that registry.
    /// Ordinary Full_Init first clears RulesExt global controls; the admitted
    /// RandomMap arm preserves them under its saved-DL ClearClasses bypass.
    /// The common reset deletes Types, processes root, clears ColorAdd,
    /// processes LANG, then rereads only root ColorAdd before (noncampaign
    /// mode)/map P. The four AudioVisual selectors retain LANG's last read.
    /// Every error restores the actual partial post-reset registry; no path
    /// rolls back to cold/pre-reset state.
    pub fn load_scenario(
        &mut self,
        prefix: NativeScenarioRulesPrefix<'_>,
        map_rules_overrides: &IniFile,
    ) -> Result<NativeScenarioRulesLoad, RulesError> {
        self.load_scenario_inner(prefix, map_rules_overrides, None, None::<fn(RuleSet)>)
            .map(|(load, _)| load)
    }

    /// Run the post-map AI registry read before Full_Init's optional TMCJ4F
    /// Process. The callback takes ownership of the map-stage RuleSet
    /// projection so later consumers can retain native read-time identities;
    /// the same native Rules processor then continues into TMCJ4F at
    /// 0x00687B76.
    ///
    /// RESIDUAL: Full_Init also runs about 20 map readers between the AI
    /// readers and the TMCJ4F gate (0x006879E8..0x00687B13). VERA's map
    /// loading reads the final Rules instead, so a TMCJ4F value one of those
    /// readers consumes takes effect earlier. Stock YR mounts no TMCJ4F.INI.
    pub fn load_scenario_with_post_map<F, T>(
        &mut self,
        prefix: NativeScenarioRulesPrefix<'_>,
        map_rules_overrides: &IniFile,
        tmcj4f_rules: &IniFile,
        after_map: F,
    ) -> Result<(NativeScenarioRulesLoad, T), RulesError>
    where
        F: FnOnce(RuleSet) -> T,
    {
        let (load, after_map_result) = self.load_scenario_inner(
            prefix,
            map_rules_overrides,
            Some(tmcj4f_rules),
            Some(after_map),
        )?;
        Ok((
            load,
            after_map_result.expect("TMCJ4F Rules stack includes the map boundary"),
        ))
    }

    fn load_scenario_inner<F, T>(
        &mut self,
        prefix: NativeScenarioRulesPrefix<'_>,
        map_rules_overrides: &IniFile,
        tmcj4f_rules: Option<&IniFile>,
        after_map: Option<F>,
    ) -> Result<(NativeScenarioRulesLoad, Option<T>), RulesError>
    where
        F: FnOnce(RuleSet) -> T,
    {
        let clear_classes = !matches!(&prefix, NativeScenarioRulesPrefix::RandomMap(_));
        let (pre_reset_trace, mode_rules_override) = match prefix {
            NativeScenarioRulesPrefix::NonCampaign(mode)
            | NativeScenarioRulesPrefix::RandomMap(mode) => {
                let state = self.take_pre_reset_state(clear_classes);
                (
                    process_native_noncampaign_rules_prepass(
                        state,
                        &self.sources.selected_rules_root,
                    ),
                    mode,
                )
            }
            #[cfg(feature = "ares")]
            NativeScenarioRulesPrefix::PreparedNonCampaign {
                preparation,
                mode_rules_override,
            } => (
                self.take_prepared_noncampaign_trace(preparation),
                mode_rules_override,
            ),
            NativeScenarioRulesPrefix::Campaign(named) => {
                let state = self.take_pre_reset_state(true);
                let named = named.map(|ini| self.personalized_layer(ini));
                let trace = match process_native_campaign_rules_prepass(
                    state,
                    named.as_ref(),
                    &self.sources.fixed_art,
                    &self.sources.fixed_eva,
                ) {
                    Ok(trace) => trace,
                    Err(failure) => {
                        let (error, partial_trace) = failure.into_parts();
                        let (_, _, partial_state) = partial_trace.into_parts();
                        self.registry = Some(NativeRulesRegistryOwner::Live(partial_state));
                        return Err(error);
                    }
                };
                (trace, None)
            }
        };
        let (pre_reset_events, pre_reset_super_count, pre_reset_state) =
            pre_reset_trace.into_parts();
        let langrule = self.read_langrule_for_process();
        let skip_root_color_add_and_mode = matches!(
            langrule,
            NativeLangRuleRead::DigestRejected | NativeLangRuleRead::DigestIndeterminate
        );
        let langrule = match langrule {
            NativeLangRuleRead::Accepted(ini) => Some(ini),
            NativeLangRuleRead::Absent
            | NativeLangRuleRead::DigestRejected
            | NativeLangRuleRead::DigestIndeterminate
            | NativeLangRuleRead::SelectionFailed => None,
        };
        let layers = self.scenario_layers(
            mode_rules_override,
            map_rules_overrides,
            langrule.as_ref(),
            tmcj4f_rules,
            skip_root_color_add_and_mode,
        );
        let process_result = match (tmcj4f_rules, after_map) {
            (Some(_), Some(after_map)) => layers
                .process_with_fixed_art_and_registry_state_recovering_after_map(
                    &self.sources.fixed_art,
                    pre_reset_state,
                    RulesProcessContext::ScenarioReset {
                        theater: crate::rules::object_image::ImageTheater::from_map(
                            map_rules_overrides,
                        ),
                        skip_root_color_add_and_mode,
                    },
                    &self.sources.fixed_eva,
                    after_map,
                )
                .map(|(processed, result)| (processed, Some(result))),
            (None, None) => layers
                .process_with_fixed_art_and_registry_state_recovering(
                    &self.sources.fixed_art,
                    pre_reset_state,
                    RulesProcessContext::ScenarioReset {
                        theater: crate::rules::object_image::ImageTheater::from_map(
                            map_rules_overrides,
                        ),
                        skip_root_color_add_and_mode,
                    },
                    &self.sources.fixed_eva,
                )
                .map(|processed| (processed, None)),
            _ => unreachable!("TMCJ4F Rules and its post-map callback are admitted together"),
        };
        let (mut processed, after_map_result) = match process_result {
            Ok((processed, after_map_result)) => (processed, after_map_result),
            Err(failure) => {
                let (error, partial_trace) = failure.into_parts();
                let (_, _, partial_state) = partial_trace.into_parts();
                self.registry = Some(NativeRulesRegistryOwner::Live(partial_state));
                return Err(error);
            }
        };

        let mut rules = match RuleSet::from_processed_rules(&processed) {
            Ok(rules) => rules,
            Err(error) => {
                let (_, post_reset_trace) = processed.into_ini_and_native_type_construction_trace();
                let (_, _, post_reset_state) = post_reset_trace.into_parts();
                self.registry = Some(NativeRulesRegistryOwner::Live(post_reset_state));
                return Err(error);
            }
        };
        self.bind_type_audio_references(&mut rules, &processed);
        debug_assert_eq!(rules.source_ini_hash(), processed.content_hash());
        let processed_ini = processed.ini().clone();
        let (post_reset_events, post_reset_super_count) = processed.take_constructor_events();
        self.registry = Some(NativeRulesRegistryOwner::Scenario(Box::new(processed)));

        Ok((
            NativeScenarioRulesLoad {
                rules,
                processed_ini,
                fixed_art_ini: self.sources.fixed_art.clone(),
                receipt: NativeScenarioRulesReceipt {
                    pre_reset: NativeRulesPhaseReceipt {
                        events: pre_reset_events,
                        allocated_super_weapon_type_count: pre_reset_super_count,
                    },
                    post_reset: NativeRulesPhaseReceipt {
                        events: post_reset_events,
                        allocated_super_weapon_type_count: post_reset_super_count,
                    },
                },
            },
            after_map_result,
        ))
    }

    fn read_langrule_for_process(&self) -> NativeLangRuleRead {
        self.sources.langrule.as_ref().map_or(
            NativeLangRuleRead::Absent,
            NativeLangRuleSource::read_for_process,
        )
    }

    fn startup_layers(&self, langrule: Option<&IniFile>) -> RulesLayerStack {
        let mut layers = RulesLayerStack::new(self.sources.selected_rules_root.clone());
        if let Some(langrule) = langrule {
            layers.push(RulesLayerKind::LangRule, langrule.clone());
        }
        layers
    }

    fn scenario_layers(
        &self,
        mode_rules_override: Option<&IniFile>,
        map_rules_overrides: &IniFile,
        langrule: Option<&IniFile>,
        tmcj4f_rules: Option<&IniFile>,
        skip_mode: bool,
    ) -> RulesLayerStack {
        let mut layers = self.startup_layers(langrule);
        if !skip_mode && let Some(mode) = mode_rules_override {
            layers.push(RulesLayerKind::GameMode, self.personalized_layer(mode));
        }
        layers.push(
            RulesLayerKind::Scenario,
            self.personalized_layer(map_rules_overrides),
        );
        if let Some(tmcj4f) = tmcj4f_rules {
            layers.push(RulesLayerKind::Tmcj4f, self.personalized_layer(tmcj4f));
        }
        layers
    }

    fn personalized_layer(&self, original: &IniFile) -> IniFile {
        let mut rules = original.clone();
        self.sources.personal_rules.apply_later_layer(&mut rules);
        rules
    }
}

#[cfg(test)]
#[path = "session_country_tests.rs"]
mod session_country_tests;

#[cfg(all(test, feature = "ares"))]
#[path = "ares_eva_process_tests.rs"]
mod ares_eva_tests;

#[cfg(all(test, feature = "ares"))]
#[path = "process_owner_global_controls_tests.rs"]
mod global_controls_tests;

#[cfg(all(test, feature = "ares"))]
#[path = "country_loading_prefix_tests.rs"]
mod country_loading_prefix_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::asset_manager::AssetManager;
    use crate::map::source::test_support::TestDirectory;
    use crate::rules::native_processing::RulesLayerStack;
    use crate::rules::retail_sources::RetailRulesSources;

    #[test]
    fn retired_voxel_damage_survives_omitted_read_until_type_reset() {
        let root = IniFile::from_str("[VoxelAnims]\n0=CHUNK\n[CHUNK]\nName=Chunk\n");
        let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
            root.clone(),
            None,
            IniFile::empty(),
            Arc::default(),
            Arc::default(),
        )
        .unwrap();
        let loaded = owner
            .load_scenario(
                NativeScenarioRulesPrefix::Campaign(None),
                &IniFile::from_str("[CHUNK]\nDamage=20\n"),
            )
            .unwrap();
        let damage = |rules: &RuleSet| {
            rules
                .voxel_anim_type(rules.voxel_anim_type_id_by_name("CHUNK").unwrap())
                .damage
        };
        assert_eq!(damage(&loaded.rules), 20);

        // Voxel74A91B can mutate Type.Damage during a scene. Read74B3D8
        // uses that current field when the next Process omits Damage.
        owner
            .consume_outgoing_scenario_rules([], [None; 2], [], [("CHUNK".into(), 3)])
            .unwrap();
        let state = owner.registry.take().unwrap().into_registry_state();
        let reread = RulesLayerStack::new(root)
            .process_with_fixed_art_and_registry_state(&IniFile::empty(), state)
            .unwrap();
        assert_eq!(damage(&RuleSet::from_processed_rules(&reread).unwrap()), 3);

        // Full_Init's later TypeReset6686C0 deletes the old type; its new
        // constructor74AD80 starts Damage at zero rather than transplanting3.
        owner.install_saved_scenario_rules(reread);
        let reset = owner
            .load_scenario(NativeScenarioRulesPrefix::Campaign(None), &IniFile::empty())
            .unwrap();
        assert_eq!(damage(&reset.rules), 0);
    }

    #[test]
    fn tmcj4f_runs_after_team_ai_read_and_does_not_retroactively_bind_a_member() {
        let rulesmd = IniFile::from_str("[InfantryTypes]\n0=E1\n[E1]\nStrength=100\nTechLevel=1\n");
        let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
            rulesmd,
            None,
            IniFile::empty(),
            Arc::default(),
            Arc::default(),
        )
        .unwrap();
        let ai_ini = IniFile::from_str(
            "[TeamTypes]\n0=TEAM\n[TEAM]\nScript=SCRIPT\nTaskForce=TF\n\
             [ScriptTypes]\n0=SCRIPT\n[SCRIPT]\n0=0,0\n\
             [TaskForces]\n0=TF\n[TF]\n0=1,TMCJONLY\n",
        );
        let tmcj4f = IniFile::from_str(
            "[InfantryTypes]\n0=TMCJONLY\n[TMCJONLY]\nStrength=100\nTechLevel=1\n",
        );
        let (loaded, ai_diagnostics) = owner
            .load_scenario_with_post_map(
                NativeScenarioRulesPrefix::NonCampaign(None),
                &IniFile::empty(),
                &tmcj4f,
                |rules_at_ai_read| {
                    let registry = crate::rules::team_ai_ini::TeamAiIniRegistry::from_sources(
                        &IniFile::empty(),
                        &ai_ini,
                        true,
                    );
                    let mut interner = crate::sim::intern::StringInterner::default();
                    crate::sim::team_script_vm::TeamScriptVm::from_ini_registry(
                        &registry,
                        &mut interner,
                        &rules_at_ai_read,
                    )
                    .1
                },
            )
            .unwrap();

        assert!(loaded.rules.infantry_ids.iter().any(|id| id == "TMCJONLY"));
        assert!(ai_diagnostics.iter().any(|diagnostic| matches!(
            diagnostic,
            crate::sim::team_script_vm::TeamAiInstallDiagnostic::UnknownTaskForceMember {
                member_type,
                ..
            } if member_type == "TMCJONLY"
        )));
    }

    #[test]
    fn langrule_without_a_digest_fails_startup() {
        fn load(with_langrule: bool) -> Result<RuleSet, String> {
            let directory = TestDirectory::new(if with_langrule {
                "langrule-missing-digest"
            } else {
                "langrule-absent"
            });
            directory.write("rulesmd.ini", b"[General]\nBuildSpeed=.8\n");
            directory.write("artmd.ini", b"");
            if with_langrule {
                directory.write("langrule.ini", b"[General]\nBuildSpeed=.7\n");
            }
            let assets = AssetManager::from_loose_root_for_test(directory.path());
            let mut owner = RetailRulesSources::select(&assets)
                .unwrap()
                .into_owner(Arc::default(), Arc::default())?;
            let mode = IniFile::from_str("[General]\nBuildSpeed=.5\n");
            Ok(owner
                .load_scenario(
                    NativeScenarioRulesPrefix::NonCampaign(Some(&mode)),
                    &IniFile::from_str("[General]\nTreeStrength=321\n"),
                )
                .unwrap()
                .rules)
        }

        assert!(
            load(true)
                .unwrap_err()
                .ends_with(&RulesError::LangRuleDigestRejected.to_string())
        );
        let absent = load(false).unwrap();
        assert_eq!(
            absent.production.build_speed,
            crate::util::native_x87::NativeF64Bits::from_bits(f64::from(0.5_f32).to_bits()),
            "an absent LANGRULE is not a Digest failure",
        );
        assert_eq!(absent.general.tree_strength, 321, "map Rules still run");
    }

    #[test]
    fn selected_langrule_digest_gates_cold_start_and_startup_projection() {
        use crate::rules::retail_ini_fixture::DIGESTED_LANGRULE_CONTROL as VALID_LANGRULE;

        fn startup_color_add(langrule: &str) -> Result<[u8; 3], String> {
            let directory = TestDirectory::new("startup-langrule-digest");
            directory.write("rulesmd.ini", b"[General]\nBuildSpeed=.8\n");
            directory.write("artmd.ini", b"");
            directory.write("langrule.ini", langrule.as_bytes());
            let assets = AssetManager::from_loose_root_for_test(directory.path());
            let owner = RetailRulesSources::select(&assets)
                .unwrap()
                .into_owner(Arc::default(), Arc::default())?;
            let projection = owner.startup_compatibility_projection().unwrap();
            Ok(RuleSet::from_processed_rules(&projection)
                .unwrap()
                .color_add
                .entries()[0]
                .rgb)
        }

        assert_eq!(startup_color_add(VALID_LANGRULE).unwrap(), [7, 8, 9]);
        let invalid_langrule = VALID_LANGRULE.replacen(
            "1=5Z9CTq3TQS20/wfWQHiRlHIBMME=",
            "1=AZ9CTq3TQS20/wfWQHiRlHIBMME=",
            1,
        );
        assert!(
            startup_color_add(&invalid_langrule)
                .unwrap_err()
                .ends_with(&RulesError::LangRuleDigestRejected.to_string())
        );
    }
}
