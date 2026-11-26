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
use std::hash::Hash;

use crate::rules::ares_prerequisites::GenericPrerequisites;
use crate::rules::combat_damage::CombatDamageDefaults;
use crate::rules::crate_rules::CrateRules;
#[cfg(test)]
use crate::rules::error::RulesError;
use crate::rules::ini_parser::{IniFile, IniSection};
use crate::rules::mission_data::MissionControl;
#[cfg(test)]
use crate::rules::native_processing::RulesLayerStack;
use crate::rules::native_processing::{CountryVeterancyState, ProcessedRulesLayers};
use crate::rules::object_type::{ObjectCategory, ObjectType};
use crate::rules::particle_system_type::{ParticleSystemType, ParticleSystemTypeId};
use crate::rules::particle_type::ParticleType;
use crate::rules::prerequisite::Prerequisite;
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
#[cfg(test)]
use crate::util::fixed_math::SimFixed;
use crate::util::lookup_hash::LookupMap;
use crate::util::native_x87::{NativeF32Bits, NativeF64Bits};

mod assets;
mod config_hash;
mod country_side;
mod extensions;
mod general;
mod loading;
mod lookup;
mod sections;
mod sound_names;

pub use general::GeneralRules;
pub(crate) use general::{
    DEFAULT_AMBIENT_CHANGE_RATE, DEFAULT_AMBIENT_CHANGE_STEP, DEFAULT_MAXIMUM_CHEER_RATE,
    DEFAULT_SURVIVOR_DIVISORS, ambient_change_terms, default_fog_rate, default_shroud_rate,
    read_cliff_back_impassability,
};
#[cfg(feature = "phobos")]
pub(crate) use general::{DEFAULT_MISSING_CAMEO, default_missing_cameo};
pub use sections::{
    BridgeRules, DetailRules, DifficultyRules, ElevationModel, GarrisonRules, GeneralBuildingTypes,
    ParadropList, PrismSupportRules, ProductionRules, RadiationRules,
};
pub use sound_names::{SoundName, SoundNameId, SoundNames};

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    /// Test builds only: this thread's [`RuleSet::object`] calls, for checks
    /// of what a draw looks up by name ([`RuleSet::type_name_lookups`]). The
    /// bound handle's debug cross-check calls `type_handle` and is not counted.
    static TYPE_NAME_LOOKUPS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    /// Test builds only: this thread's uppercase copies of a type name made
    /// to look it up ([`upper_name_key`], [`RuleSet::name_key_copies`]).
    static NAME_KEY_COPIES: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "building_damage_sound_tests.rs"]
mod building_damage_sound_tests;

#[cfg(all(test, tests_rules_o_z))]
#[path = "command_bar_sound_rules_tests.rs"]
mod command_bar_sound_rules_tests;

#[cfg(all(test, tests_rules_o_z))]
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
    pub(crate) presentation: crate::rules::ares_country_presentation::AresCountryPresentation,
    /// Ares country `ParaDrop.*` / `Parachute.Anim=` overrides. Simulation
    /// state: which units a paradrop superweapon sends.
    pub(crate) paradrop: crate::rules::ares_country_paradrop::AresCountryParadrop,
    /// Ares country `AI.PowerPlants=` candidates. Simulation state: which
    /// building the AI splices when its base runs short of power.
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

    /// Ares `HouseClass_GetCostMult` (`10041780`, hook `0x0050BEB0`,
    /// resuming at its `ret 4` `0x0050BF1E`), the FactoryPlant factor every
    /// caller stores as a float: `1 - (1 - product) * FactoryPlant.Multiplier`,
    /// spilled as binary64 (`10041833`). A multiplier of 1.0 returns the
    /// stock product.
    pub(crate) fn ares_factory_plant(
        &self,
        slot: usize,
        multiplier: crate::util::native_x87::NativeF64Bits,
    ) -> NativeF32Bits {
        use crate::util::native_x87::{MaskedX87Chop53 as X87, NativeF64Bits};
        let one = X87::load_f64(NativeF64Bits::ONE);
        let discount = X87::sub(one, X87::load_f32(self.factory_plant[slot]));
        let factor = X87::store_f64_masked_chop(X87::sub(
            one,
            X87::mul(discount, X87::load_f64(multiplier)),
        ));
        X87::store_f32_masked_chop(X87::load_f64(factor))
    }

    /// [`Self::adjust`] through [`Self::ares_factory_plant`].
    pub(crate) fn ares_adjust(
        &self,
        cost: i32,
        slot: usize,
        multiplier: crate::util::native_x87::NativeF64Bits,
    ) -> i32 {
        use crate::util::native_x87::MaskedX87Chop53 as X87;
        X87::ftol_i32_low_masked(X87::mul(
            X87::mul(
                X87::load_i32(cost),
                X87::load_f32(self.ares_factory_plant(slot, multiplier)),
            ),
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
            presentation: crate::rules::ares_country_presentation::AresCountryPresentation::default(
            ),
            paradrop: crate::rules::ares_country_paradrop::AresCountryParadrop::default(),
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
            presentation: crate::rules::ares_country_presentation::AresCountryPresentation::default(
            ),
            paradrop: crate::rules::ares_country_paradrop::AresCountryParadrop::default(),
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

/// VERA's bound on `[AI] AIBaseSpacing=`'s magnitude. The base reservation
/// (`0x00455F10`), its neighbour repair (`0x004561F0`, foundation plus five
/// spacings) and the AI site search (`0x0050B760`) walk cell rectangles
/// grown by it, so a value near either end of `i32` walks about 2^31 by 2^31
/// cells and freezes the match; retail spaces bases by 1.
pub const AI_BASE_SPACING_LIMIT: i32 = 16;

/// Registry section names in rules.ini and their corresponding category.
const TYPE_REGISTRIES: &[(&str, ObjectCategory)] = &[
    ("InfantryTypes", ObjectCategory::Infantry),
    ("VehicleTypes", ObjectCategory::Vehicle),
    ("AircraftTypes", ObjectCategory::Aircraft),
    ("BuildingTypes", ObjectCategory::Building),
];

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

fn next_rule_set_serial() -> u64 {
    static SERIALS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    SERIALS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1
}

/// O(1) index into `RuleSet::object_list`. Resolved once from a name (or from an
/// interned id via the sim `TypeHandleTable`), then dereferenced directly —
/// avoiding the per-call string round-trip + hash lookup of name resolution.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, serde::Serialize, serde::Deserialize)]
pub struct TypeHandle(pub u32);

/// O(1) index into a `RuleSet`'s weapons, which [`RuleSet::weapon_handle`]
/// resolves a name to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WeaponHandle(u32);

/// O(1) index into a `RuleSet`'s warheads.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct WarheadHandle(u32);

/// O(1) index into a `RuleSet`'s projectiles.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct ProjectileHandle(u32);

/// A weapon's `Warhead=` and `Projectile=` types, which gamemd's
/// WeaponTypeClass holds as pointers (`+0xAC`, `+0xA0`), resolved once by
/// the set that holds the weapon ([`RuleSet::resolve_handles`]), so
/// per-shot reads skip hashing the names. Derived: never hashed or
/// serialized.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct WeaponParts {
    /// The resolving set's serial; 0 for a weapon no set resolved.
    rules_serial: u64,
    warhead: Option<WarheadHandle>,
    projectile: Option<ProjectileHandle>,
}

/// A weapon name as one `RuleSet` resolved it ([`RuleSet::weapon_handle`]).
/// [`RuleSet::resolved_weapon`] reads the handle only in that set and looks
/// the name up anywhere else.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ResolvedWeapon {
    /// The resolving set's serial; 0, which no set has, when unresolved.
    rules_serial: u64,
    handle: Option<WeaponHandle>,
}

/// The weapons a type's records name — `Weapon[]`, `EliteWeapon[]`,
/// `OccupyWeapon=` and `EliteOccupyWeapon=` — resolved once by the set that
/// built the type ([`RuleSet::resolve_handles`]), so per-object reads
/// skip hashing the name. Derived: never hashed or serialized.
#[derive(Clone, Debug, Default)]
pub struct TypeWeapons {
    /// The resolving set's serial; 0 for a type no set resolved.
    rules_serial: u64,
    base: Box<[Option<WeaponHandle>]>,
    elite: Box<[Option<WeaponHandle>]>,
    /// `OccupyWeapon=`, then `EliteOccupyWeapon=`.
    occupy: [Option<WeaponHandle>; 2],
}

impl TypeWeapons {
    /// The base or, `elite`, the elite record at `index`.
    #[inline]
    pub(crate) fn slot(&self, index: usize, elite: bool) -> ResolvedWeapon {
        let slots = if elite { &self.elite } else { &self.base };
        ResolvedWeapon {
            rules_serial: self.rules_serial,
            handle: slots.get(index).copied().flatten(),
        }
    }

    /// `OccupyWeapon=` or, `elite`, `EliteOccupyWeapon=`.
    #[inline]
    pub(crate) fn occupy(&self, elite: bool) -> ResolvedWeapon {
        ResolvedWeapon {
            rules_serial: self.rules_serial,
            handle: self.occupy[usize::from(elite)],
        }
    }
}

/// A `DebrisTypes=` list, a TechnoType's or a warhead's, as the set that
/// built it resolved its names to `[VoxelAnims]` types
/// ([`RuleSet::resolve_handles`]): list order kept, `None` for a name no
/// `[VoxelAnims]` section has, so a death or a hit reads the ids without
/// hashing a name ([`RuleSet::object_debris_types`],
/// [`RuleSet::warhead_debris_types`]). Derived: never hashed or serialized.
#[derive(Clone, Debug, Default)]
pub(crate) struct ResolvedDebris {
    /// The resolving set's serial; 0 for a list no set resolved.
    rules_serial: u64,
    types: Box<[Option<VoxelAnimTypeId>]>,
}

/// `id` as the uppercase key the type name indexes hold: the name itself
/// unless it has a lowercase letter. Retail type names are uppercase, so a
/// lookup by name rarely copies it.
fn upper_name_key(id: &str) -> std::borrow::Cow<'_, str> {
    if id.bytes().any(|byte| byte.is_ascii_lowercase()) {
        #[cfg(any(test, feature = "test-support"))]
        NAME_KEY_COPIES.with(|count| count.set(count.get() + 1));
        std::borrow::Cow::Owned(id.to_ascii_uppercase())
    } else {
        std::borrow::Cow::Borrowed(id)
    }
}

/// A value for each type name of the four native TechnoType arrays: one
/// uppercase-keyed [`LookupMap`] per array, so a lookup borrows the name it
/// is given ([`upper_name_key`]) and hashes it once with the cheap hasher.
/// Only looked up, never walked.
#[derive(Clone, Debug)]
struct CategoryIndex<V>([LookupMap<String, V>; 4]);

impl<V> Default for CategoryIndex<V> {
    fn default() -> Self {
        Self(Default::default())
    }
}

impl<V: Copy> CategoryIndex<V> {
    fn array(&self, category: ObjectCategory) -> &LookupMap<String, V> {
        &self.0[Self::slot(category)]
    }

    fn slot(category: ObjectCategory) -> usize {
        match category {
            ObjectCategory::Infantry => 0,
            ObjectCategory::Vehicle => 1,
            ObjectCategory::Aircraft => 2,
            ObjectCategory::Building => 3,
        }
    }

    /// The value of `id` in `category`'s array, ignoring ASCII case.
    fn get(&self, category: ObjectCategory, id: &str) -> Option<V> {
        self.array(category).get(&*upper_name_key(id)).copied()
    }

    fn contains(&self, category: ObjectCategory, id: &str) -> bool {
        self.get(category, id).is_some()
    }

    fn insert(&mut self, category: ObjectCategory, id: &str, value: V) {
        self.0[Self::slot(category)].insert(id.to_ascii_uppercase(), value);
    }

    /// The value of `id`, after giving it `value` if it had none.
    fn get_or_insert(&mut self, category: ObjectCategory, id: &str, value: V) -> V {
        *self.0[Self::slot(category)]
            .entry(id.to_ascii_uppercase())
            .or_insert(value)
    }
}

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
    /// Every sound name the set's fields hold, as the Copy ids sound events
    /// carry. Kept out of the simulation's interner ([`SoundNames`]).
    sound_names: SoundNames,
    /// Per-native-registry identity authority. Unlike the broad lookup above,
    /// this retains distinct types when malformed/custom rules list the same
    /// ID in more than one category.
    object_category_index: CategoryIndex<TypeHandle>,
    /// Read-only projection of the process owner's global TechnoType array.
    /// Constructor order can interleave categories across lazy allocations
    /// and later Rules passes; it is not TYPE_REGISTRIES order. An absent
    /// handle records a native slot the older name-based object projection
    /// cannot represent uniquely, so exact lookup must not silently alias it.
    native_techno_type_order: Vec<(ObjectCategory, String, Option<TypeHandle>)>,
    /// Uppercase ID → the first [`Self::native_techno_type_order`] slot of
    /// that ID ignoring ASCII case: [`Self::ares_find_techno_type`]'s answer,
    /// built with the order and never changed.
    ares_techno_type_first: LookupMap<String, usize>,
    /// Case-insensitive identity of each type in the four native TechnoType
    /// registries to its ordered array index (`+0xDF8`).
    type_array_indices: CategoryIndex<i32>,
    /// All weapons' handles indexed by ID (e.g., "105mm" → its
    /// `weapon_types` index). Built once with the set, never changed.
    weapons: LookupMap<String, WeaponHandle>,
    /// The weapons `weapons` indexes, one per key.
    weapon_types: Vec<WeaponType>,
    /// Derived from the one processed native Weapon family, in allocation
    /// order. The bool admits only slots whose Weapon/Warhead stored IDs map
    /// unambiguously into the existing name projection. Rebuilt with Rules;
    /// it never allocates or mutates Type state (see issue1784).
    weapon_array: Vec<(String, bool)>,
    /// All warheads' handles indexed by ID (e.g., "AP" → its
    /// `warhead_types` index). Built once with the set, never changed.
    warheads: LookupMap<String, WarheadHandle>,
    /// The warheads `warheads` indexes, one per key.
    warhead_types: Vec<WarheadType>,
    /// All projectiles' handles indexed by ID (e.g., "InvisibleLow" → its
    /// `projectile_types` index). Built once with the set, never changed.
    projectiles: LookupMap<String, ProjectileHandle>,
    /// The projectiles `projectiles` indexes, one per key.
    projectile_types: Vec<ProjectileType>,
    /// Country-level rules indexed by country/house type ID.
    countries: HashMap<String, CountryRules>,
    /// Country identities in native `[Countries]` registration order.
    country_ids: Vec<String>,
    /// Uppercase country identity -> source-order index; only looked up.
    country_indices: LookupMap<String, CountryIdx>,
    /// Uppercase `Name=` alias or identity -> the first Country in source
    /// order that carries it, as `HouseTypeClass__FindIndexOfName @
    /// 0x005117D0`'s scan finds it ([`Self::country_index_of_name`]). Built
    /// with `countries` and `country_ids`, its sources; only looked up.
    country_name_indices: LookupMap<String, CountryIdx>,
    /// Side identities in native `[Sides]` registration order, including
    /// sides allocated later by a per-country `Side=` override.
    side_ids: Vec<String>,
    /// Commander personalities from `[AIPersonalities]`, in registration order.
    ///
    /// Empty for every rules set without that section — which is all retail
    /// data — and an empty catalog is what keeps the feature entirely inert.
    personalities: crate::rules::personality::PersonalityCatalog,
    /// `[CommanderFeedback]`: the talking-head layer's own switches, next to
    /// the personalities they govern.
    commander_feedback: crate::rules::personality::CommanderFeedbackRules,
    /// `[EndGameScreens]`: per-country score-page music and backgrounds.
    ///
    /// Empty for every rules set without that section — which is all retail
    /// data — and an empty catalog leaves the side art and the native `SCORE`
    /// theme untouched.
    end_game_screens: crate::rules::end_game::EndGameCatalog,
    /// Read-only projection of the live SideExt owner, in the same native
    /// array order as side_ids. Rebuilt after every Process/reset/save restore.
    /// Empty when the Rules were read the stock way, which allocates no SideExt.
    side_extensions: Vec<crate::rules::ares_side_extension::AresSideExtension>,
    /// Uppercase side identity -> source-order index.
    side_indices: HashMap<String, SideIdx>,
    /// Phobos `[SOMESIDE] Sidebar.GDIPositions=`, in `[Sides]` order.
    /// The first side defaults to GDI positions; all others default to the
    /// NOD table. Unlike SideExt+88 this is read in stock games too.
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
    /// `[Colors]` scheme entries in declaration order, then the built-in extra
    /// lobby colours that section leaves out
    /// ([`crate::rules::skirmish_colors`]). Source of truth for every
    /// house-color producer (loading-bar backing, lobby swatch, map `Color=`,
    /// skirmish slot priority).
    pub color_schemes: Vec<crate::rules::color_scheme::ColorSchemeEntry>,
    /// The warm colours enemies draw in under VERA's team-colour view
    /// (Alt+T), team A first, taken from the colours a lobby already offers
    /// ([`crate::rules::team_colors`]). Empty when the rules list no warm
    /// colour, which leaves every enemy on the relation view's red.
    pub enemy_team_palette: crate::rules::team_colors::EnemyTeamPalette,
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
    /// Signed `[AI] AIBaseSpacing`; constructor default is 1. Its magnitude
    /// is at most [`AI_BASE_SPACING_LIMIT`].
    pub ai_base_spacing: i32,
    /// Source-ordered `[General] Shipyard=` BuildingType identities.
    pub shipyard_types: Vec<String>,
    /// Retained `[General] RepairBay=` BuildingType identities (Rules+850).
    pub repair_bay_types: Vec<String>,
    /// Source-ordered resolved `[AI] BuildConst=` BuildingType identities.
    build_const_types: Vec<String>,
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
    /// Original-RA2 `[AI] TiberiumNearScan=` (cell radius to scan for ore in a
    /// single patch, game.exe RulesClass+0x13E0). Absent in YR; default 0
    /// matches the native constructor. Consumed when the RA2 profile is active.
    pub tiberium_near_scan: i32,
    /// Original-RA2 `[AI] TiberiumFarScan=` (cell radius to find a new ore
    /// patch, game.exe RulesClass+0x13E4). Absent in YR; default 0.
    pub tiberium_far_scan: i32,
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
    /// The `[General] Prerequisite*` lists (Rules `+0x358..+0x3E4`), in
    /// [`PrerequisiteGroup::ALL`] order: an on-map building of any of them
    /// meets that group in CanBuild.
    prerequisite_lists: [Vec<Prerequisite>; 6],
    /// Immutable gameplay projection of the retained Rules registry catalog.
    ares_prerequisites: Option<GenericPrerequisites>,
    /// Immutable projection of NativeRulesRegistryState's ordered armor catalog.
    ares_armors: Option<crate::rules::ares_armor::ArmorCatalog>,
    emp_sparkles: Option<String>,
    /// Whether Ares plays in this game, decided once from its Rules layers
    /// and `ares.mix` by `rules::ares_switch`. Read through [`Self::ares`].
    ares: bool,
    /// VERA: the player's Ares setting Off is what keeps Ares from playing in
    /// this game. Read through [`Self::ares_kept_off`].
    ares_kept_off: bool,
    /// Whether Phobos plays in this game, decided once from its Rules
    /// projection by `rules::phobos_switch`. Read through [`Self::phobos`].
    phobos: bool,
    /// Whether Kratos plays in this game, decided once from the reads its
    /// processed rules and projection made by `rules::kratos_switch`. Read
    /// through [`Self::kratos`].
    kratos: bool,
    /// Whether NPatch plays in this game, decided once from the reads its
    /// processed rules and projection made by `rules::npatch_switch`. Read
    /// through [`Self::npatch`].
    npatch: bool,
    /// VERA: whether this game plays RA2 Mode (docs/ra2-mode.md), decided once
    /// at load from the launch's choice (`util::ra2_launch`). Read through
    /// [`Self::ra2`].
    ra2: bool,
    /// Whether its Rules were read the Ares way (`ares_switch::ares_way`).
    /// Read through [`Self::reads_ares_way`].
    reads_ares_way: bool,
    ares_global_controls: crate::rules::native_processing::AresGlobalControls,
    /// Presentation projection of the retained Ares `[MouseCursors]` list.
    ares_mouse_cursors: crate::rules::ares_mouse_cursor::MouseCursors,
    /// Each SuperWeaponType's Ares `Cursor`/`NoCursor`, by uppercase ID.
    ares_super_weapon_cursors: HashMap<String, crate::rules::ares_mouse_cursor::SuperWeaponCursors>,
    /// Derived type-membership index for the lifecycle hooks; rebuilt together
    /// with the RuleSet after every load/reset/pass. The catalog above is its
    /// only source of truth. This cache is not serialized or hashed separately.
    ares_prerequisite_alternates: HashSet<TypeHandle>,
    /// All registered SW availability, independent of supported effect Types.
    /// Immutable projection of the retained ProcessedRules registry; the typed
    /// aux/list fields remain read-only compatibility views from that source.
    ares_super_weapon_metadata:
        BTreeMap<String, crate::rules::superweapon_type::AresSuperWeaponMetadata>,
    /// Phobos `SW.TechLevel`/`SW.AuxTechnos`/`SW.NegTechnos` for every
    /// registered superweapon, projected from the retained registry.
    #[cfg(feature = "phobos")]
    phobos_super_weapon_metadata:
        BTreeMap<String, crate::rules::superweapon_type::PhobosSuperWeaponAvailability>,
    /// Phobos `[GlobalControls] AllowParallelAIQueues=` and the
    /// `ForbidParallelAIQueues.*` category switches.
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
    overlay_registry: std::sync::Arc<crate::rules::overlay_types::OverlayTypeRegistry>,
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
    /// Per-cell radiation-field constants from [Radiation]. `site_warhead` is
    /// resolved once when the set is built: a write to it afterwards is not
    /// seen by [`Self::radiation_site_warhead`].
    pub radiation: RadiationRules,
    /// Derived: the warhead `radiation.site_warhead` names, as
    /// [`Self::resolve_handles`] resolved it ([`Self::radiation_site_warhead`]).
    radiation_site_warhead: Option<WarheadHandle>,
    /// Phobos `[ShieldTypes]` catalog, read per Rules pass by the ordered
    /// Process owner. Empty in builds without the capability.
    #[cfg(feature = "phobos")]
    pub(crate) shield_types: crate::rules::phobos_shields::ShieldTypeCatalog,
    /// Phobos `[InsigniaTypes]` catalog.
    #[cfg(feature = "phobos")]
    pub(crate) insignia_types: crate::rules::insignia_type::InsigniaTypeCatalog,
    /// Phobos `[General]`/`[AudioVisual]` insignia settings.
    #[cfg(feature = "phobos")]
    pub(crate) phobos_draw_globals: crate::rules::insignia_type::PhobosDrawGlobals,
    /// Phobos shield globals: `[CombatDamage] ShieldApplyArmorMult` and
    /// `[AudioVisual] Shield.ConditionYellow/ConditionRed`.
    #[cfg(feature = "phobos")]
    pub(crate) shield_globals: crate::rules::phobos_shields::ShieldGlobals,
    /// Phobos crit defaults: `[CombatDamage] Crit.*` and `[General]
    /// VeteranCritChance`.
    #[cfg(feature = "phobos")]
    pub(crate) crit_globals: crate::rules::phobos_crit::CritGlobals,
    /// Phobos Iron Curtain / Force Shield organics, tint and deploy settings.
    #[cfg(feature = "phobos")]
    pub(crate) invulnerability_globals: crate::rules::invulnerability_rules::InvulnerabilityGlobals,
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
    /// The player-colour conversion every launcher and loading-screen consumer
    /// reads, and the colours a lobby lists: `[SkirmishColors]` and Phobos
    /// `[General] SkirmishUnlimitedColors=`.
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
    /// Includes distinct runtime keys for colliding native slots. Membership
    /// is independent of a same-named art section or loaded SHP.
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
    pub(crate) voxel_model_plan: super::voxel_model_plan::VoxelModelPlan,
    /// Every building type's construction control from its Buildup SHP,
    /// bound once from the active assets (`bind_building_buildup_assets`).
    buildup_assets: crate::rules::buildup_asset_catalog::BuildupAssetCatalog,
    /// Rubble art of every `LeaveRubble=yes` building type, bound once from
    /// the active assets (`bind_building_rubble_assets`).
    rubble_assets: crate::rules::building_rubble::RubbleAssetCatalog,
    /// Raw terrain SHP counts used by authoritative TIBTRE animation timing.
    /// Presentation keeps its separate body-frame projection.
    terrain_assets: crate::rules::terrain_asset_catalog::TerrainAssetCatalog,
    /// Every loaded ObjectType `AlphaImage=` SHP (`bind_alpha_images`). Not
    /// in `simulation_config_hash`: it places and draws light glows. An Ares
    /// particle system also picks its simulation by whether its held type's
    /// loaded (`particles::ares_system`), which `file_inputs_hash` folds.
    alpha_images: crate::rules::alpha_image_assets::AlphaImageCatalog,
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

/// Source for the shared sound binder. Production uses the retained process
/// receipt; synthetic tests can explicitly supply a bare INI without a live
/// native registry. There is one binding implementation for both inputs.
#[doc(hidden)]
#[derive(Clone, Copy)]
pub enum SoundReferenceSource<'a> {
    Processed(&'a ProcessedRulesLayers),
    #[cfg(any(test, feature = "test-support"))]
    Fixture(&'a IniFile),
}

impl<'a> From<&'a ProcessedRulesLayers> for SoundReferenceSource<'a> {
    fn from(value: &'a ProcessedRulesLayers) -> Self {
        Self::Processed(value)
    }
}

#[cfg(any(test, feature = "test-support"))]
impl<'a> From<&'a IniFile> for SoundReferenceSource<'a> {
    fn from(value: &'a IniFile) -> Self {
        Self::Fixture(value)
    }
}

impl<'a> SoundReferenceSource<'a> {
    fn ini(self) -> &'a IniFile {
        match self {
            Self::Processed(processed) => processed.ini(),
            #[cfg(any(test, feature = "test-support"))]
            Self::Fixture(ini) => ini,
        }
    }
}

impl RuleSet {
    /// Ares1003DB30/1003DB50. Both group labels and ordinary type IDs share
    /// one namespace; callers compare these keys case-insensitively.
    pub fn type_selection_key<'a>(&'a self, category: ObjectCategory, type_id: &'a str) -> &'a str {
        if self.ares
            && let Some(object) = self.object_in_category(category, type_id)
        {
            return object.type_selection_key();
        }
        type_id
    }

    /// CollectSelectedIDs732D47 -> Ares10041990/10041A3F gates the
    /// DeploysInto/UndeploysInto seeds. A stock game keeps retail behavior.
    pub fn type_select_uses_deploy(&self) -> bool {
        if self.ares {
            self.ares_global_controls.type_select_use_deploy()
        } else {
            true
        }
    }

    /// RulesExt `[General] ChronoInfantryCrush` (constructor true): whether
    /// warping infantry may land on a cell holding anything but infantry.
    pub(crate) fn ares_chrono_infantry_crush(&self) -> bool {
        self.ares_global_controls.chrono_infantry_crush()
    }

    /// RulesExt `[General]`/`[AudioVisual]` keys of the team, starting-unit,
    /// wrench, disk-laser and electric-death hooks.
    pub(crate) fn ares_general(&self) -> &crate::rules::native_processing::AresGeneralRules {
        self.ares_global_controls.general()
    }

    /// RulesExt `[General] EnemyWrench` (constructor true): a player not
    /// allied with a repairing building's owner sees its wrench.
    pub fn ares_enemy_wrench(&self) -> bool {
        self.ares_global_controls.general().enemy_wrench()
    }

    /// RulesExt `[CrateRules] RandomCrateMoney` (constructor 900).
    pub(crate) fn ares_random_crate_money(&self) -> i32 {
        self.ares_global_controls.random_crate_money()
    }

    /// RulesExt `[General] RepairStopOnInsufficientFunds` (constructor true).
    pub(crate) fn ares_repair_stop_on_insufficient_funds(&self) -> bool {
        self.ares_global_controls
            .repair_stop_on_insufficient_funds()
    }

    pub(crate) fn bounty_rules(&self) -> &crate::rules::ares_bounty::BountyGlobalRules {
        self.ares_global_controls.bounty()
    }

    /// RulesExt CloakHeight, dim factors and detection speak delays.
    pub fn ares_cloak_power(&self) -> &crate::rules::ares_cloak_power::CloakPowerGlobalRules {
        self.ares_global_controls.cloak_power()
    }

    /// RulesExt `[AudioVisual] VeteranFlashTimer` (`rules::ares_veterancy`).
    pub(crate) fn ares_veterancy(&self) -> &crate::rules::ares_veterancy::VeterancyGlobalRules {
        self.ares_global_controls.veterancy()
    }

    /// The Ares `[MouseCursors]` list the client draws cursors from
    /// ([`crate::rules::ares_mouse_cursor`]); presentation only.
    pub fn ares_mouse_cursors(&self) -> &crate::rules::ares_mouse_cursor::MouseCursors {
        &self.ares_mouse_cursors
    }

    /// The `[MouseCursors]` cursors `SWTypeExt::ExtData::WhatAction @
    /// 10033EB0` shows over a cell superweapon `id` may or may not fire at.
    /// It answers only for a type a NewSWType handles or whose `Action=` is
    /// `Custom`; the others keep the stock action cursor. VERA's handled types
    /// are its implemented handlers (`SuperWeaponKind::has_ares_handler`).
    pub fn ares_super_weapon_cursors(
        &self,
        id: &str,
    ) -> Option<crate::rules::ares_mouse_cursor::SuperWeaponCursors> {
        let sw = self.super_weapon(id)?;
        (sw.kind.has_ares_handler()
            || sw
                .action
                .as_deref()
                .is_some_and(|action| action.eq_ignore_ascii_case("Custom")))
        .then(|| {
            self.ares_super_weapon_cursors
                .get(&sw.id.to_ascii_uppercase())
                .copied()
                .unwrap_or_default()
        })
    }

    /// RulesExt BerserkROFMultiplier, AutoRepel and PlayerAutoRepel.
    pub(crate) fn ares_techno_combat(
        &self,
    ) -> &crate::rules::ares_techno_type::TechnoCombatGlobalRules {
        self.ares_global_controls.techno_combat()
    }

    /// RulesExt EngineerDamage and EngineerAlwaysCaptureTech.
    pub(crate) fn ares_building_globals(
        &self,
    ) -> &crate::rules::ares_building_type::BuildingGlobalRules {
        self.ares_global_controls.building()
    }

    /// RulesExt DamageAirConsiderBridges and IronCurtainFlash.
    pub(crate) fn ares_warhead_globals(&self) -> &crate::rules::ares_warhead::WarheadGlobalRules {
        self.ares_global_controls.warhead()
    }

    /// RulesExt TiberiumDamageEnabled, TiberiumHealEnabled, the explosive
    /// warhead and anim, and OverlayExplodeThreshold.
    pub(crate) fn ares_tiberium_globals(
        &self,
    ) -> &crate::rules::ares_tiberium::TiberiumGlobalRules {
        self.ares_global_controls.tiberium()
    }

    /// RulesExt's firestorm anims, warhead and damage coefficient
    /// (`+0xF0..+0x108`, `sim::ares_firestorm`).
    pub(crate) fn ares_firestorm_globals(
        &self,
    ) -> &crate::rules::ares_firestorm::FirestormGlobalRules {
        self.ares_global_controls.firestorm()
    }

    /// Ares RulesExt+70, used by SW_HunterSeeker::Activate100728EB.
    pub(crate) fn hunter_seeker_buildings(&self) -> &[String] {
        self.ares_global_controls.hunter_seeker_buildings()
    }

    /// Ares RulesExt's DropPod globals with the stock locomotor keys, used by
    /// SW_DropPod::Activate (10070E40) and the DropPod locomotor.
    pub(crate) fn ares_drop_pod(&self) -> &crate::rules::superweapon_type::AresDropPodGlobalRules {
        self.ares_global_controls.drop_pod()
    }

    /// `TreeFire=` and `TreeFlammability=`, read by the Ares terrain fire.
    pub(crate) fn forest_fire(&self) -> &crate::rules::terrain_object_type::ForestFireRules {
        self.ares_global_controls.forest_fire()
    }

    /// RulesExt+14A indexed by native House difficulty0/1/2 (hardest first).
    /// Only the Ares CanBuild owner consumes this retained policy.
    pub fn ares_ai_bypasses_build_limit(&self, difficulty_index: usize) -> bool {
        if self.ares {
            self.ares_global_controls
                .ai_bypasses_build_limit(difficulty_index)
        } else {
            false
        }
    }

    /// RulesExt+160/+164 retained nullable named mission. A stock game
    /// neither reads this extension key nor supplies an AI EMP recovery producer.
    pub(crate) fn ares_emp_ai_recover_mission(
        &self,
    ) -> Option<crate::rules::mission_data::MissionType> {
        if self.ares {
            self.ares_global_controls.emp_ai_recover_mission()
        } else {
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

    /// Project processed Rules into the game's rules, the Ares way when their
    /// registry reads that way (`ares_switch::ares_way`). The projection's
    /// reader lookups are recorded, so the per-game Ares and Phobos switches
    /// it stores ([`Self::ares`], [`Self::phobos`]) also count the keys only
    /// these readers read.
    pub fn from_processed_rules(processed: &ProcessedRulesLayers) -> Result<Self, RulesError> {
        let _ares_way = crate::rules::ares_switch::read_ares_way(
            processed.native_registry_state().reads_ares_way(),
        );
        let (rules, reads) = crate::rules::ares_switch::record(|| Self::project(processed));
        let mut rules = rules?;
        rules.ares = processed.ares_inputs().decide(&reads);
        rules.phobos = crate::rules::phobos_switch::decide(&reads);
        rules.reads_ares_way = processed.native_registry_state().reads_ares_way();
        Ok(rules)
    }

    fn project(processed: &ProcessedRulesLayers) -> Result<Self, RulesError> {
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
                .get(category, id)
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
        for (category, id, coefficients) in processed.techno_threat_coefficients() {
            if let Some(handle) = rules.object_category_index.get(category, id)
                && rules.object_by_handle(handle).id == id
            {
                rules.object_list[handle.0 as usize].project_threat_coefficients(coefficients);
            }
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
        // Phobos TechnoTypeExt reads insignia keys on every reached pass, so
        // the processor's accumulated state is the owner, not a projection of
        // the final merged section.
        #[cfg(feature = "phobos")]
        for (category, name, insignia) in processed.techno_insignia_states() {
            if let Some(object) = rules
                .object_list
                .iter_mut()
                .find(|object| object.category == category && object.id == name)
            {
                object.phobos_insignia = insignia.clone();
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
        rules.general.ai_hate_delays = processed.ai_hate_delays().to_vec();
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
                    .get(alternate.category(), alternate.id())
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
        for (name, conventional, em_effect, sparky, anim_list, debris_types, verses, ares_verses) in
            processed.warhead_read_states()
        {
            if let Some(warhead) = rules.warhead_mut(name) {
                warhead.conventional = conventional;
                warhead.em_effect = em_effect;
                warhead.sparky = sparky;
                warhead.anim_list = anim_list.to_vec();
                warhead.debris_types = debris_types.to_vec();
                warhead.verses_f64 = verses.map(|value| f64::from_bits(value.bits()));
                warhead.install_ares_verses(ares_verses);
            }
        }
        for (name, particle_system) in processed.warhead_particle_states() {
            if let Some(warhead) = rules.warhead_mut(name) {
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
            curtain_modifiers,
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
                    curtain_modifiers,
                );
            }
        }
        // The retained canopy key can enable Ares in a stock-way game too.
        for (category, name, retained) in processed.techno_combat_states() {
            if let Some(handle) = rules.object_category_index.get(category, name) {
                rules.object_list[handle.0 as usize].bind_techno_combat_rules(retained);
            }
        }
        if crate::rules::ares_switch::ares_way() {
            for (category, name, enabled, options) in processed.ares_spotlight_states() {
                if let Some(handle) = rules.object_category_index.get(category, name) {
                    rules.object_list[handle.0 as usize].bind_ares_spotlight(enabled, options);
                }
            }
            for (category, name, retained) in processed.super_weapon_techno_states() {
                if let Some(handle) = rules.object_category_index.get(category, name) {
                    rules.object_list[handle.0 as usize].bind_super_weapon_techno_rules(retained);
                }
            }
            for (category, name, group) in processed.selection_group_states() {
                if let Some(handle) = rules.object_category_index.get(category, name) {
                    rules.object_list[handle.0 as usize].bind_selection_group(group);
                }
            }
            for (category, name, driver) in processed.driver_states() {
                if let Some(handle) = rules.object_category_index.get(category, name) {
                    rules.object_list[handle.0 as usize].bind_driver_rules(driver);
                }
            }
            for (category, name, bounty) in processed.bounty_states() {
                if let Some(handle) = rules.object_category_index.get(category, name) {
                    rules.object_list[handle.0 as usize].bind_bounty_rules(bounty);
                }
            }
            for (category, name, crew) in processed.crew_states() {
                if let Some(handle) = rules.object_category_index.get(category, name) {
                    rules.object_list[handle.0 as usize].bind_crew_rules(crew);
                }
            }
            for (name, infiltration) in processed.infiltration_states() {
                if let Some(handle) = rules
                    .object_category_index
                    .get(ObjectCategory::Building, name)
                {
                    rules.object_list[handle.0 as usize].bind_infiltration_rules(infiltration);
                }
            }
            for (category, name, veterancy) in processed.veterancy_states() {
                if let Some(handle) = rules.object_category_index.get(category, name) {
                    rules.object_list[handle.0 as usize].bind_ares_veterancy(veterancy);
                }
            }
            for (category, name, production) in processed.production_states() {
                if let Some(handle) = rules.object_category_index.get(category, name) {
                    rules.object_list[handle.0 as usize].bind_ares_production(production);
                }
            }
            for (category, name, cursors) in processed.ares_cursor_states() {
                if let Some(handle) = rules.object_category_index.get(category, name) {
                    rules.object_list[handle.0 as usize].bind_ares_cursors(cursors);
                }
            }
            // Ares ValidateThings100677B0 (hook687C16), after every INI
            // read: a Factory= building cannot be a ClonedAt source nor a
            // CloningFacility.
            let factories: HashSet<String> = rules
                .object_list
                .iter()
                .filter(|object| {
                    object.category == ObjectCategory::Building && object.factory_type().is_some()
                })
                .map(|object| object.id.to_ascii_uppercase())
                .collect();
            for object in &mut rules.object_list {
                let factory =
                    object.category == ObjectCategory::Building && object.factory_type().is_some();
                object.ares_production_mut().validate(factory, |source| {
                    factories.contains(&source.to_ascii_uppercase())
                });
            }
            for (name, academy) in processed.academy_states() {
                if let Some(handle) = rules
                    .object_category_index
                    .get(ObjectCategory::Building, name)
                {
                    rules.object_list[handle.0 as usize].bind_academy(academy);
                }
            }
            for (category, name, retained) in processed.cloak_power_states() {
                if let Some(handle) = rules.object_category_index.get(category, name) {
                    rules.object_list[handle.0 as usize].bind_cloak_power_rules(retained);
                }
            }
            // Ares ValidateThings100677B0 (hook687C16), after every INI read:
            // PoweredBy replaces PoweredUnit, and a PowersUnit naming a
            // PoweredBy type is cleared. Both write the live Type.
            let powered_by_types: HashSet<String> = rules
                .object_list
                .iter()
                .filter(|object| !object.cloak_power().powered_by().is_empty())
                .map(|object| object.id.to_ascii_uppercase())
                .collect();
            for object in &mut rules.object_list {
                if !object.cloak_power().powered_by().is_empty() {
                    object.powered_unit = false;
                }
                if object
                    .powers_unit
                    .as_deref()
                    .is_some_and(|unit| powered_by_types.contains(&unit.to_ascii_uppercase()))
                {
                    object.powers_unit = None;
                }
            }
        }
        for (name, ares) in processed.ares_warhead_states() {
            if let Some(warhead) = rules.warhead_mut(name) {
                warhead.bind_ares_rules(ares);
            }
        }
        for (category, name, armor) in processed.object_armor_states() {
            if let Some(category) = category {
                if let Some(handle) = rules.object_category_index.get(category, name) {
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
        [
            rules.general.extra_unit_light,
            rules.general.extra_infantry_light,
            rules.general.extra_aircraft_light,
        ] = processed.class_extra_lights();
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
        for (category, name, is_gattling, stages) in processed.gattling_states() {
            if let Some(object) = rules
                .object_list
                .iter_mut()
                .find(|object| object.category == category && object.id == name)
            {
                object.is_gattling = is_gattling;
                object.gattling_stages = *stages;
            }
        }
        if crate::rules::ares_switch::ares_way() {
            for (category, name, state) in processed.factory_owner_states() {
                if let Some(object) = rules
                    .object_list
                    .iter_mut()
                    .find(|object| object.category == category && object.id == name)
                {
                    object.project_factory_owners(state);
                }
            }
            for (name, state) in processed.explicit_factory_states() {
                if let Some(object) = rules
                    .object_list
                    .iter_mut()
                    .find(|object| object.category == ObjectCategory::Building && object.id == name)
                {
                    object.project_explicit_factory(state);
                }
            }
        }
        for (category, name, references, image, crush_sound) in processed.techno_reference_states()
        {
            if let Some(object) = rules
                .object_list
                .iter_mut()
                .find(|object| object.category == category && object.id == name)
            {
                object.project_image_state(image);
                object.crush_sound = crush_sound.map(str::to_owned);
                references.apply_to(object);
            }
        }
        for (name, trailer) in processed.aircraft_trailer_states() {
            if let Some(object) = rules
                .object_list
                .iter_mut()
                .find(|object| object.category == ObjectCategory::Aircraft && object.id == name)
            {
                object.project_aircraft_trailer(trailer);
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
        for (name, civilian) in processed.infantry_civilian_states() {
            if let Some(object) = rules
                .object_list
                .iter_mut()
                .find(|object| object.category == ObjectCategory::Infantry && object.id == name)
            {
                object.civilian = civilian;
            }
        }
        for (name, sparks) in processed.infantry_damage_spark_states() {
            if let Some(object) = rules
                .object_list
                .iter_mut()
                .find(|object| object.category == ObjectCategory::Infantry && object.id == name)
            {
                object.damage_sparks = sparks;
            }
        }
        for (name, art) in processed.infantry_art_states() {
            if let Some(object) = rules
                .object_list
                .iter_mut()
                .find(|object| object.category == ObjectCategory::Infantry && object.id == name)
            {
                object.project_infantry_art(art);
            }
        }
        for (name, bank) in processed.infantry_sequence_banks() {
            if let Some(object) = rules
                .object_list
                .iter_mut()
                .find(|object| object.category == ObjectCategory::Infantry && object.id == name)
            {
                object.project_infantry_sequence_bank(bank);
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
        rules
            .anim_type_names
            .extend(
                (0..rules.anim_type_art_read_states.len()).filter_map(|index| {
                    crate::rules::native_processing::anim_type_runtime_key(
                        rules
                            .anim_type_art_read_states
                            .iter()
                            .map(|(name, _)| name.as_str()),
                        index,
                    )
                    .map(|key| key.to_ascii_uppercase())
                }),
            );
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
        if crate::rules::ares_switch::ares_way() {
            for (name, ext) in processed.building_type_ext_states() {
                if let Some(handle) = rules
                    .object_category_index
                    .get(ObjectCategory::Building, name)
                {
                    let building = &mut rules.object_list[handle.0 as usize];
                    building.bind_building_type_ext(ext);
                    // AIBaseNormal joins the base-reservation gate above.
                    building.base_reservation_spacing = building
                        .base_reservation_writer_eligible()
                        .then_some(rules.ai_base_spacing);
                }
            }
            // Ares CompleteInitialization 1000D380 (hook 0x679CAF), once the
            // foundations above are final: a rubble pair shares one foundation.
            let foundations: HashMap<String, crate::rules::foundation::Foundation> = rules
                .object_list
                .iter()
                .filter(|object| object.category == ObjectCategory::Building)
                .map(|object| (object.id.to_ascii_uppercase(), object.foundation.clone()))
                .collect();
            for object in rules
                .object_list
                .iter_mut()
                .filter(|object| object.category == ObjectCategory::Building)
            {
                object
                    .complete_building_type_ext(|name| foundations.get(&name.to_ascii_uppercase()));
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
            if let Some(projectile) = rules.projectile_mut(name) {
                art.apply_to(image, projectile);
            }
        }
        for (name, (airburst, shrapnel)) in processed.projectile_weapon_states() {
            if let Some(projectile) = rules.projectile_mut(name) {
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
        if crate::rules::ares_switch::ares_way() {
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
        // Ares 3.0 InfantryClass_ReceiveDamage_ElectricDeath (1002C930, hook
        // 0x518744) replaces the InfDeath=5 anim, AnimTypes[1]: the RulesExt
        // InfantryElectrocuted, else Find("ELECTRO") over the live registry,
        // else AnimTypes[1]. The first death caches it; no later read
        // changes the registry, so it is resolved here.
        if crate::rules::ares_switch::ares_way() {
            let anims = processed
                .anim_type_art_read_states()
                .map(|(id, _)| id)
                .collect::<Vec<_>>();
            let key = |index| {
                crate::rules::native_processing::anim_type_runtime_key(anims.iter().copied(), index)
                    .map(|key| key.into_owned())
            };
            let electric = rules
                .ares_general()
                .infantry_electrocuted()
                .map(str::to_owned)
                .or_else(|| {
                    anims
                        .iter()
                        .position(|id| id.eq_ignore_ascii_case("ELECTRO"))
                        .and_then(key)
                })
                .or_else(|| key(1));
            if electric.is_some() {
                rules.general.infantry_death_anims[5] = electric;
            }
        }
        rules.project_house_masks();
        rules.rebuild_animation_sequences();
        rules.resolve_handles();
        Ok(rules)
    }

    /// Derive every type's [`HouseMasks`] against the current Country
    /// registry. The Rules build and [`Self::refresh_country_side_projection`]
    /// are the registry's only writers, and both end here.
    fn project_house_masks(&mut self) {
        let masks: Vec<HouseMasks> = self
            .object_list
            .iter()
            .map(|object| HouseMasks::derive(object, self))
            .collect();
        for (object, masks) in self.object_list.iter_mut().zip(masks) {
            object.project_house_masks(masks);
        }
    }

    /// The `ids` whose Type has an object of `category`. The ordered Rules
    /// processor allocates a Type for each token its factory misses, and
    /// native keeps it in the list; VERA's projection builds no object for
    /// some of them (a token with surrounding spaces) and drops them here, an
    /// approved exclusion that retail never reaches.
    pub(crate) fn ids_with_objects(&self, ids: &[String], category: ObjectCategory) -> Vec<String> {
        ids.iter()
            .filter(|id| self.object_category_index.contains(category, id))
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
            // `ids_with_objects` kept only ids this index holds.
            if let Some(handle) = self
                .object_category_index
                .get(ObjectCategory::Building, &id)
            {
                self.object_list[handle.0 as usize].build_const_eligible = true;
            }
        }
    }

    /// Resolve type sound readers against the startup-selected
    /// SOUNDMD registry. The processed projection retains only passes where
    /// each type existed, including the native current-ID retention order.
    /// Type defaults and Rules+208 start at -1; [AudioVisual] owns the latter.
    pub fn bind_type_sound_references<'a>(
        &mut self,
        source: impl Into<SoundReferenceSource<'a>>,
        sounds: &crate::rules::sound_ini::SoundRegistry,
    ) {
        let source = source.into();
        let ini = source.ini();
        // Original669423..6694B6, in native open/close read order. The
        // processed section already owns the reached Rules pass history.
        self.general.gui_open_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "GUIOpenSound", None));
        self.general.gui_close_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "GUICloseSound", None));
        self.general.disk_laser_charge_up = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "DiskLaserChargeUp", None));
        self.general.create_unit_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "CreateUnitSound", None));
        self.general.create_infantry_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "CreateInfantrySound", None));
        self.general.create_aircraft_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "CreateAircraftSound", None));
        // Original66A763..66A973, in native read order for these bound fields. Use the same fixed
        // registry and history-aware reader as every other bound Voc ID.
        self.general.game_closed_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "GameClosed", None));
        self.general.incoming_message_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "IncomingMessage", None));
        self.general.message_char_typed_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "MessageCharTyped", None));
        self.general.system_error_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "SystemError", None));
        self.general.options_changed_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "OptionsChanged", None));
        self.general.game_forming_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "GameForming", None));
        self.general.player_left_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "PlayerLeft", None));
        self.general.player_joined_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "PlayerJoined", None));
        self.general.airstrike_abort_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "AirstrikeAbortSound", None));
        self.general.airstrike_attack_voice = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "AirstrikeAttackVoice", None));
        if crate::rules::ares_switch::ares_way() {
            self.general.gate_sounds = ["GateDown", "GateUp"].map(|key| {
                ini.section("AudioVisual")
                    .and_then(|section| sounds.read_rules_reference(section, key, None))
            });
        }
        self.general.leave_bio_reactor_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "LeaveBioReactorSound", None));
        self.general.enter_bio_reactor_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "EnterBioReactorSound", None));
        self.general.enter_grinder_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "EnterGrinderSound", None));
        self.general.sinking_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "SinkingSound", None));
        self.general.score_anim_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "ScoreAnimSound", None));
        self.general.construction_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "Construction", None));
        self.general.building_abandoned_sound = ini.section("AudioVisual").and_then(|section| {
            sounds.read_rules_reference(section, "BuildingAbandonedSound", None)
        });
        self.general.building_damage_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "BuildingDamageSound", None));
        self.general.building_repaired_sound = ini.section("AudioVisual").and_then(|section| {
            sounds.read_rules_reference(section, "BuildingRepairedSound", None)
        });
        self.general.spy_plane_camera = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "SpyPlaneCamera", None));
        if crate::rules::ares_switch::ares_way() {
            self.general.ares_decloak_sound = ini
                .section("AudioVisual")
                .and_then(|section| sounds.read_ares_reference(section, "DecloakSound"));
        }
        self.general.stop_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "StopSound", None));
        self.general.guard_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "GuardSound", None));
        // Original66AAC0..66AB53: fixed process catalog, exact keys and
        // current-ID retention across the existing processed read history.
        self.general.radar_on = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "RadarOn", None));
        self.general.radar_off = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "RadarOff", None));
        self.general.movie_on = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "MovieOn", None));
        self.general.movie_off = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "MovieOff", None));
        // Original66A4EF..66A582 shares the retained fixed-catalog reader.
        self.general.spy_sat_activation_sound = ini.section("AudioVisual").and_then(|section| {
            sounds.read_rules_reference(section, "SpySatActivationSound", None)
        });
        self.general.spy_sat_deactivation_sound = ini.section("AudioVisual").and_then(|section| {
            sounds.read_rules_reference(section, "SpySatDeactivationSound", None)
        });
        self.general.voice_ifv_repair = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "VoiceIFVRepair", None));
        // Original ReadAudioVisual order: Start6696C0, End66970A, Add669B25.
        // The processed section owns pass history; the fixed catalog owns IDs.
        self.general.start_planning_mode_sound = ini.section("AudioVisual").and_then(|section| {
            sounds.read_rules_reference(section, "StartPlanningModeSound", None)
        });
        self.general.end_planning_mode_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "EndPlanningModeSound", None));
        self.general.add_planning_mode_command_sound =
            ini.section("AudioVisual").and_then(|section| {
                sounds.read_rules_reference(section, "AddPlanningModeCommandSound", None)
            });
        // Original669BA8, after Add669B25 in the same AudioVisual pass.
        self.general.place_beacon_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "PlaceBeaconSound", None));
        self.general.time_warp_out_sound = ini.section("AudioVisual").and_then(|section| {
            sounds.read_rules_reference(section, "LetsDoTheTimeWarpOutAgain", None)
        });
        self.general.time_warp_in_sound = ini.section("AudioVisual").and_then(|section| {
            sounds.read_rules_reference(section, "LetsDoTheTimeWarpInAgain", None)
        });
        self.general.cheer_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "CheerSound", None));
        self.general.scatter_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "ScatterSound", None));
        self.general.scold_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "ScoldSound", None));
        // RulesClass::ReadAudioVisual66AD24..66AD56: ReadString128 then
        // VocFind7514D0; unknown later passes retain the previous resolved ID.
        self.general.generic_click_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "GenericClick", None));
        // Original RulesClass::ReadAudioVisual669CE2..669D68 resolves
        // +228/+22C through ReadString128/VocFind7514D0, independently of
        // ReadGeneral. Reuse the fixed-catalog/pass-history owner, including
        // invalid later reads retaining their previous IDs (ctor66596A/70=-1).
        self.general.upgrade_veteran_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "UpgradeVeteranSound", None));
        self.general.upgrade_elite_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "UpgradeEliteSound", None));
        // WeaponTypeClass::ReadINI @ `0x00772080` calls
        // CCINIClass::ReadSoundList @ `0x00525430` for Report/DownReport at
        // `0x00772394`/`0x0077241A`; use its shared catalog/list owner.
        for weapon in &mut self.weapon_types {
            let section = ini.section(&weapon.id);
            weapon.report = section.map_or_else(Vec::new, |section| {
                sounds.read_rules_sound_list(section, "Report")
            });
            weapon.down_report = section.map_or_else(Vec::new, |section| {
                sounds.read_rules_sound_list(section, "DownReport")
            });
            // WeaponTypeExt LoadFromINIFile10055520: NullableIdx<Voc>
            // 1000E9D0 reads of the custom Ivan bomb's two cues.
            if crate::rules::ares_switch::ares_way() {
                weapon.ares_ivan_bomb.ticking_sound = section.and_then(|section| {
                    sounds.read_ares_reference(section, "IvanBomb.TickingSound")
                });
                weapon.ares_ivan_bomb.attach_sound = section.and_then(|section| {
                    sounds.read_ares_reference(section, "IvanBomb.AttachSound")
                });
            }
        }
        for sw in self.super_weapons.values_mut() {
            let section = ini.section(&sw.id);
            if crate::rules::ares_switch::ares_way() {
                // SWTypeExt::LoadFromINI10033522 -> NullableIdx<Voc>
                // 1000E9D0: successful names/none set presence; unknown
                // retains. Read against fixed SOUNDMD, in rules-pass order.
                sw.presentation.sound =
                    section.and_then(|section| sounds.read_ares_reference(section, "SW.Sound"));
                // 10033538: SW.ActivationSound, the same NullableIdx<Voc> read.
                sw.presentation.activation_sound = section
                    .and_then(|section| sounds.read_ares_reference(section, "SW.ActivationSound"));
            }
            // Original6CEB74..6CEBD8 keeps each current Voc ID when
            // ReadString128 or FindIndex7514D0 finds no replacement.
            sw.special_sound = section
                .and_then(|section| sounds.read_rules_reference(section, "SpecialSound", None));
            sw.start_sound = section
                .and_then(|section| sounds.read_rules_reference(section, "StartSound", None));
            if crate::rules::ares_switch::ares_way()
                && let Some(delivery) = sw.unit_delivery.as_mut()
            {
                // Ares NullableIdx<VocClass>: unknown retains the previous
                // resolved cue; an explicit none sentinel clears it. The
                // retained type INI owns the ordered read history.
                delivery.activation_sound = section
                    .and_then(|section| sounds.read_ares_reference(section, "SW.ActivationSound"))
                    .flatten();
            }
        }
        // WarheadTypeExt ReadINI10051D80: DieSound.Override/VoiceDie.Override
        // are NullableIdx<Voc> 1000E9D0 reads against the fixed SOUNDMD.
        for warhead in &mut self.warhead_types {
            let section = ini.section(&warhead.id);
            if let Some(ares) = warhead.ares_rules_mut() {
                ares.combat_mut().bind_sound_overrides(section, sounds);
            }
        }
        let type_array_indices = &self.type_array_indices;
        for object in &mut self.object_list {
            let section = match source {
                SoundReferenceSource::Processed(processed) => type_array_indices
                    .get(object.category, &object.id)
                    .and_then(|index| processed.techno_type_read_section(object.category, index)),
                #[cfg(any(test, feature = "test-support"))]
                SoundReferenceSource::Fixture(ini) => ini.section(&object.id),
            };
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
                    .and_then(|section| sounds.read_rules_reference(section, "BuildupSound", None));
                // BuildingType46088F/4608DD read WorkingSound (+0xE80) and
                // NotWorkingSound (+0xE84) the same way.
                object.working_sound = section
                    .and_then(|section| sounds.read_rules_reference(section, "WorkingSound", None));
                object.not_working_sound = section.and_then(|section| {
                    sounds.read_rules_reference(section, "NotWorkingSound", None)
                });
            }
            object.sinking_sound = section
                .and_then(|section| sounds.read_rules_reference(section, "SinkingSound", None));
            object.voice_sinking = section
                .and_then(|section| sounds.read_rules_reference(section, "VoiceSinking", None));
            // TechnoType712FEC reads VoiceFalling (+0x54C) the same way.
            object.voice_falling = section
                .and_then(|section| sounds.read_rules_reference(section, "VoiceFalling", None));
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
            object.turret_rotate_sound = section.and_then(|section| {
                sounds.read_rules_reference(section, "TurretRotateSound", None)
            });
            // TechnoType7138D1/71390D read ActivateSound (+0x5A8) and
            // DeactivateSound (+0x5AC) the same way, for every techno type.
            object.activate_sound = section
                .and_then(|section| sounds.read_rules_reference(section, "ActivateSound", None));
            object.deactivate_sound = section
                .and_then(|section| sounds.read_rules_reference(section, "DeactivateSound", None));
            // ObjectType5F93C1 reads AmbientSound (+0x1F4) the same way, for
            // every object type.
            object.ambient_sound = section
                .and_then(|section| sounds.read_rules_reference(section, "AmbientSound", None));
            if object.category == crate::rules::object_type::ObjectCategory::Infantry {
                // Infantry52440B then524447: missing/empty/unregistered names
                // keep the prior signed ID across the existing processed passes.
                object.enter_water_sound = section.and_then(|section| {
                    sounds.read_rules_reference(section, "EnterWaterSound", None)
                });
                object.leave_water_sound = section.and_then(|section| {
                    sounds.read_rules_reference(section, "LeaveWaterSound", None)
                });
                // ReadSequenceData (`0x0052403F`) resolves each pair through
                // VocClass::FindByName (`0x007514D0`, first _strcmpi match).
                object.bind_sequence_sounds(sounds);
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
    /// Only Rules read the Ares way (`ares_switch::ares_way`) have them.
    pub fn bind_type_eva_references(
        &mut self,
        ini: &IniFile,
        eva: &crate::rules::sound_ini::EvaRegistry,
    ) {
        if crate::rules::ares_switch::ares_way() {
            for sw in self.super_weapons.values_mut() {
                // LoadFromINI10033370 reads these ValueableIdx<VoxClass> in this
                // order after Ready/Activated/Detected; neither has a handler
                // Initialize write. Ctor10031460: Impatient -1, SelectTarget
                // and InsufficientFunds FindIndex of their literal names.
                sw.impatient_eva =
                    eva.read_rules_reference(ini.section(&sw.id), "EVA.Impatient", "");
                sw.insufficient_funds_eva = eva.read_rules_reference(
                    ini.section(&sw.id),
                    "EVA.InsufficientFunds",
                    "EVA_InsufficientFunds",
                );
                sw.select_target_eva = eva.read_rules_reference(
                    ini.section(&sw.id),
                    "EVA.SelectTarget",
                    "EVA_SelectTarget",
                );
            }
            for object in &mut self.object_list {
                // TypeExt LoadFromINI1003DFB0 reads both ValueableIdx<VoxClass>
                // after the promotion sounds; Initialize1003DEC0 seeds each with
                // FindIndex("EVA_UnitPromoted").
                let section = ini.section(&object.id);
                object.bind_promotion_eva([
                    eva.read_rules_reference(section, "EVA.VeteranPromoted", "EVA_UnitPromoted"),
                    eva.read_rules_reference(section, "EVA.ElitePromoted", "EVA_UnitPromoted"),
                ]);
                // EVA.Lost, read before those; Initialize1003DEC0 seeds
                // FindIndex("EVA_UnitLost") for every type but a building.
                let building =
                    object.category == crate::rules::object_type::ObjectCategory::Building;
                object.bind_unit_lost_eva(eva.read_rules_reference(
                    section,
                    "EVA.Lost",
                    if building { "" } else { "EVA_UnitLost" },
                ));
                // BuildingTypeExt LoadFromINIFile 1000DD40 reads LostEvaEvent;
                // the Initialize of 1000DCF0 seeds FindIndex("EVA_TechBuildingLost").
                if object.category == crate::rules::object_type::ObjectCategory::Building {
                    object.bind_lost_eva(eva.read_rules_reference(
                        section,
                        "LostEvaEvent",
                        "EVA_TechBuildingLost",
                    ));
                }
            }
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
        // One for each Rules-reading mode (`ares_switch::ares_way`).
        static EMPTY: [std::sync::OnceLock<RuleSet>; 2] =
            [std::sync::OnceLock::new(), std::sync::OnceLock::new()];
        EMPTY[usize::from(crate::rules::ares_switch::ares_way())].get_or_init(|| {
            RuleSet::from_ini(&IniFile::from_str("")).expect("an empty rules INI reads")
        })
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn from_ini_with_fixed_art_and_sounds_for_test(
        ini: &IniFile,
        art: &IniFile,
        sound_names: &[&str],
    ) -> Result<Self, RulesError> {
        let mut catalog = String::from("[SoundList]\n");
        for (index, name) in sound_names.iter().enumerate() {
            catalog.push_str(&format!("{index}={name}\n"));
        }
        let mut owner =
            crate::rules::process_owner::NativeRulesProcessOwner::from_cold_start_sources(
                ini.clone(),
                None,
                art.clone(),
                std::sync::Arc::new(crate::rules::sound_ini::SoundRegistry::from_ini(
                    &IniFile::from_str(&catalog),
                )),
                std::sync::Arc::default(),
            )?;
        owner
            .load_scenario(
                crate::rules::process_owner::NativeScenarioRulesPrefix::Campaign(None),
                &IniFile::empty(),
            )
            .map(|loaded| loaded.into_parts().0)
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
        Self::from_ini_with_fixed_art_and_sounds_for_test(ini, &IniFile::empty(), sound_names)
    }

    fn from_projected_ini(
        processed: &ProcessedRulesLayers,
        country_side_registry: ProjectedCountrySideRegistry,
    ) -> Result<Self, RulesError> {
        let ini = processed.ini();
        let mut object_list: Vec<ObjectType> = Vec::new();
        let mut object_index: LookupMap<String, TypeHandle> = LookupMap::default();
        let mut object_category_index: CategoryIndex<TypeHandle> = CategoryIndex::default();
        let mut type_array_indices: CategoryIndex<i32> = CategoryIndex::default();
        let mut infantry_ids: Vec<String> = Vec::new();
        let mut vehicle_ids: Vec<String> = Vec::new();
        let mut aircraft_ids: Vec<String> = Vec::new();
        let mut building_ids: Vec<String> = Vec::new();
        let production: ProductionRules = ProductionRules::from_ini(ini);
        let mut general: GeneralRules = GeneralRules::from_ini(ini);
        (general.drain_money_frame_delay, general.drain_money_amount) =
            processed.drain_money_controls();
        general.attack_cursor_on_disguise = processed.attack_cursor_on_disguise();
        general.allow_shrouded_subterannean_moves = processed.allow_shrouded_subterannean_moves();
        general.tree_strength = processed.tree_strength();
        general.threat_coefficient_defaults = processed.threat_coefficient_defaults();
        general.bomb_ticking_sound = processed.bomb_ticking_sound().map(str::to_owned);
        general.bomb_attach_sound = processed.bomb_attach_sound().map(str::to_owned);
        general.psychic_reveal_activate_sound =
            processed.psychic_reveal_activate_sound().map(str::to_owned);
        let [gui_build, gui_tab, building_slam] = processed.ui_sounds();
        general.gui_build_sound = gui_build.map(str::to_owned);
        general.gui_tab_sound = gui_tab.map(str::to_owned);
        general.building_slam = building_slam.map(str::to_owned);
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
        let terrain_rules: TerrainRules = processed.terrain_rules().clone();
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
        let insignia_types = processed.insignia_types().cloned().unwrap_or_default();
        #[cfg(feature = "phobos")]
        let phobos_draw_globals = crate::rules::insignia_type::PhobosDrawGlobals::from_ini(ini);
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
        let mut color_schemes = crate::rules::color_scheme::parse_color_schemes(ini);
        // Product extension: the lobby's colours beyond YR's eight. The mode
        // appends the built-in ones `[Colors]` leaves out, so the ramps below
        // and every other scheme consumer hold them.
        let skirmish_color_mode =
            crate::rules::skirmish_colors::SkirmishColorMode::from_ini(ini, &mut color_schemes);
        let house_color_ramps =
            crate::rules::house_colors::HouseColorRamps::from_schemes(&color_schemes);
        let color_add = processed.color_add_state().0.clone();

        // Step 1: Parse each type registry and load object sections.
        for &(registry_name, category) in TYPE_REGISTRIES {
            let ids: Vec<String> = parse_registry(ini, registry_name);
            log::info!("Registry [{}]: {} entries", registry_name, ids.len());

            for (registry_index, id) in ids.iter().enumerate() {
                let array_index =
                    type_array_indices.get_or_insert(category, id, registry_index as i32);
                if let Some(section) = ini.section(id) {
                    let mut obj: ObjectType = ObjectType::from_ini_section(id, section, category);
                    if category == ObjectCategory::Building {
                        obj.base_plan_type_index = array_index;
                    }
                    if obj.base_reservation_writer_eligible() {
                        obj.base_reservation_spacing = Some(ai_base_spacing);
                    }
                    let key = id.to_ascii_uppercase();
                    // Find-or-allocate is per native type registry. A duplicate
                    // within one registry reuses its slot; the same ID in a
                    // different registry remains a distinct native type.
                    let handle = match object_category_index.get(category, id) {
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
                            object_category_index.insert(category, id, handle);
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
        let mut weapons: LookupMap<String, WeaponHandle> = LookupMap::default();
        let mut weapon_types: Vec<WeaponType> = Vec::new();
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
            if crate::rules::ares_switch::ares_way() {
                weapon.ares_visuals.bolt_particle_system = state.ares_bolt_particle_system.clone();
                weapon.ares_ivan_bomb.warhead = state.ares_ivan_bomb_warhead.clone();
                weapon.ares_abductor.anim = state.ares_abductor_anim.map(str::to_owned);
                weapon.ares_cursors = state.ares_cursors;
            }
            // A repeated ID replaces the earlier weapon under its key. The
            // reserve is `insert`'s, so the keys iterate in the order they
            // did when the map held the weapons themselves.
            weapons.reserve(1);
            match weapons.entry(section.name.clone()) {
                std::collections::hash_map::Entry::Occupied(slot) => {
                    weapon_types[slot.get().0 as usize] = weapon;
                }
                std::collections::hash_map::Entry::Vacant(slot) => {
                    slot.insert(WeaponHandle(weapon_types.len() as u32));
                    weapon_types.push(weapon);
                }
            }
        }
        // Step 3: Parse warhead sections. The projected [Warheads] registry is
        // the processor's whole Warhead family: the [Warheads] values and
        // every reference-driven allocation, weapons' warheads and the
        // [Radiation] RadSiteWarhead included.
        let warhead_ids: HashSet<String> = parse_registry(ini, "Warheads").into_iter().collect();
        let mut warheads: LookupMap<String, WarheadHandle> = LookupMap::default();
        let mut warhead_types: Vec<WarheadType> = Vec::new();
        for warhead_id in &warhead_ids {
            if let Some(section) = ini.section(warhead_id) {
                // Find-or-allocate by the section's canonical header name, so
                // two references differing only in case resolve to one type
                // (the original engine matches case-insensitively).
                let canonical = section.name.clone();
                warheads.entry(canonical.clone()).or_insert_with(|| {
                    warhead_types.push(WarheadType::from_ini_section(&canonical, section));
                    WarheadHandle(warhead_types.len() as u32 - 1)
                });
            } else {
                log::trace!("Warhead '{}' referenced but has no section", warhead_id);
            }
        }

        // Step 4: Collect projectile IDs referenced by weapons and parse them.
        let mut projectiles: LookupMap<String, ProjectileHandle> = LookupMap::default();
        let mut projectile_types: Vec<ProjectileType> = Vec::new();
        let mut projectile_ids: HashSet<String> = HashSet::new();
        for weapon in &weapon_types {
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
                projectiles.entry(canonical.clone()).or_insert_with(|| {
                    projectile_types
                        .push(ProjectileType::from_ini_section(&canonical, section, None));
                    ProjectileHandle(projectile_types.len() as u32 - 1)
                });
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

        let mut mind_control = crate::rules::mind_control_rules::MindControlRules::from_ini(
            ini,
            processed.mind_control_attack_line_frames(),
        );
        let [capture, release, overload] = processed.mind_control_sounds();
        mind_control.mind_control_sound = capture.map(str::to_owned);
        mind_control.mind_cleared_sound = release.map(str::to_owned);
        mind_control.overload_sound = overload.map(str::to_owned);

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

        // Preserve actual allocations and per-pass bindings, including
        // constructor-only particles reached through HoldsWhat/NextParticle.
        let particle_types = processed.particle_type_projections().collect::<Vec<_>>();
        let (particle_system_types, particle_system_types_by_name) =
            parse_particle_system_types(processed);
        let (voxel_anim_types, voxel_anim_types_by_name) = parse_voxel_anim_types(processed);

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

        // Product extension: commander personalities ([AIPersonalities]). The
        // registry walk stays here so the stored-ID projection has one owner;
        // without the section the catalog is empty and nothing else changes.
        //
        // `Country=` is the only filter a personality is written with, at
        // `rules.ini`'s `Owner=` granularity: exact `[Countries]` keys, matched
        // against the house's own country name. Nothing here needs the country
        // or side tables, which is what lets a mod's own country take part
        // without registering anything.
        let personalities = crate::rules::personality::parse_personalities(
            ini,
            &parse_registry(ini, "AIPersonalities"),
        );

        let mut rules = RuleSet {
            object_list,
            object_index,
            serial: next_rule_set_serial(),
            object_category_index,
            native_techno_type_order: Vec::new(),
            type_array_indices,
            weapons,
            weapon_types,
            weapon_array: Vec::new(),
            warheads,
            warhead_types,
            projectiles,
            projectile_types,
            countries,
            country_ids: country_side_registry.country_ids,
            country_indices: country_side_registry.country_indices,
            side_ids: country_side_registry.side_ids,
            personalities,
            commander_feedback: crate::rules::personality::parse_commander_feedback(ini),
            end_game_screens: crate::rules::end_game::parse_end_game_screens(
                ini,
                &parse_registry(ini, "EndGameScreens"),
            ),
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
            // Decided by `from_processed_rules` once its readers ran.
            ares: false,
            phobos: false,
            reads_ares_way: false,
            ares_global_controls: processed.ares_global_controls().clone(),
            ares_mouse_cursors: processed.ares_mouse_cursors().clone(),
            ares_super_weapon_cursors: processed
                .ares_super_weapon_cursor_states()
                .map(|(name, cursors)| (name.to_ascii_uppercase(), cursors))
                .collect(),
            ares_super_weapon_metadata: BTreeMap::new(),
            #[cfg(feature = "phobos")]
            phobos_super_weapon_metadata: BTreeMap::new(),
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
            insignia_types,
            #[cfg(feature = "phobos")]
            phobos_draw_globals,
            #[cfg(feature = "phobos")]
            shield_globals,
            #[cfg(feature = "phobos")]
            crit_globals: crate::rules::phobos_crit::CritGlobals::from_ini(ini),
            #[cfg(feature = "phobos")]
            invulnerability_globals:
                crate::rules::invulnerability_rules::InvulnerabilityGlobals::from_ini(ini),
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
            voxel_model_plan: processed.voxel_model_plan().clone(),
            buildup_assets: crate::rules::buildup_asset_catalog::BuildupAssetCatalog::default(),
            rubble_assets: crate::rules::building_rubble::RubbleAssetCatalog::default(),
            terrain_assets: crate::rules::terrain_asset_catalog::TerrainAssetCatalog::default(),
            alpha_images: crate::rules::alpha_image_assets::AlphaImageCatalog::default(),
            animation_sequences: BTreeMap::new(),
            animation_sequence_by_handle: Vec::new(),
            unit_shp_read_states: BTreeMap::new(),
            mission_control,
            // The boundary-sensitive RulesLayerStack hash.
            source_ini_hash: processed.content_hash(),
        };
        rules.rebuild_animation_sequences();
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

    /// Test builds only: how many times this thread has looked a type's
    /// record up by name ([`Self::object`]).
    #[cfg(any(test, feature = "test-support"))]
    pub fn type_name_lookups() -> u64 {
        TYPE_NAME_LOOKUPS.with(std::cell::Cell::get)
    }

    /// Test builds only: how many uppercase copies of a type name this
    /// thread has made to look one up ([`upper_name_key`]).
    #[cfg(any(test, feature = "test-support"))]
    pub fn name_key_copies() -> u64 {
        NAME_KEY_COPIES.with(std::cell::Cell::get)
    }

    /// Resolve a type name to its handle, case-insensitively (engine parity).
    pub fn type_handle(&self, id: &str) -> Option<TypeHandle> {
        self.object_index.get(&*upper_name_key(id)).copied()
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

    /// Ares 3.0 TechnoTypeClass::Find (Ares.dll 10007EA0): unless the name is
    /// `none` or `<none>` (CCINIClass::IsBlank 10003950), the first type of
    /// the same TechnoType array whose ID matches ignoring ASCII case
    /// (`strcmpi`). An unrepresentable first match answers None, as above.
    pub(crate) fn ares_find_techno_type(&self, id: &str) -> Option<&ObjectType> {
        if crate::rules::ini_parser::is_native_none_type_name(id) {
            return None;
        }
        self.native_techno_type_order
            .iter()
            .find(|(_, stored_id, _)| stored_id.eq_ignore_ascii_case(id))
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
        self.resolve_handles();
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
        #[cfg(any(test, feature = "test-support"))]
        TYPE_NAME_LOOKUPS.with(|count| count.set(count.get() + 1));
        self.type_handle(id).map(|h| self.object_by_handle(h))
    }

    /// Cameo10034E75/10034F3F and SPECIAL_PLACE10038EB9 use the same
    /// retained flag. Expose the fact without exporting mutable SWExt state.
    pub fn super_weapon_uses_ai_targeting(&self, id: &str) -> bool {
        if self.ares {
            self.super_weapon_target_policy(id)
                .is_some_and(|policy| policy.use_ai_targeting())
        } else {
            false
        }
    }

    /// The ordered SWTypeExt targeting fields of an Ares GenericWarhead or
    /// UnitDelivery type. Stock types keep their retail dispatch owners.
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

    /// `Type == Rules+0x498` (`[General] PrismType=`, `0x0044B2F8`). Ares
    /// asks the type's `PrismForwarding=` instead (`rules::ares_prism_forwarding`).
    pub(crate) fn is_prism_type(&self, object: &ObjectType) -> bool {
        self.general
            .prism_type
            .as_deref()
            .is_some_and(|prism_type| object.id.eq_ignore_ascii_case(prism_type))
    }

    /// The RocketStruct a Rocket owner of this type flies by and whether its
    /// Type is the CMisl type ([`MissileSpawnRules::rocket_block`]). A game
    /// Ares plays in takes a `Missile.Custom` AircraftType's own block first:
    /// hooks 0x6622E0 (Ares.dll 100610F0), 0x66305A (10060FA0), 0x6632F2
    /// (100610C0) and 0x6634F6 (10061090). That block's Type is the type
    /// itself, which Process's Boomer arms compare with CMislType.
    ///
    /// [`MissileSpawnRules::rocket_block`]: crate::rules::missile_spawn::MissileSpawnRules::rocket_block
    pub fn rocket_block(
        &self,
        type_name: &str,
    ) -> (&crate::rules::missile_spawn::MissileSpawnParams, bool) {
        if self.ares
            && let Some(block) = self
                .ares_missile(type_name)
                .and_then(|missile| missile.custom_block())
        {
            let cmisl = block
                .type_name
                .eq_ignore_ascii_case(&self.missile_spawn.cmisl.type_name);
            return (block, cmisl);
        }
        self.missile_spawn.rocket_block(type_name)
    }

    /// The puffs and impact weapon of a Rocket owner of this type. Ares
    /// reads them for every AircraftType (`Missile.TakeOffAnim`,
    /// `Missile.TrailerAnim`, `Missile.TrailerSeparation`, hooks 0x66238A,
    /// 0x662512, 0x6627E5 and 0x662D85; the weapons only for
    /// `Missile.Custom`, 0x663218).
    pub fn rocket_type_ext(
        &self,
        type_name: &str,
    ) -> crate::rules::missile_spawn::RocketTypeExt<'_> {
        if self.ares
            && let Some(missile) = self.ares_missile(type_name)
        {
            return crate::rules::missile_spawn::RocketTypeExt {
                takeoff_anim: missile.takeoff_anim(),
                trailer_anim: missile.trailer_anim(),
                trailer_separation: missile.trailer_separation(),
                weapons: [missile.weapon(false), missile.weapon(true)],
            };
        }
        crate::rules::missile_spawn::RocketTypeExt::STOCK
    }

    /// Frames a launched missile node of a `Spawns=` pool of this type waits
    /// before it expires its child, or `None` when the pool's nodes are not
    /// missiles. The node flag is set for the three Rules families
    /// (`0x006B6D60`, `0x006B78F8`) and the wait reads their frames
    /// (`0x006B7A72`); Ares adds a `Missile.Custom` type with its own
    /// PauseFrames + TiltFrames (SpawnManagerClass_CTOR_CustomMissile
    /// 10061320, Update_CustomMissile 100613A0, Update_CustomMissile2
    /// 100613C0).
    pub(crate) fn spawn_missile_wait_frames(&self, type_name: &str) -> Option<u32> {
        if self.ares
            && let Some(block) = self
                .ares_missile(type_name)
                .and_then(|missile| missile.custom_block())
        {
            return Some(block.tilt_frames.wrapping_add(block.pause_frames).max(0) as u32);
        }
        self.missile_spawn
            .family_of(type_name)
            .map(|family| self.missile_spawn.kamikaze_wait_frames(family))
    }

    fn ares_missile(
        &self,
        type_name: &str,
    ) -> Option<&crate::rules::ares_techno_type::CustomMissileRules> {
        self.object_in_category(ObjectCategory::Aircraft, type_name)?
            .techno_combat()
            .missile()
    }

    /// Resolve one name within a specific native TechnoType registry.
    /// Category plus case-insensitive ID is the stable analogue of the
    /// category-distinct pointer retained by native TaskForce entries.
    pub(crate) fn object_in_category(
        &self,
        category: ObjectCategory,
        id: &str,
    ) -> Option<&ObjectType> {
        self.type_handle_in_category(category, id)
            .map(|handle| self.object_by_handle(handle))
    }

    /// The handle of the object [`Self::object_in_category`] answers.
    pub(crate) fn type_handle_in_category(
        &self,
        category: ObjectCategory,
        id: &str,
    ) -> Option<TypeHandle> {
        self.object_category_index.get(category, id)
    }

    /// The computer's base defense candidates of a House's side
    /// (`HouseTypeClass+0xBC`): Allied for side 0, Soviet for 1, Third for any
    /// other (`0x00507BCA..0x00507BFA`).
    pub(crate) fn base_defense_types(&self, side_index: i32) -> &[String] {
        if self.ares
            && let Some(types) = self.side_base_defenses(side_index)
        {
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
        self.type_array_indices.get(category, id)
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
        let key = upper_name_key(id);
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
        let key = upper_name_key(id);
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
        if self.ares {
            house.map_or(cost, |house| {
                house.ares_adjust(
                    cost,
                    object.factor_slot(),
                    object.ares_production().factory_plant_multiplier(),
                )
            })
        } else {
            house.map_or(cost, |house| house.adjust(cost, object.factor_slot()))
        }
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
    /// Short names use a stack key instead of allocating a temporary string.
    pub fn terrain_object_type_case_insensitive(&self, name: &str) -> Option<&TerrainObjectType> {
        crate::util::native_string::with_ascii_uppercase(name, |key| {
            self.terrain_object_types.get(key)
        })
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
        self.weapon_handle(id)
            .map(|handle| self.weapon_by_handle(handle))
    }

    /// The handle of the weapon [`Self::weapon`] answers for `id`.
    pub fn weapon_handle(&self, id: &str) -> Option<WeaponHandle> {
        Self::lookup_ci(&self.weapons, id).copied()
    }

    /// Dereference a handle this set resolved.
    #[inline]
    fn weapon_by_handle(&self, handle: WeaponHandle) -> &WeaponType {
        &self.weapon_types[handle.0 as usize]
    }

    /// [`Self::weapon`] of `id`, through `resolved` when this set resolved
    /// it from `id`; debug builds compare the handle with the name's.
    #[inline]
    pub(crate) fn resolved_weapon(
        &self,
        id: &str,
        resolved: ResolvedWeapon,
    ) -> Option<&WeaponType> {
        if resolved.rules_serial != self.serial {
            return self.weapon(id);
        }
        debug_assert!(
            !crate::util::debug_checks::live_read_check_enabled()
                || resolved.handle == self.weapon_handle(id),
            "weapon {id:?} resolved to {:?}, unlike its name",
            resolved.handle
        );
        resolved.handle.map(|handle| self.weapon_by_handle(handle))
    }

    /// Resolve every type's weapon records against this set's weapons
    /// ([`TypeWeapons`]) and every weapon's warhead and projectile
    /// ([`WeaponParts`]). Runs when the set is built, after the records'
    /// last writer (`RulesTypeReferences::apply_to` sets the occupy
    /// weapons), and when a copy takes its own serial.
    fn resolve_handles(&mut self) {
        let resolve = |id: Option<&str>| id.and_then(|id| self.weapon_handle(id));
        let resolved: Vec<TypeWeapons> = self
            .object_list
            .iter()
            .map(|object| TypeWeapons {
                rules_serial: self.serial,
                base: object
                    .weapon_list
                    .iter()
                    .map(|slot| resolve(slot.as_deref()))
                    .collect(),
                elite: object
                    .elite_weapon_list
                    .iter()
                    .map(|slot| resolve(slot.as_deref()))
                    .collect(),
                occupy: [
                    resolve(object.occupy_weapon.as_deref()),
                    resolve(object.elite_occupy_weapon.as_deref()),
                ],
            })
            .collect();
        for (object, weapons) in self.object_list.iter_mut().zip(resolved) {
            object.project_resolved_weapons(weapons);
        }
        let parts: Vec<WeaponParts> = self
            .weapon_types
            .iter()
            .map(|weapon| WeaponParts {
                rules_serial: self.serial,
                warhead: weapon
                    .warhead
                    .as_deref()
                    .and_then(|id| self.warhead_handle(id)),
                projectile: weapon
                    .projectile
                    .as_deref()
                    .and_then(|id| self.projectile_handle(id)),
            })
            .collect();
        for (weapon, parts) in self.weapon_types.iter_mut().zip(parts) {
            weapon.project_parts(parts);
        }
    }

    /// Number of WeaponType global88756C allocation slots, including refused
    /// ambiguous slots. Consumers must preserve these positions for Action42.
    pub fn weapon_array_len(&self) -> usize {
        self.weapon_array.len()
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
        self.warhead_handle(id)
            .map(|handle| self.warhead_by_handle(handle))
    }

    /// The handle of the warhead [`Self::warhead`] answers for `id`.
    pub(crate) fn warhead_handle(&self, id: &str) -> Option<WarheadHandle> {
        Self::lookup_ci(&self.warheads, id).copied()
    }

    /// The warhead `handle` names in this set, which made it.
    pub(crate) fn warhead_by_handle(&self, handle: WarheadHandle) -> &WarheadType {
        &self.warhead_types[handle.0 as usize]
    }

    /// The warhead [`Self::warhead`] answers for `id`, to rewrite. Its
    /// handle stays the same, so the weapons' resolved parts stay current.
    fn warhead_mut(&mut self, id: &str) -> Option<&mut WarheadType> {
        let handle = self.warhead_handle(id)?;
        Some(&mut self.warhead_types[handle.0 as usize])
    }

    /// [`Self::warhead`] of `weapon`'s `Warhead=`, through the handle this
    /// set resolved when it holds `weapon`; debug builds compare the handle
    /// with the name's.
    #[inline]
    pub fn weapon_warhead(&self, weapon: &WeaponType) -> Option<&WarheadType> {
        let parts = weapon.parts();
        if parts.rules_serial != self.serial {
            return weapon.warhead.as_deref().and_then(|id| self.warhead(id));
        }
        debug_assert!(
            !crate::util::debug_checks::live_read_check_enabled()
                || parts.warhead
                    == weapon
                        .warhead
                        .as_deref()
                        .and_then(|id| self.warhead_handle(id)),
            "weapon {:?} resolved its warhead to {:?}, unlike its name",
            weapon.id,
            parts.warhead
        );
        parts.warhead.map(|handle| self.warhead_by_handle(handle))
    }

    /// Phobos `[InsigniaTypes]` catalog.
    #[cfg(feature = "phobos")]
    pub fn insignia_types(&self) -> &crate::rules::insignia_type::InsigniaTypeCatalog {
        &self.insignia_types
    }

    /// Phobos `[General]`/`[AudioVisual]` insignia settings.
    #[cfg(feature = "phobos")]
    pub fn phobos_draw_globals(&self) -> &crate::rules::insignia_type::PhobosDrawGlobals {
        &self.phobos_draw_globals
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

    /// Phobos Iron Curtain / Force Shield organics, tint and deploy settings.
    #[cfg(feature = "phobos")]
    pub fn invulnerability_globals(
        &self,
    ) -> &crate::rules::invulnerability_rules::InvulnerabilityGlobals {
        &self.invulnerability_globals
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

    /// The player-colour conversion this rules load selects and the colours
    /// its lobby lists.
    ///
    /// A build without the `phobos` feature reads no
    /// `[General] SkirmishUnlimitedColors=`, so callers need no `cfg`.
    pub fn skirmish_colors(&self) -> crate::rules::skirmish_colors::SkirmishColorMode {
        self.skirmish_color_mode
    }

    /// Look up a projectile by ID (case-insensitive, gamemd parity).
    pub fn projectile(&self, id: &str) -> Option<&ProjectileType> {
        self.projectile_handle(id)
            .map(|handle| self.projectile_by_handle(handle))
    }

    /// The handle of the projectile [`Self::projectile`] answers for `id`.
    pub(crate) fn projectile_handle(&self, id: &str) -> Option<ProjectileHandle> {
        Self::lookup_ci(&self.projectiles, id).copied()
    }

    /// The projectile `handle` names in this set, which made it.
    pub(crate) fn projectile_by_handle(&self, handle: ProjectileHandle) -> &ProjectileType {
        &self.projectile_types[handle.0 as usize]
    }

    /// The projectile [`Self::projectile`] answers for `id`, to rewrite. Its
    /// handle stays the same, so the weapons' resolved parts stay current.
    fn projectile_mut(&mut self, id: &str) -> Option<&mut ProjectileType> {
        let handle = self.projectile_handle(id)?;
        Some(&mut self.projectile_types[handle.0 as usize])
    }

    /// [`Self::projectile`] of `weapon`'s `Projectile=`, through the handle
    /// this set resolved when it holds `weapon`; debug builds compare the
    /// handle with the name's.
    #[inline]
    pub fn weapon_projectile(&self, weapon: &WeaponType) -> Option<&ProjectileType> {
        let parts = weapon.parts();
        if parts.rules_serial != self.serial {
            return weapon
                .projectile
                .as_deref()
                .and_then(|id| self.projectile(id));
        }
        debug_assert!(
            !crate::util::debug_checks::live_read_check_enabled()
                || parts.projectile
                    == weapon
                        .projectile
                        .as_deref()
                        .and_then(|id| self.projectile_handle(id)),
            "weapon {:?} resolved its projectile to {:?}, unlike its name",
            weapon.id,
            parts.projectile
        );
        parts
            .projectile
            .map(|handle| &self.projectile_types[handle.0 as usize])
    }

    /// Registered projectile types, including their retained animation references.
    pub fn projectiles_iter(&self) -> impl Iterator<Item = &ProjectileType> {
        self.projectiles
            .values()
            .map(|handle| &self.projectile_types[handle.0 as usize])
    }

    /// Every projectile with its ID, in the ID map's order.
    fn projectile_entries(&self) -> impl Iterator<Item = (&String, &ProjectileType)> {
        self.projectiles
            .iter()
            .map(|(id, handle)| (id, &self.projectile_types[handle.0 as usize]))
    }

    /// Deterministic hash of the processed source INI this RuleSet was built from
    /// (RULESMD, optional LANGRULE, selected mode, then the map's rules-shaped
    /// pass). Stamped into
    /// diagnostic-log/snapshot headers so playback can detect a mismatched rules set —
    /// sensitive to scalar value overrides, not just the type-registry lists.
    pub fn source_ini_hash(&self) -> u64 {
        self.source_ini_hash
    }

    /// Whether Ares plays in this game: its Rules layers or map hold an Ares
    /// key, or the game ships `ares.mix` (`rules::ares_switch`). Sim and
    /// presentation read this one answer, in every build.
    #[inline]
    pub fn ares(&self) -> bool {
        self.ares
    }

    /// Whether Phobos plays in this game: its Rules layers or map hold a key
    /// of a Phobos piece that runs in every build, or the build has the
    /// `phobos` feature (`rules::phobos_switch`). Sim and presentation read
    /// this one answer.
    #[inline]
    pub fn phobos(&self) -> bool {
        self.phobos
    }

    /// Whether this game's Rules were read the Ares way
    /// (`ares_switch::ares_way`). The game's readers after the projection (its
    /// fixed ART, the asset closures) open the same mode with
    /// `ares_switch::read_ares_way`.
    pub fn reads_ares_way(&self) -> bool {
        self.reads_ares_way
    }

    /// Whether the code that needs Ares' own Rules takes its Ares form in this
    /// game: the superweapons, production, power and money. Their Ares arms
    /// read data only Rules read the Ares way hold (the SWTypeExt metadata
    /// and handler kinds, BuildingTypeExt, the factory owners, the generic
    /// prerequisites, a House type's power plants), and a stock arm can
    /// misread such data (a handler type's absent PreDependent). So they
    /// follow how the Rules were read, not [`Self::ares`] alone. A game Ares
    /// turned on in by its keys, read the stock way, keeps the stock form of
    /// these until loading reads its Rules the Ares way.
    pub fn ares_form(&self) -> bool {
        self.reads_ares_way
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
        if self.ares
            && let Some(name) = requested_name
        {
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
    /// [`StateHasher`](crate::util::state_hasher::StateHasher).
    pub fn simulation_config_hash(&self) -> u64 {
        let mut hasher = crate::util::state_hasher::StateHasher::new();
        // v34: Country starting-veterancy; optional extensions add domains below.
        b"rules-simulation-config-v34".hash(&mut hasher);
        self.source_ini_hash.hash(&mut hasher);
        // A previous scenario's list survives RulesClass's Type reset even
        // when the incoming source text omits AIHateDelays.
        if !self.general.ai_hate_delays.is_empty() {
            b"ai-hate-delays-v1".hash(&mut hasher);
            self.general.ai_hate_delays.hash(&mut hasher);
        }
        // Identical current text can inherit different CombatDamage values.
        b"drain-money-controls-v1".hash(&mut hasher);
        self.general.drain_money_frame_delay.hash(&mut hasher);
        self.general.drain_money_amount.hash(&mut hasher);
        // The separately saved global89EA40 table can differ for identical
        // current INI text after earlier Process passes or a load.
        self.terrain_rules.hash(&mut hasher);
        // ReadAudioVisual669A60/669A99/669FC3 can retain different fixed
        // sound references for identical current INI text after a reset/load.
        b"global-sound-references-v1".hash(&mut hasher);
        self.general.bomb_ticking_sound.hash(&mut hasher);
        self.general.bomb_attach_sound.hash(&mut hasher);
        self.general.psychic_reveal_activate_sound.hash(&mut hasher);
        [
            &self.general.gui_build_sound,
            &self.general.gui_tab_sound,
            &self.general.building_slam,
        ]
        .hash(&mut hasher);
        b"mind-control-sound-references-v1".hash(&mut hasher);
        (
            &self.mind_control.mind_control_sound,
            &self.mind_control.mind_cleared_sound,
            &self.mind_control.overload_sound,
        )
            .hash(&mut hasher);
        self.object_list
            .iter()
            .map(|object| {
                (
                    (object.category, object.id.to_ascii_uppercase()),
                    &object.mind_cleared_sound,
                )
            })
            .collect::<BTreeMap<_, _>>()
            .hash(&mut hasher);
        b"infantry-sequence-bank-v1".hash(&mut hasher);
        Self::by_upper_id(self.object_list.iter().filter_map(|object| {
            object
                .infantry_sequence_bank()
                .map(|bank| (&object.id, bank))
        }))
        .hash(&mut hasher);

        b"powerups-v1".hash(&mut hasher);
        self.powerups.hash(&mut hasher);
        // ReadINI71551E..715720 can retain different weights for identical
        // current text. Include the projected bits and process-resident defaults.
        b"threat-coefficients-v1".hash(&mut hasher);
        self.general.threat_coefficient_defaults.hash(&mut hasher);
        self.object_list
            .iter()
            .map(|object| {
                (
                    (object.category, object.id.to_ascii_uppercase()),
                    object.threat_coefficients(),
                )
            })
            .collect::<BTreeMap<_, _>>()
            .hash(&mut hasher);
        b"attack-cursor-disguise-v1".hash(&mut hasher);
        self.general.attack_cursor_on_disguise.hash(&mut hasher);
        b"allow-shrouded-subterannean-moves-v1".hash(&mut hasher);
        self.general
            .allow_shrouded_subterannean_moves
            .hash(&mut hasher);
        // InfantryTypeRead52455F retains this byte across missing/invalid
        // reads; equal current source text may therefore evacuate differently.
        let mut civilian_types: Vec<_> = self
            .object_list
            .iter()
            .filter(|object| object.category == ObjectCategory::Infantry && object.civilian)
            .map(|object| object.id.to_ascii_uppercase())
            .collect();
        if !civilian_types.is_empty() {
            civilian_types.sort_unstable();
            b"infantry-civilian-v1".hash(&mut hasher);
            civilian_types.hash(&mut hasher);
        }
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
        // ObjectTypeRead5F93D5 can retain a different Voc ID for identical
        // current text. Keep concrete type identities distinct in the hash.
        b"object-crush-sounds-v1".hash(&mut hasher);
        self.object_list
            .iter()
            .map(|object| {
                (
                    (object.category, object.id.to_ascii_uppercase()),
                    &object.crush_sound,
                )
            })
            .collect::<BTreeMap<_, _>>()
            .hash(&mut hasher);
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
            .projectiles_iter()
            .filter(|projectile| projectile.degenerates)
            .map(|projectile| projectile.id.to_ascii_uppercase())
            .collect();
        if !degenerating_projectiles.is_empty() {
            degenerating_projectiles.sort();
            b"projectile-degenerates-v1".hash(&mut hasher);
            degenerating_projectiles.hash(&mut hasher);
        }
        // Per-pass gates and overlapping writes can retain different tables
        // for identical current sources.
        b"gattling-v1".hash(&mut hasher);
        self.object_list
            .iter()
            .map(|object| {
                (
                    (object.category, object.id.to_ascii_uppercase()),
                    (object.is_gattling, object.gattling_stages),
                )
            })
            .collect::<BTreeMap<_, _>>()
            .hash(&mut hasher);
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
        // Peers must list the same lobby colours and draw a priority alike.
        b"skirmish-colors-v1".hash(&mut hasher);
        self.skirmish_color_mode.hash(&mut hasher);
        // Which plant a computer house splices is simulation state.
        if self.ares {
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
        if self.ares {
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
        }
        if self.phobos {
            self.parallel_ai_queues.hash_into(&mut hasher);
        }
        if self.ares {
            b"ares-side-defaults-v1".hash(&mut hasher);
            b"ares-super-availability-v1".hash(&mut hasher);
            self.ares_super_weapon_metadata.hash(&mut hasher);
            b"ares-weapon-ammo-v1".hash(&mut hasher);
            Self::by_upper_id(
                self.weapon_entries()
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
                .weapon_entries()
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
        // the selected INIs contain no extension keys. The `false` held the
        // retired `ares` build feature and keeps a released game's hash.
        b"rules-build-compatibility-v1".hash(&mut hasher);
        false.hash(&mut hasher);
        // The game's Ares switch: two peers or a replay with the same layers
        // but another `ares.mix` answer play different games. Folded only
        // when Ares plays, so a stock game's hash stays as it was.
        if self.ares {
            b"ares-plays-v1".hash(&mut hasher);
        }
        if self.ares {
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
            // ObjectType IgnoresFirestorm=, which only the Ares firestorm
            // walls read; folded only once a type sets it.
            let ignores_firestorm = self
                .object_list
                .iter()
                .filter(|object| object.ignores_firestorm)
                .map(|object| (object.category, object.id.to_ascii_uppercase()))
                .collect::<std::collections::BTreeSet<_>>();
            if !ignores_firestorm.is_empty() {
                b"ares-ignores-firestorm-v1".hash(&mut hasher);
                ignores_firestorm.hash(&mut hasher);
            }
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
            let chronoshift = self
                .object_list
                .iter()
                .filter(|object| object.ares_chronoshift() != Default::default())
                .map(|object| {
                    (
                        (object.category, object.id.to_ascii_uppercase()),
                        object.ares_chronoshift(),
                    )
                })
                .collect::<BTreeMap<_, _>>();
            if !chronoshift.is_empty() {
                b"ares-chronoshift-types-v1".hash(&mut hasher);
                chronoshift.hash(&mut hasher);
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
            self.warhead_entries()
                .map(|(name, warhead)| (name, warhead.ares_verses_hash_state())),
        )
        .hash(&mut hasher);

        if self.ares {
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
            // TypeExt IronCurtain.Modifier/ForceShield.Modifier scale every
            // curtain TechnoClass::IronCurtain (hook 70E2B0) applies.
            let curtain_modifiers = self
                .object_list
                .iter()
                .filter(|object| object.ares_curtain_modifiers() != [NativeF64Bits::ONE; 2])
                .map(|object| {
                    (
                        (object.category, object.id.to_ascii_uppercase()),
                        object.ares_curtain_modifiers(),
                    )
                })
                .collect::<BTreeMap<_, _>>();
            if !curtain_modifiers.is_empty() {
                b"ares-curtain-modifiers-v1".hash(&mut hasher);
                curtain_modifiers.hash(&mut hasher);
            }
            let cloak_power = self
                .object_list
                .iter()
                .filter(|object| {
                    object.cloak_power()
                        != &crate::rules::ares_cloak_power::CloakPowerTypeRules::default()
                })
                .map(|object| {
                    (
                        (object.category, object.id.to_ascii_uppercase()),
                        object.cloak_power(),
                    )
                })
                .collect::<BTreeMap<_, _>>();
            if !cloak_power.is_empty() {
                b"ares-cloak-power-rules-v1".hash(&mut hasher);
                cloak_power.hash(&mut hasher);
            }
            // TypeExt BuildTime.*, FactoryPlant.Multiplier and cloning
            // decide build times, prices and clones.
            let production = self
                .object_list
                .iter()
                .filter(|object| {
                    object.ares_production()
                        != &crate::rules::ares_production::ProductionTypeRules::default()
                })
                .map(|object| {
                    (
                        (object.category, object.id.to_ascii_uppercase()),
                        object.ares_production(),
                    )
                })
                .collect::<BTreeMap<_, _>>();
            if !production.is_empty() {
                b"ares-production-rules-v1".hash(&mut hasher);
                production.hash(&mut hasher);
            }
            // TypeExt Experience.*/Promote.* and BuildingTypeExt Academy.*
            // decide who gains experience and what a promotion converts to.
            let veterancy = self
                .object_list
                .iter()
                .filter(|object| {
                    object.ares_veterancy()
                        != &crate::rules::ares_veterancy::VeterancyTypeRules::default()
                        || object.academy()
                            != &crate::rules::ares_veterancy::AcademyTypeRules::default()
                })
                .map(|object| {
                    (
                        (object.category, object.id.to_ascii_uppercase()),
                        (object.ares_veterancy(), object.academy()),
                    )
                })
                .collect::<BTreeMap<_, _>>();
            if !veterancy.is_empty() || self.ares_veterancy().veteran_flash_timer() != 0 {
                b"ares-veterancy-rules-v1".hash(&mut hasher);
                veterancy.hash(&mut hasher);
                self.ares_veterancy().hash(&mut hasher);
            }
            // BuildingTypeExt rubble, repair and garrison keys.
            let building_type_ext = self
                .object_list
                .iter()
                .filter(|object| {
                    object.building_type_ext()
                        != &crate::rules::ares_building_type::BuildingTypeExtRules::default()
                })
                .map(|object| {
                    (
                        (object.category, object.id.to_ascii_uppercase()),
                        object.building_type_ext(),
                    )
                })
                .collect::<BTreeMap<_, _>>();
            let building_globals = self.ares_building_globals();
            if !building_type_ext.is_empty()
                || building_globals
                    != &crate::rules::ares_building_type::BuildingGlobalRules::default()
            {
                b"ares-building-type-ext-rules-v1".hash(&mut hasher);
                building_type_ext.hash(&mut hasher);
                building_globals.hash(&mut hasher);
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
                self.warhead_entries()
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
        [
            self.general.extra_unit_light,
            self.general.extra_infantry_light,
            self.general.extra_aircraft_light,
        ]
        .hash(&mut hasher);
        self.general.ambient_change_rate_nonzero.hash(&mut hasher);
        self.general
            .ambient_change_interval_frames
            .hash(&mut hasher);
        self.general.ambient_change_step.hash(&mut hasher);
        self.general.prism_support.hash(&mut hasher);
        // These current-default inputs survive a process registry handoff.
        self.general.ion_cannon_damage.hash(&mut hasher);
        self.general.spotlight.hash(&mut hasher);
        if self.ares {
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
        // AircraftTypeSave41CE90/Load41CE20 retain this pair independently
        // of final source text. Native ComputeCRC41CDB0 omits it; this is
        // VERA's complete simulation configuration identity, not that CRC.
        b"aircraft-trailer-v1".hash(&mut hasher);
        Self::by_upper_id(
            self.object_list
                .iter()
                .filter(|object| object.category == ObjectCategory::Aircraft)
                .map(|object| (&object.id, object.aircraft_trailer())),
        )
        .hash(&mut hasher);
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
            self.projectile_entries()
                .map(|(id, projectile)| (id, &projectile.shrapnel_weapon)),
        )
        .hash(&mut hasher);
        let airburst_weapons =
            Self::by_upper_id(self.projectile_entries().filter_map(|(id, projectile)| {
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
            .projectile_entries()
            .filter(|(_, projectile)| projectile.airburst)
            .map(|(id, _)| id.to_ascii_uppercase())
            .collect::<std::collections::BTreeSet<_>>();
        if !airburst_flags.is_empty() {
            b"airburst-retained-flag-v1".hash(&mut hasher);
            airburst_flags.hash(&mut hasher);
        }
        // Ares BulletTypeExt keys: solidity, trenches and launch scatter.
        if self.ares {
            let bullet_type_ext = self
                .projectile_entries()
                .filter(|(_, projectile)| {
                    projectile.bullet_type_ext
                        != crate::rules::ares_bullet_type::BulletTypeExtRules::default()
                })
                .map(|(id, projectile)| (id.to_ascii_uppercase(), &projectile.bullet_type_ext))
                .collect::<std::collections::BTreeMap<_, _>>();
            if !bullet_type_ext.is_empty() {
                b"ares-bullet-type-ext-v1".hash(&mut hasher);
                bullet_type_ext.hash(&mut hasher);
            }
        }
        // Phobos airburst/splits inputs. Defaults leave the stock hash
        // unchanged; any authored value must invalidate a save.
        #[cfg(feature = "phobos")]
        {
            let phobos_projectiles =
                Self::by_upper_id(self.projectile_entries().filter_map(|(id, projectile)| {
                    let value = &projectile.phobos_airburst;
                    (value != &crate::rules::phobos_airburst::PhobosAirburstType::default()).then(
                        || {
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
                        },
                    )
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
                self.weapon_entries()
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
                self.weapon_entries()
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
            if self.ares {
                self.ares_global_controls
                    .hunter_seeker_buildings()
                    .hash(&mut hasher);
            }
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
        if self.ares {
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
        Self::by_upper_id(self.weapon_entries().map(|(id, weapon)| {
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
            self.warhead_entries()
                .map(|(id, warhead)| (id, &warhead.debris_types)),
        )
        .hash(&mut hasher);
        #[cfg(feature = "phobos")]
        {
            let jumpjet_overrides = Self::by_upper_id(
                self.warhead_entries()
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
                tiberium.image(),
            )
                .hash(&mut hasher);
        }
        // Ares TiberiumExt and the TechnoTypeExt tiberium/harvester keys keep
        // retained reads. Defaults leave the stock hash unchanged.
        if self.ares {
            let tiberium_ext = self
                .tiberium_types
                .types()
                .iter()
                .filter(|tiberium| tiberium.ares() != &Default::default())
                .map(|tiberium| (&tiberium.section, tiberium.ares()))
                .collect::<Vec<_>>();
            let techno_tiberium = self
                .object_list
                .iter()
                .filter(|object| object.techno_combat().tiberium() != &Default::default())
                .map(|object| {
                    (
                        (object.category, object.id.to_ascii_uppercase()),
                        object.techno_combat().tiberium(),
                    )
                })
                .collect::<BTreeMap<_, _>>();
            if !tiberium_ext.is_empty() || !techno_tiberium.is_empty() {
                b"ares-tiberium-rules-v1".hash(&mut hasher);
                tiberium_ext.hash(&mut hasher);
                techno_tiberium.hash(&mut hasher);
            }
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
        general.on_fire.hash(&mut hasher);
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
        if self.phobos {
            b"phobos-ai-dehardcode-v1".hash(&mut hasher);
            self.general.ai_fire_sale.hash(&mut hasher);
            self.general.ai_fire_sale_delay.hash(&mut hasher);
            self.general.ai_all_to_hunt.hash(&mut hasher);
            self.general.gather_when_mcv_deploy.hash(&mut hasher);
            b"phobos-power-surplus-v1".hash(&mut hasher);
            self.general.enable_power_surplus.hash(&mut hasher);
            self.general.power_surplus.hash(&mut hasher);
            self.general
                .power_surplus_scale_to_drain_amount
                .hash(&mut hasher);
        }
        self.combat_damage.splash_list.hash(&mut hasher);
        self.combat_damage.death_weapon.hash(&mut hasher);
        // The live family order is independent of HashMap iteration and may
        // survive earlier passes absent from the current source stack.
        self.weapon_array.hash(&mut hasher);
        Self::by_upper_id(self.warhead_entries().map(|(name, wh)| {
            (
                name,
                (
                    wh.conventional,
                    wh.em_effect,
                    wh.sparky,
                    &wh.anim_list,
                    wh.verses_f64.map(f64::to_bits),
                ),
            )
        }))
        .hash(&mut hasher);
        #[cfg(feature = "phobos")]
        {
            let warhead_anims =
                Self::by_upper_id(self.warhead_entries().filter_map(|(name, warhead)| {
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
            let control_anims =
                Self::by_upper_id(self.warhead_entries().filter_map(|(name, wh)| {
                    wh.mind_control_anim
                        .as_ref()
                        .map(|anim| (name, anim.to_ascii_uppercase()))
                }));
            if !control_anims.is_empty() {
                b"phobos-warhead-mind-control-anim-v1".hash(&mut hasher);
                control_anims.hash(&mut hasher);
            }
        }
        Self::by_upper_id(self.weapon_entries().map(|(id, weapon)| {
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
        self.voxel_model_plan.hash(&mut hasher);
        self.buildup_assets.hash(&mut hasher);
        self.rubble_assets.hash(&mut hasher);
        self.terrain_assets.hash(&mut hasher);
        self.alpha_images.hash_declared_pictures(&mut hasher);
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
        Self::by_upper_id(self.projectile_entries().map(|(id, p)| {
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
        if self.ares {
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
        self.side_extensions = projection.side_extensions;
        self.side_indices = projection.side_indices;
        self.country_sides = projection.country_sides;
        self.project_house_masks();
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
    /// game, where the shell keeps its fixed flag table.
    pub fn country_flag(&self, id: &str) -> Option<&str> {
        if self.ares {
            self.country_rules(id)
                .and_then(|country| country.presentation.flag.as_deref())
        } else {
            None
        }
    }

    /// The insignia surface a Country selection draws, as `Game_GetFlagSurface
    /// @ 0x1001DCE0` (Ares patch site `0x4E3562`, 5 bytes) answers it: the
    /// country's own `File.Flag`, or the stock `rani.pcx` branch at `0x4E3567`
    /// when that file is cleared. `None` in a stock game, where each shell
    /// keeps its fixed country table.
    pub fn country_flag_surface(&self, id: &str) -> Option<String> {
        if self.ares {
            self.country_rules(id).map(|country| {
                country
                    .presentation
                    .flag
                    .clone()
                    .unwrap_or_else(|| STOCK_UNKNOWN_INSIGNIA.to_owned())
            })
        } else {
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
        if self.ares {
            self.country_rules(id)
                .and_then(|country| country.presentation.status.as_deref())
        } else {
            None
        }
    }

    /// Ares `File.Taunt=` pattern; `~~` or the stock `%02i` receives the
    /// taunt number elsewhere. None in a stock game.
    pub fn country_taunt_pattern(&self, id: &str) -> Option<&str> {
        if self.ares {
            self.country_rules(id)
                .and_then(|country| country.presentation.taunt.as_deref())
        } else {
            None
        }
    }

    /// `File.Taunt=` for a HouseType country index (the taunt byte's field).
    pub fn country_taunt_pattern_by_index(&self, index: u16) -> Option<&str> {
        let id = self.country_name(CountryIdx(index))?;
        self.country_taunt_pattern(id)
    }

    /// Ares `ListIndex=`; negative hides the country. A stock game returns
    /// the common 100, preserving registry order.
    pub fn country_list_index(&self, id: &str) -> i32 {
        if self.ares {
            self.country_rules(id)
                .map_or(100, |country| country.presentation.list_index)
        } else {
            100
        }
    }

    /// Ares `RandomSelectionWeight=`, default 1.
    pub fn country_random_weight(&self, id: &str) -> i32 {
        if self.ares {
            self.country_rules(id)
                .map_or(1, |country| country.presentation.random_weight)
        } else {
            1
        }
    }

    /// Ares `AIRandomSelectionWeight=` once set, else the human weight
    /// (`0x1001CA11..0x1001CA1A`).
    pub fn country_ai_random_weight(&self, id: &str) -> i32 {
        if self.ares {
            self.country_rules(id)
                .map_or(1, |country| country.presentation.ai_weight())
        } else {
            self.country_random_weight(id)
        }
    }

    /// Ares `HouseTypeExt::GetPowerplants @ 0x1001BD20`, as BuildingType
    /// indices: country `id`'s `AI.PowerPlants=` (`+0x130`) while it is
    /// non-empty, else by the country's side (`HouseType+0xBC`): 0
    /// `[GDIPowerPlant]`, 1 `[NodRegularPower, NodAdvancedPower]`
    /// (`0x1001BD6C..0x1001BD8C`), 2 `[ThirdPowerPlant]`, each without its
    /// unset slots, and any other side none.
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
    /// which defers to the side layer (also the answer in a stock game).
    pub fn country_paradrop_lists(&self, id: &str) -> Option<(&[String], &[i32])> {
        if self.ares {
            self.country_rules(id)?.paradrop.lists()
        } else {
            None
        }
    }

    /// Ares `[Country] ParaDrop.Aircraft=` (`HouseTypeExt +0x154`), which
    /// `GetParadropPlane @ 0x1001BCB0` prefers over the side's plane. A
    /// never-set or `none` index is `-1` there, so this answers `None` and the
    /// caller keeps the side layer.
    pub fn country_paradrop_aircraft(&self, id: &str) -> Option<&str> {
        if self.ares {
            self.country_rules(id)?.paradrop.aircraft.as_deref()
        } else {
            None
        }
    }

    /// Ares `[Country] Parachute.Anim=` (`HouseTypeExt +0x158`), which
    /// `GetParachuteAnim @ 0x1001BBC0` prefers over the side's canopy and the
    /// rules default. The canopy hook reaches it only when the type names no
    /// parachute of its own.
    pub fn country_parachute_anim(&self, id: &str) -> Option<&str> {
        if self.ares {
            self.country_rules(id)?.paradrop.parachute_anim.as_deref()
        } else {
            None
        }
    }

    /// Ares `File.ObserverBackground=` (PCX or SHP), when retained.
    pub fn country_observer_background(&self, id: &str) -> Option<&str> {
        self.country_rules(id)?
            .presentation
            .observer_background
            .as_deref()
    }

    /// Ares `File.ObserverFlag=` (PCX or SHP), when retained.
    pub fn country_observer_flag(&self, id: &str) -> Option<&str> {
        self.country_rules(id)?
            .presentation
            .observer_flag
            .as_deref()
    }

    /// Ares `File.ObserverFlagAltPalette=`.
    pub fn country_observer_flag_alt_palette(&self, id: &str) -> bool {
        self.country_rules(id)
            .is_some_and(|country| country.presentation.observer_flag_alt_palette)
    }

    /// Ares capture1004524F..10045275 reads CountryExt+1BC when present,
    /// otherwise the live HouseType+1A6 MultiplayPassive byte.
    pub(crate) fn country_can_be_driven(&self, id: &str) -> bool {
        self.country_rules(id).is_some_and(|country| {
            country
                .veteran_types
                .can_be_driven_override()
                .unwrap_or(country.multiplay_passive)
        })
    }

    /// Ares `HouseClass_Update_DamageDelay @ 100247A0`: CountryExt+1BA
    /// `Degrades=` when set; `None` leaves the HouseType's
    /// `!MultiplayPassive` default to the caller.
    pub(crate) fn country_degrades(&self, id: &str) -> Option<bool> {
        self.country_rules(id)
            .and_then(|country| country.veteran_types.degrades_override())
    }

    /// CountryExt+1C0 `StartInMultiplayer.Types=` of the country itself
    /// (MPGameMode_SpawnStartingUnits_Types 1001F220 reads House+34's ext);
    /// `None` when unset.
    pub(crate) fn country_start_in_multiplayer_types(
        &self,
        id: &str,
    ) -> Option<&[crate::rules::superweapon_type::TechnoTypeReference]> {
        self.country_rules(id)
            .and_then(|country| country.veteran_types.start_in_multiplayer_types())
    }

    /// The country's `StartInMultiplayer.WithConst=` (CountryExt+1D0), which
    /// MPGameMode_SpawnBaseUnit_BuildConst (1001F0B0) tests.
    pub(crate) fn country_start_in_multiplayer_with_const(&self, id: &str) -> bool {
        self.country_rules(id)
            .is_some_and(|country| country.veteran_types.start_in_multiplayer_with_const())
    }

    /// Victim CountryExt+1D1 gate in Ares bounty payout10043279..10043289.
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
        crate::util::native_string::with_ascii_uppercase(id, |key| {
            self.country_indices.get(key).copied()
        })
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
            Some(name) => self.country_index_of_name(name)?,
            None => CountryIdx(0),
        };
        self.country_name(index)
    }

    /// `HouseTypeClass__FindIndexOfName @ 0x005117D0` as a registered Country.
    /// The source-order scan checks each HouseType's `Name=` alias (`+0x64`)
    /// before its registry ID (`+0x24`), ignoring case. The shared owner
    /// retains `<random>`=-2 and unknown=-1; this typed projection returns
    /// only registered Country identities.
    pub(crate) fn country_index_of_name(&self, name: &str) -> Option<CountryIdx> {
        let index = house_type_index_of_name(name, self.country_lookup_members());
        (index >= 0).then(|| {
            CountryIdx(u16::try_from(index).expect("[Countries] exceeds u16 identity space"))
        })
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
    /// `HouseTypeClass__FindIndexOfName @ 0x005117D0`
    /// ([`Self::country_index_of_name`]). Before that scan,
    /// ReadINI727292..7272AA compares literal817474 (`<none>`) and selects
    /// Country zero. This is a TriggerType caller rule, not lookup behavior.
    /// Original caller controls: tools/input_oracle/campaign_start.py
    /// --houses, side_controls.trigger_type_reader_rows (+meta).
    pub fn trigger_house_type_index(&self, owner: &str) -> Option<CountryIdx> {
        if owner.eq_ignore_ascii_case("<none>") {
            return self.country_name(CountryIdx(0)).map(|_| CountryIdx(0));
        }
        self.country_index_of_name(owner)
    }

    /// Resolve a country index back to its source spelling.
    pub fn country_name(&self, index: CountryIdx) -> Option<&str> {
        self.country_ids.get(index.0 as usize).map(String::as_str)
    }

    /// Resolve a side name to its stable `[Sides]` registration index.
    pub fn side_index(&self, id: &str) -> Option<SideIdx> {
        crate::util::native_string::with_ascii_uppercase(id, |key| {
            self.side_indices.get(key).copied()
        })
    }

    /// Commander personalities from `[AIPersonalities]`. Empty for retail data.
    pub fn personalities(&self) -> &crate::rules::personality::PersonalityCatalog {
        &self.personalities
    }

    /// `[CommanderFeedback]`: whether the talking-head layer runs at all, and
    /// how easily a house is considered to be in trouble.
    pub fn commander_feedback(&self) -> crate::rules::personality::CommanderFeedbackRules {
        self.commander_feedback.clone()
    }

    /// `[EndGameScreens]`: the per-country score-page themes and backgrounds.
    /// Empty for retail data, which is what keeps the retail page untouched.
    pub fn end_game_screens(&self) -> &crate::rules::end_game::EndGameCatalog {
        &self.end_game_screens
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
        #[cfg(feature = "phobos")]
        {
            if self.ares && self.general.building_type_selectable {
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
    /// A stock game has no SideExt configuration.
    pub fn side_sidebar_mix_file_index(&self, index: SideIdx) -> Option<i32> {
        if self.ares {
            self.side_extensions
                .get(usize::from(index.0))
                .map(|extension| extension.sidebar_mix_file_index)
        } else {
            None
        }
    }

    /// SideExt+8C selects Yuri-specific sidebar filenames independently of
    /// the Side array index and Sidebar.MixFileIndex.
    pub fn side_sidebar_yuri_file_names(&self, index: SideIdx) -> Option<bool> {
        if self.ares {
            self.side_extensions
                .get(usize::from(index.0))
                .map(|extension| extension.sidebar_yuri_file_names)
        } else {
            None
        }
    }

    /// Signed SideExt+90 after reached EVA.Tag reads. Binding uses saved
    /// Scenario.Side and preserves an out-of-catalog value; filename selection
    /// decides whether that value has a column. Native: Ares1002FB00.
    pub fn side_eva_voice_index(&self, side: i32) -> Option<i32> {
        self.side_extension(side)
            .map(|extension| extension.eva_voice_index)
    }

    fn side_extension(
        &self,
        side: i32,
    ) -> Option<&crate::rules::ares_side_extension::AresSideExtension> {
        self.side_extensions.get(usize::try_from(side).ok()?)
    }

    /// Ares Side default fallbacks use the side's array index; a fourth or
    /// later side takes the Allied branch (3.0 getters at 0x1002DE60,
    /// 0x1002DEA0, 0x1002E130, 0x1002DDD0, 0x1002DD40, 0x1002DFB0,
    /// 0x1002E060). `None` means the game has no SideExt for this index, so
    /// the caller keeps its stock path.
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
        if self.ares {
            let extension = self.side_extension(side)?;
            Some(match &extension.crew {
                Some(value) => value.as_deref(),
                None => match side {
                    1 => self.general.soviet_crew.as_deref(),
                    2 => self.general.third_crew.as_deref(),
                    _ => self.general.allied_crew.as_deref(),
                },
            })
        } else {
            None
        }
    }

    /// `[Side] Engineer=` else global `[General] Engineer=`
    /// (`SideExt::GetEngineer @ 0x1002DEE0`).
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
        if self.ares {
            let extension = self.side_extension(side)?;
            Some(match &extension.technician {
                Some(value) => value.as_deref(),
                None => self.general.technician.as_deref(),
            })
        } else {
            None
        }
    }

    /// `[Side] DefaultDisguise=` else the stock side disguise by array index
    /// (`SideExt::GetDisguise @ 0x1002DEA0`).
    pub(crate) fn side_disguise(&self, side: i32) -> Option<Option<&str>> {
        if self.ares {
            let extension = self.side_extension(side)?;
            Some(match &extension.default_disguise {
                Some(value) => value.as_deref(),
                None => {
                    self.general.infantry_disguises[Self::ares_default_side_index(side)].as_deref()
                }
            })
        } else {
            None
        }
    }

    /// `[Side] SurvivorDivisor=` else `Allied/Soviet/ThirdSurvivorDivisor`
    /// (`SideExt::GetSurvivorDivisor @ 0x1002E130`).
    pub(crate) fn side_survivor_divisor(&self, side: i32) -> Option<i32> {
        if self.ares {
            let extension = self.side_extension(side)?;
            Some(extension.survivor_divisor.unwrap_or(match side {
                1 => self.general.soviet_survivor_divisor,
                2 => self.general.third_survivor_divisor,
                _ => self.general.allied_survivor_divisor,
            }))
        } else {
            None
        }
    }

    /// `[Side] AI.BaseDefenses=` else the side-index list; a fourth or later
    /// side takes the Allied list (`SideExt::GetBaseDefenses @ 0x1002DDD0`).
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
        if self.ares {
            let extension = self.side_extension(side)?;
            Some(
                extension
                    .base_defense_counts
                    .as_deref()
                    .unwrap_or_else(|| match side {
                        1 => &self.soviet_base_defense_counts,
                        2 => &self.third_base_defense_counts,
                        _ => &self.allied_base_defense_counts,
                    }),
            )
        } else {
            None
        }
    }

    /// `[Side] ParaDrop.Types=`/`ParaDrop.Num=` only when both are set;
    /// otherwise the side-index lists, with a fourth or later side taking the
    /// Allied pair (`SideExt::GetParaDropTypes @ 0x1002E060`,
    /// `GetParaDropNum @ 0x1002DFB0`).
    pub(crate) fn side_paradrop(&self, side: i32) -> Option<(&[String], &[i32])> {
        if self.ares {
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
            Some((lists.infantry.as_slice(), lists.counts.as_slice()))
        } else {
            None
        }
    }

    /// `[Side] ParaDrop.Aircraft=` else the plane the SideExt initializer
    /// resolved from `PDPLANE` (`+0x74`). `None` for no SideExt or the -1
    /// index (`none`, or PDPLANE unregistered at initialization), where
    /// `HouseTypeExt::GetParadropPlane @ 0x1001BCB0` falls back to
    /// `FindIndex("PDPLANE")` rather than cancelling the drop.
    pub(crate) fn side_paradrop_aircraft(&self, side: i32) -> Option<&str> {
        if self.ares {
            self.side_extension(side)?
                .paradrop_aircraft
                .as_ref()?
                .as_deref()
        } else {
            None
        }
    }

    /// `[Side] Parachute.Anim=` only; an unset key leaves the existing
    /// `[General] Parachute=`/NPSE `VehicleParachute=` owner in charge.
    /// `Some(None)` is an explicit `none`, which the canopy consumer resolves
    /// to `PARACH` (`HouseTypeExt::GetParachuteAnim @ 0x1001BBC0`).
    pub(crate) fn side_parachute_override(&self, side: i32) -> Option<Option<&str>> {
        if self.ares {
            let extension = self.side_extension(side)?;
            extension
                .parachute_anim
                .as_ref()
                .map(|name| name.as_deref())
        } else {
            None
        }
    }

    /// `[Side] ToolTipColor=` as RGB (initialized 255,255,0 for Nod/ThirdSide,
    /// 164,210,255 otherwise; `SideExt+80`).
    pub fn side_tooltip_color(&self, side: i32) -> Option<[u8; 3]> {
        if self.ares {
            self.side_extension(side)
                .map(|extension| extension.tooltip_color)
        } else {
            None
        }
    }

    /// `[Side] MessageTextColor=` as the runtime ColorScheme index
    /// (initialized 11 for Nod, 25 for ThirdSide, 21 otherwise) or -1 for an
    /// explicit `none` (`SideExt+84`).
    pub fn side_message_text_color_index(&self, side: i32) -> Option<i32> {
        if self.ares {
            self.side_extension(side)
                .map(|extension| extension.message_text_color_index)
        } else {
            None
        }
    }

    /// `[Side] HunterSeeker=` (SideExt+0x94), the fallback UnitType of
    /// SW_HunterSeeker::Activate10072885.
    pub(crate) fn side_hunter_seeker(&self, side: i32) -> Option<&str> {
        self.side_extension(side)?
            .hunter_seeker
            .as_ref()?
            .as_deref()
    }

    /// Configured Type, Country and Side canopy identities for the shared
    /// animation asset closure (Ares ObjectClass hook5F5ADD/10047C60).
    pub fn parachute_animations(&self) -> impl Iterator<Item = &str> {
        self.side_extensions
            .iter()
            .filter_map(|extension| extension.parachute_anim.as_ref()?.as_deref())
            .chain(
                self.countries
                    .values()
                    .filter_map(|country| country.paradrop.parachute_anim.as_deref()),
            )
            .chain(
                self.all_objects()
                    .filter_map(|object| object.techno_combat().presentation().parachute_anim()),
            )
    }

    /// `[Side] GraphicalText.Image=` (SideExt `+0x1F8`) and
    /// `[Side] GraphicalText.Palette=` (`+0x218`).
    /// `Game_LoadUI_LoadSideData` (`0x1002F4A0`) loads each into its own
    /// tactical-overlay slot, and `TacticalClass_DrawOverlay_GraphicalText`
    /// (`0x1002FAD0`, patch site `0x6D4E79`) reads them through `0x1002DF10` /
    /// `0x1002DF00`, which fall back to InitGame's literal `GRFXTXT` files per
    /// slot. `None` when the game has no SideExt for this index.
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

    pub(crate) fn super_weapon_metadata(
        &self,
        id: &str,
    ) -> Option<&crate::rules::superweapon_type::AresSuperWeaponMetadata> {
        crate::util::native_string::with_ascii_uppercase(id, |key| {
            self.ares_super_weapon_metadata.get(key)
        })
    }

    /// The Phobos techno gates for a superweapon, if the capability is
    /// compiled in and the type was registered.
    #[cfg(feature = "phobos")]
    pub(crate) fn phobos_super_weapon_metadata(
        &self,
        id: &str,
    ) -> Option<&crate::rules::superweapon_type::PhobosSuperWeaponAvailability> {
        crate::util::native_string::with_ascii_uppercase(id, |key| {
            self.phobos_super_weapon_metadata.get(key)
        })
    }

    /// The `[GlobalControls]` parallel AI queue switches (Phobos extension).
    pub fn parallel_ai_queues(
        &self,
    ) -> &crate::rules::phobos_parallel_ai_queues::ParallelAiQueueRules {
        &self.parallel_ai_queues
    }

    /// Whether the Phobos parallel AI queue hooks act in this game: Phobos
    /// plays ([`Self::phobos`]) and Ares' `AllowParallelAIQueues=` gate is on.
    /// Each hook of that chain does nothing otherwise.
    pub fn parallel_ai_queues_act(&self) -> bool {
        self.phobos && self.parallel_ai_queues.allows_parallel()
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
    pub fn particle_system_type_id_by_index(&self, index: i32) -> Option<ParticleSystemTypeId> {
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
        crate::util::native_string::with_ascii_uppercase(name, |key| {
            self.particle_system_types_by_name.get(key).copied()
        })
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
        crate::util::native_string::with_ascii_uppercase(name, |key| {
            self.voxel_anim_types_by_name.get(key).copied()
        })
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
        crate::util::native_string::with_ascii_uppercase(structure_id, |key| {
            self.factory_map.get(key).copied()
        })
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
            .get(category, id)
            .is_some_and(|handle| self.ares_prerequisite_alternates.contains(&handle))
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
        for obj in self.object_list.iter_mut() {
            obj.project_silo_damage(art);
            // Resolve the art.ini section: use Image= override if present,
            // otherwise fall back to the object ID itself.
            let art_key: &str = &obj.image;
            let entry = art.get(art_key).or_else(|| art.get(&obj.id));
            // Every TechnoType's ReadINI reads CanBeHidden from the ART
            // section of its own ID (0x007121EB); art is not layered.
            obj.can_be_hidden = art.can_be_hidden(&obj.id);
            // Infantry Crawls and discharge frames come from the per-pass
            // ART reads (`InfantryArt`), not this final-image projection.
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
                //
                // Only slots below the contact capacity can be selected, so
                // padding stops at VERA's `DOCK_CONTACT_LIMIT`.
                if !entry.pads.is_empty() {
                    let n = obj
                        .number_of_docks
                        .clamp(0, crate::rules::object_type::DOCK_CONTACT_LIMIT)
                        as usize;
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
        self.rebuild_animation_sequences();
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
    pub fn anim_type_name_by_index(&self, index: i32) -> Option<&str> {
        self.anim_type_art_read_states
            .get(usize::try_from(index).ok()?)
            .map(|(name, _)| name.as_str())
    }

    /// Identity consumed by the shared animation/asset owners for an indexed
    /// pointer (Pickup483371, TAction6E2357, Team6EF610). Unlike the stored
    /// name, this preserves distinct slots with colliding24-byte IDs.
    pub fn anim_type_runtime_key_by_index(&self, index: i32) -> Option<std::borrow::Cow<'_, str>> {
        crate::rules::native_processing::anim_type_runtime_key(
            self.anim_type_art_read_states
                .iter()
                .map(|(name, _)| name.as_str()),
            usize::try_from(index).ok()?,
        )
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
    ) {
        self.art_registry
            .bind_scheduler_anim_assets(roots, assets, theater_ext, theater_name);
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
    ) {
        let mut roots: Vec<String> = self
            .art_registry
            .scheduler_anim_types()
            .iter()
            .cloned()
            .collect();
        roots.push(name.to_ascii_uppercase());
        roots.sort();
        roots.dedup();
        self.bind_scheduler_anim_assets(&roots, assets, theater_ext, theater_name);
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

    /// Raw synthetic fixture replacement: no native admission or field
    /// projection, except the derived `SiloDamage=` each type keeps, which
    /// follows the new ART.
    #[cfg(any(test, feature = "test-support"))]
    pub fn replace_art_registry_for_test(&mut self, art: crate::rules::art_data::ArtRegistry) {
        self.art_registry = art;
        self.voxel_assets = Default::default();
        for object in &mut self.object_list {
            object.project_silo_damage(&self.art_registry);
        }
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
        if let Some(weapon) = self.weapon_mut_for_test(id) {
            weapon.speed = speed;
        }
    }

    /// The weapon [`Self::weapon`] answers for `id`, to rewrite. Its handle
    /// stays the same, so the types' resolved records stay current.
    #[cfg(any(test, feature = "test-support"))]
    fn weapon_mut_for_test(&mut self, id: &str) -> Option<&mut WeaponType> {
        let handle = self.weapon_handle(id)?;
        Some(&mut self.weapon_types[handle.0 as usize])
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

    /// Write a type's slot-9 names (`BuildingType +0x11B0`, `+0x11C0`) as the
    /// native slot corpora do, past ReadINI's empty-damaged copy.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_turret_anim_names_for_test(&mut self, id: &str, normal: &str, damaged: &str) {
        let name = |name: &str| (!name.is_empty()).then(|| name.to_owned());
        let object = self
            .object_list
            .iter_mut()
            .find(|object| object.id == id)
            .unwrap();
        object.turret_anim = name(normal);
        object.turret_anim_damaged = name(damaged);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn art_entry_mut_for_test(
        &mut self,
        name: &str,
    ) -> Option<&mut crate::rules::art_data::ArtEntry> {
        self.voxel_assets = Default::default();
        self.art_registry.get_mut(name)
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

    pub fn loaded_voxel_model(
        &self,
        type_id: &str,
    ) -> Option<&crate::rules::voxel_asset_catalog::LoadedVoxelModel> {
        let object = self.object(type_id)?;
        self.voxel_assets.model(object.category, &object.id)
    }

    pub fn voxel_anim_model(
        &self,
        id: VoxelAnimTypeId,
    ) -> Option<&crate::rules::voxel_asset_catalog::VoxelPart> {
        self.voxel_assets.voxel_anim(id.0 as usize)
    }

    /// VERA-only, experimental (`docs/picture-model-art.md`): the declared
    /// model building `type_id` draws for its voxel guns. None when its art
    /// declares none, so the VXL path draws; `Some(None)` when the model was
    /// refused and the guns draw nothing.
    pub fn building_gun_model(
        &self,
        type_id: &str,
    ) -> Option<Option<&std::sync::Arc<crate::rules::voxel_asset_catalog::DeclaredModel>>> {
        self.voxel_assets.building_gun_model(type_id)
    }

    /// VERA-only, experimental (`docs/picture-model-art.md`): whether the
    /// voxel unit, aircraft or building gun `type_id` draws from a declared
    /// `Model=` (admitted or refused) and never from its VXL, so the unit
    /// atlas need not load it. False for retail art.
    pub fn draws_declared_model(&self, type_id: &str) -> bool {
        if self.building_gun_model(type_id).is_some() {
            return true;
        }
        self.object(type_id).is_some_and(|object| {
            object.voxel_image_load().is_some_and(|image| {
                self.art()
                    .resolve_declared_model(&object.id, image)
                    .is_some()
            })
        })
    }

    /// [`Self::building_gun_model`] for the voxel projectile type `type_id`.
    pub fn projectile_model(
        &self,
        type_id: &str,
    ) -> Option<Option<&std::sync::Arc<crate::rules::voxel_asset_catalog::DeclaredModel>>> {
        self.voxel_assets.projectile_model(type_id)
    }

    /// The declared model the VoxelAnim type `id` draws in place of its VXL:
    /// its own `Image=` art's, or the part it shares of a `ShareSource=`
    /// type's ([`crate::rules::voxel_asset_catalog::DebrisModel`]). None
    /// for the VXL path; a refused model draws nothing.
    pub fn voxel_anim_declared_model(
        &self,
        id: VoxelAnimTypeId,
    ) -> Option<&crate::rules::voxel_asset_catalog::DebrisModel> {
        self.voxel_assets.voxel_anim_declared_model(id.0 as usize)
    }

    pub(crate) fn voxel_models_bound(&self) -> bool {
        self.voxel_assets.is_bound()
    }

    /// VERA-only, experimental (`docs/picture-model-art.md`): whether any
    /// type draws an admitted `Model=` ([`crate::rules::voxel_asset_catalog::LoadedVoxelModel::declared_model`],
    /// building guns, voxel projectiles and debris included).
    /// False for retail data, so the renderer does no model work.
    pub fn declares_models(&self) -> bool {
        self.voxel_assets.declares_models()
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
            &self.art_registry,
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

    /// Ares: a building type's sale frame rate (`SellFrameDuration`, 0 while
    /// its Buildup SHP is unbound).
    pub fn buildup_sell_rate(&self, type_id: &str) -> i32 {
        self.buildup_assets.sell_rate(type_id)
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

    /// Install one building type's Ares sale rate (fixtures without assets).
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_buildup_sell_rate_for_test(&mut self, type_id: &str, rate: i32) {
        self.buildup_assets.insert_sell_rate_for_test(type_id, rate);
    }

    /// Raw SHP header count used by `TerrainClass::AI` midpoint timing.
    pub fn terrain_spawner_frame_count(&self, name: &str) -> Option<u16> {
        self.terrain_assets.frame_count(name)
    }

    /// Load every ObjectType `AlphaImage=` SHP the folded rules name
    /// ([`crate::rules::alpha_image_assets`]).
    pub fn bind_alpha_images(
        &mut self,
        overlays: &crate::rules::overlay_types::OverlayTypeRegistry,
        assets: &crate::assets::asset_manager::AssetManager,
    ) {
        self.alpha_images =
            crate::rules::alpha_image_assets::AlphaImageCatalog::bind(self, overlays, assets);
    }

    /// The loaded AlphaImage a type's `AlphaImage=` names: the type's +0xAC
    /// pointer, `None` where it stays null.
    pub fn alpha_image(
        &self,
        name: &str,
    ) -> Option<&std::sync::Arc<crate::rules::alpha_image_assets::AlphaImage>> {
        self.alpha_images.get(name)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn supply_alpha_image_for_test(
        &mut self,
        name: &str,
        image: crate::rules::alpha_image_assets::AlphaImage,
    ) {
        self.alpha_images.insert_for_test(name, image);
    }

    fn rebuild_animation_sequences(&mut self) {
        self.animation_sequences =
            crate::rules::animation_sequence::build_animation_sequence_catalog(self);
        self.index_animation_sequences();
    }

    fn index_animation_sequences(&mut self) {
        self.animation_sequence_by_handle = self
            .object_list
            .iter()
            .map(|object| {
                // Native InfantryType+E3C belongs to this concrete type, even
                // when a Building/Unit registry contains the same stored ID.
                object
                    .infantry_sequence_bank()
                    .map(|bank| bank.sequence_set())
                    .or_else(|| self.animation_sequences.get(&object.id).cloned())
            })
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
        self.animation_sequence_by_handle(self.type_handle(type_id)?)
    }

    /// Select a concrete class's retained bank, including duplicate names in
    /// another registry. Native Infantry51D707/518DCF uses that Type pointer.
    pub fn animation_sequence_in_category(
        &self,
        category: ObjectCategory,
        type_id: &str,
    ) -> Option<&crate::rules::animation_sequence::SequenceSet> {
        self.animation_sequence_by_handle(self.type_handle_in_category(category, type_id)?)
    }

    /// The sequences of the object `handle` names, as
    /// [`Self::animation_sequence`] answers for its name.
    pub fn animation_sequence_by_handle(
        &self,
        handle: TypeHandle,
    ) -> Option<&crate::rules::animation_sequence::SequenceSet> {
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
        self.warheads
            .values()
            .map(|handle| &self.warhead_types[handle.0 as usize])
    }

    /// Every warhead with its ID, in the ID map's order.
    fn warhead_entries(&self) -> impl Iterator<Item = (&String, &WarheadType)> {
        self.warheads
            .iter()
            .map(|(id, handle)| (id, &self.warhead_types[handle.0 as usize]))
    }

    /// Iterate all parsed weapon types.
    pub fn weapons_iter(&self) -> impl Iterator<Item = &WeaponType> {
        self.weapons
            .values()
            .map(|&handle| self.weapon_by_handle(handle))
    }

    /// Every weapon with its ID, in the ID map's order.
    fn weapon_entries(&self) -> impl Iterator<Item = (&String, &WeaponType)> {
        self.weapons
            .iter()
            .map(|(id, &handle)| (id, self.weapon_by_handle(handle)))
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
        for projectile in self.projectiles_iter() {
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
    country_indices: LookupMap<String, CountryIdx>,
    side_ids: Vec<String>,
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
    // Every allocated Side owns its Ares extension when the Rules were read
    // the Ares way, and none does when they were read the stock way.
    let side_extensions = sides
        .iter()
        .map(|(_, extension)| extension.cloned())
        .collect::<Option<Vec<_>>>()
        .unwrap_or_default();
    let mut side_indices = HashMap::with_capacity(side_ids.len());
    for (index, id) in side_ids.iter().enumerate() {
        side_indices
            .entry(id.to_ascii_uppercase())
            .or_insert_with(|| {
                SideIdx(u8::try_from(index).expect("[Sides] exceeds u8 identity space"))
            });
    }
    let mut country_ids = Vec::new();
    let mut country_indices = LookupMap::default();
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
            if crate::rules::ares_switch::ares_way() {
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
        side_extensions,
        side_indices,
        country_sides,
    }
}

/// `[VoxelAnims]` registry: the flying-debris types a death throws.
///
/// Registry order is the id order, exactly as for particles and particle
/// systems, because `DebrisTypes=` and `Spawns=` resolve by name and the ids
/// are hashed.
///
/// Native ReadVoxelAnims672920 allocates every listed identity, including
/// constructor-only entries; the retained processor supplies that exact order.
fn parse_voxel_anim_types(
    processed: &ProcessedRulesLayers,
) -> (Vec<VoxelAnimType>, HashMap<String, VoxelAnimTypeId>) {
    let types: Vec<_> = processed.voxel_anim_type_projections().collect();
    let mut by_name = HashMap::new();
    for (index, kind) in types.iter().enumerate() {
        by_name
            .entry(kind.name.to_ascii_uppercase())
            .or_insert(VoxelAnimTypeId(index as u32));
    }
    (types, by_name)
}

/// System allocations retain their own body and direct HoldsWhat index.
fn parse_particle_system_types(
    processed: &ProcessedRulesLayers,
) -> (
    Vec<ParticleSystemType>,
    HashMap<String, ParticleSystemTypeId>,
) {
    let types: Vec<_> = processed.particle_system_type_projections().collect();
    let mut by_name = HashMap::with_capacity(types.len());
    for (index, system) in types.iter().enumerate() {
        by_name
            .entry(system.name.to_ascii_uppercase())
            .or_insert(ParticleSystemTypeId(index as u32));
    }
    (types, by_name)
}

#[cfg(all(test, tests_rules_o_z))]
mod tests;

#[cfg(all(test, tests_rules_o_z))]
#[path = "map_lighting_tests.rs"]
mod map_lighting_tests;

#[cfg(all(test, tests_rules_o_z))]
#[path = "name_lookup_tests.rs"]
mod name_lookup_tests;

#[cfg(all(test, tests_rules_o_z))]
#[path = "global_promotion_tests.rs"]
mod global_promotion_tests;

#[cfg(all(test, tests_rules_o_z))]
#[path = "ares_script_conversion_tests.rs"]
mod ares_script_conversion_tests;
