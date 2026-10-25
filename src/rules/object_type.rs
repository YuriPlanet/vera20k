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

pub use crate::rules::ini_parser::ini_value::TechnoTypeCategory;
use crate::rules::ini_parser::{IniSection, is_native_none_type_name};
use crate::rules::jumpjet_params::JumpjetParams;
use crate::rules::locomotor_type::{LocomotorKind, MovementZone, SpeedType};
use crate::rules::sound_ini::SoundRegistry;
use crate::rules::terrain_rules::LandType;
use crate::util::fixed_math::SimFixed;
use crate::util::native_x87::NativeF64Bits;

/// The pi/180 double multiplied by `TechnoTypeClass::ReadINI @ 0x00712170`
/// when storing RollAngle at `+0x3A0` (`FMUL [0x007F4FB8] @ 0x007123BC`).
const TECHNOTYPE_DEGREES_TO_RADIANS: f64 = f64::from_bits(0x3f91_df46_a252_9d39);

/// ObjectTypeClass::ReadINI5F95DC..5F9616 reads fixed ART under its retained
/// ImageFile. Missing/invalid values preserve each current constructor/read byte.
pub(crate) fn read_object_theater_flags(
    image_art: &IniSection,
    current: (bool, bool),
) -> (bool, bool) {
    (
        image_art.read_bool("Theater", current.0),
        image_art.read_bool("NewTheater", current.1),
    )
}

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

/// TechnoType PipScale+3D4: constructor710D1C defaults to0; ReadINI714111
/// uses ReadPipScale474940 (ReadString capacity32, case-insensitive names).
/// Native values follow table81B9B0, not the order in a C++ header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(i32)]
pub enum PipScale {
    #[default]
    None = 0,
    Ammo = 1,
    Tiberium = 2,
    Passengers = 3,
    Power = 4,
    MindControl = 5,
}

impl PipScale {
    fn from_ini(value: &str) -> Self {
        match value.to_ascii_lowercase().as_str() {
            "ammo" => Self::Ammo,
            "tiberium" => Self::Tiberium,
            "passengers" => Self::Passengers,
            "power" => Self::Power,
            "mindcontrol" => Self::MindControl,
            _ => Self::None,
        }
    }
}

/// InfantryType+DFC/E00 state, retained by the process-resident type.
/// Constructors5236BD/5236C7 set1/7; admitted InfantryRead5240D1..524125
/// unconditionally invokes the shared ReadPip for both current defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct InfantryPips {
    pip: u32,
    occupy_pip: u32,
}

impl Default for InfantryPips {
    fn default() -> Self {
        Self {
            pip: 1,
            occupy_pip: 7,
        }
    }
}

impl InfantryPips {
    pub(crate) fn apply_pass(&mut self, section: &IniSection) {
        self.pip = section.read_pip("Pip", self.pip);
        self.occupy_pip = section.read_pip("OccupyPip", self.occupy_pip);
    }
}

/// InfantryType constructor clears EBE/EC2/EC3/EC4. ReadINI reads these
/// current-default flags, then 52466E..52469A forces Infiltrate for any role.
/// Retain the postprocessed byte across reached rules passes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct InfantryInfiltration {
    infiltrate: bool,
    c4: bool,
    engineer: bool,
    agent: bool,
}

impl InfantryInfiltration {
    pub(crate) fn apply_pass(&mut self, section: &IniSection) {
        // InfantryTypeClass::ReadINI 5244A9/524545/524579/5245AD,
        // then 52466E..52469A (retail gamemd.exe, Steam gamemd).
        self.infiltrate = section.read_bool("Infiltrate", self.infiltrate);
        self.c4 = section.read_bool("C4", self.c4);
        self.engineer = section.read_bool("Engineer", self.engineer);
        self.agent = section.read_bool("Agent", self.agent);
        self.infiltrate |= self.c4 || self.engineer || self.agent;
    }
}

#[cfg(test)]
#[path = "infantry_infiltration_tests.rs"]
mod infiltration_tests;

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
    /// Shared object/type aliases in Suggest_New_Object (4FBD80),
    /// IncrementFactoryCount (4FFA50), Recount (4FF980) and
    /// GetFactoryCount (500910), Steam gamemd. Exact RTTI
    /// comparisons, including primary-factory grouping, must retain the raw value.
    pub(crate) const fn for_production_rtti(value: i32) -> Option<Self> {
        match value {
            6 | 7 => Some(Self::BuildingType),
            15 | 16 => Some(Self::InfantryType),
            1 | 40 => Some(Self::UnitType),
            2 | 3 => Some(Self::AircraftType),
            _ => None,
        }
    }

    /// Exact production Type RTTI projection, not ReadFactory's full enum.
    const fn from_rtti(value: i32) -> Option<Self> {
        match value {
            7 => Some(Self::BuildingType),
            16 => Some(Self::InfantryType),
            40 => Some(Self::UnitType),
            3 => Some(Self::AircraftType),
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

/// TechLevel (`TechnoTypeClass+0x634`) as the constructor stores it
/// (`0x00711082`). Explicit `TechLevel=-1` is the distinct
/// civilian/unbuildable sentinel.
pub(crate) const DEFAULT_TECH_LEVEL: i32 = 255;

/// LeptonMindControlOffset (`TechnoTypeClass+0x3DC`) as the constructor
/// stores it (`0x00710D28`).
pub(crate) const DEFAULT_LEPTON_MIND_CONTROL_OFFSET: i32 = 70;

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

/// Phobos `UnitTypeExt`'s three `OreGathering.*` lists
/// (`src/Ext/UnitType/Body.{h,cpp}`, read at the `UnitTypeClass_LoadFromINI`
/// hook in the order Anims, Tiberiums, FramesPerDir). Each
/// `ValueableVector::Read` clears the vector and re-parses only when its key is
/// present, so a later Rules layer replaces a whole list or retains it.
///
/// The lists are parallel by index: `OreGathering.Tiberiums=` is searched for
/// the resource the harvester stands on, and the slot it answers selects the
/// animation and its frame count. `Anims=` drops a token the `AnimType` array
/// does not hold, so a longer `Tiberiums=` list shifts onto the wrong entry —
/// Phobos' own alignment, preserved here.
#[cfg(feature = "phobos")]
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PhobosOreGathering {
    /// `OreGathering.Anims=`: the resolved `AnimType` section names.
    pub anims: Vec<String>,
    /// `OreGathering.Tiberiums=`: the `[Tiberiums]` indices each slot serves.
    pub tiberiums: Vec<i32>,
    /// `OreGathering.FramesPerDir=`: frames per facing for each slot.
    pub frames_per_dir: Vec<i32>,
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
    /// Inherited `ObjectTypeClass::ReadINI @ 0x005F92D0` AlphaImage buffer
    /// (`+0x213`, `char[25]`); the constructor starts empty and later empty
    /// reads keep the current name.
    pub(crate) alpha_image: String,
    /// Which type registry this object belongs to.
    pub category: ObjectCategory,
    /// Native `TechnoTypeClass::ReadINI @ 0x00712170` `Category=` result at
    /// `+0x3BC`, separate from the Rust registry family above.
    pub techno_category: TechnoTypeCategory,
    /// BuildingType+EBC, world leptons added by Building4500A0 to GetCoords.
    /// Constructor45DEBC..45DEC8 clears it; ReadINI460F87 uses ReadCoord529CA0
    /// with the current triple as default. Other type classes do not read it.
    target_coord_offset: [i32; 3],
    /// `HalfDamageSmokeLocation1/2=`: BuildingTypeClass constructor initializes
    /// both triples to zero; `BuildingTypeClass::ReadINI @ 0x0045FE50` reads
    /// them in order through `Read3Int @ 0x00529CA0` at `0x0045FF13` and
    /// `0x0045FF3C`, using each current triple as the next pass's default.
    /// `RulesPassProcessor` owns these Building-family reads and publishes the
    /// retained values here; other techno families do not read these keys.
    half_damage_smoke_location1: [i32; 3],
    half_damage_smoke_location2: [i32; 3],
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
    /// TechnoType+C9E, exact Rules key `Nominal`: constructor7113F0=false,
    /// retained ReadBool713F2C..713F3E. This type input is distinct from
    /// an Infantry's runtime survivor marker (Infantry+6D9).
    nominal: bool,
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
    /// authorship for diagnostics. Gameplay uses the bound `armor_id()`;
    /// reparsing this text later could retroactively bind an unknown Ares name.
    pub armor: String,
    armor_id: crate::rules::armor::ArmorId,
    /// Phobos `TechnoTypeExt::ShieldType` (`ShieldType=`): the bound
    /// `[ShieldTypes]` entry, or `None` for an absent/`none` key. Retained as
    /// the authored name; the Rules catalog resolves it at use.
    #[cfg(feature = "phobos")]
    shield_type: Option<String>,
    /// Phobos `TechnoTypeExt::DigitalDisplayTypes` (`DigitalDisplayTypes=`):
    /// authored `[DigitalDisplayTypes]` names in list order. Empty means the
    /// techno uses its category's `[AudioVisual]` default list.
    #[cfg(feature = "phobos")]
    digital_display_types: Vec<String>,
    /// Phobos `TechnoTypeExt::DigitalDisplay_Disable`.
    #[cfg(feature = "phobos")]
    digital_display_disable: bool,
    /// Phobos `TechnoTypeExt::DigitalDisplay_Health_FakeAtDisguise`; `None`
    /// inherits `[AudioVisual] DigitalDisplay.Health.FakeAtDisguise`.
    #[cfg(feature = "phobos")]
    digital_display_health_fake_at_disguise: Option<bool>,
    /// Phobos `BuildingTypeExt::PlacementPreview` (`PlacementPreview=`),
    /// default true. The preview also needs the rules global and the client
    /// switch.
    #[cfg(feature = "phobos")]
    placement_preview: bool,
    /// `PlacementPreview.Shape=` (a `TheaterSpecificSHP` name). `None` uses
    /// the buildup's last frame or the body's first frame.
    #[cfg(feature = "phobos")]
    placement_preview_shape: Option<String>,
    /// `PlacementPreview.ShapeFrame=`, `None` keeps the image's default frame.
    #[cfg(feature = "phobos")]
    placement_preview_shape_frame: Option<i32>,
    /// `PlacementPreview.Offset=` in leptons, default `0,-15,1`.
    #[cfg(feature = "phobos")]
    placement_preview_offset: [i32; 3],
    /// `PlacementPreview.Remap=`, default true.
    #[cfg(feature = "phobos")]
    placement_preview_remap: bool,
    /// `PlacementPreview.Palette=`, used when `Remap=no`; unset uses the
    /// theater unit palette.
    #[cfg(feature = "phobos")]
    placement_preview_palette: Option<String>,
    /// `PlacementPreview.Translucency=`, unset inherits the rules global.
    #[cfg(feature = "phobos")]
    placement_preview_translucency: Option<crate::rules::phobos_placement_preview::Translucency>,
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
    /// Inertia weight (`Weight=`), initially 1.0 from TechnoType710C66..710C71.
    /// ReadINI7124CF..7124EA passes the current double as each pass's default.
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
    pub accel_factor: NativeF64Bits,
    /// Fraction of max speed lost per tick during braking (DeaccelerationFactor=).
    /// TechnoType ctor710BBC/710BC6 writes double0.002 (3F60624DD2F1A9FC)
    /// to+300; ReadINI7124A3 retains the current double as its default. Both
    /// coefficients preserve widened authored floats and distinct constructor
    /// doubles. Applied when within slowdown_distance of the destination.
    pub decel_factor: NativeF64Bits,
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
    /// TechnoType+7FC, read from the current Image section of fixed ARTMD.
    /// The ordered processor owns its retained default (constructor0).
    voxel_shadow_index: i32,
    /// `PitchSpeed=` (`TechnoType+0x3A8`): constructor `0.25`; each
    /// `TechnoTypeClass::ReadINI @ 0x00712170` pass uses this current double
    /// as `ReadDouble`'s default (`0x007123DA..0x007123E8`). Normal Fly
    /// `Draw_Matrix @ 0x004CF610` gates PitchAngle/RollAngle on current speed
    /// strictly exceeding this value.
    pitch_speed: NativeF64Bits,
    /// `RollAngle=` (`TechnoType+0x3A0`): constructor 30 degrees in radians.
    /// ReadDouble uses -1 as a sentinel; each pass converts other degrees to
    /// radians and retains the current value for -1 (`ReadINI @ 0x00712170`,
    /// `0x007123A2..0x007123C2`). Normal Fly `Draw_Matrix @ 0x004CF610`
    /// applies it about X while SecondaryFacing's rotation is active.
    roll_angle: NativeF64Bits,
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
    // ReadOwners4750D0 replaces the constructor's -1 even when comma
    // tokenization produces no entries. Derived from the retained INI body.
    required_houses_explicitly_empty: bool,
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
    /// `[TechnoTypes] AIBuildLimit=` — the NPSE per-type cap on a computer
    /// house's production of this type. The techno reader extension chain
    /// reads it with the current field as its default into the per-type
    /// extension at `TechnoType+0xA90` (`+0x40` there; the reader block runs
    /// `0xC27249..0xC2725A`, entered at `0xC308F3`), whose constructor writes
    /// `0x7FFFFFFF` (`0xC32950`), so absent means no limit. The
    /// `BuildingClass::Factory_AI` fetch hook (`0x45033C`, cave
    /// `0xC335A0..0xC33726`) compares it with the house's owned counter per
    /// class (`+0x5500/+0x5514/+0x5528/+0x553C`) when positive and with its
    /// produced and owned counters when non-positive; at limit the
    /// category's choice slot clears (`+0x564C/+0x5650/+0x5654/+0x5658`) and
    /// the start skips. Clearing a building choice leaves its base-plan node
    /// unsatisfied, so the planner can select the same capped type again.
    pub aibuild_limit: i32,
    /// `[BuildingTypes] AIPlanningCount=` — the NPSE count of plan nodes a
    /// computer house's base plan gives this building. The techno reader
    /// extension chain reads it with the current field as its default and
    /// stores the result through `al` (`0xC2E223..0xC2E237`), so the
    /// stored value is one byte; the extension constructor
    /// zeroes it (`0xC3039B`), so absent means the stock single node. The
    /// `AI_RecalcBuildOptions` append loop (`0x5056C1` hook, cave
    /// `0xC2C331..0xC2C367`) re-enters the candidate head until its
    /// per-candidate counter reaches the count with signed CMP/JL. Counts
    /// 1..127 repeat eligible entries before seed selection; 0 and high-bit
    /// counts keep one entry, rather than repeating the final priority list.
    pub ai_planning_count: i32,
    /// Requires spy infiltration of an Allied Battle Lab to unlock.
    pub requires_stolen_allied_tech: bool,
    /// Requires spy infiltration of a Soviet Battle Lab to unlock.
    pub requires_stolen_soviet_tech: bool,
    /// Requires spy infiltration of a Yuri Battle Lab to unlock.
    pub requires_stolen_third_tech: bool,
    /// ObjectType+1F8 `Image=`, the type's ID unless a reached pass names
    /// another; it names the art.ini section of the type's sprite or voxel.
    /// The ordered Rules processor owns it (`ObjectImageState`).
    pub image: String,
    /// Reached first-success SHP requests (ObjectTypeLoadSHP5F9070 or
    /// TechnoRead715820), independent of current ImageFile.
    shp_image_loads: Vec<crate::rules::object_image::ObjectImageLoad>,
    /// Entry ImageFile at the last LoadVoxel5F8110, including a saved load
    /// that precedes an Arctic SHP mutation.
    voxel_image_load: Option<String>,
    /// Signed Power read result for Buildings. Constructor45DEF6/45DEFC
    /// starts output/drain at0;461060..4610A0 passes their reconstructed
    /// signed value to ReadInt5276D0. The existing reached-history reader
    /// retains that value; power consumers split generation from wrapping
    /// negated consumption.
    /// Source: building_power_status.json reader_controls.
    pub power: i32,
    /// BuildingType `ExtraPower=` output `+0xEE8` and drain `+0xEEC`, the
    /// ordered Rules processor's retained reader result (stock YAPOWR
    /// Bio-Reactor output 100, GAGAP drain 9000).
    pub extra_power: crate::rules::building_extra_power::BuildingExtraPower,
    /// Building foundation footprint (e.g., "3x2", "1x1"). Buildings only.
    pub foundation: crate::rules::foundation::Foundation,
    /// Pixel offset for health bar / selection bracket Y position.
    /// Negative values shift the bar UP (above taller sprites). Default 0.
    /// Parsed from `PixelSelectionBracketDelta` in rules.ini.
    pub pixel_selection_bracket_delta: i32,
    /// Rules LeptonMindControlOffset: Type+3DC ctor710D28=70, read714145.
    pub lepton_mind_control_offset: i32,
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
    /// `VoiceSelectEnslaved=` at TechnoType+430. Constructor710DA4..710DBD
    /// leaves an empty vector; reader712B87..712BF1 follows VoiceSelect
    /// and uses the same ReadSoundList525430/default retention. VoiceSelect
    /// chooses it while the instance's existing SlaveOwner is nonnull.
    /// Executed constructor/reader/caller controls: input_oracle/selection_navigation.
    pub voice_select_enslaved: Vec<String>,
    /// `VoiceSelectDeactivated=` at TechnoType+44C: the constructor's empty
    /// vector, then ReadSoundList (`0x00712BF1`) after VoiceSelectEnslaved,
    /// with its retention. VoiceSelect chooses it for a deactivated object
    /// (`+0x1C8`, `0x00708EFD`) that has no SlaveOwner.
    pub voice_select_deactivated: Vec<String>,
    /// VoiceMove vector at TechnoType+468/count+478. Constructor710DCD..
    /// 710E03 leaves it empty;712AB3..712B1D reads ReadSoundList525430 with
    /// current-vector retention. Fixed SOUNDMD binding preserves resolved
    /// order/duplicates and skips unknown names. Source: move_order_voice.
    voice_move: Vec<String>,
    /// VoiceAttack vector at TechnoType+484/count+494. Ctor710DEF..710E26
    /// creates an empty vector;712C5B..712CC5 uses ReadSoundList525430 with
    /// current-list retention. The fixed SOUNDMD binder preserves every
    /// resolved entry, including order/duplicates. Source: attack_order_voice.
    voice_attack: Vec<String>,
    /// Primary, primary-elite, secondary, secondary-elite voice IDs at
    /// +580/+584/+588/+58C. Ctor710F55..710F68 sets -1;713679..713775
    /// reads each exact key through ReadString128/Voc7514D0, retaining a
    /// valid current ID on missing/empty/unknown text.
    voice_attack_overrides: [Option<String>; 4],
    /// Registered scalar `VoiceHarvest=` (TechnoType+57C). Constructor710F4F
    /// starts at -1; ReadINI stores at713696 after ReadString128/Voc7514D0.
    /// The fixed SOUNDMD reader retains the prior valid slot across passes.
    voice_harvest: Option<String>,
    /// Registered scalar `VoiceDeploy=` (TechnoType+590), default -1 at
    /// constructor710F75. ReadINI713769..7137C2 uses the same retained reader.
    voice_deploy: Option<String>,
    /// Registered scalar `VoiceUndeploy=` (TechnoType+594), default -1 at
    /// constructor710F7B. ReadINI7137A5..7137FE follows VoiceDeploy.
    voice_undeploy: Option<String>,
    /// VoiceEnter scalar at TechnoType+558, constructor710F19=-1.
    /// ReadString128/Voc7514D0 stores at7130CD, retaining the current ID
    /// for missing/empty/unresolved text. Enter709020 falls to Move for -1.
    voice_enter: Option<String>,
    /// VoiceCapture scalar at TechnoType+55C, constructor710F1F=-1.
    /// The same retained reader stores at713109. Capture708DC0 falls to
    /// Enter709020 for -1; registered scalars queue without a Main draw.
    voice_capture: Option<String>,
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
    /// `CreateSound=` — TechnoType+534, constructor710EE3=-1. Original
    /// reader712E75..712EB7 uses ReadString128 then VocFindIndex7514D0,
    /// retaining the last valid fixed-catalog ID through reached rules passes.
    /// House RecordLastBuilt4FB7CA overrides its class cue with this scalar.
    create_sound: Option<String>,
    /// Retained BuildingType+16B0 and EA4/EA8/EAC; Process owns reached reads.
    secret_lab: crate::rules::secret_lab::SecretLabRules,
    /// `DamageSound=` — `TechnoTypeClass+0x538`, the type's own struck cue.
    ///
    /// Original gamemd inherited constructor710AF0
    /// stores the -1 default at710EE9;
    /// reader712EB7..712F07 passes literal84421C, empty889F64 and capacity128
    /// to ReadString528A10, then resolves through VocFindIndex7514D0.
    /// The fixed SOUNDMD catalog binder owns this resolved reference. Native
    /// result1 plays it twice at702760/7027A9 before the common7027AE tail;
    /// Building4426D2 also reads this same field to gate its global cue.
    /// Missing, empty and unregistered names retain the prior resolved ID in
    /// the existing processed-rules reader, starting at the native -1 default.
    damage_sound: Option<String>,
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
    /// `VoiceFalling=` — TechnoType+0x54C, constructor `0x00710F07` = -1,
    /// read at `0x00712FEC` through ReadString128 and `VocClass::FindIndex @
    /// 0x007514D0`, keeping the prior index for an absent, empty or unknown
    /// name. `FootClass::AI` plays it as a bomb-dropped object starts falling
    /// (`0x004DAB3C..0x004DABB1`). No retail type sets it.
    pub voice_falling: Option<String>,
    /// `VoiceComment=` — InfantryType+0xE88, an empty vector from the
    /// constructor (`0x0052372A`), read by `InfantryTypeClass::ReadINI` through
    /// ReadSoundList (`0x00524193..0x005241AC`) with prior-vector retention.
    /// The idle fidget plays the first (`0x0051CF42..0x0051CFCB`), a tethered
    /// arrival the second (`0x0051A812..0x0051A84A`). No retail type sets it.
    pub voice_comment: Vec<String>,
    /// `TurretRotateSound=` — `TechnoTypeClass+0x560`, the sustained positional
    /// loop a `Turret=yes` object plays while its turret turns.
    ///
    /// Constructor `0x00710F25` stores `-1`; `TechnoTypeClass::ReadINI @
    /// 0x007133BA` calls `ReadString` at `0x007133C8` with a 0x80 buffer,
    /// then `VocClass::FindIndex` at `0x007133D5`. `0x007133DF..0x007133E7`
    /// retains or writes the index, keeping the old value for an absent,
    /// empty or unregistered name. `RulesClass` processes type sections in
    /// scenario order, so retention holds across RULESMD/LANGRULE/mode/map
    /// layers: the field is the last name a layer gave *and* `soundmd.ini`
    /// registers, and `None` stands for the
    /// constructor's `-1` (silence, never a raw audio-bag lookup). The fixed
    /// SOUNDMD binder owns that resolution.
    ///
    /// Consumer: `crate::sim::world::turret_rotate_sound`.
    /// Stock authors: `[GTGCAN]` `GrandCannonRotate`, `[CAEAST02]`
    /// `YuriBustRotate` (both `Turret=yes`); no other type names it.
    pub turret_rotate_sound: Option<String>,
    /// `ActivateSound=` / `DeactivateSound=` (TechnoType `+0x5A8` / `+0x5AC`,
    /// constructor `-1`): read like `TurretRotateSound=` at `0x007138D1` /
    /// `0x0071390D`. Reactivate and Deactivate play them where the object
    /// stands when the latch changes (`sim::powered_unit`).
    pub activate_sound: Option<String>,
    pub deactivate_sound: Option<String>,
    /// `AmbientSound=`: `ObjectTypeClass+0x1F4`, which the constructor sets to
    /// `-1` (`0x005F7140`). `ObjectTypeClass::ReadINI` reads it at
    /// `0x005F93C1..0x005F9403` (ReadString 0x80, then `VocClass::FindIndex @
    /// 0x007514D0`) and keeps the old value for an absent, empty or
    /// unregistered name, so it has `turret_rotate_sound`'s layering. `None`
    /// is `-1`. The fixed SOUNDMD binder owns the resolution.
    ///
    /// Consumer: `crate::sim::world::object_sound`.
    pub ambient_sound: Option<String>,
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
    /// `WorkingSound=`: `BuildingTypeClass+0xE80`, `-1` from the constructor
    /// (`0x0045DE82`). `BuildingTypeClass::ReadINI` reads it at `0x0046088F`
    /// with `BuildupSound`'s layering. `None` is `-1`. Consumer:
    /// `crate::sim::world::building_working_sound`.
    pub working_sound: Option<String>,
    /// `NotWorkingSound=`: `BuildingTypeClass+0xE84`, `-1` from the
    /// constructor (`0x0045DE88`), read at `0x004608DD` the same way.
    pub not_working_sound: Option<String>,
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
    /// `EnterTransportSound=` — `TechnoTypeClass+0x564`, read at
    /// `0x007133FC` through ReadString128 and `VocClass::FindByName`.
    /// `TechnoClass::AddPassenger @ 0x00710670` plays it at the transport's
    /// Location once a passenger is in its cargo.
    pub enter_transport_sound: Option<String>,
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
    /// TechnoType+0xD21: `TurretSpins=` makes `UnitClass::Facing_Update`
    /// replace its ordinary idle/target decision with a per-frame turret Set.
    /// TechnoType construction writes false at `0x007114C4`; ReadINI reads the
    /// exact `TurretSpins` key (`0x0084412C`) through `ReadBool @ 0x005295F0`,
    /// using the current field as its default (`0x00713357`..`0x00713371`).
    pub turret_spins: bool,
    /// Turret rotation speed in RA2 "ROT" units (degrees per game frame at 15fps).
    /// Higher = faster turret rotation. Only meaningful when `has_turret` is true.
    /// Typical values: 5 (War Miner), 7 (Grizzly/Rhino).
    pub turret_rot: i32,
    /// Retained TechnoType recoil reader; shared by every voxel turret.
    pub recoil: crate::rules::recoil::RecoilConfig,
    /// Six retained settings copied when this Techno creates an Airstrike manager.
    pub airstrike: crate::rules::airstrike::AirstrikeSettings,
    /// TechnoType+6A4: constructor711134 sets zero; ReadINI7147B4..7147C8
    /// uses exact `RadialFireSegments`, ReadInt5276D0 and the current value.
    /// Signed and unclamped: FireAt distinguishes zero, negative and positive.
    pub radial_fire_segments: i32,
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
    /// Z-seed adjustment for a voxel turret's blit (TurretAnimZAdjust=, negative
    /// = nearer the camera). Building vtable `0x7E3EBC+0x2EC` folds it into the
    /// draw Z only when `TurretAnimIsVoxel`; it is not a ground-layer sort key.
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
    /// Ares3.0 TypeExt+4C6, constructor false. Manual object/cell action
    /// hooks1004A460/1004A430 leave the attack rung; automatic targeting,
    /// retaliation and script-issued missions retain their existing owners.
    #[cfg(feature = "ares")]
    no_manual_fire: bool,
    /// Ares3.0 TypeExt+646/+647. Unset uses the class/score default at the
    /// Add/Remove_Tracking owner; an explicit false is a distinct retained input.
    #[cfg(feature = "ares")]
    keep_alive: Option<bool>,
    /// Immutable projection of the retained TechnoTypeExt+308/+314/+320/+321
    /// reader. Country identities bind during Process, never from merged INI.
    #[cfg(feature = "ares")]
    factory_owners: crate::rules::native_processing::AresFactoryOwners,
    #[cfg(feature = "ares")]
    explicit_factory: crate::rules::native_processing::AresExplicitFactory,
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
    /// instead of committing to one. GreatestThreat collects its candidates
    /// (`combat::greatest_threat`), the entity's distributed-fire state picks
    /// the next one (`0x00709550`), and FireAt drops the fired target
    /// (`0x006FF872..0x006FF8EB`, `combat::world_fire`). Default no.
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
    /// `DeathWeapon=` (+D18), the WeaponType fired on death (overrides the
    /// default explosion), as the ordered Rules processor resolved it.
    pub death_weapon: Option<String>,
    /// `DeathWeaponDamageModifier=` applied to explicit/current death weapons.
    /// The native TechnoType constructor seeds this to 1.0.
    pub death_weapon_damage_modifier: f32,
    /// Superweapon type ID granted when this building is completed (SuperWeapon= in rules.ini).
    /// References a section listed in [SuperWeaponTypes].
    pub super_weapon: Option<String>,
    /// Secondary superweapon type ID, typically from an upgrade (SuperWeapon2= in rules.ini).
    pub super_weapon2: Option<String>,
    /// BuildingTypeExt+178, ordered lookup-only `SuperWeapons=` references.
    #[cfg(feature = "ares")]
    additional_super_weapons: Vec<String>,
    /// Phobos `UnitTypeExt`'s three `OreGathering.*` lists, retained per
    /// VehicleType and read by the harvester's gather overlay.
    #[cfg(feature = "phobos")]
    ore_gathering: PhobosOreGathering,
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
    /// UnitType+E1A, CarriesCrate (845DE0): ctor74715B clears it;
    /// ReadINI74771F..74773F uses ReadBool5295F0 with the current value.
    /// UnitReceiveDamage73838A also requires the appropriate Scenario flag.
    carries_crate: bool,
    /// `Spawned=` (`TechnoTypeClass+0xD54`): this type is a carrier/launcher
    /// child (Hornet, V3/Dreadnought missile). `TechnoTypeClass::ReadINI`
    /// reads key `"Spawned"` (`0x008437D8`) at `0x00714E7D..0x00714E91`.
    /// `TechnoClass::Death_Announcement @ 0x004D98C0` (`0x004D98DD`) stays
    /// silent for such types.
    pub spawned: bool,
    /// Whether this structure accepts ore/gem delivery (Refinery=yes in rules.ini).
    pub refinery: bool,
    /// `Weeder=`. A BuildingType's: ExitObject dispatch tests it independently
    /// from `Refinery=`, `WeaponsFactory=`, and `Naval=`. A UnitType's
    /// (`+0xE0F`, ReadBool at `0x007476C8`, constructor 0 at `0x00747115`)
    /// lets a damaged computer unit retreat to a dock as a `Harvester=` does;
    /// no retail unit sets it.
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
    /// Signed `Storage=` capacity from the TechnoType rules reader. Building
    /// capacity contributes to House+310, read by Available_Storage4F69D0;
    /// placement does not grant this many credits. Mobile cargo uses it too.
    pub storage: i32,
    /// Free unit spawned when this structure is placed (FreeUnit= in rules.ini),
    /// as the stored ID the ordered Rules processor resolved.
    pub free_unit: Option<String>,
    /// Structures this unit may dock with (Dock= in rules.ini), as the stored
    /// IDs the ordered Rules processor resolved.
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
    /// Shared TechnoType+684 count limit; -1 is unlimited. Aircraft use
    /// their release/dock owners; Unit Reload and Building refill read it live.
    pub ammo: i32,
    /// TechnoType+680, ReadINI71474C (InitialAmmo); -1 selects Ammo at
    /// Aircraft InitFromType414033..41404B. Other signed values are retained.
    pub initial_ammo: i32,
    /// TechnoType+698, raw signed frames. Constructor711122 seeds0;
    /// ReadINI71486A..71487E retains the current value as ReadInt's default.
    pub reload: i32,
    /// TechnoType+69C, constructor711128=-1. ReadINI714884..714898;
    /// -1 makes StartReloading6FB080 use the ordinary Reload interval.
    pub empty_reload: i32,
    /// TechnoType+6A0, constructor71112E=0. ReadINI71489E..7148B2;
    /// StartReloading6FB080 multiplies it by the signed PipWrap quotient twice.
    pub reload_increment: i32,
    /// TechnoType+6B1/+6B4; constructor711163..711169 clears both.
    /// ReceiveDamage701ADB consumes the pre-Object packet when enabled.
    pub damage_reduces_readiness: bool,
    pub readiness_reduction_multiplier: f32,
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
    /// `TargetLaser=` (TechnoType+0xC90; the constructor stores 0 at
    /// `0x0071139B`, ReadINI `0x00714C8D..0x00714CA1` reads the key string at
    /// `0x00843898`). A human player's shot starts the firer's 15-frame laser
    /// timer (`Fire_At 0x006FF48D..0x006FF4C9`); only `UnitClass::Draw_It`
    /// draws it. Stock sets it on the Apocalypse.
    pub target_laser: bool,
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
    /// `DeploysInto=` (+404), the BuildingType this unit deploys into (AMCV
    /// into GACNST), as the ordered Rules processor resolved it.
    pub deploys_into: Option<String>,
    /// Resolved non-null `UndeploysInto=` UnitType identity (e.g. GACNST→AMCV).
    /// `TechnoTypeClass__ReadINI @ 0x00712170`, block
    /// `0x0071329D..0x007132E4`, calls
    /// `UnitTypeClass__FindOrAllocate @ 0x007480D0` and stores its pointer at
    /// `BuildingType+0x408`; native `none`/`<none>` therefore remain `None`.
    pub undeploys_into: Option<String>,
    /// `PowersUnit=` (TechnoType `+0x40C`): the UnitType this building keeps
    /// active while operational (retail GAROBO's ROBO). The same block reads
    /// it at `0x007132DF..0x00713309`: ReadString 0x80, then
    /// `UnitTypeClass::FindOrAllocate`; an empty value keeps the old one,
    /// `none` nulls it. Owner: `sim::powered_unit`.
    pub powers_unit: Option<String>,
    /// `PoweredUnit=` (TechnoType `+0x410`, ReadBool at `0x0071330F`, default
    /// false): the unit goes offline while its house has no operational
    /// `PowersUnit=` building. Owner: `sim::powered_unit`.
    pub powered_unit: bool,
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
    /// TechnoType+D2B: ctor711500 sets true; ReadINI7153DA..7153F5
    /// reads TiltsWhenCrushes with the current byte as its Bool default.
    pub tilts_when_crushes: bool,
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
    emp: Option<crate::rules::ares_emp::EmpTypeRules>,
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

    /// BuildingType+EB8. Constructor45DD90 leaves0; ReadFactory474FF0 at
    /// 460529 stores any recognized raw RTTI, including object classes.
    /// Rules own this value; production categories are derived, never stored
    /// as a second, lossy authority. Steam gamemd; native controls in
    /// trigger_no_factories_events.json.
    factory_rtti: i32,
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
    /// gamemd.exe).
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
    /// constructor clear `0x0052378F`, infantry only). ReadINI's postpass
    /// (`0x0052466E..0x0052469A`) also sets it for C4/Engineer/Agent, even
    /// after explicit Infiltrate=no. An unarmed infiltrator
    /// still scans for a target (`0x0051E296..0x0051E2BC`), and a team on
    /// action 15 sends it in (`0x006EB71A..0x006EB759`).
    pub infiltrate: bool,
    /// `Agent=` (`InfantryTypeClass+0xEC4`, ReadBool `0x00524598..0x005245B2`
    /// on key `0x00825954`, constructor clear `0x005237B6`, infantry only):
    /// an infiltrator that spies. It enters `Spyable=` buildings where any
    /// other infiltrator enters `Capturable=` ones (`0x0051EE6B..0x0051EEE7`),
    /// and entering one runs [`crate::sim::spy_infiltration`]'s
    /// `BuildingClass::Infiltrate` (`0x00519FF8`).
    pub agent: bool,

    /// Whether this unit can self-deploy/undeploy via the Deploy command.
    /// Parsed from `Deployer=yes` in rules.ini. Triggers `Deploy`/`NoDeploy`
    /// cursor when the player hovers over this unit itself.
    pub deployer: bool,

    /// `Capturable=` (`BuildingTypeClass+0x1572`): an engineer may capture
    /// this building, and an infiltrator that is not `Agent=` may enter it
    /// (`0x0051EE85..0x0051EE9F`). A spy asks [`Self::spyable`] instead.
    pub capturable: bool,
    /// `Spyable=` (`BuildingTypeClass+0x1576`, ReadBool
    /// `0x0046002F..0x00460043`, constructor clear `0x0045E05D` with EBX zeroed
    /// at `0x0045DD9A`, buildings only): an `Agent=` infiltrator may enter it
    /// (`0x0051EED9..0x0051EEE7`, `0x0051B324..0x0051B330`).
    pub spyable: bool,

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

    /// `LeaveRubble=` (BuildingType `+0x1768`): a destroyed building leaves
    /// rubble on its foundation (`BuildingClass::AfterDestruction @
    /// 0x00441F60`). Read by `BuildingTypeClass::ReadINI` at `0x00460EF5`
    /// with the current value as default; the constructor's is false
    /// (`0x0045E279`).
    pub leave_rubble: bool,

    /// `CrateBeneath=` (BuildingType `+0x1767`, read at `0x00460EC1`) and
    /// `CrateBeneathIsMoney=` (`+0x1769`, read at `0x00460EDB`): a destroyed
    /// building places a crate at its origin cell, money or a random powerup
    /// (`BuildingClass::AfterDestruction @ 0x00441F60`). Both keep the current
    /// value as the read default; the constructor's is false (`0x0045E273`,
    /// `0x0045E284`).
    pub crate_beneath: bool,
    pub crate_beneath_is_money: bool,

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

    /// `SizeLimit=` at `TechnoTypeClass+0x388`, stored as binary64. The
    /// constructor `0x00710AF0` seeds 0.0 (`0x00710C93`, `0x00710C99`); the
    /// native type reader reads this field with `ReadDouble` (`0x005283D0`).
    pub size_limit: f64,

    /// `Size=` at `TechnoTypeClass+0x380`, stored as binary64. The constructor
    /// `0x00710AF0` seeds 1.0 for every category (`0x00710C66`, `0x00710C87`,
    /// `0x00710C8D`); the native type reader uses `ReadDouble` (`0x005283D0`).
    pub size: f64,

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
    /// `-1` (default) keeps the passenger's normal weapon-selection ladder.
    /// The passenger fires from the carrier. Distinct from IFVMode, which
    /// applies to Gunner transports that swap their own weapon based on the
    /// passenger's slot.
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

    /// Weapon used when this infantry fires from inside a garrisoned building:
    /// InfantryType `OccupyWeapon=` (+E04), as the ordered Rules processor
    /// resolved it. Falls back to the primary weapon when unset.
    pub occupy_weapon: Option<String>,

    /// Elite-level weapon used when garrisoned (`EliteOccupyWeapon=`, +E20).
    /// Falls back to `OccupyWeapon` or the primary weapon when unset.
    pub elite_occupy_weapon: Option<String>,

    /// Immutable projection of the retained InfantryType Pip/OccupyPip state.
    infantry_pips: InfantryPips,

    /// ReadSequenceData's sound pairs from every reached rules pass,
    /// projected from the native type registry; bound by
    /// [`Self::bind_sequence_sounds`].
    sequence_sound_reads: Vec<crate::rules::infantry_sequence::SequenceSoundRead>,

    /// The bound record sounds: at most two per DoType (record `+0x10`).
    sequence_sounds: Vec<crate::rules::infantry_sequence::SequenceSound>,

    /// What pip display to render below the health bar (PipScale= in rules.ini).
    /// `Tiberium` shows per-bale cargo pips for harvesters using pips2.shp.
    pub pip_scale: PipScale,

    /// TechnoType+3D8. Constructor710D22 clears this; the exact
    /// PipsDrawForAll ReadBool71412B..714145 retains its current default.
    /// Only the Building health caller uses this relationship bypass.
    pips_draw_for_all: bool,

    /// TechnoType+3E4: signed PipWrap. Constructor710D40 seeds0;
    /// ReadInt714179..714198 uses the retained field as its default.
    pip_wrap: i32,

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
    /// INI keys fill it is decided on every reached Rules pass by the mutually
    /// exclusive `TurretCount` branch of the ordered Rules processor
    /// (`native_processing::WeaponSlots`, where the ReadINI derivation and its
    /// addresses live); RuleSet publishes the retained array. Resolution
    /// (elite tier, index) lives in `sim::combat::combat_weapon::weapon_for_index`.
    pub weapon_list: Vec<Option<String>>,

    /// The native elite weapon array (`TechnoTypeClass+0xA94`, stride `0x1C`).
    /// Same shape and same storage relationship: slot 0 is
    /// [`Self::elite_primary`], slot 1 is [`Self::elite_secondary`].
    pub elite_weapon_list: Vec<Option<String>>,

    /// `WeaponCount=` (`TechnoTypeClass+0x80C`, ReadINI `0x00712873`). Bounds
    /// the `Weapon%d`/`EliteWeapon%d` loop; slots at or past it keep their
    /// constructor value, so a `TurretCount>0` type with `WeaponCount=1`
    /// (stock `[SREF]`) has no secondary at all. The ordered Rules processor
    /// applies that bound on each reached pass (`native_processing::WeaponSlots`),
    /// so `weapon_list` already reflects it and readers never re-apply it.
    /// Constructor default 0: the
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
    /// default 0 @ `0x0071136F`), retained by the ordered Rules processor with
    /// the weapon arrays. `> 0` short-circuits weapon selection to
    /// `CurrentWeaponNumber` unless the type is gattling.
    pub turret_count: i32,

    /// `HasTurretTooltips=` (`TechnoTypeClass+0x806`, ReadBool `0x00712837`,
    /// ctor 0 @ `0x00711369`). With a positive TurretCount it selects the
    /// turret formatter of `UnitClass::GetUIName @ 0x00746B20`.
    pub has_turret_tooltips: bool,

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

    /// TechnoType+6C0: ctor71117B=false, ReadBool71523D reads
    /// `AttackFriendlies` with the retained byte as default, no clamp.
    attack_friendlies: bool,

    /// TechnoType+6C1: ctor711181=false, ReadBool71525E reads
    /// `AttackCursorOnFriendlies` with the retained byte as default.
    /// Stock IVAN/CIVAN use it; the IvanBomb/Passengers exception belongs
    /// to What_Action, not this immutable rule projection.
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

    /// TechnoType+0xC9A `Invisible=`. BuildingType ReadINI460E09 also forces
    /// it true while InvisibleInGame is set; the ordered Rules owner retains
    /// that write across layers. Live rejection can test both bytes separately.
    pub invisible: bool,

    /// `InvisibleInGame=yes` on BuildingType. Logical-only buildings (e.g., bridge
    /// anchors) that should not receive C4 or other interaction cursors.
    /// ReadINI460E09/460E10 also forces Invisible=true and RadarVisible=false.
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
    /// BuildingType `ToOverlay=`, read from the fixed ART section the Image
    /// names, as the ordered Rules processor resolved it.
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

    /// `IsSelectableCombatant=` (TechnoType+DBC): P selection reads this
    /// independently of weapons and `Selectable`. Constructor710AF0 writes
    /// false at71164B; ReadINI71574E..715769 uses the current byte as its
    /// ReadBool default across reached rules layers. Executed controls:
    /// tools/input_oracle/selection_navigation.{json,meta.json,md}.
    pub is_selectable_combatant: bool,

    /// `CanBeHidden=` (TechnoType `+0x724`): constructor `0x00711229` writes
    /// true; ReadINI (`0x007121EB..0x00712202`) reads it with ReadBool from
    /// the ART INI (`0x00887180`) under the type's own ID, never the rules
    /// section. `RuleSet::project_art_data` sets it from
    /// [`ArtRegistry::can_be_hidden`](crate::rules::art_data::ArtRegistry::can_be_hidden).
    /// Gates the Behind indicator (`TechnoClass::AI_Update @ 0x006FA2A2`).
    pub can_be_hidden: bool,

    /// Last nonempty Palette request from the reached TechnoType ReadINI
    /// passes. This is binding metadata, not proof that its asset loaded:
    /// native717820 stores null if6263D0 cannot load the named group.
    /// The registered type's Image/reference owner supplies this projection.
    custom_palette: Option<String>,

    /// `RevealToAll=` (TechnoType `+0x5EE`): constructor `0x00710FFC` writes
    /// false; ReadINI (`0x00714382..0x00714396`) reads it with ReadBool
    /// (`0x005295F0`), the current byte as its default across reached rules
    /// layers. A computer house's building of this type shows the current
    /// house its sight and raises a radar event when it opens
    /// (`Simulation::grand_opening`), and releases that sight when it dies or
    /// leaves the map.
    pub reveal_to_all: bool,

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

    /// Immutable projection of the native BuildingType light-field reader.
    /// The process registry owns retained integer defaults across Rules passes.
    building_light: crate::rules::building_light::BuildingLightRules,
    /// Retained Rules-reader inputs for attached building upgrades.
    building_upgrade: crate::rules::building_upgrade::BuildingUpgradeRules,
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
    /// `DefaultToGuardArea=` bool — `TechnoTypeClass+0xD39`, read by
    /// `TechnoTypeClass::ReadINI` (key string `0x00843784`, `ReadBool @
    /// 0x005295F0` at `0x00714F53` with the field as its default); constructor
    /// default `false` (`0x0071153C` writes the zeroed `BL`). An idle
    /// infantryman of such a type takes Area Guard unless it is in a team
    /// (`0x0051CD54..0x0051CD75`).
    pub default_to_guard_area: bool,
    /// `Cyborg=` bool — gamemd's TechnoTypeClass `+0xC8F` "emits damage sparks"
    /// flag that `AI_Update` gates the per-tick damage-Spark prob-roll on.
    /// Verified: only `InfantryTypeClass::ReadINI` writes `+0xC8F` (from `Cyborg=`);
    /// every other leaf (vehicle/building/aircraft) keeps the ctor default 0. So
    /// the AI_Update spark draw fires only for `Cyborg=yes` INFANTRY — a TS legacy
    /// with **no stock-YR users** (the effect is dormant in stock). Default false.
    /// See [`ObjectType::emits_damage_spark`].
    pub cyborg: bool,

    /// InfantryType `UseOwnName=` (`+0xECA`, ReadBool `0x00524634..0x0052464E`,
    /// ctor 0 @ `0x005237DC`): an IFV carrying this infantryman names it in
    /// its turret tooltip (`0x00746BB8`).
    pub use_own_name: bool,
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
    /// ObjectType+238, ctor5F71A6=false; ReadBool5F9521..9549, current default.
    /// Tactical6DBE59 admits the selected object's radial-indicator virtual.
    has_radial_indicator: bool,
    /// BuildingType+1708, ctor=false; ReadBool461030..46104A, current default.
    concentric_radial_indicator: bool,
    /// TechnoType+CD2: ReadInt713FBC stores AL without a clamp;4566EC
    /// sign-extends it. The constructor's default is zero, not General.GapRadius.
    pub gap_radius_in_cells: i8,
    /// TechnoType+CD3 `SuperGapRadiusInCells=`: the gap radius while
    /// super-charged. Mission_Unload's gap arm needs it nonzero
    /// (`0x0044E2B0`) and caches it in `+0x26C` (`0x0044E2DC`).
    pub super_gap_radius_in_cells: i8,
    /// BuildingType+170C: signed ReadInt460C41, current default zero.
    pub psychic_detection_radius: i32,
    /// `SensorArray=` — building sensor-array flag used by GetSensorRange.
    pub sensor_array: bool,
    /// `Sensors=` — generic cloak-sensor flag.
    pub sensors: bool,
    /// `SensorsSight=` — generic sensor coverage; not GetSensorRange4566B0.
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
    pub no_shadow: bool,
    /// `Cloakable=` — copied into Unit/Infantry runtime cloak ability.
    pub cloakable: bool,
    /// `CloakingSpeed=` — signed frame duration between cloak progress steps.
    /// TechnoType710BE4 seeds7; ReadINI71243A..71244C retains it when omitted.
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
    /// Native TechnoType `PitchSpeed` double used by the normal Fly draw gate.
    pub fn fly_pitch_speed(&self) -> f64 {
        f64::from_bits(self.pitch_speed.bits())
    }

    /// Native TechnoType `RollAngle` in radians for normal Fly presentation.
    pub fn fly_roll_angle_radians(&self) -> f64 {
        f64::from_bits(self.roll_angle.bits())
    }

    /// Fly4CF7C5..4CF801 stores RollAngle to binary32 with native chop before
    /// table trig. Negating this stored finite value also preserves the
    /// negative-turn arm's sign-before-store result.
    pub fn fly_roll_angle_f32(&self) -> f32 {
        use crate::util::native_x87::X87Chop53 as X;
        let angle = X::load_f64(self.roll_angle).expect("finite Fly RollAngle");
        f32::from_bits(X::store_f32(angle).expect("finite Fly angle store").bits())
    }

    /// Existing typed PitchAngle value shared with Fly approach and normal
    /// Fly rendering. Rendering converts only at the f32 matrix boundary.
    pub fn fly_pitch_angle_radians(&self) -> f32 {
        self.pitch_angle.to_num::<f32>()
    }

    pub(crate) fn project_image_state(
        &mut self,
        state: &crate::rules::object_image::ObjectImageState,
    ) {
        self.image = state.image().to_string();
        self.shp_image_loads = state.shp_loads().to_vec();
        self.voxel_image_load = state.voxel_load().map(str::to_string);
    }

    /// Immutable load request consumed by the client SHP atlas.
    pub fn shp_image_loads(&self) -> &[crate::rules::object_image::ObjectImageLoad] {
        &self.shp_image_loads
    }

    pub(crate) fn voxel_image_load(&self) -> Option<&str> {
        self.voxel_image_load.as_deref()
    }

    #[cfg(feature = "ares")]
    pub(crate) fn additional_super_weapons(&self) -> &[String] {
        &self.additional_super_weapons
    }

    #[cfg(feature = "ares")]
    pub(crate) fn project_additional_super_weapons(&mut self, names: &[String]) {
        self.additional_super_weapons = names.to_vec();
    }

    /// Phobos `UnitTypeExt`'s gather-overlay lists. Every list is empty for a
    /// type that authors none, which is the constructor state.
    #[cfg(feature = "phobos")]
    pub fn ore_gathering(&self) -> &PhobosOreGathering {
        &self.ore_gathering
    }

    #[cfg(feature = "phobos")]
    pub(crate) fn project_ore_gathering(&mut self, gathering: &PhobosOreGathering) {
        self.ore_gathering = gathering.clone();
    }
    pub(crate) fn emp_rules(&self) -> Option<&crate::rules::ares_emp::EmpTypeRules> {
        self.emp.as_ref()
    }
    pub(crate) fn bind_emp_rules(&mut self, rules: Option<&crate::rules::ares_emp::EmpTypeRules>) {
        self.emp = rules.cloned();
    }

    /// Retained TechnoType+C9E; building crew admission reads this input
    /// separately from the constructed Infantry's survivor marker.
    pub const fn nominal(&self) -> bool {
        self.nominal
    }

    /// The optional Ares manual-action restriction, shared by cursor and
    /// click through the existing action owner. INIs cannot enable it in a
    /// retail-only build.
    pub const fn no_manual_fire(&self) -> bool {
        #[cfg(feature = "ares")]
        {
            self.no_manual_fire
        }
        #[cfg(not(feature = "ares"))]
        {
            false
        }
    }

    /// Optional Ares tracking override (reader1003F47C, constructor1003CF8E).
    pub const fn keep_alive(&self) -> Option<bool> {
        #[cfg(feature = "ares")]
        {
            self.keep_alive
        }
        #[cfg(not(feature = "ares"))]
        {
            None
        }
    }

    /// BuiltAt BuildingTypes ordinals, bound at each reached TechnoType read.
    /// An empty list leaves the source factory unspecified; duplicates are kept.
    pub fn built_at(&self) -> &[i32] {
        #[cfg(feature = "ares")]
        {
            self.factory_owners.built_at()
        }
        #[cfg(not(feature = "ares"))]
        {
            &[]
        }
    }

    /// BuildingExt+126: source factory only admits an explicit BuiltAt match.
    pub fn factory_explicit_only(&self) -> bool {
        #[cfg(feature = "ares")]
        {
            self.explicit_factory.explicit_only()
        }
        #[cfg(not(feature = "ares"))]
        {
            false
        }
    }

    /// Allowed original factory Countries in native list order, duplicates kept.
    pub fn factory_owners(&self) -> &[crate::rules::ruleset::CountryIdx] {
        #[cfg(feature = "ares")]
        {
            self.factory_owners.allowed()
        }
        #[cfg(not(feature = "ares"))]
        {
            &[]
        }
    }

    /// Countries excluded for the same candidate that satisfies FactoryOwners.
    pub fn forbidden_factory_owners(&self) -> &[crate::rules::ruleset::CountryIdx] {
        #[cfg(feature = "ares")]
        {
            self.factory_owners.forbidden()
        }
        #[cfg(not(feature = "ares"))]
        {
            &[]
        }
    }

    /// TypeExt+320: HaveAllPlans followed by Permanent writes the same field.
    pub fn factory_owners_permanent(&self) -> bool {
        #[cfg(feature = "ares")]
        {
            self.factory_owners.permanent()
        }
        #[cfg(not(feature = "ares"))]
        {
            false
        }
    }

    /// Independent TypeExt+321: a live building supplies all factory categories.
    pub fn factory_has_all_plans(&self) -> bool {
        #[cfg(feature = "ares")]
        {
            self.factory_owners.has_all_plans()
        }
        #[cfg(not(feature = "ares"))]
        {
            false
        }
    }

    /// Retained custom palette request; the asset loader remains responsible
    /// for resolving it. Empty ART Palette reads retain this name (717820).
    pub fn custom_palette_name(&self) -> Option<&str> {
        self.custom_palette.as_deref()
    }

    pub(crate) fn project_custom_palette(&mut self, name: Option<&str>) {
        self.custom_palette = name.map(str::to_owned);
    }

    #[cfg(feature = "ares")]
    pub(crate) fn project_factory_owners(
        &mut self,
        state: &crate::rules::native_processing::AresFactoryOwners,
    ) {
        self.factory_owners.clone_from(state);
    }

    #[cfg(feature = "ares")]
    pub(crate) fn factory_owners_hash_state(&self) -> impl std::hash::Hash + '_ {
        (&self.factory_owners, &self.explicit_factory)
    }

    #[cfg(feature = "ares")]
    pub(crate) fn project_explicit_factory(
        &mut self,
        state: &crate::rules::native_processing::AresExplicitFactory,
    ) {
        self.explicit_factory.clone_from(state);
    }

    pub fn armor_id(&self) -> crate::rules::armor::ArmorId {
        self.armor_id
    }

    pub(crate) fn bind_armor(&mut self, armor: crate::rules::armor::ArmorId) {
        self.armor_id = armor;
    }

    /// Phobos `ShieldType=`: the bound `[ShieldTypes]` entry name.
    #[cfg(feature = "phobos")]
    pub(crate) fn shield_type(&self) -> Option<&str> {
        self.shield_type.as_deref()
    }

    /// Phobos `DigitalDisplayTypes=`, in authored list order.
    #[cfg(feature = "phobos")]
    pub fn digital_display_types(&self) -> &[String] {
        &self.digital_display_types
    }

    /// Phobos `DigitalDisplay.Disable=`.
    #[cfg(feature = "phobos")]
    pub fn digital_display_disable(&self) -> bool {
        self.digital_display_disable
    }

    /// Phobos `DigitalDisplay.Health.FakeAtDisguise=`, `None` when inheriting.
    #[cfg(feature = "phobos")]
    pub fn digital_display_health_fake_at_disguise(&self) -> Option<bool> {
        self.digital_display_health_fake_at_disguise
    }

    /// Phobos `PlacementPreview=` (per building type).
    #[cfg(feature = "phobos")]
    pub fn placement_preview(&self) -> bool {
        self.placement_preview
    }

    /// Phobos `PlacementPreview.Shape=`.
    #[cfg(feature = "phobos")]
    pub fn placement_preview_shape(&self) -> Option<&str> {
        self.placement_preview_shape.as_deref()
    }

    /// Phobos `PlacementPreview.ShapeFrame=`.
    #[cfg(feature = "phobos")]
    pub fn placement_preview_shape_frame(&self) -> Option<i32> {
        self.placement_preview_shape_frame
    }

    /// Phobos `PlacementPreview.Offset=` in leptons.
    #[cfg(feature = "phobos")]
    pub fn placement_preview_offset(&self) -> [i32; 3] {
        self.placement_preview_offset
    }

    /// Phobos `PlacementPreview.Remap=`.
    #[cfg(feature = "phobos")]
    pub fn placement_preview_remap(&self) -> bool {
        self.placement_preview_remap
    }

    /// Phobos `PlacementPreview.Palette=`.
    #[cfg(feature = "phobos")]
    pub fn placement_preview_palette(&self) -> Option<&str> {
        self.placement_preview_palette.as_deref()
    }

    /// Phobos `PlacementPreview.Translucency=`; `None` inherits the global.
    #[cfg(feature = "phobos")]
    pub fn placement_preview_translucency(
        &self,
    ) -> Option<crate::rules::phobos_placement_preview::Translucency> {
        self.placement_preview_translucency
    }

    /// Native fixed-ART ShadowIndex, retained by the ordered Type reader.
    pub const fn voxel_shadow_index(&self) -> i32 {
        self.voxel_shadow_index
    }

    pub(crate) fn set_voxel_shadow_index(&mut self, index: i32) {
        self.voxel_shadow_index = index;
    }

    /// Type+C95 also selects Fly's flat shadow-matrix arm (4CFB4B).
    pub const fn is_dropship(&self) -> bool {
        self.is_dropship
    }

    pub const fn building_light(&self) -> crate::rules::building_light::BuildingLightRules {
        self.building_light
    }

    pub(crate) fn project_building_light(
        &mut self,
        light: crate::rules::building_light::BuildingLightRules,
    ) {
        self.building_light = light;
    }

    pub fn building_upgrade(&self) -> &crate::rules::building_upgrade::BuildingUpgradeRules {
        &self.building_upgrade
    }

    pub(crate) fn project_building_upgrade(
        &mut self,
        upgrade: crate::rules::building_upgrade::BuildingUpgradeRules,
    ) {
        self.building_upgrade = upgrade;
    }

    pub(crate) const fn carries_crate(&self) -> bool {
        self.carries_crate
    }

    pub const fn has_radial_indicator(&self) -> bool {
        self.has_radial_indicator
    }

    pub const fn concentric_radial_indicator(&self) -> bool {
        self.concentric_radial_indicator
    }

    /// Building GetSensorRange4566B0. The caller supplies retained+268 and
    /// GetCurrentWeapon70E1A0's resolved range; Option already represents the
    /// null WeaponStruct/WeaponType test70E240. No General.GapRadius fallback.
    pub fn building_sensor_range(&self, gap_supercharged: bool, weapon_range: Option<i32>) -> i32 {
        if self.psychic_detection_radius > 0 {
            self.psychic_detection_radius
        } else if self.gap_generator {
            i32::from(if gap_supercharged {
                self.super_gap_radius_in_cells
            } else {
                self.gap_radius_in_cells
            })
        } else if self.sensor_array || self.cloak_generator {
            i32::from(self.cloak_radius_in_cells)
        } else {
            weapon_range
                .filter(|range| *range > 0)
                .map_or(0, |range| range / 256)
        }
    }

    pub const fn attacks_friendlies(&self) -> bool {
        self.attack_friendlies
    }

    /// Signed PipWrap configuration, before the draw caller's division.
    pub const fn pip_wrap(&self) -> i32 {
        self.pip_wrap
    }

    /// Building4598A0's current-type query: UnitAbsorb OR InfantryAbsorb.
    /// Class-specific entry and lifecycle admission remain with each caller.
    /// Original instructions/readers: procedural_drawing_oracle/absorber_pips.
    pub const fn absorbs_passengers(&self) -> bool {
        self.unit_absorb || self.infantry_absorb
    }

    pub const fn pips_draw_for_all(&self) -> bool {
        self.pips_draw_for_all
    }

    /// InfantryType+DFC: passenger frame in Building PIPS or mobile PIPS2.
    pub const fn pip(&self) -> u32 {
        self.infantry_pips.pip
    }

    /// InfantryType+E00: raw PIPS frame for this infantry in a garrison.
    /// Admitted missing-key passes can produce1..5 as well as person frames.
    pub const fn occupy_pip(&self) -> u32 {
        self.infantry_pips.occupy_pip
    }

    pub(crate) fn project_infantry_pips(&mut self, pips: InfantryPips) {
        self.infantry_pips = pips;
    }

    pub(crate) fn project_sequence_sound_reads(
        &mut self,
        reads: &[crate::rules::infantry_sequence::SequenceSoundRead],
    ) {
        self.sequence_sound_reads = reads.to_vec();
    }

    /// Resolve the projected pairs through `find` (VocClass::FindByName).
    pub(crate) fn bind_sequence_sounds(&mut self, find: impl FnMut(&str) -> Option<String>) {
        self.sequence_sounds =
            crate::rules::infantry_sequence::bind_sequence_sounds(&self.sequence_sound_reads, find);
    }

    /// The sounds of DoType `action`'s record, in slot order.
    pub fn sequence_sounds(
        &self,
        action: i32,
    ) -> impl Iterator<Item = &crate::rules::infantry_sequence::SequenceSound> {
        self.sequence_sounds
            .iter()
            .filter(move |sound| i32::from(sound.action) == action)
    }

    pub(crate) fn project_infantry_infiltration(&mut self, flags: InfantryInfiltration) {
        self.infiltrate = flags.infiltrate;
        self.c4 = flags.c4;
        self.engineer = flags.engineer;
        self.agent = flags.agent;
    }

    /// TechnoType GetPipMax716290, virtual+74. Signed configuration capacity;
    /// callers own allocation/fill bounds. Uses the live bound base weapon0,
    /// not a capture manager's retained admission capacity or elite weapon.
    pub fn pip_max(&self, rules: &crate::rules::ruleset::RuleSet) -> i32 {
        match self.pip_scale {
            PipScale::None => 0,
            PipScale::Ammo => self.ammo,
            PipScale::Tiberium => {
                if self.category == ObjectCategory::Infantry {
                    3
                } else {
                    5
                }
            }
            PipScale::Passengers => self.passengers,
            PipScale::Power => 10,
            // Native7162B0 dereferences an admitted primary. Preserve the
            // existing host's safe0 for an invalid missing binding; stock
            // MindControl has a bound weapon and follows the original read.
            PipScale::MindControl => self
                .weapon_at(0)
                .and_then(|name| rules.weapon(name))
                .map_or(0, |weapon| weapon.damage),
        }
    }

    /// The authoritative BuildingType+EB8 class, including non-production
    /// RTTI names. Consumers must keep their own native comparisons.
    pub const fn factory_rtti(&self) -> i32 {
        self.factory_rtti
    }

    /// Native nonzero-Factory gates (HasOccurred17, AI, quarry and exit).
    pub const fn has_factory(&self) -> bool {
        self.factory_rtti != 0
    }

    /// Exact Type comparisons used by FindFactory, exit arms and rally.
    /// Object RTTIs (Unit1/Infantry15/...) are deliberately not type RTTIs.
    pub const fn factory_type(&self) -> Option<FactoryType> {
        FactoryType::from_rtti(self.factory_rtti)
    }

    pub(crate) fn target_coord_offset(&self) -> [i32; 3] {
        self.target_coord_offset
    }

    /// Publish the value retained by the native type-processing owner.
    pub(crate) fn set_target_coord_offset(&mut self, offset: [i32; 3]) {
        debug_assert_eq!(self.category, ObjectCategory::Building);
        self.target_coord_offset = offset;
    }

    /// Publish the Building-family ReadINI owner's retained smoke locations.
    pub(crate) fn set_half_damage_smoke_locations(
        &mut self,
        location1: [i32; 3],
        location2: [i32; 3],
    ) {
        debug_assert_eq!(self.category, ObjectCategory::Building);
        self.half_damage_smoke_location1 = location1;
        self.half_damage_smoke_location2 = location2;
    }

    pub fn voice_harvest(&self) -> Option<&str> {
        self.voice_harvest.as_deref()
    }

    pub fn voice_move(&self) -> &[String] {
        &self.voice_move
    }

    pub fn voice_enter(&self) -> Option<&str> {
        self.voice_enter.as_deref()
    }

    pub fn voice_capture(&self) -> Option<&str> {
        self.voice_capture.as_deref()
    }

    pub fn voice_attack(&self) -> &[String] {
        &self.voice_attack
    }

    /// VoiceAttack7090F8..70914A reads primary for index0, otherwise
    /// secondary; its caller supplies the raw750010 elite predicate.
    pub fn voice_attack_override(&self, weapon_index: i32, elite: bool) -> Option<&str> {
        self.voice_attack_overrides[usize::from(weapon_index != 0) * 2 + usize::from(elite)]
            .as_deref()
    }

    /// Bind the existing native type reader's list and scalar slots against
    /// the one fixed catalog and its reached processed-section history.
    pub(crate) fn bind_order_voice_references(
        &mut self,
        section: Option<&IniSection>,
        sounds: &SoundRegistry,
    ) {
        self.voice_move = section.map_or_else(Vec::new, |section| {
            sounds.read_rules_sound_list(section, "VoiceMove")
        });
        self.voice_attack = section.map_or_else(Vec::new, |section| {
            sounds.read_rules_sound_list(section, "VoiceAttack")
        });
        self.voice_enter =
            section.and_then(|section| sounds.read_rules_reference(section, "VoiceEnter"));
        self.voice_capture =
            section.and_then(|section| sounds.read_rules_reference(section, "VoiceCapture"));
        self.voice_attack_overrides = [
            "VoicePrimaryWeaponAttack",
            "VoicePrimaryEliteWeaponAttack",
            "VoiceSecondaryWeaponAttack",
            "VoiceSecondaryEliteWeaponAttack",
        ]
        .map(|key| section.and_then(|section| sounds.read_rules_reference(section, key)));
    }

    pub fn voice_deploy(&self) -> Option<&str> {
        self.voice_deploy.as_deref()
    }

    pub fn voice_undeploy(&self) -> Option<&str> {
        self.voice_undeploy.as_deref()
    }

    /// Original scalar readers71363D/713769/7137A5; constructor -1 is
    /// represented by None. Reuse the processed pass/current-ID owner.
    pub(crate) fn bind_deploy_voice_references(
        &mut self,
        section: Option<&IniSection>,
        sounds: &SoundRegistry,
    ) {
        self.voice_harvest =
            section.and_then(|section| sounds.read_rules_reference(section, "VoiceHarvest"));
        self.voice_deploy =
            section.and_then(|section| sounds.read_rules_reference(section, "VoiceDeploy"));
        self.voice_undeploy =
            section.and_then(|section| sounds.read_rules_reference(section, "VoiceUndeploy"));
    }

    pub fn damage_sound(&self) -> Option<&str> {
        self.damage_sound.as_deref()
    }

    pub fn secret_lab(&self) -> &crate::rules::secret_lab::SecretLabRules {
        &self.secret_lab
    }

    pub(crate) fn set_secret_lab(&mut self, rules: crate::rules::secret_lab::SecretLabRules) {
        self.secret_lab = rules;
    }

    pub fn create_sound(&self) -> Option<&str> {
        self.create_sound.as_deref()
    }

    /// Bind TechnoType+534/+538 in original712E75..712F07 read order,
    /// using only passes where this type existed in the native processing order.
    pub(crate) fn bind_scalar_sound_references(
        &mut self,
        section: Option<&IniSection>,
        sounds: &SoundRegistry,
    ) {
        self.create_sound =
            section.and_then(|section| sounds.read_rules_reference(section, "CreateSound"));
        self.damage_sound =
            section.and_then(|section| sounds.read_rules_reference(section, "DamageSound"));
    }

    /// `Weapon[index]` (`TechnoTypeClass+0x898 + index*0x1C`): the weapon in
    /// base slot `index`, or `None` for an empty slot or an index past the
    /// array. `TechnoTypeClass::ReadINI @ 0x007128B2` writes `Weapon1=` and
    /// `Primary=` to the same `+0x898`, so the `TurretCount` branch is decided
    /// while Rules are processed (`native_processing::WeaponSlots`); no reader
    /// has to know which INI keys a type authored.
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
    /// `native_processing::WeaponSlots`. Read it as the native field, not as
    /// "the `Primary=` key": for `[SREF]` and `[YAGGUN]` it holds the
    /// `Weapon1=` weapon even though neither section authors a live `Primary=`.
    pub fn primary(&self) -> Option<&str> {
        self.weapon_at(WEAPON_SLOT_PRIMARY)
    }

    /// Base weapon-array slot 1 (`TechnoTypeClass+0x8B4`), gamemd's `Secondary`
    /// field. `Secondary=` or `Weapon2=`, same storage.
    #[cfg(any(test, feature = "test-support"))]
    pub fn secondary(&self) -> Option<&str> {
        self.weapon_at(WEAPON_SLOT_SECONDARY)
    }

    /// Whether the type accepts an attack-move order (type vtable `+0xA4`).
    /// `TechnoTypeClass::Can_Attack_Move @ 0x00711E90`, which Infantry
    /// (`0x005247C0`) and Unit (`0x007473D0`) types jump to, answers
    /// `Primary (+0x898) != null && !PreventAttackMove (+0x6C8)`; Aircraft
    /// (`0x0041CB60`) and Building (`0x00465920`) types answer false.
    ///
    /// The raw slot-0 field, not `GetWeapon`: the elite tier does not apply,
    /// and a `TurretCount>0` type passes on its `Weapon1=` ([`Self::primary`]),
    /// so a Prism Tank attack-moves although its `Primary=` is commented out.
    /// The weapon factory answers null for `none` and `<none>`. `Secondary` is
    /// never read, so a secondary-only type is refused. The class follows list
    /// membership: the `[VehicleTypes]` helicopters (`SHAD`, `HIND`, `SCHP`,
    /// `SCHD`) are units here, refused by their `PreventAttackMove=yes`.
    pub fn can_attack_move(&self) -> bool {
        matches!(
            self.category,
            ObjectCategory::Infantry | ObjectCategory::Vehicle
        ) && !self.prevent_attack_move
            && self
                .primary()
                .is_some_and(|primary| !is_native_none_type_name(primary))
    }

    /// Elite weapon-array slot 0 (`TechnoTypeClass+0xA94`), gamemd's
    /// `ElitePrimary` field: `ElitePrimary=` or `EliteWeapon1=`. Replaces
    /// [`Self::primary`] when the unit is at Elite tier (veterancy >= 200) and
    /// this slot names a weapon; Veteran tier (100..199) does NOT swap.
    #[cfg(any(test, feature = "test-support"))]
    pub fn elite_primary(&self) -> Option<&str> {
        self.elite_weapon_at(WEAPON_SLOT_PRIMARY)
    }

    /// Elite weapon-array slot 1 (`TechnoTypeClass+0xAB0`), gamemd's
    /// `EliteSecondary` field: `EliteSecondary=` or `EliteWeapon2=`. Replaces
    /// [`Self::secondary`] at Elite tier under the same rule.
    #[cfg(test)]
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

    /// ReadOwners4750D0 / Type reader714531: absent/empty ReadString keeps
    /// the constructor's -1 (711633), but nonempty comma-only input writes0.
    /// CanBuild4F79E4 and FirstBuildable505230 both test this raw mask.
    pub(crate) fn required_houses_mask(&self, rules: &crate::rules::ruleset::RuleSet) -> u32 {
        if self.required_houses.is_empty() && !self.required_houses_explicitly_empty {
            u32::MAX
        } else {
            rules.house_type_mask(&self.required_houses)
        }
    }

    /// The AI base planning side filter: `AIBasePlanningSide=` (`+0x6D0`) is
    /// -1 or the house's side (`HouseTypeClass+0xBC`), as
    /// `FirstBuildableFromArray @ 0x005051E0` and `AI_BuildWalls @ 0x0050C340`
    /// test it.
    pub fn planned_for_side(&self, side_index: i32) -> bool {
        self.ai_base_planning_side == -1 || self.ai_base_planning_side == side_index
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
        self.undeploys_into.is_some() && self.foundation.dimensions() == (1, 1)
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
            self.factory_type(),
            Some(FactoryType::InfantryType | FactoryType::UnitType)
        ) || self.unit_repair
            || self.cloning
    }

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

        let required_tokens = section.read_list("RequiredHouses", 0x80);
        let required_houses_explicitly_empty = required_tokens.as_ref().is_some_and(Vec::is_empty);
        let required_houses: Vec<String> = required_tokens
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
            my_effectiveness_coefficient: present_double("MyEffectivenessCoefficient"),
            target_effectiveness_coefficient: present_double("TargetEffectivenessCoefficient"),
            target_special_threat_coefficient: present_double("TargetSpecialThreatCoefficient"),
            target_strength_coefficient: present_double("TargetStrengthCoefficient"),
            target_distance_coefficient: present_double("TargetDistanceCoefficient"),
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
            digital_display_disable: section.read_bool("DigitalDisplay.Disable", false),
            #[cfg(feature = "phobos")]
            digital_display_health_fake_at_disguise: section
                .read_phobos_nullable_bool("DigitalDisplay.Health.FakeAtDisguise", None),
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
            // Double fields (`0x007124A3..0x007124E5`).
            weight: SimFixed::from_num(section.read_double("Weight", 1.0) as f32),
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
                        SimFixed::from_num((degrees as f32) * (std::f32::consts::PI / 180.0))
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
            aibuild_limit: if crate::rules::NPATCH_ENABLED {
                section.read_int("AIBuildLimit", i32::MAX)
            } else {
                i32::MAX
            },
            ai_planning_count: if crate::rules::NPATCH_ENABLED {
                building_int(section, category, "AIPlanningCount") & 0xFF
            } else {
                0
            },
            requires_stolen_allied_tech: section.read_bool("RequiresStolenAlliedTech", false),
            requires_stolen_soviet_tech: section.read_bool("RequiresStolenSovietTech", false),
            requires_stolen_third_tech: section.read_bool("RequiresStolenThirdTech", false),
            // Constructor value; only the ordered Process owner reads it.
            image: id.to_owned(),
            shp_image_loads: Vec::new(),
            voxel_image_load: None,
            power: section.read_int("Power", 0),
            // Only the ordered Rules processor reads it: a later layer's
            // non-negative value keeps an earlier drain.
            extra_power: Default::default(),
            // Building ctor45DF13 initializes +EF0=0 (1x1). ReadINI461225
            // reads Foundation from ART only, never this RULES section.
            // Pass processing projects the shared reader; ART binding does not.
            foundation: crate::rules::foundation::Foundation::default(),
            pixel_selection_bracket_delta: section.read_int("PixelSelectionBracketDelta", 0),
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
            crush_sound: section.read_name("CrushSound", 0x80).map(str::to_owned),
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
            #[cfg(feature = "ares")]
            no_manual_fire: section.read_ares_bool("NoManualFire", false),
            #[cfg(feature = "ares")]
            keep_alive: section.read_ares_nullable_bool("KeepAlive", None),
            #[cfg(feature = "ares")]
            factory_owners: Default::default(),
            #[cfg(feature = "ares")]
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
            self_healing: section.read_bool("SelfHealing", false),
            // Constructor null; only the ordered Process owner reads it.
            death_weapon: None,
            // A float field (`FSTP dword` at `0x0071232B`).
            death_weapon_damage_modifier: section.read_float("DeathWeaponDamageModifier", 1.0),
            super_weapon: section.read_name("SuperWeapon", 0x20).map(str::to_owned),
            super_weapon2: section.read_name("SuperWeapon2", 0x20).map(str::to_owned),
            #[cfg(feature = "ares")]
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
            spawns_number: section.read_int("SpawnsNumber", 0),
            spawn_regen_rate: section.read_int("SpawnRegenRate", 0).max(0) as u32,
            spawn_reload_rate: section.read_int("SpawnReloadRate", 0).max(0) as u32,
            missile_spawn: section.read_bool("MissileSpawn", false),
            no_spawn_alt: section.read_bool("NoSpawnAlt", false),

            // Slave Miner / economy fields
            // Constructor null; only the ordered Process owner reads it.
            enslaves: None,
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
            teleporter: section.read_bool("Teleporter", false),
            move_to_shroud: section.read_bool("MoveToShroud", category != ObjectCategory::Aircraft),
            hover_attack: section.read_bool("HoverAttack", false),
            balloon_hover: section.read_bool("BalloonHover", false),
            target_laser: section.read_bool("TargetLaser", false),
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
            tilts_when_crushes: section.read_bool("TiltsWhenCrushes", true),
            omni_crusher: section.read_bool("OmniCrusher", false),
            omni_crush_resistant: section.read_bool("OmniCrushResistant", false),
            immune_to_radiation: section.read_bool("ImmuneToRadiation", false),
            damage_self: section.read_bool("DamageSelf", false),
            immune: section.read_bool("Immune", false),
            type_immune: section.read_bool("TypeImmune", false),
            emp: None,
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
            gunner: section.read_bool("Gunner", false),
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
            max_number_occupants: section.read_int("MaxNumberOccupants", 0).max(0) as u32,
            occupier: section.read_bool("Occupier", false),
            assaulter: section.read_bool("Assaulter", false),
            vehicle_thief: section.read_bool("VehicleThief", false),
            // Constructor nulls; only the ordered Process owner reads them.
            occupy_weapon: None,
            elite_occupy_weapon: None,
            // Constructor values; only the ordered Process owner reads them.
            infantry_pips: InfantryPips::default(),
            sequence_sound_reads: Vec::new(),
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
            is_selectable_combatant: section.read_bool("IsSelectableCombatant", false),
            // The constructor's value; ART supplies the key (`project_art_data`).
            can_be_hidden: true,
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
            debris_maximums: section.read_int_list("DebrisMaximums").unwrap_or_default(),
            debris_anims: Vec::new(),
            close_range: section.read_bool("CloseRange", false),
            stupid_hunt: section.read_bool("StupidHunt", false),
            default_to_guard_area: section.read_bool("DefaultToGuardArea", false),
            cyborg: section.read_bool("Cyborg", false),
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
            gap_radius_in_cells: section.read_int("GapRadiusInCells", 0) as i8,
            super_gap_radius_in_cells: section.read_int("SuperGapRadiusInCells", 0) as i8,
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

    #[cfg(any(test, feature = "test-support"))]
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
#[path = "vehicle_speed_type_tests.rs"]
mod vehicle_speed_type_tests;

#[cfg(test)]
#[path = "infantry_pip_tests.rs"]
mod infantry_pip_tests;

#[cfg(test)]
#[path = "ammo_pip_tests.rs"]
mod ammo_pip_tests;

#[cfg(test)]
#[path = "absorber_pip_tests.rs"]
mod absorber_pip_tests;

#[cfg(test)]
mod tests {
    #[test]
    fn fly_bank_rules_read_stock_values_and_keep_each_pass_sentinels() {
        use crate::rules::ini_parser::IniFile;
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        use crate::rules::ruleset::RuleSet;

        let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        for id in ["BPLN", "SPYP"] {
            let object = retail.rules.object(id).expect("stock aircraft type");
            assert_eq!(object.fly_pitch_speed(), 0.25, "{id} default PitchSpeed");
            assert_eq!(object.fly_pitch_angle_radians(), 0.0, "{id} PitchAngle");
            assert!(
                (object.fly_roll_angle_radians() - 30.0_f64.to_radians()).abs() < 1.0e-12,
                "{id} constructor RollAngle"
            );
        }

        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[VehicleTypes]\n0=BPLN\n[BPLN]\nPitchSpeed=0.5\nRollAngle=45\nPitchAngle=12\n",
        ));
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[BPLN]\nRollAngle=-1\nPitchAngle=-1\n"),
        );
        let rules = RuleSet::from_rules_layers(&layers).expect("production layered rules");
        let object = rules.object("BPLN").expect("registered aircraft type");
        assert_eq!(object.fly_pitch_speed(), 0.5, "omitted later key retains");
        assert!(
            (object.fly_roll_angle_radians() - 45.0_f64.to_radians()).abs() < 1.0e-12,
            "later -1 retains the earlier angle"
        );
        assert!(
            (object.fly_pitch_angle_radians() - 12.0_f32.to_radians()).abs() < 1.0e-5,
            "later PitchAngle=-1 retains the earlier angle"
        );
    }

    #[test]
    fn half_damage_smoke_location_retains_stock_battle_building_values() {
        let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let authored = retail
            .authored_rules
            .section("GAREFN")
            .expect("stock Allied refinery section");
        assert_eq!(
            authored.read_coord3("HalfDamageSmokeLocation1", [9, 9, 9]),
            [0, 0, 0]
        );
        assert!(!authored.is_present("HalfDamageSmokeLocation2"));

        let building = retail
            .rules
            .object("GAREFN")
            .expect("stock Allied refinery BuildingType");
        assert_eq!(building.category, super::ObjectCategory::Building);
        assert_eq!(building.half_damage_smoke_location1, [0, 0, 0]);
        assert_eq!(building.half_damage_smoke_location2, [0, 0, 0]);
    }

    #[test]
    fn half_damage_smoke_location_retains_native_current_values_across_rules_layers() {
        use crate::rules::ini_parser::IniFile;
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        use crate::rules::ruleset::RuleSet;

        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[BuildingTypes]\n0=HUT\n[VehicleTypes]\n0=HUTVEH\n[HUT]\nHalfDamageSmokeLocation1=11,22,33\n\
             HalfDamageSmokeLocation2=-4,5,6\n[HUTVEH]\n\
             HalfDamageSmokeLocation1=91,92,93\nHalfDamageSmokeLocation2=-94,95,96\n",
        ));
        layers.push(
            RulesLayerKind::GameMode,
            IniFile::from_str("[HUT]\nHalfDamageSmokeLocation1=-7,8,9\n"),
        );
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[HUT]\nStrength=100\n"),
        );

        let rules = RuleSet::from_rules_layers(&layers).expect("production layered rules");
        let building = rules.object("HUT").expect("registered BuildingType");
        assert_eq!(building.half_damage_smoke_location1, [-7, 8, 9]);
        assert_eq!(building.half_damage_smoke_location2, [-4, 5, 6]);
        let vehicle = rules.object("HUTVEH").expect("registered VehicleType");
        assert_eq!(vehicle.category, super::ObjectCategory::Vehicle);
        assert_eq!(vehicle.half_damage_smoke_location1, [0; 3]);
        assert_eq!(vehicle.half_damage_smoke_location2, [0; 3]);
    }

    #[test]
    fn invisible_in_game_forces_building_flags_and_retains_them_across_layers() {
        use crate::rules::ini_parser::IniFile;
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        use crate::rules::ruleset::RuleSet;

        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[BuildingTypes]\n0=HIDDEN\n[VehicleTypes]\n0=VISIBLE\n\
             [HIDDEN]\nInvisible=no\nInvisibleInGame=yes\nRadarVisible=yes\n\
             [VISIBLE]\nInvisible=no\nInvisibleInGame=yes\nRadarVisible=yes\n",
        ));
        let flags = |layers: &RulesLayerStack, name: &str| {
            let rules = RuleSet::from_rules_layers(layers).unwrap();
            let object = rules.object(name).unwrap();
            (
                object.invisible_in_game,
                object.invisible,
                object.radar_visible,
            )
        };
        assert_eq!(flags(&layers, "HIDDEN"), (true, true, false));
        assert_eq!(flags(&layers, "VISIBLE"), (true, false, true));

        // Clearing the building-only key does not undo earlier derived writes.
        // Invalid base-reader values retain those writes as their defaults.
        layers.push(
            RulesLayerKind::GameMode,
            IniFile::from_str(
                "[HIDDEN]\nInvisibleInGame=no\nInvisible=invalid\nRadarVisible=invalid\n",
            ),
        );
        assert_eq!(flags(&layers, "HIDDEN"), (false, true, false));
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[HIDDEN]\nInvisible=no\nRadarVisible=yes\n"),
        );
        assert_eq!(flags(&layers, "HIDDEN"), (false, false, true));

        let Some(retail) =
            crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
        else {
            return;
        };
        let lamp = retail.rules.object("INGALITE").unwrap();
        assert!(lamp.invisible_in_game && lamp.invisible);
        assert!(!lamp.radar_visible);
    }

    /// `RevealToAll=` through the production reader on the retail layers.
    #[test]
    #[ignore = "slow; run when changing what it covers"]
    fn reveal_to_all_reads_the_seven_stock_types() {
        let Some(retail) =
            crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
        else {
            return;
        };
        let mut types: Vec<_> = retail
            .rules
            .all_objects()
            .filter(|object| object.reveal_to_all)
            .map(|object| object.id.as_str())
            .collect();
        types.sort_unstable();
        assert_eq!(
            types,
            [
                "GACSPH", "GADUMY", "GAWEAT", "NAIRON", "NAMISL", "YAGNTC", "YAPPET"
            ]
        );
    }

    #[test]
    fn combatant_type_default_and_retained_layer_reads_match_original_execution() {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        use crate::rules::ruleset::RuleSet;
        use std::fmt::Write;

        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/input_oracle/selection_navigation.json",
        ))
        .unwrap();
        let family = &native["combatant_type_histories"];
        assert_eq!(family["key"], "IsSelectableCombatant");
        assert_eq!(family["field_offset"], 0xDBC);
        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[VehicleTypes]\n0=AUTHORED\n[AUTHORED]\nStrength=100\n",
        ));
        assert!(
            !RuleSet::from_rules_layers(&layers)
                .unwrap()
                .object("AUTHORED")
                .unwrap()
                .is_selectable_combatant
        );
        for constructor in family["constructors"].as_array().unwrap() {
            assert_eq!(constructor["value_before"], 0xA5);
            assert_eq!(
                constructor["value_after"], false,
                "original {} constructor",
                constructor["type_id"]
            );
        }
        let mut previous = false;
        for row in family["authored_history"].as_array().unwrap() {
            assert_eq!(row["before"].as_bool().unwrap(), previous);
            let mut text = String::new();
            for (section, values) in row["sections"].as_object().unwrap() {
                writeln!(text, "[{section}]").unwrap();
                for (key, value) in values.as_object().unwrap() {
                    writeln!(text, "{key}={}", value.as_str().unwrap()).unwrap();
                }
            }
            layers.push(RulesLayerKind::Scenario, IniFile::from_str(&text));
            let rules =
                RuleSet::from_rules_layers(&layers).expect("native retained-reader history");
            previous = rules.object("AUTHORED").unwrap().is_selectable_combatant;
            assert_eq!(
                previous,
                row["after"].as_bool().unwrap(),
                "original {}",
                row["id"]
            );
            for read in row["reads"].as_array().unwrap() {
                assert_eq!(read["key"], "IsSelectableCombatant");
                assert_eq!(read["reader"], "0x5295f0");
            }
        }
    }

    #[test]
    #[ignore = "slow; run when changing what it covers"]
    fn combatant_retail_rules_mode_map_reads_match_original_execution() {
        let Some(retail) =
            crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
        else {
            return;
        };
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/input_oracle/selection_navigation.json",
        ))
        .unwrap();
        let rows = native["combatant_type_histories"]["retail_histories"]
            .as_array()
            .unwrap();
        assert_eq!(rows.len(), 8);
        for row in rows {
            let name = row["type_id"].as_str().unwrap();
            assert_eq!(
                retail.rules.object(name).unwrap().is_selectable_combatant,
                row["final"].as_bool().unwrap(),
                "{name}: physical retail startup -> Battle -> XMP03T4 layers",
            );
        }
    }

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
    fn pip_scale_native_names_default_capacity_and_layer_retention() {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        use crate::rules::ruleset::RuleSet;
        // Reader474940's file-backed table81B9B0, through ReadString32.
        for (raw, expected) in [
            ("Ammo", PipScale::Ammo),
            ("tIbErIuM", PipScale::Tiberium),
            ("Passengers", PipScale::Passengers),
            ("Power", PipScale::Power),
            ("mInDcOnTrOl", PipScale::MindControl),
            ("Unknown", PipScale::None),
            ("5", PipScale::None),
            ("MindControl,Ammo", PipScale::None),
        ] {
            let ini = IniFile::from_str(&format!("[MIND]\nPipScale={raw}\n"));
            let object = ObjectType::from_ini_section(
                "MIND",
                ini.section("MIND").unwrap(),
                ObjectCategory::Vehicle,
            );
            assert_eq!(object.pip_scale, expected, "{raw}");
        }
        let ini = IniFile::from_str(&format!(
            "[MIND]\nPipScale=MindControl{}invalid\n",
            " ".repeat(20)
        ));
        assert_eq!(
            ObjectType::from_ini_section(
                "MIND",
                ini.section("MIND").unwrap(),
                ObjectCategory::Vehicle
            )
            .pip_scale,
            PipScale::MindControl,
            "copy truncates to31 bytes BEFORE trim/name lookup"
        );
        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[VehicleTypes]\n0=MIND\n[MIND]\nPipScale=MindControl\n",
        ));
        for patch in ["Strength=500", "PipScale=", "PipScale=   ", "pipscale=Ammo"] {
            layers.push(
                RulesLayerKind::Scenario,
                IniFile::from_str(&format!("[MIND]\n{patch}\n")),
            );
            assert_eq!(
                RuleSet::from_rules_layers(&layers)
                    .unwrap()
                    .object("MIND")
                    .unwrap()
                    .pip_scale,
                PipScale::MindControl,
                "absent/empty/exact-case key: {patch}"
            );
        }
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[MIND]\nPipScale=Unknown\n"),
        );
        assert_eq!(
            RuleSet::from_rules_layers(&layers)
                .unwrap()
                .object("MIND")
                .unwrap()
                .pip_scale,
            PipScale::None
        );
        let default_ini = IniFile::from_str("[MIND]\nStrength=500\n");
        assert_eq!(
            ObjectType::from_ini_section(
                "MIND",
                default_ini.section("MIND").unwrap(),
                ObjectCategory::Vehicle
            )
            .pip_scale,
            PipScale::None
        );
    }

    #[test]
    fn retail_mastermind_uses_mind_control_pips_and_current_primary_capacity() {
        let Some(fixture) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let object = fixture.rules.object("MIND").unwrap();
        assert_eq!(object.pip_scale, PipScale::MindControl);
        assert_eq!(
            fixture
                .rules
                .weapon(object.weapon_at(0).unwrap())
                .unwrap()
                .damage,
            3
        );
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

    use super::*;
    use crate::rules::ini_parser::IniFile;

    #[test]
    fn test_parse_vehicle() {
        // TechLevel and the weapon slots come from the ordered Rules owner.
        let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=MTNK\n\
             [MTNK]\nName=Grizzly Battle Tank\nCost=700\nStrength=300\n\
             Armor=heavy\nSpeed=6\nSight=6\nTechLevel=2\n\
             AccelerationFactor=.03\nDeaccelerationFactor=.002\n\
             Owner=Americans,Alliance\nRequiredHouses=Americans\n\
             Prerequisite=GAWEAP\nPrimary=105mm\n",
        ))
        .unwrap();
        let obj = rules.object("MTNK").unwrap();

        assert_eq!(obj.id, "MTNK");
        assert_eq!(obj.category, ObjectCategory::Vehicle);
        assert_eq!(obj.name, Some("Grizzly Battle Tank".to_string()));
        assert_eq!(obj.cost, 700);
        assert_eq!(obj.strength, 300);
        assert_eq!(obj.armor, "heavy");
        assert_eq!(obj.speed, 6);
        assert_eq!(obj.accel_factor.bits(), 0x3f9e_b851_e000_0000);
        assert_eq!(obj.decel_factor.bits(), 0x3f60_624d_e000_0000);
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
        let mut obj = ObjectType::from_ini_section(
            "CMIN",
            ini.section("CMIN").unwrap(),
            ObjectCategory::Vehicle,
        );
        let sounds = SoundRegistry::from_ini(&IniFile::from_str(
            "[SoundList]\n0=ChronoMinerHarvest\n1=ChronoMinerMove\n2=ChronoMinerReturn\n",
        ));
        obj.bind_deploy_voice_references(ini.section("CMIN"), &sounds);
        obj.bind_order_voice_references(ini.section("CMIN"), &sounds);
        assert_eq!(obj.voice_move(), ["ChronoMinerMove"]);
        assert_eq!(obj.voice_harvest(), Some("ChronoMinerHarvest"));
        assert_eq!(obj.voice_enter(), Some("ChronoMinerReturn"));

        // Absent keys default to None (e.g. a plain tank).
        let none_ini: IniFile = IniFile::from_str("[MTNK]\nName=Grizzly\n");
        let none_obj = ObjectType::from_ini_section(
            "MTNK",
            none_ini.section("MTNK").unwrap(),
            ObjectCategory::Vehicle,
        );
        assert_eq!(none_obj.voice_harvest(), None);
        assert_eq!(none_obj.voice_enter(), None);
    }

    /// Capture708DC0 consumes the fixed type slot before its Enter fallback.
    /// Stock engineers register this cue in SOUNDMD.
    #[test]
    fn gsi_06_parses_voice_capture_slot() {
        let ini: IniFile = IniFile::from_str(
            "[ENGINEER]\nName=Engineer\nVoiceMove=EngAllMove\n\
             VoiceEnter=EngAllMove\nVoiceCapture=EngAllAttackCommand\n",
        );
        let mut obj = ObjectType::from_ini_section(
            "ENGINEER",
            ini.section("ENGINEER").unwrap(),
            ObjectCategory::Infantry,
        );
        let sounds = SoundRegistry::from_ini(&IniFile::from_str(
            "[SoundList]\n0=EngAllMove\n1=EngAllAttackCommand\n",
        ));
        obj.bind_order_voice_references(ini.section("ENGINEER"), &sounds);
        assert_eq!(obj.voice_capture(), Some("EngAllAttackCommand"));

        // Absent key stays None, which is the retail "fall back to Enter" case.
        let none_ini: IniFile = IniFile::from_str("[E1]\nName=GI\n");
        let none_obj = ObjectType::from_ini_section(
            "E1",
            none_ini.section("E1").unwrap(),
            ObjectCategory::Infantry,
        );
        assert_eq!(none_obj.voice_capture(), None);
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
        assert_eq!(obj.accel_factor.bits(), 0x3f9e_b851_eb85_1eb8);
        assert_eq!(obj.decel_factor.bits(), 0x3f60_624d_d2f1_a9fc);
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
    fn test_parse_undeploys_into() {
        let rules = rules(
            "[BuildingTypes]\n0=GACNST\n1=MODDEPLOY\n\
             [GACNST]\nUndeploysInto=AMCV\nFoundation=4x4\n\
             [MODDEPLOY]\nUndeploysInto=MODUNIT\nFoundation=1x1\n",
        );
        let mut obj = rules.object("GACNST").unwrap().clone();
        assert_eq!(obj.undeploys_into, Some("AMCV".to_string()));
        assert_eq!(obj.deploys_into, None);
        assert!(
            obj.is_1x1_with_undeploy(),
            "constructor remains1x1 until ART reads"
        );
        // This gate consumes a loaded native field; ART input provenance and
        // every table dimension are tested by building_foundation_pass_matches_original_art_reads.
        obj.foundation = "4x4".into();
        assert!(!obj.is_1x1_with_undeploy());

        assert!(rules.object("MODDEPLOY").unwrap().is_1x1_with_undeploy());
    }

    /// Phobos `UnitTypeExt`'s three `OreGathering.*` vectors through the
    /// production layered reader: an absent key retains the whole list, a
    /// present one replaces it, and an `Anims=` token the AnimType array lacks
    /// drops out instead of holding a null slot.
    #[test]
    #[cfg(feature = "phobos")]
    fn ore_gathering_lists_replace_retain_and_drop_unknown_anims() {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        use crate::rules::ruleset::RuleSet;

        let base = IniFile::from_str(
            "[VehicleTypes]\n0=HARV\n[Animations]\n0=GATHA\n\
             [HARV]\nHarvester=yes\nOreGathering.Anims=GATHA,NOPE,GATHB\n\
             OreGathering.Tiberiums=0,1\nOreGathering.FramesPerDir=10,20\n",
        );
        let rules = RuleSet::from_rules_layers(&RulesLayerStack::new(base))
            .expect("authored phobos gather lists");
        let gathering = rules.object("HARV").expect("HARV").ore_gathering();
        assert_eq!(gathering.anims, ["GATHA"], "the unknown token drops out");
        assert_eq!(gathering.tiberiums, [0, 1]);
        assert_eq!(gathering.frames_per_dir, [10, 20]);

        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[VehicleTypes]\n0=HARV\n[Animations]\n0=GATHA\n\
             [HARV]\nHarvester=yes\nOreGathering.Anims=GATHA\n\
             OreGathering.Tiberiums=0,1\nOreGathering.FramesPerDir=10,20\n",
        ));
        layers.push(
            RulesLayerKind::GameMode,
            IniFile::from_str("[HARV]\nOreGathering.Tiberiums=2\n"),
        );
        let rules = RuleSet::from_rules_layers(&layers).expect("layered phobos gather lists");
        let gathering = rules.object("HARV").expect("HARV").ore_gathering();
        assert_eq!(gathering.tiberiums, [2], "the named list replaces");
        assert_eq!(gathering.frames_per_dir, [10, 20], "the others retain");

        let stock = RuleSet::from_rules_layers(&RulesLayerStack::new(IniFile::from_str(
            "[VehicleTypes]\n0=HARV\n[HARV]\nHarvester=yes\n",
        )))
        .expect("stock harvester rules");
        assert_eq!(
            stock.object("HARV").expect("HARV").ore_gathering(),
            &crate::rules::object_type::PhobosOreGathering::default(),
            "a type authoring none keeps every empty list"
        );
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

    /// The production reader: the ordered Rules processor reads the Type
    /// references, `RuleSet` publishes them.
    fn rules(text: &str) -> crate::rules::ruleset::RuleSet {
        crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(text)).unwrap()
    }

    /// `Dock=` tokens resolve to the stored IDs of their BuildingTypes, found
    /// case-insensitively or allocated.
    #[test]
    fn test_parse_refinery_free_unit_and_dock() {
        let rules = rules(
            "[BuildingTypes]\n0=MODPROC\n[VehicleTypes]\n0=MODHARV\n\
             [MODPROC]\nRefinery=yes\nDockUnload=yes\nFreeUnit=MODHARV\n\
             [MODHARV]\nHarvester=yes\nDock=modproc,NAREFN\n",
        );

        let refinery = rules.object("MODPROC").unwrap();
        assert!(refinery.refinery);
        assert_eq!(refinery.free_unit, Some("MODHARV".to_string()));

        let harvester = rules.object("MODHARV").unwrap();
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

    /// `WeaponTypeClass::FindOrAllocate @ 0x00772FA0` answers null for `none`
    /// and `<none>`, on both weapon branches.
    #[test]
    fn a_none_weapon_name_leaves_its_slot_empty() {
        let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=CMIN\n1=GUNNER\n\
             [CMIN]\nPrimary=none\nSecondary=<none>\nElitePrimary=NONE\n\
             [GUNNER]\nTurretCount=1\nWeaponCount=2\nWeapon1=none\nWeapon2=GUN\n",
        ))
        .unwrap();
        let miner = rules.object("CMIN").unwrap();
        assert!(miner.weapon_list.iter().all(Option::is_none));
        assert!(miner.elite_weapon_list.iter().all(Option::is_none));
        let gunner = rules.object("GUNNER").unwrap();
        assert_eq!(gunner.weapon_list[0], None);
        assert_eq!(gunner.weapon_list[1].as_deref(), Some("GUN"));
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
        let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(
            // Turret branch: Primary=/Secondary= are ignored, but the fields
            // they name are filled from Weapon1=/Weapon2=.
            "[VehicleTypes]\n0=SREF\n1=YAGGUN\n2=MTNK\n3=WIPED\n4=NOSLOTS\n\
             [SREF]\nTurretCount=4\nWeaponCount=1\nWeapon1=Comet\nEliteWeapon1=SuperComet\n\
             Weapon2=Ignored\n\
             [YAGGUN]\nTurretCount=1\nWeaponCount=6\nWeapon1=AGGattling\nWeapon2=AAGattCann\n\
             EliteWeapon2=AAGattlingE\n\
             [MTNK]\nPrimary=105mm\nSecondary=MachGun\nWeapon1=NeverRead\n\
             [WIPED]\nClearAllWeapons=yes\nPrimary=105mm\nSecondary=MachGun\n\
             [NOSLOTS]\nTurretCount=2\nWeaponCount=0\nWeapon1=NeverRead\nPrimary=NeverRead\n",
        ))
        .unwrap();
        let obj = |id: &str| rules.object(id).unwrap();

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
        let text = "[YAGGUN]\nIsGattling=yes\nTurretCount=1\nWeaponCount=6\nWeapon1=AG1\n\
             EliteWeapon1=AG1E\nWeapon2=AA1\nWeapon6=AA3\nEliteWeapon6=AA3E\n\
             [DEST]\nNavalTargeting=1\nNaval=yes\n\
             [BSUB]\nLandTargeting=2\nUnderwater=yes\nUnnatural=yes\n\
             [SQD]\nOrganic=yes\nNavalTargeting=3\n\
             [GAPOWR]\nDrainable=yes\n[TESLA]\nOverpowerable=true\n";
        let ini = IniFile::from_str(text);
        // The weapon block comes from the ordered Rules owner.
        let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(&format!(
            "[BuildingTypes]\n0=YAGGUN\n{text}"
        )))
        .unwrap();
        let yaggun = rules.object("YAGGUN").unwrap();
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
    fn test_size_defaults_by_category() {
        let ini = IniFile::from_str(
            "[INF]\nFixtureOnly=1\n[VEH]\nFixtureOnly=1\n\
             [AIR]\nFixtureOnly=1\n[BUILDING]\nFixtureOnly=1\n",
        );
        for (id, category) in [
            ("INF", ObjectCategory::Infantry),
            ("VEH", ObjectCategory::Vehicle),
            ("AIR", ObjectCategory::Aircraft),
            ("BUILDING", ObjectCategory::Building),
        ] {
            let section = ini.section(id).expect("fixture section");
            let object = ObjectType::from_ini_section(id, section, category);
            assert_eq!(
                object.size, 1.0,
                "[{id}] missing Size= must retain the native constructor's 1.0",
            );
            assert_eq!(
                object.size_limit, 0.0,
                "[{id}] missing SizeLimit= must retain the native constructor's 0.0",
            );
        }
    }

    #[test]
    fn retail_mtnk_and_e1_sizes_read_through_battle_layers() {
        let Some(retail) =
            crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
        else {
            return;
        };

        assert_eq!(retail.rules.object("MTNK").unwrap().size, 3.0);
        assert_eq!(retail.rules.object("E1").unwrap().size, 1.0);
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
    fn gsi_05_14_debris_keys_are_case_exact_so_the_retail_lowercase_spelling_is_unread() {
        // A death spends a count drawn from [MinDebris, MaxDebris] on
        // DebrisTypes VoxelAnims and DebrisAnims SHP anims.
        let rules = rules(
            "[BuildingTypes]\n0=GAPOWR\n1=NAPOWR\n\
             [GAPOWR]\n\
             MinDebris=4\nMaxDebris=9\n\
             DebrisTypes=TIRE\nDebrisMaximums=3\n\
             DebrisAnims=DBRIS1LG,DBRIS1SM,DBRIS4LG\n\
             [NAPOWR]\nMaxDebris=5\n\
             debristypes=TIRE\nDEBRISMAXIMUMS=3\nDebrisanims=DBRIS1LG\n",
        );
        let obj = rules.object("GAPOWR").unwrap();
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
        let obj = rules.object("NAPOWR").unwrap();
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
        let rules = rules(
            "[BuildingTypes]\n0=GAPOWR\n[GAPOWR]\n\
             DamageParticleSystems=BigGreySSys, SmallGreySSys ,SparkSys\n\
             DestroyParticleSystems=DebrisSmokeSys\n",
        );
        let obj = rules.object("GAPOWR").unwrap();

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
    fn turret_spins_reads_constructor_default_and_rule_layer_overrides() {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        use crate::rules::ruleset::RuleSet;

        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[VehicleTypes]\n0=DISK\n[DISK]\nTurret=yes\n",
        ));
        assert!(
            !RuleSet::from_rules_layers(&layers)
                .unwrap()
                .object("DISK")
                .unwrap()
                .turret_spins
        );

        for (kind, patch, expected) in [
            (RulesLayerKind::LangRule, "TurretSpins=yes", true),
            (RulesLayerKind::GameMode, "TurretSpins=malformed", true),
            (RulesLayerKind::Scenario, "TurretSpins=no", false),
            (RulesLayerKind::Scenario, "TurretSpins=yes", true),
        ] {
            layers.push(kind, IniFile::from_str(&format!("[DISK]\n{patch}\n")));
            let rules = RuleSet::from_rules_layers(&layers).unwrap();
            assert_eq!(
                rules.object("DISK").unwrap().turret_spins,
                expected,
                "{patch}"
            );
        }
    }

    /// The production rules reader on the stock battle source stack. DISK is
    /// the authored `TurretSpins=yes` type; an ordinary tank remains the
    /// default-false control.
    #[test]
    fn retail_flying_disk_turret_spins_reads_from_battle_rules_layers() {
        let Some(retail) =
            crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
        else {
            return;
        };
        assert!(retail.rules.object("DISK").unwrap().turret_spins);
        assert!(!retail.rules.object("MTNK").unwrap().turret_spins);
    }

    /// The production reader on retail `rulesmd.ini`: `CrateBeneath=` and
    /// `CrateBeneathIsMoney=` (BuildingTypeClass::ReadINI `0x00460EC1`,
    /// `0x00460EDB`), defaulting to the constructor's false.
    #[test]
    fn retail_crate_beneath_reads() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_rulesmd_rules() else {
            return;
        };
        for (id, beneath, money) in [
            ("CAWASH01", true, true),
            ("CARUS07", true, false),
            ("GAPOWR", false, false),
        ] {
            let object = rules.object(id).unwrap();
            assert_eq!(
                (object.crate_beneath, object.crate_beneath_is_money),
                (beneath, money),
                "{id}"
            );
        }
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

    /// TechnoType ctor711500/default and ReadINI7153DA..7153F5.
    #[test]
    fn crush_tilt_reader_preserves_native_true_default_and_explicit_no() {
        for (line, expected) in [
            ("", true),
            ("TiltsWhenCrushes=no", false),
            ("TiltsWhenCrushes=yes", true),
        ] {
            let ini = IniFile::from_str(&format!("[MTNK]\nSpeed=7\n{line}\n"));
            let object = ObjectType::from_ini_section(
                "MTNK",
                ini.section("MTNK").unwrap(),
                ObjectCategory::Vehicle,
            );
            assert_eq!(object.tilts_when_crushes, expected);
        }
    }

    #[test]
    fn absent_weight_keeps_the_techno_type_constructor_value() {
        // Original TechnoType710C66..710C71 writes 1.0. UnitType7470D0
        // does not replace it before its reader calls TechnoType712170.
        let ini = IniFile::from_str("[MTNK]\nStrength=300\n");
        let mtnk = ObjectType::from_ini_section(
            "MTNK",
            ini.section("MTNK").expect("MTNK section"),
            ObjectCategory::Vehicle,
        );
        assert_eq!(mtnk.weight, SimFixed::ONE);
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
            crate::util::fixed_math::SimFixed::from_num(5.0_f32)
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
            crate::util::fixed_math::SimFixed::from_num(16.0_f32)
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

    #[test]
    fn techno_category_retains_stock_battle_values() {
        let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };

        assert_eq!(
            battle.rules.object("E1").expect("stock E1").techno_category,
            TechnoTypeCategory::Soldier
        );
        assert_eq!(
            battle
                .rules
                .object("MTNK")
                .expect("stock MTNK")
                .techno_category,
            TechnoTypeCategory::ArmoredFightingVehicle
        );
    }

    #[test]
    fn techno_category_uses_native_names_and_current_default_per_pass() {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        use crate::rules::ruleset::RuleSet;

        let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let mut layers = RulesLayerStack::new((*battle.authored_rules).clone());
        layers.push(
            RulesLayerKind::GameMode,
            IniFile::from_str("[E1]\nCategory=Armored Fighting Vehicle\n[MTNK]\nCategory=AFV\n"),
        );
        let named = RuleSet::from_rules_layers(&layers).expect("production Category pass");
        assert_eq!(
            named.object("E1").expect("registered E1").techno_category,
            TechnoTypeCategory::ArmoredFightingVehicle
        );
        assert_eq!(
            named
                .object("MTNK")
                .expect("registered MTNK")
                .techno_category,
            TechnoTypeCategory::ArmoredFightingVehicle
        );

        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[E1]\nCost=1\n[MTNK]\nCategory=not-a-native-category\n"),
        );
        let missing_and_unknown =
            RuleSet::from_rules_layers(&layers).expect("production Category boundaries");
        assert_eq!(
            missing_and_unknown
                .object("E1")
                .expect("registered E1")
                .techno_category,
            TechnoTypeCategory::ArmoredFightingVehicle
        );
        assert_eq!(
            missing_and_unknown
                .object("MTNK")
                .expect("registered MTNK")
                .techno_category,
            TechnoTypeCategory::Unknown
        );

        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[E1]\nCategory=not-a-native-category\n[MTNK]\nCost=1\n"),
        );
        let unknown = RuleSet::from_rules_layers(&layers).expect("production layered Category");
        assert_eq!(
            unknown.object("E1").expect("registered E1").techno_category,
            TechnoTypeCategory::Unknown
        );
        assert_eq!(
            unknown
                .object("MTNK")
                .expect("registered MTNK")
                .techno_category,
            TechnoTypeCategory::Unknown
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
