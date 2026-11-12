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

use std::borrow::Cow;
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
    /// Only its Options switches change later, and only scenario roots read them.
    personal_rules: PersonalRules,
    /// Startup mounted `ares.mix`: Ares plays in every game this process
    /// loads (`rules::ares_switch`).
    ships_ares: bool,
}

#[derive(Debug)]
enum NativeRulesRegistryOwner {
    /// Cold startup and shell constructors predate the next numeric-ID reset.
    /// Retain their chronology until that reset, while identities feed E_multi.
    BeforeReset(NativeTypeConstructionTrace),
    /// Later P/P_preview events have transferred to their numeric-ID consumer;
    /// only the continuing registry identity remains process-resident.
    Live(NativeRulesRegistryState),
    /// RMG's last real reset and Process result. Cancel retires the dialog,
    /// not these Types or process globals; a cache hit projects this envelope.
    Preview(Box<ProcessedRulesLayers>),
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
            Self::Preview(processed) | Self::Scenario(processed) => {
                processed.native_registry_state()
            }
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
            Self::Preview(processed) => {
                let (_, trace) = (*processed).into_ini_and_native_type_construction_trace();
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
            Self::Preview(processed) | Self::Scenario(processed) => (*processed)
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
    /// Immutable Process result for dependencies reached before map generation.
    /// The constructor receipt stays in this move-only load until admission.
    pub fn rules(&self) -> &RuleSet {
        &self.rules
    }

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
            &fixed_sounds,
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
                // The game's startup sets it (`RetailRulesSources::into_owner`);
                // an `ares` test build's fixture owners stand for the Ares
                // game, as `RulesLayerStack::new` says.
                ships_ares: cfg!(all(feature = "ares", any(test, feature = "test-support"))),
            },
            registry: Some(NativeRulesRegistryOwner::BeforeReset(cold_trace)),
        })
    }

    /// Record whether startup mounted `ares.mix` (`AssetManager::ships_ares`),
    /// before any Process call reads its layers.
    pub fn with_ares_mix(mut self, ships_ares: bool) -> Self {
        self.sources.ships_ares = ships_ares;
        self
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

    /// The side's `DialogBackground.Image/.Palette` names for the shell dialog
    /// chrome ([`AresSideExtension::dialog_background`]), or `None` to keep
    /// the stock per-side backgrounds: a side with no SideExt, or one that
    /// leaves either name unset.
    ///
    /// [`AresSideExtension::dialog_background`]: crate::rules::ares_side_extension::AresSideExtension::dialog_background
    #[cfg(feature = "ares")]
    pub fn side_dialog_background(&self, side: i32) -> Option<(&str, &str)> {
        self.side_extension(side)?.dialog_background()
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
        rules.bind_type_sound_references(processed, &self.sources.fixed_sounds);
        rules.bind_terrain_type_sound_references(processed, &self.sources.fixed_sounds);
        rules.bind_type_eva_references(processed.ini(), &self.sources.fixed_eva);
    }

    /// Immutable startup-selected root for consumers with a root-only native read.
    pub fn selected_rules_root(&self) -> &IniFile {
        &self.sources.selected_rules_root
    }

    /// Include VERA's extra source, precedence policy and Options switches in
    /// network admission.
    pub fn personal_rules_compatibility_crc(&self) -> Option<u32> {
        self.sources.personal_rules.compatibility_crc()
    }

    /// The Options page's Terrain Deformation box, read by the next scenario.
    pub fn set_terrain_deformation(&mut self, on: bool) {
        self.sources.personal_rules.set_terrain_deformation(on);
    }

    /// The root every scenario's Rules start from: the selected root with the
    /// Options switches applied.
    fn scenario_root(&self) -> Cow<'_, IniFile> {
        self.sources
            .personal_rules
            .scenario_root(&self.sources.selected_rules_root)
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
    /// sources Full_Init686B20 actually consumes. The root is digested as
    /// scenarios read it, with the Options switches applied.
    pub fn multiplayer_source_digests(&self) -> [Option<String>; 3] {
        [
            Some(self.scenario_root().multiplayer_digest()),
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
            NativeRulesRegistryOwner::BeforeReset(_)
            | NativeRulesRegistryOwner::Live(_)
            | NativeRulesRegistryOwner::Preview(_) => None,
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
                &self.sources.fixed_sounds,
            )
            .map_err(|failure| failure.into_parts().0)
    }

    /// Read-only prospective generation inputs for network agreement. Actual
    /// Generate uses `prepare_random_map_preview` and retains its full result.
    /// Carry the process-global Powerups, land and sound inputs into this
    /// temporary source projection; no live table or registry is replaced.
    pub fn random_map_preview_projection(
        &self,
        mode_rules_override: Option<&IniFile>,
        theater_name: &str,
    ) -> Result<ProcessedRulesLayers, RulesError> {
        let (layers, skip_root_color_add_and_mode) =
            self.random_map_preview_layers(mode_rules_override);
        let state = self
            .registry
            .as_ref()
            .expect("Rules transition reentered")
            .registry_state()
            .retained_random_map_projection_seed();
        layers
            .process_with_fixed_art_and_registry_state_recovering(
                &self.sources.fixed_art,
                state,
                RulesProcessContext::ScenarioReset {
                    theater: crate::rules::object_image::ImageTheater::from_name(theater_name),
                    skip_root_color_add_and_mode,
                },
                &self.sources.fixed_eva,
                &self.sources.fixed_sounds,
            )
            .map_err(|failure| failure.into_parts().0)
    }

    /// RMG599650 resets the actual Type registries at59A0F0 only on a cache
    /// miss. Powerups keeps raw animation indices across that reset, so its
    /// table and the rebuilt registry must remain under this same owner.
    /// Root, accepted LANG and committed Session mode follow6686C0; the
    /// synthetic map and Full_Init's E_multi do not run on this preview arm.
    pub fn prepare_random_map_preview(
        &mut self,
        mode_rules_override: Option<&IniFile>,
        theater_name: &str,
        reinitialize: bool,
    ) -> Result<(), RulesError> {
        self.assert_no_prepared_prefix();
        if !reinitialize {
            let Some(NativeRulesRegistryOwner::Preview(processed)) = self.registry.as_mut() else {
                panic!("RMG cache hit requires its retained Rules result")
            };
            // Every Generate calls683610 at599B23, which resets Scenario+214
            // at683633 before the cache gate. Old preview Type IDs expire;
            // the59A0E2 hit constructs no new Types and spends no Scenario RNG.
            let _ = processed.take_constructor_events();
            return Ok(());
        }

        let (layers, skip_root_color_add_and_mode) =
            self.random_map_preview_layers(mode_rules_override);
        let state = self.take_pre_reset_state(false);
        match layers.process_with_fixed_art_and_registry_state_recovering(
            &self.sources.fixed_art,
            state,
            RulesProcessContext::ScenarioReset {
                theater: crate::rules::object_image::ImageTheater::from_name(theater_name),
                skip_root_color_add_and_mode,
            },
            &self.sources.fixed_eva,
            &self.sources.fixed_sounds,
        ) {
            Ok(processed) => {
                self.registry = Some(NativeRulesRegistryOwner::Preview(Box::new(processed)));
                Ok(())
            }
            Err(failure) => {
                let (error, partial_trace) = failure.into_parts();
                // Keep reached writes and this preview's chronology even when
                // a later reader fails. The next numeric reset retires events.
                self.registry = Some(NativeRulesRegistryOwner::BeforeReset(partial_trace));
                Err(error)
            }
        }
    }

    /// Immutable view of the last real RMG Process. Dialog cancellation leaves
    /// this owner in place; the next reset or scenario load replaces its view.
    pub fn prepared_random_map_rules(&self) -> Option<&ProcessedRulesLayers> {
        match self.registry.as_ref()? {
            NativeRulesRegistryOwner::Preview(processed) => Some(processed),
            _ => None,
        }
    }

    fn random_map_preview_layers(
        &self,
        mode_rules_override: Option<&IniFile>,
    ) -> (RulesLayerStack, bool) {
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
        (layers, skip_root_color_add_and_mode)
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
                    &self.sources.fixed_sounds,
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
                    &self.sources.fixed_sounds,
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
                    &self.sources.fixed_sounds,
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
        let mut layers = RulesLayerStack::new(self.scenario_root().into_owned())
            .with_ares_mix(self.sources.ships_ares);
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
    fn retail_class_lights_convert_omitted_keys_only_in_reached_sections() {
        let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let lights = |rules: &RuleSet| {
            [
                rules.general.extra_unit_light,
                rules.general.extra_infantry_light,
                rules.general.extra_aircraft_light,
            ]
        };
        let mut owner = crate::rules::retail_ini_fixture::retail_rules_owner(&assets);
        let stock = owner
            .load_scenario(
                NativeScenarioRulesPrefix::NonCampaign(None),
                &IniFile::empty(),
            )
            .unwrap();
        assert_eq!(lights(&stock.rules), [200; 3]);
        let unrelated = IniFile::from_str("[AudioVisual]\nBerserkColor=6\n");
        let omitted = owner
            .load_scenario(NativeScenarioRulesPrefix::NonCampaign(None), &unrelated)
            .unwrap();
        assert_eq!(lights(&omitted.rules), [0; 3], "native integer defaults");
        let signed = IniFile::from_str(
            "[AudioVisual]\nExtraUnitLight=-1.25\nExtraInfantryLight=2.75\nExtraAircraftLight=3.25\n",
        );
        let absent = owner
            .load_scenario(
                NativeScenarioRulesPrefix::NonCampaign(Some(&signed)),
                &IniFile::empty(),
            )
            .unwrap();
        assert_eq!(lights(&absent.rules), [-1250, 2750, 3250]);
        let omitted = owner
            .load_scenario(
                NativeScenarioRulesPrefix::NonCampaign(Some(&signed)),
                &unrelated,
            )
            .unwrap();
        assert_eq!(lights(&omitted.rules), [-1000, 2000, 3000]);
        let empty = IniFile::from_str(
            "[AudioVisual]\nBerserkColor=6\nExtraUnitLight=\nExtraInfantryLight=\nExtraAircraftLight=\n",
        );
        let empty = owner
            .load_scenario(
                NativeScenarioRulesPrefix::NonCampaign(Some(&signed)),
                &empty,
            )
            .unwrap();
        assert_eq!(lights(&empty.rules), [-1000, 2000, 3000]);
    }

    #[test]
    fn class_light_history_survives_preview_failure_cancel_and_saved_rules() {
        let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let retail = crate::rules::retail_ini_fixture::retail_rules_owner(&assets);
        let mut root = retail.sources.selected_rules_root.clone();
        // An explicit mod root without AudioVisual makes retained history
        // observable; the actual retail root replaces all three with .2.
        root.remove_section("AudioVisual");
        let lights = |rules: &RuleSet| {
            [
                rules.general.extra_unit_light,
                rules.general.extra_infantry_light,
                rules.general.extra_aircraft_light,
            ]
        };
        let mut hashes = Vec::new();
        for (first, expected_unit) in [("1.25", 1000), ("2.25", 2000)] {
            let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
                root.clone(),
                None,
                retail.sources.fixed_art.clone(),
                retail.sources.fixed_sounds.clone(),
                retail.sources.fixed_eva.clone(),
            )
            .unwrap();
            owner
                .load_scenario(
                    NativeScenarioRulesPrefix::Campaign(None),
                    &IniFile::from_str(&format!(
                        "[AudioVisual]\nExtraUnitLight={first}\nExtraInfantryLight=-1.25\nExtraAircraftLight=.5\n",
                    )),
                )
                .unwrap();
            let expected = [expected_unit, -1000, 0];
            owner
                .prepare_random_map_preview(
                    Some(&IniFile::from_str("[AudioVisual]\nBerserkColor=6\n")),
                    "TEMPERATE",
                    true,
                )
                .unwrap();
            let preview =
                RuleSet::from_processed_rules(owner.prepared_random_map_rules().unwrap()).unwrap();
            assert_eq!(lights(&preview), expected);
            owner
                .prepare_random_map_preview(
                    Some(&IniFile::from_str("[AudioVisual]\nExtraUnitLight=9\n")),
                    "TEMPERATE",
                    false,
                )
                .unwrap();
            let prospective = owner
                .random_map_preview_projection(None, "TEMPERATE")
                .unwrap();
            assert_eq!(
                lights(&RuleSet::from_processed_rules(&prospective).unwrap()),
                expected
            );
            // Process668F56 reads AudioVisual before Tiberium668F65. A
            // later failure keeps the already reached light write.
            let failed = IniFile::from_str(
                "[Tiberiums]\n-1=Riparius\n[AudioVisual]\nExtraInfantryLight=9\n",
            );
            assert!(
                owner
                    .prepare_random_map_preview(Some(&failed), "TEMPERATE", true)
                    .is_err()
            );
            let expected = [expected_unit, 9000, 0];
            let loaded = owner
                .load_scenario(NativeScenarioRulesPrefix::Campaign(None), &IniFile::empty())
                .unwrap();
            assert_eq!(
                lights(&loaded.rules),
                expected,
                "cancel/next reset keeps scalars"
            );
            let bytes =
                crate::util::save_codec::serialize(owner.saved_scenario_rules().unwrap()).unwrap();
            owner.install_saved_scenario_rules(
                crate::util::save_codec::deserialize(&bytes).unwrap(),
            );
            let restored = owner
                .project_saved_scenario_rules(owner.saved_scenario_rules().unwrap())
                .unwrap();
            assert_eq!(lights(&restored), expected);
            assert_eq!(
                restored.simulation_config_hash(),
                loaded.rules.simulation_config_hash()
            );
            hashes.push((
                restored.source_ini_hash(),
                restored.simulation_config_hash(),
            ));
        }
        assert_eq!(hashes[0].0, hashes[1].0, "identical final sources");
        assert_ne!(
            hashes[0].1, hashes[1].1,
            "retained light enters the effective hash"
        );
    }

    #[test]
    fn retail_crush_sounds_retain_fixed_catalog_bindings() {
        let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let mut owner = crate::rules::retail_ini_fixture::retail_rules_owner(&assets);
        let loaded = owner
            .load_scenario(
                NativeScenarioRulesPrefix::NonCampaign(Some(&IniFile::from_str(
                    "[GASAND]\nCrushSound=\n[CAKRMW]\nCrushSound=Unknown\n",
                ))),
                &IniFile::from_str("[CAFNCB]\nCrushSound=Unknown\n"),
            )
            .unwrap();
        let overlays = loaded.rules.overlay_registry();
        for (name, sound) in [
            ("GASAND", "WallCrushSandbag"),
            ("CAFNCB", "WallCrushBlack"),
            ("CAFNCW", "WallCrushWhite"),
            ("CAKRMW", "WallCrushKremlin"),
            ("CAFNCP", "WallCrushBlack"),
        ] {
            assert_eq!(
                overlays
                    .flags(overlays.id_for_name(name).unwrap())
                    .unwrap()
                    .crush_sound
                    .as_deref(),
                Some(sound),
                "{name}: ObjectTypeRead5F938B retains the fixed Voc binding"
            );
        }
        assert_eq!(
            loaded.rules.object("E1").unwrap().crush_sound.as_deref(),
            Some("InfantrySquish")
        );
    }

    #[test]
    fn object_crush_sound_history_survives_save_but_not_type_reset() {
        let sounds = Arc::new(SoundRegistry::from_ini(&IniFile::from_str(
            "[SoundList]\n0=First\n1=Second\n2=none\n3=<none>\n",
        )));
        let later = IniFile::from_str(
            "[InfantryTypes]\n1=SAME\n[SAME]\nCrushSound=Unknown\n[E1]\nCrushSound=<none>\n",
        );
        let cue = |rules: &RuleSet| {
            let overlays = rules.overlay_registry();
            overlays
                .flags(overlays.id_for_name("SAME").unwrap())
                .unwrap()
                .crush_sound
                .clone()
        };
        let mut hashes = Vec::new();
        for first in ["First", "Second"] {
            let root = IniFile::from_str(&format!(
                "[OverlayTypes]\n0=SAME\n1=EMPTY\n[InfantryTypes]\n0=E1\n\
                 [SAME]\nCrushSound={first}\n[EMPTY]\nCrushSound=none\n[E1]\nCrushSound={first}\n",
            ));
            let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
                root,
                None,
                IniFile::empty(),
                Arc::clone(&sounds),
                Arc::default(),
            )
            .unwrap();
            let loaded = owner
                .load_scenario(NativeScenarioRulesPrefix::Campaign(None), &later)
                .unwrap();
            assert_eq!(cue(&loaded.rules).as_deref(), Some(first));
            assert_eq!(
                loaded.rules.object("SAME").unwrap().crush_sound,
                None,
                "the newly allocated Infantry must not inherit the Overlay's history"
            );
            assert_eq!(
                loaded.rules.object("E1").unwrap().crush_sound.as_deref(),
                Some("<none>")
            );
            let overlays = loaded.rules.overlay_registry();
            assert_eq!(
                overlays
                    .flags(overlays.id_for_name("EMPTY").unwrap())
                    .unwrap()
                    .crush_sound
                    .as_deref(),
                Some("none")
            );
            let bytes =
                crate::util::save_codec::serialize(owner.saved_scenario_rules().unwrap()).unwrap();
            let restored = crate::util::save_codec::deserialize(&bytes).unwrap();
            owner.install_saved_scenario_rules(restored);
            let saved = owner
                .project_saved_scenario_rules(owner.saved_scenario_rules().unwrap())
                .unwrap();
            assert_eq!(cue(&saved).as_deref(), Some(first));
            assert_eq!(
                saved.simulation_config_hash(),
                loaded.rules.simulation_config_hash()
            );

            // Continue identical physical input from two different live
            // histories. The source hash covers this incoming pass; only
            // the effective configuration hash can distinguish the Voc IDs.
            let state = owner.registry.take().unwrap().into_registry_state();
            let reread = RulesLayerStack::new(later.clone())
                .process_with_fixed_art_and_registry_state_recovering(
                    &owner.sources.fixed_art,
                    state,
                    RulesProcessContext::Ordinary,
                    &owner.sources.fixed_eva,
                    &sounds,
                )
                .unwrap();
            let rules = RuleSet::from_processed_rules(&reread).unwrap();
            hashes.push((rules.source_ini_hash(), rules.simulation_config_hash()));
            owner.install_saved_scenario_rules(reread);

            owner
                .load_scenario(
                    NativeScenarioRulesPrefix::Campaign(None),
                    &IniFile::from_str("[SAME]\nCrushSound=none\n"),
                )
                .unwrap();
            let preview = owner
                .random_map_preview_projection(None, "TEMPERATE")
                .unwrap();
            assert_eq!(
                cue(&RuleSet::from_processed_rules(&preview).unwrap()).as_deref(),
                Some(first)
            );
            let reset = owner
                .load_scenario(NativeScenarioRulesPrefix::RandomMap(None), &later)
                .unwrap();
            assert_eq!(cue(&reset.rules).as_deref(), Some(first));
        }
        assert_eq!(
            hashes[0].0, hashes[1].0,
            "same final text, different retained Voc IDs"
        );
        assert_ne!(hashes[0].1, hashes[1].1);
    }

    #[test]
    fn heal_crate_binding_survives_unknown_layers_preview_and_saved_rules() {
        let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let mut retail = crate::rules::retail_ini_fixture::retail_rules_owner(&assets);
        let stock = retail
            .load_scenario(NativeScenarioRulesPrefix::Campaign(None), &IniFile::empty())
            .unwrap();
        assert!(
            stock
                .rules
                .crate_rules
                .heal_crate_sound
                .as_deref()
                .is_some_and(|sound| sound.eq_ignore_ascii_case("HealCrate"))
        );
        let rejected = retail
            .load_scenario(
                NativeScenarioRulesPrefix::Campaign(None),
                &IniFile::from_str("[CrateRules]\nHealCrateSound=Unknown\n"),
            )
            .unwrap();
        assert!(
            rejected
                .rules
                .crate_rules
                .heal_crate_sound
                .as_deref()
                .is_some_and(|sound| sound.eq_ignore_ascii_case("HealCrate")),
            "an unknown later name must retain the valid retail binding"
        );

        // Preserve the real catalog and all other retail Rules/ART. The
        // authored root's failed lookup must keep the process's prior Voc.
        let mut root = retail.sources.selected_rules_root.clone();
        root.merge(&IniFile::from_str("[CrateRules]\nHealCrateSound=Unknown\n"));
        let make_owner = || {
            NativeRulesProcessOwner::from_cold_start_sources(
                root.clone(),
                None,
                retail.sources.fixed_art.clone(),
                retail.sources.fixed_sounds.clone(),
                retail.sources.fixed_eva.clone(),
            )
            .unwrap()
        };
        let mut owner = make_owner();
        let never_bound = owner
            .load_scenario(NativeScenarioRulesPrefix::Campaign(None), &IniFile::empty())
            .unwrap();
        assert_eq!(never_bound.rules.crate_rules.heal_crate_sound, None);
        let initial = owner
            .load_scenario(
                NativeScenarioRulesPrefix::Campaign(None),
                &IniFile::from_str("[CrateRules]\nHealCrateSound=HealCrate\n"),
            )
            .unwrap();
        assert_eq!(
            initial.rules.crate_rules.heal_crate_sound.as_deref(),
            Some("HealCrate")
        );
        let mode = IniFile::from_str("[CrateRules]\nHealCrateSound=CrateMoney\n");
        owner
            .prepare_random_map_preview(Some(&mode), "TEMPERATE", true)
            .unwrap();
        let changed = IniFile::from_str("[CrateRules]\nHealCrateSound=HealCrate\n");
        owner
            .prepare_random_map_preview(Some(&changed), "TEMPERATE", false)
            .unwrap();
        assert_eq!(
            owner
                .prepared_random_map_rules()
                .unwrap()
                .crate_rules()
                .heal_crate_sound
                .as_deref(),
            Some("CrateMoney")
        );
        let prospective = owner
            .random_map_preview_projection(None, "TEMPERATE")
            .unwrap();
        assert_eq!(
            prospective.crate_rules().heal_crate_sound.as_deref(),
            Some("CrateMoney")
        );
        // Cancel followed by Full_Init retains the same scalar; the next
        // root pass still cannot resolve Unknown and must not erase it.
        let loaded = owner
            .load_scenario(NativeScenarioRulesPrefix::Campaign(None), &IniFile::empty())
            .unwrap();
        assert_eq!(
            loaded.rules.crate_rules.heal_crate_sound.as_deref(),
            Some("CrateMoney")
        );
        let hash = loaded.rules.simulation_config_hash();
        let bytes =
            crate::util::save_codec::serialize(owner.saved_scenario_rules().unwrap()).unwrap();
        owner.install_saved_scenario_rules(crate::util::save_codec::deserialize(&bytes).unwrap());
        let restored = owner
            .project_saved_scenario_rules(owner.saved_scenario_rules().unwrap())
            .unwrap();
        assert_eq!(
            restored.crate_rules.heal_crate_sound.as_deref(),
            Some("CrateMoney")
        );
        assert_eq!(restored.simulation_config_hash(), hash);
        let mut other = make_owner();
        other
            .load_scenario(NativeScenarioRulesPrefix::Campaign(None), &changed)
            .unwrap();
        let other = other
            .load_scenario(NativeScenarioRulesPrefix::Campaign(None), &IniFile::empty())
            .unwrap();
        assert_ne!(
            other.rules.simulation_config_hash(),
            hash,
            "effective bindings enter the existing hash"
        );
    }

    #[test]
    fn rmg_land_reads_obey_cache_and_preserve_untouched_rows() {
        use crate::rules::terrain_rules::{LandType, test_speed_percent};
        let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
            IniFile::from_str("[Clear]\nWheel=25%\n"),
            Some(IniFile::from_str("[Clear]\nWheel=40%\n")),
            IniFile::empty(),
            Arc::default(),
            Arc::default(),
        )
        .unwrap();
        let mode = IniFile::from_str("[Clear]\nWheel=0.5%\n[Tunnel]\nTrack=-0.5%\n");
        owner
            .prepare_random_map_preview(Some(&mode), "TEMPERATE", true)
            .unwrap();
        let first = owner
            .prepared_random_map_rules()
            .unwrap()
            .terrain_rules()
            .clone();
        assert_eq!(
            first.row(LandType::Clear).speed_costs.wheel,
            Some(test_speed_percent(0.5))
        );

        // A mode change alone is not one of InitMap's four cache keys.
        let changed = IniFile::from_str("[Clear]\nWheel=0%\n[Tunnel]\nTrack=0%\n");
        owner
            .prepare_random_map_preview(Some(&changed), "TEMPERATE", false)
            .unwrap();
        let hit = owner.prepared_random_map_rules().unwrap().terrain_rules();
        assert_eq!(hit.row(LandType::Clear), first.row(LandType::Clear));
        assert_eq!(hit.row(LandType::Tunnel), first.row(LandType::Tunnel));
        let prospective = owner
            .random_map_preview_projection(Some(&changed), "TEMPERATE")
            .unwrap();
        assert!(
            prospective
                .terrain_rules()
                .row(LandType::Clear)
                .speed_costs
                .wheel
                .unwrap()
                .is_zero()
        );
        assert_eq!(
            owner
                .prepared_random_map_rules()
                .unwrap()
                .terrain_rules()
                .row(LandType::Clear),
            first.row(LandType::Clear),
            "agreement is read-only"
        );

        // Accepted .SED always enters Full_Init: root then LANG replace Clear;
        // neither reaches Tunnel, whose prior process-global row survives.
        let loaded = owner
            .load_scenario(
                NativeScenarioRulesPrefix::RandomMap(None),
                &IniFile::empty(),
            )
            .unwrap();
        assert_eq!(
            loaded
                .rules
                .terrain_rules
                .row(LandType::Clear)
                .speed_costs
                .wheel,
            Some(test_speed_percent(40))
        );
        assert_eq!(
            loaded.rules.terrain_rules.row(LandType::Tunnel),
            first.row(LandType::Tunnel)
        );
        owner
            .prepare_random_map_preview(Some(&changed), "TEMPERATE", true)
            .unwrap();
        assert!(
            owner.saved_scenario_rules().is_none(),
            "RMG replaces the retired scenario envelope"
        );
        assert!(
            owner
                .prepared_random_map_rules()
                .unwrap()
                .terrain_rules()
                .row(LandType::Clear)
                .speed_costs
                .wheel
                .unwrap()
                .is_zero()
        );
    }

    #[test]
    fn rmg_sound_reads_survive_cancel_and_skip_cached_preview() {
        let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
            IniFile::from_str("[AudioVisual]\nBombTickingSound=Unknown\nGUIBuildSound=Unknown\nGUITabSound=Unknown\nBuildingSlam=Unknown\n"),
            None,
            IniFile::empty(),
            Arc::new(SoundRegistry::from_ini(&IniFile::from_str(
                "[SoundList]\n0=Earlier\n1=Preview\n",
            ))),
            Arc::default(),
        )
        .unwrap();
        owner
            .load_scenario(
                NativeScenarioRulesPrefix::Campaign(None),
                &IniFile::from_str("[AudioVisual]\nBombTickingSound=Earlier\nGUIBuildSound=Earlier\nGUITabSound=Earlier\nBuildingSlam=Earlier\n"),
            )
            .unwrap();
        let mode = IniFile::from_str(
            "[AudioVisual]\nBombTickingSound=Preview\nBombAttachSound=Preview\nPsychicRevealActivateSound=Preview\nGUIBuildSound=Preview\nGUITabSound=Preview\nBuildingSlam=Preview\n",
        );
        owner
            .prepare_random_map_preview(Some(&mode), "TEMPERATE", true)
            .unwrap();
        let changed = IniFile::from_str(
            "[AudioVisual]\nBombTickingSound=Earlier\nBombAttachSound=Earlier\nPsychicRevealActivateSound=Earlier\nGUIBuildSound=Earlier\nGUITabSound=Earlier\nBuildingSlam=Earlier\n",
        );
        owner
            .prepare_random_map_preview(Some(&changed), "TEMPERATE", false)
            .unwrap();
        let cached = owner.prepared_random_map_rules().unwrap();
        assert_eq!(cached.bomb_ticking_sound(), Some("Preview"));
        assert_eq!(cached.bomb_attach_sound(), Some("Preview"));
        assert_eq!(cached.psychic_reveal_activate_sound(), Some("Preview"));
        let rules = RuleSet::from_processed_rules(cached).unwrap();
        assert_eq!(rules.general.gui_build_sound.as_deref(), Some("Preview"));
        assert_eq!(rules.general.gui_tab_sound.as_deref(), Some("Preview"));
        assert_eq!(rules.general.building_slam.as_deref(), Some("Preview"));
        let prospective = owner
            .random_map_preview_projection(None, "TEMPERATE")
            .unwrap();
        assert_eq!(
            prospective.bomb_ticking_sound(),
            Some("Preview"),
            "an unknown root sound retains the live fixed-catalog binding"
        );
        // Cancel keeps process fields. The next scenario and its saved envelope
        // receive the same retained sound indices through the full owner.
        let loaded = owner
            .load_scenario(NativeScenarioRulesPrefix::Campaign(None), &IniFile::empty())
            .unwrap();
        assert_eq!(
            loaded.rules.general.bomb_ticking_sound.as_deref(),
            Some("Preview")
        );
        assert_eq!(
            loaded.rules.general.bomb_attach_sound.as_deref(),
            Some("Preview")
        );
        assert_eq!(
            loaded
                .rules
                .general
                .psychic_reveal_activate_sound
                .as_deref(),
            Some("Preview")
        );
        assert_eq!(
            loaded.rules.general.gui_build_sound.as_deref(),
            Some("Preview")
        );
        assert_eq!(
            loaded.rules.general.gui_tab_sound.as_deref(),
            Some("Preview")
        );
        assert_eq!(
            loaded.rules.general.building_slam.as_deref(),
            Some("Preview")
        );
        let saved =
            crate::util::save_codec::serialize(owner.saved_scenario_rules().unwrap()).unwrap();
        owner.install_saved_scenario_rules(crate::util::save_codec::deserialize(&saved).unwrap());
        let restored = owner
            .project_saved_scenario_rules(owner.saved_scenario_rules().unwrap())
            .unwrap();
        assert_eq!(
            restored.general.bomb_ticking_sound.as_deref(),
            Some("Preview")
        );
        assert_eq!(
            restored.general.bomb_attach_sound.as_deref(),
            Some("Preview")
        );
        assert_eq!(
            restored.general.psychic_reveal_activate_sound.as_deref(),
            Some("Preview")
        );
        assert_eq!(restored.general.gui_build_sound.as_deref(), Some("Preview"));
        assert_eq!(restored.general.gui_tab_sound.as_deref(), Some("Preview"));
        assert_eq!(restored.general.building_slam.as_deref(), Some("Preview"));
    }

    #[test]
    fn mind_control_sounds_retain_fixed_bindings_through_reset_and_save() {
        let bad = IniFile::from_str(
            "[AudioVisual]\nYuriMindControlSound=Unknown\nMindClearedSound=Unknown\n\
             MasterMindOverloadDeathSound=Unknown\n",
        );
        let sounds = Arc::new(SoundRegistry::from_ini(&IniFile::from_str(
            "[SoundList]\n0=Capture\n1=Release\n2=Overload\n3=none\n4=<none>\n",
        )));
        let new_owner = || {
            NativeRulesProcessOwner::from_cold_start_sources(
                bad.clone(),
                None,
                IniFile::empty(),
                Arc::clone(&sounds),
                Arc::default(),
            )
            .unwrap()
        };
        let cues = |rules: &RuleSet| {
            [
                rules.mind_control.mind_control_sound.clone(),
                rules.mind_control.mind_cleared_sound.clone(),
                rules.mind_control.overload_sound.clone(),
            ]
        };
        let mut owner = new_owner();
        let first = owner
            .load_scenario(NativeScenarioRulesPrefix::Campaign(None), &bad)
            .unwrap();
        // Yuri's uninitialized native field gets deterministic silence until
        // a successful binding; the other two native fields start at-1.
        assert_eq!(cues(&first.rules), [None, None, None]);
        let early = IniFile::from_str(
            "[AudioVisual]\nYuriMindControlSound=capture\nMindClearedSound=release\n\
             MasterMindOverloadDeathSound=overload\n",
        );
        let loaded = owner
            .load_scenario(NativeScenarioRulesPrefix::Campaign(Some(&early)), &bad)
            .unwrap();
        let expected = [
            Some("Capture".into()),
            Some("Release".into()),
            Some("Overload".into()),
        ];
        assert_eq!(cues(&loaded.rules), expected);
        let fresh = new_owner()
            .load_scenario(NativeScenarioRulesPrefix::Campaign(None), &bad)
            .unwrap();
        assert_eq!(
            loaded.rules.source_ini_hash(),
            fresh.rules.source_ini_hash()
        );
        assert_ne!(
            loaded.rules.simulation_config_hash(),
            fresh.rules.simulation_config_hash()
        );
        let bytes =
            crate::util::save_codec::serialize(owner.saved_scenario_rules().unwrap()).unwrap();
        owner.install_saved_scenario_rules(crate::util::save_codec::deserialize(&bytes).unwrap());
        let restored = owner
            .project_saved_scenario_rules(owner.saved_scenario_rules().unwrap())
            .unwrap();
        assert_eq!(cues(&restored), expected);
        let none = IniFile::from_str(
            "[AudioVisual]\nYuriMindControlSound=none\nMindClearedSound=<none>\n\
             MasterMindOverloadDeathSound=\n",
        );
        let loaded = owner
            .load_scenario(NativeScenarioRulesPrefix::Campaign(None), &none)
            .unwrap();
        assert_eq!(
            cues(&loaded.rules),
            [
                Some("none".into()),
                Some("<none>".into()),
                Some("Overload".into())
            ]
        );
    }

    #[test]
    fn mind_cleared_type_sound_keeps_a_valid_override_and_rejects_unknown_first_read() {
        let sounds = Arc::new(SoundRegistry::from_ini(&IniFile::from_str(
            "[SoundList]\n0=Release\n1=TypeRelease\n2=none\n",
        )));
        let build = |first: &str| {
            let root = IniFile::from_str(&format!(
                "[AudioVisual]\nMindClearedSound=Release\n[InfantryTypes]\n0=E1\n\
                 [E1]\nMindClearedSound={first}\n",
            ));
            let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
                root,
                None,
                IniFile::empty(),
                Arc::clone(&sounds),
                Arc::default(),
            )
            .unwrap();
            let loaded = owner
                .load_scenario(
                    NativeScenarioRulesPrefix::Campaign(None),
                    &IniFile::from_str("[E1]\nMindClearedSound=Unknown\n"),
                )
                .unwrap();
            (owner, loaded.rules)
        };
        let (mut owner, retained) = build("TypeRelease");
        assert_eq!(
            retained.object("E1").unwrap().mind_cleared_sound.as_deref(),
            Some("TypeRelease")
        );
        let bytes =
            crate::util::save_codec::serialize(owner.saved_scenario_rules().unwrap()).unwrap();
        owner.install_saved_scenario_rules(crate::util::save_codec::deserialize(&bytes).unwrap());
        let restored = owner
            .project_saved_scenario_rules(owner.saved_scenario_rules().unwrap())
            .unwrap();
        assert_eq!(
            restored.object("E1").unwrap().mind_cleared_sound.as_deref(),
            Some("TypeRelease")
        );
        let (_, unknown) = build("Unknown");
        assert_eq!(unknown.object("E1").unwrap().mind_cleared_sound, None);
        assert_eq!(
            unknown.mind_control.mind_cleared_sound.as_deref(),
            Some("Release")
        );
        let (_, registered_none) = build("none");
        assert_eq!(
            registered_none
                .object("E1")
                .unwrap()
                .mind_cleared_sound
                .as_deref(),
            Some("none")
        );
    }

    #[test]
    fn same_named_technos_keep_their_own_sound_read_history() {
        use crate::rules::object_type::ObjectCategory;

        let root = IniFile::from_str(
            "[AudioVisual]\nMindClearedSound=Release\n[BuildingTypes]\n0=SAME\n\
             [SAME]\nMindClearedSound=TypeRelease\nCreateSound=TypeRelease\nImmuneToPsionics=no\n",
        );
        let sounds = Arc::new(SoundRegistry::from_ini(&IniFile::from_str(
            "[SoundList]\n0=Release\n1=TypeRelease\n",
        )));
        let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
            root,
            None,
            IniFile::empty(),
            sounds,
            Arc::default(),
        )
        .unwrap();
        let map = IniFile::from_str(
            "[InfantryTypes]\n0=SAME\n[SAME]\nMindClearedSound=Unknown\nCreateSound=Unknown\n",
        );
        let loaded = owner
            .load_scenario(NativeScenarioRulesPrefix::Campaign(None), &map)
            .unwrap();
        let fields = |rules: &RuleSet, category| {
            let object = rules.object_in_category(category, "SAME").unwrap();
            (
                object.mind_cleared_sound.clone(),
                object.create_sound().map(str::to_owned),
            )
        };
        // TechnoRead713949 defaults to each object's own +5B0, not another
        // class with the same ID. Only the Building existed for the first read.
        let expected = (Some("TypeRelease".into()), Some("TypeRelease".into()));
        assert_eq!(fields(&loaded.rules, ObjectCategory::Building), expected);
        assert_eq!(
            fields(&loaded.rules, ObjectCategory::Infantry),
            (None, None)
        );
        let bytes =
            crate::util::save_codec::serialize(owner.saved_scenario_rules().unwrap()).unwrap();
        owner.install_saved_scenario_rules(crate::util::save_codec::deserialize(&bytes).unwrap());
        let restored = owner
            .project_saved_scenario_rules(owner.saved_scenario_rules().unwrap())
            .unwrap();
        assert_eq!(fields(&restored, ObjectCategory::Building), expected);
        assert_eq!(fields(&restored, ObjectCategory::Infantry), (None, None));
    }

    #[test]
    fn global_sound_bindings_survive_invalid_layers_reset_and_save() {
        let root = IniFile::from_str(
            "[AudioVisual]\nBombTickingSound=Unknown\nBombAttachSound=Unknown\n\
             PsychicRevealActivateSound=Unknown\nGUIBuildSound=Unknown\nGUITabSound=Unknown\nBuildingSlam=Unknown\n",
        );
        let sounds = Arc::new(SoundRegistry::from_ini(&IniFile::from_str(
            "[SoundList]\n0=Tick\n1=Attach\n2=Reveal\n3=none\n4=<none>\n",
        )));
        let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
            root.clone(),
            None,
            IniFile::empty(),
            Arc::clone(&sounds),
            Arc::default(),
        )
        .unwrap();
        let early = IniFile::from_str(
            "[AudioVisual]\nBombTickingSound=tIcK\nBombAttachSound=Attach\n\
             PsychicRevealActivateSound=Reveal\nGUIBuildSound=tIcK\nGUITabSound=Attach\nBuildingSlam=Reveal\n",
        );
        let cues = |rules: &RuleSet| {
            [
                rules.general.bomb_ticking_sound.clone(),
                rules.general.bomb_attach_sound.clone(),
                rules.general.psychic_reveal_activate_sound.clone(),
                rules.general.gui_build_sound.clone(),
                rules.general.gui_tab_sound.clone(),
                rules.general.building_slam.clone(),
            ]
        };
        let expected = [
            Some("Tick".into()),
            Some("Attach".into()),
            Some("Reveal".into()),
            Some("Tick".into()),
            Some("Attach".into()),
            Some("Reveal".into()),
        ];
        // Campaign's named INI precedes TypeReset; the root and map then
        // name unknown cues. All cues must keep the pre-reset fixed IDs.
        let loaded = owner
            .load_scenario(NativeScenarioRulesPrefix::Campaign(Some(&early)), &root)
            .unwrap();
        assert_eq!(cues(&loaded.rules), expected);
        let bytes =
            crate::util::save_codec::serialize(owner.saved_scenario_rules().unwrap()).unwrap();
        let restored = crate::util::save_codec::deserialize(&bytes).unwrap();
        owner.install_saved_scenario_rules(restored);
        let restored_rules = owner
            .project_saved_scenario_rules(owner.saved_scenario_rules().unwrap())
            .unwrap();
        assert_eq!(cues(&restored_rules), expected);

        let later = IniFile::from_str(
            "[AudioVisual]\nBombTickingSound=\nBombAttachSound=none\nGUIBuildSound=none\nGUITabSound=<none>\n",
        );
        let loaded = owner
            .load_scenario(NativeScenarioRulesPrefix::Campaign(None), &later)
            .unwrap();
        assert_eq!(
            cues(&loaded.rules),
            [
                Some("Tick".into()),
                Some("none".into()),
                Some("Reveal".into()),
                Some("none".into()),
                Some("<none>".into()),
                Some("Reveal".into())
            ]
        );
        let mut fresh = NativeRulesProcessOwner::from_cold_start_sources(
            root.clone(),
            None,
            IniFile::empty(),
            Arc::clone(&sounds),
            Arc::default(),
        )
        .unwrap();
        let fresh = fresh
            .load_scenario(NativeScenarioRulesPrefix::Campaign(None), &later)
            .unwrap();
        assert_eq!(
            cues(&fresh.rules),
            [
                None,
                Some("none".into()),
                None,
                Some("none".into()),
                Some("<none>".into()),
                None
            ]
        );
        assert_eq!(
            loaded.rules.source_ini_hash(),
            fresh.rules.source_ini_hash()
        );
        assert_ne!(
            loaded.rules.simulation_config_hash(),
            fresh.rules.simulation_config_hash()
        );

        // Equalize the original three cues to isolate the new UI bindings in
        // the effective hash. Only BuildingSlam differs after this same input.
        let only_bombs = IniFile::from_str(
            "[AudioVisual]\nBombTickingSound=Tick\nBombAttachSound=Attach\nPsychicRevealActivateSound=Reveal\n",
        );
        let mut ui_fresh = NativeRulesProcessOwner::from_cold_start_sources(
            root,
            None,
            IniFile::empty(),
            sounds,
            Arc::default(),
        )
        .unwrap();
        let ui_fresh = ui_fresh
            .load_scenario(
                NativeScenarioRulesPrefix::Campaign(Some(&only_bombs)),
                &later,
            )
            .unwrap();
        assert_eq!(cues(&loaded.rules)[..5], cues(&ui_fresh.rules)[..5]);
        assert_eq!(
            loaded.rules.source_ini_hash(),
            ui_fresh.rules.source_ini_hash()
        );
        assert_ne!(
            loaded.rules.simulation_config_hash(),
            ui_fresh.rules.simulation_config_hash()
        );
    }

    #[test]
    fn rmg_preview_retains_powerup_indices_for_the_next_full_init() {
        use crate::rules::powerups::POWERUP_MONEY;

        let root = IniFile::from_str("[Animations]\n0=FIRST\n1=SECOND\n");
        let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
            root.clone(),
            None,
            IniFile::empty(),
            Arc::default(),
            Arc::default(),
        )
        .unwrap();
        owner
            .load_scenario(
                NativeScenarioRulesPrefix::NonCampaign(None),
                &IniFile::from_str("[Animations]\n0=OLD\n[Powerups]\nMoney=9,OLD,yes,321\n"),
            )
            .unwrap();
        let mode = IniFile::from_str("[Animations]\n0=PREVIEW\n");
        owner
            .prepare_random_map_preview(Some(&mode), "TEMPERATE", true)
            .unwrap();
        let preview = owner.prepared_random_map_rules().unwrap();
        let rules = RuleSet::from_processed_rules(preview).unwrap();
        assert_eq!(rules.powerups.animation_index(POWERUP_MONEY), 2);
        assert_eq!(rules.anim_type_name_by_index(2), Some("PREVIEW"));
        assert_eq!(rules.powerups.weights[POWERUP_MONEY], 9);
        assert!(rules.powerups.over_water[POWERUP_MONEY]);
        assert_eq!(
            rules.powerups.magnitudes[POWERUP_MONEY].bits(),
            321_f64.to_bits()
        );
        assert!(owner.saved_scenario_rules().is_none());

        // Closing the preview does not reset Rules. The next real Full_Init
        // retains the raw index, now naming its own rebuilt AnimType array.
        let incoming = IniFile::from_str("[Animations]\n0=NEXT\n");
        let loaded = owner
            .load_scenario(NativeScenarioRulesPrefix::NonCampaign(None), &incoming)
            .unwrap();
        assert_eq!(loaded.rules.powerups, rules.powerups);
        assert_eq!(loaded.rules.anim_type_name_by_index(2), Some("NEXT"));
        let mut fresh = NativeRulesProcessOwner::from_cold_start_sources(
            root,
            None,
            IniFile::empty(),
            Arc::default(),
            Arc::default(),
        )
        .unwrap();
        let direct = fresh
            .load_scenario(NativeScenarioRulesPrefix::NonCampaign(None), &incoming)
            .unwrap();
        assert_eq!(
            loaded.receipt.pre_reset().event_count(),
            direct.receipt.pre_reset().event_count()
        );
        assert_eq!(
            loaded.receipt.post_reset().event_count(),
            direct.receipt.post_reset().event_count()
        );
    }

    #[test]
    fn rmg_cache_hit_keeps_the_processed_registry_and_retires_only_old_ids() {
        use crate::rules::powerups::POWERUP_MONEY;

        let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
            IniFile::from_str("[Animations]\n0=BASE\n"),
            None,
            IniFile::empty(),
            Arc::default(),
            Arc::default(),
        )
        .unwrap();
        let mode = IniFile::from_str("[Animations]\n0=KEPT\n[Powerups]\nMoney=7,KEPT,yes,125\n");
        owner
            .prepare_random_map_preview(Some(&mode), "TEMPERATE", true)
            .unwrap();
        let first = owner.prepared_random_map_rules().unwrap();
        assert!(first.native_type_construction_trace().event_count() > 0);
        let first_rules = RuleSet::from_processed_rules(first).unwrap();
        let inherited = owner
            .random_map_preview_projection(None, "TEMPERATE")
            .unwrap();
        assert_eq!(inherited.powerups(), &first_rules.powerups);
        let changed =
            IniFile::from_str("[Animations]\n0=IGNORED\n[Powerups]\nMoney=2,IGNORED,no,50\n");
        let prospective = owner
            .random_map_preview_projection(Some(&changed), "TEMPERATE")
            .unwrap();
        assert_eq!(prospective.powerups().weights[POWERUP_MONEY], 2);
        owner
            .prepare_random_map_preview(Some(&changed), "TEMPERATE", false)
            .unwrap();
        let hit = owner.prepared_random_map_rules().unwrap();
        assert_eq!(hit.content_hash(), first_rules.source_ini_hash());
        assert_eq!(hit.native_type_construction_trace().event_count(), 0);
        let hit_rules = RuleSet::from_processed_rules(hit).unwrap();
        assert_eq!(hit_rules.powerups, first_rules.powerups);
        assert_eq!(hit_rules.anim_type_name_by_index(1), Some("KEPT"));
        assert!(
            !hit_rules
                .anim_type_art_read_states()
                .iter()
                .any(|(id, _)| id == "IGNORED")
        );

        let invalid_mode = IniFile::from_str("[Tiberiums]\n-1=INVALID\n");
        assert!(
            owner
                .prepare_random_map_preview(Some(&invalid_mode), "TEMPERATE", true)
                .is_err()
        );
        assert!(owner.prepared_random_map_rules().is_none());
        assert_eq!(
            owner
                .registry
                .as_ref()
                .unwrap()
                .registry_state()
                .anim_type_art_read_states()
                .map(|(name, _)| name)
                .collect::<Vec<_>>(),
            ["BASE"],
            "failed mode retains the reset's reached root registry, not the old KEPT array"
        );
        let after_error = owner
            .random_map_preview_projection(None, "TEMPERATE")
            .unwrap();
        assert_eq!(after_error.powerups(), &first_rules.powerups);
    }

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

    #[test]
    fn sparky_on_fire_preserves_factory_slots_through_reread_save_and_reset() {
        let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let mut owner = crate::rules::retail_ini_fixture::retail_rules_owner(&assets);
        let retail = owner
            .load_scenario(NativeScenarioRulesPrefix::Campaign(None), &IniFile::empty())
            .unwrap()
            .into_parts()
            .0;
        assert!(retail.warheads_iter().all(|warhead| !warhead.sparky));
        assert!(retail.general.on_fire.is_empty());

        // Continue the same live Rules object through actual Process passes.
        let reread = |owner: &mut NativeRulesProcessOwner, input: &str| {
            let state = owner.registry.take().unwrap().into_registry_state();
            let processed = RulesLayerStack::new(IniFile::from_str(input))
                .process_with_fixed_art_and_registry_state_recovering(
                    &owner.sources.fixed_art,
                    state,
                    RulesProcessContext::Ordinary,
                    &owner.sources.fixed_eva,
                    &owner.sources.fixed_sounds,
                )
                .unwrap();
            let rules = owner.project_saved_scenario_rules(&processed).unwrap();
            owner.install_saved_scenario_rules(processed);
            rules
        };
        let first = reread(
            &mut owner,
            "[Fire]\nSparky=yes\n[AudioVisual]\nOnFire=ONFIRE_FIRST\n",
        );
        assert!(first.warhead("Fire").unwrap().sparky);
        assert_eq!(first.general.on_fire, ["ONFIRE_FIRST"]);

        // Repeated >24-byte names allocate distinct slots. Comma tokens keep
        // leading spaces, so ` none` is a type while exact `none` is null.
        let replaced = reread(
            &mut owner,
            "[Fire]\nSparky=invalid\n[AudioVisual]\n\
             OnFire=ABCDEFGHIJKLMNOPQRSTUVWXA,ABCDEFGHIJKLMNOPQRSTUVWXA, none,none,ONFIRE_FIRST,ONFIRE_FIRST\n",
        );
        assert!(replaced.warhead("Fire").unwrap().sparky);
        let names = &replaced.general.on_fire;
        assert_eq!(names.len(), 5);
        assert_eq!(names[0], "ABCDEFGHIJKLMNOPQRSTUVWX");
        assert_ne!(
            names[0], names[1],
            "equal stored IDs keep distinct pointers"
        );
        assert_eq!(&names[2..], [" none", "ONFIRE_FIRST", "ONFIRE_FIRST"]);
        let roots = crate::rules::effect_asset_catalog::anim_class_roots(&replaced);
        for name in names {
            assert!(roots.contains(&name.to_ascii_uppercase()));
        }
        let retained = reread(&mut owner, "[Fire]\nSparky=\n[AudioVisual]\nOnFire=\n");
        assert!(retained.warhead("Fire").unwrap().sparky);
        assert_eq!(retained.general.on_fire, *names);
        // The same pass first sweeps the late AV-allocated Anim bodies;
        // retaining this list does not retain the complete prior Rules hash.

        let bytes =
            crate::util::save_codec::serialize(owner.saved_scenario_rules().unwrap()).unwrap();
        let cleared = reread(&mut owner, "[AudioVisual]\nOnFire=none,<none>\n");
        assert!(cleared.general.on_fire.is_empty());
        assert_ne!(
            cleared.simulation_config_hash(),
            retained.simulation_config_hash()
        );
        let no_sparks = reread(&mut owner, "[Fire]\nSparky=no\n");
        assert!(!no_sparks.warhead("Fire").unwrap().sparky);
        assert_ne!(
            no_sparks.simulation_config_hash(),
            cleared.simulation_config_hash()
        );

        owner.install_saved_scenario_rules(crate::util::save_codec::deserialize(&bytes).unwrap());
        let restored = owner
            .project_saved_scenario_rules(owner.saved_scenario_rules().unwrap())
            .unwrap();
        assert!(restored.warhead("Fire").unwrap().sparky);
        assert_eq!(restored.general.on_fire, *names);
        assert_eq!(
            restored.simulation_config_hash(),
            retained.simulation_config_hash()
        );
        let reset = owner
            .load_scenario(
                NativeScenarioRulesPrefix::NonCampaign(None),
                &IniFile::empty(),
            )
            .unwrap()
            .into_parts()
            .0;
        assert!(!reset.warhead("Fire").unwrap().sparky);
        assert!(reset.general.on_fire.is_empty());
    }
}
