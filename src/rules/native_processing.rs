//! Active-YR RulesClass processing: ordered passes, live Type registries,
//! constructor chronology and typed-reader compatibility projections.
//!
//! Raw INI storage stays with ini_parser; process_owner retains registry lifetime;
//! ruleset converts this processed output into gameplay definitions. Failed passes
//! preserve already-constructed registry state rather than rolling back.

use crate::rules::crate_rules::{CrateRules, CrateRulesAccumulator};
use crate::rules::error::RulesError;
use crate::rules::gunner_turrets::GunnerTurrets;
use crate::rules::ini_parser::{IniFile, IniSection, is_native_none_type_name};

use crate::rules::missile_spawn::MissileSpawnRules;
use crate::rules::object_type::ObjectCategory;
use crate::rules::powerups::{PowerupTable, PowerupsAccumulator};
use crate::rules::prerequisite::{Prerequisite, PrerequisiteGroup, parse_prerequisites};
use crate::rules::projectile_type::ProjectileArtState;
use crate::rules::ruleset::{DetailRules, GeneralBuildingTypes, PrismSupportRules};
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

/// One native `RulesClass::Process` source in its runtime position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RulesLayerKind {
    RulesMd,
    LangRule,
    GameMode,
    Scenario,
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
        self.process_with_fixed_art_and_registry_state_recovering(fixed_art, registry_state)
            .map_err(NativeRulesProcessingFailure::into_error)
    }

    /// Continue Process calls while retaining the sole native registry owner
    /// when a later pass fails.
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
    ) -> Result<ProcessedRulesLayers, NativeRulesProcessingFailure> {
        let mut processor = RulesPassProcessor::with_registry_state(registry_state);
        for (_, ini) in self.iter_passes() {
            if let Err(error) = processor.apply_pass(ini, fixed_art) {
                let (_, partial_trace, _, _, _, _) = processor.finish();
                return Err(NativeRulesProcessingFailure {
                    error,
                    partial_trace,
                });
            }
        }
        let (
            ini,
            native_type_construction_trace,
            crate_rules,
            powerups,
            general_type_lists,
            missile_spawn,
        ) = processor.finish();
        Ok(ProcessedRulesLayers {
            ini,
            crate_rules,
            powerups,
            general_type_lists,
            missile_spawn,
            content_hash: self.content_hash(),
            native_type_construction_trace,
        })
    }
}

/// A failed native Process call plus the state already mutated before failure.
#[derive(Debug)]
pub(crate) struct NativeRulesProcessingFailure {
    error: RulesError,
    partial_trace: NativeTypeConstructionTrace,
}

impl NativeRulesProcessingFailure {
    pub(crate) fn into_parts(self) -> (RulesError, NativeTypeConstructionTrace) {
        (self.error, self.partial_trace)
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
    let (_, trace, _, _, _, _) = processor.finish();
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
        let (_, partial_trace, _, _, _, _) = processor.finish();
        return Err(NativeRulesProcessingFailure {
            error,
            partial_trace,
        });
    }
    let (_, trace, _, _, _, _) = processor.finish();
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
    let (_, trace, _, _, _, _) = processor.finish();
    (trace, [after_countries, after_general, after_house_bodies])
}

/// Retained RulesClass type vectors, distinct from the process-resident type
/// registries. The constructor @ 0x00665827 initializes them empty; ReadGeneral
/// @ 0x0066DA90/0x0066DB93 replaces MetallicDebris/BridgeExplosions only after
/// a nonempty ReadString128. Native execution:
/// tools/rules_oracle/bridge_anim_lists.{py,json}. WeatherConClouds
/// (`+0x2BC`, `0x0066DD28`) and WeatherConBolts (`+0x2D8`, `0x0066DE2B`) take
/// the same ReadString128/FindOrAllocate body.
#[derive(Debug, Default)]
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
#[derive(Debug)]
pub struct ProcessedRulesLayers {
    ini: IniFile,
    crate_rules: CrateRules,
    powerups: PowerupTable,
    general_type_lists: GeneralTypeLists,
    missile_spawn: MissileSpawnRules,
    content_hash: u64,
    native_type_construction_trace: NativeTypeConstructionTrace,
}

impl ProcessedRulesLayers {
    pub fn ini(&self) -> &IniFile {
        &self.ini
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

    /// Retained +A8 speed and +A0 projectile after the last Weapon sweep.
    pub(crate) fn weapon_speeds_and_projectiles(
        &self,
    ) -> impl Iterator<Item = (&str, i32, Option<&str>)> {
        let families = &self
            .native_type_construction_trace
            .registry_state()
            .families;
        families
            .get(&RulesTypeFamily::Weapon)
            .into_iter()
            .flatten()
            .map(move |member| {
                let projectile = member.weapon.projectile.map(|index| {
                    families[&RulesTypeFamily::Projectile][index]
                        .native_stored_id
                        .as_str()
                });
                (
                    member.native_stored_id.as_str(),
                    member.weapon.speed,
                    projectile,
                )
            })
    }

    pub(crate) fn warhead_anim_states(
        &self,
    ) -> impl Iterator<Item = (&str, bool, bool, &[String])> {
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
                )
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
    #[cfg(test)]
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
    pub(crate) fn country_registry_states(&self) -> impl Iterator<Item = (&str, i32, i32)> {
        self.native_type_construction_trace
            .registry_state()
            .families
            .get(&RulesTypeFamily::Country)
            .into_iter()
            .flatten()
            .map(|member| {
                (
                    member.native_stored_id.as_str(),
                    member.country_color_scheme,
                    member.country_side_index,
                )
            })
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

    pub(crate) fn projectile_rule_controls(&self) -> (f64, i32, [u8; 3]) {
        let state = self.native_type_construction_trace.registry_state();
        (
            state.rules_missile_rot_var,
            state.rules_safety_altitude,
            state.rules_line_trail_override,
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
        &self.crate_rules
    }

    pub fn powerups(&self) -> &PowerupTable {
        &self.powerups
    }

    /// The three `RocketStruct` blocks and their warheads after every pass.
    pub fn missile_spawn(&self) -> &MissileSpawnRules {
        &self.missile_spawn
    }

    pub(crate) fn metallic_debris(&self) -> &[String] {
        &self.general_type_lists.metallic_debris
    }

    pub(crate) fn bridge_explosions(&self) -> &[String] {
        &self.general_type_lists.bridge_explosions
    }

    pub(crate) fn weather_con_clouds(&self) -> &[String] {
        &self.general_type_lists.weather_con_clouds
    }

    pub(crate) fn weather_con_bolts(&self) -> &[String] {
        &self.general_type_lists.weather_con_bolts
    }

    /// The four `*ParaDropInf=` lists as stored type IDs: American, Allied,
    /// Soviet, Yuri.
    pub(crate) fn paradrop_infantry(&self) -> &[Vec<String>; 4] {
        &self.general_type_lists.paradrop_infantry
    }

    pub(crate) fn anim_to_infantry(&self) -> &[String] {
        &self.general_type_lists.anim_to_infantry
    }

    pub(crate) fn default_mirage_disguises(&self) -> &[String] {
        &self.general_type_lists.default_mirage_disguises
    }

    #[cfg(test)]
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
    #[cfg(test)]
    pub(crate) fn family(&self) -> NativeTypeConstructorFamily {
        self.family
    }

    #[cfg(test)]
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
#[derive(Debug)]
pub(crate) struct NativeTypeConstructionTrace {
    events: Vec<NativeTypeConstructionEvent>,
    allocated_super_weapon_type_count: usize,
    registry_state: NativeRulesRegistryState,
}

impl NativeTypeConstructionTrace {
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
#[derive(Debug)]
pub(crate) struct NativeRulesRegistryState {
    families: HashMap<RulesTypeFamily, Vec<ProcessedType>>,
    tiberiums: Vec<ProcessedType>,
    /// The existing Colors66D3A0 find-or-create catalog survives ordinary
    /// Process handoffs. Reset6686E6..668711 deletes and clears it alongside
    /// the Type registries, so destructive_reset leaves this field empty.
    colors: Vec<(String, String)>,
    rules_detail: DetailRules,
    rules_difficulty_rows: [crate::rules::ruleset::DifficultyRules; 3],
    rules_gravity: i32,
    rules_missile_rot_var: f64,
    rules_safety_altitude: i32,
    rules_line_trail_override: [u8; 3],
    rules_prism_support: PrismSupportRules,
    rules_prism_type: Option<String>,
    rules_prerequisite_proc_alternate: Option<String>,
    /// The six `[General] Prerequisite*` lists (Rules `+0x358..+0x3E4`), in
    /// [`PrerequisiteGroup::ALL`] order.
    rules_prerequisite_lists: [Vec<Prerequisite>; 6],
    rules_building_types: GeneralBuildingTypes,
    select_anim: SelectAnimRulesState,
}

impl Default for NativeRulesRegistryState {
    fn default() -> Self {
        Self {
            families: HashMap::new(),
            tiberiums: Vec::new(),
            colors: Vec::new(),
            rules_detail: DetailRules::default(),
            rules_difficulty_rows: crate::rules::ruleset::GeneralRules::default().difficulty_rows,
            // RulesClass665650 initializes +16B8 before any AudioVisual read.
            rules_gravity: 3,
            rules_missile_rot_var: 0.25,
            rules_safety_altitude: 500,
            rules_line_trail_override: [0; 3],
            rules_prism_support: PrismSupportRules::default(),
            rules_prism_type: None,
            rules_prerequisite_proc_alternate: None,
            rules_prerequisite_lists: Default::default(),
            rules_building_types: GeneralBuildingTypes::default(),
            select_anim: SelectAnimRulesState::default(),
        }
    }
}

impl NativeRulesRegistryState {
    pub(crate) fn anim_type_art_read_states(&self) -> impl Iterator<Item = (&str, bool)> {
        self.families
            .get(&RulesTypeFamily::Animation)
            .into_iter()
            .flatten()
            .map(|member| (member.native_stored_id.as_str(), member.anim_art_read))
    }

    #[cfg(test)]
    pub(crate) fn side_country_members(&self, side: &str) -> Option<&[i32]> {
        self.side_registry_states()
            .find_map(|(id, members)| id.eq_ignore_ascii_case(side).then_some(members))
    }

    #[cfg(test)]
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

    #[cfg(test)]
    pub(crate) fn family_len(&self, family: NativeTypeConstructorFamily) -> usize {
        self.families
            .iter()
            .find_map(|(rules_family, members)| {
                (rules_family.native_constructor_family() == Some(family)).then_some(members.len())
            })
            .unwrap_or(0)
    }

    /// Consume the pre-reset registry owner at Full_Init's destructive Rules
    /// reset and return an owner with empty Type registries. RulesClass itself
    /// survives 6686C0, retaining Gravity, detail thresholds and its other
    /// scalar values for the first postpass.
    ///
    /// Original reset retires every referenced Type: LightningWarhead detaches
    /// to null, while these Rules Anim references/list retain freed addresses.
    /// Rust clears all affected references at deletion rather than rebinding
    /// retired Types by name. This is a deterministic safety policy for native
    /// stale-pointer state; authored post-reset Process reads rebuild bindings.
    /// Evidence: tools/rules_oracle/select_anim_inputs.md.
    ///
    /// Numeric-ID history is intentionally not represented here and therefore
    /// cannot be rewound by this operation.
    pub(crate) fn destructive_reset(self) -> Self {
        Self {
            rules_detail: self.rules_detail,
            rules_difficulty_rows: self.rules_difficulty_rows,
            rules_gravity: self.rules_gravity,
            rules_missile_rot_var: self.rules_missile_rot_var,
            rules_safety_altitude: self.rules_safety_altitude,
            rules_line_trail_override: self.rules_line_trail_override,
            rules_prism_support: self.rules_prism_support,
            // Plain index vectors, so RulesClass keeps them; the next pass
            // reads them as its default.
            rules_prerequisite_lists: self.rules_prerequisite_lists,
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
#[derive(Debug, Default)]
struct SelectAnimRulesState {
    lightning_warhead: String,
    weather_con_bolt_explosion: String,
    weapon_nullify_anim: String,
    splash_list: Vec<String>,
}

#[derive(Debug, Clone, Default)]
struct WarheadAnimReadState {
    conventional: bool,
    em_effect: bool,
    anim_list: Vec<String>,
}

#[derive(Debug, Clone)]
struct ProcessedType {
    native_stored_id: String,
    body: IniSection,
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
    /// Unit ctor747167..7471B2 and reached fixed-ART reader7477C1..747AAE.
    unit_shp: crate::rules::shp_vehicle_sequence::UnitShpReadState,
    /// TechnoType ctor71136F/711781, generic flag read71287E and FV pair
    /// reads747BBD..747E90, retained across every reached Rules pass.
    gunner_turrets: GunnerTurrets,
    recoil: crate::rules::recoil::RecoilConfig,
    /// TechnoType `Prerequisite=` (`+0x638`) and `PrerequisiteOverride=`
    /// (`+0x654`), each read with its current list as the default.
    prerequisite: Vec<Prerequisite>,
    prerequisite_override: Vec<Prerequisite>,
    weapon: WeaponReadState,
    warhead_anim: WarheadAnimReadState,
}

/// Fields needed by Weapon7729F0, retained on the same live Weapon object.
/// The projectile handle is an allocation index, not a relookup of a truncated
/// name: repeated names longer than 24 bytes can allocate distinct native types.
#[derive(Debug, Clone, Default)]
struct WeaponReadState {
    speed: i32,
    range_leptons: i32,
    projectile: Option<usize>,
}

impl ProcessedType {
    fn new(native_stored_id: String) -> Self {
        Self {
            body: IniSection::new(native_stored_id.clone()),
            projectile_art: ProjectileArtState::new(&native_stored_id),
            native_stored_id,
            country_color_scheme: 0,
            country_side_index: -1,
            side_country_members: Vec::new(),
            anim_art_read: false,
            building_foundation: 0,
            unit_shp: Default::default(),
            gunner_turrets: GunnerTurrets::default(),
            recoil: crate::rules::recoil::RecoilConfig::default(),
            prerequisite: Vec::new(),
            prerequisite_override: Vec::new(),
            weapon: WeaponReadState::default(),
            warhead_anim: WarheadAnimReadState::default(),
        }
    }
}

#[derive(Debug)]
struct RulesPassProcessor {
    ordinary: Option<IniFile>,
    crate_rules: CrateRulesAccumulator,
    powerups: PowerupsAccumulator,
    general_type_lists: GeneralTypeLists,
    missile_spawn: MissileSpawnRules,
    families: HashMap<RulesTypeFamily, Vec<ProcessedType>>,
    native_type_construction_events: Vec<NativeTypeConstructionEvent>,
    tiberiums: Vec<ProcessedType>,
    colors: Vec<(String, String)>,
    rules_detail: DetailRules,
    rules_difficulty_rows: [crate::rules::ruleset::DifficultyRules; 3],
    rules_gravity: i32,
    rules_missile_rot_var: f64,
    rules_safety_altitude: i32,
    rules_line_trail_override: [u8; 3],
    rules_prism_support: PrismSupportRules,
    rules_prism_type: Option<String>,
    rules_prerequisite_proc_alternate: Option<String>,
    rules_prerequisite_lists: [Vec<Prerequisite>; 6],
    rules_building_types: GeneralBuildingTypes,
    select_anim: SelectAnimRulesState,
}

impl Default for RulesPassProcessor {
    fn default() -> Self {
        Self {
            ordinary: None,
            crate_rules: CrateRulesAccumulator::default(),
            powerups: PowerupsAccumulator::default(),
            general_type_lists: GeneralTypeLists::default(),
            missile_spawn: MissileSpawnRules::default(),
            families: HashMap::new(),
            native_type_construction_events: Vec::new(),
            tiberiums: Vec::new(),
            colors: Vec::new(),
            rules_detail: DetailRules::default(),
            rules_difficulty_rows: crate::rules::ruleset::GeneralRules::default().difficulty_rows,
            rules_gravity: NativeRulesRegistryState::default().rules_gravity,
            rules_missile_rot_var: NativeRulesRegistryState::default().rules_missile_rot_var,
            rules_safety_altitude: NativeRulesRegistryState::default().rules_safety_altitude,
            rules_line_trail_override: NativeRulesRegistryState::default()
                .rules_line_trail_override,
            rules_prism_support: PrismSupportRules::default(),
            rules_prism_type: None,
            rules_prerequisite_proc_alternate: None,
            rules_prerequisite_lists: Default::default(),
            rules_building_types: GeneralBuildingTypes::default(),
            select_anim: SelectAnimRulesState::default(),
        }
    }
}

impl RulesPassProcessor {
    fn with_registry_state(registry_state: NativeRulesRegistryState) -> Self {
        Self {
            families: registry_state.families,
            tiberiums: registry_state.tiberiums,
            colors: registry_state.colors,
            rules_detail: registry_state.rules_detail,
            rules_difficulty_rows: registry_state.rules_difficulty_rows,
            rules_gravity: registry_state.rules_gravity,
            rules_missile_rot_var: registry_state.rules_missile_rot_var,
            rules_safety_altitude: registry_state.rules_safety_altitude,
            rules_line_trail_override: registry_state.rules_line_trail_override,
            rules_prism_support: registry_state.rules_prism_support,
            rules_prism_type: registry_state.rules_prism_type,
            rules_prerequisite_proc_alternate: registry_state.rules_prerequisite_proc_alternate,
            rules_prerequisite_lists: registry_state.rules_prerequisite_lists,
            rules_building_types: registry_state.rules_building_types,
            select_anim: registry_state.select_anim,
            ..Self::default()
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
        self.allocate_crate_references(pass);
        // ReadCrateRules @ 0x0066B900 reads the semantic crate values in the
        // same Process pass; it allocates no Type and spends no ID.
        self.crate_rules.apply_pass(pass);
        self.powerups.apply_pass(pass);
        self.allocate_combat_references(pass);
        // ReadGeneral's rocket blocks and ReadCombatDamage's rocket warheads.
        // They read and write no other Process state, so one call per pass
        // keeps their order. RulesClass outlives scenarios, but retail
        // rulesmd.ini sets every key each scenario re-reads, so a fresh
        // constructor here is observably the same.
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
        if let Some(index) = self.find_existing(family, incoming) {
            return Some(index);
        }
        let members = self.family_mut(family);
        // AbstractTypeClass::Constructor @ 0x00410800 stores only 0x18 bytes.
        // Lookup above compares that stored ID against the full input, so a
        // repeated >24-byte spelling can construct another equal stored ID.
        let native_stored_id = abstract_type_stored_id(incoming);
        let index = members.len();
        members.push(ProcessedType::new(native_stored_id.clone()));
        if let Some(family) = family.native_constructor_family() {
            self.native_type_construction_events
                .push(NativeTypeConstructionEvent {
                    family,
                    native_stored_id,
                });
        }
        Some(index)
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

    fn allocate_scalar_from(
        &mut self,
        section: &IniSection,
        key: &str,
        family: RulesTypeFamily,
        capacity: usize,
    ) {
        if !section.is_present(key) {
            return;
        }
        let incoming = section.read_string(key, "", capacity);
        if !incoming.is_empty() {
            self.find_or_allocate(family, &incoming);
        }
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
            self.allocate_list_from(section, key, RulesTypeFamily::Building, 0x80);
        }
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
        // General66EC37/66EC57 read current RulesClass values as defaults.
        // Both survive empty/missing later passes and destructive Type resets.
        self.rules_missile_rot_var =
            section.read_double("MissileROTVar", self.rules_missile_rot_var);
        self.rules_safety_altitude =
            section.read_int("MissileSafetyAltitude", self.rules_safety_altitude);
        // ReadGeneral's Prism block (`0x0067114F..0x006711B8`), current values
        // as defaults; RulesClass keeps them across Full_Init's reset.
        self.rules_prism_support = self.rules_prism_support.read_pass(section);

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
            } else if key == "AnimToInfantry" {
                if let Some(resolved) = self.resolve_list_from(section, key, family, 0x80) {
                    self.general_type_lists.anim_to_infantry = resolved;
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
                    | "WeatherConBoltExplosion"
                    | "WeaponNullifyAnim"
                    | "PrismType"
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
                // through BuildingType's FindOrAllocate, and
                // PrerequisiteProcAlternate (`0x0066F787..0x0066F7C9`) through
                // UnitType's (`0x007480D0`).
                let incoming = section.read_string(key, "", 0x80);
                if !incoming.is_empty() {
                    let resolved = self
                        .find_or_allocate(family, &incoming)
                        .map(|index| self.families[&family][index].native_stored_id.clone())
                        .unwrap_or_default();
                    match key {
                        "LightningWarhead" => self.select_anim.lightning_warhead = resolved,
                        "WeatherConBoltExplosion" => {
                            self.select_anim.weather_con_bolt_explosion = resolved;
                        }
                        "WeaponNullifyAnim" => self.select_anim.weapon_nullify_anim = resolved,
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
            } else if is_list {
                self.allocate_list_from(section, key, family, 0x80);
            } else {
                self.allocate_scalar_from(section, key, family, 0x80);
            }
        }
    }

    /// `RulesClass::ReadGeneral` parses the six `[General] Prerequisite*`
    /// lists (`0x0066E78C..0x0066EA6E`) before any of its own BuildingType
    /// allocations, each with its current list as the default.
    fn read_prerequisite_lists(&mut self, pass: &IniFile) {
        let Some(general) = pass.section("General") else {
            return;
        };
        for group in PrerequisiteGroup::ALL {
            let parsed = parse_prerequisites(
                general.read_list(group.general_key(), 0x80),
                &self.rules_prerequisite_lists[group.index()],
                |name| self.building_index(name),
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
        let member = self.families.get_mut(&family)?.get_mut(index)?;
        member.body.overlay_rules_pass(&raw);
        Some((native_stored_id, raw, member.body.clone()))
    }

    fn process_type_data(&mut self, pass: &IniFile, fixed_art: &IniFile) {
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
            if let Some((_id, raw, _effective)) =
                self.begin_rules_member_read(RulesTypeFamily::SuperWeapon, index, pass)
            {
                self.allocate_scalar_from(&raw, "WeaponType", RulesTypeFamily::Weapon, 0x80);
                self.allocate_scalar_from(&raw, "AuxBuilding", RulesTypeFamily::Building, 0x80);
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
                self.families
                    .get_mut(&RulesTypeFamily::Animation)
                    .expect("the live AnimType member exists")[index]
                    .anim_art_read = true;
                for key in ["Next", "Spawns"] {
                    self.allocate_scalar_from(section, key, RulesTypeFamily::Animation, 0x80);
                }
                self.allocate_scalar_from(
                    section,
                    "TiberiumSpawnType",
                    RulesTypeFamily::Overlay,
                    0x80,
                );
                for key in ["BounceAnim", "ExpireAnim", "TrailerAnim"] {
                    self.allocate_scalar_from(section, key, RulesTypeFamily::Animation, 0x80);
                }
                self.allocate_scalar_from(section, "Warhead", RulesTypeFamily::Warhead, 0x80);
                self.allocate_scalar_from(
                    section,
                    "SpawnsParticle",
                    RulesTypeFamily::Particle,
                    0x20,
                );
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
                self.process_techno_base(&raw, &effective);
                // TechnoTypeClass::ReadINI parses `Prerequisite=` (`0x007141C5`)
                // and `PrerequisiteOverride=` (`0x0071424B`) after `Dock=` and
                // `DeploysInto=` (`0x00713180`, `0x00713279`), the reads above
                // that can allocate a BuildingType, each with its current list
                // as the default.
                let member = &self.families[&family][index];
                let prerequisite = parse_prerequisites(
                    raw.read_list("Prerequisite", 0x80),
                    &member.prerequisite,
                    |name| self.building_index(name),
                );
                let prerequisite_override = parse_prerequisites(
                    raw.read_list("PrerequisiteOverride", 0x80),
                    &member.prerequisite_override,
                    |name| self.building_index(name),
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
                match family {
                    RulesTypeFamily::Building => {
                        // Native ObjectType5F933B copies Rules Image into its
                        // 25-byte field using the retained field as default.
                        // The reached-body projection holds the last input, so
                        // applying that native cut here preserves the same image
                        // across missing keys and registry handoffs. The later
                        // ART Image redirect is local to visual asset loading.
                        let image = effective.read_string("Image", &native_stored_id, 0x19);
                        let member = &mut self.families.get_mut(&family).unwrap()[index];
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
                        self.allocate_scalar_from(&raw, "FreeUnit", RulesTypeFamily::Vehicle, 0x80);
                        self.allocate_scalar_from(
                            &raw,
                            "SecretInfantry",
                            RulesTypeFamily::Infantry,
                            0x80,
                        );
                        self.allocate_scalar_from(
                            &raw,
                            "SecretUnit",
                            RulesTypeFamily::Vehicle,
                            0x80,
                        );
                        self.allocate_scalar_from(
                            &raw,
                            "SecretBuilding",
                            RulesTypeFamily::Building,
                            0x80,
                        );
                        self.allocate_fixed_art_techno_reference(
                            fixed_art,
                            &native_stored_id,
                            &effective,
                            "ToOverlay",
                            RulesTypeFamily::Overlay,
                        );
                    }
                    RulesTypeFamily::Aircraft => {
                        self.allocate_fixed_art_techno_reference(
                            fixed_art,
                            &native_stored_id,
                            &effective,
                            "Trailer",
                            RulesTypeFamily::Animation,
                        );
                    }
                    RulesTypeFamily::Vehicle => {
                        // Object5F933B retains the Image buffer+1F8; Unit7477C1
                        // uses that section without a type-name fallback. The
                        // existing reached-body projection retains Image/Turret
                        // defaults across only the admitted Rules passes.
                        let image = effective.read_string("Image", &native_stored_id, 0x19);
                        let has_turret = effective.read_bool("Turret", false);
                        self.families.get_mut(&family).unwrap()[index]
                            .unit_shp
                            .read_pass(fixed_art.section_or_empty(&image), has_turret);
                    }
                    RulesTypeFamily::Infantry => {
                        self.allocate_scalar_from(
                            &raw,
                            "OccupyWeapon",
                            RulesTypeFamily::Weapon,
                            0x80,
                        );
                        self.allocate_scalar_from(
                            &raw,
                            "EliteOccupyWeapon",
                            RulesTypeFamily::Weapon,
                            0x80,
                        );
                        self.allocate_list_from(
                            &raw,
                            "DeadBodies",
                            RulesTypeFamily::Animation,
                            0x80,
                        );
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

    fn process_techno_base(&mut self, raw: &IniSection, effective: &IniSection) {
        self.allocate_scalar_from(raw, "DeathWeapon", RulesTypeFamily::Weapon, 0x80);
        self.allocate_list_from(raw, "DebrisTypes", RulesTypeFamily::VoxelAnimation, 0x80);
        self.allocate_list_from(raw, "DebrisAnims", RulesTypeFamily::Animation, 0x80);

        let turret_count = effective.read_int("TurretCount", 0);
        let weapon_count = effective.read_int("WeaponCount", 0);
        let clear_all_weapons = effective.read_bool("ClearAllWeapons", false);
        if turret_count >= 1 && weapon_count > 0 {
            for slot in 1..=weapon_count {
                self.allocate_scalar_from(
                    raw,
                    &format!("Weapon{slot}"),
                    RulesTypeFamily::Weapon,
                    0x80,
                );
                self.allocate_scalar_from(
                    raw,
                    &format!("EliteWeapon{slot}"),
                    RulesTypeFamily::Weapon,
                    0x80,
                );
            }
        } else if turret_count < 1 && !clear_all_weapons {
            for key in ["Primary", "Secondary", "ElitePrimary", "EliteSecondary"] {
                self.allocate_scalar_from(raw, key, RulesTypeFamily::Weapon, 0x80);
            }
        }

        self.allocate_list_from(raw, "Dock", RulesTypeFamily::Building, 0x80);
        self.allocate_scalar_from(raw, "DeploysInto", RulesTypeFamily::Building, 0x80);
        self.allocate_scalar_from(raw, "UndeploysInto", RulesTypeFamily::Vehicle, 0x80);
        self.allocate_scalar_from(raw, "PowersUnit", RulesTypeFamily::Vehicle, 0x80);
        self.allocate_list_from(raw, "Explosion", RulesTypeFamily::Animation, 0x80);
        self.allocate_list_from(raw, "DestroyAnim", RulesTypeFamily::Animation, 0x80);
        self.allocate_scalar_from(
            raw,
            "NaturalParticleSystem",
            RulesTypeFamily::ParticleSystem,
            0x80,
        );
        self.allocate_scalar_from(
            raw,
            "RefinerySmokeParticleSystem",
            RulesTypeFamily::ParticleSystem,
            0x80,
        );
        self.allocate_list_from(
            raw,
            "DamageParticleSystems",
            RulesTypeFamily::ParticleSystem,
            0x80,
        );
        self.allocate_list_from(
            raw,
            "DestroyParticleSystems",
            RulesTypeFamily::ParticleSystem,
            0x80,
        );
        self.allocate_scalar_from(raw, "AirstrikeTeamType", RulesTypeFamily::Aircraft, 0x80);
        self.allocate_scalar_from(
            raw,
            "EliteAirstrikeTeamType",
            RulesTypeFamily::Aircraft,
            0x80,
        );
        self.allocate_scalar_from(raw, "UnloadingClass", RulesTypeFamily::Vehicle, 0x80);
        self.allocate_scalar_from(raw, "DeployingAnim", RulesTypeFamily::Animation, 0x80);
        self.allocate_scalar_from(raw, "Enslaves", RulesTypeFamily::Infantry, 0x80);
        self.allocate_scalar_from(raw, "Spawns", RulesTypeFamily::Aircraft, 0x80);
    }

    fn allocate_fixed_art_techno_reference(
        &mut self,
        fixed_art: &IniFile,
        native_stored_id: &str,
        effective: &IniSection,
        key: &str,
        family: RulesTypeFamily,
    ) {
        let image = effective.read_string("Image", native_stored_id, 0x80);
        if image.is_empty() {
            return;
        }
        if let Some(section) = fixed_art.section(&image) {
            self.allocate_scalar_from(section, key, family, 0x80);
        }
    }

    fn process_weapon_family(&mut self, pass: &IniFile) {
        let mut index = 0;
        while index < self.family_len(RulesTypeFamily::Weapon) {
            if let Some((_id, raw, _effective)) =
                self.begin_rules_member_read(RulesTypeFamily::Weapon, index, pass)
            {
                self.allocate_list_from(&raw, "Anim", RulesTypeFamily::Animation, 0x80);
                for key in ["AssaultAnim", "OccupantAnim", "OpenToppedAnim"] {
                    self.allocate_scalar_from(&raw, key, RulesTypeFamily::Animation, 0x80);
                }
                self.allocate_scalar_from(
                    &raw,
                    "AttachedParticleSystem",
                    RulesTypeFamily::ParticleSystem,
                    0x14,
                );
                self.allocate_scalar_from(&raw, "Warhead", RulesTypeFamily::Warhead, 0x80);
                let incoming = raw.read_string("Projectile", "", 0x80);
                let projectile = (!incoming.is_empty())
                    .then(|| self.find_or_allocate(RulesTypeFamily::Projectile, &incoming));
                let state = &mut self.family_mut(RulesTypeFamily::Weapon)[index].weapon;
                if let Some(projectile) = projectile {
                    state.projectile = projectile;
                }
                state.speed = raw.read_speed("Speed", state.speed);
                state.range_leptons = raw.read_range("Range", state.range_leptons);
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
                art.read_pass(&raw, fixed_art, |incoming| {
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
                self.allocate_scalar_from(&raw, "AirburstWeapon", RulesTypeFamily::Weapon, 0x80);
                self.allocate_scalar_from(&raw, "ShrapnelWeapon", RulesTypeFamily::Weapon, 0x80);
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
                self.allocate_scalar_from(&raw, "Particle", RulesTypeFamily::ParticleSystem, 0x80);
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
                self.allocate_list_from(&raw, "DebrisTypes", RulesTypeFamily::VoxelAnimation, 0x80);
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
            {
                self.allocate_scalar_from(&raw, "Warhead", RulesTypeFamily::Warhead, 0x80);
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
                let holds_what = raw.read_string("HoldsWhat", "undefined", 0x40);
                self.find_or_allocate(RulesTypeFamily::Particle, &holds_what);
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
                for key in ["BounceAnim", "ExpireAnim", "TrailerAnim"] {
                    self.allocate_scalar_from(&raw, key, RulesTypeFamily::Animation, 0x80);
                }
                self.allocate_scalar_from(&raw, "Warhead", RulesTypeFamily::Warhead, 0x80);
                self.allocate_scalar_from(
                    &raw,
                    "AttachedSystem",
                    RulesTypeFamily::ParticleSystem,
                    0x80,
                );
            }
            index += 1;
        }
    }

    fn allocate_crate_references(&mut self, pass: &IniFile) {
        let Some(section) = pass.section("CrateRules") else {
            return;
        };
        for key in ["WoodCrateImg", "CrateImg", "WaterCrateImg"] {
            self.allocate_scalar_from(section, key, RulesTypeFamily::Overlay, 0x80);
        }
        self.allocate_scalar_from(section, "UnitCrateType", RulesTypeFamily::Vehicle, 0x80);
    }

    fn allocate_combat_references(&mut self, pass: &IniFile) {
        let Some(section) = pass.section("CombatDamage") else {
            return;
        };
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
        for key in [
            "FlameDamage",
            "FlameDamage2",
            "C4Warhead",
            "CrushWarhead",
            "V3Warhead",
            "DMislWarhead",
            "V3EliteWarhead",
            "DMislEliteWarhead",
            "CMislWarhead",
            "CMislEliteWarhead",
            "IvanWarhead",
        ] {
            self.allocate_scalar_from(section, key, RulesTypeFamily::Warhead, 0x80);
        }
        self.allocate_scalar_from(section, "DeathWeapon", RulesTypeFamily::Weapon, 0x80);
        for key in [
            "DrainAnimationType",
            "ControlledAnimationType",
            "PermaControlledAnimationType",
        ] {
            self.allocate_scalar_from(section, key, RulesTypeFamily::Animation, 0x80);
        }
        self.allocate_scalar_from(section, "IonCannonWarhead", RulesTypeFamily::Warhead, 0x80);
        for key in [
            "DefaultLargeGreySmokeSystem",
            "DefaultSmallGreySmokeSystem",
            "DefaultSparkSystem",
            "DefaultLargeRedSmokeSystem",
            "DefaultSmallRedSmokeSystem",
            "DefaultDebrisSmokeSystem",
            "DefaultFireStreamSystem",
            "DefaultTestParticleSystem",
            "DefaultRepairParticleSystem",
        ] {
            self.allocate_scalar_from(section, key, RulesTypeFamily::ParticleSystem, 0x80);
        }
    }

    fn allocate_radiation_references(&mut self, pass: &IniFile) {
        if let Some(section) = pass.section("Radiation") {
            self.allocate_scalar_from(section, "RadSiteWarhead", RulesTypeFamily::Warhead, 0x80);
        }
    }

    fn allocate_audio_visual_references(&mut self, pass: &IniFile) {
        let Some(section) = pass.section("AudioVisual") else {
            return;
        };
        // Original 66920D..669258 reads these first, including the cold
        // startup's same AudioVisual call before any complete Process.
        self.rules_detail = self.rules_detail.read_pass(section);
        // Full AudioVisual6691E0's 66B3C4 read uses the retained signed dword
        // default. Cold startup52D132 calls the same reader before Process.
        self.rules_gravity = section.read_int("Gravity", self.rules_gravity);
        // AudioVisual66B77D..66B7A7, after Gravity, retains the RGB default.
        self.rules_line_trail_override =
            section.read_color_rgb("LineTrailColorOverride", self.rules_line_trail_override);
        for key in ["DropPodPuff", "VeinAttack", "Dig", "AtmosphereEntry"] {
            self.allocate_scalar_from(section, key, RulesTypeFamily::Animation, 0x80);
        }
        for key in ["TreeFire", "OnFire"] {
            self.allocate_list_from(section, key, RulesTypeFamily::Animation, 0x80);
        }
        self.allocate_scalar_from(section, "Smoke", RulesTypeFamily::Animation, 0x80);
        self.allocate_scalar_from(section, "Smoke", RulesTypeFamily::Animation, 0x80);
        for key in ["SmallFire", "LargeFire"] {
            self.allocate_scalar_from(section, key, RulesTypeFamily::Animation, 0x80);
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
            self.allocate_scalar_from(section, key, family, 0x80);
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

    fn finish(
        mut self,
    ) -> (
        IniFile,
        NativeTypeConstructionTrace,
        CrateRules,
        PowerupTable,
        GeneralTypeLists,
        MissileSpawnRules,
    ) {
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
            if let Some(members) = self.families.get(&family) {
                for member in members {
                    let mut body = member.body.clone();
                    if family == RulesTypeFamily::Weapon {
                        if let Some(index) = member.weapon.projectile {
                            // Compatibility graph discovery must follow the
                            // retained pointer even after an empty key read.
                            body.set(
                                "Projectile",
                                &self.families[&RulesTypeFamily::Projectile][index]
                                    .native_stored_id,
                            );
                        }
                    }
                    ini.replace_first_section(body);
                }
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
                    families: self.families,
                    tiberiums: self.tiberiums,
                    colors: self.colors,
                    rules_detail: self.rules_detail,
                    rules_difficulty_rows: self.rules_difficulty_rows,
                    rules_gravity: self.rules_gravity,
                    rules_missile_rot_var: self.rules_missile_rot_var,
                    rules_safety_altitude: self.rules_safety_altitude,
                    rules_line_trail_override: self.rules_line_trail_override,
                    rules_prism_support: self.rules_prism_support,
                    rules_prism_type: self.rules_prism_type,
                    rules_prerequisite_proc_alternate: self.rules_prerequisite_proc_alternate,
                    rules_prerequisite_lists: self.rules_prerequisite_lists,
                    rules_building_types: self.rules_building_types,
                    select_anim: self.select_anim,
                },
            },
            self.crate_rules.finish(),
            self.powerups.finish(),
            self.general_type_lists,
            self.missile_spawn,
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
#[path = "projectile_art_tests.rs"]
mod projectile_art_tests;

#[cfg(test)]
#[path = "weapon_speed_tests.rs"]
mod weapon_speed_tests;

#[cfg(test)]
#[path = "guided_controls_tests.rs"]
mod guided_controls_tests;

#[cfg(test)]
#[path = "select_anim_rules_tests.rs"]
mod select_anim_rules_tests;

#[cfg(test)]
#[path = "approach_rules_tests.rs"]
mod approach_rules_tests;
