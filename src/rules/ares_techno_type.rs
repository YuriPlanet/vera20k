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
}

/// TechnoTypeExt+0x27C `PassengerTurret` (false), +0x325 `CrashSpin`
/// (true), +0x328 `AirRate` (0), +0x4C5 `NoManualUnload` and +0x4C7
/// `NoManualEnter` (false), +0x604 `DeployDir` (Nullable, unset).
#[derive(Debug, Clone, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct MovementTypeRules {
    passenger_turret: bool,
    crash_spin: bool,
    air_rate: i32,
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

/// TechnoTypeExt+0x458..+0x468, the reload amounts and the out-of-ammo
/// weapon.
#[derive(Debug, Clone, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct AmmoTypeRules {
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
            reload_amount: 1,
            empty_reload_amount: None,
            no_ammo_amount: 0,
            no_ammo_weapon: -1,
        }
    }
}

impl AmmoTypeRules {
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
    /// pointer read of a key (called in read order); `find` answers
    /// `Initialize`'s fixed-name `AnimTypeClass::Find`.
    pub(crate) fn read_pass(
        &mut self,
        section: &IniSection,
        type_name: &str,
        aircraft: bool,
        mut resolve: impl FnMut(&str, TypeRef) -> Resolved,
        find: impl Fn(&str) -> Option<String>,
    ) {
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
        // After SecretLab.*.
        let roles = &mut self.roles;
        roles.is_desolator = section.read_ares_bool("IsDesolator", roles.is_desolator);
        roles.is_desolator_rad_dependant =
            section.read_ares_bool("IsDesolator.RadDependant", roles.is_desolator_rad_dependant);
        roles.is_cow = section.read_ares_bool("IsCow", roles.is_cow);
        // After the Spotlight.* keys.
        roles.is_bomb = section.read_ares_bool("IsBomb", roles.is_bomb);
        // #1208, read after RadarJamRadius.
        self.movement.passenger_turret =
            section.read_ares_bool("PassengerTurret", self.movement.passenger_turret);
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
        combat.omni_crusher_aggressive =
            section.read_ares_bool("OmniCrusher.Aggressive", combat.omni_crusher_aggressive);
        combat.crush_damage =
            read_promotable("CrushDamage.%s", combat.crush_damage, |key, value| {
                section.read_ares_nullable_int(key, value)
            });
        if let Some(warhead) = resolve("CrushDamage.Warhead", TypeRef::Warhead) {
            combat.crush_damage_warhead = Some(warhead);
        }
        // After ReloadRate.
        let ammo = &mut self.ammo;
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
        // After the Passengers.* filters.
        let movement = &mut self.movement;
        movement.no_manual_unload =
            section.read_ares_bool("NoManualUnload", movement.no_manual_unload);
        movement.no_manual_enter =
            section.read_ares_bool("NoManualEnter", movement.no_manual_enter);
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

    pub(crate) fn tiberium(&self) -> &crate::rules::ares_tiberium::TiberiumTechnoRules {
        &self.tiberium
    }

    /// `Gattling.Cycle`: a charge at the cap restarts the stages
    /// (TechnoClass_UpdateGattling_Cycle, Ares.dll 1004B6C0, hook 0x70DEBA).
    pub(crate) fn gattling_cyclic(&self) -> bool {
        self.gattling_cyclic
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
}
