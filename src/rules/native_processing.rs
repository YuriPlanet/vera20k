//! Active-YR RulesClass processing: ordered passes, live Type registries,
//! constructor chronology and typed-reader compatibility projections.
//!
//! Raw INI storage stays with ini_parser; process_owner retains registry lifetime;
//! ruleset converts this processed output into gameplay definitions. Failed passes
//! preserve already-constructed registry state rather than rolling back.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use crate::rules::ini_parser::{IniFile, IniSection};
use crate::rules::crate_rules::{CrateRules, CrateRulesAccumulator};
use crate::rules::powerups::{PowerupTable, PowerupsAccumulator};
use crate::rules::error::RulesError;

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
                let (_, partial_trace, _, _) = processor.finish();
                return Err(NativeRulesProcessingFailure {
                    error,
                    partial_trace,
                });
            }
        }
        let (ini, native_type_construction_trace, crate_rules, powerups) = processor.finish();
        Ok(ProcessedRulesLayers {
            ini,
            crate_rules,
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
    process_native_rules_cold_start_inner(
        registry_state,
        selected_rules_root,
        fixed_art,
        langrule,
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
    processor.process_techno_family(
        RulesTypeFamily::Building,
        selected_rules_root,
        fixed_art,
    );
    let after_building_bodies = processor.native_type_construction_events.len();
    let (_, trace, _, _) = processor.finish();
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

/// Shared production/test implementation. Counts are cumulative `E_multi`
/// boundaries after Countries, General, and the live HouseType body loop.
fn process_native_noncampaign_rules_prepass_inner(
    registry_state: NativeRulesRegistryState,
    selected_rules_root: &IniFile,
) -> (NativeTypeConstructionTrace, [usize; 3]) {
    let mut processor = RulesPassProcessor::with_registry_state(registry_state);
    processor.allocate_explicit_family(
        selected_rules_root,
        "Countries",
        RulesTypeFamily::Country,
    );
    let after_countries = processor.native_type_construction_events.len();
    processor.allocate_general_references(selected_rules_root);
    let after_general = processor.native_type_construction_events.len();
    processor.process_house_family(selected_rules_root);
    let after_house_bodies = processor.native_type_construction_events.len();
    let (_, trace, _, _) = processor.finish();
    (
        trace,
        [after_countries, after_general, after_house_bodies],
    )
}

/// Result of applying an ordered rules stack.
#[derive(Debug)]
pub struct ProcessedRulesLayers {
    ini: IniFile,
    crate_rules: CrateRules,
    powerups: PowerupTable,
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
/// `AbstractClass::AssignUniqueID @ 0x00410230`.
///
/// `ParticleTypeClass` is deliberately absent: its constructor has no Assign
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
#[derive(Debug, Default)]
pub(crate) struct NativeRulesRegistryState {
    families: HashMap<RulesTypeFamily, Vec<ProcessedType>>,
    tiberiums: Vec<ProcessedType>,
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
    pub(crate) fn family_len(&self, family: NativeTypeConstructorFamily) -> usize {
        self.families
            .iter()
            .find_map(|(rules_family, members)| {
                (rules_family.native_constructor_family() == Some(family)).then_some(members.len())
            })
            .unwrap_or(0)
    }

    #[cfg(test)]
    pub(crate) fn tiberium_slot_count(&self) -> usize {
        self.tiberiums.len()
    }

    /// Consume the pre-reset registry owner at Full_Init's destructive Rules
    /// reset and return a genuinely empty post-reset owner.
    ///
    /// Numeric-ID history is intentionally not represented here and therefore
    /// cannot be rewound by this operation.
    pub(crate) fn destructive_reset(self) -> Self {
        Self::default()
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

#[derive(Debug, Clone)]
struct ProcessedType {
    native_stored_id: String,
    body: IniSection,
    /// False until the native AnimType ART-body boundary has been entered.
    /// Kept on the process-resident type so subsequent Rules passes retain it.
    anim_art_read: bool,
}

impl ProcessedType {
    fn new(native_stored_id: String) -> Self {
        Self {
            body: IniSection::new(native_stored_id.clone()),
            native_stored_id,
            anim_art_read: false,
        }
    }
}

#[derive(Debug, Default)]
struct RulesPassProcessor {
    ordinary: Option<IniFile>,
    crate_rules: CrateRulesAccumulator,
    powerups: PowerupsAccumulator,
    families: HashMap<RulesTypeFamily, Vec<ProcessedType>>,
    native_type_construction_events: Vec<NativeTypeConstructionEvent>,
    tiberiums: Vec<ProcessedType>,
    colors: Vec<(String, String)>,
    prerequisite_groups: HashMap<&'static str, Vec<String>>,
}

impl RulesPassProcessor {
    fn with_registry_state(registry_state: NativeRulesRegistryState) -> Self {
        Self {
            families: registry_state.families,
            tiberiums: registry_state.tiberiums,
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
            self.allocate_explicit_family(pass, registry, family);
        }

        // JumpjetControls and MultiplayerSettings contain no Type factory.
        self.allocate_ai_references(pass);
        self.read_prerequisite_groups(pass);
        self.allocate_general_references(pass);
        self.process_type_data(pass, fixed_art);

        // Difficulty readers contain no Type factories.
        self.allocate_crate_references(pass);
        // ReadCrateRules @ 0x0066B900 reads the semantic crate values in the
        // same Process pass; it allocates no Type and spends no ID.
        self.crate_rules.apply_pass(pass);
        self.powerups.apply_pass(pass);
        self.allocate_combat_references(pass);
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
            let identity = if family == RulesTypeFamily::Side {
                key.to_string()
            } else {
                section.read_string(key, "", 32)
            };
            if !identity.is_empty() {
                self.find_or_allocate(family, &identity);
            }
        }
    }

    fn family_mut(&mut self, family: RulesTypeFamily) -> &mut Vec<ProcessedType> {
        self.families.entry(family).or_default()
    }

    fn find_or_allocate(&mut self, family: RulesTypeFamily, incoming: &str) -> Option<usize> {
        // Caller-specific ReadString buffers trim the whole scalar before this
        // boundary. List tokens deliberately arrive untrimmed. The factory
        // must therefore inspect the exact incoming string, not trim again.
        if incoming.is_empty()
            || (family != RulesTypeFamily::Side && is_exact_native_none_type_name(incoming))
        {
            return None;
        }
        let members = self.family_mut(family);
        if let Some(index) = members
            .iter()
            .position(|member| member.native_stored_id.eq_ignore_ascii_case(incoming))
        {
            return Some(index);
        }
        // AbstractTypeClass::Constructor @ 0x00410800 stores only 0x18 bytes.
        // Lookup above compares that stored ID against the full input, so a
        // repeated >24-byte spelling can construct another equal stored ID.
        let native_stored_id = incoming.chars().take(0x18).collect::<String>();
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
        for key in section.keys() {
            let Some(value) = section.get(key) else {
                continue;
            };
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
        if section.get(key).is_none() {
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
        if section.get(key).is_none() {
            return;
        }
        let incoming = section.read_string(key, "", capacity);
        for token in native_strtok_comma_tokens(&incoming) {
            self.find_or_allocate(family, token);
        }
    }

    fn lookup_existing(&self, family: RulesTypeFamily, incoming: &str) -> Option<String> {
        if incoming.is_empty()
            || (family != RulesTypeFamily::Side && is_exact_native_none_type_name(incoming))
        {
            return None;
        }
        self.families.get(&family)?.iter().find_map(|member| {
            member
                .native_stored_id
                .eq_ignore_ascii_case(incoming)
                .then(|| member.native_stored_id.clone())
        })
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
            (
                "WeatherConBoltExplosion",
                RulesTypeFamily::Animation,
                false,
            ),
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
            (
                "IronCurtainInvokeAnim",
                RulesTypeFamily::Animation,
                false,
            ),
            (
                "ForceShieldInvokeAnim",
                RulesTypeFamily::Animation,
                false,
            ),
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
            (
                "PrerequisiteProcAlternate",
                RulesTypeFamily::Vehicle,
                false,
            ),
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
            (
                "DefaultMirageDisguises",
                RulesTypeFamily::Terrain,
                true,
            ),
        ];

        let Some(section) = pass.section("General") else {
            return;
        };
        for &(key, family, is_list) in SITES {
            if is_list {
                self.allocate_list_from(section, key, family, 0x80);
            } else {
                self.allocate_scalar_from(section, key, family, 0x80);
            }
        }
    }

    fn read_prerequisite_groups(&mut self, pass: &IniFile) {
        const KEYS: &[&str] = &[
            "PrerequisitePower",
            "PrerequisiteProc",
            "PrerequisiteRadar",
            "PrerequisiteTech",
            "PrerequisiteBarracks",
            "PrerequisiteFactory",
        ];

        let Some(general) = pass.section("General") else {
            return;
        };
        for &key in KEYS {
            if general.get(key).is_none() {
                continue;
            }
            let raw = general.read_string(key, "", 0x80);
            let resolved = native_strtok_comma_tokens(&raw)
                .filter_map(|identity| self.lookup_existing(RulesTypeFamily::Building, identity))
                .collect();
            self.prerequisite_groups.insert(key, resolved);
        }
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
        // Weapon post and Building post add no Type references.
        self.process_plain_family(RulesTypeFamily::Terrain, pass);
        self.process_plain_family(RulesTypeFamily::Smudge, pass);
        self.process_plain_family(RulesTypeFamily::Overlay, pass);
        self.process_particle_family(pass);
        self.process_particle_system_family(pass);
        self.process_voxel_anim_family(pass);
        // MissionControl adds no Type references.
    }

    fn process_house_family(&mut self, pass: &IniFile) {
        let mut index = 0;
        while index < self.family_len(RulesTypeFamily::Country) {
            if let Some((_id, raw, _effective)) =
                self.begin_rules_member_read(RulesTypeFamily::Country, index, pass)
            {
                self.allocate_list_from(
                    &raw,
                    "VeteranInfantry",
                    RulesTypeFamily::Infantry,
                    0x80,
                );
                self.allocate_list_from(
                    &raw,
                    "VeteranUnits",
                    RulesTypeFamily::Vehicle,
                    0x80,
                );
                self.allocate_list_from(
                    &raw,
                    "VeteranAircraft",
                    RulesTypeFamily::Aircraft,
                    0x80,
                );
                self.allocate_scalar_from(&raw, "Side", RulesTypeFamily::Side, 0x80);
            }
            index += 1;
        }
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
                match family {
                    RulesTypeFamily::Building => {
                        self.allocate_scalar_from(
                            &raw,
                            "FreeUnit",
                            RulesTypeFamily::Vehicle,
                            0x80,
                        );
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
                    RulesTypeFamily::Vehicle => {}
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
                self.allocate_scalar_from(&raw, "Projectile", RulesTypeFamily::Projectile, 0x80);
            }
            index += 1;
        }
    }

    fn process_bullet_family(&mut self, pass: &IniFile, fixed_art: &IniFile) {
        let mut index = 0;
        while index < self.family_len(RulesTypeFamily::Projectile) {
            if let Some((_id, raw, effective)) =
                self.begin_rules_member_read(RulesTypeFamily::Projectile, index, pass)
            {
                let image = effective.read_string("Image", "", 0x19);
                if !image.is_empty()
                    && let Some(section) = fixed_art.section(&image)
                {
                    self.allocate_scalar_from(
                        section,
                        "Trailer",
                        RulesTypeFamily::Animation,
                        0x80,
                    );
                }
                self.allocate_scalar_from(
                    &raw,
                    "AirburstWeapon",
                    RulesTypeFamily::Weapon,
                    0x80,
                );
                self.allocate_scalar_from(
                    &raw,
                    "ShrapnelWeapon",
                    RulesTypeFamily::Weapon,
                    0x80,
                );
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
                self.allocate_scalar_from(
                    &raw,
                    "Particle",
                    RulesTypeFamily::ParticleSystem,
                    0x80,
                );
                self.allocate_list_from(&raw, "AnimList", RulesTypeFamily::Animation, 0x80);
                self.allocate_list_from(
                    &raw,
                    "DebrisTypes",
                    RulesTypeFamily::VoxelAnimation,
                    0x80,
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
        for key in ["Scorches", "Scorches1", "Scorches2", "Scorches3", "Scorches4"] {
            self.allocate_list_from(section, key, RulesTypeFamily::Smudge, 0x80);
        }
        self.allocate_list_from(section, "SplashList", RulesTypeFamily::Animation, 0x80);
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
            let slot = crate::rules::ini_value::atoi_lenient(key);
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
                self.allocate_list_from(
                    &section,
                    "Debris",
                    RulesTypeFamily::Animation,
                    0x80,
                );
            }
        }
        Ok(())
    }

    fn finish(mut self) -> (IniFile, NativeTypeConstructionTrace, CrateRules, PowerupTable) {
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
        if !self.prerequisite_groups.is_empty() {
            let general = ini.projection_section_mut("General");
            for (key, values) in self.prerequisite_groups {
                general.set(key, &values.join(","));
            }
        }

        let mut colors = IniSection::new("Colors".to_string());
        for (name, value) in self.colors {
            colors.set(&name, &value);
        }
        ini.replace_first_section(colors);

        // Replace every allocated type's ordinary text body with the keys that
        // its live object actually read after allocation.
        for &(_, family) in PROJECTED_RULE_TYPE_FAMILIES {
            if let Some(members) = self.families.get(&family) {
                for member in members {
                    ini.replace_first_section(member.body.clone());
                }
            }
        }
        for family in [RulesTypeFamily::Weapon, RulesTypeFamily::Projectile] {
            if let Some(members) = self.families.get(&family) {
                for member in members {
                    ini.replace_first_section(member.body.clone());
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
                },
            },
            self.crate_rules.finish(),
            self.powerups.finish(),
        )
    }
}

/// Native `strtok(buffer, ",")` tokenization used by Type-reference vectors.
/// Whole-string trimming and caller truncation have already happened in
/// `ReadString`; empty fields collapse and individual tokens remain untrimmed.
fn native_strtok_comma_tokens(value: &str) -> impl Iterator<Item = &str> {
    value.split(',').filter(|token| !token.is_empty())
}

fn is_exact_native_none_type_name(value: &str) -> bool {
    value.eq_ignore_ascii_case("none") || value.eq_ignore_ascii_case("<none>")
}


#[cfg(test)]
#[path = "native_processing_tests.rs"]
mod tests;
