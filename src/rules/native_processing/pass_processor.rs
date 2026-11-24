//! `RulesPassProcessor`: the state of one `RulesClass::Process` run, the pass
//! order, family lookup and allocation, type-data dispatch and the final receipt.

use super::{
    AresGlobalControls, EXPLICIT_RULE_TYPE_FAMILIES, GeneralTypeLists, NativeRulesRegistryState,
    NativeTypeConstructionEvent, NativeTypeConstructionTrace, PROJECTED_RULE_TYPE_FAMILIES,
    ProcessedType, RulesTypeFamily, SelectAnimRulesState, SideRegisteredTypes,
    anim_type_runtime_key, ares_type_index_of_name, find_existing_type, find_or_construct_type,
    house_type_index_of_name, super_weapon_index_of_name,
};
use crate::rules::ares_armor::ArmorCatalog;
use crate::rules::ares_prerequisites::GenericPrerequisites;
use crate::rules::armor::armor_index;
use crate::rules::attach_effect::AttachEffectTypeCatalog;
use crate::rules::color_add::ColorAddSelectors;
use crate::rules::color_add::ColorAddTable;
use crate::rules::compatibility::RulesCompatibility;
use crate::rules::crate_rules::CrateRules;
use crate::rules::digital_display::DigitalDisplayTypeCatalog;
use crate::rules::error::RulesError;
use crate::rules::ini_parser::IniFile;
use crate::rules::ini_parser::IniSection;
use crate::rules::ini_parser::is_native_none_type_name;
use crate::rules::insignia_type::InsigniaTypeCatalog;
use crate::rules::missile_spawn::MissileSpawnRules;
use crate::rules::object_image::ImageTheater;
use crate::rules::phobos_radiation::RadiationTypeCatalog;
use crate::rules::phobos_shields::ShieldTypeCatalog;
use crate::rules::powerups::PowerupTable;
use crate::rules::prerequisite::Prerequisite;
use crate::rules::prerequisite::PrerequisiteGroup;
use crate::rules::prerequisite::parse_prerequisites;
use crate::rules::rules_type_references::RulesTypeReferences;
use crate::rules::ruleset::DetailRules;
use crate::rules::ruleset::GeneralBuildingTypes;
use crate::rules::ruleset::PrismSupportRules;
use crate::rules::sound_ini::EvaRegistry;
use crate::rules::sound_ini::GlobalSoundReferences;
use crate::rules::sound_ini::SoundRegistry;
use crate::util::native_x87::NativeF64Bits;
use std::collections::HashMap;
use std::collections::HashSet;

/// VERA indexes Sides in one byte (`SideIdx`); gamemd's array has no bound.
/// A VERA20k bound.
const SIDE_LIMIT: usize = 1 << u8::BITS;

/// VERA indexes Countries in two bytes (`CountryIdx`); gamemd's array has no
/// bound. A VERA20k bound.
const COUNTRY_LIMIT: usize = 1 << u16::BITS;

/// The longest `[Sides]` membership list VERA builds: one entry per Country
/// VERA can hold. gamemd keeps duplicates and has no bound, so a Side that
/// names an earlier Side many times multiplies that Side's list; past this,
/// the list is refused. A VERA20k bound; retail lists hold a handful.
const SIDE_MEMBER_LIMIT: usize = COUNTRY_LIMIT;

#[derive(Debug, Clone)]
pub(super) struct RulesPassProcessor {
    pub(super) rules_threat_coefficient_defaults: super::threat_coefficients::ThreatCoefficients,
    pub(super) image_theater: ImageTheater,
    pub(super) compatibility: RulesCompatibility,
    /// The registry's mode, which its Process call opens (`ares_switch::ares_way`).
    pub(super) reads_ares_way: bool,
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
    /// Ares `[MouseCursors]` list (`rules::ares_mouse_cursor`); Process
    /// retains it, destructive reset rebuilds its defaults.
    pub(super) ares_mouse_cursors: crate::rules::ares_mouse_cursor::MouseCursors,
    pub(super) ordinary: Option<IniFile>,
    pub(super) crate_rules: CrateRules,
    pub(super) powerups: PowerupTable,
    pub(super) general_type_lists: GeneralTypeLists,
    pub(super) missile_spawn: MissileSpawnRules,
    pub(super) model_plan: super::voxel_model_plan::VoxelModelPlan,
    pub(super) families: HashMap<RulesTypeFamily, Vec<ProcessedType>>,
    pub(super) techno_type_order: Vec<(RulesTypeFamily, usize)>,
    pub(super) native_type_construction_events: Vec<NativeTypeConstructionEvent>,
    pub(super) tiberiums: Vec<ProcessedType>,
    pub(super) colors: Vec<(String, String)>,
    pub(super) rules_color_add: ColorAddTable,
    pub(super) rules_color_add_selectors: ColorAddSelectors,
    pub(super) rules_detail: DetailRules,
    pub(super) rules_ambient_change_rate: NativeF64Bits,
    pub(super) rules_ambient_change_step: NativeF64Bits,
    pub(super) rules_shroud_grow: bool,
    pub(super) rules_shroud_rate: NativeF64Bits,
    pub(super) rules_fog_rate: NativeF64Bits,
    /// Same retained RulesClass triple as NativeRulesRegistryState; this
    /// processor receives and returns its sole ownership with each pass.
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
    #[cfg(feature = "phobos")]
    pub(super) rules_timer_blink_color_scheme: i32,
    /// Phobos `[AudioVisual] MissingCameo=`, retained with the other Rules
    /// scalars for the pass.
    #[cfg(feature = "phobos")]
    pub(super) rules_missing_cameo: String,
    pub(super) rules_mind_control_attack_line_frames: i32,
    pub(super) rules_drain_money_frame_delay: i32,
    pub(super) rules_drain_money_amount: i32,
    pub(super) rules_line_trail_override: [u8; 3],
    pub(super) rules_chrono_beam_color: [u8; 3],
    pub(super) rules_prism_support: PrismSupportRules,
    pub(super) rules_patrol_scan_minutes: NativeF64Bits,
    pub(super) rules_ai_hate_delays: Vec<i32>,
    pub(super) rules_comp_easy_bonus: bool,
    pub(super) rules_prism_type: Option<String>,
    pub(super) rules_drop_zone_anim: Option<String>,
    pub(super) rules_drop_zone_radius: i32,
    pub(super) rules_behind: Option<String>,
    pub(super) rules_prerequisite_proc_alternate: Option<String>,
    pub(super) rules_prerequisite_lists: [Vec<Prerequisite>; 6],
    pub(super) rules_building_types: GeneralBuildingTypes,
    pub(super) select_anim: SelectAnimRulesState,
    pub(super) rules_death_weapon: Option<String>,
    pub(super) rules_type_references: RulesTypeReferences,
    /// What this Process call's passes read, for the per-game Ares switch
    /// (`rules::ares_switch`). Not retained with the registry: each scenario
    /// Process reads every one of its layers again.
    pub(super) ares_reads: crate::rules::ares_switch::KeyReads,
}

impl Default for RulesPassProcessor {
    /// A fresh RulesClass and empty Type registries; the constructor values
    /// have one owner, [`NativeRulesRegistryState::default`].
    fn default() -> Self {
        Self::with_registry_state(NativeRulesRegistryState::default())
    }
}

impl RulesPassProcessor {
    pub(super) fn with_registry_state(registry_state: NativeRulesRegistryState) -> Self {
        Self {
            rules_threat_coefficient_defaults: registry_state.rules_threat_coefficient_defaults,
            image_theater: ImageTheater::Uninitialized,
            compatibility: registry_state.compatibility,
            reads_ares_way: registry_state.reads_ares_way,
            ares_prerequisites: registry_state.ares_prerequisites,
            ares_armors: registry_state.ares_armors,
            phobos_radiation_types: registry_state.phobos_radiation_types,
            phobos_shield_types: registry_state.phobos_shield_types,
            phobos_insignia_types: registry_state.phobos_insignia_types,
            phobos_digital_display_types: registry_state.phobos_digital_display_types,
            phobos_attach_effect_types: registry_state.phobos_attach_effect_types,
            ares_global_controls: registry_state.ares_global_controls,
            model_plan: registry_state.model_plan,
            ares_mouse_cursors: registry_state.ares_mouse_cursors,
            families: registry_state.families,
            techno_type_order: registry_state.techno_type_order,
            tiberiums: registry_state.tiberiums,
            colors: registry_state.colors,
            rules_color_add: registry_state.rules_color_add,
            rules_color_add_selectors: registry_state.rules_color_add_selectors,
            rules_detail: registry_state.rules_detail,
            rules_ambient_change_rate: registry_state.rules_ambient_change_rate,
            rules_ambient_change_step: registry_state.rules_ambient_change_step,
            rules_shroud_grow: registry_state.rules_shroud_grow,
            rules_shroud_rate: registry_state.rules_shroud_rate,
            rules_fog_rate: registry_state.rules_fog_rate,
            rules_class_extra_lights: registry_state.rules_class_extra_lights,
            rules_global_sounds: registry_state.rules_global_sounds,
            rules_difficulty_rows: registry_state.rules_difficulty_rows,
            rules_land: registry_state.rules_land,
            rules_gravity: registry_state.rules_gravity,
            rules_tree_strength: registry_state.rules_tree_strength,
            rules_wind_direction: registry_state.rules_wind_direction,
            rules_missile_rot_var: registry_state.rules_missile_rot_var,
            rules_safety_altitude: registry_state.rules_safety_altitude,
            rules_parachute_max_fall_rate: registry_state.rules_parachute_max_fall_rate,
            rules_no_parachute_max_fall_rate: registry_state.rules_no_parachute_max_fall_rate,
            rules_survivor_divisors: registry_state.rules_survivor_divisors,
            rules_infantry_repair_step: registry_state.rules_infantry_repair_step,
            rules_charge_to_drain_ratio: registry_state.rules_charge_to_drain_ratio,
            rules_unit_repair_rate: registry_state.rules_unit_repair_rate,
            rules_infantry_repair_rate: registry_state.rules_infantry_repair_rate,
            rules_maximum_cheer_rate: registry_state.rules_maximum_cheer_rate,
            rules_talk_bubble_time: registry_state.rules_talk_bubble_time,
            rules_attack_cursor_on_disguise: registry_state.rules_attack_cursor_on_disguise,
            rules_allow_shrouded_subterannean_moves: registry_state
                .rules_allow_shrouded_subterannean_moves,
            rules_spotlight: registry_state.rules_spotlight,
            rules_ammo_crate_damage: registry_state.rules_ammo_crate_damage,
            rules_tiberium_explosion_damage: registry_state.rules_tiberium_explosion_damage,
            rules_ion_cannon_damage: registry_state.rules_ion_cannon_damage,
            rules_ion_blast_anim: registry_state.rules_ion_blast_anim,
            rules_ion_beam_anim: registry_state.rules_ion_beam_anim,
            rules_ion_cannon_warhead: registry_state.rules_ion_cannon_warhead,
            #[cfg(feature = "phobos")]
            rules_timer_blink_color_scheme: registry_state.rules_timer_blink_color_scheme,
            #[cfg(feature = "phobos")]
            rules_missing_cameo: registry_state.rules_missing_cameo,
            rules_mind_control_attack_line_frames: registry_state
                .rules_mind_control_attack_line_frames,
            rules_drain_money_frame_delay: registry_state.rules_drain_money_frame_delay,
            rules_drain_money_amount: registry_state.rules_drain_money_amount,
            rules_line_trail_override: registry_state.rules_line_trail_override,
            rules_chrono_beam_color: registry_state.rules_chrono_beam_color,
            rules_prism_support: registry_state.rules_prism_support,
            rules_patrol_scan_minutes: registry_state.rules_patrol_scan_minutes,
            rules_ai_hate_delays: registry_state.rules_ai_hate_delays,
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
            powerups: registry_state.rules_powerups,
            native_type_construction_events: Vec::new(),
            ares_reads: Default::default(),
        }
    }

    /// One `RulesClass::Process @ 0x00668BF0` pass, its reader lookups
    /// recorded for the per-game Ares switch.
    pub(super) fn apply_pass(
        &mut self,
        pass: &IniFile,
        fixed_art: &IniFile,
        fixed_eva: &EvaRegistry,
        primary_rules: bool,
        fixed_sounds: &SoundRegistry,
    ) -> Result<(), RulesError> {
        let (result, reads) = crate::rules::ares_switch::record(|| {
            self.process_pass(pass, fixed_art, fixed_eva, primary_rules, fixed_sounds)
        });
        self.ares_reads.absorb(reads);
        result
    }

    fn process_pass(
        &mut self,
        pass: &IniFile,
        fixed_art: &IniFile,
        fixed_eva: &EvaRegistry,
        primary_rules: bool,
        fixed_sounds: &SoundRegistry,
    ) -> Result<(), RulesError> {
        let ares_way = crate::rules::ares_switch::ares_way();
        if ares_way {
            self.ares_global_controls.read_pass(pass);
        }
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
                self.allocate_explicit_family(pass, registry, family, 0x20);
            }
        }

        // JumpjetControls and MultiplayerSettings contain no Type factory.
        self.allocate_ai_references(pass);
        // Process668ED0 reads Powerups after explicit Animations/AI, before
        // General668EE8 and TypeData668EF0 can allocate more AnimTypes.
        self.powerups.apply_pass(pass, |name| {
            find_existing_type(&self.families, RulesTypeFamily::Animation, name)
        });
        self.read_prerequisite_lists(pass);
        self.allocate_general_references(pass);
        // Ares RulesData_LoadBeforeTypeData (hook 0x679A15, 1002AEF0):
        // 1002B2FF..1002B34D reads each [WeaponTypes] value into the 0x800
        // buffer and calls WeaponType FindOrAllocate10010570, before the
        // type bodies. AnimType's Weapon= remains Find-only1000A370, so a
        // declared animation-only weapon must already exist here.
        // Source: Ares src/Ext/Rules/Body.cpp, LoadBeforeTypeData.
        if ares_way {
            self.allocate_explicit_family(pass, "WeaponTypes", RulesTypeFamily::Weapon, 0x800);
            self.ares_global_controls.read_before_type_data(pass);
        }
        // The same 1002AB60 reads the tiberium globals after CanMakeStuffUp;
        // its warhead and anim are Find-only reads against this pass's
        // registries so far.
        if ares_way {
            let mut tiberium = std::mem::take(&mut self.ares_global_controls.tiberium);
            tiberium.read_before_type_data(pass, |section, key, warhead| {
                let family = if warhead {
                    RulesTypeFamily::Warhead
                } else {
                    RulesTypeFamily::Animation
                };
                self.resolve_existing_type_from(section, key, family)
            });
            self.ares_global_controls.tiberium = tiberium;
        }
        // Phobos `AttachEffectTypeClass::LoadFromINI`'s `Read<true>` type
        // factories run before this pass's type bodies, so a WeaponType or
        // WarheadType only an AttachEffect key names is read in this pass.
        #[cfg(feature = "phobos")]
        self.allocate_attach_effect_references(pass);
        // Phobos `RulesExt::LoadBeforeTypeData` runs
        // `InsigniaTypeClass::LoadFromINIList` before this pass's type bodies,
        // so a same-pass `InsigniaType=` resolves against it.
        #[cfg(feature = "phobos")]
        self.allocate_insignia_types(pass);
        self.process_type_data(pass, fixed_art, fixed_eva, primary_rules, fixed_sounds);
        // RulesExt LoadAfterTypeData: installed679CAF ->1002C9B0 ->1002AAF0
        // ->1002A800 reads EngineerDamage and EngineerAlwaysCaptureTech,
        // then HSBuilding at1002A8DD with the BuildingType
        // pointer-vector reader10010850, which finds and never allocates.
        // 1002A800 reads InfantryElectrocuted first (Find only), then
        // EnemyWrench and, after HSBuilding, the hunter-seeker values.
        if ares_way {
            let electrocuted = pass.section("AudioVisual").and_then(|section| {
                self.resolve_existing_type_from(
                    section,
                    "InfantryElectrocuted",
                    RulesTypeFamily::Animation,
                )
            });
            self.ares_global_controls
                .general
                .read_after_type_data(pass, electrocuted);
            self.ares_global_controls
                .building
                .read_after_type_data(pass);
        }
        if ares_way
            && let Some(buildings) = pass.section("SpecialWeapons").and_then(|section| {
                self.resolve_existing_list_from(
                    section,
                    "HSBuilding",
                    RulesTypeFamily::Building,
                    2048,
                )
            })
        {
            self.ares_global_controls.hunter_seeker_buildings = buildings;
        }
        // 1002A800 then reads DropPodTrailer, DropPodTypes, DropPodMinimum
        // and DropPodMaximum (both pointer reads Find, never allocate).
        if ares_way {
            let general = pass.section("General");
            let trailer = general.and_then(|section| {
                self.resolve_existing_type_from(
                    section,
                    "DropPodTrailer",
                    RulesTypeFamily::Animation,
                )
            });
            let types = general.and_then(|section| {
                self.resolve_existing_techno_type_list_from(section, "DropPodTypes")
            });
            self.ares_global_controls
                .drop_pod
                .read_pass(general, trailer, types);
        }
        // The same LoadAfterTypeData1002A800 continues with General's
        // BountyEnablers1002AAAB and AudioVisual's BountyDisplay1002AAC5.
        if ares_way {
            let enablers = pass.section("General").and_then(|section| {
                self.resolve_existing_list_from(
                    section,
                    "BountyEnablers",
                    RulesTypeFamily::Building,
                    2048,
                )
            });
            // 1002AAF0 sets the firestorm anims' Find defaults on the
            // rulesmd.ini pass only; 1002A800 reads the firestorm keys after
            // TogglePower and before BountyEnablers (Find-only pointers).
            let default_anims = primary_rules.then(|| {
                crate::rules::ares_firestorm::FIRESTORM_DEFAULT_ANIMS.map(|name| {
                    find_existing_type(&self.families, RulesTypeFamily::Animation, name)
                        .map(|index| self.resolved_type_key(RulesTypeFamily::Animation, index))
                })
            });
            let audio_visual = pass.section("AudioVisual");
            let anims = crate::rules::ares_firestorm::FIRESTORM_ANIM_KEYS.map(|key| {
                audio_visual.and_then(|section| {
                    self.resolve_existing_type_from(section, key, RulesTypeFamily::Animation)
                })
            });
            // Retail [CombatDamage] already names the TS key's warhead.
            let warhead = pass.section("CombatDamage").and_then(|section| {
                crate::rules::ares_switch::not_ares_read(|| {
                    self.resolve_existing_type_from(
                        section,
                        "FirestormWarhead",
                        RulesTypeFamily::Warhead,
                    )
                })
            });
            self.ares_global_controls.firestorm.read_after_type_data(
                pass,
                crate::rules::ares_firestorm::FirestormReferences {
                    default_anims,
                    anims,
                    warhead,
                },
            );
            self.ares_global_controls
                .bounty
                .read_after_type_data(pass, enablers);
            //1002A800 reads CloakHeight right after DecloakSound, which the
            // fixed-catalog sound binder owns.
            self.ares_global_controls
                .cloak_power
                .read_after_type_data(pass);
        }

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
        crate_rules.apply_pass(pass, fixed_sounds, |section, key| {
            let family = if key == "UnitCrateType" {
                RulesTypeFamily::Vehicle
            } else {
                RulesTypeFamily::Overlay
            };
            self.resolve_scalar_from(section, key, family, 0x80)
        });
        self.crate_rules = crate_rules;
        self.rules_land.apply_pass(pass);
        self.allocate_combat_references(pass);
        // ReadGeneral's rocket blocks and ReadCombatDamage's rocket warheads.
        // They read and write no other Process state, so one call per pass
        // keeps their order.
        self.missile_spawn.apply_pass(pass);
        self.allocate_radiation_references(pass);
        #[cfg(feature = "phobos")]
        self.allocate_radiation_types(pass);
        #[cfg(feature = "phobos")]
        self.allocate_shield_types(pass);
        #[cfg(feature = "phobos")]
        self.allocate_digital_display_types(pass);
        #[cfg(feature = "phobos")]
        self.allocate_attach_effect_types(pass);
        // Elevation and Wall contain no Type factories.
        self.allocate_audio_visual_references(pass, fixed_sounds);
        self.process_special_weapons(pass);
        self.process_tiberiums(pass)?;
        // AdvancedCommandBar contains no Type factory.
        self.compact_model_plan();
        Ok(())
    }

    pub(super) fn allocate_explicit_family(
        &mut self,
        pass: &IniFile,
        registry: &str,
        family: RulesTypeFamily,
        capacity: usize,
    ) {
        let Some(section) = pass.section(registry) else {
            return;
        };
        for key in section.keys() {
            let identity = section.read_string(key, "", capacity);
            if !identity.is_empty() {
                self.find_or_allocate(family, &identity);
            }
        }
    }

    pub(super) fn family_mut(&mut self, family: RulesTypeFamily) -> &mut Vec<ProcessedType> {
        self.families.entry(family).or_default()
    }

    pub(super) fn find_existing(&self, family: RulesTypeFamily, incoming: &str) -> Option<usize> {
        find_existing_type(&self.families, family, incoming)
    }

    pub(super) fn country_index_of_name(&self, incoming: &str) -> i32 {
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
    ///
    /// gamemd appends every member, duplicates included, with no bound
    /// (`CCINIClass::ReadHouseTypeList @ 0x004767C0`), so a chain of Sides
    /// that each name the one before many times grows the list
    /// exponentially. VERA refuses a list past [`SIDE_MEMBER_LIMIT`] instead.
    pub(super) fn read_side_members(
        &self,
        section: &IniSection,
        key: &str,
        current: &[i32],
    ) -> Result<Vec<i32>, RulesError> {
        let Some(tokens) = section.read_list(key, 0x80) else {
            return Ok(current.to_vec());
        };
        let mut members = Vec::new();
        for token in tokens {
            let country = self.country_index_of_name(token);
            let expansion = if country != -1 {
                std::slice::from_ref(&country)
            } else if let Some(index) = self.find_existing(RulesTypeFamily::Side, token) {
                &self.families[&RulesTypeFamily::Side][index].side_country_members[..]
            } else {
                &[]
            };
            if members.len() + expansion.len() > SIDE_MEMBER_LIMIT {
                return Err(RulesError::InvalidValue {
                    section: "Sides".to_string(),
                    key: key.to_string(),
                    expected: format!(
                        "at most {SIDE_MEMBER_LIMIT} Countries, counting each Side named \
                         by its own list (a VERA20k bound)"
                    ),
                    value: section.raw_value(key).unwrap_or_default().to_string(),
                });
            }
            members.extend_from_slice(expansion);
        }
        Ok(members)
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
            let members = self.read_side_members(section, key, prior)?;
            let side_index = i32::try_from(index).expect("native Side index exceeds i32");
            let mut countries = self.families.get_mut(&RulesTypeFamily::Country);
            let mut unresolved = None;
            for &member in &members {
                let country = usize::try_from(member)
                    .ok()
                    .and_then(|member| countries.as_mut()?.get_mut(member));
                match country {
                    Some(country) => country.country_side_index = side_index,
                    None => {
                        unresolved = Some(member);
                        break;
                    }
                }
            }
            self.families
                .get_mut(&RulesTypeFamily::Side)
                .expect("registered Side exists")[index]
                .side_country_members = members;
            if let Some(member) = unresolved {
                // `<random>` remains-2 in the reader.672597 would index
                // Country[-2]; reject this undefined pointer route after
                // retaining the actual Side mutation for failure recovery.
                return Err(RulesError::InvalidValue {
                    section: "Sides".to_string(),
                    key: key.to_string(),
                    expected: "a nonnegative resolved native Country index".to_string(),
                    value: member.to_string(),
                });
            }
        }
        Ok(())
    }

    /// ReadSide4756F0: ReadString128 with an empty default, then reuse or
    /// construct a Side. Count0 retains the caller's current index. This
    /// reader does not read memberships; only Rules ReadSides672440 does.
    /// Executed controls: campaign_start.py --houses, side_reader_rows in
    /// campaign_start_houses.json side_controls (+meta).
    pub(super) fn read_side_index(&mut self, section: &IniSection, key: &str, current: i32) -> i32 {
        let name = section.read_string(key, "", 0x80);
        if name.is_empty() {
            return current;
        }
        // `None` only past VERA's Side bound (`find_or_allocate`).
        let Some(index) = self.find_or_allocate(RulesTypeFamily::Side, &name) else {
            return current;
        };
        i32::try_from(index).expect("native Side index exceeds i32")
    }

    pub(super) fn find_or_allocate(
        &mut self,
        family: RulesTypeFamily,
        incoming: &str,
    ) -> Option<usize> {
        // Caller-specific ReadString buffers trim the whole scalar before this
        // boundary. List tokens deliberately arrive untrimmed. The factory
        // must therefore inspect the exact incoming string, not trim again.
        if incoming.is_empty()
            || (family != RulesTypeFamily::Side && is_native_none_type_name(incoming))
        {
            return None;
        }
        self.find_or_construct_within_bound(family, incoming)
    }

    /// [`Self::find_or_construct`], except that a new Side or Country past
    /// [`SIDE_LIMIT`] or [`COUNTRY_LIMIT`] constructs nothing and answers
    /// `None`, so outside data never grows the retained registry past what
    /// every later scenario can project.
    pub(super) fn find_or_construct_within_bound(
        &mut self,
        family: RulesTypeFamily,
        incoming: &str,
    ) -> Option<usize> {
        let limit = match family {
            RulesTypeFamily::Side => Some(SIDE_LIMIT),
            RulesTypeFamily::Country => Some(COUNTRY_LIMIT),
            _ => None,
        };
        if let Some(limit) = limit
            && self.families.get(&family).map_or(0, Vec::len) >= limit
            && self.find_existing(family, incoming).is_none()
        {
            log::warn!("{family:?} {incoming} not allocated: VERA20k supports at most {limit}");
            return None;
        }
        Some(self.find_or_construct(family, incoming))
    }

    /// ParticleTypeClass::FindOrCreate @ 0x00645430 answers -1 only for
    /// `<none>` (`_strcmpi` against 0x00817474); an empty name or `none`
    /// finds or constructs a ParticleType like any other spelling.
    pub(super) fn find_or_allocate_particle(&mut self, incoming: &str) -> Option<usize> {
        (!incoming.eq_ignore_ascii_case("<none>"))
            .then(|| self.find_or_construct(RulesTypeFamily::Particle, incoming))
    }

    /// ParticleSystemTypeClass::FindOrCreate @ 0x00644630 answers -1 for
    /// `<none>` (`_strcmpi` against 0x00817474) and for an empty name; `none`,
    /// like every other spelling, finds or constructs a ParticleSystemType.
    pub(super) fn find_or_create_particle_system(&mut self, incoming: &str) -> Option<usize> {
        (!incoming.is_empty() && !incoming.eq_ignore_ascii_case("<none>"))
            .then(|| self.find_or_construct(RulesTypeFamily::ParticleSystem, incoming))
    }

    /// Every Process and late factory uses the same lookup/constructor body.
    pub(super) fn find_or_construct(&mut self, family: RulesTypeFamily, incoming: &str) -> usize {
        find_or_construct_type(
            &mut self.families,
            &mut self.techno_type_order,
            &mut self.native_type_construction_events,
            family,
            incoming,
        )
    }

    pub(super) fn allocate_colors(&mut self, pass: &IniFile) {
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

    /// Native factories still compare literal stored IDs. Only their resolved
    /// pointer projection uses a distinct key for colliding AnimType slots.
    pub(super) fn resolved_type_key(&self, family: RulesTypeFamily, index: usize) -> String {
        let members = &self.families[&family];
        if family == RulesTypeFamily::Animation {
            anim_type_runtime_key(
                members
                    .iter()
                    .map(|member| member.native_stored_id.as_str()),
                index,
            )
            .expect("factory returned a live AnimType index")
            .into_owned()
        } else {
            members[index].native_stored_id.clone()
        }
    }

    /// None retains the prior pointer; Some(None) is an authored null.
    /// ReadString's empty default and FindOrAllocate are shared by scalar
    /// allocation sites and the fields which retain their resolved identities.
    pub(super) fn resolve_scalar_from(
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
                .map(|index| self.resolved_type_key(family, index)),
        )
    }

    pub(super) fn allocate_list_from(
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
    /// Ares and Phobos pointer vectors look up existing types at this exact
    /// read point. ReadString/strtok retain the native 2048-byte, comma-only
    /// list policy.
    pub(super) fn resolve_existing_list_from(
        &self,
        section: &IniSection,
        key: &str,
        family: RulesTypeFamily,
        capacity: usize,
    ) -> Option<Vec<String>> {
        self.resolve_existing_list_indices_from(section, key, family, capacity)
            .map(|indices| {
                indices
                    .into_iter()
                    .map(|index| self.resolved_type_key(family, index))
                    .collect()
            })
    }

    /// Phobos `SW.AuxTechnos`/`SW.NegTechnos`:
    /// `ValueableVector<TechnoTypeClass*>` resolves each token across every
    /// techno family (`TechnoTypeClass::Find`), skipping unknown ones; a
    /// reached list replaces and an absent key retains.
    #[cfg(feature = "phobos")]
    pub(super) fn resolve_existing_techno_list_from(
        &self,
        section: &IniSection,
        key: &str,
        capacity: usize,
    ) -> Option<Vec<String>> {
        let tokens = section.read_list(key, capacity)?;
        let mut resolved = Vec::new();
        for token in tokens {
            for family in [
                RulesTypeFamily::Infantry,
                RulesTypeFamily::Vehicle,
                RulesTypeFamily::Aircraft,
                RulesTypeFamily::Building,
            ] {
                let Some(members) = self.families.get(&family) else {
                    continue;
                };
                let index = ares_type_index_of_name(
                    token,
                    members
                        .iter()
                        .map(|member| member.native_stored_id.as_str()),
                );
                if let Some(index) = index {
                    resolved.push(members[index].native_stored_id.clone());
                    break;
                }
            }
        }
        Some(resolved)
    }

    /// Installed Ares Find10007EA0 resolves a nullable TechnoType pointer.
    /// The common construction array decides equal names across categories.
    pub(super) fn find_existing_techno_type(
        &self,
        name: &str,
    ) -> Option<crate::rules::superweapon_type::DeliveryType> {
        let index = ares_type_index_of_name(
            name,
            self.techno_type_order
                .iter()
                .map(|&(family, index)| self.families[&family][index].native_stored_id.as_str()),
        )?;
        let (family, member) = self.techno_type_order[index];
        Some(crate::rules::superweapon_type::DeliveryType {
            category: family.object_category().expect("TechnoType array category"),
            id: self.families[&family][member].native_stored_id.clone(),
        })
    }

    /// Ares pointer reader1003C650: ReadString2048, Find10007EA0, then
    /// none10003950; absent/empty/unknown retains, an explicit none clears.
    pub(super) fn resolve_existing_techno_type_from(
        &self,
        section: &IniSection,
        key: &str,
    ) -> Option<Option<crate::rules::superweapon_type::DeliveryType>> {
        let _ares = crate::rules::ares_switch::ares_reads();
        let name = section.read_name(key, 2048)?;
        if is_native_none_type_name(name) {
            return Some(None);
        }
        self.find_existing_techno_type(name).map(Some)
    }

    /// Ares ValueableVector<TechnoTypeClass*> 1000EDD0: each token goes
    /// through the same Find10007EA0; unknown tokens are skipped and a reached
    /// read replaces the list.
    pub(super) fn resolve_existing_techno_type_list_from(
        &self,
        section: &IniSection,
        key: &str,
    ) -> Option<Vec<crate::rules::superweapon_type::TechnoTypeReference>> {
        let _ares = crate::rules::ares_switch::ares_reads();
        Some(
            section
                .read_list(key, 2048)?
                .into_iter()
                .filter_map(|token| self.find_existing_techno_type(token))
                .collect(),
        )
    }

    /// Installed Ares typed pointers: Anim Parser1000C9C0/Find1000D3E0,
    /// Unit Parser1002D970/Find1002DCF0 and the Survivor.SideN Infantry
    /// reads 1003E040..1003E070 look up registered types only.
    /// A successful none clears; absent/unknown retains.
    pub(super) fn resolve_existing_type_from(
        &self,
        section: &IniSection,
        key: &str,
        family: RulesTypeFamily,
    ) -> Option<Option<String>> {
        let _ares = crate::rules::ares_switch::ares_reads();
        let name = section.read_name(key, 2048)?;
        if is_native_none_type_name(name) {
            return Some(None);
        }
        let members = self.families.get(&family)?;
        let index = ares_type_index_of_name(
            name,
            members
                .iter()
                .map(|member| member.native_stored_id.as_str()),
        )?;
        Some(Some(self.resolved_type_key(family, index)))
    }

    /// Shared Ares/Phobos pointer-vector reader: Building specialization
    /// 10010850 uses Find10007E50 after none rejection10003950. Resolve once at
    /// the current pass; callers project the retained ordinal as their existing
    /// field needs.
    pub(super) fn resolve_existing_list_indices_from(
        &self,
        section: &IniSection,
        key: &str,
        family: RulesTypeFamily,
        capacity: usize,
    ) -> Option<Vec<usize>> {
        Some(
            section
                .read_list(key, capacity)?
                .into_iter()
                .filter_map(|token| {
                    let ids = self
                        .families
                        .get(&family)
                        .into_iter()
                        .flatten()
                        .map(|member| member.native_stored_id.as_str());
                    if family == RulesTypeFamily::SuperWeapon {
                        super_weapon_index_of_name(token, ids)
                    } else {
                        ares_type_index_of_name(token, ids)
                    }
                })
                .collect(),
        )
    }

    pub(super) fn resolve_list_from(
        &mut self,
        section: &IniSection,
        key: &str,
        family: RulesTypeFamily,
        capacity: usize,
    ) -> Option<Vec<String>> {
        let mut resolved = Vec::new();
        for token in section.read_list(key, capacity)? {
            if let Some(index) = self.find_or_allocate(family, token) {
                resolved.push(self.resolved_type_key(family, index));
            }
        }
        Some(resolved)
    }
}

impl RulesPassProcessor {
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
    pub(super) fn building_index(&self, name: &str) -> Option<i32> {
        self.families
            .get(&RulesTypeFamily::Building)?
            .iter()
            .position(|member| member.native_stored_id.eq_ignore_ascii_case(name))
            .and_then(|index| i32::try_from(index).ok())
    }

    pub(super) fn family_len(&self, family: RulesTypeFamily) -> usize {
        self.families.get(&family).map_or(0, Vec::len)
    }

    /// The member's section in `pass`, after its Armor read and its body
    /// takes the pass; `None` when the member or its section is missing.
    pub(super) fn begin_rules_member_read<'p>(
        &mut self,
        family: RulesTypeFamily,
        index: usize,
        pass: &'p IniFile,
    ) -> Option<&'p IniSection> {
        let member = self.families.get(&family)?.get(index)?;
        let raw = pass.section(&member.native_stored_id)?;
        let current_armor = member.armor_id;
        let armor = if family.object_category().is_some() || family == RulesTypeFamily::Terrain {
            // Rules read the Ares way bind through Ares' armor catalog.
            match self.ares_armors.as_ref() {
                Some(armors) => armors.read_binding(raw, current_armor),
                None => raw
                    .read_name("Armor", 0x80)
                    .map_or(current_armor, armor_index),
            }
        } else {
            current_armor
        };
        let member = self.families.get_mut(&family)?.get_mut(index)?;
        member.armor_id = armor;
        member.body.overlay_rules_pass(raw);
        Some(raw)
    }

    fn process_type_data(
        &mut self,
        pass: &IniFile,
        fixed_art: &IniFile,
        _fixed_eva: &EvaRegistry,
        primary_rules: bool,
        fixed_sounds: &SoundRegistry,
    ) {
        // Ares ReadTypeData679A10 hook1002EFC0 sweeps every Side before
        // Country bodies. It does not require [Sides] or a Side section.
        // Country-only prepasses do not enter this hook. The live loop sees
        // only Sides allocated now, not any allocated by later Country reads.
        if crate::rules::ares_switch::ares_way() {
            let side_types = SideRegisteredTypes::collect(&self.families);
            let side_count = self
                .families
                .get(&RulesTypeFamily::Side)
                .map_or(0, Vec::len);
            for array_index in 0..side_count {
                // Move the extension out so the shared ColorScheme lookup can
                // read the retained registry; it goes back to the same Side.
                let member = &mut self.family_mut(RulesTypeFamily::Side)[array_index];
                let id = member.native_stored_id.clone();
                let mut side = member
                    .ares_side
                    .take()
                    .expect("allocated Side owns its Ares extension");
                side.read_pass(
                    &id,
                    i32::try_from(array_index)
                        .expect("SIDE_LIMIT keeps the Side registry within u8"),
                    pass,
                    _fixed_eva,
                    &side_types,
                    |name| self.loading_color_index(name),
                );
                self.family_mut(RulesTypeFamily::Side)[array_index].ares_side = Some(side);
            }
        }
        // Installed BeforeTypeData679A15 reads ArmorTypes/defaults before
        // any ordinary ObjectType/Warhead body in this Process pass. Only
        // Rules read the Ares way have the catalog.
        if let Some(armors) = self.ares_armors.as_mut() {
            armors.read_pass(pass);
        }
        // 1002AEF0 reads [MouseCursors] after ArmorTypes, before any Type
        // body of this pass names a cursor.
        if crate::rules::ares_switch::ares_way() {
            self.ares_mouse_cursors.read_pass(pass);
        }
        // Ares RulesClass_BeforeTypeData679A15 -> 1002AEF0. This is after
        // registry allocation but before any TechnoType body in this pass.
        // Move out and back while reading the existing registries; this is
        // still the one retained catalog, not a merged-INI reconstruction.
        // Only Rules read the Ares way have it.
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
        // Phobos RulesData_LoadBeforeTypeData (0x679A15), before this pass's
        // type bodies.
        #[cfg(feature = "phobos")]
        if let Some(section) = pass.section("AudioVisual") {
            let _phobos = crate::rules::ares_switch::not_ares_reads();
            // `PhobosFixedString<32>`: an empty read keeps the earlier pass's
            // name, and `none` or `<none>` clears it.
            if let Some(name) = section.read_name("MissingCameo", 0x20) {
                self.rules_missing_cameo =
                    if crate::rules::ini_parser::is_native_none_type_name(name) {
                        String::new()
                    } else {
                        name.to_owned()
                    };
            }
            // `ValueableIdx<ColorScheme>` resolves the name against the
            // [Colors] this pass has read (0x668BFE precedes 0x668EF0), so a
            // later pass naming no entry keeps the index.
            self.rules_timer_blink_color_scheme = section.read_ares_color_scheme_index(
                "TimerBlinkColorScheme",
                self.rules_timer_blink_color_scheme,
                |name| self.loading_color_index(name),
            );
        }
        self.process_house_family(pass, primary_rules);
        self.process_super_weapon_family(pass, _fixed_eva, fixed_sounds);
        self.process_anim_family(fixed_art, fixed_sounds);
        self.process_techno_family(RulesTypeFamily::Building, pass, fixed_art, fixed_sounds);
        self.process_techno_family(RulesTypeFamily::Aircraft, pass, fixed_art, fixed_sounds);
        self.process_techno_family(RulesTypeFamily::Vehicle, pass, fixed_art, fixed_sounds);
        self.process_techno_family(RulesTypeFamily::Infantry, pass, fixed_art, fixed_sounds);
        self.process_weapon_family(pass);
        self.process_bullet_family(pass, fixed_art, fixed_sounds);
        self.process_warhead_family(pass);
        // 679B8C follows all three readers, even for absent Type sections.
        // AudioVisual6691E0 runs later in Process, so this uses prior Gravity.
        self.postprocess_weapon_speeds();
        // Building post adds no Type references.
        self.process_plain_family(RulesTypeFamily::Terrain, pass, fixed_art, fixed_sounds);
        self.process_plain_family(RulesTypeFamily::Smudge, pass, fixed_art, fixed_sounds);
        self.process_plain_family(RulesTypeFamily::Overlay, pass, fixed_art, fixed_sounds);
        self.process_particle_family(pass, fixed_sounds);
        self.process_particle_system_family(pass, fixed_sounds);
        self.process_voxel_anim_family(pass, fixed_art, fixed_sounds);
        // MissionControl adds no Type references.
    }
}

impl RulesPassProcessor {
    fn compact_model_plan(&mut self) {
        let mut families: Vec<_> = self.families.iter_mut().collect();
        families.sort_unstable_by_key(|(family, _)| **family as u8);
        self.model_plan
            .compact(families.into_iter().flat_map(|(_, members)| {
                members
                    .iter_mut()
                    .map(|member| &mut member.object_image.model_slots)
            }));
    }

    pub(super) fn finish(mut self) -> (IniFile, NativeTypeConstructionTrace) {
        self.compact_model_plan();
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
                    rules_threat_coefficient_defaults: self.rules_threat_coefficient_defaults,
                    compatibility: self.compatibility,
                    reads_ares_way: self.reads_ares_way,
                    ares_prerequisites: self.ares_prerequisites,
                    ares_armors: self.ares_armors,
                    phobos_radiation_types: self.phobos_radiation_types,
                    phobos_shield_types: self.phobos_shield_types,
                    phobos_insignia_types: self.phobos_insignia_types,
                    phobos_digital_display_types: self.phobos_digital_display_types,
                    phobos_attach_effect_types: self.phobos_attach_effect_types,
                    ares_global_controls: self.ares_global_controls,
                    model_plan: self.model_plan,
                    ares_mouse_cursors: self.ares_mouse_cursors,
                    families: self.families,
                    techno_type_order: self.techno_type_order,
                    tiberiums: self.tiberiums,
                    colors: self.colors,
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
                    rules_allow_shrouded_subterannean_moves: self
                        .rules_allow_shrouded_subterannean_moves,
                    rules_spotlight: self.rules_spotlight,
                    rules_ammo_crate_damage: self.rules_ammo_crate_damage,
                    rules_tiberium_explosion_damage: self.rules_tiberium_explosion_damage,
                    rules_ion_cannon_damage: self.rules_ion_cannon_damage,
                    rules_ion_blast_anim: self.rules_ion_blast_anim,
                    rules_ion_beam_anim: self.rules_ion_beam_anim,
                    rules_ion_cannon_warhead: self.rules_ion_cannon_warhead,
                    #[cfg(feature = "phobos")]
                    rules_timer_blink_color_scheme: self.rules_timer_blink_color_scheme,
                    #[cfg(feature = "phobos")]
                    rules_missing_cameo: self.rules_missing_cameo,
                    rules_mind_control_attack_line_frames: self
                        .rules_mind_control_attack_line_frames,
                    rules_drain_money_frame_delay: self.rules_drain_money_frame_delay,
                    rules_drain_money_amount: self.rules_drain_money_amount,
                    rules_line_trail_override: self.rules_line_trail_override,
                    rules_chrono_beam_color: self.rules_chrono_beam_color,
                    rules_prism_support: self.rules_prism_support,
                    rules_patrol_scan_minutes: self.rules_patrol_scan_minutes,
                    rules_ai_hate_delays: self.rules_ai_hate_delays,
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
                    rules_powerups: self.powerups,
                    rules_missile_spawn: self.missile_spawn,
                    rules_general_type_lists: self.general_type_lists,
                },
            },
        )
    }
}
