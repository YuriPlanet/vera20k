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
pub use crate::rules::ini_parser::ini_value::TechnoTypeCategory;
use crate::rules::jumpjet_params::JumpjetParams;
use crate::rules::locomotor_type::{LocomotorKind, MovementZone, SpeedType};
#[cfg(test)]
use crate::rules::sound_ini::SoundRegistry;
use crate::rules::terrain_rules::LandType;
use crate::util::fixed_math::SimFixed;
use crate::util::native_x87::NativeF64Bits;

mod queries;
mod reader;

use crate::rules::threat_coefficients;

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

/// Phobos `TargetZoneScanType=` (TechnoTypeExt, default `same`): what a
/// scanner does with a candidate outside its movement zone in
/// `Greatest_Threat`'s walk over every object, once Phobos plays
/// (`crate::sim::combat::greatest_threat`'s zone gate).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TargetZoneScanType {
    /// Refuse it.
    #[default]
    Same,
    /// Keep it.
    Any,
    /// Keep it when the selected weapon reaches it from the passable cell
    /// found nearest it.
    InRange,
}

impl TargetZoneScanType {
    /// Phobos' enum reader (`detail::read<TargetZoneScanType>`,
    /// `src/Utilities/TemplateDef.h`): `INI_EX::ReadString` into its
    /// 2048-byte buffer, then `_strcmpi` of the whole value with `same`, `any`
    /// and `inrange`. Other text keeps the value, which Phobos logs
    /// (`Debug::INIParseFailed`), and each rules pass supplies its prior value.
    fn read_ini(section: &IniSection) -> Self {
        section.read_string_with("TargetZoneScanType", 2048, Self::Same, |current, value| {
            if value.eq_ignore_ascii_case("same") {
                Self::Same
            } else if value.eq_ignore_ascii_case("any") {
                Self::Any
            } else if value.eq_ignore_ascii_case("inrange") {
                Self::InRange
            } else {
                log::warn!(
                    "[{}] TargetZoneScanType={value}: expected same, any or inrange",
                    section.name
                );
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

/// InfantryType's fixed-ART reads, retained by the process-resident type.
/// The constructor sets Crawls (`+0xEBD`) to true (`0x00523788`) and clears
/// the four discharge frames (`+0xE40..+0xE4C`, `0x005236D7..0x005236F9`).
/// Every reached ReadINI pass reads all five from the ART section its Image
/// names, each with its current value as the default
/// (`InfantryTypeClass::ReadINI`, `0x005246A1..0x0052472D`), so a pass whose
/// section lacks a key keeps the value an earlier pass read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct InfantryArt {
    crawls: bool,
    discharge_frames: [i32; 4],
}

impl Default for InfantryArt {
    fn default() -> Self {
        Self {
            crawls: true,
            discharge_frames: [0; 4],
        }
    }
}

impl InfantryArt {
    /// One pass's reads from its Image's ART section.
    pub(crate) fn read_pass(&mut self, section: &IniSection) {
        self.crawls = section.read_bool("Crawls", self.crawls);
        crate::rules::art_data::read_infantry_discharge_frames(section, &mut self.discharge_frames);
    }
}

/// InfantryType constructor clears EBE/EC2..EC6. ReadINI reads these
/// current-default flags, then 52466E..52469A forces Infiltrate for C4,
/// Engineer or Agent, independently of either theft byte.
/// Retain the postprocessed byte across reached rules passes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct InfantryInfiltration {
    infiltrate: bool,
    c4: bool,
    engineer: bool,
    agent: bool,
    #[serde(default)]
    thief: Option<bool>,
    #[serde(default)]
    vehicle_thief: Option<bool>,
}

impl InfantryInfiltration {
    pub(crate) fn apply_pass(&mut self, section: &IniSection) {
        // InfantryTypeClass::ReadINI 5244A9/524545/524579/5245AD,
        // then 52466E..52469A (retail gamemd.exe, Steam gamemd).
        self.infiltrate = section.read_bool("Infiltrate", self.infiltrate);
        self.c4 = section.read_bool("C4", self.c4);
        self.engineer = section.read_bool("Engineer", self.engineer);
        self.agent = section.read_bool("Agent", self.agent);
        // InfantryType ReadINI5245B8..5245F5, current defaults; constructor
        // 5237BC/5237C3 clears both bytes (retail gamemd.exe).
        self.thief = Some(section.read_bool("Thief", self.thief.unwrap_or(false)));
        self.vehicle_thief =
            Some(section.read_bool("VehicleThief", self.vehicle_thief.unwrap_or(false)));
        self.infiltrate |= self.c4 || self.engineer || self.agent;
    }
}

#[cfg(all(test, tests_rules_o_z))]
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

/// VERA's bound on `WeaponCount=`. gamemd loops and Ares sizes its weapon,
/// turret, label and insignia vectors by the raw signed value, so a value near
/// `i32::MAX` freezes or exhausts memory there. Outside data never stops the
/// game: the read clamps here, far above any weapon list a mod authors.
pub const WEAPON_COUNT_LIMIT: i32 = 1024;

/// VERA's bound on the radio contacts (dock slots) one building allocates.
/// Native allocates `max(NumberOfDocks, 1)` contacts (`0x0043BCBD..0x0043BCD0`),
/// which for a value near `i32::MAX` exhausts memory; no dock layout needs more.
pub const DOCK_CONTACT_LIMIT: i32 = 256;

/// VERA's bound on `MaxDebris=` and `MinDebris=` (TechnoType and Warhead).
/// The debris throw (`0x00702291..0x0070256C`, `0x00469D0C..0x00469EB4`)
/// draws and builds one piece at a time, so a value near `i32::MAX` freezes
/// the match on the first death; retail throws at most 15.
pub const DEBRIS_LIMIT: i32 = 256;

/// VERA's bound on `SpawnsNumber=`, read by `TechnoTypeClass::ReadINI`'s
/// ReadInt (`0x00714EF0`, `+0xD5C`). The spawn manager constructor
/// (`0x006B6C90`) builds one node per spawn and each node its aircraft, so a
/// value near `i32::MAX` exhausts memory; no carrier needs more.
pub const SPAWNS_NUMBER_LIMIT: i32 = 128;

/// VERA's bound on `SlavesNumber=`, read by `TechnoTypeClass::ReadINI`'s
/// ReadInt (`0x00714E29`, `+0xD44`). `TechnoClass::Init_Managers` builds
/// that many slaves the moment an `Enslaves=` master is constructed, so a
/// value near `i32::MAX` exhausts memory; retail's Slave Miner has 5.
pub const SLAVES_NUMBER_LIMIT: i32 = 128;

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

/// `Owner=`, `RequiredHouses=` and `ForbiddenHouses=` as House-type masks
/// ([`crate::rules::ruleset::RuleSet::house_type_mask`]). The masks depend
/// on the Country registry, so the Rules build and each registry refresh
/// derive them again (`RuleSet::project_house_masks`); the token lists are
/// written only when a type is read.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct HouseMasks {
    owner: u32,
    required: u32,
    forbidden: u32,
}

impl HouseMasks {
    pub(crate) fn derive(object: &ObjectType, rules: &crate::rules::ruleset::RuleSet) -> Self {
        Self {
            owner: rules.house_type_mask(&object.owner),
            required: if object.required_houses.is_empty()
                && !object.required_houses_explicitly_empty
            {
                u32::MAX
            } else {
                rules.house_type_mask(&object.required_houses)
            },
            forbidden: rules.house_type_mask(&object.forbidden_houses),
        }
    }
}

/// The `VoxelBarrelOffsetTo*` pivot triples of a `BarrelAnimIsVoxel=`
/// (`+0x16C6`) building type. `BuildingTypeClass::ReadINI @ 0x0045FE50` reads
/// them in native field order right after `BarrelAnimIsVoxel=` (and the
/// unread `VoxelBarrelFile=` name) through `Get_TripleInt @ 0x00529CA0`
/// (`0x00464697`, `0x004646C0`, `0x004646E9`), each pass using the current
/// triple as its self-default. The constructor `0x0045DD90` never writes
/// them, so native's first default is uninitialized heap memory; VERA's is
/// `(0,0,0)` (a `fire_coordinate` residual). The barrel matrix chain
/// `0x00458810` composes them for `BuildingClass::GetTurretDrawPosition`
/// (`0x00453BF0`) and for the unported barrel draw (`0x0043E63E`);
/// `BuildingTypeClass::ComputeCRC` (`0x00464B30`) also hashes them. The
/// fourth triple, `VoxelBarrelOffsetToBarrelEnd=` (`+0x1754`, read at
/// `0x00464712`), is the point VERA's zero chain scale (`+0x1728`)
/// multiplies away, so VERA does not read it.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct BarrelVoxelOffsets {
    /// `VoxelBarrelOffsetToPitchPivotPoint=` (`+0x1730/34/38`), the pitch
    /// pivot the chain translates by after the barrel-elevation turn.
    pub pitch_pivot: [i32; 3],
    /// `VoxelBarrelOffsetToRotatePivotPoint=` (`+0x173C/40/44`), translated
    /// between the body-facing turn and the elevation turn.
    pub rotate_pivot: [i32; 3],
    /// `VoxelBarrelOffsetToBuildingPivotPoint=` (`+0x1748/4C/50`), the first
    /// translation off the identity.
    pub building_pivot: [i32; 3],
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
    /// Final retained TechnoType+2C8..2E8, published by the ordered reader
    /// (original ReadINI71551E..715720), never resolved at score time.
    threat_coefficients: super::threat_coefficients::ThreatCoefficients,
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
    /// Phobos `AttachEffect.AttachTypes`: authored `[AttachEffectTypes]`
    /// names, in list order.
    #[cfg(feature = "phobos")]
    attach_effect_attach_types: Vec<String>,
    /// `AttachEffect.DurationOverrides`, paired with `attach_types` by index.
    #[cfg(feature = "phobos")]
    attach_effect_duration_overrides: Vec<i32>,
    /// `AttachEffect.Delays`: re-creation delay after a natural end.
    #[cfg(feature = "phobos")]
    attach_effect_delays: Vec<i32>,
    /// `AttachEffect.InitialDelays`: first attachment delay.
    #[cfg(feature = "phobos")]
    attach_effect_initial_delays: Vec<i32>,
    /// `AttachEffect.RecreationDelays`: delay after a full removal; negative
    /// forbids re-creation.
    #[cfg(feature = "phobos")]
    attach_effect_recreation_delays: Vec<i32>,
    /// Phobos `TechnoTypeExt::DigitalDisplay_Disable`.
    #[cfg(feature = "phobos")]
    digital_display_disable: bool,
    /// Phobos `TechnoTypeExt::DigitalDisplay_Health_FakeAtDisguise`; `None`
    /// inherits `[AudioVisual] DigitalDisplay.Health.FakeAtDisguise`.
    #[cfg(feature = "phobos")]
    digital_display_health_fake_at_disguise: Option<bool>,
    /// Phobos `TechnoTypeExt::OpenTopped_UseTransportRangeModifiers`
    /// (`OpenTopped.UseTransportRangeModifiers=`); `None` inherits
    /// `[General]`. A passenger inside an `OpenTopped` transport with this
    /// set folds the transport's range modifiers instead of its own.
    #[cfg(feature = "phobos")]
    open_topped_use_transport_range_modifiers: Option<bool>,
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
    /// append flag (0x4B3EF2). Process_Drive_Track kills what the train
    /// cannot crush in each cell it enters (0x4B185C / 0x6A0EE8,
    /// `Simulation::track_train_smash`).
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
    /// Phobos `AircraftTypeExt::FlyNoWobbles`: overrides whether the Fly
    /// `Draw_Point` wobble runs, which retail gates on `IsDropship` alone at
    /// `0x4CF8B1`. Unset defers to `[AudioVisual] FlyNoWobbles`, then to that
    /// retail rule; `rules::phobos_fly_wobble` owns the resolution.
    #[cfg(feature = "phobos")]
    pub(crate) fly_no_wobbles: Option<bool>,
    /// Phobos `TechnoTypeExt::LaserTargetColor` (`Nullable<int>`): the
    /// `[ColorAdd]` index whose word tints a building this type's airstrike
    /// targets. Unset takes `[AudioVisual] LaserTargetColor`
    /// (`Simulation::airstrike_laser_target_color`).
    #[cfg(feature = "phobos")]
    pub(crate) laser_target_color: Option<i32>,
    /// Phobos `TechnoTypeExt::AirstrikeLineColor` (`Nullable<ColorStruct>`):
    /// the RGB this type's airstrike beam and target dot use. Unset takes
    /// `[AudioVisual] AirstrikeLineColor` (`Simulation::airstrike_line_color`).
    #[cfg(feature = "phobos")]
    pub(crate) airstrike_line_color: Option<[u8; 3]>,
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
    /// The three lists above as House-type masks ([`HouseMasks`]).
    house_masks: HouseMasks,
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
    /// Phobos `ForbidParallelAIQueues=` (default false): while Ares'
    /// `[GlobalControls] AllowParallelAIQueues=` is on, a computer house that
    /// already has a factory building for this type's category does not start
    /// another one for this type.
    pub forbid_parallel_ai_queues: bool,
    /// Phobos `TargetZoneScanType=` ([`TargetZoneScanType`]).
    pub target_zone_scan_type: TargetZoneScanType,
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
    pub(crate) voxel_model_slots: super::voxel_model_plan::ModelSlots,
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
    /// Phobos `TechnoTypeExt` insignia block (`Insignia*`, `InsigniaType*`,
    /// `Insignia.ShowEnemy`), accumulated over every reached Rules pass by
    /// the ordered TechnoType reader.
    #[cfg(feature = "phobos")]
    pub phobos_insignia: crate::rules::insignia_type::PhobosInsignia,
    /// Ares `TechnoTypeExt` insignia keys (`Insignia*`, `InsigniaFrame*`,
    /// `Insignia.ShowEnemy`), accumulated like the Phobos block above.
    pub ares_insignia: crate::rules::insignia_type::AresInsignia,
    /// Phobos `Passengers.BySize` (`Valueable<bool>`, default yes): the
    /// passenger-count key `DrawInsignia` indexes `Insignia.PassengersN`
    /// with. Its other consumer, the NoQueueUpToEnter admission rewrite, is
    /// not ported.
    #[cfg(feature = "phobos")]
    pub phobos_passengers_by_size: bool,
    /// Phobos `TechnoTypeExt` Iron Curtain / Force Shield overrides
    /// (`IronCurtain.*`, `ForceShield.*`).
    #[cfg(feature = "phobos")]
    pub phobos_invulnerability: crate::rules::invulnerability_rules::PhobosInvulnerability,
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
    /// Ares TechnoTypeExt `VoiceRepair` (ValueableIdx<VocClass>, unset -1):
    /// the attack voice of a repairing weapon (TechnoClass_SelectFiringVoice
    /// 1004B1D0, hook 0x7090A8).
    ares_voice_repair: Option<String>,
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
    /// Installed Ares TypeExt+504/+508 and+50C/+510 (ctor1003CE14..2E).
    /// Unset, explicit silence and a resolved Voc ID remain distinct.
    promotion_sounds: [Option<Option<String>>; 2],
    vehicle_thief_sounds: [Option<String>; 2],
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
    /// A building's turret name (`TurretAnim=` in rules, `BuildingType
    /// +0x11B0`, e.g. "SAM"). With `TurretAnimIsVoxel=` the engine loads
    /// `{TurretAnim}.VXL` + `{TurretAnim}.HVA` as the turret model; without it
    /// the name is the building's slot-9 animation
    /// ([`crate::rules::art_data::building_turret_anim_config`]).
    pub turret_anim: Option<String>,
    /// Slot 9's name for a damaged building (`+0x11C0`): `TurretAnimDamaged=`
    /// (`0x004644BC`), or the normal name when that is empty (`0x004644FD`).
    pub turret_anim_damaged: Option<String>,
    /// Whether the turret anim is a VXL model (TurretAnimIsVoxel=, default false).
    /// When false, the TurretAnim is an SHP animation in the building's slot 9.
    pub turret_anim_is_voxel: bool,
    /// Pixel X offset for building turret placement (TurretAnimX=).
    pub turret_anim_x: i32,
    /// Pixel Y offset for building turret placement (TurretAnimY=).
    pub turret_anim_y: i32,
    /// `TurretAnimYSort=` (`+0x11EC`, read at `0x0046462D`): slot 9's YSort.
    pub turret_anim_y_sort: i32,
    /// Z-seed adjustment for a voxel turret's blit (TurretAnimZAdjust=, negative
    /// = nearer the camera). Building vtable `0x7E3EBC+0x2EC` folds it into the
    /// draw Z only when `TurretAnimIsVoxel`; it is not a ground-layer sort key.
    pub turret_anim_z_adjust: i32,
    /// `BarrelAnimIsVoxel=` (`BuildingType +0x16C6`): arms the
    /// `BuildingClass::GetFLH @ 0x00453840` arm (`0x004539A1..0x004539DE`)
    /// that replaces the whole coordinate with `GetTurretDrawPosition`
    /// (`0x00453BF0`). ReadINI reads it at `0x00464661` through
    /// `INIClass::ReadBool @ 0x005295F0` with the current byte as default; the
    /// constructor zeroes it (`0x0045E16F`), so it defaults false.
    /// `VoxelBarrelFile=` (`+0x1714`, read at `0x0046467E`) names the barrel
    /// model but nothing on this fire path consumes it.
    pub barrel_anim_is_voxel: bool,
    /// The `VoxelBarrelOffsetTo*` pivot triples ([`BarrelVoxelOffsets`]) the
    /// barrel arm's matrix chain composes.
    pub barrel_voxel: BarrelVoxelOffsets,
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
    no_manual_fire: bool,
    /// Ares TechnoTypeExt+304 `ImmuneToAbduction=` (constructor false): no
    /// `Abductor=` weapon takes this type (conductAbduction 10054A80).
    pub(crate) ares_immune_to_abduction: bool,
    /// ObjectType `IgnoresFirestorm=` (`+0x239`, constructor false), which
    /// `ObjectTypeClass::ReadINI @ 0x005F92D0` reads with ReadBool for every
    /// type. Only the Ares firestorm walls read it (`sim::ares_firestorm`),
    /// so only a Rules read the Ares way keeps it.
    pub(crate) ignores_firestorm: bool,
    /// Ares TechnoTypeExt+4C4 `Passengers.BySize=` (constructor true): the
    /// hold counts passengers' `Size=` against `Passengers=`, else heads
    /// (CanHoldPassenger 100448E0, abduction only here).
    pub(crate) ares_passengers_by_size: bool,
    /// Ares TechnoTypeExt+46C `Saboteur=` (constructor false, Valueable read
    /// in `TechnoTypeExt::ExtData::LoadFromINIFile`): an `Infiltrate=`
    /// infantryman blows up the buildings it enters
    /// (`InfantryExt::GetInfiltrateAction @ 100252D0`).
    saboteur: bool,
    /// BuildingTypeExt's `SpyEffect.*` and `ImmuneToSaboteurs=`, projected
    /// from the retained rules reader (`rules::ares_infiltration`).
    infiltration: crate::rules::ares_infiltration::InfiltrationTypeRules,
    /// BuildingTypeExt's rubble and repair keys, projected from the retained
    /// rules reader (`rules::ares_building_type`).
    building_type_ext: crate::rules::ares_building_type::BuildingTypeExtRules,
    /// TechnoTypeExt Experience.* and Promote.* (TypeExt+17B..+1B0,
    /// +503..+540), projected from the retained rules reader
    /// (`rules::ares_veterancy`).
    veterancy: crate::rules::ares_veterancy::VeterancyTypeRules,
    /// TypeExt+524/+528 `EVA.VeteranPromoted`/`EVA.ElitePromoted`
    /// (ValueableIdx<VoxClass>): Initialize1003DEC0 seeds both with
    /// `EVA_UnitPromoted`; `none` silences the cue.
    promotion_eva: [Option<String>; 2],
    /// BuildingTypeExt+184 `LostEvaEvent` (ValueableIdx<VoxClass>): the
    /// Initialize of `1000DCF0` seeds it with `EVA_TechBuildingLost`; `none`
    /// silences the cue.
    lost_eva: Option<String>,
    /// BuildingTypeExt Academy.* (+138..+170), projected from the retained
    /// rules reader (`rules::ares_veterancy`).
    academy: crate::rules::ares_veterancy::AcademyTypeRules,
    /// TechnoTypeExt BuildTime.*, FactoryPlant.Multiplier and cloning and
    /// BuildingTypeExt CloningFacility (`rules::ares_production`).
    production: crate::rules::ares_production::ProductionTypeRules,
    /// TechnoTypeExt `Cursor.*` and BuildingTypeExt `Cursor.Spy`
    /// (`rules::ares_mouse_cursor`); presentation only.
    ares_cursors: crate::rules::ares_mouse_cursor::TechnoCursors,
    /// Ares3.0 TypeExt+646/+647. Unset uses the class/score default at the
    /// Add/Remove_Tracking owner; an explicit false is a distinct retained input.
    keep_alive: Option<bool>,
    /// Read-only projection of Ares TechnoTypeExt+618 (Convert.Script).
    convert_script: Option<crate::rules::superweapon_type::DeliveryType>,
    convert_water: Option<crate::rules::superweapon_type::DeliveryType>,
    convert_land: Option<crate::rules::superweapon_type::DeliveryType>,
    /// TechnoTypeExt+1DC, a registered UnitType used by Ares' water draw.
    water_image: Option<String>,
    turret_rot_override: Option<i32>,
    spotlight: crate::rules::ares_spotlight::SpotlightTypeRules,
    driver: crate::rules::ares_driver::DriverTypeRules,
    /// Ares TypeExt+250, projected from the retained rules reader.
    selection_group: String,
    bounty: crate::rules::ares_bounty::BountyTypeRules,
    crew: crate::rules::ares_crew::CrewTypeRules,
    /// TechnoTypeExt combat and movement keys, projected from the retained
    /// rules reader (`rules::ares_techno_type`).
    techno_combat: crate::rules::ares_techno_type::AresTechnoTypeRules,
    /// Ares TypeExt+278/+280/+1F8/+1F9/+1FC/+204, projected from the retained
    /// rules reader (`rules::ares_cloak_power`).
    cloak_power: crate::rules::ares_cloak_power::CloakPowerTypeRules,
    /// Ares TypeExt+1E8/+1EC CloakSound and +1F0/+1F4 DecloakSound
    /// (constructor unset): unset, explicit silence and a resolved
    /// Voc ID remain distinct.
    cloak_sounds: [Option<Option<String>>; 2],
    /// Immutable projection of the retained TechnoTypeExt+308/+314/+320/+321
    /// reader. Country identities bind during Process, never from merged INI.
    factory_owners: crate::rules::native_processing::AresFactoryOwners,
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
    /// Phobos `TechnoTypeExt::ImmuneToCrit` (`ImmuneToCrit=`), default false.
    #[cfg(feature = "phobos")]
    pub(crate) immune_to_crit: bool,
    /// Phobos `TechnoTypeExt::VeteranCritChance`; `None` inherits
    /// `[General] VeteranCritChance`. Read only when the firer's
    /// `CRITCHANCE` rank ability holds.
    #[cfg(feature = "phobos")]
    pub(crate) veteran_crit_chance: Option<f64>,
    /// The five Phobos rank abilities read from the same
    /// `VeteranAbilities=`/`EliteAbilities=` lists as the stock array.
    #[cfg(feature = "phobos")]
    pub(crate) phobos_abilities: crate::rules::phobos_crit::PhobosAbilityFlags,
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
    /// VERA's `SuperWeaponPerBuilding=` (default no; `docs/superweapon-limit.md`):
    /// each building of this type charges its own shot of the Supers it
    /// provides, rather than the house's one charge alone. gamemd has no such
    /// key; the Superweapon Limit sets it.
    pub super_weapon_per_building: bool,
    /// BuildingTypeExt+178, ordered lookup-only `SuperWeapons=` references.
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
    ///   `number_of_docks` (zero-padding any indices missing in art, truncating excess),
    ///   at most [`DOCK_CONTACT_LIMIT`].
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
    /// Spawn pool capacity (`SpawnsNumber=`). Default 0, at most
    /// [`SPAWNS_NUMBER_LIMIT`].
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
    /// Holds that art's name, built once from the section name; `None` for
    /// `NoSpawnAlt=no`.
    no_spawn_alt_model: Option<String>,

    // -- Slave Miner / economy fields --
    /// Infantry type enslaved/spawned by this unit (Enslaves= in rules.ini, YR only).
    /// Used by Slave Miner (SMIN) to spawn SLAV workers.
    pub enslaves: Option<String>,
    /// `TechnoType+0xD44`, `SlavesNumber=`: the slaves Init_Managers builds.
    /// `TechnoTypeClass::ReadINI` ReadInt at `0x00714E29`; constructor 0, at
    /// most [`SLAVES_NUMBER_LIMIT`].
    pub slaves_number: i32,
    /// `TechnoType+0xD48`, `SlaveRegenRate=`: frames before a lost slave is
    /// regrown. ReadInt at `0x00714E08`; constructor 0.
    pub slave_regen_rate: i32,
    /// `TechnoType+0xD4C`, `SlaveReloadRate=`: frames a slave waits inside
    /// after paying its load. ReadInt at `0x00714E4A`; constructor 0.
    pub slave_reload_rate: i32,
    /// `Slaved=` (TechnoType `+0xD3E`). gamemd reads the key, but no gameplay
    /// code reads the field, natively or here: a slave's ties to its master
    /// come from the master's `Enslaves=` and SlaveManager.
    pub slaved: bool,
    /// `Fearless=yes` on InfantryType. Suppresses fear/prone panic changes.
    pub fearless: bool,
    /// `Fraidycat=yes` on InfantryType. First fear hit immediately maxes fear.
    pub fraidycat: bool,
    /// InfantryType+EC1, retained Civilian ReadINI52455F. The instance
    /// predicate413CD0 also excludes the separate nominal-survivor byte.
    /// Its type-CRC524840/report64DEA0 consumer remains a separate residual.
    pub civilian: bool,
    /// `Crawls=` from ARTMD (InfantryTypeClass `+0xEBD`), the last reached
    /// pass's read ([`InfantryArt`]). Gates Down and the prone speed.
    pub crawls: bool,
    /// Phobos `InfantryTypeExt::ProneSpeed`: the prone movement multiplier for
    /// this type. Unset defers to the `[General]` value `Crawls=` selects.
    #[cfg(feature = "phobos")]
    pub(crate) prone_speed: Option<f64>,
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
    /// Ares TypeExt+323/+324: the optional Aircraft Crashable override,
    /// independent of original Unit TechnoType+D95 (10048CB5..10048CDB).
    aircraft_crashable: Option<bool>,
    /// Ares TypeExt+150 `IronCurtain.Modifier` and +158
    /// `ForceShield.Modifier` (constructor 1.0), projected from the retained
    /// rules reader.
    curtain_modifiers: [NativeF64Bits; 2],
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
    /// `TiberiumHeal=` (`TechnoTypeClass+0xC92`): constructor false
    /// (`0x00710AF0`), ReadBool over the current value at `0x00714C09`. Only
    /// Ares reads it in play: the Foot heal (`10014BA0`) and the
    /// TiberiumRemains default (`1004B050`).
    pub(crate) tiberium_heal: bool,
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
    /// AircraftType's retained ART Trailer/SpawnDelay (41CCEF..41CD4E),
    /// projected only by the ordered Rules process owner.
    aircraft_trailer: crate::rules::aircraft_trailer::AircraftTrailer,
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
    pub(super) build_const_eligible: bool,
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
    promotion_abilities: Option<crate::rules::ares_abilities::PromotionAbilities>,
    self_attach_effect: Option<crate::rules::ares_warhead::AttachEffectRules>,
    cloak_allowed: bool,
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
    /// Phobos `MindControlRangeLimit=` (`Valueable<Leptons>`): a controlled
    /// unit farther than this planar distance from its controller is freed.
    /// The constructor's `Leptons` value is 0, which disables the check.
    #[cfg(feature = "phobos")]
    pub mind_control_range_limit: i32,
    /// Phobos `MindControl.IgnoreSize=` (`Nullable<bool>`); `None` inherits
    /// `[General] MindControl.IgnoreSize` (constructor true).
    #[cfg(feature = "phobos")]
    pub mind_control_ignore_size: Option<bool>,
    /// Phobos `MindControlSize=` (`Valueable<int>`, constructor 1): the
    /// control slots this type consumes while controlled.
    #[cfg(feature = "phobos")]
    pub mind_control_size: i32,
    /// Phobos `MindControlLink.VisibleToHouse=` (`Nullable<AffectedHouse>`);
    /// `None` inherits `[General] MindControlLink.VisibleToHouse` (All).
    #[cfg(feature = "phobos")]
    pub mind_control_link_visible_to_house: Option<crate::rules::phobos_shields::AffectedHouse>,
    /// Phobos `MultiMindControl.ReleaseVictim=` (`Nullable<bool>`); `None`
    /// inherits `[General] MultiMindControl.ReleaseVictim` (constructor false).
    #[cfg(feature = "phobos")]
    pub multi_mind_control_release_victim: Option<bool>,
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

    /// InfantryType+EC5, constructor5237BC=false; ReadINI5245B8..5245D2
    /// reads exact-case Thief with the current byte as default. The native
    /// vehicle capture receiver5202F0 checks this, separately from EC6.
    pub thief: bool,

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
    /// VERA RA2 Mode: original Red Alert 2 has no `CanOccupyFire=`, so a
    /// `CanBeOccupied=` building's occupants fire without it. Derived from
    /// [`RuleSet::ra2`](crate::rules::ruleset::RuleSet::ra2) by the RuleSet
    /// at load, its one writer; false in Yuri's Revenge. Read through
    /// [`Self::occupants_fire`].
    pub(crate) ra2_occupy_fire: bool,

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

    /// Phobos `InfantryTypeExt::InfantryAutoDeploy`: re-enables the undeployed
    /// `Approach_Target` arm at `0x00522434` for this type. Unset defers to
    /// `[General] InfantryAutoDeploy`.
    #[cfg(feature = "phobos")]
    pub(crate) infantry_auto_deploy: Option<bool>,

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

    /// Immutable projection of the type's retained ReadSequenceData523D00 bank.
    infantry_sequence_bank: Option<crate::rules::infantry_sequence::InfantrySequenceBank>,

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
    /// Derived: the weapons `weapon_list`, `elite_weapon_list`,
    /// `occupy_weapon` and `elite_occupy_weapon` name, as the RuleSet that
    /// built the type resolved them (`RuleSet::resolve_handles`).
    resolved_weapons: crate::rules::ruleset::TypeWeapons,
    /// Derived: `debris_types` as the RuleSet that built the type resolved
    /// it (`RuleSet::resolve_handles`).
    resolved_debris: crate::rules::ruleset::ResolvedDebris,
    /// Retained WeaponStruct and transport-port coordinates, projected from
    /// the ordered type reader rather than reconstructed from the final Image.
    weapon_offsets: crate::rules::weapon_offsets::WeaponOffsets,

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
    /// Ares TechnoTypeExt+3E0/+3E8 Nullable<int>, read1003EE23/1003EE39.
    /// An unset value uses native Sight; zero/negative overrides remain set.
    designator_range: Option<i32>,
    inhibitor_range: Option<i32>,
    /// Ares TechnoTypeExt+3DC; AcquireHunterSeekerTarget10042E40 checks1004306C.
    hunter_seeker_ignore: bool,
    /// Ares TechnoTypeExt `Chronoshift.*` (+0x160..+0x162).
    chronoshift: crate::rules::superweapon_influence::ChronoshiftRules,
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

    /// Phobos `AllowAirstrike=`: whether an `Airstrike=` warhead may designate
    /// this type. Unset keeps the vanilla rule at each call site — buildings
    /// follow `CanC4`, every other kind is targetable.
    #[cfg(feature = "phobos")]
    pub allow_airstrike: Option<bool>,

    /// BuildingTypeClass `EligibleForDelayKill` (`+0x1551`). Native default
    /// false; only eligible buildings can convert a fatal result to PostMortem.
    pub eligible_for_delay_kill: bool,

    /// TechnoType+0xC9A `Invisible=`. BuildingType ReadINI460E09 also forces
    /// it true while InvisibleInGame is set; the ordered Rules owner retains
    /// that write across layers. Live rejection can test both bytes separately.
    pub invisible: bool,

    /// ObjectTypeClass::ObjectTypeClass stores +0x231=1 at 0x005F7168;
    /// ReadINI retains ReadBool("LegalTarget", current) at 0x005F94AE..0x005F94C2.
    pub legal_target: bool,

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

    /// ART `SiloDamage=` of the type's metadata section
    /// ([`ArtRegistry::resolve_metadata_entry`](crate::rules::art_data::ArtRegistry::resolve_metadata_entry)
    /// for its ID and Image), which a Building's storage update reads every
    /// frame (`UpdateAnimation @ 0x00450CBD`). Derived from the installed ART:
    /// `RuleSet` projects it whenever it installs one; no ART answers false.
    silo_damage: bool,

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
    /// `NaturalParticleSystem=` (Type+0x764): the system a building keeps at
    /// `+0x30C`. BuildingClass Unlimbo (`0x00440E99`) and UpdateCloak's
    /// fade-in (`0x00454F72`) build it, the fade-out deletes it and
    /// ReceiveDamage's NowYellow (`0x004426AC`) slows it. No retail INI sets
    /// the key.
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
    /// InfantryType `Cyborg=` (`+0xEAC`, ReadBool with the current value as
    /// default at `0x005243B8`). No stock INI authors it. See
    /// [`ObjectType::emits_damage_spark`] for the spark flag it feeds.
    pub cyborg: bool,
    /// TechnoType `+0xC8F`, projected from the Rules passes: see
    /// [`ObjectType::emits_damage_spark`].
    pub damage_sparks: bool,

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
    /// Ares reads it as a whole int into TechnoTypeExt+0x650 (constructor 0),
    /// which every gap radius read takes instead: CreateGap/RemoveGap
    /// (1004D330, hooks 0x6FB1B5/0x6FB4A3), the operational edge (1004D250,
    /// hook 0x454BDC), GetSensorRange (1004D170, hook 0x4566D5) and the
    /// Mission_Unload toggle (1004D1B0, hook 0x44E2B0).
    pub gap_radius_in_cells: i32,
    /// TechnoType+CD3 `SuperGapRadiusInCells=`: the gap radius while
    /// super-charged. Mission_Unload's gap arm needs it nonzero
    /// (`0x0044E2B0`) and caches it in `+0x26C` (`0x0044E2DC`). Ares: the
    /// whole int at TechnoTypeExt+0x654, as above.
    pub super_gap_radius_in_cells: i32,
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
    /// Phobos `PowerPlantEnhancer.PowerPlants=`: BuildingType IDs this
    /// building enhances (`ValueableVector<BuildingTypeClass*>`: a present
    /// value replaces the list, split on `,` without trimming). VERA keeps the
    /// tokens and matches them case-insensitively, as `BuildingTypeClass::Find`
    /// does; an unknown name matches nothing.
    #[cfg(feature = "phobos")]
    power_plant_enhancer_power_plants: Vec<String>,
    /// Phobos `PowerPlantEnhancer.Range=` (`Valueable<Leptons>`): cells read
    /// by `INI_EX::ReadDouble`, then `F2I(cells * 256)`; default 0.
    #[cfg(feature = "phobos")]
    power_plant_enhancer_range: i32,
    /// Phobos `PowerPlantEnhancer.Amount=` (`Valueable<int>`): default 0.
    #[cfg(feature = "phobos")]
    power_plant_enhancer_amount: i32,
    /// Phobos `PowerPlantEnhancer.Factor=` (`Nullable<float>`, constructed
    /// 1.0): `<default>`, `none` and `<none>` reset it to 0.
    #[cfg(feature = "phobos")]
    power_plant_enhancer_factor: f32,
    /// Phobos `PowerPlantEnhancer.MaxCount=` (`Valueable<int>`): default -1
    /// (no limit).
    #[cfg(feature = "phobos")]
    power_plant_enhancer_max_count: i32,
    /// Phobos `PowerPlant.DamageFactor=` (`Valueable<double>`): the
    /// health-scaling factor for a power plant's output, default 1.0 (the
    /// stock curve).
    #[cfg(feature = "phobos")]
    power_plant_damage_factor: f64,
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

#[cfg(all(test, tests_rules_o_z))]
#[path = "infantry_speed_type_tests.rs"]
mod infantry_speed_type_tests;

#[cfg(all(test, tests_rules_o_z))]
#[path = "vehicle_speed_type_tests.rs"]
mod vehicle_speed_type_tests;

#[cfg(all(test, tests_rules_o_z))]
#[path = "infantry_pip_tests.rs"]
mod infantry_pip_tests;

#[cfg(all(test, tests_rules_o_z))]
#[path = "ammo_pip_tests.rs"]
mod ammo_pip_tests;

#[cfg(all(test, tests_rules_o_z))]
#[path = "absorber_pip_tests.rs"]
mod absorber_pip_tests;

#[cfg(all(test, tests_rules_o_z))]
#[path = "building_pip_tests.rs"]
mod building_pip_tests;

#[cfg(all(test, tests_rules_o_z))]
mod tests;

#[cfg(all(test, tests_rules_o_z))]
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
