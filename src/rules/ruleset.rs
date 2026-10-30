//! Master game data container loaded from rules.ini.
//!
//! RuleSet is the single source of truth for all game object definitions.
//! It parses the type registries ([InfantryTypes], [VehicleTypes], etc.),
//! then loads each referenced object's section into typed structs. Every native
//! registered weapon and warhead is also projected, including retained types.
//!
//! ## Loading strategy
//! 1. Parse type registries → collect all object IDs per category
//! 2. For each ID, look up its [ID] section → parse into ObjectType
//! 3. Seed weapon/warhead closure from native registries and object references
//! 4. Parse those bodies and install retained typed-reader results
//! 5. Log summary counts
//!
//! ## Dependency rules
//! - Part of rules/ — depends on rules/ini_parser, rules/object_type,
//!   rules/weapon_type, rules/warhead_type.
//! - No dependencies on sim/, render/, ui/, etc.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::hash::{Hash, Hasher};

use crate::rules::ares_prerequisites::{GenericPrerequisite, GenericPrerequisites};
use crate::rules::combat_damage::CombatDamageDefaults;
use crate::rules::crate_rules::CrateRules;
use crate::rules::error::RulesError;
use crate::rules::ini_parser::{IniFile, IniSection};
use crate::rules::mission_data::MissionControl;
use crate::rules::native_processing::{
    CountryProjection, CountryVeterancyState, ProcessedRulesLayers, RulesLayerStack,
    SideProjection, house_type_index_of_name,
};
use crate::rules::object_type::{BuildCategory, FactoryType, ObjectCategory, ObjectType};
use crate::rules::particle_system_type::{ParticleSystemType, ParticleSystemTypeId};
use crate::rules::particle_type::{ParticleType, ParticleTypeId, PendingParticleType};
use crate::rules::prerequisite::{Prerequisite, PrerequisiteGroup};
use crate::rules::projectile_type::ProjectileType;
use crate::rules::radar_event_config::RadarEventConfig;
use crate::rules::smudge_type::SmudgeTypeRegistry;
use crate::rules::superweapon_type::SuperWeaponType;
use crate::rules::terrain_object_type::TerrainObjectType;
use crate::rules::terrain_rules::TerrainRules;
use crate::rules::tiberium_type::TiberiumTypeRegistry;
use crate::rules::voxel_anim_type::{VoxelAnimType, VoxelAnimTypeId};
use crate::rules::warhead_type::WarheadType;
use crate::rules::weapon_type::WeaponType;
use crate::util::fixed_math::SimFixed;
use crate::util::lookup_hash::LookupMap;
use crate::util::native_random::unit_draw_threshold;
use crate::util::native_x87::{NativeF32Bits, NativeF64Bits};

#[cfg(test)]
#[path = "building_damage_sound_tests.rs"]
mod building_damage_sound_tests;

#[cfg(test)]
#[path = "command_bar_sound_rules_tests.rs"]
mod command_bar_sound_rules_tests;

#[cfg(test)]
#[path = "multiplayer_rules_tests.rs"]
mod multiplayer_rules_tests;

/// Country-level fields needed by gameplay systems.
#[derive(Debug, Clone)]
pub struct CountryRules {
    /// Immutable projection of retained HouseType+98, never merged-text inheritance.
    parent_country: String,
    /// Retained HouseType+14C/+168/+184 lists projected by their process owner.
    veteran_types: CountryVeterancyState,
    /// Native HouseType+C0, initialized0 at51141C and read through
    /// ReadColor474A90 at51193D with the current index as default. The
    /// Process owner supplies the typed value from each actual catalog pass.
    color_scheme: i32,
    /// HouseType+1A5, ctor511559=false. ReadINI511A5B..511A84 reads
    /// exact Multiplay with retained ReadBool5295F0, after BuildTime and
    /// before MultiplayPassive. Setup4E3A00 gates the country catalog on it.
    multiplay: bool,
    /// `MultiplayPassive=` allows non-owner garrison entry in `BuildingClass::CanBeOccupiedBy`.
    pub multiplay_passive: bool,
    /// `WallOwner=` allows this house type's buildings to claim nearby map walls.
    pub wall_owner: bool,
    /// Phobos `AttachEffect.AttachTypes` on a country.
    ///
    /// RESIDUAL: read but not attached. Phobos `TechnoExt::InitializeAttachEffects`
    /// (`Body.Internal.cpp:157`) attaches the owner country's list to every
    /// created techno as an outside effect whose source is the techno itself,
    /// and `HouseClass` owner change (`House/Hooks.cpp:353`) attaches the new
    /// country's list under `AttachEffect.AttachOnOwnerChange`. VERA's
    /// `AttachSource` holds only a warhead source, so a country that sets
    /// the list gives its objects none of those effects.
    #[cfg(feature = "phobos")]
    pub attach_effect_attach_types: Vec<String>,
    /// `AttachEffect.DurationOverrides`, paired by index.
    #[cfg(feature = "phobos")]
    pub attach_effect_duration_overrides: Vec<i32>,
    /// `AttachEffect.Delays`, paired by index.
    #[cfg(feature = "phobos")]
    pub attach_effect_delays: Vec<i32>,
    /// `AttachEffect.InitialDelays`, paired by index.
    #[cfg(feature = "phobos")]
    pub attach_effect_initial_delays: Vec<i32>,
    /// `AttachEffect.RecreationDelays`, paired by index.
    #[cfg(feature = "phobos")]
    pub attach_effect_recreation_delays: Vec<i32>,
    /// `AttachEffect.AttachOnOwnerChange`; `None` inherits `[General]`.
    #[cfg(feature = "phobos")]
    pub attach_effect_attach_on_owner_change: Option<bool>,
    /// HouseType+148: ReadDouble511D0B -> FSTP float511D10, default1.0.
    /// Both base and purifier deposits read these exact retained float bits.
    pub income_mult: NativeF32Bits,
    /// Per-target category armor multipliers (HouseType `+0x100..+0x110`).
    /// Native stores these as f32 and `HouseClass::GetArmorMultForType @
    /// 0x0050BD30` reads the selected slot live for every receiver call.
    ///
    /// The separate scalar `Armor=` folds with the difficulty into House+1A0
    /// (`HouseClass::SetDifficulty 0x004F6F54`), whose only reader is the
    /// house CRC (`0x00502DD2`); no damage path reads it.
    pub armor_infantry_mult: f32,
    pub armor_units_mult: f32,
    pub armor_aircraft_mult: f32,
    pub armor_buildings_mult: f32,
    pub armor_defenses_mult: f32,
    /// `CostInfantryMult=`, `CostUnitsMult=`, `CostAircraftMult=`,
    /// `CostBuildingsMult=` and `CostDefensesMult=` (HouseType
    /// `+0x114..+0x124`, in [`ObjectType::factor_slot`] order): ReadDouble
    /// into floats (`0x00511B64..0x00511BF9`), the constructor's 1.0
    /// (`0x005114A8..0x005114C0`) as default, no clamp.
    /// `HouseClass::GetCostBonus @ 0x0050BDF0` returns the slot. No retail
    /// country sets them.
    pub cost_mults: [NativeF32Bits; 5],
    /// `SpeedInfantryMult=`, `SpeedUnitsMult=` and `SpeedAircraftMult=`
    /// (HouseType `+0x128/+0x12C/+0x130`): ReadDouble into floats, read in
    /// that order (`0x00511C0D`, `0x00511C2C`, `0x00511C4B`) between the
    /// Cost and BuildTime keys, the constructor's 1.0 as default, no clamp.
    /// `HouseClass::GetSpeedBonus @ 0x0050C050` returns the slot. No retail
    /// country sets them.
    pub speed_mults: [NativeF32Bits; 3],
    /// `BuildTimeInfantryMult=`, `BuildTimeUnitsMult=`, `BuildTimeAircraftMult=`,
    /// `BuildTimeBuildingsMult=` and `BuildTimeDefensesMult=` (HouseType
    /// `+0x134..+0x144`): ReadDouble into floats (`0x00511C70..0x00511CEC`),
    /// the constructor's 1.0 (`0x005114D8..0x005114F0`) as default, no clamp.
    /// No retail country sets them.
    pub build_time_mults: [NativeF32Bits; 5],
    /// `ROF=` (HouseType `+0xE8`; constructor 1.0 at `0x00511457`, ReadDouble
    /// at `0x00511A0C`). `HouseClass::SetDifficulty` multiplies it into the
    /// house's ROF bias outside campaigns. No retail country sets it.
    pub rof: f64,
    /// The HouseType scalar doubles at +C8/+D0/+D8/+E0/+F0/+F8. The
    /// constructor writes 1.0 and511850 reads with the current field as its
    /// default. SetDifficulty uses them only outside GameMode zero.
    pub firepower: f64,
    pub ground_speed: f64,
    pub air_speed: f64,
    pub armor: f64,
    pub cost: f64,
    pub build_time: f64,
    /// `UIName=` — the country's string-table key (e.g. `Name:Americans`).
    /// gamemd fills a house's stored display name from this key's localized text,
    /// which is what the end-of-match score screen shows in the Player column.
    pub ui_name: Option<String>,
    /// `Name=` — the country's plain English name, the fallback when `UIName=`
    /// is absent or its key does not resolve.
    pub name: Option<String>,
    /// Ares country presentation state (flag/status/taunt/order/weights and
    /// observer art). GUI/process state, not a simulation hash input.
    #[cfg(feature = "ares")]
    pub(crate) presentation: crate::rules::ares_country_presentation::AresCountryPresentation,
    /// Ares country `ParaDrop.*` / `Parachute.Anim=` overrides. Simulation
    /// state: which units a paradrop superweapon sends.
    #[cfg(feature = "ares")]
    pub(crate) paradrop: crate::rules::ares_country_paradrop::AresCountryParadrop,
    /// Ares country `AI.PowerPlants=` candidates. Simulation state: which
    /// building the AI splices when its base runs short of power.
    #[cfg(feature = "ares")]
    pub(crate) power_plants: Vec<String>,
}

/// The House factors [`RuleSet::cost_of`] multiplies in, each indexed by
/// [`ObjectType::factor_slot`]: the country's `Cost*Mult=` and the
/// House's FactoryPlant product (House `+0x5390..+0x53A0`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HouseCostFactors {
    pub country: [NativeF32Bits; 5],
    pub factory_plant: [NativeF32Bits; 5],
}

impl HouseCostFactors {
    /// `TechnoTypeClass::Cost_Of @ 0x00711F00` past its null-House arm:
    /// `ftol(cost * plant * country)` for factor `slot`
    /// (`0x00711F35..0x00711F41`).
    pub(crate) fn adjust(&self, cost: i32, slot: usize) -> i32 {
        use crate::util::native_x87::MaskedX87Chop53 as X87;
        X87::ftol_i32_low_masked(X87::mul(
            X87::mul(X87::load_i32(cost), X87::load_f32(self.factory_plant[slot])),
            X87::load_f32(self.country[slot]),
        ))
    }
}

impl Default for CountryRules {
    fn default() -> Self {
        // The native multiplier defaults are neutral1.0, never derived zeros.
        Self {
            veteran_types: CountryVeterancyState::default(),
            color_scheme: 0,
            parent_country: String::new(),
            multiplay: false,
            multiplay_passive: false,
            wall_owner: true,
            #[cfg(feature = "phobos")]
            attach_effect_attach_types: Vec::new(),
            #[cfg(feature = "phobos")]
            attach_effect_duration_overrides: Vec::new(),
            #[cfg(feature = "phobos")]
            attach_effect_delays: Vec::new(),
            #[cfg(feature = "phobos")]
            attach_effect_initial_delays: Vec::new(),
            #[cfg(feature = "phobos")]
            attach_effect_recreation_delays: Vec::new(),
            #[cfg(feature = "phobos")]
            attach_effect_attach_on_owner_change: None,
            income_mult: NativeF32Bits::ONE,
            armor_infantry_mult: 1.0,
            armor_units_mult: 1.0,
            armor_aircraft_mult: 1.0,
            armor_buildings_mult: 1.0,
            armor_defenses_mult: 1.0,
            cost_mults: [NativeF32Bits::ONE; 5],
            speed_mults: [NativeF32Bits::ONE; 3],
            build_time_mults: [NativeF32Bits::ONE; 5],
            rof: 1.0,
            firepower: 1.0,
            ground_speed: 1.0,
            air_speed: 1.0,
            armor: 1.0,
            cost: 1.0,
            build_time: 1.0,
            ui_name: None,
            name: None,
            #[cfg(feature = "ares")]
            presentation: crate::rules::ares_country_presentation::AresCountryPresentation::default(
            ),
            #[cfg(feature = "ares")]
            paradrop: crate::rules::ares_country_paradrop::AresCountryParadrop::default(),
            #[cfg(feature = "ares")]
            power_plants: Vec::new(),
        }
    }
}

impl CountryRules {
    fn from_ini_section(section: &crate::rules::ini_parser::IniSection) -> Self {
        Self {
            // Filled from the typed Process receipt; neither veteran lists
            // nor earlier Color strings are re-resolved against final catalogs.
            veteran_types: CountryVeterancyState::default(),
            color_scheme: 0,
            parent_country: String::new(),
            multiplay: section.read_bool("Multiplay", Self::default().multiplay),
            multiplay_passive: section.read_bool("MultiplayPassive", false),
            wall_owner: section.read_bool("WallOwner", true),
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
            attach_effect_attach_on_owner_change: section.read_phobos_nullable_bool(
                "AttachEffect.AttachOnOwnerChange",
                Self::default().attach_effect_attach_on_owner_change,
            ),
            // IncomeMult is a raw multiplier (NOT a percent). Round in f64 to avoid f32
            // drift; absent -> the neutral 1.0 (stock).
            // HouseTypeClass::ReadINI stores these as floats (`FSTP dword` at
            // 0x00511ADD..0x00511B59 and 0x00511D0B).
            income_mult: section.read_double_to_float("IncomeMult", NativeF32Bits::ONE),
            armor_infantry_mult: section.read_float("ArmorInfantryMult", 1.0),
            armor_units_mult: section.read_float("ArmorUnitsMult", 1.0),
            armor_aircraft_mult: section.read_float("ArmorAircraftMult", 1.0),
            armor_buildings_mult: section.read_float("ArmorBuildingsMult", 1.0),
            armor_defenses_mult: section.read_float("ArmorDefensesMult", 1.0),
            cost_mults: [
                "CostInfantryMult",
                "CostUnitsMult",
                "CostAircraftMult",
                "CostBuildingsMult",
                "CostDefensesMult",
            ]
            .map(|key| section.read_double_to_float(key, NativeF32Bits::ONE)),
            speed_mults: ["SpeedInfantryMult", "SpeedUnitsMult", "SpeedAircraftMult"]
                .map(|key| section.read_double_to_float(key, NativeF32Bits::ONE)),
            build_time_mults: [
                "BuildTimeInfantryMult",
                "BuildTimeUnitsMult",
                "BuildTimeAircraftMult",
                "BuildTimeBuildingsMult",
                "BuildTimeDefensesMult",
            ]
            .map(|key| section.read_double_to_float(key, NativeF32Bits::ONE)),
            firepower: section.read_double("Firepower", 1.0),
            ground_speed: section.read_double("Groundspeed", 1.0),
            air_speed: section.read_double("Airspeed", 1.0),
            armor: section.read_double("Armor", 1.0),
            rof: section.read_double("ROF", 1.0),
            cost: section.read_double("Cost", 1.0),
            build_time: section.read_double("BuildTime", 1.0),
            // AbstractTypeClass::ReadINI: `Name` into 0x31 bytes (0x00410AA0),
            // `UIName` into 0x20 (0x00410AFB).
            ui_name: section.read_name("UIName", 0x20).map(str::to_owned),
            name: section.read_name("Name", 0x31).map(str::to_owned),
            #[cfg(feature = "ares")]
            presentation: crate::rules::ares_country_presentation::AresCountryPresentation::default(
            ),
            #[cfg(feature = "ares")]
            paradrop: crate::rules::ares_country_paradrop::AresCountryParadrop::default(),
            #[cfg(feature = "ares")]
            power_plants: Vec::new(),
        }
    }
}

/// Stable source-order identity in the `[Countries]` registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct CountryIdx(pub u16);

/// Which of the four `[Side]` score themes a score dialog asks for. The two
/// sites choose on data gamemd already holds: the campaign dialog compares the
/// elapsed and par times at `0x6C9231` (`ScoreDialog_Handle_ScoreThemeA`
/// `0x1002F920`, patch site `0x6C922C`, `+0x178` when elapsed is at or under
/// par, else `+0x198`), and the skirmish dialog reads the local player's
/// `HouseClass+0x1F5` defeated flag (`Global_DrawScoreScreen_ScoreTheme`
/// `0x1002F4B0`, patch site `0x5C9B75`, `+0x1B8` when not defeated, else
/// `+0x1D8`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScoreTheme {
    Campaign { over_par: bool },
    Multiplayer { defeated: bool },
}

/// The insignia gamemd's stock selection chain has no entry for: both
/// `HouseTypeExt::Initialize`'s unknown-country branch (`0x1001C413`) and
/// `Game_GetFlagSurface`'s null-surface branch (`0x4E3567`) answer with it.
pub(crate) const STOCK_UNKNOWN_INSIGNIA: &str = "rani.pcx";

/// The `[Side]` score-screen file names one match resolves through
/// `Game_LoadCampaignScoreAssets` (`0x1002F3A0`, patch site `0x72D300`),
/// `Game_LoadMultiplayerScoreAssets` (`0x1002F430`, `0x72D730`) and
/// `Game_GetMultiplayerScoreScreenBar` (`0x1002F300`, `0x5CA110`). Each name is
/// substituted on its own; `None` keeps that name's fixed stock table entry.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SideScoreArt<'a> {
    pub campaign_background: Option<&'a str>,
    pub campaign_transition: Option<&'a str>,
    pub campaign_animation: Option<&'a str>,
    pub campaign_palette: Option<&'a str>,
    pub multiplayer_background: Option<&'a str>,
    /// Pattern whose first `~~` receives the two-digit band number (band + 1).
    pub multiplayer_bars: Option<&'a str>,
    pub multiplayer_palette: Option<&'a str>,
}

/// Stable source-order identity in the `[Sides]` registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SideIdx(pub u8);

/// Registry section names in rules.ini and their corresponding category.
const TYPE_REGISTRIES: &[(&str, ObjectCategory)] = &[
    ("InfantryTypes", ObjectCategory::Infantry),
    ("VehicleTypes", ObjectCategory::Vehicle),
    ("AircraftTypes", ObjectCategory::Aircraft),
    ("BuildingTypes", ObjectCategory::Building),
];

/// The `[General]` keys `TechnoClass::Time_To_Build @ 0x006F47A0` reads,
/// stored as the RulesClass fields hold them. The reader (`0x0066D530`) reads
/// each with `INIClass::ReadDouble @ 0x005283D0`, the field's current value as
/// default and no clamp; the values below are the constructor's
/// (`RulesClass @ 0x00665650`).
#[derive(Debug, Clone, Copy)]
pub struct ProductionRules {
    /// `BuildSpeed=`: minutes to build a 1000-credit object (`+0x1748`, double;
    /// 1.0).
    pub build_speed: NativeF64Bits,
    /// `MultipleFactory=`: the time factor per extra factory (`+0x57C`, float;
    /// 1.0).
    pub multiple_factory: NativeF32Bits,
    /// `LowPowerPenaltyModifier=` (`+0x578`, float; 1.0).
    pub low_power_penalty_modifier: NativeF32Bits,
    /// `MinLowPowerProductionSpeed=` (`+0x570`, float; 0.5).
    pub min_low_power_production_speed: NativeF32Bits,
    /// `MaxLowPowerProductionSpeed=` (`+0x574`, float; 0.9).
    pub max_low_power_production_speed: NativeF32Bits,
    /// `WallBuildSpeedCoefficient=`: a wall's time factor (`+0x758`, double;
    /// 0.5).
    pub wall_build_speed_coefficient: NativeF64Bits,
}

impl Default for ProductionRules {
    fn default() -> Self {
        Self {
            build_speed: NativeF64Bits::ONE,
            multiple_factory: NativeF32Bits::ONE,
            low_power_penalty_modifier: NativeF32Bits::ONE,
            min_low_power_production_speed: NativeF32Bits::from_bits(0.5_f32.to_bits()),
            max_low_power_production_speed: NativeF32Bits::from_bits(0x3F66_6666),
            wall_build_speed_coefficient: NativeF64Bits::HALF,
        }
    }
}

/// A `[General]` animation reference parsed from rules.ini + art.ini.
///
/// The name comes from rules.ini `[General]` (e.g., WarpIn=WARPIN).
/// The rate comes from the anim's own art.ini section (e.g., `[WARPIN]` Rate=120).
///
/// Westwood INI treats `;` as a comment marker, so `WarpOut=WARPOUT;WAKE2`
/// reads as `WARPOUT` — the `;WAKE2` portion is a comment, NOT a secondary
/// anim. The retail engine behaves the same way. A 2026-05-20 trace claimed
/// otherwise (a "primary;secondary" delimiter) but the claim was based on a
/// doc misinterpretation; no Ghidra evidence supports a secondary-anim parser.
#[derive(Debug, Clone)]
pub struct AnimRef {
    /// SHP animation name (uppercase), e.g., "WARPIN".
    pub name: String,
}

/// `ftol(minutes * 900.0)` under the process's 53-bit chop control word: the
/// frames of a rules value authored in minutes (the 900.0 is the double at
/// `0x007E27F8`).
pub(crate) fn native_minutes_to_frames(minutes: f64) -> i32 {
    use crate::util::native_x87::{MaskedX87Chop53 as X87, NativeF64Bits};
    X87::ftol_i32_low_masked(X87::mul(
        X87::load_f64(NativeF64Bits::from_bits(minutes.to_bits())),
        X87::load_i32(900),
    ))
}

/// Retained General spotlight inputs, Rules+788..7A8. Constructor6660F3..66613F;
/// ReadGeneral671748..671816 uses current defaults, exact-case native readers,
/// and no clamp. Doubles retain the native parser's binary32 promotion bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct SpotlightRules {
    pub movement_radius: i32,
    pub location_radius: i32,
    pub speed: crate::util::native_x87::NativeF64Bits,
    pub acceleration: crate::util::native_x87::NativeF64Bits,
    pub angle: crate::util::native_x87::NativeF64Bits,
    pub radius: i32,
}
impl Default for SpotlightRules {
    fn default() -> Self {
        use crate::util::native_x87::NativeF64Bits as D;
        Self {
            movement_radius: 2000,
            location_radius: 1000,
            speed: D::from_bits(0x3fa999999999999a),
            acceleration: D::from_bits(0x3f747ae147ae147b),
            angle: D::from_bits(0x4034000000000000),
            radius: 175,
        }
    }
}
impl SpotlightRules {
    pub(crate) fn read_pass(&mut self, section: &IniSection) {
        self.movement_radius = section.read_int("SpotlightMovementRadius", self.movement_radius);
        self.location_radius = section.read_int("SpotlightLocationRadius", self.location_radius);
        self.speed = section.read_double_bits("SpotlightSpeed", self.speed);
        self.acceleration = section.read_double_bits("SpotlightAcceleration", self.acceleration);
        self.angle = section.read_double_bits("SpotlightAngle", self.angle);
        self.radius = section.read_int("SpotlightRadius", self.radius);
    }
}

/// `[General] AIIonCannon*Value=`: `RulesClass::ReadGeneral` reads each
/// through the IntVector reader `0x00475D70` with the field as its default
/// (`0x00670801..0x00670AE6`, lists at `Rules+0x1194` step `0x1C`, in field
/// order); the constructor leaves them empty. `HouseClass::
/// AI_FindBestRallyTarget @ 0x0050CBF0` values an enemy object by its kind's
/// list, indexed by the firing house's difficulty (`sim::superweapon::ai_fire`).
/// Retail comments out the Plug, Helipad and Temple keys.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AiIonCannonValues {
    pub con_yard: Vec<i32>,
    pub war_factory: Vec<i32>,
    pub power: Vec<i32>,
    pub tech_center: Vec<i32>,
    pub engineer: Vec<i32>,
    pub thief: Vec<i32>,
    pub harvester: Vec<i32>,
    pub mcv: Vec<i32>,
    pub apc: Vec<i32>,
    pub base_defense: Vec<i32>,
    pub plug: Vec<i32>,
    pub helipad: Vec<i32>,
    pub temple: Vec<i32>,
}

impl AiIonCannonValues {
    fn read(general: &IniSection) -> Self {
        let list = |key: &str| general.read_int_list(key).unwrap_or_default();
        Self {
            con_yard: list("AIIonCannonConYardValue"),
            war_factory: list("AIIonCannonWarFactoryValue"),
            power: list("AIIonCannonPowerValue"),
            tech_center: list("AIIonCannonTechCenterValue"),
            engineer: list("AIIonCannonEngineerValue"),
            thief: list("AIIonCannonThiefValue"),
            harvester: list("AIIonCannonHarvesterValue"),
            mcv: list("AIIonCannonMCVValue"),
            apc: list("AIIonCannonAPCValue"),
            base_defense: list("AIIonCannonBaseDefenseValue"),
            plug: list("AIIonCannonPlugValue"),
            helipad: list("AIIonCannonHelipadValue"),
            temple: list("AIIonCannonTempleValue"),
        }
    }
}

/// Immutable HouseType scalar inputs to SetDifficulty4F6EC0. This projection
/// keeps name resolution with RuleSet; it is not another mutable country table.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CountryDifficultyBiases {
    pub firepower: NativeF64Bits,
    pub ground_speed: NativeF64Bits,
    pub air_speed: NativeF64Bits,
    pub armor: NativeF64Bits,
    pub rof: NativeF64Bits,
    pub cost: NativeF64Bits,
    pub build_time: NativeF64Bits,
}

impl Default for CountryDifficultyBiases {
    fn default() -> Self {
        Self {
            firepower: NativeF64Bits::ONE,
            ground_speed: NativeF64Bits::ONE,
            air_speed: NativeF64Bits::ONE,
            armor: NativeF64Bits::ONE,
            rof: NativeF64Bits::ONE,
            cost: NativeF64Bits::ONE,
            build_time: NativeF64Bits::ONE,
        }
    }
}

/// Global gameplay constants from `[General]` that affect vision, gap generators, etc.
#[derive(Debug, Clone)]
pub struct GeneralRules {
    /// `[General] MaxWaypointPathLength`, Rules+90. Constructor665725
    /// writes15; ReadGeneral671DB2..671DCC passes the prior signed value to
    /// ReadInteger5276D0 and stores it unchanged, without a clamp.
    pub max_waypoint_path_length: i32,
    /// General.TalkBubbleTime, Rules+1800; ctor6677A3=300 wall-clock buckets.
    pub talk_bubble_time: i32,
    /// `[General] FlashFrameTime=`, Rules `+0x88`: the constructor's 7
    /// (`0x00665711`), read by ReadInteger at `0x00671981` without a clamp.
    /// The radar combat flash's toggle period.
    pub flash_frame_time: i32,
    /// `[General] RadarCombatFlashTime=`, Rules `+0x8C`: the constructor's 21
    /// (`0x0066571B`), read by ReadInteger at `0x006719A1` without a clamp.
    /// ReceiveDamage starts each Techno's radar combat flash with it.
    pub radar_combat_flash_time: i32,
    /// `[AudioVisual]` thresholds copied to the presentation frame-rate
    /// controller by Fill_In_Data6850F5 and the radar movie transitions.
    pub detail: DetailRules,
    /// `[AudioVisual] PoseDir=`, Rules `+0x44`: the constructor's 0
    /// (`0x006656B9`), then ReadInteger (`0x00669268`) stored raw, without
    /// DeployDir's shift. An aircraft with no radio contact and no passengers
    /// lands facing it (`AircraftClass::Landing_Direction @ 0x0041B760`,
    /// which Fly shifts left by 13).
    pub pose_dir: i32,
    /// Rules+48, constructor6656BD zero; AudioVisual669272..66929B reads
    /// DeployDir with ReadInteger5276D0 then wrapping SHL5, after PoseDir.
    /// Jumpjet54C765 takes the low byte; retain the raw signed stored dword.
    pub deploy_dir: i32,
    /// Edge-scroll speed scale from `[AudioVisual] ScrollMultiplier=`.
    /// Stock YR uses `.07`; this is app-facing presentation state.
    pub scroll_multiplier: f64,
    /// Outcome-announcement grace period in minutes from `[AudioVisual]
    /// SavourDelay=`. HouseClass converts this to frames with `ftol(value*900)`
    /// before routing victory/defeat into scenario teardown.
    pub savour_delay_minutes: f64,
    /// Per-tick Spark gravity AND hover-bob amplitude, from `[AudioVisual]
    /// Gravity=` (NOT `[General]` — stock rulesmd.ini defines it under
    /// [AudioVisual], value 6; the engine's code default is 3). Native stores a
    /// signed integer and converts to f32 at the behavior-3 tick boundary.
    /// Also the ballistic gravity: `WeaponTypeClass::GetSpeed @ 0x00773070`
    /// derives every `ROT=0` launch speed from it.
    pub gravity: i32,
    /// Rules+7AC, ReadGeneral66F2AF: full signed wind-table index.
    wind_direction: i32,
    /// `[General] VeteranSight=` — `RulesClass+0x680`, a double.
    ///
    /// gamemd-derived: `TechnoClass::UpdateReveal @ 0x0070AF50` MULTIPLIES the
    /// elevation-scaled sight by it (`0x0070B095..0x0070B09F`, `FILD; FMUL;
    /// ftol`), only for a `SIGHT`-ability holder, and only when the value is
    /// not exactly `0.0` (`FCOMP` gate at `0x0070B088`). Stock `0.0` therefore
    /// disables the bonus. The constructor default is UNCHECKED.
    pub veteran_sight: f64,
    /// `[General] VeteranCombat=` — `RulesClass+0x670`. `TechnoClass::Fire_At`
    /// multiplies the folded firepower damage by it at `0x006FE3C8..0x006FE3D8`
    /// for a `FIREPOWER`-ability holder. Constructor default UNCHECKED.
    pub veteran_combat: f64,
    /// `[General] VeteranSpeed=` — `RulesClass+0x678`.
    /// `FootClass::GetCurrentSpeed @ 0x004DB1A0` multiplies the truncated type
    /// speed by it at `0x004DB1F1..0x004DB200` for a `FASTER`-ability holder.
    /// Constructor default UNCHECKED.
    pub veteran_speed: f64,
    /// Phobos `[General] ProneSpeed.Crawls=` (default 0.67): the multiplier
    /// `InfantryClass::GetMovementSpeed @ 0x00521D80` applies to a prone
    /// infantryman whose type has `Crawls=`.
    #[cfg(feature = "phobos")]
    pub prone_speed_crawls: f64,
    /// Phobos `[General] ProneSpeed.NoCrawls=` (default 1.5): the same
    /// multiplier for a prone type without `Crawls=`.
    #[cfg(feature = "phobos")]
    pub prone_speed_no_crawls: f64,
    /// `[General] VeteranROF=` — `RulesClass+0x690`, read at `0x0066EF61`.
    /// `TechnoClass::GetROF @ 0x006FCFA0` multiplies the jittered reload by it
    /// at `0x006FD136..0x006FD145` for a `ROF`-ability holder. Constructor
    /// default 1.0 (`0x00665F86`).
    pub veteran_rof: f64,
    /// Rules+1538, stride50: Easy, Normal, Difficult in native index order.
    /// ReadDifficulty66D270 skips an absent whole section and resets every
    /// missing key of a present section to a literal default. The native
    /// constructor leaves absent whole rows undefined; VERA's cold fallback
    /// uses the reader defaults. Retail supplies all three rows.
    pub difficulty_rows: [DifficultyRules; 3],
    /// Rules+1418: [General] GameSpeedBias, read at670B18/670B33. This scales
    /// the stored Groundspeed/Airspeed/BuildTime scalar biases; their native
    /// active gameplay getters use separate per-category country factors.
    pub game_speed_bias: f64,
    /// Receiver-side divisor selected by the rank-specific `STRONGER`
    /// ability (`VeteranArmor=` in `[General]`).
    pub veteran_armor: f64,
    /// `[General] CurleyShuffle=` — `RulesClass+0x17E1`, read at `0x0066FD4A`
    /// (ReadBool with the field as its default; the constructor writes 0).
    /// `AircraftClass::Mission_Attack` reads it in states 4 and 5
    /// (`0x004183C3`, `0x00418671`, `0x00418733`, `0x00418782`): a helicopter
    /// that fired or missed goes back to state 1 and picks a new firing spot.
    /// Retail: yes.
    pub curley_shuffle: bool,
    /// `[General] EnableAIBuildLimitation=` — the older extension build's
    /// switch (NPExt ext173, manual `AI克隆生产`'s predecessor): with yes, the
    /// computer's production attempt checks the type's own `BuildLimit=`
    /// (`Type+0x3B8`) like a production event does, and a plan at its limit
    /// clears its choice and skips. Read by that build's `ReadGeneral` hook
    /// `0xC2D132..0xC2D151` into `Rules+0x641`, default the current field, so
    /// the constructor clears that byte through the +640 DWORD store
    /// (`0x665F09`), so absent means no. NPSE #359 dropped this key and
    /// reads per-type AIBuildLimit (`0xC27249..0xC2725A`) instead. Both
    /// generations use factory_ai's shared tracked/produced comparison.
    pub enable_ai_build_limitation: bool,
    /// `[General] DisableParallelAIQueues=` — an NPSE 2.0 extension switch
    /// read into `Rules+0x2D` by `ReadGeneral` C30303..C3031A through
    /// ReadBooleanByte5295F0 with the current byte as default. The Rules
    /// constructor's DWORD store at 66569B clears +2D, so absent means no.
    /// With yes, Factory_AI C2F07C reserves by produced class, splitting
    /// vehicles alone on Naval. HouseAiProduction owns the reservation and
    /// its class-specific exit/abandon release phases. Default no keeps cloning.
    pub disable_parallel_ai_queues: bool,
    /// `[General] RepairRate=` in minutes — `RulesClass+0x16E0`.
    ///
    /// The self-heal pulse (`FUN_0070BE80`) divides the frame counter by
    /// `ftol(RepairRate * 900)` (`0x0070BEFE..0x0070BF0A`, constant 900.0 at
    /// `0x007E27F8`); stock `.016` gives a 14-frame pulse.
    pub repair_rate_minutes: f64,
    /// `[General] SelfHealInfantryFrames=` — `RulesClass+0x30`, read by the
    /// General reader at `0x0066D530`. The house pulse of
    /// `TechnoClass::AI_Update @ 0x006FA8E2` divides the global frame counter
    /// by it (`idiv`) and runs only on an exact remainder of zero.
    /// Ctor6656A7 defaults1000; ReadInt66E6F3 retains it. Stock50.
    pub self_heal_infantry_frames: i32,
    /// `[General] SelfHealInfantryAmount=` — `RulesClass+0x34`, read by the
    /// same reader. `HouseClass::GetInfSelfHealStep @ 0x0050D9E0` multiplies it
    /// by the house's infantry count (`+0x164`). Ctor6656AA defaults1;
    /// ReadInt66E70D retains it. Stock20.
    pub self_heal_infantry_amount: i32,
    /// `[General] SelfHealUnitFrames=` — `RulesClass+0x38`, same reader. The
    /// unit pulse of `TechnoClass::AI_Update @ 0x006FA7EE` divides the frame
    /// counter by it. Ctor6656AD defaults1000; ReadInt66E727 retains it. Stock75.
    pub self_heal_unit_frames: i32,
    /// `[General] SelfHealUnitAmount=` — `RulesClass+0x3C`, same reader.
    /// `HouseClass::GetUnitSelfHealStep @ 0x0050D9F0` multiplies it by the
    /// house's unit count (`+0x168`). Ctor6656B0 defaults1;
    /// ReadInt66E740 retains it. Stock5.
    pub self_heal_unit_amount: i32,
    /// `[General] VeteranRatio=` — how many times its own cost an object must
    /// destroy to gain one rank. `RulesClass+0x668`, read at `0x0066EEB0`.
    pub veteran_ratio: f64,
    /// `[General] VeteranCap=` — the accumulator clamp. `RulesClass+0x698`,
    /// read at `0x0066EF94`. Stock `2`, which is exactly the elite threshold,
    /// so elite is terminal.
    pub veteran_cap: f64,
    /// `[AI] ComputerBaseDefenseResponse=` (ReadAI673E41..673E61).
    /// The active House responder forms its signed/wrapping budget as
    /// `attacker ThreatPosed * this value` (7080BA..7080C4).
    pub computer_base_defense_response: i32,
    /// Signed `[General] MaximumBuildingPlacementFailures=` at `Rules+0xE48`.
    /// Native constructor default is `5`; both active retail rules files
    /// override it with `3`. Building exit compares strictly after incrementing;
    /// negative mod values remain literal.
    pub maximum_building_placement_failures: i32,
    /// `[General] PlacementDelay=` in minutes (`Rules+0x5F8`, ReadDouble at
    /// `0x0066F208`, constructor .05 at `0x00665E8C`): how long a computer
    /// Construction Yard waits after a blocked placement before it tries its
    /// finished building again ([`Self::placement_delay_frames`]).
    pub placement_delay: f64,
    /// `[AI] AttackDelay=`, Rules+0x10A8. The constructor supplies 5.0;
    /// ReadAI673977..67399E reads a double with that current-field default.
    /// House construction scales its one Scenario ranged draw by this value.
    pub attack_delay: f64,
    /// `[General] CampaignMoneyDeltaEasy/Hard`, ReadInteger at
    /// 0x0067015B/0x0067017A with current defaults 5000/-5000. Campaign
    /// House credits apply the selected signed delta after Credits*100.
    pub campaign_money_delta_easy: i32,
    pub campaign_money_delta_hard: i32,
    /// `[General] AIAlternateProductionCreditCutoff=` (`Rules+0x1300`,
    /// ReadInt at `0x0066FDFB`, constructor 1000 at `0x0066703B`): the credits
    /// below which the computer's production mode leaves its normal state.
    pub ai_alternate_production_credit_cutoff: i32,
    /// `[General] AIRestrictReplaceTime=` (`Rules+0xDF0`, ReadInt at
    /// `0x00670110`, constructor 500 at `0x006668D4`; retail 400): the frames
    /// after a building of the house is attacked during which the computer
    /// replans a lost building only if it is armed, a wall or a power plant.
    pub ai_restrict_replace_time: i32,
    /// `[General] TeamDelays=` (`Rules+0x1158`, items `+0x115C`; the
    /// `0x00475D70` vector read at `0x0066FD8B`, constructor empty): the frames
    /// between a computer house's team creation passes, by House difficulty
    /// (`sim::ai_team_creation`). Retail: 2000,2500,3500.
    pub team_delays: Vec<i32>,
    /// `[General] TotalAITeamCap=` (`Rules+0x13C8`, items `+0x13CC`, read at
    /// `0x0066FF30`): the teams a computer house may own before it stops
    /// creating more. Retail: 30,30,30.
    pub total_ai_team_cap: Vec<i32>,
    /// `[General] MinimumAIDefensiveTeams=` (`Rules+0x1390`, items `+0x1394`,
    /// read at `0x0066FEBD`). Retail: 1,1,1.
    pub minimum_ai_defensive_teams: Vec<i32>,
    /// `[General] MaximumAIDefensiveTeams=` (`Rules+0x13AC`, items `+0x13B0`,
    /// read at `0x0066FEF6`). Retail: 2,2,2.
    pub maximum_ai_defensive_teams: Vec<i32>,
    /// `[General] UseMinDefenseRule=` (`Rules+0x17F3`, GetBool at
    /// `0x0066FF60`, constructor 1): while a house owns fewer base-defense
    /// teams than the minimum, only base-defense AI triggers qualify.
    pub use_min_defense_rule: bool,
    /// `[General] FillEarliestTeamProbability=` (`Rules+0x13F0`, items
    /// `+0x13F4`, read at `0x0066FE83`): the percent chance a unit chooser
    /// fills the earliest forming team's need. Retail: 100,100,100.
    pub fill_earliest_team_probability: Vec<i32>,
    /// `[General] DissolveUnfilledTeamDelay=` (`Rules+0x1190`, ReadInt at
    /// `0x0066FF80`, constructor 5000): the frames an empty team lasts in a
    /// multiplayer game (`TeamClass::AI @ 0x006E9140`).
    pub dissolve_unfilled_team_delay: i32,
    /// `[General] AISafeDistance=` (`Rules+0xD74`, ReadInteger at
    /// `0x0066FFA7`, constructor 8 at `0x0066682A`): the cells from its base
    /// centre a team gathers at on script action 54 (`0x006EFB69`).
    /// Retail: 20.
    pub ai_safe_distance: i32,
    /// `[AI] PatrolScan=` (`Rules+0x1150`, ReadDouble6739BA inside
    /// ReadAI672AE0). Constructor666E09/666E1A stores the literal double
    /// 0.016; each reached AI pass retains its prior value as the default.
    /// Team Script16 uses the signed global frame modulo ftol(value*900),
    /// without a clamp or a per-team timer. Source: Steam gamemd,
    /// tools/team_recruit_oracle.py --patrol.
    pub patrol_scan_minutes: NativeF64Bits,
    /// `[General] AITriggerSuccessWeightDelta=` (`Rules+0xC0`, ReadDouble at
    /// `0x006718EC`, constructor 1.0; retail 20).
    pub ai_trigger_success_weight_delta: NativeF64Bits,
    /// `[General] AITriggerFailureWeightDelta=` (`Rules+0xC8`, ReadDouble at
    /// `0x00671913`, constructor -1.0; retail -50).
    pub ai_trigger_failure_weight_delta: NativeF64Bits,
    /// `[General] AITriggerTrackRecordCoefficient=` (`Rules+0xD0`, ReadDouble
    /// at `0x0067193A`, constructor 1.0; retail 1).
    pub ai_trigger_track_record_coefficient: NativeF64Bits,
    /// `[General] HarvestersPerRefinery=` (`Rules+0x1358`, items `+0x135C`,
    /// read at `0x006705D3`): the harvesters a computer house wants for each
    /// refinery. Retail: 2,2,1.
    pub harvesters_per_refinery: Vec<i32>,
    /// `[General] AIMinorSuperReadyPercent=` (`Rules+0xD70`, a float read
    /// through ReadDouble at `0x0066FFC4`, constructor 0.8f): how charged an
    /// Iron Curtain or Chronosphere must be for an AI trigger to count it.
    pub ai_minor_super_ready_percent: NativeF32Bits,
    /// `[General] BaseDefenseDelay=` in minutes. A strict responder-budget
    /// overshoot arms the attacker cooldown for `ftol(value * 900)` frames.
    pub base_defense_delay_minutes: f64,
    /// `[General] SuspendPriority=`. Teams owned by the attacked House whose
    /// signed priority is lower than this value are suspended before scanning.
    pub suspend_priority: i32,
    /// `[General] SuspendDelay=` in minutes. Suspended TeamClass instances arm
    /// their native timer for `ftol(value * 900)` frames.
    pub suspend_delay_minutes: f64,
    /// Leptons of elevation per +1 sight cell (LeptonsPerSightIncrease=).
    /// 256 leptons = 1 z-level in RA2. 0 disables the elevation bonus.
    pub leptons_per_sight_increase: i32,
    /// Gap Generator effect radius in cells (GapRadius=). Default 10.
    pub gap_radius: i32,
    /// Height-based LOS obstruction (RevealByHeight= in [General]).
    /// When true, terrain 4+ levels above the viewer at the midpoint blocks sight.
    /// Default true (the standard RA2/YR setting).
    pub reveal_by_height: bool,
    /// `[AudioVisual] AllyReveal=` (`Rules+0x17E7`, ReadBool at `0x0066B318`
    /// over the constructor's 1, `0x0066773F`): an allied house's reveals map
    /// the player's cells too (the house gate of `MapClass::RevealArea @
    /// 0x005678E0`, `0x00567AEF`). Retail yes.
    pub ally_reveal: bool,
    /// `[MultiplayerDialogSettings] AlliesAllowed=` (`Rules+0x14B5`, ReadBool
    /// at `0x0067215B` in `0x00671EA0` over the constructor's 0,
    /// `0x006672D5`): Computer_Paranoid's gate (`0x0050164D`,
    /// `sim::house_diplomacy`). Retail RULESMD says no; the Battle mode's
    /// MPBATTLEMD layers yes over it.
    pub allies_allowed: bool,
    /// `[MultiplayerDialogSettings] AllyChangeAllowed=` (`Rules+0x14BB`,
    /// ReadBool at `0x00672175` over the constructor's 1, `0x00667202` /
    /// `0x006672F9`). Only the two ALLY producers read it (`0x00731840`,
    /// `0x007318F0`). Retail RULESMD says yes; Team Game's MPTEAMMD says no.
    pub ally_change_allowed: bool,
    /// `[AI] CompEasyBonus=`: Rules+17E3 starts true in constructor667727.
    /// ReadAI673CFD..673D1D uses ReadBool5295F0 with the retained byte,
    /// after AirstripLimit and before Paranoid, independently of General.
    /// CreateHouses688293..6882B9 decrements positive AI difficulty when
    /// the admitted human vector contains more than one node, observers included.
    pub comp_easy_bonus: bool,
    /// `[AI] Paranoid=` (`Rules+0x17E0`, ReadBool at `0x00673D23..0x00673D43`
    /// in ReadAI over the constructor's 1, `0x00667715`): a defeated computer
    /// house at `MaxIQLevels` (`0x004FC0FF`), and a human house's MakeAlly
    /// (`0x004F9C5F`), run Computer_Paranoid. Retail yes.
    pub paranoid: bool,
    /// Low byte of `CliffBackImpassability=` in `[General]`.
    /// Byte 0 skips the scan; only byte 2 can write Rock. Default 2 in standard YR.
    pub cliff_back_impassability: u8,
    /// Underground travel speed for Tunnel locomotor units (TunnelSpeed=).
    /// Default 6.0 cells/second matching RA2 default.
    pub tunnel_speed: SimFixed,
    /// Rules+598: ReadDouble at 66EC37, constructor .25 at 665DBD.
    /// Bullet guidance uses sin((signed native ID + frame) % 15 * 2pi/15).
    pub missile_rot_var: f64,
    /// Rules+5A0: ReadInt at 66EC57, constructor 500 at 665DC9.
    /// Targetless guided flight detonates when old Object height reaches it.
    pub safety_altitude: i32,
    /// Rules+1863, AudioVisual66B77D ReadColorRGB. Any nonzero channel
    /// replaces ObjectType ART LineTrailColor on trail construction.
    pub line_trail_color_override: [u8; 3],
    /// Rules+1866, AudioVisual66B7A7 ReadColorRGB. SpawnRadBeam6FD752
    /// copies this RGB for an admitted Temporal weapon's mode1 beam.
    /// Constructor66785E/667864/66786A initializes all three bytes to zero.
    /// Original controls: tools/procedural_drawing_oracle/chrono_beam.md.
    pub chrono_beam_color: [u8; 3],
    /// Default cruise altitude for Fly-locomotor aircraft (FlightLevel= in [General]).
    /// Fallback 500 leptons matches the engine constructor default; retail
    /// rulesmd.ini always supplies its own (1500), so the fallback only fires
    /// for a non-retail INI missing the key. A type's own `FlightLevel=`
    /// overrides it through `ObjectType::flight_level`.
    pub flight_level: i32,
    /// `[General] AttackingAircraftSightRange=` (`Rules+0x18`, ReadInt at
    /// `0x00670F2E` over the constructor's 5, `0x00665682`): the radius
    /// `AircraftClass::Fire_At` reveals around the current player's shooting
    /// aircraft when its Location, a probe near it or its target is shrouded
    /// (`0x0041651D..0x00416595`). Retail 2.
    pub attacking_aircraft_sight_range: i32,
    /// Rules+420, [JumpjetControls] CruiseHeight. Object5F4260 uses this
    /// global threshold; linked Jumpjets instead use their own +2C height.
    pub display_cruise_height: i32,
    /// `[General] HoverHeight=`, Rules+0x5CC: the Hover altitude controller's
    /// cruise height in leptons (0x00513D20). Constructor 120 (0x00665E2B).
    pub hover_height: i32,
    /// `[General] HoverBob=`, Rules+0x5D0 double: the bob period in minutes
    /// (0x00513E1A). Constructor 30.0 (0x00665E35/3B).
    pub hover_bob: NativeF64Bits,
    /// `[General] HoverBoost=`, Rules+0x5D8 double: SpeedMult for two equal
    /// queued path words (0x00516141). Constructor 1.3 (0x00665E45/54).
    pub hover_boost: NativeF64Bits,
    /// `[General] HoverAcceleration=`, Rules+0x5E0 double, minutes: the ramp
    /// step is `1 / (value * 900)` (0x0051617F). Constructor 0.03 (0x00665E63).
    pub hover_acceleration: NativeF64Bits,
    /// `[General] HoverBrake=`, Rules+0x5E8 double, minutes (0x005161B7).
    /// Constructor 0.03 (0x00665E6F).
    pub hover_brake: NativeF64Bits,
    /// `[General] HoverDampen=`, Rules+0x5F0 double (0x00513F21). Constructor
    /// 0.8 (0x00665E7B/81).
    pub hover_dampen: NativeF64Bits,
    /// Descent rate cap for parachuted units, in leptons/tick (signed).
    /// Per gamemd, the rate field accumulates by `-1` per tick and clamps
    /// to this value. Default `-3` matches `[General] ParachuteMaxFallRate=-3`.
    /// Negative = falling.
    pub parachute_max_fall_rate: i32,
    /// ObjectAI5F3FBC's non-parachute cap, Rules+7BC. Constructor66616D
    /// initializes -100; ReadGeneral66F32E..350 reads the current value
    /// as the default for [General] NoParachuteMaxFallRate via ReadInt5276D0.
    pub no_parachute_max_fall_rate: i32,
    /// Paradrop trigger radius in leptons. From `[General] ParadropRadius=`.
    /// Default 1024 (~4 cells). Distance to target at which the carrier aircraft
    /// reveals fog + transitions to the overfly mission.
    pub paradrop_radius: i32,
    /// Parsed `[General] Parachute=` value (uppercased AnimType name, e.g.
    /// "PARACH"), `None` if unset or empty: the canopy the simulation attaches
    /// to a dropped object. Its timing is the AnimType's own art.
    pub parachute_shp: Option<String>,
    /// Parsed `[General] VehicleParachute=` value (uppercased AnimType name),
    /// read only with the explicit NPatch capability.
    /// `None` if unset or empty: the NPSE canopy an airdropped non-infantry
    /// object takes instead of `Parachute=` (`ObjectClass::Paradrop`'s
    /// `0x5F5ADD` hook, cave `0xC2B0E4`..`0xC2FA8A`: infantry keeps
    /// `Parachute=`, anything else takes this when the Rules+0x400 pointer
    /// is set — the NPSE reader stores it there, `0xC31005..0xC31017` — and
    /// `Parachute=` otherwise). Read in the General reader with the 0x80
    /// string capacity; an empty read retains the previous pointer, while
    /// `none` clears it through AnimTypeClass::FindOrAllocate @ 0x428B80.
    pub vehicle_parachute: Option<String>,
    /// `[General] AmerParaDropInf=`/`AmerParaDropNum=` (`+0xC04`/`+0xC20`):
    /// the American Paradrop's planes.
    pub amer_paradrop: ParadropList,
    /// `AllyParaDropInf=`/`AllyParaDropNum=` (`+0xC3C`/`+0xC58`): the Allied
    /// side's Paradrop, and the Spy Plane's plane count.
    pub ally_paradrop: ParadropList,
    /// `SovParaDropInf=`/`SovParaDropNum=` (`+0xC74`/`+0xC90`).
    pub sov_paradrop: ParadropList,
    /// `YuriParaDropInf=`/`YuriParaDropNum=` (`+0xCAC`/`+0xCC8`).
    pub yuri_paradrop: ParadropList,
    /// `AnimToInfantry=` (`+0xCE4`), the InfantryTypes a `MakeInfantry=`
    /// anim's end picks from, from the native processed rules (empty from
    /// the constructor; retail `BRUTE`).
    pub anim_to_infantry: Vec<String>,
    /// Unit types that count as a player's home when no buildings remain.
    /// Parsed from `[General] BaseUnit=`. Stock YR: AMCV, SMCV, PCV.
    pub base_unit_types: Vec<String>,
    /// `[General] MultiplayerAICM=` — `RulesClass+0x1304` int list read by
    /// `RulesClass__ReadGeneral` at `0x00670512..0x0067053B`. Indexed directly
    /// by `HouseClass+0x184` difficulty (0 = Hard, 1 = Normal, 2 = Easy) in
    /// `ScenarioClass__Post_Map_Init @ 0x00686A52..0x00686A5E`; each entry is a
    /// percentage of the AI house's opening balance granted once at match start.
    /// Stock `rulesmd.ini:88` is `400,0,0`. The constructor default is an empty
    /// vector (`0x00667034`), which native would index out of bounds; VERA treats
    /// a missing entry as 0 (no grant).
    pub multiplayer_ai_cm: Vec<i32>,
    /// Aircraft types in native Rules `PadAircraft` order. BuildingType's
    /// virtual value calculation reads the first two entries for the bundled
    /// helipad-cost branch.
    pub pad_aircraft_types: Vec<String>,
    /// `SeparateAircraft=`. False means the first pad building's value includes
    /// the average cost of the first two `PadAircraft` entries.
    pub separate_aircraft: bool,
    /// `[General] PrismType=` (Rules `+0x498`): the BuildingType whose
    /// Mission_Attack forwards its charge instead of firing
    /// (`sim::world::techno_ai::building_missions`).
    pub prism_type: Option<String>,
    /// Ordered [General] DropZoneAnim reference at Rules+147C. Constructor
    /// 66723F clears it;66E5D5 retains an empty read and resolves nonempty names.
    pub drop_zone_anim: Option<String>,
    /// [AudioVisual] DropZoneRadius, stored in leptons at+14BC. Constructor
    /// 6672FF:1024;66B385 uses ReadRange474620 with the retained value.
    pub drop_zone_radius: i32,
    /// `[General] PrerequisiteProcAlternate=` (Rules `+0x400`): the UnitType
    /// whose on-map units also meet a `PROC` prerequisite
    /// (`sim::production::can_build`).
    pub prerequisite_proc_alternate: Option<String>,
    /// `PrismSupportModifier/Max/Delay/Duration=`.
    pub prism_support: PrismSupportRules,
    /// `GDIGateOne=`, `GDIGateTwo=`, `NodGateOne=`, `NodGateTwo=`,
    /// `WallTower=` (Rules `+0x86C..+0x87C`) and the four power plants
    /// (`+0x89C..+0x8A8`).
    pub building_types: GeneralBuildingTypes,
    /// Whether ore cells grow denser over time (TiberiumGrows= in [General]).
    /// Default true. Can be overridden per-map in [SpecialFlags].
    pub tiberium_grows: bool,
    /// Whether rich ore spreads to adjacent empty cells (TiberiumSpreads= in [General]).
    /// Default true. Can be overridden per-map in [SpecialFlags].
    pub tiberium_spreads: bool,
    /// `GrowthRate=` in [General], in minutes. Fallback 2.0 matches the engine
    /// constructor default; retail rulesmd.ini always supplies its own (5), so
    /// the fallback only fires for a non-retail INI missing the key. Parsed for
    /// rules fidelity only: VERA's growth runs on the per-Tiberium `Growth=`
    /// timers and nothing reads this value.
    pub growth_rate_minutes: f32,
    /// Animation played when a unit warps in (WarpIn= in [General]).
    pub warp_in: AnimRef,
    /// Animation played when a unit warps out (WarpOut= in [General]).
    pub warp_out: AnimRef,
    /// Animation for chrono-erasing a unit (WarpAway= in [General]).
    pub warp_away: AnimRef,
    /// Sparkle particles during chrono teleport (ChronoSparkle1= in [General], YR feature).
    pub chrono_sparkle1: AnimRef,
    /// Wake animation spawned behind ships moving on water (Wake= in [General]).
    pub wake: AnimRef,
    /// Multiplayer move-command feedback animation (MoveFlash= in [General]).
    pub move_flash: AnimRef,
    /// `[General] Behind=` (`Rules+0xB8`): the indicator Anim a Techno
    /// behind a building takes (`TechnoClass::AI_Update @ 0x006FA2D3`).
    /// The ordered processor's General read (`0x0066E4F5..0x0066E513`,
    /// AnimType factory `0x00428B80`) publishes it; the constructor leaves it
    /// null (`0x00665765`).
    pub behind: Option<String>,
    /// Fatal infantry animation bindings indexed by Warhead `InfDeath`.
    /// Slots 3/4/6..10 come from `[General]`; slot 5 is the second declared
    /// `[Animations]` entry. Slots 0..2 intentionally have no external anim.
    pub infantry_death_anims: [Option<String>; 11],
    /// Whether the attack cursor appears on a disguised Spy (AttackCursorOnDisguise= in [General]).
    /// Default false (vanilla RA2). When false, a disguised Spy does not show the attack cursor.
    pub attack_cursor_on_disguise: bool,
    /// `[General] DefaultMirageDisguises=` selection pool, in source order.
    pub default_mirage_disguises: Vec<String>,
    /// General AlliedDisguise/SovietDisguise/ThirdDisguise (Rules
    /// `+0xD58/+0xD5C/+0xD60`), retained by the native scalar reader
    /// 66FB94..66FBEB. Infantry GetDisguise522640 uses the current player's
    /// side, independently of the actor's owner; a `CanDisguise=`/
    /// `PermaDisguise=` object of each side starts wearing its side's entry
    /// (`TechnoClass::Init_Managers`, `0x006F423F..0x006F4271`). Stock E1,
    /// E2 and INIT.
    pub(crate) infantry_disguises: [Option<String>; 3],
    /// General SecretInfantry/SecretUnits/SecretBuildings, retained native identities.
    secret_lab_choices: [Vec<String>; 3],
    /// `[General] InfantryBlinkDisguiseTime=` reveal duration in frames.
    pub infantry_blink_disguise_time: i32,
    /// Whether the attack cursor appears on trees/terrain
    /// (`TreeTargeting=` in `[CombatDamage]`).
    /// Default false in vanilla RA2.
    pub tree_targeting: bool,
    /// Signed Terrain Strength fallback. Rules ctor666DF8 stores25;
    /// General671DD2..671DF1 reads TreeStrength (retail200). Native reader
    /// evidence: tools/rules_oracle/bridge_landing_inputs.
    pub tree_strength: i32,
    /// Health ratio threshold below which the bar turns yellow (ConditionYellow= in [AudioVisual]).
    /// Default 0.5 (50%).
    pub condition_yellow: f64,
    /// Health ratio threshold below which the bar turns red (ConditionRed= in [AudioVisual]).
    /// Constructor default0.5 at667568..66756E; retail authors0.25.
    /// Native reader evidence: tools/rules_oracle/bridge_landing_inputs.
    pub condition_red: f64,
    /// `[General] CloakingStages=` — native signed progress divisor. The
    /// constructor and stock rules both use 9.
    pub cloaking_stages: i32,
    /// `[General] CloakDelay=` converted from minutes with native truncation
    /// toward zero (`ftol(minutes * 900)`). Stock `.02` becomes 18 frames.
    /// Rules66716C..667172 seeds0; ReadGeneral670AEB..670B0C retains it.
    pub cloak_delay_frames: i32,
    /// `[AudioVisual] CloakSound=` resolved by the app audio registry when a
    /// cloak transition requests positional playback. Stock YR binds
    /// `NavalUnitEmerge`; the native constructor's invalid Voc index is silence
    /// when the key is absent or cannot resolve.
    pub cloak_sound: Option<String>,
    /// `[AudioVisual] LeaveBioReactorSound=` is RulesClass+0x274, read by
    /// `RulesClass::ReadAudioVisual @ 0x006691E0` and played by the successful
    /// InfantryAbsorb arm of `BuildingClass::Mission_Unload @ 0x0044D880`.
    /// `None` is the constructor's invalid Voc index; the fixed SOUNDMD binder
    /// resolves the name and retains the prior valid index across rules passes.
    pub leave_bio_reactor_sound: Option<String>,
    /// `[AudioVisual] EnterBioReactorSound=` is RulesClass+0x270, read by
    /// `RulesClass::ReadAudioVisual @ 0x006691E0` and played after successful
    /// InfantryAbsorb admission in `InfantryClass::Per_Cell_Process
    /// @ 0x00519630` (`0x0051A298..0x0051A30A`). `None` is the constructor's
    /// invalid Voc index; the fixed SOUNDMD binder resolves the name.
    pub enter_bio_reactor_sound: Option<String>,
    /// Rules+268: ReadAudioVisual66A10B..66A141 resolves the exact-case
    /// EnterGrinderSound through the fixed Voc catalog, retaining the prior
    /// index on absent/empty/unknown values. Constructor default is -1.
    pub enter_grinder_sound: Option<String>,
    /// `[AudioVisual] UpgradeVeteranSound=` — `RulesClass::ReadAudioVisual @
    /// 0x006691E0` resolves it to a Voc index at `RulesClass+0x228`
    /// (`0x00669CFD..0x00669D27`); `TechnoClass::AI_Update` plays that index
    /// positionally on a veteran promotion (`0x006FA124..0x006FA12A`). The
    /// fixed SOUNDMD binder retains the last valid index through reached
    /// passes; absent, empty and unknown values keep it. Constructor66596A=-1.
    pub upgrade_veteran_sound: Option<String>,
    /// `[AudioVisual] UpgradeEliteSound=` — `RulesClass+0x22C`
    /// (`0x00669D2D..0x00669D68`, constructor665970=-1), played on an elite promotion
    /// (`0x006FA0B6..0x006FA0BC`).
    pub upgrade_elite_sound: Option<String>,
    /// `[AudioVisual] SlavesFreeSound=` — `RulesClass::ReadAudioVisual @
    /// 0x006691E0` resolves it to a Voc index at `RulesClass+0x234`
    /// (constructor -1); the slave release plays it at the first freed slave
    /// (`0x006B0C2C..0x006B0C60`). Absent or empty is silence.
    pub slaves_free_sound: Option<String>,
    /// The seven `[AudioVisual]` crate pickup sounds `RulesClass::ReadAudioVisual
    /// @ 0x006691E0` resolves to Voc indexes at `RulesClass+0x1E4..+0x1FC`
    /// (`0x00669746..0x00669903`, each `ReadString` then `VocClass::FindIndex`,
    /// keeping the previous index on a miss). `CellClass::PickupCrate @
    /// 0x00481A00` plays one at the crate, for the local player's house
    /// except Reveal, which plays for everyone (`0x0048202D`):
    /// `CrateMoneySound=` (`+0x1E4`, `0x00482553`), `CrateRevealSound=`
    /// (`+0x1E8`, `0x0048202F`), `CrateFireSound=` (`+0x1EC`, `0x004832E4`),
    /// `CrateArmourSound=` (`+0x1F0`, `0x00482F22`), `CrateSpeedSound=`
    /// (`+0x1F4`, `0x00483111`), `CrateUnitSound=` (`+0x1F8`, `0x0048243E`)
    /// and `CratePromoteSound=` (`+0x1FC`, `0x00482B7B`). The name is retained
    /// at the data boundary; absent or empty is silence.
    pub crate_money_sound: Option<String>,
    pub crate_reveal_sound: Option<String>,
    pub crate_fire_sound: Option<String>,
    pub crate_armour_sound: Option<String>,
    pub crate_speed_sound: Option<String>,
    pub crate_unit_sound: Option<String>,
    pub crate_promote_sound: Option<String>,
    /// `[AudioVisual] EliteFlashTimer=` — `RulesClass+0xBE8`. Seeded into
    /// `TechnoClass+0xF0` on an elite promotion (`0x006FA0D6..0x006FA0DC`),
    /// whatever house owns the object. Constructor default UNCHECKED.
    pub elite_flash_timer: i32,
    /// Exact `[AudioVisual] IdleActionFrequency=` double, `Rules+0x1710`.
    /// Original constructor667574/66757E stores0x3FB53F7CED916873;
    /// AudioVisual66B3EA reads over its current value. Infantry51CDB0 uses
    /// this value to arm its idle timer before subsequent idle/RNG decisions.
    /// Preserve the native double; milliunit quantization changes its timer.
    pub idle_action_frequency: f64,
    /// `[AudioVisual] ForceShieldColor=`, a `[ColorAdd]` index
    /// (`Rules+0x18B0`): ReadInt at `0x0066B877..0x0066B891` over its current
    /// value, constructor 0 (`0x006678C1`). A Force Shielded building's draws
    /// OR its colour into their pixels (`app/presentation/lighting.rs`).
    pub force_shield_color: i32,
    /// AudioVisual LaserTargetColor (Rules+18A4), constructor6678AA=0;
    /// ReadInt66B818..66B832 retains the current ColorAdd table index.
    pub laser_target_color: i32,
    /// Phobos `[AudioVisual] AirstrikeLineColor=`, `RulesExt`'s
    /// `Valueable<ColorStruct>` (constructor `255,0,0`, `src/Ext/Rules/Body.h`):
    /// the RGB an airstrike beam and its target dot draw. A TechnoType's own
    /// `AirstrikeLineColor` overrides it (`Simulation::airstrike_line_color`).
    #[cfg(feature = "phobos")]
    pub airstrike_line_color: [u8; 3],
    /// Phobos `[AudioVisual] AirstrikeLineZAdjust=`, `RulesExt`'s
    /// `Valueable<int>` (constructor 0): added to the smaller of the beam's two
    /// end depths, which `TechnoClass_DrawAirstrikeFlare_LineColor`
    /// (`0x007058F6`) then gives the whole beam.
    #[cfg(feature = "phobos")]
    pub airstrike_line_z_adjust: i32,
    /// AudioVisual66B832..66B857, Rules+18A8; constructor6678B0=0.
    pub iron_curtain_color: i32,
    /// AudioVisual66B857..66B877, Rules+18AC; constructor6678B6=0.
    pub berserk_color: i32,
    /// AudioVisual66A046..66A07C / 66A088..66A0B5 use ReadString128 then
    /// VocFindIndex7514D0, retaining the current index on a miss (ctor -1).
    pub airstrike_abort_sound: Option<String>,
    pub airstrike_attack_voice: Option<String>,
    /// `ConditionRedSparkingProbability=` ([General]) — per-tick probability that
    /// the `AI_Update` damage-Spark particle system spawns while health is below
    /// ConditionRed. Default **0.02** (verified `RulesClass__Constructor`; stock INI
    /// does NOT set this key). Stored as **f64** because gamemd reads it with
    /// `ReadDouble` and compares it as a double; the f32-vs-f64 rounding shifts the
    /// integer roll threshold by 1 (→ desync). Consumed via `condition_red_spark_threshold`.
    pub condition_red_sparking_probability: f64,
    /// `ConditionYellowSparkingProbability=` ([General]) — per-tick spawn probability
    /// in the yellow band (ConditionRed <= ratio < ConditionYellow). Default **0.01**
    /// (verified `RulesClass__Constructor`). f64 for the same reason as the red band.
    pub condition_yellow_sparking_probability: f64,
    /// Integer roll threshold for the red-band damage-Spark prob-roll, precomputed
    /// from `condition_red_sparking_probability` at parse time: the per-tick test
    /// becomes the pure-integer `roll < threshold` (no float in the sim hot path).
    /// `roll = scenario_rng.next_range_u32_inclusive(0, 0x7ffffffe)`. See
    /// [`unit_draw_threshold`]. Default 42_949_673 (band 0.02).
    pub condition_red_spark_threshold: u32,
    /// Integer roll threshold for the yellow-band damage-Spark prob-roll.
    /// Default 21_474_837 (band 0.01).
    pub condition_yellow_spark_threshold: u32,
    /// AI coefficient for the scorer weapon's effectiveness against a candidate.
    pub dumb_my_effectiveness_coefficient: f64,
    /// AI coefficient for the candidate weapon's effectiveness against the scorer.
    pub dumb_target_effectiveness_coefficient: f64,
    /// AI coefficient for the candidate type's `SpecialThreatValue=`.
    pub dumb_target_special_threat_coefficient: f64,
    /// AI coefficient for the candidate's live health ratio.
    pub dumb_target_strength_coefficient: f64,
    /// AI coefficient for whole cells beyond the selected weapon range.
    pub dumb_target_distance_coefficient: f64,
    /// `MyEffectivenessCoefficientDefault=` (`RulesClass+0x1040`,
    /// `RulesClass::ReadGeneral @ 0x00671AEE`). Read default for the per-type
    /// `MyEffectivenessCoefficient=`, and through it the live coefficient for
    /// every house that has ever had a building in the world — see
    /// [`crate::sim::combat::greatest_threat`]. Fallbacks here are the stock
    /// `rulesmd.ini` values; the `RulesClass` constructor seed is UNCHECKED.
    pub my_effectiveness_coefficient_default: f64,
    /// `TargetEffectivenessCoefficientDefault=` (`RulesClass+0x1048`).
    pub target_effectiveness_coefficient_default: f64,
    /// `TargetSpecialThreatCoefficientDefault=` (`RulesClass+0x1050`).
    pub target_special_threat_coefficient_default: f64,
    /// `TargetStrengthCoefficientDefault=` (`RulesClass+0x1058`).
    pub target_strength_coefficient_default: f64,
    /// `TargetDistanceCoefficientDefault=` (`RulesClass+0x1060`).
    pub target_distance_coefficient_default: f64,
    /// `EnemyHouseThreatBonus=` (`RulesClass+0x1090`, ReadDouble at
    /// `0x00671C7C`; constructor 0.0 at `0x00666CB7`, stock rulesmd 400):
    /// added to the threat score of a candidate the scorer house's current
    /// enemy owns (`TechnoClass::Calculate_Threat_Score @ 0x0070CF13`).
    pub enemy_house_threat_bonus: f64,
    /// `ThreatPerOccupant=` (`RulesClass+0x0DF4`, `RulesClass::ReadGeneral @
    /// 0x00670128`; constructor default 5 at `0x006668DE`, stock rulesmd 10).
    /// A garrisoned building's `ThreatPosed` is `occupants * this` instead of
    /// its own type value (`TechnoClass::Get_ThreatPosed @ 0x00708B40`).
    pub threat_per_occupant: i32,
    /// `NormalTargetingDelay=` ([General], stock 27; `Rules+0xE08`, ReadInt
    /// `0x006701BA..0x006701D3`, constructor 27 at `0x00666911`, no clamp) —
    /// frames between passive target scans for every mission except Area
    /// Guard. The per-object scan timer is re-armed to this value plus a
    /// 0..=2 scenario-RNG jitter.
    pub normal_targeting_delay: i32,
    /// `[General] DeadBodies=` (`Rules+0x124`): the corpse anims an
    /// infantryman's Die1..Die5 completion picks from when its type names none
    /// (`0x00520C42..0x00520C91`). Retail: `DEATH_A`..`DEATH_F`.
    pub dead_bodies: Vec<String>,
    /// `GuardAreaTargetingDelay=` ([General], stock 36; `Rules+0xE04`, ReadInt
    /// at `0x0067019A..0x006701B4`, constructor 36 at `0x00666906`, no clamp)
    /// — the same cadence for an Area Guard object, which scans twice as far
    /// and so scans less often (`0x0070985E`). It is also the dwell after a
    /// shot before a unit's idle turret returns (`0x00736B45`) and before a
    /// Gattling building's idle decay starts (`0x0043FEE9`), each plus 5.
    pub guard_area_targeting_delay: i32,
    /// SFX played when the first occupant enters a CanBeOccupied building.
    /// Parsed from [AudioVisual] BuildingGarrisonedSound (typically "BuildingGarrisoned").
    /// None = no sound configured. Resolved at app layer to a sound.ini entry.
    pub building_garrisoned_sound: Option<String>,
    /// Rules+1C0, [AudioVisual] BuildingAbandonedSound. Constructor6658D4
    /// stores -1. Reader669C23..669C62 follows BuildingGarrisonedSound and
    /// precedes BuildingRepairedSound: ReadString128, Voc Find7514D0, retain
    /// the previous ID for absent/empty/unknown names. Bound by the shared
    /// sound-reference reader over the processed rules passes.
    pub building_abandoned_sound: Option<String>,
    /// Global wall/building sale cue from `[AudioVisual] SellSound=`.
    pub sell_sound: Option<String>,
    /// Base-alert siren from `[AudioVisual] BaseUnderAttackSound=` (stock
    /// `BaseUnderAttackSiren`), stored at `Rules+0x184`.
    ///
    /// gamemd: `HouseClass::NotifyUnderAttack @ 0x004F95B8..0x004F95CF` plays
    /// it non-positionally (pan `0x2000`, volume `1.0f`, no handle) through
    /// `VocClass::PlayAtPos @ 0x00750920`, right after the EVA line at
    /// `0x004F95B3` and only when `CreateRadarEvent @ 0x0065FA70` accepted the
    /// ping. The ore-miner branch plays `EVA_OreMinerUnderAttack` at
    /// `0x004F94FB` and then jumps (`0x004F9500 JMP`) to `LAB_004F95D4`, so a
    /// harvester under attack gets the EVA and **no** siren. The
    /// `EVA_OurAllyIsUnderAttack` branch does NOT jump past — native sirens
    /// for an ally's base too, which VERA does not (pre-existing residual).
    pub base_under_attack_sound: Option<String>,
    /// Building crumble cue from `[AudioVisual] BuildingDieSound=` (stock
    /// `BuildingGenericDie`).
    ///
    /// gamemd: `RulesClass::ReadAudioVisual @ 0x0066AA49..0x0066AA88` stores
    /// the `VocClass::FindByName` result in `Rules+0x6E8`.
    /// `BuildingClass::DestructionEffects` plays it at the building's own
    /// coordinate (`0x0044174E LEA EAX,[ESI+0x9C]`, then
    /// `0x00441773 MOV ECX,[Rules+0x6E8]`, `0x00441779 CALL
    /// VocClass::PlayAtCoord @ 0x00750E20`) — but **only when the building
    /// type's own `DieSound=` list is empty**: `0x0044173F MOV
    /// ECX,[type+0x520]` is that `DynamicVectorClass`'s count
    /// (`TechnoTypeClass+0x510..0x528`) and `0x0044174A CMP ECX,EBX ; JNZ`
    /// skips the global when it is non-zero (`EBX` is zeroed for the whole
    /// function at `0x00441606`).
    pub building_die_sound: Option<String>,
    /// Struck-building cue from `[AudioVisual] BuildingDamageSound=` (stock
    /// `BuildingDamaged`), stored at `Rules+0x714`.
    /// Original gamemd ctor665650 stores -1 at666045;
    /// ReadAudioVisual6691E0's block66AC98..66ACDD passes literal83A460,
    /// empty889F64 and capacity128 to ReadString528A10, then7514D0.
    /// The fixed-catalog process binder retains earlier valid references for
    /// absent, empty or unregistered later values; no unresolved name escapes.
    ///
    /// gamemd: `BuildingClass::ReceiveDamage @ 0x00442230` plays it at the
    /// building's own coordinate (`0x004426DB LEA ECX,[ESI+0x9C]`,
    /// `0x00442700 MOV ECX,[Rules+0x714]`, `0x00442706 CALL
    /// VocClass::PlayAtCoord @ 0x00750E20`). It is **not** a per-hit cue: the
    /// shared Techno receiver's result selects the arm. `0x0044242C MOV
    /// AL,[ESI+0x90]` (`ObjectClass::IsAlive`) skips the whole dispatch for a
    /// dead building; otherwise `0x00442476 JMP [EAX*4 + 0x00442C18]` with
    /// `EAX = result - 2` enters the four-entry table
    /// `{0x004426AC, 0x004426C8, 0x004424A2, 0x0044247D}`, and only entries 0
    /// and 1 — result 2 (the hit crossed HP from `>= Strength >> 1` to below
    /// it) and result 3 (it crossed below `Strength * Rules+0x1708`,
    /// ConditionRed) — reach the gate at `0x004426D2 CMP [type+0x538],-1`.
    /// A type with its own `DamageSound=` takes `JNZ 0x0044270B` and the
    /// global is skipped.
    building_damage_sound: Option<String>,
    /// Rules+178/+17C/+180, ctor665868..665874=-1. Original AudioVisual
    /// 66A3AD..66A472 reads these fixed-catalog scalar names in that order
    /// with128-byte retention readers;
    /// House RecordLastBuilt4FB6B0 selects the concrete class before the
    /// produced type's CreateSound override. Buildings have no class fallback.
    create_unit_sound: Option<String>,
    create_infantry_sound: Option<String>,
    create_aircraft_sound: Option<String>,
    /// Rules+28C: ctor665A00=-1; AudioVisual66A355..66A38C reads the
    /// exact DiskLaserChargeUp key through ReadString128/Voc FindIndex.
    /// The fixed-catalog binder owns retention across reached rules passes.
    disk_laser_charge_up: Option<String>,
    /// Thunder cues from `[AudioVisual] LightningSounds=` (stock a single
    /// `WeatherStrike`), in source order.
    ///
    /// gamemd: `CCINIClass::ReadSoundList @ 0x00525430` reads the value with
    /// `ReadString(..., "", buf, 0x80)`, tokenises it with `strtok` on `","`
    /// (`0x00817F70`) and keeps only names `VocClass::FindPtrByName` resolves,
    /// leaving the vector at `Rules+0x734` (items `+0x738`, count `+0x744`).
    /// `LightningStorm::GroundStrike @ 0x0053A45F..0x0053A4A2` skips the cue
    /// entirely when the count is zero and otherwise plays
    /// `items[rand % count]` at the strike coordinate.
    pub lightning_sounds: Vec<String>,
    /// Displayed-credits tick cues from `[AudioVisual] CreditTicks=` (stock
    /// `CreditUp,CreditDown`), the `Rules+0x6D0` sound list (count `+0x6DC`).
    ///
    /// gamemd: `CreditsClass::Draw @ 0x004A24F4..0x004A2533` plays
    /// `items[counting_up(+0x9) ? 0 : 1]` through `VocClass::PlayAtPos @
    /// 0x00750920` at volume `0.5f`, pan `0x2000` (centre), only while the
    /// `animating(+0xA)` latch set by `CreditsClass::AI @ 0x004A2600` on a
    /// step that changed the displayed value is up, and only when the list
    /// holds at least two entries (`0x004A2505 CMP [EAX+0x6dc],2 / JL`).
    /// Same `ReadSoundList` tokenisation as `lightning_sounds`; resolution
    /// is deferred to the app audio registry.
    pub credit_ticks: Vec<String>,
    /// Weather-storm start cue from `[AudioVisual] StormSound=` (stock
    /// `WeatherIntro`), stored at `Rules+0x730`.
    ///
    /// gamemd: `RulesClass::ReadAudioVisual @ 0x0066AEAE..0x0066AEE6` reads
    /// the key at `0x0083A400` and keeps the `VocClass::FindByName` result.
    /// `LightningStorm::Start @ 0x0053A032..0x0053A044` plays it
    /// **non-positionally** — `MOV ECX,[Rules+0x730]`, `MOV EDX,0x2000`
    /// (centred pan), `PUSH 0x3F800000` (volume `1.0f`), no handle, through
    /// `VocClass::PlayAtPos @ 0x00750920` — and only inside the
    /// `0x0053A014 MOV AL,[Rules+0x17B0]` gate it shares with the on-screen
    /// storm message. See [`GeneralRules::lightning_print_text`].
    ///
    /// It is **not** a launch cue. `SuperClass::Launch @ 0x006CC390` case 2
    /// passes `[Rules+0x1794]` (`LightningDeferment`, stock 250) as `Start`'s
    /// `param_2`, and `Start` returns at `if (param_2 != 0)` before reaching
    /// `0x0053A044`; `LightningStorm::Process @ 0x0053A6C0` re-enters with
    /// `param_2` cleared (`0x0053AAC8 XOR EDX,EDX`) once the countdown expires
    /// and that entry plays it.
    pub storm_sound: Option<String>,
    /// `[General] LightningPrintText=` (`Rules+0x17B0`), the gate on both the
    /// storm message and [`GeneralRules::storm_sound`].
    ///
    /// gamemd: `RulesClass::ReadGeneral @ 0x0067107F..0x0067108C` reads the
    /// key at `0x0083BC74`. Stock `rulesmd.ini` does **not** author it, so the
    /// constructor default decides: `RulesClass::Constructor @ 0x006676BC MOV
    /// byte ptr [ESI+0x17B0],AL`, where the last definition of `EAX` before it
    /// is `0x00667202 MOV EAX,0x1` and the function's last `CALL` is at
    /// `0x0066715D` — so the default is **true** and the storm cue does play
    /// on retail data.
    pub lightning_print_text: bool,
    /// Nuclear-missile launch cue from `[AudioVisual] DigSound=` (stock
    /// `NukeSiren`), stored at `Rules+0x174`.
    ///
    /// gamemd: `RulesClass::ReadAudioVisual @ 0x00669331..0x00669367` reads
    /// the key at `0x0083AB70` (`"DigSound"`). Despite the name it is the
    /// nuke siren — stock `rulesmd.ini` says so in a comment — and
    /// `SuperClass::Launch @ 0x006CC390` case 0 (`Type=MultiMissile`) is its
    /// only reader, playing it at the target coordinate through
    /// `VocClass::PlayAtCoord @ 0x00750E20` on both branches
    /// (`0x006CDCA8`/`0x006CDCAE` when the silo animates the launch,
    /// `0x006CDDE3`/`0x006CDDE9` when it does not).
    pub dig_sound: Option<String>,
    /// `[General] NukeTakeOff=` (stock `NUKETO`), the AnimType at `Rules+0x98`
    /// (ReadString then the AnimType lookup, `0x0066D829..0x0066D850`;
    /// constructor null at `0x00665735`). `BuildingClass::Mission_Missile`
    /// plays it at the silo's launch point (`0x0044CC62`).
    pub nuke_take_off: String,
    /// `[General] AISuperDefenseProbability=` (`Rules+0xEC4`, the IntVector
    /// reader at `0x0067038E`; constructor empty), indexed by a house's
    /// difficulty: the percent chance a computer house takes up the defence
    /// when an `AIDefendAgainst=` weapon targets near its base.
    pub ai_super_defense_probability: Vec<i32>,
    /// `[General] AISuperDefenseDistance=` (`Rules+0xEE4`, ReadRange at
    /// `0x006703D9`, leptons; constructor 10 at `0x00666A35`): the alert's
    /// reach from the house's base centre.
    pub ai_super_defense_distance: i32,
    /// `[General] AISuperDefenseFrames=` (`Rules+0xEE0`, ReadInt at
    /// `0x006703A4..0x006703BE` with the field as its default; constructor
    /// 25 at `0x00666A24`): how long the launch alert lasts. A computer house
    /// aims its Force Shield at the alerted cell while the alert is younger
    /// (`HouseClass::AI_TryFireSW`, `0x00509A7F..0x00509A99`).
    pub ai_super_defense_frames: i32,
    /// `[General] AIIonCannon*Value=`: what a computer house values an enemy
    /// object by when it aims a superweapon.
    pub ai_ion_cannon_values: AiIonCannonValues,
    /// `[AudioVisual] PsychicDominatorActivateSound=` (stock
    /// `PsychicDominatorActivate`), stored at `Rules+0x24C`.
    ///
    /// gamemd: `SuperClass::Launch @ 0x006CC390` case 7 plays it at the target
    /// coordinate — `0x006CCE22 MOV ECX,[Rules+0x24C]`,
    /// `0x006CCE28 CALL VocClass::PlayAtCoord @ 0x00750E20`.
    pub psychic_dominator_activate_sound: Option<String>,
    /// `[AudioVisual] GeneticMutatorActivateSound=` (stock
    /// `GeneticMutatorActivate`), stored at `Rules+0x250`.
    ///
    /// gamemd: `SuperClass::Launch @ 0x006CC390` case 9 plays it at the target
    /// coordinate — `0x006CD8CD MOV ECX,[Rules+0x250]`,
    /// `0x006CD8D3 CALL VocClass::PlayAtCoord @ 0x00750E20`.
    pub genetic_mutator_activate_sound: Option<String>,
    /// `[AudioVisual] PsychicRevealActivateSound=` (stock
    /// `PsychicRevealActivate`), stored at `Rules+0x254`.
    ///
    /// gamemd: `SuperClass::Launch @ 0x006CC390` case 11 plays it at the
    /// target coordinate — `0x006CD7B9 MOV ECX,[Rules+0x254]`,
    /// `0x006CD7BF CALL VocClass::PlayAtCoord @ 0x00750E20`. This case plays
    /// no EVA line.
    pub psychic_reveal_activate_sound: Option<String>,
    /// `[AudioVisual] SpyPlaneCamera=` (stock `SpyPlaneSnapshot`), the sound
    /// index at `Rules+0x280` (constructor -1, no sound, at `0x006659EE`):
    /// ReadString128 then `VocClass::FindIndex @ 0x007514D0`, keeping the
    /// prior index for a missing, empty or unknown name
    /// (`0x0066A295..0x0066A2C2`); [`RuleSet::bind_type_sound_references`]
    /// resolves it. `AircraftClass::Mission_SpyplaneApproach @ 0x004155F0`
    /// plays it at the plane through `VocClass::PlayAt @ 0x007509E0`
    /// (`0x004156FB`) on each camera snapshot.
    pub spy_plane_camera: Option<String>,
    /// `[AudioVisual] SpyPlaneCameraFrames=` (`Rules+0x290`, ReadInteger at
    /// `0x0066A39A` with the field as its default; constructor 16 at
    /// `0x00665A06`): the frames Mission_SpyplaneApproach returns, its
    /// mission timer (`0x0041578D`, `0x004157A0`, `0x004157B3`).
    pub spy_plane_camera_frames: i32,
    /// SFX played when a paradropped passenger successfully deploys a parachute.
    /// Parsed from [AudioVisual] ChuteSound (stock "ParachuteDrop").
    /// None = no sound configured. Resolved at app layer to a sound.ini entry.
    pub chute_sound: Option<String>,
    /// Sound event for shell main-menu buttons from [AudioVisual] GUIMainButtonSound.
    pub gui_main_button_sound: Option<String>,
    /// Shell first-paint controls-reveal slide-in cue from [AudioVisual]
    /// GUIMoveInSound (stock `MenuSlideIn`). Played once at the start of each
    /// allow-listed shell dialog's slide. None = no sound configured.
    pub gui_move_in_sound: Option<String>,
    /// Shell teardown slide-out cue from [AudioVisual] GUIMoveOutSound (stock
    /// `MenuSlideOut`, `RulesClass+0x19C` read at `0x006694C7`), played by
    /// `0x00608070` before a shown shell dialog's buttons slide out.
    pub gui_move_out_sound: Option<String>,
    /// Shell slide-in end cue from [AudioVisual] ShellButtonSlideSound
    /// (`RulesClass+0x750`, constructor -1 at `0x00666098`). `0x0066AF8D`
    /// reads 128 bytes and keeps the prior value on an empty copy or an
    /// unknown name (`0x007514D0` -1); like its siblings, the name resolves
    /// only when played. The slide engine `0x006071E0` plays it as each
    /// slide-in ends (`0x00607F59`). Stock rules leave it empty.
    pub shell_button_slide_sound: Option<String>,
    /// Generic shell click sound from [AudioVisual] GenericClick
    /// (`Rules+0x70C`, read at `0x0066AD30` through `VocClass::FindByName`;
    /// retail `MenuClick`). A human player's sale order plays it too
    /// (`Sell_Back @ 0x00447110`), and so does switching a repair off, or on
    /// below Strength (`BuildingClass::ToggleRepair @ 0x00446FF0`).
    pub generic_click_sound: Option<String>,
    /// Guard command acknowledgement, Rules+724. Constructor665650 starts
    /// at -1; AudioVisual66AE1C..66AE67 reads GuardSound with ReadString128
    /// then Voc FindIndex7514D0, retaining the prior ID on missing, empty or
    /// unresolved text. The fixed SOUNDMD binder owns name resolution.
    pub guard_sound: Option<String>,
    /// Stop command acknowledgement, Rules+720, read just before GuardSound
    /// by the same reader (`0x0066ADE3..0x0066AE16`).
    pub stop_sound: Option<String>,
    /// Rules+6F0/+6F4: constructor66600F/666015 starts at -1;
    /// AudioVisual66AAC0..66AB53 reads exact RadarOn then RadarOff through
    /// ReadString128/Voc7514D0, retaining the prior fixed-catalog ID on
    /// missing, empty or unknown names. Radar656BE0 requests them centered,
    /// full-volume and without a retained handle on ordinary direction changes.
    /// Source: tools/input_oracle/radar_availability_audio.md and its consumed corpus.
    pub radar_on: Option<String>,
    pub radar_off: Option<String>,
    /// Rules+6F8/+6FC: the radar transmission's static, read next as exact
    /// MovieOn then MovieOff by the same reader (`0x0066AB5F..0x0066ABC7`).
    /// `RadarClass::PlayRadarMovie @ 0x006578F0` and `PerFrameMovieUpdate @
    /// 0x006579E0` play them centred at full volume on the radar's handle.
    pub movie_on: Option<String>,
    pub movie_off: Option<String>,
    /// Rules+220/+224: constructor66595E/665964 starts at -1;
    /// AudioVisual66A4EF..66A582 reads SpySatActivationSound then
    /// SpySatDeactivationSound with ReadString128/Voc7514D0 and retains the
    /// current fixed-catalog ID on missing, empty or unknown names.
    /// House508F60 requests the local cue after its map callback/latch edge.
    /// Source: tools/input_oracle/spy_sat_audio.md and its consumed corpus.
    pub spy_sat_activation_sound: Option<String>,
    pub spy_sat_deactivation_sound: Option<String>,
    /// Rules+230, VoiceIFVRepair. Ctor665976 starts at -1;
    /// AudioVisual669D63..669DA7 uses ReadString128/Voc7514D0, retaining
    /// the current ID on missing/empty/unknown text. Fixed SOUNDMD binding
    /// supplies the name. Native controls: spatial_oracle/attack_order_voice.
    voice_ifv_repair: Option<String>,
    /// Rules+1D4: constructor6658F2=-1; AudioVisual6696C0..6696FF reads
    /// StartPlanningModeSound with ReadString128 and Voc Find7514D0,
    /// retaining the prior ID on a missing or invalid name.
    pub start_planning_mode_sound: Option<String>,
    /// Rules+1E0: constructor665904=-1; AudioVisual66970A..669740 reads
    /// EndPlanningModeSound immediately after Start, with the same reader.
    pub end_planning_mode_sound: Option<String>,
    /// Rules+1D8: constructor6658F8=-1; AudioVisual669B25..669B52 reads
    /// AddPlanningModeCommandSound later in the same pass, retaining the ID.
    pub add_planning_mode_command_sound: Option<String>,
    /// Rules+1CC: constructor6658E6=-1; AudioVisual669BA8..669BD3 reads
    /// exact PlaceBeaconSound with ReadString128 and Voc Find7514D0,
    /// retaining the prior ID on a missing or unknown name. PlaceBeacon
    /// 430D7D..430D94 plays it centred at full volume for the local House.
    pub place_beacon_sound: Option<String>,
    /// ScatterSelectedObjects730FE0's centered cue. Rules+728 starts at -1
    /// (66585F/666063); AudioVisual66AE6C..66AEA2 reads exact ScatterSound
    /// with ReadString128 and fixed Voc lookup, retaining failed selections.
    pub scatter_sound: Option<String>,
    /// `[AudioVisual] ScoldSound=` (`Rules+0x700`, read at `0x0066ABE8`
    /// through `VocClass::FindByName`; retail `MenuScold`): ToggleRepair's
    /// sound when a repair is switched on at full Strength (`0x00447068`),
    /// and PlanningMode refusal63A0D0. Constructor666027 stores -1;
    ///66ABD3..66AC12 retains the prior resolved ID on failed/empty reads.
    pub scold_sound: Option<String>,
    /// Launcher Options Sound/Voice preview cue from [AudioVisual] GenericBeep.
    pub generic_beep_sound: Option<String>,
    /// Sound event for shell checkboxes from [AudioVisual] GUICheckboxSound.
    pub gui_checkbox_sound: Option<String>,
    /// `[General] OreTwinkle=` AnimType name; `None` keeps the constructor's
    /// null pointer so the post-load twinkle pass is skipped.
    ///
    /// gamemd: `RulesClass::ReadGeneral` at `0x0066D661..0x0066D699` reads
    /// section "General" (`0x00826278`) key "OreTwinkle" (`0x0083CF4C`) with an
    /// empty default and capacity 0x80, then resolves a non-empty value through
    /// `AnimTypeClass::Find_Or_Allocate @ 0x00428B80` into `Rules+0x1870`.
    pub ore_twinkle: Option<String>,
    /// `[AudioVisual] OreTwinkleChance=`: one twinkle roll in N per resource cell.
    ///
    /// gamemd: `RulesClass::ReadAudioVisual` at `0x0066B7F8..0x0066B812` reads
    /// section "AudioVisual" (`0x00839EA8`) key "OreTwinkleChance"
    /// (`0x0083A1CC`) through `INIClass::ReadInt @ 0x005276D0` into
    /// `Rules+0x186C`; the constructor default is 0x32.
    pub ore_twinkle_chance: i32,
    /// `[AudioVisual] GUIBuildSound=` (`Rules+0x18C`, read at `0x006693C1`
    /// through `VocClass::FindByName`; retail `MenuClick`): the sidebar cameo
    /// click sound, played by `SelectClass::Action @ 0x006AAD00` for every
    /// click that acts (e.g. `0x006AAE2A`, `0x006AB713`). Residual: gamemd
    /// resolves the name as it reads it and keeps the previous layer's sound
    /// when the name does not resolve; VERA stores the name and resolves it at
    /// play time. Retail's `MenuClick` resolves.
    pub gui_build_sound: Option<String>,
    /// `[AudioVisual] BuildingSlam=` (`Rules+0x6EC`, read at `0x0066AAA7`
    /// through `VocClass::FindByName`; retail `PlaceBuilding`): the PLACE
    /// event handler `HouseClass @ 0x004FB0E0` plays it centred at full
    /// volume (`0x004FB2FD..0x004FB314`) for the player's own house after the
    /// building's Unlimbo succeeds. Same name-resolution residual as
    /// `gui_build_sound`; retail's `PlaceBuilding` resolves.
    pub building_slam: Option<String>,
    /// Sidebar tab click sound from [AudioVisual] GUITabSound (retail
    /// `MenuTab`; `Rules+0x190`, read at `0x00669402..0x0066942C`).
    /// `SidebarClass::Action 0x006A7780` plays it (`VocClass::PlayAt
    /// 0x00750920`, volume 1.0) at `0x006A7A66` for the tab inputs
    /// `0x80CB..0x80CE`, including a click on the tab that is already active
    /// (`0x006A7928` jumps straight to it).
    pub gui_tab_sound: Option<String>,
    /// Rules+6AC, ctor665FB0=-1, reader66A7B3..66A7E9. Fixed SOUNDMD
    /// reference; missing/unknown later names retain the previous valid ID.
    incoming_message_sound: Option<String>,
    /// Rules+6A8, ctor665FAA=-1, AudioVisual66A763..66A7A8. Exact
    /// GameClosed ReadString128/VocFind7514D0 precedes IncomingMessage;
    /// empty/unknown later values retain the prior fixed SOUNDMD reference.
    /// LAN discovery open→closed5E02C6..5E031E plays this before its chat.
    game_closed_sound: Option<String>,
    /// Rules+6C4, ctor665FD5=-1, reader66A7EF..66A822. Exact
    /// [AudioVisual] MessageCharTyped via ReadString128/VocFindIndex7514D0;
    /// drawn nonsilent TextLabels request it at72A609, pan2000/volume1.
    message_char_typed_sound: Option<String>,
    /// Rules+6B0, constructor665FB6=-1. AudioVisual66A828..66A86D
    /// reads exact SystemError through ReadString128/VocFind7514D0,
    /// retaining the prior fixed-SOUNDMD ID on missing/empty/unknown text.
    /// LAN join validation5DF410 and timeout5DC506 play it centered at1.0.
    system_error_sound: Option<String>,
    /// Rules+6B4, constructor665FBC=-1. AudioVisual66A868..66A8AE reads exact
    /// OptionsChanged after SystemError through ReadString128/VocFind7514D0,
    /// retaining the prior ID on missing/empty/unknown text. A main-menu typed
    /// code (0x00531ECB..0x00531EDB) plays it centred at volume 1.0; the
    /// serial/modem setup dialogs (0x005B5E71, 0x005B962C; game modes 1/2,
    /// which VERA lacks) are its only other players.
    options_changed_sound: Option<String>,
    /// Rules+6BC: constructor665FC8=-1; AudioVisual66A8ED..66A932 reads
    /// exact PlayerLeft after GameForming and before PlayerJoined. ReadString
    /// capacity128/defaultempty plus fixed SOUNDMD VocFind7514D0 retains the
    /// previous ID on missing/empty/unknown text across reached Rules layers.
    /// LAN type10 endpoint removal5E10F2..5E1114 plays it only in admitted
    /// membership states>=2, independent of remaining participant count.
    player_left_sound: Option<String>,
    /// Rules+6B8/+6C0, ctor665FC2/665FCE=-1. AudioVisual readers
    ///66A8B4..66A8E7 (GameForming) and66A938..66A973 (PlayerJoined)
    /// use ReadString128/VocFind7514D0 with prior-reference retention.
    /// Native order: OptionsChanged, GameForming, PlayerLeft, PlayerJoined.
    /// Forming is received discovery5E0280/5E0483, not own New; Joined is
    /// admitted type3 at5E0743..0770 or host type6 afteraccept7 at5E1B32.
    game_forming_sound: Option<String>,
    player_joined_sound: Option<String>,
    /// `[AudioVisual] MessageDelay=` native ReadDouble in6691E0 retains
    /// Rules+14C0. Constructor665650 initializes binary64 .6. Action11
    /// 6DE0EF..6DE100 converts MessageDelay*900 through the native ftol into
    /// MessageList's 16ms buckets, independently of simulation pacing.
    pub message_delay_minutes: crate::util::native_x87::NativeF64Bits,
    /// RulesClass::ReadAudioVisual6691E0,66A679..66A693: current-default
    /// `[AudioVisual] ShroudGrow`; constructor667775 initializes false.
    pub shroud_grow: bool,
    /// Rules+1640, ReadAudioVisual66B49E..66B4BE: current-default
    /// ShroudRate in minutes, f32 widened; constructor6673FC/667402 stores4.
    pub shroud_rate: NativeF64Bits,
    /// `[AudioVisual] FogRate=` in minutes, Rules+1648. Constructor
    /// 66740C/667416 stores binary64 .05; ReadAudioVisual66B4CA..66B4E4
    /// reads the current default through ReadDouble5283D0 (f32 widened).
    pub fog_rate: NativeF64Bits,
    /// `[AudioVisual] SpeakDelay=` in MINUTES (retail `2`): the EVA advice
    /// repeat interval. `RulesClass::ReadAudioVisual` reads key
    /// `"SpeakDelay"` (`0x0083A270`) through `INIClass::ReadDouble @
    /// 0x005283D0` into `Rules+0x16A8` (`0x0066B646 FSTP double`);
    /// `RulesClass::Constructor 0x006674BA` zero-fills it, so a missing key is
    /// `0.0`. `HouseClass::Update` re-arms its funds/low-power timers to
    /// `SpeedNormalize(ftol(SpeakDelay * 900.0))` (`0x004F8BB4..0x004F8BCB`,
    /// `0x007E27F8` = `900.0`).
    pub speak_delay_minutes: f64,
    /// Sound event for opening shell combo boxes from [AudioVisual] GUIComboOpenSound.
    pub gui_combo_open_sound: Option<String>,
    /// Sound event for closing shell combo boxes from [AudioVisual] GUIComboCloseSound.
    pub gui_combo_close_sound: Option<String>,
    /// Rules+194/+198: constructors665892/665898 store-1. ReadAudioVisual
    /// 669423..6694B6 reads exact GUIOpenSound then GUICloseSound through
    /// ReadString128/VocFind7514D0, retaining each prior valid ID. The fixed
    /// SOUNDMD binder owns these references, separately from the combo keys.
    /// SidebarInput6D091A/6D0860 consumes them; see tools/audio_catalog_owner.md.
    gui_open_sound: Option<String>,
    gui_close_sound: Option<String>,
    /// Sound used by conditional reciprocal-link harvester release. Parsed
    /// from [AudioVisual] BunkerWallsDownSound (retail value "TankBunkerDown").
    /// Stock zero-link refinery unload completion does not play it. None =
    /// no sound configured.
    pub bunker_walls_down_sound: Option<String>,
    /// Tank-bunker walls-up SFX. Parsed from [AudioVisual] BunkerWallsUpSound
    /// (retail value "TankBunkerUp"). None = no sound configured.
    pub bunker_walls_up_sound: Option<String>,
    /// Direct rocker force coefficient (DirectRockingCoefficient= in [AudioVisual]).
    /// Multiplies the final DirectRocker impulse force. Default 1.5.
    pub direct_rocking_coefficient: SimFixed,
    /// Damping coefficient applied while a vehicle is moving (FallBackCoefficient=
    /// in [AudioVisual]). Multiplies the base 0.002 rad/tick decay rate; smaller
    /// values keep the body tilted longer between successive impulses. Default 0.1.
    pub fallback_coefficient: SimFixed,
    /// Fallback sound played at the arrival cell of a self-teleport when the
    /// per-unit `ChronoInSound=` is unset. Parsed from `[AudioVisual]
    /// ChronoInSound=` (stock ships `ChronoMinerTeleport`). A genuinely-absent
    /// key yields `None` = no sound, not a fabricated fallback.
    pub chrono_in_sound: Option<String>,
    /// Fallback sound played at the departure cell of a self-teleport when the
    /// per-unit `ChronoOutSound=` is unset. Parsed from `[AudioVisual]
    /// ChronoOutSound=` (stock ships `ChronoMinerTeleport`). A genuinely-absent
    /// key yields `None` = no sound, not a fabricated fallback.
    pub chrono_out_sound: Option<String>,
    /// Rules+284/+288: ReadAudioVisual6691E0 reads the exact-case
    /// LetsDoTheTimeWarpOutAgain/InAgain keys at66A2D7..66A34F through
    /// ReadString128 then VocFind7514D0, retaining the previous valid ID.
    /// Constructor6659F4/FA stores -1. Use the fixed SOUNDMD binder.
    time_warp_out_sound: Option<String>,
    time_warp_in_sound: Option<String>,
    /// Rules+1C8: ctor6658E0 stores-1; ReadAudioVisual669645..66967B
    /// reads exact CheerSound through ReadString128/VocFind7514D0 with
    /// prior-ID retention. The fixed SOUNDMD binder owns name resolution.
    cheer_sound: Option<String>,
    /// General.MaximumCheerRate: ctor665D92=1, ReadGeneral66EBD4 signed/current-default.
    maximum_cheer_rate: i32,
    /// `[AudioVisual] ImpactWaterSound=` / `ImpactLandSound=`
    /// (`RulesClass+0x200` / `+0x204`, `RulesClass::ReadAudioVisual`
    /// `0x00669924` / `0x00669965`): a crash impact's fallback cue when the
    /// type names none (`FlyLocomotionClass::Process 0x004CD83E`/`0x004CD85C`).
    /// Stock: `ExplosionWaterLarge` and empty (silence).
    pub impact_water_sound: Option<String>,
    pub impact_land_sound: Option<String>,
    /// AudioVisual/SinkingSound -> Rules+208 (6699C8); constructor665940
    /// stores -1. Used only when the sinking type has no resolved sound.
    pub sinking_sound: Option<String>,
    /// `[AudioVisual] ScoreAnimSound=` (retail `ScoreEmblemSoundLoop`): the
    /// looping cue `Dialog0x103__DlgProc` starts when the slide-in completes
    /// (`0x006C93A0..0x006C93D4`, `VocClass__PlayGlobal` with the Rules ID),
    /// stopped by Continue and by dialog destruction. None = no cue.
    pub score_anim_sound: Option<String>,
    /// Rules+6C8 ctor665FE2=-1; AudioVisual Construction at66A97F..66A9B7
    /// uses ReadString128/Find7514D0 and keeps the prior index on invalid input.
    pub construction_sound: Option<String>,
    /// Rules+1C4 ctor6658DA=-1; AudioVisual BuildingRepairedSound at
    /// 669C68..669CA3 uses ReadString128/Find7514D0 with prior-ID retention.
    /// Techno701494 plays it after an Engineer restores a building's health.
    pub building_repaired_sound: Option<String>,
    /// `[AudioVisual] BombTickingSound=` (`RulesClass+0x20C`): the looping
    /// tick at a bombed object (`BombListClass::UpdateAll @ 0x00438BF0`).
    pub bomb_ticking_sound: Option<String>,
    /// `[AudioVisual] BombAttachSound=` (`RulesClass+0x210`): played at the
    /// bombed object for the planter's own player (`BombListClass::Attach @
    /// 0x00438FD7`).
    pub bomb_attach_sound: Option<String>,
    /// Interval in minutes between low-power degradation damage ticks on Powered=yes buildings.
    /// Parsed from DamageDelay= in [General]. Default 1.0 minute.
    pub damage_delay_minutes: f32,
    /// `[General] SpyPowerBlackout=` (`RulesClass+0xD64`, ReadInt
    /// `0x0066FBF1..0x0066FC0B`; the constructor's 0, `0x0066680A`): the
    /// frames the owner of an infiltrated power plant blacks out for
    /// (`BuildingClass::Infiltrate`, `0x004572DB`). Retail sets 1000.
    pub spy_power_blackout: i32,
    /// `[General] SpyMoneyStealPercent=` (`RulesClass+0xD68`, ReadDouble
    /// stored as a float, `0x0066FC17..0x0066FC30`; the constructor's
    /// `0x3F000000`, 0.5, `0x00666810`): the share of its owner's money an
    /// infiltrated `Storage=` building yields (`0x0045743F`). Retail sets .5.
    pub spy_money_steal_percent: NativeF32Bits,
    /// Fire/smoke anim types spawned on buildings below ConditionYellow health.
    /// Parsed from DamageFireTypes= in [General]. Default: FIRE01,FIRE02,FIRE03.
    pub damage_fire_types: Vec<AnimRef>,
    /// Particle system spawned by exploding barrels.
    /// Parsed from `BarrelParticle=` in `[General]` (NOT `[AudioVisual]`,
    /// despite the proximity to other AudioVisual keys).
    /// Holds the unresolved section name; ID resolution against the
    /// particle-system registry is deferred (matches A2/A3/A4/A5a pattern).
    pub barrel_particle: Option<String>,
    /// Rules+54, ctor6656D1 null; General66D6A8..66D6D5 retains the
    /// Anim factory result on missing/empty reads. AreaDamage48A34D.
    pub barrel_explode: Option<String>,
    /// Rules+58, ctor6656C0 empty; General66D6E2..66D7B1 preserves
    /// ordered Voxel factory results. AreaDamage stops at its first passing trial.
    pub barrel_debris: Vec<String>,

    // -- Harvester scan radii and economy --
    /// `Rules+0x1778`, `[General] TiberiumShortScan=` in leptons: the ring
    /// bound of Mission_Harvest state 1's next-cell scan and its full-cargo
    /// archive scan (`0x0073E9F1`, `0x0073EAA6`, `>> 8` to cells). Read by
    /// `RulesClass::ReadGeneral` at `0x0067028C..0x006702A6` through
    /// `CCINIClass::ReadRange 0x00474620` (cells x 256, chopped) over the
    /// current value; the constructor writes `0x600` (`0x00667638`). Retail
    /// `6` is 1536.
    pub tiberium_short_scan: i32,
    /// `Rules+0x177C`, `[General] TiberiumLongScan=` in leptons: the ring
    /// bound of Mission_Harvest state 0's ore search (`0x0073E851`). Read
    /// like TiberiumShortScan (`0x006702AC..0x006702C5`); the constructor
    /// writes `0x2000` (`0x00667642`). Retail `48` is 12288.
    pub tiberium_long_scan: i32,
    /// `Rules+0x1518`, `[General] BuildupTime=` in minutes: how long a
    /// building's build-up (and pack-up) animation runs, spread over its
    /// Buildup SHP's frames (`rules::buildup_asset_catalog`).
    /// `RulesClass::ReadGeneral` reads it through `CCINIClass::ReadDouble
    /// 0x005283D0` at `0x00670CB5`; the constructor writes `.05`
    /// (`0x3FA999999999999A`, `0x006673B3..0x006673BD`). Retail `.06`.
    pub buildup_time: f64,
    /// `Rules+0x1780`, `[General] SlaveMinerShortScan=` in leptons: a deployed
    /// slave refinery with ore this close keeps working where it stands.
    /// `RulesClass::ReadGeneral` reads it through `CCINIClass::ReadRange
    /// 0x00474620` at `0x006702E0`; the constructor writes `0x500`
    /// (`0x0066764C`). Retail `8` is 2048.
    pub slave_miner_short_scan: i32,
    /// `Rules+0x1784`, `[General] SlaveMinerSlaveScan=` in leptons: how far a
    /// slave looks for ore (SlaveManagerClass AI_Update state 1,
    /// `0x006AF748`). ReadRange at `0x00670300`; constructor `0x1000`
    /// (`0x00667656`). Retail `14` is 3584.
    pub slave_miner_slave_scan: i32,
    /// `[General] DrainMoneyFrameDelay=` (`Rules+0x314`, stock 30). The drained
    /// object's `TechnoClass::AI_Update @ 0x006FA167..0x006FA17B` transfers
    /// money only on frames where `g_CurrentFrameCounter % delay == 0`
    /// (signed `IDIV`, remainder test). Constructor default UNCHECKED.
    pub drain_money_frame_delay: i32,
    /// `[General] DrainMoneyAmount=` (`Rules+0x318`, stock 30). Per transfer
    /// `min(amount, Available_Money(victim))` moves from the victim's wallet
    /// (`Spend_Money @ 0x004F9790`) to the drainer's (`Add_Credits @
    /// 0x004F9950`) at `0x006FA183..0x006FA1C0`. Constructor default UNCHECKED.
    pub drain_money_amount: i32,
    /// `Rules+0x1788`, `[General] SlaveMinerLongScan=` in leptons: how far a
    /// slave miner looks for a field. ReadRange at `0x0067031F`; constructor
    /// `0x5000` (`0x00667660`). Retail `48` is 12288.
    pub slave_miner_long_scan: i32,
    /// `Rules+0x178C`, `[General] SlaveMinerScanCorrection=` in leptons: how
    /// much closer to ore a new spot must be before a deployed slave miner
    /// moves. ReadRange at `0x0067033F`; constructor `0x300` (`0x0066766A`).
    pub slave_miner_scan_correction: i32,
    /// `Rules+0x1790`, `[General] SlaveMinerKickFrameDelay=`: frames an idle
    /// slave miner waits before it looks for a field again. ReadInt at
    /// `0x0067035F`; constructor `0x7FFFFFFF` (`0x00667674`). Retail 150.
    pub slave_miner_kick_frame_delay: i32,
    /// `Rules+0xF0`, `[General] MaximumQueuedObjects=`: how many builds a
    /// factory's queue holds behind its active one; `FactoryClass::
    /// StartProduction` refuses an append at this count (`0x004C9CDE`).
    /// `RulesClass::ReadGeneral` reads it through `CCINIClass::ReadInt
    /// 0x005276D0` at `0x00671DA7`; the constructor writes 5 (`0x006656EE` →
    /// `0x006657CD`).
    pub maximum_queued_objects: i32,
    /// Phobos `[General] BuildingProductionQueue=` (default false): a
    /// BuildingType PRODUCE queues behind a running or completed building
    /// instead of being dropped or abandoning the held product. The next
    /// building starts only after the held one is placed.
    #[cfg(feature = "phobos")]
    pub building_production_queue: bool,
    /// Phobos `[General] BuildingTypeSelectable=` (default false): type select
    /// admits every building, not only the `UndeploysInto=` ones the retail ID
    /// match admits (`0x00732C61..0x00732C83`). Phobos disables the switch
    /// without Ares (`BuildingClass_Is1x1AndUndeployable_BuildingMassSelectable`).
    #[cfg(all(feature = "phobos", feature = "ares"))]
    pub building_type_selectable: bool,
    /// Phobos `[General] AIFireSale=` (default true): whether a computer
    /// house on its last legs (`0x004FD879..0x004FD904`, no live
    /// `Factory=` building) sells every building.
    #[cfg(feature = "phobos")]
    pub ai_fire_sale: bool,
    /// Phobos `[General] AIFireSaleDelay=` in frames (default 0): the first
    /// last-legs tick starts this timer instead of selling; the sale waits
    /// until it completes. Non-positive sells at once.
    #[cfg(feature = "phobos")]
    pub ai_fire_sale_delay: i32,
    /// Phobos `[General] AIAllToHunt=` (default true): whether the same
    /// last-legs block sends every techno to hunt.
    #[cfg(feature = "phobos")]
    pub ai_all_to_hunt: bool,
    /// Phobos `[General] GatherWhenMCVDeploy=` (default true): whether a
    /// computer house's yard deploy runs
    /// `HouseClass::MoveGroundForcesToBase @ 0x0050C920` (`0x00739926`).
    #[cfg(feature = "phobos")]
    pub gather_when_mcv_deploy: bool,
    /// Phobos `[AudioVisual] TimerBlinkColorScheme=`: the runtime ColorScheme
    /// a finished superweapon timer blinks in, `[Colors]` entry `R / 2`, or -1
    /// for `none`. Constructor 5, the third `[Colors]` entry the game uses.
    #[cfg(feature = "phobos")]
    pub timer_blink_color_scheme: i32,
    /// `Rules+0xD78`, `[General] HarvesterTooFarDistance=` in cells: a
    /// refinery farther than this is approached before the dock is reserved.
    /// `RulesClass::ReadGeneral` reads it through `CCINIClass::ReadInt
    /// 0x005276D0` at `0x0066FFEB`; the constructor writes 5 (`0x00666835`).
    pub harvester_too_far_distance: i32,
    /// `Rules+0xDF8`, `[General] ApproachTargetResetMultiplier=`: read through
    /// `INIClass::ReadInt 0x005276D0` at `0x00670150` (retail `1.5` is 1);
    /// constructor 1. A returning slave re-paths when its NavCom has drifted
    /// more than this many cells from the drop cell (SlaveManagerClass
    /// AI_Update state 4, `0x006AFB13`).
    pub approach_target_reset_multiplier: i32,
    /// `Rules+0xD7C`, `[General] ChronoHarvTooFarDistance=` in cells, the
    /// Chrono Miner's threshold. Read through `ReadInt` at `0x0067000B`; the
    /// constructor writes 50 (`0x00666846`).
    pub chrono_harv_too_far_distance: i32,

    // -- Harvester timing --
    /// `Rules+0x1520`, `[General] HarvesterLoadRate=`: the StageClass rate
    /// Mission_Harvest state 1 and Harvest_Ore_Tick arm (`0x0073E951`,
    /// `0x0073D5D1`); state 1 cuts when the stage reaches 9. Read by
    /// `RulesClass::ReadGeneral` at `0x00670CE7..0x00670D01` through
    /// `CCINIClass::ReadInt 0x005276D0`; the constructor writes 2
    /// (`0x006671CD`, `0x006673C7`). Retail leaves it unset.
    pub harvester_load_rate: i32,
    /// Whole-frame dump gate for refinery unloading (HarvesterDumpRate=).
    /// The unload accumulator advances one whole frame per unloading tick and a
    /// slot drains once it reaches this threshold; gamemd's gate is the full
    /// `HarvesterDumpRate(double) × 900 <= accumulator`. Because the accumulator
    /// is integer-stepped, the first crossing is exactly `ceil(rate × 900)`, so
    /// storing the ceiling (not a tenths-quantized value) reproduces gamemd's
    /// crossing bit-for-bit with no float in the sim gate.
    /// Default 15 (from ceil(0.016 × 900) = ceil(14.4) = 15 frames per gate):
    /// the RulesClass constructor stores the double 0.016 at Rules+0x1528
    /// (`0x006673CD..0x006673DC`) and ReadDouble at `0x00670CD4` keeps it when
    /// the key is absent, as it is in retail RULESMD.INI.
    pub harvester_dump_frames: u16,

    // -- Chrono warp delay constants --
    /// Post-warp lock duration in game frames (ChronoDelay= in [General]).
    /// Applied after Chronosphere warp. Default 60 frames.
    pub chrono_delay: i32,
    /// Chrono reinforcement warp delay in game frames (ChronoReinfDelay= in [General]).
    /// Default 180 frames.
    pub chrono_reinf_delay: i32,
    /// Distance divisor for warp delay: delay = distance_leptons / factor
    /// (ChronoDistanceFactor= in [General]). Default 48.
    pub chrono_distance_factor: i32,
    /// Whether warp delay scales with distance (ChronoTrigger= in [General]).
    /// If false, always use ChronoMinimumDelay. Default true.
    pub chrono_trigger: bool,
    /// Minimum warp delay in game frames (ChronoMinimumDelay= in [General]).
    /// Floor for the distance-based calculation. Default 16 frames.
    pub chrono_minimum_delay: i32,
    /// Distance (leptons) below which delay is forced to minimum
    /// (ChronoRangeMinimum= in [General]). Default 0.
    pub chrono_range_minimum: i32,

    /// Rules+F3C: PurifierBonus ReadDouble66FC70 -> FSTP float66FC75.
    /// Caller73E42B/522DCD multiplies count, these bits, then cargo amount.
    pub purifier_bonus: NativeF32Bits,
    /// AI virtual purifier counts indexed by difficulty
    /// (AIVirtualPurifiers= in [General]). Each entry is added to the AI
    /// player's real purifier count when computing the deposit bonus. INI
    /// convention is hardest-first, so the array is `[Brutal, Medium, Easy]`
    /// with the retail default `[4, 2, 0]`.
    pub ai_virtual_purifiers: [i32; 3],
    /// `[General] DisabledDisguiseDetectionPercent=`: an IntVector
    /// (`Rules+0xE10`, read at `0x0067020F` through `0x00475D70` with the
    /// field as its default; the constructor leaves it empty), indexed by the
    /// attacking House's difficulty (`+0x184`, hardest first). A computer
    /// attacker detects a blinking disguise when `RandomRanged(0, 99)` does
    /// not exceed its entry (Evaluate_Candidate `0x006F851C..0x006F854B`).
    pub disabled_disguise_detection_percent: Vec<i32>,

    // -- Survivor spawning on sell/destroy --
    /// Divisor to compute survivor count for Allied buildings (AlliedSurvivorDivisor= in [General]).
    /// RulesClass667371 default200; HowManySurvivors451330 clamps the
    /// signed refund/divisor quotient to1..5 (zero divisor suppresses crew).
    pub allied_survivor_divisor: i32,
    /// Divisor to compute survivor count for Soviet buildings (SovietSurvivorDivisor= in [General]).
    /// RulesClass66737B default100; stock RULESMD explicitly supplies250.
    pub soviet_survivor_divisor: i32,
    /// Divisor to compute survivor count for Third-side (Yuri) buildings (ThirdSurvivorDivisor= in [General]).
    /// RulesClass667385 default300; stock RULESMD explicitly supplies750.
    pub third_survivor_divisor: i32,
    /// `AlliedCrew=`/`SovietCrew=`/`ThirdCrew=` (Rules `+0xF78/+0xF7C/+0xF80`),
    /// `Technician=` (`+0xF6C`) and `Engineer=` (`+0xF70`): the InfantryTypes
    /// `TechnoClass::GetCrew @ 0x00707D20` and the building crew pick
    /// `0x0044EB10` return. Stock E1, E2, INIT, CTECH and ENGINEER.
    pub allied_crew: Option<String>,
    pub soviet_crew: Option<String>,
    pub third_crew: Option<String>,
    pub technician: Option<String>,
    pub engineer_infantry: Option<String>,
    /// Rules+17F8: constructor667793 initializes1.0f; General671DF7
    /// reads EngineerCaptureLevel with ReadDouble and stores an f32. The
    /// Infantry object-action threshold is independent of arrival repair.
    pub engineer_capture_level: NativeF32Bits,
    /// `CrewEscape=` (Rules `+0x5C0`, `ReadDouble`), the chance a crewed
    /// vehicle's crew escapes. Constructor default 0.5 (`0x00665E11..0x00665E17`).
    pub crew_escape: crate::util::native_x87::NativeF64Bits,
    /// `RefundPercent=` (Rules `+0x1738`, `ReadDouble`), the human-owner refund
    /// share `TechnoTypeClass::GetRefund @ 0x00711F60` applies. Constructor
    /// default 0.5 (`0x006675CE..0x006675D4`, ECX set at `0x00667190`).
    pub refund_percent: crate::util::native_x87::NativeF64Bits,
    /// `ShipSinkingWeight=` (Rules `+0x630`, `ReadDouble` at `0x0066F174`;
    /// constructor default 3.0 at `0x00665EE8`). A surface naval unit at
    /// least this `Weight=` sinks on water instead of exploding
    /// (`0x00737E00..0x00737E16`). Parsed like `Weight=` so the two compare
    /// as native doubles do on stock values (3.0 against 1..5).
    pub ship_sinking_weight: SimFixed,

    // -- Cliff/slope movement coefficients ([General]) --
    // Rules +0x768/+0x770/+0x778/+0x780, each `ReadDouble` over its current
    // value (0x66F213..0x66F2A9); the constructor stores 1.0 in all four
    // (0x6660BE..0x6660ED). Read by the Drive/Ship fresh speed publish
    // (0x4B3D4C..0x4B3DA6) and the per-cell speed chain.
    /// Tracked vehicle uphill coefficient (`TrackedUphill=`).
    pub tracked_uphill: NativeF64Bits,
    /// Tracked vehicle downhill coefficient (`TrackedDownhill=`).
    pub tracked_downhill: NativeF64Bits,
    /// Non-tracked (wheeled and other) vehicle uphill coefficient (`WheeledUphill=`).
    pub wheeled_uphill: NativeF64Bits,
    /// Non-tracked vehicle downhill coefficient (`WheeledDownhill=`).
    pub wheeled_downhill: NativeF64Bits,

    // -- Per-object draw-light offsets --
    /// Signed `[AudioVisual] ExtraUnitLight=` body-light offset (`1000 == 1.0`).
    pub extra_unit_light: i32,
    /// Signed `[AudioVisual] ExtraInfantryLight=` body-light offset (`1000 == 1.0`).
    pub extra_infantry_light: i32,
    /// Signed `[AudioVisual] ExtraAircraftLight=` draw offset (`1000 == 1.0`).
    pub extra_aircraft_light: i32,

    // -- Movement arrival --
    /// `Rules+0x1718`, `[General] CloseEnough=` in leptons: a blocked mover
    /// within this distance of its destination stops instead of repathing.
    /// Read by `RulesClass::ReadGeneral` at `0x00670EDD..0x00670EF7` through
    /// `CCINIClass::ReadRange 0x00474620` (cells x 256, chopped) over the
    /// current value; the constructor writes `0x280` (`0x00667588`). Retail
    /// `CloseEnough=2.25` is 576.
    pub close_enough: i32,
    /// `Rules+0x171C`, `[General] Stray=` in leptons: how far a team member
    /// may be from its team's centre or goal (`TeamClass::Regroup @
    /// 0x006EB870`, `Coordinate_Move @ 0x006EBAD0`). `ReadRange 0x00474620`
    /// at `0x00670E93` over the constructor's `0x200` (`0x00667592`). Retail
    /// `Stray=2.0` is 512.
    pub stray: i32,
    /// `Rules+0x1720`, `[General] RelaxedStray=` in leptons: `Stray=` for a
    /// team on script action 53 or 54. `ReadRange` at `0x00670EB2` over the
    /// constructor's `0x200` (`0x0066759C`). Retail `RelaxedStray=3.0` is 768.
    pub relaxed_stray: i32,
    /// `Rules+0x1724`, exact `[General] GuardModeStray=` in leptons.
    /// `ReadGeneral 0x00670EBD..0x00670EDD` uses `ReadRange 0x00474620`
    /// over the current value. Native construction does not write this field;
    /// Rust initializes the otherwise unspecified storage deterministically to0.
    /// Active retail's base General layer supplies2.0 (512 leptons).
    /// AreaGuard uses it only when its archived post is a Foot object
    /// (`0x004D6E74..0x004D6E89`), replacing the ordinary range-based leash.
    pub guard_mode_stray: i32,

    // -- Service depot / unit repair --
    /// `Rules+0x16E8`, `[General] URepairRate=` in minutes. ReadDouble
    /// at0x00670E4C over the current value, without a clamp. The constructor
    /// stores literal double .016 (0x3F90624DD2F1A9FC); an authored .016 is
    /// parsed as f32 and widened (0x3F90624DE0000000). Depot service compares
    /// progress against this retained value times900, without truncating it
    /// to a timer first (`0x0044BD32`, `0x0044BD44`).
    pub unit_repair_rate: f64,
    /// Rules+760; ctor6660AE stores3.0. ReadGeneral671858 uses the
    /// current double as ReadDouble5283D0 default, without scaling/clamping.
    pub charge_to_drain_ratio: NativeF64Bits,
    /// Hospital repair: Rules+16D8/16F0, ReadGeneral670DE9/670E57.
    pub infantry_repair_step: i32,
    pub infantry_repair_rate: f64,
    /// `[General] RepairStep=` — `RulesClass+0x16CC`, ReadInt over the
    /// constructor's 5 with no clamp (retail 8). TechnoTypeClass vt+0xB4
    /// (`0x00712120`) returns it: the health a repair tick adds, and the
    /// divisor of Strength in the repair step cost (`0x007120EC`).
    pub repair_step: i32,
    /// `[General] RepairPercent=` — `RulesClass+0x16D0`, ReadDouble
    /// (call `0x00670DBF`) over the constructor's .25 (retail `15%`, stored as
    /// 0x3FC3333333333333). The repair step cost multiplies the per-step
    /// share of the cost by it (`0x00712101`).
    pub repair_percent: f64,

    // -- Aircraft ammo reload --
    /// `[General] ReloadRate=` — `RulesClass+0x1508`, ReadDouble (call
    /// `0x00670C8E`) over the constructor's .05 (`0x0066738F`), in minutes.
    /// A dock's Mission_Repair returns `ftol(ReloadRate * 900)` after a visit
    /// that serviced a contact (`0x0044C92F`).
    pub reload_rate: f64,

    // -- Movement delay timers --
    /// Retained [AI] PathDelay double, in minutes (Rules+1760).
    /// Convert at the timer producer with the native PC53 multiply/ftol.
    pub path_delay: f64,
    /// Ticks to wait when blocked by a friendly unit before aggressive repath
    /// ([AI] BlockagePathDelay). Native signed dword, in frames directly.
    /// When this timer expires, the unit re-pathfinds with urgency=2 (scatter).
    pub blockage_path_delay_ticks: i32,
    /// [General] AIAutoDeployFrameDelay, native signed DynamicVector at
    /// Rules+E2C (data+E30), indexed Hard/Normal/Easy by Infantry52155C.
    /// Constructor666932..666961 leaves it empty; stock15,25,100 is authored.
    pub ai_auto_deploy_frame_delay: Vec<i32>,
    /// Phobos `[General] InfantryAutoDeploy` (`RulesExt`, default false): the
    /// default for the TechnoType key that re-enables the undeployed
    /// `Approach_Target` arm at `0x00522434`.
    #[cfg(feature = "phobos")]
    pub infantry_auto_deploy: bool,
    /// Phobos `[General] AllowDamageOnSelf` (`RulesExt`, default false): the
    /// fallback for a warhead's `Nullable<bool>` when the warhead section
    /// leaves it unset.
    #[cfg(feature = "phobos")]
    pub allow_damage_on_self: bool,
    /// Phobos `[General] DecloakDamagedTargets` (`RulesExt`, default true):
    /// the fallback for a warhead's `Nullable<bool>` when the warhead section
    /// leaves it unset.
    #[cfg(feature = "phobos")]
    pub decloak_damaged_targets: bool,
    /// Phobos `[CombatDamage] ROF.RandomDelay` (`RulesExt`,
    /// `PartialVector2D<int>`, default `{0, 2}`): the full rearm's jitter
    /// range for a weapon that leaves its own key unset.
    #[cfg(feature = "phobos")]
    pub rof_random_delay: crate::rules::ini_value::PartialIntPair,
    /// `[AI] AIForcePredictionFudge=`, the signed DynamicVector at `Rules+0x9A8`
    /// (items `+0x9AC`) indexed Hard/Normal/Easy by the House difficulty:
    /// the percent by which the computer's estimate of its enemy's forces
    /// may err (`sim::ai_base_defense`). ReadAI `0x006732D9..0x00673314`
    /// copies the vector before the `0x00475D70` reader; the constructor
    /// (`0x00666373..0x00666388`) leaves it empty. Retail: 5,25,80.
    pub ai_force_prediction_fudge: Vec<i32>,
    /// `[General] AIPickWallDefensePercent=`, the signed DynamicVector at
    /// `Rules+0xDD4` (items `+0xDD8`) indexed Hard/Normal/Easy by the House
    /// difficulty: the chance that a defense node walls a building instead
    /// (`sim::ai_base_building`). ReadGeneral `0x006700C6..0x006700F7` copies
    /// the vector before the `0x00475D70` reader; the constructor
    /// (`0x0066689C..0x006668CA`) leaves it empty. Retail: 50,25,10.
    pub ai_pick_wall_defense_percent: Vec<i32>,

    // -- Cell scatter eligibility (CellClass::Scatter_Objects) --
    /// `PlayerScatter=` from `[CombatDamage]` — when set, an *unforced* cell
    /// scatter dispatches to every occupant regardless of who owns it. Stock
    /// `rulesmd.ini:900` says `no`, and the RulesClass constructor also clears
    /// the byte, so ordinarily only elite occupants and AI-owned occupants
    /// respond to an unforced scatter.
    pub player_scatter: bool,
    /// `PlayerReturnFire=` from `[CombatDamage]` (`Rules+0x17EC`, read at
    /// `0x0066CEBD`). When set, a human's objects retaliate on every mission;
    /// unset (the constructor's value and stock `rulesmd.ini:899`), they do so
    /// only on Guard, Area Guard or Patrol (`ShouldRetaliate 0x007089F7`),
    /// and a human's building hit by a non-Aircraft source turns its `+0x388`
    /// at random instead of taking the source (`0x00442A1B`).
    pub player_return_fire: bool,
    /// `Scatter=` from `[IQ]` — the house IQ level at or above which an
    /// occupant answers an *unforced* cell scatter. Stock `rulesmd.ini:3164`
    /// says `2`; the RulesClass constructor default is `3`.
    pub iq_scatter: i32,
    /// `[IQ] MaxIQLevels` stamped onto ordinary skirmish AI houses.
    pub max_iq_levels: i32,
    /// Signed `[IQ] Production` threshold for the House-update AI activation
    /// transaction. Native accepts the constructor default `5` or the parsed
    /// dword verbatim, without clamping it to `MaxIQLevels`.
    pub iq_production: i32,
    /// `[IQ] Harvester=` (`Rules+0x1458`, ReadInt at `0x00674394`, constructor
    /// 3 at `0x006671F8`; retail 2): the house IQ from which a computer house
    /// replaces its harvesters (`sim::ai_unit_choice`).
    pub iq_harvester: i32,
    /// `[IQ] SuperWeapons=` (`Rules+0x1438`, ReadInt at `0x00674282..
    /// 0x006742A2` with the field as its default; constructor 4 at
    /// `0x006671BB`): in game mode 0 the house IQ from which a computer house
    /// fires its superweapons (`AI_Building_Strategy 0x004FD77C..0x004FD797`).
    pub iq_super_weapons: i32,
    /// `[IQ] GuardArea=` (`Rules+0x1440`, ReadInt at `0x006742C7..0x006742DB`
    /// with the field as its default; constructor 4 at `0x006671C7`; retail
    /// 2): the house IQ from which a computer infantryman outside a team
    /// takes Area Guard when idle (`0x0051CCD2..0x0051CCFC`).
    pub iq_guard_area: i32,
    /// `[IQ] RepairSell` outer gate for BuildingClass repair/sell AI.
    pub iq_repair_sell: i32,
    /// `[IQ] AutoCrush=` (`Rules+0x1448`, ReadInt at `0x00674315`;
    /// constructor 2 at `0x006671DC`; retail 2): the house IQ from which a
    /// computer unit charges a crushable attacker
    /// (`UnitClass::Should_Crush_It @ 0x007438F0`).
    pub iq_auto_crush: i32,
    /// `[IQ] SellBack` gate for the red-health low-credit sell decision.
    pub iq_sell_back: i32,
    /// `[AI] CreditReserve` threshold. A latched AI building is considered
    /// for sale only while its owner's credits are strictly below this value.
    pub credit_reserve: i32,

    // -- Lightning Storm superweapon constants --
    // `RulesClass::ReadGeneral` reads each with ReadInteger over its current
    // value and no clamp (`0x00670F75..0x0067104D`); the constructor seeds
    // them at `0x0066767E..0x006676B2`.
    /// `[General] LightningStormDuration=` (`+0x179C`, constructor 900):
    /// frames a storm rages; `-1` never ends.
    pub lightning_storm_duration: i32,
    /// `[General] LightningDamage=` (`+0x1798`, constructor 200).
    pub lightning_damage: i32,
    /// `[General] LightningDeferment=` (`+0x1794`, constructor 250): frames
    /// between the launch and the storm.
    pub lightning_deferment: i32,
    /// `[General] LightningHitDelay=` (`+0x17A0`, constructor 90): a cloud
    /// over the storm's cell every frame this divides.
    pub lightning_hit_delay: i32,
    /// `[General] LightningScatterDelay=` (`+0x17A4`, constructor 10): a
    /// scattered cloud every frame this divides.
    pub lightning_scatter_delay: i32,
    /// `[General] LightningCellSpread=` (`+0x17A8`, constructor 10): a
    /// scattered cloud lands up to half this many cells off the storm's cell
    /// on each axis.
    pub lightning_cell_spread: i32,
    /// `[General] LightningSeparation=` (`+0x17AC`, constructor 3): the
    /// Manhattan distance in cells a scattered cloud keeps from every cloud
    /// present.
    pub lightning_separation: i32,
    /// [General] LightningWarhead (+17B4), constructor null.
    /// Retained factory binding from General671053; empty string means null.
    pub lightning_warhead: String,
    /// [General] WeatherConBoltExplosion (+2F4), constructor null;
    /// selected by SelectAnim48A59A for LightningWarhead.
    pub weather_con_bolt_explosion: String,
    /// [General] WeaponNullifyAnim (+350), constructor null; retained reader
    /// 66E2AF. Bullet46A2A1 uses it after AreaDamage returns IronCurtain (2).
    pub weapon_nullify_anim: String,
    /// Phobos `[AudioVisual] MissingCameo=` (default `XXICON.SHP`): the file
    /// a cameo with no art falls back to. The extension selects the loader —
    /// Phobos replaces the type's loaded `XXICON.SHP` handle when the name is
    /// a `.shp`, and blits a `.pcx` at draw time.
    #[cfg(feature = "phobos")]
    pub missing_cameo: String,
    /// Whether `[AudioVisual] AmbientChangeRate=` passes the native ordered,
    /// nonzero gate before conversion. A nonzero mod value can chop to a
    /// zero-frame interval; NaN fails the original FCOMP/C3 gate.
    pub ambient_change_rate_nonzero: bool,
    /// Lightning/global ambient transition interval in native frames:
    /// `ftol(AmbientChangeRate * 900)`.
    pub ambient_change_interval_frames: i32,
    /// Signed ambient scalar delta: `ftol(AmbientChangeStep * 100)`.
    pub ambient_change_step: i32,
    /// `[CombatDamage] IronCurtainDuration=` (`RulesClass+0xFE8`, ReadInt at
    /// `0x0066C646` defaulting to the field, which the constructor zeroes at
    /// `0x00666BC4`): the Iron Curtain's curtain in frames. Retail 750.
    pub iron_curtain_duration: i32,
    /// `[General] IronCurtainInvokeAnim=` (`RulesClass+0x348`, ReadString
    /// 0x80 at `0x0066E24C`; empty keeps the constructor's null type,
    /// `0x00665B1A`): the anim the Iron Curtain's launch builds over its cell.
    /// Retail `IRONBLST`.
    pub iron_curtain_invoke_anim: String,
    /// `[General] ChronoPlacement=` (`RulesClass+0x330`, ReadString 0x80 at
    /// `0x0066E095`, empty keeps the constructor's null type): the anim the
    /// Chronosphere's first click loops over its source cell
    /// (`SuperClass::CreateChronoAnim @ 0x006CB3A0`). Retail `CHRONOAR`.
    pub chrono_placement_anim: String,
    /// `[General] ChronoBlast=` (`+0x328`, `0x0066E112`): the Chrono Warp's
    /// anim over the source cell (`0x006CC674`). Retail `CHRONOFD`.
    pub chrono_blast_anim: String,
    /// `[General] ChronoBlastDest=` (`+0x32C`, `0x0066E151`): the Chrono
    /// Warp's anim over the destination cell (`0x006CC61A`). Retail
    /// `CHRONOTG`.
    pub chrono_blast_dest_anim: String,
    /// `[General] DominatorWarhead=` (`RulesClass+0x2F8`, ReadString 0x80
    /// then the warhead lookup at `0x0066DF72`; empty keeps the
    /// constructor's null warhead): the Psychic Dominator's area damage
    /// warhead (`PsyDom::MindControlArea @ 0x0053B080`). Retail `DominatorWH`.
    pub dominator_warhead: String,
    /// `[General] DominatorFirstAnim=` (`+0x2FC`, `0x0066DFB1`): the anim
    /// `PsyDom::Start @ 0x0053AE50` raises over the target. Start does
    /// nothing unless both Dominator anims are set. Retail `PDFXCLD`.
    pub dominator_first_anim: String,
    /// `[General] DominatorSecondAnim=` (`+0x300`, `0x0066DFEF`): the anim
    /// MindControlArea places on the target cell. Retail `PDFXLOC`.
    pub dominator_second_anim: String,
    /// `[General] DominatorFireAtPercentage=` (`+0x304`, ReadInt at
    /// `0x0066E020`, constructor 50): the share of the first anim's frames
    /// after which the Dominator strikes (`PsychicDominator::Process
    /// @ 0x0053AF40`).
    pub dominator_fire_at_percentage: i32,
    /// `[General] DominatorCaptureRange=` (`+0x308`, `0x0066E040`,
    /// constructor 2): MindControlArea's cell-spread radius, capped at 10.
    pub dominator_capture_range: i32,
    /// `[General] DominatorDamage=` (`+0x30C`, `0x0066E05F`, constructor
    /// 50): MindControlArea's area damage.
    pub dominator_damage: i32,
    /// `[General] IonBlast=` (`RulesClass+0x298`), the animation the Genetic
    /// Mutator launch constructs (`SuperClass::Launch 0x006CD8A5`). Retail
    /// `RING1`. The constructor default is a null type: no key, no animation.
    pub ion_blast_anim: String,
    /// General IonBeam, Rules+29C: ctor665A24=null, reader66DCDD..66DD0D.
    pub ion_beam_anim: String,
    /// CombatDamage IonCannonDamage, Rules+490: ctor665CD4=700;
    /// ReadInt5276D0/current default at66BBF8..66BC12, unclamped.
    pub ion_cannon_damage: i32,
    /// General spotlight parameters, projected from the retained Process owner.
    pub spotlight: SpotlightRules,
    // --- ForceShield ([General], RulesClass::ReadGeneral) ---
    /// `ForceShieldRadius=` (`Rules+0x17B8`, `0x0067109F`), in cells; the
    /// constructor's 10 (`0x006676C8`).
    pub force_shield_radius: i32,
    /// `ForceShieldDuration=` (`+0x17BC`, `0x006710BE`), the shield's frames;
    /// the constructor's 400 (`0x006676CE`).
    pub force_shield_duration: i32,
    /// `ForceShieldBlackoutDuration=` (`+0x17C0`, `0x006710DE`), the
    /// launcher's power outage; the constructor's 800 (`0x006676DD`).
    pub force_shield_blackout_duration: i32,
    /// `ForceShieldPlayFadeSoundTime=` (`+0x17C4`, `0x006710FE`): how long
    /// before the shield ends its type's `SpecialSound=` plays; the
    /// constructor's 50 (`0x006676E7`).
    pub force_shield_fade_sound_time: i32,
    /// `ForceShieldInvokeAnim=` (`+0x34C`, `0x0066E282`). The constructor's
    /// null type (`0x00665B20`): no key, no animation.
    pub force_shield_invoke_anim: String,
    // --- PsychicReveal ([CombatDamage]) ---
    /// `[CombatDamage] PsychicRevealRadius=` (`Rules+0xFEC`, ReadInteger at
    /// `0x0066C665` over the constructor's 3, `0x00666BCA`): the radius
    /// Launch case 11 hands `MapClass::RevealArea @ 0x005678E0`, which
    /// reveals nothing for 0 and clamps 11 and more to 10. Retail 15.
    pub psychic_reveal_radius: i32,
    // --- GeneticConverter ([SpecialWeapons] + [General]) ---
    /// `[SpecialWeapons] MutateWarhead=` (`+0xF98`, read at `0x006690BB`
    /// through WarheadTypeClass::FindOrAllocate `0x0075E3B0`): the warhead
    /// Launch case 9's walk hands each infantryman, as the ordered Rules
    /// processor resolved it. The constructor's null (`0x00666B3A`) and the
    /// factory's null for `none`: empty.
    pub mutate_warhead: String,
    /// `[SpecialWeapons] MutateExplosionWarhead=` (`+0xF9C`, `0x006690FA`):
    /// the warhead of case 9's area damage. The constructor's null
    /// (`0x00666B40`): empty.
    pub mutate_explosion_warhead: String,
    /// `[SpecialWeapons] EMPulseWarhead=` (`Rules+0xFA0`): the constructor
    /// stores null at `0x00666B46`. `RulesClass::ReadSpecialWeapons`
    /// (`0x00668FB0`) loads the prior pointer before `ReadString`
    /// (`0x00528A10`, call at `0x00669138`); an empty read keeps it, while a
    /// name goes through `WarheadTypeClass::FindOrAllocate`
    /// (`0x0075E3B0`, call at `0x00669145`). Retained for native state parity;
    /// no gameplay consumer is claimed.
    pub empulse_warhead: Option<String>,
    /// `[General] MutateExplosion=` (`+0x17C8`, `0x00671125`): case 9 deals
    /// area damage instead of walking the 3x3 block. The constructor's false
    /// (`0x006676ED`); retail sets yes.
    pub mutate_explosion: bool,
    /// Ordered `[General] MetallicDebris=` AnimType references from the native
    /// ReadGeneral128/factory pass. Constructor default is empty, not retail's
    /// authored list. Mirrors Rules+0x140 (data) / +0x14C (count).
    pub metallic_debris: Vec<String>,
    /// `[General] WeatherConClouds=` (`+0x2BC`, items `+0x2C0`, count
    /// `+0x2CC`), the same reader as `metallic_debris`: the Lightning Storm's
    /// clouds.
    pub weather_con_clouds: Vec<String>,
    /// `[General] WeatherConBolts=` (`+0x2D8`, items `+0x2DC`, count
    /// `+0x2E8`): the storm's bolts; the first one's image sets the clouds'
    /// height.
    pub weather_con_bolts: Vec<String>,
}

/// `[General] URepairRate=` default, in minutes.
const U_REPAIR_RATE_MINUTES: f64 = 0.016;

/// One side's paradrop lists as `RulesClass::ReadGeneral` keeps them
/// (`0x0067062F..0x006707C1`), each emptied by the constructor
/// (`0x00666649..0x006666D8`) and kept by a pass that lacks its key.
///
/// The two are separate vectors: nothing here pairs them or compares their
/// lengths. Their consumers do (`superweapon::paradrop`, which pairs them by
/// index, and `superweapon::spy_plane`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParadropList {
    /// `*ParaDropInf=` as stored InfantryType IDs, through the InfantryType
    /// list reader (`0x0067BB10`: ReadString 0x80, `strtok(",")`, and
    /// `InfantryTypeClass::FindOrAllocate @ 0x00524CB0`, which drops `none`
    /// and `<none>` and allocates a name it does not know). The processed
    /// rules own it (`native_processing`).
    pub infantry: Vec<String>,
    /// `*ParaDropNum=` through the IntVector reader (`0x00475D70`).
    pub counts: Vec<i32>,
}

impl ParadropList {
    /// The counts as the projected INI holds them; the infantry are the
    /// processed rules' ([`RuleSet::from_processed_rules`]).
    fn read_counts(general: &crate::rules::ini_parser::IniSection, key: &str) -> Self {
        Self {
            infantry: Vec::new(),
            counts: general.read_int_list(key).unwrap_or_default(),
        }
    }
}

/// `RulesClass::ReadDifficulty @ 0x0066D270`'s `RepairDelay=` default, the
/// double nearest .02 (`0x3F947AE147AE147B`, pushed at `0x0066D317`).
const DIFFICULTY_REPAIR_DELAY_DEFAULT: f64 = 0.02;

/// The scalar projection of one original ReadDifficulty66D270 row. ContentScan
/// and its other AI flags belong to their existing AI admission owners.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DifficultyRules {
    pub firepower: f64,
    pub ground_speed: f64,
    pub air_speed: f64,
    pub armor: f64,
    pub rof: f64,
    pub cost: f64,
    pub build_time: f64,
    pub repair_delay: f64,
    pub build_delay: f64,
}

impl Default for DifficultyRules {
    fn default() -> Self {
        Self {
            firepower: 1.0,
            ground_speed: 1.0,
            air_speed: 1.0,
            armor: 1.0,
            rof: 1.0,
            cost: 1.0,
            build_time: 1.0,
            repair_delay: DIFFICULTY_REPAIR_DELAY_DEFAULT,
            build_delay: 0.03,
        }
    }
}

impl DifficultyRules {
    /// Called only for a present row section. Each ReadDouble uses a literal
    /// default, independent of the preceding Process pass. Native key order
    /// places BuildTime after RepairDelay/BuildDelay and ContentScan.
    pub(crate) fn read_pass(section: &crate::rules::ini_parser::IniSection) -> Self {
        let firepower = section.read_double("FirePower", 1.0);
        let ground_speed = section.read_double("Groundspeed", 1.0);
        let air_speed = section.read_double("Airspeed", 1.0);
        let armor = section.read_double("Armor", 1.0);
        let rof = section.read_double("ROF", 1.0);
        let cost = section.read_double("Cost", 1.0);
        let repair_delay = section.read_double("RepairDelay", DIFFICULTY_REPAIR_DELAY_DEFAULT);
        let build_delay = section.read_double("BuildDelay", 0.03);
        let build_time = section.read_double("BuildTime", 1.0);
        Self {
            firepower,
            ground_speed,
            air_speed,
            armor,
            rof,
            cost,
            build_time,
            repair_delay,
            build_delay,
        }
    }
}

/// The single BuildingTypes `RulesClass::ReadGeneral` names in `[General]`,
/// as stored IDs. The ordered Process owner (`native_processing`) reads them
/// pass by pass; `RuleSet` publishes its result.
///
/// The gates and WallTower (`Rules+0x86C..+0x87C`, read at
/// `0x0066F450..0x0066F583`) are the types `CellClass::Is_Clear_To_Build` lets
/// stand on their own house's wall overlay (`sim::build_site`): WallTower and
/// the two GDI gates over GASAND/GAWALL (`0x0047C8DD..0x0047C8F7`), the two
/// Nod gates over NAWALL (`0x0047C92F..0x0047C943`). Retail names GADUMY for
/// all five. The four power plants (`Rules+0x89C..+0x8A8`, read at
/// `0x0066F692..0x0066F781`) are the plant the computer's building choice
/// splices into its BasePlan (`sim::ai_base_building`); retail GAPOWR, NAPOWR,
/// NANRCT and YAPOWR.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GeneralBuildingTypes {
    pub gdi_gate_one: Option<String>,
    pub gdi_gate_two: Option<String>,
    pub nod_gate_one: Option<String>,
    pub nod_gate_two: Option<String>,
    pub wall_tower: Option<String>,
    /// `GDIPowerPlant=` (`Rules+0x89C`).
    pub gdi_power_plant: Option<String>,
    /// `NodRegularPower=` (`Rules+0x8A0`).
    pub nod_regular_power: Option<String>,
    /// `NodAdvancedPower=` (`Rules+0x8A4`).
    pub nod_advanced_power: Option<String>,
    /// `ThirdPowerPlant=` (`Rules+0x8A8`).
    pub third_power_plant: Option<String>,
}

impl GeneralBuildingTypes {
    /// `type == Rules+0x87C || +0x86C || +0x870`.
    pub fn stands_on_gdi_wall(&self, type_id: &str) -> bool {
        [&self.wall_tower, &self.gdi_gate_one, &self.gdi_gate_two]
            .into_iter()
            .any(|name| {
                name.as_deref()
                    .is_some_and(|name| name.eq_ignore_ascii_case(type_id))
            })
    }

    /// `type == Rules+0x874 || +0x878`.
    pub fn stands_on_nod_wall(&self, type_id: &str) -> bool {
        [&self.nod_gate_one, &self.nod_gate_two]
            .into_iter()
            .any(|name| {
                name.as_deref()
                    .is_some_and(|name| name.eq_ignore_ascii_case(type_id))
            })
    }
}

/// Retained `[AudioVisual]` frame-rate thresholds. The RulesClass constructor
/// writes 15/20/5 at +0/+4/+8 (665665..665672). ReadAudioVisual66920D..669258
/// reads signed integers in this order with current defaults and no clamps.
/// Native constructor/full-reader controls: rules_oracle/weapon_laser.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct DetailRules {
    pub min_frame_rate_normal: i32,
    pub min_frame_rate_movie: i32,
    pub buffer_zone_width: i32,
}

impl Default for DetailRules {
    fn default() -> Self {
        Self {
            min_frame_rate_normal: 15,
            min_frame_rate_movie: 20,
            buffer_zone_width: 5,
        }
    }
}

impl DetailRules {
    pub(crate) fn read_pass(self, audio_visual: &IniSection) -> Self {
        Self {
            min_frame_rate_normal: audio_visual
                .read_int("DetailMinFrameRateNormal", self.min_frame_rate_normal),
            min_frame_rate_movie: audio_visual
                .read_int("DetailMinFrameRateMovie", self.min_frame_rate_movie),
            buffer_zone_width: audio_visual
                .read_int("DetailBufferZoneWidth", self.buffer_zone_width),
        }
    }
}

/// The `[General]` Prism support keys, read by `RulesClass::ReadGeneral`
/// (`0x0066D530`) on every rules pass that has a `[General]` section, each
/// with its current value as the default (`0x0067114F..0x006711B8`). The
/// beam's `PrismSupportDuration=` (`Rules+0x4A8`) feeds the support laser;
/// `PrismSupportHeight=` (`Rules+0x4AC`) has no
/// reader outside the constructor and ReadGeneral.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct PrismSupportRules {
    /// `PrismSupportModifier=` (`Rules+0x49C`), the percent each supporter
    /// adds to the shot. Original gamemd uses the scaled integer as the
    /// ReadDouble default; Ares converts that default back to a fraction.
    pub modifier: i32,
    /// `PrismSupportMax=` (`Rules+0x4A0`), the supporters one shot recruits.
    pub max: i32,
    /// `PrismSupportDelay=` (`Rules+0x4A4`), a supporter's downtime in frames.
    pub delay: i32,
    /// `PrismSupportDuration=` (`Rules+0x4A8`), signed support-laser lifetime
    /// in frames. ReadInt retains the current value; no clamp or byte cast.
    pub duration: i32,
}

impl Default for PrismSupportRules {
    /// The RulesClass constructor's values (`0x00665CF3`, `0x00665CF9`,
    /// `0x00665D03`).
    fn default() -> Self {
        Self {
            modifier: 100,
            max: 8,
            delay: 100,
            duration: 15,
        }
    }
}

impl PrismSupportRules {
    /// One ReadGeneral pass over a `[General]` section. The modifier is
    /// `fmul qword 100.0` (`0x0067116E`) on the value ReadDouble returns, then
    /// ftol (`0x007C5F00`), under the game's masked chop control word, which
    /// also chops ReadDouble's own percent product: a modded `35%` reads 34.
    /// Ares fixes the default's units before that read (see below).
    pub(crate) fn read_pass(self, general: &crate::rules::ini_parser::IniSection) -> Self {
        use crate::util::native_x87::{MaskedX87Chop53 as X87, NativeF64Bits};
        let current = f64::from(self.modifier);
        // Ares RulesClass_Addition_General_PrismSupportModifier @ 0x1005D040,
        // hook0x00671152: FILD Rules+49C; FDIV 100.0; pass that fraction to
        // ReadDouble. Without this, each keyless map/mode multiplies the
        // retained percentage by another 100 (original ReadGeneral67114F).
        #[cfg(feature = "ares")]
        let current = f64::from_bits(
            X87::store_f64_masked_chop(X87::div(
                X87::load_f64(NativeF64Bits::from_bits(current.to_bits())),
                X87::load_f64(NativeF64Bits::from_bits(100.0_f64.to_bits())),
            ))
            .bits(),
        );
        let percent = general.read_double("PrismSupportModifier", current);
        let modifier = X87::ftol_i32_low_masked(X87::mul(
            X87::load_f64(NativeF64Bits::from_bits(percent.to_bits())),
            X87::load_f64(NativeF64Bits::from_bits(100.0_f64.to_bits())),
        ));
        Self {
            modifier,
            max: general.read_int("PrismSupportMax", self.max),
            delay: general.read_int("PrismSupportDelay", self.delay),
            duration: general.read_int("PrismSupportDuration", self.duration),
        }
    }
}

/// RulesClass665650, stores667371/66737B/667385 at+14F8/+14FC/+1500.
/// Original gamemd; ReadGeneral670C13..670C6C keeps prior values.
pub(crate) const DEFAULT_SURVIVOR_DIVISORS: [i32; 3] = [200, 100, 300];
/// RulesClass665D92 initializes retained General.MaximumCheerRate (+580).
pub(crate) const DEFAULT_MAXIMUM_CHEER_RATE: i32 = 1;
/// Phobos `RulesExt`'s `MissingCameo` constructor value: the stock
/// `XXICON.SHP` a type with no cameo art falls back to.
#[cfg(feature = "phobos")]
pub(crate) const DEFAULT_MISSING_CAMEO: &str = "XXICON.SHP";
/// RulesClass665650's literal AmbientChangeRate (0.2) and AmbientChangeStep
/// (0.1) doubles, before the AudioVisual authored reads.
pub(crate) const DEFAULT_AMBIENT_CHANGE_RATE: NativeF64Bits =
    NativeF64Bits::from_bits(0x3fc9_9999_9999_999a);
pub(crate) const DEFAULT_AMBIENT_CHANGE_STEP: NativeF64Bits =
    NativeF64Bits::from_bits(0x3fb9_9999_9999_999a);

pub(crate) const fn default_fog_rate() -> NativeF64Bits {
    // RulesClass constructor66740C/667416, before any authored f32 read.
    NativeF64Bits::from_bits(0x3fa9_9999_9999_999a)
}

pub(crate) const fn default_shroud_rate() -> NativeF64Bits {
    // RulesClass constructor6673FC/667402, independent of retail tuning.
    NativeF64Bits::from_bits(0x4010_0000_0000_0000)
}

/// Serde-only placeholder for the Phobos `MissingCameo` retained string: a
/// saved state written without the key reads back as the constructor value.
#[cfg(feature = "phobos")]
pub(crate) fn default_missing_cameo() -> String {
    DEFAULT_MISSING_CAMEO.to_owned()
}

impl Default for GeneralRules {
    fn default() -> Self {
        let (ambient_change_rate_nonzero, ambient_change_interval_frames, ambient_change_step) =
            ambient_change_terms(
                f64::from_bits(DEFAULT_AMBIENT_CHANGE_RATE.bits()),
                f64::from_bits(DEFAULT_AMBIENT_CHANGE_STEP.bits()),
            );
        Self {
            max_waypoint_path_length: 15,
            talk_bubble_time: 300,
            flash_frame_time: 7,
            radar_combat_flash_time: 21,
            detail: DetailRules::default(),
            pose_dir: 0,
            deploy_dir: 0,
            scroll_multiplier: 0.07,
            // RulesClass__Constructor @ 0x00665650 writes the double
            // 0x3F9EB851EB851EB8 to +0x14C8.
            savour_delay_minutes: 0.03,
            // RulesClass665650 initializes +16B8 before any AudioVisual read.
            gravity: 3,
            wind_direction: -1,
            veteran_sight: 0.0,
            veteran_combat: 1.0,
            veteran_speed: 1.0,
            veteran_rof: 1.0,
            difficulty_rows: [DifficultyRules::default(); 3],
            game_speed_bias: 1.0,
            veteran_armor: 1.0,
            curley_shuffle: false,
            enable_ai_build_limitation: false,
            disable_parallel_ai_queues: false,
            repair_rate_minutes: 0.016,
            // Rules665696..6656B0 initializes the current-default fields
            // before ReadGeneral66E6E2..66E74B. Retail overrides are separate.
            self_heal_infantry_frames: 1000,
            self_heal_infantry_amount: 1,
            self_heal_unit_frames: 1000,
            self_heal_unit_amount: 1,
            veteran_ratio: VETERAN_RATIO_DEFAULT,
            veteran_cap: VETERAN_CAP_DEFAULT,
            computer_base_defense_response: 3,
            // Native Rules+0xE48 constructor default; active retail overrides to 3.
            maximum_building_placement_failures: 5,
            placement_delay: 0.05,
            attack_delay: 5.0,
            campaign_money_delta_easy: 5000,
            campaign_money_delta_hard: -5000,
            ai_alternate_production_credit_cutoff: 1000,
            ai_restrict_replace_time: 500,
            team_delays: Vec::new(),
            total_ai_team_cap: Vec::new(),
            minimum_ai_defensive_teams: Vec::new(),
            maximum_ai_defensive_teams: Vec::new(),
            use_min_defense_rule: true,
            fill_earliest_team_probability: Vec::new(),
            dissolve_unfilled_team_delay: 5000,
            ai_safe_distance: 8,
            patrol_scan_minutes: NativeF64Bits::from_bits(0x3f90_624d_d2f1_a9fc),
            ai_trigger_success_weight_delta: NativeF64Bits::ONE,
            ai_trigger_failure_weight_delta: NativeF64Bits::from_bits((-1.0_f64).to_bits()),
            ai_trigger_track_record_coefficient: NativeF64Bits::ONE,
            harvesters_per_refinery: Vec::new(),
            ai_minor_super_ready_percent: NativeF32Bits::from_bits(0x3f4c_cccd),
            base_defense_delay_minutes: 0.25,
            suspend_priority: 20,
            suspend_delay_minutes: 2.0,
            leptons_per_sight_increase: 0,
            gap_radius: 10,
            reveal_by_height: true,
            ally_reveal: true,
            allies_allowed: false,
            ally_change_allowed: true,
            comp_easy_bonus: true,
            paranoid: true,
            tunnel_speed: SimFixed::from_num(6.0_f32),
            missile_rot_var: 0.25,
            safety_altitude: 500,
            line_trail_color_override: [0; 3],
            chrono_beam_color: [0; 3],
            flight_level: 500,
            attacking_aircraft_sight_range: 5,
            display_cruise_height: 400, // Rules constructor665C3A
            // Rules constructor 0x00665E2B..0x00665E81.
            hover_height: 120,
            hover_bob: NativeF64Bits::from_bits(0x403e_0000_0000_0000),
            hover_boost: NativeF64Bits::from_bits(0x3ff4_cccc_cccc_cccd),
            hover_acceleration: NativeF64Bits::from_bits(0x3f9e_b851_eb85_1eb8),
            hover_brake: NativeF64Bits::from_bits(0x3f9e_b851_eb85_1eb8),
            hover_dampen: NativeF64Bits::from_bits(0x3fe9_9999_9999_999a),
            // RulesClass ctor666163/66616D initializes +7B8/+7BC.
            parachute_max_fall_rate: -3,
            no_parachute_max_fall_rate: -100,
            paradrop_radius: 1024,
            parachute_shp: None,
            vehicle_parachute: None,
            amer_paradrop: ParadropList::default(),
            ally_paradrop: ParadropList::default(),
            sov_paradrop: ParadropList::default(),
            yuri_paradrop: ParadropList::default(),
            anim_to_infantry: Vec::new(),
            base_unit_types: Vec::new(),
            multiplayer_ai_cm: Vec::new(),
            pad_aircraft_types: Vec::new(),
            separate_aircraft: false,
            prism_type: None,
            drop_zone_anim: None,
            drop_zone_radius: 1024,
            prerequisite_proc_alternate: None,
            prism_support: PrismSupportRules::default(),
            building_types: GeneralBuildingTypes::default(),
            tiberium_grows: true,
            tiberium_spreads: true,
            growth_rate_minutes: 2.0,
            // The RulesClass constructor's null pointers (+338..+344, +94 and
            // +BB4: `0x00665B02..0x00665B14`, `0x0066572F`, `0x006665CF`).
            warp_in: AnimRef {
                name: String::new(),
            },
            warp_out: AnimRef {
                name: String::new(),
            },
            warp_away: AnimRef {
                name: String::new(),
            },
            chrono_sparkle1: AnimRef {
                name: String::new(),
            },
            wake: AnimRef {
                name: String::new(),
            },
            move_flash: AnimRef {
                name: String::new(),
            },
            behind: None,
            // Slots 3/4/6..10 are the constructor's null pointers +9C..+B4
            // (`0x0066573B..0x0066575F`).
            infantry_death_anims: [
                None,
                None,
                None,
                None,
                None,
                Some("ELECTRO".to_string()),
                None,
                None,
                None,
                None,
                None,
            ],
            attack_cursor_on_disguise: false,
            default_mirage_disguises: Vec::new(),
            infantry_disguises: Default::default(),
            secret_lab_choices: Default::default(),
            infantry_blink_disguise_time: 0,
            tree_targeting: false,
            tree_strength: 25,
            condition_yellow: 0.5,
            condition_red: 0.5,
            cloaking_stages: 9,
            cloak_delay_frames: 0,
            cloak_sound: None,
            leave_bio_reactor_sound: None,
            enter_bio_reactor_sound: None,
            enter_grinder_sound: None,
            upgrade_veteran_sound: None,
            upgrade_elite_sound: None,
            slaves_free_sound: None,
            crate_money_sound: None,
            crate_reveal_sound: None,
            crate_fire_sound: None,
            crate_armour_sound: None,
            crate_speed_sound: None,
            crate_unit_sound: None,
            crate_promote_sound: None,
            elite_flash_timer: 0,
            idle_action_frequency: f64::from_bits(0x3fb5_3f7c_ed91_6873),
            force_shield_color: 0,
            laser_target_color: 0,
            #[cfg(feature = "phobos")]
            airstrike_line_color: [255, 0, 0],
            #[cfg(feature = "phobos")]
            airstrike_line_z_adjust: 0,
            iron_curtain_color: 0,
            berserk_color: 0,
            airstrike_abort_sound: None,
            airstrike_attack_voice: None,
            condition_red_sparking_probability: 0.02,
            condition_yellow_sparking_probability: 0.01,
            condition_red_spark_threshold: unit_draw_threshold(0.02),
            condition_yellow_spark_threshold: unit_draw_threshold(0.01),
            dumb_my_effectiveness_coefficient: 200.0,
            dumb_target_effectiveness_coefficient: 200.0,
            dumb_target_special_threat_coefficient: 200.0,
            dumb_target_strength_coefficient: 200.0,
            dumb_target_distance_coefficient: -1.0,
            my_effectiveness_coefficient_default: 200.0,
            target_effectiveness_coefficient_default: -200.0,
            target_special_threat_coefficient_default: 200.0,
            target_strength_coefficient_default: -200.0,
            target_distance_coefficient_default: -10.0,
            enemy_house_threat_bonus: 0.0,
            threat_per_occupant: 5,
            normal_targeting_delay: 27,
            dead_bodies: Vec::new(),
            guard_area_targeting_delay: 36,
            building_garrisoned_sound: None,
            building_abandoned_sound: None,
            sell_sound: None,
            base_under_attack_sound: None,
            building_die_sound: None,
            building_damage_sound: None,
            create_unit_sound: None,
            create_infantry_sound: None,
            create_aircraft_sound: None,
            disk_laser_charge_up: None,
            lightning_sounds: Vec::new(),
            credit_ticks: Vec::new(),
            storm_sound: None,
            // `RulesClass::Constructor @ 0x006676BC` writes AL, and the last
            // definition of EAX before it is `0x00667202 MOV EAX,0x1`.
            lightning_print_text: true,
            dig_sound: None,
            nuke_take_off: String::new(),
            ai_super_defense_probability: Vec::new(),
            ai_super_defense_distance: 10,
            ai_super_defense_frames: 25,
            ai_ion_cannon_values: AiIonCannonValues::default(),
            psychic_dominator_activate_sound: None,
            genetic_mutator_activate_sound: None,
            psychic_reveal_activate_sound: None,
            spy_plane_camera: None,
            spy_plane_camera_frames: 16,
            chute_sound: None,
            gui_main_button_sound: None,
            gui_move_in_sound: None,
            gui_move_out_sound: None,
            shell_button_slide_sound: None,
            generic_click_sound: None,
            guard_sound: None,
            stop_sound: None,
            radar_on: None,
            radar_off: None,
            movie_on: None,
            movie_off: None,
            spy_sat_activation_sound: None,
            spy_sat_deactivation_sound: None,
            voice_ifv_repair: None,
            start_planning_mode_sound: None,
            end_planning_mode_sound: None,
            add_planning_mode_command_sound: None,
            place_beacon_sound: None,
            scatter_sound: None,
            scold_sound: None,
            generic_beep_sound: None,
            gui_checkbox_sound: None,
            ore_twinkle: None,
            ore_twinkle_chance: 50,
            gui_build_sound: None,
            building_slam: None,
            gui_tab_sound: None,
            incoming_message_sound: None,
            message_char_typed_sound: None,
            system_error_sound: None,
            options_changed_sound: None,
            player_left_sound: None,
            player_joined_sound: None,
            game_forming_sound: None,
            game_closed_sound: None,
            message_delay_minutes: crate::util::native_x87::NativeF64Bits::from_bits(
                0x3FE3_3333_3333_3333,
            ),
            shroud_grow: false,
            shroud_rate: default_shroud_rate(),
            fog_rate: default_fog_rate(),
            speak_delay_minutes: 0.0,
            gui_combo_open_sound: None,
            gui_combo_close_sound: None,
            gui_open_sound: None,
            gui_close_sound: None,
            bunker_walls_down_sound: None,
            bunker_walls_up_sound: None,
            direct_rocking_coefficient: SimFixed::lit("1.5"),
            fallback_coefficient: SimFixed::lit("0.1"),
            chrono_in_sound: Some("ChronoMinerTeleport".to_string()),
            chrono_out_sound: Some("ChronoMinerTeleport".to_string()),
            time_warp_out_sound: None,
            time_warp_in_sound: None,
            cheer_sound: None,
            maximum_cheer_rate: DEFAULT_MAXIMUM_CHEER_RATE,
            impact_water_sound: None,
            impact_land_sound: None,
            sinking_sound: None,
            score_anim_sound: None,
            construction_sound: None,
            building_repaired_sound: None,
            bomb_ticking_sound: None,
            bomb_attach_sound: None,
            damage_delay_minutes: 1.0,
            spy_power_blackout: 0,
            spy_money_steal_percent: NativeF32Bits::from_bits(0x3F00_0000),
            damage_fire_types: vec![],
            barrel_particle: None,
            barrel_explode: None,
            barrel_debris: Vec::new(),
            tiberium_short_scan: 0x600,
            tiberium_long_scan: 0x2000,
            buildup_time: f64::from_bits(0x3FA9_9999_9999_999A),
            slave_miner_short_scan: 0x500,
            slave_miner_slave_scan: 0x1000,
            drain_money_frame_delay: 30,
            drain_money_amount: 30,
            slave_miner_long_scan: 0x5000,
            slave_miner_scan_correction: 0x300,
            slave_miner_kick_frame_delay: 0x7FFF_FFFF,
            maximum_queued_objects: 5,
            #[cfg(feature = "phobos")]
            building_production_queue: false,
            #[cfg(all(feature = "phobos", feature = "ares"))]
            building_type_selectable: false,
            #[cfg(feature = "phobos")]
            ai_fire_sale: true,
            #[cfg(feature = "phobos")]
            ai_fire_sale_delay: 0,
            #[cfg(feature = "phobos")]
            ai_all_to_hunt: true,
            #[cfg(feature = "phobos")]
            gather_when_mcv_deploy: true,
            #[cfg(feature = "phobos")]
            timer_blink_color_scheme: 5,
            harvester_too_far_distance: 5,
            approach_target_reset_multiplier: 1,
            chrono_harv_too_far_distance: 50,
            harvester_load_rate: 2,
            harvester_dump_frames: 15,
            chrono_delay: 60,
            chrono_reinf_delay: 180,
            chrono_distance_factor: 48,
            chrono_trigger: true,
            chrono_minimum_delay: 16,
            chrono_range_minimum: 0,
            purifier_bonus: NativeF32Bits::from_bits(0x3E80_0000),
            ai_virtual_purifiers: [4, 2, 0],
            disabled_disguise_detection_percent: Vec::new(),
            allied_survivor_divisor: DEFAULT_SURVIVOR_DIVISORS[0],
            soviet_survivor_divisor: DEFAULT_SURVIVOR_DIVISORS[1],
            third_survivor_divisor: DEFAULT_SURVIVOR_DIVISORS[2],
            allied_crew: None,
            soviet_crew: None,
            third_crew: None,
            technician: None,
            engineer_infantry: None,
            engineer_capture_level: NativeF32Bits::ONE,
            crew_escape: crate::util::native_x87::NativeF64Bits::from_bits(0x3fe0_0000_0000_0000),
            refund_percent: crate::util::native_x87::NativeF64Bits::from_bits(
                0x3fe0_0000_0000_0000,
            ),
            ship_sinking_weight: SimFixed::lit("3.0"),
            // RulesClass constructor 0x6660BE..0x6660ED.
            tracked_uphill: NativeF64Bits::ONE,
            tracked_downhill: NativeF64Bits::ONE,
            wheeled_uphill: NativeF64Bits::ONE,
            wheeled_downhill: NativeF64Bits::ONE,
            extra_unit_light: 0,
            extra_infantry_light: 0,
            extra_aircraft_light: 0,
            // RulesClass constructor 0x00667588.
            close_enough: 0x280,
            stray: 0x200,
            relaxed_stray: 0x200,
            guard_mode_stray: 0,
            // Rules constructor667530/53A retains the double, not frame ticks.
            unit_repair_rate: U_REPAIR_RATE_MINUTES,
            charge_to_drain_ratio: NativeF64Bits::from_bits(0x4008_0000_0000_0000),
            // Constructor66751A and667540: 1 and double0.016.
            infantry_repair_step: 1,
            infantry_repair_rate: 0.016,
            repair_step: 5,
            repair_percent: 0.25,
            #[cfg(feature = "phobos")]
            prone_speed_crawls: 0.67,
            #[cfg(feature = "phobos")]
            prone_speed_no_crawls: 1.5,
            // RulesClass constructor 0x0066738F.
            reload_rate: 0.05,
            // PathDelay=.01 min = 0.6 sec = 9 ticks at 15 Hz.
            path_delay: 0.016,
            // BlockagePathDelay=60 frames (directly in frames, not minutes).
            blockage_path_delay_ticks: 60,
            ai_auto_deploy_frame_delay: Vec::new(),
            #[cfg(feature = "phobos")]
            infantry_auto_deploy: false,
            #[cfg(feature = "phobos")]
            allow_damage_on_self: false,
            #[cfg(feature = "phobos")]
            decloak_damaged_targets: true,
            #[cfg(feature = "phobos")]
            rof_random_delay: crate::rules::ini_value::PartialIntPair {
                low: 0,
                high: 2,
                values: 2,
            },
            ai_force_prediction_fudge: Vec::new(),
            ai_pick_wall_defense_percent: Vec::new(),
            // RulesClass constructor clears PlayerScatter and stores 3 into
            // [IQ] Scatter; stock rulesmd overrides the latter with 2.
            player_scatter: false,
            player_return_fire: false,
            iq_scatter: 3,
            max_iq_levels: 5,
            iq_production: 5,
            iq_harvester: 3,
            iq_super_weapons: 4,
            iq_guard_area: 4,
            iq_repair_sell: 3,
            iq_auto_crush: 2,
            iq_sell_back: 2,
            credit_reserve: 1000,
            cliff_back_impassability: 2,
            lightning_storm_duration: 900,
            lightning_damage: 200,
            lightning_deferment: 250,
            lightning_hit_delay: 90,
            lightning_scatter_delay: 10,
            lightning_cell_spread: 10,
            lightning_separation: 3,
            lightning_warhead: String::new(),
            weather_con_bolt_explosion: String::new(),
            weapon_nullify_anim: String::new(),
            #[cfg(feature = "phobos")]
            missing_cameo: DEFAULT_MISSING_CAMEO.to_owned(),
            ambient_change_rate_nonzero,
            ambient_change_interval_frames,
            ambient_change_step,
            iron_curtain_duration: 0,
            iron_curtain_invoke_anim: String::new(),
            chrono_placement_anim: String::new(),
            chrono_blast_anim: String::new(),
            chrono_blast_dest_anim: String::new(),
            dominator_warhead: String::new(),
            dominator_first_anim: String::new(),
            dominator_second_anim: String::new(),
            // RulesClass constructor (`0x00665A91..0x00665AB8`).
            dominator_fire_at_percentage: 50,
            dominator_capture_range: 2,
            dominator_damage: 50,
            ion_blast_anim: String::new(),
            ion_beam_anim: String::new(),
            ion_cannon_damage: 700,
            spotlight: SpotlightRules::default(),
            force_shield_radius: 10,
            force_shield_duration: 400,
            force_shield_blackout_duration: 800,
            force_shield_fade_sound_time: 50,
            force_shield_invoke_anim: String::new(),
            psychic_reveal_radius: 3,
            mutate_warhead: String::new(),
            mutate_explosion_warhead: String::new(),
            empulse_warhead: None,
            mutate_explosion: false,
            metallic_debris: Vec::new(),
            weather_con_clouds: Vec::new(),
            weather_con_bolts: Vec::new(),
        }
    }
}

/// Garrison/occupation combat rules parsed from `[CombatDamage]` in `rules(md).ini`.
/// These global multipliers govern how garrisoned infantry fire from buildings.
#[derive(Debug, Clone)]
pub struct GarrisonRules {
    /// Damage multiplier applied to garrison fire: the f32 at `Rules+0xF40`
    /// that FireAt multiplies on the x87 (`0x006FE3F1`).
    pub occupy_damage_multiplier: f32,
    /// `[CombatDamage] OccupyROFMultiplier=`, the single at `Rules+0xF44`
    /// (ReadDouble stored with `FSTP dword`, `0x0066C6A9..0x0066C6B4`;
    /// constructor 1.0f, `0x00666AB6`). GetROF divides a garrison's reload by
    /// it on the x87 when it is greater than zero (`0x006FD19C`).
    pub occupy_rof_multiplier: f32,
    /// Fixed weapon range in cells for garrisoned fire, replaces weapon's own range.
    pub occupy_weapon_range: i32,
    /// Damage multiplier for bunker passengers.
    pub bunker_damage_multiplier: f32,
    /// `[CombatDamage] BunkerROFMultiplier=`, the single at `Rules+0xF50`
    /// (`0x0066C712..0x0066C71D`; constructor 1.0f, `0x00666ACD`). GetROF
    /// divides a bunkered unit's reload by it when it is non-zero
    /// (`0x006FD1B1..0x006FD1EF`).
    pub bunker_rof_multiplier: f32,
    /// `[CombatDamage] BunkerWeaponRangeBonus=` cells, Rules+F54.
    /// Constructor666AC8..666AD3 stores2; reader66C723..66C73D uses the
    /// current signed value as its ReadInteger default, without a clamp.
    /// Executed constructor/reader controls: walk_cell_range_bunker.json.
    pub bunker_weapon_range_bonus: i32,
    /// `[CombatDamage] OpenToppedDamageMultiplier=`, the single at
    /// `Rules+0xF58` (constructor 1.0f `0x00666AD9`; ReadDouble stored with
    /// `FSTP dword` at `0x0066C743..`). FireAt multiplies an open-topped
    /// passenger's damage by it (`0x006FE43B`).
    pub open_topped_damage_multiplier: f32,
    /// `[CombatDamage] OpenToppedRangeBonus=` in cells, `Rules+0xF5C`
    /// (constructor 2, `0x00666AD9..`; ReadInt at `0x0066C743..0x0066C7AC`).
    /// InRange adds it, shifted to leptons, for an open-topped passenger
    /// (`0x006F72C8`).
    pub open_topped_range_bonus: i32,
}

impl Default for GarrisonRules {
    fn default() -> Self {
        Self {
            occupy_damage_multiplier: 1.0,
            occupy_rof_multiplier: 1.0,
            occupy_weapon_range: 5,
            bunker_damage_multiplier: 1.0,
            bunker_rof_multiplier: 1.0,
            bunker_weapon_range_bonus: 2,
            open_topped_damage_multiplier: 1.0,
            open_topped_range_bonus: 2,
        }
    }
}

/// `[ElevationModel]`: the range bonus `TechnoClass::InRange` gives a shot
/// fired down at a lower target (`0x006F6F60` on the direct arm, `0x006F70E0`
/// on the arcing arm). `RulesClass::ReadElevationModel @ 0x0066D150`, called
/// by `RulesClass::Process` at `0x00668F46`, reads the three keys only when the
/// pass has the section (`0x00526810`), each defaulting to its current value.
#[derive(Debug, Clone, Copy)]
pub struct ElevationModel {
    /// `ElevationIncrement=` (`Rules+0x1838`, ReadInt `0x0066D183`): levels of
    /// height per bonus step. InRange divides by it unchecked (`IDIV` at
    /// `0x006F705A` and `0x006F71D6`).
    ///
    /// RESIDUAL: at 0, the constructor's value, that division faults natively
    /// on the first `SubjectToElevation=` shot that passes both vt+0x50 checks
    /// (a grounded firer at a grounded object or a dry cell), even with no
    /// drop; VERA adds no bonus. Dormant: retail `rulesmd.ini` sets 4, so only
    /// a map or mode that sets 0 reaches it.
    pub increment: i32,
    /// `ElevationIncrementBonus=` (`Rules+0x1840`, ReadDouble `0x0066D1AA`):
    /// cells of range per step.
    pub increment_bonus: NativeF64Bits,
    /// `ElevationBonusCap=` (`Rules+0x1848`, ReadDouble `0x0066D1D1`): the most
    /// cells the steps add.
    pub bonus_cap: NativeF64Bits,
}

impl Default for ElevationModel {
    /// The constructor's stores (`0x00667807..0x0066781F`): `EBX` is 0 and
    /// `EBP` holds `0x3FF00000`, the high dword of 1.0.
    fn default() -> Self {
        Self {
            increment: 0,
            increment_bonus: NativeF64Bits::ONE,
            bonus_cap: NativeF64Bits::POSITIVE_ZERO,
        }
    }
}

impl ElevationModel {
    fn from_ini(ini: &IniFile) -> Self {
        let section = ini.section_or_empty("ElevationModel");
        let d = Self::default();
        Self {
            increment: section.read_int("ElevationIncrement", d.increment),
            increment_bonus: section.read_double_bits("ElevationIncrementBonus", d.increment_bonus),
            bonus_cap: section.read_double_bits("ElevationBonusCap", d.bonus_cap),
        }
    }
}

impl GarrisonRules {
    fn from_ini(ini: &IniFile) -> Self {
        // The multipliers are float fields (`FSTP dword` after each ReadDouble,
        // `0x0066C68A..0x0066C75C`).
        let section = ini.section_or_empty("CombatDamage");
        let d = Self::default();
        Self {
            occupy_damage_multiplier: section
                .read_float("OccupyDamageMultiplier", d.occupy_damage_multiplier),
            occupy_rof_multiplier: section
                .read_float("OccupyROFMultiplier", d.occupy_rof_multiplier),
            occupy_weapon_range: section.read_int("OccupyWeaponRange", d.occupy_weapon_range),
            bunker_damage_multiplier: section
                .read_float("BunkerDamageMultiplier", d.bunker_damage_multiplier),
            bunker_rof_multiplier: section
                .read_float("BunkerROFMultiplier", d.bunker_rof_multiplier),
            bunker_weapon_range_bonus: section
                .read_int("BunkerWeaponRangeBonus", d.bunker_weapon_range_bonus),
            open_topped_damage_multiplier: section.read_float(
                "OpenToppedDamageMultiplier",
                d.open_topped_damage_multiplier,
            ),
            open_topped_range_bonus: section
                .read_int("OpenToppedRangeBonus", d.open_topped_range_bonus),
        }
    }
}

/// Bridge damage/destruction rules parsed from `rules(md).ini`.
#[derive(Debug, Clone)]
pub struct BridgeRules {
    /// Signed upper bound of the per-hit bridge damage admission draw.
    pub strength: i32,
    /// Reset/default value for `SpecialFlags::DestroyableBridges`.
    ///
    /// `[CombatDamage] DestroyableBridges=` exists in retail INI text but is
    /// not read by gamemd as the gameplay gate.
    #[cfg(test)]
    pub destroyable_by_default: bool,
    /// SHP animation names to spawn when a bridge group is destroyed
    /// (e.g., TWLT026, TWLT036, TWLT050, TWLT070). Picked randomly per cell.
    pub explosions: Vec<String>,
    /// Maximum metallic-debris voxels spawned per destroyed bridge cell.
    /// Parsed from `[General] BridgeVoxelMax=` in rules.ini (default 3).
    /// Consumed by the damage state machine in a later tier.
    #[cfg(test)]
    pub voxel_max: u8,
    /// Sound ID played when a bridge segment is repaired by an
    /// Engineer entering a `BridgeRepairHut=yes` building.
    /// Parsed from `[AudioVisual] RepairBridgeSound=` in rules.ini
    /// (stock default `BridgeRepaired`). Stored uppercased.
    /// `None` means the consumer applies its own default.
    pub repair_sound: Option<String>,
}

impl Default for BridgeRules {
    fn default() -> Self {
        Self {
            strength: 1000,
            #[cfg(test)]
            destroyable_by_default: true,
            explosions: Vec::new(),
            #[cfg(test)]
            voxel_max: 3,
            repair_sound: None,
        }
    }
}

impl BridgeRules {
    fn from_ini(ini: &IniFile) -> Self {
        // Rules ctor6675DA sets1000; ReadCombatDamage66CD66..66CD86 retains
        // the current signed dword as ReadInteger's default, with no clamp.
        let strength = ini
            .section("CombatDamage")
            .map_or(1000, |section| section.read_int("BridgeStrength", 1000));
        // Published from the ordered RulesClass reader by from_processed_rules.
        let explosions = Vec::new();
        #[cfg(test)]
        let voxel_max = ini
            .section_or_empty("General")
            .read_int("BridgeVoxelMax", 3)
            .clamp(0, 255) as u8;
        let repair_sound = ini
            .section_or_empty("AudioVisual")
            .read_name("RepairBridgeSound", 0x80)
            .map(str::to_uppercase);
        Self {
            strength,
            #[cfg(test)]
            destroyable_by_default: true,
            explosions,
            #[cfg(test)]
            voxel_max,
            repair_sound,
        }
    }
}

/// Global radiation-field constants parsed from the `[Radiation]` section.
/// Consumed by the per-cell radiation service (`sim::radiation`) and the
/// per-foot-unit damage step. Render-only keys (light/tint/color) are parsed
/// here so the render layer can pick them up later.
#[derive(Debug, Clone)]
pub struct RadiationRules {
    /// Frames a site lasts per point of radiation level (`RadDurationMultiple`).
    /// Site duration = level × this.
    pub duration_multiple: i32,
    /// Frames between radiation damage applications to units (`RadApplicationDelay`).
    pub application_delay: i32,
    /// Cap on the level a cell damages as, not on storage (`RadLevelMax`).
    pub level_max: i32,
    /// Frames between per-cell level decrements (`RadLevelDelay`).
    pub level_delay: i32,
    /// Frames between light intensity decrements (`RadLightDelay`). Render-only.
    pub light_delay: i32,
    /// Damage per point of (clamped) cell level (`RadLevelFactor`).
    /// Carried as f64 — the damage step truncates `level × factor` toward
    /// zero under the original PC53/chop arithmetic. The shared ReadDouble
    /// reader scans binary32 and widens it to this stored double.
    pub level_factor: f64,
    /// Light intensity per level point (`RadLightFactor`). Render-only.
    pub light_factor: SimFixed,
    /// Tint scale for the radiation glow (`RadTintFactor`). Render-only.
    pub tint_factor: SimFixed,
    /// Glow color (`RadColor=R,G,B`). Render-only.
    pub color: (u8, u8, u8),
    /// Warhead used for radiation damage (`RadSiteWarhead=`, Rules+1834), as
    /// the ordered Rules processor resolved it: empty for the constructor's
    /// null (`0x00667801`) or the factory's null for `none`. An empty read
    /// keeps it (`0x0066D0EA` reloads the field, `0x0066D127` stores it back).
    pub site_warhead: String,
    /// Phobos `[Radiation] UseGlobalRadApplicationDelay` (default true): when
    /// true every radiation type shares the global `RadApplicationDelay`
    /// gate, and the per-type delay is not consulted.
    #[cfg(feature = "phobos")]
    pub use_global_rad_application_delay: bool,
    /// Phobos `[Radiation] RadApplicationDelay.Building` (default 0). Above 0
    /// it enables radiation damage to buildings; the building damage chain is
    /// not implemented, so the value is retained only.
    #[cfg(feature = "phobos")]
    pub application_delay_building: i32,
    /// Phobos `[Radiation] RadBuildingDamageMaxCount` (default -1: no limit).
    /// Retained only, like the building delay above.
    #[cfg(feature = "phobos")]
    pub building_damage_max_count: i32,
    /// Phobos `[Radiation] RadSiteWarhead.Detonate` (default false): makes
    /// `RadSiteWarhead` detonate on affected objects. Retained only.
    #[cfg(feature = "phobos")]
    pub site_warhead_detonate: bool,
    /// Phobos `[Radiation] RadSiteWarhead.Detonate.Full` (default true): full
    /// warhead detonation instead of area damage when detonating. Retained
    /// only.
    #[cfg(feature = "phobos")]
    pub site_warhead_detonate_full: bool,
    /// Phobos `[Radiation] RadHasOwner` (default false): radiation damage
    /// counts as dealt by the house that fired the projectile. Retained only.
    #[cfg(feature = "phobos")]
    pub has_owner: bool,
    /// Phobos `[Radiation] RadHasInvoker` (default false): radiation damage
    /// counts as dealt by the firing TechnoType and grants it experience.
    /// Retained only.
    #[cfg(feature = "phobos")]
    pub has_invoker: bool,
    /// Phobos `[RadiationTypes]` catalog with the default `Radiation` entry,
    /// read per Rules pass by the ordered Process owner. Empty in builds
    /// without the capability. Reached through
    /// [`crate::rules::phobos_radiation::effective_radiation`], never directly.
    #[cfg(feature = "phobos")]
    pub(crate) radiation_types: crate::rules::phobos_radiation::RadiationTypeCatalog,
}

impl Default for RadiationRules {
    fn default() -> Self {
        Self {
            duration_multiple: 1,
            application_delay: 16,
            level_max: 500,
            level_delay: 90,
            light_delay: 90,
            level_factor: 0.2,
            light_factor: SimFixed::from_num(0.1_f32),
            tint_factor: SimFixed::from_num(1.0_f32),
            // Rules665663 zeroes EBX;6677EF/F5/FB seed RGB from BL.
            //66CF70 retains these bytes when RadColor is absent.
            color: (0, 0, 0),
            site_warhead: String::new(),
            // Phobos [Radiation] globals (RulesExt::Global() defaults).
            #[cfg(feature = "phobos")]
            use_global_rad_application_delay: true,
            #[cfg(feature = "phobos")]
            application_delay_building: 0,
            #[cfg(feature = "phobos")]
            building_damage_max_count: -1,
            #[cfg(feature = "phobos")]
            site_warhead_detonate: false,
            #[cfg(feature = "phobos")]
            site_warhead_detonate_full: true,
            #[cfg(feature = "phobos")]
            has_owner: false,
            #[cfg(feature = "phobos")]
            has_invoker: false,
            #[cfg(feature = "phobos")]
            radiation_types: crate::rules::phobos_radiation::RadiationTypeCatalog::default(),
        }
    }
}

impl RadiationRules {
    fn from_ini(ini: &IniFile) -> Self {
        let d = Self::default();
        let Some(section) = ini.section("Radiation") else {
            return d;
        };
        let [red, green, blue] =
            section.read_color_rgb("RadColor", [d.color.0, d.color.1, d.color.2], None);
        Self {
            duration_multiple: section.read_int("RadDurationMultiple", d.duration_multiple),
            // Delays are used as divisors/modulo periods — clamp to >= 1 so a
            // degenerate INI value cannot divide by zero.
            application_delay: section
                .read_int("RadApplicationDelay", d.application_delay)
                .max(1),
            level_max: section.read_int("RadLevelMax", d.level_max),
            level_delay: section.read_int("RadLevelDelay", d.level_delay).max(1),
            light_delay: section.read_int("RadLightDelay", d.light_delay).max(1),
            // Double fields (`0x0066D057..0x0066D0A5`).
            level_factor: section.read_double("RadLevelFactor", d.level_factor),
            light_factor: SimFixed::from_num(
                section.read_double("RadLightFactor", d.light_factor.to_num()) as f32,
            ),
            tint_factor: SimFixed::from_num(
                section.read_double("RadTintFactor", d.tint_factor.to_num()) as f32,
            ),
            color: (red, green, blue),
            // Only the ordered Process owner reads it (`RulesTypeReferences`).
            site_warhead: d.site_warhead,
            #[cfg(feature = "phobos")]
            use_global_rad_application_delay: section
                .read_bool("UseGlobalRadApplicationDelay", true),
            #[cfg(feature = "phobos")]
            application_delay_building: section.read_int("RadApplicationDelay.Building", 0),
            #[cfg(feature = "phobos")]
            building_damage_max_count: section.read_int("RadBuildingDamageMaxCount", -1),
            #[cfg(feature = "phobos")]
            site_warhead_detonate: section.read_bool("RadSiteWarhead.Detonate", false),
            #[cfg(feature = "phobos")]
            site_warhead_detonate_full: section.read_bool("RadSiteWarhead.Detonate.Full", true),
            #[cfg(feature = "phobos")]
            has_owner: section.read_bool("RadHasOwner", false),
            #[cfg(feature = "phobos")]
            has_invoker: section.read_bool("RadHasInvoker", false),
            #[cfg(feature = "phobos")]
            radiation_types: crate::rules::phobos_radiation::RadiationTypeCatalog::default(),
        }
    }
}

/// Fallback `VeteranRatio=` when `[General]` omits it.
///
/// UNCHECKED against `RulesClass`'s constructor initialiser for `+0x668`; stock
/// `rulesmd.ini` supplies `3`, so this only bites a mod that deletes the key.
const VETERAN_RATIO_DEFAULT: f64 = 3.0;
/// Fallback `VeteranCap=` when `[General]` omits it. Same UNCHECKED status as
/// [`VETERAN_RATIO_DEFAULT`]; stock supplies `2`.
const VETERAN_CAP_DEFAULT: f64 = 2.0;

/// `[AudioVisual] AmbientChangeRate=` and `AmbientChangeStep=` (Rules `+0x1668`,
/// `+0x1670`) as `LogicClass::PerTickUpdate`'s ambient fade reads them:
/// whether the rate is nonzero (`0x0055B351..0x0055B362`), the interval
/// `ftol(rate * 900)` (`0x0055B3D8..0x0055B3E4`) and the step
/// `ftol(step * 100)` (`0x0055B447..0x0055B453`).
pub(crate) fn ambient_change_terms(rate: f64, step: f64) -> (bool, i32, i32) {
    use crate::util::native_x87::{MaskedX87Chop53 as X, MaskedX87Ordering, NativeF64Bits};
    let rate = X::load_f64(NativeF64Bits::from_bits(rate.to_bits()));
    let step = X::load_f64(NativeF64Bits::from_bits(step.to_bits()));
    // FCOMP/test C3 at55B351..55B362 rejects zero and unordered. Math_ftol
    // returns the low dword of its signed64 result; a Rust saturating cast
    // differs for large/exceptional binary32 action inputs.
    let enabled = matches!(
        X::compare(rate, X::load_i32(0)),
        MaskedX87Ordering::Less | MaskedX87Ordering::Greater
    );
    (
        enabled,
        X::ftol_i32_low_masked(X::mul(rate, X::load_i32(900))),
        X::ftol_i32_low_masked(X::mul(step, X::load_i32(100))),
    )
}

impl GeneralRules {
    /// LogicClass::AI55B259..55B265 uses the shared ftol(minutes*900),
    /// without clamping negative, zero or exceptional conversion results.
    pub fn shroud_growth_interval_frames(&self) -> i32 {
        native_minutes_to_frames(f64::from_bits(self.shroud_rate.bits()))
    }

    /// LogicClass::AI55B304..55B310 converts the retained FogRate through
    /// the shared native minute-to-frame conversion before restarting its timer.
    pub fn fog_regrowth_interval_frames(&self) -> i32 {
        native_minutes_to_frames(f64::from_bits(self.fog_rate.bits()))
    }

    /// Original Rules+D00/D1C/D38 in Infantry, Unit, Building order.
    /// Retained canonical identities, including duplicate entries.
    pub fn secret_lab_choices(&self) -> &[Vec<String>; 3] {
        &self.secret_lab_choices
    }

    pub fn incoming_message_sound(&self) -> Option<&str> {
        self.incoming_message_sound.as_deref()
    }

    pub fn voice_ifv_repair(&self) -> Option<&str> {
        self.voice_ifv_repair.as_deref()
    }

    pub fn message_char_typed_sound(&self) -> Option<&str> {
        self.message_char_typed_sound.as_deref()
    }

    /// Centered UI rejection cue resolved by the fixed SOUNDMD owner.
    pub fn system_error_sound(&self) -> Option<&str> {
        self.system_error_sound.as_deref()
    }

    /// Centred main-menu typed-code cue resolved by the fixed SOUNDMD owner.
    pub fn options_changed_sound(&self) -> Option<&str> {
        self.options_changed_sound.as_deref()
    }

    pub fn player_left_sound(&self) -> Option<&str> {
        self.player_left_sound.as_deref()
    }

    pub fn player_joined_sound(&self) -> Option<&str> {
        self.player_joined_sound.as_deref()
    }

    pub fn game_forming_sound(&self) -> Option<&str> {
        self.game_forming_sound.as_deref()
    }

    pub fn game_closed_sound(&self) -> Option<&str> {
        self.game_closed_sound.as_deref()
    }

    pub fn wind_direction(&self) -> i32 {
        self.wind_direction
    }

    /// The Rules+714 reference resolved by the fixed SOUNDMD process binder.
    pub fn building_damage_sound(&self) -> Option<&str> {
        self.building_damage_sound.as_deref()
    }

    pub fn create_unit_sound(&self) -> Option<&str> {
        self.create_unit_sound.as_deref()
    }

    pub fn create_infantry_sound(&self) -> Option<&str> {
        self.create_infantry_sound.as_deref()
    }

    pub fn create_aircraft_sound(&self) -> Option<&str> {
        self.create_aircraft_sound.as_deref()
    }

    pub fn disk_laser_charge_up(&self) -> Option<&str> {
        self.disk_laser_charge_up.as_deref()
    }

    pub fn time_warp_out_sound(&self) -> Option<&str> {
        self.time_warp_out_sound.as_deref()
    }

    pub fn time_warp_in_sound(&self) -> Option<&str> {
        self.time_warp_in_sound.as_deref()
    }

    /// Signed retained input-rate value read by AllToCheer730F40.
    pub fn maximum_cheer_rate(&self) -> i32 {
        self.maximum_cheer_rate
    }

    pub(crate) fn cheer_sound(&self) -> Option<&str> {
        self.cheer_sound.as_deref()
    }

    /// Resolved command-bar opener cue. After SidebarInput6D08F5's defeat
    /// refusal, playback6D091A precedes the open-state test.
    pub fn gui_open_sound(&self) -> Option<&str> {
        self.gui_open_sound.as_deref()
    }

    /// Resolved command-bar close cue used by SidebarInput6D0860.
    pub fn gui_close_sound(&self) -> Option<&str> {
        self.gui_close_sound.as_deref()
    }

    /// Resolved campaign score emblem loop, `[AudioVisual] ScoreAnimSound=`.
    pub fn score_anim_sound(&self) -> Option<&str> {
        self.score_anim_sound.as_deref()
    }

    /// Drive4B3A65, Ship6A30B4 and Foot/Walk's timer producers retain the
    /// configured double until FLD/FMUL900/ftol7C5F00. In particular, authored
    /// decimal .01 is parsed through float by ReadDouble5283D0 and converts to
    /// 8, while 1% converts to9. No clamp, round-to-nearest or u16 narrowing.
    pub fn path_delay_ticks(&self) -> i32 {
        native_minutes_to_frames(self.path_delay)
    }

    /// Script16's FLD/FMUL900/ftol at6ECD46..6ECD57. Reuse the common,
    /// deterministic rules-minute converter rather than host float math.
    pub(crate) fn patrol_scan_frames(&self) -> i32 {
        native_minutes_to_frames(f64::from_bits(self.patrol_scan_minutes.bits()))
    }

    /// `ftol(PlacementDelay * 900.0)`, the computer Construction Yard's retry
    /// wait (`BuildingClass::Factory_AI @ 0x004501DB..0x004501F3`). Retail .05
    /// gives 45.
    pub fn placement_delay_frames(&self) -> i32 {
        native_minutes_to_frames(self.placement_delay)
    }

    pub fn infantry_death_anim(&self, inf_death: i32) -> Option<&str> {
        usize::try_from(inf_death)
            .ok()
            .and_then(|slot| self.infantry_death_anims.get(slot))
            .and_then(Option::as_deref)
    }

    fn from_ini(ini: &IniFile) -> Self {
        let defaults = Self::default();
        // Separate ReadJumpjetControls674467..67447E, no General gate.
        let display_cruise_height = ini
            .section("JumpjetControls")
            .map_or(defaults.display_cruise_height, |s| {
                s.read_int("CruiseHeight", defaults.display_cruise_height)
            });
        // AI IQ thresholds live in their own [IQ] read.
        let iq = ini.section_or_empty("IQ");
        // gamemd-derived: `RulesClass__ReadIQ @ 0x00674240` reads signed
        // `[IQ] Production` into `Rules+0x143C` at `0x006742C1`, independently
        // of the `[General]` pass and without clamping the parsed dword.
        let iq_production = iq.read_int("Production", defaults.iq_production);
        // `0x00674379..0x00674399`, the same section gate and reader.
        let iq_harvester = iq.read_int("Harvester", defaults.iq_harvester);
        // `0x00674282..0x006742A2`, the same section gate and reader.
        let iq_super_weapons = iq.read_int("SuperWeapons", defaults.iq_super_weapons);
        // RulesProcess668F56 reaches ReadAudioVisual6691E0 independently of
        // ReadGeneral.66B34B/66B372 pass AudioVisual to5283D0 and store raw
        // doubles in Rules+1708/+1700; a missing General section cannot skip them.
        let audio_visual = ini.section_or_empty("AudioVisual");
        let message_delay_minutes =
            audio_visual.read_double_bits("MessageDelay", defaults.message_delay_minutes);
        // Steam gamemd: AudioVisual66B478..66B498 reads SavourDelay
        // into Rules+14C8; Process668F56 reaches it independently of General.
        let savour_delay_minutes =
            audio_visual.read_double("SavourDelay", defaults.savour_delay_minutes);
        let pose_dir = audio_visual.read_int("PoseDir", defaults.pose_dir);
        let deploy_dir = audio_visual
            .read_int("DeployDir", defaults.deploy_dir >> 5)
            .wrapping_shl(5);
        let condition_yellow_native = audio_visual.read_double("ConditionYellow", 0.5);
        let condition_red_native = audio_visual.read_double("ConditionRed", defaults.condition_red);
        // The bomb sounds (Rules+0x20C/+0x210) come from the same pass.
        let audio_visual_sound = |key: &str| {
            audio_visual
                .read_name(key, 0x80)
                .filter(|name| !name.eq_ignore_ascii_case("none"))
                .map(str::to_owned)
        };
        let bomb_ticking_sound = audio_visual_sound("BombTickingSound");
        let bomb_attach_sound = audio_visual_sound("BombAttachSound");
        let ally_reveal = audio_visual.read_bool("AllyReveal", defaults.ally_reveal);
        let psychic_reveal_activate_sound = audio_visual
            .read_name("PsychicRevealActivateSound", 0x80)
            .map(str::to_owned);
        // RulesProcess668F36 reaches ReadCombatDamage66BBB0 independently of
        // ReadGeneral too; it gates on its own section only.
        let psychic_reveal_radius = ini
            .section_or_empty("CombatDamage")
            .read_int("PsychicRevealRadius", defaults.psychic_reveal_radius);
        // Rules ReadAI6739E5..673A31 is independent of ReadGeneral.
        // Constructor66760E..66761E supplies PathDelay0.016 and blockage60.
        let ai = ini.section_or_empty("AI");
        let paranoid = ai.read_bool("Paranoid", defaults.paranoid);
        // RulesProcess668EC0 reaches 671EA0 independently of ReadGeneral too.
        let allies_allowed = ini
            .section_or_empty("MultiplayerDialogSettings")
            .read_bool("AlliesAllowed", defaults.allies_allowed);
        let ally_change_allowed = ini
            .section_or_empty("MultiplayerDialogSettings")
            .read_bool("AllyChangeAllowed", defaults.ally_change_allowed);
        let attack_delay = ai.read_double("AttackDelay", defaults.attack_delay);
        let path_delay = ai.read_double("PathDelay", defaults.path_delay);
        let blockage_path_delay_ticks =
            ai.read_int("BlockagePathDelay", defaults.blockage_path_delay_ticks);
        // ReadAudioVisual6691E0 is independent of the ReadGeneral section
        // gate.66B673..66B754 reads all three class lights and stores signed
        // milliunits after the active x87 chop-toward-zero conversion.
        // Original no-General GI control: infantry_berserk_body.json.
        // Retained residual: a later AudioVisual pass with a missing key uses
        // native signed integer /1000 defaults; this projection retains fractions.
        let extra_unit_light = (audio_visual
            .read_double("ExtraUnitLight", defaults.extra_unit_light as f64 / 1000.0)
            * 1000.0) as i32;
        let extra_infantry_light = (audio_visual.read_double(
            "ExtraInfantryLight",
            defaults.extra_infantry_light as f64 / 1000.0,
        ) * 1000.0) as i32;
        let extra_aircraft_light = (audio_visual.read_double(
            "ExtraAircraftLight",
            defaults.extra_aircraft_light as f64 / 1000.0,
        ) * 1000.0) as i32;
        // Phobos `RulesExt` reads these `[AudioVisual]` keys independently of
        // RulesClass's `[General]` gate.
        #[cfg(feature = "phobos")]
        let airstrike_line_color =
            audio_visual.read_phobos_color("AirstrikeLineColor", defaults.airstrike_line_color);
        #[cfg(feature = "phobos")]
        let airstrike_line_z_adjust =
            audio_visual.read_ares_int("AirstrikeLineZAdjust", defaults.airstrike_line_z_adjust);
        let Some(general) = ini.section("General") else {
            return Self {
                message_delay_minutes,
                savour_delay_minutes,
                pose_dir,
                deploy_dir,
                iq_production,
                iq_harvester,
                iq_super_weapons,
                display_cruise_height,
                condition_yellow: condition_yellow_native,
                condition_red: condition_red_native,
                path_delay,
                attack_delay,
                blockage_path_delay_ticks,
                bomb_ticking_sound,
                bomb_attach_sound,
                ally_reveal,
                allies_allowed,
                ally_change_allowed,
                paranoid,
                psychic_reveal_activate_sound,
                psychic_reveal_radius,
                extra_unit_light,
                extra_infantry_light,
                extra_aircraft_light,
                #[cfg(feature = "phobos")]
                airstrike_line_color,
                #[cfg(feature = "phobos")]
                airstrike_line_z_adjust,
                ..defaults
            };
        };
        // Combat-only globals are read in the late [CombatDamage] pass.
        let combat_damage = ini.section_or_empty("CombatDamage");
        // Slots 3/4/6..10 are RulesClass pointers the ordered Rules processor
        // publishes (`rules_type_references`).
        let mut infantry_death_anims = defaults.infantry_death_anims.clone();
        infantry_death_anims[5] = Some(
            ini.section("Animations")
                .and_then(|section| section.registry_ids().get(1).copied())
                .filter(|name| !name.is_empty())
                .unwrap_or("ELECTRO")
                .to_string(),
        );
        // [General] damage-Spark spawn probabilities (verified ctor defaults
        // 0.02/0.01; stock INI omits them). Raw doubles, not percentages. Bound
        // before `Self` so each band feeds both its stored value and its derived
        // integer roll threshold (a struct literal can't reference sibling fields).
        let condition_red_spark_prob: f64 =
            general.read_double("ConditionRedSparkingProbability", 0.02);
        let condition_yellow_spark_prob: f64 =
            general.read_double("ConditionYellowSparkingProbability", 0.01);
        Self {
            pose_dir,
            deploy_dir,
            max_waypoint_path_length: general
                .read_int("MaxWaypointPathLength", defaults.max_waypoint_path_length),
            flash_frame_time: general.read_int("FlashFrameTime", defaults.flash_frame_time),
            radar_combat_flash_time: general
                .read_int("RadarCombatFlashTime", defaults.radar_combat_flash_time),
            scroll_multiplier: audio_visual
                .read_double("ScrollMultiplier", defaults.scroll_multiplier),
            savour_delay_minutes,
            condition_red_sparking_probability: condition_red_spark_prob,
            condition_yellow_sparking_probability: condition_yellow_spark_prob,
            condition_red_spark_threshold: unit_draw_threshold(condition_red_spark_prob),
            condition_yellow_spark_threshold: unit_draw_threshold(condition_yellow_spark_prob),
            dumb_my_effectiveness_coefficient: general.read_double(
                "DumbMyEffectivenessCoefficient",
                defaults.dumb_my_effectiveness_coefficient,
            ),
            dumb_target_effectiveness_coefficient: general.read_double(
                "DumbTargetEffectivenessCoefficient",
                defaults.dumb_target_effectiveness_coefficient,
            ),
            dumb_target_special_threat_coefficient: general.read_double(
                "DumbTargetSpecialThreatCoefficient",
                defaults.dumb_target_special_threat_coefficient,
            ),
            dumb_target_strength_coefficient: general.read_double(
                "DumbTargetStrengthCoefficient",
                defaults.dumb_target_strength_coefficient,
            ),
            dumb_target_distance_coefficient: general.read_double(
                "DumbTargetDistanceCoefficient",
                defaults.dumb_target_distance_coefficient,
            ),
            my_effectiveness_coefficient_default: general.read_double(
                "MyEffectivenessCoefficientDefault",
                defaults.my_effectiveness_coefficient_default,
            ),
            target_effectiveness_coefficient_default: general.read_double(
                "TargetEffectivenessCoefficientDefault",
                defaults.target_effectiveness_coefficient_default,
            ),
            target_special_threat_coefficient_default: general.read_double(
                "TargetSpecialThreatCoefficientDefault",
                defaults.target_special_threat_coefficient_default,
            ),
            target_strength_coefficient_default: general.read_double(
                "TargetStrengthCoefficientDefault",
                defaults.target_strength_coefficient_default,
            ),
            target_distance_coefficient_default: general.read_double(
                "TargetDistanceCoefficientDefault",
                defaults.target_distance_coefficient_default,
            ),
            enemy_house_threat_bonus: general
                .read_double("EnemyHouseThreatBonus", defaults.enemy_house_threat_bonus),
            threat_per_occupant: general
                .read_int("ThreatPerOccupant", defaults.threat_per_occupant),
            // The ordered Rules processor publishes this and the other
            // RulesClass Type pointers and lists (`rules_type_references`).
            dead_bodies: defaults.dead_bodies.clone(),
            // Passive-scan cadence, in frames. Both keys are present in stock
            // rulesmd.ini with exactly the constructor defaults (27 / 36); read
            // them rather than hardcoding so a mod's values take effect.
            normal_targeting_delay: general
                .read_int("NormalTargetingDelay", defaults.normal_targeting_delay),
            guard_area_targeting_delay: general.read_int(
                "GuardAreaTargetingDelay",
                defaults.guard_area_targeting_delay,
            ),
            veteran_sight: general.read_double("VeteranSight", defaults.veteran_sight),
            veteran_combat: general.read_double("VeteranCombat", defaults.veteran_combat),
            veteran_speed: general.read_double("VeteranSpeed", defaults.veteran_speed),
            veteran_rof: general.read_double("VeteranROF", defaults.veteran_rof),
            game_speed_bias: general.read_double("GameSpeedBias", defaults.game_speed_bias),
            veteran_armor: general.read_double("VeteranArmor", 1.0),
            curley_shuffle: general.read_bool("CurleyShuffle", defaults.curley_shuffle),
            // ext173 reads it with the current byte as default into
            // `Rules+0x641` (`0xC2D132..0xC2D151`, `ReadBooleanByte 0x5295F0`).
            enable_ai_build_limitation: crate::rules::NPATCH_ENABLED
                && general.read_bool(
                    "EnableAIBuildLimitation",
                    defaults.enable_ai_build_limitation,
                ),
            // NPSE reads it with the current field as default into Rules+0x2D
            // (C30303..C3031A); the constructor's 66569B store leaves 0.
            disable_parallel_ai_queues: crate::rules::NPATCH_ENABLED
                && general.read_bool(
                    "DisableParallelAIQueues",
                    defaults.disable_parallel_ai_queues,
                ),
            repair_rate_minutes: general.read_double("RepairRate", defaults.repair_rate_minutes),
            // `RulesClass+0x30/+0x34/+0x38/+0x3C`, read by the General reader at
            // `0x0066D530`. ReadInt66E6F3/70D/727/740 retains each current
            // field, initially constructor1000/1/1000/1, without a clamp.
            self_heal_infantry_frames: general
                .read_int("SelfHealInfantryFrames", defaults.self_heal_infantry_frames),
            self_heal_infantry_amount: general
                .read_int("SelfHealInfantryAmount", defaults.self_heal_infantry_amount),
            self_heal_unit_frames: general
                .read_int("SelfHealUnitFrames", defaults.self_heal_unit_frames),
            self_heal_unit_amount: general
                .read_int("SelfHealUnitAmount", defaults.self_heal_unit_amount),
            veteran_ratio: general.read_double("VeteranRatio", VETERAN_RATIO_DEFAULT),
            veteran_cap: general.read_double("VeteranCap", VETERAN_CAP_DEFAULT),
            computer_base_defense_response: ai.read_int(
                "ComputerBaseDefenseResponse",
                defaults.computer_base_defense_response,
            ),
            // Signed [General] binding for native Rules+0xE48. The verified
            // report establishes default 5 and active-retail override 3.
            maximum_building_placement_failures: general.read_int(
                "MaximumBuildingPlacementFailures",
                defaults.maximum_building_placement_failures,
            ),
            placement_delay: general.read_double("PlacementDelay", defaults.placement_delay),
            attack_delay,
            campaign_money_delta_easy: general
                .read_int("CampaignMoneyDeltaEasy", defaults.campaign_money_delta_easy),
            campaign_money_delta_hard: general
                .read_int("CampaignMoneyDeltaHard", defaults.campaign_money_delta_hard),
            ai_alternate_production_credit_cutoff: general.read_int(
                "AIAlternateProductionCreditCutoff",
                defaults.ai_alternate_production_credit_cutoff,
            ),
            ai_restrict_replace_time: general
                .read_int("AIRestrictReplaceTime", defaults.ai_restrict_replace_time),
            team_delays: general.read_int_list("TeamDelays").unwrap_or_default(),
            total_ai_team_cap: general.read_int_list("TotalAITeamCap").unwrap_or_default(),
            minimum_ai_defensive_teams: general
                .read_int_list("MinimumAIDefensiveTeams")
                .unwrap_or_default(),
            maximum_ai_defensive_teams: general
                .read_int_list("MaximumAIDefensiveTeams")
                .unwrap_or_default(),
            use_min_defense_rule: general
                .read_bool("UseMinDefenseRule", defaults.use_min_defense_rule),
            fill_earliest_team_probability: general
                .read_int_list("FillEarliestTeamProbability")
                .unwrap_or_default(),
            dissolve_unfilled_team_delay: general.read_int(
                "DissolveUnfilledTeamDelay",
                defaults.dissolve_unfilled_team_delay,
            ),
            ai_safe_distance: general.read_int("AISafeDistance", defaults.ai_safe_distance),
            ai_trigger_success_weight_delta: general.read_double_bits(
                "AITriggerSuccessWeightDelta",
                defaults.ai_trigger_success_weight_delta,
            ),
            ai_trigger_failure_weight_delta: general.read_double_bits(
                "AITriggerFailureWeightDelta",
                defaults.ai_trigger_failure_weight_delta,
            ),
            ai_trigger_track_record_coefficient: general.read_double_bits(
                "AITriggerTrackRecordCoefficient",
                defaults.ai_trigger_track_record_coefficient,
            ),
            harvesters_per_refinery: general
                .read_int_list("HarvestersPerRefinery")
                .unwrap_or_default(),
            ai_minor_super_ready_percent: general.read_double_to_float(
                "AIMinorSuperReadyPercent",
                defaults.ai_minor_super_ready_percent,
            ),
            base_defense_delay_minutes: general
                .read_double("BaseDefenseDelay", defaults.base_defense_delay_minutes),
            suspend_priority: general.read_int("SuspendPriority", defaults.suspend_priority),
            suspend_delay_minutes: general
                .read_double("SuspendDelay", defaults.suspend_delay_minutes),
            leptons_per_sight_increase: general.read_int("LeptonsPerSightIncrease", 0),
            gap_radius: general.read_int("GapRadius", 10),
            reveal_by_height: general.read_bool("RevealByHeight", true),
            ally_reveal,
            allies_allowed,
            ally_change_allowed,
            comp_easy_bonus: defaults.comp_easy_bonus,
            paranoid,
            tunnel_speed: SimFixed::from_num(general.read_double("TunnelSpeed", 6.0) as f32),
            flight_level: general.read_int("FlightLevel", 500),
            attacking_aircraft_sight_range: general.read_int(
                "AttackingAircraftSightRange",
                defaults.attacking_aircraft_sight_range,
            ),
            display_cruise_height,
            // ReadGeneral 0x0066EDC5..0x0066EE83 reads these into their
            // fields with the current value as default (%-aware ReadDouble).
            hover_height: general.read_int("HoverHeight", defaults.hover_height),
            hover_bob: general.read_double_bits("HoverBob", defaults.hover_bob),
            hover_boost: general.read_double_bits("HoverBoost", defaults.hover_boost),
            hover_acceleration: general
                .read_double_bits("HoverAcceleration", defaults.hover_acceleration),
            hover_brake: general.read_double_bits("HoverBrake", defaults.hover_brake),
            hover_dampen: general.read_double_bits("HoverDampen", defaults.hover_dampen),
            paradrop_radius: general.read_int("ParadropRadius", 1024),
            parachute_shp: defaults.parachute_shp.clone(),
            // The ordered RulesTypeReferences owner publishes the retained
            // NPSE ReadGeneral pointer (C31005..C31017), like Parachute.
            vehicle_parachute: defaults.vehicle_parachute.clone(),
            amer_paradrop: ParadropList::read_counts(general, "AmerParaDropNum"),
            ally_paradrop: ParadropList::read_counts(general, "AllyParaDropNum"),
            sov_paradrop: ParadropList::read_counts(general, "SovParaDropNum"),
            yuri_paradrop: ParadropList::read_counts(general, "YuriParaDropNum"),
            // The processed RulesClass vector is authoritative.
            anim_to_infantry: Vec::new(),
            base_unit_types: defaults.base_unit_types.clone(),
            multiplayer_ai_cm: general
                .read_int_list("MultiplayerAICM")
                .unwrap_or(defaults.multiplayer_ai_cm),
            pad_aircraft_types: defaults.pad_aircraft_types.clone(),
            separate_aircraft: general.read_bool("SeparateAircraft", defaults.separate_aircraft),
            // Only the ordered Process projection binds these references/reads.
            drop_zone_anim: defaults.drop_zone_anim,
            drop_zone_radius: defaults.drop_zone_radius,
            tiberium_grows: general.read_bool("TiberiumGrows", true),
            tiberium_spreads: general.read_bool("TiberiumSpreads", true),
            growth_rate_minutes: general.read_double("GrowthRate", 2.0) as f32,
            attack_cursor_on_disguise: general.read_bool("AttackCursorOnDisguise", false),
            // Filled from the native ordered TerrainType list owner after
            // processing, including allocation/none-token/retention semantics.
            default_mirage_disguises: Vec::new(),
            infantry_disguises: Default::default(),
            secret_lab_choices: Default::default(),
            // Rules671D80: signed ReadInt, constructor665650 defaults0.
            infantry_blink_disguise_time: general.read_int("InfantryBlinkDisguiseTime", 0),
            tree_targeting: combat_damage.read_bool("TreeTargeting", false),
            // The ordered General owner retains this across Type resets and
            // projects it before Terrain parsing; this is only the ctor value.
            tree_strength: defaults.tree_strength,
            condition_yellow: condition_yellow_native,
            condition_red: condition_red_native,
            cloaking_stages: general.read_int("CloakingStages", 9),
            cloak_delay_frames: (general.read_double("CloakDelay", 0.0) * 900.0)
                .trunc()
                .clamp(i32::MIN as f64, i32::MAX as f64) as i32,
            // RulesClass::ReadAudioVisual @ 0x006691E0 resolves CloakSound into
            // RulesClass+0x6A0. Retain the name at the data boundary; an absent
            // or empty key leaves the native invalid-index/no-play behavior.
            cloak_sound: audio_visual
                .read_name("CloakSound", 0x80)
                .map(str::to_owned),
            // The fixed SOUNDMD binder resolves Rules+0x274 as a Voc index.
            // Keep the constructor's invalid index until that owner binds it.
            leave_bio_reactor_sound: None,
            enter_bio_reactor_sound: None,
            enter_grinder_sound: None,
            // The fixed SOUNDMD binder resolves Rules+0x228/+0x22C as Voc
            // indexes; keep the constructor's invalid index until it binds them.
            upgrade_veteran_sound: None,
            upgrade_elite_sound: None,
            slaves_free_sound: audio_visual
                .read_name("SlavesFreeSound", 0x80)
                .map(str::to_owned),
            crate_money_sound: audio_visual
                .read_name("CrateMoneySound", 0x80)
                .map(str::to_owned),
            crate_reveal_sound: audio_visual
                .read_name("CrateRevealSound", 0x80)
                .map(str::to_owned),
            crate_fire_sound: audio_visual
                .read_name("CrateFireSound", 0x80)
                .map(str::to_owned),
            crate_armour_sound: audio_visual
                .read_name("CrateArmourSound", 0x80)
                .map(str::to_owned),
            crate_speed_sound: audio_visual
                .read_name("CrateSpeedSound", 0x80)
                .map(str::to_owned),
            crate_unit_sound: audio_visual
                .read_name("CrateUnitSound", 0x80)
                .map(str::to_owned),
            crate_promote_sound: audio_visual
                .read_name("CratePromoteSound", 0x80)
                .map(str::to_owned),
            talk_bubble_time: defaults.talk_bubble_time,
            elite_flash_timer: audio_visual.read_int("EliteFlashTimer", defaults.elite_flash_timer),
            idle_action_frequency: audio_visual
                .read_double("IdleActionFrequency", defaults.idle_action_frequency),
            airstrike_abort_sound: None,
            airstrike_attack_voice: None,
            building_garrisoned_sound: audio_visual
                .read_name("BuildingGarrisonedSound", 0x80)
                .map(str::to_owned),
            building_abandoned_sound: None,
            sell_sound: audio_visual.read_name("SellSound", 0x80).map(str::to_owned),
            base_under_attack_sound: audio_visual
                .read_name("BaseUnderAttackSound", 0x80)
                .map(str::to_owned),
            building_die_sound: audio_visual
                .read_name("BuildingDieSound", 0x80)
                .map(str::to_owned),
            building_damage_sound: None,
            create_unit_sound: None,
            create_infantry_sound: None,
            create_aircraft_sound: None,
            disk_laser_charge_up: None,
            // `CCINIClass::ReadSoundList @ 0x00525430`: `ReadString(..., "",
            // buf, 0x80)` first (so the whole value is byte-cut and trimmed
            // once), then `strtok` on `","` (`0x00817F70`) with the tokens
            // left untrimmed. Resolution against the Voc registry
            // (`VocClass::FindPtrByName`, which drops unresolvable names) is
            // deferred to the app layer, where the registry lives.
            lightning_sounds: audio_visual
                .read_list("LightningSounds", 0x80)
                .map(|tokens| tokens.into_iter().map(str::to_owned).collect())
                .unwrap_or_default(),
            credit_ticks: audio_visual
                .read_list("CreditTicks", 0x80)
                .map(|tokens| tokens.into_iter().map(str::to_owned).collect())
                .unwrap_or_default(),
            storm_sound: audio_visual
                .read_name("StormSound", 0x80)
                .map(str::to_owned),
            // `RulesClass::ReadGeneral @ 0x0067107F` passes the field's own
            // current value as the default, so an absent key keeps the
            // constructor's `true` (see the field doc).
            lightning_print_text: general.read_bool("LightningPrintText", true),
            dig_sound: audio_visual.read_name("DigSound", 0x80).map(str::to_owned),
            nuke_take_off: defaults.nuke_take_off.clone(),
            ai_super_defense_probability: general
                .read_int_list("AISuperDefenseProbability")
                .unwrap_or_default(),
            ai_super_defense_distance: general.read_range("AISuperDefenseDistance", 10),
            ai_super_defense_frames: general
                .read_int("AISuperDefenseFrames", defaults.ai_super_defense_frames),
            ai_ion_cannon_values: AiIonCannonValues::read(general),
            psychic_dominator_activate_sound: audio_visual
                .read_name("PsychicDominatorActivateSound", 0x80)
                .map(str::to_owned),
            genetic_mutator_activate_sound: audio_visual
                .read_name("GeneticMutatorActivateSound", 0x80)
                .map(str::to_owned),
            psychic_reveal_activate_sound,
            // Constructor -1 until the fixed SOUNDMD catalog resolves it.
            spy_plane_camera: None,
            spy_plane_camera_frames: audio_visual
                .read_int("SpyPlaneCameraFrames", defaults.spy_plane_camera_frames),
            chute_sound: audio_visual
                .read_name("ChuteSound", 0x80)
                .map(str::to_owned),
            bunker_walls_down_sound: audio_visual
                .read_name("BunkerWallsDownSound", 0x80)
                .map(str::to_owned),
            bunker_walls_up_sound: audio_visual
                .read_name("BunkerWallsUpSound", 0x80)
                .map(str::to_owned),
            gui_main_button_sound: audio_visual
                .read_name("GUIMainButtonSound", 0x80)
                .map(str::to_owned),
            gui_move_in_sound: audio_visual
                .read_name("GUIMoveInSound", 0x80)
                .map(str::to_owned),
            gui_move_out_sound: audio_visual
                .read_name("GUIMoveOutSound", 0x80)
                .map(str::to_owned),
            shell_button_slide_sound: audio_visual
                .read_name("ShellButtonSlideSound", 0x80)
                .map(str::to_owned),
            generic_click_sound: audio_visual
                .read_name("GenericClick", 0x80)
                .map(str::to_owned),
            // Native constructor -1; resolved by the fixed SOUNDMD binder.
            guard_sound: None,
            stop_sound: None,
            radar_on: None,
            radar_off: None,
            movie_on: None,
            movie_off: None,
            spy_sat_activation_sound: None,
            spy_sat_deactivation_sound: None,
            start_planning_mode_sound: None,
            end_planning_mode_sound: None,
            add_planning_mode_command_sound: None,
            place_beacon_sound: None,
            scatter_sound: None,
            scold_sound: None,
            generic_beep_sound: audio_visual
                .read_name("GenericBeep", 0x80)
                .map(str::to_owned),
            gui_checkbox_sound: audio_visual
                .read_name("GUICheckboxSound", 0x80)
                .map(str::to_owned),
            ore_twinkle: defaults.ore_twinkle.clone(),
            ore_twinkle_chance: audio_visual
                .read_int("OreTwinkleChance", defaults.ore_twinkle_chance),
            gui_build_sound: audio_visual
                .read_name("GUIBuildSound", 0x80)
                .map(str::to_owned),
            building_slam: audio_visual
                .read_name("BuildingSlam", 0x80)
                .map(str::to_owned),
            gui_tab_sound: audio_visual
                .read_name("GUITabSound", 0x80)
                .map(str::to_owned),
            // The fixed sound registry binds these readers below, including
            // current-ID retention through the actual Rules pass history.
            voice_ifv_repair: None,
            incoming_message_sound: None,
            message_char_typed_sound: None,
            system_error_sound: None,
            options_changed_sound: None,
            player_left_sound: None,
            player_joined_sound: None,
            game_forming_sound: None,
            game_closed_sound: None,
            message_delay_minutes,
            shroud_grow: audio_visual.read_bool("ShroudGrow", defaults.shroud_grow),
            shroud_rate: audio_visual.read_double_bits("ShroudRate", defaults.shroud_rate),
            fog_rate: audio_visual.read_double_bits("FogRate", defaults.fog_rate),
            speak_delay_minutes: audio_visual.read_double("SpeakDelay", 0.0),
            gui_combo_open_sound: audio_visual
                .read_name("GUIComboOpenSound", 0x80)
                .map(str::to_owned),
            gui_combo_close_sound: audio_visual
                .read_name("GUIComboCloseSound", 0x80)
                .map(str::to_owned),
            // Constructor -1 until the fixed SOUNDMD binder resolves each
            // reached ReadString128 value, including failed-read retention.
            gui_open_sound: None,
            gui_close_sound: None,
            // Float fields (`FSTP dword` at `0x0066B8B1`/`0x0066B8DB`).
            direct_rocking_coefficient: SimFixed::from_num(
                audio_visual.read_float("DirectRockingCoefficient", 1.5),
            ),
            fallback_coefficient: SimFixed::from_num(
                audio_visual.read_float("FallBackCoefficient", 0.1),
            ),
            // ChronoInSound/ChronoOutSound live in [AudioVisual], not [General].
            // No hardcoded fallback: a genuinely-absent key means no fallback
            // sound (silence), matching gamemd. Stock ships these keys present
            // (= ChronoMinerTeleport), so stock audio is unchanged.
            chrono_in_sound: audio_visual
                .read_name("ChronoInSound", 0x80)
                .map(str::to_owned),
            chrono_out_sound: audio_visual
                .read_name("ChronoOutSound", 0x80)
                .map(str::to_owned),
            time_warp_out_sound: None,
            time_warp_in_sound: None,
            cheer_sound: None,
            impact_water_sound: audio_visual
                .read_name("ImpactWaterSound", 0x80)
                .map(str::to_owned),
            impact_land_sound: audio_visual
                .read_name("ImpactLandSound", 0x80)
                .map(str::to_owned),
            // Constructor -1 until the fixed SOUNDMD catalog resolves it.
            sinking_sound: None,
            score_anim_sound: None,
            construction_sound: None,
            building_repaired_sound: None,
            bomb_ticking_sound,
            bomb_attach_sound,
            warp_in: defaults.warp_in.clone(),
            warp_out: defaults.warp_out.clone(),
            warp_away: defaults.warp_away.clone(),
            chrono_sparkle1: defaults.chrono_sparkle1.clone(),
            wake: defaults.wake.clone(),
            move_flash: defaults.move_flash.clone(),
            behind: defaults.behind,
            infantry_death_anims,
            damage_delay_minutes: general.read_double("DamageDelay", 1.0) as f32,
            spy_power_blackout: general.read_int("SpyPowerBlackout", defaults.spy_power_blackout),
            spy_money_steal_percent: general
                .read_double_to_float("SpyMoneyStealPercent", defaults.spy_money_steal_percent),
            damage_fire_types: defaults.damage_fire_types.clone(),
            barrel_particle: defaults.barrel_particle.clone(),
            barrel_explode: defaults.barrel_explode.clone(),
            barrel_debris: defaults.barrel_debris.clone(),
            tiberium_short_scan: general.read_range("TiberiumShortScan", 0x600),
            tiberium_long_scan: general.read_range("TiberiumLongScan", 0x2000),
            buildup_time: general.read_double("BuildupTime", f64::from_bits(0x3FA9_9999_9999_999A)),
            slave_miner_short_scan: general.read_range("SlaveMinerShortScan", 0x500),
            slave_miner_slave_scan: general.read_range("SlaveMinerSlaveScan", 0x1000),
            drain_money_frame_delay: general.read_int("DrainMoneyFrameDelay", 30),
            drain_money_amount: general.read_int("DrainMoneyAmount", 30),
            slave_miner_long_scan: general.read_range("SlaveMinerLongScan", 0x5000),
            slave_miner_scan_correction: general.read_range("SlaveMinerScanCorrection", 0x300),
            slave_miner_kick_frame_delay: general.read_int("SlaveMinerKickFrameDelay", 0x7FFF_FFFF),
            maximum_queued_objects: general.read_int("MaximumQueuedObjects", 5),
            #[cfg(feature = "phobos")]
            building_production_queue: general.read_bool("BuildingProductionQueue", false),
            #[cfg(all(feature = "phobos", feature = "ares"))]
            building_type_selectable: general.read_bool("BuildingTypeSelectable", false),
            #[cfg(feature = "phobos")]
            ai_fire_sale: general.read_bool("AIFireSale", true),
            #[cfg(feature = "phobos")]
            ai_fire_sale_delay: general.read_int("AIFireSaleDelay", 0),
            #[cfg(feature = "phobos")]
            ai_all_to_hunt: general.read_bool("AIAllToHunt", true),
            #[cfg(feature = "phobos")]
            gather_when_mcv_deploy: general.read_bool("GatherWhenMCVDeploy", true),
            harvester_too_far_distance: general.read_int("HarvesterTooFarDistance", 5),
            approach_target_reset_multiplier: general.read_int("ApproachTargetResetMultiplier", 1),
            chrono_harv_too_far_distance: general.read_int("ChronoHarvTooFarDistance", 50),
            harvester_load_rate: general.read_int("HarvesterLoadRate", 2),
            harvester_dump_frames: {
                // gamemd reads HarvesterDumpRate with ReadDouble and gates on
                // `rate × 900 <= accumulator`; the accumulator is integer-stepped,
                // so the first crossing is ceil(rate × 900). Take the ceiling at
                // full double precision to match that crossing exactly (no tenths
                // rounding). Clamp to u16::MAX to keep the frame threshold in range.
                let rate = general.read_double("HarvesterDumpRate", 0.016);
                (rate * 900.0).clamp(0.0, u16::MAX as f64).ceil() as u16
            },
            chrono_delay: general.read_int("ChronoDelay", 60),
            chrono_reinf_delay: general.read_int("ChronoReinfDelay", 180),
            chrono_distance_factor: general.read_int("ChronoDistanceFactor", 48),
            chrono_trigger: general.read_bool("ChronoTrigger", true),
            chrono_minimum_delay: general.read_int("ChronoMinimumDelay", 16),
            chrono_range_minimum: general.read_int("ChronoRangeMinimum", 0),
            purifier_bonus: general.read_double_to_float("PurifierBonus", defaults.purifier_bonus),
            // An IntVector (`0x00475D70`) indexed by difficulty.
            ai_virtual_purifiers: general
                .read_int_list("AIVirtualPurifiers")
                .and_then(|values| values.get(..3)?.try_into().ok())
                .unwrap_or([4, 2, 0]),
            disabled_disguise_detection_percent: general
                .read_int_list("DisabledDisguiseDetectionPercent")
                .unwrap_or_else(|| defaults.disabled_disguise_detection_percent.clone()),
            allied_crew: defaults.allied_crew.clone(),
            soviet_crew: defaults.soviet_crew.clone(),
            third_crew: defaults.third_crew.clone(),
            technician: defaults.technician.clone(),
            engineer_infantry: defaults.engineer_infantry.clone(),
            engineer_capture_level: general
                .read_double_to_float("EngineerCaptureLevel", defaults.engineer_capture_level),
            crew_escape: crate::util::native_x87::NativeF64Bits::from_bits(
                general
                    .read_double("CrewEscape", f64::from_bits(defaults.crew_escape.bits()))
                    .to_bits(),
            ),
            refund_percent: crate::util::native_x87::NativeF64Bits::from_bits(
                general
                    .read_double(
                        "RefundPercent",
                        f64::from_bits(defaults.refund_percent.bits()),
                    )
                    .to_bits(),
            ),
            ship_sinking_weight: SimFixed::from_num(
                general.read_double("ShipSinkingWeight", defaults.ship_sinking_weight.to_num())
                    as f32,
            ),
            tracked_uphill: general.read_double_bits("TrackedUphill", defaults.tracked_uphill),
            tracked_downhill: general
                .read_double_bits("TrackedDownhill", defaults.tracked_downhill),
            wheeled_uphill: general.read_double_bits("WheeledUphill", defaults.wheeled_uphill),
            wheeled_downhill: general
                .read_double_bits("WheeledDownhill", defaults.wheeled_downhill),
            extra_unit_light,
            extra_infantry_light,
            extra_aircraft_light,
            close_enough: general.read_range("CloseEnough", defaults.close_enough),
            stray: general.read_range("Stray", defaults.stray),
            relaxed_stray: general.read_range("RelaxedStray", defaults.relaxed_stray),
            guard_mode_stray: general.read_range("GuardModeStray", defaults.guard_mode_stray),
            // Repair service scalars below are projected from the ordered
            // native reader; do not reread them from this flattened INI.
            repair_percent: general.read_double("RepairPercent", defaults.repair_percent),
            #[cfg(feature = "phobos")]
            prone_speed_crawls: general
                .read_phobos_double("ProneSpeed.Crawls", defaults.prone_speed_crawls),
            #[cfg(feature = "phobos")]
            prone_speed_no_crawls: general
                .read_phobos_double("ProneSpeed.NoCrawls", defaults.prone_speed_no_crawls),
            repair_step: general.read_int("RepairStep", defaults.repair_step),
            unit_repair_rate: defaults.unit_repair_rate,
            charge_to_drain_ratio: defaults.charge_to_drain_ratio,
            infantry_repair_step: defaults.infantry_repair_step,
            infantry_repair_rate: defaults.infantry_repair_rate,
            reload_rate: general.read_double("ReloadRate", defaults.reload_rate),
            path_delay,
            blockage_path_delay_ticks,
            // ReadGeneral670235..670267 uses the same475D70 signed vector
            // reader as the existing Recalc difficulty tables below.
            ai_auto_deploy_frame_delay: general
                .read_int_list("AIAutoDeployFrameDelay")
                .unwrap_or_default(),
            #[cfg(feature = "phobos")]
            infantry_auto_deploy: general.read_bool("InfantryAutoDeploy", false),
            #[cfg(feature = "phobos")]
            allow_damage_on_self: general.read_bool("AllowDamageOnSelf", false),
            #[cfg(feature = "phobos")]
            decloak_damaged_targets: general.read_bool("DecloakDamagedTargets", true),
            #[cfg(feature = "phobos")]
            rof_random_delay: combat_damage.read_ares_partial_int_pair(
                "ROF.RandomDelay",
                crate::rules::ini_value::PartialIntPair {
                    low: 0,
                    high: 2,
                    values: 2,
                },
            ),
            ai_force_prediction_fudge: ai
                .read_int_list("AIForcePredictionFudge")
                .unwrap_or_default(),
            ai_pick_wall_defense_percent: general
                .read_int_list("AIPickWallDefensePercent")
                .unwrap_or_default(),
            // PlayerScatter belongs to the [CombatDamage] read, IQ Scatter to
            // the [IQ] read; neither is a [General] key.
            player_scatter: combat_damage.read_bool("PlayerScatter", defaults.player_scatter),
            player_return_fire: combat_damage
                .read_bool("PlayerReturnFire", defaults.player_return_fire),
            iq_scatter: iq.read_int("Scatter", defaults.iq_scatter),
            max_iq_levels: iq.read_int("MaxIQLevels", defaults.max_iq_levels),
            iq_production,
            iq_harvester,
            iq_super_weapons,
            iq_guard_area: iq.read_int("GuardArea", defaults.iq_guard_area),
            iq_repair_sell: iq.read_int("RepairSell", defaults.iq_repair_sell),
            iq_auto_crush: iq.read_int("AutoCrush", defaults.iq_auto_crush),
            iq_sell_back: iq.read_int("SellBack", defaults.iq_sell_back),
            credit_reserve: ai.read_int("CreditReserve", defaults.credit_reserve),
            cliff_back_impassability: general.read_int("CliffBackImpassability", 2) as u8,
            lightning_storm_duration: general
                .read_int("LightningStormDuration", defaults.lightning_storm_duration),
            lightning_damage: general.read_int("LightningDamage", defaults.lightning_damage),
            lightning_deferment: general
                .read_int("LightningDeferment", defaults.lightning_deferment),
            lightning_hit_delay: general
                .read_int("LightningHitDelay", defaults.lightning_hit_delay),
            lightning_scatter_delay: general
                .read_int("LightningScatterDelay", defaults.lightning_scatter_delay),
            lightning_cell_spread: general
                .read_int("LightningCellSpread", defaults.lightning_cell_spread),
            lightning_separation: general
                .read_int("LightningSeparation", defaults.lightning_separation),
            // The sole ordered AV reader supplies these after projection.
            #[cfg(feature = "phobos")]
            timer_blink_color_scheme: defaults.timer_blink_color_scheme,
            ambient_change_rate_nonzero: defaults.ambient_change_rate_nonzero,
            ambient_change_interval_frames: defaults.ambient_change_interval_frames,
            ambient_change_step: defaults.ambient_change_step,
            iron_curtain_duration: combat_damage.read_int("IronCurtainDuration", 0),
            iron_curtain_invoke_anim: defaults.iron_curtain_invoke_anim.clone(),
            chrono_placement_anim: defaults.chrono_placement_anim.clone(),
            chrono_blast_anim: defaults.chrono_blast_anim.clone(),
            chrono_blast_dest_anim: defaults.chrono_blast_dest_anim.clone(),
            dominator_warhead: defaults.dominator_warhead.clone(),
            dominator_first_anim: defaults.dominator_first_anim.clone(),
            dominator_second_anim: defaults.dominator_second_anim.clone(),
            dominator_fire_at_percentage: general.read_int("DominatorFireAtPercentage", 50),
            dominator_capture_range: general.read_int("DominatorCaptureRange", 2),
            dominator_damage: general.read_int("DominatorDamage", 50),
            // The native Process state supplies all four IonBlast inputs.
            ion_blast_anim: defaults.ion_blast_anim.clone(),
            ion_beam_anim: defaults.ion_beam_anim.clone(),
            ion_cannon_damage: defaults.ion_cannon_damage,
            spotlight: defaults.spotlight,
            force_shield_radius: general.read_int("ForceShieldRadius", 10),
            force_shield_duration: general.read_int("ForceShieldDuration", 400),
            force_shield_blackout_duration: general.read_int("ForceShieldBlackoutDuration", 800),
            force_shield_fade_sound_time: general.read_int("ForceShieldPlayFadeSoundTime", 50),
            force_shield_invoke_anim: defaults.force_shield_invoke_anim.clone(),
            psychic_reveal_radius,
            // The constructor's nulls; only the ordered Process owner reads
            // them (`RulesTypeReferences`).
            mutate_warhead: String::new(),
            mutate_explosion_warhead: String::new(),
            empulse_warhead: None,
            mutate_explosion: general.read_bool("MutateExplosion", false),
            // The processed RulesClass vector is authoritative; a merged INI
            // cannot reproduce successful-read replacement or factory identity.
            metallic_debris: Vec::new(),
            weather_con_clouds: Vec::new(),
            weather_con_bolts: Vec::new(),
            // Read only by the ordered Process owner (`native_processing`),
            // which retains them across passes; the RuleSet projection
            // publishes its values over these constructor placeholders.
            detail: defaults.detail,
            difficulty_rows: defaults.difficulty_rows,
            patrol_scan_minutes: defaults.patrol_scan_minutes,
            gravity: defaults.gravity,
            wind_direction: defaults.wind_direction,
            missile_rot_var: defaults.missile_rot_var,
            safety_altitude: defaults.safety_altitude,
            line_trail_color_override: defaults.line_trail_color_override,
            chrono_beam_color: defaults.chrono_beam_color,
            force_shield_color: defaults.force_shield_color,
            laser_target_color: defaults.laser_target_color,
            #[cfg(feature = "phobos")]
            airstrike_line_color,
            #[cfg(feature = "phobos")]
            airstrike_line_z_adjust,
            iron_curtain_color: defaults.iron_curtain_color,
            berserk_color: defaults.berserk_color,
            parachute_max_fall_rate: defaults.parachute_max_fall_rate,
            no_parachute_max_fall_rate: defaults.no_parachute_max_fall_rate,
            maximum_cheer_rate: defaults.maximum_cheer_rate,
            allied_survivor_divisor: defaults.allied_survivor_divisor,
            soviet_survivor_divisor: defaults.soviet_survivor_divisor,
            third_survivor_divisor: defaults.third_survivor_divisor,
            prism_type: defaults.prism_type,
            prerequisite_proc_alternate: defaults.prerequisite_proc_alternate,
            prism_support: defaults.prism_support,
            building_types: defaults.building_types,
            lightning_warhead: defaults.lightning_warhead,
            weather_con_bolt_explosion: defaults.weather_con_bolt_explosion,
            weapon_nullify_anim: defaults.weapon_nullify_anim,
            // The compatibility projection drops keys no native reader takes,
            // so the Phobos read reaches `GeneralRules` through the retained
            // Rules scalar instead (`from_processed_rules`).
            #[cfg(feature = "phobos")]
            missing_cameo: defaults.missing_cameo.clone(),
        }
    }
}

impl ProductionRules {
    /// The RulesClass `[General]` reads, in native order: the four floats
    /// (`0x0066EB5B..0x0066EBC9`), then `BuildSpeed=` (`0x00670D23`) and
    /// `WallBuildSpeedCoefficient=` (`0x0067187F`). An absent section skips
    /// them all (the section test `0x00526810` jumps to `0x00671E8E`).
    fn from_ini(ini: &IniFile) -> Self {
        let mut rules = Self::default();
        let Some(general) = ini.section("General") else {
            return rules;
        };
        rules.min_low_power_production_speed = general.read_double_to_float(
            "MinLowPowerProductionSpeed",
            rules.min_low_power_production_speed,
        );
        rules.max_low_power_production_speed = general.read_double_to_float(
            "MaxLowPowerProductionSpeed",
            rules.max_low_power_production_speed,
        );
        rules.low_power_penalty_modifier = general
            .read_double_to_float("LowPowerPenaltyModifier", rules.low_power_penalty_modifier);
        rules.multiple_factory =
            general.read_double_to_float("MultipleFactory", rules.multiple_factory);
        rules.build_speed = general.read_double_bits("BuildSpeed", rules.build_speed);
        rules.wall_build_speed_coefficient = general.read_double_bits(
            "WallBuildSpeedCoefficient",
            rules.wall_build_speed_coefficient,
        );
        rules
    }
}

fn next_rule_set_serial() -> u64 {
    static SERIALS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    SERIALS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1
}

/// O(1) index into `RuleSet::object_list`. Resolved once from a name (or from an
/// interned id via the sim `TypeHandleTable`), then dereferenced directly —
/// avoiding the per-call string round-trip + hash lookup of name resolution.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, serde::Serialize, serde::Deserialize)]
pub struct TypeHandle(pub u32);

/// Master container for all game data parsed from rules.ini.
///
/// Name lookups are case-insensitive (matching the engine's find-or-allocate),
/// resolved via `type_handle`/`object_by_handle`. The sim/ module uses RuleSet
/// to look up costs, speeds, weapons, and prerequisites for every game action.
///
/// Tests may clone a set to reuse one retail load; a clone keeps the serial
/// until [`Self::with_fresh_serial`] gives it its own.
#[derive(Debug)]
#[cfg_attr(any(test, feature = "test-support"), derive(Clone))]
pub struct RuleSet {
    /// All game objects in registry insertion order, indexed by `TypeHandle`.
    object_list: Vec<ObjectType>,
    /// Uppercase type ID → handle. Uppercase keys give O(1) case-insensitive
    /// resolution, matching the engine's case-insensitive find-or-allocate.
    object_index: LookupMap<String, TypeHandle>,
    /// Distinct for every `RuleSet` the process builds, so a cache of this
    /// set's handles can tell it from another set ([`Self::serial`]).
    serial: u64,
    /// Per-native-registry identity authority. Unlike the broad lookup above,
    /// this retains distinct types when malformed/custom rules list the same
    /// ID in more than one category.
    object_category_index: HashMap<(ObjectCategory, String), TypeHandle>,
    /// Read-only projection of the process owner's global TechnoType array.
    /// Constructor order can interleave categories across lazy allocations
    /// and later Rules passes; it is not TYPE_REGISTRIES order. An absent
    /// handle records a native slot the older name-based object projection
    /// cannot represent uniquely, so exact lookup must not silently alias it.
    native_techno_type_order: Vec<(ObjectCategory, String, Option<TypeHandle>)>,
    /// Case-insensitive identity of each type in the four native TechnoType
    /// registries to its ordered array index (`+0xDF8`).
    type_array_indices: HashMap<(ObjectCategory, String), i32>,
    /// All weapons indexed by ID (e.g., "105mm" → WeaponType).
    weapons: LookupMap<String, WeaponType>,
    /// Derived from the one processed native Weapon family, in allocation
    /// order. The bool admits only slots whose Weapon/Warhead stored IDs map
    /// unambiguously into the existing name projection. Rebuilt with Rules;
    /// it never allocates or mutates Type state (see issue1784).
    weapon_array: Vec<(String, bool)>,
    /// All warheads indexed by ID (e.g., "AP" → WarheadType).
    warheads: LookupMap<String, WarheadType>,
    /// All projectiles indexed by ID (e.g., "InvisibleLow" → ProjectileType).
    projectiles: LookupMap<String, ProjectileType>,
    /// Country-level rules indexed by country/house type ID.
    countries: HashMap<String, CountryRules>,
    /// Country identities in native `[Countries]` registration order.
    country_ids: Vec<String>,
    /// Uppercase country identity -> source-order index.
    country_indices: HashMap<String, CountryIdx>,
    /// Side identities in native `[Sides]` registration order, including
    /// sides allocated later by a per-country `Side=` override.
    side_ids: Vec<String>,
    /// Read-only projection of the live SideExt owner, in the same native
    /// array order as side_ids. Rebuilt after every Process/reset/save restore.
    #[cfg(feature = "ares")]
    side_extensions: Vec<crate::rules::ares_side_extension::AresSideExtension>,
    /// Uppercase side identity -> source-order index.
    side_indices: HashMap<String, SideIdx>,
    /// Phobos `[SOMESIDE] Sidebar.GDIPositions=`, in `[Sides]` order.
    /// The first side defaults to GDI positions; all others default to the
    /// NOD table. Unlike SideExt+88 this is read without the Ares feature.
    #[cfg(feature = "phobos")]
    side_gdi_positions: Vec<bool>,
    /// Phobos `[GlobalControls]`/`[AudioVisual]` switches for the left
    /// superweapon sidebar.
    #[cfg(feature = "phobos")]
    pub sw_sidebar: crate::rules::phobos_sw_sidebar::SuperWeaponSidebarGlobals,
    /// Phobos `[AudioVisual] SelectionFlashDuration=`: frames a newly selected
    /// Techno flashes for; 0 disables it.
    #[cfg(feature = "phobos")]
    pub selection_flash: crate::rules::phobos_selection_flash::SelectionFlashGlobals,
    /// Phobos `[AudioVisual] FlyNoWobbles=`: the default a TechnoType's own
    /// `FlyNoWobbles` overrides. `None` keeps the retail rule, where
    /// `IsDropship` alone disables the Fly wobble.
    #[cfg(feature = "phobos")]
    fly_no_wobbles: Option<bool>,
    /// Phobos `[AudioVisual] ShowPowerPlantEnhancerRange=`: the rules switch
    /// for the placement-time enhancer rings.
    #[cfg(feature = "phobos")]
    pub power_plant_enhancer: crate::rules::phobos_power_plant_enhancer::PowerPlantEnhancerGlobals,
    /// Phobos `[CombatDamage]` airburst/splits defaults, consumed by the
    /// projectile emitter's nullable type keys.
    #[cfg(feature = "phobos")]
    pub phobos_airburst: crate::rules::phobos_airburst::PhobosAirburstGlobals,
    /// Phobos `[SIDE] SuperWeaponSidebar.*PCX=` names, in `[Sides]` order.
    #[cfg(feature = "phobos")]
    side_sw_sidebar_art: Vec<crate::rules::phobos_sw_sidebar::SuperWeaponSidebarSideArt>,
    /// Effective side for each country after `[Sides]` membership and the
    /// per-country `Side=` override have both run.
    country_sides: Vec<Option<SideIdx>>,
    /// `[Colors]` scheme entries in declaration order. Source of truth for every
    /// house-color producer (loading-bar backing, lobby swatch, map `Color=`,
    /// skirmish slot priority).
    pub color_schemes: Vec<crate::rules::color_scheme::ColorSchemeEntry>,
    /// Per-`[Colors]`-entry team-color ramps (palette indices 16..31), built once
    /// from `color_schemes`. Indexed by `HouseColorIndex` (= `[Colors]` entry
    /// index). Consumed by unit/building atlas bake, the voxel GPU ramp texture,
    /// radar dots, target lines, and the loading screen.
    pub house_color_ramps: crate::rules::house_colors::HouseColorRamps,
    /// Fixed source-ordered `[ColorAdd]` slots retained as raw RGB565 magnitudes.
    pub color_add: crate::rules::color_add::ColorAddTable,
    pub production: ProductionRules,
    /// Global gameplay constants (vision, gap generator, etc.).
    pub general: GeneralRules,
    /// `[General] RevealTriggerRadius`, Rules+62C: ctor665EDE stores5;
    /// ReadGeneral67181C..671836 uses ReadInt5276D0 over the retained field,
    /// without clamping. Ordered rule passes use the shared typed projection.
    /// Executed reader controls: tools/spatial_oracle/trigger_action_values.
    reveal_trigger_radius: i32,
    /// Signed, unclamped `[AI] AIBaseSpacing`; constructor default is 1.
    pub ai_base_spacing: i32,
    /// Source-ordered `[General] Shipyard=` BuildingType identities.
    pub shipyard_types: Vec<String>,
    /// Retained `[General] RepairBay=` BuildingType identities (Rules+850).
    pub repair_bay_types: Vec<String>,
    /// Source-ordered resolved `[AI] BuildConst=` BuildingType identities.
    pub build_const_types: Vec<String>,
    /// Source-ordered resolved `[AI] BuildPower=` BuildingType identities.
    pub build_power_types: Vec<String>,
    /// Source-ordered resolved `[AI] BuildRefinery=` BuildingType identities.
    pub build_refinery_types: Vec<String>,
    /// Source-ordered resolved `[AI] BuildBarracks=` BuildingType identities.
    pub build_barracks_types: Vec<String>,
    /// Source-ordered `[AI] BuildTech=` identities used by the native
    /// FirstBuildableFromArray superweapon-disabled tail.
    pub build_tech_types: Vec<String>,
    /// Source-ordered resolved `[AI] BuildWeapons=` BuildingType identities.
    pub build_weapons_types: Vec<String>,
    /// Source-ordered resolved `[AI] BuildRadar=` BuildingType identities.
    pub build_radar_types: Vec<String>,
    /// Source-ordered resolved `[AI] AlliedBaseDefenses=`,
    /// `SovietBaseDefenses=` and `ThirdBaseDefenses=` BuildingType identities
    /// (`Rules+0x954`, `+0x970`, `+0x98C`, read like the other BasePlan lists
    /// at `0x00673058..0x006732D0`): the computer's base defense candidates by
    /// side ([`Self::base_defense_types`]).
    pub allied_base_defense_types: Vec<String>,
    pub soviet_base_defense_types: Vec<String>,
    pub third_base_defense_types: Vec<String>,
    /// Source-ordered resolved `[AI] ConcreteWalls=` BuildingType identities
    /// (the TypeList at `Rules+0xA50`, items `+0xA54`, count `+0xA60`, read
    /// like the lists above at `0x0067375B..0x00673824`; the constructor
    /// leaves it empty, `0x0066641B..0x00666430`): the computer's wall types
    /// (`sim::ai_base_building`).
    pub concrete_wall_types: Vec<String>,
    /// Original RulesClass::ReadAI672AE0, NSGates673830..673870 and
    /// EWGates673875..6738A7: ordered BuildingType lists. Constructor
    ///666437..666468 leaves both empty; omitted/empty reads retain them.
    pub ns_gate_types: Vec<String>,
    pub ew_gate_types: Vec<String>,
    /// Source-ordered resolved `[General] HarvesterUnit=` UnitType identities.
    pub harvester_unit_types: Vec<String>,
    /// Signed Hard/Normal/Easy vectors consumed directly by BasePlan Recalc.
    pub ai_slave_miner_number: Vec<i32>,
    pub ai_extra_refineries: Vec<i32>,
    pub allied_base_defense_counts: Vec<i32>,
    pub soviet_base_defense_counts: Vec<i32>,
    pub third_base_defense_counts: Vec<i32>,
    /// Signed `[General] AINavalYardAdjacency=` in cells. Native constructor
    /// default is 20 and the consumer shifts it left by eight without clamping.
    pub ai_naval_yard_adjacency: i32,
    /// Reset value of `[SpecialFlags] InitialVeteran=`. The similarly named
    /// stock `[General]` key is not read by the native SpecialFlags parser.
    pub initial_veteran: bool,
    /// Infantry IDs in registry order.
    pub infantry_ids: Vec<String>,
    /// Vehicle IDs in registry order.
    pub vehicle_ids: Vec<String>,
    /// Aircraft IDs in registry order.
    pub aircraft_ids: Vec<String>,
    /// Building IDs in registry order.
    pub building_ids: Vec<String>,
    /// Maps structure ID (uppercase) → FactoryType for quick lookup.
    /// Built once at load time from all ObjectType entries with Factory= set.
    /// Used by production_tech to determine what a building produces without
    /// hardcoding building names.
    pub factory_map: HashMap<String, FactoryType>,
    /// The `[General] Prerequisite*` lists (Rules `+0x358..+0x3E4`), in
    /// [`PrerequisiteGroup::ALL`] order: an on-map building of any of them
    /// meets that group in CanBuild.
    prerequisite_lists: [Vec<Prerequisite>; 6],
    /// Immutable gameplay projection of the retained Rules registry catalog.
    ares_prerequisites: Option<GenericPrerequisites>,
    /// Immutable projection of NativeRulesRegistryState's ordered armor catalog.
    ares_armors: Option<crate::rules::ares_armor::ArmorCatalog>,
    emp_sparkles: Option<String>,
    #[cfg(feature = "ares")]
    ares_global_controls: crate::rules::native_processing::AresGlobalControls,
    /// Derived type-membership index for the lifecycle hooks; rebuilt together
    /// with the RuleSet after every load/reset/pass. The catalog above is its
    /// only source of truth. This cache is not serialized or hashed separately.
    ares_prerequisite_alternates: HashSet<TypeHandle>,
    /// All registered SW availability, independent of supported effect Types.
    /// Immutable projection of the retained ProcessedRules registry; the typed
    /// aux/list fields remain read-only compatibility views from that source.
    #[cfg(feature = "ares")]
    ares_super_weapon_metadata:
        BTreeMap<String, crate::rules::superweapon_type::AresSuperWeaponMetadata>,
    /// Phobos `SW.TechLevel`/`SW.AuxTechnos`/`SW.NegTechnos` for every
    /// registered superweapon, projected from the retained registry.
    #[cfg(feature = "phobos")]
    phobos_super_weapon_metadata:
        BTreeMap<String, crate::rules::superweapon_type::PhobosSuperWeaponAvailability>,
    /// Phobos `[GlobalControls] AllowParallelAIQueues=` and the
    /// `ForbidParallelAIQueues.*` category switches.
    #[cfg(feature = "phobos")]
    parallel_ai_queues: crate::rules::phobos_parallel_ai_queues::ParallelAiQueueRules,
    /// Rules-driven terrain land-type semantics keyed by TMP land byte.
    pub terrain_rules: TerrainRules,
    /// Native `[Tiberiums]` definitions in GameMD type order.
    pub tiberium_types: TiberiumTypeRegistry,
    /// Terrain object type definitions (TIBTRE*, TREE*, ROCK*, etc.) keyed by
    /// uppercase section name. Distinct from `terrain_rules` (land semantics);
    /// these are per-decoration-object types parsed from `[TerrainTypes]`.
    pub terrain_object_types: HashMap<String, TerrainObjectType>,
    /// Immutable projection of the process-resident Overlay type reader.
    overlay_registry: crate::rules::overlay_types::OverlayTypeRegistry,
    /// Rules-driven bridge destruction defaults.
    pub bridge_rules: BridgeRules,
    /// Scenario-start crate counts and crate overlay images from `[CrateRules]`.
    pub crate_rules: CrateRules,
    /// The fixed nineteen-entry `[Powerups]` crate outcome table. Native keeps
    /// it in four globals outside `RulesClass`; the slot order is what matters.
    pub powerups: crate::rules::powerups::PowerupTable,
    /// Garrison/bunker/open-topped combat multipliers from [CombatDamage].
    pub garrison_rules: GarrisonRules,
    /// `[WallModel] AlliedWallTransparency` — `RulesClass+0x1850`, read by
    /// `RulesClass::ReadWallModel` (body 0x0066D1F0-0x0066D262) at
    /// 0x0066D20E/0x0066D22E with key string 0x0083B39C, default `false` from
    /// the constructor store at 0x00667825. Its one consumer is the
    /// line-of-fire walk's wall arm (0x004CC458): when set, a wall belonging
    /// to a house allied with the firer stops blocking the shot. Stock rules
    /// set it to `no`.
    ///
    /// `[WallModel] WallPenetratorThreshold` — a DOUBLE at `+0x1858`, read at
    /// 0x0066D24A with key string 0x0083B384 and stored by the `FSTP` at
    /// 0x0066D24F — is deliberately NOT parsed here: it belongs to the AI's
    /// "fire through walls anyway" decision, a different mechanism with no
    /// consumer in this tree.
    pub allied_wall_transparency: bool,
    /// `[ElevationModel]`, InRange's range bonus for a shot fired downhill.
    pub elevation_model: ElevationModel,
    /// Per-cell radiation-field constants from [Radiation].
    pub radiation: RadiationRules,
    /// Phobos `[ShieldTypes]` catalog, read per Rules pass by the ordered
    /// Process owner. Empty in builds without the capability.
    #[cfg(feature = "phobos")]
    pub(crate) shield_types: crate::rules::phobos_shields::ShieldTypeCatalog,
    /// Phobos shield globals: `[CombatDamage] ShieldApplyArmorMult` and
    /// `[AudioVisual] Shield.ConditionYellow/ConditionRed`.
    #[cfg(feature = "phobos")]
    pub(crate) shield_globals: crate::rules::phobos_shields::ShieldGlobals,
    /// Phobos crit defaults: `[CombatDamage] Crit.*` and `[General]
    /// VeteranCritChance`.
    #[cfg(feature = "phobos")]
    pub(crate) crit_globals: crate::rules::phobos_crit::CritGlobals,
    /// Phobos `[General]` warhead animation defaults.
    #[cfg(feature = "phobos")]
    pub(crate) phobos_warhead_anim: crate::rules::phobos_warhead_anim::PhobosWarheadAnimGlobals,
    /// Phobos `[DigitalDisplayTypes]` catalog, read per Rules pass by the
    /// ordered Process owner. Empty in builds without the capability.
    #[cfg(feature = "phobos")]
    pub(crate) digital_display_types: crate::rules::digital_display::DigitalDisplayTypeCatalog,
    /// Phobos DigitalDisplay globals: the four `[AudioVisual]` category
    /// default lists and `DigitalDisplay.Health.FakeAtDisguise`.
    #[cfg(feature = "phobos")]
    pub(crate) digital_display_globals: crate::rules::digital_display::DigitalDisplayGlobals,
    /// Phobos `[AttachEffectTypes]` catalog, retained across Rules passes.
    #[cfg(feature = "phobos")]
    pub(crate) attach_effect_types: crate::rules::attach_effect::AttachEffectTypeCatalog,
    /// Phobos AttachEffect globals (`[General]`).
    #[cfg(feature = "phobos")]
    pub(crate) attach_effect_globals: crate::rules::attach_effect::AttachEffectGlobals,
    /// Phobos placement preview globals (`[AudioVisual] PlacementPreview*`).
    #[cfg(feature = "phobos")]
    pub(crate) placement_preview_globals:
        crate::rules::phobos_placement_preview::PlacementPreviewGlobals,
    /// Phobos `[General] SkirmishUnlimitedColors=`: the player-colour
    /// conversion every launcher and loading-screen consumer reads.
    #[cfg(feature = "phobos")]
    pub(crate) skirmish_color_mode: crate::rules::skirmish_colors::SkirmishColorMode,
    /// Radar event visual parameters (ping rectangles on minimap).
    pub radar_event_config: RadarEventConfig,
    /// All superweapon types indexed by ID (e.g., "LightningStormSpecial" → SuperWeaponType).
    pub super_weapons: HashMap<String, SuperWeaponType>,
    /// `[SuperWeaponTypes]` in list order — the `SuperWeaponTypeClass` array
    /// index gamemd switches on where it does not use `Type=` (the
    /// `BuildingClass::OnConstructionComplete 0x00446948` `*Detected` jump
    /// table indexes this list: stock `1=NukeSpecial` is index 0).
    pub super_weapon_order: Vec<String>,
    /// Default particle systems from `[CombatDamage]` (smoke, sparks, debris, fire-stream).
    pub combat_damage: CombatDamageDefaults,
    /// Pre-resolved bridge-related warhead names (`[CombatDamage]
    /// IonCannonWarhead=`, `C4Warhead=`, `CrushWarhead=`). Resolution to interned IDs happens
    /// at world init.
    pub bridge_warheads: crate::rules::bridge_warheads::BridgeWarheads,
    /// Mind-control globals: ring anim, Mastermind overload, sounds and the
    /// AI capture-decision tables.
    pub mind_control: crate::rules::mind_control_rules::MindControlRules,
    /// The three hardcoded missile-spawn families (`[General] V3RocketType=`,
    /// `DMislType=`, `CMislType=`) with their launch frames, impact damage and
    /// warheads. Read by the spawn manager to classify a spawn child and by the
    /// missile detonation path.
    pub missile_spawn: crate::rules::missile_spawn::MissileSpawnRules,
    /// `[CombatDamage] C4Delay=` (`Rules+0x1750`), minutes: ReadDouble
    /// (`0x0066CDA2`) with the field as its default, which the constructor
    /// sets to 0.03 (`0x006675F0`). Only the C4 plant reads it
    /// ([`Self::c4_delay_frames`], [`Self::c4_flash_frames`]).
    pub c4_delay: f64,
    /// Particle types in registry order. Index = `ParticleTypeId.0`.
    particle_types: Vec<ParticleType>,
    /// Particle system types in registry order. Index = `ParticleSystemTypeId.0`.
    particle_system_types: Vec<ParticleSystemType>,
    /// Uppercase name → `ParticleSystemTypeId` for case-insensitive lookup.
    particle_system_types_by_name: HashMap<String, ParticleSystemTypeId>,
    /// `[VoxelAnims]` types in registry order. Index = `VoxelAnimTypeId.0`.
    voxel_anim_types: Vec<VoxelAnimType>,
    /// Uppercase name → `VoxelAnimTypeId` for case-insensitive lookup.
    voxel_anim_types_by_name: HashMap<String, VoxelAnimTypeId>,
    /// Smudge type registry parsed from `[SmudgeTypes]` and per-name sections.
    /// Populated by `RuleSet::from_ini` from rulesmd.ini.
    pub smudge_types: SmudgeTypeRegistry,
    /// Scenario-owned ART metadata and bound animation closure. Loaders transfer
    /// it once; every later binding and consumer uses this same registry.
    art_registry: crate::rules::art_data::ArtRegistry,
    /// Existing AnimTypes for Building451890's lookup-only427CB0 gate.
    /// Membership is independent of a same-named art section or loaded SHP.
    pub anim_type_names: BTreeSet<String>,
    /// Ordered native registry receipt: whether this type actually reached a
    /// successful fixed-ART ReadINI before the final rules pass completed.
    anim_type_art_read_states: Vec<(String, bool)>,
    /// Immutable projection of the same retained type owner's image calls.
    anim_type_images: Vec<(String, crate::rules::object_image::ObjectImageState)>,
    /// Each AnimType's fixed-ART Type references after the last pass, in the
    /// same registry order; installing ART data hands them to the registry.
    anim_type_references: Vec<(String, crate::rules::native_processing::AnimTypeReferences)>,
    /// GPU-independent SHP frame counts used by particle timing. Bound once
    /// from the active assets and ART data.
    effect_assets: crate::rules::effect_asset_catalog::EffectAssetCatalog,
    /// Immutable loaded main-VXL admission, rebuilt from ART/rules and the
    /// active AssetManager with effect assets. Gameplay and rendering share
    /// the GPU-independent physical model loader.
    voxel_assets: crate::rules::voxel_asset_catalog::VoxelAssetCatalog,
    /// Every building type's construction control from its Buildup SHP,
    /// bound once from the active assets (`bind_building_buildup_assets`).
    buildup_assets: crate::rules::buildup_asset_catalog::BuildupAssetCatalog,
    /// Rubble art of every `LeaveRubble=yes` building type, bound once from
    /// the active assets (`bind_building_rubble_assets`).
    rubble_assets: crate::rules::building_rubble::RubbleAssetCatalog,
    /// Raw terrain SHP counts used by authoritative TIBTRE animation timing.
    /// Presentation keeps its separate body-frame projection.
    terrain_assets: crate::rules::terrain_asset_catalog::TerrainAssetCatalog,
    /// Complete immutable per-object animation timing catalog. Gameplay reads
    /// this rules-owned resource directly; presentation cannot replace timing
    /// on an individual frame.
    animation_sequences: BTreeMap<String, crate::rules::animation_sequence::SequenceSet>,
    /// Each object handle's `animation_sequences` entry (by the object's
    /// canonical id), so a lookup by name needs no string search. Derived:
    /// every writer of the catalog rebuilds it; never hashed.
    animation_sequence_by_handle: Vec<Option<crate::rules::animation_sequence::SequenceSet>>,
    /// Original UnitType fixed-ART read state. The animation catalog derives
    /// its supported frame projection here; asset installation never rereads it.
    unit_shp_read_states: BTreeMap<String, crate::rules::shp_vehicle_sequence::UnitShpReadState>,
    /// Per-mission behaviour table parsed from the `[<MissionName>]` sections
    /// (Rate/AARate + NoThreat/Zombie/Recruitable/Paralyzed/Retaliate/Scatter).
    pub mission_control: MissionControl,
    /// Deterministic hash of the processed source INI (RULESMD, optional
    /// LANGRULE, selected mode, then the map's rules-shaped pass) this RuleSet
    /// was built from. Unlike a
    /// registry-only hash it is sensitive to scalar value overrides, so it can
    /// gate diagnostic-log/snapshot playback against a mismatched rules set. Lives on
    /// `RuleSet` (in `rules/`) so `sim/` can read it without an app-layer dep.
    source_ini_hash: u64,
}

impl RuleSet {
    /// Ares1003DB30/1003DB50. Both group labels and ordinary type IDs share
    /// one namespace; callers compare these keys case-insensitively.
    pub fn type_selection_key<'a>(&'a self, category: ObjectCategory, type_id: &'a str) -> &'a str {
        #[cfg(feature = "ares")]
        if let Some(object) = self.object_in_category(category, type_id) {
            return object.type_selection_key();
        }
        #[cfg(not(feature = "ares"))]
        let _ = category;
        type_id
    }

    /// CollectSelectedIDs732D47 -> Ares10041990/10041A3F gates the
    /// DeploysInto/UndeploysInto seeds. Default builds keep retail behavior.
    pub fn type_select_uses_deploy(&self) -> bool {
        #[cfg(feature = "ares")]
        {
            self.ares_global_controls.type_select_use_deploy()
        }
        #[cfg(not(feature = "ares"))]
        {
            true
        }
    }

    #[cfg(feature = "ares")]
    pub(crate) fn bounty_rules(&self) -> &crate::rules::ares_bounty::BountyGlobalRules {
        self.ares_global_controls.bounty()
    }

    /// Ares RulesExt+70, used by SW_HunterSeeker::Activate100728EB.
    #[cfg(feature = "ares")]
    pub(crate) fn hunter_seeker_buildings(&self) -> &[String] {
        self.ares_global_controls.hunter_seeker_buildings()
    }

    /// RulesExt+14A indexed by native House difficulty0/1/2 (hardest first).
    /// Only the compiled Ares CanBuild owner consumes this retained policy.
    pub fn ares_ai_bypasses_build_limit(&self, difficulty_index: usize) -> bool {
        #[cfg(feature = "ares")]
        {
            self.ares_global_controls
                .ai_bypasses_build_limit(difficulty_index)
        }
        #[cfg(not(feature = "ares"))]
        {
            let _ = difficulty_index;
            false
        }
    }

    /// RulesExt+160/+164 retained nullable named mission. Default builds
    /// neither read this extension key nor supply an AI EMP recovery producer.
    pub(crate) fn ares_emp_ai_recover_mission(
        &self,
    ) -> Option<crate::rules::mission_data::MissionType> {
        #[cfg(feature = "ares")]
        {
            self.ares_global_controls.emp_ai_recover_mission()
        }
        #[cfg(not(feature = "ares"))]
        {
            None
        }
    }

    pub(crate) fn bind_emp_sparkles(&mut self, name: Option<String>) {
        self.emp_sparkles = name;
    }
    pub(crate) fn emp_sparkles(&self) -> Option<&str> {
        self.emp_sparkles.as_deref()
    }

    pub(crate) fn reveal_trigger_radius(&self) -> i32 {
        self.reveal_trigger_radius
    }

    /// The immutable Overlay projection from this same processed Rules owner.
    pub fn overlay_registry(&self) -> &crate::rules::overlay_types::OverlayTypeRegistry {
        &self.overlay_registry
    }

    /// Build from the active ordered rules sources.
    pub fn from_rules_layers(layers: &RulesLayerStack) -> Result<Self, RulesError> {
        Self::from_processed_rules(&layers.process()?)
    }

    pub fn from_processed_rules(processed: &ProcessedRulesLayers) -> Result<Self, RulesError> {
        processed.validate_saved_state()?;
        let mut rules = Self::from_projected_ini(
            processed,
            project_country_side_registry(
                processed.country_projection(),
                processed.side_projection(),
            ),
        )?;
        let mut native_identity_counts = HashMap::new();
        for (category, id, _) in processed.techno_type_states() {
            *native_identity_counts
                .entry((category, id.to_ascii_uppercase()))
                .or_insert(0usize) += 1;
        }
        for (category, id, tech_level) in processed.techno_type_states() {
            let key = (category, id.to_ascii_uppercase());
            let object = rules
                .object_category_index
                .get(&key)
                .copied()
                .filter(|handle| rules.object_by_handle(*handle).id == id);
            if let Some(object) = object {
                // TechnoType+634 is retained typed state, not a merged INI
                // default: reached ReadINI714570 and outgoing106 handoff.
                // Equal truncated stored IDs share one projected object, which
                // the last such member's body built; its value is written last.
                rules.object_list[object.0 as usize].tech_level = tech_level;
            }
            let handle = object.filter(|_| native_identity_counts[&key] == 1);
            rules
                .native_techno_type_order
                .push((category, id.to_owned(), handle));
        }
        for (category, name, slots) in processed.weapon_slot_states() {
            if let Some(object) = rules
                .object_list
                .iter_mut()
                .find(|object| object.category == category && object.id == name)
            {
                object.turret_count = slots.turret_count();
                object.weapon_count = slots.weapon_count();
                object.weapon_list = slots.base();
                object.elite_weapon_list = slots.elite();
                object.bind_weapon_offsets(slots.offsets());
            }
        }
        rules.crate_rules = processed.crate_rules().clone();
        rules.powerups = processed.powerups().clone();
        rules.general.metallic_debris = processed.metallic_debris().to_vec();
        rules.general.weather_con_clouds = processed.weather_con_clouds().to_vec();
        rules.general.weather_con_bolts = processed.weather_con_bolts().to_vec();
        let [amer, ally, sov, yuri] = processed.paradrop_infantry().clone();
        rules.general.amer_paradrop.infantry = amer;
        rules.general.ally_paradrop.infantry = ally;
        rules.general.sov_paradrop.infantry = sov;
        rules.general.yuri_paradrop.infantry = yuri;
        rules.general.anim_to_infantry = processed.anim_to_infantry().to_vec();
        rules.general.secret_lab_choices = processed.secret_lab_choices().clone();
        rules.general.default_mirage_disguises = processed.default_mirage_disguises().to_vec();
        rules.general.infantry_disguises = processed.infantry_disguises().clone();
        rules.bridge_rules.explosions = processed.bridge_explosions().to_vec();
        rules.general.gravity = processed.gravity();
        rules.general.wind_direction = processed.wind_direction();
        rules.general.difficulty_rows = *processed.difficulty_rows();
        rules.general.detail = processed.detail();
        rules.general.talk_bubble_time = processed.talk_bubble_time();
        rules.general.spotlight = processed.spotlight();
        let (damage, blast, beam, ion_cannon) = processed.ion_blast_inputs();
        rules.bridge_warheads.ion_cannon_name = ion_cannon.to_owned();
        rules.general.ion_cannon_damage = damage;
        rules.general.ion_blast_anim = blast.to_string();
        rules.general.ion_beam_anim = beam.to_string();
        let (ambient_rate, ambient_step) = processed.ambient_change_inputs();
        (
            rules.general.ambient_change_rate_nonzero,
            rules.general.ambient_change_interval_frames,
            rules.general.ambient_change_step,
        ) = ambient_change_terms(
            f64::from_bits(ambient_rate.bits()),
            f64::from_bits(ambient_step.bits()),
        );
        rules.general.prism_support = processed.prism_support();
        rules.general.patrol_scan_minutes = processed.patrol_scan_minutes();
        (rules.general.shroud_grow, rules.general.shroud_rate) = processed.shroud_growth_inputs();
        rules.general.fog_rate = processed.fog_rate();
        rules.general.prism_type = processed.prism_type().map(str::to_owned);
        #[cfg(feature = "phobos")]
        {
            rules.general.timer_blink_color_scheme = processed.timer_blink_color_scheme();
            rules.general.missing_cameo = processed.missing_cameo().to_owned();
        }
        let (drop_zone_anim, drop_zone_radius) = processed.drop_zone_controls();
        rules.general.drop_zone_anim = drop_zone_anim.map(str::to_owned);
        rules.general.drop_zone_radius = drop_zone_radius;
        rules.general.behind = processed.behind().map(str::to_owned);
        rules.general.prerequisite_proc_alternate =
            processed.prerequisite_proc_alternate().map(str::to_owned);
        rules.prerequisite_lists = processed.prerequisite_lists().clone();
        rules.ares_prerequisites = processed.ares_prerequisites().cloned();
        rules.ares_prerequisite_alternates = rules
            .ares_prerequisites
            .as_ref()
            .into_iter()
            .flat_map(|catalog| catalog.groups())
            .flat_map(|group| group.alternates())
            .filter_map(|alternate| {
                rules
                    .object_category_index
                    .get(&(alternate.category(), alternate.id().to_ascii_uppercase()))
                    .copied()
            })
            .collect();
        rules.general.building_types = processed.building_types().clone();
        let (lightning, weather_anim, nullify_anim, splash) = processed.select_anim_rules();
        rules.general.lightning_warhead = lightning.to_owned();
        rules.general.weather_con_bolt_explosion = weather_anim.to_owned();
        rules.general.weapon_nullify_anim = nullify_anim.to_owned();
        rules.combat_damage.splash_list = splash.to_vec();
        rules.combat_damage.ammo_crate_damage = processed.ammo_crate_damage();
        rules.combat_damage.tiberium_explosion_damage = processed.tiberium_explosion_damage();
        rules.combat_damage.death_weapon =
            processed.combat_damage_death_weapon().map(str::to_owned);
        for (name, conventional, em_effect, anim_list, debris_types, verses, ares_verses) in
            processed.warhead_read_states()
        {
            if let Some(warhead) = rules
                .warheads
                .values_mut()
                .find(|wh| wh.id.eq_ignore_ascii_case(name))
            {
                warhead.conventional = conventional;
                warhead.em_effect = em_effect;
                warhead.anim_list = anim_list.to_vec();
                warhead.debris_types = debris_types.to_vec();
                warhead.verses_f64 = verses.map(|value| f64::from_bits(value.bits()));
                warhead.install_ares_verses(ares_verses);
            }
        }
        for (name, particle_system) in processed.warhead_particle_states() {
            if let Some(warhead) = rules
                .warheads
                .values_mut()
                .find(|warhead| warhead.id.eq_ignore_ascii_case(name))
            {
                warhead.particle_system = particle_system.map(str::to_owned);
            }
        }
        for (
            category,
            name,
            emp,
            abilities,
            attachment,
            cloak_allowed,
            turret_rot,
            aircraft_crashable,
        ) in processed.ares_techno_states()
        {
            if let Some(object) = rules
                .object_list
                .iter_mut()
                .find(|object| object.category == category && object.id == name)
            {
                object.bind_ares_techno_rules(
                    emp,
                    abilities,
                    attachment,
                    cloak_allowed,
                    turret_rot,
                    aircraft_crashable,
                );
            }
        }
        #[cfg(feature = "ares")]
        for (category, name, enabled, options) in processed.ares_spotlight_states() {
            if let Some(handle) = rules
                .object_category_index
                .get(&(category, name.to_ascii_uppercase()))
                .copied()
            {
                rules.object_list[handle.0 as usize].bind_ares_spotlight(enabled, options);
            }
        }
        #[cfg(feature = "ares")]
        for (category, name, retained) in processed.super_weapon_techno_states() {
            if let Some(handle) = rules
                .object_category_index
                .get(&(category, name.to_ascii_uppercase()))
                .copied()
            {
                rules.object_list[handle.0 as usize].bind_super_weapon_techno_rules(
                    retained.designator_range,
                    retained.inhibitor_range,
                    retained.hunter_seeker_ignore,
                );
            }
        }
        #[cfg(feature = "ares")]
        for (category, name, group) in processed.selection_group_states() {
            if let Some(handle) = rules
                .object_category_index
                .get(&(category, name.to_ascii_uppercase()))
                .copied()
            {
                rules.object_list[handle.0 as usize].bind_selection_group(group);
            }
        }
        #[cfg(feature = "ares")]
        for (category, name, driver) in processed.driver_states() {
            if let Some(handle) = rules
                .object_category_index
                .get(&(category, name.to_ascii_uppercase()))
                .copied()
            {
                rules.object_list[handle.0 as usize].bind_driver_rules(driver);
            }
        }
        #[cfg(feature = "ares")]
        for (category, name, bounty) in processed.bounty_states() {
            if let Some(handle) = rules
                .object_category_index
                .get(&(category, name.to_ascii_uppercase()))
                .copied()
            {
                rules.object_list[handle.0 as usize].bind_bounty_rules(bounty);
            }
        }
        #[cfg(feature = "ares")]
        for (category, name, crew) in processed.crew_states() {
            if let Some(handle) = rules
                .object_category_index
                .get(&(category, name.to_ascii_uppercase()))
                .copied()
            {
                rules.object_list[handle.0 as usize].bind_crew_rules(crew);
            }
        }
        for (name, ares) in processed.ares_warhead_states() {
            if let Some(warhead) = rules
                .warheads
                .values_mut()
                .find(|wh| wh.id.eq_ignore_ascii_case(name))
            {
                warhead.bind_ares_rules(ares);
            }
        }
        for (category, name, armor) in processed.object_armor_states() {
            if let Some(category) = category {
                if let Some(handle) = rules
                    .object_category_index
                    .get(&(category, name.to_ascii_uppercase()))
                    .copied()
                {
                    rules.object_list[handle.0 as usize].bind_armor(armor);
                }
            } else if let Some(terrain) = rules
                .terrain_object_types
                .get_mut(&name.to_ascii_uppercase())
            {
                terrain.bind_armor(armor);
            }
        }
        (
            rules.general.missile_rot_var,
            rules.general.safety_altitude,
            rules.general.line_trail_color_override,
        ) = processed.projectile_rule_controls();
        rules.general.chrono_beam_color = processed.chrono_beam_color();
        (
            rules.general.parachute_max_fall_rate,
            rules.general.no_parachute_max_fall_rate,
        ) = processed.fall_rate_controls();
        [
            rules.general.allied_survivor_divisor,
            rules.general.soviet_survivor_divisor,
            rules.general.third_survivor_divisor,
        ] = processed.survivor_divisors();
        rules.general.maximum_cheer_rate = processed.maximum_cheer_rate();
        rules.general.comp_easy_bonus = processed.comp_easy_bonus();
        (
            rules.general.infantry_repair_step,
            rules.general.unit_repair_rate,
            rules.general.infantry_repair_rate,
        ) = processed.repair_service_inputs();
        rules.general.charge_to_drain_ratio = processed.charge_to_drain_ratio();
        let mut weapon_name_counts = HashMap::<String, usize>::new();
        for weapon in processed.weapon_states() {
            *weapon_name_counts
                .entry(weapon.id.to_ascii_uppercase())
                .or_default() += 1;
        }
        let mut warhead_name_counts = HashMap::<String, usize>::new();
        for (name, ..) in processed.warhead_read_states() {
            *warhead_name_counts
                .entry(name.to_ascii_uppercase())
                .or_default() += 1;
        }
        for weapon in processed.weapon_states() {
            // Abstract410800 truncates stored IDs to24 bytes, but native
            // FindOrAllocate compares the full incoming name. Do not alias two
            // such Types through Action42's ordinal lookup. The wider existing
            // name-projection limitation remains issue1784, without rejecting
            // unrelated custom scenarios at load.
            let unambiguous = weapon_name_counts[&weapon.id.to_ascii_uppercase()] == 1
                && weapon.warhead.is_none_or(|name| {
                    warhead_name_counts.get(&name.to_ascii_uppercase()) == Some(&1)
                });
            rules.weapon_array.push((weapon.id.to_owned(), unambiguous));
        }
        for (name, speed_type) in processed.unit_speed_type_states() {
            if let Some(object) = rules
                .object_list
                .iter_mut()
                .find(|object| object.category == ObjectCategory::Vehicle && object.id == name)
            {
                object.speed_type = speed_type;
            }
        }
        for (category, name, airstrike) in processed.airstrike_states() {
            if let Some(object) = rules
                .object_list
                .iter_mut()
                .find(|object| object.category == category && object.id == name)
            {
                object.airstrike = airstrike.clone();
            }
        }
        for (category, name, recoil) in processed.recoil_states() {
            if let Some(object) = rules
                .object_list
                .iter_mut()
                .find(|object| object.category == category && object.id == name)
            {
                object.recoil = recoil;
            }
        }
        for (category, name, segments) in processed.radial_fire_segments_states() {
            if let Some(object) = rules
                .object_list
                .iter_mut()
                .find(|object| object.category == category && object.id == name)
            {
                object.radial_fire_segments = segments;
            }
        }
        for (category, name, offset) in processed.lepton_mind_control_offset_states() {
            if let Some(object) = rules
                .object_list
                .iter_mut()
                .find(|object| object.category == category && object.id == name)
            {
                object.lepton_mind_control_offset = offset;
            }
        }
        for (category, name, prerequisite, prerequisite_override) in processed.prerequisite_states()
        {
            if let Some(object) = rules
                .object_list
                .iter_mut()
                .find(|object| object.category == category && object.id == name)
            {
                object.prerequisite = prerequisite.to_vec();
                object.prerequisite_override = prerequisite_override.to_vec();
            }
        }
        for (category, name, gunner_turrets) in processed.gunner_turret_states() {
            if let Some(object) = rules
                .object_list
                .iter_mut()
                .find(|object| object.category == category && object.id == name)
            {
                object.gunner = gunner_turrets.is_gunner();
                object.gunner_turrets = gunner_turrets.clone();
            }
        }
        #[cfg(feature = "ares")]
        for (category, name, state) in processed.factory_owner_states() {
            if let Some(object) = rules
                .object_list
                .iter_mut()
                .find(|object| object.category == category && object.id == name)
            {
                object.project_factory_owners(state);
            }
        }
        #[cfg(feature = "ares")]
        for (name, state) in processed.explicit_factory_states() {
            if let Some(object) = rules
                .object_list
                .iter_mut()
                .find(|object| object.category == ObjectCategory::Building && object.id == name)
            {
                object.project_explicit_factory(state);
            }
        }
        for (category, name, references, image) in processed.techno_reference_states() {
            if let Some(object) = rules
                .object_list
                .iter_mut()
                .find(|object| object.category == category && object.id == name)
            {
                object.project_image_state(image);
                references.apply_to(object);
            }
        }
        for (category, name, shadow_index) in processed.voxel_shadow_states() {
            if let Some(object) = rules
                .object_list
                .iter_mut()
                .find(|object| object.category == category && object.id == name)
            {
                object.set_voxel_shadow_index(shadow_index);
            }
        }
        for (name, pips) in processed.infantry_pip_states() {
            if let Some(object) = rules
                .object_list
                .iter_mut()
                .find(|object| object.category == ObjectCategory::Infantry && object.id == name)
            {
                object.project_infantry_pips(pips);
            }
        }
        for (name, reads) in processed.infantry_sequence_sound_reads() {
            if let Some(object) = rules
                .object_list
                .iter_mut()
                .find(|object| object.category == ObjectCategory::Infantry && object.id == name)
            {
                object.project_sequence_sound_reads(reads);
            }
        }
        for (name, flags) in processed.infantry_infiltration_states() {
            if let Some(object) = rules
                .object_list
                .iter_mut()
                .find(|object| object.category == ObjectCategory::Infantry && object.id == name)
            {
                object.project_infantry_infiltration(flags);
            }
        }
        rules.anim_type_art_read_states = processed
            .anim_type_art_read_states()
            .map(|(name, read)| (name.to_owned(), read))
            .collect();
        rules.anim_type_images = processed
            .anim_type_image_states()
            .map(|(name, image)| (name.to_owned(), image.clone()))
            .collect();
        rules.anim_type_references = processed
            .anim_type_reference_states()
            .map(|(name, references)| (name.to_owned(), references.clone()))
            .collect();
        rules.unit_shp_read_states = processed
            .unit_shp_read_states()
            .map(|(name, state)| (name.to_owned(), *state))
            .collect();
        // Building ctor45DF13 / ReadINI461225..46125D: the process-resident
        // owner retained +EF0 across reached rules passes and exact ART reads.
        // Projection receives its result; RULES Foundation is not an input.
        for (name, config) in processed.secret_lab_states() {
            if let Some(building) = rules
                .object_list
                .iter_mut()
                .find(|object| object.category == ObjectCategory::Building && object.id == name)
            {
                building.set_secret_lab(config.clone());
            }
        }
        for (name, foundation_id) in processed.building_foundation_geometries() {
            if let Some(building) = rules
                .object_list
                .iter_mut()
                .find(|object| object.category == ObjectCategory::Building && object.id == name)
            {
                building.foundation = foundation_id.clone();
                building.base_reservation_spacing = building
                    .base_reservation_writer_eligible()
                    .then_some(rules.ai_base_spacing);
            }
        }
        for (name, flag) in processed.building_spotlight_flags() {
            if let Some(building) = rules
                .object_list
                .iter_mut()
                .find(|o| o.category == ObjectCategory::Building && o.id == name)
            {
                building.has_spotlight = flag;
            }
        }
        for (name, light) in processed.building_light_states() {
            if let Some(building) = rules
                .object_list
                .iter_mut()
                .find(|o| o.category == ObjectCategory::Building && o.id == name)
            {
                building.project_building_light(light);
            }
        }
        for (name, upgrade) in processed.building_upgrade_states() {
            if let Some(building) = rules
                .object_list
                .iter_mut()
                .find(|o| o.category == ObjectCategory::Building && o.id == name)
            {
                building.project_building_upgrade(upgrade.clone());
            }
        }
        for (name, extra_power) in processed.building_extra_power_states() {
            if let Some(building) = rules
                .object_list
                .iter_mut()
                .find(|object| object.category == ObjectCategory::Building && object.id == name)
            {
                building.extra_power = extra_power;
            }
        }
        for (name, offset) in processed.building_target_coord_offsets() {
            if let Some(building) = rules
                .object_list
                .iter_mut()
                .find(|object| object.category == ObjectCategory::Building && object.id == name)
            {
                building.set_target_coord_offset(offset);
            }
        }
        for (name, location1, location2) in processed.building_half_damage_smoke_locations() {
            if let Some(building) = rules
                .object_list
                .iter_mut()
                .find(|object| object.category == ObjectCategory::Building && object.id == name)
            {
                building.set_half_damage_smoke_locations(location1, location2);
            }
        }
        // The registry processor owns native read timing and retained values;
        // the runtime definition receives that result, not another ART read.
        for (name, art, image) in processed.projectile_art_states() {
            if let Some(projectile) = rules
                .projectiles
                .values_mut()
                .find(|projectile| projectile.id.eq_ignore_ascii_case(name))
            {
                art.apply_to(image, projectile);
            }
        }
        for (name, (airburst, shrapnel)) in processed.projectile_weapon_states() {
            if let Some(projectile) = rules
                .projectiles
                .values_mut()
                .find(|projectile| projectile.id.eq_ignore_ascii_case(name))
            {
                projectile.airburst_weapon = airburst.map(str::to_owned);
                projectile.shrapnel_weapon = shrapnel.map(str::to_owned);
            }
        }
        for (name, (weapon, aux_building)) in processed.super_weapon_reference_states() {
            if let Some(super_weapon) = rules
                .super_weapons
                .values_mut()
                .find(|super_weapon| super_weapon.id.eq_ignore_ascii_case(name))
            {
                super_weapon.weapon_type = weapon.map(str::to_owned);
                super_weapon.aux_building = aux_building.map(str::to_owned);
            }
        }
        for (name, requirements) in processed.super_weapon_building_requirements() {
            if let Some(super_weapon) = rules
                .super_weapons
                .values_mut()
                .find(|super_weapon| super_weapon.id.eq_ignore_ascii_case(name))
            {
                super_weapon.building_requirements = requirements.cloned();
            }
        }
        #[cfg(feature = "phobos")]
        {
            rules.phobos_super_weapon_metadata.clear();
            for (name, availability) in processed.phobos_super_weapon_availability_states() {
                rules
                    .phobos_super_weapon_metadata
                    .insert(name.to_ascii_uppercase(), availability);
            }
            for (name, gathering) in processed.ore_gathering_states() {
                if let Some(unit) = rules.object_list.iter_mut().find(|object| {
                    object.category == ObjectCategory::Vehicle
                        && object.id.eq_ignore_ascii_case(name)
                }) {
                    unit.project_ore_gathering(gathering);
                }
            }
        }
        #[cfg(feature = "ares")]
        {
            rules.ares_super_weapon_metadata.clear();
            for (name, metadata) in processed.super_weapon_availability_states() {
                rules
                    .ares_super_weapon_metadata
                    .insert(name.to_ascii_uppercase(), metadata);
            }
            for (name, presentation) in processed.super_weapon_presentation_states() {
                if let Some(sw) = rules
                    .super_weapons
                    .values_mut()
                    .find(|sw| sw.id.eq_ignore_ascii_case(name))
                {
                    sw.presentation = presentation.clone();
                }
            }
            for (name, supers) in processed.additional_super_weapon_states() {
                if let Some(building) = rules.object_list.iter_mut().find(|object| {
                    object.category == ObjectCategory::Building
                        && object.id.eq_ignore_ascii_case(name)
                }) {
                    building.project_additional_super_weapons(supers);
                }
            }
        }
        for (name, settings) in processed.hunter_seeker_states() {
            if let Some(sw) = rules
                .super_weapons
                .values_mut()
                .find(|sw| sw.id.eq_ignore_ascii_case(name))
            {
                sw.hunter_seeker = settings.cloned();
            }
        }
        for (name, delivery) in processed.unit_delivery_states() {
            if let Some(sw) = rules
                .super_weapons
                .values_mut()
                .find(|sw| sw.id.eq_ignore_ascii_case(name))
            {
                sw.unit_delivery = delivery.cloned();
            }
        }
        // Equal truncated stored IDs share one projected type, which the last
        // such member's state is written to last.
        for (name, warhead) in processed.generic_super_weapon_warhead_states() {
            if let Some(settings) = rules
                .super_weapons
                .values_mut()
                .find(|super_weapon| super_weapon.id.eq_ignore_ascii_case(name))
                .and_then(|super_weapon| super_weapon.generic_warhead.as_mut())
            {
                settings.warhead = warhead.map(str::to_owned);
            }
        }
        for (name, warhead) in processed.particle_warhead_states() {
            if let Some(particle) = rules
                .particle_types
                .iter_mut()
                .find(|particle| particle.name.eq_ignore_ascii_case(name))
            {
                particle.warhead = warhead.map(str::to_owned);
            }
        }
        for (name, references, damage) in processed.voxel_anim_reference_states() {
            if let Some(voxel_anim) = rules
                .voxel_anim_types
                .iter_mut()
                .find(|voxel_anim| voxel_anim.name.eq_ignore_ascii_case(name))
            {
                references.apply_to(voxel_anim);
                voxel_anim.damage = damage;
            }
        }
        processed.rules_type_references().apply_to(&mut rules);
        Ok(rules)
    }

    /// The `ids` whose Type has an object of `category`. The ordered Rules
    /// processor allocates a Type for each token its factory misses, and
    /// native keeps it in the list; VERA's projection builds no object for
    /// some of them (a token with surrounding spaces) and drops them here, an
    /// approved exclusion that retail never reaches.
    pub(crate) fn ids_with_objects(&self, ids: &[String], category: ObjectCategory) -> Vec<String> {
        ids.iter()
            .filter(|id| {
                self.object_category_index
                    .contains_key(&(category, id.to_ascii_uppercase()))
            })
            .cloned()
            .collect()
    }

    /// `[AI] BuildConst=` and the BuildingTypes it makes eligible.
    pub(crate) fn set_build_const_types(&mut self, ids: &[String]) {
        self.build_const_types = self.ids_with_objects(ids, ObjectCategory::Building);
        for object in &mut self.object_list {
            object.build_const_eligible = false;
        }
        for id in self.build_const_types.clone() {
            let handle =
                self.object_category_index[&(ObjectCategory::Building, id.to_ascii_uppercase())];
            self.object_list[handle.0 as usize].build_const_eligible = true;
        }
    }

    /// Resolve type sound readers against the startup-selected
    /// SOUNDMD registry. The processed projection retains only passes where
    /// each type existed, including the native current-ID retention order.
    /// Type defaults and Rules+208 start at -1; [AudioVisual] owns the latter.
    pub fn bind_type_sound_references(
        &mut self,
        ini: &IniFile,
        sounds: &crate::rules::sound_ini::SoundRegistry,
    ) {
        // Original669423..6694B6, in native open/close read order. The
        // processed section already owns the reached Rules pass history.
        self.general.gui_open_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "GUIOpenSound"));
        self.general.gui_close_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "GUICloseSound"));
        self.general.disk_laser_charge_up = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "DiskLaserChargeUp"));
        self.general.create_unit_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "CreateUnitSound"));
        self.general.create_infantry_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "CreateInfantrySound"));
        self.general.create_aircraft_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "CreateAircraftSound"));
        // Original66A763..66A973, in native read order for these bound fields. Use the same fixed
        // registry and history-aware reader as every other bound Voc ID.
        self.general.game_closed_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "GameClosed"));
        self.general.incoming_message_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "IncomingMessage"));
        self.general.message_char_typed_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "MessageCharTyped"));
        self.general.system_error_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "SystemError"));
        self.general.options_changed_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "OptionsChanged"));
        self.general.game_forming_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "GameForming"));
        self.general.player_left_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "PlayerLeft"));
        self.general.player_joined_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "PlayerJoined"));
        self.general.airstrike_abort_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "AirstrikeAbortSound"));
        self.general.airstrike_attack_voice = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "AirstrikeAttackVoice"));
        self.general.leave_bio_reactor_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "LeaveBioReactorSound"));
        self.general.enter_bio_reactor_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "EnterBioReactorSound"));
        self.general.enter_grinder_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "EnterGrinderSound"));
        self.general.sinking_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "SinkingSound"));
        self.general.score_anim_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "ScoreAnimSound"));
        self.general.construction_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "Construction"));
        self.general.building_abandoned_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "BuildingAbandonedSound"));
        self.general.building_damage_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "BuildingDamageSound"));
        self.general.building_repaired_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "BuildingRepairedSound"));
        self.general.spy_plane_camera = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "SpyPlaneCamera"));
        self.general.stop_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "StopSound"));
        self.general.guard_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "GuardSound"));
        // Original66AAC0..66AB53: fixed process catalog, exact keys and
        // current-ID retention across the existing processed read history.
        self.general.radar_on = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "RadarOn"));
        self.general.radar_off = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "RadarOff"));
        self.general.movie_on = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "MovieOn"));
        self.general.movie_off = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "MovieOff"));
        // Original66A4EF..66A582 shares the retained fixed-catalog reader.
        self.general.spy_sat_activation_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "SpySatActivationSound"));
        self.general.spy_sat_deactivation_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "SpySatDeactivationSound"));
        self.general.voice_ifv_repair = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "VoiceIFVRepair"));
        // Original ReadAudioVisual order: Start6696C0, End66970A, Add669B25.
        // The processed section owns pass history; the fixed catalog owns IDs.
        self.general.start_planning_mode_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "StartPlanningModeSound"));
        self.general.end_planning_mode_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "EndPlanningModeSound"));
        self.general.add_planning_mode_command_sound =
            ini.section("AudioVisual").and_then(|section| {
                sounds.read_rules_reference(section, "AddPlanningModeCommandSound")
            });
        // Original669BA8, after Add669B25 in the same AudioVisual pass.
        self.general.place_beacon_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "PlaceBeaconSound"));
        self.general.time_warp_out_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "LetsDoTheTimeWarpOutAgain"));
        self.general.time_warp_in_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "LetsDoTheTimeWarpInAgain"));
        self.general.cheer_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "CheerSound"));
        self.general.scatter_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "ScatterSound"));
        self.general.scold_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "ScoldSound"));
        // Original RulesClass::ReadAudioVisual669CE2..669D68 resolves
        // +228/+22C through ReadString128/VocFind7514D0, independently of
        // ReadGeneral. Reuse the fixed-catalog/pass-history owner, including
        // invalid later reads retaining their previous IDs (ctor66596A/70=-1).
        self.general.upgrade_veteran_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "UpgradeVeteranSound"));
        self.general.upgrade_elite_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "UpgradeEliteSound"));
        // WeaponTypeClass::ReadINI @ `0x00772080` calls
        // CCINIClass::ReadSoundList @ `0x00525430` for Report/DownReport at
        // `0x00772394`/`0x0077241A`; use its shared catalog/list owner.
        for weapon in self.weapons.values_mut() {
            let section = ini.section(&weapon.id);
            weapon.report = section.map_or_else(Vec::new, |section| {
                sounds.read_rules_sound_list(section, "Report")
            });
            weapon.down_report = section.map_or_else(Vec::new, |section| {
                sounds.read_rules_sound_list(section, "DownReport")
            });
        }
        for sw in self.super_weapons.values_mut() {
            let section = ini.section(&sw.id);
            #[cfg(feature = "ares")]
            {
                // SWTypeExt::LoadFromINI10033522 -> NullableIdx<Voc>
                // 1000E9D0: successful names/none set presence; unknown
                // retains. Read against fixed SOUNDMD, in rules-pass order.
                sw.presentation.sound =
                    section.and_then(|section| sounds.read_ares_reference(section, "SW.Sound"));
            }
            // Original6CEB74..6CEBD8 keeps each current Voc ID when
            // ReadString128 or FindIndex7514D0 finds no replacement.
            sw.special_sound =
                section.and_then(|section| sounds.read_rules_reference(section, "SpecialSound"));
            sw.start_sound =
                section.and_then(|section| sounds.read_rules_reference(section, "StartSound"));
            #[cfg(feature = "ares")]
            if let Some(delivery) = sw.unit_delivery.as_mut() {
                // Ares NullableIdx<VocClass>: unknown retains the previous
                // resolved cue; an explicit none sentinel clears it. The
                // retained type INI owns the ordered read history.
                delivery.activation_sound = section
                    .and_then(|section| sounds.read_ares_reference(section, "SW.ActivationSound"))
                    .flatten();
            }
        }
        for object in &mut self.object_list {
            let section = ini.section(&object.id);
            object.bind_scalar_sound_references(section, sounds);
            object.bind_deploy_voice_references(section, sounds);
            object.bind_order_voice_references(section, sounds);
            object.voice_select = section.map_or_else(Vec::new, |section| {
                sounds.read_rules_sound_list(section, "VoiceSelect")
            });
            object.voice_select_enslaved = section.map_or_else(Vec::new, |section| {
                sounds.read_rules_sound_list(section, "VoiceSelectEnslaved")
            });
            object.voice_select_deactivated = section.map_or_else(Vec::new, |section| {
                sounds.read_rules_sound_list(section, "VoiceSelectDeactivated")
            });
            object.move_sound = section.map_or_else(Vec::new, |section| {
                sounds.read_rules_sound_list(section, "MoveSound")
            });
            object.voice_special_attack = section.map_or_else(Vec::new, |section| {
                sounds.read_rules_sound_list(section, "VoiceSpecialAttack")
            });
            object.voice_feedback = section.map_or_else(Vec::new, |section| {
                sounds.read_rules_sound_list(section, "VoiceFeedback")
            });
            if object.category == crate::rules::object_type::ObjectCategory::Building {
                object.buildup_sound = section
                    .and_then(|section| sounds.read_rules_reference(section, "BuildupSound"));
                // BuildingType46088F/4608DD read WorkingSound (+0xE80) and
                // NotWorkingSound (+0xE84) the same way.
                object.working_sound = section
                    .and_then(|section| sounds.read_rules_reference(section, "WorkingSound"));
                object.not_working_sound = section
                    .and_then(|section| sounds.read_rules_reference(section, "NotWorkingSound"));
            }
            object.sinking_sound =
                section.and_then(|section| sounds.read_rules_reference(section, "SinkingSound"));
            object.voice_sinking =
                section.and_then(|section| sounds.read_rules_reference(section, "VoiceSinking"));
            // TechnoType712FEC reads VoiceFalling (+0x54C) the same way.
            object.voice_falling =
                section.and_then(|section| sounds.read_rules_reference(section, "VoiceFalling"));
            // InfantryType524193 reads VoiceComment (+0xE88) through
            // ReadSoundList525430; no other type class reads the key.
            if object.category == crate::rules::object_type::ObjectCategory::Infantry {
                object.voice_comment = section.map_or_else(Vec::new, |section| {
                    sounds.read_rules_sound_list(section, "VoiceComment")
                });
            }
            // TechnoType7133BA reads TurretRotateSound (+0x560) through
            // ReadString128 and Voc7514D0 for every techno type; the key is not
            // category-gated even though only buildings consume it.
            object.turret_rotate_sound = section
                .and_then(|section| sounds.read_rules_reference(section, "TurretRotateSound"));
            // TechnoType7138D1/71390D read ActivateSound (+0x5A8) and
            // DeactivateSound (+0x5AC) the same way, for every techno type.
            object.activate_sound =
                section.and_then(|section| sounds.read_rules_reference(section, "ActivateSound"));
            object.deactivate_sound =
                section.and_then(|section| sounds.read_rules_reference(section, "DeactivateSound"));
            // ObjectType5F93C1 reads AmbientSound (+0x1F4) the same way, for
            // every object type.
            object.ambient_sound =
                section.and_then(|section| sounds.read_rules_reference(section, "AmbientSound"));
            if object.category == crate::rules::object_type::ObjectCategory::Infantry {
                // Infantry52440B then524447: missing/empty/unregistered names
                // keep the prior signed ID across the existing processed passes.
                object.enter_water_sound = section
                    .and_then(|section| sounds.read_rules_reference(section, "EnterWaterSound"));
                object.leave_water_sound = section
                    .and_then(|section| sounds.read_rules_reference(section, "LeaveWaterSound"));
                // ReadSequenceData (`0x0052403F`) resolves each pair through
                // VocClass::FindByName (`0x007514D0`, first _strcmpi match).
                object.bind_sequence_sounds(|name| sounds.get(name).map(|entry| entry.id.clone()));
            }
        }
    }

    pub(crate) fn bind_terrain_type_sound_references(
        &mut self,
        processed: &ProcessedRulesLayers,
        sounds: &crate::rules::sound_ini::SoundRegistry,
    ) {
        for (name, section) in processed.terrain_type_read_sections() {
            if let Some(terrain) = self
                .terrain_object_types
                .get_mut(&name.to_ascii_uppercase())
            {
                terrain.bind_sound_references(section, sounds);
            }
        }
    }

    /// Resolve Ares refusal cues through the same fixed EVAMD catalog that
    /// owns process playback. The retained INI body supplies each read pass.
    pub fn bind_type_eva_references(
        &mut self,
        ini: &IniFile,
        eva: &crate::rules::sound_ini::EvaRegistry,
    ) {
        #[cfg(not(feature = "ares"))]
        let _ = (ini, eva);
        #[cfg(feature = "ares")]
        for sw in self.super_weapons.values_mut() {
            sw.insufficient_funds_eva = eva.read_rules_reference(
                ini.section(&sw.id),
                "EVA.InsufficientFunds",
                "EVA_InsufficientFunds",
            );
        }
    }

    /// Parse a complete RuleSet from a rules.ini IniFile.
    ///
    /// Loads all type registries, individual object sections, and any
    /// weapons/warheads referenced by those objects. Missing sections
    /// are logged as warnings but don't cause errors — RA2's rules.ini
    /// sometimes references sections that don't exist.
    pub fn from_ini(ini: &IniFile) -> Result<Self, RulesError> {
        Self::from_rules_layers(&RulesLayerStack::new(ini.clone()))
    }

    /// The rules an empty `RULESMD.INI` reads to: every key at its reader
    /// default and no types. Fixtures that exercise no rules data pass it
    /// where the game passes the scenario's rules.
    #[cfg(any(test, feature = "test-support"))]
    pub fn empty_for_test() -> &'static RuleSet {
        static EMPTY: std::sync::OnceLock<RuleSet> = std::sync::OnceLock::new();
        EMPTY.get_or_init(|| {
            RuleSet::from_ini(&IniFile::from_str("")).expect("an empty rules INI reads")
        })
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn from_ini_with_fixed_art_for_test(
        ini: &IniFile,
        art: &IniFile,
    ) -> Result<Self, RulesError> {
        Self::from_processed_rules(&RulesLayerStack::new(ini.clone()).process_with_fixed_art(art)?)
    }

    /// [`Self::from_ini`] with the type sounds bound, as production binds
    /// them, against a SOUNDMD `[SoundList]` of `sound_names`.
    #[cfg(any(test, feature = "test-support"))]
    pub fn from_ini_with_sounds_for_test(
        ini: &IniFile,
        sound_names: &[&str],
    ) -> Result<Self, RulesError> {
        let mut catalog = String::from("[SoundList]\n");
        for (index, name) in sound_names.iter().enumerate() {
            catalog.push_str(&format!("{index}={name}\n"));
        }
        let processed = RulesLayerStack::new(ini.clone()).process()?;
        let mut rules = Self::from_processed_rules(&processed)?;
        let sounds = crate::rules::sound_ini::SoundRegistry::from_ini(&IniFile::from_str(&catalog));
        rules.bind_type_sound_references(processed.ini(), &sounds);
        rules.bind_terrain_type_sound_references(&processed, &sounds);
        Ok(rules)
    }

    fn from_projected_ini(
        processed: &ProcessedRulesLayers,
        country_side_registry: ProjectedCountrySideRegistry,
    ) -> Result<Self, RulesError> {
        let ini = processed.ini();
        let mut object_list: Vec<ObjectType> = Vec::new();
        let mut object_index: LookupMap<String, TypeHandle> = LookupMap::default();
        let mut object_category_index: HashMap<(ObjectCategory, String), TypeHandle> =
            HashMap::new();
        let mut type_array_indices: HashMap<(ObjectCategory, String), i32> = HashMap::new();
        let mut infantry_ids: Vec<String> = Vec::new();
        let mut vehicle_ids: Vec<String> = Vec::new();
        let mut aircraft_ids: Vec<String> = Vec::new();
        let mut building_ids: Vec<String> = Vec::new();
        let production: ProductionRules = ProductionRules::from_ini(ini);
        let mut general: GeneralRules = GeneralRules::from_ini(ini);
        general.tree_strength = processed.tree_strength();
        let (_, color_indexes) = processed.color_add_state();
        general.laser_target_color = color_indexes.laser_target_color();
        general.iron_curtain_color = color_indexes.iron_curtain_color();
        general.berserk_color = color_indexes.berserk_color();
        general.force_shield_color = color_indexes.force_shield_color();
        let ai_base_spacing = ini.section_or_empty("AI").read_int("AIBaseSpacing", 1);
        // `DifficultyClass::ReadINI_IntVector @ 0x00475D70`, reached by
        // `RulesClass::ReadGeneral` for AISlaveMinerNumber
        // `0x00670585..0x006705B7`, AIExtraRefineries `0x006705F9..0x0067062A`
        // and the three BaseDefenseCounts vectors `0x00670013..0x006700BE`.
        let parse_difficulty_vector = |key: &str| {
            ini.section_or_empty("General")
                .read_int_list(key)
                .unwrap_or_default()
        };
        let ai_slave_miner_number = parse_difficulty_vector("AISlaveMinerNumber");
        let ai_extra_refineries = parse_difficulty_vector("AIExtraRefineries");
        let allied_base_defense_counts = parse_difficulty_vector("AlliedBaseDefenseCounts");
        let soviet_base_defense_counts = parse_difficulty_vector("SovietBaseDefenseCounts");
        let third_base_defense_counts = parse_difficulty_vector("ThirdBaseDefenseCounts");
        // gamemd-derived: `RulesClass::Constructor @ 0x00666922` stores 20 at
        // +0xE0C; `RulesClass::ReadGeneral @ 0x006701D9..0x006701FE` reads the
        // signed `AINavalYardAdjacency=` override.
        let ai_naval_yard_adjacency = ini
            .section_or_empty("General")
            .read_int("AINavalYardAdjacency", 20);
        let initial_veteran = ini
            .section_or_empty("SpecialFlags")
            .read_bool("InitialVeteran", false);
        let terrain_rules: TerrainRules = TerrainRules::from_ini(ini);
        let tiberium_types = TiberiumTypeRegistry::from_processed_rules(processed);
        let bridge_rules: BridgeRules = BridgeRules::from_ini(ini);
        let crate_rules = CrateRules::default();
        // Overwritten from the processed layers by the two callers above; the
        // projection path has no `[Powerups]` accumulator of its own.
        let powerups = crate::rules::powerups::PowerupTable::default();
        let garrison_rules: GarrisonRules = GarrisonRules::from_ini(ini);
        // `RulesClass::ReadWallModel` 0x0066D1F0 (body 0x0066D1F0-0x0066D262)
        // leaves the constructor value (0, from 0x00667825) in place when
        // `[WallModel]` is absent: 0x0066D20E passes the current
        // `[ESI+0x1850]` as the default and 0x0066D22E stores the answer back.
        let allied_wall_transparency: bool = ini
            .section_or_empty("WallModel")
            .read_bool("AlliedWallTransparency", false);
        let elevation_model = ElevationModel::from_ini(ini);
        let radiation: RadiationRules = RadiationRules::from_ini(ini);
        #[cfg(feature = "phobos")]
        let (shield_types, shield_globals) = {
            let shield_types = processed.shield_types().cloned().unwrap_or_default();
            log::info!(
                "Phobos shield types: [{}]",
                shield_types.names().collect::<Vec<_>>().join(", ")
            );
            (
                shield_types,
                crate::rules::phobos_shields::ShieldGlobals::from_ini(ini),
            )
        };
        #[cfg(feature = "phobos")]
        let (digital_display_types, digital_display_globals) = {
            let digital_display_types = processed
                .digital_display_types()
                .cloned()
                .unwrap_or_default();
            log::info!(
                "Phobos digital display types: [{}]",
                digital_display_types.names().collect::<Vec<_>>().join(", ")
            );
            (
                digital_display_types,
                crate::rules::digital_display::DigitalDisplayGlobals::from_ini(ini),
            )
        };
        #[cfg(feature = "phobos")]
        let (attach_effect_types, attach_effect_globals) = {
            let attach_effect_types = processed.attach_effect_types().cloned().unwrap_or_default();
            log::info!(
                "Phobos attach effect types: [{}]",
                attach_effect_types.names().collect::<Vec<_>>().join(", ")
            );
            (
                attach_effect_types,
                crate::rules::attach_effect::AttachEffectGlobals::from_ini(ini),
            )
        };
        #[cfg(feature = "phobos")]
        let placement_preview_globals =
            crate::rules::phobos_placement_preview::PlacementPreviewGlobals::from_ini(ini);
        #[cfg(feature = "phobos")]
        let skirmish_color_mode = crate::rules::skirmish_colors::SkirmishColorMode::from_ini(ini);
        #[cfg(feature = "phobos")]
        let radiation = {
            let mut radiation = radiation;
            radiation.radiation_types = processed.radiation_types().cloned().unwrap_or_default();
            // The catalog is the only rules state a mod names by list, so
            // report it once per load: a missing entry then shows up here
            // instead of as an unchanged radiation field.
            log::info!(
                "Phobos radiation types: [{}]",
                radiation
                    .radiation_types
                    .names()
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            radiation
        };
        let radar_event_config: RadarEventConfig = RadarEventConfig::from_ini(ini);
        let countries = country_side_registry.rules;
        #[cfg(feature = "phobos")]
        let side_gdi_positions: Vec<bool> = country_side_registry
            .side_ids
            .iter()
            .enumerate()
            .map(|(index, name)| {
                ini.section(name).map_or(index == 0, |section| {
                    section.read_bool("Sidebar.GDIPositions", index == 0)
                })
            })
            .collect();
        #[cfg(feature = "phobos")]
        let sw_sidebar = crate::rules::phobos_sw_sidebar::SuperWeaponSidebarGlobals::from_ini(ini);
        #[cfg(feature = "phobos")]
        let selection_flash =
            crate::rules::phobos_selection_flash::SelectionFlashGlobals::from_ini(ini);
        #[cfg(feature = "phobos")]
        let fly_no_wobbles = ini
            .section("AudioVisual")
            .and_then(|section| section.read_phobos_nullable_bool("FlyNoWobbles", None));
        #[cfg(feature = "phobos")]
        let power_plant_enhancer =
            crate::rules::phobos_power_plant_enhancer::PowerPlantEnhancerGlobals::from_ini(ini);
        #[cfg(feature = "phobos")]
        let phobos_airburst = crate::rules::phobos_airburst::PhobosAirburstGlobals::from_ini(ini);
        #[cfg(feature = "phobos")]
        let side_sw_sidebar_art: Vec<
            crate::rules::phobos_sw_sidebar::SuperWeaponSidebarSideArt,
        > = country_side_registry
            .side_ids
            .iter()
            .map(|name| {
                ini.section(name)
                    .map(crate::rules::phobos_sw_sidebar::SuperWeaponSidebarSideArt::from_section)
                    .unwrap_or_default()
            })
            .collect();
        let color_schemes = crate::rules::color_scheme::parse_color_schemes(ini);
        let house_color_ramps =
            crate::rules::house_colors::HouseColorRamps::from_schemes(&color_schemes);
        let color_add = processed.color_add_state().0.clone();

        // Step 1: Parse each type registry and load object sections.
        for &(registry_name, category) in TYPE_REGISTRIES {
            let ids: Vec<String> = parse_registry(ini, registry_name);
            log::info!("Registry [{}]: {} entries", registry_name, ids.len());

            for (registry_index, id) in ids.iter().enumerate() {
                type_array_indices
                    .entry((category, id.to_ascii_uppercase()))
                    .or_insert(registry_index as i32);
                if let Some(section) = ini.section(id) {
                    let mut obj: ObjectType = ObjectType::from_ini_section(id, section, category);
                    if category == ObjectCategory::Building {
                        obj.base_plan_type_index = type_array_indices
                            .get(&(category, id.to_ascii_uppercase()))
                            .copied()
                            .expect("BuildingType index was registered above");
                    }
                    if obj.base_reservation_writer_eligible() {
                        obj.base_reservation_spacing = Some(ai_base_spacing);
                    }
                    let key = id.to_ascii_uppercase();
                    let category_key = (category, key.clone());
                    // Find-or-allocate is per native type registry. A duplicate
                    // within one registry reuses its slot; the same ID in a
                    // different registry remains a distinct native type.
                    let handle = match object_category_index.get(&category_key).copied() {
                        Some(TypeHandle(idx)) => {
                            log::warn!(
                                "Object '{}' merges onto an existing case-duplicate {:?} type",
                                id,
                                category,
                            );
                            object_list[idx as usize] = obj;
                            TypeHandle(idx)
                        }
                        None => {
                            let handle = TypeHandle(object_list.len() as u32);
                            object_list.push(obj);
                            object_category_index.insert(category_key, handle);
                            handle
                        }
                    };
                    if object_index.get(&key).is_some_and(|prior| *prior != handle) {
                        log::warn!(
                            "Object '{}' is registered in multiple native type categories",
                            id
                        );
                    }
                    // Preserve the pre-existing broad lookup's later-registry
                    // winner for callers that do not own a category order.
                    object_index.insert(key, handle);
                } else {
                    log::trace!(
                        "Object '{}' listed in [{}] but has no section",
                        id,
                        registry_name
                    );
                }
            }

            // Store ID lists per category.
            match category {
                ObjectCategory::Infantry => infantry_ids = ids,
                ObjectCategory::Vehicle => vehicle_ids = ids,
                ObjectCategory::Aircraft => aircraft_ids = ids,
                ObjectCategory::Building => building_ids = ids,
            }
        }

        // Step 2: WeaponTypes. The ordered Rules processor allocated every
        // weapon a reached reader names (TechnoType slots, DeathWeapon,
        // OccupyWeapon, SuperWeapon WeaponType, [CombatDamage] DeathWeapon,
        // Projectile AirburstWeapon/ShrapnelWeapon, ...) and keeps each one
        // even when nothing current references it: Action42 indexes this
        // family directly (6E2390). Its retained Damage, Speed, Range,
        // Projectile, Warhead, Anim, OccupantAnim, OpenToppedAnim and
        // AttachedParticleSystem are retained here; AssaultAnim is retained by
        // this pass too, and Report lists bind below through the SOUNDMD catalog.
        let mut weapons: LookupMap<String, WeaponType> = LookupMap::default();
        for state in processed.weapon_states() {
            let Some(section) = ini.section(state.id) else {
                log::trace!("Weapon '{}' allocated but has no section", state.id);
                continue;
            };
            let mut weapon = WeaponType::from_ini_section(&section.name, section);
            weapon.speed = state.speed;
            weapon.range_leptons = state.range_leptons;
            weapon.projectile = state.projectile.map(str::to_owned);
            weapon.damage = state.damage;
            weapon.warhead = state.warhead.map(str::to_owned);
            weapon.anim = state.anim.to_vec();
            weapon.assault_anim = state.assault_anim.map(str::to_owned);
            weapon.occupant_anim = state.occupant_anim.map(str::to_owned);
            weapon.open_topped_anim = state.open_topped_anim.map(str::to_owned);
            weapon.attached_particle_system = state.attached_particle_system.map(str::to_owned);
            weapons.insert(section.name.clone(), weapon);
        }
        // Step 3: Parse warhead sections. The projected [Warheads] registry is
        // the processor's whole Warhead family: the [Warheads] values and
        // every reference-driven allocation, weapons' warheads and the
        // [Radiation] RadSiteWarhead included.
        let warhead_ids: HashSet<String> = parse_registry(ini, "Warheads").into_iter().collect();
        let mut warheads: LookupMap<String, WarheadType> = LookupMap::default();
        for warhead_id in &warhead_ids {
            if let Some(section) = ini.section(warhead_id) {
                // Find-or-allocate by the section's canonical header name, so
                // two references differing only in case resolve to one type
                // (the original engine matches case-insensitively).
                let canonical = section.name.clone();
                warheads
                    .entry(canonical.clone())
                    .or_insert_with(|| WarheadType::from_ini_section(&canonical, section));
            } else {
                log::trace!("Warhead '{}' referenced but has no section", warhead_id);
            }
        }

        // Step 4: Collect projectile IDs referenced by weapons and parse them.
        let mut projectiles: LookupMap<String, ProjectileType> = LookupMap::default();
        let mut projectile_ids: HashSet<String> = HashSet::new();
        for weapon in weapons.values() {
            if let Some(ref proj_id) = weapon.projectile {
                projectile_ids.insert(proj_id.clone());
            }
        }
        for proj_id in &projectile_ids {
            if let Some(section) = ini.section(proj_id) {
                // Find-or-allocate by canonical section name (see warheads above).
                // Stock rulesmd references [InvisibleLow] as both "InvisibleLow"
                // and "Invisiblelow"; this collapses them to one entry, as the
                // original engine does.
                let canonical = section.name.clone();
                projectiles
                    .entry(canonical.clone())
                    .or_insert_with(|| ProjectileType::from_ini_section(&canonical, section, None));
            } else {
                log::trace!("Projectile '{}' referenced but has no section", proj_id);
            }
        }

        // Step 5: Build factory lookup map from Factory= keys on all objects.
        let factory_map: HashMap<String, FactoryType> = object_list
            .iter()
            .filter_map(|obj| {
                obj.factory_type()
                    .map(|ft| (obj.id.to_ascii_uppercase(), ft))
            })
            .collect();
        log::info!("Factory map: {} entries", factory_map.len());

        // Step 6: Parse superweapon type registry.
        let mut super_weapons: HashMap<String, SuperWeaponType> = HashMap::new();
        let sw_ids: Vec<String> = parse_registry(ini, "SuperWeaponTypes");
        let super_weapon_order: Vec<String> = sw_ids.clone();
        for sw_id in &sw_ids {
            if let Some(section) = ini.section(sw_id) {
                let sw = SuperWeaponType::from_ini_section(sw_id, section);
                super_weapons.insert(sw_id.clone(), sw);
            } else {
                log::trace!(
                    "SuperWeapon '{}' listed in [SuperWeaponTypes] but has no section",
                    sw_id
                );
            }
        }
        log::info!("SuperWeaponTypes: {} loaded", super_weapons.len());

        // Parse [CombatDamage] defaults (particle-system fallbacks).
        let combat_damage: CombatDamageDefaults = ini
            .section("CombatDamage")
            .map(CombatDamageDefaults::from_ini_section)
            .unwrap_or_default();

        // [CombatDamage] IonCannonWarhead, C4Warhead and CrushWarhead: the
        // ordered Process owner's, published by from_processed_rules.
        let bridge_warheads = crate::rules::bridge_warheads::BridgeWarheads::default();

        let mind_control = crate::rules::mind_control_rules::MindControlRules::from_ini(
            ini,
            processed.mind_control_attack_line_frames(),
        );

        // [General] rocket blocks + [CombatDamage] missile warheads, retained
        // pass by pass by the ordered Process owner.
        let missile_spawn = processed.missile_spawn().clone();

        let c4_delay = ini
            .section_or_empty("CombatDamage")
            .read_double("C4Delay", 0.03);

        // Per-mission behaviour table from the [<MissionName>] sections.
        let mission_control = MissionControl::from_ini(ini);

        // Parse [TerrainTypes] registry → per-type sections (TIBTRE01, TREE01, etc.).
        let terrain_object_types: HashMap<String, TerrainObjectType> = processed
            .terrain_type_projections()
            .map(|terrain| (terrain.name.to_ascii_uppercase(), terrain))
            .collect();
        log::info!(
            "TerrainTypes: {} loaded ({} with SpawnsTiberium=yes)",
            terrain_object_types.len(),
            terrain_object_types
                .values()
                .filter(|t| t.spawns_tiberium)
                .count(),
        );

        // Step 7: Two-pass parse for [Particles] and [ParticleSystems].
        // NextParticle is resolved in pass 2 so that INI ordering does not
        // matter; HoldsWhat is the ordered Rules processor's retained binding.
        let (particle_types, particle_types_by_name) = parse_particle_types(ini);
        let (particle_system_types, particle_system_types_by_name) =
            parse_particle_system_types(processed, &particle_types_by_name);
        let (voxel_anim_types, voxel_anim_types_by_name) = parse_voxel_anim_types(ini);

        log::info!(
            "RuleSet loaded: {} objects ({} inf, {} veh, {} air, {} bld), \
             {} weapons, {} warheads, {} projectiles, \
             {} particle types, {} particle system types",
            object_list.len(),
            infantry_ids.len(),
            vehicle_ids.len(),
            aircraft_ids.len(),
            building_ids.len(),
            weapons.len(),
            warheads.len(),
            projectiles.len(),
            particle_types.len(),
            particle_system_types.len()
        );

        // Lockstep invariant: `lookup_ci`'s case-insensitive scan is deterministic
        // only if no two stored type names are equal ignoring case. The original
        // engine's case-insensitive find-or-allocate merges case-duplicate names,
        // guaranteeing this for valid data; assert it in debug so a malformed INI
        // surfaces loudly instead of desyncing silently in lockstep. Compiled out
        // of release, so normal play pays nothing.
        #[cfg(debug_assertions)]
        {
            let check_unique_ci = |label: &str, keys: Vec<&String>| {
                let mut lowered: Vec<String> =
                    keys.iter().map(|k| k.to_ascii_lowercase()).collect();
                lowered.sort();
                for pair in lowered.windows(2) {
                    debug_assert_ne!(
                        pair[0], pair[1],
                        "{label}: type names collide ignoring case ({:?}) — breaks deterministic lookup",
                        pair[0]
                    );
                }
            };
            // Objects merge case-duplicates at insert (find-or-allocate), so the
            // uppercase-keyed index can't hold a case-collision by construction.
            check_unique_ci("weapons", weapons.keys().collect());
            check_unique_ci("warheads", warheads.keys().collect());
            check_unique_ci("projectiles", projectiles.keys().collect());
            check_unique_ci("super_weapons", super_weapons.keys().collect());
        }

        let mut rules = RuleSet {
            object_list,
            object_index,
            serial: next_rule_set_serial(),
            object_category_index,
            native_techno_type_order: Vec::new(),
            type_array_indices,
            weapons,
            weapon_array: Vec::new(),
            warheads,
            projectiles,
            countries,
            country_ids: country_side_registry.country_ids,
            country_indices: country_side_registry.country_indices,
            side_ids: country_side_registry.side_ids,
            #[cfg(feature = "phobos")]
            side_gdi_positions,
            #[cfg(feature = "phobos")]
            sw_sidebar,
            #[cfg(feature = "phobos")]
            selection_flash,
            #[cfg(feature = "phobos")]
            fly_no_wobbles,
            #[cfg(feature = "phobos")]
            power_plant_enhancer,
            #[cfg(feature = "phobos")]
            phobos_airburst,
            #[cfg(feature = "phobos")]
            side_sw_sidebar_art,
            #[cfg(feature = "ares")]
            side_extensions: country_side_registry.side_extensions,
            side_indices: country_side_registry.side_indices,
            country_sides: country_side_registry.country_sides,
            color_schemes,
            house_color_ramps,
            color_add,
            production,
            general,
            reveal_trigger_radius: ini
                .section_or_empty("General")
                .read_int("RevealTriggerRadius", 5),
            ai_base_spacing,
            // The ordered Rules processor publishes these lists
            // (`rules_type_references`).
            shipyard_types: Vec::new(),
            repair_bay_types: Vec::new(),
            build_const_types: Vec::new(),
            build_power_types: Vec::new(),
            build_refinery_types: Vec::new(),
            build_barracks_types: Vec::new(),
            build_tech_types: Vec::new(),
            build_weapons_types: Vec::new(),
            build_radar_types: Vec::new(),
            allied_base_defense_types: Vec::new(),
            soviet_base_defense_types: Vec::new(),
            third_base_defense_types: Vec::new(),
            concrete_wall_types: Vec::new(),
            ns_gate_types: Vec::new(),
            ew_gate_types: Vec::new(),
            harvester_unit_types: Vec::new(),
            ai_slave_miner_number,
            ai_extra_refineries,
            allied_base_defense_counts,
            soviet_base_defense_counts,
            third_base_defense_counts,
            ai_naval_yard_adjacency,
            initial_veteran,
            infantry_ids,
            vehicle_ids,
            aircraft_ids,
            building_ids,
            factory_map,
            prerequisite_lists: Default::default(),
            ares_prerequisites: None,
            ares_armors: processed.ares_armors().cloned(),
            emp_sparkles: None,
            #[cfg(feature = "ares")]
            ares_global_controls: processed.ares_global_controls().clone(),
            #[cfg(feature = "ares")]
            ares_super_weapon_metadata: BTreeMap::new(),
            #[cfg(feature = "phobos")]
            phobos_super_weapon_metadata: BTreeMap::new(),
            #[cfg(feature = "phobos")]
            parallel_ai_queues:
                crate::rules::phobos_parallel_ai_queues::ParallelAiQueueRules::from_ini(ini),
            ares_prerequisite_alternates: HashSet::new(),
            terrain_rules,
            tiberium_types,
            terrain_object_types,
            overlay_registry:
                crate::rules::overlay_types::OverlayTypeRegistry::from_processed_rules(processed),
            bridge_rules,
            crate_rules,
            powerups,
            garrison_rules,
            allied_wall_transparency,
            elevation_model,
            radiation,
            #[cfg(feature = "phobos")]
            shield_types,
            #[cfg(feature = "phobos")]
            shield_globals,
            #[cfg(feature = "phobos")]
            crit_globals: crate::rules::phobos_crit::CritGlobals::from_ini(ini),
            #[cfg(feature = "phobos")]
            phobos_warhead_anim:
                crate::rules::phobos_warhead_anim::PhobosWarheadAnimGlobals::from_ini(ini),
            #[cfg(feature = "phobos")]
            digital_display_types,
            #[cfg(feature = "phobos")]
            digital_display_globals,
            #[cfg(feature = "phobos")]
            attach_effect_types,
            #[cfg(feature = "phobos")]
            attach_effect_globals,
            #[cfg(feature = "phobos")]
            placement_preview_globals,
            #[cfg(feature = "phobos")]
            skirmish_color_mode,
            radar_event_config,
            super_weapons,
            super_weapon_order,
            combat_damage,
            bridge_warheads,
            mind_control,
            missile_spawn,
            c4_delay,
            particle_types,
            particle_system_types,
            particle_system_types_by_name,
            voxel_anim_types,
            voxel_anim_types_by_name,
            smudge_types: SmudgeTypeRegistry::from_rules_ini(ini),
            art_registry: crate::rules::art_data::ArtRegistry::empty(),
            anim_type_art_read_states: Vec::new(),
            anim_type_images: Vec::new(),
            anim_type_references: Vec::new(),
            anim_type_names: ini
                .section("Animations")
                .into_iter()
                .flat_map(|section| section.registry_ids())
                .map(|name| name.to_ascii_uppercase())
                .filter(|name| !name.is_empty())
                .collect(),
            effect_assets: crate::rules::effect_asset_catalog::EffectAssetCatalog::default(),
            voxel_assets: crate::rules::voxel_asset_catalog::VoxelAssetCatalog::default(),
            buildup_assets: crate::rules::buildup_asset_catalog::BuildupAssetCatalog::default(),
            rubble_assets: crate::rules::building_rubble::RubbleAssetCatalog::default(),
            terrain_assets: crate::rules::terrain_asset_catalog::TerrainAssetCatalog::default(),
            animation_sequences: BTreeMap::new(),
            animation_sequence_by_handle: Vec::new(),
            unit_shp_read_states: BTreeMap::new(),
            mission_control,
            // The boundary-sensitive RulesLayerStack hash.
            source_ini_hash: processed.content_hash(),
        };
        rules.rebuild_animation_sequences(None);
        Ok(rules)
    }

    /// Look up a game object by ID.
    /// Case-insensitive type-name lookup matching the original engine's
    /// find-or-allocate (stricmp-style) name resolution.
    ///
    /// Exact match first — O(1) and the normal path, since RA2 type IDs are
    /// consistently cased. Only on a case-mismatch miss does it scan for the
    /// unique case-insensitive match. Valid RA2 data never holds two names
    /// equal-ignoring-case (the original engine's case-insensitive find merges
    /// them), so the scan yields at most one hit and the result stays
    /// deterministic for lockstep.
    fn lookup_ci<'a, T, S: std::hash::BuildHasher>(
        map: &'a HashMap<String, T, S>,
        id: &str,
    ) -> Option<&'a T> {
        map.get(id).or_else(|| {
            map.iter()
                .find_map(|(key, value)| key.eq_ignore_ascii_case(id).then_some(value))
        })
    }

    /// The C4 charge's fuse, `ftol(C4Delay * 900.0)`, which the plant starts
    /// the building's C4Timer with (`InfantryClass::Per_Cell_Process
    /// 0x0051A5E2..0x0051A608`). Under the process's chop control word retail
    /// `.03`, scanned through `float`, gives 26.
    pub fn c4_delay_frames(&self) -> i32 {
        native_minutes_to_frames(self.c4_delay)
    }

    /// The building's flash at a C4 plant, `ftol(C4Delay * 900.0 * 0.5)`
    /// (`0x0051A5B6..0x0051A5D0`): the 900.0 product stays on the x87 stack
    /// for the halving. Retail gives 13.
    pub fn c4_flash_frames(&self) -> i32 {
        use crate::util::native_x87::{MaskedX87Chop53 as X87, NativeF64Bits};
        X87::ftol_i32_low_masked(X87::mul(
            X87::mul(
                X87::load_f64(NativeF64Bits::from_bits(self.c4_delay.to_bits())),
                X87::load_i32(900),
            ),
            X87::load_f64(NativeF64Bits::HALF),
        ))
    }

    /// Resolve a type name to its handle, case-insensitively (engine parity).
    pub fn type_handle(&self, id: &str) -> Option<TypeHandle> {
        // The keys are uppercase, so a name without a lowercase letter is its
        // own key and needs no uppercase copy.
        if id.bytes().any(|byte| byte.is_ascii_lowercase()) {
            self.object_index.get(&id.to_ascii_uppercase()).copied()
        } else {
            self.object_index.get(id).copied()
        }
    }

    /// TAction106's active6DF5D1 and FlashCameo115's6E4150 compare IDs with exact case,
    /// scanning TechnoType::Array A8EB04/countA8EB10 backwards. This has
    /// different semantics from broad `object()` and subtype lookup.
    ///
    /// The old ObjectType projection merges repeated IDs in one category.
    /// Native can construct those from >24-byte factory inputs. If the last
    /// native match is one of these unrepresentable identities, return None;
    /// never redirect the mutation to an earlier/global broad-lookup winner.
    pub(crate) fn native_techno_type_exact(&self, id: &str) -> Option<&ObjectType> {
        self.native_techno_type_order
            .iter()
            .rev()
            .find(|(_, stored_id, _)| stored_id == id)
            .and_then(|(_, _, handle)| *handle)
            .map(|handle| self.object_by_handle(handle))
    }

    /// This set's identity: nonzero and distinct from every other `RuleSet`
    /// built in the process. It reaches no result; caches of handles compare
    /// it to know whose handles they hold.
    pub fn serial(&self) -> u64 {
        self.serial
    }

    /// This set as a distinct set: a test's copy of a shared retail load.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn with_fresh_serial(mut self) -> Self {
        self.serial = next_rule_set_serial();
        self
    }

    /// Dereference a handle to its object. Handles only originate from this
    /// `RuleSet`, so the index is always in bounds.
    #[inline]
    pub fn object_by_handle(&self, handle: TypeHandle) -> &ObjectType {
        &self.object_list[handle.0 as usize]
    }

    /// Look up a game object by ID (case-insensitive, engine parity).
    pub fn object(&self, id: &str) -> Option<&ObjectType> {
        self.type_handle(id).map(|h| self.object_by_handle(h))
    }

    /// Cameo10034E75/10034F3F and SPECIAL_PLACE10038EB9 use the same
    /// retained flag. Expose the fact without exporting mutable SWExt state.
    pub fn super_weapon_uses_ai_targeting(&self, id: &str) -> bool {
        #[cfg(feature = "ares")]
        {
            self.super_weapon_target_policy(id)
                .is_some_and(|policy| policy.use_ai_targeting())
        }
        #[cfg(not(feature = "ares"))]
        {
            let _ = id;
            false
        }
    }

    /// The ordered SWTypeExt targeting fields of an Ares GenericWarhead or
    /// UnitDelivery type. Stock types keep their retail dispatch owners.
    #[cfg(feature = "ares")]
    pub(crate) fn super_weapon_target_policy(
        &self,
        id: &str,
    ) -> Option<crate::rules::superweapon_targeting::AresSuperWeaponTargetingState> {
        if !self.super_weapon(id)?.kind.has_ares_handler() {
            return None;
        }
        self.super_weapon_metadata(id)
            .map(|metadata| metadata.targeting())
    }

    /// `Type == Rules+0x498` (`[General] PrismType=`, `0x0044B2F8`).
    pub(crate) fn is_prism_type(&self, object: &ObjectType) -> bool {
        self.general
            .prism_type
            .as_deref()
            .is_some_and(|prism_type| object.id.eq_ignore_ascii_case(prism_type))
    }

    /// Resolve one name within a specific native TechnoType registry.
    /// Category plus case-insensitive ID is the stable analogue of the
    /// category-distinct pointer retained by native TaskForce entries.
    pub(crate) fn object_in_category(
        &self,
        category: ObjectCategory,
        id: &str,
    ) -> Option<&ObjectType> {
        self.object_category_index
            .get(&(category, id.to_ascii_uppercase()))
            .map(|handle| self.object_by_handle(*handle))
    }

    /// The computer's base defense candidates of a House's side
    /// (`HouseTypeClass+0xBC`): Allied for side 0, Soviet for 1, Third for any
    /// other (`0x00507BCA..0x00507BFA`).
    pub(crate) fn base_defense_types(&self, side_index: i32) -> &[String] {
        #[cfg(feature = "ares")]
        if let Some(types) = self.side_base_defenses(side_index) {
            return types;
        }
        match side_index {
            0 => &self.allied_base_defense_types,
            1 => &self.soviet_base_defense_types,
            _ => &self.third_base_defense_types,
        }
    }

    /// The BuildingType at native array index `index` (`BuildingTypes[index]`,
    /// the index `ObjectType::base_plan_type_index` and BasePlan nodes hold).
    /// `None` for a negative index, one past the array, or a registered name
    /// without a section.
    pub(crate) fn building_type_at(&self, index: i32) -> Option<&ObjectType> {
        self.type_array_at(ObjectCategory::Building, index)
    }

    /// Resolve one scenario BasePlan token through the native BuildingType
    /// registry only, matching
    /// `BuildingTypeClass__FindIndexByName @ 0x0045E7B0` used by
    /// `FUN_0042EBE0`, and return its ordered index.
    pub(crate) fn building_type_index(&self, id: &str) -> Option<i32> {
        self.type_array_index(ObjectCategory::Building, id)
    }

    /// The ids of one native TechnoType array (`InfantryTypeClass::Array`
    /// `0xA8E34C`, `UnitTypeClass::Array` `0xA83CE4`, `AircraftTypeClass::
    /// Array` `0xA8B21C`, `BuildingTypeClass::Array` `0xA83C6C`), in index
    /// order: the registry's first spelling of each identity.
    pub(crate) fn type_array_ids(&self, category: ObjectCategory) -> &[String] {
        match category {
            ObjectCategory::Infantry => &self.infantry_ids,
            ObjectCategory::Vehicle => &self.vehicle_ids,
            ObjectCategory::Aircraft => &self.aircraft_ids,
            ObjectCategory::Building => &self.building_ids,
        }
    }

    /// The type at native array index `index` of `category`'s array; `None`
    /// for a negative index, one past the array, or a registered name
    /// without a section.
    pub(crate) fn type_array_at(
        &self,
        category: ObjectCategory,
        index: i32,
    ) -> Option<&ObjectType> {
        let id = self
            .type_array_ids(category)
            .get(usize::try_from(index).ok()?)?;
        self.object_in_category(category, id)
    }

    /// The native array index (`+0xDF8`) of `id` in `category`'s array.
    pub fn type_array_index(&self, category: ObjectCategory, id: &str) -> Option<i32> {
        self.type_array_indices
            .get(&(category, id.to_ascii_uppercase()))
            .copied()
    }

    /// Installed Ares Building Find10007E50, shared with BuiltAt's current-pass
    /// vector reader. Runtime upgrade queries use the current registered IDs,
    /// rejecting none/<none> before the first case-insensitive stored-ID hit.
    pub fn ares_find_building(&self, token: &str) -> Option<i32> {
        crate::rules::native_processing::ares_type_index_of_name(
            token,
            self.type_array_ids(ObjectCategory::Building)
                .iter()
                .map(String::as_str),
        )
        .map(|index| i32::try_from(index).expect("BuildingTypes exceeds native i32 index space"))
    }

    /// Resolve a TaskForce member through the exact native family order used
    /// by `gamemd.exe 0x004C4EF0`: Infantry, Unit, then Aircraft. BuildingType
    /// is never searched.
    pub(crate) fn task_force_member_object(&self, id: &str) -> Option<&ObjectType> {
        let key = id.to_ascii_uppercase();
        [
            ObjectCategory::Infantry,
            ObjectCategory::Vehicle,
            ObjectCategory::Aircraft,
        ]
        .into_iter()
        .find_map(|category| self.object_in_category(category, &key))
    }

    /// Resolve AITrigger token 6 through the exact native family order used
    /// by `gamemd.exe 0x0041F77D..0x0041F7E4`: Infantry, Unit, Aircraft,
    /// then Building.
    pub(crate) fn ai_trigger_object(&self, id: &str) -> Option<&ObjectType> {
        let key = id.to_ascii_uppercase();
        [
            ObjectCategory::Infantry,
            ObjectCategory::Vehicle,
            ObjectCategory::Aircraft,
            ObjectCategory::Building,
        ]
        .into_iter()
        .find_map(|category| self.object_in_category(category, &key))
    }

    /// First registered BuildingType whose merged ART `ToOverlay=` resolves to
    /// the requested overlay. Native stops on this first match even when that
    /// type is later rejected as `Unsellable=`.
    pub fn first_building_type_for_overlay(
        &self,
        overlay_id: u8,
        overlays: &crate::rules::overlay_types::OverlayTypeRegistry,
    ) -> Option<&ObjectType> {
        self.object_list.iter().find(|object| {
            object.category == crate::rules::object_type::ObjectCategory::Building
                && object
                    .to_overlay
                    .as_deref()
                    .and_then(|name| overlays.id_for_name(name))
                    == Some(overlay_id)
        })
    }

    /// TechnoType virtual `+0x84`, the cost a House pays for `object`.
    ///
    /// `TechnoTypeClass::Cost_Of @ 0x00711F00` (Infantry, Unit and Aircraft
    /// types) returns the type's virtual `+0xAC` cost ([`Self::type_cost`])
    /// for a null House; otherwise it stores the country factor
    /// (`0x0050BDF0`) and the FactoryPlant factor (`0x0050BEB0`) as floats
    /// and returns `ftol(cost * plant * country)` (`0x00711F35..0x00711F41`).
    /// The BuildingType override `0x0045EDD0` adjusts its actual cost
    /// (virtual `+0xAC` = `0x0045ED50`) the same way, adds the halved sum of
    /// both PadAircraft costs when it bundles them, and adds its FreeUnit's
    /// cost, clamping only that arm at zero (`0x0045EE47..0x0045EE50`).
    /// Native comparison: `tools/spatial_oracle/cost_of`.
    pub fn cost_of(&self, object: &ObjectType, house: Option<&HouseCostFactors>) -> i32 {
        let cost = self.adjusted_cost(object, house);
        if object.category != ObjectCategory::Building {
            return cost;
        }
        let mut cost = cost;
        if let Some((first, second)) = self.bundled_pad_aircraft(object) {
            let pads = self
                .adjusted_cost(second, house)
                .wrapping_add(self.adjusted_cost(first, house));
            cost = cost.wrapping_add(pads / 2);
        }
        match object
            .free_unit
            .as_deref()
            .and_then(|name| self.object(name))
        {
            Some(free) => cost.wrapping_add(self.adjusted_cost(free, house)).max(0),
            None => cost,
        }
    }

    /// `0x00711F00` over the type's virtual `+0xAC` cost ([`Self::type_cost`]).
    fn adjusted_cost(&self, object: &ObjectType, house: Option<&HouseCostFactors>) -> i32 {
        let cost = self.type_cost(object);
        house.map_or(cost, |house| house.adjust(cost, object.factor_slot()))
    }

    /// The two PadAircraft a BuildingType bundles into its price: with
    /// `SeparateAircraft=no`, when it is the first `Dock=` of the first
    /// PadAircraft (`0x0045ED63..0x0045ED7D`).
    fn bundled_pad_aircraft(&self, object: &ObjectType) -> Option<(&ObjectType, &ObjectType)> {
        if self.general.separate_aircraft {
            return None;
        }
        let [first_id, second_id, ..] = self.general.pad_aircraft_types.as_slice() else {
            return None;
        };
        let (first, second) = (self.object(first_id)?, self.object(second_id)?);
        first
            .dock
            .first()
            .is_some_and(|dock| dock.eq_ignore_ascii_case(&object.id))
            .then_some((first, second))
    }

    /// TechnoType virtual `+0xAC` (GetCost): `Cost=` (`0x00711EB0`), or a
    /// BuildingType's [`Self::building_actual_cost`] (`0x0045ED50`).
    pub fn type_cost(&self, object: &ObjectType) -> i32 {
        if object.category == ObjectCategory::Building {
            self.building_actual_cost(object)
        } else {
            object.cost
        }
    }

    /// BuildingType virtual `+0xAC` value. Wall sale invokes and discards it;
    /// receiver anger uses the same authority.
    pub(crate) fn building_actual_cost(&self, object: &ObjectType) -> i32 {
        let mut value = object.cost;
        if let Some((first, second)) = self.bundled_pad_aircraft(object) {
            value = value.wrapping_sub(first.cost.wrapping_add(second.cost) / 2);
        }
        if let Some(free_unit) = object.free_unit.as_deref() {
            value = value
                .wrapping_sub(self.object(free_unit).map_or(0, |free| free.cost))
                .max(0);
        }
        value
    }

    /// Look up a TerrainObjectType by section name, case-insensitive.
    pub fn terrain_object_type_case_insensitive(&self, name: &str) -> Option<&TerrainObjectType> {
        self.terrain_object_types.get(&name.to_ascii_uppercase())
    }

    /// Incremental projection of the retained FindOrAllocate71E2A0 result.
    /// Existing types keep their bound assets; no global Rules/ART rebuild.
    pub(crate) fn install_map_terrain_type(&mut self, terrain: TerrainObjectType) {
        // This name-keyed projection covers distinct short stored IDs.
        // Repeated >24-byte inputs can allocate equal-ID native slots; their
        // separate per-slot identity remains outside this projection's scope.
        self.terrain_object_types
            .insert(terrain.name.to_ascii_uppercase(), terrain);
    }

    /// Look up a weapon by ID (case-insensitive, gamemd parity).
    pub fn weapon(&self, id: &str) -> Option<&WeaponType> {
        Self::lookup_ci(&self.weapons, id)
    }

    /// WeaponType global88756C's allocation index, used by Action42 at
    /// 6E2390. Negative/out-of-range indices and ambiguous truncated
    /// Weapon/Warhead names refuse admission rather than selecting an alias.
    pub fn weapon_by_array_index(&self, index: i32) -> Option<&WeaponType> {
        let (name, unambiguous) = self.weapon_array.get(usize::try_from(index).ok()?)?;
        unambiguous.then(|| self.weapon(name)).flatten()
    }

    /// Look up a warhead by ID (case-insensitive, gamemd parity).
    pub fn warhead(&self, id: &str) -> Option<&WarheadType> {
        Self::lookup_ci(&self.warheads, id)
    }

    /// Phobos `[ShieldTypes]` entry by name (case-insensitive, first spelling
    /// retained). No entry in a build without the capability.
    #[cfg(feature = "phobos")]
    pub fn shield_type(&self, name: &str) -> Option<&crate::rules::phobos_shields::ShieldType> {
        self.shield_types.entry(name)
    }

    /// Phobos shield globals (`[CombatDamage]`/`[AudioVisual]`).
    #[cfg(feature = "phobos")]
    pub fn shield_globals(&self) -> &crate::rules::phobos_shields::ShieldGlobals {
        &self.shield_globals
    }

    /// Phobos crit defaults (`[CombatDamage]`/`[General]`).
    #[cfg(feature = "phobos")]
    pub fn crit_globals(&self) -> &crate::rules::phobos_crit::CritGlobals {
        &self.crit_globals
    }

    /// Phobos `[DigitalDisplayTypes]` catalog.
    #[cfg(feature = "phobos")]
    pub fn digital_display_types(
        &self,
    ) -> &crate::rules::digital_display::DigitalDisplayTypeCatalog {
        &self.digital_display_types
    }

    /// Phobos `[AttachEffectTypes]` catalog.
    #[cfg(feature = "phobos")]
    pub fn attach_effect_types(&self) -> &crate::rules::attach_effect::AttachEffectTypeCatalog {
        &self.attach_effect_types
    }

    /// Phobos AttachEffect `[General]` switches.
    #[cfg(feature = "phobos")]
    pub fn attach_effect_globals(&self) -> &crate::rules::attach_effect::AttachEffectGlobals {
        &self.attach_effect_globals
    }

    /// Phobos `AttachEffect.AttachTypes` authored on a country/house type.
    #[cfg(feature = "phobos")]
    pub fn country_attach_effect_types(&self, id: &str) -> &[String] {
        self.country_rules(id)
            .map_or(&[], |country| country.attach_effect_attach_types.as_slice())
    }

    /// Phobos DigitalDisplay globals (`[AudioVisual]`).
    #[cfg(feature = "phobos")]
    pub fn digital_display_globals(&self) -> &crate::rules::digital_display::DigitalDisplayGlobals {
        &self.digital_display_globals
    }

    /// `TechnoExt::ProcessDigitalDisplays`'s list resolution: an explicit
    /// `DigitalDisplayTypes=` wins; unknown names are skipped; otherwise the
    /// category's `[AudioVisual]` default list applies. Yields catalog
    /// indices in list order without allocating.
    #[cfg(feature = "phobos")]
    pub fn digital_display_indices<'a>(
        &'a self,
        object: &'a crate::rules::object_type::ObjectType,
    ) -> impl Iterator<Item = usize> + 'a {
        let explicit = object.digital_display_types();
        let names: &[String] = if !explicit.is_empty() {
            explicit
        } else {
            match object.category {
                crate::rules::object_type::ObjectCategory::Building => {
                    &self.digital_display_globals.buildings_default
                }
                crate::rules::object_type::ObjectCategory::Infantry => {
                    &self.digital_display_globals.infantry_default
                }
                crate::rules::object_type::ObjectCategory::Vehicle => {
                    &self.digital_display_globals.vehicles_default
                }
                crate::rules::object_type::ObjectCategory::Aircraft => {
                    &self.digital_display_globals.aircraft_default
                }
            }
        };
        names
            .iter()
            .filter_map(|name| self.digital_display_types.index_of(name))
    }

    /// Phobos placement preview globals (`[AudioVisual]`).
    #[cfg(feature = "phobos")]
    pub fn placement_preview_globals(
        &self,
    ) -> &crate::rules::phobos_placement_preview::PlacementPreviewGlobals {
        &self.placement_preview_globals
    }

    /// The player-colour conversion this rules load selects.
    ///
    /// A build without the `phobos` feature reads no
    /// `[General] SkirmishUnlimitedColors=` and always answers the vanilla
    /// eight-priority table, so callers need no `cfg`.
    pub fn skirmish_colors(&self) -> crate::rules::skirmish_colors::SkirmishColorMode {
        #[cfg(feature = "phobos")]
        {
            self.skirmish_color_mode
        }
        #[cfg(not(feature = "phobos"))]
        {
            crate::rules::skirmish_colors::SkirmishColorMode::VANILLA
        }
    }

    /// Look up a projectile by ID (case-insensitive, gamemd parity).
    pub fn projectile(&self, id: &str) -> Option<&ProjectileType> {
        Self::lookup_ci(&self.projectiles, id)
    }

    /// Registered projectile types, including their retained animation references.
    pub fn projectiles_iter(&self) -> impl Iterator<Item = &ProjectileType> {
        self.projectiles.values()
    }

    /// Deterministic hash of the processed source INI this RuleSet was built from
    /// (RULESMD, optional LANGRULE, selected mode, then the map's rules-shaped
    /// pass). Stamped into
    /// diagnostic-log/snapshot headers so playback can detect a mismatched rules set —
    /// sensitive to scalar value overrides, not just the type-registry lists.
    pub fn source_ini_hash(&self) -> u64 {
        self.source_ini_hash
    }

    /// Resolve one retained palette request. Ares715857->715876 removes
    /// only the original successful-group limit: a supplied request resolves
    /// its own file; a failed file never inherits another type's group.
    /// Without Ares, retain the original one-successful-group compatibility
    /// policy and admit a supplied request only if it matches that group.
    /// None selects the singleton for the original effect/legacy consumers.
    /// This is the sole resource lookup/decoder used by core and client.
    pub fn load_extra_building_palette<'a>(
        &'a self,
        assets: &crate::assets::asset_manager::AssetManager,
        theater_ext: &str,
        requested_name: Option<&'a str>,
    ) -> Option<(&'a str, crate::assets::pal_file::Palette)> {
        let load = |name: &'a str| {
            let data = assets.get_ref(&format!("{name}{theater_ext}.pal"))?;
            crate::assets::pal_file::Palette::from_bytes(data)
                .ok()
                .map(|palette| (name, palette))
        };
        #[cfg(feature = "ares")]
        if let Some(name) = requested_name {
            return load(name);
        }
        let selected = self
            .object_list
            .iter()
            .find_map(|object| load(object.custom_palette_name()?));
        if requested_name.is_some_and(|name| {
            selected
                .as_ref()
                .is_none_or(|(loaded, _)| !name.eq_ignore_ascii_case(loaded))
        }) {
            return None;
        }
        selected
    }

    /// Techno705D70 reads this type's actual DF0 group, not DD0 text or
    /// another type's success. The immutable asset owner qualifies and
    /// returns the retained name for a producer to freeze on its animation.
    pub(crate) fn loaded_custom_palette_name(&self, object: &ObjectType) -> Option<&str> {
        self.effect_assets
            .loaded_extra_building_palette_name(object.custom_palette_name()?)
    }

    /// Sorted (upper-case ID, value) pairs for the config hash. They hash as
    /// the `BTreeMap` these walks once collected, without letting a map's
    /// iteration order (per process for `HashMap`, per target for
    /// `LookupMap`) pick which case variant survives: IDs longer than the
    /// registry's 0x18 stored bytes can differ only by case, and all are kept.
    fn by_upper_id<'a, T>(entries: impl Iterator<Item = (&'a String, T)>) -> Vec<(String, T)> {
        let mut entries: Vec<_> = entries
            .map(|(id, value)| (id.to_ascii_uppercase(), id, value))
            .collect();
        entries.sort_unstable_by(|a, b| (&a.0, a.1).cmp(&(&b.0, b.1)));
        entries
            .into_iter()
            .map(|(upper, _, value)| (upper, value))
            .collect()
    }

    /// Compatibility identity for processed rules plus the resolved animation,
    /// effect-frame, terrain-spawner frame, smudge-selection, projectile
    /// launch and TechnoType `CanBeHidden=` and `Palette=` ART inputs bound to
    /// this ruleset.
    /// Other asset-derived simulation inputs are added by later ownership
    /// slices and are not claimed by this hash yet. Replays and saves compare
    /// it across platforms, so it folds with the portable
    /// [`StateHasher`](crate::sim::state_hasher::StateHasher).
    pub fn simulation_config_hash(&self) -> u64 {
        let mut hasher = crate::sim::state_hasher::StateHasher::new();
        // v34: Country starting-veterancy; optional extensions add domains below.
        b"rules-simulation-config-v34".hash(&mut hasher);
        self.source_ini_hash.hash(&mut hasher);
        // InfantryType ReadINI5245B8..5245F5 retains both theft bytes across
        // invalid later reads. Flattened text alone cannot identify that state.
        let mut theft_flags: Vec<_> = self
            .object_list
            .iter()
            .filter(|object| {
                object.category == ObjectCategory::Infantry
                    && (object.thief || object.vehicle_thief)
            })
            .map(|object| {
                (
                    object.id.to_ascii_uppercase(),
                    object.thief,
                    object.vehicle_thief,
                )
            })
            .collect();
        if !theft_flags.is_empty() {
            theft_flags.sort_unstable();
            b"infantry-theft-flags-v1".hash(&mut hasher);
            theft_flags.hash(&mut hasher);
        }
        // OverlayRead5FE8BE / InitTheater5FE620 retain typed history that a
        // flattened Image key cannot recover; saved resource plans matter too.
        self.overlay_registry.hash_configuration(&mut hasher);
        // Fixed ART foundation state is not covered by the layered RULES
        // source hash. Include retained declared/list/outline geometry, also
        // when a later rules pass no longer authors the original custom key.
        b"building-foundation-v1".hash(&mut hasher);
        self.object_list
            .iter()
            .filter(|object| object.category == ObjectCategory::Building)
            .map(|object| (object.id.to_ascii_uppercase(), &object.foundation))
            .collect::<BTreeMap<_, _>>()
            .hash(&mut hasher);
        // ReadOwners4750D0 retains an explicit zero RequiredHouses mask
        // through later absent/empty reads. Current-pass source text does
        // not encode this history; keep ordinary stock hashes unchanged.
        let mut empty_required_houses: Vec<_> = self
            .object_list
            .iter()
            .filter(|object| {
                object.required_houses.is_empty() && object.required_houses_mask(self) == 0
            })
            .map(|object| (object.category, object.id.to_ascii_uppercase()))
            .collect();
        if !empty_required_houses.is_empty() {
            empty_required_houses.sort_unstable();
            b"required-houses-explicit-empty-v1".hash(&mut hasher);
            empty_required_houses.hash(&mut hasher);
        }
        // Bullet46BEE0 retains this byte even when a later raw value is
        // invalid. Equal flattened INIs can therefore have different flight
        // damage. Preserve the previous hash domain for all-false stock rules.
        let mut degenerating_projectiles: Vec<_> = self
            .projectiles
            .values()
            .filter(|projectile| projectile.degenerates)
            .map(|projectile| projectile.id.to_ascii_uppercase())
            .collect();
        if !degenerating_projectiles.is_empty() {
            degenerating_projectiles.sort();
            b"projectile-degenerates-v1".hash(&mut hasher);
            degenerating_projectiles.hash(&mut hasher);
        }
        b"country-parent-v1".hash(&mut hasher);
        // The native parent lookup scans registration order and checks Name
        // aliases as well as IDs. These retained inputs can differ after an
        // identical later pass, even when source_ini_hash is the same.
        self.country_ids
            .iter()
            .map(|id| {
                let country = &self.countries[id];
                (id, &country.name, &country.parent_country)
            })
            .collect::<Vec<_>>()
            .hash(&mut hasher);
        b"country-starting-veterancy-v1".hash(&mut hasher);
        Self::by_upper_id(
            self.countries
                .iter()
                .map(|(id, country)| (id, &country.veteran_types)),
        )
        .hash(&mut hasher);
        // Which plant a computer house splices is simulation state.
        #[cfg(feature = "ares")]
        {
            b"ares-country-powerplants-v1".hash(&mut hasher);
            Self::by_upper_id(
                self.countries
                    .iter()
                    .map(|(id, country)| (id, &country.power_plants)),
            )
            .hash(&mut hasher);
        }
        // Which units a country's paradrop sends is simulation state, so the
        // Ares country overrides enter the state hash alongside it.
        #[cfg(feature = "ares")]
        {
            b"ares-country-paradrop-v1".hash(&mut hasher);
            Self::by_upper_id(
                self.countries
                    .iter()
                    .map(|(id, country)| (id, &country.paradrop)),
            )
            .hash(&mut hasher);
        }
        #[cfg(feature = "phobos")]
        {
            b"phobos-super-availability-v1".hash(&mut hasher);
            self.phobos_super_weapon_metadata.hash(&mut hasher);
            self.parallel_ai_queues.hash_into(&mut hasher);
        }
        #[cfg(feature = "ares")]
        {
            b"ares-side-defaults-v1".hash(&mut hasher);
            b"ares-super-availability-v1".hash(&mut hasher);
            self.ares_super_weapon_metadata.hash(&mut hasher);
            b"ares-weapon-ammo-v1".hash(&mut hasher);
            Self::by_upper_id(
                self.weapons
                    .iter()
                    .map(|(id, weapon)| (id, weapon.ammo_cost())),
            )
            .hash(&mut hasher);
            self.object_list
                .iter()
                .filter(|object| !object.additional_super_weapons().is_empty())
                .map(|object| (&object.id, object.additional_super_weapons()))
                .collect::<Vec<_>>()
                .hash(&mut hasher);
            self.side_ids.hash(&mut hasher);
            self.side_extensions.hash(&mut hasher);
        }
        #[cfg(feature = "phobos")]
        {
            b"phobos-sidebar-gdi-positions-v1".hash(&mut hasher);
            self.side_gdi_positions.hash(&mut hasher);
        }
        #[cfg(feature = "phobos")]
        {
            b"phobos-radiation-types-v1".hash(&mut hasher);
            self.radiation.radiation_types.hash(&mut hasher);
            // Bound weapon `RadType=` names, by weapon ID: the map's
            // iteration order differs between targets.
            let mut rad_types: Vec<_> = self
                .weapons
                .iter()
                .map(|(id, weapon)| (id, &weapon.rad_type))
                .collect();
            rad_types.sort_unstable_by_key(|&(id, _)| id);
            rad_types.hash(&mut hasher);
            b"phobos-shield-types-v1".hash(&mut hasher);
            self.shield_types.hash(&mut hasher);
            self.shield_globals.apply_armor_mult.hash(&mut hasher);
            self.shield_globals
                .condition_yellow
                .to_bits()
                .hash(&mut hasher);
            self.shield_globals
                .condition_red
                .to_bits()
                .hash(&mut hasher);
            self.shield_globals.pips.hash(&mut hasher);
            self.shield_globals.pips_building.hash(&mut hasher);
            self.shield_globals.pips_background.hash(&mut hasher);
            self.shield_globals.pips_building_empty.hash(&mut hasher);
            // Bound techno `ShieldType=` names, in registry order.
            self.object_list
                .iter()
                .map(|object| (&object.id, object.shield_type()))
                .collect::<Vec<_>>()
                .hash(&mut hasher);
            b"phobos-digital-display-v1".hash(&mut hasher);
            self.digital_display_types.hash(&mut hasher);
            self.digital_display_globals.hash(&mut hasher);
            b"phobos-placement-preview-v1".hash(&mut hasher);
            self.placement_preview_globals.hash(&mut hasher);
            b"phobos-skirmish-colors-v1".hash(&mut hasher);
            self.skirmish_color_mode.hash(&mut hasher);
            self.object_list
                .iter()
                .map(|object| {
                    (
                        &object.id,
                        object.digital_display_types(),
                        object.digital_display_disable(),
                        object.digital_display_health_fake_at_disguise(),
                        object.placement_preview(),
                        object.placement_preview_shape(),
                        object.placement_preview_shape_frame(),
                        object.placement_preview_offset(),
                        object.placement_preview_remap(),
                        object.placement_preview_palette(),
                        object.placement_preview_translucency(),
                    )
                })
                .collect::<Vec<_>>()
                .hash(&mut hasher);
        }
        self.object_list
            .iter()
            .filter(|object| object.category == ObjectCategory::Building)
            .map(|object| (object.id.to_ascii_uppercase(), object.building_upgrade()))
            .collect::<BTreeMap<_, _>>()
            .hash(&mut hasher);
        // The process registry is admitted only when its retained capability
        // matches this binary. Include that immutable build identity even if
        // the selected INIs contain no extension keys.
        b"rules-build-compatibility-v1".hash(&mut hasher);
        crate::rules::ARES_ENABLED.hash(&mut hasher);
        #[cfg(feature = "ares")]
        {
            b"ares-global-controls-v1".hash(&mut hasher);
            self.ares_global_controls.hash(&mut hasher);
            b"ares-factory-owners-v1".hash(&mut hasher);
            self.object_list
                .iter()
                .map(|object| {
                    (
                        (object.category, object.id.to_ascii_uppercase()),
                        object.factory_owners_hash_state(),
                    )
                })
                .collect::<BTreeMap<_, _>>()
                .hash(&mut hasher);
            // A previous Process can retain this manual-action policy even
            // when the current source stack contains only invalid tokens.
            b"ares-no-manual-fire-v1".hash(&mut hasher);
            self.object_list
                .iter()
                .map(|object| {
                    (
                        (object.category, object.id.to_ascii_uppercase()),
                        object.no_manual_fire(),
                    )
                })
                .collect::<BTreeMap<_, _>>()
                .hash(&mut hasher);
            b"ares-keep-alive-v1".hash(&mut hasher);
            let influence_types = self
                .object_list
                .iter()
                .filter(|object| {
                    object.designator_range().is_some()
                        || object.inhibitor_range().is_some()
                        || object.hunter_seeker_ignored()
                })
                .map(|object| {
                    (
                        (object.category, object.id.to_ascii_uppercase()),
                        (
                            object.designator_range(),
                            object.inhibitor_range(),
                            object.hunter_seeker_ignored(),
                        ),
                    )
                })
                .collect::<BTreeMap<_, _>>();
            if !influence_types.is_empty() {
                b"ares-sw-influence-types-v2".hash(&mut hasher);
                influence_types.hash(&mut hasher);
            }
            self.object_list
                .iter()
                .map(|object| {
                    (
                        (object.category, object.id.to_ascii_uppercase()),
                        object.keep_alive(),
                    )
                })
                .collect::<BTreeMap<_, _>>()
                .hash(&mut hasher);
            let conversions = self
                .object_list
                .iter()
                .filter_map(|object| {
                    object
                        .script_conversion()
                        .map(|target| ((object.category, object.id.to_ascii_uppercase()), target))
                })
                .collect::<BTreeMap<_, _>>();
            if !conversions.is_empty() {
                b"ares-script-conversion-v1".hash(&mut hasher);
                conversions.hash(&mut hasher);
            }
            let surface_conversions = self
                .object_list
                .iter()
                .filter_map(|object| {
                    let targets = (
                        object.surface_conversion(true),
                        object.surface_conversion(false),
                    );
                    (targets.0.is_some() || targets.1.is_some())
                        .then(|| ((object.category, object.id.to_ascii_uppercase()), targets))
                })
                .collect::<BTreeMap<_, _>>();
            if !surface_conversions.is_empty() {
                b"ares-surface-conversion-v1".hash(&mut hasher);
                surface_conversions.hash(&mut hasher);
            }
            let turn_overrides = self
                .object_list
                .iter()
                .filter_map(|object| {
                    object
                        .turret_rot_override()
                        .map(|rate| ((object.category, object.id.to_ascii_uppercase()), rate))
                })
                .collect::<BTreeMap<_, _>>();
            if !turn_overrides.is_empty() {
                b"ares-secondary-turn-rate-v1".hash(&mut hasher);
                turn_overrides.hash(&mut hasher);
            }
        }
        // Equal flattened INIs may have different retained group lists.
        self.ares_prerequisites.hash(&mut hasher);
        b"retained-armor-bindings-and-ares-versus-v1".hash(&mut hasher);
        self.ares_armors.hash(&mut hasher);
        self.object_list
            .iter()
            .map(|object| {
                (
                    (object.category, object.id.to_ascii_uppercase()),
                    object.armor_id(),
                )
            })
            .collect::<BTreeMap<_, _>>()
            .hash(&mut hasher);
        Self::by_upper_id(
            self.terrain_object_types
                .iter()
                .map(|(name, terrain)| (name, terrain.armor_id())),
        )
        .hash(&mut hasher);
        #[cfg(feature = "phobos")]
        {
            // TerrainTypeExt::LoadFromINI retains these across Process calls.
            // They change spawner placement, particle births and RNG draws.
            b"phobos-retained-ore-spawners-v1".hash(&mut hasher);
            Self::by_upper_id(
                self.terrain_object_types
                    .iter()
                    .map(|(name, terrain)| (name, &terrain.spawns_tiberium_ext)),
            )
            .hash(&mut hasher);
        }
        // TerrainTypeReadINI71DEA0 retains post-read writes and ReadInt
        // defaults across Process calls. Equal current text can admit
        // different Strength, Logic members, targets or occupation, so identify
        // the effective fields as well as the current source layers.
        b"terrain-strength-logic-targeting-occupation-and-foundation-v6".hash(&mut hasher);
        Self::by_upper_id(self.terrain_object_types.iter().map(|(name, terrain)| {
            (
                name,
                (
                    terrain.strength,
                    terrain.is_logic(),
                    terrain.legal_target,
                    terrain.raw_occupation_bits(false),
                    terrain.raw_occupation_bits(true),
                    terrain.foundation.as_str(),
                    terrain.has_occupy_list(),
                ),
            )
        }))
        .hash(&mut hasher);
        Self::by_upper_id(
            self.warheads
                .iter()
                .map(|(name, warhead)| (name, warhead.ares_verses_hash_state())),
        )
        .hash(&mut hasher);

        #[cfg(feature = "ares")]
        {
            // TypeExt bounty values and the attacker's enable bit are gameplay
            // inputs; display-only overrides do not enter this map or its hash.
            let bounty = self
                .object_list
                .iter()
                .filter(|object| object.bounty_rules().has_gameplay_configuration())
                .map(|object| {
                    (
                        (object.category, object.id.to_ascii_uppercase()),
                        object.bounty_rules(),
                    )
                })
                .collect::<BTreeMap<_, _>>();
            if !bounty.is_empty() {
                b"ares-bounty-rules-v1".hash(&mut hasher);
                bounty.hash(&mut hasher);
            }
            // Ares TypeExt driver fields retain earlier successful reads even
            // when the merged text contains later invalid/empty values.
            let drivers = self
                .object_list
                .iter()
                .filter(|object| {
                    object.driver_rules() != &crate::rules::ares_driver::DriverTypeRules::default()
                        || object.vehicle_thief_sound(false).is_some()
                        || object.vehicle_thief_sound(true).is_some()
                })
                .map(|object| {
                    (
                        (object.category, object.id.to_ascii_uppercase()),
                        (
                            object.driver_rules(),
                            object.vehicle_thief_sound(false),
                            object.vehicle_thief_sound(true),
                        ),
                    )
                })
                .collect::<BTreeMap<_, _>>();
            if !drivers.is_empty() {
                b"ares-driver-rules-v1".hash(&mut hasher);
                drivers.hash(&mut hasher);
            }
            let aircraft_crash_overrides = self
                .object_list
                .iter()
                .filter_map(|object| {
                    object
                        .aircraft_crashable_override()
                        .map(|value| ((object.category, object.id.to_ascii_uppercase()), value))
                })
                .collect::<BTreeMap<_, _>>();
            if !aircraft_crash_overrides.is_empty() {
                b"ares-aircraft-crashable-v1".hash(&mut hasher);
                aircraft_crash_overrides.hash(&mut hasher);
            }
            let crews = self
                .object_list
                .iter()
                .filter(|object| {
                    object.crew_rules() != &crate::rules::ares_crew::CrewTypeRules::default()
                })
                .map(|object| {
                    (
                        (object.category, object.id.to_ascii_uppercase()),
                        object.crew_rules(),
                    )
                })
                .collect::<BTreeMap<_, _>>();
            if !crews.is_empty() {
                b"ares-survivor-rules-v1".hash(&mut hasher);
                crews.hash(&mut hasher);
            }
            b"ares-emp-rules-v1".hash(&mut hasher);
            self.emp_sparkles.hash(&mut hasher);
            self.object_list
                .iter()
                .map(|object| {
                    (
                        (object.category, object.id.to_ascii_uppercase()),
                        (
                            object.emp_rules(),
                            object.promotion_abilities(),
                            object.self_attach_effect(),
                            object.cloak_allowed(),
                        ),
                    )
                })
                .collect::<BTreeMap<_, _>>()
                .hash(&mut hasher);
            Self::by_upper_id(
                self.warheads
                    .iter()
                    .map(|(name, wh)| (name, wh.ares_rules())),
            )
            .hash(&mut hasher);
        }

        // UnitType747734 retains CarriesCrate across later absent/invalid
        // keys; equal current source text can still mean different cargo.
        self.object_list
            .iter()
            .filter(|object| object.category == ObjectCategory::Vehicle)
            .map(|object| (object.id.to_ascii_uppercase(), object.carries_crate()))
            .collect::<BTreeMap<_, _>>()
            .hash(&mut hasher);
        // Global construction order changes which category106 mutates when
        // IDs coincide. Current TechLevels can also differ after retained
        // default reads even when the compatibility INI is identical.
        b"native-techno-type-order-and-tech-level-v1".hash(&mut hasher);
        self.native_techno_type_order.hash(&mut hasher);
        for (category, id, handle) in &self.native_techno_type_order {
            (category, id).hash(&mut hasher);
            handle
                .map(|handle| self.object_by_handle(handle).tech_level)
                .hash(&mut hasher);
        }
        // A continued Rules process can retain a different BuildingType+EBC
        // for the same current source stack. Hash the effective projection.
        self.object_list
            .iter()
            .filter(|object| object.category == ObjectCategory::Building)
            .map(|object| (object.id.to_ascii_uppercase(), object.target_coord_offset()))
            .collect::<BTreeMap<_, _>>()
            .hash(&mut hasher);
        // Process-resident Gravity and Weapon postpass results can differ for
        // identical current source stacks because earlier passes retained them.
        self.general.gravity.hash(&mut hasher);
        self.general.tree_strength.hash(&mut hasher);
        self.general.infantry_repair_step.hash(&mut hasher);
        self.general.unit_repair_rate.to_bits().hash(&mut hasher);
        self.general.charge_to_drain_ratio.hash(&mut hasher);
        self.general
            .infantry_repair_rate
            .to_bits()
            .hash(&mut hasher);
        self.general.wind_direction.hash(&mut hasher);
        self.general.parachute_max_fall_rate.hash(&mut hasher);
        self.general.no_parachute_max_fall_rate.hash(&mut hasher);
        self.general.detail.hash(&mut hasher);
        self.general.ambient_change_rate_nonzero.hash(&mut hasher);
        self.general
            .ambient_change_interval_frames
            .hash(&mut hasher);
        self.general.ambient_change_step.hash(&mut hasher);
        self.general.prism_support.hash(&mut hasher);
        // These current-default inputs survive a process registry handoff.
        self.general.ion_cannon_damage.hash(&mut hasher);
        self.general.spotlight.hash(&mut hasher);
        #[cfg(feature = "ares")]
        {
            let extensions: Vec<_> = self
                .object_list
                .iter()
                .filter(|object| {
                    object.has_spotlight || object.spotlight_rules() != Default::default()
                })
                .map(|object| {
                    (
                        object.category,
                        &object.id,
                        object.has_spotlight,
                        object.spotlight_rules(),
                    )
                })
                .collect();
            if !extensions.is_empty() {
                b"ares-spotlight-type-v1".hash(&mut hasher);
                extensions.hash(&mut hasher);
            }
        }
        self.object_list
            .iter()
            .filter(|o| o.category == ObjectCategory::Building)
            .map(|o| (&o.id, o.has_spotlight))
            .collect::<Vec<_>>()
            .hash(&mut hasher);
        self.general.ion_blast_anim.hash(&mut hasher);
        self.general.ion_beam_anim.hash(&mut hasher);
        self.bridge_warheads.ion_cannon_name.hash(&mut hasher);
        // Earlier Rules passes retain these signed divisors even when the
        // current source stack omits them; sale crew and its RNG consumers
        // must not be presented as the same effective configuration.
        self.general.allied_survivor_divisor.hash(&mut hasher);
        self.general.soviet_survivor_divisor.hash(&mut hasher);
        self.general.third_survivor_divisor.hash(&mut hasher);
        self.general.maximum_cheer_rate.hash(&mut hasher);
        self.general.comp_easy_bonus.hash(&mut hasher);
        self.general.patrol_scan_minutes.hash(&mut hasher);
        if self.general.shroud_grow || self.general.shroud_rate != default_shroud_rate() {
            b"shroud-growth-controls-v1".hash(&mut hasher);
            self.general.shroud_grow.hash(&mut hasher);
            self.general.shroud_rate.hash(&mut hasher);
        }
        if self.general.fog_rate != default_fog_rate() {
            b"fog-regrowth-rate-v1".hash(&mut hasher);
            self.general.fog_rate.hash(&mut hasher);
        }
        // Earlier Process passes can retain these despite identical current text.
        if self.general.drop_zone_anim.is_some() || self.general.drop_zone_radius != 1024 {
            b"drop-zone-controls-v1".hash(&mut hasher);
            self.general.drop_zone_anim.hash(&mut hasher);
            self.general.drop_zone_radius.hash(&mut hasher);
        }
        // An earlier Process pass can retain Behind past an absent key.
        if self.general.behind.is_some() {
            b"behind-anim-v1".hash(&mut hasher);
            self.general.behind.hash(&mut hasher);
        }
        // CanBeHidden is read from ART (0x007121EB), outside the source INI
        // hash; it decides which objects construct the Behind anim.
        self.object_list
            .iter()
            .map(|o| (&o.id, o.can_be_hidden))
            .collect::<Vec<_>>()
            .hash(&mut hasher);
        // So is Palette= (0x0071582B); DestructionEffects gives a custom
        // palette type's DestroyAnim its scheme (0x00441D2F).
        self.object_list
            .iter()
            .filter(|o| o.custom_palette_name().is_some())
            .map(|o| (&o.id, o.custom_palette_name()))
            .collect::<Vec<_>>()
            .hash(&mut hasher);
        self.general.default_mirage_disguises.hash(&mut hasher);
        self.general.infantry_disguises.hash(&mut hasher);
        if self
            .general
            .secret_lab_choices
            .iter()
            .any(|list| !list.is_empty())
        {
            b"secret-production-choices-v1".hash(&mut hasher);
            self.general.secret_lab_choices.hash(&mut hasher);
        }
        for building in self
            .object_list
            .iter()
            .filter(|object| object.category == ObjectCategory::Building)
        {
            if building.secret_lab() != &crate::rules::secret_lab::SecretLabRules::default() {
                b"secret-lab-type-v1".hash(&mut hasher);
                building.id.hash(&mut hasher);
                building.secret_lab().hash(&mut hasher);
            }
        }
        self.general.infantry_blink_disguise_time.hash(&mut hasher);
        let nominal_types = self
            .object_list
            .iter()
            .filter(|object| object.nominal())
            .map(|object| ((object.category, object.id.to_ascii_uppercase()), true))
            .collect::<BTreeMap<_, _>>();
        if !nominal_types.is_empty() {
            b"retained-type-nominal-v1".hash(&mut hasher);
            nominal_types.hash(&mut hasher);
        }
        // The native type reader retains these flags through missing keys
        // and later process passes. Equal current source text can therefore
        // yield different disguise admission, reveal and observer behavior.
        b"retained-type-disguise-v1".hash(&mut hasher);
        self.object_list
            .iter()
            .map(|object| {
                (
                    (object.category, object.id.to_ascii_uppercase()),
                    (
                        object.can_disguise,
                        object.perma_disguise,
                        object.detect_disguise,
                        object.disguise_when_still,
                    ),
                )
            })
            .collect::<BTreeMap<_, _>>()
            .hash(&mut hasher);
        // The ordered Rules processor retains these references through
        // missing keys and later passes: each TechnoType's
        // (`TechnoTypeReferences`), ShrapnelWeapon +2B4, SuperWeapon
        // WeaponType +9C and AuxBuilding +C8, the WeaponType anims and
        // AttachedParticleSystem, ParticleType and VoxelAnimType references,
        // Warhead DebrisTypes, the AnimTypes' fixed-ART references and the
        // RulesClass Type pointers and lists. RulesClass also keeps the crate
        // and rocket blocks across Process calls, their scalars across
        // Full_Init's reset.
        b"retained-type-references-v3".hash(&mut hasher);
        self.object_list
            .iter()
            .map(|object| {
                (
                    (object.category, object.id.to_ascii_uppercase()),
                    (
                        (
                            &object.image,
                            &object.death_weapon,
                            &object.deploys_into,
                            &object.occupy_weapon,
                            &object.elite_occupy_weapon,
                            &object.undeploys_into,
                            &object.powers_unit,
                            &object.free_unit,
                            &object.to_overlay,
                        ),
                        (
                            &object.dock,
                            &object.unloading_class,
                            &object.deploying_anim,
                            &object.enslaves,
                            &object.spawns,
                            &object.explosion_anims,
                            &object.destroy_anims,
                            &object.dead_bodies,
                        ),
                        (
                            &object.natural_particle_system,
                            &object.refinery_smoke_particle_system,
                            &object.damage_particle_systems,
                            &object.destroy_particle_systems,
                            &object.debris_types,
                            &object.debris_anims,
                        ),
                    ),
                )
            })
            .collect::<BTreeMap<_, _>>()
            .hash(&mut hasher);
        Self::by_upper_id(
            self.projectiles
                .iter()
                .map(|(id, projectile)| (id, &projectile.shrapnel_weapon)),
        )
        .hash(&mut hasher);
        let airburst_weapons =
            Self::by_upper_id(self.projectiles.iter().filter_map(|(id, projectile)| {
                projectile
                    .airburst_weapon
                    .as_ref()
                    .map(|weapon| (id, weapon))
            }));
        if !airburst_weapons.is_empty() {
            b"airburst-weapons-v1".hash(&mut hasher);
            airburst_weapons.hash(&mut hasher);
        }
        // A later invalid raw token retains BulletType+294. Equal flattened
        // INIs can therefore differ in the child-fan and parent-cluster gates.
        let airburst_flags = self
            .projectiles
            .iter()
            .filter(|(_, projectile)| projectile.airburst)
            .map(|(id, _)| id.to_ascii_uppercase())
            .collect::<std::collections::BTreeSet<_>>();
        if !airburst_flags.is_empty() {
            b"airburst-retained-flag-v1".hash(&mut hasher);
            airburst_flags.hash(&mut hasher);
        }
        // Phobos airburst/splits inputs. Defaults leave the stock hash
        // unchanged; any authored value must invalidate a save.
        #[cfg(feature = "phobos")]
        {
            let phobos_projectiles =
                Self::by_upper_id(self.projectiles.iter().filter_map(|(id, projectile)| {
                        let value = &projectile.phobos_airburst;
                        (value != &crate::rules::phobos_airburst::PhobosAirburstType::default())
                            .then(|| {
                                let fields = (
                                    (
                                        value.splits,
                                        value.airburst_spread.to_bits(),
                                        value.retarget_accuracy.to_bits(),
                                        value.retarget_self,
                                        value.retarget_self_probability.to_bits(),
                                    ),
                                    (
                                        value.around_target,
                                        value.airburst_use_cluster,
                                        value.airburst_random_clusters,
                                        value.airburst_target_as_source,
                                        value.airburst_target_as_source_skip_height,
                                    ),
                                    (
                                        value.splits_targeting_distance,
                                        value.splits_targeting_distance_cylindrical,
                                        value.splits_allow_repeat_targets,
                                        value.splits_target_cell_range,
                                        value.splits_use_weapon_targeting,
                                        value.airburst_weapon_apply_firepower_mult,
                                        value.airburst_weapon_source_scatter_min,
                                        value.airburst_weapon_source_scatter_max,
                                        value.airburst_weapon_use_firing_effects,
                                        value.airburst_weapon_head_to_target,
                                        value.airburst_weapon_radial_fire_segments,
                                        value.aa_only,
                                    ),
                                );
                                (id, fields)
                            })
                }));
            if !phobos_projectiles.is_empty() {
                b"phobos-airburst-splits-v1".hash(&mut hasher);
                phobos_projectiles.hash(&mut hasher);
            }
            if self.phobos_airburst != Default::default() {
                b"phobos-airburst-globals-v1".hash(&mut hasher);
                self.phobos_airburst.hash(&mut hasher);
            }
            let can_target_houses = Self::by_upper_id(
                self.weapons
                    .iter()
                    .filter(|(_, weapon)| {
                        weapon.can_target_houses()
                            != crate::rules::phobos_shields::AffectedHouse::ALL
                    })
                    .map(|(id, weapon)| (id, weapon.can_target_houses().bits())),
            );
            if !can_target_houses.is_empty() {
                b"phobos-weapon-can-target-houses-v1".hash(&mut hasher);
                can_target_houses.hash(&mut hasher);
            }
            let weapon_targeting = Self::by_upper_id(
                self.weapons
                    .iter()
                    .filter(|(_, weapon)| {
                        weapon.weapon_targeting()
                            != crate::rules::phobos_airburst::PhobosWeaponTargeting::default()
                    })
                    .map(|(id, weapon)| (id, weapon.weapon_targeting())),
            );
            if !weapon_targeting.is_empty() {
                b"phobos-weapon-targeting-v1".hash(&mut hasher);
                weapon_targeting.len().hash(&mut hasher);
                for (id, targeting) in &weapon_targeting {
                    id.hash(&mut hasher);
                    targeting.hash_state(&mut hasher);
                }
            }
        }
        Self::by_upper_id(self.super_weapons.iter().map(|(id, super_weapon)| {
            (id, (&super_weapon.weapon_type, &super_weapon.aux_building))
        }))
        .hash(&mut hasher);
        // Keep stock rules hashes unchanged. Extension payload references are
        // resolved state too: a layered clear/rebind must invalidate a save.
        let generic_warheads = Self::by_upper_id(
            self.super_weapons
                .iter()
                .filter_map(|(id, sw)| sw.generic_warhead.as_ref().map(|settings| (id, settings))),
        );
        if !generic_warheads.is_empty() {
            generic_warheads.hash(&mut hasher);
        }
        let hunters = Self::by_upper_id(
            self.super_weapons
                .iter()
                .filter_map(|(id, sw)| sw.hunter_seeker.as_ref().map(|settings| (id, settings))),
        );
        if !hunters.is_empty() {
            b"ares-hunter-seeker-v1".hash(&mut hasher);
            hunters.hash(&mut hasher);
            #[cfg(feature = "ares")]
            self.ares_global_controls
                .hunter_seeker_buildings()
                .hash(&mut hasher);
        }
        // No extension tags means no extra hash input: preserve stock hashes.
        let building_requirements =
            Self::by_upper_id(self.super_weapons.iter().filter_map(|(id, sw)| {
                sw.building_requirements
                    .as_ref()
                    .map(|requirements| (id, requirements))
            }));
        if !building_requirements.is_empty() {
            "Ares.SuperWeaponBuildingRequirements".hash(&mut hasher);
            building_requirements.hash(&mut hasher);
        }
        let superweapon_money = Self::by_upper_id(
            self.super_weapons
                .iter()
                .filter(|(_, sw)| sw.money_amount != 0)
                .map(|(id, sw)| (id, (sw.money_amount, &sw.insufficient_funds_eva))),
        );
        if !superweapon_money.is_empty() {
            "Ares.SuperWeaponMoney".hash(&mut hasher);
            superweapon_money.hash(&mut hasher);
        }
        #[cfg(feature = "ares")]
        {
            let default = crate::rules::superweapon_type::AresSuperWeaponPresentation::default();
            let presentation = Self::by_upper_id(
                self.super_weapons
                    .iter()
                    .filter(|(_, sw)| sw.presentation != default)
                    .map(|(id, sw)| (id, &sw.presentation)),
            );
            if !presentation.is_empty() {
                "Ares.SuperWeaponPresentation".hash(&mut hasher);
                presentation.hash(&mut hasher);
            }
        }
        let deliveries = Self::by_upper_id(
            self.super_weapons
                .iter()
                .filter_map(|(id, sw)| sw.unit_delivery.as_ref().map(|delivery| (id, delivery))),
        );
        if !deliveries.is_empty() {
            "Ares.UnitDelivery".hash(&mut hasher);
            deliveries.hash(&mut hasher);
        }
        Self::by_upper_id(self.weapons.iter().map(|(id, weapon)| {
            (
                id,
                (
                    &weapon.anim,
                    &weapon.occupant_anim,
                    &weapon.open_topped_anim,
                    &weapon.attached_particle_system,
                ),
            )
        }))
        .hash(&mut hasher);
        Self::by_upper_id(
            self.warheads
                .iter()
                .map(|(id, warhead)| (id, &warhead.debris_types)),
        )
        .hash(&mut hasher);
        #[cfg(feature = "phobos")]
        {
            let jumpjet_overrides = Self::by_upper_id(
                self.warheads
                    .iter()
                    .filter(|(_, warhead)| !warhead.jumpjet_override.is_empty())
                    .map(|(id, warhead)| (id, warhead.jumpjet_override)),
            );
            if !jumpjet_overrides.is_empty() {
                b"phobos-warhead-jumpjet-override-v1".hash(&mut hasher);
                jumpjet_overrides.len().hash(&mut hasher);
                for (id, overrides) in &jumpjet_overrides {
                    id.hash(&mut hasher);
                    overrides.hash_state(&mut hasher);
                }
            }
        }
        for particle in &self.particle_types {
            (&particle.name, &particle.warhead).hash(&mut hasher);
        }
        for voxel_anim in &self.voxel_anim_types {
            (
                &voxel_anim.name,
                &voxel_anim.spawns,
                &voxel_anim.bounce_anim,
                &voxel_anim.expire_anim,
                &voxel_anim.trailer_anim,
                &voxel_anim.warhead,
                &voxel_anim.attached_system,
            )
                .hash(&mut hasher);
        }
        self.anim_type_references.hash(&mut hasher);
        self.combat_damage.ammo_crate_damage.hash(&mut hasher);
        // Native retained defaults can differ with identical incoming INIs.
        self.combat_damage
            .tiberium_explosion_damage
            .hash(&mut hasher);
        for tiberium in self.tiberium_types.types() {
            (
                &tiberium.section,
                tiberium.color_scheme(),
                tiberium.debris(),
            )
                .hash(&mut hasher);
        }
        self.general.barrel_explode.hash(&mut hasher);
        self.general.barrel_debris.hash(&mut hasher);
        for kind in &self.voxel_anim_types {
            kind.damage.hash(&mut hasher);
        }
        self.combat_damage.flame_damage().hash(&mut hasher);
        (
            self.combat_damage.flame_damage2(),
            &self.bridge_warheads.c4_name,
            &self.bridge_warheads.crush_name,
            &self.bridge_warheads.ion_cannon_name,
            &self.combat_damage.ivan_warhead,
            &self.combat_damage.drain_animation_type,
            &self.mind_control.controlled_anim,
            &self.mind_control.perma_controlled_anim,
            &self.radiation.site_warhead,
            &self.general.mutate_warhead,
            &self.general.mutate_explosion_warhead,
            &self.general.empulse_warhead,
        )
            .hash(&mut hasher);
        #[cfg(feature = "phobos")]
        {
            if !self.mind_control.mind_control_ignore_size
                || self.mind_control.mind_control_link_visible_to_house
                    != crate::rules::phobos_shields::AffectedHouse::ALL
                || self.mind_control.multi_mind_control_release_victim
            {
                b"phobos-mind-control-globals-v1".hash(&mut hasher);
                self.mind_control.mind_control_ignore_size.hash(&mut hasher);
                self.mind_control
                    .mind_control_link_visible_to_house
                    .bits()
                    .hash(&mut hasher);
                self.mind_control
                    .multi_mind_control_release_victim
                    .hash(&mut hasher);
            }
            let control_sizes = Self::by_upper_id(self.object_list.iter().filter_map(|object| {
                    let value = (
                        object.mind_control_range_limit,
                        object.mind_control_ignore_size,
                        object.mind_control_size,
                        object
                            .mind_control_link_visible_to_house
                            .map(|house| house.bits()),
                        object.multi_mind_control_release_victim,
                    );
                (value != (0, None, 1, None, None)).then_some((&object.id, value))
            }));
            if !control_sizes.is_empty() {
                b"phobos-mind-control-size-v1".hash(&mut hasher);
                control_sizes.hash(&mut hasher);
            }
        }
        let general = &self.general;
        if crate::rules::NPATCH_ENABLED {
            general.vehicle_parachute.hash(&mut hasher);
        }
        (
            (
                &general.ore_twinkle,
                &general.barrel_particle,
                &general.nuke_take_off,
                &general.wake.name,
                &general.warp_in.name,
                &general.warp_out.name,
                &general.warp_away.name,
                &general.chrono_sparkle1.name,
                &general.move_flash.name,
                &general.parachute_shp,
            ),
            (
                &general.dominator_warhead,
                &general.dominator_first_anim,
                &general.dominator_second_anim,
                &general.chrono_placement_anim,
                &general.chrono_blast_anim,
                &general.chrono_blast_dest_anim,
                &general.iron_curtain_invoke_anim,
                &general.force_shield_invoke_anim,
                &general.infantry_death_anims,
            ),
            (
                &general.engineer_infantry,
                &general.technician,
                &general.allied_crew,
                &general.soviet_crew,
                &general.third_crew,
            ),
        )
            .hash(&mut hasher);
        general
            .damage_fire_types
            .iter()
            .map(|anim| &anim.name)
            .collect::<Vec<_>>()
            .hash(&mut hasher);
        (
            &general.dead_bodies,
            &general.base_unit_types,
            &general.pad_aircraft_types,
            &self.shipyard_types,
            &self.repair_bay_types,
            &self.harvester_unit_types,
        )
            .hash(&mut hasher);
        (
            (
                &self.build_const_types,
                &self.build_power_types,
                &self.build_refinery_types,
                &self.build_barracks_types,
                &self.build_tech_types,
                &self.build_weapons_types,
            ),
            (
                &self.build_radar_types,
                &self.allied_base_defense_types,
                &self.soviet_base_defense_types,
                &self.third_base_defense_types,
                &self.concrete_wall_types,
                &self.ns_gate_types,
                &self.ew_gate_types,
            ),
        )
            .hash(&mut hasher);
        self.crate_rules.hash(&mut hasher);
        self.missile_spawn.hash(&mut hasher);
        let damage = &self.combat_damage;
        (
            &damage.default_large_grey_smoke_system,
            &damage.default_small_grey_smoke_system,
            &damage.default_spark_system,
            &damage.default_large_red_smoke_system,
            &damage.default_small_red_smoke_system,
            &damage.default_debris_smoke_system,
            &damage.default_fire_stream_system,
            &damage.default_test_particle_system,
            &damage.default_repair_particle_system,
        )
            .hash(&mut hasher);
        self.general.missile_rot_var.to_bits().hash(&mut hasher);
        self.general.safety_altitude.hash(&mut hasher);
        self.general.line_trail_color_override.hash(&mut hasher);
        self.general.chrono_beam_color.hash(&mut hasher);
        self.general.lightning_warhead.hash(&mut hasher);
        self.general.weather_con_bolt_explosion.hash(&mut hasher);
        self.general.weapon_nullify_anim.hash(&mut hasher);
        #[cfg(feature = "phobos")]
        self.general.building_production_queue.hash(&mut hasher);
        #[cfg(feature = "phobos")]
        {
            b"phobos-ai-dehardcode-v1".hash(&mut hasher);
            self.general.ai_fire_sale.hash(&mut hasher);
            self.general.ai_fire_sale_delay.hash(&mut hasher);
            self.general.ai_all_to_hunt.hash(&mut hasher);
            self.general.gather_when_mcv_deploy.hash(&mut hasher);
        }
        self.combat_damage.splash_list.hash(&mut hasher);
        self.combat_damage.death_weapon.hash(&mut hasher);
        // The live family order is independent of HashMap iteration and may
        // survive earlier passes absent from the current source stack.
        self.weapon_array.hash(&mut hasher);
        Self::by_upper_id(self.warheads.iter().map(|(name, wh)| {
            (
                name,
                (
                    wh.conventional,
                    wh.em_effect,
                    &wh.anim_list,
                    wh.verses_f64.map(f64::to_bits),
                ),
            )
        }))
        .hash(&mut hasher);
        #[cfg(feature = "phobos")]
        {
            let warhead_anims = Self::by_upper_id(self.warheads.iter().filter_map(|(name, warhead)| {
                (warhead.phobos_anim
                    != crate::rules::phobos_warhead_anim::PhobosWarheadAnimRules::default())
                .then_some((name, &warhead.phobos_anim))
            }));
            if !warhead_anims.is_empty() {
                b"phobos-warhead-anim-v1".hash(&mut hasher);
                warhead_anims.hash(&mut hasher);
            }
            if self.phobos_warhead_anim != Default::default() {
                b"phobos-warhead-anim-globals-v1".hash(&mut hasher);
                self.phobos_warhead_anim.hash(&mut hasher);
            }
        }
        #[cfg(feature = "phobos")]
        {
            let control_anims = Self::by_upper_id(self.warheads.iter().filter_map(|(name, wh)| {
                wh.mind_control_anim
                    .as_ref()
                    .map(|anim| (name, anim.to_ascii_uppercase()))
            }));
            if !control_anims.is_empty() {
                b"phobos-warhead-mind-control-anim-v1".hash(&mut hasher);
                control_anims.hash(&mut hasher);
            }
        }
        Self::by_upper_id(self.weapons.iter().map(|(id, weapon)| {
            (
                id,
                (
                    (
                        weapon.speed,
                        &weapon.projectile,
                        weapon.damage,
                        &weapon.warhead,
                        weapon.range_leptons,
                    ),
                    (
                        weapon.is_laser,
                        weapon.is_house_color,
                        weapon.is_big_laser,
                        weapon.laser_duration,
                        weapon.laser_inner_color,
                        weapon.laser_outer_color,
                        weapon.laser_outer_spread,
                    ),
                ),
            )
        }))
        .hash(&mut hasher);
        // Selection order and duplicate references affect the scenario RNG's
        // consumers, including the truncated retail pool's unread AnimType D.
        self.general.metallic_debris.hash(&mut hasher);
        self.general.weather_con_clouds.hash(&mut hasher);
        self.general.weather_con_bolts.hash(&mut hasher);
        self.bridge_rules.explosions.hash(&mut hasher);
        self.animation_sequences.hash(&mut hasher);
        self.unit_shp_read_states.hash(&mut hasher);
        self.effect_assets.hash(&mut hasher);
        self.voxel_assets.hash(&mut hasher);
        self.buildup_assets.hash(&mut hasher);
        self.rubble_assets.hash(&mut hasher);
        self.terrain_assets.hash(&mut hasher);
        b"art-smudge-config-v2".hash(&mut hasher);
        let smudge_anim_inputs = self
            .art_registry
            .iter_entries()
            .filter(|(_, entry)| entry.scorch || entry.crater || entry.force_big_craters)
            .map(|(name, entry)| {
                (
                    name.to_string(),
                    (
                        entry.scorch,
                        entry.crater,
                        entry.force_big_craters,
                        entry.frame_width,
                        entry.frame_height,
                    ),
                )
            })
            .collect::<BTreeMap<_, _>>();
        smudge_anim_inputs.hash(&mut hasher);
        // These effective Bullet values feed FireAt and drawing after load. Hash
        // consumed values in canonical type order, not ART insertion order,
        // authored-key presence, or unrelated presentation metadata.
        b"art-projectile-config-v4".hash(&mut hasher);
        Self::by_upper_id(self.projectiles.iter().map(|(id, p)| {
            (
                id,
                (
                    (
                        &p.image,
                        &p.image_loads,
                        p.voxel_image_load(),
                        p.voxel,
                        p.theater,
                        p.new_theater,
                        p.inviso,
                    ),
                    (p.rotates, p.flat, p.anim_palette, p.color_scheme()),
                    (
                        p.use_line_trail,
                        p.line_trail_color,
                        p.line_trail_color_decrement,
                    ),
                    (
                        p.anim_low,
                        p.anim_high,
                        p.anim_rate,
                        p.spawn_delay,
                        &p.trailer,
                    ),
                ),
            )
        }))
        .hash(&mut hasher);
        // The fixed SOUNDMD registry changes which names resolve even when
        // Rules text is identical. Empty versus nonempty vectors gates Main
        // draws; list order/duplicates select the sound. Hash effective lists
        // in canonical type order, independently of registry insertion order.
        b"resolved-type-sound-lists-v4".hash(&mut hasher);
        #[cfg(feature = "ares")]
        {
            let promotion_sounds = self
                .object_list
                .iter()
                .filter_map(|object| {
                    let sounds = (
                        object.promotion_sound_override(false),
                        object.promotion_sound_override(true),
                    );
                    (sounds.0.is_some() || sounds.1.is_some())
                        .then(|| ((object.category, object.id.to_ascii_uppercase()), sounds))
                })
                .collect::<BTreeMap<_, _>>();
            if !promotion_sounds.is_empty() {
                b"ares-type-promotion-sounds-v1".hash(&mut hasher);
                promotion_sounds.hash(&mut hasher);
            }
        }
        self.general.voice_ifv_repair().hash(&mut hasher);
        self.general.upgrade_veteran_sound.hash(&mut hasher);
        self.general.upgrade_elite_sound.hash(&mut hasher);
        self.object_list
            .iter()
            .map(|object| {
                (
                    (object.category, object.id.to_ascii_uppercase()),
                    (
                        &object.voice_select,
                        &object.voice_select_enslaved,
                        object.voice_move(),
                        object.voice_enter(),
                        object.voice_capture(),
                        object.voice_harvest(),
                        &object.move_sound,
                        &object.voice_special_attack,
                        &object.voice_feedback,
                        object.voice_attack(),
                        std::array::from_fn::<_, 4, _>(|slot| {
                            object.voice_attack_override((slot / 2) as i32, slot % 2 != 0)
                        }),
                    ),
                )
            })
            .collect::<BTreeMap<_, _>>()
            .hash(&mut hasher);
        self.object_list
            .iter()
            .filter(|object| object.category == ObjectCategory::Building)
            .map(|object| {
                (
                    object.id.to_ascii_uppercase(),
                    self.building_launch_height(object),
                )
            })
            .collect::<BTreeMap<_, _>>()
            .hash(&mut hasher);
        // Elite Airstrike fallbacks depend on reached pass boundaries, not
        // merely merged text; managers copy these values into gameplay state.
        b"airstrike-settings-v1".hash(&mut hasher);
        self.general.airstrike_abort_sound.hash(&mut hasher);
        self.general.airstrike_attack_voice.hash(&mut hasher);
        self.general.laser_target_color.hash(&mut hasher);
        self.object_list
            .iter()
            .map(|object| {
                (
                    (object.category, object.id.to_ascii_uppercase()),
                    &object.airstrike,
                )
            })
            .collect::<BTreeMap<_, _>>()
            .hash(&mut hasher);
        // Radial launch settings likewise survive earlier reached passes even
        // when the current source stack omits their key.
        b"radial-fire-segments-v1".hash(&mut hasher);
        self.object_list
            .iter()
            .map(|object| {
                (
                    (object.category, object.id.to_ascii_uppercase()),
                    object.radial_fire_segments,
                )
            })
            .collect::<BTreeMap<_, _>>()
            .hash(&mut hasher);
        // Reached-pass barrel resets are not derivable from final merged text.
        b"voxel-recoil-config-v1".hash(&mut hasher);
        for object in &self.object_list {
            object.recoil.hash(&mut hasher);
        }
        // The same current source stack can retain different FV mappings from
        // earlier Process calls. Gunner initialization and passenger entry use
        // these fields, so restore must compare the effective owner state.
        b"gunner-turret-config-v1".hash(&mut hasher);
        self.object_list
            .iter()
            .map(|object| {
                (
                    (object.category, object.id.to_ascii_uppercase()),
                    (
                        object.gunner,
                        object.ifv_mode,
                        object.turret_count,
                        &object.gunner_turrets,
                    ),
                )
            })
            .collect::<BTreeMap<_, _>>()
            .hash(&mut hasher);
        // GetFLH consumes the retained type records and the existing pivot.
        b"art-weapon-flh-v2".hash(&mut hasher);
        self.object_list
            .iter()
            .map(|object| {
                let art = self
                    .art_registry
                    .get(&object.image)
                    .or_else(|| self.art_registry.get(&object.id));
                let slots = (0..object.weapon_record_count())
                    .map(|index| {
                        let elite = object
                            .elite_weapon_list
                            .get(index)
                            .is_some_and(Option::is_some);
                        (
                            object.weapon_flh(index as i32, false),
                            object.weapon_flh(index as i32, elite),
                        )
                    })
                    .collect::<Vec<_>>();
                let ports = object.open_topped.then(|| {
                    (0..5)
                        .map(|port| object.alternate_fire_flh(port))
                        .collect::<Vec<_>>()
                });
                let consumed = (art.map_or(0, |art| art.turret_offset), slots, ports);
                (object.id.to_ascii_uppercase(), consumed)
            })
            .collect::<BTreeMap<_, _>>()
            .hash(&mut hasher);
        self.hash_building_body_config(&mut hasher);
        self.hash_building_slot_config(&mut hasher);
        hasher.finish()
    }

    /// Slot constructors consume selected names, offsets and the common runtime
    /// metadata. Canonical ordering makes ART section insertion order irrelevant.
    fn hash_building_slot_config(&self, hasher: &mut impl Hasher) {
        b"building-slot-config-v1".hash(hasher);
        self.anim_type_names.hash(hasher);
        self.anim_type_art_read_states.hash(hasher);
        self.anim_type_images.hash(hasher);
        self.art_registry.scheduler_anim_types().hash(hasher);
        let mut types = self.anim_type_names.clone();
        types.extend(self.art_registry.scheduler_anim_types().iter().cloned());
        for name in types {
            name.hash(hasher);
            self.art_registry.anim_runtime_config(&name).hash(hasher);
        }
        let buildings = self
            .object_list
            .iter()
            .filter(|object| object.category == ObjectCategory::Building)
            .map(|object| (object.id.to_ascii_uppercase(), object))
            .collect::<BTreeMap<_, _>>();
        for (name, object) in buildings {
            name.hash(hasher);
            object.powered.hash(hasher);
            object.powered_special.hash(hasher);
            let entry = self
                .art_registry
                .resolve_metadata_entry(&object.id, &object.image);
            entry
                .map(|e| e.building_anim_power)
                .unwrap_or([Default::default(); 21])
                .hash(hasher);
            entry.is_some_and(|e| e.is_anim_delayed_fire).hash(hasher);
            entry.is_some_and(|e| e.silo_damage).hash(hasher);
            let mut slots = self
                .art_registry
                .resolve_metadata_entry(&object.id, &object.image)
                .map(|entry| {
                    entry
                        .building_anims
                        .iter()
                        //0..2 are immutable asset declarations here. The
                        // consumed PowerUp records live on BuildingUpgradeRules
                        // and are hashed once with that retained Type state.
                        .filter(|config| config.native_slot >= 3)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            slots.sort_by_key(|config| config.native_slot);
            slots.hash(hasher);
        }
    }

    /// Effective body inputs used by the receiver's native43EF90 frame guard.
    /// Parser provenance: tools/spatial_oracle/building_body_rules.json.
    fn hash_building_body_config(&self, hasher: &mut impl Hasher) {
        b"building-body-config-v1".hash(hasher);
        self.object_list
            .iter()
            .filter(|object| object.category == ObjectCategory::Building)
            .map(|object| {
                let art = self
                    .art_registry
                    .resolve_metadata_entry(&object.id, &object.image);
                (
                    object.id.to_ascii_uppercase(),
                    (
                        object.firestorm_wall,
                        art.map_or(9, |entry| entry.building_gate_stages),
                        art.map_or([[0, 1, 0]; 4], |entry| entry.building_body_ranges),
                    ),
                )
            })
            .collect::<BTreeMap<_, _>>()
            .hash(hasher);
    }

    /// FireAt's Building Height input: native BuildingRead4610D8..461101
    /// reads ART[effective Image].Height, retaining constructor default2 for
    /// fresh missing data. Explicit Image does not fall back to type ID.
    /// Stock reload equivalence is bounded by the retail Image census; arbitrary
    /// image-changing reload history remains an open producer contract.
    pub(crate) fn building_launch_height(&self, object: &ObjectType) -> i32 {
        self.art_registry
            .get(&object.image)
            .map_or(2, |art| art.height)
    }

    /// BuildingType Dimension2 @0x00464AF0, virtual+7C: the foundation's
    /// width/height in 256-lepton cells and fixed ART Height times 104. Shares
    /// the existing Height reader/default with FireAt's distinct Height * 200
    /// launch calculation. Original execution: building_brackets.json and
    /// spatial_oracle/height_factor.json; consumers do not copy this query.
    pub fn building_dimensions(&self, object: &ObjectType) -> [i32; 3] {
        let [width, height] = object.foundation.dimensions_leptons();
        [
            width,
            height,
            self.building_launch_height(object)
                .wrapping_mul(crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS),
        ]
    }

    /// HouseType Multiplay eligibility from the same retained Country body
    /// as gameplay.4E3A00 also requires a supported index and nonempty
    /// localized UIName; the app catalog owns that presentation check.
    pub(crate) fn refresh_country_side_projection<'a>(
        &mut self,
        countries: impl Iterator<Item = CountryProjection<'a>>,
        sides: impl Iterator<Item = SideProjection<'a>>,
    ) {
        let projection = project_country_side_registry(countries, sides);
        self.countries = projection.rules;
        self.country_ids = projection.country_ids;
        self.country_indices = projection.country_indices;
        self.side_ids = projection.side_ids;
        #[cfg(feature = "ares")]
        {
            self.side_extensions = projection.side_extensions;
        }
        self.side_indices = projection.side_indices;
        self.country_sides = projection.country_sides;
    }

    /// Native Country veteran-list membership uses the current HouseType,
    /// not ParentCountry, and is independent of Trainable/Naval/spy state.
    /// Starting-rank callers: Infantry517CE7, Unit735604/746827,
    /// Aircraft413F98. With Ares, Building Init_Managers442D1B reaches
    ///100236D0 and its retained VeteranBuildings list with the same gates.
    pub fn country_starts_veteran(&self, country: &str, object: &ObjectType) -> bool {
        self.country_rules(country)
            .is_some_and(|rules| rules.veteran_types.contains(object.category, &object.id))
    }

    pub fn country_multiplay(&self, id: &str) -> bool {
        self.country_rules(id)
            .is_some_and(|country| country.multiplay)
    }

    /// Whether a country/house type has `MultiplayPassive=true`.
    pub fn country_multiplay_passive(&self, id: &str) -> bool {
        self.country_rules(id)
            .is_some_and(|country| country.multiplay_passive)
    }

    /// Ares `File.Flag=` (stock/unknown defaults included). None in a stock
    /// build, where the shell keeps its fixed flag table.
    pub fn country_flag(&self, id: &str) -> Option<&str> {
        #[cfg(feature = "ares")]
        {
            return self
                .country_rules(id)
                .and_then(|country| country.presentation.flag.as_deref());
        }
        #[cfg(not(feature = "ares"))]
        {
            let _ = id;
            None
        }
    }

    /// The insignia surface a Country selection draws, as `Game_GetFlagSurface
    /// @ 0x1001DCE0` (Ares patch site `0x4E3562`, 5 bytes) answers it: the
    /// country's own `File.Flag`, or the stock `rani.pcx` branch at `0x4E3567`
    /// when that file is cleared. `None` in a stock build, where each shell
    /// keeps its fixed country table.
    pub fn country_flag_surface(&self, id: &str) -> Option<String> {
        #[cfg(feature = "ares")]
        {
            return self.country_rules(id).map(|country| {
                country
                    .presentation
                    .flag
                    .clone()
                    .unwrap_or_else(|| STOCK_UNKNOWN_INSIGNIA.to_owned())
            });
        }
        #[cfg(not(feature = "ares"))]
        {
            let _ = id;
            None
        }
    }

    /// `Game_GetFlagSurface @ 0x1001DCE0` for a HouseType country index. An
    /// index outside the allocated array returns `None`, where the stock
    /// selection chain from `0x4E3579` still decides.
    pub fn country_insignia_pcx(&self, index: u16) -> Option<String> {
        let id = self.country_name(CountryIdx(index))?;
        self.country_flag_surface(id)
    }

    /// Ares `MenuText.Status=` CSF key (stock/unknown defaults included).
    pub fn country_status_key(&self, id: &str) -> Option<&str> {
        #[cfg(feature = "ares")]
        {
            return self
                .country_rules(id)
                .and_then(|country| country.presentation.status.as_deref());
        }
        #[cfg(not(feature = "ares"))]
        {
            let _ = id;
            None
        }
    }

    /// Ares `File.Taunt=` pattern; `~~` or the stock `%02i` receives the
    /// taunt number elsewhere. None in a stock build.
    pub fn country_taunt_pattern(&self, id: &str) -> Option<&str> {
        #[cfg(feature = "ares")]
        {
            return self
                .country_rules(id)
                .and_then(|country| country.presentation.taunt.as_deref());
        }
        #[cfg(not(feature = "ares"))]
        {
            let _ = id;
            None
        }
    }

    /// `File.Taunt=` for a HouseType country index (the taunt byte's field).
    pub fn country_taunt_pattern_by_index(&self, index: u16) -> Option<&str> {
        let id = self.country_name(CountryIdx(index))?;
        self.country_taunt_pattern(id)
    }

    /// Ares `ListIndex=`; negative hides the country. A stock build returns
    /// the common 100, preserving registry order.
    pub fn country_list_index(&self, id: &str) -> i32 {
        #[cfg(feature = "ares")]
        {
            return self
                .country_rules(id)
                .map_or(100, |country| country.presentation.list_index);
        }
        #[cfg(not(feature = "ares"))]
        {
            let _ = id;
            100
        }
    }

    /// Ares `RandomSelectionWeight=`, default 1.
    pub fn country_random_weight(&self, id: &str) -> i32 {
        #[cfg(feature = "ares")]
        {
            return self
                .country_rules(id)
                .map_or(1, |country| country.presentation.random_weight);
        }
        #[cfg(not(feature = "ares"))]
        {
            let _ = id;
            1
        }
    }

    /// Ares `AIRandomSelectionWeight=` once set, else the human weight
    /// (`0x1001CA11..0x1001CA1A`).
    pub fn country_ai_random_weight(&self, id: &str) -> i32 {
        #[cfg(feature = "ares")]
        {
            return self
                .country_rules(id)
                .map_or(1, |country| country.presentation.ai_weight());
        }
        #[cfg(not(feature = "ares"))]
        {
            self.country_random_weight(id)
        }
    }

    /// Ares `HouseTypeExt::GetPowerplants @ 0x1001BD20`, as BuildingType
    /// indices: country `id`'s `AI.PowerPlants=` (`+0x130`) while it is
    /// non-empty, else by the country's side (`HouseType+0xBC`): 0
    /// `[GDIPowerPlant]`, 1 `[NodRegularPower, NodAdvancedPower]`
    /// (`0x1001BD6C..0x1001BD8C`), 2 `[ThirdPowerPlant]`, each without its
    /// unset slots, and any other side none.
    #[cfg(feature = "ares")]
    pub fn ares_power_plants(&self, id: &str, side: i32) -> Vec<i32> {
        let index = |name: &Option<String>| {
            name.as_deref()
                .and_then(|name| self.building_type_index(name))
        };
        let country = self
            .country_rules(id)
            .map_or(&[][..], |country| country.power_plants.as_slice());
        if !country.is_empty() {
            return country
                .iter()
                .filter_map(|name| self.building_type_index(name))
                .collect();
        }
        let plants = &self.general.building_types;
        let slots = match side {
            0 => [&plants.gdi_power_plant, &None],
            1 => [&plants.nod_regular_power, &plants.nod_advanced_power],
            2 => [&plants.third_power_plant, &None],
            _ => [&None, &None],
        };
        slots.into_iter().filter_map(index).collect()
    }

    /// `HouseTypeExt::GetParadropContent @ 0x1001BC20` for a country: its own
    /// `ParaDrop.Types`/`.Num` while the type list is non-empty, else `None`,
    /// which defers to the side layer (also the answer in a stock build).
    pub fn country_paradrop_lists(&self, id: &str) -> Option<(&[String], &[i32])> {
        #[cfg(feature = "ares")]
        {
            return self.country_rules(id)?.paradrop.lists();
        }
        #[cfg(not(feature = "ares"))]
        {
            let _ = id;
            None
        }
    }

    /// Ares `[Country] ParaDrop.Aircraft=` (`HouseTypeExt +0x154`), which
    /// `GetParadropPlane @ 0x1001BCB0` prefers over the side's plane. A
    /// never-set or `none` index is `-1` there, so this answers `None` and the
    /// caller keeps the side layer.
    pub fn country_paradrop_aircraft(&self, id: &str) -> Option<&str> {
        #[cfg(feature = "ares")]
        {
            return self.country_rules(id)?.paradrop.aircraft.as_deref();
        }
        #[cfg(not(feature = "ares"))]
        {
            let _ = id;
            None
        }
    }

    /// Ares `[Country] Parachute.Anim=` (`HouseTypeExt +0x158`), which
    /// `GetParachuteAnim @ 0x1001BBC0` prefers over the side's canopy and the
    /// rules default. The canopy hook reaches it only when the type names no
    /// parachute of its own.
    pub fn country_parachute_anim(&self, id: &str) -> Option<&str> {
        #[cfg(feature = "ares")]
        {
            return self.country_rules(id)?.paradrop.parachute_anim.as_deref();
        }
        #[cfg(not(feature = "ares"))]
        {
            let _ = id;
            None
        }
    }

    /// Ares `File.ObserverBackground=` (PCX or SHP), when retained.
    #[cfg(feature = "ares")]
    pub fn country_observer_background(&self, id: &str) -> Option<&str> {
        self.country_rules(id)?
            .presentation
            .observer_background
            .as_deref()
    }

    /// Ares `File.ObserverFlag=` (PCX or SHP), when retained.
    #[cfg(feature = "ares")]
    pub fn country_observer_flag(&self, id: &str) -> Option<&str> {
        self.country_rules(id)?
            .presentation
            .observer_flag
            .as_deref()
    }

    /// Ares `File.ObserverFlagAltPalette=`.
    #[cfg(feature = "ares")]
    pub fn country_observer_flag_alt_palette(&self, id: &str) -> bool {
        self.country_rules(id)
            .is_some_and(|country| country.presentation.observer_flag_alt_palette)
    }

    /// Ares capture1004524F..10045275 reads CountryExt+1BC when present,
    /// otherwise the live HouseType+1A6 MultiplayPassive byte.
    #[cfg(feature = "ares")]
    pub(crate) fn country_can_be_driven(&self, id: &str) -> bool {
        self.country_rules(id).is_some_and(|country| {
            country
                .veteran_types
                .can_be_driven_override()
                .unwrap_or(country.multiplay_passive)
        })
    }

    /// Victim CountryExt+1D1 gate in Ares bounty payout10043279..10043289.
    #[cfg(feature = "ares")]
    pub(crate) fn country_gives_bounty(&self, id: &str) -> bool {
        self.country_rules(id)
            .is_some_and(|country| country.veteran_types.gives_bounty())
    }

    /// HouseType511850 scalar fields used by SetDifficulty4F6EC0. Unknown
    /// countries retain its constructor's neutral doubles. Category cost,
    /// speed and armor getters use their own independent fields.
    pub(crate) fn country_difficulty_biases(&self, id: &str) -> CountryDifficultyBiases {
        self.country_rules(id)
            .map_or_else(CountryDifficultyBiases::default, |country| {
                let bits = |value: f64| NativeF64Bits::from_bits(value.to_bits());
                CountryDifficultyBiases {
                    firepower: bits(country.firepower),
                    ground_speed: bits(country.ground_speed),
                    air_speed: bits(country.air_speed),
                    armor: bits(country.armor),
                    rof: bits(country.rof),
                    cost: bits(country.cost),
                    build_time: bits(country.build_time),
                }
            })
    }

    /// Whether a country/house type may claim nearby map walls. Native default is true.
    pub fn country_wall_owner(&self, id: &str) -> bool {
        self.country_rules(id)
            .is_none_or(|country| country.wall_owner)
    }

    /// HouseType+148 income float; an unknown country has the neutral default.
    pub fn country_income_mult(&self, id: &str) -> NativeF32Bits {
        self.country_rules(id)
            .map_or(NativeF32Bits::ONE, |country| country.income_mult)
    }

    /// A country's `Cost*Mult=` floats (`HouseType+0x114..+0x124`), the
    /// constructor's 1.0 for an unknown country.
    pub(crate) fn country_cost_mults(&self, id: &str) -> [NativeF32Bits; 5] {
        self.country_rules(id)
            .map_or([NativeF32Bits::ONE; 5], |country| country.cost_mults)
    }

    /// A country's `Speed*Mult=` floats (`HouseType+0x128..+0x130`), the
    /// constructor's 1.0 for an unknown country.
    pub fn country_speed_mults(&self, id: &str) -> [NativeF32Bits; 3] {
        self.country_rules(id)
            .map_or([NativeF32Bits::ONE; 3], |country| country.speed_mults)
    }

    /// `HouseClass::GetArmorMultForType @ 0x0050BD30` for a house of country
    /// `id`: its per-category float for `object` (a `BuildCat=Combat`
    /// building takes `ArmorDefensesMult=`), 1.0 for an unknown country.
    pub(crate) fn country_armor_mult_for_type(&self, id: &str, object: &ObjectType) -> f32 {
        let Some(country) = self.country_rules(id) else {
            return 1.0;
        };
        match object.category {
            ObjectCategory::Infantry => country.armor_infantry_mult,
            ObjectCategory::Vehicle => country.armor_units_mult,
            ObjectCategory::Aircraft => country.armor_aircraft_mult,
            ObjectCategory::Building if object.build_cat == Some(BuildCategory::Combat) => {
                country.armor_defenses_mult
            }
            ObjectCategory::Building => country.armor_buildings_mult,
        }
    }

    /// `HouseClass @ 0x0050C0A0` for a house of country `id`: its
    /// `BuildTime*Mult=` for `object`'s class (a `BuildCat=Combat` building
    /// takes the defenses one), 1.0 for an unknown country.
    pub(crate) fn country_build_time_mult_for_type(
        &self,
        id: &str,
        object: &ObjectType,
    ) -> NativeF32Bits {
        let Some(country) = self.country_rules(id) else {
            return NativeF32Bits::ONE;
        };
        country.build_time_mults[object.factor_slot()]
    }

    /// Native Country+98 as retained by511850. An unread registered Country
    /// has an empty parent; a missing key in a reached section defaults to its ID.
    pub fn country_parent_name(&self, country: &str) -> Option<&str> {
        self.country_rules(country)
            .map(|fields| fields.parent_country.as_str())
    }

    /// Owner-side bit used by FirstOwnable505316, FirstBuildable5051E9,
    /// Recalc5054E2 and the AI economy/defense choosers. Reuse the native
    /// alias-before-ID5117D0 scan: <random> is -2/bit30, unknown is -1/bit31.
    /// RequiredHouses/ForbiddenHouses instead use the actual Country index.
    pub fn country_owner_mask(&self, country: &str) -> u32 {
        let parent = self.country_parent_name(country).unwrap_or(country);
        let index = house_type_index_of_name(parent, self.country_lookup_members());
        1u32.wrapping_shl(index as u32)
    }

    /// The token/OR tail of INI ReadOwners4750D0 through48DEB0. Only -1
    /// is rejected; <random> (-2) contributes bit30. Trigger's <none> ->0
    /// override does not apply. The production list reader owns capacity
    /// and tokenization; the retained Country registry owns name lookup.
    /// Native controls: tools/rules_oracle/session_country_starting_ownership.json
    /// and tools/rules_oracle/session_country_parent.json.
    pub fn house_type_mask(&self, tokens: &[String]) -> u32 {
        crate::rules::native_processing::house_type_mask_from_indices(
            tokens
                .iter()
                .map(|token| house_type_index_of_name(token, self.country_lookup_members())),
        )
    }

    /// Resolve a country name to its stable `[Countries]` registration index.
    pub fn country_index(&self, id: &str) -> Option<CountryIdx> {
        self.country_indices.get(&id.to_ascii_uppercase()).copied()
    }

    /// Installed Ares1001BA90 shared Find: first stored-ID match, sentinel
    /// rejection and no alias/ParentCountry expansion. Callers own self fallback.
    /// Native controls: rules_oracle/ares_country_veterancy.json parent_find.
    pub fn ares_find_country(&self, token: &str) -> Option<CountryIdx> {
        crate::rules::native_processing::ares_country_index_of_name(
            token,
            self.country_ids.iter().map(String::as_str),
        )
    }

    /// HouseRead500A05 calls ReadHouseType475540: an absent Country maps
    /// caller-1 to entry0; known tokens use the shared alias-before-ID5117D0
    /// lookup. Unknown nonempty tokens need the native factory branch and
    /// return None here, never an invented entry0 binding.
    pub(crate) fn scenario_country_name(&self, country: Option<&str>) -> Option<&str> {
        let index = match country {
            Some(name) => {
                let index = house_type_index_of_name(name, self.country_lookup_members());
                if index < 0 {
                    return None;
                }
                CountryIdx(u16::try_from(index).expect("[Countries] exceeds u16 identity space"))
            }
            None => CountryIdx(0),
        };
        self.country_name(index)
    }

    /// Cooperative5C34F0/5C35F0 draw over the complete retained HouseType
    /// registry, then49D9F0 matches ID/Name. Passive and non-playable entries
    /// remain rejection candidates and therefore affect Scenario RNG order.
    pub fn cooperative_country_candidates(&self) -> impl Iterator<Item = (&str, Option<&str>)> {
        self.country_lookup_members()
    }

    fn country_lookup_members(&self) -> impl Iterator<Item = (&str, Option<&str>)> {
        self.country_ids.iter().map(|id| {
            (
                id.as_str(),
                self.countries
                    .get(id)
                    .and_then(|country| country.name.as_deref()),
            )
        })
    }

    /// House ctor4F5E5A copies Country+C0 to its current color. A registered
    /// Country whose entire section was absent retains the ctor's0.
    pub fn country_color_scheme(&self, country: &str) -> i32 {
        self.country_rules(country)
            .map_or(0, |fields| fields.color_scheme)
    }

    /// Resolve a trigger owner token to the canonical HouseType registration.
    ///
    /// gamemd-derived: `TriggerTypeClass::Read` resolves token 1 through
    /// `HouseTypeClass__FindIndexOfName @ 0x005117D0`. Before that scan,
    /// ReadINI727292..7272AA compares literal817474 (`<none>`) and selects
    /// Country zero. This is a TriggerType caller rule, not lookup behavior.
    /// The source-order scan
    /// checks each HouseType's `Name=` alias (`+0x64`) before its registry ID
    /// (`+0x24`). The shared owner retains `<random>`=-2 and unknown=-1;
    /// this typed projection returns only registered Country identities.
    /// Original caller controls: tools/input_oracle/campaign_start.py
    /// --houses, side_controls.trigger_type_reader_rows (+meta).
    pub fn trigger_house_type_index(&self, owner: &str) -> Option<CountryIdx> {
        if owner.eq_ignore_ascii_case("<none>") {
            return self.country_name(CountryIdx(0)).map(|_| CountryIdx(0));
        }
        let index = house_type_index_of_name(owner, self.country_lookup_members());
        (index >= 0).then(|| {
            CountryIdx(u16::try_from(index).expect("[Countries] exceeds u16 identity space"))
        })
    }

    /// Resolve a country index back to its source spelling.
    pub fn country_name(&self, index: CountryIdx) -> Option<&str> {
        self.country_ids.get(index.0 as usize).map(String::as_str)
    }

    /// Resolve a side name to its stable `[Sides]` registration index.
    pub fn side_index(&self, id: &str) -> Option<SideIdx> {
        self.side_indices.get(&id.to_ascii_uppercase()).copied()
    }

    /// Phobos `WarheadTypeExt::AllowDamageOnSelf.Get([General] ...)`: the
    /// warhead's own nullable value, then the global, defaulting to false.
    #[cfg(feature = "phobos")]
    pub fn allow_damage_on_self_for(&self, warhead: &WarheadType) -> bool {
        warhead
            .allow_damage_on_self()
            .unwrap_or(self.general.allow_damage_on_self)
    }

    /// Phobos is compiled out: the native `DamageSelf` gate stays final.
    #[cfg(not(feature = "phobos"))]
    pub fn allow_damage_on_self_for(&self, _warhead: &WarheadType) -> bool {
        false
    }

    /// Phobos `WarheadTypeExt::DecloakDamagedTargets.Get([General] ...)`: the
    /// warhead's own nullable value, then the global, defaulting to true.
    #[cfg(feature = "phobos")]
    pub fn decloak_damaged_targets_for(&self, warhead: &WarheadType) -> bool {
        warhead
            .decloak_damaged_targets()
            .unwrap_or(self.general.decloak_damaged_targets)
    }

    /// Phobos `InfantryAutoDeploy`, resolved the way `InfantryClass::
    /// Approach_Target`'s hook resolves it: the TechnoType's own value, then
    /// `[General]`, defaulting to `false` (the retail YR behaviour). `true`
    /// re-enables the deploy arm at original `0x00522434`.
    #[cfg(feature = "phobos")]
    pub fn infantry_auto_deploy_for(&self, ty: &ObjectType) -> bool {
        ty.infantry_auto_deploy()
            .unwrap_or(self.general.infantry_auto_deploy)
    }

    /// Phobos `ProneSpeed`, resolved the way the hook at original `0x00521D94`
    /// resolves it: the InfantryType's own value, then the global that the
    /// type's `Crawls=` selects.
    #[cfg(feature = "phobos")]
    pub fn prone_speed_for(&self, ty: &ObjectType) -> f64 {
        ty.prone_speed().unwrap_or(if ty.crawls {
            self.general.prone_speed_crawls
        } else {
            self.general.prone_speed_no_crawls
        })
    }

    /// Phobos `MindControl.IgnoreSize`: the type's own nullable, then
    /// `[General]`, constructor default true.
    #[cfg(feature = "phobos")]
    pub fn mind_control_ignore_size_for(&self, ty: &ObjectType) -> bool {
        ty.mind_control_ignore_size
            .unwrap_or(self.mind_control.mind_control_ignore_size)
    }

    /// Phobos `MultiMindControl.ReleaseVictim`: the type's own nullable, then
    /// `[General]`, constructor default false.
    #[cfg(feature = "phobos")]
    pub fn multi_mind_control_release_victim_for(&self, ty: &ObjectType) -> bool {
        ty.multi_mind_control_release_victim
            .unwrap_or(self.mind_control.multi_mind_control_release_victim)
    }

    /// Phobos `MindControlLink.VisibleToHouse`: the type's own nullable, then
    /// `[General]`, constructor default All.
    #[cfg(feature = "phobos")]
    pub fn mind_control_link_visible_to_house_for(
        &self,
        ty: &ObjectType,
    ) -> crate::rules::phobos_shields::AffectedHouse {
        ty.mind_control_link_visible_to_house
            .unwrap_or(self.mind_control.mind_control_link_visible_to_house)
    }

    /// Type-select admission for a structure.
    ///
    /// The native ID match (`0x00732C30`) admits a building of a listed type
    /// when CanBeSelectedNow (vt+0x13C) answers true or its type has
    /// `UndeploysInto=` (`0x00732C61..0x00732C83`). A building's
    /// CanBeSelectedNow (`0x00459C00`) first asks `0x00465D40`, true only for
    /// a 1x1 `UndeploysInto=` type, so retail admits exactly the
    /// `UndeploysInto=` buildings. Phobos's hook makes `0x00465D40` answer true
    /// during the ID match under `BuildingTypeSelectable`, with Ares loaded.
    /// RESIDUAL: with the switch on, native still applies CanBeSelectedNow's
    /// TechnoClass checks (`0x006FC030`); this admits every building of the
    /// type.
    pub fn structure_type_selectable(&self, id: &str) -> bool {
        #[cfg(all(feature = "phobos", feature = "ares"))]
        {
            if self.general.building_type_selectable {
                return true;
            }
        }
        self.object(id)
            .is_some_and(|object| object.undeploys_into.is_some())
    }

    /// Phobos `[AudioVisual] MissingCameo=`: the file a cameo with no art
    /// falls back to.
    #[cfg(feature = "phobos")]
    pub fn missing_cameo(&self) -> &str {
        &self.general.missing_cameo
    }

    /// Phobos `Sidebar.GDIPositions=` for a native side index. A missing or
    /// out-of-range side falls back to the first-side default (index 0 true).
    #[cfg(feature = "phobos")]
    pub fn side_gdi_positions(&self, index: i32) -> bool {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.side_gdi_positions.get(index).copied())
            .unwrap_or(index == 0)
    }

    /// Phobos `FlyNoWobbles`, resolved the way the hook at original `0x4CF8B1`
    /// resolves it: the TechnoType's own value, then `[AudioVisual]`, then the
    /// retail rule that branch reads in an unmodified game (`IsDropship`).
    /// `true` skips the `FlyLocomotionClass::Draw_Point` wobble.
    #[cfg(feature = "phobos")]
    pub fn fly_no_wobbles_for(&self, ty: &ObjectType) -> bool {
        crate::rules::phobos_fly_wobble::resolve(
            ty.fly_no_wobbles(),
            self.fly_no_wobbles,
            ty.is_dropship(),
        )
    }

    /// Phobos `[SIDE] SuperWeaponSidebar.*PCX=` for a native side index.
    /// Missing sides have no artwork, which is valid.
    #[cfg(feature = "phobos")]
    pub fn side_sw_sidebar_art(
        &self,
        index: i32,
    ) -> Option<&crate::rules::phobos_sw_sidebar::SuperWeaponSidebarSideArt> {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.side_sw_sidebar_art.get(index))
    }

    /// Resolve a side index back to its source spelling.
    pub fn side_name(&self, index: SideIdx) -> Option<&str> {
        self.side_ids.get(index.0 as usize).map(String::as_str)
    }

    /// The already-allocated Side array, including constructor-only entries;
    /// querying it never rereads an INI or creates a missing side.
    pub fn allocated_sides(&self) -> impl Iterator<Item = (SideIdx, &str)> {
        self.side_ids.iter().enumerate().map(|(index, id)| {
            (
                SideIdx(u8::try_from(index).expect("[Sides] exceeds u8 identity space")),
                id.as_str(),
            )
        })
    }

    /// Signed SideExt+88. Zero and negative values are intentional native
    /// selector inputs, not missing values or an instruction to clamp to one.
    /// An ordinary retail build has no SideExt configuration.
    pub fn side_sidebar_mix_file_index(&self, index: SideIdx) -> Option<i32> {
        #[cfg(feature = "ares")]
        return self
            .side_extensions
            .get(usize::from(index.0))
            .map(|extension| extension.sidebar_mix_file_index);
        #[cfg(not(feature = "ares"))]
        {
            let _ = index;
            None
        }
    }

    /// SideExt+8C selects Yuri-specific sidebar filenames independently of
    /// the Side array index and Sidebar.MixFileIndex.
    pub fn side_sidebar_yuri_file_names(&self, index: SideIdx) -> Option<bool> {
        #[cfg(feature = "ares")]
        return self
            .side_extensions
            .get(usize::from(index.0))
            .map(|extension| extension.sidebar_yuri_file_names);
        #[cfg(not(feature = "ares"))]
        {
            let _ = index;
            None
        }
    }

    /// Signed SideExt+90 after reached EVA.Tag reads. Binding uses saved
    /// Scenario.Side and preserves an out-of-catalog value; filename selection
    /// decides whether that value has a column. Native: Ares1002FB00.
    #[cfg(feature = "ares")]
    pub fn side_eva_voice_index(&self, side: i32) -> Option<i32> {
        self.side_extension(side)
            .map(|extension| extension.eva_voice_index)
    }

    #[cfg(feature = "ares")]
    fn side_extension(
        &self,
        side: i32,
    ) -> Option<&crate::rules::ares_side_extension::AresSideExtension> {
        self.side_extensions.get(usize::try_from(side).ok()?)
    }

    /// Ares Side default fallbacks use the side's array index; a fourth or
    /// later side takes the Allied branch (3.0 getters at 0x1002DE60,
    /// 0x1002DEA0, 0x1002E130, 0x1002DDD0, 0x1002DD40, 0x1002DFB0,
    /// 0x1002E060). `None` means the installed build has no SideExt for this
    /// index, so the caller keeps its stock path.
    #[cfg(feature = "ares")]
    fn ares_default_side_index(side: i32) -> usize {
        match side {
            1 => 1,
            2 => 2,
            _ => 0,
        }
    }

    /// `[Side] Crew=` else `AlliedCrew`/`SovietCrew`/`ThirdCrew` by array
    /// index. `Some(None)` is an explicit `none` (no crew), distinct from an
    /// unset key (caller falls back as native `SideExt::GetCrew @ 0x1002DE60`).
    pub(crate) fn side_crew(&self, side: i32) -> Option<Option<&str>> {
        #[cfg(feature = "ares")]
        {
            let extension = self.side_extension(side)?;
            return Some(match &extension.crew {
                Some(value) => value.as_deref(),
                None => match side {
                    1 => self.general.soviet_crew.as_deref(),
                    2 => self.general.third_crew.as_deref(),
                    _ => self.general.allied_crew.as_deref(),
                },
            });
        }
        #[cfg(not(feature = "ares"))]
        {
            let _ = side;
            None
        }
    }

    /// `[Side] Engineer=` else global `[General] Engineer=`
    /// (`SideExt::GetEngineer @ 0x1002DEE0`).
    #[cfg(feature = "ares")]
    pub(crate) fn side_engineer(&self, side: i32) -> Option<Option<&str>> {
        let extension = self.side_extension(side)?;
        Some(match &extension.engineer {
            Some(value) => value.as_deref(),
            None => self.general.engineer_infantry.as_deref(),
        })
    }

    /// `[Side] Technician=` else global `[General] Technician=`
    /// (`SideExt::GetTechnician @ 0x1002E170`).
    pub(crate) fn side_technician(&self, side: i32) -> Option<Option<&str>> {
        #[cfg(feature = "ares")]
        {
            let extension = self.side_extension(side)?;
            return Some(match &extension.technician {
                Some(value) => value.as_deref(),
                None => self.general.technician.as_deref(),
            });
        }
        #[cfg(not(feature = "ares"))]
        {
            let _ = side;
            None
        }
    }

    /// `[Side] DefaultDisguise=` else the stock side disguise by array index
    /// (`SideExt::GetDisguise @ 0x1002DEA0`).
    pub(crate) fn side_disguise(&self, side: i32) -> Option<Option<&str>> {
        #[cfg(feature = "ares")]
        {
            let extension = self.side_extension(side)?;
            return Some(match &extension.default_disguise {
                Some(value) => value.as_deref(),
                None => {
                    self.general.infantry_disguises[Self::ares_default_side_index(side)].as_deref()
                }
            });
        }
        #[cfg(not(feature = "ares"))]
        {
            let _ = side;
            None
        }
    }

    /// `[Side] SurvivorDivisor=` else `Allied/Soviet/ThirdSurvivorDivisor`
    /// (`SideExt::GetSurvivorDivisor @ 0x1002E130`).
    pub(crate) fn side_survivor_divisor(&self, side: i32) -> Option<i32> {
        #[cfg(feature = "ares")]
        {
            let extension = self.side_extension(side)?;
            return Some(extension.survivor_divisor.unwrap_or(match side {
                1 => self.general.soviet_survivor_divisor,
                2 => self.general.third_survivor_divisor,
                _ => self.general.allied_survivor_divisor,
            }));
        }
        #[cfg(not(feature = "ares"))]
        {
            let _ = side;
            None
        }
    }

    /// `[Side] AI.BaseDefenses=` else the side-index list; a fourth or later
    /// side takes the Allied list (`SideExt::GetBaseDefenses @ 0x1002DDD0`).
    #[cfg(feature = "ares")]
    pub(crate) fn side_base_defenses(&self, side: i32) -> Option<&[String]> {
        let extension = self.side_extension(side)?;
        Some(
            extension
                .base_defenses
                .as_deref()
                .unwrap_or_else(|| match side {
                    1 => &self.soviet_base_defense_types,
                    2 => &self.third_base_defense_types,
                    _ => &self.allied_base_defense_types,
                }),
        )
    }

    /// `[Side] AI.BaseDefenseCounts=` else the side-index difficulty vector;
    /// a fourth or later side takes the Allied vector
    /// (`SideExt::GetBaseDefenseCounts @ 0x1002DD40`).
    pub(crate) fn side_base_defense_counts(&self, side: i32) -> Option<&[i32]> {
        #[cfg(feature = "ares")]
        {
            let extension = self.side_extension(side)?;
            return Some(
                extension
                    .base_defense_counts
                    .as_deref()
                    .unwrap_or_else(|| match side {
                        1 => &self.soviet_base_defense_counts,
                        2 => &self.third_base_defense_counts,
                        _ => &self.allied_base_defense_counts,
                    }),
            );
        }
        #[cfg(not(feature = "ares"))]
        {
            let _ = side;
            None
        }
    }

    /// `[Side] ParaDrop.Types=`/`ParaDrop.Num=` only when both are set;
    /// otherwise the side-index lists, with a fourth or later side taking the
    /// Allied pair (`SideExt::GetParaDropTypes @ 0x1002E060`,
    /// `GetParaDropNum @ 0x1002DFB0`).
    pub(crate) fn side_paradrop(&self, side: i32) -> Option<(&[String], &[i32])> {
        #[cfg(feature = "ares")]
        {
            let extension = self.side_extension(side)?;
            if let (Some(types), Some(counts)) =
                (&extension.paradrop_types, &extension.paradrop_num)
            {
                return Some((types, counts));
            }
            let lists = match side {
                1 => &self.general.sov_paradrop,
                2 => &self.general.yuri_paradrop,
                _ => &self.general.ally_paradrop,
            };
            return Some((lists.infantry.as_slice(), lists.counts.as_slice()));
        }
        #[cfg(not(feature = "ares"))]
        {
            let _ = side;
            None
        }
    }

    /// `[Side] ParaDrop.Aircraft=` else the plane the SideExt initializer
    /// resolved from `PDPLANE` (`+0x74`). `None` for no SideExt or the -1
    /// index (`none`, or PDPLANE unregistered at initialization), where
    /// `HouseTypeExt::GetParadropPlane @ 0x1001BCB0` falls back to
    /// `FindIndex("PDPLANE")` rather than cancelling the drop.
    pub(crate) fn side_paradrop_aircraft(&self, side: i32) -> Option<&str> {
        #[cfg(feature = "ares")]
        {
            return self
                .side_extension(side)?
                .paradrop_aircraft
                .as_ref()?
                .as_deref();
        }
        #[cfg(not(feature = "ares"))]
        {
            let _ = side;
            None
        }
    }

    /// `[Side] Parachute.Anim=` only; an unset key leaves the existing
    /// `[General] Parachute=`/NPSE `VehicleParachute=` owner in charge.
    /// `Some(None)` is an explicit `none`, which the canopy consumer resolves
    /// to `PARACH` (`HouseTypeExt::GetParachuteAnim @ 0x1001BBC0`).
    pub(crate) fn side_parachute_override(&self, side: i32) -> Option<Option<&str>> {
        #[cfg(feature = "ares")]
        {
            let extension = self.side_extension(side)?;
            return extension
                .parachute_anim
                .as_ref()
                .map(|name| name.as_deref());
        }
        #[cfg(not(feature = "ares"))]
        {
            let _ = side;
            None
        }
    }

    /// `[Side] ToolTipColor=` as RGB (initialized 255,255,0 for Nod/ThirdSide,
    /// 164,210,255 otherwise; `SideExt+80`).
    pub fn side_tooltip_color(&self, side: i32) -> Option<[u8; 3]> {
        #[cfg(feature = "ares")]
        {
            return self
                .side_extension(side)
                .map(|extension| extension.tooltip_color);
        }
        #[cfg(not(feature = "ares"))]
        {
            let _ = side;
            None
        }
    }

    /// `[Side] MessageTextColor=` as the runtime ColorScheme index
    /// (initialized 11 for Nod, 25 for ThirdSide, 21 otherwise) or -1 for an
    /// explicit `none` (`SideExt+84`).
    pub fn side_message_text_color_index(&self, side: i32) -> Option<i32> {
        #[cfg(feature = "ares")]
        {
            return self
                .side_extension(side)
                .map(|extension| extension.message_text_color_index);
        }
        #[cfg(not(feature = "ares"))]
        {
            let _ = side;
            None
        }
    }

    /// `[Side] HunterSeeker=` (SideExt+0x94), the fallback UnitType of
    /// SW_HunterSeeker::Activate10072885.
    #[cfg(feature = "ares")]
    pub(crate) fn side_hunter_seeker(&self, side: i32) -> Option<&str> {
        self.side_extension(side)?
            .hunter_seeker
            .as_ref()?
            .as_deref()
    }

    /// Every distinct `[Side] Parachute.Anim=` identity, for the animation
    /// asset preload and canopy owner scan.
    #[cfg(feature = "ares")]
    pub fn side_parachute_animations(&self) -> impl Iterator<Item = &str> {
        self.side_extensions
            .iter()
            .filter_map(|extension| extension.parachute_anim.as_ref()?.as_deref())
    }

    /// `[Side] GraphicalText.Image=` (SideExt `+0x1F8`) and
    /// `[Side] GraphicalText.Palette=` (`+0x218`).
    /// `Game_LoadUI_LoadSideData` (`0x1002F4A0`) loads each into its own
    /// tactical-overlay slot, and `TacticalClass_DrawOverlay_GraphicalText`
    /// (`0x1002FAD0`, patch site `0x6D4E79`) reads them through `0x1002DF10` /
    /// `0x1002DF00`, which fall back to InitGame's literal `GRFXTXT` files per
    /// slot. `None` when the installed build has no SideExt for this index.
    #[cfg(feature = "ares")]
    pub fn side_graphical_text(&self, side: i32) -> Option<(Option<&str>, Option<&str>)> {
        let extension = self.side_extension(side)?;
        Some((
            extension.graphical_text_image.as_deref(),
            extension.graphical_text_palette.as_deref(),
        ))
    }

    /// Return the country's effective side after its optional `Side=`
    /// override has superseded `[Sides]` membership.
    pub fn country_side_index(&self, id: &str) -> Option<SideIdx> {
        let country = self.country_index(id)?;
        self.country_sides
            .get(country.0 as usize)
            .copied()
            .flatten()
    }

    /// Country+BC is signed: an allocated constructor retains-1 until a
    /// reached Sides/Country reader writes it (511416,672597,51208C).
    /// None means unknown Country; Some(-1) is a known unbound Country.
    pub fn country_side_index_raw(&self, id: &str) -> Option<i32> {
        let country = self.country_index(id)?;
        self.country_sides
            .get(usize::from(country.0))
            .map(|side| side.map_or(-1, |index| i32::from(index.0)))
    }

    /// The country's `UIName=` string-table key, then its plain `Name=`.
    /// Callers resolve the key through the CSF table; the plain name is the
    /// fallback when there is no key or the key is missing from the table.
    pub fn country_display_name_sources(&self, id: &str) -> (Option<&str>, Option<&str>) {
        match self.country_rules(id) {
            Some(rules) => (rules.ui_name.as_deref(), rules.name.as_deref()),
            None => (None, None),
        }
    }

    /// Case-insensitive country lookup (gamemd parity), exact key first.
    fn country_rules(&self, id: &str) -> Option<&CountryRules> {
        let index = self.country_index(id)?;
        self.countries.get(self.country_name(index)?)
    }

    /// Whether `type_id` is in `[AI] BuildTech=` (`RulesClass+0x91C`, a
    /// vector of BuildingType pointers, so the match is by identity, here a
    /// case-insensitive name).
    pub fn is_build_tech(&self, type_id: &str) -> bool {
        self.build_tech_types
            .iter()
            .any(|tech| tech.eq_ignore_ascii_case(type_id))
    }

    /// Look up a superweapon type by ID (case-insensitive, gamemd parity).
    pub fn super_weapon(&self, id: &str) -> Option<&SuperWeaponType> {
        Self::lookup_ci(&self.super_weapons, id)
    }

    #[cfg(feature = "ares")]
    pub(crate) fn super_weapon_metadata(
        &self,
        id: &str,
    ) -> Option<&crate::rules::superweapon_type::AresSuperWeaponMetadata> {
        self.ares_super_weapon_metadata
            .get(&id.to_ascii_uppercase())
    }

    /// The Phobos techno gates for a superweapon, if the capability is
    /// compiled in and the type was registered.
    #[cfg(feature = "phobos")]
    pub(crate) fn phobos_super_weapon_metadata(
        &self,
        id: &str,
    ) -> Option<&crate::rules::superweapon_type::PhobosSuperWeaponAvailability> {
        self.phobos_super_weapon_metadata
            .get(&id.to_ascii_uppercase())
    }

    /// The `[GlobalControls]` parallel AI queue switches (Phobos extension).
    #[cfg(feature = "phobos")]
    pub fn parallel_ai_queues(
        &self,
    ) -> &crate::rules::phobos_parallel_ai_queues::ParallelAiQueueRules {
        &self.parallel_ai_queues
    }

    /// A superweapon type's `SuperWeaponTypeClass` array index (its
    /// `[SuperWeaponTypes]` position, case-insensitive), the value
    /// `SuperWeapon=` stores (BuildingType `+0x16F0`).
    pub fn super_weapon_index(&self, id: &str) -> Option<usize> {
        crate::rules::native_processing::super_weapon_index_of_name(
            id,
            self.super_weapon_order.iter().map(String::as_str),
        )
    }

    /// Look up a particle type by ID. Panics if `id` is out of range.
    pub fn particle_type(&self, id: ParticleTypeId) -> &ParticleType {
        &self.particle_types[id.0 as usize]
    }

    /// Iterate every parsed `[Particles]` definition.
    pub fn particle_types_iter(&self) -> impl Iterator<Item = &ParticleType> {
        self.particle_types.iter()
    }

    /// Look up a particle system type by ID. Panics if `id` is out of range.
    pub fn particle_system_type(&self, id: ParticleSystemTypeId) -> &ParticleSystemType {
        &self.particle_system_types[id.0 as usize]
    }

    /// Resolve an ordinal in the native ParticleSystemType registry A83D6C.
    pub(crate) fn particle_system_type_id_by_index(
        &self,
        index: i32,
    ) -> Option<ParticleSystemTypeId> {
        let index = usize::try_from(index).ok()?;
        self.particle_system_types
            .get(index)
            .map(|_| ParticleSystemTypeId(index as u32))
    }

    /// Resolve a particle type name to its ID (case-insensitive).
    pub fn p_type_id_by_name(&self, name: &str) -> Option<ParticleTypeId> {
        self.particle_types
            .iter()
            .position(|particle| particle.name.eq_ignore_ascii_case(name))
            .map(|index| ParticleTypeId(index as u32))
    }

    /// Resolve a particle system type name to its ID (case-insensitive).
    pub fn ps_type_id_by_name(&self, name: &str) -> Option<ParticleSystemTypeId> {
        self.particle_system_types_by_name
            .get(&name.to_ascii_uppercase())
            .copied()
    }

    /// Parsed VoxelAnim Types in their retained registry order.
    pub fn voxel_anim_types_iter(&self) -> impl Iterator<Item = &VoxelAnimType> {
        self.voxel_anim_types.iter()
    }

    /// Look up a `[VoxelAnims]` type by ID. Panics if `id` is out of range.
    pub fn voxel_anim_type(&self, id: VoxelAnimTypeId) -> &VoxelAnimType {
        &self.voxel_anim_types[id.0 as usize]
    }

    /// Resolve a `[VoxelAnims]` type name to its ID (case-insensitive).
    pub fn voxel_anim_type_id_by_name(&self, name: &str) -> Option<VoxelAnimTypeId> {
        self.voxel_anim_types_by_name
            .get(&name.to_ascii_uppercase())
            .copied()
    }

    /// Number of `[VoxelAnims]` types loaded, also used to validate saved IDs.
    pub fn voxel_anim_type_count(&self) -> usize {
        self.voxel_anim_types.len()
    }

    /// Number of particle types loaded from `[Particles]`.
    #[cfg(any(test, feature = "test-support"))]
    pub fn particle_type_count(&self) -> usize {
        self.particle_types.len()
    }

    /// Number of particle system types loaded from `[ParticleSystems]`.
    #[cfg(any(test, feature = "test-support"))]
    pub fn particle_system_type_count(&self) -> usize {
        self.particle_system_types.len()
    }

    /// Look up the factory type for a structure by ID (case-insensitive).
    /// Returns None if the structure has no Factory= key in rules.ini.
    pub fn factory_type(&self, structure_id: &str) -> Option<FactoryType> {
        self.factory_map
            .get(&structure_id.to_ascii_uppercase())
            .copied()
    }

    /// `group`'s `[General] Prerequisite*` list.
    pub(crate) fn prerequisite_list(&self, group: PrerequisiteGroup) -> &[Prerequisite] {
        &self.prerequisite_lists[group.index()]
    }

    pub(crate) fn ares_prerequisite_group(
        &self,
        entry: Prerequisite,
    ) -> Option<&GenericPrerequisite> {
        self.ares_prerequisites
            .as_ref()?
            .group(entry.group_index()?)
    }

    /// Ares TechnoTypeExt::IsGenericPrerequisite1003DD00 and its six Foot
    /// lifecycle callers1004E750..1004E8B9. For a normal fresh match Types
    /// finish loading before these checks. Retain an immutable O(1) membership
    /// projection, rather than scanning every group for each live object.
    /// Original cache/live-reread limits: rules_oracle/ares_prerequisites.md.
    pub(crate) fn is_ares_prerequisite_alternate(
        &self,
        category: ObjectCategory,
        id: &str,
    ) -> bool {
        self.object_category_index
            .get(&(category, id.to_ascii_uppercase()))
            .is_some_and(|handle| self.ares_prerequisite_alternates.contains(handle))
    }

    /// Resolve BuildingType FreeUnit, read by Grand_Opening446AA9. The native
    /// tail does not require Refinery; the type reader has already allocated
    /// a referenced UnitType, including names absent from VehicleTypes.
    pub fn building_free_unit(&self, structure_id: &str) -> Option<&str> {
        let obj = self.object(structure_id)?;
        let free_unit = obj.free_unit.as_deref()?;
        let resolved = self.object(free_unit)?;
        Some(resolved.id.as_str())
    }

    /// Whether a harvester type may dock at a specific structure according to Dock=.
    pub fn harvester_can_dock_at(&self, harvester_id: &str, structure_id: &str) -> bool {
        let Some(harvester) = self.object(harvester_id) else {
            return false;
        };
        let Some(_structure) = self.object(structure_id) else {
            return false;
        };
        harvester
            .dock
            .iter()
            .any(|dock| dock.eq_ignore_ascii_case(structure_id))
    }

    /// Bind ART metadata and project QueueingCell/DockingOffset and presentation.
    /// Building Foundation already came from the ordered native rules processor;
    /// binding assets cannot execute a ReadINI for a constructor-only type.
    pub fn install_art_data(&mut self, art: crate::rules::art_data::ArtRegistry) {
        self.art_registry = art;
        self.art_registry
            .apply_anim_type_read_states(&self.anim_type_art_read_states);
        self.art_registry
            .apply_anim_type_image_states(&self.anim_type_images);
        self.art_registry
            .apply_anim_type_references(&self.anim_type_references);
        self.project_art_data();
    }

    fn project_art_data(&mut self) {
        self.voxel_assets = Default::default();
        let art = &self.art_registry;
        // Projectile ART is already projected from the per-pass registry owner.
        // A final-image reread here would lose omission/cache/default semantics.
        let ai_base_spacing = self.ai_base_spacing;
        let mut dock_patched: u32 = 0;
        let mut buildings_checked: u32 = 0;
        let mut infantry_checked: u32 = 0;
        let mut crawls_patched: u32 = 0;
        for obj in self.object_list.iter_mut() {
            // Resolve the art.ini section: use Image= override if present,
            // otherwise fall back to the object ID itself.
            let art_key: &str = &obj.image;
            let entry = art.get(art_key).or_else(|| art.get(&obj.id));
            // Every TechnoType's ReadINI reads CanBeHidden from the ART
            // section of its own ID (0x007121EB); art is not layered.
            obj.can_be_hidden = art.can_be_hidden(&obj.id);
            if obj.category == crate::rules::object_type::ObjectCategory::Infantry {
                infantry_checked += 1;
                if let Some(entry) = entry {
                    obj.crawls = entry.crawls;
                    // Original ART5246BE..52473A owns four independent signed
                    // fields; project them without narrowing or fallback.
                    obj.fire_up_frame = entry.fire_up;
                    obj.fire_prone_frame = entry.fire_prone;
                    obj.secondary_fire_frame = entry.secondary_fire;
                    obj.secondary_prone_frame = entry.secondary_prone;
                    if entry.crawls {
                        crawls_patched += 1;
                    }
                }
                continue;
            }
            if obj.category != crate::rules::object_type::ObjectCategory::Building {
                continue;
            }
            buildings_checked += 1;
            obj.hidden_occupancy = art.building_hidden_occupancy_profile(&obj.id, art_key);
            // +EF0 and ToOverlay are supplied by the sole pass owner in
            // from_processed_rules. Repeating461225 here changes late
            // constructor-only types and loses retained state when
            // installation ART differs from the pass.
            if let Some(entry) = entry {
                // Merge QueueingCell from art.ini (TibSun legacy dock system).
                if entry.queueing_cell != [0, 0] {
                    obj.queueing_cell = entry.queueing_cell;
                    dock_patched += 1;
                }
                // Multi-pad merge: when art declares at least one DockingOffset,
                // size pads to NumberOfDocks (from rules.ini), zero-padding missing
                // indices and truncating excess. Mirrors the original game's
                // memory layout where the array is sized by NumberOfDocks and
                // unspecified DockingOffset%d slots default to (0,0,0).
                //
                // When art declares ZERO DockingOffset entries (retail refineries
                // like GAREFN/NAREFN/YAREFN), obj.pads is left empty so existing
                // fallback paths (e.g. refinery_pad_cell's rightmost-column
                // anchor) keep firing. Otherwise zero-padding would silently
                // shift refinery dock positions, which is out of scope here.
                if !entry.pads.is_empty() {
                    let n = obj.number_of_docks.max(0) as usize;
                    obj.pads = entry.pads.iter().take(n).copied().collect();
                    while obj.pads.len() < n {
                        obj.pads.push(crate::rules::object_type::DockPad {
                            lepton_offset: (0, 0, 0),
                        });
                    }
                }
            }
            // The native reveal-time gate sees the final loaded foundation.
            // Rules parsing runs before ART supplies stock Building foundations,
            // so overwrite the provisional profile after effective ART resolution.
            obj.base_reservation_spacing = obj
                .base_reservation_writer_eligible()
                .then_some(ai_base_spacing);
        }
        log::info!(
            "Merged art.ini → RuleSet: {} dock cells ({} buildings checked)",
            dock_patched,
            buildings_checked,
        );
        log::trace!(
            "Merged infantry art metadata: {} Crawls flags ({} infantry checked)",
            crawls_patched,
            infantry_checked,
        );
        self.rebuild_animation_sequences(None);
    }

    /// Read the same bound ART owner used during construction and gameplay.
    pub fn art(&self) -> &crate::rules::art_data::ArtRegistry {
        &self.art_registry
    }

    /// Registered AnimType identities selected by Phobos' gather overlay.
    /// Keep the type identity: its existing image owner resolves retained load
    /// requests, Image aliases and theater filenames.
    #[cfg(feature = "phobos")]
    pub fn harvest_overlay_anim_types(&self) -> Vec<&str> {
        let mut anims = Vec::new();
        for object in &self.object_list {
            if object.category != ObjectCategory::Vehicle {
                continue;
            }
            for anim in &object.ore_gathering().anims {
                if !anims.contains(&anim.as_str()) {
                    anims.push(anim.as_str());
                }
            }
        }
        anims
    }

    /// AnimTypeArray8B4154's retained constructor order, including types
    /// discovered before the master list and by later rules/ART readers.
    /// TAction41 indexes this registry at6E2357; `[Animations]` numeric
    /// labels and the membership-only BTreeSet are not ordinal catalogs.
    pub(crate) fn anim_type_name_by_index(&self, index: i32) -> Option<&str> {
        self.anim_type_art_read_states
            .get(usize::try_from(index).ok()?)
            .map(|(name, _)| name.as_str())
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn anim_type_art_read_states(&self) -> &[(String, bool)] {
        &self.anim_type_art_read_states
    }

    pub fn bind_scheduler_anim_assets(
        &mut self,
        roots: &[String],
        assets: &crate::assets::asset_manager::AssetManager,
        theater_ext: &str,
        theater_name: &str,
    ) -> Result<(), crate::rules::art_data::AnimAssetBindError> {
        self.art_registry
            .bind_scheduler_anim_assets(roots, assets, theater_ext, theater_name)
    }

    pub fn bind_anim_class_assets(
        &mut self,
        roots: &[String],
        assets: &crate::assets::asset_manager::AssetManager,
        theater_ext: &str,
        theater_name: &str,
    ) -> usize {
        self.art_registry
            .bind_anim_class_assets(roots, assets, theater_ext, theater_name)
    }

    pub fn populate_anim_frame_dims(
        &mut self,
        assets: &crate::assets::asset_manager::AssetManager,
        theater_ext: &str,
        theater_name: &str,
    ) -> (u32, u32) {
        self.art_registry
            .populate_anim_frame_dims(assets, theater_ext, theater_name)
    }

    /// Extend the bound closure before an authored-load constructor spends its
    /// native ID. The constructor then borrows this same owner for Reveal/Start
    /// (AnimClass421EA0, Unlimbo5F4EC0, Start424CE0).
    pub fn bind_authored_load_anim(
        &mut self,
        name: &str,
        assets: &crate::assets::asset_manager::AssetManager,
        theater_ext: &str,
        theater_name: &str,
    ) -> Result<(), crate::rules::art_data::AnimAssetBindError> {
        let mut roots: Vec<String> = self
            .art_registry
            .scheduler_anim_types()
            .iter()
            .cloned()
            .collect();
        roots.push(name.to_ascii_uppercase());
        roots.sort();
        roots.dedup();
        // Preserve the existing strict load failure boundary. RESIDUAL: a
        // previously tolerant root with a missing chained SHP can fail here.
        // This affects modded art; none of retail's nine chain keys starts
        // from a tolerant root. No new asset admission policy is introduced.
        self.bind_scheduler_anim_assets(&roots, assets, theater_ext, theater_name)
    }

    /// Synthetic fixtures that previously projected ART then restored the raw
    /// registry deliberately bypass native read admission. Move that fixture
    /// once while sharing the production projection, without clone publication.
    /// The AnimTypes' Type references still come from the ordered Rules
    /// processor, as in [`Self::install_art_data`].
    #[cfg(any(test, feature = "test-support"))]
    pub fn install_art_fixture(&mut self, art: crate::rules::art_data::ArtRegistry) {
        self.art_registry = art;
        self.art_registry
            .apply_anim_type_references(&self.anim_type_references);
        self.project_art_data();
    }

    /// Raw synthetic fixture replacement: no native admission or field projection.
    #[cfg(any(test, feature = "test-support"))]
    pub fn replace_art_registry_for_test(&mut self, art: crate::rules::art_data::ArtRegistry) {
        self.art_registry = art;
        self.voxel_assets = Default::default();
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn bind_anim_frame_count_for_test(&mut self, name: &str, raw_count: i32) {
        self.art_registry
            .bind_anim_frame_count_for_test(name, raw_count);
    }

    /// The SHP header height `+4` asset binding would read for `name`.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn bind_anim_shp_height_for_test(&mut self, name: &str, raw_height: i32) {
        self.art_registry
            .bind_anim_shp_height_for_test(name, raw_height);
    }

    /// A weapon's stored speed (after Process's postpass), as a native
    /// fixture writes it.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_weapon_speed_for_test(&mut self, id: &str, speed: i32) {
        if let Some(weapon) = self
            .weapons
            .values_mut()
            .find(|weapon| weapon.id.eq_ignore_ascii_case(id))
        {
            weapon.speed = speed;
        }
    }

    /// Supply original fixture vectors that the real sound reader cannot
    /// produce (notably a nonempty vector containing native sound ID -1,
    /// represented by an empty name). This is an explicit input boundary,
    /// not ReadSoundList or sound-catalog binding evidence.
    #[cfg(any(test, feature = "test-support"))]
    pub fn supply_selection_voice_vectors_for_test(
        &mut self,
        type_id: &str,
        normal: Vec<String>,
        enslaved: Vec<String>,
    ) {
        let handle = self
            .type_handle(type_id)
            .expect("native fixture type exists");
        let object = &mut self.object_list[handle.0 as usize];
        object.voice_select = normal;
        object.voice_select_enslaved = enslaved;
    }

    /// Apply a native post-reader TechnoType Size override to a test fixture.
    /// The fixture owns the exact binary64 value written to `Type+0x380`;
    /// this does not model INI parsing or change the production Rules reader.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_object_size_for_test(&mut self, type_id: &str, size: f64) {
        let handle = self
            .type_handle(type_id)
            .expect("native fixture type exists");
        self.object_list[handle.0 as usize].size = size;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn art_entry_mut_for_test(
        &mut self,
        name: &str,
    ) -> Option<&mut crate::rules::art_data::ArtEntry> {
        self.voxel_assets = Default::default();
        self.art_registry.get_mut(name)
    }

    /// Resolve every registered object type's authoritative animation timing
    /// after ART's `[*Sequence]` sections have been parsed.
    pub fn bind_animation_sequences(
        &mut self,
        infantry_sequences: &crate::rules::infantry_sequence::InfantrySequenceRegistry,
    ) {
        self.rebuild_animation_sequences(Some(infantry_sequences));
    }

    /// Resolve particle image SHP frame counts from the active theater assets
    /// without constructing a renderer atlas.
    pub fn bind_effect_assets(
        &mut self,
        asset_manager: &crate::assets::asset_manager::AssetManager,
        theater_ext: &str,
        theater_name: &str,
    ) {
        self.effect_assets = crate::rules::effect_asset_catalog::EffectAssetCatalog::bind(
            self,
            asset_manager,
            theater_ext,
            theater_name,
        );
        self.voxel_assets =
            crate::rules::voxel_asset_catalog::VoxelAssetCatalog::bind(self, asset_manager);
    }

    /// Techno6F9E10's loaded-main-VXL predicate for the type `object` names,
    /// never an ART declaration or renderer-cache query. Unbound/missing/
    /// malformed models answer false.
    pub(crate) fn object_has_loaded_voxel(&self, object: TypeHandle) -> bool {
        self.voxel_assets.contains(object)
    }

    /// Original main-VXL half sizes retained by the same immutable binding as
    /// IsVoxel6F9E10. Draw_Matrix and GetFLH consume this input independently
    /// of the renderer's model or pose caches.
    pub(crate) fn object_voxel_half_sizes(&self, type_id: &str) -> Option<[f32; 2]> {
        self.voxel_assets.half_sizes(type_id)
    }

    /// Explicit loaded model prior for headless component fixtures. Production
    /// callers bind immutable admission from the active physical assets.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn supply_loaded_voxel_for_test(&mut self, type_id: &str) {
        let object = self
            .type_handle(type_id)
            .expect("loaded type fixture must exist");
        self.voxel_assets.supply_loaded(type_id, object);
    }

    /// Bind native TerrainType frame-0 RadarColor inputs and raw SHP frame
    /// counts for TIBTRE timing from the active theater's asset bytes.
    pub fn bind_terrain_assets(
        &mut self,
        asset_manager: &crate::assets::asset_manager::AssetManager,
        theater_ext: &str,
        phase: crate::rules::terrain_asset_catalog::TerrainAssetLoadPhase,
    ) {
        self.terrain_assets = crate::rules::terrain_asset_catalog::TerrainAssetCatalog::bind(
            &mut self.terrain_object_types,
            asset_manager,
            theater_ext,
            phase,
        );
    }

    pub fn terrain_shape_filename(&self, name: &str) -> Option<&str> {
        self.terrain_assets.shape_filename(name)
    }

    /// Resolve every building type's Buildup SHP from the active theater's
    /// assets into its construction control, without a renderer atlas.
    pub fn bind_building_buildup_assets(
        &mut self,
        asset_manager: &crate::assets::asset_manager::AssetManager,
        theater_name: &str,
    ) {
        self.buildup_assets = crate::rules::buildup_asset_catalog::BuildupAssetCatalog::bind(
            self,
            asset_manager,
            theater_name,
        );
    }

    /// Resolve every `LeaveRubble=yes` building type's rubble frames from the
    /// active theater's assets, without a renderer atlas.
    pub fn bind_building_rubble_assets(
        &mut self,
        asset_manager: &crate::assets::asset_manager::AssetManager,
        theater_ext: &str,
        theater_name: &str,
    ) {
        self.rubble_assets = crate::rules::building_rubble::RubbleAssetCatalog::bind(
            self,
            asset_manager,
            theater_ext,
            theater_name,
        );
    }

    /// A building type's rubble art; `None` when it leaves no rubble
    /// ([`crate::rules::building_rubble`]).
    pub fn building_rubble_art(
        &self,
        type_id: &str,
    ) -> Option<&crate::rules::building_rubble::RubbleArt> {
        self.rubble_assets.art(type_id)
    }

    /// Every bound rubble art, for presentation preloading.
    pub fn building_rubble_arts(
        &self,
    ) -> impl Iterator<Item = &crate::rules::building_rubble::RubbleArt> {
        self.rubble_assets.iter()
    }

    /// Install one building type's rubble art (fixtures without assets).
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_building_rubble_art_for_test(
        &mut self,
        type_id: &str,
        art: crate::rules::building_rubble::RubbleArt,
    ) {
        self.rubble_assets.insert_for_test(type_id, art);
    }

    /// A building type's construction control (`Type+0xF04`: first frame,
    /// frame count, rate); the constructor's `{0, 1, 0}` when its Buildup SHP
    /// is unbound.
    pub fn buildup_control(&self, type_id: &str) -> [i32; 3] {
        self.buildup_assets.control(type_id)
    }

    /// Whether a building type's Buildup SHP is bound (`+0x6E9`,
    /// [`crate::rules::buildup_asset_catalog::BuildupAssetCatalog::has_buildup`]).
    pub fn has_buildup(&self, type_id: &str) -> bool {
        self.buildup_assets.has_buildup(type_id)
    }

    /// Install one building type's construction control (fixtures without
    /// assets).
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_buildup_control_for_test(&mut self, type_id: &str, control: [i32; 3]) {
        self.buildup_assets.insert_for_test(type_id, control);
    }

    /// Raw SHP header count used by `TerrainClass::AI` midpoint timing.
    pub fn terrain_spawner_frame_count(&self, name: &str) -> Option<u16> {
        self.terrain_assets.frame_count(name)
    }

    fn rebuild_animation_sequences(
        &mut self,
        infantry_sequences: Option<&crate::rules::infantry_sequence::InfantrySequenceRegistry>,
    ) {
        self.animation_sequences =
            crate::rules::animation_sequence::build_animation_sequence_catalog(
                self,
                infantry_sequences,
            );
        self.index_animation_sequences();
    }

    fn index_animation_sequences(&mut self) {
        self.animation_sequence_by_handle = self
            .object_list
            .iter()
            .map(|object| self.animation_sequences.get(&object.id).cloned())
            .collect();
    }

    pub(crate) fn animation_sequences(
        &self,
    ) -> &BTreeMap<String, crate::rules::animation_sequence::SequenceSet> {
        &self.animation_sequences
    }

    pub fn animation_sequence(
        &self,
        type_id: &str,
    ) -> Option<&crate::rules::animation_sequence::SequenceSet> {
        let handle = self.type_handle(type_id)?;
        self.animation_sequence_by_handle[handle.0 as usize].as_ref()
    }

    pub(crate) fn unit_shp_read_state(
        &self,
        type_id: &str,
    ) -> Option<&crate::rules::shp_vehicle_sequence::UnitShpReadState> {
        let object = self.object(type_id)?;
        self.unit_shp_read_states.get(&object.id)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn replace_animation_sequences_for_test(
        &mut self,
        animation_sequences: BTreeMap<String, crate::rules::animation_sequence::SequenceSet>,
    ) {
        self.animation_sequences = animation_sequences;
        self.index_animation_sequences();
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_effect_frame_count_for_test(&mut self, name: &str, raw: u16, available: u16) {
        self.effect_assets.set_for_test(name, raw, available);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_terrain_spawner_frame_count_for_test(
        &mut self,
        name: &str,
        frame_count: u16,
    ) {
        self.terrain_assets.set_for_test(name, frame_count);
    }

    /// Total number of game objects across all categories.
    #[cfg(test)]
    pub fn object_count(&self) -> usize {
        self.object_list.len()
    }

    /// Total number of weapons.
    #[cfg(test)]
    pub fn weapon_count(&self) -> usize {
        self.weapons.len()
    }

    /// Total number of warheads.
    #[cfg(test)]
    pub fn warhead_count(&self) -> usize {
        self.warheads.len()
    }

    /// Iterate all parsed warhead types.
    pub fn warheads_iter(&self) -> impl Iterator<Item = &WarheadType> {
        self.warheads.values()
    }

    /// Iterate all parsed weapon types.
    pub fn weapons_iter(&self) -> impl Iterator<Item = &WeaponType> {
        self.weapons.values()
    }

    /// Total number of projectiles.
    #[cfg(any(test, feature = "test-support"))]
    pub fn projectile_count(&self) -> usize {
        self.projectiles.len()
    }

    /// Iterate over all game objects in the registry.
    pub fn all_objects(&self) -> impl Iterator<Item = &ObjectType> {
        self.object_list.iter()
    }

    /// Visit AlphaImage names only for typed families whose native
    /// ObjectTypeClass reader has an allocated Rust owner. AnimType names are
    /// restricted to ART bodies admitted by the Rules processor.
    pub(crate) fn visit_alpha_image_names(
        &self,
        overlays: &crate::rules::overlay_types::OverlayTypeRegistry,
        mut visit: impl FnMut(&str),
    ) {
        for object in &self.object_list {
            visit(&object.alpha_image);
        }
        for projectile in self.projectiles.values() {
            visit(&projectile.alpha_image);
        }
        for particle in &self.particle_types {
            visit(&particle.alpha_image);
        }
        for system in &self.particle_system_types {
            visit(&system.alpha_image);
        }
        for (_, smudge) in self.smudge_types.iter_with_id() {
            visit(&smudge.alpha_image);
        }
        for terrain in self.terrain_object_types.values() {
            visit(&terrain.alpha_image);
        }
        for voxel_anim in &self.voxel_anim_types {
            visit(&voxel_anim.alpha_image);
        }
        for image_name in overlays.alpha_images() {
            visit(image_name);
        }
        for (name, admitted) in &self.anim_type_art_read_states {
            if *admitted
                && let Some(config) = self.art_registry.anim_runtime_config(name)
                && config.art_body_read
            {
                visit(&config.alpha_image);
            }
        }
    }
}

/// Parse a type registry section (e.g., [InfantryTypes]) into a list of IDs.
///
/// Registry sections use numbered keys: `0=E1`, `1=E2`, ...
/// Returns empty Vec if the section doesn't exist.
fn parse_registry(ini: &IniFile, section_name: &str) -> Vec<String> {
    match ini.section(section_name) {
        Some(section) => {
            // Every caller consumes NativeRulesRegistryState::finish's
            // projected stored IDs. The original registry ReadString32 and
            // constructor410800 truncation already ran there. Re-reading
            // them would trim valid list-created IDs such as " TWO" and lose
            // the Type pointer identity retained by Country veteran lists.
            let raw: Vec<String> = section
                .registry_ids()
                .into_iter()
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .collect();
            // TypeClass identity is case-insensitive find-or-allocate. Preserve
            // declaration order while keeping the first spelling of an identity.
            let mut seen: std::collections::HashSet<String> =
                std::collections::HashSet::with_capacity(raw.len());
            let before = raw.len();
            let deduped: Vec<String> = raw
                .into_iter()
                .filter(|id| seen.insert(id.to_ascii_uppercase()))
                .collect();
            let removed = before - deduped.len();
            if removed > 0 {
                log::info!(
                    "Registry [{}]: removed {} duplicate entries",
                    section_name,
                    removed,
                );
            }
            deduped
        }
        None => {
            log::warn!("Registry section [{}] not found in rules.ini", section_name);
            Vec::new()
        }
    }
}

struct ProjectedCountrySideRegistry {
    rules: HashMap<String, CountryRules>,
    country_ids: Vec<String>,
    country_indices: HashMap<String, CountryIdx>,
    side_ids: Vec<String>,
    #[cfg(feature = "ares")]
    side_extensions: Vec<crate::rules::ares_side_extension::AresSideExtension>,
    side_indices: HashMap<String, SideIdx>,
    country_sides: Vec<Option<SideIdx>>,
}

/// A gameplay projection of the one live native registry. ReadSides672440
/// and Country511850 already made every allocation and side assignment in
/// reached-pass order; reconstructing those decisions from merged text would
/// invent Countries, lose prior Side vectors and change allocation chronology.
fn project_country_side_registry<'a>(
    countries: impl Iterator<Item = CountryProjection<'a>>,
    sides: impl Iterator<Item = SideProjection<'a>>,
) -> ProjectedCountrySideRegistry {
    let sides = sides.collect::<Vec<_>>();
    let side_ids: Vec<String> = sides.iter().map(|(id, _)| (*id).to_owned()).collect();
    #[cfg(feature = "ares")]
    let side_extensions = sides
        .iter()
        .map(|(_, extension)| (*extension.expect("allocated Side owns its Ares extension")).clone())
        .collect();
    let mut side_indices = HashMap::with_capacity(side_ids.len());
    for (index, id) in side_ids.iter().enumerate() {
        side_indices
            .entry(id.to_ascii_uppercase())
            .or_insert_with(|| {
                SideIdx(u8::try_from(index).expect("[Sides] exceeds u8 identity space"))
            });
    }
    let mut country_ids = Vec::new();
    let mut country_indices = HashMap::new();
    let mut country_sides = Vec::new();
    let mut rules = HashMap::new();
    for (id, section, color, side, parent, veteran) in countries {
        let index = CountryIdx(
            u16::try_from(country_ids.len()).expect("[Countries] exceeds u16 identity space"),
        );
        country_ids.push(id.to_owned());
        country_indices
            .entry(id.to_ascii_uppercase())
            .or_insert(index);
        country_sides.push((side >= 0).then(|| {
            let index = usize::try_from(side).expect("nonnegative Side index");
            assert!(index < side_ids.len(), "retained Country Side exists");
            SideIdx(u8::try_from(side).expect("[Sides] exceeds u8 identity space"))
        }));
        rules.entry(id.to_owned()).or_insert_with(|| {
            let mut fields = CountryRules::from_ini_section(section);
            fields.color_scheme = color;
            fields.parent_country = parent.to_owned();
            fields.veteran_types = veteran.clone();
            #[cfg(feature = "ares")]
            {
                fields.presentation = veteran.presentation().clone();
                fields.paradrop = veteran.paradrop().clone();
                fields.power_plants = veteran.power_plants().to_vec();
            }
            fields
        });
    }
    ProjectedCountrySideRegistry {
        rules,
        country_ids,
        country_indices,
        side_ids,
        #[cfg(feature = "ares")]
        side_extensions,
        side_indices,
        country_sides,
    }
}

/// Two-pass parse of `[Particles]`: collect `Pending` entries from each
/// referenced section, then resolve each `NextParticle=` name to a
/// `ParticleTypeId`. Missing references log a warning and stay `None`.
fn parse_particle_types(ini: &IniFile) -> (Vec<ParticleType>, HashMap<String, ParticleTypeId>) {
    let ids: Vec<String> = parse_registry(ini, "Particles");
    if ids.is_empty() {
        return (Vec::new(), HashMap::new());
    }

    // Pass 1: parse each section into PendingParticleType. Skip IDs whose
    // section is missing — matches the behavior used elsewhere in this file.
    let mut pending: Vec<PendingParticleType> = Vec::with_capacity(ids.len());
    for id in &ids {
        if let Some(section) = ini.section(id) {
            pending.push(ParticleType::from_ini_section_pending(id, section));
        } else {
            log::trace!(
                "ParticleType '{}' listed in [Particles] but has no section",
                id
            );
        }
    }

    // Build the name → ID map (uppercase keys for case-insensitive lookup).
    let mut by_name: HashMap<String, ParticleTypeId> = HashMap::with_capacity(pending.len());
    for (idx, p) in pending.iter().enumerate() {
        by_name.insert(
            p.partial.name.to_ascii_uppercase(),
            ParticleTypeId(idx as u32),
        );
    }

    // Pass 2: resolve NextParticle references.
    let particle_types: Vec<ParticleType> = pending
        .into_iter()
        .map(|p| {
            let mut partial = p.partial;
            if let Some(ref next_name) = p.next_particle_name {
                let key = next_name.to_ascii_uppercase();
                match by_name.get(&key) {
                    Some(&id) => partial.next_particle = Some(id),
                    None => {
                        log::warn!(
                            "ParticleType '{}': NextParticle='{}' references unknown particle, leaving unresolved",
                            partial.name,
                            next_name
                        );
                    }
                }
            }
            partial
        })
        .collect();

    log::info!("Particles: {} loaded", particle_types.len());
    (particle_types, by_name)
}

/// `[VoxelAnims]` registry: the flying-debris types a death throws.
///
/// Registry order is the id order, exactly as for particles and particle
/// systems, because `DebrisTypes=` and `Spawns=` resolve by name and the ids
/// are hashed.
///
/// RESIDUAL: RulesClass::ReadVoxelAnims (`0x00672920`) allocates a type for
/// every listed name through FindOrAllocate (`0x0074B960`), section or not;
/// VERA skips an entry without a section, which shifts later ids and leaves a
/// `DebrisTypes=` reference to it unresolved. Every retail entry has a
/// section.
fn parse_voxel_anim_types(ini: &IniFile) -> (Vec<VoxelAnimType>, HashMap<String, VoxelAnimTypeId>) {
    let ids: Vec<String> = parse_registry(ini, "VoxelAnims");
    let mut types: Vec<VoxelAnimType> = Vec::with_capacity(ids.len());
    let mut by_name: HashMap<String, VoxelAnimTypeId> = HashMap::with_capacity(ids.len());
    for id in &ids {
        let Some(section) = ini.section(id) else {
            log::trace!(
                "VoxelAnimType '{}' listed in [VoxelAnims] but has no section",
                id
            );
            continue;
        };
        by_name.insert(id.to_ascii_uppercase(), VoxelAnimTypeId(types.len() as u32));
        types.push(VoxelAnimType::from_ini_section(id, section));
    }
    (types, by_name)
}

/// `[ParticleSystems]` in registry order, each bound to the ParticleType its
/// retained `HoldsWhat` names (ParticleSystemType+294).
///
/// RESIDUAL: a reached pass that omits `HoldsWhat` binds the native
/// empty-named ParticleType, a section-less type with constructor values that
/// the `[Particles]` projection cannot list. VERA holds `None`, so the system
/// spawns none of that type's particles. Only a layer that states a system
/// without its `HoldsWhat` reaches it; every retail ParticleSystem section is
/// in RULESMD and names a particle.
fn parse_particle_system_types(
    processed: &ProcessedRulesLayers,
    p_by_name: &HashMap<String, ParticleTypeId>,
) -> (
    Vec<ParticleSystemType>,
    HashMap<String, ParticleSystemTypeId>,
) {
    let ini = processed.ini();
    let ids: Vec<String> = parse_registry(ini, "ParticleSystems");
    if ids.is_empty() {
        return (Vec::new(), HashMap::new());
    }
    // Equal truncated stored IDs share one projected body, the last such
    // member's; its binding is inserted last.
    let holds_what: HashMap<String, Option<&str>> = processed
        .particle_system_holds_what_states()
        .map(|(id, particle)| (id.to_ascii_uppercase(), particle))
        .collect();

    let mut particle_system_types = Vec::with_capacity(ids.len());
    let mut by_name: HashMap<String, ParticleSystemTypeId> = HashMap::with_capacity(ids.len());
    for id in &ids {
        let Some(section) = ini.section(id) else {
            log::trace!(
                "ParticleSystemType '{}' listed in [ParticleSystems] but has no section",
                id
            );
            continue;
        };
        let mut system = ParticleSystemType::from_ini_section(id, section);
        if let Some(&Some(particle)) = holds_what.get(&id.to_ascii_uppercase()) {
            system.holds_what = p_by_name.get(&particle.to_ascii_uppercase()).copied();
            if system.holds_what.is_none() && !particle.is_empty() {
                log::warn!(
                    "ParticleSystemType '{}': HoldsWhat='{}' references unknown particle, leaving unresolved",
                    id,
                    particle
                );
            }
        }
        by_name.insert(
            id.to_ascii_uppercase(),
            ParticleSystemTypeId(particle_system_types.len() as u32),
        );
        particle_system_types.push(system);
    }

    log::info!("ParticleSystems: {} loaded", particle_system_types.len());
    (particle_system_types, by_name)
}

#[cfg(test)]
mod tests {
    #[test]
    fn ares_emp_ai_global_mission_uses_production_layers_and_config_hash() {
        use crate::rules::{
            mission_data::MissionType,
            native_processing::{RulesLayerKind, RulesLayerStack},
        };
        let root = "[VehicleTypes]\n0=TANK\n[TANK]\nStrength=400\n[CombatDamage]\nEMPAIRecoverMission=Guard\n";
        let make = |language: &str| {
            let mut layers = RulesLayerStack::new(IniFile::from_str(root));
            layers.push(
                RulesLayerKind::LangRule,
                IniFile::from_str(&format!("[CombatDamage]\nEMPAIRecoverMission={language}\n")),
            );
            layers.push(
                RulesLayerKind::GameMode,
                IniFile::from_str("[CombatDamage]\nEMPAIRecoverMission=junk\n"),
            );
            layers.push(
                RulesLayerKind::Scenario,
                IniFile::from_str("[CombatDamage]\nEMPAIRecoverMission=\n"),
            );
            RuleSet::from_processed_rules(&layers.process().unwrap()).unwrap()
        };
        let rules = make("Area Guard");
        let expected = crate::rules::ARES_ENABLED.then_some(MissionType::AreaGuard);
        assert_eq!(rules.ares_emp_ai_recover_mission(), expected);
        let retained_hash = rules.simulation_config_hash();
        let mut changed = make("Hunt");
        // Source identity deliberately includes layer history, even when the
        // final merged text agrees. Normalize only this private test input to
        // isolate the retained extension state in the compatibility hash.
        assert_ne!(rules.source_ini_hash(), changed.source_ini_hash());
        changed.source_ini_hash = rules.source_ini_hash;
        assert_eq!(
            changed.ares_emp_ai_recover_mission(),
            crate::rules::ARES_ENABLED.then_some(MissionType::Hunt)
        );
        assert_eq!(
            retained_hash == changed.simulation_config_hash(),
            !crate::rules::ARES_ENABLED
        );
        let fresh = RuleSet::from_processed_rules(
            &RulesLayerStack::new(IniFile::from_str(
                "[VehicleTypes]\n0=TANK\n[TANK]\nStrength=400\n",
            ))
            .process()
            .unwrap(),
        )
        .unwrap();
        assert!(
            fresh.ares_emp_ai_recover_mission().is_none(),
            "new RulesClass constructor"
        );
    }
    #[test]
    fn infantry_occupy_pip_uses_each_admitted_rules_section_pass() {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        // Original Infantry5240D1..524125 calls ReadPip4748A0 on every
        // admitted section, even when the pip key is missing. Executed native
        // section controls: passenger_pips.json/read_pip (also Pip3→4→5→7).
        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[InfantryTypes]\n0=E1\n[E1]\nPip=white\nOccupyPip=PersonBlue\n",
        ));
        for (patch, expected) in [
            ("[Unrelated]\nStrength=1\n", 11),
            ("[E1]\nStrength=125\n", 1),
            ("[E1]\nStrength=126\n", 2),
            ("[E1]\nStrength=127\n", 3),
        ] {
            layers.push(RulesLayerKind::Scenario, IniFile::from_str(patch));
            let processed = layers.process().expect("ordered native rules passes");
            let rules = RuleSet::from_processed_rules(&processed).expect("projected types");
            assert_eq!(
                rules.object("E1").unwrap().occupy_pip(),
                expected,
                "ReadPip must use the retained field as a table index after {patch:?}"
            );
        }
    }

    fn detail_native() -> &'static serde_json::Value {
        crate::test_fixture::json("tools/rules_oracle/weapon_laser.json")
    }

    fn detail_state(detail: DetailRules) -> serde_json::Value {
        serde_json::json!({
            "min_frame_rate_normal": detail.min_frame_rate_normal,
            "min_frame_rate_movie": detail.min_frame_rate_movie,
            "buffer_zone_width": detail.buffer_zone_width,
        })
    }

    fn detail_cached_ini(sections: &serde_json::Value) -> IniFile {
        let mut ini = IniFile::empty();
        for (name, keys) in sections.as_object().unwrap() {
            let mut section = IniSection::new(name.clone());
            for (key, value) in keys.as_object().unwrap() {
                section.set(key, value.as_str().unwrap());
            }
            ini.replace_first_section(section);
        }
        ini
    }

    #[test]
    fn detail_rules_match_original_constructor_and_signed_readers() {
        let native = detail_native();
        assert_eq!(
            detail_state(DetailRules::default()),
            native["detail"]["constructor"]
        );
        let rows = native["detail"]["scalar_controls"].as_array().unwrap();
        assert_eq!(rows.len(), 15);
        for row in rows {
            let ini = detail_cached_ini(&row["sections"]);
            let rules = RuleSet::from_ini(&ini).unwrap();
            assert_eq!(detail_state(rules.general.detail), row["state"], "{row}");
        }
    }

    #[test]
    fn detail_rules_retain_state_through_cold_reads_process_handoffs_and_type_reset() {
        use crate::rules::native_processing::{
            NativeRulesRegistryState, RulesLayerStack, process_native_rules_cold_start,
        };

        let native = detail_native();
        let rows = native["detail"]["retained_history"].as_array().unwrap();
        assert_eq!(rows.len(), 10);
        let mut retained = NativeRulesRegistryState::default();
        for row in rows {
            let pass = detail_cached_ini(&row["sections"]);
            if row["kind"] == "audio_visual" {
                retained = process_native_rules_cold_start(
                    retained,
                    &pass,
                    &IniFile::empty(),
                    None,
                    &crate::rules::sound_ini::EvaRegistry::default(),
                )
                .unwrap()
                .into_registry_state_discarding_events();
            } else if row["kind"] == "type_reset_and_process" {
                retained = retained.destructive_reset();
            }
            // The empty Process after a cold read exposes retained RulesClass
            // fields without replaying the authored AudioVisual keys.
            let pass = if row["kind"] == "audio_visual" {
                IniFile::empty()
            } else {
                pass
            };
            let processed = RulesLayerStack::new(pass)
                .process_with_fixed_art_and_registry_state(&IniFile::empty(), retained)
                .unwrap();
            let rules = RuleSet::from_processed_rules(&processed).unwrap();
            assert_eq!(detail_state(rules.general.detail), row["state"], "{row}");
            let (_, trace) = processed.into_ini_and_native_type_construction_trace();
            retained = trace.into_registry_state_discarding_events();
        }
    }

    #[test]
    fn retained_detail_rules_change_configuration_identity() {
        use crate::rules::native_processing::RulesLayerStack;
        let mut identities = Vec::new();
        for initial in [15, 31] {
            let first = RulesLayerStack::new(IniFile::from_str(&format!(
                "[AudioVisual]\nDetailMinFrameRateNormal={initial}\n"
            )))
            .process()
            .unwrap();
            let (_, trace) = first.into_ini_and_native_type_construction_trace();
            let processed = RulesLayerStack::new(IniFile::empty())
                .process_with_fixed_art_and_registry_state(
                    &IniFile::empty(),
                    trace.into_registry_state_discarding_events(),
                )
                .unwrap();
            let rules = RuleSet::from_processed_rules(&processed).unwrap();
            assert_eq!(rules.general.detail.min_frame_rate_normal, initial);
            identities.push((rules.source_ini_hash(), rules.simulation_config_hash()));
        }
        assert_eq!(identities[0].0, identities[1].0);
        assert_ne!(identities[0].1, identities[1].1);
    }

    #[test]
    fn gsi_05_14_voxel_anim_registry_indexes_the_stock_list_order() {
        // `[VoxelAnims]` is a numbered registry like `[Particles]`; ids are
        // list order and `DebrisTypes=`/`Spawns=` resolve by name.
        let ini = crate::rules::ini_parser::IniFile::from_str(
            "[VoxelAnims]
1=PIECE
2=TIRE
3=METEOR01
\
             [PIECE]
Duration=75
Elasticity=0
\
             [TIRE]
Duration=150
Elasticity=0.8
\
             [METEOR01]
IsMeteor=yes
Spawns=PEBBLE
SpawnCount=3
",
        );
        let rules = RuleSet::from_ini(&ini).expect("voxel anim rules parse");
        // METEOR01's `Spawns=PEBBLE` goes through `VoxelAnimTypeClass::
        // FindOrCreate @ 0x0074BB20`, which appends the unlisted PEBBLE.
        assert_eq!(rules.voxel_anim_type_count(), 4);
        assert_eq!(rules.voxel_anim_type_id_by_name("PEBBLE").unwrap().0, 3);
        let piece = rules
            .voxel_anim_type_id_by_name("piece")
            .expect("case-insensitive lookup");
        assert_eq!(piece.0, 0);
        assert_eq!(rules.voxel_anim_type(piece).duration, 75);
        let meteor = rules.voxel_anim_type_id_by_name("METEOR01").unwrap();
        assert_eq!(meteor.0, 2);
        let meteor = rules.voxel_anim_type(meteor);
        assert!(meteor.is_meteor);
        assert_eq!(meteor.spawns.as_deref(), Some("PEBBLE"));
        assert_eq!(meteor.spawn_count, 3);
    }
    use super::*;
    use crate::rules::native_processing::RulesLayerKind;

    #[test]
    fn retail_shell_slide_cues_come_from_audio_visual() {
        // RulesClass+0x19C GUIMoveOutSound (0x006694C7) and +0x1A0
        // GUIMoveInSound (0x00669509), read from [AudioVisual].
        let Some(ini) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
            return;
        };
        let general = GeneralRules::from_ini(&ini);
        assert_eq!(general.gui_move_out_sound.as_deref(), Some("MenuSlideOut"));
        assert_eq!(general.gui_move_in_sound.as_deref(), Some("MenuSlideIn"));
        // +0x750 ShellButtonSlideSound (0x0066AF8D) is authored empty, so it
        // keeps the constructor's -1, also after Battle's mode and map passes.
        assert_eq!(general.shell_button_slide_sound, None);
        if let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules() {
            assert_eq!(battle.rules.general.shell_button_slide_sound, None);
        }
    }

    #[test]
    fn retail_battle_hills_retains_empulse_warhead_from_rulesmd() {
        let Some(retail) =
            crate::rules::retail_ini_fixture::retail_battle_rules_for_map("Hills.mmx")
        else {
            return;
        };
        assert_eq!(
            retail.rules.general.empulse_warhead.as_deref(),
            Some("EMPuls"),
            "RulesMD EMPulseWarhead survives Battle and map passes without an override"
        );
    }

    #[test]
    fn empulse_warhead_pointer_keeps_native_empty_reads_and_clears_none() {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};

        assert_eq!(
            RuleSet::from_ini(&IniFile::empty())
                .unwrap()
                .general
                .empulse_warhead,
            None,
            "native constructor initializes Rules+0xFA0 to null"
        );
        let root = IniFile::from_str(
            "[Warheads]\n0=EMPuls\n[EMPuls]\n\
             Verses=100%,90%,80%,70%,60%,50%,40%,30%,20%,10%,0%\n\
             [SpecialWeapons]\nEMPulseWarhead=EMPuls\n",
        );
        let mut layers = RulesLayerStack::new(root);
        layers.push(
            RulesLayerKind::GameMode,
            IniFile::from_str("[SpecialWeapons]\nEMPulseWarhead=\n"),
        );
        layers.push(RulesLayerKind::Scenario, IniFile::from_str("[General]\n"));
        let retained = RuleSet::from_processed_rules(&layers.process().unwrap()).unwrap();
        assert_eq!(retained.general.empulse_warhead.as_deref(), Some("EMPuls"));

        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[SpecialWeapons]\nEMPulseWarhead=none\n"),
        );
        let cleared = RuleSet::from_processed_rules(&layers.process().unwrap()).unwrap();
        assert_eq!(cleared.general.empulse_warhead, None);
    }

    #[test]
    fn retail_production_click_keys_come_from_rulesmd() {
        // Rules+0xF0 MaximumQueuedObjects ([General], 0x00671DA7), +0x18C
        // GUIBuildSound, +0x6EC BuildingSlam and +0x700 ScoldSound
        // ([AudioVisual], 0x006693C1, 0x0066AAA7 and 0x0066ABE8).
        let Some(ini) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
            return;
        };
        let general = GeneralRules::from_ini(&ini);
        assert_eq!(general.maximum_queued_objects, 29);
        assert_eq!(general.gui_build_sound.as_deref(), Some("MenuClick"));
        assert_eq!(general.building_slam.as_deref(), Some("PlaceBuilding"));
        let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        assert_eq!(
            retail.rules.general.scold_sound.as_deref(),
            Some("MenuScold")
        );
    }

    /// RulesClass constructor @ 0x00667588 stores 0x280 leptons; ReadGeneral
    /// @ 0x00670EDD calls ReadRange @ 0x00474620 for [General] CloseEnough.
    /// This pins the Rust reader against the absent key and physical retail
    /// input; the arithmetic claim is still instruction-level native evidence.
    #[test]
    fn close_enough_keeps_constructor_default_and_reads_retail_range() {
        let absent = RuleSet::from_ini(&IniFile::from_str("[General]\n"))
            .expect("rules with absent CloseEnough parse");
        assert_eq!(absent.general.close_enough, 0x280);

        let Some(ini) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
            return;
        };
        assert!(
            ini.section("General")
                .and_then(|section| section.get_for_test("CloseEnough"))
                .is_some(),
            "retail General CloseEnough key is present"
        );
        let retail = RuleSet::from_ini(&ini).expect("retail rules parse");
        assert_eq!(retail.general.close_enough, 576);
    }

    /// `[CombatDamage] PsychicRevealRadius=` (ReadInteger at `0x0066C665`)
    /// and `[AudioVisual] AllyReveal=` (ReadBool at `0x0066B318`) keep the
    /// constructor's 3 and 1 (`0x00666BCA`, `0x0066773F`) when absent;
    /// retail RULESMD.INI sets 15 and yes.
    #[test]
    fn psychic_reveal_radius_and_ally_reveal_read_over_constructor_defaults() {
        let absent = GeneralRules::from_ini(&IniFile::from_str("[General]\nFlightLevel=500\n"));
        assert_eq!(absent.psychic_reveal_radius, 3);
        assert!(absent.ally_reveal);
        let off = GeneralRules::from_ini(&IniFile::from_str(
            "[General]\nFlightLevel=500\n[AudioVisual]\nAllyReveal=no\n",
        ));
        assert!(!off.ally_reveal);
        // Both readers run whether or not [General] exists (`0x00668F36`,
        // `0x00668F56`), and so does the case's activate sound.
        let alone = GeneralRules::from_ini(&IniFile::from_str(
            "[CombatDamage]\nPsychicRevealRadius=7\n[AudioVisual]\nAllyReveal=no\n\
             PsychicRevealActivateSound=PsychicRevealActivate\n",
        ));
        assert_eq!((alone.psychic_reveal_radius, alone.ally_reveal), (7, false));
        assert_eq!(
            alone.psychic_reveal_activate_sound.as_deref(),
            Some("PsychicRevealActivate")
        );
        let Some(ini) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
            return;
        };
        let general = GeneralRules::from_ini(&ini);
        assert_eq!(general.psychic_reveal_radius, 15);
        assert!(general.ally_reveal);
    }

    /// `[MultiplayerDialogSettings] AlliesAllowed=` (`0x0067215B`) and `[AI]
    /// Paranoid=` (`0x00673D43`) keep the constructor's 0 and 1
    /// (`0x006672D5`, `0x00667715`) when absent, and both readers run whether
    /// or not [General] exists (`0x00668EC0`, `0x00668EC8`). The Battle mode
    /// layers MPBATTLEMD's `AlliesAllowed=yes` over RULESMD's `no`;
    /// FreeForAll's MPFREEFORALLMD says `no`.
    #[test]
    #[ignore = "slow; run when changing what it covers"]
    fn allies_allowed_and_paranoid_read_over_constructor_defaults() {
        let absent = GeneralRules::from_ini(&IniFile::from_str("[General]\nFlightLevel=500\n"));
        assert!(!absent.allies_allowed);
        assert!(absent.paranoid);
        let alone = GeneralRules::from_ini(&IniFile::from_str(
            "[MultiplayerDialogSettings]\nAlliesAllowed=yes\n[AI]\nParanoid=no\n",
        ));
        assert!(alone.allies_allowed);
        assert!(!alone.paranoid);
        let Some(ini) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
            return;
        };
        let general = GeneralRules::from_ini(&ini);
        assert!(!general.allies_allowed);
        assert!(general.paranoid);
        let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        assert!(battle.rules.general.allies_allowed);
        assert!(battle.rules.general.paranoid);
        let free_for_all =
            crate::rules::retail_ini_fixture::retail_mode_rules_for_map(2, "Hills.mmx")
                .expect("retail FreeForAll rules beside the Battle ones");
        assert!(!free_for_all.rules.general.allies_allowed);
        assert!(free_for_all.rules.general.paranoid);
    }

    #[test]
    fn cloak_global_defaults_and_native_minute_conversion_parse() {
        let defaults = GeneralRules::default();
        assert_eq!(defaults.cloaking_stages, 9);
        assert_eq!(defaults.cloak_delay_frames, 0);
        assert_eq!(
            GeneralRules::from_ini(&IniFile::from_str("[General]\n")).cloak_delay_frames,
            defaults.cloak_delay_frames,
        );
        assert_eq!(defaults.cloak_sound, None);
        assert_eq!(defaults.leave_bio_reactor_sound, None);
        assert_eq!(defaults.enter_bio_reactor_sound, None);
        let parsed = GeneralRules::from_ini(&IniFile::from_str(
            "[General]\nCloakingStages=13\nCloakDelay=.031\n\
             [AudioVisual]\nCloakSound=NavalUnitEmerge\n",
        ));
        assert_eq!(parsed.cloaking_stages, 13);
        assert_eq!(parsed.cloak_delay_frames, 27);
        assert_eq!(parsed.cloak_sound.as_deref(), Some("NavalUnitEmerge"));
        assert_eq!(parsed.leave_bio_reactor_sound, None);
        assert_eq!(parsed.enter_bio_reactor_sound, None);
        assert_eq!(
            GeneralRules::from_ini(&IniFile::from_str("[General]\nCloakingStages=9\n")).cloak_sound,
            None,
            "missing CloakSound preserves the native invalid-index default"
        );
        assert_eq!(
            GeneralRules::from_ini(&IniFile::from_str(
                "[General]\nCloakingStages=9\n[AudioVisual]\nCloakSound=\n",
            ))
            .cloak_sound,
            None,
            "empty CloakSound preserves the native invalid-index default"
        );
    }

    #[cfg(feature = "ares")]
    #[test]
    fn unit_delivery_negative_deferment_keeps_the_twenty_frame_fallback() {
        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[SuperWeaponTypes]\n0=Delivery\n[Delivery]\nType=UnitDelivery\nSW.Deferment=-1",
        ));
        layers.push(
            crate::rules::native_processing::RulesLayerKind::Scenario,
            IniFile::from_str("[Delivery]\nRechargeTime=2"),
        );
        let rules = RuleSet::from_rules_layers(&layers).unwrap();
        assert_eq!(
            rules
                .super_weapon("Delivery")
                .unwrap()
                .unit_delivery
                .as_ref()
                .unwrap()
                .deferment,
            20
        );
    }

    #[cfg(feature = "ares")]
    #[test]
    fn unit_delivery_activation_sound_binding_keeps_known_values_across_unknown_layers() {
        let sounds = crate::rules::sound_ini::SoundRegistry::from_ini(&IniFile::from_str(
            "[SoundList]\n0=DeliveryCue\n",
        ));
        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[SuperWeaponTypes]\n0=Delivery\n[Delivery]\nType=UnitDelivery\nSW.ActivationSound=deliverycue",
        ));
        layers.push(
            crate::rules::native_processing::RulesLayerKind::Scenario,
            IniFile::from_str("[Delivery]\nSW.ActivationSound=Unknown"),
        );
        let inspect = |layers: &RulesLayerStack| {
            let processed = layers.process_with_fixed_art(&IniFile::empty()).unwrap();
            let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
            rules.bind_type_sound_references(processed.ini(), &sounds);
            rules
                .super_weapon("Delivery")
                .unwrap()
                .unit_delivery
                .as_ref()
                .unwrap()
                .activation_sound
                .clone()
        };
        assert_eq!(inspect(&layers).as_deref(), Some("DeliveryCue"));
        layers.push(
            crate::rules::native_processing::RulesLayerKind::Scenario,
            IniFile::from_str("[Delivery]\nSW.ActivationSound=none"),
        );
        assert_eq!(inspect(&layers), None);
    }

    #[test]
    fn bio_reactor_sounds_resolve_only_registered_vocs() {
        let sounds = crate::rules::sound_ini::SoundRegistry::from_ini(&IniFile::from_str(
            "[SoundList]\n0=BioReactorEnter\n1=BioReactorExit\n",
        ));
        let ini = IniFile::from_str(
            "[General]\n[AudioVisual]\nEnterBioReactorSound=BioReactorEnter\n\
             LeaveBioReactorSound=BioReactorExit\n",
        );
        let mut rules = RuleSet::from_ini(&ini).expect("rules");
        assert_eq!(rules.general.leave_bio_reactor_sound, None);
        assert_eq!(rules.general.enter_bio_reactor_sound, None);
        rules.bind_type_sound_references(&ini, &sounds);
        assert_eq!(
            rules.general.leave_bio_reactor_sound.as_deref(),
            Some("BioReactorExit")
        );
        assert_eq!(
            rules.general.enter_bio_reactor_sound.as_deref(),
            Some("BioReactorEnter")
        );

        let unknown_ini = IniFile::from_str(
            "[General]\n[AudioVisual]\nEnterBioReactorSound=NotRegistered\n\
             LeaveBioReactorSound=NotRegistered\n",
        );
        let mut unknown_rules = RuleSet::from_ini(&unknown_ini).expect("rules");
        unknown_rules.bind_type_sound_references(&unknown_ini, &sounds);
        assert_eq!(
            unknown_rules.general.leave_bio_reactor_sound, None,
            "native Voc lookup leaves the constructor's invalid index on a miss"
        );
        assert_eq!(
            unknown_rules.general.enter_bio_reactor_sound, None,
            "native Voc lookup leaves the constructor's invalid index on a miss"
        );
    }

    #[test]
    fn bio_reactor_entry_sound_uses_the_effective_processed_rules_pass() {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};

        let sounds = crate::rules::sound_ini::SoundRegistry::from_ini(&IniFile::from_str(
            "[SoundList]\n0=BioReactorEnter\n1=BioReactorExit\n",
        ));
        let rulesmd = IniFile::from_str(
            "[General]\n[AudioVisual]\nEnterBioReactorSound=BioReactorEnter\n\
             LeaveBioReactorSound=BioReactorExit\n",
        );
        let mut layers = RulesLayerStack::new(rulesmd);
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[AudioVisual]\nCloakSound=NavalUnitEmerge\n"),
        );
        let processed = layers.process().expect("ordered Rules passes process");
        let mut rules = RuleSet::from_processed_rules(&processed).expect("projected Rules");
        rules.bind_type_sound_references(processed.ini(), &sounds);
        assert_eq!(
            rules.general.enter_bio_reactor_sound.as_deref(),
            Some("BioReactorEnter"),
            "a later pass without the key leaves the earlier effective value"
        );
        assert_eq!(
            rules.general.leave_bio_reactor_sound.as_deref(),
            Some("BioReactorExit"),
            "a later pass without the key leaves the earlier effective value"
        );

        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str(
                "[AudioVisual]\nEnterBioReactorSound=BioReactorExit\n\
                 LeaveBioReactorSound=BioReactorEnter\n",
            ),
        );
        let processed = layers.process().expect("later Rules pass processes");
        let mut rules = RuleSet::from_processed_rules(&processed).expect("projected Rules");
        rules.bind_type_sound_references(processed.ini(), &sounds);
        assert_eq!(
            rules.general.enter_bio_reactor_sound.as_deref(),
            Some("BioReactorExit"),
            "the effective later Rules pass overrides the earlier key"
        );
        assert_eq!(
            rules.general.leave_bio_reactor_sound.as_deref(),
            Some("BioReactorEnter"),
            "the effective later Rules pass overrides the earlier key"
        );

        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str(
                "[AudioVisual]\nEnterBioReactorSound=NotRegistered\n\
                 LeaveBioReactorSound=NotRegistered\n",
            ),
        );
        let processed = layers.process().expect("invalid later pass processes");
        let mut rules = RuleSet::from_processed_rules(&processed).expect("projected Rules");
        rules.bind_type_sound_references(processed.ini(), &sounds);
        assert_eq!(
            rules.general.enter_bio_reactor_sound.as_deref(),
            Some("BioReactorExit"),
            "an unregistered later entry sound retains its earlier valid Voc"
        );
        assert_eq!(
            rules.general.leave_bio_reactor_sound.as_deref(),
            Some("BioReactorEnter"),
            "an unregistered later leave sound retains its earlier valid Voc"
        );
    }

    /// Build a minimal rules.ini string for testing.
    fn make_test_rules() -> String {
        "\
[InfantryTypes]
0=E1
1=E2

[General]
BuildSpeed=0.75
MultipleFactory=0.7
LowPowerPenaltyModifier=1.25
MinLowPowerProductionSpeed=0.4
MaxLowPowerProductionSpeed=0.85

[VehicleTypes]
0=MTNK

[AircraftTypes]

[BuildingTypes]
0=GAPOWR

[E1]
Name=GI
Cost=200
Strength=125
Armor=flak
Speed=4
Primary=M60
BuildTimeMultiplier=1.15

[E2]
Name=Conscript
Cost=100
Strength=100
Armor=flak
Speed=4
Primary=INTL

[MTNK]
Name=Grizzly
Cost=700
Strength=300
Armor=heavy
Speed=6
Primary=105mm
Secondary=MachGun

[GAPOWR]
Name=Power Plant
Cost=800
Strength=750
Power=200
Foundation=2x2

[M60]
Damage=25
ROF=20
Range=5
Warhead=SA

[INTL]
Damage=20
ROF=20
Range=4.75
Warhead=SA

[105mm]
Damage=65
ROF=50
Range=5.75
Speed=40
Projectile=InvisibleLow
Warhead=AP
Burst=2

[MachGun]
Damage=20
ROF=15
Range=5
Projectile=InvisibleLow
Warhead=SA

[InvisibleLow]
AA=no
AG=yes

[SA]
Verses=100%,100%,100%,90%,70%,25%,100%,25%,25%,0%,0%
CellSpread=0

[AP]
Verses=100%,100%,90%,75%,75%,75%,60%,30%,20%,0%,0%
CellSpread=0
"
        .to_string()
    }

    /// P7: per-country IncomeMult parses to PPM, defaults to the neutral 1.0×, and looks
    /// up case-insensitively. The hand-written Default must NOT be a derived zero.
    #[test]
    fn cost_of_matches_the_original_cost_virtuals() {
        let rows: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/cost_of.json",
        ))
        .unwrap();
        let bits = |value: &serde_json::Value| -> [NativeF32Bits; 5] {
            let bits: Vec<u32> = value
                .as_array()
                .unwrap()
                .iter()
                .map(|bits| bits.as_u64().unwrap() as u32)
                .collect();
            std::array::from_fn(|slot| NativeF32Bits::from_bits(bits[slot]))
        };
        let rows = rows.as_array().unwrap();
        for row in rows {
            let input = &row["input"];
            let cost = input["cost"].as_i64().unwrap();
            let techno = input["techno"].as_str();
            let own_cost = |kind: &str, other: i64| if techno == Some(kind) { cost } else { other };
            let free = input["free"].as_i64();
            let pads = &input["pads"];
            let key = |set: bool, line: &'static str| if set { line } else { "" };
            let separate = if input["separate_aircraft"] == 1 {
                "yes"
            } else {
                "no"
            };
            let ini = format!(
                "[General]\nSeparateAircraft={}\nPadAircraft=PAD1,PAD2\n\
                 [BuildingTypes]\n0=BUILDING\n[VehicleTypes]\n0=FREE\n\
                 [InfantryTypes]\n0=INF\n[AircraftTypes]\n0=PAD1\n1=PAD2\n\
                 [BUILDING]\nCost={cost}\n{}{}\n[FREE]\nCost={}\n[INF]\nCost={cost}\n\
                 [PAD1]\nCost={}\n{}\n[PAD2]\nCost={}\n",
                separate,
                key(free.is_some(), "FreeUnit=FREE\n"),
                key(input["build_cat"] == 5, "BuildCat=Combat"),
                own_cost("unit", free.unwrap_or(0)),
                own_cost("aircraft", pads[0].as_i64().unwrap()),
                key(input["docks"] == true, "Dock=BUILDING"),
                pads[1].as_i64().unwrap(),
            );
            let rules = RuleSet::from_ini(&IniFile::from_str(&ini)).unwrap();
            let object = match techno {
                None => "BUILDING",
                Some("unit") => "FREE",
                Some("infantry") => "INF",
                Some(_) => "PAD1",
            };
            let factors = HouseCostFactors {
                country: bits(&input["country"]),
                factory_plant: bits(&input["plant"]),
            };
            let house = (input["house"] == true).then_some(&factors);
            assert_eq!(
                rules.cost_of(rules.object(object).unwrap(), house),
                row["cost"].as_i64().unwrap() as i32,
                "native Cost_Of row {input}"
            );
        }
        assert_eq!(rows.len(), 90);
    }

    #[test]
    fn country_cost_mults_read_doubles_into_floats() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[Countries]\n0=Americans\n1=Russians\n\
             [Americans]\nCostInfantryMult=.8\nCostUnitsMult=85%\n",
        ))
        .unwrap();
        let one = NativeF32Bits::ONE;
        assert_eq!(
            rules.country_cost_mults("americans"),
            [
                NativeF32Bits::from_bits(0.8_f32.to_bits()),
                // 0.85000000000000009 stored under the chop control word.
                NativeF32Bits::from_bits(0x3F59_9999),
                one,
                one,
                one,
            ]
        );
        assert_eq!(rules.country_cost_mults("Russians"), [one; 5]);
        assert_eq!(rules.country_cost_mults("Nowhere"), [one; 5]);
    }

    #[test]
    fn country_speed_mults_read_doubles_into_floats() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[Countries]\n0=Americans\n1=Russians\n\
             [Americans]\nSpeedUnitsMult=1.15\nSpeedAircraftMult=85%\n",
        ))
        .unwrap();
        let one = NativeF32Bits::ONE;
        assert_eq!(
            rules.country_speed_mults("americans"),
            [
                one,
                NativeF32Bits::from_bits(1.15_f32.to_bits()),
                NativeF32Bits::from_bits(0x3F59_9999),
            ]
        );
        assert_eq!(rules.country_speed_mults("Russians"), [one; 3]);
    }

    /// No retail `RULESMD.INI` country authors a speed bonus, through the
    /// production reader (mode INIs and maps are not checked).
    #[test]
    fn retail_countries_author_no_speed_mults() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_rulesmd_rules() else {
            return;
        };
        assert!(!rules.countries.is_empty());
        for (name, country) in &rules.countries {
            assert_eq!(country.speed_mults, [NativeF32Bits::ONE; 3], "{name}");
        }
    }

    #[test]
    fn country_income_mult_parses_and_defaults() {
        assert_eq!(
            CountryRules::default().income_mult,
            NativeF32Bits::ONE,
            "default IncomeMult is 1.0x (a derived Default would zero it and wipe income)"
        );

        let src = format!(
            "{}\n[Countries]\n0=Americans\n1=Russia\n[Americans]\nIncomeMult=1.2\n[Russia]\nFixtureOnly=1\n",
            make_test_rules()
        );
        let ini = IniFile::from_str(&src);
        let rules = RuleSet::from_ini(&ini).expect("Should parse");
        assert_eq!(rules.country_income_mult("Americans").bits(), 0x3F99_999A);
        assert_eq!(rules.country_income_mult("Russia"), NativeF32Bits::ONE);
        assert_eq!(rules.country_income_mult("Nonexistent"), NativeF32Bits::ONE);
        assert_eq!(rules.country_income_mult("americans").bits(), 0x3F99_999A);
    }

    #[test]
    fn native_income_and_purifier_field_readers_preserve_observed_bits() {
        let corpus = crate::test_fixture::json("tools/spatial_oracle/refinery_income.json");
        let rows = corpus["reader_rows"].as_array().unwrap();
        assert_eq!(rows.len(), 12);
        for row in rows {
            let input = &row["input"];
            let field = input["field"].as_str().unwrap();
            let section = input["section"].as_str().unwrap();
            let current = NativeF32Bits::from_bits(
                u32::from_str_radix(input["default_bits"].as_str().unwrap(), 16).unwrap(),
            );
            let expected = NativeF32Bits::from_bits(
                u32::from_str_radix(row["output"]["bits"].as_str().unwrap(), 16).unwrap(),
            );
            let mut ini = IniFile::from_str(
                "[Countries]\n0=SYNTHCOUNTRY\n[SYNTHCOUNTRY]\nFixtureOnly=1\n[General]\nFixtureOnly=1\n",
            );
            if let Some(prefix) = input["prefix_raw"].as_str() {
                ini.merge(&IniFile::from_str(&format!(
                    "[{section}]\n{field}={prefix}\n"
                )));
            }
            if let Some(raw) = input["raw"].as_str() {
                ini.merge(&IniFile::from_str(&format!("[{section}]\n{field}={raw}\n")));
            }
            let rules = RuleSet::from_ini(&ini).unwrap();
            let actual = match field {
                "IncomeMult" => rules.country_income_mult("SYNTHCOUNTRY"),
                "PurifierBonus" => rules.general.purifier_bonus,
                _ => unreachable!(),
            };
            assert_eq!(actual, expected, "{} {}", field, input["name"]);
            // Also compare the actual retained-default reader input, not just
            // the immutable view reconstructed from the processed layers.
            let raw = input["raw"]
                .as_str()
                .map_or(String::new(), |raw| format!("{field}={raw}\n"));
            let direct = IniFile::from_str(&format!("[{section}]\nFixtureOnly=1\n{raw}"));
            assert_eq!(
                direct
                    .section(section)
                    .unwrap()
                    .read_double_to_float(field, current),
                expected
            );
        }
    }

    #[test]
    fn gsi_04_07_placement_wall_owner_parses_exact_key_and_defaults_true() {
        assert!(CountryRules::default().wall_owner);
        let ini = IniFile::from_str(
            "[Countries]\n0=Allowed\n1=Denied\n\
             [Allowed]\nWallOwner=yes\n\
             [Denied]\nWallOwner=no\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("country registry parses");
        assert!(rules.country_wall_owner("allowed"));
        assert!(!rules.country_wall_owner("DENIED"));
        assert!(rules.country_wall_owner("unknown"));
    }

    #[test]
    fn ordered_country_side_source_identity_and_case_insensitive_lookup() {
        let ini = IniFile::from_str(
            "[Countries]\n9=Zulu\n2=Alpha\n7=Middle\n\
             [Sides]\nNorth=Alpha,Middle\nSouth=Zulu\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("country/side registries parse");

        assert_eq!(rules.country_index("zulu"), Some(CountryIdx(0)));
        assert_eq!(rules.country_index("ALPHA"), Some(CountryIdx(1)));
        assert_eq!(rules.country_index("Middle"), Some(CountryIdx(2)));
        assert_eq!(rules.country_name(CountryIdx(1)), Some("Alpha"));
        assert_eq!(rules.side_index("north"), Some(SideIdx(0)));
        assert_eq!(rules.side_index("SOUTH"), Some(SideIdx(1)));
        assert_eq!(rules.side_name(SideIdx(0)), Some("North"));
    }

    /// Phobos `ValueableIdx<ColorScheme>` in each rules pass: a name finds the
    /// shade-count-1 scheme of a `[Colors]` entry loaded so far, `none` stores
    /// -1, and an empty or unknown name keeps the earlier pass's index.
    #[cfg(feature = "phobos")]
    #[test]
    fn timer_blink_color_scheme_resolves_in_each_rules_pass() {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        let scheme = |map: &str| {
            let mut layers = RulesLayerStack::new(IniFile::from_str(
                "[Colors]\nGold=43,239,255\nGrey=0,0,131\nLightGrey=0,0,240\n\
                 [AudioVisual]\nTimerBlinkColorScheme=grey\n",
            ));
            layers.push(RulesLayerKind::Scenario, IniFile::from_str(map));
            RuleSet::from_rules_layers(&layers)
                .expect("rules read")
                .general
                .timer_blink_color_scheme
        };
        // `Grey`, entry 1, is runtime scheme 2.
        assert_eq!(scheme(""), 2);
        assert_eq!(scheme("[AudioVisual]\nTimerBlinkColorScheme=\n"), 2);
        assert_eq!(scheme("[AudioVisual]\nTimerBlinkColorScheme=Nope\n"), 2);
        assert_eq!(scheme("[AudioVisual]\nTimerBlinkColorScheme=none\n"), -1);
        // The map's own [Colors] entry is loaded before the read.
        assert_eq!(
            scheme("[Colors]\nTeal=120,255,200\n[AudioVisual]\nTimerBlinkColorScheme=Teal\n"),
            6
        );
    }

    #[cfg(feature = "phobos")]
    #[test]
    fn missing_cameo_defaults_to_xxicon_and_reads_audio_visual() {
        let defaults = RuleSet::from_ini(&IniFile::from_str("[General]\n")).expect("rules read");
        assert_eq!(defaults.missing_cameo(), "XXICON.SHP");

        let named = RuleSet::from_ini(&IniFile::from_str(
            "[AudioVisual]\nMissingCameo=CUSTOM.PCX\n",
        ))
        .expect("rules read");
        assert_eq!(named.missing_cameo(), "CUSTOM.PCX");

        // Empty is no readable name, so the constructor default stands.
        let empty = RuleSet::from_ini(&IniFile::from_str("[AudioVisual]\nMissingCameo=\n"))
            .expect("rules read");
        assert_eq!(empty.missing_cameo(), "XXICON.SHP");

        // `PhobosFixedString::Read` clears the name on `none` or `<none>`.
        let none = RuleSet::from_ini(&IniFile::from_str("[AudioVisual]\nMissingCameo=<None>\n"))
            .expect("rules read");
        assert_eq!(none.missing_cameo(), "");
    }

    #[cfg(feature = "phobos")]
    #[test]
    fn sidebar_gdi_positions_default_the_first_side_and_read_side_sections() {
        let ini = IniFile::from_str(
            "[Countries]\n0=Alpha\n1=Beta\n2=Gamma\n\
             [Sides]\nNorth=Alpha\nSouth=Beta\nThird=Gamma\n\
             [North]\nSidebar.GDIPositions=no\n\
             [Third]\nSidebar.GDIPositions=yes\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("side sections parse");
        assert!(!rules.side_gdi_positions(0), "explicit no");
        assert!(!rules.side_gdi_positions(1), "default non-first side");
        assert!(rules.side_gdi_positions(2), "explicit yes");
        assert!(!rules.side_gdi_positions(99), "missing side is NOD");
        assert!(!rules.side_gdi_positions(-1), "negative side is NOD");

        let defaults = RuleSet::from_ini(&IniFile::from_str(
            "[Countries]\n0=Alpha\n1=Beta\n[Sides]\nNorth=Alpha\nSouth=Beta\n",
        ))
        .expect("default side positions parse");
        assert!(defaults.side_gdi_positions(0));
        assert!(!defaults.side_gdi_positions(1));
    }

    #[test]
    fn country_color_retains_the_catalog_present_at_each_reached_read() {
        // ReadColor474A90 resolves only currently registered schemes. The
        // original House controls observe Americans' Gold as native3;
        // Country ctor51141C and whole-section admission establish the0
        // boundary for the two unread/unresolved members below.
        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[Colors]\nLightGold=25,255,255\nGold=43,239,255\n\
             [Countries]\n0=Americans\n1=UnknownAtRead\n2=MissingBody\n\
             [Americans]\nColor=Gold\n\
             [UnknownAtRead]\nColor=AddedLater\n",
        ));
        layers.push(
            crate::rules::native_processing::RulesLayerKind::Scenario,
            IniFile::from_str("[Colors]\nAddedLater=0,0,0\n[Americans]\nColor=UnknownName\n"),
        );
        let rules = RuleSet::from_rules_layers(&layers).expect("ordered Color reads");
        assert_eq!(rules.country_color_scheme("americans"), 3);
        assert_eq!(rules.country_color_scheme("UnknownAtRead"), 0);
        assert_eq!(rules.country_color_scheme("MissingBody"), 0);
        assert_eq!(rules.scenario_country_name(None), Some("Americans"));
        assert_eq!(rules.scenario_country_name(Some("unknown")), None);
    }

    #[test]
    fn country_color_catalog_survives_handoff_and_is_cleared_by_type_reset() {
        let first = RulesLayerStack::new(IniFile::from_str(
            "[Colors]\nLightGold=25,255,255\nGold=43,239,255\n\
             [Countries]\n0=Americans\n[Americans]\nColor=Gold\n",
        ))
        .process()
        .unwrap();
        let (_, trace) = first.into_ini_and_native_type_construction_trace();
        let second = RulesLayerStack::new(IniFile::from_str("[Americans]\nColor=UnknownName\n"))
            .process_with_fixed_art_and_registry_state(
                &IniFile::empty(),
                trace.into_registry_state_discarding_events(),
            )
            .unwrap();
        let rules = RuleSet::from_processed_rules(&second).unwrap();
        assert_eq!(rules.country_color_scheme("Americans"), 3);
        assert_eq!(
            rules
                .color_schemes
                .iter()
                .map(|scheme| scheme.name.as_str())
                .collect::<Vec<_>>(),
            ["LightGold", "Gold"]
        );
        let (_, trace) = second.into_ini_and_native_type_construction_trace();
        let after_reset = RulesLayerStack::new(IniFile::from_str(
            "[Colors]\nNewAccent=0,0,0\n[Countries]\n0=Americans\n\
             [Americans]\nColor=UnknownName\n",
        ))
        .process_with_fixed_art_and_registry_state(
            &IniFile::empty(),
            trace
                .into_registry_state_discarding_events()
                .destructive_reset(),
        )
        .unwrap();
        let rules = RuleSet::from_processed_rules(&after_reset).unwrap();
        assert_eq!(rules.country_color_scheme("Americans"), 0);
        assert_eq!(
            rules
                .color_schemes
                .iter()
                .map(|scheme| scheme.name.as_str())
                .collect::<Vec<_>>(),
            ["NewAccent"]
        );
    }

    #[test]
    fn trigger_house_type_owner_uses_alias_then_id_and_native_signed_special() {
        let ini = IniFile::from_str(
            "[Countries]\n0=First\n1=Second\n2=Third\n\
             [First]\nName=Shared Alias\n\
             [Second]\nName=Shared Alias\n\
             [Third]\nName=Third Alias\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("country aliases parse");

        assert_eq!(
            rules.trigger_house_type_index("shared alias"),
            Some(CountryIdx(0)),
            "duplicate aliases keep first registration order"
        );
        assert_eq!(
            rules.trigger_house_type_index("second"),
            Some(CountryIdx(1))
        );
        assert_eq!(
            rules.trigger_house_type_index("THIRD ALIAS"),
            Some(CountryIdx(2))
        );
        assert_eq!(
            rules.trigger_house_type_index("<none>"),
            Some(CountryIdx(0))
        );
        assert_eq!(rules.trigger_house_type_index("<random>"), None);
        assert_eq!(rules.trigger_house_type_index("missing"), None);
        assert_eq!(
            rules.scenario_country_name(Some("shared alias")),
            Some("First")
        );
        assert_eq!(rules.scenario_country_name(Some("second")), Some("Second"));
    }

    #[test]
    fn gsi_02_13_ruleset_exposes_parsed_color_add_table() {
        let ini = IniFile::from_str(
            "[ColorAdd]\n\
             First=1,2,3\n\
             StrongGreen=0,63,0\n\
             BrightWhite=31,63,31\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("ColorAdd rules parse");

        assert_eq!(rules.color_add.entries()[0].name.as_deref(), Some("First"));
        assert_eq!(rules.color_add.entries()[0].rgb, [1, 2, 3]);
        assert_eq!(
            rules.color_add.entries()[1].name.as_deref(),
            Some("StrongGreen")
        );
        assert_eq!(rules.color_add.entries()[1].rgb, [0, 63, 0]);
        assert_eq!(rules.color_add.entries()[2].rgb, [31, 63, 31]);
        assert_eq!(
            rules.color_add.entries()[15],
            crate::rules::color_add::ColorAddEntry::default()
        );
    }

    #[test]
    fn ordered_country_side_country_override_moves_and_allocates_side() {
        let ini = IniFile::from_str(
            "[Countries]\n0=Alpha\n1=Beta\n2=Gamma\n\
             [Sides]\nGDI=Alpha,Beta\nNod=Gamma\n\
             [Beta]\nSide=Nod\n\
             [Gamma]\nSide=FourthSide\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("country side overrides parse");

        assert_eq!(rules.country_side_index("Alpha"), Some(SideIdx(0)));
        assert_eq!(rules.country_side_index("beta"), Some(SideIdx(1)));
        assert_eq!(rules.country_side_index("GAMMA"), Some(SideIdx(2)));
        assert_eq!(rules.side_name(SideIdx(2)), Some("FourthSide"));
        assert_eq!(rules.side_index("fourthside"), Some(SideIdx(2)));
    }

    #[test]
    fn ordered_country_side_members_do_not_allocate_unknown_country() {
        let ini = IniFile::from_str(
            "[Countries]\n0=Alpha\n\
             [Sides]\nGDI=Alpha,Beta\n\
             [Beta]\nSide=NewSide\nMultiplayPassive=yes\n",
        );
        let processed = RulesLayerStack::new(ini)
            .process()
            .expect("Side members process");
        let rules = RuleSet::from_processed_rules(&processed).expect("registered Country projects");

        assert_eq!(rules.country_index("Beta"), None);
        assert_eq!(rules.side_index("NewSide"), None);
        assert_eq!(rules.country_side_index("beta"), None);
        assert_eq!(rules.country_side_index("Alpha"), Some(SideIdx(0)));
        assert_eq!(processed.country_color_scheme("Beta"), None);
        assert_eq!(
            processed
                .native_type_construction_trace()
                .registry_state()
                .side_country_members("GDI"),
            Some([0].as_slice()),
            "ReadHouses4767C0 skips unknown Beta; its orphan body is never read",
        );
    }

    #[test]
    fn ordered_country_side_failed_binding_keeps_the_live_vector_until_reset() {
        use crate::rules::process_owner::{NativeRulesProcessOwner, NativeScenarioRulesPrefix};

        let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
            IniFile::from_str("[Countries]\n0=Alpha\n"),
            None,
            IniFile::empty(),
            std::sync::Arc::default(),
            Default::default(),
        )
        .expect("cold Rules owner");
        let broken = IniFile::from_str("[Countries]\n0=Alpha\n[Sides]\nBroken=<random>\n");
        let empty_map = IniFile::empty();
        let error = owner
            .load_scenario(
                NativeScenarioRulesPrefix::Campaign(Some(&broken)),
                &empty_map,
            )
            .err()
            .expect("native Country[-2] binding is outside admitted Rules sources");
        assert!(matches!(
            error,
            RulesError::InvalidValue { section, key, value, .. }
                if section == "Sides" && key == "Broken" && value == "-2"
        ));

        // This next real owner transition resolves the retained Side, then
        // expands its actual signed vector. Losing the failed pass's registry
        // or replacing its vector with empty would incorrectly succeed here.
        let probe = IniFile::from_str("[Sides]\nProbe=Broken\n");
        let error = owner
            .load_scenario(
                NativeScenarioRulesPrefix::Campaign(Some(&probe)),
                &empty_map,
            )
            .err()
            .expect("a failed Process retains its actual Side vector");
        assert!(matches!(
            error,
            RulesError::InvalidValue { section, key, value, .. }
                if section == "Sides" && key == "Probe" && value == "-2"
        ));

        let repair = IniFile::from_str("[Sides]\nBroken=Alpha\n");
        let loaded = owner
            .load_scenario(
                NativeScenarioRulesPrefix::Campaign(Some(&repair)),
                &empty_map,
            )
            .expect("the corrected vector reaches the native destructive reset");
        let (rules, _, _, _) = loaded.into_parts();
        assert_eq!(rules.country_index("Alpha"), Some(CountryIdx(0)));
        assert_eq!(rules.country_side_index("Alpha"), None);
        assert_eq!(rules.side_index("Broken"), None);
        assert_eq!(rules.side_index("Probe"), None);
    }

    #[test]
    fn test_load_ruleset() {
        let ini: IniFile = IniFile::from_str(&make_test_rules());
        let rules: RuleSet = RuleSet::from_ini(&ini).expect("Should parse");

        assert_eq!(rules.infantry_ids.len(), 2);
        assert_eq!(rules.vehicle_ids.len(), 1);
        assert_eq!(rules.aircraft_ids.len(), 0);
        assert_eq!(rules.building_ids.len(), 1);
        assert_eq!(rules.object_count(), 4); // E1, E2, MTNK, GAPOWR
        // ReadDouble scans a float and widens it; float fields store it back.
        let float = |value: f32| NativeF32Bits::from_bits(value.to_bits());
        assert_eq!(
            rules.production.build_speed,
            NativeF64Bits::from_bits(f64::from(0.75_f32).to_bits())
        );
        assert_eq!(rules.production.multiple_factory, float(0.7));
        assert_eq!(rules.production.low_power_penalty_modifier, float(1.25));
        assert_eq!(rules.production.min_low_power_production_speed, float(0.4));
        assert_eq!(rules.production.max_low_power_production_speed, float(0.85));
        assert_eq!(rules.bridge_rules.strength, 1000);
        assert!(rules.bridge_rules.destroyable_by_default);
    }

    /// `HouseClass @ 0x0050C0A0` takes the country's build-time multiplier by
    /// WhatAmI: infantry `+0x134`, units `+0x138`, aircraft `+0x13C`, and a
    /// building's `+0x140`, or `+0x144` when its BuildCat is 5, Combat
    /// (`0x0050C0F0`). An absent key or an unknown country gives 1.0.
    #[test]
    fn country_build_time_mults_follow_the_native_slots() {
        let ini = IniFile::from_str(
            "[Countries]\n0=T\n1=U\n\
             [T]\nBuildTimeInfantryMult=0.5\nBuildTimeUnitsMult=0.75\n\
             BuildTimeAircraftMult=1.25\nBuildTimeBuildingsMult=1.5\n\
             BuildTimeDefensesMult=2.0\n\
             [U]\nBuildTimeUnitsMult=0.8\n\
             [InfantryTypes]\n0=I\n[VehicleTypes]\n0=V\n[AircraftTypes]\n0=A\n\
             [BuildingTypes]\n0=B\n1=D\n2=P\n\
             [I]\nStrength=1\n[V]\nStrength=1\n[A]\nStrength=1\n[B]\nStrength=1\n\
             [D]\nStrength=1\nBuildCat=Combat\n[P]\nStrength=1\nBuildCat=Power\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("country fixture parses");
        let float = |value: f32| NativeF32Bits::from_bits(value.to_bits());
        let mult = |country: &str, object: &str| {
            rules.country_build_time_mult_for_type(country, rules.object(object).unwrap())
        };
        for (object, expected) in [
            ("I", 0.5),
            ("V", 0.75),
            ("A", 1.25),
            ("B", 1.5),
            ("D", 2.0),
            ("P", 1.5),
        ] {
            assert_eq!(mult("T", object), float(expected), "{object}");
        }
        assert_eq!(mult("U", "V"), float(0.8));
        assert_eq!(mult("U", "I"), NativeF32Bits::ONE);
        assert_eq!(mult("Nowhere", "V"), NativeF32Bits::ONE);
    }

    #[test]
    fn gsi_13_10_extra_object_lights_default_zero_and_parse_signed_truncated_milliunits() {
        let defaults = GeneralRules::from_ini(&IniFile::from_str(
            "[General]\n\
             FixtureOnly=1\n\
             ExtraUnitLight=9.0\n\
             ExtraInfantryLight=8.0\n\
             ExtraAircraftLight=7.0\n",
        ));
        assert_eq!(defaults.extra_unit_light, 0);
        assert_eq!(defaults.extra_infantry_light, 0);
        assert_eq!(defaults.extra_aircraft_light, 0);

        let parsed = GeneralRules::from_ini(&IniFile::from_str(
            "[General]\n\
             FixtureOnly=1\n\
             ExtraUnitLight=9.0\n\
             ExtraInfantryLight=8.0\n\
             ExtraAircraftLight=7.0\n\
             [AudioVisual]\n\
             ExtraUnitLight=.2\n\
             ExtraInfantryLight=-.1259\n\
             ExtraAircraftLight=.3339\n",
        ));
        assert_eq!(parsed.extra_unit_light, 200);
        assert_eq!(parsed.extra_infantry_light, -125);
        assert_eq!(parsed.extra_aircraft_light, 333);

        let without_general = GeneralRules::from_ini(&IniFile::from_str(
            "[AudioVisual]\nExtraUnitLight=.2\nExtraInfantryLight=-.1259\nExtraAircraftLight=.3339\n",
        ));
        assert_eq!(without_general.extra_unit_light, parsed.extra_unit_light);
        assert_eq!(
            without_general.extra_infantry_light,
            parsed.extra_infantry_light
        );
        assert_eq!(
            without_general.extra_aircraft_light,
            parsed.extra_aircraft_light
        );
    }

    #[test]
    fn gsi_04_04_cliff_back_rule_stores_the_ini_integer_low_byte() {
        for (raw, expected) in [(2, 2), (258, 2), (-1, 255), (256, 0)] {
            let ini = IniFile::from_str(&format!("[General]\nCliffBackImpassability={raw}\n"));
            assert_eq!(
                GeneralRules::from_ini(&ini).cliff_back_impassability,
                expected,
                "raw INI integer {raw}"
            );
        }
    }

    #[test]
    fn gsi_04_05_base_defense_response_rules_preserve_native_defaults_and_ini_values() {
        let defaults = GeneralRules::default();
        assert_eq!(defaults.computer_base_defense_response, 3);
        assert_eq!(defaults.base_defense_delay_minutes, 0.25);
        assert_eq!(defaults.suspend_priority, 20);
        assert_eq!(defaults.suspend_delay_minutes, 2.0);

        let parsed = GeneralRules::from_ini(&IniFile::from_str(
            "[General]\n\
             BaseDefenseDelay=.125\n\
             SuspendPriority=-2\n\
             SuspendDelay=1.5\n\
             [AI]\nComputerBaseDefenseResponse=-4\n",
        ));
        assert_eq!(parsed.computer_base_defense_response, -4);
        assert_eq!(parsed.base_defense_delay_minutes, 0.125_f32 as f64);
        assert_eq!(parsed.suspend_priority, -2);
        assert_eq!(parsed.suspend_delay_minutes, 1.5_f32 as f64);
    }

    /// Original Rules ReadAI673E41..673E61 uses the AI section pointer;
    /// a later General-only pass cannot overwrite the retained AI value.
    /// Executable controls are saved by foot_navigation_coordinate --base-response.
    #[test]
    fn native_base_response_reads_ai_section_across_rules_passes() {
        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[General]\nComputerBaseDefenseResponse=17\n\
             [AI]\nComputerBaseDefenseResponse=5\n",
        ));
        layers.push(
            crate::rules::native_processing::RulesLayerKind::Scenario,
            IniFile::from_str("[General]\nComputerBaseDefenseResponse=19\n"),
        );
        let rules = RuleSet::from_rules_layers(&layers).unwrap();
        assert_eq!(rules.general.computer_base_defense_response, 5);
    }

    /// Original TechnoType ReadINI71464A..71469F retains the current speed
    /// when a later rules pass reads -1. The constructor starts it at zero.
    /// Executed reader history: spatial_oracle/base_defense_response.json.
    #[test]
    fn native_base_response_type_speed_retains_minus_one_across_rules_passes() {
        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[VehicleTypes]\n0=V\n[V]\nStrength=100\nSpeed=7\n",
        ));
        layers.push(
            crate::rules::native_processing::RulesLayerKind::Scenario,
            IniFile::from_str("[V]\nSpeed=-1\n"),
        );
        let rules = RuleSet::from_rules_layers(&layers).unwrap();
        assert_eq!(rules.object("V").unwrap().speed, 7);
        assert_eq!(
            crate::util::fixed_math::ra2_speed_to_leptons_per_frame(
                rules.object("V").unwrap().speed,
            ),
            17,
        );
    }

    #[test]
    fn gsi_04_05_base_plan_rules_preserve_signed_retry_limit_and_building_identity() {
        assert_eq!(
            GeneralRules::default().maximum_building_placement_failures,
            5
        );
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[General]\nMaximumBuildingPlacementFailures=-4\n\
             [BuildingTypes]\n0=GAPOWR\n1=GACNST\n\
             [GAPOWR]\nStrength=750\nIsBaseDefense=yes\n\
             [GACNST]\nStrength=1000\nUndeploysInto=AMCV\n",
        ))
        .expect("base-plan rules");

        assert_eq!(rules.general.maximum_building_placement_failures, -4);
        assert_eq!(rules.building_type_index("gapowr"), Some(0));
        assert_eq!(rules.building_type_index("GACNST"), Some(1));
        let defense = rules
            .object_in_category(ObjectCategory::Building, "GAPOWR")
            .unwrap();
        assert_eq!(defense.base_plan_type_index, 0);
        assert!(defense.is_base_defense);
        assert!(
            rules
                .object_in_category(ObjectCategory::Building, "GACNST")
                .unwrap()
                .undeploys_into
                .is_some()
        );
    }

    #[test]
    fn parse_multiplayer_ai_cm_list_in_source_order() {
        // rulesmd.ini:88 `MultiplayerAICM=400,0,0` (Hard, Normal, Easy).
        let ini = IniFile::from_str("[General]\nMultiplayerAICM=400,0,0\n");
        let general = GeneralRules::from_ini(&ini);
        assert_eq!(general.multiplayer_ai_cm, vec![400, 0, 0]);

        // RulesClass constructor leaves the +0x1304 vector empty (0x00667034).
        let ini = IniFile::from_str("[General]\n");
        let general = GeneralRules::from_ini(&ini);
        assert!(general.multiplayer_ai_cm.is_empty());
    }

    #[test]
    fn parse_tier1_superweapon_rules() {
        let ini_text = "[General]\n\
ForceShieldRadius=5\n\
ForceShieldDuration=600\n\
MutateExplosion=no\n\
[CombatDamage]\n\
IronCurtainDuration=900\n\
PsychicRevealRadius=12\n\
TreeTargeting=yes\n\
[SpecialWeapons]\n\
MutateWarhead=MyMutate\n\
";
        let ini = IniFile::from_str(ini_text);
        let general = RuleSet::from_ini(&ini).unwrap().general;
        assert_eq!(general.iron_curtain_duration, 900);
        assert_eq!(general.force_shield_radius, 5);
        assert_eq!(general.force_shield_duration, 600);
        assert_eq!(general.psychic_reveal_radius, 12);
        assert_eq!(general.mutate_warhead, "MyMutate");
        assert!(general.tree_targeting);
        assert!(!general.mutate_explosion);
        // Unspecified keys fall back to defaults: the constructor's null type
        // (`Rules+0x348`, `0x00665B1A`).
        assert_eq!(general.iron_curtain_invoke_anim, "");
        // The constructor's null type (`Rules+0x34C`, `0x00665B20`).
        assert_eq!(general.force_shield_invoke_anim, "");
        assert_eq!(general.force_shield_blackout_duration, 800);
        assert_eq!(general.force_shield_fade_sound_time, 50);
        // The constructor's null warhead (`Rules+0xF9C`, `0x00666B40`).
        assert_eq!(general.mutate_explosion_warhead, "");
    }

    #[test]
    fn gsi_04_20_ambient_transition_rules_use_native_scales_and_nonzero_gate() {
        let defaults = GeneralRules::default();
        assert!(defaults.ambient_change_rate_nonzero);
        assert_eq!(defaults.ambient_change_interval_frames, 180);
        assert_eq!(defaults.ambient_change_step, 10);

        let stock = RuleSet::from_ini(&IniFile::from_str(
            "[AudioVisual]\nAmbientChangeRate=.2\nAmbientChangeStep=.2\n",
        ))
        .unwrap()
        .general;
        assert!(stock.ambient_change_rate_nonzero);
        assert_eq!(stock.ambient_change_interval_frames, 180);
        assert_eq!(stock.ambient_change_step, 20);

        let tiny = RuleSet::from_ini(&IniFile::from_str(
            "[AudioVisual]\nAmbientChangeRate=.0009\nAmbientChangeStep=.019\n",
        ))
        .unwrap()
        .general;
        assert!(tiny.ambient_change_rate_nonzero);
        assert_eq!(tiny.ambient_change_interval_frames, 0);
        assert_eq!(tiny.ambient_change_step, 1);

        let disabled = RuleSet::from_ini(&IniFile::from_str(
            "[AudioVisual]\nAmbientChangeRate=0\nAmbientChangeStep=.2\n",
        ))
        .unwrap()
        .general;
        assert!(!disabled.ambient_change_rate_nonzero);
        assert_eq!(disabled.ambient_change_interval_frames, 0);
    }

    #[test]
    fn parse_rules_rocking_coefficients_defaults() {
        // [General] must be present, otherwise GeneralRules::from_ini bails to
        // Self::default(). Missing AudioVisual keys then fall back to defaults.
        let ini = IniFile::from_str("[General]\nFixtureOnly=1\n[AudioVisual]\nFixtureOnly=1\n");
        let r = GeneralRules::from_ini(&ini);
        assert_eq!(r.direct_rocking_coefficient, SimFixed::lit("1.5"));
        assert_eq!(r.fallback_coefficient, SimFixed::lit("0.1"));
    }

    #[test]
    fn parse_spark_gravity_preserves_signed_integer_storage() {
        assert_eq!(GeneralRules::default().gravity, 3);
        let gravity = |text: &str| {
            RuleSet::from_ini(&IniFile::from_str(text))
                .unwrap()
                .general
                .gravity
        };
        // Gravity lives in [AudioVisual] (stock rulesmd.ini), NOT [General].
        assert_eq!(
            gravity("[General]\nFlightLevel=500\n[AudioVisual]\nGravity=6\n"),
            6
        );
        assert_eq!(
            gravity("[General]\nFlightLevel=500\n[AudioVisual]\nGravity=-7\n"),
            -7
        );
        // A [General] Gravity is ignored (the engine reads it in ReadAudioVisual).
        assert_eq!(gravity("[General]\nGravity=9\n"), 3);
    }

    #[test]
    fn item82_scroll_multiplier_parses_from_audio_visual_with_stock_default() {
        assert_eq!(GeneralRules::default().scroll_multiplier, 0.07);
        let parsed = GeneralRules::from_ini(&IniFile::from_str(
            "[General]\nFlightLevel=500\n[AudioVisual]\nScrollMultiplier=.125\n",
        ));
        assert_eq!(parsed.scroll_multiplier, 0.125);
        let absent = GeneralRules::from_ini(&IniFile::from_str(
            "[General]\nFlightLevel=500\n[AudioVisual]\n",
        ));
        assert_eq!(absent.scroll_multiplier, 0.07);
    }

    #[test]
    fn gsi_01_04_savour_delay_parses_from_audio_visual_with_native_default() {
        // The already-executed C4 scalar controls exercise the same double
        // *900/Math__ftol body and FPCW as FlagWin4FCA35/FlagLose4FCC61.
        // This is a bounded arithmetic reuse, not a full Flag-body replay.
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/rules_oracle/read_double_percent.json",
        ))
        .unwrap();
        let compare_scalar = |minutes: f64, raw: Option<&str>| {
            let row = native["c4"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["raw"].as_str() == raw)
                .unwrap();
            assert_eq!(
                format!("{:016x}", minutes.to_bits()),
                row["bits"].as_str().unwrap()
            );
            assert_eq!(
                i64::from(native_minutes_to_frames(minutes)),
                row["fuse"].as_i64().unwrap()
            );
        };
        let ctor_default = GeneralRules::default().savour_delay_minutes;
        assert_eq!(ctor_default, 0.03);
        compare_scalar(ctor_default, None);
        let stock = GeneralRules::from_ini(&IniFile::from_str(
            "[General]\nFlightLevel=500\n[AudioVisual]\nSavourDelay=.1\n",
        ));
        assert_eq!(stock.savour_delay_minutes, 0.1_f32 as f64);
        compare_scalar(stock.savour_delay_minutes, Some("0.1"));
        let explicit_point_zero_three = GeneralRules::from_ini(&IniFile::from_str(
            "[General]\nFlightLevel=500\n[AudioVisual]\nSavourDelay=.03\n",
        ));
        assert_eq!(
            explicit_point_zero_three.savour_delay_minutes,
            0.03_f32 as f64
        );
        compare_scalar(explicit_point_zero_three.savour_delay_minutes, Some(".03"));
        let absent = GeneralRules::from_ini(&IniFile::from_str(
            "[General]\nFlightLevel=500\n[AudioVisual]\n",
        ));
        assert_eq!(absent.savour_delay_minutes, 0.03);
        compare_scalar(absent.savour_delay_minutes, None);
    }

    #[test]
    fn retail_savour_delay_uses_production_audio_visual_reader() {
        let Some(ini) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
            return;
        };
        let parsed = GeneralRules::from_ini(&ini);
        assert_eq!(
            parsed.savour_delay_minutes.to_bits(),
            (0.1_f32 as f64).to_bits()
        );
        assert_eq!(native_minutes_to_frames(parsed.savour_delay_minutes), 90);
    }

    #[test]
    fn shroud_growth_controls_retain_native_defaults_through_process_reset_and_save() {
        assert!(!GeneralRules::default().shroud_grow);
        assert_eq!(GeneralRules::default().shroud_rate, default_shroud_rate());
        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[AudioVisual]\nShroudGrow=yes\nShroudRate=.5\n",
        ));
        layers.push(
            crate::rules::native_processing::RulesLayerKind::Scenario,
            IniFile::from_str("[AudioVisual]\nshroudgrow=no\n"),
        );
        let processed = layers.process().unwrap();
        let (_, trace) = processed.into_ini_and_native_type_construction_trace();
        let retained = trace
            .into_registry_state_discarding_events()
            .destructive_reset();
        let saved = crate::util::save_codec::serialize(&retained).unwrap();
        let retained = crate::util::save_codec::deserialize(&saved).unwrap();
        let after_reset = RulesLayerStack::new(IniFile::empty())
            .process_with_fixed_art_and_registry_state(&IniFile::empty(), retained)
            .unwrap();
        let rules = RuleSet::from_processed_rules(&after_reset).unwrap();
        assert!(rules.general.shroud_grow);
        assert_eq!(rules.general.shroud_rate.bits(), (0.5_f32 as f64).to_bits());
        assert_eq!(rules.general.shroud_growth_interval_frames(), 450);
        if let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() {
            assert!(!retail.rules.general.shroud_grow);
            assert_eq!(retail.rules.general.shroud_rate, default_shroud_rate());
            assert_eq!(retail.rules.general.shroud_growth_interval_frames(), 3600);
        }
    }

    #[test]
    fn fog_configuration_preserves_native_read_defaults_and_campaign_reset() {
        use crate::map::basic::parse_special_flags_section;

        assert_eq!(GeneralRules::default().fog_rate, default_fog_rate());
        let mut layers = RulesLayerStack::new(IniFile::from_str("[AudioVisual]\nFogRate=.05\n"));
        layers.push(
            crate::rules::native_processing::RulesLayerKind::Scenario,
            IniFile::from_str("[AudioVisual]\nfograte=invalid\n"),
        );
        let processed = layers.process().unwrap();
        let (_, trace) = processed.into_ini_and_native_type_construction_trace();
        let retained = trace
            .into_registry_state_discarding_events()
            .destructive_reset();
        let after_reset = RulesLayerStack::new(IniFile::empty())
            .process_with_fixed_art_and_registry_state(&IniFile::empty(), retained)
            .unwrap();
        let rules = RuleSet::from_processed_rules(&after_reset).unwrap();
        assert_eq!(rules.general.fog_rate.bits(), (0.05_f32 as f64).to_bits());

        let prior =
            parse_special_flags_section(&IniFile::from_str("[SpecialFlags]\nFogOfWar=yes\n"));
        assert_eq!(prior.fog_of_war, Some(true));
        let mut fresh = parse_special_flags_section(&IniFile::empty());
        fresh.resolve_campaign_defaults(Some(&prior));
        assert_eq!(fresh.fog_of_war, Some(false));
        let mut authored = prior.clone();
        authored.resolve_campaign_defaults(Some(&fresh));
        assert_eq!(authored.fog_of_war, Some(true));

        if let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() {
            assert_eq!(
                retail.rules.general.fog_rate.bits(),
                (0.01_f32 as f64).to_bits(),
            );
            assert_eq!(retail.rules.general.fog_regrowth_interval_frames(), 8);
        }
    }

    #[test]
    fn parse_rules_rocking_coefficients_explicit() {
        let ini = IniFile::from_str(
            "[General]\nFlightLevel=500\n[AudioVisual]\nDirectRockingCoefficient=2.0\nFallBackCoefficient=0.05\n",
        );
        let r = GeneralRules::from_ini(&ini);
        assert_eq!(r.direct_rocking_coefficient, SimFixed::lit("2"));
        assert_eq!(r.fallback_coefficient, SimFixed::lit("0.05"));
    }

    #[test]
    fn parse_retail_rules_rocking_coefficients() {
        let ini = IniFile::from_str(
            "[AudioVisual]\nDirectRockingCoefficient=1.5\nFallBackCoefficient=0.1\n",
        );
        let r = GeneralRules::from_ini(&ini);
        assert_eq!(r.direct_rocking_coefficient, SimFixed::lit("1.5"));
        assert_eq!(r.fallback_coefficient, SimFixed::lit("0.1"));
    }

    #[test]
    fn test_object_lookup() {
        let ini: IniFile = IniFile::from_str(&make_test_rules());
        let art = IniFile::from_str("[GAPOWR]\nFoundation=2x2\n");
        let rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).expect("Should parse");

        let e1: &ObjectType = rules.object("E1").expect("E1 exists");
        assert_eq!(e1.cost, 200);
        assert_eq!(e1.strength, 125);
        assert_eq!(e1.category, ObjectCategory::Infantry);
        assert_eq!(e1.primary(), Some("M60"));
        assert_eq!(
            e1.build_time_multiplier,
            NativeF32Bits::from_bits(1.15_f32.to_bits())
        );

        let mtnk: &ObjectType = rules.object("MTNK").expect("MTNK exists");
        assert_eq!(mtnk.cost, 700);
        assert_eq!(mtnk.category, ObjectCategory::Vehicle);
        assert_eq!(mtnk.secondary(), Some("MachGun"));

        let gapowr: &ObjectType = rules.object("GAPOWR").expect("GAPOWR exists");
        assert_eq!(gapowr.power, 200);
        assert_eq!(gapowr.foundation, "2x2");
    }

    #[test]
    fn test_weapon_and_warhead_loading() {
        let ini: IniFile = IniFile::from_str(&make_test_rules());
        let rules: RuleSet = RuleSet::from_ini(&ini).expect("Should parse");

        // Weapons referenced by objects should be loaded.
        let m60: &WeaponType = rules.weapon("M60").expect("M60 exists");
        assert_eq!(m60.damage, 25);
        assert_eq!(m60.warhead, Some("SA".to_string()));

        let cannon: &WeaponType = rules.weapon("105mm").expect("105mm exists");
        assert_eq!(cannon.damage, 65);
        assert_eq!(cannon.warhead, Some("AP".to_string()));
        assert_eq!(cannon.burst, 2);
        assert_eq!(cannon.projectile, Some("InvisibleLow".to_string()));

        // Burst defaults to 1 when not specified.
        assert_eq!(m60.burst, 1);

        // Projectiles referenced by weapons should be loaded.
        assert_eq!(rules.projectile_count(), 1);
        let proj = rules
            .projectile("InvisibleLow")
            .expect("InvisibleLow exists");
        assert!(!proj.aa);
        assert!(proj.ag);

        // Warheads referenced by weapons should be loaded.
        let sa: &WarheadType = rules.warhead("SA").expect("SA exists");
        assert_eq!(
            f64::from_bits(
                sa.interaction(crate::rules::armor::armor_index("none"))
                    .multiplier()
                    .bits()
            ),
            1.0
        );
        assert_eq!(
            f64::from_bits(
                sa.interaction(crate::rules::armor::armor_index("heavy"))
                    .multiplier()
                    .bits()
            ),
            0.25
        );

        let ap: &WarheadType = rules.warhead("AP").expect("AP exists");
        assert!(
            (f64::from_bits(
                ap.interaction(crate::rules::armor::armor_index("wood"))
                    .multiplier()
                    .bits()
            ) - 0.6)
                .abs()
                < 1e-9
        );
    }

    #[test]
    fn registry_only_warhead_loads_through_rules_layer_stack() {
        let mut layers = RulesLayerStack::new(IniFile::from_str("[General]\n"));
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str(
                "[Warheads]\n\
                 0=RegistryOnlyWH\n\
                 [RegistryOnlyWH]\n\
                 CellSpread=2\n\
                 PercentAtMax=.5\n\
                 Verses=100%,90%,80%,70%,60%,50%,40%,30%,20%,10%,0%\n",
            ),
        );

        let rules = RuleSet::from_rules_layers(&layers).expect("layered registry-only rules parse");
        assert_eq!(
            rules.weapon_count(),
            0,
            "the fixture must not smuggle the warhead in through a weapon"
        );
        assert_eq!(rules.warhead_count(), 1);

        let lower = rules
            .warhead("registryonlywh")
            .expect("scenario-created registry warhead resolves case-insensitively");
        let mixed = rules
            .warhead("RegistryOnlyWh")
            .expect("mixed-case registry warhead lookup resolves");
        assert!(std::ptr::eq(lower, mixed));
        assert_eq!(lower.id, "RegistryOnlyWH");
        assert_eq!(lower.percent_at_max_f64, 0.5);
        let expected = [1.0, 0.9, 0.8, 0.7, 0.6, 0.5, 0.4, 0.3, 0.2, 0.1, 0.0];
        for (name, expected) in crate::rules::armor::RETAIL_ARMOR_NAMES
            .into_iter()
            .zip(expected)
        {
            let actual = f64::from_bits(
                lower
                    .interaction(crate::rules::armor::armor_index(name))
                    .multiplier()
                    .bits(),
            );
            assert!((actual - expected).abs() < 1e-9);
        }
    }

    #[test]
    fn refinery_helpers_are_data_driven_and_case_insensitive() {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n\
             [VehicleTypes]\n\
             0=MODHARV\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             0=MODPROC\n\
             1=FAKEREF\n\
             [MODHARV]\n\
             Harvester=yes\n\
             Dock=modproc\n\
             [MODPROC]\n\
             Refinery=yes\nDockUnload=yes\n\
             FreeUnit=modharv\n\
             [FAKEREF]\n\
             Name=Fake Refinery\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("Should parse");

        assert!(rules.object("modproc").is_some_and(|obj| obj.refinery));
        assert!(!rules.object("FAKEREF").is_some_and(|obj| obj.refinery));
        assert_eq!(rules.building_free_unit("MODPROC"), Some("MODHARV"));
        assert!(rules.harvester_can_dock_at("modharv", "MODPROC"));
        assert!(!rules.harvester_can_dock_at("modharv", "GAREFN"));
    }

    /// `BuildingTypeClass::ReadINI @ 0x0045FE50` reads `FreeUnit` through
    /// `UnitTypeClass::Find_Or_Allocate`, so an unregistered name constructs an
    /// empty UnitType instead of being dropped (see
    /// `RULES_PROCESS_NATIVE_ID_CONSTRUCTOR_CHRONOLOGY_REINVESTIGATION_GHIDRA_REPORT.md`).
    #[test]
    fn refinery_free_unit_allocates_missing_target_like_native() {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n\
             [VehicleTypes]\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             0=MODPROC\n\
             [MODPROC]\n\
             Refinery=yes\nDockUnload=yes\n\
             FreeUnit=UNKNOWN\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("Should parse");

        assert!(rules.object("MODPROC").is_some_and(|obj| obj.refinery));
        assert_eq!(rules.building_free_unit("MODPROC"), Some("UNKNOWN"));
        assert!(
            rules.type_handle("UNKNOWN").is_some(),
            "native Find_Or_Allocate constructs the referenced UnitType"
        );
    }

    #[test]
    fn harvester_scan_radii_parsed_from_general() {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n\
             [VehicleTypes]\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [General]\n\
             TiberiumShortScan=10\n\
             TiberiumLongScan=6.5\n\
             HarvesterLoadRate=3\n\
             SlaveMinerShortScan=12\n\
             SlaveMinerSlaveScan=20\n\
             SlaveMinerLongScan=55\n\
             SlaveMinerScanCorrection=5\n\
             SlaveMinerKickFrameDelay=200\n\
             HarvesterTooFarDistance=8\n\
             MaximumQueuedObjects=$1D\n\
             ChronoHarvTooFarDistance=40\n\
             ApproachTargetResetMultiplier=1.5\n\
             PurifierBonus=.30\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("Should parse");
        // ReadRange: cells x 256, chopped.
        assert_eq!(rules.general.tiberium_short_scan, 10 * 256);
        assert_eq!(rules.general.tiberium_long_scan, 1664);
        assert_eq!(rules.general.harvester_load_rate, 3);
        assert_eq!(rules.general.slave_miner_short_scan, 12 * 256);
        assert_eq!(rules.general.slave_miner_slave_scan, 20 * 256);
        assert_eq!(rules.general.slave_miner_long_scan, 55 * 256);
        assert_eq!(rules.general.slave_miner_scan_correction, 5 * 256);
        assert_eq!(rules.general.slave_miner_kick_frame_delay, 200);
        // ReadInt stops at the decimal point.
        assert_eq!(rules.general.approach_target_reset_multiplier, 1);
        assert_eq!(rules.general.harvester_too_far_distance, 8);
        assert_eq!(
            rules.general.maximum_queued_objects, 29,
            "ReadInt takes `$` hex"
        );
        assert_eq!(rules.general.chrono_harv_too_far_distance, 40);
        assert_eq!(rules.general.purifier_bonus.bits(), 0x3E99_999A);
    }

    #[test]
    fn harvester_scan_radii_use_defaults_when_missing() {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n\
             [VehicleTypes]\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [General]\n\
             FixtureOnly=1\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("Should parse");
        // The RulesClass constructor's leptons (`0x00667638`, `0x00667642`).
        assert_eq!(rules.general.tiberium_short_scan, 0x600);
        assert_eq!(rules.general.tiberium_long_scan, 0x2000);
        assert_eq!(rules.general.harvester_load_rate, 2);
        assert_eq!(rules.general.slave_miner_short_scan, 0x500);
        assert_eq!(rules.general.slave_miner_slave_scan, 0x1000);
        assert_eq!(rules.general.slave_miner_long_scan, 0x5000);
        assert_eq!(rules.general.slave_miner_scan_correction, 0x300);
        assert_eq!(rules.general.slave_miner_kick_frame_delay, 0x7FFF_FFFF);
        assert_eq!(rules.general.approach_target_reset_multiplier, 1);
        assert_eq!(rules.general.harvester_too_far_distance, 5);
        assert_eq!(rules.general.maximum_queued_objects, 5);
        assert_eq!(rules.general.chrono_harv_too_far_distance, 50);
        assert_eq!(rules.general.purifier_bonus.bits(), 0x3E80_0000);
    }

    /// ReadDouble -> FSTP float retains the exact stored binary32, including
    /// fractions that neither whole percent nor PPM represent exactly.
    #[test]
    fn purifier_bonus_keeps_fractional_precision() {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n\
             [VehicleTypes]\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [General]\n\
             PurifierBonus=.333\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("Should parse");
        assert_eq!(rules.general.purifier_bonus.bits(), 0x3EAA_7EFA);
        assert_ne!(rules.general.purifier_bonus.bits(), 0x3EA8_F5C2);
    }

    #[test]
    fn from_ini_loads_tibtre_terrain_object_types() {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n\
             [VehicleTypes]\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [TerrainTypes]\n1=TIBTRE01\n2=TREE01\n\
             [TIBTRE01]\nSpawnsTiberium=yes\nIsAnimated=yes\n\
             AnimationRate=3\nAnimationProbability=.003\n\
             [TREE01]\nIsAnimated=no\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("rules parse");
        let t = rules
            .terrain_object_type_case_insensitive("tibtre01")
            .expect("TIBTRE01 should be parsed");
        assert!(t.spawns_tiberium);
        assert_eq!(t.animation_probability.bits(), 0x3b44_9ba6);
        // TREE01 also parsed but with default flags.
        let tree = rules
            .terrain_object_type_case_insensitive("TREE01")
            .expect("TREE01 should be parsed");
        assert!(!tree.spawns_tiberium);
    }

    #[test]
    fn bridge_animation_vectors_have_empty_constructor_defaults() {
        assert!(GeneralRules::default().metallic_debris.is_empty());
        assert!(BridgeRules::default().explosions.is_empty());
        let rules = RuleSet::from_ini(&IniFile::empty()).unwrap();
        assert!(rules.general.metallic_debris.is_empty());
        assert!(rules.bridge_rules.explosions.is_empty());
    }

    #[test]
    fn metallic_debris_parses_from_ini() {
        let ini = IniFile::from_str("[General]\nMetallicDebris=ANIM1,ANIM2,ANIM3\n");
        let rules = RuleSet::from_ini(&ini).unwrap();
        assert_eq!(
            rules.general.metallic_debris,
            vec!["ANIM1", "ANIM2", "ANIM3"]
        );
    }

    #[test]
    fn resolved_bridge_animation_order_and_duplicates_affect_simulation_hash() {
        let mut rules = RuleSet::from_ini(&IniFile::from_str(
            "[General]\nMetallicDebris=A,B,A\nBridgeExplosions=X,Y\n",
        ))
        .unwrap();
        let original = rules.simulation_config_hash();
        rules.general.metallic_debris.swap(0, 1);
        assert_ne!(original, rules.simulation_config_hash());
        rules.general.metallic_debris.swap(0, 1);
        rules.general.metallic_debris.pop();
        assert_ne!(original, rules.simulation_config_hash());
        rules.general.metallic_debris.push("A".to_owned());
        assert_eq!(original, rules.simulation_config_hash());
        rules.bridge_rules.explosions.reverse();
        assert_ne!(original, rules.simulation_config_hash());
    }

    #[test]
    fn bridge_rules_load_from_ini() {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n\
             [VehicleTypes]\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [General]\n\
             BridgeVoxelMax=5\n\
             [AudioVisual]\n\
             RepairBridgeSound=foo\n\
             [CombatDamage]\n\
             BridgeStrength=900\n\
             DestroyableBridges=no\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("Should parse");
        assert_eq!(rules.bridge_rules.strength, 900);
        assert!(rules.bridge_rules.destroyable_by_default);
        assert_eq!(rules.bridge_rules.voxel_max, 5);
        assert_eq!(rules.bridge_rules.repair_sound.as_deref(), Some("FOO"));
    }

    #[test]
    fn bridge_rules_voxel_max_clamps_oversize() {
        // Regression: u8 storage clamps oversize INI values to 255 instead
        // of wrapping/truncating.
        let ini = IniFile::from_str(
            "[InfantryTypes]\n\
             [VehicleTypes]\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [General]\n\
             BridgeVoxelMax=999\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("Should parse");
        assert_eq!(rules.bridge_rules.voxel_max, 255);
    }

    #[test]
    fn bridge_rules_destroyable_in_specialflags_is_ignored() {
        // Map `[SpecialFlags]` is parsed with map data, not rules.ini.
        let ini = IniFile::from_str(
            "[InfantryTypes]\n\
             [VehicleTypes]\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [SpecialFlags]\n\
             DestroyableBridges=no\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("Should parse");
        assert!(rules.bridge_rules.destroyable_by_default);
    }

    /// `RulesClass::ReadGeneral @ 0x0066D530` reads the four rank multipliers
    /// as doubles (`+0x670`, `+0x678`, `+0x680`, `+0x690`) and `RepairRate`
    /// (`+0x16E0`); `ReadAudioVisual @ 0x006691E0` resolves the two upgrade
    /// sounds and `EliteFlashTimer` (`+0xBE8`).
    #[test]
    fn gsi_08_12_veterancy_multipliers_and_promotion_cues_parse() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
             [General]\nVeteranCombat=1.1\nVeteranSpeed=1.2\nVeteranSight=0.0\nVeteranROF=0.6\nRepairRate=.016\n\
             [AudioVisual]\nUpgradeVeteranSound=UpgradeVeteran\nUpgradeEliteSound=UpgradeElite\nEliteFlashTimer=150\n",
        ))
        .unwrap();
        // `ReadDouble` is a `%f` single widened to a double, so the stored
        // multipliers are the f32-widened values, not the decimal literals.
        assert_eq!(rules.general.veteran_combat, f64::from(1.1f32));
        assert_eq!(rules.general.veteran_speed, f64::from(1.2f32));
        assert_eq!(rules.general.veteran_sight, 0.0);
        assert_eq!(rules.general.veteran_rof, f64::from(0.6f32));
        assert_eq!(rules.general.repair_rate_minutes, f64::from(0.016f32));
        // The widened singles sit clear of every integer boundary the stock
        // consumers reach: the products below truncate the same way at any
        // x87 precision.
        use crate::sim::combat::veterancy::{ftol_scale, self_heal_interval_frames};
        assert_eq!(ftol_scale(50, rules.general.veteran_rof), 30);
        assert_eq!(ftol_scale(51, rules.general.veteran_rof), 30);
        assert_eq!(ftol_scale(52, rules.general.veteran_rof), 31);
        assert_eq!(ftol_scale(65, rules.general.veteran_combat), 71);
        assert_eq!(ftol_scale(15, rules.general.veteran_speed), 18);
        assert_eq!(
            self_heal_interval_frames(rules.general.repair_rate_minutes),
            14
        );
        assert_eq!(rules.general.elite_flash_timer, 150);
        let bare = RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n[General]\nFixtureOnly=1\n",
        ))
        .unwrap();
        assert_eq!(bare.general.veteran_rof, 1.0);
        assert_eq!(bare.general.upgrade_veteran_sound, None);
        // Rules6656A7..6656B0 supplies the retained ReadInt defaults.
        assert_eq!(bare.general.self_heal_infantry_frames, 1000);
        assert_eq!(bare.general.self_heal_infantry_amount, 1);
        assert_eq!(bare.general.self_heal_unit_frames, 1000);
        assert_eq!(bare.general.self_heal_unit_amount, 1);
    }

    /// Original constructor6656A7..6656B0 and ReadGeneral66E6E2..66E74B.
    /// The retained corpus distinguishes physical empty-value omission from
    /// an explicitly prepared stored-empty native cache entry.
    #[test]
    fn house_self_heal_defaults_and_retained_readers_match_native_execution() {
        use crate::rules::native_processing::RulesLayerKind;

        let native =
            crate::test_fixture::json("tools/procedural_drawing_oracle/self_heal_status.json");
        let controls = &native["reader_controls"];
        let state = |general: &GeneralRules| {
            serde_json::json!({
                "SelfHealInfantryFrames": general.self_heal_infantry_frames,
                "SelfHealInfantryAmount": general.self_heal_infantry_amount,
                "SelfHealUnitFrames": general.self_heal_unit_frames,
                "SelfHealUnitAmount": general.self_heal_unit_amount,
            })
        };
        assert_eq!(state(&GeneralRules::default()), controls["constructor"]);
        let mut layers = RulesLayerStack::new(IniFile::empty());
        let mut rules = RuleSet::from_rules_layers(&layers).unwrap();
        assert_eq!(state(&rules.general), controls["constructor"]);
        let rows = controls["retained_history"].as_array().unwrap();
        assert_eq!(rows.len(), 11);
        for row in rows {
            assert_eq!(state(&rules.general), row["before"], "{}", row["name"]);
            let mut pass = IniFile::from_str(row["input_text"].as_str().unwrap());
            if row["raw_cached_values"] == true {
                // This row supplies a native cache, rather than a physical
                // file. Use the existing cache API; do not reinterpret empty
                // authored values that the normal lexical loader omits.
                for (name, keys) in row["sections"].as_object().unwrap() {
                    let mut section = IniSection::new(name.clone());
                    for (key, value) in keys.as_object().unwrap() {
                        section.set(key, value.as_str().unwrap());
                    }
                    pass.replace_first_section(section);
                }
            }
            layers.push(RulesLayerKind::Scenario, pass);
            rules = RuleSet::from_rules_layers(&layers).unwrap();
            assert_eq!(state(&rules.general), row["after"], "{}", row["name"]);
        }
    }

    #[test]
    fn starting_force_initial_veteran_reads_only_specialflags() {
        let general_only = IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
             [General]\nInitialVeteran=yes\n",
        );
        assert!(!RuleSet::from_ini(&general_only).unwrap().initial_veteran);

        let special_flags = IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
             [General]\nInitialVeteran=no\n\
             [SpecialFlags]\nInitialVeteran=yes\n",
        );
        assert!(RuleSet::from_ini(&special_flags).unwrap().initial_veteran);
    }

    #[test]
    fn test_building_garrisoned_sound_parsed() {
        let ini_str = "\
[General]
FlightLevel=500
[AudioVisual]
BuildingGarrisonedSound=BuildingGarrisoned
";
        let ini = IniFile::from_str(ini_str);
        let general = GeneralRules::from_ini(&ini);
        assert_eq!(
            general.building_garrisoned_sound.as_deref(),
            Some("BuildingGarrisoned")
        );
    }

    /// `[AudioVisual] BaseUnderAttackSound=` — the siren
    /// `HouseClass::NotifyUnderAttack` plays at `0x004F95CF` next to the EVA
    /// line.
    #[test]
    fn base_under_attack_sound_parses_and_an_absent_key_is_silence() {
        let stock = GeneralRules::from_ini(&IniFile::from_str(
            "[General]\nFlightLevel=500\n[AudioVisual]\nBaseUnderAttackSound=BaseUnderAttackSiren\n",
        ));
        assert_eq!(
            stock.base_under_attack_sound.as_deref(),
            Some("BaseUnderAttackSiren")
        );

        let absent = GeneralRules::from_ini(&IniFile::from_str(
            "[General]\nFlightLevel=500\n[AudioVisual]\n",
        ));
        assert_eq!(absent.base_under_attack_sound, None);
    }

    /// `[AudioVisual] BuildingDieSound=` — the global building crumble cue
    /// `BuildingClass::DestructionEffects` plays at `0x00441779` when the
    /// dying building's type has no `DieSound=` of its own.
    #[test]
    fn building_die_sound_parses_and_an_absent_key_is_silence() {
        let stock = GeneralRules::from_ini(&IniFile::from_str(
            "[General]\nFlightLevel=500\n[AudioVisual]\nBuildingDieSound=BuildingGenericDie\n",
        ));
        assert_eq!(
            stock.building_die_sound.as_deref(),
            Some("BuildingGenericDie")
        );

        let absent = GeneralRules::from_ini(&IniFile::from_str(
            "[General]\nFlightLevel=500\n[AudioVisual]\n",
        ));
        assert_eq!(absent.building_die_sound, None);

        let empty = GeneralRules::from_ini(&IniFile::from_str(
            "[General]\nFlightLevel=500\n[AudioVisual]\nBuildingDieSound=\n",
        ));
        assert_eq!(empty.building_die_sound, None);
    }

    /// `[AudioVisual] BuildingDamageSound=` — the global struck-building cue
    /// `BuildingClass::ReceiveDamage` plays at `0x00442706` when the damaged
    /// building's type has no `DamageSound=` of its own
    /// (`0x004426D2 CMP [type+0x538],-1`).
    #[test]
    fn building_damage_sound_remains_unbound_without_the_fixed_catalog() {
        let stock = GeneralRules::from_ini(&IniFile::from_str(
            "[General]\nFlightLevel=500\n[AudioVisual]\nBuildingDamageSound=BuildingDamaged\n",
        ));
        assert_eq!(stock.building_damage_sound(), None);

        let absent = GeneralRules::from_ini(&IniFile::from_str(
            "[General]\nFlightLevel=500\n[AudioVisual]\n",
        ));
        assert_eq!(absent.building_damage_sound(), None);

        let empty = GeneralRules::from_ini(&IniFile::from_str(
            "[General]\nFlightLevel=500\n[AudioVisual]\nBuildingDamageSound=\n",
        ));
        assert_eq!(empty.building_damage_sound(), None);
    }

    /// `[AudioVisual] LightningSounds=` goes through `CCINIClass::ReadSoundList
    /// @ 0x00525430`, which tokenises on `","` (`0x00817F70`) after one
    /// `ReadString` trim; empty fields collapse and an absent key leaves the
    /// vector empty, which is the `0x0053A46A JLE` "play nothing" branch.
    #[test]
    fn lightning_sounds_reads_the_native_comma_list() {
        let stock = GeneralRules::from_ini(&IniFile::from_str(
            "[General]\nFlightLevel=500\n[AudioVisual]\nLightningSounds=WeatherStrike\n",
        ));
        assert_eq!(stock.lightning_sounds, vec!["WeatherStrike".to_string()]);

        let multi = GeneralRules::from_ini(&IniFile::from_str(
            "[General]\nFlightLevel=500\n[AudioVisual]\nLightningSounds=A,,B\n",
        ));
        assert_eq!(
            multi.lightning_sounds,
            vec!["A".to_string(), "B".to_string()],
            "strtok collapses the empty field between the two commas"
        );

        let absent = GeneralRules::from_ini(&IniFile::from_str(
            "[General]\nFlightLevel=500\n[AudioVisual]\n",
        ));
        assert!(absent.lightning_sounds.is_empty());

        let empty = GeneralRules::from_ini(&IniFile::from_str(
            "[General]\nFlightLevel=500\n[AudioVisual]\nLightningSounds=\n",
        ));
        assert!(
            empty.lightning_sounds.is_empty(),
            "stock ships an empty IceCrackSounds= in the same shape"
        );
    }

    /// `SuperClass::Launch @ 0x006CC390` reads three `[AudioVisual]` cues
    /// straight off `RulesClass` — `+0x24C` at `0x006CCE22`, `+0x250` at
    /// `0x006CD8CD`, `+0x254` at `0x006CD7B9` — plus `DigSound` (`+0x174`,
    /// `0x006CDCA8`), the nuke siren despite the name.
    #[test]
    fn superweapon_activate_sounds_parse_their_stock_values() {
        let stock = GeneralRules::from_ini(&IniFile::from_str(
            "[General]
FlightLevel=500
[AudioVisual]
             PsychicDominatorActivateSound=PsychicDominatorActivate
             GeneticMutatorActivateSound=GeneticMutatorActivate
             PsychicRevealActivateSound=PsychicRevealActivate
             DigSound=NukeSiren		; HACK: Nuke siren here
             StormSound=WeatherIntro
",
        ));
        assert_eq!(
            stock.psychic_dominator_activate_sound.as_deref(),
            Some("PsychicDominatorActivate")
        );
        assert_eq!(
            stock.genetic_mutator_activate_sound.as_deref(),
            Some("GeneticMutatorActivate")
        );
        assert_eq!(
            stock.psychic_reveal_activate_sound.as_deref(),
            Some("PsychicRevealActivate")
        );
        assert_eq!(
            stock.dig_sound.as_deref(),
            Some("NukeSiren"),
            "the inline comment is cut before the value is trimmed"
        );
        assert_eq!(stock.storm_sound.as_deref(), Some("WeatherIntro"));

        let absent = GeneralRules::from_ini(&IniFile::from_str(
            "[General]
FlightLevel=500
[AudioVisual]
DigSound=
StormSound=
",
        ));
        assert_eq!(absent.psychic_dominator_activate_sound, None);
        assert_eq!(absent.genetic_mutator_activate_sound, None);
        assert_eq!(absent.psychic_reveal_activate_sound, None);
        assert_eq!(absent.dig_sound, None, "an empty value is silence");
        assert_eq!(absent.storm_sound, None);
    }

    /// The Spy Plane's camera keys: the sound (`Rules+0x280`, constructor -1
    /// at `0x006659EE`), resolved against SOUNDMD with the prior index kept
    /// for a missing, empty or unknown name (`0x0066A295..0x0066A2C2`), and
    /// the frames (`Rules+0x290`, constructor 16 at `0x00665A06`, read with
    /// the field as its default at `0x0066A39A`).
    #[test]
    fn spy_plane_camera_keys_read_audio_visual_over_the_constructor() {
        let sounds = crate::rules::sound_ini::SoundRegistry::from_ini(&IniFile::from_str(
            "[SoundList]\n0=SpyPlaneSnapshot\n",
        ));
        let read = |audio_visual: &str| {
            let ini = IniFile::from_str(&format!(
                "[General]\nFlightLevel=500\n[AudioVisual]\n{audio_visual}"
            ));
            let mut rules = RuleSet::from_ini(&ini).unwrap();
            assert_eq!(
                rules.general.spy_plane_camera, None,
                "unbound: the constructor's -1"
            );
            rules.bind_type_sound_references(&ini, &sounds);
            rules.general
        };
        let stock = read("SpyPlaneCamera=SpyPlaneSnapshot\nSpyPlaneCameraFrames=12\n");
        assert_eq!(stock.spy_plane_camera.as_deref(), Some("SpyPlaneSnapshot"));
        assert_eq!(stock.spy_plane_camera_frames, 12);
        for silent in ["SpyPlaneCamera=\n", "SpyPlaneCamera=NoSuchSound\n", ""] {
            let general = read(silent);
            assert_eq!(general.spy_plane_camera, None, "{silent:?} keeps -1");
            assert_eq!(general.spy_plane_camera_frames, 16);
        }
    }

    /// `LightningPrintText` is absent from stock `rulesmd.ini`, so the gate on
    /// `StormSound` is decided by `RulesClass::Constructor @ 0x006676BC`, which
    /// stores `AL` after `0x00667202 MOV EAX,0x1` with no intervening `CALL`
    /// (the function's last is `0x0066715D`).
    #[test]
    fn lightning_print_text_defaults_true_and_reads_the_general_key() {
        let stock = GeneralRules::from_ini(&IniFile::from_str(
            "[General]
FlightLevel=500
",
        ));
        assert!(
            stock.lightning_print_text,
            "the storm cue is audible on stock data"
        );
        let off = GeneralRules::from_ini(&IniFile::from_str(
            "[General]
FlightLevel=500
LightningPrintText=no
",
        ));
        assert!(!off.lightning_print_text);
    }

    #[test]
    fn test_chute_sound_parsed() {
        let ini_str = "\
[General]
FlightLevel=500
[AudioVisual]
ChuteSound=CustomDrop
";
        let ini = IniFile::from_str(ini_str);
        let general = GeneralRules::from_ini(&ini);
        assert_eq!(general.chute_sound.as_deref(), Some("CustomDrop"));
    }

    #[test]
    fn test_stock_chute_sound_parsed() {
        let ini_str = "\
[General]
FlightLevel=500
[AudioVisual]
ChuteSound=ParachuteDrop
";
        let ini = IniFile::from_str(ini_str);
        let general = GeneralRules::from_ini(&ini);
        assert_eq!(general.chute_sound.as_deref(), Some("ParachuteDrop"));
    }

    #[test]
    fn test_stock_chrono_sounds_parsed_from_audiovisual() {
        // Stock ships both keys under [AudioVisual] with value ChronoMinerTeleport.
        let ini_str = "\
[General]
FlightLevel=500
[AudioVisual]
ChronoInSound=ChronoMinerTeleport
ChronoOutSound=ChronoMinerTeleport
";
        let ini = IniFile::from_str(ini_str);
        let general = GeneralRules::from_ini(&ini);
        assert_eq!(
            general.chrono_in_sound.as_deref(),
            Some("ChronoMinerTeleport")
        );
        assert_eq!(
            general.chrono_out_sound.as_deref(),
            Some("ChronoMinerTeleport")
        );
    }

    #[test]
    fn test_chrono_sounds_absent_yield_none() {
        // A genuinely-absent key must be silence (None), not a fabricated
        // fallback. Guards against reading the wrong section or re-adding a
        // hardcoded default. [General] present so from_ini does not early-return.
        let ini_str = "\
[General]
FlightLevel=500
[AudioVisual]
";
        let ini = IniFile::from_str(ini_str);
        let general = GeneralRules::from_ini(&ini);
        assert_eq!(general.chrono_in_sound, None);
        assert_eq!(general.chrono_out_sound, None);
    }

    #[test]
    fn harvester_dump_frames_uses_ceil_of_rate_times_900_not_tenths() {
        // The dump gate crosses at ceil(HarvesterDumpRate × 900). The old code
        // quantized to tenths and could cross a frame early for modded rates.
        let frames = |line: &str| {
            let ini = IniFile::from_str(&format!("[General]\n{line}\n"));
            GeneralRules::from_ini(&ini).harvester_dump_frames
        };
        // Stock (key absent -> default 0.016): ceil(14.4) = 15, unchanged.
        assert_eq!(frames(""), 15, "stock 0.016 stays at 15 frames");
        // Modded 0.0156: ceil(14.04) = 15. Old tenths code crossed at 14 (bug).
        assert_eq!(
            frames("HarvesterDumpRate=0.0156"),
            15,
            "0.0156 must ceil to 15, not 14"
        );
        // Modded 0.02: ceil(18.0) = 18.
        assert_eq!(
            frames("HarvesterDumpRate=0.02"),
            18,
            "0.02 -> exactly 18 frames"
        );
        // Modded 0.0201: ceil(18.09) = 19 (round-to-nearest would give 18).
        assert_eq!(
            frames("HarvesterDumpRate=0.0201"),
            19,
            "0.0201 must ceil up to 19"
        );
    }

    #[test]
    fn test_gui_main_button_sound_parsed() {
        let ini_str = "\
[General]
FlightLevel=500
[AudioVisual]
GUIMainButtonSound=MenuClick
";
        let ini = IniFile::from_str(ini_str);
        let general = GeneralRules::from_ini(&ini);
        assert_eq!(general.gui_main_button_sound.as_deref(), Some("MenuClick"));
    }

    /// The two OreTwinkle keys are read by different native readers: the anim
    /// name from `[General]` (ReadGeneral) and the chance from `[AudioVisual]`
    /// (ReadAudioVisual). A key in the other section is invisible to gamemd.
    #[test]
    fn ore_twinkle_keys_follow_the_native_reader_sections() {
        let ini = IniFile::from_str(
            "[General]\nOreTwinkle=TWNK1\nOreTwinkleChance=7\n\
             [AudioVisual]\nOreTwinkleChance=30\nOreTwinkle=WRONG\n",
        );
        let general = RuleSet::from_ini(&ini).unwrap().general;
        assert_eq!(general.ore_twinkle.as_deref(), Some("TWNK1"));
        assert_eq!(general.ore_twinkle_chance, 30);

        let defaults = RuleSet::from_ini(&IniFile::from_str("[General]\nOreTwinkle=\n"))
            .unwrap()
            .general;
        assert_eq!(
            defaults.ore_twinkle, None,
            "an empty value keeps the null pointer"
        );
        assert_eq!(
            defaults.ore_twinkle_chance, 50,
            "RulesClass constructor default 0x32"
        );
    }

    #[test]
    fn shell_ui_sound_keys_parse_independently() {
        let ini_str = "\
[General]
FlightLevel=500
[AudioVisual]
GUIMainButtonSound=MainButtonClick
GenericClick=GenericPress
GenericBeep=GenericBeep
GUICheckboxSound=CheckboxTick
GUIComboOpenSound=ComboOpen
GUIComboCloseSound=ComboClose
ShellButtonSlideSound=SlideDone
";
        let ini = IniFile::from_str(ini_str);
        let general = GeneralRules::from_ini(&ini);
        assert_eq!(
            general.gui_main_button_sound.as_deref(),
            Some("MainButtonClick")
        );
        assert_eq!(
            general.shell_button_slide_sound.as_deref(),
            Some("SlideDone")
        );
        assert_eq!(general.generic_click_sound.as_deref(), Some("GenericPress"));
        assert_eq!(general.generic_beep_sound.as_deref(), Some("GenericBeep"));
        assert_eq!(general.gui_checkbox_sound.as_deref(), Some("CheckboxTick"));
        assert_eq!(general.gui_combo_open_sound.as_deref(), Some("ComboOpen"));
        assert_eq!(general.gui_combo_close_sound.as_deref(), Some("ComboClose"));
    }

    #[test]
    fn shell_ui_sound_keys_trim_and_ignore_empty_values() {
        let ini_str = concat!(
            "[General]\nFixtureOnly=1\n",
            "[AudioVisual]\n",
            "ChuteSound=  ParachuteDrop  \n",
            "GenericClick=  MenuClick  \n",
            "GenericBeep=   \n",
            "GUICheckboxSound=\n",
            "GUIComboOpenSound=   \n",
            "GUIComboCloseSound=MenuACBClose\n",
        );
        let ini = IniFile::from_str(ini_str);
        let general = GeneralRules::from_ini(&ini);
        assert_eq!(general.chute_sound.as_deref(), Some("ParachuteDrop"));
        assert_eq!(general.generic_click_sound.as_deref(), Some("MenuClick"));
        assert!(general.generic_beep_sound.is_none());
        assert!(general.gui_checkbox_sound.is_none());
        assert!(general.gui_combo_open_sound.is_none());
        assert_eq!(
            general.gui_combo_close_sound.as_deref(),
            Some("MenuACBClose")
        );
    }

    #[test]
    fn barrel_particle_parsed_from_general() {
        let ini_str = "\
[General]
BarrelParticle=SmallGreySSys
";
        let ini = IniFile::from_str(ini_str);
        let general = RuleSet::from_ini(&ini).unwrap().general;
        assert_eq!(general.barrel_particle.as_deref(), Some("SmallGreySSys"));
    }

    #[test]
    fn barrel_particle_default_none() {
        let ini_str = "[General]\nFixtureOnly=1\n";
        let ini = IniFile::from_str(ini_str);
        let general = RuleSet::from_ini(&ini).unwrap().general;
        assert!(general.barrel_particle.is_none());
    }

    #[test]
    fn gsi_05_10_prism_type_is_parsed_as_a_building_identity() {
        let ini = IniFile::from_str("[BuildingTypes]\n1=ATESLA\n[General]\nPrismType= atesla \n");
        let general = RuleSet::from_ini(&ini).unwrap().general;
        assert_eq!(general.prism_type.as_deref(), Some("ATESLA"));

        let absent = RuleSet::from_ini(&IniFile::from_str("[General]\nFixtureOnly=1\n"))
            .unwrap()
            .general;
        assert!(absent.prism_type.is_none());
    }

    #[test]
    fn barrel_particle_ignored_under_audiovisual() {
        // Per report sec 11.8.H the key lives in [General], not [AudioVisual].
        // Verify the parser doesn't accidentally accept it elsewhere.
        let ini_str = "\
[General]
FixtureOnly=1
[AudioVisual]
BarrelParticle=SmallGreySSys
";
        let ini = IniFile::from_str(ini_str);
        let general = RuleSet::from_ini(&ini).unwrap().general;
        assert!(general.barrel_particle.is_none());
    }

    #[test]
    fn test_building_garrisoned_sound_default_none() {
        let ini_str = "\
[General]
FixtureOnly=1
[AudioVisual]
";
        let ini = IniFile::from_str(ini_str);
        let general = GeneralRules::from_ini(&ini);
        assert!(general.building_garrisoned_sound.is_none());
    }

    #[test]
    fn test_chute_sound_empty_treated_as_none() {
        let ini_str = "\
[General]
FixtureOnly=1
[AudioVisual]
ChuteSound=
";
        let ini = IniFile::from_str(ini_str);
        let general = GeneralRules::from_ini(&ini);
        assert!(general.chute_sound.is_none());
    }

    #[test]
    fn cell_scatter_gate_keys_parse_from_their_own_sections() {
        // PlayerScatter is a [CombatDamage] key and Scatter an [IQ] key; neither
        // lives in [General], so a [General]-only file must fall back to the
        // RulesClass constructor values (no / 3).
        let ini = IniFile::from_str("[General]\nFixtureOnly=1\n");
        let general = GeneralRules::from_ini(&ini);
        assert!(!general.player_scatter);
        assert_eq!(general.iq_scatter, 3);

        // Stock values.
        let ini = IniFile::from_str(
            "[General]\nFlightLevel=500\n[CombatDamage]\nPlayerScatter=no\n[IQ]\nScatter=2\n",
        );
        let general = GeneralRules::from_ini(&ini);
        assert!(!general.player_scatter);
        assert_eq!(general.iq_scatter, 2);

        let ini = IniFile::from_str(
            "[General]\nFlightLevel=500\n[CombatDamage]\nPlayerScatter=yes\n[IQ]\nScatter=4\n",
        );
        let general = GeneralRules::from_ini(&ini);
        assert!(general.player_scatter);
        assert_eq!(general.iq_scatter, 4);
    }

    #[test]
    fn house_ai_activation_iq_production_preserves_native_signed_iq_binding() {
        assert_eq!(
            GeneralRules::from_ini(&IniFile::from_str("")).iq_production,
            5,
            "missing [IQ] retains the RulesClass constructor default"
        );
        assert_eq!(
            GeneralRules::from_ini(&IniFile::from_str(
                "[General]\nProduction=-12\n[IQ]\nFixtureOnly=1\n"
            ))
            .iq_production,
            5,
            "the same key under [General] is not an IQ input"
        );
        assert_eq!(
            GeneralRules::from_ini(&IniFile::from_str(
                "[General]\nFixtureOnly=1\n[IQ]\nProduction=5\n"
            ))
            .iq_production,
            5
        );
        assert_eq!(
            GeneralRules::from_ini(&IniFile::from_str(
                "[General]\nFixtureOnly=1\n[IQ]\nProduction=-7\n"
            ))
            .iq_production,
            -7,
            "negative custom thresholds remain signed and unclamped"
        );
        assert_eq!(
            GeneralRules::from_ini(&IniFile::from_str(
                "[General]\nFixtureOnly=1\n[IQ]\nMaxIQLevels=5\nProduction=9\n"
            ))
            .iq_production,
            9,
            "Production is not clamped to MaxIQLevels"
        );
        assert_eq!(
            GeneralRules::from_ini(&IniFile::from_str("[IQ]\nProduction=-3\n")).iq_production,
            -3,
            "ReadIQ remains authoritative when [General] is absent"
        );
    }

    #[test]
    fn base_unit_types_parse_from_general() {
        let ini = IniFile::from_str("[General]\nBaseUnit=AMCV,SMCV,PCV\n");
        let general = RuleSet::from_ini(&ini).unwrap().general;
        assert_eq!(general.base_unit_types, vec!["AMCV", "SMCV", "PCV"]);
    }

    #[test]
    fn test_parachute_max_fall_rate_parsed() {
        let ini_str = "\
[General]
ParachuteMaxFallRate=-3
";
        let ini = IniFile::from_str(ini_str);
        let general = RuleSet::from_ini(&ini).unwrap().general;
        assert_eq!(general.parachute_max_fall_rate, -3);
    }

    #[test]
    fn test_parachute_max_fall_rate_default_when_missing() {
        let ini_str = "[General]\nFixtureOnly=1\n";
        let ini = IniFile::from_str(ini_str);
        let general = RuleSet::from_ini(&ini).unwrap().general;
        assert_eq!(
            general.parachute_max_fall_rate, -3,
            "default must be -3 per gamemd Rules+0x7B8"
        );
    }

    #[test]
    fn test_parachute_max_fall_rate_custom() {
        // Mod-friendliness: a non-default value must be respected, not clamped.
        let ini_str = "\
[General]
ParachuteMaxFallRate=-1
";
        let ini = IniFile::from_str(ini_str);
        let general = RuleSet::from_ini(&ini).unwrap().general;
        assert_eq!(general.parachute_max_fall_rate, -1);
    }

    #[test]
    fn test_missile_rot_var_missing_key_keeps_native_constructor() {
        let ini = IniFile::from_str("[General]\nFixtureOnly=1\n");
        let general = RuleSet::from_ini(&ini).unwrap().general;
        assert_eq!(general.missile_rot_var, 0.25);
    }

    #[test]
    fn test_missile_rot_var_stock_rules_value_parsed() {
        let ini = IniFile::from_str("[General]\nMissileROTVar=.25\n");
        let general = RuleSet::from_ini(&ini).unwrap().general;
        let diff = (general.missile_rot_var - 0.25).abs();
        assert!(diff < 0.001, "got {:?}", general.missile_rot_var);
    }

    #[test]
    fn test_missile_rot_var_parsed() {
        let ini = IniFile::from_str("[General]\nMissileROTVar=2.5\n");
        let general = RuleSet::from_ini(&ini).unwrap().general;
        let diff = (general.missile_rot_var - 2.5).abs();
        assert!(diff < 0.001, "got {:?}", general.missile_rot_var);
    }

    #[test]
    fn test_building_garrisoned_sound_empty_treated_as_none() {
        let ini_str = "\
[General]
FixtureOnly=1
[AudioVisual]
BuildingGarrisonedSound=
";
        let ini = IniFile::from_str(ini_str);
        let general = GeneralRules::from_ini(&ini);
        assert!(general.building_garrisoned_sound.is_none());
    }

    fn make_particle_test_rules(extra: &str) -> String {
        // Minimal rules that load into a RuleSet — empty registries for the
        // unit categories so RuleSet::from_ini doesn't reject the input.
        format!(
            "\
[General]
BuildSpeed=0.75
MultipleFactory=0.7
LowPowerPenaltyModifier=1.25
MinLowPowerProductionSpeed=0.4
MaxLowPowerProductionSpeed=0.85

[InfantryTypes]
[VehicleTypes]
[AircraftTypes]
[BuildingTypes]

{extra}",
        )
    }

    #[test]
    fn two_pass_resolves_next_particle_regardless_of_order() {
        let extra = "\
[Particles]
1=ChainEnd
2=ChainStart

[ChainStart]
NextParticle=ChainEnd
BehavesLike=Gas

[ChainEnd]
BehavesLike=Gas
";
        let ini = IniFile::from_str(&make_particle_test_rules(extra));
        let rs = RuleSet::from_ini(&ini).unwrap();
        let start_id = rs.p_type_id_by_name("ChainStart").unwrap();
        let end_id = rs.p_type_id_by_name("ChainEnd").unwrap();
        assert_eq!(rs.particle_type(start_id).next_particle, Some(end_id));
        assert_eq!(rs.particle_type(end_id).next_particle, None);
    }

    #[test]
    fn two_pass_resolves_holds_what() {
        let extra = "\
[Particles]
1=Smoke1

[ParticleSystems]
1=BigSmoke

[BigSmoke]
HoldsWhat=Smoke1
BehavesLike=Smoke

[Smoke1]
BehavesLike=Smoke
";
        let ini = IniFile::from_str(&make_particle_test_rules(extra));
        let rs = RuleSet::from_ini(&ini).unwrap();
        let s = rs.ps_type_id_by_name("BigSmoke").unwrap();
        let p = rs.p_type_id_by_name("Smoke1").unwrap();
        assert_eq!(rs.particle_system_type(s).holds_what, Some(p));
    }

    #[test]
    fn missing_reference_logs_and_leaves_none() {
        let extra = "\
[Particles]
1=GhostRef

[GhostRef]
NextParticle=DoesNotExist
BehavesLike=Gas
";
        let ini = IniFile::from_str(&make_particle_test_rules(extra));
        let rs = RuleSet::from_ini(&ini).unwrap();
        let id = rs.p_type_id_by_name("GhostRef").unwrap();
        assert_eq!(rs.particle_type(id).next_particle, None);
    }

    #[test]
    fn p_type_id_by_name_is_case_insensitive() {
        let extra = "\
[Particles]
1=GasCloud1

[GasCloud1]
BehavesLike=Gas
";
        let ini = IniFile::from_str(&make_particle_test_rules(extra));
        let rs = RuleSet::from_ini(&ini).unwrap();
        let a = rs.p_type_id_by_name("GasCloud1").unwrap();
        let b = rs.p_type_id_by_name("GASCLOUD1").unwrap();
        let c = rs.p_type_id_by_name("gascloud1").unwrap();
        assert_eq!(a, b);
        assert_eq!(b, c);
    }

    #[test]
    fn combat_damage_defaults_load_from_ini() {
        let extra = "\
[Particles]
1=Fp

[ParticleSystems]
1=FireStreamSys

[FireStreamSys]
BehavesLike=Fire

[Fp]
BehavesLike=Fire

[CombatDamage]
DefaultFireStreamSystem=FireStreamSys
DefaultSparkSystem=SparkSys
";
        let ini = IniFile::from_str(&make_particle_test_rules(extra));
        let rs = RuleSet::from_ini(&ini).unwrap();
        assert_eq!(
            rs.combat_damage.default_fire_stream_system.as_deref(),
            Some("FireStreamSys")
        );
        assert_eq!(
            rs.combat_damage.default_spark_system.as_deref(),
            Some("SparkSys")
        );
        // Other slots stay None when the key isn't present.
        assert!(rs.combat_damage.default_repair_particle_system.is_none());
    }

    #[test]
    fn combat_damage_defaults_when_section_absent() {
        let ini = IniFile::from_str(&make_particle_test_rules(""));
        let rs = RuleSet::from_ini(&ini).unwrap();
        assert!(rs.combat_damage.default_fire_stream_system.is_none());
        assert!(rs.combat_damage.default_spark_system.is_none());
    }

    #[test]
    fn damage_fire_thresholds_use_the_same_authored_double_as_other_health_readers() {
        let ini = IniFile::from_str(
            "[General]\nDamageFireTypes=FIRE01\n[AudioVisual]\nConditionYellow=49%\nConditionRed=13%\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("native thresholds are not stock-only");
        let section = ini.section("AudioVisual").unwrap();
        assert_eq!(
            rules.general.condition_yellow,
            section.read_double("ConditionYellow", 0.5)
        );
        assert_eq!(
            rules.general.condition_red,
            section.read_double("ConditionRed", GeneralRules::default().condition_red)
        );
    }

    #[test]
    fn empty_particles_section_leaves_registries_empty() {
        // Pre-existing rules without [Particles]/[ParticleSystems] still parse.
        let ini = IniFile::from_str(&make_particle_test_rules(""));
        let rs = RuleSet::from_ini(&ini).unwrap();
        assert_eq!(rs.particle_type_count(), 0);
        assert_eq!(rs.particle_system_type_count(), 0);
        assert_eq!(rs.p_type_id_by_name("Anything"), None);
        assert_eq!(rs.ps_type_id_by_name("Anything"), None);
    }

    fn ini_with_general(body: &str) -> IniFile {
        let text = format!("[General]\n{}\n", body);
        IniFile::from_str(&text)
    }

    #[cfg(feature = "phobos")]
    #[test]
    fn ai_dehardcode_keys_default_and_read_general() {
        let defaults = GeneralRules::default();
        assert!(defaults.ai_fire_sale);
        assert_eq!(defaults.ai_fire_sale_delay, 0);
        assert!(defaults.ai_all_to_hunt);
        assert!(defaults.gather_when_mcv_deploy);

        let read = GeneralRules::from_ini(&ini_with_general(
            "AIFireSale=no\nAIFireSaleDelay=450\nAIAllToHunt=no\nGatherWhenMCVDeploy=no",
        ));
        assert!(!read.ai_fire_sale);
        assert_eq!(read.ai_fire_sale_delay, 450);
        assert!(!read.ai_all_to_hunt);
        assert!(!read.gather_when_mcv_deploy);

        let partial = GeneralRules::from_ini(&ini_with_general("AIFireSaleDelay=-3"));
        assert!(partial.ai_fire_sale && partial.ai_all_to_hunt && partial.gather_when_mcv_deploy);
        assert_eq!(partial.ai_fire_sale_delay, -3);
    }

    #[test]
    fn sparking_probability_defaults_and_override() {
        // Stock INI omits both keys -> the verified RulesClass ctor defaults
        // (0.02 red / 0.01 yellow), so the AI_Update Spark effect is on by default.
        let d = GeneralRules::default();
        assert_eq!(d.condition_red_sparking_probability, 0.02);
        assert_eq!(d.condition_yellow_sparking_probability, 0.01);
        let none = GeneralRules::from_ini(&IniFile::from_str("[Foo]\n"));
        assert_eq!(none.condition_red_sparking_probability, 0.02);
        assert_eq!(none.condition_yellow_sparking_probability, 0.01);
        // A mod overrides them from [General].
        let g = GeneralRules::from_ini(&ini_with_general(
            "ConditionRedSparkingProbability=0.05\n\
             ConditionYellowSparkingProbability=0.03",
        ));
        assert_eq!(g.condition_red_sparking_probability, f64::from(0.05_f32));
        assert_eq!(g.condition_yellow_sparking_probability, f64::from(0.03_f32));
    }

    #[test]
    fn health_conditions_retain_the_native_double_without_scaled_copies() {
        let ini = IniFile::from_str("[AudioVisual]\nConditionYellow=37%\nConditionRed=13%\n");
        let general = GeneralRules::from_ini(&ini);
        let section = ini.section("AudioVisual").unwrap();
        assert_eq!(
            general.condition_yellow.to_bits(),
            section.read_double("ConditionYellow", 0.5).to_bits()
        );
        assert_eq!(
            general.condition_red.to_bits(),
            section
                .read_double("ConditionRed", GeneralRules::default().condition_red)
                .to_bits()
        );
        assert_ne!(
            general.condition_yellow.to_bits(),
            f64::from(general.condition_yellow as f32).to_bits()
        );
    }

    #[test]
    fn targeting_delay_defaults_and_override() {
        // Stock rulesmd.ini carries both keys with exactly the constructor
        // defaults; a missing section must still yield them.
        let d = GeneralRules::default();
        assert_eq!(d.normal_targeting_delay, 27);
        assert_eq!(d.guard_area_targeting_delay, 36);
        let none = GeneralRules::from_ini(&IniFile::from_str("[Foo]\n"));
        assert_eq!(none.normal_targeting_delay, 27);
        assert_eq!(none.guard_area_targeting_delay, 36);
        // Values come FROM the INI, not from a hardcoded constant.
        let g = GeneralRules::from_ini(&ini_with_general(
            "NormalTargetingDelay=9\nGuardAreaTargetingDelay=13",
        ));
        assert_eq!(g.normal_targeting_delay, 9);
        assert_eq!(g.guard_area_targeting_delay, 13);
        // ReadInt keeps a negative delay (no clamp at `0x006701B4` or
        // `0x006701D3`).
        let g = GeneralRules::from_ini(&ini_with_general(
            "NormalTargetingDelay=-3\nGuardAreaTargetingDelay=-10",
        ));
        assert_eq!(g.normal_targeting_delay, -3);
        assert_eq!(g.guard_area_targeting_delay, -10);
    }

    /// The constructor bands take the executed Spark gate's boundary draws
    /// (`0x006FAE2D..0x006FAE40`); a 1-off flips the draw count on a boundary
    /// tick. A subnormal and a tiny reader value admit roll 0 alone.
    #[test]
    fn damage_spark_thresholds_from_the_general_reader() {
        let d = GeneralRules::default();
        let mut checked = 0;
        for (band, draw, passed) in crate::util::native_random::native_gate_rows("spark") {
            for (probability, threshold) in [
                (
                    d.condition_red_sparking_probability,
                    d.condition_red_spark_threshold,
                ),
                (
                    d.condition_yellow_sparking_probability,
                    d.condition_yellow_spark_threshold,
                ),
            ] {
                if band.to_bits() == probability.to_bits() {
                    assert_eq!(draw < threshold, passed, "band {band}, draw {draw:#x}");
                    checked += 1;
                }
            }
        }
        assert_eq!(checked, 12);
        assert_eq!(d.condition_red_spark_threshold, 42_949_673);
        assert_eq!(d.condition_yellow_spark_threshold, 21_474_837);

        let g = GeneralRules::from_ini(&ini_with_general(
            "ConditionRedSparkingProbability=1e-45\nConditionYellowSparkingProbability=1e-30",
        ));
        let red = g.condition_red_sparking_probability;
        assert!(red > 0.0 && red < f64::from(f32::MIN_POSITIVE), "{red:e}");
        assert_eq!(g.condition_red_spark_threshold, 1);
        assert_eq!(g.condition_yellow_spark_threshold, 1);
    }

    /// The constructor empties every paradrop list (`0x00666649..0x006666D8`)
    /// and a pass without the keys keeps them.
    #[test]
    fn paradrop_lists_start_empty() {
        let rules = RuleSet::from_ini(&IniFile::from_str("[Foo]\nBar=1\n")).unwrap();
        let g = &rules.general;
        assert_eq!(g.paradrop_radius, 1024);
        for list in [
            &g.amer_paradrop,
            &g.ally_paradrop,
            &g.sov_paradrop,
            &g.yuri_paradrop,
        ] {
            assert_eq!(list, &ParadropList::default());
        }
    }

    /// Each list is read on its own: the infantry through the InfantryType
    /// list reader, which keeps a name it does not know and drops `none`, the
    /// counts as ints. Nothing pairs them or compares their lengths.
    #[test]
    fn paradrop_lists_read_separately() {
        let rules = RuleSet::from_ini(&ini_with_general(
            "ParadropRadius=2048\n\
             AmerParaDropInf=E1,GHOST,none,ENGINEER\n\
             AmerParaDropNum=6,-2\n\
             SovParaDropInf=E2\n\
             SovParaDropNum=9,4",
        ))
        .unwrap();
        let g = &rules.general;
        assert_eq!(g.paradrop_radius, 2048);
        assert_eq!(g.amer_paradrop.infantry, ["E1", "GHOST", "ENGINEER"]);
        assert_eq!(g.amer_paradrop.counts, [6, -2]);
        assert_eq!(g.sov_paradrop.infantry, ["E2"]);
        assert_eq!(g.sov_paradrop.counts, [9, 4]);
        assert_eq!(g.ally_paradrop, ParadropList::default());
    }

    #[test]
    fn paradrop_weapon_rof_reaches_resolved_weapon() {
        // Verifies the Task 5 grounding question: does [ParaDropWeapon] ROF=130
        // flow through the weapon parser into rules.weapon("ParaDropWeapon").rof?
        // The parser only reads weapon sections referenced from an ObjectType's
        // Primary= / Secondary=, so we need a minimal aircraft entry that points
        // to ParaDropWeapon.
        let text = "\
[AircraftTypes]
1=PDPLANE

[PDPLANE]
Primary=ParaDropWeapon
Strength=400
Speed=15
Image=PDPLANE

[ParaDropWeapon]
Damage=60
ROF=130
Range=1
Projectile=Invisible
";
        let ini = IniFile::from_str(text);
        let rs = RuleSet::from_ini(&ini).expect("rules parse");
        let weapon = rs
            .weapon("ParaDropWeapon")
            .expect("ParaDropWeapon must reach the weapon registry");
        assert_eq!(weapon.rof, 130);
    }

    #[test]
    fn merge_art_propagates_add_remove_occupy() {
        let mut layers = RulesLayerStack::new(IniFile::from_str(&make_test_rules()));
        layers.push(
            crate::rules::native_processing::RulesLayerKind::Scenario,
            IniFile::from_str(
                "[BuildingTypes]\n0=GAREFN\n\
             [GAREFN]\nName=Refinery\nCost=2000\nFoundation=4x3\n",
            ),
        );
        let art_text = "[GAREFN]\nFoundation=4x3\nCanHideThings=no\nOccupyHeight=4\nAddOccupy1=-1,0\nAddOccupy2=-1,-1\nRemoveOccupy1=3,1\n";
        let art_ini: IniFile = IniFile::from_str(art_text);
        let mut rules =
            RuleSet::from_processed_rules(&layers.process_with_fixed_art(&art_ini).unwrap())
                .expect("rules parse");
        let art = crate::rules::art_data::ArtRegistry::from_ini(&art_ini);
        rules.install_art_data(art);
        let obj = rules.object("GAREFN").expect("GAREFN");
        assert_eq!(obj.hidden_occupancy.add_occupy[0], Some((-1, 0)));
        assert_eq!(obj.hidden_occupancy.add_occupy[1], Some((-1, -1)));
        assert_eq!(obj.hidden_occupancy.remove_occupy[0], Some((3, 1)));
        assert!(!obj.hidden_occupancy.can_hide_things);
        assert_eq!(obj.hidden_occupancy.occupy_height, 4);
        assert!(!rules.art_registry.can_hide_things("GAREFN"));
        assert_eq!(rules.art_registry.occupy_height("GAREFN"), 4);
    }

    #[test]
    fn native_discharge_retail_gi_binding_retains_independent_zero_defaults() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/infantry_discharge_rules.json",
        ))
        .unwrap();
        let physical = corpus["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["case"] == "physical_GI")
            .unwrap();
        let expected: [i32; 4] = serde_json::from_value(physical["after"].clone()).unwrap();
        let Some((retail_rules, retail_art)) =
            crate::rules::retail_ini_fixture::retail_rules_and_art()
        else {
            return;
        };
        let mut rules = RuleSet::from_ini(&retail_rules).expect("retail rules parse");
        rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&retail_art));
        let infantry = rules.object("E1").expect("retail E1");
        assert_eq!(infantry.image, "GI");
        assert_eq!(
            [
                infantry.fire_up_frame,
                infantry.fire_prone_frame,
                infantry.secondary_fire_frame,
                infantry.secondary_prone_frame,
            ],
            expected,
            "original5246BE..52473A on physical ARTMD.INI[GI]"
        );
    }

    #[test]
    fn native_discharge_signed_art_fields_reach_infantry_type() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/infantry_discharge_rules.json",
        ))
        .unwrap();
        let row = corpus["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["case"] == "all_independent")
            .unwrap();
        let expected: [i32; 4] = serde_json::from_value(row["after"].clone()).unwrap();
        let mut layers = RulesLayerStack::new(IniFile::from_str(&make_test_rules()));
        layers.push(
            crate::rules::native_processing::RulesLayerKind::Scenario,
            IniFile::from_str("[E1]\nImage=GI\n"),
        );
        let mut rules = RuleSet::from_rules_layers(&layers).expect("rules parse");
        let mut art_text = "[GI]\nFixtureOnly=1\n".to_owned();
        for (key, raw) in row["layers"][0].as_object().unwrap() {
            art_text.push_str(&format!("{key}={}\n", raw.as_str().unwrap()));
        }
        rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(
            &IniFile::from_str(&art_text),
        ));
        let infantry = rules.object("E1").expect("E1");
        assert_eq!(
            [
                infantry.fire_up_frame,
                infantry.fire_prone_frame,
                infantry.secondary_fire_frame,
                infantry.secondary_prone_frame,
            ],
            expected,
            "original independent signed DWORDs survive ART projection"
        );
    }

    #[test]
    fn merge_art_propagates_infantry_crawls_without_building_side_effects() {
        let mut layers = RulesLayerStack::new(IniFile::from_str(&make_test_rules()));
        layers.push(
            crate::rules::native_processing::RulesLayerKind::Scenario,
            IniFile::from_str(
                "[E1]\nName=GI\nImage=GI\nStrength=125\nArmor=flak\nSpeed=4\n\
             [GAPOWR]\nName=Power\nStrength=750\nArmor=wood\nFoundation=2x2\n",
            ),
        );
        let art_ini = IniFile::from_str(
            "[GI]\nCrawls=yes\nFireUp=2\nFireProne=3\nSecondaryFire=4\nSecondaryProne=5\n\n[GAPOWR]\nFoundation=2x2\nCrawls=yes\nFireUp=9\n",
        );
        let mut rules =
            RuleSet::from_processed_rules(&layers.process_with_fixed_art(&art_ini).unwrap())
                .expect("rules parse");
        let art = crate::rules::art_data::ArtRegistry::from_ini(&art_ini);
        rules.install_art_data(art);

        let infantry = rules.object("E1").expect("E1");
        assert!(infantry.crawls);
        assert_eq!(infantry.fire_up_frame, 2);
        assert_eq!(infantry.fire_prone_frame, 3);
        assert_eq!(infantry.secondary_fire_frame, 4);
        assert_eq!(infantry.secondary_prone_frame, 5);
        let building = rules.object("GAPOWR").expect("GAPOWR");
        assert!(!building.crawls);
        assert_eq!(building.fire_up_frame, 0);
        assert_eq!(building.foundation, "2x2");
    }

    #[test]
    fn wall_overlay_first_match_order_survives_registry_move() {
        // F04: the overlay registry moved from map to rules. Pin the two
        // orders wall selling depends on: `[OverlayTypes]` declaration order
        // IS the overlay ID, and `first_building_type_for_overlay` returns
        // the FIRST registered building even when a later one also matches.
        let overlay_ini = IniFile::from_str("[OverlayTypes]\n0=GASAND\n1=GAWALL\n2=CAWALL\n");
        let overlays =
            crate::rules::overlay_types::OverlayTypeRegistry::from_ini(&overlay_ini, None);
        assert_eq!(overlays.id_for_name("GAWALL"), Some(1));
        assert_eq!(overlays.id_for_name("CAWALL"), Some(2));

        let rules_ini = IniFile::from_str(
            "[BuildingTypes]\n0=GAWALL2\n1=GAWALL3\n[GAWALL2]\nCost=1\n[GAWALL3]\nCost=1\n",
        );
        let mut rules: RuleSet = RuleSet::from_ini(&rules_ini).expect("rules parse");
        for object in rules.object_list.iter_mut() {
            object.to_overlay = Some("GAWALL".to_string());
        }
        let first = rules
            .first_building_type_for_overlay(1, &overlays)
            .expect("first registered building wins");
        assert_eq!(first.id, "GAWALL2");
        assert!(
            rules
                .first_building_type_for_overlay(2, &overlays)
                .is_none()
        );
    }

    #[test]
    fn resolved_rule_handles_stay_null_without_combat_damage_keys() {
        use crate::sim::intern::StringInterner;
        use crate::sim::type_handle_table::ResolvedRuleHandles;
        let ini: IniFile = IniFile::from_str(&make_test_rules());
        let rules: RuleSet = RuleSet::from_ini(&ini).expect("rules parse");
        let mut interner = StringInterner::default();
        let handles = ResolvedRuleHandles::resolve(&rules, &mut interner);
        // The RulesClass constructor leaves the three warheads null
        // (`0x00666B52`, `0x00666B58`, `0x00666BD5`) and the test rules.ini
        // has no `[CombatDamage]` keys, so no named warhead matches them.
        assert_eq!(interner.resolve(handles.ion_cannon), "");
        assert_eq!(interner.resolve(handles.c4), "");
        assert_eq!(interner.resolve(handles.crush), "");
        assert!(!handles.is_crush(interner.intern("Crush")));
    }

    #[test]
    fn resolved_rule_handles_honor_combat_damage_overrides() {
        use crate::sim::intern::StringInterner;
        use crate::sim::type_handle_table::ResolvedRuleHandles;
        let rules_text = format!(
            "{}\n[CombatDamage]\nIonCannonWarhead=CustomIon\nC4Warhead=CustomC4\nCrushWarhead=CustomCrush\n",
            make_test_rules()
        );
        let ini: IniFile = IniFile::from_str(&rules_text);
        let rules: RuleSet = RuleSet::from_ini(&ini).expect("rules parse");
        let mut interner = StringInterner::default();
        let handles = ResolvedRuleHandles::resolve(&rules, &mut interner);
        assert_eq!(interner.resolve(handles.ion_cannon), "CustomIon");
        assert_eq!(interner.resolve(handles.c4), "CustomC4");
        assert_eq!(interner.resolve(handles.crush), "CustomCrush");
    }

    #[test]
    fn retail_rulesmd_c4_flags_parse_correctly() {
        let ini = IniFile::from_str(
            "[CombatDamage]\nC4Delay=0.03\n\
             [InfantryTypes]\n\
             0=GHOST\n1=TANY\n2=PTROOP\n3=E1\n4=ENGINEER\n5=CCOMAND\n\
             [VehicleTypes]\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             0=CAMISC01\n1=CAMISC02\n2=CAMISC06\n3=AMMOCRAT\n\
             4=GAPILE\n5=NAHAND\n6=GAREFN\n\
             [GHOST]\nC4=yes\n\
             [TANY]\nC4=yes\n\
             [PTROOP]\nC4=yes\n\
             [E1]\nFixtureOnly=1\n\
             [ENGINEER]\nFixtureOnly=1\n\
             [CCOMAND]\nFixtureOnly=1\n\
             [CAMISC01]\nCanC4=no\n\
             [CAMISC02]\nCanC4=no\n\
             [CAMISC06]\nCanC4=no\n\
             [AMMOCRAT]\nCanC4=no\n\
             [GAPILE]\nFixtureOnly=1\n\
             [NAHAND]\nFixtureOnly=1\n\
             [GAREFN]\nFixtureOnly=1\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("parse C4 stock-contract fixture");

        // C4-capable units must have c4=true.
        for unit in &["GHOST", "TANY", "PTROOP"] {
            let obj = rules
                .object(unit)
                .unwrap_or_else(|| panic!("no [{}]", unit));
            assert!(obj.c4, "[{}] must have c4=true (C4=yes in INI)", unit);
        }
        // Non-C4 infantry must have c4=false.
        for unit in &["E1", "ENGINEER", "CCOMAND"] {
            if let Some(obj) = rules.object(unit) {
                assert!(!obj.c4, "[{}] must have c4=false", unit);
            }
        }

        // CanC4-opt-out buildings — verified by direct grep of ini/rulesmd.ini
        // for `^CanC4=no`. Four sections match: CAMISC01, CAMISC02, CAMISC06,
        // AMMOCRAT. (The plan originally listed CAMSC09/CAMSC10 in error;
        // the retail INI does not set the flag on either.)
        for bld in &["CAMISC01", "CAMISC02", "CAMISC06", "AMMOCRAT"] {
            let obj = rules.object(bld).unwrap_or_else(|| panic!("no [{}]", bld));
            assert!(
                !obj.can_c4,
                "[{}] must have can_c4=false (CanC4=no in INI)",
                bld
            );
        }
        // Normal buildings inherit can_c4=true.
        for bld in &["GAPILE", "NAHAND", "GAREFN"] {
            if let Some(obj) = rules.object(bld) {
                assert!(obj.can_c4, "[{}] must have can_c4=true (default)", bld);
            }
        }

        // Retail C4Delay=.03 is a 26-frame fuse.
        assert_eq!(rules.c4_delay_frames(), 26);
    }

    #[test]
    fn type_lookups_are_case_insensitive() {
        // Parity: the original engine resolves type names case-insensitively
        // (stricmp-style find-or-allocate). Use a hermetic rules graph and prove every
        // type accessor resolves a stored name regardless of case, to the same entry.
        let ini_text = format!(
            "{}\n[SuperWeaponTypes]\n0=FixtureSW\n[FixtureSW]\nType=MultiMissile\n",
            make_test_rules()
        );
        let ini = IniFile::from_str(&ini_text);
        let rules = RuleSet::from_ini(&ini).expect("parse case-lookup fixture");

        // Concrete, readable anchor from the fixture.
        assert!(rules.object("MTNK").is_some());
        assert!(
            rules.object("mtnk").is_some(),
            "lowercase must resolve (gamemd parity)"
        );
        assert!(rules.object("Mtnk").is_some(), "mixed case must resolve");
        assert_eq!(
            rules.object("mtnk").map(|o| o as *const _),
            rules.object("MTNK").map(|o| o as *const _),
            "all casings resolve to the same object",
        );

        // Property over each of the five type maps: take a real stored key and
        // assert both the upper- and lower-cased forms resolve to the same entry.
        // Toggling case guarantees at least one form differs from the stored key,
        // so the case-fold scan path (not just exact match) is exercised.
        fn check_ci<'a, M, V, S>(
            map: &'a HashMap<String, M, S>,
            lookup: impl Fn(&str) -> Option<&'a V>,
            label: &str,
        ) where
            V: 'a,
        {
            let key = map
                .keys()
                .next()
                .cloned()
                .unwrap_or_else(|| panic!("{label} map empty"));
            let canon = lookup(&key).map(|v| v as *const V);
            assert!(canon.is_some(), "{label} '{key}' must resolve as stored");
            assert_eq!(
                lookup(&key.to_ascii_lowercase()).map(|v| v as *const V),
                canon,
                "{label} '{key}' lowercase must resolve to same entry"
            );
            assert_eq!(
                lookup(&key.to_ascii_uppercase()).map(|v| v as *const V),
                canon,
                "{label} '{key}' uppercase must resolve to same entry"
            );
        }

        check_ci(&rules.object_index, |k| rules.object(k), "object");
        check_ci(&rules.weapons, |k| rules.weapon(k), "weapon");
        check_ci(&rules.warheads, |k| rules.warhead(k), "warhead");
        check_ci(&rules.projectiles, |k| rules.projectile(k), "projectile");
        check_ci(
            &rules.super_weapons,
            |k| rules.super_weapon(k),
            "super_weapon",
        );
    }

    #[test]
    fn naval_base_rules_preserve_source_order_defaults_and_buildconst_identity() {
        let ini = IniFile::from_str(
            "[General]\n\
             Shipyard=GAYARD,nayard,YAYARD\n\
             BuildConst=WRONG\n\
             AINavalYardAdjacency=-7\n\
             [AI]\nBuildConst=,gacnst,,NACNST,gacnst,none,<NoNe>,YACNST,,\n\
             BuildTech=GAYARD,YAYARD\n\
             [InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
             [BuildingTypes]\n\
             0=GAYARD\n1=NAYARD\n2=YAYARD\n3=GACNST\n4=NACNST\n5=YACNST\n6=WRONG\n\
             [GAYARD]\nFoundation=4x4\n\
             [NAYARD]\nFoundation=4x4\nAIBasePlanningSide=1\n\
             [YAYARD]\nFoundation=4x4\nAIBasePlanningSide=-3\n\
             [GACNST]\nFoundation=4x4\n\
             [NACNST]\nFoundation=4x4\n\
             [YACNST]\nFoundation=4x4\n\
             [WRONG]\nFoundation=4x4\n",
        );
        let art = IniFile::from_str(
            "[GAYARD]\nFoundation=4x4\n[NAYARD]\nFoundation=4x4\n\
             [YAYARD]\nFoundation=4x4\n[GACNST]\nFoundation=4x4\n\
             [NACNST]\nFoundation=4x4\n[YACNST]\nFoundation=4x4\n\
             [WRONG]\nFoundation=4x4\n",
        );
        let rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art)
            .expect("naval base rules and ART");

        assert_eq!(rules.shipyard_types, ["GAYARD", "NAYARD", "YAYARD"]);
        assert_eq!(
            rules.build_const_types,
            ["GACNST", "NACNST", "GACNST", "YACNST"],
            "ReadAI keeps the order and duplicates of the Types it found, without null sentinels"
        );
        assert_eq!(rules.build_tech_types, ["GAYARD", "YAYARD"]);
        assert_eq!(rules.ai_naval_yard_adjacency, -7);
        assert_eq!(rules.object("GAYARD").unwrap().ai_base_planning_side, -1);
        assert_eq!(rules.object("NAYARD").unwrap().ai_base_planning_side, 1);
        assert_eq!(rules.object("YAYARD").unwrap().ai_base_planning_side, -3);
        for id in ["GACNST", "NACNST", "YACNST"] {
            assert!(
                rules
                    .object_in_category(ObjectCategory::Building, &id.to_ascii_lowercase())
                    .unwrap()
                    .build_const_eligible,
                "{id}"
            );
        }
        for id in ["GAYARD", "NAYARD", "YAYARD"] {
            let object = rules.object(id).unwrap();
            assert!(!object.build_const_eligible, "{id}");
            assert_eq!(object.foundation.dimensions(), (4, 4));
        }
        assert!(
            !rules.object("WRONG").unwrap().build_const_eligible,
            "the poisoned General key must not contribute membership"
        );

        let defaults = RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n0=YARD\n[YARD]\nFoundation=1x1\n",
        ))
        .expect("constructor defaults");
        assert_eq!(defaults.ai_naval_yard_adjacency, 20);
        assert_eq!(defaults.object("YARD").unwrap().ai_base_planning_side, -1);
        assert!(defaults.build_const_types.is_empty());
        assert!(!defaults.object("YARD").unwrap().build_const_eligible);
    }

    #[test]
    fn build_const_readai_does_not_trim_individual_tokens() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[AI]\nBuildConst=GACNST, NACNST\n\
             [InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
             [BuildingTypes]\n0=GACNST\n1=NACNST\n\
             [GACNST]\nFoundation=1x1\n\
             [NACNST]\nFoundation=1x1\n",
        ))
        .expect("BuildConst whitespace rules");

        // BuildingType FindOrAllocate (`0x004653C0`) compares whole IDs, so
        // " NACNST" allocates its own type rather than aliasing NACNST.
        assert_eq!(rules.build_const_types, ["GACNST", " NACNST"]);
        assert!(rules.object("GACNST").unwrap().build_const_eligible);
        assert!(rules.object(" NACNST").unwrap().build_const_eligible);
        assert!(
            !rules.object("NACNST").unwrap().build_const_eligible,
            "the internally space-prefixed token must not alias registered NACNST"
        );
    }

    #[test]
    fn build_const_readai_uses_native_127_source_byte_prefix() {
        let oversized = format!("GACNST,{},LATE", "X".repeat(120));
        assert_eq!(oversized.as_bytes()[127], b',');
        let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[AI]\nBuildConst={oversized}\n\
             [InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
             [BuildingTypes]\n0=GACNST\n1=LATE\n\
             [GACNST]\nFoundation=1x1\n\
             [LATE]\nFoundation=1x1\n"
        )))
        .expect("oversized BuildConst rules");
        // The oversized token names the BuildingType its factory allocated,
        // stored as its first 24 bytes (`AbstractType` `0x00410800`).
        let allocated = "X".repeat(24);
        assert_eq!(rules.build_const_types, ["GACNST", allocated.as_str()]);
        assert!(!rules.object("LATE").unwrap().build_const_eligible);

        // `IniFile::from_bytes` widens 0xE9 to U+00E9, whose Rust UTF-8
        // encoding is two bytes. It is nevertheless one native source byte at
        // payload index 126; the comma at source byte 127 and LATE stay outside
        // the copied buffer without a UTF-8 boundary slice or panic.
        let mut bytes = b"[AI]\nBuildConst=GACNST,".to_vec();
        bytes.extend(std::iter::repeat_n(b'X', 119));
        bytes.push(0xE9);
        bytes.extend_from_slice(
            b",LATE\n[InfantryTypes]\n0=DUMMY\n[DUMMY]\nStrength=1\n\
              [BuildingTypes]\n0=GACNST\n1=LATE\n\
              [GACNST]\nFoundation=1x1\n[LATE]\nFoundation=1x1\n",
        );
        let ini = IniFile::from_bytes(&bytes).expect("byte-domain BuildConst rules");
        let stored = ini
            .section("AI")
            .unwrap()
            .get_for_test("BuildConst")
            .unwrap();
        assert_eq!(stored.chars().nth(126), Some(char::from(0xE9)));
        assert_eq!(stored.chars().nth(127), Some(','));
        let rules = RuleSet::from_ini(&ini).expect("byte-domain BuildConst RuleSet");
        assert_eq!(rules.build_const_types, ["GACNST", allocated.as_str()]);
        assert!(!rules.object("LATE").unwrap().build_const_eligible);
    }

    #[test]
    fn recalc_type_lists_share_native_parser_and_registered_family_resolution() {
        let ini = IniFile::from_str(
            "[General]\n\
             HarvesterUnit=harv,,none,HARV,<none>, AUNIT\n\
             BuildPower=POISON\n\
             [AI]\n\
             BuildConst=con,CON\n\
             BuildPower=pow,none,POW\n\
             BuildRefinery=ref\n\
             BuildBarracks=bar\n\
             BuildTech=tech, TECH\n\
             BuildWeapons=weap\n\
             BuildRadar=rad,<none>,RAD\n\
             NSGates=con,,none,CON\nEWGates=pow,<none>,POW\n\
             [VehicleTypes]\n0=HARV\n1=AUNIT\n\
             [BuildingTypes]\n0=CON\n1=POW\n2=REF\n3=BAR\n4=TECH\n5=WEAP\n6=RAD\n7=POISON\n\
             [HARV]\nStrength=1\n[AUNIT]\nStrength=1\n\
             [CON]\nFoundation=1x1\n[POW]\nFoundation=1x1\n\
             [REF]\nFoundation=1x1\n[BAR]\nFoundation=1x1\n\
             [TECH]\nFoundation=1x1\n[WEAP]\nFoundation=1x1\n\
             [RAD]\nFoundation=1x1\n[POISON]\nFoundation=1x1\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("all Recalc type lists");

        assert_eq!(rules.build_const_types, ["CON", "CON"]);
        assert_eq!(rules.build_power_types, ["POW", "POW"]);
        assert_eq!(rules.build_refinery_types, ["REF"]);
        assert_eq!(rules.build_barracks_types, ["BAR"]);
        assert_eq!(
            rules.build_tech_types,
            ["TECH", " TECH"],
            "the internally space-prefixed token is not trimmed; it allocates its own type"
        );
        assert_eq!(rules.build_weapons_types, ["WEAP"]);
        assert_eq!(rules.build_radar_types, ["RAD", "RAD"]);
        assert_eq!(rules.ns_gate_types, ["CON", "CON"]);
        assert_eq!(rules.ew_gate_types, ["POW", "POW"]);
        assert_eq!(
            rules.harvester_unit_types,
            ["HARV", "HARV", " AUNIT"],
            "sentinels are omitted, duplicates retained, and a space-prefixed Unit token \
             allocates its own UnitType (`0x007480D0`)"
        );
        assert!(rules.object("CON").unwrap().build_const_eligible);
        assert!(!rules.object("POISON").unwrap().build_const_eligible);
    }

    #[test]
    fn recalc_type_lists_preserve_prior_layer_when_later_key_is_missing_or_empty() {
        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[General]\nHarvesterUnit=HARV\n\
             [AI]\nBuildConst=CON\nBuildPower=POW\nBuildTech=TECH\n\
             NSGates=CON\nEWGates=POW\n\
             [VehicleTypes]\n0=HARV\n[HARV]\nStrength=1\n\
             [BuildingTypes]\n0=CON\n1=POW\n2=TECH\n3=NEWPOW\n\
             [CON]\nFoundation=1x1\n[POW]\nFoundation=1x1\n\
             [TECH]\nFoundation=1x1\n[NEWPOW]\nFoundation=1x1\n",
        ));
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str(
                "[General]\nHarvesterUnit=\nUnrelated=1\n\
                 [AI]\nBuildConst=\nBuildPower=NEWPOW\nNSGates=\n",
            ),
        );
        let rules = RuleSet::from_rules_layers(&layers).expect("layered Recalc lists");
        assert_eq!(rules.harvester_unit_types, ["HARV"]);
        assert_eq!(rules.build_const_types, ["CON"]);
        assert_eq!(rules.build_power_types, ["NEWPOW"]);
        assert_eq!(rules.build_tech_types, ["TECH"]);
        assert_eq!(rules.ns_gate_types, ["CON"]);
        assert_eq!(rules.ew_gate_types, ["POW"]);
    }

    #[test]
    fn recalc_difficulty_vectors_parse_exact_lengths_and_layer_defaults() {
        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[General]\n\
             AISlaveMinerNumber=4,3\n\
             AIExtraRefineries=2,1,0, -1\n\
             AlliedBaseDefenseCounts=25\n\
             SovietBaseDefenseCounts=25,22,6\n\
             ThirdBaseDefenseCounts=25,22,6\n\
             [BuildingTypes]\n0=DUMMY\n[DUMMY]\nFoundation=1x1\n",
        ));
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str(
                "[General]\nAISlaveMinerNumber=\nAIExtraRefineries=9,,7\nUnrelated=1\n",
            ),
        );
        let rules = RuleSet::from_rules_layers(&layers).expect("difficulty vectors");
        assert_eq!(rules.ai_slave_miner_number, [4, 3]);
        assert_eq!(rules.ai_extra_refineries, [9, 7]);
        assert_eq!(rules.allied_base_defense_counts, [25]);
        assert_eq!(rules.soviet_base_defense_counts, [25, 22, 6]);
        assert_eq!(rules.third_base_defense_counts, [25, 22, 6]);

        let missing = RuleSet::from_ini(&IniFile::from_str(
            "[BuildingTypes]\n0=DUMMY\n[DUMMY]\nFoundation=1x1\n",
        ))
        .expect("missing vectors");
        assert!(missing.ai_slave_miner_number.is_empty());
        assert!(missing.ai_extra_refineries.is_empty());
        assert!(missing.allied_base_defense_counts.is_empty());
    }

    /// Slice 8 acceptance: the sim TypeHandleTable resolves every interned type id
    /// (no orphans) and does so case-insensitively (htnk vs [HTNK]).
    #[test]
    fn type_handle_table_completeness_and_casing() {
        let ini = IniFile::from_str(&make_test_rules());
        let rules = RuleSet::from_ini(&ini).expect("fixture parses");
        let mut interner = crate::sim::intern::StringInterner::new();
        // Mirror Simulation::intern_rule_type_ids (the production interning
        // pass) so the table sees every registry id.
        for id in rules
            .infantry_ids
            .iter()
            .chain(&rules.vehicle_ids)
            .chain(&rules.aircraft_ids)
            .chain(&rules.building_ids)
        {
            interner.intern(id);
        }
        let table = crate::sim::type_handle_table::TypeHandleTable::build(&rules, &interner);

        // Completeness: every registry id in the fixture (E1, E2, MTNK, GAPOWR)
        // has a [section], so none is an orphan type_ref.
        assert_eq!(
            table.orphan_count(),
            0,
            "no interned type id should fail to resolve to an object"
        );

        // Casing: a lowercased reference resolves to the same handle as the stored
        // uppercase id. The interner is case-insensitive, so the interning pass ("MTNK")
        // and a later get("mtnk") share one id.
        let mtnk_lower = interner
            .get("mtnk")
            .expect("MTNK interned case-insensitively");
        assert_eq!(table.handle_for(mtnk_lower), rules.type_handle("MTNK"));
        assert!(
            rules.object("mtnk").is_some(),
            "lowercased object() resolves"
        );
    }

    /// Helper: parse a (rules.ini, art.ini) pair into a merged RuleSet for
    /// pad-merge tests. Keeps a minimal scaffolding (one BuildingType) so
    /// `RuleSet::from_ini` does not reject the input.
    fn parse_rules_with_art(building_section: &str, art_ini: &str) -> RuleSet {
        let rules_str = format!(
            "[General]\n\
             BuildSpeed=1\n\
             MultipleFactory=1\n\
             LowPowerPenaltyModifier=1\n\
             MinLowPowerProductionSpeed=1\n\
             MaxLowPowerProductionSpeed=1\n\
             [InfantryTypes]\n\
             [VehicleTypes]\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             0=GAAIRC\n\
             {}",
            building_section,
        );
        let rules_ini = IniFile::from_str(&rules_str);
        let art_ini_parsed = IniFile::from_str(art_ini);
        let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&rules_ini, &art_ini_parsed)
            .expect("rules parse");
        let art = crate::rules::art_data::ArtRegistry::from_ini(&art_ini_parsed);
        rules.install_art_data(art);
        rules
    }

    #[test]
    fn merge_pads_zero_pads_missing_indices() {
        // NumberOfDocks=4 but art only has DockingOffset0,1.
        // Merge must produce pads.len() == 4 with indices 2,3 zero-init.
        let rules = parse_rules_with_art(
            "[GAAIRC]\nName=Airforce\nCost=1000\nStrength=1000\nNumberOfDocks=4\n",
            "[GAAIRC]\n\
             DockingOffset0=0,-128,0\n\
             DockingOffset1=0,128,0\n",
        );
        let obj = rules.object("GAAIRC").expect("obj");
        assert_eq!(obj.pads.len(), 4, "pads sized to NumberOfDocks");
        assert_eq!(obj.pads[0].lepton_offset, (0, -128, 0));
        assert_eq!(obj.pads[1].lepton_offset, (0, 128, 0));
        assert_eq!(
            obj.pads[2].lepton_offset,
            (0, 0, 0),
            "missing index 2 zero-init"
        );
        assert_eq!(
            obj.pads[3].lepton_offset,
            (0, 0, 0),
            "missing index 3 zero-init"
        );
    }

    #[test]
    fn merge_pads_truncates_excess_offsets() {
        // NumberOfDocks=2 but art has 4 offsets. Truncate.
        let rules = parse_rules_with_art(
            "[GAAIRC]\nName=Airforce\nCost=1000\nStrength=1000\nNumberOfDocks=2\n",
            "[GAAIRC]\n\
             DockingOffset0=0,0,0\n\
             DockingOffset1=128,0,0\n\
             DockingOffset2=256,0,0\n\
             DockingOffset3=384,0,0\n",
        );
        let obj = rules.object("GAAIRC").expect("obj");
        assert_eq!(obj.pads.len(), 2, "truncated to NumberOfDocks=2");
        assert_eq!(obj.pads[0].lepton_offset, (0, 0, 0));
        assert_eq!(obj.pads[1].lepton_offset, (128, 0, 0));
    }

    #[test]
    fn gsi_04_05_reservation_ai_base_spacing_default_signed_and_writer_gates() {
        let default_rules = RuleSet::from_ini_with_fixed_art_for_test(
            &IniFile::from_str("[BuildingTypes]\n0=PLAIN\n[PLAIN]\nName=Plain\n"),
            &IniFile::from_str("[PLAIN]\nFoundation=2x2\n"),
        )
        .expect("default AI spacing rules");
        assert_eq!(default_rules.ai_base_spacing, 1);
        assert_eq!(
            default_rules
                .object("PLAIN")
                .unwrap()
                .base_reservation_spacing,
            Some(1)
        );

        let rules = RuleSet::from_ini_with_fixed_art_for_test(
            &IniFile::from_str(
                "[AI]\nAIBaseSpacing=-3\n\
             [BuildingTypes]\n\
             0=PLAIN\n1=GATHER\n2=UNDEPLOY\n3=UNDEPLOYGATHER\n4=UNDEPLOYONE\n\
             [PLAIN]\nFoundation=2x2\n\
             [GATHER]\nFoundation=2x2\nResourceGatherer=yes\n\
             [UNDEPLOY]\nFoundation=2x2\nUndeploysInto=MCV\n\
             [UNDEPLOYGATHER]\nFoundation=2x2\nUndeploysInto=MCV\nResourceGatherer=yes\n\
             [UNDEPLOYONE]\nFoundation=1x1\nUndeploysInto=MCV\n",
            ),
            &IniFile::from_str(
                "[PLAIN]\nFoundation=2x2\n[GATHER]\nFoundation=2x2\n\
             [UNDEPLOY]\nFoundation=2x2\n[UNDEPLOYGATHER]\nFoundation=2x2\n\
             [UNDEPLOYONE]\nFoundation=1x1\n",
            ),
        )
        .expect("signed AI spacing rules");
        assert_eq!(rules.ai_base_spacing, -3);
        assert_eq!(
            rules.object("PLAIN").unwrap().base_reservation_spacing,
            Some(-3)
        );
        assert_eq!(
            rules.object("GATHER").unwrap().base_reservation_spacing,
            Some(-3)
        );
        assert_eq!(
            rules.object("UNDEPLOY").unwrap().base_reservation_spacing,
            Some(-3)
        );
        assert_eq!(
            rules
                .object("UNDEPLOYGATHER")
                .unwrap()
                .base_reservation_spacing,
            None
        );
        assert_eq!(
            rules
                .object("UNDEPLOYONE")
                .unwrap()
                .base_reservation_spacing,
            None
        );
    }

    #[test]
    fn gsi_04_11_infantry_death_anim_bindings_use_general_and_animation_order() {
        let ini = IniFile::from_str(
            "[General]\n\
             InfantryExplode=EX3\n\
             FlamingInfantry=EX4\n\
             InfantryHeadPop=EX6\n\
             InfantryNuked=EX7\n\
             InfantryVirus=EX8\n\
             InfantryMutate=EX9\n\
             InfantryBrute=EX10\n\
             [Animations]\n\
             99=FIRST_DECLARED\n\
             2=SECOND_DECLARED\n",
        );
        let parsed = RuleSet::from_ini(&ini).unwrap().general;
        assert_eq!(parsed.infantry_death_anim(0), None);
        assert_eq!(parsed.infantry_death_anim(1), None);
        assert_eq!(parsed.infantry_death_anim(2), None);
        assert_eq!(parsed.infantry_death_anim(3), Some("EX3"));
        assert_eq!(parsed.infantry_death_anim(4), Some("EX4"));
        // Slot 5 is the second declared Animation, which no General key names.
        assert_eq!(
            GeneralRules::from_ini(&ini).infantry_death_anim(5),
            Some("SECOND_DECLARED")
        );
        assert_eq!(parsed.infantry_death_anim(6), Some("EX6"));
        assert_eq!(parsed.infantry_death_anim(7), Some("EX7"));
        assert_eq!(parsed.infantry_death_anim(8), Some("EX8"));
        assert_eq!(parsed.infantry_death_anim(9), Some("EX9"));
        assert_eq!(parsed.infantry_death_anim(10), Some("EX10"));

        let defaults = GeneralRules::default();
        assert_eq!(defaults.infantry_death_anim(3), None);
        assert_eq!(defaults.infantry_death_anim(5), Some("ELECTRO"));
        assert_eq!(defaults.infantry_death_anim(10), None);
    }

    #[test]
    fn simulation_config_hash_covers_registered_slot_names_offsets_and_runtime() {
        let ini =
            IniFile::from_str("[BuildingTypes]\n0=B\n[B]\nImage=BODY\n[Animations]\n0=N\n1=D\n");
        let make = |slot: &str, runtime: &str| {
            let art_ini = IniFile::from_str(&format!(
                "[BODY]\nActiveAnim=N\n{slot}\n[N]\n{runtime}\n[D]\n"
            ));
            let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art_ini).unwrap();
            let art = crate::rules::art_data::ArtRegistry::from_ini(&art_ini);
            rules.install_art_data(art);
            rules
        };
        let original = make("", "Rate=900");
        for (slot, runtime) in [
            ("ActiveAnimDamaged=D", "Rate=900"),
            ("ActiveAnimX=1", "Rate=900"),
            ("ActiveAnimY=-1", "Rate=900"),
            ("ActiveAnimZAdjust=7", "Rate=900"),
            ("ActiveAnimYSort=2", "Rate=900"),
            ("", "Rate=450"),
            ("", "RandomRate=900,300"),
            ("", "Next=D"),
        ] {
            let changed = make(slot, runtime);
            assert_eq!(original.source_ini_hash(), changed.source_ini_hash());
            assert_ne!(
                original.simulation_config_hash(),
                changed.simulation_config_hash(),
                "{slot}, {runtime}"
            );
        }
        let mut changed = make("", "Rate=900");
        changed.anim_type_names.insert("ADDED".into());
        assert_ne!(
            original.simulation_config_hash(),
            changed.simulation_config_hash()
        );
    }

    #[test]
    fn simulation_config_hash_covers_effective_building_body_art() {
        use crate::rules::art_data::ArtRegistry;
        let ini = IniFile::from_str("[BuildingTypes]\n0=BUILD\n[BUILD]\nImage=BODY\nGate=yes\n");
        let make = |art: &str| {
            let mut rules = RuleSet::from_ini(&ini).unwrap();
            rules.install_art_data(ArtRegistry::from_ini(&IniFile::from_str(art)));
            rules
        };
        let defaults =
            "GateStages=9\nAnimIdle=0,1,0\nAnimActive=0,1,0\nAnimAux1=0,1,0\nAnimAux2=0,1,0\n";
        let original = make(&format!("[BODY]\n{defaults}"));
        let reordered = make(&format!("[UNUSED]\nGateStages=-1\n[BODY]\n{defaults}"));
        assert_eq!(
            original.simulation_config_hash(),
            reordered.simulation_config_hash(),
            "unused art and section insertion order do not alter resolved body inputs"
        );
        assert_eq!(
            make("[UNUSED]\nGateStages=99\n").simulation_config_hash(),
            original.simulation_config_hash(),
            "missing metadata and explicit constructor defaults are equivalent"
        );
        assert_eq!(
            make(&format!(
                "[BODY]\n{defaults}PowerUp1Anim=UNCONSUMED\nPowerUp1LocXX=123\n"
            ))
            .simulation_config_hash(),
            original.simulation_config_hash(),
            "flat PowerUp declarations cannot replace the retained Type inputs"
        );
        for changed in [
            "GateStages=-1\n",
            "AnimIdle=-1,1,0\n",
            "AnimActive=0,2147483647,0\n",
            "AnimAux1=0,1,-2\n",
            "AnimAux2=65536,1,0\n",
        ] {
            let changed = make(&format!("[BODY]\n{changed}"));
            assert_eq!(original.source_ini_hash(), changed.source_ini_hash());
            assert_ne!(
                original.simulation_config_hash(),
                changed.simulation_config_hash()
            );
        }
        let mut firestorm = make(&format!("[BODY]\n{defaults}"));
        firestorm.object_list[0].firestorm_wall = true;
        assert_ne!(
            original.simulation_config_hash(),
            firestorm.simulation_config_hash()
        );
    }

    #[test]
    fn simulation_config_hash_changes_with_resolved_effect_frame_count() {
        let ini = IniFile::from_str("[General]\nWarpOut=WARPOUT\n");
        let mut first = RuleSet::from_ini(&ini).expect("first rules");
        let mut second = RuleSet::from_ini(&ini).expect("second rules");

        first.set_effect_frame_count_for_test("WARPOUT", 5, 5);
        second.set_effect_frame_count_for_test("WARPOUT", 6, 6);

        assert_eq!(first.source_ini_hash(), second.source_ini_hash());
        assert_ne!(
            first.simulation_config_hash(),
            second.simulation_config_hash()
        );
    }

    #[test]
    fn simulation_config_hash_changes_with_terrain_spawner_frame_count() {
        let ini = IniFile::from_str(
            "[TerrainTypes]\n0=TIBTRE01\n\
             [TIBTRE01]\nSpawnsTiberium=yes\nIsAnimated=yes\n",
        );
        let mut first = RuleSet::from_ini(&ini).expect("first rules");
        let mut second = RuleSet::from_ini(&ini).expect("second rules");

        first.set_terrain_spawner_frame_count_for_test("TIBTRE01", 22);
        second.set_terrain_spawner_frame_count_for_test("TIBTRE01", 24);

        assert_eq!(first.source_ini_hash(), second.source_ini_hash());
        assert_ne!(
            first.simulation_config_hash(),
            second.simulation_config_hash()
        );
    }

    #[test]
    fn simulation_config_hash_covers_retained_projectile_color() {
        let make = |color: &str| {
            RuleSet::from_ini(&IniFile::from_str(&format!(
                "[Colors]\nFirst=0,0,0\nSecond=1,2,3\n[VehicleTypes]\n0=UNIT\n[UNIT]\nPrimary=GUN\n[GUN]\nProjectile=SHOT\n[SHOT]\nColor={color}\n",
            )))
            .unwrap()
        };
        let first = make("First");
        let mut second = make("Second");
        assert_eq!(first.projectile("SHOT").unwrap().color_scheme(), 1);
        assert_eq!(second.projectile("SHOT").unwrap().color_scheme(), 3);
        // Isolate the effective retained field from the raw-text fingerprint.
        second.source_ini_hash = first.source_ini_hash;
        assert_ne!(
            first.simulation_config_hash(),
            second.simulation_config_hash()
        );
    }

    /// The ordered processor retains each reference and the weapon range
    /// through missing keys and later passes, so equal current text can carry
    /// different values. Each one is part of the identity.
    #[test]
    fn simulation_config_hash_covers_retained_references_and_range() {
        fn unit(rules: &mut RuleSet) -> &mut ObjectType {
            rules
                .object_list
                .iter_mut()
                .find(|object| object.id == "UNIT")
                .unwrap()
        }
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=UNIT\n[UNIT]\nPrimary=GUN\n\
             [GUN]\nRange=5\nProjectile=SHOT\nWarhead=WH\n[SHOT]\n[WH]\n\
             [SuperWeaponTypes]\n0=NUKE\n[NUKE]\nType=MultiMissile\n\
             [Particles]\n0=GAS\n[GAS]\n[VoxelAnims]\n0=PIECE\n[PIECE]\n",
        ))
        .unwrap();
        let base = rules.simulation_config_hash();
        let name = || Some("NAME".to_owned());
        let names = || vec!["NAME".to_owned()];
        let edits: Vec<(&str, fn(&mut RuleSet))> = vec![
            ("Image", |rules| unit(rules).image = "OTHER".into()),
            ("DeathWeapon", |rules| {
                unit(rules).death_weapon = Some("GUN".into());
            }),
            ("DeploysInto", |rules| {
                unit(rules).deploys_into = Some("GUN".into());
            }),
            ("OccupyWeapon", |rules| {
                unit(rules).occupy_weapon = Some("GUN".into());
            }),
            ("EliteOccupyWeapon", |rules| {
                unit(rules).elite_occupy_weapon = Some("GUN".into());
            }),
            ("Range", |rules| {
                rules.weapons.get_mut("GUN").unwrap().range_leptons = 1;
            }),
            ("ShrapnelWeapon", |rules| {
                rules.projectiles.get_mut("SHOT").unwrap().shrapnel_weapon = Some("GUN".into());
            }),
            ("WeaponType", |rules| {
                rules.super_weapons.get_mut("NUKE").unwrap().weapon_type = Some("GUN".into());
            }),
        ];
        for (key, edit) in edits {
            let mut changed = rules.clone();
            edit(&mut changed);
            assert_ne!(changed.simulation_config_hash(), base, "{key}");
        }

        // The references the ordered processor alone reads since part 3.
        type Edit = Box<dyn Fn(&mut RuleSet)>;
        let unit_edit =
            |set: fn(&mut ObjectType)| -> Edit { Box::new(move |rules| set(unit(rules))) };
        let edits: Vec<(&str, Edit)> = vec![
            (
                "UndeploysInto",
                unit_edit(|unit| unit.undeploys_into = Some("NAME".into())),
            ),
            (
                "PowersUnit",
                unit_edit(|unit| unit.powers_unit = Some("NAME".into())),
            ),
            (
                "FreeUnit",
                unit_edit(|unit| unit.free_unit = Some("NAME".into())),
            ),
            (
                "ToOverlay",
                unit_edit(|unit| unit.to_overlay = Some("NAME".into())),
            ),
            ("Dock", unit_edit(|unit| unit.dock = vec!["NAME".into()])),
            (
                "UnloadingClass",
                unit_edit(|unit| unit.unloading_class = Some("NAME".into())),
            ),
            (
                "DeployingAnim",
                unit_edit(|unit| unit.deploying_anim = Some("NAME".into())),
            ),
            (
                "Enslaves",
                unit_edit(|unit| unit.enslaves = Some("NAME".into())),
            ),
            (
                "Spawns",
                unit_edit(|unit| unit.spawns = Some("NAME".into())),
            ),
            (
                "Explosion",
                unit_edit(|unit| unit.explosion_anims = vec!["NAME".into()]),
            ),
            (
                "DestroyAnim",
                unit_edit(|unit| unit.destroy_anims = vec!["NAME".into()]),
            ),
            (
                "DeadBodies",
                unit_edit(|unit| unit.dead_bodies = vec!["NAME".into()]),
            ),
            (
                "NaturalParticleSystem",
                unit_edit(|unit| unit.natural_particle_system = Some("NAME".into())),
            ),
            (
                "RefinerySmokeParticleSystem",
                unit_edit(|unit| unit.refinery_smoke_particle_system = Some("NAME".into())),
            ),
            (
                "DamageParticleSystems",
                unit_edit(|unit| unit.damage_particle_systems = vec!["NAME".into()]),
            ),
            (
                "DestroyParticleSystems",
                unit_edit(|unit| unit.destroy_particle_systems = vec!["NAME".into()]),
            ),
            (
                "DebrisTypes",
                unit_edit(|unit| unit.debris_types = vec!["NAME".into()]),
            ),
            (
                "DebrisAnims",
                unit_edit(|unit| unit.debris_anims = vec!["NAME".into()]),
            ),
            (
                "AuxBuilding",
                Box::new(move |rules| {
                    rules.super_weapons.get_mut("NUKE").unwrap().aux_building = name()
                }),
            ),
            (
                "Anim",
                Box::new(move |rules| rules.weapons.get_mut("GUN").unwrap().anim = names()),
            ),
            (
                "OccupantAnim",
                Box::new(move |rules| rules.weapons.get_mut("GUN").unwrap().occupant_anim = name()),
            ),
            (
                "OpenToppedAnim",
                Box::new(move |rules| {
                    rules.weapons.get_mut("GUN").unwrap().open_topped_anim = name()
                }),
            ),
            (
                "AttachedParticleSystem",
                Box::new(move |rules| {
                    rules
                        .weapons
                        .get_mut("GUN")
                        .unwrap()
                        .attached_particle_system = name();
                }),
            ),
            (
                "Warhead DebrisTypes",
                Box::new(move |rules| rules.warheads.get_mut("WH").unwrap().debris_types = names()),
            ),
            (
                "Particle Warhead",
                Box::new(move |rules| rules.particle_types[0].warhead = name()),
            ),
            (
                "VoxelAnim Spawns",
                Box::new(move |rules| rules.voxel_anim_types[0].spawns = name()),
            ),
            (
                "VoxelAnim BounceAnim",
                Box::new(move |rules| rules.voxel_anim_types[0].bounce_anim = name()),
            ),
            (
                "VoxelAnim ExpireAnim",
                Box::new(move |rules| rules.voxel_anim_types[0].expire_anim = name()),
            ),
            (
                "VoxelAnim TrailerAnim",
                Box::new(move |rules| rules.voxel_anim_types[0].trailer_anim = name()),
            ),
            (
                "VoxelAnim Warhead",
                Box::new(move |rules| rules.voxel_anim_types[0].warhead = name()),
            ),
            (
                "VoxelAnim AttachedSystem",
                Box::new(move |rules| rules.voxel_anim_types[0].attached_system = name()),
            ),
            (
                "AnimType references",
                Box::new(|rules| {
                    rules.anim_type_references.push((
                        "ANIM".into(),
                        crate::rules::native_processing::AnimTypeReferences::default(),
                    ));
                }),
            ),
            (
                "C4Warhead",
                Box::new(|rules| rules.bridge_warheads.c4_name = "NAME".into()),
            ),
            (
                "CrushWarhead",
                Box::new(|rules| rules.bridge_warheads.crush_name = "NAME".into()),
            ),
            (
                "IonCannonWarhead",
                Box::new(|rules| rules.bridge_warheads.ion_cannon_name = "NAME".into()),
            ),
            (
                "IvanWarhead",
                Box::new(move |rules| rules.combat_damage.ivan_warhead = name()),
            ),
            (
                "DrainAnimationType",
                Box::new(move |rules| rules.combat_damage.drain_animation_type = name()),
            ),
            (
                "ControlledAnimationType",
                Box::new(move |rules| rules.mind_control.controlled_anim = name()),
            ),
            (
                "PermaControlledAnimationType",
                Box::new(move |rules| rules.mind_control.perma_controlled_anim = name()),
            ),
            (
                "RadSiteWarhead",
                Box::new(|rules| rules.radiation.site_warhead = "NAME".into()),
            ),
            (
                "MutateWarhead",
                Box::new(|rules| rules.general.mutate_warhead = "NAME".into()),
            ),
            (
                "MutateExplosionWarhead",
                Box::new(|rules| rules.general.mutate_explosion_warhead = "NAME".into()),
            ),
            (
                "EMPulseWarhead",
                Box::new(|rules| rules.general.empulse_warhead = Some("NAME".into())),
            ),
            (
                "OreTwinkle",
                Box::new(move |rules| rules.general.ore_twinkle = name()),
            ),
            (
                "DominatorWarhead",
                Box::new(|rules| rules.general.dominator_warhead = "NAME".into()),
            ),
            (
                "Engineer",
                Box::new(move |rules| rules.general.engineer_infantry = name()),
            ),
            (
                "DamageFireTypes",
                Box::new(|rules| {
                    rules.general.damage_fire_types = vec![AnimRef {
                        name: "NAME".into(),
                    }];
                }),
            ),
            (
                "DeadBodies",
                Box::new(move |rules| rules.general.dead_bodies = names()),
            ),
            (
                "BuildConst",
                Box::new(move |rules| rules.build_const_types = names()),
            ),
            (
                "ConcreteWalls",
                Box::new(move |rules| rules.concrete_wall_types = names()),
            ),
            (
                "NSGates",
                Box::new(move |rules| rules.ns_gate_types = names()),
            ),
            (
                "EWGates",
                Box::new(move |rules| rules.ew_gate_types = names()),
            ),
            (
                "CrateImg",
                Box::new(move |rules| rules.crate_rules.crate_img = name()),
            ),
            (
                "CrateMaximum",
                Box::new(|rules| rules.crate_rules.maximum = 7),
            ),
            (
                "V3RocketDamage",
                Box::new(|rules| rules.missile_spawn.v3.damage = 7),
            ),
        ];
        for (key, edit) in edits {
            let mut changed = rules.clone();
            edit(&mut changed);
            assert_ne!(changed.simulation_config_hash(), base, "{key}");
        }
        for (key, publish) in crate::rules::rules_type_references::DEFAULT_PARTICLE_SYSTEMS {
            let mut changed = rules.clone();
            publish(&mut changed, "NAME");
            assert_ne!(changed.simulation_config_hash(), base, "{key}");
        }
    }

    #[test]
    fn simulation_config_hash_covers_effective_projectile_launch_art_and_load() {
        use crate::rules::art_data::ArtRegistry;
        use crate::sim::snapshot::{GameSnapshot, SnapshotError};
        let ini = IniFile::from_str(
            "[VehicleTypes]\n0=UNIT\n[UNIT]\nPrimary=GUN\n[GUN]\nProjectile=SHOT\n[SHOT]\nImage=SHOTART\n[BuildingTypes]\n0=BUILD\n[BUILD]\nImage=BUILDART\n",
        );
        let make = |art: &str| {
            let mut rules =
                RuleSet::from_ini_with_fixed_art_for_test(&ini, &IniFile::from_str(art)).unwrap();
            rules.install_art_data(ArtRegistry::from_ini(&IniFile::from_str(art)));
            rules
        };
        let first =
            make("[SHOTART]\nVoxel=yes\nFlat=yes\n[BUILDART]\nHeight=4\n[UNUSED]\nHeight=9\n");
        let reordered = make(
            "[UNUSED]\nHeight=200\nVoxel=yes\nFlat=no\n[BUILDART]\nHeight=4\n[SHOTART]\nVoxel=yes\nFlat=yes\n",
        );
        let voxel_changed = make("[SHOTART]\nVoxel=no\nFlat=yes\n[BUILDART]\nHeight=4\n");
        let height_changed = make("[SHOTART]\nVoxel=yes\nFlat=yes\n[BUILDART]\nHeight=5\n");
        let flat_changed = make("[SHOTART]\nVoxel=yes\nFlat=no\n[BUILDART]\nHeight=4\n");
        assert!(first.projectile("SHOT").unwrap().voxel);
        assert!(first.projectile("SHOT").unwrap().flat);
        assert_eq!(
            first.building_launch_height(first.object("BUILD").unwrap()),
            4
        );
        assert_eq!(
            first.simulation_config_hash(),
            reordered.simulation_config_hash(),
            "canonical consumed inputs ignore ART section order and unused metadata"
        );
        for changed in [&voxel_changed, &height_changed, &flat_changed] {
            assert_eq!(first.source_ini_hash(), changed.source_ini_hash());
            assert_ne!(
                first.simulation_config_hash(),
                changed.simulation_config_hash()
            );
        }
        let absent = make("[UNUSED]\nHeight=999\n");
        let explicit_default = make("[SHOTART]\nVoxel=no\nFlat=no\n[BUILDART]\nHeight=2\n");
        assert_eq!(
            absent.simulation_config_hash(),
            explicit_default.simulation_config_hash(),
            "authored presence alone is not a consumed launch input"
        );
        let mut retained = make("[SHOTART]\nVoxel=yes\nFlat=yes\n[BUILDART]\nHeight=4\n");
        retained.install_art_data(ArtRegistry::from_ini(&IniFile::from_str(
            "[BUILDART]\nHeight=4\n",
        )));
        assert_eq!(
            first.simulation_config_hash(),
            retained.simulation_config_hash(),
            "hash the retained effective ProjectileType value after an absent Voxel key"
        );
        let sim = crate::sim::world::Simulation::new();
        let bytes =
            GameSnapshot::save_validated(&sim, 11, first.simulation_config_hash(), "launch ART", 0);
        assert!(
            GameSnapshot::load_validated(
                &bytes,
                11,
                reordered.simulation_config_hash(),
                &sim.session.map_name()
            )
            .is_ok()
        );
        for changed in [&voxel_changed, &height_changed, &flat_changed] {
            assert!(matches!(
                GameSnapshot::load_validated(
                    &bytes,
                    11,
                    changed.simulation_config_hash(),
                    &sim.session.map_name()
                ),
                Err(SnapshotError::RulesMismatch { .. })
            ));
        }
    }

    #[cfg(feature = "phobos")]
    #[test]
    fn simulation_config_hash_covers_phobos_warhead_animation_rules() {
        let make = |keys: &str, general: &str| {
            RuleSet::from_ini(&IniFile::from_str(&format!(
                "[General]\n{general}\n[Warheads]\n0=WH\n[WH]\nAnimList=A\n{keys}\n\
                 [Animations]\n0=A\n"
            )))
            .unwrap()
        };
        let base = make("", "");
        let mut per_warhead = make("AnimList.CreateAll=yes\n", "");
        let mut global = make("", "Conventional.IgnoreUnits=yes\n");
        per_warhead.source_ini_hash = base.source_ini_hash;
        global.source_ini_hash = base.source_ini_hash;
        assert_ne!(
            base.simulation_config_hash(),
            per_warhead.simulation_config_hash(),
            "a per-warhead animation key changes the simulation config"
        );
        assert_ne!(
            base.simulation_config_hash(),
            global.simulation_config_hash(),
            "a [General] animation default changes the simulation config"
        );
    }

    #[cfg(feature = "phobos")]
    #[test]
    fn simulation_config_hash_covers_warhead_jumpjet_overrides() {
        let make = |keys: &str| {
            RuleSet::from_ini(&IniFile::from_str(&format!(
                "[Warheads]\n0=LIFT\n[LIFT]\nIsLocomotor=yes\nLocomotor=Jumpjet\n{keys}\n"
            )))
            .unwrap()
        };
        let base = make("");
        let mut changed = make("JumpjetSpeed=33\n");
        changed.source_ini_hash = base.source_ini_hash;
        assert_ne!(
            base.simulation_config_hash(),
            changed.simulation_config_hash(),
            "a warhead Jumpjet override changes the simulation config"
        );
    }

    #[cfg(feature = "phobos")]
    #[test]
    fn simulation_config_hash_covers_mind_control_enhancements() {
        let make = |general: &str, unit: &str, warhead: &str| {
            RuleSet::from_ini(&IniFile::from_str(&format!(
                "[General]\n{general}\n[VehicleTypes]\n0=UNIT\n[UNIT]\n{unit}\n\
                 [Warheads]\n0=WH\n[WH]\n{warhead}\n[Animations]\n0=MIND\n"
            )))
            .unwrap()
        };
        let base = make("", "", "");
        let mut size = make("", "MindControlSize=2\n", "");
        let mut globals = make("MindControl.IgnoreSize=no\n", "", "");
        let mut anim = make("", "", "MindControl.Anim=MIND\n");
        size.source_ini_hash = base.source_ini_hash;
        globals.source_ini_hash = base.source_ini_hash;
        anim.source_ini_hash = base.source_ini_hash;
        assert_ne!(
            base.simulation_config_hash(),
            size.simulation_config_hash(),
            "a per-type MindControlSize changes the simulation config"
        );
        assert_ne!(
            base.simulation_config_hash(),
            globals.simulation_config_hash(),
            "a [General] IgnoreSize default changes the simulation config"
        );
        assert_ne!(
            base.simulation_config_hash(),
            anim.simulation_config_hash(),
            "a warhead MindControl.Anim changes the simulation config"
        );
    }

    #[test]
    fn simulation_config_hash_covers_canonical_smudge_anim_dimensions() {
        let ini = IniFile::from_str("[InfantryTypes]\n[VehicleTypes]\n");
        let mut first = RuleSet::from_ini(&ini).expect("first rules");
        let mut reordered = RuleSet::from_ini(&ini).expect("reordered rules");
        let mut changed = RuleSet::from_ini(&ini).expect("changed rules");

        let mut first_art = crate::rules::art_data::ArtRegistry::from_ini(&IniFile::from_str(
            "[BIGCRATER]\nCrater=yes\n[SCORCH]\nScorch=yes\n",
        ));
        let mut reordered_art = crate::rules::art_data::ArtRegistry::from_ini(&IniFile::from_str(
            "[SCORCH]\nScorch=yes\n[BIGCRATER]\nCrater=yes\n",
        ));
        for art in [&mut first_art, &mut reordered_art] {
            let big = art.get_mut("BIGCRATER").expect("big crater art");
            big.frame_width = 61;
            big.frame_height = 51;
        }
        let mut changed_art = first_art.clone();
        let changed_big = changed_art
            .get_mut("BIGCRATER")
            .expect("changed crater art");
        changed_big.frame_width = 60;
        changed_big.frame_height = 50;

        first.install_art_data(first_art);
        reordered.install_art_data(reordered_art);
        changed.install_art_data(changed_art);

        assert_eq!(first.source_ini_hash(), reordered.source_ini_hash());
        assert_eq!(
            first.simulation_config_hash(),
            reordered.simulation_config_hash(),
            "art section insertion order must not affect compatibility"
        );
        assert_ne!(
            first.simulation_config_hash(),
            changed.simulation_config_hash()
        );
    }

    /// Retail difficulty rows and country ROF through the production reader:
    /// `[Easy] ROF=.8`, `[Normal] ROF=1.0`, `[Difficult] ROF=1.2` (ReadDouble
    /// widens the `%f` single), and no retail country sets `ROF=`, so every
    /// country keeps the constructor's 1.0.
    #[test]
    fn retail_difficulty_and_country_rof() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_rulesmd_rules() else {
            return;
        };
        assert_eq!(
            rules.general.difficulty_rows.map(|row| row.rof),
            [f64::from(0.8f32), 1.0, f64::from(1.2f32)]
        );
        for country in ["Americans", "Russians", "YuriCountry"] {
            assert_eq!(
                rules.country_difficulty_biases(country).rof,
                NativeF64Bits::ONE,
                "{country}"
            );
        }
    }

    /// Retail corpse anims through the production reader: `[General]
    /// DeadBodies=` lists six, and the GI and Conscript name none of their own
    /// and are not `NotHuman=`, so their deaths pick from the six.
    #[test]
    fn retail_dead_bodies() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_rulesmd_rules() else {
            return;
        };
        assert_eq!(
            rules.general.dead_bodies,
            [
                "DEATH_A", "DEATH_B", "DEATH_C", "DEATH_D", "DEATH_E", "DEATH_F"
            ]
        );
        for infantry in ["E1", "E2"] {
            let object = rules.object(infantry).unwrap();
            assert!(
                object.dead_bodies.is_empty() && !object.not_human,
                "{infantry}"
            );
        }
    }

    /// Retail `FireAngle=` through the production reader: ships, submarines,
    /// missiles and the tech outpost set it (`CAOUTP` behind a trailing
    /// comment); the Grizzly and Rhino keep the constructor's 8.
    #[test]
    fn retail_fire_angle() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_rulesmd_rules() else {
            return;
        };
        for (object, fire_angle) in [
            ("DEST", 32),
            ("BSUB", 64),
            ("V3ROCKET", 1),
            ("CAOUTP", 0),
            ("MTNK", 8),
            ("HTNK", 8),
        ] {
            assert_eq!(
                rules.object(object).unwrap().fire_angle,
                fire_angle,
                "{object}"
            );
        }
    }

    #[test]
    fn weight_reads_keep_the_current_value_between_rules_passes() {
        // TechnoType ctor710C66..710C71 initializes double1.0; reader
        //7124CF..7124EA passes that current value to ReadDouble5283D0.
        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[VehicleTypes]\n0=MTNK\n[MTNK]\nStrength=300\n",
        ));
        for (text, expected) in [
            ("[MTNK]\nArmor=heavy\n", SimFixed::ONE),
            ("[MTNK]\nWeight=3.5\n", SimFixed::lit("3.5")),
            ("[MTNK]\nSpeed=6\n", SimFixed::lit("3.5")),
            // Native ReadDouble scans malformed authored text as zero; only
            // an absent key retains the current field.
            ("[MTNK]\nWeight=invalid\n", SimFixed::ZERO),
            ("[MTNK]\nSpeed=7\n", SimFixed::ZERO),
        ] {
            layers.push(RulesLayerKind::Scenario, IniFile::from_str(text));
            let rules = RuleSet::from_rules_layers(&layers).unwrap();
            assert_eq!(rules.object("MTNK").unwrap().weight, expected);
        }
    }

    #[test]
    fn bunker_range_default_and_layered_reads_match_original() {
        use crate::rules::native_processing::RulesLayerKind;

        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/walk_cell_range_bunker.json",
        ))
        .unwrap();
        assert_eq!(
            i64::from(GarrisonRules::default().bunker_weapon_range_bonus),
            native["inputs"]["constructor"]["bunker_bonus"]
                .as_i64()
                .unwrap()
        );
        for row in native["inputs"]["bunker_reader_controls"]
            .as_array()
            .unwrap()
        {
            let mut layers = RulesLayerStack::new(IniFile::empty());
            for (keys, expected) in row["layers"]
                .as_array()
                .unwrap()
                .iter()
                .zip(row["values"].as_array().unwrap())
            {
                let mut text = String::from("[CombatDamage]\n");
                for (key, value) in keys.as_object().unwrap() {
                    text.push_str(&format!("{key}={}\n", value.as_str().unwrap()));
                }
                layers.push(RulesLayerKind::Scenario, IniFile::from_str(&text));
                let rules = RuleSet::from_rules_layers(&layers).unwrap();
                assert_eq!(
                    i64::from(rules.garrison_rules.bunker_weapon_range_bonus),
                    expected.as_i64().unwrap(),
                    "{}",
                    row["name"]
                );
            }
        }
    }

    #[test]
    fn retail_lasher_range_inputs_match_original_readers() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules() else {
            return;
        };
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/walk_cell_range_bunker.json",
        ))
        .unwrap();
        let tank = rules.object("LTNK").unwrap();
        let primary = tank.primary().unwrap();
        let weapon = rules.weapon(primary).unwrap();
        let projectile_name = weapon.projectile.as_deref().unwrap();
        let projectile = rules.projectile(projectile_name).unwrap();
        // This check compares the same physical RULESMD pass. The joined
        // Battle/map fixture separately covers production scenario loading.
        assert_eq!(
            serde_json::json!({
                "primary": primary,
                "projectile": projectile_name,
                "range": weapon.range_leptons,
                "minimum": weapon.minimum_range_leptons,
                "cell_rangefinding": weapon.cell_rangefinding,
                "arcing": projectile.arcing,
                "subject_to_elevation": projectile.subject_to_elevation,
                "floater": projectile.floater,
                "gravity": rules.general.gravity,
                "bunker_bonus": rules.garrison_rules.bunker_weapon_range_bonus,
                "open_topped_bonus": rules.garrison_rules.open_topped_range_bonus,
            }),
            native["inputs"]["layers"][0]["state"],
        );
    }

    #[test]
    fn cheer_sound_binds_registered_names_with_reached_pass_retention() {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        use crate::rules::sound_ini::SoundRegistry;

        let sounds = SoundRegistry::from_ini(&IniFile::from_str(
            "[SoundList]\n0=FirstCheer\n1=MapCheer\n[FirstCheer]\n[MapCheer]\n",
        ));
        for (later, expected) in [
            ("", Some("FirstCheer")),
            ("CheerSound=\n", Some("FirstCheer")),
            ("CheerSound=Unknown\n", Some("FirstCheer")),
            ("cheersound=MapCheer\n", Some("FirstCheer")),
            ("CheerSound=mapcheer\n", Some("MapCheer")),
        ] {
            let mut layers =
                RulesLayerStack::new(IniFile::from_str("[AudioVisual]\nCheerSound=FirstCheer\n"));
            layers.push(
                RulesLayerKind::Scenario,
                IniFile::from_str(&format!("[AudioVisual]\n{later}")),
            );
            let processed = layers.process().unwrap();
            let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
            rules.bind_type_sound_references(processed.ini(), &sounds);
            assert_eq!(rules.general.cheer_sound(), expected, "{later:?}");
        }
        let ini = IniFile::empty();
        let mut absent = RuleSet::from_ini(&ini).unwrap();
        absent.bind_type_sound_references(&ini, &sounds);
        assert_eq!(absent.general.cheer_sound(), None, "constructor6658E0=-1");
    }

    /// Retail `[ElevationModel]` through the production reader: four levels a
    /// step, two cells a step, two cells at most. Without the section the
    /// constructor's 0, 1.0 and 0.0 stay.
    #[test]
    fn retail_elevation_model() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_rulesmd_rules() else {
            return;
        };
        let two = NativeF64Bits::from_bits(2.0f64.to_bits());
        let model = rules.elevation_model;
        assert_eq!(
            (model.increment, model.increment_bonus, model.bonus_cap),
            (4, two, two)
        );
        let model = ElevationModel::from_ini(&IniFile::from_str("[General]\n"));
        assert_eq!(
            (model.increment, model.increment_bonus, model.bonus_cap),
            (0, NativeF64Bits::ONE, NativeF64Bits::POSITIVE_ZERO)
        );
    }

    #[cfg(feature = "phobos")]
    #[test]
    fn parallel_ai_queue_switches_default_the_gate_on_and_read_global_controls() {
        // Phobos RulesExt constructs AllowParallelAIQueues { true }.
        let empty = RuleSet::from_ini(&IniFile::from_str("")).unwrap();
        let controls = empty.parallel_ai_queues();
        assert!(controls.allows_parallel());
        assert!(!controls.forbids_category(ObjectCategory::Building, false));

        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[GlobalControls]\nAllowParallelAIQueues=no\n\
             ForbidParallelAIQueues.Infantry=yes\nForbidParallelAIQueues.Navy=yes\n",
        ))
        .unwrap();
        let controls = rules.parallel_ai_queues();
        assert!(!controls.allows_parallel());
        assert!(controls.forbids_category(ObjectCategory::Infantry, false));
        assert!(!controls.forbids_category(ObjectCategory::Vehicle, false));
        assert!(controls.forbids_category(ObjectCategory::Vehicle, true));
        assert!(!controls.forbids_category(ObjectCategory::Aircraft, false));
        assert!(!controls.forbids_category(ObjectCategory::Building, false));
    }

    #[cfg(feature = "phobos")]
    #[test]
    fn techno_forbid_parallel_ai_queues_reads_per_type() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n0=BLOCK\n1=OPEN\n[BLOCK]\nForbidParallelAIQueues=yes\n[OPEN]\n",
        ))
        .unwrap();
        assert!(rules.object("BLOCK").unwrap().forbid_parallel_ai_queues);
        assert!(!rules.object("OPEN").unwrap().forbid_parallel_ai_queues);
    }
}

#[cfg(test)]
#[path = "map_lighting_tests.rs"]
mod map_lighting_tests;

#[cfg(test)]
#[path = "global_promotion_tests.rs"]
mod global_promotion_tests;

#[cfg(all(test, feature = "ares"))]
#[path = "ares_script_conversion_tests.rs"]
mod ares_script_conversion_tests;
