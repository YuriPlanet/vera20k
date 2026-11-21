//! Active-YR RulesClass processing: ordered passes, live Type registries,
//! constructor chronology and typed-reader compatibility projections.
//!
//! Raw INI storage stays with ini_parser; process_owner retains registry lifetime;
//! ruleset converts this processed output into gameplay definitions. Failed passes
//! preserve already-constructed registry state rather than rolling back.

use crate::rules::color_add::ColorAddTable;
use crate::rules::error::RulesError;
#[cfg(test)]
use crate::rules::ini_parser::IniSection;
use crate::rules::ini_parser::{IniFile, is_native_none_type_name};
use crate::rules::sound_ini::{EvaRegistry, SoundRegistry};

use crate::rules::object_image::ImageTheater;
#[cfg(test)]
use crate::rules::object_type::ObjectCategory;
#[cfg(test)]
use crate::rules::prerequisite::Prerequisite;
#[cfg(test)]
use crate::rules::prerequisite::PrerequisiteGroup;
#[cfg(test)]
use crate::rules::projectile_type::ProjectileArtState;
use crate::rules::ruleset::RuleSet;
#[cfg(test)]
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use super::{threat_coefficients, voxel_frame_catalog, voxel_model_plan};

mod ares_house_state;
mod ares_rules_ext;
mod general_projections;
mod pass_families;
mod pass_houses;
mod pass_processor;
mod pass_references;
mod pass_techno;
mod processed_layers;
mod processed_type;
mod registry_state;
mod type_projections;

#[cfg(test)]
use ares_house_state::AresCountryState;
use ares_house_state::{
    AresSideState, SideRegisteredTypes, default_curtain_modifiers, read_ares_type_ref,
    read_valueable_int_vector, read_valueable_type, read_valueable_type_vector,
};
pub(crate) use ares_house_state::{
    CountryProjection, CountryVeterancyState, SideProjection, read_nullable_type_vector,
};
pub(crate) use ares_rules_ext::{AresGeneralRules, AresGlobalControls};
use pass_processor::RulesPassProcessor;
pub(crate) use processed_type::{
    AnimTypeReferences, AresExplicitFactory, AresFactoryOwners, TechnoTypeReferences,
    UrbanTypeKind, VoxelAnimTypeReferences, WeaponSlots, WeaponState,
};
use processed_type::{
    EXPLICIT_RULE_TYPE_FAMILIES, PROJECTED_RULE_TYPE_FAMILIES, ProcessedType, RulesTypeFamily,
    SelectAnimRulesState, find_existing_type, find_or_construct_type, read_member,
};
pub(crate) use registry_state::{
    NativeRulesRegistryState, NativeTypeConstructionEvent, NativeTypeConstructionTrace,
    NativeTypeConstructorFamily,
};

/// AbstractType constructor410800's stored byte ID. The INI store represents
/// native bytes as characters, so this cut is shared by Type and Campaign
/// registries without UTF-8 byte-boundary differences.
pub(crate) fn abstract_type_stored_id(incoming: &str) -> String {
    incoming.chars().take(0x18).collect()
}

/// Runtime identity for one retained AnimType array slot. AbstractType410800
/// stores24 bytes, but AnimFind428B80 compares the full input, so two objects
/// can have the same stored name and different ART/read state. Name lookups
/// retain the first slot; pointer/index consumers keep the selected object.
/// The NUL namespace cannot collide with a native C-string ID and is never
/// used as a filename: the slot's ObjectImageState owns resource requests.
///
/// Yields one key per slot of `names`, in registry order, in one pass.
pub(crate) fn anim_type_runtime_keys<'a>(
    names: impl IntoIterator<Item = &'a str>,
) -> impl Iterator<Item = std::borrow::Cow<'a, str>> {
    // `eq_ignore_ascii_case` holds exactly when the ASCII uppercase spellings
    // are equal, so a set of those finds the same earlier twin.
    let mut seen = std::collections::HashSet::new();
    names.into_iter().enumerate().map(move |(index, name)| {
        if seen.insert(name.to_ascii_uppercase()) {
            std::borrow::Cow::Borrowed(name)
        } else {
            std::borrow::Cow::Owned(format!("\0ANIM:{index}:{name}"))
        }
    })
}

/// The [`anim_type_runtime_keys`] key of the one slot at `index`, found by
/// comparing the earlier names in place, without the set; a loop over every
/// slot takes the iterator instead.
pub(crate) fn anim_type_runtime_key<'a>(
    names: impl Iterator<Item = &'a str> + Clone,
    index: usize,
) -> Option<std::borrow::Cow<'a, str>> {
    let name = names.clone().nth(index)?;
    Some(
        if names
            .take(index)
            .any(|prior| prior.eq_ignore_ascii_case(name))
        {
            std::borrow::Cow::Owned(format!("\0ANIM:{index}:{name}"))
        } else {
            std::borrow::Cow::Borrowed(name)
        },
    )
}

/// HouseTypeClass::FindIndexOfName5117D0: the literal `<random>` returns-2;
/// otherwise each current Country's Name+64 is compared before its ID+24.
/// Both comparisons are case-insensitive, and an unknown token returns-1.
/// Side4767C0 and the gameplay identity projections share this one scan.
/// Executed controls: tools/input_oracle/campaign_start.py --houses,
/// campaign_start_houses.json side_controls and its meta sidecar.
pub(crate) fn house_type_index_of_name<'a>(
    incoming: &str,
    countries: impl Iterator<Item = (&'a str, Option<&'a str>)>,
) -> i32 {
    if incoming.eq_ignore_ascii_case("<random>") {
        return -2;
    }
    countries
        .enumerate()
        .find_map(|(index, (id, alias))| {
            (alias.is_some_and(|alias| alias.eq_ignore_ascii_case(incoming))
                || id.eq_ignore_ascii_case(incoming))
            .then(|| i32::try_from(index).expect("native Country index exceeds i32"))
        })
        .unwrap_or(-1)
}

/// ReadOwners4750D0/48DEB0's shared OR tail: only -1 is rejected;
/// <random>'s -2 sets bit30 through the native masked shift. Used by both
/// retained ordered readers and the immutable Rules projection.
pub(crate) fn house_type_mask_from_indices(indices: impl Iterator<Item = i32>) -> u32 {
    indices
        .filter(|&index| index != -1)
        .fold(0, |mask, index| mask | 1u32.wrapping_shl(index as u32))
}

/// Installed Ares Country Find1001BA90: reject none/<none> via10003950,
/// then scan current source-order stored ID+24, returning the first stricmp hit.
/// Name aliases, ParentCountry and whitespace trimming are not part of Find.
/// Native execution: tools/rules_oracle/ares_country_veterancy.json parent_find
/// (including registered sentinel IDs, duplicate IDs and untrimmed operands).
pub(crate) fn ares_country_index_of_name<'a>(
    incoming: &str,
    countries: impl Iterator<Item = &'a str>,
) -> Option<crate::rules::ruleset::CountryIdx> {
    ares_type_index_of_name(incoming, countries).map(|index| {
        crate::rules::ruleset::CountryIdx(
            u16::try_from(index).expect("COUNTRY_LIMIT keeps the Country registry within u16"),
        )
    })
}

/// Shared installed Ares Find template: Country1001BA90 and Building10007E50
/// both use none predicate10003950 then the source-order ID+24 stricmp scan.
/// Each caller supplies its actual currently registered native type array.
pub(crate) fn ares_type_index_of_name<'a>(
    incoming: &str,
    mut ids: impl Iterator<Item = &'a str>,
) -> Option<usize> {
    if is_native_none_type_name(incoming) {
        return None;
    }
    ids.position(|id| id.eq_ignore_ascii_case(incoming))
}

/// BuildingTypeExt's SuperWeapons IndexLookup1000D4D0: raw stored-ID
/// comparison, including a registered sentinel-named entry. Unlike the
/// pointer Find template, the none predicate runs only after a failed lookup.
/// The immutable RuleSet array-index projection uses this same owner.
pub(crate) fn super_weapon_index_of_name<'a>(
    incoming: &str,
    mut ids: impl Iterator<Item = &'a str>,
) -> Option<usize> {
    ids.position(|id| id.eq_ignore_ascii_case(incoming))
}

/// HouseClass::GetPlayerAtFromString @ 0x00510FB0 and ReadHouseType
/// @ 0x00475540 use selectors starting at 0x117B. VERA extends their eight
/// entries to the offline Skirmish row count, preserving the original A..H
/// identities.
pub(crate) const HOUSE_TYPE_PLAYER_AT_SELECTORS: std::ops::Range<i32> =
    0x117B..0x117B + crate::skirmish_launch::SKIRMISH_MAX_SLOT_COUNT as i32;

/// Shared case-sensitive player token reader for Session and TeamType House.
/// VERA extension: A..Z, then AA..AZ through DA..DP, represent the
/// one hundred twenty start slots.
pub(crate) fn house_type_player_at(incoming: &str) -> Option<u8> {
    // A..Z plus the four two-letter blocks A..D cover 130 selectors; the
    // slot bound below stops at DP, the one hundred twentieth.
    const _: () = assert!(crate::skirmish_launch::SKIRMISH_MAX_SLOT_COUNT <= 26 + 26 * 4);
    let letters = incoming.strip_prefix("<Player @ ")?.strip_suffix('>')?;
    let slot = match letters.as_bytes() {
        [letter @ b'A'..=b'Z'] => letter - b'A',
        // Slots 26..119: two letters, the first carrying the block of twenty-six.
        [first @ b'A'..=b'D', letter @ b'A'..=b'Z'] => 26 + (first - b'A') * 26 + (letter - b'A'),
        _ => return None,
    };
    (usize::from(slot) < crate::skirmish_launch::SKIRMISH_MAX_SLOT_COUNT).then_some(slot)
}

/// INIClass::Put_HouseType @ 0x00475610 player-selector text, extended to
/// the same one hundred twenty slots as the reader. Other HouseType values stay with
/// callers.
pub fn house_type_player_at_text(index: i32) -> Option<String> {
    if !HOUSE_TYPE_PLAYER_AT_SELECTORS.contains(&index) {
        return None;
    }
    let slot = (index - HOUSE_TYPE_PLAYER_AT_SELECTORS.start) as u8;
    let text = if slot < 26 {
        char::from(b'A' + slot).to_string()
    } else {
        // The same A..D blocks the reader takes: AA..AZ through DA..DP.
        let block = (slot - 26) / 26;
        format!(
            "{}{}",
            char::from(b'A' + block),
            char::from(b'A' + (slot - 26) % 26)
        )
    };
    Some(format!("<Player @ {text}>"))
}

/// One native `RulesClass::Process` source in its runtime position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RulesLayerKind {
    RulesMd,
    LangRule,
    GameMode,
    Scenario,
    Tmcj4f,
}

/// Whether these passes continue a live registry or enter Full_Init's reset.
#[derive(Debug, Clone, Copy)]
pub(crate) enum RulesProcessContext {
    Ordinary,
    ScenarioReset {
        theater: ImageTheater,
        /// ReadCCFile's LANGRULE digest-failure exit skips the root ColorAdd
        /// reread and mode Process, while Full_Init continues into the map.
        skip_root_color_add_and_mode: bool,
    },
}

#[derive(Debug, Clone)]
struct RulesLayer {
    kind: RulesLayerKind,
    ini: IniFile,
}

/// Ordered active-YR rules sources.
///
/// Each member remains an independent INI because `RulesClass::Process`
/// applies it to live type state. Flattening the text first is observably
/// wrong: a type allocated by a later map must not read an orphan body that
/// appeared in an earlier source.
#[derive(Debug, Clone)]
pub struct RulesLayerStack {
    layers: Vec<RulesLayer>,
    /// The game is an Ares game (`rules::ares_switch`): it mounted `ares.mix`,
    /// or the player's Ares setting is On.
    ships_ares: bool,
    /// VERA: the player's Ares setting is Off, so no Ares key turns Ares on
    /// (`AssetManager::ares_setting`).
    ares_keys_off: bool,
}

impl RulesLayerStack {
    pub fn new(rulesmd: IniFile) -> Self {
        Self {
            layers: vec![RulesLayer {
                kind: RulesLayerKind::RulesMd,
                ini: rulesmd,
            }],
            // The game's owner sets both from its asset manager
            // (`NativeRulesProcessOwner::startup_layers`). A stack built
            // without saying stands for the game its thread reads
            // (`ares_switch::ares_way`), with the Ares setting Auto.
            ships_ares: crate::rules::ares_switch::ares_way(),
            ares_keys_off: false,
        }
    }

    /// Build the stack for a game whose install or mod ships Ares's
    /// `ares.mix` (or not), which makes Ares play whatever these layers hold
    /// (`rules::ares_switch`); a stack processed from a fresh registry also
    /// reads its layers the Ares way. Set once while the stack is built,
    /// before any Process pass reads it.
    pub fn with_ares_mix(mut self, ships_ares: bool) -> Self {
        self.ships_ares = ships_ares;
        self
    }

    /// VERA: the stack of a game whose player set Ares Off, where no Ares key
    /// turns Ares on. The game's owner sets it from the launch
    /// (`NativeRulesProcessOwner::startup_layers`); a stack built without
    /// saying reads as Auto.
    pub fn with_ares_keys_off(mut self, off: bool) -> Self {
        self.ares_keys_off = off;
        self
    }

    pub fn push(&mut self, kind: RulesLayerKind, ini: IniFile) {
        self.layers.push(RulesLayer { kind, ini });
    }

    pub fn iter_passes(&self) -> impl Iterator<Item = (RulesLayerKind, &IniFile)> {
        self.layers.iter().map(|layer| (layer.kind, &layer.ini))
    }

    /// Hash both source contents and pass boundaries. Two stacks that flatten
    /// to the same key/value view can still produce different live type state.
    /// Portable ([`StateHasher`](crate::util::state_hasher::StateHasher)):
    /// the rules identity replays and saves compare folds it.
    pub fn content_hash(&self) -> u64 {
        self.content_hash_for_layers(self.layers.len())
    }

    fn content_hash_for_layers(&self, layer_count: usize) -> u64 {
        let mut hasher = crate::util::state_hasher::StateHasher::new();
        "VERA20K_RULES_LAYER_STACK_V1".hash(&mut hasher);
        layer_count.hash(&mut hasher);
        for layer in &self.layers[..layer_count] {
            layer.kind.hash(&mut hasher);
            layer.ini.content_hash().hash(&mut hasher);
        }
        hasher.finish()
    }

    /// Apply the verified registry-allocation and per-pass body timing with an
    /// intentionally empty fixed-Art source.
    ///
    /// The returned INI is a compatibility projection for existing typed Rust
    /// readers. It contains the final live scalar values, unioned registries,
    /// and only the per-type keys read at or after that type was allocated.
    ///
    /// Production active-YR loading uses NativeRulesProcessOwner with its
    /// fixed ARTMD and EVAMD snapshots. This convenience exists for synthetic
    /// Rules-only fixtures whose fixed Art/EVA sources are genuinely empty.
    pub fn process(&self) -> Result<ProcessedRulesLayers, RulesError> {
        self.process_with_fixed_art(&IniFile::empty())
    }

    /// Apply every Rules pass against fixed ARTMD with empty EVA and SOUNDMD
    /// catalogs, so no type sound resolves. Production uses the process owner
    /// to supply its fixed EVAMD and SOUNDMD.
    ///
    /// Retail provenance: `Load_Game_Rules @ 0x0052CD70` selects ARTMD before
    /// the first `RulesClass::Process @ 0x00668BF0`; `ReadTypeData @ 0x00679A10`
    /// then reuses global `g_ArtINI @ 0x00887180` on every later pass.
    pub fn process_with_fixed_art(
        &self,
        fixed_art: &IniFile,
    ) -> Result<ProcessedRulesLayers, RulesError> {
        self.process_with_fixed_art_and_registry_state(
            fixed_art,
            NativeRulesRegistryState::new(self.ships_ares),
        )
    }

    /// [`Self::process_with_fixed_art`] against a fixed SOUNDMD, which the
    /// process owner always supplies outside tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn process_with_fixed_art_and_sounds(
        &self,
        fixed_art: &IniFile,
        fixed_sounds: &SoundRegistry,
    ) -> Result<ProcessedRulesLayers, RulesError> {
        self.process_with_fixed_art_and_registry_state_recovering(
            fixed_art,
            NativeRulesRegistryState::new(self.ships_ares),
            RulesProcessContext::Ordinary,
            &EvaRegistry::default(),
            fixed_sounds,
        )
        .map_err(NativeRulesProcessingFailure::into_error)
    }

    /// Continue Process calls against an already-live process registry.
    ///
    /// Shell preview and gameplay Full_Init share native Type registries even
    /// though their numeric-ID cursors have distinct reset rules. This move-only
    /// input prevents a second Rust registry authority from being synthesized.
    pub(crate) fn process_with_fixed_art_and_registry_state(
        &self,
        fixed_art: &IniFile,
        registry_state: NativeRulesRegistryState,
    ) -> Result<ProcessedRulesLayers, RulesError> {
        self.process_with_fixed_art_and_registry_state_recovering(
            fixed_art,
            registry_state,
            RulesProcessContext::Ordinary,
            &EvaRegistry::default(),
            &crate::rules::sound_ini::SoundRegistry::default(),
        )
        .map_err(NativeRulesProcessingFailure::into_error)
    }

    /// Execute Process calls, optionally entering Full_Init's destructive
    /// reset, while retaining the sole registry owner when a later pass fails.
    ///
    /// Native `RulesClass::Process @ 0x00668BF0` is not transactional. A
    /// Tiberium failure can occur after earlier constructors in the same pass
    /// have already mutated the live registries. Process-lifetime callers must
    /// therefore recover that partial state instead of rolling back to the
    /// pre-call owner or synthesizing a fresh registry from the compatibility
    /// projection.
    pub(crate) fn process_with_fixed_art_and_registry_state_recovering(
        &self,
        fixed_art: &IniFile,
        registry_state: NativeRulesRegistryState,
        context: RulesProcessContext,
        fixed_eva: &EvaRegistry,
        fixed_sounds: &SoundRegistry,
    ) -> Result<ProcessedRulesLayers, NativeRulesProcessingFailure> {
        self.process_internal(
            fixed_art,
            registry_state,
            context,
            fixed_eva,
            None::<fn(RuleSet)>,
            fixed_sounds,
        )
        .map(|(processed, _)| processed)
    }

    /// Execute a read that native Full_Init places after map Rules and before
    /// its optional TMCJ4F Rules Process. The callback takes ownership of the
    /// map-stage RuleSet projection to retain read-time identities; the
    /// original processor remains the single authority and continues into
    /// TMCJ4F before producing the final Rules.
    pub(crate) fn process_with_fixed_art_and_registry_state_recovering_after_map<F, T>(
        &self,
        fixed_art: &IniFile,
        registry_state: NativeRulesRegistryState,
        context: RulesProcessContext,
        fixed_eva: &EvaRegistry,
        after_map: F,
        fixed_sounds: &SoundRegistry,
    ) -> Result<(ProcessedRulesLayers, T), NativeRulesProcessingFailure>
    where
        F: FnOnce(RuleSet) -> T,
    {
        let (processed, after_map_result) = self.process_internal(
            fixed_art,
            registry_state,
            context,
            fixed_eva,
            Some(after_map),
            fixed_sounds,
        )?;
        Ok((
            processed,
            after_map_result.expect("scenario Rules stack must contain its map pass"),
        ))
    }

    fn process_internal<F, T>(
        &self,
        fixed_art: &IniFile,
        registry_state: NativeRulesRegistryState,
        context: RulesProcessContext,
        fixed_eva: &EvaRegistry,
        mut after_map: Option<F>,
        fixed_sounds: &SoundRegistry,
    ) -> Result<(ProcessedRulesLayers, Option<T>), NativeRulesProcessingFailure>
    where
        F: FnOnce(RuleSet) -> T,
    {
        let (registry_state, mut root_color_add_reread, reset_color_add_after_root, theater) =
            match context {
                RulesProcessContext::Ordinary => {
                    (registry_state, None, false, ImageTheater::Uninitialized)
                }
                RulesProcessContext::ScenarioReset {
                    theater,
                    skip_root_color_add_and_mode,
                } => (
                    registry_state.destructive_reset(),
                    (!skip_root_color_add_and_mode).then_some(&self.layers[0].ini),
                    true,
                    theater,
                ),
            };
        let _ares_way = crate::rules::ares_switch::read_ares_way(registry_state.reads_ares_way);
        let mut processor = RulesPassProcessor::with_registry_state(registry_state);
        processor.image_theater = theater;
        let mut after_map_result = None;
        for (index, (kind, ini)) in self.iter_passes().enumerate() {
            if index > 0
                && kind != RulesLayerKind::LangRule
                && let Some(root) = root_color_add_reread.take()
            {
                // Reset668B13..668B8E rereads only root ColorAdd after
                // LANG668B05 and before mode668BAA/map68774F. In
                // particular, LANG's AudioVisual selectors stay current.
                processor.rules_color_add.apply_pass(root);
            }
            if let Err(error) = processor.apply_pass(
                ini,
                fixed_art,
                fixed_eva,
                kind == RulesLayerKind::RulesMd,
                fixed_sounds,
            ) {
                let (_, partial_trace) = processor.finish();
                return Err(NativeRulesProcessingFailure {
                    error,
                    partial_trace: Box::new(partial_trace),
                });
            }
            if index == 0 && reset_color_add_after_root {
                // Reset668A34..668A56 clears all16 RGB triples AFTER root
                // Process668A27. It leaves the four selector DWORDs alone.
                // Whole original Reset controls: tools/berserk_color.json.
                processor.rules_color_add = ColorAddTable::default();
            }
            if kind == RulesLayerKind::Scenario
                && let Some(after_map) = after_map.take()
            {
                let ares_inputs = crate::rules::ares_switch::AresInputs::from_passes(
                    &processor.ares_reads,
                    self.ships_ares,
                    self.ares_keys_off,
                );
                let (ini, native_type_construction_trace) = processor.clone().finish();
                let prefix = ProcessedRulesLayers {
                    ini,
                    content_hash: self.content_hash_for_layers(index + 1),
                    native_type_construction_trace,
                    ares_inputs,
                };
                let rules = match RuleSet::from_processed_rules(&prefix) {
                    Ok(rules) => rules,
                    Err(error) => {
                        let (_, partial_trace) =
                            prefix.into_ini_and_native_type_construction_trace();
                        return Err(NativeRulesProcessingFailure {
                            error,
                            partial_trace: Box::new(partial_trace),
                        });
                    }
                };
                after_map_result = Some(after_map(rules));
            }
        }
        if let Some(root) = root_color_add_reread {
            processor.rules_color_add.apply_pass(root);
        }
        if matches!(context, RulesProcessContext::ScenarioReset { .. }) {
            // Read_Map4AD071 / RMG59A201: all Overlay types receive this
            // postpass after the incoming scenario's ordered Rules reads.
            processor.init_overlay_theater();
            processor.init_anim_theater();
        }
        let ares_inputs = crate::rules::ares_switch::AresInputs::from_passes(
            &processor.ares_reads,
            self.ships_ares,
            self.ares_keys_off,
        );
        let (ini, native_type_construction_trace) = processor.finish();
        Ok((
            ProcessedRulesLayers {
                ini,
                content_hash: self.content_hash(),
                native_type_construction_trace,
                ares_inputs,
            },
            after_map_result,
        ))
    }
}

/// A failed native Process call plus the state already mutated before failure.
#[derive(Debug)]
pub(crate) struct NativeRulesProcessingFailure {
    error: RulesError,
    partial_trace: Box<NativeTypeConstructionTrace>,
}

impl NativeRulesProcessingFailure {
    pub(crate) fn into_parts(self) -> (RulesError, NativeTypeConstructionTrace) {
        (self.error, *self.partial_trace)
    }

    fn into_error(self) -> RulesError {
        self.error
    }
}

/// Reproduce the Type-constructor portion of active YR's cold rules startup.
///
/// This is deliberately not a `RulesClass::Process` call for the selected
/// RULESMD root. `Load_Game_Rules @ 0x0052CD70` first runs only Colors,
/// ColorAdd, `Read_Movies(ARTMD)`, `ReadAudioVisual(root)` and
/// `ReadMultiplayerDialogSettings(root)`, may then run one full Process for optional
/// `LANGRULE.INI`, and `Init_Game @ 0x0052BA60` follows with the root Anim and
/// Building master/body sweeps. Both body loops reload their live family count.
///
/// The input state makes the native direct-repeat behavior explicit. The real
/// cold call supplies [`NativeRulesRegistryState::default`]; a repeat without a
/// destructive reset continues the retained registries and emits only new
/// successful constructor events.
pub(crate) fn process_native_rules_cold_start(
    registry_state: NativeRulesRegistryState,
    selected_rules_root: &IniFile,
    fixed_art: &IniFile,
    langrule: Option<&IniFile>,
    fixed_eva: &EvaRegistry,
    fixed_sounds: &SoundRegistry,
) -> Result<NativeTypeConstructionTrace, RulesError> {
    process_native_rules_cold_start_inner(
        registry_state,
        selected_rules_root,
        fixed_art,
        langrule,
        fixed_eva,
        fixed_sounds,
    )
    .map(|(trace, _phase_event_counts)| trace)
}

/// Shared production/test implementation. Counts are cumulative boundaries
/// after AudioVisual, Anim master, Anim bodies, Building master, and Building
/// bodies respectively; retaining them here keeps the stock oracle tied to the
/// exact production sequence instead of duplicating that sequence in a test.
fn process_native_rules_cold_start_inner(
    registry_state: NativeRulesRegistryState,
    selected_rules_root: &IniFile,
    fixed_art: &IniFile,
    langrule: Option<&IniFile>,
    fixed_eva: &EvaRegistry,
    fixed_sounds: &SoundRegistry,
) -> Result<(NativeTypeConstructionTrace, [usize; 5]), RulesError> {
    let _ares_way = crate::rules::ares_switch::read_ares_way(registry_state.reads_ares_way);
    let mut processor = RulesPassProcessor::with_registry_state(registry_state);
    // Load_Game_Rules52D0FF calls Colors66D3A0 on the selected root before
    // ColorAdd52D111 and AudioVisual52D132, even without LANGRULE.INI.
    // Executed prefix: tools/rules_oracle/cold_colors.{py,json,meta.json}.
    processor.allocate_colors(selected_rules_root);
    processor.rules_color_add.apply_pass(selected_rules_root);
    processor.allocate_audio_visual_references(selected_rules_root, fixed_sounds);
    let after_audio_visual = processor.native_type_construction_events.len();
    if let Some(langrule) = langrule {
        processor.apply_pass(langrule, fixed_art, fixed_eva, false, fixed_sounds)?;
    }
    processor.allocate_explicit_family(
        selected_rules_root,
        "Animations",
        RulesTypeFamily::Animation,
        0x20,
    );
    let after_animation_master = processor.native_type_construction_events.len();
    processor.process_anim_family(fixed_art, fixed_sounds);
    let after_animation_bodies = processor.native_type_construction_events.len();
    processor.allocate_explicit_family(
        selected_rules_root,
        "BuildingTypes",
        RulesTypeFamily::Building,
        0x20,
    );
    let after_building_master = processor.native_type_construction_events.len();
    processor.process_techno_family(
        RulesTypeFamily::Building,
        selected_rules_root,
        fixed_art,
        fixed_sounds,
    );
    let after_building_bodies = processor.native_type_construction_events.len();
    let (_, trace) = processor.finish();
    Ok((
        trace,
        [
            after_audio_visual,
            after_animation_master,
            after_animation_bodies,
            after_building_master,
            after_building_bodies,
        ],
    ))
}

/// Run the constructor-capable noncampaign prefix before Full_Init's rules
/// destruction boundary.
///
/// Active YR `ScenarioClass::Full_Init @ 0x00686B20` performs the root
/// Countries master, root General references, then a live HouseType body loop
/// against the process-retained startup registries. The returned event vector
/// is therefore only `E_multi`; the caller must already have drained the older
/// cold-start events while retaining their registry state.
pub(crate) fn process_native_noncampaign_rules_prepass(
    registry_state: NativeRulesRegistryState,
    selected_rules_root: &IniFile,
) -> NativeTypeConstructionTrace {
    process_native_noncampaign_rules_prepass_inner(registry_state, selected_rules_root).0
}

/// Campaign Full_Init686D35 processes the optional scenario-basename INI
/// against the retained registry, then destroys Types. It skips Countries,
/// General and HouseType multiplayer prepass reads. The same Process body
/// handles every key; an absent named source performs no pass.
pub(crate) fn process_native_campaign_rules_prepass(
    registry_state: NativeRulesRegistryState,
    scenario_named_ini: Option<&IniFile>,
    fixed_art: &IniFile,
    fixed_eva: &EvaRegistry,
    fixed_sounds: &SoundRegistry,
) -> Result<NativeTypeConstructionTrace, NativeRulesProcessingFailure> {
    let _ares_way = crate::rules::ares_switch::read_ares_way(registry_state.reads_ares_way);
    let mut processor = RulesPassProcessor::with_registry_state(registry_state);
    if let Some(ini) = scenario_named_ini
        && let Err(error) = processor.apply_pass(ini, fixed_art, fixed_eva, false, fixed_sounds)
    {
        let (_, partial_trace) = processor.finish();
        return Err(NativeRulesProcessingFailure {
            error,
            partial_trace: Box::new(partial_trace),
        });
    }
    let (_, trace) = processor.finish();
    Ok(trace)
}

/// Shared production/test implementation. Counts are cumulative `E_multi`
/// boundaries after Countries, General, and the live HouseType body loop.
fn process_native_noncampaign_rules_prepass_inner(
    registry_state: NativeRulesRegistryState,
    selected_rules_root: &IniFile,
) -> (NativeTypeConstructionTrace, [usize; 3]) {
    let _ares_way = crate::rules::ares_switch::read_ares_way(registry_state.reads_ares_way);
    let mut processor = RulesPassProcessor::with_registry_state(registry_state);
    processor.allocate_explicit_family(
        selected_rules_root,
        "Countries",
        RulesTypeFamily::Country,
        0x20,
    );
    let after_countries = processor.native_type_construction_events.len();
    processor.allocate_general_references(selected_rules_root);
    let after_general = processor.native_type_construction_events.len();
    processor.process_house_family(selected_rules_root, true);
    let after_house_bodies = processor.native_type_construction_events.len();
    let (_, trace) = processor.finish();
    (trace, [after_countries, after_general, after_house_bodies])
}

/// Session ReadMultiPlayer6980C0 calls Countries6722F0 before its Side475540
/// read. It does not read Country bodies; PrepareSession52E123 does that later.
/// Executed controls: tools/rules_oracle/session_country.{py,json,meta.json}.
pub(crate) fn read_native_multiplayer_country(
    registry: NativeRulesRegistryState,
    root: &IniFile,
    settings: &IniFile,
    current: i32,
) -> (i32, NativeTypeConstructionTrace) {
    let _ares_way = crate::rules::ares_switch::read_ares_way(registry.reads_ares_way);
    let mut processor = RulesPassProcessor::with_registry_state(registry);
    processor.allocate_explicit_family(root, "Countries", RulesTypeFamily::Country, 0x20);
    let name = settings
        .section_or_empty("MultiPlayer")
        .read_string("Side", "", 0x80);
    let index = if name.is_empty() {
        current
    } else if let Some(player) = house_type_player_at(&name) {
        HOUSE_TYPE_PLAYER_AT_SELECTORS.start + i32::from(player)
    } else {
        let found = processor.country_index_of_name(&name);
        if found != -1 {
            found
        } else {
            // Unlike generic FindOrAllocate, this reader constructs none
            // spellings too (4755A0..4755F5). Past VERA's Country bound it
            // constructs nothing and keeps the current index.
            processor
                .find_or_construct_within_bound(RulesTypeFamily::Country, &name)
                .map_or(current, |index| {
                    i32::try_from(index).expect("Country index is within VERA's u16 bound")
                })
        }
    };
    (index, processor.finish().1)
}

/// PrepareSession52E123..52E14B reads each live Country against the selected
/// root after Session settings. D7/55FD31 omits this separate body pass.
pub(crate) fn refresh_native_multiplayer_country_bodies(
    registry: NativeRulesRegistryState,
    root: &IniFile,
) -> NativeTypeConstructionTrace {
    let _ares_way = crate::rules::ares_switch::read_ares_way(registry.reads_ares_way);
    let mut processor = RulesPassProcessor::with_registry_state(registry);
    processor.process_house_family(root, true);
    processor.finish().1
}

/// Retained RulesClass type vectors, distinct from the process-resident type
/// registries. The constructor @ 0x00665827 initializes them empty; ReadGeneral
/// @ 0x0066DA90/0x0066DB93 replaces MetallicDebris/BridgeExplosions only after
/// a nonempty ReadString128. Native execution:
/// tools/rules_oracle/bridge_anim_lists.{py,json}. WeatherConClouds
/// (`+0x2BC`, `0x0066DD28`) and WeatherConBolts (`+0x2D8`, `0x0066DE2B`) take
/// the same ReadString128/FindOrAllocate body.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
struct GeneralTypeLists {
    metallic_debris: Vec<String>,
    bridge_explosions: Vec<String>,
    weather_con_clouds: Vec<String>,
    weather_con_bolts: Vec<String>,
    /// `AmerParaDropInf=`, `AllyParaDropInf=`, `SovParaDropInf=` and
    /// `YuriParaDropInf=` (`+0xC04`, `+0xC3C`, `+0xC74`, `+0xCAC`, emptied by
    /// the constructor at `0x00666649..0x006666D8`), read at
    /// `0x00670648..0x00670780` through the InfantryType list reader
    /// `0x0067BB10`: the same body, through `InfantryTypeClass::FindOrAllocate
    /// @ 0x00524CB0`.
    paradrop_infantry: [Vec<String>; 4],
    /// `AnimToInfantry=` (`+0xCE4`, items `+0xCE8`, count `+0xCF4`, emptied
    /// by the constructor at `0x00666781`), read at `0x006707DC` through the
    /// same InfantryType list reader `0x0067BB10`: the types a `MakeInfantry=`
    /// anim's end creates (`AnimClass::AI @ 0x004249F9`).
    anim_to_infantry: Vec<String>,
    /// Rules data+0xFFC/count+0x1008;671D3E uses TerrainType reader67BDD0.
    /// Native reader/identity controls: spatial_oracle/mirage_disguise.
    default_mirage_disguises: Vec<String>,
    /// Rules+D58/D5C/D60, NULL at6667F8..666804; General66FB94..66FBEB
    /// uses67BAC0's ReadString128/InfantryType524CB0 scalar reader.
    infantry_disguises: [Option<String>; 3],
    /// Rules+D00/D1C/D38, emptied by666773..6667F8 and read in that
    /// order at66FA13..66FAD6 through the native ReadString128 list readers.
    secret_lab_choices: [Vec<String>; 3],
}

/// The `[General]` keys of [`GeneralTypeLists::paradrop_infantry`], in its
/// order.
const PARADROP_INFANTRY_KEYS: [&str; 4] = [
    "AmerParaDropInf",
    "AllyParaDropInf",
    "SovParaDropInf",
    "YuriParaDropInf",
];

/// The `[General]` type lists Ares 3.0 Buf_General (1005BB00, hook
/// 0x66D55E) reads first, in its order. Its int lists (`*ParaDropNum=`) are
/// RuleSet's.
const ARES_GENERAL_LISTS: [(&str, RulesTypeFamily); 17] = [
    ("AmerParaDropInf", RulesTypeFamily::Infantry),
    ("AllyParaDropInf", RulesTypeFamily::Infantry),
    ("SovParaDropInf", RulesTypeFamily::Infantry),
    ("YuriParaDropInf", RulesTypeFamily::Infantry),
    ("AnimToInfantry", RulesTypeFamily::Infantry),
    ("SecretInfantry", RulesTypeFamily::Infantry),
    ("SecretUnits", RulesTypeFamily::Vehicle),
    ("SecretBuildings", RulesTypeFamily::Building),
    ("HarvesterUnit", RulesTypeFamily::Vehicle),
    ("BaseUnit", RulesTypeFamily::Vehicle),
    ("PadAircraft", RulesTypeFamily::Aircraft),
    ("Shipyard", RulesTypeFamily::Building),
    ("RepairBay", RulesTypeFamily::Building),
    ("WeatherConClouds", RulesTypeFamily::Animation),
    ("WeatherConBolts", RulesTypeFamily::Animation),
    ("BridgeExplosions", RulesTypeFamily::Animation),
    ("DefaultMirageDisguises", RulesTypeFamily::Terrain),
];

/// Result of applying an ordered rules stack.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ProcessedRulesLayers {
    ini: IniFile,
    content_hash: u64,
    native_type_construction_trace: NativeTypeConstructionTrace,
    /// What the passes give the per-game Ares switch; saved with the rules,
    /// so a loaded game projects the same answer.
    ares_inputs: crate::rules::ares_switch::AresInputs,
}

#[cfg(all(test, tests_rules_a_n))]
#[path = "native_processing_tests.rs"]
mod tests;

#[cfg(all(test, tests_rules_a_n))]
#[path = "ares_country_veterancy_tests.rs"]
mod ares_country_veterancy_tests;

#[cfg(all(test, tests_rules_a_n))]
#[path = "ares_factory_owners_tests.rs"]
mod ares_factory_owners_tests;

#[cfg(all(test, tests_rules_a_n))]
#[path = "side_registry_tests.rs"]
mod side_registry_tests;

#[cfg(all(test, tests_rules_a_n))]
#[path = "ares_side_sidebar_tests.rs"]
mod ares_side_sidebar_tests;

#[cfg(all(test, tests_rules_a_n))]
#[path = "ares_country_paradrop_tests.rs"]
mod ares_country_paradrop_tests;

#[cfg(all(test, tests_rules_a_n))]
#[path = "ares_country_power_plants_tests.rs"]
mod ares_country_power_plants_tests;

#[cfg(all(test, tests_rules_a_n))]
#[path = "ares_country_presentation_tests.rs"]
mod ares_country_presentation_tests;

#[cfg(all(test, tests_rules_a_n))]
#[path = "ares_side_defaults_tests.rs"]
mod ares_side_defaults_tests;

#[cfg(all(test, tests_rules_a_n))]
#[path = "native_tech_level_tests.rs"]
mod native_tech_level_tests;

#[cfg(all(test, tests_rules_a_n))]
#[path = "projectile_art_tests.rs"]
mod projectile_art_tests;

#[cfg(all(test, tests_rules_a_n))]
#[path = "weapon_speed_tests.rs"]
mod weapon_speed_tests;

#[cfg(all(test, tests_rules_a_n))]
#[path = "weapon_registry_tests.rs"]
mod weapon_registry_tests;

#[cfg(all(test, tests_rules_a_n))]
#[path = "guided_controls_tests.rs"]
mod guided_controls_tests;

#[cfg(all(test, tests_rules_a_n))]
#[path = "select_anim_rules_tests.rs"]
mod select_anim_rules_tests;

#[cfg(all(test, tests_rules_a_n))]
#[path = "approach_rules_tests.rs"]
mod approach_rules_tests;

#[cfg(all(test, tests_rules_a_n))]
#[path = "chrono_beam_rules_tests.rs"]
mod chrono_beam_rules_tests;

#[cfg(all(test, tests_rules_a_n))]
#[path = "control_line_rules_tests.rs"]
mod control_line_rules_tests;

#[cfg(all(test, tests_rules_a_n))]
#[path = "disguise_cursor_rules_tests.rs"]
mod disguise_cursor_rules_tests;

#[cfg(all(test, tests_rules_a_n))]
#[path = "native_type_reader_tests.rs"]
mod native_type_reader_tests;

#[cfg(all(test, tests_rules_a_n))]
#[path = "ares_side_eva_tests.rs"]
mod ares_side_eva_tests;

#[cfg(all(test, tests_rules_a_n))]
#[path = "ares_country_loading_tests.rs"]
mod ares_country_loading_tests;
