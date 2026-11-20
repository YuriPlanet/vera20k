//! Native Type construction chronology (`NativeTypeConstructionTrace`) and the
//! process-resident live Type registries (`NativeRulesRegistryState`).

use super::{
    AresGlobalControls, CountryProjection, GeneralTypeLists, ProcessedType, RulesTypeFamily,
    SelectAnimRulesState, SideProjection,
};
use crate::rules::ares_armor::ArmorCatalog;
use crate::rules::ares_prerequisites::GenericPrerequisites;
use crate::rules::attach_effect::AttachEffectTypeCatalog;
use crate::rules::color_add::ColorAddSelectors;
use crate::rules::color_add::ColorAddTable;
use crate::rules::compatibility::RulesCompatibility;
use crate::rules::crate_rules::CrateRules;
use crate::rules::digital_display::DigitalDisplayTypeCatalog;
use crate::rules::error::RulesError;
use crate::rules::ini_parser::IniSection;
use crate::rules::insignia_type::InsigniaTypeCatalog;
use crate::rules::missile_spawn::MissileSpawnRules;
use crate::rules::object_type::ObjectCategory;
use crate::rules::phobos_radiation::RadiationTypeCatalog;
use crate::rules::phobos_shields::ShieldTypeCatalog;
use crate::rules::powerups::PowerupTable;
use crate::rules::prerequisite::Prerequisite;
use crate::rules::prerequisite::PrerequisiteGroup;
use crate::rules::rules_type_references::RulesTypeReferences;
use crate::rules::ruleset::DetailRules;
use crate::rules::ruleset::GeneralBuildingTypes;
use crate::rules::ruleset::PrismSupportRules;
use crate::rules::sound_ini::GlobalSoundReferences;
use crate::util::native_x87::NativeF64Bits;
use std::collections::HashMap;
use std::collections::HashSet;

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
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NativeTypeConstructionEvent {
    pub(super) family: NativeTypeConstructorFamily,
    pub(super) native_stored_id: String,
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
    pub(super) events: Vec<NativeTypeConstructionEvent>,
    pub(super) allocated_super_weapon_type_count: usize,
    pub(super) registry_state: NativeRulesRegistryState,
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
    /// Rules ctor666C3F..666C75 / ReadGeneral671AF6..671B97. Mandatory
    /// saved history: flattened text cannot reconstruct previous read defaults.
    pub(super) rules_threat_coefficient_defaults: super::threat_coefficients::ThreatCoefficients,
    /// Build capability is retained with the Rules owner, never selected by INI.
    pub(super) compatibility: RulesCompatibility,
    /// Whether this registry's Rules are read the Ares way
    /// (`ares_switch::ares_way`): fixed when the registry starts, from
    /// whether the game ships `ares.mix`, and opened by every Process call.
    pub(super) reads_ares_way: bool,
    /// Ares Clear/AddDefaults rebuilds these at a destructive scenario reset;
    /// ordinary Process passes and saves retain the reached list reads.
    pub(super) ares_prerequisites: Option<GenericPrerequisites>,
    /// Ordered Ares armor registry; Process retains it, destructive reset rebuilds it.
    pub(super) ares_armors: Option<ArmorCatalog>,
    /// Phobos `[RadiationTypes]` catalog; Process retains it per pass.
    pub(super) phobos_radiation_types: Option<RadiationTypeCatalog>,
    /// Phobos `[ShieldTypes]` catalog; Process retains it per pass.
    pub(super) phobos_shield_types: Option<ShieldTypeCatalog>,
    /// Phobos `[InsigniaTypes]` catalog; Process retains it per pass.
    pub(super) phobos_insignia_types: Option<InsigniaTypeCatalog>,
    /// Phobos `[DigitalDisplayTypes]` catalog; Process retains it per pass.
    pub(super) phobos_digital_display_types: Option<DigitalDisplayTypeCatalog>,
    /// Phobos `[AttachEffectTypes]` catalog; Process retains it per pass.
    pub(super) phobos_attach_effect_types: Option<AttachEffectTypeCatalog>,
    pub(super) ares_global_controls: AresGlobalControls,
    pub(super) model_plan: super::voxel_model_plan::VoxelModelPlan,
    /// Ares `[MouseCursors]` list (`rules::ares_mouse_cursor`); Process
    /// retains it, destructive reset rebuilds its defaults.
    #[serde(default)]
    pub(super) ares_mouse_cursors: crate::rules::ares_mouse_cursor::MouseCursors,
    pub(super) families: HashMap<RulesTypeFamily, Vec<ProcessedType>>,
    /// TechnoTypeClass::Array A8EB04/countA8EB10. Each pair points to the
    /// existing family member; names alone cannot distinguish repeated native
    /// stored IDs allocated from inputs longer than AbstractType's24 bytes.
    /// Controls: tools/spatial_oracle/trigger_object_tech_level.json (+meta).
    pub(super) techno_type_order: Vec<(RulesTypeFamily, usize)>,
    pub(super) tiberiums: Vec<ProcessedType>,
    /// The existing Colors66D3A0 find-or-create catalog survives ordinary
    /// Process handoffs. Reset6686E6..668711 deletes and clears it alongside
    /// the Type registries, so destructive_reset leaves this field empty.
    pub(super) colors: Vec<(String, String)>,
    pub(super) rules_color_add: ColorAddTable,
    pub(super) rules_color_add_selectors: ColorAddSelectors,
    pub(super) rules_detail: DetailRules,
    pub(super) rules_ambient_change_rate: NativeF64Bits,
    pub(super) rules_ambient_change_step: NativeF64Bits,
    #[serde(default)]
    pub(super) rules_shroud_grow: bool,
    #[serde(default = "crate::rules::ruleset::default_shroud_rate")]
    pub(super) rules_shroud_rate: NativeF64Bits,
    #[serde(default = "crate::rules::ruleset::default_fog_rate")]
    pub(super) rules_fog_rate: NativeF64Bits,
    /// Unit/Infantry/Aircraft at +17D4/+17D8/+17DC. Constructor665650
    /// stores zero at667703..66770F; ReadAudioVisual66B673..66B74E updates
    /// them even when an admitted section omits their keys.
    pub(super) rules_class_extra_lights: [i32; 3],
    // Ctor665946/66594C/6659AC=-1; fixed Voc IDs represented by canonical names.
    pub(super) rules_global_sounds: GlobalSoundReferences,
    pub(super) rules_difficulty_rows: [crate::rules::ruleset::DifficultyRules; 3],
    /// Global89EA40 survives type resets and is saved by67F87D, restored67FAF1.
    pub(super) rules_land: crate::rules::terrain_rules::TerrainRules,
    pub(super) rules_gravity: i32,
    pub(super) rules_tree_strength: i32,
    pub(super) rules_wind_direction: i32,
    pub(super) rules_missile_rot_var: f64,
    pub(super) rules_safety_altitude: i32,
    pub(super) rules_parachute_max_fall_rate: i32,
    pub(super) rules_no_parachute_max_fall_rate: i32,
    pub(super) rules_survivor_divisors: [i32; 3],
    pub(super) rules_infantry_repair_step: i32,
    pub(super) rules_charge_to_drain_ratio: NativeF64Bits,
    pub(super) rules_unit_repair_rate: NativeF64Bits,
    pub(super) rules_infantry_repair_rate: NativeF64Bits,
    pub(super) rules_maximum_cheer_rate: i32,
    pub(super) rules_talk_bubble_time: i32,
    pub(super) rules_attack_cursor_on_disguise: bool,
    pub(super) rules_allow_shrouded_subterannean_moves: bool,
    pub(super) rules_spotlight: crate::rules::ruleset::SpotlightRules,
    pub(super) rules_ammo_crate_damage: i32,
    pub(super) rules_tiberium_explosion_damage: i32,
    pub(super) rules_ion_cannon_damage: i32,
    pub(super) rules_ion_blast_anim: String,
    pub(super) rules_ion_beam_anim: String,
    pub(super) rules_ion_cannon_warhead: String,
    /// Phobos `RulesExt::TimerBlinkColorScheme`: the runtime ColorScheme the
    /// blinking timer prints in, `[Colors]` entry `R / 2`, or -1 for `none`.
    #[cfg(feature = "phobos")]
    pub(super) rules_timer_blink_color_scheme: i32,
    /// Phobos `RulesExt::MissingCameo`: the file a type with no cameo art and
    /// a superweapon with no icon take in place of the stock `XXICON.SHP`
    /// (`Replace_XXICON_With_New` at `0x715A4D`, `0x716D13`, `0x6CE8AA` and
    /// `0x6CEE31`); empty after `none`.
    #[cfg(feature = "phobos")]
    #[serde(default = "crate::rules::ruleset::default_missing_cameo")]
    pub(super) rules_missing_cameo: String,
    pub(super) rules_mind_control_attack_line_frames: i32,
    pub(super) rules_drain_money_frame_delay: i32,
    pub(super) rules_drain_money_amount: i32,
    pub(super) rules_line_trail_override: [u8; 3],
    pub(super) rules_chrono_beam_color: [u8; 3],
    pub(super) rules_prism_support: PrismSupportRules,
    pub(super) rules_patrol_scan_minutes: NativeF64Bits,
    /// RulesClass+1174: constructor00665650 at00666E3F..00666E59 makes an
    /// empty integer vector. ReadGeneral0066FDB1..0066FDD9 uses
    /// ReadINI_IntVector00475D70 with the current vector as its default.
    /// It survives Type reset006686C0 and saved Rules state.
    pub(super) rules_ai_hate_delays: Vec<i32>,
    pub(super) rules_comp_easy_bonus: bool,
    pub(super) rules_prism_type: Option<String>,
    pub(super) rules_drop_zone_anim: Option<String>,
    pub(super) rules_drop_zone_radius: i32,
    pub(super) rules_behind: Option<String>,
    pub(super) rules_prerequisite_proc_alternate: Option<String>,
    /// The six `[General] Prerequisite*` lists (Rules `+0x358..+0x3E4`), in
    /// [`PrerequisiteGroup::ALL`] order.
    pub(super) rules_prerequisite_lists: [Vec<Prerequisite>; 6],
    pub(super) rules_building_types: GeneralBuildingTypes,
    pub(super) select_anim: SelectAnimRulesState,
    /// `[CombatDamage] DeathWeapon=` (Rules+FDC), the stored ID of its
    /// WeaponType: constructor null (`0x00666BB2`); ReadCombatDamage
    /// (`0x0066C57E..0x0066C5A8`) keeps it on an empty ReadString128 and
    /// otherwise stores the WeaponType factory's answer, null for `none`.
    /// The destructive reset clears it with the other Type references.
    pub(super) rules_death_weapon: Option<String>,
    pub(super) rules_type_references: RulesTypeReferences,
    /// `[CrateRules]` (`0x0066B900`): its scalars, three images and
    /// UnitCrateType.
    pub(super) rules_crate: CrateRules,
    /// ReadPowerups673E80 updates process globals; Reset6686C0 retains them.
    /// SaveMisc67F93E..67F982 / LoadMisc67FC97..67FCDB transfer all four arrays.
    pub(super) rules_powerups: PowerupTable,
    /// The three `RocketStruct` blocks with their types and warheads.
    pub(super) rules_missile_spawn: MissileSpawnRules,
    pub(super) rules_general_type_lists: GeneralTypeLists,
}

impl Default for NativeRulesRegistryState {
    /// A fresh registry in its thread's mode (`ares_switch::ares_way`).
    fn default() -> Self {
        Self::new(crate::rules::ares_switch::ares_way())
    }
}

impl NativeRulesRegistryState {
    /// A fresh RulesClass and empty Type registries, read the Ares way
    /// (Ares' generic prerequisites and armor catalog with them) or the stock
    /// way for the rest of the process.
    pub(crate) fn new(ares_way: bool) -> Self {
        // RulesClass constructor values; `GeneralRules::default` owns them.
        let general = crate::rules::ruleset::GeneralRules::default();
        Self {
            rules_threat_coefficient_defaults: general.threat_coefficient_defaults(),
            compatibility: RulesCompatibility::default(),
            reads_ares_way: ares_way,
            ares_prerequisites: ares_way.then(GenericPrerequisites::default),
            ares_armors: ares_way.then(ArmorCatalog::default),
            phobos_radiation_types: crate::rules::PHOBOS_ENABLED
                .then(RadiationTypeCatalog::with_default),
            phobos_shield_types: crate::rules::PHOBOS_ENABLED.then(ShieldTypeCatalog::default),
            phobos_insignia_types: crate::rules::PHOBOS_ENABLED.then(InsigniaTypeCatalog::default),
            phobos_digital_display_types: crate::rules::PHOBOS_ENABLED
                .then(DigitalDisplayTypeCatalog::default),
            phobos_attach_effect_types: crate::rules::PHOBOS_ENABLED
                .then(AttachEffectTypeCatalog::default),
            ares_global_controls: AresGlobalControls::default(),
            model_plan: Default::default(),
            ares_mouse_cursors: Default::default(),
            families: HashMap::new(),
            techno_type_order: Vec::new(),
            tiberiums: Vec::new(),
            colors: Vec::new(),
            rules_color_add: ColorAddTable::default(),
            rules_color_add_selectors: ColorAddSelectors::default(),
            rules_detail: DetailRules::default(),
            rules_ambient_change_rate: crate::rules::ruleset::DEFAULT_AMBIENT_CHANGE_RATE,
            rules_ambient_change_step: crate::rules::ruleset::DEFAULT_AMBIENT_CHANGE_STEP,
            rules_shroud_grow: general.shroud_grow,
            rules_shroud_rate: general.shroud_rate,
            rules_fog_rate: general.fog_rate,
            rules_class_extra_lights: [
                general.extra_unit_light,
                general.extra_infantry_light,
                general.extra_aircraft_light,
            ],
            rules_global_sounds: GlobalSoundReferences::default(),
            rules_difficulty_rows: general.difficulty_rows,
            rules_land: Default::default(),
            rules_gravity: general.gravity,
            rules_tree_strength: general.tree_strength,
            rules_wind_direction: general.wind_direction(),
            rules_missile_rot_var: general.missile_rot_var,
            rules_safety_altitude: general.safety_altitude,
            rules_parachute_max_fall_rate: general.parachute_max_fall_rate,
            rules_no_parachute_max_fall_rate: general.no_parachute_max_fall_rate,
            rules_survivor_divisors: crate::rules::ruleset::DEFAULT_SURVIVOR_DIVISORS,
            rules_infantry_repair_step: general.infantry_repair_step,
            rules_charge_to_drain_ratio: general.charge_to_drain_ratio,
            rules_unit_repair_rate: NativeF64Bits::from_bits(general.unit_repair_rate.to_bits()),
            rules_infantry_repair_rate: NativeF64Bits::from_bits(
                general.infantry_repair_rate.to_bits(),
            ),
            rules_maximum_cheer_rate: crate::rules::ruleset::DEFAULT_MAXIMUM_CHEER_RATE,
            rules_talk_bubble_time: general.talk_bubble_time,
            rules_attack_cursor_on_disguise: general.attack_cursor_on_disguise,
            rules_allow_shrouded_subterannean_moves: general.allow_shrouded_subterannean_moves(),
            rules_spotlight: general.spotlight,
            // Rules665CD4=700; pointer fields665A18/665A24/666BD5 start null.
            rules_ammo_crate_damage: 100,
            // RulesCtor665CE8/665D5C: signed Rules+568 starts at100.
            rules_tiberium_explosion_damage: 100,
            rules_ion_cannon_damage: 700,
            rules_ion_blast_anim: String::new(),
            rules_ion_beam_anim: String::new(),
            rules_ion_cannon_warhead: String::new(),
            // RulesExt::ExtData constructor `TimerBlinkColorScheme { 5 }`, the
            // scheme the game blinks in.
            #[cfg(feature = "phobos")]
            rules_timer_blink_color_scheme: 5,
            #[cfg(feature = "phobos")]
            rules_missing_cameo: crate::rules::ruleset::DEFAULT_MISSING_CAMEO.to_owned(),
            // MindControlAttackLineFrames, Rules+310: constructor 0x00665ABE.
            rules_mind_control_attack_line_frames: 15,
            rules_drain_money_frame_delay: general.drain_money_frame_delay,
            rules_drain_money_amount: general.drain_money_amount,
            rules_line_trail_override: general.line_trail_color_override,
            rules_chrono_beam_color: general.chrono_beam_color,
            rules_prism_support: PrismSupportRules::default(),
            rules_patrol_scan_minutes: general.patrol_scan_minutes,
            rules_ai_hate_delays: general.ai_hate_delays().to_vec(),
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
            rules_powerups: PowerupTable::default(),
            rules_missile_spawn: MissileSpawnRules::default(),
            rules_general_type_lists: GeneralTypeLists::default(),
        }
    }
}

impl NativeRulesRegistryState {
    /// Whether this registry's Rules are read the Ares way
    /// (`ares_switch::ares_way`).
    pub(crate) fn reads_ares_way(&self) -> bool {
        self.reads_ares_way
    }

    /// Input to the existing read-only RMG source projection. Other domains
    /// retain its prior default-based policy; the process-global tables start
    /// with the same retained inputs as a real Reset6686C0.
    pub(crate) fn retained_random_map_projection_seed(&self) -> Self {
        Self {
            rules_powerups: self.rules_powerups.clone(),
            rules_land: self.rules_land.clone(),
            rules_global_sounds: self.rules_global_sounds.clone(),
            rules_class_extra_lights: self.rules_class_extra_lights,
            // ReadCrateRules66BA08 retains the same process scalar on a
            // failed root lookup in an offline preview. Network agreement
            // projects from the cold registry instead.
            rules_crate: CrateRules {
                heal_crate_sound: self.rules_crate.heal_crate_sound.clone(),
                ..CrateRules::default()
            },
            ..Self::new(self.reads_ares_way)
        }
    }

    /// Read one presentation HSV from the retained paired Colors registry.
    /// Native66D3A0 appends kind1 then kind53 for each entry; Ares loading
    /// stores a kind1 ordinal. A negative index is no override: the loading
    /// screen's own stock AlliedLoad/SovietLoad branch keeps the color.
    /// The process owner exposes the value, never the mutable/raw registry.
    pub(crate) fn loading_text_hsv(&self, index: i32) -> Option<[u8; 3]> {
        // This representation is owned by allocate_colors; both native
        // variants share one HSV. No final RuleSet list participates.
        let (name, value) = self.colors.get(usize::try_from(index).ok()? / 2)?;
        let mut section = IniSection::new("Colors".into());
        section.set(name, value);
        Some(crate::rules::color_scheme::read_scheme_hsv(&section, name))
    }

    /// Real non-RMG Full_Init686B65 calls ClearClasses6851F0, whose Ares
    /// ScenarioClear10004EF1/10004EF7 reconstructs RulesExt1002A670. This is
    /// separate from Type deletion6686C0, which retains the extension below.
    pub(crate) fn clear_scenario_global_controls(&mut self) {
        self.ares_global_controls = AresGlobalControls::default();
    }

    pub(crate) fn country_projection(&self) -> impl Iterator<Item = CountryProjection<'_>> {
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
                    member.country_parent.as_str(),
                    &member.country_veterancy,
                )
            })
    }

    pub(crate) fn side_projection(&self) -> impl Iterator<Item = SideProjection<'_>> {
        self.families
            .get(&RulesTypeFamily::Side)
            .into_iter()
            .flatten()
            .map(|member| {
                // Only Rules read the Ares way allocate the extension.
                let extension = member.ares_side.as_ref().map(|state| &state.fields);
                (member.native_stored_id.as_str(), extension)
            })
    }

    pub(super) fn validate_techno_type_order(&self) -> Result<(), RulesError> {
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

    pub(super) fn validate_radiation_types(&self) -> Result<(), RulesError> {
        if self.phobos_radiation_types.is_some() != crate::rules::PHOBOS_ENABLED {
            return Err(RulesError::InvalidSavedState(
                "saved radiation type catalog does not match the compiled capability",
            ));
        }
        Ok(())
    }

    pub(super) fn validate_shield_types(&self) -> Result<(), RulesError> {
        if self.phobos_shield_types.is_some() != crate::rules::PHOBOS_ENABLED {
            return Err(RulesError::InvalidSavedState(
                "saved shield type catalog does not match the compiled capability",
            ));
        }
        Ok(())
    }

    #[cfg(feature = "phobos")]
    pub(super) fn validate_attach_effect_types(&self) -> Result<(), RulesError> {
        if self.phobos_attach_effect_types.is_some() != crate::rules::PHOBOS_ENABLED {
            return Err(RulesError::InvalidSavedState(
                "saved attach effect type catalog does not match the compiled capability",
            ));
        }
        Ok(())
    }

    #[cfg(feature = "phobos")]
    pub(super) fn validate_digital_display_types(&self) -> Result<(), RulesError> {
        if self.phobos_digital_display_types.is_some() != crate::rules::PHOBOS_ENABLED {
            return Err(RulesError::InvalidSavedState(
                "saved digital display type catalog does not match the compiled capability",
            ));
        }
        Ok(())
    }

    pub(super) fn validate_prerequisites(&self) -> Result<(), RulesError> {
        if self.ares_prerequisites.is_some() != self.reads_ares_way {
            return Err(RulesError::InvalidSavedState(
                "saved prerequisite catalog does not match the saved Ares mode",
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
            // Ares' extra lists stay empty unless read the Ares way.
            member
                .prerequisite
                .iter()
                .chain(&member.prerequisite_override)
                .chain(member.techno_combat.prerequisites().entries())
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

    pub(super) fn techno_type_states(&self) -> impl Iterator<Item = (ObjectCategory, &str, i32)> {
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
        voxel_damage: impl IntoIterator<Item = (String, i32)>,
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
        let mut resolved_voxels = Vec::new();
        for (id, damage) in voxel_damage {
            let index = self
                .exact_type_index(RulesTypeFamily::VoxelAnimation, &id)
                .ok_or(RulesError::InvalidSavedState(
                    "outgoing VoxelAnim Damage does not identify one exact live VoxelAnimType",
                ))?;
            resolved_voxels.push((index, damage));
        }
        self.consume_outgoing_type_tech_levels(tech_levels)?;
        for (index, slot, name) in resolved {
            self.families
                .get_mut(&RulesTypeFamily::Building)
                .expect("validated BuildingType family")[index]
                .building_upgrade
                .set_normal_art_name(slot, name);
        }
        for (index, damage) in resolved_voxels {
            self.families
                .get_mut(&RulesTypeFamily::VoxelAnimation)
                .expect("validated VoxelAnimType family")[index]
                .voxel_anim_damage = damage;
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
        let ares_way = self.reads_ares_way;
        // RulesClass::Reset6686C0 deletes Overlay/Anim types but has no
        // Tiberium deletion loop. PointerExpired722140 drops their bindings;
        // scalar fields, body history and Tiberium array identity survive.
        let mut tiberiums = self.tiberiums;
        for member in &mut tiberiums {
            member.tiberium_image.clear_overlay();
            member.tiberium_debris.clear();
        }
        Self {
            tiberiums,
            // Reset6686C0 deletes Types, retaining the same RulesClass scalars.
            rules_threat_coefficient_defaults: self.rules_threat_coefficient_defaults,
            //6686C0..668BF0 has no RulesExt reconstruction. Early-map Process
            //686D35 precedes this TypeReset6876AC and its flags/phase survive.
            ares_global_controls: self.ares_global_controls,
            // Phobos allocates RulesExt only with RulesClass (0x667A1D).
            #[cfg(feature = "phobos")]
            rules_timer_blink_color_scheme: self.rules_timer_blink_color_scheme,
            #[cfg(feature = "phobos")]
            rules_missing_cameo: self.rules_missing_cameo,
            rules_color_add: self.rules_color_add,
            rules_color_add_selectors: self.rules_color_add_selectors,
            rules_detail: self.rules_detail,
            rules_ambient_change_rate: self.rules_ambient_change_rate,
            rules_ambient_change_step: self.rules_ambient_change_step,
            rules_shroud_grow: self.rules_shroud_grow,
            rules_shroud_rate: self.rules_shroud_rate,
            rules_fog_rate: self.rules_fog_rate,
            rules_class_extra_lights: self.rules_class_extra_lights,
            rules_global_sounds: self.rules_global_sounds,
            rules_difficulty_rows: self.rules_difficulty_rows,
            rules_land: self.rules_land,
            rules_gravity: self.rules_gravity,
            rules_tree_strength: self.rules_tree_strength,
            rules_drop_zone_radius: self.rules_drop_zone_radius,
            rules_wind_direction: self.rules_wind_direction,
            rules_missile_rot_var: self.rules_missile_rot_var,
            rules_safety_altitude: self.rules_safety_altitude,
            rules_parachute_max_fall_rate: self.rules_parachute_max_fall_rate,
            rules_no_parachute_max_fall_rate: self.rules_no_parachute_max_fall_rate,
            rules_survivor_divisors: self.rules_survivor_divisors,
            rules_infantry_repair_step: self.rules_infantry_repair_step,
            rules_charge_to_drain_ratio: self.rules_charge_to_drain_ratio,
            rules_unit_repair_rate: self.rules_unit_repair_rate,
            rules_infantry_repair_rate: self.rules_infantry_repair_rate,
            rules_maximum_cheer_rate: self.rules_maximum_cheer_rate,
            rules_talk_bubble_time: self.rules_talk_bubble_time,
            rules_attack_cursor_on_disguise: self.rules_attack_cursor_on_disguise,
            rules_allow_shrouded_subterannean_moves: self.rules_allow_shrouded_subterannean_moves,
            rules_spotlight: self.rules_spotlight,
            rules_ammo_crate_damage: self.rules_ammo_crate_damage,
            rules_tiberium_explosion_damage: self.rules_tiberium_explosion_damage,
            rules_ion_cannon_damage: self.rules_ion_cannon_damage,
            rules_mind_control_attack_line_frames: self.rules_mind_control_attack_line_frames,
            rules_drain_money_frame_delay: self.rules_drain_money_frame_delay,
            rules_drain_money_amount: self.rules_drain_money_amount,
            rules_line_trail_override: self.rules_line_trail_override,
            rules_chrono_beam_color: self.rules_chrono_beam_color,
            rules_prism_support: self.rules_prism_support,
            rules_patrol_scan_minutes: self.rules_patrol_scan_minutes,
            rules_ai_hate_delays: self.rules_ai_hate_delays,
            rules_comp_easy_bonus: self.rules_comp_easy_bonus,
            // Plain index vectors, so RulesClass keeps them; the next pass
            // reads them as its default.
            rules_prerequisite_lists: self.rules_prerequisite_lists,
            rules_crate: self.rules_crate.without_type_references(),
            // Numeric animation indices survive deletion and may name a new
            // slot after recreation; they are not swizzled Type pointers.
            rules_powerups: self.rules_powerups,
            rules_missile_spawn: self.rules_missile_spawn.without_type_references(),
            ..Self::new(ares_way)
        }
    }
}
