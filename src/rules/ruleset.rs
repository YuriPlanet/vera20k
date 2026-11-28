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
