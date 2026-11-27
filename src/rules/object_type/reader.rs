//! `ObjectType::from_ini_section`: the per-section rules.ini reader that builds
//! one object type from its constructor defaults and the keys gamemd's
//! Object/Techno/category ReadINI functions read, with its small numeric and
//! building-only key helpers.

use glam::IVec3;

use crate::rules::ini_parser::IniSection;
use crate::rules::ini_parser::ini_value::TechnoTypeCategory;
use crate::rules::jumpjet_params::JumpjetParams;
use crate::rules::locomotor_type::{MovementZone, SpeedType};
use crate::rules::terrain_rules::LandType;
use crate::util::fixed_math::SimFixed;
use crate::util::native_x87::NativeF64Bits;

#[cfg(feature = "phobos")]
use super::PhobosOreGathering;
use super::{AbilityFlags, BarrelVoxelOffsets, BuildCategory, BuildingHiddenOccupancyProfile};
use super::{DEBRIS_LIMIT, DEFAULT_LEPTON_MIND_CONTROL_OFFSET, DEFAULT_TECH_LEVEL, HouseMasks};
use super::{InfantryPips, ObjectCategory, ObjectType};
use super::{
    PipScale, SLAVES_NUMBER_LIMIT, SPAWNS_NUMBER_LIMIT, TargetZoneScanType, VhpScan,
    WEAPON_SLOT_COUNT,
};

/// The pi/180 double multiplied by `TechnoTypeClass::ReadINI @ 0x00712170`
/// when storing RollAngle at `+0x3A0` (`FMUL [0x007F4FB8] @ 0x007123BC`).
const TECHNOTYPE_DEGREES_TO_RADIANS: f64 = f64::from_bits(0x3f91_df46_a252_9d39);

fn native_minutes_to_ticks(value: f64) -> u32 {
    //ReadDouble widens its scanned f32; multiplication by900 fits binary64
    //precision. Reuse the existing ftol owner for signed64-to-low32 and
    //invalid semantics, exactly as MissionControl rates do. Native executed
    //Door controls include -39, low-dword overflow and invalid input.
    use crate::util::native_x87::{MaskedX87Chop53, NativeF64Bits};
    MaskedX87Chop53::ftol_i32_low_masked(MaskedX87Chop53::load_f64(NativeF64Bits::from_bits(
        (value * 900.0).to_bits(),
    ))) as u32
}

impl ObjectType {
    /// Parse an ObjectType from a rules.ini section.
    ///
    /// Missing keys get sensible defaults matching RA2's behavior.
    /// The `id` is the section name, and `category` comes from which
    /// type registry listed this object. Fields that only the ordered Process
    /// owner (`native_processing`) reads keep their constructor values here;
    /// `RuleSet::from_processed_rules` publishes that owner's retained result.
    pub fn from_ini_section(id: &str, section: &IniSection, category: ObjectCategory) -> Self {
        let armor = section.read_string("Armor", "none", 0x80);
        // BuildingType45FE50: Hospital, Armory460B03, then the three
        // barracks readers460B1D/37/51. Each starts at constructor0 and folds
        // reached layer values with the live byte as the next pass's default.
        let (hospital, armory, gdi_barracks, nod_barracks, yuri_barracks) =
            if category == ObjectCategory::Building {
                (
                    section.read_bool("Hospital", false),
                    section.read_bool("Armory", false),
                    section.read_bool("GDIBarracks", false),
                    section.read_bool("NODBarracks", false),
                    section.read_bool("YuriBarracks", false),
                )
            } else {
                (false, false, false, false, false)
            };
        // `GuardRange=` and `AirRangeBonus=` are ReadRange whole leptons
        // (`0x007122B3`, `0x007147A9`), kept here as cells; `None` marks an
        // absent key.
        let present_range = |key: &str| {
            section
                .is_present(key)
                .then(|| SimFixed::from_bits(section.read_range(key, 0) << 8))
        };
        // The house lists (`0x004750D0`) read `char[128]`; Ares 3.0
        // INIClass_ReadHouseTypesList (1005BED0 hook 0x4750EC, strtok
        // 1005BEF0 hook 0x475107) swaps in its 0x800 buffer. The two
        // prerequisite lists name BuildingTypes, so `RuleSet` parses them.
        let ares_way = crate::rules::ares_switch::ares_way();
        let house_list_capacity = if ares_way { 0x800 } else { 0x80 };
        let owner: Vec<String> = section
            .read_list("Owner", house_list_capacity)
            .map(|tokens| tokens.into_iter().map(str::to_owned).collect())
            .unwrap_or_default();

        let required_tokens = section.read_list("RequiredHouses", house_list_capacity);
        let required_houses_explicitly_empty = required_tokens.as_ref().is_some_and(Vec::is_empty);
        let required_houses: Vec<String> = required_tokens
            .map(|tokens| tokens.into_iter().map(str::to_owned).collect())
            .unwrap_or_default();

        let forbidden_houses: Vec<String> = section
            .read_list("ForbiddenHouses", house_list_capacity)
            .map(|tokens| tokens.into_iter().map(str::to_owned).collect())
            .unwrap_or_default();

        // `TechnoTypeClass::ReadINI` reads `VeteranAbilities=` into `+0x29C`
        // (`0x007154A3`) and `EliteAbilities=` into `+0x2AE` (`0x007154E8`).
        // VERA's merged section already carries the last INI pass's value
        // for a present key, and a key no pass authored is the constructor's
        // all-clear array.
        let veteran_abilities = AbilityFlags::parse(
            section.read_list("VeteranAbilities", 0x80),
            AbilityFlags::default(),
        );
        let elite_abilities = AbilityFlags::parse(
            section.read_list("EliteAbilities", 0x80),
            AbilityFlags::default(),
        );
        // Phobos `ReadAdditionalAbilities` parses the same two keys for its
        // five extension tokens, independently of the stock array above.
        #[cfg(feature = "phobos")]
        let mut phobos_abilities = crate::rules::phobos_crit::PhobosAbilityFlags::default();
        #[cfg(feature = "phobos")]
        phobos_abilities.read_pass(section);

        // `TechnoTypeClass::ReadINI @ 0x00712587..0x007125DF` reads the debris
        // span in a fixed order and then clamps it twice:
        //   `0x0071258E PUSH 0x84439C` ("MaxDebris") -> `0x0071259B` +0x5BC
        //   `0x007125A8 PUSH 0x844390` ("MinDebris") -> `0x007125B7` +0x5C0
        //   `0x007125BD JGE`  -> `0x007125BF` MinDebris = 0 when it read below 0
        //   `0x007125D7 JGE`  -> `0x007125D9` MaxDebris = MinDebris when it is lower
        // The order is load-bearing and matches the warhead sibling at
        // `WarheadTypeClass::ReadINI 0x0075DAC4`/`0x0075DAE0`: MinDebris is
        // floored first, so a negative MinDebris cannot drag MaxDebris below
        // zero and re-open the block that `MaxDebris <= 0` closes. No stock
        // section authors a negative MinDebris or an inverted span, so this is
        // reachable only from modded rules — but it is what makes
        // `throw_death_debris`'s inverted-span branch well-defined.
        //
        // Both keys are read case-exactly. `0x0084439C` is the C string
        // `MaxDebris` and `0x00844390` is `MinDebris`, and `CCINIClass::ReadInt
        // @ 0x005276D0` CRCs the raw key bytes (`0x00527715..0x00527750`) with
        // no folding step, so the 17 `[VehicleTypes]` that spell it
        // `Maxdebris=3` are invisible to gamemd and keep the constructor
        // default of 0 (`TechnoTypeClass::Constructor 0x00710FB7`).
        let max_debris = section.read_int("MaxDebris", 0).min(DEBRIS_LIMIT);
        let min_debris = section.read_int("MinDebris", 0).clamp(0, DEBRIS_LIMIT);
        let max_debris = max_debris.max(min_debris);

        Self {
            id: id.to_string(),
            alpha_image: section.read_object_alpha_image(""),
            category,
            techno_category: section.read_techno_type_category(TechnoTypeCategory::Unknown),
            // The process-resident Building reader publishes this below the
            // projection; merged text must not perform a second coordinate read.
            target_coord_offset: [0; 3],
            half_damage_smoke_location1: [0; 3],
            half_damage_smoke_location2: [0; 3],
            name: section.read_name("Name", 0x31).map(str::to_owned),
            ui_name: section.read_name("UIName", 0x20).map(str::to_owned),
            cost: section.read_int("Cost", 0),
            soylent: section.read_int("Soylent", 0),
            points: section.read_int("Points", 0),
            factory_plant: section.read_bool("FactoryPlant", false),
            cost_bonuses: [
                "InfantryCostBonus",
                "UnitsCostBonus",
                "AircraftCostBonus",
                "BuildingsCostBonus",
                "DefensesCostBonus",
            ]
            .map(|key| {
                section.read_double_to_float(key, crate::util::native_x87::NativeF32Bits::ONE)
            }),
            // Constructor empty; only the ordered Process owner reads the
            // Type references (`TechnoTypeReferences`).
            explosion_anims: Vec::new(),
            destroy_anims: Vec::new(),
            trainable: section.read_bool("Trainable", category != ObjectCategory::Building),
            burst_delays: std::array::from_fn(|index| {
                if category == ObjectCategory::Vehicle {
                    section.read_int(&format!("BurstDelay{index}"), -1)
                } else {
                    -1
                }
            }),
            // Constructor empty; only the ordered Process owner reads it.
            dead_bodies: Vec::new(),
            not_human: category == ObjectCategory::Infantry && section.read_bool("NotHuman", false),
            strength: section.read_int("Strength", 0),
            dont_score: section.read_bool("DontScore", false),
            nominal: section.read_bool("Nominal", false),
            special_threat_value: section.read_double("SpecialThreatValue", 0.0),
            threat_posed: section.read_int("ThreatPosed", 0),
            leadership_rating: section.read_int("LeadershipRating", 5),
            threat_coefficients: Default::default(),
            threat_avoidance_coefficient: section.read_double_bits(
                "ThreatAvoidanceCoefficient",
                crate::util::native_x87::NativeF64Bits::POSITIVE_ZERO,
            ),
            armor_id: crate::rules::armor::armor_index(&armor),
            armor,
            #[cfg(feature = "phobos")]
            shield_type: section
                .read_name("ShieldType", 0x80)
                .filter(|name| {
                    !name.is_empty()
                        && !name.eq_ignore_ascii_case("none")
                        && !name.eq_ignore_ascii_case("<none>")
                })
                .map(str::to_owned),
            #[cfg(feature = "phobos")]
            digital_display_types: section
                .read_list("DigitalDisplayTypes", 0x100)
                .map(|tokens| {
                    tokens
                        .into_iter()
                        .map(|token| token.trim().to_owned())
                        .collect()
                })
                .unwrap_or_default(),
            #[cfg(feature = "phobos")]
            attach_effect_attach_types: section
                .read_list("AttachEffect.AttachTypes", 0x800)
                .map(|tokens| {
                    tokens
                        .into_iter()
                        .map(|token| token.trim().to_owned())
                        .collect()
                })
                .unwrap_or_default(),
            #[cfg(feature = "phobos")]
            attach_effect_duration_overrides: section
                .read_int_list("AttachEffect.DurationOverrides")
                .unwrap_or_default(),
            #[cfg(feature = "phobos")]
            attach_effect_delays: section
                .read_int_list("AttachEffect.Delays")
                .unwrap_or_default(),
            #[cfg(feature = "phobos")]
            attach_effect_initial_delays: section
                .read_int_list("AttachEffect.InitialDelays")
                .unwrap_or_default(),
            #[cfg(feature = "phobos")]
            attach_effect_recreation_delays: section
                .read_int_list("AttachEffect.RecreationDelays")
                .unwrap_or_default(),
            #[cfg(feature = "phobos")]
            digital_display_disable: section.read_bool("DigitalDisplay.Disable", false),
            #[cfg(feature = "phobos")]
            digital_display_health_fake_at_disguise: section
                .read_phobos_nullable_bool("DigitalDisplay.Health.FakeAtDisguise", None),
            #[cfg(feature = "phobos")]
            open_topped_use_transport_range_modifiers: section
                .read_phobos_nullable_bool("OpenTopped.UseTransportRangeModifiers", None),
            #[cfg(feature = "phobos")]
            placement_preview: category == ObjectCategory::Building
                && section.read_bool("PlacementPreview", true),
            #[cfg(feature = "phobos")]
            placement_preview_shape: if category == ObjectCategory::Building {
                section
                    .read_name("PlacementPreview.Shape", 0x100)
                    .map(str::to_owned)
            } else {
                None
            },
            #[cfg(feature = "phobos")]
            placement_preview_shape_frame: if category == ObjectCategory::Building {
                section.read_phobos_nullable_int("PlacementPreview.ShapeFrame", None)
            } else {
                None
            },
            #[cfg(feature = "phobos")]
            placement_preview_offset: if category == ObjectCategory::Building {
                section.read_coord3("PlacementPreview.Offset", [0, -15, 1])
            } else {
                [0, -15, 1]
            },
            #[cfg(feature = "phobos")]
            placement_preview_remap: category != ObjectCategory::Building
                || section.read_bool("PlacementPreview.Remap", true),
            #[cfg(feature = "phobos")]
            placement_preview_palette: if category == ObjectCategory::Building {
                section
                    .read_name("PlacementPreview.Palette", 0x100)
                    .map(str::to_owned)
            } else {
                None
            },
            #[cfg(feature = "phobos")]
            placement_preview_translucency: if category == ObjectCategory::Building {
                section
                    .read_phobos_nullable_int("PlacementPreview.Translucency", None)
                    .map(crate::rules::phobos_placement_preview::Translucency::from_int)
            } else {
                None
            },
            speed: section.read_techno_speed("Speed", 0),
            // TechnoTypeClass ctor/read contract: raw signed ints, with no
            // clamp or conversion. A zero WalkRate is invalid content natively
            // (the live consumer executes IDIV without a zero guard).
            walk_rate: section.read_int("WalkRate", 1),
            idle_rate: section.read_int("IdleRate", 0),
            // Double fields (`0x007124A3..0x007124E5`). Saturating: an
            // out-of-range literal scans as infinity.
            weight: SimFixed::saturating_from_num(section.read_double("Weight", 1.0) as f32),
            accel_factor: section.read_double_bits(
                "AccelerationFactor",
                NativeF64Bits::from_bits(0x3f9e_b851_eb85_1eb8),
            ),
            decel_factor: section.read_double_bits(
                "DeaccelerationFactor",
                NativeF64Bits::from_bits(0x3f60_624d_d2f1_a9fc),
            ),
            accelerates: section.read_bool("Accelerates", true),
            passive: category == ObjectCategory::Vehicle && section.read_bool("Passive", false),
            is_train: section.read_bool("IsTrain", false),
            slowdown_distance: section.read_int("SlowdownDistance", 500),
            flight_level: section.read_int("FlightLevel", -1),
            is_dropship: section.read_bool("IsDropship", false),
            // Phobos holds this in InfantryTypeExt, so only an
            // `[InfantryTypes]` section carries it.
            #[cfg(feature = "phobos")]
            prone_speed: if category == ObjectCategory::Infantry {
                section.read_phobos_nullable_double("ProneSpeed", None)
            } else {
                None
            },
            // Phobos keeps this in AircraftTypeExt, so a non-aircraft section
            // carries no value even when it repeats the key.
            #[cfg(feature = "phobos")]
            fly_no_wobbles: if category == ObjectCategory::Aircraft {
                section.read_phobos_nullable_bool("FlyNoWobbles", None)
            } else {
                None
            },
            #[cfg(feature = "phobos")]
            laser_target_color: section.read_phobos_nullable_int("LaserTargetColor", None),
            #[cfg(feature = "phobos")]
            airstrike_line_color: section.read_phobos_nullable_color("AirstrikeLineColor", None),
            voxel_shadow_index: 0,
            pitch_speed: section.read_double_bits(
                "PitchSpeed",
                NativeF64Bits::from_bits(0x3fd0_0000_0000_0000),
            ),
            // Constructor +0x3A0 is 30 degrees in radians. Native ReadINI
            // passes -1.0 to ReadDouble then preserves the current field for
            // that sentinel on every RulesClass::Process pass.
            roll_angle: NativeF64Bits::from_bits(
                section
                    .read_double_with(
                        "RollAngle",
                        f64::from_bits(0x3fe0_c152_382d_7365),
                        |current, degrees| {
                            if degrees == -1.0 {
                                current
                            } else {
                                degrees * TECHNOTYPE_DEGREES_TO_RADIANS
                            }
                        },
                    )
                    .to_bits(),
            ),
            // `ReadDouble(-1)` in each TechnoTypeClass::ReadINI pass
            // (`0x00712170`): -1 keeps the current radians, otherwise convert
            // the degrees before the existing SimFixed storage boundary.
            pitch_angle: section.read_double_with(
                "PitchAngle",
                SimFixed::from_num(20.0_f32 * (std::f32::consts::PI / 180.0)),
                |current, degrees| {
                    if degrees == -1.0 {
                        current
                    } else {
                        SimFixed::saturating_from_num(
                            (degrees as f32) * (std::f32::consts::PI / 180.0),
                        )
                    }
                },
            ),
            aux_sound1: section
                .read_name("AuxSound1", 0x80)
                .map(str::to_owned)
                .filter(|s| !s.is_empty()),
            aux_sound2: section
                .read_name("AuxSound2", 0x80)
                .map(str::to_owned)
                .filter(|s| !s.is_empty()),
            sight: section.read_int("Sight", 0),
            // The ordered Rules processor owns the layered read; RuleSet
            // publishes its retained value.
            tech_level: DEFAULT_TECH_LEVEL,
            build_time_multiplier: section.read_double_to_float(
                "BuildTimeMultiplier",
                crate::util::native_x87::NativeF32Bits::ONE,
            ),
            owner,
            // ReadBool over the constructor's 0 at +0xC99 (`0x00712299`).
            double_owned: section.read_bool("DoubleOwned", false),
            required_houses,
            required_houses_explicitly_empty,
            house_masks: HouseMasks::default(),
            forbidden_houses,
            // gamemd-derived: `TechnoTypeClass__Constructor @ 0x00710FF0`
            // seeds +0x6D0 to -1; `TechnoTypeClass::ReadINI` around 0x007149FB
            // applies the signed `AIBasePlanningSide=` override.
            ai_base_planning_side: section.read_int("AIBasePlanningSide", -1),
            // gamemd-derived: `BuildingTypeClass__Constructor` clears
            // `AIBuildThis` at 0x0045E21F; `BuildingTypeClass__ReadINI`
            // 0x00460FE2..0x00460FF6 binds `AIBuildThis=`.
            ai_build_this: section.read_bool("AIBuildThis", false),
            allowed_to_start_in_multiplayer: section.read_bool("AllowedToStartInMultiplayer", true),
            prerequisite: Vec::new(),
            prerequisite_override: Vec::new(),
            // ReadInt over the constructor's 0x7FFFFFFF (`0x00713152`).
            build_limit: section.read_int("BuildLimit", i32::MAX),
            aibuild_limit: crate::rules::npatch_switch::npatch_read(|| {
                section.read_int("AIBuildLimit", i32::MAX)
            }),
            forbid_parallel_ai_queues: crate::rules::phobos_switch::phobos_read(|| {
                section.read_bool("ForbidParallelAIQueues", false)
            }),
            // TechnoTypeExt::ExtData::LoadFromINIFile (src/Ext/TechnoType/Body.cpp).
            target_zone_scan_type: crate::rules::phobos_switch::phobos_read(|| {
                TargetZoneScanType::read_ini(section)
            }),
            ai_planning_count: crate::rules::npatch_switch::npatch_read(|| {
                building_int(section, category, "AIPlanningCount") & 0xFF
            }),
            requires_stolen_allied_tech: section.read_bool("RequiresStolenAlliedTech", false),
            requires_stolen_soviet_tech: section.read_bool("RequiresStolenSovietTech", false),
            requires_stolen_third_tech: section.read_bool("RequiresStolenThirdTech", false),
            // Constructor value; only the ordered Process owner reads it.
            image: id.to_owned(),
            shp_image_loads: Vec::new(),
            voxel_image_load: None,
            voxel_model_slots: Default::default(),
            power: section.read_int("Power", 0),
            // Only the ordered Rules processor reads it: a later layer's
            // non-negative value keeps an earlier drain.
            extra_power: Default::default(),
            // Building ctor45DF13 initializes +EF0=0 (1x1). ReadINI461225
            // reads Foundation from ART only, never this RULES section.
            // Pass processing projects the shared reader; ART binding does not.
            foundation: crate::rules::foundation::Foundation::default(),
            pixel_selection_bracket_delta: section.read_int("PixelSelectionBracketDelta", 0),
            // Constructor default; the ordered retained TechnoType reader
            // accumulates these keys over every reached pass.
            #[cfg(feature = "phobos")]
            phobos_insignia: crate::rules::insignia_type::PhobosInsignia::default(),
            ares_insignia: crate::rules::insignia_type::AresInsignia::default(),
            #[cfg(feature = "phobos")]
            phobos_passengers_by_size: crate::rules::ares_switch::not_ares_read(|| {
                section.read_ares_bool("Passengers.BySize", true)
            }),
            #[cfg(feature = "phobos")]
            phobos_invulnerability:
                crate::rules::invulnerability_rules::PhobosInvulnerability::parse(section),
            // Replaced by the ordered retained Type reader in RuleSet.
            lepton_mind_control_offset: DEFAULT_LEPTON_MIND_CONTROL_OFFSET,
            build_cat: section
                .read_name("BuildCat", 0x20)
                .and_then(BuildCategory::from_ini),
            adjacent: section.read_int("Adjacent", 3),
            protect_with_wall: section.read_bool("ProtectWithWall", false),
            wants_extra_space: section.read_bool("WantsExtraSpace", false),
            base_normal: section.read_bool("BaseNormal", true),
            eligibile_for_ally_building: section.read_bool("EligibileForAllyBuilding", false),
            crewed: section.read_bool("Crewed", false),
            // Constructor empty vectors and -1 scalars: RuleSet's fixed
            // SOUNDMD binder owns the selection, order and feedback voice
            // reads, and MoveSound below.
            voice_select: Vec::new(),
            voice_select_enslaved: Vec::new(),
            voice_select_deactivated: Vec::new(),
            voice_move: Vec::new(),
            voice_attack: Vec::new(),
            voice_attack_overrides: [const { None }; 4],
            voice_harvest: None,
            voice_deploy: None,
            voice_undeploy: None,
            voice_enter: None,
            ares_voice_repair: None,
            voice_capture: None,
            prevent_attack_move: section.read_bool("PreventAttackMove", false),
            voice_die: section
                .read_sound_list("VoiceDie")
                .map(|tokens| tokens.into_iter().map(str::to_owned).collect())
                .unwrap_or_default(),
            die_sounds: section
                .read_sound_list("DieSound")
                .map(|tokens| tokens.into_iter().map(str::to_owned).collect())
                .unwrap_or_default(),
            create_sound: None,
            secret_lab: Default::default(),
            damage_sound: None,
            promotion_sounds: [None, None],
            vehicle_thief_sounds: [None, None],
            move_sound: Vec::new(),
            crashing_sound: section.read_name("CrashingSound", 0x80).map(str::to_owned),
            voice_crashing: section.read_name("VoiceCrashing", 0x80).map(str::to_owned),
            // Native constructors store -1; the process owner later binds
            // these references against its fixed SOUNDMD catalog.
            sinking_sound: None,
            voice_sinking: None,
            voice_falling: None,
            voice_comment: Vec::new(),
            // TechnoTypeClass+0x560 starts at -1. RuleSet's fixed SOUNDMD
            // binder later resolves the layered ReadINI7133BA reference.
            turret_rotate_sound: None,
            // `+0x5A8`/`+0x5AC` start at -1; the binder resolves them like
            // `TurretRotateSound=`.
            activate_sound: None,
            deactivate_sound: None,
            // ObjectTypeClass+0x1F4 starts at -1 (5F7140); the binder resolves
            // the layered ReadINI5F93C1 reference.
            ambient_sound: None,
            impact_water_sound: section
                .read_name("ImpactWaterSound", 0x80)
                .map(str::to_owned),
            impact_land_sound: section
                .read_name("ImpactLandSound", 0x80)
                .map(str::to_owned),
            voice_feedback: Vec::new(),
            voice_special_attack: Vec::new(),
            // ObjectTypeCtor5F713A; ProcessedType projects the retained
            // fixed-Voc binding from inherited ReadINI5F938B..5F93D5.
            crush_sound: None,
            deploy_sound: section.read_name("DeploySound", 0x80).map(str::to_owned),
            buildup_sound: None,
            working_sound: None,
            not_working_sound: None,
            undeploy_sound: section.read_name("UndeploySound", 0x80).map(str::to_owned),
            enter_water_sound: None,
            leave_water_sound: None,
            packup_sound: section.read_name("PackupSound", 0x80).map(str::to_owned),
            enter_transport_sound: section
                .read_name("EnterTransportSound", 0x80)
                .map(str::to_owned),
            leave_transport_sound: section
                .read_name("LeaveTransportSound", 0x80)
                .map(str::to_owned),
            chrono_in_sound: section.read_name("ChronoInSound", 0x80).map(str::to_owned),
            chrono_out_sound: section.read_name("ChronoOutSound", 0x80).map(str::to_owned),
            // Native relative order: ManualReload713352, TurretSpins713357,
            // TiltCrashJumpjet713386, then Turret. The shared bool reader
            // retains earlier-layer values when a later token is malformed.
            manual_reload: section.read_bool("ManualReload", false),
            turret_spins: section.read_bool("TurretSpins", false),
            tilt_crash_jumpjet: section.read_bool("TiltCrashJumpjet", false),
            has_turret: section.read_bool("Turret", false),
            // gamemd writes a separate UnitType +0x398=10 for Harvester/Weeder,
            // but ROT= remains the parsed TechnoType +0x71C facing-rate field.
            turret_rot: section.read_int("ROT", 0),
            // Constructor values. Recoil, Airstrike and RadialFireSegments
            // are read only by the ordered Process owner (`native_processing`),
            // whose retained per-pass result `RuleSet` publishes.
            recoil: crate::rules::recoil::RecoilConfig::default(),
            airstrike: crate::rules::airstrike::AirstrikeSettings::default(),
            radial_fire_segments: 0,
            // Slot 9's record, which ReadINI fills from the rules INI in this
            // order (`0x00464450..0x00464647`): the two names into `char[16]`,
            // then X, Y, ZAdjust, YSort and the voxel flag.
            //
            // RESIDUAL: gamemd copies the normal name into an empty damaged
            // one in each pass (`0x004644FD`), so the copy from the first pass
            // stays. Trigger: a later rules layer (mode INI or map) renames
            // `TurretAnim` and no layer sets `TurretAnimDamaged`; effect: the
            // damaged building shows the new name here, the first layer's
            // name in gamemd. No retail type has an SHP turret.
            turret_anim: section.read_name("TurretAnim", 0x10).map(str::to_uppercase),
            turret_anim_damaged: section
                .read_name("TurretAnimDamaged", 0x10)
                .or_else(|| section.read_name("TurretAnim", 0x10))
                .map(str::to_uppercase),
            turret_anim_is_voxel: section.read_bool("TurretAnimIsVoxel", false),
            turret_anim_x: section.read_int("TurretAnimX", 0),
            turret_anim_y: section.read_int("TurretAnimY", 0),
            turret_anim_z_adjust: section.read_int("TurretAnimZAdjust", 0),
            turret_anim_y_sort: section.read_int("TurretAnimYSort", 0),
            // Native order after TurretAnimIsVoxel (0x00464647): the
            // `BarrelAnimIsVoxel` bool (0x00464661), the VoxelBarrelFile name
            // this port does not read (0x0046467E, retained above), then the
            // `Read3Int` pivot triples (0x00464697..0x004646E9) whose
            // self-default keeps an earlier layer's triple. VERA does not
            // read the barrel-end triple (0x00464712); see `BarrelVoxelOffsets`.
            barrel_anim_is_voxel: section.read_bool("BarrelAnimIsVoxel", false),
            barrel_voxel: BarrelVoxelOffsets {
                pitch_pivot: section.read_coord3_fold("VoxelBarrelOffsetToPitchPivotPoint", [0; 3]),
                rotate_pivot: section
                    .read_coord3_fold("VoxelBarrelOffsetToRotatePivotPoint", [0; 3]),
                building_pivot: section
                    .read_coord3_fold("VoxelBarrelOffsetToBuildingPivotPoint", [0; 3]),
            },
            guard_range: present_range("GuardRange"),
            air_range_bonus: present_range("AirRangeBonus"),
            // Default no (passive acquisition off unless the type opts in).
            opportunity_fire: section.read_bool("OpportunityFire", false),
            // Default yes (retaliation allowed unless the type opts out).
            can_retaliate: section.read_bool("CanRetaliate", true),
            no_manual_fire: ares_way && section.read_ares_bool("NoManualFire", false),
            // TechnoTypeExt +0x304 and +0x4C4 (constructor false/true), and
            // +0x46C below: read in every Rules mode, and only Ares code
            // consumes them.
            ares_immune_to_abduction: section.read_ares_bool("ImmuneToAbduction", false),
            ignores_firestorm: ares_way && section.read_bool("IgnoresFirestorm", false),
            ares_passengers_by_size: section.read_ares_bool("Passengers.BySize", true),
            saboteur: section.read_ares_bool("Saboteur", false),
            infiltration: Default::default(),
            building_type_ext: Default::default(),
            veterancy: Default::default(),
            promotion_eva: [None, None],
            lost_eva: None,
            academy: Default::default(),
            production: Default::default(),
            ares_cursors: Default::default(),
            keep_alive: if ares_way {
                section.read_ares_nullable_bool("KeepAlive", None)
            } else {
                None
            },
            convert_script: None,
            convert_water: None,
            convert_land: None,
            water_image: None,
            turret_rot_override: None,
            spotlight: Default::default(),
            driver: Default::default(),
            selection_group: String::new(),
            bounty: Default::default(),
            crew: Default::default(),
            techno_combat: Default::default(),
            cloak_power: Default::default(),
            cloak_sounds: [None, None],
            factory_owners: Default::default(),
            explicit_factory: Default::default(),
            // ReadINI7144AF then7144C9. The shared bool reader folds only
            // post-allocation passes retained by native_processing, preserving
            // an earlier false when a later key is missing or malformed.
            can_approach_target: section.read_bool("CanApproachTarget", true),
            can_recalc_approach_target: section.read_bool("CanRecalcApproachTarget", true),
            // Default yes. The INI spelling really is "Aquire" — do not correct it.
            can_passive_acquire: section.read_bool("CanPassiveAquire", true),
            spray_attack: section.read_bool("SprayAttack", false),
            distributed_fire: section.read_bool("DistributedFire", false),
            vhp_scan: VhpScan::read_ini(section),
            explodes: section.read_bool("Explodes", false),
            veteran_abilities,
            elite_abilities,
            #[cfg(feature = "phobos")]
            immune_to_crit: section.read_bool("ImmuneToCrit", false),
            #[cfg(feature = "phobos")]
            veteran_crit_chance: section.read_phobos_nullable_double("VeteranCritChance", None),
            #[cfg(feature = "phobos")]
            phobos_abilities,
            self_healing: section.read_bool("SelfHealing", false),
            // Constructor null; only the ordered Process owner reads it.
            death_weapon: None,
            // A float field (`FSTP dword` at `0x0071232B`).
            death_weapon_damage_modifier: section.read_float("DeathWeaponDamageModifier", 1.0),
            super_weapon: section.read_name("SuperWeapon", 0x20).map(str::to_owned),
            super_weapon2: section.read_name("SuperWeapon2", 0x20).map(str::to_owned),
            super_weapon_per_building: section.read_bool("SuperWeaponPerBuilding", false),
            additional_super_weapons: Vec::new(),
            #[cfg(feature = "phobos")]
            ore_gathering: PhobosOreGathering::default(),
            nuke_silo: section.read_bool("NukeSilo", false),
            charged_anim_time: section.read_float("ChargedAnimTime", 999.0),
            spy_sat: section.read_bool("SpySat", false),
            gap_generator: section.read_bool("GapGenerator", false),
            radar: section.read_bool("Radar", false),
            radar_invisible: section.read_bool("RadarInvisible", false),
            radar_visible: section.read_bool("RadarVisible", false),
            insignificant: section.read_bool("Insignificant", false),
            to_protect: section.read_bool("ToProtect", false),
            harvester: section.read_bool("Harvester", false),
            crate_goodie: section.read_bool("CrateGoodie", false),
            carries_crate: category == ObjectCategory::Vehicle
                && section.read_bool("CarriesCrate", false),
            spawned: section.read_bool("Spawned", false),
            refinery: section.read_bool("Refinery", false),
            weeder: section.read_bool("Weeder", false),
            dock_unload: section.read_bool("DockUnload", false),
            bib: section.read_bool("Bib", false),
            gate: section.read_bool("Gate", false),
            // Double fields (`0x00714B94`, `0x00460DE0`).
            deploy_time_ticks: native_minutes_to_ticks(section.read_double("DeployTime", 0.0)),
            gate_close_delay_ticks: native_minutes_to_ticks(
                section.read_double("GateCloseDelay", 0.0),
            ),
            storage: section.read_int("Storage", 0),
            // Constructor null and empty; only the ordered Process owner
            // reads them.
            free_unit: None,
            dock: Vec::new(),
            queueing_cell: [0, 0], // merged from art.ini later
            pads: Vec::new(),      // merged from art.ini later
            hidden_occupancy: BuildingHiddenOccupancyProfile::default(),
            base_reservation_spacing: None,
            // Constructor nulls; only the ordered Process owner reads them.
            unloading_class: None,
            deploying_anim: None,
            ammo: section.read_int("Ammo", -1),
            initial_ammo: section.read_int("InitialAmmo", -1),
            // TechnoTypeClass::ReadINI71486A..7148B2, raw signed frames.
            // The retained section folds each reached pass with its current
            // member as the next default; no time conversion or clamp.
            reload: section.read_int("Reload", 0),
            empty_reload: section.read_int("EmptyReload", -1),
            reload_increment: section.read_int("ReloadIncrement", 0),
            // ReadINI7148B8..7148EB: bool, then ReadDouble stored as float.
            damage_reduces_readiness: section.read_bool("DamageReducesReadiness", false),
            readiness_reduction_multiplier: section.read_float("ReadinessReductionMultiplier", 0.0),

            // Spawn manager pool. Constructor null; only the ordered Process
            // owner reads `Spawns=`.
            spawns: None,
            spawns_number: section.read_int("SpawnsNumber", 0).min(SPAWNS_NUMBER_LIMIT),
            spawn_regen_rate: section.read_int("SpawnRegenRate", 0).max(0) as u32,
            spawn_reload_rate: section.read_int("SpawnReloadRate", 0).max(0) as u32,
            missile_spawn: section.read_bool("MissileSpawn", false),
            no_spawn_alt_model: section.read_bool("NoSpawnAlt", false).then(|| {
                format!(
                    "{id}{}",
                    crate::rules::voxel_frame_catalog::NO_SPAWN_ALT_SUFFIX
                )
            }),

            // Slave Miner / economy fields
            // Constructor null; only the ordered Process owner reads it.
            enslaves: None,
            slaves_number: section.read_int("SlavesNumber", 0).min(SLAVES_NUMBER_LIMIT),
            slave_regen_rate: section.read_int("SlaveRegenRate", 0),
            slave_reload_rate: section.read_int("SlaveReloadRate", 0),
            slaved: section.read_bool("Slaved", false),
            fearless: section.read_bool("Fearless", false),
            fraidycat: section.read_bool("Fraidycat", false),
            // InfantryTypeCtor5237A1; only the ordered retained reader
            // supplies Civilian, independently of Image or House identity.
            civilian: false,
            // InfantryTypeClass's constructor sets Crawls (`+0xEBD`) to true
            // (`0x00523788`); the per-pass ART read projects over it
            // ([`InfantryArt`]). Other types have no such field.
            crawls: category == ObjectCategory::Infantry,
            fire_up_frame: 0,
            fire_prone_frame: 0,
            secondary_fire_frame: 0,
            secondary_prone_frame: 0,
            harvest_rate: section.read_int("HarvestRate", 1),
            resource_gatherer: section.read_bool("ResourceGatherer", false),
            resource_destination: section.read_bool("ResourceDestination", false),
            ore_purifier: section.read_bool("OrePurifier", false),

            // Locomotor / movement fields
            // Absent key and unparseable CLSID both resolve to Teleport, which
            // is the type constructor's seed — see
            // `sim::movement::locomotion::install` for why there is one rule
            // here and not a per-category table.
            // `INIClass::ReadCLSID @ 0x00527920` copies `char[128]` and trims
            // before `CLSIDFromString`.
            locomotor: crate::rules::locomotor_type::resolve_installed_kind(
                section.read_name("Locomotor", 0x80),
            ),
            // InfantryType5236A0 and BuildingType45DD9D pass Foot0 to
            // TechnoType710AF0, stored7110E0. Original Infantry constructor and
            // ENGINEER retail-layer reads: rules_oracle/infantry_speed_type.
            // A UnitType keeps its -1 constructor seed here: only the ordered
            // Process owner reads it (`SpeedType::read_vehicle_pass`, retained
            // across admitted Rules passes), and `RuleSet` publishes that value.
            speed_type: if category == ObjectCategory::Vehicle {
                SpeedType::Invalid
            } else {
                section.read_speed_type(
                    "SpeedType",
                    if matches!(
                        category,
                        ObjectCategory::Building | ObjectCategory::Infantry
                    ) {
                        SpeedType::Foot
                    } else {
                        SpeedType::default()
                    },
                )
            },
            movement_zone: section
                .read_name("MovementZone", 0x20)
                .map(MovementZone::from_ini)
                .unwrap_or_default(),
            movement_restricted_to: section.read_name("MovementRestrictedTo", 0x80).and_then(
                |value| {
                    LandType::ALL
                        .into_iter()
                        .find(|land| value.eq_ignore_ascii_case(land.section_name()))
                },
            ),
            // gamemd-derived: AircraftTypeClass::Constructor @ 0x0041C8B0 sets
            // TechnoTypeClass+0xD96 true after the parent constructor; the
            // ReadINI site @ 0x00714FE9 then applies an explicit key override.
            considered_aircraft: section
                .read_bool("ConsideredAircraft", category == ObjectCategory::Aircraft),
            zfudge_cliff: section.read_int("ZFudgeCliff", 10),
            zfudge_column: section.read_int("ZFudgeColumn", 5),
            zfudge_tunnel: section.read_int("ZFudgeTunnel", 10),
            zfudge_bridge: section.read_int("ZFudgeBridge", 0),
            too_big_to_fit_under_bridge: section.read_bool("TooBigToFitUnderBridge", false),
            crashable: section.read_bool("Crashable", false),
            aircraft_crashable: None,
            curtain_modifiers: [NativeF64Bits::ONE; 2],
            teleporter: section.read_bool("Teleporter", false),
            move_to_shroud: section.read_bool("MoveToShroud", category != ObjectCategory::Aircraft),
            hover_attack: section.read_bool("HoverAttack", false),
            balloon_hover: section.read_bool("BalloonHover", false),
            target_laser: section.read_bool("TargetLaser", false),
            is_simple_deployer: section.read_bool("IsSimpleDeployer", false),
            is_limpet_mine: category == ObjectCategory::Building
                && section.read_bool("IsLimpetMine", false),
            is_limpet_drone: category == ObjectCategory::Vehicle
                && section.read_bool("IsLimpetDrone", false),
            small_visceroid: category == ObjectCategory::Vehicle
                && section.read_bool("SmallVisceroid", false),
            large_visceroid: category == ObjectCategory::Vehicle
                && section.read_bool("LargeVisceroid", false),
            tiberium_heal: ares_way && section.read_bool("TiberiumHeal", false),
            deploy_to_land: section.read_bool("DeployToLand", false),
            airport_bound: section.read_bool("AirportBound", false),
            fighter: section.read_bool("Fighter", false),
            fly_by: section.read_bool("FlyBy", false),
            fly_back: section.read_bool("FlyBack", false),
            landable: section.read_bool("Landable", false),
            carryall: section.read_bool("Carryall", false),
            aircraft_trailer: Default::default(),
            // gamemd-derived: `TechnoTypeClass::ReadINI` reads `JumpJet` into
            // its own boolean at `+0xD94` (`0x007151EC PUSH 0x843640` ->
            // `0x00715200 MOV [EBP+0xD94],AL`), the last member of the same
            // straight-line run that reads the nine parameters below. It is a
            // sibling of theirs, not a gate on them.
            jumpjet: section.read_bool("JumpJet", false),
            // gamemd-derived: the nine `Jumpjet*` parameters are read
            // unconditionally for every section — `TechnoTypeClass::ReadINI`
            // `0x00715020`-`0x0071520F` contains no branch instruction at all,
            // so `JumpJet=` cannot gate them. Both stock sections that take
            // their Jumpjet locomotor from the GUID alone, `[ZEP]` (Kirov,
            // `JumpjetHeight=750`) and `[DISK]` (Floating Disc, likewise 750),
            // omit `JumpJet=`; gating the block on it threw away all nine of
            // their authored keys and dropped both units to the constructor's
            // 500-lepton hover. Only the Jumpjet locomotor consults these —
            // the parameter copy at `0x0054AD30` reads `+0xD70`..`+0xD8C` off
            // the type — so carrying them on every `ObjectType`, as
            // `TechnoTypeClass` does, changes nothing for other locomotors.
            jumpjet_params: JumpjetParams::from_ini_section(section),

            // Crush properties. `Crushable=` is the one key whose default is
            // category-dependent: the ObjectTypeClass constructor clears the
            // byte, but the InfantryTypeClass constructor overwrites it with 1,
            // so every infantry type defaults `Crushable=yes` and everything
            // else defaults no. Stock rulesmd leaves the key off ~35 infantry
            // sections (Conscript, Engineer, Flak Trooper, ...), so a blanket
            // `false` here makes most of the game's infantry un-crushable.
            crushable: section.read_bool("Crushable", category == ObjectCategory::Infantry),
            deployed_crushable: section.read_bool("DeployedCrushable", true),
            crusher: section.read_bool("Crusher", false),
            tilts_when_crushes: section.read_bool("TiltsWhenCrushes", true),
            omni_crusher: section.read_bool("OmniCrusher", false),
            omni_crush_resistant: section.read_bool("OmniCrushResistant", false),
            immune_to_radiation: section.read_bool("ImmuneToRadiation", false),
            damage_self: section.read_bool("DamageSelf", false),
            immune: section.read_bool("Immune", false),
            type_immune: section.read_bool("TypeImmune", false),
            emp: None,
            promotion_abilities: None,
            self_attach_effect: None,
            cloak_allowed: true,
            immune_to_psionics: section
                .read_bool("ImmuneToPsionics", category == ObjectCategory::Building),
            warpable: section.read_bool("Warpable", true),
            bombable: section.read_bool("Bombable", true),
            bomb_sight: section.read_int("BombSight", 0),
            mind_control_ring_offset: section.read_int("MindControlRingOffset", 0x8C),
            // Ctor710FA5=-1; bind_scalar_sound_references owns the fixed
            // Voc lookup, including registered none spellings.
            mind_cleared_sound: None,
            #[cfg(feature = "phobos")]
            mind_control_range_limit: section.read_phobos_leptons("MindControlRangeLimit", 0),
            #[cfg(feature = "phobos")]
            mind_control_ignore_size: section
                .read_phobos_nullable_bool("MindControl.IgnoreSize", None),
            #[cfg(feature = "phobos")]
            mind_control_size: crate::rules::ares_switch::not_ares_read(|| {
                section.read_ares_int("MindControlSize", 1)
            }),
            #[cfg(feature = "phobos")]
            mind_control_link_visible_to_house: section.read_phobos_nullable_with(
                "MindControlLink.VisibleToHouse",
                None,
                crate::rules::phobos_shields::AffectedHouse::parse,
            ),
            #[cfg(feature = "phobos")]
            multi_mind_control_release_victim: section
                .read_phobos_nullable_bool("MultiMindControl.ReleaseVictim", None),
            immune_to_psionic_weapons: section.read_bool(
                "ImmuneToPsionicWeapons",
                category == ObjectCategory::Building,
            ),
            immune_to_poison: section.read_bool("ImmuneToPoison", false),

            // Constructor nulls; only the ordered Process owner reads them.
            deploys_into: None,
            undeploys_into: None,
            powers_unit: None,
            powered_unit: section.read_bool("PoweredUnit", false),
            // `ReadInt(DeployFacing, field >> 5) << 5` (`0x00460C6C..0x00460C86`).
            deploy_facing: (section.read_int("DeployFacing", 0x80 >> 5).clamp(0, 7) as u8) << 5,
            construction_yard: section.read_bool("ConstructionYard", false),
            build_const_eligible: false,
            base_plan_type_index: -1,
            // BuildingTypeClass__ReadINI 0x00460FFC..0x00461010 writes
            // `IsBaseDefense=` to BuildingType+0x1706; constructor default false.
            is_base_defense: section.read_bool("IsBaseDefense", false),
            hover_pad: building_bool(section, category, "HoverPad"),
            is_temple: building_bool(section, category, "IsTemple"),
            is_plug: building_bool(section, category, "IsPlug"),
            anti_air_value: building_int(section, category, "AntiAirValue"),
            anti_armor_value: building_int(section, category, "AntiArmorValue"),
            anti_infantry_value: building_int(section, category, "AntiInfantryValue"),
            factory_rtti: if category == ObjectCategory::Building {
                section.read_factory("Factory", 0)
            } else {
                0
            },
            // `BuildingTypeClass::ReadINI` alone reads it (`0x00460A79`).
            weapons_factory: building_bool(section, category, "WeaponsFactory"),
            armory,
            gdi_barracks,
            nod_barracks,
            yuri_barracks,
            cloning: section.read_bool("Cloning", false),
            // Read3Int (`0x00460FCD`).
            exit_coord: section
                .read_coord3_value("ExitCoord")
                .map(|[x, y, z]| (x, y, z)),

            // Cursor / interaction capability flags. An InfantryType's
            // Engineer, C4, Infiltrate and Agent start at the constructor's
            // false: only the ordered Process owner reads them
            // (`InfantryInfiltration`), and `RuleSet` publishes its result.
            engineer: category != ObjectCategory::Infantry && section.read_bool("Engineer", false),
            ivan: category == ObjectCategory::Infantry && section.read_bool("Ivan", false),
            infiltrate: false,
            agent: false,
            deployer: section.read_bool("Deployer", false),
            capturable: section.read_bool("Capturable", false),
            spyable: category == ObjectCategory::Building && section.read_bool("Spyable", false),
            needs_engineer: section.read_bool("NeedsEngineer", false),
            capture_eva_event: section
                .read_name("CaptureEvaEvent", 0x64)
                .map(str::to_owned),
            // Repairable defaults to true — most buildings can be repaired in RA2.
            repairable: section.read_bool("Repairable", true),
            can_be_occupied: section.read_bool("CanBeOccupied", false),
            leave_rubble: category == ObjectCategory::Building
                && section.read_bool("LeaveRubble", false),
            crate_beneath: category == ObjectCategory::Building
                && section.read_bool("CrateBeneath", false),
            crate_beneath_is_money: category == ObjectCategory::Building
                && section.read_bool("CrateBeneathIsMoney", false),
            can_occupy_fire: section.read_bool("CanOccupyFire", false),
            // Decided by the RuleSet at load.
            ra2_occupy_fire: false,
            show_occupant_pips: section.read_bool("ShowOccupantPips", true),
            bridge_repair_hut: section.read_bool("BridgeRepairHut", false),
            laser_fence: section.read_bool("LaserFence", false),
            laser_fence_post: section.read_bool("LaserFencePost", false),
            firestorm_wall: section.read_bool("FirestormWall", false),
            passengers: section.read_int("Passengers", 0),
            // `TechnoTypeClass::ReadINI @ 0x00712170` reads Size first from
            // key `0x00820178`, then SizeLimit from `0x008443BC`; both use the
            // current qword as `ReadDouble @ 0x005283D0`'s default and retain
            // the returned binary64 without clamping.
            size: section.read_double("Size", 1.0),
            size_limit: section.read_double("SizeLimit", 0.0),
            open_topped: section.read_bool("OpenTopped", false),
            gunner: false, // Constructor byte; retained reader binds GunnerTurrets::is_gunner.
            ifv_mode: section.read_int("IFVMode", 0),
            // Constructor table; only the ordered Process owner reads it.
            gunner_turrets: crate::rules::gunner_turrets::GunnerTurrets::default(),
            open_transport_weapon: section.read_int("OpenTransportWeapon", -1),
            deploy_fire: section.read_bool("DeployFire", false),
            // `TechnoTypeClass::Constructor @ 0x00711187` seeds -1, and
            // `ReadINI @ 0x00714BBA` only overwrites it when the key is present.
            undeploy_delay: section.read_int("UndeployDelay", -1),
            fire_angle: section.read_int("FireAngle", 8),
            deploy_fire_weapon: section.read_int("DeployFireWeapon", 1),
            // Phobos holds this in InfantryTypeExt, so only an
            // `[InfantryTypes]` section carries it.
            #[cfg(feature = "phobos")]
            infantry_auto_deploy: if category == ObjectCategory::Infantry {
                section.read_phobos_nullable_bool("InfantryAutoDeploy", None)
            } else {
                None
            },
            max_number_occupants: section.read_int("MaxNumberOccupants", 0).max(0) as u32,
            occupier: section.read_bool("Occupier", false),
            assaulter: section.read_bool("Assaulter", false),
            thief: category == ObjectCategory::Infantry && section.read_bool("Thief", false),
            vehicle_thief: category == ObjectCategory::Infantry
                && section.read_bool("VehicleThief", false),
            // Constructor nulls; only the ordered Process owner reads them.
            occupy_weapon: None,
            elite_occupy_weapon: None,
            // Constructor values; only the ordered Process owner reads them.
            infantry_pips: InfantryPips::default(),
            infantry_sequence_bank: (category == ObjectCategory::Infantry).then(Default::default),
            sequence_sounds: Vec::new(),
            pip_scale: section
                .read_name("PipScale", 0x20)
                .map(PipScale::from_ini)
                .unwrap_or_default(),
            pips_draw_for_all: section.read_bool("PipsDrawForAll", false),
            pip_wrap: section.read_int("PipWrap", 0),
            unit_absorb: section.read_bool("UnitAbsorb", false),
            infantry_absorb: section.read_bool("InfantryAbsorb", false),
            grinding: section.read_bool("Grinding", false),
            hospital,
            bunkerable: section.read_bool("Bunkerable", category == ObjectCategory::Vehicle),
            // Constructor values: the ordered Rules processor owns the
            // per-pass weapon block and RuleSet publishes its arrays.
            weapon_list: vec![None; WEAPON_SLOT_COUNT],
            elite_weapon_list: vec![None; WEAPON_SLOT_COUNT],
            resolved_weapons: Default::default(),
            resolved_debris: Default::default(),
            weapon_offsets: Default::default(),
            kratos_freeze: crate::rules::kratos_freeze::FreezeRules::read(section),
            weapon_count: 0,
            naval_targeting: section.read_int("NavalTargeting", 0),
            land_targeting: section.read_int("LandTargeting", 0),
            underwater: section.read_bool("Underwater", false),
            organic: section.read_bool("Organic", category == ObjectCategory::Infantry),
            parasiteable: section.read_bool(
                "Parasiteable",
                matches!(
                    category,
                    ObjectCategory::Infantry | ObjectCategory::Vehicle | ObjectCategory::Aircraft
                ),
            ),
            suppression_threshold: section.read_int("SuppressionThreshold", 0),
            reselect_if_limboed: section.read_bool("ReselectIfLimboed", false),
            rejoin_team_if_limboed: section.read_bool("RejoinTeamIfLimboed", false),
            unnatural: section.read_bool("Unnatural", false),
            natural: section.read_bool("Natural", false),
            pushy: section.read_bool("Pushy", false),
            berserk_friendly: section.read_bool("BerserkFriendly", false),
            mobile_fire: section.read_bool("MobileFire", true),
            hunter_seeker: section.read_bool("HunterSeeker", false),
            designator_range: None,
            chronoshift: Default::default(),
            inhibitor_range: None,
            hunter_seeker_ignore: false,
            non_vehicle: category == ObjectCategory::Vehicle
                && section.read_bool("NonVehicle", false),
            jumpjet_turn: category == ObjectCategory::Infantry
                && section.read_bool("JumpJetTurn", false),
            emp_pulse_cannon: category == ObjectCategory::Building
                && section.read_bool("EMPulseCannon", false),
            has_stupid_guard_mode: category == ObjectCategory::Building
                && section.read_bool("HasStupidGuardMode", true),
            tick_tank: category == ObjectCategory::Building && section.read_bool("TickTank", false),
            artillary: category == ObjectCategory::Building
                && section.read_bool("Artillary", false),
            // Retained per-pass flag/table from the ordered Rules owner.
            is_gattling: false,
            gattling_stages: Default::default(),
            turret_count: 0,
            has_turret_tooltips: section.read_bool("HasTurretTooltips", false),
            drainable: section.read_bool("Drainable", false),
            // Constructor defaults UNCHECKED; stock authors write all three
            // on CAOILD only (rulesmd.ini:13949-13951) and comment them out
            // elsewhere, so the absent-key value 0 matches the dormant case.
            produce_cash_startup: section.read_int("ProduceCashStartup", 0),
            produce_cash_amount: section.read_int("ProduceCashAmount", 0),
            produce_cash_delay: section.read_int("ProduceCashDelay", 0),
            overpowerable: section.read_bool("Overpowerable", false),
            attack_friendlies: section.read_bool("AttackFriendlies", false),
            attack_cursor_on_friendlies: section.read_bool("AttackCursorOnFriendlies", false),
            sabotage_cursor: section.read_bool("SabotageCursor", false),
            c4: category != ObjectCategory::Infantry && section.read_bool("C4", false),
            can_c4: section.read_bool("CanC4", category == ObjectCategory::Building),
            #[cfg(feature = "phobos")]
            allow_airstrike: section.read_bool_value("AllowAirstrike"),
            eligible_for_delay_kill: section.read_bool("EligibleForDelayKill", false),
            invisible: section.read_bool("Invisible", false),
            legal_target: section.read_bool("LegalTarget", true),
            invisible_in_game: section.read_bool("InvisibleInGame", false),
            place_anywhere: section.read_bool("PlaceAnywhere", false),
            to_tile: section.read_name("ToTile", 0x3c).map(str::to_owned),
            // BuildingType45FE50: ReadBool calls460915/46092F/460949,
            // constructor45DD90 clears+16A9/+16AA/+16AB in that order.
            // Native identity/control coverage: building_repair.depot_service.md.
            unit_repair: section.read_bool("UnitRepair", false),
            unit_reload: section.read_bool("UnitReload", false),
            bunker: section.read_bool("Bunker", false),
            helipad: section.read_bool("Helipad", false),
            number_of_docks: section.read_int("NumberOfDocks", 1),
            // TogglePower defaults to true for buildings, false for units.
            toggle_power: section.read_bool("TogglePower", category == ObjectCategory::Building),
            // BuildingType+0x1564/+0x1568. The native constructor does not write
            // them, so an absent key keeps the reader's argument (zero).
            infantry_gain_self_heal: section.read_int("InfantryGainSelfHeal", 0),
            units_gain_self_heal: section.read_int("UnitsGainSelfHeal", 0),
            powered: section.read_bool("Powered", false),
            powered_special: section.read_bool("PoweredSpecial", false),
            // Original714404..71446C reads this block in this order,
            // retaining each current value across reached rules passes.
            can_disguise: section.read_bool("CanDisguise", false),
            perma_disguise: section.read_bool("PermaDisguise", false),
            detect_disguise: section.read_bool("DetectDisguise", false),
            disguise_when_still: section.read_bool("DisguiseWhenStill", false),
            wall: section.read_bool("Wall", false),
            to_overlay: None,
            unsellable: section.read_bool("Unsellable", false),
            click_repairable: section.read_bool("ClickRepairable", true),
            // Selectable defaults to yes — the ObjectTypeClass constructor seeds
            // the field true and only the 67 stock types that spell out
            // `Selectable=no` turn it off.
            selectable: section.read_bool("Selectable", true),
            is_selectable_combatant: section.read_bool("IsSelectableCombatant", false),
            // The constructor's value; ART supplies the key (`project_art_data`).
            can_be_hidden: true,
            silo_damage: false,
            custom_palette: None,
            reveal_to_all: section.read_bool("RevealToAll", false),

            // Naval flags
            water_bound: {
                // WaterBound defaults to true if SpeedType is already Float.
                let default = section
                    .read_name("SpeedType", 0x80)
                    .is_some_and(|s| s.eq_ignore_ascii_case("Float"));
                section.read_bool("WaterBound", default)
            },
            naval: section.read_bool("Naval", false),
            number_impassable_rows: section.read_int("NumberImpassableRows", -1),

            // Rules' ordered processor publishes the retained reader result.
            // Merged text cannot reproduce missing-key /1000 truncation.
            building_light: Default::default(),
            building_upgrade: Default::default(),
            has_spotlight: section.read_bool("HasSpotlight", false),

            // TechnoType particle effects. The Type references start null
            // and empty; only the ordered Process owner reads them.
            natural_particle_system: None,
            natural_particle_location: IVec3::from_array(
                section.read_coord_tokens("NaturalParticleLocation", [0; 3]),
            ),
            refinery_smoke_particle_system: None,
            damage_particle_systems: Vec::new(),
            max_debris,
            min_debris,
            debris_types: Vec::new(),
            // Ares 3.0 TechnoTypeClass_LoadFromINI_ListLength (1005BF90,
            // hook 0x7125DF) reads it through ParseList<int> (1005B030).
            debris_maximums: if ares_way {
                crate::rules::ares_switch::not_ares_read(|| {
                    section.read_ares_int_list("DebrisMaximums")
                })
            } else {
                section.read_int_list("DebrisMaximums")
            }
            .unwrap_or_default(),
            debris_anims: Vec::new(),
            close_range: section.read_bool("CloseRange", false),
            stupid_hunt: section.read_bool("StupidHunt", false),
            default_to_guard_area: section.read_bool("DefaultToGuardArea", false),
            cyborg: section.read_bool("Cyborg", false),
            // Firestorm 2.0 InfantryTypeClass::Read_INI 0x004DAB31..0x004DAB45
            // reads it with the current value as its default, from the
            // constructor's false (0x004DA229); the layered read keeps the
            // current value across rules passes the same way.
            is_web_immune: category == ObjectCategory::Infantry
                && section.read_bool("IsWebImmune", false),
            damage_sparks: false,
            use_own_name: category == ObjectCategory::Infantry
                && section.read_bool("UseOwnName", false),
            destroy_particle_systems: Vec::new(),
            damage_smoke_offset: IVec3::from_array(
                section.read_coord_tokens("DamageSmokeOffset", [0; 3]),
            ),
            dam_smk_off_scrn_rel: section.read_bool("DamSmkOffScrnRel", false),
            destroy_smoke_offset: IVec3::from_array(
                section.read_coord_tokens("DestroySmokeOffset", [0; 3]),
            ),
            refinery_smoke_offsets: [
                IVec3::from_array(section.read_coord_tokens("RefinerySmokeOffsetOne", [0; 3])),
                IVec3::from_array(section.read_coord_tokens("RefinerySmokeOffsetTwo", [0; 3])),
                IVec3::from_array(section.read_coord_tokens("RefinerySmokeOffsetThree", [0; 3])),
                IVec3::from_array(section.read_coord_tokens("RefinerySmokeOffsetFour", [0; 3])),
            ],
            refinery_smoke_frames: section.read_int("RefinerySmokeFrames", 25),
            has_radial_indicator: section.read_bool("HasRadialIndicator", false),
            concentric_radial_indicator: category == ObjectCategory::Building
                && section.read_bool("ConcentricRadialIndicator", false),
            // The reached-pass history folds current-default reads before
            // this immutable projection. Low-byte storage commutes with
            // absent/default reads; it must not become a saturation clamp.
            // Read the Ares way, these are Ares TechnoTypeExt::LoadFromINIFile
            // Valueable<int> reads, which keep the whole int.
            gap_radius_in_cells: if ares_way {
                crate::rules::ares_switch::not_ares_read(|| {
                    section.read_ares_int("GapRadiusInCells", 0)
                })
            } else {
                i32::from(section.read_int("GapRadiusInCells", 0) as i8)
            },
            super_gap_radius_in_cells: if ares_way {
                crate::rules::ares_switch::not_ares_read(|| {
                    section.read_ares_int("SuperGapRadiusInCells", 0)
                })
            } else {
                i32::from(section.read_int("SuperGapRadiusInCells", 0) as i8)
            },
            psychic_detection_radius: section.read_int("PsychicDetectionRadius", 0),
            sensor_array: section.read_bool("SensorArray", false),
            sensors: section.read_bool("Sensors", false),
            sensors_sight: section.read_int("SensorsSight", 0).clamp(0, u8::MAX as i32) as u8,
            detect_disguise_range: section
                .read_int("DetectDisguiseRange", 0)
                .clamp(0, u8::MAX as i32) as u8,
            no_shadow: section.read_bool("NoShadow", false),
            cloakable: section.read_bool("Cloakable", false),
            cloaking_speed: section.read_int("CloakingSpeed", 7),
            cloak_stop: section.read_bool("CloakStop", false),
            cloak_radius_in_cells: section.read_int("CloakRadiusInCells", 20) as i8,
            cloak_generator: section.read_bool("CloakGenerator", false),
            // Phobos `BuildingTypeExt::ExtData::LoadFromINIFile` reads these
            // through INI_EX and its 2048-byte buffer.
            #[cfg(feature = "phobos")]
            power_plant_enhancer_power_plants: section
                .read_list("PowerPlantEnhancer.PowerPlants", 0x800)
                .map(|tokens| tokens.into_iter().map(str::to_owned).collect())
                .unwrap_or_default(),
            #[cfg(feature = "phobos")]
            power_plant_enhancer_range: (section
                .read_phobos_double("PowerPlantEnhancer.Range", 0.0)
                * crate::util::lepton::LEPTONS_PER_CELL_I32 as f64)
                as i32,
            #[cfg(feature = "phobos")]
            power_plant_enhancer_amount: crate::rules::ares_switch::not_ares_read(|| {
                section.read_ares_int("PowerPlantEnhancer.Amount", 0)
            }),
            #[cfg(feature = "phobos")]
            power_plant_enhancer_factor: section
                .read_phobos_nullable_double("PowerPlantEnhancer.Factor", Some(1.0))
                .map_or(0.0, |factor| factor as f32),
            #[cfg(feature = "phobos")]
            power_plant_enhancer_max_count: crate::rules::ares_switch::not_ares_read(|| {
                section.read_ares_int("PowerPlantEnhancer.MaxCount", -1)
            }),
            #[cfg(feature = "phobos")]
            power_plant_damage_factor: section.read_phobos_double("PowerPlant.DamageFactor", 1.0),
        }
    }
}

/// A signed key only `BuildingTypeClass::ReadINI` reads (constructor 0).
fn building_int(section: &IniSection, category: ObjectCategory, key: &str) -> i32 {
    if category == ObjectCategory::Building {
        section.read_int(key, 0)
    } else {
        0
    }
}

/// A flag only `BuildingTypeClass::ReadINI` reads (constructor false).
fn building_bool(section: &IniSection, category: ObjectCategory, key: &str) -> bool {
    category == ObjectCategory::Building && section.read_bool(key, false)
}
