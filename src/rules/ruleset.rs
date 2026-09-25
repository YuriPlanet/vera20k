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
use crate::rules::ini_parser::IniFile;
use crate::rules::mission_data::MissionControl;
use crate::rules::native_processing::{ProcessedRulesLayers, RulesLayerStack};
use crate::rules::object_type::{BuildCategory, FactoryType, ObjectCategory, ObjectType};
use crate::rules::particle_system_type::{
    ParticleSystemType, ParticleSystemTypeId, PendingParticleSystemType,
};
use crate::rules::particle_type::{ParticleType, ParticleTypeId, PendingParticleType};
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

/// Country-level fields needed by gameplay systems.
#[derive(Debug, Clone)]
pub struct CountryRules {
    /// `MultiplayPassive=` allows non-owner garrison entry in `BuildingClass::CanDock`.
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

/// PPM scale for `IncomeMult` (1_000_000 = 1.0×). Must equal `apply_income_mult`'s divisor.
pub const INCOME_PPM_SCALE: i64 = 1_000_000;

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
            rof: 1.0,
            ui_name: None,
            name: None,
        }
    }
}

impl CountryRules {
    fn from_ini_section(section: &crate::rules::ini_parser::IniSection) -> Self {
        Self {
            multiplay_passive: section.get_bool("MultiplayPassive").unwrap_or(false),
            wall_owner: section.get_bool("WallOwner").unwrap_or(true),
            // IncomeMult is a raw multiplier (NOT a percent). Round in f64 to avoid f32
            // drift; absent -> the neutral 1.0 (stock).
            income_ppm: section
                .get_f32("IncomeMult")
                .map(|v| (v as f64 * INCOME_PPM_SCALE as f64).round() as i64)
                .unwrap_or(INCOME_PPM_SCALE),
            armor_infantry_mult: section.get_f32("ArmorInfantryMult").unwrap_or(1.0),
            armor_units_mult: section.get_f32("ArmorUnitsMult").unwrap_or(1.0),
            armor_aircraft_mult: section.get_f32("ArmorAircraftMult").unwrap_or(1.0),
            armor_buildings_mult: section.get_f32("ArmorBuildingsMult").unwrap_or(1.0),
            armor_defenses_mult: section.get_f32("ArmorDefensesMult").unwrap_or(1.0),
            rof: section.read_double("ROF", 1.0),
            ui_name: section
                .get("UIName")
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
            name: section
                .get("Name")
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
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

/// Production timing rules parsed from `[General]`.
///
/// The `_ppm` (parts-per-million) fields are pre-computed at INI parse time from the
/// corresponding f32 fields so that sim code can use pure integer arithmetic.
/// 1_000_000 = 1.0×. The f32 originals are kept for logging/debugging.
#[derive(Debug, Clone, Copy)]
pub struct ProductionRules {
    /// Minutes to build an object that costs 1000 credits before per-object modifiers.
    pub build_speed: f32,
    /// Time multiplier applied for each extra matching factory.
    pub multiple_factory: f32,
    /// Severity of the low-power speed penalty.
    pub low_power_penalty_modifier: f32,
    /// Lower bound on production speed while low power is active.
    pub min_low_power_production_speed: f32,
    /// Upper bound on production speed while low power is active.
    pub max_low_power_production_speed: f32,
    // -- Pre-computed integer-scaled values for deterministic sim math --
    /// `multiple_factory` scaled to PPM (e.g., 0.8 → 800_000).
    pub multiple_factory_ppm: u64,
    /// `low_power_penalty_modifier` scaled to PPM.
    pub low_power_penalty_modifier_ppm: u64,
    /// `min_low_power_production_speed` scaled to PPM.
    pub min_low_power_production_speed_ppm: u64,
    /// `max_low_power_production_speed` scaled to PPM.
    pub max_low_power_production_speed_ppm: u64,
    /// `build_speed` pre-scaled ×1000 for deterministic build-time computation.
    pub build_speed_x1000: u64,
    /// Speed coefficient applied to wall building production after all other
    /// queue time scaling. Parsed from `WallBuildSpeedCoefficient=` in [General].
    pub wall_build_speed_coefficient: f32,
}

/// PPM scale constant (1_000_000 = 1.0×) used for f32→integer conversion at parse time.
const PRODUCTION_PPM: u64 = 1_000_000;

/// Convert an f32 value clamped to `[min, ∞)` into PPM u64 at parse time only.
fn f32_to_ppm(val: f32, min: f32) -> u64 {
    (val.max(min) as f64 * PRODUCTION_PPM as f64) as u64
}

impl Default for ProductionRules {
    fn default() -> Self {
        Self {
            build_speed: 1.0,
            multiple_factory: 0.8,
            low_power_penalty_modifier: 1.0,
            min_low_power_production_speed: 0.5,
            max_low_power_production_speed: 0.9,
            multiple_factory_ppm: f32_to_ppm(0.8, 0.01),
            low_power_penalty_modifier_ppm: f32_to_ppm(1.0, 0.0),
            min_low_power_production_speed_ppm: f32_to_ppm(0.5, 0.0),
            max_low_power_production_speed_ppm: f32_to_ppm(0.9, 0.0),
            build_speed_x1000: (1.0f64 * 1000.0) as u64,
            wall_build_speed_coefficient: 1.0,
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
    /// Native gameplay-frame delay derived from art.ini `[ANIM_NAME]` Rate=.
    pub frame_delay: u16,
}

/// Convert RulesClass `[AudioVisual] SavourDelay` minutes to the signed timer's
/// ordinary non-negative frame domain. Native multiplies by 900.0 and calls
/// `Math__ftol`, whose active x87 control word truncates toward zero.
pub(crate) fn savour_delay_frames(minutes: f64) -> u64 {
    (minutes * 900.0).clamp(0.0, u64::MAX as f64).trunc() as u64
}

/// Global gameplay constants from `[General]` that affect vision, gap generators, etc.
#[derive(Debug, Clone)]
pub struct GeneralRules {
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
    /// `[General] VeteranRatio=` — how many times its own cost an object must
    /// destroy to gain one rank. `RulesClass+0x668`, read at `0x0066EEB0`.
    pub veteran_ratio: f64,
    /// `[General] VeteranCap=` — the accumulator clamp. `RulesClass+0x698`,
    /// read at `0x0066EF94`. Stock `2`, which is exactly the elite threshold,
    /// so elite is terminal.
    pub veteran_cap: f64,
    /// `[General] ComputerBaseDefenseResponse=`. The active House responder
    /// forms its signed/wrapping budget as `attacker Cost * this value`.
    pub computer_base_defense_response: i32,
    /// Signed `[General] MaximumBuildingPlacementFailures=` at `Rules+0xE48`.
    /// Native constructor default is `5`; both active retail rules files
    /// override it with `3`. Building exit compares strictly after incrementing;
    /// negative mod values remain literal.
    pub maximum_building_placement_failures: i32,
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
    /// Low byte of `CliffBackImpassability=` in `[General]`.
    /// Byte 0 skips the scan; only byte 2 can write Rock. Default 2 in standard YR.
    pub cliff_back_impassability: u8,
    /// Underground travel speed for Tunnel locomotor units (TunnelSpeed=).
    /// Default 6.0 cells/second matching RA2 default.
    pub tunnel_speed: SimFixed,
    /// `MissileROTVar=` from [General]. Amplitude of the sidewinder cosine
    /// modulation in homing missile flight; the per-tick ROT scales by
    /// `(1 + var) + cos(2π * frame / 15) * var`. Stock RA2/YR rules set
    /// `.25`, yielding roughly 1.0 to 1.5 times the projectile's base ROT.
    /// The parser fallback for a missing key remains 1.0.
    pub missile_rot_var: SimFixed,
    /// Default cruise altitude for Fly-locomotor aircraft (FlightLevel= in [General]).
    /// Fallback 500 leptons matches the engine constructor default; retail
    /// rulesmd.ini always supplies its own (1500), so the fallback only fires
    /// for a non-retail INI missing the key. A type's own `FlightLevel=`
    /// overrides it through `ObjectType::flight_level`.
    pub flight_level: i32,
    /// Rules+420, [JumpjetControls] CruiseHeight. Object5F4260 uses this
    /// global threshold; linked Jumpjets instead use their own +2C height.
    pub display_cruise_height: i32,
    /// Hover locomotor cruise altitude in leptons (`[General] HoverHeight=`, default 120).
    /// The damped-spring vertical controller holds hover units at this height.
    pub hover_height: i32,
    /// Hover bob PERIOD in minutes (`[General] HoverBob=`, default 0.04). The visible
    /// `2·cos` float wobble completes one cycle every `round(HoverBob × 900)` ticks
    /// (≈36 moving). NOTE: this is the period, not the amplitude (amplitude = `Gravity`).
    pub hover_bob: SimFixed,
    /// Hover straightaway speed boost (`[General] HoverBoost=`, default 1.5 / 150%).
    /// Applied as `SpeedMult` when two same-direction path steps are queued, but the
    /// throttle target is clamped to 1.0 afterward, so it only bites at approach speed.
    pub hover_boost: SimFixed,
    /// Hover acceleration TIME in minutes (`[General] HoverAcceleration=`, default 0.02).
    /// Per-tick throttle ramp-up = `1 / (HoverAcceleration × 900)` → 18 ticks 0→full.
    pub hover_acceleration: SimFixed,
    /// Hover brake TIME in minutes (`[General] HoverBrake=`, default 0.03).
    /// Per-tick throttle ramp-down = `1 / (HoverBrake × 900)` → 27 ticks full→0.
    pub hover_brake: SimFixed,
    /// Hover vertical damped-spring coefficient (`[General] HoverDampen=`, default 0.4 / 40%).
    pub hover_dampen: SimFixed,
    /// Descent rate cap for parachuted units, in leptons/tick (signed).
    /// Per gamemd, the rate field accumulates by `-1` per tick and clamps
    /// to this value. Default `-3` matches `[General] ParachuteMaxFallRate=-3`.
    /// Negative = falling.
    pub parachute_max_fall_rate: i32,
    /// Paradrop trigger radius in leptons. From `[General] ParadropRadius=`.
    /// Default 1024 (~4 cells). Distance to target at which the carrier aircraft
    /// reveals fog + transitions to the overfly mission.
    pub paradrop_radius: i32,
    /// Carrier aircraft type for paradrop missions. Default `PDPLANE`.
    pub paradrop_aircraft_type: String,
    /// Parsed `[General] Parachute=` value (uppercased AnimType name, e.g.
    /// "PARACH"), `None` if unset or empty: the canopy the simulation attaches
    /// to a dropped object. Its timing is the AnimType's own art.
    pub parachute_shp: Option<String>,
    /// American paradrop list: parallel `(infantry_type, count)` pairs.
    /// From `[General] AmerParaDropInf=` zipped with `AmerParaDropNum=`.
    /// Default `[("E1", 8)]`.
    pub amer_paradrop_list: Vec<(String, u32)>,
    /// Allied paradrop list. Default `[("E1", 6)]`.
    pub ally_paradrop_list: Vec<(String, u32)>,
    /// Soviet paradrop list. Default `[("E2", 9)]`. Per gamemd the dispatch
    /// case skips the count-equality assert on this branch only — preserved.
    pub sov_paradrop_list: Vec<(String, u32)>,
    /// Yuri paradrop list. Default `[("INIT", 6)]`.
    pub yuri_paradrop_list: Vec<(String, u32)>,
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
    /// BuildingType identity handled by the Prism support/cascade mission path.
    /// The generic art-delayed fire path must not consume this type.
    pub prism_type: Option<String>,
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
    pub infantry_blink_disguise_time: u32,
    /// Whether the attack cursor appears on trees/terrain
    /// (`TreeTargeting=` in `[CombatDamage]`).
    /// Default false in vanilla RA2.
    pub tree_targeting: bool,
    /// Health ratio threshold below which the bar turns yellow (ConditionYellow= in [AudioVisual]).
    /// Default 0.5 (50%).
    pub condition_yellow: f64,
    /// Health ratio threshold below which the bar turns red (ConditionRed= in [AudioVisual]).
    /// Default 0.25 (25%).
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
    /// `[AudioVisual] EliteFlashTimer=` — `RulesClass+0xBE8`. Seeded into
    /// `TechnoClass+0xF0` on an elite promotion (`0x006FA0D6..0x006FA0DC`),
    /// whatever house owns the object. Constructor default UNCHECKED.
    pub elite_flash_timer: i32,
    /// `IdleActionFrequency=` from `[AudioVisual]`, pre-scaled to integer ×1000.
    ///
    /// Scales how long an idle infantryman waits between fidgets: the wait is
    /// drawn from `frequency * 450` to `frequency * 1800` frames, so stock
    /// `.15` gives 67 to 270 frames. Stored ×1000 because the sim may only do
    /// integer arithmetic with it. gamemd's own constructor default (what it
    /// would use if the key were missing) is UNCHECKED; stock `rulesmd.ini`
    /// always supplies the key, so the fallback below only ever serves fixtures.
    pub idle_action_frequency_x1000: i64,
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
    /// `ThreatPerOccupant=` (`RulesClass+0x0DF4`, `RulesClass::ReadGeneral @
    /// 0x00670128`; constructor default 5 at `0x006668DE`, stock rulesmd 10).
    /// A garrisoned building's `ThreatPosed` is `occupants * this` instead of
    /// its own type value (`TechnoClass::Get_ThreatPosed @ 0x00708B40`).
    pub threat_per_occupant: i32,
    /// `NormalTargetingDelay=` ([General], stock 27) — frames between passive
    /// target scans for every mission except Area Guard. The per-object scan
    /// timer is re-armed to this value plus a 0..=2 scenario-RNG jitter.
    pub normal_targeting_delay: u32,
    /// `[General] DeadBodies=` (`Rules+0x124`): the corpse anims an
    /// infantryman's Die1..Die5 completion picks from when its type names none
    /// (`0x00520C42..0x00520C91`). Retail: `DEATH_A`..`DEATH_F`.
    pub dead_bodies: Vec<String>,
    /// `GuardAreaTargetingDelay=` ([General], stock 36) — the same cadence for
    /// an Area Guard object, which scans twice as far and so scans less often.
    pub guard_area_targeting_delay: u32,
    /// SFX played when the first occupant enters a CanBeOccupied building.
    /// Parsed from [AudioVisual] BuildingGarrisonedSound (typically "BuildingGarrisoned").
    /// None = no sound configured. Resolved at app layer to a sound.ini entry.
    pub building_garrisoned_sound: Option<String>,
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
    /// Generic shell click sound from [AudioVisual] GenericClick.
    pub generic_click_sound: Option<String>,
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
    /// Short-range ore scan radius in cells (TiberiumShortScan= in [General]).
    /// Used when harvesting a single patch — scan nearby for the next cell.
    /// Default 6 cells. YR only (RA2 hardcodes the same value).
    pub tiberium_short_scan: i32,
    /// Long-range ore scan radius in cells (TiberiumLongScan= in [General]).
    /// Used when short scan fails — look further for a new ore patch.
    /// Default 48 cells.
    pub tiberium_long_scan: i32,
    /// Slave Miner short scan distance in cells (SlaveMinerShortScan= in [General]).
    /// Deployed Slave Miner checks this range to decide if it should reposition.
    /// Default 8.
    pub slave_miner_short_scan: i32,
    /// Slave unit scan distance in cells (SlaveMinerSlaveScan= in [General]).
    /// Slaves scan further than their master since they trust it would reposition if needed.
    /// Default 14.
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
    /// Slave Miner long scan distance in cells (SlaveMinerLongScan= in [General]).
    /// Used when searching for a new ore field to deploy near. Default 48.
    pub slave_miner_long_scan: i32,
    /// Cell improvement threshold for Slave Miner repositioning (SlaveMinerScanCorrection=).
    /// The new spot must be this many cells closer to ore to justify moving. Default 3.
    pub slave_miner_scan_correction: i32,
    /// Guard duration before deployed Slave Miner re-scans for ore (SlaveMinerKickFrameDelay=).
    /// In game frames (15 fps). Default 150 (~10 seconds).
    pub slave_miner_kick_frame_delay: u32,
    /// Standard harvester "too far" threshold in cells (HarvesterTooFarDistance=).
    /// If the nearest refinery is farther than this, the harvester drives next to it
    /// before reserving a dock. Default 5.
    pub harvester_too_far_distance: i32,
    /// Chrono harvester "too far" threshold in cells (ChronoHarvTooFarDistance=).
    /// Larger than standard because chrono miners teleport back. Default 50.
    pub chrono_harv_too_far_distance: i32,

    // -- Harvester timing --
    /// Frames per StepTimer increment during ore gathering (HarvesterLoadRate=).
    /// One bale requires 9 steps, so harvest_interval = rate * 9. Default 2.
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
    /// `CrewEscape=` (Rules `+0x5C0`, `ReadDouble`), the chance a crewed
    /// vehicle's crew escapes. Constructor default 0.5 (`0x00665E11..0x00665E17`).
    /// `read_double` scales a `%` value by 0.01 in host binary64; native
    /// `CCINIClass::ReadDouble @ 0x005283D0` FMULs under the chop control word,
    /// so a mod percentage can differ in the last bit (stock "50%" is exact).
    pub crew_escape: crate::util::native_x87::NativeF64Bits,
    /// `RefundPercent=` (Rules `+0x1738`, `ReadDouble`), the human-owner refund
    /// share `TechnoTypeClass::GetRefund @ 0x00711F60` applies. Constructor
    /// default 0.5 (`0x006675CE..0x006675D4`, ECX set at `0x00667190`). Same
    /// `%` rounding note as `crew_escape`.
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

    // -- Service depot / unit repair --
    /// Ticks between applying RepairStep HP when a unit is on a repair depot.
    /// Derived from URepairRate= in [General] (minutes). Default 0.016 min ≈ 14 ticks at 15 Hz.
    pub unit_repair_rate_ticks: u32,
    /// HP healed per repair step on a service depot (RepairStep= in [General]).
    /// Fallback 5 matches the engine constructor default; retail rulesmd sets 8.
    pub repair_step: u16,
    /// Percent of build cost charged for a full unit repair (RepairPercent= in [General]).
    /// Fallback 25 (25%) matches the engine constructor default; retail rulesmd
    /// sets 15%. Total cost = cost * repair_percent / 100.
    pub repair_percent: u16,

    // -- Aircraft ammo reload --
    /// Ticks to reload one ammo point at an airfield (from ReloadRate= minutes in [General]).
    /// Default: 270 ticks (0.3 min × 60 sec × 15 ticks/sec).
    pub reload_rate_ticks: u32,

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
    /// only on Guard, Area Guard or Patrol (`ShouldRetaliate 0x007089F7`).
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
    /// `[IQ] RepairSell` outer gate for BuildingClass repair/sell AI.
    pub iq_repair_sell: i32,
    /// `[IQ] SellBack` gate for the red-health low-credit sell decision.
    pub iq_sell_back: i32,
    /// `[AI] CreditReserve` threshold. A latched AI building is considered
    /// for sale only while its owner's credits are strictly below this value.
    pub credit_reserve: i32,

    /// Overlay type names that are opaque concrete walls (ConcreteWalls= in [General]).
    /// Concrete walls do NOT render a ghost sprite during placement -- only the
    /// valid/invalid cell grid is shown. Fence walls (not in this list) still
    /// render their connectivity ghost. Stored uppercase for case-insensitive matching.
    pub concrete_walls: Vec<String>,

    // -- Lightning Storm superweapon constants --
    /// Duration of active storm in game frames (LightningStormDuration= in [General]).
    /// Default 180 frames (12 seconds at 15 fps).
    pub lightning_storm_duration: i32,
    /// Damage per lightning bolt strike (LightningDamage= in [General]). Default 250.
    pub lightning_damage: i32,
    /// Deferment countdown before storm bolts begin (LightningDeferment= in [General]).
    /// Default 250 frames.
    pub lightning_deferment: i32,
    /// Frames between center bolt strikes (LightningHitDelay= in [General]). Default 10.
    pub lightning_hit_delay: i32,
    /// Frames between scatter bolt strikes (LightningScatterDelay= in [General]). Default 5.
    pub lightning_scatter_delay: i32,
    /// Cell radius for scatter bolt placement (LightningCellSpread= in [General]). Default 10.
    pub lightning_cell_spread: i32,
    /// Minimum manhattan distance between consecutive bolts (LightningSeparation= in [General]).
    /// Default 3.
    pub lightning_separation: i32,
    /// Warhead ID for lightning bolt damage (LightningWarhead= in [General]). Default "IonWH".
    pub lightning_warhead: String,
    /// Whether `[General] AmbientChangeRate=` is nonzero before its native
    /// frame conversion. Kept separately because a nonzero mod value can chop
    /// to a zero-frame interval while still passing ScenarioClass's outer gate.
    pub ambient_change_rate_nonzero: bool,
    /// Lightning/global ambient transition interval in native frames:
    /// `ftol(AmbientChangeRate * 900)`.
    pub ambient_change_interval_frames: i32,
    /// Signed ambient scalar delta: `ftol(AmbientChangeStep * 100)`.
    pub ambient_change_step: i32,
    // --- IronCurtain ([CombatDamage]) ---
    /// IronCurtain invulnerability duration in frames (IronCurtainDuration= in [CombatDamage]).
    pub iron_curtain_duration: u32,
    // --- IronCurtain ([General]) ---
    /// Animation played on IC target (IronCurtainInvokeAnim= in [General]). Default IRONBLST.
    pub iron_curtain_invoke_anim: String,
    /// `[General] IonBlast=` (`RulesClass+0x298`), the animation the Genetic
    /// Mutator launch constructs (`SuperClass::Launch 0x006CD8A5`). Retail
    /// `RING1`. The constructor default is a null type: no key, no animation.
    pub ion_blast_anim: String,
    // --- ForceShield ([General]) ---
    /// Cell radius of ForceShield AoE (ForceShieldRadius= in [General]).
    pub force_shield_radius: u32,
    /// ForceShield invulnerability duration in frames (ForceShieldDuration= in [General]).
    pub force_shield_duration: u32,
    /// Power blackout duration triggered by ForceShield (ForceShieldBlackoutDuration= in [General]).
    pub force_shield_blackout_duration: u32,
    /// Frames before fade sound plays (ForceShieldPlayFadeSoundTime= in [General]).
    pub force_shield_fade_sound_time: u32,
    /// Animation played on FS target (ForceShieldInvokeAnim= in [General]). Default FORCSHLD.
    pub force_shield_invoke_anim: String,
    // --- PsychicReveal ([CombatDamage]) ---
    /// Cell radius revealed by PsychicReveal
    /// (`PsychicRevealRadius=` in `[CombatDamage]`).
    pub psychic_reveal_radius: u32,
    // --- GeneticConverter ([SpecialWeapons] + [General]) ---
    /// Warhead used for mutation (`MutateWarhead=` in `[SpecialWeapons]`).
    pub mutate_warhead: String,
    /// Warhead used for mutate explosion
    /// (`MutateExplosionWarhead=` in `[SpecialWeapons]`).
    pub mutate_explosion_warhead: String,
    /// Whether MutateExplosion is enabled (MutateExplosion= in [General]). Default true.
    pub mutate_explosion: bool,
    /// `[General] MetallicDebris=` — list of animation names to spawn (50%-RNG
    /// gated, count-checked) on bridge-cell collapse. Default 20 entries.
    /// Mirrors gamemd `Rules+0x140` (data ptr) / `+0x14C` (count).
    pub metallic_debris: Vec<String>,
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

/// Default animation rate when art.ini section is missing.
/// Matches gamemd constructor default: 1 game frame at 60fps ≈ 17ms.
const DEFAULT_ANIM_FRAME_DELAY: u16 = 1;

/// Stand-in for `[AudioVisual] IdleActionFrequency=` when the key is absent.
///
/// Stock `rulesmd.ini` sets `.15`, so this only serves fixtures that build a
/// RuleSet without an `[AudioVisual]` section. gamemd's own constructor default
/// is UNCHECKED.
const STOCK_IDLE_ACTION_FREQUENCY_X1000: i64 = 150;

/// A `[General]` InfantryType reference (RulesClass binds these through
/// `FindOrAllocate`); an absent or empty value leaves the pointer null.
fn general_type_name(general: &crate::rules::ini_parser::IniSection, key: &str) -> Option<String> {
    general
        .get(key)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

/// Zip a parallel pair of paradrop INI keys (`Inf` + `Num`) into `(type, count)` pairs.
/// `skip_count_assert` mirrors gamemd's Soviet branch which lacks the equality check.
fn parse_paradrop_list(
    general: &crate::rules::ini_parser::IniSection,
    inf_key: &str,
    num_key: &str,
    skip_count_assert: bool,
    default: Vec<(String, u32)>,
) -> Vec<(String, u32)> {
    let inf: Vec<String> = match general.get_list(inf_key) {
        Some(list) => list
            .into_iter()
            .filter(|s| !s.is_empty())
            .map(|s| s.to_uppercase())
            .collect(),
        None => return default,
    };
    let nums: Vec<u32> = match general.get_list(num_key) {
        Some(list) => list
            .into_iter()
            .filter_map(|s| s.parse::<u32>().ok())
            .collect(),
        None => return default,
    };
    if !skip_count_assert && inf.len() != nums.len() {
        log::warn!(
            "Paradrop list mismatch: {}={} entries but {}={} entries — using defaults",
            inf_key,
            inf.len(),
            num_key,
            nums.len(),
        );
        return default;
    }
    inf.into_iter().zip(nums.into_iter()).collect()
}

impl Default for GeneralRules {
    fn default() -> Self {
        Self {
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
            veteran_armor: 1.0,
            curley_shuffle: false,
            repair_rate_minutes: 0.016,
            veteran_ratio: VETERAN_RATIO_DEFAULT,
            veteran_cap: VETERAN_CAP_DEFAULT,
            computer_base_defense_response: 3,
            // Native Rules+0xE48 constructor default; active retail overrides to 3.
            maximum_building_placement_failures: 5,
            base_defense_delay_minutes: 0.25,
            suspend_priority: 20,
            suspend_delay_minutes: 2.0,
            leptons_per_sight_increase: 0,
            gap_radius: 10,
            reveal_by_height: true,
            tunnel_speed: sim_from_f32(6.0),
            missile_rot_var: sim_from_f32(1.0),
            flight_level: 500,
            display_cruise_height: 400, // Rules constructor665C3A
            hover_height: 120,
            hover_bob: sim_from_f32(0.04),
            hover_boost: sim_from_f32(1.5),
            hover_acceleration: sim_from_f32(0.02),
            hover_brake: sim_from_f32(0.03),
            hover_dampen: sim_from_f32(0.4),
            parachute_max_fall_rate: -3,
            paradrop_radius: 1024,
            paradrop_aircraft_type: "PDPLANE".to_string(),
            parachute_shp: None,
            amer_paradrop_list: vec![("E1".to_string(), 8)],
            ally_paradrop_list: vec![("E1".to_string(), 6)],
            sov_paradrop_list: vec![("E2".to_string(), 9)],
            yuri_paradrop_list: vec![("INIT".to_string(), 6)],
            base_unit_types: vec!["AMCV".to_string(), "SMCV".to_string(), "PCV".to_string()],
            multiplayer_ai_cm: Vec::new(),
            pad_aircraft_types: Vec::new(),
            separate_aircraft: false,
            prism_type: None,
            tiberium_grows: true,
            tiberium_spreads: true,
            growth_rate_minutes: 2.0,
            warp_in: AnimRef {
                name: "WARPIN".to_string(),
                frame_delay: 1,
            },
            warp_out: AnimRef {
                name: "WARPOUT".to_string(),
                frame_delay: 1,
            },
            warp_away: AnimRef {
                name: "WARPAWAY".to_string(),
                frame_delay: 1,
            },
            chrono_sparkle1: AnimRef {
                name: "CHRONOSK".to_string(),
                frame_delay: 1,
            },
            wake: AnimRef {
                name: "WAKE1".to_string(),
                frame_delay: 1,
            },
            move_flash: AnimRef {
                name: "RING".to_string(),
                frame_delay: 1,
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
                Some("GENDEATH".to_string()),
                Some("BRUTDIE".to_string()),
            ],
            attack_cursor_on_disguise: false,
            default_mirage_disguises: Vec::new(),
            infantry_blink_disguise_time: 0,
            tree_targeting: false,
            condition_yellow: 0.5,
            condition_red: 0.25,
            cloaking_stages: 9,
            cloak_delay_frames: 18,
            cloak_sound: None,
            upgrade_veteran_sound: None,
            upgrade_elite_sound: None,
            elite_flash_timer: 0,
            idle_action_frequency_x1000: STOCK_IDLE_ACTION_FREQUENCY_X1000,
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
            threat_per_occupant: 5,
            normal_targeting_delay: 27,
            dead_bodies: Vec::new(),
            guard_area_targeting_delay: 36,
            building_garrisoned_sound: None,
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
            psychic_dominator_activate_sound: None,
            genetic_mutator_activate_sound: None,
            psychic_reveal_activate_sound: None,
            chute_sound: None,
            gui_main_button_sound: None,
            gui_move_in_sound: None,
            gui_move_out_sound: None,
            generic_click_sound: None,
            generic_beep_sound: None,
            gui_checkbox_sound: None,
            ore_twinkle: None,
            ore_twinkle_chance: 50,
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
            bomb_ticking_sound: None,
            bomb_attach_sound: None,
            damage_delay_minutes: 1.0,
            spy_power_blackout_frames: 1000,
            damage_fire_types: vec![],
            barrel_particle: None,
            tiberium_short_scan: 6,
            tiberium_long_scan: 48,
            slave_miner_short_scan: 8,
            slave_miner_slave_scan: 14,
            drain_money_frame_delay: 30,
            drain_money_amount: 30,
            slave_miner_long_scan: 48,
            slave_miner_scan_correction: 3,
            slave_miner_kick_frame_delay: 150,
            harvester_too_far_distance: 5,
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
            // URepairRate=.016 min = 0.96 sec ≈ 14 ticks at 15 Hz.
            unit_repair_rate_ticks: 14,
            repair_step: 5,
            repair_percent: 25,
            // ReloadRate=.3 min = 18 sec = 270 ticks at 15 Hz.
            reload_rate_ticks: 270,
            // PathDelay=.01 min = 0.6 sec = 9 ticks at 15 Hz.
            path_delay: 0.016,
            // BlockagePathDelay=60 frames (directly in frames, not minutes).
            blockage_path_delay_ticks: 60,
            ai_auto_deploy_frame_delay: Vec::new(),
            // RulesClass constructor clears PlayerScatter and stores 3 into
            // [IQ] Scatter; stock rulesmd overrides the latter with 2.
            player_scatter: false,
            player_return_fire: false,
            iq_scatter: 3,
            max_iq_levels: 5,
            iq_production: 5,
            iq_repair_sell: 3,
            iq_sell_back: 2,
            credit_reserve: 1000,
            concrete_walls: Vec::new(),
            cliff_back_impassability: 2,
            lightning_storm_duration: 180,
            lightning_damage: 250,
            lightning_deferment: 250,
            lightning_hit_delay: 10,
            lightning_scatter_delay: 5,
            lightning_cell_spread: 10,
            lightning_separation: 3,
            lightning_warhead: "IonWH".to_string(),
            ambient_change_rate_nonzero: true,
            ambient_change_interval_frames: 180,
            ambient_change_step: 20,
            iron_curtain_duration: 750,
            iron_curtain_invoke_anim: "IRONBLST".to_string(),
            ion_blast_anim: String::new(),
            force_shield_radius: 4,
            force_shield_duration: 500,
            force_shield_blackout_duration: 1000,
            force_shield_fade_sound_time: 75,
            force_shield_invoke_anim: "FORCSHLD".to_string(),
            psychic_reveal_radius: 15,
            mutate_warhead: "Mutate".to_string(),
            mutate_explosion_warhead: "MutateExplosion".to_string(),
            mutate_explosion: true,
            metallic_debris: vec![
                "DBRIS1LG", "DBRIS2LG", "DBRIS3LG", "DBRIS4LG", "DBRIS5LG", "DBRIS6LG", "DBRIS7LG",
                "DBRIS8LG", "DBRIS9LG", "DBRS10LG", "DBRIS1SM", "DBRIS2SM", "DBRIS3SM", "DBRIS4SM",
                "DBRIS5SM", "DBRIS6SM", "DBRIS7SM", "DBRIS8SM", "DBRIS9SM", "DBRS10SM",
            ]
            .into_iter()
            .map(|s| s.to_string())
            .collect(),
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
    /// Damage multiplier for open-topped passengers.
    pub open_topped_damage_multiplier: f32,
    /// Range bonus in cells for open-topped passengers.
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
            open_topped_range_bonus: 0,
        }
    }
}

impl GarrisonRules {
    fn from_ini(ini: &IniFile) -> Self {
        let section = ini.section("CombatDamage");
        let get_f32 = |key: &str, default: f32| -> f32 {
            section.and_then(|s| s.get_f32(key)).unwrap_or(default)
        };
        let get_i32 = |key: &str, default: i32| -> i32 {
            section.and_then(|s| s.get_i32(key)).unwrap_or(default)
        };
        Self {
            occupy_damage_multiplier: get_f32("OccupyDamageMultiplier", 1.0),
            occupy_rof_multiplier: get_f32("OccupyROFMultiplier", 1.0),
            occupy_weapon_range: get_i32("OccupyWeaponRange", 5),
            bunker_damage_multiplier: get_f32("BunkerDamageMultiplier", 1.0),
            bunker_rof_multiplier: get_f32("BunkerROFMultiplier", 1.0),
            bunker_weapon_range_bonus: get_i32("BunkerWeaponRangeBonus", 0),
            open_topped_damage_multiplier: get_f32("OpenToppedDamageMultiplier", 1.0),
            open_topped_range_bonus: get_i32("OpenToppedRangeBonus", 0),
        }
    }
}

/// Bridge damage/destruction rules parsed from `rules(md).ini`.
#[derive(Debug, Clone)]
pub struct BridgeRules {
    /// Hit points shared by a destroyable bridge span.
    pub strength: u16,
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
            strength: 1500,
            destroyable_by_default: true,
            explosions: Vec::new(),
            voxel_max: 3,
            repair_sound: None,
        }
    }
}

impl BridgeRules {
    fn from_ini(ini: &IniFile) -> Self {
        let strength = ini
            .section("CombatDamage")
            .and_then(|section| section.get_i32("BridgeStrength"))
            .unwrap_or(1500)
            .max(1) as u16;
        let destroyable_by_default = true;
        let explosions = ini
            .section("General")
            .and_then(|section| section.get_list("BridgeExplosions"))
            .map(|list| list.into_iter().map(|s| s.to_uppercase()).collect())
            .unwrap_or_default();
        let voxel_max = ini
            .section("General")
            .and_then(|section| section.get_i32("BridgeVoxelMax"))
            .unwrap_or(3)
            .clamp(0, 255) as u8;
        let repair_sound = ini
            .section("AudioVisual")
            .and_then(|section| section.get("RepairBridgeSound"))
            .map(|s| s.trim().to_uppercase())
            .filter(|s| !s.is_empty());
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
        let get_i32 = |key: &str, default: i32| -> i32 { section.get_i32(key).unwrap_or(default) };
        Self {
            duration_multiple: get_i32("RadDurationMultiple", d.duration_multiple),
            // Delays are used as divisors/modulo periods — clamp to >= 1 so a
            // degenerate INI value cannot divide by zero.
            application_delay: get_i32("RadApplicationDelay", d.application_delay).max(1),
            level_max: get_i32("RadLevelMax", d.level_max),
            level_delay: get_i32("RadLevelDelay", d.level_delay).max(1),
            light_delay: get_i32("RadLightDelay", d.light_delay).max(1),
            level_factor: section
                .get("RadLevelFactor")
                .and_then(|s| s.trim().parse::<f64>().ok())
                .unwrap_or(d.level_factor),
            light_factor: section
                .get_f32("RadLightFactor")
                .map(sim_from_f32)
                .unwrap_or(d.light_factor),
            tint_factor: section
                .get_f32("RadTintFactor")
                .map(sim_from_f32)
                .unwrap_or(d.tint_factor),
            color: section
                .get("RadColor")
                .and_then(|s| {
                    let mut it = s.split(',').map(|p| p.trim().parse::<u8>());
                    match (it.next(), it.next(), it.next()) {
                        (Some(Ok(r)), Some(Ok(g)), Some(Ok(b))) => Some((r, g, b)),
                        _ => None,
                    }
                })
                .unwrap_or(d.color),
            site_warhead: section
                .get("RadSiteWarhead")
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
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

impl GeneralRules {
    /// Drive4B3A65, Ship6A30B4 and Foot/Walk's timer producers retain the
    /// configured double until FLD/FMUL900/ftol7C5F00. In particular, authored
    /// decimal .01 is parsed through float by ReadDouble5283D0 and converts to
    /// 8, while 1% converts to9. No clamp, round-to-nearest or u16 narrowing.
    pub fn path_delay_ticks(&self) -> i32 {
        use crate::util::native_x87::{MaskedX87Chop53 as X87, NativeF64Bits};
        X87::ftol_i32_low_masked(X87::mul(
            X87::load_f64(NativeF64Bits::from_bits(self.path_delay.to_bits())),
            X87::load_i32(900),
        ))
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
        // gamemd-derived: `RulesClass__ReadIQ @ 0x00674240` reads signed
        // `[IQ] Production` into `Rules+0x143C` at `0x006742C1`, independently
        // of the `[General]` pass and without clamping the parsed dword.
        let iq_production = ini
            .section("IQ")
            .and_then(|section| section.get_i32("Production"))
            .unwrap_or(defaults.iq_production);
        // RulesProcess668F56 reaches ReadAudioVisual6691E0 independently of
        // ReadGeneral.66B34B/66B372 pass AudioVisual to5283D0 and store raw
        // doubles in Rules+1708/+1700; a missing General section cannot skip them.
        let audio_visual = ini.section("AudioVisual");
        let condition_yellow_native = audio_visual
            .map(|s| s.read_double("ConditionYellow", 0.5))
            .unwrap_or(0.5);
        let condition_red_native = audio_visual
            .map(|s| s.read_double("ConditionRed", 0.25))
            .unwrap_or(0.25);
        // The bomb sounds (Rules+0x20C/+0x210) come from the same pass.
        let audio_visual_sound = |key: &str| {
            audio_visual
                .and_then(|s| s.get(key))
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("none"))
        };
        let bomb_ticking_sound = audio_visual_sound("BombTickingSound");
        let bomb_attach_sound = audio_visual_sound("BombAttachSound");
        // Rules ReadAI6739E5..673A31 is independent of ReadGeneral.
        // Constructor66760E..66761E supplies PathDelay0.016 and blockage60.
        let ai = ini.section("AI");
        let path_delay = ai
            .map(|s| s.read_double("PathDelay", defaults.path_delay))
            .unwrap_or(defaults.path_delay);
        let blockage_path_delay_ticks = ai
            .map(|s| s.read_int("BlockagePathDelay", defaults.blockage_path_delay_ticks))
            .unwrap_or(defaults.blockage_path_delay_ticks);
        let Some(general) = ini.section("General") else {
            return Self {
                iq_production,
                display_cruise_height,
                condition_yellow: condition_yellow_native,
                condition_red: condition_red_native,
                path_delay,
                blockage_path_delay_ticks,
                bomb_ticking_sound,
                bomb_attach_sound,
                ..defaults
            };
        };
        // Combat-only globals are read in the late [CombatDamage] pass.
        let combat_damage = ini.section("CombatDamage");
        // AI IQ thresholds live in their own [IQ] read.
        let iq = ini.section("IQ");
        // Base-planning/credit controls live in the independent [AI] read.
        // Genetic Mutator warhead references are read by [SpecialWeapons].
        let special_weapons = ini.section("SpecialWeapons");
        // INI parser already strips everything after `;` (Westwood comment
        // marker), so values like `WarpOut=WARPOUT;WAKE2` are read as
        // `WARPOUT` — matching gamemd's behaviour. Rate is filled in later
        // from art.ini in `resolve_art_rates`.
        let parse_anim_name = |key: &str, default: &str| -> String {
            general
                .get(key)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(default)
                .to_string()
        };
        let mut infantry_death_anims = defaults.infantry_death_anims.clone();
        for (index, key, fallback) in [
            (3, "InfantryExplode", "S_BANG34"),
            (4, "FlamingInfantry", "FLAMEGUY"),
            (6, "InfantryHeadPop", "YURIDIE"),
            (7, "InfantryNuked", "NUKEDIE"),
            (8, "InfantryVirus", "VIRUSD"),
            (9, "InfantryMutate", "GENDEATH"),
            (10, "InfantryBrute", "BRUTDIE"),
        ] {
            infantry_death_anims[index] = Some(parse_anim_name(key, fallback));
        }
        infantry_death_anims[5] = Some(
            ini.section("Animations")
                .and_then(|section| section.get_values().get(1).copied())
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .unwrap_or("ELECTRO")
                .to_string(),
        );
        // [General] damage-Spark spawn probabilities (verified ctor defaults
        // 0.02/0.01; stock INI omits them). Raw doubles, not percentages. Bound
        // before `Self` so each band feeds both its stored value and its derived
        // integer roll threshold (a struct literal can't reference sibling fields).
        let condition_red_spark_prob: f64 = general
            .get_f64("ConditionRedSparkingProbability")
            .unwrap_or(0.02);
        let condition_yellow_spark_prob: f64 = general
            .get_f64("ConditionYellowSparkingProbability")
            .unwrap_or(0.01);
        // These are ReadDouble values (single-precision parse widened to f64)
        // and the consumer's ftol boundary chops toward zero.
        let ambient_change_rate = general.read_double("AmbientChangeRate", 0.2);
        let ambient_change_step = general.read_double("AmbientChangeStep", 0.2);
        Self {
            scroll_multiplier: audio_visual
                .and_then(|s| s.get_f64("ScrollMultiplier"))
                .unwrap_or(defaults.scroll_multiplier),
            savour_delay_minutes: audio_visual
                .map(|s| s.read_double("SavourDelay", defaults.savour_delay_minutes))
                .unwrap_or(defaults.savour_delay_minutes),
            condition_red_sparking_probability: condition_red_spark_prob,
            condition_yellow_sparking_probability: condition_yellow_spark_prob,
            condition_red_spark_threshold: damage_spark_spawn_threshold(condition_red_spark_prob),
            condition_yellow_spark_threshold: damage_spark_spawn_threshold(
                condition_yellow_spark_prob,
            ),
            dumb_my_effectiveness_coefficient: general
                .get_f64("DumbMyEffectivenessCoefficient")
                .unwrap_or(defaults.dumb_my_effectiveness_coefficient),
            dumb_target_effectiveness_coefficient: general
                .get_f64("DumbTargetEffectivenessCoefficient")
                .unwrap_or(defaults.dumb_target_effectiveness_coefficient),
            dumb_target_special_threat_coefficient: general
                .get_f64("DumbTargetSpecialThreatCoefficient")
                .unwrap_or(defaults.dumb_target_special_threat_coefficient),
            dumb_target_strength_coefficient: general
                .get_f64("DumbTargetStrengthCoefficient")
                .unwrap_or(defaults.dumb_target_strength_coefficient),
            dumb_target_distance_coefficient: general
                .get_f64("DumbTargetDistanceCoefficient")
                .unwrap_or(defaults.dumb_target_distance_coefficient),
            my_effectiveness_coefficient_default: general
                .get_f64("MyEffectivenessCoefficientDefault")
                .unwrap_or(defaults.my_effectiveness_coefficient_default),
            target_effectiveness_coefficient_default: general
                .get_f64("TargetEffectivenessCoefficientDefault")
                .unwrap_or(defaults.target_effectiveness_coefficient_default),
            target_special_threat_coefficient_default: general
                .get_f64("TargetSpecialThreatCoefficientDefault")
                .unwrap_or(defaults.target_special_threat_coefficient_default),
            target_strength_coefficient_default: general
                .get_f64("TargetStrengthCoefficientDefault")
                .unwrap_or(defaults.target_strength_coefficient_default),
            target_distance_coefficient_default: general
                .get_f64("TargetDistanceCoefficientDefault")
                .unwrap_or(defaults.target_distance_coefficient_default),
            threat_per_occupant: general
                .get_i32("ThreatPerOccupant")
                .unwrap_or(defaults.threat_per_occupant),
            // Passive-scan cadence, in frames. Both keys are present in stock
            // rulesmd.ini with exactly the constructor defaults (27 / 36); read
            // them rather than hardcoding so a mod's values take effect.
            dead_bodies: general
                .get_list("DeadBodies")
                .unwrap_or_default()
                .into_iter()
                .filter(|name| !name.is_empty())
                .map(str::to_string)
                .collect(),
            normal_targeting_delay: general
                .get_i32("NormalTargetingDelay")
                .map(|v| v.max(0) as u32)
                .unwrap_or(defaults.normal_targeting_delay),
            guard_area_targeting_delay: general
                .get_i32("GuardAreaTargetingDelay")
                .map(|v| v.max(0) as u32)
                .unwrap_or(defaults.guard_area_targeting_delay),
            // Gravity lives in [AudioVisual] (stock value 6). Reading it from
            // [General] silently fell back to the code default 3 — half stock
            // gravity for spark ballistics and the hover bob amplitude.
            gravity: audio_visual
                .and_then(|s| s.get_i32("Gravity"))
                .unwrap_or(defaults.gravity),
            veteran_sight: general
                .get_f64("VeteranSight")
                .unwrap_or(defaults.veteran_sight),
            veteran_combat: general
                .get_f64("VeteranCombat")
                .unwrap_or(defaults.veteran_combat),
            veteran_speed: general
                .get_f64("VeteranSpeed")
                .unwrap_or(defaults.veteran_speed),
            veteran_rof: general
                .get_f64("VeteranROF")
                .unwrap_or(defaults.veteran_rof),
            difficulty_rof: ["Easy", "Normal", "Difficult"].map(|name| {
                ini.section(name)
                    .map_or(1.0, |section| section.read_double("ROF", 1.0))
            }),
            veteran_armor: general.get_f64("VeteranArmor").unwrap_or(1.0),
            curley_shuffle: general
                .get_bool("CurleyShuffle")
                .unwrap_or(defaults.curley_shuffle),
            repair_rate_minutes: general
                .get_f64("RepairRate")
                .unwrap_or(defaults.repair_rate_minutes),
            veteran_ratio: general
                .get_f64("VeteranRatio")
                .unwrap_or(VETERAN_RATIO_DEFAULT),
            veteran_cap: general.get_f64("VeteranCap").unwrap_or(VETERAN_CAP_DEFAULT),
            computer_base_defense_response: general
                .get_i32("ComputerBaseDefenseResponse")
                .unwrap_or(defaults.computer_base_defense_response),
            // Signed [General] binding for native Rules+0xE48. The verified
            // report establishes default 5 and active-retail override 3.
            maximum_building_placement_failures: general
                .get_i32("MaximumBuildingPlacementFailures")
                .unwrap_or(defaults.maximum_building_placement_failures),
            base_defense_delay_minutes: general
                .read_double("BaseDefenseDelay", defaults.base_defense_delay_minutes),
            suspend_priority: general
                .get_i32("SuspendPriority")
                .unwrap_or(defaults.suspend_priority),
            suspend_delay_minutes: general
                .read_double("SuspendDelay", defaults.suspend_delay_minutes),
            leptons_per_sight_increase: general.get_i32("LeptonsPerSightIncrease").unwrap_or(0),
            gap_radius: general.get_i32("GapRadius").unwrap_or(10),
            reveal_by_height: general.get_bool("RevealByHeight").unwrap_or(true),
            tunnel_speed: general
                .get_f32("TunnelSpeed")
                .map(sim_from_f32)
                .unwrap_or(sim_from_f32(6.0)),
            missile_rot_var: general
                .get_f32("MissileROTVar")
                .map(sim_from_f32)
                .unwrap_or(sim_from_f32(1.0)),
            flight_level: general.get_i32("FlightLevel").unwrap_or(500),
            display_cruise_height,
            // Hover keys. gamemd reads these with the %-aware Get_Double (150% → 1.5),
            // which `get_percent` matches (it also passes bare floats like `.02` through).
            // The three time keys (bob/accel/brake) are in MINUTES; ×900 = ticks.
            hover_height: general.get_i32("HoverHeight").unwrap_or(120),
            hover_bob: general
                .get_percent("HoverBob")
                .map(sim_from_f32)
                .unwrap_or(sim_from_f32(0.04)),
            hover_boost: general
                .get_percent("HoverBoost")
                .map(sim_from_f32)
                .unwrap_or(sim_from_f32(1.5)),
            hover_acceleration: general
                .get_percent("HoverAcceleration")
                .map(sim_from_f32)
                .unwrap_or(sim_from_f32(0.02)),
            hover_brake: general
                .get_percent("HoverBrake")
                .map(sim_from_f32)
                .unwrap_or(sim_from_f32(0.03)),
            hover_dampen: general
                .get_percent("HoverDampen")
                .map(sim_from_f32)
                .unwrap_or(sim_from_f32(0.4)),
            parachute_max_fall_rate: general.get_i32("ParachuteMaxFallRate").unwrap_or(-3),
            paradrop_radius: general.get_i32("ParadropRadius").unwrap_or(1024),
            paradrop_aircraft_type: general
                .get("ParaDropPlane")
                .map(|s| s.trim().to_uppercase())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "PDPLANE".to_string()),
            parachute_shp: general
                .get("Parachute")
                .map(|s| s.trim().to_uppercase())
                .filter(|s| !s.is_empty()),
            // Resolved later in `resolve_art_rates` once art.ini is available.
            amer_paradrop_list: parse_paradrop_list(
                general,
                "AmerParaDropInf",
                "AmerParaDropNum",
                false,
                vec![("E1".to_string(), 8)],
            ),
            ally_paradrop_list: parse_paradrop_list(
                general,
                "AllyParaDropInf",
                "AllyParaDropNum",
                false,
                vec![("E1".to_string(), 6)],
            ),
            sov_paradrop_list: parse_paradrop_list(
                general,
                "SovParaDropInf",
                "SovParaDropNum",
                true,
                vec![("E2".to_string(), 9)],
            ),
            yuri_paradrop_list: parse_paradrop_list(
                general,
                "YuriParaDropInf",
                "YuriParaDropNum",
                false,
                vec![("INIT".to_string(), 6)],
            ),
            base_unit_types: general
                .get_list("BaseUnit")
                .map(|items| {
                    items
                        .into_iter()
                        .map(|s| s.trim().to_ascii_uppercase())
                        .filter(|s| !s.is_empty())
                        .collect()
                })
                .unwrap_or_else(|| defaults.base_unit_types),
            multiplayer_ai_cm: general
                .get_list("MultiplayerAICM")
                .map(|items| {
                    items
                        .into_iter()
                        .filter_map(crate::rules::ini_value::parse_read_int_value)
                        .collect()
                })
                .unwrap_or_else(|| defaults.multiplayer_ai_cm),
            pad_aircraft_types: general
                .get_list("PadAircraft")
                .map(|items| {
                    items
                        .into_iter()
                        .map(|item| item.trim().to_ascii_uppercase())
                        .filter(|item| !item.is_empty())
                        .collect()
                })
                .unwrap_or_else(|| defaults.pad_aircraft_types),
            separate_aircraft: general
                .get_bool("SeparateAircraft")
                .unwrap_or(defaults.separate_aircraft),
            // RulesClass reads this BuildingType identity from [General].
            // BuildingClass::Mission_Attack @ 0x0044ACF0 dispatches it to the
            // Prism-specific path before considering generic delayed fire.
            prism_type: general
                .get("PrismType")
                .map(|value| value.trim().to_ascii_uppercase())
                .filter(|value| !value.is_empty()),
            tiberium_grows: general.get_bool("TiberiumGrows").unwrap_or(true),
            tiberium_spreads: general.get_bool("TiberiumSpreads").unwrap_or(true),
            growth_rate_minutes: general.get_f32("GrowthRate").unwrap_or(2.0),
            attack_cursor_on_disguise: general.get_bool("AttackCursorOnDisguise").unwrap_or(false),
            default_mirage_disguises: general
                .get_list("DefaultMirageDisguises")
                .unwrap_or_default()
                .into_iter()
                .map(|value| value.to_ascii_uppercase())
                .collect(),
            infantry_blink_disguise_time: general
                .get_i32("InfantryBlinkDisguiseTime")
                .unwrap_or(0)
                .max(0) as u32,
            tree_targeting: combat_damage
                .and_then(|section| section.get_bool("TreeTargeting"))
                .unwrap_or(false),
            condition_yellow: condition_yellow_native,
            condition_red: condition_red_native,
            cloaking_stages: general.get_i32("CloakingStages").unwrap_or(9),
            cloak_delay_frames: (general.read_double("CloakDelay", 0.02) * 900.0)
                .trunc()
                .clamp(i32::MIN as f64, i32::MAX as f64) as i32,
            // RulesClass::ReadAudioVisual @ 0x006691E0 resolves CloakSound into
            // RulesClass+0x6A0. Retain the name at the data boundary; an absent
            // or empty key leaves the native invalid-index/no-play behavior.
            cloak_sound: audio_visual
                .and_then(|s| s.get("CloakSound"))
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            upgrade_veteran_sound: audio_visual
                .and_then(|s| s.get("UpgradeVeteranSound"))
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            upgrade_elite_sound: audio_visual
                .and_then(|s| s.get("UpgradeEliteSound"))
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            elite_flash_timer: audio_visual
                .and_then(|s| s.get_i32("EliteFlashTimer"))
                .unwrap_or(defaults.elite_flash_timer),
            idle_action_frequency_x1000: (audio_visual
                .map(|s| {
                    s.read_double(
                        "IdleActionFrequency",
                        STOCK_IDLE_ACTION_FREQUENCY_X1000 as f64 / 1000.0,
                    )
                })
                .unwrap_or(STOCK_IDLE_ACTION_FREQUENCY_X1000 as f64 / 1000.0)
                * 1000.0) as i64,
            building_garrisoned_sound: audio_visual
                .and_then(|s| s.get("BuildingGarrisonedSound"))
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            sell_sound: audio_visual
                .and_then(|s| s.get("SellSound"))
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            base_under_attack_sound: audio_visual
                .and_then(|s| s.get("BaseUnderAttackSound"))
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            building_die_sound: audio_visual
                .and_then(|s| s.get("BuildingDieSound"))
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            building_damage_sound: audio_visual
                .and_then(|s| s.get("BuildingDamageSound"))
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            // `CCINIClass::ReadSoundList @ 0x00525430`: `ReadString(..., "",
            // buf, 0x80)` first (so the whole value is byte-cut and trimmed
            // once), then `strtok` on `","` (`0x00817F70`) with the tokens
            // left untrimmed. Resolution against the Voc registry
            // (`VocClass::FindPtrByName`, which drops unresolvable names) is
            // deferred to the app layer, where the registry lives.
            lightning_sounds: audio_visual
                .map(|section| section.read_string("LightningSounds", "", 0x80))
                .map(|value| {
                    value
                        .split(',')
                        .filter(|token| !token.is_empty())
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default(),
            credit_ticks: audio_visual
                .map(|section| section.read_string("CreditTicks", "", 0x80))
                .map(|value| {
                    value
                        .split(',')
                        .filter(|token| !token.is_empty())
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default(),
            storm_sound: audio_visual
                .and_then(|s| s.get("StormSound"))
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            // `RulesClass::ReadGeneral @ 0x0067107F` passes the field's own
            // current value as the default, so an absent key keeps the
            // constructor's `true` (see the field doc).
            lightning_print_text: general.get_bool("LightningPrintText").unwrap_or(true),
            dig_sound: audio_visual
                .and_then(|s| s.get("DigSound"))
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            psychic_dominator_activate_sound: audio_visual
                .and_then(|s| s.get("PsychicDominatorActivateSound"))
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            genetic_mutator_activate_sound: audio_visual
                .and_then(|s| s.get("GeneticMutatorActivateSound"))
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            psychic_reveal_activate_sound: audio_visual
                .and_then(|s| s.get("PsychicRevealActivateSound"))
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            chute_sound: audio_visual
                .and_then(|s| s.get("ChuteSound"))
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            bunker_walls_down_sound: audio_visual
                .and_then(|s| s.get("BunkerWallsDownSound"))
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            bunker_walls_up_sound: audio_visual
                .and_then(|s| s.get("BunkerWallsUpSound"))
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            gui_main_button_sound: audio_visual
                .and_then(|s| s.get("GUIMainButtonSound"))
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            gui_move_in_sound: audio_visual
                .and_then(|s| s.get("GUIMoveInSound"))
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            gui_move_out_sound: audio_visual
                .and_then(|s| s.get("GUIMoveOutSound"))
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            generic_click_sound: audio_visual
                .and_then(|s| s.get("GenericClick"))
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            generic_beep_sound: audio_visual
                .and_then(|s| s.get("GenericBeep"))
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            gui_checkbox_sound: audio_visual
                .and_then(|s| s.get("GUICheckboxSound"))
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            ore_twinkle: {
                let value = general.read_string("OreTwinkle", "", 0x80);
                (!value.is_empty()).then_some(value)
            },
            ore_twinkle_chance: audio_visual
                .and_then(|s| s.get_i32("OreTwinkleChance"))
                .unwrap_or(defaults.ore_twinkle_chance),
            gui_tab_sound: audio_visual
                .and_then(|s| s.get("GUITabSound"))
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            incoming_message_sound: audio_visual
                .and_then(|s| s.get("IncomingMessage"))
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            message_delay_minutes: audio_visual
                .and_then(|s| s.get_f32("MessageDelay"))
                .unwrap_or(0.6),
            speak_delay_minutes: audio_visual
                .and_then(|s| s.get_f64("SpeakDelay"))
                .unwrap_or(0.0),
            gui_combo_open_sound: audio_visual
                .and_then(|s| s.get("GUIComboOpenSound"))
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            gui_combo_close_sound: audio_visual
                .and_then(|s| s.get("GUIComboCloseSound"))
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            direct_rocking_coefficient: audio_visual
                .and_then(|s| s.get_f32("DirectRockingCoefficient"))
                .map(sim_from_f32)
                .unwrap_or(SimFixed::lit("1.5")),
            fallback_coefficient: audio_visual
                .and_then(|s| s.get_f32("FallBackCoefficient"))
                .map(sim_from_f32)
                .unwrap_or(SimFixed::lit("0.1")),
            // ChronoInSound/ChronoOutSound live in [AudioVisual], not [General].
            // No hardcoded fallback: a genuinely-absent key means no fallback
            // sound (silence), matching gamemd. Stock ships these keys present
            // (= ChronoMinerTeleport), so stock audio is unchanged.
            chrono_in_sound: audio_visual
                .and_then(|s| s.get("ChronoInSound"))
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
            chrono_out_sound: audio_visual
                .and_then(|s| s.get("ChronoOutSound"))
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
            bomb_ticking_sound,
            bomb_attach_sound,
            warp_in: AnimRef {
                name: parse_anim_name("WarpIn", "WARPIN"),
                frame_delay: defaults.warp_in.frame_delay,
            },
            warp_out: AnimRef {
                name: parse_anim_name("WarpOut", "WARPOUT"),
                frame_delay: defaults.warp_out.frame_delay,
            },
            warp_away: AnimRef {
                name: parse_anim_name("WarpAway", "WARPAWAY"),
                frame_delay: defaults.warp_away.frame_delay,
            },
            chrono_sparkle1: AnimRef {
                name: parse_anim_name("ChronoSparkle1", "CHRONOSK"),
                frame_delay: defaults.chrono_sparkle1.frame_delay,
            },
            wake: AnimRef {
                name: parse_anim_name("Wake", "WAKE1"),
                frame_delay: defaults.wake.frame_delay,
            },
            move_flash: AnimRef {
                name: parse_anim_name("MoveFlash", "RING"),
                frame_delay: defaults.move_flash.frame_delay,
            },
            infantry_death_anims,
            damage_delay_minutes: general.get_f32("DamageDelay").unwrap_or(1.0),
            spy_power_blackout_frames: general.get_i32("SpyPowerBlackout").unwrap_or(1000).max(0)
                as u32,
            damage_fire_types: general
                .get_list("DamageFireTypes")
                .map(|list| {
                    list.into_iter()
                        .filter(|s| !s.is_empty())
                        .map(|s| AnimRef {
                            name: s.to_uppercase(),
                            frame_delay: DEFAULT_ANIM_FRAME_DELAY,
                        })
                        .collect()
                })
                .unwrap_or_default(),
            barrel_particle: general
                .get("BarrelParticle")
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
            tiberium_short_scan: general.get_i32("TiberiumShortScan").unwrap_or(6),
            tiberium_long_scan: general.get_i32("TiberiumLongScan").unwrap_or(48),
            slave_miner_short_scan: general.get_i32("SlaveMinerShortScan").unwrap_or(8),
            slave_miner_slave_scan: general.get_i32("SlaveMinerSlaveScan").unwrap_or(14),
            drain_money_frame_delay: general.get_i32("DrainMoneyFrameDelay").unwrap_or(30),
            drain_money_amount: general.get_i32("DrainMoneyAmount").unwrap_or(30),
            slave_miner_long_scan: general.get_i32("SlaveMinerLongScan").unwrap_or(48),
            slave_miner_scan_correction: general.get_i32("SlaveMinerScanCorrection").unwrap_or(3),
            slave_miner_kick_frame_delay: general
                .get_i32("SlaveMinerKickFrameDelay")
                .unwrap_or(150)
                .max(0) as u32,
            harvester_too_far_distance: general.get_i32("HarvesterTooFarDistance").unwrap_or(5),
            chrono_harv_too_far_distance: general.get_i32("ChronoHarvTooFarDistance").unwrap_or(50),
            harvester_load_rate: general.get_i32("HarvesterLoadRate").unwrap_or(2),
            harvester_dump_frames: {
                // gamemd reads HarvesterDumpRate with ReadDouble and gates on
                // `rate × 900 <= accumulator`; the accumulator is integer-stepped,
                // so the first crossing is ceil(rate × 900). Take the ceiling at
                // full double precision to match that crossing exactly (no tenths
                // rounding). Clamp to u16::MAX to keep the frame threshold in range.
                let rate = general.get_f64("HarvesterDumpRate").unwrap_or(0.016);
                (rate * 900.0).clamp(0.0, u16::MAX as f64).ceil() as u16
            },
            chrono_delay: general.get_i32("ChronoDelay").unwrap_or(60),
            chrono_reinf_delay: general.get_i32("ChronoReinfDelay").unwrap_or(180),
            chrono_distance_factor: general.get_i32("ChronoDistanceFactor").unwrap_or(48),
            chrono_trigger: general.get_bool("ChronoTrigger").unwrap_or(true),
            chrono_minimum_delay: general.get_i32("ChronoMinimumDelay").unwrap_or(16),
            chrono_range_minimum: general.get_i32("ChronoRangeMinimum").unwrap_or(0),
            // Parse-time float -> fixed-point ppm (mirrors the IncomeMult parse); the runtime
            // bonus math is all integer. Full precision — no whole-percent quantize.
            purifier_bonus_ppm: (general.get_percent("PurifierBonus").unwrap_or(0.25) as f64
                * INCOME_PPM_SCALE as f64)
                .round() as i64,
            ai_virtual_purifiers: {
                let defaults = [4, 2, 0];
                general
                    .get("AIVirtualPurifiers")
                    .and_then(|raw| {
                        let parsed: Vec<i32> = raw
                            .split(',')
                            .map(|s| s.trim())
                            .filter(|s| !s.is_empty())
                            .filter_map(|s| s.parse::<i32>().ok())
                            .collect();
                        if parsed.len() == 3 {
                            Some([parsed[0], parsed[1], parsed[2]])
                        } else {
                            None
                        }
                    })
                    .unwrap_or(defaults)
            },
            allied_survivor_divisor: general.get_i32("AlliedSurvivorDivisor").unwrap_or(500),
            soviet_survivor_divisor: general.get_i32("SovietSurvivorDivisor").unwrap_or(250),
            third_survivor_divisor: general.get_i32("ThirdSurvivorDivisor").unwrap_or(750),
            allied_crew: general_type_name(general, "AlliedCrew"),
            soviet_crew: general_type_name(general, "SovietCrew"),
            third_crew: general_type_name(general, "ThirdCrew"),
            technician: general_type_name(general, "Technician"),
            engineer_infantry: general_type_name(general, "Engineer"),
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
            ship_sinking_weight: general
                .get_f32("ShipSinkingWeight")
                .map(sim_from_f32)
                .unwrap_or(defaults.ship_sinking_weight),
            tracked_uphill: SimFixed::from_num(general.read_double(
                "TrackedUphill",
                defaults.tracked_uphill.to_num::<f64>(),
            )),
            tracked_downhill: SimFixed::from_num(general.read_double(
                "TrackedDownhill",
                defaults.tracked_downhill.to_num::<f64>(),
            )),
            wheeled_uphill: SimFixed::from_num(general.read_double(
                "WheeledUphill",
                defaults.wheeled_uphill.to_num::<f64>(),
            )),
            wheeled_downhill: SimFixed::from_num(general.read_double(
                "WheeledDownhill",
                defaults.wheeled_downhill.to_num::<f64>(),
            )),
            // RulesClass's AudioVisual pass stores these ReadDouble values as
            // signed milliunits after the active x87 chop-toward-zero conversion.
            extra_unit_light: (audio_visual
                .map(|section| {
                    section.read_double("ExtraUnitLight", defaults.extra_unit_light as f64 / 1000.0)
                })
                .unwrap_or(defaults.extra_unit_light as f64 / 1000.0)
                * 1000.0) as i32,
            extra_infantry_light: (audio_visual
                .map(|section| {
                    section.read_double(
                        "ExtraInfantryLight",
                        defaults.extra_infantry_light as f64 / 1000.0,
                    )
                })
                .unwrap_or(defaults.extra_infantry_light as f64 / 1000.0)
                * 1000.0) as i32,
            extra_aircraft_light: (audio_visual
                .map(|section| {
                    section.read_double(
                        "ExtraAircraftLight",
                        defaults.extra_aircraft_light as f64 / 1000.0,
                    )
                })
                .unwrap_or(defaults.extra_aircraft_light as f64 / 1000.0)
                * 1000.0) as i32,
            close_enough: general.read_range("CloseEnough", defaults.close_enough),
            // URepairRate= is in minutes. Convert to ticks: minutes * 60 * 15 ticks/sec.
            unit_repair_rate_ticks: general
                .get_f32("URepairRate")
                .map(|minutes| {
                    (minutes * 60.0 * (crate::util::fixed_math::RA2_LOGIC_FRAMES_PER_SECOND as f32))
                        .round()
                        .max(1.0) as u32
                })
                .unwrap_or(defaults.unit_repair_rate_ticks),
            repair_step: general
                .get_i32("RepairStep")
                .unwrap_or(defaults.repair_step as i32)
                .max(1) as u16,
            repair_percent: general
                .get_percent("RepairPercent")
                .map(|frac| (frac * 100.0).round() as u16)
                .unwrap_or(defaults.repair_percent),
            reload_rate_ticks: general
                .get_f32("ReloadRate")
                .map(|minutes| {
                    (minutes * 60.0 * (crate::util::fixed_math::RA2_LOGIC_FRAMES_PER_SECOND as f32))
                        .round()
                        .max(1.0) as u32
                })
                .unwrap_or(defaults.reload_rate_ticks),
            path_delay,
            blockage_path_delay_ticks,
            // ReadGeneral670235..670267 uses the same475D70 signed vector
            // reader as the existing Recalc difficulty tables below.
            ai_auto_deploy_frame_delay: read_retained_difficulty_vector(
                general,
                "AIAutoDeployFrameDelay",
            ),
            // PlayerScatter belongs to the [CombatDamage] read, IQ Scatter to
            // the [IQ] read; neither is a [General] key.
            player_scatter: combat_damage
                .and_then(|s| s.get_bool("PlayerScatter"))
                .unwrap_or(defaults.player_scatter),
            player_return_fire: combat_damage
                .and_then(|s| s.get_bool("PlayerReturnFire"))
                .unwrap_or(defaults.player_return_fire),
            iq_scatter: iq
                .and_then(|s| s.get_i32("Scatter"))
                .unwrap_or(defaults.iq_scatter),
            max_iq_levels: iq
                .and_then(|s| s.get_i32("MaxIQLevels"))
                .unwrap_or(defaults.max_iq_levels),
            iq_production,
            iq_repair_sell: iq
                .and_then(|s| s.get_i32("RepairSell"))
                .unwrap_or(defaults.iq_repair_sell),
            iq_sell_back: iq
                .and_then(|s| s.get_i32("SellBack"))
                .unwrap_or(defaults.iq_sell_back),
            credit_reserve: ai
                .and_then(|s| s.get_i32("CreditReserve"))
                .unwrap_or(defaults.credit_reserve),
            concrete_walls: general
                .get_list("ConcreteWalls")
                .map(|list| {
                    list.into_iter()
                        .filter(|s| !s.is_empty())
                        .map(|s| s.to_ascii_uppercase())
                        .collect()
                })
                .unwrap_or_default(),
            cliff_back_impassability: general.get_i32("CliffBackImpassability").unwrap_or(2) as u8,
            lightning_storm_duration: general.get_i32("LightningStormDuration").unwrap_or(180),
            lightning_damage: general.get_i32("LightningDamage").unwrap_or(250),
            lightning_deferment: general.get_i32("LightningDeferment").unwrap_or(250),
            lightning_hit_delay: general.get_i32("LightningHitDelay").unwrap_or(10).max(1),
            lightning_scatter_delay: general.get_i32("LightningScatterDelay").unwrap_or(5).max(1),
            lightning_cell_spread: general.get_i32("LightningCellSpread").unwrap_or(10),
            lightning_separation: general.get_i32("LightningSeparation").unwrap_or(3),
            lightning_warhead: general
                .get("LightningWarhead")
                .unwrap_or("IonWH")
                .to_string(),
            ambient_change_rate_nonzero: ambient_change_rate != 0.0,
            ambient_change_interval_frames: (ambient_change_rate * 900.0) as i32,
            ambient_change_step: (ambient_change_step * 100.0) as i32,
            iron_curtain_duration: combat_damage
                .and_then(|s| s.get_i32("IronCurtainDuration"))
                .unwrap_or(750) as u32,
            iron_curtain_invoke_anim: general
                .get("IronCurtainInvokeAnim")
                .unwrap_or("IRONBLST")
                .to_string(),
            ion_blast_anim: general.get("IonBlast").unwrap_or("").trim().to_string(),
            force_shield_radius: general.get_i32("ForceShieldRadius").unwrap_or(4) as u32,
            force_shield_duration: general.get_i32("ForceShieldDuration").unwrap_or(500) as u32,
            force_shield_blackout_duration: general
                .get_i32("ForceShieldBlackoutDuration")
                .unwrap_or(1000) as u32,
            force_shield_fade_sound_time: general
                .get_i32("ForceShieldPlayFadeSoundTime")
                .unwrap_or(75) as u32,
            force_shield_invoke_anim: general
                .get("ForceShieldInvokeAnim")
                .unwrap_or("FORCSHLD")
                .to_string(),
            psychic_reveal_radius: combat_damage
                .and_then(|section| section.get_i32("PsychicRevealRadius"))
                .unwrap_or(15) as u32,
            mutate_warhead: special_weapons
                .and_then(|section| section.get("MutateWarhead"))
                .unwrap_or("Mutate")
                .to_string(),
            mutate_explosion_warhead: special_weapons
                .and_then(|section| section.get("MutateExplosionWarhead"))
                .unwrap_or("MutateExplosion")
                .to_string(),
            mutate_explosion: general.get_bool("MutateExplosion").unwrap_or(true),
            metallic_debris: general
                .get("MetallicDebris")
                .map(|v| {
                    v.split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>()
                })
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| Self::default().metallic_debris),
        }
    }

    /// Resolve animation playback rates from art.ini sections.
    ///
    /// Called after both rules.ini and art.ini are loaded. Looks up each
    /// anim's own `[ANIM_NAME]` section for its native `Rate=` frame delay.
    pub fn resolve_art_rates(&mut self, art_ini: &IniFile) {
        fn rate_from_section(ini: &IniFile, name: &str, fallback: u16) -> u16 {
            ini.section(name)
                .and_then(|s| s.get_i32("Rate"))
                .map(crate::rules::art_data::art_rate_to_logic_frames)
                .unwrap_or(fallback)
        }
        self.warp_in.frame_delay =
            rate_from_section(art_ini, &self.warp_in.name, DEFAULT_ANIM_FRAME_DELAY);
        self.warp_out.frame_delay =
            rate_from_section(art_ini, &self.warp_out.name, DEFAULT_ANIM_FRAME_DELAY);
        self.warp_away.frame_delay =
            rate_from_section(art_ini, &self.warp_away.name, DEFAULT_ANIM_FRAME_DELAY);
        self.chrono_sparkle1.frame_delay = rate_from_section(
            art_ini,
            &self.chrono_sparkle1.name,
            DEFAULT_ANIM_FRAME_DELAY,
        );
        self.wake.frame_delay =
            rate_from_section(art_ini, &self.wake.name, DEFAULT_ANIM_FRAME_DELAY);
        self.move_flash.frame_delay =
            rate_from_section(art_ini, &self.move_flash.name, DEFAULT_ANIM_FRAME_DELAY);
        log::info!(
            "Warp anim frame delays: {}={}, {}={}, {}={}, wake: {}={}",
            self.warp_in.name,
            self.warp_in.frame_delay,
            self.warp_out.name,
            self.warp_out.frame_delay,
            self.warp_away.name,
            self.warp_away.frame_delay,
            self.wake.name,
            self.wake.frame_delay,
        );
        for fire in &mut self.damage_fire_types {
            fire.frame_delay = rate_from_section(art_ini, &fire.name, DEFAULT_ANIM_FRAME_DELAY);
        }
        if !self.damage_fire_types.is_empty() {
            log::info!(
                "DamageFireTypes: {} types ({})",
                self.damage_fire_types.len(),
                self.damage_fire_types
                    .iter()
                    .map(|f| format!("{}={}", f.name, f.frame_delay))
                    .collect::<Vec<_>>()
                    .join(", "),
            );
        }
    }
}

impl ProductionRules {
    fn from_ini(ini: &IniFile) -> Self {
        let Some(general) = ini.section("General") else {
            return Self::default();
        };

        // Fallback for an absent key matches the engine's constructor default
        // (BuildSpeed 1.0). Retail rulesmd.ini always supplies its own value
        // (.7), so this fallback fires only for a non-retail INI missing the key.
        let bs = general.get_f32("BuildSpeed").unwrap_or(1.0);
        let mf = general.get_f32("MultipleFactory").unwrap_or(0.8);
        let lpp = general.get_f32("LowPowerPenaltyModifier").unwrap_or(1.0);
        let min_lp = general.get_f32("MinLowPowerProductionSpeed").unwrap_or(0.5);
        let max_lp = general.get_f32("MaxLowPowerProductionSpeed").unwrap_or(0.9);
        let wall_coeff = general.get_f32("WallBuildSpeedCoefficient").unwrap_or(1.0);
        let result = Self {
            build_speed: bs,
            multiple_factory: mf,
            low_power_penalty_modifier: lpp,
            min_low_power_production_speed: min_lp,
            max_low_power_production_speed: max_lp,
            multiple_factory_ppm: f32_to_ppm(mf, 0.01),
            low_power_penalty_modifier_ppm: f32_to_ppm(lpp, 0.0),
            min_low_power_production_speed_ppm: f32_to_ppm(min_lp, 0.0),
            max_low_power_production_speed_ppm: f32_to_ppm(max_lp.max(min_lp), 0.0),
            build_speed_x1000: (bs.max(0.01) as f64 * 1000.0).round() as u64,
            wall_build_speed_coefficient: wall_coeff,
        };
        log::info!(
            "ProductionRules: BuildSpeed={}, MultipleFactory={}, LowPowerPenalty={}",
            result.build_speed,
            result.multiple_factory,
            result.low_power_penalty_modifier,
        );
        result
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
    /// Case-insensitive native BuildingType registry identity to its ordered index.
    building_type_indices: HashMap<String, i32>,
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
    /// Maps prerequisite alias (uppercase, e.g. "POWER") → list of building IDs
    /// (uppercase) that satisfy it. Built from [General] PrerequisiteXxx keys.
    /// RA2 uses these so that Prerequisite=POWER means "any power plant" rather
    /// than a specific building ID.
    pub prerequisite_groups: HashMap<String, Vec<String>>,
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
    /// Uppercase name → `ParticleTypeId` for case-insensitive lookup.
    particle_types_by_name: HashMap<String, ParticleTypeId>,
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
    /// Retained art.ini registry. Populated by the app loading path (`app::loading::init`) after `merge_art_data`
    /// so dispatchers (e.g. smudge spawning) can read per-anim spawn flags.
    pub art_registry: crate::rules::art_data::ArtRegistry,
    /// Existing AnimTypes for Building451890's lookup-only427CB0 gate.
    /// Membership is independent of a same-named art section or loaded SHP.
    pub anim_type_names: BTreeSet<String>,
    /// Ordered native registry receipt: whether this type actually reached a
    /// successful fixed-ART ReadINI before the final rules pass completed.
    pub anim_type_art_read_states: Vec<(String, bool)>,
    /// GPU-independent SHP frame counts used by particle timing. Bound once
    /// from the active assets and ART data.
    effect_assets: crate::rules::effect_asset_catalog::EffectAssetCatalog,
    /// Raw terrain SHP counts used by authoritative TIBTRE animation timing.
    /// Presentation keeps its separate body-frame projection.
    terrain_spawner_assets: crate::rules::terrain_asset_catalog::TerrainSpawnerAssetCatalog,
    /// Complete immutable per-object animation timing catalog. Gameplay reads
    /// this rules-owned resource directly; presentation cannot replace timing
    /// on an individual frame.
    animation_sequences: BTreeMap<String, crate::rules::animation_sequence::SequenceSet>,
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

/// Project one native AI planning type list through its `char[128]` reader.
///
/// gamemd-derived: `RulesClass__ReadAI @ 0x00672AE0`, seven list blocks
/// `0x00672B14..0x00673058` and `0x0067368C..0x0067375B`, plus
/// `RulesClass__ReadGeneral @ 0x0066D530` HarvesterUnit block
/// `0x0066F8C8..0x0066F9CB`. Each calls `CCINIClass__ReadString @ 0x00528A10`
/// with length `0x80`, trims the complete copied buffer, then tokenizes with
/// comma as the sole `strtok` delimiter. `IniFile::from_bytes` stores each
/// source byte as one zero-extended scalar, so narrowing before truncation
/// preserves the native payload-byte boundary even when Rust UTF-8 would not.
fn parse_native_type_list_source_tokens(value: &str) -> Vec<String> {
    const NATIVE_PAYLOAD_BYTES: usize = 127;

    let Some(mut copied) = value
        .chars()
        .map(|character| u8::try_from(u32::from(character)).ok())
        .collect::<Option<Vec<_>>>()
    else {
        // Production INI values come through `IniFile::from_bytes` and are
        // always in the reversible byte domain. A direct Unicode-only test
        // value has no native narrow-byte identity and must not alias a
        // registered BuildingType.
        return Vec::new();
    };
    copied.truncate(NATIVE_PAYLOAD_BYTES);

    let first = copied.iter().position(|byte| *byte > b' ');
    let Some(first) = first else {
        return Vec::new();
    };
    let last = copied
        .iter()
        .rposition(|byte| *byte > b' ')
        .expect("the first non-control byte also supplies the last");

    copied[first..=last]
        .split(|byte| *byte == b',')
        // CRT `strtok` coalesces repeated delimiters and emits no empty token
        // for leading, repeated, or trailing commas.
        .filter(|token| !token.is_empty())
        .filter(|token| {
            !token.eq_ignore_ascii_case(b"none") && !token.eq_ignore_ascii_case(b"<none>")
        })
        .map(crate::util::native_string::widen_bytes)
        .collect()
}

/// Parse the signed DynamicVector payload used by Recalc difficulty tables.
///
/// gamemd-derived: `DifficultyClass__ReadINI_IntVector @ 0x00475D70`, reached
/// by `RulesClass__ReadGeneral @ 0x0066D530` for AISlaveMinerNumber
/// `0x00670585..0x006705B7`, AIExtraRefineries `0x006705F9..0x0067062A`, and
/// the three BaseDefenseCounts vectors `0x00670013..0x006700BE`. The reader has
/// a native `char[512]` buffer, whole-buffer trim, comma `strtok`, and CRT atoi.
fn read_retained_difficulty_vector(
    section: &crate::rules::ini_parser::IniSection,
    key: &str,
) -> Vec<i32> {
    // ReadGeneral670232..67026C copies the existing vector before475D70.
    // Missing/empty ReadString retains it; a nonempty delimiter-only value
    // replaces it with an empty vector. Original42-row deploy-rules corpus.
    let mut result = Vec::new();
    let mut read = |value: &str| {
        let copied = crate::rules::ini_value::truncate_bytes(value, 511);
        if !crate::rules::ini_value::strtrim_ascii(copied).is_empty() {
            result = parse_native_difficulty_int_vector(value);
        }
    };
    if let Some(values) = section.projected_values(key) {
        for value in values {
            read(value);
        }
    } else if let Some(value) = section.get(key) {
        read(value);
    }
    result
}

fn parse_native_difficulty_int_vector(value: &str) -> Vec<i32> {
    const NATIVE_PAYLOAD_BYTES: usize = 511;

    let Some(mut copied) = value
        .chars()
        .map(|character| u8::try_from(u32::from(character)).ok())
        .collect::<Option<Vec<_>>>()
    else {
        return Vec::new();
    };
    copied.truncate(NATIVE_PAYLOAD_BYTES);

    let Some(first) = copied.iter().position(|byte| *byte > b' ') else {
        return Vec::new();
    };
    let last = copied
        .iter()
        .rposition(|byte| *byte > b' ')
        .expect("the first non-control byte also supplies the last");

    copied[first..=last]
        .split(|byte| *byte == b',')
        .filter(|token| !token.is_empty())
        .map(native_atoi_bytes)
        .collect()
}

/// CRT `atoi` byte-domain projection: skip exactly ASCII whitespace, accept an
/// optional sign, consume the decimal prefix, and retain 32-bit wrapping.
fn native_atoi_bytes(value: &[u8]) -> i32 {
    let mut index = value
        .iter()
        .take_while(|&&byte| matches!(byte, b'\t'..=b'\r' | b' '))
        .count();
    let negative = value.get(index) == Some(&b'-');
    if value
        .get(index)
        .is_some_and(|byte| matches!(*byte, b'-' | b'+'))
    {
        index += 1;
    }
    let mut result = 0u32;
    let mut any = false;
    while let Some(byte) = value.get(index).filter(|byte| byte.is_ascii_digit()) {
        any = true;
        result = result
            .wrapping_mul(10)
            .wrapping_add(u32::from(*byte - b'0'));
        index += 1;
    }
    if !any {
        0
    } else if negative {
        result.wrapping_neg() as i32
    } else {
        result as i32
    }
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
        rules.anim_type_art_read_states = processed
            .anim_type_art_read_states()
            .map(|(name, read)| (name.to_owned(), read))
            .collect();
        // The registry processor owns native read timing and retained values;
        // the runtime definition receives that result, not another ART read.
        for (name, flat) in processed.projectile_flat_states() {
            if let Some(projectile) = rules
                .projectiles
                .values_mut()
                .find(|projectile| projectile.id.eq_ignore_ascii_case(name))
            {
                projectile.flat = flat;
            }
        }
        rules.source_ini_hash = processed.content_hash();
        Ok(rules)
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
        let mut building_type_indices: HashMap<String, i32> = HashMap::new();
        let mut infantry_ids: Vec<String> = Vec::new();
        let mut vehicle_ids: Vec<String> = Vec::new();
        let mut aircraft_ids: Vec<String> = Vec::new();
        let mut building_ids: Vec<String> = Vec::new();
        let production: ProductionRules = ProductionRules::from_ini(ini);
        let general: GeneralRules = GeneralRules::from_ini(ini);
        let ai_base_spacing = ini
            .section("AI")
            .and_then(|section| section.get_i32("AIBaseSpacing"))
            .unwrap_or(1);
        let parse_named_list = |section: &str, key: &str| -> Vec<String> {
            ini.section(section)
                .and_then(|section| section.get_list(key))
                .unwrap_or_default()
                .into_iter()
                .filter(|entry| !entry.is_empty())
                .map(str::to_string)
                .collect()
        };
        let shipyard_types = parse_named_list("General", "Shipyard");
        // gamemd-derived: `RulesClass__Process` reads type registries first
        // (`ReadBuildingTypes` 0x00668E78), then `RulesClass__ReadAI @
        // 0x00672AE0` (0x00668EC8). All seven BasePlan lists use the exact
        // char[128]/whole-buffer-trim/comma-only path owned by their blocks at
        // 0x00672B14..0x00673058 and 0x0067368C..0x0067375B. None falls back
        // to `[General]`, and individual tokens are not trimmed.
        let parse_planning_list = |section_name: &str, key: &str| {
            ini.section(section_name)
                .and_then(|section| section.get(key))
                .map(parse_native_type_list_source_tokens)
                .unwrap_or_default()
        };
        let build_const_source_tokens = parse_planning_list("AI", "BuildConst");
        let build_power_source_tokens = parse_planning_list("AI", "BuildPower");
        let build_refinery_source_tokens = parse_planning_list("AI", "BuildRefinery");
        let build_barracks_source_tokens = parse_planning_list("AI", "BuildBarracks");
        let build_tech_source_tokens = parse_planning_list("AI", "BuildTech");
        let build_weapons_source_tokens = parse_planning_list("AI", "BuildWeapons");
        let build_radar_source_tokens = parse_planning_list("AI", "BuildRadar");
        let harvester_unit_source_tokens = parse_planning_list("General", "HarvesterUnit");
        let parse_difficulty_vector = |key: &str| {
            ini.section("General")
                .and_then(|section| section.get(key))
                .map(parse_native_difficulty_int_vector)
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
            .section("General")
            .and_then(|section| section.get_i32("AINavalYardAdjacency"))
            .unwrap_or(20);
        let initial_veteran = ini
            .section("SpecialFlags")
            .and_then(|section| section.get_bool("InitialVeteran"))
            .unwrap_or(false);
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
            .section("WallModel")
            .and_then(|s| s.get_bool("AlliedWallTransparency"))
            .unwrap_or(false);
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
                if category == ObjectCategory::Building {
                    building_type_indices
                        .entry(id.to_ascii_uppercase())
                        .or_insert(registry_index as i32);
                }
                if let Some(section) = ini.section(id) {
                    let mut obj: ObjectType = ObjectType::from_ini_section(id, section, category);
                    if category == ObjectCategory::Building {
                        obj.base_plan_type_index = building_type_indices
                            .get(&id.to_ascii_uppercase())
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
        let harvester_unit_types =
            resolve_registered(harvester_unit_source_tokens, ObjectCategory::Vehicle);
        for type_id in &build_const_types {
            let handle = object_category_index
                .get(&(ObjectCategory::Building, type_id.to_ascii_uppercase()))
                .copied()
                .expect("resolved BuildConst membership remains registered");
            object_list[handle.0 as usize].build_const_eligible = true;
        }

        // Step 2: Collect all weapon and warhead IDs referenced by objects.
        let (mut weapon_ids, warhead_refs) = collect_weapon_refs(&object_list);
        if let Some(default_death_weapon) = ini
            .section("CombatDamage")
            .and_then(|section| section.get("DeathWeapon"))
            .filter(|value| !value.trim().is_empty())
        {
            weapon_ids.insert(default_death_weapon.to_string());
        }

        // Step 3: Parse weapon sections.
        let mut weapons: HashMap<String, WeaponType> = HashMap::new();
        // RulesClass allocates every value in [Warheads] before the per-type
        // ReadINI pass. Keep reference-driven allocations too: later rules
        // layers and runtime-specific globals can create warheads outside the
        // explicit registry.
        let mut warhead_ids: HashSet<String> =
            parse_registry(ini, "Warheads").into_iter().collect();
        warhead_ids.extend(warhead_refs);

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

        // Step 7: Parse prerequisite alias groups from [General].
        let prerequisite_groups: HashMap<String, Vec<String>> = parse_prerequisite_groups(ini);
        log::info!("Prerequisite groups: {} aliases", prerequisite_groups.len());

        // Step 8: Parse superweapon type registry.
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

        // [General] rocket type/frame slots + [CombatDamage] missile warheads.
        let missile_spawn = crate::rules::missile_spawn::MissileSpawnRules::from_ini_sections(
            ini.section("General"),
            ini.section("CombatDamage"),
        );

        // [CombatDamage] C4Delay = minutes (double). Default 0.03 = 27 ticks @ 15 fps.
        // Stored as integer ticks for lockstep-safe per-tick comparison.
        const SIM_TICKS_PER_SECOND: u32 = crate::util::fixed_math::RA2_LOGIC_FRAMES_PER_SECOND;
        let c4_delay_ticks: u32 = ini
            .section("CombatDamage")
            .and_then(|s| s.get("C4Delay"))
            .and_then(|v| v.trim().parse::<f64>().ok())
            .map(|minutes| (minutes * 60.0 * SIM_TICKS_PER_SECOND as f64).round() as u32)
            .unwrap_or(27); // 0.03 × 60 × 15 = 27

        // Per-mission behaviour table from the [<MissionName>] sections.
        let mission_control = MissionControl::from_ini(ini);

        // Parse [TerrainTypes] registry → per-type sections (TIBTRE01, TREE01, etc.).
        let mut terrain_object_types: HashMap<String, TerrainObjectType> = HashMap::new();
        let terrain_names: Vec<String> = parse_registry(ini, "TerrainTypes");
        let tree_strength = ini
            .section("General")
            .and_then(|section| section.get_i32("TreeStrength"))
            .unwrap_or(200);
        for name in &terrain_names {
            if let Some(section) = ini.section(name) {
                terrain_object_types.insert(
                    name.to_ascii_uppercase(),
                    TerrainObjectType::from_ini_section_with_tree_strength(
                        name,
                        section,
                        tree_strength,
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

        // Step 9: Two-pass parse for [Particles] and [ParticleSystems].
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
            building_type_indices,
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
            prerequisite_groups,
            terrain_rules,
            tiberium_types,
            terrain_object_types,
            bridge_rules,
            crate_rules,
            powerups,
            garrison_rules,
            allied_wall_transparency,
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
            particle_types_by_name,
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
                .flat_map(|section| section.get_values())
                .map(|name| name.to_ascii_uppercase())
                .filter(|name| !name.is_empty())
                .collect(),
            effect_assets: crate::rules::effect_asset_catalog::EffectAssetCatalog::default(),
            terrain_spawner_assets:
                crate::rules::terrain_asset_catalog::TerrainSpawnerAssetCatalog::default(),
            animation_sequences: BTreeMap::new(),
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

    /// Resolve one scenario BasePlan token through the native BuildingType
    /// registry only, matching
    /// `BuildingTypeClass__FindIndexByName @ 0x0045E7B0` used by
    /// `FUN_0042EBE0`, and return its ordered index.
    pub(crate) fn building_type_index(&self, id: &str) -> Option<i32> {
        self.building_type_indices
            .get(&id.to_ascii_uppercase())
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

    /// BuildingType virtual `+0xAC` value. Wall sale invokes and discards it;
    /// receiver anger uses the same authority.
    pub(crate) fn building_actual_cost(&self, object: &ObjectType) -> i32 {
        let mut value = object.cost;
        if !self.general.separate_aircraft
            && let [first_id, second_id, ..] = self.general.pad_aircraft_types.as_slice()
            && let (Some(first), Some(second)) = (
                self.object_case_insensitive(first_id),
                self.object_case_insensitive(second_id),
            )
            && first
                .dock
                .first()
                .is_some_and(|dock| dock.eq_ignore_ascii_case(&object.id))
        {
            value = value.wrapping_sub(first.cost.wrapping_add(second.cost) / 2);
        }
        if let Some(free_unit) = object.free_unit.as_deref() {
            value = value
                .wrapping_sub(
                    self.object_case_insensitive(free_unit)
                        .map_or(0, |free| free.cost),
                )
                .max(0);
        }
        value
    }

    /// Deprecated: `object` is now case-insensitive. Retained as an alias so
    /// the existing call sites keep compiling without churn.
    pub fn object_case_insensitive(&self, id: &str) -> Option<&ObjectType> {
        self.object(id)
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
        b"rules-simulation-config-v5".hash(&mut hasher);
        self.source_ini_hash.hash(&mut hasher);
        self.animation_sequences.hash(&mut hasher);
        self.effect_assets.hash(&mut hasher);
        self.terrain_spawner_assets.hash(&mut hasher);
        b"art-smudge-config-v1".hash(&mut hasher);
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
        b"art-projectile-launch-config-v2".hash(&mut hasher);
        self.projectiles
            .iter()
            .map(|(id, projectile)| (id.to_ascii_uppercase(), (projectile.voxel, projectile.flat)))
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
        self.particle_types_by_name
            .get(&name.to_ascii_uppercase())
            .copied()
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

    /// Look up which building IDs satisfy a prerequisite alias (case-insensitive).
    /// Returns None if the alias is not a known prerequisite group.
    pub fn prerequisite_group(&self, alias: &str) -> Option<&[String]> {
        self.prerequisite_groups
            .get(&alias.to_ascii_uppercase())
            .map(|v| v.as_slice())
    }

    /// Whether a structure type is marked as a refinery in rules.ini.
    pub fn is_refinery_type(&self, structure_id: &str) -> bool {
        self.object_case_insensitive(structure_id)
            .is_some_and(|obj| obj.refinery)
    }

    /// Resolve a refinery's free starter unit if both the refinery and the unit exist.
    pub fn refinery_free_unit(&self, structure_id: &str) -> Option<&str> {
        let obj = self.object_case_insensitive(structure_id)?;
        if !obj.refinery {
            return None;
        }
        let free_unit = obj.free_unit.as_deref()?;
        let resolved = self.object_case_insensitive(free_unit)?;
        Some(resolved.id.as_str())
    }

    /// Whether a harvester type may dock at a specific structure according to Dock=.
    pub fn harvester_can_dock_at(&self, harvester_id: &str, structure_id: &str) -> bool {
        let Some(harvester) = self.object_case_insensitive(harvester_id) else {
            return false;
        };
        let Some(_structure) = self.object_case_insensitive(structure_id) else {
            return false;
        };
        harvester
            .dock
            .iter()
            .any(|dock| dock.eq_ignore_ascii_case(structure_id))
    }

    /// Merge art.ini data into object types (Foundation, QueueingCell, DockingOffset).
    ///
    /// In the original engine, `Foundation=` is an **art.ini-only** property — it does
    /// NOT exist in rules.ini. ObjectType defaults to "1x1" and this method overwrites
    /// it with the authoritative value from art.ini, resolved via the `Image=` key.
    /// Without this, all buildings would be 1x1 which breaks placement and rendering.
    pub fn merge_art_data(&mut self, art: &crate::rules::art_data::ArtRegistry) {
        self.art_registry = art.clone();
        self.art_registry
            .apply_anim_type_read_states(&self.anim_type_art_read_states);
        // ObjectRead 5F933B/5F962E precedes BulletRead 46C1E8. On a fresh
        // type the Object image defaults to its ID, even though Bullet's later
        // missing-Image read clears its separate rendering name. An explicit
        // Image never falls back to the type ID or follows ART Image redirects.
        for projectile in self.projectiles.values_mut() {
            let image = projectile.image.as_deref().unwrap_or(&projectile.id);
            if let Some(voxel) = art.get(image).and_then(|entry| entry.authored_voxel) {
                projectile.voxel = voxel;
            }
        }
        let ai_base_spacing = self.ai_base_spacing;
        let mut patched: u32 = 0;
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
            let rules_foundation_id = crate::rules::foundation::foundation_id(&obj.foundation);
            if let Some(entry) = entry {
                obj.to_overlay = entry.to_overlay.clone();
                if let Some(ref foundation) = entry.foundation {
                    let effective_foundation =
                        if rules_foundation_id != crate::rules::foundation::DEFAULT_FOUNDATION_ID {
                            crate::rules::foundation::foundation_name(&obj.foundation)
                        } else {
                            crate::rules::foundation::foundation_name(foundation)
                        };
                    if obj.foundation != effective_foundation {
                        log::trace!(
                            "Foundation patch: {} (image={}) {} → {}",
                            obj.id,
                            art_key,
                            obj.foundation,
                            effective_foundation,
                        );
                    }
                    obj.foundation = effective_foundation.to_string();
                    patched += 1;
                } else {
                    obj.foundation =
                        crate::rules::foundation::foundation_name(&obj.foundation).to_string();
                }
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
            "Merged art.ini → RuleSet: {} foundations, {} dock cells ({} buildings checked)",
            patched,
            dock_patched,
            buildings_checked,
        );
        let mut terrain_foundations_patched: u32 = 0;
        for terrain in self.terrain_object_types.values_mut() {
            if let Some(entry) = art.get(&terrain.name) {
                if let Some(ref foundation) = entry.foundation {
                    terrain.merge_art_foundation(foundation);
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
            if let Some(members) = sides.get_list(side_name) {
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

/// Collect all weapon and warhead IDs referenced by objects.
///
/// Returns (weapon_ids, warhead_ids) as sets (deduplicated).
fn collect_weapon_refs(objects: &[ObjectType]) -> (HashSet<String>, HashSet<String>) {
    let mut weapon_ids: HashSet<String> = HashSet::new();
    let warhead_ids: HashSet<String> = HashSet::new();

    for obj in objects.iter() {
        if let Some(ref w) = obj.primary {
            weapon_ids.insert(w.clone());
        }
        if let Some(ref w) = obj.secondary {
            weapon_ids.insert(w.clone());
        }
        if let Some(ref w) = obj.elite_primary {
            weapon_ids.insert(w.clone());
        }
        if let Some(ref w) = obj.elite_secondary {
            weapon_ids.insert(w.clone());
        }
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

    (weapon_ids, warhead_ids)
}

/// Parse prerequisite alias groups from [General] PrerequisiteXxx keys.
///
/// RA2's rules.ini defines abstract prerequisite names (POWER, RADAR, etc.)
/// that map to lists of concrete building IDs. For example:
///   PrerequisitePower=GAPOWR,NAPOWR,NANRCT
/// means any unit with `Prerequisite=POWER` is satisfied by owning any of those.
///
/// Also registers secondary aliases used in RA2 prerequisites:
/// - FACTORY / WARFACTORY → same as PrerequisiteFactory list
/// - BARRACKS / TENT → same as PrerequisiteBarracks list
fn parse_prerequisite_groups(ini: &IniFile) -> HashMap<String, Vec<String>> {
    let mut groups: HashMap<String, Vec<String>> = HashMap::new();
    let Some(general) = ini.section("General") else {
        return groups;
    };

    /// Known [General] keys and the alias name they define.
    const PREREQ_KEYS: &[(&str, &str)] = &[
        ("PrerequisitePower", "POWER"),
        ("PrerequisiteProc", "PROC"),
        ("PrerequisiteRadar", "RADAR"),
        ("PrerequisiteTech", "TECH"),
        ("PrerequisiteBarracks", "BARRACKS"),
        ("PrerequisiteFactory", "FACTORY"),
    ];

    for &(ini_key, alias) in PREREQ_KEYS {
        if let Some(list) = general.get_list(ini_key) {
            let ids: Vec<String> = list
                .into_iter()
                .filter(|s| !s.is_empty())
                .map(|s| s.to_ascii_uppercase())
                .collect();
            if !ids.is_empty() {
                groups.insert(alias.to_string(), ids);
            }
        }
    }

    // Register secondary aliases that RA2 prerequisites use interchangeably.
    if let Some(factory_list) = groups.get("FACTORY").cloned() {
        groups.insert("WARFACTORY".to_string(), factory_list);
    }
    if let Some(barracks_list) = groups.get("BARRACKS").cloned() {
        groups.insert("TENT".to_string(), barracks_list);
    }

    groups
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
        assert!((rules.production.build_speed - 0.75).abs() < 0.0001);
        assert!((rules.production.multiple_factory - 0.7).abs() < 0.0001);
        assert!((rules.production.low_power_penalty_modifier - 1.25).abs() < 0.0001);
        assert!((rules.production.min_low_power_production_speed - 0.4).abs() < 0.0001);
        assert!((rules.production.max_low_power_production_speed - 0.85).abs() < 0.0001);
        assert_eq!(rules.bridge_rules.strength, 1500);
        assert!(rules.bridge_rules.destroyable_by_default);
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
             ComputerBaseDefenseResponse=-4\n\
             BaseDefenseDelay=.125\n\
             SuspendPriority=-2\n\
             SuspendDelay=1.5\n",
        ));
        assert_eq!(parsed.computer_base_defense_response, -4);
        assert_eq!(parsed.base_defense_delay_minutes, 0.125_f32 as f64);
        assert_eq!(parsed.suspend_priority, -2);
        assert_eq!(parsed.suspend_delay_minutes, 1.5_f32 as f64);
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
        // Unspecified keys fall back to defaults.
        assert_eq!(general.iron_curtain_invoke_anim, "IRONBLST");
        assert_eq!(general.force_shield_invoke_anim, "FORCSHLD");
        assert_eq!(general.mutate_explosion_warhead, "MutateExplosion");
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
        let rules: RuleSet = RuleSet::from_ini(&ini).expect("Should parse");

        let e1: &ObjectType = rules.object("E1").expect("E1 exists");
        assert_eq!(e1.cost, 200);
        assert_eq!(e1.strength, 125);
        assert_eq!(e1.category, ObjectCategory::Infantry);
        assert_eq!(e1.primary, Some("M60".to_string()));
        assert!((e1.build_time_multiplier - 1.15).abs() < 0.0001);

        let mtnk: &ObjectType = rules.object("MTNK").expect("MTNK exists");
        assert_eq!(mtnk.cost, 700);
        assert_eq!(mtnk.category, ObjectCategory::Vehicle);
        assert_eq!(mtnk.secondary, Some("MachGun".to_string()));

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
        assert_eq!(sa.verses.len(), 11);
        assert_eq!(sa.verses[0], 100); // none: 100%
        assert_eq!(sa.verses[5], 25); // heavy: 25%

        let ap: &WarheadType = rules.warhead("AP").expect("AP exists");
        assert_eq!(ap.verses[6], 60); // wood: 60%
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
        assert_eq!(lower.percent_at_max, 50);
        assert_eq!(
            lower.verses,
            vec![100, 90, 80, 70, 60, 50, 40, 30, 20, 10, 0]
        );
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
        assert_eq!(rules.refinery_free_unit("MODPROC"), Some("MODHARV"));
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
        assert_eq!(rules.refinery_free_unit("MODPROC"), Some("UNKNOWN"));
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
             TiberiumLongScan=60\n\
             SlaveMinerShortScan=12\n\
             SlaveMinerSlaveScan=20\n\
             SlaveMinerLongScan=55\n\
             SlaveMinerScanCorrection=5\n\
             SlaveMinerKickFrameDelay=200\n\
             HarvesterTooFarDistance=8\n\
             ChronoHarvTooFarDistance=40\n\
             PurifierBonus=.30\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("Should parse");
        assert_eq!(rules.general.tiberium_short_scan, 10);
        assert_eq!(rules.general.tiberium_long_scan, 60);
        assert_eq!(rules.general.slave_miner_short_scan, 12);
        assert_eq!(rules.general.slave_miner_slave_scan, 20);
        assert_eq!(rules.general.slave_miner_long_scan, 55);
        assert_eq!(rules.general.slave_miner_scan_correction, 5);
        assert_eq!(rules.general.slave_miner_kick_frame_delay, 200);
        assert_eq!(rules.general.harvester_too_far_distance, 8);
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
        assert_eq!(rules.general.tiberium_short_scan, 6);
        assert_eq!(rules.general.tiberium_long_scan, 48);
        assert_eq!(rules.general.slave_miner_short_scan, 8);
        assert_eq!(rules.general.slave_miner_slave_scan, 14);
        assert_eq!(rules.general.slave_miner_long_scan, 48);
        assert_eq!(rules.general.slave_miner_scan_correction, 3);
        assert_eq!(rules.general.slave_miner_kick_frame_delay, 150);
        assert_eq!(rules.general.harvester_too_far_distance, 5);
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
        assert_eq!(t.animation_probability_micros, 3000);
        // TREE01 also parsed but with default flags.
        let tree = rules
            .terrain_object_type_case_insensitive("TREE01")
            .expect("TREE01 should be parsed");
        assert!(!tree.spawns_tiberium);
    }

    #[test]
    fn metallic_debris_default_matches_retail() {
        let g = GeneralRules::default();
        assert_eq!(g.metallic_debris.len(), 20);
        assert_eq!(g.metallic_debris[0], "DBRIS1LG");
        assert_eq!(g.metallic_debris[19], "DBRS10SM");
    }

    #[test]
    fn metallic_debris_parses_from_ini() {
        let ini = IniFile::from_str("[General]\nMetallicDebris=ANIM1,ANIM2,ANIM3\n");
        let g = GeneralRules::from_ini(&ini);
        assert_eq!(g.metallic_debris, vec!["ANIM1", "ANIM2", "ANIM3"]);
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
    fn test_missile_rot_var_missing_key_falls_back_to_one() {
        let ini = IniFile::from_str("[General]\nFixtureOnly=1\n");
        let general = GeneralRules::from_ini(&ini);
        assert_eq!(general.missile_rot_var, sim_from_f32(1.0));
    }

    #[test]
    fn test_missile_rot_var_stock_rules_value_parsed() {
        let ini = IniFile::from_str("[General]\nMissileROTVar=.25\n");
        let general = GeneralRules::from_ini(&ini);
        let diff = (general.missile_rot_var - SimFixed::lit("0.25")).abs();
        assert!(
            diff < SimFixed::lit("0.001"),
            "got {:?}",
            general.missile_rot_var
        );
    }

    #[test]
    fn test_missile_rot_var_parsed() {
        let ini = IniFile::from_str("[General]\nMissileROTVar=2.5\n");
        let general = GeneralRules::from_ini(&ini);
        let diff = (general.missile_rot_var - sim_from_f32(2.5)).abs();
        assert!(
            diff < SimFixed::lit("0.001"),
            "got {:?}",
            general.missile_rot_var
        );
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
            section.read_double("ConditionRed", 0.25)
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
            section.read_double("ConditionRed", 0.25).to_bits()
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

    #[test]
    fn paradrop_defaults_when_no_general_section() {
        let ini = IniFile::from_str("[Foo]\nBar=1\n");
        let g = GeneralRules::from_ini(&ini);
        assert_eq!(g.paradrop_radius, 1024);
        assert_eq!(g.paradrop_aircraft_type, "PDPLANE");
        assert_eq!(g.amer_paradrop_list, vec![("E1".to_string(), 8)]);
        assert_eq!(g.ally_paradrop_list, vec![("E1".to_string(), 6)]);
        assert_eq!(g.sov_paradrop_list, vec![("E2".to_string(), 9)]);
        assert_eq!(g.yuri_paradrop_list, vec![("INIT".to_string(), 6)]);
    }

    #[test]
    fn paradrop_explicit_values_parse() {
        let ini = ini_with_general(
            "ParadropRadius=2048\n\
             AmerParaDropInf=E1,GHOST,ENGINEER\n\
             AmerParaDropNum=6,6,6",
        );
        let g = GeneralRules::from_ini(&ini);
        assert_eq!(g.paradrop_radius, 2048);
        assert_eq!(
            g.amer_paradrop_list,
            vec![
                ("E1".to_string(), 6),
                ("GHOST".to_string(), 6),
                ("ENGINEER".to_string(), 6),
            ]
        );
    }

    #[test]
    fn paradrop_list_mismatch_falls_back_to_default() {
        let ini = ini_with_general(
            "AllyParaDropInf=E1,E2\n\
             AllyParaDropNum=5",
        );
        let g = GeneralRules::from_ini(&ini);
        assert_eq!(g.ally_paradrop_list, vec![("E1".to_string(), 6)]);
    }

    #[test]
    fn paradrop_soviet_branch_skips_count_assert() {
        // gamemd's Soviet dispatch path has no count-equality assert; mirror it.
        let ini = ini_with_general(
            "SovParaDropInf=E2,E3\n\
             SovParaDropNum=9",
        );
        let g = GeneralRules::from_ini(&ini);
        // zip up to the shorter length — only ("E2", 9) survives.
        assert_eq!(g.sov_paradrop_list, vec![("E2".to_string(), 9)]);
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
        let mut rules: RuleSet = RuleSet::from_rules_layers(&layers).expect("rules parse");
        let art_ini: IniFile = IniFile::from_str(art_text);
        let art = crate::rules::art_data::ArtRegistry::from_ini(&art_ini);
        rules.merge_art_data(&art);
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
    fn merge_art_propagates_infantry_crawls_without_building_side_effects() {
        let mut layers = RulesLayerStack::new(IniFile::from_str(&make_test_rules()));
        layers.push(
            crate::rules::native_processing::RulesLayerKind::Scenario,
            IniFile::from_str(
                "[E1]\nName=GI\nImage=GI\nStrength=125\nArmor=flak\nSpeed=4\n\
             [GAPOWR]\nName=Power\nStrength=750\nArmor=wood\nFoundation=2x2\n",
            ),
        );
        let mut rules = RuleSet::from_rules_layers(&layers).expect("rules parse");
        let art_ini = IniFile::from_str(
            "[GI]\nCrawls=yes\nFireUp=2\nFireProne=3\nSecondaryFire=4\nSecondaryProne=5\n\n[GAPOWR]\nCrawls=yes\nFireUp=9\n",
        );
        let art = crate::rules::art_data::ArtRegistry::from_ini(&art_ini);
        rules.merge_art_data(&art);

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
        let rules = RuleSet::from_ini(&ini).expect("naval base rules");

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
        let stored = ini.section("AI").unwrap().get("BuildConst").unwrap();
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
    fn recalc_difficulty_vectors_match_native_projection_and_atoi() {
        assert_eq!(
            parse_native_difficulty_int_vector(" \t1,,  -2junk,,+3,abc,4294967297,-2147483649  \n"),
            [1, -2, 3, 0, 1, i32::MAX]
        );
        assert_eq!(parse_native_difficulty_int_vector("7"), [7]);
        assert!(parse_native_difficulty_int_vector(" \t\r\n").is_empty());

        let oversized = format!("1,{},7", "0".repeat(509));
        assert_eq!(oversized.as_bytes()[511], b',');
        assert_eq!(
            parse_native_difficulty_int_vector(&oversized),
            [1, 0],
            "bytes beyond the char[512] payload cannot add another entry"
        );
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
        let mut rules = RuleSet::from_ini(&rules_ini).expect("rules parse");
        let art_ini_parsed = IniFile::from_str(art_ini);
        let art = crate::rules::art_data::ArtRegistry::from_ini(&art_ini_parsed);
        rules.merge_art_data(&art);
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
        let default_rules = RuleSet::from_ini(&IniFile::from_str(
            "[BuildingTypes]\n0=PLAIN\n[PLAIN]\nFoundation=2x2\n",
        ))
        .expect("default AI spacing rules");
        assert_eq!(default_rules.ai_base_spacing, 1);
        assert_eq!(
            default_rules
                .object("PLAIN")
                .unwrap()
                .base_reservation_spacing,
            Some(1)
        );

        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[AI]\nAIBaseSpacing=-3\n\
             [BuildingTypes]\n\
             0=PLAIN\n1=GATHER\n2=UNDEPLOY\n3=UNDEPLOYGATHER\n4=UNDEPLOYONE\n\
             [PLAIN]\nFoundation=2x2\n\
             [GATHER]\nFoundation=2x2\nResourceGatherer=yes\n\
             [UNDEPLOY]\nFoundation=2x2\nUndeploysInto=MCV\n\
             [UNDEPLOYGATHER]\nFoundation=2x2\nUndeploysInto=MCV\nResourceGatherer=yes\n\
             [UNDEPLOYONE]\nFoundation=1x1\nUndeploysInto=MCV\n",
        ))
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
            rules.merge_art_data(&art);
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
            rules.merge_art_data(&ArtRegistry::from_ini(&IniFile::from_str(art)));
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
            rules.merge_art_data(&ArtRegistry::from_ini(&IniFile::from_str(art)));
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
        retained.merge_art_data(&ArtRegistry::from_ini(&IniFile::from_str(
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

        first.merge_art_data(&first_art);
        reordered.merge_art_data(&reordered_art);
        changed.merge_art_data(&changed_art);

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
}
