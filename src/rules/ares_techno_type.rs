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
            if aircraft {
                let mut missile = CustomMissileRules::new(type_name);
                missile.trailer_anim = find("V3TRAIL");
                missile.takeoff_anim = find("V3TAKOFF");
                self.missile = Some(Box::new(missile));
                self.damage.smoke_anim = find("SGRYSMK1");
            }
        }
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
        combat.omni_crusher_aggressive =
            section.read_ares_bool("OmniCrusher.Aggressive", combat.omni_crusher_aggressive);
        // Promotable<int>::Read: the common `CrushDamage` to every rank,
        // then `CrushDamage.Rookie/.Veteran/.Elite`.
        if let Some(all) = section.read_ares_nullable_int("CrushDamage", None) {
            combat.crush_damage = [all; 3];
        }
        for (rank, key) in [
            "CrushDamage.Rookie",
            "CrushDamage.Veteran",
            "CrushDamage.Elite",
        ]
        .into_iter()
        .enumerate()
        {
            combat.crush_damage[rank] = section.read_ares_int(key, combat.crush_damage[rank]);
        }
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

    /// `Gattling.Cycle`: a charge at the cap restarts the stages
    /// (TechnoClass_UpdateGattling_Cycle, Ares.dll 1004B6C0, hook 0x70DEBA).
    pub(crate) fn gattling_cyclic(&self) -> bool {
        self.gattling_cyclic
    }
}

fn non_empty(name: &str) -> Option<String> {
    (!name.is_empty()).then(|| name.to_owned())
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
}
