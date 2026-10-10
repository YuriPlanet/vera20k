//! Game object type definitions parsed from rules.ini.
//!
//! Every unit, vehicle, aircraft, and building in RA2 is defined by a section in
//! rules.ini. This module provides the `ObjectType` struct which captures the
//! common properties shared by all game objects. Object-specific behavior (e.g.,
//! infantry prone stance, building power grid) is handled by category-specific
//! fields with sensible defaults.
//!
//! ## rules.ini format
//! ```ini
//! [MTNK]
//! Name=Grizzly Battle Tank
//! Cost=700
//! Strength=300
//! Armor=heavy
//! Speed=6
//! Sight=6
//! TechLevel=2
//! Owner=Americans,Alliance,British,French,Germans,Koreans
//! Prerequisite=GAWEAP
//! Primary=105mm
//! Image=MTNK
//! ```
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on sim/, render/, ui/, etc.

use glam::IVec3;

use crate::rules::ini_parser::IniSection;
use crate::rules::jumpjet_params::JumpjetParams;
use crate::rules::locomotor_type::{LocomotorKind, MovementZone, SpeedType};
use crate::rules::terrain_rules::LandType;
use crate::util::fixed_math::{SimFixed, sim_from_f32};

/// Which type registry an object belongs to.
///
/// Determines which `[XxxTypes]` section listed this object and affects
/// which game behaviors apply (e.g., only buildings have power, only
/// infantry can garrison).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum ObjectCategory {
    Infantry,
    Vehicle,
    Aircraft,
    Building,
}

/// `TechnoTypeClass+394`: constructor710CA5 and INI reader477590.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(i32)]
pub enum VhpScan {
    #[default]
    None = 0,
    Normal = 1,
    Strong = 2,
}

impl VhpScan {
    /// ReadString into `char[128]`, then the first `strtok(",")` token
    /// (`TechnoTypeClass::ReadINI`, `0x0071256D`). Unknown names keep the
    /// previous field, and each rules pass supplies its prior value.
    fn read_ini(section: &IniSection) -> Self {
        section.read_string_with("VHPScan", 0x80, Self::None, |current, value| {
            let Some(token) = crate::rules::ini_value::strtok(value, &[',']).next() else {
                // Native passes null to the CRT comparator for comma-only input;
                // keep malformed non-retail input inert instead of manufacturing a mode.
                return current;
            };
            if token.eq_ignore_ascii_case("None") {
                Self::None
            } else if token.eq_ignore_ascii_case("Normal") {
                Self::Normal
            } else if token.eq_ignore_ascii_case("Strong") {
                Self::Strong
            } else {
                current
            }
        })
    }
}

/// Sidebar/build-queue classification for buildable buildings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildCategory {
    Tech,
    Resource,
    Power,
    Infrastructure,
    Combat,
}

impl BuildCategory {
    fn from_ini(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "tech" => Some(Self::Tech),
            "resource" | "resoure" => Some(Self::Resource),
            "power" => Some(Self::Power),
            "infrastructure" => Some(Self::Infrastructure),
            "combat" => Some(Self::Combat),
            _ => None,
        }
    }
}

/// What pip display to show below a unit's health bar (PipScale= in rules.ini).
///
/// Controls the type of pip overlay rendered beneath selected units:
/// - `Tiberium`: cargo fill pips for harvesters (green=ore, colored=gem)
/// - `Passengers`: passenger count pips for transports
/// - `Ammo`: ammunition count pips
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PipScale {
    #[default]
    None,
    Tiberium,
    Passengers,
    Ammo,
}

impl PipScale {
    fn from_ini(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "tiberium" => Self::Tiberium,
            "passengers" => Self::Passengers,
            "ammo" => Self::Ammo,
            _ => Self::None,
        }
    }
}

/// What type of objects this building can produce (Factory= in rules.ini).
///
/// RA2 uses this key to determine which production queue a building serves:
/// a building with `Factory=InfantryType` acts as a barracks, one with
/// `Factory=UnitType` as a war factory, etc. This replaces hardcoded
/// building-name checks and lets modders add new factories without code changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FactoryType {
    /// Produces buildings (ConYards: GACNST, NACNST, YACNST).
    BuildingType,
    /// Produces infantry (Barracks: GAPILE, NAHAND, YABRCK).
    InfantryType,
    /// Produces vehicles (War Factories: GAWEAP, NAWEAP, YAWEAP).
    UnitType,
    /// Produces aircraft (Airfields: GAAIRC, AMRADR).
    AircraftType,
}

impl FactoryType {
    /// Parse the Factory= INI value (case-insensitive).
    ///
    /// RESIDUAL: native reads `Factory=` through `INIClass::ReadFactory @
    /// 0x00474FF0`, the case-insensitive class-name table of
    /// `AbstractType_From_Name @ 0x0040DCE0`, so the object class names
    /// (`Unit`, `Infantry`, ...) and every other class parse too; VERA keeps
    /// the four type names and reads anything else as no factory. Trigger: a
    /// rules or map INI naming another class. Effect: each reader compares
    /// that class its own way natively. Frequency: none in retail (all 14
    /// `rulesmd.ini` `Factory=` values are type names).
    pub fn from_ini(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "buildingtype" => Some(Self::BuildingType),
            "infantrytype" => Some(Self::InfantryType),
            "unittype" => Some(Self::UnitType),
            "aircrafttype" => Some(Self::AircraftType),
            _ => None,
        }
    }

    /// The factory that builds an object of `category`: its type class.
    pub const fn for_category(category: ObjectCategory) -> Self {
        match category {
            ObjectCategory::Infantry => Self::InfantryType,
            ObjectCategory::Vehicle => Self::UnitType,
            ObjectCategory::Aircraft => Self::AircraftType,
            ObjectCategory::Building => Self::BuildingType,
        }
    }
}

/// One docking pad on a building. Stored in `ObjectType.pads` as a Vec
/// whose index IS the pad index (0-based, matching `DockingOffset0..N-1`).
///
/// `lepton_offset` is parsed from art.ini `DockingOffset%d=X,Y,Z` and is
/// interpreted as an offset from the building's geometric center (not its
/// origin top-left). 256 leptons = 1 cell. Zero-initialized entries are
/// valid: when rules declares `NumberOfDocks=N` but art only specifies
/// `DockingOffset0..K-1` with K < N, the remaining pads get zero offsets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DockPad {
    pub lepton_offset: (i32, i32, i32),
}

/// Native `AddOccupy1..8` / `RemoveOccupy1..8` storage cardinality.
pub const HIDDEN_OCCUPY_SLOT_COUNT: usize = 8;

/// Native `TechnoTypeClass::Weapon[]` / `EliteWeapon[]` slot count.
///
/// gamemd-derived: `TechnoClass::SetGunnerWeapon @ 0x0070DC70` accepts
/// `0 <= idx < 18`, and `EliteWeapon[]` (`+0xA94`) sits `18 * 0x1C` (+4)
/// bytes after `Weapon[]` (`+0x898`).
pub const WEAPON_SLOT_COUNT: usize = 18;

/// Weapon-array slot the `Primary=` / `ElitePrimary=` keys name
/// (`TechnoTypeClass+0x898` / `+0xA94`).
pub const WEAPON_SLOT_PRIMARY: usize = 0;

/// Weapon-array slot the `Secondary=` / `EliteSecondary=` keys name
/// (`TechnoTypeClass+0x8B4` / `+0xAB0`).
pub const WEAPON_SLOT_SECONDARY: usize = 1;

/// Immutable building-type inputs for the separate hidden-object cell counter.
///
/// Missing numbered offsets remain `None` in their native slot instead of being
/// compacted. This profile is copied onto placed objects because cell-list exit
/// runs without borrowing the rules registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct BuildingHiddenOccupancyProfile {
    pub can_hide_things: bool,
    pub occupy_height: i32,
    pub add_occupy: [Option<(i16, i16)>; HIDDEN_OCCUPY_SLOT_COUNT],
    pub remove_occupy: [Option<(i16, i16)>; HIDDEN_OCCUPY_SLOT_COUNT],
}

impl Default for BuildingHiddenOccupancyProfile {
    fn default() -> Self {
        Self {
            can_hide_things: true,
            occupy_height: 2,
            add_occupy: [None; HIDDEN_OCCUPY_SLOT_COUNT],
            remove_occupy: [None; HIDDEN_OCCUPY_SLOT_COUNT],
        }
    }
}

/// A game object definition parsed from a rules.ini section.
///
/// Fields use sensible defaults when the INI key is absent, matching
/// the original game's behavior (RA2 uses hardcoded defaults for missing keys).
#[derive(Debug, Clone)]
pub struct ObjectType {
    /// Section name in rules.ini (e.g., "MTNK", "E1", "GAWEAP").
    /// This is the unique identifier used throughout the engine.
    pub id: String,
    /// Which type registry this object belongs to.
    pub category: ObjectCategory,
    /// Display name (CSF string table key or raw text). None if not specified.
    pub name: Option<String>,
    /// Localized display-name CSF key from `UIName=` (e.g. "Name:MTNK").
    /// None if not specified. Tooltip text resolves this through the CSF
    /// table; `name` (English `Name=`) is the fallback.
    pub ui_name: Option<String>,
    /// Credit cost to produce this object.
    pub cost: i32,
    /// `Soylent=` (TechnoType `+0x614`): when nonzero, the refund
    /// `TechnoTypeClass::GetRefund @ 0x00711F60` returns instead of the cost.
    pub soylent: i32,
    /// `FactoryPlant=` (BuildingType `+0x16CD`): Unlimbo and ChangeOwner add
    /// the building to its House's FactoryPlant list (House `+0x140`).
    pub factory_plant: bool,
    /// `InfantryCostBonus=`, `UnitsCostBonus=`, `AircraftCostBonus=`,
    /// `BuildingsCostBonus=` and `DefensesCostBonus=` (BuildingType
    /// `+0x16D0..+0x16E0`, in [`Self::factor_slot`] order): ReadDouble
    /// into floats (`0x00460686..0x00460718`), the constructor's 1.0
    /// (`0x0045E199..0x0045E1BC`) as default. `HouseClass::CalculateCostMultipliers
    /// @ 0x0050BF60` multiplies each FactoryPlant's bonuses into the House.
    pub cost_bonuses: [crate::util::native_x87::NativeF32Bits; 5],
    /// `Explosion=` — the type's OWN death animations, one chosen at random.
    ///
    /// gamemd-derived: `UnitClass::Death_Explosion @ 0x00738680` picks
    /// `Explosion[Random__Next() % len]` at the object's coordinate, after the
    /// killing warhead's own `AnimList=` anim. 487 stock sections author it.
    pub explosion_anims: Vec<String>,
    /// `DestroyAnim=` — a second list, drawn from after the explosion.
    ///
    /// gamemd-derived: same function; the vector is `TechnoTypeClass+0x748`
    /// (items `+0x74C`, count `+0x758`, key push at `0x00713A97`) and native
    /// takes `Random__Next() % count` from it, exactly as it does for
    /// `Explosion=` — one draw each, explosion first.
    pub destroy_anims: Vec<String>,
    /// `Trainable=` (TechnoType `+0xC8E`) — whether this object can gain
    /// veterancy from its kills.
    ///
    /// gamemd-derived: `TechnoClass::Record_The_Kill @ 0x00702D40` pays an
    /// untrainable killer nothing (a garrisoned building's kill goes to its
    /// firing occupant instead). `TechnoTypeClass::ReadINI` reads the key with
    /// the constructor's value as its default (`0x00714A15..0x00714A29`): true
    /// from `TechnoTypeClass`'s constructor (`0x0071138E`), false from
    /// `BuildingTypeClass`'s (`0x0045E42E`, EBX zeroed at `0x0045DD9A`). Retail
    /// sets `Trainable=yes` on one building (`[YAREFN]`) and `no` on 82 other
    /// sections.
    pub trainable: bool,
    /// A Unit's `BurstDelay0..3=` (UnitType `+0xE48 + i*4`; constructor -1 at
    /// `0x0074726D..0x0074727F`, ReadInteger with the current value as default
    /// at `0x00747B03..0x00747B47`). GetROF returns `BurstDelay{i-1}` without a
    /// draw for burst index `i` in 1..=4 when it is not -1. `[-1; 4]` for other
    /// classes; no retail section authors them.
    pub burst_delays: [i32; 4],
    /// An InfantryType's `DeadBodies=` (`+0xE50`, TypeList read at
    /// `0x005241FB`): the corpse anims its Die1..Die5 completion picks from
    /// (`0x00520BC6`). Empty for other classes; no retail infantry sets it.
    pub dead_bodies: Vec<String>,
    /// An InfantryType's `NotHuman=` (`+0xEAD`, read at `0x005243D2`,
    /// constructor 0 at `0x0052375A`): a death completion with no own
    /// `DeadBodies=` leaves no `[General] DeadBodies=` corpse (`0x00520C42`).
    pub not_human: bool,
    /// Hit points (health). 0 = invincible or not applicable.
    pub strength: i32,
    /// `DontScore=` — this object's destruction is invisible to the end-of-match
    /// score. gamemd's kill-record step returns on this byte before ANY
    /// bookkeeping, so the victim contributes no kill, no loss and no points.
    /// Stock sets it on exactly four types: `SLAV`, `V3ROCKET`, `DMISL`, `CMISL`
    /// — the enslaved miners and the three spawner missiles, all of which die
    /// constantly in ordinary play.
    pub dont_score: bool,
    /// `SpecialThreatValue=` candidate multiplier used by threat scoring.
    pub special_threat_value: f64,
    /// `ThreatPosed=` (`TechnoTypeClass+0x670`, read by
    /// `TechnoTypeClass::ReadINI @ 0x007149CE` through `ReadInteger @
    /// 0x005276D0`). Constructor default is 0 — `TechnoTypeClass::Constructor
    /// @ 0x007110CE` stores EBX, and `XOR EBX,EBX @ 0x00710B00` is the only
    /// write to that register in the whole body.
    ///
    /// This is NOT a term of `TechnoClass::Calculate_Threat_Score @ 0x0070CD10`
    /// (that reads `SpecialThreatValue` at `TechnoTypeClass+0x2C0`). Inside
    /// acquisition it has exactly one consumer: the human-attacker building
    /// gate in `TechnoClass::Evaluate_Candidate @ 0x006F85E6`, which refuses an
    /// enemy BUILDING that has no weapon or poses no threat. A `ThreatPosed=0`
    /// infantryman or vehicle is acquired like any other.
    pub threat_posed: i32,
    /// `LeadershipRating=` (`TechnoTypeClass+0x5FC`, ReadInteger at
    /// `0x0071433E`, constructor 5 at `0x0071101A`): a team's leader is its
    /// qualifying member of the highest rating (`TeamClass::Fetch_A_Leader @
    /// 0x006EC3D0`).
    pub leadership_rating: i32,
    /// `MyEffectivenessCoefficient=` (`TechnoTypeClass+0x2C8`, read at
    /// `0x0071556B`). `None` means the key is absent, in which case native
    /// passes `[General] MyEffectivenessCoefficientDefault` (`RulesClass+0x1040`)
    /// as the read default — so the resolution happens where the value is used.
    pub my_effectiveness_coefficient: Option<f64>,
    /// `TargetEffectivenessCoefficient=` (`TechnoTypeClass+0x2D0`, `0x007155D8`).
    pub target_effectiveness_coefficient: Option<f64>,
    /// `TargetSpecialThreatCoefficient=` (`TechnoTypeClass+0x2D8`, `0x0071563F`).
    pub target_special_threat_coefficient: Option<f64>,
    /// `TargetStrengthCoefficient=` (`TechnoTypeClass+0x2E0`, `0x007156A6`).
    pub target_strength_coefficient: Option<f64>,
    /// `TargetDistanceCoefficient=` (`TechnoTypeClass+0x2E8`, `0x0071570C`).
    pub target_distance_coefficient: Option<f64>,
    /// TechnoType+2F0: exact-case `ThreatAvoidanceCoefficient`, ReadDouble
    /// 712452..712473 (5283D0), constructor positive-zero. FootUnlimbo copies
    /// this to its own retained+530 after placement succeeds (4D72EA..4D72F4).
    /// No clamp or post-read conversion; retain the native double bits.
    pub threat_avoidance_coefficient: crate::util::native_x87::NativeF64Bits,
    /// Armor type name (e.g., "heavy", "light", "wood"). Determines damage
    /// multipliers from warhead Verses= values.
    pub armor: String,
    /// Canonical Speed percent,0..100 (0 is immobile). TechnoType
    /// ReadINI71464A retains the prior value for -1 across rules passes.
    /// Whole-lepton Type+678 conversion belongs to util::fixed_math.
    pub speed: i32,
    /// `WalkRate=` — signed native-frame divisor for Foot body animation.
    /// TechnoTypeClass owns this value; art.ini owns only the frame layout.
    pub walk_rate: i32,
    /// `IdleRate=` — signed native-frame divisor for idle Foot body animation.
    /// Zero disables idle counter advancement.
    pub idle_rate: i32,
    /// Inertia weight (`Weight=` in vehicle/aircraft sections). Default 2.0.
    /// Used as the divisor in rocker-impulse force scaling: heavier units rock
    /// proportionally less per equivalent impulse. Retail range: 0.5 (lightest
    /// vehicles) to 5 (Aircraft Carrier).
    pub weight: SimFixed,
    /// Fraction of max speed gained per tick during acceleration (AccelerationFactor=).
    /// Default 0.03: `TechnoTypeClass` ctor `0x00710BD0`/`0x00710BDA` writes the
    /// double `0x3F9EB851EB851EB8` to `+0x308`, and `ReadINI 0x007124C4` passes
    /// that current value as the key's default. About 33 ticks from rest to full
    /// speed; the wall-clock time follows `GameSpeed=` (criterion A12), so no
    /// frame rate is quoted here.
    pub accel_factor: SimFixed,
    /// Fraction of max speed lost per tick during braking (DeaccelerationFactor=).
    /// Default 0.002 — not 0.02, which this comment claimed until the constant was
    /// read: ctor `0x00710BBC`/`0x00710BC6` writes `0x3F60624DD2F1A9FC` to `+0x300`
    /// (`ReadINI 0x007124A3`, key string `0x008443F4`). The parse below was already
    /// correct. Applied when within slowdown_distance of destination.
    pub decel_factor: SimFixed,
    /// Whether Drive/Ship locomotors ramp toward target speed (`Accelerates=`).
    /// Defaults to true; `Accelerates=false` is handled by locomotor speed
    /// fraction ownership, not by mutating raw `Speed=`.
    pub accelerates: bool,
    /// UnitType+E0C: ctor747103=false, ReadINI747829/74783D reads Passive
    /// through INIClass::ReadBool 0x5295F0; other classes keep false.
    /// Ordinary Drive4B1C59..1C72/Ship6A12A3..12BC chain admission requires it.
    pub passive: bool,
    /// TechnoType+C94 `IsTrain=`: TechnoTypeClass::ReadINI 0x0071227F reads it
    /// through INIClass::ReadBool 0x5295F0 over the constructor's false. No
    /// retail type sets it; Drive/Ship Process_Movement coerces codes below 7
    /// to 0 for a train (0x4B34DE / 0x6A2B2D) and passes it as Find_Path's
    /// append flag (0x4B3EF2).
    pub is_train: bool,
    /// Lepton distance from destination at which braking begins (SlowdownDistance=).
    /// Native default500 leptons.
    pub slowdown_distance: i32,
    /// TechnoType+618: constructor711050 seeds -1; ReadINI712336 reads
    /// `FlightLevel`. Getter717800 uses General.FlightLevel only for -1.
    pub(crate) flight_level: i32,
    /// TechnoType+C95: ctor7113B9=false, ReadINI712350..712373 reads
    /// IsDropship; Fly4CDA28 selects its distinct vertical motion.
    pub(crate) is_dropship: bool,
    /// TechnoType+3B0, ReadINI712379..712391: degrees converted to radians;
    /// -1 preserves the constructor value (20 degrees). Used by Fly approach.
    pub(crate) pitch_angle: SimFixed,
    /// TechnoType+52C/+530, ReadINI712E4D/712E89: takeoff/landing cues.
    pub(crate) aux_sound1: Option<String>,
    pub(crate) aux_sound2: Option<String>,
    /// Vision range in cells.
    pub sight: i32,
    /// Technology level required (-1 = unbuildable by player).
    pub tech_level: i32,
    /// `BuildTimeMultiplier=` (TechnoType `+0x608`): read by `0x00714377` with
    /// ReadDouble into the float field; the TechnoType constructor's 1.0
    /// (`0x00711030`) is the default, with no clamp.
    pub build_time_multiplier: crate::util::native_x87::NativeF32Bits,
    /// Which houses/sides can build this (e.g., ["Americans", "Alliance"]).
    pub owner: Vec<String>,
    /// `DoubleOwned=` (`TechnoType+0xC99`, ReadINI `0x0071228A..0x0071229E`,
    /// constructor 0): outside a campaign `Get_Ownable @ 0x00711EC0` answers
    /// every house instead of `Owner=`. Retail sets it on no type.
    pub double_owned: bool,
    /// Specific countries that may build this object.
    pub required_houses: Vec<String>,
    /// Countries explicitly forbidden from building this (ForbiddenHouses= in rules.ini).
    /// Inverse of Owner — if the player's country is in this list, they cannot build.
    pub forbidden_houses: Vec<String>,
    /// Signed `TechnoTypeClass+0x6D0` side filter used only by native AI base
    /// planning selectors ([`Self::planned_for_side`]). The constructor seed
    /// is `-1` (all sides).
    pub ai_base_planning_side: i32,
    /// Native `BuildingTypeClass+0x1705` AI plan-generation eligibility bit.
    /// The BuildingType constructor clears it and `AIBuildThis=` may set it.
    pub ai_build_this: bool,
    /// Whether this type may appear in multiplayer starting-unit generation.
    pub allowed_to_start_in_multiplayer: bool,
    /// `Prerequisite=` (`TechnoTypeClass+0x638`, items `+0x63C`, count
    /// `+0x648`), parsed in each rules pass that reads it
    /// (`crate::rules::prerequisite`).
    pub prerequisite: Vec<crate::rules::prerequisite::Prerequisite>,
    /// `PrerequisiteOverride=` (`TechnoTypeClass+0x654`): a house with one of
    /// these on the map skips CanBuild's tech and prerequisite gates. Parsed
    /// like [`Self::prerequisite`].
    pub prerequisite_override: Vec<crate::rules::prerequisite::Prerequisite>,
    /// `BuildLimit=` (`TechnoTypeClass+0x3B8`, ReadINI `0x00713157`): the
    /// constructor's `0x7FFFFFFF` (`0x00710CF0`) when absent. `CanBuild`
    /// caps a positive limit by the house's tracked count of the type and a
    /// non-positive one by the count it has produced.
    pub build_limit: i32,
    /// Requires spy infiltration of an Allied Battle Lab to unlock.
    pub requires_stolen_allied_tech: bool,
    /// Requires spy infiltration of a Soviet Battle Lab to unlock.
    pub requires_stolen_soviet_tech: bool,
    /// Requires spy infiltration of a Yuri Battle Lab to unlock.
    pub requires_stolen_third_tech: bool,
    /// Art.ini image reference. Defaults to the object's ID if not specified.
    /// Used to look up sprite/voxel filenames in art.ini.
    pub image: String,
    /// Power generation (positive) or consumption (negative). Buildings only.
    pub power: i32,
    /// Extra power bonus per occupant for `InfantryAbsorb`/`UnitAbsorb`
    /// buildings. Parsed from `ExtraPower=` (signed i32). Only contributes
    /// when the building has `InfantryAbsorb=yes` or `UnitAbsorb=yes` and
    /// at least one passenger is garrisoned. Stock YR: YAPOWR Bio-Reactor
    /// uses `ExtraPower=100` × up to 5 garrisoned infantry.
    pub extra_power: i32,
    /// Building foundation footprint (e.g., "3x2", "1x1"). Buildings only.
    pub foundation: String,
    /// Pixel offset for health bar / selection bracket Y position.
    /// Negative values shift the bar UP (above taller sprites). Default 0.
    /// Parsed from `PixelSelectionBracketDelta` in rules.ini.
    pub pixel_selection_bracket_delta: i32,
    /// Sidebar/build tab grouping for structures.
    pub build_cat: Option<BuildCategory>,
    /// Human placement radius away from existing base-normal structures.
    pub adjacent: i32,
    /// `ProtectWithWall=` (`BuildingTypeClass+0x1765`, ReadINI
    /// `0x0046026B..0x0046027F`) adds one cell to the active AI site's
    /// first-phase CheckOccupancy border, and marks a building the computer
    /// may wall in (`sim::ai_base_building`). It is distinct from `Wall=`
    /// segment identity.
    pub protect_with_wall: bool,
    /// `WantsExtraSpace=` adds the same one-cell first-phase AI site border.
    pub wants_extra_space: bool,
    /// Whether this structure expands the owner's build area.
    pub base_normal: bool,
    /// Whether this structure can expand allied build area when BuildOffAlly is enabled.
    pub eligibile_for_ally_building: bool,
    /// Whether selling/destruction can eject infantry crew from this structure.
    pub crewed: bool,
    /// `VoiceSelect=` at TechnoType+414. Constructor710D74..710D9A creates
    /// an empty vector; ReadINI712B1D..712B87 calls ReadSoundList525430,
    /// retaining the prior vector on an empty/missing read. Production binds
    /// every resolved token, in order, against the fixed SOUNDMD catalog.
    pub voice_select: Vec<String>,
    /// Sound ID played when this unit is ordered to move.
    pub voice_move: Option<String>,
    /// Sound ID played when this unit is ordered to attack.
    pub voice_attack: Option<String>,
    /// Sound ID played when a harvester is ordered to harvest a resource cell.
    pub voice_harvest: Option<String>,
    /// Sound ID played when a harvester is manually ordered to return to a
    /// friendly refinery (right-click own refinery).
    pub voice_enter: Option<String>,
    /// Sound ID played when this unit is ordered to capture a building.
    ///
    /// Retail gives capture its own order-ack slot and falls back to the Enter
    /// slot only when this key is absent. Stock YR ships it on every engineer
    /// (`EngAllAttackCommand` / `EngSovAttackCommand`), so the fallback is not
    /// the ordinary-play path.
    pub voice_capture: Option<String>,
    /// `PreventAttackMove=` — the type refuses attack-move orders even when it
    /// carries a real `Primary=`.
    ///
    /// Retail's attack-move eligibility predicate is
    /// `Primary != null && !PreventAttackMove`; stock YR sets the key on
    /// ENGINEER / SENGINEER / YENGINEER / SPY, all of which carry a real
    /// `Primary=` and would otherwise pass. Default false.
    pub prevent_attack_move: bool,
    /// Ordered voice-sound choices for a human-controlled entity's fatal result.
    pub voice_die: Vec<String>,
    /// Ordered sound choices played when this entity dies or is destroyed.
    pub die_sounds: Vec<String>,
    /// `DamageSound=` — `TechnoTypeClass+0x538`, the type's own struck cue.
    ///
    /// Only its presence is consumed here: `BuildingClass::ReceiveDamage`
    /// gates the global `[AudioVisual] BuildingDamageSound=` on
    /// `0x004426D2 CMP [type+0x538],-1`, so a type that names any resolvable
    /// sound suppresses the global.
    ///
    /// RESIDUAL: the per-type cue itself is not routed. Native plays it from
    /// `TechnoClass::ReceiveDamage @ 0x00701900` arm `0x00702713`/`0x00702717`
    /// — index **1** of the switch table at `0x00702D24`, i.e. the ordinary
    /// **non-crossing** hit (damage result 1), not the result-2/3 crossings
    /// the global rides — and plays it **twice** (`0x00702760`, `0x007027A9`).
    /// The two cues are therefore mutually exclusive by result, not just by
    /// the `-1` gate. Trigger / player effect / frequency are recorded in full
    /// on `crate::audio::events::SoundEventQueue`.
    pub damage_sound: Option<String>,
    /// Ordered MoveSound vector, TechnoType+4F4. Constructor710E7B..710EAC
    /// creates an empty list; ReadINI713459..7134D9 uses ReadSoundList525430.
    /// The fixed SOUNDMD binder resolves each token, retaining duplicates.
    pub move_sound: Vec<String>,
    /// `CrashingSound=` — `TechnoTypeClass+0x544`, read at `0x00712F80`.
    /// `FootClass::AI` plays it on the object's MoveSound controller
    /// (`+0x544`) when the crash latch rises (`0x004DAD5E..0x004DADA7`).
    pub crashing_sound: Option<String>,
    /// `VoiceCrashing=` — `+0x550`, read at `0x00713034`. Played at the same
    /// latch edge, before `CrashingSound=`, for a human player's object only
    /// (`HouseClass::IsHumanPlayer @ 0x0050B6F0`, `0x004DAD10..0x004DAD59`).
    pub voice_crashing: Option<String>,
    /// TechnoType+548/+554, read at712FF1/7130A5. Foot AI4DABC7 observes
    /// the sinking edge, plays VoiceSinking, then the held SinkingSound.
    pub sinking_sound: Option<String>,
    pub voice_sinking: Option<String>,
    /// `ImpactWaterSound=` / `ImpactLandSound=` — `+0x53C` / `+0x540`, read at
    /// `0x00712EFC` / `0x00712F38`: the crash impact's cue by the impact cell's
    /// LandType (`FlyLocomotionClass::Process 0x004CD818..0x004CD891`), each
    /// falling back to the `[AudioVisual]` key of the same name.
    ///
    /// Like every `ReadString`+`VocClass::FindIndex` sound key, native keeps
    /// the previous layer's value when a later layer names no registered
    /// sound; VERA keeps the later name and the app resolves it to silence.
    /// No retail INI names an unregistered crash sound.
    pub impact_water_sound: Option<String>,
    pub impact_land_sound: Option<String>,
    /// `VoiceFeedback=` — `TechnoTypeClass+0x4D8`, the line spoken when this
    /// techno's HP crosses below half strength.
    ///
    /// Identity: `0x00712D9C LEA EDI,[EBP+0x4D8]` in
    /// `TechnoTypeClass::ReadINI` pushes the key string at `0x0084424C`
    /// (`"VoiceFeedback"`) into `CCINIClass::ReadSoundList @ 0x00525430`
    /// (`0x00712DCB`), so the vector is at `+0x4D8` with items `+0x4DC` and
    /// count `+0x4E8` — the exact fields
    /// `TechnoClass::ReceiveDamage @ 0x00702695` reads. The fixed SOUNDMD
    /// binder retains resolved order and duplicates. The constructor starts
    /// empty; accepted damage consumes a percent draw followed by one raw
    /// Main draw even for a singleton, then emits a positional Voc request.
    pub voice_feedback: Vec<String>,
    /// Ordered default command-voice list at TechnoType+4A0. Constructor
    /// 710E13..710E49 creates an empty vector; ReadINI712CC5..712D2A uses
    /// ReadSoundList525430. QueueMegaMission6FFD42 draws once for a nonempty
    /// list, then resolves the unsigned remainder through the fixed SOUNDMD
    /// catalog's retained ordered names.
    pub voice_special_attack: Vec<String>,
    /// Sound ID played when this entity is crushed by a vehicle (squish).
    pub crush_sound: Option<String>,
    /// Sound ID played when this unit deploys (e.g. GI sandbag-up).
    pub deploy_sound: Option<String>,
    /// BuildingType+E6C ctor45DE64=-1. ReadINI460738..46078B exact
    /// BuildupSound/ReadString128 retains the prior index when absent, empty
    /// or unknown; RuleSet::bind_type_sound_references resolves SOUNDMD.
    pub buildup_sound: Option<String>,
    /// Sound ID played when this unit undeploys.
    pub undeploy_sound: Option<String>,
    /// InfantryType+EA4/+EA8: ctor523748/52374E sets both to -1.
    /// ReadINI52440B/524447 reads EnterWaterSound/LeaveWaterSound through
    /// ReadString128 and Voc7514D0. The fixed sound catalog owns resolution.
    pub enter_water_sound: Option<String>,
    pub leave_water_sound: Option<String>,
    /// `PackupSound=` (BuildingType `+0xE70`, read at `0x00460786` through
    /// `VocClass::FindByName`): played at a human player's building as its
    /// sale starts packing up (`BuildingClass::Mission_Selling` stage 1, `0x0044A85B`).
    /// No retail type sets it.
    pub packup_sound: Option<String>,
    /// `LeaveTransportSound=` — `TechnoTypeClass+0x568`. Read in
    /// `TechnoTypeClass::ReadINI` right after `EnterTransportSound=`
    /// (`+0x564`, key push at `0x00713432`); played by
    /// `UnitClass::Mission_Unload @ 0x0073D630` at the transport's own
    /// coordinate (`0x0073DC28`..`0x0073DC67`) once an ejected passenger has
    /// been placed.
    pub leave_transport_sound: Option<String>,
    /// Sound played at the destination cell when this unit warps in
    /// (chrono teleport arrival).
    pub chrono_in_sound: Option<String>,
    /// Sound played at the source cell when this unit warps out
    /// (chrono teleport departure).
    pub chrono_out_sound: Option<String>,
    /// Whether this unit has an independently rotating turret.
    /// Parsed from rules.ini `Turret=yes`. Only meaningful for vehicles/aircraft.
    pub has_turret: bool,
    /// Turret rotation speed in RA2 "ROT" units (degrees per game frame at 15fps).
    /// Higher = faster turret rotation. Only meaningful when `has_turret` is true.
    /// Typical values: 5 (War Miner), 7 (Grizzly/Rhino).
    pub turret_rot: i32,
    /// Retained TechnoType recoil reader; shared by every voxel turret.
    pub recoil: crate::rules::recoil::RecoilConfig,
    /// VXL turret model name for buildings (TurretAnim= in rules.ini, e.g., "SAM").
    /// The engine loads `{TurretAnim}.VXL` + `{TurretAnim}.HVA` as the turret model.
    pub turret_anim: Option<String>,
    /// Whether the turret anim is a VXL model (TurretAnimIsVoxel=, default false).
    /// When false, the TurretAnim is an SHP overlay handled by the building anim system.
    pub turret_anim_is_voxel: bool,
    /// Pixel X offset for building turret placement (TurretAnimX=).
    pub turret_anim_x: i32,
    /// Pixel Y offset for building turret placement (TurretAnimY=).
    pub turret_anim_y: i32,
    /// Depth adjustment for building turret (TurretAnimZAdjust=, negative = behind).
    pub turret_anim_z_adjust: i32,
    /// Scan radius in cells for auto-targeting idle enemies. If None, defaults
    /// to the primary weapon's range at runtime.
    pub guard_range: Option<SimFixed>,
    /// Range bonus (in cells) added to the weapon's max range when firing at
    /// high-flying targets. Read from `AirRangeBonus=` in the unit section.
    /// None means no bonus.
    pub air_range_bonus: Option<SimFixed>,
    /// `OpportunityFire=` — when true, the unit runs the passive target
    /// scanner while on an ordinary Move / Harvest / Guard mission and can
    /// acquire a target without an explicit attack order. Default no.
    pub opportunity_fire: bool,
    /// `CanRetaliate=` — when false, the unit does not fire back when hit
    /// (suppresses the damage-triggered retaliation acquisition). Default yes.
    pub can_retaliate: bool,
    /// TechnoType+D33, read by Foot Approach_Target4D5690. Constructor
    /// 71152A sets true; ReadINI7144AF uses native ReadBool with the current
    /// value as its default. Attack still searches when this flag is false.
    /// Original constructor/reader controls and retail layer inputs:
    /// `tools/spatial_oracle/fv_cell_attack/approach_rules.json`.
    pub can_approach_target: bool,
    /// TechnoType+D34 (constructor711530, ReadINI7144C9): permit Approach
    /// to discard a retained destination too far from the current target.
    pub can_recalc_approach_target: bool,
    /// `CanPassiveAquire=` (the key is misspelled in the original INI and in
    /// the binary's key table — parsed verbatim). When false the object never
    /// reaches the passive target scanner, so it only ever fires at a target it
    /// was explicitly given. Default **yes**; stock `rulesmd.ini` opts 17 types
    /// out ("Won't try to pick up own targets").
    pub can_passive_acquire: bool,
    /// `SprayAttack=` (TechnoType `+0x691`, read at `0x0071491A`, constructor
    /// 0 at `0x00711104`): the passive-acquire gate's AreaFire test reads
    /// weapon slot 0 instead of 1 (`0x0070DD70`).
    pub spray_attack: bool,
    /// `DistributedFire=` — the type spreads fire across several nearby targets
    /// instead of committing to one. VERA parses it only to keep those types
    /// OFF the single-target passive-acquire commit; the spread-fire mechanism
    /// itself is not implemented. Default no.
    pub distributed_fire: bool,
    /// `VHPScan=` estimated-health filtering/scoring; native default None.
    pub vhp_scan: VhpScan,
    /// Whether this unit fires a warhead at its own position on death (e.g.,
    /// Apocalypse Tank explosion damages nearby units).
    pub explodes: bool,
    /// The full native `VeteranAbilities=` byte array (`TechnoTypeClass+0x29C`,
    /// 18 bytes). Readers test a rank through
    /// `sim::combat::veterancy::has_weapon_ability` (`0x0070D0D0`'s shape, an
    /// elite inheriting the veteran list), among them `CloakingTick @
    /// 0x006FB740`, `CanAutoCloak @ 0x006FBDC0` and `ShouldUncloak @
    /// 0x006FBC90` (CLOAK), the radar registration test `TechnoClass+0x324 @
    /// 0x0070D1D0` (RADAR_INVISIBLE), the death arm (EXPLODES), Crusher
    /// (CRUSHER beside `Crusher=`), FEARLESS and SCATTER.
    pub veteran_abilities: AbilityFlags,
    /// The full native `EliteAbilities=` byte array (`TechnoTypeClass+0x2AE`).
    pub elite_abilities: AbilityFlags,
    /// `SelfHealing=` (`TechnoTypeClass+0xD14`, `ReadBool` at `0x00714AE8`).
    /// A type with this set heals at every rank; otherwise the `SELF_HEAL`
    /// ability gates the same pulse (`FUN_0070BE80`).
    pub self_healing: bool,
    /// Specific weapon fired on death (overrides default explosion behavior).
    /// References a [WeaponName] section in rules.ini.
    pub death_weapon: Option<String>,
    /// `DeathWeaponDamageModifier=` applied to explicit/current death weapons.
    /// The native TechnoType constructor seeds this to 1.0.
    pub death_weapon_damage_modifier: f32,
    /// Superweapon type ID granted when this building is completed (SuperWeapon= in rules.ini).
    /// References a section listed in [SuperWeaponTypes].
    pub super_weapon: Option<String>,
    /// Secondary superweapon type ID, typically from an upgrade (SuperWeapon2= in rules.ini).
    pub super_weapon2: Option<String>,
    /// `NukeSilo=` (BuildingType `+0x16BA`, ReadBool at `0x00460A45`,
    /// constructor clear at `0x0045E127`): `SuperClass::Launch @ 0x006CC390`
    /// fires a `MultiMissile` weapon from the first such type granting it,
    /// and `BuildingClass::Mission_Missile @ 0x0044C980` takes its silo arm.
    pub nuke_silo: bool,
    /// `ChargedAnimTime=` (BuildingType `+0x16E8`, a float: ReadDouble at
    /// `0x00460B9E`, `FSTP dword`; constructor 999.0 at `0x0045E1D4`), in
    /// minutes. A granted weapon's building swaps its SuperAnim slots once
    /// that little charge remains (`BuildingClass::UpdateAnimation`
    /// `0x00450F9E`); above 990 the block is off.
    pub charged_anim_time: f32,
    /// When true, this building provides full map vision while powered.
    /// Used by the Allied Spy Satellite Uplink (GASPYSAT).
    pub spy_sat: bool,
    /// When true, this building/unit emits a gap field that hides enemy vision
    /// within GapRadius cells (parsed from [General]).
    pub gap_generator: bool,
    /// When true, this building activates the owner's radar display (minimap).
    /// Radar=yes in rules.ini. Used by GARADR (Allied), NARADR (Soviet), YARADR (Yuri).
    /// SpySat=yes is a separate full-map reveal authority and does not imply Radar=yes.
    pub radar: bool,
    /// When true, this unit does NOT appear on enemy radar even when in line of sight.
    /// RadarInvisible= in rules.ini. Used by subs, Night Hawk, dolphins, giant squid.
    pub radar_invisible: bool,
    /// `RadarVisible=`. In active `RenderCellPixel` this restores an otherwise
    /// skipped Insignificant/passive-owner entry. It does not override shroud
    /// or an earlier hostile `RadarInvisible` rejection.
    pub radar_visible: bool,
    /// `Insignificant=` (`ObjectTypeClass+0x232`). In the active live-radar
    /// pixel gate this is distinct from `RadarVisible`: insignificant objects
    /// owned by a passive/missing house are skipped unless RadarVisible is set.
    pub insignificant: bool,
    /// `ToProtect=` (`TechnoTypeClass+0xC96`). The active generic Techno
    /// damage receiver invokes the House base-defence responder only when this
    /// type byte is set. This is independent of dormant instance ShouldProtect.
    pub to_protect: bool,
    /// Whether this unit is a resource harvester (Harvester=yes in rules.ini).
    /// Data-driven replacement for hardcoded type ID string checks.
    pub harvester: bool,
    /// `CrateGoodie=` (`UnitTypeClass+0xE0D`): a vehicle the Unit crate may
    /// draw at random. `UnitTypeClass::ReadFromINI` reads it with `ReadBool`
    /// defaulting to the live byte (`0x0074764C..0x00747665`); the constructor
    /// clears it (`0x007470D0`). `CellClass::PickupCrate @ 0x00481A00` rejects
    /// every drawn type without it (`0x004821BD`).
    pub crate_goodie: bool,
    /// `Spawned=` (`TechnoTypeClass+0xD54`): this type is a carrier/launcher
    /// child (Hornet, V3/Dreadnought missile). `TechnoTypeClass::ReadINI`
    /// reads key `"Spawned"` (`0x008437D8`) at `0x00714E7D..0x00714E91`.
    /// `TechnoClass::Death_Announcement @ 0x004D98C0` (`0x004D98DD`) stays
    /// silent for such types.
    pub spawned: bool,
    /// Whether this structure accepts ore/gem delivery (Refinery=yes in rules.ini).
    pub refinery: bool,
    /// Native BuildingType `Weeder=` classification. ExitObject dispatch tests
    /// this independently from `Refinery=`, `WeaponsFactory=`, and `Naval=`.
    pub weeder: bool,
    /// `DockUnload=` (BuildingType+0x16B3; `BuildingTypeClass::ReadINI`
    /// ReadBool at `0x004609DD`, constructor default 0). The refinery dock
    /// keys on this flag, not on `Refinery=` (+0x16BB): the harvester
    /// CAN_LOAD answer (`0x0043C66B`), the DOCKING pad arm (`0x0043CA1B`)
    /// and the unload request (`0x0043C788`). Stock NAREFN/GAREFN set both.
    pub dock_unload: bool,
    /// Whether this building has a bib (`Bib=yes` in rules.ini). When true, the
    /// east-edge column of the foundation footprint is unit-passable — units
    /// can drive across that strip even though the cells remain part of the
    /// building's placement / ownership footprint. Matches the original
    /// engine's HasBib relaxation in the per-cell occupant chain check.
    pub bib: bool,
    /// Whether this building is a native opening gate (`Gate=yes`).
    ///
    /// Consumed by the runtime cell-entry classifier together with live building
    /// gate state. `GateStages=` is visual timing data and is not part of the
    /// `CanGarrison` passability predicate.
    pub gate: bool,
    ///Raw signed-low-dword result of native `DeployTime=` x900/ftol.
    ///The u32 retains its bits; DoorClass reads it as i32. Shared by factory,
    ///Gate and Unit doors, including zero and negative transitions.
    pub deploy_time_ticks: u32,
    /// Native open-hold delay from `GateCloseDelay=` converted as
    /// `trunc(value * 900)`.
    pub gate_close_delay_ticks: u32,
    /// Bonus credits storage for refineries (Storage= in rules.ini).
    /// Refineries typically have Storage=300 — added to owner credits on placement.
    pub storage: i32,
    /// Free unit spawned when this structure is placed (FreeUnit= in rules.ini).
    pub free_unit: Option<String>,
    /// Structures this unit may dock with (Dock= in rules.ini), normalized uppercase.
    pub dock: Vec<String>,
    /// `QueueingCell=` from the art section (see `ArtEntry::queueing_cell`),
    /// merged during init; `(0, 0)` without one.
    pub queueing_cell: [i32; 2],
    /// All docking pads on this building, parsed from art.ini `DockingOffset0..N-1`.
    /// Index in vec IS the pad index.
    ///
    /// After the art→rules merge:
    /// - If art declared at least one `DockingOffset%d`, the vec is sized to
    ///   `number_of_docks` (zero-padding any indices missing in art, truncating excess).
    /// - If art declared none, the vec is left empty — consumers fall back to
    ///   their own anchor (e.g. `refinery_pad_cell`'s rightmost-column default).
    ///
    /// Service depots / single-pad helipads / airfields with declared offsets
    /// use this vec directly. Retail refineries (GAREFN/NAREFN/YAREFN) leave
    /// it empty.
    pub pads: Vec<DockPad>,
    /// Art-driven inputs for the separate hidden-object counter. These never
    /// alter normal foundation, placement, selection, or movement footprints.
    pub hidden_occupancy: BuildingHiddenOccupancyProfile,
    /// Immutable `[AI] AIBaseSpacing` snapshot for Building types that execute
    /// the native base-reservation writer. `None` is the exact writer gate.
    pub base_reservation_spacing: Option<i32>,
    /// Alternative voxel model while unloading at a refinery or fully deployed.
    /// HARV uses HORV, CMIN uses CMON, and SCHP uses SCHD (UnloadingClass=).
    pub unloading_class: Option<String>,
    /// TechnoType+6BC, ctor711175=null; rules714729 ReadString128 after
    /// UnloadingClass. Empty/missing retains the prior pointer. AnimType
    /// FindOrAllocate428B80 owns registration, before the InitialAmmo read.
    pub deploying_anim: Option<String>,
    /// Ammo count for aircraft. -1 = unlimited (default), 0+ = finite.
    /// Aircraft with finite ammo return to a helipad/airfield to reload after depleting.
    pub ammo: i32,
    /// TechnoType+680, ReadINI71474C (InitialAmmo); -1 selects Ammo at
    /// Aircraft InitFromType414033..41404B. Other signed values are retained.
    pub initial_ammo: i32,
    /// `TechnoType+0xD24`, exact `ManualReload=`: constructor7114D6 clears
    /// it; ReadBool713352 retains the current value (store713366). Depot
    /// MissionRepairAndProduce reads it before the first RepairTick and may
    /// enter its progress state even when the contact was already at Strength.
    pub manual_reload: bool,

    // -- Spawn manager (Spawns= pool: V3, Dreadnought, Boomer, Carrier, Destroyer) --
    /// TechnoType this unit spawns as sub-units (`Spawns=`). Presence of a
    /// resolvable value is the sole gate on creating a spawn manager, matching
    /// `TechnoClass::Init_Managers`.
    pub spawns: Option<String>,
    /// Spawn pool capacity (`SpawnsNumber=`). Default 0.
    pub spawns_number: i32,
    /// Frames before a destroyed spawn child is rebuilt (`SpawnRegenRate=`).
    pub spawn_regen_rate: u32,
    /// Frames a docked child spends reloading after landing (`SpawnReloadRate=`).
    pub spawn_reload_rate: u32,
    /// `MissileSpawn=yes`. On a child this marks the fire-and-forget flavour; on
    /// a *parent* it shortens the per-launch delay from 20 to 9 frames. No stock
    /// YR parent sets it, so the 9-frame branch is unreachable in stock play.
    pub missile_spawn: bool,
    /// `NoSpawnAlt=yes` — swap to the `<TYPE>WO` art while the pool is empty.
    pub no_spawn_alt: bool,

    // -- Slave Miner / economy fields --
    /// Infantry type enslaved/spawned by this unit (Enslaves= in rules.ini, YR only).
    /// Used by Slave Miner (SMIN) to spawn SLAV workers.
    pub enslaves: Option<String>,
    /// `TechnoType+0xD44`, `SlavesNumber=`: the slaves Init_Managers builds.
    /// `TechnoTypeClass::ReadINI` ReadInt at `0x00714E29`; constructor 0.
    pub slaves_number: i32,
    /// `TechnoType+0xD48`, `SlaveRegenRate=`: frames before a lost slave is
    /// regrown. ReadInt at `0x00714E08`; constructor 0.
    pub slave_regen_rate: i32,
    /// `TechnoType+0xD4C`, `SlaveReloadRate=`: frames a slave waits inside
    /// after paying its load. ReadInt at `0x00714E4A`; constructor 0.
    pub slave_reload_rate: i32,
    /// Whether this infantry is a slave unit (Slaved=yes in rules.ini).
    /// Slave units are bound to a master (Slave Miner) and have restricted AI.
    pub slaved: bool,
    /// `Fearless=yes` on InfantryType. Suppresses fear/prone panic changes.
    pub fearless: bool,
    /// `Fraidycat=yes` on InfantryType. First fear hit immediately maxes fear.
    pub fraidycat: bool,
    /// `Crawls=yes` from art.ini. Controls the prone movement speed branch.
    pub crawls: bool,
    /// InfantryType+E40 signed primary standing frame, art.ini `FireUp=`.
    /// ART ReadInt5246D6 retains this field independently; constructor0.
    pub fire_up_frame: i32,
    /// InfantryType+E44 signed primary prone frame, art.ini `FireProne=`.
    /// ART ReadInt5246F3 retains this field independently; constructor0.
    pub fire_prone_frame: i32,
    /// InfantryType+E48 signed secondary standing frame, art.ini `SecondaryFire=`.
    /// ART ReadInt524710 retains this field independently; constructor0.
    pub secondary_fire_frame: i32,
    /// InfantryType+E4C signed secondary prone frame, art.ini `SecondaryProne=`.
    /// ART ReadInt52472D retains this field independently; constructor0.
    pub secondary_prone_frame: i32,
    /// `InfantryType+0xEB8`, `HarvestRate=`: the delay a slave's
    /// `InfantryClass::Mission_Harvest @ 0x00522E70` returns after each cut.
    /// `InfantryTypeClass::ReadINI` ReadInt at `0x0052452B`; the constructor
    /// writes 1 (`0x00523778`).
    pub harvest_rate: i32,
    /// `TechnoType+0x5EC`, `ResourceGatherer=` (ReadBool at `0x007143DF`,
    /// default false): the house counts it on the map (House `+0x158`).
    pub resource_gatherer: bool,
    /// `TechnoType+0x5ED`, `ResourceDestination=` (ReadBool at
    /// `0x007143F9`, default false).
    pub resource_destination: bool,
    /// Whether this building is an Ore Purifier (OrePurifier=yes in rules.ini).
    /// Owning one grants a PurifierBonus to all harvested ore.
    pub ore_purifier: bool,

    // -- Locomotor / movement fields --
    /// Which locomotor class controls this unit's movement (parsed from Locomotor= CLSID).
    pub locomotor: LocomotorKind,
    /// Which terrain cells are traversable (SpeedType= in rules.ini).
    pub speed_type: SpeedType,
    /// Pathfinder routing assumptions (MovementZone= in rules.ini).
    pub movement_zone: MovementZone,
    /// UnitType `MovementRestrictedTo=`. Native constructor default `-1` is
    /// represented as `None`; a parsed value is a canonical CellClass LandType.
    pub movement_restricted_to: Option<LandType>,
    /// Whether this unit is treated as aircraft for game logic (ConsideredAircraft=).
    /// Native `AircraftTypeClass` construction defaults this true; other
    /// categories inherit the false `TechnoTypeClass` default.
    pub considered_aircraft: bool,
    /// Foot draw coefficients: TechnoType constructor 0x710AF0 (+0xDC0..0xDCC)
    /// and ReadINI 0x71541C onward. Absent keys retain 10/5/10/0; these are
    /// combined by signed maximum, not added (Foot 0x4DAFF0).
    pub zfudge_cliff: i32,
    pub zfudge_column: i32,
    pub zfudge_tunnel: i32,
    pub zfudge_bridge: i32,
    /// `TechnoTypeClass+0xE16`. gamemd reads it only in the draw pipeline
    /// (sprite Z fudge on bridge-edge cells); movement and
    /// `UnitClass::Can_Enter_Cell` never read it
    /// (TOO_BIG_TO_FIT_UNDER_BRIDGE_GHIDRA_REPORT.md).
    pub too_big_to_fit_under_bridge: bool,
    /// `Crashable=` (`TechnoTypeClass+0xD95`): a Unit killed in the air crashes
    /// instead of vanishing (`UnitClass::ReceiveDamage 0x00738457`), and only
    /// such a type answers its Jumpjet's impact notice (`0x0074619D`).
    pub crashable: bool,
    /// `TiltCrashJumpjet=` (`TechnoTypeClass+0xD22`, constructor false at
    /// `0x007114CA`, `TechnoTypeClass::ReadINI 0x00713377..0x00713391`, key
    /// string `0x00844118`): the Jumpjet Draw_Matrix tilts the body by its
    /// rocking angles (`0x0054DCDA`). Stock sets it on the Floating Disc.
    pub tilt_crash_jumpjet: bool,
    /// Whether this unit can use chrono teleport movement (Teleporter=).
    pub teleporter: bool,
    /// TechnoType+0xC8D: the constructor stores true at 0x711387 and the
    /// AircraftType constructor stores false at 0x41C9A3. ReadINI
    /// 0x712256..0x71226A reads `MoveToShroud` (key string at 0x8444C4).
    pub move_to_shroud: bool,
    /// Whether this unit can fire while hovering / in air (HoverAttack=).
    pub hover_attack: bool,
    /// Whether this unit stays airborne by default / doesn't land (BalloonHover=).
    pub balloon_hover: bool,
    /// `IsSimpleDeployer=` (`UnitTypeClass+0xE13`, key string `0x00845DFC`).
    /// The Jumpjet cruise reads it on a Unit owner at arrival
    /// (`State3_Translate 0x0054C1FE`).
    pub is_simple_deployer: bool,
    /// UnitType+E18/E19, original747862..747896: the Unit section's
    /// ReadBool SmallVisceroid/LargeVisceroid, distinct from General's type
    /// references with the same keys. Unit Unlimbo737BF5 reads these bytes.
    /// Constructor defaults and executed reads: anytown_damage/unit_unlimbo.
    pub(crate) small_visceroid: bool,
    pub(crate) large_visceroid: bool,
    /// `DeployToLand=` (`TechnoTypeClass+0x6AD`, `TechnoTypeClass::ReadINI`
    /// `0x00714809..0x00714816`, key string `0x00843A90`). Stock sets it on the
    /// Siege Chopper (`[SCHP]`, `[SCHD]`). A Jumpjet Unit with it keeps full
    /// cruise height and holds on arrival instead of descending.
    pub deploy_to_land: bool,
    /// AirportBound=yes — aircraft must dock at helipad; crashes if none available.
    pub airport_bound: bool,
    /// Fighter=yes — fighter aircraft classification (affects targeting).
    pub fighter: bool,
    /// FlyBy=yes — strafing fly-by attack pattern (continue forward after firing).
    pub fly_by: bool,
    /// FlyBack=yes — after fly-by, reverse course back over target.
    pub fly_back: bool,
    /// Landable=yes — aircraft can land on the ground.
    pub landable: bool,
    /// AircraftType+DFC, Carryall reader41CC9B..41CCC7; ctor41C8D0=false.
    /// Aircraft's landing-base query41B6A0 combines it with live cargo/radio.
    pub(crate) carryall: bool,
    /// `JumpJet=` in rules.ini — `TechnoTypeClass+0xD94`, a sibling of the
    /// nine parameters below rather than a gate on them.
    pub jumpjet: bool,
    /// The `TechnoTypeClass+0xD70..+0xD90` jumpjet block. Present on every
    /// type, as in gamemd; only the Jumpjet locomotor ever reads it.
    pub jumpjet_params: JumpjetParams,

    // -- Deploy / undeploy fields --
    /// What this unit deploys into (e.g., AMCV DeploysInto=GACNST).
    /// Parsed from rules.ini `DeploysInto=`. Used for MCV→ConYard and similar transforms.
    pub deploys_into: Option<String>,
    /// Resolved non-null `UndeploysInto=` UnitType identity (e.g. GACNST→AMCV).
    /// `TechnoTypeClass__ReadINI @ 0x00712170`, block
    /// `0x0071329D..0x007132E4`, calls
    /// `UnitTypeClass__FindOrAllocate @ 0x007480D0` and stores its pointer at
    /// `BuildingType+0x408`; native `none`/`<none>` therefore remain `None`.
    pub undeploys_into: Option<String>,
    /// Raw 8-bit facing required before a unit can deploy into this building type.
    /// BuildingType+EDC, read at460C6C..460C86: `DeployFacing=` <<5; default0x80.
    /// This is separate from the immutable BuildingType+ED8 below.
    pub deploy_facing: u8,
    /// Whether this building is a construction yard. Enables ConYard-only MCV repack gates.
    pub construction_yard: bool,
    /// Immutable membership in the resolved `[AI] BuildConst=` BuildingType
    /// pointer vector (`RulesClass__ReadAI @ 0x00672AE0`, binding
    /// `0x00672B14..0x00672C01`). `RuleSet` stamps this after all type
    /// registries exist.
    pub build_const_eligible: bool,
    /// Native BuildingType registry index used by ordered House BasePlan nodes.
    /// Non-building types retain `-1`.
    pub base_plan_type_index: i32,
    /// BuildingType `IsBaseDefense=` immutable lifecycle input at
    /// `BuildingType+0x1706`. The BuildingType constructor store at
    /// `0x0045E225` defaults it false; reader block
    /// `0x00460FFC..0x00461010` binds it.
    pub is_base_defense: bool,
    /// BuildingType `HoverPad=`, `IsTemple=` and `IsPlug=` (`+0x154E`,
    /// `+0x154C`, `+0x154D`): ReadBool with the field as default
    /// (`0x00460573..0x004605D2`); the constructor clears them
    /// (`0x0045DFC3`, `0x0045DFE0`, `0x0045DFEC`). Other types never read
    /// them. A computer house values an enemy building by them when it aims
    /// a superweapon (`sim::superweapon::ai_fire`); no retail building sets
    /// one.
    pub hover_pad: bool,
    pub is_temple: bool,
    pub is_plug: bool,
    /// BuildingType `AntiAirValue=`, `AntiArmorValue=` and
    /// `AntiInfantryValue=` (`+0x1524`, `+0x1528`, `+0x152C`): signed ReadInt
    /// with the field as default (`0x0045FED2`, `0x0045FEB8`, `0x0045FE9E`);
    /// the constructor stores 0 (`0x0045DF79..0x0045DF87`). Other types never
    /// read them. The computer weighs its base defense choice and its
    /// defensive coverage by them (`sim::ai_base_defense`).
    pub anti_air_value: i32,
    pub anti_armor_value: i32,
    pub anti_infantry_value: i32,

    /// Whether this unit can be crushed by vehicles with Crusher movement zones.
    /// Default: false for all types. Parsed from `Crushable=` in rules.ini.
    /// Only specific infantry (GI, GGI, SEAL, Rocketeer, Lunar) and some walls
    /// have this set to true.
    pub crushable: bool,
    /// Whether deployed infantry remains crushable by regular crushers.
    /// Defaults to true; stock Guardian GI overrides this with `DeployedCrushable=no`.
    pub deployed_crushable: bool,
    /// Whether this unit has normal TechnoType `Crusher=yes` capability.
    /// Distinct from `OmniCrusher=` and MovementZone crusher names.
    pub crusher: bool,
    /// Whether this unit can crush non-Crushable targets (only Battle Fortress).
    /// Default: false. Parsed from `OmniCrusher=` in rules.ini.
    pub omni_crusher: bool,
    /// Whether this unit is immune to ALL crush types including OmniCrusher.
    /// Default: false. Parsed from `OmniCrushResistant=` in rules.ini.
    pub omni_crush_resistant: bool,
    /// Whether this unit ignores per-cell radiation damage (Desolator, Chrono
    /// Ivan, etc.). Default: false. Parsed from `ImmuneToRadiation=` in
    /// rules.ini. Also selects the deployed self-irradiate mission behaviour
    /// for deploy-fire units whose deploy weapon emits radiation.
    pub immune_to_radiation: bool,
    /// Allows this object to be admitted as its own ground AoE receiver.
    /// CrushWarhead independently admits self regardless of this flag.
    pub damage_self: bool,
    /// ObjectClass receiver immunity (`Immune=`). With defenses enabled this
    /// rejects the request before the ordinary damage kernel and callbacks.
    pub immune: bool,
    /// Nullifies damage from an exact same-type, same-owner source object.
    pub type_immune: bool,
    /// Psychedelic/mind-control immunity. Buildings default true natively.
    pub immune_to_psionics: bool,
    /// `Warpable=` (`TechnoTypeClass+0xD3A`, ReadINI `0x00714F65..0x00714F79`;
    /// constructor default 1 at `0x0071140D`/`0x00711542`): a Chrono
    /// Legionnaire may start erasing it (`TemporalClass::CanWarpTarget @
    /// 0x0071AE50`). No stock section authors the key.
    pub warpable: bool,
    /// `Bombable=` (`ObjectTypeClass+0x22E`, ReadINI `0x005F942C`;
    /// constructor default 1 at `0x005F715C`). Its only reader is the Ivan's
    /// cursor (`InfantryClass::What_Action_OnObject` `0x0051EB24`): neither
    /// GetFireError, target acquisition nor the bomb's attach tests it.
    pub bombable: bool,
    /// `BombSight=` in cells (`TechnoTypeClass+0x5F8`, ReadINI `0x00714329`):
    /// its owner sees Ivan bombs within that 3-D radius
    /// (`BombListClass::UpdateAll @ 0x00438BF0`). Stock: the three Engineers, 4.
    pub bomb_sight: i32,
    /// `MindControlRingOffset=` (`TechnoTypeClass+0x60C`, ReadINI
    /// `0x00714350`; constructor default 0x8C at `0x0071103A`): the capture
    /// ring's height above a captured non-building's coordinate.
    pub mind_control_ring_offset: i32,
    /// A type's own `MindClearedSound=` (`TechnoTypeClass+0x5B0`, ReadINI
    /// `0x0071395E`); FreeUnit prefers it to the global one. No stock type
    /// authors it.
    pub mind_cleared_sound: Option<String>,
    /// PsychicDamage immunity. Buildings default true natively.
    pub immune_to_psionic_weapons: bool,
    /// Poison-warhead immunity.
    pub immune_to_poison: bool,

    /// What type of objects this building can produce (Factory= in rules.ini).
    /// None for non-factory buildings/units. Data-driven replacement for
    /// hardcoded building-name checks in production queue logic.
    pub factory: Option<FactoryType>,
    /// Native BuildingType `WeaponsFactory=` classification used by Unit
    /// ReadyToCommence. Independent from `Factory=` and `Naval=`.
    pub weapons_factory: bool,
    /// BuildingType+16C2: constructor45E157 clears it. Rules45FE50 reads
    /// exact-case Armory via ReadBool5295F0 at460B03, using the current byte
    /// as default, then stores AL460B08 before GDIBarracks. Exit4440A6 uses it.
    armory: bool,
    /// BuildingType+16E4, rules `GDIBarracks=`: constructor45E1C2 clears
    /// it; ReadBool5295F0 at460B1D uses the current byte as its default.
    /// Source: building_construction.py and the sealed native barracks input
    /// contract in basic-factory-output-event-admission-research (retail
    /// gamemd.exe SHA256 1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c).
    gdi_barracks: bool,
    /// BuildingType+16E5, rules `NODBarracks=`: constructor45E1C8 clears
    /// it; ReadBool5295F0 at460B37 follows GDIBarracks with a live default.
    nod_barracks: bool,
    /// BuildingType+16E6, rules `YuriBarracks=`: constructor45E1CE clears
    /// it; ReadBool5295F0 at460B51 follows NODBarracks with a live default.
    yuri_barracks: bool,
    /// Whether this building clones produced infantry (Cloning=yes in rules.ini).
    pub cloning: bool,

    /// Exit coordinate for produced units, in leptons relative to building origin.
    /// Parsed from `ExitCoord=X,Y,Z` in rules.ini. 256 leptons = 1 cell.
    /// Used by spawn logic to place newly built units near the correct factory exit.
    pub exit_coord: Option<(i32, i32, i32)>,

    // -- Cursor / interaction capability flags --
    // These drive which cursor is shown when hovering this unit/building.
    /// Whether this infantry type behaves as an engineer (captures buildings,
    /// repairs structures). Parsed from `Engineer=yes` in rules.ini.
    /// Triggers `EngineerRepair` cursor on damaged friendly buildings and
    /// `Enter` cursor on capturable enemy buildings when this unit is selected.
    pub engineer: bool,
    /// `Ivan=` (`InfantryTypeClass+0xEAE`, ReadINI `0x005244C3`, infantry
    /// only): the bomb cursor (`0x0051EB24..0x0051EB7E`), its only reader.
    pub ivan: bool,
    /// `Infiltrate=` (`InfantryTypeClass+0xEBE`, ReadBool `0x005244A9`,
    /// constructor clear `0x0052378F`, infantry only): an unarmed infiltrator
    /// still scans for a target (`0x0051E296..0x0051E2BC`), and a team on
    /// action 15 sends it in (`0x006EB71A..0x006EB759`).
    pub infiltrate: bool,

    /// Whether this unit can self-deploy/undeploy via the Deploy command.
    /// Parsed from `Deployer=yes` in rules.ini. Triggers `Deploy`/`NoDeploy`
    /// cursor when the player hovers over this unit itself.
    pub deployer: bool,

    /// Whether this building can be infiltrated by a spy or captured by an engineer.
    /// Parsed from `Capturable=yes` in rules.ini. Enables the `Enter` cursor
    /// when an enemy Engineer or Spy is selected and hovering this building.
    pub capturable: bool,

    /// `NeedsEngineer=` — `BuildingTypeClass::ReadINI 0x0046023E..0x0046024B`
    /// stores it at `+0x1552`. A tech building that starts neutral and is
    /// taken by an engineer; `BuildingClass::ChangeOwner 0x00448401` routes
    /// the capture announcement on it (`EVA_TechBuildingLost` for the local
    /// old owner instead of the radar-gated `EVA_BuildingCaptured`).
    pub needs_engineer: bool,

    /// `CaptureEvaEvent=` — `BuildingTypeClass::ReadINI 0x00460258..0x00460265`
    /// stores the `VoxClass` entry index at `+0x1554` (`-1` when absent).
    /// `BuildingClass::ChangeOwner 0x00448443..0x00448459` queues it
    /// (`QueueVoice(index, -1, -1)`) for a local NEW owner of a
    /// `NeedsEngineer=` building (stock: `EVA_OilRefineryCaptured` on
    /// `CAOILD`, `EVA_HospitalCaptured`, ...). The name is kept; the app
    /// resolves it through the EVA registry.
    pub capture_eva_event: Option<String>,

    /// Whether this building can be repaired via the Repair command.
    /// Parsed from `Repairable=yes` in rules.ini. Defaults to true for buildings.
    pub repairable: bool,

    /// Whether infantry can garrison/occupy this building.
    /// Parsed from `CanBeOccupied=yes` in rules.ini. Enables `Enter` cursor
    /// for friendly infantry hovering this building.
    pub can_be_occupied: bool,

    /// Whether garrisoned infantry can fire from this building.
    /// Parsed from `CanOccupyFire=yes` in rules.ini. Building must also
    /// have `CanBeOccupied=yes` and at least one occupant for fire to occur.
    pub can_occupy_fire: bool,

    /// Whether to show pip indicators for each occupant inside the building.
    /// Parsed from `ShowOccupantPips=yes` in rules.ini.
    pub show_occupant_pips: bool,

    /// Whether Engineer entry into this building triggers bridge-segment
    /// repair on the nearest damaged bridge. Parsed from
    /// `BridgeRepairHut=yes` in rules.ini. Stock CABHUT is the only
    /// consumer in retail. Default `false`.
    pub bridge_repair_hut: bool,

    /// Whether this building type uses the LaserFence runtime connectivity
    /// exclusion in Spark collision (`LaserFence=yes`).
    pub laser_fence: bool,
    /// BuildingType+0x16BE, cleared by the constructor (`0x0045E13F`) and read
    /// as `LaserFencePost=` at `0x00460A93`. No retail type sets it.
    pub laser_fence_post: bool,

    /// BuildingType+16C0, initialized false45E14B; ReadBool460AC0 reads
    /// FirestormWall. Body frame43EF90 selects the retained firestorm frame.
    pub firestorm_wall: bool,

    /// Signed native `Passengers=` value at `TechnoTypeClass+0x5E0`.
    /// Positive values allocate transport capacity; TeamType post-load zone
    /// derivation distinguishes exact zero from every signed nonzero value.
    pub passengers: i32,

    /// Maximum Size= of individual passenger allowed (SizeLimit= in rules.ini).
    /// 0 means no size restriction. SizeLimit=2 means only Size<=2 can enter.
    pub size_limit: u32,

    /// How much transport space this unit occupies (Size= in rules.ini).
    /// Infantry default 1, vehicles default 3.
    pub size: u32,

    /// Whether this transport is open-topped — passengers can fire from inside.
    /// Parsed from `OpenTopped=yes` in rules.ini.
    pub open_topped: bool,

    /// Whether this transport uses the Gunner system (IFV weapon swap).
    /// Parsed from `Gunner=yes` in rules.ini. When a passenger enters, the
    /// transport's active weapon changes based on the passenger's IFVMode.
    pub gunner: bool,

    /// Weapon mode selected when this passenger enters a Gunner transport.
    /// TechnoType+688: constructor7110F2 initializes 0; ReadINI714780..71479A
    /// uses signed ReadInt(current). The gunner selector owns mode validation.
    pub ifv_mode: i32,

    /// Retained charge flag and IFV weapon-to-turret mapping. The native Rules
    /// process owns per-pass writes; this is its immutable runtime projection.
    pub gunner_turrets: crate::rules::gunner_turrets::GunnerTurrets,

    /// `OpenTransportWeapon=` from rules.ini. Slot selector when this infantry
    /// is riding inside an open-topped transport that is NOT `Gunner=yes`:
    /// `0` fires the passenger's Primary, `1` fires the passenger's Secondary,
    /// `-1` (default) means no override — the transport doesn't fire on the
    /// passenger's behalf. Distinct from IFVMode, which only applies to Gunner
    /// transports that swap their own weapon based on the passenger's slot.
    pub open_transport_weapon: i32,

    /// Whether this infantry can toggle into a deploy-fire stance (DeployFire=yes
    /// in rules.ini). Only deploy-fire types respond to `Command::ToggleInfantryDeploy`.
    /// Stock YR sets this on GI (E1), GuardianGI (GGI), and a handful of others.
    pub deploy_fire: bool,

    /// `UndeployDelay=` int — `TechnoTypeClass+0x6C4`, read by
    /// `TechnoTypeClass::ReadINI` (key string `0x008438F4` =
    /// `"UndeployDelay"`, `ReadInt @ 0x005276D0`, store `0x00714BBA`).
    /// Constructor default **-1** (`TechnoTypeClass::Constructor` loads
    /// `EBP = -1` at `0x00710CED` and stores it at `0x00711187`).
    ///
    /// The infantry deploy shim `FUN_00521320` — the body behind
    /// `InfantryClass::Mission_Guard @ 0x0051F620` and
    /// `InfantryClass::Mission_AreaGuard @ 0x0051F640` — tests `-1 < this`
    /// FIRST for an already-deployed man and makes him undeploy when it holds,
    /// ahead of every `DeployFire=` arm. Stock: only `YURI` (150) and `YURIPR`
    /// (75) set it, which is why a deployed Yuri leaves his stance on its own
    /// and a deployed GI does not.
    ///
    /// **Frame trap:** this is the TYPE field. The `InfantryClass` *instance*
    /// carries its DoType at the same `+0x6C4`, and a `UnitClass` instance
    /// carries a `UnitTypeClass*` there.
    pub undeploy_delay: i32,

    /// `FireAngle=` int — `TechnoTypeClass+0x3D0`, read by
    /// `TechnoTypeClass::ReadINI` (key string `0x00843910`, `ReadInt @
    /// 0x005276D0` with the field as default, store `0x00714B71`).
    /// Constructor default 8 (`0x00710D12`).
    ///
    /// `TechnoClass::Unlimbo` aims the barrel elevation (`+0x370`) at
    /// `0x4000 - (low byte << 8)` (`0x006F6DD9..0x006F6DF5`), so the default
    /// raises a voxel unit's barrel one 11.25° step. Retail sets it on ships,
    /// submarines, missiles and the tech outpost, none of which draws a unit
    /// barrel.
    pub fire_angle: i32,

    /// `DeployFireWeapon=` (`TechnoTypeClass+0x6A8`, read at `0x007147DD`): the
    /// weapon slot a deployed unit fires. Constructor default slot 1
    /// (`0x0071113A`).
    pub deploy_fire_weapon: i32,

    /// Maximum number of garrison occupants for CanBeOccupied buildings.
    /// Parsed from `MaxNumberOccupants=N` in rules.ini. Default 0.
    pub max_number_occupants: u32,

    /// Whether this infantry can garrison `CanBeOccupied` buildings.
    /// Parsed from `Occupier=yes` in rules.ini. Only GI and Conscript in base RA2.
    pub occupier: bool,

    /// Whether this infantry can assault enemy buildings (hostile garrison entry).
    /// Parsed from `Assaulter=yes` in rules.ini.
    pub assaulter: bool,

    /// `VehicleThief=` bool — `InfantryTypeClass+0xEC6`, read by
    /// `InfantryTypeClass::ReadINI` (key string `0x0082593C` = `"VehicleThief"`,
    /// default loaded from the field itself at `0x005245CC`, `ReadBool @
    /// 0x005295F0` called at `0x005245E1`).
    ///
    /// The last of the three type arms in `FootClass::Mission_Hunt @
    /// 0x004D5350` (`0x004D546C`): a hunting thief walks onto its target and
    /// queues Capture(8) instead of shooting. **No stock section sets it**, so
    /// the arm is dead on retail data; it is parsed because the Hunt body
    /// branches on it and a map or mod INI can set it.
    pub vehicle_thief: bool,

    /// Weapon used when this infantry fires from inside a garrisoned building.
    /// Parsed from `OccupyWeapon=WeaponName` in rules.ini. Falls back to
    /// primary weapon if not specified.
    pub occupy_weapon: Option<String>,

    /// Elite-level weapon used when garrisoned. Falls back to `OccupyWeapon`
    /// or primary weapon if not specified.
    /// Parsed from `EliteOccupyWeapon=WeaponName` in rules.ini.
    pub elite_occupy_weapon: Option<String>,

    /// pips.shp frame index for this infantry when garrisoned in a building.
    /// Parsed from `OccupyPip=PersonGreen` in rules.ini.  Default 7 (PersonGreen).
    /// Values: 7=PersonGreen, 8=PersonYellow, 9=PersonWhite, 10=PersonRed,
    /// 11=PersonBlue, 12=PersonPurple.  Empty slots use frame 6.
    pub occupy_pip: u32,

    /// What pip display to render below the health bar (PipScale= in rules.ini).
    /// `Tiberium` shows per-bale cargo pips for harvesters using pips2.shp.
    pub pip_scale: PipScale,

    /// Whether this building absorbs infantry (Yuri Bio Reactor).
    /// Parsed from `InfantryAbsorb=yes` in rules.ini.
    pub infantry_absorb: bool,

    /// Whether this building absorbs vehicles.
    /// Parsed from `UnitAbsorb=yes` in rules.ini.
    pub unit_absorb: bool,

    /// Whether this building grinds entering units (`BuildingTypeClass+0x16AD`).
    /// gamemd 0x45E0D8 defaults false; 0x460968..0x460982 reads `Grinding=`.
    /// The Unit cell-entry receiver tests this separately from UnitAbsorb.
    pub grinding: bool,
    /// BuildingType+16C1, ctor45E151=false; ReadBool460AD4..460AFD
    /// exact Hospital key. The treatment lifecycle is a separate mechanism.
    pub hospital: bool,

    /// Whether this techno type can enter a Tank Bunker.
    /// Parsed from `Bunkerable=` in rules.ini. UnitTypeClass entries default
    /// true; other object categories default false.
    pub bunkerable: bool,

    /// The native base weapon array (`TechnoTypeClass+0x898`, stride `0x1C`,
    /// `WEAPON_SLOT_COUNT` slots). Always that long; empty slots are `None`.
    ///
    /// **This array is the storage `Primary` and `Secondary` live in** —
    /// slot 0 *is* [`Self::primary`] and slot 1 *is* [`Self::secondary`],
    /// because `Primary=` and `Weapon1=` write the same field in gamemd. Which
    /// INI keys fill it is decided by the mutually exclusive `TurretCount`
    /// branch in `ObjectType::read_weapon_arrays`, where the full ReadINI
    /// derivation and its addresses live. Resolution (elite tier, index) lives
    /// in `sim::combat::combat_weapon::weapon_for_index`.
    pub weapon_list: Vec<Option<String>>,

    /// The native elite weapon array (`TechnoTypeClass+0xA94`, stride `0x1C`).
    /// Same shape and same storage relationship: slot 0 is
    /// [`Self::elite_primary`], slot 1 is [`Self::elite_secondary`].
    pub elite_weapon_list: Vec<Option<String>>,

    /// `WeaponCount=` (`TechnoTypeClass+0x80C`, ReadINI `0x00712873`). Bounds
    /// the `Weapon%d`/`EliteWeapon%d` loop; slots at or past it keep their
    /// constructor value, so a `TurretCount>0` type with `WeaponCount=1`
    /// (stock `[SREF]`) has no secondary at all. That bound is applied once, in
    /// `ObjectType::read_weapon_arrays`, so `weapon_list` already reflects it
    /// and readers never re-apply it. Constructor default 0: the
    /// ReadINI store at `0x00712878` is the only write to `+0x80C` in the
    /// image, so the field arrives block-cleared (UNCHECKED — the clearing
    /// instruction itself was not located).
    pub weapon_count: i32,

    /// `NavalTargeting=` (`TechnoTypeClass+0x600`, ReadINI `0x007121CB`,
    /// ctor default 0 @ `0x00711024`). Consumed by
    /// `TechnoClass::SelectNavalTargetingWeapon @ 0x006F3820`.
    pub naval_targeting: i32,

    /// `LandTargeting=` (`TechnoTypeClass+0x604`, ReadINI `0x007121B1`,
    /// ctor default 0 @ `0x0071102A`). `2` selects slot 1 against land in
    /// `What_Weapon_Should_I_Use`; `1` is a `GetFireError` ILLEGAL verdict.
    pub land_targeting: i32,

    /// `Underwater=` (`TechnoTypeClass+0xD69`, ReadINI `0x00714D88`).
    pub underwater: bool,

    /// `Organic=` (`TechnoTypeClass+0xD97`, ReadINI `0x0071503F`). The
    /// InfantryType constructor stores 1 (`0x00523911`); Unit, Aircraft and
    /// Building constructors leave 0.
    pub organic: bool,

    /// `Parasiteable=` (`TechnoTypeClass+0xD38`, ReadINI `0x00714F9A`),
    /// read by ParasiteClass CanInfect `0x0062A8E0`. Infantry (`0x0052390A`),
    /// Unit (`0x00747297`) and Aircraft (`0x0041C997`) constructors store 1;
    /// BuildingType leaves 0.
    pub parasiteable: bool,

    /// `SuppressionThreshold=` (`TechnoTypeClass+0xD6C`, ReadINI `0x0071506D`).
    /// FootClass ReceiveDamage `0x004D7330` arms the eater's suppression when
    /// a third party's raw damage exceeds this eater-type value.
    pub suppression_threshold: i32,

    /// `ReselectIfLimboed=` (`TechnoTypeClass+0xD3C`, `0x007142C1`) and
    /// `RejoinTeamIfLimboed=` (`+0xD3D`, `0x007142DB`): LimboLaunch memo gates
    /// in TechnoClass::Fire `0x006FF763`/`0x006FF7A3`.
    pub reselect_if_limboed: bool,
    pub rejoin_team_if_limboed: bool,

    /// `Unnatural=` (`TechnoTypeClass+0x694`, ReadINI `0x0071496D`).
    pub unnatural: bool,

    /// GetFireError's type flags, each zeroed by its class constructor
    /// unless noted and read with the current value as default:
    /// - `Natural=` (`TechnoTypeClass+0x693`, ReadINI `0x00714953`): a
    ///   Natural attacker never fires at an Unnatural target (T14).
    /// - `Pushy=` (`+0x692`, key `0x008439E4`): I3.
    /// - `BerserkFriendly=` (`+0x690`, key `0x008439F8`): a berserk attacker
    ///   spares it (T13).
    /// - `MobileFire=` (`+0x6AE`, ReadINI `0x00714830`; constructor 1 at
    ///   `0x00711150`): U7.
    /// - `HunterSeeker=` (`+0xD27`, ReadINI `0x00714CC2`): T49.
    /// - `NonVehicle=` (UnitType `+0xE1B`, UnitTypeClass::ReadINI
    ///   `0x007478B0`): U6's vehicle test.
    /// - `JumpJetTurn=` (InfantryType `+0xECB`, InfantryTypeClass::ReadINI
    ///   `0x00524668`): I6.
    /// - `EMPulseCannon=` (BuildingType `+0x16C3`, BuildingTypeClass::ReadINI
    ///   `0x00460B76`): B3.
    pub natural: bool,
    pub pushy: bool,
    pub berserk_friendly: bool,
    pub mobile_fire: bool,
    pub hunter_seeker: bool,
    pub non_vehicle: bool,
    pub jumpjet_turn: bool,
    pub emp_pulse_cannon: bool,
    /// BuildingType `HasStupidGuardMode=` (`+0x16B5`, ReadINI `0x00460EB4`,
    /// read over the BuildingTypeClass constructor's true, `0x0045E108`). An
    /// unarmed building's Mission_Guard returns 100 frames at once while it
    /// holds (`0x004497E4..0x00449800`); retail clears it on the depots,
    /// airfields, the missile silo and the defences.
    pub has_stupid_guard_mode: bool,
    /// BuildingType `TickTank=` (`+0x16C4`, ReadINI `0x00460B95`, constructor
    /// false). No retail type sets it.
    pub tick_tank: bool,

    /// `IsGattling=` (`TechnoTypeClass+0xCD5`, ReadINI `0x0071402A`).
    pub is_gattling: bool,
    /// BuildingType `Artillary=` (`+0x16CA`, read at `0x00460F56`). A human's
    /// unit whose `DeploysInto=` building sets it never retaliates
    /// (`ShouldRetaliate 0x007089C5..0x007089E2`). No retail type sets it.
    pub artillary: bool,
    /// `WeaponStages=`, `Stage%d=`, `EliteStage%d=`, `RateUp=`, `RateDown=`
    /// (`TechnoTypeClass+0xCD8..+0xD10`, ReadINI `0x00714030..0x0071410F`).
    pub gattling_stages: crate::rules::gattling_type::GattlingStages,

    /// `TurretCount=` (`TechnoTypeClass+0x808`, ReadINI `0x0071285E`, ctor
    /// default 0 @ `0x0071136F`). `> 0` short-circuits weapon selection to
    /// `CurrentWeaponNumber` unless the type is gattling.
    pub turret_count: i32,

    /// `Drainable=` (`TechnoTypeClass+0x5EF`, ReadINI `0x007143B0`).
    pub drainable: bool,

    /// `ProduceCashStartup=` (`BuildingTypeClass+0x1558`). Credited to the NEW
    /// owner by `BuildingClass::ChangeOwner @ 0x004482BD..0x004482D0` when the
    /// OLD owner's HouseType is `MultiplayPassive` and the value is non-zero;
    /// the same branch arms the ProduceCash timer (`+0x6D0`/`+0x6D8`).
    pub produce_cash_startup: i32,
    /// `ProduceCashAmount=` (`BuildingTypeClass+0x155C`). `BuildingClass::Update
    /// @ 0x0043FDAA..0x0043FDD1`: `> 0` → `Add_Credits`, `<= 0` →
    /// `Spend_Money(-amount)`, once per timer expiry while operational.
    pub produce_cash_amount: i32,
    /// `ProduceCashDelay=` (`BuildingTypeClass+0x1560`). Timer duration stored
    /// at `+0x6D8` on arm/re-arm; the timer fires when its remaining count
    /// reads exactly 1 (`0x0043FD56 CMP ECX,1`), i.e. every `Delay - 1` frames.
    pub produce_cash_delay: i32,

    /// `Overpowerable=` (`BuildingTypeClass+0x1575`, ReadINI `0x00460029`).
    /// Constructor default UNCHECKED; every stock author writes it explicitly.
    pub overpowerable: bool,

    /// Whether this unit shows an `Attack` cursor even on friendly targets.
    /// Parsed from `AttackCursorOnFriendlies=yes` in rules.ini.
    /// Used by Desolator and Boris whose weapons affect friendlies.
    pub attack_cursor_on_friendlies: bool,

    /// Whether this infantry uses the `Enter`/sabotage cursor instead of
    /// the normal `Attack` cursor on enemy structures.
    /// Parsed from `SabotageCursor=yes` in rules.ini. Used by Tanya and Navy SEAL.
    pub sabotage_cursor: bool,

    /// `C4=yes` on InfantryType. Gates the player-issued C4 plant mission path
    /// (SEAL, Tanya, Psi-Corp Trooper). Distinct from `sabotage_cursor`, which
    /// is now purely a modder-flag for cursor display on weapons; the live
    /// cursor + click behavior is driven by `c4 + can_c4` instead.
    pub c4: bool,

    /// `CanC4=yes` on BuildingType. When false, the building cannot be C4'd by
    /// SEAL/Tanya/PTROOP. Default `true` for buildings, `false` for non-buildings.
    /// Stock buildings opting out in retail rulesmd.ini: CAMISC01 (Concrete
    /// Barrel), CAMISC02 (Wooden Barrel), CAMISC06 (Civilian Barrel variant),
    /// AMMOCRAT (Ammo Crate).
    pub can_c4: bool,

    /// BuildingTypeClass `EligibleForDelayKill` (`+0x1551`). Native default
    /// false; only eligible buildings can convert a fatal result to PostMortem.
    pub eligible_for_delay_kill: bool,

    /// `Invisible=yes` on BuildingType. Live building rejection checks this
    /// plain invisible byte separately from `InvisibleInGame`.
    pub invisible: bool,

    /// `InvisibleInGame=yes` on BuildingType. Logical-only buildings (e.g., bridge
    /// anchors) that should not receive C4 or other interaction cursors.
    pub invisible_in_game: bool,
    /// BuildingType+0x1703, the immediate placement admission read by
    /// 0x464AC0; the constructor clears it at 0x45E212 and ReadINI
    /// 0x460F08..0x460F1C reads `PlaceAnywhere`. Retail AMMOCRAT and UFO set it.
    pub place_anywhere: bool,
    /// Authored `ToTile` key; 0x465CC0 resolves it through 0x544CE0 into
    /// BuildingType+0xE58 only when the named tile is registered.
    /// The theater-aware receiver must validate the named tile before treating
    /// this as a nonnull type pointer (retail GAGREEN uses Green01).
    pub to_tile: Option<String>,

    /// Whether this building repairs docked ground units (UnitRepair=yes in rules.ini).
    /// Used by Service Depots (GADEPT, NADEPT, YADEPT).
    pub unit_repair: bool,
    /// Whether this building is a Tank Bunker (Bunker=yes in rules.ini).
    /// Stock YR uses this on NATBNK. The live pathing helper treats empty and
    /// occupied bunkers differently, so this must be data-driven rather than a
    /// NATBNK string check.
    pub bunker: bool,
    /// Whether this building reloads ammo for docked aircraft (UnitReload=yes in rules.ini).
    /// Used by Airfields (GAAIRC, NAAIRC).
    pub unit_reload: bool,
    /// Whether this building is a helipad (Helipad=yes in rules.ini).
    pub helipad: bool,
    /// Signed BuildingType+1780 (`NumberOfDocks`), constructor45E28A default1.
    /// ReadInteger46492E preserves zero/negative counts for coordinate queries.
    /// Contact allocation applies its separate minimum through `dock_contact_capacity`.
    pub number_of_docks: i32,

    /// Whether this building can be toggled on/off by the player.
    /// Parsed from `TogglePower=yes` in rules.ini.
    /// Defaults to true for buildings (most can be powered down).
    /// Triggers `TogglePower` cursor when hovering this building in power-toggle mode.
    pub toggle_power: bool,

    /// `InfantryGainSelfHeal=` (`BuildingTypeClass+0x1564`, read by the
    /// BuildingType reader). `BuildingClass::OnConstructionComplete` adds it to
    /// the house's infantry count (`+0x164`, `0x00446392..0x00446398`), Limbo
    /// and ChangeOwner take it back. Non-zero on the retail Tech Hospital.
    pub infantry_gain_self_heal: i32,
    /// `UnitsGainSelfHeal=` (`BuildingTypeClass+0x1568`). The same three sites
    /// move it through the house's unit count (`+0x168`). Non-zero on the
    /// retail Tech Machine Shop.
    pub units_gain_self_heal: i32,

    /// Whether this building is affected by low-power situations.
    /// Parsed from `Powered=yes`; native constructor45E04B defaults false.
    /// When true and the owner is in low power, the building deactivates:
    /// defenses stop firing, radar goes offline, gap/spysat/superweapons pause.
    /// Power plants (positive Power=) are never deactivated regardless of this flag.
    pub powered: bool,
    /// BuildingType+1574, constructor45E051 and ReadBool46000A.
    pub powered_special: bool,

    /// Whether this unit can use the disguise ability (Spy).
    /// Parsed from `CanDisguise=yes` in rules.ini. Enables `Disguise` cursor
    /// when the selected Spy hovers over an eligible enemy infantry target.
    pub can_disguise: bool,
    /// TechnoType+0xD30, constructor710AF0 false. UnitAI736486 and
    /// TechnoReceiveDamage701FE6 exclude permanent disguises.
    pub perma_disguise: bool,
    /// `DisguiseWhenStill=` — UnitClass idle Mirage disguise lifecycle gate.
    pub disguise_when_still: bool,

    /// Whether this building is a wall segment (Wall=yes in rules.ini).
    /// Wall buildings render as overlays (auto-tiled connectivity frames),
    /// not as normal SHP building sprites. GAWALL, NAWALL, GAFWLL etc.
    pub wall: bool,
    /// ART `ToOverlay=` identity merged after rules parsing.
    pub to_overlay: Option<String>,
    /// `Unsellable=` BuildingType gate. Native default is false.
    pub unsellable: bool,
    /// `ClickRepairable=` BuildingType byte. Wall selling does not consult it.
    pub click_repairable: bool,

    /// Whether the player may put this object into the selection group
    /// (`Selectable=` in rules.ini). gamemd reads this on the object *type* and
    /// consults it from `CanBeSelected`, which `ObjectClass::Select` calls as its
    /// last rejection test — so a `Selectable=no` object can never join the
    /// selection and never receives player orders. Stock uses it for the
    /// scripted aircraft (`PDPLANE`, `SPYP`, `BPLN`), walls, and civilian props.
    /// Omission means yes: the type constructor seeds the field true and the INI
    /// read passes that seed as its default.
    pub selectable: bool,

    // -- Naval flags --
    /// Building requires water placement (WaterBound=yes in INI).
    /// When set, the placement validator checks the water speed column instead
    /// of Buildable. Default: true if SpeedType is Float, false otherwise.
    pub water_bound: bool,
    /// Unit or building is classified as naval (Naval=yes in INI).
    /// Controls AI targeting priority, factory classification, and UI filtering.
    pub naval: bool,
    /// Number of foundation columns from the west edge (X-axis) that remain
    /// impassable in the UnitClass live building-occupant helper. Default -1
    /// keeps the whole checked building as a blocker. Parsed from
    /// `NumberImpassableRows=` despite the native name.
    pub number_impassable_rows: i32,

    // -- Point light source fields (from rules.ini, primarily buildings) --
    /// Light emission range in leptons (LightVisibility= in rules.ini). Default 5000.
    /// 256 leptons = 1 cell. Used by lamp posts (GALITE=5000) and other light-emitting buildings.
    pub light_visibility: i32,
    /// Light emission brightness (LightIntensity= in rules.ini). Default 0.0.
    /// Negative values darken the area. Typical range: 0.0–1.0.
    pub light_intensity: f32,
    /// Red channel tint for emitted light (LightRedTint= in rules.ini). Default 1.0.
    pub light_red_tint: f32,
    /// Green channel tint for emitted light (LightGreenTint= in rules.ini). Default 1.0.
    pub light_green_tint: f32,
    /// Blue channel tint for emitted light (LightBlueTint= in rules.ini). Default 1.0.
    pub light_blue_tint: f32,
    /// `HasSpotlight=` — owns a map-presence-bound BuildingLight child.
    pub has_spotlight: bool,

    // -- TechnoType particle effects --
    // String fields below hold unresolved particle-system section names; ID
    // resolution against the particle-system registry is deferred (matches
    // the same deferred-typing decision used for ParticleType.warhead and
    // ParticleSystemType.holds_what's parse-side captures).
    /// `NaturalParticleSystem=` — gap-generator / cloak-related particle
    /// system. Live code path in YR (BuildingClass::UpdateGapGenerator_Tick),
    /// but no retail INI sets the key, so the slot is normally null.
    pub natural_particle_system: Option<String>,
    /// `NaturalParticleLocation=` X,Y,Z offset paired with the system above.
    pub natural_particle_location: IVec3,
    /// `RefinerySmokeParticleSystem=` — chimney smoke for refineries.
    pub refinery_smoke_particle_system: Option<String>,
    /// `DamageParticleSystems=` CSV list — spawned periodically while the
    /// object is damaged.
    pub damage_particle_systems: Vec<String>,
    /// `MaxDebris=` — the ceiling on how many debris objects this type throws
    /// when it dies. `TechnoClass::ReceiveDamage @ 0x00701900` draws the count
    /// from `[MinDebris, MaxDebris]` and spends it on `DebrisTypes` VoxelAnims
    /// and `DebrisAnims` SHP anims.
    ///
    /// 456 stock sections author some spelling of it, but gamemd reads the key
    /// case-exactly, so only the **439** spelled `MaxDebris=` count. The other
    /// 17 spell it `Maxdebris=3` and keep the constructor default of 0
    /// (`TechnoTypeClass::Constructor 0x00710FB7`), which is why the Rhino, the
    /// Apocalypse and 12 more buildable vehicles throw no death debris at all.
    pub max_debris: i32,
    /// `MinDebris=` — the floor of the same range, read at `0x007125A8` into
    /// `TechnoType+0x5C0` and floored to 0 at `0x007125BD`. 272 stock sections
    /// author it, every one of them a `[BuildingTypes]` member.
    pub min_debris: i32,
    /// `DebrisTypes=` CSV of `[VoxelAnims]` ids. 36 stock sections author it
    /// and every one of them names exactly `TIRE`; 32 of the 36 also reach the
    /// block, the other 4 (`CMON`, `FV`, `HORV`, `HTK`) being among the 17 that
    /// mis-spell `MaxDebris=` and so throw nothing.
    pub debris_types: Vec<String>,
    /// `DebrisMaximums=` CSV, positionally paired with `debris_types`: the cap
    /// on how many of each type may be thrown. 36 stock sections.
    pub debris_maximums: Vec<i32>,
    /// `DebrisAnims=` CSV of `[Animations]` ids — the SHP half of the debris a
    /// death throws, as distinct from the VXL half above. 166 stock sections,
    /// all of them `[BuildingTypes]`.
    pub debris_anims: Vec<String>,
    /// `CloseRange=` bool — `TechnoTypeClass+0x695`, read at `0x0071497A`
    /// (key string `0x008439C4`) and stored at `0x00714987`.
    ///
    /// `FootClass::Mission_Attack @ 0x004D4DC0` uses it as one half of the
    /// gate on its halved dispatch cadence: an INFANTRY type carrying it, or
    /// any type whose primary weapon reaches under 513 leptons, dispatches
    /// twice as often while closing on a target. Three stock entries —
    /// `BRUTE`, `DNOA`, `MUMY`.
    ///
    /// Not to be confused with `TechnoTypeClass+0x390` (`HoverAttack`) or
    /// `+0xD39` (`DefaultToGuardArea`); the research corpus has all three
    /// crossed.
    pub close_range: bool,
    /// `StupidHunt=` bool — `TechnoTypeClass+0x6D4`, read by
    /// `TechnoTypeClass::ReadINI` (key string `0x008438A4` = `"StupidHunt"`,
    /// `ReadBool @ 0x005295F0`, store `0x00714C80`); constructor default
    /// `false` (`0x007111A4` writes the zeroed `BL`).
    ///
    /// It is the very first test in `FootClass::Mission_Hunt @ 0x004D5350`
    /// (`0x004D535F`): a type carrying it **never scans for a target on Hunt**
    /// and drops straight through to the idle / return-to-base arm. The INI's
    /// own trailing comment says it plainly — "this guy can't handle a hunt
    /// command, so he should just run towards the player". Six stock sections:
    /// `SPY`, `LCRF`, `CMIN`, `SAPC`, `YHVR`, `SMIN`.
    pub stupid_hunt: bool,
    /// `Cyborg=` bool — gamemd's TechnoTypeClass `+0xC8F` "emits damage sparks"
    /// flag that `AI_Update` gates the per-tick damage-Spark prob-roll on.
    /// Verified: only `InfantryTypeClass::ReadINI` writes `+0xC8F` (from `Cyborg=`);
    /// every other leaf (vehicle/building/aircraft) keeps the ctor default 0. So
    /// the AI_Update spark draw fires only for `Cyborg=yes` INFANTRY — a TS legacy
    /// with **no stock-YR users** (the effect is dormant in stock). Default false.
    /// See [`ObjectType::emits_damage_spark`].
    pub cyborg: bool,
    /// `DestroyParticleSystems=` CSV list. Parsed for completeness;
    /// no live consumer in retail YR.
    pub destroy_particle_systems: Vec<String>,
    /// `DamageSmokeOffset=` X,Y,Z — anchor offset for damage smoke.
    pub damage_smoke_offset: IVec3,
    /// `DamSmkOffScrnRel=` — interpret `DamageSmokeOffset` as screen-relative
    /// rather than world-relative.
    pub dam_smk_off_scrn_rel: bool,
    /// `DestroySmokeOffset=` X,Y,Z — anchor offset for destruction smoke.
    pub destroy_smoke_offset: IVec3,
    /// Four `RefinerySmokeOffsetOne/Two/Three/Four=` X,Y,Z triplets.
    /// Used to position the four chimney-smoke emitters on a refinery.
    pub refinery_smoke_offsets: [IVec3; 4],
    /// `RefinerySmokeFrames=` — frame count for the smoke particle system.
    pub refinery_smoke_frames: i32,
    /// `GapRadiusInCells=` — per-object gap-generator radius (overrides the
    /// global default for this object).
    pub gap_radius_in_cells: u8,
    /// `SuperGapRadiusInCells=` — oversized gap radius applied during
    /// specific game states.
    pub super_gap_radius_in_cells: u8,
    /// `PsychicDetectionRadius=` — selected Psychic Sensor ring radius.
    pub psychic_detection_radius: u8,
    /// `SensorArray=` — building sensor-array flag used by GetSensorRange.
    pub sensor_array: bool,
    /// `Sensors=` — generic cloak-sensor flag.
    pub sensors: bool,
    /// `SensorsSight=` — fallback sensor/cloak-generator ring radius.
    pub sensors_sight: u8,
    /// `DetectDisguise=` — `TechnoTypeClass+0xD31`, written by
    /// `TechnoTypeClass::ReadINI @ 0x0071444C` (key string `0x00843C78`).
    /// Two distinct consumers, both verified:
    /// * as an ATTACKER type it bypasses the disguise rejection in
    ///   `TechnoClass::Evaluate_Candidate @ 0x006F84D4` — dogs, Yuri and the
    ///   Psi Corps Trooper auto-acquire Spies and Mirage Tanks;
    /// * as a BUILDING type it drives the per-cell detect counter through
    ///   `BuildingClass::OnConstructionComplete @ 0x004467AD` /
    ///   `BuildingClass::Limbo @ 0x00445A58`.
    pub detect_disguise: bool,
    /// `DetectDisguiseRange=` — `TechnoTypeClass+0x5F4`
    /// (`TechnoTypeClass::ReadINI @ 0x0071430F`, key string `0x00843D3C`),
    /// the circular radius `BuildingClass::AddDetectDisguiseAt @ 0x00455A80`
    /// stamps. `TechnoTypeClass::Constructor @ 0x0071100E` seeds it to zero,
    /// so a `DetectDisguise=yes` building without this key stamps no cells —
    /// stock YAPSYT and NAPSYB are exactly that case; only NAPSIS (15) deposits.
    pub detect_disguise_range: u8,
    /// `NoShadow=` — TechnoType+0xD98, constructor false at0x71160D.
    /// ReadINI0x715087..0x7150A2 reads exact key0x8436E0 through
    /// ReadBool0x5295F0 with the prior byte as its default on every rules pass.
    /// VXL shadow706BF3 and SHP shadow706052 consume the same type flag.
    /// Original byte receipts: tools/procedural_drawing_oracle/validation/source-dependencies/.
    pub no_shadow: bool,
    /// `Cloakable=` — copied into Unit/Infantry runtime cloak ability.
    pub cloakable: bool,
    /// `CloakingSpeed=` — signed frame duration between cloak progress steps.
    pub cloaking_speed: i32,
    /// `CloakStop=` — FootClass requires an idle locomotor before reporting
    /// its runtime cloak ability as currently usable.
    pub cloak_stop: bool,
    /// `CloakRadiusInCells=`. Building sensor-array removal uses this signed
    /// byte (constructor default 20), deliberately not `SensorsSight=`.
    pub cloak_radius_in_cells: i8,
    /// `CloakGenerator=` — cloak field provider flag used by GetSensorRange.
    pub cloak_generator: bool,
}

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
    /// `Weapon[index]` (`TechnoTypeClass+0x898 + index*0x1C`): the weapon in
    /// base slot `index`, or `None` for an empty slot or an index past the
    /// array. `TechnoTypeClass::ReadINI @ 0x007128B2` writes `Weapon1=` and
    /// `Primary=` to the same `+0x898`, so the `TurretCount` branch is decided
    /// once, at parse time (`read_weapon_arrays`); no reader has to
    /// know which INI keys a type authored.
    pub fn weapon_at(&self, index: usize) -> Option<&str> {
        self.weapon_list.get(index).and_then(|slot| slot.as_deref())
    }

    /// `EliteWeapon[index]` (`TechnoTypeClass+0xA94 + index*0x1C`).
    pub fn elite_weapon_at(&self, index: usize) -> Option<&str> {
        self.elite_weapon_list
            .get(index)
            .and_then(|slot| slot.as_deref())
    }

    /// Base weapon-array slot 0 (`TechnoTypeClass+0x898`), the field gamemd
    /// calls `Primary`. Filled from `Primary=` for an ordinary type and from
    /// `Weapon1=` for a `TurretCount>0` type — the same storage either way, see
    /// `ObjectType::read_weapon_arrays`. Read it as the native field, not as
    /// "the `Primary=` key": for `[SREF]` and `[YAGGUN]` it holds the
    /// `Weapon1=` weapon even though neither section authors a live `Primary=`.
    pub fn primary(&self) -> Option<&str> {
        self.weapon_at(WEAPON_SLOT_PRIMARY)
    }

    /// Base weapon-array slot 1 (`TechnoTypeClass+0x8B4`), gamemd's `Secondary`
    /// field. `Secondary=` or `Weapon2=`, same storage.
    pub fn secondary(&self) -> Option<&str> {
        self.weapon_at(WEAPON_SLOT_SECONDARY)
    }

    /// Elite weapon-array slot 0 (`TechnoTypeClass+0xA94`), gamemd's
    /// `ElitePrimary` field: `ElitePrimary=` or `EliteWeapon1=`. Replaces
    /// [`Self::primary`] when the unit is at Elite tier (veterancy >= 200) and
    /// this slot names a weapon; Veteran tier (100..199) does NOT swap.
    pub fn elite_primary(&self) -> Option<&str> {
        self.elite_weapon_at(WEAPON_SLOT_PRIMARY)
    }

    /// Elite weapon-array slot 1 (`TechnoTypeClass+0xAB0`), gamemd's
    /// `EliteSecondary` field: `EliteSecondary=` or `EliteWeapon2=`. Replaces
    /// [`Self::secondary`] at Elite tier under the same rule.
    pub fn elite_secondary(&self) -> Option<&str> {
        self.elite_weapon_at(WEAPON_SLOT_SECONDARY)
    }

    /// BuildingType+ED8 is initialized to zero at45DEE6 and has no active-YR
    /// reader/write override. Construction449AFE and Selling449DAA/449DE9
    /// use this raw byte, shifted8, to reset FacingClass's two direction words.
    /// DeployFacing lives separately at+EDC. Keep this invariant with the type
    /// metadata instead of adding a configurable or per-instance copy.
    pub const BUILDING_FACING: u8 = 0;

    /// Native barracks classification read by GetDockCell44EFB0 and
    /// ExitObject443C60; independent from Factory=InfantryType.
    pub fn gdi_barracks(&self) -> bool {
        self.gdi_barracks
    }

    pub fn armory(&self) -> bool {
        self.armory
    }

    /// Native BuildingType+16E5 classification, independent from Factory=.
    pub fn nod_barracks(&self) -> bool {
        self.nod_barracks
    }

    /// Native BuildingType+16E6 classification, independent from Factory=.
    pub fn yuri_barracks(&self) -> bool {
        self.yuri_barracks
    }

    /// Native TechnoType virtual+BC (717800), used by Fly takeoff4CF9F2
    /// and Aircraft Unlimbo414383. Zero and other negatives are literal.
    pub fn flight_level(&self, general_flight_level: i32) -> i32 {
        if self.flight_level == -1 {
            general_flight_level
        } else {
            self.flight_level
        }
    }

    /// The AI base planning side filter: `AIBasePlanningSide=` (`+0x6D0`) is
    /// -1 or the house's side (`HouseTypeClass+0xBC`), as
    /// `FirstBuildableFromArray @ 0x005051E0` and `AI_BuildWalls @ 0x0050C340`
    /// test it.
    pub fn planned_for_side(&self, side_index: u8) -> bool {
        self.ai_base_planning_side == -1 || self.ai_base_planning_side == i32::from(side_index)
    }

    /// Building43BCBD..43BCD0 allocates at least one radio contact even when
    /// the signed type count is nonpositive. This is not the coordinate-query count.
    pub fn dock_contact_capacity(&self) -> u32 {
        self.number_of_docks.max(1) as u32
    }

    /// BuildingType virtual used by the Building receive-damage prelude and
    /// the native base-reservation writer.
    ///
    /// gamemd-derived: `BuildingClass` vtable `+0x80` resolves through
    /// `0x00457620` to `BuildingTypeClass__Is1x1WithUndeploy @ 0x00465D40`.
    pub fn is_1x1_with_undeploy(&self) -> bool {
        self.undeploys_into.is_some()
            && crate::rules::foundation::foundation_dimensions(&self.foundation) == (1, 1)
    }

    /// The slot the per-class House and HouseType factor getters select by the
    /// type's RTTI: InfantryType 0, UnitType 1, AircraftType 2, BuildingType 3,
    /// or 4 for a `BuildCat=Combat` BuildingType (`+0xE08 == 5`)
    /// (`HouseClass::GetCostBonus @ 0x0050BDF0`,
    /// `HouseClass::GetAccumulatedBonus @ 0x0050BEB0`, the build-time factor
    /// `0x0050C0A0`).
    pub fn factor_slot(&self) -> usize {
        match self.category {
            ObjectCategory::Infantry => 0,
            ObjectCategory::Vehicle => 1,
            ObjectCategory::Aircraft => 2,
            ObjectCategory::Building if self.build_cat == Some(BuildCategory::Combat) => 4,
            ObjectCategory::Building => 3,
        }
    }

    /// BuildingType gate used by the native base-reservation setter.
    pub fn base_reservation_writer_eligible(&self) -> bool {
        self.category == ObjectCategory::Building
            && (self.undeploys_into.is_none() || !self.resource_gatherer)
            && !self.is_1x1_with_undeploy()
    }

    /// gamemd's TechnoTypeClass `+0xC8F` — whether `AI_Update` runs the per-tick
    /// damage-Spark prob-roll for this type. Only `InfantryTypeClass::ReadINI`
    /// sets `+0xC8F` (from `Cyborg=`); all other leaves keep the ctor default 0,
    /// so a non-infantry type never emits AI_Update sparks even if `Cyborg=yes` is
    /// (nonsensically) set on it. Dormant in stock YR (no `Cyborg=yes` units).
    pub fn emits_damage_spark(&self) -> bool {
        self.cyborg && self.category == ObjectCategory::Infantry
    }

    /// `BuildingClass` vt+0x284 (`0x00455DA0`, HasRallyPoint): a
    /// Factory=UnitType or InfantryType, UnitRepair or Cloning building. The
    /// rally click archives only on these, and their rally lines draw.
    pub fn has_rally_line(&self) -> bool {
        matches!(
            self.factory,
            Some(FactoryType::InfantryType | FactoryType::UnitType)
        ) || self.unit_repair
            || self.cloning
    }

    /// Fill the 18-slot base and elite weapon arrays the way native ReadINI
    /// does. **`Primary=` and `Weapon1=` are the same storage.**
    ///
    /// gamemd-derived: `TechnoTypeClass::ReadINI @ 0x007128B2`. A techno type
    /// carries one weapon array at `+0x898` (base) and one at `+0xA94` (elite),
    /// stride `0x1C`; `Primary=`/`Secondary=`/`ElitePrimary=`/`EliteSecondary=`
    /// are simply *names for slots 0 and 1 of those arrays*, not separate
    /// fields. Two mutually exclusive branches write them:
    ///
    /// - `0x007128B2 MOV ECX,[EBP+0x808]` / `TEST ECX,ECX` / `JLE 0x007129A3` —
    ///   `TurretCount > 0` runs the `Weapon%d` (key `0x844310`, read at
    ///   `0x007128E5`) / `EliteWeapon%d` (key `0x844300`, `0x007128F9`) loop for
    ///   indices `1..=WeaponCount` (`+0x80C`; `WeaponCount <= 0` skips
    ///   everything at `0x007128C8`). The cursor starts at
    ///   `0x007128D6 LEA EDI,[EBP+0xA94]`, stores the base result at
    ///   `0x0071294A MOV [EDI-0x1FC],EAX` and the elite result at
    ///   `0x00712981 MOV [EDI],EAX`, then `0x00712992 ADD EDI,0x1C`. On
    ///   iteration 1 the base store lands at `0xA94 - 0x1FC` = **`+0x898`** —
    ///   so `Weapon1=` writes the `Primary` field and `Weapon2=` the
    ///   `Secondary` field. The loop then jumps past the `Primary=` block
    ///   (`0x0071299E -> 0x00712A8F`).
    /// - `TurretCount <= 0` lands at `0x007129A3 TEST AL,AL`, where `AL` still
    ///   holds the `ClearAllWeapons=` (`+0xA90`) read from `0x007128A7`: set
    ///   skips the block entirely, clear reads `Primary=` (`0x8442F8` →
    ///   `+0x898` at `0x007129F2`), `Secondary=` (`0x8442EC` → `+0x8B4` at
    ///   `0x00712A17`), `ElitePrimary=` (`0x8442DC` → `+0xA94` at
    ///   `0x00712A64`) and `EliteSecondary=` (`0x8442CC` → `+0xAB0` at
    ///   `0x00712A89`), and never reads `Weapon%d`.
    ///
    /// Every native read passes the slot's current value as the default, so
    /// slots the taken branch does not write keep their constructor `NULL`.
    ///
    /// VERA-internal: `WeaponCount` is clamped to `WEAPON_SLOT_COUNT`. Native
    /// would write past the array; no stock section authors more than 17
    /// (`[FV]`), so the gamemd behaviour beyond the array is UNCHECKED and
    /// unreachable on retail data.
    fn read_weapon_arrays(section: &IniSection) -> (Vec<Option<String>>, Vec<Option<String>>) {
        let mut base: Vec<Option<String>> = vec![None; WEAPON_SLOT_COUNT];
        let mut elite: Vec<Option<String>> = vec![None; WEAPON_SLOT_COUNT];

        if section.read_int("TurretCount", 0) > 0 {
            let authored = usize::try_from(section.read_int("WeaponCount", 0)).unwrap_or(0);
            // `Weapon%d`/`EliteWeapon%d`: ReadString into 0x80 bytes (0x0071292B).
            for slot in 0..authored.min(WEAPON_SLOT_COUNT) {
                base[slot] = section
                    .read_name(&format!("Weapon{}", slot + 1), 0x80)
                    .map(str::to_owned);
                elite[slot] = section
                    .read_name(&format!("EliteWeapon{}", slot + 1), 0x80)
                    .map(str::to_owned);
            }
        } else if !section.read_bool("ClearAllWeapons", false) {
            base[WEAPON_SLOT_PRIMARY] = section.read_name("Primary", 0x80).map(str::to_owned);
            base[WEAPON_SLOT_SECONDARY] = section.read_name("Secondary", 0x80).map(str::to_owned);
            elite[WEAPON_SLOT_PRIMARY] = section.read_name("ElitePrimary", 0x80).map(str::to_owned);
            elite[WEAPON_SLOT_SECONDARY] =
                section.read_name("EliteSecondary", 0x80).map(str::to_owned);
        }

        (base, elite)
    }

    /// Parse an ObjectType from a rules.ini section.
    ///
    /// Missing keys get sensible defaults matching RA2's behavior.
    /// The `id` is the section name, and `category` comes from which
    /// type registry listed this object.
    pub fn from_ini_section(id: &str, section: &IniSection, category: ObjectCategory) -> Self {
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
        // `IsGattling=` gates the Stage loop read beside it (`0x0071407E`).
        let is_gattling = section.read_bool("IsGattling", false);
        // The targeting coefficients (`0x0071556B..0x0071570C`) read with a
        // `[General]` default resolved where the value is used; `None` marks
        // an absent key.
        let present_double = |key: &str| {
            section
                .is_present(key)
                .then(|| section.read_double(key, 0.0))
        };
        // These remaining legacy voice keys are sound lists (ReadSoundList
        // 525430), but currently retain only the first sound. VoiceSelect,
        // VoiceSpecialAttack and MoveSound use the complete list below.
        let first_sound = |key: &str| {
            section
                .read_sound_list(key)
                .and_then(|sounds| sounds.first().map(|sound| sound.to_string()))
        };
        // `GuardRange=` and `AirRangeBonus=` are ReadRange whole leptons
        // (`0x007122B3`, `0x007147A9`), kept here as cells; `None` marks an
        // absent key.
        let present_range = |key: &str| {
            section
                .is_present(key)
                .then(|| SimFixed::from_bits(section.read_range(key, 0) << 8))
        };
        // The house lists (`0x004750D0`) read `char[128]`. The two
        // prerequisite lists name BuildingTypes, so `RuleSet` parses them.
        let owner: Vec<String> = section
            .read_list("Owner", 0x80)
            .map(|tokens| tokens.into_iter().map(str::to_owned).collect())
            .unwrap_or_default();

        let required_houses: Vec<String> = section
            .read_list("RequiredHouses", 0x80)
            .map(|tokens| tokens.into_iter().map(str::to_owned).collect())
            .unwrap_or_default();

        let forbidden_houses: Vec<String> = section
            .read_list("ForbiddenHouses", 0x80)
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

        // `Primary`/`Secondary` are slots 0/1 of these arrays in gamemd — the
        // same storage, filled by whichever ReadINI branch this type takes.
        let (weapon_list, elite_weapon_list) = Self::read_weapon_arrays(section);

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
        let max_debris = section.read_int("MaxDebris", 0);
        let min_debris = section.read_int("MinDebris", 0).max(0);
        let max_debris = max_debris.max(min_debris);

        Self {
            id: id.to_string(),
            category,
            name: section.read_name("Name", 0x31).map(str::to_owned),
            ui_name: section.read_name("UIName", 0x20).map(str::to_owned),
            cost: section.read_int("Cost", 0),
            soylent: section.read_int("Soylent", 0),
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
            explosion_anims: section
                .read_list("Explosion", 0x80)
                .map(|tokens| tokens.into_iter().map(str::to_owned).collect())
                .unwrap_or_default(),
            destroy_anims: section
                .read_list("DestroyAnim", 0x80)
                .map(|tokens| tokens.into_iter().map(str::to_owned).collect())
                .unwrap_or_default(),
            trainable: section.read_bool("Trainable", category != ObjectCategory::Building),
            burst_delays: std::array::from_fn(|index| {
                if category == ObjectCategory::Vehicle {
                    section.read_int(&format!("BurstDelay{index}"), -1)
                } else {
                    -1
                }
            }),
            dead_bodies: if category == ObjectCategory::Infantry {
                section
                    .read_list("DeadBodies", 0x80)
                    .map(|tokens| tokens.into_iter().map(str::to_owned).collect())
                    .unwrap_or_default()
            } else {
                Vec::new()
            },
            not_human: category == ObjectCategory::Infantry && section.read_bool("NotHuman", false),
            strength: section.read_int("Strength", 0),
            dont_score: section.read_bool("DontScore", false),
            special_threat_value: section.read_double("SpecialThreatValue", 0.0),
            threat_posed: section.read_int("ThreatPosed", 0),
            leadership_rating: section.read_int("LeadershipRating", 5),
            my_effectiveness_coefficient: present_double("MyEffectivenessCoefficient"),
            target_effectiveness_coefficient: present_double("TargetEffectivenessCoefficient"),
            target_special_threat_coefficient: present_double("TargetSpecialThreatCoefficient"),
            target_strength_coefficient: present_double("TargetStrengthCoefficient"),
            target_distance_coefficient: present_double("TargetDistanceCoefficient"),
            threat_avoidance_coefficient: section.read_double_bits(
                "ThreatAvoidanceCoefficient",
                crate::util::native_x87::NativeF64Bits::POSITIVE_ZERO,
            ),
            armor: section.read_string("Armor", "none", 0x80),
            speed: section.read_techno_speed("Speed", 0),
            // TechnoTypeClass ctor/read contract: raw signed ints, with no
            // clamp or conversion. A zero WalkRate is invalid content natively
            // (the live consumer executes IDIV without a zero guard).
            walk_rate: section.read_int("WalkRate", 1),
            idle_rate: section.read_int("IdleRate", 0),
            // Double fields (`0x007124A3..0x007124E5`).
            weight: sim_from_f32(section.read_double("Weight", 2.0) as f32),
            accel_factor: sim_from_f32(section.read_double("AccelerationFactor", 0.03) as f32),
            decel_factor: sim_from_f32(section.read_double("DeaccelerationFactor", 0.002) as f32),
            accelerates: section.read_bool("Accelerates", true),
            passive: category == ObjectCategory::Vehicle && section.read_bool("Passive", false),
            is_train: section.read_bool("IsTrain", false),
            slowdown_distance: section.read_int("SlowdownDistance", 500),
            flight_level: section.read_int("FlightLevel", -1),
            is_dropship: section.read_bool("IsDropship", false),
            // `ReadDouble(-1)` (`0x00712364..0x00712399`): -1 keeps the
            // constructor's 20 degrees.
            pitch_angle: sim_from_f32(
                match section.read_double("PitchAngle", -1.0) {
                    -1.0 => 20.0,
                    degrees => degrees as f32,
                } * (std::f32::consts::PI / 180.0),
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
            // TechnoTypeClass ctor @ gamemd.exe 0x00711082 initializes
            // +0x634 to 255; ReadINI preserves that current value when the
            // key is absent. Explicit TechLevel=-1 remains a distinct
            // civilian/unbuildable sentinel.
            tech_level: section.read_int("TechLevel", 255),
            build_time_multiplier: section.read_double_to_float(
                "BuildTimeMultiplier",
                crate::util::native_x87::NativeF32Bits::ONE,
            ),
            owner,
            // ReadBool over the constructor's 0 at +0xC99 (`0x00712299`).
            double_owned: section.read_bool("DoubleOwned", false),
            required_houses,
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
            requires_stolen_allied_tech: section.read_bool("RequiresStolenAlliedTech", false),
            requires_stolen_soviet_tech: section.read_bool("RequiresStolenSovietTech", false),
            requires_stolen_third_tech: section.read_bool("RequiresStolenThirdTech", false),
            image: section.read_string("Image", id, 0x19),
            power: section.read_int("Power", 0),
            extra_power: section.read_int("ExtraPower", 0),
            // Building ctor45DF13 initializes +EF0=0 (1x1). ReadINI461225
            // reads Foundation from ART only, never this RULES section.
            // Pass processing projects the shared reader; ART binding does not.
            foundation: crate::rules::foundation::FOUNDATION_TABLE
                [usize::from(crate::rules::foundation::DEFAULT_FOUNDATION_ID)]
            .name
            .to_owned(),
            pixel_selection_bracket_delta: section.read_int("PixelSelectionBracketDelta", 0),
            build_cat: section
                .read_name("BuildCat", 0x20)
                .and_then(BuildCategory::from_ini),
            adjacent: section.read_int("Adjacent", 3),
            protect_with_wall: section.read_bool("ProtectWithWall", false),
            wants_extra_space: section.read_bool("WantsExtraSpace", false),
            base_normal: section.read_bool("BaseNormal", true),
            eligibile_for_ally_building: section.read_bool("EligibileForAllyBuilding", false),
            crewed: section.read_bool("Crewed", false),
            voice_select: section
                .read_sound_list("VoiceSelect")
                .map(|tokens| tokens.into_iter().map(str::to_owned).collect())
                .unwrap_or_default(),
            voice_move: first_sound("VoiceMove"),
            voice_attack: first_sound("VoiceAttack"),
            voice_harvest: section.read_name("VoiceHarvest", 0x80).map(str::to_owned),
            voice_enter: section.read_name("VoiceEnter", 0x80).map(str::to_owned),
            voice_capture: section.read_name("VoiceCapture", 0x80).map(str::to_owned),
            prevent_attack_move: section.read_bool("PreventAttackMove", false),
            voice_die: section
                .read_sound_list("VoiceDie")
                .map(|tokens| tokens.into_iter().map(str::to_owned).collect())
                .unwrap_or_default(),
            die_sounds: section
                .read_sound_list("DieSound")
                .map(|tokens| tokens.into_iter().map(str::to_owned).collect())
                .unwrap_or_default(),
            damage_sound: section.read_name("DamageSound", 0x80).map(str::to_owned),
            move_sound: section
                .read_sound_list("MoveSound")
                .map(|tokens| tokens.into_iter().map(str::to_owned).collect())
                .unwrap_or_default(),
            crashing_sound: section.read_name("CrashingSound", 0x80).map(str::to_owned),
            voice_crashing: section.read_name("VoiceCrashing", 0x80).map(str::to_owned),
            // Native constructors store -1; the process owner later binds
            // these references against its fixed SOUNDMD catalog.
            sinking_sound: None,
            voice_sinking: None,
            impact_water_sound: section
                .read_name("ImpactWaterSound", 0x80)
                .map(str::to_owned),
            impact_land_sound: section
                .read_name("ImpactLandSound", 0x80)
                .map(str::to_owned),
            voice_feedback: section
                .read_sound_list("VoiceFeedback")
                .unwrap_or_default()
                .into_iter()
                .map(str::to_owned)
                .collect(),
            voice_special_attack: section
                .read_sound_list("VoiceSpecialAttack")
                .unwrap_or_default()
                .into_iter()
                .map(str::to_owned)
                .collect(),
            crush_sound: section.read_name("CrushSound", 0x80).map(str::to_owned),
            deploy_sound: section.read_name("DeploySound", 0x80).map(str::to_owned),
            buildup_sound: None,
            undeploy_sound: section.read_name("UndeploySound", 0x80).map(str::to_owned),
            enter_water_sound: None,
            leave_water_sound: None,
            packup_sound: section.read_name("PackupSound", 0x80).map(str::to_owned),
            leave_transport_sound: section
                .read_name("LeaveTransportSound", 0x80)
                .map(str::to_owned),
            chrono_in_sound: section.read_name("ChronoInSound", 0x80).map(str::to_owned),
            chrono_out_sound: section.read_name("ChronoOutSound", 0x80).map(str::to_owned),
            // Native relative order: ManualReload713352, then
            // TiltCrashJumpjet713386, then Turret. The shared bool reader
            // retains earlier-layer values when a later token is malformed.
            manual_reload: section.read_bool("ManualReload", false),
            tilt_crash_jumpjet: section.read_bool("TiltCrashJumpjet", false),
            has_turret: section.read_bool("Turret", false),
            // gamemd writes a separate UnitType +0x398=10 for Harvester/Weeder,
            // but ROT= remains the parsed TechnoType +0x71C facing-rate field.
            turret_rot: section.read_int("ROT", 0),
            recoil: crate::rules::recoil::RecoilConfig::from_ini_section(section),
            turret_anim: section.read_name("TurretAnim", 0x10).map(str::to_uppercase),
            turret_anim_is_voxel: section.read_bool("TurretAnimIsVoxel", false),
            turret_anim_x: section.read_int("TurretAnimX", 0),
            turret_anim_y: section.read_int("TurretAnimY", 0),
            turret_anim_z_adjust: section.read_int("TurretAnimZAdjust", 0),
            guard_range: present_range("GuardRange"),
            air_range_bonus: present_range("AirRangeBonus"),
            // Default no (passive acquisition off unless the type opts in).
            opportunity_fire: section.read_bool("OpportunityFire", false),
            // Default yes (retaliation allowed unless the type opts out).
            can_retaliate: section.read_bool("CanRetaliate", true),
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
            self_healing: section.read_bool("SelfHealing", false),
            death_weapon: section.read_name("DeathWeapon", 0x80).map(str::to_owned),
            // A float field (`FSTP dword` at `0x0071232B`).
            death_weapon_damage_modifier: section.read_float("DeathWeaponDamageModifier", 1.0),
            super_weapon: section.read_name("SuperWeapon", 0x20).map(str::to_owned),
            super_weapon2: section.read_name("SuperWeapon2", 0x20).map(str::to_owned),
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
            free_unit: section.read_name("FreeUnit", 0x80).map(str::to_owned),
            dock: section
                .read_list("Dock", 0x80)
                .map(|tokens| tokens.into_iter().map(str::to_ascii_uppercase).collect())
                .unwrap_or_default(),
            queueing_cell: [0, 0], // merged from art.ini later
            pads: Vec::new(),      // merged from art.ini later
            hidden_occupancy: BuildingHiddenOccupancyProfile::default(),
            base_reservation_spacing: None,
            unloading_class: section.read_name("UnloadingClass", 0x80).map(str::to_owned),
            deploying_anim: section
                .read_type_name("DeployingAnim", 0x80)
                .map(str::to_owned),
            ammo: section.read_int("Ammo", -1),
            initial_ammo: section.read_int("InitialAmmo", -1),

            // Spawn manager pool
            // `0x00714E97..0x00714EB3` reads the name through
            // `AircraftTypeClass::FindOrAllocate @ 0x0041CEF0`, which answers
            // null for `none` and `<none>`.
            spawns: section
                .read_type_name("Spawns", 0x80)
                .map(str::to_ascii_uppercase),
            spawns_number: section.read_int("SpawnsNumber", 0),
            spawn_regen_rate: section.read_int("SpawnRegenRate", 0).max(0) as u32,
            spawn_reload_rate: section.read_int("SpawnReloadRate", 0).max(0) as u32,
            missile_spawn: section.read_bool("MissileSpawn", false),
            no_spawn_alt: section.read_bool("NoSpawnAlt", false),

            // Slave Miner / economy fields
            enslaves: section.read_name("Enslaves", 0x80).map(str::to_owned),
            slaves_number: section.read_int("SlavesNumber", 0),
            slave_regen_rate: section.read_int("SlaveRegenRate", 0),
            slave_reload_rate: section.read_int("SlaveReloadRate", 0),
            slaved: section.read_bool("Slaved", false),
            fearless: section.read_bool("Fearless", false),
            fraidycat: section.read_bool("Fraidycat", false),
            crawls: false,
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
            // Other category defaults remain with this existing owner.
            speed_type: section.read_speed_type(
                "SpeedType",
                if matches!(
                    category,
                    ObjectCategory::Building | ObjectCategory::Infantry
                ) {
                    SpeedType::Foot
                } else {
                    SpeedType::default()
                },
            ),
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
            teleporter: section.read_bool("Teleporter", false),
            move_to_shroud: section.read_bool("MoveToShroud", category != ObjectCategory::Aircraft),
            hover_attack: section.read_bool("HoverAttack", false),
            balloon_hover: section.read_bool("BalloonHover", false),
            is_simple_deployer: section.read_bool("IsSimpleDeployer", false),
            small_visceroid: category == ObjectCategory::Vehicle
                && section.read_bool("SmallVisceroid", false),
            large_visceroid: category == ObjectCategory::Vehicle
                && section.read_bool("LargeVisceroid", false),
            deploy_to_land: section.read_bool("DeployToLand", false),
            airport_bound: section.read_bool("AirportBound", false),
            fighter: section.read_bool("Fighter", false),
            fly_by: section.read_bool("FlyBy", false),
            fly_back: section.read_bool("FlyBack", false),
            landable: section.read_bool("Landable", false),
            carryall: section.read_bool("Carryall", false),
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
            omni_crusher: section.read_bool("OmniCrusher", false),
            omni_crush_resistant: section.read_bool("OmniCrushResistant", false),
            immune_to_radiation: section.read_bool("ImmuneToRadiation", false),
            damage_self: section.read_bool("DamageSelf", false),
            immune: section.read_bool("Immune", false),
            type_immune: section.read_bool("TypeImmune", false),
            immune_to_psionics: section
                .read_bool("ImmuneToPsionics", category == ObjectCategory::Building),
            warpable: section.read_bool("Warpable", true),
            bombable: section.read_bool("Bombable", true),
            bomb_sight: section.read_int("BombSight", 0),
            mind_control_ring_offset: section.read_int("MindControlRingOffset", 0x8C),
            // "none" finds no sound (-1), which FreeUnit reads as unset.
            mind_cleared_sound: section
                .read_name("MindClearedSound", 0x80)
                .map(str::to_owned)
                .filter(|sound| !sound.is_empty() && !sound.eq_ignore_ascii_case("none")),
            immune_to_psionic_weapons: section.read_bool(
                "ImmuneToPsionicWeapons",
                category == ObjectCategory::Building,
            ),
            immune_to_poison: section.read_bool("ImmuneToPoison", false),

            deploys_into: section.read_name("DeploysInto", 0x80).map(str::to_owned),
            undeploys_into: section
                .read_type_name("UndeploysInto", 0x80)
                .map(str::to_owned),
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
            factory: section
                .read_name("Factory", 0x20)
                .and_then(FactoryType::from_ini),
            weapons_factory: section.read_bool("WeaponsFactory", false),
            armory,
            gdi_barracks,
            nod_barracks,
            yuri_barracks,
            cloning: section.read_bool("Cloning", false),
            // Read3Int (`0x00460FCD`).
            exit_coord: section
                .read_coord3_value("ExitCoord")
                .map(|[x, y, z]| (x, y, z)),

            // Cursor / interaction capability flags
            engineer: section.read_bool("Engineer", false),
            ivan: category == ObjectCategory::Infantry && section.read_bool("Ivan", false),
            infiltrate: category == ObjectCategory::Infantry
                && section.read_bool("Infiltrate", false),
            deployer: section.read_bool("Deployer", false),
            capturable: section.read_bool("Capturable", false),
            needs_engineer: section.read_bool("NeedsEngineer", false),
            capture_eva_event: section
                .read_name("CaptureEvaEvent", 0x64)
                .map(str::to_owned),
            // Repairable defaults to true — most buildings can be repaired in RA2.
            repairable: section.read_bool("Repairable", true),
            can_be_occupied: section.read_bool("CanBeOccupied", false),
            can_occupy_fire: section.read_bool("CanOccupyFire", false),
            show_occupant_pips: section.read_bool("ShowOccupantPips", true),
            bridge_repair_hut: section.read_bool("BridgeRepairHut", false),
            laser_fence: section.read_bool("LaserFence", false),
            laser_fence_post: section.read_bool("LaserFencePost", false),
            firestorm_wall: section.read_bool("FirestormWall", false),
            passengers: section.read_int("Passengers", 0),
            size_limit: section.read_int("SizeLimit", 0).max(0) as u32,
            size: section
                .read_int(
                    "Size",
                    if category == ObjectCategory::Infantry {
                        1
                    } else {
                        3
                    },
                )
                .max(0) as u32,
            open_topped: section.read_bool("OpenTopped", false),
            gunner: section.read_bool("Gunner", false),
            ifv_mode: section.read_int("IFVMode", 0),
            gunner_turrets: crate::rules::gunner_turrets::GunnerTurrets::from_ini_section(
                id, section, category,
            ),
            open_transport_weapon: section.read_int("OpenTransportWeapon", -1),
            deploy_fire: section.read_bool("DeployFire", false),
            // `TechnoTypeClass::Constructor @ 0x00711187` seeds -1, and
            // `ReadINI @ 0x00714BBA` only overwrites it when the key is present.
            undeploy_delay: section.read_int("UndeployDelay", -1),
            fire_angle: section.read_int("FireAngle", 8),
            deploy_fire_weapon: section.read_int("DeployFireWeapon", 1),
            max_number_occupants: section.read_int("MaxNumberOccupants", 0).max(0) as u32,
            occupier: section.read_bool("Occupier", false),
            assaulter: section.read_bool("Assaulter", false),
            vehicle_thief: section.read_bool("VehicleThief", false),
            occupy_weapon: section.read_name("OccupyWeapon", 0x80).map(str::to_owned),
            elite_occupy_weapon: section
                .read_name("EliteOccupyWeapon", 0x80)
                .map(str::to_owned),
            occupy_pip: section
                .read_name("OccupyPip", 0x20)
                .map(|s| match s.to_ascii_lowercase().as_str() {
                    "persongreen" => 7,
                    "personyellow" => 8,
                    "personwhite" => 9,
                    "personred" => 10,
                    "personblue" => 11,
                    "personpurple" => 12,
                    _ => 7,
                })
                .unwrap_or(7),
            pip_scale: section
                .read_name("PipScale", 0x20)
                .map(PipScale::from_ini)
                .unwrap_or_default(),
            infantry_absorb: section.read_bool("InfantryAbsorb", false),
            unit_absorb: section.read_bool("UnitAbsorb", false),
            grinding: section.read_bool("Grinding", false),
            hospital,
            bunkerable: section.read_bool("Bunkerable", category == ObjectCategory::Vehicle),
            weapon_list,
            elite_weapon_list,
            weapon_count: section.read_int("WeaponCount", 0),
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
            is_gattling,
            gattling_stages: crate::rules::gattling_type::GattlingStages::read(
                section,
                is_gattling,
            ),
            turret_count: section.read_int("TurretCount", 0),
            drainable: section.read_bool("Drainable", false),
            // Constructor defaults UNCHECKED; stock authors write all three
            // on CAOILD only (rulesmd.ini:13949-13951) and comment them out
            // elsewhere, so the absent-key value 0 matches the dormant case.
            produce_cash_startup: section.read_int("ProduceCashStartup", 0),
            produce_cash_amount: section.read_int("ProduceCashAmount", 0),
            produce_cash_delay: section.read_int("ProduceCashDelay", 0),
            overpowerable: section.read_bool("Overpowerable", false),
            attack_cursor_on_friendlies: section.read_bool("AttackCursorOnFriendlies", false),
            sabotage_cursor: section.read_bool("SabotageCursor", false),
            c4: section.read_bool("C4", false),
            can_c4: section.read_bool("CanC4", category == ObjectCategory::Building),
            eligible_for_delay_kill: section.read_bool("EligibleForDelayKill", false),
            invisible: section.read_bool("Invisible", false),
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

            // Point light source fields
            light_visibility: section.read_int("LightVisibility", 5000),
            // ReadDouble (`0x00460CD3..0x00460DAE`). Native stores
            // `ftol(value * 1000 + 0.1)` milliunits; VERA keeps the read value.
            light_intensity: section.read_double("LightIntensity", 0.0) as f32,
            light_red_tint: section.read_double("LightRedTint", 1.0) as f32,
            light_green_tint: section.read_double("LightGreenTint", 1.0) as f32,
            light_blue_tint: section.read_double("LightBlueTint", 1.0) as f32,
            has_spotlight: section.read_bool("HasSpotlight", false),

            // TechnoType particle effects
            natural_particle_system: section
                .read_name("NaturalParticleSystem", 0x80)
                .map(str::to_owned),
            natural_particle_location: IVec3::from_array(
                section.read_coord_tokens("NaturalParticleLocation", [0; 3]),
            ),
            refinery_smoke_particle_system: section
                .read_name("RefinerySmokeParticleSystem", 0x80)
                .map(str::to_owned),
            damage_particle_systems: section
                .read_list("DamageParticleSystems", 0x80)
                .map(|tokens| tokens.into_iter().map(str::to_owned).collect())
                .unwrap_or_default(),
            max_debris,
            min_debris,
            debris_types: section
                .read_list("DebrisTypes", 0x80)
                .map(|tokens| tokens.into_iter().map(str::to_owned).collect())
                .unwrap_or_default(),
            debris_maximums: section.read_int_list("DebrisMaximums").unwrap_or_default(),
            debris_anims: section
                .read_list("DebrisAnims", 0x80)
                .map(|tokens| tokens.into_iter().map(str::to_owned).collect())
                .unwrap_or_default(),
            close_range: section.read_bool("CloseRange", false),
            stupid_hunt: section.read_bool("StupidHunt", false),
            cyborg: section.read_bool("Cyborg", false),
            destroy_particle_systems: section
                .read_list("DestroyParticleSystems", 0x80)
                .map(|tokens| tokens.into_iter().map(str::to_owned).collect())
                .unwrap_or_default(),
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
            gap_radius_in_cells: section
                .read_int("GapRadiusInCells", 0)
                .clamp(0, u8::MAX as i32) as u8,
            super_gap_radius_in_cells: section
                .read_int("SuperGapRadiusInCells", 0)
                .clamp(0, u8::MAX as i32) as u8,
            psychic_detection_radius: section
                .read_int("PsychicDetectionRadius", 0)
                .clamp(0, u8::MAX as i32) as u8,
            sensor_array: section.read_bool("SensorArray", false),
            sensors: section.read_bool("Sensors", false),
            sensors_sight: section.read_int("SensorsSight", 0).clamp(0, u8::MAX as i32) as u8,
            detect_disguise_range: section
                .read_int("DetectDisguiseRange", 0)
                .clamp(0, u8::MAX as i32) as u8,
            no_shadow: section.read_bool("NoShadow", false),
            cloakable: section.read_bool("Cloakable", false),
            cloaking_speed: section.read_int("CloakingSpeed", 1),
            cloak_stop: section.read_bool("CloakStop", false),
            cloak_radius_in_cells: section.read_int("CloakRadiusInCells", 20) as i8,
            cloak_generator: section.read_bool("CloakGenerator", false),
        }
    }
}

/// One entry of the native ability table.
///
/// gamemd-derived: `AbilityClass::FindAbilityByName @ 0x0074FEFF` walks the
/// 18-pointer table at `0x008463B8..0x008463FC` in this order and returns the
/// index, which is the byte offset inside `VeteranAbilities` (`+0x29C`) and
/// `EliteAbilities` (`+0x2AE`) on `TechnoTypeClass`. The discriminants ARE
/// the native indices; the consumers read `+0x29C + idx` / `+0x2AE + idx`
/// (`TechnoClass::HasWeaponAbility @ 0x0070D0D0`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum Ability {
    Faster = 0,
    Stronger = 1,
    Firepower = 2,
    Scatter = 3,
    Rof = 4,
    Sight = 5,
    Cloak = 6,
    TiberiumProof = 7,
    VeinProof = 8,
    SelfHeal = 9,
    Explodes = 10,
    RadarInvisible = 11,
    Sensors = 12,
    Fearless = 13,
    C4 = 14,
    TiberiumHeal = 15,
    GuardArea = 16,
    Crusher = 17,
}

impl Ability {
    /// The native name table, indexed by discriminant. Strings verified by
    /// `search_strings` against the pointers the table holds.
    pub const NAMES: [&'static str; 18] = [
        "FASTER",
        "STRONGER",
        "FIREPOWER",
        "SCATTER",
        "ROF",
        "SIGHT",
        "CLOAK",
        "TIBERIUM_PROOF",
        "VEIN_PROOF",
        "SELF_HEAL",
        "EXPLODES",
        "RADAR_INVISIBLE",
        "SENSORS",
        "FEARLESS",
        "C4",
        "TIBERIUM_HEAL",
        "GUARD_AREA",
        "CRUSHER",
    ];

    /// `AbilityClass::FindAbilityByName @ 0x0074FEFF`: a case-insensitive
    /// (ASCII-only fold, `FUN_007C8D20`) walk of the table; `None` is the
    /// native `-1`, which the list parser silently skips.
    pub fn from_ini_token(token: &str) -> Option<Self> {
        let index = Self::NAMES
            .iter()
            .position(|name| name.eq_ignore_ascii_case(token))?;
        Some(Self::from_index(index))
    }

    fn from_index(index: usize) -> Self {
        const ALL: [Ability; 18] = [
            Ability::Faster,
            Ability::Stronger,
            Ability::Firepower,
            Ability::Scatter,
            Ability::Rof,
            Ability::Sight,
            Ability::Cloak,
            Ability::TiberiumProof,
            Ability::VeinProof,
            Ability::SelfHeal,
            Ability::Explodes,
            Ability::RadarInvisible,
            Ability::Sensors,
            Ability::Fearless,
            Ability::C4,
            Ability::TiberiumHeal,
            Ability::GuardArea,
            Ability::Crusher,
        ];
        ALL[index]
    }
}

/// One 18-byte native ability array.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct AbilityFlags([bool; 18]);

impl AbilityFlags {
    /// The list parser behind both keys (`FUN_00477640`, called from
    /// `TechnoTypeClass::ReadINI` at `0x007154BB` / `0x007154FA`).
    ///
    /// gamemd-derived: a PRESENT key starts from an all-zero array and sets
    /// one byte per recognised `strtok` token, so a later INI pass that
    /// restates the key replaces the whole array rather than OR-ing into it;
    /// unknown tokens are skipped. An ABSENT key copies the array already on
    /// the type (`param_4`), which the caller passes as the default — so the
    /// value carried from the previous pass survives. `None` here is that
    /// absent-key case; the caller keeps its prior flags.
    pub fn parse_present(list: &[&str]) -> Self {
        let mut flags = [false; 18];
        for token in list {
            if let Some(ability) = Ability::from_ini_token(token) {
                flags[ability as usize] = true;
            }
        }
        Self(flags)
    }

    /// Parse one key's list, keeping `prior` when the key is absent.
    pub fn parse(list: Option<Vec<&str>>, prior: Self) -> Self {
        match list {
            Some(list) => Self::parse_present(&list),
            None => prior,
        }
    }

    pub fn has(self, ability: Ability) -> bool {
        self.0[ability as usize]
    }

    #[cfg(test)]
    pub(crate) fn from_abilities(abilities: &[Ability]) -> Self {
        let mut flags = [false; 18];
        for ability in abilities {
            flags[*ability as usize] = true;
        }
        Self(flags)
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

#[cfg(test)]
#[path = "infantry_speed_type_tests.rs"]
mod infantry_speed_type_tests;

#[cfg(test)]
mod tests {
    #[test]
    fn flight_level_reader_and_fallback_match_original_executable() {
        #[derive(serde::Deserialize)]
        struct Row {
            raw: Option<String>,
            general: i32,
            stored: i32,
            effective: i32,
        }
        let rows: Vec<Row> = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/flight_level.json",
        ))
        .unwrap();
        assert_eq!(rows.len(), 30);
        for row in rows {
            let mut ini = IniFile::from_str("[PLANE]\nStrength=100\n");
            if let Some(raw) = &row.raw {
                ini.projection_section_mut("PLANE").set("FlightLevel", raw);
            }
            let obj = ObjectType::from_ini_section(
                "PLANE",
                ini.section("PLANE").unwrap(),
                ObjectCategory::Aircraft,
            );
            assert_eq!(obj.flight_level, row.stored, "{:?}", row.raw);
            assert_eq!(
                obj.flight_level(row.general),
                row.effective,
                "{:?}",
                row.raw
            );
        }
    }

    #[test]
    fn signed_dock_count_is_distinct_from_contact_capacity() {
        #[derive(serde::Deserialize)]
        struct Row {
            initial: i32,
            raw: Option<String>,
            count: i32,
            capacity: u32,
        }
        // Executed original ReadInteger call site and separate contact clamp.
        // Non-constructor defaults test the shared reader, not Rules pass lifetime.
        let rows: Vec<Row> = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/building_dock_count_rules.json",
        ))
        .unwrap();
        for row in rows {
            // The native corpus supplies cached INI entries directly. Physical
            // INI loading drops empty values; preserve the reader boundary here.
            let mut ini = IniFile::from_str("[PAD]\nStrength=100\n");
            if let Some(raw) = &row.raw {
                ini.projection_section_mut("PAD").set("NumberOfDocks", raw);
            }
            let section = ini.section("PAD").unwrap();
            assert_eq!(
                section.read_int("NumberOfDocks", row.initial),
                row.count,
                "initial={} raw={:?}",
                row.initial,
                row.raw
            );
            let mut object = ObjectType::from_ini_section("PAD", section, ObjectCategory::Building);
            if row.initial == 1 {
                assert_eq!(object.number_of_docks, row.count, "{:?}", row.raw);
            } else {
                object.number_of_docks = row.count;
            }
            assert_eq!(
                object.dock_contact_capacity(),
                row.capacity,
                "{:?}",
                row.raw
            );
        }
    }

    #[test]
    fn vhp_scan_parser_preserves_native_names_token_and_default() {
        // Unknown first tokens keep the constructor's None.
        for (raw, expected) in [
            ("Strong", VhpScan::Strong),
            ("nOrMaL", VhpScan::Normal),
            ("Strong,Normal", VhpScan::Strong),
            (",,Normal", VhpScan::Normal),
            (" Normal ", VhpScan::Normal),
            ("Normal ,Strong", VhpScan::None),
            ("Weak", VhpScan::None),
            ("2", VhpScan::None),
        ] {
            let mut section = IniSection::new("TYPE".to_string());
            section.set("VHPScan", raw);
            assert_eq!(VhpScan::read_ini(&section), expected, "{raw}");
        }
        let ini = IniFile::from_str("[TYPE]\nStrength=100\n");
        assert_eq!(
            ObjectType::from_ini_section(
                "TYPE",
                ini.section("TYPE").unwrap(),
                ObjectCategory::Vehicle
            )
            .vhp_scan,
            VhpScan::None,
        );
    }

    #[test]
    fn vhp_scan_layers_retain_prior_mode_and_ignore_preallocation_body() {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        use crate::rules::ruleset::RuleSet;
        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[VehicleTypes]\n0=EXISTING\n[EXISTING]\nVHPScan=Strong\n[LATE]\nVHPScan=Strong\n",
        ));
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str(
                "[VehicleTypes]\n0=LATE\n[EXISTING]\nVHPScan=Invalid\n[LATE]\nVHPScan=Invalid\n",
            ),
        );
        let rules = RuleSet::from_rules_layers(&layers).unwrap();
        assert_eq!(rules.object("EXISTING").unwrap().vhp_scan, VhpScan::Strong);
        assert_eq!(rules.object("LATE").unwrap().vhp_scan, VhpScan::None);
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[EXISTING]\nVHPScan=None\n[LATE]\nVHPScan=Normal,Strong\n"),
        );
        let rules = RuleSet::from_rules_layers(&layers).unwrap();
        assert_eq!(rules.object("EXISTING").unwrap().vhp_scan, VhpScan::None);
        assert_eq!(rules.object("LATE").unwrap().vhp_scan, VhpScan::Normal);
    }

    #[test]
    fn vhp_scan_retail_nasam_is_strong() {
        // Checked retail rulesmd.ini NASAM section, VHPScan=Strong at12132;
        // keep the fixture independent of machine-local retail installations.
        let ini = IniFile::from_str("[NASAM]\nVHPScan=Strong\n");
        let obj = ObjectType::from_ini_section(
            "NASAM",
            ini.section("NASAM").unwrap(),
            ObjectCategory::Building,
        );
        assert_eq!(obj.vhp_scan, VhpScan::Strong);
    }

    #[test]
    fn building_native_speed_default_and_repair_type_inputs() {
        let ini = crate::rules::ini_parser::IniFile::from_str(
            "[TEST]\nPlaceAnywhere=yes\nToTile=Green01\n[EXPLICIT]\nSpeedType=Float\n",
        );
        let building = super::ObjectType::from_ini_section(
            "TEST",
            ini.section("TEST").unwrap(),
            super::ObjectCategory::Building,
        );
        assert_eq!(building.speed_type, super::SpeedType::Foot);
        assert!(building.place_anywhere);
        assert_eq!(building.to_tile.as_deref(), Some("Green01"));
        let explicit = super::ObjectType::from_ini_section(
            "EXPLICIT",
            ini.section("EXPLICIT").unwrap(),
            super::ObjectCategory::Building,
        );
        assert_eq!(explicit.speed_type, super::SpeedType::Float);
    }

    use super::*;
    use crate::rules::ini_parser::IniFile;

    #[test]
    fn test_parse_vehicle() {
        let ini: IniFile = IniFile::from_str(
            "[MTNK]\nName=Grizzly Battle Tank\nCost=700\nStrength=300\n\
             Armor=heavy\nSpeed=6\nSight=6\nTechLevel=2\n\
             Owner=Americans,Alliance\nRequiredHouses=Americans\n\
             Prerequisite=GAWEAP\nPrimary=105mm\n",
        );
        let section: &IniSection = ini.section("MTNK").unwrap();
        let obj: ObjectType =
            ObjectType::from_ini_section("MTNK", section, ObjectCategory::Vehicle);

        assert_eq!(obj.id, "MTNK");
        assert_eq!(obj.category, ObjectCategory::Vehicle);
        assert_eq!(obj.name, Some("Grizzly Battle Tank".to_string()));
        assert_eq!(obj.cost, 700);
        assert_eq!(obj.strength, 300);
        assert_eq!(obj.armor, "heavy");
        assert_eq!(obj.speed, 6);
        assert_eq!(obj.tech_level, 2);
        assert_eq!(
            obj.build_time_multiplier,
            crate::util::native_x87::NativeF32Bits::ONE
        );
        assert_eq!(obj.owner, vec!["Americans", "Alliance"]);
        assert_eq!(obj.required_houses, vec!["Americans"]);
        assert!(obj.allowed_to_start_in_multiplayer);
        assert_eq!(obj.primary(), Some("105mm"));
        assert_eq!(obj.secondary(), None);
        assert_eq!(obj.image, "MTNK"); // Defaults to ID when Image= absent.
        assert_eq!(obj.build_cat, None);
        assert_eq!(obj.adjacent, 3);
        assert!(obj.base_normal);
        assert!(!obj.eligibile_for_ally_building);
        assert!(!obj.crewed);
    }

    #[test]
    fn parses_miner_order_voices() {
        // Stock [CMIN] harvest ack: VoiceHarvest round-trips into the parsed
        // type so the order-ack dispatch can play it instead of the generic
        // move voice.
        let ini: IniFile = IniFile::from_str(
            "[CMIN]\nName=Chrono Miner\nVoiceMove=ChronoMinerMove\n\
             VoiceHarvest=ChronoMinerHarvest\nVoiceEnter=ChronoMinerReturn\n",
        );
        let obj = ObjectType::from_ini_section(
            "CMIN",
            ini.section("CMIN").unwrap(),
            ObjectCategory::Vehicle,
        );
        assert_eq!(obj.voice_move, Some("ChronoMinerMove".to_string()));
        assert_eq!(obj.voice_harvest, Some("ChronoMinerHarvest".to_string()));
        assert_eq!(obj.voice_enter, Some("ChronoMinerReturn".to_string()));

        // Absent keys default to None (e.g. a plain tank).
        let none_ini: IniFile = IniFile::from_str("[MTNK]\nName=Grizzly\n");
        let none_obj = ObjectType::from_ini_section(
            "MTNK",
            none_ini.section("MTNK").unwrap(),
            ObjectCategory::Vehicle,
        );
        assert_eq!(none_obj.voice_harvest, None);
        assert_eq!(none_obj.voice_enter, None);
    }

    /// GSI-06.04 carry-over: retail's capture order-ack reads the type's own
    /// `VoiceCapture=` slot and only falls back to Enter when the key is absent.
    /// Stock engineers ship the key, so it must round-trip through the parse.
    #[test]
    fn gsi_06_parses_voice_capture_slot() {
        let ini: IniFile = IniFile::from_str(
            "[ENGINEER]\nName=Engineer\nVoiceMove=EngAllMove\n\
             VoiceEnter=EngAllMove\nVoiceCapture=EngAllAttackCommand\n",
        );
        let obj = ObjectType::from_ini_section(
            "ENGINEER",
            ini.section("ENGINEER").unwrap(),
            ObjectCategory::Infantry,
        );
        assert_eq!(obj.voice_capture, Some("EngAllAttackCommand".to_string()));

        // Absent key stays None, which is the retail "fall back to Enter" case.
        let none_ini: IniFile = IniFile::from_str("[E1]\nName=GI\n");
        let none_obj = ObjectType::from_ini_section(
            "E1",
            none_ini.section("E1").unwrap(),
            ObjectCategory::Infantry,
        );
        assert_eq!(none_obj.voice_capture, None);
    }

    /// GSI-06.04 carry-over: retail's attack-move eligibility is
    /// `Primary != null && !PreventAttackMove`. Stock engineers carry both a
    /// real `Primary=` and the key, so the flag has to survive the parse for
    /// the predicate to refuse them.
    #[test]
    fn gsi_06_parses_prevent_attack_move_flag() {
        let ini: IniFile = IniFile::from_str(
            "[ENGINEER]\nName=Engineer\nPrimary=DefuseKit\nPreventAttackMove=yes\n",
        );
        let obj = ObjectType::from_ini_section(
            "ENGINEER",
            ini.section("ENGINEER").unwrap(),
            ObjectCategory::Infantry,
        );
        assert!(obj.prevent_attack_move);
        assert_eq!(obj.primary(), Some("DefuseKit"));

        // Absent key defaults to false — an ordinary tank still attack-moves.
        let none_ini: IniFile = IniFile::from_str("[MTNK]\nName=Grizzly\nPrimary=90mm\n");
        let none_obj = ObjectType::from_ini_section(
            "MTNK",
            none_ini.section("MTNK").unwrap(),
            ObjectCategory::Vehicle,
        );
        assert!(!none_obj.prevent_attack_move);
    }

    #[test]
    fn parses_ui_name_key() {
        let ini: IniFile =
            IniFile::from_str("[MTNK]\nName=Grizzly Tank\nUIName=Name:MTNK\nCost=700\n");
        let parsed = ObjectType::from_ini_section(
            "MTNK",
            ini.section("MTNK").unwrap(),
            ObjectCategory::Vehicle,
        );
        assert_eq!(parsed.ui_name, Some("Name:MTNK".to_string()));

        // Absent UIName yields None.
        let absent_ini = IniFile::from_str("[HTNK]\nName=Rhino Tank\n");
        let absent = ObjectType::from_ini_section(
            "HTNK",
            absent_ini.section("HTNK").unwrap(),
            ObjectCategory::Vehicle,
        );
        assert_eq!(absent.ui_name, None);

        // Empty UIName= also yields None.
        let empty_ini = IniFile::from_str("[E1]\nUIName=\nCost=0\n");
        let empty = ObjectType::from_ini_section(
            "E1",
            empty_ini.section("E1").unwrap(),
            ObjectCategory::Infantry,
        );
        assert_eq!(empty.ui_name, None);
    }

    #[test]
    fn allowed_to_start_in_multiplayer_parses_and_defaults_yes() {
        let enabled_ini = IniFile::from_str("[MTNK]\nAllowedToStartInMultiplayer=yes\n");
        let enabled = ObjectType::from_ini_section(
            "MTNK",
            enabled_ini.section("MTNK").unwrap(),
            ObjectCategory::Vehicle,
        );
        assert!(enabled.allowed_to_start_in_multiplayer);

        let disabled_ini = IniFile::from_str("[HTNK]\nAllowedToStartInMultiplayer=no\n");
        let disabled = ObjectType::from_ini_section(
            "HTNK",
            disabled_ini.section("HTNK").unwrap(),
            ObjectCategory::Vehicle,
        );
        assert!(!disabled.allowed_to_start_in_multiplayer);

        let default_ini = IniFile::from_str("[E1]\nFixtureOnly=1\n");
        let defaulted = ObjectType::from_ini_section(
            "E1",
            default_ini.section("E1").unwrap(),
            ObjectCategory::Infantry,
        );
        assert!(defaulted.allowed_to_start_in_multiplayer);
    }

    #[test]
    fn test_parse_building() {
        let ini: IniFile = IniFile::from_str(
            "[GAPOWR]\nName=Power Plant\nCost=800\nStrength=750\n\
             Power=200\nFoundation=2x2\nArmor=wood\nBuildCat=Power\nCrewed=yes\n",
        );
        let section: &IniSection = ini.section("GAPOWR").unwrap();
        let obj: ObjectType =
            ObjectType::from_ini_section("GAPOWR", section, ObjectCategory::Building);

        assert_eq!(obj.power, 200);
        assert_eq!(obj.foundation, "1x1", "RULES Foundation is not an input");
        assert_eq!(obj.armor, "wood");
        assert_eq!(obj.build_cat, Some(BuildCategory::Power));
        assert_eq!(obj.adjacent, 3);
        assert!(obj.base_normal);
        assert!(!obj.eligibile_for_ally_building);
        assert!(obj.crewed);
    }

    #[test]
    fn test_light_visibility_defaults_to_5000_without_intensity() {
        let ini: IniFile = IniFile::from_str("[GALITE]\nFixtureOnly=1\n");
        let section: &IniSection = ini.section("GALITE").unwrap();
        let obj: ObjectType =
            ObjectType::from_ini_section("GALITE", section, ObjectCategory::Building);

        assert_eq!(obj.light_visibility, 5000);
        assert_eq!(obj.light_intensity, 0.0);
    }

    #[test]
    fn test_light_fields_use_light_float_parser() {
        let ini: IniFile = IniFile::from_str(
            "[GALITE]\n\
             LightVisibility=4096\n\
             LightIntensity=0.75\n\
             LightRedTint=-0.25\n\
             LightGreenTint=0,01\n\
             LightBlueTint=1.5\n",
        );
        let section: &IniSection = ini.section("GALITE").unwrap();
        let obj: ObjectType =
            ObjectType::from_ini_section("GALITE", section, ObjectCategory::Building);

        assert_eq!(obj.light_visibility, 4096);
        assert!((obj.light_intensity - 0.75).abs() < 0.001);
        assert!((obj.light_red_tint + 0.25).abs() < 0.001);
        assert_eq!(obj.light_green_tint, 0.0);
        assert!((obj.light_blue_tint - 1.5).abs() < 0.001);
    }

    #[test]
    fn parse_bridge_repair_hut_flag() {
        let ini: IniFile =
            IniFile::from_str("[CABHUT]\nBridgeRepairHut=yes\n[NACABH]\nFixtureOnly=1\n");
        let obj_on: ObjectType = ObjectType::from_ini_section(
            "CABHUT",
            ini.section("CABHUT").unwrap(),
            ObjectCategory::Building,
        );
        let obj_off: ObjectType = ObjectType::from_ini_section(
            "NACABH",
            ini.section("NACABH").unwrap(),
            ObjectCategory::Building,
        );
        assert!(obj_on.bridge_repair_hut);
        assert!(!obj_off.bridge_repair_hut);
    }

    #[test]
    fn parse_laser_fence_flag() {
        let ini: IniFile = IniFile::from_str("[FENCE]\nLaserFence=yes\n[OTHER]\nFixtureOnly=1\n");
        let fence = ObjectType::from_ini_section(
            "FENCE",
            ini.section("FENCE").unwrap(),
            ObjectCategory::Building,
        );
        let other = ObjectType::from_ini_section(
            "OTHER",
            ini.section("OTHER").unwrap(),
            ObjectCategory::Building,
        );
        assert!(fence.laser_fence);
        assert!(!other.laser_fence);
    }

    #[test]
    fn firestorm_wall_matches_original_native_reader() {
        #[derive(serde::Deserialize)]
        struct Row {
            raw: Option<String>,
            output: bool,
        }
        #[derive(serde::Deserialize)]
        struct Fixture {
            firestorm_wall: Vec<Row>,
        }
        let fixture: Fixture = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/building_body_rules.json",
        ))
        .unwrap();
        for row in fixture.firestorm_wall {
            let mut section = IniSection::new("TEST".to_string());
            if let Some(raw) = &row.raw {
                section.set("FirestormWall", raw);
            }
            let object = ObjectType::from_ini_section("TEST", &section, ObjectCategory::Building);
            assert_eq!(object.firestorm_wall, row.output, "{:?}", row.raw);
        }
    }

    #[test]
    fn test_defaults_for_missing_keys() {
        let ini: IniFile = IniFile::from_str("[BARE]\nFixtureOnly=1\n");
        let section: &IniSection = ini.section("BARE").unwrap();
        let obj: ObjectType =
            ObjectType::from_ini_section("BARE", section, ObjectCategory::Infantry);

        assert_eq!(obj.cost, 0);
        assert_eq!(obj.strength, 0);
        assert_eq!(obj.armor, "none");
        assert_eq!(obj.speed, 0);
        assert_eq!(obj.walk_rate, 1);
        assert_eq!(obj.idle_rate, 0);
        assert_eq!(obj.sight, 0);
        assert_eq!(obj.tech_level, 255);
        assert_eq!(
            obj.build_time_multiplier,
            crate::util::native_x87::NativeF32Bits::ONE
        );
        assert!(obj.owner.is_empty());
        assert!(obj.required_houses.is_empty());
        assert_eq!(obj.primary(), None);
        assert_eq!(obj.image, "BARE");
        assert_eq!(obj.power, 0);
        assert_eq!(obj.foundation, "1x1");
        assert_eq!(obj.build_cat, None);
        assert_eq!(obj.adjacent, 3);
        assert!(obj.base_normal);
        assert!(!obj.eligibile_for_ally_building);
        assert!(!obj.crewed);
    }

    #[test]
    fn gsi_13_06_walk_and_idle_rates_are_raw_rules_owned_signed_values() {
        let ini = IniFile::from_str(
            "[DLPH]\nWalkRate=4\nIdleRate=8\n\
             [SQD]\nWalkRate=2\nIdleRate=4\n\
             [DRON]\nFixtureOnly=1\n\
             [RAW]\nWalkRate=-3\nIdleRate=-7\n",
        );
        let parse = |id| {
            ObjectType::from_ini_section(
                id,
                ini.section(id).expect("section"),
                ObjectCategory::Vehicle,
            )
        };

        let dlph = parse("DLPH");
        assert_eq!((dlph.walk_rate, dlph.idle_rate), (4, 8));
        let sqd = parse("SQD");
        assert_eq!((sqd.walk_rate, sqd.idle_rate), (2, 4));
        let dron = parse("DRON");
        assert_eq!((dron.walk_rate, dron.idle_rate), (1, 0));
        let raw = parse("RAW");
        assert_eq!((raw.walk_rate, raw.idle_rate), (-3, -7));
    }

    #[test]
    fn test_parse_build_time_multiplier() {
        let ini: IniFile = IniFile::from_str("[HTNK]\nBuildTimeMultiplier=1.3\n");
        let section: &IniSection = ini.section("HTNK").unwrap();
        let obj: ObjectType =
            ObjectType::from_ini_section("HTNK", section, ObjectCategory::Vehicle);

        assert_eq!(
            obj.build_time_multiplier,
            crate::util::native_x87::NativeF32Bits::from_bits(1.3_f32.to_bits())
        );
    }

    #[test]
    fn test_parse_building_placement_flags() {
        let ini: IniFile = IniFile::from_str(
            "[GAGAP]\nFoundation=2x2\nAdjacent=0\nBaseNormal=no\nEligibileForAllyBuilding=yes\n",
        );
        let section: &IniSection = ini.section("GAGAP").unwrap();
        let obj: ObjectType =
            ObjectType::from_ini_section("GAGAP", section, ObjectCategory::Building);

        assert_eq!(obj.adjacent, 0);
        assert!(!obj.base_normal);
        assert!(obj.eligibile_for_ally_building);
    }

    #[test]
    fn test_parse_bridge_render_flags() {
        let ini: IniFile = IniFile::from_str(
            "[DEST]\nZFudgeCliff=0\nZFudgeColumn=-7\nZFudgeTunnel=13\nZFudgeBridge=11\nTooBigToFitUnderBridge=yes\n",
        );
        let section: &IniSection = ini.section("DEST").unwrap();
        let obj: ObjectType =
            ObjectType::from_ini_section("DEST", section, ObjectCategory::Vehicle);
        assert_eq!(obj.zfudge_cliff, 0);
        assert_eq!(obj.zfudge_column, -7);
        assert_eq!(obj.zfudge_tunnel, 13);
        assert_eq!(obj.zfudge_bridge, 11);
        assert!(obj.too_big_to_fit_under_bridge);
    }

    #[test]
    fn test_bridge_render_flags_default() {
        let ini: IniFile = IniFile::from_str("[BOAT]\nFixtureOnly=1\n");
        let section: &IniSection = ini.section("BOAT").unwrap();
        let obj: ObjectType =
            ObjectType::from_ini_section("BOAT", section, ObjectCategory::Vehicle);
        assert_eq!(obj.zfudge_cliff, 10);
        assert_eq!(obj.zfudge_column, 5);
        assert_eq!(obj.zfudge_tunnel, 10);
        assert_eq!(obj.zfudge_bridge, 0);
        assert!(!obj.too_big_to_fit_under_bridge);
    }

    #[test]
    fn considered_aircraft_uses_native_category_defaults_and_ini_overrides() {
        let ini = IniFile::from_str(
            "[VEH_DEFAULT]\nFixtureOnly=1\n\
             [AIR_DEFAULT]\nFixtureOnly=1\n\
             [VEH_EXPLICIT]\nConsideredAircraft=yes\n\
             [AIR_EXPLICIT]\nConsideredAircraft=no\n",
        );

        let parse =
            |id, category| ObjectType::from_ini_section(id, ini.section(id).unwrap(), category);

        assert!(!parse("VEH_DEFAULT", ObjectCategory::Vehicle).considered_aircraft);
        assert!(parse("AIR_DEFAULT", ObjectCategory::Aircraft).considered_aircraft);
        assert!(parse("VEH_EXPLICIT", ObjectCategory::Vehicle).considered_aircraft);
        assert!(!parse("AIR_EXPLICIT", ObjectCategory::Aircraft).considered_aircraft);
    }

    #[test]
    fn test_parse_deploys_into() {
        let ini: IniFile = IniFile::from_str("[AMCV]\nDeploysInto=GACNST\nSpeed=4\n");
        let section: &IniSection = ini.section("AMCV").unwrap();
        let obj: ObjectType =
            ObjectType::from_ini_section("AMCV", section, ObjectCategory::Vehicle);
        assert_eq!(obj.deploys_into, Some("GACNST".to_string()));
        assert_eq!(obj.undeploys_into, None);
    }

    #[test]
    fn test_parse_undeploys_into() {
        let ini: IniFile = IniFile::from_str("[GACNST]\nUndeploysInto=AMCV\nFoundation=4x4\n");
        let section: &IniSection = ini.section("GACNST").unwrap();
        let mut obj: ObjectType =
            ObjectType::from_ini_section("GACNST", section, ObjectCategory::Building);
        assert_eq!(obj.undeploys_into, Some("AMCV".to_string()));
        assert_eq!(obj.deploys_into, None);
        assert!(
            obj.is_1x1_with_undeploy(),
            "constructor remains1x1 until ART reads"
        );
        // This gate consumes a loaded native field; ART input provenance and
        // every table dimension are tested by building_foundation_pass_matches_original_art_reads.
        obj.foundation = "4x4".to_owned();
        assert!(!obj.is_1x1_with_undeploy());

        let one_by_one = IniFile::from_str("[MODDEPLOY]\nUndeploysInto=MODUNIT\nFoundation=1x1\n");
        let obj = ObjectType::from_ini_section(
            "MODDEPLOY",
            one_by_one.section("MODDEPLOY").unwrap(),
            ObjectCategory::Building,
        );
        assert!(obj.is_1x1_with_undeploy());
    }

    #[test]
    fn parse_construction_yard_and_deploy_facing() {
        let ini: IniFile = IniFile::from_str("[GACNST]\nConstructionYard=yes\nDeployFacing=2\n");
        let section: &IniSection = ini.section("GACNST").unwrap();
        let obj = ObjectType::from_ini_section("GACNST", section, ObjectCategory::Building);
        assert!(obj.construction_yard);
        assert_eq!(obj.deploy_facing, 0x40);

        let default_ini = IniFile::from_str("[GAPOWR]\nFixtureOnly=1\n");
        let default_obj = ObjectType::from_ini_section(
            "GAPOWR",
            default_ini.section("GAPOWR").unwrap(),
            ObjectCategory::Building,
        );
        assert!(!default_obj.construction_yard);
        assert_eq!(default_obj.deploy_facing, 0x80);
    }

    #[test]
    fn ai_build_this_defaults_false_and_reads_native_boolean() {
        let ini = IniFile::from_str(
            "[DEFAULT]\nFixtureOnly=1\n[YES]\nAIBuildThis=yes\n[NO]\nAIBuildThis=no\n",
        );
        let parse = |id| {
            ObjectType::from_ini_section(id, ini.section(id).unwrap(), ObjectCategory::Building)
        };
        assert!(!parse("DEFAULT").ai_build_this);
        assert!(parse("YES").ai_build_this);
        assert!(!parse("NO").ai_build_this);
    }

    #[test]
    fn test_parse_slave_miner_fields() {
        let ini: IniFile = IniFile::from_str(
            "[SMIN]\nEnslaves=SLAV\nSlavesNumber=5\nSlaveRegenRate=500\n\
             SlaveReloadRate=25\nResourceGatherer=yes\nResourceDestination=yes\n\
             DeploysInto=YAREFN\nStorage=20\nSpeed=3\n",
        );
        let section: &IniSection = ini.section("SMIN").unwrap();
        let obj: ObjectType =
            ObjectType::from_ini_section("SMIN", section, ObjectCategory::Vehicle);
        assert_eq!(obj.enslaves, Some("SLAV".to_string()));
        assert_eq!(obj.slaves_number, 5);
        assert_eq!(obj.slave_regen_rate, 500);
        assert_eq!(obj.slave_reload_rate, 25);
        assert!(obj.resource_gatherer);
        assert!(obj.resource_destination);
        assert_eq!(obj.deploys_into, Some("YAREFN".to_string()));
        assert!(!obj.slaved); // SMIN is master, not slave
    }

    #[test]
    fn test_parse_slave_infantry_fields() {
        let ini: IniFile = IniFile::from_str("[SLAV]\nSlaved=yes\nStorage=4\nHarvestRate=150\n");
        let section: &IniSection = ini.section("SLAV").unwrap();
        let obj: ObjectType =
            ObjectType::from_ini_section("SLAV", section, ObjectCategory::Infantry);
        assert!(obj.slaved);
        assert_eq!(obj.storage, 4);
        assert_eq!(obj.harvest_rate, 150);
        assert_eq!(obj.enslaves, None);
    }

    #[test]
    fn test_parse_ore_purifier() {
        let ini: IniFile = IniFile::from_str("[GAPROC]\nOrePurifier=yes\n");
        let section: &IniSection = ini.section("GAPROC").unwrap();
        let obj: ObjectType =
            ObjectType::from_ini_section("GAPROC", section, ObjectCategory::Building);
        assert!(obj.ore_purifier);
    }

    #[test]
    fn test_parse_refinery_free_unit_and_dock() {
        let ini: IniFile = IniFile::from_str(
            "[MODPROC]\nRefinery=yes\nDockUnload=yes\nFreeUnit=MODHARV\n\
             [MODHARV]\nHarvester=yes\nDock=modproc,NAREFN\n",
        );

        let refinery = ObjectType::from_ini_section(
            "MODPROC",
            ini.section("MODPROC").expect("MODPROC section"),
            ObjectCategory::Building,
        );
        assert!(refinery.refinery);
        assert_eq!(refinery.free_unit, Some("MODHARV".to_string()));

        let harvester = ObjectType::from_ini_section(
            "MODHARV",
            ini.section("MODHARV").expect("MODHARV section"),
            ObjectCategory::Vehicle,
        );
        assert!(harvester.harvester);
        assert_eq!(
            harvester.dock,
            vec!["MODPROC".to_string(), "NAREFN".to_string()]
        );
    }

    #[test]
    fn stock_cmin_rot_remains_5_after_harvester_parse() {
        let ini: IniFile = IniFile::from_str("[CMIN]\nHarvester=yes\nROT=5\n");
        let section: &IniSection = ini.section("CMIN").unwrap();
        let obj: ObjectType =
            ObjectType::from_ini_section("CMIN", section, ObjectCategory::Vehicle);

        assert!(obj.harvester);
        assert_eq!(obj.turret_rot, 5);
    }

    #[test]
    fn unit_weeder_is_independent_from_slave_ownership() {
        let ini: IniFile = IniFile::from_str("[WEED]\nWeeder=yes\n[SLAVER]\nEnslaves=SLAV\n");
        let weeder = ObjectType::from_ini_section(
            "WEED",
            ini.section("WEED").unwrap(),
            ObjectCategory::Vehicle,
        );
        let slaver = ObjectType::from_ini_section(
            "SLAVER",
            ini.section("SLAVER").unwrap(),
            ObjectCategory::Vehicle,
        );

        assert!(weeder.weeder);
        assert!(weeder.enslaves.is_none());
        assert!(!slaver.weeder);
        assert_eq!(slaver.enslaves.as_deref(), Some("SLAV"));
    }

    #[test]
    fn cloning_key_parses_and_participates_in_rally_lines() {
        let ini = IniFile::from_str("[YACLON]\nName=Cloning Vats\nStrength=1000\nCloning=yes\n");
        let section = ini.section("YACLON").unwrap();
        let obj = ObjectType::from_ini_section("YACLON", section, ObjectCategory::Building);
        assert!(obj.cloning);
        assert!(obj.has_rally_line());
    }

    #[test]
    fn rally_line_accepts_infantry_vehicle_factories_and_repair() {
        let ini = IniFile::from_str(
            "[GAPILE]\nFactory=InfantryType\n\
             [GAWEAP]\nFactory=UnitType\n\
             [GADEPT]\nUnitRepair=yes\n",
        );
        let barracks = ObjectType::from_ini_section(
            "GAPILE",
            ini.section("GAPILE").unwrap(),
            ObjectCategory::Building,
        );
        let factory = ObjectType::from_ini_section(
            "GAWEAP",
            ini.section("GAWEAP").unwrap(),
            ObjectCategory::Building,
        );
        let depot = ObjectType::from_ini_section(
            "GADEPT",
            ini.section("GADEPT").unwrap(),
            ObjectCategory::Building,
        );
        assert!(barracks.has_rally_line());
        assert!(factory.has_rally_line());
        assert!(depot.has_rally_line());
    }

    #[test]
    fn weapons_factory_parses_independently_from_factory_type() {
        let ini = IniFile::from_str(
            "[MISSING]\n\
             FixtureOnly=1\n\
             [EXPLICIT_NO]\nWeaponsFactory=no\n\
             [EXPLICIT_YES]\nWeaponsFactory=yes\n\
             [FACTORY_ONLY]\nFactory=UnitType\n\
             [WEAPONS_FACTORY_ONLY]\nWeaponsFactory=yes\n",
        );

        let parse = |id| {
            ObjectType::from_ini_section(
                id,
                ini.section(id).expect("fixture section"),
                ObjectCategory::Building,
            )
        };

        assert!(!parse("MISSING").weapons_factory);
        assert!(!parse("EXPLICIT_NO").weapons_factory);
        assert!(parse("EXPLICIT_YES").weapons_factory);
        assert_eq!(parse("FACTORY_ONLY").factory, Some(FactoryType::UnitType));
        assert!(!parse("FACTORY_ONLY").weapons_factory);
        assert_eq!(parse("WEAPONS_FACTORY_ONLY").factory, None);
        assert!(parse("WEAPONS_FACTORY_ONLY").weapons_factory);
    }

    #[test]
    fn barracks_flags_keep_native_defaults_parser_and_class_scope() {
        // Original ctor45E1C2/45E1C8/45E1CE and ReadBool5295F0:
        // missing/unrecognized -> current byte; only the first byte decides.
        for (raw, expected) in [
            ("", false),
            ("yes", true),
            ("Tjunk", true),
            ("1junk", true),
            ("no", false),
            ("false", false),
            ("0junk", false),
            ("on", false),
            ("off", false),
        ] {
            let ini = IniFile::from_str(&format!(
                "[TYPE]\nFactory=InfantryType\nGDIBarracks={raw}\nNODBarracks={raw}\nYuriBarracks={raw}\n"
            ));
            let section = ini.section("TYPE").unwrap();
            for category in [
                ObjectCategory::Building,
                ObjectCategory::Infantry,
                ObjectCategory::Vehicle,
                ObjectCategory::Aircraft,
            ] {
                let object = ObjectType::from_ini_section("TYPE", section, category);
                let native = category == ObjectCategory::Building && expected;
                assert_eq!(
                    (
                        object.gdi_barracks(),
                        object.nod_barracks(),
                        object.yuri_barracks()
                    ),
                    (native, native, native),
                    "{category:?} {raw:?}"
                );
            }
        }
        let ini = IniFile::from_str(
            "[TYPE]\nFactory=InfantryType\ngdibarracks=yes\nNodBarracks=yes\nYURIBARRACKS=yes\n",
        );
        let object = ObjectType::from_ini_section(
            "TYPE",
            ini.section("TYPE").unwrap(),
            ObjectCategory::Building,
        );
        assert_eq!(
            (
                object.gdi_barracks(),
                object.nod_barracks(),
                object.yuri_barracks()
            ),
            (false, false, false)
        );
    }

    #[test]
    fn armory_flag_retains_native_defaults_across_reached_rules_layers() {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        use crate::rules::ruleset::RuleSet;
        //45E157 default0;460B03/460B08 use the shared ReadBool5295F0
        //reader before the three barracks keys. Native ordinary GAPILE reads0.
        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[BuildingTypes]\n0=BRKS\n[BRKS]\nArmory=yes\n",
        ));
        assert!(
            RuleSet::from_rules_layers(&layers)
                .unwrap()
                .object("BRKS")
                .unwrap()
                .armory()
        );
        layers.push(
            RulesLayerKind::LangRule,
            IniFile::from_str("[BRKS]\nArmory=malformed\n"),
        );
        assert!(
            RuleSet::from_rules_layers(&layers)
                .unwrap()
                .object("BRKS")
                .unwrap()
                .armory()
        );
        layers.push(
            RulesLayerKind::GameMode,
            IniFile::from_str("[BRKS]\nArmory=n\n"),
        );
        assert!(
            !RuleSet::from_rules_layers(&layers)
                .unwrap()
                .object("BRKS")
                .unwrap()
                .armory()
        );
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[BRKS]\narmory=yes\n"),
        );
        assert!(
            !RuleSet::from_rules_layers(&layers)
                .unwrap()
                .object("BRKS")
                .unwrap()
                .armory()
        );
        let ini = IniFile::from_str("[TYPE]\nArmory=yes\n");
        assert!(
            !ObjectType::from_ini_section(
                "TYPE",
                ini.section("TYPE").unwrap(),
                ObjectCategory::Infantry
            )
            .armory()
        );
        let empty = IniFile::from_str("[TYPE]\n");
        assert!(
            !ObjectType::from_ini_section(
                "TYPE",
                empty.section_or_empty("TYPE"),
                ObjectCategory::Building
            )
            .armory()
        );
    }

    #[test]
    fn barracks_flags_keep_live_defaults_across_rules_layers() {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        use crate::rules::ruleset::RuleSet;
        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[BuildingTypes]\n0=BRKS\n[BRKS]\nFactory=InfantryType\nGDIBarracks=yes\nNODBarracks=yes\n",
        ));
        for (kind, patch, expected) in [
            (
                RulesLayerKind::LangRule,
                "[BRKS]\nName=Fixture\nNODBarracks=malformed\n",
                (true, true, false),
            ),
            (
                RulesLayerKind::GameMode,
                "[BRKS]\nGDIBarracks=no\nYuriBarracks=yes\n",
                (false, true, true),
            ),
            (
                RulesLayerKind::Scenario,
                "[BRKS]\nGDIBarracks=t\nNODBarracks=off\nYuriBarracks=0\n",
                (true, true, false),
            ),
        ] {
            layers.push(kind, IniFile::from_str(patch));
            let rules = RuleSet::from_rules_layers(&layers).unwrap();
            let object = rules.object("BRKS").unwrap();
            assert_eq!(
                (
                    object.gdi_barracks(),
                    object.nod_barracks(),
                    object.yuri_barracks()
                ),
                expected,
                "{kind:?}"
            );
        }
    }

    #[test]
    fn retail_barracks_flags_match_whole_original_gapile_reader() {
        // Whole original BuildingType ctor/reader for the stock GAPILE and
        // GAPOWR in the sealed native input contract: all defaults false,
        // GAPILE GDIBarracks=yes; later selected mode/map have no override.
        let Some(ini) = retail_rules_ini() else {
            return;
        };
        let rules = crate::rules::ruleset::RuleSet::from_ini(&ini).unwrap();
        for (id, expected) in [
            ("GAPILE", (true, false, false)),
            ("GAPOWR", (false, false, false)),
        ] {
            let object = rules.object(id).unwrap();
            assert_eq!(
                (
                    object.gdi_barracks(),
                    object.nod_barracks(),
                    object.yuri_barracks()
                ),
                expected,
                "[{id}]"
            );
        }
    }

    #[test]
    fn weapons_factory_flag_includes_stock_land_factories_and_naval_yards() {
        let merged = IniFile::from_str(
            "[GAWEAP]\nWeaponsFactory=yes\n\
             [NAWEAP]\nWeaponsFactory=yes\n\
             [GAYARD]\nWeaponsFactory=yes\n\
             [NAYARD]\nWeaponsFactory=yes\n\
             [YAWEAP]\nWeaponsFactory=yes\n\
             [YAYARD]\nWeaponsFactory=yes\n\
             [GAPILE]\nFactory=InfantryType\n\
             [GAPOWR]\nPower=200\n",
        );

        for id in ["GAWEAP", "NAWEAP", "GAYARD", "NAYARD", "YAWEAP", "YAYARD"] {
            let object = ObjectType::from_ini_section(
                id,
                merged.section(id).expect("stock building section"),
                ObjectCategory::Building,
            );
            assert!(
                object.weapons_factory,
                "{id} must retain WeaponsFactory=yes"
            );
        }

        for id in ["GAPILE", "GAPOWR"] {
            let object = ObjectType::from_ini_section(
                id,
                merged
                    .section(id)
                    .expect("stock non-weapons-factory section"),
                ObjectCategory::Building,
            );
            assert!(
                !object.weapons_factory,
                "{id} must not be inferred as a weapons factory"
            );
        }
    }

    #[test]
    fn test_parse_transport_fields() {
        let ini: IniFile = IniFile::from_str(
            "[HTK]\nPassengers=5\nSizeLimit=2\nOpenTopped=no\nGunner=no\nSize=3\n",
        );
        let section: &IniSection = ini.section("HTK").unwrap();
        let obj = ObjectType::from_ini_section("HTK", section, ObjectCategory::Vehicle);
        assert_eq!(obj.passengers, 5);
        assert_eq!(obj.size_limit, 2);
        assert_eq!(obj.size, 3);
        assert!(!obj.open_topped);
        assert!(!obj.gunner);
        assert_eq!(obj.weapon_list.len(), WEAPON_SLOT_COUNT);
        assert!(obj.weapon_list.iter().all(Option::is_none));
    }

    #[test]
    fn gsi_04_05_passengers_preserves_native_signed_nonzero_value() {
        let ini = IniFile::from_str("[ODDTRANSPORT]\nPassengers=-3\n");
        let obj = ObjectType::from_ini_section(
            "ODDTRANSPORT",
            ini.section("ODDTRANSPORT").unwrap(),
            ObjectCategory::Vehicle,
        );
        assert_eq!(obj.passengers, -3);
    }

    #[test]
    fn test_parse_ifv_gunner_fields() {
        // Stock-shaped: the `Weapon%d` loop only runs for a `TurretCount>0`
        // type, and only up to `WeaponCount` slots.
        let ini: IniFile = IniFile::from_str(
            "[FV]\nPassengers=1\nSizeLimit=1\nGunner=yes\nSize=3\n\
             TurretCount=4\nWeaponCount=3\n\
             Weapon1=Missiles\nWeapon2=FlakGun\nWeapon3=RepairArm\n",
        );
        let section: &IniSection = ini.section("FV").unwrap();
        let obj = ObjectType::from_ini_section("FV", section, ObjectCategory::Vehicle);
        assert_eq!(obj.passengers, 1);
        assert_eq!(obj.size_limit, 1);
        assert!(obj.gunner);
        assert_eq!(obj.weapon_list.len(), WEAPON_SLOT_COUNT);
        assert_eq!(obj.weapon_list[0].as_deref(), Some("Missiles"));
        assert_eq!(obj.weapon_list[1].as_deref(), Some("FlakGun"));
        assert_eq!(obj.weapon_list[2].as_deref(), Some("RepairArm"));
        assert!(obj.weapon_list[3..].iter().all(Option::is_none));
        assert!(obj.elite_weapon_list.iter().all(Option::is_none));
        // Slots 0 and 1 ARE the `Primary`/`Secondary` fields.
        assert_eq!(obj.primary(), Some("Missiles"));
        assert_eq!(obj.secondary(), Some("FlakGun"));
        assert_eq!(obj.elite_primary(), None);
    }

    /// `Primary=` and `Weapon1=` are the same storage in gamemd, so whichever
    /// ReadINI branch a type takes must land in the same fields.
    ///
    /// gamemd-derived: `TechnoTypeClass::ReadINI`. The `Weapon%d` cursor starts
    /// at `0x007128D6 LEA EDI,[EBP+0xA94]` and stores the base result at
    /// `0x0071294A MOV [EDI-0x1FC],EAX`; on iteration 1 that address is
    /// `0xA94 - 0x1FC` = `+0x898`, which is exactly where the `Primary=` block
    /// stores at `0x007129F2`. `Secondary`/slot 1 is `+0x8B4` (`0x00712A17`),
    /// elite slot 0 `+0xA94` (`0x00712A64`), elite slot 1 `+0xAB0`
    /// (`0x00712A89`), stride `0x1C` (`0x00712992`).
    #[test]
    fn gsi_08_02_weapon_one_and_primary_share_one_field() {
        let ini = IniFile::from_str(
            // Turret branch: Primary=/Secondary= are ignored, but the fields
            // they name are filled from Weapon1=/Weapon2=.
            "[SREF]\nTurretCount=4\nWeaponCount=1\nWeapon1=Comet\nEliteWeapon1=SuperComet\n\
             Weapon2=Ignored\n\
             [YAGGUN]\nTurretCount=1\nWeaponCount=6\nWeapon1=AGGattling\nWeapon2=AAGattCann\n\
             EliteWeapon2=AAGattlingE\n\
             [MTNK]\nPrimary=105mm\nSecondary=MachGun\nWeapon1=NeverRead\n\
             [WIPED]\nClearAllWeapons=yes\nPrimary=105mm\nSecondary=MachGun\n\
             [NOSLOTS]\nTurretCount=2\nWeaponCount=0\nWeapon1=NeverRead\nPrimary=NeverRead\n",
        );
        let obj = |id: &str| {
            ObjectType::from_ini_section(id, ini.section(id).unwrap(), ObjectCategory::Vehicle)
        };

        // Prism Tank: `Weapon1=` IS the `Primary` field. `WeaponCount=1` stops
        // the loop, so `Weapon2=` never reaches slot 1.
        let sref = obj("SREF");
        assert_eq!(sref.primary(), Some("Comet"));
        assert_eq!(sref.elite_primary(), Some("SuperComet"));
        assert_eq!(sref.secondary(), None);

        // Gattling Cannon: both slots filled, including the elite array.
        let yaggun = obj("YAGGUN");
        assert_eq!(yaggun.primary(), Some("AGGattling"));
        assert_eq!(yaggun.secondary(), Some("AAGattCann"));
        assert_eq!(yaggun.elite_secondary(), Some("AAGattlingE"));

        // No turret: the `Primary=` block runs and `Weapon1=` is never read.
        let mtnk = obj("MTNK");
        assert_eq!(mtnk.primary(), Some("105mm"));
        assert_eq!(mtnk.secondary(), Some("MachGun"));
        assert!(mtnk.weapon_list[2..].iter().all(Option::is_none));

        // `ClearAllWeapons=` skips the whole `Primary=` block (`0x007129A5`).
        let wiped = obj("WIPED");
        assert_eq!(wiped.primary(), None);
        assert_eq!(wiped.secondary(), None);

        // `WeaponCount <= 0` skips the loop AND the `Primary=` block
        // (`0x007128C8` jumps to `0x00712A8F`).
        let noslots = obj("NOSLOTS");
        assert_eq!(noslots.primary(), None);
        assert!(noslots.weapon_list.iter().all(Option::is_none));
    }

    #[test]
    fn weapon_slot_lists_parse_for_non_gunner_types_and_selector_flags_parse() {
        // Stock YAGGUN shape: IsGattling with six WeaponN/EliteWeaponN slots
        // and no Gunner=. Stock DEST shape: NavalTargeting=1.
        let ini: IniFile = IniFile::from_str(
            "[YAGGUN]\nIsGattling=yes\nTurretCount=1\nWeaponCount=6\nWeapon1=AG1\nEliteWeapon1=AG1E\n\
             Weapon2=AA1\nWeapon6=AA3\nEliteWeapon6=AA3E\n\
             [DEST]\nNavalTargeting=1\nNaval=yes\n\
             [BSUB]\nLandTargeting=2\nUnderwater=yes\nUnnatural=yes\n\
             [SQD]\nOrganic=yes\nNavalTargeting=3\n\
             [GAPOWR]\nDrainable=yes\n[TESLA]\nOverpowerable=true\n",
        );
        let yaggun = ObjectType::from_ini_section(
            "YAGGUN",
            ini.section("YAGGUN").unwrap(),
            ObjectCategory::Building,
        );
        assert!(yaggun.is_gattling);
        assert_eq!(yaggun.turret_count, 1);
        assert_eq!(yaggun.weapon_count, 6);
        assert_eq!(yaggun.weapon_list[0].as_deref(), Some("AG1"));
        assert_eq!(yaggun.elite_weapon_list[0].as_deref(), Some("AG1E"));
        assert_eq!(yaggun.weapon_list[1].as_deref(), Some("AA1"));
        assert!(yaggun.weapon_list[2].is_none());
        assert_eq!(yaggun.weapon_list[5].as_deref(), Some("AA3"));
        assert_eq!(yaggun.elite_weapon_list[5].as_deref(), Some("AA3E"));
        let dest = ObjectType::from_ini_section(
            "DEST",
            ini.section("DEST").unwrap(),
            ObjectCategory::Vehicle,
        );
        assert_eq!(dest.naval_targeting, 1);
        assert_eq!(dest.land_targeting, 0);
        assert!(!dest.underwater);
        let bsub = ObjectType::from_ini_section(
            "BSUB",
            ini.section("BSUB").unwrap(),
            ObjectCategory::Vehicle,
        );
        assert_eq!(bsub.land_targeting, 2);
        assert!(bsub.underwater);
        assert!(bsub.unnatural);
        let sqd = ObjectType::from_ini_section(
            "SQD",
            ini.section("SQD").unwrap(),
            ObjectCategory::Vehicle,
        );
        assert!(sqd.organic);
        assert_eq!(sqd.naval_targeting, 3);
        let powr = ObjectType::from_ini_section(
            "GAPOWR",
            ini.section("GAPOWR").unwrap(),
            ObjectCategory::Building,
        );
        assert!(powr.drainable);
        assert!(!powr.overpowerable);
        let tesla = ObjectType::from_ini_section(
            "TESLA",
            ini.section("TESLA").unwrap(),
            ObjectCategory::Building,
        );
        assert!(tesla.overpowerable);
    }

    #[test]
    fn test_parse_infantry_ifv_mode() {
        let ini: IniFile = IniFile::from_str("[E1]\nIFVMode=0\nSize=1\n");
        let section: &IniSection = ini.section("E1").unwrap();
        let obj = ObjectType::from_ini_section("E1", section, ObjectCategory::Infantry);
        assert_eq!(obj.ifv_mode, 0);
        assert_eq!(obj.size, 1);
    }

    #[test]
    fn test_parse_garrison_building() {
        let ini: IniFile =
            IniFile::from_str("[GAPOST]\nCanBeOccupied=yes\nMaxNumberOccupants=10\n");
        let section: &IniSection = ini.section("GAPOST").unwrap();
        let obj = ObjectType::from_ini_section("GAPOST", section, ObjectCategory::Building);
        assert!(obj.can_be_occupied);
        assert_eq!(obj.max_number_occupants, 10);
    }

    #[test]
    fn test_parses_elite_weapon_overrides() {
        let ini: IniFile = IniFile::from_str(
            "[GGI]\nPrimary=M60\nSecondary=MissileLauncher\nElitePrimary=M60E\nEliteSecondary=MissileLauncherE\n",
        );
        let section: &IniSection = ini.section("GGI").unwrap();
        let obj = ObjectType::from_ini_section("GGI", section, ObjectCategory::Infantry);
        assert_eq!(obj.primary(), Some("M60"));
        assert_eq!(obj.secondary(), Some("MissileLauncher"));
        assert_eq!(obj.elite_primary(), Some("M60E"));
        assert_eq!(obj.elite_secondary(), Some("MissileLauncherE"));
    }

    #[test]
    fn test_elite_weapons_default_to_none() {
        let ini: IniFile = IniFile::from_str("[E1]\nPrimary=M60\n");
        let section: &IniSection = ini.section("E1").unwrap();
        let obj = ObjectType::from_ini_section("E1", section, ObjectCategory::Infantry);
        assert_eq!(obj.elite_primary(), None);
        assert_eq!(obj.elite_secondary(), None);
    }

    #[test]
    fn test_parses_open_transport_weapon() {
        let ini: IniFile = IniFile::from_str("[GGI]\nOpenTransportWeapon=1\n");
        let section: &IniSection = ini.section("GGI").unwrap();
        let obj = ObjectType::from_ini_section("GGI", section, ObjectCategory::Infantry);
        assert_eq!(obj.open_transport_weapon, 1);
    }

    #[test]
    fn test_open_transport_weapon_defaults_to_neg_one() {
        let ini: IniFile = IniFile::from_str("[E1]\nPrimary=M60\n");
        let section: &IniSection = ini.section("E1").unwrap();
        let obj = ObjectType::from_ini_section("E1", section, ObjectCategory::Infantry);
        assert_eq!(obj.open_transport_weapon, -1);
    }

    #[test]
    fn test_parse_absorb_fields() {
        let ini: IniFile = IniFile::from_str("[YABRCK]\nInfantryAbsorb=yes\nUnitAbsorb=no\n");
        let section: &IniSection = ini.section("YABRCK").unwrap();
        let obj = ObjectType::from_ini_section("YABRCK", section, ObjectCategory::Building);
        assert!(obj.infantry_absorb);
        assert!(!obj.unit_absorb);
        assert!(!obj.grinding);
        let grinder_ini = IniFile::from_str("[YAGRND]\nGrinding=yes\nUnitAbsorb=no\n");
        let grinder = ObjectType::from_ini_section(
            "YAGRND",
            grinder_ini.section("YAGRND").unwrap(),
            ObjectCategory::Building,
        );
        assert!(grinder.grinding);
        assert!(!grinder.unit_absorb);
    }

    #[test]
    fn test_parse_extra_power_positive() {
        let ini: IniFile =
            IniFile::from_str("[YAPOWR]\nPower=150\nExtraPower=100\nInfantryAbsorb=yes\n");
        let section: &IniSection = ini.section("YAPOWR").unwrap();
        let obj = ObjectType::from_ini_section("YAPOWR", section, ObjectCategory::Building);
        assert_eq!(obj.power, 150);
        assert_eq!(obj.extra_power, 100);
        assert!(obj.infantry_absorb);
    }

    #[test]
    fn test_parse_extra_power_negative() {
        // GAOREP (Allied Ore Processor) in rules.ini has ExtraPower=-9000.
        // Signed parse; downstream gate will suppress the bonus.
        let ini: IniFile = IniFile::from_str("[GAOREP]\nExtraPower=-9000\n");
        let section: &IniSection = ini.section("GAOREP").unwrap();
        let obj = ObjectType::from_ini_section("GAOREP", section, ObjectCategory::Building);
        assert_eq!(obj.extra_power, -9000);
    }

    #[test]
    fn test_parse_extra_power_default_zero() {
        let ini: IniFile = IniFile::from_str("[GAPOWR]\nPower=200\n");
        let section: &IniSection = ini.section("GAPOWR").unwrap();
        let obj = ObjectType::from_ini_section("GAPOWR", section, ObjectCategory::Building);
        assert_eq!(obj.extra_power, 0);
    }

    #[test]
    fn test_size_defaults_by_category() {
        // Infantry defaults to Size=1
        let ini: IniFile = IniFile::from_str("[INF]\nFixtureOnly=1\n");
        let section: &IniSection = ini.section("INF").unwrap();
        let obj = ObjectType::from_ini_section("INF", section, ObjectCategory::Infantry);
        assert_eq!(obj.size, 1);

        // Vehicle defaults to Size=3
        let ini2: IniFile = IniFile::from_str("[VEH]\nFixtureOnly=1\n");
        let section2: &IniSection = ini2.section("VEH").unwrap();
        let obj2 = ObjectType::from_ini_section("VEH", section2, ObjectCategory::Vehicle);
        assert_eq!(obj2.size, 3);
    }

    /// `AbilityClass::FindAbilityByName @ 0x0074FEFF` resolves every one of
    /// the 18 native tokens to its table index; the parser (`FUN_00477640`)
    /// sets one byte per recognised token, skips unknown ones, folds ASCII
    /// case, and leaves the prior array alone when the key is absent.
    #[test]
    fn gsi_08_12_ability_lists_parse_the_full_native_token_table() {
        // The whole table is 163 bytes; the 0x80 ReadString copy keeps 127,
        // ending after FEARLESS, so the last four tokens go in a second list.
        let all = Ability::NAMES.join(",");
        assert_eq!(all[..127].rsplit(',').next(), Some("FEARLESS"));
        let tail = Ability::NAMES[14..].join(",");
        let ini = IniFile::from_str(&format!(
            "[X]\nVeteranAbilities={all}\nEliteAbilities=self_heal,BOGUS,Rof\n\
             [T]\nVeteranAbilities={tail}\n"
        ));
        let obj =
            ObjectType::from_ini_section("X", ini.section("X").unwrap(), ObjectCategory::Vehicle);
        let tail_obj =
            ObjectType::from_ini_section("T", ini.section("T").unwrap(), ObjectCategory::Vehicle);
        for (index, name) in Ability::NAMES.iter().enumerate() {
            let ability = Ability::from_ini_token(name).expect("native token");
            assert_eq!(
                ability as usize, index,
                "{name} sits at native index {index}"
            );
            assert_eq!(obj.veteran_abilities.has(ability), index < 14, "{name} cut");
            if index >= 14 {
                assert!(tail_obj.veteran_abilities.has(ability), "{name} parsed");
            }
        }
        assert!(obj.elite_abilities.has(Ability::SelfHeal));
        assert!(obj.elite_abilities.has(Ability::Rof));
        assert!(!obj.elite_abilities.has(Ability::Faster));
        assert_eq!(Ability::from_ini_token("BOGUS"), None);

        let absent = IniFile::from_str("[Y]\nFixtureOnly=1\n");
        let obj = ObjectType::from_ini_section(
            "Y",
            absent.section("Y").unwrap(),
            ObjectCategory::Vehicle,
        );
        assert_eq!(obj.veteran_abilities, AbilityFlags::default());
        assert!(!obj.self_healing, "SelfHealing= defaults off");
        let prior = AbilityFlags::from_abilities(&[Ability::Sight]);
        assert_eq!(
            AbilityFlags::parse(None, prior),
            prior,
            "absent key keeps the prior array"
        );
        assert_eq!(
            AbilityFlags::parse(Some(vec!["ROF"]), prior),
            AbilityFlags::from_abilities(&[Ability::Rof]),
            "a present key replaces the whole array"
        );

        let healing = IniFile::from_str("[Z]\nSelfHealing=yes\n");
        let obj = ObjectType::from_ini_section(
            "Z",
            healing.section("Z").unwrap(),
            ObjectCategory::Vehicle,
        );
        assert!(obj.self_healing);
    }

    #[test]
    fn c4_flag_parses_from_ini() {
        let ini = IniFile::from_str("[GHOST]\nC4=yes\n");
        let section = ini.section("GHOST").unwrap();
        let obj = ObjectType::from_ini_section("GHOST", section, ObjectCategory::Infantry);
        assert!(obj.c4);
    }

    #[test]
    fn c4_defaults_to_false() {
        let ini = IniFile::from_str("[E1]\nFixtureOnly=1\n");
        let section = ini.section("E1").unwrap();
        let obj = ObjectType::from_ini_section("E1", section, ObjectCategory::Infantry);
        assert!(!obj.c4);
    }

    #[test]
    fn can_c4_defaults_to_true_for_buildings() {
        let ini = IniFile::from_str("[GAPILE]\nFixtureOnly=1\n");
        let section = ini.section("GAPILE").unwrap();
        let obj = ObjectType::from_ini_section("GAPILE", section, ObjectCategory::Building);
        assert!(obj.can_c4);
    }

    #[test]
    fn can_c4_defaults_to_false_for_non_buildings() {
        let ini = IniFile::from_str("[E1]\nFixtureOnly=1\n");
        let section = ini.section("E1").unwrap();
        let obj = ObjectType::from_ini_section("E1", section, ObjectCategory::Infantry);
        assert!(!obj.can_c4);
    }

    #[test]
    fn can_c4_no_overrides_default() {
        let ini = IniFile::from_str("[CAMISC01]\nCanC4=no\n");
        let section = ini.section("CAMISC01").unwrap();
        let obj = ObjectType::from_ini_section("CAMISC01", section, ObjectCategory::Building);
        assert!(!obj.can_c4);
    }

    #[test]
    fn bunker_flag_parses_from_ini() {
        let ini = IniFile::from_str("[NATBNK]\nBunker=yes\nNumberImpassableRows=0\n");
        let section = ini.section("NATBNK").unwrap();
        let obj = ObjectType::from_ini_section("NATBNK", section, ObjectCategory::Building);
        assert!(obj.bunker);
        assert_eq!(obj.number_impassable_rows, 0);
    }

    #[test]
    fn gate_flag_parses_from_ini() {
        let ini = IniFile::from_str(
            "[GAGATE_A]\nGate=yes\nFoundation=3x1\nDeployTime=.044\nGateCloseDelay=.2\n",
        );
        let section = ini.section("GAGATE_A").unwrap();
        let obj = ObjectType::from_ini_section("GAGATE_A", section, ObjectCategory::Building);
        assert!(obj.gate);
        assert_eq!(obj.deploy_time_ticks, 39);
        assert_eq!(obj.gate_close_delay_ticks, 180);
    }

    #[test]
    fn bunkerable_defaults_true_for_vehicles_only() {
        let ini = IniFile::from_str("[TEST]\nFixtureOnly=1\n");
        let section = ini.section("TEST").unwrap();

        let vehicle = ObjectType::from_ini_section("TEST", section, ObjectCategory::Vehicle);
        let infantry = ObjectType::from_ini_section("TEST", section, ObjectCategory::Infantry);
        let aircraft = ObjectType::from_ini_section("TEST", section, ObjectCategory::Aircraft);
        let building = ObjectType::from_ini_section("TEST", section, ObjectCategory::Building);

        assert!(vehicle.bunkerable);
        assert!(!infantry.bunkerable);
        assert!(!aircraft.bunkerable);
        assert!(!building.bunkerable);
    }

    #[test]
    fn bunkerable_ini_overrides_vehicle_default() {
        let ini = IniFile::from_str("[HTNK]\nBunkerable=no\n");
        let section = ini.section("HTNK").unwrap();
        let obj = ObjectType::from_ini_section("HTNK", section, ObjectCategory::Vehicle);
        assert!(!obj.bunkerable);
    }

    #[test]
    fn invisible_in_game_defaults_to_false() {
        let ini = IniFile::from_str("[GAPILE]\nFixtureOnly=1\n");
        let section = ini.section("GAPILE").unwrap();
        let obj = ObjectType::from_ini_section("GAPILE", section, ObjectCategory::Building);
        assert!(!obj.invisible);
        assert!(!obj.invisible_in_game);
    }

    #[test]
    fn invisible_and_invisible_in_game_parse_independently() {
        let ini = IniFile::from_str(
            "[BRIDGEA]\nInvisible=yes\nInvisibleInGame=no\n\
             [BRIDGEB]\nInvisible=no\nInvisibleInGame=yes\n",
        );

        let plain = ObjectType::from_ini_section(
            "BRIDGEA",
            ini.section("BRIDGEA").unwrap(),
            ObjectCategory::Building,
        );
        let in_game = ObjectType::from_ini_section(
            "BRIDGEB",
            ini.section("BRIDGEB").unwrap(),
            ObjectCategory::Building,
        );

        assert!(plain.invisible);
        assert!(!plain.invisible_in_game);
        assert!(!in_game.invisible);
        assert!(in_game.invisible_in_game);
    }

    #[test]
    fn radar_insignificant_parses_independently_from_radar_visible() {
        let ini = IniFile::from_str(
            "[A]\nInsignificant=yes\nRadarVisible=no\n\
             [B]\nInsignificant=no\nRadarVisible=yes\n",
        );
        let a =
            ObjectType::from_ini_section("A", ini.section("A").unwrap(), ObjectCategory::Vehicle);
        let b =
            ObjectType::from_ini_section("B", ini.section("B").unwrap(), ObjectCategory::Vehicle);
        assert!(a.insignificant);
        assert!(!a.radar_visible);
        assert!(!b.insignificant);
        assert!(b.radar_visible);
    }

    #[test]
    fn gsi_04_05_to_protect_is_an_independent_false_default_type_gate() {
        let ini = IniFile::from_str("[PLAIN]\nStrength=100\n[GUARDED]\nToProtect=yes\n");
        let plain = ObjectType::from_ini_section(
            "PLAIN",
            ini.section("PLAIN").unwrap(),
            ObjectCategory::Vehicle,
        );
        let guarded = ObjectType::from_ini_section(
            "GUARDED",
            ini.section("GUARDED").unwrap(),
            ObjectCategory::Vehicle,
        );
        assert!(!plain.to_protect);
        assert!(guarded.to_protect);
    }

    #[test]
    fn radar_invisible_veterancy_abilities_parse_by_rank() {
        let ini = IniFile::from_str(
            "[DOT]\nVeteranAbilities=FASTER,RADAR_INVISIBLE\n\
             EliteAbilities=STRONGER,RADAR_INVISIBLE\n",
        );
        let obj = ObjectType::from_ini_section(
            "DOT",
            ini.section("DOT").unwrap(),
            ObjectCategory::Vehicle,
        );
        assert!(obj.veteran_abilities.has(Ability::RadarInvisible));
        assert!(obj.elite_abilities.has(Ability::RadarInvisible));
        assert!(!obj.radar_invisible);
    }

    #[test]
    fn techno_type_particle_fields_default_to_empty() {
        let ini: IniFile = IniFile::from_str("[E1]\nFixtureOnly=1\n");
        let section = ini.section("E1").unwrap();
        let obj = ObjectType::from_ini_section("E1", section, ObjectCategory::Infantry);

        assert_eq!(obj.natural_particle_system, None);
        assert_eq!(obj.natural_particle_location, IVec3::ZERO);
        assert_eq!(obj.refinery_smoke_particle_system, None);
        assert!(obj.damage_particle_systems.is_empty());
        assert!(obj.destroy_particle_systems.is_empty());
        assert_eq!(obj.damage_smoke_offset, IVec3::ZERO);
        assert!(!obj.dam_smk_off_scrn_rel);
        assert_eq!(obj.destroy_smoke_offset, IVec3::ZERO);
        assert_eq!(obj.refinery_smoke_offsets, [IVec3::ZERO; 4]);
        assert_eq!(obj.gap_radius_in_cells, 0);
        assert_eq!(obj.super_gap_radius_in_cells, 0);
    }

    #[test]
    fn gsi_05_14_debris_keys_are_case_exact_so_the_retail_lowercase_spelling_is_unread() {
        // A death spends a count drawn from [MinDebris, MaxDebris] on
        // DebrisTypes VoxelAnims and DebrisAnims SHP anims.
        let ini: IniFile = IniFile::from_str(
            "[GAPOWR]\n\
             MinDebris=4\nMaxDebris=9\n\
             DebrisTypes=TIRE\nDebrisMaximums=3\n\
             DebrisAnims=DBRIS1LG,DBRIS1SM,DBRIS4LG\n",
        );
        let obj = ObjectType::from_ini_section(
            "GAPOWR",
            ini.section("GAPOWR").unwrap(),
            ObjectCategory::Building,
        );
        assert_eq!((obj.min_debris, obj.max_debris), (4, 9));
        assert_eq!(obj.debris_types, vec!["TIRE".to_string()]);
        assert_eq!(obj.debris_maximums, vec![3]);
        assert_eq!(obj.debris_anims.len(), 3);

        // The lowercase spelling must NOT be read. gamemd's `INIClass` hashes
        // the raw bytes of the key on both the store side
        // (`INIClass::LoadFromStraw @ 0x005260C9`) and the lookup side
        // (`CCINIClass::ReadInt @ 0x00527715`), and both binary searches
        // compare 32-bit CRCs only — there is no folding instruction anywhere
        // on the path, and `strtrim @ 0x00727CF0` strips whitespace and
        // nothing else. `0x0084439C` is the literal `MaxDebris`, so the 17
        // stock `[VehicleTypes]` that spell it `Maxdebris=3` are invisible to
        // the read and keep the `TechnoTypeClass::Constructor 0x00710FB7`
        // default of 0. That zero closes the whole debris block at
        // `TechnoClass::ReceiveDamage 0x00702293 JLE`, so the Rhino, the
        // Apocalypse, the Prism Tank, the Battle Fortress, the IFV and 9 more
        // buildable vehicles throw nothing on death in retail and take no RNG
        // draw for it. Reading them would be the divergence, not the fix.
        let ini: IniFile = IniFile::from_str("[NAPOWR]\nMaxdebris=7\nMindebris=2\n");
        let obj = ObjectType::from_ini_section(
            "NAPOWR",
            ini.section("NAPOWR").unwrap(),
            ObjectCategory::Building,
        );
        assert_eq!((obj.min_debris, obj.max_debris), (0, 0));

        // The same rule on the CSV siblings: no stock section mis-spells any
        // of these three, so this arm is wrong-in-principle only today, but it
        // goes live the moment a mod mis-spells one.
        let ini: IniFile = IniFile::from_str(
            "[NAPOWR]\nMaxDebris=5\n\
             debristypes=TIRE\nDEBRISMAXIMUMS=3\nDebrisanims=DBRIS1LG\n",
        );
        let obj = ObjectType::from_ini_section(
            "NAPOWR",
            ini.section("NAPOWR").unwrap(),
            ObjectCategory::Building,
        );
        assert_eq!(obj.max_debris, 5);
        assert!(obj.debris_types.is_empty());
        assert!(obj.debris_maximums.is_empty());
        assert!(obj.debris_anims.is_empty());
    }

    /// `TechnoTypeClass::ReadINI @ 0x007125BD` floors `MinDebris` at 0, then
    /// `0x007125D9` raises `MaxDebris` to meet it. The order matters: flooring
    /// first is what stops a negative `MinDebris` from dragging `MaxDebris`
    /// below zero and re-opening the block that `MaxDebris <= 0` closes.
    ///
    /// Stock-unreachable — 0 sections author a negative `MinDebris` and 0
    /// author an inverted span — so this is modded-rules territory, but it is
    /// what makes `throw_death_debris`'s inverted-span branch well-defined.
    #[test]
    fn gsi_05_14_mindebris_floors_at_zero_then_raises_maxdebris() {
        let ini: IniFile = IniFile::from_str("[NAPOWR]\nMaxDebris=2\nMinDebris=-9\n");
        let obj = ObjectType::from_ini_section(
            "NAPOWR",
            ini.section("NAPOWR").unwrap(),
            ObjectCategory::Building,
        );
        // MinDebris floored to 0 first, so MaxDebris is left where it was.
        assert_eq!((obj.min_debris, obj.max_debris), (0, 2));

        let ini: IniFile = IniFile::from_str("[NAPOWR]\nMaxDebris=2\nMinDebris=7\n");
        let obj = ObjectType::from_ini_section(
            "NAPOWR",
            ini.section("NAPOWR").unwrap(),
            ObjectCategory::Building,
        );
        // Inverted span: MaxDebris rises to meet MinDebris.
        assert_eq!((obj.min_debris, obj.max_debris), (7, 7));

        let ini: IniFile = IniFile::from_str("[NAPOWR]\nMaxDebris=-4\nMinDebris=-9\n");
        let obj = ObjectType::from_ini_section(
            "NAPOWR",
            ini.section("NAPOWR").unwrap(),
            ObjectCategory::Building,
        );
        // A negative MinDebris must not drag MaxDebris up out of its own
        // negative: the floor lands on 0, and MaxDebris rises only to 0.
        assert_eq!((obj.min_debris, obj.max_debris), (0, 0));
    }
    /// `0x00713C37`: ReadString 0x80, then each `strtok` token as written to
    /// `ParticleSystemTypeClass::FindOrAllocate` (`0x00644890`).
    #[test]
    fn techno_type_parses_damage_particle_systems_csv() {
        let ini: IniFile = IniFile::from_str(
            "[GAPOWR]\n\
             DamageParticleSystems=BigGreySSys, SmallGreySSys ,SparkSys\n\
             DestroyParticleSystems=DebrisSmokeSys\n",
        );
        let section = ini.section("GAPOWR").unwrap();
        let obj = ObjectType::from_ini_section("GAPOWR", section, ObjectCategory::Building);

        assert_eq!(
            obj.damage_particle_systems,
            vec![
                "BigGreySSys".to_string(),
                " SmallGreySSys ".to_string(),
                "SparkSys".to_string(),
            ]
        );
        assert_eq!(
            obj.destroy_particle_systems,
            vec!["DebrisSmokeSys".to_string()]
        );
    }

    #[test]
    fn emits_damage_spark_only_for_cyborg_infantry() {
        // Type+0xC8F = Cyborg, written ONLY by InfantryTypeClass::ReadINI; other
        // leaves keep the ctor default 0. So `emits_damage_spark` is true iff the
        // type is `Cyborg=yes` AND infantry.
        let ini = IniFile::from_str("[X]\nCyborg=yes\n");
        let s = ini.section("X").unwrap();
        assert!(
            ObjectType::from_ini_section("X", s, ObjectCategory::Infantry).emits_damage_spark()
        );
        // A Cyborg=yes vehicle/building/aircraft never emits (category gate).
        assert!(
            !ObjectType::from_ini_section("X", s, ObjectCategory::Vehicle).emits_damage_spark()
        );
        assert!(
            !ObjectType::from_ini_section("X", s, ObjectCategory::Building).emits_damage_spark()
        );
        assert!(
            !ObjectType::from_ini_section("X", s, ObjectCategory::Aircraft).emits_damage_spark()
        );
        // Default (no Cyborg key) → false even for infantry (dormant in stock YR).
        let plain = IniFile::from_str("[E1]\nFixtureOnly=1\n");
        let ps = plain.section("E1").unwrap();
        let plain_inf = ObjectType::from_ini_section("E1", ps, ObjectCategory::Infantry);
        assert!(!plain_inf.cyborg);
        assert!(!plain_inf.emits_damage_spark());
    }

    #[test]
    fn techno_type_opportunity_fire_and_can_retaliate_defaults() {
        // Absent keys: OpportunityFire defaults off, CanRetaliate defaults on
        // (the gamemd TechnoType defaults — passive acquire opt-in, retaliate
        // opt-out).
        let ini: IniFile = IniFile::from_str("[E1]\nFixtureOnly=1\n");
        let section = ini.section("E1").unwrap();
        let obj = ObjectType::from_ini_section("E1", section, ObjectCategory::Infantry);
        assert!(!obj.opportunity_fire, "OpportunityFire defaults to no");
        assert!(obj.can_retaliate, "CanRetaliate defaults to yes");
    }

    #[test]
    fn techno_type_parses_opportunity_fire_and_can_retaliate() {
        let ini: IniFile = IniFile::from_str(
            "[MTNK]\n\
             OpportunityFire=yes\n\
             CanRetaliate=no\n",
        );
        let section = ini.section("MTNK").unwrap();
        let obj = ObjectType::from_ini_section("MTNK", section, ObjectCategory::Vehicle);
        assert!(obj.opportunity_fire, "OpportunityFire=yes parses true");
        assert!(!obj.can_retaliate, "CanRetaliate=no parses false");
    }

    #[test]
    fn techno_type_can_passive_acquire_defaults_yes_and_parses_the_misspelled_key() {
        // Absent key → yes (the gamemd TechnoType constructor default). The INI
        // spelling is "CanPassiveAquire"; the correctly-spelled variant is NOT a
        // key the original reads, so it must not turn the flag off.
        let plain = IniFile::from_str("[E1]\nFixtureOnly=1\n");
        let obj = ObjectType::from_ini_section(
            "E1",
            plain.section("E1").unwrap(),
            ObjectCategory::Infantry,
        );
        assert!(obj.can_passive_acquire, "CanPassiveAquire defaults to yes");

        let opted_out = IniFile::from_str("[DESO]\nCanPassiveAquire=no\n");
        let obj = ObjectType::from_ini_section(
            "DESO",
            opted_out.section("DESO").unwrap(),
            ObjectCategory::Infantry,
        );
        assert!(!obj.can_passive_acquire, "CanPassiveAquire=no parses false");

        let misspelled_the_other_way = IniFile::from_str("[DESO]\nCanPassiveAcquire=no\n");
        let obj = ObjectType::from_ini_section(
            "DESO",
            misspelled_the_other_way.section("DESO").unwrap(),
            ObjectCategory::Infantry,
        );
        assert!(
            obj.can_passive_acquire,
            "only the binary's misspelled key is read"
        );
    }

    #[test]
    fn techno_type_distributed_fire_defaults_no_and_parses() {
        let plain = IniFile::from_str("[MTNK]\nFixtureOnly=1\n");
        let obj = ObjectType::from_ini_section(
            "MTNK",
            plain.section("MTNK").unwrap(),
            ObjectCategory::Vehicle,
        );
        assert!(!obj.distributed_fire, "DistributedFire defaults to no");

        let aegis = IniFile::from_str("[AEGIS]\nDistributedFire=yes\n");
        let obj = ObjectType::from_ini_section(
            "AEGIS",
            aegis.section("AEGIS").unwrap(),
            ObjectCategory::Vehicle,
        );
        assert!(obj.distributed_fire, "DistributedFire=yes parses true");
    }

    #[test]
    fn techno_type_parses_refinery_smoke_offsets() {
        let ini: IniFile = IniFile::from_str(
            "[GAREFN]\n\
             RefinerySmokeParticleSystem=BigGreySSys\n\
             RefinerySmokeOffsetOne=10,20,30\n\
             RefinerySmokeOffsetTwo=-5,0,15\n\
             RefinerySmokeOffsetThree=0,0,0\n\
             RefinerySmokeOffsetFour=100,-50,200\n",
        );
        let section = ini.section("GAREFN").unwrap();
        let obj = ObjectType::from_ini_section("GAREFN", section, ObjectCategory::Building);

        assert_eq!(
            obj.refinery_smoke_particle_system.as_deref(),
            Some("BigGreySSys")
        );
        assert_eq!(obj.refinery_smoke_offsets[0], IVec3::new(10, 20, 30));
        assert_eq!(obj.refinery_smoke_offsets[1], IVec3::new(-5, 0, 15));
        assert_eq!(obj.refinery_smoke_offsets[2], IVec3::ZERO);
        assert_eq!(obj.refinery_smoke_offsets[3], IVec3::new(100, -50, 200));
    }

    #[test]
    fn techno_type_parses_refinery_smoke_frames() {
        let ini: IniFile = IniFile::from_str("[FOO]\nRefinerySmokeFrames=50\n");
        let section = ini.section("FOO").expect("section");
        let obj = ObjectType::from_ini_section("FOO", section, ObjectCategory::Building);
        assert_eq!(obj.refinery_smoke_frames, 50);
    }

    #[test]
    fn techno_type_refinery_smoke_frames_defaults_to_native_25() {
        let ini: IniFile = IniFile::from_str("[FOO]\nFixtureOnly=1\n");
        let section = ini.section("FOO").expect("section");
        let obj = ObjectType::from_ini_section("FOO", section, ObjectCategory::Building);
        assert_eq!(obj.refinery_smoke_frames, 25);
    }

    #[test]
    fn original_eight_refinery_smoke_frames_scalar_rows() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/refinery_smoke.json",
        ))
        .unwrap();
        let rows = corpus["frames_parser"].as_array().unwrap();
        assert_eq!(rows.len(), 8);
        for row in rows {
            // Feed the original ReadInt boundary directly. A physical empty
            // INI value is removed earlier by the separate lexical loader.
            let mut section = IniSection::new("B".to_owned());
            if let Some(raw) = row["raw"].as_str() {
                section.set("RefinerySmokeFrames", raw);
            }
            let object = ObjectType::from_ini_section("B", &section, ObjectCategory::Building);
            assert_eq!(
                object.refinery_smoke_frames,
                row["output"].as_i64().unwrap() as i32,
                "{row}"
            );
        }
    }

    #[test]
    fn techno_type_parses_natural_and_smoke_keys() {
        let ini: IniFile = IniFile::from_str(
            "[GAGAP]\n\
             NaturalParticleSystem=GapPSys\n\
             NaturalParticleLocation=0,0,256\n\
             DamageSmokeOffset=12,-12,48\n\
             DamSmkOffScrnRel=yes\n\
             DestroySmokeOffset=0,0,128\n\
             GapRadiusInCells=8\n\
             SuperGapRadiusInCells=12\n\
             PsychicDetectionRadius=15\n\
             SensorArray=yes\n\
             Sensors=yes\n\
             SensorsSight=14\n\
             CloakGenerator=yes\n",
        );
        let section = ini.section("GAGAP").unwrap();
        let obj = ObjectType::from_ini_section("GAGAP", section, ObjectCategory::Building);

        assert_eq!(obj.natural_particle_system.as_deref(), Some("GapPSys"));
        assert_eq!(obj.natural_particle_location, IVec3::new(0, 0, 256));
        assert_eq!(obj.damage_smoke_offset, IVec3::new(12, -12, 48));
        assert!(obj.dam_smk_off_scrn_rel);
        assert_eq!(obj.destroy_smoke_offset, IVec3::new(0, 0, 128));
        assert_eq!(obj.gap_radius_in_cells, 8);
        assert_eq!(obj.super_gap_radius_in_cells, 12);
        assert_eq!(obj.psychic_detection_radius, 15);
        assert!(obj.sensor_array);
        assert!(obj.sensors);
        assert_eq!(obj.sensors_sight, 14);
        assert!(obj.cloak_generator);
    }

    #[test]
    fn no_shadow_reader_retains_native_prior_value_across_rules_layers() {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        use crate::rules::ruleset::RuleSet;

        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[VehicleTypes]\n0=GENERATED\n[GENERATED]\nNoShadow=yes\n",
        ));
        // ReadBool's original default branch preserves the prior byte for
        // invalid values; physical INI loading omits authored empty values.
        for (kind, patch, expected) in [
            (RulesLayerKind::LangRule, "NoShadow=off", true),
            (RulesLayerKind::GameMode, "NoShadow=", true),
            (RulesLayerKind::Scenario, "NoShadow=no", false),
            (RulesLayerKind::Scenario, "noshadow=yes", false),
        ] {
            layers.push(kind, IniFile::from_str(&format!("[GENERATED]\n{patch}\n")));
            let rules = RuleSet::from_rules_layers(&layers).unwrap();
            assert_eq!(
                rules.object("GENERATED").unwrap().no_shadow,
                expected,
                "{patch}"
            );
        }
        let defaults = IniFile::from_str("[DEFAULTS]\nFixtureOnly=yes\n");
        assert!(
            !ObjectType::from_ini_section(
                "DEFAULTS",
                defaults.section("DEFAULTS").unwrap(),
                ObjectCategory::Vehicle,
            )
            .no_shadow
        );
    }

    #[test]
    fn retail_cloaked_naval_and_vehicle_shadow_type_inputs() {
        use crate::rules::art_data::ArtRegistry;
        use crate::rules::locomotor_type::LocomotorKind;
        use crate::rules::ruleset::RuleSet;

        let Some((ini, art_ini)) = crate::rules::retail_ini_fixture::retail_rules_and_art() else {
            return;
        };
        let mut rules = RuleSet::from_ini(&ini).unwrap();
        rules.install_art_data(ArtRegistry::from_ini(&art_ini));
        for (id, voxel) in [("SUB", true), ("DLPH", false)] {
            let object = rules.object(id).unwrap();
            assert!(!object.no_shadow, "{id}");
            assert!(object.cloakable, "{id}");
            assert_eq!(object.locomotor, LocomotorKind::Ship, "{id}");
            assert!(!object.has_turret, "{id}");
            assert_eq!(object.turret_count, 0, "{id}");
            let art = rules
                .art()
                .resolve_metadata_entry(id, &object.image)
                .expect("retail naval body art");
            assert_eq!(art.voxel, voxel, "{id}: native VXL/SHP caller distinction");
        }
        assert!(!rules.object("MTNK").unwrap().no_shadow);
        // Positive retail controls prevent a constructor-default-only port.
        for id in ["DNOA", "DNOB"] {
            assert!(rules.object(id).unwrap().no_shadow, "{id}");
        }
    }

    #[test]
    fn stock_cloak_and_sensor_type_inputs_parse_without_inference() {
        let ini = IniFile::from_str(
            "[DLPH]\nCloakable=yes\nCloakingSpeed=1\nSensorsSight=8\n\
             [SUB]\nCloakable=yes\nCloakingSpeed=1\nSensorsSight=7\n\
             [SQD]\nCloakable=yes\nCloakingSpeed=5\nSensorsSight=8\n\
             [BSUB]\nCloakable=yes\nCloakingSpeed=1\nSensorsSight=8\n\
             [NAPSIS]\nSensorArray=yes\nSensorsSight=15\n\
             [RANKED]\nVeteranAbilities=CLOAK\nEliteAbilities=STRONGER,CLOAK\n\
             [DEFAULTS]\nFixtureOnly=yes\n",
        );
        for (id, speed, sight) in [("DLPH", 1, 8), ("SUB", 1, 7), ("SQD", 5, 8), ("BSUB", 1, 8)] {
            let object = ObjectType::from_ini_section(
                id,
                ini.section(id).expect("stock-shaped cloak section"),
                ObjectCategory::Vehicle,
            );
            assert!(object.cloakable, "{id}");
            assert_eq!(object.cloaking_speed, speed, "{id}");
            assert_eq!(object.sensors_sight, sight, "{id}");
        }
        let napsis = ObjectType::from_ini_section(
            "NAPSIS",
            ini.section("NAPSIS").unwrap(),
            ObjectCategory::Building,
        );
        assert!(napsis.sensor_array);
        assert_eq!(napsis.sensors_sight, 15);
        assert_eq!(napsis.cloak_radius_in_cells, 20);
        let ranked = ObjectType::from_ini_section(
            "RANKED",
            ini.section("RANKED").unwrap(),
            ObjectCategory::Vehicle,
        );
        assert!(ranked.veteran_abilities.has(Ability::Cloak));
        assert!(ranked.elite_abilities.has(Ability::Cloak));
        let defaults = ObjectType::from_ini_section(
            "DEFAULTS",
            ini.section("DEFAULTS").unwrap(),
            ObjectCategory::Vehicle,
        );
        assert!(!defaults.cloakable && !defaults.cloak_stop);
        assert_eq!(defaults.cloaking_speed, 1);
        assert_eq!(defaults.cloak_radius_in_cells, 20);
    }

    /// ReadSoundList keeps `strtok` tokens as written; empty fields drop.
    #[test]
    fn parses_ordered_death_sound_lists() {
        let ini =
            IniFile::from_str("[E1]\nVoiceDie= VoiceA,VoiceB, VoiceC \nDieSound= DieA,,DieB\n");
        let object = ObjectType::from_ini_section(
            "E1",
            ini.section("E1").unwrap(),
            ObjectCategory::Infantry,
        );

        assert_eq!(object.voice_die, ["VoiceA", "VoiceB", " VoiceC"]);
        assert_eq!(object.die_sounds, ["DieA", "DieB"]);
    }

    #[test]
    fn parses_infantry_fear_flags_and_fearless_abilities() {
        let ini = IniFile::from_str(
            "[E1]\nFearless=yes\nFraidycat=yes\nVeteranAbilities=FEARLESS\nEliteAbilities=SELF_HEAL,FEARLESS\n",
        );
        let section = ini.section("E1").unwrap();
        let obj = ObjectType::from_ini_section("E1", section, ObjectCategory::Infantry);
        assert!(obj.fearless);
        assert!(obj.fraidycat);
        assert!(!obj.crawls);
        assert!(obj.veteran_abilities.has(Ability::Fearless));
        assert!(obj.elite_abilities.has(Ability::Fearless));
    }

    #[test]
    fn crushable_defaults_yes_for_infantry_and_no_for_everything_else() {
        // The InfantryTypeClass constructor overwrites the ObjectTypeClass
        // default with 1, so a key-less infantry section (Conscript, Engineer,
        // Flak Trooper, ... in stock rulesmd) is crushable.
        let ini = IniFile::from_str("[E2]\nName=Conscript\n");
        let section = ini.section("E2").unwrap();
        assert!(ObjectType::from_ini_section("E2", section, ObjectCategory::Infantry).crushable);

        let ini = IniFile::from_str("[MTNK]\nSpeed=7\n");
        let section = ini.section("MTNK").unwrap();
        assert!(!ObjectType::from_ini_section("MTNK", section, ObjectCategory::Vehicle).crushable);

        // An explicit key still wins in both directions.
        let ini = IniFile::from_str("[DESO]\nCrushable=no\n");
        let section = ini.section("DESO").unwrap();
        assert!(!ObjectType::from_ini_section("DESO", section, ObjectCategory::Infantry).crushable);

        let ini = IniFile::from_str("[GASAND]\nCrushable=yes\n");
        let section = ini.section("GASAND").unwrap();
        assert!(
            ObjectType::from_ini_section("GASAND", section, ObjectCategory::Building).crushable
        );
    }

    #[test]
    fn parses_deployed_crushable_with_default_yes() {
        let ini = IniFile::from_str("[E1]\nCrushable=yes\n");
        let section = ini.section("E1").unwrap();
        let obj = ObjectType::from_ini_section("E1", section, ObjectCategory::Infantry);
        assert!(obj.deployed_crushable);

        let ini = IniFile::from_str("[GGI]\nCrushable=yes\nDeployedCrushable=no\n");
        let section = ini.section("GGI").unwrap();
        let obj = ObjectType::from_ini_section("GGI", section, ObjectCategory::Infantry);
        assert!(!obj.deployed_crushable);
    }

    #[test]
    fn object_type_parses_regular_crusher_for_amcv_fixture() {
        let ini = IniFile::from_str("[AMCV]\nSpeed=4\nMovementZone=Normal\nCrusher=yes\n");
        let section = ini.section("AMCV").unwrap();
        let obj = ObjectType::from_ini_section("AMCV", section, ObjectCategory::Vehicle);
        assert!(obj.crusher);
    }

    #[test]
    fn object_type_crusher_defaults_false() {
        let ini = IniFile::from_str("[MTNK]\nSpeed=7\nMovementZone=Normal\n");
        let section = ini.section("MTNK").unwrap();
        let obj = ObjectType::from_ini_section("MTNK", section, ObjectCategory::Vehicle);
        assert!(!obj.crusher);
    }

    #[test]
    fn object_type_parses_accelerates_false() {
        let ini = IniFile::from_str("[MTNK]\nSpeed=7\nAccelerates=false\n");
        let section = ini.section("MTNK").unwrap();
        let obj = ObjectType::from_ini_section("MTNK", section, ObjectCategory::Vehicle);
        assert!(!obj.accelerates);
    }

    #[test]
    fn object_type_accelerates_defaults_true() {
        let ini = IniFile::from_str("[AMCV]\nSpeed=4\n");
        let section = ini.section("AMCV").unwrap();
        let obj = ObjectType::from_ini_section("AMCV", section, ObjectCategory::Vehicle);
        assert!(obj.accelerates);
    }

    #[test]
    fn parses_air_range_bonus() {
        let ini: IniFile =
            IniFile::from_str("[MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nAirRangeBonus=4\n");
        let section = ini.section("MTNK").expect("section");
        let obj = ObjectType::from_ini_section("MTNK", section, ObjectCategory::Vehicle);
        assert_eq!(obj.air_range_bonus, Some(sim_from_f32(4.0)));
    }

    #[test]
    fn air_range_bonus_default_none() {
        let ini: IniFile = IniFile::from_str("[MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\n");
        let section = ini.section("MTNK").expect("section");
        let obj = ObjectType::from_ini_section("MTNK", section, ObjectCategory::Vehicle);
        assert_eq!(obj.air_range_bonus, None);
    }

    #[test]
    fn parse_weight_default_two() {
        let ini: IniFile = IniFile::from_str("[MTNK]\nStrength=300\n");
        let section = ini.section("MTNK").expect("section");
        let obj = ObjectType::from_ini_section("MTNK", section, ObjectCategory::Vehicle);
        assert_eq!(obj.weight, SimFixed::lit("2.0"));
    }

    #[test]
    fn parse_weight_custom() {
        let ini: IniFile = IniFile::from_str("[HTNK]\nWeight=3.5\n");
        let section = ini.section("HTNK").expect("section");
        let obj = ObjectType::from_ini_section("HTNK", section, ObjectCategory::Vehicle);
        assert_eq!(obj.weight, SimFixed::lit("3.5"));
    }

    #[test]
    fn parse_retail_weight_apocalypse_is_three_point_five() {
        // Retail rulesmd.ini: APOC (Apocalypse Tank) has Weight=3.5.
        // The heaviest unit in stock retail is CARRIER (Aircraft Carrier) at Weight=5.
        let ini = IniFile::from_str("[APOC]\nWeight=3.5\n");
        let apoc = ObjectType::from_ini_section(
            "APOC",
            ini.section("APOC").expect("APOC section"),
            ObjectCategory::Vehicle,
        );
        assert_eq!(apoc.weight, SimFixed::lit("3.5"));
    }

    #[test]
    fn parse_retail_weight_grizzly_defaults_to_two() {
        // Retail rulesmd.ini: MTNK (Grizzly) has no Weight= line, so it should
        // fall back to the engine default 2.0.
        let ini = IniFile::from_str("[MTNK]\nStrength=300\n");
        let mtnk = ObjectType::from_ini_section(
            "MTNK",
            ini.section("MTNK").expect("MTNK section"),
            ObjectCategory::Vehicle,
        );
        assert_eq!(mtnk.weight, SimFixed::lit("2.0"));
    }

    /// Retail `rulesmd.ini`, read from the gitignored `ini/` corpus.
    ///
    /// The golden is retail INI bytes, not a hand-written fixture, so these are
    /// parity checks on the authored data rather than Rust-vs-Rust. `None` (the
    /// test skips) when `ini/` is absent; see `retail_ini_fixture`.
    #[cfg(test)]
    fn retail_rules_ini() -> Option<IniFile> {
        crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini")
    }

    /// Retail `Trainable=` through the production reader: a BuildingType is
    /// untrainable unless its section says so (constructor `0x0045E42E`), so a
    /// garrisonable building and a Tank Bunker never rank while the Yuri
    /// refinery (the one retail `Trainable=yes` building) and ordinary units
    /// do.
    #[test]
    fn retail_trainable_defaults_by_category() {
        let Some(ini) = retail_rules_ini() else {
            return;
        };
        let rules = crate::rules::ruleset::RuleSet::from_ini(&ini).expect("retail rules parse");
        for (id, trainable) in [
            ("CAGAS01", false),
            ("NABNKR", false),
            ("GAPILL", false),
            ("YAREFN", true),
            ("E1", true),
            ("HTNK", true),
        ] {
            assert_eq!(rules.object(id).unwrap().trainable, trainable, "[{id}]");
        }
    }

    /// The two stock sections that take their Jumpjet locomotor from the
    /// `Locomotor=` GUID alone and never author `JumpJet=`.
    ///
    /// gamemd reads all nine parameters for them anyway —
    /// `TechnoTypeClass::ReadINI` `0x00715020`-`0x0071520F` has no branch — so
    /// a retail Kirov and Floating Disc hover at their authored 750, not at the
    /// constructor's 500 (`0x007115D3`). Gating the block on `JumpJet=yes`
    /// dropped every one of these values.
    #[test]
    fn retail_kirov_and_disc_keep_their_jumpjet_block_without_the_flag() {
        let Some(ini) = retail_rules_ini() else {
            return;
        };

        for id in ["ZEP", "DISK"] {
            let section = ini.section(id).unwrap_or_else(|| panic!("[{id}] section"));
            assert!(
                section.get_for_test("JumpJet").is_none(),
                "[{id}] is only interesting because stock omits JumpJet="
            );
            let obj = ObjectType::from_ini_section(id, section, ObjectCategory::Vehicle);
            assert!(
                !obj.jumpjet,
                "[{id}] JumpJet= is absent, so the flag is false"
            );
            assert_eq!(
                obj.locomotor,
                LocomotorKind::Jumpjet,
                "[{id}] takes the Jumpjet locomotor from its CLSID"
            );
            assert_eq!(
                obj.jumpjet_params.height, 750,
                "[{id}] authors JumpjetHeight=750 and gamemd reads it"
            );
        }

        let zep = ObjectType::from_ini_section(
            "ZEP",
            ini.section("ZEP").expect("[ZEP]"),
            ObjectCategory::Vehicle,
        );
        assert_eq!(
            zep.jumpjet_params.speed,
            crate::util::fixed_math::sim_from_f32(5.0)
        );
        assert_eq!(zep.jumpjet_params.climb, 6.0);
        assert_eq!(zep.jumpjet_params.crash, 12.0);
        assert!(zep.jumpjet_params.no_wobbles, "[ZEP] JumpjetNoWobbles=yes");
        // `[ZEP]` authors no deviation, so the constructor's 40 stands.
        assert_eq!(zep.jumpjet_params.deviation, 40);

        let disk = ObjectType::from_ini_section(
            "DISK",
            ini.section("DISK").expect("[DISK]"),
            ObjectCategory::Vehicle,
        );
        assert_eq!(
            disk.jumpjet_params.speed,
            crate::util::fixed_math::sim_from_f32(16.0)
        );
        assert_eq!(disk.jumpjet_params.climb, 8.0);
        assert_eq!(disk.jumpjet_params.crash, 15.0);
        assert_eq!(disk.jumpjet_params.deviation, 15);
    }

    /// The six stock sections that do author `JumpJet=yes` must be untouched by
    /// removing the gate: they all author `JumpjetHeight=500`, which is also the
    /// constructor seed.
    #[test]
    fn retail_jumpjet_yes_sections_are_unchanged_by_dropping_the_gate() {
        let Some(ini) = retail_rules_ini() else {
            return;
        };
        // Categories as the stock registries list them: the two jumpjet
        // infantry under `[InfantryTypes]`, the four choppers/transports under
        // `[VehicleTypes]`. Category does not reach the jumpjet block, but
        // parsing a section under the wrong one invites a later reader to
        // misread the fixture.
        for (id, category) in [
            ("JUMPJET", ObjectCategory::Infantry),
            ("LUNR", ObjectCategory::Infantry),
            ("SHAD", ObjectCategory::Vehicle),
            ("HIND", ObjectCategory::Vehicle),
            ("SCHP", ObjectCategory::Vehicle),
            ("SCHD", ObjectCategory::Vehicle),
        ] {
            let section = ini.section(id).unwrap_or_else(|| panic!("[{id}] section"));
            let obj = ObjectType::from_ini_section(id, section, category);
            assert!(obj.jumpjet, "[{id}] authors JumpJet=yes");
            assert_eq!(obj.jumpjet_params.height, 500, "[{id}] authors 500");
        }
    }

    /// gamemd looks up `JumpjetTurnRate` (`0x008436D0`) and `JumpjetAccel`
    /// (`0x00843680`) — lowercase `jet` — and `INIClass` compares CRC-32s of
    /// the raw key bytes (`CRCEngine::AddData @ 0x004A1DE0`), so the capital-J
    /// spelling stock uses is invisible to it. Every stock jumpjet section
    /// therefore flies on the constructor's turn rate 4 (`0x007115AE`) and
    /// acceleration 2.0 (`0x007115DD`).
    ///
    /// Derived over the retail file rather than asserted from a list: the
    /// section set is whatever authors a `Jumpjet*` key, and the two counts are
    /// counted here.
    #[test]
    fn retail_never_authors_the_spelling_gamemd_reads_for_turn_rate_or_accel() {
        let Some(ini) = retail_rules_ini() else {
            return;
        };

        let mut native_spelling = 0usize;
        let mut ini_spelling = 0usize;
        let mut jumpjet_sections = 0usize;
        let mut seen: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
        for name in ini.section_names() {
            if !seen.insert(name) {
                continue;
            }
            let Some(section) = ini.section(name) else {
                continue;
            };
            let authors_any = section
                .keys()
                .any(|k| k.to_ascii_lowercase().starts_with("jumpjet"));
            if !authors_any {
                continue;
            }
            jumpjet_sections += 1;
            for key in ["JumpjetTurnRate", "JumpjetAccel"] {
                if section.get_for_test(key).is_some() {
                    native_spelling += 1;
                }
            }
            for key in ["JumpJetTurnRate", "JumpJetAccel"] {
                if section.get_for_test(key).is_some() {
                    ini_spelling += 1;
                }
            }

            let obj = ObjectType::from_ini_section(name, section, ObjectCategory::Vehicle);
            assert_eq!(
                obj.jumpjet_params.turn_rate, 4,
                "[{name}] must keep the constructor turn rate"
            );
            assert_eq!(
                obj.jumpjet_params.accel, 2.0,
                "[{name}] must keep the constructor acceleration"
            );
        }

        assert_eq!(
            native_spelling, 0,
            "no stock section spells the keys the way gamemd looks them up"
        );
        assert_eq!(
            (jumpjet_sections, ini_spelling),
            (8, 16),
            "8 stock sections author Jumpjet* keys and all 8 carry both mis-cased keys"
        );
    }
}

#[cfg(test)]
mod simple_deploy_reader_tests {
    use crate::rules::{
        ini_parser::IniFile,
        native_processing::{RulesLayerKind, RulesLayerStack},
        ruleset::RuleSet,
    };
    #[test]
    fn simple_deploy_keys_match_native_reader_history() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/unit_simple_deploy.json",
        ))
        .unwrap();
        let base = "[VehicleTypes]\n0=SCHP\n[SCHP]\nIsSimpleDeployer=yes\nDeployToLand=yes\n[AudioVisual]\nDeployDir=2\n";
        for row in corpus["readers"]["deploy_dir"].as_array().unwrap() {
            // The native reader fixture can supply an empty cached value;
            // physical INI loading omits that entry before ReadInteger. It is
            // not the same input as an authored `DeployDir=` line.
            if row["raw"].as_str() == Some("") {
                continue;
            }
            let mut layers = RulesLayerStack::new(IniFile::from_str(base));
            let patch = row["raw"]
                .as_str()
                .map_or("[AudioVisual]\n".to_owned(), |value| {
                    format!("[AudioVisual]\nDeployDir={value}\n")
                });
            layers.push(RulesLayerKind::Scenario, IniFile::from_str(&patch));
            let rules = RuleSet::from_rules_layers(&layers).unwrap();
            assert_eq!(
                i64::from(rules.general.deploy_dir),
                row["native_raw"].as_i64().unwrap(),
                "{row}"
            );
        }
        let mut layers = RulesLayerStack::new(IniFile::from_str(base));
        for row in corpus["readers"]["deploying_anim_history"]
            .as_array()
            .unwrap()
        {
            let patch = row["raw"].as_str().map_or("[SCHP]\n".to_owned(), |value| {
                format!("[SCHP]\nDeployingAnim={value}\n")
            });
            layers.push(RulesLayerKind::Scenario, IniFile::from_str(&patch));
            let rules = RuleSet::from_rules_layers(&layers).unwrap();
            let object = rules.object("SCHP").unwrap();
            assert_eq!(
                object.deploying_anim.as_deref(),
                row["native_name"].as_str(),
                "{row}"
            );
            assert!(object.is_simple_deployer && object.deploy_to_land);
        }
    }
}
