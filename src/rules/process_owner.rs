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

use std::sync::Arc;

use crate::rules::error::RulesError;
use crate::rules::ini_parser::IniFile;
use crate::rules::native_processing::{
    NativeRulesRegistryState, NativeTypeConstructionEvent, NativeTypeConstructionTrace,
    ProcessedRulesLayers, RulesLayerKind, RulesLayerStack, process_native_campaign_rules_prepass,
    process_native_noncampaign_rules_prepass, process_native_rules_cold_start,
};
use crate::rules::ruleset::RuleSet;
use crate::rules::sound_ini::SoundRegistry;

/// The startup-selected INI objects reused by every later native Process call.
///
/// This is intentionally move-only. Re-reading loose files during a scenario
/// would introduce a mutation window that native's already-selected INI
/// objects do not have.
#[derive(Debug)]
struct NativeRulesSourceSnapshot {
    selected_rules_root: IniFile,
    langrule: Option<IniFile>,
    fixed_art: IniFile,
    /// ARTMD `[Movies]`, selected once by Load_Game_Rules52D121. Movie
    /// references in campaigns and Basic share these indices across reloads.
    movies: crate::rules::movies::MovieRegistry,
    /// Immutable Voc catalog selected once by Init_Game52C763..52C796.
    /// It is not a Rules layer and survives scenario Rules reconstruction.
    fixed_sounds: Arc<SoundRegistry>,
}

#[derive(Debug)]
enum NativeRulesRegistryOwner {
    /// Cold constructors predate the next numeric-ID reset. Their events must
    /// be drained, while their registry identities feed the E prepass.
    ColdStartup(NativeTypeConstructionTrace),
    /// Later P/P_preview events have transferred to their numeric-ID consumer;
    /// only the continuing registry identity remains process-resident.
    Live(NativeRulesRegistryState),
}

/// The one move-only native Rules authority for a process.
#[derive(Debug)]
pub(crate) struct NativeRulesProcessOwner {
    sources: NativeRulesSourceSnapshot,
    /// `None` exists only while an in-place transition is executing. Every
    /// ordinary return, including a failed Process/RuleSet parse, restores it.
    registry: Option<NativeRulesRegistryOwner>,
}

/// One constructor phase transferred to the independent native-ID cursor.
#[derive(Debug)]
pub(crate) struct NativeRulesPhaseReceipt {
    events: Vec<NativeTypeConstructionEvent>,
    allocated_super_weapon_type_count: usize,
}

impl NativeRulesPhaseReceipt {
    #[cfg(test)]
    pub(crate) fn events(&self) -> &[NativeTypeConstructionEvent] {
        &self.events
    }

    #[cfg(test)]
    pub(crate) fn event_count(&self) -> usize {
        self.events.len()
    }

    #[cfg(test)]
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
pub(crate) struct NativeScenarioRulesReceipt {
    pre_reset: NativeRulesPhaseReceipt,
    post_reset: NativeRulesPhaseReceipt,
}

impl NativeScenarioRulesReceipt {
    #[cfg(test)]
    pub(crate) fn pre_reset(&self) -> &NativeRulesPhaseReceipt {
        &self.pre_reset
    }

    #[cfg(test)]
    pub(crate) fn post_reset(&self) -> &NativeRulesPhaseReceipt {
        &self.post_reset
    }

    pub(crate) fn into_parts(self) -> (NativeRulesPhaseReceipt, NativeRulesPhaseReceipt) {
        (self.pre_reset, self.post_reset)
    }
}

/// Native Process output plus the compatibility products used by current Rust
/// readers. Registry ownership has already returned to the process owner.
pub(crate) struct NativeScenarioRulesLoad {
    rules: RuleSet,
    processed_ini: IniFile,
    fixed_art_ini: IniFile,
    receipt: NativeScenarioRulesReceipt,
}

/// The mutually exclusive native pre-reset family and its optional source.
/// Campaigns use a basename INI and never a multiplayer mode override.
pub(crate) enum NativeScenarioRulesPrefix<'a> {
    NonCampaign(Option<&'a IniFile>),
    Campaign(Option<&'a IniFile>),
}

impl NativeScenarioRulesLoad {
    pub(crate) fn into_parts(self) -> (RuleSet, IniFile, IniFile, NativeScenarioRulesReceipt) {
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
    pub(crate) fn from_cold_start_sources(
        selected_rules_root: IniFile,
        langrule: Option<IniFile>,
        fixed_art: IniFile,
        fixed_sounds: Arc<SoundRegistry>,
    ) -> Result<Self, RulesError> {
        let movies = crate::rules::movies::MovieRegistry::from_art(&fixed_art);
        let cold_trace = process_native_rules_cold_start(
            NativeRulesRegistryState::default(),
            &selected_rules_root,
            &fixed_art,
            langrule.as_ref(),
        )?;
        Ok(Self {
            sources: NativeRulesSourceSnapshot {
                selected_rules_root,
                langrule,
                fixed_art,
                movies,
                fixed_sounds,
            },
            registry: Some(NativeRulesRegistryOwner::ColdStartup(cold_trace)),
        })
    }

    /// The same immutable catalog used by process playback; never replaced by
    /// Rules reconstruction or asset-manager recovery.
    #[cfg(test)]
    pub(crate) fn fixed_sounds(&self) -> &Arc<SoundRegistry> {
        &self.sources.fixed_sounds
    }

    pub(crate) fn bind_type_sound_references(
        &self,
        rules: &mut RuleSet,
        processed: &ProcessedRulesLayers,
    ) {
        rules.bind_type_sound_references(processed.ini(), &self.sources.fixed_sounds);
    }

    /// Immutable startup-selected root for consumers with a root-only native read.
    pub(crate) fn selected_rules_root(&self) -> &IniFile {
        &self.sources.selected_rules_root
    }

    /// Fixed ART snapshot, unaffected by mode/map Rules passes.
    pub(crate) fn fixed_art(&self) -> &IniFile {
        &self.sources.fixed_art
    }

    pub(crate) fn movies(&self) -> &crate::rules::movies::MovieRegistry {
        &self.sources.movies
    }

    /// Parsed-cache hashes of the actual frozen process sources. These are
    /// not byte SHA256 values and do not reopen files after source selection.
    pub(crate) fn selected_source_ini_hashes(&self) -> [(&'static str, Option<u64>); 3] {
        [
            (
                "RULESMD.INI",
                Some(self.sources.selected_rules_root.content_hash()),
            ),
            (
                "LANGRULE.INI",
                self.sources.langrule.as_ref().map(IniFile::content_hash),
            ),
            ("ARTMD.INI", Some(self.sources.fixed_art.content_hash())),
        ]
    }

    /// Build the shell-facing compatibility projection without changing the
    /// one native registry owner.
    pub(crate) fn startup_compatibility_projection(
        &self,
    ) -> Result<ProcessedRulesLayers, RulesError> {
        self.startup_layers()
            .process_with_fixed_art(&self.sources.fixed_art)
    }

    /// Execute the active Full_Init Rules chronology in place.
    ///
    /// Noncampaign runs `E_multi` against the process-retained registry;
    /// campaign runs optional scenario-basename INI P against that registry.
    /// The common destructive reset then precedes root/LANG/(noncampaign
    /// mode)/map P.
    /// Every error restores the actual partial post-reset registry; no path
    /// rolls back to cold/pre-reset state.
    pub(crate) fn load_scenario(
        &mut self,
        prefix: NativeScenarioRulesPrefix<'_>,
        map_rules_overrides: &IniFile,
    ) -> Result<NativeScenarioRulesLoad, RulesError> {
        let registry_owner = self
            .registry
            .take()
            .expect("native Rules registry transition may not be re-entered");
        let pre_reset_state = match registry_owner {
            NativeRulesRegistryOwner::ColdStartup(trace) => {
                trace.into_registry_state_discarding_events()
            }
            NativeRulesRegistryOwner::Live(state) => state,
        };

        let (pre_reset_trace, mode_rules_override) = match prefix {
            NativeScenarioRulesPrefix::NonCampaign(mode) => (
                process_native_noncampaign_rules_prepass(
                    pre_reset_state,
                    &self.sources.selected_rules_root,
                ),
                mode,
            ),
            NativeScenarioRulesPrefix::Campaign(named) => {
                let trace = match process_native_campaign_rules_prepass(
                    pre_reset_state,
                    named,
                    &self.sources.fixed_art,
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
        let post_reset_state = pre_reset_state.destructive_reset();

        let layers = self.scenario_layers(mode_rules_override, map_rules_overrides);
        let processed = match layers.process_with_fixed_art_and_registry_state_recovering(
            &self.sources.fixed_art,
            post_reset_state,
        ) {
            Ok(processed) => processed,
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
        self.bind_type_sound_references(&mut rules, &processed);
        debug_assert_eq!(rules.source_ini_hash(), processed.content_hash());
        let (processed_ini, post_reset_trace) =
            processed.into_ini_and_native_type_construction_trace();
        let (post_reset_events, post_reset_super_count, post_reset_state) =
            post_reset_trace.into_parts();
        self.registry = Some(NativeRulesRegistryOwner::Live(post_reset_state));

        Ok(NativeScenarioRulesLoad {
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
        })
    }

    fn startup_layers(&self) -> RulesLayerStack {
        let mut layers = RulesLayerStack::new(self.sources.selected_rules_root.clone());
        if let Some(langrule) = self.sources.langrule.as_ref() {
            layers.push(RulesLayerKind::LangRule, langrule.clone());
        }
        layers
    }

    fn scenario_layers(
        &self,
        mode_rules_override: Option<&IniFile>,
        map_rules_overrides: &IniFile,
    ) -> RulesLayerStack {
        let mut layers = self.startup_layers();
        if let Some(mode) = mode_rules_override {
            layers.push(RulesLayerKind::GameMode, mode.clone());
        }
        layers.push(RulesLayerKind::Scenario, map_rules_overrides.clone());
        layers
    }

    #[cfg(test)]
    fn live_registry(&self) -> &NativeRulesRegistryState {
        match self.registry.as_ref().expect("registry is resident") {
            NativeRulesRegistryOwner::ColdStartup(trace) => trace.registry_state(),
            NativeRulesRegistryOwner::Live(state) => state,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::NativeRulesProcessOwner;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::native_processing::NativeTypeConstructorFamily;

    fn ini(text: &str) -> IniFile {
        IniFile::from_bytes(text.as_bytes()).expect("valid synthetic INI")
    }

    #[test]
    fn second_noncampaign_scenario_continues_live_registry_then_resets_it() {
        let root = ini("[Animations]\n0=ROOTANIM\n\
             [BuildingTypes]\n0=ROOTBLDG\n\
             [Countries]\n0=ROOTCOUNTRY\n\
             [General]\nParaDrop.Types=ROOTUNIT\n\
             [ROOTCOUNTRY]\nVeteranUnits=ROOTINF\n");
        let art = ini("[ROOTANIM]\nImage=ROOTANIM\n[ROOTBLDG]\nImage=ROOTBLDG\n");
        let first_map = ini("[BuildingTypes]\n1=MAPONLY\n[MAPONLY]\nImage=MAPONLY\n");
        let second_map = ini("");
        let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
            root,
            None,
            art,
            std::sync::Arc::default(),
        )
        .unwrap();

        let first = owner
            .load_scenario(
                crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(None),
                &first_map,
            )
            .expect("first scenario");
        assert!(first.receipt.pre_reset.event_count() > 0);
        assert!(first.receipt.post_reset.events().iter().any(|event| {
            event.family() == NativeTypeConstructorFamily::BuildingType
                && event.native_stored_id() == "MAPONLY"
        }));
        assert_eq!(
            owner
                .live_registry()
                .family_len(NativeTypeConstructorFamily::BuildingType),
            2
        );

        let second = owner
            .load_scenario(
                crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(None),
                &second_map,
            )
            .expect("second scenario");
        assert_eq!(
            second.receipt.pre_reset.event_count(),
            0,
            "the second prepass looks up names in scenario-one's live registry"
        );
        assert_eq!(
            owner
                .live_registry()
                .family_len(NativeTypeConstructorFamily::BuildingType),
            1,
            "the destructive reset removes the prior map-only type"
        );
    }

    #[test]
    fn failed_post_reset_process_keeps_its_partial_live_registry() {
        let root = ini("[BuildingTypes]\n0=ROOTBLDG\n");
        let failing_map = ini("[BuildingTypes]\n1=PARTIAL\n\
             [Tiberiums]\n-1=INVALID\n");
        let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
            root,
            None,
            ini(""),
            std::sync::Arc::default(),
        )
        .unwrap();

        assert!(
            owner
                .load_scenario(
                    crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(None),
                    &failing_map
                )
                .is_err(),
            "negative Tiberium slot fails after explicit families"
        );
        assert_eq!(
            owner
                .live_registry()
                .family_len(NativeTypeConstructorFamily::BuildingType),
            2,
            "ROOTBLDG and PARTIAL both remain in the post-reset native registry"
        );
    }
}
