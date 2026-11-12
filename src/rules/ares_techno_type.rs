//! Ares TechnoTypeExt combat and movement keys, retained by the ordered Rules
//! processor and projected onto `ObjectType`. Sources: the extension
//! constructor `TechnoTypeExt::ExtData::ExtData @ 1003C710`, the first-read
//! `Initialize` in `ExtMap_LoadFromINI @ 1003DEC0` and the reader
//! `LoadFromINIFile @ 1003DFB0` (`src/Ext/TechnoType/Body.cpp`).
//!
//! Type pointers are Find-only Valueable reads (`detail::read<T*>` with
//! `allocate = false`): an absent or unknown name keeps the field, `none` or
//! `<none>` clears it. The caller resolves them against the registry as the
//! read reaches them ([`TypeRef`]).

use crate::rules::ini_parser::{IniFile, IniSection};
use crate::rules::missile_spawn::MissileSpawnParams;
use crate::util::native_x87::NativeF64Bits;

/// The registry a Find-only pointer read looks in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TypeRef {
    Animation,
    Warhead,
    Weapon,
}

/// A pointer read's result: `None` keeps the field (absent, empty or
/// unknown), `Some(None)` is a blank `none`, `Some(Some(id))` a found type.
pub(crate) type Resolved = Option<Option<String>>;

/// Retained TechnoTypeExt keys of this chain.
#[derive(Debug, Clone, Default, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct AresTechnoTypeRules {
    /// `Initialize` ran (ExtMap state 1): it seeds once, at the type's first
    /// reached read.
    initialized: bool,
    /// `Gattling.Cycle` (+0x322, false).
    gattling_cyclic: bool,
    /// AircraftType-only custom missile block and its rocket puffs.
    missile: Option<Box<CustomMissileRules>>,
    ammo: AmmoTypeRules,
    combat: CombatTypeRules,
    damage: DamagePresentationRules,
    movement: MovementTypeRules,
    self_healing: SelfHealingTypeRules,
    fall_rate: FallRateTypeRules,
    roles: TypeRoleRules,
    /// The tiberium and harvester keys (`rules::ares_tiberium`).
    #[serde(default)]
    tiberium: crate::rules::ares_tiberium::TiberiumTechnoRules,
    #[serde(default)]
    presentation: PresentationTypeRules,
    #[serde(default)]
    residual: ResidualTypeRules,
    #[serde(default)]
    secret_lab_houses: SecretLabHouseRules,
    #[serde(default)]
    prerequisites: AresPrerequisiteRules,
}

/// TechnoTypeExt `PrerequisiteLists` (+0x40) past list 0,
/// `PrerequisiteNegatives` (+0x4C), `PrerequisiteTheaters` (+0x64,
/// constructor 0xFFFFFFFF) and `RequiredStolenTech` (+0x128, constructor
/// 0), read by `LoadFromINIFile @ 1003DFB0`. List 0 is the type's
/// `Prerequisite=` itself (`ObjectType::prerequisite`): installed Ares
/// parses that key into it again (`0x1003E0B7`) through the same
/// Parse100085F0 the stock read already used under Ares, and no stock reader
/// of TechnoType+0x648 is left once CanBuild (hook 0x4F7870), the planner
/// check (hook 0x505360) and the power plant choice (hook 0x4FE782) are
/// replaced, so one field holds both. `RequirementsMet @ 10021FF0`,
/// `PrerequisitesMet @ 10021CA0` and `PrerequisitesListed @ 10021C50`
/// consume these.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct AresPrerequisiteRules {
    /// Lists 1.. of +0x40 (`Prerequisite.List1=` on).
    extra_lists: Vec<Vec<crate::rules::prerequisite::Prerequisite>>,
    negatives: Vec<crate::rules::prerequisite::Prerequisite>,
    theaters: u32,
    stolen_techs: u32,
}

impl Default for AresPrerequisiteRules {
    fn default() -> Self {
        Self {
            extra_lists: Vec::new(),
            negatives: Vec::new(),
            theaters: u32::MAX,
            stolen_techs: 0,
        }
    }
}

impl AresPrerequisiteRules {
    /// The prerequisite block of one reached `1003DFB0` pass, after the
    /// `Survivor.*` reads (`0x1003E093..0x1003E0F1`, then `.Negative` and
    /// `.RequiredTheaters`). `list0` is the type's
    /// `Prerequisite=` list and `parse(key, prior)` Prereqs::Parse100085F0
    /// of `key` (2048-byte ReadString, comma tokens, BuildingType before
    /// generic group, unknown tokens skipped; a missing or empty key keeps
    /// `prior`).
    pub(crate) fn read_pass(
        &mut self,
        section: &IniSection,
        list0: &mut Vec<crate::rules::prerequisite::Prerequisite>,
        mut parse: impl FnMut(
            &str,
            &[crate::rules::prerequisite::Prerequisite],
        ) -> Vec<crate::rules::prerequisite::Prerequisite>,
    ) {
        let _ares = crate::rules::ares_switch::ares_reads();
        // `INIClass::ReadInteger("Prerequisite.Lists", size - 1)`, at least
        // 0, plus one: the vector grows with empty lists or is truncated.
        // Initialize (1003DEC0) sized it to one at the first read.
        let current = i32::try_from(self.extra_lists.len()).unwrap_or(i32::MAX);
        let extra = section.read_int("Prerequisite.Lists", current).max(0);
        self.extra_lists
            .resize(usize::try_from(extra).unwrap_or(0), Vec::new());
        // `Prerequisite` (list 0) and `PrerequisiteOverride` come next; the
        // stock read already parsed both through Parse100085F0.
        // `Prerequisite.List%u` for every list, 0 included (`0x1003E0F1`).
        *list0 = parse("Prerequisite.List0", list0.as_slice());
        for (index, list) in self.extra_lists.iter_mut().enumerate() {
            let key = format!("Prerequisite.List{}", index + 1);
            *list = parse(key.as_str(), list.as_slice());
        }
        self.negatives = parse("Prerequisite.Negative", self.negatives.as_slice());
        // INI_EX::ReadString (2048) then strtok(","): a nonempty value
        // replaces the mask with the bits of the names FindTheater48DBE0
        // knows; an unknown name is only logged.
        if let Some(tokens) = section.read_list("Prerequisite.RequiredTheaters", 0x800) {
            self.theaters = 0;
            for token in tokens {
                let index =
                    crate::rules::object_image::ImageTheater::from_name(token).native_index();
                if index >= 0 {
                    self.theaters |= 1 << index;
                }
            }
        }
        // Read later in 1003DFB0 (after CameoPalette), with no reader
        // between them that depends on it: a nonempty value clears
        // the set, then each token's `atoi` below 32 (unsigned compare, so
        // negatives are skipped silently) sets its bit.
        if let Some(tokens) = section.read_list("Prerequisite.StolenTechs", 0x800) {
            self.stolen_techs = 0;
            for token in tokens {
                let index = crate::rules::ini_value::crt_atoi(token) as u32;
                if index < 32 {
                    self.stolen_techs |= 1 << index;
                }
            }
        }
    }

    /// Lists 1.. of the type's prerequisite lists; list 0 is
    /// `ObjectType::prerequisite`.
    pub(crate) fn extra_lists(&self) -> &[Vec<crate::rules::prerequisite::Prerequisite>] {
        &self.extra_lists
    }

    /// `Prerequisite.Negative`: any of them owned forbids the type.
    pub(crate) fn negatives(&self) -> &[crate::rules::prerequisite::Prerequisite] {
        &self.negatives
    }

    /// Every parsed entry, for the saved-state registry check.
    pub(crate) fn entries(
        &self,
    ) -> impl Iterator<Item = &crate::rules::prerequisite::Prerequisite> {
        self.extra_lists.iter().flatten().chain(&self.negatives)
    }

    /// RequirementsMet's theater test (`10021FF0`): the scenario's theater
    /// bit (`1 << (Theater & 31)`, so an unknown theater tests bit 31).
    pub(crate) fn admits_theater(&self, theater: &str) -> bool {
        let index = crate::rules::object_image::ImageTheater::from_name(theater).native_index();
        self.theaters & (1u32 << (index as u32 & 31)) != 0
    }

    /// `Prerequisite.StolenTechs`: the HouseExt StolenTech bits the type
    /// needs, all of them.
    pub(crate) fn required_stolen_techs(&self) -> u32 {
        self.stolen_techs
    }
}

/// TechnoTypeExt `SecretLab.RequiredHouses` (+0x6C, constructor 0xFFFFFFFF)
/// and `SecretLab.ForbiddenHouses` (+0x70, 0), each read through
/// `INIClass::ReadHouseTypesList @ 0x004750D0` (Ares 0x800 buffer, 1005BED0)
/// with the current mask as its default. As for the stock house lists
/// (`object_type::HouseMasks`), the tokens of the last pass that set a key
/// are kept and masked against the Country registry when asked.
#[derive(Debug, Clone, Default, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct SecretLabHouseRules {
    required: Option<Vec<String>>,
    forbidden: Option<Vec<String>>,
}

impl SecretLabHouseRules {
    /// UpdateSecretLab's AddToOptions filter (lambda Ares.dll 10011340): the
    /// owner's Country bit (`1 << ArrayIndex`) is required and not forbidden.
    pub(crate) fn admit(&self, rules: &crate::rules::ruleset::RuleSet, house_bit: u32) -> bool {
        let required = self
            .required
            .as_deref()
            .map_or(u32::MAX, |tokens| rules.house_type_mask(tokens));
        let forbidden = self
            .forbidden
            .as_deref()
            .map_or(0, |tokens| rules.house_type_mask(tokens));
        required & house_bit != 0 && forbidden & house_bit == 0
    }
}

/// A Find-only `Valueable<TechnoTypeClass*>` read (1003FA40 over
/// TechnoTypeClass::Find 10007EA0): `None` keeps the field, `Some(None)` is
/// a blank `none`, `Some(Some(type))` a found type.
pub(crate) type ResolvedTechno =
    Option<Option<crate::rules::superweapon_type::TechnoTypeReference>>;

/// TechnoTypeExt+0x1BC/+0x1C4 `VoiceAirstrikeAttack`/`VoiceAirstrikeAbort`
/// (NullableIdx<VocClass>, unset), +0x4CC `EnemyUIName` (Nullable<CSFText>,
/// unset) and +0x648 `RadialIndicatorRadius` (Nullable<int>, unset).
#[derive(Debug, Clone, Default, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct PresentationTypeRules {
    /// Bound against the fixed sound catalog with the type's other sounds
    /// (`ObjectType::bind_scalar_sound_references`); `Some(None)` is an
    /// explicit silence.
    airstrike_voices: [Option<Option<String>>; 2],
    /// The CSF label (at most 31 bytes, `detail::read<CSFText>` 10076510).
    enemy_ui_name: Option<String>,
    radial_indicator_radius: Option<i32>,
}

impl PresentationTypeRules {
    /// The voice an airstrike plays as it starts (`abort` false,
    /// AirstrikeClass_Fire_AirstrikeAttackVoice, Ares.dll 10048D60, hook
    /// 0x41D940) or is called off (`abort` true, AirstrikeClass_PointerGot
    /// Invalid_AirstrikeAbortSound 10048DE0, hook 0x41D5AE): `None` keeps
    /// the caller's value, `Some(None)` silences it.
    #[cfg(feature = "ares")]
    pub(crate) fn airstrike_voice(&self, abort: bool) -> Option<Option<&str>> {
        self.airstrike_voices[usize::from(abort)]
            .as_ref()
            .map(|voice| voice.as_deref())
    }

    /// `EnemyUIName`, the CSF label a non-allied player reads for a building
    /// of this type (BuildingClass_GetUIName, Ares.dll 10015DA0, hook
    /// 0x459ED0).
    #[cfg(feature = "ares")]
    pub(crate) fn enemy_ui_name(&self) -> Option<&str> {
        self.enemy_ui_name.as_deref()
    }

    /// `RadialIndicatorRadius`: a building's radial indicator range in cells
    /// in place of GetSensorRange's own answer (BuildingClass_GetRangeOf
    /// Radial_Radius, Ares.dll 10048E60, hook 0x4566B0).
    #[cfg(feature = "ares")]
    pub(crate) fn radial_indicator_radius(&self) -> Option<i32> {
        self.radial_indicator_radius
    }
}

/// TechnoTypeExt `HunterSeeker.AscentSpeed`, `.DescentSpeed` and
/// `.EmergeSpeed` (+0x3C4/+0x3CC/+0x3D4, Nullable<int>), each falling back
/// to `[General] HunterSeekerAscentSpeed` .. (RulesExt +0x84/+0x88/+0x8C).
#[cfg(feature = "ares")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HunterSeekerClimb {
    pub ascent: i32,
    pub descent: i32,
    pub emerge: i32,
}

/// TechnoTypeExt keys whose consumer mechanisms VERA does not have yet
/// (docs/ares-techno-types.md, "Not covered"). They are read in the DLL's
/// order with the constructor's defaults (1003C710, Initialize 1003DEC0)
/// and nothing consumes them.
#[derive(Debug, Clone, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct ResidualTypeRules {
    /// `InitialPayload.Types` (+0x104, ValueableVector<TechnoTypeClass*>
    /// 1000EDD0, Find only) and `InitialPayload.Nums` (+0x110,
    /// ValueableVector<int> 1000ED00), empty; consumed by
    /// `sim::ares_transports` (CreateInitialPayload 10043C10).
    #[serde(default)]
    initial_payload_types: Vec<crate::rules::superweapon_type::TechnoTypeReference>,
    initial_payload_nums: Vec<i32>,
    /// `CanBeReversed` (+0x270, true) and `ReversedAs` (+0x274, null),
    /// consumed by `sim::reverse_engineering`.
    can_be_reversed: bool,
    reversed_as: Option<crate::rules::superweapon_type::TechnoTypeReference>,
    /// `Unsellable` (+0x644, Nullable<bool>, unset), consumed by the unit
    /// sale at a depot (`production::foot_can_be_sold`).
    #[serde(default)]
    unsellable: Option<bool>,
    /// `Carryall.Allowed` (+0x2F8, Nullable<bool>) and `Carryall.SizeLimit`
    /// (+0x2FC, Nullable<int>), unset; consumed by
    /// `Simulation::ares_carryall_can_lift`.
    carryall_allowed: Option<bool>,
    carryall_size_limit: Option<i32>,
    /// `EVA.Lost` (+0x394, ValueableIdx<VoxClass>): Initialize seeds a
    /// non-building with `FindIndex("EVA_UnitLost")`, a building keeps -1;
    /// bound against the fixed EVA catalog (`RuleSet::bind_type_eva_references`).
    unit_lost_eva: Option<String>,
    /// Whether that binding ran; rules read without the catalog (fixtures)
    /// leave the consumers to take the Initialize seed themselves.
    #[serde(default)]
    unit_lost_eva_bound: bool,
    /// `HunterSeeker.DetonateProximity`, `.DescendProximity`, `.AscentSpeed`,
    /// `.DescentSpeed` and `.EmergeSpeed` (+0x3B4..+0x3D4, Nullable<int>,
    /// unset; else the RulesExt values).
    hunter_seeker: [Option<i32>; 5],
    /// `Assaulter.Level` (+0x42C, Valueable<int>, 0), consumed through
    /// [`AresTechnoTypeRules::assaulter_level`].
    assaulter_level: i32,
    /// `NoSelfGuardArea` (+0x4C8, false).
    no_self_guard_area: bool,
    /// `DigInSound`/`DigOutSound` (+0x550/+0x558, NullableIdx<VocClass>),
    /// bound with the type's other sounds. Consumed by the Tunnel
    /// locomotor's dig effects ([`AresTechnoTypeRules::dig_effects`]).
    dig_sounds: [Option<Option<String>>; 2],
    /// `DigIn`/`DigOut` (+0x560/+0x568, Nullable<AnimTypeClass*>, Find
    /// only): `Some(None)` is an explicit none. Consumed with the sounds.
    dig_anims: [Option<Option<String>>; 2],
    /// `FakeOf` (+0x600) and `Convert.Deploy` (+0x60C), Find-only
    /// Valueable<TechnoTypeClass*>, null; `Convert.Deploy` is consumed by
    /// the simple deployer's Unload (`sim::unit_simple_deploy`).
    fake_of: Option<crate::rules::superweapon_type::TechnoTypeReference>,
    convert_deploy: Option<crate::rules::superweapon_type::TechnoTypeReference>,
}

impl Default for ResidualTypeRules {
    fn default() -> Self {
        Self {
            initial_payload_types: Vec::new(),
            initial_payload_nums: Vec::new(),
            can_be_reversed: true,
            reversed_as: None,
            unsellable: None,
            carryall_allowed: None,
            carryall_size_limit: None,
            unit_lost_eva: None,
            unit_lost_eva_bound: false,
            hunter_seeker: [None; 5],
            assaulter_level: 0,
            no_self_guard_area: false,
            dig_sounds: [None, None],
            dig_anims: [None, None],
            fake_of: None,
            convert_deploy: None,
        }
    }
}

/// TechnoTypeExt+0x27C `PassengerTurret` (false), +0x325 `CrashSpin`
/// (true), +0x328 `AirRate` (0), +0x4AC/+0x4B8 `Passengers.Allowed`/
/// `.Disallowed` (ValueableVector<TechnoTypeClass*> 1000EDD0, Find only,
/// empty), +0x4C5 `NoManualUnload` and +0x4C7 `NoManualEnter` (false),
/// +0x604 `DeployDir` (Nullable, unset).
#[derive(Debug, Clone, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct MovementTypeRules {
    passenger_turret: bool,
    crash_spin: bool,
    air_rate: i32,
    #[serde(default)]
    passengers_allowed: Vec<crate::rules::superweapon_type::TechnoTypeReference>,
    #[serde(default)]
    passengers_disallowed: Vec<crate::rules::superweapon_type::TechnoTypeReference>,
    no_manual_unload: bool,
    no_manual_enter: bool,
    #[serde(default)]
    deploy_dir: Option<u8>,
}

impl Default for MovementTypeRules {
    fn default() -> Self {
        Self {
            passenger_turret: false,
            crash_spin: true,
            air_rate: 0,
            passengers_allowed: Vec::new(),
            passengers_disallowed: Vec::new(),
            no_manual_unload: false,
            no_manual_enter: false,
            deploy_dir: None,
        }
    }
}

impl MovementTypeRules {
    /// `PassengerTurret`: the current turret follows the passenger count
    /// (TechnoClass_Update, Ares.dll 1004B320, hook 0x6F9E50, `#1208`).
    pub(crate) fn passenger_turret(&self) -> bool {
        self.passenger_turret
    }

    /// `CrashSpin=no` skips Crash's spin and its three draws
    /// (FootClass_Crash_Spin, Ares.dll 10049050, hook 0x4DECAE).
    pub(crate) fn crash_spin(&self) -> bool {
        self.crash_spin
    }

    /// `AirRate`: an airborne Foot steps its body counter every `AirRate`
    /// frames (FootClass_Update_AnimRate, Ares.dll 100493D0, hook 0x4DA8B2);
    /// 0 keeps the stock cadence.
    pub(crate) fn air_rate(&self) -> i32 {
        self.air_rate
    }

    /// `TechnoTypeExt::ExtData::IsPassengerAllowed @ 1003DD60`: a passenger
    /// of this type (`category`, `type_id`) may board a carrier of this
    /// type when `Passengers.Allowed` is empty or lists it and
    /// `Passengers.Disallowed` does not. Consulted by the carriers' CAN_LOAD
    /// (Unit `1004C270`, hook `0x73769E`; Building `100174E0`, hook
    /// `0x43C326`; Aircraft `10048D10`, hook `0x41949F`, whose hold VERA
    /// does not represent).
    #[cfg(feature = "ares")]
    pub(crate) fn passenger_allowed(
        &self,
        category: crate::rules::object_type::ObjectCategory,
        type_id: &str,
    ) -> bool {
        let listed = |list: &[crate::rules::superweapon_type::TechnoTypeReference]| {
            list.iter().any(|listed| {
                listed.category == category && listed.id.eq_ignore_ascii_case(type_id)
            })
        };
        (self.passengers_allowed.is_empty() || listed(&self.passengers_allowed))
            && !listed(&self.passengers_disallowed)
    }

    /// `NoManualUnload`: a player cannot unload this transport
    /// (UnitClass_GetActionOnObject 1004BEF0, hook 0x740031; Aircraft
    /// 10048A90, 0x417DD2; CanDeploySlashUnload 10049E70, 0x700EEC).
    pub(crate) fn no_manual_unload(&self) -> bool {
        self.no_manual_unload
    }

    /// `NoManualEnter`: a player cannot order passengers into this
    /// transport (Unit 1004BEA0, hook 0x74031A; Infantry 100176E0, 0x51ED8E).
    pub(crate) fn no_manual_enter(&self) -> bool {
        self.no_manual_enter
    }

    /// `DeployDir` (read<Direction::Value> 100409B0: 0..7, else unset): the
    /// eighth-turn a simple deployer with a `DeployingAnim=` faces before it
    /// deploys, else `[AudioVisual] DeployDir` (JumpjetLocomotionClass_State4
    /// _54C550_DeployDir 100499F0, hook 0x54C767; UnitClass_SimpleDeploy
    /// _Facing 1004C2D0, hook 0x739B8A).
    #[cfg(feature = "ares")]
    pub(crate) fn deploy_dir(&self) -> Option<u8> {
        self.deploy_dir
    }
}

/// TechnoTypeExt+0x74..+0x76 and +0x398/+0x39C: the type roles Ares names by
/// key where gamemd tests a literal ID or another flag.
#[derive(Debug, Clone, Default, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct TypeRoleRules {
    /// `IsDesolator` (+0x74) and `IsDesolator.RadDependant` (+0x75):
    /// Initialize seeds both `ID == "DESO"`. RadDependant has no 3.0 reader.
    is_desolator: bool,
    is_desolator_rad_dependant: bool,
    /// `IsCow` (+0x76): Initialize seeds `ID == "COW"`.
    is_cow: bool,
    /// `IsBomb` (+0x88, false).
    is_bomb: bool,
    /// `Drain.Local` (+0x398, false) and `Drain.Amount` (+0x39C, 0).
    drain_local: bool,
    drain_amount: i32,
}

impl TypeRoleRules {
    /// `IsDesolator`, in place of `ImmuneToRadiation=` in the deployed
    /// Guard shim (Ares.dll 100497C0/100497F0, hooks 0x5215F9/0x52138C) and
    /// of the `"DESO"` ID in Mission_Unload (10049790, hook 0x51F76D).
    pub(crate) fn is_desolator(&self) -> bool {
        self.is_desolator
    }

    /// `IsCow`, in place of the `"COW"` ID in the idle roll
    /// (InfantryClass_Idle, Ares.dll 10049740, hook 0x51CE9A).
    pub(crate) fn is_cow(&self) -> bool {
        self.is_cow
    }

    /// `IsBomb`: a paradropped Foot of this type lands as a bomb
    /// (ObjectClass_SpawnParachuted_Animation, Ares.dll 10047C60, hook
    /// 0x5F5ADD).
    pub(crate) fn is_bomb(&self) -> bool {
        self.is_bomb
    }

    /// `Drain.Local` and `Drain.Amount` (HouseClass_UpdatePower_LocalDrain1,
    /// Ares.dll 10024690, hook 0x508D32).
    pub(crate) fn drain(&self) -> (bool, i32) {
        (self.drain_local, self.drain_amount)
    }
}

/// TechnoTypeExt+0x3A0..+0x3B0 and +0x3F0: the damaged-aircraft smoke and
/// the damage sparks.
#[derive(Debug, Clone, Default, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct DamagePresentationRules {
    /// `Smoke.ChanceRed` (Nullable<int> +0x3A0) and `Smoke.ChanceDead`
    /// (+0x3A8), unset: 10 and 80.
    smoke_chance_red: Option<i32>,
    smoke_chance_dead: Option<i32>,
    /// `Smoke.Anim` (+0x3B0); Initialize seeds an AircraftType's with
    /// `Find("SGRYSMK1")`, other types keep null.
    smoke_anim: Option<String>,
    /// `DamageSparks` (Nullable<bool> +0x3F0), unset: the type's own
    /// `TechnoTypeClass+0xC8F`.
    damage_sparks: Option<bool>,
}

impl DamagePresentationRules {
    /// An airborne aircraft below ConditionRed draws a `RandomRanged(0, 99)`
    /// and puffs `Smoke.Anim` under `Smoke.ChanceRed` while alive or
    /// `Smoke.ChanceDead` at zero health (AircraftClass_Update_DamageSmoke,
    /// Ares.dll 10047B60, hook 0x415085). `None`: no anim, and no draw.
    pub(crate) fn smoke(&self, alive: bool) -> Option<(&str, i32)> {
        let anim = self.smoke_anim.as_deref()?;
        let chance = if alive {
            self.smoke_chance_red.unwrap_or(10)
        } else {
            self.smoke_chance_dead.unwrap_or(80)
        };
        Some((anim, chance))
    }

    /// Whether a damaged techno makes spark systems: `DamageSparks`, else
    /// the type's own flag (TechnoClass_Update_DamageSparks, Ares.dll
    /// 1004B760, hook 0x6FACD9).
    pub(crate) fn damage_sparks(&self, type_default: bool) -> bool {
        self.damage_sparks.unwrap_or(type_default)
    }
}

/// TechnoTypeExt combat tweaks: +0x32C, +0x418, +0x430..+0x440 and
/// +0x46D/+0x46E.
#[derive(Debug, Clone, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct CombatTypeRules {
    /// `CivilianEnemy` (+0x32C, false).
    civilian_enemy: bool,
    /// `Berserk.ROFMultiplier` (Nullable<double> +0x418, unset).
    berserk_rof_multiplier: Option<NativeF64Bits>,
    /// `ImmuneToBerserk` (Nullable<bool> +0x428, unset).
    immune_to_berserk: Option<bool>,
    /// `OmniCrusher.Aggressive` (+0x430, true).
    omni_crusher_aggressive: bool,
    /// `CrushDamage.%s` (Promotable<int> +0x434: rookie, veteran, elite; 0).
    crush_damage: [i32; 3],
    /// `CrushDamage.Warhead` (Nullable<WarheadTypeClass*> +0x440): `None`
    /// unset (`[CombatDamage] C4Warhead`), `Some(None)` an explicit none.
    crush_damage_warhead: Option<Option<String>>,
    /// `CanPassiveAquire.Guard` (+0x46D) and `.Cloak` (+0x46E), both true.
    can_passive_acquire_guard: bool,
    can_passive_acquire_cloak: bool,
}

impl Default for CombatTypeRules {
    fn default() -> Self {
        Self {
            civilian_enemy: false,
            berserk_rof_multiplier: None,
            immune_to_berserk: None,
            omni_crusher_aggressive: true,
            crush_damage: [0; 3],
            crush_damage_warhead: None,
            can_passive_acquire_guard: true,
            can_passive_acquire_cloak: true,
        }
    }
}

impl CombatTypeRules {
    /// `CivilianEnemy`: an auto-targeting scanner always takes this type as
    /// an enemy (TechnoClass_CanAutoTargetObject_CivilianEnemy, Ares.dll
    /// 10049D00, hook 0x6F826E).
    pub(crate) fn civilian_enemy(&self) -> bool {
        self.civilian_enemy
    }

    /// `Berserk.ROFMultiplier`, else the Rules `BerserkROFMultiplier`
    /// (TechnoClass_Fire_BerserkROFMultiplier, Ares.dll 1004A3D0, hook
    /// 0x6FF28F).
    pub(crate) fn berserk_rof_multiplier(&self, rules_default: NativeF64Bits) -> NativeF64Bits {
        self.berserk_rof_multiplier.unwrap_or(rules_default)
    }

    /// `ImmuneToBerserk`, else `psionics_immune` (ImmuneToPsionics or the
    /// rank's PSIONICSIMMUNE): a Psychedelic warhead neither fires at
    /// (TechnoClass_CanFire_PsionicsImmune, Ares.dll 10049EF0, hook 0x6FC417)
    /// nor turns this type (TechnoClass_ReceiveDamage_Abilities, 1004A950,
    /// hook 0x701BFE).
    #[cfg(feature = "ares")]
    pub(crate) fn immune_to_berserk(&self, psionics_immune: bool) -> bool {
        self.immune_to_berserk.unwrap_or(psionics_immune)
    }

    /// `OmniCrusher.Aggressive=no` skips Approach_Target's OmniCrusher arm
    /// (UnitClass_ApproachTarget_OmniCrusher, Ares.dll 1004BC10, hook
    /// 0x741613).
    pub(crate) fn omni_crusher_aggressive(&self) -> bool {
        self.omni_crusher_aggressive
    }

    /// The damage a crusher takes for crushing this type at `rank` (0
    /// rookie, 1 veteran, 2 elite) and its warhead (`None`: `C4Warhead`;
    /// `Some(None)`: no damage); UnitClass_CrushCell_CrushDamage (Ares.dll
    /// 1004BCE0, hook 0x7418AA).
    pub(crate) fn crush_damage(&self, rank: usize) -> (i32, Option<Option<&str>>) {
        (
            self.crush_damage[rank.min(2)],
            self.crush_damage_warhead
                .as_ref()
                .map(|warhead| warhead.as_deref()),
        )
    }

    /// `CanPassiveAquire.Guard`/`.Cloak`: a human player's guarding Foot
    /// acquires no target (FootClass_SelectAutoTarget_Cloaked, Ares.dll
    /// 100491E0, hook 0x4D9920).
    pub(crate) fn can_passive_acquire_guard(&self) -> bool {
        self.can_passive_acquire_guard
    }

    pub(crate) fn can_passive_acquire_cloak(&self) -> bool {
        self.can_passive_acquire_cloak
    }
}

/// RulesExt combat globals, read by `LoadBeforeTypeData` (Ares.dll
/// 1002AB60) from `[CombatDamage]`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct TechnoCombatGlobalRules {
    /// `BerserkROFMultiplier` (RulesExt+0x68, 0.5).
    berserk_rof_multiplier: NativeF64Bits,
    /// `AutoRepel` (+0xAC) and `PlayerAutoRepel` (+0xAD), both false.
    auto_repel_ai: bool,
    auto_repel_player: bool,
}

impl Default for TechnoCombatGlobalRules {
    fn default() -> Self {
        Self {
            berserk_rof_multiplier: NativeF64Bits::from_bits(0.5f64.to_bits()),
            auto_repel_ai: false,
            auto_repel_player: false,
        }
    }
}

impl TechnoCombatGlobalRules {
    pub(crate) fn read_before_type_data(&mut self, pass: &IniFile) {
        let _ares = crate::rules::ares_switch::ares_reads();
        let Some(section) = pass.section("CombatDamage") else {
            return;
        };
        self.berserk_rof_multiplier =
            section.read_ares_double_bits("BerserkROFMultiplier", self.berserk_rof_multiplier);
        self.auto_repel_ai = section.read_ares_bool("AutoRepel", self.auto_repel_ai);
        self.auto_repel_player = section.read_ares_bool("PlayerAutoRepel", self.auto_repel_player);
    }

    pub(crate) fn berserk_rof_multiplier(&self) -> NativeF64Bits {
        self.berserk_rof_multiplier
    }

    /// `PlayerAutoRepel` for a human-controlled scanner, else `AutoRepel`.
    pub(crate) fn auto_repel(&self, controlled_by_human: bool) -> bool {
        if controlled_by_human {
            self.auto_repel_player
        } else {
            self.auto_repel_ai
        }
    }
}

/// TechnoTypeExt+0x448..+0x468, the dock reload rate, the reload amounts and
/// the out-of-ammo weapon.
#[derive(Debug, Clone, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct AmmoTypeRules {
    /// `ReloadRate` (Nullable<double> +0x448, minutes), unset: Rules
    /// `ReloadRate`.
    #[serde(default)]
    reload_rate: Option<NativeF64Bits>,
    /// `ReloadAmount` (+0x458, 1).
    reload_amount: i32,
    /// `EmptyReloadAmount` (Nullable +0x45C/+0x460, unset).
    empty_reload_amount: Option<i32>,
    /// `NoAmmoAmount` (+0x464, 0).
    no_ammo_amount: i32,
    /// `NoAmmoWeapon` (+0x468, -1).
    no_ammo_weapon: i32,
}

impl Default for AmmoTypeRules {
    fn default() -> Self {
        Self {
            reload_rate: None,
            reload_amount: 1,
            empty_reload_amount: None,
            no_ammo_amount: 0,
            no_ammo_weapon: -1,
        }
    }
}

impl AmmoTypeRules {
    /// The minutes between a docked aircraft's reloads at its airfield
    /// (BuildingClass_MissionRepair_Reload, Ares.dll 10014190, hook
    /// 0x44C844): `ReloadRate`, else `[General] ReloadRate`.
    pub(crate) fn reload_rate(&self, general: f64) -> f64 {
        self.reload_rate
            .map_or(general, |rate| f64::from_bits(rate.bits()))
    }

    /// The rounds one `TechnoClass::Reload` adds (TechnoClass_Reload_
    /// ReloadAmount, Ares.dll 10047A70, hook 0x6FB05B): `ReloadAmount`, or
    /// `EmptyReloadAmount` when set and the count is zero.
    pub(crate) fn reload_amount(&self, ammo: i32) -> i32 {
        match self.empty_reload_amount {
            Some(empty) if ammo == 0 => empty,
            _ => self.reload_amount,
        }
    }

    /// The weapon index `What_Weapon_Should_I_Use` answers once the count is
    /// at or below `NoAmmoAmount` (TechnoClass_SelectWeapon_NoAmmoWeapon,
    /// Ares.dll 10047AE0, hook 0x6F3410); the caller tests `Ammo= >= 0`.
    pub(crate) fn no_ammo_weapon(&self, ammo: i32) -> Option<i32> {
        (self.no_ammo_weapon >= 0 && ammo <= self.no_ammo_amount).then_some(self.no_ammo_weapon)
    }
}

/// TechnoTypeExt+0x470..+0x4A8, the self-heal pulse that
/// `TechnoExt::ExtData::GetSelfHealAmount` (Ares.dll 10044E80) reads.
#[derive(Debug, Clone, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct SelfHealingTypeRules {
    /// `SelfHealing.Rate` (Nullable<double> +0x470), unset: Rules
    /// `RepairRate`.
    rate: Option<NativeF64Bits>,
    /// `SelfHealing.%sAmount` (Promotable<int> +0x480, 1).
    amount: [i32; 3],
    /// `SelfHealing.%sMax` (Promotable<double> +0x490, 1.0).
    max: [NativeF64Bits; 3],
    /// `SelfHealing.CombatDelay` (+0x4A8, 0).
    combat_delay: i32,
}

impl Default for SelfHealingTypeRules {
    fn default() -> Self {
        Self {
            rate: None,
            amount: [1; 3],
            max: [NativeF64Bits::ONE; 3],
            combat_delay: 0,
        }
    }
}

impl SelfHealingTypeRules {
    /// The pulse period in minutes: `SelfHealing.Rate`, else the Rules
    /// `RepairRate`.
    pub(crate) fn rate(&self, repair_rate: f64) -> NativeF64Bits {
        self.rate
            .unwrap_or(NativeF64Bits::from_bits(repair_rate.to_bits()))
    }

    /// `SelfHealing.%sAmount` and `SelfHealing.%sMax` for `rank` (0 rookie,
    /// 1 veteran, 2 elite; Promotable::Get).
    pub(crate) fn amount_and_max(&self, rank: usize) -> (i32, NativeF64Bits) {
        let rank = rank.min(2);
        (self.amount[rank], self.max[rank])
    }

    /// `SelfHealing.CombatDelay`: the frames a damaging hit holds the
    /// pulse off (TechnoClass_ReceiveDamage_Aftermath, Ares.dll 1004AB50,
    /// hook 0x702819).
    pub(crate) fn combat_delay(&self) -> i32 {
        self.combat_delay
    }
}

/// TechnoTypeExt+0x570..+0x584, the fall step ObjectClass_Update_MaxFallRate
/// (Ares.dll 10049BA0, hook 0x5F3FB2) takes.
#[derive(Debug, Clone, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct FallRateTypeRules {
    /// `FallRate.Parachute` (+0x570) and `FallRate.NoParachute` (+0x574),
    /// both 1.
    steps: [i32; 2],
    /// `FallRate.ParachuteMax` (Nullable<int> +0x578) and
    /// `FallRate.NoParachuteMax` (+0x580), unset: the Rules caps.
    caps: [Option<i32>; 2],
}

impl Default for FallRateTypeRules {
    fn default() -> Self {
        Self {
            steps: [1; 2],
            caps: [None; 2],
        }
    }
}

impl FallRateTypeRules {
    /// The step and the cap of one frame's fall, with or without a
    /// parachute; `rules_caps` are `ParachuteMaxFallRate` and
    /// `NoParachuteMaxFallRate`.
    pub(crate) fn step(&self, parachute: bool, rules_caps: [i32; 2]) -> (i32, i32) {
        let arm = usize::from(!parachute);
        (self.steps[arm], self.caps[arm].unwrap_or(rules_caps[arm]))
    }
}

/// TechnoTypeExt+0x32D..+0x37C, read for an AircraftType only (`#245`).
#[derive(Debug, Clone, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct CustomMissileRules {
    /// `Missile.Custom` (+0x32D, false).
    custom: bool,
    /// `Missile.*` RocketStruct (+0x330, zeroed, Type = the owner type).
    /// Its warhead pair holds `Missile.Warhead` (+0x364) and
    /// `Missile.EliteWarhead` (+0x368), both null by default.
    block: MissileSpawnParams,
    /// `Missile.TakeOffAnim` (+0x36C); Initialize seeds `Find("V3TAKOFF")`.
    takeoff_anim: Option<String>,
    /// `Missile.TrailerAnim` (+0x370); Initialize seeds `Find("V3TRAIL")`.
    trailer_anim: Option<String>,
    /// `Missile.TrailerSeparation` (+0x374, 3).
    trailer_separation: i32,
    /// `Missile.Weapon` (+0x378) and `Missile.EliteWeapon` (+0x37C), null.
    weapons: [Option<String>; 2],
}

impl CustomMissileRules {
    fn new(type_name: &str) -> Self {
        Self {
            custom: false,
            block: MissileSpawnParams::ares_custom(type_name),
            takeoff_anim: None,
            trailer_anim: None,
            trailer_separation: 3,
            weapons: [None, None],
        }
    }

    /// The type's own RocketStruct when `Missile.Custom` is set: Process
    /// (hook 0x6622E0, Ares.dll 100610F0), Detonate (0x66305A, 10060FA0),
    /// Move_To (0x6632F2, 100610C0) and Draw_Matrix (0x6634F6, 10061090)
    /// use it in place of the Rules block.
    pub(crate) fn custom_block(&self) -> Option<&MissileSpawnParams> {
        self.custom.then_some(&self.block)
    }

    pub(crate) fn takeoff_anim(&self) -> Option<&str> {
        self.takeoff_anim.as_deref()
    }

    pub(crate) fn trailer_anim(&self) -> Option<&str> {
        self.trailer_anim.as_deref()
    }

    pub(crate) fn trailer_separation(&self) -> i32 {
        self.trailer_separation
    }

    /// `Missile.EliteWeapon` for an elite launcher, else `Missile.Weapon`,
    /// for a custom missile only (Explode_CustomMissile2 10060FF0).
    pub(crate) fn weapon(&self, elite: bool) -> Option<&str> {
        if !self.custom {
            return None;
        }
        self.weapons[usize::from(elite)].as_deref()
    }
}

impl AresTechnoTypeRules {
    /// One reached `LoadFromINIFile` pass. `type_name` is the type's ID,
    /// `aircraft` whether it is an AircraftType, and `resolve` the Find-only
    /// pointer read of a key (called in read order), `resolve_techno` and
    /// `resolve_techno_list` the TechnoType pointer and list reads (the
    /// list's `None` keeps the field); `find` answers `Initialize`'s
    /// fixed-name `AnimTypeClass::Find`.
    pub(crate) fn read_pass(
        &mut self,
        section: &IniSection,
        type_name: &str,
        aircraft: bool,
        mut resolve: impl FnMut(&str, TypeRef) -> Resolved,
        mut resolve_techno: impl FnMut(&str) -> ResolvedTechno,
        mut resolve_techno_list: impl FnMut(
            &str,
        ) -> Option<
            Vec<crate::rules::superweapon_type::TechnoTypeReference>,
        >,
        find: impl Fn(&str) -> Option<String>,
    ) {
        let _ares = crate::rules::ares_switch::ares_reads();
        if !self.initialized {
            self.initialized = true;
            // 1003DEC0 strcmps the stored ID; it also seeds the EVA cues
            // (other chains).
            let roles = &mut self.roles;
            roles.is_desolator = type_name == "DESO";
            roles.is_desolator_rad_dependant = roles.is_desolator;
            roles.is_cow = type_name == "COW";
            if aircraft {
                let mut missile = CustomMissileRules::new(type_name);
                missile.trailer_anim = find("V3TRAIL");
                missile.takeoff_anim = find("V3TAKOFF");
                self.missile = Some(Box::new(missile));
                self.damage.smoke_anim = find("SGRYSMK1");
            }
        }
        // After Prerequisite.RequiredTheaters: ReadHouseTypesList keeps the
        // default for an absent or empty value; any other value replaces it.
        for (key, list) in [
            (
                "SecretLab.RequiredHouses",
                &mut self.secret_lab_houses.required,
            ),
            (
                "SecretLab.ForbiddenHouses",
                &mut self.secret_lab_houses.forbidden,
            ),
        ] {
            if let Some(tokens) = section.read_list(key, 0x800) {
                *list = Some(tokens.into_iter().map(str::to_owned).collect());
            }
        }
        // After SecretLab.*.
        let roles = &mut self.roles;
        roles.is_desolator = section.read_ares_bool("IsDesolator", roles.is_desolator);
        roles.is_desolator_rad_dependant =
            section.read_ares_bool("IsDesolator.RadDependant", roles.is_desolator_rad_dependant);
        roles.is_cow = section.read_ares_bool("IsCow", roles.is_cow);
        // After the Spotlight.* keys.
        roles.is_bomb = section.read_ares_bool("IsBomb", roles.is_bomb);
        // After Operator, InitialPayload.Types (1000EDD0) and .Nums
        // (ValueableVector<int> 1000ED00), then after the Chronoshift.* and
        // CameoPCX keys the reverse-engineering pair.
        let residual = &mut self.residual;
        if let Some(types) = resolve_techno_list("InitialPayload.Types") {
            residual.initial_payload_types = types;
        }
        if let Some(nums) = section.read_ares_int_list("InitialPayload.Nums") {
            residual.initial_payload_nums = nums;
        }
        residual.can_be_reversed =
            section.read_ares_bool("CanBeReversed", residual.can_be_reversed);
        if let Some(reversed_as) = resolve_techno("ReversedAs") {
            residual.reversed_as = reversed_as;
        }
        // #1208, read after RadarJamRadius.
        self.movement.passenger_turret =
            section.read_ares_bool("PassengerTurret", self.movement.passenger_turret);
        // After ClonedAs, before ImmuneToAbduction and FactoryOwners.
        let residual = &mut self.residual;
        residual.carryall_allowed =
            section.read_ares_nullable_bool("Carryall.Allowed", residual.carryall_allowed);
        residual.carryall_size_limit =
            section.read_ares_nullable_int("Carryall.SizeLimit", residual.carryall_size_limit);
        self.gattling_cyclic = section.read_ares_bool("Gattling.Cycle", self.gattling_cyclic);
        // 1003DFB0: "#245 custom missiles", after Gattling.Cycle.
        if let Some(missile) = self.missile.as_deref_mut() {
            missile.custom = section.read_ares_bool("Missile.Custom", missile.custom);
            missile.block.read_ares(section, "Missile");
            let mut pointer = |key: &str, kind: TypeRef, field: &mut Option<String>| {
                if let Some(value) = resolve(key, kind) {
                    *field = value;
                }
            };
            let mut warhead = non_empty(&missile.block.warhead);
            pointer("Missile.Warhead", TypeRef::Warhead, &mut warhead);
            let mut elite_warhead = non_empty(&missile.block.elite_warhead);
            pointer("Missile.EliteWarhead", TypeRef::Warhead, &mut elite_warhead);
            missile.block.warhead = warhead.unwrap_or_default();
            missile.block.elite_warhead = elite_warhead.unwrap_or_default();
            pointer(
                "Missile.TakeOffAnim",
                TypeRef::Animation,
                &mut missile.takeoff_anim,
            );
            pointer(
                "Missile.TrailerAnim",
                TypeRef::Animation,
                &mut missile.trailer_anim,
            );
            missile.trailer_separation =
                section.read_ares_int("Missile.TrailerSeparation", missile.trailer_separation);
            let [weapon, elite_weapon] = &mut missile.weapons;
            pointer("Missile.Weapon", TypeRef::Weapon, weapon);
            pointer("Missile.EliteWeapon", TypeRef::Weapon, elite_weapon);
        }
        // After Missile.* and Crashable; AirRate after the cloak sounds.
        self.movement.crash_spin = section.read_ares_bool("CrashSpin", self.movement.crash_spin);
        self.movement.air_rate = section.read_ares_int("AirRate", self.movement.air_rate);
        // Right after AirRate the tiberium block, later the Harvester.* one.
        self.tiberium.read_pass(section);
        let roles = &mut self.roles;
        roles.drain_local = section.read_ares_bool("Drain.Local", roles.drain_local);
        roles.drain_amount = section.read_ares_int("Drain.Amount", roles.drain_amount);
        // After the Drain.* keys: the smoke.
        let damage = &mut self.damage;
        if let Some(anim) = resolve("Smoke.Anim", TypeRef::Animation) {
            damage.smoke_anim = anim;
        }
        damage.smoke_chance_red =
            section.read_ares_nullable_int("Smoke.ChanceRed", damage.smoke_chance_red);
        damage.smoke_chance_dead =
            section.read_ares_nullable_int("Smoke.ChanceDead", damage.smoke_chance_dead);
        // Then the hunter-seeker overrides, before HunterSeeker.Ignore
        // (`rules::superweapon_influence`).
        for (slot, key) in [
            "HunterSeeker.DetonateProximity",
            "HunterSeeker.DescendProximity",
            "HunterSeeker.AscentSpeed",
            "HunterSeeker.DescentSpeed",
            "HunterSeeker.EmergeSpeed",
        ]
        .into_iter()
        .enumerate()
        {
            self.residual.hunter_seeker[slot] =
                section.read_ares_nullable_int(key, self.residual.hunter_seeker[slot]);
        }
        // After HunterSeeker.*: CivilianEnemy, then DamageSparks; after the
        // particle-system lists, the berserk, Assaulter and crush blocks.
        let combat = &mut self.combat;
        combat.civilian_enemy = section.read_ares_bool("CivilianEnemy", combat.civilian_enemy);
        damage.damage_sparks =
            section.read_ares_nullable_bool("DamageSparks", damage.damage_sparks);
        combat.berserk_rof_multiplier = section
            .read_ares_nullable_double_bits("Berserk.ROFMultiplier", combat.berserk_rof_multiplier);
        combat.immune_to_berserk =
            section.read_ares_nullable_bool("ImmuneToBerserk", combat.immune_to_berserk);
        // After DesignatorRange/InhibitorRange.
        self.residual.assaulter_level =
            section.read_ares_int("Assaulter.Level", self.residual.assaulter_level);
        let combat = &mut self.combat;
        combat.omni_crusher_aggressive =
            section.read_ares_bool("OmniCrusher.Aggressive", combat.omni_crusher_aggressive);
        combat.crush_damage =
            read_promotable("CrushDamage.%s", combat.crush_damage, |key, value| {
                section.read_ares_nullable_int(key, value)
            });
        if let Some(warhead) = resolve("CrushDamage.Warhead", TypeRef::Warhead) {
            combat.crush_damage_warhead = Some(warhead);
        }
        let ammo = &mut self.ammo;
        ammo.reload_rate = section.read_ares_nullable_double_bits("ReloadRate", ammo.reload_rate);
        ammo.reload_amount = section.read_ares_int("ReloadAmount", ammo.reload_amount);
        ammo.empty_reload_amount =
            section.read_ares_nullable_int("EmptyReloadAmount", ammo.empty_reload_amount);
        ammo.no_ammo_amount = section.read_ares_int("NoAmmoAmount", ammo.no_ammo_amount);
        ammo.no_ammo_weapon = section.read_ares_int("NoAmmoWeapon", ammo.no_ammo_weapon);
        // After Saboteur.
        let combat = &mut self.combat;
        combat.can_passive_acquire_guard =
            section.read_ares_bool("CanPassiveAquire.Guard", combat.can_passive_acquire_guard);
        combat.can_passive_acquire_cloak =
            section.read_ares_bool("CanPassiveAquire.Cloak", combat.can_passive_acquire_cloak);
        // Then the self heal.
        let heal = &mut self.self_healing;
        heal.rate = section.read_ares_nullable_double_bits("SelfHealing.Rate", heal.rate);
        heal.amount = read_promotable("SelfHealing.%sAmount", heal.amount, |key, value| {
            section.read_ares_nullable_int(key, value)
        });
        heal.max = read_promotable("SelfHealing.%sMax", heal.max, |key, value| {
            section.read_ares_nullable_double_bits(key, value)
        });
        heal.combat_delay = section.read_ares_int("SelfHealing.CombatDelay", heal.combat_delay);
        // Then the Passengers.* filters (1000EDD0; `None` keeps the list),
        // before Passengers.BySize.
        let movement = &mut self.movement;
        if let Some(types) = resolve_techno_list("Passengers.Allowed") {
            movement.passengers_allowed = types;
        }
        if let Some(types) = resolve_techno_list("Passengers.Disallowed") {
            movement.passengers_disallowed = types;
        }
        movement.no_manual_unload =
            section.read_ares_bool("NoManualUnload", movement.no_manual_unload);
        movement.no_manual_enter =
            section.read_ares_bool("NoManualEnter", movement.no_manual_enter);
        self.residual.no_self_guard_area =
            section.read_ares_bool("NoSelfGuardArea", self.residual.no_self_guard_area);
        // Nullable<CSFText> 1003F6B0: a nonempty value sets the label.
        self.presentation.enemy_ui_name = section.read_string_with(
            "EnemyUIName",
            2048,
            self.presentation.enemy_ui_name.take(),
            |_, label| Some(crate::rules::ini_value::truncate_native_bytes(label, 31).to_owned()),
        );
        // After the Bounty.*, Promote.* and FactoryPlant keys: the tunnel
        // anims (DigInSound/DigOutSound bind with the other sounds).
        for (slot, key) in ["DigIn", "DigOut"].into_iter().enumerate() {
            if let Some(anim) = resolve(key, TypeRef::Animation) {
                self.residual.dig_anims[slot] = Some(anim);
            }
        }
        // After DigOut.
        let fall = &mut self.fall_rate;
        for (arm, prefix) in ["FallRate.Parachute", "FallRate.NoParachute"]
            .into_iter()
            .enumerate()
        {
            fall.steps[arm] = section.read_ares_int(prefix, fall.steps[arm]);
        }
        for (arm, key) in ["FallRate.ParachuteMax", "FallRate.NoParachuteMax"]
            .into_iter()
            .enumerate()
        {
            fall.caps[arm] = section.read_ares_nullable_int(key, fall.caps[arm]);
        }
        // After the BuildTime.* keys.
        if let Some(fake_of) = resolve_techno("FakeOf") {
            self.residual.fake_of = fake_of;
        }
        // After FakeOf: an invalid or out-of-range facing keeps the value.
        self.movement.deploy_dir = section.read_string_with(
            "DeployDir",
            2048,
            self.movement.deploy_dir,
            |current, value| {
                crate::rules::ini_value::parse_ares_int(value)
                    .filter(|facing| (0..8).contains(facing))
                    .map(|facing| facing as u8)
                    .or(current)
            },
        );
        // Then Convert.Deploy, ahead of Convert.Water/Land/Script.
        if let Some(convert_deploy) = resolve_techno("Convert.Deploy") {
            self.residual.convert_deploy = convert_deploy;
        }
        // After Harvester.KickDelay, then KeepAlive (`sim::house_tracking`).
        self.residual.unsellable =
            section.read_ares_nullable_bool("Unsellable", self.residual.unsellable);
        // Last, after Unsellable and KeepAlive.
        self.presentation.radial_indicator_radius = section.read_ares_nullable_int(
            "RadialIndicatorRadius",
            self.presentation.radial_indicator_radius,
        );
    }

    /// The type's sounds in TechnoTypeExt (NullableIdx<VocClass> 1000E9D0
    /// against the fixed catalog, unknown names retain): the airstrike
    /// voices, read after VoiceIFVRepair, and the tunnel sounds, after
    /// FactoryPlant.Multiplier.
    #[cfg(feature = "ares")]
    pub(crate) fn bind_sound_references(
        &mut self,
        section: Option<&IniSection>,
        sounds: &crate::rules::sound_ini::SoundRegistry,
    ) {
        let _ares = crate::rules::ares_switch::ares_reads();
        let read = |key| section.and_then(|section| sounds.read_ares_reference(section, key));
        self.presentation.airstrike_voices =
            ["VoiceAirstrikeAttack", "VoiceAirstrikeAbort"].map(read);
        self.residual.dig_sounds = ["DigInSound", "DigOutSound"].map(read);
    }

    /// `EVA.Lost` (ValueableIdx<VoxClass>), read after SensorArray.Warn
    /// from the type's Initialize seed.
    #[cfg(feature = "ares")]
    pub(crate) fn bind_unit_lost_eva(&mut self, eva: Option<String>) {
        self.residual.unit_lost_eva = eva;
        self.residual.unit_lost_eva_bound = true;
    }

    /// `EVA.Lost`, the line FootClass_Destroyed (Ares.dll 10049080, hook
    /// 0x4D98C0) and BuildingClass_Destroyed_UnitLost (10013F10, hook
    /// 0x44D760) play: `Some(None)` is the silent -1, `None` rules never
    /// bound against the EVA catalog.
    #[cfg(feature = "ares")]
    pub(crate) fn unit_lost_eva(&self) -> Option<Option<&str>> {
        self.residual
            .unit_lost_eva_bound
            .then(|| self.residual.unit_lost_eva.as_deref())
    }

    /// `PlayDigEffects` (Ares.dll `0x100443E0`): the sound and anim of a
    /// dig-in or dig-out, `DigInSound`/`DigIn` or `DigOutSound`/`DigOut`
    /// (`+0x550 + (digIn ^ 1) * 8`, `+0x560 + (digIn ^ 1) * 8`). `None` is
    /// unset, which plays the `[AudioVisual]` value; `Some(None)` is an
    /// explicit none.
    #[cfg(feature = "ares")]
    pub(crate) fn dig_effects(&self, dig_in: bool) -> [Option<Option<&str>>; 2] {
        let slot = usize::from(!dig_in);
        [
            self.residual.dig_sounds[slot]
                .as_ref()
                .map(|sound| sound.as_deref()),
            self.residual.dig_anims[slot]
                .as_ref()
                .map(|anim| anim.as_deref()),
        ]
    }

    #[cfg(feature = "ares")]
    pub(crate) fn presentation(&self) -> &PresentationTypeRules {
        &self.presentation
    }

    /// The AircraftType's missile fields; `None` for other types.
    pub(crate) fn missile(&self) -> Option<&CustomMissileRules> {
        self.missile.as_deref()
    }

    pub(crate) fn ammo(&self) -> &AmmoTypeRules {
        &self.ammo
    }

    pub(crate) fn combat(&self) -> &CombatTypeRules {
        &self.combat
    }

    pub(crate) fn damage(&self) -> &DamagePresentationRules {
        &self.damage
    }

    pub(crate) fn movement(&self) -> &MovementTypeRules {
        &self.movement
    }

    pub(crate) fn self_healing(&self) -> &SelfHealingTypeRules {
        &self.self_healing
    }

    pub(crate) fn fall_rate(&self) -> &FallRateTypeRules {
        &self.fall_rate
    }

    pub(crate) fn roles(&self) -> &TypeRoleRules {
        &self.roles
    }

    /// `SecretLab.RequiredHouses` and `SecretLab.ForbiddenHouses`.
    pub(crate) fn secret_lab_houses(&self) -> &SecretLabHouseRules {
        &self.secret_lab_houses
    }

    /// `Prerequisite.List#`, `.Negative`, `.RequiredTheaters` and
    /// `.StolenTechs`.
    pub(crate) fn prerequisites(&self) -> &AresPrerequisiteRules {
        &self.prerequisites
    }

    /// The Rules processor's handle for [`AresPrerequisiteRules::read_pass`],
    /// which needs the type's list 0 and the registry's Parse100085F0.
    pub(crate) fn prerequisites_mut(&mut self) -> &mut AresPrerequisiteRules {
        &mut self.prerequisites
    }

    pub(crate) fn tiberium(&self) -> &crate::rules::ares_tiberium::TiberiumTechnoRules {
        &self.tiberium
    }

    /// `CanBeReversed` and `ReversedAs`: whether a victim of this type is
    /// reverse engineered at all, and as which type (the type itself when
    /// unset), HouseExt::ExtData::ReverseEngineer (Ares.dll 10022360).
    pub(crate) fn reverse_engineering(
        &self,
    ) -> (
        bool,
        Option<&crate::rules::superweapon_type::TechnoTypeReference>,
    ) {
        (
            self.residual.can_be_reversed,
            self.residual.reversed_as.as_ref(),
        )
    }

    /// `Carryall.Allowed` (+0x2F8): whether a Carryall may lift a Unit of
    /// this type, when set.
    #[cfg(feature = "ares")]
    pub(crate) fn carryall_allowed(&self) -> Option<bool> {
        self.residual.carryall_allowed
    }

    /// `Carryall.SizeLimit` (+0x2FC): the largest `Size=` this Carryall
    /// lifts, when set.
    #[cfg(feature = "ares")]
    pub(crate) fn carryall_size_limit(&self) -> Option<i32> {
        self.residual.carryall_size_limit
    }

    /// `Assaulter.Level` (+0x42C): an assaulter takes a building whose type's
    /// level is not negative and at most his own
    /// (BuildingClass_CanBeOccupied_SpecificAssaulters, Ares.dll 10016FD0,
    /// hook 0x457DB7; `passenger::can_dock_occupier_garrison`).
    #[cfg(feature = "ares")]
    pub(crate) fn assaulter_level(&self) -> i32 {
        self.residual.assaulter_level
    }

    /// `Unsellable.Get(UnitsUnsellable)`: a unit of this type cannot be sold
    /// on a depot (FootClass_CanBeSold_SellUnit, Ares.dll 10048FD0, hook
    /// 0x4D9EBD).
    pub(crate) fn unsellable(&self, units_unsellable: bool) -> bool {
        self.residual.unsellable.unwrap_or(units_unsellable)
    }

    /// `HunterSeeker.AscentSpeed`, `.DescentSpeed` and `.EmergeSpeed`,
    /// each `.Get(RulesExt)` (the Fly height step's hooks 0x4CDE64 and
    /// 0x4CDF54, Ares.dll 1004D930 and 1004DA20).
    #[cfg(feature = "ares")]
    pub(crate) fn hunter_seeker_climb(
        &self,
        general: &crate::rules::native_processing::AresGeneralRules,
    ) -> HunterSeekerClimb {
        let [_, _, ascent, descent, emerge] = general.hunter_seeker_speeds();
        let own = &self.residual.hunter_seeker;
        HunterSeekerClimb {
            ascent: own[2].unwrap_or(ascent),
            descent: own[3].unwrap_or(descent),
            emerge: own[4].unwrap_or(emerge),
        }
    }

    /// `HunterSeeker.DetonateProximity` and `.DescendProximity`, each
    /// `.Get(RulesExt)` (Horizontal_Step's hook 0x4CF3D0, Ares.dll 1004DC30).
    #[cfg(feature = "ares")]
    pub(crate) fn hunter_seeker_proximities(
        &self,
        general: &crate::rules::native_processing::AresGeneralRules,
    ) -> (i32, i32) {
        let [detonate, descend, ..] = general.hunter_seeker_speeds();
        let own = &self.residual.hunter_seeker;
        (own[0].unwrap_or(detonate), own[1].unwrap_or(descend))
    }

    /// `Gattling.Cycle`: a charge at the cap restarts the stages
    /// (TechnoClass_UpdateGattling_Cycle, Ares.dll 1004B6C0, hook 0x70DEBA).
    pub(crate) fn gattling_cyclic(&self) -> bool {
        self.gattling_cyclic
    }

    /// `InitialPayload.Types` and `InitialPayload.Nums`: the passengers or
    /// occupants an object of this type is created with
    /// (TechnoExt::ExtData::CreateInitialPayload, Ares.dll 10043C10).
    #[cfg(feature = "ares")]
    pub(crate) fn initial_payload(
        &self,
    ) -> (
        &[crate::rules::superweapon_type::TechnoTypeReference],
        &[i32],
    ) {
        (
            &self.residual.initial_payload_types,
            &self.residual.initial_payload_nums,
        )
    }

    /// `Convert.Deploy`: the type a deployed simple deployer converts to
    /// (UnitClass_Mi_Unload_SimpleDeployer, Ares.dll 1004C070, hook
    /// 0x73DE90).
    #[cfg(feature = "ares")]
    pub(crate) fn convert_deploy(
        &self,
    ) -> Option<&crate::rules::superweapon_type::TechnoTypeReference> {
        self.residual.convert_deploy.as_ref()
    }
}

fn non_empty(name: &str) -> Option<String> {
    (!name.is_empty()).then(|| name.to_owned())
}

/// Ares `Promotable<T>::Read` (TemplateDef.h): `format` with "" and a
/// trailing dot stripped is the common key, which sets all three ranks when
/// it parses; then the `Rookie`, `Veteran` and `Elite` keys each override
/// one. `read` is the field's Valueable read (`None` in: no current value);
/// failures retain.
fn read_promotable<T: Copy>(
    format: &str,
    current: [T; 3],
    read: impl Fn(&str, Option<T>) -> Option<T>,
) -> [T; 3] {
    let common = format.replace("%s", "");
    let common = common.strip_suffix('.').unwrap_or(&common);
    let mut value = read(common, None).map_or(current, |all| [all; 3]);
    for (slot, rank) in ["Rookie", "Veteran", "Elite"].into_iter().enumerate() {
        let key = format.replace("%s", rank);
        if let Some(read) = read(&key, Some(value[slot])) {
            value[slot] = read;
        }
    }
    value
}

#[cfg(test)]
mod tests {
    use crate::rules::ini_parser::IniFile;
    use crate::rules::native_processing::RulesLayerStack;
    use crate::rules::ruleset::RuleSet;

    fn rules(extra: &str) -> RuleSet {
        let text = format!(
            "[AircraftTypes]\n0=MYMISL\n1=PLAIN\n[Animations]\n0=V3TRAIL\n1=MYPUFF\n\
             [Warheads]\n0=MYWH\n[MYMISL]\nPrimary=MYGUN\n{extra}\n[PLAIN]\nStrength=1\n\
             [MYGUN]\nDamage=50\nWarhead=MYWH\n"
        );
        let processed = RulesLayerStack::new(IniFile::from_str(&text))
            .process()
            .unwrap();
        RuleSet::from_processed_rules(&processed).unwrap()
    }

    /// Initialize's Find-only seeds and the 1003C710 defaults: without
    /// `Missile.Custom` the Rules block flies, V3TRAIL is found and an
    /// unregistered V3TAKOFF leaves the takeoff puff null.
    #[test]
    fn aircraft_types_seed_their_puffs_and_keep_the_rules_block() {
        let rules = rules("");
        let ext = rules.rocket_type_ext("PLAIN");
        assert_eq!(ext.trailer_anim, Some("V3TRAIL"));
        assert_eq!(ext.takeoff_anim, None);
        assert_eq!((ext.trailer_separation, ext.weapons), (3, [None, None]));
        assert!(std::ptr::eq(
            rules.rocket_block("PLAIN").0,
            rules.missile_spawn.rocket_block("PLAIN").0,
        ));
        assert_eq!(rules.spawn_missile_wait_frames("PLAIN"), None);
    }

    /// `Missile.Custom` makes the type fly, wait and detonate by its own
    /// zero-seeded block (TechnoTypeExt+0x330) with `Missile.*` pointers
    /// read Find-only: an unknown name keeps the null default.
    #[test]
    fn custom_missile_reads_its_block_warheads_and_weapons() {
        let rules = rules(
            "Missile.Custom=yes\nMissile.PauseFrames=5\nMissile.TiltFrames=40\n\
             Missile.PitchInitial=0.25\nMissile.RaiseRate=2.9\nMissile.Damage=300\n\
             Missile.LazyCurve=yes\nMissile.Warhead=MYWH\nMissile.EliteWarhead=UNKNOWN\n\
             Missile.TrailerAnim=none\nMissile.TakeOffAnim=MYPUFF\n\
             Missile.TrailerSeparation=7\nMissile.EliteWeapon=MYGUN\n",
        );
        let (block, cmisl) = rules.rocket_block("MYMISL");
        assert!(!cmisl);
        assert_eq!(block.type_name, "MYMISL");
        assert_eq!((block.pause_frames, block.tilt_frames), (5, 40));
        assert_eq!(block.pitch_initial.bits(), 0x3E80_0000);
        assert_eq!(
            (block.raise_rate, block.damage, block.elite_damage),
            (2, 300, 0)
        );
        assert_eq!((block.altitude, block.body_length), (0, 0));
        assert!(block.lazy_curve);
        assert_eq!(block.warhead_for(false), "MYWH");
        assert_eq!(block.warhead_for(true), "");
        let ext = rules.rocket_type_ext("MYMISL");
        assert_eq!(ext.trailer_anim, None);
        assert_eq!(ext.takeoff_anim, Some("MYPUFF"));
        assert_eq!(ext.trailer_separation, 7);
        assert_eq!(ext.weapons, [None, Some("MYGUN")]);
        assert_eq!(rules.spawn_missile_wait_frames("MYMISL"), Some(45));
    }

    /// Initialize (1003DEC0) seeds IsDesolator and its RadDependant from the
    /// stored ID `DESO` and IsCow from `COW`; the keys then
    /// override, and the 1003C710 defaults hold the rest.
    #[cfg(feature = "ares")]
    #[test]
    fn initialize_seeds_the_desolator_and_cow_roles_by_id() {
        use crate::util::native_x87::NativeF64Bits;
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n0=DESO\n1=COW\n2=E1\n[DESO]\nStrength=1\n\
             [E1]\nIsDesolator=yes\nIsBomb=yes\nDrain.Local=yes\nDrain.Amount=-5\n\
             [COW]\nIsCow=no\n",
        ))
        .unwrap();
        let roles = |name: &str| rules.object(name).unwrap().techno_combat().roles().clone();
        let deso = roles("DESO");
        assert!(deso.is_desolator() && deso.is_desolator_rad_dependant && !deso.is_cow());
        assert!(!roles("COW").is_cow());
        let e1 = roles("E1");
        assert!(e1.is_desolator() && !e1.is_desolator_rad_dependant && e1.is_bomb());
        assert_eq!(e1.drain(), (true, -5));
        let heal = rules
            .object("E1")
            .unwrap()
            .techno_combat()
            .self_healing()
            .clone();
        assert_eq!(heal.amount_and_max(2), (1, NativeF64Bits::ONE));
    }

    /// 1003DFB0's reads of the keys without a consumer and of the
    /// presentation keys, with the 1003C710 defaults on a type that sets
    /// none: pointers are Find only (an unknown name keeps null), the CSF
    /// label keeps 31 bytes, and the sounds and `EVA.Lost` bind against the
    /// fixed catalogs, a building keeping its unset `EVA.Lost`.
    #[cfg(feature = "ares")]
    #[test]
    fn residual_and_presentation_keys_read_with_constructor_defaults() {
        use crate::rules::sound_ini::{EvaRegistry, SoundRegistry};
        let ini = IniFile::from_str(
            "[InfantryTypes]\n0=E1\n1=E2\n[BuildingTypes]\n0=POWR\n[Animations]\n0=DIG\n\
             [E1]\nInitialPayload.Types=E2,Missing,E1\nInitialPayload.Nums=1,x,3\n\
             CanBeReversed=no\nReversedAs=E2\n\
             Carryall.Allowed=yes\nCarryall.SizeLimit=2\nHunterSeeker.AscentSpeed=7\n\
             HunterSeeker.EmergeSpeed=bad\nAssaulter.Level=2\nNoSelfGuardArea=yes\n\
             EnemyUIName=NAME:0123456789012345678901234567890123\nDigIn=DIG\nDigOut=none\n\
             FakeOf=E2\nConvert.Deploy=Missing\nRadialIndicatorRadius=5\n\
             VoiceAirstrikeAttack=Hit\nVoiceAirstrikeAbort=none\nDigInSound=Missing\n\
             EVA.Lost=none\n[E2]\nStrength=1\n[POWR]\nStrength=1\n",
        );
        let mut rules = RuleSet::from_ini(&ini).unwrap();
        let sounds = SoundRegistry::from_ini(&IniFile::from_str("[SoundList]\n0=Hit\n"));
        rules.bind_type_sound_references(&ini, &sounds);
        let eva = EvaRegistry::from_ini(&IniFile::from_str(
            "[DialogList]\n0=EVA_UnitLost\n[EVA_UnitLost]\nAllied=LOST\n",
        ));
        rules.bind_type_eva_references(&ini, &eva);
        let ext = |name: &str| rules.object(name).unwrap().techno_combat().clone();
        let id = |reference: &Option<crate::rules::superweapon_type::TechnoTypeReference>| {
            reference.as_ref().map(|reference| reference.id.clone())
        };
        let e1 = ext("E1");
        let residual = &e1.residual;
        assert_eq!(residual.initial_payload_nums, [1, 3]);
        // 1000EDD0 drops the name no TechnoType holds.
        let payload: Vec<_> = residual
            .initial_payload_types
            .iter()
            .map(|reference| reference.id.as_str())
            .collect();
        assert_eq!(payload, ["E2", "E1"]);
        assert!(!residual.can_be_reversed);
        assert_eq!(id(&residual.reversed_as).as_deref(), Some("E2"));
        assert_eq!(
            (residual.carryall_allowed, residual.carryall_size_limit),
            (Some(true), Some(2))
        );
        assert_eq!(residual.hunter_seeker, [None, None, Some(7), None, None]);
        assert_eq!(residual.assaulter_level, 2);
        assert!(residual.no_self_guard_area);
        assert_eq!(residual.dig_anims, [Some(Some("DIG".into())), Some(None)]);
        assert_eq!(residual.dig_sounds, [None, None]);
        assert_eq!(id(&residual.fake_of).as_deref(), Some("E2"));
        assert_eq!(residual.convert_deploy, None);
        assert_eq!(residual.unit_lost_eva, None);
        let presentation = e1.presentation();
        assert_eq!(
            presentation.enemy_ui_name(),
            Some("NAME:01234567890123456789012345")
        );
        assert_eq!(presentation.radial_indicator_radius(), Some(5));
        assert_eq!(presentation.airstrike_voice(false), Some(Some("Hit")));
        assert_eq!(presentation.airstrike_voice(true), Some(None));
        let e2 = ext("E2");
        let residual = &e2.residual;
        assert!(residual.can_be_reversed && residual.initial_payload_nums.is_empty());
        assert!(residual.initial_payload_types.is_empty());
        assert_eq!(residual.reversed_as, None);
        assert_eq!(residual.hunter_seeker, [None; 5]);
        assert_eq!(
            (residual.assaulter_level, residual.no_self_guard_area),
            (0, false)
        );
        assert_eq!(residual.dig_anims, [None, None]);
        assert_eq!(residual.unit_lost_eva.as_deref(), Some("EVA_UnitLost"));
        assert_eq!(e2.presentation().enemy_ui_name(), None);
        assert_eq!(e2.presentation().radial_indicator_radius(), None);
        assert_eq!(e2.presentation().airstrike_voice(false), None);
        assert_eq!(ext("POWR").residual.unit_lost_eva, None);
    }
}
