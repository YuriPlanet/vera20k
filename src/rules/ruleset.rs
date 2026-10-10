//! Master game data container loaded from rules.ini.
//!
//! RuleSet is the single source of truth for all game object definitions.
//! It parses the type registries ([InfantryTypes], [VehicleTypes], etc.),
//! then loads each referenced object's section into typed structs. Weapons
//! referenced by objects and every explicitly registered warhead are also parsed.
//!
//! ## Loading strategy
//! 1. Parse type registries → collect all object IDs per category
//! 2. For each ID, look up its [ID] section → parse into ObjectType
//! 3. Collect weapon/warhead IDs referenced by all objects
//! 4. Parse each referenced weapon and every registered/referenced warhead section
//! 5. Log summary counts
//!
//! ## Dependency rules
//! - Part of rules/ — depends on rules/ini_parser, rules/object_type,
//!   rules/weapon_type, rules/warhead_type.
//! - No dependencies on sim/, render/, ui/, etc.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::hash::{Hash, Hasher};

use crate::rules::combat_damage::CombatDamageDefaults;
use crate::rules::crate_rules::CrateRules;
use crate::rules::error::RulesError;
use crate::rules::ini_parser::{IniFile, IniSection, is_native_none_type_name};
use crate::rules::mission_data::MissionControl;
use crate::rules::native_processing::{ProcessedRulesLayers, RulesLayerStack};
use crate::rules::object_type::{BuildCategory, FactoryType, ObjectCategory, ObjectType};
use crate::rules::particle_system_type::{
    ParticleSystemType, ParticleSystemTypeId, PendingParticleSystemType,
};
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
use crate::util::fixed_math::{SIM_ONE, SimFixed, sim_from_f32};
use crate::util::native_x87::{NativeF32Bits, NativeF64Bits};

#[cfg(test)]
#[path = "building_abandoned_sound_tests.rs"]
mod building_abandoned_sound_tests;

/// Country-level fields needed by gameplay systems.
#[derive(Debug, Clone)]
pub struct CountryRules {
    /// `MultiplayPassive=` allows non-owner garrison entry in `BuildingClass::CanBeOccupiedBy`.
    pub multiplay_passive: bool,
    /// `WallOwner=` allows this house type's buildings to claim nearby map walls.
    pub wall_owner: bool,
    /// `IncomeMult=` ore-refining income multiplier, parts-per-million
    /// (`round(value × 1e6)`); default `1_000_000` (=1.0, the stock value — the key is
    /// commented out in stock rulesmd). Applied at ore/gem deposit time to BOTH the base
    /// credits and the OrePurifier-bonus credits.
    pub income_ppm: i64,
    /// Per-target category armor multipliers (HouseType `+0x100..+0x110`).
    /// Native stores these as f32 and `HouseClass::GetArmorMultForType @
    /// 0x0050BD30` reads the selected slot live for every receiver call.
    ///
    /// The country's `Armor=` (HouseType `+0xE0`) is not represented: native
    /// folds it with the difficulty `Armor=` into `House+0x1A0`
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
    /// `UIName=` — the country's string-table key (e.g. `Name:Americans`).
    /// gamemd fills a house's stored display name from this key's localized text,
    /// which is what the end-of-match score screen shows in the Player column.
    pub ui_name: Option<String>,
    /// `Name=` — the country's plain English name, the fallback when `UIName=`
    /// is absent or its key does not resolve.
    pub name: Option<String>,
}

/// PPM scale for `IncomeMult` and `PurifierBonus` (1_000_000 = 1.0×), the
/// divisor of `Economy::add_tiberium_credits`.
pub const INCOME_PPM_SCALE: i64 = 1_000_000;

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
        // Hand-written (NOT derived): a derived Default would zero `income_ppm`, which would
        // wipe out all ore income for any house built from the default. The neutral 1.0
        // multiplier is `INCOME_PPM_SCALE`, not 0.
        Self {
            multiplay_passive: false,
            wall_owner: true,
            income_ppm: INCOME_PPM_SCALE,
            armor_infantry_mult: 1.0,
            armor_units_mult: 1.0,
            armor_aircraft_mult: 1.0,
            armor_buildings_mult: 1.0,
            armor_defenses_mult: 1.0,
            cost_mults: [NativeF32Bits::ONE; 5],
            speed_mults: [NativeF32Bits::ONE; 3],
            build_time_mults: [NativeF32Bits::ONE; 5],
            rof: 1.0,
            ui_name: None,
            name: None,
        }
    }
}

impl CountryRules {
    fn from_ini_section(section: &crate::rules::ini_parser::IniSection) -> Self {
        Self {
            multiplay_passive: section.read_bool("MultiplayPassive", false),
            wall_owner: section.read_bool("WallOwner", true),
            // IncomeMult is a raw multiplier (NOT a percent). Round in f64 to avoid f32
            // drift; absent -> the neutral 1.0 (stock).
            // HouseTypeClass::ReadINI stores these as floats (`FSTP dword` at
            // 0x00511ADD..0x00511B59 and 0x00511D0B).
            income_ppm: (f64::from(section.read_float("IncomeMult", 1.0)) * INCOME_PPM_SCALE as f64)
                .round() as i64,
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
            rof: section.read_double("ROF", 1.0),
            // AbstractTypeClass::ReadINI: `Name` into 0x31 bytes (0x00410AA0),
            // `UIName` into 0x20 (0x00410AFB).
            ui_name: section.read_name("UIName", 0x20).map(str::to_owned),
            name: section.read_name("Name", 0x31).map(str::to_owned),
        }
    }
}

/// Stable source-order identity in the `[Countries]` registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct CountryIdx(pub u16);

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

/// Convert RulesClass `[AudioVisual] SavourDelay` minutes to the signed timer's
/// ordinary non-negative frame domain. Native multiplies by 900.0 and calls
/// `Math__ftol`, whose active x87 control word truncates toward zero.
pub(crate) fn savour_delay_frames(minutes: f64) -> u64 {
    (minutes * 900.0).clamp(0.0, u64::MAX as f64).trunc() as u64
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

/// Global gameplay constants from `[General]` that affect vision, gap generators, etc.
#[derive(Debug, Clone)]
pub struct GeneralRules {
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
    /// `[General] VeteranROF=` — `RulesClass+0x690`, read at `0x0066EF61`.
    /// `TechnoClass::GetROF @ 0x006FCFA0` multiplies the jittered reload by it
    /// at `0x006FD136..0x006FD145` for a `ROF`-ability holder. Constructor
    /// default 1.0 (`0x00665F86`).
    pub veteran_rof: f64,
    /// `[Easy]`, `[Normal]` and `[Difficult]` `ROF=`, the `ROF` field
    /// (`+0x20`) of the three difficulty rows at `RulesClass+0x1538`
    /// (stride `0x50`), in that order. `RulesClass::Process` reads them at
    /// `0x00668EF5..0x00668F26` through `ReadDifficulty @ 0x0066D270`, which
    /// runs only when the section exists and reads `ROF` with ReadDouble and
    /// an explicit default of 1.0 (`0x0066D2E9..0x0066D2FD`). The row index is
    /// `HouseClass+0x184` ([`HouseDifficulty`](crate::sim::house_state::HouseDifficulty)
    /// order: 0 is a Hard AI, 2 an Easy AI). The constructor leaves the rows
    /// unset (it skips from `+0x1530` to `+0x1638`, `0x006673E2`), so a
    /// missing section is undefined natively; retail defines all three, and
    /// VERA reads a missing one as 1.0.
    pub difficulty_rof: [f64; 3],
    /// The same rows' `RepairDelay=` (`+0x38`, ReadDouble with an explicit
    /// default of .02, `0x0066D317`; retail `.02`, `.02`, `.05`).
    /// `HouseClass::SetDifficulty` copies the house's row into `+0x1C0`; the
    /// computer's auto-repair start draws its latch time from it
    /// (`0x00450727`). A missing section skips every read
    /// (`0x0066D27C..0x0066D288`) and leaves the row the constructor never
    /// writes, undefined natively as the ROF rows are; VERA reads a missing
    /// one as the key's default .02.
    pub difficulty_repair_delay: [f64; 3],
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
    /// `[General] RepairRate=` in minutes — `RulesClass+0x16E0`.
    ///
    /// The self-heal pulse (`FUN_0070BE80`) divides the frame counter by
    /// `ftol(RepairRate * 900)` (`0x0070BEFE..0x0070BF0A`, constant 900.0 at
    /// `0x007E27F8`); stock `.016` gives a 14-frame pulse.
    pub repair_rate_minutes: f64,
    /// `[General] SelfHealInfantryFrames=` — `RulesClass+0x30`, read by the
    /// General reader at `0x0066D530`. The house pulse of
    /// `TechnoClass::AI_Update @ 0x006FA8E2` divides the global frame counter
    /// by it (`idiv`) and runs only on an exact remainder of zero. Stock 50.
    pub self_heal_infantry_frames: i32,
    /// `[General] SelfHealInfantryAmount=` — `RulesClass+0x34`, read by the
    /// same reader. `HouseClass::GetInfSelfHealStep @ 0x0050D9E0` multiplies it
    /// by the house's infantry count (`+0x164`). Stock 20.
    pub self_heal_infantry_amount: i32,
    /// `[General] SelfHealUnitFrames=` — `RulesClass+0x38`, same reader. The
    /// unit pulse of `TechnoClass::AI_Update @ 0x006FA8E2` divides the frame
    /// counter by it. Stock 75.
    pub self_heal_unit_frames: i32,
    /// `[General] SelfHealUnitAmount=` — `RulesClass+0x3C`, same reader.
    /// `HouseClass::GetUnitSelfHealStep @ 0x0050D9F0` multiplies it by the
    /// house's unit count (`+0x168`). Stock 5.
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
    /// Paradrop trigger radius in leptons. From `[General] ParadropRadius=`.
    /// Default 1024 (~4 cells). Distance to target at which the carrier aircraft
    /// reveals fog + transitions to the overfly mission.
    pub paradrop_radius: i32,
    /// Parsed `[General] Parachute=` value (uppercased AnimType name, e.g.
    /// "PARACH"), `None` if unset or empty: the canopy the simulation attaches
    /// to a dropped object. Its timing is the AnimType's own art.
    pub parachute_shp: Option<String>,
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
    /// Fatal infantry animation bindings indexed by Warhead `InfDeath`.
    /// Slots 3/4/6..10 come from `[General]`; slot 5 is the second declared
    /// `[Animations]` entry. Slots 0..2 intentionally have no external anim.
    pub infantry_death_anims: [Option<String>; 11],
    /// Whether the attack cursor appears on a disguised Spy (AttackCursorOnDisguise= in [General]).
    /// Default false (vanilla RA2). When false, a disguised Spy does not show the attack cursor.
    pub attack_cursor_on_disguise: bool,
    /// `[General] DefaultMirageDisguises=` selection pool, in source order.
    pub default_mirage_disguises: Vec<String>,
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
    pub cloak_delay_frames: i32,
    /// `[AudioVisual] CloakSound=` resolved by the app audio registry when a
    /// cloak transition requests positional playback. Stock YR binds
    /// `NavalUnitEmerge`; the native constructor's invalid Voc index is silence
    /// when the key is absent or cannot resolve.
    pub cloak_sound: Option<String>,
    /// `[AudioVisual] UpgradeVeteranSound=` — `RulesClass::ReadAudioVisual @
    /// 0x006691E0` resolves it to a Voc index at `RulesClass+0x228`
    /// (`0x00669CFD..0x00669D27`); `TechnoClass::AI_Update` plays that index
    /// positionally on a veteran promotion (`0x006FA124..0x006FA12A`). The
    /// name is retained at the data boundary; absent or empty is silence.
    pub upgrade_veteran_sound: Option<String>,
    /// `[AudioVisual] UpgradeEliteSound=` — `RulesClass+0x22C`
    /// (`0x00669D3E..0x00669D27`), played on an elite promotion
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
    /// [`damage_spark_spawn_threshold`]. Default 42_949_673 (band 0.02).
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
    pub building_damage_sound: Option<String>,
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
    /// `[AudioVisual] ScoldSound=` (`Rules+0x700`, read at `0x0066ABE8`
    /// through `VocClass::FindByName`; retail `MenuScold`): ToggleRepair's
    /// sound when a repair is switched on at full Strength (`0x00447068`).
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
    /// `MenuTab`). The key→tab-click mapping is name-inferred — flagged for a
    /// Ghidra spot-check of the tab-ID consumer before parity sign-off.
    pub gui_tab_sound: Option<String>,
    /// Message-insert sound from [AudioVisual] IncomingMessage (retail
    /// `MessageText`). Plays on every non-silent message-list insert.
    pub incoming_message_sound: Option<String>,
    /// Chat/system message lifetime in MINUTES from [AudioVisual]
    /// MessageDelay (retail `.6`). The exact native minutes→ticks binding is
    /// untraced (plan deferred item); the driver converts minutes→ms.
    pub message_delay_minutes: f32,
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
    /// Duration of spy-triggered total power blackout in game frames (15 fps).
    /// Parsed from SpyPowerBlackout= in [General]. Default 1000 frames (~67 seconds).
    pub spy_power_blackout_frames: u32,
    /// Fire/smoke anim types spawned on buildings below ConditionYellow health.
    /// Parsed from DamageFireTypes= in [General]. Default: FIRE01,FIRE02,FIRE03.
    pub damage_fire_types: Vec<AnimRef>,
    /// Particle system spawned by exploding barrels.
    /// Parsed from `BarrelParticle=` in `[General]` (NOT `[AudioVisual]`,
    /// despite the proximity to other AudioVisual keys).
    /// Holds the unresolved section name; ID resolution against the
    /// particle-system registry is deferred (matches A2/A3/A4/A5a pattern).
    pub barrel_particle: Option<String>,

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

    /// Ore Purifier bonus as a fixed-point fraction in parts-per-million
    /// (PurifierBonus= in [General]; `INCOME_PPM_SCALE` = 1.0×). Stored at full
    /// precision so modded fractional percentages (e.g. `.333`) are not quantized
    /// to whole percent. Stock `.25` -> 250_000 (25%). Default 250_000.
    pub purifier_bonus_ppm: i64,
    /// AI virtual purifier counts indexed by difficulty
    /// (AIVirtualPurifiers= in [General]). Each entry is added to the AI
    /// player's real purifier count when computing the deposit bonus. INI
    /// convention is hardest-first, so the array is `[Brutal, Medium, Easy]`
    /// with the retail default `[4, 2, 0]`.
    pub ai_virtual_purifiers: [i32; 3],

    // -- Survivor spawning on sell/destroy --
    /// Divisor to compute survivor count for Allied buildings (AlliedSurvivorDivisor= in [General]).
    /// Survivor count = sell_refund / divisor (rounded down, min 0). Default 500.
    pub allied_survivor_divisor: i32,
    /// Divisor to compute survivor count for Soviet buildings (SovietSurvivorDivisor= in [General]).
    /// Default 250.
    pub soviet_survivor_divisor: i32,
    /// Divisor to compute survivor count for Third-side (Yuri) buildings (ThirdSurvivorDivisor= in [General]).
    /// YR addition. Default 750.
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
    pub tracked_uphill: SimFixed,
    /// Tracked vehicle downhill coefficient (`TrackedDownhill=`).
    pub tracked_downhill: SimFixed,
    /// Non-tracked (wheeled and other) vehicle uphill coefficient (`WheeledUphill=`).
    pub wheeled_uphill: SimFixed,
    /// Non-tracked vehicle downhill coefficient (`WheeledDownhill=`).
    pub wheeled_downhill: SimFixed,

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
    /// `[IQ] RepairSell` outer gate for BuildingClass repair/sell AI.
    pub iq_repair_sell: i32,
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
    /// Whether `[General] AmbientChangeRate=` is nonzero before its native
    /// frame conversion. Kept separately because a nonzero mod value can chop
    /// to a zero-frame interval while still passing ScenarioClass's outer gate.
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
    /// Launch case 9's walk hands each infantryman. The constructor's null
    /// (`0x00666B3A`): empty.
    pub mutate_warhead: String,
    /// `[SpecialWeapons] MutateExplosionWarhead=` (`+0xF9C`, `0x006690FA`):
    /// the warhead of case 9's area damage. The constructor's null
    /// (`0x00666B40`): empty.
    pub mutate_explosion_warhead: String,
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

/// Count of representable `roll` values for the damage-Spark prob-roll, i.e.
/// `RandomRanged(0, 0x7ffffffe)` yields `[0, 0x7ffffffe]`, so 0x7fffffff values.
/// A band of >= 1.0 lets every roll pass.
const DAMAGE_SPARK_ROLL_COUNT: u32 = 0x7fff_ffff;

/// Compute the integer spawn threshold for a damage-Spark probability `band`:
/// the number of `roll` values in `[0, 0x7ffffffe]` for which gamemd's roll
/// SUCCEEDS, so the per-tick test reduces to the pure-integer `roll < threshold`
/// (no float in the sim hot path; `roll` from `next_range_u32_inclusive(0,
/// 0x7ffffffe)`).
///
/// gamemd compares `(double)roll * SCALE < band` with `SCALE` the exact double
/// `0x3E00000000400000` = `(2^30 + 1)·2^-61`, evaluated in x87 80-bit. Because
/// `roll·(2^30 + 1) <= 2^61 - 2` fits the 64-bit x87 mantissa, that product is
/// the EXACT real value, and `band` is an exact f64 — so the boundary is the
/// exact-rational comparison `roll·(2^30+1)·2^-61 < band`, computed here with
/// integer arithmetic (machine-independent; no float, no x87 dependency, no
/// multiply-vs-divide rounding hazard). A 1-off here flips the draw count on a
/// boundary tick → desync, so the two stock thresholds are pinned by test.
pub(crate) fn damage_spark_spawn_threshold(band: f64) -> u32 {
    // band <= 0 (or NaN): no roll passes. band >= 1: every roll passes.
    if !(band > 0.0) {
        return 0;
    }
    if band >= 1.0 {
        return DAMAGE_SPARK_ROLL_COUNT;
    }
    // Decompose band = mant · 2^exp (normalized double carries the implicit 53rd
    // bit): value = (2^52 + frac) · 2^(exp_field - 1075).
    let bits = band.to_bits();
    let exp_field = ((bits >> 52) & 0x7ff) as i32;
    let frac = bits & ((1u64 << 52) - 1);
    let (mant, exp) = if exp_field == 0 {
        (frac, -1074) // subnormal
    } else {
        ((1u64 << 52) | frac, exp_field - 1075)
    };
    // roll·SCALE >= band  <=>  roll·(2^30+1) >= mant·2^(exp+61).
    // threshold = smallest qualifying roll = ceil(mant·2^(exp+61) / (2^30+1)).
    const D: u128 = (1u128 << 30) + 1;
    let k = exp + 61;
    let threshold: u128 = if k >= 0 {
        ((mant as u128) << (k as u32)).div_ceil(D)
    } else {
        // mant·2^(exp+61) is fractional; scale the divisor instead:
        // roll >= ceil(mant / ((2^30+1) << -k)).
        (mant as u128).div_ceil(D << ((-k) as u32))
    };
    threshold.min(DAMAGE_SPARK_ROLL_COUNT as u128) as u32
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

/// The single BuildingTypes `RulesClass::ReadGeneral` names in `[General]`,
/// as stored IDs (`read_building_identity`; the layered reader in
/// `native_processing` replaces the projection).
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
#[derive(Debug, Clone, Default, PartialEq, Eq)]
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

/// ReadGeneral's BuildingType identity reads (PrismType `0x0067BCE0`, the
/// gates and WallTower `0x0066F450..0x0066F583`): ReadString128 into a local
/// buffer; an empty value keeps the current pointer, anything else goes
/// through BuildingType FindOrAllocate (`0x004653C0`), whose `<none>` and
/// `none` answer null. PrerequisiteProcAlternate reads a UnitType the same
/// way (`0x007480D0`).
fn read_building_identity(
    general: &IniSection,
    key: &str,
    current: Option<String>,
) -> Option<String> {
    match general.read_name(key, 0x80) {
        None => current,
        Some(name) if is_native_none_type_name(name) => None,
        Some(name) => Some(name.to_ascii_uppercase()),
    }
}

/// Retained `[AudioVisual]` frame-rate thresholds. The RulesClass constructor
/// writes 15/20/5 at +0/+4/+8 (665665..665672). ReadAudioVisual66920D..669258
/// reads signed integers in this order with current defaults and no clamps.
/// Native constructor/full-reader controls: rules_oracle/weapon_laser.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PrismSupportRules {
    /// `PrismSupportModifier=` (`Rules+0x49C`), the percent each supporter
    /// adds to the shot: `ftol(ReadDouble(key, current) x 100)`, so a pass
    /// with a `[General]` section and no key multiplies it by 100 again.
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
    pub(crate) fn read_pass(self, general: &crate::rules::ini_parser::IniSection) -> Self {
        use crate::util::native_x87::{MaskedX87Chop53 as X87, NativeF64Bits};
        let percent = general.read_double("PrismSupportModifier", f64::from(self.modifier));
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

impl Default for GeneralRules {
    fn default() -> Self {
        Self {
            detail: DetailRules::default(),
            pose_dir: 0,
            deploy_dir: 0,
            scroll_multiplier: 0.07,
            // RulesClass__Constructor @ 0x00665650 writes the double
            // 0x3F9EB851EB851EB8 to +0x14C8.
            savour_delay_minutes: 0.03,
            gravity: 3,
            veteran_sight: 0.0,
            veteran_combat: 1.0,
            veteran_speed: 1.0,
            veteran_rof: 1.0,
            difficulty_rof: [1.0; 3],
            difficulty_repair_delay: [DIFFICULTY_REPAIR_DELAY_DEFAULT; 3],
            veteran_armor: 1.0,
            curley_shuffle: false,
            repair_rate_minutes: 0.016,
            // The native constructor never writes Rules+0x30/+0x34/+0x38/+0x3C,
            // so the reader's argument stands when the key is absent.
            self_heal_infantry_frames: 0,
            self_heal_infantry_amount: 0,
            self_heal_unit_frames: 0,
            self_heal_unit_amount: 0,
            veteran_ratio: VETERAN_RATIO_DEFAULT,
            veteran_cap: VETERAN_CAP_DEFAULT,
            computer_base_defense_response: 3,
            // Native Rules+0xE48 constructor default; active retail overrides to 3.
            maximum_building_placement_failures: 5,
            placement_delay: 0.05,
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
            tunnel_speed: sim_from_f32(6.0),
            missile_rot_var: 0.25,
            safety_altitude: 500,
            line_trail_color_override: [0; 3],
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
            parachute_max_fall_rate: -3,
            paradrop_radius: 1024,
            parachute_shp: None,
            amer_paradrop: ParadropList::default(),
            ally_paradrop: ParadropList::default(),
            sov_paradrop: ParadropList::default(),
            yuri_paradrop: ParadropList::default(),
            anim_to_infantry: Vec::new(),
            base_unit_types: vec!["AMCV".to_string(), "SMCV".to_string(), "PCV".to_string()],
            multiplayer_ai_cm: Vec::new(),
            pad_aircraft_types: Vec::new(),
            separate_aircraft: false,
            prism_type: None,
            prerequisite_proc_alternate: None,
            prism_support: PrismSupportRules::default(),
            building_types: GeneralBuildingTypes::default(),
            tiberium_grows: true,
            tiberium_spreads: true,
            growth_rate_minutes: 2.0,
            warp_in: AnimRef {
                name: "WARPIN".to_string(),
            },
            warp_out: AnimRef {
                name: "WARPOUT".to_string(),
            },
            warp_away: AnimRef {
                name: "WARPAWAY".to_string(),
            },
            chrono_sparkle1: AnimRef {
                name: "CHRONOSK".to_string(),
            },
            wake: AnimRef {
                name: "WAKE1".to_string(),
            },
            move_flash: AnimRef {
                name: "RING".to_string(),
            },
            infantry_death_anims: [
                None,
                None,
                None,
                Some("S_BANG34".to_string()),
                Some("FLAMEGUY".to_string()),
                Some("ELECTRO".to_string()),
                Some("YURIDIE".to_string()),
                Some("NUKEDIE".to_string()),
                Some("VIRUSD".to_string()),
                None,
                Some("BRUTDIE".to_string()),
            ],
            attack_cursor_on_disguise: false,
            default_mirage_disguises: Vec::new(),
            infantry_blink_disguise_time: 0,
            tree_targeting: false,
            tree_strength: 25,
            condition_yellow: 0.5,
            condition_red: 0.5,
            cloaking_stages: 9,
            cloak_delay_frames: 18,
            cloak_sound: None,
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
            condition_red_sparking_probability: 0.02,
            condition_yellow_sparking_probability: 0.01,
            condition_red_spark_threshold: damage_spark_spawn_threshold(0.02),
            condition_yellow_spark_threshold: damage_spark_spawn_threshold(0.01),
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
            generic_click_sound: None,
            guard_sound: None,
            scold_sound: None,
            generic_beep_sound: None,
            gui_checkbox_sound: None,
            ore_twinkle: None,
            ore_twinkle_chance: 50,
            gui_build_sound: None,
            building_slam: None,
            gui_tab_sound: None,
            incoming_message_sound: None,
            message_delay_minutes: 0.6,
            speak_delay_minutes: 0.0,
            gui_combo_open_sound: None,
            gui_combo_close_sound: None,
            bunker_walls_down_sound: None,
            bunker_walls_up_sound: None,
            direct_rocking_coefficient: SimFixed::lit("1.5"),
            fallback_coefficient: SimFixed::lit("0.1"),
            chrono_in_sound: Some("ChronoMinerTeleport".to_string()),
            chrono_out_sound: Some("ChronoMinerTeleport".to_string()),
            impact_water_sound: None,
            impact_land_sound: None,
            sinking_sound: None,
            construction_sound: None,
            building_repaired_sound: None,
            bomb_ticking_sound: None,
            bomb_attach_sound: None,
            damage_delay_minutes: 1.0,
            spy_power_blackout_frames: 1000,
            damage_fire_types: vec![],
            barrel_particle: None,
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
            purifier_bonus_ppm: 250_000,
            ai_virtual_purifiers: [4, 2, 0],
            allied_survivor_divisor: 500,
            soviet_survivor_divisor: 250,
            third_survivor_divisor: 750,
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
            tracked_uphill: SIM_ONE,
            tracked_downhill: SIM_ONE,
            wheeled_uphill: SIM_ONE,
            wheeled_downhill: SIM_ONE,
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
            repair_step: 5,
            repair_percent: 0.25,
            // RulesClass constructor 0x0066738F.
            reload_rate: 0.05,
            // PathDelay=.01 min = 0.6 sec = 9 ticks at 15 Hz.
            path_delay: 0.016,
            // BlockagePathDelay=60 frames (directly in frames, not minutes).
            blockage_path_delay_ticks: 60,
            ai_auto_deploy_frame_delay: Vec::new(),
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
            iq_repair_sell: 3,
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
            ambient_change_rate_nonzero: true,
            ambient_change_interval_frames: 180,
            ambient_change_step: 20,
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
            force_shield_radius: 10,
            force_shield_duration: 400,
            force_shield_blackout_duration: 800,
            force_shield_fade_sound_time: 50,
            force_shield_invoke_anim: String::new(),
            psychic_reveal_radius: 3,
            mutate_warhead: String::new(),
            mutate_explosion_warhead: String::new(),
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
    /// Range bonus in cells for bunker passengers.
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
            bunker_weapon_range_bonus: 0,
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
    pub destroyable_by_default: bool,
    /// SHP animation names to spawn when a bridge group is destroyed
    /// (e.g., TWLT026, TWLT036, TWLT050, TWLT070). Picked randomly per cell.
    pub explosions: Vec<String>,
    /// Maximum metallic-debris voxels spawned per destroyed bridge cell.
    /// Parsed from `[General] BridgeVoxelMax=` in rules.ini (default 3).
    /// Consumed by the damage state machine in a later tier.
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
            destroyable_by_default: true,
            explosions: Vec::new(),
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
        let destroyable_by_default = true;
        // Published from the ordered RulesClass reader by from_processed_rules.
        let explosions = Vec::new();
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
            destroyable_by_default,
            explosions,
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
    /// zero, and the original computes that product in doubles (the same
    /// documented float exception as `combat::damage`). Parsed straight from
    /// the INI string so the value is bit-identical to a double `atof`.
    pub level_factor: f64,
    /// Light intensity per level point (`RadLightFactor`). Render-only.
    pub light_factor: SimFixed,
    /// Tint scale for the radiation glow (`RadTintFactor`). Render-only.
    pub tint_factor: SimFixed,
    /// Glow color (`RadColor=R,G,B`). Render-only.
    pub color: (u8, u8, u8),
    /// Warhead used for radiation damage (`RadSiteWarhead`), uppercased.
    pub site_warhead: String,
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
            light_factor: sim_from_f32(0.1),
            tint_factor: sim_from_f32(1.0),
            color: (0, 255, 0),
            site_warhead: "RadSite".to_string(),
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
            section.read_color_rgb("RadColor", [d.color.0, d.color.1, d.color.2]);
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
            light_factor: sim_from_f32(
                section.read_double("RadLightFactor", d.light_factor.to_num()) as f32,
            ),
            tint_factor: sim_from_f32(
                section.read_double("RadTintFactor", d.tint_factor.to_num()) as f32
            ),
            color: (red, green, blue),
            site_warhead: section
                .read_name("RadSiteWarhead", 0x80)
                .map(str::to_owned)
                .unwrap_or(d.site_warhead),
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

/// `[General] AmbientChangeRate=` and `AmbientChangeStep=` (Rules `+0x1668`,
/// `+0x1670`) as `LogicClass::PerTickUpdate`'s ambient fade reads them:
/// whether the rate is nonzero (`0x0055B351..0x0055B362`), the interval
/// `ftol(rate * 900)` (`0x0055B3D8..0x0055B3E4`) and the step
/// `ftol(step * 100)` (`0x0055B447..0x0055B453`).
pub(crate) fn ambient_change_terms(rate: f64, step: f64) -> (bool, i32, i32) {
    (rate != 0.0, (rate * 900.0) as i32, (step * 100.0) as i32)
}

impl GeneralRules {
    /// Drive4B3A65, Ship6A30B4 and Foot/Walk's timer producers retain the
    /// configured double until FLD/FMUL900/ftol7C5F00. In particular, authored
    /// decimal .01 is parsed through float by ReadDouble5283D0 and converts to
    /// 8, while 1% converts to9. No clamp, round-to-nearest or u16 narrowing.
    pub fn path_delay_ticks(&self) -> i32 {
        native_minutes_to_frames(self.path_delay)
    }

    /// `ftol(PlacementDelay * 900.0)`, the computer Construction Yard's retry
    /// wait (`BuildingClass::Factory_AI @ 0x004501DB..0x004501F3`). Retail .05
    /// gives 45.
    pub fn placement_delay_frames(&self) -> i32 {
        native_minutes_to_frames(self.placement_delay)
    }

    pub fn infantry_death_anim(&self, inf_death: u8) -> Option<&str> {
        self.infantry_death_anims
            .get(usize::from(inf_death))
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
        let detail = defaults.detail.read_pass(audio_visual);
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
        // ReadAudioVisual too (`0x0066B877..0x0066B891`).
        let force_shield_color =
            audio_visual.read_int("ForceShieldColor", defaults.force_shield_color);
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
        let path_delay = ai.read_double("PathDelay", defaults.path_delay);
        let blockage_path_delay_ticks =
            ai.read_int("BlockagePathDelay", defaults.blockage_path_delay_ticks);
        let Some(general) = ini.section("General") else {
            return Self {
                detail,
                pose_dir,
                deploy_dir,
                iq_production,
                iq_harvester,
                iq_super_weapons,
                display_cruise_height,
                condition_yellow: condition_yellow_native,
                condition_red: condition_red_native,
                path_delay,
                blockage_path_delay_ticks,
                bomb_ticking_sound,
                bomb_attach_sound,
                force_shield_color,
                ally_reveal,
                psychic_reveal_activate_sound,
                psychic_reveal_radius,
                ..defaults
            };
        };
        // Combat-only globals are read in the late [CombatDamage] pass.
        let combat_damage = ini.section_or_empty("CombatDamage");
        // Base-planning/credit controls live in the independent [AI] read.
        // Genetic Mutator warhead references are read by [SpecialWeapons].
        let special_weapons = ini.section_or_empty("SpecialWeapons");
        // INI parser already strips everything after `;` (Westwood comment
        // marker), so values like `WarpOut=WARPOUT;WAKE2` are read as
        // `WARPOUT` — matching gamemd's behaviour.
        let parse_anim_name =
            |key: &str, default: &str| -> String { general.read_string(key, default, 0x80) };
        let mut infantry_death_anims = defaults.infantry_death_anims.clone();
        for (index, key, fallback) in [
            (3, "InfantryExplode", "S_BANG34"),
            (4, "FlamingInfantry", "FLAMEGUY"),
            (6, "InfantryHeadPop", "YURIDIE"),
            (7, "InfantryNuked", "NUKEDIE"),
            (8, "InfantryVirus", "VIRUSD"),
            // `InfantryMutate=` (`+0xB4`, `0x0066E4B6`) keeps the
            // constructor's null type when absent; retail `GENDEATH`.
            (9, "InfantryMutate", ""),
            (10, "InfantryBrute", "BRUTDIE"),
        ] {
            infantry_death_anims[index] =
                Some(parse_anim_name(key, fallback)).filter(|name| !name.is_empty());
        }
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
        // These are ReadDouble values (single-precision parse widened to f64)
        // and the consumer's ftol boundary chops toward zero.
        let (ambient_change_rate_nonzero, ambient_change_interval_frames, ambient_change_step) =
            ambient_change_terms(
                general.read_double("AmbientChangeRate", 0.2),
                general.read_double("AmbientChangeStep", 0.2),
            );
        Self {
            detail,
            pose_dir,
            deploy_dir,
            scroll_multiplier: audio_visual
                .read_double("ScrollMultiplier", defaults.scroll_multiplier),
            savour_delay_minutes: audio_visual
                .read_double("SavourDelay", defaults.savour_delay_minutes),
            condition_red_sparking_probability: condition_red_spark_prob,
            condition_yellow_sparking_probability: condition_yellow_spark_prob,
            condition_red_spark_threshold: damage_spark_spawn_threshold(condition_red_spark_prob),
            condition_yellow_spark_threshold: damage_spark_spawn_threshold(
                condition_yellow_spark_prob,
            ),
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
            // Passive-scan cadence, in frames. Both keys are present in stock
            // rulesmd.ini with exactly the constructor defaults (27 / 36); read
            // them rather than hardcoding so a mod's values take effect.
            dead_bodies: general
                .read_list("DeadBodies", 0x80)
                .map(|tokens| tokens.into_iter().map(str::to_owned).collect())
                .unwrap_or_default(),
            normal_targeting_delay: general
                .read_int("NormalTargetingDelay", defaults.normal_targeting_delay),
            guard_area_targeting_delay: general.read_int(
                "GuardAreaTargetingDelay",
                defaults.guard_area_targeting_delay,
            ),
            // Gravity lives in [AudioVisual] (stock value 6). Reading it from
            // [General] silently fell back to the code default 3 — half stock
            // gravity for spark ballistics and the hover bob amplitude.
            gravity: audio_visual.read_int("Gravity", defaults.gravity),
            veteran_sight: general.read_double("VeteranSight", defaults.veteran_sight),
            veteran_combat: general.read_double("VeteranCombat", defaults.veteran_combat),
            veteran_speed: general.read_double("VeteranSpeed", defaults.veteran_speed),
            veteran_rof: general.read_double("VeteranROF", defaults.veteran_rof),
            difficulty_rof: ["Easy", "Normal", "Difficult"].map(|name| {
                ini.section(name)
                    .map_or(1.0, |section| section.read_double("ROF", 1.0))
            }),
            difficulty_repair_delay: ["Easy", "Normal", "Difficult"].map(|name| {
                ini.section(name)
                    .map_or(DIFFICULTY_REPAIR_DELAY_DEFAULT, |section| {
                        section.read_double("RepairDelay", DIFFICULTY_REPAIR_DELAY_DEFAULT)
                    })
            }),
            veteran_armor: general.read_double("VeteranArmor", 1.0),
            curley_shuffle: general.read_bool("CurleyShuffle", defaults.curley_shuffle),
            repair_rate_minutes: general.read_double("RepairRate", defaults.repair_rate_minutes),
            // `RulesClass+0x30/+0x34/+0x38/+0x3C`, read by the General reader at
            // `0x0066D530`. The native constructor leaves them untouched, so a
            // rules set without the key keeps the reader's argument: zero.
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
            tunnel_speed: sim_from_f32(general.read_double("TunnelSpeed", 6.0) as f32),
            missile_rot_var: general.read_double("MissileROTVar", defaults.missile_rot_var),
            safety_altitude: general.read_int("MissileSafetyAltitude", defaults.safety_altitude),
            line_trail_color_override: audio_visual
                .read_color_rgb("LineTrailColorOverride", defaults.line_trail_color_override),
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
            parachute_max_fall_rate: general.read_int("ParachuteMaxFallRate", -3),
            paradrop_radius: general.read_int("ParadropRadius", 1024),
            parachute_shp: general.read_name("Parachute", 0x80).map(str::to_uppercase),
            amer_paradrop: ParadropList::read_counts(general, "AmerParaDropNum"),
            ally_paradrop: ParadropList::read_counts(general, "AllyParaDropNum"),
            sov_paradrop: ParadropList::read_counts(general, "SovParaDropNum"),
            yuri_paradrop: ParadropList::read_counts(general, "YuriParaDropNum"),
            // The processed RulesClass vector is authoritative.
            anim_to_infantry: Vec::new(),
            base_unit_types: general
                .read_list("BaseUnit", 0x80)
                .map(|tokens| tokens.into_iter().map(str::to_ascii_uppercase).collect())
                .unwrap_or_else(|| defaults.base_unit_types),
            multiplayer_ai_cm: general
                .read_int_list("MultiplayerAICM")
                .unwrap_or_else(|| defaults.multiplayer_ai_cm),
            pad_aircraft_types: general
                .read_list("PadAircraft", 0x80)
                .map(|tokens| tokens.into_iter().map(str::to_ascii_uppercase).collect())
                .unwrap_or_else(|| defaults.pad_aircraft_types),
            separate_aircraft: general.read_bool("SeparateAircraft", defaults.separate_aircraft),
            // `0x00671144` -> `0x0067BCE0`. The layered reader
            // (`native_processing`) replaces this projection.
            prism_type: read_building_identity(general, "PrismType", defaults.prism_type),
            // `0x0066F787..0x0066F7C9`, replaced by the layered reader too.
            prerequisite_proc_alternate: read_building_identity(
                general,
                "PrerequisiteProcAlternate",
                defaults.prerequisite_proc_alternate,
            ),
            prism_support: defaults.prism_support.read_pass(general),
            building_types: GeneralBuildingTypes {
                gdi_gate_one: read_building_identity(general, "GDIGateOne", None),
                gdi_gate_two: read_building_identity(general, "GDIGateTwo", None),
                nod_gate_one: read_building_identity(general, "NodGateOne", None),
                nod_gate_two: read_building_identity(general, "NodGateTwo", None),
                wall_tower: read_building_identity(general, "WallTower", None),
                gdi_power_plant: read_building_identity(general, "GDIPowerPlant", None),
                nod_regular_power: read_building_identity(general, "NodRegularPower", None),
                nod_advanced_power: read_building_identity(general, "NodAdvancedPower", None),
                third_power_plant: read_building_identity(general, "ThirdPowerPlant", None),
            },
            tiberium_grows: general.read_bool("TiberiumGrows", true),
            tiberium_spreads: general.read_bool("TiberiumSpreads", true),
            growth_rate_minutes: general.read_double("GrowthRate", 2.0) as f32,
            attack_cursor_on_disguise: general.read_bool("AttackCursorOnDisguise", false),
            // Filled from the native ordered TerrainType list owner after
            // processing, including allocation/none-token/retention semantics.
            default_mirage_disguises: Vec::new(),
            // Rules671D80: signed ReadInt, constructor665650 defaults0.
            infantry_blink_disguise_time: general.read_int("InfantryBlinkDisguiseTime", 0),
            tree_targeting: combat_damage.read_bool("TreeTargeting", false),
            tree_strength: general.read_int("TreeStrength", defaults.tree_strength),
            condition_yellow: condition_yellow_native,
            condition_red: condition_red_native,
            cloaking_stages: general.read_int("CloakingStages", 9),
            cloak_delay_frames: (general.read_double("CloakDelay", 0.02) * 900.0)
                .trunc()
                .clamp(i32::MIN as f64, i32::MAX as f64) as i32,
            // RulesClass::ReadAudioVisual @ 0x006691E0 resolves CloakSound into
            // RulesClass+0x6A0. Retain the name at the data boundary; an absent
            // or empty key leaves the native invalid-index/no-play behavior.
            cloak_sound: audio_visual
                .read_name("CloakSound", 0x80)
                .map(str::to_owned),
            upgrade_veteran_sound: audio_visual
                .read_name("UpgradeVeteranSound", 0x80)
                .map(str::to_owned),
            upgrade_elite_sound: audio_visual
                .read_name("UpgradeEliteSound", 0x80)
                .map(str::to_owned),
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
            elite_flash_timer: audio_visual.read_int("EliteFlashTimer", defaults.elite_flash_timer),
            idle_action_frequency: audio_visual
                .read_double("IdleActionFrequency", defaults.idle_action_frequency),
            force_shield_color,
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
            building_damage_sound: audio_visual
                .read_name("BuildingDamageSound", 0x80)
                .map(str::to_owned),
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
            nuke_take_off: general.read_string("NukeTakeOff", "", 0x80),
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
            generic_click_sound: audio_visual
                .read_name("GenericClick", 0x80)
                .map(str::to_owned),
            // Native constructor -1; resolved by the fixed SOUNDMD binder.
            guard_sound: None,
            scold_sound: audio_visual
                .read_name("ScoldSound", 0x80)
                .map(str::to_owned),
            generic_beep_sound: audio_visual
                .read_name("GenericBeep", 0x80)
                .map(str::to_owned),
            gui_checkbox_sound: audio_visual
                .read_name("GUICheckboxSound", 0x80)
                .map(str::to_owned),
            ore_twinkle: general.read_name("OreTwinkle", 0x80).map(str::to_owned),
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
            incoming_message_sound: audio_visual
                .read_name("IncomingMessage", 0x80)
                .map(str::to_owned),
            message_delay_minutes: audio_visual.read_double("MessageDelay", 0.6) as f32,
            speak_delay_minutes: audio_visual.read_double("SpeakDelay", 0.0),
            gui_combo_open_sound: audio_visual
                .read_name("GUIComboOpenSound", 0x80)
                .map(str::to_owned),
            gui_combo_close_sound: audio_visual
                .read_name("GUIComboCloseSound", 0x80)
                .map(str::to_owned),
            // Float fields (`FSTP dword` at `0x0066B8B1`/`0x0066B8DB`).
            direct_rocking_coefficient: sim_from_f32(
                audio_visual.read_float("DirectRockingCoefficient", 1.5),
            ),
            fallback_coefficient: sim_from_f32(audio_visual.read_float("FallBackCoefficient", 0.1)),
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
            impact_water_sound: audio_visual
                .read_name("ImpactWaterSound", 0x80)
                .map(str::to_owned),
            impact_land_sound: audio_visual
                .read_name("ImpactLandSound", 0x80)
                .map(str::to_owned),
            // Constructor -1 until the fixed SOUNDMD catalog resolves it.
            sinking_sound: None,
            construction_sound: None,
            building_repaired_sound: None,
            bomb_ticking_sound,
            bomb_attach_sound,
            warp_in: AnimRef {
                name: parse_anim_name("WarpIn", "WARPIN"),
            },
            warp_out: AnimRef {
                name: parse_anim_name("WarpOut", "WARPOUT"),
            },
            warp_away: AnimRef {
                name: parse_anim_name("WarpAway", "WARPAWAY"),
            },
            chrono_sparkle1: AnimRef {
                name: parse_anim_name("ChronoSparkle1", "CHRONOSK"),
            },
            wake: AnimRef {
                name: parse_anim_name("Wake", "WAKE1"),
            },
            move_flash: AnimRef {
                name: parse_anim_name("MoveFlash", "RING"),
            },
            infantry_death_anims,
            damage_delay_minutes: general.read_double("DamageDelay", 1.0) as f32,
            spy_power_blackout_frames: general.read_int("SpyPowerBlackout", 1000).max(0) as u32,
            damage_fire_types: general
                .read_list("DamageFireTypes", 0x80)
                .map(|tokens| {
                    tokens
                        .into_iter()
                        .map(|name| AnimRef {
                            name: name.to_uppercase(),
                        })
                        .collect()
                })
                .unwrap_or_default(),
            barrel_particle: general.read_name("BarrelParticle", 0x80).map(str::to_owned),
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
            // Parse-time float -> fixed-point ppm (mirrors the IncomeMult parse); the runtime
            // bonus math is all integer. Full precision — no whole-percent quantize.
            // A float field (`FSTP dword` at `0x0066FC70`).
            purifier_bonus_ppm: (f64::from(general.read_float("PurifierBonus", 0.25))
                * INCOME_PPM_SCALE as f64)
                .round() as i64,
            // An IntVector (`0x00475D70`) indexed by difficulty.
            ai_virtual_purifiers: general
                .read_int_list("AIVirtualPurifiers")
                .and_then(|values| values.get(..3)?.try_into().ok())
                .unwrap_or([4, 2, 0]),
            allied_survivor_divisor: general.read_int("AlliedSurvivorDivisor", 500),
            soviet_survivor_divisor: general.read_int("SovietSurvivorDivisor", 250),
            third_survivor_divisor: general.read_int("ThirdSurvivorDivisor", 750),
            allied_crew: general.read_name("AlliedCrew", 0x80).map(str::to_owned),
            soviet_crew: general.read_name("SovietCrew", 0x80).map(str::to_owned),
            third_crew: general.read_name("ThirdCrew", 0x80).map(str::to_owned),
            technician: general.read_name("Technician", 0x80).map(str::to_owned),
            engineer_infantry: general.read_name("Engineer", 0x80).map(str::to_owned),
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
            ship_sinking_weight: sim_from_f32(
                general.read_double("ShipSinkingWeight", defaults.ship_sinking_weight.to_num())
                    as f32,
            ),
            tracked_uphill: SimFixed::from_num(
                general.read_double("TrackedUphill", defaults.tracked_uphill.to_num::<f64>()),
            ),
            tracked_downhill: SimFixed::from_num(
                general.read_double("TrackedDownhill", defaults.tracked_downhill.to_num::<f64>()),
            ),
            wheeled_uphill: SimFixed::from_num(
                general.read_double("WheeledUphill", defaults.wheeled_uphill.to_num::<f64>()),
            ),
            wheeled_downhill: SimFixed::from_num(
                general.read_double("WheeledDownhill", defaults.wheeled_downhill.to_num::<f64>()),
            ),
            // RulesClass's AudioVisual pass stores these ReadDouble values as
            // signed milliunits after the active x87 chop-toward-zero conversion.
            extra_unit_light: (audio_visual
                .read_double("ExtraUnitLight", defaults.extra_unit_light as f64 / 1000.0)
                * 1000.0) as i32,
            extra_infantry_light: (audio_visual.read_double(
                "ExtraInfantryLight",
                defaults.extra_infantry_light as f64 / 1000.0,
            ) * 1000.0) as i32,
            extra_aircraft_light: (audio_visual.read_double(
                "ExtraAircraftLight",
                defaults.extra_aircraft_light as f64 / 1000.0,
            ) * 1000.0) as i32,
            close_enough: general.read_range("CloseEnough", defaults.close_enough),
            stray: general.read_range("Stray", defaults.stray),
            relaxed_stray: general.read_range("RelaxedStray", defaults.relaxed_stray),
            guard_mode_stray: general.read_range("GuardModeStray", defaults.guard_mode_stray),
            // Selected native reader order670DA3,670DCA,670E30.
            repair_percent: general.read_double("RepairPercent", defaults.repair_percent),
            repair_step: general.read_int("RepairStep", defaults.repair_step),
            unit_repair_rate: general.read_double("URepairRate", defaults.unit_repair_rate),
            reload_rate: general.read_double("ReloadRate", defaults.reload_rate),
            path_delay,
            blockage_path_delay_ticks,
            // ReadGeneral670235..670267 uses the same475D70 signed vector
            // reader as the existing Recalc difficulty tables below.
            ai_auto_deploy_frame_delay: general
                .read_int_list("AIAutoDeployFrameDelay")
                .unwrap_or_default(),
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
            iq_repair_sell: iq.read_int("RepairSell", defaults.iq_repair_sell),
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
            lightning_warhead: general.read_string("LightningWarhead", "", 128),
            weather_con_bolt_explosion: general.read_string("WeatherConBoltExplosion", "", 128),
            weapon_nullify_anim: general.read_string("WeaponNullifyAnim", "", 128),
            ambient_change_rate_nonzero,
            ambient_change_interval_frames,
            ambient_change_step,
            iron_curtain_duration: combat_damage.read_int("IronCurtainDuration", 0),
            iron_curtain_invoke_anim: general.read_string("IronCurtainInvokeAnim", "", 0x80),
            chrono_placement_anim: general.read_string("ChronoPlacement", "", 0x80),
            chrono_blast_anim: general.read_string("ChronoBlast", "", 0x80),
            chrono_blast_dest_anim: general.read_string("ChronoBlastDest", "", 0x80),
            dominator_warhead: general.read_string("DominatorWarhead", "", 0x80),
            dominator_first_anim: general.read_string("DominatorFirstAnim", "", 0x80),
            dominator_second_anim: general.read_string("DominatorSecondAnim", "", 0x80),
            dominator_fire_at_percentage: general.read_int("DominatorFireAtPercentage", 50),
            dominator_capture_range: general.read_int("DominatorCaptureRange", 2),
            dominator_damage: general.read_int("DominatorDamage", 50),
            ion_blast_anim: general.read_string("IonBlast", "", 0x80),
            force_shield_radius: general.read_int("ForceShieldRadius", 10),
            force_shield_duration: general.read_int("ForceShieldDuration", 400),
            force_shield_blackout_duration: general.read_int("ForceShieldBlackoutDuration", 800),
            force_shield_fade_sound_time: general.read_int("ForceShieldPlayFadeSoundTime", 50),
            force_shield_invoke_anim: general.read_string("ForceShieldInvokeAnim", "", 0x80),
            psychic_reveal_radius,
            mutate_warhead: special_weapons.read_string("MutateWarhead", "", 0x80),
            mutate_explosion_warhead: special_weapons.read_string(
                "MutateExplosionWarhead",
                "",
                0x80,
            ),
            mutate_explosion: general.read_bool("MutateExplosion", false),
            // The processed RulesClass vector is authoritative; a merged INI
            // cannot reproduce successful-read replacement or factory identity.
            metallic_debris: Vec::new(),
            weather_con_clouds: Vec::new(),
            weather_con_bolts: Vec::new(),
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
#[derive(Debug)]
pub struct RuleSet {
    /// All game objects in registry insertion order, indexed by `TypeHandle`.
    object_list: Vec<ObjectType>,
    /// Uppercase type ID → handle. Uppercase keys give O(1) case-insensitive
    /// resolution, matching the engine's case-insensitive find-or-allocate.
    object_index: HashMap<String, TypeHandle>,
    /// Per-native-registry identity authority. Unlike the broad lookup above,
    /// this retains distinct types when malformed/custom rules list the same
    /// ID in more than one category.
    object_category_index: HashMap<(ObjectCategory, String), TypeHandle>,
    /// Case-insensitive identity of each type in the four native TechnoType
    /// registries to its ordered array index (`+0xDF8`).
    type_array_indices: HashMap<(ObjectCategory, String), i32>,
    /// All weapons indexed by ID (e.g., "105mm" → WeaponType).
    weapons: HashMap<String, WeaponType>,
    /// All warheads indexed by ID (e.g., "AP" → WarheadType).
    warheads: HashMap<String, WarheadType>,
    /// All projectiles indexed by ID (e.g., "InvisibleLow" → ProjectileType).
    projectiles: HashMap<String, ProjectileType>,
    /// Country-level rules indexed by country/house type ID.
    countries: HashMap<String, CountryRules>,
    /// Country identities in native `[Countries]` registration order.
    country_ids: Vec<String>,
    /// Uppercase country identity -> source-order index.
    country_indices: HashMap<String, CountryIdx>,
    /// Side identities in native `[Sides]` registration order, including
    /// sides allocated later by a per-country `Side=` override.
    side_ids: Vec<String>,
    /// Uppercase side identity -> source-order index.
    side_indices: HashMap<String, SideIdx>,
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
    /// Signed, unclamped `[AI] AIBaseSpacing`; constructor default is 1.
    pub ai_base_spacing: i32,
    /// Source-ordered `[General] Shipyard=` BuildingType identities.
    pub shipyard_types: Vec<String>,
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
    /// Rules-driven terrain land-type semantics keyed by TMP land byte.
    pub terrain_rules: TerrainRules,
    /// Native `[Tiberiums]` definitions in GameMD type order.
    pub tiberium_types: TiberiumTypeRegistry,
    /// Terrain object type definitions (TIBTRE*, TREE*, ROCK*, etc.) keyed by
    /// uppercase section name. Distinct from `terrain_rules` (land semantics);
    /// these are per-decoration-object types parsed from `[TerrainTypes]`.
    pub terrain_object_types: HashMap<String, TerrainObjectType>,
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
    /// `[CombatDamage] C4Delay=`. Default `0.03` minutes = 27 ticks @ 15 fps.
    /// Time between SEAL plant claim and detonation. Stored as integer ticks
    /// (not minutes) so the per-tick comparison stays integer/lockstep-safe.
    pub c4_delay_ticks: u32,
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
    pub anim_type_art_read_states: Vec<(String, bool)>,
    /// GPU-independent SHP frame counts used by particle timing. Bound once
    /// from the active assets and ART data.
    effect_assets: crate::rules::effect_asset_catalog::EffectAssetCatalog,
    /// Every building type's construction control from its Buildup SHP,
    /// bound once from the active assets (`bind_building_buildup_assets`).
    buildup_assets: crate::rules::buildup_asset_catalog::BuildupAssetCatalog,
    /// Raw terrain SHP counts used by authoritative TIBTRE animation timing.
    /// Presentation keeps its separate body-frame projection.
    terrain_spawner_assets: crate::rules::terrain_asset_catalog::TerrainSpawnerAssetCatalog,
    /// Complete immutable per-object animation timing catalog. Gameplay reads
    /// this rules-owned resource directly; presentation cannot replace timing
    /// on an individual frame.
    animation_sequences: BTreeMap<String, crate::rules::animation_sequence::SequenceSet>,
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
    /// Build from the active ordered rules sources.
    pub fn from_rules_layers(layers: &RulesLayerStack) -> Result<Self, RulesError> {
        Self::from_processed_rules(&layers.process()?)
    }

    pub(crate) fn from_processed_rules(
        processed: &ProcessedRulesLayers,
    ) -> Result<Self, RulesError> {
        let mut rules = Self::from_projected_ini(processed.ini())?;
        rules.crate_rules = processed.crate_rules().clone();
        rules.powerups = processed.powerups().clone();
        rules.missile_spawn = processed.missile_spawn().clone();
        rules.general.metallic_debris = processed.metallic_debris().to_vec();
        rules.general.weather_con_clouds = processed.weather_con_clouds().to_vec();
        rules.general.weather_con_bolts = processed.weather_con_bolts().to_vec();
        let [amer, ally, sov, yuri] = processed.paradrop_infantry().clone();
        rules.general.amer_paradrop.infantry = amer;
        rules.general.ally_paradrop.infantry = ally;
        rules.general.sov_paradrop.infantry = sov;
        rules.general.yuri_paradrop.infantry = yuri;
        rules.general.anim_to_infantry = processed.anim_to_infantry().to_vec();
        rules.general.default_mirage_disguises = processed.default_mirage_disguises().to_vec();
        rules.bridge_rules.explosions = processed.bridge_explosions().to_vec();
        rules.general.gravity = processed.gravity();
        rules.general.detail = processed.detail();
        rules.general.prism_support = processed.prism_support();
        rules.general.prism_type = processed.prism_type().map(str::to_owned);
        rules.general.prerequisite_proc_alternate =
            processed.prerequisite_proc_alternate().map(str::to_owned);
        rules.prerequisite_lists = processed.prerequisite_lists().clone();
        rules.general.building_types = processed.building_types().clone();
        let (lightning, weather_anim, nullify_anim, splash) = processed.select_anim_rules();
        rules.general.lightning_warhead = lightning.to_owned();
        rules.general.weather_con_bolt_explosion = weather_anim.to_owned();
        rules.general.weapon_nullify_anim = nullify_anim.to_owned();
        rules.combat_damage.splash_list = splash.to_vec();
        for (name, conventional, em_effect, anim_list) in processed.warhead_anim_states() {
            if let Some(warhead) = rules
                .warheads
                .values_mut()
                .find(|wh| wh.id.eq_ignore_ascii_case(name))
            {
                warhead.conventional = conventional;
                warhead.em_effect = em_effect;
                warhead.anim_list = anim_list.to_vec();
            }
        }
        (
            rules.general.missile_rot_var,
            rules.general.safety_altitude,
            rules.general.line_trail_color_override,
        ) = processed.projectile_rule_controls();
        for (name, speed, projectile) in processed.weapon_speeds_and_projectiles() {
            if let Some(weapon) = rules
                .weapons
                .values_mut()
                .find(|weapon| weapon.id.eq_ignore_ascii_case(name))
            {
                weapon.speed = speed;
                weapon.projectile = projectile.map(str::to_owned);
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
                object.gunner_turrets = *gunner_turrets;
            }
        }
        rules.anim_type_art_read_states = processed
            .anim_type_art_read_states()
            .map(|(name, read)| (name.to_owned(), read))
            .collect();
        rules.unit_shp_read_states = processed
            .unit_shp_read_states()
            .map(|(name, state)| (name.to_owned(), *state))
            .collect();
        // Building ctor45DF13 / ReadINI461225..46125D: the process-resident
        // owner retained +EF0 across reached rules passes and exact ART reads.
        // Projection receives its result; RULES Foundation is not an input.
        for (name, foundation_id) in processed.building_foundation_states() {
            if let Some(building) = rules
                .object_list
                .iter_mut()
                .find(|object| object.category == ObjectCategory::Building && object.id == name)
            {
                building.foundation = crate::rules::foundation::FOUNDATION_TABLE
                    [usize::from(foundation_id)]
                .name
                .to_owned();
                building.base_reservation_spacing = building
                    .base_reservation_writer_eligible()
                    .then_some(rules.ai_base_spacing);
            }
        }
        // The registry processor owns native read timing and retained values;
        // the runtime definition receives that result, not another ART read.
        for (name, art) in processed.projectile_art_states() {
            if let Some(projectile) = rules
                .projectiles
                .values_mut()
                .find(|projectile| projectile.id.eq_ignore_ascii_case(name))
            {
                art.apply_to(projectile);
            }
        }
        rules.source_ini_hash = processed.content_hash();
        Ok(rules)
    }

    /// Resolve type sound readers against the startup-selected
    /// SOUNDMD registry. The processed projection retains only passes where
    /// each type existed, including the native current-ID retention order.
    /// Type defaults and Rules+208 start at -1; [AudioVisual] owns the latter.
    pub(crate) fn bind_type_sound_references(
        &mut self,
        ini: &IniFile,
        sounds: &crate::rules::sound_ini::SoundRegistry,
    ) {
        self.general.sinking_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "SinkingSound"));
        self.general.construction_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "Construction"));
        self.general.building_abandoned_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "BuildingAbandonedSound"));
        self.general.building_repaired_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "BuildingRepairedSound"));
        self.general.spy_plane_camera = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "SpyPlaneCamera"));
        self.general.guard_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "GuardSound"));
        for object in &mut self.object_list {
            let section = ini.section(&object.id);
            object.voice_select = section.map_or_else(Vec::new, |section| {
                sounds.read_rules_sound_list(section, "VoiceSelect")
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
            }
            object.sinking_sound =
                section.and_then(|section| sounds.read_rules_reference(section, "SinkingSound"));
            object.voice_sinking =
                section.and_then(|section| sounds.read_rules_reference(section, "VoiceSinking"));
            if object.category == crate::rules::object_type::ObjectCategory::Infantry {
                // Infantry52440B then524447: missing/empty/unregistered names
                // keep the prior signed ID across the existing processed passes.
                object.enter_water_sound = section
                    .and_then(|section| sounds.read_rules_reference(section, "EnterWaterSound"));
                object.leave_water_sound = section
                    .and_then(|section| sounds.read_rules_reference(section, "LeaveWaterSound"));
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

    #[cfg(test)]
    pub(crate) fn from_ini_with_fixed_art_for_test(
        ini: &IniFile,
        art: &IniFile,
    ) -> Result<Self, RulesError> {
        Self::from_processed_rules(&RulesLayerStack::new(ini.clone()).process_with_fixed_art(art)?)
    }

    fn from_projected_ini(ini: &IniFile) -> Result<Self, RulesError> {
        let mut object_list: Vec<ObjectType> = Vec::new();
        let mut object_index: HashMap<String, TypeHandle> = HashMap::new();
        let mut object_category_index: HashMap<(ObjectCategory, String), TypeHandle> =
            HashMap::new();
        let mut type_array_indices: HashMap<(ObjectCategory, String), i32> = HashMap::new();
        let mut infantry_ids: Vec<String> = Vec::new();
        let mut vehicle_ids: Vec<String> = Vec::new();
        let mut aircraft_ids: Vec<String> = Vec::new();
        let mut building_ids: Vec<String> = Vec::new();
        let production: ProductionRules = ProductionRules::from_ini(ini);
        let general: GeneralRules = GeneralRules::from_ini(ini);
        let ai_base_spacing = ini.section_or_empty("AI").read_int("AIBaseSpacing", 1);
        let shipyard_types: Vec<String> = ini
            .section_or_empty("General")
            .read_list("Shipyard", 0x80)
            .map(|tokens| tokens.into_iter().map(str::to_owned).collect())
            .unwrap_or_default();
        // gamemd-derived: `RulesClass__Process` reads type registries first
        // (`ReadBuildingTypes` 0x00668E78), then `RulesClass__ReadAI @
        // 0x00672AE0` (0x00668EC8). All seven BasePlan lists use the exact
        // char[128]/whole-buffer-trim/comma-only path owned by their blocks at
        // 0x00672B14..0x00673058 and 0x0067368C..0x0067375B. None falls back
        // to `[General]`, and individual tokens are not trimmed.
        let parse_planning_list = |section_name: &str, key: &str| -> Vec<String> {
            ini.section_or_empty(section_name)
                .read_list(key, 0x80)
                .unwrap_or_default()
                .into_iter()
                .filter(|token| !is_native_none_type_name(token))
                .map(str::to_owned)
                .collect()
        };
        let build_const_source_tokens = parse_planning_list("AI", "BuildConst");
        let build_power_source_tokens = parse_planning_list("AI", "BuildPower");
        let build_refinery_source_tokens = parse_planning_list("AI", "BuildRefinery");
        let build_barracks_source_tokens = parse_planning_list("AI", "BuildBarracks");
        let build_tech_source_tokens = parse_planning_list("AI", "BuildTech");
        let build_weapons_source_tokens = parse_planning_list("AI", "BuildWeapons");
        let build_radar_source_tokens = parse_planning_list("AI", "BuildRadar");
        let allied_base_defense_source_tokens = parse_planning_list("AI", "AlliedBaseDefenses");
        let soviet_base_defense_source_tokens = parse_planning_list("AI", "SovietBaseDefenses");
        let third_base_defense_source_tokens = parse_planning_list("AI", "ThirdBaseDefenses");
        let concrete_wall_source_tokens = parse_planning_list("AI", "ConcreteWalls");
        let harvester_unit_source_tokens = parse_planning_list("General", "HarvesterUnit");
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
        let tiberium_types = TiberiumTypeRegistry::from_ini(ini);
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
        let radar_event_config: RadarEventConfig = RadarEventConfig::from_ini(ini);
        let country_side_registry = parse_country_side_registry(ini);
        let countries = country_side_registry.rules;
        let color_schemes = crate::rules::color_scheme::parse_color_schemes(ini);
        let house_color_ramps =
            crate::rules::house_colors::HouseColorRamps::from_schemes(&color_schemes);
        let color_add = crate::rules::color_add::ColorAddTable::from_ini(ini);

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

        // Native BuildingTypeClass__FindOrAllocate @ 0x004653C0 and
        // UnitTypeClass__FindOrAllocate @ 0x007480D0 resolve the planning-list
        // tokens case-insensitively while retaining source order and duplicate
        // pointers. Active-retail tokens were allocated by the earlier type
        // registry passes. Unknown custom allocation depends on every later
        // ReadAI list and remains the approved stock-inactive exclusion, so the
        // Rust projection resolves only within the matching registered family.
        let resolve_registered = |source: Vec<String>, category: ObjectCategory| {
            source
                .into_iter()
                .filter(|type_id| {
                    object_category_index.contains_key(&(category, type_id.to_ascii_uppercase()))
                })
                .collect::<Vec<_>>()
        };
        let build_const_types =
            resolve_registered(build_const_source_tokens, ObjectCategory::Building);
        let build_power_types =
            resolve_registered(build_power_source_tokens, ObjectCategory::Building);
        let build_refinery_types =
            resolve_registered(build_refinery_source_tokens, ObjectCategory::Building);
        let build_barracks_types =
            resolve_registered(build_barracks_source_tokens, ObjectCategory::Building);
        let build_tech_types =
            resolve_registered(build_tech_source_tokens, ObjectCategory::Building);
        let build_weapons_types =
            resolve_registered(build_weapons_source_tokens, ObjectCategory::Building);
        let build_radar_types =
            resolve_registered(build_radar_source_tokens, ObjectCategory::Building);
        let allied_base_defense_types =
            resolve_registered(allied_base_defense_source_tokens, ObjectCategory::Building);
        let soviet_base_defense_types =
            resolve_registered(soviet_base_defense_source_tokens, ObjectCategory::Building);
        let third_base_defense_types =
            resolve_registered(third_base_defense_source_tokens, ObjectCategory::Building);
        let concrete_wall_types =
            resolve_registered(concrete_wall_source_tokens, ObjectCategory::Building);
        let harvester_unit_types =
            resolve_registered(harvester_unit_source_tokens, ObjectCategory::Vehicle);
        for type_id in &build_const_types {
            let handle = object_category_index
                .get(&(ObjectCategory::Building, type_id.to_ascii_uppercase()))
                .copied()
                .expect("resolved BuildConst membership remains registered");
            object_list[handle.0 as usize].build_const_eligible = true;
        }

        // Step 2: Collect all weapon IDs referenced by objects.
        let mut weapon_ids = collect_weapon_refs(&object_list);
        if let Some(default_death_weapon) = ini
            .section_or_empty("CombatDamage")
            .read_name("DeathWeapon", 0x80)
        {
            weapon_ids.insert(default_death_weapon.to_string());
        }
        // `SuperWeaponTypeClass::ReadINI` allocates its `WeaponType=` too
        // (`WeaponTypeClass::FindOrAllocate @ 0x00772FA0` at `0x006CEA7D`):
        // the nuclear missile's `NukeCarrier`.
        for sw_id in parse_registry(ini, "SuperWeaponTypes") {
            if let Some(name) = ini
                .section(&sw_id)
                .and_then(|section| section.read_name("WeaponType", 0x80))
            {
                weapon_ids.insert(name.to_string());
            }
        }

        // Step 3: Parse weapon sections.
        let mut weapons: HashMap<String, WeaponType> = HashMap::new();
        // RulesClass allocates every value in [Warheads] before the per-type
        // ReadINI pass. Keep reference-driven allocations too: later rules
        // layers and runtime-specific globals can create warheads outside the
        // explicit registry.
        let mut warhead_ids: HashSet<String> =
            parse_registry(ini, "Warheads").into_iter().collect();

        for weapon_id in &weapon_ids {
            if let Some(section) = ini.section(weapon_id) {
                // Find-or-allocate by the section's canonical header name, so two
                // references differing only in case resolve to a single type (the
                // original engine allocates one type per section, matched
                // case-insensitively). The header name is unique, so the key is
                // deterministic regardless of reference iteration order.
                let canonical = section.name.clone();
                if weapons.contains_key(&canonical) {
                    continue;
                }
                let weapon: WeaponType = WeaponType::from_ini_section(&canonical, section);
                // Also collect warhead references from weapons themselves.
                if let Some(wh) = &weapon.warhead {
                    warhead_ids.insert(wh.clone());
                }
                weapons.insert(canonical, weapon);
            } else {
                log::trace!("Weapon '{}' referenced but has no section", weapon_id);
            }
        }

        // Step 4: Parse warhead sections.
        // The radiation-field warhead is referenced by [Radiation], not by any
        // weapon — pull it into the referenced set explicitly so the periodic
        // radiation damage can resolve it.
        warhead_ids.insert(radiation.site_warhead.clone());
        let mut warheads: HashMap<String, WarheadType> = HashMap::new();
        for warhead_id in &warhead_ids {
            if let Some(section) = ini.section(warhead_id) {
                // Find-or-allocate by canonical section name (see weapons above).
                let canonical = section.name.clone();
                warheads
                    .entry(canonical.clone())
                    .or_insert_with(|| WarheadType::from_ini_section(&canonical, section));
            } else {
                log::trace!("Warhead '{}' referenced but has no section", warhead_id);
            }
        }

        // Step 5: Collect projectile IDs referenced by weapons and parse them.
        let mut projectiles: HashMap<String, ProjectileType> = HashMap::new();
        let mut projectile_ids: HashSet<String> = HashSet::new();
        for weapon in weapons.values() {
            if let Some(ref proj_id) = weapon.projectile {
                projectile_ids.insert(proj_id.clone());
            }
        }
        for proj_id in &projectile_ids {
            if let Some(section) = ini.section(proj_id) {
                // Find-or-allocate by canonical section name (see weapons above).
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

        // ShrapnelWeapon is a ProjectileType-owned weapon reference, so it is
        // discovered only after the first projectile pass. Follow that graph
        // to closure before runtime detonation; otherwise an otherwise valid
        // child weapon/projectile pair silently disappears from live Shrapnel.
        loop {
            let mut nested_weapon_ids: Vec<String> = projectiles
                .values()
                .filter_map(|projectile| projectile.shrapnel_weapon.clone())
                .filter(|weapon_id| {
                    !weapons
                        .keys()
                        .any(|existing| existing.eq_ignore_ascii_case(weapon_id))
                })
                .collect();
            nested_weapon_ids.sort_by_key(|id| id.to_ascii_uppercase());
            nested_weapon_ids.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
            if nested_weapon_ids.is_empty() {
                break;
            }

            let mut nested_projectile_ids = Vec::new();
            let mut inserted_weapon = false;
            for weapon_id in nested_weapon_ids {
                let Some(section) = ini.section(&weapon_id) else {
                    log::trace!("Shrapnel weapon '{}' has no section", weapon_id);
                    continue;
                };
                let canonical = section.name.clone();
                let weapon = WeaponType::from_ini_section(&canonical, section);
                if let Some(warhead) = &weapon.warhead {
                    warhead_ids.insert(warhead.clone());
                }
                if let Some(projectile) = &weapon.projectile {
                    nested_projectile_ids.push(projectile.clone());
                }
                weapons.insert(canonical, weapon);
                inserted_weapon = true;
            }

            if !inserted_weapon {
                break;
            }

            for projectile_id in nested_projectile_ids {
                if projectiles
                    .keys()
                    .any(|existing| existing.eq_ignore_ascii_case(&projectile_id))
                {
                    continue;
                }
                let Some(section) = ini.section(&projectile_id) else {
                    log::trace!("Shrapnel projectile '{}' has no section", projectile_id);
                    continue;
                };
                let canonical = section.name.clone();
                projectiles.insert(
                    canonical.clone(),
                    ProjectileType::from_ini_section(&canonical, section, None),
                );
            }
        }

        // The initial warhead pass precedes ProjectileType-owned weapon
        // discovery. Allocate any newly referenced child warheads now.
        for warhead_id in &warhead_ids {
            if warheads
                .keys()
                .any(|existing| existing.eq_ignore_ascii_case(warhead_id))
            {
                continue;
            }
            if let Some(section) = ini.section(warhead_id) {
                let canonical = section.name.clone();
                warheads.insert(
                    canonical.clone(),
                    WarheadType::from_ini_section(&canonical, section),
                );
            }
        }

        // Step 6: Build factory lookup map from Factory= keys on all objects.
        let factory_map: HashMap<String, FactoryType> = object_list
            .iter()
            .filter_map(|obj| obj.factory.map(|ft| (obj.id.to_ascii_uppercase(), ft)))
            .collect();
        log::info!("Factory map: {} entries", factory_map.len());

        // Step 7: Parse superweapon type registry.
        let mut super_weapons: HashMap<String, SuperWeaponType> = HashMap::new();
        let sw_ids: Vec<String> = parse_registry(ini, "SuperWeaponTypes");
        let super_weapon_order: Vec<String> = sw_ids.clone();
        for sw_id in &sw_ids {
            if let Some(section) = ini.section(sw_id) {
                if let Some(sw) = SuperWeaponType::from_ini_section(sw_id, section) {
                    super_weapons.insert(sw_id.clone(), sw);
                } else {
                    log::warn!("SuperWeapon '{}' has unknown Type=, skipping", sw_id);
                }
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

        // Parse [CombatDamage] bridge-warhead names (IonCannonWarhead, C4Warhead).
        let bridge_warheads = ini
            .section("CombatDamage")
            .map(crate::rules::bridge_warheads::BridgeWarheads::from_ini_section)
            .unwrap_or_default();

        let mind_control = crate::rules::mind_control_rules::MindControlRules::from_ini(ini);

        // [General] rocket blocks + [CombatDamage] missile warheads; the
        // per-pass result replaces this in `from_processed_rules`.
        let missile_spawn = crate::rules::missile_spawn::MissileSpawnRules::from_ini(ini);

        // [CombatDamage] C4Delay = minutes (double). Default 0.03 = 27 ticks @ 15 fps.
        // Stored as integer ticks for lockstep-safe per-tick comparison.
        const SIM_TICKS_PER_SECOND: u32 = crate::util::fixed_math::RA2_LOGIC_FRAMES_PER_SECOND;
        let c4_delay_ticks: u32 = (ini
            .section_or_empty("CombatDamage")
            .read_double("C4Delay", 0.03)
            * 60.0
            * SIM_TICKS_PER_SECOND as f64)
            .round() as u32; // 0.03 × 60 × 15 = 27

        // Per-mission behaviour table from the [<MissionName>] sections.
        let mission_control = MissionControl::from_ini(ini);

        // Parse [TerrainTypes] registry → per-type sections (TIBTRE01, TREE01, etc.).
        let mut terrain_object_types: HashMap<String, TerrainObjectType> = HashMap::new();
        let terrain_names: Vec<String> = parse_registry(ini, "TerrainTypes");
        for name in &terrain_names {
            if let Some(section) = ini.section(name) {
                terrain_object_types.insert(
                    name.to_ascii_uppercase(),
                    TerrainObjectType::from_ini_section_with_tree_strength(
                        name,
                        section,
                        general.tree_strength,
                    ),
                );
            }
        }
        log::info!(
            "TerrainTypes: {} loaded ({} with SpawnsTiberium=yes)",
            terrain_object_types.len(),
            terrain_object_types
                .values()
                .filter(|t| t.spawns_tiberium)
                .count(),
        );

        // Step 8: Two-pass parse for [Particles] and [ParticleSystems].
        // Cross-references (NextParticle, HoldsWhat) are resolved in pass 2 so
        // that INI ordering does not matter.
        let (particle_types, particle_types_by_name) = parse_particle_types(ini);
        let (particle_system_types, particle_system_types_by_name) =
            parse_particle_system_types(ini, &particle_types_by_name);
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
            object_category_index,
            type_array_indices,
            weapons,
            warheads,
            projectiles,
            countries,
            country_ids: country_side_registry.country_ids,
            country_indices: country_side_registry.country_indices,
            side_ids: country_side_registry.side_ids,
            side_indices: country_side_registry.side_indices,
            country_sides: country_side_registry.country_sides,
            color_schemes,
            house_color_ramps,
            color_add,
            production,
            general,
            ai_base_spacing,
            shipyard_types,
            build_const_types,
            build_power_types,
            build_refinery_types,
            build_barracks_types,
            build_tech_types,
            build_weapons_types,
            build_radar_types,
            allied_base_defense_types,
            soviet_base_defense_types,
            third_base_defense_types,
            concrete_wall_types,
            harvester_unit_types,
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
            terrain_rules,
            tiberium_types,
            terrain_object_types,
            bridge_rules,
            crate_rules,
            powerups,
            garrison_rules,
            allied_wall_transparency,
            elevation_model,
            radiation,
            radar_event_config,
            super_weapons,
            super_weapon_order,
            combat_damage,
            bridge_warheads,
            mind_control,
            missile_spawn,
            c4_delay_ticks,
            particle_types,
            particle_system_types,
            particle_system_types_by_name,
            voxel_anim_types,
            voxel_anim_types_by_name,
            smudge_types: SmudgeTypeRegistry::from_rules_ini(ini),
            art_registry: crate::rules::art_data::ArtRegistry::empty(),
            anim_type_art_read_states: Vec::new(),
            anim_type_names: ini
                .section("Animations")
                .into_iter()
                .flat_map(|section| section.registry_ids())
                .map(|name| name.to_ascii_uppercase())
                .filter(|name| !name.is_empty())
                .collect(),
            effect_assets: crate::rules::effect_asset_catalog::EffectAssetCatalog::default(),
            buildup_assets: crate::rules::buildup_asset_catalog::BuildupAssetCatalog::default(),
            terrain_spawner_assets:
                crate::rules::terrain_asset_catalog::TerrainSpawnerAssetCatalog::default(),
            animation_sequences: BTreeMap::new(),
            unit_shp_read_states: BTreeMap::new(),
            mission_control,
            // Single-source callers hash their one parsed INI. Production
            // ordered-stack callers replace this with the boundary-sensitive
            // RulesLayerStack hash in `from_processed_rules`.
            source_ini_hash: ini.content_hash(),
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
    fn lookup_ci<'a, T>(map: &'a HashMap<String, T>, id: &str) -> Option<&'a T> {
        map.get(id).or_else(|| {
            map.iter()
                .find_map(|(key, value)| key.eq_ignore_ascii_case(id).then_some(value))
        })
    }

    /// Resolve a type name to its handle, case-insensitively (engine parity).
    pub fn type_handle(&self, id: &str) -> Option<TypeHandle> {
        self.object_index.get(&id.to_ascii_uppercase()).copied()
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
    pub(crate) fn base_defense_types(&self, side_index: u8) -> &[String] {
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
    pub(crate) fn type_array_index(&self, category: ObjectCategory, id: &str) -> Option<i32> {
        self.type_array_indices
            .get(&(category, id.to_ascii_uppercase()))
            .copied()
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

    /// Look up a weapon by ID (case-insensitive, gamemd parity).
    pub fn weapon(&self, id: &str) -> Option<&WeaponType> {
        Self::lookup_ci(&self.weapons, id)
    }

    /// Look up a warhead by ID (case-insensitive, gamemd parity).
    pub fn warhead(&self, id: &str) -> Option<&WarheadType> {
        Self::lookup_ci(&self.warheads, id)
    }

    /// Look up a projectile by ID (case-insensitive, gamemd parity).
    pub fn projectile(&self, id: &str) -> Option<&ProjectileType> {
        Self::lookup_ci(&self.projectiles, id)
    }

    /// Registered projectile types, including their retained animation references.
    pub(crate) fn projectiles_iter(&self) -> impl Iterator<Item = &ProjectileType> {
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

    /// Compatibility identity for processed rules plus the resolved animation,
    /// effect-frame, terrain-spawner frame, smudge-selection and projectile
    /// launch ART inputs bound to this ruleset.
    /// Other asset-derived simulation inputs are added by later ownership
    /// slices and are not claimed by this hash yet.
    pub fn simulation_config_hash(&self) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        b"rules-simulation-config-v15".hash(&mut hasher);
        self.source_ini_hash.hash(&mut hasher);
        // Process-resident Gravity and Weapon postpass results can differ for
        // identical current source stacks because earlier passes retained them.
        self.general.gravity.hash(&mut hasher);
        self.general.detail.hash(&mut hasher);
        self.general.prism_support.hash(&mut hasher);
        self.general.default_mirage_disguises.hash(&mut hasher);
        self.general.infantry_blink_disguise_time.hash(&mut hasher);
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
        self.general.missile_rot_var.to_bits().hash(&mut hasher);
        self.general.safety_altitude.hash(&mut hasher);
        self.general.line_trail_color_override.hash(&mut hasher);
        self.general.lightning_warhead.hash(&mut hasher);
        self.general.weather_con_bolt_explosion.hash(&mut hasher);
        self.general.weapon_nullify_anim.hash(&mut hasher);
        self.combat_damage.splash_list.hash(&mut hasher);
        self.warheads
            .iter()
            .map(|(name, wh)| {
                (
                    name.to_ascii_uppercase(),
                    (wh.conventional, wh.em_effect, &wh.anim_list),
                )
            })
            .collect::<BTreeMap<_, _>>()
            .hash(&mut hasher);
        self.weapons
            .iter()
            .map(|(id, weapon)| {
                (
                    id.to_ascii_uppercase(),
                    (
                        (weapon.speed, &weapon.projectile),
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
            })
            .collect::<BTreeMap<_, _>>()
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
        self.buildup_assets.hash(&mut hasher);
        self.terrain_spawner_assets.hash(&mut hasher);
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
        // These effective ART values feed ordinary FireAt after load. Hash
        // consumed values in canonical type order, not ART insertion order,
        // authored-key presence, or unrelated presentation metadata.
        b"art-projectile-config-v3".hash(&mut hasher);
        self.projectiles
            .iter()
            .map(|(id, p)| {
                (
                    id.to_ascii_uppercase(),
                    (
                        (
                            &p.image,
                            &p.image_load,
                            p.voxel,
                            p.theater,
                            p.new_theater,
                            p.inviso,
                        ),
                        (p.rotates, p.flat, p.anim_palette),
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
            })
            .collect::<BTreeMap<_, _>>()
            .hash(&mut hasher);
        // The fixed SOUNDMD registry changes which names resolve even when
        // Rules text is identical. Empty versus nonempty vectors gates Main
        // draws; list order/duplicates select the sound. Hash effective lists
        // in canonical type order, independently of registry insertion order.
        b"resolved-type-sound-lists-v1".hash(&mut hasher);
        self.object_list
            .iter()
            .map(|object| {
                (
                    (object.category, object.id.to_ascii_uppercase()),
                    (
                        &object.voice_select,
                        &object.move_sound,
                        &object.voice_special_attack,
                        &object.voice_feedback,
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
                        object.gunner_turrets,
                    ),
                )
            })
            .collect::<BTreeMap<_, _>>()
            .hash(&mut hasher);
        // Actual selected FLH records and turret pivot, in canonical type order.
        b"art-weapon-flh-v1".hash(&mut hasher);
        self.object_list
            .iter()
            .map(|object| {
                let art = self
                    .art_registry
                    .get(&object.image)
                    .or_else(|| self.art_registry.get(&object.id));
                // FireAt uses zero FLH/pivot when metadata is absent. Hash
                // that same effective value, not the presence of an ART section.
                let slots = (0..crate::rules::object_type::WEAPON_SLOT_COUNT)
                    .map(|index| {
                        let elite = object
                            .elite_weapon_list
                            .get(index)
                            .is_some_and(Option::is_some);
                        art.map_or_else(Default::default, |art| {
                            (
                                art.weapon_flh(
                                    object.turret_count,
                                    object.weapon_count,
                                    index as i32,
                                    false,
                                ),
                                art.weapon_flh(
                                    object.turret_count,
                                    object.weapon_count,
                                    index as i32,
                                    elite,
                                ),
                            )
                        })
                    })
                    .collect::<Vec<_>>();
                let consumed = (art.map_or(0, |art| art.turret_offset), slots);
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
                .map(|entry| entry.building_anims.iter().collect::<Vec<_>>())
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

    /// Whether a country/house type has `MultiplayPassive=true`.
    pub fn country_multiplay_passive(&self, id: &str) -> bool {
        self.country_rules(id)
            .is_some_and(|country| country.multiplay_passive)
    }

    /// The country's `ROF=` (HouseType `+0xE8`); 1.0 for an unknown country.
    pub fn country_rof(&self, id: &str) -> f64 {
        self.country_rules(id).map_or(1.0, |country| country.rof)
    }

    /// Whether a country/house type may claim nearby map walls. Native default is true.
    pub fn country_wall_owner(&self, id: &str) -> bool {
        self.country_rules(id)
            .map_or(true, |country| country.wall_owner)
    }

    /// A country's `IncomeMult` as parts-per-million (default `INCOME_PPM_SCALE` = 1.0×).
    /// Unknown/absent country -> the neutral multiplier (no income change).
    pub fn country_income_ppm(&self, id: &str) -> i64 {
        self.country_rules(id)
            .map_or(INCOME_PPM_SCALE, |country| country.income_ppm)
    }

    /// A country's `Cost*Mult=` floats (`HouseType+0x114..+0x124`), the
    /// constructor's 1.0 for an unknown country.
    pub(crate) fn country_cost_mults(&self, id: &str) -> [NativeF32Bits; 5] {
        self.country_rules(id)
            .map_or([NativeF32Bits::ONE; 5], |country| country.cost_mults)
    }

    /// A country's `Speed*Mult=` floats (`HouseType+0x128..+0x130`), the
    /// constructor's 1.0 for an unknown country.
    pub(crate) fn country_speed_mults(&self, id: &str) -> [NativeF32Bits; 3] {
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

    /// Resolve a country name to its stable `[Countries]` registration index.
    pub fn country_index(&self, id: &str) -> Option<CountryIdx> {
        self.country_indices.get(&id.to_ascii_uppercase()).copied()
    }

    /// Resolve a trigger owner token to the canonical HouseType registration.
    ///
    /// gamemd-derived: `TriggerTypeClass::Read` resolves token 1 through
    /// `HouseTypeClass__FindIndexOfName @ 0x005117D0`. The source-order scan
    /// checks each HouseType's `Name=` alias (`+0x64`) before its registry ID
    /// (`+0x24`). Native `<none>` selects the first registered HouseType.
    pub fn trigger_house_type_index(&self, owner: &str) -> Option<CountryIdx> {
        if owner.eq_ignore_ascii_case("<none>") {
            return (!self.country_ids.is_empty()).then_some(CountryIdx(0));
        }
        self.country_ids.iter().enumerate().find_map(|(index, id)| {
            let alias_matches = self
                .countries
                .get(id)
                .and_then(|country| country.name.as_deref())
                .is_some_and(|name| name.eq_ignore_ascii_case(owner));
            (alias_matches || id.eq_ignore_ascii_case(owner)).then(|| {
                CountryIdx(u16::try_from(index).expect("[Countries] exceeds u16 identity space"))
            })
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

    /// Resolve a side index back to its source spelling.
    pub fn side_name(&self, index: SideIdx) -> Option<&str> {
        self.side_ids.get(index.0 as usize).map(String::as_str)
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

    /// Look up a superweapon type by ID (case-insensitive, gamemd parity).
    pub fn super_weapon(&self, id: &str) -> Option<&SuperWeaponType> {
        Self::lookup_ci(&self.super_weapons, id)
    }

    /// A superweapon type's `SuperWeaponTypeClass` array index (its
    /// `[SuperWeaponTypes]` position, case-insensitive), the value
    /// `SuperWeapon=` stores (BuildingType `+0x16F0`).
    pub fn super_weapon_index(&self, id: &str) -> Option<usize> {
        self.super_weapon_order
            .iter()
            .position(|name| name.eq_ignore_ascii_case(id))
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

    /// Resolve a particle type name to its ID (case-insensitive).
    #[cfg(test)]
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

    /// Number of `[VoxelAnims]` types loaded.
    #[cfg(test)]
    pub fn voxel_anim_type_count(&self) -> usize {
        self.voxel_anim_types.len()
    }

    /// Number of particle types loaded from `[Particles]`.
    #[cfg(test)]
    pub fn particle_type_count(&self) -> usize {
        self.particle_types.len()
    }

    /// Number of particle system types loaded from `[ParticleSystems]`.
    #[cfg(test)]
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

    /// Whether a structure type is marked as a refinery in rules.ini.
    pub fn is_refinery_type(&self, structure_id: &str) -> bool {
        self.object(structure_id).is_some_and(|obj| obj.refinery)
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
        self.project_art_data();
    }

    fn project_art_data(&mut self) {
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
            // +EF0 is supplied by the sole pass owner in from_processed_rules.
            // Repeating461225 here changes late constructor-only types and
            // loses retained state when installation ART differs from the pass.
            if let Some(entry) = entry {
                obj.to_overlay = entry.to_overlay.clone();
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
        let mut terrain_foundations_patched: u32 = 0;
        for terrain in self.terrain_object_types.values_mut() {
            if let Some(entry) = art.get(&terrain.name) {
                if let Some(foundation) = entry.foundation {
                    terrain.merge_art_foundation(
                        crate::rules::foundation::FOUNDATION_TABLE[usize::from(foundation)].name,
                    );
                    terrain_foundations_patched += 1;
                }
            }
        }
        log::trace!(
            "Merged infantry art metadata: {} Crawls flags ({} infantry checked)",
            crawls_patched,
            infantry_checked,
        );
        log::trace!(
            "Merged terrain art metadata: {} Foundation values",
            terrain_foundations_patched,
        );
        self.rebuild_animation_sequences(None);
    }

    /// Read the same bound ART owner used during construction and gameplay.
    pub fn art(&self) -> &crate::rules::art_data::ArtRegistry {
        &self.art_registry
    }

    pub(crate) fn bind_scheduler_anim_assets(
        &mut self,
        roots: &[String],
        assets: &crate::assets::asset_manager::AssetManager,
        theater_ext: &str,
        theater_name: &str,
    ) -> Result<(), crate::rules::art_data::AnimAssetBindError> {
        self.art_registry
            .bind_scheduler_anim_assets(roots, assets, theater_ext, theater_name)
    }

    pub(crate) fn bind_anim_class_assets(
        &mut self,
        roots: &[String],
        assets: &crate::assets::asset_manager::AssetManager,
        theater_ext: &str,
        theater_name: &str,
    ) -> usize {
        self.art_registry
            .bind_anim_class_assets(roots, assets, theater_ext, theater_name)
    }

    pub(crate) fn populate_anim_frame_dims(
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
    pub(crate) fn bind_authored_load_anim(
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
    #[cfg(test)]
    pub(crate) fn install_art_fixture(&mut self, art: crate::rules::art_data::ArtRegistry) {
        self.art_registry = art;
        self.project_art_data();
    }

    /// Raw synthetic fixture replacement: no native admission or field projection.
    #[cfg(test)]
    pub(crate) fn replace_art_registry_for_test(
        &mut self,
        art: crate::rules::art_data::ArtRegistry,
    ) {
        self.art_registry = art;
    }

    #[cfg(test)]
    pub(crate) fn bind_anim_frame_count_for_test(&mut self, name: &str, raw_count: i32) {
        self.art_registry
            .bind_anim_frame_count_for_test(name, raw_count);
    }

    /// The SHP header height `+4` asset binding would read for `name`.
    #[cfg(test)]
    pub(crate) fn bind_anim_shp_height_for_test(&mut self, name: &str, raw_height: i32) {
        self.art_registry
            .bind_anim_shp_height_for_test(name, raw_height);
    }

    /// A weapon's stored speed (after Process's postpass), as a native
    /// fixture writes it.
    #[cfg(test)]
    pub(crate) fn set_weapon_speed_for_test(&mut self, id: &str, speed: i32) {
        if let Some(weapon) = self
            .weapons
            .values_mut()
            .find(|weapon| weapon.id.eq_ignore_ascii_case(id))
        {
            weapon.speed = speed;
        }
    }

    #[cfg(test)]
    pub(crate) fn art_entry_mut_for_test(
        &mut self,
        name: &str,
    ) -> Option<&mut crate::rules::art_data::ArtEntry> {
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
    }

    /// Resolve raw terrain SHP frame counts for authoritative TIBTRE midpoint
    /// timing without consulting a renderer atlas.
    pub fn bind_terrain_spawner_assets(
        &mut self,
        rules_ini: &crate::rules::ini_parser::IniFile,
        asset_manager: &crate::assets::asset_manager::AssetManager,
        theater_ext: &str,
        theater_name: &str,
    ) {
        self.terrain_spawner_assets =
            crate::rules::terrain_asset_catalog::TerrainSpawnerAssetCatalog::bind(
                self,
                rules_ini,
                asset_manager,
                theater_ext,
                theater_name,
            );
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
    #[cfg(test)]
    pub(crate) fn set_buildup_control_for_test(&mut self, type_id: &str, control: [i32; 3]) {
        self.buildup_assets.insert_for_test(type_id, control);
    }

    /// Consumer-visible SHP frame count for a particle image. Lookup is
    /// case-insensitive and does not intern names.
    pub fn effect_frame_count(&self, name: &str) -> Option<u16> {
        self.effect_assets.effect_frame_count(name)
    }

    /// Raw SHP header count used by `TerrainClass::AI` midpoint timing.
    pub fn terrain_spawner_frame_count(&self, name: &str) -> Option<u16> {
        self.terrain_spawner_assets.frame_count(name)
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
    }

    pub(crate) fn animation_sequences(
        &self,
    ) -> &BTreeMap<String, crate::rules::animation_sequence::SequenceSet> {
        &self.animation_sequences
    }

    pub(crate) fn animation_sequence(
        &self,
        type_id: &str,
    ) -> Option<&crate::rules::animation_sequence::SequenceSet> {
        let canonical = self.object(type_id)?.id.as_str();
        self.animation_sequences.get(canonical)
    }

    pub(crate) fn unit_shp_read_state(
        &self,
        type_id: &str,
    ) -> Option<&crate::rules::shp_vehicle_sequence::UnitShpReadState> {
        let object = self.object(type_id)?;
        self.unit_shp_read_states.get(&object.id)
    }

    #[cfg(test)]
    pub(crate) fn replace_animation_sequences_for_test(
        &mut self,
        animation_sequences: BTreeMap<String, crate::rules::animation_sequence::SequenceSet>,
    ) {
        self.animation_sequences = animation_sequences;
    }

    #[cfg(test)]
    pub(crate) fn set_effect_frame_count_for_test(&mut self, name: &str, raw: u16, available: u16) {
        self.effect_assets.set_for_test(name, raw, available);
    }

    #[cfg(test)]
    pub(crate) fn set_terrain_spawner_frame_count_for_test(
        &mut self,
        name: &str,
        frame_count: u16,
    ) {
        self.terrain_spawner_assets.set_for_test(name, frame_count);
    }

    /// Total number of game objects across all categories.
    pub fn object_count(&self) -> usize {
        self.object_list.len()
    }

    /// Total number of weapons.
    pub fn weapon_count(&self) -> usize {
        self.weapons.len()
    }

    /// Total number of warheads.
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
    #[cfg(test)]
    pub fn projectile_count(&self) -> usize {
        self.projectiles.len()
    }

    /// Iterate over all game objects in the registry.
    pub fn all_objects(&self) -> impl Iterator<Item = &ObjectType> {
        self.object_list.iter()
    }
}

/// Parse a type registry section (e.g., [InfantryTypes]) into a list of IDs.
///
/// Registry sections use numbered keys: `0=E1`, `1=E2`, ...
/// Returns empty Vec if the section doesn't exist.
fn parse_registry(ini: &IniFile, section_name: &str) -> Vec<String> {
    match ini.section(section_name) {
        Some(section) => {
            let raw: Vec<String> = section
                .keys()
                .map(|key| section.read_string(key, "", 32))
                .filter(|value| !value.is_empty())
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

struct ParsedCountrySideRegistry {
    rules: HashMap<String, CountryRules>,
    country_ids: Vec<String>,
    country_indices: HashMap<String, CountryIdx>,
    side_ids: Vec<String>,
    side_indices: HashMap<String, SideIdx>,
    country_sides: Vec<Option<SideIdx>>,
}

fn parse_country_side_registry(ini: &IniFile) -> ParsedCountrySideRegistry {
    let mut country_ids = parse_registry(ini, "Countries");
    let mut country_indices: HashMap<String, CountryIdx> = country_ids
        .iter()
        .enumerate()
        .map(|(index, id)| {
            let index = u16::try_from(index).expect("[Countries] exceeds u16 identity space");
            (id.to_ascii_uppercase(), CountryIdx(index))
        })
        .collect();
    let mut side_ids = Vec::new();
    let mut side_indices = HashMap::new();
    let mut country_sides = vec![None; country_ids.len()];

    if let Some(sides) = ini.section("Sides") {
        for side_name in sides.keys() {
            let side = find_or_allocate_side(side_name, &mut side_ids, &mut side_indices);
            // `0x004767C0`: ReadString into `char[128]`, then `strtok(",")`.
            if let Some(members) = sides.read_list(side_name, 0x80) {
                for member in members {
                    let country = find_or_allocate_country(
                        member,
                        &mut country_ids,
                        &mut country_indices,
                        &mut country_sides,
                    );
                    country_sides[country.0 as usize] = Some(side);
                }
            }
        }
    }

    let mut rules = HashMap::with_capacity(country_ids.len());
    for id in &country_ids {
        if let Some(section) = ini.section(id) {
            rules.insert(id.clone(), CountryRules::from_ini_section(section));
        }
    }

    // HouseTypeClass::ReadINI runs after the `[Sides]` registration pass. Its
    // `Side=` value therefore wins and can find-or-allocate a new side.
    for (country_index, country_id) in country_ids.iter().enumerate() {
        let Some(section) = ini.section(country_id) else {
            continue;
        };
        let side_name = section.read_string("Side", "", 32);
        if side_name.is_empty() {
            continue;
        }
        let side = find_or_allocate_side(&side_name, &mut side_ids, &mut side_indices);
        country_sides[country_index] = Some(side);
    }

    ParsedCountrySideRegistry {
        rules,
        country_ids,
        country_indices,
        side_ids,
        side_indices,
        country_sides,
    }
}

fn find_or_allocate_country(
    country_name: &str,
    country_ids: &mut Vec<String>,
    country_indices: &mut HashMap<String, CountryIdx>,
    country_sides: &mut Vec<Option<SideIdx>>,
) -> CountryIdx {
    let key = country_name.to_ascii_uppercase();
    if let Some(index) = country_indices.get(&key) {
        return *index;
    }
    let index = u16::try_from(country_ids.len()).expect("[Countries] exceeds u16 identity space");
    let index = CountryIdx(index);
    country_ids.push(country_name.to_string());
    country_indices.insert(key, index);
    country_sides.push(None);
    index
}

fn find_or_allocate_side(
    side_name: &str,
    side_ids: &mut Vec<String>,
    side_indices: &mut HashMap<String, SideIdx>,
) -> SideIdx {
    let key = side_name.to_ascii_uppercase();
    if let Some(index) = side_indices.get(&key) {
        return *index;
    }
    let index = u8::try_from(side_ids.len()).expect("[Sides] exceeds u8 identity space");
    let index = SideIdx(index);
    side_ids.push(side_name.to_string());
    side_indices.insert(key, index);
    index
}

/// Collect all weapon IDs referenced by objects (deduplicated).
fn collect_weapon_refs(objects: &[ObjectType]) -> HashSet<String> {
    let mut weapon_ids: HashSet<String> = HashSet::new();

    for obj in objects.iter() {
        if let Some(ref w) = obj.occupy_weapon {
            weapon_ids.insert(w.clone());
        }
        if let Some(ref w) = obj.elite_occupy_weapon {
            weapon_ids.insert(w.clone());
        }
        if let Some(ref w) = obj.death_weapon {
            weapon_ids.insert(w.clone());
        }
        weapon_ids.extend(obj.weapon_list.iter().flatten().cloned());
        weapon_ids.extend(obj.elite_weapon_list.iter().flatten().cloned());
    }

    weapon_ids
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

/// Two-pass parse of `[ParticleSystems]`: collect `Pending` entries and
/// resolve each `HoldsWhat=` name against the already-built particle-type
/// name map. Missing references log a warning and stay `None`.
/// `[VoxelAnims]` registry: the flying-debris types a death throws.
///
/// Registry order is the id order, exactly as for particles and particle
/// systems, because `DebrisTypes=` and `Spawns=` resolve by name and the ids
/// are hashed.
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

fn parse_particle_system_types(
    ini: &IniFile,
    p_by_name: &HashMap<String, ParticleTypeId>,
) -> (
    Vec<ParticleSystemType>,
    HashMap<String, ParticleSystemTypeId>,
) {
    let ids: Vec<String> = parse_registry(ini, "ParticleSystems");
    if ids.is_empty() {
        return (Vec::new(), HashMap::new());
    }

    let mut pending: Vec<PendingParticleSystemType> = Vec::with_capacity(ids.len());
    for id in &ids {
        if let Some(section) = ini.section(id) {
            pending.push(ParticleSystemType::from_ini_section_pending(id, section));
        } else {
            log::trace!(
                "ParticleSystemType '{}' listed in [ParticleSystems] but has no section",
                id
            );
        }
    }

    let mut by_name: HashMap<String, ParticleSystemTypeId> = HashMap::with_capacity(pending.len());
    for (idx, pst) in pending.iter().enumerate() {
        by_name.insert(
            pst.partial.name.to_ascii_uppercase(),
            ParticleSystemTypeId(idx as u32),
        );
    }

    let particle_system_types: Vec<ParticleSystemType> = pending
        .into_iter()
        .map(|pst| {
            let mut partial = pst.partial;
            if let Some(ref holds_name) = pst.holds_what_name {
                let key = holds_name.to_ascii_uppercase();
                match p_by_name.get(&key) {
                    Some(&id) => partial.holds_what = Some(id),
                    None => {
                        log::warn!(
                            "ParticleSystemType '{}': HoldsWhat='{}' references unknown particle, leaving unresolved",
                            partial.name,
                            holds_name
                        );
                    }
                }
            }
            partial
        })
        .collect();

    log::info!("ParticleSystems: {} loaded", particle_system_types.len());
    (particle_system_types, by_name)
}

#[cfg(test)]
mod tests {
    fn detail_native() -> serde_json::Value {
        serde_json::from_str(crate::test_fixture::text(
            "tools/rules_oracle/weapon_laser.json",
        ))
        .unwrap()
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
                retained =
                    process_native_rules_cold_start(retained, &pass, &IniFile::empty(), None)
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
        assert_eq!(rules.voxel_anim_type_count(), 3);
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
        assert_eq!(general.scold_sound.as_deref(), Some("MenuScold"));
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

    #[test]
    fn cloak_global_defaults_and_native_minute_conversion_parse() {
        let defaults = GeneralRules::default();
        assert_eq!(defaults.cloaking_stages, 9);
        assert_eq!(defaults.cloak_delay_frames, 18);
        assert_eq!(defaults.cloak_sound, None);
        let parsed = GeneralRules::from_ini(&IniFile::from_str(
            "[General]\nCloakingStages=13\nCloakDelay=.031\n\
             [AudioVisual]\nCloakSound=NavalUnitEmerge\n",
        ));
        assert_eq!(parsed.cloaking_stages, 13);
        assert_eq!(parsed.cloak_delay_frames, 27);
        assert_eq!(parsed.cloak_sound.as_deref(), Some("NavalUnitEmerge"));
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
        let Some(ini) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
            return;
        };
        let rules = RuleSet::from_ini(&ini).expect("retail rules parse");
        assert!(!rules.countries.is_empty());
        for (name, country) in &rules.countries {
            assert_eq!(country.speed_mults, [NativeF32Bits::ONE; 3], "{name}");
        }
    }

    #[test]
    fn country_income_mult_parses_and_defaults() {
        assert_eq!(
            CountryRules::default().income_ppm,
            INCOME_PPM_SCALE,
            "default IncomeMult is 1.0x (a derived Default would zero it and wipe income)"
        );

        let src = format!(
            "{}\n[Countries]\n0=Americans\n1=Russia\n[Americans]\nIncomeMult=1.2\n[Russia]\nFixtureOnly=1\n",
            make_test_rules()
        );
        let ini = IniFile::from_str(&src);
        let rules = RuleSet::from_ini(&ini).expect("Should parse");
        assert_eq!(rules.country_income_ppm("Americans"), 1_200_000);
        assert_eq!(rules.country_income_ppm("Russia"), INCOME_PPM_SCALE);
        assert_eq!(rules.country_income_ppm("Nonexistent"), INCOME_PPM_SCALE);
        assert_eq!(rules.country_income_ppm("americans"), 1_200_000); // case-insensitive
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

    #[test]
    fn trigger_house_type_owner_uses_alias_then_id_source_order_and_none_default() {
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
        assert_eq!(rules.trigger_house_type_index("missing"), None);
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

        assert_eq!(rules.color_add.slots[0].name.as_deref(), Some("First"));
        assert_eq!(rules.color_add.slots[0].rgb, [1, 2, 3]);
        assert_eq!(
            rules.color_add.slots[1].name.as_deref(),
            Some("StrongGreen")
        );
        assert_eq!(rules.color_add.slots[1].rgb, [0, 63, 0]);
        assert_eq!(rules.color_add.slots[2].rgb, [31, 63, 31]);
        assert_eq!(
            rules.color_add.slots[15],
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
    fn ordered_country_side_members_find_or_allocate_missing_country() {
        let ini = IniFile::from_str(
            "[Countries]\n0=Alpha\n\
             [Sides]\nGDI=Alpha,Beta\n\
             [Beta]\nSide=NewSide\nMultiplayPassive=yes\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("side-created country parses");

        assert_eq!(rules.country_index("Beta"), Some(CountryIdx(1)));
        assert_eq!(rules.side_index("NewSide"), Some(SideIdx(1)));
        assert_eq!(rules.country_side_index("beta"), Some(SideIdx(1)));
        assert!(rules.country_multiplay_passive("BETA"));
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
        let general = GeneralRules::from_ini(&ini);
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
        assert_eq!(defaults.ambient_change_step, 20);

        let stock = GeneralRules::from_ini(&IniFile::from_str(
            "[General]\nAmbientChangeRate=.2\nAmbientChangeStep=.2\n",
        ));
        assert!(stock.ambient_change_rate_nonzero);
        assert_eq!(stock.ambient_change_interval_frames, 180);
        assert_eq!(stock.ambient_change_step, 20);

        let tiny = GeneralRules::from_ini(&IniFile::from_str(
            "[General]\nAmbientChangeRate=.0009\nAmbientChangeStep=.019\n",
        ));
        assert!(tiny.ambient_change_rate_nonzero);
        assert_eq!(tiny.ambient_change_interval_frames, 0);
        assert_eq!(tiny.ambient_change_step, 1);

        let disabled = GeneralRules::from_ini(&IniFile::from_str(
            "[General]\nAmbientChangeRate=0\nAmbientChangeStep=.2\n",
        ));
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
        // Gravity lives in [AudioVisual] (stock rulesmd.ini), NOT [General].
        let stock = GeneralRules::from_ini(&IniFile::from_str(
            "[General]\nFlightLevel=500\n[AudioVisual]\nGravity=6\n",
        ));
        assert_eq!(stock.gravity, 6);
        let signed = GeneralRules::from_ini(&IniFile::from_str(
            "[General]\nFlightLevel=500\n[AudioVisual]\nGravity=-7\n",
        ));
        assert_eq!(signed.gravity, -7);
        // A [General] Gravity is ignored (the engine reads it in ReadAudioVisual).
        let misplaced = GeneralRules::from_ini(&IniFile::from_str("[General]\nGravity=9\n"));
        assert_eq!(misplaced.gravity, 3);
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
        let ctor_default = GeneralRules::default().savour_delay_minutes;
        assert_eq!(ctor_default, 0.03);
        assert_eq!(savour_delay_frames(ctor_default), 27);
        let stock = GeneralRules::from_ini(&IniFile::from_str(
            "[General]\nFlightLevel=500\n[AudioVisual]\nSavourDelay=.1\n",
        ));
        assert_eq!(stock.savour_delay_minutes, 0.1_f32 as f64);
        assert_eq!(savour_delay_frames(stock.savour_delay_minutes), 90);
        let explicit_point_zero_three = GeneralRules::from_ini(&IniFile::from_str(
            "[General]\nFlightLevel=500\n[AudioVisual]\nSavourDelay=.03\n",
        ));
        assert_eq!(
            explicit_point_zero_three.savour_delay_minutes,
            0.03_f32 as f64
        );
        assert_eq!(
            savour_delay_frames(explicit_point_zero_three.savour_delay_minutes),
            26,
            "explicit ReadDouble is parsed through f32 before ftol"
        );
        let absent = GeneralRules::from_ini(&IniFile::from_str(
            "[General]\nFlightLevel=500\n[AudioVisual]\n",
        ));
        assert_eq!(absent.savour_delay_minutes, 0.03);
        assert_eq!(savour_delay_frames(absent.savour_delay_minutes), 27);
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
        assert!((sa.verses_f64[0] - 1.0).abs() < 1e-9); // none: 100%
        assert!((sa.verses_f64[5] - 0.25).abs() < 1e-9); // heavy: 25%

        let ap: &WarheadType = rules.warhead("AP").expect("AP exists");
        assert!((ap.verses_f64[6] - 0.6).abs() < 1e-9); // wood: 60%
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
        for (actual, expected) in lower.verses_f64.iter().zip(expected) {
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

        assert!(rules.is_refinery_type("modproc"));
        assert!(!rules.is_refinery_type("FAKEREF"));
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

        assert!(rules.is_refinery_type("MODPROC"));
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
        assert_eq!(rules.general.purifier_bonus_ppm, 300_000);
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
        assert_eq!(rules.general.purifier_bonus_ppm, 250_000);
    }

    /// Fractional-percent PurifierBonus survives at full fixed-point precision
    /// (parts-per-million), NOT quantized to whole percent. `.333` -> 333_000 ppm,
    /// not 330_000. Stock `.25` stays exactly 250_000 (byte-identical to the old
    /// integer-percent form).
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
        assert_eq!(rules.general.purifier_bonus_ppm, 333_000);
        // The old integer-percent path would have rounded to 33% (330_000) — the drift.
        assert_ne!(rules.general.purifier_bonus_ppm, 330_000);
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
    fn combatdamage_destroyablebridges_no_does_not_clear_default_bridge_flag() {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n\
             [VehicleTypes]\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [CombatDamage]\n\
             DestroyableBridges=no\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("Should parse");
        assert!(rules.bridge_rules.destroyable_by_default);
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
        assert_eq!(
            rules.general.upgrade_veteran_sound.as_deref(),
            Some("UpgradeVeteran")
        );
        assert_eq!(
            rules.general.upgrade_elite_sound.as_deref(),
            Some("UpgradeElite")
        );
        assert_eq!(rules.general.elite_flash_timer, 150);
        let bare = RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n[General]\nFixtureOnly=1\n",
        ))
        .unwrap();
        assert_eq!(bare.general.veteran_rof, 1.0);
        assert_eq!(bare.general.upgrade_veteran_sound, None);
        // `RulesClass+0x30/+0x34/+0x38/+0x3C` are never written by the native
        // constructor, so a rules set without the keys keeps the reader's
        // argument: zero, which the heal arms decline to divide by.
        assert_eq!(bare.general.self_heal_infantry_frames, 0);
        assert_eq!(bare.general.self_heal_infantry_amount, 0);
        assert_eq!(bare.general.self_heal_unit_frames, 0);
        assert_eq!(bare.general.self_heal_unit_amount, 0);
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
    fn building_damage_sound_parses_and_an_absent_key_is_silence() {
        let stock = GeneralRules::from_ini(&IniFile::from_str(
            "[General]\nFlightLevel=500\n[AudioVisual]\nBuildingDamageSound=BuildingDamaged\n",
        ));
        assert_eq!(
            stock.building_damage_sound.as_deref(),
            Some("BuildingDamaged")
        );

        let absent = GeneralRules::from_ini(&IniFile::from_str(
            "[General]\nFlightLevel=500\n[AudioVisual]\n",
        ));
        assert_eq!(absent.building_damage_sound, None);

        let empty = GeneralRules::from_ini(&IniFile::from_str(
            "[General]\nFlightLevel=500\n[AudioVisual]\nBuildingDamageSound=\n",
        ));
        assert_eq!(empty.building_damage_sound, None);
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
        let general = GeneralRules::from_ini(&ini);
        assert_eq!(general.ore_twinkle.as_deref(), Some("TWNK1"));
        assert_eq!(general.ore_twinkle_chance, 30);

        let defaults = GeneralRules::from_ini(&IniFile::from_str("[General]\nOreTwinkle=\n"));
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
";
        let ini = IniFile::from_str(ini_str);
        let general = GeneralRules::from_ini(&ini);
        assert_eq!(
            general.gui_main_button_sound.as_deref(),
            Some("MainButtonClick")
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
        let general = GeneralRules::from_ini(&ini);
        assert_eq!(general.barrel_particle.as_deref(), Some("SmallGreySSys"));
    }

    #[test]
    fn barrel_particle_default_none() {
        let ini_str = "[General]\nFixtureOnly=1\n";
        let ini = IniFile::from_str(ini_str);
        let general = GeneralRules::from_ini(&ini);
        assert!(general.barrel_particle.is_none());
    }

    #[test]
    fn gsi_05_10_prism_type_is_parsed_as_a_building_identity() {
        let ini = IniFile::from_str("[General]\nPrismType= atesla \n");
        let general = GeneralRules::from_ini(&ini);
        assert_eq!(general.prism_type.as_deref(), Some("ATESLA"));

        let absent = GeneralRules::from_ini(&IniFile::from_str("[General]\nFixtureOnly=1\n"));
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
        let general = GeneralRules::from_ini(&ini);
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
        let general = GeneralRules::from_ini(&ini);
        assert_eq!(general.base_unit_types, vec!["AMCV", "SMCV", "PCV"]);
    }

    #[test]
    fn base_unit_types_default_to_stock_yr() {
        let ini = IniFile::from_str("[General]\nFixtureOnly=1\n");
        let general = GeneralRules::from_ini(&ini);
        assert_eq!(general.base_unit_types, vec!["AMCV", "SMCV", "PCV"]);
    }

    #[test]
    fn test_parachute_max_fall_rate_parsed() {
        let ini_str = "\
[General]
ParachuteMaxFallRate=-3
";
        let ini = IniFile::from_str(ini_str);
        let general = GeneralRules::from_ini(&ini);
        assert_eq!(general.parachute_max_fall_rate, -3);
    }

    #[test]
    fn test_parachute_max_fall_rate_default_when_missing() {
        let ini_str = "[General]\nFixtureOnly=1\n";
        let ini = IniFile::from_str(ini_str);
        let general = GeneralRules::from_ini(&ini);
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
        let general = GeneralRules::from_ini(&ini);
        assert_eq!(general.parachute_max_fall_rate, -1);
    }

    #[test]
    fn test_missile_rot_var_missing_key_keeps_native_constructor() {
        let ini = IniFile::from_str("[General]\nFixtureOnly=1\n");
        let general = GeneralRules::from_ini(&ini);
        assert_eq!(general.missile_rot_var, 0.25);
    }

    #[test]
    fn test_missile_rot_var_stock_rules_value_parsed() {
        let ini = IniFile::from_str("[General]\nMissileROTVar=.25\n");
        let general = GeneralRules::from_ini(&ini);
        let diff = (general.missile_rot_var - 0.25).abs();
        assert!(diff < 0.001, "got {:?}", general.missile_rot_var);
    }

    #[test]
    fn test_missile_rot_var_parsed() {
        let ini = IniFile::from_str("[General]\nMissileROTVar=2.5\n");
        let general = GeneralRules::from_ini(&ini);
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

    #[test]
    fn damage_spark_thresholds_match_x87_boundary() {
        // The stock-default thresholds are the EXACT boundary of gamemd's 80-bit
        // `(double)roll * 0x3E00000000400000 < band` compare. A 1-off here flips
        // the per-tick draw count → desync, so pin both bands and the threshold
        // derived into GeneralRules.
        assert_eq!(damage_spark_spawn_threshold(0.02), 42_949_673);
        assert_eq!(damage_spark_spawn_threshold(0.01), 21_474_837);
        let d = GeneralRules::default();
        assert_eq!(d.condition_red_spark_threshold, 42_949_673);
        assert_eq!(d.condition_yellow_spark_threshold, 21_474_837);

        // Verify the boundary directly against the exact f64 product gamemd uses:
        // roll = threshold-1 must PASS (roll*scale < band), roll = threshold must FAIL.
        // (roll·(2^30+1) fits the 64-bit x87 mantissa, so this f64 multiply equals
        // gamemd's 80-bit product exactly at the boundary.)
        let scale = f64::from_bits(0x3E00_0000_0040_0000);
        for (band, t) in [(0.02_f64, 42_949_673_u32), (0.01, 21_474_837)] {
            assert!(
                (t as f64 - 1.0) * scale < band,
                "roll=t-1 must pass for band {band}"
            );
            assert!(
                !((t as f64) * scale < band),
                "roll=t must fail for band {band}"
            );
        }

        // Degenerate bands.
        assert_eq!(damage_spark_spawn_threshold(0.0), 0);
        assert_eq!(damage_spark_spawn_threshold(-1.0), 0);
        assert_eq!(damage_spark_spawn_threshold(1.0), DAMAGE_SPARK_ROLL_COUNT);
        assert_eq!(damage_spark_spawn_threshold(2.0), DAMAGE_SPARK_ROLL_COUNT);
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
    fn resolved_rule_handles_use_bridge_warhead_defaults() {
        use crate::sim::intern::StringInterner;
        use crate::sim::type_handle_table::ResolvedRuleHandles;
        let ini: IniFile = IniFile::from_str(&make_test_rules());
        let rules: RuleSet = RuleSet::from_ini(&ini).expect("rules parse");
        let mut interner = StringInterner::default();
        let handles = ResolvedRuleHandles::resolve(&rules, &mut interner);
        // Defaults match retail rulesmd.ini ("IonCannonWH" + "Super") because
        // the test rules.ini has no `[CombatDamage]` overrides.
        assert_eq!(interner.resolve(handles.ion_cannon), "IonCannonWH");
        assert_eq!(interner.resolve(handles.c4), "Super");
        assert_eq!(interner.resolve(handles.crush), "Crush");
        assert!(handles.is_crush(handles.crush));
        assert!(!handles.is_crush(handles.c4));
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
    fn c4_delay_defaults_to_27_ticks() {
        let ini = IniFile::from_str("");
        let rules = RuleSet::from_ini(&ini).expect("parse");
        assert_eq!(rules.c4_delay_ticks, 27);
    }

    #[test]
    fn c4_delay_parses_double_minutes_to_ticks() {
        let ini = IniFile::from_str("[CombatDamage]\nC4Delay=0.1\n");
        let rules = RuleSet::from_ini(&ini).expect("parse");
        // 0.1 minutes × 60 × 15 = 90 ticks
        assert_eq!(rules.c4_delay_ticks, 90);
    }

    #[test]
    fn c4_delay_retail_default_value() {
        let ini = IniFile::from_str("[CombatDamage]\nC4Delay=0.03\n");
        let rules = RuleSet::from_ini(&ini).expect("parse");
        // 0.03 × 60 × 15 = 27 (.round())
        assert_eq!(rules.c4_delay_ticks, 27);
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

        // C4Delay must match the retail value (0.03 minutes = 27 ticks).
        assert_eq!(rules.c4_delay_ticks, 27, "C4Delay must parse to 27 ticks");
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
        fn check_ci<'a, M, V>(
            map: &'a HashMap<String, M>,
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

        assert_eq!(rules.shipyard_types, ["GAYARD", "nayard", "YAYARD"]);
        assert_eq!(
            rules.build_const_types,
            ["gacnst", "NACNST", "gacnst", "YACNST"],
            "ReadAI retains authored case, order, and duplicates while omitting null sentinels"
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
            assert_eq!(
                crate::rules::foundation::foundation_dimensions(&object.foundation),
                (4, 4)
            );
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

        assert_eq!(rules.build_const_types, ["GACNST"]);
        assert!(rules.object("GACNST").unwrap().build_const_eligible);
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
        assert_eq!(rules.build_const_types, ["GACNST"]);
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
        assert_eq!(rules.build_const_types, ["GACNST"]);
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
             [VehicleTypes]\n0=HARV\n1=AUNIT\n\
             [BuildingTypes]\n0=CON\n1=POW\n2=REF\n3=BAR\n4=TECH\n5=WEAP\n6=RAD\n7=POISON\n\
             [HARV]\nStrength=1\n[AUNIT]\nStrength=1\n\
             [CON]\nFoundation=1x1\n[POW]\nFoundation=1x1\n\
             [REF]\nFoundation=1x1\n[BAR]\nFoundation=1x1\n\
             [TECH]\nFoundation=1x1\n[WEAP]\nFoundation=1x1\n\
             [RAD]\nFoundation=1x1\n[POISON]\nFoundation=1x1\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("all Recalc type lists");

        assert_eq!(rules.build_const_types, ["con", "CON"]);
        assert_eq!(rules.build_power_types, ["pow", "POW"]);
        assert_eq!(rules.build_refinery_types, ["ref"]);
        assert_eq!(rules.build_barracks_types, ["bar"]);
        assert_eq!(
            rules.build_tech_types,
            ["tech"],
            "the internally space-prefixed token is not individually trimmed"
        );
        assert_eq!(rules.build_weapons_types, ["weap"]);
        assert_eq!(rules.build_radar_types, ["rad", "RAD"]);
        assert_eq!(
            rules.harvester_unit_types,
            ["harv", "HARV"],
            "sentinels are omitted, duplicates retained, and a space-prefixed Unit token is unresolved"
        );
        assert!(rules.object("CON").unwrap().build_const_eligible);
        assert!(!rules.object("POISON").unwrap().build_const_eligible);
    }

    #[test]
    fn recalc_type_lists_preserve_prior_layer_when_later_key_is_missing_or_empty() {
        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[General]\nHarvesterUnit=HARV\n\
             [AI]\nBuildConst=CON\nBuildPower=POW\nBuildTech=TECH\n\
             [VehicleTypes]\n0=HARV\n[HARV]\nStrength=1\n\
             [BuildingTypes]\n0=CON\n1=POW\n2=TECH\n3=NEWPOW\n\
             [CON]\nFoundation=1x1\n[POW]\nFoundation=1x1\n\
             [TECH]\nFoundation=1x1\n[NEWPOW]\nFoundation=1x1\n",
        ));
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str(
                "[General]\nHarvesterUnit=\nUnrelated=1\n\
                 [AI]\nBuildConst=\nBuildPower=NEWPOW\n",
            ),
        );
        let rules = RuleSet::from_rules_layers(&layers).expect("layered Recalc lists");
        assert_eq!(rules.harvester_unit_types, ["HARV"]);
        assert_eq!(rules.build_const_types, ["CON"]);
        assert_eq!(rules.build_power_types, ["NEWPOW"]);
        assert_eq!(rules.build_tech_types, ["TECH"]);
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
        let parsed = GeneralRules::from_ini(&ini);
        assert_eq!(parsed.infantry_death_anim(0), None);
        assert_eq!(parsed.infantry_death_anim(1), None);
        assert_eq!(parsed.infantry_death_anim(2), None);
        assert_eq!(parsed.infantry_death_anim(3), Some("EX3"));
        assert_eq!(parsed.infantry_death_anim(4), Some("EX4"));
        assert_eq!(parsed.infantry_death_anim(5), Some("SECOND_DECLARED"));
        assert_eq!(parsed.infantry_death_anim(6), Some("EX6"));
        assert_eq!(parsed.infantry_death_anim(7), Some("EX7"));
        assert_eq!(parsed.infantry_death_anim(8), Some("EX8"));
        assert_eq!(parsed.infantry_death_anim(9), Some("EX9"));
        assert_eq!(parsed.infantry_death_anim(10), Some("EX10"));

        let defaults = GeneralRules::default();
        assert_eq!(defaults.infantry_death_anim(3), Some("S_BANG34"));
        assert_eq!(defaults.infantry_death_anim(5), Some("ELECTRO"));
        assert_eq!(defaults.infantry_death_anim(10), Some("BRUTDIE"));
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
                &sim.session.map_name
            )
            .is_ok()
        );
        for changed in [&voxel_changed, &height_changed, &flat_changed] {
            assert!(matches!(
                GameSnapshot::load_validated(
                    &bytes,
                    11,
                    changed.simulation_config_hash(),
                    &sim.session.map_name
                ),
                Err(SnapshotError::RulesMismatch { .. })
            ));
        }
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
        let Some(ini) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
            return;
        };
        let rules = RuleSet::from_ini(&ini).expect("retail rules parse");
        assert_eq!(
            rules.general.difficulty_rof,
            [f64::from(0.8f32), 1.0, f64::from(1.2f32)]
        );
        for country in ["Americans", "Russians", "YuriCountry"] {
            assert_eq!(rules.country_rof(country), 1.0, "{country}");
        }
    }

    /// Retail corpse anims through the production reader: `[General]
    /// DeadBodies=` lists six, and the GI and Conscript name none of their own
    /// and are not `NotHuman=`, so their deaths pick from the six.
    #[test]
    fn retail_dead_bodies() {
        let Some(ini) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
            return;
        };
        let rules = RuleSet::from_ini(&ini).expect("retail rules parse");
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
        let Some(ini) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
            return;
        };
        let rules = RuleSet::from_ini(&ini).expect("retail rules parse");
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

    /// Retail `[ElevationModel]` through the production reader: four levels a
    /// step, two cells a step, two cells at most. Without the section the
    /// constructor's 0, 1.0 and 0.0 stay.
    #[test]
    fn retail_elevation_model() {
        let Some(ini) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
            return;
        };
        let two = NativeF64Bits::from_bits(2.0f64.to_bits());
        let model = RuleSet::from_ini(&ini)
            .expect("retail rules parse")
            .elevation_model;
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
}
