//! Active-YR RulesClass processing: ordered passes, live Type registries,
//! constructor chronology and typed-reader compatibility projections.
//!
//! Raw INI storage stays with ini_parser; process_owner retains registry lifetime;
//! ruleset converts this processed output into gameplay definitions. Failed passes
//! preserve already-constructed registry state rather than rolling back.

use crate::rules::ares_armor::{AresWarheadVerses, ArmorCatalog};
use crate::rules::ares_prerequisites::GenericPrerequisites;
use crate::rules::armor::ArmorId;
#[cfg(not(feature = "ares"))]
use crate::rules::armor::armor_index;
use crate::rules::color_add::{ColorAddSelectors, ColorAddTable};
use crate::rules::compatibility::RulesCompatibility;
use crate::rules::crate_rules::CrateRules;
use crate::rules::error::RulesError;
use crate::rules::gunner_turrets::GunnerTurrets;
use crate::rules::ini_parser::{IniFile, IniSection, is_native_none_type_name};
use crate::rules::locomotor_type::SpeedType;

use crate::rules::missile_spawn::{MissileFamily, MissileSpawnRules};
use crate::rules::object_type::{
    InfantryInfiltration, InfantryPips, ObjectCategory, WEAPON_SLOT_COUNT,
};
use crate::rules::powerups::{PowerupTable, PowerupsAccumulator};
use crate::rules::prerequisite::{Prerequisite, PrerequisiteGroup, parse_prerequisites};
use crate::rules::projectile_type::ProjectileArtState;
use crate::rules::rules_type_references::{DEFAULT_PARTICLE_SYSTEMS, RulesTypeReferences};
use crate::rules::ruleset::{DetailRules, GeneralBuildingTypes, PrismSupportRules};
use crate::rules::superweapon_type::SuperWeaponBuildingRequirements;
use crate::rules::superweapon_type::UnitDeliveryRules;
#[cfg(feature = "ares")]
use crate::rules::superweapon_type::{DeliveryOwner, DeliveryType};
use crate::util::native_x87::NativeF64Bits;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

/// AbstractType constructor410800's stored byte ID. The INI store represents
/// native bytes as characters, so this cut is shared by Type and Campaign
/// registries without UTF-8 byte-boundary differences.
pub(crate) fn abstract_type_stored_id(incoming: &str) -> String {
    incoming.chars().take(0x18).collect()
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

/// HouseClassGetPlayerAtFromString510FB0's exact physical token table.
/// Shared by ReadHouseType475540 and TeamType House decoding.
pub(crate) fn house_type_player_at(incoming: &str) -> Option<u8> {
    let bytes = incoming.as_bytes();
    (bytes.len() == 12
        && bytes.starts_with(b"<Player @ ")
        && bytes[11] == b'>'
        && (b'A'..=b'H').contains(&bytes[10]))
    .then(|| bytes[10] - b'A')
}

/// One native `RulesClass::Process` source in its runtime position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RulesLayerKind {
    RulesMd,
    LangRule,
    GameMode,
    Scenario,
}

/// Whether these passes continue a live registry or enter Full_Init's reset.
#[derive(Debug, Clone, Copy)]
pub(crate) enum RulesProcessContext {
    Ordinary,
    ScenarioReset,
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
}

impl RulesLayerStack {
    pub fn new(rulesmd: IniFile) -> Self {
        Self {
            layers: vec![RulesLayer {
                kind: RulesLayerKind::RulesMd,
                ini: rulesmd,
            }],
        }
    }

    pub fn push(&mut self, kind: RulesLayerKind, ini: IniFile) {
        self.layers.push(RulesLayer { kind, ini });
    }

    pub fn iter_passes(&self) -> impl Iterator<Item = (RulesLayerKind, &IniFile)> {
        self.layers.iter().map(|layer| (layer.kind, &layer.ini))
    }

    /// Hash both source contents and pass boundaries. Two stacks that flatten
    /// to the same key/value view can still produce different live type state.
    pub fn content_hash(&self) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        "VERA20K_RULES_LAYER_STACK_V1".hash(&mut hasher);
        self.layers.len().hash(&mut hasher);
        for layer in &self.layers {
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
    /// Production active-YR loading must call [`Self::process_with_fixed_art`]
    /// with the one selected ARTMD.INI snapshot. This convenience exists for
    /// synthetic Rules-only fixtures whose fixed-Art source is genuinely empty.
    pub fn process(&self) -> Result<ProcessedRulesLayers, RulesError> {
        self.process_with_fixed_art(&IniFile::empty())
    }

    /// Apply every Rules pass against the same fixed ARTMD.INI snapshot.
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
            NativeRulesRegistryState::default(),
        )
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
    ) -> Result<ProcessedRulesLayers, NativeRulesProcessingFailure> {
        let (registry_state, mut root_color_add_reread) = match context {
            RulesProcessContext::Ordinary => (registry_state, None),
            RulesProcessContext::ScenarioReset => (
                registry_state.destructive_reset(),
                Some(&self.layers[0].ini),
            ),
        };
        let mut processor = RulesPassProcessor::with_registry_state(registry_state);
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
            if let Err(error) = processor.apply_pass(ini, fixed_art) {
                let (_, partial_trace, _) = processor.finish();
                return Err(NativeRulesProcessingFailure {
                    error,
                    partial_trace: Box::new(partial_trace),
                });
            }
            if index == 0 && root_color_add_reread.is_some() {
                // Reset668A34..668A56 clears all16 RGB triples AFTER root
                // Process668A27. It leaves the four selector DWORDs alone.
                // Whole original Reset controls: tools/berserk_color.json.
                processor.rules_color_add = ColorAddTable::default();
            }
        }
        if let Some(root) = root_color_add_reread {
            processor.rules_color_add.apply_pass(root);
        }
        let (ini, native_type_construction_trace, powerups) = processor.finish();
        Ok(ProcessedRulesLayers {
            ini,
            powerups,
            content_hash: self.content_hash(),
            native_type_construction_trace,
        })
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
/// RULESMD root. `Load_Game_Rules @ 0x0052CD70` first runs only
/// `ReadAudioVisual(root)`, may then run one full Process for optional
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
) -> Result<NativeTypeConstructionTrace, RulesError> {
    process_native_rules_cold_start_inner(registry_state, selected_rules_root, fixed_art, langrule)
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
) -> Result<(NativeTypeConstructionTrace, [usize; 5]), RulesError> {
    let mut processor = RulesPassProcessor::with_registry_state(registry_state);
    // Cold52D111 reads ColorAdd66D480 before AudioVisual52D132.
    processor.rules_color_add.apply_pass(selected_rules_root);
    processor.allocate_audio_visual_references(selected_rules_root);
    let after_audio_visual = processor.native_type_construction_events.len();
    if let Some(langrule) = langrule {
        processor.apply_pass(langrule, fixed_art)?;
    }
    processor.allocate_explicit_family(
        selected_rules_root,
        "Animations",
        RulesTypeFamily::Animation,
    );
    let after_animation_master = processor.native_type_construction_events.len();
    processor.process_anim_family(fixed_art);
    let after_animation_bodies = processor.native_type_construction_events.len();
    processor.allocate_explicit_family(
        selected_rules_root,
        "BuildingTypes",
        RulesTypeFamily::Building,
    );
    let after_building_master = processor.native_type_construction_events.len();
    processor.process_techno_family(RulesTypeFamily::Building, selected_rules_root, fixed_art);
    let after_building_bodies = processor.native_type_construction_events.len();
    let (_, trace, _) = processor.finish();
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
) -> Result<NativeTypeConstructionTrace, NativeRulesProcessingFailure> {
    let mut processor = RulesPassProcessor::with_registry_state(registry_state);
    if let Some(ini) = scenario_named_ini
        && let Err(error) = processor.apply_pass(ini, fixed_art)
    {
        let (_, partial_trace, _) = processor.finish();
        return Err(NativeRulesProcessingFailure {
            error,
            partial_trace: Box::new(partial_trace),
        });
    }
    let (_, trace, _) = processor.finish();
    Ok(trace)
}

/// Shared production/test implementation. Counts are cumulative `E_multi`
/// boundaries after Countries, General, and the live HouseType body loop.
fn process_native_noncampaign_rules_prepass_inner(
    registry_state: NativeRulesRegistryState,
    selected_rules_root: &IniFile,
) -> (NativeTypeConstructionTrace, [usize; 3]) {
    let mut processor = RulesPassProcessor::with_registry_state(registry_state);
    processor.allocate_explicit_family(selected_rules_root, "Countries", RulesTypeFamily::Country);
    let after_countries = processor.native_type_construction_events.len();
    processor.allocate_general_references(selected_rules_root);
    let after_general = processor.native_type_construction_events.len();
    processor.process_house_family(selected_rules_root);
    let after_house_bodies = processor.native_type_construction_events.len();
    let (_, trace, _) = processor.finish();
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
    let mut processor = RulesPassProcessor::with_registry_state(registry);
    processor.allocate_explicit_family(root, "Countries", RulesTypeFamily::Country);
    let name = settings
        .section_or_empty("MultiPlayer")
        .read_string("Side", "", 0x80);
    let index = if name.is_empty() {
        current
    } else if let Some(player) = house_type_player_at(&name) {
        0x117b + i32::from(player)
    } else {
        let found = processor.country_index_of_name(&name);
        if found != -1 {
            found
        } else {
            // Unlike generic FindOrAllocate, this reader constructs none
            // spellings too (4755A0..4755F5).
            i32::try_from(processor.find_or_construct(RulesTypeFamily::Country, &name))
                .expect("native Country index exceeds i32")
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
    let mut processor = RulesPassProcessor::with_registry_state(registry);
    processor.process_house_family(root);
    processor.finish().1
}

/// Retained RulesClass type vectors, distinct from the process-resident type
/// registries. The constructor @ 0x00665827 initializes them empty; ReadGeneral
/// @ 0x0066DA90/0x0066DB93 replaces MetallicDebris/BridgeExplosions only after
/// a nonempty ReadString128. Native execution:
/// tools/rules_oracle/bridge_anim_lists.{py,json}. WeatherConClouds
/// (`+0x2BC`, `0x0066DD28`) and WeatherConBolts (`+0x2D8`, `0x0066DE2B`) take
/// the same ReadString128/FindOrAllocate body.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
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

/// Result of applying an ordered rules stack.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ProcessedRulesLayers {
    ini: IniFile,
    powerups: PowerupTable,
    content_hash: u64,
    native_type_construction_trace: NativeTypeConstructionTrace,
}

impl ProcessedRulesLayers {
    /// Ares definitions have the same retained process lifetime as Types.
    pub(crate) fn ares_prerequisites(&self) -> Option<&GenericPrerequisites> {
        self.native_type_construction_trace
            .registry_state
            .ares_prerequisites
            .as_ref()
    }

    pub(crate) fn ares_armors(&self) -> Option<&ArmorCatalog> {
        self.native_type_construction_trace
            .registry_state
            .ares_armors
            .as_ref()
    }

    /// Read-only projection of the retained registry's build capability.
    pub(crate) fn compatibility(&self) -> RulesCompatibility {
        self.native_type_construction_trace
            .registry_state
            .compatibility
    }

    pub(crate) fn native_registry_state(&self) -> &NativeRulesRegistryState {
        self.native_type_construction_trace.registry_state()
    }

    pub fn ini(&self) -> &IniFile {
        &self.ini
    }

    /// Transfer constructor chronology once, retaining the same processed
    /// Rules/type authority for save and later process transitions.
    pub(crate) fn take_constructor_events(&mut self) -> (Vec<NativeTypeConstructionEvent>, usize) {
        (
            std::mem::take(&mut self.native_type_construction_trace.events),
            self.native_type_construction_trace
                .allocated_super_weapon_type_count,
        )
    }

    /// RulesLoad675210 and the type hierarchy restore live fields, rather than
    /// executing fresh Full_Init/Process. VERA persists this existing processed
    /// state and rebuilds its RuleSet projection; no constructors or IDs replay.
    /// Reject indexes before any projection can dereference a saved type slot.
    pub(crate) fn validate_saved_state(&self) -> Result<(), RulesError> {
        let registry = &self.native_type_construction_trace.registry_state;
        if !registry.compatibility.matches_build() {
            return Err(RulesError::InvalidSavedState(
                "saved Rules require a different compiled Ares capability",
            ));
        }
        #[cfg(not(feature = "ares"))]
        if registry.families.values().flatten().any(|member| {
            member.super_weapon_building_requirements.is_some()
                || member.generic_super_weapon_warhead.is_some()
                || member.unit_delivery.is_some()
        }) {
            return Err(RulesError::InvalidSavedState(
                "saved Rules contain disabled Ares extension state",
            ));
        }
        let families = &registry.families;
        if registry.ares_armors.is_some() != crate::rules::ARES_ENABLED {
            return Err(RulesError::InvalidSavedState(
                "saved armor capability differs from build",
            ));
        }
        if let Some(armors) = &registry.ares_armors {
            if !armors.valid_saved_state()
                || families
                    .values()
                    .flatten()
                    .any(|member| !armors.contains(member.armor_id))
                || families.iter().any(|(&family, members)| {
                    members.iter().any(|member| {
                        if family == RulesTypeFamily::Warhead {
                            member
                                .ares_verses
                                .as_ref()
                                .is_none_or(|verses| !verses.valid_saved_state(armors))
                        } else {
                            member.ares_verses.is_some()
                        }
                    })
                })
            {
                return Err(RulesError::InvalidSavedState("invalid Ares armor registry"));
            }
        } else if families
            .values()
            .flatten()
            .any(|member| member.armor_id.index() >= 11 || member.ares_verses.is_some())
        {
            return Err(RulesError::InvalidSavedState(
                "Ares armor state in retail rules",
            ));
        }
        registry.validate_techno_type_order()?;
        registry.validate_prerequisites()?;
        let families = &registry.families;
        let projectile_count = families
            .get(&RulesTypeFamily::Projectile)
            .map_or(0, Vec::len);
        let warhead_count = families.get(&RulesTypeFamily::Warhead).map_or(0, Vec::len);
        let particle_count = families.get(&RulesTypeFamily::Particle).map_or(0, Vec::len);
        if families.values().flatten().any(|member| {
            usize::from(member.building_foundation)
                >= crate::rules::foundation::FOUNDATION_TABLE.len()
                || member
                    .weapon
                    .projectile
                    .is_some_and(|index| index >= projectile_count)
                || member
                    .weapon
                    .warhead
                    .is_some_and(|index| index >= warhead_count)
                || member
                    .holds_what
                    .is_some_and(|index| index >= particle_count)
        }) {
            return Err(RulesError::InvalidSavedState(
                "type reference is outside its registry",
            ));
        }
        Ok(())
    }

    /// The live global TechnoType array, not a concatenation of the four
    /// subtype arrays. Constructors71181D/711ABD append each new identity.
    /// TAction106's active inline body6DF5D1 scans this order backwards.
    pub(crate) fn techno_type_states(&self) -> impl Iterator<Item = (ObjectCategory, &str, i32)> {
        self.native_type_construction_trace
            .registry_state()
            .techno_type_states()
    }

    /// Allocated AnimTypes and whether their fixed-ART body has actually been
    /// read, in native registry order. A present ART section alone is not
    /// evidence of a read: later Type readers can allocate after the Anim sweep.
    pub(crate) fn anim_type_art_read_states(&self) -> impl Iterator<Item = (&str, bool)> {
        self.native_type_construction_trace
            .registry_state()
            .anim_type_art_read_states()
    }

    /// Retained Bullet/ObjectType Image and ART fields after each native pass.
    pub(crate) fn projectile_art_states(
        &self,
    ) -> impl Iterator<Item = (&str, &ProjectileArtState)> {
        self.native_type_construction_trace
            .registry_state()
            .families
            .get(&RulesTypeFamily::Projectile)
            .into_iter()
            .flatten()
            .map(|member| (member.native_stored_id.as_str(), &member.projectile_art))
    }

    /// BuildingType+16B0/EA4/EA8/EAC retained by its original reached body.
    pub(crate) fn secret_lab_states(
        &self,
    ) -> impl Iterator<Item = (&str, &crate::rules::secret_lab::SecretLabRules)> {
        self.native_type_construction_trace
            .registry_state()
            .families
            .get(&RulesTypeFamily::Building)
            .into_iter()
            .flatten()
            .map(|member| (member.native_stored_id.as_str(), &member.secret_lab))
    }

    pub(crate) fn secret_lab_choices(&self) -> &[Vec<String>; 3] {
        &self.general_type_lists().secret_lab_choices
    }

    /// BuildingType +EF0 after each reached ReadINI body, retained with its
    /// native process registry rather than reconstructed from merged Rules keys.
    pub(crate) fn building_foundation_states(&self) -> impl Iterator<Item = (&str, u8)> {
        self.native_type_construction_trace
            .registry_state()
            .families
            .get(&RulesTypeFamily::Building)
            .into_iter()
            .flatten()
            .map(|member| (member.native_stored_id.as_str(), member.building_foundation))
    }

    /// BuildingType+154B after each reached Rules body; current-default ReadBool.
    pub(crate) fn building_spotlight_flags(&self) -> impl Iterator<Item = (&str, bool)> {
        self.native_type_construction_trace
            .registry_state()
            .families
            .get(&RulesTypeFamily::Building)
            .into_iter()
            .flatten()
            .map(|member| {
                (
                    member.native_stored_id.as_str(),
                    member.building_has_spotlight,
                )
            })
    }

    /// BuildingType+E30..E40 after each admitted Rules section, including
    /// missing-key integer-default conversions460C8C..460DCA.
    pub(crate) fn building_light_states(
        &self,
    ) -> impl Iterator<Item = (&str, crate::rules::building_light::BuildingLightRules)> {
        self.native_type_construction_trace
            .registry_state()
            .families
            .get(&RulesTypeFamily::Building)
            .into_iter()
            .flatten()
            .map(|member| (member.native_stored_id.as_str(), member.building_light))
    }

    /// BuildingType's retained upgrade inputs after the last reached Rules body.
    pub(crate) fn building_upgrade_states(
        &self,
    ) -> impl Iterator<Item = (&str, &crate::rules::building_upgrade::BuildingUpgradeRules)> {
        self.native_type_construction_trace
            .registry_state()
            .families
            .get(&RulesTypeFamily::Building)
            .into_iter()
            .flatten()
            .map(|member| (member.native_stored_id.as_str(), &member.building_upgrade))
    }

    /// BuildingType+EBC after each reached Rules body. Fixed ART never reads it.
    pub(crate) fn building_target_coord_offsets(&self) -> impl Iterator<Item = (&str, [i32; 3])> {
        self.native_type_construction_trace
            .registry_state()
            .families
            .get(&RulesTypeFamily::Building)
            .into_iter()
            .flatten()
            .map(|member| {
                (
                    member.native_stored_id.as_str(),
                    member.building_target_coord_offset,
                )
            })
    }

    /// Unit747620's retained frame fields after the last reached Rules body.
    /// Missing Rules sections skip ART; the fixed ART itself is never layered.
    pub(crate) fn unit_shp_read_states(
        &self,
    ) -> impl Iterator<Item = (&str, &crate::rules::shp_vehicle_sequence::UnitShpReadState)> {
        self.native_type_construction_trace
            .registry_state()
            .families
            .get(&RulesTypeFamily::Vehicle)
            .into_iter()
            .flatten()
            .map(|member| (member.native_stored_id.as_str(), &member.unit_shp))
    }

    /// UnitType+67C after the last admitted Rules body; an unread type keeps
    /// constructor -1. Missing sections and missing SpeedType keys differ.
    pub(crate) fn unit_speed_type_states(&self) -> impl Iterator<Item = (&str, SpeedType)> {
        self.native_type_construction_trace
            .registry_state()
            .families
            .get(&RulesTypeFamily::Vehicle)
            .into_iter()
            .flatten()
            .map(|member| (member.native_stored_id.as_str(), member.unit_speed_type))
    }

    /// TechnoType +810/+814 state after the reached generic and FV readers.
    /// The FV pairs have literal defaults and overlapping writes, so merged
    /// source keys cannot reconstruct this process-resident result.
    pub(crate) fn gunner_turret_states(
        &self,
    ) -> impl Iterator<Item = (ObjectCategory, &str, &GunnerTurrets)> {
        let families = &self
            .native_type_construction_trace
            .registry_state()
            .families;
        [
            RulesTypeFamily::Building,
            RulesTypeFamily::Aircraft,
            RulesTypeFamily::Vehicle,
            RulesTypeFamily::Infantry,
        ]
        .into_iter()
        .flat_map(move |family| {
            let category = family.object_category().expect("TechnoType family");
            families
                .get(&family)
                .into_iter()
                .flatten()
                .map(move |member| {
                    (
                        category,
                        member.native_stored_id.as_str(),
                        &member.gunner_turrets,
                    )
                })
        })
    }

    /// Final retained InfantryType state; the merged present-key history cannot
    /// represent admitted missing-Pip calls. Projection performs no new read.
    pub(crate) fn infantry_pip_states(&self) -> impl Iterator<Item = (&str, InfantryPips)> {
        self.native_type_construction_trace
            .registry_state()
            .families
            .get(&RulesTypeFamily::Infantry)
            .into_iter()
            .flatten()
            .map(|member| (member.native_stored_id.as_str(), member.infantry_pips))
    }

    /// Infantry52466E's postprocessed flags, including a prior role's latch
    /// when a later pass omits Infiltrate while clearing the role itself.
    pub(crate) fn infantry_infiltration_states(
        &self,
    ) -> impl Iterator<Item = (&str, InfantryInfiltration)> {
        self.native_type_construction_trace
            .registry_state()
            .families
            .get(&RulesTypeFamily::Infantry)
            .into_iter()
            .flatten()
            .map(|member| {
                (
                    member.native_stored_id.as_str(),
                    member.infantry_infiltration,
                )
            })
    }

    /// Native715320 recopies the turret controls on every reached rules pass.
    pub(crate) fn recoil_states(
        &self,
    ) -> impl Iterator<Item = (ObjectCategory, &str, crate::rules::recoil::RecoilConfig)> {
        self.native_type_construction_trace
            .registry_state()
            .families
            .iter()
            .filter_map(|(family, members)| {
                family.object_category().map(|category| (category, members))
            })
            .flat_map(|(category, members)| {
                members
                    .iter()
                    .map(move |member| (category, member.native_stored_id.as_str(), member.recoil))
            })
    }

    /// Retained six-field TechnoType71458A Airstrike reader state.
    pub(crate) fn airstrike_states(
        &self,
    ) -> impl Iterator<
        Item = (
            ObjectCategory,
            &str,
            &crate::rules::airstrike::AirstrikeSettings,
        ),
    > {
        self.native_type_construction_trace
            .registry_state()
            .families
            .iter()
            .filter_map(|(family, members)| {
                family.object_category().map(|category| (category, members))
            })
            .flat_map(|(category, members)| {
                members.iter().map(move |member| {
                    (
                        category,
                        member.native_stored_id.as_str(),
                        &member.airstrike,
                    )
                })
            })
    }

    /// TechnoType+6A4 after every reached ReadINI7147B4 pass.
    pub(crate) fn radial_fire_segments_states(
        &self,
    ) -> impl Iterator<Item = (ObjectCategory, &str, i32)> {
        self.native_type_construction_trace
            .registry_state()
            .families
            .iter()
            .filter_map(|(family, members)| {
                family.object_category().map(|category| (category, members))
            })
            .flat_map(|(category, members)| {
                members.iter().map(move |member| {
                    (
                        category,
                        member.native_stored_id.as_str(),
                        member.radial_fire_segments,
                    )
                })
            })
    }

    /// TechnoType+3DC after each reached ReadInt714145..714159.
    pub(crate) fn lepton_mind_control_offset_states(
        &self,
    ) -> impl Iterator<Item = (ObjectCategory, &str, i32)> {
        self.native_type_construction_trace
            .registry_state()
            .families
            .iter()
            .filter_map(|(family, members)| {
                family.object_category().map(|category| (category, members))
            })
            .flat_map(|(category, members)| {
                members.iter().map(move |member| {
                    (
                        category,
                        member.native_stored_id.as_str(),
                        member.lepton_mind_control_offset,
                    )
                })
            })
    }

    /// TechnoType's TurretCount, WeaponCount and base/elite weapon arrays
    /// after every reached ReadINI pass.
    pub(crate) fn weapon_slot_states(
        &self,
    ) -> impl Iterator<Item = (ObjectCategory, &str, &WeaponSlots)> {
        self.native_type_construction_trace
            .registry_state()
            .families
            .iter()
            .filter_map(|(family, members)| {
                family.object_category().map(|category| (category, members))
            })
            .flat_map(|(category, members)| {
                members.iter().map(move |member| {
                    (
                        category,
                        member.native_stored_id.as_str(),
                        &member.weapon_slots,
                    )
                })
            })
    }

    /// ParticleSystemType+294 after every reached ReadINI: the bound
    /// ParticleType's stored ID, or `None` for `<none>` and unread types.
    pub(crate) fn particle_system_holds_what_states(
        &self,
    ) -> impl Iterator<Item = (&str, Option<&str>)> {
        let families = &self
            .native_type_construction_trace
            .registry_state()
            .families;
        families
            .get(&RulesTypeFamily::ParticleSystem)
            .into_iter()
            .flatten()
            .map(move |member| {
                let particle = member.holds_what.map(|index| {
                    families[&RulesTypeFamily::Particle][index]
                        .native_stored_id
                        .as_str()
                });
                (member.native_stored_id.as_str(), particle)
            })
    }

    /// Each WeaponType's retained reads after the last Weapon sweep, in the
    /// actual WeaponType array order. References remain native family indices
    /// until this read-only projection.
    pub(crate) fn weapon_states(&self) -> impl Iterator<Item = WeaponState<'_>> {
        let families = &self
            .native_type_construction_trace
            .registry_state()
            .families;
        families
            .get(&RulesTypeFamily::Weapon)
            .into_iter()
            .flatten()
            .map(move |member| {
                let name = |family: RulesTypeFamily, index: usize| {
                    families[&family][index].native_stored_id.as_str()
                };
                WeaponState {
                    id: member.native_stored_id.as_str(),
                    damage: member.weapon.damage,
                    speed: member.weapon.speed,
                    range_leptons: member.weapon.range_leptons,
                    projectile: member
                        .weapon
                        .projectile
                        .map(|index| name(RulesTypeFamily::Projectile, index)),
                    warhead: member
                        .weapon
                        .warhead
                        .map(|index| name(RulesTypeFamily::Warhead, index)),
                    anim: &member.weapon.anim,
                    occupant_anim: member.weapon.occupant_anim.as_deref(),
                    open_topped_anim: member.weapon.open_topped_anim.as_deref(),
                    attached_particle_system: member.weapon.attached_particle_system.as_deref(),
                }
            })
    }

    /// Each TechnoType's Image and pointer fields after every reached
    /// ReadINI pass.
    pub(crate) fn techno_reference_states(
        &self,
    ) -> impl Iterator<Item = (ObjectCategory, &str, &TechnoTypeReferences)> {
        self.native_type_construction_trace
            .registry_state()
            .families
            .iter()
            .filter_map(|(family, members)| {
                family.object_category().map(|category| (category, members))
            })
            .flat_map(|(category, members)| {
                members.iter().map(move |member| {
                    (
                        category,
                        member.native_stored_id.as_str(),
                        &member.techno_references,
                    )
                })
            })
    }

    /// TechnoType+7FC after each reached fixed-ART ShadowIndex read.
    pub(crate) fn voxel_shadow_states(&self) -> impl Iterator<Item = (ObjectCategory, &str, i32)> {
        self.native_type_construction_trace
            .registry_state()
            .families
            .iter()
            .filter_map(|(family, members)| {
                family.object_category().map(|category| (category, members))
            })
            .flat_map(|(category, members)| {
                members.iter().map(move |member| {
                    (
                        category,
                        member.native_stored_id.as_str(),
                        member.voxel_shadow_index,
                    )
                })
            })
    }

    /// Each SuperWeaponType's `WeaponType=` (+9C) and `AuxBuilding=` (+C8)
    /// after every reached pass.
    pub(crate) fn super_weapon_reference_states(
        &self,
    ) -> impl Iterator<Item = (&str, (Option<&str>, Option<&str>))> {
        self.member_states(RulesTypeFamily::SuperWeapon, |member| {
            (
                member.super_weapon_weapon.as_deref(),
                member.super_weapon_aux_building.as_deref(),
            )
        })
    }

    pub(crate) fn super_weapon_building_requirements(
        &self,
    ) -> impl Iterator<Item = (&str, Option<&SuperWeaponBuildingRequirements>)> {
        self.member_states(RulesTypeFamily::SuperWeapon, |member| {
            member.super_weapon_building_requirements.as_ref()
        })
    }

    /// Ares GenericWarhead references, resolved by the same ordered type
    /// allocator as native references. Kept separate from the retail fields.
    pub(crate) fn generic_super_weapon_warhead_states(
        &self,
    ) -> impl Iterator<Item = (&str, Option<&str>)> {
        self.member_states(RulesTypeFamily::SuperWeapon, |member| {
            member.generic_super_weapon_warhead.as_deref()
        })
    }

    pub(crate) fn unit_delivery_states(
        &self,
    ) -> impl Iterator<Item = (&str, Option<&UnitDeliveryRules>)> {
        self.member_states(RulesTypeFamily::SuperWeapon, |member| {
            member.unit_delivery.as_ref()
        })
    }

    /// Each ParticleType's `Warhead=` after every reached pass.
    pub(crate) fn particle_warhead_states(&self) -> impl Iterator<Item = (&str, Option<&str>)> {
        self.member_states(RulesTypeFamily::Particle, |member| {
            member.particle_warhead.as_deref()
        })
    }

    /// Each VoxelAnimType's references after every reached pass.
    pub(crate) fn voxel_anim_reference_states(
        &self,
    ) -> impl Iterator<Item = (&str, &VoxelAnimTypeReferences)> {
        self.member_states(RulesTypeFamily::VoxelAnimation, |member| {
            &member.voxel_anim_references
        })
    }

    /// Each AnimType's fixed-ART references, in AnimType registry order; a
    /// type whose ART body was never read keeps the constructor's nulls.
    pub(crate) fn anim_type_reference_states(
        &self,
    ) -> impl Iterator<Item = (&str, &AnimTypeReferences)> {
        self.member_states(RulesTypeFamily::Animation, |member| &member.anim_references)
    }

    /// Each BulletType's `ShrapnelWeapon=` (+2B4) after every reached pass.
    pub(crate) fn shrapnel_weapon_states(&self) -> impl Iterator<Item = (&str, Option<&str>)> {
        self.member_states(RulesTypeFamily::Projectile, |member| {
            member.shrapnel_weapon.as_deref()
        })
    }

    fn member_states<'a, T>(
        &'a self,
        family: RulesTypeFamily,
        state: impl Fn(&'a ProcessedType) -> T + 'a,
    ) -> impl Iterator<Item = (&'a str, T)> + 'a {
        self.native_type_construction_trace
            .registry_state()
            .families
            .get(&family)
            .into_iter()
            .flatten()
            .map(move |member| (member.native_stored_id.as_str(), state(member)))
    }

    /// `[CombatDamage] DeathWeapon=` (Rules+FDC) after the last pass.
    pub(crate) fn combat_damage_death_weapon(&self) -> Option<&str> {
        self.native_type_construction_trace
            .registry_state()
            .rules_death_weapon
            .as_deref()
    }

    /// The RulesClass Type pointers and lists of [`RulesTypeReferences`]
    /// after the last pass.
    pub(crate) fn rules_type_references(&self) -> &RulesTypeReferences {
        &self
            .native_type_construction_trace
            .registry_state()
            .rules_type_references
    }

    /// `[AI] NeutralTechBuildings=` (Rules+ADC) after the last pass, as
    /// stored BuildingType IDs.
    pub fn neutral_tech_buildings(&self) -> &[String] {
        self.rules_type_references().list("NeutralTechBuildings")
    }

    /// Retained Warhead ReadINI fields, including+0xA0 after its last reached
    /// ReadINI75DDCC..75DE5A. The compatibility INI cannot represent the
    /// table's literal per-pass default.
    pub(crate) fn warhead_read_states(
        &self,
    ) -> impl Iterator<
        Item = (
            &str,
            bool,
            bool,
            &[String],
            &[String],
            &[NativeF64Bits; 11],
            Option<&AresWarheadVerses>,
        ),
    > {
        self.native_type_construction_trace
            .registry_state()
            .families
            .get(&RulesTypeFamily::Warhead)
            .into_iter()
            .flatten()
            .map(|member| {
                (
                    &*member.native_stored_id,
                    member.warhead_anim.conventional,
                    member.warhead_anim.em_effect,
                    member.warhead_anim.anim_list.as_slice(),
                    member.warhead_anim.debris_types.as_slice(),
                    &member.warhead_verses,
                    member.ares_verses.as_ref(),
                )
            })
    }

    pub(crate) fn object_armor_states(
        &self,
    ) -> impl Iterator<Item = (Option<ObjectCategory>, &str, ArmorId)> {
        self.native_type_construction_trace
            .registry_state()
            .families
            .iter()
            .filter(|(family, _)| {
                family.object_category().is_some() || **family == RulesTypeFamily::Terrain
            })
            .flat_map(|(&family, members)| {
                members.iter().map(move |member| {
                    (
                        family.object_category(),
                        member.native_stored_id.as_str(),
                        member.armor_id,
                    )
                })
            })
    }

    pub(crate) fn select_anim_rules(&self) -> (&str, &str, &str, &[String]) {
        let state = &self
            .native_type_construction_trace
            .registry_state()
            .select_anim;
        (
            &state.lightning_warhead,
            &state.weather_con_bolt_explosion,
            &state.weapon_nullify_anim,
            &state.splash_list,
        )
    }

    pub(crate) fn wind_direction(&self) -> i32 {
        self.native_type_construction_trace
            .registry_state()
            .rules_wind_direction
    }

    /// ReadGeneral670DE9/670E30/670E57, current scalar defaults across
    /// layers and Type resets (original gamemd).
    pub(crate) fn repair_service_inputs(&self) -> (i32, f64, f64) {
        let state = self.native_type_construction_trace.registry_state();
        (
            state.rules_infantry_repair_step,
            f64::from_bits(state.rules_unit_repair_rate.bits()),
            f64::from_bits(state.rules_infantry_repair_rate.bits()),
        )
    }

    pub(crate) fn gravity(&self) -> i32 {
        self.native_type_construction_trace
            .registry_state()
            .rules_gravity
    }

    pub(crate) fn detail(&self) -> DetailRules {
        self.native_type_construction_trace
            .registry_state()
            .rules_detail
    }

    /// Rules+1668/+1670 after the reached AudioVisual passes, including cold
    /// startup and Type reset. Retain raw literal/default and authored f32-
    /// widened doubles; only the consuming projection truncates to frames.
    /// Original controls: trigger_map_lighting.json rate_controls (+meta).
    pub(crate) fn ambient_change_inputs(&self) -> (NativeF64Bits, NativeF64Bits) {
        let registry = self.native_type_construction_trace.registry_state();
        (
            registry.rules_ambient_change_rate,
            registry.rules_ambient_change_step,
        )
    }

    /// Final Difficulty readers after the ordered Process passes. Keeping the
    /// typed rows preserves literal f64 defaults separately from ReadDouble's
    /// widened f32 authored values (ReadDifficulty @ 0x0066D270).
    pub(crate) fn difficulty_rows(&self) -> &[crate::rules::ruleset::DifficultyRules; 3] {
        &self
            .native_type_construction_trace
            .registry_state()
            .rules_difficulty_rows
    }

    /// Country+C0 after each reached HouseType ReadINI51193D. The typed
    /// value preserves catalog availability at the actual read, unlike final
    /// projected Color strings resolved against later additions.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn country_color_scheme(&self, country: &str) -> Option<i32> {
        self.native_type_construction_trace
            .registry_state()
            .families
            .get(&RulesTypeFamily::Country)?
            .iter()
            .find(|member| member.native_stored_id.eq_ignore_ascii_case(country))
            .map(|member| member.country_color_scheme)
    }

    /// Ordered live Country identities, Color+C0 and Side+BC. Side672440
    /// writes BC before HouseType511850 can override it in ReadTypeData.
    /// A constructor-only Country retains BC=-1, not an invented side.
    pub(crate) fn country_projection(&self) -> impl Iterator<Item = (&str, &IniSection, i32, i32)> {
        self.native_type_construction_trace
            .registry_state()
            .country_projection()
    }

    pub(crate) fn side_registry_names(&self) -> impl Iterator<Item = &str> {
        self.native_type_construction_trace
            .registry_state()
            .families
            .get(&RulesTypeFamily::Side)
            .into_iter()
            .flatten()
            .map(|member| member.native_stored_id.as_str())
    }

    pub(crate) fn prism_support(&self) -> PrismSupportRules {
        self.native_type_construction_trace
            .registry_state()
            .rules_prism_support
    }

    /// ReadAI67399E..6739C5's retained Rules+1150 after all reached passes.
    pub(crate) fn patrol_scan_minutes(&self) -> NativeF64Bits {
        self.native_type_construction_trace
            .registry_state()
            .rules_patrol_scan_minutes
    }

    /// Rules+147C/+14BC after ordered General/AudioVisual reads. Original
    /// ctor665650 and read blocks66E5D5/66B385 establish null/1024 defaults,
    /// empty retention and exact-case keys; the existing factory owns identity.
    pub(crate) fn drop_zone_controls(&self) -> (Option<&str>, i32) {
        let state = self.native_type_construction_trace.registry_state();
        (
            state.rules_drop_zone_anim.as_deref(),
            state.rules_drop_zone_radius,
        )
    }

    /// `[General] Behind=` (Rules `+0xB8`) after the ordered General reads
    /// (`0x0066E4F5..0x0066E513`): the constructor's null (`0x00665765`),
    /// empty retention and the AnimType factory's identity.
    pub(crate) fn behind(&self) -> Option<&str> {
        self.native_type_construction_trace
            .registry_state()
            .rules_behind
            .as_deref()
    }

    /// `[General] PrismType=` (Rules `+0x498`), the stored ID of its type.
    pub(crate) fn prism_type(&self) -> Option<&str> {
        self.native_type_construction_trace
            .registry_state()
            .rules_prism_type
            .as_deref()
    }

    /// `[General] PrerequisiteProcAlternate=` (Rules `+0x400`), the stored ID
    /// of its UnitType.
    pub(crate) fn prerequisite_proc_alternate(&self) -> Option<&str> {
        self.native_type_construction_trace
            .registry_state()
            .rules_prerequisite_proc_alternate
            .as_deref()
    }

    /// The six `[General] Prerequisite*` lists after the last pass, in
    /// [`PrerequisiteGroup::ALL`] order.
    pub(crate) fn prerequisite_lists(&self) -> &[Vec<Prerequisite>; 6] {
        &self
            .native_type_construction_trace
            .registry_state()
            .rules_prerequisite_lists
    }

    /// Each TechnoType's `Prerequisite=` and `PrerequisiteOverride=` after
    /// the last pass that read them.
    pub(crate) fn prerequisite_states(
        &self,
    ) -> impl Iterator<Item = (ObjectCategory, &str, &[Prerequisite], &[Prerequisite])> {
        self.native_type_construction_trace
            .registry_state()
            .families
            .iter()
            .filter_map(|(family, members)| {
                family.object_category().map(|category| (category, members))
            })
            .flat_map(|(category, members)| {
                members.iter().map(move |member| {
                    (
                        category,
                        member.native_stored_id.as_str(),
                        member.prerequisite.as_slice(),
                        member.prerequisite_override.as_slice(),
                    )
                })
            })
    }

    /// `[General]` gates, WallTower and power plants (Rules `+0x86C..+0x87C`,
    /// `+0x89C..+0x8A8`), stored IDs.
    pub(crate) fn building_types(&self) -> &GeneralBuildingTypes {
        &self
            .native_type_construction_trace
            .registry_state()
            .rules_building_types
    }

    /// Retained ColorAdd prefix/table and AudioVisual indexes, not a text merge.
    pub(crate) fn color_add_state(&self) -> (&ColorAddTable, ColorAddSelectors) {
        let state = self.native_type_construction_trace.registry_state();
        (&state.rules_color_add, state.rules_color_add_selectors)
    }

    pub(crate) fn projectile_rule_controls(&self) -> (f64, i32, [u8; 3]) {
        let state = self.native_type_construction_trace.registry_state();
        (
            state.rules_missile_rot_var,
            state.rules_safety_altitude,
            state.rules_line_trail_override,
        )
    }

    /// Rules+310, constructor665ABE=15; CombatDamage66C914..66C92E.
    pub(crate) fn mind_control_attack_line_frames(&self) -> i32 {
        self.native_type_construction_trace
            .registry_state()
            .rules_mind_control_attack_line_frames
    }

    /// Rules+1866, retained by AudioVisual66B7A7..66B7D2 through the
    /// current-RGB default of ReadColorRGB474B50.
    pub(crate) fn chrono_beam_color(&self) -> [u8; 3] {
        self.native_type_construction_trace
            .registry_state()
            .rules_chrono_beam_color
    }

    /// RulesClass's retained ObjectAI fall-rate limits, with and without a
    /// parachute respectively (`+0x7B8`, `+0x7BC`).
    pub(crate) fn fall_rate_controls(&self) -> (i32, i32) {
        let state = self.native_type_construction_trace.registry_state();
        (
            state.rules_parachute_max_fall_rate,
            state.rules_no_parachute_max_fall_rate,
        )
    }

    /// Process-resident Rules+14F8/+14FC/+1500, Allied/Soviet/Third order.
    pub(crate) fn survivor_divisors(&self) -> [i32; 3] {
        self.native_type_construction_trace
            .registry_state()
            .rules_survivor_divisors
    }

    /// Retained AI.CompEasyBonus at Rules+17E3 (ReadAI673D18).
    pub(crate) fn comp_easy_bonus(&self) -> bool {
        self.native_type_construction_trace
            .registry_state()
            .rules_comp_easy_bonus
    }

    /// Retained General.MaximumCheerRate at Rules+580 (ReadGeneral66EBD4).
    pub(crate) fn maximum_cheer_rate(&self) -> i32 {
        self.native_type_construction_trace
            .registry_state()
            .rules_maximum_cheer_rate
    }

    /// Rules+1800, ctor6677A3=300, General671E3B..671E81.
    pub(crate) fn talk_bubble_time(&self) -> i32 {
        self.native_type_construction_trace
            .registry_state()
            .rules_talk_bubble_time
    }

    pub(crate) fn spotlight(&self) -> crate::rules::ruleset::SpotlightRules {
        self.native_type_construction_trace
            .registry_state()
            .rules_spotlight
    }

    /// Rules+490, +298, +29C, +FF0, retained across ordered Process calls.
    pub(crate) fn ion_blast_inputs(&self) -> (i32, &str, &str, &str) {
        let state = self.native_type_construction_trace.registry_state();
        (
            state.rules_ion_cannon_damage,
            &state.rules_ion_blast_anim,
            &state.rules_ion_beam_anim,
            &state.rules_ion_cannon_warhead,
        )
    }

    /// Consume only the typed-reader compatibility projection and deliberately
    /// discard the native constructor/registry receipt.
    ///
    /// Gameplay-equivalent fresh loads must use
    /// [`Self::into_ini_and_native_type_construction_trace`] instead.
    pub(crate) fn into_projection_discarding_native_receipt(self) -> IniFile {
        self.ini
    }

    pub fn content_hash(&self) -> u64 {
        self.content_hash
    }

    pub fn crate_rules(&self) -> &CrateRules {
        &self
            .native_type_construction_trace
            .registry_state()
            .rules_crate
    }

    pub fn powerups(&self) -> &PowerupTable {
        &self.powerups
    }

    /// The three `RocketStruct` blocks and their warheads after every pass.
    pub fn missile_spawn(&self) -> &MissileSpawnRules {
        &self
            .native_type_construction_trace
            .registry_state()
            .rules_missile_spawn
    }

    fn general_type_lists(&self) -> &GeneralTypeLists {
        &self
            .native_type_construction_trace
            .registry_state()
            .rules_general_type_lists
    }

    pub(crate) fn metallic_debris(&self) -> &[String] {
        &self.general_type_lists().metallic_debris
    }

    pub(crate) fn bridge_explosions(&self) -> &[String] {
        &self.general_type_lists().bridge_explosions
    }

    pub(crate) fn weather_con_clouds(&self) -> &[String] {
        &self.general_type_lists().weather_con_clouds
    }

    pub(crate) fn weather_con_bolts(&self) -> &[String] {
        &self.general_type_lists().weather_con_bolts
    }

    /// The four `*ParaDropInf=` lists as stored type IDs: American, Allied,
    /// Soviet, Yuri.
    pub(crate) fn paradrop_infantry(&self) -> &[Vec<String>; 4] {
        &self.general_type_lists().paradrop_infantry
    }

    pub(crate) fn anim_to_infantry(&self) -> &[String] {
        &self.general_type_lists().anim_to_infantry
    }

    pub(crate) fn default_mirage_disguises(&self) -> &[String] {
        &self.general_type_lists().default_mirage_disguises
    }

    pub(crate) fn infantry_disguises(&self) -> &[Option<String>; 3] {
        &self.general_type_lists().infantry_disguises
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn native_type_construction_trace(&self) -> &NativeTypeConstructionTrace {
        &self.native_type_construction_trace
    }

    pub(crate) fn into_ini_and_native_type_construction_trace(
        self,
    ) -> (IniFile, NativeTypeConstructionTrace) {
        (self.ini, self.native_type_construction_trace)
    }
}

/// One active-YR Type constructor family whose constructor calls
/// `AbstractClass::Create_ID @ 0x00410230`.
///
/// `ParticleTypeClass` is deliberately absent: its constructor has no Create_ID
/// call. Script/Team/TaskForce/Trigger/Tag/Tiberium types are absent for the
/// same reason. The family label records the native constructor, not the INI
/// section spelling (`Countries` constructs `HouseType`, for example).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeTypeConstructorFamily {
    HouseType,
    Side,
    OverlayType,
    SuperWeaponType,
    WarheadType,
    SmudgeType,
    TerrainType,
    BuildingType,
    UnitType,
    AircraftType,
    InfantryType,
    AnimType,
    VoxelAnimType,
    ParticleSystemType,
    WeaponType,
    BulletType,
}

/// One successful Type construction in native process order.
///
/// This is normally a first-new-name event, but inputs longer than the native
/// 24-byte stored ID can repeatedly miss lookup and emit duplicate stored IDs.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct NativeTypeConstructionEvent {
    family: NativeTypeConstructorFamily,
    native_stored_id: String,
}

impl NativeTypeConstructionEvent {
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn family(&self) -> NativeTypeConstructorFamily {
        self.family
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn native_stored_id(&self) -> &str {
        &self.native_stored_id
    }
}

/// Move-only receipt for the exact Type-ID prefix emitted by an ordered Rules
/// stack, plus the allocated SuperWeaponType count needed by later House
/// constructor blocks.
///
/// This is chronology, not a final-registry recount. It must therefore travel
/// with the processed Rules result and may not be cloned into a second prefix
/// authority.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct NativeTypeConstructionTrace {
    #[serde(skip)]
    events: Vec<NativeTypeConstructionEvent>,
    allocated_super_weapon_type_count: usize,
    registry_state: NativeRulesRegistryState,
}

impl NativeTypeConstructionTrace {
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn events(&self) -> &[NativeTypeConstructionEvent] {
        &self.events
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn event_count(&self) -> usize {
        self.events.len()
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn allocated_super_weapon_type_count(&self) -> usize {
        self.allocated_super_weapon_type_count
    }

    pub(crate) fn prepend_events(&mut self, mut earlier: Vec<NativeTypeConstructionEvent>) {
        earlier.append(&mut self.events);
        self.events = earlier;
    }

    pub(crate) fn registry_state(&self) -> &NativeRulesRegistryState {
        &self.registry_state
    }

    pub(crate) fn into_parts(
        self,
    ) -> (
        Vec<NativeTypeConstructionEvent>,
        usize,
        NativeRulesRegistryState,
    ) {
        (
            self.events,
            self.allocated_super_weapon_type_count,
            self.registry_state,
        )
    }

    /// Drain constructor history at a native numeric-ID reset while retaining
    /// the one live Type-registry authority for the next lookup pass.
    ///
    /// Cold startup events predate Full_Init's `Clear_Scene` reset. They affect
    /// `E_multi` duplicate suppression but must not be charged to the fresh
    /// Scenario cursor.
    pub(crate) fn into_registry_state_discarding_events(self) -> NativeRulesRegistryState {
        self.registry_state
    }
}

/// Process-resident live Type registries after one or more Rules passes.
///
/// The vectors are ordered native stored IDs plus the bodies that have actually
/// been read so far. The receipt is deliberately move-only: preview, Start, and
/// fresh Full_Init must hand off one authority instead of recounting a merged
/// INI. Tiberium slots are included even though their constructors spend no ID.
/// RulesClass scalars share this process lifetime and survive Type resets.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct NativeRulesRegistryState {
    /// Build capability is retained with the Rules owner, never selected by INI.
    compatibility: RulesCompatibility,
    /// Ares Clear/AddDefaults rebuilds these at a destructive scenario reset;
    /// ordinary Process passes and saves retain the reached list reads.
    ares_prerequisites: Option<GenericPrerequisites>,
    /// Ordered Ares armor registry; Process retains it, destructive reset rebuilds it.
    ares_armors: Option<ArmorCatalog>,
    families: HashMap<RulesTypeFamily, Vec<ProcessedType>>,
    /// TechnoTypeClass::Array A8EB04/countA8EB10. Each pair points to the
    /// existing family member; names alone cannot distinguish repeated native
    /// stored IDs allocated from inputs longer than AbstractType's24 bytes.
    /// Controls: tools/spatial_oracle/trigger_object_tech_level.json (+meta).
    techno_type_order: Vec<(RulesTypeFamily, usize)>,
    tiberiums: Vec<ProcessedType>,
    /// The existing Colors66D3A0 find-or-create catalog survives ordinary
    /// Process handoffs. Reset6686E6..668711 deletes and clears it alongside
    /// the Type registries, so destructive_reset leaves this field empty.
    colors: Vec<(String, String)>,
    rules_color_add: ColorAddTable,
    rules_color_add_selectors: ColorAddSelectors,
    rules_detail: DetailRules,
    rules_ambient_change_rate: NativeF64Bits,
    rules_ambient_change_step: NativeF64Bits,
    rules_difficulty_rows: [crate::rules::ruleset::DifficultyRules; 3],
    rules_gravity: i32,
    rules_wind_direction: i32,
    rules_missile_rot_var: f64,
    rules_safety_altitude: i32,
    rules_parachute_max_fall_rate: i32,
    rules_no_parachute_max_fall_rate: i32,
    rules_survivor_divisors: [i32; 3],
    rules_infantry_repair_step: i32,
    rules_unit_repair_rate: NativeF64Bits,
    rules_infantry_repair_rate: NativeF64Bits,
    rules_maximum_cheer_rate: i32,
    rules_talk_bubble_time: i32,
    rules_spotlight: crate::rules::ruleset::SpotlightRules,
    rules_ion_cannon_damage: i32,
    rules_ion_blast_anim: String,
    rules_ion_beam_anim: String,
    rules_ion_cannon_warhead: String,
    rules_mind_control_attack_line_frames: i32,
    rules_line_trail_override: [u8; 3],
    rules_chrono_beam_color: [u8; 3],
    rules_prism_support: PrismSupportRules,
    rules_patrol_scan_minutes: NativeF64Bits,
    rules_comp_easy_bonus: bool,
    rules_prism_type: Option<String>,
    rules_drop_zone_anim: Option<String>,
    rules_drop_zone_radius: i32,
    rules_behind: Option<String>,
    rules_prerequisite_proc_alternate: Option<String>,
    /// The six `[General] Prerequisite*` lists (Rules `+0x358..+0x3E4`), in
    /// [`PrerequisiteGroup::ALL`] order.
    rules_prerequisite_lists: [Vec<Prerequisite>; 6],
    rules_building_types: GeneralBuildingTypes,
    select_anim: SelectAnimRulesState,
    /// `[CombatDamage] DeathWeapon=` (Rules+FDC), the stored ID of its
    /// WeaponType: constructor null (`0x00666BB2`); ReadCombatDamage
    /// (`0x0066C57E..0x0066C5A8`) keeps it on an empty ReadString128 and
    /// otherwise stores the WeaponType factory's answer, null for `none`.
    /// The destructive reset clears it with the other Type references.
    rules_death_weapon: Option<String>,
    rules_type_references: RulesTypeReferences,
    /// `[CrateRules]` (`0x0066B900`): its scalars, three images and
    /// UnitCrateType.
    rules_crate: CrateRules,
    /// The three `RocketStruct` blocks with their types and warheads.
    rules_missile_spawn: MissileSpawnRules,
    rules_general_type_lists: GeneralTypeLists,
}

impl Default for NativeRulesRegistryState {
    fn default() -> Self {
        // RulesClass constructor values; `GeneralRules::default` owns them.
        let general = crate::rules::ruleset::GeneralRules::default();
        Self {
            compatibility: RulesCompatibility::default(),
            ares_prerequisites: crate::rules::ARES_ENABLED.then(GenericPrerequisites::default),
            ares_armors: crate::rules::ARES_ENABLED.then(ArmorCatalog::default),
            families: HashMap::new(),
            techno_type_order: Vec::new(),
            tiberiums: Vec::new(),
            colors: Vec::new(),
            rules_color_add: ColorAddTable::default(),
            rules_color_add_selectors: ColorAddSelectors::default(),
            rules_detail: DetailRules::default(),
            rules_ambient_change_rate: crate::rules::ruleset::DEFAULT_AMBIENT_CHANGE_RATE,
            rules_ambient_change_step: crate::rules::ruleset::DEFAULT_AMBIENT_CHANGE_STEP,
            rules_difficulty_rows: general.difficulty_rows,
            rules_gravity: general.gravity,
            rules_wind_direction: general.wind_direction(),
            rules_missile_rot_var: general.missile_rot_var,
            rules_safety_altitude: general.safety_altitude,
            rules_parachute_max_fall_rate: general.parachute_max_fall_rate,
            rules_no_parachute_max_fall_rate: general.no_parachute_max_fall_rate,
            rules_survivor_divisors: crate::rules::ruleset::DEFAULT_SURVIVOR_DIVISORS,
            rules_infantry_repair_step: general.infantry_repair_step,
            rules_unit_repair_rate: NativeF64Bits::from_bits(general.unit_repair_rate.to_bits()),
            rules_infantry_repair_rate: NativeF64Bits::from_bits(
                general.infantry_repair_rate.to_bits(),
            ),
            rules_maximum_cheer_rate: crate::rules::ruleset::DEFAULT_MAXIMUM_CHEER_RATE,
            rules_talk_bubble_time: general.talk_bubble_time,
            rules_spotlight: general.spotlight,
            // Rules665CD4=700; pointer fields665A18/665A24/666BD5 start null.
            rules_ion_cannon_damage: 700,
            rules_ion_blast_anim: String::new(),
            rules_ion_beam_anim: String::new(),
            rules_ion_cannon_warhead: String::new(),
            // MindControlAttackLineFrames, Rules+310: constructor 0x00665ABE.
            rules_mind_control_attack_line_frames: 15,
            rules_line_trail_override: general.line_trail_color_override,
            rules_chrono_beam_color: general.chrono_beam_color,
            rules_prism_support: PrismSupportRules::default(),
            rules_patrol_scan_minutes: general.patrol_scan_minutes,
            rules_comp_easy_bonus: general.comp_easy_bonus,
            rules_prism_type: None,
            rules_drop_zone_anim: None,
            rules_drop_zone_radius: general.drop_zone_radius,
            rules_behind: None,
            rules_prerequisite_proc_alternate: None,
            rules_prerequisite_lists: Default::default(),
            rules_building_types: GeneralBuildingTypes::default(),
            select_anim: SelectAnimRulesState::default(),
            rules_death_weapon: None,
            rules_type_references: RulesTypeReferences::default(),
            rules_crate: CrateRules::default(),
            rules_missile_spawn: MissileSpawnRules::default(),
            rules_general_type_lists: GeneralTypeLists::default(),
        }
    }
}

impl NativeRulesRegistryState {
    pub(crate) fn country_projection(&self) -> impl Iterator<Item = (&str, &IniSection, i32, i32)> {
        self.families
            .get(&RulesTypeFamily::Country)
            .into_iter()
            .flatten()
            .map(|member| {
                (
                    member.native_stored_id.as_str(),
                    &member.body,
                    member.country_color_scheme,
                    member.country_side_index,
                )
            })
    }

    pub(crate) fn side_projection_names(&self) -> impl Iterator<Item = &str> {
        self.families
            .get(&RulesTypeFamily::Side)
            .into_iter()
            .flatten()
            .map(|member| member.native_stored_id.as_str())
    }

    fn validate_techno_type_order(&self) -> Result<(), RulesError> {
        let expected = self
            .families
            .iter()
            .filter(|(family, _)| family.object_category().is_some())
            .map(|(_, members)| members.len())
            .sum::<usize>();
        let mut seen = HashSet::with_capacity(expected);
        if self.techno_type_order.len() != expected
            || self.techno_type_order.iter().any(|&(family, index)| {
                family.object_category().is_none()
                    || self
                        .families
                        .get(&family)
                        .is_none_or(|members| index >= members.len())
                    || !seen.insert((family, index))
            })
        {
            return Err(RulesError::InvalidSavedState(
                "global TechnoType order does not identify each live type exactly once",
            ));
        }
        Ok(())
    }

    fn validate_prerequisites(&self) -> Result<(), RulesError> {
        if self.ares_prerequisites.is_some() != crate::rules::ARES_ENABLED {
            return Err(RulesError::InvalidSavedState(
                "saved prerequisite catalog does not match the compiled capability",
            ));
        }
        let building_count = self
            .families
            .get(&RulesTypeFamily::Building)
            .map_or(0, Vec::len);
        let valid_entry = |entry: &Prerequisite| match *entry {
            Prerequisite::Building(index) => {
                usize::try_from(index).is_ok_and(|index| index < building_count)
            }
            Prerequisite::Group(_) => true,
            Prerequisite::GenericGroup(index) => {
                index >= 6
                    && self
                        .ares_prerequisites
                        .as_ref()
                        .is_some_and(|catalog| (index as usize) < catalog.groups().len())
            }
        };
        if self.families.values().flatten().any(|member| {
            member
                .prerequisite
                .iter()
                .chain(&member.prerequisite_override)
                .any(|entry| !valid_entry(entry))
        }) {
            return Err(RulesError::InvalidSavedState(
                "prerequisite reference is outside its registry",
            ));
        }
        if let Some(catalog) = &self.ares_prerequisites {
            if PrerequisiteGroup::ALL.into_iter().any(|group| {
                catalog
                    .group(group.index())
                    .is_none_or(|entry| entry.name() != group.token())
            }) || catalog.groups().iter().any(|group| {
                group.members().iter().any(|entry| !valid_entry(entry))
                    || group.alternates().iter().any(|alternate| {
                        !self.techno_type_states().any(|(category, id, _)| {
                            category == alternate.category() && id == alternate.id()
                        })
                    })
            }) {
                return Err(RulesError::InvalidSavedState(
                    "invalid retained generic prerequisite catalog",
                ));
            }
        }
        Ok(())
    }

    fn techno_type_states(&self) -> impl Iterator<Item = (ObjectCategory, &str, i32)> {
        self.techno_type_order.iter().map(|&(family, index)| {
            let member = &self.families[&family][index];
            (
                family
                    .object_category()
                    .expect("global member is a TechnoType"),
                member.native_stored_id.as_str(),
                member.tech_level,
            )
        })
    }

    /// Transfer runtime values back to the process registry only after the
    /// outgoing Simulation has retired. Prefix Process714570 reads the same
    /// current field before Reset6686C0 deletes the old Type generation.
    /// Validate the complete transfer before changing any field. A category
    /// with duplicate exact stored IDs cannot represent a unique live object.
    pub(crate) fn consume_outgoing_type_tech_levels(
        &mut self,
        updates: impl IntoIterator<Item = (ObjectCategory, String, i32)>,
    ) -> Result<(), RulesError> {
        let mut resolved = Vec::new();
        for (category, id, value) in updates {
            let family = RulesTypeFamily::from_object_category(category);
            let index = self
                .exact_type_index(family, &id)
                .ok_or(RulesError::InvalidSavedState(
                    "outgoing TechLevel does not identify one exact live TechnoType",
                ))?;
            resolved.push((family, index, value));
        }
        for (family, index, value) in resolved {
            self.families.get_mut(&family).expect("validated family")[index].tech_level = value;
        }
        Ok(())
    }

    fn exact_type_index(&self, family: RulesTypeFamily, id: &str) -> Option<usize> {
        let mut matching = self
            .families
            .get(&family)?
            .iter()
            .enumerate()
            .filter(|(_, member)| member.native_stored_id == id);
        let index = matching.next()?.0;
        matching.next().is_none().then_some(index)
    }

    /// One retirement transaction for current Type fields. Validate upgrade
    /// identities/slots first, then use TechLevel's existing atomic owner;
    /// neither family of writes commits when either transfer is invalid.
    pub(crate) fn consume_outgoing_type_runtime_fields(
        &mut self,
        tech_levels: impl IntoIterator<Item = (ObjectCategory, String, i32)>,
        upgrade_art_names: impl IntoIterator<Item = (String, u8, String)>,
    ) -> Result<(), RulesError> {
        let mut resolved = Vec::new();
        for (id, slot, name) in upgrade_art_names {
            let index = (slot < 3)
                .then(|| self.exact_type_index(RulesTypeFamily::Building, &id))
                .flatten()
                .ok_or(RulesError::InvalidSavedState(
                    "outgoing PowerUp animation does not identify one exact live BuildingType slot",
                ))?;
            resolved.push((index, slot, name));
        }
        self.consume_outgoing_type_tech_levels(tech_levels)?;
        for (index, slot, name) in resolved {
            self.families
                .get_mut(&RulesTypeFamily::Building)
                .expect("validated BuildingType family")[index]
                .building_upgrade
                .set_normal_art_name(slot, name);
        }
        Ok(())
    }

    /// Runtime Execute72/71 writes become retained defaults for later
    /// AudioVisual reads. Reset6686C0 deletes Types, not these Rules scalars.
    pub(crate) fn consume_outgoing_ambient_change_inputs(
        &mut self,
        inputs: [Option<NativeF64Bits>; 2],
    ) {
        if let Some(rate) = inputs[0] {
            self.rules_ambient_change_rate = rate;
        }
        if let Some(step) = inputs[1] {
            self.rules_ambient_change_step = step;
        }
    }

    pub(crate) fn anim_type_art_read_states(&self) -> impl Iterator<Item = (&str, bool)> {
        self.families
            .get(&RulesTypeFamily::Animation)
            .into_iter()
            .flatten()
            .map(|member| (member.native_stored_id.as_str(), member.anim_art_read))
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn side_country_members(&self, side: &str) -> Option<&[i32]> {
        self.side_registry_states()
            .find_map(|(id, members)| id.eq_ignore_ascii_case(side).then_some(members))
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn side_registry_states(&self) -> impl Iterator<Item = (&str, &[i32])> {
        self.families
            .get(&RulesTypeFamily::Side)
            .into_iter()
            .flatten()
            .map(|member| {
                (
                    member.native_stored_id.as_str(),
                    member.side_country_members.as_slice(),
                )
            })
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn family_len(&self, family: NativeTypeConstructorFamily) -> usize {
        self.families
            .iter()
            .find_map(|(rules_family, members)| {
                (rules_family.native_constructor_family() == Some(family)).then_some(members.len())
            })
            .unwrap_or(0)
    }

    /// Supply the exact Rules+1150 prior of a bounded native ReadAI control.
    /// This is fixture state, not a second reader or a production writer.
    /// Native controls: tools/team_recruit_oracle.py --patrol.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn with_patrol_scan_minutes_for_test(mut self, minutes: NativeF64Bits) -> Self {
        self.rules_patrol_scan_minutes = minutes;
        self
    }

    /// Consume the pre-reset registry owner at Full_Init's destructive Rules
    /// Type-deletion phase and return an owner with empty Type registries.
    /// RulesClass itself survives6686C0, retaining Gravity, detail thresholds
    /// and its scalar values for the first root Process. The ColorAdd clear
    /// and root reread occur later in RulesLayerStack's ScenarioReset context.
    ///
    /// Original reset retires every referenced Type: LightningWarhead detaches
    /// to null, while these Rules Anim references/list retain freed addresses.
    /// Rust clears all affected references at deletion rather than rebinding
    /// retired Types by name. This is a deterministic safety policy for native
    /// stale-pointer state; authored post-reset Process reads rebuild bindings.
    /// Evidence: tools/rules_oracle/select_anim_inputs.md. Every RulesClass
    /// Type pointer and pointer list follows it, the crate images,
    /// UnitCrateType and the rocket types and warheads included, while the
    /// crate and rocket scalars stay with the others.
    ///
    /// Numeric-ID history is intentionally not represented here and therefore
    /// cannot be rewound by this operation.
    pub(crate) fn destructive_reset(self) -> Self {
        Self {
            rules_color_add: self.rules_color_add,
            rules_color_add_selectors: self.rules_color_add_selectors,
            rules_detail: self.rules_detail,
            rules_ambient_change_rate: self.rules_ambient_change_rate,
            rules_ambient_change_step: self.rules_ambient_change_step,
            rules_difficulty_rows: self.rules_difficulty_rows,
            rules_gravity: self.rules_gravity,
            rules_drop_zone_radius: self.rules_drop_zone_radius,
            rules_wind_direction: self.rules_wind_direction,
            rules_missile_rot_var: self.rules_missile_rot_var,
            rules_safety_altitude: self.rules_safety_altitude,
            rules_parachute_max_fall_rate: self.rules_parachute_max_fall_rate,
            rules_no_parachute_max_fall_rate: self.rules_no_parachute_max_fall_rate,
            rules_survivor_divisors: self.rules_survivor_divisors,
            rules_infantry_repair_step: self.rules_infantry_repair_step,
            rules_unit_repair_rate: self.rules_unit_repair_rate,
            rules_infantry_repair_rate: self.rules_infantry_repair_rate,
            rules_maximum_cheer_rate: self.rules_maximum_cheer_rate,
            rules_talk_bubble_time: self.rules_talk_bubble_time,
            rules_spotlight: self.rules_spotlight,
            rules_ion_cannon_damage: self.rules_ion_cannon_damage,
            rules_mind_control_attack_line_frames: self.rules_mind_control_attack_line_frames,
            rules_line_trail_override: self.rules_line_trail_override,
            rules_chrono_beam_color: self.rules_chrono_beam_color,
            rules_prism_support: self.rules_prism_support,
            rules_patrol_scan_minutes: self.rules_patrol_scan_minutes,
            rules_comp_easy_bonus: self.rules_comp_easy_bonus,
            // Plain index vectors, so RulesClass keeps them; the next pass
            // reads them as its default.
            rules_prerequisite_lists: self.rules_prerequisite_lists,
            rules_crate: self.rules_crate.without_type_references(),
            rules_missile_spawn: self.rules_missile_spawn.without_type_references(),
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
enum RulesTypeFamily {
    Country,
    Side,
    Overlay,
    SuperWeapon,
    Warhead,
    Smudge,
    Terrain,
    Building,
    Vehicle,
    Aircraft,
    Infantry,
    Animation,
    VoxelAnimation,
    Particle,
    ParticleSystem,
    Weapon,
    Projectile,
}

/// Explicit registry order in `RulesClass::Process @ 0x00668BF0`.
///
/// `Sides` is special: `FUN_00672440` passes the entry name to
/// `SideClass::Constructor @ 0x006A4550`; all other rows read a 32-byte value.
const EXPLICIT_RULE_TYPE_FAMILIES: &[(&str, RulesTypeFamily)] = &[
    ("Countries", RulesTypeFamily::Country),
    ("Sides", RulesTypeFamily::Side),
    ("OverlayTypes", RulesTypeFamily::Overlay),
    ("SuperWeaponTypes", RulesTypeFamily::SuperWeapon),
    ("Warheads", RulesTypeFamily::Warhead),
    ("SmudgeTypes", RulesTypeFamily::Smudge),
    ("TerrainTypes", RulesTypeFamily::Terrain),
    ("BuildingTypes", RulesTypeFamily::Building),
    ("VehicleTypes", RulesTypeFamily::Vehicle),
    ("AircraftTypes", RulesTypeFamily::Aircraft),
    ("InfantryTypes", RulesTypeFamily::Infantry),
    ("Animations", RulesTypeFamily::Animation),
    ("VoxelAnims", RulesTypeFamily::VoxelAnimation),
    ("Particles", RulesTypeFamily::Particle),
    ("ParticleSystems", RulesTypeFamily::ParticleSystem),
];

/// Families rebuilt into the compatibility INI projection. `Sides` remains
/// an ordinary section there because its values are country membership lists,
/// not an index-to-Type registry.
const PROJECTED_RULE_TYPE_FAMILIES: &[(&str, RulesTypeFamily)] = &[
    ("Countries", RulesTypeFamily::Country),
    ("OverlayTypes", RulesTypeFamily::Overlay),
    ("SuperWeaponTypes", RulesTypeFamily::SuperWeapon),
    ("Warheads", RulesTypeFamily::Warhead),
    ("SmudgeTypes", RulesTypeFamily::Smudge),
    ("TerrainTypes", RulesTypeFamily::Terrain),
    ("BuildingTypes", RulesTypeFamily::Building),
    ("VehicleTypes", RulesTypeFamily::Vehicle),
    ("AircraftTypes", RulesTypeFamily::Aircraft),
    ("InfantryTypes", RulesTypeFamily::Infantry),
    ("Animations", RulesTypeFamily::Animation),
    ("VoxelAnims", RulesTypeFamily::VoxelAnimation),
    ("Particles", RulesTypeFamily::Particle),
    ("ParticleSystems", RulesTypeFamily::ParticleSystem),
];

impl RulesTypeFamily {
    fn from_object_category(category: ObjectCategory) -> Self {
        match category {
            ObjectCategory::Building => Self::Building,
            ObjectCategory::Aircraft => Self::Aircraft,
            ObjectCategory::Vehicle => Self::Vehicle,
            ObjectCategory::Infantry => Self::Infantry,
        }
    }

    fn object_category(self) -> Option<ObjectCategory> {
        match self {
            Self::Building => Some(ObjectCategory::Building),
            Self::Aircraft => Some(ObjectCategory::Aircraft),
            Self::Vehicle => Some(ObjectCategory::Vehicle),
            Self::Infantry => Some(ObjectCategory::Infantry),
            _ => None,
        }
    }

    fn native_constructor_family(self) -> Option<NativeTypeConstructorFamily> {
        Some(match self {
            Self::Country => NativeTypeConstructorFamily::HouseType,
            Self::Side => NativeTypeConstructorFamily::Side,
            Self::Overlay => NativeTypeConstructorFamily::OverlayType,
            Self::SuperWeapon => NativeTypeConstructorFamily::SuperWeaponType,
            Self::Warhead => NativeTypeConstructorFamily::WarheadType,
            Self::Smudge => NativeTypeConstructorFamily::SmudgeType,
            Self::Terrain => NativeTypeConstructorFamily::TerrainType,
            Self::Building => NativeTypeConstructorFamily::BuildingType,
            Self::Vehicle => NativeTypeConstructorFamily::UnitType,
            Self::Aircraft => NativeTypeConstructorFamily::AircraftType,
            Self::Infantry => NativeTypeConstructorFamily::InfantryType,
            Self::Animation => NativeTypeConstructorFamily::AnimType,
            Self::VoxelAnimation => NativeTypeConstructorFamily::VoxelAnimType,
            Self::Particle => return None,
            Self::ParticleSystem => NativeTypeConstructorFamily::ParticleSystemType,
            Self::Weapon => NativeTypeConstructorFamily::WeaponType,
            Self::Projectile => NativeTypeConstructorFamily::BulletType,
        })
    }
}

/// Resolved names preserve native factory spelling and vector order. These
/// pointer bindings belong to the retained Rules object, not merged INI text.
/// Native reader corpus: tools/rules_oracle/select_anim_inputs.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct SelectAnimRulesState {
    lightning_warhead: String,
    weather_con_bolt_explosion: String,
    weapon_nullify_anim: String,
    splash_list: Vec<String>,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
struct WarheadAnimReadState {
    conventional: bool,
    em_effect: bool,
    anim_list: Vec<String>,
    /// `DebrisTypes=` (+18C, `0x0075DAF5`): ReadString128, then each `strtok`
    /// token's non-null VoxelAnimType replaces the list.
    debris_types: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct ProcessedType {
    native_stored_id: String,
    body: IniSection,
    /// Constructor711351=0; fixed ART ReadInt71592A..715948 keeps the
    /// current value as default across Image changes and later Rules passes.
    #[serde(default)]
    voxel_shadow_index: i32,
    /// TechnoType ctor711082 initializes+634 to255; ReadInt714570 uses
    /// its current value, including a prior TAction106 write, as the default.
    /// Native controls: trigger_object_tech_level.json scalar_layers/lifecycle.
    tech_level: i32,
    /// HouseType ctor51141C starts Color+C0 at0; each reached ReadINI
    /// passes the current index to the shared ReadColor474A90.
    country_color_scheme: i32,
    /// HouseType ctor511416 writes-1. Side67259A and the late Side reader
    /// 51209E/4756F0 update this binding on the same retained Country.
    country_side_index: i32,
    /// Side+9C ordered Country indexes. ReadHouses4767C0 retains the prior
    /// vector on count0 and otherwise replaces it without deduplication.
    /// Signed entries preserve `<random>`=-2 before the full-reader boundary.
    side_country_members: Vec<i32>,
    /// False until the native AnimType ART-body boundary has been entered.
    /// Kept on the process-resident type so subsequent Rules passes retain it.
    anim_art_read: bool,
    /// One owner for Bullet's two-phase Image/ART reads across rules passes.
    projectile_art: ProjectileArtState,
    /// BuildingType45DF13 initializes +EF0 to zero; ReadINI461225..46125D
    /// updates it from fixed ART after ObjectType5F933B reads effective Image.
    building_foundation: u8,
    /// Constructor45DEBC..45DEC8 clears BuildingType+EBC/EC0/EC4.
    building_target_coord_offset: [i32; 3],
    /// BuildingType ctor45DFDA=false; ReadBool45FEE5..45FF0D, current default.
    building_has_spotlight: bool,
    building_light: crate::rules::building_light::BuildingLightRules,
    /// BuildingType+E88/+16FC/+14E0, retained by ReadINI460E17/464727.
    building_upgrade: crate::rules::building_upgrade::BuildingUpgradeRules,
    secret_lab: crate::rules::secret_lab::SecretLabRules,
    /// Unit ctor747167..7471B2 and reached fixed-ART reader7477C1..747AAE.
    unit_shp: crate::rules::shp_vehicle_sequence::UnitShpReadState,
    /// Unit7470D7 ctor -1 and7476D3..74770B retained default/read sequence.
    unit_speed_type: SpeedType,
    /// TechnoType ctor71136F/711781, generic flag read71287E and FV pair
    /// reads747BBD..747E90, retained across every reached Rules pass.
    gunner_turrets: GunnerTurrets,
    /// Infantry ctor5236BD/C7 and unconditional ReadPip5240D1..524125.
    /// Missing type sections skip the whole reader; missing keys do not.
    #[serde(default)]
    infantry_pips: InfantryPips,
    #[serde(default)]
    infantry_infiltration: InfantryInfiltration,
    recoil: crate::rules::recoil::RecoilConfig,
    airstrike: crate::rules::airstrike::AirstrikeSettings,
    /// TechnoType constructor711134 and current-default ReadInt7147B4.
    #[serde(default)]
    radial_fire_segments: i32,
    /// Type+3DC, constructor710D28 and retained ReadInt714145..714159.
    lepton_mind_control_offset: i32,
    /// TechnoType `Prerequisite=` (`+0x638`) and `PrerequisiteOverride=`
    /// (`+0x654`), each read with its current list as the default.
    prerequisite: Vec<Prerequisite>,
    prerequisite_override: Vec<Prerequisite>,
    weapon_slots: WeaponSlots,
    techno_references: TechnoTypeReferences,
    weapon: WeaponReadState,
    warhead_anim: WarheadAnimReadState,
    /// Warhead ctor75CEC0 starts every +A0 slot at1.0. Each reached
    /// ReadINI75DDCC..75DE5A uses its literal all-100% missing-key default;
    /// no section leaves this live table alone. The retained registry owns
    /// these bits; RuleSet only projects them. Native execution:
    /// tools/rules_oracle/bridge_landing_inputs.{py,json,meta.json}.
    warhead_verses: [NativeF64Bits; 11],
    /// Independent Ares10051D80 constructor/10006460 read vector.
    ares_verses: Option<AresWarheadVerses>,
    /// ObjectType4753F0 -> Ares10006E10 binds at each reached body read.
    armor_id: ArmorId,
    /// ParticleSystemType+294, the bound ParticleType's index: constructor -1
    /// (`0x006440B3`), rebound by every reached ReadINI (`0x0064430F..0x00644323`).
    holds_what: Option<usize>,
    /// SuperWeaponType+9C `WeaponType=`, the stored ID of its WeaponType:
    /// constructor null (`0x006CE5CB`); ReadINI (`0x006CEA4A..0x006CEA8C`)
    /// keeps it on an empty ReadString128 and otherwise stores
    /// `WeaponTypeClass::FindOrAllocate @ 0x00772FA0`, null for `none`.
    super_weapon_weapon: Option<String>,
    /// SuperWeaponType+C8 `AuxBuilding=` (`0x006CED07..0x006CED2E`), read the
    /// same way through BuildingType's factory (`0x004653C0`).
    super_weapon_aux_building: Option<String>,
    #[serde(default)]
    super_weapon_building_requirements: Option<SuperWeaponBuildingRequirements>,
    /// Ares `SW.Warhead=`, read only for Type=GenericWarhead.
    generic_super_weapon_warhead: Option<String>,
    unit_delivery: Option<UnitDeliveryRules>,
    /// ParticleType `Warhead=` (`0x00645043..0x00645067`): ReadString128,
    /// then WarheadType's factory (`0x0075E3B0`); an empty read keeps it.
    particle_warhead: Option<String>,
    voxel_anim_references: VoxelAnimTypeReferences,
    anim_references: AnimTypeReferences,
    /// BulletType+2B4 `ShrapnelWeapon=`, read the same way by BulletType
    /// ReadINI (`0x0046C2E1..0x0046C31F`) over the constructor's null
    /// (`0x0046BC82`). Its `AirburstWeapon=` (+2B0, `0x0046C2B9`) is
    /// allocated but not retained: VERA has no consumer of the airburst
    /// weapon native loads at `0x00469ECE`.
    shrapnel_weapon: Option<String>,
}

/// The ObjectType `Image` and TechnoType pointer fields each reached ReadINI
/// pass replaces and later passes keep; `RuleSet` publishes them onto its
/// `ObjectType`. Each pointer is a ReadString into 0x80 bytes with an empty
/// default: an empty read keeps the field, and a value goes through its
/// family's FindOrAllocate, which answers null for `none` and `<none>`
/// (WeaponTypes `0x00772FA0`, BuildingTypes `0x004653C0`, UnitTypes
/// `0x007480D0`, AnimTypes `0x00428B80`, ParticleSystemTypes `0x00644890`).
/// Each list is a ReadString into 0x80 bytes whose nonempty read replaces the
/// list with the non-null answers for its `strtok` tokens. The constructors
/// leave every pointer null and every list empty.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct TechnoTypeReferences {
    /// ObjectType+1F8 (`0x005F92F8..0x005F933B`): ReadString into 25 bytes
    /// over a copy of the retained text, which starts as the type's ID. It
    /// names the type's fixed ART section and its asset.
    image: String,
    /// `DeathWeapon=` (+D18): TechnoType ReadINI `0x007122D8..0x00712311`
    /// over the constructor's null (`0x007114AE`).
    death_weapon: Option<String>,
    /// `DebrisTypes=` (`0x007125F1`), VoxelAnimTypes, and `DebrisAnims=`
    /// (`0x0071274F`), AnimTypes.
    debris_types: Vec<String>,
    debris_anims: Vec<String>,
    /// `Dock=` (`0x00713180`), BuildingTypes.
    dock: Vec<String>,
    /// `DeploysInto=` (+404): `0x00713279..0x00713297`, a BuildingType, over
    /// the constructor's null (`0x00710D61`).
    deploys_into: Option<String>,
    /// `UndeploysInto=` (+408, `0x007132B2..0x007132E4`) and `PowersUnit=`
    /// (+40C, `0x007132DF..0x00713309`), UnitTypes.
    undeploys_into: Option<String>,
    powers_unit: Option<String>,
    /// `Explosion=` (`0x0071399A`) and `DestroyAnim=` (`0x00713A97`),
    /// AnimTypes.
    explosion: Vec<String>,
    destroy_anim: Vec<String>,
    /// `NaturalParticleSystem=` (+764, `0x00713B9A..0x00713BD5`) and
    /// `RefinerySmokeParticleSystem=` (+774, `0x00713BCD..0x00713C05`), then the
    /// `DamageParticleSystems=` (`0x00713C2C`) and `DestroyParticleSystems=`
    /// (`0x00713D2C`) lists, all ParticleSystemTypes.
    natural_particle_system: Option<String>,
    refinery_smoke_particle_system: Option<String>,
    damage_particle_systems: Vec<String>,
    destroy_particle_systems: Vec<String>,
    /// `UnloadingClass=` (+6B8, `0x007146E8..0x0071471A`), a UnitType, and
    /// `DeployingAnim=` (+6BC, `0x00714715..0x0071473F`), an AnimType.
    unloading_class: Option<String>,
    deploying_anim: Option<String>,
    /// `Enslaves=` (+D40, `0x00714DD7..0x00714DEC`) through the InfantryType
    /// reader `0x0067BAC0`, and `Spawns=` (+D58, `0x00714E9E..0x00714EB3`)
    /// through the AircraftType reader `0x0067BD30`. Both are the same
    /// ReadString128 and factory.
    enslaves: Option<String>,
    spawns: Option<String>,
    /// `OccupyWeapon=` (+E04) and `EliteOccupyWeapon=` (+E20). Only
    /// InfantryType ReadINI reads them (`0x00524102..0x0052417F`); the
    /// InfantryType constructor nulls both (`0x00523890`, `0x005238B4`).
    occupy_weapon: Option<String>,
    elite_occupy_weapon: Option<String>,
    /// InfantryType `DeadBodies=` (`0x005241F3`), AnimTypes.
    dead_bodies: Vec<String>,
    /// BuildingType `FreeUnit=` (+EA0, `0x00460540..0x0046056D`), a UnitType,
    /// and `ToOverlay=`, which BuildingType ReadINI reads from the fixed ART
    /// section +1F8 names (`0x004611F5..0x0046121F`), an OverlayType.
    free_unit: Option<String>,
    to_overlay: Option<String>,
}

impl TechnoTypeReferences {
    fn new(native_stored_id: &str) -> Self {
        Self {
            image: native_stored_id.to_owned(),
            death_weapon: None,
            debris_types: Vec::new(),
            debris_anims: Vec::new(),
            dock: Vec::new(),
            deploys_into: None,
            undeploys_into: None,
            powers_unit: None,
            explosion: Vec::new(),
            destroy_anim: Vec::new(),
            natural_particle_system: None,
            refinery_smoke_particle_system: None,
            damage_particle_systems: Vec::new(),
            destroy_particle_systems: Vec::new(),
            unloading_class: None,
            deploying_anim: None,
            enslaves: None,
            spawns: None,
            occupy_weapon: None,
            elite_occupy_weapon: None,
            dead_bodies: Vec::new(),
            free_unit: None,
            to_overlay: None,
        }
    }

    /// Publish every retained reference onto the type's `ObjectType`, as
    /// stored IDs.
    pub(crate) fn apply_to(&self, object: &mut crate::rules::object_type::ObjectType) {
        object.image.clone_from(&self.image);
        object.death_weapon.clone_from(&self.death_weapon);
        object.debris_types.clone_from(&self.debris_types);
        object.debris_anims.clone_from(&self.debris_anims);
        object.dock.clone_from(&self.dock);
        object.deploys_into.clone_from(&self.deploys_into);
        object.undeploys_into.clone_from(&self.undeploys_into);
        object.powers_unit.clone_from(&self.powers_unit);
        object.explosion_anims.clone_from(&self.explosion);
        object.destroy_anims.clone_from(&self.destroy_anim);
        object
            .natural_particle_system
            .clone_from(&self.natural_particle_system);
        object
            .refinery_smoke_particle_system
            .clone_from(&self.refinery_smoke_particle_system);
        object
            .damage_particle_systems
            .clone_from(&self.damage_particle_systems);
        object
            .destroy_particle_systems
            .clone_from(&self.destroy_particle_systems);
        object.unloading_class.clone_from(&self.unloading_class);
        object.deploying_anim.clone_from(&self.deploying_anim);
        object.enslaves.clone_from(&self.enslaves);
        object.spawns.clone_from(&self.spawns);
        object.occupy_weapon.clone_from(&self.occupy_weapon);
        object
            .elite_occupy_weapon
            .clone_from(&self.elite_occupy_weapon);
        object.dead_bodies.clone_from(&self.dead_bodies);
        object.free_unit.clone_from(&self.free_unit);
        object.to_overlay.clone_from(&self.to_overlay);
    }
}

/// The Type references `AnimTypeClass::ReadINI` reads from the AnimType's
/// fixed ART section (`0x00428045..0x00428735`), retained on the live type:
/// `RuleSet` hands them to its ART registry. Each is a ReadString into 0x80
/// bytes whose empty read keeps the field; `Next=` (+2C8, `0x00428045`) and
/// `Spawns=` (+34C, `0x004281DE`) go through `AnimTypeClass::FindOrCreate @
/// 0x00428F70`, `BounceAnim=` (`0x0042840D`), `ExpireAnim=` (`0x004284C4`)
/// and `TrailerAnim=` (`0x00428588`) through the inlined AnimType factory,
/// `TiberiumSpawnType=` (+338, `0x0042828A`) through OverlayType's
/// (`0x005FEC70`) and `Warhead=` (`0x0042867A`) through WarheadType's
/// (`0x0075E3B0`); every one of them answers null for `none` and `<none>`.
/// `SpawnsParticle=` (+2CC, `0x00428716..0x00428735`) reads into 0x20 bytes
/// and stores ParticleType FindOrCreate's answer (`0x00645430`), which is
/// null only for `<none>`. The constructor leaves each one null.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct AnimTypeReferences {
    next: Option<String>,
    spawns: Option<String>,
    tiberium_spawn_type: Option<String>,
    bounce_anim: Option<String>,
    expire_anim: Option<String>,
    trailer_anim: Option<String>,
    warhead: Option<String>,
    spawns_particle: Option<String>,
}

impl AnimTypeReferences {
    /// Publish onto the ART registry's runtime record, whose lookups key
    /// every name in upper case.
    pub(crate) fn apply_to(&self, config: &mut crate::rules::art_data::AnimTypeRuntimeConfig) {
        let upper = |name: &Option<String>| name.as_deref().map(str::to_ascii_uppercase);
        config.next = upper(&self.next);
        config.spawns = upper(&self.spawns);
        config.tiberium_spawn_type = upper(&self.tiberium_spawn_type);
        config.bounce_anim = upper(&self.bounce_anim);
        config.expire_anim = upper(&self.expire_anim);
        config.trailer_anim = upper(&self.trailer_anim);
        config.warhead = upper(&self.warhead);
        config.spawns_particle = upper(&self.spawns_particle);
    }
}

/// The Type references `VoxelAnimTypeClass::ReadINI` retains, each a
/// ReadString into 0x80 bytes whose empty read keeps the field: `Spawns=`
/// (+2D4, `0x0074B1FF..0x0074B224`) through `VoxelAnimTypeClass::FindOrCreate
/// @ 0x0074BB20`, `BounceAnim=` (+2E4, `0x0074B330`), `ExpireAnim=` (+2E8,
/// `0x0074B36F`) and `TrailerAnim=` (+2EC, `0x0074B3AC`) through AnimType's
/// factory (`0x00428B80`), `Warhead=` (+2F8, `0x0074B421`) through
/// WarheadType's and `AttachedSystem=` (+2FC, `0x0074B45E..0x0074B484`)
/// through ParticleSystemType's (`0x00644890`). Each factory answers null for
/// `none` and `<none>`; the constructor leaves each pointer null.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct VoxelAnimTypeReferences {
    spawns: Option<String>,
    bounce_anim: Option<String>,
    expire_anim: Option<String>,
    trailer_anim: Option<String>,
    warhead: Option<String>,
    attached_system: Option<String>,
}

impl VoxelAnimTypeReferences {
    pub(crate) fn apply_to(&self, voxel_anim: &mut crate::rules::voxel_anim_type::VoxelAnimType) {
        voxel_anim.spawns.clone_from(&self.spawns);
        voxel_anim.bounce_anim.clone_from(&self.bounce_anim);
        voxel_anim.expire_anim.clone_from(&self.expire_anim);
        voxel_anim.trailer_anim.clone_from(&self.trailer_anim);
        voxel_anim.warhead.clone_from(&self.warhead);
        voxel_anim.attached_system.clone_from(&self.attached_system);
    }
}

/// The weapon block of `TechnoTypeClass::ReadINI @ 0x00712170`
/// (`0x0071284A..0x00712AAD`), retained on the live Type. Each reached pass
/// reads TurretCount (+808, `0x0071285E`), WeaponCount (+80C, `0x00712878`)
/// and ClearAllWeapons (+A90, `0x007128AC`) with the live values as defaults,
/// then picks the slot keys from those live values:
///
/// - TurretCount > 0 (`0x007128B2`): `Weapon%d` (key `0x00844310`) and
///   `EliteWeapon%d` (`0x00844300`) for 1..=WeaponCount, stored at `+898` and
///   `+A94` plus `(n-1)*0x1C` (`0x0071294A`, `0x00712981`). `Weapon1` and
///   `Primary` are the same storage.
/// - Otherwise, unless ClearAllWeapons (`0x007129A3`): `Primary`, `Secondary`,
///   `ElitePrimary` and `EliteSecondary` into slots 0 and 1 of each array
///   (`0x007129F2`, `0x00712A17`, `0x00712A64`, `0x00712A89`).
/// - Then ClearAllWeapons nulls both arrays' slots 0 and 1
///   (`0x00712A8F..0x00712AAD`), after either branch.
///
/// Each slot read is a ReadString into 0x80 bytes; an empty read keeps the
/// slot, so a pass that omits a key keeps what an earlier pass wrote. A value
/// goes through `WeaponTypeClass::FindOrAllocate @ 0x00772FA0`, which answers
/// null for `none` and `<none>` (stock `[CMIN]` `Primary=none` is unarmed).
/// The TechnoType constructor zeroes TurretCount (`0x0071136F`),
/// ClearAllWeapons (`0x0071137B`) and both 18-entry arrays
/// (`0x007116E6..0x0071173A`); see `ObjectType::weapon_count` for
/// WeaponCount's.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct WeaponSlots {
    turret_count: i32,
    weapon_count: i32,
    clear_all_weapons: bool,
    /// Base (+898) and elite (+A94) arrays, as stored WeaponType IDs.
    base: [Option<String>; WEAPON_SLOT_COUNT],
    elite: [Option<String>; WEAPON_SLOT_COUNT],
}

impl WeaponSlots {
    pub(crate) fn turret_count(&self) -> i32 {
        self.turret_count
    }

    pub(crate) fn weapon_count(&self) -> i32 {
        self.weapon_count
    }

    pub(crate) fn base(&self) -> &[Option<String>] {
        &self.base
    }

    pub(crate) fn elite(&self) -> &[Option<String>] {
        &self.elite
    }
}

/// One WeaponType's retained reads ([`WeaponReadState`]) with its references
/// as stored IDs, for `RuleSet`.
pub(crate) struct WeaponState<'a> {
    pub(crate) id: &'a str,
    pub(crate) damage: i32,
    pub(crate) speed: i32,
    pub(crate) range_leptons: i32,
    pub(crate) projectile: Option<&'a str>,
    pub(crate) warhead: Option<&'a str>,
    pub(crate) anim: &'a [String],
    pub(crate) occupant_anim: Option<&'a str>,
    pub(crate) open_topped_anim: Option<&'a str>,
    pub(crate) attached_particle_system: Option<&'a str>,
}

/// Fields needed by Weapon7729F0, retained on the same live Weapon object.
/// The projectile handle is an allocation index, not a relookup of a truncated
/// name: repeated names longer than 24 bytes can allocate distinct native types.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
struct WeaponReadState {
    speed: i32,
    /// +B4, ReadRange (`0x00474620`) at `0x00772331` with the retained value
    /// as its default, over the constructor's zero (`0x00771CB6`): leptons,
    /// `-1` keeps the field.
    range_leptons: i32,
    projectile: Option<usize>,
    /// Weapon771C70 initializes +A4/+AC to zero/null. ReadINI7722D4..7722E8
    /// reads Damage with its current signed value;77294C..772981 only replaces
    /// the Warhead pointer after a nonempty ReadString128. Executed controls:
    /// spatial_oracle/trigger_do_explosion.json (Steam gamemd).
    damage: i32,
    warhead: Option<usize>,
    /// `Anim=` (`0x00772471`): ReadString128, then each `strtok` token's
    /// non-null AnimType (`0x00428B80`) replaces the list.
    anim: Vec<String>,
    /// `OccupantAnim=` (+110, `0x007725A1..0x007725CB`) and `OpenToppedAnim=`
    /// (+118, `0x007725E6`): ReadString128, then the AnimType factory, which
    /// answers null for `none` and `<none>`; an empty read keeps the field.
    occupant_anim: Option<String>,
    open_topped_anim: Option<String>,
    /// `AttachedParticleSystem=` (+11C, `0x0077290B..0x0077293C`): ReadString
    /// into 0x14 bytes over an empty default, then
    /// `ParticleSystemTypeClass::FindOrCreate @ 0x00644630`. Its -1 for an
    /// empty name or `<none>` keeps the field; `none`, like any other name,
    /// finds or constructs a ParticleSystemType of that name.
    attached_particle_system: Option<String>,
}

impl ProcessedType {
    fn new(native_stored_id: String) -> Self {
        Self {
            body: IniSection::new(native_stored_id.clone()),
            voxel_shadow_index: 0,
            tech_level: crate::rules::object_type::DEFAULT_TECH_LEVEL,
            projectile_art: ProjectileArtState::new(&native_stored_id),
            techno_references: TechnoTypeReferences::new(&native_stored_id),
            native_stored_id,
            country_color_scheme: 0,
            country_side_index: -1,
            side_country_members: Vec::new(),
            anim_art_read: false,
            building_foundation: 0,
            building_target_coord_offset: [0; 3],
            building_has_spotlight: false,
            building_light: Default::default(),
            building_upgrade: Default::default(),
            secret_lab: Default::default(),
            unit_shp: Default::default(),
            unit_speed_type: SpeedType::Invalid,
            gunner_turrets: GunnerTurrets::default(),
            infantry_pips: InfantryPips::default(),
            infantry_infiltration: InfantryInfiltration::default(),
            recoil: crate::rules::recoil::RecoilConfig::default(),
            airstrike: crate::rules::airstrike::AirstrikeSettings::default(),
            radial_fire_segments: 0,
            lepton_mind_control_offset:
                crate::rules::object_type::DEFAULT_LEPTON_MIND_CONTROL_OFFSET,
            prerequisite: Vec::new(),
            prerequisite_override: Vec::new(),
            weapon_slots: WeaponSlots::default(),
            weapon: WeaponReadState::default(),
            warhead_anim: WarheadAnimReadState::default(),
            warhead_verses: [NativeF64Bits::ONE; 11],
            ares_verses: None,
            armor_id: ArmorId::NONE,
            holds_what: None,
            super_weapon_weapon: None,
            super_weapon_aux_building: None,
            unit_delivery: None,
            super_weapon_building_requirements: None,
            generic_super_weapon_warhead: None,
            particle_warhead: None,
            voxel_anim_references: VoxelAnimTypeReferences::default(),
            anim_references: AnimTypeReferences::default(),
            shrapnel_weapon: None,
        }
    }
}

#[derive(Debug)]
struct RulesPassProcessor {
    compatibility: RulesCompatibility,
    ares_prerequisites: Option<GenericPrerequisites>,
    /// Ordered Ares armor registry; Process retains it, destructive reset rebuilds it.
    ares_armors: Option<ArmorCatalog>,
    ordinary: Option<IniFile>,
    crate_rules: CrateRules,
    powerups: PowerupsAccumulator,
    general_type_lists: GeneralTypeLists,
    missile_spawn: MissileSpawnRules,
    families: HashMap<RulesTypeFamily, Vec<ProcessedType>>,
    techno_type_order: Vec<(RulesTypeFamily, usize)>,
    native_type_construction_events: Vec<NativeTypeConstructionEvent>,
    tiberiums: Vec<ProcessedType>,
    colors: Vec<(String, String)>,
    rules_color_add: ColorAddTable,
    rules_color_add_selectors: ColorAddSelectors,
    rules_detail: DetailRules,
    rules_ambient_change_rate: NativeF64Bits,
    rules_ambient_change_step: NativeF64Bits,
    rules_difficulty_rows: [crate::rules::ruleset::DifficultyRules; 3],
    rules_gravity: i32,
    rules_wind_direction: i32,
    rules_missile_rot_var: f64,
    rules_safety_altitude: i32,
    rules_parachute_max_fall_rate: i32,
    rules_no_parachute_max_fall_rate: i32,
    rules_survivor_divisors: [i32; 3],
    rules_infantry_repair_step: i32,
    rules_unit_repair_rate: NativeF64Bits,
    rules_infantry_repair_rate: NativeF64Bits,
    rules_maximum_cheer_rate: i32,
    rules_talk_bubble_time: i32,
    rules_spotlight: crate::rules::ruleset::SpotlightRules,
    rules_ion_cannon_damage: i32,
    rules_ion_blast_anim: String,
    rules_ion_beam_anim: String,
    rules_ion_cannon_warhead: String,
    rules_mind_control_attack_line_frames: i32,
    rules_line_trail_override: [u8; 3],
    rules_chrono_beam_color: [u8; 3],
    rules_prism_support: PrismSupportRules,
    rules_patrol_scan_minutes: NativeF64Bits,
    rules_comp_easy_bonus: bool,
    rules_prism_type: Option<String>,
    rules_drop_zone_anim: Option<String>,
    rules_drop_zone_radius: i32,
    rules_behind: Option<String>,
    rules_prerequisite_proc_alternate: Option<String>,
    rules_prerequisite_lists: [Vec<Prerequisite>; 6],
    rules_building_types: GeneralBuildingTypes,
    select_anim: SelectAnimRulesState,
    rules_death_weapon: Option<String>,
    rules_type_references: RulesTypeReferences,
}

impl Default for RulesPassProcessor {
    /// A fresh RulesClass and empty Type registries; the constructor values
    /// have one owner, [`NativeRulesRegistryState::default`].
    fn default() -> Self {
        Self::with_registry_state(NativeRulesRegistryState::default())
    }
}

impl RulesPassProcessor {
    fn with_registry_state(registry_state: NativeRulesRegistryState) -> Self {
        Self {
            compatibility: registry_state.compatibility,
            ares_prerequisites: registry_state.ares_prerequisites,
            ares_armors: registry_state.ares_armors,
            families: registry_state.families,
            techno_type_order: registry_state.techno_type_order,
            tiberiums: registry_state.tiberiums,
            colors: registry_state.colors,
            rules_color_add: registry_state.rules_color_add,
            rules_color_add_selectors: registry_state.rules_color_add_selectors,
            rules_detail: registry_state.rules_detail,
            rules_ambient_change_rate: registry_state.rules_ambient_change_rate,
            rules_ambient_change_step: registry_state.rules_ambient_change_step,
            rules_difficulty_rows: registry_state.rules_difficulty_rows,
            rules_gravity: registry_state.rules_gravity,
            rules_wind_direction: registry_state.rules_wind_direction,
            rules_missile_rot_var: registry_state.rules_missile_rot_var,
            rules_safety_altitude: registry_state.rules_safety_altitude,
            rules_parachute_max_fall_rate: registry_state.rules_parachute_max_fall_rate,
            rules_no_parachute_max_fall_rate: registry_state.rules_no_parachute_max_fall_rate,
            rules_survivor_divisors: registry_state.rules_survivor_divisors,
            rules_infantry_repair_step: registry_state.rules_infantry_repair_step,
            rules_unit_repair_rate: registry_state.rules_unit_repair_rate,
            rules_infantry_repair_rate: registry_state.rules_infantry_repair_rate,
            rules_maximum_cheer_rate: registry_state.rules_maximum_cheer_rate,
            rules_talk_bubble_time: registry_state.rules_talk_bubble_time,
            rules_spotlight: registry_state.rules_spotlight,
            rules_ion_cannon_damage: registry_state.rules_ion_cannon_damage,
            rules_ion_blast_anim: registry_state.rules_ion_blast_anim,
            rules_ion_beam_anim: registry_state.rules_ion_beam_anim,
            rules_ion_cannon_warhead: registry_state.rules_ion_cannon_warhead,
            rules_mind_control_attack_line_frames: registry_state
                .rules_mind_control_attack_line_frames,
            rules_line_trail_override: registry_state.rules_line_trail_override,
            rules_chrono_beam_color: registry_state.rules_chrono_beam_color,
            rules_prism_support: registry_state.rules_prism_support,
            rules_patrol_scan_minutes: registry_state.rules_patrol_scan_minutes,
            rules_comp_easy_bonus: registry_state.rules_comp_easy_bonus,
            rules_prism_type: registry_state.rules_prism_type,
            rules_drop_zone_anim: registry_state.rules_drop_zone_anim,
            rules_drop_zone_radius: registry_state.rules_drop_zone_radius,
            rules_behind: registry_state.rules_behind,
            rules_prerequisite_proc_alternate: registry_state.rules_prerequisite_proc_alternate,
            rules_prerequisite_lists: registry_state.rules_prerequisite_lists,
            rules_building_types: registry_state.rules_building_types,
            select_anim: registry_state.select_anim,
            rules_death_weapon: registry_state.rules_death_weapon,
            rules_type_references: registry_state.rules_type_references,
            crate_rules: registry_state.rules_crate,
            general_type_lists: registry_state.rules_general_type_lists,
            missile_spawn: registry_state.rules_missile_spawn,
            ordinary: None,
            powerups: PowerupsAccumulator::default(),
            native_type_construction_events: Vec::new(),
        }
    }

    fn apply_pass(&mut self, pass: &IniFile, fixed_art: &IniFile) -> Result<(), RulesError> {
        if let Some(ordinary) = self.ordinary.as_mut() {
            ordinary.merge_rules_projection(pass);
        } else {
            let mut ordinary = IniFile::empty();
            ordinary.merge_rules_projection(pass);
            self.ordinary = Some(ordinary);
        }

        // Exact constructor-capable order from `RulesClass::Process @
        // 0x00668BF0`. Colors precede every Type registry but spend no Type ID.
        self.allocate_colors(pass);
        self.rules_color_add.apply_pass(pass);
        for &(registry, family) in EXPLICIT_RULE_TYPE_FAMILIES {
            if family == RulesTypeFamily::Side {
                self.process_side_registry(pass)?;
            } else {
                self.allocate_explicit_family(pass, registry, family);
            }
        }

        // JumpjetControls and MultiplayerSettings contain no Type factory.
        self.allocate_ai_references(pass);
        self.read_prerequisite_lists(pass);
        self.allocate_general_references(pass);
        self.process_type_data(pass, fixed_art);

        // ReadDifficulty66D270 skips an absent section. Once present, each
        // absent key receives its literal default, rather than a prior-pass
        // field. This cannot be represented by merging raw INI text.
        for (index, name) in ["Easy", "Normal", "Difficult"].into_iter().enumerate() {
            if let Some(section) = pass.section(name) {
                self.rules_difficulty_rows[index] =
                    crate::rules::ruleset::DifficultyRules::read_pass(section);
            }
        }
        // ReadCrateRules @ 0x0066B900 allocates its three crate images and
        // UnitCrateType as it reads them, between scalars that allocate none.
        let mut crate_rules = std::mem::take(&mut self.crate_rules);
        crate_rules.apply_pass(pass, |section, key| {
            let family = if key == "UnitCrateType" {
                RulesTypeFamily::Vehicle
            } else {
                RulesTypeFamily::Overlay
            };
            self.resolve_scalar_from(section, key, family, 0x80)
        });
        self.crate_rules = crate_rules;
        self.powerups.apply_pass(pass);
        self.allocate_combat_references(pass);
        // ReadGeneral's rocket blocks and ReadCombatDamage's rocket warheads.
        // They read and write no other Process state, so one call per pass
        // keeps their order.
        self.missile_spawn.apply_pass(pass);
        self.allocate_radiation_references(pass);
        // Elevation and Wall contain no Type factories.
        self.allocate_audio_visual_references(pass);
        self.process_special_weapons(pass);
        self.process_tiberiums(pass)?;
        // AdvancedCommandBar contains no Type factory.
        Ok(())
    }

    fn allocate_explicit_family(
        &mut self,
        pass: &IniFile,
        registry: &str,
        family: RulesTypeFamily,
    ) {
        let Some(section) = pass.section(registry) else {
            return;
        };
        for key in section.keys() {
            let identity = section.read_string(key, "", 32);
            if !identity.is_empty() {
                self.find_or_allocate(family, &identity);
            }
        }
    }

    fn family_mut(&mut self, family: RulesTypeFamily) -> &mut Vec<ProcessedType> {
        self.families.entry(family).or_default()
    }

    fn find_existing(&self, family: RulesTypeFamily, incoming: &str) -> Option<usize> {
        self.families
            .get(&family)?
            .iter()
            .position(|member| member.native_stored_id.eq_ignore_ascii_case(incoming))
    }

    fn country_index_of_name(&self, incoming: &str) -> i32 {
        house_type_index_of_name(
            incoming,
            self.families
                .get(&RulesTypeFamily::Country)
                .into_iter()
                .flatten()
                .map(|member| {
                    (
                        member.native_stored_id.as_str(),
                        member.body.read_name("Name", 0x31),
                    )
                }),
        )
    }

    /// ReadHouses4767C0: lookup-only Country5117D0, then existing Side6A46D0
    /// for exactly-1. A Side token expands its current vector; no Country is
    /// constructed. Self-reference therefore reads the prior vector until
    /// the caller replaces it. Executed list controls: campaign_start.py
    /// --houses / campaign_start_houses.json side_controls (+meta).
    fn read_side_members(&self, section: &IniSection, key: &str, current: &[i32]) -> Vec<i32> {
        let Some(tokens) = section.read_list(key, 0x80) else {
            return current.to_vec();
        };
        let mut members = Vec::new();
        for token in tokens {
            let country = self.country_index_of_name(token);
            if country != -1 {
                members.push(country);
            } else if let Some(index) = self.find_existing(RulesTypeFamily::Side, token) {
                members.extend_from_slice(
                    &self.families[&RulesTypeFamily::Side][index].side_country_members,
                );
            }
        }
        members
    }

    /// Rules ReadSides672440 creates/reuses each key and immediately reads
    /// its membership, then writes Country+BC. Registration is per entry;
    /// later new Sides cannot be resolved by an earlier entry in this pass.
    fn process_side_registry(&mut self, pass: &IniFile) -> Result<(), RulesError> {
        let Some(section) = pass.section("Sides") else {
            return Ok(());
        };
        for key in section.keys() {
            let Some(index) = self.find_or_allocate(RulesTypeFamily::Side, key) else {
                continue;
            };
            let prior = &self.families[&RulesTypeFamily::Side][index].side_country_members;
            let members = self.read_side_members(section, key, prior);
            self.families
                .get_mut(&RulesTypeFamily::Side)
                .expect("registered Side exists")[index]
                .side_country_members = members.clone();
            for member in members {
                let country = usize::try_from(member).ok().and_then(|index| {
                    self.families
                        .get_mut(&RulesTypeFamily::Country)?
                        .get_mut(index)
                });
                let Some(country) = country else {
                    // `<random>` remains-2 in the reader.672597 would index
                    // Country[-2]; reject this undefined pointer route after
                    // retaining the actual Side mutation for failure recovery.
                    return Err(RulesError::InvalidValue {
                        section: "Sides".to_string(),
                        key: key.to_string(),
                        expected: "a nonnegative resolved native Country index".to_string(),
                        value: member.to_string(),
                    });
                };
                country.country_side_index =
                    i32::try_from(index).expect("native Side index exceeds i32");
            }
        }
        Ok(())
    }

    /// ReadSide4756F0: ReadString128 with an empty default, then reuse or
    /// construct a Side. Count0 retains the caller's current index. This
    /// reader does not read memberships; only Rules ReadSides672440 does.
    /// Executed controls: campaign_start.py --houses, side_reader_rows in
    /// campaign_start_houses.json side_controls (+meta).
    fn read_side_index(&mut self, section: &IniSection, key: &str, current: i32) -> i32 {
        let name = section.read_string(key, "", 0x80);
        if name.is_empty() {
            return current;
        }
        let index = self
            .find_or_allocate(RulesTypeFamily::Side, &name)
            .expect("a nonempty Side name is a valid factory input");
        i32::try_from(index).expect("native Side index exceeds i32")
    }

    fn find_or_allocate(&mut self, family: RulesTypeFamily, incoming: &str) -> Option<usize> {
        // Caller-specific ReadString buffers trim the whole scalar before this
        // boundary. List tokens deliberately arrive untrimmed. The factory
        // must therefore inspect the exact incoming string, not trim again.
        if incoming.is_empty()
            || (family != RulesTypeFamily::Side && is_native_none_type_name(incoming))
        {
            return None;
        }
        Some(self.find_or_construct(family, incoming))
    }

    /// ParticleTypeClass::FindOrAllocate @ 0x00645430 answers -1 only for
    /// `<none>` (`_strcmpi` against 0x00817474); an empty name or `none`
    /// finds or constructs a ParticleType like any other spelling.
    fn find_or_allocate_particle(&mut self, incoming: &str) -> Option<usize> {
        (!incoming.eq_ignore_ascii_case("<none>"))
            .then(|| self.find_or_construct(RulesTypeFamily::Particle, incoming))
    }

    /// ParticleSystemTypeClass::FindOrCreate @ 0x00644630 answers -1 for
    /// `<none>` (`_strcmpi` against 0x00817474) and for an empty name; `none`,
    /// like every other spelling, finds or constructs a ParticleSystemType.
    fn find_or_create_particle_system(&mut self, incoming: &str) -> Option<usize> {
        (!incoming.is_empty() && !incoming.eq_ignore_ascii_case("<none>"))
            .then(|| self.find_or_construct(RulesTypeFamily::ParticleSystem, incoming))
    }

    /// The lookup and construction every Type factory shares once its own
    /// null answers are excluded.
    fn find_or_construct(&mut self, family: RulesTypeFamily, incoming: &str) -> usize {
        if let Some(index) = self.find_existing(family, incoming) {
            return index;
        }
        let members = self.family_mut(family);
        // AbstractTypeClass::Constructor @ 0x00410800 stores only 0x18 bytes.
        // Lookup above compares that stored ID against the full input, so a
        // repeated >24-byte spelling can construct another equal stored ID.
        let native_stored_id = abstract_type_stored_id(incoming);
        let index = members.len();
        members.push(ProcessedType::new(native_stored_id.clone()));
        if family == RulesTypeFamily::Terrain {
            members[index].armor_id = ArmorId::WOOD;
        }
        if crate::rules::ARES_ENABLED && family == RulesTypeFamily::Warhead {
            members[index].ares_verses = Some(AresWarheadVerses::default());
        }
        if family.object_category().is_some() {
            // TechnoType constructors71181D..711832 /711ABD..711ACB append
            // to the shared vector at allocation time, including lazy Types.
            self.techno_type_order.push((family, index));
        }
        if let Some(family) = family.native_constructor_family() {
            self.native_type_construction_events
                .push(NativeTypeConstructionEvent {
                    family,
                    native_stored_id,
                });
        }
        index
    }

    fn allocate_colors(&mut self, pass: &IniFile) {
        let Some(section) = pass.section("Colors") else {
            return;
        };
        for (key, value) in section.raw_entries() {
            if self
                .colors
                .iter()
                .any(|(existing, _)| existing.eq_ignore_ascii_case(key))
            {
                continue;
            }
            self.colors.push((key.to_string(), value.to_string()));
        }
    }

    /// None retains the prior pointer; Some(None) is an authored null.
    /// ReadString's empty default and FindOrAllocate are shared by scalar
    /// allocation sites and the fields which retain their resolved identities.
    fn resolve_scalar_from(
        &mut self,
        section: &IniSection,
        key: &str,
        family: RulesTypeFamily,
        capacity: usize,
    ) -> Option<Option<String>> {
        let incoming = section.read_string(key, "", capacity);
        if incoming.is_empty() {
            return None;
        }
        Some(
            self.find_or_allocate(family, &incoming)
                .map(|index| self.families[&family][index].native_stored_id.clone()),
        )
    }

    fn allocate_list_from(
        &mut self,
        section: &IniSection,
        key: &str,
        family: RulesTypeFamily,
        capacity: usize,
    ) {
        let _ = self.resolve_list_from(section, key, family, capacity);
    }

    /// Resolve each native pointer once, in constructor order. None means the
    /// reader retained its previous vector; Some(empty) is a real replacement
    /// (for example `,,,` or only exact `none` sentinels).
    /// Ares pointer vectors look up existing types at this exact read point.
    /// ReadString/strtok retain the native 2048-byte, comma-only list policy.
    #[cfg(feature = "ares")]
    fn resolve_existing_list_from(
        &self,
        section: &IniSection,
        key: &str,
        family: RulesTypeFamily,
        capacity: usize,
    ) -> Option<Vec<String>> {
        Some(
            section
                .read_list(key, capacity)?
                .into_iter()
                .filter_map(|token| {
                    self.find_existing(family, token)
                        .map(|index| self.families[&family][index].native_stored_id.clone())
                })
                .collect(),
        )
    }

    fn resolve_list_from(
        &mut self,
        section: &IniSection,
        key: &str,
        family: RulesTypeFamily,
        capacity: usize,
    ) -> Option<Vec<String>> {
        let mut resolved = Vec::new();
        for token in section.read_list(key, capacity)? {
            if let Some(index) = self.find_or_allocate(family, token) {
                resolved.push(self.families[&family][index].native_stored_id.clone());
            }
        }
        Some(resolved)
    }

    fn allocate_ai_references(&mut self, pass: &IniFile) {
        const BUILDING_LISTS: &[&str] = &[
            "BuildConst",
            "BuildPower",
            "BuildRefinery",
            "BuildBarracks",
            "BuildTech",
            "BuildWeapons",
            "AlliedBaseDefenses",
            "SovietBaseDefenses",
            "ThirdBaseDefenses",
            "BuildDefense",
            "BuildPDefense",
            "BuildAA",
            "BuildHelipad",
            "BuildRadar",
            "ConcreteWalls",
            "NSGates",
            "EWGates",
            "BuildNavalYard",
            "BuildDummy",
            "NeutralTechBuildings",
        ];

        let Some(section) = pass.section("AI") else {
            return;
        };
        for key in BUILDING_LISTS {
            let resolved = self.resolve_list_from(section, key, RulesTypeFamily::Building, 0x80);
            self.rules_type_references.read_list(key, resolved);
        }
        // ReadAI672AE0 has its own AI-section gate.67399E..6739C5 reads
        // the current double as default, with no clamp, after the AI lists.
        // RulesClass outlives the destructive Type reset, so keep this scalar
        // in the same process-resident authority as the other retained rules.
        self.rules_patrol_scan_minutes =
            section.read_double_bits("PatrolScan", self.rules_patrol_scan_minutes);
        // ReadAI672AE0 reaches673CFD..673D1D after AirstripLimit and
        // before Paranoid. Rules+17E3 survives the6686C0 Type reset;
        // ReadBool5295F0 retains its current byte on absent/invalid input.
        self.rules_comp_easy_bonus = section.read_bool("CompEasyBonus", self.rules_comp_easy_bonus);
    }

    fn allocate_general_references(&mut self, pass: &IniFile) {
        const SITES: &[(&str, RulesTypeFamily, bool)] = &[
            ("DamageFireTypes", RulesTypeFamily::Animation, true),
            ("OreTwinkle", RulesTypeFamily::Animation, false),
            ("BarrelExplode", RulesTypeFamily::Animation, false),
            ("BarrelDebris", RulesTypeFamily::VoxelAnimation, true),
            ("BarrelParticle", RulesTypeFamily::ParticleSystem, false),
            ("NukeTakeOff", RulesTypeFamily::Animation, false),
            ("Wake", RulesTypeFamily::Animation, false),
            ("DropPod", RulesTypeFamily::Animation, true),
            ("DeadBodies", RulesTypeFamily::Animation, true),
            ("MetallicDebris", RulesTypeFamily::Animation, true),
            ("BridgeExplosions", RulesTypeFamily::Animation, true),
            ("IonBlast", RulesTypeFamily::Animation, false),
            ("IonBeam", RulesTypeFamily::Animation, false),
            ("WeatherConClouds", RulesTypeFamily::Animation, true),
            ("WeatherConBolts", RulesTypeFamily::Animation, true),
            ("WeatherConBoltExplosion", RulesTypeFamily::Animation, false),
            ("DominatorWarhead", RulesTypeFamily::Warhead, false),
            ("DominatorFirstAnim", RulesTypeFamily::Animation, false),
            ("DominatorSecondAnim", RulesTypeFamily::Animation, false),
            ("ChronoPlacement", RulesTypeFamily::Animation, false),
            ("ChronoBeam", RulesTypeFamily::Animation, false),
            ("ChronoBlast", RulesTypeFamily::Animation, false),
            ("ChronoBlastDest", RulesTypeFamily::Animation, false),
            ("WarpIn", RulesTypeFamily::Animation, false),
            ("WarpOut", RulesTypeFamily::Animation, false),
            ("WarpAway", RulesTypeFamily::Animation, false),
            ("IronCurtainInvokeAnim", RulesTypeFamily::Animation, false),
            ("ForceShieldInvokeAnim", RulesTypeFamily::Animation, false),
            ("WeaponNullifyAnim", RulesTypeFamily::Animation, false),
            ("ChronoSparkle1", RulesTypeFamily::Animation, false),
            ("InfantryExplode", RulesTypeFamily::Animation, false),
            ("FlamingInfantry", RulesTypeFamily::Animation, false),
            ("InfantryHeadPop", RulesTypeFamily::Animation, false),
            ("InfantryNuked", RulesTypeFamily::Animation, false),
            ("InfantryVirus", RulesTypeFamily::Animation, false),
            ("InfantryBrute", RulesTypeFamily::Animation, false),
            ("InfantryMutate", RulesTypeFamily::Animation, false),
            ("Behind", RulesTypeFamily::Animation, false),
            ("MoveFlash", RulesTypeFamily::Animation, false),
            ("Parachute", RulesTypeFamily::Animation, false),
            ("BombParachute", RulesTypeFamily::Animation, false),
            ("DropZoneAnim", RulesTypeFamily::Animation, false),
            ("EMPulseSparkles", RulesTypeFamily::Animation, false),
            ("LargeVisceroid", RulesTypeFamily::Vehicle, false),
            ("SmallVisceroid", RulesTypeFamily::Vehicle, false),
            ("DropPodWeapon", RulesTypeFamily::Weapon, false),
            (
                "ExplosiveVoxelDebris",
                RulesTypeFamily::VoxelAnimation,
                true,
            ),
            ("TireVoxelDebris", RulesTypeFamily::VoxelAnimation, false),
            ("ScrapVoxelDebris", RulesTypeFamily::VoxelAnimation, false),
            ("RepairBay", RulesTypeFamily::Building, true),
            ("GDIGateOne", RulesTypeFamily::Building, false),
            ("GDIGateTwo", RulesTypeFamily::Building, false),
            ("NodGateOne", RulesTypeFamily::Building, false),
            ("NodGateTwo", RulesTypeFamily::Building, false),
            ("WallTower", RulesTypeFamily::Building, false),
            ("Shipyard", RulesTypeFamily::Building, true),
            ("GDIPowerPlant", RulesTypeFamily::Building, false),
            ("NodRegularPower", RulesTypeFamily::Building, false),
            ("NodAdvancedPower", RulesTypeFamily::Building, false),
            ("ThirdPowerPlant", RulesTypeFamily::Building, false),
            ("PrerequisiteProcAlternate", RulesTypeFamily::Vehicle, false),
            ("BaseUnit", RulesTypeFamily::Vehicle, true),
            ("HarvesterUnit", RulesTypeFamily::Vehicle, true),
            ("PadAircraft", RulesTypeFamily::Aircraft, true),
            ("Paratrooper", RulesTypeFamily::Infantry, false),
            ("SecretInfantry", RulesTypeFamily::Infantry, true),
            ("SecretUnits", RulesTypeFamily::Vehicle, true),
            ("SecretBuildings", RulesTypeFamily::Building, true),
            ("AlliedDisguise", RulesTypeFamily::Infantry, false),
            ("SovietDisguise", RulesTypeFamily::Infantry, false),
            ("ThirdDisguise", RulesTypeFamily::Infantry, false),
            ("Engineer", RulesTypeFamily::Infantry, false),
            ("Technician", RulesTypeFamily::Infantry, false),
            ("Pilot", RulesTypeFamily::Infantry, false),
            ("AlliedCrew", RulesTypeFamily::Infantry, false),
            ("SovietCrew", RulesTypeFamily::Infantry, false),
            ("ThirdCrew", RulesTypeFamily::Infantry, false),
            ("AmerParaDropInf", RulesTypeFamily::Infantry, true),
            ("AllyParaDropInf", RulesTypeFamily::Infantry, true),
            ("SovParaDropInf", RulesTypeFamily::Infantry, true),
            ("YuriParaDropInf", RulesTypeFamily::Infantry, true),
            ("AnimToInfantry", RulesTypeFamily::Infantry, true),
            ("LightningWarhead", RulesTypeFamily::Warhead, false),
            ("PrismType", RulesTypeFamily::Building, false),
            ("V3RocketType", RulesTypeFamily::Aircraft, false),
            ("DMislType", RulesTypeFamily::Aircraft, false),
            ("CMislType", RulesTypeFamily::Aircraft, false),
            ("VeinholeTypeClass", RulesTypeFamily::Terrain, false),
            ("DefaultMirageDisguises", RulesTypeFamily::Terrain, true),
        ];

        let Some(section) = pass.section("General") else {
            return;
        };
        // ReadGeneral66EBD4..66EBEE: exact-case signed ReadInt5276D0,
        // current Rules+580 as default, no clamp. Ctor665D92 stores1.
        self.rules_maximum_cheer_rate =
            section.read_int("MaximumCheerRate", self.rules_maximum_cheer_rate);
        // General66EC37/66EC57 read current RulesClass values as defaults.
        // Both survive empty/missing later passes and destructive Type resets.
        self.rules_missile_rot_var =
            section.read_double("MissileROTVar", self.rules_missile_rot_var);
        self.rules_safety_altitude =
            section.read_int("MissileSafetyAltitude", self.rules_safety_altitude);
        // Rules665650 +7AC starts-1; ReadGeneral66F2AF..66F2CF uses
        // previous fullsigned WindDirection as default, no clamp.
        self.rules_wind_direction = section.read_int("WindDirection", self.rules_wind_direction);
        // ReadGeneral66F30E..66F350: ReadInt5276D0, exact-case keys,
        // current +7B8/+7BC as defaults, in this order, without clamps.
        // Original: tools.native_inspect disasm0x66F30E --bytes66;
        // ctor0x666163 --bytes20 (supported retail gamemd).
        self.rules_parachute_max_fall_rate =
            section.read_int("ParachuteMaxFallRate", self.rules_parachute_max_fall_rate);
        self.rules_no_parachute_max_fall_rate = section.read_int(
            "NoParachuteMaxFallRate",
            self.rules_no_parachute_max_fall_rate,
        );
        // ReadGeneral670C13..670C6C: ReadInt5276D0 in side order, current
        // signed values as defaults, no clamp. Full_Init6876AC ->6686C0
        // resets Types, not these RulesClass fields. Original gamemd.
        for (value, key) in self.rules_survivor_divisors.iter_mut().zip([
            "AlliedSurvivorDivisor",
            "SovietSurvivorDivisor",
            "ThirdSurvivorDivisor",
        ]) {
            *value = section.read_int(key, *value);
        }
        // ReadGeneral670DE9/670E30/670E57: ReadInt, then ReadDouble twice,
        // current Rules+16D8/+16E8/+16F0 defaults, exact keys, no read clamps.
        // Rules66751A/667540 constructor defaults come from GeneralRules.
        self.rules_infantry_repair_step =
            section.read_int("IRepairStep", self.rules_infantry_repair_step);
        self.rules_unit_repair_rate =
            section.read_double_bits("URepairRate", self.rules_unit_repair_rate);
        self.rules_infantry_repair_rate =
            section.read_double_bits("IRepairRate", self.rules_infantry_repair_rate);
        // ReadGeneral's Prism block (`0x0067114F..0x006711B8`), current values
        // as defaults; RulesClass keeps them across Full_Init's reset.
        self.rules_prism_support = self.rules_prism_support.read_pass(section);
        self.rules_spotlight.read_pass(section);
        self.rules_talk_bubble_time = section.read_talk_bubble_time(self.rules_talk_bubble_time);

        for &(key, family, is_list) in SITES {
            if matches!(
                key,
                "MetallicDebris" | "BridgeExplosions" | "WeatherConClouds" | "WeatherConBolts"
            ) {
                if let Some(resolved) = self.resolve_list_from(section, key, family, 0x80) {
                    let lists = &mut self.general_type_lists;
                    *match key {
                        "MetallicDebris" => &mut lists.metallic_debris,
                        "BridgeExplosions" => &mut lists.bridge_explosions,
                        "WeatherConClouds" => &mut lists.weather_con_clouds,
                        _ => &mut lists.weather_con_bolts,
                    } = resolved;
                }
            } else if let Some(side) = PARADROP_INFANTRY_KEYS.iter().position(|&k| k == key) {
                if let Some(resolved) = self.resolve_list_from(section, key, family, 0x80) {
                    self.general_type_lists.paradrop_infantry[side] = resolved;
                }
            } else if let Some(slot) = ["SecretInfantry", "SecretUnits", "SecretBuildings"]
                .iter()
                .position(|&name| name == key)
            {
                if let Some(resolved) = self.resolve_list_from(section, key, family, 0x80) {
                    self.general_type_lists.secret_lab_choices[slot] = resolved;
                }
            } else if key == "AnimToInfantry" {
                if let Some(resolved) = self.resolve_list_from(section, key, family, 0x80) {
                    self.general_type_lists.anim_to_infantry = resolved;
                }
            } else if let Some(side) = ["AlliedDisguise", "SovietDisguise", "ThirdDisguise"]
                .iter()
                .position(|&name| name == key)
            {
                // Absent/empty retains the prior pointer; a factory none
                // token clears it. Reuse the existing allocating scalar reader.
                if let Some(resolved) = self.resolve_scalar_from(section, key, family, 0x80) {
                    self.general_type_lists.infantry_disguises[side] = resolved;
                }
            } else if key == "DefaultMirageDisguises" {
                // Keep actual FindOrAllocate identities, duplicate slots and
                // empty/missing-layer retention. Null factory tokens are
                // excluded by this shared native list reader.
                if let Some(resolved) = self.resolve_list_from(section, key, family, 0x80) {
                    self.general_type_lists.default_mirage_disguises = resolved;
                }
            } else if matches!(
                key,
                "LightningWarhead"
                    | "IonBlast"
                    | "IonBeam"
                    | "WeatherConBoltExplosion"
                    | "WeaponNullifyAnim"
                    | "PrismType"
                    | "DropZoneAnim"
                    | "Behind"
                    | "PrerequisiteProcAlternate"
                    | "GDIGateOne"
                    | "GDIGateTwo"
                    | "NodGateOne"
                    | "NodGateTwo"
                    | "WallTower"
                    | "GDIPowerPlant"
                    | "NodRegularPower"
                    | "NodAdvancedPower"
                    | "ThirdPowerPlant"
            ) {
                // General671053/66DF19/66E2AF: empty ReadString128 retains the
                // current pointer; exact none clears it through the factory.
                // PrismType's reader (`0x0067BCE0`, called at `0x00671144`),
                // the gate/WallTower reads (`0x0066F450..0x0066F583`) and the
                // power plant reads (`0x0066F692..0x0066F781`) do the same
                // through BuildingType's FindOrAllocate (`0x004653C0`), and
                // PrerequisiteProcAlternate (`0x0066F787..0x0066F7C9`) through
                // UnitType's (`0x007480D0`).
                if let Some(resolved) = self.resolve_scalar_from(section, key, family, 0x80) {
                    let resolved = resolved.unwrap_or_default();
                    match key {
                        "LightningWarhead" => self.select_anim.lightning_warhead = resolved,
                        //66DCC7..66DD0D: ReadString128/Anim428B80, current
                        //pointer retained for missing/empty; native none clears.
                        "IonBlast" => self.rules_ion_blast_anim = resolved,
                        "IonBeam" => self.rules_ion_beam_anim = resolved,
                        "WeatherConBoltExplosion" => {
                            self.select_anim.weather_con_bolt_explosion = resolved;
                        }
                        "WeaponNullifyAnim" => self.select_anim.weapon_nullify_anim = resolved,
                        "DropZoneAnim" => {
                            self.rules_drop_zone_anim = Some(resolved).filter(|id| !id.is_empty());
                        }
                        "Behind" => {
                            self.rules_behind = Some(resolved).filter(|id| !id.is_empty());
                        }
                        "PrismType" => {
                            self.rules_prism_type = Some(resolved).filter(|id| !id.is_empty());
                        }
                        "PrerequisiteProcAlternate" => {
                            self.rules_prerequisite_proc_alternate =
                                Some(resolved).filter(|id| !id.is_empty());
                        }
                        _ => {
                            let types = &mut self.rules_building_types;
                            let slot = match key {
                                "GDIGateOne" => &mut types.gdi_gate_one,
                                "GDIGateTwo" => &mut types.gdi_gate_two,
                                "NodGateOne" => &mut types.nod_gate_one,
                                "NodGateTwo" => &mut types.nod_gate_two,
                                "WallTower" => &mut types.wall_tower,
                                "GDIPowerPlant" => &mut types.gdi_power_plant,
                                "NodRegularPower" => &mut types.nod_regular_power,
                                "NodAdvancedPower" => &mut types.nod_advanced_power,
                                _ => &mut types.third_power_plant,
                            };
                            *slot = Some(resolved).filter(|id| !id.is_empty());
                        }
                    }
                }
            } else if let Some(missile) = [
                ("V3RocketType", MissileFamily::V3Rocket),
                ("DMislType", MissileFamily::DMisl),
                ("CMislType", MissileFamily::CMisl),
            ]
            .into_iter()
            .find_map(|(name, missile)| (name == key).then_some(missile))
            {
                // ReadGeneral's rocket blocks end with their type, read by
                // `0x0067BD30` (`0x006713B0`, `0x0067156A`, `0x00671723`):
                // ReadString128, AircraftType's factory, default the current
                // pointer.
                if let Some(resolved) = self.resolve_scalar_from(section, key, family, 0x80) {
                    self.missile_spawn.set_type(missile, resolved);
                }
            } else if is_list {
                let resolved = self.resolve_list_from(section, key, family, 0x80);
                self.rules_type_references.read_list(key, resolved);
            } else {
                let resolved = self.resolve_scalar_from(section, key, family, 0x80);
                self.rules_type_references.read_pointer(key, resolved);
            }
        }
    }

    /// `RulesClass::ReadGeneral` parses the six `[General] Prerequisite*`
    /// lists (`0x0066E78C..0x0066EA6E`) before any of its own BuildingType
    /// allocations, each with its current list as the default.
    fn read_prerequisite_lists(&mut self, pass: &IniFile) {
        // Ares leaves the native66E78C..66EA6E reads reached. Its later
        // BeforeTypeData hook reads the separate generic catalog; do not
        // replace these retained retail lists or their128-byte parser.
        let Some(general) = pass.section("General") else {
            return;
        };
        for group in PrerequisiteGroup::ALL {
            let parsed = parse_prerequisites(
                general.read_list(group.general_key(), 0x80),
                &self.rules_prerequisite_lists[group.index()],
                |name| self.building_index(name),
                None,
            );
            self.rules_prerequisite_lists[group.index()] = parsed;
        }
    }

    /// `BuildingTypeClass::FindIndexByName @ 0x0045E7B0` on the BuildingTypes
    /// allocated so far: the first case-insensitive match.
    fn building_index(&self, name: &str) -> Option<i32> {
        self.families
            .get(&RulesTypeFamily::Building)?
            .iter()
            .position(|member| member.native_stored_id.eq_ignore_ascii_case(name))
            .and_then(|index| i32::try_from(index).ok())
    }

    fn family_len(&self, family: RulesTypeFamily) -> usize {
        self.families.get(&family).map_or(0, Vec::len)
    }

    fn begin_rules_member_read(
        &mut self,
        family: RulesTypeFamily,
        index: usize,
        pass: &IniFile,
    ) -> Option<(String, IniSection, IniSection)> {
        let native_stored_id = self
            .families
            .get(&family)?
            .get(index)?
            .native_stored_id
            .clone();
        let raw = pass.section(&native_stored_id)?.clone();
        let current_armor = self.families.get(&family)?.get(index)?.armor_id;
        let armor = if family.object_category().is_some() || family == RulesTypeFamily::Terrain {
            #[cfg(feature = "ares")]
            {
                self.ares_armors
                    .as_ref()
                    .expect("Ares body reads require the retained armor catalog")
                    .read_binding(&raw, current_armor)
            }
            #[cfg(not(feature = "ares"))]
            {
                raw.read_name("Armor", 0x80)
                    .map_or(current_armor, armor_index)
            }
        } else {
            current_armor
        };
        let member = self.families.get_mut(&family)?.get_mut(index)?;
        member.armor_id = armor;
        member.body.overlay_rules_pass(&raw);
        Some((native_stored_id, raw, member.body.clone()))
    }

    fn process_type_data(&mut self, pass: &IniFile, fixed_art: &IniFile) {
        // Installed BeforeTypeData679A15 reads ArmorTypes/defaults before
        // any ordinary ObjectType/Warhead body in this Process pass.
        #[cfg(feature = "ares")]
        self.ares_armors
            .as_mut()
            .expect("Ares Rules own armor catalog")
            .read_pass(pass);
        // Ares RulesClass_BeforeTypeData679A15 -> 1002AEF0. This is after
        // registry allocation but before any TechnoType body in this pass.
        // Move out and back while reading the existing registries; this is
        // still the one retained catalog, not a merged-INI reconstruction.
        #[cfg(feature = "ares")]
        if let Some(mut catalog) = self.ares_prerequisites.take() {
            catalog.apply_pass(
                pass,
                |name| self.building_index(name),
                |name| {
                    self.techno_type_order.iter().find_map(|&(family, index)| {
                        let member = &self.families[&family][index];
                        member.native_stored_id.eq_ignore_ascii_case(name).then(|| {
                            (
                                family.object_category().expect("live TechnoType"),
                                member.native_stored_id.clone(),
                            )
                        })
                    })
                },
            );
            self.ares_prerequisites = Some(catalog);
        }
        self.process_house_family(pass);
        self.process_super_weapon_family(pass);
        self.process_anim_family(fixed_art);
        self.process_techno_family(RulesTypeFamily::Building, pass, fixed_art);
        self.process_techno_family(RulesTypeFamily::Aircraft, pass, fixed_art);
        self.process_techno_family(RulesTypeFamily::Vehicle, pass, fixed_art);
        self.process_techno_family(RulesTypeFamily::Infantry, pass, fixed_art);
        self.process_weapon_family(pass);
        self.process_bullet_family(pass, fixed_art);
        self.process_warhead_family(pass);
        // 679B8C follows all three readers, even for absent Type sections.
        // AudioVisual6691E0 runs later in Process, so this uses prior Gravity.
        self.postprocess_weapon_speeds();
        // Building post adds no Type references.
        self.process_plain_family(RulesTypeFamily::Terrain, pass);
        self.process_plain_family(RulesTypeFamily::Smudge, pass);
        self.process_plain_family(RulesTypeFamily::Overlay, pass);
        self.process_particle_family(pass);
        self.process_particle_system_family(pass);
        self.process_voxel_anim_family(pass);
        // MissionControl adds no Type references.
    }

    fn process_house_family(&mut self, pass: &IniFile) {
        // Process668BFE calls Colors66D3A0 before Country allocation668CD1
        // and this first ReadTypeData679A2D loop. Use only names registered
        // now; a later pass cannot resolve an earlier unknown retroactively.
        let mut index = 0;
        while index < self.family_len(RulesTypeFamily::Country) {
            if let Some((_id, raw, _effective)) =
                self.begin_rules_member_read(RulesTypeFamily::Country, index, pass)
            {
                let member = &mut self
                    .families
                    .get_mut(&RulesTypeFamily::Country)
                    .expect("the live Country exists")[index];
                member.country_color_scheme = raw.read_color_scheme(
                    "Color",
                    member.country_color_scheme,
                    self.colors.iter().map(|(name, _)| name.as_str()),
                );
                self.allocate_list_from(&raw, "VeteranInfantry", RulesTypeFamily::Infantry, 0x80);
                self.allocate_list_from(&raw, "VeteranUnits", RulesTypeFamily::Vehicle, 0x80);
                self.allocate_list_from(&raw, "VeteranAircraft", RulesTypeFamily::Aircraft, 0x80);
                let current = self.families[&RulesTypeFamily::Country][index].country_side_index;
                let side = self.read_side_index(&raw, "Side", current);
                if let Ok(side) = usize::try_from(side) {
                    self.change_country_side(index, side);
                }
            }
            index += 1;
        }
    }

    /// HouseType ReadINI51208C..512162 stores BC, then updates Side vectors
    /// only when it changed. The old vector deletes POSITION Country+B8,
    /// not the first matching value; an out-of-count position leaves stale
    /// membership. The new vector always appends that Country index.
    /// Executed histories: tools/input_oracle/campaign_start.py --houses,
    /// campaign_start_houses.json side_controls (+meta).
    fn change_country_side(&mut self, country_index: usize, side_index: usize) {
        let side = i32::try_from(side_index).expect("native Side index exceeds i32");
        let country = &mut self
            .families
            .get_mut(&RulesTypeFamily::Country)
            .expect("the live Country exists")[country_index];
        let prior = country.country_side_index;
        country.country_side_index = side;
        if prior == side {
            return;
        }
        let sides = self
            .families
            .get_mut(&RulesTypeFamily::Side)
            .expect("the new Side exists");
        if prior != -1 {
            let prior = usize::try_from(prior).expect("a bound Country Side is nonnegative");
            let members = &mut sides[prior].side_country_members;
            if country_index < members.len() {
                members.remove(country_index);
            }
        }
        sides[side_index]
            .side_country_members
            .push(i32::try_from(country_index).expect("native Country index exceeds i32"));
    }

    fn process_super_weapon_family(&mut self, pass: &IniFile) {
        let mut index = 0;
        while index < self.family_len(RulesTypeFamily::SuperWeapon) {
            if let Some((_id, raw, effective)) =
                self.begin_rules_member_read(RulesTypeFamily::SuperWeapon, index, pass)
            {
                if let Some(weapon) =
                    self.resolve_scalar_from(&raw, "WeaponType", RulesTypeFamily::Weapon, 0x80)
                {
                    self.family_mut(RulesTypeFamily::SuperWeapon)[index].super_weapon_weapon =
                        weapon;
                }
                if let Some(building) =
                    self.resolve_scalar_from(&raw, "AuxBuilding", RulesTypeFamily::Building, 0x80)
                {
                    self.family_mut(RulesTypeFamily::SuperWeapon)[index]
                        .super_weapon_aux_building = building;
                }
                #[cfg(feature = "ares")]
                {
                    // Ares ValueableVector<BuildingTypeClass*> uses Find, not
                    // FindOrAllocate (Utilities/TemplateDef.h::parse_values and
                    // Utilities/Parser.h::Parser<T,1>, Ares 4f1d9299). Unknown
                    // tokens are skipped; each nonempty read replaces the list.
                    let auxiliary = self.resolve_existing_list_from(
                        &raw,
                        "SW.AuxBuildings",
                        RulesTypeFamily::Building,
                        0x800,
                    );
                    let negative = self.resolve_existing_list_from(
                        &raw,
                        "SW.NegBuildings",
                        RulesTypeFamily::Building,
                        0x800,
                    );
                    if auxiliary.is_some() || negative.is_some() {
                        let requirements = self.family_mut(RulesTypeFamily::SuperWeapon)[index]
                            .super_weapon_building_requirements
                            .get_or_insert_default();
                        if let Some(auxiliary) = auxiliary {
                            requirements.auxiliary = auxiliary;
                        }
                        if let Some(negative) = negative {
                            requirements.negative = negative;
                        }
                    }
                    if effective
                        .read_name("Type", 0x28)
                        .is_some_and(|kind| kind.eq_ignore_ascii_case("GenericWarhead"))
                        && let Some(warhead) = self.resolve_scalar_from(
                            &raw,
                            "SW.Warhead",
                            RulesTypeFamily::Warhead,
                            0x80,
                        )
                    {
                        self.family_mut(RulesTypeFamily::SuperWeapon)[index]
                            .generic_super_weapon_warhead = warhead;
                    }
                    if raw
                        .read_name("Type", 0x28)
                        .is_some_and(|kind| kind.eq_ignore_ascii_case("UnitDelivery"))
                        || self.families[&RulesTypeFamily::SuperWeapon][index]
                            .unit_delivery
                            .is_some()
                    {
                        let types = raw.read_list("Deliver.Types", 2048).map(|tokens| {
                            tokens
                                .into_iter()
                                .filter_map(|token| {
                                    let matches: Vec<_> = [
                                        (RulesTypeFamily::Infantry, ObjectCategory::Infantry),
                                        (RulesTypeFamily::Vehicle, ObjectCategory::Vehicle),
                                        (RulesTypeFamily::Aircraft, ObjectCategory::Aircraft),
                                        (RulesTypeFamily::Building, ObjectCategory::Building),
                                    ]
                                    .into_iter()
                                    .filter_map(|(family, category)| {
                                        self.find_existing(family, token).map(|index| {
                                            DeliveryType {
                                                category,
                                                id: self.families[&family][index]
                                                    .native_stored_id
                                                    .clone(),
                                            }
                                        })
                                    })
                                    .collect();
                                    // Do not silently bind an ambiguous cross-category ID
                                    // to an unrelated type. Unknown/none tokens add none.
                                    (matches.len() == 1)
                                        .then(|| matches.into_iter().next().unwrap())
                                })
                                .collect()
                        });
                        let state = self.family_mut(RulesTypeFamily::SuperWeapon)[index]
                            .unit_delivery
                            .get_or_insert_default();
                        if let Some(types) = types {
                            state.types = types;
                        }
                        let delay = raw.read_int("SW.Deferment", state.deferment);
                        state.deferment = if delay < 0 { 20 } else { delay };
                        state.base_normal = raw.read_bool("Deliver.BaseNormal", state.base_normal);
                        if let Some(owner) = raw.read_name("Deliver.Owner", 2048) {
                            state.owner = match owner.to_ascii_lowercase().as_str() {
                                "invoker" => DeliveryOwner::Invoker,
                                "neutral" => DeliveryOwner::Neutral,
                                "special" => DeliveryOwner::Special,
                                "civilian" => DeliveryOwner::Civilian,
                                _ => state.owner,
                            };
                        }
                        // Activation sound resolves against the fixed SOUNDMD catalog
                        // in RuleSet::bind_type_sound_references, using this type's
                        // retained INI read history rather than an unresolved name.
                    }
                }
                #[cfg(not(feature = "ares"))]
                let _ = effective;
            }
            index += 1;
        }
    }

    fn process_anim_family(&mut self, fixed_art: &IniFile) {
        let mut index = 0;
        while index < self.family_len(RulesTypeFamily::Animation) {
            let native_stored_id = self
                .families
                .get(&RulesTypeFamily::Animation)
                .and_then(|members| members.get(index))
                .map(|member| member.native_stored_id.clone());
            if let Some(section) = native_stored_id
                .as_deref()
                .and_then(|identity| fixed_art.section(identity))
            {
                // ReadTypeData 0x00679A5D..0x00679A82 visits the live Anim
                // registry before Techno readers can allocate later roots.
                // Anim ReadINI 0x00427D22 refuses a missing ART section via
                // AbstractType's lookup at 0x00410A7F. Record this actual visit,
                // not membership or eventual ART existence. See
                // .local/anim-type-art-read-acceptance.md.
                let member = &mut self.family_mut(RulesTypeFamily::Animation)[index];
                member.anim_art_read = true;
                let mut references = std::mem::take(&mut member.anim_references);
                for (key, family, field) in [
                    ("Next", RulesTypeFamily::Animation, &mut references.next),
                    ("Spawns", RulesTypeFamily::Animation, &mut references.spawns),
                    (
                        "TiberiumSpawnType",
                        RulesTypeFamily::Overlay,
                        &mut references.tiberium_spawn_type,
                    ),
                    (
                        "BounceAnim",
                        RulesTypeFamily::Animation,
                        &mut references.bounce_anim,
                    ),
                    (
                        "ExpireAnim",
                        RulesTypeFamily::Animation,
                        &mut references.expire_anim,
                    ),
                    (
                        "TrailerAnim",
                        RulesTypeFamily::Animation,
                        &mut references.trailer_anim,
                    ),
                    ("Warhead", RulesTypeFamily::Warhead, &mut references.warhead),
                ] {
                    if let Some(resolved) = self.resolve_scalar_from(section, key, family, 0x80) {
                        *field = resolved;
                    }
                }
                // ReadString with no default (0x00428716) skips an empty
                // read; any other name goes to ParticleType's factory
                // (0x00428730), whose answer is stored, null only for `<none>`.
                let spawns_particle = section.read_string("SpawnsParticle", "", 0x20);
                if !spawns_particle.is_empty() {
                    references.spawns_particle = self
                        .find_or_allocate_particle(&spawns_particle)
                        .map(|particle| {
                            self.families[&RulesTypeFamily::Particle][particle]
                                .native_stored_id
                                .clone()
                        });
                }
                self.family_mut(RulesTypeFamily::Animation)[index].anim_references = references;
            }
            index += 1;
        }
    }

    fn process_techno_family(
        &mut self,
        family: RulesTypeFamily,
        pass: &IniFile,
        fixed_art: &IniFile,
    ) {
        let mut index = 0;
        while index < self.family_len(family) {
            if let Some((native_stored_id, raw, effective)) =
                self.begin_rules_member_read(family, index, pass)
            {
                self.process_techno_base(family, index, &raw);
                // TechnoTypeClass::ReadINI parses `Prerequisite=` (`0x007141C5`)
                // and `PrerequisiteOverride=` (`0x0071424B`) after `Dock=` and
                // `DeploysInto=` (`0x00713180`, `0x00713279`), the reads above
                // that can allocate a BuildingType, each with its current list
                // as the default.
                let member = &self.families[&family][index];
                let prerequisite_capacity = if crate::rules::ARES_ENABLED {
                    0x800
                } else {
                    0x80
                };
                let prerequisite = parse_prerequisites(
                    raw.read_list("Prerequisite", prerequisite_capacity),
                    &member.prerequisite,
                    |name| self.building_index(name),
                    self.ares_prerequisites.as_ref(),
                );
                let prerequisite_override = parse_prerequisites(
                    raw.read_list("PrerequisiteOverride", prerequisite_capacity),
                    &member.prerequisite_override,
                    |name| self.building_index(name),
                    self.ares_prerequisites.as_ref(),
                );
                let member = &mut self.families.get_mut(&family).unwrap()[index];
                member.prerequisite = prerequisite;
                member.prerequisite_override = prerequisite_override;
                self.families.get_mut(&family).unwrap()[index]
                    .recoil
                    .apply_pass(&raw);
                self.families.get_mut(&family).unwrap()[index]
                    .gunner_turrets
                    .apply_pass(
                        &native_stored_id,
                        &raw,
                        family.object_category().expect("TechnoType family"),
                    );
                if family == RulesTypeFamily::Infantry {
                    let member = &mut self.families.get_mut(&family).unwrap()[index];
                    member.infantry_pips.apply_pass(&raw);
                    member.infantry_infiltration.apply_pass(&raw);
                }
                // ObjectType ReadINI (`0x005F92F8..0x005F933B`) reads Image
                // into its 25-byte +1F8 over a copy of the retained text, so a
                // pass without the key keeps it. It names each fixed ART
                // section below; the later ART Image redirect is local to
                // visual asset loading.
                let references = &mut self.family_mut(family)[index].techno_references;
                let image = raw.read_string("Image", &references.image, 0x19);
                references.image.clone_from(&image);
                let member = &mut self.families.get_mut(&family).unwrap()[index];
                member.voxel_shadow_index = fixed_art
                    .section_or_empty(&image)
                    .read_int("ShadowIndex", member.voxel_shadow_index);
                match family {
                    RulesTypeFamily::Building => {
                        let member = &mut self.families.get_mut(&family).unwrap()[index];
                        // ReadINI460F50..460F9C: exact Rules key, ReadCoord529CA0,
                        // current value as default, no clamp or conversion.
                        member.building_has_spotlight =
                            raw.read_bool("HasSpotlight", member.building_has_spotlight);
                        member.building_light.read_pass(&raw);
                        member
                            .building_upgrade
                            .read_pass(&raw, fixed_art.section_or_empty(&image));
                        member.building_target_coord_offset = raw
                            .read_coord3("TargetCoordOffset", member.building_target_coord_offset);
                        member.building_foundation =
                            crate::rules::foundation::read_building_foundation(
                                member.building_foundation,
                                &image,
                                &native_stored_id,
                                |section, current| {
                                    crate::rules::foundation::read_foundation(
                                        fixed_art.section_or_empty(section),
                                        current,
                                    )
                                },
                            );
                        if let Some(unit) = self.resolve_scalar_from(
                            &raw,
                            "FreeUnit",
                            RulesTypeFamily::Vehicle,
                            0x80,
                        ) {
                            self.family_mut(family)[index].techno_references.free_unit = unit;
                        }
                        // Building4605BB..460683: three ReadString128 and
                        // FindOrAllocate calls retain empty reads and clear
                        // authored none. Their families and order are native.
                        for (slot, key, target_family) in [
                            (0, "SecretInfantry", RulesTypeFamily::Infantry),
                            (1, "SecretUnit", RulesTypeFamily::Vehicle),
                            (2, "SecretBuilding", RulesTypeFamily::Building),
                        ] {
                            if let Some(value) =
                                self.resolve_scalar_from(&raw, key, target_family, 0x80)
                            {
                                self.families.get_mut(&family).unwrap()[index]
                                    .secret_lab
                                    .set_override(slot, value);
                            }
                        }
                        self.families.get_mut(&family).unwrap()[index]
                            .secret_lab
                            .read_flag(&raw);
                        // ToOverlay's ART section is +1F8 (0x004610DE).
                        if let Some(overlay) = self.resolve_fixed_art_reference(
                            fixed_art,
                            &image,
                            "ToOverlay",
                            RulesTypeFamily::Overlay,
                        ) {
                            self.family_mut(family)[index].techno_references.to_overlay = overlay;
                        }
                    }
                    RulesTypeFamily::Aircraft => {
                        // Trailer's ART section is +1F8 (0x0041CCB5). Only
                        // the allocation is kept: VERA ports no reader of the
                        // aircraft's Trailer pointer.
                        let _ = self.resolve_fixed_art_reference(
                            fixed_art,
                            &image,
                            "Trailer",
                            RulesTypeFamily::Animation,
                        );
                    }
                    RulesTypeFamily::Vehicle => {
                        let member = &mut self.families.get_mut(&family).unwrap()[index];
                        member.unit_speed_type = member
                            .unit_speed_type
                            .read_vehicle_pass(&raw, effective.read_bool("Crusher", false));
                        // Unit7477C1 uses the +1F8 section without a type-name
                        // fallback. The existing reached-body projection
                        // retains the Turret default across only the admitted
                        // Rules passes.
                        let has_turret = effective.read_bool("Turret", false);
                        self.families.get_mut(&family).unwrap()[index]
                            .unit_shp
                            .read_pass(fixed_art.section_or_empty(&image), has_turret);
                    }
                    RulesTypeFamily::Infantry => {
                        if let Some(weapon) = self.resolve_scalar_from(
                            &raw,
                            "OccupyWeapon",
                            RulesTypeFamily::Weapon,
                            0x80,
                        ) {
                            self.family_mut(family)[index]
                                .techno_references
                                .occupy_weapon = weapon;
                        }
                        if let Some(weapon) = self.resolve_scalar_from(
                            &raw,
                            "EliteOccupyWeapon",
                            RulesTypeFamily::Weapon,
                            0x80,
                        ) {
                            self.family_mut(family)[index]
                                .techno_references
                                .elite_occupy_weapon = weapon;
                        }
                        if let Some(anims) = self.resolve_list_from(
                            &raw,
                            "DeadBodies",
                            RulesTypeFamily::Animation,
                            0x80,
                        ) {
                            self.family_mut(family)[index].techno_references.dead_bodies = anims;
                        }
                        self.allocate_list_from(
                            &raw,
                            "DeathAnims",
                            RulesTypeFamily::Animation,
                            0x80,
                        );
                    }
                    _ => unreachable!("only Techno families enter the Techno reader"),
                }
            }
            index += 1;
        }
    }

    fn process_techno_base(&mut self, family: RulesTypeFamily, index: usize, raw: &IniSection) {
        if let Some(weapon) =
            self.resolve_scalar_from(raw, "DeathWeapon", RulesTypeFamily::Weapon, 0x80)
        {
            self.family_mut(family)[index]
                .techno_references
                .death_weapon = weapon;
        }
        if let Some(types) =
            self.resolve_list_from(raw, "DebrisTypes", RulesTypeFamily::VoxelAnimation, 0x80)
        {
            self.family_mut(family)[index]
                .techno_references
                .debris_types = types;
        }
        if let Some(anims) =
            self.resolve_list_from(raw, "DebrisAnims", RulesTypeFamily::Animation, 0x80)
        {
            self.family_mut(family)[index]
                .techno_references
                .debris_anims = anims;
        }

        // TechnoType ReadINI 0x0071284A..0x00712AAD. IsChargeTurret, read at
        // 0x0071287E between WeaponCount and ClearAllWeapons, is GunnerTurrets'.
        let mut slots = std::mem::take(&mut self.family_mut(family)[index].weapon_slots);
        slots.turret_count = raw.read_int("TurretCount", slots.turret_count);
        slots.weapon_count = raw.read_int("WeaponCount", slots.weapon_count);
        slots.clear_all_weapons = raw.read_bool("ClearAllWeapons", slots.clear_all_weapons);
        if slots.turret_count > 0 {
            // 0x007128C0..0x0071299E: Weapon%d then EliteWeapon%d for
            // 1..=WeaponCount, each ReadString128 empty read keeping the slot.
            for slot in 1..=slots.weapon_count {
                let base = self.resolve_scalar_from(
                    raw,
                    &format!("Weapon{slot}"),
                    RulesTypeFamily::Weapon,
                    0x80,
                );
                let elite = self.resolve_scalar_from(
                    raw,
                    &format!("EliteWeapon{slot}"),
                    RulesTypeFamily::Weapon,
                    0x80,
                );
                // RESIDUAL: native has no bound, so WeaponCount > 18 stores
                // past both arrays (slot 19's base write lands on
                // ClearAllWeapons at +A90). VERA allocates every authored slot
                // but keeps the first 18. Retail's largest WeaponCount is 17.
                let Some(slot) = usize::try_from(slot - 1)
                    .ok()
                    .filter(|&slot| slot < WEAPON_SLOT_COUNT)
                else {
                    continue;
                };
                if let Some(weapon) = base {
                    slots.base[slot] = weapon;
                }
                if let Some(weapon) = elite {
                    slots.elite[slot] = weapon;
                }
            }
        } else if !slots.clear_all_weapons {
            // 0x007129AB..0x00712A89: Primary/Secondary are base slots 0/1,
            // ElitePrimary/EliteSecondary elite slots 0/1.
            for (key, elite, slot) in [
                ("Primary", false, 0),
                ("Secondary", false, 1),
                ("ElitePrimary", true, 0),
                ("EliteSecondary", true, 1),
            ] {
                if let Some(weapon) =
                    self.resolve_scalar_from(raw, key, RulesTypeFamily::Weapon, 0x80)
                {
                    let array = if elite {
                        &mut slots.elite
                    } else {
                        &mut slots.base
                    };
                    array[slot] = weapon;
                }
            }
        }
        if slots.clear_all_weapons {
            // 0x00712A8F..0x00712AAD clears slots 0 and 1 after either branch.
            for array in [&mut slots.base, &mut slots.elite] {
                array[0] = None;
                array[1] = None;
            }
        }
        self.family_mut(family)[index].weapon_slots = slots;

        if let Some(buildings) =
            self.resolve_list_from(raw, "Dock", RulesTypeFamily::Building, 0x80)
        {
            self.family_mut(family)[index].techno_references.dock = buildings;
        }
        if let Some(building) =
            self.resolve_scalar_from(raw, "DeploysInto", RulesTypeFamily::Building, 0x80)
        {
            self.family_mut(family)[index]
                .techno_references
                .deploys_into = building;
        }
        if let Some(unit) =
            self.resolve_scalar_from(raw, "UndeploysInto", RulesTypeFamily::Vehicle, 0x80)
        {
            self.family_mut(family)[index]
                .techno_references
                .undeploys_into = unit;
        }
        if let Some(unit) =
            self.resolve_scalar_from(raw, "PowersUnit", RulesTypeFamily::Vehicle, 0x80)
        {
            self.family_mut(family)[index].techno_references.powers_unit = unit;
        }
        if let Some(anims) =
            self.resolve_list_from(raw, "Explosion", RulesTypeFamily::Animation, 0x80)
        {
            self.family_mut(family)[index].techno_references.explosion = anims;
        }
        if let Some(anims) =
            self.resolve_list_from(raw, "DestroyAnim", RulesTypeFamily::Animation, 0x80)
        {
            self.family_mut(family)[index]
                .techno_references
                .destroy_anim = anims;
        }
        if let Some(system) = self.resolve_scalar_from(
            raw,
            "NaturalParticleSystem",
            RulesTypeFamily::ParticleSystem,
            0x80,
        ) {
            self.family_mut(family)[index]
                .techno_references
                .natural_particle_system = system;
        }
        if let Some(system) = self.resolve_scalar_from(
            raw,
            "RefinerySmokeParticleSystem",
            RulesTypeFamily::ParticleSystem,
            0x80,
        ) {
            self.family_mut(family)[index]
                .techno_references
                .refinery_smoke_particle_system = system;
        }
        if let Some(systems) = self.resolve_list_from(
            raw,
            "DamageParticleSystems",
            RulesTypeFamily::ParticleSystem,
            0x80,
        ) {
            self.family_mut(family)[index]
                .techno_references
                .damage_particle_systems = systems;
        }
        if let Some(systems) = self.resolve_list_from(
            raw,
            "DestroyParticleSystems",
            RulesTypeFamily::ParticleSystem,
            0x80,
        ) {
            self.family_mut(family)[index]
                .techno_references
                .destroy_particle_systems = systems;
        }
        // ReadINI714570..71458A passes the current+634 to ReadInt5276D0.
        // An absent exact-case key (also physically blank values omitted by
        // the INI loader) retains it. Present integers stay signed/unclamped.
        // This precedes the AirstrikeTeam constructor reads.
        let member = &mut self.family_mut(family)[index];
        member.lepton_mind_control_offset =
            raw.read_int("LeptonMindControlOffset", member.lepton_mind_control_offset);
        member.tech_level = raw.read_int("TechLevel", member.tech_level);

        // Preserve both the six-field fallback order and Aircraft constructor
        // chronology at TechnoType71458A..714654. Later text merging cannot
        // recover an elite value reset by an omitted key in a reached section.
        let mut airstrike = std::mem::take(&mut self.family_mut(family)[index].airstrike);
        airstrike.apply_pass(raw, |name| {
            self.find_or_allocate(RulesTypeFamily::Aircraft, name)
                .map(|aircraft| {
                    self.families[&RulesTypeFamily::Aircraft][aircraft]
                        .native_stored_id
                        .clone()
                })
        });
        self.family_mut(family)[index].airstrike = airstrike;
        if let Some(unit) =
            self.resolve_scalar_from(raw, "UnloadingClass", RulesTypeFamily::Vehicle, 0x80)
        {
            self.family_mut(family)[index]
                .techno_references
                .unloading_class = unit;
        }
        if let Some(anim) =
            self.resolve_scalar_from(raw, "DeployingAnim", RulesTypeFamily::Animation, 0x80)
        {
            self.family_mut(family)[index]
                .techno_references
                .deploying_anim = anim;
        }
        if let Some(infantry) =
            self.resolve_scalar_from(raw, "Enslaves", RulesTypeFamily::Infantry, 0x80)
        {
            self.family_mut(family)[index].techno_references.enslaves = infantry;
        }
        if let Some(aircraft) =
            self.resolve_scalar_from(raw, "Spawns", RulesTypeFamily::Aircraft, 0x80)
        {
            self.family_mut(family)[index].techno_references.spawns = aircraft;
        }
        // Original ReadINI7147B4..7147C8 stores ReadInt5276D0 directly.
        // Missing sections never enter this reader; omitted keys retain the
        // prior value, including across process-resident registry handoffs.
        let member = &mut self.family_mut(family)[index];
        member.radial_fire_segments =
            raw.read_int("RadialFireSegments", member.radial_fire_segments);
    }

    /// A reference read from the fixed ART section the type's Image names; a
    /// missing section reads nothing.
    fn resolve_fixed_art_reference(
        &mut self,
        fixed_art: &IniFile,
        image: &str,
        key: &str,
        family: RulesTypeFamily,
    ) -> Option<Option<String>> {
        if image.is_empty() {
            return None;
        }
        self.resolve_scalar_from(fixed_art.section(image)?, key, family, 0x80)
    }

    fn process_weapon_family(&mut self, pass: &IniFile) {
        let mut index = 0;
        while index < self.family_len(RulesTypeFamily::Weapon) {
            if let Some((_id, raw, _effective)) =
                self.begin_rules_member_read(RulesTypeFamily::Weapon, index, pass)
            {
                // 7722D4..772350: these scalar reads precede the later
                // constructor-capable animation and type-reference reads.
                let state = &mut self.family_mut(RulesTypeFamily::Weapon)[index].weapon;
                state.damage = raw.read_int("Damage", state.damage);
                state.speed = raw.read_speed("Speed", state.speed);
                state.range_leptons = raw.read_range("Range", state.range_leptons);
                if let Some(anims) =
                    self.resolve_list_from(&raw, "Anim", RulesTypeFamily::Animation, 0x80)
                {
                    self.family_mut(RulesTypeFamily::Weapon)[index].weapon.anim = anims;
                }
                // AssaultAnim (`0x00772574`) is allocated but not retained:
                // VERA ports no reader of the WeaponType+114 it stores.
                let _ =
                    self.resolve_scalar_from(&raw, "AssaultAnim", RulesTypeFamily::Animation, 0x80);
                if let Some(anim) =
                    self.resolve_scalar_from(&raw, "OccupantAnim", RulesTypeFamily::Animation, 0x80)
                {
                    self.family_mut(RulesTypeFamily::Weapon)[index]
                        .weapon
                        .occupant_anim = anim;
                }
                if let Some(anim) = self.resolve_scalar_from(
                    &raw,
                    "OpenToppedAnim",
                    RulesTypeFamily::Animation,
                    0x80,
                ) {
                    self.family_mut(RulesTypeFamily::Weapon)[index]
                        .weapon
                        .open_topped_anim = anim;
                }
                let incoming = raw.read_string("AttachedParticleSystem", "", 0x14);
                if let Some(system) = self.find_or_create_particle_system(&incoming) {
                    let id = self.families[&RulesTypeFamily::ParticleSystem][system]
                        .native_stored_id
                        .clone();
                    self.family_mut(RulesTypeFamily::Weapon)[index]
                        .weapon
                        .attached_particle_system = Some(id);
                }
                let incoming_warhead = raw.read_string("Warhead", "", 0x80);
                let warhead = (!incoming_warhead.is_empty())
                    .then(|| self.find_or_allocate(RulesTypeFamily::Warhead, &incoming_warhead));
                let incoming = raw.read_string("Projectile", "", 0x80);
                let projectile = (!incoming.is_empty())
                    .then(|| self.find_or_allocate(RulesTypeFamily::Projectile, &incoming));
                let state = &mut self.family_mut(RulesTypeFamily::Weapon)[index].weapon;
                if let Some(projectile) = projectile {
                    state.projectile = projectile;
                }
                if let Some(warhead) = warhead {
                    state.warhead = warhead;
                }
            }
            index += 1;
        }
    }

    /// WeaponType7729F0 writes +A8 only for a nonnull projectile with ROT=0.
    /// Original chronology and arithmetic: rules_oracle/weapon_speed_order.
    fn postprocess_weapon_speeds(&mut self) {
        for index in 0..self.family_len(RulesTypeFamily::Weapon) {
            let state = &self.families[&RulesTypeFamily::Weapon][index].weapon;
            let Some(projectile) = state.projectile else {
                continue;
            };
            let bullet = &self.families[&RulesTypeFamily::Projectile][projectile].body;
            if bullet.read_int("ROT", 0) == 0 {
                let speed = crate::util::native_ballistics::ballistic_launch_speed(
                    state.range_leptons,
                    self.rules_gravity,
                    bullet.read_bool("Floater", false),
                );
                self.family_mut(RulesTypeFamily::Weapon)[index].weapon.speed = speed;
            }
        }
    }

    fn process_bullet_family(&mut self, pass: &IniFile, fixed_art: &IniFile) {
        // Colors66D3A0 precedes this sweep. Own the small name list while
        // reference readers below may allocate more native type members.
        let color_names: Vec<_> = self.colors.iter().map(|(name, _)| name.clone()).collect();
        let mut index = 0;
        while index < self.family_len(RulesTypeFamily::Projectile) {
            if let Some((_id, raw, _effective)) =
                self.begin_rules_member_read(RulesTypeFamily::Projectile, index, pass)
            {
                // Full native body46BEE0: base Object ART reads precede the
                // clearing Image25 read and byte-narrowed Bullet ART fields.
                // Keep type-reference allocation here in native read order.
                let mut art = self.families[&RulesTypeFamily::Projectile][index]
                    .projectile_art
                    .clone();
                art.read_pass(&raw, fixed_art, &color_names, |incoming| {
                    let animation = self.find_or_allocate(RulesTypeFamily::Animation, incoming)?;
                    Some(
                        self.families[&RulesTypeFamily::Animation][animation]
                            .native_stored_id
                            .clone(),
                    )
                });
                self.families
                    .get_mut(&RulesTypeFamily::Projectile)
                    .expect("the live BulletType member exists")[index]
                    .projectile_art = art;
                let _ =
                    self.resolve_scalar_from(&raw, "AirburstWeapon", RulesTypeFamily::Weapon, 0x80);
                if let Some(weapon) =
                    self.resolve_scalar_from(&raw, "ShrapnelWeapon", RulesTypeFamily::Weapon, 0x80)
                {
                    self.family_mut(RulesTypeFamily::Projectile)[index].shrapnel_weapon = weapon;
                }
            }
            index += 1;
        }
    }

    fn process_warhead_family(&mut self, pass: &IniFile) {
        let mut index = 0;
        while index < self.family_len(RulesTypeFamily::Warhead) {
            if let Some((_id, raw, _effective)) =
                self.begin_rules_member_read(RulesTypeFamily::Warhead, index, pass)
            {
                let _ = self.resolve_scalar_from(
                    &raw,
                    "Particle",
                    RulesTypeFamily::ParticleSystem,
                    0x80,
                );
                // Warhead ctor75CF89/75CFB3 initializes both false. The
                // original reader uses the live field as its ReadBool default.
                let current = &mut self.family_mut(RulesTypeFamily::Warhead)[index].warhead_anim;
                current.conventional = raw.read_bool("Conventional", current.conventional);
                if let Some(list) =
                    self.resolve_list_from(&raw, "AnimList", RulesTypeFamily::Animation, 0x80)
                {
                    self.family_mut(RulesTypeFamily::Warhead)[index]
                        .warhead_anim
                        .anim_list = list;
                }
                let current = &mut self.family_mut(RulesTypeFamily::Warhead)[index].warhead_anim;
                current.em_effect = raw.read_bool("EMEffect", current.em_effect);
                if let Some(types) = self.resolve_list_from(
                    &raw,
                    "DebrisTypes",
                    RulesTypeFamily::VoxelAnimation,
                    0x80,
                ) {
                    self.family_mut(RulesTypeFamily::Warhead)[index]
                        .warhead_anim
                        .debris_types = types;
                }
                // Installed Ares75DDCC->10054410 skips the entire original
                // raw11 read and its+149 derivation. Its independent vector
                // is read below even when the original section was absent.
                #[cfg(not(feature = "ares"))]
                {
                    let current =
                        &mut self.family_mut(RulesTypeFamily::Warhead)[index].warhead_verses;
                    *current = crate::rules::warhead_type::read_verses(
                        &raw,
                        current.map(|value| f64::from_bits(value.bits())),
                    )
                    .map(|value| NativeF64Bits::from_bits(value.to_bits()));
                }
            }
            #[cfg(feature = "ares")]
            {
                let member = &mut self
                    .families
                    .get_mut(&RulesTypeFamily::Warhead)
                    .expect("Warhead family exists during its reader")[index];
                let section = pass.section(&member.native_stored_id);
                member
                    .ares_verses
                    .as_mut()
                    .expect("Ares Warhead constructor installs its extension")
                    .read_pass(
                        section,
                        self.ares_armors
                            .as_ref()
                            .expect("Ares reader requires its armor catalog"),
                    );
            }
            index += 1;
        }
    }

    fn process_plain_family(&mut self, family: RulesTypeFamily, pass: &IniFile) {
        let mut index = 0;
        while index < self.family_len(family) {
            self.begin_rules_member_read(family, index, pass);
            index += 1;
        }
    }

    fn process_particle_family(&mut self, pass: &IniFile) {
        let mut index = 0;
        while index < self.family_len(RulesTypeFamily::Particle) {
            if let Some((_id, raw, _effective)) =
                self.begin_rules_member_read(RulesTypeFamily::Particle, index, pass)
                && let Some(warhead) =
                    self.resolve_scalar_from(&raw, "Warhead", RulesTypeFamily::Warhead, 0x80)
            {
                self.family_mut(RulesTypeFamily::Particle)[index].particle_warhead = warhead;
            }
            index += 1;
        }
    }

    fn process_particle_system_family(&mut self, pass: &IniFile) {
        let mut index = 0;
        while index < self.family_len(RulesTypeFamily::ParticleSystem) {
            if let Some((_id, raw, _effective)) =
                self.begin_rules_member_read(RulesTypeFamily::ParticleSystem, index, pass)
            {
                // ParticleSystemType ReadINI 0x0064430F..0x00644323: ReadString
                // into 0x40 bytes over the shared empty default 0x00889F64,
                // then an unconditional ParticleType FindOrAllocate whose index
                // replaces +294. A reached pass without HoldsWhat therefore
                // binds the empty-named ParticleType.
                let holds_what = raw.read_string("HoldsWhat", "", 0x40);
                let particle = self.find_or_allocate_particle(&holds_what);
                self.family_mut(RulesTypeFamily::ParticleSystem)[index].holds_what = particle;
            }
            index += 1;
        }
    }

    fn process_voxel_anim_family(&mut self, pass: &IniFile) {
        let mut index = 0;
        while index < self.family_len(RulesTypeFamily::VoxelAnimation) {
            if let Some((_id, raw, _effective)) =
                self.begin_rules_member_read(RulesTypeFamily::VoxelAnimation, index, pass)
            {
                let mut references = std::mem::take(
                    &mut self.family_mut(RulesTypeFamily::VoxelAnimation)[index]
                        .voxel_anim_references,
                );
                for (key, family, field) in [
                    (
                        "Spawns",
                        RulesTypeFamily::VoxelAnimation,
                        &mut references.spawns,
                    ),
                    (
                        "BounceAnim",
                        RulesTypeFamily::Animation,
                        &mut references.bounce_anim,
                    ),
                    (
                        "ExpireAnim",
                        RulesTypeFamily::Animation,
                        &mut references.expire_anim,
                    ),
                    (
                        "TrailerAnim",
                        RulesTypeFamily::Animation,
                        &mut references.trailer_anim,
                    ),
                    ("Warhead", RulesTypeFamily::Warhead, &mut references.warhead),
                    (
                        "AttachedSystem",
                        RulesTypeFamily::ParticleSystem,
                        &mut references.attached_system,
                    ),
                ] {
                    if let Some(resolved) = self.resolve_scalar_from(&raw, key, family, 0x80) {
                        *field = resolved;
                    }
                }
                self.family_mut(RulesTypeFamily::VoxelAnimation)[index].voxel_anim_references =
                    references;
            }
            index += 1;
        }
    }

    fn allocate_combat_references(&mut self, pass: &IniFile) {
        let Some(section) = pass.section("CombatDamage") else {
            return;
        };
        // CombatDamage66BBF8..66BC12: exact-case signed ReadInt5276D0,
        // current Rules+490 default, constructor665CD4=700, no clamp.
        self.rules_ion_cannon_damage =
            section.read_int("IonCannonDamage", self.rules_ion_cannon_damage);
        for key in [
            "Scorches",
            "Scorches1",
            "Scorches2",
            "Scorches3",
            "Scorches4",
        ] {
            self.allocate_list_from(section, key, RulesTypeFamily::Smudge, 0x80);
        }
        // CombatDamage66C184..66C287: ctor-empty vector, nonempty
        // ReadString128 replaces it using untrimmed strtok tokens/factories.
        if let Some(list) =
            self.resolve_list_from(section, "SplashList", RulesTypeFamily::Animation, 0x80)
        {
            self.select_anim.splash_list = list;
        }
        // FlameDamage (`0x0066C2A1`) and FlameDamage2 are allocated but not
        // retained: VERA ports no reader of either pointer.
        for key in ["FlameDamage", "FlameDamage2"] {
            let _ = self.resolve_scalar_from(section, key, RulesTypeFamily::Warhead, 0x80);
        }
        for key in ["C4Warhead", "CrushWarhead"] {
            let warhead = self.resolve_scalar_from(section, key, RulesTypeFamily::Warhead, 0x80);
            self.rules_type_references.read_pointer(key, warhead);
        }
        // 0x0066C36A..0x0066C4F4: the six rocket warheads, in this order.
        let mut missile_spawn = std::mem::take(&mut self.missile_spawn);
        missile_spawn.read_warheads(|key| {
            self.resolve_scalar_from(section, key, RulesTypeFamily::Warhead, 0x80)
        });
        self.missile_spawn = missile_spawn;
        let warhead =
            self.resolve_scalar_from(section, "IvanWarhead", RulesTypeFamily::Warhead, 0x80);
        self.rules_type_references
            .read_pointer("IvanWarhead", warhead);
        if let Some(weapon) =
            self.resolve_scalar_from(section, "DeathWeapon", RulesTypeFamily::Weapon, 0x80)
        {
            self.rules_death_weapon = weapon;
        }
        //66C914..66C92E: current signed default, no clamp, before DrainAnimationType.
        self.rules_mind_control_attack_line_frames = section.read_int(
            "MindControlAttackLineFrames",
            self.rules_mind_control_attack_line_frames,
        );
        for key in [
            "DrainAnimationType",
            "ControlledAnimationType",
            "PermaControlledAnimationType",
        ] {
            let anim = self.resolve_scalar_from(section, key, RulesTypeFamily::Animation, 0x80);
            self.rules_type_references.read_pointer(key, anim);
        }
        //66CA96..66CACB retains the current Rules+FF0 pointer on an
        //empty read; the existing allocating scalar reader handles none.
        if let Some(resolved) =
            self.resolve_scalar_from(section, "IonCannonWarhead", RulesTypeFamily::Warhead, 0x80)
        {
            self.rules_ion_cannon_warhead = resolved.unwrap_or_default();
        }
        for (key, _) in DEFAULT_PARTICLE_SYSTEMS {
            let system =
                self.resolve_scalar_from(section, key, RulesTypeFamily::ParticleSystem, 0x80);
            self.rules_type_references.read_pointer(key, system);
        }
    }

    fn allocate_radiation_references(&mut self, pass: &IniFile) {
        if let Some(section) = pass.section("Radiation") {
            let warhead =
                self.resolve_scalar_from(section, "RadSiteWarhead", RulesTypeFamily::Warhead, 0x80);
            self.rules_type_references
                .read_pointer("RadSiteWarhead", warhead);
        }
    }

    fn allocate_audio_visual_references(&mut self, pass: &IniFile) {
        let Some(section) = pass.section("AudioVisual") else {
            return;
        };
        // Original 66920D..669258 reads these first, including the cold
        // startup's same AudioVisual call before any complete Process.
        self.rules_detail = self.rules_detail.read_pass(section);
        // AudioVisual66B385..66B39E: ReadRange474620, current +14BC default.
        // Rules6672FF constructs1024 leptons; literal -1 retains the prior.
        self.rules_drop_zone_radius =
            section.read_range("DropZoneRadius", self.rules_drop_zone_radius);
        // Full AudioVisual6691E0's 66B3C4 read uses the retained signed dword
        // default. Cold startup52D132 calls the same reader before Process.
        self.rules_gravity = section.read_int("Gravity", self.rules_gravity);
        // AudioVisual66B5D7..66B625: rate then step; both ReadDouble defaults
        // are the current Rules fields. Missing reads retain; malformed float
        // output is native undefined stack data, using ini_value's safe policy.
        // Original physical/retained controls: trigger_map_lighting.json (+meta).
        self.rules_ambient_change_rate =
            section.read_double_bits("AmbientChangeRate", self.rules_ambient_change_rate);
        self.rules_ambient_change_step =
            section.read_double_bits("AmbientChangeStep", self.rules_ambient_change_step);
        // AudioVisual66B77D..66B7A7, after Gravity, retains the RGB default.
        self.rules_line_trail_override = section.read_color_rgb(
            "LineTrailColorOverride",
            self.rules_line_trail_override,
            None,
        );
        // AudioVisual66B7A7..66B7D2 reads this next, even without General.
        // Its default is the current unsigned RGB; complete scans narrow to
        // bytes. Incomplete scans retain ini_value's deterministic safe policy.
        self.rules_chrono_beam_color =
            section.read_color_rgb("ChronoBeamColor", self.rules_chrono_beam_color, None);
        self.rules_color_add_selectors.read_pass(section);
        for key in ["DropPodPuff", "VeinAttack", "Dig", "AtmosphereEntry"] {
            let _ = self.resolve_scalar_from(section, key, RulesTypeFamily::Animation, 0x80);
        }
        for key in ["TreeFire", "OnFire"] {
            self.allocate_list_from(section, key, RulesTypeFamily::Animation, 0x80);
        }
        let _ = self.resolve_scalar_from(section, "Smoke", RulesTypeFamily::Animation, 0x80);
        let _ = self.resolve_scalar_from(section, "Smoke", RulesTypeFamily::Animation, 0x80);
        for key in ["SmallFire", "LargeFire"] {
            let _ = self.resolve_scalar_from(section, key, RulesTypeFamily::Animation, 0x80);
        }
    }

    fn process_special_weapons(&mut self, pass: &IniFile) {
        let Some(section) = pass.section("SpecialWeapons") else {
            return;
        };
        for (key, family) in [
            ("NukeWarhead", RulesTypeFamily::Warhead),
            ("NukeProjectile", RulesTypeFamily::Projectile),
            ("NukeDown", RulesTypeFamily::Projectile),
            ("MutateWarhead", RulesTypeFamily::Warhead),
            ("MutateExplosionWarhead", RulesTypeFamily::Warhead),
            ("EMPulseWarhead", RulesTypeFamily::Warhead),
            ("EMPulseProjectile", RulesTypeFamily::Projectile),
        ] {
            // VERA ports no reader of the Nuke and EMPulse pointers; only
            // their allocations are kept.
            let resolved = self.resolve_scalar_from(section, key, family, 0x80);
            self.rules_type_references.read_pointer(key, resolved);
        }
        self.process_warhead_family(pass);
    }

    fn process_tiberiums(&mut self, pass: &IniFile) -> Result<(), RulesError> {
        let Some(registry) = pass.section("Tiberiums") else {
            return Ok(());
        };

        for key in registry.keys() {
            let slot = crate::rules::ini_value::crt_atoi(key);
            let identity = registry.read_string(key, "", 0x18);
            if identity.is_empty() {
                continue;
            }

            if slot < 0 {
                return Err(RulesError::InvalidValue {
                    section: "Tiberiums".to_string(),
                    key: key.to_string(),
                    expected: "a nonnegative native Tiberium slot".to_string(),
                    value: key.to_string(),
                });
            }

            let index = if slot < self.tiberiums.len() as i32 {
                slot as usize
            } else {
                self.tiberiums.push(ProcessedType::new(identity));
                self.tiberiums.len() - 1
            };
            let native_stored_id = self.tiberiums[index].native_stored_id.clone();
            if let Some(section) = pass.section(&native_stored_id).cloned() {
                self.tiberiums[index].body.overlay_rules_pass(&section);
                self.allocate_list_from(&section, "Debris", RulesTypeFamily::Animation, 0x80);
            }
        }
        Ok(())
    }

    fn finish(mut self) -> (IniFile, NativeTypeConstructionTrace, PowerupTable) {
        let allocated_super_weapon_type_count = self
            .families
            .get(&RulesTypeFamily::SuperWeapon)
            .map_or(0, Vec::len);
        let mut ini = self.ordinary.take().unwrap_or_else(IniFile::empty);

        for &(registry, family) in PROJECTED_RULE_TYPE_FAMILIES {
            let mut section = IniSection::new(registry.to_string());
            if let Some(members) = self.families.get(&family) {
                for (index, member) in members.iter().enumerate() {
                    section.set(&index.to_string(), &member.native_stored_id);
                }
            }
            ini.replace_first_section(section);
        }
        let mut tiberiums = IniSection::new("Tiberiums".to_string());
        for (index, member) in self.tiberiums.iter().enumerate() {
            tiberiums.set(&index.to_string(), &member.native_stored_id);
        }
        ini.replace_first_section(tiberiums);

        let mut colors = IniSection::new("Colors".to_string());
        for (name, value) in &self.colors {
            colors.set(name, value);
        }
        ini.replace_first_section(colors);

        // Replace every allocated type's ordinary text body with the keys that
        // its live object actually read after allocation. ReadTypeData hands
        // the Anim registry the ART INI (`push 0x00887180` at `0x00679A6D`),
        // so an AnimType's Rules body stays empty and must not replace the
        // body of an earlier type of the same name: retail `[KTSTLEXP]` is an
        // Animation and the elite Kirov bomb's warhead.
        let mut rules_bodies = HashSet::new();
        for &(_, family) in PROJECTED_RULE_TYPE_FAMILIES {
            if let Some(members) = self.families.get(&family) {
                for member in members {
                    if family != RulesTypeFamily::Animation {
                        rules_bodies.insert(member.native_stored_id.as_str());
                    } else if rules_bodies.contains(member.native_stored_id.as_str()) {
                        continue;
                    }
                    ini.replace_first_section(member.body.clone());
                }
            }
        }
        for family in [RulesTypeFamily::Weapon, RulesTypeFamily::Projectile] {
            for member in self.families.get(&family).into_iter().flatten() {
                ini.replace_first_section(member.body.clone());
            }
        }
        for member in &self.tiberiums {
            ini.replace_first_section(member.body.clone());
        }

        (
            ini,
            NativeTypeConstructionTrace {
                events: self.native_type_construction_events,
                allocated_super_weapon_type_count,
                registry_state: NativeRulesRegistryState {
                    compatibility: self.compatibility,
                    ares_prerequisites: self.ares_prerequisites,
                    ares_armors: self.ares_armors,
                    families: self.families,
                    techno_type_order: self.techno_type_order,
                    tiberiums: self.tiberiums,
                    colors: self.colors,
                    rules_color_add: self.rules_color_add,
                    rules_color_add_selectors: self.rules_color_add_selectors,
                    rules_detail: self.rules_detail,
                    rules_ambient_change_rate: self.rules_ambient_change_rate,
                    rules_ambient_change_step: self.rules_ambient_change_step,
                    rules_difficulty_rows: self.rules_difficulty_rows,
                    rules_gravity: self.rules_gravity,
                    rules_wind_direction: self.rules_wind_direction,
                    rules_missile_rot_var: self.rules_missile_rot_var,
                    rules_safety_altitude: self.rules_safety_altitude,
                    rules_parachute_max_fall_rate: self.rules_parachute_max_fall_rate,
                    rules_no_parachute_max_fall_rate: self.rules_no_parachute_max_fall_rate,
                    rules_survivor_divisors: self.rules_survivor_divisors,
                    rules_infantry_repair_step: self.rules_infantry_repair_step,
                    rules_unit_repair_rate: self.rules_unit_repair_rate,
                    rules_infantry_repair_rate: self.rules_infantry_repair_rate,
                    rules_maximum_cheer_rate: self.rules_maximum_cheer_rate,
                    rules_talk_bubble_time: self.rules_talk_bubble_time,
                    rules_spotlight: self.rules_spotlight,
                    rules_ion_cannon_damage: self.rules_ion_cannon_damage,
                    rules_ion_blast_anim: self.rules_ion_blast_anim,
                    rules_ion_beam_anim: self.rules_ion_beam_anim,
                    rules_ion_cannon_warhead: self.rules_ion_cannon_warhead,
                    rules_mind_control_attack_line_frames: self
                        .rules_mind_control_attack_line_frames,
                    rules_line_trail_override: self.rules_line_trail_override,
                    rules_chrono_beam_color: self.rules_chrono_beam_color,
                    rules_prism_support: self.rules_prism_support,
                    rules_patrol_scan_minutes: self.rules_patrol_scan_minutes,
                    rules_comp_easy_bonus: self.rules_comp_easy_bonus,
                    rules_prism_type: self.rules_prism_type,
                    rules_drop_zone_anim: self.rules_drop_zone_anim,
                    rules_drop_zone_radius: self.rules_drop_zone_radius,
                    rules_behind: self.rules_behind,
                    rules_prerequisite_proc_alternate: self.rules_prerequisite_proc_alternate,
                    rules_prerequisite_lists: self.rules_prerequisite_lists,
                    rules_building_types: self.rules_building_types,
                    select_anim: self.select_anim,
                    rules_death_weapon: self.rules_death_weapon,
                    rules_type_references: self.rules_type_references,
                    rules_crate: self.crate_rules,
                    rules_missile_spawn: self.missile_spawn,
                    rules_general_type_lists: self.general_type_lists,
                },
            },
            self.powerups.finish(),
        )
    }
}

#[cfg(test)]
#[path = "native_processing_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "side_registry_tests.rs"]
mod side_registry_tests;

#[cfg(test)]
#[path = "native_tech_level_tests.rs"]
mod native_tech_level_tests;

#[cfg(test)]
#[path = "projectile_art_tests.rs"]
mod projectile_art_tests;

#[cfg(test)]
#[path = "weapon_speed_tests.rs"]
mod weapon_speed_tests;

#[cfg(test)]
#[path = "weapon_registry_tests.rs"]
mod weapon_registry_tests;

#[cfg(test)]
#[path = "guided_controls_tests.rs"]
mod guided_controls_tests;

#[cfg(test)]
#[path = "select_anim_rules_tests.rs"]
mod select_anim_rules_tests;

#[cfg(test)]
#[path = "approach_rules_tests.rs"]
mod approach_rules_tests;

#[cfg(test)]
#[path = "chrono_beam_rules_tests.rs"]
mod chrono_beam_rules_tests;

#[cfg(test)]
#[path = "control_line_rules_tests.rs"]
mod control_line_rules_tests;

#[cfg(test)]
#[path = "native_type_reader_tests.rs"]
mod native_type_reader_tests;
